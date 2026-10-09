//! Yard props: scrap piles, market stalls, drum fires, string lights,
//! stairs and ladders, pools, barriers, floodlights and gantry cranes.

#[allow(unused_imports)]
use super::*;

/// Heap of scrap: tyres, sheet, drums and a car on top sometimes.
pub fn scrap_pile(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32) {
    let f = &c.frame;
    let n = (r * r * 1.2) as i32 + 4;
    for _ in 0..n {
        let d = rng.random_range(0.0..r);
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        let q = p + Vec2::new(a.cos(), a.sin()) * d;
        let size = Vec3::new(rng.random_range(0.5..2.2), rng.random_range(0.3..1.2), rng.random_range(0.5..2.0));
        let y = (1.0 - d / r) * h * rng.random_range(0.3..1.0);
        let (s, col) = match rng.random_range(0..4) {
            0 => (Rust, c.palette.rust),
            1 => (Metal, pick(rng, &c.palette.container_colors)),
            2 => (Tarp, pick(rng, &c.palette.tarp_colors)),
            _ => (Matte, DARK),
        };
        // Pieces near the edge of the heap can stick out past the fence.
        if !c.inside_local(q) || distance_to_boundary(q, &c.lot_poly) < size.x.max(size.z) * 0.75 {
            continue;
        }
        f.bx_yaw(g, s, q.x, y + size.y * 0.5, q.y, size, mul(col, 0.8), rng.random_range(0.0..3.0));
    }
    // tyre stack
    let tp = p + Vec2::new(r * 0.6, 0.0);
    if !c.inside_local(tp) || distance_to_boundary(tp, &c.lot_poly) < 0.6 {
        return;
    }
    let w = f.to_world(tp.x, tp.y);
    for k in 0..rng.random_range(2..6) {
        g.part(Matte).cylinder(w, 0.45, 0.45, k as f32 * 0.25, k as f32 * 0.25 + 0.22, 10, rgba(DARK));
    }
}

/// Market stall against a face: tarp roof, counter, a sign strip.
pub fn stall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, d: f32) {
    let f = &c.frame;
    // The tarp roof is 4.2 by 2.2: the whole stall has to fit on the lot.
    if [(-2.1f32, -1.1f32), (2.1, -1.1), (2.1, 1.1), (-2.1, 1.1)].iter().any(|(dt, dd)| !c.inside_local(face.at(t + dt, d + dd))) {
        return;
    }
    let tc = mul(pick(rng, &c.palette.tarp_colors), 0.9);
    face.bx(g, f, Tarp, t, 2.4, d, Vec3::new(4.2, 0.1, 2.2), tc);
    face.bx(g, f, Wood, t, 0.5, d, Vec3::new(3.6, 1.0, 1.2), c.palette.wood);
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, t + side * 2.0, 1.2, d + 1.0, Vec3::new(0.08, 2.4, 0.08), CHARCOAL);
        face.bx(g, f, Matte, t + side * 2.0, 1.2, d - 1.0, Vec3::new(0.08, 2.4, 0.08), CHARCOAL);
    }
    let sc = pick(rng, &c.palette.sign_colors);
    face.bx(g, f, Glow, t, 2.1, d + 1.1, Vec3::new(3.0, 0.3, 0.06), mul(sc, 0.9 - 0.5 * c.wear));
    // goods: two crates of produce on the counter
    for i in 0..2 {
        face.bx(g, f, Tarp, t - 0.9 + i as f32 * 1.8, 1.2, d, Vec3::new(1.4, 0.4, 0.6), pick(rng, &c.palette.awning_colors));
    }
}

pub fn drum_fire(g: &mut Geometry, f: &Frame, pal: &Palette, p: Vec2) {
    let w = f.to_world(p.x, p.y);
    g.part(Rust).cylinder(w, 0.3, 0.3, 0.0, 0.9, 10, rgba(pal.rust));
    g.part(Glow).boxed(Vec3::new(w.x, 1.0, w.y), Vec3::new(0.3, 0.25, 0.3), 0.0, rgba([2.0, 0.9, 0.3]));
}

/// Sagging string of bulbs between two lot-local (u, y, v) anchors.
pub fn string_lights(g: &mut Geometry, f: &Frame, a: Vec3, b: Vec3, n: i32, color: [f32; 3]) {
    let wa = f.at(a.x, a.y, a.z);
    let wb = f.at(b.x, b.y, b.z);
    g.part(Matte).wire(wa, wb, 0.5, 0.03, rgba(DARK));
    for i in 1..n {
        let t = i as f32 / n as f32;
        let p = wa.lerp(wb, t) - Vec3::Y * (0.5 * 4.0 * t * (1.0 - t) + 0.15);
        g.part(Glow).boxed(p, Vec3::splat(0.16), 0.0, rgba(color));
    }
}

/// Outdoor steelwork rusts once a building is run down: the surface and
/// colour for stairs, ladders and fire escapes.
pub fn outdoor_steel(c: &Ctx, color: [f32; 3]) -> (Surface, [f32; 3]) {
    // Cheap buildings let it go sooner.
    let threshold = if c.tier == nd_sim::Tier::Low { 0.2 } else { 0.4 };
    if c.wear > threshold { (Rust, mix(color, c.palette.rust, 0.55)) } else { (Metal, color) }
}

