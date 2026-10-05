//! Turns simulation state into Bevy entities. Static geometry (terrain, roads,
//! lots, zones, project preview) is rebuilt as whole meshes when it changes;
//! buildings get two entities each (lit body + unlit glow); citizens and cars
//! are small glowing markers moved every frame.

use bevy::asset::RenderAssetUsages;
use bevy::light::GlobalAmbientLight;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use nd_procgen::{build_terrain, generate_building, MeshData, TerrainConfig};
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
#[derive(Component)]
pub struct BuildingTag(pub BuildingId);
#[derive(Component)]
pub struct CitizenTag(pub CitizenId);
#[derive(Component)]
pub struct CarTag(pub CarId);

#[derive(Resource)]
struct Materials {
    solid: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
    ground: Handle<StandardMaterial>,
    citizen: Handle<StandardMaterial>,
    citizen_spotlight: Handle<StandardMaterial>,
    citizen_selected: Handle<StandardMaterial>,
    citizen_mesh: Handle<Mesh>,
    car_bodies: Vec<Handle<StandardMaterial>>,
    car_body_mesh: Handle<Mesh>,
    truck_body_mesh: Handle<Mesh>,
    car_lights_mesh: Handle<Mesh>,
    signal_green: Handle<StandardMaterial>,
    signal_red: Handle<StandardMaterial>,
    signal_mesh: Handle<Mesh>,
    overlay: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct RenderState {
    roads_version: u64,
    lanes_version: u64,
    plan_version: u64,
    lots_signature: (usize, usize, usize, u64),
    buildings: HashMap<BuildingId, [Entity; 2]>,
    citizens: HashMap<CitizenId, Entity>,
    cars: HashMap<CarId, [Entity; 2]>,
    overlay_version: u64,
    overlay_shown: bool,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderState>().init_resource::<Overlays>().add_systems(Startup, setup).add_systems(
            Update,
            (sync_terrain, sync_roads, sync_signals, sync_preview, sync_lots, sync_overlay, sync_buildings, sync_citizens, sync_cars, update_daylight)
                .chain()
                .in_set(SimSet::Sync),
        );
    }
}

