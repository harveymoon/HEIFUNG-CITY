//! Turns simulation state into Bevy entities. Static geometry (terrain,
//! streets, lot ground, project preview) is rebuilt as whole meshes when it
//! changes; buildings and vacant-lot dressing are batched per [`CHUNK`]-metre
//! cell, one mesh per surface per cell, and a cell is rebuilt when anything in
//! it changes; citizens and cars are small shared meshes moved every frame.
//!
//! Every [`Surface`] has a material with a procedurally generated, seamlessly
//! tiling colour texture and normal map. Meshes carry world-scale UVs, so the
//! same brick texture lines up across a whole facade.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight, NotShadowCaster, NotShadowReceiver};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use nd_procgen::street::{generate_street, signal_posts, StreetInput};
use nd_procgen::{build_terrain, bus_stops, generate_building, generate_texture, generate_vacant, mip_chain, scatter_nature, Extras, Geometry, MeshData, StyleBook, Surface, TerrainConfig};
use nd_sim::roads::{LANE_WIDTH, SHOULDER};
use nd_sim::traffic::LaneId;
use nd_sim::{BuildingId, CarId, CitizenId, LotId, SimEvent, Vec2 as SVec2, World};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::assets::GameAssets;
use crate::ui::UiFlags;
use crate::sim::{Sim, SimSet};
use crate::tools::Selection;

/// Side of the square cells that buildings and vacant-lot dressing are
/// batched into. Wide enough that a few hundred buildings become a few dozen
/// meshes per surface, small enough that one changed building regenerates a
/// street block rather than the whole city.
const CHUNK: f32 = 128.0;

/// Cell index of a world position on the [`CHUNK`] grid.
type ChunkKey = (i32, i32);

fn chunk_of(p: SVec2) -> ChunkKey {
    ((p.x / CHUNK).floor() as i32, (p.y / CHUNK).floor() as i32)
}

#[derive(Component)]
struct SunTag;
#[derive(Component)]
struct TerrainTag;
#[derive(Component, Clone)]
struct ScatterTag;
#[derive(Component)]
struct RoadsTag;
/// Lot ground fills and outline glows: one mesh per surface for the whole city.
#[derive(Component, Clone)]
struct LotsTag;
/// Park layouts, one set of entities per park lot.
#[derive(Component, Clone)]
struct ParkTag;
/// Vacant-lot dressing, batched per chunk.
#[derive(Component, Clone)]
struct VacantTag;
#[derive(Component)]
struct PreviewTag;
#[derive(Component)]
struct SignalTag(LaneId);
#[derive(Component)]
struct OverlayTag;
#[derive(Component)]
struct PathsTag;
#[derive(Component)]
struct HazeTag;
#[derive(Component, Clone)]
struct TransitTag;
/// Marker only: which train an entity belongs to is tracked in
/// [`RenderState::trains`], so the tag carries no id.
#[derive(Component)]
struct TrainTag;

/// Toggleable data layers drawn over the ground.
#[derive(Resource)]
pub struct Overlays {
    pub land_value: bool,
    /// Pedestrian network: lot loops, alleys and gates.
    pub paths: bool,
    /// Zone outline glows and vacant-lot outlines. The ground fills stay.
    pub zoning: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self { land_value: false, paths: false, zoning: true }
    }
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
/// One chunk's worth of building geometry. Which buildings a mesh holds is
/// tracked in [`RenderState::building_chunks`]; selection and inspection go
/// through the sim's lots, never through mesh entities, so no id is needed.
#[derive(Component, Clone)]
pub struct BuildingTag;
/// Movers are looked up through the maps in [`RenderState`], so these tags
/// are plain markers for the per-frame transform queries.
#[derive(Component)]
pub struct CitizenTag;
#[derive(Component)]
pub struct CarTag;

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
    bus_body_mesh: Handle<Mesh>,
    loco_mesh: Handle<Mesh>,
    coach_mesh: Handle<Mesh>,
    wagon_mesh: Handle<Mesh>,
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

/// The meshes of one chunk and what they were built from.
#[derive(Default)]
struct ChunkMeshes {
    entities: Vec<Entity>,
    triangles: usize,
}

#[derive(Resource, Default)]
struct RenderState {
    roads_version: u64,
    lanes_version: u64,
    plan_version: u64,
    /// What the lot ground meshes were built for: lot and zone counts, roads
    /// version, zoning overlay, and a hash of every lot's built/park/
    /// landlocked state.
    lots_signature: Option<(usize, usize, u64, bool, u64)>,
    /// Park lots drawn, with a hash of the layout and polygon they show.
    parks: HashMap<LotId, (u64, Vec<Entity>)>,
    /// Vacant dressing per chunk, with a hash of the lots and land-value
    /// buckets it was generated from.
    vacant_chunks: HashMap<ChunkKey, (u64, Vec<Entity>)>,
    /// Roads version and zone count the open-ground scatter was built for.
    scatter_signature: Option<(u64, usize)>,
    /// Which chunk each drawn building sits in, so a changed or removed
    /// building can mark its chunk dirty.
    building_chunks: HashMap<BuildingId, ChunkKey>,
    chunks: HashMap<ChunkKey, ChunkMeshes>,
    citizens: HashMap<CitizenId, Entity>,
    cars: HashMap<CarId, [Entity; 2]>,
    overlay_version: u64,
    overlay_shown: bool,
    paths_version: u64,
    paths_shown: bool,
    haze_signature: usize,
    transit_version: u64,
    trains: HashMap<nd_sim::TrainId, Vec<Entity>>,
    /// Quantised sun elevation the lighting was last written for.
    daylight_key: Option<i32>,
    /// Quantised glow strength last written to the shared Glow material.
    glow_key: Option<i32>,
}

/// Every building's resolved style and the older neighbours it was picked
/// against, so styles cluster or contrast with what stands nearby (see
/// `nd_procgen::buildings::adjacency`).
#[derive(Resource, Default)]
struct BuildingStyles(StyleBook);

/// Street-level props drawn only around the walker (`nd_procgen::walk`).
#[derive(Component, Clone)]
struct WalkDetailTag;

