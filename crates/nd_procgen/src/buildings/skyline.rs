//! [r4-skyline] Skyline Luxe, Neo Tokyo Heights' look: luxury towers for
//! the rich over a hectic street. Slim towers of smeared tinted glass on
//! podiums of noisy little shops (signs, awnings, vending machines, mopeds
//! parked nose to tail), twin towers joined by a sky bridge, a tower with a
//! pool and cabanas on its roof, a hotel or office tower with a helipad, a
//! vertical mall podium wrapped in screens, a boutique row under neon; and
//! for its rich, a sky villa: a low block crowned by a glass penthouse with
//! an infinity pool and a terrace garden (a helipad on a billionaire's).
//!
//! Money upstairs, grit downstairs: the glass is dark and smeared, the
//! podium's foot is black with dirt, plant and AC crowd every roof, the
//! shops are cheap and lit late. From section 2 of the style guide it takes
//! the cantilevered glass-box floors stacked out of the frame, the exposed
//! bracing up a tower's flank, big lit brand signs (fictional brands) and
//! corner screens, railings round every roof and terrace, rooftop plant,
//! the deep soffit over the shops with its strip lights, and the clutter
//! along the pavement.
//!
//! By night the towers light a floor at a time (one window light per floor
//! and face, the renderer's to switch), soffit lights, pools, lounges,
//! bridges and lamps come on (base glow marked `NIGHT_MARK`, lit only after
//! dark), and the signs and screens glow as signs do. Drawn floors follow
//! the sim's (`Ctx::floors`): a refit keeps its height.
//!
//! Lot-local as every style: `u` along the street, `v` in from it. Everything
//! comes from the building's seed, the same from every camera. Mullions,
//! braces, awnings, cabanas, mopeds' bodies and plant are mid detail;
//! rails, smears, loungers, wheels and window frames near.

use super::glasscampus::{curtain_block, lamp, night_box, rails, roof_plant};
use super::kit::*;
use super::parts::*;
use super::{Ctx, Extras, Face};
use crate::depth::PANE;
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Tower glass: blue-black, champagne bronze, deep teal, smoke.
const TINTS: [[f32; 3]; 4] = [[0.07, 0.09, 0.13], [0.16, 0.13, 0.09], [0.07, 0.12, 0.14], [0.11, 0.11, 0.13]];
/// What the weather and the street's soot smear onto the glass.
const SMEAR: [f32; 3] = [0.30, 0.29, 0.27];
/// Spandrels and fins: graphite, champagne, black.
const SPANDRELS: [[f32; 3]; 3] = [[0.15, 0.15, 0.16], [0.52, 0.46, 0.36], [0.06, 0.06, 0.07]];
/// Brass and bronze trim for the money.
const BRASS: [f32; 3] = [0.60, 0.46, 0.22];
/// Street-level dirt.
const DIRT: [f32; 3] = [0.07, 0.065, 0.06];
/// Mopeds and delivery scooters: red, cream, black, mint, yellow.
const MOPEDS: [[f32; 3]; 5] = [[0.62, 0.08, 0.06], [0.80, 0.76, 0.66], [0.06, 0.06, 0.07], [0.36, 0.66, 0.56], [0.86, 0.68, 0.10]];
/// Vending machine fronts (lit day and night, as signs).
const VENDING: [[f32; 3]; 4] = [[1.9, 1.9, 2.0], [2.0, 0.5, 0.4], [0.4, 1.4, 2.2], [1.9, 1.5, 0.4]];
/// Pool water by day.
const WATER: [f32; 3] = [0.10, 0.42, 0.52];
/// A lit pool after dark.
const POOL_LIGHT: [f32; 3] = [0.35, 1.4, 1.7];
/// Lounges, bridges and the penthouse: warm.
const LOUNGE: [f32; 3] = [1.75, 1.25, 0.80];
/// Soffit strips over the shops: cold white.
const SOFFIT: [f32; 3] = [1.9, 1.9, 1.8];
/// Cabana canvas: cream, black, sand.
const CANVAS: [[f32; 3]; 3] = [[0.84, 0.80, 0.70], [0.10, 0.10, 0.11], [0.70, 0.60, 0.44]];

/// Fictional brands for the towers' big lit signs: a name (its letters are
/// the sign's glyph blocks) and its colour, lit.
const BRANDS: [(&str, [f32; 3]); 8] = [
    ("KAIROS", [2.6, 0.35, 1.4]),
    ("NOVASEI", [0.3, 2.2, 2.6]),
    ("TAKAMINE", [2.5, 1.8, 0.5]),
    ("MIRAIQ", [1.4, 0.5, 2.7]),
    ("SORA-9", [2.4, 2.4, 2.5]),
    ("LUMENKAI", [0.5, 2.5, 1.2]),
    ("ZENKO", [2.7, 0.5, 0.35]),
    ("VANTA", [2.2, 1.3, 2.5]),
];

fn glass(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mix(TINTS[rng.random_range(0..TINTS.len())], SMEAR, 0.06 + 0.3 * c.wear)
}

fn spandrel(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(SPANDRELS[rng.random_range(0..SPANDRELS.len())], 1.0 - 0.2 * c.wear)
}

/// The podium's walls: the look's dark tints, dirtier with wear.
fn podium_wall(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mix(pick(rng, &c.style.wall_colors), DIRT, 0.15 + 0.35 * c.wear)
}

fn steel(c: &Ctx) -> [f32; 3] {
    mix(mul(c.palette.steel, 0.7), c.palette.rust, 0.3 * c.wear)
}

// ── signs and the street ──────────────────────────────────────────────────────

/// A big lit brand sign on a face at `t`, centred at height `y`, `w` by
/// `h`: a dark board (base), one glow block per letter of a fictional
/// brand's name in its colour, some dead with wear, a lit rule under them;
/// its frame mid detail. Signs glow day and night, as every sign does.
#[allow(clippy::too_many_arguments)]
fn brand_sign(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    if w < 1.5 || h < 0.5 {
        return;
    }
    let (name, col) = BRANDS[rng.random_range(0..BRANDS.len())];
    let letters: Vec<char> = name.chars().collect();
    let n = letters.len() as f32;
    face.bx(g, f, Matte, t, y, 0.2, Vec3::new(w + 0.3, h + 0.3, 0.14), DARK);
    let lw = w / n;
    let k = 1.0 - 0.3 * c.wear;
    for (i, ch) in letters.iter().enumerate() {
        if *ch == '-' || coin(rng, 0.25 * c.wear) {
            continue;
        }
        let lt = t - w * 0.5 + (i as f32 + 0.5) * lw;
        face.decal(g, f, Glow, lt, y + h * 0.08, 0.28, lw * 0.72, h * 0.66, mul(col, k));
    }
    face.decal(g, f, Glow, t, y - h * 0.38, 0.28, w * 0.9, h * 0.06, mul(col, 0.7 * k));
    g.lod(Lod::Mid, |g| {
        face.bx(g, f, Matte, t, y + h * 0.5 + 0.2, 0.2, Vec3::new(w + 0.5, 0.1, 0.2), CHARCOAL);
        face.bx(g, f, Matte, t, y - h * 0.5 - 0.2, 0.2, Vec3::new(w + 0.5, 0.1, 0.2), CHARCOAL);
    });
}

/// A vending machine against a face at `t`: the cabinet and its lit front
/// (lit day and night, as signs are), a bin beside it (mid).
fn vending(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32) {
    let f = &c.frame;
    if c.reach_out(face) < 1.0 {
        return;
    }
    let body = mul(pick(rng, &c.palette.metal_paint), 0.8 - 0.3 * c.wear);
    face.bx(g, f, Metal, t, 0.95, 0.42, Vec3::new(0.9, 1.9, 0.72), body);
    let lit = mul(VENDING[rng.random_range(0..VENDING.len())], 1.0 - 0.4 * c.wear);
    face.decal(g, f, Glow, t - 0.08, 1.15, 0.79, 0.6, 1.2, lit);
    g.lod(Lod::Mid, |g| {
        face.bx(g, f, Matte, t + 0.75, 0.45, 0.36, Vec3::new(0.45, 0.9, 0.5), mul(c.palette.steel, 0.5));
        face.decal(g, f, Matte, t + 0.3, 0.5, 0.79, 0.15, 0.3, DARK);
    });
}

