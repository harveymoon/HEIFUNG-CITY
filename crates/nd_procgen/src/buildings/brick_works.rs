//! [r4-brick] The works of the Brick & Steel look (Foundry Row's
//! industry): saw-tooth sheds with north lights glowing after dark, a
//! foundry with a casting shed, a cupola, a goliath gantry over the stock
//! yard and a rail siding, and a gas works with its holders in their
//! guide frames beside a brick retort house. Brick chimneys everywhere,
//! sodium lamps over the yards, the furnace glow in the doors.
//!
//! The shared colours and parts (brick paint, steel windows, works
//! stacks, lamps) are in `brick.rs`.

use super::brick::*;
use super::brick::Paint;
use super::kit::*;
use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

/// Glass of the north lights.
const NORTH_GLASS: [f32; 3] = [0.16, 0.20, 0.22];
/// Coal in a heap or a wagon.
const COAL: [f32; 3] = [0.07, 0.07, 0.08];
/// Paint of the gas holders and the gantries: works green, oxide red,
/// battleship grey.
const WORKS_PAINT: [[f32; 3]; 3] = [[0.20, 0.30, 0.24], [0.40, 0.15, 0.11], [0.30, 0.32, 0.34]];

/// A saw-tooth roof on a rectangle at `y`: ridges running back from the
/// street, each tooth a slope of sheet and a steep north light of glass
/// facing +u, brick gables front and back (so the street sees the saw).
/// After dark `glow` of the lights show the work inside.
#[allow(clippy::too_many_arguments)]
pub(super) fn saw_teeth(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32, tooth: f32, pt: &Paint, glow: f32) {
    let f = &c.frame;
    let n = ((r.w() / tooth).round() as i32).max(1);
    let tw = r.w() / n as f32;
    let rise = (tw * 0.42).clamp(1.0, 3.4);
    let sheet = rgba(mul(pt.roof, 1.15));
    let slope_n = (f.dir3(Vec2::NEG_X) * rise + Vec3::Y * tw).normalize();
    for i in 0..n {
        let ua = r.u0 + i as f32 * tw;
        let ub = ua + tw;
        g.part(Metal).quad_facing([f.at(ua, y, r.v0), f.at(ub, y + rise, r.v0), f.at(ub, y + rise, r.v1), f.at(ua, y, r.v1)], slope_n, sheet);
        g.part(Glass).quad_facing([f.at(ub, y, r.v0), f.at(ub, y + rise, r.v0), f.at(ub, y + rise, r.v1), f.at(ub, y, r.v1)], f.dir3(Vec2::X), rgba(NORTH_GLASS));
        for (v, out) in [(r.v0, Vec2::NEG_Y), (r.v1, Vec2::Y)] {
            g.part(Brick).tri_facing(f.at(ua, y, v), f.at(ub, y, v), f.at(ub, y + rise, v), f.dir3(out), rgba(pt.brick));
        }
        if coin(rng, glow) && r.d() > 1.2 && rise > 0.8 {
            let col = mul(if coin(rng, 0.5) { FURNACE } else { SODIUM }, rng.random_range(0.35..0.55));
            let from = g.get(Glow).colors.len();
            g.part(Glow).quad_facing(
                [f.at(ub + 0.06, y + 0.3, r.v0 + 0.4), f.at(ub + 0.06, y + rise - 0.25, r.v0 + 0.4), f.at(ub + 0.06, y + rise - 0.25, r.v1 - 0.4), f.at(ub + 0.06, y + 0.3, r.v1 - 0.4)],
                f.dir3(Vec2::X),
                rgba(col),
            );
            mark_glow(g, from, NIGHT_MARK);
        }
        // Glazing bars down the lights.
        g.lod(Lod::Near, |g| {
            let bars = ((r.d() / 1.6) as i32).clamp(1, 40);
            for k in 1..bars {
                let v = r.v0 + r.d() * k as f32 / bars as f32;
                g.part(Matte).beam(f.at(ub + 0.1, y + 0.05, v), f.at(ub + 0.1, y + rise - 0.05, v), 0.06, rgba(pt.steel));
            }
        });
    }
}

