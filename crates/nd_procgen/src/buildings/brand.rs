//! [megacorp] Brands on buildings: a mega-corp's logo lit on the roof of
//! everything it owns, a facade banner on its taller holdings, a crown of
//! light with the logo on all four sides of its headquarters, small
//! banners on its staff housing; a university's crest on its main hall,
//! annexes and the plazas it claimed (flags on poles). And the university's
//! own style: a hall with a portico, a clock tower and a lawn quad.
//!
//! The logo is the sim's pixel mark (`nd_sim::corp::BrandMark::bitmap`):
//! the shape lit in the brand colour, the monogram cut dark into it, on a
//! dark backing board so it reads by day as well as glowing at night.
//! Everything is placed off the geometry already drawn (its top and the
//! extent of its upper half), so it works on any style.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::corp::{Annex, BrandMark, BrandRole, LogoPixel, LOGO_N};
use rand::Rng;

const BOARD: [f32; 3] = [0.025, 0.026, 0.032];
const LETTER_DARK: [f32; 3] = [0.02, 0.02, 0.025];
const LETTER_LIT: [f32; 3] = [2.6, 2.6, 2.5];
const POLE: [f32; 3] = [0.5, 0.52, 0.55];

/// The mark's colour, lit.
fn lit(m: &BrandMark, k: f32) -> [f32; 3] {
    mul(m.colour, k)
}

/// The top of the building's mass and the extent (lot-local) of its upper
/// half: walls, glass and roofs drawn so far.
fn mass(g: &Geometry, c: &Ctx) -> Option<(f32, Vec2, Vec2)> {
    use Surface as S;
    let surfaces = [S::Concrete, S::Plaster, S::Brick, S::Stone, S::Panel, S::Glass, S::Metal, S::WornConcrete, S::WornPlaster, S::WornBrick, S::WornPanel, S::Wood];
    let mut top = 0.0f32;
    for s in surfaces {
        if let Some(m) = g.parts.get(s.index()) {
            for p in &m.positions {
                top = top.max(p[1]);
            }
        }
    }
    if top < 2.0 {
        return None;
    }
    let band = top * 0.5;
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for s in surfaces {
        if let Some(m) = g.parts.get(s.index()) {
            for p in &m.positions {
                if p[1] >= band {
                    let l = c.frame.to_local(Vec2::new(p[0], p[2]));
                    lo = lo.min(l);
                    hi = hi.max(l);
                }
            }
        }
    }
    (lo.x < hi.x && lo.y < hi.y).then_some((top, lo, hi))
}

/// The logo, `size` metres square, standing in the vertical plane through
/// `at` (lot-local) facing `out` (lot-local), centred at height `y`, on a
/// dark board a little bigger. Rows of equal pixels are merged.
pub fn logo(g: &mut Geometry, c: &Ctx, m: &BrandMark, at: Vec2, out: Vec2, y: f32, size: f32) {
    let f = &c.frame;
    let out = out.normalize_or_zero();
    if out == Vec2::ZERO {
        return;
    }
    // The viewer's right, looking at the face from outside.
    let right = Vec2::new(out.y, -out.x);
    let yaw = right.y.atan2(right.x);
    let px = size / LOGO_N as f32;
    let half = (LOGO_N as f32 - 1.0) * 0.5;
    // The board.
    let b = at - out * 0.06;
    f.bx_yaw(g, Matte, b.x, y, b.y, Vec3::new(size + px * 1.4, size + px * 1.4, 0.12), BOARD, yaw);
    let bmp = m.bitmap();
    let shape = lit(m, 2.4);
    for (r, row) in bmp.iter().enumerate() {
        let mut c0 = 0;
        while c0 < LOGO_N {
            let kind = row[c0];
            let mut c1 = c0 + 1;
            while c1 < LOGO_N && row[c1] == kind {
                c1 += 1;
            }
            if kind != LogoPixel::Empty {
                let (surf, col, d) = match kind {
                    LogoPixel::Shape => (Glow, shape, 0.04),
                    LogoPixel::LetterOnShape => (Matte, LETTER_DARK, 0.07),
                    _ => (Glow, LETTER_LIT, 0.04),
                };
                let mid = (c0 as f32 + c1 as f32 - 1.0) * 0.5;
                let p = at + right * ((mid - half) * px) + out * d;
                let yy = y + (half - r as f32) * px;
                f.bx_yaw(g, surf, p.x, yy, p.y, Vec3::new(px * (c1 - c0) as f32, px, 0.05), col, yaw);
                // The letter cut into the shape sits on a lit backing.
                if kind == LogoPixel::LetterOnShape {
                    let q = at + right * ((mid - half) * px) + out * 0.03;
                    f.bx_yaw(g, Glow, q.x, yy, q.y, Vec3::new(px * (c1 - c0) as f32, px, 0.02), mul(shape, 0.25), yaw);
                }
            }
            c0 = c1;
        }
    }
}