/// Mopeds and delivery scooters parked nose to tail from lot-local `p`
/// along `dir`: bodies mid detail, wheels, seats and boxes near; only where
/// they stand on the lot.
fn mopeds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, dir: Vec2, n: i32) {
    let f = &c.frame;
    let across = Vec2::new(-dir.y, dir.x);
    let yaw = across.y.atan2(across.x) + rng.random_range(-0.25..0.25);
    let picks: Vec<(Vec2, [f32; 3], bool)> = (0..n)
        .map(|i| {
            let q = p + dir * (i as f32 * 0.85) + across * rng.random_range(-0.15..0.15);
            (q, MOPEDS[rng.random_range(0..MOPEDS.len())], coin(rng, 0.4))
        })
        .filter(|(q, _, _)| c.inside_local(*q) && distance_to_boundary(*q, &c.lot_poly) > 1.0)
        .collect();
    g.lod(Lod::Mid, |g| {
        for (q, col, _) in &picks {
            let col = mul(*col, 1.0 - 0.35 * c.wear);
            f.bx_yaw(g, Matte, q.x, 0.55, q.y, Vec3::new(1.5, 0.45, 0.34), col, yaw);
            let front = *q + Vec2::new(yaw.cos(), yaw.sin()) * 0.62;
            f.bx_yaw(g, Matte, front.x, 0.9, front.y, Vec3::new(0.12, 0.7, 0.4), col, yaw);
        }
    });
    g.lod(Lod::Near, |g| {
        let d = Vec2::new(yaw.cos(), yaw.sin());
        for (q, _, boxed) in &picks {
            for s in [-0.55f32, 0.55] {
                let w = *q + d * s;
                let axle = f.dir3(Vec2::new(-d.y, d.x)) * 0.06;
                let a = f.at2(w, 0.27);
                g.part(Matte).tube(a - axle, a + axle, 0.26, 8, rgba(DARK));
            }
            f.bx_yaw(g, Matte, q.x - d.x * 0.15, 0.84, q.y - d.y * 0.15, Vec3::new(0.65, 0.1, 0.3), DARK, yaw);
            if *boxed {
                let b = *q - d * 0.55;
                f.bx_yaw(g, Matte, b.x, 1.05, b.y, Vec3::new(0.45, 0.42, 0.45), mul(VENDING[1], 0.3), yaw);
            }
        }
    });
}

/// A run of shop units along a face, from `t0` to `t1`: narrow fronts of
/// lit glass or a shutter half down, a lit fascia sign over each, awnings
/// on some (mid), now and then a vending machine pair between them.
fn shop_units(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t0: f32, t1: f32, fh: f32) {
    let f = &c.frame;
    let len = t1 - t0;
    if len < 2.5 {
        return;
    }
    let n = ((len / 4.2).floor() as i32).clamp(1, 14);
    let w = len / n as f32;
    let sh = (fh - 1.1).clamp(2.2, 3.2);
    for i in 0..n {
        let t = t0 + (i as f32 + 0.5) * w;
        if coin(rng, 0.12 + 0.35 * c.wear) {
            // A shutter, down or half down, stickered.
            let down = rng.random_range(0.5..1.0);
            face.glaze(g, c, t, 0.15 + sh * 0.5, 0.04, w * 0.86, sh, Pane::Dark);
            let col = mul(pick(rng, &c.palette.metal_paint), 0.7 - 0.25 * c.wear);
            face.decal(g, f, Metal, t, 0.15 + sh - sh * down * 0.5, 0.07, w * 0.88, sh * down, col);
            g.lod(Lod::Near, |g| {
                let k = ((sh * down) / 0.25).floor() as i32;
                for j in 1..k.min(12) {
                    face.decal(g, f, Matte, t, 0.15 + sh - j as f32 * 0.25, 0.075, w * 0.88, 0.03, mul(col, 0.6));
                }
            });
        } else {
            face.glaze(g, c, t, 0.15 + sh * 0.5, 0.04, w * 0.86, sh, Pane::Dark);
            if coin(rng, 0.55 + 0.4 * c.lit_chance()) {
                let col = mul(pick(rng, &c.palette.glow_colors), rng.random_range(0.5..0.8));
                face.glaze(g, c, t, 0.15 + sh * 0.5, 0.06, w * 0.8, sh * 0.85, Pane::Lit(col));
            }
            g.lod(Lod::Near, |g| face.window_frame(g, c, t, 0.15 + sh * 0.5, w * 0.86, sh, 0.12, CHARCOAL));
        }
        sign_board(g, c, rng, face, t, sh + 0.55, (w * 0.72).min(3.6), 0.42);
        // Awnings only where both their front corners stand over the lot.
        if coin(rng, 0.45) && c.inside_local(face.at(t - w * 0.45, 1.5)) && c.inside_local(face.at(t + w * 0.45, 1.5)) {
            g.lod(Lod::Mid, |g| awning(g, c, rng, face, t, sh + 0.05, w * 0.8));
        }
    }
    // Vending machines between two units, out by the door.
    if n >= 2 && coin(rng, 0.7) {
        let k = rng.random_range(1..n);
        let t = t0 + k as f32 * w;
        vending(g, c, rng, face, t - 0.5);
        if coin(rng, 0.5) {
            vending(g, c, rng, face, t + 0.5);
        }
    }
}

/// The hectic street level on `r`: a podium of `floors` storeys of the
/// look's dark walls, black with dirt at the foot; shop units along the
/// street, a deep soffit over them with strip lights under it after dark,
/// a sign hung out over the pavement, a screen or billboard on an upper
/// storey; AC units and pipes up the sides (near), plant on its roof;
/// mopeds parked along the kerb. Returns its top.
fn street_podium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, floors: i32, fh: f32) -> f32 {
    let f = &c.frame;
    let h = floors as f32 * fh;
    let wall = podium_wall(c, rng);
    block(g, c, Concrete, r, 0.0, h, wall);
    let front = r.front();
    // Dirt at the foot of every face, grime down them.
    for face in r.faces() {
        face.decal(g, f, Matte, 0.0, 0.5, 0.011, face.len - 0.05, 1.0, mix(wall, DIRT, 0.7));
        grime(g, c, rng, &face, 0.0, h, 0.8 + 2.0 * c.wear);
    }
    shop_units(g, c, rng, &front, -front.len * 0.5 + 0.3, front.len * 0.5 - 0.3, fh);
    // A side open to the street (a corner lot's) gets shops too; else AC
    // boxes and a downpipe.
    let side = if c.seed % 2 == 0 { r.left() } else { r.right() };
    if c.reach_out(&side) > 3.0 && side.len > 6.0 {
        shop_units(g, c, rng, &side, -side.len * 0.5 + 0.3, -side.len * 0.5 + (side.len * 0.5).min(13.0), fh);
    }
    for face in [r.left(), r.right()] {
        g.lod(Lod::Near, |g| {
            let n = ((face.len / 4.0).floor() as i32).clamp(0, 6);
            for i in 0..n {
                let t = -face.len * 0.5 + (i as f32 + 0.75) * face.len / n.max(1) as f32;
                for k in 0..floors {
                    if coin(rng, 0.6) {
                        ac_unit(g, c, &face, t, k as f32 * fh + fh * 0.75, 0.2);
                    }
                }
            }
        });
    }
    // The deep soffit over the shops, strip lights under it after dark.
    let reach = (c.reach_out(&front) - 0.2).clamp(0.0, 2.2);
    if reach > 0.7 {
        let sy = fh - 0.15;
        front.bx(g, f, Concrete, 0.0, sy, reach * 0.5 - 0.1, Vec3::new(front.len - 0.1, 0.36, reach + 0.2), mul(wall, 0.8));
        let n = ((front.len / 3.0).floor() as i32).clamp(1, 12);
        let from = g.part(Glow).positions.len();
        for i in 0..n {
            let t = -front.len * 0.5 + (i as f32 + 0.5) * front.len / n as f32;
            front.bx(g, f, Glow, t, sy - 0.2, reach * 0.55, Vec3::new(1.4, 0.05, 0.14), mul(SOFFIT, 0.8 - 0.3 * c.wear));
        }
        mark_glow(g, from, NIGHT_MARK);
    }
    // Upstairs: a screen or a billboard, a sign hung over the pavement.
    if floors >= 2 {
        let y = fh + (h - fh) * 0.5;
        if front.len > 9.0 {
            let t = if c.seed % 3 == 0 { -front.len * 0.22 } else { front.len * 0.22 };
            led_screen(g, c, rng, &front, t, y, (front.len * 0.35).min(9.0), (h - fh - 1.0).clamp(1.2, 4.0));
        }
        pane_row(g, c, rng, &front, fh, floors - 1, fh, 3.0, 1.4, fh * 0.45, false);
        hanging_sign(g, c, rng, &front, -front.len * 0.42, fh + 1.6, 1.6, 2.4);
    } else {
        hanging_sign(g, c, rng, &front, -front.len * 0.42, fh + 0.4, 1.4, 0.8);
    }
    // Mopeds along the kerb.
    let kerb = (c.reach_out(&front) - 0.2).min(2.4);
    if kerb > 1.6 {
        let n = rng.random_range(2..7);
        let p = front.at(front.len * 0.1, kerb * 0.6);
        mopeds(g, c, rng, p, front.tangent, n);
    }
    parapet(g, c, &r.poly(), h, 0.6, 0.18, Concrete, mul(wall, 0.85));
    h
}

