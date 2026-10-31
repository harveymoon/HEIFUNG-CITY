//! [malls] Strip malls, malls and mega-malls (`nd_sim::estate::Form`): the
//! shops in them are the building's units (`nd_sim::buildings::Unit`), one
//! sign each, lit in its tenant's neon when let and a pale board when to
//! let, so a mall is drawn as full as it is. Stylised and gritty: stucco
//! and concrete, cheap fascias, sodium lamps over the asphalt, cars in the
//! bays. Everything comes from the building's seed, so it looks the same
//! from every camera and after every reload.
//!
//! The lot's street side is `v0`: the car park lies in front, the shops
//! face it.

use super::kit::{fit, R};
use super::parts::*;
use super::{Ctx, Extras, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::buildings::{Unit, UnitRole};
use nd_sim::rng_for;
use rand::Rng;

const TO_LET: [f32; 3] = [0.62, 0.60, 0.55];
const STUCCO: [[f32; 3]; 4] = [[0.72, 0.64, 0.52], [0.66, 0.60, 0.55], [0.58, 0.55, 0.50], [0.70, 0.58, 0.48]];
const BAY_W: f32 = 2.6;
const BAY_D: f32 = 5.0;
/// Bay lines' paint.
const BAY_PAINT: [f32; 3] = [0.82, 0.8, 0.72];

/// A shop unit as drawn: let or not, its role, its floor, and whether the
/// building's own business has it.
struct ShopUnit {
    lit: bool,
    role: UnitRole,
    lo: u8,
    main: bool,
    /// [r4-leisure-places] The word over a place to eat or go out.
    word: Option<&'static [u8]>,
}

/// The shop units, or a stand-in row of `n` (a style preview with no sim
/// building behind it: half of them let).
fn shops_or(units: &[Unit], n: usize) -> Vec<ShopUnit> {
    let v: Vec<ShopUnit> = units.iter().filter(|u| u.shopfront).map(|u| ShopUnit { lit: !u.vacant(), role: u.role, lo: u.lo, main: u.occupant == nd_sim::buildings::Occupant::Main, word: u.sign.and_then(nd_sim::leisure::sign_word) }).collect();
    if !v.is_empty() {
        return v;
    }
    (0..n).map(|i| ShopUnit { lit: i % 2 == 0, role: UnitRole::Shop, lo: (i % 2) as u8, main: false, word: None }).collect()
}

/// A shop's sign colour: its unit's place in the building and the seed.
fn sign_colour(c: &Ctx, i: usize) -> [f32; 3] {
    let signs = &c.palette.sign_colors;
    let k = (nd_sim::hash::mix2(c.seed, i as u64 + 1) % signs.len().max(1) as u64) as usize;
    signs.get(k).copied().unwrap_or([1.0, 0.3, 0.8])
}

/// A sign over a shop door on `face`, `t` along it at height `y`, `w` wide:
/// a dark board with its tenant's neon, or a pale board when to let.
#[allow(clippy::too_many_arguments)]
fn shop_sign(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, let_: bool, col: [f32; 3]) {
    let f = &c.frame;
    if !let_ {
        face.bx(g, f, Matte, t, y, 0.06, Vec3::new(w * 0.6, 0.6, 0.12), TO_LET);
        return;
    }
    face.bx(g, f, Matte, t, y, 0.08, Vec3::new(w, 0.8, 0.16), DARK);
    face.decal(g, f, Glow, t, y, 0.18, w * 0.9, 0.42, mul(col, 2.2 * (1.0 - 0.4 * c.wear)));
}

/// Painted bays in a lot-local rectangle (rows across `r`, bays facing the
/// building at `v1`), cars in a share of them, and the bays' count.
fn bays_in(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, share: f32, rows: i32) -> usize {
    let f = &c.frame;
    let pal = c.palette;
    let n = (r.w() / BAY_W).floor() as i32;
    if n < 1 || r.d() < BAY_D {
        return 0;
    }
    let mut count = 0;
    for row in 0..rows {
        let v = r.v1 - BAY_D * 0.5 - row as f32 * (BAY_D * 2.0 + 6.0);
        if v - BAY_D * 0.5 < r.v0 - 0.01 {
            break;
        }
        for k in 0..n {
            let u = r.u0 + (k as f32 + 0.5) * r.w() / n as f32;
            let p = Vec2::new(u, v);
            let corners = [p + Vec2::new(-1.3, -2.5), p + Vec2::new(1.3, -2.5), p + Vec2::new(1.3, 2.5), p + Vec2::new(-1.3, 2.5)];
            if !c.all_inside(&corners) {
                continue;
            }
            count += 1;
            // [p2-look] Painted lines (they were a faint glow, lost by day).
            for s in [-1.0f32, 1.0] {
                f.flat(g, Matte, u + s * BAY_W * 0.5, 0.06, v, 0.12, BAY_D * 0.9, 0.0, mul(BAY_PAINT, 1.0 - 0.35 * c.wear));
            }
            if coin(rng, share) && vehicle_fits(c, p, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
                let col = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, p, 0.0, std::f32::consts::FRAC_PI_2, Vehicle::Car, col);
            }
        }
    }
    count
}

