//! [horizon] The world beyond the map, out to the horizon: the far ground
//! and water in rings of coarser detail (`nd_procgen::horizon`), mountain
//! ridges as hazy silhouettes past the fog, the highway, the railway and
//! the roads off the map running on to the neighbouring towns, the towns'
//! skylines (lit at night) and the shipping lanes' buoys.
//!
//! Nothing out there is simulated. It is all built from the saved map, the
//! ways leading off it and the seed, and rebuilt only when those change.
//! The far ground's first row of vertices is the landscape tiles' last.
//!
//! The silhouettes (ridges, skylines) are unlit and out of the fog: their
//! material is the fog's own colour, and their vertex colours a shade of
//! it, so they read as shapes in the haze and fade to nothing where the
//! fog would hide everything (walk view, rain). Lights are additive and
//! shown only at night. [`update_far_look`] sets the few materials from
//! the fog and the daylight when they change (a handful of colour writes).

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use nd_procgen::horizon::{self, towns, ways, FarLand, Seam};
use nd_procgen::{MeshData, Surface};
use nd_sim::landscape::WAY_REACH;
use nd_sim::{Vec2 as SVec2, World};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::landscape::LandscapeRender;
use super::sky::SkyLook;
use super::{despawn_all, terrain_config, to_mesh, Materials};
use crate::assets::GameAssets;
use crate::camera::OrbitCamera;
use crate::sim::Sim;

#[derive(Component)]
pub(super) struct FarTag;

/// Far lights (towns, far ways, buoys): shown only at night.
#[derive(Component)]
pub(super) struct FarLights;

