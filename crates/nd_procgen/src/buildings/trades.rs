//! [economy] Shops, garages and places to go out that the economy opens by
//! pressure (`nd_sim::market::BusinessKind`): a car repair shop, a car
//! dealership, a bike shop, a fish market, a seafood restaurant, a golf
//! driving range, artists' studios and a gallery. Each is forced by its
//! business (`super::industry_styles`), whatever the zone, and varies its
//! size, materials, signage and condition by tier.
//!
//! The layouts work in axis-aligned rectangles of the lot frame ([`R`]):
//! `u` along the street, `v` in from it, so the street side is `v0`.

use super::parts::*;
use super::kit::{block, bunting, fish_crates, fish_sign, fit, flagpole, mast_light, mural, pane, pane_row, sculpture, table, tyres, MURAL, R};
use super::{Ctx, Face, Frame};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

// ── car repair shop ──────────────────────────────────────────────────────────

/// Garage bays with roll-up doors along the back of a concrete apron, one
/// with a car up on a lift, an office with the sign, tyre stacks, cars
/// waiting. Cheap ones are brick with wrecks about; dear ones clad and
/// tidy with a pylon sign.
pub fn mechanic(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, Concrete, mul(pal.concrete(0), [0.7, 0.85, 0.95][t]));
    let area = R::of(&c.poly);
    // Oil stains on the apron.
    for _ in 0..[6, 3, 1][t] {
        if let Some(p) = c.random_inside(rng, 2.5) {
            let stain = ngon(p.x, p.y, rng.random_range(0.6..1.4), 6, rng.random_range(0.0..1.0));
            if poly_inside(c, &stain) {
                // Over the bay floors (0.05), not in them.
                ground(g, f, Asphalt, &stain, 0.056, mul(DARK, 0.6));
            }
        }
    }
    let (wall_s, wall) = match t {
        0 => (Brick, mul(pick(rng, &pal.brick), 0.85)),
        1 => (Panel, pick(rng, &pal.panel)),
        _ => (Panel, mix(pick(rng, &pal.panel), [0.85, 0.86, 0.88], 0.5)),
    };
    let h = [4.2, 4.6, 5.2][t];
    let gd = (area.d() - 6.0).clamp(5.5, 10.0);
    let want = R::new(area.u0, area.u0 + area.w().min([16.0, 22.0, 28.0][t]), area.v1 - gd, area.v1);
    let accent = pick(rng, &pal.sign_colors);
    if let Some(r) = fit(c, want, 5.0) {
        let mut office_w = [3.5, 4.5, 6.0][t];
        let bay_w = 4.6;
        if r.w() < office_w + bay_w {
            office_w = 0.0;
        }
        let n = (((r.w() - office_w) / bay_w).floor() as i32).clamp(1, [2, 3, 4][t]);
        let bw = ((r.w() - office_w) / n as f32).min(5.4);
        let bays = R::new(r.u0, r.u0 + bw * n as f32, r.v0, r.v1);
        // The open shell: back wall, roof, partitions, fascia.
        f.bx(g, wall_s, bays.c().x, h * 0.5, r.v1 - 0.15, Vec3::new(bays.w(), h, 0.3), wall);
        f.bx(g, Metal, r.c().x, h + 0.15, r.c().y, Vec3::new(r.w() + 0.3, 0.3, r.d() + 0.3), mul(pal.steel, 0.55));
        for k in 0..=n {
            let u = bays.u0 + k as f32 * bw;
            let u = crate::mesh::clamp_safe(u, bays.u0 + 0.15, bays.u1 - 0.15);
            f.bx(g, wall_s, u, h * 0.5, r.c().y, Vec3::new(0.3, h, r.d()), wall);
        }
        f.bx(g, wall_s, bays.c().x, h - 0.4, r.v0 + 0.15, Vec3::new(bays.w(), 0.8, 0.3), mul(wall, 0.9));
        // A stripe of the shop's colour along the fascia.
        let front = bays.front();
        front.decal(g, f, Matte, 0.0, h - 0.6, 0.02, bays.w(), 0.18, mul(accent, 0.5));
        let inner_back = Face::new(Vec2::new(bays.u0, r.v1 - 0.31), Vec2::new(bays.u1, r.v1 - 0.31), Vec2::new(0.0, -1.0));
        for k in 0..n {
            let uc = bays.u0 + (k as f32 + 0.5) * bw;
            let tt = uc - bays.c().x;
            ground(g, f, Concrete, &rect(uc, r.c().y, bw * 0.5 - 0.2, r.d() * 0.5 - 0.3), 0.05, mul(pal.concrete(0), 0.55));
            inner_back.decal(g, f, Matte, tt, h * 0.45, 0.01, bw - 0.4, h * 0.85, mul(wall, 0.45));
            f.bx(g, Glow, uc, h - 0.05, r.c().y, Vec3::new(0.25, 0.06, r.d() * 0.6), mul(pal.lamp, 0.9));
            // A tool wall: a red chest and a pegboard.
            inner_back.bx(g, f, Metal, tt - bw * 0.3, 0.5, -0.3, Vec3::new(0.9, 1.0, 0.5), [0.7, 0.12, 0.1]);
            inner_back.decal(g, f, Matte, tt + bw * 0.15, 1.8, 0.03, 1.6, 1.0, mul(pal.wood, 0.9));
            let car = pick(rng, &pal.car_colors);
            match if k == 0 { 0 } else { rng.random_range(0..3) } {
                0 => {
                    // Two-post lift with a car up on it.
                    for s in [-1.0f32, 1.0] {
                        f.bx(g, Metal, uc + s * (bw * 0.5 - 0.55), 1.7, r.c().y, Vec3::new(0.35, 3.4, 0.45), [0.85, 0.6, 0.1]);
                        f.bx(g, Metal, uc + s * (bw * 0.5 - 0.9), 1.55, r.c().y, Vec3::new(0.6, 0.1, 2.4), CHARCOAL);
                    }
                    if r.d() > 4.8 {
                        vehicle(g, f, pal, rng, Vec2::new(uc, r.c().y), 1.6, FRAC_PI_2, Vehicle::Car, car);
                    }
                }
                1 => roller_door(g, c, &front, tt, bw - 0.6, h - 1.0, mul(pal.steel, [0.5, 0.7, 0.85][t])),
                _ => {
                    // Door half up, a car in for a service.
                    front.decal(g, f, Metal, tt, h - 1.4, 0.05, bw - 0.6, 0.8, mul(pal.steel, 0.7));
                    if r.d() > 4.8 {
                        vehicle(g, f, pal, rng, Vec2::new(uc, r.c().y), 0.0, FRAC_PI_2, Vehicle::Car, car);
                    }
                }
            }
        }
        // Office at the end: glazed, the sign over it.
        if office_w > 0.0 {
            let o = R::new(bays.u1, r.u1, r.v0, r.v1);
            let oh = if t == 2 { h * 1.6 } else { h };
            block(g, c, wall_s, &o, 0.0, oh, wall);
            parapet(g, c, &o.poly(), oh, 0.4, 0.2, Matte, mul(wall, 0.8));
            let of = o.front();
            // The window beside the door, never across it (the door is a
            // decal in the window's plane).
            let pw = (o.w() - 2.5).max(0.6);
            pane(g, c, rng, &of, 0.55, 1.5, pw, 1.6, false);
            of.decal(g, f, Matte, -o.w() * 0.5 + 0.7, 1.05, 0.04, 0.9, 2.1, DARK);
            if t == 2 {
                pane_row(g, c, rng, &of, h, 1, oh - h, 2.0, 1.4, 1.2, false);
            }
        }
        let sign_w = (r.w() * 0.5).clamp(2.5, 9.0);
        sign_board(g, c, rng, &r.front(), -r.w() * 0.15, h + 0.75, sign_w, 0.7);
        if t == 0 {
            grime(g, c, rng, &r.front(), 0.0, h, 1.2);
        }
    }
    // Tyre stacks by the street corner, cars waiting on the apron.
    let front = c.front_face(&c.poly);
    for k in 0..[5, 4, 3][t] {
        let p = front.at(front.len * 0.5 - 1.0 - (k / 2) as f32 * 0.9, -1.0 - (k % 2) as f32 * 0.9);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.5 {
            tyres(g, f, p, rng.random_range(3..7));
        }
    }
    for k in 0..[3, 2, 2][t] {
        let p = Vec2::new(area.u0 + 3.0 + k as f32 * 3.0, area.v0 + 2.8);
        let kind = if t == 0 && k == 0 { Vehicle::Wreck } else { Vehicle::Car };
        if vehicle_fits(c, p, FRAC_PI_2, kind) && coin(rng, 0.75) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, kind, col);
        }
    }
    if t == 0 {
        if let Some(p) = c.random_inside(rng, 3.0) {
            for k in 0..3 {
                let w = f.to_world(p.x + k as f32 * 0.7, p.y);
                g.part(Rust).cylinder(w, 0.3, 0.3, 0.0, 0.9, 8, rgba(pal.rust));
            }
        }
    } else {
        // Pylon sign at the kerb.
        let p = front.at(-front.len * 0.42, -0.8);
        if c.inside_local(p) {
            let ph = [0.0, 4.5, 7.0][t];
            f.bx(g, Matte, p.x, ph * 0.5, p.y, Vec3::new(0.3, ph, 0.3), CHARCOAL);
            f.bx(g, Matte, p.x, ph, p.y, Vec3::new(2.2, 1.2, 0.3), DARK);
            f.bx(g, Glow, p.x, ph, p.y, Vec3::new(2.0, 1.0, 0.34), mul(accent, 0.9));
        }
    }
    floodlight(g, c, front.at(front.len * 0.3, -0.8), 5.5);
}

