//! [r4-style] The Hong Kong detail pass for the walled blocks and pencil
//! towers (docs/STYLE_REFERENCE.md, section 1): a deep ledge over the
//! shops, a different frame per flat, AC units under most windows with
//! damp streaks below, steel cages, a stair and lift core up one side with
//! pipes on it, tile bands, roller shutters and striped awnings, a podium
//! billboard, corner neon and a roof of railings, aerials, tanks and shacks.
//!
//! Everything here is drawn from the building's own seed: the street level
//! and the roof from [`stream`] (a stream of its own, so the building's
//! main stream, and with it its windows, signs and mass, runs as it did
//! before the detail existed), and the per-flat choices from a hash of the
//! seed, the face, the floor and the column. Small parts go into the
//! detail layers ([`Lod`]) that the renderer drops with distance.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::hash::hash2_salted;
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Salt of the detail stream.
const DETAIL_STREAM: u64 = 0x4B10_0D;
/// Salt of the per-flat hash.
const FLAT_SALT: u64 = 0x4B_F1A7;

/// Depths off the wall (see `crate::depth`): tile bands under the frames,
/// the frames under the grime (0.02), streaks between the grime and the
/// graffiti (0.024), mullions just proud of the glazing (0.04).
const BAND: f32 = 0.008;
const FRAME: f32 = 0.012;
const STREAK: f32 = 0.0215;
const MULLION: f32 = 0.046;

/// The colour of damp run down a wall from a drip.
fn damp(c: &Ctx) -> [f32; 3] {
    mul(mix(c.palette.rust_stain, [0.30, 0.32, 0.28], 0.65), 0.75)
}

/// The building's detail stream.
pub fn stream(c: &Ctx) -> nd_sim::SimRng {
    nd_sim::rng_for(c.seed, DETAIL_STREAM)
}

/// A face's identity for the per-flat hash: its middle, to the decimetre.
fn face_key(face: &Face) -> u64 {
    let m = face.mid();
    hash2_salted((m.x * 10.0).round() as i64 as u64, (m.y * 10.0).round() as i64 as u64, FLAT_SALT)
}

/// 0..1 from sixteen bits of a hash, from bit `shift`.
fn unit(h: u64, shift: u32) -> f32 {
    ((h >> shift) & 0xFFFF) as f32 / 65_536.0
}

/// A box against a face with no back: `t` along, centred at height `y`,
/// its back `d` proud of the wall and `size` (along, up, out). Ten
/// triangles where a closed box spends twelve on a face pressed to the wall.
#[allow(clippy::too_many_arguments)]
pub fn wall_box(g: &mut Geometry, c: &Ctx, s: Surface, face: &Face, t: f32, y: f32, d: f32, size: Vec3, color: [f32; 3]) {
    let f = &c.frame;
    let (hx, hy, hz) = (size.x * 0.5, size.y * 0.5, size.z * 0.5);
    let centre = f.at2(face.at(t, d + hz), y);
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    let col = rgba(color);
    let m = g.part(s);
    m.decal(centre + out * hz, along, Vec3::Y, size.x, size.y, out, col);
    m.decal(centre + Vec3::Y * hy, along, out, size.x, size.z, Vec3::Y, col);
    m.decal(centre - Vec3::Y * hy, along, out, size.x, size.z, -Vec3::Y, col);
    m.decal(centre + along * hx, out, Vec3::Y, size.z, size.y, along, col);
    m.decal(centre - along * hx, out, Vec3::Y, size.z, size.y, -along, col);
}

// ── windows ──────────────────────────────────────────────────────────────────

/// Window frame colours: white aluminium, grey aluminium, green-painted
/// steel, dark timber, faded red, black.
const FRAMES: [[f32; 3]; 6] = [[0.80, 0.80, 0.76], [0.56, 0.57, 0.58], [0.26, 0.38, 0.31], [0.30, 0.22, 0.16], [0.55, 0.26, 0.22], [0.12, 0.12, 0.13]];

/// What dresses the windows of a face.
#[derive(Clone, Copy)]
pub struct FlatDress {
    /// Chance of an AC unit under a window on top of the facade's own.
    pub more_ac: f32,
    /// Leave the bottom floor's AC units out: a ledge runs under it.
    pub ledge_below: bool,
}

