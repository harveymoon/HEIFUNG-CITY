//! [dirty-industry] Clean-up crews at work (`World::cleanups`, sent with
//! the Bulldoze tool's Clean up ground): the site is fenced with posts and
//! red-and-white tape at its full radius, the ground already dug out is
//! fresh brown earth out to the radius reached, spoil heaps stand along the
//! dig front, workers in hi-vis stand by, and yellow excavators work the
//! front (on open ground, never in a street or on a building), swinging
//! their arms, an amber beacon on each cab.
//!
//! The site is one merged geometry, rebuilt when a crew comes or goes or
//! its dig moves on a step (every 2%); the excavators are their own
//! entities so they can swing every frame.

use super::*;
use std::f32::consts::TAU;

#[derive(Component, Clone)]
pub(super) struct CleanupTag;

/// An excavator: it swings back and forth about its own middle.
#[derive(Component, Clone)]
pub(super) struct Digger {
    yaw: f32,
    phase: f32,
}

#[derive(Resource, Default)]
pub(super) struct CleanupRender {
    key: Option<u64>,
    ents: Vec<Entity>,
}

const EARTH: [f32; 4] = [0.2, 0.13, 0.07, 1.0];
const SPOIL: [f32; 4] = [0.27, 0.19, 0.11, 1.0];
const YELLOW: [f32; 4] = [0.95, 0.62, 0.05, 1.0];
const TRACK: [f32; 4] = [0.06, 0.06, 0.06, 1.0];

/// An excavator at the origin, facing +x: tracks, a turning body and cab,
/// the boom and stick reaching out and down to a bucket, a beacon.
fn excavator() -> Geometry {
    let mut g = Geometry::default();
    for side in [-1.0, 1.0] {
        g.part(Surface::Metal).boxed(Vec3::new(0.0, 0.4, side * 1.1), Vec3::new(4.2, 0.8, 0.7), 0.0, TRACK);
    }
    g.part(Surface::Matte).boxed(Vec3::new(-0.3, 1.25, 0.0), Vec3::new(3.0, 1.0, 2.4), 0.0, YELLOW);
    g.part(Surface::Matte).boxed(Vec3::new(0.55, 2.2, 0.65), Vec3::new(1.2, 1.0, 1.0), 0.0, YELLOW);
    g.part(Surface::Glass).decal(Vec3::new(1.16, 2.25, 0.65), Vec3::Z, Vec3::Y, 0.85, 0.7, Vec3::X, [0.15, 0.2, 0.25, 1.0]);
    g.part(Surface::Matte).boxed(Vec3::new(-1.7, 1.4, 0.0), Vec3::new(0.6, 0.8, 2.3), 0.0, [0.25, 0.25, 0.25, 1.0]);
    // Boom up and out, stick down to the ground.
    let boom = |g: &mut Geometry, a: Vec3, b: Vec3, t: f32| {
        let d = b - a;
        let len = d.length();
        let mid = (a + b) * 0.5;
        // A box along x, pitched in the x-y plane.
        let pitch = d.y.atan2(d.x);
        let mut m = Geometry::default();
        m.part(Surface::Matte).boxed(Vec3::ZERO, Vec3::new(len, t, t * 0.8), 0.0, YELLOW);
        for p in m.part(Surface::Matte).positions.iter_mut() {
            let (x, y) = (p[0], p[1]);
            let (s, c) = pitch.sin_cos();
            *p = [mid.x + x * c - y * s, mid.y + x * s + y * c, mid.z + p[2]];
        }
        for n in m.part(Surface::Matte).normals.iter_mut() {
            let (x, y) = (n[0], n[1]);
            let (s, c) = pitch.sin_cos();
            *n = [x * c - y * s, x * s + y * c, n[2]];
        }
        g.append(&m);
    };
    let (root, elbow, tip) = (Vec3::new(0.9, 1.7, -0.5), Vec3::new(3.4, 3.6, -0.5), Vec3::new(5.0, 0.6, -0.5));
    boom(&mut g, root, elbow, 0.45);
    boom(&mut g, elbow, tip, 0.32);
    g.part(Surface::Metal).boxed(tip + Vec3::new(0.2, -0.1, 0.0), Vec3::new(0.7, 0.6, 1.0), 0.0, [0.3, 0.27, 0.22, 1.0]);
    g.part(Surface::Glow).boxed(Vec3::new(0.55, 2.82, 0.65), Vec3::new(0.18, 0.2, 0.18), 0.0, [3.0, 1.4, 0.1, 1.0]);
    g
}

