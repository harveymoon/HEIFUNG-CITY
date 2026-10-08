//! [society] The trash depot (`nd_sim::ServiceKind::TrashDepot`): a fenced
//! asphalt yard with a corrugated transfer hall (roller doors, a tipping
//! floor showing through the open bays), garbage trucks parked in a row,
//! skips and roll-off containers, a weighbridge hut at the gate and, at the
//! top tier, a recycling sorting tower with a conveyor up its side. A low
//! tier is little more than the yard, skips and a shed.

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

const SANITATION_GREEN: [f32; 3] = [0.16, 0.42, 0.22];
const HAZARD: [f32; 3] = [0.95, 0.7, 0.08];

pub fn trash_depot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let _yard = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.15));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    // The transfer hall at the back, its open bays facing the yard.
    let share = [0.55, 0.7, 0.75][t];
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, hi.y - d * 0.22, w * share * 0.5, d * 0.2));
    let wall = mul(pick(rng, &pal.metal_paint), 0.75);
    let hh = [4.5, 7.0, 9.0][t];
    if hall.len() >= 3 && inradius(&hall) > 1.5 {
        extrude(g, f, WornMetal, &hall, 0.0, hh, wall);
        parapet(g, c, &hall, hh, 0.5, 0.2, Metal, mul(wall, 0.8));
        // Corrugation: ribs down every long face.
        for face in c.faces(&hall) {
            if face.len < 4.0 {
                continue;
            }
            let ribs = (face.len / 1.2) as i32;
            for k in 0..ribs {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / ribs as f32;
                face.bx(g, f, Metal, tt, hh * 0.5, 0.05, Vec3::new(0.08, hh * 0.98, 0.1), mul(wall, 0.85));
            }
            grime(g, c, rng, &face, 0.0, hh, 0.8);
        }
        // The yard side: bays, half shut, the tipping floor dark inside.
        let front = c.front_face(&hall);
        let bays = ([1, 2, 3][t] as i32).min(((front.len - 2.0) / 5.0).floor().max(1.0) as i32);
        let bw = ((front.len - 1.0) / bays as f32).min(5.2);
        for k in 0..bays {
            let tt = -front.len * 0.5 + 0.5 + (k as f32 + 0.5) * bw;
            if k % 2 == 0 {
                front.bx(g, f, Matte, tt, 2.3, 0.06, Vec3::new(bw - 0.8, 4.4, 0.08), DARK);
                front.bx(g, f, Metal, tt, 4.0, 0.12, Vec3::new(bw - 0.8, 1.2, 0.1), mul(pal.steel, 0.6));
            } else {
                roller_door(g, c, &front, tt, bw - 0.8, 4.4, mul(pal.steel, 0.55));
            }
            // Hazard stripes on the bay posts.
            front.bx(g, f, Matte, tt - bw * 0.5 + 0.25, 1.0, 0.15, Vec3::new(0.3, 2.0, 0.3), HAZARD);
        }
        // A green band and the sign.
        front.bx(g, f, Panel, 0.0, hh - 0.8, 0.12, Vec3::new(front.len * 0.9, 0.9, 0.1), SANITATION_GREEN);
        front.decal(g, f, Glow, 0.0, hh - 0.8, 0.2, (front.len * 0.4).min(8.0), 0.45, mul([0.4, 2.0, 0.6], 0.7));
        roof_gear(g, c, rng, &hall, hh, 0.15);
    }
    // Garbage trucks parked in a row facing the gate.
    let trucks = [1, 3, 5][t];
    let mut placed = 0;
    for k in 0..12 {
        if placed >= trucks {
            break;
        }
        let u = lo.x + 3.0 + k as f32 * 3.6;
        let p = Vec2::new(u, lo.y + d * 0.42);
        if vehicle_fits(c, p, -FRAC_PI_2, Vehicle::Truck) && hall.iter().all(|_| !nd_sim::zoning::point_in_polygon(p, &hall)) {
            vehicle(g, f, pal, rng, p, 0.0, -FRAC_PI_2, Vehicle::Truck, SANITATION_GREEN);
            placed += 1;
        }
    }
    // Skips and roll-off containers along one side, some heaped.
    let skips = [3, 4, 6][t];
    for k in 0..skips {
        let v = lo.y + 4.0 + k as f32 * 3.2;
        let p = Vec2::new(hi.x - 2.2, v);
        if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.6 || nd_sim::zoning::point_in_polygon(p, &hall) {
            continue;
        }
        let col = pick(rng, &[[0.55, 0.3, 0.1], [0.2, 0.32, 0.5], SANITATION_GREEN, [0.5, 0.45, 0.12]]);
        f.bx(g, Rust, p.x, 0.6, p.y, Vec3::new(2.6, 1.2, 1.6), mul(col, 0.85));
        for _ in 0..rng.random_range(1..4) {
            let q = p + Vec2::new(rng.random_range(-0.9..0.9), rng.random_range(-0.5..0.5));
            f.bx(g, Tarp, q.x, 1.35, q.y, Vec3::new(0.55, 0.45, 0.5), [0.06, 0.06, 0.07]);
        }
    }
    // A recycling tower with a conveyor running up to it.
    if t == 2 && hall.len() >= 3 {
        let (hl, hh2) = bounds(&hall);
        let tower = fit_inside(c, &rect(hl.x + 3.0, hh2.y - 3.0, 2.5, 2.5));
        if tower.len() >= 3 && inradius(&tower) > 1.2 {
            let th = hh + 8.0;
            extrude(g, f, Panel, &tower, 0.0, th, mul(pal.concrete(0), 0.9));
            parapet(g, c, &tower, th, 0.6, 0.2, Metal, mul(pal.steel, 0.7));
            let base = Vec2::new(hl.x + 9.0, lo.y + d * 0.35);
            if c.inside_local(base) {
                let top = interior_anchor(&tower);
                g.part(Metal).beam(f.at2(base, 0.6), f.at2(top, th - 1.0), 0.9, rgba(mul(pal.steel, 0.75)));
            }
            g.part(Glow).boxed(f.at2(interior_anchor(&tower), th + 0.5), Vec3::new(1.2, 0.3, 1.2), 0.0, rgba(mul([0.4, 2.0, 0.6], 0.8)));
        }
    }
    // Weighbridge hut and plate at the gate, a fence round the rest.
    let gate = Vec2::new(lo.x + w * 0.8, lo.y + 2.0);
    if c.inside_local(gate) && distance_to_boundary(gate, &c.lot_poly) > 1.2 {
        f.bx(g, Panel, gate.x, 1.3, gate.y, Vec3::new(2.4, 2.6, 2.0), mul(pal.panel(0), 0.9));
        f.bx(g, Glass, gate.x, 1.6, gate.y - 1.01, Vec3::new(1.6, 0.8, 0.04), pal.glass_dark);
        let plate = Vec2::new(gate.x - 4.5, gate.y + 2.0);
        if [Vec2::new(-1.7, -3.6), Vec2::new(1.7, -3.6), Vec2::new(-1.7, 3.6), Vec2::new(1.7, 3.6)].iter().all(|o| c.inside_local(plate + *o)) {
            f.bx(g, Metal, plate.x, 0.08, plate.y, Vec3::new(3.2, 0.16, 7.0), mul(pal.steel, 0.6));
        }
    }
    let edge = offset_poly(&c.lot_poly, 0.6);
    let n = edge.len();
    for i in 0..n {
        let (a, b) = (edge[i], edge[(i + 1) % n]);
        // Leave the front open for the trucks.
        if (a.y - lo.y).abs() < 1.5 && (b.y - lo.y).abs() < 1.5 {
            continue;
        }
        fence(g, c, a, b, 2.4, t == 0);
    }
    floodlight(g, c, Vec2::new(lo.x + 2.0, lo.y + d * 0.55), 8.0);
}
