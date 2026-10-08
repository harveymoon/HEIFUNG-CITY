//! Plants and docks that live by the water or the hot ground: a run-of-river
//! hydro station, a tidal turbine house, a geothermal well field, a shipping
//! port and a ferry terminal. Lots front the street at low `v`; for the
//! water plants and docks the water lies past the back edge (high `v`). The
//! pier, dam or barrage out in the water is drawn by `terrain::waterworks`,
//! which knows where the water is; everything here stays on the lot.

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Turbine hall at the back by the water, penstock pipes, switchyard in front.
pub fn hydro(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Concrete, mul(pal.concrete(0), 0.8));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, hi.y - d * 0.22, w * 0.4, d * 0.18));
    let h = 12.0;
    let wall = mul(pal.concrete(1), 0.85);
    extrude(g, f, Concrete, &hall, 0.0, h, wall);
    cornice(g, c, &hall, h - 0.4, 0.8, 0.3, Concrete, mul(wall, 0.85));
    for face in c.faces(&hall) {
        if face.len > 6.0 {
            band_windows(g, c, rng, &face, 3.0, 2, 3.6);
            grime(g, c, rng, &face, 0.0, h, 2.0);
        }
    }
    // Penstocks: three big pipes from the back edge into the hall.
    let hc = poly_center(&hall);
    for k in 0..3 {
        let u = hc.x + (k as f32 - 1.0) * w * 0.14;
        let back = Vec2::new(u, hi.y - 0.8);
        if !c.inside_local(back) || distance_to_boundary(back, &c.lot_poly) < 1.3 {
            continue;
        }
        let a = f.at(u, 1.6, hi.y - 0.8);
        let b = f.at(u, 1.6, hc.y);
        g.part(Metal).tube(a, b, 1.1, 10, rgba(mul(pal.steel, 0.7)));
    }
    // Gantry crane over the hall roof for the turbines.
    f.bx(g, Metal, hc.x, h + 2.0, hc.y, Vec3::new(w * 0.5, 0.6, 0.6), mul(pal.metal_paint(2), 0.9));
    // Switchyard in front: transformers and a lattice of bus bars.
    let sy = Vec2::new(lo.x + w * 0.25, lo.y + d * 0.28);
    let pen = rect(sy.x, sy.y, (w * 0.18).min(9.0), (d * 0.14).min(6.0));
    if (w * 0.18).min(9.0) >= 5.5 && poly_inside(c, &pen) {
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.0, None, true);
        for k in 0..3 {
            let p = sy + Vec2::new((k as f32 - 1.0) * 3.2, 0.0);
            f.bx(g, Metal, p.x, 1.2, p.y, Vec3::new(1.6, 2.4, 1.2), mul(pal.metal_paint(1), 0.9));
            f.bx(g, Matte, p.x, 3.4, p.y, Vec3::new(0.15, 2.0, 0.15), [0.6, 0.6, 0.55]);
        }
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.2, false);
    floodlight(g, c, front.at(front.len * 0.35, -1.5), 9.0);
    sign_board(g, c, rng, &front, -front.len * 0.2, 2.6, (front.len * 0.35).min(8.0), 0.8);
}

