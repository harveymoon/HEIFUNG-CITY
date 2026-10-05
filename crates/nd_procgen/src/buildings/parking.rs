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
fn bays(c: &Ctx, poly: &[Vec2], margin: f32) -> Vec<(Vec2, f32)> {
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
    let _ = c;
    out
}

/// Paint the bay lines and put cars in a share of the bays.
fn fill_bays(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, bays: &[(Vec2, f32)], y: f32, occupancy: f32, line: [f32; 3]) {
    let f = &c.frame;
    for (p, yaw) in bays {
        // Side lines of the bay, along its depth.
        for s in [-1.0f32, 1.0] {
            let (sn, cs) = yaw.sin_cos();
            let off = Vec2::new(-sn, cs) * (BAY_W * 0.5 * s);
            let q = *p + off;
            f.bx_yaw(g, Glow, q.x, y + 0.01, q.y, Vec3::new(BAY_D * 0.9, 0.01, 0.1), line, *yaw);
        }
    }
    let n = ((bays.len() as f32) * occupancy.clamp(0.0, 1.0)).round() as usize;
    let mut order: Vec<usize> = (0..bays.len()).collect();
    let mut pick_rng = rng_for(c.seed, 77);
    order.shuffle(&mut pick_rng);
    for &i in order.iter().take(n) {
        let (p, yaw) = bays[i];
        let col = pick(rng, &c.palette.car_colors);
        let q = f.to_world(p.x, p.y);
        let a = f.yaw() + yaw;
        // Draw straight into world space at deck height.
        let m = |g: &mut Geometry, s, dx: f32, dy: f32, dz: f32, size: Vec3, col: [f32; 3]| {
            let (sn, cs) = a.sin_cos();
            g.part(s).boxed(Vec3::new(q.x + dx * cs - dz * sn, y + dy, q.y + dx * sn + dz * cs), size, a, rgba(col));
        };
        m(g, Metal, 0.0, 0.7, 0.0, Vec3::new(4.2, 0.8, 1.85), col);
        m(g, Glass, -0.3, 1.4, 0.0, Vec3::new(2.2, 0.6, 1.6), [0.12, 0.15, 0.18]);
        for (dx, dz) in [(-1.35f32, -0.9f32), (1.35, -0.9), (-1.35, 0.9), (1.35, 0.9)] {
            m(g, Matte, dx, 0.33, dz, Vec3::new(0.66, 0.66, 0.24), DARK);
        }
    }
}

/// Entrance booth, barrier arm and a lit P sign on the front face.
fn gate(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, front: &Face, y: f32) {
    let f = &c.frame;
    let t = -front.len * 0.3;
    front.bx(g, f, Panel, t - 2.2, y + 1.3, -1.6, Vec3::new(1.8, 2.6, 1.8), mul(pick(rng, &c.palette.panel), 0.95));
    front.bx(g, f, Glow, t - 2.2, y + 1.6, -0.68, Vec3::new(1.2, 0.8, 0.04), mul(c.style.window_colors[0], 0.7));
    front.bx(g, f, Matte, t, y + 0.5, -1.0, Vec3::new(0.3, 1.0, 0.3), CHARCOAL);
    front.bx(g, f, Matte, t + 1.9, y + 1.0, -1.0, Vec3::new(4.0, 0.08, 0.08), [0.8, 0.12, 0.1]);
    front.bx(g, f, Matte, t + 1.9, y + 1.0, -1.0, Vec3::new(4.0, 0.06, 0.12), [0.95, 0.95, 0.9]);
    // P sign
    front.bx(g, f, Matte, front.len * 0.35, y + 3.2, -0.6, Vec3::new(0.15, 3.6, 0.15), c.palette.steel);
    front.bx(g, f, Glow, front.len * 0.35, y + 4.6, -0.6, Vec3::new(1.2, 1.2, 0.1), [0.3, 0.6, 2.4]);
    front.bx(g, f, Matte, front.len * 0.35, y + 4.6, -0.5, Vec3::new(0.5, 0.8, 0.02), [0.98, 0.98, 1.0]);
}

/// Surface lot: asphalt, painted bays, a booth, a barrier, lamps and a fence.
pub fn parking_lot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let occupancy = c.parking.unwrap_or(0.3);
    let yard = offset_poly(&c.lot_poly, 0.4);
    ground(g, f, Asphalt, &yard, 0.04, mul(pal.asphalt, 1.2 + 0.4 * c.wear));
    let front = c.front_face(&yard);
    let b = bays(c, &yard, 3.0);
    fill_bays(g, c, rng, &b, 0.045, occupancy, mul(pal.lane_marker, 0.6));
    gate(g, c, rng, &front, 0.0);
    for (i, face) in c.faces(&yard).iter().enumerate() {
        if face.is_front {
            continue;
        }
        fence(g, c, face.a, face.b, 1.8, false);
        if i % 2 == 0 && face.len > 10.0 {
            lamp_post(g, f, face.at(0.0, -1.2), 7.0, pal.street_lamp);
        }
    }
    lamp_post(g, f, front.at(front.len * 0.1, -1.4), 7.0, pal.street_lamp);
    if c.wear > 0.4 {
        super::industrial::weeds(g, c, rng, &yard, 6);
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
    let deck_bays = bays(c, poly, 1.8);
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
        for k in 0..floors {
            let y0 = k as f32 * fh;
            let a = f.at2(back.at(-back.len * 0.4, -3.5), y0 + 0.2);
            let b = f.at2(back.at(back.len * 0.4, -3.5), y0 + fh + 0.2);
            let (a, b) = if k % 2 == 0 { (a, b) } else { (Vec3::new(b.x, a.y, b.z), Vec3::new(a.x, b.y, a.z)) };
            g.part(Concrete).beam(a, b, 5.0, rgba(mul(concrete, 0.9)));
        }
    }
    // Stair and lift core, P sign, barrier at the entrance.
    let core = fit_inside(c, &rect(c.fw * 0.35, -c.fd * 0.2, 2.5, 3.0));
    extrude(g, f, Concrete, &core, 0.0, top + 3.0, mul(concrete, 0.85));
    extrude(g, f, Glow, &offset_poly(&core, 0.2), top + 2.2, top + 2.7, mul(pal.window_cold, 0.6));
    gate(g, c, rng, &front, 0.0);
    front.bx(g, f, Glow, 0.0, fh * 0.5 + 0.4, 0.05, Vec3::new((front.len * 0.5).min(10.0), 0.8, 0.04), [0.3, 0.6, 2.4]);
    // Level markers glow on each deck edge.
    for k in 1..floors {
        front.bx(g, f, Glow, -front.len * 0.4, k as f32 * fh + 0.6, 0.05, Vec3::new(0.6, 0.6, 0.04), mul(pal.window_warm, 0.8));
    }
    grime(g, c, rng, &front, 0.0, top, 2.0);
}