/// A railway siding along lot-local `a` to `b` with `wagons` wagons on it
/// (coal or box wagons): ballast, sleepers and rails.
fn siding(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec2, b: Vec2, wagons: usize) {
    let f = &c.frame;
    if a.distance(b) < 8.0 {
        return;
    }
    // The ballast is 3.6 m wide: both its edges must stay on the lot.
    let n = (b - a).normalize().perp() * 2.0;
    if !c.spans_lot(a, b) || !c.spans_lot(a + n, b + n) || !c.spans_lot(a - n, b - n) {
        return;
    }
    railway_track(g, c.palette, &[f.to_world(a.x, a.y), f.to_world(b.x, b.y)], 3.6);
    g.lod(Lod::Mid, |g| {
        let d = (b - a).normalize();
        for side in [-0.72f32, 0.72] {
            let o = Vec2::new(-d.y, d.x) * side;
            g.part(Matte).beam(f.at(a.x + o.x, 0.24, a.y + o.y), f.at(b.x + o.x, 0.24, b.y + o.y), 0.1, rgba(mul(c.palette.steel, 0.6)));
        }
    });
    let d = (b - a).normalize();
    let yaw = d.y.atan2(d.x);
    let len = 8.0;
    let room = a.distance(b) - 2.0;
    for k in 0..wagons {
        let s = 1.0 + len * 0.5 + k as f32 * (len + 1.0);
        if s + len * 0.5 > room {
            break;
        }
        let p = a + d * s;
        let coal = coin(rng, 0.6);
        let body = mul(pick(rng, &[[0.32, 0.14, 0.10], [0.24, 0.24, 0.26], [0.20, 0.17, 0.14]]), 1.0 - 0.3 * c.wear);
        f.bx_yaw(g, Matte, p.x, 0.75, p.y, Vec3::new(len, 0.4, 2.0), DARK, yaw);
        g.lod(Lod::Mid, |g| {
            for e in [-0.3f32, 0.3] {
                let q = p + d * (len * e);
                f.bx_yaw(g, Matte, q.x, 0.45, q.y, Vec3::new(1.8, 0.5, 1.5), CHARCOAL, yaw);
            }
        });
        if coal {
            f.bx_yaw(g, Rust, p.x, 1.85, p.y, Vec3::new(len - 0.2, 1.8, 2.6), body, yaw);
            f.bx_yaw(g, Dirt, p.x, 2.66, p.y, Vec3::new(len - 0.6, 0.3, 2.2), COAL, yaw);
        } else {
            f.bx_yaw(g, Metal, p.x, 2.0, p.y, Vec3::new(len - 0.2, 2.1, 2.6), body, yaw);
        }
    }
}

/// A goliath gantry over a stock yard: an A-frame of legs at each end,
/// a box girder across with a trolley, the hook hanging, from `u0` to
/// `u1` at `v`, `h` high.
fn goliath(g: &mut Geometry, c: &Ctx, u0: f32, u1: f32, v: f32, h: f32, paint: [f32; 3]) {
    let f = &c.frame;
    let col = rgba(paint);
    for u in [u0, u1] {
        for s in [-1.0f32, 1.0] {
            g.part(Matte).beam(f.at(u, 0.0, v + s * 2.6), f.at(u, h, v + s * 0.4), 0.55, col);
        }
        f.bx(g, Matte, u, h * 0.35, v, Vec3::new(0.4, 0.4, 4.2), paint);
        f.bx(g, Matte, u, 0.35, v, Vec3::new(1.2, 0.7, 6.0), mul(paint, 0.7));
    }
    f.bx(g, Metal, (u0 + u1) * 0.5, h + 0.6, v, Vec3::new(u1 - u0 + 1.6, 1.2, 1.5), paint);
    let tu = u0 + (u1 - u0) * 0.38;
    f.bx(g, Matte, tu, h - 0.2, v, Vec3::new(2.0, 0.8, 2.0), CHARCOAL);
    g.lod(Lod::Mid, |g| {
        f.bx(g, Matte, tu, h * 0.62, v, Vec3::new(0.06, h * 0.7, 0.06), DARK);
        f.bx(g, Matte, tu, h * 0.25, v, Vec3::new(0.6, 0.7, 0.4), [0.75, 0.55, 0.08]);
    });
    if h > 14.0 {
        aviation_light_local(g, f, c.palette, (u0 + u1) * 0.5, h + 1.4, v, 0.3);
    }
}

