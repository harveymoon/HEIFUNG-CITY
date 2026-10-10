//! Industry: junkyards and rusted sheds where land is cheap, saw-tooth
//! factories and tank farms in the middle, datacentres and container depots
//! where it is dear.

use super::kit::{gable_roof, squash};
use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

// ── junkyard ─────────────────────────────────────────────────────────────────

pub fn junkyard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = yard(g, c, 0.4, Dirt, pal.dirt);
    let front = c.front_face(&poly);
    fence_lot(g, c, &poly, 2.4, true);
    // Hoardings of corrugated sheet just inside the fence on the back and
    // sides, tagged on both faces.
    for face in c.faces(&poly) {
        if face.is_front || face.len < 7.0 || !rng.random_bool(0.7) {
            continue;
        }
        let span = (face.len * rng.random_range(0.45..0.8)).min(24.0);
        let slack = (face.len - span) * 0.5;
        let t = rng.random_range(-slack..slack);
        face.bx(g, f, Metal, t, 1.25, -0.22, Vec3::new(span, 2.5, 0.08), mul(pick(rng, &pal.metal_paint), 0.75));
        let outer = Face::from_edge(face.at(t - span * 0.5, -0.18), face.at(t + span * 0.5, -0.18));
        graffiti(g, c, rng, &outer, 0.1, 2.4, 1.2);
        let inner = Face::from_edge(face.at(t + span * 0.5, -0.26), face.at(t - span * 0.5, -0.26));
        graffiti(g, c, rng, &inner, 0.1, 2.4, 2.0);
    }

    // Piles and wrecks.
    for _ in 0..rng.random_range(2..5) {
        if let Some(p) = c.random_inside(rng, 4.0) {
            let (r, h) = (rng.random_range(2.0..4.0), rng.random_range(1.5..3.5));
            scrap_pile(g, c, rng, p, r, h);
        }
    }
    for _ in 0..rng.random_range(3..9) {
        if let Some(p) = c.random_inside(rng, 3.0) {
            let kind = if rng.random_bool(0.75) { Vehicle::Wreck } else { Vehicle::Van };
            let yaw = rng.random_range(0.0..std::f32::consts::TAU);
            let col = pick(rng, &pal.car_colors);
            if vehicle_fits(c, p, yaw, kind) {
                vehicle(g, f, pal, rng, p, 0.0, yaw, kind, col);
            }
        }
    }
    // Stacked crushed cars: flat boxes.
    if let Some(p) = c.random_inside(rng, 4.0) {
        for k in 0..rng.random_range(3..6) {
            f.bx_yaw(g, Rust, p.x, 0.3 + k as f32 * 0.6, p.y, Vec3::new(3.8, 0.55, 1.7), mul(pick(rng, &pal.car_colors), 0.5), rng.random_range(-0.1..0.1));
        }
    }
    // Office: a container by the gate with a lit window and a sign.
    let office = front.at(front.len * 0.3, -4.5);
    if poly_inside(c, &rect(office.x, office.y, 3.2, 1.4)) {
        let col = pick(rng, &pal.container_colors);
        container_unit(g, c, rng, office.x, office.y, 0.0, 6.05, 2.44, 2.6, 0.0, col);
        // The container's road side, facing the road.
        let of = Face::new(office + Vec2::new(-3.0, -1.22), office + Vec2::new(3.0, -1.22), Vec2::new(0.0, -1.0));
        of.decal(g, f, Glow, -1.0, 1.5, 0.04, 1.0, 0.7, mul(c.style.window_color(0), 0.9));
        sign_board(g, c, rng, &of, 0.5, 2.3, 4.0, 0.5);
        dish(g, f, office, 2.6, 0.6, pal.steel);
    }
    // Gate sign on the fence line.
    sign_board(g, c, rng, &front, -front.len * 0.25, 2.9, (front.len * 0.3).min(6.0), 0.7);
    // Crane claw on a small gantry over the biggest pile.
    if c.fw > 18.0 && c.fd > 14.0 && c.all_inside(&[Vec2::new(-6.0, -c.fd * 0.2), Vec2::new(6.0, -c.fd * 0.2), Vec2::new(-6.0, c.fd * 0.25), Vec2::new(6.0, c.fd * 0.25)]) {
        gantry_crane(g, c, 0.0, -c.fd * 0.2, c.fd * 0.25, 7.0);
    }
    for k in 0..2 {
        let p = front.at((k as f32 - 0.5) * front.len * 0.7, -2.0);
        if c.inside_local(p) {
            floodlight(g, c, p, 8.0);
        }
    }
    drum_fire(g, f, pal, front.at(-front.len * 0.1, -3.0));
    // Dog house and weeds.
    if let Some(p) = c.random_inside(rng, 2.0) {
        f.bx(g, Wood, p.x, 0.5, p.y, Vec3::new(1.2, 1.0, 1.0), pal.plywood);
    }
    weeds(g, c, rng, &poly, 12);
}

// ── rusted warehouse ─────────────────────────────────────────────────────────

pub fn rusty_warehouse(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = &c.poly;
    let h = (c.floors as f32 * c.fh()).clamp(6.0, 12.0);
    let paint = mul(pick(rng, &pal.metal_paint), 1.0 - 0.3 * c.wear);
    extrude(g, f, Metal, poly, 0.0, h, paint);
    // Plinth of concrete block, rust everywhere above it.
    extrude(g, f, Concrete, &offset_poly(poly, -0.1), 0.0, 1.2, mul(pick(rng, &pal.concrete), 0.8));
    gable_roof(g, c, poly, h, (inradius(poly) * 0.35).clamp(1.5, 4.0), mul(paint, 0.85));
    let front = c.front_face(poly);
    for face in c.faces(poly) {
        if face.len < 4.0 {
            continue;
        }
        grime(g, c, rng, &face, 1.2, h, 7.0);
        graffiti(g, c, rng, &face, 1.2, 4.5, 1.2);
        // High clerestory windows, many boarded or broken.
        let mut spec = WindowSpec::from_ctx(c);
        spec.w = 2.2;
        spec.h = 0.9;
        spec.spacing = 3.6;
        spec.sill = h - 2.2;
        spec.lit = c.lit_chance() * 0.6;
        spec.boarded = 0.4 + 0.5 * c.wear;
        spec.ac = 0.0;
        windows(g, c, rng, &face, 0.0, 1, h, &spec);
        // Rust patches that have eaten through the sheet.
        for _ in 0..((1.0 + 3.0 * c.wear) as i32) {
            let t = rng.random_range(-face.len * 0.4..face.len * 0.4);
            face.decal(g, f, Rust, t, rng.random_range(1.5..h - 1.0), 0.03, rng.random_range(1.0..3.0), rng.random_range(0.8..2.5), c.palette.rust);
        }
        downpipes(g, c, rng, &face, h);
    }
    // Roll-up doors on the front, one hanging open.
    let doors = ((front.len / 8.0) as i32).clamp(1, 4);
    for k in 0..doors {
        let t = -front.len * 0.5 + (k as f32 + 0.5) * (front.len / doors as f32);
        let open = rng.random_bool(0.3);
        if open {
            front.bx(g, f, Matte, t, 2.2, -0.2, Vec3::new(4.0, 4.4, 0.3), DARK);
            front.bx(g, f, Glow, t, 2.0, -0.3, Vec3::new(2.0, 1.2, 0.05), mul(c.style.window_color(0), 0.5));
            front.bx(g, f, Metal, t, 4.6, 0.05, Vec3::new(4.0, 0.9, 0.3), mul(paint, 0.7));
        } else {
            roller_door(g, c, &front, t, 4.0, 4.4, mul(paint, 0.75));
        }
    }
    // Faded sign.
    if rng.random_bool(0.7) {
        front.bx(g, f, Matte, 0.0, h - 1.4, 0.1, Vec3::new((front.len * 0.5).min(12.0), 1.4, 0.12), mul(paint, 0.6));
        sign_board(g, c, rng, &front, 0.0, h - 1.4, (front.len * 0.4).min(10.0), 1.0);
    }
    // Yard: pallets, a van, drums, weeds, a broken fence.
    let yard = offset_poly(&c.lot_poly, 0.5);
    for _ in 0..rng.random_range(2..5) {
        let p = front.at(rng.random_range(-front.len * 0.45..front.len * 0.45), rng.random_range(2.0..5.0));
        if c.inside_local(p) {
            for k in 0..rng.random_range(1..4) {
                f.bx_yaw(g, Wood, p.x, 0.08 + k as f32 * 0.16, p.y, Vec3::new(1.2, 0.14, 1.0), pal.wood, rng.random_range(-0.2..0.2));
            }
        }
    }
    let van = front.at(front.len * 0.4, 3.5);
    if vehicle_fits(c, van, front.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Van) && rng.random_bool(0.6) {
        let col = pick(rng, &pal.car_colors);
        vehicle(g, f, pal, rng, van, 0.0, front.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Van, col);
    }
    for _ in 0..rng.random_range(2..6) {
        if let Some(p) = c.random_inside(rng, 1.0) {
            if !nd_sim::zoning::point_in_polygon(p, poly) {
                let w = f.to_world(p.x, p.y);
                let (s, col) = if rng.random_bool(0.5) { (Rust, pal.rust) } else { (Metal, pick(rng, &pal.metal_paint)) };
                g.part(s).cylinder(w, 0.3, 0.3, 0.0, 0.9, 10, rgba(col));
            }
        }
    }
    weeds(g, c, rng, &yard, 10);
    let faces = c.faces(&yard);
    for (i, face) in faces.iter().enumerate() {
        if face.is_front || i % 2 == 0 {
            continue;
        }
        fence(g, c, face.a, face.at(-face.len * 0.1, 0.0), 2.2, false);
    }
    vent_stack(g, f, poly_center(poly), h + 1.0, 0.5, 2.5, mul(pal.steel, 0.6));
    floodlight(g, c, front.at(-front.len * 0.45, 1.5), 7.0);
}

