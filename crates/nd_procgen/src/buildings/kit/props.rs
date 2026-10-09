//! Props more than one style family uses: yard and forecourt furniture,
//! signs, sculptures and murals, livestock and the mine's dump truck.

use super::super::parts::*;
use super::super::{Ctx, Face, Frame};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

/// A stack of tyres.
pub(crate) fn tyres(g: &mut Geometry, f: &Frame, p: Vec2, n: i32) {
    let w = f.to_world(p.x, p.y);
    for k in 0..n {
        g.part(Matte).cylinder(w, 0.38, 0.38, k as f32 * 0.24, k as f32 * 0.24 + 0.22, 8, rgba(DARK));
    }
}

/// A pole with a flag (or a feather banner) on it.
pub(crate) fn flagpole(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, color: [f32; 3], banner: bool) {
    f.bx(g, Matte, p.x, h * 0.5, p.y, Vec3::new(0.12, h, 0.12), [0.75, 0.76, 0.78]);
    if banner {
        f.bx(g, Tarp, p.x + 0.35, h * 0.62, p.y, Vec3::new(0.6, h * 0.6, 0.04), color);
    } else {
        f.bx(g, Tarp, p.x + 0.8, h - 0.6, p.y, Vec3::new(1.5, 0.95, 0.04), color);
    }
}

/// A sagging line of little pennants between two lot-local (u, y, v) points.
pub(crate) fn bunting(g: &mut Geometry, f: &Frame, a: Vec3, b: Vec3, n: i32, colors: &[[f32; 3]]) {
    let wa = f.at(a.x, a.y, a.z);
    let wb = f.at(b.x, b.y, b.z);
    g.part(Matte).wire(wa, wb, 0.4, 0.025, rgba(DARK));
    let yaw = (wb.z - wa.z).atan2(wb.x - wa.x);
    for i in 1..n {
        let t = i as f32 / n as f32;
        let p = wa.lerp(wb, t) - Vec3::Y * (0.4 * 4.0 * t * (1.0 - t) + 0.25);
        let col = crate::palette::at(colors, i as usize);
        g.part(Tarp).boxed(p, Vec3::new(0.35, 0.4, 0.02), yaw, rgba(col));
    }
}

/// A floodlight mast whose lamp bank faces `facing` (lot-local, unit).
pub(crate) fn mast_light(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32, facing: Vec2) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.4 {
        return;
    }
    let yaw = facing.y.atan2(facing.x) + FRAC_PI_2;
    f.bx(g, Matte, p.x, h * 0.5, p.y, Vec3::new(0.35, h, 0.35), mul(c.palette.steel, 0.6));
    f.bx_yaw(g, Matte, p.x, h + 0.4, p.y, Vec3::new(2.4, 0.9, 0.35), CHARCOAL, yaw);
    let q = p + facing * 0.2;
    f.bx_yaw(g, Glow, q.x, h + 0.4, q.y, Vec3::new(2.2, 0.75, 0.04), mul(c.palette.street_lamp, 1.5), yaw);
}

/// A fish in glowing outline-blocks on a face: body, tail and an eye.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fish_sign(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, d: f32, size: f32, color: [f32; 3]) {
    let f = &c.frame;
    let k = 1.0 - 0.5 * c.wear;
    face.bx(g, f, Matte, t, y, d - 0.08, Vec3::new(size * 1.5, size * 0.75, 0.1), DARK);
    face.decal(g, f, Glow, t, y, d, size * 0.8, size * 0.42, mul(color, k));
    face.decal(g, f, Glow, t - size * 0.05, y, d, size * 0.55, size * 0.56, mul(color, k));
    face.decal(g, f, Glow, t + size * 0.42, y, d, size * 0.16, size * 0.24, mul(color, k));
    // Tail: two fins splayed.
    face.decal(g, f, Glow, t - size * 0.55, y + size * 0.13, d, size * 0.18, size * 0.16, mul(color, k));
    face.decal(g, f, Glow, t - size * 0.55, y - size * 0.13, d, size * 0.18, size * 0.16, mul(color, k));
    face.decal(g, f, Matte, t + size * 0.3, y + size * 0.06, d + 0.01, size * 0.07, size * 0.07, DARK);
}