/// A gas holder at lot-local `p`: the tank wall, the bell risen to how
/// full it is, and the guide frame round it: standards, the top ring
/// girder (and the middle one and the cross bracing as detail).
fn gas_holder(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32) {
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    let sides = 24;
    let conc = mul(c.palette.concrete(1), 0.7);
    g.part(Concrete).cylinder(w, r + 0.6, r + 0.6, 0.0, 1.4, sides, rgba(conc));
    let fill = rng.random_range(0.3..0.95);
    let top = 1.4 + (h - 2.4) * fill;
    let paint = mul(pick(rng, &WORKS_PAINT), 1.0 - 0.3 * c.wear);
    g.part(Metal).cylinder_capped(w, r, r, 1.4, top, sides, false, false, rgba(paint));
    g.part(Metal).cylinder(w, r, r * 0.25, top, top + r * 0.12, sides, rgba(mul(paint, 0.85)));
    let frame = mul(pick(rng, &WORKS_PAINT), 0.9);
    let n = ((r * 0.8) as usize).clamp(8, 14);
    let rr = r + 1.1;
    let at = |i: usize| {
        let a = i as f32 / n as f32 * TAU;
        p + Vec2::new(a.cos(), a.sin()) * rr
    };
    for i in 0..n {
        let q = at(i);
        let a = i as f32 / n as f32 * TAU;
        f.bx_yaw(g, Matte, q.x, h * 0.5, q.y, Vec3::new(0.5, h, 0.5), frame, a);
        let q2 = at((i + 1) % n);
        g.part(Matte).beam(f.at(q.x, h, q.y), f.at(q2.x, h, q2.y), 0.45, rgba(frame));
    }
    g.lod(Lod::Mid, |g| {
        for i in 0..n {
            let (q, q2) = (at(i), at((i + 1) % n));
            g.part(Matte).beam(f.at(q.x, h * 0.5, q.y), f.at(q2.x, h * 0.5, q2.y), 0.3, rgba(frame));
        }
    });
    g.lod(Lod::Near, |g| {
        for i in 0..n {
            let (q, q2) = (at(i), at((i + 1) % n));
            for (y0, y1) in [(0.0, h * 0.5), (h * 0.5, h)] {
                g.part(Matte).beam(f.at(q.x, y0 + 0.3, q.y), f.at(q2.x, y1 - 0.3, q2.y), 0.1, rgba(frame));
                g.part(Matte).beam(f.at(q2.x, y0 + 0.3, q2.y), f.at(q.x, y1 - 0.3, q.y), 0.1, rgba(frame));
            }
        }
    });
    if h > 18.0 {
        aviation_light_local(g, f, c.palette, at(0).x, h + 0.5, at(0).y, 0.3);
    }
}

/// The works yard: the lot floored in soot-grey, a fence round it.
fn works_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) -> Vec<Vec2> {
    let lot = yard(g, c, 0.3, Dirt, mix(c.palette.dirt, SOOT, 0.45));
    g.lod(Lod::Near, |g| super::dirty::soot_stains(g, c, rng, &lot, 0.5 + 0.5 * c.wear, false));
    let barbed = coin(rng, 0.5);
    g.lod(Lod::Mid, |g| fence_lot(g, c, &lot, 2.4, barbed));
    lot
}

/// A two-storey brick office at the front of a works with the firm's
/// board over its door.
fn works_office(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, pt: &Paint) {
    let fh = 3.4;
    let h = 2.0 * fh;
    let poly = r.poly();
    brick_body(g, c, &poly, h, 2, fh, 1, 0.6, pt);
    for face in r.faces() {
        if face.len > 3.0 {
            window_rows(g, c, rng, &face, 0.0, 2, fh, 2.8, 0.55, 0.5, pt);
        }
    }
    let front = r.front();
    sign_board(g, c, rng, &front, 0.0, h - 0.2, (front.len * 0.8).min(8.0), 0.7);
    wall_lamp(g, c, &front, front.len * 0.3, 2.8);
}

// ── saw-tooth factory ────────────────────────────────────────────────────────

