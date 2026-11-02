//! [cable] Cable cars on the map: station halls with their bullwheels,
//! lattice pylons with warning lights on the tall ones, the ropes sagging
//! between supports, and the cabins riding them (swinging a little in the
//! wind and more as they slow through a station), lit at night.
//!
//! The halls, pylons and ropes are one batch of meshes rebuilt only when
//! a line or station changes (`CableWays::version`). Every cabin of a kind
//! and colour shares one mesh per surface, so a long line's hundred
//! cabins draw as a handful of instanced batches; each cabin is a parent
//! transform moved every frame from the sim's pure pose
//! (`CableLine::loop_pose`, `shuttle_pose`), cabins beyond `CABIN_RANGE`
//! of the camera are not drawn, and the pylons' lights blink by changing
//! one material.

use super::*;
use nd_sim::cable::{CableKind, CableLine, CableLineId, CableStation, Spec};

/// Static cable meshes (halls, pylons, ropes, lights).
#[derive(Component, Clone)]
pub(super) struct CableTag;
/// A cabin's parent transform.
#[derive(Component)]
pub(super) struct CabinTag;
/// A turning bullwheel.
#[derive(Component)]
pub(super) struct WheelTag;

/// Cabins further than this from the camera are not drawn.
const CABIN_RANGE: f32 = 2600.0;
/// Pylons taller than this carry red warning lights.
const WARN_HEIGHT: f32 = 30.0;

#[derive(Resource, Default)]
pub(super) struct CableRender {
    key: Option<u64>,
    statics: Vec<Entity>,
    /// Each line's cabins (parent entities) and what they were made for.
    cabins: HashMap<CableLineId, Vec<Entity>>,
    wheels: Vec<(Entity, CableLineId, f32)>,
    /// Cabin meshes per (kind, colour): one per surface.
    models: HashMap<(CableKind, u8), Vec<(Surface, Handle<Mesh>)>>,
    warn: Option<Handle<StandardMaterial>>,
    wheel_mesh: HashMap<u8, Vec<(Surface, Handle<Mesh>)>>,
    spin: f32,
}

