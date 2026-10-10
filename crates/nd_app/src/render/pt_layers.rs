//! [pt-ux] The playtest round's map layers:
//!
//! - **Parking** (`parking`): every street piece coloured by how much legal
//!   parking a driver finds there (its own free kerb slots plus the free
//!   spaces of public lots within a short walk), red for none to green for
//!   plenty; every public lot outlined by how full it is, with its
//!   "used / capacity" label. Over everything, refreshed every second.
//! - **Trails** (`trails`): a long-exposure view. The city dims almost to
//!   black (`focus::Focus::Trails`) and everything that moves leaves a
//!   glowing streak that fades out over [`TRAIL_SAMPLES`] samples: cars red,
//!   buses gold, lorries amber, sirens blue-white, people cyan, robots
//!   green.
//! - **Land density** (`density`): how densely each plot is built, as a
//!   heat map. [`plot_density`] is the hook for the growth model's land
//!   density (fix/pt-growth): until that lands it reads the built floor
//!   area over the plot's area.
//!
//! The Pedestrian network layer's extra (every walkway drawn, people
//! glowing over a dimmed city) is in `overlays::sync_paths` and
//! `focus::Focus::Walk`.

use std::collections::{HashMap, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_procgen::overlay::{heat_color, traffic_color, HeatGrid};
use nd_sim::{Activity, Vec2 as SVec2, World};

use super::{despawn_all, focus::FocusKeep, Overlays};
use crate::sim::{Sim, SimSet};

pub(super) struct PtLayersPlugin;

impl Plugin for PtLayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Trails>()
            .add_systems(Update, (parking_gizmos, sync_trails, sync_density).in_set(SimSet::Sync).after(super::lift_movers))
            .add_systems(bevy_egui::EguiPrimaryContextPass, parking_labels.after(crate::ui::side_panel));
    }
}

// ── parking ────────────────────────────────────────────────────────────────

/// How far (m) a driver will walk from a lot: its free spaces count for
/// the streets this close, less the farther.
pub const PARK_WALK: f32 = 220.0;
/// Free spaces at which a street reads as fully green.
pub const PARK_PLENTY: f32 = 24.0;

/// A public parking lot (a parking business or a city lot) that is open.
fn public_lot(w: &World, id: nd_sim::BuildingId, b: &nd_sim::Building) -> bool {
    b.construction.is_none() && b.parking_capacity > 0 && (b.is_parking() || w.vehicles.city_lots.contains_key(&id))
}

/// The availability score of a street: its free kerb slots plus each lot's
/// free spaces weighted down linearly to nothing at [`PARK_WALK`], as 0..1
/// of [`PARK_PLENTY`]. Pure, for the tests.
pub fn availability(free_kerb: u32, lots: &[(f32, u32)]) -> f32 {
    let lot: f32 = lots.iter().filter(|(d, _)| *d < PARK_WALK).map(|(d, free)| *free as f32 * (1.0 - d / PARK_WALK)).sum();
    ((free_kerb as f32 + lot) / PARK_PLENTY).clamp(0.0, 1.0)
}

/// The colour of an availability (green plenty, red none).
pub fn availability_color(a: f32) -> [f32; 3] {
    traffic_color(1.0 - a.clamp(0.0, 1.0))
}

/// How full a lot is, 0..1.
pub fn lot_use(used: u16, cap: u16) -> f32 {
    if cap == 0 { 1.0 } else { (used as f32 / cap as f32).clamp(0.0, 1.0) }
}

/// Each street piece's availability and each public lot (outline, use).
struct ParkingView {
    streets: Vec<(Vec3, Vec3, Color)>,
    lots: Vec<(Vec<Vec3>, Color)>,
}