/// Renderer statistics shown in the UI.
#[derive(Resource, Default)]
pub struct RenderStats {
    /// Building meshes alive: one per non-empty surface per chunk.
    pub building_entities: usize,
    pub street_triangles: usize,
    pub building_triangles: usize,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderState>().init_resource::<Overlays>().init_resource::<Lighting>().init_resource::<RenderStats>().init_resource::<BuildingStyles>().add_systems(Startup, setup).add_systems(
            Update,
            (sync_terrain, sync_scatter, sync_roads, sync_signals, sync_preview, sync_lots, sync_parks, sync_vacant, sync_overlay, sync_paths, sync_haze, sync_transit, sync_buildings, sync_walk_detail, sync_citizens, sync_cars, sync_trains, update_daylight)
                .chain()
                .in_set(SimSet::Sync),
        );
        app.init_resource::<LandscapeRender>().add_systems(Update, (sync_landscape, sync_ships, lift_spawned, lift_movers).chain().in_set(SimSet::Sync).after(update_daylight));
    }
}

/// Convert mesh data; lit meshes get tangents so normal maps work.
///
/// The CPU copy is dropped once the mesh is uploaded: nothing reads vertices
/// back, and the generated city is hundreds of megabytes of them. Vertex
/// colours stay `Float32x4` because that is the only format Bevy accepts for
/// `ATTRIBUTE_COLOR`, and the generators use values above 1 for emissive
/// surfaces anyway.
pub fn to_mesh(data: &MeshData, tangents: bool) -> Mesh {
    mesh_with_usage(data, tangents, RenderAssetUsages::RENDER_WORLD)
}

/// A mesh many entities share over the whole run (citizens, cars, signal
/// lamps). These keep their CPU copy: Bevy computes an entity's bounds from
/// the main-world mesh when the entity is spawned, and movers are spawned
/// long after the mesh was first uploaded.
fn shared_mesh(data: &MeshData, tangents: bool) -> Mesh {
    mesh_with_usage(data, tangents, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD)
}

fn mesh_with_usage(data: &MeshData, tangents: bool, usage: RenderAssetUsages) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone())
        .with_inserted_indices(Indices::U32(data.indices.clone()));
    if tangents && !data.indices.is_empty() {
        if data.tangents.len() == data.positions.len() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, data.tangents.clone());
        } else {
            let _ = mesh.generate_tangents();
        }
    }
    mesh
}

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn scale(c: [f32; 3], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

/// Spawn one entity per non-empty surface of a geometry. Glow strips are
/// thin unlit decals: they neither cast nor receive shadows, which also
/// keeps them out of the shadow passes.
fn spawn_geometry<T: Component + Clone>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, geo: &Geometry, tag: T) -> Vec<Entity> {
    let mut out = Vec::new();
    for (s, data) in geo.iter() {
        let mesh = to_mesh(data, !s.is_glow());
        let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, tag.clone()));
        if s.is_glow() {
            e.insert((NotShadowCaster, NotShadowReceiver));
        }
        out.push(e.id());
    }
    out
}