/// A logo on a frame standing on the roof at `p` (lot-local), facing `out`.
fn roof_sign(g: &mut Geometry, c: &Ctx, m: &BrandMark, p: Vec2, out: Vec2, roof: f32, size: f32) {
    let f = &c.frame;
    let legs = size * 0.25;
    let right = Vec2::new(out.y, -out.x);
    for s in [-0.35f32, 0.35] {
        let q = p + right * (s * size) - out * 0.3;
        f.bx(g, Metal, q.x, roof + legs * 0.5, q.y, Vec3::new(0.18, legs, 0.18), POLE);
    }
    logo(g, c, m, p, out, roof + legs + size * 0.5 + 0.2, size);
    // A lit sill under it.
    let s = p + out * 0.1;
    f.bx_yaw(g, Glow, s.x, roof + legs + 0.05, s.y, Vec3::new(size * 0.9, 0.12, 0.12), lit(m, 1.6), right.y.atan2(right.x));
}

/// A vertical banner on the face whose outward normal is `out`, through
/// `at`, from `y0` to `y1`: a lit strip in the brand colour with the logo
/// at its head.
fn banner(g: &mut Geometry, c: &Ctx, m: &BrandMark, at: Vec2, out: Vec2, y0: f32, y1: f32, w: f32) {
    if y1 - y0 < w * 1.5 {
        return;
    }
    let f = &c.frame;
    let right = Vec2::new(out.y, -out.x);
    let yaw = right.y.atan2(right.x);
    let strip_top = y1 - w * 1.2;
    let p = at + out * 0.08;
    f.bx_yaw(g, Matte, p.x, (y0 + y1) * 0.5, p.y, Vec3::new(w * 1.1, y1 - y0, 0.06), BOARD, yaw);
    let q = at + out * 0.13;
    f.bx_yaw(g, Glow, q.x, (y0 + strip_top) * 0.5, q.y, Vec3::new(w * 0.35, strip_top - y0 - 0.4, 0.04), lit(m, 1.8), yaw);
    logo(g, c, m, at + out * 0.16, out, y1 - w * 0.6, w);
}

