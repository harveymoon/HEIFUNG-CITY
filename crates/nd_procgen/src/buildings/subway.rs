//! [subway] Metro entrances and the Central Station.
//!
//! A metro station shows nothing of itself at street level but its
//! entrance: a paved plaza with a stairwell going down under an arched
//! glass canopy, a pylon with the lit M, benches, bike racks, trees. The
//! platform is underground and drawn by the app in the Underground view
//! (`nd_procgen::subway`).
//!
//! The Central Station is the city's grand terminus: a hall under a
//! barrel-vaulted glass roof on steel ribs, a stone front pavilion with a
//! great clock and the name in neon, a clock tower, a bus terminal under
//! saw-tooth canopies at one end, a metro entrance in the forecourt, and
//! long canopied platforms along the track (the railway station's own,
//! `rail::platform`).

use super::parts::*;
use super::{Ctx, Face, Frame};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::PI;

/// Neon for the metro sign: the M and the line band.
pub const METRO_CYAN: [f32; 3] = [0.25, 1.5, 1.9];
pub const NEON_MAGENTA: [f32; 3] = [1.9, 0.25, 1.35];
pub const NEON_AMBER: [f32; 3] = [1.9, 1.05, 0.25];

/// A small entrance on its plaza.
pub fn metro_entrance(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    // The whole lot is a paved plaza.
    ground(g, f, Paving, &offset_poly(&c.lot_poly, 0.3), 0.04, mul(pal.paving, 1.15));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    // The stairs run back from the street, so people walk straight in.
    let at = Vec2::new((lo.x + hi.x) * 0.5, lo.y + (d * 0.4).clamp(3.5, 9.0));
    let long = w < d;
    let turn = if long { 0.0 } else { PI * 0.5 };
    let len = (d.min(w) * 0.6).clamp(4.0, 7.0);
    // The canopy where it fits: as laid out, shorter, turned, or in the
    // lot's deep middle; on a scrap of a lot only the pylon shows.
    let anchor = interior_anchor(&c.lot_poly);
    let tries = [(at, turn, len), (at, turn, 4.0), (at, turn + PI * 0.5, 4.0), (anchor, turn, 4.0), (anchor, turn + PI * 0.5, 4.0), (anchor, turn, 3.0)];
    if let Some(&(p, t, l)) = tries.iter().find(|(p, t, l)| kiosk_fits(c, *p, *t, *l)) {
        kiosk(g, f, p, t, l, pal);
    }
    // The pylon by the kerb: the M that says "metro" from down the street.
    let pylon = clamp_into(&c.poly, Vec2::new(lo.x + w * 0.2, lo.y + 1.2));
    metro_pylon(g, f, pylon, 4.6, pal);
    // Benches, a bin, bike racks and trees where there is room.
    let face = c.front_face(&offset_poly(&c.lot_poly, 0.3));
    for s in [-1.0f32, 1.0] {
        let p = clamp_into(&c.poly, at + Vec2::new(s * (w * 0.32).max(2.5), 0.5));
        if c.inside_local(p) && p.distance(at) > 2.4 {
            let wp = f.to_world(p.x, p.y);
            bench_world(g, wp, f.yaw() + PI * 0.5, pal.wood, CHARCOAL);
        }
    }
    let back = Vec2::new((lo.x + hi.x) * 0.5, hi.y - 2.0);
    if d > 12.0 && c.inside_local(back) {
        for k in 0..3 {
            let p = back + Vec2::new((k as f32 - 1.0) * (w * 0.3), 0.0);
            if c.inside_local(p) && p.distance(at) > 4.0 {
                tree_in(g, f, pal, p, 5.0 + rng.random_range(0.0..2.0), 1.8, pal.foliage);
            }
        }
    }
    for k in 0..4 {
        let p = face.at(face.len * 0.25 + k as f32 * 0.9 - 1.4, -2.2);
        if c.inside_local(p) && p.distance(at) > 3.0 {
            f.bx(g, Matte, p.x, 0.45, p.y, Vec3::new(0.05, 0.9, 0.8), mul(pal.steel, 0.8));
        }
    }
    lamp_post(g, f, clamp_into(&c.poly, Vec2::new(hi.x - 1.0, lo.y + 1.0)), 5.0, pal.lamp);
}