// ── car dealership ───────────────────────────────────────────────────────────

/// Paints a dealer's whole stock comes in.
const NEW_CAR_PAINT: [[f32; 3]; 9] = [
    [0.7, 0.05, 0.06],
    [0.9, 0.9, 0.9],
    [0.62, 0.64, 0.68],
    [0.04, 0.04, 0.05],
    [0.05, 0.22, 0.7],
    [0.9, 0.7, 0.05],
    [0.05, 0.5, 0.5],
    [0.35, 0.05, 0.4],
    [0.9, 0.35, 0.05],
];

/// A forecourt full of rows of the same new car in the same paint (one
/// model and colour per dealer, from the seed), a glass showroom behind,
/// flags and bunting. Cheap dealers trade from a cabin under strings of
/// pennants; dear ones have a tall glass hall, flagpoles and a turntable.
pub fn car_dealer(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, if t == 0 { Asphalt } else { Paving }, if t == 0 { mul(pal.asphalt, 1.6) } else { mul(pal.paving, 1.1) });
    let area = R::of(&c.poly);
    let paint = NEW_CAR_PAINT[rng_for(c.seed, 0xCA_0D).random_range(0..NEW_CAR_PAINT.len())];
    let brand = pick(rng, &pal.sign_colors);
    // The showroom across the back.
    let sd = (area.d() * 0.35).clamp(6.0, 16.0);
    let sh = [3.2, 5.0, 8.0][t];
    let share = [0.4, 0.7, 0.92][t];
    let want = R::new(area.c().x - area.w() * share * 0.5, area.c().x + area.w() * share * 0.5, area.v1 - sd, area.v1);
    let mut court_v1 = area.v1;
    if let Some(r) = fit(c, want, 4.0) {
        court_v1 = r.v0 - 1.0;
        if t == 0 {
            // Sales cabin.
            let cab = r.inset((r.w() * 0.5 - 3.2).max(0.0).min(r.w() * 0.3));
            let cab = R::new(cab.u0, cab.u1, cab.v0, (cab.v0 + 2.6).min(cab.v1));
            block(g, c, WornPanel, &cab, 0.0, sh, pick(rng, &pal.panel));
            let fr = cab.front();
            pane(g, c, rng, &fr, -cab.w() * 0.2, 1.5, (cab.w() * 0.4).max(0.8), 1.0, false);
            sign_board(g, c, rng, &fr, 0.0, sh + 0.6, (cab.w() * 0.8).max(1.5), 0.6);
        } else {
            let glass = mix(pick(rng, &pal.glass), [0.7, 0.8, 0.85], 0.3);
            block(g, c, Glass, &r, 0.0, sh, glass);
            // A clean frame: roof plate overhanging the front, mullions.
            f.bx(g, Panel, r.c().x, sh + 0.25, r.c().y - 0.6, Vec3::new(r.w() + 0.6, 0.5, r.d() + 1.2), [0.88, 0.89, 0.9]);
            let fr = r.front();
            let mull = ((r.w() / 3.0) as i32).max(1);
            for k in 0..=mull {
                let tt = -r.w() * 0.5 + k as f32 * r.w() / mull as f32;
                fr.bx(g, f, Matte, tt, sh * 0.5, 0.05, Vec3::new(0.12, sh, 0.1), [0.85, 0.86, 0.88]);
                if k < mull {
                    // The showroom lit bright inside.
                    let tc = tt + r.w() / mull as f32 * 0.5;
                    let col = mul([1.25, 1.2, 1.1], 0.6 + 0.3 * c.lit_chance());
                    fr.glaze(g, c, tc, sh * 0.45, 0.07, r.w() / mull as f32 * 0.85, sh * 0.75, Pane::Lit(col));
                }
            }
            // The brand along the roof plate.
            fr.decal(g, f, Glow, 0.0, sh + 0.25, 0.65, (r.w() * 0.5).min(14.0), 0.32, brand);
            if t == 2 {
                // Second height: a service wing behind the glass.
                let wing = R::new(r.u0, r.u1, r.c().y + r.d() * 0.15, r.v1);
                block(g, c, Panel, &wing, sh, sh + 3.0, [0.82, 0.83, 0.85]);
                pane_row(g, c, rng, &wing.front(), sh, 1, 3.0, 3.0, 2.2, 1.0, false);
            }
        }
    }
    // The forecourt: rows of matching new cars facing the street.
    let court = R::new(area.u0 + 0.5, area.u1 - 0.5, area.v0 + 0.8, court_v1);
    let yaw = -FRAC_PI_2;
    let (cw, cd) = (2.7, 5.3);
    let mut cars = 0;
    let mut v = court.v0 + cd * 0.5;
    let pitch = if t == 2 { 3.1 } else { cw };
    while v + cd * 0.5 <= court.v1 && cars < 26 {
        let n = ((court.w() / pitch).floor() as i32).max(0);
        let u0 = court.c().x - (n as f32 - 1.0) * pitch * 0.5;
        for i in 0..n {
            let p = Vec2::new(u0 + i as f32 * pitch, v);
            if cars < 26 && vehicle_fits(c, p, yaw, Vehicle::Car) {
                // One model: every car draws from the same stream.
                let mut model = rng_for(c.seed, 0xCA_0E);
                vehicle(g, f, pal, &mut model, p, 0.0, yaw, Vehicle::Car, paint);
                // A price card on the windscreen.
                f.bx(g, Matte, p.x, 1.25, p.y - 0.5, Vec3::new(0.35, 0.25, 0.02), [0.95, 0.95, 0.9]);
                cars += 1;
            }
        }
        v += cd + if t == 0 { 0.4 } else { 1.2 };
    }
    // Flags, banners and bunting along the street.
    let front = c.front_face(&c.poly);
    let flags = [3, 4, 5][t];
    let flag_cols = [brand, [0.95, 0.95, 0.95], paint, [0.9, 0.1, 0.1]];
    let mut poles: Vec<Vec2> = Vec::new();
    for k in 0..flags {
        let tt = -front.len * 0.45 + k as f32 * front.len * 0.9 / (flags - 1).max(1) as f32;
        let p = front.at(tt, -0.4);
        if c.inside_local(p) {
            flagpole(g, f, p, [5.0, 7.0, 10.0][t], flag_cols[k as usize % flag_cols.len()], t < 2);
            poles.push(p);
        }
    }
    if t < 2 {
        let colors = [[0.9, 0.1, 0.1], [0.95, 0.85, 0.1], [0.1, 0.4, 0.9], [0.95, 0.95, 0.95]];
        for w in poles.windows(2) {
            bunting(g, f, Vec3::new(w[0].x, 4.0, w[0].y), Vec3::new(w[1].x, 4.0, w[1].y), 10, &colors);
        }
        // Strings back to the cabin.
        if let Some(p) = poles.first().filter(|_| c.inside_local(Vec2::new(area.c().x, court_v1)) && court_v1 < area.v1) {
            bunting(g, f, Vec3::new(p.x, 4.0, p.y), Vec3::new(area.c().x, 3.4, court_v1), 12, &colors);
        }
    } else {
        // A turntable with the star car, lit.
        let p = Vec2::new(area.u1 - 4.0, area.v0 + 4.0);
        if poly_inside(c, &rect(p.x, p.y, 3.0, 3.0)) {
            let w = f.to_world(p.x, p.y);
            g.part(Metal).cylinder(w, 2.9, 2.9, 0.0, 0.3, 16, rgba([0.75, 0.76, 0.78]));
            g.part(Glow).cylinder(w, 3.0, 3.0, 0.05, 0.12, 16, rgba(mul(brand, 0.8)));
            let mut model = rng_for(c.seed, 0xCA_0E);
            vehicle(g, f, pal, &mut model, p, 0.3, 0.6, Vehicle::Car, paint);
        }
    }
    // Pylon with the brand.
    let p = front.at(front.len * 0.47, -0.6);
    if c.inside_local(p) {
        let ph = [3.5, 6.0, 9.0][t];
        f.bx(g, Panel, p.x, ph * 0.5, p.y, Vec3::new(0.9, ph, 0.4), [0.85, 0.86, 0.88]);
        f.bx(g, Glow, p.x, ph - 1.0, p.y, Vec3::new(0.95, 1.4, 0.44), brand);
    }
    for k in 0..2 {
        let p = Vec2::new(area.u0 + 1.0 + k as f32 * (area.w() - 2.0), area.v0 + 1.0);
        if c.inside_local(p) {
            lamp_post_in(g, c, p, 6.0, pal.street_lamp);
        }
    }
}

