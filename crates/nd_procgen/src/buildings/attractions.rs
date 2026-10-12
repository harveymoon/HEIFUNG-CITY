//! [attractions] The landmark attractions the player places, one of each
//! per city (`nd_sim::ServiceKind::LANDMARKS`): an art museum round a
//! sculpture court, a natural history and science museum with a skeleton
//! and a planetarium, a city history museum behind a colonnade, the
//! pleasure pier on piles out over the sea, a grand park, a zoo, an
//! observatory on its hill, an aquarium, a botanical garden's glasshouse
//! and an opera house of white sails. Each has its neon for the night.
//!
//! Sites are several plots joined into one, so footprints are fitted to
//! whatever outline the site has (`fit_inside`, `poly_inside`); the grounds
//! people walk (park, zoo, garden, pier) come from the sim's plan
//! (`nd_sim::attractions::plan_grounds`) in `Ctx::park`.

use super::parts::*;
use super::{Ctx, Face};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::park::{ParkFeature, ParkLayout};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const NEON_PINK: [f32; 3] = [2.6, 0.35, 1.5];
const NEON_CYAN: [f32; 3] = [0.3, 2.1, 2.6];
const NEON_AMBER: [f32; 3] = [2.6, 1.4, 0.3];
const NEON_VIOLET: [f32; 3] = [1.3, 0.45, 2.6];
const NEON_GREEN: [f32; 3] = [0.45, 2.6, 0.8];
const NEON_RED: [f32; 3] = [2.6, 0.3, 0.25];
const WARM: [f32; 3] = [1.9, 1.3, 0.6];
const WHITE_STONE: [f32; 3] = [0.86, 0.85, 0.80];
const SAIL_WHITE: [f32; 3] = [0.92, 0.92, 0.9];
const BONE: [f32; 3] = [0.84, 0.79, 0.66];
const COPPER: [f32; 3] = [0.55, 0.7, 0.62];
const SILVER: [f32; 3] = [0.78, 0.79, 0.82];
const WATER: [f32; 3] = [0.1, 0.32, 0.4];

// ── helpers ──────────────────────────────────────────────────────────────────

/// A lot-local point in world space.
fn wv(c: &Ctx, p: Vec2) -> Vec2 {
    c.frame.to_world(p.x, p.y)
}

fn w3(c: &Ctx, p: Vec2, y: f32) -> Vec3 {
    c.frame.at2(p, y)
}

/// A solid block: walls and a roof.
fn block(g: &mut Geometry, c: &Ctx, s: Surface, poly: &[Vec2], y0: f32, y1: f32, color: [f32; 3]) {
    if poly.len() < 3 {
        return;
    }
    extrude(g, &c.frame, s, poly, y0, y1, color);
    ground(g, &c.frame, s, poly, y1, mul(color, 0.92));
}

/// The whole site floored with `s`: the outline cleaned of near-repeated
/// corners and triangulated, and under it a fill of small squares, so a
/// notched outline of many joined plots (which an ear-clipper can choke
/// on) still has no bare patches.
fn fill_site(g: &mut Geometry, c: &Ctx, s: Surface, y: f32, color: [f32; 3]) {
    let poly = clean_poly(&c.lot_poly, 0.3);
    ground(g, &c.frame, s, &poly, y, color);
    if is_convex_local(&poly) {
        return;
    }
    let (lo, hi) = bounds(&poly);
    let step = 2.5;
    let mut v = lo.y;
    while v < hi.y {
        let mut u = lo.x;
        while u < hi.x {
            let sq = [Vec2::new(u, v), Vec2::new(u + step, v), Vec2::new(u + step, v + step), Vec2::new(u, v + step)];
            if sq.iter().all(|p| c.inside_local(*p)) {
                ground(g, &c.frame, s, &sq, y - 0.005, color);
            }
            u += step;
        }
        v += step;
    }
}

fn is_convex_local(poly: &[Vec2]) -> bool {
    let n = poly.len();
    if n < 4 {
        return true;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        let z = (b - a).perp_dot(c - b);
        if z.abs() < 1e-4 {
            continue;
        }
        if sign == 0.0 {
            sign = z.signum();
        } else if z.signum() != sign {
            return false;
        }
    }
    true
}

/// The site's paved forecourt and the bounds of the buildable footprint.
fn forecourt(g: &mut Geometry, c: &Ctx, color: [f32; 3]) -> (Vec2, Vec2) {
    // The lot's own outline (a site of joined plots may be notched, and an
    // inset of a notched outline can fold over itself).
    fill_site(g, c, Paving, 0.03, color);
    bounds(&c.poly)
}

/// A rectangle as a share of the footprint's bounds, fitted onto the lot.
fn share_rect(c: &Ctx, cu: f32, cv: f32, hw: f32, hd: f32) -> Vec<Vec2> {
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    fit_inside(c, &rect(lo.x + w * cu, lo.y + d * cv, w * hw, d * hd))
}

/// A lot-local point as a share of the footprint's bounds.
fn share_pt(c: &Ctx, u: f32, v: f32) -> Vec2 {
    let (lo, hi) = bounds(&c.poly);
    Vec2::new(lo.x + (hi.x - lo.x) * u, lo.y + (hi.y - lo.y) * v)
}

/// A circle of radius `r` round `p` stays on the lot `margin` in from its
/// edge.
fn fits(c: &Ctx, p: Vec2, r: f32, margin: f32) -> bool {
    c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) >= r + margin
}

/// Shrink `r` until a circle round `p` fits; None when not even `min` does.
fn fit_radius(c: &Ctx, p: Vec2, r: f32, min: f32) -> Option<f32> {
    if !c.inside_local(p) {
        return None;
    }
    let room = distance_to_boundary(p, &c.lot_poly) - 0.8;
    let r = r.min(room);
    (r >= min).then_some(r)
}

/// A neon tube along a lot-local polyline at height `y`.
fn neon_line(g: &mut Geometry, c: &Ctx, pts: &[Vec2], y: f32, closed: bool, color: [f32; 3]) {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        g.part(Glow).tube(w3(c, pts[i], y), w3(c, pts[(i + 1) % n], y), 0.09, 4, rgba(color));
    }
}

/// A neon ring round a lot-local centre.
fn neon_ring(g: &mut Geometry, c: &Ctx, p: Vec2, r: f32, y: f32, color: [f32; 3]) {
    let pts: Vec<Vec2> = (0..24).map(|i| p + Vec2::from_angle(i as f32 / 24.0 * TAU) * r).collect();
    neon_line(g, c, &pts, y, true, color);
}

/// A dome of radius `r` springing from height `y0` over a lot-local centre.
fn dome(g: &mut Geometry, c: &Ctx, p: Vec2, r: f32, y0: f32, s: Surface, color: [f32; 3]) {
    // Up to a small cap at the crown: a ring of radius zero would leave
    // degenerate triangles.
    let profile: Vec<(f32, f32)> = (0..=8)
        .map(|k| {
            let a = k as f32 / 8.0 * FRAC_PI_2 * 0.95;
            (a.cos() * r, y0 + a.sin() * r)
        })
        .collect();
    g.part(s).lathe(wv(c, p), &profile, 24, rgba(color));
}

/// A lit sign on a face: a dark backing and a row of glowing letters.
#[allow(clippy::too_many_arguments)]
fn marquee(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, y, 0.25, Vec3::new(w + 0.4, h + 0.4, 0.3), DARK);
    let letters = ((w / (h * 0.75)).floor() as i32).clamp(3, 14);
    let lw = w / letters as f32;
    for k in 0..letters {
        let lh = h * rng.random_range(0.7..0.95);
        face.decal(g, f, Glow, t - w * 0.5 + (k as f32 + 0.5) * lw, y, 0.42, lw * 0.7, lh, color);
    }
}

/// Lamp posts along the lot's front, `n` of them.
fn front_lamps(g: &mut Geometry, c: &Ctx, n: usize) {
    let front = c.front_face(&c.lot_poly);
    for k in 0..n {
        let t = (k as f32 + 0.5) / n as f32 - 0.5;
        lamp_post_in(g, c, front.at(t * front.len * 0.9, -1.4), 5.0, c.palette.street_lamp);
    }
}

/// Trees round the lot's edge, clear of the front (the gate) and of what
/// is already there (`clear`).
fn edge_trees(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, every: f32, clear: &dyn Fn(Vec2) -> bool) {
    let ring = offset_poly(&c.lot_poly, 2.6);
    let front = c.front_face_index(&c.lot_poly);
    let n = ring.len();
    for i in 0..n {
        if Some(i) == front {
            continue;
        }
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let len = a.distance(b);
        let k = (len / every).floor() as usize;
        for j in 0..k {
            let p = a.lerp(b, (j as f32 + 0.5) / k as f32);
            if fits(c, p, 1.8, 0.2) && clear(p) {
                let leaf = mul(c.palette.foliage, rng.random_range(0.85..1.15));
                tree_in(g, &c.frame, c.palette, p, rng.random_range(5.0..7.5), rng.random_range(1.6..2.2), leaf);
            }
        }
    }
}

/// The deepest free spots of a grounds plan for something `r` across:
/// on the lot, clear of its paths, features, benches and what was put down
/// already. Lot-local; deterministic.
fn free_spots(c: &Ctx, plan: Option<&ParkLayout>, r: f32, taken: &[(Vec2, f32)], want: usize) -> Vec<Vec2> {
    let (lo, hi) = bounds(&c.lot_poly);
    let step = (r * 0.6).max(3.0);
    let mut cands: Vec<(f32, Vec2)> = Vec::new();
    let mut v = lo.y + step * 0.5;
    while v < hi.y {
        let mut u = lo.x + step * 0.5;
        while u < hi.x {
            let p = Vec2::new(u, v);
            if fits(c, p, r, 1.0) {
                let w = wv(c, p);
                let mut clear = distance_to_boundary(p, &c.lot_poly) - r;
                if let Some(plan) = plan {
                    for path in &plan.paths {
                        for s in path.windows(2) {
                            clear = clear.min(nd_sim::geom::project_on_segment(w, s[0], s[1]).1.distance(w) - r - 2.0);
                        }
                    }
                    for f in &plan.features {
                        clear = clear.min(f.pos.distance(w) - f.radius - r - 1.5);
                    }
                    for (b, _) in &plan.benches {
                        clear = clear.min(b.distance(w) - r - 1.0);
                    }
                }
                for (q, qr) in taken {
                    clear = clear.min(q.distance(p) - qr - r - 2.0);
                }
                if clear >= 0.0 {
                    cands.push((clear, p));
                }
            }
            u += step;
        }
        v += step;
    }
    // Spread out: the clearest first, each next one clear of those before.
    cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.x.total_cmp(&b.1.x)).then(a.1.y.total_cmp(&b.1.y)));
    let mut out: Vec<Vec2> = Vec::new();
    for (_, p) in cands {
        if out.iter().all(|q| q.distance(p) >= r * 2.0 + 2.0) {
            out.push(p);
            if out.len() >= want {
                break;
            }
        }
    }
    out
}

/// [`gate_arch`] on the lot: narrowed until both pylons stand on it, left
/// out when not even a narrow one fits.
#[allow(clippy::too_many_arguments)]
fn gate_arch_in(g: &mut Geometry, c: &Ctx, at: Vec2, along: Vec2, span: f32, h: f32, pillar: [f32; 3], neon: [f32; 3]) {
    let side = along.perp();
    let mut s = span;
    while s >= 3.0 {
        let ok = [-1.0f32, 1.0].iter().all(|k| {
            let p = c.frame.to_local(at + side * (s * 0.5 + 0.8) * *k);
            fits(c, p, 0.8, 0.3)
        }) && fits(c, c.frame.to_local(at), 0.6, 0.3);
        if ok {
            gate_arch(g, at, along, s, h, pillar, neon);
            return;
        }
        s *= 0.8;
    }
}

/// A gateway arch over a way in: two pylons and a lit sign across.
fn gate_arch(g: &mut Geometry, at: Vec2, along: Vec2, span: f32, h: f32, pillar: [f32; 3], neon: [f32; 3]) {
    let side = along.perp();
    for s in [-1.0f32, 1.0] {
        let p = at + side * (span * 0.5) * s;
        g.part(Stone).boxed(Vec3::new(p.x, h * 0.5, p.y), Vec3::new(1.1, h, 1.1), side.to_angle(), rgba(pillar));
        g.part(Glow).boxed(Vec3::new(p.x, h + 0.25, p.y), Vec3::new(1.2, 0.2, 1.2), side.to_angle(), rgba(neon));
    }
    let yaw = side.to_angle();
    g.part(Matte).boxed(Vec3::new(at.x, h - 0.6, at.y), Vec3::new(span + 1.2, 1.4, 0.5), yaw, rgba(DARK));
    for face in [1.0f32, -1.0] {
        let o = along * 0.27 * face;
        g.part(Glow).boxed(Vec3::new(at.x + o.x, h - 0.6, at.y + o.y), Vec3::new(span * 0.85, 0.8, 0.04), yaw, rgba(neon));
    }
}

/// A big ferris wheel standing at `at` (world) in the plane along `dir`,
/// its hub `r + 2` up, its rim and spokes outlined in neon.
fn ferris_wheel(g: &mut Geometry, pal: &Palette, at: Vec2, y0: f32, dir: Vec2, r: f32) {
    let n = dir.perp();
    let n3 = Vec3::new(n.x, 0.0, n.y);
    let hub = Vec3::new(at.x, y0 + r + 2.0, at.y);
    let steel = mul(pal.steel, 1.1);
    for side in [-1.0f32, 1.0] {
        for along in [-0.42f32, 0.42] {
            let foot = at + n * side * (r * 0.18 + 1.2) + dir * r * along;
            g.part(Metal).beam(Vec3::new(foot.x, y0, foot.y), hub + n3 * side * 1.0, 0.35, rgba(steel));
        }
    }
    g.part(Metal).tube(hub - n3 * 1.4, hub + n3 * 1.4, 0.5, 10, rgba(steel));
    let spokes = 24;
    let rim: Vec<Vec3> = (0..spokes)
        .map(|k| {
            let a = k as f32 / spokes as f32 * TAU;
            hub + Vec3::new(dir.x * a.cos() * r, a.sin() * r, dir.y * a.cos() * r)
        })
        .collect();
    let colors = [NEON_PINK, NEON_CYAN, NEON_AMBER, NEON_VIOLET];
    for k in 0..spokes {
        let (a, b) = (rim[k], rim[(k + 1) % spokes]);
        for side in [-0.8f32, 0.8] {
            let o = n3 * side;
            g.part(Metal).tube(a + o, b + o, 0.12, 5, rgba(steel));
            g.part(Glow).tube(a + o * 1.1, b + o * 1.1, 0.07, 4, rgba(colors[(k / 6) % 4]));
            g.part(Metal).tube(hub + o, a + o, 0.07, 4, rgba(mul(steel, 0.9)));
        }
        if k % 2 == 0 {
            g.part(Glow).tube(hub, a, 0.05, 3, rgba(mul(colors[k % 4], 0.7)));
        }
        let gp = a - Vec3::Y * 1.1;
        g.part(Matte).boxed(gp, Vec3::new(1.6, 1.4, 1.3), dir.to_angle(), rgba(pal.awning(k)));
        g.part(Glow).boxed(gp + Vec3::Y * 0.2, Vec3::new(1.2, 0.5, 1.35), dir.to_angle(), rgba(mul(pal.window_warm, 1.2)));
    }
}

