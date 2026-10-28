//! [r4-oldstone] The Old Stone look of the Old Town Charter story (the
//! Historic look's own buildings): stone and timber-frame houses, narrow
//! gabled rows, courtyard houses, market-square arcades, the guild hall,
//! the clock tower and its church, old inns, and a stone manor round a
//! courtyard for the district's rich (its McMansion and billionaire's
//! mansion). The small industry (the old mill with its wheel, the tannery,
//! the cobbled yards) is in `oldstone_works.rs`.
//!
//! The look is stylised and weathered, not quaint: limestone and sandstone
//! darkened by age, ground floors of stone under jettied storeys of
//! limewashed render and black oak framing, steep tile and stone-slate
//! roofs, crow-stepped gables, leaded windows in stone surrounds, arched
//! gateways. At night the windows glow candle-warm, gas lanterns hang by
//! the doors and on cast-iron posts (lit only after dark, `NIGHT_MARK`, with
//! a pool of light under them), and the church windows and the clock faces
//! are lit.
//!
//! Every choice comes from the building's seed: the mass from its main
//! stream, the per-window mullions from a hash of the seed, the face and
//! the window, so a building is the same from every camera. Sills, heads,
//! quoins, studs and braces, brackets and leading go into the detail
//! layers ([`Lod`]) the renderer drops with distance.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::depth::{FRAME, PANE};
use crate::hash::hash2_salted;
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::LandUse;
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Limestone and sandstone, from fresh to old.
pub(super) const STONES: [[f32; 3]; 5] = [[0.58, 0.54, 0.46], [0.52, 0.47, 0.39], [0.61, 0.56, 0.47], [0.47, 0.44, 0.39], [0.55, 0.48, 0.37]];
/// Limewash and render over the timber framing: cream, ochre, oatmeal, a
/// faded red and a grey-green.
pub(super) const RENDERS: [[f32; 3]; 5] = [[0.78, 0.72, 0.58], [0.74, 0.60, 0.42], [0.70, 0.67, 0.58], [0.64, 0.48, 0.38], [0.60, 0.62, 0.54]];
/// Old oak, tarred black.
pub(super) const TIMBERS: [[f32; 3]; 3] = [[0.15, 0.11, 0.08], [0.22, 0.15, 0.10], [0.12, 0.10, 0.09]];
/// Clay tiles, stone slates and slate.
pub(super) const ROOFS: [[f32; 3]; 4] = [[0.40, 0.21, 0.14], [0.34, 0.19, 0.14], [0.29, 0.28, 0.26], [0.17, 0.17, 0.19]];
/// Painted oak doors: bare oak, green, oxblood, faded blue.
pub(super) const DOORS: [[f32; 3]; 4] = [[0.22, 0.15, 0.10], [0.15, 0.24, 0.17], [0.34, 0.10, 0.08], [0.18, 0.22, 0.30]];
/// A gas mantle: warm white with a little green.
pub(super) const GAS: [f32; 3] = [1.85, 1.6, 0.9];
/// Candle and lamp light in the windows.
pub(super) const WARM: [[f32; 3]; 4] = [[1.35, 0.88, 0.42], [1.3, 1.0, 0.6], [1.4, 0.76, 0.34], [1.2, 0.95, 0.66]];
/// Stained glass lit from inside: amber, ruby, cobalt, green.
pub(super) const STAINED: [[f32; 3]; 4] = [[1.6, 1.05, 0.4], [1.5, 0.35, 0.25], [0.45, 0.6, 1.5], [0.6, 1.2, 0.5]];
/// A clock dial lit from behind.
pub(super) const DIAL: [f32; 3] = [1.75, 1.55, 1.05];
/// Iron.
pub(super) const IRON: [f32; 3] = [0.10, 0.10, 0.11];
/// Gilding.
const GILT: [f32; 3] = [0.72, 0.56, 0.24];
/// Salt of the per-window hash.
const PANE_SALT: u64 = 0x01D5_7011;
/// Depths off a wall: heads under the grime (0.02), the timber framing
/// over it and under the graffiti (0.024) and the frames (0.03), the
/// mullions and leading just proud of the glass.
pub(super) const HEAD: f32 = 0.012;
pub(super) const TIMBER: f32 = 0.022;
const BAR: f32 = PANE + 0.008;
/// A lit disc over its dark glass, and what is drawn over it.
const LIT_OVER: f32 = PANE + 0.008;
const HANDS: f32 = PANE + 0.016;
/// Quoins stand out between the frames and the glass; banners hang there
/// too, clear of the carved letters.
const QUOIN: f32 = 0.04;
const BANNER: f32 = 0.04;

/// One building's colours.
#[derive(Clone, Copy)]
pub(super) struct Paint {
    pub stone: [f32; 3],
    /// Plinth, quoins' shadow side, old stone.
    pub dark: [f32; 3],
    /// Dressed stone: surrounds, sills, copings.
    pub trim: [f32; 3],
    pub render: [f32; 3],
    pub timber: [f32; 3],
    pub roof: [f32; 3],
    pub door: [f32; 3],
}

/// A building's paint: stone darkened as it ages.
pub(super) fn paint(c: &Ctx, rng: &mut impl Rng) -> Paint {
    let age = 1.0 - 0.22 * c.wear;
    let stone = mul(pick(rng, &STONES), age * rng.random_range(0.93..1.05));
    Paint {
        stone,
        dark: mul(stone, 0.7),
        trim: mul(mix(stone, [0.72, 0.68, 0.60], 0.5), 1.0 - 0.15 * c.wear),
        render: mul(pick(rng, &RENDERS), age * rng.random_range(0.95..1.04)),
        timber: pick(rng, &TIMBERS),
        roof: mul(pick(rng, &ROOFS), rng.random_range(0.9..1.08)),
        door: pick(rng, &DOORS),
    }
}

/// A warm window light, or dark glass, by the building's lit chance
/// (raised by `more` for an inn or a hall).
pub(super) fn light(c: &Ctx, rng: &mut impl Rng, more: f32) -> Pane {
    if coin(rng, (c.lit_chance() + more).min(1.0)) {
        Pane::Lit(mul(pick(rng, &WARM), rng.random_range(0.6..1.0)))
    } else {
        Pane::Dark
    }
}

/// A face's identity for the per-window hash: its middle, to the decimetre.
fn face_key(c: &Ctx, face: &Face) -> u64 {
    let m = face.mid();
    hash2_salted((m.x * 10.0).round() as i64 as u64, (m.y * 10.0).round() as i64 as u64, PANE_SALT ^ c.seed)
}

// ── windows, doors and arches ────────────────────────────────────────────────

/// What a window is set in: a stone surround or an oak frame.
#[derive(Clone, Copy)]
pub(super) enum Set {
    Stone,
    Oak,
}

/// A leaded window `w` by `h` centred `t` along a face at height `y`: its
/// surround, the glazing, and (detail) a sill and a head, then the stone
/// mullion and transom or the oak glazing bars (`hash` picks which).
#[allow(clippy::too_many_arguments)]
pub(super) fn window(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, h: f32, pane: Pane, pt: &Paint, set: Set, hash: u64) {
    let f = &c.frame;
    let (s, col) = match set {
        Set::Stone => (Stone, pt.trim),
        Set::Oak => (Wood, pt.timber),
    };
    face.decal(g, f, s, t, y, FRAME, w + 0.26, h + 0.26, col);
    face.glaze(g, c, t, y, PANE, w, h, pane);
    g.lod(Lod::Mid, |g| {
        super::hk::wall_box(g, c, s, face, t, y - h * 0.5 - 0.17, 0.0, Vec3::new(w + 0.4, 0.12, 0.16), col);
        if matches!(set, Set::Stone) {
            face.decal(g, f, Stone, t, y + h * 0.5 + 0.22, HEAD, w + 0.6, 0.3, mul(pt.trim, 0.9));
        }
    });
    g.lod(Lod::Near, |g| {
        let n = if w > 1.0 { 1 + (hash % 2) as i32 } else { 1 };
        for i in 1..=n {
            let bt = t - w * 0.5 + w * i as f32 / (n + 1) as f32;
            face.decal(g, f, s, bt, y, BAR, 0.09, h, col);
        }
        face.decal(g, f, s, t, y + h * 0.2, BAR + 0.002, w, 0.08, col);
    });
}

/// Windows across a face, `floors` storeys from `y0`, one a bay of about
/// `spacing`, each `wf` of its bay wide and `hf` of a storey high, keeping
/// clear of the spans in `keep` (centre, half width, and whether all the
/// storeys or only the bottom one).
#[allow(clippy::too_many_arguments)]
pub(super) fn window_rows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spacing: f32, wf: f32, hf: f32, pt: &Paint, set: Set, keep: &[(f32, f32, bool)]) {
    let usable = face.len - 1.0;
    let n = (usable / spacing).floor() as i32;
    if n < 1 || floors < 1 {
        return;
    }
    let step = usable / n as f32;
    let (w, h) = ((step * wf).min(1.6), (fh * hf).min(2.2));
    let key = face_key(c, face);
    for k in 0..floors {
        let y = y0 + k as f32 * fh + fh * 0.55;
        for i in 0..n {
            let t = -usable * 0.5 + (i as f32 + 0.5) * step;
            if keep.iter().any(|(kt, kw, all)| (*all || k == 0) && (t - kt).abs() < kw + w * 0.5 + 0.3) {
                continue;
            }
            let hh = hash2_salted(key, ((k as u64) << 16) | i as u64, PANE_SALT);
            let pane = light(c, rng, 0.0);
            window(g, c, face, t, y, w, h, pane, pt, set, hh);
        }
    }
}

/// An oak door `w` by `h` at `t` in a stone surround, a light over it, a
/// stone step and a hood (detail).
pub(super) fn door(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, w: f32, h: f32, pt: &Paint, over: Pane) {
    let f = &c.frame;
    face.decal(g, f, Stone, t, h * 0.5 + 0.1, FRAME, w + 0.36, h + 0.3, pt.trim);
    face.decal(g, f, Wood, t, h * 0.5 + 0.05, PANE, w, h - 0.1, pt.door);
    face.glaze(g, c, t, h + 0.32, PANE, w * 0.8, 0.3, over);
    g.lod(Lod::Mid, |g| {
        face.bx(g, f, Stone, t, 0.08, 0.2, Vec3::new(w + 0.5, 0.16, 0.4), pt.trim);
        super::hk::wall_box(g, c, Stone, face, t, h + 0.62, 0.0, Vec3::new(w + 0.7, 0.14, 0.3), pt.trim);
    });
}

/// The points of a round arch's half circle on a face: from the left
/// springing to the right, `n` + 1 of them, radius `r` about (`t`, `y`).
fn arc(t: f32, y: f32, r: f32, n: usize) -> impl Iterator<Item = (f32, f32)> {
    (0..=n).map(move |i| {
        let a = PI - PI * i as f32 / n as f32;
        (t + r * a.cos(), y + r * a.sin())
    })
}

/// A flat half disc on a face: radius `r` over the line through (`t`,
/// `y`), `d` proud of it, in `s`.
#[allow(clippy::too_many_arguments)]
pub(super) fn half_disc(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, r: f32, d: f32, s: Surface, col: [f32; 3]) {
    let f = &c.frame;
    let ring: Vec<Vec3> = arc(t, y, r, 10).map(|(tt, yy)| f.at2(face.at(tt, d), yy)).collect();
    g.part(s).cap(&ring, f.dir3(face.out), rgba(col));
}