// ── bike shop ────────────────────────────────────────────────────────────────

/// A bike: two wheels, a frame, a bar and a saddle.
pub(super) fn bike(g: &mut Geometry, f: &Frame, p: Vec2, yaw: f32, col: [f32; 3]) {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    for s in [-0.52f32, 0.52] {
        let q = p + d * s;
        f.bx_yaw(g, Matte, q.x, 0.34, q.y, Vec3::new(0.66, 0.66, 0.05), DARK, yaw);
    }
    let rear = p - d * 0.5;
    let head = p + d * 0.35;
    g.part(Metal).beam(f.at(rear.x, 0.36, rear.y), f.at(head.x, 0.85, head.y), 0.06, rgba(col));
    g.part(Metal).beam(f.at(p.x - d.x * 0.15, 0.4, p.y - d.y * 0.15), f.at(p.x - d.x * 0.2, 0.9, p.y - d.y * 0.2), 0.06, rgba(col));
    let bar = p + d * 0.42;
    f.bx_yaw(g, Matte, bar.x, 0.98, bar.y, Vec3::new(0.05, 0.05, 0.5), CHARCOAL, yaw);
    let seat = p - d * 0.2;
    f.bx_yaw(g, Matte, seat.x, 0.95, seat.y, Vec3::new(0.25, 0.05, 0.12), DARK, yaw);
}

