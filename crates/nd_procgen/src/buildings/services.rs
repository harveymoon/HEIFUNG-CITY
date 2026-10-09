//! City services and attractions, each in three tiers: police posts to
//! headquarters with a helipad, fire stations with apparatus bays and a hose
//! tower, clinics to medical centres with an emergency entrance, schools
//! with a yard and a court, 911 dispatch centres under a radio mast,
//! memorials to landmark spires, and stadium bowls with floodlit pitches.
//! The tier comes from `Ctx::tier` (the one the player picked); floors are
//! the generator's own, so a service never grows into a tower by accident.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::Tier;
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

const POLICE_BLUE: [f32; 3] = [0.25, 0.55, 2.4];
const FIRE_RED: [f32; 3] = [0.62, 0.08, 0.06];
const MEDIC_GREEN: [f32; 3] = [0.3, 2.2, 0.8];
const EMERGENCY_RED: [f32; 3] = [2.4, 0.25, 0.2];
const SCHOOL_YELLOW: [f32; 3] = [0.85, 0.7, 0.25];

fn tier_pick<T: Copy>(c: &Ctx, v: [T; 3]) -> T {
    v[c.tier.index()]
}

/// The main block: a rectangle across the lot, `front` of the depth from the
/// road to its face and `share` of the width and depth, fitted onto the lot.
fn main_block(c: &Ctx, front: f32, share_w: f32, share_d: f32) -> Vec<Vec2> {
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let hd = d * share_d * 0.5;
    fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + d * front + hd, w * share_w * 0.5, hd))
}

/// Walls, punched windows, parapet and grime for a plain civic block.
#[allow(clippy::too_many_arguments)]
fn block(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y0: f32, floors: i32, s: Surface, wall: [f32; 3]) -> f32 {
    let fh = c.fh();
    let h = y0 + floors as f32 * fh;
    extrude(g, &c.frame, s, poly, y0, h, wall);
    parapet(g, c, poly, h, 0.9, 0.3, s, mul(wall, 0.85));
    let mut spec = WindowSpec::from_ctx(c);
    spec.boarded = 0.0;
    spec.ac = 0.05;
    for face in c.faces(poly) {
        if face.len > 4.0 {
            windows(g, c, rng, &face, y0, floors, fh, &spec);
            grime(g, c, rng, &face, y0, h, 0.6);
        }
    }
    h
}

/// A lit sign band across a face: a dark board with a glowing strip.
fn sign(g: &mut Geometry, c: &Ctx, face: &Face, y: f32, w: f32, color: [f32; 3]) {
    let f = &c.frame;
    let w = w.min(face.len * 0.8);
    face.bx(g, f, Matte, 0.0, y, 0.15, Vec3::new(w, 0.9, 0.2), DARK);
    face.decal(g, f, Glow, 0.0, y, 0.27, w * 0.9, 0.5, color);
}

/// A cross on a face: two glowing bars.
fn cross(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, size: f32, color: [f32; 3]) {
    let f = &c.frame;
    face.decal(g, f, Glow, t, y, 0.3, size, size * 0.33, color);
    face.decal(g, f, Glow, t, y, 0.31, size * 0.33, size, color);
}

/// Light bar on a parked emergency vehicle (police and ambulances).
fn light_bar(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, a: [f32; 3], b: [f32; 3]) {
    let f = &c.frame;
    f.bx(g, Glow, p.x - 0.35, y, p.y, Vec3::new(0.5, 0.18, 0.3), a);
    f.bx(g, Glow, p.x + 0.35, y, p.y, Vec3::new(0.5, 0.18, 0.3), b);
}