/// [p2-look] A shop's sign on the fascia along a covered walk's edge,
/// `walk` out from the face: lit box letters in its tenant's neon on a
/// dark board, or a pale board when to let.
#[allow(clippy::too_many_arguments)]
fn fascia_sign(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, walk: f32, w: f32, let_: bool, col: [f32; 3], word: Option<&[u8]>) {
    let f = &c.frame;
    let d = walk + 0.2;
    if !let_ {
        face.bx(g, f, Matte, t, y, d, Vec3::new(w * 0.55, 0.7, 0.1), TO_LET);
        return;
    }
    face.bx(g, f, Matte, t, y, d, Vec3::new(w, 0.95, 0.12), DARK);
    if let Some(word) = word {
        let h = ((w * 0.85) / ((4 * word.len()) as f32 - 1.0) * 5.0).min(0.6);
        super::eatout::neon_word(g, c, face, t, y, d + 0.07, h, word, mul(col, 1.8));
        return;
    }
    let letters = ((w / 0.75).floor() as i32).clamp(3, 9);
    let lw = w * 0.86 / letters as f32;
    for k in 0..letters {
        let lt = t - w * 0.43 + (k as f32 + 0.5) * lw;
        face.decal(g, f, Glow, lt, y, d + 0.07, lw * 0.72, 0.55, mul(col, 2.2 * (1.0 - 0.4 * c.wear)));
    }
}

/// Sodium lamps over a car park rectangle, every `step` m along its middle.
fn lot_lamps(g: &mut Geometry, c: &Ctx, r: &R, step: f32) {
    let n = (r.w() / step).floor().max(1.0) as i32;
    for k in 0..n {
        let u = r.u0 + (k as f32 + 0.5) * r.w() / n as f32;
        lamp_post_in(g, c, Vec2::new(u, r.c().y), 7.5, c.palette.street_lamp);
    }
}

/// A pylon sign by the street: a pole with a lit name box over a stack of
/// tenant panels, the strip mall's one tall thing.
fn pylon(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32, panels: &[(bool, [f32; 3])]) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.2 {
        return;
    }
    f.bx(g, Metal, p.x, h * 0.5, p.y, Vec3::new(0.35, h, 0.35), mul(c.palette.steel, 0.7));
    let top = h + 1.0;
    f.bx(g, Matte, p.x, top, p.y, Vec3::new(3.2, 2.0, 0.5), DARK);
    f.bx(g, Glow, p.x, top, p.y, Vec3::new(2.9, 1.6, 0.56), mul(c.palette.sign_colors.first().copied().unwrap_or([1.0, 0.4, 0.2]), 1.6));
    for (k, (lit, col)) in panels.iter().take(6).enumerate() {
        let y = h - 0.6 - k as f32 * 0.75;
        if y < 2.0 {
            break;
        }
        let colour = if *lit { mul(*col, 1.4) } else { TO_LET };
        let s = if *lit { Glow } else { Matte };
        f.bx(g, s, p.x, y, p.y, Vec3::new(2.4, 0.55, 0.44), colour);
    }
}