/// A narrow shopfront with bikes racked on the pavement, wheels hung in
/// the window, a hanging sign; flats above when the building is taller.
pub fn bike_shop(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Paving, mul(pal.paving, [0.85, 1.0, 1.1][t]));
    let area = R::of(&c.poly);
    let bw = area.w().min([7.0, 9.0, 11.0][t]);
    let bd = (area.d() - 2.5).clamp(4.0, 14.0);
    let cu = area.u0 + bw * 0.5 + if area.w() > bw + 6.0 { 3.0 } else { 0.0 };
    let want = R::new(cu - bw * 0.5, cu + bw * 0.5, area.v0 + 2.5, area.v0 + 2.5 + bd);
    let floors = c.floors.clamp(1, 3) as i32;
    let fh = 3.4;
    let accent = MURAL[rng_for(c.seed, 0xB1CE).random_range(0..MURAL.len())];
    let (s, wall) = match t {
        0 => (Brick, mul(pick(rng, &pal.brick), 0.9)),
        1 => (Plaster, pick(rng, &pal.plaster)),
        _ => (Panel, mix(pick(rng, &pal.panel), [0.9, 0.9, 0.88], 0.4)),
    };
    let mut shop_front: Option<(Face, R)> = None;
    if let Some(r) = fit(c, want, 3.5) {
        let h = floors as f32 * fh;
        block(g, c, s, &r, 0.0, h, wall);
        cornice(g, c, &r.poly(), h, 0.3, 0.2, Matte, mul(wall, 0.8));
        let fr = r.front();
        // Shop window, lit warm, wheels hung in it.
        let gw = (r.w() - 2.2).max(1.0);
        fr.glaze(g, c, -r.w() * 0.5 + 1.0 + gw * 0.5, 1.5, 0.04, gw, 2.2, Pane::Dark);
        let col = mul(pal.window_warm, 0.7);
        fr.glaze(g, c, -r.w() * 0.5 + 1.0 + gw * 0.5, 1.5, 0.06, gw * 0.95, 2.0, Pane::Lit(col));
        for k in 0..((gw / 1.2) as i32).min(5) {
            fr.decal(g, f, Matte, -r.w() * 0.5 + 1.6 + k as f32 * 1.2, 2.0, 0.08, 0.6, 0.6, DARK);
        }
        fr.decal(g, f, Matte, r.w() * 0.5 - 0.7, 1.1, 0.05, 1.0, 2.2, mul(accent, 0.5));
        // Fascia in the shop's colour with the name, an awning.
        fr.bx(g, f, Matte, 0.0, 3.0, 0.12, Vec3::new(r.w(), 0.6, 0.2), mul(accent, 0.7 - 0.3 * c.wear));
        sign_board(g, c, rng, &fr, 0.0, 3.0, (r.w() * 0.6).min(6.0), 0.4);
        if t > 0 {
            awning(g, c, rng, &fr, -r.w() * 0.2, 2.5, (r.w() * 0.55).min(5.0));
        }
        if floors > 1 {
            let mut spec = WindowSpec::from_ctx(c);
            spec.spacing = 2.4;
            for face in r.faces() {
                if face.is_front || coin(rng, 0.5) {
                    windows(g, c, rng, &face, fh, floors - 1, fh, &spec);
                }
            }
        }
        // A wheel-shaped hanging sign.
        let p = fr.at(r.w() * 0.5 - 0.3, 0.9);
        if c.inside_local(p) {
            super::kit::hanging_sign(g, c, rng, &fr, r.w() * 0.5 - 0.3, 3.6, 0.9, 0.9);
        }
        if t == 0 {
            graffiti(g, c, rng, &r.left(), 0.2, 2.8, 1.0);
        }
        shop_front = Some((fr, r));
    }
    // Racks of bikes on the pavement.
    let racks_v = shop_front.map(|(_, r)| r.v0 - 1.1).unwrap_or(area.v0 + 1.1);
    let n = [5, 8, 10][t];
    let span_u = (area.u0 + 0.6, area.u1 - 0.6);
    let mut rack = (f32::MAX, f32::MIN);
    for k in 0..n {
        let u = span_u.0 + 0.4 + k as f32 * 0.8;
        if u > span_u.1 {
            break;
        }
        let p = Vec2::new(u, racks_v);
        if !c.inside_local(p + Vec2::new(0.0, -0.7)) || !c.inside_local(p + Vec2::new(0.0, 0.7)) {
            continue;
        }
        f.bx(g, Metal, u, 0.4, racks_v, Vec3::new(0.06, 0.8, 0.06), mul(pal.steel, 1.1));
        rack = (rack.0.min(u), rack.1.max(u));
        if coin(rng, 0.8) {
            let col = MURAL[rng.random_range(0..MURAL.len())];
            bike(g, f, p, FRAC_PI_2, col);
        }
    }
    if rack.1 > rack.0 {
        f.bx(g, Metal, (rack.0 + rack.1) * 0.5, 0.75, racks_v, Vec3::new(rack.1 - rack.0, 0.05, 0.05), mul(pal.steel, 1.1));
    }
    // A workstand and a bike up on it out front when the shop is open.
    if let Some((_, r)) = shop_front {
        let p = Vec2::new(r.u1 + 1.6, r.v0 + 1.5);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
            f.bx(g, Matte, p.x, 0.6, p.y, Vec3::new(0.08, 1.2, 0.08), CHARCOAL);
            bike(g, f, p + Vec2::new(0.0, 0.0), 0.0, accent);
        }
    }
}

// ── fish market ──────────────────────────────────────────────────────────────

/// An open-sided hall over rows of stalls with awnings, ice trays of fish,
/// crates stacked about, lamps hung from the roof. Tarps over poles when
/// cheap, a steel shed roof in the middle, a glazed vault when dear.
pub fn fish_market(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, if t == 0 { WornPaving } else { Paving }, mul(pal.paving, 0.8));
    let area = R::of(&c.poly);
    let want = R::new(area.u0, area.u1, area.v0 + 2.5, area.v1);
    let h = [3.6, 5.5, 7.0][t];
    let Some(r) = fit(c, want, 5.0) else {
        for _ in 0..4 {
            if let Some(p) = c.random_inside(rng, 1.5) {
                fish_crates(g, c, rng, p, 2);
            }
        }
        return;
    };
    // Columns round the edge.
    let col = if t == 0 { mul(pal.wood, 0.8) } else { mul(pal.steel, 0.9) };
    let nu = ((r.w() / 5.0).ceil() as i32).max(1);
    let nv = ((r.d() / 5.0).ceil() as i32).max(1);
    for i in 0..=nu {
        for j in 0..=nv {
            if i != 0 && i != nu && j != 0 && j != nv && (i + j) % 2 == 1 {
                continue;
            }
            let p = Vec2::new(r.u0 + 0.2 + i as f32 * (r.w() - 0.4) / nu as f32, r.v0 + 0.2 + j as f32 * (r.d() - 0.4) / nv as f32);
            f.bx(g, if t == 0 { Wood } else { Metal }, p.x, h * 0.5, p.y, Vec3::new(0.25, h, 0.25), col);
        }
    }
    match t {
        0 => {
            // Patchwork tarps.
            for i in 0..nu {
                let u = r.u0 + (i as f32 + 0.5) * r.w() / nu as f32;
                f.bx(g, Tarp, u, h + 0.05 * (i % 2) as f32, r.c().y, Vec3::new(r.w() / nu as f32 + 0.3, 0.08, r.d() + 0.2), pick(rng, &pal.tarp_colors));
            }
        }
        1 => {
            f.bx(g, Metal, r.c().x, h, r.c().y, Vec3::new(r.w() + 0.4, 0.3, r.d() + 0.4), mul(pick(rng, &pal.metal_paint), 0.8));
            super::industrial::gable_roof(g, c, &r.poly(), h + 0.15, (r.d() * 0.12).clamp(0.8, 2.5), mul(pal.steel, 0.75));
        }
        _ => {
            // A glazed vault in strips over brick gable ends.
            for k in 0..7 {
                let s = (k as f32 + 0.5) / 7.0;
                let v = r.v0 + r.d() * s;
                let y = h + 2.0 * (1.0 - (2.0 * s - 1.0).powi(2));
                f.bx(g, if k % 2 == 0 { Metal } else { Glass }, r.c().x, y, v, Vec3::new(r.w(), 0.25, r.d() / 7.0 + 0.05), if k % 2 == 0 { mul(pal.steel, 1.05) } else { pick(rng, &pal.glass) });
            }
            for face in [r.left(), r.right()] {
                face.bx(g, f, Brick, 0.0, h * 0.5 + 0.4, -0.2, Vec3::new(face.len, h + 0.8, 0.4), mul(pick(rng, &pal.brick), 1.0));
            }
        }
    }
    // Stalls in rows: counter, ice tray, fish, an awning, a lamp.
    let (sw, sd) = (3.0, 4.5);
    let nx = ((r.w() - 1.0) / sw).floor() as i32;
    let ny = ((r.d() - 1.0) / sd).floor() as i32;
    let mut stalls = 0;
    for j in 0..ny {
        for i in 0..nx {
            if stalls >= 18 || (j % 2 == 1 && i % 3 == 1) {
                continue;
            }
            let p = Vec2::new(r.u0 + 0.5 + (i as f32 + 0.5) * (r.w() - 1.0) / nx as f32, r.v0 + 0.5 + (j as f32 + 0.5) * (r.d() - 1.0) / ny as f32);
            f.bx(g, if t == 2 { Panel } else { Wood }, p.x, 0.45, p.y, Vec3::new(sw - 0.6, 0.9, 1.2), if t == 2 { [0.85, 0.86, 0.85] } else { pal.wood });
            f.bx(g, Matte, p.x, 0.93, p.y, Vec3::new(sw - 0.8, 0.08, 1.0), [0.82, 0.9, 0.95]);
            for k in 0..4 {
                let fish = if k % 3 == 2 { [0.85, 0.5, 0.45] } else { [0.7, 0.74, 0.78] };
                f.bx(g, Metal, p.x - (sw - 1.4) * 0.5 + k as f32 * (sw - 1.4) / 3.0, 1.0, p.y + rng.random_range(-0.2..0.2), Vec3::new(0.22, 0.08, 0.6), fish);
            }
            let tc = mul(pick(rng, &pal.awning_colors), 1.0 - 0.3 * c.wear);
            f.bx(g, Tarp, p.x, 2.3, p.y - 0.4, Vec3::new(sw - 0.4, 0.06, 1.6), tc);
            f.bx(g, Glow, p.x, h - 0.4, p.y, Vec3::new(0.3, 0.2, 0.3), pal.lamp);
            stalls += 1;
        }
    }
    // Crates on the forecourt, awnings and the sign on the street side.
    let fr = r.front();
    for k in 0..[3, 4, 5][t] {
        let p = fr.at(-fr.len * 0.4 + k as f32 * fr.len * 0.2, 1.2);
        { let n = rng.random_range(1..4); fish_crates(g, c, rng, p, n); }
    }
    sign_board(g, c, rng, &fr, 0.0, h - 0.6, (fr.len * 0.4).min(10.0), 0.7);
    fish_sign(g, c, &fr, fr.len * 0.35, h - 0.6, 0.3, 1.4, [0.3, 0.9, 1.3]);
}