// ── towers ────────────────────────────────────────────────────────────────────

/// A glass box hung off a face at `t`: `w` wide, `out` proud, from `y0`
/// for `floors` storeys of `fh`; its back reaches into the tower, a panel
/// soffit under it, a light per floor on its front, its mullions mid, rails
/// round its top (near). The cantilevered floors stacked out of the frame.
#[allow(clippy::too_many_arguments)]
fn glass_box(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, out: f32, y0: f32, floors: i32, fh: f32, tint: [f32; 3], span: [f32; 3]) {
    let f = &c.frame;
    let h = floors as f32 * fh;
    if w < 2.0 || out < 0.6 || floors < 1 {
        return;
    }
    face.bx(g, f, Glass, t, y0 + 0.3 + h * 0.5, out * 0.5 - 0.15, Vec3::new(w, h, out + 0.3), mul(tint, 1.15));
    // The slabs stand 8 cm proud of the glass, their backs short of its.
    face.bx(g, f, Panel, t, y0 + 0.16, out * 0.5 - 0.07, Vec3::new(w + 0.2, 0.32, out + 0.3), mul(span, 0.9));
    face.bx(g, f, Panel, t, y0 + 0.3 + h + 0.12, out * 0.5 - 0.07, Vec3::new(w + 0.2, 0.24, out + 0.3), mul(span, 0.9));
    let front = Face::new(face.at(t - w * 0.5, out), face.at(t + w * 0.5, out), face.out);
    for k in 0..floors {
        let yb = y0 + 0.3 + k as f32 * fh;
        front.decal(g, f, Panel, 0.0, yb + 0.25, 0.035, w - 0.2, 0.5, span);
        if coin(rng, c.lit_chance() * 0.9) {
            let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.85));
            front.glaze(g, c, 0.0, yb + 0.5 + (fh - 0.5) * 0.5, PANE, w - 0.5, fh - 0.9, Pane::Lit(col));
        }
    }
    g.lod(Lod::Mid, |g| {
        let n = ((w / 1.5).floor() as i32).max(1);
        for i in 1..n {
            let tt = -w * 0.5 + i as f32 * w / n as f32;
            front.decal(g, f, Matte, tt, y0 + 0.3 + h * 0.5, 0.065, 0.08, h, mul(SPANDRELS[0], 0.8));
        }
    });
    let top = R::of(&[face.at(t - w * 0.5, 0.0), face.at(t + w * 0.5, out)]);
    rails(g, c, &top.inset(0.17), y0 + 0.3 + h + 0.24, mul(BRASS, 0.8));
}

/// Exposed bracing up a tower's flank: two steel columns standing off the
/// face (base) and an X across every four floors between them (mid).
fn bracing(g: &mut Geometry, c: &Ctx, face: &Face, y0: f32, y1: f32, fh: f32) {
    let f = &c.frame;
    if face.len < 6.0 || y1 - y0 < fh * 3.0 {
        return;
    }
    let half = face.len * 0.5 - 0.6;
    let (a, b) = (face.at(-half, 0.55), face.at(half, 0.55));
    if !(c.inside_local(a) && c.inside_local(b)) {
        return;
    }
    let col = steel(c);
    for t in [-half, half] {
        face.bx(g, f, Matte, t, (y0 + y1) * 0.5, 0.55, Vec3::new(0.4, y1 - y0, 0.4), col);
    }
    let bay = fh * 4.0;
    let n = ((y1 - y0) / bay).floor() as i32;
    g.lod(Lod::Mid, |g| {
        for k in 0..n {
            let (ya, yb) = (y0 + k as f32 * bay, y0 + (k + 1) as f32 * bay);
            g.part(Matte).beam(f.at2(a, ya + 0.2), f.at2(b, yb - 0.2), 0.22, rgba(col));
            g.part(Matte).beam(f.at2(b, ya + 0.2), f.at2(a, yb - 0.2), 0.22, rgba(col));
            face.bx(g, f, Matte, 0.0, yb, 0.55, Vec3::new(face.len - 1.0, 0.3, 0.3), col);
        }
    });
}

/// The crown of a tower on `r` at `top`: a big brand sign on the street
/// face just under the roof, a corner screen down the flank on a tall
/// one, plant and rails on the roof, an aviation light on a mast.
fn crown(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, top: f32, plant: bool) {
    let front = r.front();
    let sw = (front.len * 0.7).min(14.0);
    if front.len > 5.0 && top > 14.0 {
        brand_sign(g, c, rng, &front, 0.0, top - 1.8, sw, (sw * 0.16).clamp(0.8, 2.2));
    }
    if top > 40.0 {
        let side = if c.seed % 2 == 0 { r.right() } else { r.left() };
        let sh = (top * 0.35).min(28.0);
        led_screen(g, c, rng, &side, side.len * 0.5 - 1.47, top - 4.5 - sh * 0.5, 1.8, sh);
    }
    if plant {
        roof_plant(g, c, rng, &r.inset(0.2), top);
    } else {
        rails(g, c, &r.inset(0.25), top, mul(c.palette.steel, 0.7));
    }
    if top > 30.0 {
        let m = Vec2::new(r.u0 + 0.6, r.v1 - 0.6);
        c.frame.bx(g, Matte, m.x, top + 2.5, m.y, Vec3::new(0.22, 5.0, 0.22), c.palette.steel);
        g.lod(Lod::Mid, |g| c.frame.bx(g, Matte, m.x, top + 3.6, m.y, Vec3::new(1.4, 0.06, 0.06), c.palette.steel));
        aviation_light_local(g, &c.frame, c.palette, m.x, top + 5.2, m.y, 0.35);
    }
}

/// A slim curtain-walled tower on `r` from `y0`, `floors` of `fh`: glass
/// boxes hung off its front every few floors, exposed bracing up one flank,
/// and its crown. Returns its top.
#[allow(clippy::too_many_arguments)]
fn slim_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y0: f32, floors: i32, fh: f32, plant: bool) -> f32 {
    let tint = glass(c, rng);
    let span = spandrel(c, rng);
    if floors < 1 {
        return y0;
    }
    let top = curtain_block(g, c, rng, r, y0, floors, fh, tint, span);
    // Cantilevered glass boxes off the front, alternating sides.
    let front = r.front();
    let every = rng.random_range(4..7);
    let bw = (front.len * 0.45).clamp(2.5, 9.0);
    let mut k = 2;
    let mut i = 0;
    while k + 2 <= floors - 1 && front.len > 6.0 {
        let t = if (i + c.seed as i32) % 2 == 0 { -front.len * 0.5 + bw * 0.5 + 0.4 } else { front.len * 0.5 - bw * 0.5 - 0.4 };
        let out = 1.6f32.min((c.reach_out(&front) - 0.4).max(0.0));
        glass_box(g, c, rng, &front, t, bw, out, y0 + k as f32 * fh, 2, fh, tint, span);
        k += every;
        i += 1;
    }
    let flank = if c.seed % 2 == 0 { r.left() } else { r.right() };
    bracing(g, c, &flank, y0, top - 0.4, fh);
    crown(g, c, rng, r, top, plant);
    top
}