fn v3(p: SVec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

fn col(c: [f32; 3], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

const STEEL: [f32; 3] = [0.30, 0.31, 0.33];
const DARK: [f32; 3] = [0.10, 0.10, 0.11];
const CONCRETE: [f32; 3] = [0.52, 0.50, 0.47];

/// A cabin, its floor at the origin, facing +X, the grip on the rope at
/// `spec.hanger` above.
fn cabin_model(kind: CableKind, colour: [f32; 3]) -> Geometry {
    let spec = kind.spec();
    let mut g = Geometry::default();
    let paint = col(colour, 0.9);
    let (len, w, h) = match kind {
        CableKind::Gondola => (spec.cabin_len, 1.85, spec.cabin_h),
        CableKind::Tramway => (spec.cabin_len, 3.1, spec.cabin_h),
    };
    let skirt = h * 0.42;
    // The body: a painted lower half, a band of lit windows, a steel roof.
    g.part(Surface::Metal).boxed(Vec3::new(0.0, skirt * 0.5, 0.0), Vec3::new(len, skirt, w), 0.0, paint);
    // A tapered underside, so it reads as a cabin and not a crate.
    g.part(Surface::Metal).boxed(Vec3::new(0.0, -0.12, 0.0), Vec3::new(len * 0.82, 0.24, w * 0.8), 0.0, col(colour, 0.6));
    let win = h - skirt - 0.28;
    g.part(Surface::Glow).boxed(Vec3::new(0.0, skirt + win * 0.5, 0.0), Vec3::new(len * 0.97, win, w * 0.97), 0.0, [1.7, 1.35, 0.85, 1.0]);
    // Mullions over the windows.
    let posts = if kind == CableKind::Gondola { 3 } else { 7 };
    for k in 0..=posts {
        let x = -len * 0.5 + len * k as f32 / posts as f32;
        g.part(Surface::Metal).boxed(Vec3::new(x.clamp(-len * 0.5 + 0.04, len * 0.5 - 0.04), skirt + win * 0.5, 0.0), Vec3::new(0.08, win, w + 0.02), 0.0, col(DARK, 1.0));
    }
    g.part(Surface::Metal).boxed(Vec3::new(0.0, h - 0.14, 0.0), Vec3::new(len, 0.28, w), 0.0, col(STEEL, 1.1));
    g.part(Surface::Metal).boxed(Vec3::new(0.0, h + 0.04, 0.0), Vec3::new(len * 0.7, 0.12, w * 0.6), 0.0, col(STEEL, 0.9));
    // A line-colour light along the roof edge (soft glow at night).
    g.part(Surface::Glow).boxed(Vec3::new(0.0, h - 0.30, 0.0), Vec3::new(len * 1.01, 0.05, w * 1.01), 0.0, col(colour, 1.6));
    let grip = spec.hanger;
    match kind {
        CableKind::Gondola => {
            // The hanger arm, bent out over the roof to the grip.
            let top = Vec3::new(0.0, h + 0.1, 0.0);
            let elbow = Vec3::new(0.0, grip - 0.55, w * 0.18);
            g.part(Surface::Metal).beam(top, elbow, 0.13, col(DARK, 1.0));
            g.part(Surface::Metal).beam(elbow, Vec3::new(0.0, grip - 0.1, 0.0), 0.13, col(DARK, 1.0));
            g.part(Surface::Metal).boxed(Vec3::new(0.0, grip, 0.0), Vec3::new(0.75, 0.32, 0.32), 0.0, col(STEEL, 0.8));
        }
        CableKind::Tramway => {
            // Two hangers up to the carriage running on the track ropes.
            for x in [-len * 0.3, len * 0.3] {
                g.part(Surface::Metal).beam(Vec3::new(x, h + 0.1, 0.0), Vec3::new(x * 0.3, grip - 0.9, 0.0), 0.22, col(DARK, 1.0));
            }
            g.part(Surface::Metal).boxed(Vec3::new(0.0, grip - 0.55, 0.0), Vec3::new(4.2, 0.7, 1.4), 0.0, col(STEEL, 0.9));
            for x in [-1.6, -0.55, 0.55, 1.6] {
                g.part(Surface::Metal).cylinder_capped(SVec2::new(x, 0.0), 0.22, 0.22, grip - 0.25, grip + 0.05, 8, true, true, col(DARK, 1.0));
            }
        }
    }
    g
}

/// A bullwheel: a steel rim, spokes and a hub, horizontal, its centre at
/// the origin.
fn wheel_model(radius: f32) -> Geometry {
    let mut g = Geometry::default();
    let rim = col(STEEL, 1.15);
    g.part(Surface::Metal).cylinder_capped(SVec2::ZERO, radius + 0.25, radius + 0.25, -0.22, 0.22, 28, true, true, rim);
    g.part(Surface::Metal).cylinder_capped(SVec2::ZERO, 0.6, 0.6, -0.4, 0.4, 12, true, true, col(DARK, 1.0));
    for k in 0..6 {
        let a = k as f32 * std::f32::consts::TAU / 6.0;
        let d = Vec3::new(a.cos(), 0.0, a.sin());
        g.part(Surface::Metal).beam(d * 0.5, d * radius, 0.22, col(STEEL, 0.9));
    }
    g
}

/// A lattice pylon from `base` up to the rope at `top`, its crossarm
/// square to the line (`dir`).
fn pylon(g: &mut Geometry, warn: &mut MeshData, base: Vec3, top: f32, dir: SVec2, spec: &Spec, tram: bool) {
    let h = (top - base.y).max(2.0);
    let along = Vec3::new(dir.x, 0.0, dir.y).normalize_or_zero();
    let across = Vec3::new(-dir.y, 0.0, dir.x).normalize_or_zero();
    let foot = if tram { 2.6 + 0.04 * h } else { 1.3 + 0.03 * h };
    let head = if tram { 1.4 } else { 0.55 };
    let steel = col(STEEL, 1.0);
    // The footing, a little into the ground.
    g.part(Surface::Concrete).boxed(base + Vec3::Y * 0.2, Vec3::new(foot * 2.0 + 1.4, 1.4, foot * 2.0 + 1.4), dir.y.atan2(dir.x), col(CONCRETE, 1.0));
    let arm_y = top - 1.1;
    let corner = |y: f32, sa: f32, sb: f32| {
        let k = ((y - base.y) / (arm_y - base.y)).clamp(0.0, 1.0);
        let r = foot + (head - foot) * k;
        base + Vec3::Y * (y - base.y) + along * (sa * r) + across * (sb * r)
    };
    let corners = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)];
    let leg = if tram { 0.45 } else { 0.28 };
    for &(a, b) in &corners {
        g.part(Surface::Metal).beam(corner(base.y + 0.9, a, b), corner(arm_y, a, b), leg, steel);
    }
    // Cross bracing on every face, panel by panel.
    let panels = ((arm_y - base.y) / if tram { 7.0 } else { 5.0 }).ceil().max(1.0) as usize;
    let brace = if tram { 0.16 } else { 0.1 };
    for p in 0..panels {
        let (y0, y1) = (base.y + 0.9 + (arm_y - base.y - 0.9) * p as f32 / panels as f32, base.y + 0.9 + (arm_y - base.y - 0.9) * (p + 1) as f32 / panels as f32);
        for i in 0..4 {
            let (a0, b0) = corners[i];
            let (a1, b1) = corners[(i + 1) % 4];
            g.part(Surface::Metal).beam(corner(y0, a0, b0), corner(y1, a1, b1), brace, col(STEEL, 0.85));
            g.part(Surface::Metal).beam(corner(y0, a1, b1), corner(y1, a0, b0), brace, col(STEEL, 0.85));
            g.part(Surface::Metal).beam(corner(y1, a0, b0), corner(y1, a1, b1), brace, col(STEEL, 0.85));
        }
    }
    // The crossarm and a sheave train under each rope.
    let reach = spec.gauge * 0.5 + 1.2;
    let mid = base + Vec3::Y * (arm_y - base.y);
    g.part(Surface::Metal).beam(mid - across * reach, mid + across * reach, if tram { 0.9 } else { 0.5 }, col(STEEL, 1.05));
    for s in [-1.0f32, 1.0] {
        let at = mid + across * (s * spec.gauge * 0.5);
        g.part(Surface::Metal).beam(at, at + Vec3::Y * 0.6, 0.22, col(DARK, 1.0));
        let train = if tram { 3.4 } else { 2.2 };
        g.part(Surface::Metal).boxed(at + Vec3::Y * 0.95, Vec3::new(train, 0.35, 0.4), dir.y.atan2(dir.x), col(DARK, 1.2));
        // Wheels of the train: small dark drums.
        for k in [-0.35f32, 0.35] {
            let wpos = at + Vec3::Y * 1.0 + along * (k * train);
            warn_free_drum(g, wpos, along);
        }
        if h > WARN_HEIGHT {
            let lamp = at + across * (s * 0.9) + Vec3::Y * 0.2;
            warn.boxed(lamp, Vec3::splat(0.45), 0.0, [1.0, 1.0, 1.0, 1.0]);
        }
    }
    // A maintenance ladder up one leg.
    let (a, b) = corners[0];
    let rail = corner(base.y + 1.0, a, b) + across * 0.3;
    let rail_top = corner(arm_y, a, b) + across * 0.3;
    g.part(Surface::Metal).beam(rail, rail_top, 0.06, col(STEEL, 0.7));
    if h > WARN_HEIGHT {
        // And a light half way up the tallest.
        warn.boxed(corner(base.y + h * 0.55, 1.0, 1.0) + along * 0.4, Vec3::splat(0.35), 0.0, [1.0, 1.0, 1.0, 1.0]);
    }
}