// ── seafood restaurant ───────────────────────────────────────────────────────

/// A restaurant with a timber deck out front: tables under umbrellas,
/// string lights between posts, warm windows and a glowing fish sign. A
/// weathered clapboard shack when cheap, white render in the middle, glass
/// and a rooftop terrace when dear.
pub fn seafood_restaurant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.9));
    let area = R::of(&c.poly);
    let deck_d = (area.d() * 0.35).clamp(3.5, 9.0);
    let want = R::new(area.u0, area.u1, area.v0 + deck_d, area.v1.min(area.v0 + deck_d + 16.0));
    let floors = if t == 2 { 2 } else { 1 };
    let fh = 3.6;
    let h = floors as f32 * fh;
    let (s, wall) = match t {
        0 => (WornPlaster, mix([0.3, 0.5, 0.62], pick(rng, &pal.plaster), 0.3)),
        1 => (Plaster, [0.88, 0.87, 0.84]),
        _ => (Panel, mix([0.25, 0.3, 0.36], pick(rng, &pal.panel), 0.3)),
    };
    let deck_v1;
    if let Some(r) = fit(c, want, 4.0) {
        deck_v1 = r.v0;
        block(g, c, s, &r, 0.0, h, wall);
        if t < 2 {
            super::industrial::gable_roof(g, c, &r.poly(), h, (r.d() * 0.18).clamp(1.0, 3.0), if t == 0 { mul(pal.rust, 0.9) } else { [0.2, 0.3, 0.45] });
        } else {
            parapet(g, c, &r.poly(), h, 1.0, 0.15, Glass, pick(rng, &pal.glass));
        }
        let fr = r.front();
        // Big warm windows and a door.
        let n = ((fr.len - 2.0) / 2.6).floor().max(1.0) as i32;
        for i in 0..n {
            let tt = -fr.len * 0.5 + 1.0 + (i as f32 + 0.5) * (fr.len - 2.0) / n as f32;
            if i == n / 2 {
                fr.decal(g, f, Wood, tt, 1.1, 0.05, 1.2, 2.2, mul(pal.wood, 0.7));
                continue;
            }
            pane(g, c, rng, &fr, tt, 1.6, 1.8, 1.8, true);
        }
        if floors > 1 {
            pane_row(g, c, rng, &fr, fh, 1, fh, 2.6, 2.0, 1.8, true);
        }
        for face in [r.left(), r.right()] {
            pane_row(g, c, rng, &face, 0.0, 1, fh, 3.0, 1.4, 1.4, true);
        }
        // The fish sign over the door, and the name.
        let accent = [[1.4, 0.6, 0.3], [0.3, 0.9, 1.4], [1.3, 0.4, 0.8]][rng.random_range(0..3usize)];
        fish_sign(g, c, &fr, 0.0, h - 0.8, 0.35, 2.0, accent);
        sign_board(g, c, rng, &fr, -fr.len * 0.3, h - 0.8, (fr.len * 0.3).min(5.0), 0.5);
        if t == 0 {
            // Lobster pots and a lifebuoy by the door.
            for k in 0..3 {
                let p = fr.at(fr.len * 0.4 - k as f32 * 0.9, 0.6);
                f.bx(g, Wood, p.x, 0.3 + (k % 2) as f32 * 0.6, p.y, Vec3::new(0.8, 0.55, 0.6), mul(pal.wood, 0.8));
            }
            fr.decal(g, f, Matte, fr.len * 0.35, 2.2, 0.06, 0.7, 0.7, [0.9, 0.3, 0.15]);
        }
        roof_gear(g, c, rng, &r.poly(), h, 0.2);
    } else {
        deck_v1 = area.v1;
    }
    // The deck: boards on joists a step up, a rail, tables and lights.
    let deck = fit(c, R::new(area.u0 + 0.2, area.u1 - 0.2, area.v0 + 0.3, deck_v1 - 0.05), 2.0);
    if let Some(deck) = deck.filter(|d| d.w() > 3.0) {
        let dy = 0.45;
        block(g, c, Wood, &deck, 0.0, dy, mul(pal.wood, [0.7, 0.95, 1.05][t]));
        for face in [deck.front(), deck.left(), deck.right()] {
            face.bx(g, f, Wood, 0.0, dy + 0.9, -0.05, Vec3::new(face.len, 0.08, 0.08), mul(pal.wood, 0.8));
            let posts = (face.len / 2.0) as i32;
            for k in 0..=posts {
                face.bx(g, f, Wood, -face.len * 0.5 + k as f32 * face.len / posts.max(1) as f32, dy + 0.45, -0.05, Vec3::new(0.08, 0.9, 0.08), mul(pal.wood, 0.75));
            }
        }
        let umb = [Some([0.85, 0.2, 0.15]), Some([0.92, 0.9, 0.85]), Some([0.15, 0.3, 0.55])][t];
        let nx = ((deck.w() - 1.0) / 3.2).floor() as i32;
        let ny = ((deck.d() - 1.0) / 2.8).floor() as i32;
        for i in 0..nx.min(8) {
            for j in 0..ny.min(3) {
                let p = Vec2::new(deck.u0 + 0.5 + (i as f32 + 0.5) * (deck.w() - 1.0) / nx as f32, deck.v0 + 0.5 + (j as f32 + 0.5) * (deck.d() - 1.0) / ny as f32);
                table(g, f, p, dy, mul(pal.wood, 1.1), if (i + j) % 2 == 0 { umb } else { None });
            }
        }
        // String lights between corner posts and the building.
        let posts = [Vec2::new(deck.u0 + 0.2, deck.v0 + 0.2), Vec2::new(deck.u1 - 0.2, deck.v0 + 0.2)];
        for p in posts {
            f.bx(g, Wood, p.x, 1.8, p.y, Vec3::new(0.15, 3.6, 0.15), mul(pal.wood, 0.7));
        }
        let warm = [1.6, 1.1, 0.55];
        string_lights(g, f, Vec3::new(posts[0].x, 3.5, posts[0].y), Vec3::new(posts[1].x, 3.5, posts[1].y), 14, warm);
        for (k, p) in posts.iter().enumerate() {
            let to = Vec3::new(if k == 0 { deck.u0 + deck.w() * 0.3 } else { deck.u1 - deck.w() * 0.3 }, 3.2, deck_v1);
            string_lights(g, f, Vec3::new(p.x, 3.5, p.y), to, 8, warm);
        }
    }
}