// ── factory ──────────────────────────────────────────────────────────────────

pub fn factory(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let h = (c.floors as f32 * fh).clamp(8.0, 16.0);
    let wall = mul(pick(rng, &c.style.wall_colors), 1.0 - 0.3 * c.wear);
    let poly = &c.poly;
    extrude(g, f, Panel, poly, 0.0, h, wall);
    extrude(g, f, Concrete, &offset_poly(poly, -0.1), 0.0, 1.5, mul(pick(rng, &pal.concrete), 0.85));
    let front = c.front_face(poly);
    // Saw-tooth roof lights running across the footprint.
    let axis = front.out;
    let (lo, hi) = bounds(poly);
    let span = hi.y - lo.y;
    let teeth = ((span / 5.0) as i32).max(1);
    for k in 0..teeth {
        let v0 = lo.y + k as f32 * (span / teeth as f32) + 0.6;
        let v1 = v0 + span / teeth as f32 - 1.2;
        let strip = clip_v(poly, v0, v1);
        if strip.len() < 3 {
            continue;
        }
        let inner = offset_poly(&strip, 0.8);
        if inner.len() < 3 {
            continue;
        }
        let top = squash(&inner, axis, 1.0 - 0.08);
        loft(g, f, Metal, &inner, &shift_poly(&top, Vec2::new(0.0, -(v1 - v0) * 0.46)), h, h + 2.2, mul(wall, 0.9));
        if coin(rng, 0.6 * (1.0 - c.wear)) {
            let lit = squash(&inner, axis, 1.0 - 0.5);
            extrude(g, f, Glow, &shift_poly(&lit, Vec2::new(0.0, (v1 - v0) * 0.2)), h + 0.6, h + 1.6, mul(c.style.window_color(0), 0.6));
        }
    }
    // Loading dock and doors on the front.
    let doors = ((front.len / 7.0) as i32).max(1);
    for k in 0..doors {
        let t = -front.len * 0.5 + (k as f32 + 0.5) * (front.len / doors as f32);
        roller_door(g, c, &front, t, 4.0, 4.4, mul(pal.steel, 0.6));
    }
    // Loading dock: as deep as the forecourt allows, shortened until it fits.
    let dock_d = (3.0f32).min(c.reach_out(&front) - 0.3);
    let mut dock_half = front.len * 0.425;
    while dock_half > 2.0 && !(c.inside_local(front.at(-dock_half, dock_d)) && c.inside_local(front.at(dock_half, dock_d))) {
        dock_half -= 0.5;
    }
    if dock_d > 0.8 && dock_half > 2.0 {
        front.bx(g, f, Concrete, 0.0, 0.6, dock_d * 0.5, Vec3::new(dock_half * 2.0, 1.2, dock_d), mul(wall, 0.8));
    }
    for face in c.faces(poly) {
        if face.len > 5.0 {
            band_windows(g, c, rng, &face, 1.5, 1, 4.0);
            grime(g, c, rng, &face, 1.5, h, 2.5);
        }
    }
    // Chimneys, tanks and pipes.
    for _ in 0..rng.random_range(1..=3) {
        if let Some(p) = c.random_inside(rng, 4.0) {
            chimney(g, f, p, 1.2, h, rng.random_range(6.0..16.0), mul(pal.steel, 0.7), pal.aviation_light);
        }
    }
    let back = c.faces(poly).into_iter().filter(|x| !x.is_front).max_by(|a, b| a.len.total_cmp(&b.len));
    if let Some(back) = back {
        for k in 0..rng.random_range(0..3) {
            let p = back.at(-back.len * 0.3 + k as f32 * back.len * 0.3, 2.5);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.2 {
                let (s, col) = if rng.random_bool(0.5) { (Rust, pal.rust) } else { (Metal, pick(rng, &pal.metal_paint)) };
            tank_vertical(g, f, p, 1.8, 0.0, rng.random_range(4.0..8.0), s, col);
            }
        }
        // Pipe runs along the back wall with elbows down to the ground.
        for k in 0..rng.random_range(1..4) {
            let y = h * (0.35 + 0.18 * k as f32);
            let a = back.at(-back.len * 0.45, 0.35);
            let b = back.at(back.len * 0.45, 0.35);
            let e = back.at(back.len * 0.45, 1.5);
            pipe_run(g, f, &[Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y), Vec3::new(e.x, y, e.y), Vec3::new(e.x, 0.5, e.y)], 0.18, mul(pal.steel, 0.8));
        }
    }
    // Conveyor bridge to an annex when there is room.
    let annex_p = Vec2::new(hi.x - 4.0, (lo.y + hi.y) * 0.5);
    if c.inside_local(annex_p + Vec2::new(3.5, 3.5)) && c.inside_local(annex_p + Vec2::new(3.5, -3.5)) && hi.x - lo.x > 20.0 {
        let annex = fit_inside(c, &rect(hi.x - 1.0, annex_p.y, 3.5, 4.5));
        extrude(g, f, Metal, &annex, 0.0, h * 0.6, mul(wall, 0.9));
        let a = f.at(hi.x - 6.0, h * 0.5, annex_p.y);
        let b = f.at(hi.x - 1.0, h * 0.5, annex_p.y);
        g.part(Metal).boxed(a.lerp(b, 0.5), Vec3::new(a.distance(b), 2.0, 2.0), f.yaw(), rgba(mul(wall, 0.85)));
    }
    if coin(rng, c.rules.sign_chance) {
        sign_board(g, c, rng, &front, -front.len * 0.25, h - 1.5, (front.len * 0.35).min(10.0), 1.1);
    }
    // Yard: trucks at the docks, floodlights, fence at the sides.
    for k in 0..doors.min(3) {
        if !rng.random_bool(0.5) {
            continue;
        }
        let t = -front.len * 0.5 + (k as f32 + 0.5) * (front.len / doors as f32);
        let p = front.at(t, 7.5);
        if vehicle_fits(c, p, front.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Truck) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, front.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Truck, col);
        }
    }
    floodlight(g, c, front.at(-front.len * 0.45, 4.0), 9.0);
    floodlight(g, c, front.at(front.len * 0.45, 4.0), 9.0);
    let yard = offset_poly(&c.lot_poly, 0.4);
    for face in c.faces(&yard) {
        if !face.is_front {
            fence(g, c, face.a, face.b, 2.4, true);
        }
    }
}