/// [`dense_windows`] in the Hong Kong way: the same glazing, cages and AC
/// units at the same windows from the same draws of `rng`, and then from the
/// per-flat hash a frame (and mullion, transom or sill) that changes from
/// flat to flat, more AC units, a damp streak under some. Cages and AC units
/// are detail ([`Lod::Mid`], [`Lod::Near`]); the glazing is the base mesh.
#[allow(clippy::too_many_arguments)]
pub fn flat_windows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spacing: f32, ac: f32, cages: f32, dress: FlatDress) {
    let usable = face.len - 0.8;
    let count = (usable / spacing).floor() as i32;
    if count < 1 || floors < 1 {
        return;
    }
    let step = usable / count as f32;
    let (w, h) = ((step * 0.62).min(1.3), (fh * 0.45).min(1.5));
    let lit = c.lit_chance();
    let room = c.reach_out(face);
    let key = face_key(face) ^ c.seed;
    let wear = c.wear;
    let (cage_s, cage_c) = outdoor_steel(c, mul(c.palette.steel, 0.55));
    let ac_cols = [mul(c.palette.steel, 0.9), mul([0.78, 0.77, 0.72], 1.0 - 0.3 * wear), mul([0.66, 0.68, 0.66], 1.0 - 0.25 * wear)];
    // Damp reads as a soft darkening of the wall, not a rust texture.
    let streak = damp(c);
    // Two windows to a flat, more on a long face.
    let per_flat = if count > 6 { 3 } else { 2 };
    for k in 0..floors {
        let y = y0 + k as f32 * fh + fh * 0.52;
        for i in 0..count {
            let t = -usable * 0.5 + (i as f32 + 0.5) * step;
            // The glazing and the cage and AC draws, exactly as
            // `dense_windows` makes them.
            let r: f32 = rng.random();
            if r < c.wear * 0.25 {
                face.glaze(g, c, t, y, 0.04, w, h, Pane::Boarded);
            } else if coin(rng, lit) {
                let wc = pick(rng, &c.style.window_colors);
                let col = mul(wc, rng.random_range(0.45..0.95));
                face.glaze(g, c, t, y, 0.04, w, h, Pane::Lit(col));
            } else {
                face.glaze(g, c, t, y, 0.04, w, h, Pane::Dark);
            }
            let caged = room > -0.3 && coin(rng, cages);
            let boxed = !caged && room > -0.3 && coin(rng, ac);
            // The flat's own look, and this window's.
            let flat = hash2_salted(key, ((k as u64) << 20) | (i / per_flat) as u64, FLAT_SALT);
            let win = hash2_salted(flat, i as u64, FLAT_SALT + 1);
            let fc = mul(FRAMES[(flat % FRAMES.len() as u64) as usize], 1.0 - 0.35 * wear * unit(flat, 40));
            let kind = (flat >> 8) % 5;
            g.lod(Lod::Near, |g| {
                face.decal(g, &c.frame, Matte, t, y, FRAME, w + 0.16, h + 0.16, fc);
                if kind == 1 || kind == 3 {
                    face.decal(g, &c.frame, Matte, t, y, MULLION, 0.05, h, fc);
                }
                if kind == 2 || kind == 3 {
                    face.decal(g, &c.frame, Matte, t, y + h * 0.22, MULLION, w, 0.05, fc);
                }
                if kind == 4 {
                    // A painted sill under it.
                    face.decal(g, &c.frame, Matte, t, y - h * 0.5 - 0.14, MULLION, w + 0.3, 0.1, mul(fc, 0.9));
                }
            });
            if caged {
                g.lod(Lod::Mid, |g| cage(g, c, face, t, y, w, h, cage_s, cage_c));
                continue;
            }
            let more = room > -0.3 && unit(win, 0) < dress.more_ac;
            if !(boxed || more) || (k == 0 && dress.ledge_below) {
                continue;
            }
            let (ty, ya) = (t + w * 0.3, y - h * 0.5 - 0.3);
            if ya < 0.6 {
                continue;
            }
            // A window box or a split unit's condenser, on the wall.
            let split = unit(win, 16) < 0.4;
            let size = if split { Vec3::new(0.82, 0.56, 0.32) } else { Vec3::new(0.66, 0.44, 0.42) };
            let col = ac_cols[(win >> 32) as usize % ac_cols.len()];
            let ya = ya.min(y - h * 0.5 - size.y * 0.5 - 0.02);
            g.lod(Lod::Near, |g| {
                wall_box(g, c, Metal, face, ty, ya, 0.03, size, col);
                // The grille (a split unit's fan).
                let gw = if split { 0.42 } else { 0.5 };
                face.decal(g, &c.frame, Matte, ty + if split { 0.12 } else { 0.0 }, ya, 0.03 + size.z + 0.008, gw, size.y * 0.7, DARK);
                // Damp down the wall from its drip.
                if unit(win, 48) < 0.25 + 0.45 * wear {
                    let len = 0.8 + 2.4 * unit(win, 24);
                    // Clear of the unit on the floor below.
                    let len = len.min(ya - size.y * 0.5 - 0.1).min(fh - 2.0 * size.y);
                    if len > 0.3 {
                        face.decal(g, &c.frame, Plaster, ty, ya - size.y * 0.5 - len * 0.5, STREAK, 0.22 + 0.25 * unit(win, 8), len, streak);
                    }
                }
            });
        }
    }
}