/// [malls] An LA-style strip mall: a car park in front for ten to twenty
/// cars, a two- or three-storey stucco block behind with a row of
/// shopfronts under a covered walk, a sign over each door, an open gallery
/// to the offices upstairs, and a pylon sign by the street.
pub fn strip_mall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = R::of(&c.poly);
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.15 + 0.4 * c.wear));
    let floors = (c.floors as i32).clamp(2, 3);
    let fh = c.fh().max(3.2);
    let ground_h = fh + 0.6;
    let height = ground_h + (floors - 1) as f32 * fh;
    // [p2-look] A shallow block (8 to 14 m) at the back, the car park in
    // front taking the rest: one row of bays and an aisle, deeper lots two
    // (a deep plot used to get an 18 m deep box behind one row of bays).
    let block_d = (lot.d() * 0.42).clamp(8.0, 14.0);
    let park_d = (lot.d() - block_d - 1.5).max(BAY_D + 6.5);
    let block = R::new(lot.u0 + 1.0, lot.u1 - 1.0, lot.v0 + park_d, lot.v0 + park_d + block_d);
    let poly = fit_inside(c, &block.poly());
    if poly.len() < 3 {
        return;
    }
    let br = R::of(&poly);
    let stucco = mul(pick(rng, &STUCCO), 1.0 - 0.3 * c.wear);
    extrude(g, f, Concrete, &poly, 0.0, height, stucco);
    parapet(g, c, &poly, height, 0.9, 0.25, Concrete, mul(stucco, 0.9));
    roof_gear(g, c, rng, &poly, height, 0.8);
    let front = br.front();
    let units = shops_or(e.units, ((front.len / 7.0).floor() as usize).clamp(3, 8));
    let n = units.len().max(1);
    let seg = front.len / n as f32;
    // The covered walk along the shops, on posts at the car park's edge.
    let walk = 2.4f32.min(c.reach_out(&front) - 0.4);
    // Shopfronts: glazing and a door each, pilasters between them, a sign
    // over each.
    for (i, u) in units.iter().enumerate() {
        let let_ = &u.lit;
        let t = -front.len * 0.5 + seg * (i as f32 + 0.5);
        if *let_ {
            front.glaze(g, c, t, 1.6, 0.04, seg * 0.78, 2.6, Pane::Dark);
            if coin(rng, 0.45 + 0.5 * c.lit_chance()) {
                front.glaze(g, c, t, 1.6, 0.06, seg * 0.72, 2.3, Pane::Lit(mul(pick(rng, &pal.glow_colors), 0.55)));
            }
        } else {
            // To let: papered windows, a roller shutter half down.
            front.decal(g, f, Matte, t, 1.6, 0.04, seg * 0.78, 2.6, mul(TO_LET, 0.7));
            front.decal(g, f, Metal, t, 2.5, 0.07, seg * 0.78, 0.9, mul(pick(rng, &pal.metal_paint), 0.7));
        }
        front.decal(g, f, Matte, t + seg * 0.28, 1.15, 0.09, 1.0, 2.2, DARK);
        if i > 0 {
            front.bx(g, f, Concrete, t - seg * 0.5, ground_h * 0.5, 0.15, Vec3::new(0.5, ground_h, 0.3), mul(stucco, 0.85));
        }
        // [p2-look] Each shop's sign on the fascia over the walk's edge,
        // where it is seen from the car park and the street (under the
        // walk's roof it was hidden from every camera above it); a small
        // board over the door under the walk.
        if walk > 1.0 {
            fascia_sign(g, c, &front, t, ground_h - 0.25, walk, (seg * 0.82).min(7.0), *let_, sign_colour(c, i), u.word);
            front.bx(g, f, Matte, t + seg * 0.28, 2.75, 0.12, Vec3::new(1.2, 0.35, 0.08), if *let_ { mul(sign_colour(c, i), 0.6) } else { TO_LET });
        } else {
            shop_sign(g, c, &front, t, ground_h - 0.55, (seg * 0.8).min(7.0), *let_, sign_colour(c, i));
        }
    }
    // The top gallery's floor: the posts run up to it.
    let gallery_top = ground_h + (floors - 2).max(0) as f32 * fh;
    if walk > 1.0 {
        // The fascia board the signs hang on, along the walk's edge.
        front.bx(g, f, Concrete, 0.0, ground_h - 0.25, walk + 0.05, Vec3::new(front.len, 1.1, 0.22), mul(stucco, 0.92));
        for k in 1..floors {
            let y = ground_h + (k - 1) as f32 * fh;
            front.bx(g, f, Panel, 0.0, y + 0.1, walk * 0.5, Vec3::new(front.len, 0.3, walk), mul(stucco, 0.8));
        }
        front.bx(g, f, Paving, 0.0, 0.08, walk * 0.5, Vec3::new(front.len, 0.16, walk), mul(pal.paving, 1.0));
        let posts = (front.len / 6.0).floor().max(1.0) as i32;
        for k in 0..=posts {
            let t = -front.len * 0.5 + 0.3 + k as f32 * (front.len - 0.6) / posts as f32;
            if c.inside_local(front.at(t, walk - 0.1)) && distance_to_boundary(front.at(t, walk - 0.2), &c.lot_poly) > 0.25 {
                front.bx(g, f, Metal, t, gallery_top * 0.5, walk - 0.2, Vec3::new(0.2, gallery_top, 0.2), mul(pal.steel, 0.6));
            }
        }
        front.decal(g, f, Glow, 0.0, ground_h - 0.05, walk - 0.05, front.len * 0.9, 0.06, mul(pal.lamp, 0.6));
    }
    // Upstairs: band windows and an open gallery with a rail, a stair at
    // one end.
    for k in 1..floors {
        let y0 = ground_h + (k - 1) as f32 * fh;
        band_windows(g, c, rng, &front, y0, 1, fh);
        if walk > 1.0 {
            front.bx(g, f, Concrete, 0.0, y0 + 1.0, walk - 0.25, Vec3::new(front.len, 1.0, 0.12), mul(stucco, 0.75));
        }
    }
    if walk > 1.0 && floors > 1 && front.len > 14.0 && [0.0f32, 5.0].iter().all(|dt| distance_to_boundary(front.at(front.len * 0.5 - 1.2 - dt, walk * 0.5), &c.lot_poly) > 0.8 && c.inside_local(front.at(front.len * 0.5 - 1.2 - dt, walk * 0.5))) {
        let t = front.len * 0.5 - 1.2;
        let a = f.at2(front.at(t, walk * 0.5), 0.2);
        let b = f.at2(front.at(t - 5.0, walk * 0.5), ground_h);
        g.part(Metal).slab(a, b, 1.1, 0.2, rgba(mul(pal.steel, 0.55)));
    }
    for face in c.faces(&poly).iter().filter(|x| !x.is_front && x.len > 6.0) {
        grime(g, c, rng, face, 0.0, height, 1.5);
    }
    // The car park: bays, cars, lamps, a pylon sign by the street.
    let park = R::new(lot.u0 + 1.2, lot.u1 - 1.2, lot.v0 + 1.0, br.v0 - walk.max(0.5) - 0.3);
    let mut prng = rng_for(c.seed, 0x5A11);
    bays_in(g, c, &mut prng, &park, 0.55 + 0.3 * (1.0 - c.wear), if park.d() > 2.0 * BAY_D + 6.0 { 2 } else { 1 });
    lot_lamps(g, c, &R::new(park.u0, park.u1, park.v0, park.v0 + 1.0), 16.0);
    let panels: Vec<(bool, [f32; 3])> = units.iter().enumerate().map(|(i, u)| (u.lit, sign_colour(c, i))).collect();
    pylon(g, c, Vec2::new(lot.u1 - 2.0, lot.v0 + 1.6), 8.5, &panels);
    if c.wear > 0.4 {
        weeds(g, c, rng, &c.lot_poly, 5);
    }
}

// ── malls and mega-malls ─────────────────────────────────────────────────────