/// A long low turbine house on the shore with sluice gates at the back.
pub fn tidal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Concrete, mul(pal.concrete(2), 0.75));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let house = fit_inside(c, &rect((lo.x + hi.x) * 0.5, hi.y - d * 0.2, w * 0.45, d * 0.15));
    let h = 8.0;
    let wall = mul(pick(rng, &pal.panel), 0.8);
    extrude(g, f, Panel, &house, 0.0, h, wall);
    for face in c.faces(&house) {
        if face.len > 6.0 {
            band_windows(g, c, rng, &face, 2.5, 1, 3.5);
        }
    }
    // Sluice gates along the back: tall frames with a lifting gear each.
    let (hlo, hhi) = bounds(&house);
    let gates = ((hhi.x - hlo.x) / 7.0).floor().max(1.0) as i32;
    for k in 0..gates {
        let u = hlo.x + (k as f32 + 0.5) * (hhi.x - hlo.x) / gates as f32;
        let v = hhi.y.min(hi.y - 0.6);
        f.bx(g, Metal, u, h + 2.0, v, Vec3::new(4.5, 4.0, 0.6), mul(pal.steel, 0.65));
        f.bx(g, Metal, u, h + 4.4, v, Vec3::new(1.4, 0.8, 1.2), mul(pal.metal_paint(3), 0.9));
    }
    // Cable drum and a transformer out front.
    let p = Vec2::new(lo.x + w * 0.3, lo.y + d * 0.3);
    if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 3.0 {
        f.bx(g, Metal, p.x, 1.3, p.y, Vec3::new(2.4, 2.6, 1.8), mul(pal.metal_paint(1), 0.9));
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.2, false);
    floodlight(g, c, front.at(-front.len * 0.35, -1.5), 9.0);
    sign_board(g, c, rng, &front, front.len * 0.2, 2.6, (front.len * 0.35).min(8.0), 0.8);
}

/// Well heads, a separator, a cooling tower and pipes looping across the yard.
pub fn geothermal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Dirt, mul(pal.gravel, 0.9));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let plant = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + d * 0.62, w * 0.22, d * 0.14));
    extrude(g, f, Metal, &plant, 0.0, 10.0, mul(pick(rng, &pal.metal_paint), 0.85));
    let pc = poly_center(&plant);
    // A squat cooling tower with steam.
    let tower = Vec2::new(lo.x + w * 0.78, lo.y + d * 0.7);
    if c.inside_local(tower) && distance_to_boundary(tower, &c.lot_poly) > 9.0 {
        let tw = f.to_world(tower.x, tower.y);
        g.part(Concrete).cylinder(tw, 5.5, 4.2, 0.0, 14.0, 16, rgba(mul(pal.concrete(0), 0.9)));
        for s in 0..4 {
            let y = 14.5 + s as f32 * 2.6;
            g.part(Matte).cylinder(tw + Vec2::splat(s as f32 * 0.4), 3.6 + s as f32 * 0.7, 4.0 + s as f32 * 0.7, y, y + 2.4, 10, rgba([0.86, 0.87, 0.88]));
        }
    }
    // Well heads with pipes to the plant.
    // [plant-scale] More wells on a bigger plant.
    let wells = (((3 + rng.random_range(0..3)) as f32 * c.plant_scale).round() as i32).clamp(2, 10);
    for k in 0..wells {
        let p = Vec2::new(lo.x + w * (0.15 + 0.7 * k as f32 / wells as f32), lo.y + d * (0.25 + 0.12 * (k % 2) as f32));
        if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 2.0 {
            continue;
        }
        f.bx(g, Metal, p.x, 1.0, p.y, Vec3::new(1.4, 2.0, 1.4), mul(pal.steel, 0.8));
        f.bx(g, Metal, p.x, 2.3, p.y, Vec3::new(0.5, 0.6, 0.5), mul(pal.metal_paint(4), 0.9));
        pipe_run(g, f, &[Vec3::new(p.x, 2.0, p.y), Vec3::new(p.x, 2.6, pc.y - 4.0), Vec3::new(pc.x, 2.6, pc.y - 4.0)], 0.35, mul(pal.steel, 0.9));
        // A wisp of steam from the bleed valve.
        let wp = f.to_world(p.x, p.y);
        g.part(Matte).cylinder(wp, 0.4, 0.9, 2.6, 5.0, 6, rgba([0.85, 0.86, 0.87]));
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.2, true);
    floodlight(g, c, front.at(front.len * 0.4, -1.5), 9.0);
    sign_board(g, c, rng, &front, 0.0, 2.6, (front.len * 0.3).min(8.0), 0.8);
}