/// A brick factory shed under a saw-tooth roof: buttressed walls with tall
/// steel windows, roller doors, the north lights glowing after dark, a
/// brick office at the front with the firm's board, a works chimney, a
/// lorry at the dock and sodium lamps over the yard.
pub fn sawtooth_factory(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let pt = paint(c, rng);
    let t = c.tier.index();
    let lot = works_yard(g, c, rng);
    let area = R::of(&c.poly);
    let h = [5.5f32, 6.5, 7.5][t] + (c.floors.min(4) as f32 - 1.0).max(0.0) * 0.8;
    let front_room = (area.d() * 0.22).clamp(4.0, 14.0);
    let want = R::new(area.u0 + 1.2, area.u1 - 1.2, area.v0 + front_room, area.v1 - (area.d() * 0.1).clamp(1.2, 6.0));
    let Some(hall) = fit(c, want, 5.0) else {
        super::brick::brick_workshop(g, c, rng);
        return;
    };
    let poly = hall.poly();
    brick_body(g, c, &poly, h, 1, h, 0, 0.0, &pt);
    for (i, face) in hall.faces().iter().enumerate() {
        if face.len < 3.0 {
            continue;
        }
        if i == 0 {
            let doors = ((face.len - 2.0) / 9.0).floor().clamp(1.0, 3.0) as i32;
            for k in 0..doors {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / doors as f32;
                roller_door(g, c, face, tt, 3.6f32.min(face.len * 0.3), (h - 1.6).min(4.5), mul(pick(rng, &STEELS), 1.6));
            }
            sign_board(g, c, rng, face, 0.0, h - 0.7, (face.len * 0.5).min(12.0), 0.8);
            wall_lamp(g, c, face, -face.len * 0.45, h - 1.6);
            wall_lamp(g, c, face, face.len * 0.45, h - 1.6);
        } else {
            window_rows(g, c, rng, face, 0.6, 1, h - 1.2, 4.4, 0.55, 0.62, &pt);
            g.lod(Lod::Mid, |g| {
                let n = (face.len / 4.4).floor() as i32;
                for k in 0..=n {
                    let tt = (-face.len * 0.5 + k as f32 * face.len / n.max(1) as f32).clamp(-face.len * 0.5 + 0.35, face.len * 0.5 - 0.35);
                    super::hk::wall_box(g, c, Brick, face, tt, h * 0.45, 0.0, Vec3::new(0.7, h * 0.9, 0.5), mul(pt.brick, 0.9));
                }
            });
        }
        grime(g, c, rng, face, 0.0, h, 2.0 + 2.0 * c.wear);
    }
    saw_teeth(g, c, rng, &hall, h, [6.0f32, 7.0, 8.0][t], &pt, 0.65);
    // The office at the front corner.
    if front_room > 7.0 && area.w() > 22.0 {
        let ow = (area.w() * 0.3).min(12.0);
        if let Some(o) = fit(c, R::new(hall.u0, hall.u0 + ow, area.v0 + 1.2, hall.v0 - 1.2), 4.0) {
            works_office(g, c, rng, &o, &pt);
        }
    }
    // The works chimney at a back corner, else beside the shed.
    for sp in [Vec2::new(hall.u1 - 2.5, hall.v1 + 2.6), Vec2::new(hall.u1 + 2.4, hall.v1 - 3.0), Vec2::new(hall.u0 - 2.4, hall.v1 - 3.0)] {
        if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > 2.2 {
            works_stack(g, c, sp, 0.0, [18.0f32, 24.0, 30.0][t], 1.3, &pt);
            break;
        }
    }
    // A lorry at the dock, pallets, the lamps.
    let fr = hall.front();
    let lp = fr.at(fr.len * 0.25, -front_room * 0.45);
    if vehicle_fits(c, lp, FRAC_PI_2, Vehicle::Lorry) {
        let col = pick(rng, &pal.car_colors);
        vehicle(g, f, pal, rng, lp, 0.03, FRAC_PI_2, Vehicle::Lorry, col);
    }
    g.lod(Lod::Mid, |g| {
        for k in 0..4 {
            let p = fr.at(-fr.len * 0.3 + k as f32 * 1.4, -1.4);
            if c.inside_local(p) {
                f.bx(g, Wood, p.x, 0.5, p.y, Vec3::new(1.1, 1.0, 1.1), mul(pal.wood, 0.75));
            }
        }
    });
    for p in [fr.at(-fr.len * 0.5, -front_room * 0.6), fr.at(fr.len * 0.5, -front_room * 0.6)] {
        yard_lamp(g, c, p, 7.0);
    }
    let _ = lot;
}