pub fn police_station(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.2));
    let floors = tier_pick(c, [1, 2, 4]);
    let main = main_block(c, 0.05, 0.62, 0.5);
    let wall = tier_pick(c, [mul(pick(rng, &pal.concrete), 0.85), mul(pick(rng, &pal.panel), 0.8), [0.2, 0.24, 0.32]]);
    let s = if c.tier == Tier::High { Panel } else { Concrete };
    let h = block(g, c, rng, &main, 0.0, floors, s, wall);
    let front = c.front_face(&main);
    // Blue band, the sign, lamps either side of the door.
    extrude(g, f, Glow, &offset_poly(&main, -0.05), c.fh() - 0.35, c.fh() - 0.15, mul(POLICE_BLUE, 0.45));
    sign(g, c, &front, h + 0.6, 9.0, mul(POLICE_BLUE, 0.8));
    entrance(g, c, rng, &front, 0.0, 2.2, 2.8);
    for t in [-1.7f32, 1.7] {
        front.bx(g, f, Glow, t, 3.2, 0.35, Vec3::new(0.35, 0.35, 0.35), POLICE_BLUE);
    }
    barrier(g, c, &front, front.len * 0.35, 2.5, 3.0);
    // Garage: a side wing with roller doors facing the yard behind.
    let (lo, hi) = bounds(&main);
    let bays = tier_pick(c, [1, 2, 3]);
    let garage = fit_inside(c, &rect(hi.x - (hi.x - lo.x) * 0.25, hi.y + 4.0, (hi.x - lo.x) * 0.25, 3.5));
    if polygon_ok(&garage, &main) {
        extrude(g, f, Concrete, &garage, 0.0, 4.2, mul(wall, 0.9));
        let gf = c.faces(&garage).into_iter().max_by(|a, b| a.out.y.total_cmp(&b.out.y)).unwrap();
        for k in 0..bays {
            let t = -gf.len * 0.5 + (k as f32 + 0.5) * gf.len / bays as f32;
            roller_door(g, c, &gf, t, (gf.len / bays as f32 - 0.6).min(3.4), 3.2, mul(pal.steel, 0.6));
        }
    }
    // Patrol cars in the yard, light bars on top.
    let (llo, lhi) = bounds(&lot);
    let v = (hi.y + 10.0).min(lhi.y - 3.0);
    let cars = tier_pick(c, [2, 3, 5]);
    let mut u = llo.x + 2.5;
    let mut n = 0;
    while u < lhi.x - 2.0 && n < cars {
        let p = Vec2::new(u, v);
        if vehicle_fits(c, p, FRAC_PI_2, Vehicle::Car) && !nd_sim::zoning::point_in_polygon(p, &main) && !nd_sim::zoning::point_in_polygon(p, &garage) {
            vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, Vehicle::Car, [0.85, 0.87, 0.9]);
            light_bar(g, c, p, 1.85, EMERGENCY_RED, POLICE_BLUE);
            n += 1;
        }
        u += 3.2;
    }
    fence_lot(g, c, &lot, 2.4, c.tier != Tier::High);
    floodlight(g, c, front.at(front.len * 0.45, -1.2), 7.0);
    if c.tier == Tier::High {
        let r = (inradius(&main) * 0.8).min(7.0);
        if r > 3.0 {
            helipad(g, c, interior_anchor(&main), h + 0.2, r);
        }
    } else {
        antenna_mast(g, c, interior_anchor(&main) + Vec2::new(1.5, 0.0), h, 6.0 + 4.0 * floors as f32);
    }
}

/// True when a fitted annex did not collapse into the main block.
fn polygon_ok(annex: &[Vec2], main: &[Vec2]) -> bool {
    annex.len() >= 3 && inradius(annex) > 1.2 && !nd_sim::zoning::point_in_polygon(poly_center(annex), main)
}

pub fn fire_station(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _lot = yard(g, c, 0.4, Paving, mul(pal.paving, 1.05));
    let floors = tier_pick(c, [1, 2, 3]);
    // The station stands back from the road behind an apron for the engines.
    let main = main_block(c, 0.3, 0.8, 0.55);
    let brick = tier_pick(c, [mul(pick(rng, &pal.brick), 0.9), mul(pick(rng, &pal.brick), 1.0), mul(pick(rng, &pal.panel), 0.85)]);
    let s = if c.tier == Tier::High { Panel } else { Brick };
    let fh = c.fh().max(4.5);
    let h = floors as f32 * fh;
    extrude(g, f, s, &main, 0.0, h, brick);
    parapet(g, c, &main, h, 1.0, 0.35, s, mul(brick, 0.8));
    let front = c.front_face(&main);
    // Apparatus bays: tall red doors across the front, lit when open.
    let bays = tier_pick(c, [2, 3, 4]).min(((front.len - 2.0) / 4.4).floor().max(1.0) as i32);
    let bay_w = ((front.len - 1.0) / bays as f32).min(4.6);
    for k in 0..bays {
        let t = -front.len * 0.5 + 0.5 + (k as f32 + 0.5) * bay_w;
        if k % 2 == 0 {
            roller_door(g, c, &front, t, bay_w - 0.7, 4.0, FIRE_RED);
        } else {
            front.decal(g, f, Glow, t, 2.0, 0.08, bay_w - 0.7, 4.0, mul(pal.window_warm, 0.5));
        }
    }
    sign(g, c, &front, (h - 0.6).max(4.8), 10.0, mul(EMERGENCY_RED, 0.8));
    let mut spec = WindowSpec::from_ctx(c);
    spec.boarded = 0.0;
    for face in c.faces(&main) {
        if !face.is_front && face.len > 4.0 {
            windows(g, c, rng, &face, 0.0, floors, fh, &spec);
        }
        if face.is_front && floors > 1 {
            windows(g, c, rng, &face, fh, floors - 1, fh, &spec);
        }
    }
    // Hose tower at a back corner.
    let (lo, hi) = bounds(&main);
    let tw = fit_inside(c, &rect(lo.x + 2.0, hi.y - 2.0, 1.8, 1.8));
    if tw.len() >= 3 && inradius(&tw) > 0.8 {
        let th = h + tier_pick(c, [6.0, 8.0, 10.0]);
        extrude(g, f, s, &tw, h, th, mul(brick, 0.95));
        extrude(g, f, Metal, &offset_poly(&tw, -0.3), th, th + 0.4, mul(pal.steel, 0.7));
        g.part(Glow).boxed(f.at2(interior_anchor(&tw), th + 0.7), Vec3::splat(0.4), 0.0, rgba(EMERGENCY_RED));
    }
    // An engine out on the apron, nose to the road.
    let ap = front.at(-front.len * 0.25, -4.5);
    if vehicle_fits(c, ap, FRAC_PI_2, Vehicle::Truck) && !nd_sim::zoning::point_in_polygon(ap, &main) {
        vehicle(g, f, pal, rng, ap, 0.0, -FRAC_PI_2, Vehicle::Truck, FIRE_RED);
        f.bx(g, Glow, ap.x, 3.05, ap.y + 2.5, Vec3::new(1.6, 0.2, 0.3), EMERGENCY_RED);
    }
    if c.tier == Tier::High {
        let r = (inradius(&main) * 0.7).min(7.0);
        if r > 3.0 {
            helipad(g, c, interior_anchor(&main), h + 0.2, r);
        }
    } else {
        roof_gear(g, c, rng, &main, h, 0.25);
    }
    floodlight(g, c, front.at(front.len * 0.5 + 0.8, -1.0), 7.0);
}