/// A flat disc on a face, `d` proud of it, in `s`.
#[allow(clippy::too_many_arguments)]
pub(super) fn disc(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, r: f32, d: f32, s: Surface, col: [f32; 3]) {
    let f = &c.frame;
    let ring: Vec<Vec3> = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * TAU;
            f.at2(face.at(t + r * a.cos(), d), y + r * a.sin())
        })
        .collect();
    g.part(s).cap(&ring, f.dir3(face.out), rgba(col));
}

/// An arched opening on a face: an oak gate or dark passage `w` wide whose
/// arch springs at `ys`, framed by stone voussoirs and jambs (with no
/// `arch`, a square head under a beam).
#[allow(clippy::too_many_arguments)]
pub(super) fn gateway(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, w: f32, ys: f32, fill: [f32; 3], pt: &Paint, arch: bool) {
    let f = &c.frame;
    let r = if arch { w * 0.5 } else { 0.0 };
    let head = if arch { 0.0 } else { 0.3 };
    face.decal(g, f, Stone, t, (ys + head) * 0.5, FRAME, w + 0.7, ys + head, pt.trim);
    face.decal(g, f, Wood, t, ys * 0.5, PANE, w, ys, fill);
    if arch {
        half_disc(g, c, face, t, ys, r + 0.4, FRAME, Stone, pt.trim);
        half_disc(g, c, face, t, ys, r, PANE, Wood, fill);
    }
    g.lod(Lod::Mid, |g| {
        // The keystone and the iron straps of the gate.
        face.decal(g, f, Stone, t, ys + r + 0.2, BAR, 0.5, 0.6, mul(pt.trim, 1.08));
        for k in 1..4 {
            face.decal(g, f, Matte, t, ys * k as f32 / 4.0, BAR, w * 0.9, 0.08, IRON);
        }
        face.decal(g, f, Matte, t, ys * 0.5, BAR + 0.002, 0.06, ys, IRON);
    });
}

/// A tall round-headed window `w` wide, its sides `h` high from `y0`: a
/// stone surround and the glass, stained and lit after dark (`night`) or a
/// window light.
#[allow(clippy::too_many_arguments)]
pub(super) fn arched_window(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, w: f32, h: f32, pt: &Paint, night: bool) {
    let f = &c.frame;
    let ys = y0 + h;
    face.decal(g, f, Stone, t, y0 + h * 0.5, FRAME, w + 0.4, h, pt.trim);
    half_disc(g, c, face, t, ys, w * 0.5 + 0.2, FRAME, Stone, pt.trim);
    face.decal(g, f, Glass, t, y0 + h * 0.5, PANE, w, h, c.palette.glass_dark);
    half_disc(g, c, face, t, ys, w * 0.5, PANE, Glass, c.palette.glass_dark);
    if night {
        let from = g.get(Glow).colors.len();
        let col = mul(pick(rng, &STAINED), rng.random_range(0.75..1.0));
        face.decal(g, f, Glow, t, y0 + h * 0.5, LIT_OVER, w, h, col);
        half_disc(g, c, face, t, ys, w * 0.5, LIT_OVER, Glow, col);
        mark_glow(g, from, NIGHT_MARK);
    } else if let Pane::Lit(col) = light(c, rng, 0.0) {
        window_light(g, c, |g| {
            face.decal(g, f, Glow, t, y0 + h * 0.5, LIT_OVER, w, h, col);
            half_disc(g, c, face, t, ys, w * 0.5, LIT_OVER, Glow, col);
        });
    }
    g.lod(Lod::Near, |g| {
        face.decal(g, f, Stone, t, y0 + (h + w * 0.5) * 0.5, HANDS, 0.08, h + w * 0.5, pt.trim);
        for k in 1..((h / 1.1) as i32).max(2) {
            face.decal(g, f, Matte, t, y0 + h * k as f32 / ((h / 1.1) as i32).max(2) as f32, HANDS, w, 0.05, IRON);
        }
    });
}

/// A clock on a face: a stone ring, a cream dial by day and a lit one
/// after dark, the hands and the hour marks (detail).
pub(super) fn clock_face(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, r: f32, pt: &Paint, hour: u64) {
    let f = &c.frame;
    disc(g, c, face, t, y, r + 0.22, FRAME, Stone, pt.trim);
    disc(g, c, face, t, y, r, PANE, Matte, [0.78, 0.74, 0.62]);
    let from = g.get(Glow).colors.len();
    disc(g, c, face, t, y, r, LIT_OVER, Glow, DIAL);
    mark_glow(g, from, NIGHT_MARK);
    // The hands, set to an hour of the seed's.
    let a_h = FRAC_PI_2 - (hour % 12) as f32 / 12.0 * TAU;
    let a_m = FRAC_PI_2 - ((hour >> 4) % 12) as f32 / 12.0 * TAU;
    let out = f.dir3(face.out);
    let along = f.dir3(face.tangent);
    let centre = f.at2(face.at(t, HANDS), y);
    for (a, len, wid) in [(a_h, r * 0.55, 0.12), (a_m, r * 0.85, 0.08)] {
        let dir = (along * a.cos() + Vec3::Y * a.sin()).normalize();
        let up = (Vec3::Y * a.cos() - along * a.sin()).normalize();
        g.part(Matte).decal(centre + dir * len * 0.5, dir, up, len, wid, out, rgba(IRON));
    }
    g.lod(Lod::Near, |g| {
        for k in 0..12 {
            let a = k as f32 / 12.0 * TAU;
            let p = Vec2::new(t + a.cos() * r * 0.88, y + a.sin() * r * 0.88);
            face.decal(g, f, Matte, p.x, p.y, HANDS, 0.08, 0.08, IRON);
        }
    });
}

// ── timber framing ───────────────────────────────────────────────────────────

/// One timber on a face, from (`t0`, `y0`) to (`t1`, `y1`), `w` wide.
#[allow(clippy::too_many_arguments)]
fn timber(g: &mut Geometry, c: &Ctx, face: &Face, t0: f32, y0: f32, t1: f32, y1: f32, w: f32, col: [f32; 3]) {
    let f = &c.frame;
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    let (dt, dy) = (t1 - t0, y1 - y0);
    let len = (dt * dt + dy * dy).sqrt();
    if len < 0.05 {
        return;
    }
    let dir = (along * dt + Vec3::Y * dy) / len;
    let up = (Vec3::Y * (dt / len) - along * (dy / len)).normalize();
    let centre = f.at2(face.at((t0 + t1) * 0.5, TIMBER), (y0 + y1) * 0.5);
    g.part(Wood).decal(centre, dir, up, len, w, out, rgba(col));
}

/// Black oak framing on a storey of render between `y0` and `y1`: the
/// plates and corner posts (always drawn: the look from afar), then the
/// studs, a mid rail and the braces in one of three patterns (detail).
pub(super) fn framing(g: &mut Geometry, c: &Ctx, face: &Face, y0: f32, y1: f32, pt: &Paint) {
    let len = face.len;
    if len < 1.2 || y1 - y0 < 1.0 {
        return;
    }
    let f = &c.frame;
    let col = pt.timber;
    let e = len * 0.5;
    face.decal(g, f, Wood, 0.0, y0 + 0.13, TIMBER, len, 0.26, col);
    face.decal(g, f, Wood, 0.0, y1 - 0.13, TIMBER, len, 0.26, col);
    for s in [-1.0f32, 1.0] {
        face.decal(g, f, Wood, s * (e - 0.14), (y0 + y1) * 0.5, TIMBER, 0.28, y1 - y0 - 0.52, col);
    }
    let pattern = face_key(c, face) % 3;
    g.lod(Lod::Mid, |g| {
        let n = ((len - 0.6) / if pattern == 0 { 0.7 } else { 1.3 }).floor().max(1.0) as i32;
        let step = (len - 0.56) / n as f32;
        let (lo, hi) = (y0 + 0.26, y1 - 0.26);
        for i in 1..n {
            let t = -e + 0.28 + i as f32 * step;
            timber(g, c, face, t, lo, t, hi, 0.2, col);
        }
        if pattern != 0 {
            let mid = (lo + hi) * 0.5;
            timber(g, c, face, -e + 0.28, mid, e - 0.28, mid, 0.18, col);
        }
        // Braces up from the corner posts; a cross in the middle panel.
        let b = step.min(1.6);
        timber(g, c, face, -e + 0.28, lo + 0.1, -e + 0.28 + b, hi - 0.1, 0.18, col);
        timber(g, c, face, e - 0.28, lo + 0.1, e - 0.28 - b, hi - 0.1, 0.18, col);
        if pattern == 2 && n >= 3 {
            let m = -e + 0.28 + (n / 2) as f32 * step;
            timber(g, c, face, m, lo, m + step, hi, 0.16, col);
            timber(g, c, face, m + step, lo, m, hi, 0.16, col);
        }
    });
}

// ── roofs, chimneys, lamps ───────────────────────────────────────────────────

/// A steep roof on a rectangle: two slopes `rise` high meeting at a ridge
/// along u (`along_u`) or v, the eaves `eave` out over the long walls and
/// gables in `gable` (its surface and colour) at the ends.
#[allow(clippy::too_many_arguments)]
pub(super) fn roof(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, along_u: bool, eave: f32, col: [f32; 3], gable: (Surface, [f32; 3])) {
    let f = &c.frame;
    let p = |a: f32, yy: f32, b: f32| if along_u { f.at(a, yy, b) } else { f.at(b, yy, a) };
    let d = |a: f32, b: f32| if along_u { f.dir3(Vec2::new(a, b)) } else { f.dir3(Vec2::new(b, a)) };
    let (a0, a1, b0, b1) = if along_u { (r.u0, r.u1, r.v0, r.v1) } else { (r.v0, r.v1, r.u0, r.u1) };
    let half = (b1 - b0) * 0.5;
    if half < 0.3 || a1 - a0 < 0.3 || rise < 0.1 {
        return;
    }
    let bm = (b0 + b1) * 0.5;
    let ye = y - rise * eave / half;
    for s in [-1.0f32, 1.0] {
        let be = bm + s * (half + eave);
        let n = (d(0.0, s) * rise + Vec3::Y * half).normalize();
        g.part(Panel).quad_facing([p(a0, ye, be), p(a1, ye, be), p(a1, y + rise, bm), p(a0, y + rise, bm)], n, rgba(col));
    }
    for (a, s) in [(a0, -1.0f32), (a1, 1.0)] {
        g.part(gable.0).tri_facing(p(a, y, b0), p(a, y, b1), p(a, y + rise, bm), d(s, 0.0), rgba(gable.1));
    }
    // Ridge tiles (detail).
    g.lod(Lod::Mid, |g| {
        let (am, len) = ((a0 + a1) * 0.5, a1 - a0);
        if along_u {
            f.bx(g, Panel, am, y + rise + 0.06, bm, Vec3::new(len, 0.16, 0.26), mul(col, 0.8));
        } else {
            f.bx(g, Panel, bm, y + rise + 0.06, am, Vec3::new(0.26, 0.16, len), mul(col, 0.8));
        }
    });
}

