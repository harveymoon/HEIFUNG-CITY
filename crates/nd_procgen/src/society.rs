//! Street-level society props, drawn by the app from the simulation's
//! state (`nd_sim::homeless`, `nd_sim::trash`): skid row camps (dome tents,
//! tarp lean-tos, shopping carts piled with bags, mattresses, cardboard and
//! a drum fire), the old RVs and vans people live in at the kerb, and the
//! trash that overflows onto the pavement when nobody collects it (wheelie
//! bins or a dumpster with the lids forced up and bags heaped around them).
//!
//! Everything is laid out in world coordinates around a point with an
//! "along" direction (the kerb) and an "out" direction (towards the street),
//! from a seed, so a camp keeps its look as long as its size does not change.

use crate::buildings::parts::{mul, pick, rgba, DARK};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::Rng;
use std::f32::consts::{PI, TAU};

/// Height of the pavement surface (as the street generator lays it).
pub const PAVE_Y: f32 = 0.155;
/// Height of a lot's ground.
pub const LOT_Y: f32 = 0.04;

/// A frame on the ground: `along` the kerb, `out` towards the street.
#[derive(Clone, Copy)]
struct Frame {
    p: Vec2,
    along: Vec2,
    out: Vec2,
    y: f32,
}

impl Frame {
    fn new(p: Vec2, along: Vec2, out: Vec2, y: f32) -> Self {
        let along = along.try_normalize().unwrap_or(Vec2::X);
        let mut out = out.try_normalize().unwrap_or(Vec2::new(-along.y, along.x));
        // Keep `out` square to `along`.
        out = (out - along * out.dot(along)).try_normalize().unwrap_or(Vec2::new(-along.y, along.x));
        Self { p, along, out, y }
    }

    fn at(&self, u: f32, v: f32) -> Vec2 {
        self.p + self.along * u + self.out * v
    }

    fn yaw(&self) -> f32 {
        self.along.y.atan2(self.along.x)
    }

    fn v3(&self, u: f32, v: f32, h: f32) -> Vec3 {
        let q = self.at(u, v);
        Vec3::new(q.x, self.y + h, q.y)
    }
}

fn ring(c: Vec2, r: f32, sides: usize, rot: f32, y: f32) -> Vec<Vec3> {
    MeshData::ngon(c, r, sides, rot, y)
}

/// A dome tent: a low many-sided cone with a rounded top and a dark door.
fn dome_tent(g: &mut Geometry, f: &Frame, u: f32, v: f32, r: f32, h: f32, col: [f32; 3], rng: &mut impl Rng) {
    let c = f.at(u, v);
    let rot = rng.random_range(0.0..TAU);
    let base = ring(c, r, 8, rot, f.y);
    let mid = ring(c, r * 0.72, 8, rot, f.y + h * 0.65);
    let top = ring(c, r * 0.2, 8, rot, f.y + h);
    let m = g.part(Tarp);
    m.frustum(&base, &mid, rgba(col));
    m.frustum(&mid, &top, rgba(mul(col, 0.92)));
    m.cap(&top, Vec3::Y, rgba(mul(col, 0.9)));
    // The door, facing the street, and a guy rope pole.
    let door = f.v3(u, v + r * 0.98, h * 0.3);
    g.part(Matte).boxed(door, Vec3::new(r * 0.7, h * 0.55, 0.05), f.yaw(), rgba(DARK));
}

/// A ridge tent (A-frame) along the kerb.
fn ridge_tent(g: &mut Geometry, f: &Frame, u: f32, v: f32, len: f32, w: f32, h: f32, col: [f32; 3]) {
    let a0 = f.v3(u - len * 0.5, v - w * 0.5, 0.0);
    let a1 = f.v3(u + len * 0.5, v - w * 0.5, 0.0);
    let b0 = f.v3(u - len * 0.5, v + w * 0.5, 0.0);
    let b1 = f.v3(u + len * 0.5, v + w * 0.5, 0.0);
    let r0 = f.v3(u - len * 0.5, v, h);
    let r1 = f.v3(u + len * 0.5, v, h);
    let out3 = Vec3::new(f.out.x, 0.0, f.out.y);
    let al3 = Vec3::new(f.along.x, 0.0, f.along.y);
    let m = g.part(Tarp);
    m.quad_facing([a0, a1, r1, r0], (-out3 + Vec3::Y).normalize(), rgba(col));
    m.quad_facing([b0, b1, r1, r0], (out3 + Vec3::Y).normalize(), rgba(mul(col, 0.9)));
    m.tri_facing(a0, b0, r0, -al3, rgba(mul(col, 0.8)));
    m.tri_facing(a1, b1, r1, al3, rgba(mul(col, 0.8)));
}