/// A carousel: a striped drum, a coned roof and a lit crown (world).
fn carousel(g: &mut Geometry, pal: &Palette, at: Vec2, y0: f32, r: f32) {
    g.part(Wood).cylinder_capped(at, r, r, y0, y0 + 0.4, 20, false, true, rgba(pal.wood));
    for k in 0..10 {
        let a = k as f32 / 10.0 * TAU;
        let p = at + Vec2::from_angle(a) * r * 0.85;
        g.part(Metal).cylinder(p, 0.08, 0.08, y0 + 0.4, y0 + 3.2, 5, rgba(SILVER));
        g.part(Matte).boxed(Vec3::new(p.x, y0 + 1.3, p.y), Vec3::new(1.0, 0.7, 0.35), a + FRAC_PI_2, rgba(pal.awning(k)));
    }
    g.part(Tarp).cylinder_capped(at, r * 1.05, 0.3, y0 + 3.2, y0 + 5.0, 20, true, false, rgba(pal.awning(3)));
    g.part(Glow).tube(Vec3::new(at.x, y0 + 3.2, at.y) + Vec3::X * 0.0, Vec3::new(at.x, y0 + 5.4, at.y), 0.18, 6, rgba(NEON_AMBER));
    let ring: Vec<Vec3> = (0..20).map(|k| { let p = at + Vec2::from_angle(k as f32 / 20.0 * TAU) * r * 1.05; Vec3::new(p.x, y0 + 3.2, p.y) }).collect();
    for k in 0..20 {
        g.part(Glow).tube(ring[k], ring[(k + 1) % 20], 0.06, 3, rgba(NEON_PINK));
    }
}

/// A food stand: a hut, a striped awning, a lit sign (world).
fn food_stand(g: &mut Geometry, pal: &Palette, at: Vec2, y0: f32, facing: Vec2, k: usize) {
    let yaw = facing.perp().to_angle();
    g.part(Panel).boxed(Vec3::new(at.x, y0 + 1.3, at.y), Vec3::new(3.2, 2.6, 2.4), yaw, rgba(mul(pal.panel(k), 0.95)));
    let front = at + facing * 1.5;
    g.part(Tarp).boxed(Vec3::new(front.x, y0 + 2.5, front.y), Vec3::new(3.6, 0.12, 1.4), yaw, rgba(pal.awning(k)));
    g.part(Glow).boxed(Vec3::new(at.x, y0 + 3.0, at.y) + Vec3::new(facing.x, 0.0, facing.y) * 1.22, Vec3::new(2.6, 0.55, 0.05), yaw, rgba([NEON_PINK, NEON_CYAN, NEON_AMBER, NEON_GREEN][k % 4]));
    g.part(Glow).boxed(Vec3::new(front.x, y0 + 1.15, front.y), Vec3::new(2.6, 0.08, 0.05), yaw, rgba(WARM));
}

// ── museums ──────────────────────────────────────────────────────────────────

/// An art museum: white galleries at the back, a stepped spiral drum
/// ringed in neon at the front corner, a sculpture court on the forecourt
/// and an LED art wall.
pub fn art_museum(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.paving, 1.15));
    let hall = share_rect(c, 0.58, 0.66, 0.36, 0.28);
    let h = 13.0;
    block(g, c, Stone, &hall, 0.0, h, WHITE_STONE);
    let glass = pick(rng, &pal.glass);
    for face in c.faces(&hall) {
        if face.len < 4.0 {
            continue;
        }
        face.decal(g, f, Glass, 0.0, h * 0.55, 0.06, face.len * 0.88, 2.4, glass);
        face.decal(g, f, Glow, 0.0, h - 0.9, 0.07, face.len * 0.96, 0.22, NEON_PINK);
    }
    parapet(g, c, &hall, h, 0.9, 0.3, Stone, mul(WHITE_STONE, 0.95));
    // Roof lanterns: the galleries' daylight.
    let ac = poly_center(&hall);
    for k in -1..=1 {
        let p = ac + Vec2::new(k as f32 * 5.0, 0.0);
        if point_in_polygon_local(&hall, p) {
            f.bx(g, Glass, p.x, h + 0.9, p.y, Vec3::new(3.6, 1.8, 3.6), glass);
            f.bx(g, Glow, p.x, h + 0.2, p.y, Vec3::new(3.0, 0.3, 3.0), mul(WARM, 0.6));
        }
    }
    let front = c.front_face(&hall);
    entrance(g, c, rng, &front, 0.0, (6.0f32).min(front.len * 0.5), 5.0);
    if front.len > 16.0 {
        for (k, t) in [-0.34f32, 0.34].iter().enumerate() {
            front.decal(g, f, Matte, t * front.len, h * 0.56, 0.09, 2.4, h * 0.62, pal.awning(k * 2 + 1));
        }
        marquee(g, c, rng, &front, 0.0, 7.4, (front.len * 0.4).min(14.0), 1.2, NEON_PINK);
    }
    // The spiral drum, widening as it rises, each tier lipped in neon.
    let dc = share_pt(c, 0.24, 0.34);
    if let Some(r0) = fit_radius(c, dc, (inradius(&c.poly) * 0.42).clamp(4.0, 11.0), 3.5) {
        let r0 = r0 / 1.2;
        let tiers = 4;
        for k in 0..tiers {
            let y0 = k as f32 * 4.2;
            let r = r0 * (0.85 + 0.12 * k as f32);
            g.part(Plaster).cylinder_capped(wv(c, dc), r * 0.97, r, y0, y0 + 3.4, 32, false, false, rgba(WHITE_STONE));
            g.part(Glass).cylinder_capped(wv(c, dc), r * 0.94, r * 0.94, y0 + 3.4, y0 + 4.2, 32, false, false, rgba(glass));
            neon_ring(g, c, dc, r + 0.05, y0 + 3.45, if k % 2 == 0 { NEON_CYAN } else { NEON_PINK });
        }
        let top = tiers as f32 * 4.2;
        g.part(Plaster).cylinder_capped(wv(c, dc), r0 * 1.2, r0 * 1.2, top - 0.01, top, 32, false, true, rgba(mul(WHITE_STONE, 0.9)));
        dome(g, c, dc, r0 * 0.5, top, Glass, glass);
    }
    // The sculpture court: a lit ring, stacked turned cubes, a red spike.
    let court = [share_pt(c, 0.52, 0.12), share_pt(c, 0.7, 0.18), share_pt(c, 0.88, 0.12)];
    for (k, p) in court.iter().enumerate() {
        if !fits(c, *p, 2.2, 0.5) {
            continue;
        }
        let wp = wv(c, *p);
        match k {
            0 => {
                let ring: Vec<Vec3> = (0..20).map(|i| { let a = i as f32 / 20.0 * TAU; Vec3::new(wp.x + a.cos() * 2.0 * f.along.x, 2.6 + a.sin() * 2.0, wp.y + a.cos() * 2.0 * f.along.y) }).collect();
                for i in 0..20 {
                    g.part(Metal).tube(ring[i], ring[(i + 1) % 20], 0.22, 6, rgba(SILVER));
                    g.part(Glow).tube(ring[i] + Vec3::Y * 0.0, ring[(i + 1) % 20], 0.08, 3, rgba(NEON_VIOLET));
                }
            }
            1 => {
                for s in 0..4 {
                    let size = 1.6 - s as f32 * 0.3;
                    f.bx_yaw(g, Matte, p.x, 0.8 + s as f32 * 1.4, p.y, Vec3::splat(size), pal.awning(s), s as f32 * 0.4);
                }
            }
            _ => {
                g.part(Matte).cylinder(wp, 0.9, 0.05, 0.0, 7.0, 6, rgba([0.75, 0.08, 0.1]));
                g.part(Glow).cylinder(wp, 1.0, 1.0, 0.02, 0.1, 12, rgba(NEON_RED));
            }
        }
    }
    // An LED art wall on a side of the galleries.
    if let Some(side) = c.faces(&hall).into_iter().filter(|fc| fc.out.x.abs() > 0.7 && fc.len > 8.0).max_by(|a, b| a.len.total_cmp(&b.len)) {
        led_screen(g, c, rng, &side, 0.0, h * 0.52, (side.len * 0.6).min(16.0), h * 0.5);
    }
    let clear = |p: Vec2| distance_to_boundary(p, &hall) > 3.0 && !point_in_polygon_local(&hall, p) && p.distance(dc) > 14.0;
    edge_trees(g, c, rng, 9.0, &clear);
    front_lamps(g, c, 4);
}

fn point_in_polygon_local(poly: &[Vec2], p: Vec2) -> bool {
    nd_sim::zoning::point_in_polygon(p, poly)
}

/// A natural history and science museum: a long hall under a glass
/// barrel vault, a planetarium dome ringed in neon, a sauropod skeleton
/// on the forecourt and a rocket at the corner.
pub fn science_museum(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.paving, 1.05));
    let hall = share_rect(c, 0.4, 0.66, 0.3, 0.26);
    let h = 9.0;
    let panel = mix(pick(rng, &pal.panel), [0.85, 0.83, 0.78], 0.6);
    extrude(g, f, Panel, &hall, 0.0, h, panel);
    let (hlo, hhi) = bounds(&hall);
    // The vault along the hall's long side.
    let long_u = hhi.x - hlo.x >= hhi.y - hlo.y;
    let (len, span) = if long_u { (hhi.x - hlo.x, hhi.y - hlo.y) } else { (hhi.y - hlo.y, hhi.x - hlo.x) };
    let mid = (hlo + hhi) * 0.5;
    let along = if long_u { Vec2::X } else { Vec2::Y };
    let across = along.perp();
    let ribs = ((len / 4.0).ceil() as usize).max(2);
    let arc = |t: f32, a: f32| -> Vec3 {
        let p = mid + along * (t - 0.5) * len + across * (a.cos() * span * 0.5);
        w3(c, p, h + a.sin() * span * 0.42)
    };
    let steps = 10;
    for i in 0..ribs {
        let (t0, t1) = (i as f32 / ribs as f32, (i + 1) as f32 / ribs as f32);
        for j in 0..steps {
            let (a0, a1) = (j as f32 / steps as f32 * PI, (j + 1) as f32 / steps as f32 * PI);
            let up = Vec3::Y;
            g.part(Glass).quad_facing([arc(t0, a0), arc(t1, a0), arc(t1, a1), arc(t0, a1)], up + (arc(t0, a0) - w3(c, mid, h)).normalize_or_zero(), rgba(pick(rng, &pal.glass)));
        }
    }
    for i in 0..=ribs {
        let t = i as f32 / ribs as f32;
        for j in 0..steps {
            let (a0, a1) = (j as f32 / steps as f32 * PI, (j + 1) as f32 / steps as f32 * PI);
            g.part(Metal).tube(arc(t, a0), arc(t, a1), 0.12, 4, rgba(SILVER));
        }
    }
    // Lit spine along the crown of the vault.
    g.part(Glow).tube(arc(0.0, FRAC_PI_2) + Vec3::Y * 0.15, arc(1.0, FRAC_PI_2) + Vec3::Y * 0.15, 0.1, 4, rgba(NEON_CYAN));
    let front = c.front_face(&hall);
    entrance(g, c, rng, &front, 0.0, (7.0f32).min(front.len * 0.5), 5.0);
    if front.len > 12.0 {
        marquee(g, c, rng, &front, 0.0, 6.8, (front.len * 0.55).min(18.0), 1.1, NEON_GREEN);
    }
    // The planetarium.
    let pc = share_pt(c, 0.78, 0.42);
    if let Some(r) = fit_radius(c, pc, (inradius(&c.poly) * 0.4).clamp(5.0, 11.0), 4.0) {
        g.part(Concrete).cylinder_capped(wv(c, pc), r, r, 0.0, 5.0, 32, false, false, rgba(mul(pal.concrete(1), 1.1)));
        dome(g, c, pc, r * 1.02, 5.0, Metal, SILVER);
        neon_ring(g, c, pc, r * 1.04, 5.05, NEON_VIOLET);
        // Stars on the drum.
        for k in 0..12 {
            let a = k as f32 / 12.0 * TAU + 0.13;
            let q = pc + Vec2::from_angle(a) * (r + 0.05);
            let y = 1.5 + (k * 7 % 5) as f32 * 0.6;
            g.part(Glow).boxed(w3(c, q, y), Vec3::splat(0.22), 0.0, rgba([1.8, 1.8, 2.4]));
        }
    }
    // A sauropod skeleton on the forecourt, spot-lit.
    let sk = share_pt(c, 0.36, 0.17);
    if fits(c, sk, 7.0, 0.5) {
        let dir = f.along;
        let side = dir.perp();
        let at = wv(c, sk);
        let bone = rgba(BONE);
        let spine: Vec<Vec3> = (0..=16)
            .map(|k| {
                let t = k as f32 / 16.0;
                let x = (t - 0.5) * 13.0;
                // Tail low behind, the back high, the neck rising ahead.
                let y = if t < 0.35 { 1.2 + t / 0.35 * 2.6 } else if t < 0.6 { 3.8 } else { 3.8 + (t - 0.6) / 0.4 * 4.2 };
                Vec3::new(at.x + dir.x * x, y, at.y + dir.y * x)
            })
            .collect();
        for k in 0..16 {
            g.part(Matte).tube(spine[k], spine[k + 1], 0.22 - k as f32 * 0.006, 5, bone);
        }
        for k in 6..10 {
            let p = spine[k];
            for s in [-1.0f32, 1.0] {
                let rib = p + Vec3::new(side.x, 0.0, side.y) * s * 0.9 - Vec3::Y * 1.1;
                g.part(Matte).tube(p, rib, 0.07, 3, bone);
            }
        }
        for (k, s) in [(6usize, -1.0f32), (6, 1.0), (10, -1.0), (10, 1.0)] {
            let hip = spine[k] + Vec3::new(side.x, 0.0, side.y) * s * 0.6;
            g.part(Matte).tube(hip, Vec3::new(hip.x, 0.0, hip.z), 0.2, 5, bone);
        }
        g.part(Matte).boxed(spine[16] + Vec3::new(dir.x, 0.0, dir.y) * 0.4, Vec3::new(1.0, 0.5, 0.45), dir.to_angle(), bone);
        g.part(Concrete).boxed(Vec3::new(at.x, 0.1, at.y), Vec3::new(14.5, 0.2, 3.6), dir.to_angle(), rgba(mul(pal.concrete(0), 0.8)));
        for s in [-1.0f32, 1.0] {
            let l = at + side * 2.4 * s;
            g.part(Glow).boxed(Vec3::new(l.x, 0.3, l.y), Vec3::new(0.4, 0.4, 0.4), 0.0, rgba(WARM));
        }
    }
    // A rocket at the corner.
    let rc = share_pt(c, 0.9, 0.12);
    if fits(c, rc, 2.6, 0.4) {
        let at = wv(c, rc);
        g.part(Metal).cylinder_capped(at, 1.3, 1.3, 0.0, 16.0, 16, false, false, rgba(SILVER));
        g.part(Metal).cylinder_capped(at, 1.3, 0.05, 16.0, 20.0, 16, false, false, rgba([0.8, 0.15, 0.12]));
        for k in 0..4 {
            let a = k as f32 * FRAC_PI_2 + 0.3;
            let d = Vec2::from_angle(a);
            let p = at + d * 1.6;
            g.part(Metal).boxed(Vec3::new(p.x, 1.6, p.y), Vec3::new(1.4, 3.2, 0.15), a, rgba([0.8, 0.15, 0.12]));
        }
        for y in [5.0f32, 10.0] {
            g.part(Glow).cylinder(at, 1.33, 1.33, y, y + 0.25, 16, rgba(NEON_CYAN));
        }
        g.part(Glow).cylinder(at, 1.6, 0.6, 0.0, 0.8, 12, rgba(NEON_AMBER));
    }
    front_lamps(g, c, 4);
}