pub fn hospital(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _lot = yard(g, c, 0.4, Paving, mul(pal.paving, 1.15));
    let white = [0.82, 0.84, 0.86];
    // Podium for every tier; the medical centre adds a ward tower.
    let podium = main_block(c, 0.18, 0.78, 0.55);
    let pf = tier_pick(c, [1, 2, 3]);
    let ph = block(g, c, rng, &podium, 0.0, pf, Plaster, white);
    let front = c.front_face(&podium);
    // Coloured band and the green cross.
    extrude(g, f, Panel, &offset_poly(&podium, -0.06), ph - 1.0, ph - 0.6, mul(MEDIC_GREEN, 0.25));
    cross(g, c, &front, front.len * 0.32, ph - 1.6, 1.8, MEDIC_GREEN);
    entrance(g, c, rng, &front, -front.len * 0.15, 3.0, 3.0);
    // Emergency entrance: a deep canopy over the ambulance bay with a red sign.
    let t = front.len * 0.3;
    let mut depth = 6.0f32;
    while depth > 1.0 && !(c.inside_local(front.at(t - 3.8, depth + 0.3)) && c.inside_local(front.at(t + 3.8, depth + 0.3))) {
        depth -= 0.5;
    }
    if depth > 1.0 {
        canopy(g, c, &front, t, 3.6, 7.0, depth, mul(c.style.trim_color, 0.9));
    }
    front.decal(g, f, Glow, t, 4.4, 0.25, 5.0, 0.6, EMERGENCY_RED);
    let bay = front.at(t, -3.5);
    if vehicle_fits(c, bay, 0.0, Vehicle::Van) && !nd_sim::zoning::point_in_polygon(bay, &podium) {
        vehicle(g, f, pal, rng, bay, 0.0, front.yaw(), Vehicle::Van, [0.9, 0.9, 0.88]);
        light_bar(g, c, bay, 2.3, EMERGENCY_RED, mul(pal.lamp, 1.2));
    }
    let top = if c.tier == Tier::Low {
        roof_gear(g, c, rng, &podium, ph, 0.3);
        ph
    } else {
        // Ward block on the podium, slimmer, with a lit cross high up.
        let wards = fit_inside(c, &scale_poly(&podium, 0.62));
        let wf = tier_pick(c, [0, 2, 5]);
        let wh = block(g, c, rng, &wards, ph, wf, Panel, mul(white, 0.95));
        let wfront = c.front_face(&wards);
        cross(g, c, &wfront, 0.0, wh - 2.0, 2.6, MEDIC_GREEN);
        wh
    };
    if c.tier == Tier::High {
        let base = fit_inside(c, &scale_poly(&podium, 0.62));
        let r = (inradius(&base) * 0.85).min(8.0);
        if r > 3.0 {
            helipad(g, c, interior_anchor(&base), top + 0.2, r);
            g.part(Glow).boxed(f.at2(interior_anchor(&base), top + 0.6), Vec3::new(r * 0.3, 0.06, r * 0.9), 0.0, rgba(EMERGENCY_RED));
        }
    }
    // A few trees and benches on the forecourt.
    for k in 0..3 {
        let p = front.at(-front.len * 0.4 + k as f32 * 3.0, -3.0);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.5 && !nd_sim::zoning::point_in_polygon(p, &podium) {
            tree_in(g, f, pal, p, 3.5, 2.4, pal.foliage);
        }
    }
}