// ── foundry ──────────────────────────────────────────────────────────────────

/// A foundry: a casting shed of brick and black sheet with a roof
/// monitor, its great doors glowing with the pour, a cupola furnace
/// beside it with a tap and a ladle, two or three works chimneys, a
/// goliath gantry over a stock yard of ingots and scrap, and a siding of
/// coal and box wagons along the back.
pub fn foundry_works(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let pt = paint(c, rng);
    let t = c.tier.index();
    let _ = works_yard(g, c, rng);
    let area = R::of(&c.poly);
    let h = [10.0f32, 12.5, 15.0][t];
    // The siding along the back when the lot is deep enough.
    let rail = area.d() > 40.0;
    let back = if rail { 7.0 } else { 2.0 };
    let stock = (area.d() * 0.3).clamp(5.0, 22.0);
    let shed_d = (area.d() - stock - back - 4.0).clamp(6.0, 30.0);
    let want = R::new(area.u0 + 1.5, area.u0 + 1.5 + (area.w() * 0.68).clamp(6.0, 70.0), area.v0 + stock + 1.0, area.v0 + stock + 1.0 + shed_d);
    let Some(shed) = fit(c, want, 6.0) else {
        sawtooth_factory(g, c, rng);
        return;
    };
    let sooty = mix(pt.brick, SOOT, 0.3);
    let sheet = mix(mul(c.palette.steel, 0.4), pal.rust, 0.2 + 0.4 * c.wear);
    let poly = shed.poly();
    extrude(g, f, Brick, &poly, 0.0, 3.4, sooty);
    extrude(g, f, Metal, &poly, 3.4, h, sheet);
    cornice(g, c, &poly, 3.45, 0.2, 0.08, Brick, pt.dark);
    pitched(g, c, &shed, h, (shed.d() * 0.18).clamp(1.2, 4.0), true, 0.4, mul(sheet, 0.8), sheet);
    let mon = R::new(shed.u0 + shed.w() * 0.12, shed.u1 - shed.w() * 0.12, shed.c().y - 1.4, shed.c().y + 1.4);
    let ridge = h + (shed.d() * 0.18).clamp(1.2, 4.0);
    block(g, c, Metal, &mon, ridge - 0.6, ridge + 1.6, mul(sheet, 0.7));
    for face in shed.faces() {
        if face.len < 3.0 {
            continue;
        }
        // A glowing strip of high windows under the eaves.
        let from = g.get(Glow).colors.len();
        face.decal(g, f, Glow, 0.0, h - 1.4, 0.06, face.len * 0.8, 0.9, mul(FURNACE, 0.4));
        mark_glow(g, from, NIGHT_MARK);
        grime(g, c, rng, &face, 0.0, h, 3.0);
    }
    // The great doors, glowing with the pour.
    let fr = shed.front();
    let doors = ((fr.len - 2.0) / 9.0).floor().clamp(1.0, 3.0) as i32;
    for k in 0..doors {
        let tt = -fr.len * 0.5 + (k as f32 + 0.5) * fr.len / doors as f32;
        fr.decal(g, f, Matte, tt, 3.0, 0.05, 5.0, 6.0, DARK);
        fr.decal(g, f, Glow, tt, 2.0, 0.08, 4.0, 3.6, mul(FURNACE, 0.75));
    }
    sign_board(g, c, rng, &fr, 0.0, h - 0.5, (fr.len * 0.4).min(12.0), 0.8);
    // The cupola beside the shed, its tap and ladle glowing.
    let cp = Vec2::new(shed.u1 + 4.0, shed.v0 + 4.0);
    if c.inside_local(cp) && distance_to_boundary(cp, &c.lot_poly) > 4.0 {
        let w = f.to_world(cp.x, cp.y);
        let ch = h + 6.0;
        g.part(Rust).cylinder(w, 2.2, 2.0, 0.0, ch * 0.45, 14, rgba(mix(pal.rust, SOOT, 0.4)));
        g.part(Metal).cylinder_capped(w, 1.8, 1.4, ch * 0.45, ch, 14, false, false, rgba(mul(pal.steel, 0.45)));
        g.part(Metal).cylinder(w, 1.6, 0.4, ch, ch + 1.6, 14, rgba(SOOT));
        f.bx(g, Glow, cp.x, 1.3, cp.y - 2.2, Vec3::new(1.0, 0.9, 0.3), FURNACE);
        let lp = Vec2::new(cp.x, cp.y - 3.6);
        if c.inside_local(lp) {
            let lw = f.to_world(lp.x, lp.y);
            g.part(Metal).cylinder(lw, 1.1, 0.9, 0.0, 1.6, 10, rgba(CHARCOAL));
            g.part(Glow).cylinder(lw, 1.0, 1.0, 1.6, 1.66, 10, rgba(FURNACE));
        }
        g.lod(Lod::Mid, |g| {
            for k in 1..(ch / 5.0) as i32 {
                let y = k as f32 * 5.0;
                g.part(Matte).cylinder(w, 2.6, 2.6, y, y + 0.16, 14, rgba(mul(pal.warning_light, 0.3)));
            }
        });
        aviation_light_local(g, f, pal, cp.x, ch + 1.9, cp.y, 0.3);
    }
    // Chimneys behind the shed.
    let n_st = [2, 2, 3][t];
    for k in 0..n_st {
        let sp = Vec2::new(shed.u0 + 3.0 + k as f32 * 7.0, shed.v1 + 2.6);
        if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > 2.2 {
            works_stack(g, c, sp, 0.0, [22.0f32, 28.0, 36.0][t] - k as f32 * 3.0, 1.5, &pt);
        }
    }
    // The stock yard in front: a goliath gantry over ingots and scrap.
    let sv = area.v0 + 1.2 + stock * 0.5;
    let (gu0, gu1) = (shed.u0 + 1.0, shed.u0 + (shed.w() * 0.75).clamp(6.0, 34.0));
    if stock > 7.0 && c.inside_local(Vec2::new(gu0, sv - 2.8)) && c.inside_local(Vec2::new(gu1, sv + 2.8)) && c.inside_local(Vec2::new(gu1, sv - 2.8)) {
        goliath(g, c, gu0, gu1, sv, (stock * 0.9).clamp(8.0, 16.0), pick(rng, &WORKS_PAINT));
        for k in 0..((gu1 - gu0) / 4.0) as i32 {
            let p = Vec2::new(gu0 + 2.5 + k as f32 * 4.0, sv + if k % 2 == 0 { 1.0 } else { -1.2 });
            if !c.inside_local(p) {
                continue;
            }
            if k % 3 == 2 {
                heap(g, c, p, 1.8, 1.6, mix(pal.rust, SOOT, 0.3));
            } else {
                g.lod(Lod::Mid, |g| {
                    for j in 0..3 {
                        f.bx(g, Rust, p.x, 0.25 + j as f32 * 0.42, p.y, Vec3::new(2.4 - j as f32 * 0.3, 0.4, 1.2), mul(pal.rust, 0.8 + 0.08 * j as f32));
                    }
                });
            }
        }
    }
    // The siding.
    if rail {
        let v = area.v1 - 3.4;
        siding(g, c, rng, Vec2::new(area.u0 + 1.0, v), Vec2::new(area.u1 - 1.0, v), 3);
    }
    for p in [Vec2::new(area.u0 + 2.0, area.v0 + 2.0), Vec2::new(area.u1 - 2.0, area.v0 + 2.0), Vec2::new(area.u1 - 2.0, area.c().y)] {
        yard_lamp(g, c, p, 8.0);
    }
}