/// The city history and design museum: a colonnade under a pediment on a
/// plinth of steps, a clock tower, an old tram on show and the city's old
/// street signs lit on posts.
pub fn history_museum(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.paving, 1.0));
    let stone = mix(pal.stone, WHITE_STONE, 0.5);
    let body = share_rect(c, 0.5, 0.62, 0.34, 0.26);
    let steps = 4usize;
    let rise = 0.3;
    plinth(g, c, &body, steps, rise, 0.6, Stone, mul(stone, 0.9));
    let base = steps as f32 * rise;
    let h = 11.0;
    block(g, c, Stone, &body, base, base + h, stone);
    let front = c.front_face(&body);
    // The colonnade before the front face.
    let cols = ((front.len / 3.6).floor() as i32).clamp(4, 12);
    let col_h = h - 2.0;
    for k in 0..cols {
        let t = (k as f32 + 0.5) / cols as f32 - 0.5;
        let p = front.at(t * front.len * 0.92, 2.2);
        if c.inside_local(p) {
            g.part(Stone).cylinder_capped(wv(c, p), 0.55, 0.48, base, base + col_h, 10, false, false, rgba(mul(stone, 1.05)));
            f.bx(g, Stone, p.x, base + col_h + 0.2, p.y, Vec3::new(1.3, 0.4, 1.3), stone);
        }
    }
    // Architrave and pediment over the columns.
    let reach = 2.8f32.min(c.reach_out(&front) - 0.3).max(0.6);
    front.bx(g, f, Stone, 0.0, base + col_h + 0.9, reach * 0.5, Vec3::new(front.len, 1.0, reach), stone);
    front.decal(g, f, Glow, 0.0, base + col_h + 0.9, reach + 0.02, front.len * 0.9, 0.18, NEON_AMBER);
    let (pa, pb) = (front.at(-front.len * 0.5, reach), front.at(front.len * 0.5, reach));
    let (qa, qb) = (front.at(-front.len * 0.5, -2.0), front.at(front.len * 0.5, -2.0));
    let ytop = base + col_h + 1.4;
    let apex_h = (front.len * 0.16).min(5.0);
    let apex_f = (pa + pb) * 0.5;
    let apex_b = (qa + qb) * 0.5;
    let out3 = Vec3::new(c.frame.dir3(front.out).x, 0.0, c.frame.dir3(front.out).z);
    g.part(Stone).tri_facing(w3(c, pa, ytop), w3(c, pb, ytop), w3(c, apex_f, ytop + apex_h), out3, rgba(mul(stone, 1.05)));
    g.part(Stone).tri_facing(w3(c, pa, ytop), w3(c, pb, ytop), w3(c, apex_f, ytop + apex_h), -out3, rgba(mul(stone, 0.8)));
    for (a, b) in [(pa, qa), (pb, qb)] {
        let roof = [w3(c, a, ytop), w3(c, b, ytop), w3(c, apex_b, ytop + apex_h), w3(c, apex_f, ytop + apex_h)];
        let mid3 = (roof[0] + roof[1]) * 0.5;
        g.part(Metal).quad_facing(roof, Vec3::Y + (mid3 - w3(c, (apex_f + apex_b) * 0.5, ytop)).normalize_or_zero(), rgba(COPPER));
    }
    // Neon along the pediment's rake.
    g.part(Glow).tube(w3(c, pa, ytop + 0.1), w3(c, apex_f, ytop + apex_h + 0.1), 0.08, 4, rgba(NEON_AMBER));
    g.part(Glow).tube(w3(c, apex_f, ytop + apex_h + 0.1), w3(c, pb, ytop + 0.1), 0.08, 4, rgba(NEON_AMBER));
    entrance(g, c, rng, &front, 0.0, (4.0f32).min(front.len * 0.5), 5.5);
    for face in c.faces(&body) {
        if face.len < 6.0 || face.out.y < -0.5 {
            continue;
        }
        let n = (face.len / 5.0).floor() as i32;
        for k in 0..n {
            let t = (k as f32 + 0.5) / n as f32 - 0.5;
            face.glaze(g, c, t * face.len, base + h * 0.55, 0.06, 1.6, 3.8, Pane::Lit(mul(WARM, 0.5)));
        }
    }
    // The clock tower at the back corner.
    let tc = share_pt(c, 0.84, 0.74);
    if fits(c, tc, 3.0, 0.5) {
        let th = h + 12.0;
        f.bx(g, Stone, tc.x, th * 0.5, tc.y, Vec3::new(4.4, th, 4.4), mul(stone, 0.95));
        for face in c.faces(&rect(tc.x, tc.y, 2.2, 2.2)) {
            face.decal(g, f, Glow, 0.0, th - 2.6, 0.05, 2.6, 2.6, [1.9, 1.7, 1.2]);
            face.decal(g, f, Matte, 0.0, th - 2.2, 0.08, 0.15, 1.1, DARK);
        }
        let top = wv(c, tc);
        g.part(Metal).cylinder_capped(top, 3.0, 0.1, th, th + 4.0, 4, true, false, rgba(COPPER));
    }
    // An old tram on a length of track, and the city's old signs.
    let tram = share_pt(c, 0.3, 0.16);
    if fits(c, tram, 5.5, 0.3) {
        let at = wv(c, tram);
        let dir = f.along;
        let yaw = dir.to_angle();
        for s in [-0.75f32, 0.75] {
            let q = at + dir.perp() * s;
            g.part(Metal).boxed(Vec3::new(q.x, 0.08, q.y), Vec3::new(12.0, 0.12, 0.12), yaw, rgba(SILVER));
        }
        g.part(Panel).boxed(Vec3::new(at.x, 1.9, at.y), Vec3::new(9.0, 2.6, 2.3), yaw, rgba([0.62, 0.12, 0.1]));
        g.part(Panel).boxed(Vec3::new(at.x, 3.35, at.y), Vec3::new(8.6, 0.3, 2.0), yaw, rgba([0.85, 0.8, 0.65]));
        for k in 0..6 {
            let x = -3.5 + k as f32 * 1.4;
            for s in [-1.0f32, 1.0] {
                let q = at + dir * x + dir.perp() * s * 1.17;
                g.part(Glow).boxed(Vec3::new(q.x, 2.2, q.y), Vec3::new(0.9, 0.8, 0.04), yaw, rgba(mul(WARM, 0.8)));
            }
        }
        g.part(Metal).beam(Vec3::new(at.x, 3.5, at.y), Vec3::new(at.x - dir.x * 2.0, 5.2, at.y - dir.y * 2.0), 0.06, rgba(DARK));
    }
    for k in 0..4 {
        let p = share_pt(c, 0.55 + 0.11 * k as f32, 0.08);
        if fits(c, p, 0.8, 0.3) {
            f.bx(g, Metal, p.x, 1.6, p.y, Vec3::new(0.12, 3.2, 0.12), DARK);
            f.bx(g, Glow, p.x, 3.3, p.y, Vec3::new(1.6, 0.5, 0.06), [NEON_CYAN, NEON_PINK, NEON_GREEN, NEON_AMBER][k]);
        }
    }
    let clear = |p: Vec2| !point_in_polygon_local(&body, p) && distance_to_boundary(p, &body) > 4.0 && p.distance(tc) > 6.0 && p.distance(tram) > 8.0;
    edge_trees(g, c, rng, 10.0, &clear);
    front_lamps(g, c, 4);
}

// ── the pleasure pier ───────────────────────────────────────────────────────

/// The pleasure pier: a lit gateway and ticket booths ashore, and the deck
/// out on piles over the sea (`Ctx::pier`) with food stands, lamps and
/// string lights along it, and on the head a ferris wheel, a coaster, a
/// carousel and an arcade, all neon after dark.
pub fn pier(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.paving, 1.0));
    // The way down to the deck: boards from the street to the shore.
    let gate = c.front_face(&c.lot_poly);
    let gate_at = gate.at(0.0, -3.0);
    if let Some(deck) = c.pier.clone() {
        let pts = [wv(c, gate_at), deck.root];
        g.part(Wood).ground_strip(&pts, 8.0, 0.12, rgba(pal.wood));
        pier_deck(g, c, rng, &deck);
    }
    // The gateway sign over the way in.
    let along = c.frame.dir3(-gate.out);
    gate_arch_in(g, c, wv(c, gate_at), Vec2::new(along.x, along.z), 9.0, 7.0, mul(pal.concrete(0), 1.1), NEON_PINK);
    // Ticket booths and lamps either side.
    for s in [-1.0f32, 1.0] {
        let p = gate.at(s * 9.0, -6.0);
        if fits(c, p, 1.6, 0.3) {
            f.bx(g, Panel, p.x, 1.3, p.y, Vec3::new(2.4, 2.6, 2.0), pal.awning(if s < 0.0 { 0 } else { 2 }));
            f.bx(g, Glow, p.x, 2.9, p.y, Vec3::new(2.6, 0.4, 2.2), NEON_CYAN);
        }
    }
    front_lamps(g, c, 5);
    let _ = rng.random::<u32>();
}

