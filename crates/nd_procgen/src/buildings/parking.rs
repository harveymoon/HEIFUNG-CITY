//! Parking: surface lots where land is cheap, multi-storey car parks in the
//! middle. The share of bays that hold a car follows the sim's occupancy, so
//! a full lot looks full.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::seq::SliceRandom;
use rand::Rng;

const BAY_W: f32 = 2.5;
const BAY_D: f32 = 5.0;
const AISLE: f32 = 6.0;

/// Bay centres and their yaw (bays face the aisle), laid out in double rows
/// across the footprint's bounding rectangle, kept inside the polygon.
fn bays(poly: &[Vec2], margin: f32) -> Vec<(Vec2, f32)> {
    let inner = offset_poly(poly, margin);
    if inner.len() < 3 {
        return Vec::new();
    }
    let (lo, hi) = bounds(&inner);
    let w = hi.x - lo.x;
    let d = hi.y - lo.y;
    let mut out = Vec::new();
    // Rows run along u; a module is two bay depths plus an aisle.
    let module = BAY_D * 2.0 + AISLE;
    let modules = ((d + AISLE) / module).floor().max(1.0) as i32;
    let cols = (w / BAY_W).floor().max(1.0) as i32;
    for m in 0..modules {
        let v0 = lo.y + m as f32 * module;
        for side in 0..2 {
            let v = v0 + if side == 0 { BAY_D * 0.5 } else { BAY_D * 1.5 + AISLE };
            if v + BAY_D * 0.5 > hi.y + 0.1 {
                continue;
            }
            for k in 0..cols {
                let u = lo.x + (k as f32 + 0.5) * BAY_W;
                let p = Vec2::new(u, v);
                let corners = [p + Vec2::new(-BAY_W * 0.5, -BAY_D * 0.5), p + Vec2::new(BAY_W * 0.5, BAY_D * 0.5), p + Vec2::new(-BAY_W * 0.5, BAY_D * 0.5), p + Vec2::new(BAY_W * 0.5, -BAY_D * 0.5)];
                if corners.iter().all(|q| nd_sim::zoning::point_in_polygon(*q, &inner)) {
                    out.push((p, if side == 0 { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 }));
                }
            }
        }
    }
    out
}

/// [taxis] The bays a surface lot on `lot` paints, in world coordinates
/// with each bay's heading (the way a car parked in it faces), in the
/// order the lot draws them: what `parking_lot` lays out, so the renderer
/// can stand vehicles of its own (a taxi yard's cabs) in the painted bays.
pub fn surface_lot_bays_world(lot: &nd_sim::Lot) -> Vec<(Vec2, f32)> {
    let (frame, lot_poly) = crate::buildings::Frame::lot_local(lot);
    let yard = offset_poly(&lot_poly, 0.4);
    bays(&yard, 3.0).into_iter().map(|(p, yaw)| (frame.to_world(p.x, p.y), frame.yaw() + yaw)).collect()
}

/// Paint the bay lines and put cars in a share of the bays.
fn fill_bays(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, bays: &[(Vec2, f32)], y: f32, occupancy: f32, line: [f32; 3]) {
    let f = &c.frame;
    for (p, yaw) in bays {
        // Side lines of the bay, along its depth: painted, so one quad each.
        for s in [-1.0f32, 1.0] {
            let (sn, cs) = yaw.sin_cos();
            let off = Vec2::new(-sn, cs) * (BAY_W * 0.5 * s);
            let q = *p + off;
            f.flat(g, Glow, q.x, y + 0.012, q.y, BAY_D * 0.9, 0.1, *yaw, line);
        }
    }
    let n = ((bays.len() as f32) * occupancy.clamp(0.0, 1.0)).round() as usize;
    let mut order: Vec<usize> = (0..bays.len()).collect();
    let mut pick_rng = rng_for(c.seed, 77);
    order.shuffle(&mut pick_rng);
    for &i in order.iter().take(n) {
        let (p, yaw) = bays[i];
        let col = pick(rng, &c.palette.car_colors);
        vehicle(g, f, c.palette, rng, p, y, yaw, Vehicle::Car, col);
    }
}

/// Entrance booth, barrier arm and a lit P sign on the front face.
fn gate(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, front: &Face, y: f32) {
    let f = &c.frame;
    let t = -front.len * 0.3;
    if !c.inside_local(front.at(t - 3.1, -0.7)) || !c.inside_local(front.at(t - 1.3, -2.5)) {
        return;
    }
    front.bx(g, f, Panel, t - 2.2, y + 1.3, -1.6, Vec3::new(1.8, 2.6, 1.8), mul(pick(rng, &c.palette.panel), 0.95));
    front.decal(g, f, Glow, t - 2.2, y + 1.6, -0.68, 1.2, 0.8, mul(c.style.window_color(0), 0.7));
    front.bx(g, f, Matte, t, y + 0.5, -1.0, Vec3::new(0.3, 1.0, 0.3), CHARCOAL);
    front.bx(g, f, Matte, t + 1.9, y + 1.0, -1.0, Vec3::new(4.0, 0.08, 0.08), mul(c.palette.warning_light, 0.4));
    front.bx(g, f, Matte, t + 1.9, y + 1.0, -1.0, Vec3::new(4.0, 0.06, 0.12), c.palette.chalk);
    // P sign
    front.bx(g, f, Matte, front.len * 0.35, y + 3.2, -0.6, Vec3::new(0.15, 3.6, 0.15), c.palette.steel);
    front.bx(g, f, Glow, front.len * 0.35, y + 4.6, -0.6, Vec3::new(1.2, 1.2, 0.1), c.palette.parking_sign);
    front.decal(g, f, Matte, front.len * 0.35, y + 4.6, -0.54, 0.5, 0.8, c.palette.chalk);
}