#[derive(Resource, Default)]
pub(super) struct FarRender {
    key: Option<u64>,
    /// The counters `key` was hashed under: the landscape, the roads, the
    /// built ground, the ports.
    gate: super::Gate<(u64, u64, u64, u64, u32, usize)>,
    water_mat: Option<Handle<StandardMaterial>>,
    way_mat: Option<Handle<StandardMaterial>>,
    /// Ridges and skylines: the fog's colour, alpha the clarity.
    haze_mat: Option<Handle<StandardMaterial>>,
    /// Additive lights and the towns' glow.
    light_mat: Option<Handle<StandardMaterial>>,
    glow_mat: Option<Handle<StandardMaterial>>,
    /// What the materials were last set to: (fog rgb, clarity, night).
    look: Option<([f32; 3], f32, f32)>,
    /// Triangles drawn out there.
    pub(super) triangles: usize,
    /// Where the neighbouring towns stand.
    pub(super) towns: Vec<SVec2>,
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

fn far_key(w: &World, seam: u64) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{:?}", w.landscape.params).hash(&mut h);
    w.landscape.is_flat().hash(&mut h);
    w.landscape.rivers.len().hash(&mut h);
    // [r4-tiles] Land bought moves the seam out.
    w.landscape.land_half().to_bits().hash(&mut h);
    seam.hash(&mut h);
    for (a, b, hw) in far_ways(w) {
        for v in [a.x, a.y, b.x, b.y, hw] {
            (v.round() as i32).hash(&mut h);
        }
    }
    for (p, d) in ways::lane_starts(w) {
        for v in [p.x, p.y, d.x * 100.0, d.y * 100.0] {
            (v.round() as i32).hash(&mut h);
        }
    }
    if !w.landscape.is_flat() {
        // The level the ways run at out there follows the ground at the edge.
        for (_, s) in w.roads.alive_segments() {
            for n in [s.a, s.b] {
                let p = w.roads.node(n).pos;
                if p.x.abs().max(p.y.abs()) > w.landscape.land_half() - 300.0 {
                    (w.road_z.get(n.0 as usize).copied().unwrap_or(0.0).round() as i32).hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// A hash of the seam's heights, so the far ground follows an edit that
/// regraded the landscape's edge.
fn seam_hash(seam: &Seam) -> u64 {
    let half = seam.half();
    let n = (half * 2.0 / seam.cell()).round() as i32;
    let mut h = DefaultHasher::new();
    for i in 0..=n {
        let t = -half + i as f32 * seam.cell();
        for p in [SVec2::new(t, half), SVec2::new(t, -half), SVec2::new(half, t), SVec2::new(-half, t)] {
            if let Some(s) = seam.get(p) {
                s.h.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

/// A mesh centred on its own middle, and where that middle is: the
/// translucent layers sort by their entity's place, so a far mesh must not
/// claim to stand at the map's centre (it would be drawn over the towns).
fn centred(m: &MeshData) -> (MeshData, Vec3) {
    if m.positions.is_empty() {
        return (m.clone(), Vec3::ZERO);
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for p in &m.positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    let c = (lo + hi) * 0.5;
    let mut out = m.clone();
    for p in out.positions.iter_mut() {
        *p = (Vec3::from(*p) - c).to_array();
    }
    (out, c)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_far(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    land_render: Res<LandscapeRender>,
    mut state: ResMut<FarRender>,
    existing: Query<Entity, With<FarTag>>,
) {
    let w = &sim.world;
    let forced = assets.palette_changed || sim.full_rebuild || existing.is_empty();
    if state.gate.same((sim.generation, w.landscape.version(), w.roads.version(), w.ground.version(), w.config.half_extent.to_bits(), w.ports.ports.len())) && state.key.is_some() && !forced {
        return;
    }
    // The playable ground's edge: the landscape's tiles, or the flat map's hills.
    let flat = w.landscape.is_flat();
    let cfg = terrain_config(w);
    let seam = if flat {
        let m = nd_procgen::build_terrain(&cfg, &assets.palette);
        Seam::from_meshes([&m], cfg.half_size, cfg.cell)
    } else {
        let Some(tiles) = land_render.tiles.as_ref() else {
            // Not drawn yet: next frame.
            state.key = None;
            return;
        };
        Seam::from_meshes((0..tiles.len()).map(|t| tiles.mesh(t)), w.landscape.land_half(), 8.0)
    };
    let key = far_key(w, seam_hash(&seam));
    if state.key == Some(key) && !forced {
        return;
    }
    let t0 = std::time::Instant::now();
    state.key = Some(key);
    despawn_all(&mut commands, &existing);
    let pal = &assets.palette;
    let far = FarLand::new(&w.landscape);
    let flat_h = nd_procgen::flat_height(&cfg);
    let near = |p: SVec2| if flat { flat_h(p) } else { w.ground_height(p) };
    let ground = horizon::far_ground(&far, &near, &seam, pal);
    let height = |p: SVec2| near(p) + far.extra(p);
    let mut tris = ground.triangles();

    // The far ground: opaque, fogged, casting no shadow (a 10 km mesh in
    // every shadow cascade was the old far ground's biggest cost).
    let dirt = mats.surface(Surface::Dirt);
    for m in ground.ground.iter().filter(|m| !m.is_empty()) {
        commands.spawn((Mesh3d(meshes.add(to_mesh(m, true))), MeshMaterial3d(dirt.clone()), Transform::IDENTITY, NotShadowCaster, FarTag));
    }
    let haze = state
        .haze_mat
        .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::NONE, unlit: true, fog_enabled: false, alpha_mode: AlphaMode::Blend, ..default() }))
        .clone();
    let light = state
        .light_mat
        .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::BLACK, unlit: true, fog_enabled: false, alpha_mode: AlphaMode::Add, ..default() }))
        .clone();
    let glow = state
        .glow_mat
        .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::BLACK, unlit: true, fog_enabled: false, alpha_mode: AlphaMode::Add, ..default() }))
        .clone();
    state.look = None;
    let spawn_at = |commands: &mut Commands, meshes: &mut Assets<Mesh>, m: &MeshData, mat: &Handle<StandardMaterial>, lights: bool| {
        if m.is_empty() {
            return;
        }
        let (local, at) = centred(m);
        let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(&local, false))), MeshMaterial3d(mat.clone()), Transform::from_translation(at), NotShadowCaster, NotShadowReceiver, FarTag));
        if lights {
            e.insert((FarLights, Visibility::Hidden));
        }
    };
    for m in &ground.ridges {
        spawn_at(&mut commands, &mut meshes, m, &haze, false);
    }

    // The far water.
    if !flat {
        let water_mat = state
            .water_mat
            .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.06, reflectance: 0.6, metallic: 0.1, ..default() }))
            .clone();
        for m in horizon::far_water(&far, pal).iter().filter(|m| !m.is_empty()) {
            tris += m.indices.len() / 3;
            let (local, at) = centred(m);
            commands.spawn((Mesh3d(meshes.add(to_mesh(&local, false))), MeshMaterial3d(water_mat.clone()), Transform::from_translation(at), NotShadowCaster, FarTag));
        }
    }

    // The towns where the ways lead, and the ways on to them.
    let ends = ways::way_ends(w);
    let exits: Vec<SVec2> = ends.iter().map(|e| e.dir).collect();
    let town_list = towns::place(&far, &exits, &height);
    for t in &town_list {
        let sk = towns::skyline(t, &|o| height(t.pos + o) - t.ground);
        tris += sk.triangles();
        let at = Transform::from_xyz(t.pos.x, t.ground, t.pos.y);
        for (m, mat, lights) in [(&sk.towers, &haze, false), (&sk.lights, &light, true), (&sk.glow, &glow, true)] {
            if m.is_empty() {
                continue;
            }
            let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(m, false))), MeshMaterial3d(mat.clone()), at, NotShadowCaster, NotShadowReceiver, FarTag));
            if lights {
                e.insert((FarLights, Visibility::Hidden));
            }
        }
    }
    let (ribbons, lamps) = ways::ways(&ends, &town_list, &height);
    if !ribbons.is_empty() {
        tris += ribbons.indices.len() / 3;
        let way_mat = state
            .way_mat
            .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.92, ..default() }))
            .clone();
        let (local, at) = centred(&ribbons);
        commands.spawn((Mesh3d(meshes.add(to_mesh(&local, false))), MeshMaterial3d(way_mat), Transform::from_translation(at), NotShadowCaster, FarTag));
    }
    // Lamps along the ways from the map's edge out to their ends too, and
    // the buoys of the shipping lanes.
    let mut lit = lamps;
    lit.append(&near_lamps(w, &near));
    if !flat {
        lit.append(&ways::lane_lights(&ways::lanes(&far, &ways::lane_starts(w))));
    }
    tris += lit.indices.len() / 3;
    spawn_at(&mut commands, &mut meshes, &lit, &light, true);
    state.triangles = tris;
    state.towns = town_list.iter().map(|t| t.pos).collect();
    info!("horizon: far landscape built in {:.0} ms, {} triangles, {} towns", t0.elapsed().as_secs_f32() * 1000.0, tris, town_list.len());
}