/// The deck on piles, its rails and lamps, the stands and the rides.
fn pier_deck(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, deck: &nd_sim::attractions::PierDeck) {
    let pal = c.palette;
    let y = nd_sim::attractions::PIER_DECK_RISE;
    let drop = c.shore.map_or(2.6, |s| s.drop.max(1.2)) + y;
    let sea = deck.seaward;
    let side = deck.side();
    let outline = deck.outline();
    g.part(Wood).ground_polygon(&outline, y, rgba(mul(pal.wood, 1.05)));
    // The deck's edge, down to the beams.
    let n = outline.len();
    for i in 0..n {
        let (a, b) = (outline[i], outline[(i + 1) % n]);
        let out = (b - a).perp().normalize_or_zero();
        let out = if (a + b - deck.root - deck.tip).dot(out) < 0.0 { -out } else { out };
        let q = [Vec3::new(a.x, y - 0.9, a.y), Vec3::new(b.x, y - 0.9, b.y), Vec3::new(b.x, y, b.y), Vec3::new(a.x, y, a.y)];
        g.part(Wood).quad_facing(q, Vec3::new(out.x, 0.0, out.y), rgba(mul(pal.wood, 0.7)));
    }
    // Piles in rows across the walk and a grid under the head.
    let len = deck.length();
    let neck = deck.length() - deck.head;
    let mut d = 2.0;
    while d <= len {
        let half = if d >= neck { deck.head * 0.5 - 1.0 } else { deck.width * 0.5 - 0.6 };
        let count = if d >= neck { 5 } else { 2 };
        for k in 0..count {
            let t = if count == 1 { 0.0 } else { k as f32 / (count - 1) as f32 * 2.0 - 1.0 };
            let p = deck.root + sea * d + side * half * t;
            g.part(Wood).cylinder_capped(p, 0.32, 0.32, -drop - 2.0, y - 0.9, 6, false, false, rgba(mul(pal.wood, 0.55)));
        }
        // Cross-bracing under the walk.
        if d < neck {
            let (a, b) = (deck.root + sea * d + side * (deck.width * 0.5 - 0.6), deck.root + sea * d - side * (deck.width * 0.5 - 0.6));
            g.part(Wood).beam(Vec3::new(a.x, y - 0.9, a.y), Vec3::new(b.x, -drop * 0.6, b.y), 0.18, rgba(mul(pal.wood, 0.5)));
        }
        d += 7.0;
    }
    // Rails, lamps and string lights along both sides of the walk.
    let rail = rgba([0.88, 0.88, 0.86]);
    for s in [-1.0f32, 1.0] {
        let a = deck.root + side * s * (deck.width * 0.5 - 0.2) + sea * 3.0;
        let b = deck.root + side * s * (deck.width * 0.5 - 0.2) + sea * neck;
        g.part(Matte).tube(Vec3::new(a.x, y + 1.05, a.y), Vec3::new(b.x, y + 1.05, b.y), 0.05, 4, rail);
        let mut k = 0;
        let mut t = 3.0;
        let mut prev: Option<Vec3> = None;
        while t <= neck {
            let p = deck.root + side * s * (deck.width * 0.5 - 0.2) + sea * t;
            g.part(Matte).boxed(Vec3::new(p.x, y + 0.5, p.y), Vec3::new(0.08, 1.0, 0.08), 0.0, rail);
            if k % 4 == 0 {
                lamp_post_world(g, p - side * s * 0.4, 5.2, pal.street_lamp);
                let top = Vec3::new(p.x, y + 5.0, p.y);
                if let Some(q) = prev {
                    g.part(Matte).wire(q, top, 0.6, 0.02, rgba(DARK));
                    for j in 1..8 {
                        let u = j as f32 / 8.0;
                        let at = q.lerp(top, u) - Vec3::Y * (0.6 * 4.0 * u * (1.0 - u) + 0.1);
                        g.part(Glow).boxed(at, Vec3::splat(0.14), 0.0, rgba([NEON_PINK, NEON_AMBER, NEON_CYAN][j % 3]));
                    }
                }
                prev = Some(top);
            }
            k += 1;
            t += 3.0;
        }
    }
    // Food stands along the walk, alternating sides.
    let mut t = 16.0;
    let mut k = 0usize;
    while t < neck - 10.0 {
        let s = if k % 2 == 0 { 1.0 } else { -1.0 };
        let p = deck.root + sea * t + side * s * (deck.width * 0.5 - 2.4);
        food_stand(g, pal, p, y, -side * s, k + rng.random_range(0..2));
        k += 1;
        t += 17.0;
    }
    // The head: the arcade across its landward end, the wheel and the
    // coaster either side, the carousel out at the end.
    let neck_at = deck.root + sea * neck;
    let arc_c = neck_at + sea * 6.0;
    let yaw = side.to_angle();
    g.part(Panel).boxed(Vec3::new(arc_c.x, y + 3.2, arc_c.y), Vec3::new(deck.head * 0.42, 6.4, 7.0), yaw, rgba(mul(pal.panel(2), 0.8)));
    g.part(Glow).boxed(Vec3::new(arc_c.x, y + 1.6, arc_c.y) - Vec3::new(sea.x, 0.0, sea.y) * 3.52, Vec3::new(deck.head * 0.36, 2.4, 0.04), yaw, rgba(mul(WARM, 0.7)));
    g.part(Glow).boxed(Vec3::new(arc_c.x, y + 5.1, arc_c.y) - Vec3::new(sea.x, 0.0, sea.y) * 3.55, Vec3::new(deck.head * 0.32, 1.2, 0.05), yaw, rgba(NEON_PINK));
    // A lit cornice round the arcade's roof.
    for s in [-1.0f32, 1.0] {
        let e = arc_c + sea * 3.55 * s;
        g.part(Glow).boxed(Vec3::new(e.x, y + 6.5, e.y), Vec3::new(deck.head * 0.43, 0.15, 0.12), yaw, rgba(NEON_CYAN));
        let e = arc_c + side * (deck.head * 0.215) * s;
        g.part(Glow).boxed(Vec3::new(e.x, y + 6.5, e.y), Vec3::new(0.12, 0.15, 7.1), yaw, rgba(NEON_CYAN));
    }
    let wheel_at = deck.head_centre() + side * (deck.head * 0.22) + sea * 3.0;
    ferris_wheel(g, pal, wheel_at, y, sea, (deck.head * 0.3).min(13.0));
    coaster(g, deck.head_centre() - side * (deck.head * 0.24) + sea * 2.0, y, sea, deck.head * 0.22, deck.head * 0.34);
    carousel(g, pal, deck.tip - sea * 5.5 - side * 1.0, y, 3.4);
}

/// A little coaster: a closed loop of track that dips and climbs over its
/// supports, lit along its rails (world).
fn coaster(g: &mut Geometry, at: Vec2, y0: f32, dir: Vec2, rw: f32, rl: f32) {
    let side = dir.perp();
    let n = 40;
    let pts: Vec<Vec3> = (0..n)
        .map(|k| {
            let a = k as f32 / n as f32 * TAU;
            let p = at + dir * (a.cos() * rl * 0.5) + side * (a.sin() * rw * 0.5);
            let h = 4.0 + 3.0 * (2.0 * a).sin() + 2.5 * (a + 1.0).cos();
            Vec3::new(p.x, y0 + h.max(1.2), p.y)
        })
        .collect();
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        g.part(Metal).tube(a, b, 0.14, 4, rgba([0.85, 0.2, 0.18]));
        g.part(Glow).tube(a + Vec3::Y * 0.22, b + Vec3::Y * 0.22, 0.05, 3, rgba(NEON_AMBER));
        if k % 3 == 0 {
            g.part(Metal).tube(Vec3::new(a.x, y0, a.z), a, 0.1, 4, rgba(SILVER));
        }
    }
    // A train of cars on the track.
    for k in 0..4 {
        let p = pts[(k + 5) % n];
        g.part(Matte).boxed(p + Vec3::Y * 0.5, Vec3::new(1.4, 0.7, 1.1), dir.to_angle(), rgba([0.2, 0.3, 0.75]));
    }
}

// ── the grounds people walk ─────────────────────────────────────────────────

/// A grand city park: the park itself (lakes, lawns, a bandstand, rides,
/// fountains, trees, from the sim's plan) and its landmarks: a lit gate
/// at the way in, a glass pavilion, a boathouse on the lake and an
/// obelisk.
pub fn grand_park(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    fill_site(g, c, Foliage, 0.02, mul(pal.foliage, 1.0));
    super::park::draw(g, c, rng);
    let plan = c.park.as_ref();
    let mut taken: Vec<(Vec2, f32)> = Vec::new();
    // The gate where the main path comes in.
    if let Some(path) = plan.and_then(|p| p.paths.iter().find(|p| p.len() >= 2)) {
        let along = (path[1] - path[0]).normalize_or_zero();
        let at = path[0] + along * 2.0;
        gate_arch_in(g, c, at, along, 7.0, 6.0, mul(pal.stone, 1.1), NEON_GREEN);
    }
    // A glass pavilion: an octagon of glass under a dome.
    for p in free_spots(c, plan, 7.0, &taken, 1) {
        let w = wv(c, p);
        let glass = pick(rng, &pal.glass);
        g.part(Stone).cylinder_capped(w, 7.4, 7.4, 0.0, 0.5, 8, false, true, rgba(mul(pal.stone, 1.1)));
        g.part(Glass).cylinder_capped(w, 6.0, 6.0, 0.5, 5.0, 8, false, false, rgba(glass));
        for k in 0..8 {
            let q = w + Vec2::from_angle(k as f32 / 8.0 * TAU + PI / 8.0) * 6.05;
            g.part(Metal).cylinder(q, 0.12, 0.12, 0.5, 5.0, 4, rgba([0.9, 0.9, 0.88]));
        }
        dome(g, c, p, 6.0, 5.0, Glass, glass);
        neon_ring(g, c, p, 6.1, 5.0, NEON_GREEN);
        g.part(Glow).boxed(Vec3::new(w.x, 2.6, w.y), Vec3::new(2.0, 0.3, 2.0), 0.0, rgba(WARM));
        taken.push((p, 7.0));
    }
    // A boathouse on the lake's shore.
    if let Some(lake) = plan.and_then(|p| p.features.iter().find(|f| matches!(f.kind, ParkFeature::Lake | ParkFeature::Pond))) {
        let lc = c.frame.to_local(lake.pos);
        for p in free_spots(c, plan, 3.5, &taken, 12) {
            if p.distance(lc) > lake.radius + 9.0 {
                continue;
            }
            let d = (lc - p).normalize_or_zero();
            let yaw = (c.frame.dir3(d).z).atan2(c.frame.dir3(d).x);
            let w = wv(c, p);
            g.part(Wood).boxed(Vec3::new(w.x, 1.6, w.y), Vec3::new(6.0, 3.2, 4.4), yaw, rgba(mul(pal.wood, 0.9)));
            g.part(Metal).boxed(Vec3::new(w.x, 3.5, w.y), Vec3::new(6.6, 0.6, 5.0), yaw, rgba(COPPER));
            g.part(Glow).boxed(Vec3::new(w.x, 2.3, w.y), Vec3::new(6.05, 0.5, 2.0), yaw, rgba(mul(WARM, 0.8)));
            taken.push((p, 3.5));
            break;
        }
    }
    // An obelisk ringed with light.
    for p in free_spots(c, plan, 2.5, &taken, 1) {
        let w = wv(c, p);
        g.part(Stone).cylinder_capped(w, 1.6, 1.6, 0.0, 0.8, 4, false, true, rgba(mul(pal.stone, 0.9)));
        g.part(Stone).cylinder_capped(w, 0.9, 0.55, 0.8, 14.0, 4, false, false, rgba(mul(pal.stone, 1.15)));
        g.part(Stone).cylinder_capped(w, 0.55, 0.02, 14.0, 15.5, 4, false, false, rgba(mul(pal.stone, 1.15)));
        g.part(Glow).cylinder(w, 1.65, 1.65, 0.8, 0.95, 16, rgba(NEON_GREEN));
        taken.push((p, 2.5));
    }
}

/// A zoo: enclosures of savanna, rock and pools with their animals round
/// the visitors' loop, an aviary under a wire dome, a lit gate.
pub fn zoo(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    let f = &c.frame;
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.95));
    let plan = c.park.as_ref();
    if let Some(plan) = plan {
        for path in &plan.paths {
            g.part(Paving).ground_strip(path, 3.2, 0.06, rgba(mul(pal.paving, 1.1)));
        }
        for (k, (p, yaw)) in plan.benches.iter().enumerate() {
            bench_world(g, *p, *yaw + FRAC_PI_2, pal.wood, DARK);
            if k % 2 == 0 {
                let l = *p + Vec2::from_angle(*yaw) * -1.2;
                lamp_post_world(g, l, 4.2, pal.street_lamp);
            }
        }
        if let Some(path) = plan.paths.iter().find(|p| p.len() >= 2) {
            let along = (path[1] - path[0]).normalize_or_zero();
            gate_arch_in(g, c, path[0] + along * 1.5, along, 6.5, 6.0, mul(pal.wood, 0.8), NEON_AMBER);
        }
    }
    // The perimeter fence.
    for face in c.faces(&offset_poly(&c.lot_poly, 0.6)) {
        if face.len > 1.0 {
            face.bx(g, f, Metal, 0.0, 0.7, 0.0, Vec3::new(face.len, 1.4, 0.06), mul(pal.fence_mesh, 0.9));
            face.bx(g, f, Metal, 0.0, 1.45, 0.0, Vec3::new(face.len, 0.1, 0.12), mul(pal.wood, 0.7));
        }
    }
    let mut taken: Vec<(Vec2, f32)> = Vec::new();
    let sizes = [12.0f32, 10.0, 10.0, 9.0, 9.0, 8.0, 8.0, 7.0];
    let mut biome = 0usize;
    for r0 in sizes {
        // As big as the room allows: smaller pens where the site is tight.
        let Some((p, r)) = [r0, r0 * 0.75, r0 * 0.55].into_iter().find_map(|r| free_spots(c, plan, r, &taken, 1).into_iter().next().map(|p| (p, r))) else { continue };
        taken.push((p, r));
        let w = wv(c, p);
        enclosure(g, pal, rng, w, r, biome);
        biome += 1;
    }
    // The aviary in the biggest spot left.
    if let Some(p) = free_spots(c, plan, 7.0, &taken, 1).into_iter().next() {
        taken.push((p, 7.0));
        let w = wv(c, p);
        let r = 7.0;
        for k in 0..12 {
            let a = k as f32 / 12.0 * TAU;
            let mut prev = Vec3::new(w.x + a.cos() * r, 0.0, w.y + a.sin() * r);
            for j in 1..=6 {
                let e = j as f32 / 6.0 * FRAC_PI_2;
                let q = Vec3::new(w.x + a.cos() * r * e.cos(), r * 0.9 * e.sin(), w.y + a.sin() * r * e.cos());
                g.part(Metal).tube(prev, q, 0.06, 3, rgba([0.85, 0.86, 0.88]));
                prev = q;
            }
        }
        let ring: Vec<Vec3> = (0..24).map(|k| { let a = k as f32 / 24.0 * TAU; Vec3::new(w.x + a.cos() * r, 0.2, w.y + a.sin() * r) }).collect();
        for k in 0..24 {
            g.part(Glow).tube(ring[k], ring[(k + 1) % 24], 0.06, 3, rgba(NEON_CYAN));
        }
        for k in 0..10 {
            let a = k as f32 * 2.4;
            let q = w + Vec2::from_angle(a) * (r * 0.5 * ((k % 3) as f32 * 0.3 + 0.3));
            g.part(Matte).boxed(Vec3::new(q.x, 2.0 + (k % 4) as f32, q.y), Vec3::new(0.5, 0.2, 0.3), a, rgba(pal.awning(k)));
        }
        tree_kind(g, w, 0.0, TreeKind::Round, 5.0, 2.2, pal.bark, pal.foliage);
    }
    // Trees in the corners left.
    for p in free_spots(c, plan, 2.5, &taken, 14) {
        tree_in(g, f, pal, p, rng.random_range(5.0..8.0), rng.random_range(1.8..2.6), mul(pal.foliage, rng.random_range(0.85..1.1)));
    }
}