/// Crow steps up the two raking edges of a gable on `face`: the wall `y`
/// high, the gable `rise` over it, `steps` a side, in stone.
pub(super) fn crow_steps(g: &mut Geometry, c: &Ctx, face: &Face, y: f32, rise: f32, steps: i32, pt: &Paint) {
    let f = &c.frame;
    let half = face.len * 0.5;
    let sw = half / steps as f32;
    for s in [-1.0f32, 1.0] {
        for k in 0..steps {
            let outer = half - k as f32 * sw;
            let inner = outer - sw;
            let top = y + rise * (half - inner) / half + 0.35;
            let bottom = y + rise * (half - outer) / half - 0.2;
            face.bx(g, f, Stone, s * (outer + inner) * 0.5, (top + bottom) * 0.5, -0.22, Vec3::new(sw + 0.02, top - bottom, 0.5), pt.stone);
        }
    }
}

/// A stone chimney stack at lot-local `p` from `y` to `top`, with pots.
pub(super) fn chimney(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, top: f32, w: f32, pt: &Paint) {
    let f = &c.frame;
    f.bx(g, Stone, p.x, (y + top) * 0.5, p.y, Vec3::new(w, top - y, 0.8), mul(pt.stone, 0.92));
    f.bx(g, Stone, p.x, top + 0.1, p.y, Vec3::new(w + 0.18, 0.2, 0.98), pt.trim);
    g.lod(Lod::Mid, |g| {
        let n = ((w / 0.55) as i32).clamp(1, 4);
        for k in 0..n {
            let u = p.x - w * 0.5 + (k as f32 + 0.5) * w / n as f32;
            g.part(Concrete).cylinder(f.to_world(u, p.y), 0.15, 0.12, top + 0.2, top + 0.8, 6, rgba([0.50, 0.30, 0.22]));
        }
    });
}

/// A gas lantern on an iron bracket off a face at height `y`: lit after
/// dark.
pub(super) fn wall_lantern(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32) {
    let f = &c.frame;
    g.lod(Lod::Mid, |g| {
        face.bx(g, f, Matte, t, y + 0.42, 0.32, Vec3::new(0.05, 0.05, 0.64), IRON);
        face.bx(g, f, Matte, t, y + 0.2, 0.08, Vec3::new(0.05, 0.4, 0.1), IRON);
    });
    face.bx(g, f, Matte, t, y + 0.3, 0.62, Vec3::new(0.34, 0.08, 0.34), IRON);
    let from = g.get(Glow).colors.len();
    face.bx(g, f, Glow, t, y, 0.62, Vec3::new(0.24, 0.44, 0.24), GAS);
    mark_glow(g, from, NIGHT_MARK);
}

/// A cast-iron gas lamp post on the lot (skipped near the edge), lit after
/// dark, a pool of warm light under it.
pub(super) fn lamp_post(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 0.6 {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Matte).cylinder(w, 0.17, 0.13, 0.0, 0.7, 8, rgba(IRON));
    g.part(Matte).cylinder(w, 0.08, 0.06, 0.7, h, 6, rgba(IRON));
    f.bx(g, Matte, p.x, h + 0.62, p.y, Vec3::new(0.46, 0.1, 0.46), IRON);
    let from = g.get(Glow).colors.len();
    f.bx(g, Glow, p.x, h + 0.3, p.y, Vec3::new(0.32, 0.5, 0.32), GAS);
    let r = (h * 0.5).min(distance_to_boundary(p, &c.lot_poly) - 0.2);
    if r > 1.0 {
        ground(g, f, Glow, &ngon(p.x, p.y, r, 12, 0.0), 0.075, mul(GAS, 0.08));
    }
    mark_glow(g, from, NIGHT_MARK);
    g.lod(Lod::Mid, |g| {
        // The ladder bar the lamplighter leaned on.
        f.bx(g, Matte, p.x, h - 0.4, p.y, Vec3::new(0.7, 0.05, 0.05), IRON);
    });
}

/// Setts: a cobbled yard on `poly` at `y` (a darker stone, the joints
/// drawn as lines across it close up).
pub(super) fn cobbles(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, pt: &Paint) {
    if poly.len() < 3 {
        return;
    }
    let f = &c.frame;
    ground(g, f, Paving, poly, y, mul(mix(pt.stone, [0.30, 0.29, 0.28], 0.5), 0.8));
    let (lo, hi) = bounds(poly);
    g.lod(Lod::Near, |g| {
        let n = (((hi.y - lo.y) / 1.6) as i32).min(14);
        for k in 1..n {
            let v = lo.y + k as f32 * 1.6;
            if let Some((a, b)) = common_span(poly, v - 0.05, v + 0.05, false) {
                if b - a > 1.0 {
                    f.flat(g, Matte, (a + b) * 0.5, y + 0.012, v, b - a - 0.4, 0.06, 0.0, mul(pt.dark, 0.6));
                }
            }
        }
    });
}

// ── a building range ─────────────────────────────────────────────────────────

/// One range of an old building: a stone ground floor, stone or timber
/// framing above (jettied out over the front by `jetty`), a steep roof.
#[derive(Clone, Copy)]
pub(super) struct Range {
    pub r: R,
    pub floors: i32,
    pub fh: f32,
    /// Timber-framed render above a stone ground floor.
    pub timber: bool,
    pub jetty: f32,
    /// The ridge along u (eaves to the street) or v (a gable to it).
    pub along_u: bool,
    pub rise: f32,
    pub eave: f32,
    /// Window bay spacing.
    pub bay: f32,
    /// Faces (front, right, back, left) with no windows: party walls.
    pub blank: [bool; 4],
}

/// A span of a range's face kept clear of windows (a door, a gate, a bay).
#[derive(Clone, Copy)]
pub(super) struct Keep {
    /// The face: front, right, back, left.
    pub face: usize,
    pub t: f32,
    pub half: f32,
    /// Every storey (a gate's arch, loading doors) or the bottom one.
    pub all: bool,
}

impl Keep {
    /// A door on the front's bottom storey.
    pub fn door(t: f32, half: f32) -> Self {
        Keep { face: 0, t, half, all: false }
    }
}

/// Draws a [`Range`]: the walls, the framing, the windows (none in the
/// spans in `keep`), the roof and its gables, the plinth and the quoins
/// (detail). Returns the eaves height.
pub(super) fn range(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, rg: &Range, pt: &Paint, keep: &[Keep]) -> f32 {
    let f = &c.frame;
    let r = rg.r;
    let fh = rg.fh;
    let h = rg.floors as f32 * fh;
    let upper = R::new(r.u0, r.u1, r.v0 - rg.jetty, r.v1);
    let timber = rg.timber && rg.floors >= 2;
    if timber {
        block(g, c, Stone, &r, 0.0, fh, pt.stone);
        block(g, c, Plaster, &upper, fh, h, pt.render);
        // The jetty's bressummer, proud of the wall and past its ends.
        if rg.jetty > 0.0 {
            upper.front().bx(g, f, Wood, 0.0, fh + 0.1, 0.07, Vec3::new(upper.w() + 0.06, 0.26, 0.14), pt.timber);
        }
    } else {
        block(g, c, Stone, &r, 0.0, h, pt.stone);
    }
    let set = if timber { Set::Oak } else { Set::Stone };
    for (i, face) in r.faces().iter().enumerate() {
        if face.len < 1.5 {
            continue;
        }
        let up_face = upper.faces()[i];
        if !rg.blank[i] {
            // The plinth, the face's length exactly: a neighbour's in
            // another stone meets it end to end, never over it. Its back is
            // sunk in the wall, clear of the backs of steps and bays.
            face.bx(g, f, Stone, 0.0, 0.25, 0.02, Vec3::new(face.len, 0.5, 0.08), pt.dark);
            let kp: Vec<(f32, f32, bool)> = keep.iter().filter(|k| k.face == i).map(|k| (k.t, k.half, k.all)).collect();
            let ka: Vec<(f32, f32, bool)> = kp.iter().copied().filter(|k| k.2).collect();
            window_rows(g, c, rng, face, 0.0, 1, fh, rg.bay, 0.42, 0.48, pt, Set::Stone, &kp);
            window_rows(g, c, rng, &up_face, fh, rg.floors - 1, fh, rg.bay, 0.42, 0.5, pt, set, &ka);
        }
        if timber {
            for k in 1..rg.floors {
                framing(g, c, &up_face, k as f32 * fh, (k + 1) as f32 * fh, pt);
            }
        }
        if !rg.blank[i] {
            grime(g, c, rng, face, 0.0, if timber { fh } else { h }, 1.0 + 2.0 * c.wear);
        }
    }
    // Quoins up the corners of the stone (detail).
    g.lod(Lod::Mid, |g| {
        let top = if timber { fh } else { h };
        let n = ((top - 0.5) / 0.6) as i32;
        for (i, face) in r.faces().iter().enumerate() {
            if rg.blank[i] || face.len < 2.0 {
                continue;
            }
            for s in [-1.0f32, 1.0] {
                for k in 0..n {
                    let w = if k % 2 == 0 { 0.7 } else { 0.42 };
                    super::hk::wall_box(g, c, Stone, face, s * (face.len * 0.5 - w * 0.5), 0.55 + k as f32 * 0.6 + 0.27, 0.0, Vec3::new(w, 0.5, QUOIN), pt.trim);
                }
            }
        }
    });
    let gable = if timber { (Plaster, pt.render) } else { (Stone, pt.stone) };
    roof(g, c, &upper, h, rg.rise, rg.along_u, rg.eave, pt.roof, gable);
    h
}

/// A steep pitch for a range `span` across: about fifty degrees.
pub(super) fn steep(span: f32) -> f32 {
    (span * 0.6).clamp(2.0, 7.5)
}

/// Something old that fits any scrap of a lot: a one- or two-storey stone
/// cottage under a steep roof, a door, a window and a lantern.
pub(super) fn cottage(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pt: &Paint) {
    let Some(r) = fit(c, R::of(&c.poly).inset(0.3), 2.0) else { return };
    let fh = c.fh().clamp(2.7, 3.2);
    let floors = if r.w().min(r.d()) > 4.0 && c.floors > 1 { 2 } else { 1 };
    let along_u = r.w() >= r.d();
    let span = if along_u { r.d() } else { r.w() };
    let rg = Range { r, floors, fh, timber: floors > 1 && coin(rng, 0.5), jetty: 0.0, along_u, rise: steep(span).min(4.0), eave: 0.0, bay: 2.4, blank: [false; 4] };
    let front = r.front();
    let keep = [Keep::door(-front.len * 0.2, 0.5)];
    range(g, c, rng, &rg, pt, &keep);
    if front.len > 1.6 {
        door(g, c, &front, -front.len * 0.2, 0.9f32.min(front.len * 0.3), 2.0, pt, Pane::Dark);
    }
    wall_lantern(g, c, &front, (front.len * 0.1).min(0.9), 2.4);
}

// ── stone and timber-frame houses ────────────────────────────────────────────