/// Part of a lot-local polygon between two `v` values.
pub(super) fn clip_v(poly: &[Vec2], v0: f32, v1: f32) -> Vec<Vec2> {
    use nd_sim::zoning::clip_halfplane;
    let a = clip_halfplane(poly, Vec2::new(0.0, v0), Vec2::new(0.0, 1.0));
    clip_halfplane(&a, Vec2::new(0.0, v1), Vec2::new(0.0, -1.0))
}

// ── tank farm ────────────────────────────────────────────────────────────────

pub fn tank_farm(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.6));
    let bund = offset_poly(&yard, 2.0);
    parapet(g, c, &bund, 0.0, 1.0, 0.4, Concrete, mul(pick(rng, &pal.concrete), 0.9));
    let front = c.front_face(&yard);
    // Tanks on a jittered grid.
    let (lo, hi) = bounds(&offset_poly(&bund, 3.0));
    let r = ((hi.x - lo.x).min(hi.y - lo.y) * 0.18).clamp(2.5, 6.0);
    let mut tanks: Vec<(Vec2, f32)> = Vec::new();
    let mut tries = 0;
    while tanks.len() < 7 && tries < 60 {
        tries += 1;
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        let rr = r * rng.random_range(0.7..1.0);
        if !nd_sim::zoning::point_in_polygon(p, &offset_poly(&bund, rr + 0.5)) || distance_to_boundary(p, &c.lot_poly) < rr + 1.2 {
            continue;
        }
        if tanks.iter().any(|(q, qr)| q.distance(p) < rr + qr + 1.5) {
            continue;
        }
        let h = rng.random_range(5.0..12.0);
        let rusty = coin(rng, 0.3 + 0.6 * c.wear);
        tank_vertical(g, f, p, rr, 0.0, h, if rusty { Rust } else { Metal }, if rusty { pal.rust } else { pick(rng, &pal.metal_paint) });
        let w = f.to_world(p.x, p.y);
        aviation_light(g, pal, Vec3::new(w.x, h + 1.3, w.y), 0.3);
        tanks.push((p, rr));
    }
    // Pipe runs linking tank tops and running to a manifold by the gate.
    for w in tanks.windows(2) {
        let (a, ra) = w[0];
        let (b, _) = w[1];
        // Dog-leg between the tanks when the elbows are on the lot, else straight.
        let (e1, e2) = (Vec2::new(a.x, (a.y + b.y) * 0.5), Vec2::new(b.x, (a.y + b.y) * 0.5));
        if c.inside_local(e1) && c.inside_local(e2) && distance_to_boundary(e1, &c.lot_poly) > 0.5 && distance_to_boundary(e2, &c.lot_poly) > 0.5 {
            pipe_run(g, f, &[Vec3::new(a.x, 1.2, a.y + ra), Vec3::new(e1.x, 1.2, e1.y), Vec3::new(e2.x, 1.2, e2.y), Vec3::new(b.x, 1.2, b.y)], 0.22, mul(pal.steel, 0.8));
        } else {
            pipe_run(g, f, &[Vec3::new(a.x, 1.2, a.y), Vec3::new(b.x, 1.2, b.y)], 0.22, mul(pal.steel, 0.8));
        }
    }
    let manifold = front.at(front.len * 0.3, 4.0);
    if poly_inside(c, &rect(manifold.x + 1.2, manifold.y + 0.5, 1.8, 0.8)) {
        for k in 0..4 {
            let p = manifold + Vec2::new(k as f32 * 0.8, 0.0);
            f.bx(g, Metal, p.x, 0.8, p.y, Vec3::new(0.3, 1.6, 0.3), mul(pal.steel, 0.8));
            f.bx(g, Matte, p.x, 1.4, p.y, Vec3::new(0.5, 0.15, 0.5), mul(pal.warning_light, 0.3));
        }
        f.bx(g, Metal, manifold.x + 1.2, 0.4, manifold.y + 1.0, Vec3::new(3.6, 0.3, 0.3), mul(pal.steel, 0.8));
    }
    // Pump house and flare stack.
    let ph = front.at(-front.len * 0.3, 5.0);
    if c.inside_local(ph) {
        let house = fit_inside(c, &rect(ph.x, ph.y, 3.0, 2.5));
        extrude(g, f, Metal, &house, 0.0, 3.5, pick(rng, &pal.metal_paint));
        let hf = c.front_face(&house);
        roller_door(g, c, &hf, 0.0, 2.6, 2.8, mul(pal.steel, 0.6));
        vent_stack(g, f, ph, 3.5, 0.3, 1.5, mul(pal.steel, 0.7));
    }
    if let Some((p, rr)) = tanks.last() {
        let flare = *p + Vec2::new(rr + 3.0, 0.0);
        if c.inside_local(flare) {
            chimney(g, f, flare, 0.5, 0.0, 18.0, mul(pal.steel, 0.75), mul(pal.warning_light, 1.2));
            let w = f.to_world(flare.x, flare.y);
            g.part(Glow).boxed(Vec3::new(w.x, 18.9, w.y), Vec3::new(0.9, 1.6, 0.9), 0.0, rgba([3.0, 1.4, 0.3]));
        }
    }
    // Fence, floodlights, hazard signs.
    fence_lot(g, c, &yard, 2.4, true);
    for p in [front.at(-front.len * 0.4, -1.5), front.at(front.len * 0.4, -1.5)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 10.0);
        }
    }
    for k in 0..3 {
        front.decal(g, f, Glow, -front.len * 0.3 + k as f32 * front.len * 0.3, 1.6, -0.1, 0.6, 0.6, pal.warning_light);
    }
}

// ── datacentre ───────────────────────────────────────────────────────────────

