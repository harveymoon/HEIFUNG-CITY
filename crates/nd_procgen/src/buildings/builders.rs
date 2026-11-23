//! [r14-builders] The building trade's own looks (`nd_sim::builders`): a
//! building contractor's yard and a builders' merchant. Each is forced by
//! its business (`super::industry_styles`), whatever the zone, and wears
//! the company's fleet colour (`Extras::fleet_colour`) on its vans, its
//! lorries and its sign band.
//!
//! Layouts work in axis-aligned rectangles of the lot frame ([`R`]): `u`
//! along the street, `v` in from it, so the street side is `v0`.

use super::construction::{builder_lorry, builder_van, material_stack};
use super::kit::{block, fence_poly, fit, heap, pane, pane_row, R};
use super::parts::*;
use super::{Ctx, Extras};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

/// The company colour, or a sign colour from the palette.
fn paint_of(c: &Ctx, rng: &mut impl Rng, e: &Extras) -> [f32; 3] {
    e.fleet_colour.unwrap_or_else(|| pick(rng, &c.palette.sign_colors))
}

/// A builder's skip: an open steel box of rubble.
fn skip(g: &mut Geometry, c: &Ctx, p: Vec2, yaw: f32, paint: [f32; 3]) {
    let f = &c.frame;
    f.bx_yaw(g, Metal, p.x, 0.65, p.y, Vec3::new(3.6, 1.3, 1.8), mul(paint, 0.8), yaw);
    f.bx_yaw(g, Concrete, p.x, 1.25, p.y, Vec3::new(3.2, 0.25, 1.5), mul(c.palette.concrete(0), 0.6), yaw);
}

/// Bay lines for a car park along the street: `n` bays of `w` at `v`.
fn bays(g: &mut Geometry, c: &Ctx, u0: f32, n: i32, w: f32, v0: f32, d: f32) {
    let f = &c.frame;
    for k in 0..=n {
        let u = u0 + k as f32 * w;
        let p = Vec2::new(u, v0 + d * 0.5);
        if c.inside_local(p) {
            ground(g, f, Matte, &rect(u, v0 + d * 0.5, 0.06, d * 0.5 - 0.2), 0.05, [0.85, 0.85, 0.8]);
        }
    }
}

// ── the contractor's yard ────────────────────────────────────────────────────