/// One to three town houses along the street: a stone ground floor and
/// either stone or a jettied timber-framed storey or two over it, a steep
/// tile or stone-slate roof with its ridge along the street, a dormer, a
/// chimney at the gable, an oak door under a hood with a gas lantern by it,
/// a low wall round the front garden and a garden behind with a tree.
pub fn stone_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let fh = c.fh().clamp(2.9, 3.3);
    let floors = (c.floors as i32).clamp(2, 3);
    let area = R::of(&c.poly);
    let depth = (area.d() * 0.45).clamp(6.5, 10.0).min(area.d() - 1.6);
    let Some(site) = fit(c, R::new(area.u0 + 0.4, area.u1 - 0.4, area.v0 + 1.4, area.v0 + 1.4 + depth), 5.0) else {
        let pt = paint(c, rng);
        cottage(g, c, rng, &pt);
        return;
    };
    let n = ((site.w() / 9.5).round() as i32).clamp(1, 3);
    let hw = site.w() / n as f32;
    let detached = n > 1 && coin(rng, 0.5);
    let gap = if detached { 1.4 } else { 0.0 };
    for i in 0..n {
        let pt = paint(c, rng);
        let hr = R::new(site.u0 + i as f32 * hw + gap * 0.5, site.u0 + (i + 1) as f32 * hw - gap * 0.5, site.v0, site.v1);
        let timber = coin(rng, 0.6);
        let jetty = if timber { 0.45 } else { 0.0 };
        let rise = steep(hr.d()) * rng.random_range(0.85..1.0);
        let blank = [false, !detached && i + 1 < n, false, !detached && i > 0];
        let front = hr.front();
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dt = side * (hw * 0.25).min(2.4);
        let rg = Range { r: hr, floors, fh, timber, jetty, along_u: true, rise, eave: 0.35, bay: 2.4, blank };
        let h = range(g, c, rng, &rg, &pt, &[Keep::door(dt, 0.6)]);
        door(g, c, &front, dt, 1.0, 2.2, &pt, light(c, rng, 0.0));
        wall_lantern(g, c, &front, dt + side * -1.0, 2.5);
        // A dormer on the front slope.
        if coin(rng, 0.55) && hw > 5.0 {
            let half = hr.d() * 0.5 + jetty * 0.5;
            let v0 = hr.v0 - jetty + 0.5;
            let dh = 1.7f32.min(rise * 0.6);
            let back = v0 + dh * half / rise + 0.4;
            let dr = R::new(hr.c().x - 0.9, hr.c().x + 0.9, v0, back.min(hr.c().y));
            if dr.d() > 0.6 {
                let col = if timber { pt.render } else { pt.stone };
                block(g, c, if timber { Plaster } else { Stone }, &dr, h - 0.2, h + dh, col);
                roof(g, c, &dr, h + dh, 0.7, false, 0.1, pt.roof, (if timber { Plaster } else { Stone }, col));
                window(g, c, &dr.front(), 0.0, h + dh * 0.5, 0.9, dh * 0.55, light(c, rng, 0.0), &pt, Set::Oak, i as u64);
            }
        }
        // The chimney at the outer gable.
        let cu = if side < 0.0 { hr.u0 + 0.6 } else { hr.u1 - 0.6 };
        chimney(g, c, Vec2::new(cu, hr.c().y - jetty * 0.5), h + rise - 1.2, h + rise + 1.2, 1.0, &pt);
    }
    // The front garden wall, a gap at each door, and the gardens behind.
    let pt = paint(c, rng);
    let fv = area.v0 + 0.5;
    if site.v0 - area.v0 > 1.2 {
        let fr = Face::new(Vec2::new(site.u0, fv), Vec2::new(site.u1, fv), Vec2::new(0.0, -1.0));
        for k in 0..n {
            let a = -fr.len * 0.5 + k as f32 * hw + 0.2;
            let b = a + hw - 0.4;
            let mid = (a + b) * 0.5;
            for (s0, s1) in [(a, mid - 0.7), (mid + 0.7, b)] {
                if s1 - s0 > 0.4 && c.spans_lot(fr.at(s0, 0.0), fr.at(s1, 0.0)) {
                    fr.bx(g, f, Stone, (s0 + s1) * 0.5, 0.45, 0.0, Vec3::new(s1 - s0, 0.9, 0.4), pt.stone);
                }
            }
        }
    }
    let back = R::new(site.u0, site.u1, site.v1 + 0.4, area.v1 - 0.3);
    if back.d() > 3.0 {
        let gp = clip_box(&c.poly, back.u0, back.u1, back.v0, back.v1);
        if gp.len() >= 3 {
            ground(g, f, Foliage, &gp, 0.04, mix(c.palette.foliage, [0.30, 0.36, 0.20], 0.5));
            for _ in 0..n.min(2) {
                if let Some(p) = random_in_poly(rng, &gp, 2.0) {
                    tree_in(g, f, c.palette, p, rng.random_range(5.0..8.5), rng.random_range(1.6..2.6), c.palette.foliage);
                }
            }
            g.lod(Lod::Mid, |g| {
                for face in c.faces(&gp) {
                    if face.len > 1.0 && face.out.y > 0.5 {
                        face.bx(g, f, Stone, 0.0, 0.8, -0.2, Vec3::new(face.len, 1.6, 0.36), mul(pt.stone, 0.9));
                    }
                }
            });
        }
    }
}

// ── narrow gabled rows ───────────────────────────────────────────────────────

/// A row of narrow tall houses with their gables to the street, each its
/// own height and its own stone or render, some with crow steps, a hoist
/// beam in the gable and an attic window; shops on the ground floor off a
/// residential street, with painted boards lit after dark; yards behind.
pub fn gabled_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let fh = c.fh().clamp(2.9, 3.4);
    let base = (c.floors as i32).clamp(2, 4);
    let area = R::of(&c.poly);
    let depth = (area.d() * 0.6).clamp(8.0, 13.0).min(area.d() - 0.6);
    let Some(row) = fit(c, R::new(area.u0 + 0.2, area.u1 - 0.2, area.v0 + 0.3, area.v0 + 0.3 + depth), 4.0) else {
        let pt = paint(c, rng);
        cottage(g, c, rng, &pt);
        return;
    };
    let n = ((row.w() / 5.4).round() as i32).max(1);
    let hw = row.w() / n as f32;
    let shops = c.land_use != LandUse::Residential;
    for i in 0..n {
        let pt = paint(c, rng);
        let floors = (base + [-1, 0, 0, 1][rng.random_range(0..4)]).clamp(2, 5);
        let hr = R::new(row.u0 + i as f32 * hw, row.u0 + (i + 1) as f32 * hw, row.v0, row.v1);
        let timber = coin(rng, 0.35);
        let rise = (hw * 0.95).clamp(2.4, 5.5);
        let blank = [false, i + 1 < n, false, i > 0];
        let front = hr.front();
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dt = side * hw * 0.3;
        let rg = Range { r: hr, floors, fh, timber, jetty: 0.0, along_u: false, rise, eave: 0.0, bay: 2.3, blank };
        let keep = if shops { [Keep::door(0.0, hw * 0.5)] } else { [Keep::door(dt, 0.6)] };
        let h = range(g, c, rng, &rg, &pt, &keep);
        if shops && hw > 3.2 {
            // A shop: a wide window, the door beside it, the painted board.
            window(g, c, &front, -side * hw * 0.18, fh * 0.45, (hw * 0.42).min(2.6), fh * 0.5, light(c, rng, 0.25), &pt, Set::Oak, i as u64);
            door(g, c, &front, side * hw * 0.32, 0.9, 2.2, &pt, light(c, rng, 0.0));
            let board = pick(rng, &[[0.12, 0.20, 0.15], [0.30, 0.09, 0.07], [0.10, 0.10, 0.12], [0.20, 0.16, 0.10]]);
            front.decal(g, f, Wood, 0.0, fh - 0.25, TIMBER, hw - 0.6, 0.42, board);
            let from = g.get(Glow).colors.len();
            let letters = (((hw - 1.4) / 0.42) as i32).clamp(2, 9);
            let lw = (hw - 1.4) / letters as f32;
            for k in 0..letters {
                front.decal(g, f, Glow, -(hw - 1.4) * 0.5 + (k as f32 + 0.5) * lw, fh - 0.25, FRAME, lw * 0.62, 0.24, mul(GILT, 1.6));
            }
            mark_glow(g, from, NEON_MARK);
            if coin(rng, 0.5) {
                // A hung sign on an iron bracket.
                g.lod(Lod::Mid, |g| front.bx(g, f, Matte, side * hw * 0.42, fh + 0.5, 0.45, Vec3::new(0.05, 0.05, 0.9), IRON));
                let p = front.at(side * hw * 0.42, 0.75);
                f.bx_yaw(g, Wood, p.x, fh + 0.1, p.y, Vec3::new(0.7, 0.6, 0.06), board, front.yaw() + FRAC_PI_2);
            }
        } else {
            door(g, c, &front, dt, 0.95, 2.2, &pt, light(c, rng, 0.0));
        }
        if i == n / 2 || coin(rng, 0.35) {
            wall_lantern(g, c, &front, dt - side * 1.0, 2.6);
        }
        // The gable: an attic window, crow steps on a stone one, a hoist.
        window(g, c, &front, 0.0, h + rise * 0.32, (hw * 0.2).min(0.9), (rise * 0.28).min(1.3), light(c, rng, 0.0), &pt, if timber { Set::Oak } else { Set::Stone }, i as u64 + 7);
        if !timber && coin(rng, 0.45) && rise > 2.6 {
            crow_steps(g, c, &front, h, rise, ((hw / 1.1) as i32).clamp(2, 5), &pt);
        } else if coin(rng, 0.4) && c.reach_out(&front) > 1.0 {
            g.lod(Lod::Mid, |g| front.bx(g, f, Wood, 0.0, h + rise * 0.72, 0.4, Vec3::new(0.2, 0.2, 0.8), pt.timber));
        }
        if i > 0 {
            chimney(g, c, Vec2::new(hr.u0, hr.v1 - 1.2), h - 0.4, h + rise * 0.6, 0.9, &pt);
        }
    }
    // Yards behind: walls between, flagstones.
    let pt = paint(c, rng);
    let back_room = area.v1 - row.v1;
    if back_room > 3.0 {
        let yd = back_room.min(8.0) - 0.4;
        for i in 0..=n {
            let u = row.u0 + i as f32 * hw;
            let (a, b) = (Vec2::new(u, row.v1 + 0.15), Vec2::new(u, row.v1 + yd));
            if c.inside_local(a) && c.inside_local(b) {
                f.bx(g, Stone, u, 0.9, (a.y + b.y) * 0.5, Vec3::new(0.3, 1.8, yd - 0.15), mul(pt.stone, 0.9));
            }
        }
        let yp = clip_box(&c.poly, row.u0, row.u1, row.v1 + 0.05, row.v1 + yd);
        cobbles(g, c, &yp, 0.04, &pt);
    }
}

// ── courtyard houses ─────────────────────────────────────────────────────────