pub fn to_mesh(data: &MeshData) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
        .with_inserted_indices(Indices::U32(data.indices.clone()))
}

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn scale(c: [f32; 3], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>, assets: Res<GameAssets>) {
    let pal = &assets.palette;
    let unlit = |c: [f32; 3]| StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), unlit: true, ..default() };
    let solid = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, metallic: 0.15, ..default() });
    let glow = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
    let ground = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 1.0, ..default() });
    let citizen = materials.add(unlit(pal.citizen));
    let citizen_spotlight = materials.add(unlit(pal.citizen_spotlight));
    let citizen_selected = materials.add(unlit([8.0, 8.0, 8.0]));
    let citizen_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 2.0, 1.0), 0.0, [1.0, 1.0, 1.0, 1.0]);
        meshes.add(to_mesh(&m))
    };
    let car_bodies = pal
        .car_colors
        .iter()
        .map(|&c| materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), perceptual_roughness: 0.4, metallic: 0.6, ..default() }))
        .collect();
    let car_body_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.7, 0.0), Vec3::new(4.2, 1.0, 1.9), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(-0.3, 1.45, 0.0), Vec3::new(2.2, 0.6, 1.6), 0.0, [0.6, 0.6, 0.7, 1.0]);
        meshes.add(to_mesh(&m))
    };
    let truck_body_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(-0.9, 1.4, 0.0), Vec3::new(5.6, 2.6, 2.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(3.0, 1.1, 0.0), Vec3::new(2.0, 2.0, 2.2), 0.0, [0.5, 0.5, 0.55, 1.0]);
        meshes.add(to_mesh(&m))
    };
    let car_lights_mesh = {
        let mut m = MeshData::default();
        for z in [-0.6, 0.6] {
            m.boxed(Vec3::new(2.1, 0.7, z), Vec3::new(0.15, 0.3, 0.4), 0.0, rgba(pal.headlight));
            m.boxed(Vec3::new(-2.1, 0.75, z), Vec3::new(0.15, 0.25, 0.4), 0.0, rgba(pal.taillight));
        }
        meshes.add(to_mesh(&m))
    };
    let overlay = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let signal_green = materials.add(unlit(pal.signal_green));
    let signal_red = materials.add(unlit(pal.signal_red));
    let signal_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.35, 0.12, LANE_WIDTH * 0.8), 0.0, [1.0, 1.0, 1.0, 1.0]);
        meshes.add(to_mesh(&m))
    };
    commands.insert_resource(Materials {
        solid,
        glow,
        ground,
        citizen,
        citizen_spotlight,
        citizen_selected,
        citizen_mesh,
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
    commands.spawn((Mesh3d(meshes.add(to_mesh(&data))), MeshMaterial3d(mats.ground.clone()), Transform::IDENTITY, TerrainTag));
}

/// Dashed strip along a line.
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

fn road_meshes(sim: &Sim, assets: &GameAssets) -> (MeshData, MeshData) {
    let pal = &assets.palette;
    let roads = &sim.world.roads;
    let net = &sim.world.traffic.net;
    let mut asphalt = MeshData::default();
    let mut glow = MeshData::default();
    let asphalt_c = rgba(pal.asphalt);
    let edge_c = rgba(pal.lane_marker);
    let centre_c = rgba(pal.centre_line);
    let divider_c = rgba(pal.lane_divider);
    let pole_c = [0.16, 0.16, 0.17, 1.0];
    let lamp_c = rgba(pal.street_lamp);

    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let hw = seg.half_width();
        asphalt.ground_strip(&[a, b], hw * 2.0, 0.05, asphalt_c);
        let d = (b - a).normalize_or_zero();
        let right = SVec2::new(d.y, -d.x);
        // Edge lines just inside the shoulder.
        let e = right * (hw - SHOULDER * 0.5);
        glow.ground_strip(&[a + e, b + e], 0.22, 0.09, edge_c);
        glow.ground_strip(&[a - e, b - e], 0.22, 0.09, edge_c);
        // Lane boundaries, measured from the centreline: fwd lanes on the right.
        let total = seg.lanes() as f32;
        let centre_offset = (seg.lanes_fwd as f32 - seg.lanes_bwd as f32) * 0.5 * LANE_WIDTH;
        // Boundary k sits at offset (k - lanes_bwd) * LANE_WIDTH from the centreline shifted so the
        // road is symmetric about the segment line.
        // Street lamps every 28 m, alternating sides.
        let len = a.distance(b);
        let mut t = 14.0;
        let mut side = 1.0;
        while t < len - 6.0 {
            let p = a + d * t + right * side * (hw + 0.7);
            asphalt.boxed(bevy::math::Vec3::new(p.x, 3.5, p.y), bevy::math::Vec3::new(0.22, 7.0, 0.22), 0.0, pole_c);
            let arm = p - right * side * 1.2;
            asphalt.boxed(bevy::math::Vec3::new((p.x + arm.x) * 0.5, 7.0, (p.y + arm.y) * 0.5), bevy::math::Vec3::new(1.3, 0.12, 0.12), -d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2, pole_c);
            glow.boxed(bevy::math::Vec3::new(arm.x, 6.9, arm.y), bevy::math::Vec3::new(0.5, 0.18, 0.35), 0.0, lamp_c);
            t += 28.0;
            side = -side;
        }
        for k in 1..seg.lanes() as u32 {
            let off = (k as f32 - total * 0.5) * LANE_WIDTH;
            let p = right * off;
            let is_centre = seg.lanes_fwd > 0 && seg.lanes_bwd > 0 && (off - (-centre_offset)).abs() < 0.01;
            if is_centre {
                glow.ground_strip(&[a + p + right * 0.2, b + p + right * 0.2], 0.16, 0.09, centre_c);
                glow.ground_strip(&[a + p - right * 0.2, b + p - right * 0.2], 0.16, 0.09, centre_c);
            } else {
                dashes(&mut glow, a + p, b + p, 0.18, 0.09, 3.0, 4.0, divider_c);
            }
        }
    }
    // Intersection pads hide the seams where segments meet.
    for (i, node) in roads.nodes.iter().enumerate() {
        let alive = node.segments.iter().filter(|&&s| roads.segment(s).alive).count();
        if alive == 0 {
            continue;
        }
        let r = net.node_radius.get(i).copied().unwrap_or(4.0) + 0.3;
        let poly: Vec<SVec2> = (0..20)
            .map(|k| {
                let a = k as f32 / 20.0 * std::f32::consts::TAU;
                node.pos + SVec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        asphalt.ground_polygon(&poly, 0.06, asphalt_c);
        if alive >= 3 {
            // Faint crosswalk-style ring so junctions read from above.
            let mut ring = poly.clone();
            ring.push(poly[0]);
            glow.ground_strip(&ring, 0.3, 0.085, scale(pal.lane_marker, 0.6));
        }
    }
    (asphalt, glow)
}

fn sync_roads(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<RoadsTag>>,
) {
    let version = sim.world.roads.version;
    if version == state.roads_version && !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.roads_version = version;
    despawn_all(&mut commands, &existing);
    let (asphalt, glow) = road_meshes(&sim, &assets);
    if !asphalt.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&asphalt))), MeshMaterial3d(mats.solid.clone()), Transform::IDENTITY, RoadsTag));
    }
    if !glow.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&glow))), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, RoadsTag));
    }
}