/// Spawn a glow-surface mesh: unlit, no shadows either way.
fn spawn_glow<T: Component>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, data: &MeshData, tag: T) {
    commands.spawn((Mesh3d(meshes.add(to_mesh(data, false))), MeshMaterial3d(mats.glow()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, tag));
}

/// Spawn a translucent overlay mesh (heat map, haze): no shadows either way.
fn spawn_overlay<T: Component>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, data: &MeshData, tag: T) {
    commands.spawn((Mesh3d(meshes.add(to_mesh(data, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, tag));
}

/// Hash helper for change detection on generated geometry inputs.
fn hash_f32s<H: Hasher>(h: &mut H, pts: impl IntoIterator<Item = SVec2>) {
    for p in pts {
        p.x.to_bits().hash(h);
        p.y.to_bits().hash(h);
    }
}

/// Terrain extents follow the buildable square: the hills start a little
/// past the edge of the city and the mesh reaches twice as far out.
fn terrain_config(w: &World) -> TerrainConfig {
    let half = w.config.half_extent;
    // On a landscape the scatter is generated flat and set on the ground
    // by `lift_spawned`, so it must not carry the old hills itself.
    let hill_height = if w.landscape.is_flat() { TerrainConfig::default().hill_height } else { 0.0 };
    TerrainConfig { half_size: half * 2.0, basin_radius: half + 100.0, hill_height, ..TerrainConfig::default() }
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
        meshes.add(shared_mesh(&m, false))
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
        meshes.add(shared_mesh(&m, false))
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
        meshes.add(shared_mesh(&m, true))
    };
    let truck_body_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(-0.9, 1.75, 0.0), Vec3::new(5.6, 2.5, 2.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(3.0, 1.4, 0.0), Vec3::new(2.0, 1.8, 2.2), 0.0, [0.5, 0.5, 0.55, 1.0]);
        m.boxed(Vec3::new(3.9, 1.8, 0.0), Vec3::new(0.2, 0.7, 2.0), 0.0, [0.2, 0.25, 0.3, 1.0]);
        wheels(&mut m, &[-2.6, -1.4, 2.6], 1.05, 0.5);
        meshes.add(shared_mesh(&m, true))
    };
    let bus_body_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 1.65, 0.0), Vec3::new(11.5, 3.0, 2.5), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(0.0, 2.25, 0.0), Vec3::new(11.0, 0.9, 2.56), 0.0, [0.14, 0.17, 0.2, 1.0]);
        m.boxed(Vec3::new(5.6, 1.95, 0.0), Vec3::new(0.4, 1.6, 2.3), 0.0, [0.14, 0.17, 0.2, 1.0]);
        m.boxed(Vec3::new(-5.72, 2.95, 0.0), Vec3::new(0.1, 0.4, 1.6), 0.0, [1.0, 0.7, 0.2, 1.0]);
        wheels(&mut m, &[-3.8, 3.6], 1.15, 0.5);
        meshes.add(shared_mesh(&m, true))
    };
    let rail_wheels = |m: &mut MeshData, len: f32| {
        for x in [-len * 0.35, len * 0.35] {
            for s in [-1.0f32, 1.0] {
                m.boxed(Vec3::new(x, 0.5, s * 0.75), Vec3::new(2.6, 0.7, 0.3), 0.0, [0.1, 0.1, 0.1, 1.0]);
            }
        }
    };
    let loco_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 2.1, 0.0), Vec3::new(17.4, 2.6, 2.8), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(5.5, 3.7, 0.0), Vec3::new(5.0, 0.8, 2.6), 0.0, [0.2, 0.22, 0.25, 1.0]);
        m.boxed(Vec3::new(7.5, 2.6, 0.0), Vec3::new(1.2, 1.4, 2.5), 0.0, [0.14, 0.17, 0.2, 1.0]);
        m.boxed(Vec3::new(8.8, 1.1, 0.0), Vec3::new(0.6, 0.6, 2.4), 0.0, [0.9, 0.2, 0.1, 1.0]);
        m.boxed(Vec3::new(-2.0, 3.6, 0.0), Vec3::new(9.0, 0.5, 2.0), 0.0, [0.3, 0.3, 0.32, 1.0]);
        rail_wheels(&mut m, 17.0);
        meshes.add(shared_mesh(&m, true))
    };
    let coach_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 2.1, 0.0), Vec3::new(18.4, 2.6, 2.8), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(0.0, 2.6, 0.0), Vec3::new(17.8, 0.9, 2.86), 0.0, [0.14, 0.17, 0.2, 1.0]);
        m.boxed(Vec3::new(0.0, 3.5, 0.0), Vec3::new(18.0, 0.3, 2.4), 0.0, [0.5, 0.5, 0.52, 1.0]);
        rail_wheels(&mut m, 18.0);
        meshes.add(shared_mesh(&m, true))
    };
    let wagon_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 1.0, 0.0), Vec3::new(13.6, 0.5, 2.7), 0.0, [0.2, 0.2, 0.22, 1.0]);
        m.boxed(Vec3::new(0.0, 2.55, 0.0), Vec3::new(12.2, 2.6, 2.44), 0.0, [1.0, 1.0, 1.0, 1.0]);
        rail_wheels(&mut m, 13.0);
        meshes.add(shared_mesh(&m, true))
    };
    let car_lights_mesh = {
        let mut m = MeshData::default();
        for z in [-0.6, 0.6] {
            m.boxed(Vec3::new(2.1, 0.7, z), Vec3::new(0.15, 0.3, 0.4), 0.0, rgba(pal.headlight));
            m.boxed(Vec3::new(-2.1, 0.75, z), Vec3::new(0.15, 0.25, 0.4), 0.0, rgba(pal.taillight));
        }
        meshes.add(shared_mesh(&m, false))
    };
    let overlay = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let signal_green = materials.add(unlit(pal.signal_green));
    let signal_red = materials.add(unlit(pal.signal_red));
    let signal_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::ZERO, Vec3::new(0.08, 0.26, 0.26), 0.0, [1.0, 1.0, 1.0, 1.0]);
        meshes.add(shared_mesh(&m, false))
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
        bus_body_mesh,
        loco_mesh,
        coach_mesh,
        wagon_mesh,
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
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    existing: Query<Entity, With<TerrainTag>>,
) {
    if !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    // A landscape's ground is drawn by `sync_landscape`.
    if !sim.world.landscape.is_flat() {
        return;
    }
    despawn_all(&mut commands, &existing);
    let data = build_terrain(&terrain_config(&sim.world), &assets.palette);
    commands.spawn((Mesh3d(meshes.add(to_mesh(&data, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, TerrainTag));
}

/// Open ground within `reach` metres of a way, answered from a coarse grid.
/// The scatter pass asks about ~40k sample points, and the network can hold
/// thousands of segments, so a linear nearest-segment scan per sample was a
/// visible hitch on every implement. Each segment is binned into every cell
/// its reach-expanded bounding box touches, so a query only looks in its own
/// cell.
struct NearWays {
    cell: f32,
    reach: f32,
    origin: SVec2,
    cols: i32,
    rows: i32,
    bins: Vec<Vec<(SVec2, SVec2)>>,
}

impl NearWays {
    fn new(roads: &nd_sim::RoadNetwork, reach: f32, half_size: f32, keep: impl Fn(&nd_sim::Segment) -> bool) -> Self {
        let cell = reach;
        let origin = SVec2::splat(-half_size - reach);
        let cols = ((half_size + reach) * 2.0 / cell).ceil() as i32 + 1;
        let mut me = Self { cell, reach, origin, cols, rows: cols, bins: vec![Vec::new(); (cols * cols) as usize] };
        for (sid, seg) in roads.alive_segments() {
            if !keep(seg) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let lo = a.min(b) - SVec2::splat(reach);
            let hi = a.max(b) + SVec2::splat(reach);
            let (c0, r0) = me.cell_of(lo);
            let (c1, r1) = me.cell_of(hi);
            for r in r0.max(0)..=r1.min(me.rows - 1) {
                for c in c0.max(0)..=c1.min(me.cols - 1) {
                    me.bins[(r * me.cols + c) as usize].push((a, b));
                }
            }
        }
        me
    }

    fn cell_of(&self, p: SVec2) -> (i32, i32) {
        (((p.x - self.origin.x) / self.cell).floor() as i32, ((p.y - self.origin.y) / self.cell).floor() as i32)
    }

    fn near(&self, p: SVec2) -> bool {
        let (c, r) = self.cell_of(p);
        if c < 0 || r < 0 || c >= self.cols || r >= self.rows {
            return false;
        }
        self.bins[(r * self.cols + c) as usize].iter().any(|&(a, b)| nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < self.reach)
    }
}

/// Rocks, scrub and dead trees on the open ground: everything at least 40 m
/// from any road or track and outside every zone. Rebuilt when roads or
/// zones change, since both push the scrub back.
fn sync_scatter(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<ScatterTag>>,
) {
    let w = &sim.world;
    let signature = (w.roads.version, w.zones.len());
    if state.scatter_signature == Some(signature) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.scatter_signature = Some(signature);
    despawn_all(&mut commands, &existing);
    let cfg = terrain_config(w);
    const REACH: f32 = 40.0;
    // Streets and paths keep the scrub back, and so does the railway; the
    // network's own nearest queries treat them separately, so bin them all.
    let ways = NearWays::new(&w.roads, REACH, cfg.half_size, |_| true);
    // Zone polygons with bounding boxes so most samples skip the point test.
    let zones: Vec<(SVec2, SVec2, &[SVec2])> = w
        .zones
        .values()
        .filter(|z| !z.polygon.is_empty())
        .map(|z| {
            let lo = z.polygon.iter().copied().fold(SVec2::splat(f32::MAX), SVec2::min);
            let hi = z.polygon.iter().copied().fold(SVec2::splat(f32::MIN), SVec2::max);
            (lo, hi, z.polygon.as_slice())
        })
        .collect();
    let clear = |p: SVec2| {
        // Nothing grows in the water or on the wet sand.
        if !w.landscape.is_flat() && w.landscape.water_distance(p) < 6.0 {
            return false;
        }
        if zones.iter().any(|(lo, hi, poly)| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && nd_sim::zoning::point_in_polygon(p, poly)) {
            return false;
        }
        !ways.near(p)
    };
    let geo = scatter_nature(&cfg, &assets.palette, w.config.seed, &clear);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, ScatterTag);
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
        let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(data, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, RoadsTag));
        if s.is_glow() {
            e.insert((NotShadowCaster, NotShadowReceiver));
        }
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
                NotShadowCaster,
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
        let w = preview_width(*kind, *fwd, *bwd);
        glow.ground_strip(pts, w, 0.22, scale(pal.preview, 0.35));
        glow.ground_strip(pts, 0.5, 0.23, rgba(pal.preview));
    }
    for (_, pts, fwd, bwd, kind) in &pv.removed_roads {
        let w = preview_width(*kind, *fwd, *bwd);
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
        spawn_glow(&mut commands, &mut meshes, &mats, &glow, PreviewTag);
    }
}