pub fn datacenter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let h = (c.floors as f32 * c.fh()).clamp(9.0, 18.0);
    let panel = pick(rng, &pal.panel);
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.5));
    // Main hall: windowless panel box, set back behind the security yard.
    let hall = offset_poly(&c.poly, 1.5);
    let hall = shift_poly(&hall, Vec2::new(0.0, 2.0));
    let hall = fit_inside(c, &hall);
    extrude(g, f, Panel, &hall, 0.0, h, panel);
    extrude(g, f, Concrete, &offset_poly(&hall, -0.15), 0.0, 1.0, mul(pick(rng, &pal.concrete), 0.8));
    cornice(g, c, &hall, h - 0.4, 0.8, 0.3, Panel, mul(panel, 0.8));
    // Vertical seam fins and a cold status strip along the roof edge.
    for face in c.faces(&hall) {
        if face.len < 4.0 {
            continue;
        }
        let n = (face.len / 6.0).floor() as i32;
        for k in 1..n {
            let t = -face.len * 0.5 + k as f32 * (face.len / n as f32);
            face.bx(g, f, Matte, t, h * 0.5, 0.08, Vec3::new(0.25, h - 1.2, 0.16), mul(panel, 0.6));
        }
        face.decal(g, f, Glow, 0.0, h - 0.9, 0.05, face.len * 0.96, 0.08, mul([0.5, 1.3, 2.2], 0.6));
        // Louvre bands low down.
        face.decal(g, f, Matte, 0.0, 2.2, 0.06, face.len * 0.9, 1.0, mul(panel, 0.5));
    }
    // Roof plant: rows of chillers. [pt-visual] A high-end data centre
    // keeps one row of chillers along the front and covers the rest of the
    // roof in solar panels.
    let solar = c.tier == nd_sim::Tier::High && nd_sim::zoning::polygon_area(&hall).abs() > 400.0;
    let (lo, hi) = bounds(&offset_poly(&hall, 3.0));
    let rows = if solar { 1 } else { ((hi.y - lo.y) / 6.0).floor().max(1.0) as i32 };
    let cols = ((hi.x - lo.x) / 5.0).floor().max(1.0) as i32;
    for r in 0..rows {
        for col in 0..cols {
            let p = Vec2::new(lo.x + (col as f32 + 0.5) * (hi.x - lo.x) / cols as f32, lo.y + (r as f32 + 0.5) * (hi.y - lo.y) / rows as f32);
            if nd_sim::zoning::point_in_polygon(p, &offset_poly(&hall, 2.5)) && distance_to_boundary(p, &hall) > 2.2 {
                let p = if solar { Vec2::new(p.x, lo.y + 1.5) } else { p };
                chiller(g, c, p, h, 4.0, 2.0, 0.0, mul(pal.steel, 0.9));
            }
        }
    }
    if solar {
        let (blo, bhi) = bounds(&hall);
        let back = [Vec2::new(blo.x, lo.y + 4.0), Vec2::new(bhi.x, lo.y + 4.0), Vec2::new(bhi.x, bhi.y), Vec2::new(blo.x, bhi.y)];
        let roof: Vec<Vec2> = back.iter().map(|q| clamp_into(&hall, *q)).collect();
        solar_roof(g, c, &roof, h);
    }
    let hc = poly_center(&hall);
    let roof = offset_poly(&hall, 2.0);
    let mast_p = roof.iter().copied().max_by(|a, b| (a.y + a.x * 0.3).total_cmp(&(b.y + b.x * 0.3))).unwrap_or(hc);
    let dish_p = roof.iter().copied().max_by(|a, b| (a.y - a.x * 0.3).total_cmp(&(b.y - b.x * 0.3))).unwrap_or(hc);
    antenna_mast(g, c, mast_p.lerp(hc, 0.1), h, 8.0);
    if dish_p.distance(mast_p) > 3.0 {
        dish(g, f, dish_p.lerp(hc, 0.1), h, 1.4, pal.steel);
    }
    // Generator yard beside the hall.
    let front = c.front_face(&c.lot_poly);
    let gen_p = Vec2::new(lo.x - 5.0, (lo.y + hi.y) * 0.5);
    if poly_inside(c, &rect(gen_p.x, gen_p.y + 1.0, 3.4, 7.2)) && poly_inside(c, &rect(gen_p.x, gen_p.y + 8.0, 1.3, 3.2)) {
        for k in 0..3 {
            let p = gen_p + Vec2::new(0.0, (k as f32 - 1.0) * 3.4);
            f.bx(g, Metal, p.x, 1.3, p.y, Vec3::new(4.5, 2.6, 2.2), mul(pick(rng, &pal.metal_paint), 1.0));
            vent_stack(g, f, p + Vec2::new(1.6, 0.0), 2.6, 0.2, 1.4, mul(pal.steel, 0.6));
        }
        let pen = rect(gen_p.x, gen_p.y, 3.2, 6.0);
        fence_poly(g, c, &pen, 2.2, None, false);
        // diesel tank
        tank_horizontal(g, f, gen_p + Vec2::new(0.0, 8.0), std::f32::consts::FRAC_PI_2, 6.0, 1.1, 1.6, pick(rng, &pal.metal_paint));
    }
    // Security: perimeter fence, gatehouse, bollards, floodlights, cameras.
    fence_lot(g, c, &yard, 2.6, true);
    let gate = front.at(front.len * 0.3, -2.8);
    if c.inside_local(gate) {
        let house = fit_inside(c, &rect(gate.x, gate.y, 1.6, 1.4));
        extrude(g, f, Panel, &house, 0.0, 3.0, mul(panel, 0.9));
        curtain_lights(g, c, rng, &house, 1.0, 1, 1.6);
        let hf = c.front_face(&house);
        if c.inside_local(hf.at(2.6, 0.0)) && c.inside_local(hf.at(2.6, -3.2)) {
            hf.bx(g, f, Matte, 2.6, 0.5, 0.0, Vec3::new(0.2, 1.0, 0.2), pal.steel);
            hf.bx(g, f, Matte, 2.6, 1.0, -1.5, Vec3::new(0.1, 0.1, 3.2), mul(pal.warning_light, 0.4));
        }
    }
    bollards(g, c, &front, 6, -1.4);
    for p in [front.at(-front.len * 0.42, -1.5), front.at(front.len * 0.42, -1.5)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 9.0);
        }
    }
    // Lit logo high on the front face of the hall.
    let hf = c.front_face(&hall);
    sign_board(g, c, rng, &hf, hf.len * 0.2, h - 2.5, (hf.len * 0.3).min(8.0), 1.2);
    entrance(g, c, rng, &hf, -hf.len * 0.3, 2.0, 2.8);
}

// ── container depot ──────────────────────────────────────────────────────────