/// A house round a courtyard: a front range with an arched carriage gate,
/// side wings and (on a deep lot) a back range, all under steep roofs; the
/// courtyard cobbled, a well or a tree in it, a lamp; windows inside and
/// out. Also the merchant's house of a shopping street.
pub fn courtyard_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.0, 3.5);
    let floors = (c.floors as i32).clamp(2, 3);
    let area = R::of(&c.poly);
    let Some(site) = fit(c, area.inset(0.4), 13.0) else {
        gabled_row(g, c, rng);
        return;
    };
    let rd = (site.w().min(site.d()) * 0.28).clamp(5.0, 8.0);
    let timber = coin(rng, 0.4);
    let front = R::new(site.u0, site.u1, site.v0, site.v0 + rd);
    let back_ok = site.d() > 2.0 * rd + 5.0;
    let wing_v1 = if back_ok { site.v1 - rd } else { (site.v0 + rd + 9.0).min(site.v1) };
    let ranges = [
        (front, true),
        (R::new(site.u0, site.u0 + rd, front.v1, wing_v1), false),
        (R::new(site.u1 - rd, site.u1, front.v1, wing_v1), false),
    ];
    let gw = 3.2f32.min(front.w() * 0.25);
    let gt = if coin(rng, 0.5) { 0.0 } else { -front.w() * 0.25 };
    let mut eaves = 0.0f32;
    for (k, (r, along_u)) in ranges.iter().enumerate() {
        if r.w() < 3.0 || r.d() < 3.0 {
            continue;
        }
        let span = if *along_u { r.d() } else { r.w() };
        // A wing's ends butt on the front and back ranges: no windows there.
        let blank = if k == 0 { [false; 4] } else { [true, false, back_ok, false] };
        let rg = Range { r: *r, floors, fh, timber: timber && k == 0 || timber && coin(rng, 0.5), jetty: 0.0, along_u: *along_u, rise: steep(span), eave: if *along_u { 0.3 } else { 0.0 }, bay: 2.6, blank };
        let keep: &[Keep] = if k == 0 { &[Keep { face: 0, t: gt, half: gw * 0.5 + 0.5, all: true }, Keep { face: 2, t: -gt, half: gw * 0.5 + 0.5, all: true }] } else { &[] };
        eaves = eaves.max(range(g, c, rng, &rg, &pt, keep));
    }
    if back_ok {
        let r = R::new(site.u0, site.u1, site.v1 - rd, site.v1);
        let rg = Range { r, floors, fh, timber: false, jetty: 0.0, along_u: true, rise: steep(rd), eave: 0.3, bay: 2.6, blank: [false; 4] };
        range(g, c, rng, &rg, &pt, &[]);
    }
    // The carriage gate through the front range, and its inner side.
    let ff = front.front();
    let ys = (fh * 1.2).min(eaves - 1.2 - gw * 0.5).max(2.2);
    gateway(g, c, &ff, gt, gw, ys, mul(pt.door, 0.8), &pt, true);
    wall_lantern(g, c, &ff, gt + gw * 0.5 + 0.9, 2.8);
    let inner = front.back();
    gateway(g, c, &inner, -gt, gw, ys, mul(pt.door, 0.8), &pt, true);
    // Chimneys on the front range.
    let ridge = eaves + steep(rd);
    for u in [front.u0 + rd * 0.5, front.u1 - rd * 0.5] {
        chimney(g, c, Vec2::new(u, front.c().y), ridge - 1.4, ridge + 1.0, 1.1, &pt);
    }
    // The courtyard: setts, a well or a tree, a lamp.
    let court = R::new(site.u0 + rd, site.u1 - rd, front.v1, wing_v1);
    if court.w() > 2.0 && court.d() > 2.0 {
        cobbles(g, c, &court.poly(), 0.04, &pt);
        let p = court.c();
        if coin(rng, 0.5) || court.w().min(court.d()) < 6.0 {
            let w = f.to_world(p.x, p.y);
            g.part(Stone).cylinder_capped(w, 0.9, 0.9, 0.0, 0.85, 12, false, true, rgba(pt.stone));
            ground(g, f, Matte, &ngon(p.x, p.y, 0.7, 12, 0.0), 0.86, DARK);
            g.lod(Lod::Mid, |g| {
                for s in [-1.0f32, 1.0] {
                    f.bx(g, Wood, p.x + s * 0.8, 1.6, p.y, Vec3::new(0.14, 1.5, 0.14), pt.timber);
                }
                roof(g, c, &R::new(p.x - 1.0, p.x + 1.0, p.y - 0.6, p.y + 0.6), 2.35, 0.6, true, 0.1, pt.roof, (Wood, pt.timber));
            });
        } else {
            tree_in(g, f, c.palette, p, rng.random_range(6.0..9.0), (court.w().min(court.d()) * 0.3).clamp(1.5, 3.0), c.palette.foliage);
        }
        lamp_post(g, c, Vec2::new(court.u0 + 1.2, court.v1 - 1.2), 3.4);
    }
    // A garden behind an open U.
    if !back_ok {
        let gp = clip_box(&c.poly, site.u0, site.u1, wing_v1 + 0.3, area.v1 - 0.3);
        if gp.len() >= 3 && R::of(&gp).d() > 2.0 {
            ground(g, f, Foliage, &gp, 0.04, mix(c.palette.foliage, [0.30, 0.36, 0.20], 0.5));
        }
    }
}

// ── the market square ────────────────────────────────────────────────────────

/// Market-square arcades: the upper storeys carried over the pavement on
/// stone piers and round arches, shops in the arcade behind with painted
/// boards and lit windows, lanterns hung in the bays after dark; render or
/// stone above under a steep roof with dormers.
pub fn market_arcade(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.0, 3.5);
    let floors = (c.floors as i32).clamp(2, 4);
    let area = R::of(&c.poly);
    let depth = (area.d() * 0.6).clamp(9.0, 16.0).min(area.d() - 0.6);
    let Some(r) = fit(c, R::new(area.u0 + 0.3, area.u1 - 0.3, area.v0 + 0.3, area.v0 + 0.3 + depth), 7.0) else {
        gabled_row(g, c, rng);
        return;
    };
    let ad = 3.0f32.min(r.d() * 0.3);
    let ah = fh * 1.25;
    let top = ah + (floors - 1) as f32 * fh;
    let timber = coin(rng, 0.4);
    // The upper storeys over the whole footprint, the shops set back under.
    let shops = R::new(r.u0, r.u1, r.v0 + ad, r.v1);
    block(g, c, Stone, &shops, 0.0, ah, pt.stone);
    let (us, ucol) = if timber { (Plaster, pt.render) } else { (Stone, pt.stone) };
    block(g, c, us, &r, ah, top, ucol);
    cornice(g, c, &r.poly(), ah + 0.1, 0.25, 0.06, Stone, pt.trim);
    // Piers and arches.
    // Bays of at most 3.8 m: round arches that spring well above the ground.
    let n = ((r.w() - 0.7) / 3.8).ceil().max(1.0) as i32;
    let step = (r.w() - 0.7) / n as f32;
    let front = r.front();
    for k in 0..=n {
        let u = r.u0 + 0.35 + k as f32 * step;
        f.bx(g, Stone, u, ah * 0.5, r.v0 + 0.35, Vec3::new(0.7, ah, 0.7), pt.stone);
        // The impost at the springing.
        g.lod(Lod::Mid, |g| f.bx(g, Stone, u, ah - (step - 0.7) * 0.5, r.v0 + 0.35, Vec3::new(0.86, 0.18, 0.86), pt.trim));
    }
    let col = rgba(pt.stone);
    let out = f.dir3(front.out);
    for k in 0..n {
        let t0 = -front.len * 0.5 + 0.7 + k as f32 * step;
        let t1 = t0 + step - 0.7;
        let ra = (t1 - t0) * 0.5;
        let ys = ah - ra;
        let pts: Vec<(f32, f32)> = arc((t0 + t1) * 0.5, ys, ra, 8).collect();
        let at = |t: f32, y: f32| f.at2(front.at(t, 0.0), y);
        for (corner, part) in [(t0, &pts[..=4]), (t1, &pts[4..])] {
            for w in part.windows(2) {
                g.part(Stone).tri_facing(at(corner, ah), at(w[0].0, w[0].1), at(w[1].0, w[1].1), out, col);
            }
        }
        g.lod(Lod::Mid, |g| front.decal(g, f, Stone, (t0 + t1) * 0.5, ah - 0.18, HEAD, 0.45, 0.36, pt.trim));
        // A lantern hung in every other bay, lit after dark.
        if k % 2 == 0 {
            let p = front.at((t0 + t1) * 0.5, -ad * 0.5);
            g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x, ah - 0.45, p.y, Vec3::new(0.04, 0.9, 0.04), IRON));
            let from = g.get(Glow).colors.len();
            f.bx(g, Glow, p.x, ah - 1.1, p.y, Vec3::new(0.3, 0.42, 0.3), GAS);
            mark_glow(g, from, NIGHT_MARK);
        }
    }
    // The arcade floor and the shopfronts behind it.
    ground(g, f, Paving, &R::new(r.u0, r.u1, r.v0, r.v0 + ad).poly(), 0.05, mul(pt.trim, 0.8));
    let sf = shops.front();
    let from = g.get(Glow).colors.len();
    for k in 0..n {
        let t = -sf.len * 0.5 + 0.35 + (k as f32 + 0.5) * step;
        let shop = Paint { door: pick(rng, &DOORS), ..pt };
        window(g, c, &sf, t - step * 0.18, fh * 0.48, step * 0.38, fh * 0.55, light(c, rng, 0.3), &shop, Set::Oak, k as u64);
        door(g, c, &sf, t + step * 0.3, 0.8, 2.2, &shop, Pane::Dark);
        let board = pick(rng, &[[0.12, 0.20, 0.15], [0.30, 0.09, 0.07], [0.10, 0.10, 0.12], [0.24, 0.18, 0.10]]);
        sf.decal(g, f, Wood, t, ah - 0.45, TIMBER, step * 0.8, 0.45, board);
        let letters = ((step * 0.6 / 0.42) as i32).clamp(2, 8);
        let lw = step * 0.6 / letters as f32;
        for i in 0..letters {
            sf.decal(g, f, Glow, t - step * 0.3 + (i as f32 + 0.5) * lw, ah - 0.45, FRAME, lw * 0.62, 0.26, mul(GILT, 1.5));
        }
    }
    mark_glow(g, from, NEON_MARK);
    // The storeys above.
    let set = if timber { Set::Oak } else { Set::Stone };
    for (i, face) in r.faces().iter().enumerate() {
        if face.len < 2.0 {
            continue;
        }
        window_rows(g, c, rng, face, ah, floors - 1, fh, 2.6, 0.42, 0.5, &pt, set, &[]);
        if timber {
            for k in 0..floors - 1 {
                framing(g, c, face, ah + k as f32 * fh, ah + (k + 1) as f32 * fh, &pt);
            }
        }
        if i != 0 {
            let sface = shops.faces()[i];
            if i == 2 {
                window_rows(g, c, rng, &sface, 0.0, 1, fh, 3.0, 0.4, 0.45, &pt, Set::Stone, &[]);
            }
            grime(g, c, rng, &sface, 0.0, ah, 1.0 + 2.0 * c.wear);
        }
    }
    let rise = steep(r.d()).min(6.0);
    roof(g, c, &r, top, rise, true, 0.35, pt.roof, (us, ucol));
    // Dormers on the street slope, chimneys on the ridge.
    let half = r.d() * 0.5;
    for k in 0..(n / 2).min(4) {
        let u = r.u0 + (k as f32 + 0.5) * r.w() / (n / 2) as f32;
        let dh = 1.6f32.min(rise * 0.5);
        let dr = R::new(u - 0.85, u + 0.85, r.v0 + 0.5, (r.v0 + 0.9 + dh * half / rise).min(r.c().y));
        if dr.d() > 0.6 {
            block(g, c, us, &dr, top - 0.2, top + dh, ucol);
            roof(g, c, &dr, top + dh, 0.7, false, 0.1, pt.roof, (us, ucol));
            window(g, c, &dr.front(), 0.0, top + dh * 0.5, 0.9, dh * 0.55, light(c, rng, 0.0), &pt, Set::Oak, k as u64);
        }
    }
    for u in [r.u0 + 1.2, r.u1 - 1.2] {
        chimney(g, c, Vec2::new(u, r.c().y), top + rise - 1.2, top + rise + 1.0, 1.0, &pt);
    }
    // A yard behind with a lamp.
    let yp = clip_box(&c.poly, r.u0, r.u1, r.v1 + 0.3, area.v1 - 0.3);
    if yp.len() >= 3 && R::of(&yp).d() > 2.5 {
        cobbles(g, c, &yp, 0.04, &pt);
        let (lo, hi) = bounds(&yp);
        lamp_post(g, c, Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5), 3.4);
    }
}

