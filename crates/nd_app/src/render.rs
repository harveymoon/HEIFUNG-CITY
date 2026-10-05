//! Turns simulation state into Bevy entities. Static geometry (terrain,
//! streets, lots, zones, project preview) is rebuilt as whole meshes when it
//! changes; buildings get one entity per surface they use; citizens and cars
//! are small meshes moved every frame.
//!
//! Every [`Surface`] has a material with a procedurally generated, seamlessly
//! tiling colour texture and normal map. Meshes carry world-scale UVs, so the
//! same brick texture lines up across a whole facade.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use nd_procgen::street::{generate_street, signal_posts, StreetInput};
use nd_procgen::{build_terrain, generate_building, generate_texture, mip_chain, Geometry, MeshData, Surface, TerrainConfig};
use nd_sim::roads::{LANE_WIDTH, SHOULDER};
use nd_sim::traffic::LaneId;
use nd_sim::{BuildingId, CarId, CitizenId, SimEvent, Vec2 as SVec2};
use std::collections::HashMap;

use crate::assets::GameAssets;
use crate::sim::{Sim, SimSet};
use crate::tools::Selection;

#[derive(Component)]
struct SunTag;
#[derive(Component)]
struct TerrainTag;
#[derive(Component)]
struct RoadsTag;
#[derive(Component)]
struct LotsTag;
#[derive(Component)]
struct PreviewTag;
#[derive(Component)]
struct SignalTag(LaneId);
#[derive(Component)]
struct OverlayTag;

/// Toggleable data layers drawn over the ground.
#[derive(Resource, Default)]
pub struct Overlays {
    pub land_value: bool,
}

/// Sun and sky control. By default the lighting follows the sim clock; the
/// player can pin the time of day instead.
#[derive(Resource)]
pub struct Lighting {
    pub follow_clock: bool,
    /// Hour of day used when not following the clock, 0..24.
    pub hour: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self { follow_clock: true, hour: 14.0 }
    }
}
#[derive(Component, Clone)]
pub struct BuildingTag(pub BuildingId);
#[derive(Component)]
pub struct CitizenTag(pub CitizenId);
#[derive(Component)]
pub struct CarTag(pub CarId);

#[derive(Resource)]
struct Materials {
    /// One material per [`Surface`], indexed by `Surface::index()`.
    surfaces: Vec<Handle<StandardMaterial>>,
    citizen: Handle<StandardMaterial>,
    citizen_spotlight: Handle<StandardMaterial>,
    citizen_selected: Handle<StandardMaterial>,
    citizen_mesh: Handle<Mesh>,
    bike_mesh: Handle<Mesh>,
    car_bodies: Vec<Handle<StandardMaterial>>,
    car_body_mesh: Handle<Mesh>,
    truck_body_mesh: Handle<Mesh>,
    car_lights_mesh: Handle<Mesh>,
    signal_green: Handle<StandardMaterial>,
    signal_red: Handle<StandardMaterial>,
    signal_mesh: Handle<Mesh>,
    overlay: Handle<StandardMaterial>,
}

impl Materials {
    fn surface(&self, s: Surface) -> Handle<StandardMaterial> {
        self.surfaces[s.index()].clone()
    }
    fn glow(&self) -> Handle<StandardMaterial> {
        self.surface(Surface::Glow)
    }
}

#[derive(Resource, Default)]
struct RenderState {
    roads_version: u64,
    lanes_version: u64,
    plan_version: u64,
    lots_signature: (usize, usize, usize, u64),
    buildings: HashMap<BuildingId, Vec<Entity>>,
    citizens: HashMap<CitizenId, Entity>,
    cars: HashMap<CarId, [Entity; 2]>,
    overlay_version: u64,
    overlay_shown: bool,
}

