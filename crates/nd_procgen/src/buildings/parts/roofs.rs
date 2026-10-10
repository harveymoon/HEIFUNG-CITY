//! Roof gear: parapets of plant, water tanks, vents, dishes, solar arrays
//! and antennas.

#[allow(unused_imports)]
use super::*;

// ── roofs ────────────────────────────────────────────────────────────────────

/// Scatter rooftop clutter inside a footprint at height `y`.
pub fn roof_gear(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32, density: f32) {
    let area = polygon_area(poly).abs();
    let inner = offset_poly(poly, 1.3);
    if inner.len() < 3 {
        return;
    }
    let (lo, hi) = bounds(&inner);
    let n = ((area / 60.0) * density).round().clamp(0.0, 9.0) as i32;
    let f = &c.frame;
    let mut placed = 0;
    for _ in 0..n * 6 {
        if placed >= n {
            break;
        }
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        if !point_in_polygon(p, &inner) {
            continue;
        }
        placed += 1;
        match rng.random_range(0..9) {
            0 | 1 => {
                let s = rng.random_range(0.8..1.6);
                f.bx(g, Metal, p.x, y + s * 0.45, p.y, Vec3::new(s * 1.3, s * 0.9, s), mul(c.palette.steel, 0.85));
                f.bx(g, Matte, p.x, y + s * 0.92, p.y, Vec3::new(s * 1.1, 0.04, s * 0.8), DARK);
            }
            2 => {
                let (s, col) = if rng.random_bool(0.5) { (Rust, c.palette.rust) } else { (Metal, mul(c.palette.steel, 0.8)) };
                water_tank(g, f, p, y, rng.random_range(0.9..1.4), rng.random_range(1.6..2.6), s, col)
            }
            3 => vent_stack(g, f, p, y, rng.random_range(0.25..0.45), rng.random_range(1.2..3.0), mul(c.palette.steel, 0.7)),
            4 => dish(g, f, p, y, rng.random_range(0.5..1.1), c.palette.steel),
            5 => antenna_mast(g, c, p, y, rng.random_range(4.0..10.0)),
            6 => solar_array(g, f, p, y, rng.random_range(2.0..4.0), rng.random_range(1.5..3.0), rng.random_range(0.0..1.5), c.palette.glass_dark),
            _ => {
                // stair bulkhead
                f.bx(g, Concrete, p.x, y + 1.3, p.y, Vec3::new(3.0, 2.6, 2.4), mul(pick(rng, &c.palette.concrete), 0.9));
                f.bx(g, Matte, p.x, y + 1.2, p.y - 1.25, Vec3::new(1.0, 2.1, 0.08), DARK);
            }
        }
    }
}

/// Rooftop tank on four legs. `s` is the drum's surface (rusted or painted
/// steel), passed explicitly rather than guessed from the colour.
#[allow(clippy::too_many_arguments)]
pub fn water_tank(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, h: f32, s: Surface, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    let legs = 1.0;
    for (du, dv) in [(-0.6f32, -0.6f32), (0.6, -0.6), (0.6, 0.6), (-0.6, 0.6)] {
        f.bx(g, Matte, p.x + du * r, y + legs * 0.5, p.y + dv * r, Vec3::new(0.12, legs, 0.12), CHARCOAL);
    }
    g.part(s).cylinder(w, r, r, y + legs, y + legs + h, 12, rgba(color));
    g.part(Metal).cylinder(w, r, r * 0.3, y + legs + h, y + legs + h + 0.5, 12, rgba(mul(color, 0.8)));
}

pub fn vent_stack(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, h: f32, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    g.part(Metal).cylinder(w, r, r, y, y + h, 10, rgba(color));
    g.part(Metal).cylinder(w, r * 1.4, r * 1.4, y + h, y + h + 0.3, 10, rgba(mul(color, 0.8)));
}