/// Warehouse at the gate, container stacks, straddle carriers and a ship-to-
/// shore crane on the quay at the back.
pub fn shipping_port(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.25));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    // Quay edge along the back.
    let quay = rect((lo.x + hi.x) * 0.5, hi.y - 2.0, w * 0.49, 1.6);
    if poly_inside(c, &quay) {
        extrude(g, f, Concrete, &quay, 0.0, 0.5, mul(pal.concrete(0), 0.9));
        for k in 0..((w / 9.0) as i32) {
            let u = lo.x + 4.0 + k as f32 * 9.0;
            f.bx(g, Metal, u, 0.8, hi.y - 1.0, Vec3::new(0.6, 0.6, 0.6), mul(pal.steel, 0.4));
        }
    }
    // Shed by the gate.
    let shed = fit_inside(c, &rect(lo.x + w * 0.2, lo.y + d * 0.2, w * 0.16, d * 0.13));
    let shed_h = 9.0;
    extrude(g, f, Metal, &shed, 0.0, shed_h, mul(pick(rng, &pal.metal_paint), 0.8));
    for face in c.faces(&shed) {
        if face.len > 8.0 && face.out.y < -0.5 {
            roller_door(g, c, &face, 0.0, 5.0, 5.0, mul(pal.steel, 0.7));
        }
    }
    // Container stacks in rows.
    let colors = &pal.container_colors;
    for row in 0..3 {
        let v = lo.y + d * (0.42 + row as f32 * 0.12);
        let mut u = lo.x + w * 0.42;
        while u < hi.x - 8.0 {
            let tiers = rng.random_range(1..4);
            for t in 0..tiers {
                let col = colors[rng.random_range(0..colors.len().max(1)) % colors.len().max(1)];
                if poly_inside(c, &rect(u, v, 6.4, 1.4)) {
                    container_unit(g, c, rng, u, v, 0.0, 12.2, 2.45, 2.6, t as f32 * 2.6, col);
                }
            }
            u += 13.0;
        }
    }
    // Ship-to-shore crane straddling the quay: legs, boom out over the water.
    let cu = lo.x + w * 0.62;
    let legs = [Vec2::new(cu - 8.0, hi.y - 3.0), Vec2::new(cu + 8.0, hi.y - 3.0), Vec2::new(cu - 8.0, hi.y - 15.0), Vec2::new(cu + 8.0, hi.y - 15.0)];
    if legs.iter().all(|p| c.inside_local(*p)) && poly_inside(c, &rect(cu, hi.y - 11.0, 9.5, 10.5)) {
        let crane = mul(pal.metal_paint(0), 1.0);
        for p in legs {
            f.bx(g, Metal, p.x, 14.0, p.y, Vec3::new(1.0, 28.0, 1.0), crane);
        }
        f.bx(g, Metal, cu, 28.5, hi.y - 9.0, Vec3::new(18.0, 1.4, 14.0), crane);
        // The boom reaches back over the land only as far as the lot allows;
        // the seaward part is drawn with the pier.
        f.bx(g, Metal, cu, 31.0, hi.y - 12.0, Vec3::new(2.0, 1.2, 18.0), mul(crane, 0.9));
        g.part(Glow).boxed(f.at(cu, 32.0, hi.y - 3.0), Vec3::splat(0.6), f.yaw(), rgba(pal.aviation_light));
    }
    // Straddle carriers and a lorry at the gate.
    for k in 0..2 {
        let p = Vec2::new(lo.x + w * (0.5 + 0.2 * k as f32), lo.y + d * 0.3);
        if poly_inside(c, &rect(p.x, p.y, 3.0, 2.0)) {
            for (du, dv) in [(-2.4, -1.6), (2.4, -1.6), (-2.4, 1.6), (2.4, 1.6)] {
                f.bx(g, Metal, p.x + du, 3.5, p.y + dv, Vec3::new(0.4, 7.0, 0.4), mul(pal.metal_paint(3), 1.0));
            }
            f.bx(g, Metal, p.x, 7.2, p.y, Vec3::new(5.4, 0.8, 3.6), mul(pal.metal_paint(3), 0.9));
        }
    }
    let lorry = Vec2::new(lo.x + w * 0.38, lo.y + 6.0);
    if vehicle_fits(c, lorry, 0.0, Vehicle::Truck) {
        let paint = pick(rng, &pal.metal_paint);
        vehicle(g, f, pal, rng, lorry, 0.0, 0.0, Vehicle::Truck, paint);
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.6, true);
    floodlight(g, c, front.at(-front.len * 0.4, -1.5), 14.0);
    floodlight(g, c, front.at(front.len * 0.4, -1.5), 14.0);
    floodlight(g, c, Vec2::new(lo.x + w * 0.35, hi.y - 6.0), 16.0);
    sign_board(g, c, rng, &front, -front.len * 0.15, 3.0, (front.len * 0.3).min(12.0), 1.0);
}