/// [p2-look] Plant on a big roof: a row of packaged air handlers with fan
/// grilles, duct runs between them, an exhaust stack, and a few of the
/// usual vents and tanks.
fn roof_plant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let r = R::of(&offset_poly(poly, 2.5));
    if r.w() < 6.0 || r.d() < 5.0 {
        roof_gear(g, c, rng, poly, y, 0.8);
        return;
    }
    let steel = mul(pal.steel, 0.85 - 0.2 * c.wear);
    let n = ((r.w() / 7.0).floor() as i32).clamp(1, 6);
    let v = r.v0 + r.d() * 0.35;
    for k in 0..n {
        let u = r.u0 + (k as f32 + 0.5) * r.w() / n as f32;
        f.bx(g, Metal, u, y + 0.9, v, Vec3::new(3.0, 1.8, 2.0), steel);
        for s in [-0.7f32, 0.7] {
            f.flat(g, Matte, u + s, y + 1.82, v, 1.1, 1.1, 0.0, DARK);
        }
        // A duct from each unit back into the roof.
        f.bx(g, Metal, u, y + 0.45, v + 2.2, Vec3::new(0.9, 0.9, 2.4), mul(steel, 0.9));
    }
    // A duct run along the row.
    f.bx(g, Metal, r.c().x, y + 0.35, v + 3.6, Vec3::new((r.w() - 2.0).max(1.0), 0.7, 0.7), mul(steel, 0.8));
    let stack = Vec2::new(r.u1 - 1.5, r.v1 - 1.5);
    vent_stack(g, f, stack, y, 0.45, 3.5, mul(steel, 0.7));
    roof_gear(g, c, rng, &R::new(r.u0, r.u1, v + 4.5, r.v1).poly(), y, 0.5);
}

/// [p2-look] An anchor store: its own block, taller than the halls, in its
/// own cladding, a reveal band near the top, a big lit sign high on its
/// front, its own entrance under a deep canopy, a loading dock at the back
/// and plant on the roof.
#[allow(clippy::too_many_arguments)]
fn anchor_store(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: R, h: f32, lit: bool, sign: [f32; 3], k: usize) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = r.poly();
    if !poly_inside(c, &poly) {
        return;
    }
    let (surface, tone) = match (nd_sim::hash::mix2(c.seed, 0xA4C + k as u64) % 3) as usize {
        0 => (Panel, mul(pick(rng, &pal.panel), 0.95 - 0.25 * c.wear)),
        1 => (Brick, mul(pick(rng, &pal.brick), 0.9 - 0.2 * c.wear)),
        _ => (Concrete, mul(pick(rng, &STUCCO), 1.0 - 0.3 * c.wear)),
    };
    extrude(g, f, surface, &poly, 0.0, h, tone);
    parapet(g, c, &poly, h, 1.2, 0.35, surface, mul(tone, 0.85));
    let front = r.front();
    // A dark reveal under the parapet round the front and sides.
    for face in [r.front(), r.left(), r.right()] {
        face.decal(g, f, Matte, 0.0, h - 1.4, 0.03, face.len * 0.98, 0.5, mul(tone, 0.55));
    }
    // The sign: a dark box with its name in the tenant's neon (a pale
    // board when the store is to let).
    let sw = (r.w() * 0.7).min(18.0);
    if lit {
        front.bx(g, f, Matte, 0.0, h - 3.2, 0.15, Vec3::new(sw, 2.2, 0.3), DARK);
        let letters = ((sw / 1.6).floor() as i32).clamp(3, 10);
        let lw = sw / letters as f32;
        for i in 0..letters {
            let t = -sw * 0.5 + (i as f32 + 0.5) * lw;
            front.decal(g, f, Glow, t, h - 3.2, 0.31, lw * 0.72, 1.4, mul(sign, 2.0 * (1.0 - 0.4 * c.wear)));
        }
    } else {
        front.bx(g, f, Matte, 0.0, h - 3.2, 0.1, Vec3::new(sw * 0.6, 1.6, 0.16), TO_LET);
    }
    // Its doors under a deep canopy, display windows either side.
    let dw = (r.w() * 0.3).clamp(4.0, 9.0);
    entrance(g, c, rng, &front, 0.0, dw, 3.6);
    for s in [-1.0f32, 1.0] {
        let t = s * (dw * 0.5 + (r.w() * 0.5 - dw * 0.5) * 0.5);
        let w = (r.w() * 0.5 - dw * 0.5 - 2.0).max(0.0);
        if w > 1.5 {
            front.glaze(g, c, t, 1.9, 0.04, w, 2.8, if lit { Pane::Lit(mul(pal.window_warm, 0.5)) } else { Pane::Dark });
        }
    }
    // The loading dock at the back.
    let back = r.back();
    if c.reach_out(&back) > 2.0 {
        for t in [-back.len * 0.25, back.len * 0.1] {
            roller_door(g, c, &back, t, 3.6, 3.8, mul(pick(rng, &pal.metal_paint), 0.75));
        }
        back.bx(g, f, Concrete, 0.0, 0.6, 1.0, Vec3::new((back.len * 0.6).max(2.0), 1.2, 2.0), mul(tone, 0.7));
    }
    for face in [r.left(), r.right(), r.back()] {
        grime(g, c, rng, &face, 0.0, h, 1.5);
    }
    roof_plant(g, c, rng, &poly, h);
}

/// [p2-look] The entrance atrium: a glass box proud of the halls' front,
/// taller than them, with a mullion grid, lit inside, the doors under a
/// canopy and the mall's sign along its top.
fn atrium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: R, h: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = r.poly();
    if !poly_inside(c, &poly) {
        return;
    }
    extrude(g, f, Glass, &poly, 0.0, h, pick(rng, &pal.glass));
    curtain_lights(g, c, rng, &poly, 0.0, 3, h / 3.0);
    let front = r.front();
    // Mullions every 2.5 m and a transom at each level.
    let n = (front.len / 2.5).floor().max(1.0) as i32;
    for k in 0..=n {
        let t = -front.len * 0.5 + k as f32 * front.len / n as f32;
        front.bx(g, f, Metal, t, h * 0.5, 0.06, Vec3::new(0.18, h, 0.18), mul(pal.steel, 0.6));
    }
    for k in 1..3 {
        front.bx(g, f, Metal, 0.0, k as f32 * h / 3.0, 0.06, Vec3::new(front.len, 0.2, 0.2), mul(pal.steel, 0.6));
    }
    extrude(g, f, Metal, &offset_poly(&poly, -0.3), h, h + 0.6, mul(pal.steel, 0.55));
    entrance(g, c, rng, &front, 0.0, (r.w() * 0.5).min(8.0), 3.4);
    sign_board(g, c, rng, &front, 0.0, h - 1.2, r.w() * 0.75, 1.3);
}