pub fn depot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.5));
    let front = c.front_face(&yard);
    let (lo, hi) = bounds(&c.poly);
    // Warehouse along the back of the parcel with docks facing the yard.
    let depth = ((hi.y - lo.y) * 0.4).clamp(8.0, 22.0);
    let shed = clip_v(&c.poly, hi.y - depth, hi.y);
    let shed = fit_inside(c, &shed);
    let h = 9.0;
    let panel = pick(rng, &pal.panel);
    if shed.len() >= 3 {
        extrude(g, f, Panel, &shed, 0.0, h, panel);
        extrude(g, f, Concrete, &offset_poly(&shed, -0.1), 0.0, 1.3, mul(pick(rng, &pal.concrete), 0.85));
        cornice(g, c, &shed, h - 0.3, 0.6, 0.2, Panel, mul(panel, 0.8));
        let dock = c.front_face(&shed);
        let docks = ((dock.len / 5.0) as i32).max(1);
        for k in 0..docks {
            let t = -dock.len * 0.5 + (k as f32 + 0.5) * (dock.len / docks as f32);
            roller_door(g, c, &dock, t, 3.4, 4.0, mul(pal.steel, 0.65));
            f.bx(g, Matte, dock.at(t, 0.3).x, 1.1, dock.at(t, 0.3).y, Vec3::new(1.2, 0.5, 0.4), DARK);
            if rng.random_bool(0.5) {
                let p = dock.at(t, 6.0);
                if vehicle_fits(c, p, dock.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Truck) {
                    let col = pick(rng, &pal.car_colors);
                    vehicle(g, f, pal, rng, p, 0.0, dock.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Truck, col);
                }
            }
        }
        canopy(g, c, &dock, 0.0, 5.0, dock.len * 0.95, 3.0, mul(panel, 0.85));
        sign_board(g, c, rng, &dock, 0.0, h - 1.2, (dock.len * 0.4).min(12.0), 1.3);
        roof_gear(g, c, rng, &shed, h, 0.5);
        for face in c.faces(&shed) {
            if !face.is_front && face.len > 6.0 {
                band_windows(g, c, rng, &face, 5.5, 1, 3.0);
            }
        }
    }
    // Office block at one front corner.
    let office = fit_inside(c, &rect(lo.x + 5.0, lo.y + 5.0, 4.5, 4.0));
    if nd_sim::zoning::polygon_area(&office).abs() > 20.0 {
        extrude(g, f, Panel, &office, 0.0, 7.0, mul(panel, 0.95));
        for face in c.faces(&office) {
            band_windows(g, c, rng, &face, 0.0, 2, 3.5);
        }
        parapet(g, c, &office, 7.0, 0.5, 0.2, Panel, mul(panel, 0.8));
        let of = c.front_face(&office);
        entrance(g, c, rng, &of, 0.0, 1.6, 2.6);
    }
    // Container rows in the yard with a gantry crane over them.
    let yard_lo = Vec2::new(lo.x + 1.0, lo.y + 10.0);
    let yard_hi = Vec2::new(hi.x - 1.0, hi.y - depth - 4.0);
    if yard_hi.x - yard_lo.x > 14.0 && yard_hi.y - yard_lo.y > 4.0 {
        let rows = ((yard_hi.y - yard_lo.y) / 3.6).floor() as i32;
        let cols = ((yard_hi.x - yard_lo.x) / 13.0).floor() as i32;
        for r in 0..rows {
            for col in 0..cols {
                if rng.random_bool(0.25) {
                    continue;
                }
                let u = yard_lo.x + 6.5 + col as f32 * 13.0;
                let v = yard_lo.y + 1.5 + r as f32 * 3.6;
                let stack = rng.random_range(1..=3);
                for k in 0..stack {
                    let len = if rng.random_bool(0.7) { 12.1 } else { 6.05 };
                    if !c.inside_local(Vec2::new(u + len * 0.5, v)) || !c.inside_local(Vec2::new(u - len * 0.5, v)) {
                        break;
                    }
                    let col = pick(rng, &pal.container_colors);
                    container_unit(g, c, rng, u, v, 0.0, len, 2.44, 2.6, k as f32 * 2.6, col);
                }
            }
        }
        gantry_crane(g, c, (yard_lo.x + yard_hi.x) * 0.5, yard_lo.y - 1.0, yard_hi.y + 1.0, 11.0);
    }
    // Perimeter, gate, lights.
    fence_lot(g, c, &yard, 2.6, true);
    for p in [front.at(-front.len * 0.42, -2.0), front.at(front.len * 0.42, -2.0), Vec2::new(lo.x + 2.0, hi.y - depth - 2.0), Vec2::new(hi.x - 2.0, hi.y - depth - 2.0)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 12.0);
        }
    }
    // Lane markings in the yard: painted lines, one quad each.
    for k in 0..3 {
        let t = -front.len * 0.3 + k as f32 * front.len * 0.3;
        let p = front.at(t, -5.0);
        if c.inside_local(front.at(t, -1.0)) && c.inside_local(front.at(t, -9.0)) {
            f.flat(g, Glow, p.x, 0.06, p.y, 0.15, 8.0, front.yaw(), mul(pal.lane_marker, 0.5));
        }
    }
}

// ── industry sub-zones ───────────────────────────────────────────────────────
//
// The looks of the industry kinds (`nd_sim::industry`): farms, oil fields,
// refineries, container yards, fish processing and truck lots. Each is
// forced by the business on the lot (see `choose_style`).

/// Strips of a polygon across `v`, `width` wide with `gap` between them.
fn bands(poly: &[Vec2], width: f32, gap: f32) -> Vec<Vec<Vec2>> {
    let (lo, hi) = bounds(poly);
    let mut out = Vec::new();
    let mut v = lo.y;
    while v < hi.y && out.len() < 40 {
        let strip = clip_v(poly, v, (v + width).min(hi.y));
        if strip.len() >= 3 && nd_sim::zoning::polygon_area(&strip).abs() > 2.0 {
            out.push(strip);
        }
        v += width + gap;
    }
    out
}

/// A pitched-roof shed: walls, a gable roof and a door on its front.
pub(super) fn shed(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], h: f32, wall: [f32; 3], s: crate::surface::Surface) {
    let f = &c.frame;
    extrude(g, f, s, poly, 0.0, h, wall);
    gable_roof(g, c, poly, h, (inradius(poly) * 0.45).clamp(1.0, 3.5), mul(wall, 0.7));
    let front = c.front_face(poly);
    if front.len > 4.0 {
        roller_door(g, c, &front, 0.0, (front.len * 0.35).min(4.0), (h - 0.6).min(4.0), mul(c.palette.steel, 0.6));
    }
    let _ = rng;
}

// ── farm ─────────────────────────────────────────────────────────────────────

/// Fields in rows behind a barn, a farmhouse, silos and a greenhouse.
pub fn farmstead(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = offset_poly(&c.lot_poly, 0.4);
    ground(g, f, Dirt, &lot, 0.02, mix(pal.dirt, pal.foliage_dry, 0.3));
    let (lo, hi) = bounds(&lot);
    let depth = hi.y - lo.y;
    // The farmyard along the front, the fields behind.
    let yard_d = (depth * 0.38).clamp(8.0, 18.0);
    let fields = clip_v(&lot, lo.y + yard_d, hi.y);
    let crop = if coin(rng, 0.5) { pal.foliage } else { mix(pal.foliage, pal.foliage_dry, 0.5) };
    for (i, strip) in bands(&fields, 2.2, 0.9).iter().enumerate() {
        let col = if i % 2 == 0 { mul(crop, 0.9 + 0.1 * (i % 3) as f32) } else { mul(crop, 0.75) };
        ground(g, f, Foliage, strip, 0.08, col);
    }
    // A hedge or fence on the field margin.
    let margin = c.faces(&fields);
    for face in margin.iter().filter(|fc| !fc.is_front && fc.len > 6.0).take(3) {
        if coin(rng, 0.6) {
            hedge(g, c, face.at(-face.len * 0.45, 0.6), face.at(face.len * 0.45, 0.6), 1.2, 0.8, mul(pal.foliage, 0.7));
        }
    }
    let front = c.front_face(&lot);
    // Barn.
    let barn_c = front.at(-front.len * 0.18, -yard_d * 0.5);
    let barn = fit_inside(c, &rect(barn_c.x, barn_c.y, (front.len * 0.18).clamp(3.0, 8.0), (yard_d * 0.32).clamp(2.5, 5.0)));
    if nd_sim::zoning::polygon_area(&barn).abs() > 12.0 {
        let red = mix([0.45, 0.12, 0.08], pick(rng, &pal.metal_paint), 0.3);
        shed(g, c, rng, &barn, 4.5, red, Wood);
    }
    // Silos beside it.
    let silos = rng.random_range(1..=3);
    for k in 0..silos {
        let p = front.at(front.len * (0.05 + 0.1 * k as f32), -yard_d * 0.55);
        let r = 1.8;
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > r + 0.6 {
            let h = rng.random_range(8.0..13.0);
            tank_vertical(g, f, p, r, 0.0, h, Metal, mul(pal.steel, 1.1));
            let w = f.to_world(p.x, p.y);
            g.part(Metal).cylinder(w, r * 1.02, 0.25, h + 1.0, h + 2.4, 12, rgba(mul(pal.steel, 0.9)));
        }
    }
    // Greenhouse at the far end of the yard.
    let gh_c = front.at(front.len * 0.32, -yard_d * 0.5);
    let gh = fit_inside(c, &rect(gh_c.x, gh_c.y, (front.len * 0.12).clamp(2.0, 6.0), (yard_d * 0.3).clamp(2.0, 4.5)));
    if nd_sim::zoning::polygon_area(&gh).abs() > 8.0 {
        extrude(g, f, Glass, &gh, 0.0, 2.6, mul(pick(rng, &pal.glass), 1.1));
        gable_roof(g, c, &gh, 2.6, 1.2, mul(pick(rng, &pal.glass), 1.2));
        ground(g, f, Foliage, &offset_poly(&gh, 0.4), 0.1, pal.foliage);
    }
    // A tractor (a van will do at this scale) and a water trough.
    let t = front.at(-front.len * 0.4, -3.5);
    if vehicle_fits(c, t, front.yaw(), Vehicle::Van) {
        vehicle(g, f, pal, rng, t, 0.0, front.yaw(), Vehicle::Van, mix([0.2, 0.45, 0.15], pal.foliage, 0.2));
    }
    water_tank(g, f, front.at(front.len * 0.2, -2.5), 0.0, 0.8, 1.2, Metal, mul(pal.steel, 0.8));
    // Low fence along the road.
    if front.len > 6.0 {
        fence(g, c, front.at(-front.len * 0.48, -0.3), front.at(-front.len * 0.05, -0.3), 1.1, false);
    }
}

