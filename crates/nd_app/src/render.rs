//! Turns simulation state into Bevy entities. Static geometry (terrain, roads,
//! lots, zones) is rebuilt as whole meshes when it changes; buildings get two
//! entities each (lit body + unlit glow); citizens are small glowing markers
//! moved every frame.

use bevy::asset::RenderAssetUsages;
use bevy::light::GlobalAmbientLight;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use nd_procgen::{build_terrain, generate_building, MeshData, TerrainConfig};
use nd_sim::{BuildingId, CitizenId, SimEvent, Vec2 as SVec2};
use std::collections::HashMap;

use crate::assets::GameAssets;
use crate::sim::{Sim, SimSet};
use crate::tools::Selection;

#[derive(Component)]
struct TerrainTag;
#[derive(Component)]
struct RoadsTag;
#[derive(Component)]
struct LotsTag;
#[derive(Component)]
pub struct BuildingTag(pub BuildingId);
#[derive(Component)]
pub struct CitizenTag(pub CitizenId);

#[derive(Resource)]
struct Materials {
    solid: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
    ground: Handle<StandardMaterial>,
    citizen: Handle<StandardMaterial>,
    citizen_spotlight: Handle<StandardMaterial>,
    citizen_selected: Handle<StandardMaterial>,
    citizen_mesh: Handle<Mesh>,
}

#[derive(Resource, Default)]
struct RenderState {
    roads_version: u64,
    lots_signature: (usize, usize, usize),
    buildings: HashMap<BuildingId, [Entity; 2]>,
    citizens: HashMap<CitizenId, Entity>,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderState>()
            .add_systems(Startup, setup)
            .add_systems(Update, (sync_terrain, sync_roads, sync_lots, sync_buildings, sync_citizens).chain().in_set(SimSet::Sync));
    }
}