/// [p2-look] The food court's glazing: a glass pavilion on the upper level
/// proud of the halls (the counters' signs glowing inside, warm light),
/// on columns over the walk below, its roof edge lit.
fn food_court(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: R, y0: f32, h: f32, counters: &[(bool, [f32; 3])]) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = r.poly();
    if !poly_inside(c, &poly) {
        return;
    }
    let warm = mix(pick(rng, &pal.glass), pal.window_warm, 0.35);
    extrude(g, f, Concrete, &poly, y0 - 0.4, y0, mul(pick(rng, &pal.concrete), 0.9));
    extrude(g, f, Glass, &poly, y0, y0 + h, warm);
    extrude(g, f, Glow, &offset_poly(&poly, 0.15), y0 + h, y0 + h + 0.15, mul(pal.window_warm, 0.6));
    pilotis(g, c, &poly, 0.0, y0 - 0.4, 6.0, 0.45, Concrete, mul(pick(rng, &pal.concrete), 0.85));
    let front = r.front();
    front.glaze(g, c, 0.0, y0 + h * 0.5, 0.04, front.len * 0.95, h * 0.85, Pane::Lit(mul(pal.window_warm, 0.75)));
    // The counters' signs behind the glass, a row along the back wall.
    let n = counters.len().max(1);
    for (i, (lit, col)) in counters.iter().enumerate() {
        let t = -front.len * 0.4 + (i as f32 + 0.5) * front.len * 0.8 / n as f32;
        let colour = if *lit { mul(*col, 1.8) } else { TO_LET };
        front.decal(g, f, if *lit { Glow } else { Matte }, t, y0 + h - 0.8, 0.07, front.len * 0.6 / n as f32, 0.5, colour);
    }
}

/// [p2-look] A parking structure on `r`: open decks on columns, a ramp up
/// the side, stair cores at two corners, strip lights under each deck, a
/// lit P sign and cars on every level.
fn parking_deck(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: R, decks: i32) {
    let f = &c.frame;
    let pal = c.palette;
    let deck = r.poly();
    if !poly_inside(c, &deck) || r.w() < 12.0 || r.d() < 12.0 {
        return;
    }
    let concrete = mul(pick(rng, &pal.concrete), 0.95 - 0.25 * c.wear);
    let dh = 3.0;
    for k in 1..=decks {
        let y = k as f32 * dh;
        extrude(g, f, Concrete, &deck, y - 0.4, y, concrete);
        // Spandrel walls with the open slot over them.
        for face in c.faces(&deck) {
            face.bx(g, f, Concrete, 0.0, y + 0.5, 0.0, Vec3::new(face.len + 0.3, 1.0, 0.25), mul(concrete, 1.08));
            face.decal(g, f, Glow, 0.0, y - 0.5, -1.0, face.len * 0.8, 0.06, mul(pal.lamp, 0.45));
        }
        pilotis(g, c, &offset_poly(&deck, 0.6), y - dh, y - 0.4, 7.5, 0.5, Concrete, mul(concrete, 0.9));
    }
    let dr = r;
    // Stair and lift cores at two corners, a little taller than the top.
    let top = decks as f32 * dh;
    for (u, v) in [(dr.u0 + 1.6, dr.v0 + 1.6), (dr.u1 - 1.6, dr.v1 - 1.6)] {
        f.bx(g, Concrete, u, (top + 2.6) * 0.5, v, Vec3::new(3.2, top + 2.6, 3.2), mul(concrete, 0.8));
        f.bx(g, Glass, u, top + 1.3, v - 1.62, Vec3::new(2.0, 1.8, 0.05), pal.glass_dark);
    }
    // The ramp up the long side, outside the decks where there is room.
    let side = dr.right();
    if c.reach_out(&side) > 4.5 {
        let a = f.at2(side.at(-side.len * 0.45, 2.2), 0.05);
        let b = f.at2(side.at(side.len * 0.3, 2.2), dh);
        g.part(Concrete).slab(a, b, 3.6, 0.35, rgba(mul(concrete, 0.95)));
        side.bx(g, f, Concrete, -side.len * 0.075, dh * 0.5, 4.1, Vec3::new(side.len * 0.75, dh * 0.4, 0.2), mul(concrete, 1.05));
    }
    let df = dr.front();
    df.bx(g, f, Matte, df.len * 0.3, top + 1.6, 0.2, Vec3::new(2.4, 2.4, 0.3), DARK);
    df.decal(g, f, Glow, df.len * 0.3, top + 1.6, 0.36, 1.9, 1.9, pal.parking_sign);
    df.bx(g, f, Metal, df.len * 0.3, top + 0.2, 0.2, Vec3::new(0.2, 0.4, 0.2), mul(pal.steel, 0.6));
    // Cars on every level: the decks below seen through their open sides.
    let mut drng = rng_for(c.seed, 0xDEC);
    let bays = R::new(dr.u0 + 2.0, dr.u1 - 2.0, dr.v0 + 2.0, dr.v1 - 2.0);
    let n = (bays.w() / BAY_W).floor() as i32;
    for level in 0..=decks {
        let y = level as f32 * dh + 0.01;
        for row in 0..((bays.d() / (BAY_D + 3.0)).floor() as i32).max(0) {
            let v = bays.v0 + BAY_D * 0.5 + row as f32 * (BAY_D + 3.0);
            for k in 0..n {
                let p = Vec2::new(bays.u0 + (k as f32 + 0.5) * BAY_W, v);
                if coin(&mut drng, if level == decks { 0.55 } else { 0.7 }) && c.inside_local(p) {
                    let col = pick(&mut drng, &pal.car_colors);
                    vehicle(g, f, pal, &mut drng, p, y, std::f32::consts::FRAC_PI_2, Vehicle::Car, col);
                }
            }
        }
    }
    // Lamps on the open top deck.
    for (u, v) in [(dr.u0 + dr.w() * 0.33, dr.c().y), (dr.u0 + dr.w() * 0.67, dr.c().y)] {
        if c.inside_local(Vec2::new(u, v)) {
            let w = f.to_world(u, v);
            g.part(Matte).cylinder(w, 0.12, 0.08, top, top + 6.0, 8, rgba(CHARCOAL));
            g.part(Glow).boxed(Vec3::new(w.x, top + 5.95, w.y), Vec3::new(0.5, 0.15, 0.5), 0.0, rgba(pal.street_lamp));
            g.lamps.push(Vec3::new(w.x, top + 5.8, w.y));
        }
    }
}