/// Renderer statistics shown in the UI.
#[derive(Resource, Default)]
pub struct RenderStats {
    pub building_entities: usize,
    pub street_triangles: usize,
    pub building_triangles: usize,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderState>().init_resource::<Overlays>().init_resource::<Lighting>().init_resource::<RenderStats>().add_systems(Startup, setup).add_systems(
            Update,
            (sync_terrain, sync_roads, sync_signals, sync_preview, sync_lots, sync_overlay, sync_buildings, sync_citizens, sync_cars, update_daylight)
                .chain()
                .in_set(SimSet::Sync),
        );
    }
}

/// Convert mesh data; lit meshes get tangents so normal maps work.
pub fn to_mesh(data: &MeshData, tangents: bool) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone())
        .with_inserted_indices(Indices::U32(data.indices.clone()));
    if tangents && !data.indices.is_empty() {
        let _ = mesh.generate_tangents();
    }
    mesh
}

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn scale(c: [f32; 3], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

/// Spawn one entity per non-empty surface of a geometry.
fn spawn_geometry<T: Component + Clone>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, geo: &Geometry, tag: T) -> Vec<Entity> {
    let mut out = Vec::new();
    for (s, data) in geo.iter() {
        let mesh = to_mesh(data, !s.is_glow());
        let e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, tag.clone())).id();
        out.push(e);
    }
    out
}