/// Park land is a park as soon as it is planned, built or not, and park
/// backland with no road is reached through the park itself.
fn is_park_lot(lot: &nd_sim::Lot) -> bool {
    lot.building.is_none() && lot.park.is_some()
}

/// A lot with nothing on it and no park plan: it gets the vacant dressing.
fn is_vacant_lot(lot: &nd_sim::Lot) -> bool {
    lot.building.is_none() && lot.park.is_none() && !lot.landlocked
}

/// Lot ground fills and (with the zoning overlay, toggled with O) the zone
/// outline glows and vacant-lot outlines: two meshes for the whole city,
/// rebuilt when a lot appears, disappears, or changes between built, vacant
/// and park. Land value does not reach them, so the periodic land-value
/// recompute no longer touches any ground mesh.
fn sync_lots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut overlays: ResMut<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<LotsTag>>,
    flags: Res<UiFlags>,
) {
    if keys.just_pressed(KeyCode::KeyO) && !flags.keyboard_captured {
        overlays.zoning = !overlays.zoning;
    }
    let w = &sim.world;
    let mut h = DefaultHasher::new();
    for (id, lot) in w.lots.iter() {
        id.hash(&mut h);
        (lot.building.is_some(), lot.park.is_some(), lot.landlocked).hash(&mut h);
    }
    let signature = (w.lots.len(), w.zones.len(), w.roads.version, overlays.zoning, h.finish());
    if state.lots_signature == Some(signature) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.lots_signature = Some(signature);
    despawn_all(&mut commands, &existing);
    let pal = &assets.palette;
    let outlines = overlays.zoning;
    let mut glow = MeshData::default();
    let mut fill = MeshData::default();
    if outlines {
        for zone in w.zones.values().filter(|z| !z.polygon.is_empty()) {
            let c = pal.zone_color(zone.land_use);
            let mut outline = zone.polygon.clone();
            outline.push(zone.polygon[0]);
            glow.ground_strip(&outline, 0.5, 0.04, scale(c, 1.0));
        }
    }
    for lot in w.lots.values() {
        if is_park_lot(lot) || lot.polygon.is_empty() {
            continue;
        }
        if lot.landlocked {
            // Unusable ground: plain earth, no outline.
            fill.ground_polygon(&lot.polygon, 0.02, scale(pal.ground, 1.15));
        } else if lot.building.is_none() {
            if outlines {
                let c = pal.zone_color(lot.land_use);
                let mut outline = lot.polygon.clone();
                outline.push(lot.polygon[0]);
                glow.ground_strip(&outline, 0.22, 0.035, scale(c, 0.45));
            }
        } else {
            fill.ground_polygon(&lot.polygon, 0.02, rgba(pal.gravel));
        }
    }
    if !fill.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&fill, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, LotsTag));
    }
    if !glow.is_empty() {
        spawn_glow(&mut commands, &mut meshes, &mats, &glow, LotsTag);
    }
}

/// What a park lot's drawing depends on: its tier and feature set, its
/// paths and benches, and the parcel outline itself.
fn park_key(lot: &nd_sim::Lot, layout: &nd_sim::ParkLayout) -> u64 {
    let mut h = DefaultHasher::new();
    layout.tier.hash(&mut h);
    hash_f32s(&mut h, lot.polygon.iter().copied());
    for f in &layout.features {
        f.kind.hash(&mut h);
        hash_f32s(&mut h, [f.pos, SVec2::new(f.radius, f.heading)]);
    }
    for path in &layout.paths {
        hash_f32s(&mut h, path.iter().copied());
    }
    for &(p, facing) in &layout.benches {
        hash_f32s(&mut h, [p]);
        facing.to_bits().hash(&mut h);
    }
    layout.shared_edges.hash(&mut h);
    h.finish()
}

/// Park layouts, one set of entities per park lot. A park is regenerated
/// only when its own layout or outline changes; neighbouring parks and the
/// land-value recompute leave it alone.
fn sync_parks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
) {
    let w = &sim.world;
    let rebuild_all = assets.palette_changed || assets.rules_changed || sim.full_rebuild;
    // Drop parks that are gone, built on, or stale.
    let mut wanted: HashMap<LotId, u64> = HashMap::new();
    for (id, lot) in w.lots.iter() {
        if let Some(layout) = lot.park.as_ref().filter(|_| is_park_lot(lot)) {
            wanted.insert(id, park_key(lot, layout));
        }
    }
    let stale: Vec<LotId> = state.parks.iter().filter(|(id, (key, _))| rebuild_all || wanted.get(id) != Some(key)).map(|(id, _)| *id).collect();
    for id in stale {
        if let Some((_, ents)) = state.parks.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    for (id, key) in wanted {
        if state.parks.contains_key(&id) {
            continue;
        }
        let lot = &w.lots[id];
        let Some(layout) = &lot.park else { continue };
        let seed = (lot.center.x * 31.0 + lot.center.y * 17.0).abs() as u64;
        let extras = Extras { park: Some(layout), ..Extras::default() };
        let geo = generate_building(lot, nd_sim::LandUse::Park, 1, layout.tier, 1.0, seed, &assets.rules, &assets.palette, &extras);
        let ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, ParkTag);
        state.parks.insert(id, (key, ents));
    }
}

/// Which look a vacant lot gets for a land value: dormant, wild or parklet.
fn vacant_bucket(land_value: f32) -> u8 {
    if land_value < nd_procgen::vacant::LOW {
        0
    } else if land_value < nd_procgen::vacant::HIGH {
        1
    } else {
        2
    }
}

