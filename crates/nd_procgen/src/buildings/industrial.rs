//! Industry: junkyards and rusted sheds where land is cheap, saw-tooth
//! factories and tank farms in the middle, datacentres and container depots
//! where it is dear.

use super::parts::*;
use super::{Ctx, Face};
use crate::palette::RUST;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Shrink a polygon towards its centroid along one axis only, which turns a
/// footprint into a roof ridge.
fn squash(poly: &[Vec2], axis: Vec2, k: f32) -> Vec<Vec2> {
    let c = poly_center(poly);
    poly.iter()
        .map(|p| {
            let d = *p - c;
            let along = d.dot(axis);
            c + d - axis * along * (1.0 - k)
        })
        .collect()
}

fn gable_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, ridge: f32, color: [f32; 3]) {
    let f = &c.frame;
    let front = c.front_face(poly);
    let axis = front.out;
    let top = squash(poly, axis, 0.04);
    loft(g, f, Metal, &offset_poly(poly, -0.4), &top, y, y + ridge, color);
}

// ── junkyard ─────────────────────────────────────────────────────────────────

pub fn junkyard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = offset_poly(&c.lot_poly, 0.4);
    ground(g, f, Dirt, &poly, 0.03, pal.dirt);
    let front = c.front_face(&poly);
    let front_idx = c.faces(&poly).iter().position(|x| x.a.distance(front.a) < 0.1);
    fence_poly(g, c, &poly, 2.4, front_idx, true);

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
            vehicle(g, f, rng, p, yaw, kind, col);
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
    if c.inside(office) {
        let col = pick(rng, &pal.container_colors);
        container_unit(g, c, rng, office.x, office.y, 0.0, 6.05, 2.44, 2.6, 0.0, col);
        let of = Face::from_edge(office + Vec2::new(3.0, -1.22), office + Vec2::new(-3.0, -1.22));
        let of = Face { out: Vec2::new(0.0, -1.0), tangent: Vec2::X, ..of };
        of.bx(g, f, Glow, -1.0, 1.5, 0.06, Vec3::new(1.0, 0.7, 0.04), mul(c.style.window_colors[0], 0.9));
        sign_board(g, c, rng, &of, 0.5, 2.3, 4.0, 0.5);
        dish(g, f, office, 2.6, 0.6, pal.steel);
    }
    // Gate sign on the fence line.
    sign_board(g, c, rng, &front, -front.len * 0.25, 2.9, (front.len * 0.3).min(6.0), 0.7);
    // Crane claw on a small gantry over the biggest pile.
    if c.fw > 18.0 && c.fd > 14.0 {
        gantry_crane(g, c, 0.0, -c.fd * 0.2, c.fd * 0.25, 7.0);
    }
    for k in 0..2 {
        let p = front.at((k as f32 - 0.5) * front.len * 0.7, -2.0);
        if c.inside(p) {
            floodlight(g, c, p, 8.0);
        }
    }
    drum_fire(g, f, front.at(-front.len * 0.1, -3.0));
    // Dog house and weeds.
    if let Some(p) = c.random_inside(rng, 2.0) {
        f.bx(g, Wood, p.x, 0.5, p.y, Vec3::new(1.2, 1.0, 1.0), pal.plywood);
    }
    weeds(g, c, rng, &poly, 12);
}

