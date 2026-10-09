//! [economy] The land side of the waterfront docks (`nd_sim::ports::PortKind`):
//! a small marina's boathouse, trailers and car park; a large marina's
//! clubhouse with its terrace, a yacht on a cradle and flagpoles; a small
//! fishing port's quay shed, nets and crates; a large one's ice house, quay
//! crane, big shed and stacks of crates. The water side (pontoons, quays
//! and the moored boats) is drawn with the piers in `terrain::waterworks`.
//! The lot's street side is `v0`; the water lies past `v1`.

use super::parts::*;
use super::trades::{block, fish_crates, fit, pane, pane_row, table, R};
use super::{Ctx, Frame};
use crate::surface::{Geometry, Surface::*};
use crate::terrain::boat_mesh;
use glam::{Vec2, Vec3};
use nd_sim::ShipKind;
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

/// A boat standing on the lot at `p` (lot-local), bow along `yaw` (frame
/// relative), keel at height `y`.
fn boat_on_land(g: &mut Geometry, c: &Ctx, kind: ShipKind, p: Vec2, y: f32, yaw: f32) {
    let f = &c.frame;
    let mesh = boat_mesh(kind, c.palette);
    let w = f.to_world(p.x, p.y);
    // The keel sits about a metre under the waterline.
    g.part(Panel).append_transformed(&mesh, Vec3::new(w.x, y + 1.0, w.y), f.yaw() + yaw);
}

/// A boat trailer: frame, wheels and a small boat on it.
fn trailer(g: &mut Geometry, c: &Ctx, p: Vec2, yaw: f32) {
    let f = &c.frame;
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    f.bx_yaw(g, Metal, p.x, 0.55, p.y, Vec3::new(6.5, 0.12, 1.6), mul(c.palette.steel, 0.9), yaw);
    for s in [-1.0f32, 1.0] {
        let q = p + n * s * 0.95;
        f.bx_yaw(g, Matte, q.x, 0.33, q.y, Vec3::new(0.66, 0.66, 0.22), DARK, yaw);
    }
    let hitch = p + d * 3.7;
    f.bx_yaw(g, Metal, hitch.x, 0.45, hitch.y, Vec3::new(1.2, 0.1, 0.1), CHARCOAL, yaw);
    boat_on_land(g, c, ShipKind::Sailboat, p, 0.0, yaw);
}

/// A car park strip with cars in some of its bays.
fn parking_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, share: f32, colors: &[[f32; 3]]) {
    let f = &c.frame;
    let n = ((r.w() / 2.6).floor() as i32).max(0);
    for k in 0..n {
        let u = r.u0 + (k as f32 + 0.5) * r.w() / n as f32;
        f.flat(g, Paint, u - 1.3, 0.05, r.c().y, 0.1, 4.6, 0.0, c.palette.lane_marker);
        let p = Vec2::new(u, r.c().y);
        if coin(rng, share) && vehicle_fits(c, p, FRAC_PI_2, Vehicle::Car) {
            let col = pick(rng, colors);
            vehicle(g, f, c.palette, rng, p, 0.0, FRAC_PI_2, Vehicle::Car, col);
        }
    }
}

/// A run of bollards along the quay edge at the back of the lot.
fn quay_edge(g: &mut Geometry, c: &Ctx, area: &R, color: [f32; 3]) {
    let f = &c.frame;
    let n = (area.w() / 6.0) as i32;
    for k in 0..=n {
        let p = Vec2::new(area.u0 + 0.5 + k as f32 * (area.w() - 1.0) / n.max(1) as f32, area.v1 - 0.6);
        if c.inside_local(p) {
            let w = f.to_world(p.x, p.y);
            g.part(Metal).cylinder(w, 0.22, 0.28, 0.0, 0.6, 8, rgba(color));
        }
    }
}

/// Flagpoles in a row with flags.
fn flags(g: &mut Geometry, f: &Frame, a: Vec2, b: Vec2, n: i32, h: f32) {
    let cols = [[0.1, 0.2, 0.6], [0.9, 0.9, 0.9], [0.8, 0.1, 0.1], [0.95, 0.75, 0.1]];
    for k in 0..n {
        let p = a.lerp(b, if n > 1 { k as f32 / (n - 1) as f32 } else { 0.5 });
        super::trades::flagpole(g, f, p, h, cols[k as usize % cols.len()], false);
    }
}

// ── marinas ──────────────────────────────────────────────────────────────────