// ── oil field ────────────────────────────────────────────────────────────────

/// Pumpjacks nodding over a stained, oily yard; a derrick, tanks and a flare.
pub fn oil_field(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Dirt, mul(pal.dirt, 0.75));
    // Black stains where the oil has soaked in.
    for _ in 0..rng.random_range(4..9) {
        if let Some(p) = c.random_inside(rng, 3.0) {
            let r = rng.random_range(1.5..4.5);
            let stain: Vec<Vec2> = ngon(p.x, p.y, r, 7, rng.random_range(0.0..1.0)).into_iter().map(|q| clamp_into(&lot, q)).collect();
            ground(g, f, Asphalt, &stain, 0.05, mul(DARK, 0.7));
        }
    }
    fence_lot(g, c, &lot, 2.0, true);
    // Pumpjacks on a loose grid.
    let (lo, hi) = bounds(&offset_poly(&lot, 4.0));
    let mut jacks = 0;
    let (mut v, steel) = (lo.y + 3.0, pick(rng, &pal.metal_paint));
    while v < hi.y && jacks < 6 {
        let mut u = lo.x + 3.0;
        while u < hi.x && jacks < 6 {
            let p = Vec2::new(u + rng.random_range(-1.5..1.5), v);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 4.0 {
                pumpjack(g, c, rng, p, steel);
                jacks += 1;
            }
            u += 11.0;
        }
        v += 10.0;
    }
    // A derrick if there is room.
    if let Some(p) = c.random_inside(rng, 5.0) {
        if distance_to_boundary(p, &c.lot_poly) > 4.0 {
            derrick(g, c, p, rng.random_range(14.0..22.0));
        }
    }
    // Storage tanks and a flare by the gate.
    let front = c.front_face(&lot);
    for k in 0..2 {
        let p = front.at(front.len * (0.25 + 0.15 * k as f32), -4.5);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 3.0 {
            tank_vertical(g, f, p, 2.2, 0.0, 5.0, Rust, pal.rust);
        }
    }
    let flare = front.at(-front.len * 0.38, -3.0);
    if c.inside_local(flare) && distance_to_boundary(flare, &c.lot_poly) > 1.0 {
        chimney(g, f, flare, 0.35, 0.0, 12.0, mul(pal.steel, 0.7), [2.4, 1.0, 0.25]);
    }
    weeds(g, c, rng, &lot, 6);
}

/// One pumpjack: a sampson post, a walking beam with a horse head, the
/// counterweight crank and a wellhead.
fn pumpjack(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, color: [f32; 3]) {
    let f = &c.frame;
    let yaw = rng.random_range(0.0..std::f32::consts::TAU);
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let tilt = rng.random_range(-0.25..0.25);
    f.bx_yaw(g, Concrete, p.x, 0.2, p.y, Vec3::new(5.5, 0.4, 1.6), mul(c.palette.concrete(0), 0.8), yaw);
    // Sampson post (A-frame).
    for s in [-0.5f32, 0.5] {
        let q = p + Vec2::new(-d.y, d.x) * s;
        f.bx(g, Matte, q.x, 1.7, q.y, Vec3::new(0.25, 3.0, 0.25), mul(color, 0.8));
    }
    // Walking beam, tilted by the phase of the stroke.
    let top = Vec3::new(p.x, 3.3, p.y);
    let a = top + Vec3::new(d.x * 2.8, 2.8 * tilt, d.y * 2.8);
    let b = top - Vec3::new(d.x * 2.4, 2.4 * tilt, d.y * 2.4);
    g.part(Metal).beam(f.at(a.x, a.y, a.z), f.at(b.x, b.y, b.z), 0.35, rgba(color));
    // Horse head over the well.
    let head = p + d * 3.0;
    f.bx_yaw(g, Metal, head.x, 3.3 + 3.0 * tilt, head.y, Vec3::new(0.6, 1.4, 0.5), mul(color, 0.9), yaw);
    f.bx(g, Matte, head.x, 1.1, head.y, Vec3::new(0.08, 2.0, 0.08), CHARCOAL);
    f.bx(g, Metal, head.x, 0.4, head.y, Vec3::new(0.5, 0.8, 0.5), mul(c.palette.steel, 0.7));
    // Crank and counterweights at the back.
    let crank = p - d * 2.4;
    f.bx_yaw(g, Matte, crank.x, 1.0, crank.y, Vec3::new(1.2, 1.6, 1.0), CHARCOAL, yaw);
    f.bx_yaw(g, Metal, crank.x, 1.4, crank.y, Vec3::new(0.4, 1.0, 1.4), mul(c.palette.warning_light, 0.4), yaw);
}

/// A lattice drilling derrick: four legs tapering to a crown, with braces.
fn derrick(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.steel, 0.85);
    let base = 2.2;
    let top = 0.5;
    let corners = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
    for q in corners {
        let a = f.at(p.x + q.x * base, 0.0, p.y + q.y * base);
        let b = f.at(p.x + q.x * top, h, p.y + q.y * top);
        g.part(Metal).beam(a, b, 0.18, rgba(steel));
    }
    // Braces every few metres.
    let levels = (h / 4.0) as i32;
    for k in 1..levels {
        let t = k as f32 / levels as f32;
        let r = base + (top - base) * t;
        for i in 0..4 {
            let (q0, q1) = (corners[i], corners[(i + 1) % 4]);
            g.part(Metal).beam(f.at(p.x + q0.x * r, h * t, p.y + q0.y * r), f.at(p.x + q1.x * r, h * t, p.y + q1.y * r), 0.1, rgba(steel));
        }
    }
    f.bx(g, Metal, p.x, h + 0.4, p.y, Vec3::new(1.6, 0.8, 1.6), steel);
    aviation_light_local(g, f, c.palette, p.x, h + 1.0, p.y, 0.3);
    f.bx(g, Concrete, p.x, 0.3, p.y, Vec3::new(5.0, 0.6, 5.0), mul(c.palette.concrete(0), 0.75));
}

// ── refinery ─────────────────────────────────────────────────────────────────