fn warn_free_drum(g: &mut Geometry, at: Vec3, along: Vec3) {
    g.part(Surface::Metal).beam(at - along * 0.18, at + along * 0.18, 0.34, col(DARK, 1.0));
}

/// Which way a line runs through station `i` (unit, sim plane).
fn station_dir(l: &CableLine, i: usize) -> SVec2 {
    let pts: Vec<SVec2> = l.supports.iter().filter(|s| s.station.is_some()).map(|s| s.pos).collect();
    let n = pts.len();
    if n < 2 {
        return SVec2::X;
    }
    let d = if i == 0 {
        pts[1] - pts[0]
    } else if i + 1 >= n {
        pts[n - 1] - pts[n - 2]
    } else {
        (pts[i] - pts[i - 1]).normalize_or_zero() + (pts[i + 1] - pts[i]).normalize_or_zero()
    };
    d.normalize_or_zero()
}

/// A station hall: the platform deck at the floor, glass sides, a steel
/// roof over the rope, the bullwheel's drive house at a terminal, a neon
/// band in the line's colour; on the ground or a hillside a concrete base
/// down to the street, on a roof a lift tower.
#[allow(clippy::too_many_arguments)]
fn station(g: &mut Geometry, st: &CableStation, dir: SVec2, spec: &Spec, end: Option<bool>, colour: [f32; 3], tram: bool) {
    let yaw = dir.y.atan2(dir.x);
    let along = Vec3::new(dir.x, 0.0, dir.y);
    let across = Vec3::new(-dir.y, 0.0, dir.x);
    let c = v3(st.pos, st.floor);
    let half = spec.station_half + 2.0;
    let wide = spec.gauge * 0.5 + 3.4;
    let rope = st.floor + spec.hanger;
    let roof_y = rope + 2.2;
    // The deck and, below it, the base or the plinth.
    g.part(Surface::Concrete).boxed(c - Vec3::Y * 0.35, Vec3::new(half * 2.0, 0.7, wide * 2.0), yaw, col(CONCRETE, 1.05));
    let drop = st.floor - st.ground;
    if !st.roof && drop > 0.3 {
        // A ticket hall under the platform, down to the street or slope.
        let base_top = st.floor - 0.7;
        let lo = st.ground - 3.0;
        g.part(Surface::WornConcrete).boxed(v3(st.pos, (base_top + lo) * 0.5), Vec3::new(half * 1.7, base_top - lo, wide * 1.6), yaw, col(CONCRETE, 0.85));
        // A lit entrance door at the front.
        g.part(Surface::Glow).boxed(v3(st.pos, st.ground + 1.3) - across * (wide * 0.8 + 0.02), Vec3::new(3.0, 2.4, 0.06), yaw, [1.4, 1.2, 0.9, 1.0]);
    } else if st.roof {
        // The lift tower and stair the riders come up by.
        let t = c - along * (half * 0.6) - across * (wide + 1.6);
        g.part(Surface::Panel).boxed(t + Vec3::Y * 2.0, Vec3::new(3.0, 4.0, 3.0), yaw, col([0.36, 0.37, 0.4], 1.0));
        g.part(Surface::Glow).boxed(t + Vec3::Y * 1.3 + across * 1.52, Vec3::new(1.4, 2.2, 0.05), yaw, [1.3, 1.1, 0.8, 1.0]);
    }
    // Posts along both sides and glass between them.
    let posts = ((half * 2.0) / 4.0).ceil() as i32;
    for s in [-1.0f32, 1.0] {
        for k in 0..=posts {
            let x = -half + 2.0 * half * k as f32 / posts as f32;
            let p = c + along * x + across * (s * wide);
            g.part(Surface::Metal).boxed(p + Vec3::Y * (roof_y - st.floor) * 0.5, Vec3::new(0.3, roof_y - st.floor, 0.3), yaw, col(STEEL, 1.0));
        }
        let glass = c + across * (s * wide) + Vec3::Y * 1.5;
        g.part(Surface::Glass).boxed(glass, Vec3::new(half * 2.0, 2.6, 0.1), yaw, [0.45, 0.55, 0.6, 1.0]);
        // The neon band in the line's colour under the eaves.
        g.part(Surface::Glow).boxed(c + across * (s * (wide + 0.1)) + Vec3::Y * (roof_y - st.floor - 0.35), Vec3::new(half * 2.0, 0.22, 0.06), yaw, col(colour, 2.2));
        // The platform edge lights and the rail the cabins run on inside.
        g.part(Surface::Glow).boxed(c + across * (s * (spec.gauge * 0.5 - 1.3)) + Vec3::Y * 0.04, Vec3::new(half * 1.8, 0.04, 0.16), yaw, [1.6, 1.3, 0.7, 1.0]);
        g.part(Surface::Metal).beam(c + along * (-half) + across * (s * spec.gauge * 0.5) + Vec3::Y * spec.hanger, c + along * half + across * (s * spec.gauge * 0.5) + Vec3::Y * spec.hanger, 0.3, col(DARK, 1.0));
    }
    // The roof: a shallow gable over the rope, steel above, dark below.
    let ridge = c + Vec3::Y * (roof_y + 1.2 - st.floor);
    let over = half + 1.0;
    for s in [-1.0f32, 1.0] {
        let eave = c + across * (s * (wide + 1.0)) + Vec3::Y * (roof_y - st.floor);
        let (a0, a1) = (ridge - along * over, ridge + along * over);
        let (b0, b1) = (eave - along * over, eave + along * over);
        let n = (a1 - a0).cross(b0 - a0).normalize_or_zero();
        let n = if n.y < 0.0 { -n } else { n };
        g.part(Surface::Panel).quad_facing([a0, a1, b1, b0], n, col([0.30, 0.31, 0.33], 1.0));
        let lo = |p: Vec3| p - Vec3::Y * 0.2;
        g.part(Surface::Panel).quad_facing([lo(a0), lo(a1), lo(b1), lo(b0)], -n, col([0.16, 0.16, 0.18], 1.0));
        // The gutter along the eave.
        g.part(Surface::Metal).beam(b0, b1, 0.22, col(DARK, 1.0));
    }
    // A terminal's drive house over its bullwheel, at the far end.
    if let Some(first) = end {
        let back = if first { -1.0 } else { 1.0 };
        let wheel = c + along * (back * spec.station_half * 0.45) + Vec3::Y * spec.hanger;
        let house = c + along * (back * (half + 3.2)) + Vec3::Y * (roof_y - st.floor - 1.0);
        g.part(Surface::Panel).boxed(house, Vec3::new(6.0, 5.0, wide * 2.0 - 1.0), yaw, col([0.33, 0.34, 0.37], 1.0));
        g.part(Surface::Glow).boxed(house + across * (wide - 0.45) + Vec3::Y * 1.2, Vec3::new(3.6, 0.6, 0.06), yaw, col(colour, 2.4));
        // The counterweight shaft (tensioning) behind it, on a gondola.
        if !tram {
            let tower = wheel + along * (back * 7.5);
            g.part(Surface::Metal).boxed(v3(SVec2::new(tower.x, tower.z), (st.floor + roof_y) * 0.5), Vec3::new(1.6, roof_y - st.floor, 1.6), yaw, col(STEEL, 0.9));
        }
    }
}