// ── the guild hall ───────────────────────────────────────────────────────────

/// The guild hall: a tall stone hall on a stepped plinth with its
/// crow-stepped gable to the square, tall round-headed windows between
/// buttresses, a lit rose window in the gable, great doors under an arch,
/// guild banners, a gilt name band, lanterns at the door and a lantern
/// turret on the ridge.
pub fn guild_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.6, 4.4);
    let floors = (c.floors as i32).clamp(2, 3);
    let h = floors as f32 * fh + 1.0;
    let area = R::of(&c.poly);
    let w = (area.w() - 1.2).clamp(8.0, 22.0);
    let d = (area.d() * 0.72).clamp(10.0, 30.0).min(area.d() - 2.0);
    let Some(r) = fit(c, R::new(area.c().x - w * 0.5, area.c().x + w * 0.5, area.v0 + 1.6, area.v0 + 1.6 + d), 7.0) else {
        market_arcade(g, c, rng);
        return;
    };
    let poly = r.poly();
    block(g, c, Stone, &r, 0.0, h, pt.stone);
    plinth(g, c, &poly, 2, 0.22, 0.35, Stone, pt.trim);
    cornice(g, c, &poly, fh + 0.4, 0.25, 0.08, Stone, pt.trim);
    cornice(g, c, &poly, h - 0.15, 0.35, 0.2, Stone, pt.trim);
    let rise = steep(r.w()).min(8.5);
    roof(g, c, &r, h, rise, false, 0.3, pt.roof, (Stone, pt.stone));
    let front = r.front();
    crow_steps(g, c, &front, h, rise, ((r.w() / 1.6) as i32).clamp(3, 7), &pt);
    crow_steps(g, c, &r.back(), h, rise, ((r.w() / 1.6) as i32).clamp(3, 7), &pt);
    // The rose window, lit after dark.
    let ry = h + rise * 0.36;
    let rr = (rise * 0.26).min(r.w() * 0.16).min(2.0);
    if rr > 0.5 {
        disc(g, c, &front, 0.0, ry, rr + 0.3, FRAME, Stone, pt.trim);
        disc(g, c, &front, 0.0, ry, rr, PANE, Glass, c.palette.glass_dark);
        let from = g.get(Glow).colors.len();
        disc(g, c, &front, 0.0, ry, rr, LIT_OVER, Glow, mul(pick(rng, &STAINED), 0.9));
        mark_glow(g, from, NIGHT_MARK);
        g.lod(Lod::Near, |g| {
            for k in 0..4 {
                let a = k as f32 / 8.0 * TAU;
                let along = f.dir3(front.tangent);
                let dir = (along * a.cos() + Vec3::Y * a.sin()).normalize();
                let up = (Vec3::Y * a.cos() - along * a.sin()).normalize();
                g.part(Stone).decal(f.at2(front.at(0.0, HANDS), ry), dir, up, rr * 2.0, 0.1, f.dir3(front.out), rgba(pt.trim));
            }
        });
    }
    // Great doors under an arch, the gilt band, lanterns, banners.
    let dw = (front.len * 0.18).clamp(1.8, 3.0);
    gateway(g, c, &front, 0.0, dw, 2.8, mul(pt.door, 0.9), &pt, true);
    for s in [-1.0f32, 1.0] {
        wall_lantern(g, c, &front, s * (dw * 0.5 + 1.0), 2.7);
    }
    let bw = (front.len * 0.6).min(12.0);
    front.decal(g, f, Stone, 0.0, h - 1.1, HEAD, bw, 0.8, mul(pt.trim, 0.85));
    let letters = ((bw / 0.75) as i32).clamp(4, 14);
    for i in 0..letters {
        let lt = -bw * 0.45 + (i as f32 + 0.5) * bw * 0.9 / letters as f32;
        front.decal(g, f, Matte, lt, h - 1.1, TIMBER, bw * 0.9 / letters as f32 * 0.6, 0.46, GILT);
    }
    let banner = pick(rng, &[[0.50, 0.08, 0.06], [0.10, 0.18, 0.40], [0.12, 0.30, 0.16], [0.46, 0.34, 0.08]]);
    // Tall windows between buttresses down the sides and on the front.
    for (i, face) in r.faces().iter().enumerate() {
        if face.len < 3.0 || i == 2 {
            continue;
        }
        let nb = ((face.len - 1.0) / 3.6).floor().max(1.0) as i32;
        let step = (face.len - 1.0) / nb as f32;
        for k in 0..nb {
            let t = -(face.len - 1.0) * 0.5 + (k as f32 + 0.5) * step;
            if i == 0 && t.abs() < dw * 0.5 + 1.6 {
                continue;
            }
            let ww = (step * 0.36).min(1.5);
            let wh = (h - fh - 2.2).max(1.8);
            arched_window(g, c, rng, face, t, fh + 0.9, ww, wh, &pt, false);
            window(g, c, face, t, fh * 0.5, ww, fh * 0.4, light(c, rng, 0.0), &pt, Set::Stone, k as u64);
            if i != 0 && k + 1 < nb {
                g.lod(Lod::Mid, |g| super::hk::wall_box(g, c, Stone, face, t + step * 0.5, h * 0.45, 0.0, Vec3::new(0.6, h * 0.9, 0.5), pt.dark));
            }
            if i == 0 && (k == 0 || k + 1 == nb) {
                face.decal(g, f, Tarp, t + step * 0.36, h - 3.4, BANNER, 0.8, (h - 5.0).clamp(2.0, 4.5), banner);
                g.lod(Lod::Mid, |g| face.bx(g, f, Matte, t + step * 0.36, h - 1.95, 0.12, Vec3::new(1.1, 0.06, 0.12), IRON));
            }
        }
        grime(g, c, rng, face, 0.0, h, 1.0 + 2.0 * c.wear);
    }
    // The lantern turret on the ridge: an octagon, a lead cap, a vane.
    let tp = Vec2::new(r.c().x, r.v0 + r.d() * 0.5);
    let ridge = h + rise;
    let oct = ngon(tp.x, tp.y, 0.9, 8, 0.0);
    extrude(g, f, Wood, &oct, ridge - 0.6, ridge + 1.6, mul(pt.timber, 1.4));
    loft(g, f, Metal, &ngon(tp.x, tp.y, 1.1, 8, 0.0), &ngon(tp.x, tp.y, 0.06, 8, 0.0), ridge + 1.6, ridge + 3.0, [0.26, 0.30, 0.30]);
    let from = g.get(Glow).colors.len();
    extrude(g, f, Glow, &ngon(tp.x, tp.y, 0.92, 8, 0.0), ridge + 0.4, ridge + 1.2, mul(GAS, 0.7));
    mark_glow(g, from, NIGHT_MARK);
    g.lod(Lod::Near, |g| f.bx(g, Matte, tp.x, ridge + 3.5, tp.y, Vec3::new(0.05, 1.0, 0.05), IRON));
    lamp_post(g, c, Vec2::new(r.u0 - 1.2, area.v0 + 1.2), 3.6);
    lamp_post(g, c, Vec2::new(r.u1 + 1.2, area.v0 + 1.2), 3.6);
}

// ── the clock tower ──────────────────────────────────────────────────────────

/// The clock tower and its church: a square stone tower to the street in
/// stages (an arched door, lancets, a lit clock face on every side, a
/// louvred belfry) under a lead spire with pinnacles and a vane, the nave
/// behind it with stained windows lit after dark between buttresses, and a
/// churchyard of graves and a yew.
pub fn clock_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.4, 4.2);
    let area = R::of(&c.poly);
    let ts = (area.w().min(area.d()) * 0.3).clamp(4.5, 7.0);
    let Some(tower) = fit(c, R::new(area.c().x - ts * 0.5, area.c().x + ts * 0.5, area.v0 + 1.2, area.v0 + 1.2 + ts), 3.5) else {
        cottage(g, c, rng, &pt);
        return;
    };
    let ts = tower.w().min(tower.d());
    // The nave behind.
    let nw = (area.w() * 0.55).clamp(ts + 2.0, 14.0);
    let nd = (area.d() - tower.d() - 4.0).clamp(0.0, 28.0);
    let mut nave_top = 6.0f32;
    if nd > 7.0 {
        if let Some(nave) = fit(c, R::new(area.c().x - nw * 0.5, area.c().x + nw * 0.5, tower.v1, tower.v1 + nd), 6.0) {
            let nh = (fh * 2.0).max(7.0);
            block(g, c, Stone, &nave, 0.0, nh, pt.stone);
            plinth(g, c, &nave.poly(), 1, 0.4, 0.3, Stone, pt.dark);
            let rise = steep(nave.w()).min(7.0);
            roof(g, c, &nave, nh, rise, false, 0.3, pt.roof, (Stone, pt.stone));
            nave_top = nh + rise;
            for face in [nave.left(), nave.right()] {
                let nb = ((face.len - 2.0) / 3.4).floor().max(1.0) as i32;
                let step = (face.len - 2.0) / nb as f32;
                for k in 0..nb {
                    let t = -(face.len - 2.0) * 0.5 + (k as f32 + 0.5) * step;
                    arched_window(g, c, rng, &face, t, 1.6, (step * 0.32).min(1.3), nh - 3.4, &pt, true);
                    if k + 1 < nb {
                        g.lod(Lod::Mid, |g| super::hk::wall_box(g, c, Stone, &face, t + step * 0.5, nh * 0.42, 0.0, Vec3::new(0.7, nh * 0.84, 0.6), pt.dark));
                    }
                }
                grime(g, c, rng, &face, 0.0, nh, 1.0 + 2.0 * c.wear);
            }
            // The east window over the altar.
            let east = nave.back();
            arched_window(g, c, rng, &east, 0.0, 2.0, (east.len * 0.3).min(3.2), nh - 2.6, &pt, true);
            // Churchyard: grass, graves, a yew.
            let yard = clip_box(&c.poly, area.u0 + 0.3, area.u1 - 0.3, tower.v0, area.v1 - 0.3);
            if yard.len() >= 3 {
                ground(g, f, Foliage, &yard, 0.035, mix(c.palette.foliage, [0.24, 0.30, 0.18], 0.6));
                g.lod(Lod::Mid, |g| {
                    for side in [-1.0f32, 1.0] {
                        let u = if side < 0.0 { (area.u0 + nave.u0) * 0.5 } else { (area.u1 + nave.u1) * 0.5 };
                        let room = if side < 0.0 { nave.u0 - area.u0 } else { area.u1 - nave.u1 };
                        if room < 3.0 {
                            continue;
                        }
                        for k in 0..((nave.d() / 2.2) as i32).min(8) {
                            let p = Vec2::new(u + if k % 2 == 0 { -0.6 } else { 0.6 }, nave.v0 + 1.5 + k as f32 * 2.2);
                            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.8 {
                                f.bx(g, Stone, p.x, 0.45, p.y, Vec3::new(0.6, 0.9, 0.14), mul(pt.stone, 0.8));
                            }
                        }
                    }
                });
                let yew = Vec2::new(if coin(rng, 0.5) { area.u0 + 2.4 } else { area.u1 - 2.4 }, area.v1 - 3.0);
                if c.inside_local(yew) && distance_to_boundary(yew, &c.lot_poly) > 2.0 {
                    tree(g, f, yew, rng.random_range(5.0..7.0), 2.2, c.palette.bark, mix(c.palette.foliage, [0.08, 0.16, 0.08], 0.6));
                }
            }
        }
    }
    // The tower in stages.
    let th = (nave_top + 6.0).clamp(15.0, 26.0);
    let tp = tower.poly();
    block(g, c, Stone, &tower, 0.0, th, pt.stone);
    plinth(g, c, &tp, 1, 0.4, 0.3, Stone, pt.dark);
    let clock_y = th - 5.4;
    for y in [th * 0.3, clock_y - 1.6, th - 0.2] {
        cornice(g, c, &tp, y, 0.3, 0.12, Stone, pt.trim);
    }
    let hour = hash2_salted(c.seed, 12, PANE_SALT);
    for (i, face) in tower.faces().iter().enumerate() {
        if i == 0 {
            gateway(g, c, face, 0.0, (ts * 0.32).min(1.8), 2.4, mul(pt.door, 0.9), &pt, true);
            wall_lantern(g, c, face, ts * 0.36, 2.6);
        } else if i != 2 {
            arched_window(g, c, rng, face, 0.0, th * 0.3 + 0.8, 0.55, 2.0, &pt, false);
        }
        clock_face(g, c, face, 0.0, clock_y, (ts * 0.24).min(1.4), &pt, hour);
        // The belfry: two louvred openings.
        for s in [-1.0f32, 1.0] {
            let t = s * ts * 0.2;
            face.decal(g, f, Stone, t, th - 2.2, FRAME, 1.0, 2.4, pt.trim);
            face.decal(g, f, Matte, t, th - 2.3, PANE, 0.7, 2.0, DARK);
            half_disc(g, c, face, t, th - 1.3, 0.35, PANE + 0.004, Matte, DARK);
            g.lod(Lod::Near, |g| {
                for k in 0..5 {
                    face.decal(g, f, Wood, t, th - 3.1 + k as f32 * 0.36, BAR, 0.7, 0.12, mul(pt.timber, 1.5));
                }
            });
        }
        grime(g, c, rng, face, 0.0, th, 1.0 + 1.5 * c.wear);
    }
    // The spire, its pinnacles and the vane.
    let cpt = tower.c();
    let base = rect(cpt.x, cpt.y, ts * 0.42, ts * 0.42);
    let tip = rect(cpt.x, cpt.y, 0.05, 0.05);
    let sh = ts * rng.random_range(1.4..2.0);
    loft(g, f, Metal, &base, &tip, th, th + sh, [0.24, 0.27, 0.28]);
    for (du, dv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let p = Vec2::new(cpt.x + du * (ts * 0.5 - 0.3), cpt.y + dv * (ts * 0.5 - 0.3));
        loft(g, f, Stone, &rect(p.x, p.y, 0.3, 0.3), &rect(p.x, p.y, 0.03, 0.03), th - 0.2, th + 1.8, pt.trim);
    }
    g.lod(Lod::Mid, |g| {
        f.bx(g, Matte, cpt.x, th + sh + 0.6, cpt.y, Vec3::new(0.06, 1.2, 0.06), IRON);
        f.bx(g, Metal, cpt.x, th + sh + 1.0, cpt.y, Vec3::new(0.8, 0.3, 0.04), GILT);
    });
    lamp_post(g, c, Vec2::new(tower.u0 - 1.6, area.v0 + 1.0), 3.6);
    lamp_post(g, c, Vec2::new(tower.u1 + 1.6, area.v0 + 1.0), 3.6);
}