/// Crates of fish on ice, stacked.
pub(crate) fn fish_crates(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, n: i32) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.0 {
        return;
    }
    let crate_col = [[0.2, 0.42, 0.75], [0.85, 0.5, 0.15], [0.85, 0.85, 0.82]][rng.random_range(0..3usize)];
    let yaw = rng.random_range(-0.3..0.3);
    for k in 0..n {
        f.bx_yaw(g, Matte, p.x, 0.22 + k as f32 * 0.44, p.y, Vec3::new(0.9, 0.42, 0.6), mul(crate_col, 0.9 + 0.1 * (k % 2) as f32), yaw);
    }
    // Ice and silver fish in the top crate.
    let top = n as f32 * 0.44;
    f.bx_yaw(g, Matte, p.x, top + 0.01, p.y, Vec3::new(0.8, 0.04, 0.5), [0.85, 0.92, 0.95], yaw);
    for i in 0..3 {
        f.bx_yaw(g, Metal, p.x - 0.25 + i as f32 * 0.25, top + 0.05, p.y, Vec3::new(0.12, 0.06, 0.42), [0.72, 0.75, 0.78], yaw);
    }
}

/// A café table with two chairs, and an umbrella when `umbrella`.
pub(crate) fn table(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, wood: [f32; 3], umbrella: Option<[f32; 3]>) {
    f.bx(g, Wood, p.x, y + 0.72, p.y, Vec3::new(0.8, 0.06, 0.8), wood);
    f.bx(g, Matte, p.x, y + 0.36, p.y, Vec3::new(0.08, 0.72, 0.08), CHARCOAL);
    for s in [-0.7f32, 0.7] {
        f.bx(g, Wood, p.x + s, y + 0.45, p.y, Vec3::new(0.42, 0.06, 0.42), mul(wood, 0.85));
        f.bx(g, Wood, p.x + s * 1.2, y + 0.7, p.y, Vec3::new(0.05, 0.5, 0.42), mul(wood, 0.8));
    }
    if let Some(col) = umbrella {
        f.bx(g, Matte, p.x, y + 1.3, p.y, Vec3::new(0.05, 1.2, 0.05), [0.8, 0.8, 0.78]);
        let w = f.to_world(p.x, p.y);
        g.part(Tarp).cylinder(w, 1.25, 0.1, y + 1.95, y + 2.35, 8, rgba(col));
    }
}

/// A sculpture: a few twisted blocks, a ring, or a bronze figure-ish stack.
pub(crate) fn sculpture(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, scale: f32) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < scale * 0.8 {
        return;
    }
    f.bx(g, Concrete, p.x, 0.3, p.y, Vec3::new(1.4 * scale, 0.6, 1.4 * scale), mul(c.palette.concrete(1), 1.1));
    match rng.random_range(0..3) {
        0 => {
            // Stacked, turned blocks in bright paint.
            let mut y = 0.6;
            for k in 0..rng.random_range(3..6) {
                let h = rng.random_range(0.5..1.1) * scale;
                let col = MURAL[rng.random_range(0..MURAL.len())];
                f.bx_yaw(g, Matte, p.x, y + h * 0.5, p.y, Vec3::new(0.9 * scale, h, 0.6 * scale), col, k as f32 * 0.5 + rng.random_range(0.0..0.3));
                y += h;
            }
        }
        1 => {
            // A ring stood on edge.
            let col = if coin(rng, 0.5) { c.palette.bronze } else { [0.85, 0.15, 0.1] };
            let r = 1.3 * scale;
            let a0 = rng.random_range(0.0..TAU);
            let axis = Vec2::new(a0.cos(), a0.sin());
            let n = 10;
            for i in 0..n {
                let (t0, t1) = (i as f32 / n as f32 * TAU, (i + 1) as f32 / n as f32 * TAU);
                let pa = p + axis * (r * t0.cos());
                let pb = p + axis * (r * t1.cos());
                let a = f.at(pa.x, 0.6 + r + r * t0.sin(), pa.y);
                let b = f.at(pb.x, 0.6 + r + r * t1.sin(), pb.y);
                g.part(Metal).beam(a, b, 0.22 * scale, rgba(col));
            }
        }
        _ => {
            // Bronze: tapered drum, a body and a head.
            let w = f.to_world(p.x, p.y);
            let b = c.palette.bronze;
            g.part(Metal).cylinder(w, 0.45 * scale, 0.3 * scale, 0.6, 0.6 + 1.6 * scale, 8, rgba(b));
            g.part(Metal).cylinder(w, 0.3 * scale, 0.05, 0.6 + 1.6 * scale, 0.6 + 2.3 * scale, 8, rgba(mul(b, 0.9)));
        }
    }
}