/// A worker in hi-vis at `p`, on the ground at `y`.
fn worker(g: &mut Geometry, p: SVec2, y: f32, yaw: f32) {
    g.part(Surface::Matte).boxed(Vec3::new(p.x, y + 0.45, p.y), Vec3::new(0.3, 0.9, 0.22), yaw, [0.12, 0.12, 0.16, 1.0]);
    g.part(Surface::Matte).boxed(Vec3::new(p.x, y + 1.2, p.y), Vec3::new(0.42, 0.62, 0.26), yaw, [1.0, 0.42, 0.04, 1.0]);
    g.part(Surface::Matte).boxed(Vec3::new(p.x, y + 1.62, p.y), Vec3::new(0.22, 0.22, 0.22), yaw, [0.55, 0.4, 0.3, 1.0]);
    g.part(Surface::Matte).boxed(Vec3::new(p.x, y + 1.76, p.y), Vec3::new(0.28, 0.1, 0.28), yaw, [0.98, 0.85, 0.1, 1.0]);
}

/// Excavators a crew brings: one per 40 m of radius, one to five.
fn diggers(radius: f32) -> usize {
    ((radius / 40.0).round() as usize).clamp(1, 5)
}

pub(super) fn sync_cleanup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, mats: Res<Materials>, time: Res<Time>, mut state: ResMut<CleanupRender>, mut digs: Query<(&Digger, &mut Transform)>) {
    // Swing the arms every frame.
    let t = time.elapsed_secs();
    for (d, mut tf) in &mut digs {
        tf.rotation = Quat::from_rotation_y(d.yaw + 0.6 * (t * 0.5 + d.phase).sin());
    }
    let w = &sim.world;
    let mut h = DefaultHasher::new();
    for j in &w.cleanups {
        (j.id, (j.progress() * 50.0) as u32).hash(&mut h);
        hash_f32s(&mut h, [j.center, SVec2::splat(j.radius)]);
    }
    w.relief.hash(&mut h);
    let key = h.finish();
    if state.key == Some(key) && !sim.full_rebuild {
        return;
    }
    state.key = Some(key);
    for e in state.ents.drain(..) {
        commands.entity(e).despawn();
    }
    if w.cleanups.is_empty() {
        return;
    }
    let y = |p: SVec2| w.surface_height(p);
    let mut g = Geometry::default();
    let rig = excavator();
    for j in &w.cleanups {
        let c = j.center;
        let seed = j.id as u64 ^ 0xC1EA;
        // Fresh earth out to the dig front, on the land (not the streets
        // and pavements): square cells following the ground.
        let dug = j.dug_radius().max(6.0);
        let cell = (dug / 25.0).max(4.0);
        let n = (dug / cell).ceil() as i32;
        let on_street = |q: SVec2| w.roads.nearest_where(q, |s| !s.tunnel).is_some_and(|(sid, _, at)| at.distance(q) < w.roads.segments[sid.0 as usize].half_width() + 3.0);
        let built = |q: SVec2| w.buildings.values().any(|b| std::iter::once(b.lot).chain(b.plot.extra_lots.iter().copied()).any(|l| w.lots.get(l).is_some_and(|l| l.contains(q))));
        for gy in -n..n {
            for gx in -n..n {
                let lo = c + SVec2::new(gx as f32, gy as f32) * cell;
                let mid = lo + SVec2::splat(cell * 0.5);
                if mid.distance(c) > dug || on_street(mid) {
                    continue;
                }
                let p = |q: SVec2| Vec3::new(q.x, y(q) + 0.12, q.y);
                let (a, b, cc, d) = (lo, lo + SVec2::new(cell, 0.0), lo + SVec2::splat(cell), lo + SVec2::new(0.0, cell));
                g.part(Surface::Dirt).quad_facing([p(a), p(b), p(cc), p(d)], Vec3::Y, EARTH);
            }
        }
        // Posts and tape round the whole site.
        let posts = ((TAU * j.radius / 9.0) as usize).clamp(12, 120);
        let at = |k: usize| c + SVec2::from_angle(k as f32 / posts as f32 * TAU) * j.radius;
        for k in 0..posts {
            let (p, q) = (at(k), at(k + 1));
            g.part(Surface::Matte).cylinder(p, 0.05, 0.05, y(p), y(p) + 1.1, 5, [0.9, 0.9, 0.85, 1.0]);
            let d = (q - p).normalize_or_zero();
            let nrm = Vec3::new(d.y, 0.0, -d.x);
            for (hgt, col) in [(1.0, [0.95, 0.1, 0.08, 1.0]), (0.6, [0.95, 0.95, 0.95, 1.0])] {
                let (a, b) = (Vec3::new(p.x, y(p) + hgt, p.y), Vec3::new(q.x, y(q) + hgt, q.y));
                g.part(Surface::Matte).quad_facing([a, b, b + Vec3::Y * 0.08, a + Vec3::Y * 0.08], nrm, col);
                g.part(Surface::Matte).quad_facing([b, a, a + Vec3::Y * 0.08, b + Vec3::Y * 0.08], -nrm, col);
            }
        }
        // Spoil heaps along the front, more as the dig goes on.
        let heaps = 3 + (j.progress() * 8.0) as usize;
        for k in 0..heaps {
            let a = (k as f32 + nd_sim::pedestrians::hash01(seed, k as u64, 1)) / heaps as f32 * TAU;
            let p = c + SVec2::from_angle(a) * (dug + 4.0).min(j.radius - 3.0).max(3.0);
            let r = 2.2 + 1.5 * nd_sim::pedestrians::hash01(seed, k as u64, 2);
            g.part(Surface::Dirt).cylinder(p, r, r * 0.25, y(p), y(p) + r * 0.7, 9, SPOIL);
        }
        // The excavators on the front, and workers by them.
        let m = diggers(j.radius);
        for k in 0..m {
            // On open ground at the front, off the streets and clear of
            // buildings; none where there is no such ground.
            let place = |t: usize| {
                let a = (k as f32 + 0.5) / m as f32 * TAU + j.progress() * 2.0 + t as f32 / 16.0 * TAU;
                (a, c + SVec2::from_angle(a) * (dug - 2.0).max(2.0))
            };
            let Some((a, p)) = (0..16).map(place).find(|(_, q)| !on_street(*q) && !built(*q)) else { continue };
            let yaw = -a;
            for (s, data) in rig.iter() {
                let mesh = to_mesh(data, !s.is_glow());
                let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.surface(s)), Transform::from_xyz(p.x, y(p) + 0.05, p.y).with_rotation(Quat::from_rotation_y(yaw)), CleanupTag, Digger { yaw, phase: k as f32 * 1.7 }));
                if s.is_glow() {
                    e.insert((NotShadowCaster, NotShadowReceiver));
                }
                state.ents.push(e.id());
            }
            for wk in 0..2 {
                let q = p + SVec2::from_angle(a + 0.5 + wk as f32 * 0.4) * 7.0;
                worker(&mut g, q, y(q), a + wk as f32);
            }
        }
    }
    if g.triangle_count() > 0 {
        state.ents.extend(spawn_geometry(&mut commands, &mut meshes, &mats, &g, CleanupTag));
    }
}