pub fn to_mesh(data: &MeshData) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
        .with_inserted_indices(Indices::U32(data.indices.clone()))
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>, assets: Res<GameAssets>) {
    let pal = &assets.palette;
    let solid = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, metallic: 0.15, ..default() });
    let glow = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
    let ground = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 1.0, ..default() });
    let c = pal.citizen;
    let citizen = materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), unlit: true, ..default() });
    let s = pal.citizen_spotlight;
    let citizen_spotlight = materials.add(StandardMaterial { base_color: Color::linear_rgb(s[0], s[1], s[2]), unlit: true, ..default() });
    let citizen_selected = materials.add(StandardMaterial { base_color: Color::linear_rgb(8.0, 8.0, 8.0), unlit: true, ..default() });
    let citizen_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 2.0, 1.0), 0.0, [1.0, 1.0, 1.0, 1.0]);
        meshes.add(to_mesh(&m))
    };
    commands.insert_resource(Materials { solid, glow, ground, citizen, citizen_spotlight, citizen_selected, citizen_mesh });

    commands.insert_resource(GlobalAmbientLight { color: Color::linear_rgb(0.35, 0.45, 0.9), brightness: 70.0, ..default() });
    commands.spawn((
        DirectionalLight { color: Color::linear_rgb(0.45, 0.55, 1.0), illuminance: 700.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(300.0, 500.0, -200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
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
    for e in &existing {
        commands.entity(e).despawn();
    }
    let data = build_terrain(&TerrainConfig::default(), &assets.palette);
    commands.spawn((Mesh3d(meshes.add(to_mesh(&data))), MeshMaterial3d(mats.ground.clone()), Transform::IDENTITY, TerrainTag));
}

fn road_meshes(sim: &Sim, assets: &GameAssets) -> (MeshData, MeshData) {
    let pal = &assets.palette;
    let roads = &sim.world.roads;
    let mut asphalt = MeshData::default();
    let mut glow = MeshData::default();
    let asphalt_c = [pal.asphalt[0], pal.asphalt[1], pal.asphalt[2], 1.0];
    let marker_c = [pal.lane_marker[0], pal.lane_marker[1], pal.lane_marker[2], 1.0];
    let dim_marker = [pal.lane_marker[0] * 0.45, pal.lane_marker[1] * 0.45, pal.lane_marker[2] * 0.45, 1.0];

    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let hw = seg.half_width();
        asphalt.ground_strip(&[a, b], hw * 2.0, 0.05, asphalt_c);
        // Glowing edge lines.
        let d = (b - a).normalize_or_zero();
        let n = SVec2::new(-d.y, d.x) * (hw - 0.6);
        glow.ground_strip(&[a + n, b + n], 0.25, 0.09, dim_marker);
        glow.ground_strip(&[a - n, b - n], 0.25, 0.09, dim_marker);
        // Dashed centre line.
        if seg.lanes >= 2 {
            let len = a.distance(b);
            let mut t = 2.0;
            while t + 2.5 < len {
                glow.ground_strip(&[a + d * t, a + d * (t + 2.5)], 0.3, 0.09, marker_c);
                t += 6.0;
            }
        }
    }
    // Intersection pads hide the seams where segments meet.
    for node in &roads.nodes {
        let alive: Vec<_> = node.segments.iter().filter(|&&s| roads.segment(s).alive).collect();
        if alive.len() < 2 {
            continue;
        }
        let r = alive.iter().map(|&&s| roads.segment(s).half_width()).fold(0.0, f32::max);
        let poly: Vec<SVec2> = (0..16)
            .map(|i| {
                let a = i as f32 / 16.0 * std::f32::consts::TAU;
                node.pos + SVec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        asphalt.ground_polygon(&poly, 0.06, asphalt_c);
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
    for e in &existing {
        commands.entity(e).despawn();
    }
    let (asphalt, glow) = road_meshes(&sim, &assets);
    if !asphalt.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&asphalt))), MeshMaterial3d(mats.solid.clone()), Transform::IDENTITY, RoadsTag));
    }
    if !glow.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&glow))), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, RoadsTag));
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
    let signature = (w.lots.len(), vacant, w.zones.len());
    if signature == state.lots_signature && !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.lots_signature = signature;
    for e in &existing {
        commands.entity(e).despawn();
    }
    let pal = &assets.palette;
    let mut glow = MeshData::default();
    let mut fill = MeshData::default();
    for zone in w.zones.values() {
        let c = pal.zone_color(zone.land_use);
        let mut outline = zone.polygon.clone();
        outline.push(zone.polygon[0]);
        glow.ground_strip(&outline, 0.6, 0.04, [c[0] * 1.2, c[1] * 1.2, c[2] * 1.2, 1.0]);
    }
    for lot in w.lots.values() {
        let c = pal.zone_color(lot.land_use);
        if lot.building.is_none() {
            let mut outline = lot.corners.to_vec();
            outline.push(lot.corners[0]);
            glow.ground_strip(&outline, 0.25, 0.035, [c[0] * 0.5, c[1] * 0.5, c[2] * 0.5, 1.0]);
            fill.ground_polygon(&lot.corners, 0.02, [c[0] * 0.08 + pal.lot_vacant[0], c[1] * 0.08 + pal.lot_vacant[1], c[2] * 0.08 + pal.lot_vacant[2], 1.0]);
        } else {
            fill.ground_polygon(&lot.corners, 0.02, [pal.curb[0], pal.curb[1], pal.curb[2], 1.0]);
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
    let geo = generate_building(lot, b.land_use, b.floors, b.seed, &assets.rules, &assets.palette);
    let solid = commands
        .spawn((Mesh3d(meshes.add(to_mesh(&geo.solid))), MeshMaterial3d(mats.solid.clone()), Transform::IDENTITY, BuildingTag(id)))
        .id();
    let glow = commands
        .spawn((Mesh3d(meshes.add(to_mesh(&geo.glow))), MeshMaterial3d(mats.glow.clone()), Transform::IDENTITY, BuildingTag(id)))
        .id();
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
        let new_ids: Vec<BuildingId> = sim
            .events
            .iter()
            .filter_map(|e| match e {
                SimEvent::BuildingBuilt(id) => Some(*id),
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
    // Spawn markers for new citizens.
    for (id, _) in w.citizens.iter() {
        if !state.citizens.contains_key(&id) {
            let e = commands
                .spawn((Mesh3d(mats.citizen_mesh.clone()), MeshMaterial3d(mats.citizen.clone()), Transform::IDENTITY, Visibility::Hidden, CitizenTag(id)))
                .id();
            state.citizens.insert(id, e);
        }
    }
    // Remove markers for citizens that left.
    let gone: Vec<CitizenId> = state.citizens.keys().copied().filter(|id| !w.citizens.contains_key(*id)).collect();
    for id in gone {
        if let Some(e) = state.citizens.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    // Update positions.
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