/// The tower's footprint over a podium `pod`: slim (an eighth narrower than
/// the podium or more), set back from the street, shifted to one side.
fn tower_rect(c: &Ctx, pod: &R, share: f32) -> R {
    let tw = (pod.w() * share).clamp(7.0, 22.0).min(pod.w() - 1.0);
    let td = (pod.d() * 0.68).clamp(6.5, 18.0).min(pod.d() - 1.5);
    let left = c.seed % 3 == 0;
    let tu = if left { pod.u0 + 0.8 + tw * 0.5 } else if c.seed % 3 == 1 { pod.u1 - 0.8 - tw * 0.5 } else { pod.c().x };
    let tv0 = pod.v0 + (pod.d() - td).clamp(0.8, 3.0);
    let tr = R::new(tu - tw * 0.5, tu + tw * 0.5, tv0, tv0 + td);
    R::new(tr.u0.max(pod.u0 + 0.5), tr.u1.min(pod.u1 - 0.5), tr.v0, tr.v1.min(pod.v1 - 0.5))
}

/// The podium for a lot: the lot's own rectangle a metre in from the
/// street, as deep as a tower wants.
fn podium_rect(c: &Ctx, min: f32) -> Option<R> {
    let area = R::of(&c.poly);
    fit(c, R::new(area.u0 + 0.4, area.u1 - 0.4, area.v0 + 1.4, area.v0 + 1.4 + (area.d() - 1.8).min(26.0)), min)
}

/// A street corner kiosk with a vending machine, for a lot too small for
/// anything else.
fn kiosk(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let a = R::of(&c.poly);
    let Some(r) = fit(c, R::new(a.c().x - 1.8, a.c().x + 1.8, a.v0 + 0.8, a.v0 + 3.6), 1.4) else {
        let p = a.c();
        c.frame.bx(g, Metal, p.x, 1.0, p.y, Vec3::new(0.9, 2.0, 0.7), mul(c.palette.steel, 0.6));
        return;
    };
    block(g, c, Panel, &r, 0.0, 2.8, podium_wall(c, rng));
    let fr = r.front();
    shop_units(g, c, rng, &fr, -fr.len * 0.5 + 0.1, fr.len * 0.5 - 0.1, 3.3);
    block(g, c, Concrete, &r.inset(-0.15), 2.8, 3.05, mul(SPANDRELS[0], 1.2));
}

/// How many of a tower's floors go to its podium of shops.
fn podium_floors(floors: i32) -> i32 {
    match floors {
        0..=3 => 1,
        4..=14 => 2,
        _ => 3,
    }
}

/// A slim glass residence tower on a podium of noisy shops: the hectic
/// street level, the tower set back over it, glass boxes hung off it,
/// braced up a flank, a brand sign and plant at the crown; the podium roof
/// a terrace with rails and planters.
pub fn luxe_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let fh = c.fh().clamp(3.1, 3.4);
    let Some(pod) = podium_rect(c, 7.0) else { return kiosk(g, c, rng) };
    let floors = (c.floors as i32).clamp(2, 80);
    let pf = podium_floors(floors);
    let ph = street_podium(g, c, rng, &pod, pf, fh + 0.4);
    let tr = tower_rect(c, &pod, 0.55);
    slim_tower(g, c, rng, &tr, ph, floors - pf, fh, true);
    podium_terrace(g, c, rng, &pod, &tr, ph);
}

/// The podium's roof round a tower: planters along the rails (mid), a
/// couple of trees, AC boxes in a corner.
fn podium_terrace(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pod: &R, tower: &R, y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    rails(g, c, &pod.inset(0.3), y, mul(BRASS, 0.7));
    let strip = R::new(pod.u0 + 0.8, pod.u1 - 0.8, pod.v0 + 0.6, (tower.v0 - 0.4).max(pod.v0 + 0.6));
    if strip.d() > 1.2 && strip.w() > 4.0 {
        let n = ((strip.w() / 3.5).floor() as i32).clamp(1, 8);
        g.lod(Lod::Mid, |g| {
            for i in 0..n {
                let u = strip.u0 + (i as f32 + 0.5) * strip.w() / n as f32;
                f.bx(g, Concrete, u, y + 0.35, strip.v0 + 0.4, Vec3::new(1.6, 0.7, 0.7), mul(SPANDRELS[0], 1.3));
                f.bx(g, Foliage, u, y + 0.85, strip.v0 + 0.4, Vec3::new(1.4, 0.4, 0.55), mul(pal.foliage, 0.85 - 0.3 * c.wear));
            }
        });
    }
    // AC boxes and a chiller on whichever side the tower leaves free.
    let free = if tower.u0 - pod.u0 > pod.u1 - tower.u1 { R::new(pod.u0 + 0.6, tower.u0 - 0.6, tower.v0, pod.v1 - 0.6) } else { R::new(tower.u1 + 0.6, pod.u1 - 0.6, tower.v0, pod.v1 - 0.6) };
    if free.w() > 2.2 && free.d() > 2.2 {
        chiller(g, c, free.c(), y, (free.w() - 0.6).min(3.4), 1.6f32.min(free.d() - 0.4), 0.0, mul(pal.steel, 0.85 - 0.2 * c.wear));
        if free.d() > 5.0 {
            g.lod(Lod::Mid, |g| {
                for k in 0..3 {
                    let p = Vec2::new(free.c().x, free.v0 + 0.6 + k as f32 * 0.9);
                    f.bx(g, Metal, p.x, y + 0.35, p.y, Vec3::new(0.8, 0.6, 0.45), mul(pal.steel, 0.9));
                }
            });
        }
    }
    let _ = rng;
}

/// Twin towers of different heights on one podium, joined by a glazed sky
/// bridge (two on tall ones), lit inside after dark; on a lot too narrow
/// for two, one tower.
pub fn twin_towers(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let fh = c.fh().clamp(3.1, 3.4);
    let Some(pod) = podium_rect(c, 7.0) else { return kiosk(g, c, rng) };
    let floors = (c.floors as i32).clamp(2, 80);
    if pod.w() < 17.5 || floors < 6 {
        return luxe_tower(g, c, rng, e);
    }
    let pf = podium_floors(floors);
    let ph = street_podium(g, c, rng, &pod, pf, fh + 0.4);
    let gap = (pod.w() * 0.18).clamp(4.0, 10.0);
    let tw = ((pod.w() - gap - 1.6) * 0.5).min(16.0);
    let td = (pod.d() * 0.62).clamp(6.5, 16.0).min(pod.d() - 1.5);
    let tv0 = pod.v0 + (pod.d() - td).clamp(0.8, 3.0);
    let m = pod.c().x;
    let a = R::new(m - gap * 0.5 - tw, m - gap * 0.5, tv0, tv0 + td);
    let b = R::new(m + gap * 0.5, m + gap * 0.5 + tw, tv0 + 0.6f32.min(pod.v1 - 0.5 - tv0 - td), tv0 + td + 0.6f32.min(pod.v1 - 0.5 - tv0 - td));
    let rest = floors - pf;
    // The second tower a few floors short of the first (never the sim's
    // count over: the first carries it).
    let short = (rest - (rest / 5).max(1)).max(1);
    let (fa, fb) = if c.seed % 2 == 0 { (rest, short) } else { (short, rest) };
    let ta = slim_tower(g, c, rng, &a, ph, fa, fh, true);
    let tb = slim_tower(g, c, rng, &b, ph, fb, fh, true);
    // Sky bridges between the inner faces.
    let low = ta.min(tb) - ph;
    let levels: Vec<f32> = if low > 60.0 { vec![0.45, 0.8] } else { vec![0.7] };
    let v0 = a.v0.max(b.v0) + 1.0;
    let v1 = a.v1.min(b.v1) - 1.0;
    if v1 - v0 > 2.5 && low > fh * 4.0 {
        let bw = (v1 - v0).min(5.0);
        let vc = (v0 + v1) * 0.5;
        for l in levels {
            let y = ph + (low * l / fh).floor() * fh;
            sky_bridge_luxe(g, c, Vec2::new(a.u1 - 0.4, vc), Vec2::new(b.u0 + 0.4, vc), y, bw, fh * 2.0 - 0.4);
        }
    }
    podium_terrace(g, c, rng, &pod, &R::new(a.u0, b.u1, a.v0.min(b.v0), a.v1.max(b.v1)), ph);
}