// ── gas works ────────────────────────────────────────────────────────────────

/// A gas works: one to three holders in their guide frames, a long brick
/// retort house with tall windows and a louvred ridge, its chimney, pipes
/// from the house to the holders, coal heaps and a siding of coal wagons.
pub fn gas_works(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let pt = paint(c, rng);
    let t = c.tier.index();
    let _ = works_yard(g, c, rng);
    let area = R::of(&c.poly);
    // The retort house along the street.
    let rd = (area.d() * 0.22).clamp(5.0, 14.0);
    let Some(house) = fit(c, R::new(area.u0 + 1.5, area.u1 - 1.5, area.v0 + 2.0, area.v0 + 2.0 + rd), 4.0) else {
        sawtooth_factory(g, c, rng);
        return;
    };
    let hh = [7.0f32, 8.5, 10.0][t];
    brick_body(g, c, &house.poly(), hh, 1, hh, 0, 0.0, &pt);
    let rise = (house.d() * 0.28).clamp(1.0, 3.6);
    pitched(g, c, &house, hh, rise, true, 0.3, pt.roof, pt.brick);
    let lv = R::new(house.u0 + house.w() * 0.1, house.u1 - house.w() * 0.1, house.c().y - 0.9, house.c().y + 0.9);
    if lv.w() > 2.0 {
        block(g, c, Metal, &lv, hh + rise - 0.3, hh + rise + 0.9, mul(pt.roof, 0.8));
        pitched(g, c, &lv, hh + rise + 0.9, 0.5, true, 0.25, pt.roof, mul(pt.roof, 0.8));
    }
    for (i, face) in house.faces().iter().enumerate() {
        if face.len < 3.0 {
            continue;
        }
        window_rows(g, c, rng, face, 0.8, 1, hh - 1.6, 3.6, 0.45, 0.75, &pt);
        if i == 0 {
            wall_lamp(g, c, face, -face.len * 0.3, 3.4);
            wall_lamp(g, c, face, face.len * 0.3, 3.4);
        }
        grime(g, c, rng, face, 0.0, hh, 2.5);
    }
    let sp = Vec2::new(house.u1 - 2.4, house.v1 + 2.6);
    if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > 2.2 {
        works_stack(g, c, sp, 0.0, [16.0f32, 20.0, 26.0][t], 1.1, &pt);
    }
    // The holders behind, as many as fit side by side.
    let back = R::new(area.u0 + 1.5, area.u1 - 1.5, house.v1 + 4.0, area.v1 - 1.5);
    let rail = back.d() > 30.0 && back.w() > 30.0;
    let hd = if rail { back.d() - 6.0 } else { back.d() };
    let r = (hd * 0.5 - 1.8).clamp(0.0, [9.0f32, 12.0, 16.0][t]);
    let mut holders = Vec::new();
    if r >= 4.5 {
        let n = (((back.w() + 2.0) / (2.0 * r + 5.0)).floor() as i32).clamp(1, 3);
        for k in 0..n {
            let p = Vec2::new(back.u0 + (k as f32 + 0.5) * back.w() / n as f32, back.v0 + hd * 0.5);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > r + 2.0 {
                let h = r * rng.random_range(1.3..1.8);
                gas_holder(g, c, rng, p, r, h);
                holders.push(p);
            }
        }
    }
    // Pipes from the house to each holder.
    let pipe = mul(pick(rng, &WORKS_PAINT), 0.8);
    for p in &holders {
        let a = Vec3::new(p.x, 2.2, house.v1 + 0.3);
        let b = Vec3::new(p.x, 2.2, p.y - r - 1.6);
        if b.z > a.z + 0.5 {
            pipe_run(g, f, &[a, b], 0.35, pipe);
            g.lod(Lod::Mid, |g| {
                let n = ((b.z - a.z) / 4.0) as i32;
                for k in 1..=n {
                    f.bx(g, Concrete, p.x, 0.95, a.z + k as f32 * 4.0 - 2.0, Vec3::new(0.5, 1.9, 0.5), mul(pal.concrete(0), 0.6));
                }
            });
        }
    }
    // Coal by the house, and the siding along the back.
    for k in 0..2 {
        let p = Vec2::new(house.u0 + 3.0 + k as f32 * 6.0, house.v1 + 3.0);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.6 && holders.iter().all(|h| h.distance(p) > r + 4.0) {
            heap(g, c, p, 2.4, 2.0, COAL);
        }
    }
    if rail {
        let v = area.v1 - 4.0;
        siding(g, c, rng, Vec2::new(area.u0 + 1.0, v), Vec2::new(area.u1 - 1.0, v), 4);
    }
    for p in [Vec2::new(area.u0 + 2.0, house.v1 + 2.0), Vec2::new(area.u1 - 2.0, house.v1 + 2.0)] {
        yard_lamp(g, c, p, 7.5);
    }
}