/// A glowing stop bar at the end of every lane feeding a signalised junction,
/// coloured by whether that lane currently has a green connector.
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
        for inter in &net.intersections {
            if !inter.signalized {
                continue;
            }
            let mut seen = std::collections::HashSet::new();
            for &(conn, _) in &inter.connectors {
                for (lid, lane) in net.lanes.iter().enumerate() {
                    if lane.next.contains(&conn) && !lane.is_connector() && seen.insert(lid) {
                        let (p, d) = lane.point_at(lane.length - 0.6);
                        let heading = d.y.atan2(d.x);
                        commands.spawn((
                            Mesh3d(mats.signal_mesh.clone()),
                            MeshMaterial3d(mats.signal_red.clone()),
                            Transform::from_xyz(p.x, 0.0, p.y).with_rotation(Quat::from_rotation_y(-heading)),
                            SignalTag(lid as LaneId),
                        ));
                    }
                }
            }
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
    for (_, pts, fwd, bwd) in &pv.planned_roads {
        let w = (*fwd + *bwd) as f32 * LANE_WIDTH + SHOULDER * 2.0;
        glow.ground_strip(pts, w, 0.12, scale(pal.preview, 0.35));
        glow.ground_strip(pts, 0.5, 0.13, rgba(pal.preview));
    }
    for (_, pts, fwd, bwd) in &pv.removed_roads {
        let w = (*fwd + *bwd) as f32 * LANE_WIDTH + SHOULDER * 2.0;
        glow.ground_strip(pts, w, 0.12, scale(pal.preview_remove, 0.35));
        dashes(&mut glow, pts[0], *pts.last().unwrap(), 0.6, 0.13, 4.0, 3.0, rgba(pal.preview_remove));
    }
    for (_, poly, lu) in &pv.planned_zones {
        let c = pal.zone_color(*lu);
        let mut outline = poly.clone();
        outline.push(poly[0]);
        glow.ground_strip(&outline, 0.8, 0.12, scale(c, 1.3));
        glow.ground_polygon(poly, 0.045, scale(c, 0.12));
    }
    for (_, poly) in &pv.removed_zones {
        let mut outline = poly.clone();
        outline.push(poly[0]);
        glow.ground_strip(&outline, 0.8, 0.12, rgba(pal.preview_remove));
    }
    if !glow.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&glow))), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, PreviewTag));
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
        glow.ground_strip(&outline, 0.6, 0.04, scale(c, 1.2));
    }
    for lot in w.lots.values() {
        let c = pal.zone_color(lot.land_use);
        if lot.building.is_none() {
            let mut outline = lot.corners.to_vec();
            outline.push(lot.corners[0]);
            glow.ground_strip(&outline, 0.25, 0.035, scale(c, 0.5));
            fill.ground_polygon(&lot.corners, 0.02, [c[0] * 0.08 + pal.lot_vacant[0], c[1] * 0.08 + pal.lot_vacant[1], c[2] * 0.08 + pal.lot_vacant[2], 1.0]);
        } else {
            fill.ground_polygon(&lot.corners, 0.02, rgba(pal.curb));
        }
    }
    if !fill.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&fill))), MeshMaterial3d(mats.solid.clone()), Transform::IDENTITY, LotsTag));
    }
    if !glow.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&glow))), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, LotsTag));
    }
}