// ── golf driving range ───────────────────────────────────────────────────────

/// A two-storey tee building along the street with its bays facing in, the
/// rest a mown field with target greens, flags and distance boards, tall
/// poles round it hung with netting, and floodlight masts that glow at
/// night. A single storey of bays under a tin roof when cheap.
pub fn golf_range(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.4));
    let area = R::of(&c.poly);
    let bd = (area.d() * 0.18).clamp(5.0, 7.5);
    let storeys = if t == 0 { 1 } else { 2 };
    let fh = 3.2;
    let h = storeys as f32 * fh;
    let field_v0;
    let wall = if t == 2 { [0.85, 0.86, 0.88] } else { pick(rng, &pal.panel) };
    if let Some(r) = fit(c, R::new(area.u0, area.u1, area.v0, area.v0 + bd), 4.0) {
        field_v0 = r.v1;
        // Service strip on the street side, the bays behind it.
        let svc = R::new(r.u0, r.u1, r.v0, r.v0 + (r.d() * 0.35).max(1.8));
        block(g, c, if t == 0 { WornPanel } else { Panel }, &svc, 0.0, h, wall);
        let fr = svc.front();
        entrance(g, c, rng, &fr, -fr.len * 0.25, 2.4, 2.6);
        sign_board(g, c, rng, &fr, fr.len * 0.1, h - 0.7, (fr.len * 0.4).min(12.0), 0.8);
        if storeys > 1 {
            pane_row(g, c, rng, &fr, fh, 1, fh, 3.0, 2.0, 1.2, false);
        }
        let bays = R::new(r.u0, r.u1, svc.v1, r.v1);
        for k in 1..=storeys {
            f.bx(g, Concrete, bays.c().x, k as f32 * fh - 0.15, bays.c().y, Vec3::new(bays.w(), 0.3, bays.d() + 0.3), mul(pal.concrete(0), 1.0));
        }
        let n = ((bays.w() / 3.0).floor() as i32).max(1);
        let bw = bays.w() / n as f32;
        for k in 0..=n {
            let u = crate::mesh::clamp_safe(bays.u0 + k as f32 * bw, bays.u0 + 0.1, bays.u1 - 0.1);
            f.bx(g, Matte, u, h * 0.5, bays.c().y, Vec3::new(0.15, h, bays.d()), mul(wall, 0.85));
        }
        for s in 0..storeys {
            let y = s as f32 * fh;
            for k in 0..n {
                let u = bays.u0 + (k as f32 + 0.5) * bw;
                f.bx(g, Foliage, u, y + 0.05, bays.c().y + 0.3, Vec3::new(1.4, 0.06, 1.4), [0.15, 0.45, 0.2]);
                f.bx(g, Matte, u + 0.9, y + 0.3, bays.c().y - 0.6, Vec3::new(0.4, 0.6, 0.4), [0.1, 0.3, 0.6]);
                f.bx(g, Glow, u, y + fh - 0.33, bays.c().y, Vec3::new(1.2, 0.05, 0.3), mul(pal.lamp, 1.1));
            }
        }
        f.bx(g, Metal, r.c().x, h + 0.15, r.c().y, Vec3::new(r.w() + 0.4, 0.3, r.d() + 0.6), mul(pal.steel, 0.6));
        if t == 2 {
            kit_glow_band(g, c, &bays.back(), h - 0.1);
        }
    } else {
        field_v0 = area.v0 + bd;
    }
    // The field: mown stripes, target greens with flags, distance boards.
    let field = R::new(area.u0, area.u1, field_v0, area.v1);
    let fpoly = super::kit::clip_box(&offset_poly(&c.lot_poly, 0.5), field.u0 - 2.0, field.u1 + 2.0, field.v0, field.v1 + 2.0);
    if !fpoly.is_empty() {
        ground(g, f, Foliage, &fpoly, 0.05, [0.13, 0.36, 0.14]);
        let mut v = field.v0;
        let mut k = 0;
        while v < field.v1 && k < 30 {
            if k % 2 == 1 {
                let strip = super::kit::clip_box(&fpoly, field.u0 - 2.0, field.u1 + 2.0, v, (v + 6.0).min(field.v1));
                if !strip.is_empty() {
                    ground(g, f, Foliage, &strip, 0.07, [0.16, 0.42, 0.17]);
                }
            }
            v += 6.0;
            k += 1;
        }
        for i in 0..rng.random_range(3..6) {
            let s = 0.3 + 0.6 * (i as f32 + rng.random::<f32>()) / 5.0;
            let p = Vec2::new(field.u0 + field.w() * rng.random_range(0.2..0.8), field.v0 + field.d() * s);
            let rr = rng.random_range(2.0..4.0) * (0.6 + s);
            let green = ngon(p.x, p.y, rr, 10, 0.0);
            if poly_inside(c, &green) {
                ground(g, f, Foliage, &green, 0.09, [0.3, 0.62, 0.25]);
                ground(g, f, Paint, &ngon(p.x, p.y, rr * 0.25, 8, 0.0), 0.1, [0.85, 0.85, 0.8]);
                f.bx(g, Matte, p.x, 1.2, p.y, Vec3::new(0.05, 2.4, 0.05), [0.9, 0.9, 0.9]);
                f.bx(g, Tarp, p.x + 0.35, 2.15, p.y, Vec3::new(0.7, 0.45, 0.03), [0.9, 0.1, 0.1]);
            }
        }
        let mut yards = 50;
        let mut v = field.v0 + 20.0;
        while v < field.v1 - 4.0 {
            for u in [field.u0 + 2.5, field.u1 - 2.5] {
                let p = Vec2::new(u, v);
                if c.inside_local(p) {
                    f.bx(g, Matte, p.x, 0.6, p.y, Vec3::new(0.1, 1.2, 0.1), CHARCOAL);
                    f.bx(g, Matte, p.x, 1.5, p.y, Vec3::new(1.4, 0.9, 0.08), [0.92, 0.92, 0.9]);
                    let face = Face::new(p + Vec2::new(-0.7, -0.05), p + Vec2::new(0.7, -0.05), Vec2::new(0.0, -1.0));
                    let digits = if yards >= 100 { 3 } else { 2 };
                    for d in 0..digits {
                        face.decal(g, f, Glow, -0.3 + d as f32 * 0.3, 1.5, 0.01, 0.22, 0.55, if yards % 100 == 0 { [1.4, 0.3, 0.2] } else { [0.2, 0.25, 0.3] });
                    }
                }
            }
            yards += 50;
            v += 22.0;
        }
        // Net poles round the sides and the back, nets strung between.
        let ph = [12.0, 18.0, 24.0][t];
        let corners = [Vec2::new(field.u0 + 0.6, field.v0 + 0.5), Vec2::new(field.u0 + 0.6, field.v1 - 0.6), Vec2::new(field.u1 - 0.6, field.v1 - 0.6), Vec2::new(field.u1 - 0.6, field.v0 + 0.5)];
        let net = [0.12, 0.16, 0.13];
        let mut poles = 0;
        for side in 0..3 {
            let (a, b) = (corners[side], corners[side + 1]);
            let len = a.distance(b);
            let spans = ((len / 12.0).ceil() as i32).max(1);
            for k in 0..spans {
                let pa = a.lerp(b, k as f32 / spans as f32);
                let pb = a.lerp(b, (k + 1) as f32 / spans as f32);
                if !c.inside_local(pa) || !c.inside_local(pb) || poles > 40 {
                    continue;
                }
                f.bx(g, Metal, pa.x, ph * 0.5, pa.y, Vec3::new(0.35, ph, 0.35), mul(pal.steel, 0.7));
                poles += 1;
                let mid = (pa + pb) * 0.5;
                let span = pa.distance(pb);
                let yaw = (pb - pa).y.atan2((pb - pa).x);
                for row in 0..5 {
                    let y = ph * (0.15 + 0.2 * row as f32);
                    f.bx_yaw(g, Matte, mid.x, y, mid.y, Vec3::new(span, 0.05, 0.03), net, yaw);
                }
                let threads = (span / 3.0) as i32;
                for th in 1..threads {
                    let q = pa.lerp(pb, th as f32 / threads as f32);
                    f.bx(g, Matte, q.x, ph * 0.5, q.y, Vec3::new(0.03, ph * 0.9, 0.03), net);
                }
            }
            if c.inside_local(b) && side == 2 {
                f.bx(g, Metal, b.x, ph * 0.5, b.y, Vec3::new(0.35, ph, 0.35), mul(pal.steel, 0.7));
            }
        }
        // Floodlight masts on the field's front corners and sides, aimed in.
        let mh = [9.0, 12.0, 15.0][t];
        let masts = [Vec2::new(field.u0 + 1.5, field.v0 + 1.5), Vec2::new(field.u1 - 1.5, field.v0 + 1.5), Vec2::new(field.u0 + 1.5, field.c().y), Vec2::new(field.u1 - 1.5, field.c().y)];
        for (k, p) in masts.iter().enumerate() {
            if k >= 2 && t == 0 {
                break;
            }
            let aim = (field.c() + Vec2::new(0.0, field.d() * 0.2) - *p).normalize_or_zero();
            mast_light(g, c, *p, mh, aim);
        }
    }
}