pub fn school(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _lot = yard(g, c, 0.4, Paving, mul(pal.paving, 1.0));
    let floors = tier_pick(c, [1, 2, 3]);
    // Classroom wing along the road; the yard behind it.
    let wing = main_block(c, 0.04, 0.85, 0.38);
    let wall = tier_pick(c, [mul(pick(rng, &pal.plaster), 1.0), mul(pick(rng, &pal.brick), 1.0), mul(pick(rng, &pal.glass), 0.9)]);
    let s = if c.tier == Tier::High { Glass } else { Plaster };
    let h = block(g, c, rng, &wing, 0.0, floors, s, wall);
    let front = c.front_face(&wing);
    entrance(g, c, rng, &front, 0.0, 3.0, 3.0);
    front.bx(g, f, Panel, 0.0, h + 0.5, 0.2, Vec3::new((front.len * 0.5).min(10.0), 1.0, 0.2), SCHOOL_YELLOW);
    front.decal(g, f, Glow, 0.0, h + 0.5, 0.32, (front.len * 0.4).min(8.0), 0.4, mul(pal.lamp, 0.7));
    if c.tier == Tier::High {
        // College: a glass atrium and a taller lecture block.
        let atrium = fit_inside(c, &scale_poly(&wing, 0.4));
        extrude(g, f, Glass, &atrium, h, h + 6.0, mul(pal.glass_dark, 1.2));
        curtain_lights(g, c, rng, &atrium, h, 1, 6.0);
    } else {
        roof_gear(g, c, rng, &wing, h, 0.2);
    }
    // The yard: a court with lines and hoops, a fence, trees on the edge.
    let (lo, hi) = bounds(&wing);
    let (llo, lhi) = bounds(&c.lot_poly);
    let yv = (hi.y + lhi.y) * 0.5;
    let court = fit_inside(c, &rect((llo.x + lhi.x) * 0.5, yv, ((lhi.x - llo.x) * 0.3).min(14.0), ((lhi.y - hi.y) * 0.3).clamp(1.0, 8.0)));
    if court.len() >= 3 && inradius(&court) > 2.0 && !nd_sim::zoning::point_in_polygon(poly_center(&court), &wing) {
        ground(g, f, Matte, &court, 0.06, [0.2, 0.35, 0.55]);
        let (clo, chi) = bounds(&court);
        let cm = poly_center(&court);
        f.bx(g, Matte, cm.x, 0.08, cm.y, Vec3::new(0.12, 0.02, chi.y - clo.y), [0.9, 0.9, 0.9]);
        for x in [clo.x + 0.6, chi.x - 0.6] {
            if c.inside_local(Vec2::new(x, cm.y)) {
                f.bx(g, Matte, x, 1.5, cm.y, Vec3::new(0.12, 3.0, 0.12), mul(pal.steel, 0.7));
                f.bx(g, Panel, x, 3.1, cm.y, Vec3::new(0.1, 0.9, 1.4), [0.9, 0.9, 0.9]);
            }
        }
    }
    for k in 0..6 {
        let p = Vec2::new(llo.x + 2.5 + k as f32 * (lhi.x - llo.x - 5.0) / 5.0, lhi.y - 2.5);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 && !nd_sim::zoning::point_in_polygon(p, &wing) && !nd_sim::zoning::point_in_polygon(p, &court) {
            tree_in(g, f, pal, p, 4.0, 2.6, pal.foliage);
        }
    }
    let _ = lo;
    fence_lot(g, c, &offset_poly(&c.lot_poly, 0.4), 1.8, false);
}