/// A boathouse with the harbour office in it, a slipway down to the water,
/// boats on trailers and a small car park.
pub fn marina_small(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.0));
    let area = R::of(&c.poly);
    // Boathouse on the water side, big doors to the slip.
    let bw = (area.w() * 0.35).clamp(6.0, 14.0);
    let bd = (area.d() * 0.4).clamp(5.0, 10.0);
    let wall = [[0.25, 0.4, 0.55], [0.85, 0.85, 0.82], [0.3, 0.42, 0.5]][t];
    if let Some(r) = fit(c, R::new(area.u0, area.u0 + bw, area.v1 - bd, area.v1), 4.0) {
        let h = 4.5;
        block(g, c, Wood, &r, 0.0, h, wall);
        super::industrial::gable_roof(g, c, &r.poly(), h, (r.d() * 0.25).clamp(1.2, 2.8), [0.2, 0.22, 0.25]);
        let back = r.back();
        back.decal(g, f, Wood, 0.0, 1.8, 0.05, (back.len * 0.6).min(5.0), 3.6, mul(pal.wood, 0.7));
        let fr = r.front();
        pane_row(g, c, rng, &fr, 0.0, 1, 3.2, 2.4, 1.4, 1.1, true);
        super::parts::sign_board(g, c, rng, &fr, 0.0, h - 0.5, (fr.len * 0.6).min(6.0), 0.5);
        // The slipway down to the water beside it.
        let slip = fit(c, R::new(r.u1 + 0.5, (r.u1 + 4.5).min(area.u1), r.v0 + 1.0, area.v1 + 0.3), 2.0);
        if let Some(slip) = slip {
            ground(g, f, Concrete, &slip.poly(), 0.06, mul(pal.concrete(0), 0.85));
        }
    }
    // Boats on trailers in a row.
    let n = [2, 3, 4][t];
    for k in 0..n {
        let p = Vec2::new(area.u1 - 2.0 - k as f32 * 2.6, area.v0 + area.d() * 0.55);
        if poly_inside(c, &rect_yaw(p.x, p.y, 1.3, 4.5, 0.0)) {
            trailer(g, c, p, FRAC_PI_2);
        }
    }
    // Car park on the street side.
    let park = R::new(area.u0 + 1.0, area.u1 - 1.0, area.v0 + 0.5, area.v0 + 5.5);
    if let Some(park) = fit(c, park, 4.5) {
        parking_row(g, c, rng, &park, 0.6, &pal.car_colors);
    }
    quay_edge(g, c, &area, mul(pal.steel, 0.5));
    let front = c.front_face(&c.poly);
    lamp_post_in(g, c, front.at(0.0, -0.8), 5.0, pal.street_lamp);
    lamp_post_in(g, c, Vec2::new(area.c().x, area.v1 - 1.5), 4.5, pal.lamp);
}