/// One enclosure: its ground, a fence and its animals (world).
fn enclosure(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, w: Vec2, r: f32, biome: usize) {
    let ring = |rr: f32| -> Vec<Vec2> { (0..20).map(|k| w + Vec2::from_angle(k as f32 / 20.0 * TAU) * rr).collect() };
    let (floor, s): ([f32; 3], Surface) = match biome % 6 {
        0 => ([0.74, 0.62, 0.38], Dirt),
        1 => ([0.52, 0.45, 0.36], Dirt),
        2 => ([0.5, 0.5, 0.5], Stone),
        3 => ([0.82, 0.84, 0.86], Concrete),
        4 => ([0.38, 0.5, 0.26], Foliage),
        _ => ([0.6, 0.55, 0.4], Dirt),
    };
    g.part(s).ground_polygon(&ring(r - 0.4), 0.06, rgba(floor));
    // Fence: posts and two rails.
    let posts = ring(r);
    for k in 0..posts.len() {
        let (a, b) = (posts[k], posts[(k + 1) % posts.len()]);
        g.part(Metal).cylinder(a, 0.06, 0.06, 0.0, 1.6, 4, rgba(DARK));
        for y in [0.7f32, 1.5] {
            g.part(Metal).tube(Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y), 0.03, 3, rgba(mul(pal.steel, 0.8)));
        }
    }
    let n = |k: usize| w + Vec2::from_angle(k as f32 * 2.1 + biome as f32) * r * (0.25 + 0.15 * (k % 3) as f32);
    match biome % 6 {
        // Savanna: giraffes and zebras by a lone tree.
        0 => {
            tree_kind(g, w + Vec2::new(r * 0.4, -r * 0.3), 0.0, TreeKind::Round, 4.5, 2.6, pal.bark, mul(pal.foliage_dry, 1.1));
            for k in 0..2 {
                giraffe(g, n(k), k as f32 * 1.7);
            }
            for k in 2..5 {
                zebra(g, n(k), k as f32 * 2.3);
            }
        }
        // Elephants at a water hole.
        1 => {
            g.part(Matte).ground_polygon(&ring(r * 0.3).iter().map(|p| *p + Vec2::new(r * 0.3, 0.0)).collect::<Vec<_>>(), 0.08, rgba(WATER));
            for k in 0..2 {
                elephant(g, n(k), k as f32 * 2.6 + 0.4, 1.0 - k as f32 * 0.3);
            }
        }
        // Big cats on the rocks.
        2 => {
            for k in 0..3 {
                let q = n(k);
                let rh = 1.6 + k as f32 * 0.6;
                g.part(Stone).boxed(Vec3::new(q.x, rh * 0.5, q.y), Vec3::new(3.0, rh, 2.4), k as f32, rgba(mul(pal.stone, 0.8)));
            }
            for k in 3..5 {
                lion(g, n(k), k as f32 * 1.3);
            }
        }
        // Penguins round a pool.
        3 => {
            g.part(Matte).ground_polygon(&ring(r * 0.55), 0.08, rgba([0.2, 0.55, 0.7]));
            for k in 0..9 {
                let q = w + Vec2::from_angle(k as f32 * 0.7) * r * 0.72;
                g.part(Matte).cylinder_capped(q, 0.2, 0.12, 0.06, 0.65, 6, false, true, rgba([0.08, 0.08, 0.09]));
                g.part(Matte).boxed(Vec3::new(q.x, 0.35, q.y), Vec3::new(0.08, 0.4, 0.24), k as f32, rgba([0.9, 0.9, 0.9]));
            }
        }
        // Bears by a den.
        4 => {
            g.part(Stone).cylinder_capped(w, r * 0.35, r * 0.15, 0.0, 2.4, 7, false, true, rgba(mul(pal.stone, 0.7)));
            for k in 0..2 {
                let q = n(k + 1);
                g.part(Matte).boxed(Vec3::new(q.x, 0.9, q.y), Vec3::new(2.0, 1.2, 1.1), k as f32 * 2.0, rgba([0.32, 0.2, 0.12]));
                let hd = q + Vec2::from_angle(k as f32 * 2.0) * 1.2;
                g.part(Matte).boxed(Vec3::new(hd.x, 1.2, hd.y), Vec3::new(0.7, 0.6, 0.6), k as f32 * 2.0, rgba([0.3, 0.19, 0.11]));
            }
        }
        // Flamingos in a pond.
        _ => {
            g.part(Matte).ground_polygon(&ring(r * 0.6), 0.08, rgba(WATER));
            for k in 0..8 {
                let q = w + Vec2::from_angle(k as f32 * 0.8 + rng.random_range(0.0..0.3)) * r * 0.4;
                let pink = rgba([0.95, 0.45, 0.55]);
                g.part(Matte).cylinder(q, 0.03, 0.03, 0.0, 0.9, 3, rgba([0.7, 0.35, 0.4]));
                g.part(Matte).boxed(Vec3::new(q.x, 1.05, q.y), Vec3::new(0.6, 0.3, 0.3), k as f32, pink);
                let hd = q + Vec2::from_angle(k as f32) * 0.25;
                g.part(Matte).tube(Vec3::new(q.x, 1.15, q.y), Vec3::new(hd.x, 1.7, hd.y), 0.04, 3, pink);
            }
        }
    }
}

fn elephant(g: &mut Geometry, p: Vec2, yaw: f32, scale: f32) {
    let grey = rgba([0.45, 0.44, 0.43]);
    let d = Vec2::from_angle(yaw);
    let s = d.perp();
    let k = scale.clamp(0.6, 1.0);
    g.part(Matte).boxed(Vec3::new(p.x, 2.3 * k, p.y), Vec3::new(3.2, 2.0, 1.9) * k, yaw, grey);
    let head = p + d * 2.0 * k;
    g.part(Matte).boxed(Vec3::new(head.x, 2.7 * k, head.y), Vec3::new(1.2, 1.4, 1.3) * k, yaw, grey);
    for e in [-1.0f32, 1.0] {
        let ear = head + s * e * 0.8 * k - d * 0.2;
        g.part(Matte).boxed(Vec3::new(ear.x, 2.8 * k, ear.y), Vec3::new(0.15, 1.2, 1.0) * k, yaw, grey);
    }
    let tip = head + d * 0.9 * k;
    g.part(Matte).tube(Vec3::new(head.x + d.x * 0.6 * k, 2.4 * k, head.y + d.y * 0.6 * k), Vec3::new(tip.x, 0.4, tip.y), 0.2 * k, 5, grey);
    for (a, b) in [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let l = p + d * a * 1.1 * k + s * b * 0.6 * k;
        g.part(Matte).cylinder(l, 0.32 * k, 0.32 * k, 0.0, 1.5 * k, 6, grey);
    }
}

fn giraffe(g: &mut Geometry, p: Vec2, yaw: f32) {
    let hide = rgba([0.84, 0.62, 0.3]);
    let d = Vec2::from_angle(yaw);
    let s = d.perp();
    g.part(Matte).boxed(Vec3::new(p.x, 2.2, p.y), Vec3::new(2.0, 1.1, 0.9), yaw, hide);
    let base = p + d * 0.9;
    let head = p + d * 1.6;
    g.part(Matte).tube(Vec3::new(base.x, 2.5, base.y), Vec3::new(head.x, 5.0, head.y), 0.2, 5, hide);
    g.part(Matte).boxed(Vec3::new(head.x + d.x * 0.3, 5.0, head.y + d.y * 0.3), Vec3::new(0.8, 0.35, 0.3), yaw, hide);
    for (a, b) in [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let l = p + d * a * 0.8 + s * b * 0.3;
        g.part(Matte).cylinder(l, 0.09, 0.09, 0.0, 1.7, 4, hide);
    }
    for k in 0..4 {
        let q = p + d * (k as f32 * 0.5 - 0.75);
        g.part(Matte).boxed(Vec3::new(q.x, 2.76, q.y), Vec3::new(0.3, 0.04, 0.6), yaw, rgba([0.45, 0.28, 0.12]));
    }
}

fn zebra(g: &mut Geometry, p: Vec2, yaw: f32) {
    let d = Vec2::from_angle(yaw);
    let s = d.perp();
    g.part(Matte).boxed(Vec3::new(p.x, 1.15, p.y), Vec3::new(1.6, 0.75, 0.6), yaw, rgba([0.9, 0.9, 0.88]));
    for k in 0..5 {
        let q = p + d * (k as f32 * 0.32 - 0.64);
        g.part(Matte).boxed(Vec3::new(q.x, 1.16, q.y), Vec3::new(0.1, 0.77, 0.62), yaw, rgba([0.06, 0.06, 0.06]));
    }
    let head = p + d * 1.0;
    g.part(Matte).tube(Vec3::new(head.x - d.x * 0.3, 1.3, head.y - d.y * 0.3), Vec3::new(head.x, 1.8, head.y), 0.15, 4, rgba([0.88, 0.88, 0.86]));
    for (a, b) in [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let l = p + d * a * 0.6 + s * b * 0.2;
        g.part(Matte).cylinder(l, 0.07, 0.07, 0.0, 0.8, 4, rgba([0.85, 0.85, 0.85]));
    }
}

fn lion(g: &mut Geometry, p: Vec2, yaw: f32) {
    let d = Vec2::from_angle(yaw);
    g.part(Matte).boxed(Vec3::new(p.x, 0.7, p.y), Vec3::new(1.7, 0.7, 0.7), yaw, rgba([0.78, 0.6, 0.32]));
    let head = p + d * 0.95;
    g.part(Matte).boxed(Vec3::new(head.x, 0.95, head.y), Vec3::new(0.7, 0.85, 0.9), yaw, rgba([0.5, 0.32, 0.12]));
    g.part(Matte).boxed(Vec3::new(head.x + d.x * 0.35, 0.9, head.y + d.y * 0.35), Vec3::new(0.35, 0.45, 0.45), yaw, rgba([0.78, 0.6, 0.32]));
}

/// An observatory: a terrace with a lit parapet, an art deco hall with a
/// great dome and two lesser ones, telescopes poking out of their slits,
/// telescopes on the terrace, a ringed planet on a plinth, and a green
/// beam from the main dome after dark.
pub fn observatory(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.stone, 1.0));
    let wall = offset_poly(&c.lot_poly, 1.2);
    for face in c.faces(&wall) {
        if face.len < 2.0 || face.out.y < -0.5 {
            continue;
        }
        face.bx(g, f, Stone, 0.0, 0.5, 0.0, Vec3::new(face.len, 1.0, 0.4), mul(pal.stone, 0.9));
        face.bx(g, f, Glow, 0.0, 1.05, 0.0, Vec3::new(face.len * 0.96, 0.08, 0.1), mul(NEON_VIOLET, 0.6));
    }
    let hall = share_rect(c, 0.5, 0.6, 0.32, 0.18);
    let h = 8.0;
    let white = [0.9, 0.89, 0.85];
    block(g, c, Plaster, &hall, 0.0, h, white);
    cornice(g, c, &hall, h - 0.6, 0.6, 0.3, Plaster, mul(white, 0.95));
    let front = c.front_face(&hall);
    entrance(g, c, rng, &front, 0.0, (4.0f32).min(front.len * 0.5), 4.5);
    for face in c.faces(&hall) {
        if face.len > 6.0 {
            face.decal(g, f, Glow, 0.0, 3.0, 0.06, 0.2, 5.0, mul(NEON_VIOLET, 0.7));
        }
    }
    let (lo, hi) = bounds(&hall);
    let mid = (lo + hi) * 0.5;
    let wide = (hi.x - lo.x).max(hi.y - lo.y);
    let axis = if hi.x - lo.x >= hi.y - lo.y { Vec2::X } else { Vec2::Y };
    let domes = [(mid, (wide * 0.2).clamp(4.0, 9.0), true), (mid - axis * wide * 0.36, (wide * 0.1).clamp(2.5, 4.5), false), (mid + axis * wide * 0.36, (wide * 0.1).clamp(2.5, 4.5), false)];
    for (k, (p, r, main)) in domes.iter().enumerate() {
        let Some(r) = fit_radius(c, *p, *r, 1.5) else { continue };
        let r = &r;
        let drum = if *main { 3.0 } else { 1.6 };
        g.part(Plaster).cylinder_capped(wv(c, *p), *r, *r, h, h + drum, 24, false, false, rgba(white));
        dome(g, c, *p, *r, h + drum, Metal, if *main { COPPER } else { SILVER });
        neon_ring(g, c, *p, *r + 0.06, h + drum, if k == 0 { NEON_GREEN } else { NEON_VIOLET });
        // The slit, open to the sky, and the telescope in it.
        let toward = Vec2::new(0.3, -1.0).normalize();
        for j in 0..6 {
            let e = j as f32 / 6.0 * FRAC_PI_2 * 0.95;
            let q = *p + toward * (*r * e.cos() + 0.05);
            g.part(Matte).boxed(w3(c, q, h + drum + *r * e.sin() + 0.05), Vec3::new(*r * 0.25, 0.12, *r * 0.3), f.yaw() + toward.to_angle(), rgba(DARK));
        }
        let base = w3(c, *p, h + drum + *r * 0.35);
        let tip = w3(c, *p + toward * *r * 1.05, h + drum + *r * 1.0);
        g.part(Metal).tube(base, tip, *r * 0.13, 10, rgba([0.2, 0.22, 0.25]));
        if *main {
            // The night beam, straight up from the crown.
            let crown = w3(c, *p, h + drum + *r);
            g.part(Glow).tube(crown, crown + Vec3::Y * 70.0, 0.12, 4, rgba(mul(NEON_GREEN, 0.8)));
        }
    }
    // Telescopes on the terrace and a ringed planet on a plinth.
    for k in 0..3 {
        let p = share_pt(c, 0.25 + k as f32 * 0.25, 0.16);
        if fits(c, p, 1.0, 0.3) {
            let w = wv(c, p);
            for a in 0..3 {
                let foot = w + Vec2::from_angle(a as f32 * TAU / 3.0) * 0.5;
                g.part(Metal).tube(Vec3::new(foot.x, 0.0, foot.y), Vec3::new(w.x, 1.3, w.y), 0.04, 3, rgba(DARK));
            }
            let dir = Vec2::from_angle(k as f32 + 0.5);
            g.part(Metal).tube(Vec3::new(w.x - dir.x * 0.6, 1.1, w.y - dir.y * 0.6), Vec3::new(w.x + dir.x * 0.6, 1.9, w.y + dir.y * 0.6), 0.14, 6, rgba(SILVER));
        }
    }
    let pl = share_pt(c, 0.12, 0.5);
    if fits(c, pl, 3.0, 0.4) {
        let w = wv(c, pl);
        g.part(Stone).cylinder_capped(w, 1.0, 1.0, 0.0, 1.6, 8, false, true, rgba(mul(pal.stone, 0.95)));
        let planet: Vec<(f32, f32)> = (0..=8).map(|k| { let a = (-FRAC_PI_2 + k as f32 / 8.0 * PI) * 0.94; (a.cos() * 1.4, 3.0 + a.sin() * 1.4) }).collect();
        g.part(Metal).lathe(w, &planet, 16, rgba([0.8, 0.6, 0.35]));
        let ring: Vec<Vec3> = (0..24).map(|k| { let a = k as f32 / 24.0 * TAU; Vec3::new(w.x + a.cos() * 2.4, 3.0 + a.sin() * 0.5, w.y + a.sin() * 2.4) }).collect();
        for k in 0..24 {
            g.part(Glow).tube(ring[k], ring[(k + 1) % 24], 0.06, 3, rgba(NEON_AMBER));
        }
    }
    front_lamps(g, c, 3);
}