/// A steel cage over a window: a floor and a tin roof on the wall, a post
/// at each side and bars across the front, open so a lit window shows.
#[allow(clippy::too_many_arguments)]
fn cage(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, h: f32, s: Surface, col: [f32; 3]) {
    let d = 0.6;
    let (bot, top) = (y - h * 0.5 - 0.3, y + h * 0.5 + 0.25);
    wall_box(g, c, s, face, t, bot, 0.0, Vec3::new(w + 0.4, 0.08, d), col);
    wall_box(g, c, Metal, face, t, top, 0.0, Vec3::new(w + 0.5, 0.06, d + 0.1), mul(col, 0.85));
    for side in [-1.0f32, 1.0] {
        wall_box(g, c, s, face, t + side * (w * 0.5 + 0.17), (bot + top) * 0.5, 0.0, Vec3::new(0.05, top - bot, d), col);
    }
    for b in [-0.25f32, 0.25] {
        face.decal(g, &c.frame, Matte, t + b * w, (bot + top) * 0.5, d + 0.01, 0.035, top - bot, col);
    }
    face.decal(g, &c.frame, Matte, t, bot + 0.9, d + 0.012, w + 0.36, 0.04, col);
}

/// Tile bands between the window rows (the stained pink or brown tile on
/// one wing and bare render on the next): a flat band per floor, in the
/// base mesh since they read from afar.
pub fn tile_bands(g: &mut Geometry, c: &Ctx, face: &Face, y0: f32, floors: i32, fh: f32, color: [f32; 3]) {
    for k in 1..=floors {
        face.decal(g, &c.frame, Panel, 0.0, y0 + k as f32 * fh - fh * 0.02, BAND, face.len - 0.05, fh * 0.4, color);
    }
}

/// The tile a Hong Kong wing is faced in: stained pink, brown, pale green or
/// white, from the walls of the look so a district tint carries.
pub fn tile_tone(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    let base = pick(rng, &c.palette.plaster);
    let hue = [[0.66, 0.45, 0.43], [0.50, 0.33, 0.28], [0.52, 0.60, 0.54], [0.80, 0.79, 0.76]][rng.random_range(0..4)];
    mul(mix(base, hue, 0.6), 1.0 - 0.3 * c.wear)
}

// ── street level ─────────────────────────────────────────────────────────────

/// The deep ledge over the shops: a concrete slab out over the pavement
/// at `y` (its underside), `depth` deep, running `ext` (start, end) past the
/// face's ends where the ledge turns a corner, with lights under it.
#[allow(clippy::too_many_arguments)]
pub fn ledge(g: &mut Geometry, c: &Ctx, face: &Face, y: f32, depth: f32, ext: (f32, f32), color: [f32; 3]) {
    let f = &c.frame;
    let thick = 0.45;
    let len = face.len + ext.0 + ext.1;
    let mid = (ext.1 - ext.0) * 0.5;
    face.bx(g, f, Concrete, mid, y + thick * 0.5, depth * 0.5 - 0.05, Vec3::new(len, thick, depth + 0.1), color);
    // A darker drip band along its edge, and a row of lamps under it.
    face.decal(g, f, Matte, mid, y + thick * 0.5, depth + 0.012, len, thick * 0.5, mul(color, 0.55));
    let n = (face.len / 2.6).floor().max(1.0) as i32;
    let under = f.dir3(face.out);
    let along = f.dir3(face.tangent);
    for i in 0..n {
        let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / n as f32;
        let p = f.at2(face.at(t, depth * 0.6), y - 0.01);
        g.part(Glow).decal(p, along, under, 0.5, 0.1, -Vec3::Y, rgba(mul(c.palette.lamp, 0.7)));
    }
}