/// Distillation columns in a pipe rack, tanks, a cracking unit and flares.
pub fn refinery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.4));
    // [dirty-industry] Oil and soot soaked into the yard.
    super::dirty::soot_stains(g, c, rng, &lot, 0.7, false);
    fence_lot(g, c, &lot, 2.6, true);
    let (lo, hi) = bounds(&offset_poly(&lot, 3.0));
    let mid = (lo + hi) * 0.5;
    let steel = mul(pal.steel, 1.05);
    // Columns along the middle, linked by a pipe rack.
    let n = (((hi.x - lo.x) / 7.0) as i32).clamp(1, 5);
    let mut tops: Vec<Vec3> = Vec::new();
    for k in 0..n {
        let u = lo.x + (k as f32 + 0.5) * (hi.x - lo.x) / n as f32;
        let p = Vec2::new(u, mid.y);
        let r = rng.random_range(1.0..1.8);
        if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < r + 1.0 {
            continue;
        }
        let h = rng.random_range(16.0..30.0);
        let w = f.to_world(p.x, p.y);
        g.part(Metal).cylinder(w, r, r * 0.9, 0.0, h, 12, rgba(steel));
        // Platforms ring the column.
        for k in 1..(h / 6.0) as i32 {
            g.part(Matte).cylinder(w, r + 0.6, r + 0.6, k as f32 * 6.0, k as f32 * 6.0 + 0.15, 12, rgba(mul(pal.warning_light, 0.35)));
        }
        aviation_light_local(g, f, pal, p.x, h + 0.4, p.y, 0.3);
        tops.push(Vec3::new(p.x, h * 0.6, p.y));
    }
    if tops.len() >= 2 {
        let a = tops[0];
        let b = tops[tops.len() - 1];
        pipe_run(g, f, &[Vec3::new(a.x, 6.0, a.z + 2.5), Vec3::new(b.x, 6.0, b.z + 2.5)], 0.35, mul(pal.steel, 0.75));
        pipe_run(g, f, &[Vec3::new(a.x, 4.5, a.z + 2.5), Vec3::new(b.x, 4.5, b.z + 2.5)], 0.25, mul(pick(rng, &pal.metal_paint), 0.9));
        for t in &tops {
            if c.inside_local(Vec2::new(t.x, t.z + 2.5)) {
                f.bx(g, Matte, t.x, 3.0, t.z + 2.5, Vec3::new(0.3, 6.0, 0.3), CHARCOAL);
            }
        }
    }
    // Spherical-ish storage (short fat tanks) at the back.
    for k in 0..3 {
        let p = Vec2::new(lo.x + (k as f32 + 0.5) * (hi.x - lo.x) / 3.0, hi.y - 4.0);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 3.5 {
            tank_vertical(g, f, p, 3.0, 0.0, 4.5, Metal, mul(pal.steel, 1.15));
        }
    }
    // A cracking unit: a boxy furnace with a stack.
    let fu = Vec2::new(lo.x + 4.0, lo.y + 4.0);
    let furnace = fit_inside(c, &rect(fu.x, fu.y, 3.0, 2.5));
    if nd_sim::zoning::polygon_area(&furnace).abs() > 10.0 {
        extrude(g, f, Rust, &furnace, 0.0, 7.0, mul(pal.rust, 0.9));
        let stack = poly_center(&furnace);
        if nd_sim::zoning::point_in_polygon(stack, &furnace) && distance_to_boundary(stack, &furnace) > 0.7 {
            chimney(g, f, stack, 0.6, 7.0, 14.0, mul(pal.steel, 0.6), pal.aviation_light);
        }
    }
    // Flare stacks, burning.
    for (k, p) in [Vec2::new(hi.x - 2.0, lo.y + 2.0), Vec2::new(hi.x - 2.0, hi.y - 9.0)].into_iter().enumerate() {
        if k == 1 && !coin(rng, 0.5) {
            continue;
        }
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
            let h = rng.random_range(22.0..34.0);
            chimney(g, f, p, 0.4, 0.0, h, mul(pal.steel, 0.65), [2.6, 1.1, 0.25]);
            let w = f.to_world(p.x, p.y);
            g.part(Glow).cylinder(w, 0.5, 0.05, h + 0.6, h + 3.2, 6, rgba([3.0, 1.3, 0.3]));
        }
    }
    let front = c.front_face(&lot);
    sign_board(g, c, rng, &front, front.len * 0.1, 2.4, (front.len * 0.3).min(8.0), 0.8);
    for p in [front.at(-front.len * 0.45, -1.5), front.at(front.len * 0.45, -1.5)] {
        floodlight(g, c, p, 10.0);
    }
}

// ── container yard ───────────────────────────────────────────────────────────

/// Container stacks in blocks with a gantry and reach stackers, a lorry
/// loading lane along the front.
pub fn container_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.5));
    fence_lot(g, c, &lot, 2.6, true);
    let (lo, hi) = bounds(&offset_poly(&lot, 1.5));
    // Loading lane along the front, with lorries.
    let lane_v = lo.y + 4.0;
    let front = c.front_face(&lot);
    for k in 0..3 {
        let p = Vec2::new(lo.x + 7.0 + k as f32 * 13.0, lane_v);
        if vehicle_fits(c, p, 0.0, Vehicle::Truck) && coin(rng, 0.6) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, 0.0, Vehicle::Truck, col);
        }
    }
    let line = clip_v(&offset_poly(&lot, 1.0), lane_v + 3.1, lane_v + 3.25);
    if line.len() >= 3 {
        ground(g, f, Glow, &line, 0.06, mul(pal.lane_marker, 0.6));
    }
    // Stacks: rows of 12 m boxes, two to four high.
    let mut v = lane_v + 6.0;
    let mut rows = 0;
    while v + 2.6 < hi.y && rows < 12 {
        let mut u = lo.x + 6.5;
        while u + 6.1 < hi.x {
            let p = Vec2::new(u, v);
            if c.inside_local(p + Vec2::new(-6.2, -1.3)) && c.inside_local(p + Vec2::new(6.2, 1.3)) && c.inside_local(p + Vec2::new(6.2, -1.3)) && c.inside_local(p + Vec2::new(-6.2, 1.3)) && !coin(rng, 0.15) {
                let high = rng.random_range(2..=4);
                for k in 0..high {
                    let col = pick(rng, &pal.container_colors);
                    container_unit(g, c, rng, u, v, 0.0, 12.1, 2.44, 2.6, k as f32 * 2.6, col);
                }
            }
            u += 13.0;
        }
        v += if rows % 2 == 0 { 2.8 } else { 6.0 };
        rows += 1;
    }
    // A rail-mounted gantry over the stacks.
    let (gu, gv0, gv1) = ((lo.x + hi.x) * 0.5, lane_v + 4.0, hi.y - 1.0);
    let posts = [Vec2::new(gu - 2.0, gv0), Vec2::new(gu + 2.0, gv0), Vec2::new(gu - 2.0, gv1 + 1.0), Vec2::new(gu + 2.0, gv1 + 1.0), Vec2::new(gu, gv0 - 1.0)];
    if hi.y - lane_v > 14.0 && hi.x - lo.x > 14.0 && c.all_inside(&posts) {
        gantry_crane(g, c, gu, gv0, gv1, 14.0);
    }
    // Gatehouse and lights.
    let gate = front.at(front.len * 0.42, -2.5);
    if poly_inside(c, &rect(gate.x, gate.y, 1.4, 1.2)) {
        extrude(g, f, Panel, &rect(gate.x, gate.y, 1.4, 1.2), 0.0, 2.8, pick(rng, &pal.panel));
    }
    for p in [front.at(-front.len * 0.45, -1.5), Vec2::new(lo.x + 1.5, hi.y - 1.5), Vec2::new(hi.x - 1.5, hi.y - 1.5)] {
        floodlight(g, c, p, 14.0);
    }
}

// ── fish processing ──────────────────────────────────────────────────────────