/// External zigzag stair up the side of a pile. Rusted when the lot is worn.
#[allow(clippy::too_many_arguments)]
pub fn stair_zigzag(g: &mut Geometry, c: &Ctx, u: f32, v: f32, levels: i32, rise: f32, run: f32, color: [f32; 3]) {
    let f = &c.frame;
    let (s, color) = outdoor_steel(c, color);
    let rail = if s == Rust { Rust } else { Matte };
    let mut y = 0.0;
    for l in 0..levels {
        let steps = 8;
        let dir = if l % 2 == 0 { 1.0 } else { -1.0 };
        for st in 0..steps {
            let t = (st as f32 + 0.5) / steps as f32;
            f.bx(g, s, u, y + t * rise, v + dir * (t * run - run * 0.5), Vec3::new(0.9, 0.16, 0.42), color);
        }
        f.bx(g, rail, u - 0.5, y + rise * 0.5 + 0.5, v, Vec3::new(0.06, rise + 1.0, run), color);
        f.bx(g, s, u, y + rise + 0.08, v + dir * run * 0.5, Vec3::new(1.0, 0.16, 1.0), color);
        y += rise;
    }
    f.bx(g, rail, u, y * 0.5, v + run * 0.55, Vec3::new(0.15, y, 0.15), color);
    f.bx(g, rail, u, y * 0.5, v - run * 0.55, Vec3::new(0.15, y, 0.15), color);
}

pub fn ladder(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y0: f32, y1: f32, color: [f32; 3]) {
    let f = &c.frame;
    let (s, color) = outdoor_steel(c, color);
    let s = if s == Rust { Rust } else { Matte };
    let h = y1 - y0;
    for side in [-0.25f32, 0.25] {
        face.bx(g, f, s, t + side, y0 + h * 0.5, 0.2, Vec3::new(0.05, h, 0.05), color);
    }
    let rungs = (h / 0.3) as i32;
    for k in 0..rungs {
        face.bx(g, f, s, t, y0 + (k as f32 + 0.5) * 0.3, 0.2, Vec3::new(0.5, 0.04, 0.04), color);
    }
}

pub fn pool(g: &mut Geometry, c: &Ctx, cu: f32, cv: f32, w: f32, d: f32) {
    let f = &c.frame;
    f.bx(g, Paving, cu, 0.12, cv, Vec3::new(w + 1.6, 0.24, d + 1.6), mul(c.palette.paving, 1.2));
    f.bx(g, Glow, cu, 0.2, cv, Vec3::new(w, 0.1, d), mul(c.palette.water, 0.7));
}

/// Concrete jersey barrier in front of a face.
pub fn barrier(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, d: f32, w: f32) {
    let f = &c.frame;
    let d = d.min(c.reach_out(face) - 0.6);
    if !c.inside_local(face.at(t, d)) {
        return;
    }
    let col = mul(c.palette.concrete(0), 0.9);
    face.bx(g, f, Concrete, t, 0.25, d, Vec3::new(w, 0.5, 0.7), col);
    face.bx(g, f, Concrete, t, 0.7, d, Vec3::new(w, 0.4, 0.35), col);
}

/// Floodlight mast, skipped when its head would hang over the lot edge.
pub fn floodlight(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 0.9 {
        return;
    }
    f.bx(g, Matte, p.x, h * 0.5, p.y, Vec3::new(0.3, h, 0.3), mul(c.palette.steel, 0.6));
    f.bx(g, Matte, p.x, h + 0.2, p.y, Vec3::new(1.6, 0.4, 0.5), CHARCOAL);
    f.bx(g, Glow, p.x, h + 0.2, p.y - 0.28, Vec3::new(1.4, 0.3, 0.06), mul(c.palette.street_lamp, 1.3));
}

/// Gantry crane spanning across the lot at `v`, running along u.
pub fn gantry_crane(g: &mut Geometry, c: &Ctx, u: f32, v0: f32, v1: f32, h: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.metal_paint(0), 1.1);
    for v in [v0, v1] {
        for du in [-1.6f32, 1.6] {
            f.bx(g, Matte, u + du, h * 0.5, v, Vec3::new(0.4, h, 0.4), steel);
        }
        f.bx(g, Matte, u, h, v, Vec3::new(4.0, 0.5, 0.5), steel);
    }
    let span = v1 - v0;
    f.bx(g, Metal, u, h + 0.5, (v0 + v1) * 0.5, Vec3::new(1.2, 1.0, span + 2.0), steel);
    f.bx(g, Matte, u, h - 0.3, (v0 + v1) * 0.4, Vec3::new(1.6, 0.6, 2.0), CHARCOAL);
    f.bx(g, Matte, u, h * 0.6, (v0 + v1) * 0.4, Vec3::new(0.05, h * 0.5, 0.05), DARK);
    aviation_light_local(g, f, c.palette, u, h + 1.1, v0, 0.3);
}