/// A tarp strung between two poles and a fence or wall, sloping to the ground.
fn lean_to(g: &mut Geometry, f: &Frame, u: f32, v: f32, w: f32, d: f32, col: [f32; 3]) {
    let hi = 1.5;
    let back = [f.v3(u - w * 0.5, v - d * 0.5, hi), f.v3(u + w * 0.5, v - d * 0.5, hi)];
    let front = [f.v3(u - w * 0.5, v + d * 0.5, 0.25), f.v3(u + w * 0.5, v + d * 0.5, 0.25)];
    let out3 = Vec3::new(f.out.x, 0.0, f.out.y);
    g.part(Tarp).quad_facing([back[0], back[1], front[1], front[0]], (out3 + Vec3::Y).normalize(), rgba(col));
    for k in [-0.5f32, 0.5] {
        let p = f.at(u + w * k, v - d * 0.5);
        g.part(Wood).cylinder(p, 0.04, 0.04, f.y, f.y + hi, 5, rgba(mul([0.45, 0.35, 0.25], 0.8)));
    }
    // A mattress underneath.
    g.part(Matte).boxed(f.v3(u, v - d * 0.1, 0.1), Vec3::new(w * 0.7, 0.18, d * 0.55), f.yaw(), rgba([0.42, 0.38, 0.3]));
}

/// A shopping cart heaped with bags.
fn cart(g: &mut Geometry, f: &Frame, u: f32, v: f32, yaw: f32, pal: &Palette, rng: &mut impl Rng) {
    let c = f.at(u, v);
    let (s, co) = yaw.sin_cos();
    let d = Vec2::new(co, s);
    let n = Vec2::new(-s, co);
    let steel = mul(pal.steel, 1.1);
    g.part(Metal).boxed_open(Vec3::new(c.x, f.y + 0.75, c.y), Vec3::new(0.9, 0.5, 0.55), yaw, rgba(steel));
    for (a, b) in [(-0.38f32, -0.22f32), (-0.38, 0.22), (0.38, -0.22), (0.38, 0.22)] {
        let w = c + d * a + n * b;
        g.part(Metal).boxed(Vec3::new(w.x, f.y + 0.3, w.y), Vec3::new(0.04, 0.5, 0.04), yaw, rgba(steel));
        g.part(Matte).boxed(Vec3::new(w.x, f.y + 0.06, w.y), Vec3::new(0.12, 0.12, 0.04), yaw, rgba(DARK));
    }
    let h = c - d * 0.5;
    g.part(Metal).boxed(Vec3::new(h.x, f.y + 1.05, h.y), Vec3::new(0.05, 0.05, 0.6), yaw, rgba(steel));
    // The load: bags and bundles over the top.
    for k in 0..rng.random_range(2..5) {
        let q = c + d * rng.random_range(-0.3..0.3) + n * rng.random_range(-0.15..0.15);
        let col = if rng.random_bool(0.5) { [0.06, 0.06, 0.07] } else { pick(rng, &pal.tarp_colors) };
        g.part(Tarp).boxed(Vec3::new(q.x, f.y + 1.0 + k as f32 * 0.12, q.y), Vec3::new(0.45, 0.35, 0.4), yaw + k as f32, rgba(col));
    }
}