/// A glazed sky bridge between lot-local `a` and `b` (along `u`) with its
/// floor at `y`, `w` wide and `h` tall: tinted glass, a floor and roof
/// slab, mullions mid; lit inside after dark (a glow box just round the
/// glass, `NIGHT_MARK`).
fn sky_bridge_luxe(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    let len = b.x - a.x;
    if len < 1.5 {
        return;
    }
    let m = (a + b) * 0.5;
    let tint = mul(TINTS[(c.seed % 4) as usize], 1.3);
    f.bx(g, Glass, m.x, y + h * 0.5, m.y, Vec3::new(len, h, w), tint);
    night_box(g, c, m.x, y + h * 0.5, m.y, Vec3::new(len - 0.6, h + 0.04, w + 0.04), mul(LOUNGE, 0.7));
    let slab = mul(SPANDRELS[0], 1.1);
    f.bx(g, Panel, m.x, y - 0.25, m.y, Vec3::new(len + 0.2, 0.5, w + 0.4), slab);
    f.bx(g, Panel, m.x, y + h + 0.2, m.y, Vec3::new(len + 0.2, 0.4, w + 0.4), slab);
    g.lod(Lod::Mid, |g| {
        let n = ((len / 2.0).floor() as i32).max(1);
        for side in [-1.0f32, 1.0] {
            for i in 1..n {
                let u = a.x + i as f32 * len / n as f32;
                f.bx(g, Matte, u, y + h * 0.5, m.y + side * (w * 0.5 + 0.05), Vec3::new(0.1, h, 0.06), DARK);
            }
        }
    });
}

/// A tower with a pool on its roof: a podium of shops, the tower, and up
/// top a deck with an infinity pool lit after dark, cabanas and loungers,
/// a glass balustrade and a bar pavilion; the plant on a lower roof.
pub fn pool_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let fh = c.fh().clamp(3.1, 3.4);
    let Some(pod) = podium_rect(c, 7.0) else { return kiosk(g, c, rng) };
    let floors = (c.floors as i32).clamp(2, 80);
    let pf = podium_floors(floors);
    let ph = street_podium(g, c, rng, &pod, pf, fh + 0.4);
    let tr = tower_rect(c, &pod, 0.62);
    let top = slim_tower(g, c, rng, &tr, ph, floors - pf, fh, false);
    roof_pool(g, c, rng, &tr.inset(0.25), top);
    podium_terrace(g, c, rng, &pod, &tr, ph);
}

/// A pool deck on a roof `r` at `y`: a timber deck, a raised infinity pool
/// along the street side (water glass by day, lit from below after dark),
/// cabanas under canvas (mid), loungers (near), a bar pavilion lit warm
/// after dark, planted tubs, a glass balustrade and rails round it.
fn roof_pool(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    if r.w() < 4.0 || r.d() < 4.0 {
        rails(g, c, r, y, mul(BRASS, 0.8));
        return;
    }
    block(g, c, Wood, &r.inset(0.1), y - 0.05, y + 0.18, mul(pal.wood, 0.75 - 0.25 * c.wear));
    let pd = (r.d() * 0.38).clamp(2.0, 6.0);
    let pool = R::new(r.u0 + 0.7, r.u1 - 0.7, r.v0 + 0.7, r.v0 + 0.7 + pd);
    block(g, c, Concrete, &pool.inset(-0.35), y - 0.08, y + 0.55, mul(SPANDRELS[2], 1.4));
    block(g, c, Glass, &pool, y + 0.3, y + 0.6, mul(WATER, 1.0 - 0.3 * c.wear));
    let from = g.part(Glow).positions.len();
    block(g, c, Glow, &pool.inset(-0.03), y + 0.28, y + 0.63, mul(POOL_LIGHT, 0.8 - 0.3 * c.wear));
    mark_glow(g, from, NIGHT_MARK);
    // A glass balustrade round the deck (base), rails over it (near).
    let ring = r.inset(0.05);
    for face in ring.faces() {
        face.bx(g, f, Glass, 0.0, y + 0.7, -0.05, Vec3::new(face.len, 1.0, 0.06), mul(TINTS[2], 1.6));
    }
    rails(g, c, &r.inset(0.12), y + 0.1, mul(BRASS, 0.85));
    // Cabanas along the back, loungers by the water.
    let back = R::new(r.u0 + 0.6, r.u1 - 0.6, pool.v1 + 1.0, r.v1 - 0.6);
    if back.d() > 2.2 {
        let n = ((back.w() / 3.6).floor() as i32).clamp(0, 5);
        let canvas = CANVAS[rng.random_range(0..CANVAS.len())];
        g.lod(Lod::Mid, |g| {
            for i in 0..n {
                let u = back.u0 + (i as f32 + 0.5) * back.w() / n as f32;
                let v = back.v1 - 1.1;
                for (du, dv) in [(-1.1f32, -0.9f32), (1.1, -0.9), (-1.1, 0.9), (1.1, 0.9)] {
                    f.bx(g, Matte, u + du, y + 1.25, v + dv, Vec3::new(0.08, 2.2, 0.08), DARK);
                }
                f.bx(g, Tarp, u, y + 2.4, v, Vec3::new(2.5, 0.12, 2.1), mul(canvas, 1.0 - 0.3 * c.wear));
                f.bx(g, Tarp, u, y + 0.45, v + 0.2, Vec3::new(1.8, 0.35, 1.4), mul(CANVAS[0], 0.9));
            }
        });
        // A bar pavilion at one end, lit warm after dark.
        if back.w() > 10.0 && back.d() > 3.0 {
            let bar = R::new(back.u1 - 3.2, back.u1, back.v0, back.v0 + 2.6f32.min(back.d() - 0.4));
            block(g, c, Glass, &bar, y + 0.18, y + 2.6, mul(TINTS[1], 1.5));
            let from = g.part(Glow).positions.len();
            block(g, c, Glow, &bar.inset(-0.03), y + 0.3, y + 2.5, mul(LOUNGE, 0.75 - 0.25 * c.wear));
            mark_glow(g, from, NIGHT_MARK);
            block(g, c, Panel, &bar.inset(-0.4), y + 2.6, y + 2.85, mul(SPANDRELS[1], 1.0));
        }
    }
    g.lod(Lod::Near, |g| {
        let n = ((pool.w() / 1.6).floor() as i32).clamp(0, 10);
        for i in 0..n {
            let u = pool.u0 + (i as f32 + 0.5) * pool.w() / n as f32;
            let v = pool.v1 + 0.95;
            if v < r.v1 - 0.8 {
                f.bx(g, Matte, u, y + 0.38, v, Vec3::new(0.6, 0.1, 1.5), CANVAS[(i as usize + c.seed as usize) % CANVAS.len()]);
                f.bx(g, Matte, u, y + 0.55, v + 0.6, Vec3::new(0.6, 0.35, 0.1), CANVAS[(i as usize + c.seed as usize) % CANVAS.len()]);
            }
        }
    });
    // Tubs with small palms at the corners (mid).
    g.lod(Lod::Mid, |g| {
        for p in [Vec2::new(r.u0 + 0.9, r.v1 - 0.9), Vec2::new(r.u1 - 0.9, r.v1 - 0.9)] {
            // The fronds spread two metres and more: not over the lot's edge.
            if distance_to_boundary(p, &c.lot_poly) < 2.8 || !c.inside_local(p) {
                continue;
            }
            f.bx(g, Concrete, p.x, y + 0.5, p.y, Vec3::new(1.0, 0.65, 1.0), mul(SPANDRELS[0], 1.4));
            tree_kind(g, f.to_world(p.x, p.y), y + 0.8, TreeKind::Palm, 2.6, 1.0, pal.bark, mul(pal.foliage, 0.9 - 0.25 * c.wear));
        }
    });
    // Pool lights along the coping after dark.
    let n = ((pool.w() / 3.0).floor() as i32).clamp(1, 8);
    for i in 0..n {
        let u = pool.u0 + (i as f32 + 0.5) * pool.w() / n as f32;
        night_box(g, c, u, y + 0.62, pool.v1 + 0.2, Vec3::new(0.2, 0.06, 0.12), POOL_LIGHT);
    }
}

// ── shops and hotels ──────────────────────────────────────────────────────────

/// A hotel or office tower with a helipad: a podium of shops round a lobby
/// lit all night, a tower of tinted glass braced up a flank, glass boxes
/// hung off it, a screen down a corner, the brand sign under the crown, and
/// a helipad on steel legs over the roof (its lights after dark).
pub fn helipad_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let fh = c.fh().clamp(3.4, 3.8);
    let Some(pod) = podium_rect(c, 7.0) else { return kiosk(g, c, rng) };
    let floors = (c.floors as i32).clamp(2, 80);
    let pf = podium_floors(floors);
    let ph = street_podium(g, c, rng, &pod, pf, fh + 0.4);
    let tr = tower_rect(c, &pod, 0.68);
    let top = slim_tower(g, c, rng, &tr, ph, floors - pf, fh, false);
    helideck(g, c, &tr, top);
    podium_terrace(g, c, rng, &pod, &tr, ph);
}