/// Which faces of a footprint get the ledge (the street faces) and how far
/// each runs on round its corners: `(face index, (start, end))`.
pub fn ledge_faces(c: &Ctx, poly: &[Vec2], outer: &[Vec2], depth: f32) -> Vec<(usize, (f32, f32))> {
    let faces = c.faces(poly);
    let on: Vec<bool> = faces.iter().map(|fc| fc.len > 3.0 && face_is_outer(fc, outer) && (fc.is_front || c.reach_out(fc) > 3.5)).collect();
    let n = faces.len();
    (0..n).filter(|&i| on[i]).map(|i| (i, (if on[(i + n - 1) % n] { depth } else { 0.0 }, if on[(i + 1) % n] { depth } else { 0.0 }))).collect()
}

/// Shops under the ledge: roller shutters (down, half up or open on a lit
/// shop), the shutter's drum, a lit fascia sign and a striped awning on
/// steel arms. Draws only from `rng` (the detail stream).
pub fn shops(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, h: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let len = face.len * 0.92;
    let units = (len / 4.6).floor().max(1.0) as i32;
    let unit_w = len / units as f32;
    let room = c.reach_out(face);
    let (shut_h, drum_y) = (h * 0.66, y0 + h * 0.72);
    for i in 0..units {
        let t = -len * 0.5 + (i as f32 + 0.5) * unit_w;
        let state = rng.random::<f32>() - 0.4 * c.wear;
        let shutter = mul(pick(rng, &pal.metal_paint), 0.75 - 0.25 * c.wear);
        let w = unit_w * 0.86;
        if state > 0.25 {
            // Open: the lit shop behind the glass, the shutter rolled up.
            face.glaze(g, c, t, y0 + shut_h * 0.5, 0.04, w, shut_h, Pane::Dark);
            if coin(rng, 0.55 + 0.4 * c.lit_chance()) {
                let wc = pick(rng, &pal.glow_colors);
                face.glaze(g, c, t, y0 + shut_h * 0.5, 0.06, w * 0.96, shut_h * 0.92, Pane::Lit(mul(wc, 0.6)));
            }
            if state < 0.55 {
                // Half down.
                face.decal(g, f, Metal, t, y0 + shut_h * 0.8, 0.075, w, shut_h * 0.4, shutter);
            }
        } else {
            face.decal(g, f, Metal, t, y0 + shut_h * 0.5, 0.075, w, shut_h, shutter);
            if coin(rng, 0.5 + c.wear) {
                graffiti(g, c, rng, face, y0, y0 + shut_h, 0.6);
            }
        }
        // Pillar between the shops.
        face.decal(g, f, Concrete, t - unit_w * 0.5, y0 + h * 0.5, 0.03, unit_w - w - 0.04, h, mul(pick(rng, &pal.concrete), 0.8));
        g.lod(Lod::Mid, |g| wall_box(g, c, Metal, face, t, drum_y, 0.0, Vec3::new(w, 0.3, 0.26), mul(shutter, 0.8)));
        // The shop's name over it.
        if coin(rng, 0.85) {
            sign_board(g, c, rng, face, t, y0 + h * 0.9, unit_w * 0.78, h * 0.1);
        }
        let reach = (room + 0.6).min(1.6);
        if reach > 0.7 && coin(rng, 0.6) {
            let col = mul(pick(rng, &pal.awning_colors), 1.0 - 0.4 * c.wear);
            let stripe = if coin(rng, 0.6) { Some(mix(col, [0.85, 0.84, 0.8], 0.7)) } else { None };
            g.lod(Lod::Mid, |g| awning(g, c, face, t, drum_y + 0.15, unit_w * 0.84, reach, col, stripe));
        }
    }
}