/// An aquarium: a hall under a rolling wave of a roof, its glass front
/// lit blue by the tanks behind, a shark over the door, a touch pool and
/// a whale's tail fountain.
pub fn aquarium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mix(pal.paving, [0.7, 0.8, 0.85], 0.3));
    let hall = share_rect(c, 0.5, 0.64, 0.38, 0.26);
    let (lo, hi) = bounds(&hall);
    let n = 16;
    let wall = [0.82, 0.9, 0.92];
    let top = |i: usize| 8.0 + 3.0 * ((i as f32 / n as f32) * TAU * 1.0).sin() + 1.5 * ((i as f32 / n as f32) * TAU * 2.0 + 0.7).sin();
    let x = |i: usize| lo.x + (hi.x - lo.x) * i as f32 / n as f32;
    // Front (glass) and back walls, the roof between them, the ends.
    for i in 0..n {
        let (x0, x1) = (x(i), x(i + 1));
        let (t0, t1) = (top(i), top(i + 1));
        let fa = [w3(c, Vec2::new(x0, lo.y), 0.0), w3(c, Vec2::new(x1, lo.y), 0.0), w3(c, Vec2::new(x1, lo.y), t1), w3(c, Vec2::new(x0, lo.y), t0)];
        let out = c.frame.dir3(Vec2::new(0.0, -1.0));
        g.part(Glass).quad_facing(fa, out, rgba(pick(rng, &pal.glass)));
        let ba = [w3(c, Vec2::new(x0, hi.y), 0.0), w3(c, Vec2::new(x1, hi.y), 0.0), w3(c, Vec2::new(x1, hi.y), t1), w3(c, Vec2::new(x0, hi.y), t0)];
        g.part(Panel).quad_facing(ba, -out, rgba(wall));
        let roof = [w3(c, Vec2::new(x0, lo.y), t0), w3(c, Vec2::new(x1, lo.y), t1), w3(c, Vec2::new(x1, hi.y), t1), w3(c, Vec2::new(x0, hi.y), t0)];
        g.part(Panel).quad_facing(roof, Vec3::Y, rgba(mul(wall, 0.95)));
        // Tanks glowing behind the glass.
        let mid = Vec2::new((x0 + x1) * 0.5, lo.y - 0.08);
        if i % 2 == 0 && i > 0 && i + 1 < n {
            g.part(Glow).decal(w3(c, mid, t0.min(t1) * 0.42), c.frame.dir3(Vec2::X), Vec3::Y, (x1 - x0) * 1.6, t0.min(t1) * 0.6, out, rgba(if i % 4 == 0 { [0.1, 0.9, 2.2] } else { [0.25, 1.6, 2.0] }));
        }
        // The wave's lit crest.
        g.part(Glow).tube(fa[3] + Vec3::Y * 0.1, fa[2] + Vec3::Y * 0.1, 0.09, 4, rgba(NEON_CYAN));
    }
    for (xe, sgn) in [(lo.x, -1.0f32), (hi.x, 1.0)] {
        let i = if sgn < 0.0 { 0 } else { n };
        let q = [w3(c, Vec2::new(xe, lo.y), 0.0), w3(c, Vec2::new(xe, hi.y), 0.0), w3(c, Vec2::new(xe, hi.y), top(i)), w3(c, Vec2::new(xe, lo.y), top(i))];
        g.part(Panel).quad_facing(q, c.frame.dir3(Vec2::X * sgn), rgba(wall));
    }
    let front = Face::from_edge(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y));
    entrance(g, c, rng, &front, 0.0, (6.0f32).min(front.len * 0.5), 4.0);
    marquee(g, c, rng, &front, 0.0, 5.2, ((hi.x - lo.x) * 0.35).min(14.0), 1.0, NEON_CYAN);
    // A shark over the door.
    let sp = Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
    let sy = top(n / 2) + 2.5;
    let sw = wv(c, sp);
    let d = f.along;
    let grey = rgba([0.4, 0.46, 0.52]);
    let shark = ((hi.x - lo.x) * 0.7).min(10.0);
    let k_s = shark / 10.0;
    // A spindle body of tapering tubes, a dorsal fin, a forked tail.
    let body = |t: f32| -> (Vec3, f32) {
        let q = sw + d * (t - 0.45) * shark;
        let r = 1.15 * k_s * (1.0 - (2.0 * (t - 0.45)).abs().powf(1.8)).max(0.12);
        (Vec3::new(q.x, sy, q.y), r)
    };
    for k in 0..10 {
        let (a, ra) = body(k as f32 / 10.0);
        let (b, rb) = body((k + 1) as f32 / 10.0);
        g.part(Metal).tube(a, b, (ra + rb) * 0.5, 10, grey);
    }
    let d3 = Vec3::new(d.x, 0.0, d.y);
    let side3 = Vec3::new(-d.y, 0.0, d.x);
    let (fin0, _) = body(0.48);
    for s in [1.0f32, -1.0] {
        g.part(Metal).tri_facing(fin0 + Vec3::Y * 0.9 * k_s, fin0 + Vec3::Y * 0.9 * k_s - d3 * 2.2 * k_s, fin0 + Vec3::Y * 2.6 * k_s - d3 * 1.6 * k_s, side3 * s, grey);
    }
    let (tail0, _) = body(0.0);
    for s in [1.0f32, -1.0] {
        for up in [1.0f32, -1.0] {
            g.part(Metal).tri_facing(tail0, tail0 - d3 * 1.6 * k_s + Vec3::Y * 1.9 * k_s * up, tail0 - d3 * 0.6 * k_s + Vec3::Y * 0.4 * up, side3 * s, grey);
        }
    }
    g.part(Glow).boxed(body(0.86).0 + Vec3::Y * 0.3 * k_s + side3 * 0.7 * k_s, Vec3::splat(0.2), 0.0, rgba(NEON_CYAN));
    g.part(Glow).boxed(body(0.86).0 + Vec3::Y * 0.3 * k_s - side3 * 0.7 * k_s, Vec3::splat(0.2), 0.0, rgba(NEON_CYAN));
    g.part(Metal).tube(Vec3::new(sw.x, sy - 1.2, sw.y), Vec3::new(sw.x, top(n / 2), sw.y), 0.12, 4, rgba(DARK));
    // The touch pool and a whale's tail.
    let pool = share_pt(c, 0.25, 0.16);
    if let Some(r) = fit_radius(c, pool, 4.5, 2.0) {
        let ring: Vec<Vec2> = (0..20).map(|k| wv(c, pool) + Vec2::from_angle(k as f32 / 20.0 * TAU) * r).collect();
        g.part(Concrete).cylinder_capped(wv(c, pool), r + 0.3, r + 0.3, 0.0, 0.5, 20, false, false, rgba(mul(pal.concrete(0), 1.1)));
        g.part(Matte).ground_polygon(&ring, 0.42, rgba([0.1, 0.45, 0.55]));
        neon_ring(g, c, pool, r + 0.32, 0.5, NEON_CYAN);
    }
    let tail_at = share_pt(c, 0.78, 0.16);
    if fits(c, tail_at, 2.5, 0.3) {
        let w = wv(c, tail_at);
        for s in [-1.0f32, 1.0] {
            let a = Vec3::new(w.x, 0.3, w.y);
            let b = Vec3::new(w.x + d.x * s * 2.2, 3.6, w.y + d.y * s * 2.2);
            g.part(Metal).beam(a, b, 0.6, grey);
        }
        g.part(Glow).cylinder(w, 1.6, 1.6, 0.02, 0.12, 14, rgba(NEON_CYAN));
    }
    front_lamps(g, c, 4);
}

/// A botanical garden: the gardens (the sim's plan: beds, pools, a
/// fountain, paths) round a Victorian conservatory of glass, a domed
/// palm house with two vaulted wings, lit warm from within.
pub fn botanical_garden(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    fill_site(g, c, Foliage, 0.02, mul(pal.foliage, 1.0));
    super::park::draw(g, c, rng);
    let centre = nd_sim::geom::polygon_centroid(&c.lot_poly);
    let r = (inradius(&c.lot_poly) * 0.55).clamp(8.0, 18.0);
    let glass = mix(pick(rng, &pal.glass), [0.7, 0.85, 0.8], 0.4);
    let white = rgba([0.92, 0.92, 0.9]);
    let core = r * 0.5;
    if let Some(core) = fit_radius(c, centre, core, 3.0) {
        let w = wv(c, centre);
        g.part(Stone).cylinder_capped(w, core + 0.4, core + 0.4, 0.0, 0.6, 12, false, true, rgba(mul(pal.stone, 1.05)));
        g.part(Glass).cylinder_capped(w, core, core, 0.6, 6.5, 12, false, false, rgba(glass));
        dome(g, c, centre, core, 6.5, Glass, glass);
        for k in 0..12 {
            let a = k as f32 / 12.0 * TAU;
            let q = w + Vec2::from_angle(a) * core;
            g.part(Metal).cylinder(q, 0.1, 0.1, 0.6, 6.5, 4, white);
            let mut prev = Vec3::new(q.x, 6.5, q.y);
            for j in 1..=5 {
                let e = j as f32 / 5.0 * FRAC_PI_2;
                let p = w + Vec2::from_angle(a) * core * e.cos();
                let nx = Vec3::new(p.x, 6.5 + core * e.sin(), p.y);
                g.part(Metal).tube(prev, nx, 0.08, 3, white);
                prev = nx;
            }
        }
        g.part(Glow).cylinder(w, core * 0.9, core * 0.9, 6.4, 6.6, 12, rgba(mul(WARM, 0.9)));
        tree_kind(g, w, 0.6, TreeKind::Palm, 9.0, 2.6, pal.bark, pal.foliage);
        // The wings: vaults either side along the lot's width.
        for s in [-1.0f32, 1.0] {
            let along = Vec2::X * s;
            let len = (r * 0.95).min(distance_to_boundary(centre, &c.lot_poly) - core - 1.0);
            if len < 4.0 {
                continue;
            }
            let half = core * 0.55;
            let a0 = centre + along * (core * 0.8);
            let steps = 8;
            let ribs = ((len / 2.5).ceil() as usize).max(2);
            let arc = |t: f32, k: usize| -> Vec3 {
                let e = k as f32 / steps as f32 * PI;
                let p = a0 + along * t * len + along.perp() * (e.cos() * half);
                w3(c, p, 4.0 + e.sin() * half * 0.8)
            };
            let wall_a: Vec<Vec2> = vec![a0 - along.perp() * half, a0 + along * len - along.perp() * half, a0 + along * len + along.perp() * half, a0 + along.perp() * half];
            if !c.all_inside(&wall_a) {
                continue;
            }
            extrude(g, &c.frame, Glass, &wall_a, 0.0, 4.0, glass);
            for i in 0..ribs {
                let (t0, t1) = (i as f32 / ribs as f32, (i + 1) as f32 / ribs as f32);
                for k in 0..steps {
                    g.part(Glass).quad_facing([arc(t0, k), arc(t1, k), arc(t1, k + 1), arc(t0, k + 1)], Vec3::Y, rgba(glass));
                }
                for k in 0..steps {
                    g.part(Metal).tube(arc(t0, k), arc(t0, k + 1), 0.06, 3, white);
                }
            }
            let end = a0 + along * len * 0.6;
            tree_in(g, &c.frame, pal, end, 4.0, 1.6, mul(pal.foliage, 1.2));
            g.part(Glow).boxed(w3(c, end, 0.4), Vec3::new(1.5, 0.2, 1.5), 0.0, rgba(WARM));
        }
    }
}

/// An opera and concert hall: a podium of steps, a glass foyer, and white
/// sails rising over it, their edges lit at night; a marquee on the
/// podium.
pub fn concert_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.paving, 1.08));
    let podium = share_rect(c, 0.5, 0.58, 0.42, 0.32);
    let ph = 3.0;
    plinth(g, c, &podium, 3, 0.25, 0.9, Stone, mul(pal.stone, 1.1));
    block(g, c, Stone, &podium, 0.0, ph, mul(pal.stone, 1.15));
    let pfront = c.front_face(&podium);
    pfront.decal(g, f, Glow, 0.0, ph - 0.4, 0.05, pfront.len * 0.9, 0.12, NEON_AMBER);
    led_screen(g, c, rng, &pfront, pfront.len * 0.28, ph * 0.55, (pfront.len * 0.22).min(8.0), ph * 0.6);
    let (lo, hi) = bounds(&podium);
    let along_u = hi.x - lo.x >= hi.y - lo.y;
    let long = if along_u { hi.x - lo.x } else { hi.y - lo.y };
    let short = if along_u { hi.y - lo.y } else { hi.x - lo.x };
    let axis = if along_u { Vec2::X } else { Vec2::Y };
    let mid = (lo + hi) * 0.5;
    // The foyer: low glass under the shells, lit warm.
    let foyer = fit_inside(c, &rect(mid.x, mid.y, if along_u { long * 0.4 } else { short * 0.2 }, if along_u { short * 0.2 } else { long * 0.4 }));
    extrude(g, f, Glass, &foyer, ph, ph + 4.0, pick(rng, &pal.glass));
    ground(g, f, Stone, &foyer, ph + 4.0, mul(pal.stone, 1.1));
    for face in c.faces(&foyer) {
        if face.len > 3.0 {
            face.decal(g, f, Glow, 0.0, ph + 1.6, 0.05, face.len * 0.9, 2.2, mul(WARM, 0.55));
        }
    }
    // The sails: shells like slices of a sphere standing on their corners,
    // in a row along the podium, all opening the same way, stepping down
    // in size; white outside, warm wood within, their rims lit at night.
    let fwd = axis;
    let side = fwd.perp();
    let sails = 3;
    for k in 0..sails {
        let scale = 1.0 - 0.2 * k as f32;
        let mut bw = short * 0.46 * scale;
        let bd = long * 0.16 * scale;
        let hgt = (short * 0.8 * scale).clamp(8.0, 30.0);
        let c0 = mid + fwd * (long * 0.3 - k as f32 * long * 0.27);
        // Narrowed until its corners and its bulge stand on the lot.
        while bw > 2.0 && ![c0 + side * bw, c0 - side * bw, c0 - fwd * (bd * 1.5), c0 + fwd * (bd * 0.5)].iter().all(|p| fits(c, *p, 0.3, 0.4)) {
            bw *= 0.85;
        }
        if bw <= 2.0 || bd < 1.0 {
            continue;
        }
        shell(g, c, c0, fwd, side, bw, bd, hgt, ph + 4.0, [NEON_AMBER, NEON_PINK, NEON_CYAN][k % 3]);
        // A smaller shell back to back, as at the harbour.
        if k == 0 {
            shell(g, c, c0 - fwd * bd * 0.9, -fwd, side, bw * 0.55, bd * 0.6, hgt * 0.5, ph + 4.0, NEON_VIOLET);
        }
    }
    front_lamps(g, c, 5);
}