/// A builder's yard: a portal shed with roller doors and an office at the
/// back under a sign band in the firm's colour, the vans parked by the
/// gate in its colours, a flatbed lorry with a load, bays of pallets,
/// blocks and steel, a skip and a heap of sand, a mesh fence round it.
pub fn builders_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let paint = paint_of(c, rng, e);
    yard(g, c, 0.3, if t == 0 { Dirt } else { Concrete }, if t == 0 { pal.gravel } else { mul(pal.concrete(0), 0.75) });
    let area = R::of(&c.poly);
    // The shed and office across the back.
    let sd = (area.d() * 0.38).clamp(7.0, 16.0);
    let want = R::new(area.u0 + 1.0, area.u0 + (area.w() * 0.7).max(10.0), area.v1 - sd, area.v1 - 0.5);
    let h = [5.0, 6.0, 7.0][t] + if c.floors > 1 { 2.5 } else { 0.0 };
    let mut yard_v1 = area.v1;
    if let Some(r) = fit(c, want, 6.0) {
        yard_v1 = r.v0 - 0.5;
        let (wall_s, wall) = if t == 0 { (WornPanel, pick(rng, &pal.panel)) } else { (Metal, mix(pick(rng, &pal.panel), [0.7, 0.72, 0.74], 0.4)) };
        let office_w = (r.w() * 0.3).clamp(3.5, 8.0);
        let shed = R::new(r.u0, r.u1 - office_w, r.v0, r.v1);
        let office = R::new(r.u1 - office_w, r.u1, r.v0, r.v1);
        block(g, c, wall_s, &shed, 0.0, h, wall);
        // A shallow pitched roof over the shed.
        f.bx(g, Metal, shed.c().x, h + 0.35, shed.c().y, Vec3::new(shed.w() + 0.4, 0.3, shed.d() + 0.4), mul(pal.steel, 0.6));
        let front = shed.front();
        let doors = ((shed.w() / 5.5).floor() as i32).clamp(1, 3);
        let dw = shed.w() / doors as f32;
        for k in 0..doors {
            let tt = -shed.w() * 0.5 + (k as f32 + 0.5) * dw;
            roller_door(g, c, &front, tt, dw - 1.0, (h - 1.2).min(4.5), mul(pal.steel, [0.55, 0.7, 0.8][t]));
        }
        // The office: two storeys of windows, a door, the sign band.
        let oh = h.max(5.5);
        block(g, c, if t == 0 { WornPanel } else { Panel }, &office, 0.0, oh, mix(wall, [0.9, 0.9, 0.88], 0.4));
        parapet(g, c, &office.poly(), oh, 0.4, 0.2, Matte, mul(wall, 0.8));
        let of = office.front();
        pane_row(g, c, rng, &of, 0.0, 2, oh * 0.5, 2.0, 1.3, 1.2, true);
        of.decal(g, f, Matte, -office.w() * 0.5 + 0.8, 1.05, 0.04, 0.9, 2.1, DARK);
        // The firm's colour along the whole front, the name over it.
        r.front().decal(g, f, Matte, 0.0, h - 0.5, 0.03, r.w(), 0.5, paint);
        sign_board(g, c, rng, &r.front(), r.w() * 0.15, h + 0.7, (r.w() * 0.45).clamp(3.0, 10.0), 0.9);
        floodlight(g, c, front.at(-shed.w() * 0.4, -0.6), h + 0.6);
    }
    // The yard in front: vans by the gate, the lorry, the stock.
    let yard_r = R::new(area.u0 + 0.8, area.u1 - 0.8, area.v0 + 0.8, yard_v1);
    let front = c.front_face(&c.poly);
    let along = Vec2::new(1.0, 0.0);
    let vans = [2, 3, 4][t];
    let mut placed = 0;
    let mut u = yard_r.u0 + 2.0;
    while placed < vans && u < yard_r.u1 - 1.5 {
        let p = Vec2::new(u, yard_r.v0 + 3.4);
        if vehicle_fits(c, p, FRAC_PI_2, Vehicle::Van) {
            builder_van(g, c, rng, p, FRAC_PI_2, paint);
            placed += 1;
        }
        u += 2.9;
    }
    // The lorry along the shed with a load on.
    let lp = Vec2::new(yard_r.c().x + yard_r.w() * 0.12, yard_v1 - 2.2);
    if vehicle_fits(c, lp, 0.0, Vehicle::Truck) && yard_r.d() > 9.0 {
        builder_lorry(g, c, rng, lp, 0.0, paint);
    }
    // Stock bays down the far side: pallets, blocks, steel and pipe.
    let side_u = yard_r.u1 - 2.2;
    let mut v = yard_r.v0 + 2.0;
    let mut k = 0;
    while v < yard_v1 - 1.5 && k < 6 {
        let p = Vec2::new(side_u, v);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.2 {
            material_stack(g, c, rng, p, FRAC_PI_2 + rng.random_range(-0.15..0.15));
        }
        v += 2.6;
        k += 1;
    }
    // A skip and a heap of sand.
    let sp = Vec2::new(yard_r.u0 + 2.5, yard_v1 - 2.0);
    if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > 2.0 {
        skip(g, c, sp, 0.0, paint);
    }
    heap(g, c, Vec2::new(yard_r.c().x - yard_r.w() * 0.15, yard_r.c().y), 1.8, 1.3, mix(pal.gravel, [0.75, 0.62, 0.4], 0.6));
    // Mesh fence round the yard, open at the gate.
    let lot = offset_poly(&c.lot_poly, 0.25);
    if lot.len() >= 3 {
        let front_i = c.faces(&lot).iter().enumerate().max_by(|a, b| (a.1.out.dot(Vec2::new(0.0, -1.0)) * a.1.len).total_cmp(&(b.1.out.dot(Vec2::new(0.0, -1.0)) * b.1.len))).map(|(i, _)| i);
        fence_poly(g, c, &lot, 2.2, front_i, t == 0);
    }
    let _ = (front, along);
    lamp_post_in(g, c, Vec2::new(area.u0 + 1.0, area.v0 + 1.0), 6.0, pal.street_lamp);
}

// ── the builders' merchant ───────────────────────────────────────────────────