/// A helipad over a roof `r` at `y`: a deck on steel legs (the legs and
/// its edge net mid), the pad's markings and edge lights lit after dark, a
/// windsock (near). Pushes the pad for the air taxis.
fn helideck(g: &mut Geometry, c: &Ctx, r: &R, y: f32) {
    let f = &c.frame;
    let rad = (r.w().min(r.d()) * 0.46).clamp(2.6, 9.0);
    let p = r.c();
    let deck = y + 1.4;
    let col = steel(c);
    g.lod(Lod::Mid, |g| {
        for k in 0..4 {
            let a = k as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
            let q = p + Vec2::new(a.cos(), a.sin()) * rad * 0.6;
            f.bx(g, Matte, q.x, y + 0.7, q.y, Vec3::new(0.3, 1.4, 0.3), col);
        }
    });
    let from = g.part(Glow).positions.len();
    helipad(g, c, p, deck - 0.3, rad);
    let ring = 12;
    for k in 0..ring {
        let a = k as f32 * std::f32::consts::TAU / ring as f32;
        let q = p + Vec2::new(a.cos(), a.sin()) * (rad - 0.25);
        f.bx(g, Glow, q.x, deck + 0.08, q.y, Vec3::new(0.18, 0.1, 0.18), [0.4, 2.0, 0.6]);
    }
    mark_glow(g, from, NIGHT_MARK);
    // The windsock on the roof's corner, clear of the pad.
    let w = Vec2::new(r.u1 - 0.4, r.v1 - 0.4);
    if w.distance(p) > rad + 0.4 {
        g.lod(Lod::Near, |g| {
            f.bx(g, Matte, w.x, y + 1.5, w.y, Vec3::new(0.06, 3.0, 0.06), DARK);
            f.bx(g, Tarp, w.x + 0.45, y + 2.85, w.y, Vec3::new(0.9, 0.22, 0.22), [0.9, 0.35, 0.08]);
        });
    }
}

/// A vertical mall: levels stacked up from the street, every other one
/// cantilevered out over the pavement, a band of tenant signs and lit
/// glass on each, big screens across two or three levels, a glass atrium
/// entrance lit all night, billboards on the roof; the street's clutter
/// at its feet. Over six levels the rest of its floors rise as a tower.
pub fn mall_podium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let fh = c.fh().clamp(3.8, 4.4);
    let Some(pod) = podium_rect(c, 9.0) else { return boutique_row(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(1, 80);
    let levels = floors.min(if floors > 6 { 4 } else { 6 });
    let wall = podium_wall(c, rng);
    // The ground level: shops round the entrance.
    block(g, c, Concrete, &pod, 0.0, fh, wall);
    let front = pod.front();
    let aw = (front.len * 0.3).clamp(4.0, 10.0);
    let at = if c.seed % 2 == 0 { -front.len * 0.15 } else { front.len * 0.15 };
    shop_units(g, c, rng, &front, -front.len * 0.5 + 0.3, at - aw * 0.5 - 0.3, fh);
    shop_units(g, c, rng, &front, at + aw * 0.5 + 0.3, front.len * 0.5 - 0.3, fh);
    for face in pod.faces() {
        face.decal(g, f, Matte, 0.0, 0.5, 0.011, face.len - 0.05, 1.0, mix(wall, DIRT, 0.7));
    }
    // The levels above, alternately pushed out over the street.
    let mut y = fh;
    let reach = (c.reach_out(&front) - 0.3).clamp(0.0, 1.8);
    for k in 1..levels {
        let out = if k % 2 == 1 { reach } else { 0.0 };
        let lr = R::new(pod.u0 + if k % 2 == 1 { 0.0 } else { 0.6 }, pod.u1 - if k % 2 == 1 { 0.0 } else { 0.6 }, pod.v0 - out, pod.v1 - 0.4 * k as f32);
        let Some(lr) = fit(c, lr, 6.0) else { break };
        let col = if k % 2 == 1 { mul(SPANDRELS[0], 1.2) } else { mul(wall, 1.05) };
        block(g, c, Panel, &lr, y, y + fh, col);
        let lf = lr.front();
        // A lit glass band (one light per level) and the tenants' signs over it.
        lf.glaze(g, c, 0.0, y + fh * 0.42, 0.04, lf.len - 1.0, fh * 0.5, Pane::Dark);
        if coin(rng, c.lit_chance()) {
            let col = mul(pick(rng, &c.palette.glow_colors), rng.random_range(0.5..0.8));
            lf.glaze(g, c, 0.0, y + fh * 0.42, 0.06, lf.len - 1.4, fh * 0.42, Pane::Lit(col));
        }
        let n = ((lf.len / 5.0).floor() as i32).clamp(1, 8);
        for i in 0..n {
            let t = -lf.len * 0.5 + (i as f32 + 0.5) * lf.len / n as f32;
            if coin(rng, 0.85) {
                sign_board(g, c, rng, &lf, t, y + fh - 0.55, (lf.len / n as f32 * 0.7).min(4.0), 0.5);
            }
        }
        for face in [lr.left(), lr.right()] {
            grime(g, c, rng, &face, y, y + fh, 0.5 + 1.5 * c.wear);
        }
        y += fh;
    }
    let mall_top = y;
    // Big screens across the upper levels, one on a corner.
    if levels >= 3 {
        let sw = (front.len * 0.32).min(10.0);
        let st = if c.seed % 2 == 0 { front.len * 0.28 } else { -front.len * 0.28 };
        let sh = ((levels - 1) as f32 * fh - 1.5).min(8.0);
        let sf = Face::new(front.at(-front.len * 0.5, reach + 0.05), front.at(front.len * 0.5, reach + 0.05), front.out);
        led_screen(g, c, rng, &sf, st, fh + 0.8 + sh * 0.5, sw, sh);
    }
    let side = if c.seed % 2 == 0 { pod.right() } else { pod.left() };
    if levels >= 3 && side.len > 6.0 {
        led_screen(g, c, rng, &side, -side.len * 0.3, fh + (mall_top - fh) * 0.5, (side.len * 0.4).min(7.0), (mall_top - fh - 2.0).clamp(2.0, 9.0));
    }
    // The entrance atrium, lit all night, its canopy.
    let tint = glass(c, rng);
    super::glasscampus::atrium(g, c, rng, &front, at, aw, 1.0f32.min((c.reach_out(&front) - 0.4).max(0.0)), 0.0, (fh * 2.0).min(mall_top) - 0.35, tint, fh, LOUNGE);
    // The roof: billboards, plant, rails.
    let roof = R::new(pod.u0 + 0.6, pod.u1 - 0.6, pod.v0 + 0.6, pod.v1 - 0.4 * (levels - 1) as f32 - 0.6);
    if floors > levels {
        // The rest of its floors as a tower over the mall.
        let tr = tower_rect(c, &pod, 0.5);
        let tr = R::new(tr.u0, tr.u1, tr.v0, tr.v1.min(roof.v1));
        if tr.d() > 5.0 {
            slim_tower(g, c, rng, &tr, mall_top, floors - levels, c.fh().clamp(3.1, 3.4), true);
        }
    } else if roof.w() > 6.0 && roof.d() > 4.0 {
        let p = Vec2::new(roof.c().x, roof.v0 + 1.0);
        billboard(g, c, rng, p, 0.0, mall_top, (roof.w() * 0.6).min(12.0), 3.5);
        roof_plant(g, c, rng, &R::new(roof.u0, roof.u1, roof.v0 + 2.2, roof.v1), mall_top);
    }
    let kerb = (c.reach_out(&front) - 0.2).min(2.4);
    if kerb > 1.6 {
        let n = rng.random_range(3..8);
        mopeds(g, c, rng, front.at(-front.len * 0.4, kerb * 0.6), front.tangent, n);
    }
    lamp(g, c, Vec2::new(pod.u1 - 0.6, pod.v0 - 0.8), 4.5);
}

/// A luxury street-front boutique row under neon: units of black glass,
/// pale stone or bronze panel, each with a double-height lit window in a
/// brass frame, its brand's sign, a neon line under the cornice, a canopy;
/// flats or offices over them; the hectic street between: vending
/// machines, mopeds, a sign hung out. A row of more than five floors puts
/// the rest in a slim tower behind.
pub fn boutique_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let f = &c.frame;
    let area = R::of(&c.poly);
    let floors = (c.floors as i32).clamp(1, 80);
    let low = floors.min(4);
    let fh = c.fh().clamp(3.2, 3.6);
    let depth = (area.d() - 2.0).clamp(6.0, 14.0);
    let Some(row) = fit(c, R::new(area.u0 + 0.3, area.u1 - 0.3, area.v0 + 1.2, area.v0 + 1.2 + depth), 5.0) else { return kiosk(g, c, rng) };
    let n = ((row.w() / 7.0).floor() as i32).clamp(1, 7);
    let uw = row.w() / n as f32;
    let neon = pick(rng, &c.palette.sign_colors);
    for i in 0..n {
        let u0 = row.u0 + i as f32 * uw;
        let ur = R::new(u0 + 0.05, u0 + uw - 0.05, row.v0, row.v1);
        let fl = (low - (i % 2)).max(1);
        let h = fl as f32 * fh + 0.6;
        let (surf, col) = match rng.random_range(0..3) {
            0 => (Glass, mul(TINTS[0], 1.2)),
            1 => (Stone, mix([0.78, 0.75, 0.70], DIRT, 0.1 + 0.4 * c.wear)),
            _ => (Panel, mix(BRASS, [0.2, 0.18, 0.15], 0.5 + 0.2 * c.wear)),
        };
        block(g, c, surf, &ur, 0.0, h, col);
        let fr = ur.front();
        // The window, double height where the unit has the floors.
        let wh = if fl >= 2 { fh * 2.0 - 1.0 } else { fh - 0.9 };
        let ww = (fr.len - 1.4).max(1.0);
        fr.glaze(g, c, 0.0, 0.2 + wh * 0.5, 0.05, ww, wh, Pane::Dark);
        let lit = mul(pick(rng, &c.style.window_colors), rng.random_range(0.6..0.9));
        fr.glaze(g, c, 0.0, 0.2 + wh * 0.5, 0.07, ww - 0.3, wh - 0.3, Pane::Lit(lit));
        g.lod(Lod::Mid, |g| fr.window_frame(g, c, 0.0, 0.2 + wh * 0.5, ww, wh, 0.25, mul(BRASS, 1.0 - 0.3 * c.wear)));
        // Its sign over the window, its canopy, a neon line under the cornice.
        let sy = 0.2 + wh + 0.55;
        if sy + 0.5 < h {
            brand_sign(g, c, rng, &fr, 0.0, sy, (fr.len * 0.6).min(4.5), 0.6);
        }
        let reach = (c.reach_out(&fr) - 0.2).min(1.4);
        if reach > 0.5 {
            fr.bx(g, f, Panel, 0.0, 0.2 + wh + 0.12, reach * 0.5, Vec3::new(ww + 0.4, 0.22, reach), mul(SPANDRELS[2], 1.5));
            let p = fr.at(0.0, reach * 0.5);
            night_box(g, c, p.x, 0.2 + wh - 0.01, p.y, Vec3::new(ww * 0.6, 0.04, reach * 0.4), SOFFIT);
        }
        fr.decal(g, f, Glow, 0.0, h - 0.35, 0.06, fr.len - 0.3, 0.08, mul(neon, 1.0 - 0.4 * c.wear));
        // Flats or offices over the shop.
        let up = if fl >= 2 { 2 } else { 1 };
        if fl > up {
            pane_row(g, c, rng, &fr, up as f32 * fh, fl - up, fh, 2.4, 1.3, fh * 0.5, true);
        }
        cornice(g, c, &ur.poly(), h - 0.1, 0.22, 0.14, Concrete, mul(SPANDRELS[0], 1.2));
        g.lod(Lod::Mid, |g| roof_gear(g, c, rng, &ur.inset(0.6).poly(), h, 0.6));
        grime(g, c, rng, &fr, 0.0, 2.0, 0.6 + 1.6 * c.wear);
        fr.decal(g, f, Matte, 0.0, 0.25, 0.012, fr.len - 0.05, 0.5, mix(col, DIRT, 0.7));
    }
    // The street between: vending machines, a hanging sign, mopeds, bins.
    let front = row.front();
    if n >= 2 {
        vending(g, c, rng, &front, -front.len * 0.5 + uw - 0.6);
    }
    hanging_sign(g, c, rng, &front, front.len * 0.5 - 1.0, fh * 1.6, 1.6, 2.0);
    let kerb = (c.reach_out(&front) - 0.2).min(2.4);
    if kerb > 1.6 {
        let k = rng.random_range(2..6);
        mopeds(g, c, rng, front.at(front.len * 0.15, kerb * 0.65), front.tangent, k);
    }
    // The rest of the floors as a slim tower behind the row.
    if floors > 5 {
        let back = R::new(row.u0 + 0.6, row.u1 - 0.6, row.v1 + 0.6, area.v1 - 0.6);
        if let Some(back) = fit(c, back, 6.0) {
            let tw = (back.w() * 0.6).clamp(6.0, 18.0).min(back.w());
            let tr = R::new(back.c().x - tw * 0.5, back.c().x + tw * 0.5, back.v0, back.v0 + back.d().min(16.0));
            slim_tower(g, c, rng, &tr, 0.0, floors, c.fh().clamp(3.1, 3.4), true);
        } else {
            // No room behind: the row itself takes the floors over it.
            let tr = R::new(row.u0 + 1.0, row.u1 - 1.0, row.v0 + 1.6, row.v1 - 0.4);
            if tr.w() > 4.0 && tr.d() > 3.0 {
                slim_tower(g, c, rng, &tr, low as f32 * fh + 0.6, floors - low, c.fh().clamp(3.1, 3.4), true);
            }
        }
    }
}