pub fn dispatch_center(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.25));
    let floors = tier_pick(c, [1, 2, 5]);
    let main = main_block(c, 0.15, 0.6, 0.6);
    let wall = tier_pick(c, [mul(pick(rng, &pal.concrete), 0.7), mul(pick(rng, &pal.panel), 0.7), [0.12, 0.13, 0.16]]);
    let fh = c.fh();
    let h = floors as f32 * fh;
    extrude(g, f, Panel, &main, 0.0, h, wall);
    parapet(g, c, &main, h, 0.8, 0.3, Panel, mul(wall, 0.8));
    // The ops room: a continuous band of screen-lit glass on every floor.
    for face in c.faces(&main) {
        if face.len > 3.0 {
            for k in 0..floors {
                face.decal(g, f, Glow, 0.0, k as f32 * fh + fh * 0.55, 0.06, face.len - 1.2, fh * 0.35, mul([0.3, 0.9, 1.6], 0.35 + 0.3 * c.lit_chance()));
            }
        }
    }
    let front = c.front_face(&main);
    sign(g, c, &front, h + 0.6, 8.0, mul(EMERGENCY_RED, 0.7));
    entrance(g, c, rng, &front, 0.0, 2.0, 2.8);
    // Radio mast and dishes; a taller mast with aviation lights at the top tier.
    let a = interior_anchor(&main);
    let mast = tier_pick(c, [10.0, 16.0, 26.0]);
    antenna_mast(g, c, a, h, mast);
    for (k, d) in [Vec2::new(2.5, 1.5), Vec2::new(-2.5, -1.0)].iter().enumerate() {
        let p = a + *d;
        if nd_sim::zoning::point_in_polygon(p, &main) && k < tier_pick(c, [1, 2, 2]) {
            dish(g, f, p, h, 1.0 + k as f32 * 0.4, [0.85, 0.85, 0.85]);
        }
    }
    aviation_light(g, pal, f.at2(a, h + mast + 0.3), 0.5);
    fence_lot(g, c, &offset_poly(&c.lot_poly, 0.4), 2.4, true);
    floodlight(g, c, front.at(front.len * 0.4, -1.2), 6.0);
}