/// A canvas awning sloping out from the wall at `y` to `reach`, with a
/// valance at its edge, stripes and two steel arms.
#[allow(clippy::too_many_arguments)]
fn awning(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, reach: f32, col: [f32; 3], stripe: Option<[f32; 3]>) {
    let f = &c.frame;
    let drop = reach * 0.42;
    let a = f.at2(face.at(t, 0.3), y);
    let b = f.at2(face.at(t, 0.3 + reach), y - drop);
    g.part(Tarp).slab(a, b, w, 0.05, rgba(col));
    let along = f.dir3(face.tangent);
    let down = (b - a).normalize_or_zero();
    let n = along.cross(down);
    let n = if n.y < 0.0 { -n } else { n };
    if let Some(sc) = stripe {
        let k = 4;
        let sw = w / (2 * k) as f32;
        for s in 0..k {
            let off = -w * 0.5 + (2 * s) as f32 * sw + sw * 0.5;
            g.part(Tarp).decal((a + b) * 0.5 + along * off + n * 0.006, along, down, sw, a.distance(b), n, rgba(sc));
        }
    }
    let out = f.dir3(face.out);
    g.part(Tarp).decal(b + out * 0.01 - Vec3::Y * 0.15, along, Vec3::Y, w, 0.3, out, rgba(mul(stripe.unwrap_or(col), 0.92)));
    // The arms, from the wall under the drum to the front corners.
    for side in [-1.0f32, 1.0] {
        let pa = f.at2(face.at(t + side * w * 0.48, 0.05), y - drop - 0.25);
        let pb = f.at2(face.at(t + side * w * 0.48, 0.3 + reach - 0.05), y - drop - 0.06);
        g.part(Matte).slab(pa, pb, 0.04, 0.04, rgba(CHARCOAL));
    }
}


/// A billboard on the podium: a framed board on brackets, its face a
/// poster in sign colours with lit lettering, and lamps on arms over it.
#[allow(clippy::too_many_arguments)]
pub fn podium_billboard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let d = 0.35;
    face.bx(g, f, Matte, t, y, d, Vec3::new(w + 0.3, h + 0.3, 0.2), DARK);
    let poster = mul(pick(rng, &pal.sign_colors), 0.55 * (1.0 - 0.4 * c.wear));
    face.decal(g, f, Plaster, t, y, d + 0.11, w, h, mix(poster, [0.85, 0.82, 0.6], 0.35));
    let ink = mul(pick(rng, &pal.sign_colors), 1.0 - 0.5 * c.wear);
    let lines = if h > 2.2 { 2 } else { 1 };
    for l in 0..lines {
        let ly = y + h * (0.18 - 0.36 * l as f32) * (lines - 1) as f32;
        let n = ((w * 0.8) / (h * 0.32)).floor().clamp(2.0, 9.0) as i32;
        for i in 0..n {
            if coin(rng, 0.1 + 0.3 * c.wear) {
                continue;
            }
            let lt = t - w * 0.4 + (i as f32 + 0.5) * w * 0.8 / n as f32;
            face.decal(g, f, Glow, lt, ly, d + 0.125, w * 0.8 / n as f32 * 0.72, h * 0.26, ink);
        }
    }
    g.lod(Lod::Near, |g| {
        for k in 0..3 {
            let lt = t + (k as f32 - 1.0) * w * 0.35;
            face.bx(g, f, Matte, lt, y + h * 0.5 + 0.25, d + 0.35, Vec3::new(0.05, 0.05, 0.8), CHARCOAL);
        }
    });
    for k in 0..3 {
        let lt = t + (k as f32 - 1.0) * w * 0.35;
        face.bx(g, f, Glow, lt, y + h * 0.5 + 0.2, d + 0.72, Vec3::new(0.3, 0.08, 0.16), mul(pal.lamp, 0.8));
    }
}

// ── the core and its pipes ───────────────────────────────────────────────────