/// The ropes of a line: one per side, sampled every few metres along the
/// sag; a tramway's track ropes in pairs with the haul rope.
fn ropes(g: &mut Geometry, l: &CableLine) {
    let spec = l.spec();
    let total = l.length();
    let tram = l.kind == CableKind::Tramway;
    let step = 6.0;
    let rope = col(DARK, 1.4);
    let sides: Vec<(f32, f32)> = if tram {
        vec![(spec.gauge * 0.5 - 0.35, 0.0), (spec.gauge * 0.5 + 0.35, 0.0), (spec.gauge * 0.5, -0.9), (-spec.gauge * 0.5 - 0.35, 0.0), (-spec.gauge * 0.5 + 0.35, 0.0), (-spec.gauge * 0.5, -0.9)]
    } else {
        vec![(spec.gauge * 0.5, 0.0), (-spec.gauge * 0.5, 0.0)]
    };
    let thick = if tram { 0.11 } else { 0.08 };
    let (from, to) = (spec.station_half * 0.2, total - spec.station_half * 0.2);
    for &(side, dz) in &sides {
        let mut s = from;
        let mut last: Option<Vec3> = None;
        while s <= to + 0.01 {
            let (p, _) = l.point_at(s, side);
            let q = v3(p, l.rope_z(s) + dz);
            if let Some(a) = last {
                g.part(Surface::Metal).beam(a, q, thick, rope);
            }
            last = Some(q);
            if s >= to {
                break;
            }
            s = (s + step).min(to);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_cable(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut st: ResMut<CableRender>,
    cams: Query<&GlobalTransform, With<crate::camera::OrbitCamera>>,
    mut cabins: Query<(&mut Transform, &mut Visibility), (With<CabinTag>, Without<WheelTag>)>,
    mut wheels: Query<&mut Transform, (With<WheelTag>, Without<CabinTag>)>,
) {
    let w = &sim.world;
    let cw = &w.cable;
    if sim.full_rebuild {
        st.key = None;
    }
    let key = {
        let mut h = DefaultHasher::new();
        (cw.version, cw.lines.len(), cw.stations.len()).hash(&mut h);
        for (id, l) in &cw.lines {
            (slotmap::Key::data(&id).as_ffi(), l.cabins, l.supports.len(), l.color, l.kind as u8).hash(&mut h);
        }
        h.finish()
    };
    if st.key != Some(key) {
        st.key = Some(key);
        for e in st.statics.drain(..) {
            commands.entity(e).despawn();
        }
        for (_, v) in st.cabins.drain() {
            for e in v {
                commands.entity(e).despawn();
            }
        }
        for (e, _, _) in st.wheels.drain(..) {
            commands.entity(e).despawn();
        }
        if st.warn.is_none() {
            st.warn = Some(materials.add(StandardMaterial { base_color: Color::linear_rgb(0.9, 0.05, 0.03), emissive: LinearRgba::rgb(30.0, 1.0, 0.5), unlit: true, ..default() }));
        }
        let mut g = Geometry::default();
        let mut warn = MeshData::default();
        let mut drawn: HashSet<nd_sim::cable::CableStationId> = HashSet::new();
        for (_, l) in &cw.lines {
            let spec = l.spec();
            let tram = l.kind == CableKind::Tramway;
            ropes(&mut g, l);
            for p in l.pylons() {
                let (_, dir) = l.point_at(p.s, 0.0);
                pylon(&mut g, &mut warn, v3(p.pos, p.base), p.top, dir, spec, tram);
            }
            let colour = nd_procgen::metro::line_colour(l.color);
            let n = l.stations.len();
            for (i, sid) in l.stations.iter().enumerate() {
                if !drawn.insert(*sid) {
                    continue;
                }
                let Some(s) = cw.stations.get(*sid) else { continue };
                let end = if i == 0 { Some(true) } else if i + 1 == n { Some(false) } else { None };
                station(&mut g, s, station_dir(l, i), spec, end, colour, tram);
            }
        }
        // Stations no line calls at any more (a line just closed).
        for (sid, s) in cw.stations.iter() {
            if drawn.insert(sid) {
                station(&mut g, s, SVec2::X, &nd_sim::cable::GONDOLA, None, [0.6, 0.6, 0.6], false);
            }
        }
        st.statics = spawn_geometry(&mut commands, &mut meshes, &mats, &g, CableTag);
        if !warn.is_empty() {
            let e = commands.spawn((Mesh3d(meshes.add(to_mesh(&warn, false))), MeshMaterial3d(st.warn.clone().unwrap()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, CableTag)).id();
            st.statics.push(e);
        }
        // The cabins, sharing a mesh per surface per kind and colour.
        for (id, l) in &cw.lines {
            let k = (l.kind, l.color);
            if !st.models.contains_key(&k) {
                let colour = nd_procgen::metro::line_colour(l.color);
                let model = cabin_model(l.kind, colour);
                let parts = model.iter().map(|(s, m)| (s, meshes.add(shared_mesh(m, !s.is_glow())))).collect();
                st.models.insert(k, parts);
            }
            let parts = st.models[&k].clone();
            let mut list = Vec::with_capacity(l.cabins as usize);
            for _ in 0..l.cabins {
                let parent = commands.spawn((super::offstage(), Visibility::Visible, CabinTag)).id();
                for (s, m) in &parts {
                    let mut e = commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mats.surface(*s)), Transform::IDENTITY, ChildOf(parent)));
                    if s.is_glow() {
                        e.insert((NotShadowCaster, NotShadowReceiver));
                    }
                }
                list.push(parent);
            }
            st.cabins.insert(id, list);
            // Bullwheels at a gondola's terminals.
            if l.kind == CableKind::Gondola && l.stations.len() >= 2 {
                let spec = l.spec();
                let r = (spec.gauge * 0.5 * 10.0).round() as u8;
                if !st.wheel_mesh.contains_key(&r) {
                    let wm = wheel_model(spec.gauge * 0.5);
                    let parts = wm.iter().map(|(s, m)| (s, meshes.add(shared_mesh(m, true)))).collect();
                    st.wheel_mesh.insert(r, parts);
                }
                let parts = st.wheel_mesh[&r].clone();
                for (i, dir) in [(0usize, 1.0f32), (l.stations.len() - 1, -1.0)] {
                    let Some(s) = cw.stations.get(l.stations[i]) else { continue };
                    let out = station_dir(l, i) * if i == 0 { 1.0 } else { -1.0 };
                    let c = s.pos - out * (spec.station_half * 0.45);
                    let parent = commands.spawn((Transform::from_translation(v3(c, s.floor + spec.hanger)), Visibility::Visible, WheelTag)).id();
                    for (sf, m) in &parts {
                        commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mats.surface(*sf)), Transform::IDENTITY, ChildOf(parent)));
                    }
                    st.wheels.push((parent, id, dir));
                }
            }
        }
    }
    if cw.lines.is_empty() {
        return;
    }
    // The warning lights blink after dusk (and dimly by day).
    let t = time.elapsed_secs();
    let night = super::night_strength(&sim, &lighting);
    if let Some(m) = st.warn.as_ref().and_then(|h| materials.get_mut(h)) {
        let on = (t % 1.6) < 0.55;
        let k = if on { 4.0 + 26.0 * night } else { 0.15 };
        let mut m = m;
        m.emissive = LinearRgba::rgb(k, k * 0.04, k * 0.02);
    }
    let cam = cams.iter().next().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
    let secs = w.config.traffic_seconds_per_tick;
    let f = sim.frac();
    for (id, l) in &cw.lines {
        let Some(list) = st.cabins.get(&id) else { continue };
        let adv = if l.running { l.advance_per_tick(secs) } else { 0.0 };
        let phase = l.phase + adv * f;
        let layout = (l.kind == CableKind::Gondola).then(|| l.loop_layout());
        let spec = l.spec();
        let pivot = Vec3::Y * spec.hanger;
        for (k, e) in list.iter().enumerate() {
            let Ok((mut tf, mut vis)) = cabins.get_mut(*e) else { continue };
            let pose = match &layout {
                Some(lay) => {
                    let u = (phase + k as f32 * lay.length / l.cabins.max(1) as f32).rem_euclid(lay.length.max(1.0));
                    l.loop_pose(lay, u)
                }
                None => l.shuttle_pose(k as u32, phase, secs),
            };
            let at = v3(pose.pos, pose.z);
            if at.distance(cam) > CABIN_RANGE {
                vis.set_if_neq(Visibility::Hidden);
                continue;
            }
            vis.set_if_neq(Visibility::Visible);
            // A little sway in the wind, more as it slows through a station.
            let ph = k as f32 * 1.7;
            let calm = if l.running { 1.0 } else { 0.35 };
            let roll = calm * (0.022 * (t * 0.9 + ph).sin() + 0.012 * (t * 2.3 + ph * 0.5).sin());
            let pitch = calm * (0.05 * (1.0 - pose.pace) * (t * 1.9 + ph).sin() + if pose.station.is_some() { 0.03 * (1.0 - pose.pace) } else { 0.0 });
            let rot = Quat::from_rotation_y(-pose.heading) * Quat::from_rotation_x(roll) * Quat::from_rotation_z(pitch);
            tf.rotation = rot;
            tf.translation = at + pivot - rot * pivot;
        }
    }
    // The bullwheels turn while the rope runs.
    let dt = if sim.paused { 0.0 } else { time.delta_secs() };
    st.spin = (st.spin + dt * 0.9) % std::f32::consts::TAU;
    for (e, id, dir) in &st.wheels {
        let running = cw.lines.get(*id).is_some_and(|l| l.running);
        if let Ok(mut tf) = wheels.get_mut(*e) {
            if running {
                tf.rotation = Quat::from_rotation_y(st.spin * dir);
            }
        }
    }
}