/// The stairwell down to the platform: a dark slot with steps, glass
/// balustrades and an arched glass canopy over it, its edge lit in the
/// metro's cyan. `at` is the head of the stairs (lot-local), `turn` the
/// extra yaw (0: the stairs run inward from the street), `len` their run.
pub fn kiosk(g: &mut Geometry, f: &Frame, at: Vec2, turn: f32, len: f32, pal: &crate::palette::Palette) {
    let (sin, cos) = turn.sin_cos();
    let rot = |u: f32, v: f32| at + Vec2::new(u * cos - v * sin, u * sin + v * cos);
    let half_w = 1.5;
    // The well: a dark floor with lighter treads stepping down out of sight.
    let mid = rot(0.0, len * 0.5);
    f.flat(g, Matte, mid.x, 0.06, mid.y, half_w * 2.0, len, turn + PI * 0.5, DARK);
    let treads = (len / 0.6) as i32;
    for k in 0..treads {
        let p = rot(0.0, 0.3 + k as f32 * 0.6);
        let shade = 0.55 - 0.4 * k as f32 / treads.max(1) as f32;
        f.flat(g, Concrete, p.x, 0.07, p.y, half_w * 1.9, 0.12, turn, mul(pal.concrete(0), shade));
    }
    // Glass balustrades on three sides with steel caps.
    let glass = mul(pick_glass(pal), 1.1);
    for s in [-1.0f32, 1.0] {
        let a = rot(s * half_w, 0.0);
        let b = rot(s * half_w, len);
        let (wa, wb) = (f.to_world(a.x, a.y), f.to_world(b.x, b.y));
        g.part(Glass).path_box(&[wa, wb], 0.0, 0.08, 0.0, 1.1, rgba(glass));
        g.part(Metal).path_box(&[wa, wb], 0.0, 0.12, 1.1, 1.18, rgba(mul(pal.steel, 1.1)));
    }
    let (ba, bb) = (rot(-half_w, len), rot(half_w, len));
    let (wa, wb) = (f.to_world(ba.x, ba.y), f.to_world(bb.x, bb.y));
    g.part(Glass).path_box(&[wa, wb], 0.0, 0.08, 0.0, 1.1, rgba(glass));
    // The canopy: posts and an arched glass roof along the stairs.
    let (span, rise, base) = (half_w + 0.35, 1.1, 2.6);
    for v in [0.2, len - 0.2] {
        for s in [-1.0f32, 1.0] {
            let p = rot(s * span, v);
            f.bx(g, Metal, p.x, base * 0.5, p.y, Vec3::new(0.14, base, 0.14), mul(pal.steel, 0.8));
        }
    }
    let a3 = |u: f32, y: f32, v: f32| {
        let p = rot(u, v);
        f.at(p.x, y, p.y)
    };
    vault(g, Glass, rgba(mul(glass, 0.9)), 6, span, rise, base, 0.0, len, &a3);
    // Ribs at both ends and the lit edge.
    for v in [0.0, len] {
        rib(g, Metal, rgba(mul(pal.steel, 0.7)), 6, span, rise, base, v, 0.1, &a3);
    }
    let (e0, e1) = (a3(-span, base, 0.0), a3(-span, base, len));
    g.part(Glow).beam(e0, e1, 0.08, rgba(METRO_CYAN));
    let (e0, e1) = (a3(span, base, 0.0), a3(span, base, len));
    g.part(Glow).beam(e0, e1, 0.08, rgba(METRO_CYAN));
    // The M over the head of the stairs, facing the street.
    // Facing the street: the stairs run away from it along +v.
    let m = rot(0.0, -0.05);
    let face = Face::new(m - rot_dir(turn), m + rot_dir(turn), Vec2::new(sin, -cos));
    neon_text(g, f, &face, "M", 0.0, base + rise * 0.45, 0.7, 0.05, METRO_CYAN);
}