/// A thin glowing band along a face at height `y`.
fn kit_glow_band(g: &mut Geometry, c: &Ctx, face: &Face, y: f32) {
    face.decal(g, &c.frame, Glow, 0.0, y, 0.06, face.len * 0.96, 0.12, mul(c.palette.lamp, 1.2));
}

// ── artists' studios ─────────────────────────────────────────────────────────

/// A converted brick warehouse: big multi-pane industrial windows, murals
/// in bright colours down its walls, a roller door open on a studio,
/// sculptures in the yard. Rough and tagged when cheap, polished with
/// planters and a sawtooth of rooflights when dear.
pub fn art_studio(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, if t == 0 { Dirt } else { Paving }, if t == 0 { mix(pal.dirt, pal.paving, 0.5) } else { mul(pal.paving, 1.0) });
    let area = R::of(&c.poly);
    let yard_d = (area.d() * 0.3).clamp(3.0, 10.0);
    let floors = c.floors.clamp(2, 3) as i32;
    let fh = 4.2;
    let h = floors as f32 * fh;
    let brick = mul(pick(rng, &pal.brick), [0.8, 0.95, 1.05][t]);
    let mut yard_r = R::new(area.u0, area.u1, area.v0, area.v0 + yard_d);
    if let Some(r) = fit(c, R::new(area.u0, area.u1, area.v0 + yard_d, area.v1), 5.0) {
        yard_r = R::new(area.u0, area.u1, area.v0, r.v0);
        block(g, c, if t == 0 { WornBrick } else { Brick }, &r, 0.0, h, brick);
        cornice(g, c, &r.poly(), h, 0.4, 0.15, Brick, mul(brick, 0.85));
        // Roof: a sawtooth of north lights, or a gable.
        if t > 0 && r.w() > 8.0 {
            let n = ((r.d() / 5.0) as i32).clamp(1, 6);
            let step = r.d() / n as f32;
            for k in 0..n {
                let v = r.v0 + (k as f32 + 0.5) * step;
                let bottom = rect(r.c().x, v, r.w() * 0.5, step * 0.5);
                let top = vec![Vec2::new(r.u0, v + step * 0.3), Vec2::new(r.u1, v + step * 0.3), Vec2::new(r.u1, v + step * 0.5), Vec2::new(r.u0, v + step * 0.5)];
                loft(g, f, Metal, &bottom, &top, h, h + 2.0, mul(pal.steel, 0.7));
                let glass = Face::new(Vec2::new(r.u0, v + step * 0.4), Vec2::new(r.u1, v + step * 0.4), Vec2::new(0.0, 1.0));
                glass.decal(g, f, Glass, 0.0, h + 1.0, 0.12, r.w() * 0.95, 1.7, pick(rng, &pal.glass));
            }
        } else {
            super::industrial::gable_roof(g, c, &r.poly(), h, (r.d() * 0.15).clamp(1.0, 3.5), mul(pal.steel, 0.55));
        }
        // Big steel-framed windows: panes in a grid, some lit.
        for (fi, face) in r.faces().iter().enumerate() {
            let side = fi % 2 == 1;
            let n = ((face.len - 2.0) / 4.5).floor() as i32;
            let step = if n > 0 { (face.len - 2.0) / n as f32 } else { 0.0 };
            for fl in 0..floors {
                let y = fl as f32 * fh + fh * 0.55;
                for i in 0..n {
                    // Murals take the middle of the side walls.
                    if side && i > 0 && i < n - 1 {
                        continue;
                    }
                    let tt = -(face.len - 2.0) * 0.5 + (i as f32 + 0.5) * step;
                    let (w, wh) = (step * 0.7, fh * 0.65);
                    face.decal(g, f, Matte, tt, y, 0.03, w + 0.3, wh + 0.3, mul(DARK, 1.4));
                    let lit = coin(rng, c.lit_chance());
                    for gi in 0..3 {
                        let gt = tt - w / 3.0 + gi as f32 * w / 3.0;
                        if lit {
                            let col = mul(pal.window_warm, rng.random_range(0.55..0.9));
                            face.glaze(g, c, gt, y, 0.05, w / 3.0 - 0.1, wh - 0.1, Pane::Lit(col));
                        } else {
                            face.glaze(g, c, gt, y, 0.05, w / 3.0 - 0.1, wh - 0.1, Pane::Dark);
                        }
                    }
                    face.decal(g, f, Matte, tt, y + wh * 0.15, 0.07, w, 0.06, mul(DARK, 1.4));
                    face.decal(g, f, Matte, tt, y - wh * 0.2, 0.07, w, 0.06, mul(DARK, 1.4));
                }
            }
            if side {
                mural(g, c, rng, face, 0.3, h - 0.6);
            }
        }
        // A mural on the street face too, beside the door.
        let fr = r.front();
        let door_t = -fr.len * 0.3;
        roller_door(g, c, &fr, door_t, 4.0, 3.6, mul(MURAL[rng.random_range(0..MURAL.len())], 0.8));
        sign_board(g, c, rng, &fr, fr.len * 0.15, fh - 0.3, (fr.len * 0.3).min(6.0), 0.5);
        if t == 0 {
            graffiti(g, c, rng, &fr, 0.1, 2.8, 1.8);
            fire_escape(g, c, rng, &r.right(), 0.0, 0.0, floors, fh);
        }
        roof_gear(g, c, rng, &r.poly(), h, 0.25);
    }
    // Sculptures and seats in the yard; string lights over it.
    let n = [2, 2, 3][t];
    for k in 0..n {
        let p = Vec2::new(yard_r.u0 + (k as f32 + 0.5) * yard_r.w() / n as f32, yard_r.c().y);
        sculpture(g, c, rng, p, [0.8, 1.0, 1.3][t]);
    }
    if t == 2 {
        for k in 0..2 {
            let p = Vec2::new(yard_r.u0 + 1.5 + k as f32 * (yard_r.w() - 3.0), yard_r.c().y);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.5 {
                tree_in(g, f, pal, p, 4.0, 1.4, pal.foliage);
            }
        }
    } else if yard_r.d() > 3.0 {
        let a = Vec3::new(yard_r.u0 + 0.5, 3.6, yard_r.v0 + 0.6);
        let b = Vec3::new(yard_r.u1 - 0.5, 3.6, yard_r.v1 - 0.2);
        if c.inside_local(Vec2::new(a.x, a.z)) && c.inside_local(Vec2::new(b.x, b.z)) {
            f.bx(g, Wood, a.x, 1.8, a.z, Vec3::new(0.15, 3.6, 0.15), mul(pal.wood, 0.7));
            string_lights(g, f, a, b, 12, [1.5, 1.0, 0.6]);
        }
    }
}

