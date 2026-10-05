//! Railway stations and freight yards. Both sit on a lot that fronts a
//! street and back onto the track: the station has its hall at the road
//! and a canopied platform along the rails; the yard has a warehouse at
//! the road, an apron and a gantry crane along the rails.

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

pub fn station(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(link) = c.rail.as_ref() else { return };
    // Forecourt the whole lot, paved.
    ground(g, f, Paving, &offset_poly(&c.lot_poly, 0.3), 0.04, mul(pal.paving, 1.1));
    // Hall: on the road half of the lot, as wide as the lot allows.
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.32, (hi.x - lo.x) * 0.42, (hi.y - lo.y) * 0.22));
    let h = 8.0;
    let brick = pick(rng, &pal.brick);
    extrude(g, f, Brick, &hall, 0.0, h, brick);
    cornice(g, c, &hall, h - 0.5, 1.0, 0.4, Stone, mul(pal.stone, 1.3));
    let front = c.front_face(&hall);
    // Tall arched glass front with a canopy and a clock.
    front.bx(g, f, Glass, 0.0, h * 0.5, 0.06, Vec3::new((front.len * 0.7).max(3.0), h * 0.7, 0.1), pick(rng, &pal.glass));
    front.bx(g, f, Glow, 0.0, h * 0.5, 0.0, Vec3::new((front.len * 0.66).max(2.6), h * 0.62, 0.08), mul(pal.window_warm, 0.55));
    canopy(g, c, &front, 0.0, 4.0, (front.len * 0.8).max(4.0), 3.0, mul(pal.steel, 0.9));
    front.bx(g, f, Panel, 0.0, h + 1.0, -0.6, Vec3::new(2.0, 2.0, 0.6), mul(pal.plaster(0), 1.1));
    front.decal(g, f, Glow, 0.0, h + 1.0, -0.29, 1.2, 1.2, pal.chalk);
    sign_board(g, c, rng, &front, 0.0, h - 1.4, (front.len * 0.6).min(14.0), 1.0);
    for face in c.faces(&hall) {
        if !face.is_front && face.len > 5.0 {
            band_windows(g, c, rng, &face, h * 0.55, 1, 3.0);
        }
    }
    // Platform along the track, with a canopy on posts, benches, lamps,
    // a yellow line and a name board.
    let [pa, pb] = link.platform;
    let along = link.along();
    let out = Vec2::new(-along.y, along.x) * link.side;
    let length = pa.distance(pb);
    let concrete = mul(pal.concrete(0), 1.2);
    let edge_a = pa - out * 1.3;
    let edge_b = pb - out * 1.3;
    g.part(Concrete).path_box(&[edge_a + out * 2.0, edge_b + out * 2.0], 0.0, 4.0, 0.0, 0.95, rgba(concrete));
    g.part(Glow).path_strip(&[edge_a + out * 0.5, edge_b + out * 0.5], 0.0, 0.15, 0.96, rgba(pal.platform_line));
    let posts = ((length / 7.0).floor() as i32).max(2);
    for k in 0..=posts {
        let p = pa.lerp(pb, k as f32 / posts as f32) + out * 2.4;
        g.part(Matte).boxed(Vec3::new(p.x, 2.7, p.y), Vec3::new(0.18, 3.5, 0.18), 0.0, rgba(mul(pal.steel, 0.7)));
        if k % 2 == 0 && k < posts {
            let b = pa.lerp(pb, (k as f32 + 0.5) / posts as f32) + out * 1.6;
            bench_world(g, b, along.y.atan2(along.x), pal.wood, CHARCOAL);
        }
    }
    g.part(Panel).path_box(&[pa + out * 2.4, pb + out * 2.4], 0.0, 4.4, 4.4, 4.7, rgba(mul(pick(rng, &pal.metal_paint), 0.9)));
    g.part(Glow).path_box(&[pa + out * 2.4, pb + out * 2.4], 0.0, 0.3, 4.3, 4.4, rgba(mul(pal.lamp, 0.7)));
    for t in [0.3f32, 0.7] {
        let p = pa.lerp(pb, t) + out * 1.2;
        g.part(Matte).boxed(Vec3::new(p.x, 2.0, p.y), Vec3::new(0.1, 2.2, 0.1), 0.0, rgba(mul(pal.steel, 0.6)));
        g.part(Panel).boxed(Vec3::new(p.x, 3.2, p.y), Vec3::new(2.4, 0.5, 0.06), along.y.atan2(along.x), rgba([0.1, 0.2, 0.5]));
        g.part(Glow).boxed(Vec3::new(p.x, 3.2, p.y), Vec3::new(2.2, 0.3, 0.07), along.y.atan2(along.x), rgba([1.3, 1.3, 1.2]));
    }
    // Path from the hall to the platform, ramped up.
    let hc = poly_center(&hall);
    let hw = f.to_world(hc.x, hc.y);
    let pc = link.centre() + out * 2.8;
    g.part(Paving).path_strip(&[hw, pc], 0.0, 3.0, 0.05, rgba(mul(pal.paving, 1.2)));
    g.part(Concrete).slab(Vec3::new(pc.x + (hw.x - pc.x) * 0.25, 0.05, pc.y + (hw.y - pc.y) * 0.25), Vec3::new(pc.x, 0.95, pc.y), 3.0, 0.15, rgba(concrete));
    // Forecourt: taxi rank, bike racks, bins, bollards, a lamp.
    let gate = c.front_face(&offset_poly(&c.lot_poly, 0.3));
    for s in [-1.0f32, 1.0] {
        let p = gate.at(s * gate.len * 0.3, -4.0);
        if c.inside_local(p) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, 0.0, Vehicle::Car, col);
        }
    }
    for k in 0..4 {
        let p = gate.at(-gate.len * 0.42 + k as f32 * 1.0, -2.0);
        if c.inside_local(p) {
            f.bx(g, Matte, p.x, 0.45, p.y, Vec3::new(0.05, 0.9, 0.8), mul(pal.steel, 0.8));
        }
    }
    bollards(g, c, &gate, 6, -1.0);
    lamp_post(g, f, gate.at(gate.len * 0.42, -1.6), 5.0, pal.lamp);
}

