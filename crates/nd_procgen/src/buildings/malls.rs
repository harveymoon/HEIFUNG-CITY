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

use super::kit::R;
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

/// The shop units, or a stand-in row of `n` (a style preview with no sim
/// building behind it: half of them let).
fn shops_or(units: &[Unit], n: usize) -> Vec<(bool, UnitRole, u8)> {
    let v: Vec<(bool, UnitRole, u8)> = units.iter().filter(|u| u.shopfront).map(|u| (!u.vacant(), u.role, u.lo)).collect();
    if !v.is_empty() {
        return v;
    }
    (0..n).map(|i| (i % 2 == 0, UnitRole::Shop, 0)).collect()
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
            for s in [-1.0f32, 1.0] {
                f.flat(g, Glow, u + s * BAY_W * 0.5, 0.06, v, 0.1, BAY_D * 0.9, 0.0, mul(pal.lane_marker, 0.55));
            }
            if coin(rng, share) && vehicle_fits(c, p, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
                let col = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, p, 0.0, std::f32::consts::FRAC_PI_2, Vehicle::Car, col);
            }
        }
    }
    count
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
    // The car park in front: one row of bays and an aisle, deeper lots two.
    let park_d = if lot.d() > 44.0 { 2.0 * BAY_D + 6.0 + 2.0 } else { BAY_D + 6.5 };
    let block_d = (lot.d() - park_d - 1.5).clamp(6.0, 18.0);
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
    // Shopfronts: glazing and a door each, pilasters between them, a sign
    // over each door.
    for (i, (let_, _, _)) in units.iter().enumerate() {
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
        shop_sign(g, c, &front, t, ground_h - 0.55, (seg * 0.8).min(7.0), *let_, sign_colour(c, i));
    }
    // The covered walk along the shops, on posts at the car park's edge.
    let walk = 2.4f32.min(c.reach_out(&front) - 0.4);
    // The top gallery's floor: the posts run up to it.
    let gallery_top = ground_h + (floors - 2).max(0) as f32 * fh;
    if walk > 1.0 {
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
    let panels: Vec<(bool, [f32; 3])> = units.iter().enumerate().map(|(i, u)| (u.0, sign_colour(c, i))).collect();
    pylon(g, c, Vec2::new(lot.u1 - 2.0, lot.v0 + 1.6), 8.5, &panels);
    if c.wear > 0.4 {
        weeds(g, c, rng, &c.lot_poly, 5);
    }
}

/// The mall block's facade: concrete panels, a lit band of tenant signs on
/// each level along the front, the entrance atrium of glass in the middle.
fn mall_block(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], height: f32, levels: i32, units: &[(bool, UnitRole, u8)]) {
    let f = &c.frame;
    let pal = c.palette;
    let panel = mul(pick(rng, &pal.concrete), 1.05 - 0.3 * c.wear);
    extrude(g, f, Concrete, poly, 0.0, height, panel);
    parapet(g, c, poly, height, 1.2, 0.3, Concrete, mul(panel, 0.85));
    roof_gear(g, c, rng, poly, height, 1.0);
    let br = R::of(poly);
    let front = br.front();
    let lh = height / levels.max(1) as f32;
    // A dark band at each level, the shops' signs along it.
    for lv in 0..levels {
        let y = (lv as f32 + 0.75) * lh;
        front.decal(g, f, Matte, 0.0, y, 0.05, front.len * 0.96, 1.2, DARK);
        let mine: Vec<(usize, bool)> = units.iter().enumerate().filter(|(_, u)| u.2 as i32 == lv || (u.1 == UnitRole::Anchor && lv == 0)).map(|(i, u)| (i, u.0)).collect();
        let n = mine.len().max(1);
        let seg = front.len * 0.9 / n as f32;
        for (k, (i, lit)) in mine.iter().enumerate() {
            let t = -front.len * 0.45 + seg * (k as f32 + 0.5);
            if t.abs() < 5.0 {
                continue;
            }
            shop_sign(g, c, &front, t, y, (seg * 0.75).min(6.0), *lit, sign_colour(c, *i));
        }
    }
    // Entrance atrium: a glass box proud of the front, taller than the block.
    let aw = (front.len * 0.18).clamp(6.0, 16.0);
    let atrium = fit_inside(c, &R::new(br.c().x - aw * 0.5, br.c().x + aw * 0.5, br.v0 - 3.0, br.v0 + 4.0).poly());
    if atrium.len() >= 3 && poly_inside(c, &atrium) {
        extrude(g, f, Glass, &atrium, 0.0, height + 2.5, pick(rng, &pal.glass));
        curtain_lights(g, c, rng, &atrium, 0.0, levels + 1, (height + 2.5) / (levels + 1) as f32);
        let af = R::of(&atrium).front();
        entrance(g, c, rng, &af, 0.0, aw * 0.5, 3.4);
        sign_board(g, c, rng, &af, 0.0, height + 1.2, aw * 0.8, 1.4);
    }
    for face in c.faces(poly).iter().filter(|x| !x.is_front && x.len > 6.0) {
        grime(g, c, rng, face, 0.0, height, 2.0);
        // Service doors on the back and sides.
        roller_door(g, c, face, 0.0, 4.0, 3.6, mul(pick(rng, &pal.metal_paint), 0.7));
    }
}