pub fn dish(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, color: [f32; 3]) {
    f.bx(g, Matte, p.x, y + 0.6, p.y, Vec3::new(0.1, 1.2, 0.1), CHARCOAL);
    let w = f.to_world(p.x, p.y);
    g.part(Metal).frustum(&MeshData::ngon(w, r * 0.2, 10, 0.0, y + 1.0), &MeshData::ngon(w + Vec2::new(0.0, 0.3), r, 10, 0.0, y + 1.4), rgba(color));
}

pub fn antenna_mast(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, h: f32) {
    let f = &c.frame;
    f.bx(g, Matte, p.x, y + h * 0.5, p.y, Vec3::new(0.25, h, 0.25), c.palette.steel);
    f.bx(g, Matte, p.x, y + h * 0.7, p.y, Vec3::new(1.4, 0.06, 0.06), c.palette.steel);
    aviation_light_local(g, f, c.palette, p.x, y + h + 0.15, p.y, 0.35);
}

#[allow(clippy::too_many_arguments)]
pub fn solar_array(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, w: f32, d: f32, yaw: f32, color: [f32; 3]) {
    f.bx_yaw(g, Glass, p.x, y + 0.6, p.y, Vec3::new(w, 0.08, d), color, yaw);
    f.bx_yaw(g, Matte, p.x, y + 0.3, p.y, Vec3::new(w * 0.8, 0.6, 0.1), CHARCOAL, yaw);
}

pub fn helipad(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, r: f32) {
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Concrete).cylinder(w, r, r, y, y + 0.3, 16, rgba(mul(c.palette.concrete(0), 0.8)));
    g.part(Glow).frustum(&MeshData::ngon(w, r * 0.92, 16, 0.0, y + 0.31), &MeshData::ngon(w, r * 0.92, 16, 0.0, y + 0.36), rgba(mul(c.palette.lamp, 0.4)));
    f.bx(g, Glow, p.x, y + 0.4, p.y, Vec3::new(r * 0.9, 0.08, 0.3), mul(c.palette.lamp, 0.9));
    f.bx(g, Glow, p.x - r * 0.35, y + 0.4, p.y, Vec3::new(0.3, 0.08, r * 0.8), mul(c.palette.lamp, 0.9));
    f.bx(g, Glow, p.x + r * 0.35, y + 0.4, p.y, Vec3::new(0.3, 0.08, r * 0.8), mul(c.palette.lamp, 0.9));
    g.pads.push(Vec3::new(w.x, y + 0.36, w.y));
}

/// Big HVAC / chiller unit with fan rings on top.
#[allow(clippy::too_many_arguments)]
pub fn chiller(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, w: f32, d: f32, yaw: f32, color: [f32; 3]) {
    let f = &c.frame;
    let h = 2.2;
    f.bx_yaw(g, Panel, p.x, y + h * 0.5, p.y, Vec3::new(w, h, d), color, yaw);
    let fans = (w / 2.2).floor().max(1.0) as i32;
    let (s, co) = yaw.sin_cos();
    for i in 0..fans {
        let t = -w * 0.5 + (i as f32 + 0.5) * (w / fans as f32);
        let q = Vec2::new(p.x + t * co, p.y + t * s);
        let wq = f.to_world(q.x, q.y);
        let fr = (0.9f32).min(d * 0.4);
        g.part(Matte).cylinder(wq, fr, fr, y + h, y + h + 0.3, 12, rgba(DARK));
    }
}

// ── [pt-visual] green and solar roofs ───────────────────────────────────────

