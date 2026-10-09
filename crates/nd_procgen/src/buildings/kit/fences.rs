//! Fences.

//! Every fence, hedge and hoarding: chain-link with posts and rails (and
//! barbed wire), post-and-rail, the vacant lot's broken chain-link, the
//! building site's plywood hoarding, hedges, and gates. Posts stand in
//! [`bays`] of about the same width along each run.

use super::super::parts::*;
use super::super::{Ctx, Face};
use crate::color::{mul, rgba};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// How many bays a run of `len` metres is split into, posts about
/// `spacing` apart: at least one.
pub(crate) fn bays(len: f32, spacing: f32) -> i32 {
    (len / spacing).ceil().max(1.0) as i32
}

/// Post-and-rail fence from `a` to `b` (lot-local).
pub(crate) fn rail_fence(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let n = bays(len, 3.0);
    for k in 0..=n {
        let p = a.lerp(b, k as f32 / n as f32);
        f.bx(g, Wood, p.x, h * 0.5, p.y, Vec3::new(0.14, h, 0.14), color);
    }
    let yaw = (b - a).y.atan2((b - a).x);
    let m = (a + b) * 0.5;
    for y in [h * 0.45, h * 0.85] {
        f.bx_yaw(g, Wood, m.x, y, m.y, Vec3::new(len, 0.1, 0.06), color, yaw);
    }
}


/// Rail fence between two lot-local points, optionally topped with barbed wire.
pub fn fence(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, barbed: bool) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let face = Face::from_edge(a, b);
    let posts = bays(len, 2.6);
    let color = mul(c.palette.steel, 0.5);
    for i in 0..=posts {
        let t = -len * 0.5 + i as f32 * (len / posts as f32);
        face.bx(g, f, Matte, t, h * 0.5, 0.0, Vec3::new(0.08, h, 0.08), color);
    }
    for k in 0..3 {
        let y = h * (0.25 + 0.35 * k as f32);
        face.bx(g, f, Matte, 0.0, y, 0.0, Vec3::new(len, 0.04, 0.04), color);
    }
    // mesh panel: thin, dark, slightly translucent-looking grey; a box, not
    // a decal, because a fence is seen from both sides
    face.bx(g, f, Matte, 0.0, h * 0.5, 0.0, Vec3::new(len, h * 0.96, 0.015), c.palette.fence_mesh);
    if barbed {
        let wa = f.at2(a, h + 0.2);
        let wb = f.at2(b, h + 0.2);
        g.part(Rust).tube(wa, wb, 0.12, 5, rgba(c.palette.rust));
    }
}

/// Fence round a yard with the gate on the face that looks at the road:
/// the perimeter every depot, plant and compound shares.
pub fn fence_lot(g: &mut Geometry, c: &Ctx, poly: &[Vec2], h: f32, barbed: bool) {
    fence_poly(g, c, poly, h, c.front_face_index(poly), barbed);
}

/// Fence around a polygon, leaving one face open as a gate.
pub fn fence_poly(g: &mut Geometry, c: &Ctx, poly: &[Vec2], h: f32, skip_face: Option<usize>, barbed: bool) {
    let n = poly.len();
    for i in 0..n {
        if skip_face == Some(i) {
            gate(g, c, poly[i], poly[(i + 1) % n], h);
            continue;
        }
        fence(g, c, poly[i], poly[(i + 1) % n], h, barbed);
    }
}

/// Gate posts at both ends of a lot-local edge and a sliding gate half open.
pub fn gate(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32) {
    let face = Face::from_edge(a, b);
    let f = &c.frame;
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, side * face.len * 0.5, h * 0.6, 0.0, Vec3::new(0.2, h * 1.2, 0.2), mul(c.palette.steel, 0.6));
    }
    face.bx(g, f, Matte, -face.len * 0.2, h * 0.5, 0.15, Vec3::new(face.len * 0.55, h * 0.9, 0.05), mul(c.palette.steel, 0.5));
}

pub fn hedge(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, w: f32, color: [f32; 3]) {
    let face = Face::from_edge(a, b);
    face.bx(g, &c.frame, Foliage, 0.0, h * 0.5, 0.0, Vec3::new(face.len, h, w), color);
}

/// Height of a building site's hoarding.
pub(crate) const HOARDING_H: f32 = 2.3;