// ── old inns ─────────────────────────────────────────────────────────────────

/// The old coaching inn: a long jettied timber-framed front over stone,
/// bay windows warm when lit, an arch through to the coach yard, the inn
/// sign on an iron bracket under a lantern, the name painted along the
/// front, tall chimneys; the yard behind cobbled with a stable range,
/// benches, barrels, a cart and lanterns.
pub fn old_inn(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(2.9, 3.3);
    let floors = (c.floors as i32).clamp(2, 3);
    let area = R::of(&c.poly);
    let depth = (area.d() * 0.42).clamp(7.0, 11.0).min(area.d() - 1.4);
    let Some(r) = fit(c, R::new(area.u0 + 0.3, area.u1 - 0.3, area.v0 + 0.9, area.v0 + 0.9 + depth), 6.0) else {
        cottage(g, c, rng, &pt);
        return;
    };
    let front = r.front();
    let left = coin(rng, 0.5);
    let gw = 3.0f32.min(front.len * 0.22);
    let gt = if left { -front.len * 0.5 + gw * 0.5 + 1.2 } else { front.len * 0.5 - gw * 0.5 - 1.2 };
    let dt = -gt * 0.3;
    let rg = Range { r, floors, fh, timber: true, jetty: 0.5, along_u: true, rise: steep(r.d() + 0.5), eave: 0.35, bay: 2.4, blank: [false; 4] };
    let keep = [Keep::door(gt, gw * 0.5 + 0.3), Keep::door(dt, 1.0), Keep::door(dt - 3.0, 1.3), Keep::door(dt + 3.0, 1.3)];
    let h = range(g, c, rng, &rg, &pt, &keep);
    // The coach gate under the jetty: square-headed, the bressummer over it.
    gateway(g, c, &front, gt, gw, fh - 0.3, mul(pt.door, 0.7), &pt, false);
    door(g, c, &front, dt, 1.1, 2.2, &pt, Pane::Lit(WARM[0]));
    // Bay windows either side of the door.
    for s in [-1.0f32, 1.0] {
        let t = dt + s * 3.0;
        if (t - gt).abs() < gw * 0.5 + 1.6 || t.abs() > front.len * 0.5 - 1.2 {
            continue;
        }
        if c.reach_out(&front) > 0.8 {
            front.bx(g, f, Wood, t, fh * 0.45, 0.3, Vec3::new(2.2, fh * 0.62, 0.6), pt.timber);
            front.bx(g, f, Panel, t, fh * 0.8, 0.32, Vec3::new(2.4, 0.14, 0.7), pt.roof);
            let pane = light(c, rng, 0.4);
            front.glaze(g, c, t, fh * 0.46, 0.6 + PANE, 1.8, fh * 0.42, pane);
            g.lod(Lod::Near, |g| {
                for k in [-1.0f32, 1.0] {
                    front.decal(g, f, Wood, t + k * 0.45, fh * 0.46, 0.6 + BAR, 0.08, fh * 0.42, pt.timber);
                }
            });
        }
    }
    // The inn sign on its bracket, a lantern over it; the painted name.
    let st = if left { front.len * 0.5 - 1.0 } else { -front.len * 0.5 + 1.0 };
    let board = pick(rng, &[[0.30, 0.09, 0.07], [0.10, 0.18, 0.12], [0.08, 0.08, 0.10], [0.40, 0.30, 0.10]]);
    if c.reach_out(&front) > 1.0 {
        g.lod(Lod::Mid, |g| front.bx(g, f, Matte, st, fh + 0.55, 0.62, Vec3::new(0.06, 0.06, 1.24), IRON));
        let p = front.at(st, 1.0);
        f.bx_yaw(g, Wood, p.x, fh - 0.25, p.y, Vec3::new(1.0, 1.2, 0.08), board, front.yaw() + FRAC_PI_2);
        f.bx_yaw(g, Wood, p.x, fh - 0.25, p.y, Vec3::new(1.12, 1.32, 0.05), GILT, front.yaw() + FRAC_PI_2);
        let from = g.get(Glow).colors.len();
        f.bx(g, Glow, p.x, fh + 0.85, p.y, Vec3::new(0.26, 0.36, 0.26), GAS);
        mark_glow(g, from, NIGHT_MARK);
    }
    let up = R::new(r.u0, r.u1, r.v0 - 0.5, r.v1).front();
    let bw = (front.len * 0.5).min(14.0);
    let from = g.get(Glow).colors.len();
    let letters = ((bw / 0.6) as i32).clamp(3, 18);
    for i in 0..letters {
        if coin(rng, 0.08) {
            continue;
        }
        up.decal(g, f, Glow, -bw * 0.5 + (i as f32 + 0.5) * bw / letters as f32, fh + 0.55, FRAME, bw / letters as f32 * 0.6, 0.36, [1.3, 1.0, 0.5]);
    }
    mark_glow(g, from, NEON_MARK);
    for s in [-1.0f32, 1.0] {
        wall_lantern(g, c, &front, dt + s * 1.0, 2.6);
    }
    let ridge = h + rg.rise;
    for u in [r.u0 + 1.4, r.c().x, r.u1 - 1.4] {
        chimney(g, c, Vec2::new(u, r.c().y - 0.25), ridge - 1.4, ridge + 1.5, 1.2, &pt);
    }
    // The coach yard behind.
    let yard = R::new(r.u0, r.u1, r.v1 + 0.3, area.v1 - 0.3);
    if yard.d() > 5.0 {
        let yp = clip_box(&c.poly, yard.u0, yard.u1, yard.v0, yard.v1);
        cobbles(g, c, &yp, 0.04, &pt);
        // The stables along the far side.
        let sd = (yard.d() * 0.35).clamp(4.0, 6.0);
        if let Some(sr) = fit(c, R::new(yard.u0 + 0.3, yard.u1 - 0.3, yard.v1 - sd, yard.v1 - 0.2), 3.5) {
            let srg = Range { r: sr, floors: 1, fh: fh * 1.1, timber: false, jetty: 0.0, along_u: true, rise: steep(sr.d()).min(3.0), eave: 0.3, bay: 3.0, blank: [true, false, true, false] };
            range(g, c, rng, &srg, &pt, &[]);
            let sf = sr.front();
            let nd = ((sf.len - 1.0) / 3.2).floor().clamp(1.0, 6.0) as i32;
            for k in 0..nd {
                let t = -(sf.len - 1.0) * 0.5 + (k as f32 + 0.5) * (sf.len - 1.0) / nd as f32;
                sf.decal(g, f, Stone, t, 1.25, FRAME, 1.8, 2.5, pt.trim);
                sf.decal(g, f, Wood, t, 1.1, PANE, 1.5, 2.2, mul(pt.door, 0.85));
                g.lod(Lod::Near, |g| sf.decal(g, f, Wood, t, 1.4, BAR, 1.5, 0.1, pt.timber));
            }
            wall_lantern(g, c, &sf, 0.0, 2.6);
        }
        g.lod(Lod::Mid, |g| {
            let n = ((yard.w() - 4.0) / 3.2) as i32;
            for k in 0..n.clamp(0, 4) {
                let p = Vec2::new(yard.u0 + 2.0 + k as f32 * 3.2, yard.v0 + 1.6);
                if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.4 {
                    table(g, f, p, 0.04, mul(c.palette.wood, 0.7), None);
                }
            }
            for k in 0..3 {
                let p = Vec2::new(if left { yard.u1 - 1.0 } else { yard.u0 + 1.0 }, yard.v0 + 1.0 + k as f32 * 0.8);
                if c.inside_local(p) {
                    g.part(Wood).cylinder(f.to_world(p.x, p.y), 0.3, 0.34, 0.04, 0.9, 8, rgba(mul(c.palette.wood, 0.6)));
                }
            }
        });
        super::oldstone_works::cart(g, c, Vec2::new(yard.c().x, yard.v0 + (yard.d() * 0.45).min(3.5)), 0.3, &pt);
        lamp_post(g, c, Vec2::new(if left { yard.u0 + 1.2 } else { yard.u1 - 1.2 }, yard.v0 + 1.2), 3.4);
    }
}