fn tiling_image(size: usize, rgba: &[u8], srgb: bool) -> Image {
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut img = Image::new(
        Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba.to_vec(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    let (data, levels) = mip_chain(size, rgba);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, assets: Res<GameAssets>) {
    let pal = &assets.palette;
    let unlit = |c: [f32; 3]| StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), unlit: true, ..default() };

    // Surface materials with generated textures.
    const TEX: usize = 256;
    let mut surfaces = Vec::with_capacity(Surface::ALL.len());
    for s in Surface::ALL {
        let mat = match generate_texture(s, TEX) {
            Some(tex) => {
                let color = images.add(tiling_image(TEX, &tex.color, true));
                let normal = images.add(tiling_image(TEX, &tex.normal, false));
                StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(color),
                    normal_map_texture: Some(normal),
                    perceptual_roughness: s.roughness(),
                    metallic: s.metallic(),
                    reflectance: if s == Surface::Glass { 0.6 } else { 0.4 },
                    uv_transform: Affine2::from_scale(Vec2::splat(1.0 / s.tile())),
                    ..default()
                }
            }
            None => StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() },
        };
        surfaces.push(materials.add(mat));
    }

    let citizen = materials.add(unlit(pal.citizen));
    let citizen_spotlight = materials.add(unlit(pal.citizen_spotlight));
    let citizen_selected = materials.add(unlit([8.0, 8.0, 8.0]));
    let citizen_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.34, 0.84, 0.28), 0.0, [0.45, 0.45, 0.5, 1.0]);
        m.boxed(Vec3::new(0.0, 1.14, 0.0), Vec3::new(0.5, 0.62, 0.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(0.0, 1.62, 0.0), Vec3::new(0.25, 0.26, 0.25), 0.0, [1.3, 1.2, 1.1, 1.0]);
        meshes.add(to_mesh(&m, false))
    };
    let bike_mesh = {
        let mut m = MeshData::default();
        // wheels, frame, seated rider
        for x in [-0.55f32, 0.55] {
            m.boxed(Vec3::new(x, 0.34, 0.0), Vec3::new(0.68, 0.68, 0.06), 0.0, [0.1, 0.1, 0.1, 1.0]);
        }
        m.boxed(Vec3::new(0.0, 0.6, 0.0), Vec3::new(1.1, 0.06, 0.06), 0.0, [0.6, 0.6, 0.65, 1.0]);
        m.boxed(Vec3::new(0.35, 0.95, 0.0), Vec3::new(0.06, 0.6, 0.4), 0.0, [0.6, 0.6, 0.65, 1.0]);
        m.boxed(Vec3::new(-0.15, 0.95, 0.0), Vec3::new(0.4, 0.5, 0.26), 0.0, [0.45, 0.45, 0.5, 1.0]);
        m.boxed(Vec3::new(-0.2, 1.4, 0.0), Vec3::new(0.45, 0.55, 0.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(-0.15, 1.82, 0.0), Vec3::new(0.25, 0.26, 0.25), 0.0, [1.3, 1.2, 1.1, 1.0]);
        meshes.add(to_mesh(&m, false))
    };
    let car_bodies = pal
        .car_colors
        .iter()
        .map(|&c| materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), perceptual_roughness: 0.35, metallic: 0.6, ..default() }))
        .collect();
    let wheels = |m: &mut MeshData, xs: &[f32], z: f32, r: f32| {
        for &x in xs {
            for s in [-1.0f32, 1.0] {
                m.boxed(Vec3::new(x, r, s * z), Vec3::new(r * 2.0, r * 2.0, 0.26), 0.0, [0.08, 0.08, 0.08, 1.0]);
            }
        }
    };
    let car_body_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.75, 0.0), Vec3::new(4.2, 0.8, 1.85), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(-0.3, 1.45, 0.0), Vec3::new(2.2, 0.6, 1.6), 0.0, [0.25, 0.28, 0.32, 1.0]);
        wheels(&mut m, &[-1.35, 1.35], 0.86, 0.34);
        meshes.add(to_mesh(&m, true))
    };
    let truck_body_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(-0.9, 1.75, 0.0), Vec3::new(5.6, 2.5, 2.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(3.0, 1.4, 0.0), Vec3::new(2.0, 1.8, 2.2), 0.0, [0.5, 0.5, 0.55, 1.0]);
        m.boxed(Vec3::new(3.9, 1.8, 0.0), Vec3::new(0.2, 0.7, 2.0), 0.0, [0.2, 0.25, 0.3, 1.0]);
        wheels(&mut m, &[-2.6, -1.4, 2.6], 1.05, 0.5);
        meshes.add(to_mesh(&m, true))
    };
    let car_lights_mesh = {
        let mut m = MeshData::default();
        for z in [-0.6, 0.6] {
            m.boxed(Vec3::new(2.1, 0.7, z), Vec3::new(0.15, 0.3, 0.4), 0.0, rgba(pal.headlight));
            m.boxed(Vec3::new(-2.1, 0.75, z), Vec3::new(0.15, 0.25, 0.4), 0.0, rgba(pal.taillight));
        }
        meshes.add(to_mesh(&m, false))
    };
    let overlay = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let signal_green = materials.add(unlit(pal.signal_green));
    let signal_red = materials.add(unlit(pal.signal_red));
    let signal_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::ZERO, Vec3::new(0.08, 0.26, 0.26), 0.0, [1.0, 1.0, 1.0, 1.0]);
        meshes.add(to_mesh(&m, false))
    };
    commands.insert_resource(Materials {
        surfaces,
        citizen,
        citizen_spotlight,
        citizen_selected,
        citizen_mesh,
        bike_mesh,
        car_bodies,
        car_body_mesh,
        truck_body_mesh,
        car_lights_mesh,
        signal_green,
        signal_red,
        signal_mesh,
        overlay,
    });

    commands.insert_resource(GlobalAmbientLight { color: Color::linear_rgb(0.35, 0.45, 0.9), brightness: 70.0, ..default() });
    commands.spawn((
        DirectionalLight { color: Color::linear_rgb(0.45, 0.55, 1.0), illuminance: 700.0, shadow_maps_enabled: true, ..default() },
        CascadeShadowConfigBuilder { num_cascades: 3, minimum_distance: 0.5, maximum_distance: 650.0, first_cascade_far_bound: 45.0, ..default() }.build(),
        Transform::from_xyz(300.0, 500.0, -200.0).looking_at(Vec3::ZERO, Vec3::Y),
        SunTag,
    ));
}