fn spawn_building(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, sim: &Sim, assets: &GameAssets, id: BuildingId) -> Option<[Entity; 2]> {
    let b = sim.world.buildings.get(id)?;
    let lot = sim.world.lots.get(b.lot)?;
    let geo = generate_building(lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &assets.rules, &assets.palette);
    let solid = commands.spawn((Mesh3d(meshes.add(to_mesh(&geo.solid))), MeshMaterial3d(mats.solid.clone()), Transform::IDENTITY, BuildingTag(id))).id();
    let glow = commands.spawn((Mesh3d(meshes.add(to_mesh(&geo.glow))), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, BuildingTag(id))).id();
    Some([solid, glow])
}

fn sync_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sim: ResMut<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
) {
    let rebuild_all = sim.full_rebuild || assets.rules_changed || assets.palette_changed;
    if rebuild_all {
        for (_, ents) in state.buildings.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        let ids: Vec<BuildingId> = sim.world.buildings.keys().collect();
        for id in ids {
            if let Some(ents) = spawn_building(&mut commands, &mut meshes, &mats, &sim, &assets, id) {
                state.buildings.insert(id, ents);
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
            if let Some(ents) = spawn_building(&mut commands, &mut meshes, &mats, &sim, &assets, id) {
                state.buildings.insert(id, ents);
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
    mut q: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>), With<CitizenTag>>,
) {
    let w = &sim.world;
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
        let Ok((mut tf, mut vis, mut mat)) = q.get_mut(e) else { continue };
        if c.is_visible() {
            *vis = Visibility::Visible;
            tf.translation = Vec3::new(c.pos.x, 0.1, c.pos.y);
            tf.rotation = Quat::from_rotation_y(-c.heading);
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
            let lights = commands.spawn((Mesh3d(mats.car_lights_mesh.clone()), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, CarTag(id))).id();
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
                tf.translation = Vec3::new(pos.x, if selected { 0.4 } else { 0.0 }, pos.y);
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
    commands.spawn((Mesh3d(meshes.add(to_mesh(&mesh))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, OverlayTag));
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
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<SunTag>>,
    mut fog: Query<&mut DistanceFog>,
) {
    let pal = &assets.palette;
    let t = sim.world.clock.day_fraction();
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    let daylight = smoothstep(-0.12, 0.30, elev);
    let low_sun = 1.0 - smoothstep(0.05, 0.45, elev.abs());
    let dusk = (1.0 - smoothstep(0.0, 0.22, elev.abs())) * 0.8;

    let sun_color = lerp3(lerp3(pal.sun_low, pal.sun, 1.0 - low_sun), pal.moon, 1.0 - daylight);
    let illuminance = 500.0 + 9500.0 * daylight;
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
    ambient.brightness = 70.0 + 700.0 * daylight;
    let amb = lerp3([0.35, 0.45, 0.9], [0.8, 0.85, 1.0], daylight);
    ambient.color = Color::linear_rgb(amb[0], amb[1], amb[2]);

    let glow_k = 1.0 - 0.7 * daylight;
    if let Some(mut m) = materials.get_mut(&mats.glow) {
        m.base_color = Color::linear_rgb(glow_k, glow_k, glow_k);
    }
}