/// Whether a kiosk at `at` turned `turn` with stairs `len` long stands
/// wholly on the lot, canopy included.
pub fn kiosk_fits(c: &Ctx, at: Vec2, turn: f32, len: f32) -> bool {
    let (sin, cos) = turn.sin_cos();
    let rot = |u: f32, v: f32| at + Vec2::new(u * cos - v * sin, u * sin + v * cos);
    let w = 1.5 + 0.5;
    poly_inside(c, &[rot(-w, -0.4), rot(w, -0.4), rot(w, len + 0.4), rot(-w, len + 0.4)])
}

fn rot_dir(turn: f32) -> Vec2 {
    Vec2::new(turn.cos(), turn.sin())
}

fn pick_glass(pal: &crate::palette::Palette) -> [f32; 3] {
    pal.glass.first().copied().unwrap_or([0.3, 0.45, 0.55])
}

/// A tall dark pylon with the lit M in a ring at the top.
pub fn metro_pylon(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, pal: &crate::palette::Palette) {
    f.bx(g, Panel, p.x, h * 0.5, p.y, Vec3::new(0.5, h, 0.5), mul(CHARCOAL, 1.2));
    f.bx(g, Panel, p.x, h + 0.55, p.y, Vec3::new(1.3, 1.3, 0.3), DARK);
    for (s, n) in [(1.0f32, Vec2::new(0.0, -1.0)), (-1.0, Vec2::new(0.0, 1.0))] {
        let face = Face::new(p + Vec2::new(-0.6 * s, 0.0), p + Vec2::new(0.6 * s, 0.0), n);
        neon_text(g, f, &face, "M", 0.0, h + 0.55, 0.8, 0.17, METRO_CYAN);
    }
    f.bx(g, Glow, p.x, h - 0.4, p.y, Vec3::new(0.52, 0.18, 0.52), mul(pal.lamp, 0.8));
}

/// A barrel vault of `segs` facets: across `-span..span` (local u) rising
/// `rise` above `base`, from `v0` to `v1` along the axis. Both faces drawn.
#[allow(clippy::too_many_arguments)]
fn vault(g: &mut Geometry, s: Surface, color: [f32; 4], segs: usize, span: f32, rise: f32, base: f32, v0: f32, v1: f32, at: &dyn Fn(f32, f32, f32) -> Vec3) {
    let arc = |k: usize| {
        let t = PI * k as f32 / segs as f32;
        (-span * t.cos(), base + rise * t.sin())
    };
    for k in 0..segs {
        let (u0, y0) = arc(k);
        let (u1, y1) = arc(k + 1);
        let q = [at(u0, y0, v0), at(u1, y1, v0), at(u1, y1, v1), at(u0, y0, v1)];
        g.part(s).quad(q, color);
        g.part(s).quad([q[0], q[3], q[2], q[1]], color);
    }
}

/// One rib of a vault at `v`, a bar `thick` square.
#[allow(clippy::too_many_arguments)]
fn rib(g: &mut Geometry, s: Surface, color: [f32; 4], segs: usize, span: f32, rise: f32, base: f32, v: f32, thick: f32, at: &dyn Fn(f32, f32, f32) -> Vec3) {
    for k in 0..segs {
        let t0 = PI * k as f32 / segs as f32;
        let t1 = PI * (k + 1) as f32 / segs as f32;
        g.part(s).beam(at(-span * t0.cos(), base + rise * t0.sin(), v), at(-span * t1.cos(), base + rise * t1.sin(), v), thick, color);
    }
}