/// Attractions: a memorial with an eternal flame, an obelisk on a plaza,
/// or a landmark spire that lights up the skyline.
pub fn landmark(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _plaza = yard(g, c, 0.4, Stone, mul(pal.paving, 1.25));
    let center = interior_anchor(&c.poly);
    let room = inradius(&c.poly).max(2.0);
    let stone = mul(pal.stone, 1.5);
    match c.tier {
        Tier::Low => {
            // Plinth, a bronze figure, a low wall of names and a flame.
            let r = (room * 0.25).clamp(1.2, 3.0);
            extrude(g, f, Stone, &ngon(center.x, center.y, r * 1.6, 8, 0.2), 0.0, 0.6, stone);
            extrude(g, f, Stone, &ngon(center.x, center.y, r, 4, 0.0), 0.6, 2.4, stone);
            f.bx(g, Metal, center.x, 3.6, center.y, Vec3::new(0.8, 2.4, 0.6), pal.bronze);
            f.bx(g, Metal, center.x, 5.0, center.y, Vec3::new(0.5, 0.5, 0.5), pal.bronze);
            let flame = center + Vec2::new(0.0, -r * 2.2);
            if c.inside_local(flame) {
                f.bx(g, Stone, flame.x, 0.4, flame.y, Vec3::new(1.2, 0.8, 1.2), stone);
                f.bx(g, Glow, flame.x, 1.1, flame.y, Vec3::new(0.5, 0.7, 0.5), [2.6, 1.2, 0.3]);
            }
        }
        Tier::Mid => {
            // A triumphal arch across the plaza, facing the road, with a
            // statue group on a plinth under it.
            let w = (room * 1.2).clamp(4.0, 22.0);
            let pier = (w * 0.18).max(1.4);
            let h = (w * 0.75).clamp(7.0, 16.0);
            let dpt = (w * 0.22).max(2.0);
            for sgn in [-1.0f32, 1.0] {
                let px = center.x + sgn * (w * 0.5 - pier * 0.5);
                f.bx(g, Stone, px, h * 0.5, center.y, Vec3::new(pier, h, dpt), stone);
                f.bx(g, Glow, px, h * 0.35, center.y - dpt * 0.5 - 0.05, Vec3::new(pier * 0.4, h * 0.4, 0.06), mul(pal.lamp, 0.5));
            }
            f.bx(g, Stone, center.x, h + 1.2, center.y, Vec3::new(w, 2.4, dpt + 0.4), mul(stone, 1.05));
            f.bx(g, Stone, center.x, h + 2.7, center.y, Vec3::new(w * 0.7, 0.6, dpt * 0.8), stone);
            f.bx(g, Glow, center.x, h + 1.2, center.y - dpt * 0.5 - 0.25, Vec3::new(w * 0.6, 0.4, 0.06), mul(pal.window_cold, 0.9));
            // Statue group: three bronze figures on a low plinth.
            f.bx(g, Stone, center.x, 0.4, center.y, Vec3::new(w * 0.35, 0.8, dpt * 0.8), stone);
            for k in 0..3 {
                let x = center.x + (k as f32 - 1.0) * w * 0.09;
                let tall = if k == 1 { 2.2 } else { 1.8 };
                f.bx(g, Metal, x, 0.8 + tall * 0.5, center.y, Vec3::new(0.5, tall, 0.4), pal.bronze);
                f.bx(g, Metal, x, 0.8 + tall + 0.2, center.y, Vec3::new(0.35, 0.35, 0.35), pal.bronze);
            }
        }
        Tier::High => {
            // A faceted glass sculpture: a broad crystal on a stepped base,
            // wider than it is tall, under a lit crown ring.
            let r = (room * 0.45).clamp(1.5, 14.0);
            extrude(g, f, Stone, &ngon(center.x, center.y, r * 1.25, 8, 0.2), 0.0, 1.0, stone);
            extrude(g, f, Stone, &ngon(center.x, center.y, r * 1.1, 8, 0.2), 1.0, 2.0, mul(stone, 1.05));
            let girdle = ngon(center.x, center.y, r, 8, 0.0);
            let waist = r * 0.9;
            let glass = mul(pick(rng, &pal.glass), 1.2);
            loft(g, f, Glass, &ngon(center.x, center.y, r * 0.55, 8, 0.39), &girdle, 2.0, 2.0 + waist * 0.6, glass);
            loft(g, f, Glass, &girdle, &ngon(center.x, center.y, r * 0.45, 8, 0.39), 2.0 + waist * 0.6, 2.0 + waist * 1.1, mul(glass, 1.1));
            let top = 2.0 + waist * 1.1;
            // Crown: a ring of glowing facets and a halo.
            for k in 0..8 {
                let a = k as f32 / 8.0 * TAU + 0.39;
                let p = center + Vec2::new(a.cos(), a.sin()) * r * 0.45;
                f.bx(g, Glow, p.x, top + 0.6, p.y, Vec3::new(0.5, 1.2, 0.5), mul(pick(rng, &pal.led_colors), 1.0));
            }
            extrude(g, f, Glow, &ngon(center.x, center.y, r * 1.02, 8, 0.0), 2.0 + waist * 0.6 - 0.15, 2.0 + waist * 0.6 + 0.15, mul(pal.window_cold, 1.1));
        }
    }
    // Benches and lamps round the plaza edge.
    let front = c.front_face(&c.poly);
    for k in 0..3 {
        let t = -front.len * 0.35 + k as f32 * front.len * 0.35;
        lamp_post_in(g, c, front.at(t, -1.5), 4.5, pal.lamp);
    }
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU;
        let p = center + Vec2::new(a.cos(), a.sin()) * (room * 0.7);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 {
            if k % 2 == 0 {
                tree_in(g, f, pal, p, 4.0, 2.8, pal.foliage);
            } else {
                bench_world(g, f.to_world(p.x, p.y), f.yaw() + a, pal.wood, CHARCOAL);
            }
        }
    }
}