// ── the stone manor ──────────────────────────────────────────────────────────

/// [r4-oldstone] An Old Town family's big house (two plots) or a
/// billionaire's estate (up to four): a stone manor round a courtyard. The
/// hall range across the back with tall mullioned windows and a porch
/// tower with a clock, wings forward either side of a gravel court with a
/// fountain, a wall to the street with a gatehouse arch between lamp-lit
/// piers, a formal garden of clipped hedges and trees, the cars on the
/// gravel. On the bigger lot the wings are longer and a glasshouse stands
/// in the garden.
pub fn stone_manor(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let pt = Paint { roof: mul(ROOFS[2 + rng.random_range(0..2)], 1.0), ..paint(c, rng) };
    let fh = c.fh().clamp(3.4, 4.0);
    let area = R::of(&c.poly);
    let big = area.w() * area.d() > 2600.0;
    let floors = (c.floors as i32).clamp(2, 3);
    let yard_poly = yard(g, c, 0.25, Paving, mix(pal.paving, [0.62, 0.58, 0.50], 0.5));
    let w = (area.w() * if big { 0.66 } else { 0.8 }).clamp(14.0, 48.0);
    let rd = (area.d() * 0.2).clamp(7.0, 10.0);
    let hv1 = area.v1 - if big { area.d() * 0.32 } else { 3.0 };
    let Some(hall) = fit(c, R::new(area.c().x - w * 0.5, area.c().x + w * 0.5, hv1 - rd, hv1), 7.0) else {
        courtyard_house(g, c, rng);
        return;
    };
    let rg = Range { r: hall, floors, fh, timber: false, jetty: 0.0, along_u: true, rise: steep(rd).min(6.0), eave: 0.3, bay: 2.8, blank: [false; 4] };
    let h = range(g, c, rng, &rg, &pt, &[Keep { face: 0, t: 0.0, half: 2.4, all: true }]);
    let ridge = h + rg.rise;
    for u in [hall.u0 + 1.5, hall.u0 + hall.w() * 0.35, hall.u1 - hall.w() * 0.35, hall.u1 - 1.5] {
        chimney(g, c, Vec2::new(u, hall.c().y), ridge - 1.6, ridge + 1.8, 1.3, &pt);
    }
    // The porch tower in the middle of the front, a clock on it.
    let pw = 4.2f32.min(hall.w() * 0.2);
    let porch = R::new(hall.c().x - pw * 0.5, hall.c().x + pw * 0.5, hall.v0 - pw * 0.7, hall.v0);
    let ph = h + fh * 0.8;
    block(g, c, Stone, &porch, 0.0, ph, pt.stone);
    cornice(g, c, &porch.poly(), ph - 0.15, 0.3, 0.15, Stone, pt.trim);
    loft(g, f, Metal, &porch.inset(0.1).poly(), &rect(porch.c().x, porch.c().y, 0.05, 0.05), ph, ph + pw * 0.9, mul(pt.roof, 1.1));
    let pf = porch.front();
    gateway(g, c, &pf, 0.0, (pw * 0.4).min(1.8), 2.4, mul(pt.door, 0.85), &pt, true);
    clock_face(g, c, &pf, 0.0, ph - 1.3, (pw * 0.18).min(0.8), &pt, hash2_salted(c.seed, 3, PANE_SALT));
    window(g, c, &pf, 0.0, fh + 1.6, 0.9, 1.4, light(c, rng, 0.2), &pt, Set::Stone, 1);
    for s in [-1.0f32, 1.0] {
        wall_lantern(g, c, &pf, s * (pw * 0.5 - 0.5), 2.6);
    }
    // Wings forward round the court.
    let wl = (hall.v0 - area.v0 - 6.0).min(if big { 30.0 } else { 16.0 });
    let ww = (rd * 0.85).min(hall.w() * 0.25);
    let mut court = R::new(hall.u0, hall.u1, hall.v0 - wl.max(0.0), hall.v0);
    if wl > 6.0 {
        for s in [-1.0f32, 1.0] {
            let u0 = if s < 0.0 { hall.u0 } else { hall.u1 - ww };
            if let Some(wr) = fit(c, R::new(u0, u0 + ww, hall.v0 - wl, hall.v0), 4.0) {
                let wrg = Range { r: wr, floors: (floors - 1).max(2), fh, timber: false, jetty: 0.0, along_u: false, rise: steep(ww).min(5.0), eave: 0.0, bay: 2.8, blank: [false, false, true, false] };
                let wh = range(g, c, rng, &wrg, &pt, &[]);
                crow_steps(g, c, &wr.front(), wh, wrg.rise, 3, &pt);
                chimney(g, c, Vec2::new(wr.c().x, wr.v0 + 1.0), wh + wrg.rise - 1.6, wh + wrg.rise + 0.6, 1.0, &pt);
                if s < 0.0 {
                    court.u0 = wr.u1;
                } else {
                    court.u1 = wr.u0;
                }
            }
        }
    }
    // The gravel court: a fountain, the cars.
    let cp = court.inset(0.6);
    if court.w() > 6.0 && court.d() > 6.0 && poly_inside(c, &rect(cp.c().x, cp.c().y, 2.2, 2.2)) {
        let gravel = clip_box(&yard_poly, cp.u0, cp.u1, cp.v0, cp.v1);
        ground(g, f, Dirt, &gravel, 0.06, mix(pal.paving, [0.70, 0.66, 0.56], 0.6));
        let fp = cp.c();
        let fw = f.to_world(fp.x, fp.y);
        g.part(Stone).cylinder_capped(fw, 2.0, 2.0, 0.06, 0.6, 16, false, true, rgba(pt.trim));
        ground(g, f, Glass, &ngon(fp.x, fp.y, 1.75, 16, 0.0), 0.61, [0.16, 0.24, 0.28]);
        g.part(Stone).cylinder_capped(fw, 0.3, 0.22, 0.6, 2.0, 8, false, true, rgba(pt.trim));
        g.part(Stone).cylinder_capped(fw, 0.8, 0.7, 2.0, 2.2, 12, true, true, rgba(pt.trim));
        for (k, du) in [-1.0f32, 1.0].into_iter().enumerate() {
            let p = Vec2::new(fp.x + du * (cp.w() * 0.3).min(7.0), fp.y + cp.d() * 0.2);
            if vehicle_fits(c, p, FRAC_PI_2, Vehicle::Car) && (k == 0 || big) {
                let col = pick(rng, &[[0.06, 0.06, 0.07], [0.12, 0.20, 0.14], [0.30, 0.06, 0.06], [0.62, 0.62, 0.60]]);
                vehicle(g, f, pal, rng, p, 0.06, FRAC_PI_2, Vehicle::Car, col);
            }
        }
        lamp_post(g, c, Vec2::new(cp.u0 + 1.0, cp.v1 - 1.0), 3.6);
        lamp_post(g, c, Vec2::new(cp.u1 - 1.0, cp.v1 - 1.0), 3.6);
    }
    // The formal garden behind: a lawn, clipped hedges, trees, a glasshouse.
    let gr = R::new(area.u0 + 1.5, area.u1 - 1.5, hall.v1 + 1.0, area.v1 - 1.5);
    if gr.d() > 5.0 && gr.w() > 6.0 {
        let gp = clip_box(&yard_poly, gr.u0, gr.u1, gr.v0, gr.v1);
        if gp.len() >= 3 {
            ground(g, f, Foliage, &gp, 0.08, mix(pal.foliage, [0.30, 0.42, 0.22], 0.5));
            let hedge_c = mix(pal.foliage, [0.10, 0.22, 0.10], 0.5);
            g.lod(Lod::Mid, |g| {
                let rows = ((gr.d() - 2.0) / 4.0) as i32;
                for k in 0..rows.min(6) {
                    let v = gr.v0 + 2.0 + k as f32 * 4.0;
                    for s in [-1.0f32, 1.0] {
                        let a = Vec2::new(gr.c().x + s * 1.5, v);
                        let b = Vec2::new(gr.c().x + s * (gr.w() * 0.5 - 1.0), v);
                        if c.spans_lot(a, b) {
                            hedge(g, c, a, b, 0.8, 0.6, hedge_c);
                        }
                    }
                }
            });
            for s in [-1.0f32, 1.0] {
                let p = Vec2::new(gr.c().x + s * (gr.w() * 0.5 - 2.0), gr.v1 - 2.5);
                if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.5 {
                    tree_in(g, f, pal, p, rng.random_range(7.0..10.0), rng.random_range(2.0..3.0), pal.foliage);
                }
            }
            if big && gr.w() > 20.0 && gr.d() > 10.0 {
                let gh = R::new(gr.c().x - 5.0, gr.c().x + 5.0, gr.v1 - 6.5, gr.v1 - 2.5);
                if poly_inside(c, &gh.poly()) {
                    block(g, c, Stone, &gh, 0.0, 0.7, pt.stone);
                    block(g, c, Glass, &gh.inset(0.1), 0.7, 3.2, mul(pal.glass_dark, 1.6));
                    roof(g, c, &gh.inset(0.1), 3.2, 1.4, true, 0.0, mul(pal.glass_dark, 1.8), (Glass, mul(pal.glass_dark, 1.6)));
                    let from = g.get(Glow).colors.len();
                    gh.front().decal(g, f, Glow, 0.0, 1.9, 0.16, gh.w() - 1.0, 1.6, mul(WARM[3], 0.5));
                    mark_glow(g, from, NIGHT_MARK);
                }
            }
        }
    }
    // The wall to the street, the gatehouse arch between lamp-lit piers.
    let fi = c.front_face_index(&yard_poly);
    for (i, face) in c.faces(&yard_poly).iter().enumerate() {
        if face.len < 1.0 {
            continue;
        }
        if Some(i) == fi {
            let gw = 4.0f32.min(face.len * 0.3);
            for s in [-1.0f32, 1.0] {
                let len = face.len * 0.5 - gw * 0.5 - 0.5;
                if len > 0.5 {
                    face.bx(g, f, Stone, s * (gw * 0.5 + 0.5 + len * 0.5), 1.1, -0.25, Vec3::new(len, 2.2, 0.5), pt.stone);
                }
                face.bx(g, f, Stone, s * (gw * 0.5 + 0.25), 1.7, -0.35, Vec3::new(0.9, 3.4, 0.9), pt.trim);
                face.bx(g, f, Stone, s * (gw * 0.5 + 0.25), 3.5, -0.35, Vec3::new(1.1, 0.2, 1.1), pt.trim);
                let from = g.get(Glow).colors.len();
                face.bx(g, f, Glow, s * (gw * 0.5 + 0.25), 3.9, -0.35, Vec3::new(0.34, 0.5, 0.34), GAS);
                mark_glow(g, from, NIGHT_MARK);
            }
            gate(g, c, face.at(-gw * 0.5 + 0.2, -0.25), face.at(gw * 0.5 - 0.2, -0.25), 2.6);
        } else {
            face.bx(g, f, Stone, 0.0, 1.1, -0.25, Vec3::new(face.len, 2.2, 0.5), pt.stone);
        }
    }
}