/// A planted roof on a flat roof at `y`: sedum over the whole roof inside
/// the parapet, shrub beds in rows, a gravel path down the middle. No random
/// draws: high-end buildings that take one keep the rest of their look.
pub fn green_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let inner = super::offset_poly(poly, 0.7);
    if inner.len() < 3 || polygon_area(&inner).abs() < 12.0 {
        return;
    }
    super::ground(g, f, Foliage, &inner, y + 0.12, super::mul(pal.foliage, 0.75));
    let (lo, hi) = super::bounds(&inner);
    // Shrub beds on a 3 m grid, with a gap row for the path.
    let mid = (lo.y + hi.y) * 0.5;
    let mut v = lo.y + 1.5;
    while v < hi.y - 1.0 {
        if (v - mid).abs() > 1.2 {
            let mut u = lo.x + 1.5;
            while u < hi.x - 1.0 {
                let p = Vec2::new(u, v);
                if point_in_polygon(p, &inner) && super::distance_to_boundary(p, &inner) > 0.8 {
                    let k = ((u * 7.3 + v * 3.1).abs() as i32) % 3;
                    let s = [0.9f32, 1.2, 0.7][k as usize];
                    f.bx(g, Foliage, p.x, y + 0.12 + s * 0.3, p.y, Vec3::new(s, s * 0.6, s), super::mul(pal.foliage, 0.9 + 0.1 * k as f32));
                }
                u += 4.0;
            }
        }
        v += 4.0;
    }
    let path = [Vec2::new(lo.x, mid - 0.5), Vec2::new(hi.x, mid - 0.5), Vec2::new(hi.x, mid + 0.5), Vec2::new(lo.x, mid + 0.5)];
    let clipped: Vec<Vec2> = path.iter().map(|p| super::clamp_into(&inner, *p)).collect();
    if polygon_area(&clipped).abs() > 2.0 {
        super::ground(g, f, Paving, &clipped, y + 0.16, super::mul(pal.paving, 1.1));
    }
}

/// Solar panels in rows across a flat roof at `y`, tilted towards the
/// equator side of the lot, a walkway every few rows. Hash-free and
/// draw-free like [`green_roof`].
pub fn solar_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let inner = super::offset_poly(poly, 1.2);
    if inner.len() < 3 || polygon_area(&inner).abs() < 16.0 {
        return;
    }
    let (lo, hi) = super::bounds(&inner);
    let (pw, pd) = (1.9f32, 1.1f32);
    let mut row = 0;
    let mut v = lo.y + pd;
    while v < hi.y - pd * 0.5 {
        row += 1;
        if row % 4 != 0 {
            let mut u = lo.x + pw * 0.5 + 0.2;
            while u < hi.x - pw * 0.5 {
                let corners = [Vec2::new(u - pw * 0.5, v - pd * 0.5), Vec2::new(u + pw * 0.5, v - pd * 0.5), Vec2::new(u + pw * 0.5, v + pd * 0.5), Vec2::new(u - pw * 0.5, v + pd * 0.5)];
                if corners.iter().all(|p| point_in_polygon(*p, &inner)) {
                    // One tilted quad a panel (a roof holds hundreds), low
                    // edge to the front.
                    let (x0, x1, v0, v1) = (u - pw * 0.5 + 0.05, u + pw * 0.5 - 0.05, v - pd * 0.5, v + pd * 0.5);
                    g.part(Glass).quad_facing([f.at(x0, y + 0.3, v0), f.at(x1, y + 0.3, v0), f.at(x1, y + 0.75, v1), f.at(x0, y + 0.75, v1)], Vec3::Y, rgba(pal.glass_dark));
                }
                u += pw + 0.15;
            }
            // The row's rack: a rail under its high edge.
            let rv = v + pd * 0.4;
            let xs: Vec<f32> = (0..=((hi.x - lo.x) as i32)).map(|k| lo.x + k as f32).filter(|x| point_in_polygon(Vec2::new(*x, rv), &inner)).collect();
            if let (Some(&x0), Some(&x1)) = (xs.first(), xs.last()) {
                let (a, b) = (Vec2::new(x0, rv), Vec2::new(x1, rv));
                g.part(Matte).beam(f.at(a.x, y + 0.36, a.y), f.at(b.x, y + 0.36, b.y), 0.12, rgba(CHARCOAL));
            }
        }
        v += pd + 0.55;
    }
}