/// Cardboard, a mattress, a camp chair, bags: the clutter round a camp.
fn clutter(g: &mut Geometry, f: &Frame, u: f32, v: f32, pal: &Palette, rng: &mut impl Rng) {
    let yaw = f.yaw() + rng.random_range(-0.4..0.4);
    let q = f.v3(u, v, 0.0);
    match rng.random_range(0..4) {
        0 => {
            // Flattened cardboard and a box or two.
            g.part(Wood).boxed(q + Vec3::Y * 0.02, Vec3::new(1.6, 0.03, 0.9), yaw, rgba(mul(pal.plywood, 0.9)));
            for k in 0..rng.random_range(1..3) {
                let s = rng.random_range(0.35..0.55);
                g.part(Wood).boxed(q + Vec3::new(0.5 * k as f32, s * 0.5, 0.3), Vec3::splat(s), yaw + 0.3 * k as f32, rgba(mul(pal.plywood, 0.8)));
            }
        }
        1 => {
            // An old mattress with a sleeping bag.
            g.part(Matte).boxed(q + Vec3::Y * 0.1, Vec3::new(1.9, 0.18, 0.9), yaw, rgba([0.5, 0.46, 0.36]));
            g.part(Tarp).boxed(q + Vec3::Y * 0.24, Vec3::new(1.5, 0.12, 0.6), yaw, rgba(pick(rng, &pal.tarp_colors)));
        }
        2 => {
            // A folding chair and a crate table.
            let col = pick(rng, &pal.tarp_colors);
            g.part(Tarp).boxed(q + Vec3::Y * 0.45, Vec3::new(0.5, 0.06, 0.5), yaw, rgba(col));
            g.part(Tarp).boxed(q + Vec3::new(0.0, 0.7, 0.0) - Vec3::new(f.out.x, 0.0, f.out.y) * 0.25, Vec3::new(0.5, 0.5, 0.05), yaw, rgba(col));
            g.part(Wood).boxed(q + Vec3::new(f.along.x * 0.7, 0.25, f.along.y * 0.7), Vec3::new(0.5, 0.5, 0.4), yaw, rgba(mul(pal.plywood, 0.7)));
        }
        _ => {
            // Bags in a heap.
            for k in 0..rng.random_range(3..6) {
                let o = Vec3::new(rng.random_range(-0.5..0.5), 0.2 + (k / 3) as f32 * 0.3, rng.random_range(-0.4..0.4));
                let col = if rng.random_bool(0.6) { [0.06, 0.06, 0.07] } else { pick(rng, &pal.tarp_colors) };
                g.part(Tarp).boxed(q + o, Vec3::new(0.5, 0.4, 0.45), yaw + k as f32, rgba(col));
            }
        }
    }
}

/// A camp of `tents` tents around `p`, along `along`, opening towards
/// `out`. On a pavement it is a single file against the lot line; on a
/// vacant lot a loose cluster with a drum fire.
pub fn camp(p: Vec2, along: Vec2, out: Vec2, tents: u8, sidewalk: bool, seed: u64, pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 0xCA4F);
    let f = Frame::new(p, along, out, if sidewalk { PAVE_Y } else { LOT_Y });
    let n = tents.max(1) as usize;
    let spacing = if sidewalk { 2.3 } else { 3.0 };
    for k in 0..n {
        let (u, v) = if sidewalk {
            // Tents in a row against the lot line, a gap to walk past.
            ((k as f32 - (n as f32 - 1.0) * 0.5) * spacing, -0.6)
        } else {
            let a = k as f32 * 2.39996 + rng.random_range(-0.3..0.3);
            let r = 1.5 + (k as f32).sqrt() * 1.9;
            (a.cos() * r, a.sin() * r)
        };
        let col = pick(&mut rng, &pal.tarp_colors);
        let col = mul(col, rng.random_range(0.7..1.0));
        match rng.random_range(0..5) {
            0 | 1 => dome_tent(&mut g, &f, u, v, rng.random_range(0.8..1.05), rng.random_range(0.95..1.2), col, &mut rng),
            2 | 3 => ridge_tent(&mut g, &f, u, v, rng.random_range(1.9..2.3), rng.random_range(1.2..1.5), rng.random_range(0.9..1.15), col),
            _ => lean_to(&mut g, &f, u, v, rng.random_range(1.8..2.4), rng.random_range(1.3..1.7), col),
        }
        // A cart or some clutter between tents.
        if k % 2 == 0 {
            let (cu, cv) = if sidewalk { (u + spacing * 0.5, 0.35) } else { (u * 1.2 + 1.0, v * 1.2) };
            if rng.random_bool(0.55) {
                cart(&mut g, &f, cu, cv, f.yaw() + rng.random_range(-0.6..0.6), pal, &mut rng);
            } else {
                clutter(&mut g, &f, cu, cv, pal, &mut rng);
            }
        }
    }
    // Tarps over everything, pallets, a drum fire for the night.
    if !sidewalk || n >= 4 {
        let (u, v) = if sidewalk { (-(n as f32) * spacing * 0.5 - 1.2, 0.0) } else { (0.0, 0.0) };
        let q = f.at(u, v);
        g.part(Rust).cylinder(q, 0.3, 0.3, f.y, f.y + 0.85, 8, rgba(pal.rust));
        g.part(Glow).boxed(Vec3::new(q.x, f.y + 0.95, q.y), Vec3::new(0.32, 0.28, 0.32), 0.0, rgba([2.0, 0.9, 0.3]));
        if !sidewalk {
            for k in 0..3 {
                let a = k as f32 * TAU / 3.0 + 0.4;
                let w = q + Vec2::from_angle(a) * 1.3;
                g.part(Wood).boxed(Vec3::new(w.x, f.y + 0.07, w.y), Vec3::new(1.1, 0.14, 0.8), a, rgba(mul(pal.plywood, 0.65)));
            }
        }
    }
    g
}

