//! Open ground, sampling and scatter inside a lot, and the railway track.

#[allow(unused_imports)]
use super::*;

// ── open ground ──────────────────────────────────────────────────────────────

/// A boulder: a faceted lump sunk a little into the ground at height `y`.
pub fn rock_world(g: &mut Geometry, rng: &mut impl Rng, p: Vec2, y: f32, r: f32, color: [f32; 3]) {
    let sides = rng.random_range(5..8);
    let rot = rng.random_range(0.0..std::f32::consts::TAU);
    let h = r * rng.random_range(0.5..1.0);
    let shift = Vec2::new(rng.random_range(-0.3..0.3), rng.random_range(-0.3..0.3)) * r;
    let bottom = MeshData::ngon(p, r, sides, rot, y - r * 0.25);
    let top = MeshData::ngon(p + shift, r * rng.random_range(0.35..0.6), sides, rot + 0.4, y + h);
    g.part(Stone).frustum(&bottom, &top, rgba(color));
}

/// A low bush: two stacked frusta that read as a rounded clump.
pub fn scrub_world(g: &mut Geometry, rng: &mut impl Rng, p: Vec2, y: f32, r: f32, color: [f32; 3]) {
    let rot = rng.random_range(0.0..std::f32::consts::TAU);
    let h = r * rng.random_range(0.6..1.1);
    let b = MeshData::ngon(p, r * 0.45, 6, rot, y);
    let m = MeshData::ngon(p, r, 6, rot + 0.3, y + h * 0.55);
    let t = MeshData::ngon(p, r * 0.35, 6, rot + 0.6, y + h);
    let f = g.part(Foliage);
    f.frustum(&b, &m, rgba(color));
    f.frustum(&m, &t, rgba(mul(color, 1.1)));
}

// ── sampling and scatter ─────────────────────────────────────────────────────

/// Random point inside a polygon at least `margin` from its edges, if one
/// turns up in a couple of dozen tries. The one rejection sampler behind
/// `Ctx::random_inside`, vacant lots, sites and yards.
pub fn random_in_poly(rng: &mut impl Rng, poly: &[Vec2], margin: f32) -> Option<Vec2> {
    let inner = if margin > 0.0 { offset_poly(poly, margin) } else { poly.to_vec() };
    if inner.len() < 3 {
        return None;
    }
    let (lo, hi) = bounds(&inner);
    if hi.x - lo.x < 0.2 || hi.y - lo.y < 0.2 {
        return None;
    }
    for _ in 0..24 {
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        // offset_poly falls back to scaling when the margin is near the
        // inradius, which does not keep the margin on a long thin lot, so
        // the distance is checked as well.
        if point_in_polygon(p, &inner) && (margin <= 0.0 || distance_to_boundary(p, poly) >= margin) {
            return Some(p);
        }
    }
    None
}

/// Tufts of dry weeds scattered over a lot-local polygon.
pub fn weeds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], n: i32) {
    let f = &c.frame;
    let (lo, hi) = bounds(poly);
    for _ in 0..n {
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        if !point_in_polygon(p, poly) {
            continue;
        }
        let s = rng.random_range(0.4..1.1);
        f.bx_yaw(g, Foliage, p.x, s * 0.3, p.y, Vec3::new(s, s * 0.6, s * 0.8), mul(c.palette.foliage_dry, rng.random_range(0.7..1.1)), rng.random_range(0.0..1.5));
    }
}

// ── railway ──────────────────────────────────────────────────────────────────

/// Single track in world coordinates: ballast `ballast_w` wide, a raised
/// bed, sleepers every 0.7 m and two rails. The main line and yard sidings
/// are drawn by the same hand.
pub fn railway_track(g: &mut Geometry, pal: &Palette, pts: &[Vec2], ballast_w: f32) {
    if pts.len() < 2 {
        return;
    }
    g.part(Dirt).path_strip(pts, 0.0, ballast_w, 0.04, rgba(mul(pal.gravel, 0.85)));
    g.part(Dirt).path_strip(pts, 0.0, 3.0, 0.1, rgba(pal.gravel));
    let len = crate::mesh::path_length(pts);
    let mut s = 0.35;
    while s < len {
        let (p, d) = crate::mesh::point_along(pts, s);
        g.part(Wood).boxed(Vec3::new(p.x, 0.17, p.y), Vec3::new(0.24, 0.14, 2.5), d.y.atan2(d.x), rgba(mul(pal.wood, 0.55)));
        s += 0.7;
    }
    for side in [-1.0f32, 1.0] {
        g.part(Metal).path_box(pts, side * 0.72, 0.08, 0.24, 0.38, rgba(mul(pal.steel, 0.75)));
    }
}
