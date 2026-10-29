//! [horizon] The neighbouring towns: where the highway, the railway and
//! the roads off the map lead, a town stands on the far land, seen as a
//! skyline in the haze by day and a field of lights under a glow at night.
//!
//! A town is placed where the ways leaving the map point (one per bearing
//! they share), plus a couple more from the seed alone, each on dry, fairly
//! level far land well past the map. Placement is a pure function of the
//! far land, the ways' bearings and the seed. Later these become real
//! places that trade with the city (`todo.md`, "A big world", step 3).
//!
//! Meshes are in the town's own frame (origin at its centre, on its
//! ground): the renderer puts each at its place, so the translucent
//! skylines sort by distance.

use super::land::{FarLand, HORIZON};
use crate::hash::hash2_salted;
use crate::mesh::MeshData;
use glam::{Vec2, Vec3};

/// Towns stand this far from the centre of the map, and no nearer one another.
pub const NEAREST: f32 = 6200.0;
pub const FARTHEST: f32 = 11500.0;
const APART: f32 = 2600.0;
/// Bearings within this many radians lead to the same town.
const SAME_WAY: f32 = 0.42;

const SALT: u64 = 0x70_57_0E_11;

fn unit(seed: u64, k: u64) -> f32 {
    (hash2_salted(seed, k, SALT) >> 40) as f32 / (1u64 << 24) as f32
}

#[derive(Clone, Debug, PartialEq)]
pub struct FarTown {
    pub pos: Vec2,
    /// Height of the far ground at its centre.
    pub ground: f32,
    /// 0..1: a village to a city.
    pub size: f32,
    /// Built-up radius, metres.
    pub radius: f32,
    /// Its tallest tower, metres.
    pub peak: f32,
    pub seed: u64,
    /// Does a way from the map lead here?
    pub on_way: bool,
}

/// Group bearings (unit directions out of the map) that lead the same way:
/// the mean direction of each group, in a fixed order.
pub fn bearings(exits: &[Vec2]) -> Vec<Vec2> {
    let mut angles: Vec<f32> = exits.iter().filter(|d| d.length_squared() > 1e-6).map(|d| d.y.atan2(d.x)).collect();
    angles.sort_by(|a, b| a.total_cmp(b));
    let mut groups: Vec<Vec<f32>> = Vec::new();
    for a in angles {
        match groups.last_mut() {
            Some(g) if a - g[g.len() - 1] < SAME_WAY => g.push(a),
            _ => groups.push(vec![a]),
        }
    }
    // The first and last groups may meet across ±π.
    if groups.len() > 1 {
        let first = groups[0][0];
        let last = *groups[groups.len() - 1].last().unwrap();
        if first + std::f32::consts::TAU - last < SAME_WAY {
            let tail = groups.pop().unwrap();
            groups[0].extend(tail.into_iter().map(|a| a - std::f32::consts::TAU));
        }
    }
    groups.iter().map(|g| g.iter().fold(Vec2::ZERO, |s, a| s + Vec2::new(a.cos(), a.sin())).normalize_or(Vec2::X)).collect()
}

/// The towns: one where each group of `exits` leads (if dry land can be
/// found that way), then a few from the seed alone. `ground` is the far
/// ground's height.
pub fn place(far: &FarLand, exits: &[Vec2], ground: &dyn Fn(Vec2) -> f32) -> Vec<FarTown> {
    let seed = far.landscape().params.seed;
    let mut towns: Vec<FarTown> = Vec::new();
    let fits = |p: Vec2, towns: &[FarTown]| p.length() <= FARTHEST + 2500.0 && towns.iter().all(|t| t.pos.distance(p) > APART) && far.town_ground(p);
    let settle = |dir: Vec2, k: u64, on_way: bool, towns: &mut Vec<FarTown>| {
        let side = Vec2::new(-dir.y, dir.x);
        let start = NEAREST + unit(seed, k) * (FARTHEST - NEAREST) * 0.6;
        for step in 0..14 {
            let along = start + step as f32 * 300.0;
            for lateral in [0.0, 450.0, -450.0, 900.0, -900.0, 1500.0, -1500.0] {
                let p = dir * along + side * lateral;
                if fits(p, towns.as_slice()) {
                    let size = if on_way { 0.45 + 0.55 * unit(seed, k + 1) } else { 0.15 + 0.5 * unit(seed, k + 1) };
                    towns.push(FarTown { pos: p, ground: ground(p), size, radius: 450.0 + 900.0 * size, peak: 45.0 + 230.0 * size * size, seed: hash2_salted(seed, k, SALT ^ 0xFF), on_way });
                    return;
                }
            }
        }
    };
    for (i, dir) in bearings(exits).into_iter().enumerate() {
        settle(dir, 100 + i as u64 * 7, true, &mut towns);
    }
    let extra = 2 + (unit(seed, 3) * 2.0) as u64;
    for k in 0..extra * 3 {
        if towns.iter().filter(|t| !t.on_way).count() as u64 >= extra {
            break;
        }
        let a = unit(seed, 500 + k) * std::f32::consts::TAU;
        settle(Vec2::new(a.cos(), a.sin()), 900 + k * 13, false, &mut towns);
    }
    towns
}