// ── the big house ─────────────────────────────────────────────────────────────

/// A sky villa: a low block of dark glass and pale stone on a planted
/// plinth, crowned by a glass penthouse lit warm, its roof a terrace garden
/// round an infinity pool on the street side; a gate in a stone wall, a
/// car on the drive, lamps; on a billionaire's four plots a floor more, a
/// second pavilion and a helipad on the penthouse roof.
pub fn sky_villa(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(site) = fit(c, R::of(&offset_poly(&c.lot_poly, 0.3)), 12.0).map(|r| r.inset(1.0)) else { return luxe_tower(g, c, rng, e) };
    let big = site.w() * site.d() > 1300.0;
    let fh = 3.5;
    ground(g, f, Foliage, &offset_poly(&c.lot_poly, 0.4), 0.03, mul(pal.foliage, 0.75 - 0.25 * c.wear));
    let bw = (site.w() * 0.62).min(if big { 32.0 } else { 22.0 });
    let bd = (site.d() * 0.42).clamp(8.0, if big { 18.0 } else { 13.0 });
    let bv0 = site.v0 + (site.d() * 0.3).clamp(5.0, 12.0);
    let base = R::new(site.c().x - bw * 0.5, site.c().x + bw * 0.5, bv0, bv0 + bd);
    if base.v1 > site.v1 - 1.0 {
        return luxe_tower(g, c, rng, e);
    }
    // The block: the sim's floors (two for a family, a floor more on an estate).
    let floors = (c.floors as i32).clamp(1, 4) + if big { 1 } else { 0 };
    let stone = mix([0.80, 0.77, 0.72], DIRT, 0.08 + 0.35 * c.wear);
    block(g, c, Stone, &base, 0.0, 0.6, mul(stone, 0.9));
    let tint = glass(c, rng);
    let top = curtain_block(g, c, rng, &base, 0.6, floors, fh, tint, stone);
    // Stone fins up the street face (mid), a deep stone frame round it.
    let front = base.front();
    for face in [base.left(), base.right()] {
        face.bx(g, f, Stone, 0.0, (0.6 + top - 0.25) * 0.5, 0.25, Vec3::new(face.len + 0.5, top - 0.85, 0.5), stone);
    }
    g.lod(Lod::Mid, |g| {
        let n = ((front.len / 2.4).floor() as i32).max(1);
        for i in 1..n {
            let t = -front.len * 0.5 + i as f32 * front.len / n as f32;
            front.bx(g, f, Stone, t, (0.6 + top - 0.25) * 0.5, 0.35, Vec3::new(0.18, top - 0.85, 0.7), mul(stone, 0.95));
        }
    });
    // The penthouse set back on the roof, lit warm, its roof floating.
    let pw = (base.w() * 0.55).max(6.0);
    let pd = (base.d() * 0.55).max(5.0);
    let pu = if c.seed % 2 == 0 { base.u1 - 1.0 - pw * 0.5 } else { base.u0 + 1.0 + pw * 0.5 };
    let pent = R::new(pu - pw * 0.5, pu + pw * 0.5, base.v1 - 0.8 - pd, base.v1 - 0.8);
    let py = top;
    block(g, c, Glass, &pent, py, py + fh, mul(tint, 1.3));
    for face in pent.faces() {
        if face.len > 2.0 && coin(rng, 0.5 + 0.5 * c.lit_chance()) {
            face.glaze(g, c, 0.0, py + fh * 0.45, PANE, face.len - 1.0, fh - 0.8, Pane::Lit(mul(pal.window_warm, rng.random_range(0.6..0.9))));
        }
    }
    g.lod(Lod::Mid, |g| {
        for face in pent.faces() {
            let n = ((face.len / 2.0).floor() as i32).max(1);
            for i in 1..n {
                face.decal(g, f, Matte, -face.len * 0.5 + i as f32 * face.len / n as f32, py + fh * 0.5, 0.065, 0.07, fh, DARK);
            }
        }
    });
    let proof = pent.inset(-1.2);
    let proof = R::new(proof.u0.max(base.u0), proof.u1.min(base.u1), proof.v0, proof.v1.min(base.v1));
    block(g, c, Concrete, &proof, py + fh, py + fh + 0.35, mul(stone, 1.02));
    night_box(g, c, proof.c().x, py + fh - 0.03, proof.v0 + 0.5, Vec3::new(proof.w() - 1.0, 0.04, 0.12), LOUNGE);
    for (u, v) in [(proof.u0 + 0.3, proof.v0 + 0.3), (proof.u1 - 0.3, proof.v0 + 0.3)] {
        f.bx(g, Matte, u, py + fh * 0.5, v, Vec3::new(0.14, fh, 0.14), DARK);
    }
    // The roof terrace: an infinity pool along the street side, the garden.
    let terrace = R::new(base.u0 + 0.4, base.u1 - 0.4, base.v0 + 0.4, pent.v0 - 1.4);
    if terrace.d() > 2.6 {
        let pool = R::new(terrace.u0 + 0.3, terrace.u0 + 0.3 + (terrace.w() * 0.6).min(16.0), terrace.v0 + 0.3, terrace.v0 + 0.3 + (terrace.d() * 0.5).clamp(1.8, 4.0));
        block(g, c, Concrete, &pool.inset(-0.3), py - 0.08, py + 0.55, mul(stone, 0.8));
        block(g, c, Glass, &pool, py + 0.3, py + 0.6, mul(WATER, 1.0 - 0.3 * c.wear));
        let from = g.part(Glow).positions.len();
        block(g, c, Glow, &pool.inset(-0.03), py + 0.28, py + 0.63, mul(POOL_LIGHT, 0.75 - 0.3 * c.wear));
        mark_glow(g, from, NIGHT_MARK);
        let bed = R::new(pool.u1 + 0.8, terrace.u1 - 0.2, terrace.v0 + 0.2, terrace.v1 - 0.2);
        if bed.w() > 1.5 {
            ground(g, f, Foliage, &bed.poly(), py + 0.12, mul(pal.foliage, 0.85 - 0.25 * c.wear));
            g.lod(Lod::Mid, |g| {
                let k = ((bed.w() / 3.0).floor() as i32).clamp(1, 4);
                for i in 0..k {
                    let p = Vec2::new(bed.u0 + (i as f32 + 0.5) * bed.w() / k as f32, bed.c().y);
                    tree_kind(g, f.to_world(p.x, p.y), py + 0.1, TreeKind::Round, 3.0, 1.2, pal.bark, mul(pal.foliage, 1.0 - 0.2 * c.wear));
                }
            });
        }
    }
    for face in base.inset(0.05).faces() {
        face.bx(g, f, Glass, 0.0, py + 0.6, -0.05, Vec3::new(face.len, 1.0, 0.06), mul(TINTS[2], 1.6));
    }
    rails(g, c, &base.inset(0.12), py + 0.05, mul(BRASS, 0.85));
    // The estate's second pavilion and its helipad on the penthouse roof.
    if big {
        let other = if pu > base.c().x { R::new(base.u0 + 1.0, pent.u0 - 1.5, pent.v0 + 1.0, pent.v1) } else { R::new(pent.u1 + 1.5, base.u1 - 1.0, pent.v0 + 1.0, pent.v1) };
        if other.w() > 3.0 && other.d() > 3.0 {
            block(g, c, Glass, &other, py, py + fh - 0.4, mul(tint, 1.2));
            if coin(rng, c.lit_chance()) {
                let of = other.front();
                of.glaze(g, c, 0.0, py + fh * 0.45, PANE, of.len - 0.8, fh - 1.2, Pane::Lit(mul(pal.window_warm, 0.7)));
            }
            block(g, c, Concrete, &other.inset(-0.4), py + fh - 0.4, py + fh - 0.1, mul(stone, 1.02));
        }
        helideck(g, c, &proof.inset(0.3), py + fh + 0.35);
    }
    // The stone wall to the street, the gate and the drive, a car, lamps.
    let wall_v = site.v0 + 0.25;
    let gw = 4.4;
    let gu = if pu > base.c().x { site.u0 + gw * 0.5 + 2.0 } else { site.u1 - gw * 0.5 - 2.0 };
    for (u0, u1) in [(site.u0, gu - gw * 0.5), (gu + gw * 0.5, site.u1)] {
        if u1 - u0 > 0.8 {
            f.bx(g, Stone, (u0 + u1) * 0.5, 1.1, wall_v, Vec3::new(u1 - u0, 2.2, 0.45), mul(stone, 0.85));
        }
    }
    for s in [-1.0f32, 1.0] {
        let p = Vec2::new(gu + s * (gw * 0.5 + 0.25), wall_v);
        f.bx(g, Stone, p.x, 1.4, p.y, Vec3::new(0.6, 2.8, 0.6), mul(stone, 0.75));
        night_box(g, c, p.x, 2.95, p.y, Vec3::new(0.35, 0.3, 0.35), LOUNGE);
    }
    g.lod(Lod::Mid, |g| {
        let n = ((gw / 0.2).floor() as i32).max(2);
        for i in 0..n {
            let u = gu - gw * 0.5 + (i as f32 + 0.5) * gw / n as f32;
            f.bx(g, Matte, u, 1.0, wall_v, Vec3::new(0.05, 2.0, 0.05), mul(BRASS, 0.6));
        }
    });
    let drive = R::new(gu - gw * 0.5 + 0.4, gu + gw * 0.5 - 0.4, wall_v + 0.3, base.v0 - 0.4);
    if drive.d() > 3.0 {
        ground(g, f, Paving, &drive.poly(), 0.05, mul(pal.paving, 0.8));
        let cp = Vec2::new(drive.c().x, drive.v1 - 2.8);
        if drive.d() > 7.0 && vehicle_fits(c, cp, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
            vehicle(g, f, pal, rng, cp, 0.05, std::f32::consts::FRAC_PI_2, Vehicle::Car, [[0.05, 0.05, 0.06], [0.70, 0.08, 0.08], [0.82, 0.80, 0.74]][(c.seed % 3) as usize]);
        }
        for k in 0..3 {
            lamp(g, c, Vec2::new(drive.u0 - 0.7, drive.v0 + 1.5 + k as f32 * (drive.d() - 2.5) / 2.0), 1.0);
        }
    }
    // Trees in the garden behind and beside.
    for i in 0..if big { 6 } else { 3 } {
        let p = Vec2::new(site.u0 + 2.0 + (i as f32 * 0.41).fract() * (site.w() - 4.0), base.v1 + 2.5 + (i % 3) as f32 * ((site.v1 - base.v1 - 3.5).max(0.0) / 3.0));
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.5 && p.y < site.v1 - 1.0 {
            tree(g, f, p, rng.random_range(5.0..8.0), 2.2, pal.bark, mul(pal.foliage, 1.0 - 0.2 * c.wear));
        }
    }
}

/// [r4-skyline] The style a trade builds in a Skyline Luxe district where
/// the look has one for it: hotels are towers with a helipad.
pub fn trade_style(kind: Option<nd_sim::BusinessKind>) -> Option<super::Style> {
    match kind? {
        nd_sim::BusinessKind::Hotel => Some(super::Style::HelipadTower),
        _ => None,
    }
}