/// Plywood hoarding along one edge, with a gate gap of `gate` metres in the
/// middle when given.
pub(crate) fn hoarding(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, gate: Option<f32>) {
    let f = &c.frame;
    if face.len < 0.6 {
        return;
    }
    let ply = mul(c.palette.plywood, rng.random_range(0.85..1.05));
    let post = mul(c.palette.steel, 0.55);
    let h = HOARDING_H;
    let spans: Vec<(f32, f32)> = match gate {
        Some(gw) if face.len > gw + 2.0 => vec![(-face.len * 0.5, -gw * 0.5), (gw * 0.5, face.len * 0.5)],
        _ => vec![(-face.len * 0.5, face.len * 0.5)],
    };
    for (a, b) in &spans {
        let len = b - a;
        let mid = (a + b) * 0.5;
        face.bx(g, f, Wood, mid, h * 0.5, 0.0, Vec3::new(len, h, 0.06), ply);
        // Rail along the top and posts behind.
        face.bx(g, f, Matte, mid, h + 0.03, -0.05, Vec3::new(len, 0.08, 0.14), post);
        let posts = bays(len, 2.4);
        for k in 0..=posts {
            let t = a + k as f32 * (len / posts as f32);
            face.bx(g, f, Matte, t, h * 0.5, -0.08, Vec3::new(0.1, h, 0.1), post);
        }
        // Posters and a warning stripe.
        if rng.random_bool(0.5) && len > 4.0 {
            let t = rng.random_range(a + 1.0..b - 1.0);
            face.decal(g, f, Glow, t, 1.3, 0.04, 1.2, 1.6, mul(pick(rng, &c.palette.sign_colors), 0.6));
        }
        face.decal(g, f, Matte, mid, 0.45, 0.035, len, 0.25, c.palette.hi_vis);
    }
    if let Some(gw) = gate {
        if face.len > gw + 2.0 {
            // Gate posts and a sliding mesh gate, part open.
            for side in [-1.0f32, 1.0] {
                face.bx(g, f, Matte, side * gw * 0.5, (h + 0.3) * 0.5, 0.0, Vec3::new(0.22, h + 0.3, 0.22), post);
            }
            face.bx(g, f, Matte, -gw * 0.5 - gw * 0.3, h * 0.5, -0.25, Vec3::new(gw * 0.6, h * 0.92, 0.04), [0.30, 0.31, 0.33]);
            face.bx(g, f, Matte, gw * 0.5 - gw * 0.15, h * 0.5, 0.25, Vec3::new(gw * 0.3, h * 0.92, 0.04), [0.30, 0.31, 0.33]);
            // A barrier across the opening.
            face.bx(g, f, Matte, 0.0, 0.9, 0.4, Vec3::new(gw * 0.8, 0.12, 0.08), c.palette.hi_vis);
        }
    }
}

// ── in world coordinates (vacant lots) ──

fn world_bx(g: &mut Geometry, s: crate::surface::Surface, p: Vec2, y: f32, size: Vec3, yaw: f32, col: [f32; 3]) {
    g.part(s).boxed(Vec3::new(p.x, y, p.y), size, yaw, rgba(col));
}

/// Chain-link fence from `a` to `b`. `broken` (0..1) removes posts and
/// panels; a missing panel sometimes lies on the ground just inside.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fence_run(g: &mut Geometry, rng: &mut impl Rng, a: Vec2, b: Vec2, inward: Vec2, h: f32, broken: f32, post: [f32; 3], mesh: [f32; 3]) {
    let len = a.distance(b);
    if len < 1.0 {
        return;
    }
    let t = (b - a) / len;
    let yaw = t.y.atan2(t.x);
    let bays = bays(len, 2.6);
    let bay = len / bays as f32;
    for i in 0..=bays {
        if coin(rng, broken * 0.25) {
            continue;
        }
        let p = a + t * (i as f32 * bay);
        world_bx(g, Matte, p, h * 0.5, Vec3::new(0.08, h, 0.08), yaw, post);
    }
    for i in 0..bays {
        let m = a + t * ((i as f32 + 0.5) * bay);
        if coin(rng, broken * 0.5) {
            if rng.random_bool(0.5) {
                let q = m + inward * rng.random_range(0.8..1.6);
                world_bx(g, Matte, q, 0.03, Vec3::new(bay * 0.9, 0.03, h * 0.9), yaw + rng.random_range(-0.4..0.4), mesh);
            }
            continue;
        }
        world_bx(g, Matte, m, h * 0.3, Vec3::new(bay, 0.04, 0.04), yaw, post);
        world_bx(g, Matte, m, h * 0.92, Vec3::new(bay, 0.04, 0.04), yaw, post);
        world_bx(g, Matte, m, h * 0.5, Vec3::new(bay, h * 0.9, 0.015), yaw, mesh);
    }
}

/// Clipped hedge from `a` to `b`, `w` thick, standing on the inward side.
pub(crate) fn hedge_run(g: &mut Geometry, a: Vec2, b: Vec2, inward: Vec2, h: f32, w: f32, col: [f32; 3]) {
    let len = a.distance(b);
    if len < 0.8 {
        return;
    }
    let t = (b - a) / len;
    let m = (a + b) * 0.5 + inward * (w * 0.5 + 0.1);
    world_bx(g, Foliage, m, h * 0.5, Vec3::new(len, h, w), t.y.atan2(t.x), col);
}