/// A clubhouse with a glazed lounge and a terrace over the water, a motor
/// yacht up on a cradle, a row of flagpoles and a car park of good cars.
pub fn marina_large(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.2));
    let area = R::of(&c.poly);
    let cw = (area.w() * 0.4).clamp(10.0, 30.0);
    let cd = (area.d() * 0.35).clamp(7.0, 14.0);
    let white = [0.9, 0.9, 0.88];
    let terrace_d = 3.5;
    if let Some(r) = fit(c, R::new(area.c().x - cw * 0.5, area.c().x + cw * 0.5, area.v1 - cd - terrace_d, area.v1 - terrace_d), 5.0) {
        let fh = 3.6;
        block(g, c, Plaster, &r, 0.0, fh, white);
        // Glazed lounge upstairs, set back, with a flat roof plate.
        let up = R::new(r.u0 + 1.0, r.u1 - 1.0, r.v0 + 1.0, r.v1);
        block(g, c, Glass, &up, fh, fh * 2.0, pick(rng, &pal.glass));
        f.bx(g, Panel, r.c().x, fh * 2.0 + 0.2, up.c().y, Vec3::new(r.w(), 0.4, up.d() + 0.8), white);
        for face in [up.back(), up.front()] {
            let n = (face.len / 3.0) as i32;
            for k in 0..n {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / n as f32;
                if coin(rng, c.lit_chance()) {
                    let col = mul(pal.window_warm, 0.8);
                    window_light(g, c, |g| face.decal(g, f, Glow, tt, fh * 1.5, 0.06, face.len / n as f32 * 0.85, fh * 0.7, col));
                }
            }
        }
        pane_row(g, c, rng, &r.front(), 0.0, 1, fh, 2.8, 1.6, 1.6, true);
        super::parts::entrance(g, c, rng, &r.front(), 0.0, 2.4, 2.8);
        super::parts::sign_board(g, c, rng, &r.front(), r.w() * 0.25, fh - 0.4, (r.w() * 0.3).min(7.0), 0.5);
        // The terrace on the water side: deck, rail, tables, lights.
        let deck = R::new(r.u0, r.u1, r.v1, (r.v1 + terrace_d).min(area.v1 + 0.2));
        if let Some(deck) = fit(c, deck, 1.5) {
            block(g, c, Wood, &deck, 0.0, 0.4, mul(pal.wood, 1.05));
            let back = deck.back();
            back.bx(g, f, Glass, 0.0, 0.95, -0.1, Vec3::new(back.len, 1.0, 0.05), pick(rng, &pal.glass));
            let n = ((deck.w() - 1.0) / 3.0).floor() as i32;
            for k in 0..n.min(8) {
                let p = Vec2::new(deck.u0 + 0.5 + (k as f32 + 0.5) * (deck.w() - 1.0) / n as f32, deck.c().y);
                table(g, f, p, 0.4, mul(pal.wood, 1.2), if k % 2 == 0 { Some([0.92, 0.92, 0.9]) } else { None });
            }
            super::parts::string_lights(g, f, Vec3::new(deck.u0, 3.2, deck.v1 - 0.2), Vec3::new(deck.u1, 3.2, deck.v1 - 0.2), 16, [1.5, 1.15, 0.7]);
        }
    }
    // A yacht up on a cradle in the yard.
    let yp = Vec2::new(area.u0 + (area.w() * 0.15).max(6.0), area.c().y);
    if poly_inside(c, &rect_yaw(yp.x, yp.y, 3.5, 11.5, 0.0)) {
        for k in 0..3 {
            let v = yp.y - 7.0 + k as f32 * 7.0;
            for s in [-1.6f32, 1.6] {
                f.bx(g, Metal, yp.x + s, 1.3, v, Vec3::new(0.3, 2.6, 0.3), [0.25, 0.3, 0.6]);
            }
            f.bx(g, Metal, yp.x, 0.4, v, Vec3::new(3.6, 0.3, 0.4), [0.25, 0.3, 0.6]);
        }
        boat_on_land(g, c, ShipKind::Yacht, yp, 1.6, FRAC_PI_2);
    }
    // Flagpoles along the street, the car park.
    let front = c.front_face(&c.poly);
    flags(g, f, front.at(-front.len * 0.15, -0.6), front.at(front.len * 0.15, -0.6), [3, 4, 5][t], 9.0);
    let park = R::new(area.u0 + 1.0, area.u1 - 1.0, area.v0 + 1.0, area.v0 + 6.0);
    if let Some(park) = fit(c, park, 4.5) {
        let good = [[0.05, 0.05, 0.06], [0.9, 0.9, 0.9], [0.55, 0.57, 0.6], [0.1, 0.15, 0.35]];
        parking_row(g, c, rng, &park, 0.7, &good);
    }
    quay_edge(g, c, &area, [0.85, 0.86, 0.88]);
    for k in 0..3 {
        lamp_post_in(g, c, Vec2::new(area.u0 + (k as f32 + 0.5) * area.w() / 3.0, area.v0 + 7.0), 5.0, pal.street_lamp);
    }
}

// ── fishing ports ────────────────────────────────────────────────────────────

/// A net pile or a net hung to dry on poles.
fn nets(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 2.0 {
        return;
    }
    let col = [[0.12, 0.3, 0.2], [0.85, 0.45, 0.1], [0.15, 0.25, 0.5]][rng.random_range(0..3usize)];
    if coin(rng, 0.5) {
        f.bx_yaw(g, Tarp, p.x, 0.35, p.y, Vec3::new(2.6, 0.7, 1.8), col, rng.random_range(0.0..1.0));
        f.bx_yaw(g, Tarp, p.x + 0.3, 0.8, p.y, Vec3::new(1.6, 0.4, 1.2), mul(col, 0.85), rng.random_range(0.0..1.0));
        // Floats on top.
        for k in 0..4 {
            f.bx(g, Matte, p.x - 0.6 + k as f32 * 0.4, 1.05, p.y, Vec3::splat(0.2), [0.95, 0.75, 0.1]);
        }
    } else {
        for s in [-1.6f32, 1.6] {
            f.bx(g, Wood, p.x + s, 1.3, p.y, Vec3::new(0.15, 2.6, 0.15), mul(c.palette.wood, 0.7));
        }
        f.bx(g, Tarp, p.x, 1.6, p.y, Vec3::new(3.2, 1.8, 0.05), col);
        f.bx(g, Wood, p.x, 2.55, p.y, Vec3::new(3.4, 0.08, 0.08), mul(c.palette.wood, 0.7));
    }
}