/// Lamps along the map's ways from its edge out to where they end, a pair
/// every 240 m: by night the highway and the roads off the map are strings
/// of light running out to the towns.
fn near_lamps(w: &World, ground: &dyn Fn(SVec2) -> f32) -> MeshData {
    let mut m = MeshData::default();
    for (id, s) in w.roads.alive_segments() {
        if matches!(s.kind, nd_sim::roads::RoadKind::Rail | nd_sim::roads::RoadKind::Path) {
            continue;
        }
        let (a, b) = w.roads.segment_points(id);
        let len = a.distance(b);
        if len < 1.0 {
            continue;
        }
        let dir = (b - a) / len;
        let side = SVec2::new(-dir.y, dir.x) * (s.half_width() + 2.0);
        let mut t = 0.0;
        while t <= len {
            let p = a + dir * t;
            let r = p.x.abs().max(p.y.abs());
            // Past where the fog takes the map's own lamps.
            let edge = w.landscape.land_half();
            if r > edge + 1200.0 && r < WAY_REACH {
                let k = (1.0 - (r - edge) / 9000.0).clamp(0.3, 1.0) * 0.6;
                for q in [p + side, p - side] {
                    let y = ground(q).max(nd_sim::landscape::WATER_LEVEL + 1.0);
                    towns::light_dot(&mut m, Vec3::new(q.x, y + 9.0, q.y), 3.0, [k, 0.5 * k, 0.15 * k]);
                }
            }
            t += 240.0;
        }
    }
    m
}