/// Strokes of a tiny neon font on a 2 by 4 grid (x 0..2, y 0..4, y up).
fn glyph(ch: char) -> &'static [[f32; 4]] {
    match ch {
        'A' => &[[0.0, 0.0, 0.0, 3.0], [0.0, 3.0, 1.0, 4.0], [1.0, 4.0, 2.0, 3.0], [2.0, 3.0, 2.0, 0.0], [0.0, 2.0, 2.0, 2.0]],
        'B' => &[[0.0, 0.0, 0.0, 4.0], [0.0, 4.0, 1.5, 4.0], [1.5, 4.0, 2.0, 3.0], [2.0, 3.0, 1.5, 2.0], [0.0, 2.0, 1.5, 2.0], [1.5, 2.0, 2.0, 1.0], [2.0, 1.0, 1.5, 0.0], [1.5, 0.0, 0.0, 0.0]],
        'C' => &[[2.0, 4.0, 0.0, 4.0], [0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0]],
        'E' => &[[2.0, 4.0, 0.0, 4.0], [0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [0.0, 2.0, 1.5, 2.0]],
        'L' => &[[0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0]],
        'M' => &[[0.0, 0.0, 0.0, 4.0], [0.0, 4.0, 1.0, 2.0], [1.0, 2.0, 2.0, 4.0], [2.0, 4.0, 2.0, 0.0]],
        'N' => &[[0.0, 0.0, 0.0, 4.0], [0.0, 4.0, 2.0, 0.0], [2.0, 0.0, 2.0, 4.0]],
        'O' => &[[0.0, 0.0, 0.0, 4.0], [0.0, 4.0, 2.0, 4.0], [2.0, 4.0, 2.0, 0.0], [2.0, 0.0, 0.0, 0.0]],
        'R' => &[[0.0, 0.0, 0.0, 4.0], [0.0, 4.0, 2.0, 4.0], [2.0, 4.0, 2.0, 2.0], [2.0, 2.0, 0.0, 2.0], [0.6, 2.0, 2.0, 0.0]],
        'T' => &[[0.0, 4.0, 2.0, 4.0], [1.0, 4.0, 1.0, 0.0]],
        'S' => &[[2.0, 4.0, 0.0, 4.0], [0.0, 4.0, 0.0, 2.0], [0.0, 2.0, 2.0, 2.0], [2.0, 2.0, 2.0, 0.0], [2.0, 0.0, 0.0, 0.0]],
        'I' => &[[1.0, 0.0, 1.0, 4.0]],
        'U' => &[[0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [2.0, 0.0, 2.0, 4.0]],
        _ => &[],
    }
}

/// Neon letters standing on a face: `text` centred `t` along it, its
/// middle at height `y`, letters `h` tall, `d` proud of the face.
#[allow(clippy::too_many_arguments)]
pub fn neon_text(g: &mut Geometry, f: &Frame, face: &Face, text: &str, t: f32, y: f32, h: f32, d: f32, color: [f32; 3]) {
    let unit = h / 4.0;
    let advance = 3.0 * unit;
    let n = text.chars().count() as f32;
    let start = t - (n * advance - unit) * 0.5;
    let tube = (h * 0.07).clamp(0.05, 0.35);
    for (i, ch) in text.chars().enumerate() {
        let x0 = start + i as f32 * advance;
        // Mirrored about `t`: a face's tangent runs right to left as seen
        // from outside, and the text must read from the street.
        for s in glyph(ch) {
            let a = face.at(2.0 * t - (x0 + s[0] * unit), d);
            let b = face.at(2.0 * t - (x0 + s[2] * unit), d);
            let (ya, yb) = (y - h * 0.5 + s[1] * unit, y - h * 0.5 + s[3] * unit);
            g.part(Glow).beam(f.at(a.x, ya, a.y), f.at(b.x, yb, b.y), tube, rgba(color));
        }
    }
}

/// A clock face on `face`: a bronze rim, a lit dial and two hands.
#[allow(clippy::too_many_arguments)]
pub fn clock(g: &mut Geometry, f: &Frame, face: &Face, t: f32, y: f32, r: f32, d: f32, pal: &crate::palette::Palette) {
    let sides = 20;
    let ring = |rad: f32, dd: f32| -> Vec<Vec3> {
        (0..sides)
            .map(|k| {
                let a = 2.0 * PI * k as f32 / sides as f32;
                let p = face.at(t + rad * a.cos(), dd);
                f.at(p.x, y + rad * a.sin(), p.y)
            })
            .collect()
    };
    let out = f.dir3(face.out);
    g.part(Metal).cap(&ring(r * 1.12, d), out, rgba(pal.bronze));
    g.part(Glow).cap(&ring(r, d + 0.05), out, rgba(mul(pal.window_warm, 1.1)));
    // Hands: ten past ten, the shop-window time.
    let centre = face.at(t, d + 0.1);
    let c3 = f.at(centre.x, y, centre.y);
    for (ang, len) in [(PI * 0.5 + PI / 3.0, 0.55), (PI * 0.5 - PI / 3.0 * 0.5, 0.85)] {
        let p = face.at(t + r * len * ang.cos(), d + 0.1);
        g.part(Matte).beam(c3, f.at(p.x, y + r * len * ang.sin(), p.y), r * 0.08, rgba(DARK));
    }
    // Hour marks.
    for k in 0..12 {
        let a = 2.0 * PI * k as f32 / 12.0;
        let (p0, p1) = (face.at(t + r * 0.8 * a.cos(), d + 0.08), face.at(t + r * 0.93 * a.cos(), d + 0.08));
        g.part(Matte).beam(f.at(p0.x, y + r * 0.8 * a.sin(), p0.y), f.at(p1.x, y + r * 0.93 * a.sin(), p1.y), r * 0.05, rgba(DARK));
    }
}

/// The Central Station.
pub fn central_station(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    ground(g, f, Paving, &offset_poly(&c.lot_poly, 0.3), 0.04, mul(pal.paving, 1.2));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    // The hall takes the left three fifths, the bus terminal the rest.
    let (u0, u1) = (lo.x + w * 0.04, lo.x + w * 0.6);
    let (v0, v1) = (lo.y + d * 0.2, lo.y + d * 0.72);
    let hall = fit_inside(c, &vec![Vec2::new(u0, v0), Vec2::new(u1, v0), Vec2::new(u1, v1), Vec2::new(u0, v1)]);
    let (hlo, hhi) = bounds(&hall);
    let (hw, hd) = (hhi.x - hlo.x, hhi.y - hlo.y);
    let wall = (hd * 0.28).clamp(5.0, 11.0);
    let stone = mul(pal.stone, 1.25);
    // Stone base, glass clerestory above it.
    extrude(g, f, Stone, &hall, 0.0, wall * 0.45, stone);
    for face in c.faces(&hall) {
        face.bx(g, f, Glass, 0.0, wall * 0.72, -0.15, Vec3::new(face.len, wall * 0.54, 0.2), pick(rng, &pal.glass));
        let mullions = ((face.len / 4.0) as i32).max(1);
        for k in 0..=mullions {
            let t = -face.len * 0.5 + face.len * k as f32 / mullions as f32;
            face.bx(g, f, Metal, t, wall * 0.72, 0.0, Vec3::new(0.18, wall * 0.54, 0.3), mul(pal.steel, 0.75));
        }
        if face.len > 8.0 {
            band_windows(g, c, rng, &face, 1.2, 1, 3.0);
        }
    }
    cornice(g, c, &hall, wall - 0.4, 0.8, 0.5, Stone, mul(stone, 0.9));
    // The vault: ribs every 7 m along the hall, glass between, a neon ridge.
    let span = hd * 0.5;
    let rise = (span * 0.75).min(16.0);
    let mid_v = (hlo.y + hhi.y) * 0.5;
    let at = |u: f32, y: f32, v: f32| f.at(v, y, mid_v + u);
    // Local axes: the vault's axis runs along the frame's u; `at(u, y, v)`
    // here takes the arch coordinate first (across the hall, local v).
    let segs = if hd > 20.0 { 10 } else { 6 };
    vault(g, Glass, rgba(mul(pick(rng, &pal.glass), 1.15)), segs, span, rise, wall, hlo.x, hhi.x, &at);
    let ribs = ((hw / 7.0) as i32).max(1);
    for k in 0..=ribs {
        let u = hlo.x + hw * k as f32 / ribs as f32;
        rib(g, Metal, rgba(mul(pal.steel, 0.6)), segs, span, rise, wall, u, 0.45, &at);
    }
    g.part(Glow).beam(at(0.0, wall + rise + 0.25, hlo.x), at(0.0, wall + rise + 0.25, hhi.x), 0.22, rgba(NEON_MAGENTA));
    for s in [-1.0f32, 1.0] {
        g.part(Glow).beam(at(s * span, wall + 0.1, hlo.x), at(s * span, wall + 0.1, hhi.x), 0.12, rgba(METRO_CYAN));
    }
    // Glass fans close the vault's ends.
    for u in [hlo.x, hhi.x] {
        let centre = at(0.0, wall, u);
        let tint = rgba(mul(pal.window_warm, 0.45));
        for k in 0..segs {
            let t0 = PI * k as f32 / segs as f32;
            let t1 = PI * (k + 1) as f32 / segs as f32;
            let a = at(-span * t0.cos(), wall + rise * t0.sin(), u);
            let b = at(-span * t1.cos(), wall + rise * t1.sin(), u);
            let out = Vec3::new(f.along.x, 0.0, f.along.y) * if u == hlo.x { -1.0 } else { 1.0 };
            g.part(Glow).tri_facing(centre, a, b, out, tint);
            g.part(Glow).tri_facing(centre, a, b, -out, tint);
        }
    }
    // The front pavilion: stone, the great clock, the name in neon.
    let front = c.front_face(&hall);
    let pav_w = (front.len * 0.38).clamp(8.0, 28.0);
    let pav_h = wall + rise * 0.9;
    let pav = fit_inside(c, &rect(front.mid().x, front.mid().y - 1.5, pav_w * 0.5, 4.0));
    extrude(g, f, Stone, &pav, 0.0, pav_h, stone);
    cornice(g, c, &pav, pav_h - 0.6, 1.0, 0.6, Stone, mul(stone, 0.85));
    let pf = c.front_face(&pav);
    // Three tall arched doorways, lit from inside.
    for k in [-1.0f32, 0.0, 1.0] {
        let t = k * pf.len * 0.28;
        pf.bx(g, f, Glow, t, wall * 0.42, 0.05, Vec3::new(pf.len * 0.18, wall * 0.8, 0.1), mul(pal.window_warm, 0.75));
        pf.bx(g, f, Stone, t, wall * 0.84, 0.15, Vec3::new(pf.len * 0.2, 0.6, 0.3), mul(stone, 0.8));
    }
    canopy(g, c, &pf, 0.0, 4.2, pf.len * 0.9, 3.5, mul(pal.steel, 0.9));
    let clock_r = (pav_w * 0.16).clamp(1.2, 3.4);
    clock(g, f, &pf, 0.0, pav_h - clock_r - 1.4, clock_r, 0.12, pal);
    neon_text(g, f, &pf, "CENTRAL", 0.0, wall * 0.98, (pav_w / 9.0).clamp(0.6, 2.6), 0.4, NEON_MAGENTA);
    // A thin cyan frame round the name.
    let nw = pav_w * 0.86;
    let nh = (pav_w / 9.0).clamp(0.6, 2.6) * 1.5;
    for (a, b) in [((-0.5f32, -0.5f32), (0.5f32, -0.5f32)), ((0.5, -0.5), (0.5, 0.5)), ((0.5, 0.5), (-0.5, 0.5)), ((-0.5, 0.5), (-0.5, -0.5))] {
        let pa = pf.at(a.0 * nw, 0.35);
        let pb = pf.at(b.0 * nw, 0.35);
        g.part(Glow).beam(f.at(pa.x, wall * 0.98 + a.1 * nh, pa.y), f.at(pb.x, wall * 0.98 + b.1 * nh, pb.y), 0.1, rgba(METRO_CYAN));
    }
    // The clock tower at the hall's left front corner.
    let tower_s = (hw * 0.08).clamp(4.0, 8.0);
    let tc = clamp_into(&c.poly, Vec2::new(hlo.x + tower_s * 0.5 + 0.5, hlo.y - tower_s * 0.2));
    let tower = fit_inside(c, &rect(tc.x, tc.y, tower_s * 0.5, tower_s * 0.5));
    let tower_h = (pav_h * 1.8).clamp(18.0, 42.0);
    extrude(g, f, Stone, &tower, 0.0, tower_h, mul(stone, 0.95));
    cornice(g, c, &tower, tower_h - 0.6, 0.8, 0.4, Stone, mul(stone, 0.8));
    for face in c.faces(&tower) {
        if face.len > 2.0 {
            clock(g, f, &face, 0.0, tower_h - tower_s * 0.55, tower_s * 0.32, 0.1, pal);
            face.bx(g, f, Glow, 0.0, tower_h * 0.5, 0.04, Vec3::new(0.25, tower_h * 0.55, 0.08), NEON_AMBER);
        }
    }
    let tcen = poly_center(&tower);
    f.bx(g, Metal, tcen.x, tower_h + 2.0, tcen.y, Vec3::new(tower_s * 0.5, 4.0, tower_s * 0.5), mul(pal.steel, 0.6));
    f.bx(g, Glow, tcen.x, tower_h + 4.2, tcen.y, Vec3::new(tower_s * 0.55, 0.3, tower_s * 0.55), NEON_MAGENTA);
    f.bx(g, Metal, tcen.x, tower_h + 7.0, tcen.y, Vec3::new(0.25, 6.0, 0.25), mul(pal.steel, 0.5));
    aviation_light_local(g, f, pal, tcen.x, tower_h + 10.1, tcen.y, 0.4);
    // The bus terminal: saw-tooth canopies over drive-through bays.
    let term = fit_inside(c, &rect((lo.x + w * 0.64 + hi.x - 1.0) * 0.5, lo.y + d * 0.4, (hi.x - 1.0 - lo.x - w * 0.64) * 0.5, d * 0.28));
    let (tlo, thi) = bounds(&term);
    let (b0, b1) = (tlo.x, thi.x);
    if b1 - b0 > 10.0 && thi.y - tlo.y > 8.0 && poly_inside(c, &term) {
        let bays = (((b1 - b0) / 7.0) as i32).clamp(1, 8);
        let (bv0, bv1) = (tlo.y, thi.y);
        for k in 0..bays {
            let u = b0 + (k as f32 + 0.5) * (b1 - b0) / bays as f32;
            // Island with a shelter and the bay's lit number post.
            let island = fit_inside(c, &rect(u + 2.4, (bv0 + bv1) * 0.5, 0.8, (bv1 - bv0) * 0.45));
            extrude(g, f, Concrete, &island, 0.0, 0.18, mul(pal.concrete(0), 1.2));
            let ic = poly_center(&island);
            f.bx(g, Glow, ic.x, 2.4, bv0 + 1.5, Vec3::new(0.3, 0.8, 0.3), NEON_AMBER);
            bench_world(g, f.to_world(ic.x, ic.y), f.yaw() + PI * 0.5, pal.wood, CHARCOAL);
            // Bay markings.
            f.flat(g, Glow, u - 0.6, 0.06, (bv0 + bv1) * 0.5, 0.12, (bv1 - bv0) * 0.9, 0.0, mul(NEON_AMBER, 0.5));
            if k % 2 == 0 {
                let p = Vec2::new(u - 0.6, (bv0 + bv1) * 0.5);
                if vehicle_fits(c, p, PI * 0.5, Vehicle::Bus) {
                    let col = pick(rng, &pal.car_colors);
                    vehicle(g, f, pal, rng, p, 0.0, PI * 0.5, Vehicle::Bus, col);
                }
            }
        }
        // The canopy: a saw-tooth of sloping glass on steel columns.
        let teeth = bays;
        for k in 0..teeth {
            let ua = b0 + (b1 - b0) * k as f32 / teeth as f32;
            let ub = b0 + (b1 - b0) * (k + 1) as f32 / teeth as f32;
            let q = [f.at(ua, 6.0, bv0), f.at(ub, 7.6, bv0), f.at(ub, 7.6, bv1), f.at(ua, 6.0, bv1)];
            g.part(Glass).quad(q, rgba(mul(pick_glass(pal), 1.2)));
            g.part(Glass).quad([q[0], q[3], q[2], q[1]], rgba(mul(pick_glass(pal), 1.2)));
            g.part(Metal).beam(f.at(ub, 6.0, bv0), f.at(ub, 7.6, bv0), 0.25, rgba(mul(pal.steel, 0.7)));
            g.part(Metal).beam(f.at(ub, 6.0, bv1), f.at(ub, 7.6, bv1), 0.25, rgba(mul(pal.steel, 0.7)));
            g.part(Metal).beam(f.at(ua, 6.0, bv0), f.at(ub, 6.0, bv0), 0.25, rgba(mul(pal.steel, 0.7)));
            g.part(Metal).beam(f.at(ua, 6.0, bv1), f.at(ub, 6.0, bv1), 0.25, rgba(mul(pal.steel, 0.7)));
            for v in [bv0 + 0.5, bv1 - 0.5] {
                f.bx(g, Metal, ua + 0.3, 3.0, v, Vec3::new(0.35, 6.0, 0.35), mul(pal.steel, 0.7));
            }
        }
        let sign = Face::new(Vec2::new(b0, bv0 - 0.2), Vec2::new(b1, bv0 - 0.2), Vec2::new(0.0, -1.0));
        neon_text(g, f, &sign, "BUS", 0.0, 8.6, 1.2, 0.0, NEON_AMBER);
    }
    // The metro entrance in the forecourt, its pylon by the kerb.
    let mk = clamp_into(&c.poly, Vec2::new(lo.x + w * 0.62 - 3.0, lo.y + 1.0));
    if kiosk_fits(c, mk, 0.0, 5.0) {
        kiosk(g, f, mk, 0.0, 5.0, pal);
    }
    metro_pylon(g, f, clamp_into(&c.poly, Vec2::new(lo.x + w * 0.62 + 0.5, lo.y + 0.8)), 5.0, pal);
    // Taxi rank and lamps along the front.
    let gate = c.front_face(&offset_poly(&c.lot_poly, 0.3));
    for k in 0..3 {
        let p = gate.at(-gate.len * 0.12 + k as f32 * 5.0, -4.0);
        if vehicle_fits(c, p, 0.0, Vehicle::Car) {
            vehicle(g, f, pal, rng, p, 0.0, 0.0, Vehicle::Car, [0.95, 0.75, 0.1]);
        }
    }
    for k in 0..5 {
        lamp_post(g, f, gate.at(-gate.len * 0.45 + gate.len * 0.22 * k as f32, -1.4), 6.0, pal.lamp);
    }
    // Platforms along the track, as at any station.
    super::rail::platform(g, c, rng, poly_center(&hall));
}