/// An RV (camper) or an old van someone lives in, parked at `p` facing
/// `heading`: rust, a cracked windscreen covered in cardboard, curtains,
/// a blue tarp over the roof and a chair on the pavement side.
pub fn rv(p: Vec2, heading: f32, camper: bool, seed: u64, pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 0x2F7);
    let along = Vec2::from_angle(heading);
    let right = Vec2::new(along.y, -along.x);
    let f = Frame::new(p, along, right, 0.0);
    let yaw = f.yaw();
    let body = mix_dim(pick(&mut rng, &pal.car_colors), [0.75, 0.72, 0.66], if camper { 0.75 } else { 0.35 });
    let (len, w, h) = if camper { (7.0, 2.4, 2.9) } else { (5.2, 2.0, 2.2) };
    // Body and cab.
    let cab = if camper { 1.4 } else { 1.0 };
    g.part(Panel).boxed(f.v3(-cab * 0.5, 0.0, 0.35 + (h - 0.35) * 0.5), Vec3::new(len - cab, h - 0.35, w), yaw, rgba(body));
    g.part(Panel).boxed(f.v3(len * 0.5 - cab * 0.5, 0.0, 0.35 + (h * 0.62 - 0.35) * 0.5), Vec3::new(cab, h * 0.62 - 0.35, w * 0.98), yaw, rgba(mul(body, 0.95)));
    if camper {
        // The over-cab bunk.
        g.part(Panel).boxed(f.v3(len * 0.5 - cab * 0.55, 0.0, h - 0.4), Vec3::new(cab * 1.1, 0.8, w * 0.98), yaw, rgba(mul(body, 1.05)));
        // A stripe along the side.
        g.part(Matte).boxed(f.v3(-cab * 0.5, 0.0, h * 0.55), Vec3::new(len - cab - 0.1, 0.18, w + 0.02), yaw, rgba(pick(&mut rng, &pal.awning_colors)));
    }
    // Windscreen covered in cardboard, side windows with curtains.
    let fwd = len * 0.5 + 0.01;
    g.part(Wood).boxed(f.v3(fwd, 0.0, h * 0.5), Vec3::new(0.04, h * 0.25, w * 0.85), yaw, rgba(mul(pal.plywood, 0.9)));
    for k in 0..if camper { 3 } else { 1 } {
        let u = -len * 0.35 + k as f32 * 1.6;
        for s in [-1.0f32, 1.0] {
            g.part(Glass).boxed(f.v3(u, s * (w * 0.5 + 0.01), h * 0.62), Vec3::new(0.9, 0.55, 0.03), yaw, rgba(pal.glass_dark));
            g.part(Matte).boxed(f.v3(u - 0.2, s * (w * 0.5 + 0.02), h * 0.64), Vec3::new(0.4, 0.5, 0.03), yaw, rgba(pick(&mut rng, &pal.tarp_colors)));
        }
    }
    // A warm light inside, after dark it reads as somebody home.
    g.part(Glow).boxed(f.v3(-len * 0.35, w * 0.5 + 0.03, h * 0.62), Vec3::new(0.3, 0.2, 0.02), yaw, rgba([1.4, 0.8, 0.35]));
    // Rust patches and a mismatched door.
    for _ in 0..rng.random_range(2..5) {
        let u = rng.random_range(-len * 0.45..len * 0.4);
        let s = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        g.part(Rust).boxed(f.v3(u, s * (w * 0.5 + 0.015), rng.random_range(0.45..0.9)), Vec3::new(rng.random_range(0.3..0.9), rng.random_range(0.15..0.4), 0.02), yaw, rgba(pal.rust));
    }
    // A blue tarp over a leak.
    g.part(Tarp).boxed(f.v3(rng.random_range(-1.0..0.5), 0.0, h + 0.02), Vec3::new(rng.random_range(1.4..2.4), 0.04, w * 1.05), yaw + rng.random_range(-0.1..0.1), rgba([0.12, 0.3, 0.75]));
    // Wheels across the body.
    for u in [len * 0.32, -len * 0.32] {
        let c = f.at(u, 0.0);
        g.part(Matte).pipe(c, right, w + 0.1, 0.38, 0.38, 8, rgba(DARK));
    }
    // On the pavement side: a chair, a generator, crates.
    let side = -1.0;
    g.part(Tarp).boxed(f.v3(-0.5, side * (w * 0.5 + 1.0), 0.45), Vec3::new(0.5, 0.06, 0.5), yaw, rgba(pick(&mut rng, &pal.tarp_colors)));
    if rng.random_bool(0.6) {
        g.part(Metal).boxed(f.v3(1.2, side * (w * 0.5 + 0.8), 0.3), Vec3::new(0.6, 0.45, 0.4), yaw, rgba(mul([0.75, 0.55, 0.1], 0.9)));
    }
    let _ = PI;
    g
}