/// How clearly the far silhouettes show: not at all when the fog closes
/// in (walk view, heavy weather), fully in the clear orbit view.
pub(super) fn clarity(fog_end: f32) -> f32 {
    nd_sim::geom::smoothstep(1000.0, 2500.0, fog_end)
}

/// How much night it is for the far lights.
pub(super) fn night(daylight: f32) -> f32 {
    1.0 - nd_sim::geom::smoothstep(0.15, 0.55, daylight)
}

/// The silhouettes take the fog's colour, the lights the night: a few
/// material writes when they change.
pub(super) fn update_far_look(
    mut state: ResMut<FarRender>,
    look: Res<SkyLook>,
    lighting: Res<super::Lighting>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    fogs: Query<&DistanceFog, With<OrbitCamera>>,
    mut lights: Query<&mut Visibility, With<FarLights>>,
) {
    let Some(fog) = fogs.iter().next() else { return };
    let end = match fog.falloff {
        FogFalloff::Linear { end, .. } => end,
        _ => nd_sim::landscape::SIGHT,
    };
    let c = fog.color.to_linear();
    let rgb = [c.red, c.green, c.blue];
    let heat = lighting.heat;
    let clear = if heat { 0.0 } else { clarity(end) };
    let dark = if heat { 0.0 } else { night(look.daylight) };
    if let Some((r0, c0, n0)) = state.look {
        let close = (0..3).all(|i| (r0[i] - rgb[i]).abs() < 0.002) && (c0 - clear).abs() < 0.01 && (n0 - dark).abs() < 0.01;
        if close {
            return;
        }
    }
    state.look = Some((rgb, clear, dark));
    if let Some(mut m) = state.haze_mat.as_ref().and_then(|h| materials.get_mut(h)) {
        m.base_color = Color::linear_rgba(rgb[0], rgb[1], rgb[2], clear * 0.92);
    }
    let shine = dark * (0.3 + 0.7 * clear);
    if let Some(mut m) = state.light_mat.as_ref().and_then(|h| materials.get_mut(h)) {
        let k = shine * 3.0;
        m.base_color = Color::linear_rgb(k, k, k);
    }
    if let Some(mut m) = state.glow_mat.as_ref().and_then(|h| materials.get_mut(h)) {
        let k = shine * 0.5;
        m.base_color = Color::linear_rgb(k, k, k);
    }
    let show = shine > 0.01;
    for mut v in &mut lights {
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_silhouettes_fade_with_the_fog_and_lights_come_on_at_night() {
        assert_eq!(clarity(900.0), 0.0);
        assert_eq!(clarity(2600.0), 1.0);
        assert!(clarity(1800.0) > 0.2 && clarity(1800.0) < 0.8);
        assert_eq!(night(1.0), 0.0);
        assert_eq!(night(0.0), 1.0);
    }

    #[test]
    fn centred_meshes_keep_their_world_positions() {
        let m = MeshData { positions: vec![[100.0, 0.0, 200.0], [300.0, 10.0, 400.0]], ..MeshData::default() };
        let (local, at) = centred(&m);
        assert_eq!(at, Vec3::new(200.0, 5.0, 300.0));
        for (a, b) in local.positions.iter().zip(&m.positions) {
            assert_eq!((Vec3::from(*a) + at).to_array(), *b);
        }
    }
}