/// The HQ's crown: two bands of light round the top, the logo on every
/// side above the roof, and a spire with an aviation light.
fn crown(g: &mut Geometry, c: &Ctx, m: &BrandMark, top: f32, lo: Vec2, hi: Vec2) {
    let f = &c.frame;
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let mid = (lo + hi) * 0.5;
    for (k, y) in [top - 1.2, top + 0.4].into_iter().enumerate() {
        let col = lit(m, if k == 0 { 1.4 } else { 2.2 });
        f.bx(g, Glow, mid.x, y, lo.y - 0.12, Vec3::new(w + 0.3, 0.3, 0.12), col);
        f.bx(g, Glow, mid.x, y, hi.y + 0.12, Vec3::new(w + 0.3, 0.3, 0.12), col);
        f.bx(g, Glow, lo.x - 0.12, y, mid.y, Vec3::new(0.12, 0.3, d + 0.3), col);
        f.bx(g, Glow, hi.x + 0.12, y, mid.y, Vec3::new(0.12, 0.3, d + 0.3), col);
    }
    // A screen wall round the roof carrying the logo on all four sides.
    let size = (w.min(d) * 0.7).clamp(4.0, 14.0);
    let wall = size + 1.5;
    let y = top + 0.6 + wall * 0.5;
    let inset = 0.4;
    let sides = [
        (Vec2::new(mid.x, lo.y + inset), Vec2::new(0.0, -1.0), w),
        (Vec2::new(mid.x, hi.y - inset), Vec2::new(0.0, 1.0), w),
        (Vec2::new(lo.x + inset, mid.y), Vec2::new(-1.0, 0.0), d),
        (Vec2::new(hi.x - inset, mid.y), Vec2::new(1.0, 0.0), d),
    ];
    for (p, out, len) in sides {
        let right = Vec2::new(out.y, -out.x);
        f.bx_yaw(g, Panel, p.x - out.x * 0.2, y, p.y - out.y * 0.2, Vec3::new((len - inset * 2.0).max(1.0), wall, 0.3), BOARD, right.y.atan2(right.x));
        logo(g, c, m, p, out, y, size);
    }
    // The spire.
    let spire = (top * 0.18).clamp(6.0, 30.0);
    g.part(Metal).cylinder(f.to_world(mid.x, mid.y), 0.6, 0.12, top + 0.6, top + 0.6 + spire + wall, 8, rgba(POLE));
    f.bx(g, Glow, mid.x, top + 0.6 + spire + wall, mid.y, Vec3::splat(0.5), c.palette.aviation_light);
    f.bx(g, Glow, mid.x, top + 0.6 + wall + 0.3, mid.y, Vec3::new(1.4, 0.25, 1.4), lit(m, 2.6));
}

/// Flags in the brand colour on poles at the corners of a plaza, the crest
/// on a board at its gate and coloured paving bands along its walks.
fn plaza(g: &mut Geometry, c: &Ctx, m: &BrandMark) {
    let f = &c.frame;
    let (lo, hi) = bounds(&c.lot_poly);
    let corners = [Vec2::new(lo.x + 2.0, lo.y + 2.0), Vec2::new(hi.x - 2.0, lo.y + 2.0), Vec2::new(lo.x + 2.0, hi.y - 2.0), Vec2::new(hi.x - 2.0, hi.y - 2.0)];
    for p in corners {
        if !c.inside_local(p) {
            continue;
        }
        f.bx(g, Metal, p.x, 3.5, p.y, Vec3::new(0.12, 7.0, 0.12), POLE);
        f.bx(g, Tarp, p.x + 0.8, 6.2, p.y, Vec3::new(1.5, 1.0, 0.04), mul(m.colour, 0.9));
        f.bx(g, Glow, p.x, 7.1, p.y, Vec3::splat(0.18), lit(m, 2.0));
    }
    // The gate: the crest on a low board by the street.
    let gate = Vec2::new((lo.x + hi.x) * 0.5, lo.y + 1.5);
    if c.inside_local(gate) {
        for s in [-1.4f32, 1.4] {
            f.bx(g, Stone, gate.x + s, 1.1, gate.y, Vec3::new(0.5, 2.2, 0.5), c.palette.stone);
        }
        logo(g, c, m, gate, Vec2::new(0.0, -1.0), 1.6, 2.2);
    }
    // Paving bands in the campus colour across the plaza.
    let mid_v = (lo.y + hi.y) * 0.5;
    f.flat(g, Matte, (lo.x + hi.x) * 0.5, 0.09, mid_v, (hi.x - lo.x) * 0.7, 0.6, 0.0, mul(m.colour, 0.55));
    f.flat(g, Matte, (lo.x + hi.x) * 0.5, 0.09, mid_v, (hi.y - lo.y) * 0.7, 0.6, std::f32::consts::FRAC_PI_2, mul(m.colour, 0.55));
}