/// An oval bowl of terraces round a floodlit pitch; the top tier gets a
/// roof ring and a band of LED screens.
pub fn stadium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _plaza = yard(g, c, 0.4, Paving, pal.paving);
    let center = interior_anchor(&c.poly);
    let (lo, hi) = bounds(&c.poly);
    let room = inradius(&c.poly).max(3.0);
    // Ellipse along the longer axis, kept inside the footprint.
    let (ax, ay) = if hi.x - lo.x > hi.y - lo.y { (1.35f32, 1.0f32) } else { (1.0, 1.35) };
    let n = 24;
    let oval = |r: f32| -> Vec<Vec2> { (0..n).map(|i| { let a = i as f32 / n as f32 * TAU; center + Vec2::new(a.cos() * r * ax, a.sin() * r * ay) }).collect() };
    let mut r = room * 0.95;
    let mut outer = oval(r);
    for _ in 0..12 {
        if poly_inside(c, &outer) {
            break;
        }
        r *= 0.9;
        outer = oval(r);
    }
    let h = tier_pick(c, [6.0, 12.0, 20.0]);
    let inner = oval(r * 0.62);
    let panel = pick(rng, &pal.panel);
    // Pitch, raking stands from the touchline up to the rim, and the open
    // outer wall: quads, not solids, so nothing caps the bowl.
    ground(g, f, Foliage, &inner, 0.06, [0.15, 0.42, 0.18]);
    let rim = oval(r * 0.96);
    let centre3 = f.at2(center, 0.0);
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = (f.at2(inner[i], 0.6), f.at2(inner[j], 0.6));
        let (cc, d) = (f.at2(rim[j], h), f.at2(rim[i], h));
        g.part(Concrete).quad_facing([a, b, cc, d], (centre3 - (a + b) * 0.5).normalize_or_zero() + Vec3::Y, rgba(mul(pal.concrete(0), 0.9)));
        // Seats: a coloured band halfway up.
        let (sa, sb) = (a.lerp(d, 0.45), b.lerp(cc, 0.45));
        let (sc, sd) = (b.lerp(cc, 0.55), a.lerp(d, 0.55));
        g.part(Matte).quad_facing([sa + Vec3::Y * 0.05, sb + Vec3::Y * 0.05, sc + Vec3::Y * 0.05, sd + Vec3::Y * 0.05], Vec3::Y, rgba(if i % 2 == 0 { [0.55, 0.1, 0.12] } else { [0.12, 0.2, 0.5] }));
        let (oa, ob) = (f.at2(outer[i], 0.0), f.at2(outer[j], 0.0));
        let (oc, od) = (f.at2(outer[j], h + 0.8), f.at2(outer[i], h + 0.8));
        let out = ((oa + ob) * 0.5 - centre3).normalize_or_zero();
        g.part(Panel).quad_facing([oa, ob, oc, od], out, rgba(mul(panel, 0.9)));
        g.part(Panel).quad_facing([oa, ob, oc, od], -out, rgba(mul(panel, 0.6)));
        // Lip joining wall and stands.
        g.part(Panel).quad_facing([od, oc, cc + Vec3::Y * 0.8, d + Vec3::Y * 0.8], Vec3::Y, rgba(mul(panel, 0.8)));
    }
    // Centre circle and halfway line.
    let ring = |k: f32| -> Vec<Vec2> { (0..n).map(|i| { let a = i as f32 / n as f32 * TAU; center + Vec2::new(a.cos(), a.sin()) * r * 0.12 * k }).collect() };
    let (c0, c1) = (ring(1.0), ring(0.85));
    for i in 0..n {
        let j = (i + 1) % n;
        g.part(Matte).quad_facing([f.at2(c1[i], 0.09), f.at2(c1[j], 0.09), f.at2(c0[j], 0.09), f.at2(c0[i], 0.09)], Vec3::Y, rgba([0.9, 0.9, 0.9]));
    }
    let across = if ax > ay { Vec2::Y } else { Vec2::X };
    let (la, lb) = (center - across * r * 0.6, center + across * r * 0.6);
    let side = Vec2::new(-across.y, across.x) * 0.08;
    g.part(Matte).quad_facing([f.at2(la - side, 0.09), f.at2(lb - side, 0.09), f.at2(lb + side, 0.09), f.at2(la + side, 0.09)], Vec3::Y, rgba([0.9, 0.9, 0.9]));
    // LED band round the outside.
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = (outer[i], outer[j]);
        let mid = (a + b) * 0.5;
        let out = (mid - center).normalize_or_zero() * 0.08;
        let led = mul(crate::palette::at(&pal.led_colors, i), 0.8);
        g.part(Glow).quad_facing([f.at2(a + out, h * 0.55), f.at2(b + out, h * 0.55), f.at2(b + out, h * 0.62), f.at2(a + out, h * 0.62)], f.at2(mid + out * 10.0, 0.0) - f.at2(mid, 0.0), rgba(led));
    }
    // Gates at the road side.
    let front = c.front_face(&c.poly);
    for k in 0..3 {
        let p = center + (front.mid() - center).normalize_or_zero() * r * if ax > ay { 1.0 } else { 1.35 } * 0.98 + front.tangent * ((k as f32 - 1.0) * r * 0.3);
        f.bx(g, Glow, p.x, 3.4, p.y, Vec3::new(2.2, 0.3, 0.3), mul(pal.lamp, 0.9));
    }
    // Floodlight masts on four sides of the bowl.
    let mast_h = h + tier_pick(c, [8.0, 14.0, 18.0]);
    for k in 0..4 {
        let a = (k as f32 + 0.5) / 4.0 * TAU;
        let p = center + Vec2::new(a.cos() * r * ax * 0.86, a.sin() * r * ay * 0.86);
        if c.inside_local(p) {
            f.bx(g, Matte, p.x, mast_h * 0.5, p.y, Vec3::new(0.6, mast_h, 0.6), mul(pal.steel, 0.7));
            f.bx(g, Matte, p.x, mast_h + 0.6, p.y, Vec3::new(3.0, 1.6, 0.6), CHARCOAL);
            f.bx(g, Glow, p.x, mast_h + 0.6, p.y, Vec3::new(2.8, 1.4, 0.7), mul(pal.street_lamp, 1.6));
        }
    }
    if c.tier == Tier::High {
        // Roof ring on posts over the stands: an open annulus.
        let ring_o = oval(r * 1.0);
        let ring_i = oval(r * 0.78);
        for i in 0..n {
            let j = (i + 1) % n;
            let q = [f.at2(ring_i[i], h + 4.5), f.at2(ring_i[j], h + 4.5), f.at2(ring_o[j], h + 4.5), f.at2(ring_o[i], h + 4.5)];
            g.part(Metal).quad_facing(q, Vec3::Y, rgba(mul(pal.steel, 0.85)));
            g.part(Metal).quad_facing(q.map(|p| p - Vec3::Y * 0.3), -Vec3::Y, rgba(mul(pal.steel, 0.5)));
        }
        for (k, p) in ring_i.iter().enumerate() {
            if k % 3 == 0 {
                f.bx(g, Matte, p.x, h + 2.0, p.y, Vec3::new(0.4, 4.0, 0.4), mul(pal.steel, 0.6));
            }
        }
    }
}