/// The halls: a two-level block of concrete panels with a lit band of
/// tenant signs on each level along the front, pilasters, a skylight
/// ridge down the middle and plant on the roof.
fn halls(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: R, height: f32, levels: i32, signs: &[(usize, bool, u8)]) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = r.poly();
    let panel = mul(pick(rng, &pal.concrete), 1.05 - 0.3 * c.wear);
    extrude(g, f, Concrete, &poly, 0.0, height, panel);
    parapet(g, c, &poly, height, 1.0, 0.3, Concrete, mul(panel, 0.85));
    let front = r.front();
    let lh = height / levels.max(1) as f32;
    // Pilasters every 8 m.
    let np = (front.len / 8.0).floor().max(1.0) as i32;
    for k in 1..np {
        let t = -front.len * 0.5 + k as f32 * front.len / np as f32;
        front.bx(g, f, Concrete, t, height * 0.5, 0.2, Vec3::new(0.7, height, 0.4), mul(panel, 0.9));
    }
    // A dark band at each level, the shops' signs along it, shop windows
    // on the ground floor.
    for lv in 0..levels {
        let y = (lv as f32 + 0.75) * lh;
        front.decal(g, f, Matte, 0.0, y, 0.05, front.len * 0.96, 1.2, DARK);
        let mine: Vec<(usize, bool)> = signs.iter().filter(|s| s.2 as i32 == lv).map(|s| (s.0, s.1)).collect();
        let n = mine.len().max(1);
        let seg = front.len * 0.9 / n as f32;
        for (k, (i, lit)) in mine.iter().enumerate() {
            let t = -front.len * 0.45 + seg * (k as f32 + 0.5);
            shop_sign(g, c, &front, t, y, (seg * 0.75).min(6.0), *lit, sign_colour(c, *i));
            if lv == 0 {
                front.glaze(g, c, t, 1.6, 0.04, seg * 0.6, 2.4, if *lit { Pane::Lit(mul(pick(rng, &pal.glow_colors), 0.45)) } else { Pane::Dark });
            }
        }
    }
    // The skylight ridge down the middle of the roof, lit from inside.
    let sky = R::new(r.u0 + 3.0, r.u1 - 3.0, r.c().y - 2.0, r.c().y + 2.0);
    if sky.w() > 4.0 && r.d() > 10.0 {
        extrude(g, f, Glass, &sky.poly(), height, height + 1.6, mix(pick(rng, &pal.glass), pal.window_warm, 0.2));
        extrude(g, f, Metal, &sky.inset(-0.2).poly(), height + 1.6, height + 1.75, mul(pal.steel, 0.6));
    }
    for face in c.faces(&poly).iter().filter(|x| !x.is_front && x.len > 6.0) {
        grime(g, c, rng, face, 0.0, height, 2.0);
        roller_door(g, c, face, 0.0, 4.0, 3.6, mul(pick(rng, &pal.metal_paint), 0.7));
    }
    let plant = if sky.w() > 4.0 && r.d() > 10.0 { R::new(r.u0, r.u1, sky.v1 + 0.5, r.v1) } else { r };
    if plant.d() > 4.0 {
        roof_plant(g, c, rng, &plant.poly(), height);
    }
}