/// One town's meshes, in its own frame.
pub struct Skyline {
    /// Towers and blocks: colour is a shade of the haze (1 at the foot,
    /// darker up the towers), drawn in the fog's colour.
    pub towers: MeshData,
    /// Lit windows, street lights, red lamps on the tallest roofs: drawn
    /// additive at night.
    pub lights: MeshData,
    /// The orange glow over the town at night, additive.
    pub glow: MeshData,
}

impl Skyline {
    pub fn triangles(&self) -> usize {
        (self.towers.indices.len() + self.lights.indices.len() + self.glow.indices.len()) / 3
    }
}

/// A light seen from any side: two crossed upright quads and a flat one.
pub fn light_dot(m: &mut MeshData, at: Vec3, size: f32, rgb: [f32; 3]) {
    let c = [rgb[0], rgb[1], rgb[2], 1.0];
    let s = size * 0.5;
    for (u, v) in [(Vec3::X * s, Vec3::Y * s), (Vec3::Z * s, Vec3::Y * s), (Vec3::X * s, Vec3::Z * s)] {
        double_quad(m, [at - u - v, at + u - v, at + u + v, at - u + v], c);
    }
}

/// A quad seen from both sides (the light materials cull back faces).
fn double_quad(m: &mut MeshData, q: [Vec3; 4], c: [f32; 4]) {
    let n = (q[1] - q[0]).cross(q[3] - q[0]).normalize_or(Vec3::Y);
    let base = m.positions.len() as u32;
    for p in q {
        m.positions.push(p.to_array());
        m.normals.push(n.to_array());
        m.colors.push(c);
        m.uvs.push([0.0, 0.0]);
    }
    m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// A box tower: four sides and a roof, `shade` from 1 at the foot to
/// `top` at the roof (the haze is thickest low down).
fn tower(m: &mut MeshData, at: Vec3, w: f32, d: f32, h: f32, yaw: f32, top: f32) {
    let (s, c) = yaw.sin_cos();
    let rot = |x: f32, z: f32| Vec3::new(x * c - z * s, 0.0, x * s + z * c);
    let corners = [rot(-w, -d), rot(w, -d), rot(w, d), rot(-w, d)].map(|v| v * 0.5 + at);
    let up = Vec3::Y * h;
    let foot = [1.0, 1.0, 1.0, 1.0];
    let roof = [top, top, top, 1.0];
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        let n = Vec3::Y.cross(b - a).normalize_or(Vec3::X);
        let base = m.positions.len() as u32;
        for (p, col) in [(a, foot), (b, foot), (b + up, roof), (a + up, roof)] {
            m.positions.push(p.to_array());
            m.normals.push(n.to_array());
            m.colors.push(col);
            m.uvs.push([0.0, 0.0]);
        }
        // (a, b+up, b) faces out.
        m.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let base = m.positions.len() as u32;
    for p in corners {
        m.positions.push((p + up).to_array());
        m.normals.push([0.0, 1.0, 0.0]);
        m.colors.push(roof);
        m.uvs.push([0.0, 0.0]);
    }
    m.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// Warm sodium, cold white, the odd neon.
fn light_colour(r: f32) -> [f32; 3] {
    if r < 0.62 {
        [1.0, 0.52, 0.16]
    } else if r < 0.9 {
        [0.75, 0.82, 1.0]
    } else if r < 0.95 {
        [1.0, 0.2, 0.75]
    } else {
        [0.2, 0.9, 1.0]
    }
}

/// The town's skyline and lights. `ground(offset)` is the far ground's
/// height at an offset from its centre, less the centre's.
pub fn skyline(t: &FarTown, ground: &dyn Fn(Vec2) -> f32) -> Skyline {
    let r = |k: u64| unit(t.seed, k);
    let mut towers = MeshData::default();
    let mut lights = MeshData::default();
    let count = 30 + (t.size * 120.0) as u64;
    let lit = |k: u64| r(k) < 0.6;
    for i in 0..count {
        let k = 1000 + i * 17;
        let ang = r(k) * std::f32::consts::TAU;
        let dist = t.radius * r(k + 1).powf(1.35);
        let off = Vec2::new(ang.cos(), ang.sin()) * dist;
        let core = 1.0 - dist / t.radius;
        let tall = r(k + 2);
        // Towers in the middle, low wide blocks out in the sprawl.
        let (w, d, h) = if tall < 0.35 + 0.4 * core {
            let h = 14.0 + t.peak * core.powf(1.6) * (0.3 + 0.7 * r(k + 3));
            let w = 18.0 + 22.0 * r(k + 4) + h * 0.12;
            (w, w * (0.7 + 0.5 * r(k + 5)), h)
        } else {
            (50.0 + 70.0 * r(k + 4), 40.0 + 60.0 * r(k + 5), 9.0 + 12.0 * r(k + 3))
        };
        let foot = ground(off) - 4.0;
        let at = Vec3::new(off.x, foot, off.y);
        let top = 0.86 - 0.32 * (h / (t.peak + 20.0)).min(1.0);
        tower(&mut towers, at, w, d, h + 4.0, r(k + 6) * 1.6, top);
        // Lit floors: a few bands of windows on two faces.
        if h > 20.0 {
            let bands = 1 + (h / 30.0) as u64;
            for b in 0..bands {
                if !lit(k + 20 + b) {
                    continue;
                }
                let y = foot + 6.0 + (h - 8.0) * (b as f32 + 0.3 + 0.4 * r(k + 40 + b)) / bands as f32;
                let warm = light_colour(r(k + 60 + b) * 0.9);
                let glow = 0.35 + 0.4 * r(k + 80 + b);
                let rgb = [warm[0] * glow, warm[1] * glow, warm[2] * glow];
                let band = (h / bands as f32 * 0.45).clamp(6.0, 22.0);
                light_dot(&mut lights, Vec3::new(off.x, y, off.y), band.max(w * 0.55), rgb);
            }
            if h > 110.0 {
                light_dot(&mut lights, Vec3::new(off.x, foot + h + 7.0, off.y), 7.0, [1.6, 0.05, 0.03]);
            }
        }
    }
    // Street lights along a loose grid of streets out into the sprawl.
    let streets = 80 + (t.size * 320.0) as u64;
    for i in 0..streets {
        let k = 50_000 + i * 11;
        let ang = r(k) * std::f32::consts::TAU;
        let dist = t.radius * 1.35 * r(k + 1).sqrt();
        let mut off = Vec2::new(ang.cos(), ang.sin()) * dist;
        // Onto the grid: one coordinate snapped to a street every 90 m.
        if r(k + 2) < 0.5 {
            off.x = (off.x / 90.0).round() * 90.0;
        } else {
            off.y = (off.y / 90.0).round() * 90.0;
        }
        let c = light_colour(r(k + 3));
        let k2 = 0.5 + 0.5 * r(k + 4);
        light_dot(&mut lights, Vec3::new(off.x, ground(off) + 6.0, off.y), 11.0, [c[0] * k2, c[1] * k2, c[2] * k2]);
    }
    Skyline { towers, lights, glow: glow_dome(t.radius * 1.7, t.radius * 0.45 + t.peak) }
}

/// The warm haze of light over the town at night: two soft discs of glow
/// hanging over it, brightest in the middle and fading to nothing at their
/// rims, so from far off they read as a glow hugging the town and from
/// above as a soft pool. Drawn additive, so the fade is in the colour (an
/// additive material ignores alpha), and from both sides.
fn glow_dome(radius: f32, height: f32) -> MeshData {
    let mut m = MeshData::default();
    let (segs, rings) = (32usize, 6usize);
    for (y, k) in [(height * 0.25, 1.0), (height * 0.7, 0.55)] {
        let start = m.positions.len() as u32;
        for j in 0..=rings {
            let v = j as f32 / rings as f32;
            let fade = (1.0 - v).powf(2.2) * k;
            for i in 0..=segs {
                let u = i as f32 / segs as f32 * std::f32::consts::TAU;
                m.positions.push([radius * v * u.cos(), y, radius * v * u.sin()]);
                m.normals.push([0.0, 1.0, 0.0]);
                m.colors.push([0.30 * fade, 0.14 * fade, 0.05 * fade, 1.0]);
                m.uvs.push([u, v]);
            }
        }
        let row = segs as u32 + 1;
        for j in 0..rings as u32 {
            for i in 0..segs as u32 {
                let a = start + j * row + i;
                let (b, c, d) = (a + 1, a + row, a + row + 1);
                m.indices.extend_from_slice(&[a, c, b, b, c, d, a, b, c, b, d, c]);
            }
        }
    }
    m
}

/// How far a town's skyline may be seen at most.
pub const SEEN_TO: f32 = HORIZON;