/// A builders' merchant: a big clad shed with a glazed entrance and a trade
/// counter under a canopy, a lit sign band in the company's colour; beside
/// it a covered timber yard of racked lumber, pallets of bricks and blocks;
/// a car park along the street with bays, cars and a pickup or two, a pylon
/// sign at the kerb.
pub fn builders_merchant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let paint = paint_of(c, rng, e);
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, [1.3, 1.5, 1.7][t]));
    let area = R::of(&c.poly);
    // Car park along the street, the shed and yard behind it.
    let park_d = (area.d() * 0.3).clamp(6.0, 14.0);
    let back = R::new(area.u0 + 0.5, area.u1 - 0.5, area.v0 + park_d, area.v1 - 0.5);
    let yard_w = (back.w() * 0.32).clamp(6.0, 22.0);
    let shed_want = R::new(back.u0, back.u1 - yard_w, back.v0, back.v1);
    let h = [7.0, 8.5, 10.0][t] + if c.floors > 1 { 2.0 } else { 0.0 };
    let mut shed_r: Option<R> = None;
    if let Some(r) = fit(c, shed_want, 8.0) {
        shed_r = Some(r);
        let clad = match t {
            0 => (WornPanel, mix(pick(rng, &pal.panel), [0.6, 0.62, 0.6], 0.5)),
            1 => (Metal, mix(pick(rng, &pal.panel), [0.75, 0.76, 0.78], 0.5)),
            _ => (Panel, [0.82, 0.83, 0.85]),
        };
        block(g, c, clad.0, &r, 0.0, h, clad.1);
        parapet(g, c, &r.poly(), h, 0.6, 0.25, Matte, mul(clad.1, 0.8));
        // Roof lights in rows.
        let rows = ((r.d() / 6.0).floor() as i32).clamp(1, 4);
        for k in 0..rows {
            let v = r.v0 + (k as f32 + 0.5) * r.d() / rows as f32;
            f.bx(g, Glass, r.c().x, h + 0.08, v, Vec3::new(r.w() * 0.7, 0.12, 1.2), [0.55, 0.62, 0.66]);
        }
        let fr = r.front();
        // The sign band across the top in the company's colour, lit.
        fr.decal(g, f, Matte, 0.0, h - 0.8, 0.03, r.w(), 1.4, paint);
        fr.decal(g, f, Glow, -r.w() * 0.12, h - 0.8, 0.06, (r.w() * 0.45).clamp(4.0, 18.0), 0.8, mix(paint, [1.0, 1.0, 1.0], 0.35));
        // The glazed entrance and its canopy, to one side; the trade
        // counter's roller door at the other.
        let ew = (r.w() * 0.28).clamp(4.0, 12.0);
        let et = -r.w() * 0.5 + ew * 0.5 + 1.0;
        for k in 0..((ew / 2.0) as i32).max(1) {
            let tt = et - ew * 0.5 + (k as f32 + 0.5) * 2.0;
            let col = mul([1.2, 1.15, 1.05], 0.5 + 0.35 * c.lit_chance());
            fr.glaze(g, c, tt, 1.6, 0.05, 1.8, 3.0, Pane::Lit(col));
        }
        fr.bx(g, f, Panel, et, 3.6, -1.6, Vec3::new(ew + 1.0, 0.25, 3.2), mul(paint, 0.9));
        for s in [-1.0f32, 1.0] {
            fr.bx(g, f, Metal, et + s * (ew * 0.5 + 0.3), 1.8, -3.0, Vec3::new(0.2, 3.6, 0.2), mul(pal.steel, 0.7));
        }
        let dt = r.w() * 0.5 - 4.0;
        roller_door(g, c, &fr, dt, 4.5, 4.2, mul(pal.steel, 0.75));
        fr.decal(g, f, Glow, dt, 4.8, 0.05, 3.2, 0.5, [1.0, 0.75, 0.25]);
        // Pallets of compost and bags out front by the door.
        for k in 0..[4, 5, 6][t] {
            let p = fr.at(et + ew * 0.5 + 1.2 + k as f32 * 1.3, -1.0);
            if c.inside_local(p) {
                f.bx(g, Wood, p.x, 0.08, p.y, Vec3::new(1.1, 0.14, 1.0), pal.wood);
                let col = pick(rng, &[[0.2, 0.42, 0.18], [0.55, 0.38, 0.2], [0.75, 0.72, 0.62], mul(paint, 0.8)]);
                f.bx(g, Tarp, p.x, 0.5, p.y, Vec3::new(1.0, 0.7, 0.9), col);
            }
        }
        if t == 0 {
            grime(g, c, rng, &fr, 0.0, h, 1.4);
        }
    }
    // The timber yard beside the shed: a canopy over racks of lumber, and
    // pallets of bricks and blocks in the open.
    let yr = R::new(shed_r.map_or(back.u1 - yard_w, |r| r.u1 + 0.6), back.u1, back.v0, back.v1);
    if yr.w() > 4.0 && yr.d() > 6.0 {
        ground(g, f, Concrete, &yr.inset(0.2).poly(), 0.04, mul(pal.concrete(0), 0.7));
        let canopy = R::new(yr.u0 + 0.5, yr.u1 - 0.5, yr.c().y, yr.v1 - 0.5);
        let ch = 5.5;
        if canopy.w() > 3.0 && canopy.d() > 3.0 {
            for (u, v) in [(canopy.u0, canopy.v0), (canopy.u1, canopy.v0), (canopy.u0, canopy.v1), (canopy.u1, canopy.v1)] {
                if c.inside_local(Vec2::new(u, v)) {
                    f.bx(g, Metal, u, ch * 0.5, v, Vec3::new(0.25, ch, 0.25), mul(pal.steel, 0.7));
                }
            }
            f.bx(g, Metal, canopy.c().x, ch + 0.15, canopy.c().y, Vec3::new(canopy.w() + 0.6, 0.3, canopy.d() + 0.6), mul(paint, 0.7));
            // Racks of lumber in tiers.
            let racks = ((canopy.w() / 2.2).floor() as i32).clamp(1, 6);
            for k in 0..racks {
                let u = canopy.u0 + (k as f32 + 0.5) * canopy.w() / racks as f32;
                f.bx(g, Metal, u, 2.0, canopy.c().y, Vec3::new(0.15, 4.0, canopy.d() * 0.8), mul(pal.steel, 0.5));
                for tier in 0..3 {
                    let y = 0.5 + tier as f32 * 1.4;
                    if coin(rng, 0.85) {
                        let wood = mix(pal.wood, [0.85, 0.7, 0.45], rng.random_range(0.2..0.6));
                        f.bx(g, Wood, u, y, canopy.c().y, Vec3::new(1.2, 0.6, canopy.d() * 0.75), wood);
                    }
                }
            }
        }
        // Pallets in the open, in rows.
        let mut v = yr.v0 + 1.2;
        while v < canopy.v0 - 1.0 {
            let mut u = yr.u0 + 1.0;
            while u < yr.u1 - 0.8 {
                let p = Vec2::new(u, v);
                if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.8 {
                    f.bx(g, Wood, p.x, 0.08, p.y, Vec3::new(1.15, 0.14, 1.0), pal.wood);
                    let layers = rng.random_range(2..5);
                    let (s, col) = if coin(rng, 0.5) { (Brick, mul(pick(rng, &pal.brick), 0.95)) } else { (Concrete, mul(pick(rng, &pal.concrete), 0.95)) };
                    f.bx(g, s, p.x, 0.15 + layers as f32 * 0.22, p.y, Vec3::new(1.05, layers as f32 * 0.44, 0.9), col);
                    if coin(rng, 0.3) {
                        f.bx(g, Tarp, p.x, 0.17 + layers as f32 * 0.44, p.y, Vec3::new(1.12, 0.05, 0.98), pick(rng, &pal.tarp_colors));
                    }
                }
                u += 1.6;
            }
            v += 1.6;
        }
        // The merchant's delivery lorry, loaded, by the yard.
        let lp = Vec2::new(yr.c().x, yr.v0 + 0.5);
        if vehicle_fits(c, lp, FRAC_PI_2, Vehicle::Truck) && yr.w() > 8.0 {
            builder_lorry(g, c, rng, lp, FRAC_PI_2, paint);
        }
    }
    // The car park: bays along the street, customers' cars and pickups.
    let park = R::new(area.u0 + 1.0, area.u1 - 1.0, area.v0 + 0.8, area.v0 + park_d - 0.6);
    let bw = 2.7;
    let n = ((park.w() / bw).floor() as i32).clamp(0, 30);
    if n > 0 && park.d() > 5.0 {
        bays(g, c, park.u0, n, bw, park.v0 + 0.5, 5.2);
        for k in 0..n {
            if !coin(rng, [0.45, 0.6, 0.75][t]) {
                continue;
            }
            let p = Vec2::new(park.u0 + (k as f32 + 0.5) * bw, park.v0 + 3.0);
            let kind = if coin(rng, 0.3) { Vehicle::Van } else { Vehicle::Car };
            if vehicle_fits(c, p, FRAC_PI_2, kind) {
                let col = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, kind, col);
            }
        }
    }
    // Pylon sign at the kerb and lamp posts in the car park.
    let front = c.front_face(&c.poly);
    let p = front.at(front.len * 0.42, -0.7);
    if c.inside_local(p) {
        let ph = [5.0, 7.0, 9.0][t];
        f.bx(g, Matte, p.x, ph * 0.5, p.y, Vec3::new(0.4, ph, 0.4), CHARCOAL);
        f.bx(g, Matte, p.x, ph, p.y, Vec3::new(3.0, 1.6, 0.4), DARK);
        f.bx(g, Glow, p.x, ph, p.y, Vec3::new(2.8, 1.4, 0.44), paint);
    }
    for k in 0..3 {
        let q = Vec2::new(area.u0 + 2.0 + k as f32 * (area.w() - 4.0) * 0.5, area.v0 + park_d * 0.5);
        if c.inside_local(q) {
            lamp_post_in(g, c, q, 7.0, pal.street_lamp);
        }
    }
    let _ = pane;
}