/// Vacant-lot dressing, merged into one mesh per surface per chunk. A chunk
/// is regenerated when the set of vacant lots in it changes, a lot's outline
/// changes, or a lot crosses a land-value bucket; the land-value recompute
/// itself (every 120 ticks) changes nothing until a lot crosses a threshold.
fn sync_vacant(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
) {
    let w = &sim.world;
    let rebuild_all = assets.palette_changed || sim.full_rebuild;
    // Per chunk: the lots in it and a hash of what their dressing depends on.
    let mut per_chunk: HashMap<ChunkKey, (DefaultHasher, Vec<LotId>)> = HashMap::new();
    for (id, lot) in w.lots.iter() {
        if !is_vacant_lot(lot) {
            continue;
        }
        let (h, ids) = per_chunk.entry(chunk_of(lot.center)).or_insert_with(|| (DefaultHasher::new(), Vec::new()));
        id.hash(h);
        vacant_bucket(w.land_value.at(lot.center)).hash(h);
        hash_f32s(h, lot.polygon.iter().copied());
        ids.push(id);
    }
    let wanted: HashMap<ChunkKey, (u64, Vec<LotId>)> = per_chunk.into_iter().map(|(k, (h, ids))| (k, (h.finish(), ids))).collect();
    let stale: Vec<ChunkKey> = state.vacant_chunks.iter().filter(|(k, (key, _))| rebuild_all || wanted.get(k).map(|(h, _)| h) != Some(key)).map(|(k, _)| *k).collect();
    for k in stale {
        if let Some((_, ents)) = state.vacant_chunks.remove(&k) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let pal = &assets.palette;
    for (k, (key, ids)) in wanted {
        if state.vacant_chunks.contains_key(&k) {
            continue;
        }
        let mut geo = Geometry::default();
        for id in ids {
            let lot = &w.lots[id];
            // Dormant, going wild or kept as a parklet, by what the land is worth.
            let seed = w.config.seed ^ ((lot.center.x * 31.0 + lot.center.y * 17.0).abs() as u64);
            geo.append(&generate_vacant(lot, w.land_value.at(lot.center), seed, pal));
        }
        let ents = if geo.triangle_count() > 0 { spawn_geometry(&mut commands, &mut meshes, &mats, &geo, VacantTag) } else { Vec::new() };
        state.vacant_chunks.insert(k, (key, ents));
    }
}

/// Generate one building's geometry into the chunk's merged geometry.
fn append_building(geo: &mut Geometry, world: &World, assets: &GameAssets, styles: &StyleBook, id: BuildingId) -> Option<()> {
    let b = world.buildings.get(id)?;
    let lot = world.lots.get(b.lot)?;
    let extras = Extras {
        parking: b.parking_visual(),
        plant: b.plant,
        powered: b.powered,
        park: lot.park.as_ref(),
        depot: b.depot,
        rail: b.rail.as_ref(),
        port: b.port,
        construction: b.construction.as_ref().map(|c| c.progress),
        neighbours: styles.neighbours(id),
        business: b.business.as_ref().map(|z| z.kind),
        fleet: b.business.as_ref().filter(|z| z.kind == nd_sim::BusinessKind::FleetLot).map_or(0.5, |z| z.fleet_trucks as f32 / world.fleet_capacity(id).max(1) as f32),
    };
    let mut g = generate_building(lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &assets.rules, &assets.palette, &extras);
    // Every building stands on its lot's levelled pad.
    nd_procgen::terrain::raise_geometry(&mut g, world.lot_pad(b.lot));
    geo.append(&g);
    Some(())
}

/// The chunk a building is drawn in: by its lot centre, so a building and
/// its lot's vacant/park neighbours share cells.
fn building_chunk(world: &World, id: BuildingId) -> Option<ChunkKey> {
    let b = world.buildings.get(id)?;
    Some(chunk_of(world.lots.get(b.lot)?.center))
}

/// Buildings, batched per [`CHUNK`] cell: one mesh per non-empty surface per
/// cell instead of one per surface per building. A cell is thrown away and
/// regenerated whenever a building in it is built, changed or removed, so
/// an edit costs one street block of procgen rather than the whole city, and
/// a few hundred buildings are a few dozen draw calls per surface.
fn sync_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sim: ResMut<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut stats: ResMut<RenderStats>,
    mut styles: ResMut<BuildingStyles>,
) {
    let rebuild_all = sim.full_rebuild || assets.rules_changed || assets.palette_changed;
    if sim.full_rebuild || assets.rules_changed {
        styles.0.clear();
    }
    let mut dirty: HashSet<ChunkKey> = HashSet::new();
    if rebuild_all {
        for (_, chunk) in state.chunks.drain() {
            for e in chunk.entities {
                commands.entity(e).despawn();
            }
        }
        state.building_chunks.clear();
        if sim.full_rebuild {
            // Slotmap keys are reused by a loaded world, so an old car mesh
            // could stand in for a new bus; start the movers from scratch.
            for (_, e) in state.citizens.drain() {
                commands.entity(e).despawn();
            }
            for (_, ents) in state.cars.drain() {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
            for (_, ents) in state.trains.drain() {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
        }
        for id in sim.world.buildings.keys() {
            if let Some(k) = building_chunk(&sim.world, id) {
                dirty.insert(k);
            }
        }
    } else {
        // Built or changed: the chunk the building lives in now, and the one
        // it was drawn in (the same unless the lot moved under it).
        for e in &sim.events {
            if let SimEvent::BuildingBuilt(id) | SimEvent::BuildingChanged(id) = e {
                if let Some(k) = building_chunk(&sim.world, *id) {
                    dirty.insert(k);
                }
                if let Some(k) = state.building_chunks.get(id) {
                    dirty.insert(*k);
                }
            }
        }
        // Removed buildings leave no event in some paths (a rebuild of the
        // master plan drops them), so compare against what was drawn.
        let gone: Vec<BuildingId> = state.building_chunks.keys().copied().filter(|id| !sim.world.buildings.contains_key(*id)).collect();
        for id in gone {
            if let Some(k) = state.building_chunks.remove(&id) {
                dirty.insert(k);
            }
        }
        // Buildings that exist but were never drawn (missed events) are
        // caught the same way, cheaply, since the map lookup is O(1).
        for id in sim.world.buildings.keys() {
            if !state.building_chunks.contains_key(&id) {
                if let Some(k) = building_chunk(&sim.world, id) {
                    dirty.insert(k);
                }
            }
        }
    }
    let touched = !dirty.is_empty();
    if touched {
        styles.0.refresh(&sim.world, &assets.rules);
        // Bucket every building once; only the dirty cells are regenerated.
        let mut members: HashMap<ChunkKey, Vec<BuildingId>> = HashMap::new();
        for id in sim.world.buildings.keys() {
            if let Some(k) = building_chunk(&sim.world, id) {
                if dirty.contains(&k) {
                    members.entry(k).or_default().push(id);
                }
            }
        }
        for k in dirty {
            if let Some(chunk) = state.chunks.remove(&k) {
                for e in chunk.entities {
                    commands.entity(e).despawn();
                }
            }
            let Some(ids) = members.remove(&k) else { continue };
            let mut geo = Geometry::default();
            for &id in &ids {
                if append_building(&mut geo, &sim.world, &assets, &styles.0, id).is_some() {
                    state.building_chunks.insert(id, k);
                }
            }
            let entities = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, BuildingTag);
            state.chunks.insert(k, ChunkMeshes { entities, triangles: geo.triangle_count() });
        }
    }
    if rebuild_all || touched {
        stats.building_entities = state.chunks.values().map(|c| c.entities.len()).sum();
        stats.building_triangles = state.chunks.values().map(|c| c.triangles).sum();
    }
    // Clear the full-rebuild flag last so every sync system saw it this frame.
    if sim.full_rebuild {
        sim.full_rebuild = false;
    }
}

/// Walk mode only: street-level detail (signs, carts, vending machines,
/// steam, cables, puddles, scooters, clutter, cracks) within
/// `WALK_RADIUS` of the walker, redrawn after `WALK_REDRAW` metres.
#[allow(clippy::too_many_arguments)]
fn sync_walk_detail(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mode: Res<crate::camera::CameraMode>,
    walk: Res<crate::camera::WalkState>,
    mut drawn: Local<Option<(SVec2, u64, usize)>>,
    existing: Query<Entity, With<WalkDetailTag>>,
) {
    if *mode != crate::camera::CameraMode::Walk {
        if drawn.take().is_some() {
            despawn_all(&mut commands, &existing);
        }
        return;
    }
    let here = SVec2::new(walk.pos.x, walk.pos.z);
    let key = (sim.world.roads.version, sim.world.buildings.len());
    if let Some((at, rv, nb)) = *drawn {
        if at.distance(here) < nd_procgen::WALK_REDRAW && (rv, nb) == key && !assets.palette_changed && !sim.full_rebuild {
            return;
        }
    }
    despawn_all(&mut commands, &existing);
    let geo = nd_procgen::walk_detail(&sim.world, &assets.palette, here, nd_procgen::WALK_RADIUS);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, WalkDetailTag);
    *drawn = Some((here, key.0, key.1));
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
                .spawn((Mesh3d(mats.citizen_mesh.clone()), MeshMaterial3d(mats.citizen.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, CitizenTag))
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
            let seated = matches!(c.activity, nd_sim::Activity::Sitting { .. });
            tf.translation = Vec3::new(pos.x, if seated { 0.02 } else { 0.16 }, pos.y);
            tf.rotation = Quat::from_rotation_y(-heading);
            tf.scale = if seated { Vec3::new(1.0, 0.62, 1.15) } else { Vec3::ONE };
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
            let mesh = if car.bus.is_some() {
                mats.bus_body_mesh.clone()
            } else if car.cargo.is_some() {
                mats.truck_body_mesh.clone()
            } else {
                mats.car_body_mesh.clone()
            };
            let body = commands.spawn((Mesh3d(mesh), MeshMaterial3d(body_mat), Transform::IDENTITY, NotShadowCaster, CarTag)).id();
            let lights = commands.spawn((Mesh3d(mats.car_lights_mesh.clone()), MeshMaterial3d(mats.glow()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, CarTag)).id();
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
        // Lifted when selected, whether through its driver or, for a bus or
        // truck with nobody to inspect, as the vehicle itself.
        let selected = *selection == Selection::Citizen(car.citizen) || *selection == Selection::Car(*id);
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
    flags: Res<UiFlags>,
) {
    if keys.just_pressed(KeyCode::KeyV) && !flags.keyboard_captured {
        overlays.land_value = !overlays.land_value;
    }
    let lv = &sim.world.land_value;
    if overlays.land_value == state.overlay_shown && lv.version == state.overlay_version && !assets.palette_changed && !sim.full_rebuild {
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
    spawn_overlay(&mut commands, &mut meshes, &mats, &mesh, OverlayTag);
}

/// The pedestrian network inside blocks: loops, alleys and gates. Toggled with P.
fn sync_paths(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut overlays: ResMut<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<PathsTag>>,
    flags: Res<UiFlags>,
) {
    if keys.just_pressed(KeyCode::KeyP) && !flags.keyboard_captured {
        overlays.paths = !overlays.paths;
    }
    let walk = &sim.world.walk;
    if overlays.paths == state.paths_shown && walk.version == state.paths_version && !sim.full_rebuild {
        return;
    }
    state.paths_shown = overlays.paths;
    state.paths_version = walk.version;
    despawn_all(&mut commands, &existing);
    if !overlays.paths {
        return;
    }
    let pal = &assets.palette;
    let mut mesh = MeshData::default();
    for (a, b) in walk.interior_edges() {
        mesh.ground_strip(&[a, b], 0.7, 0.25, scale(pal.preview, 0.5));
    }
    if !mesh.is_empty() {
        spawn_glow(&mut commands, &mut meshes, &mats, &mesh, PathsTag);
    }
}

/// Trains: a locomotive and its coaches or wagons, each a body in the
/// train's colour, placed along the route and blended between ticks.
fn sync_trains(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut q: Query<&mut Transform, With<TrainTag>>,
) {
    let rail = &sim.world.rail;
    for (id, train) in rail.trains.iter() {
        if !state.trains.contains_key(&id) {
            let mut ents = Vec::new();
            for k in 0..=(train.cars as usize) {
                let mesh = match (k, train.kind) {
                    (0, _) => mats.loco_mesh.clone(),
                    (_, nd_sim::TrainKind::Passenger) => mats.coach_mesh.clone(),
                    (_, nd_sim::TrainKind::Freight) => mats.wagon_mesh.clone(),
                };
                let colour = if k == 0 || train.kind == nd_sim::TrainKind::Passenger { train.color as usize } else { (train.color as usize + k * 3) % mats.car_bodies.len().max(1) };
                let mat = mats.car_bodies[colour % mats.car_bodies.len().max(1)].clone();
                ents.push(commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, TrainTag)).id());
            }
            state.trains.insert(id, ents);
        }
    }
    let gone: Vec<nd_sim::TrainId> = state.trains.keys().copied().filter(|id| !rail.trains.contains_key(*id)).collect();
    for id in gone {
        if let Some(ents) = state.trains.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let f = sim.frac();
    for (id, ents) in state.trains.iter() {
        let Some(train) = rail.trains.get(*id) else { continue };
        let s = train.prev_s + (train.s - train.prev_s) * f;
        for (k, e) in ents.iter().enumerate() {
            if let Ok(mut tf) = q.get_mut(*e) {
                let (p, heading) = train.vehicle_pose_at(k, s);
                tf.translation = Vec3::new(p.x, 0.3, p.y);
                tf.rotation = Quat::from_rotation_y(-heading);
            }
        }
    }
}

/// Bus stops: a pole, a sign, a kerb mark and a shelter at every stop of
/// every line, redrawn when a line is edited.
fn sync_transit(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<TransitTag>>,
) {
    let w = &sim.world;
    if w.transit.version == state.transit_version && existing.is_empty() == w.transit.routes.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.transit_version = w.transit.version;
    despawn_all(&mut commands, &existing);
    let mut stops: Vec<(SVec2, SVec2, SVec2)> = Vec::new();
    for route in w.transit.routes.values() {
        for stop in &route.stops {
            if !w.roads.segment(stop.segment).alive {
                continue;
            }
            let (a, b) = w.roads.segment_points(stop.segment);
            let mut d = (b - a).normalize_or_zero();
            let mut right = SVec2::new(-d.y, d.x);
            let (_, q) = nd_sim::roads::project_on_segment(stop.pos, a, b);
            if (stop.pos - q).dot(right) < 0.0 {
                d = -d;
                right = -right;
            }
            stops.push((stop.pos, d, right));
        }
    }
    if stops.is_empty() {
        return;
    }
    let geo = bus_stops(&stops, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, TransitTag);
}

/// Brown haze over coal plants: stacked translucent layers that read as a
/// dome from above and as murk from the street.
fn sync_haze(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<HazeTag>>,
) {
    let w = &sim.world;
    let coal: Vec<SVec2> = w.buildings.values().filter(|b| b.plant.map_or(false, |k| k.makes_haze())).filter_map(|b| w.lots.get(b.lot).map(|l| l.center)).collect();
    let signature = coal.iter().fold(coal.len(), |acc, c| acc ^ ((c.x * 7.0 + c.y * 13.0) as i64 as usize));
    if signature == state.haze_signature {
        return;
    }
    state.haze_signature = signature;
    despawn_all(&mut commands, &existing);
    if coal.is_empty() {
        return;
    }
    let mut mesh = MeshData::default();
    for c in coal {
        // Four thin layers, light enough that the ground still reads through;
        // the plume drifts the upper layers a little downwind.
        for (k, (h, r)) in [(22.0f32, 95.0f32), (42.0, 135.0), (66.0, 165.0), (92.0, 185.0)].iter().enumerate() {
            let drift = SVec2::new(18.0, 8.0) * k as f32;
            let poly: Vec<SVec2> = (0..28).map(|i| {
                let a = i as f32 / 28.0 * std::f32::consts::TAU;
                c + drift + SVec2::new(a.cos(), a.sin()) * *r
            }).collect();
            let alpha = 0.075 - k as f32 * 0.012;
            mesh.ground_polygon(&poly, *h, [0.52, 0.40, 0.27, alpha]);
        }
    }
    spawn_overlay(&mut commands, &mut meshes, &mats, &mesh, HazeTag);
}

fn preview_width(kind: nd_sim::roads::RoadKind, fwd: u8, bwd: u8) -> f32 {
    match kind {
        nd_sim::roads::RoadKind::Path => nd_sim::roads::PATH_HALF_WIDTH * 2.0,
        nd_sim::roads::RoadKind::Rail => nd_sim::roads::RAIL_HALF_WIDTH * 2.0,
        nd_sim::roads::RoadKind::Street => (fwd + bwd) as f32 * LANE_WIDTH + SHOULDER * 2.0,
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Sun, ambient light, sky, fog and glow strength follow the sim clock.
///
/// Everything is written only when the quantised time of day moves, which
/// is far below a visible step, and the shared Glow material (whose rewrite re-uploads it and breaks the
/// batches of every glow mesh) is rounded to 1/64 of its range, so a full
/// day costs at most a few dozen material updates instead of one per frame.
fn update_daylight(
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<SunTag>>,
    mut fog: Query<&mut DistanceFog>,
) {
    let pal = &assets.palette;
    let t = if lighting.follow_clock { sim.world.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    // Quantised to 1/4096 of a day (about 20 sim seconds, a 0.09 degree
    // step of the sun), so at 1x speed the lighting is written a few times a
    // second rather than every frame.
    let daylight_key = (t * 4096.0).round() as i32;
    if state.daylight_key == Some(daylight_key) && !assets.palette_changed {
        return;
    }
    state.daylight_key = Some(daylight_key);
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

    let glow_key = ((1.0 - 0.7 * daylight) * 64.0).round() as i32;
    if state.glow_key != Some(glow_key) || assets.palette_changed {
        state.glow_key = Some(glow_key);
        let glow_k = glow_key as f32 / 64.0;
        if let Some(mut m) = materials.get_mut(&mats.glow()) {
            m.base_color = Color::linear_rgb(glow_k, glow_k, glow_k);
        }
    }
}

// ── landscape: ground, water, waterworks, ships, and setting things on the ground ──

/// Marker for anything that moves and should stand on the ground (hills,
/// bridge decks): add it to a moving entity and `lift_movers` raises it each
/// frame after its pose is written. Citizens, cars and trains have it implied.
#[derive(Component)]
pub struct Grounded;

/// The lift `lift_movers` last applied, and the height it left the entity
/// at, so a pose not rewritten this frame is not lifted twice.
#[derive(Component)]
struct Lifted {
    after: f32,
    lift: f32,
}

#[derive(Component)]
struct WaterTag;
#[derive(Component, Clone)]
struct WaterworksTag;
#[derive(Component)]
struct ShipTag;

#[derive(Resource, Default)]
struct LandscapeRender {
    /// What the ground, water and works were built for.
    signature: Option<u64>,
    water_mat: Option<Handle<StandardMaterial>>,
    ship_meshes: Option<[Handle<Mesh>; 2]>,
    ships: HashMap<nd_sim::ports::ShipId, Entity>,
}

fn landscape_signature(w: &World) -> u64 {
    let mut h = DefaultHasher::new();
    w.landscape.params.seed.hash(&mut h);
    format!("{:?}", w.landscape.params.preset).hash(&mut h);
    w.landscape.rivers.len().hash(&mut h);
    w.roads.version.hash(&mut h);
    w.lots.len().hash(&mut h);
    w.ports.ports.len().hash(&mut h);
    w.buildings.values().filter(|b| b.plant.is_some()).count().hash(&mut h);
    w.landscape.is_flat().hash(&mut h);
    h.finish()
}

/// The ground of a landscape (graded to the roads and lots), the water
/// sheet, and the bridges, piers, weirs and vents. Rebuilt when roads, lots,
/// docks or plants change, since the ground is graded to them.
#[allow(clippy::too_many_arguments)]
fn sync_landscape(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<LandscapeRender>,
    terrain: Query<Entity, With<TerrainTag>>,
    water: Query<Entity, With<WaterTag>>,
    works: Query<Entity, With<WaterworksTag>>,
) {
    let w = &sim.world;
    let sig = landscape_signature(w);
    if state.signature == Some(sig) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.signature = Some(sig);
    despawn_all(&mut commands, &water);
    despawn_all(&mut commands, &works);
    if w.landscape.is_flat() {
        return;
    }
    despawn_all(&mut commands, &terrain);
    let ground = nd_procgen::terrain::build_landscape_terrain(w, &assets.palette, 8.0);
    commands.spawn((Mesh3d(meshes.add(to_mesh(&ground, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, TerrainTag));
    let water_mat = state
        .water_mat
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.06,
                reflectance: 0.6,
                metallic: 0.1,
                ..default()
            })
        })
        .clone();
    let sheet = nd_procgen::terrain::build_water(w, &assets.palette);
    if !sheet.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&sheet, false))), MeshMaterial3d(water_mat), Transform::IDENTITY, NotShadowCaster, WaterTag));
    }
    let geo = nd_procgen::terrain::waterworks(w, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, WaterworksTag);
}