/// Bright paints for murals, sculptures and banners.
pub(crate) const MURAL: [[f32; 3]; 8] = [[0.95, 0.25, 0.35], [0.98, 0.72, 0.1], [0.1, 0.65, 0.85], [0.35, 0.8, 0.35], [0.6, 0.3, 0.85], [0.98, 0.45, 0.1], [0.1, 0.8, 0.7], [0.95, 0.9, 0.85]];

/// A mural on a face between two heights: a ground colour and blocks,
/// bands and dots of bright paint over it.
pub(crate) fn mural(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32) {
    let f = &c.frame;
    if face.len < 3.0 || y1 - y0 < 1.5 {
        return;
    }
    let fade = 1.0 - 0.45 * c.wear;
    let w = face.len * rng.random_range(0.6..0.9);
    let h = y1 - y0;
    let t0 = rng.random_range(-(face.len - w) * 0.5..=(face.len - w) * 0.5);
    let base = mul(MURAL[rng.random_range(0..MURAL.len())], 0.8 * fade);
    // Painted on the wall, under the window frames (0.03) and lights (0.05)
    // that sit over it, so nothing shares a plane.
    face.decal(g, f, Matte, t0, y0 + h * 0.5, 0.008, w, h, base);
    for i in 0..rng.random_range(5..10) {
        let col = mul(MURAL[rng.random_range(0..MURAL.len())], fade);
        let (bw, bh) = if i % 3 == 0 { (w * rng.random_range(0.5..0.9), h * rng.random_range(0.06..0.15)) } else { (w * rng.random_range(0.08..0.3), h * rng.random_range(0.15..0.6)) };
        let t = t0 + rng.random_range(-0.5..0.5) * (w - bw);
        let y = y0 + bh * 0.5 + rng.random::<f32>() * (h - bh);
        face.decal(g, f, Matte, t, y, 0.011 + i as f32 * 0.0015, bw, bh, col);
    }
}


/// A cow as a few boxes: body, head, two leg slabs and a patch on the coat.
pub(crate) fn cow(g: &mut Geometry, f: &Frame, p: Vec2, yaw: f32, coat: [f32; 3], patch: Option<[f32; 3]>) {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    f.bx_yaw(g, Matte, p.x, 1.05, p.y, Vec3::new(1.9, 0.75, 0.8), coat, yaw);
    let head = p + d * 1.15;
    f.bx_yaw(g, Matte, head.x, 1.25, head.y, Vec3::new(0.5, 0.45, 0.4), mul(coat, 0.9), yaw);
    for s in [-0.65f32, 0.65] {
        let q = p + d * s;
        f.bx_yaw(g, Matte, q.x, 0.35, q.y, Vec3::new(0.2, 0.7, 0.66), mul(coat, 0.8), yaw);
    }
    if let Some(pc) = patch {
        let q = p - d * 0.2;
        f.bx_yaw(g, Matte, q.x, 1.08, q.y, Vec3::new(0.6, 0.78, 0.84), pc, yaw);
    }
}

/// A cow's coat: Holstein, brown or black.
/// A big dump truck for the mine: wheels, chassis, tipper and cab.
pub(crate) fn haul_truck(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, yaw: f32, paint: [f32; 3]) {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    for (a, s) in [(2.0f32, 1.3f32), (2.0, -1.3), (-1.8, 1.3), (-1.8, -1.3)] {
        let q = p + d * a + n * s;
        f.bx_yaw(g, Matte, q.x, y + 0.85, q.y, Vec3::new(1.7, 1.7, 0.7), DARK, yaw);
    }
    f.bx_yaw(g, Matte, p.x, y + 1.6, p.y, Vec3::new(6.0, 0.6, 2.0), CHARCOAL, yaw);
    let bed = p - d * 0.6;
    f.bx_yaw(g, Metal, bed.x, y + 2.6, bed.y, Vec3::new(4.6, 1.6, 3.2), paint, yaw);
    let cab = p + d * 2.4 + n * 0.7;
    f.bx_yaw(g, Metal, cab.x, y + 2.7, cab.y, Vec3::new(1.2, 1.6, 1.4), mul(paint, 0.9), yaw);
    f.bx_yaw(g, Glass, cab.x + d.x * 0.62, y + 3.0, cab.y + d.y * 0.62, Vec3::new(0.04, 0.7, 1.1), c_glass(), yaw);
}

pub(crate) fn c_glass() -> [f32; 3] {
    [0.1, 0.13, 0.16]
}

/// A cone heap.
pub(crate) fn heap(g: &mut Geometry, c: &Ctx, p: Vec2, r: f32, h: f32, color: [f32; 3]) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < r * 0.8 {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Dirt).cylinder(w, r, r * 0.15, 0.0, h, 9, rgba(color));
}