/// The car park in front of a mall: bays in rows, lamps down the middle,
/// planted islands at the row ends, a trolley bay; its rectangle.
fn front_car_park(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, park: R, salt: u64) {
    let f = &c.frame;
    let pal = c.palette;
    let mut prng = rng_for(c.seed, salt);
    let rows = ((park.d() + 6.0) / (BAY_D * 2.0 + 6.0)).ceil().max(1.0) as i32;
    let inner = R::new(park.u0 + 3.0, park.u1 - 3.0, park.v0, park.v1);
    bays_in(g, c, &mut prng, &inner, 0.5 + 0.3 * (1.0 - c.wear), rows * 2);
    lot_lamps(g, c, &R::new(park.u0, park.u1, park.c().y - 0.5, park.c().y + 0.5), 18.0);
    // Planted islands at both ends of the rows.
    for u in [park.u0 + 1.4, park.u1 - 1.4] {
        let island = R::new(u - 1.2, u + 1.2, park.v0 + 1.0, park.v1 - 1.0);
        if island.d() > 3.0 && poly_inside(c, &island.poly()) {
            f.bx(g, Concrete, island.c().x, 0.12, island.c().y, Vec3::new(island.w(), 0.24, island.d()), mul(pal.paving, 0.9));
            f.flat(g, Matte, island.c().x, 0.25, island.c().y, island.w() - 0.3, island.d() - 0.3, 0.0, mul(pal.foliage, 0.7 - 0.3 * c.wear));
            let trees = (island.d() / 9.0).floor() as i32;
            for k in 0..trees {
                let v = island.v0 + (k as f32 + 0.5) * island.d() / trees as f32;
                let w = f.to_world(island.c().x, v);
                g.part(Wood).cylinder(w, 0.12, 0.1, 0.25, 2.2, 6, rgba(mul(pal.foliage_dry, 0.6)));
                g.part(Matte).cylinder(w, 1.1, 0.4, 2.0, 4.2, 7, rgba(mul(pal.foliage, 0.85 - 0.3 * c.wear)));
            }
        }
    }
    let trolleys = Vec2::new(park.u0 + park.w() * 0.3, park.v1 - 1.2);
    if coin(rng, 0.8) && poly_inside(c, &rect(trolleys.x, trolleys.y, 2.2, 0.8)) {
        f.bx(g, Metal, park.u0 + park.w() * 0.3, 0.55, park.v1 - 1.2, Vec3::new(4.0, 1.1, 1.2), mul(pal.steel, 0.8));
    }
}

/// [malls] [p2-look] A mall: two levels of shops round its own car park.
/// The halls stand at the back of the lot, the owner's store as an anchor
/// block at one end (taller, its own cladding and sign), a glass entrance
/// atrium in the middle of the front, the food court's glazing on the
/// upper level at the other end, plant on the roofs; the car park in
/// front with lamps and planted islands, a pylon by the road.
pub fn mall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let pal = c.palette;
    let lot = R::of(&c.poly);
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.1 + 0.4 * c.wear));
    let height = 2.0 * c.fh().max(3.4) * 1.35;
    let park_d = (lot.d() * 0.38).clamp(12.0, 30.0);
    let side = (lot.w() * 0.06).clamp(1.5, 6.0);
    let Some(block) = fit(c, R::new(lot.u0 + side, lot.u1 - side, lot.v0 + park_d, lot.v1 - 3.0), 8.0) else { return };
    let units = shops_or(e.units, 8);
    // The owner's store is the anchor, at the end the seed picks.
    let main = units.iter().position(|u| u.main || u.role == UnitRole::Anchor);
    let left = nd_sim::hash::mix2(c.seed, 0xA11) % 2 == 0;
    let aw = if block.w() >= 30.0 { (block.w() * 0.3).clamp(10.0, 22.0) } else { 0.0 };
    let (anchor_r, hall_r) = if aw > 0.0 {
        if left {
            (Some(R::new(block.u0, block.u0 + aw, block.v0 - 2.0, block.v1)), R::new(block.u0 + aw, block.u1, block.v0, block.v1))
        } else {
            (Some(R::new(block.u1 - aw, block.u1, block.v0 - 2.0, block.v1)), R::new(block.u0, block.u1 - aw, block.v0, block.v1))
        }
    } else {
        (None, block)
    };
    let signs: Vec<(usize, bool, u8)> = units.iter().enumerate().filter(|(i, _)| anchor_r.is_none() || Some(*i) != main).map(|(i, u)| (i, u.lit, u.lo)).collect();
    halls(g, c, rng, hall_r, height, 2, &signs);
    if let Some(ar) = anchor_r {
        let (lit, col) = main.map_or((true, sign_colour(c, 90)), |i| (units[i].lit, sign_colour(c, i)));
        anchor_store(g, c, rng, ar, height + 3.5, lit, col, 0);
    }
    // The entrance atrium in the middle of the halls' front.
    let w = (hall_r.w() * 0.22).clamp(6.0, 14.0);
    atrium(g, c, rng, R::new(hall_r.c().x - w * 0.5, hall_r.c().x + w * 0.5, hall_r.v0 - 3.5, hall_r.v0 + 3.0), height + 2.5);
    // The food court's glazing at the end away from the anchor.
    let fw = (hall_r.w() * 0.25).clamp(6.0, 12.0);
    let fu = if left { hall_r.u1 - fw * 0.5 - 1.0 } else { hall_r.u0 + fw * 0.5 + 1.0 };
    if hall_r.w() > w + fw + 6.0 {
        food_court(g, c, rng, R::new(fu - fw * 0.5, fu + fw * 0.5, hall_r.v0 - 3.0, hall_r.v0 + 0.5), height * 0.5, height * 0.5 - 0.2, &[]);
    }
    let park = R::new(lot.u0 + 1.0, lot.u1 - 1.0, lot.v0 + 1.0, block.v0 - 4.5);
    front_car_park(g, c, rng, park, 0x3A11);
    let panels: Vec<(bool, [f32; 3])> = units.iter().enumerate().take(6).map(|(i, u)| (u.lit, sign_colour(c, i))).collect();
    pylon(g, c, Vec2::new(if left { lot.u1 - 2.5 } else { lot.u0 + 2.5 }, lot.v0 + 2.0), 11.0, &panels);
}