/// A quay shed, nets drying and in piles, stacks of fish crates and the
/// harbourmaster's lamp.
pub fn fishing_port_small(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, if t == 0 { WornConcrete } else { Concrete }, mul(pal.concrete(0), 0.8));
    let area = R::of(&c.poly);
    let sw = (area.w() * 0.45).clamp(8.0, 18.0);
    let sd = (area.d() * 0.4).clamp(5.0, 10.0);
    if let Some(r) = fit(c, R::new(area.u0, area.u0 + sw, area.v1 - sd - 2.0, area.v1 - 2.0), 4.0) {
        let h = 4.5;
        let wall = mix([0.5, 0.2, 0.15], pick(rng, &pal.metal_paint), 0.4);
        super::industrial::shed(g, c, rng, &r.poly(), h, wall, if t == 0 { WornMetal } else { Metal });
        let back = r.back();
        super::parts::roller_door(g, c, &back, 0.0, (back.len * 0.4).min(4.0), 3.5, mul(pal.steel, 0.6));
        pane(g, c, rng, &r.front(), r.w() * 0.25, 1.6, 1.2, 1.0, true);
        super::parts::sign_board(g, c, rng, &r.front(), -r.w() * 0.15, h - 0.6, (r.w() * 0.4).min(5.0), 0.45);
    }
    for k in 0..[2, 3, 4][t] {
        nets(g, c, rng, Vec2::new(area.u1 - 3.0 - k as f32 * 4.5, area.v1 - 4.0));
    }
    for _ in 0..[4, 6, 8][t] {
        if let Some(p) = c.random_inside(rng, 2.0) {
            { let n = rng.random_range(1..4); fish_crates(g, c, rng, p, n); }
        }
    }
    let front = c.front_face(&c.poly);
    let van = front.at(front.len * 0.25, -2.0);
    if vehicle_fits(c, van, 0.0, Vehicle::Van) {
        vehicle(g, f, pal, rng, van, 0.0, 0.0, Vehicle::Van, [0.85, 0.88, 0.9]);
    }
    quay_edge(g, c, &area, mul(pal.steel, 0.45));
    lamp_post_in(g, c, Vec2::new(area.u1 - 1.5, area.v1 - 1.5), 5.0, pal.lamp);
}