/// Put the brand on what has been drawn.
pub fn decorate(g: &mut Geometry, c: &Ctx, m: &BrandMark) {
    if m.role == BrandRole::Annex(Annex::Plaza) {
        plaza(g, c, m);
        return;
    }
    let Some((top, lo, hi)) = mass(g, c) else { return };
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let mid = (lo + hi) * 0.5;
    let front = Vec2::new(mid.x, lo.y);
    let down = Vec2::new(0.0, -1.0);
    match m.role {
        BrandRole::Headquarters => {
            crown(g, c, m, top, lo, hi);
            banner(g, c, m, front + Vec2::new(-w * 0.3, 0.0), down, top * 0.2, top * 0.8, (w * 0.12).clamp(2.0, 5.0));
            banner(g, c, m, front + Vec2::new(w * 0.3, 0.0), down, top * 0.2, top * 0.8, (w * 0.12).clamp(2.0, 5.0));
        }
        BrandRole::Holding => {
            let size = (w.min(d) * 0.55).clamp(2.2, 9.0);
            roof_sign(g, c, m, mid, down, top, size);
            if top > 14.0 {
                banner(g, c, m, front + Vec2::new(w * 0.32, 0.0), down, top * 0.3, top * 0.85, (w * 0.1).clamp(1.6, 3.5));
            }
        }
        BrandRole::Housing => {
            let s = (w * 0.12).clamp(1.4, 2.4);
            banner(g, c, m, front + Vec2::new(-w * 0.35, 0.0), down, (top * 0.35).max(2.5), top * 0.9, s);
        }
        BrandRole::Campus => {
            let s = (w * 0.1).clamp(1.6, 3.0);
            banner(g, c, m, front + Vec2::new(-w * 0.38, 0.0), down, top * 0.25, top * 0.95, s);
            banner(g, c, m, front + Vec2::new(w * 0.38, 0.0), down, top * 0.25, top * 0.95, s);
        }
        BrandRole::Annex(_) => {
            let s = (w * 0.12).clamp(1.4, 2.6);
            banner(g, c, m, front + Vec2::new(w * 0.36, 0.0), down, (top * 0.3).max(1.5), top * 0.95, s);
            if top < 12.0 {
                roof_sign(g, c, m, mid, down, top, (w.min(d) * 0.35).clamp(1.8, 4.0));
            }
        }
    }
}

// ── the university ───────────────────────────────────────────────────────────