/// A glazed waiting hall with a curved roof, a canopy to the gangway, a
/// forecourt with a taxi rank, and a timetable board.
pub fn ferry_terminal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.3, Paving, mul(pal.paving, 1.0));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + d * 0.55, w * 0.32, d * 0.2));
    let h = 7.0;
    let glass = pick(rng, &pal.glass);
    extrude(g, f, Glass, &hall, 0.0, h, glass);
    // A shallow vaulted roof in strips.
    let (hlo, hhi) = bounds(&hall);
    for k in 0..6 {
        let t = k as f32 / 5.0;
        let v = hlo.y + (hhi.y - hlo.y) * t;
        let y = h + 1.6 * (1.0 - (2.0 * t - 1.0).powi(2));
        f.bx(g, Metal, (hlo.x + hhi.x) * 0.5, y, v, Vec3::new(hhi.x - hlo.x + 1.0, 0.3, (hhi.y - hlo.y) / 5.0 + 0.3), mul(pal.steel, 1.05));
    }
    for face in c.faces(&hall) {
        if face.out.y < -0.5 && face.len > 6.0 {
            entrance(g, c, rng, &face, 0.0, 4.0, 3.2);
        }
    }
    // Canopy from the hall to the back edge, where the gangway meets the pier.
    let cu = (hlo.x + hhi.x) * 0.5;
    let v0 = hhi.y;
    let v1 = hi.y - 0.5;
    if v1 > v0 + 1.0 && poly_inside(c, &rect(cu, (v0 + v1) * 0.5, 1.8, (v1 - v0) * 0.5)) {
        f.bx(g, Metal, cu, 3.4, (v0 + v1) * 0.5, Vec3::new(3.2, 0.2, v1 - v0), mul(pal.steel, 0.9));
        for k in 0..=((v1 - v0) / 4.0) as i32 {
            let v = v0 + k as f32 * 4.0;
            for du in [-1.5, 1.5] {
                f.bx(g, Metal, cu + du, 1.7, v, Vec3::new(0.15, 3.4, 0.15), mul(pal.steel, 0.7));
            }
        }
    }
    // Timetable board and lamps on the forecourt.
    let front = c.front_face(&yard);
    led_screen(g, c, rng, &front, front.len * 0.25, 2.2, 3.0, 1.4);
    for k in 0..3 {
        let p = Vec2::new(lo.x + w * (0.2 + 0.3 * k as f32), lo.y + d * 0.2);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
            lamp_post_in(g, c, p, 4.5, pal.lamp);
        }
    }
    let taxi = Vec2::new(lo.x + w * 0.22, lo.y + d * 0.32);
    if vehicle_fits(c, taxi, 0.0, Vehicle::Car) {
        vehicle(g, f, pal, rng, taxi, 0.0, 0.0, Vehicle::Car, [0.9, 0.75, 0.1]);
    }
    sign_board(g, c, rng, &front, -front.len * 0.25, 3.0, (front.len * 0.3).min(9.0), 0.9);
}