pub fn weeds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], n: i32) {
    let f = &c.frame;
    let (lo, hi) = bounds(poly);
    for _ in 0..n {
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        if !nd_sim::zoning::point_in_polygon(p, poly) {
            continue;
        }
        let s = rng.random_range(0.4..1.1);
        f.bx_yaw(g, Foliage, p.x, s * 0.3, p.y, Vec3::new(s, s * 0.6, s * 0.8), mul(c.palette.foliage_dry, rng.random_range(0.7..1.1)), rng.random_range(0.0..1.5));
    }
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
            face.bx(g, f, Rust, t, rng.random_range(1.5..h - 1.0), 0.03, Vec3::new(rng.random_range(1.0..3.0), rng.random_range(0.8..2.5), 0.04), RUST);
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
            front.bx(g, f, Glow, t, 2.0, -0.3, Vec3::new(2.0, 1.2, 0.05), mul(c.style.window_colors[0], 0.5));
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
        if c.inside(p) {
            for k in 0..rng.random_range(1..4) {
                f.bx_yaw(g, Wood, p.x, 0.08 + k as f32 * 0.16, p.y, Vec3::new(1.2, 0.14, 1.0), pal.wood, rng.random_range(-0.2..0.2));
            }
        }
    }
    let van = front.at(front.len * 0.4, 3.5);
    if c.inside(van) && rng.random_bool(0.6) {
        let col = pick(rng, &pal.car_colors);
        vehicle(g, f, rng, van, front.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Van, col);
    }
    for _ in 0..rng.random_range(2..6) {
        if let Some(p) = c.random_inside(rng, 1.0) {
            if !nd_sim::zoning::point_in_polygon(p, poly) {
                let w = f.to_world(p.x, p.y);
                g.part(if rng.random_bool(0.5) { Rust } else { Metal }).cylinder(w, 0.3, 0.3, 0.0, 0.9, 10, rgba(if rng.random_bool(0.5) { RUST } else { pick(rng, &pal.metal_paint) }));
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
        let top = squash(&inner, axis, 0.08);
        loft(g, f, Metal, &inner, &shift_poly(&top, Vec2::new(0.0, -(v1 - v0) * 0.46)), h, h + 2.2, mul(wall, 0.9));
        if rng.random_bool((0.6 * (1.0 - c.wear)) as f64) {
            let lit = squash(&inner, axis, 0.5);
            extrude(g, f, Glow, &shift_poly(&lit, Vec2::new(0.0, (v1 - v0) * 0.2)), h + 0.6, h + 1.6, mul(c.style.window_colors[0], 0.6));
        }
    }
    // Loading dock and doors on the front.
    let doors = ((front.len / 7.0) as i32).max(1);
    for k in 0..doors {
        let t = -front.len * 0.5 + (k as f32 + 0.5) * (front.len / doors as f32);
        roller_door(g, c, &front, t, 4.0, 4.4, mul(pal.steel, 0.6));
    }
    front.bx(g, f, Concrete, 0.0, 0.6, 1.5, Vec3::new(front.len * 0.85, 1.2, 3.0), mul(wall, 0.8));
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
            if c.inside(p) {
                tank_vertical(g, f, p, 1.8, 0.0, rng.random_range(4.0..8.0), if rng.random_bool(0.5) { Rust } else { Metal }, if rng.random_bool(0.5) { RUST } else { pick(rng, &pal.metal_paint) });
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
    if c.inside(annex_p + Vec2::new(3.5, 3.5)) && c.inside(annex_p + Vec2::new(3.5, -3.5)) && hi.x - lo.x > 20.0 {
        let annex = fit_inside(c, &rect(hi.x - 1.0, annex_p.y, 3.5, 4.5));
        extrude(g, f, Metal, &annex, 0.0, h * 0.6, mul(wall, 0.9));
        let a = f.at(hi.x - 6.0, h * 0.5, annex_p.y);
        let b = f.at(hi.x - 1.0, h * 0.5, annex_p.y);
        g.part(Metal).boxed(a.lerp(b, 0.5), Vec3::new(a.distance(b), 2.0, 2.0), f.yaw(), rgba(mul(wall, 0.85)));
    }
    if rng.random_bool(c.rules.sign_chance as f64) {
        sign_board(g, c, rng, &front, -front.len * 0.25, h - 1.5, (front.len * 0.35).min(10.0), 1.1);
    }
    // Yard: trucks at the docks, floodlights, fence at the sides.
    for k in 0..doors.min(3) {
        if !rng.random_bool(0.5) {
            continue;
        }
        let t = -front.len * 0.5 + (k as f32 + 0.5) * (front.len / doors as f32);
        let p = front.at(t, 7.5);
        if c.inside(p) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, rng, p, front.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Truck, col);
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
fn clip_v(poly: &[Vec2], v0: f32, v1: f32) -> Vec<Vec2> {
    use nd_sim::zoning::clip_halfplane;
    let a = clip_halfplane(poly, Vec2::new(0.0, v0), Vec2::new(0.0, 1.0));
    clip_halfplane(&a, Vec2::new(0.0, v1), Vec2::new(0.0, -1.0))
}

// ── tank farm ────────────────────────────────────────────────────────────────

pub fn tank_farm(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = offset_poly(&c.lot_poly, 0.5);
    ground(g, f, Asphalt, &yard, 0.03, mul(pal.asphalt, 1.6));
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
        if !nd_sim::zoning::point_in_polygon(p, &offset_poly(&bund, rr + 0.5)) {
            continue;
        }
        if tanks.iter().any(|(q, qr)| q.distance(p) < rr + qr + 1.5) {
            continue;
        }
        let h = rng.random_range(5.0..12.0);
        let rusty = rng.random_bool((0.3 + 0.6 * c.wear) as f64);
        tank_vertical(g, f, p, rr, 0.0, h, if rusty { Rust } else { Metal }, if rusty { RUST } else { pick(rng, &pal.metal_paint) });
        let w = f.to_world(p.x, p.y);
        g.part(Glow).boxed(Vec3::new(w.x, h + 1.3, w.y), Vec3::splat(0.3), 0.0, rgba(pal.aviation_light));
        tanks.push((p, rr));
    }
    // Pipe runs linking tank tops and running to a manifold by the gate.
    for w in tanks.windows(2) {
        let (a, ra) = w[0];
        let (b, _) = w[1];
        pipe_run(g, f, &[Vec3::new(a.x, 1.2, a.y + ra), Vec3::new(a.x, 1.2, (a.y + b.y) * 0.5), Vec3::new(b.x, 1.2, (a.y + b.y) * 0.5), Vec3::new(b.x, 1.2, b.y)], 0.22, mul(pal.steel, 0.8));
    }
    let manifold = front.at(front.len * 0.3, 4.0);
    if c.inside(manifold) {
        for k in 0..4 {
            let p = manifold + Vec2::new(k as f32 * 0.8, 0.0);
            f.bx(g, Metal, p.x, 0.8, p.y, Vec3::new(0.3, 1.6, 0.3), mul(pal.steel, 0.8));
            f.bx(g, Matte, p.x, 1.4, p.y, Vec3::new(0.5, 0.15, 0.5), [0.6, 0.1, 0.1]);
        }
        f.bx(g, Metal, manifold.x + 1.2, 0.4, manifold.y + 1.0, Vec3::new(3.6, 0.3, 0.3), mul(pal.steel, 0.8));
    }
    // Pump house and flare stack.
    let ph = front.at(-front.len * 0.3, 5.0);
    if c.inside(ph) {
        let house = fit_inside(c, &rect(ph.x, ph.y, 3.0, 2.5));
        extrude(g, f, Metal, &house, 0.0, 3.5, pick(rng, &pal.metal_paint));
        let hf = c.front_face(&house);
        roller_door(g, c, &hf, 0.0, 2.6, 2.8, mul(pal.steel, 0.6));
        vent_stack(g, f, ph, 3.5, 0.3, 1.5, mul(pal.steel, 0.7));
    }
    if let Some((p, rr)) = tanks.last() {
        let flare = *p + Vec2::new(rr + 3.0, 0.0);
        if c.inside(flare) {
            chimney(g, f, flare, 0.5, 0.0, 18.0, mul(pal.steel, 0.75), [2.6, 1.2, 0.3]);
            let w = f.to_world(flare.x, flare.y);
            g.part(Glow).boxed(Vec3::new(w.x, 18.9, w.y), Vec3::new(0.9, 1.6, 0.9), 0.0, rgba([3.0, 1.4, 0.3]));
        }
    }
    // Fence, floodlights, hazard signs.
    let faces = c.faces(&yard);
    let front_idx = faces.iter().position(|x| x.a.distance(front.a) < 0.1);
    fence_poly(g, c, &yard, 2.4, front_idx, true);
    for p in [front.at(-front.len * 0.4, -1.5), front.at(front.len * 0.4, -1.5)] {
        if c.inside(p) {
            floodlight(g, c, p, 10.0);
        }
    }
    for k in 0..3 {
        front.bx(g, f, Glow, -front.len * 0.3 + k as f32 * front.len * 0.3, 1.6, -0.1, Vec3::new(0.6, 0.6, 0.05), [2.2, 0.3, 0.2]);
    }
}

// ── datacentre ───────────────────────────────────────────────────────────────

pub fn datacenter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let h = (c.floors as f32 * c.fh()).clamp(9.0, 18.0);
    let panel = pick(rng, &pal.panel);
    let yard = offset_poly(&c.lot_poly, 0.5);
    ground(g, f, Asphalt, &yard, 0.03, mul(pal.asphalt, 1.5));
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
        face.bx(g, f, Glow, 0.0, h - 0.9, 0.05, Vec3::new(face.len * 0.96, 0.08, 0.03), mul([0.5, 1.3, 2.2], 0.6));
        // Louvre bands low down.
        face.bx(g, f, Matte, 0.0, 2.2, 0.06, Vec3::new(face.len * 0.9, 1.0, 0.1), mul(panel, 0.5));
    }
    // Roof plant: rows of chillers.
    let (lo, hi) = bounds(&offset_poly(&hall, 3.0));
    let rows = ((hi.y - lo.y) / 6.0).floor().max(1.0) as i32;
    let cols = ((hi.x - lo.x) / 5.0).floor().max(1.0) as i32;
    for r in 0..rows {
        for col in 0..cols {
            let p = Vec2::new(lo.x + (col as f32 + 0.5) * (hi.x - lo.x) / cols as f32, lo.y + (r as f32 + 0.5) * (hi.y - lo.y) / rows as f32);
            if nd_sim::zoning::point_in_polygon(p, &offset_poly(&hall, 2.5)) {
                chiller(g, c, p, h, 4.0, 2.0, 0.0, mul(pal.steel, 0.9));
            }
        }
    }
    antenna_mast(g, c, Vec2::new(hi.x - 1.0, hi.y - 1.0), h, 8.0);
    dish(g, f, Vec2::new(lo.x + 1.0, hi.y - 1.0), h, 1.4, pal.steel);
    // Generator yard beside the hall.
    let front = c.front_face(&c.lot_poly);
    let gen_p = Vec2::new(lo.x - 5.0, (lo.y + hi.y) * 0.5);
    if c.inside(gen_p + Vec2::new(-2.5, 4.0)) && c.inside(gen_p + Vec2::new(-2.5, -4.0)) {
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
    let faces = c.faces(&yard);
    let front_idx = faces.iter().position(|x| x.is_front);
    fence_poly(g, c, &yard, 2.6, front_idx, true);
    let gate = front.at(front.len * 0.3, -2.8);
    if c.inside(gate) {
        let house = fit_inside(c, &rect(gate.x, gate.y, 1.6, 1.4));
        extrude(g, f, Panel, &house, 0.0, 3.0, mul(panel, 0.9));
        curtain_lights(g, c, rng, &house, 1.0, 1, 1.6);
        let hf = c.front_face(&house);
        hf.bx(g, f, Matte, 2.6, 0.5, 0.0, Vec3::new(0.2, 1.0, 0.2), pal.steel);
        hf.bx(g, f, Matte, 2.6, 1.0, -1.5, Vec3::new(0.1, 0.1, 3.2), [0.8, 0.1, 0.1]);
    }
    bollards(g, c, &front, 6, -1.4);
    for p in [front.at(-front.len * 0.42, -1.5), front.at(front.len * 0.42, -1.5)] {
        if c.inside(p) {
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
    let yard = offset_poly(&c.lot_poly, 0.5);
    ground(g, f, Asphalt, &yard, 0.03, mul(pal.asphalt, 1.5));
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
                if c.inside(p) {
                    let col = pick(rng, &pal.car_colors);
                    vehicle(g, f, rng, p, dock.yaw() + std::f32::consts::FRAC_PI_2, Vehicle::Truck, col);
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
                    if !c.inside(Vec2::new(u + len * 0.5, v)) || !c.inside(Vec2::new(u - len * 0.5, v)) {
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
    let faces = c.faces(&yard);
    let front_idx = faces.iter().position(|x| x.a.distance(front.a) < 0.1);
    fence_poly(g, c, &yard, 2.6, front_idx, true);
    for p in [front.at(-front.len * 0.42, -2.0), front.at(front.len * 0.42, -2.0), Vec2::new(lo.x + 2.0, hi.y - depth - 2.0), Vec2::new(hi.x - 2.0, hi.y - depth - 2.0)] {
        if c.inside(p) {
            floodlight(g, c, p, 12.0);
        }
    }
    // Lane markings in the yard.
    for k in 0..3 {
        let t = -front.len * 0.3 + k as f32 * front.len * 0.3;
        front.bx(g, f, Glow, t, 0.06, -5.0, Vec3::new(0.15, 0.02, 8.0), mul(pal.lane_marker, 0.5));
    }
}