/// Surface lot: asphalt, painted bays, a booth, a barrier, lamps and a fence.
pub fn parking_lot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    let occupancy = c.parking.unwrap_or(0.3);
    let yard = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.2 + 0.4 * c.wear));
    let front = c.front_face(&yard);
    let b = bays(&yard, 3.0);
    fill_bays(g, c, rng, &b, 0.045, occupancy, mul(pal.lane_marker, 0.6));
    gate(g, c, rng, &front, 0.0);
    for (i, face) in c.faces(&yard).iter().enumerate() {
        if face.is_front {
            continue;
        }
        fence(g, c, face.a, face.b, 1.8, false);
        if i % 2 == 0 && face.len > 10.0 {
            lamp_post_in(g, c, face.at(0.0, -1.2), 7.0, pal.street_lamp);
        }
    }
    lamp_post_in(g, c, front.at(front.len * 0.1, -1.4), 7.0, pal.street_lamp);
    if c.wear > 0.4 {
        weeds(g, c, rng, &yard, 6);
    }
}

/// Multi-storey car park: open concrete decks with parapets, a ramp, a
/// stair core, cars along the open edges and on the roof.
pub fn car_park(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let occupancy = c.parking.unwrap_or(0.3);
    let floors = (c.floors as i32).clamp(2, 6);
    let fh = 3.0;
    let poly = &c.poly;
    let concrete = mul(pick(rng, &pal.concrete), 1.0 - 0.25 * c.wear);
    let front = c.front_face(poly);
    let deck_bays = bays(poly, 1.8);
    for k in 0..=floors {
        let y = k as f32 * fh;
        // Deck slab and parapet; the ground floor has no slab of its own.
        if k > 0 {
            extrude(g, f, Concrete, poly, y - 0.4, y, concrete);
        }
        if k < floors {
            parapet(g, c, poly, y, 1.1, 0.25, Concrete, mul(concrete, 1.05));
            pilotis(g, c, &offset_poly(poly, 0.6), y, y + fh, 7.5, 0.5, Concrete, mul(concrete, 0.95));
            // Cars visible along the open edges of each deck.
            let edge_bays: Vec<(Vec2, f32)> = deck_bays.iter().copied().filter(|(p, _)| !nd_sim::zoning::point_in_polygon(*p, &offset_poly(poly, 7.0))).collect();
            fill_bays(g, c, rng, &edge_bays, y + 0.01, occupancy * 0.9, mul(pal.lane_marker, 0.5));
        } else {
            // Roof deck: full bay layout, lamps and the lift head.
            fill_bays(g, c, rng, &deck_bays, y + 0.01, occupancy, mul(pal.lane_marker, 0.6));
            parapet(g, c, poly, y, 1.1, 0.25, Concrete, mul(concrete, 1.05));
            for face in c.faces(poly) {
                if face.len > 12.0 {
                    let p = face.at(0.0, -1.0);
                    let w = f.to_world(p.x, p.y);
                    g.part(Matte).cylinder(w, 0.1, 0.08, y, y + 5.0, 8, rgba(CHARCOAL));
                    g.part(Glow).boxed(Vec3::new(w.x, y + 4.9, w.y), Vec3::new(0.5, 0.15, 0.5), 0.0, rgba(pal.street_lamp));
                }
            }
        }
    }
    let top = floors as f32 * fh;
    // Ramp: a sloped slab along the back face, one storey per run.
    let back = c.faces(poly).into_iter().filter(|x| !x.is_front).max_by(|a, b| a.len.total_cmp(&b.len));
    if let Some(back) = back {
        // Shorten the run until the 5 m wide slab is on the lot (a wedge
        // narrows behind its back face).
        let mut run = back.len * 0.4;
        while run > 4.0 && ![-run, run].iter().all(|t| c.inside_local(back.at(*t, -1.0)) && c.inside_local(back.at(*t, -6.0))) {
            run -= 1.0;
        }
        for k in 0..floors.min(if run > 4.0 { floors } else { 0 }) {
            let y0 = k as f32 * fh;
            let a = f.at2(back.at(-run, -3.5), y0 + 0.2);
            let b = f.at2(back.at(run, -3.5), y0 + fh + 0.2);
            let (a, b) = if k % 2 == 0 { (a, b) } else { (Vec3::new(b.x, a.y, b.z), Vec3::new(a.x, b.y, a.z)) };
            g.part(Concrete).slab(a, b, 5.0, 0.4, rgba(mul(concrete, 0.9)));
        }
    }
    // Stair and lift core, P sign, barrier at the entrance.
    let core = fit_inside(c, &rect(c.fw * 0.35, -c.fd * 0.2, 2.5, 3.0));
    extrude(g, f, Concrete, &core, 0.0, top + 3.0, mul(concrete, 0.85));
    extrude(g, f, Glow, &offset_poly(&core, 0.2), top + 2.2, top + 2.7, mul(pal.window_cold, 0.6));
    gate(g, c, rng, &front, 0.0);
    front.decal(g, f, Glow, 0.0, fh * 0.5 + 0.4, 0.05, (front.len * 0.5).min(10.0), 0.8, pal.parking_sign);
    // Level markers glow on each deck edge.
    for k in 1..floors {
        front.decal(g, f, Glow, -front.len * 0.4, k as f32 * fh + 0.6, 0.05, 0.6, 0.6, mul(pal.window_warm, 0.8));
    }
    grime(g, c, rng, &front, 0.0, top, 2.0);
}