fn parking_view(w: &World) -> ParkingView {
    let lots: Vec<(SVec2, u32, &nd_sim::Lot, f32)> = w
        .buildings
        .iter()
        .filter(|(id, b)| public_lot(w, *id, b))
        .filter_map(|(_, b)| {
            let l = w.lots.get(b.lot)?;
            Some((l.center, b.parking_capacity.saturating_sub(b.parking_used) as u32, l, lot_use(b.parking_used, b.parking_capacity)))
        })
        .collect();
    let mut streets = Vec::new();
    for (sid, s) in w.roads.alive_segments() {
        if s.tunnel || !s.is_street() {
            continue;
        }
        let (a, b) = w.roads.segment_points(sid);
        let sides = (s.lanes_fwd > 0) as u32 + (s.lanes_bwd > 0) as u32;
        let total = w.kerb_slot_count(sid) as u32 * sides;
        let used = w.vehicles.kerbs.get(&sid).map_or(0, |m| m.len() as u32);
        let mid = (a + b) * 0.5;
        let near: Vec<(f32, u32)> = lots.iter().map(|(c, free, _, _)| (c.distance(mid), *free)).collect();
        let av = availability(total.saturating_sub(used), &near);
        let c = availability_color(av);
        let col = Color::linear_rgba(c[0] * 1.5, c[1] * 1.5, c[2] * 1.5, 0.85);
        let (za, zb) = (w.road_height(sid, 0.0), w.road_height(sid, 1.0));
        streets.push((Vec3::new(a.x, za + 0.9, a.y), Vec3::new(b.x, zb + 0.9, b.y), col));
    }
    let lots = lots
        .iter()
        .map(|(_, _, l, u)| {
            let c = traffic_color(*u);
            let pts: Vec<Vec3> = l.polygon.iter().chain(l.polygon.first()).map(|p| Vec3::new(p.x, w.surface_height(*p) + 1.0, p.y)).collect();
            (pts, Color::linear_rgb(c[0] * 2.2, c[1] * 2.2, c[2] * 2.2))
        })
        .collect();
    ParkingView { streets, lots }
}

/// The Parking layer's lines, over everything (`layers::LoadGizmos`).
fn parking_gizmos(sim: Res<Sim>, overlays: Res<Overlays>, lighting: Res<super::Lighting>, time: Res<Time>, mut cache: Local<Option<(f32, ParkingView)>>, mut gizmos: Gizmos<super::layers::LoadGizmos>) {
    if lighting.heat || !overlays.parking {
        *cache = None;
        return;
    }
    let now = time.elapsed_secs();
    if cache.as_ref().is_none_or(|(at, _)| now - at >= 1.0) || sim.full_rebuild {
        *cache = Some((now, parking_view(&sim.world)));
    }
    let Some((_, v)) = cache.as_ref() else { return };
    for (a, b, c) in &v.streets {
        gizmos.line(*a, *b, *c);
    }
    for (pts, c) in &v.lots {
        gizmos.linestrip(pts.iter().copied(), *c);
    }
}

/// Every public lot's "used / capacity" over the map.
fn parking_labels(mut contexts: EguiContexts, sim: Res<Sim>, overlays: Res<Overlays>, flags: Res<crate::ui::UiFlags>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    if !overlays.parking {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let w = &sim.world;
    let screen = flags.map_rect(ctx.viewport_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("parking_labels"))).with_clip_rect(screen);
    let cam = gt.translation();
    let mut shown = 0;
    for (id, b) in w.buildings.iter().filter(|(id, b)| public_lot(w, *id, b)) {
        let Some(l) = w.lots.get(b.lot) else { continue };
        let at3 = Vec3::new(l.center.x, w.surface_height(l.center) + 2.0, l.center.y);
        if at3.distance(cam) > 1400.0 || shown > 200 {
            continue;
        }
        let Ok(v) = camera.world_to_viewport(gt, at3) else { continue };
        let at = crate::ui::theme::to_points(ctx, v);
        if !screen.contains(at) {
            continue;
        }
        shown += 1;
        let u = lot_use(b.parking_used, b.parking_capacity);
        let c = traffic_color(u);
        let col = egui::Color32::from(egui::Rgba::from_rgb(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0)));
        let city = w.vehicles.city_lots.contains_key(&id);
        let text = format!("P {}/{}{}", b.parking_used, b.parking_capacity, if city { " city" } else { "" });
        let galley = painter.layout_no_wrap(text, crate::ui::theme::small(), egui::Color32::WHITE);
        let rect = egui::Rect::from_center_size(at, galley.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(190));
        painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Outside);
        painter.galley(rect.min + egui::vec2(5.0, 2.0), galley, egui::Color32::WHITE);
    }
    Ok(())
}

// ── trails ─────────────────────────────────────────────────────────────────