/// A university's main hall: a long stone block with a columned portico
/// and steps, a clock tower over the middle, wings at the back by tier, and
/// a lawn quad crossed by paths with trees and lamps.
pub fn university(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lawn = [0.14, 0.30, 0.10];
    let _ = yard(g, c, 0.3, Paving, mul(pal.paving, 1.05));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let floors = [2, 3, 4][c.tier.index()];
    let fh = c.fh().max(3.6);
    // The hall, set back behind the quad.
    let hall_d = (d * 0.32).clamp(8.0, 22.0);
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, hi.y - hall_d * 0.5 - 1.0, w * 0.42, hall_d * 0.5));
    if hall.len() < 3 {
        return;
    }
    let stone = mix(pal.stone, [0.78, 0.74, 0.66], 0.5);
    let h = floors as f32 * fh;
    extrude(g, f, Stone, &hall, 0.0, h, stone);
    cornice(g, c, &hall, h, 0.6, 0.35, Stone, mul(stone, 0.85));
    let mut spec = WindowSpec::from_ctx(c);
    spec.boarded = 0.0;
    spec.ac = 0.0;
    spec.w = 1.2;
    spec.h = 2.2;
    spec.spacing = 3.0;
    for face in c.faces(&hall) {
        if face.len > 4.0 {
            windows(g, c, rng, &face, 0.0, floors, fh, &spec);
        }
    }
    // Portico: columns and a pediment on the front face.
    let front: Face = c.front_face(&hall);
    let pw = (front.len * 0.45).clamp(6.0, 22.0);
    let n = ((pw / 2.6) as i32).max(3);
    for k in 0..=n {
        let t = -pw * 0.5 + pw * k as f32 / n as f32;
        front.bx(g, f, Stone, t, h * 0.42, 1.6, Vec3::new(0.7, h * 0.84, 0.7), mul(stone, 1.05));
    }
    front.bx(g, f, Stone, 0.0, h * 0.88, 1.6, Vec3::new(pw + 1.2, h * 0.12, 3.4), stone);
    // Steps.
    for s in 0..3 {
        front.bx(g, f, Stone, 0.0, 0.1 + s as f32 * 0.2, 3.6 - s as f32 * 0.5, Vec3::new(pw + 2.0 - s as f32, 0.2, 1.2), mul(stone, 0.9));
    }
    entrance(g, c, rng, &front, 0.0, 2.6, 3.2);
    // Clock tower over the middle.
    let mid = poly_center(&hall);
    let tw = (hall_d * 0.35).clamp(4.0, 7.0);
    let th = h + [8.0, 12.0, 18.0][c.tier.index()];
    f.bx(g, Stone, mid.x, (h + th) * 0.5, mid.y, Vec3::new(tw, th - h, tw), mul(stone, 0.95));
    for (out, rot) in [(Vec2::new(0.0, -1.0), 0.0f32), (Vec2::new(0.0, 1.0), 0.0), (Vec2::new(-1.0, 0.0), std::f32::consts::FRAC_PI_2), (Vec2::new(1.0, 0.0), std::f32::consts::FRAC_PI_2)] {
        let p = mid + out * (tw * 0.5 + 0.05);
        f.bx_yaw(g, Glow, p.x, th - 2.0, p.y, Vec3::new(tw * 0.55, tw * 0.55, 0.06), [2.2, 2.0, 1.5], rot);
    }
    loft(g, f, Metal, &rect(mid.x, mid.y, tw * 0.55, tw * 0.55), &rect(mid.x, mid.y, 0.15, 0.15), th, th + tw * 1.1, [0.18, 0.32, 0.26]);
    // Wings to the back at the bigger tiers.
    if c.tier.index() >= 1 {
        for s in [-1.0f32, 1.0] {
            let wing = fit_inside(c, &rect(mid.x + s * w * 0.32, hi.y - hall_d - 4.0, w * 0.1, 4.0));
            if wing.len() >= 3 && inradius(&wing) > 1.5 {
                extrude(g, f, Stone, &wing, 0.0, h * 0.8, stone);
            }
        }
    }
    // The quad: lawn, crossing paths, trees and lamps.
    let qv0 = lo.y + 1.5;
    let qv1 = hi.y - hall_d - 2.5;
    if qv1 - qv0 > 4.0 {
        let quad = fit_inside(c, &rect((lo.x + hi.x) * 0.5, (qv0 + qv1) * 0.5, w * 0.46, (qv1 - qv0) * 0.5));
        if quad.len() >= 3 {
            ground(g, f, Matte, &quad, 0.05, lawn);
            let (qlo, qhi) = bounds(&quad);
            let qc = (qlo + qhi) * 0.5;
            // Crossing paths, kept on the lawn.
            let cross_v = fit_inside(c, &rect(qc.x, qc.y, 1.2, (qhi.y - qlo.y) * 0.48));
            let cross_u = fit_inside(c, &rect(qc.x, qc.y, (qhi.x - qlo.x) * 0.48, 1.0));
            for p in [cross_v, cross_u] {
                if p.len() >= 3 {
                    ground(g, f, Paving, &p, 0.08, mul(pal.paving, 1.15));
                }
            }
            for k in 0..6 {
                let p = Vec2::new(lo.x + 3.0 + k as f32 * (w - 6.0) / 5.0, qc.y + if k % 2 == 0 { 3.5 } else { -3.5 });
                if c.inside_local(p) && (p.x - qc.x).abs() > 2.5 {
                    tree_in(g, f, pal, p, 6.0, 3.0, pal.foliage);
                }
            }
            for s in [-1.0f32, 1.0] {
                let p = Vec2::new(qc.x + s * 2.2, qc.y + 1.5);
                if c.inside_local(p) {
                    lamp_post_in(g, c, p, 3.6, pal.lamp);
                }
            }
        }
    }
}