/// [malls] A mall: one big two-level block of shops at the back of its lot,
/// its glass entrance atrium over a car park that runs round the front
/// and sides, a sign for every shop along each level.
pub fn mall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let pal = c.palette;
    let lot = R::of(&c.poly);
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.1 + 0.4 * c.wear));
    let height = 2.0 * c.fh().max(3.4) * 1.35;
    let park_d = (lot.d() * 0.42).clamp(12.0, 34.0);
    let side = (lot.w() * 0.08).clamp(1.5, 8.0);
    let block = R::new(lot.u0 + side, lot.u1 - side, lot.v0 + park_d, lot.v1 - 3.0);
    let poly = fit_inside(c, &block.poly());
    if poly.len() < 3 {
        return;
    }
    let units = shops_or(e.units, 8);
    mall_block(g, c, rng, &poly, height, 2, &units);
    let br = R::of(&poly);
    let mut prng = rng_for(c.seed, 0x3A11);
    let park = R::new(lot.u0 + 1.0, lot.u1 - 1.0, lot.v0 + 1.0, br.v0 - 3.5);
    bays_in(g, c, &mut prng, &park, 0.5 + 0.3 * (1.0 - c.wear), 3);
    lot_lamps(g, c, &R::new(park.u0, park.u1, park.c().y - 0.5, park.c().y + 0.5), 18.0);
    // Trolley bay and a planter strip by the doors.
    let front = br.front();
    if c.reach_out(&front) > 3.0 {
        front.bx(g, &c.frame, Metal, -front.len * 0.3, 0.55, 1.6, Vec3::new(4.0, 1.1, 1.2), mul(pal.steel, 0.8));
    }
}