/// Samples each trail keeps: its length in time is this times
/// [`TRAIL_EVERY`].
pub const TRAIL_SAMPLES: usize = 28;
/// Real seconds between samples.
pub const TRAIL_EVERY: f32 = 0.35;
/// At most this many movers leave a trail (the rest are skipped by id hash,
/// so the same ones every sample).
pub const TRAIL_MOVERS: usize = 6000;
/// A jump longer than this between two samples is a teleport (in or out of
/// a building, off the map): no streak across it.
const TRAIL_JUMP: f32 = 45.0;

/// A mover's kind, for its colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mover {
    Car,
    Bus,
    Lorry,
    Siren,
    Walker,
    Bot,
}

impl Mover {
    /// Linear, over 1 so it blooms against the dark.
    pub fn color(self) -> [f32; 3] {
        match self {
            Mover::Car => [2.6, 0.35, 0.12],
            Mover::Bus => [2.6, 1.7, 0.3],
            Mover::Lorry => [2.4, 0.95, 0.2],
            Mover::Siren => [0.9, 1.4, 3.2],
            Mover::Walker => [0.25, 1.7, 2.3],
            Mover::Bot => [0.4, 2.2, 0.8],
        }
    }
}

/// One mover's last positions, oldest first; `None` where it was not seen.
#[derive(Default)]
struct Trail {
    kind: Option<Mover>,
    pts: VecDeque<Option<Vec3>>,
}

#[derive(Resource, Default)]
pub(super) struct Trails {
    trails: HashMap<u64, Trail>,
    last: f32,
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
}

/// How bright a streak is `age` samples back of `n` (newest 1, fading to 0).
pub fn trail_fade(age: usize, n: usize) -> f32 {
    if n <= 1 {
        return 1.0;
    }
    let t = 1.0 - age as f32 / (n - 1) as f32;
    t.clamp(0.0, 1.0).powf(1.6)
}

/// A hash of a slotmap key, for the trail table and the sampling.
fn key_hash<K: slotmap::Key>(tag: u64, k: K) -> u64 {
    let v = k.data().as_ffi();
    (v ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9)
}

/// Where every mover is now, with its kind.
fn movers(w: &World, f: f32) -> Vec<(u64, Mover, Vec3)> {
    let mut out = Vec::new();
    for (id, car) in w.traffic.cars.iter() {
        let (p, _) = car.pose_at(f);
        let kind = if car.siren { Mover::Siren } else if car.bus.is_some() { Mover::Bus } else if car.cargo.is_some() { Mover::Lorry } else { Mover::Car };
        out.push((key_hash(1, id), kind, Vec3::new(p.x, w.car_height_at(car, p) + 0.8, p.y)));
    }
    for (id, c) in w.citizens.iter() {
        if !c.is_visible() || matches!(c.activity, Activity::Driving { .. } | Activity::Arriving) {
            continue;
        }
        let p = c.prev_pos.lerp(c.pos, f);
        out.push((key_hash(2, id), Mover::Walker, Vec3::new(p.x, w.surface_height(p) + 0.6, p.y)));
    }
    for (id, b) in w.bots.bots.iter() {
        let p = if b.prev_pos.distance(b.pos) < 30.0 { b.prev_pos.lerp(b.pos, f) } else { b.pos };
        out.push((key_hash(3, id), Mover::Bot, Vec3::new(p.x, w.surface_height(p) + 0.5, p.y)));
    }
    if out.len() > TRAIL_MOVERS {
        // The same movers every sample: keep those whose hash is lowest.
        out.sort_by_key(|m| m.0);
        out.truncate(TRAIL_MOVERS);
    }
    out
}

/// One sample: every trail moves on by one, the seen ones with their new
/// point, the unseen with a gap, and the empty ones go.
fn sample(trails: &mut HashMap<u64, Trail>, now: Vec<(u64, Mover, Vec3)>) {
    let mut seen = std::collections::HashSet::with_capacity(now.len());
    for (k, kind, p) in now {
        seen.insert(k);
        let t = trails.entry(k).or_default();
        t.kind = Some(kind);
        t.pts.push_back(Some(p));
    }
    trails.retain(|k, t| {
        if !seen.contains(k) {
            t.pts.push_back(None);
        }
        while t.pts.len() > TRAIL_SAMPLES {
            t.pts.pop_front();
        }
        t.pts.iter().any(Option::is_some)
    });
}