/// One sail of the opera house: a curved shell from two corners on the
/// podium (`c0 -/+ side * bw`) up to an apex leaning forward, bulging back
/// by `bd`; its rim lit in `neon`. Lot-local centre and directions.
#[allow(clippy::too_many_arguments)]
fn shell(g: &mut Geometry, c: &Ctx, c0: Vec2, fwd: Vec2, side: Vec2, bw: f32, bd: f32, h: f32, y0: f32, neon: [f32; 3]) {
    let (nu, nv) = (12usize, 9usize);
    let apex = c0 + fwd * (bd * 0.35);
    let at = |u: f32, v: f32| -> Vec3 {
        let bulge = (PI * u).sin();
        let base = c0 + side * (bw * (2.0 * u - 1.0)) - fwd * (bd * bulge);
        let v = v.min(0.985);
        let q = base.lerp(apex, v) - fwd * (bd * 0.5 * bulge * (PI * v).sin());
        // The outline narrows towards the apex in a curve, not a cone.
        let y = y0 + h * (1.0 - (1.0 - v).powf(1.35));
        w3(c, q, y)
    };
    let mid = w3(c, c0 - fwd * bd, y0 + h * 0.4);
    for i in 0..nu {
        for j in 0..nv {
            let (u0, u1) = (i as f32 / nu as f32, (i + 1) as f32 / nu as f32);
            let (v0, v1) = (j as f32 / nv as f32, (j + 1) as f32 / nv as f32);
            let q = [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)];
            let centre = (q[0] + q[1] + q[2] + q[3]) * 0.25;
            let back = c.frame.dir3(-fwd);
            let out = (centre - mid).normalize_or_zero() * 0.3 + back + Vec3::Y * 0.4;
            g.part(Panel).quad_facing(q, out, rgba(mul(SAIL_WHITE, 0.94 + 0.06 * (j % 2) as f32)));
            g.part(Wood).quad_facing(q, -out, rgba([0.62, 0.42, 0.28]));
        }
    }
    // The rim: up one side, over the apex, down the other.
    let rim: Vec<Vec3> = (0..=nv).map(|j| at(0.0, j as f32 / nv as f32)).chain((0..=nv).rev().map(|j| at(1.0, j as f32 / nv as f32))).collect();
    for w in rim.windows(2) {
        g.part(Glow).tube(w[0], w[1], 0.09, 4, rgba(neon));
    }
    // Uplights at its foot.
    for u in [0.25f32, 0.5, 0.75] {
        let p = at(u, 0.0);
        g.part(Glow).boxed(p + Vec3::Y * 0.15, Vec3::splat(0.35), 0.0, rgba(WARM));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ferris_wheel_and_coaster_draw_something() {
        let pal = crate::palette::default_palette();
        let mut g = Geometry::default();
        ferris_wheel(&mut g, pal, Vec2::ZERO, 0.0, Vec2::X, 10.0);
        coaster(&mut g, Vec2::new(20.0, 0.0), 0.0, Vec2::X, 8.0, 12.0);
        assert!(g.triangle_count() > 500);
    }
}

// ── (D4) the second league: amusement park, convention centre, cathedral,
// casino resort ─────────────────────────────────────────────────────────────

/// A drop tower: a lattice mast, a ring of seats part way up, a lit crown
/// (world).
fn drop_tower(g: &mut Geometry, at: Vec2, y0: f32, h: f32) {
    let steel = [0.75, 0.76, 0.8];
    for k in 0..4 {
        let o = Vec2::from_angle(k as f32 * FRAC_PI_2 + PI / 4.0) * 1.1;
        g.part(Metal).cylinder(at + o, 0.18, 0.14, y0, y0 + h, 6, rgba(steel));
    }
    let mut y = y0 + 3.0;
    while y < y0 + h - 1.0 {
        g.part(Metal).cylinder(at, 1.25, 1.25, y, y + 0.12, 4, rgba(steel));
        y += 3.0;
    }
    let seats = y0 + h * 0.62;
    g.part(Paint).cylinder_capped(at, 2.6, 2.6, seats, seats + 1.1, 16, true, true, rgba([0.95, 0.3, 0.2]));
    g.part(Glow).cylinder(at, 2.65, 2.65, seats + 1.1, seats + 1.25, 16, rgba(NEON_AMBER));
    g.part(Matte).cylinder_capped(at, 1.6, 1.0, y0 + h, y0 + h + 1.6, 8, false, true, rgba(DARK));
    g.part(Glow).cylinder(at, 1.65, 1.65, y0 + h + 0.6, y0 + h + 0.9, 12, rgba(NEON_PINK));
    g.part(Glow).boxed(Vec3::new(at.x, y0 + h + 2.0, at.y), Vec3::splat(0.5), 0.0, rgba(NEON_RED));
}

/// A swing ride: a mast, a tilted crown and chairs hanging off it (world).
fn swing_ride(g: &mut Geometry, pal: &Palette, at: Vec2, y0: f32, r: f32) {
    g.part(Metal).cylinder(at, 0.5, 0.4, y0, y0 + 9.0, 8, rgba(SILVER));
    g.part(Tarp).cylinder_capped(at, r * 0.55, 0.3, y0 + 8.5, y0 + 10.5, 16, true, false, rgba(pal.awning(5)));
    for k in 0..12 {
        let a = k as f32 / 12.0 * TAU;
        let top = at + Vec2::from_angle(a) * r * 0.5;
        let seat = at + Vec2::from_angle(a) * r;
        g.part(Metal).tube(Vec3::new(top.x, y0 + 8.5, top.y), Vec3::new(seat.x, y0 + 4.2, seat.y), 0.04, 3, rgba(SILVER));
        g.part(Paint).boxed(Vec3::new(seat.x, y0 + 4.0, seat.y), Vec3::new(0.6, 0.3, 0.6), a, rgba(pal.awning(k)));
    }
    let ring: Vec<Vec3> = (0..20).map(|k| { let p = at + Vec2::from_angle(k as f32 / 20.0 * TAU) * r * 0.55; Vec3::new(p.x, y0 + 8.5, p.y) }).collect();
    for k in 0..20 {
        g.part(Glow).tube(ring[k], ring[(k + 1) % 20], 0.06, 3, rgba(NEON_VIOLET));
    }
}

/// An amusement park: the midway (the sim's loop and benches), a lit gate,
/// and its rides in the clearest ground: a coaster, a ferris wheel, a drop
/// tower, a swing ride, a carousel and food stands along the way.
pub fn amusement_park(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    let f = &c.frame;
    fill_site(g, c, Paving, 0.03, mix(pal.paving, [0.62, 0.55, 0.5], 0.35));
    let plan = c.park.as_ref();
    if let Some(plan) = plan {
        for path in &plan.paths {
            g.part(Paving).ground_strip(path, 4.0, 0.06, rgba(mul(pal.paving, 1.15)));
            // Strings of light along the midway.
            for s in path.windows(2) {
                g.part(Glow).tube(Vec3::new(s[0].x, 4.2, s[0].y), Vec3::new(s[1].x, 4.2, s[1].y), 0.04, 3, rgba(mul(NEON_AMBER, 0.8)));
            }
        }
        for (k, (p, yaw)) in plan.benches.iter().enumerate() {
            bench_world(g, *p, *yaw + FRAC_PI_2, pal.wood, DARK);
            if k % 2 == 0 {
                lamp_post_world(g, *p + Vec2::from_angle(*yaw) * -1.2, 4.4, pal.street_lamp);
            }
        }
        if let Some(path) = plan.paths.iter().find(|p| p.len() >= 2) {
            let along = (path[1] - path[0]).normalize_or_zero();
            gate_arch_in(g, c, path[0] + along * 1.5, along, 8.0, 7.5, [0.95, 0.85, 0.3], NEON_PINK);
        }
    }
    // A fence round it all.
    for face in c.faces(&offset_poly(&c.lot_poly, 0.6)) {
        if face.len > 1.0 {
            face.bx(g, f, Metal, 0.0, 0.9, 0.0, Vec3::new(face.len, 1.8, 0.06), mul(pal.fence_mesh, 0.9));
        }
    }
    let mut taken: Vec<(Vec2, f32)> = Vec::new();
    let dir = f.along;
    // The coaster takes the most room.
    for r in [16.0f32, 12.0, 9.0] {
        if let Some(p) = free_spots(c, plan, r, &taken, 1).into_iter().next() {
            coaster(g, wv(c, p), 0.0, dir, r * 1.4, r * 1.9);
            taken.push((p, r));
            break;
        }
    }
    for r in [11.0f32, 8.0] {
        if let Some(p) = free_spots(c, plan, r * 0.6, &taken, 1).into_iter().next() {
            ferris_wheel(g, pal, wv(c, p), 0.0, dir.perp(), r);
            taken.push((p, r * 0.6));
            break;
        }
    }
    if let Some(p) = free_spots(c, plan, 4.0, &taken, 1).into_iter().next() {
        drop_tower(g, wv(c, p), 0.0, 28.0);
        taken.push((p, 4.0));
    }
    if let Some(p) = free_spots(c, plan, 6.5, &taken, 1).into_iter().next() {
        swing_ride(g, pal, wv(c, p), 0.0, 6.0);
        taken.push((p, 6.5));
    }
    if let Some(p) = free_spots(c, plan, 5.0, &taken, 1).into_iter().next() {
        carousel(g, pal, wv(c, p), 0.0, 4.5);
        taken.push((p, 5.0));
    }
    // Food stands beside the midway.
    if let Some(plan) = plan {
        let mut k = 0;
        for path in plan.paths.iter().take(1) {
            for s in path.windows(2).step_by(3) {
                let d = (s[1] - s[0]).normalize_or_zero();
                let at = s[0].lerp(s[1], 0.5) + d.perp() * 4.5;
                if fits(c, f.to_local(at), 2.2, 0.5) && taken.iter().all(|(q, r)| q.distance(f.to_local(at)) > r + 3.0) {
                    food_stand(g, pal, at, 0.0, -d.perp(), k);
                    k += 1;
                }
                if k >= 6 {
                    break;
                }
            }
        }
    }
    for p in free_spots(c, plan, 2.5, &taken, 10) {
        tree_in(g, f, pal, p, rng.random_range(5.0..7.0), rng.random_range(1.6..2.2), mul(pal.foliage, rng.random_range(0.9..1.1)));
    }
}

/// A convention centre: a long hall under a shallow barrel roof on steel
/// trusses, a full-height glass front lit warm, a canopy along the plaza,
/// flag poles and a big LED wall.
pub fn convention_centre(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.paving, 1.12));
    let hall = share_rect(c, 0.5, 0.62, 0.44, 0.3);
    if hall.len() < 3 {
        return;
    }
    let (lo, hi) = bounds(&hall);
    let h = 13.0;
    let wall = [0.78, 0.8, 0.84];
    // The back and the ends, panelled; the front all glass.
    extrude(g, f, Panel, &hall, 0.0, h, wall);
    let front = Face::from_edge(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y));
    let glass = pick(rng, &pal.glass);
    front.bx(g, f, Glass, 0.0, h * 0.5, 0.05, Vec3::new(front.len, h - 0.4, 0.1), glass);
    for k in 0..((front.len / 6.0) as i32) {
        let t = -front.len * 0.5 + 3.0 + k as f32 * 6.0;
        front.bx(g, f, Metal, t, h * 0.5, 0.15, Vec3::new(0.25, h, 0.25), SILVER);
    }
    front.decal(g, f, Glow, 0.0, h * 0.42, 0.18, front.len * 0.94, h * 0.6, mul(WARM, 0.45));
    // A barrel roof over the hall: ribs across, sheeting between.
    let n = 10;
    let span = hi.y - lo.y;
    let arch = |t: f32| h + (span * 0.12).min(5.0) * (1.0 - (2.0 * t - 1.0).powi(2));
    for i in 0..n {
        let (v0, v1) = (lo.y + span * i as f32 / n as f32, lo.y + span * (i + 1) as f32 / n as f32);
        let (y0, y1) = (arch(i as f32 / n as f32), arch((i + 1) as f32 / n as f32));
        let q = [w3(c, Vec2::new(lo.x, v0), y0), w3(c, Vec2::new(lo.x, v1), y1), w3(c, Vec2::new(hi.x, v1), y1), w3(c, Vec2::new(hi.x, v0), y0)];
        g.part(Metal).quad_facing(q, Vec3::Y, rgba(mul(SILVER, 0.9)));
    }
    let ribs = ((hi.x - lo.x) / 8.0).ceil() as i32;
    for k in 0..=ribs {
        let u = lo.x + (hi.x - lo.x) * k as f32 / ribs as f32;
        let mut prev = w3(c, Vec2::new(u, lo.y), arch(0.0) + 0.3);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let p = w3(c, Vec2::new(u, lo.y + span * t), arch(t) + 0.3);
            g.part(Metal).tube(prev, p, 0.18, 4, rgba(DARK));
            prev = p;
        }
    }
    // The roof's lit edge over the front.
    g.part(Glow).tube(w3(c, Vec2::new(lo.x, lo.y - 0.2), h + 0.1), w3(c, Vec2::new(hi.x, lo.y - 0.2), h + 0.1), 0.12, 4, rgba(NEON_CYAN));
    // The canopy along the plaza and its columns.
    let canopy_d = ((lo.y - bounds(&c.poly).0.y) * 0.6).clamp(3.0, 9.0);
    front.bx(g, f, Metal, 0.0, 5.2, canopy_d * 0.5, Vec3::new(front.len * 0.8, 0.4, canopy_d), mul(SILVER, 0.95));
    front.decal(g, f, Glow, 0.0, 5.0, canopy_d + 0.05, front.len * 0.78, 0.15, NEON_CYAN);
    for k in 0..5 {
        let t = (k as f32 / 4.0 - 0.5) * front.len * 0.75;
        let p = front.at(t, canopy_d - 0.6);
        g.part(Metal).cylinder(wv(c, p), 0.22, 0.22, 0.0, 5.0, 6, rgba(SILVER));
    }
    entrance(g, c, rng, &front, 0.0, (10.0f32).min(front.len * 0.4), 4.5);
    marquee(g, c, rng, &front, 0.0, h + 1.6, (front.len * 0.45).min(26.0), 1.6, NEON_CYAN);
    led_screen(g, c, rng, &front, front.len * 0.32, h * 0.6, (front.len * 0.18).min(10.0), h * 0.4);
    // Flag poles across the plaza.
    let flags = [NEON_PINK, NEON_AMBER, NEON_GREEN, NEON_VIOLET, NEON_CYAN, NEON_RED];
    for k in 0..6 {
        let t = (k as f32 / 5.0 - 0.5) * front.len * 0.85;
        let p = front.at(t, canopy_d + 3.0);
        if !fits(c, p, 0.5, 0.4) {
            continue;
        }
        let w = wv(c, p);
        g.part(Metal).cylinder(w, 0.08, 0.06, 0.0, 9.0, 5, rgba(SILVER));
        g.part(Tarp).boxed(Vec3::new(w.x, 8.2, w.y) + f.dir3(Vec2::X) * 0.8, Vec3::new(1.5, 1.0, 0.03), f.yaw(), rgba(mul(flags[k], 0.35)));
    }
    front_lamps(g, c, 5);
}