/// Freighters and ferries along their routes, blended between ticks.
fn sync_ships(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: ResMut<LandscapeRender>, mut q: Query<&mut Transform, With<ShipTag>>) {
    let ships = &sim.world.ports.ships;
    if ships.is_empty() && state.ships.is_empty() {
        return;
    }
    let handles = state
        .ship_meshes
        .get_or_insert_with(|| [meshes.add(shared_mesh(&nd_procgen::terrain::ship_mesh(true, &assets.palette), true)), meshes.add(shared_mesh(&nd_procgen::terrain::ship_mesh(false, &assets.palette), true))])
        .clone();
    for (id, ship) in ships.iter() {
        if !state.ships.contains_key(&id) {
            let mesh = if ship.kind == nd_sim::ShipKind::Freighter { handles[0].clone() } else { handles[1].clone() };
            let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mats.surface(Surface::Panel)), Transform::IDENTITY, ShipTag)).id();
            state.ships.insert(id, e);
        }
    }
    let gone: Vec<nd_sim::ports::ShipId> = state.ships.keys().copied().filter(|id| !ships.contains_key(*id)).collect();
    for id in gone {
        if let Some(e) = state.ships.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    let f = sim.frac();
    for (id, e) in state.ships.iter() {
        let Some(ship) = ships.get(*id) else { continue };
        let s = ship.prev_s + (ship.s - ship.prev_s) * f;
        let (p, dir) = ship.pose_at(s);
        if let Ok(mut tf) = q.get_mut(*e) {
            tf.translation = Vec3::new(p.x, nd_sim::landscape::WATER_LEVEL, p.y);
            tf.rotation = Quat::from_rotation_y(-dir.y.atan2(dir.x));
        }
    }
}