/// [malls] [p2-look] A mega-mall: two anchor stores at the ends of the
/// halls (taller blocks in their own cladding, big signs, their own
/// entrances), a tall glass entrance atrium in the middle, the food court
/// as a glass pavilion on the upper level beside it under a glazed roof,
/// plant on every roof, a multi-storey parking structure (beside the mall
/// on a wide site, behind it on a deep one) with a ramp, stair cores and
/// cars on every deck, a big surface car park in front and a pylon with
/// the anchors' names by the road. The site is compact
/// (`nd_sim::world::malls::MEGA_MALL_MIN_ASPECT`), so the anchors get room.
pub fn mega_mall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let pal = c.palette;
    let lot = R::of(&c.poly);
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.1 + 0.3 * c.wear));
    let fh = c.fh().max(3.4) * 1.35;
    let height = 2.0 * fh;
    // The parking structure beside the mall on a wide site, behind it on a
    // deep one; the surface car park in front.
    let wide = lot.w() >= lot.d() * 1.1 && lot.w() >= 60.0;
    let deck_w = if wide { (lot.w() * 0.26).clamp(16.0, 36.0) } else { 0.0 };
    let park_d = (lot.d() * if wide { 0.32 } else { 0.24 }).clamp(14.0, 34.0);
    let deck_d = if wide { 0.0 } else { (lot.d() * 0.26).clamp(14.0, 30.0) };
    let mall_back = if wide { lot.v1 - 3.0 } else { lot.v1 - deck_d - 6.0 };
    let want = R::new(lot.u0 + 2.0, lot.u1 - if wide { deck_w + 5.0 } else { 2.0 }, lot.v0 + park_d, mall_back.max(lot.v0 + park_d + 14.0));
    let Some(block) = fit(c, want, 10.0) else { return };
    let units = shops_or(e.units, 18);
    let anchors: Vec<usize> = units.iter().enumerate().filter(|(_, u)| u.role == UnitRole::Anchor).map(|(i, _)| i).collect();
    let aw = (block.w() * 0.24).clamp(10.0, 30.0);
    let hall_r = R::new(block.u0 + aw, block.u1 - aw, block.v0, block.v1 - 2.0);
    let signs: Vec<(usize, bool, u8)> = units.iter().enumerate().filter(|(_, u)| u.role == UnitRole::Shop).map(|(i, u)| (i, u.lit, u.lo)).collect();
    if hall_r.w() > 8.0 {
        halls(g, c, rng, hall_r, height, 2, &signs);
    }
    for (k, r) in [R::new(block.u0, block.u0 + aw, block.v0 - 3.0, block.v1), R::new(block.u1 - aw, block.u1, block.v0 - 3.0, block.v1)].into_iter().enumerate() {
        let (lit, col) = anchors.get(k).map_or((true, sign_colour(c, 90 + k)), |&i| (units[i].lit, sign_colour(c, i)));
        anchor_store(g, c, rng, r, height + 4.0, lit, col, k);
    }
    // The atrium in the middle of the front, the food court beside it.
    let w = (hall_r.w() * 0.22).clamp(7.0, 16.0);
    atrium(g, c, rng, R::new(hall_r.c().x - w * 0.5, hall_r.c().x + w * 0.5, hall_r.v0 - 4.0, hall_r.v0 + 3.0), height + 4.0);
    let counters: Vec<(bool, [f32; 3])> = units.iter().enumerate().filter(|(_, u)| u.role == UnitRole::FoodCourt).map(|(i, u)| (u.lit, sign_colour(c, i))).collect();
    let fw = ((hall_r.w() - w) * 0.5 - 2.0).clamp(0.0, 18.0);
    if fw > 6.0 {
        let fu = hall_r.c().x + w * 0.5 + 1.0 + fw * 0.5;
        food_court(g, c, rng, R::new(fu - fw * 0.5, fu + fw * 0.5, hall_r.v0 - 4.0, hall_r.v0 + 0.5), fh, fh - 0.3, &counters);
        // Its glazed roof, over the hall behind it.
        let roof = R::new(fu - fw * 0.5, fu + fw * 0.5, hall_r.v0 + 1.0, (hall_r.v0 + hall_r.d() * 0.45).min(hall_r.v1 - 1.0));
        if roof.d() > 3.0 {
            extrude(g, &c.frame, Glass, &roof.poly(), height, height + 2.4, mix(pick(rng, &pal.glass), pal.window_warm, 0.3));
            extrude(g, &c.frame, Glow, &roof.inset(-0.15).poly(), height + 2.4, height + 2.55, mul(pal.window_warm, 0.6));
        }
    }
    // The parking structure.
    let deck_r = if wide { R::new(lot.u1 - deck_w - 1.5, lot.u1 - 1.5, lot.v0 + park_d * 0.6, lot.v1 - 2.0) } else { R::new(lot.u0 + 3.0, lot.u1 - 8.0, lot.v1 - deck_d - 1.5, lot.v1 - 1.5) };
    if let Some(dr) = fit(c, deck_r, 12.0) {
        parking_deck(g, c, rng, dr, 3);
    }
    // The surface car park in front, the pylon by the road.
    let park = R::new(lot.u0 + 1.0, lot.u1 - if wide { deck_w + 3.0 } else { 1.0 }, lot.v0 + 1.0, block.v0 - 5.5);
    front_car_park(g, c, rng, park, 0x3E6A);
    let mut panels: Vec<(bool, [f32; 3])> = anchors.iter().map(|&i| (units[i].lit, sign_colour(c, i))).collect();
    panels.extend(signs.iter().take(4).map(|s| (s.1, sign_colour(c, s.0))));
    pylon(g, c, Vec2::new(lot.u0 + 2.5, lot.v0 + 2.0), 15.0, &panels);
}