/// A cathedral: a stone nave with buttresses down its sides, twin west
/// towers with spires at the front, a lantern over the crossing, a rose
/// window that glows at night, and a square before the doors.
pub fn cathedral(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mul(pal.stone, 0.95));
    let stone = mix(pal.stone, [0.78, 0.74, 0.66], 0.5);
    let (blo, bhi) = bounds(&c.poly);
    // The nave runs back from the front, across the middle of the site.
    let nave = share_rect(c, 0.5, 0.6, 0.16, 0.34);
    if nave.len() < 3 {
        return;
    }
    let (lo, hi) = bounds(&nave);
    let wall_h = 16.0;
    block(g, c, Stone, &nave, 0.0, wall_h, stone);
    // A pitched roof of dark lead along the nave.
    let ridge = wall_h + (hi.x - lo.x) * 0.55;
    let mid_u = (lo.x + hi.x) * 0.5;
    for (u0, side) in [(lo.x, -1.0f32), (hi.x, 1.0)] {
        let q = [w3(c, Vec2::new(u0, lo.y), wall_h), w3(c, Vec2::new(u0, hi.y), wall_h), w3(c, Vec2::new(mid_u, hi.y), ridge), w3(c, Vec2::new(mid_u, lo.y), ridge)];
        let n = Vec3::new(f.dir3(Vec2::X).x * side, 0.8, f.dir3(Vec2::X).z * side).normalize();
        g.part(Metal).quad_facing(q, n, rgba([0.28, 0.32, 0.36]));
    }
    for v0 in [lo.y, hi.y] {
        let out = if v0 == lo.y { -1.0 } else { 1.0 };
        g.part(Stone).tri_facing(w3(c, Vec2::new(lo.x, v0), wall_h), w3(c, Vec2::new(hi.x, v0), wall_h), w3(c, Vec2::new(mid_u, v0), ridge), f.dir3(Vec2::Y * out), rgba(stone));
    }
    // Transepts across, two thirds of the way back.
    let cross_v = lo.y + (hi.y - lo.y) * 0.62;
    let tw = (hi.y - lo.y) * 0.1;
    let transept = fit_inside(c, &rect(mid_u, cross_v, (hi.x - lo.x) * 1.15, tw));
    block(g, c, Stone, &transept, 0.0, wall_h * 0.9, stone);
    // The lantern over the crossing.
    let crossing = Vec2::new(mid_u, cross_v);
    g.part(Stone).cylinder_capped(wv(c, crossing), 3.2, 3.2, ridge - 1.0, ridge + 6.0, 8, false, true, rgba(stone));
    g.part(Glow).cylinder(wv(c, crossing), 3.25, 3.25, ridge + 2.0, ridge + 4.0, 8, rgba(mul(WARM, 0.9)));
    g.part(Metal).cylinder_capped(wv(c, crossing), 3.0, 0.05, ridge + 6.0, ridge + 14.0, 8, false, false, rgba([0.3, 0.36, 0.4]));
    // Buttresses down both sides, tall windows glowing between them.
    let bays = ((hi.y - lo.y) / 6.0).floor().max(3.0) as i32;
    for k in 0..bays {
        let v = lo.y + (k as f32 + 0.5) * (hi.y - lo.y) / bays as f32;
        for (u, s) in [(lo.x, -1.0f32), (hi.x, 1.0)] {
            let p = Vec2::new(u + s * 1.2, v);
            if fits(c, p, 0.6, 0.2) {
                f.bx(g, Stone, p.x, wall_h * 0.4, p.y, Vec3::new(2.4, wall_h * 0.8, 1.0), mul(stone, 0.9));
            }
            let side = Face::new(Vec2::new(u, v - 2.0), Vec2::new(u, v + 2.0), Vec2::X * s);
            side.decal(g, f, Glow, 0.0, wall_h * 0.5, 0.06, 1.6, wall_h * 0.55, [0.6, 0.35, 1.1]);
        }
    }
    // The west front: twin towers and their spires, the rose window and
    // the doors.
    let tower_w = ((hi.x - lo.x) * 0.38).clamp(4.0, 9.0);
    let th = wall_h + 18.0;
    for s in [-1.0f32, 1.0] {
        let cu = mid_u + s * ((hi.x - lo.x) * 0.5 + tower_w * 0.35);
        let t = fit_inside(c, &rect(cu, lo.y + tower_w * 0.5, tower_w * 0.5, tower_w * 0.5));
        if t.len() < 3 {
            continue;
        }
        block(g, c, Stone, &t, 0.0, th, stone);
        let (tl, thi) = bounds(&t);
        let tc = (tl + thi) * 0.5;
        let r = (thi.x - tl.x).min(thi.y - tl.y) * 0.55;
        g.part(Stone).cylinder_capped(wv(c, tc), r, 0.15, th, th + 16.0, 8, false, false, rgba(mul(stone, 0.95)));
        g.part(Glow).cylinder(wv(c, tc), r + 0.05, r + 0.05, th - 0.4, th - 0.1, 8, rgba(NEON_AMBER));
        // Belfry openings.
        for face in c.faces(&t) {
            if face.len > 2.0 {
                face.decal(g, f, Glow, 0.0, th - 4.0, 0.06, face.len * 0.35, 3.5, mul(WARM, 0.7));
            }
        }
        g.part(Metal).cylinder(wv(c, tc), 0.08, 0.08, th + 16.0, th + 18.0, 4, rgba([0.85, 0.75, 0.4]));
    }
    let west = Face::from_edge(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y));
    let rose_y = wall_h * 0.66;
    let rose_r = ((hi.x - lo.x) * 0.28).clamp(1.5, 4.0);
    let rose = wv(c, west.at(0.0, 0.08));
    let out3 = f.dir3(west.out);
    let side3 = f.dir3(west.tangent);
    let centre = Vec3::new(rose.x, rose_y, rose.y);
    let colors = [NEON_VIOLET, NEON_CYAN, NEON_PINK, NEON_AMBER];
    for k in 0..16 {
        let a0 = k as f32 / 16.0 * TAU;
        let a1 = (k + 1) as f32 / 16.0 * TAU;
        let p0 = centre + (side3 * a0.cos() + Vec3::Y * a0.sin()) * rose_r;
        let p1 = centre + (side3 * a1.cos() + Vec3::Y * a1.sin()) * rose_r;
        g.part(Glow).tri_facing(centre + out3 * 0.02, p0, p1, out3, rgba(mul(colors[k % 4], 0.55)));
        g.part(Stone).tube(centre, p0, 0.08, 3, rgba(stone));
        g.part(Stone).tube(p0, p1, 0.12, 3, rgba(stone));
    }
    entrance(g, c, rng, &west, 0.0, ((hi.x - lo.x) * 0.4).min(5.0), 6.0);
    west.decal(g, f, Glow, 0.0, 6.4, 0.1, ((hi.x - lo.x) * 0.42).min(5.4), 0.15, NEON_AMBER);
    // The square: a fountain and lamps.
    let sq = Vec2::new(mid_u, (blo.y + lo.y) * 0.5);
    if let Some(r) = fit_radius(c, sq, 3.0, 1.5) {
        if sq.y < lo.y - r - 1.0 {
            g.part(Stone).cylinder_capped(wv(c, sq), r, r, 0.0, 0.6, 16, false, true, rgba(stone));
            g.part(Matte).ground_polygon(&(0..16).map(|k| wv(c, sq) + Vec2::from_angle(k as f32 / 16.0 * TAU) * (r - 0.3)).collect::<Vec<_>>(), 0.62, rgba(WATER));
            neon_ring(g, c, sq, r + 0.05, 0.6, NEON_CYAN);
        }
    }
    let _ = bhi;
    front_lamps(g, c, 4);
}

/// A casino resort: a curved hotel tower of glass over a podium of neon,
/// the casino floor behind a lit facade, a porte-cochère, a pool deck with
/// palms and a tall sign.
pub fn casino_resort(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    forecourt(g, c, mix(pal.paving, [0.3, 0.25, 0.32], 0.5));
    let podium = share_rect(c, 0.5, 0.55, 0.44, 0.3);
    if podium.len() < 3 {
        return;
    }
    let (lo, hi) = bounds(&podium);
    let ph = 9.0;
    block(g, c, Panel, &podium, 0.0, ph, [0.16, 0.12, 0.2]);
    let front = Face::from_edge(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y));
    // The facade: bands of neon up the podium.
    let bands = [NEON_PINK, NEON_AMBER, NEON_VIOLET];
    for (k, col) in bands.iter().enumerate() {
        front.decal(g, f, Glow, 0.0, 2.0 + k as f32 * 2.6, 0.06, front.len * 0.96, 0.22, *col);
    }
    for face in c.faces(&podium) {
        if face.len > 4.0 && !face.is_front {
            face.decal(g, f, Glow, 0.0, ph - 0.6, 0.05, face.len * 0.9, 0.18, NEON_PINK);
        }
    }
    entrance(g, c, rng, &front, -front.len * 0.15, (8.0f32).min(front.len * 0.3), 4.5);
    marquee(g, c, rng, &front, front.len * 0.18, ph - 2.6, (front.len * 0.4).min(20.0), 2.0, NEON_AMBER);
    // The porte-cochère over the drive.
    let pc = front.at(-front.len * 0.15, 4.5);
    f.bx(g, Metal, pc.x, 5.2, pc.y, Vec3::new(12.0f32.min(front.len * 0.4), 0.5, 7.0), [0.85, 0.7, 0.3]);
    for s in [-1.0f32, 1.0] {
        let p = front.at(-front.len * 0.15 + s * 5.0f32.min(front.len * 0.17), 7.5);
        if fits(c, p, 0.4, 0.2) {
            g.part(Metal).cylinder(wv(c, p), 0.3, 0.3, 0.0, 5.0, 6, rgba([0.85, 0.7, 0.3]));
        }
    }
    // The hotel tower: a crescent of glass on the podium's back half.
    let (pu, pv) = ((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.65);
    let rw = (hi.x - lo.x) * 0.36;
    let rd = ((hi.y - lo.y) * 0.22).max(4.0);
    let th = 58.0;
    let glass = pick(rng, &pal.glass);
    let arc: Vec<Vec2> = (0..=12).map(|k| { let a = PI * (k as f32 / 12.0); Vec2::new(pu - a.cos() * rw, pv + a.sin() * rd) }).collect();
    let inner: Vec<Vec2> = arc.iter().rev().map(|p| Vec2::new(p.x, p.y + ((p.x - pu) / rw).powi(2).mul_add(-rd * 0.6, rd * 0.6) - 6.0)).collect();
    let mut tower: Vec<Vec2> = arc.clone();
    tower.extend(inner);
    let tower = fit_inside(c, &tower);
    if tower.len() >= 3 {
        extrude(g, f, Glass, &tower, ph, ph + th, glass);
        ground(g, f, Panel, &tower, ph + th, [0.2, 0.18, 0.24]);
        // Floors lit warm, a crown of light.
        for face in c.faces(&tower) {
            if face.len > 1.5 {
                let mut y = ph + 3.0;
                while y < ph + th - 2.0 {
                    if rng.random_bool(0.55) {
                        face.decal(g, f, Glow, 0.0, y, 0.04, face.len * 0.9, 1.1, mul(WARM, 0.55));
                    }
                    y += 3.4;
                }
                face.decal(g, f, Glow, 0.0, ph + th - 0.6, 0.05, face.len, 0.4, NEON_AMBER);
            }
        }
    }
    // The pool deck on the podium's roof, palms round it.
    let pool = fit_inside(c, &rect(pu, lo.y + (hi.y - lo.y) * 0.22, (hi.x - lo.x) * 0.25, (hi.y - lo.y) * 0.1));
    if pool.len() >= 3 {
        ground(g, f, Matte, &pool, ph + 0.08, [0.15, 0.6, 0.75]);
        let (plo, phi) = bounds(&pool);
        neon_line(g, c, &[plo, Vec2::new(phi.x, plo.y), phi, Vec2::new(plo.x, phi.y)], ph + 0.15, true, NEON_CYAN);
        for k in 0..4 {
            let p = Vec2::new(if k % 2 == 0 { plo.x - 1.5 } else { phi.x + 1.5 }, if k < 2 { plo.y } else { phi.y });
            if c.inside_local(p) {
                tree_kind(g, wv(c, p), ph, TreeKind::Palm, 6.0, 3.2, pal.bark, mul(pal.foliage, 1.1));
            }
        }
    }
    // The tall sign at the corner of the forecourt.
    let sp = Vec2::new(lo.x + 2.0, (bounds(&c.poly).0.y + lo.y) * 0.5);
    if fits(c, sp, 1.2, 0.3) {
        let w = wv(c, sp);
        g.part(Metal).cylinder(w, 0.6, 0.6, 0.0, 16.0, 8, rgba(DARK));
        let face_dir = f.dir3(Vec2::new(0.0, -1.0));
        let yaw = f.yaw();
        g.part(Matte).boxed(Vec3::new(w.x, 13.0, w.y), Vec3::new(5.0, 6.0, 0.6), yaw, rgba(DARK));
        for (k, col) in [NEON_AMBER, NEON_PINK, NEON_CYAN, NEON_VIOLET].iter().enumerate() {
            g.part(Glow).boxed(Vec3::new(w.x, 10.8 + k as f32 * 1.4, w.y) + face_dir * 0.32, Vec3::new(4.2, 0.9, 0.05), yaw, rgba(*col));
        }
        g.part(Glow).boxed(Vec3::new(w.x, 16.6, w.y), Vec3::new(1.4, 1.4, 1.4), PI / 4.0, rgba(NEON_AMBER));
    }
    front_lamps(g, c, 4);
}
