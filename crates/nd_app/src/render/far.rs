//! The world beyond the map: the ground and the water out past the drawn
//! square, so the highway, the trunk line and the roads off the map edge run
//! on into the fog instead of ending in the void. Built from the landscape
//! and the ways that reach out there, and rebuilt only when those change.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use nd_procgen::Surface;
use nd_sim::landscape::{FAR_GROUND, LAND_HALF};
use nd_sim::{Vec2 as SVec2, World};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{despawn_all, terrain_config, to_mesh, Materials};
use crate::assets::GameAssets;
use crate::sim::Sim;

/// Grid spacing of the far ground.
const FAR_CELL: f32 = 25.0;

#[derive(Component)]
pub(super) struct FarTag;

#[derive(Resource, Default)]
pub(super) struct FarRender {
    key: Option<u64>,
    /// [refactor/app] The counters `key` was hashed under: the landscape,
    /// the roads, and the built ground (set with the roads' heights).
    gate: super::Gate<(u64, u64, u64, u64, u32)>,
    water_mat: Option<Handle<StandardMaterial>>,
}

/// Ways that reach out of the city into the hills (flat map) or past the
/// built-ground raster (landscape): the far ground is graded to them.
pub(super) fn far_ways(w: &World) -> Vec<(SVec2, SVec2, f32)> {
    let reach = if w.landscape.is_flat() { w.config.half_extent + 50.0 } else { w.ground_half() - 50.0 };
    let out = |p: SVec2| if w.landscape.is_flat() { p.length() > reach } else { p.x.abs().max(p.y.abs()) > reach };
    w.roads
        .alive_segments()
        .filter_map(|(id, s)| {
            let (a, b) = w.roads.segment_points(id);
            (out(a) || out(b)).then(|| (a, b, s.half_width()))
        })
        .collect()
}

fn far_key(w: &World) -> u64 {
    let mut h = DefaultHasher::new();
    w.landscape.params.seed.hash(&mut h);
    format!("{:?}", w.landscape.params.preset).hash(&mut h);
    w.landscape.is_flat().hash(&mut h);
    w.landscape.rivers.len().hash(&mut h);
    for (a, b, hw) in far_ways(w) {
        for v in [a.x, a.y, b.x, b.y, hw] {
            (v.round() as i32).hash(&mut h);
        }
    }
    if !w.landscape.is_flat() {
        // The level the ways run at out there follows the ground at the edge.
        for (_, s) in w.roads.alive_segments() {
            for n in [s.a, s.b] {
                let p = w.roads.node(n).pos;
                if p.x.abs().max(p.y.abs()) > LAND_HALF - 300.0 {
                    (w.road_z.get(n.0 as usize).copied().unwrap_or(0.0).round() as i32).hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_far(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<FarRender>,
    existing: Query<Entity, With<FarTag>>,
) {
    let w = &sim.world;
    if state.gate.same((sim.generation, w.landscape.version(), w.roads.version(), w.ground.version(), w.config.half_extent.to_bits())) && state.key.is_some() && !assets.palette_changed && !sim.full_rebuild && !existing.is_empty() {
        return;
    }
    let key = far_key(w);
    if state.key == Some(key) && !assets.palette_changed && !sim.full_rebuild && !existing.is_empty() {
        return;
    }
    state.key = Some(key);
    despawn_all(&mut commands, &existing);
    let pal = &assets.palette;
    let ground = if w.landscape.is_flat() {
        let cfg = terrain_config(w);
        let height = nd_procgen::flat_height(&cfg);
        nd_procgen::build_far_ground(LAND_HALF, FAR_GROUND, FAR_CELL, &height, &|_| false, pal)
    } else {
        let land = &w.landscape;
        nd_procgen::build_far_ground(LAND_HALF, FAR_GROUND, FAR_CELL, &|p| w.ground_height(p), &|p| land.is_water(p), pal)
    };
    commands.spawn((Mesh3d(meshes.add(to_mesh(&ground, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, FarTag));
    let water = nd_procgen::terrain::build_far_water(w, pal);
    if !water.is_empty() {
        let mat = state
            .water_mat
            .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.06, reflectance: 0.6, metallic: 0.1, ..default() }))
            .clone();
        commands.spawn((Mesh3d(meshes.add(to_mesh(&water, false))), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, FarTag));
    }
}