fn despawn_all<T: Component>(commands: &mut Commands, q: &Query<Entity, With<T>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn sync_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    existing: Query<Entity, With<TerrainTag>>,
) {
    if !existing.is_empty() && !assets.palette_changed {
        return;
    }
    despawn_all(&mut commands, &existing);
    let data = build_terrain(&TerrainConfig::default(), &assets.palette);
    commands.spawn((Mesh3d(meshes.add(to_mesh(&data, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, TerrainTag));
}

fn sync_roads(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut stats: ResMut<RenderStats>,
    existing: Query<Entity, With<RoadsTag>>,
) {
    let version = sim.world.roads.version;
    if version == state.roads_version && !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.roads_version = version;
    despawn_all(&mut commands, &existing);
    let w = &sim.world;
    let geo = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &assets.palette, seed: w.config.seed });
    stats.street_triangles = geo.triangle_count();
    for (s, data) in geo.iter() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(data, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, RoadsTag));
    }
}

/// A glowing lamp in every signal head, coloured by whether that lane
/// currently has a green connector.
fn sync_signals(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<SignalTag>>,
    mut q: Query<(&SignalTag, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let net = &sim.world.traffic.net;
    if net.version != state.lanes_version || sim.full_rebuild {
        state.lanes_version = net.version;
        despawn_all(&mut commands, &existing);
        for post in signal_posts(net) {
            commands.spawn((
                Mesh3d(mats.signal_mesh.clone()),
                MeshMaterial3d(mats.signal_red.clone()),
                Transform::from_translation(post.lamp).with_rotation(Quat::from_rotation_y(-post.heading)),
                SignalTag(post.lane),
            ));
        }
        return;
    }
    for (tag, mut mat) in &mut q {
        let Some(lane) = net.lanes.get(tag.0 as usize) else { continue };
        let green = lane.next.iter().any(|&n| net.lanes[n as usize].green);
        let wanted = if green { &mats.signal_green } else { &mats.signal_red };
        if mat.0 != *wanted {
            mat.0 = wanted.clone();
        }
    }
}

fn dashes(mesh: &mut MeshData, a: SVec2, b: SVec2, width: f32, y: f32, dash: f32, gap: f32, color: [f32; 4]) {
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let d = (b - a) / len;
    let mut t = gap * 0.5;
    while t + dash <= len {
        mesh.ground_strip(&[a + d * t, a + d * (t + dash)], width, y, color);
        t += dash + gap;
    }
}

fn sync_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<PreviewTag>>,
) {
    if sim.world.plan_version == state.plan_version && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.plan_version = sim.world.plan_version;
    despawn_all(&mut commands, &existing);
    let pv = sim.world.preview();
    let pal = &assets.palette;
    let mut glow = MeshData::default();
    for (_, pts, fwd, bwd, kind) in &pv.planned_roads {
        let w = if *kind == nd_sim::roads::RoadKind::Path { nd_sim::roads::PATH_HALF_WIDTH * 2.0 } else { (*fwd + *bwd) as f32 * LANE_WIDTH + SHOULDER * 2.0 };
        glow.ground_strip(pts, w, 0.22, scale(pal.preview, 0.35));
        glow.ground_strip(pts, 0.5, 0.23, rgba(pal.preview));
    }
    for (_, pts, fwd, bwd, kind) in &pv.removed_roads {
        let w = if *kind == nd_sim::roads::RoadKind::Path { nd_sim::roads::PATH_HALF_WIDTH * 2.0 } else { (*fwd + *bwd) as f32 * LANE_WIDTH + SHOULDER * 2.0 };
        glow.ground_strip(pts, w, 0.22, scale(pal.preview_remove, 0.35));
        dashes(&mut glow, pts[0], *pts.last().unwrap(), 0.6, 0.23, 4.0, 3.0, rgba(pal.preview_remove));
    }
    for (_, poly, lu) in &pv.planned_zones {
        let c = pal.zone_color(*lu);
        let mut outline = poly.clone();
        outline.push(poly[0]);
        glow.ground_strip(&outline, 0.8, 0.22, scale(c, 1.3));
        glow.ground_polygon(poly, 0.045, scale(c, 0.12));
    }
    for (_, poly) in &pv.removed_zones {
        let mut outline = poly.clone();
        outline.push(poly[0]);
        glow.ground_strip(&outline, 0.8, 0.22, rgba(pal.preview_remove));
    }
    if !glow.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&glow, false))), MeshMaterial3d(mats.glow()), Transform::IDENTITY, PreviewTag));
    }
}