/// An ice house with chillers and an ice chute towards the quay, a crane on
/// the quay edge, a big processing shed, stacks of crates and a forklift.
pub fn fishing_port_large(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.85));
    let area = R::of(&c.poly);
    // The big shed across the middle.
    let sw = (area.w() * 0.55).clamp(10.0, 40.0);
    let sd = (area.d() * 0.4).clamp(6.0, 16.0);
    if let Some(r) = fit(c, R::new(area.u0, area.u0 + sw, area.v0 + 6.0, area.v0 + 6.0 + sd), 5.0) {
        let h = [6.0, 7.0, 8.5][t];
        let wall = mix([0.6, 0.7, 0.75], pick(rng, &pal.panel), 0.3);
        super::industrial::shed(g, c, rng, &r.poly(), h, wall, if t == 0 { WornMetal } else { Metal });
        let back = r.back();
        for k in 0..2 {
            super::parts::roller_door(g, c, &back, -back.len * 0.25 + k as f32 * back.len * 0.5, 4.0, 4.0, mul(pal.steel, 0.6));
        }
        band_windows(g, c, rng, &r.front(), 0.5, 1, 3.0);
        super::parts::sign_board(g, c, rng, &r.front(), 0.0, h - 0.8, (r.w() * 0.4).min(10.0), 0.7);
        super::trades::fish_sign(g, c, &r.front(), r.w() * 0.35, h - 0.9, 0.3, 1.5, [0.3, 0.9, 1.4]);
    }
    // The ice house: white, chillers, a chute out to the quay.
    let iw = (area.w() * 0.18).clamp(5.0, 12.0);
    if let Some(r) = fit(c, R::new(area.u1 - iw, area.u1, area.v0 + 4.0, area.v1 - 5.0), 4.0) {
        let h = [7.0, 8.0, 10.0][t];
        block(g, c, Panel, &r, 0.0, h, [0.88, 0.9, 0.92]);
        for k in 0..2 {
            chiller(g, c, Vec2::new(r.c().x, r.v0 + (k as f32 + 0.5) * r.d() / 2.0), h, (r.w() * 0.6).min(3.0), 1.5, 0.0, mul(pal.steel, 0.9));
        }
        let a = f.at(r.c().x, h - 1.0, r.v1);
        if c.inside_local(Vec2::new(r.c().x, area.v1 - 0.3)) && distance_to_boundary(Vec2::new(r.c().x, area.v1 - 0.8), &c.lot_poly) > 0.6 {
            let b = f.at(r.c().x, 2.5, area.v1 - 0.8);
            g.part(Metal).beam(a, b, 0.7, rgba(mul(pal.steel, 0.85)));
        }
        super::parts::sign_board(g, c, rng, &r.front(), 0.0, h - 1.0, (r.w() * 0.6).min(5.0), 0.5);
    }
    // A quay crane: mast, jib out over the water, hook.
    let cp = Vec2::new(area.c().x, area.v1 - 1.5);
    if c.inside_local(cp) && distance_to_boundary(cp, &c.lot_poly) > 1.9 && c.inside_local(cp + Vec2::new((area.u1 - cp.x - 1.0).clamp(3.0, 12.0) + 0.5, 0.0)) {
        let ch = 12.0;
        let paint = [0.9, 0.65, 0.1];
        f.bx(g, Concrete, cp.x, 0.4, cp.y, Vec3::new(2.4, 0.8, 2.4), mul(pal.concrete(0), 0.9));
        f.bx(g, Metal, cp.x, ch * 0.5, cp.y, Vec3::new(0.8, ch, 0.8), paint);
        f.bx(g, Metal, cp.x, ch - 0.5, cp.y - 0.4, Vec3::new(1.6, 1.4, 2.2), mul(paint, 0.9));
        let a = f.at(cp.x, ch, cp.y);
        // The jib swung along the quay (over the water when working).
        let reach = (area.u1 - cp.x - 1.0).clamp(3.0, 12.0);
        let b = f.at(cp.x + reach, ch + 2.5, cp.y);
        g.part(Metal).beam(a, b, 0.5, rgba(paint));
        let hook = f.at(cp.x + reach - 1.0, 6.0, cp.y);
        g.part(Matte).beam(f.at(cp.x + reach - 1.0, ch + 2.3, cp.y), hook, 0.05, rgba(DARK));
        g.part(Metal).boxed(hook - Vec3::Y * 0.3, Vec3::new(0.5, 0.6, 0.5), f.yaw(), rgba(CHARCOAL));
        g.part(Glow).boxed(b + Vec3::Y * 0.4, Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
    }
    // Crate stacks in blocks, nets, a forklift and a reefer lorry.
    let mut placed = 0;
    let mut v = area.v1 - 4.0;
    while v > area.v0 + 6.0 + sd && placed < 24 {
        let mut u = area.u0 + 2.0;
        while u < area.u1 - iw - 2.0 && placed < 24 {
            if coin(rng, 0.6) {
                { let n = rng.random_range(2..5); fish_crates(g, c, rng, Vec2::new(u, v), n); }
                placed += 1;
            }
            u += 1.6;
        }
        v -= 2.2;
    }
    for k in 0..[2, 3, 3][t] {
        nets(g, c, rng, Vec2::new(area.u0 + 3.0 + k as f32 * 4.5, area.v1 - 3.0));
    }
    if let Some(p) = c.random_inside(rng, 3.0) {
        f.bx(g, Metal, p.x, 0.7, p.y, Vec3::new(1.8, 1.0, 1.1), [0.9, 0.6, 0.1]);
        f.bx(g, Metal, p.x + 0.2, 1.7, p.y, Vec3::new(0.1, 2.2, 1.0), CHARCOAL);
        f.bx(g, Metal, p.x + 1.2, 0.3, p.y, Vec3::new(1.0, 0.06, 0.9), CHARCOAL);
    }
    let front = c.front_face(&c.poly);
    let lorry = front.at(-front.len * 0.2, -3.0);
    if vehicle_fits(c, lorry, 0.0, Vehicle::Lorry) {
        vehicle(g, f, pal, rng, lorry, 0.0, 0.0, Vehicle::Lorry, [0.88, 0.9, 0.93]);
    }
    quay_edge(g, c, &area, mul(pal.steel, 0.45));
    for p in [front.at(-front.len * 0.45, -1.0), front.at(front.len * 0.45, -1.0), Vec2::new(area.u0 + 1.0, area.v1 - 1.0)] {
        floodlight(g, c, p, 11.0);
    }
}