// ── gallery ──────────────────────────────────────────────────────────────────

/// A white cube with a full-height glass front lit warm inside, a tall
/// banner down the facade, a sculpture on the forecourt and uplights. A
/// shopfront gallery when cheap; a larger cube with a cantilevered upper
/// box when dear.
pub fn gallery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Paving, mul(pal.paving, [0.95, 1.15, 1.3][t]));
    let area = R::of(&c.poly);
    let gw = area.w().min([9.0, 15.0, 22.0][t]);
    let court = (area.d() * 0.3).clamp(3.0, 9.0);
    let gd = (area.d() - court).min([10.0, 14.0, 18.0][t]);
    let h = [4.5, 6.5, 8.0][t];
    let white = [0.92, 0.92, 0.9];
    let warm = [1.4, 1.05, 0.7];
    let banner = MURAL[rng_for(c.seed, 0x6A11).random_range(0..MURAL.len() - 1)];
    let want = R::new(area.c().x - gw * 0.5, area.c().x + gw * 0.5, area.v0 + court, area.v0 + court + gd);
    let mut forecourt = R::new(area.u0, area.u1, area.v0, area.v0 + court);
    if let Some(r) = fit(c, want, 4.0) {
        forecourt = R::new(r.u0, r.u1, area.v0, r.v0);
        block(g, c, if t == 0 { WornPlaster } else { Plaster }, &r, 0.0, h, white);
        let fr = r.front();
        // The glass front, lit warm from inside.
        let gl = fr.len * [0.6, 0.7, 0.8][t];
        let gt = -fr.len * 0.5 + 0.6 + gl * 0.5;
        fr.glaze(g, c, gt, (h - 0.8) * 0.5, 0.04, gl, h - 1.0, Pane::Dark);
        let lit = mul(warm, 0.55 + 0.4 * c.lit_chance());
        fr.glaze(g, c, gt, (h - 0.8) * 0.5, 0.06, gl * 0.96, h - 1.2, Pane::Lit(lit));
        // Mullions and a dark frame.
        for k in 0..=((gl / 2.5) as i32).max(1) {
            let tt = gt - gl * 0.5 + k as f32 * gl / ((gl / 2.5) as i32).max(1) as f32;
            fr.decal(g, f, Matte, tt, (h - 0.8) * 0.5, 0.08, 0.08, h - 1.0, CHARCOAL);
        }
        // Banner down the solid part of the facade.
        let bt = gt + gl * 0.5 + (fr.len * 0.5 - gt - gl * 0.5) * 0.5;
        fr.decal(g, f, Tarp, bt, h * 0.5, 0.1, (fr.len * 0.5 - gt - gl * 0.5 - 0.4).clamp(0.4, 1.8), h * 0.75, mul(banner, 1.0 - 0.4 * c.wear));
        fr.bx(g, f, Matte, bt, h * 0.88, 0.12, Vec3::new(2.0, 0.08, 0.08), CHARCOAL);
        // Name in dark letters over the glass, with a warm wash below it.
        let letters = ((gl / 0.8) as i32).clamp(3, 9);
        for k in 0..letters {
            fr.decal(g, f, Matte, gt - gl * 0.3 + k as f32 * gl * 0.6 / letters as f32, h - 0.45, 0.06, 0.35, 0.4, CHARCOAL);
        }
        fr.decal(g, f, Glow, gt, h - 0.75, 0.05, gl * 0.7, 0.06, mul(warm, 0.8));
        // Uplights washing the white walls.
        for face in r.faces() {
            let n = (face.len / 3.0) as i32;
            for k in 0..n {
                face.bx(g, f, Glow, -face.len * 0.5 + (k as f32 + 0.5) * face.len / n as f32, 0.08, 0.15, Vec3::new(0.3, 0.12, 0.2), mul(warm, 0.9));
            }
        }
        if t == 2 {
            // A cantilevered box over the forecourt.
            let up = R::new(r.u0 + r.w() * 0.3, r.u1 + 0.0, r.v0 - (court * 0.5).min(3.0), r.v0 + r.d() * 0.6);
            if poly_inside(c, &up.poly()) {
                block(g, c, Plaster, &up, h, h + 4.0, white);
                let uf = up.front();
                uf.glaze(g, c, 0.0, h + 2.0, 0.04, uf.len * 0.8, 1.2, Pane::Dark);
                uf.glaze(g, c, 0.0, h + 2.0, 0.06, uf.len * 0.78, 1.0, Pane::Lit(mul(warm, 0.6)));
            } else {
                block(g, c, Plaster, &r.inset(r.w().min(r.d()) * 0.2), h, h + 3.0, white);
            }
        } else if t == 0 {
            grime(g, c, rng, &fr, 0.0, h, 0.6);
        }
    }
    // The sculpture on the forecourt, a bench, a lamp.
    let p = Vec2::new(forecourt.u0 + forecourt.w() * 0.7, forecourt.c().y);
    sculpture(g, c, rng, p, [0.8, 1.1, 1.5][t]);
    let q = Vec2::new(forecourt.u0 + forecourt.w() * 0.25, forecourt.c().y);
    if c.inside_local(q) && t > 0 {
        let w = f.to_world(q.x, q.y);
        bench_world(g, w, f.yaw(), mul(pal.wood, 1.1), pal.steel);
    }
}