fn sync_lots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<LotsTag>>,
) {
    let w = &sim.world;
    let vacant = w.lots.values().filter(|l| l.building.is_none()).count();
    let signature = (w.lots.len(), vacant, w.zones.len(), w.roads.version);
    if signature == state.lots_signature && !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.lots_signature = signature;
    despawn_all(&mut commands, &existing);
    let pal = &assets.palette;
    let mut glow = MeshData::default();
    let mut fill = MeshData::default();
    for zone in w.zones.values() {
        let c = pal.zone_color(zone.land_use);
        let mut outline = zone.polygon.clone();
        outline.push(zone.polygon[0]);
        glow.ground_strip(&outline, 0.5, 0.04, scale(c, 1.0));
    }
    for lot in w.lots.values() {
        let c = pal.zone_color(lot.land_use);
        if lot.landlocked {
            // Unusable ground: plain earth, no outline.
            fill.ground_polygon(&lot.polygon, 0.02, scale(pal.ground, 1.15));
        } else if lot.building.is_none() {
            let mut outline = lot.polygon.clone();
            outline.push(lot.polygon[0]);
            glow.ground_strip(&outline, 0.22, 0.035, scale(c, 0.45));
            fill.ground_polygon(&lot.polygon, 0.02, [c[0] * 0.08 + pal.lot_vacant[0], c[1] * 0.08 + pal.lot_vacant[1], c[2] * 0.08 + pal.lot_vacant[2], 1.0]);
        } else {
            fill.ground_polygon(&lot.polygon, 0.02, rgba(pal.gravel));
        }
    }
    if !fill.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&fill, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, LotsTag));
    }
    if !glow.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&glow, false))), MeshMaterial3d(mats.glow()), Transform::IDENTITY, LotsTag));
    }
}

fn spawn_building(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, sim: &Sim, assets: &GameAssets, id: BuildingId) -> Option<(Vec<Entity>, usize)> {
    let b = sim.world.buildings.get(id)?;
    let lot = sim.world.lots.get(b.lot)?;
    let geo = generate_building(lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &assets.rules, &assets.palette);
    let tris = geo.triangle_count();
    Some((spawn_geometry(commands, meshes, mats, &geo, BuildingTag(id)), tris))
}