/// Long processing sheds, an ice plant with its cooling towers, crates and
/// a boat on blocks.
pub fn fish_processing(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Paving, mul(pal.paving, 0.85));
    let (lo, hi) = bounds(&offset_poly(&lot, 1.5));
    let depth = hi.y - lo.y;
    // Processing shed along the back half.
    let shed_poly = fit_inside(c, &clip_v(&offset_poly(&c.poly, 0.5), lo.y + depth * 0.45, hi.y));
    let paint = mix([0.75, 0.82, 0.86], pick(rng, &pal.panel), 0.3);
    let h = 3.0 + 3.2 * c.floors.min(2) as f32;
    if shed_poly.len() >= 3 && nd_sim::zoning::polygon_area(&shed_poly).abs() > 20.0 {
        shed(g, c, rng, &shed_poly, h, paint, Metal);
        let sf = c.front_face(&shed_poly);
        sign_board(g, c, rng, &sf, sf.len * 0.25, h - 1.0, (sf.len * 0.3).min(7.0), 0.9);
        band_windows(g, c, rng, &sf, 0.5, 1, 2.6);
    }
    // Ice plant: a white block with cooling fans on the roof.
    let ice = fit_inside(c, &rect(lo.x + 4.0, lo.y + depth * 0.25, 3.0, (depth * 0.15).clamp(2.0, 3.5)));
    if nd_sim::zoning::polygon_area(&ice).abs() > 10.0 {
        extrude(g, f, Panel, &ice, 0.0, 5.0, [0.85, 0.88, 0.9]);
        let ic = poly_center(&ice);
        chiller(g, c, ic, 5.0, 2.4, 1.6, 0.0, mul(pal.steel, 0.9));
        vent_stack(g, f, ic + Vec2::new(1.5, 0.0), 5.0, 0.3, 1.8, mul(pal.steel, 0.7));
    }
    // Crates and nets in the yard, a refrigerated lorry at the dock.
    for _ in 0..rng.random_range(3..7) {
        if let Some(p) = c.random_inside(rng, 2.0) {
            if !nd_sim::zoning::point_in_polygon(p, &shed_poly) && !nd_sim::zoning::point_in_polygon(p, &ice) {
                for k in 0..rng.random_range(1..4) {
                    f.bx_yaw(g, Matte, p.x, 0.3 + k as f32 * 0.6, p.y, Vec3::new(1.2, 0.55, 0.8), mix([0.2, 0.45, 0.75], [0.85, 0.5, 0.15], (k % 2) as f32), rng.random_range(-0.2..0.2));
                }
            }
        }
    }
    let front = c.front_face(&lot);
    let truck = front.at(front.len * 0.15, -4.0);
    if vehicle_fits(c, truck, front.yaw(), Vehicle::Truck) {
        vehicle(g, f, pal, rng, truck, 0.0, front.yaw(), Vehicle::Truck, [0.88, 0.9, 0.92]);
    }
    // A boat hauled out on blocks.
    if let Some(p) = c.random_inside(rng, 5.0) {
        if !nd_sim::zoning::point_in_polygon(p, &shed_poly) && distance_to_boundary(p, &c.lot_poly) > 4.0 {
            let hull = mix([0.15, 0.3, 0.55], pick(rng, &pal.metal_paint), 0.3);
            f.bx(g, Wood, p.x, 0.8, p.y, Vec3::new(7.0, 1.4, 2.4), hull);
            f.bx(g, Wood, p.x - 1.0, 2.1, p.y, Vec3::new(2.4, 1.4, 1.8), [0.85, 0.85, 0.82]);
            f.bx(g, Matte, p.x + 0.8, 3.6, p.y, Vec3::new(0.12, 3.0, 0.12), CHARCOAL);
        }
    }
    for p in [front.at(-front.len * 0.45, -1.5), front.at(front.len * 0.45, -1.5)] {
        floodlight(g, c, p, 8.0);
    }
}

// ── truck and bus lot ────────────────────────────────────────────────────────

/// Marked bays for lorries and buses, a fuel island and a small office;
/// the share of bays in use follows the sim (`Ctx::parking`).
pub fn truck_lot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.3));
    fence_lot(g, c, &lot, 2.2, false);
    let (lo, hi) = bounds(&offset_poly(&lot, 1.0));
    let occupancy = c.parking.unwrap_or(0.5).clamp(0.0, 1.0);
    // Long bays across the lot, nose to the back, an aisle at the front.
    let bay_w = 4.0;
    let bay_d = 13.0;
    let v0 = lo.y + 7.0;
    let mut bays: Vec<Vec2> = Vec::new();
    let mut v = v0;
    while v + bay_d <= hi.y + 0.1 && bays.len() < 40 {
        let mut u = lo.x + bay_w * 0.5;
        while u + bay_w * 0.5 <= hi.x {
            let p = Vec2::new(u, v + bay_d * 0.5);
            if c.all_inside(&[Vec2::new(-bay_w * 0.5, -bay_d * 0.5), Vec2::new(bay_w * 0.5, bay_d * 0.5), Vec2::new(-bay_w * 0.5, bay_d * 0.5), Vec2::new(bay_w * 0.5, -bay_d * 0.5)].map(|q| p + q)) {
                bays.push(p);
            }
            u += bay_w;
        }
        v += bay_d + 7.0;
    }
    for p in &bays {
        for s in [-1.0f32, 1.0] {
            f.flat(g, Glow, p.x + s * bay_w * 0.5, 0.05, p.y, 0.12, bay_d * 0.95, 0.0, mul(pal.lane_marker, 0.6));
        }
    }
    let n = ((bays.len() as f32) * occupancy).round() as usize;
    let mut order: Vec<usize> = (0..bays.len()).collect();
    // A fixed order from the seed, so the lot fills the same way.
    order.sort_by_key(|i| (nd_sim::rng_for(c.seed, *i as u64 + 91).random::<u32>(), *i));
    for &i in order.iter().take(n) {
        let p = bays[i];
        let yaw = std::f32::consts::FRAC_PI_2;
        let kind = if i % 4 == 3 { Vehicle::Bus } else { Vehicle::Truck };
        let col = if kind == Vehicle::Bus { mix(pal.hi_vis, [0.9, 0.9, 0.85], 0.5) } else { pick(rng, &pal.car_colors) };
        if vehicle_fits(c, p, yaw, kind) {
            vehicle(g, f, pal, rng, p, 0.0, yaw, kind, col);
        }
    }
    // Fuel island and a portakabin office by the gate.
    let front = c.front_face(&lot);
    let fuel = front.at(-front.len * 0.3, -3.5);
    if poly_inside(c, &rect(fuel.x, fuel.y, 2.5, 1.2)) {
        f.bx(g, Concrete, fuel.x, 0.15, fuel.y, Vec3::new(5.0, 0.3, 1.6), mul(pal.concrete(0), 0.9));
        for k in [-1.0f32, 1.0] {
            f.bx(g, Metal, fuel.x + k * 1.2, 0.95, fuel.y, Vec3::new(0.6, 1.3, 0.5), mul(pal.warning_light, 0.5));
        }
        f.bx(g, Matte, fuel.x, 4.2, fuel.y, Vec3::new(6.0, 0.3, 3.0), mul(pal.steel, 0.9));
        for k in [-1.0f32, 1.0] {
            f.bx(g, Matte, fuel.x + k * 2.6, 2.1, fuel.y, Vec3::new(0.2, 4.2, 0.2), mul(pal.steel, 0.7));
        }
    }
    let office = front.at(front.len * 0.35, -3.0);
    if poly_inside(c, &rect(office.x, office.y, 3.0, 1.3)) {
        let col = pick(rng, &pal.container_colors);
        container_unit(g, c, rng, office.x, office.y, 0.0, 6.05, 2.44, 2.6, 0.0, col);
        let of = Face::new(office + Vec2::new(-3.0, -1.22), office + Vec2::new(3.0, -1.22), Vec2::new(0.0, -1.0));
        of.decal(g, f, Glow, 0.8, 1.5, 0.04, 1.2, 0.7, mul(c.style.window_color(0), 0.9));
    }
    for p in [front.at(-front.len * 0.46, -1.0), front.at(front.len * 0.46, -1.0), Vec2::new(lo.x + 1.0, hi.y - 1.0), Vec2::new(hi.x - 1.0, hi.y - 1.0)] {
        floodlight(g, c, p, 9.0);
    }
}