/// [malls] A mega-mall: a long block with an anchor store at each end
/// (taller, its own colour and a big sign), a glazed food-court roof over
/// the middle, a multi-storey parking structure at one side and a big
/// surface car park in front; a pylon with the mall's name by the road.
pub fn mega_mall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = R::of(&c.poly);
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.1 + 0.3 * c.wear));
    let fh = c.fh().max(3.4) * 1.35;
    let height = 2.0 * fh;
    // The parking structure beside the mall on a wide site, behind it on a
    // narrow one; the surface car park in front.
    let wide = lot.w() >= 70.0;
    let deck_w = if wide { (lot.w() * 0.24).clamp(16.0, 40.0) } else { 0.0 };
    let park_d = (lot.d() * if wide { 0.35 } else { 0.25 }).clamp(14.0, 36.0);
    let mall_back = if wide { lot.v1 - 3.0 } else { lot.v0 + park_d + ((lot.d() - park_d) * 0.55).max(14.0) };
    let mall_r = R::new(lot.u0 + 2.0, lot.u1 - if wide { deck_w + 4.0 } else { 2.0 }, lot.v0 + park_d, mall_back);
    let poly = fit_inside(c, &mall_r.poly());
    if poly.len() < 3 {
        return;
    }
    let units = shops_or(e.units, 18);
    mall_block(g, c, rng, &poly, height, 2, &units);
    let br = R::of(&poly);
    // Anchors: a taller box at each end of the block, a big sign on each.
    let aw = (br.w() * 0.2).clamp(8.0, 30.0);
    let anchors: Vec<usize> = units.iter().enumerate().filter(|(_, u)| u.1 == UnitRole::Anchor).map(|(i, _)| i).collect();
    for (k, u) in [br.u0 + aw * 0.5, br.u1 - aw * 0.5].into_iter().enumerate() {
        let a = fit_inside(c, &R::new(u - aw * 0.5, u + aw * 0.5, br.v0 - 1.0, br.v1).poly());
        if a.len() < 3 || !poly_inside(c, &a) {
            continue;
        }
        let tone = mul(pick(rng, &pal.panel), 0.9 - 0.25 * c.wear);
        extrude(g, f, Panel, &a, 0.0, height + 3.0, tone);
        parapet(g, c, &a, height + 3.0, 1.0, 0.3, Panel, mul(tone, 0.8));
        let af = R::of(&a).front();
        let lit = anchors.get(k).map_or(true, |&i| units[i].0);
        let col = anchors.get(k).map_or(sign_colour(c, 90 + k), |&i| sign_colour(c, i));
        shop_sign(g, c, &af, 0.0, height + 0.6, (aw * 0.7).min(14.0), lit, col);
        entrance(g, c, rng, &af, 0.0, 5.0, 3.6);
    }
    // The food court's glazed roof over the middle of the block, glowing.
    let fc = fit_inside(c, &R::new(br.c().x - br.w() * 0.15, br.c().x + br.w() * 0.15, br.c().y - br.d() * 0.25, br.c().y + br.d() * 0.25).poly());
    if fc.len() >= 3 && poly_inside(c, &fc) {
        extrude(g, f, Glass, &fc, height, height + 2.6, mix(pick(rng, &pal.glass), pal.window_warm, 0.25));
        extrude(g, f, Glow, &offset_poly(&fc, 0.2), height + 2.6, height + 2.75, mul(pal.window_warm, 0.6));
    }
    // The parking structure: three open decks on columns, cars on the top.
    let deck_r = if wide { R::new(lot.u1 - deck_w - 1.5, lot.u1 - 1.5, lot.v0 + park_d * 0.5, lot.v1 - 3.0) } else { R::new(lot.u0 + 2.0, lot.u1 - 2.0, mall_back + 4.0, lot.v1 - 2.0) };
    let deck = if deck_r.d() >= 12.0 && deck_r.w() >= 12.0 { fit_inside(c, &deck_r.poly()) } else { Vec::new() };
    if deck.len() >= 3 && poly_inside(c, &deck) {
        let concrete = mul(pick(rng, &pal.concrete), 0.95 - 0.25 * c.wear);
        let dh = 3.0;
        for k in 1..=3 {
            let y = k as f32 * dh;
            extrude(g, f, Concrete, &deck, y - 0.4, y, concrete);
            parapet(g, c, &deck, y, 1.1, 0.25, Concrete, mul(concrete, 1.05));
            pilotis(g, c, &offset_poly(&deck, 0.6), y - dh, y - 0.4, 7.5, 0.5, Concrete, mul(concrete, 0.9));
        }
        let dr = R::of(&deck);
        let df = dr.front();
        df.decal(g, f, Glow, 0.0, dh * 0.5 + 0.4, 0.05, (df.len * 0.5).min(10.0), 0.8, pal.parking_sign);
        let mut drng = rng_for(c.seed, 0xDEC);
        let top = R::new(dr.u0 + 1.5, dr.u1 - 1.5, dr.v0 + 1.5, dr.v1 - 1.5);
        let fw = &c.frame;
        let n = (top.w() / BAY_W).floor() as i32;
        for row in 0..((top.d() / (BAY_D + 3.0)).floor() as i32).max(0) {
            let v = top.v0 + BAY_D * 0.5 + row as f32 * (BAY_D + 3.0);
            for k in 0..n {
                let p = Vec2::new(top.u0 + (k as f32 + 0.5) * BAY_W, v);
                if coin(&mut drng, 0.6) && c.inside_local(p) {
                    let col = pick(&mut drng, &pal.car_colors);
                    vehicle(g, fw, pal, &mut drng, p, 3.0 * dh + 0.01, std::f32::consts::FRAC_PI_2, Vehicle::Car, col);
                }
            }
        }
    }
    // The surface car park in front, lamps, the pylon by the road.
    let mut prng = rng_for(c.seed, 0x3E6A);
    let park = R::new(lot.u0 + 1.0, lot.u1 - if wide { deck_w + 2.0 } else { 1.0 }, lot.v0 + 1.0, br.v0 - 4.0);
    bays_in(g, c, &mut prng, &park, 0.6, 3);
    lot_lamps(g, c, &R::new(park.u0, park.u1, park.c().y - 0.5, park.c().y + 0.5), 16.0);
    let panels: Vec<(bool, [f32; 3])> = units.iter().enumerate().filter(|(_, u)| u.1 == UnitRole::Anchor).map(|(i, u)| (u.0, sign_colour(c, i))).collect();
    pylon(g, c, Vec2::new(lot.u0 + 2.5, lot.v0 + 2.0), 14.0, &panels);
}