/// Set freshly spawned flat-datum meshes (streets, lot fills, previews,
/// overlays, parks, vacant dressing, stops, scatter) on the built ground,
/// and signal lamps with them. Buildings are raised to their pad when they
/// are generated.
#[allow(clippy::type_complexity)]
fn lift_spawned(
    sim: Res<Sim>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<&Mesh3d, Or<(Added<RoadsTag>, Added<LotsTag>, Added<PreviewTag>, Added<ParkTag>, Added<VacantTag>, Added<OverlayTag>, Added<PathsTag>, Added<TransitTag>, Added<HazeTag>, Added<ScatterTag>)>>,
    mut signals: Query<&mut Transform, Added<SignalTag>>,
) {
    let w = &sim.world;
    if w.landscape.is_flat() {
        return;
    }
    for m in &q {
        let Some(mut mesh) = meshes.get_mut(&m.0) else { continue };
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            for p in pos.iter_mut() {
                p[1] += w.surface_height(SVec2::new(p[0], p[2]));
            }
        }
    }
    for mut tf in &mut signals {
        tf.translation.y += w.surface_height(SVec2::new(tf.translation.x, tf.translation.z));
    }
}

/// Raise movers onto the ground after their poses are written.
#[allow(clippy::type_complexity)]
fn lift_movers(mut commands: Commands, sim: Res<Sim>, mut q: Query<(Entity, &mut Transform, Option<&mut Lifted>), Or<(With<CitizenTag>, With<CarTag>, With<TrainTag>, With<Grounded>)>>) {
    let w = &sim.world;
    if w.landscape.is_flat() {
        return;
    }
    for (e, mut tf, lifted) in &mut q {
        let base = match &lifted {
            // Not rewritten since the last lift: take the old lift off.
            Some(l) if tf.translation.y == l.after => tf.translation.y - l.lift,
            _ => tf.translation.y,
        };
        let lift = w.surface_height(SVec2::new(tf.translation.x, tf.translation.z));
        tf.translation.y = base + lift;
        let after = tf.translation.y;
        match lifted {
            Some(mut l) => {
                l.after = after;
                l.lift = lift;
            }
            None => {
                commands.entity(e).insert(Lifted { after, lift });
            }
        }
    }
}