/// A run of pipes up a face from `y0` to `y1`: `n` risers side by side at
/// `t`, a bracket band every few floors and a branch into the wall now and
/// then, the "ladder" of pipes on a Hong Kong core. Near detail.
#[allow(clippy::too_many_arguments)]
pub fn pipe_run(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, y1: f32, fh: f32, n: i32) {
    if y1 - y0 < 1.0 || n < 1 {
        return;
    }
    let pipe = mul(mix(c.style.trim_color, c.palette.steel, 0.5), 0.75 - 0.2 * c.wear);
    let rust = damp(c);
    let gap = 0.24;
    let h = y1 - y0;
    g.lod(Lod::Near, |g| {
        for k in 0..n {
            let tk = t + (k as f32 - (n - 1) as f32 * 0.5) * gap;
            let r = if k == 0 { 0.16 } else { 0.1 };
            wall_box(g, c, Metal, face, tk, y0 + h * 0.5, 0.04, Vec3::new(r, h, r), pipe);
        }
        let floors = (h / fh).floor() as i32;
        let w = (n - 1) as f32 * gap + 0.3;
        for fl in (1..floors).step_by(2) {
            let y = y0 + fl as f32 * fh;
            // Bracket band across the risers.
            face.decal(g, &c.frame, Matte, t, y, 0.22, w, 0.06, mul(pipe, 0.6));
            if coin(rng, 0.25 + 0.4 * c.wear) {
                let dt = rng.random_range(-0.2..0.2);
                face.decal(g, &c.frame, Plaster, t + dt, y - 0.6, STREAK, 0.3, 1.2, rust);
            }
        }
    });
}

/// A stair and lift core standing against one side of a tower (lot-local
/// `tower`), on the roof of the podium at `y0` and a storey over the
/// tower's `top`: a narrow shaft in a material of its own with slit
/// windows, a lift motor room on top and pipes up its outer face. Returns
/// the core's footprint, or `None` where the podium has no room for it.
#[allow(clippy::too_many_arguments)]
pub fn side_core(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, tower: &[Vec2], y0: f32, top: f32, fh: f32, color: [f32; 3]) -> Option<Vec<Vec2>> {
    let f = &c.frame;
    let (tlo, thi) = bounds(tower);
    let (plo, phi) = bounds(&c.poly);
    let (left, right) = (tlo.x - plo.x, phi.x - thi.x);
    let room = left.max(right);
    if room < 2.6 || thi.y - tlo.y < 5.0 {
        return None;
    }
    let cw = (room - 0.4).clamp(2.2, 3.6);
    let cd = ((thi.y - tlo.y) * 0.4).clamp(3.0, 5.0);
    let cv = tlo.y + (thi.y - tlo.y) * rng.random_range(0.45..0.6);
    let (u0, u1) = if right >= left { (thi.x - 0.2, thi.x + cw) } else { (tlo.x - cw, tlo.x + 0.2) };
    let core = clip_box(&c.poly, u0, u1, cv - cd * 0.5, cv + cd * 0.5);
    if core.is_empty() || !poly_inside(c, &core) {
        return None;
    }
    let h1 = top + fh * 0.8;
    extrude(g, f, Concrete, &core, y0, h1, color);
    parapet(g, c, &core, h1, 0.6, 0.15, Concrete, mul(color, 0.9));
    let outer_u = if right >= left { 1.0 } else { -1.0 };
    let floors = ((top - y0) / fh).floor() as i32;
    for face in c.faces(&core) {
        // The face against the tower is inside it.
        if face.len < 1.5 || face.out.x * outer_u < -0.5 {
            continue;
        }
        let side = face.out.x * outer_u > 0.5;
        // Slit windows up the stair, one a landing.
        for k in 0..floors {
            let y = y0 + k as f32 * fh + fh * 0.55;
            let pane = if coin(rng, c.lit_chance() * 0.6) { Pane::Lit(mul(c.palette.window_cold, 0.55)) } else { Pane::Dark };
            face.glaze(g, c, if side { 0.0 } else { face.len * 0.2 }, y, 0.04, 0.45, fh * 0.55, pane);
        }
        grime(g, c, rng, &face, y0, h1, 1.5);
        if side {
            // The pipe ladder up the outside.
            pipe_run(g, c, rng, &face, -face.len * 0.32, y0, h1, fh, 3);
            pipe_run(g, c, rng, &face, face.len * 0.34, y0, top, fh, 1);
        }
    }
    // Lift motor room.
    let room_poly = fit_inside(c, &scale_poly(&core, 0.7));
    extrude(g, f, Concrete, &room_poly, h1, h1 + 2.2, mul(color, 0.92));
    g.lod(Lod::Near, |g| {
        let (p, h) = (poly_center(&room_poly), rng.random_range(2.0..3.5));
        yagi(g, c, rng, p, h1 + 2.2, h);
    });
    Some(core)
}