/// A jail: cell blocks inside a high wall with watchtowers at the corners
/// and floodlights over an exercise yard; the penitentiary gets more blocks.
pub fn prison(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let _yard = yard(g, c, 0.4, Concrete, mul(pal.concrete(0), 0.8));
    let wall = offset_poly(&c.lot_poly, 0.8);
    let grey = mul(pick(rng, &pal.concrete), 0.7);
    // The wall: thin slabs along every edge, a lit strip on top.
    let wh = tier_pick(c, [3.5, 5.0, 6.5]);
    for face in c.faces(&wall) {
        if face.len < 1.0 {
            continue;
        }
        face.bx(g, f, Concrete, 0.0, wh * 0.5, -0.3, Vec3::new(face.len, wh, 0.5), grey);
        face.bx(g, f, Glow, 0.0, wh + 0.05, -0.3, Vec3::new(face.len * 0.98, 0.08, 0.1), mul(pal.street_lamp, 0.6));
    }
    // Cell blocks: one, two or three slabs side by side.
    let blocks = tier_pick(c, [1, 2, 3]);
    let floors = tier_pick(c, [1, 2, 3]);
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    for k in 0..blocks {
        let cu = lo.x + w * (k as f32 + 0.5) / blocks as f32;
        let b = fit_inside(c, &rect(cu, lo.y + d * 0.42, w * 0.4 / blocks as f32, d * 0.24));
        if b.len() < 3 || inradius(&b) < 1.0 {
            continue;
        }
        let h = floors as f32 * c.fh();
        extrude(g, f, Concrete, &b, 0.0, h, mul(grey, 1.15));
        parapet(g, c, &b, h, 0.8, 0.3, Concrete, grey);
        for face in c.faces(&b) {
            if face.len > 3.0 {
                // Slit windows, barred.
                let n = (face.len / 2.2) as i32;
                for i in 0..n.max(1) {
                    let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / n.max(1) as f32;
                    for fl in 0..floors {
                        face.decal(g, f, Glow, t, fl as f32 * c.fh() + c.fh() * 0.6, 0.05, 0.25, 0.9, mul(pal.window_cold, 0.25 + 0.3 * c.lit_chance()));
                    }
                }
            }
        }
    }
    // Watchtowers at the corners of the wall and floodlights on the yard.
    for p in wall.iter() {
        let inset = toward(*p, poly_center(&wall), 1.6);
        if !c.inside_local(inset) || distance_to_boundary(inset, &c.lot_poly) < 1.0 {
            continue;
        }
        let th = wh + 4.0;
        f.bx(g, Concrete, inset.x, th * 0.5, inset.y, Vec3::new(1.6, th, 1.6), grey);
        f.bx(g, Glass, inset.x, th + 0.8, inset.y, Vec3::new(2.0, 1.6, 2.0), pal.glass_dark);
        f.bx(g, Metal, inset.x, th + 1.7, inset.y, Vec3::new(2.4, 0.2, 2.4), mul(pal.steel, 0.7));
        f.bx(g, Glow, inset.x, th + 0.9, inset.y, Vec3::new(2.05, 0.15, 2.05), mul(pal.street_lamp, 1.2));
    }
    let front = c.front_face(&wall);
    f.bx(g, Metal, front.at(0.0, -0.3).x, 2.0, front.at(0.0, -0.3).y, Vec3::new(4.0, 4.0, 0.6), mul(pal.steel, 0.5));
    sign(g, c, &front, wh + 1.0, 6.0, mul(pal.window_cold, 0.6));
    floodlight(g, c, poly_center(&c.poly) + Vec2::new(0.0, d * 0.3), wh + 3.0);
}