/// The streaks as flat ribbons (one quad a segment), vertex-coloured by
/// kind and fade.
fn trail_mesh(trails: &HashMap<u64, Trail>) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for t in trails.values() {
        let Some(kind) = t.kind else { continue };
        let c = kind.color();
        let half = if kind == Mover::Walker || kind == Mover::Bot { 0.35 } else { 0.7 };
        let n = t.pts.len();
        for i in 1..n {
            let (Some(a), Some(b)) = (t.pts[i - 1], t.pts[i]) else { continue };
            let d = b - a;
            let len = Vec2::new(d.x, d.z).length();
            if !(0.05..=TRAIL_JUMP).contains(&len) {
                continue;
            }
            let side = Vec3::new(-d.z, 0.0, d.x) / len * half;
            // Newest at the end of the deque.
            let (fa, fb) = (trail_fade(n - i, n), trail_fade(n - 1 - i, n));
            let base = pos.len() as u32;
            pos.extend([(a - side).to_array(), (a + side).to_array(), (b + side).to_array(), (b - side).to_array()]);
            let ca = [c[0] * fa, c[1] * fa, c[2] * fa, 1.0];
            let cb = [c[0] * fb, c[1] * fb, c[2] * fb, 1.0];
            col.extend([ca, ca, cb, cb]);
            idx.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD);
    if pos.is_empty() {
        // An empty mesh logs a warning in Bevy's allocator: one degenerate quad.
        pos = vec![[0.0, -500.0, 0.0]; 3];
        col = vec![[0.0; 4]; 3];
        idx = vec![0, 1, 2];
    }
    let n = pos.len();
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    m
}

/// The Trails layer: sample, rebuild the ribbons, show them; all gone when
/// the layer is off.
fn sync_trails(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, sim: Res<Sim>, overlays: Res<Overlays>, time: Res<Time>, mut st: ResMut<Trails>) {
    if !overlays.trails {
        if let Some(e) = st.entity.take() {
            commands.entity(e).despawn();
        }
        st.trails.clear();
        st.mesh = None;
        return;
    }
    if sim.full_rebuild {
        st.trails.clear();
    }
    let now = time.elapsed_secs();
    if st.entity.is_some() && now - st.last < TRAIL_EVERY {
        return;
    }
    st.last = now;
    let m = movers(&sim.world, sim.frac());
    sample(&mut st.trails, m);
    let mesh = trail_mesh(&st.trails);
    match st.mesh.clone() {
        Some(h) => {
            if let Some(mut slot) = meshes.get_mut(&h) {
                *slot = mesh;
            }
        }
        None => st.mesh = Some(meshes.add(mesh)),
    }
    if st.entity.is_none() {
        let mat = st
            .material
            .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Add, double_sided: true, cull_mode: None, fog_enabled: false, ..default() }))
            .clone();
        let mesh = st.mesh.clone().unwrap_or_default();
        st.entity = Some(commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, FocusKeep, bevy::camera::visibility::NoFrustumCulling, TrailsTag)).id());
    }
}

#[derive(Component)]
pub(super) struct TrailsTag;

// ── land density ───────────────────────────────────────────────────────────

/// [pt-growth hook] How densely a plot is built, 0..1. The growth model
/// (fix/pt-growth) brings a land density score driven by foot traffic;
/// once it is on the World, read it here and the layer follows. Until then:
/// the built floor area over the plot's area (a floor-area ratio), with a
/// ratio of 8 reading as full.
pub fn plot_density(w: &World, lot: &nd_sim::Lot) -> Option<f32> {
    let b = lot.building.and_then(|b| w.buildings.get(b))?;
    let area = nd_sim::geom::polygon_area(&lot.polygon).abs().max(1.0);
    // Buildings cover about this much of their plot (taller ones a little less).
    let cover = if b.floors > 12 { 0.45 } else { 0.6 };
    Some(floor_area_ratio(area * cover, b.floors as f32, area))
}

/// Floor area over plot area, as 0..1 of a ratio of 8.
pub fn floor_area_ratio(footprint: f32, floors: f32, plot: f32) -> f32 {
    (footprint * floors.max(0.0) / plot.max(1.0) / 8.0).clamp(0.0, 1.0)
}

#[derive(Component)]
pub(super) struct DensityTag;