// ── the roof ─────────────────────────────────────────────────────────────────

/// A TV aerial: a mast with a boom and three to five elements across it.
fn yagi(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32, h: f32) {
    let f = &c.frame;
    let col = mul(c.palette.steel, 0.55);
    f.bx(g, Matte, p.x, y + h * 0.5, p.y, Vec3::new(0.05, h, 0.05), col);
    let yaw = rng.random_range(0.0..std::f32::consts::PI);
    let boom = rng.random_range(1.0..1.8);
    let (bx, bz) = (yaw.cos(), yaw.sin());
    f.bx_yaw(g, Matte, p.x, y + h - 0.2, p.y, Vec3::new(boom, 0.04, 0.04), col, yaw);
    let n = rng.random_range(3..=5);
    for k in 0..n {
        let s = (k as f32 / (n - 1) as f32 - 0.5) * boom * 0.9;
        let len = 0.9 - 0.12 * k as f32;
        f.bx_yaw(g, Matte, p.x + bx * s, y + h - 0.2, p.y + bz * s, Vec3::new(0.03, 0.03, len), col, yaw);
    }
}

/// A railing round a roof edge, inside it: posts and a top rail.
fn railing(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, color: [f32; 3]) {
    let f = &c.frame;
    for face in c.faces(poly) {
        if face.len < 1.0 {
            continue;
        }
        let n = (face.len / 1.8).ceil() as i32;
        for i in 0..=n {
            let t = -face.len * 0.5 + i as f32 * face.len / n as f32;
            let p = face.at(t, -0.1);
            g.part(Matte).boxed_open(f.at2(p, y + 0.5), Vec3::new(0.05, 1.0, 0.05), f.yaw(), rgba(color));
        }
        face.bx(g, f, Matte, 0.0, y + 1.02, -0.1, Vec3::new(face.len, 0.05, 0.05), color);
        face.bx(g, f, Matte, 0.0, y + 0.55, -0.1, Vec3::new(face.len, 0.03, 0.03), color);
    }
}

/// The clutter on a Hong Kong roof at `y`: a railing round it on top of
/// its parapet (`rail_y` over the roof), TV aerials and a dish (the tanks
/// and shacks are `antenna_forest`'s). Near detail but the dish (mid).
pub fn roof_clutter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, roof: &[Vec2], y: f32, rail_y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let area = nd_sim::zoning::polygon_area(roof).abs();
    if area < 6.0 {
        return;
    }
    let inner = offset_poly(roof, 0.5);
    let (s, rail) = outdoor_steel(c, mul(pal.steel, 0.6));
    let rail = if s == Rust { mul(pal.rust, 0.8) } else { rail };
    g.lod(Lod::Near, |g| {
        railing(g, c, roof, y + rail_y, rail);
        let n = ((area / 30.0) as i32).clamp(1, 5);
        for _ in 0..n {
            if let Some(p) = random_in_poly(rng, &inner, 0.4) {
                let h = rng.random_range(2.2..4.5);
                yagi(g, c, rng, p, y, h);
            }
        }
    });
    if coin(rng, 0.45) {
        if let Some(p) = random_in_poly(rng, &inner, 1.0) {
            let r = rng.random_range(0.4..0.8);
            g.lod(Lod::Mid, |g| dish(g, f, p, y, r, pal.steel));
        }
    }
}

/// A vertical neon sign on the corner of a face near `end` (+1 the face's
/// `b` end, -1 its `a` end), running up `floors` storeys from `y0`.
pub fn corner_neon(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, end: f32, y0: f32, h: f32) {
    if face.len < 3.0 || c.reach_out(face) < 0.6 || h < 3.0 {
        return;
    }
    vertical_sign(g, c, rng, face, end * (face.len * 0.5 - 0.9), y0, h);
}

/// The end of a face nearer the street (the lot's -v side): +1 its `b`
/// end, -1 its `a` end.
pub fn street_end(face: &Face) -> f32 {
    if face.b.y < face.a.y { 1.0 } else { -1.0 }
}