fn mix_dim(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Bins on the pavement at a building's front, overflowing by `level` (0
/// = full to the lid, 1 = as much again on the ground, 2+ a heap):
/// wheelie bins for homes, a dumpster for shops and industry; lids forced
/// up, bags heaped around and spilling towards the kerb.
pub fn trash_pile(p: Vec2, along: Vec2, out: Vec2, level: f32, dumpster: bool, seed: u64, _pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 0x7A54);
    let f = Frame::new(p, along, out, PAVE_Y);
    let yaw = f.yaw();
    if dumpster {
        let col = mul(pick(&mut rng, &[[0.12, 0.32, 0.18], [0.15, 0.2, 0.42], [0.35, 0.3, 0.12]]), 0.9);
        g.part(Metal).boxed(f.v3(0.0, -0.5, 0.65), Vec3::new(1.9, 1.2, 1.2), yaw, rgba(col));
        // The lid thrown back, bags showing over the rim.
        g.part(Matte).boxed(f.v3(0.0, -1.15, 1.35), Vec3::new(1.9, 0.9, 0.06), yaw, rgba(DARK));
        for k in 0..3 {
            g.part(Tarp).boxed(f.v3(-0.6 + k as f32 * 0.6, -0.5, 1.3), Vec3::new(0.55, 0.4, 0.5), yaw + k as f32, rgba([0.06, 0.06, 0.07]));
        }
    } else {
        for k in 0..3 {
            let col = [[0.2, 0.22, 0.24], [0.12, 0.3, 0.14], [0.14, 0.2, 0.4]][k % 3];
            let u = -0.9 + k as f32 * 0.75;
            g.part(Panel).boxed(f.v3(u, -0.55, 0.52), Vec3::new(0.6, 1.04, 0.7), yaw, rgba(col));
            // Lid propped up on the bags inside.
            g.part(Matte).boxed(f.v3(u, -0.68, 1.15), Vec3::new(0.62, 0.05, 0.6), yaw, rgba(mul(col, 0.8)));
            g.part(Tarp).boxed(f.v3(u, -0.5, 1.12), Vec3::new(0.45, 0.25, 0.45), yaw + k as f32, rgba([0.06, 0.06, 0.07]));
        }
    }
    // Bags on the ground, more the worse it is, spreading towards the kerb.
    let bags = ((1.0 + level * 6.0) as usize).min(16);
    for k in 0..bags {
        let spread = 1.0 + level.min(2.5) * 0.9;
        let u = rng.random_range(-spread..spread);
        let v = rng.random_range(-0.6..(0.2 + level.min(2.0) * 0.5));
        let tier = (k / 6) as f32;
        let col = if rng.random_bool(0.7) { [0.05, 0.05, 0.06] } else if rng.random_bool(0.5) { [0.1, 0.18, 0.45] } else { [0.55, 0.5, 0.42] };
        g.part(Tarp).boxed(f.v3(u, v, 0.2 + tier * 0.3), Vec3::new(rng.random_range(0.4..0.6), 0.4, rng.random_range(0.35..0.5)), yaw + rng.random_range(0.0..TAU), rgba(col));
    }
    // Loose litter: cups, boxes, a split bag.
    for _ in 0..((level * 5.0) as usize).min(10) {
        let u = rng.random_range(-2.2..2.2);
        let v = rng.random_range(-0.5..1.2);
        let s = rng.random_range(0.12..0.3);
        let col = pick(&mut rng, &[[0.8, 0.78, 0.7], [0.6, 0.45, 0.3], [0.7, 0.15, 0.12], [0.2, 0.5, 0.7]]);
        g.part(Matte).boxed(f.v3(u, v, s * 0.3), Vec3::new(s, s * 0.6, s * 0.8), rng.random_range(0.0..TAU), rgba(col));
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(g: &Geometry) -> (Vec3, Vec3, usize) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        let mut n = 0;
        for (_, m) in g.iter() {
            for p in &m.positions {
                let v = Vec3::from(*p);
                assert!(v.is_finite());
                lo = lo.min(v);
                hi = hi.max(v);
                n += 1;
            }
        }
        (lo, hi, n)
    }

    #[test]
    fn camps_rvs_and_trash_stay_near_their_spot_and_grow_with_size() {
        let pal = Palette::default();
        let p = Vec2::new(100.0, -40.0);
        let mut last = 0;
        for tents in [1u8, 3, 6, 10] {
            for sidewalk in [true, false] {
                let g = camp(p, Vec2::X, Vec2::Y, tents, sidewalk, 9, &pal);
                let (lo, hi, n) = bounds(&g);
                assert!(n > 0);
                assert!(lo.y >= -0.01 && hi.y < 3.0, "a camp stays low: {lo} {hi}");
                assert!(Vec2::new(lo.x, lo.z).distance(p) < 16.0 && Vec2::new(hi.x, hi.z).distance(p) < 16.0, "{tents} tents spill: {lo} {hi}");
                if sidewalk {
                    assert!(n >= last, "more tents, more camp");
                    last = n;
                    // A pavement camp is a file along the kerb: deep at most a few metres.
                    assert!(hi.z - lo.z < 5.0, "a pavement camp keeps the street clear");
                }
            }
        }
        for camper in [true, false] {
            let g = rv(p, 0.3, camper, 4, &pal);
            let (lo, hi, n) = bounds(&g);
            assert!(n > 0 && hi.y < 3.6 && lo.y >= -0.01);
            assert!(Vec2::new(hi.x, hi.z).distance(p) < 7.0);
        }
        let small = bounds(&trash_pile(p, Vec2::X, Vec2::Y, 0.1, false, 3, &pal)).2;
        let big = bounds(&trash_pile(p, Vec2::X, Vec2::Y, 2.0, true, 3, &pal)).2;
        assert!(big > small, "a worse overflow is a bigger heap");
    }
}