fn sync_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sim: ResMut<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut stats: ResMut<RenderStats>,
) {
    let rebuild_all = sim.full_rebuild || assets.rules_changed || assets.palette_changed;
    if rebuild_all {
        for (_, ents) in state.buildings.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        stats.building_triangles = 0;
        let ids: Vec<BuildingId> = sim.world.buildings.keys().collect();
        for id in ids {
            if let Some((ents, tris)) = spawn_building(&mut commands, &mut meshes, &mats, &sim, &assets, id) {
                state.buildings.insert(id, ents);
                stats.building_triangles += tris;
            }
        }
    } else {
        let changed: Vec<BuildingId> = sim
            .events
            .iter()
            .filter_map(|e| match e {
                SimEvent::BuildingChanged(id) => Some(*id),
                _ => None,
            })
            .collect();
        for id in changed {
            if let Some(ents) = state.buildings.remove(&id) {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
        }
        let new_ids: Vec<BuildingId> = sim
            .events
            .iter()
            .filter_map(|e| match e {
                SimEvent::BuildingBuilt(id) | SimEvent::BuildingChanged(id) => Some(*id),
                _ => None,
            })
            .collect();
        for id in new_ids {
            if state.buildings.contains_key(&id) {
                continue;
            }
            if let Some((ents, tris)) = spawn_building(&mut commands, &mut meshes, &mats, &sim, &assets, id) {
                state.buildings.insert(id, ents);
                stats.building_triangles += tris;
            }
        }
        let gone: Vec<BuildingId> = state.buildings.keys().copied().filter(|id| !sim.world.buildings.contains_key(*id)).collect();
        for id in gone {
            if let Some(ents) = state.buildings.remove(&id) {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
        }
    }
    stats.building_entities = state.buildings.values().map(|v| v.len()).sum();
    // Clear the full-rebuild flag last so every sync system saw it this frame.
    if sim.full_rebuild {
        sim.full_rebuild = false;
    }
}

fn sync_citizens(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    selection: Res<Selection>,
    mut state: ResMut<RenderState>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>, &mut Mesh3d), With<CitizenTag>>,
) {
    let w = &sim.world;
    let f = sim.frac();
    for (id, _) in w.citizens.iter() {
        if !state.citizens.contains_key(&id) {
            let e = commands
                .spawn((Mesh3d(mats.citizen_mesh.clone()), MeshMaterial3d(mats.citizen.clone()), Transform::IDENTITY, Visibility::Hidden, CitizenTag(id)))
                .id();
            state.citizens.insert(id, e);
        }
    }
    let gone: Vec<CitizenId> = state.citizens.keys().copied().filter(|id| !w.citizens.contains_key(*id)).collect();
    for id in gone {
        if let Some(e) = state.citizens.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    for (id, &e) in state.citizens.iter() {
        let Some(c) = w.citizens.get(*id) else { continue };
        let Ok((mut tf, mut vis, mut mat, mut mesh)) = q.get_mut(e) else { continue };
        if c.is_visible() {
            *vis = Visibility::Visible;
            let pos = if c.prev_pos.distance(c.pos) < 30.0 { c.prev_pos.lerp(c.pos, f) } else { c.pos };
            let heading = lerp_angle(c.prev_heading, c.heading, f);
            tf.translation = Vec3::new(pos.x, 0.16, pos.y);
            tf.rotation = Quat::from_rotation_y(-heading);
            let cycling = c.mode == nd_sim::citizens::TravelMode::Bike && c.activity.is_traveling();
            let wanted_mesh = if cycling { &mats.bike_mesh } else { &mats.citizen_mesh };
            if mesh.0 != *wanted_mesh {
                mesh.0 = wanted_mesh.clone();
            }
        } else {
            *vis = Visibility::Hidden;
        }
        let wanted = if *selection == Selection::Citizen(*id) {
            &mats.citizen_selected
        } else if c.spotlight {
            &mats.citizen_spotlight
        } else {
            &mats.citizen
        };
        if mat.0 != *wanted {
            mat.0 = wanted.clone();
        }
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = b - a;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d * t
}

fn sync_cars(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    selection: Res<Selection>,
    mut state: ResMut<RenderState>,
    mut q: Query<&mut Transform, With<CarTag>>,
) {
    let traffic = &sim.world.traffic;
    for (id, car) in traffic.cars.iter() {
        if !state.cars.contains_key(&id) {
            let body_mat = mats.car_bodies[car.color as usize % mats.car_bodies.len().max(1)].clone();
            let mesh = if car.cargo.is_some() { mats.truck_body_mesh.clone() } else { mats.car_body_mesh.clone() };
            let body = commands.spawn((Mesh3d(mesh), MeshMaterial3d(body_mat), Transform::IDENTITY, CarTag(id))).id();
            let lights = commands.spawn((Mesh3d(mats.car_lights_mesh.clone()), MeshMaterial3d(mats.glow()), Transform::IDENTITY, CarTag(id))).id();
            state.cars.insert(id, [body, lights]);
        }
    }
    let gone: Vec<CarId> = state.cars.keys().copied().filter(|id| !traffic.cars.contains_key(*id)).collect();
    for id in gone {
        if let Some(ents) = state.cars.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let f = sim.frac();
    for (id, ents) in state.cars.iter() {
        let Some(car) = traffic.cars.get(*id) else { continue };
        let pos = car.prev_pos.lerp(car.pos, f);
        let heading = lerp_angle(car.prev_heading, car.heading, f);
        let selected = *selection == Selection::Citizen(car.citizen);
        for e in ents {
            if let Ok(mut tf) = q.get_mut(*e) {
                tf.translation = Vec3::new(pos.x, 0.05 + if selected { 0.4 } else { 0.0 }, pos.y);
                tf.rotation = Quat::from_rotation_y(-heading);
            }
        }
    }
}

/// Land value heat map over the ground. Toggled with V.
fn sync_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut overlays: ResMut<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<OverlayTag>>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        overlays.land_value = !overlays.land_value;
    }
    let lv = &sim.world.land_value;
    if overlays.land_value == state.overlay_shown && lv.version == state.overlay_version && !assets.palette_changed {
        return;
    }
    state.overlay_shown = overlays.land_value;
    state.overlay_version = lv.version;
    despawn_all(&mut commands, &existing);
    if !overlays.land_value {
        return;
    }
    let pal = &assets.palette;
    let mut mesh = MeshData::default();
    let half = lv.cell * 0.5;
    for iy in 0..lv.n {
        for ix in 0..lv.n {
            let v = lv.values[iy * lv.n + ix];
            let c = lv.cell_center(ix, iy);
            let t = ((v - 0.15) / 0.7).clamp(0.0, 1.0);
            let col = lerp3(pal.overlay_low, pal.overlay_high, t);
            let poly = [SVec2::new(c.x - half, c.y - half), SVec2::new(c.x + half, c.y - half), SVec2::new(c.x + half, c.y + half), SVec2::new(c.x - half, c.y + half)];
            mesh.ground_polygon(&poly, 0.3, [col[0], col[1], col[2], 0.42]);
        }
    }
    commands.spawn((Mesh3d(meshes.add(to_mesh(&mesh, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, OverlayTag));
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Sun, ambient light, sky, fog and glow strength follow the sim clock.
fn update_daylight(
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<SunTag>>,
    mut fog: Query<&mut DistanceFog>,
) {
    let pal = &assets.palette;
    let t = if lighting.follow_clock { sim.world.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    let daylight = smoothstep(-0.12, 0.30, elev);
    let low_sun = 1.0 - smoothstep(0.05, 0.45, elev.abs());
    let dusk = (1.0 - smoothstep(0.0, 0.22, elev.abs())) * 0.8;

    let sun_color = lerp3(lerp3(pal.sun_low, pal.sun, 1.0 - low_sun), pal.moon, 1.0 - daylight);
    let illuminance = 500.0 + 10500.0 * daylight;
    let azimuth = (t - 0.25) * std::f32::consts::TAU;
    let sun_dir = if elev > 0.02 {
        let r = (1.0 - elev * elev).sqrt();
        Vec3::new(azimuth.cos() * r, elev, azimuth.sin() * r)
    } else {
        Vec3::new(0.4, 0.75, -0.5)
    };
    for (mut light, mut tf) in &mut sun {
        light.color = Color::linear_rgb(sun_color[0], sun_color[1], sun_color[2]);
        light.illuminance = illuminance;
        tf.translation = sun_dir * 1000.0;
        tf.look_at(Vec3::ZERO, Vec3::Y);
    }

    let sky = lerp3(lerp3(pal.sky, pal.sky_day, daylight), pal.sky_dusk, dusk * 0.6);
    let fog_c = lerp3(lerp3(pal.fog, pal.fog_day, daylight), pal.sky_dusk, dusk * 0.5);
    clear.0 = Color::linear_rgb(sky[0], sky[1], sky[2]);
    for mut f in &mut fog {
        f.color = Color::linear_rgb(fog_c[0], fog_c[1], fog_c[2]);
    }
    ambient.brightness = 90.0 + 800.0 * daylight;
    let amb = lerp3([0.35, 0.45, 0.9], [0.8, 0.85, 1.0], daylight);
    ambient.color = Color::linear_rgb(amb[0], amb[1], amb[2]);

    let glow_k = 1.0 - 0.7 * daylight;
    if let Some(mut m) = materials.get_mut(&mats.glow()) {
        m.base_color = Color::linear_rgb(glow_k, glow_k, glow_k);
    }
}