pub fn freight_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(link) = c.rail.as_ref() else { return };
    let yard = yard(g, c, 0.4, Concrete, mul(pal.concrete(0), 0.95));
    // Warehouse by the road, doors to the yard.
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let shed = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.28, (hi.x - lo.x) * 0.4, (hi.y - lo.y) * 0.2));
    let h = 9.0;
    let panel = mul(pick(rng, &pal.panel), 0.85);
    extrude(g, f, Panel, &shed, 0.0, h, panel);
    cornice(g, c, &shed, h - 0.4, 0.8, 0.3, Panel, mul(panel, 0.8));
    for face in c.faces(&shed) {
        if face.len < 6.0 {
            continue;
        }
        let doors = ((face.len / 6.0).floor() as i32).clamp(1, 4);
        for k in 0..doors {
            let t = -face.len * 0.5 + (k as f32 + 0.5) * face.len / doors as f32;
            roller_door(g, c, &face, t, 4.0, 4.5, mul(pal.metal_paint(0), 0.9));
        }
        grime(g, c, rng, &face, 0.0, h, 2.5);
    }
    sign_board(g, c, rng, &c.front_face(&shed), 0.0, h - 1.3, 8.0, 0.9);
    roof_gear(g, c, rng, &shed, h, 0.2);
    // Sidings fan off the main line beside the apron, as many as the yard
    // is long for, under a gantry that spans them all.
    let [pa, pb] = link.platform;
    let along = link.along();
    let out = Vec2::new(-along.y, along.x) * link.side;
    let length = pa.distance(pb);
    // The platform line sits 3.8 m off the track centre; sidings every 4.6 m.
    let main = -(nd_sim::roads::RAIL_HALF_WIDTH + 1.4);
    let sidings = if length > 80.0 { 3 } else { 2 };
    // The points where a siding rejoins the main line are clamped to the
    // lot's own extent along the track, so a yard does not lay rail through
    // its neighbours: a short lot gets short, steep turnouts.
    let (lot_lo, lot_hi) = c.lot_poly.iter().map(|p| (f.to_world(p.x, p.y) - pa).dot(along)).fold((f32::MAX, f32::MIN), |(lo, hi), s| (lo.min(s), hi.max(s)));
    let (lot_lo, lot_hi) = (lot_lo.min(-2.0), lot_hi.max(length + 2.0));
    for k in 1..=sidings {
        let off = main + 4.6 * k as f32;
        let reach = 26.0 + 8.0 * k as f32;
        let s0 = (-reach).max(lot_lo);
        let s1 = (length + reach).min(lot_hi);
        let a0 = pa + along * s0 + out * main;
        let a1 = pa + along * (s0 * 0.25) + out * (main + (off - main) * 0.55);
        let b1 = pa + along * (length + (s1 - length) * 0.75) + out * (main + (off - main) * 0.55);
        let b0 = pa + along * s1 + out * main;
        let ctrl = [a0, a1, pa + out * off, pb + out * off, b1, b0];
        let pts = nd_sim::plan::sample_curve(&ctrl);
        railway_track(g, pal, &pts, 4.4);
    }
    let apron_in = main + 4.6 * sidings as f32 + 3.4;
    let apron_w = 9.0;
    g.part(Concrete).path_box(&[pa + out * (apron_in + apron_w * 0.5), pb + out * (apron_in + apron_w * 0.5)], 0.0, apron_w, 0.0, 0.3, rgba(mul(pal.concrete(0), 1.1)));
    g.part(Glow).path_strip(&[pa + out * (apron_in + 0.3), pb + out * (apron_in + 0.3)], 0.0, 0.12, 0.32, rgba(pal.platform_line));
    // Gantry: legs on the apron and beyond the main line, beam over everything.
    let steel = mul(pal.metal_paint(0), 1.1);
    let far_off = main - 3.0;
    let near_off = apron_in + apron_w - 0.6;
    for t in [0.25f32, 0.75] {
        let p = pa.lerp(pb, t);
        let near = p + out * near_off;
        let far = p + out * far_off;
        for q in [near, far] {
            g.part(Metal).boxed(Vec3::new(q.x, 4.0, q.y), Vec3::new(0.5, 8.0, 0.5), 0.0, rgba(steel));
        }
        g.part(Metal).beam(Vec3::new(near.x, 8.2, near.y), Vec3::new(far.x, 8.2, far.y), 0.6, rgba(steel));
        let hook = near.lerp(far, rng.random_range(0.2..0.8));
        g.part(Matte).boxed(Vec3::new(hook.x, 7.4, hook.y), Vec3::new(1.4, 1.0, 1.4), 0.0, rgba(CHARCOAL));
        g.part(Matte).boxed(Vec3::new(hook.x, 5.5, hook.y), Vec3::new(0.05, 3.0, 0.05), 0.0, rgba(DARK));
    }
    for off in [near_off, far_off] {
        g.part(Metal).beam(Vec3::new(pa.x + out.x * off, 8.4, pa.y + out.y * off), Vec3::new(pb.x + out.x * off, 8.4, pb.y + out.y * off), 0.4, rgba(steel));
    }
    g.part(Glow).boxed(Vec3::new(pa.x + out.x * near_off, 8.9, pa.y + out.y * near_off), Vec3::splat(0.35), 0.0, rgba(pal.aviation_light));
    // Container stacks on the apron's far edge, towards the shed.
    let n = ((length / 7.0) as i32).clamp(1, 10);
    for k in 0..n {
        let p = pa.lerp(pb, (k as f32 + 0.5) / n as f32) + out * (apron_in + apron_w + 1.8);
        let lp = f.to_local(p);
        if !c.inside_local(lp) {
            continue;
        }
        let stack = rng.random_range(1..=3);
        for level in 0..stack {
            let col = pick(rng, &pal.container_colors);
            container_unit(g, c, rng, lp.x, lp.y, along.y.atan2(along.x) - f.yaw(), 6.05, 2.44, 2.6, level as f32 * 2.6, col);
        }
    }
    // Forklift and lorries at the doors.
    let fp = pa.lerp(pb, 0.5) + out * (apron_in + apron_w + 5.5);
    if c.inside_local(f.to_local(fp)) {
        let yaw = rng.random_range(0.0..3.0);
        vehicle(g, f, pal, rng, f.to_local(fp), 0.0, yaw, Vehicle::Van, [0.9, 0.7, 0.1]);
    }
    let front = c.front_face(&shed);
    for s in [-1.0f32, 1.0] {
        let p = front.at(s * front.len * 0.25, 5.5);
        if c.inside_local(p) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, Vehicle::Truck, col);
        }
    }
    // Fence, gate, floodlights, weighbridge.
    let gate = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.4, true);
    barrier(g, c, &gate, gate.len * 0.2, -1.0, 3.5);
    for p in [gate.at(-gate.len * 0.4, -1.5), Vec2::new(lo.x + 2.0, hi.y - 2.0), Vec2::new(hi.x - 2.0, hi.y - 2.0)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 11.0);
        }
    }
    let wb = gate.at(-gate.len * 0.2, -5.0);
    if c.inside_local(wb) {
        f.bx(g, Metal, wb.x, 0.08, wb.y, Vec3::new(3.0, 0.16, 9.0), mul(pal.steel, 0.5));
    }
}