/// The Land density layer: a heat map of [`plot_density`], rebuilt every
/// few seconds while it is on.
fn sync_density(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mats: Res<super::Materials>, sim: Res<Sim>, overlays: Res<Overlays>, time: Res<Time>, mut at: Local<Option<f32>>, q: Query<Entity, With<DensityTag>>) {
    if !overlays.density {
        if at.take().is_some() {
            despawn_all(&mut commands, &q);
        }
        return;
    }
    let now = time.elapsed_secs();
    if at.is_some_and(|t| now - t < 4.0) && !sim.full_rebuild {
        return;
    }
    *at = Some(now);
    despawn_all(&mut commands, &q);
    let w = &sim.world;
    let mut lo = SVec2::splat(f32::MAX);
    let mut hi = SVec2::splat(f32::MIN);
    for l in w.lots.values() {
        lo = lo.min(l.center);
        hi = hi.max(l.center);
    }
    if lo.x > hi.x {
        return;
    }
    let mut g = HeatGrid::covering(lo - SVec2::splat(60.0), hi + SVec2::splat(60.0), 12.0, 30_000);
    for l in w.lots.values() {
        if let Some(d) = plot_density(w, l) {
            g.splat_radius(l.center, 16.0, d);
        }
    }
    g.blur();
    let mut m = g.mesh(g.max().max(0.15), 0.62, heat_color, |_| 0.0);
    if !w.landscape.is_flat() {
        for p in &mut m.positions {
            p[1] += w.surface_height(SVec2::new(p[0], p[2]));
        }
    }
    if m.is_empty() {
        return;
    }
    commands.spawn((Mesh3d(meshes.add(super::to_mesh(&m, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, DensityTag));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_counts_kerbs_and_near_lots() {
        assert_eq!(availability(0, &[]), 0.0);
        assert_eq!(availability(PARK_PLENTY as u32, &[]), 1.0);
        // A lot right there counts in full, one at the walk's end not at all.
        assert!((availability(0, &[(0.0, 12)]) - 0.5).abs() < 1e-5);
        assert_eq!(availability(0, &[(PARK_WALK, 100)]), 0.0);
        assert_eq!(availability(0, &[(PARK_WALK * 2.0, 100)]), 0.0);
        // Halfway, half.
        assert!((availability(0, &[(PARK_WALK / 2.0, 12)]) - 0.25).abs() < 1e-5);
        // Green when plenty, red when none.
        assert!(availability_color(1.0)[1] > availability_color(1.0)[0]);
        assert!(availability_color(0.0)[0] > availability_color(0.0)[1]);
        assert_eq!(lot_use(13, 30), 13.0 / 30.0);
        assert_eq!(lot_use(0, 0), 1.0);
    }

    #[test]
    fn trails_fade_and_drop_the_gone() {
        assert_eq!(trail_fade(0, 10), 1.0);
        assert_eq!(trail_fade(9, 10), 0.0);
        assert!(trail_fade(3, 10) > trail_fade(6, 10));
        let mut t = HashMap::new();
        sample(&mut t, vec![(1, Mover::Car, Vec3::ZERO), (2, Mover::Walker, Vec3::X)]);
        sample(&mut t, vec![(1, Mover::Car, Vec3::Z)]);
        assert_eq!(t[&1].pts.len(), 2);
        assert_eq!(t[&2].pts.back(), Some(&None));
        // Unseen for a whole trail: gone.
        for _ in 0..TRAIL_SAMPLES {
            sample(&mut t, vec![(1, Mover::Car, Vec3::Z)]);
        }
        assert!(t.contains_key(&1) && !t.contains_key(&2));
        assert_eq!(t[&1].pts.len(), TRAIL_SAMPLES);
        // Two movers, one segment each; a teleport makes no streak.
        let mut t = HashMap::new();
        sample(&mut t, vec![(1, Mover::Car, Vec3::ZERO), (2, Mover::Car, Vec3::ZERO)]);
        sample(&mut t, vec![(1, Mover::Car, Vec3::new(3.0, 0.0, 0.0)), (2, Mover::Car, Vec3::new(300.0, 0.0, 0.0))]);
        let m = trail_mesh(&t);
        assert_eq!(m.count_vertices(), 4);
    }

    #[test]
    fn floor_area_ratio_reads_built_density() {
        assert_eq!(floor_area_ratio(100.0, 1.0, 400.0), 0.25 / 8.0);
        assert_eq!(floor_area_ratio(400.0, 40.0, 400.0), 1.0);
        assert_eq!(floor_area_ratio(0.0, 3.0, 400.0), 0.0);
        assert!(floor_area_ratio(200.0, 10.0, 400.0) > floor_area_ratio(200.0, 2.0, 400.0));
    }
}
