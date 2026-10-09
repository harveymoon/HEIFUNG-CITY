//! [land] Homes by class and density, and the forms bought and merged plots
//! are built in: one-storey shacks, trailer parks and huts for the poor
//! where the city is quiet; suburbs of ordinary houses for the middle
//! class; McMansions (two plots, one house, a pool); billionaires'
//! mansions with grounds and a garden; mega hotels (valet, roof pool,
//! sky-bridges) and museums grown out of galleries.
//!
//! All lot-local: `u` along the frontage, `v` inward from the street (the
//! front is at negative `v`).

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::zoning::point_in_polygon;
use nd_sim::Tier;
use rand::Rng;

const LAWN: [f32; 3] = [0.16, 0.30, 0.10];
const LAWN_RICH: [f32; 3] = [0.12, 0.34, 0.09];
const GRAVEL: [f32; 3] = [0.42, 0.40, 0.36];
const WHITE: [f32; 3] = [0.86, 0.85, 0.82];
const CREAM: [f32; 3] = [0.82, 0.76, 0.64];

/// The ridge of a rectangle's gable roof, as a quad of the same vertex
/// order (for `loft`): along its longer side, a hand's width thick.
fn ridge(r: &[Vec2]) -> Vec<Vec2> {
    if r.len() != 4 {
        return r.to_vec();
    }
    let (p0, p1, p2, p3) = (r[0], r[1], r[2], r[3]);
    if p0.distance(p1) >= p1.distance(p2) {
        let (a, b) = (p0.lerp(p3, 0.5), p1.lerp(p2, 0.5));
        let e = (p3 - p0).normalize_or_zero() * 0.06;
        vec![a - e, b - e, b + e, a + e]
    } else {
        let (a, b) = (p0.lerp(p1, 0.5), p3.lerp(p2, 0.5));
        let e = (p1 - p0).normalize_or_zero() * 0.06;
        vec![a - e, a + e, b + e, b - e]
    }
}

/// A gabled house on a rectangle fitted to the lot: walls, a pitched roof
/// with eaves, windows round every storey and a front door. The wall
/// outline it stood on, or `None` when it would not fit.
#[allow(clippy::too_many_arguments)]
fn house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, cu: f32, cv: f32, hw: f32, hd: f32, yaw: f32, storeys: u8, wall: (Surface, [f32; 3]), roof: [f32; 3], pitch: f32) -> Option<Vec<Vec2>> {
    let f = &c.frame;
    let p = fit_inside(c, &rect_yaw(cu, cv, hw, hd, yaw));
    // Pushed off its spot to fit (an odd or small lot): left out rather
    // than stacked onto another house.
    if p.len() != 4 || !poly_inside(c, &p) || p[0].distance(p[1]) < 3.0 || poly_center(&p).distance(Vec2::new(cu, cv)) > 1.5 {
        return None;
    }
    let fh = 3.0;
    let top = fh * storeys.max(1) as f32;
    extrude(g, f, wall.0, &p, 0.0, top, wall.1);
    let eaves = offset_poly(&p, -0.45);
    if eaves.len() == 4 {
        extrude(g, f, Matte, &eaves, top, top + 0.12, mul(roof, 0.8));
        loft(g, f, Surface::Panel, &eaves, &ridge(&eaves), top + 0.12, top + 0.12 + pitch, roof);
    }
    let mut spec = WindowSpec::from_ctx(c);
    spec.w = 1.3;
    spec.h = 1.4;
    spec.spacing = 2.8;
    spec.sill = 0.9;
    spec.ac = 0.0;
    spec.boarded = if c.tier == Tier::Low { 0.1 } else { 0.0 };
    spec.frame = WHITE;
    for face in c.faces(&p) {
        if face.len > 2.5 {
            windows(g, c, rng, &face, 0.0, storeys.max(1) as i32, fh, &spec);
        }
    }
    let front = c.front_face(&p);
    let t = (rng.random::<f32>() - 0.5) * front.len * 0.3;
    // The canopy stands out from the door: only where there is lot for it.
    if room(c, &front, t - 1.2, t + 1.2, 1.6) {
        entrance(g, c, rng, &front, t, 1.1, 2.2);
    }
    Some(p)
}


/// Is there lot in front of `face` from `t0` to `t1` along it, `d` out?
fn room(c: &Ctx, face: &super::Face, t0: f32, t1: f32, d: f32) -> bool {
    (0..=6).all(|k| c.inside_local(face.at(t0 + (t1 - t0) * k as f32 / 6.0, d)))
}

/// A row of fence posts and rails (picket or chain-link) from a to b.
fn picket(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    let face = super::Face::from_edge(a, b);
    if face.len < 0.5 {
        return;
    }
    face.bx(g, f, Wood, 0.0, h * 0.75, 0.0, Vec3::new(face.len, 0.08, 0.06), color);
    face.bx(g, f, Wood, 0.0, h * 0.3, 0.0, Vec3::new(face.len, 0.08, 0.06), color);
    let n = (face.len / 1.4).ceil() as i32;
    for k in 0..=n {
        let t = -face.len * 0.5 + face.len * k as f32 / n.max(1) as f32;
        face.bx(g, f, Wood, t, h * 0.5, 0.0, Vec3::new(0.09, h, 0.09), color);
    }
}

/// A parked car at `p` if it fits.
fn car(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32, paint: Option<[f32; 3]>) {
    if vehicle_fits(c, p, yaw, Vehicle::Car) {
        let col = paint.unwrap_or_else(|| pick(rng, &c.palette.car_colors));
        vehicle(g, &c.frame, c.palette, rng, p, 0.0, yaw, Vehicle::Car, col);
    }
}

// ── shacks ───────────────────────────────────────────────────────────────────

/// One-storey shacks of tin, ply and tarp on bare earth, a scrap fence, a
/// drum fire and washing lines between them.
pub fn shacks(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.3, Dirt, mul(pal.lot_vacant, 0.85));
    let fi = c.front_face_index(&yard_poly);
    fence_poly(g, c, &yard_poly, 1.7, fi, false);
    let (w, d) = (c.fw, c.fd);
    let cols = ((w / 7.5) as i32).clamp(1, 8);
    let rows = ((d / 8.5) as i32).clamp(1, 8);
    let mut spots: Vec<Vec2> = Vec::new();
    for i in 0..cols {
        for j in 0..rows {
            if rng.random_bool(0.12) {
                continue;
            }
            let cu = -w * 0.5 + (i as f32 + 0.5) * w / cols as f32 + rng.random_range(-0.8..0.8);
            let cv = -d * 0.5 + (j as f32 + 0.5) * d / rows as f32 + rng.random_range(-0.8..0.8);
            let (hw, hd) = (rng.random_range(1.5..2.6), rng.random_range(1.4..2.3));
            let p = fit_inside(c, &rect_yaw(cu, cv, hw, hd, rng.random_range(-0.25..0.25)));
            if p.len() != 4 || !poly_inside(c, &p) || p[0].distance(p[1]) < 2.0 {
                continue;
            }
            let (s, col) = match rng.random_range(0..3) {
                0 => (Rust, mul(pal.rust, rng.random_range(0.8..1.1))),
                1 => (Wood, mul(pal.plywood, rng.random_range(0.75..1.0))),
                _ => (Metal, mul(pick(rng, &pal.metal_paint), 0.7)),
            };
            let h = rng.random_range(2.2..2.6);
            extrude(g, f, s, &p, 0.0, h, col);
            // A tin or tarp roof lid, overhanging.
            let lid = offset_poly(&p, -0.3);
            let roof = if rng.random_bool(0.5) { (Tarp, pick(rng, &pal.tarp_colors)) } else { (Rust, mul(pal.rust, 0.8)) };
            extrude(g, f, roof.0, &lid, h, h + 0.12, roof.1);
            let front = c.front_face(&p);
            front.decal(g, f, Matte, 0.0, 0.95, 0.03, 0.85, 1.85, CHARCOAL);
            if rng.random_bool(0.6) {
                front.glaze(g, c, front.len * 0.28, 1.4, 0.03, 0.6, 0.5, Pane::Dark);
            }
            if rng.random_bool(0.3) {
                f.bx(g, Tarp, poly_center(&p).x, h + 0.2, poly_center(&p).y, Vec3::new(1.2, 0.05, 1.0), pick(rng, &pal.tarp_colors));
            }
            spots.push(poly_center(&p));
        }
    }
    // Washing lines between neighbours, a drum fire and a scrap heap.
    for k in 1..spots.len().min(6) {
        let (a, b) = (spots[k - 1], spots[k]);
        if a.distance(b) < 12.0 {
            string_lights(g, f, Vec3::new(a.x, 2.0, a.y), Vec3::new(b.x, 2.0, b.y), 5, mul(pal.lamp, 0.25));
        }
    }
    if let Some(p) = c.random_inside(rng, 2.0) {
        drum_fire(g, f, pal, p);
    }
    if let Some(p) = c.random_inside(rng, 3.0) {
        scrap_pile(g, c, rng, p, 1.6, 1.2);
    }
    weeds(g, c, rng, &yard_poly, 6);
}

// ── trailer park ─────────────────────────────────────────────────────────────

/// Mobile homes in two rows off a gravel lane, each with a porch deck, an
/// awning, a propane tank and maybe a car; a sign at the gate.
pub fn trailer_park(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.3, Dirt, mix(GRAVEL, pal.lot_vacant, 0.4));
    let (w, d) = (c.fw, c.fd);
    // The lane up the middle.
    let lane = fit_inside(c, &rect(0.0, 0.0, 2.2, d * 0.5));
    ground(g, f, Paving, &lane, 0.05, mul(GRAVEL, 0.8));
    let pastel = [[0.78, 0.74, 0.62], [0.62, 0.72, 0.74], [0.80, 0.70, 0.66], [0.70, 0.76, 0.64], [0.86, 0.84, 0.80]];
    let side_w = (w * 0.5 - 2.6).max(3.0);
    let per = ((d - 2.0) / 7.0).floor().max(1.0) as i32;
    for side in [-1.0f32, 1.0] {
        for k in 0..per {
            if rng.random_bool(0.1) {
                continue;
            }
            let cv = -d * 0.5 + 1.5 + (k as f32 + 0.5) * (d - 2.0) / per as f32;
            let hl = (side_w * 0.5 - 0.3).clamp(2.5, 7.0);
            let cu = side * (2.6 + hl + 0.2);
            let body = fit_inside(c, &rect(cu, cv, hl, 1.35));
            if body.len() != 4 || !poly_inside(c, &body) || body[0].distance(body[1]) < 4.0 {
                continue;
            }
            let col = pastel[rng.random_range(0..pastel.len())];
            let col = mul(col, 1.0 - 0.3 * c.wear);
            extrude(g, f, Metal, &body, 0.5, 0.75, mul(col, 0.5));
            extrude(g, f, Panel, &body, 0.75, 3.1, col);
            extrude(g, f, Metal, &offset_poly(&body, -0.08), 3.1, 3.25, mul(WHITE, 0.8));
            let center = poly_center(&body);
            // Porch deck on the lane side.
            let porch_u = center.x - side * (body[0].distance(body[1]) * 0.5 + 1.2);
            let porch = rect(porch_u, center.y, 1.1, 1.0);
            if poly_inside(c, &porch) {
                extrude(g, f, Wood, &porch, 0.0, 0.45, pal.wood);
            }
            for face in c.faces(&body) {
                if face.len > 4.0 {
                    let n = (face.len / 2.6) as i32;
                    for i in 0..n {
                        let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / n as f32;
                        face.glaze(g, c, t, 1.9, 0.03, 0.9, 0.7, Pane::Dark);
                        if coin(rng, c.lit_chance() * 0.6) {
                            let lc = mul(pick(rng, &c.style.window_colors), 0.6);
                            face.glaze(g, c, t, 1.9, 0.05, 0.8, 0.6, Pane::Lit(lc));
                        }
                    }
                }
            }
            if rng.random_bool(0.5) {
                tank_horizontal(g, f, Vec2::new(center.x + side * 1.0, center.y + 1.9), 0.0, 1.1, 0.35, 0.35, WHITE);
            }
            if rng.random_bool(0.45) {
                car(g, c, rng, Vec2::new(center.x, center.y - 2.4), 0.0, None);
            }
        }
    }
    // A sign at the gate and a couple of lamps.
    let front = c.front_face(&yard_poly);
    if room(c, &front, -4.4, -2.4, -0.7) {
        front.bx(g, f, Wood, -3.4, 1.2, -0.6, Vec3::new(0.15, 2.4, 0.15), pal.wood);
        front.decal(g, f, Glow, -3.4, 2.2, -0.5, 1.8, 0.5, mul(pick(rng, &pal.sign_colors), 0.8));
    }
    for t in [-0.3f32, 0.3] {
        lamp_post_in(g, c, Vec2::new(t * w * 0.5, 0.0), 3.5, mul(pal.lamp, 0.6));
    }
}

// ── huts ─────────────────────────────────────────────────────────────────────

/// Round huts with thatched cones round a fire pit, vegetable rows and
/// water barrels: the poorest homes on the cheapest quiet land.
pub fn huts(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.3, Dirt, mul(pal.lot_vacant, 0.95));
    let centre = interior_anchor(&yard_poly);
    let r_ring = (c.fw.min(c.fd) * 0.5 - 3.4).max(2.5);
    // Fire pit.
    extrude(g, f, Stone, &ngon(centre.x, centre.y, 1.1, 8, 0.0), 0.0, 0.3, pal.stone);
    drum_fire(g, f, pal, centre);
    let n = ((r_ring * std::f32::consts::TAU / 6.5) as usize).clamp(3, 9);
    let thatch = mix(pal.foliage_dry, pal.wood, 0.4);
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU + rng.random_range(-0.15..0.15);
        let p = centre + Vec2::new(a.cos(), a.sin()) * r_ring;
        let r = rng.random_range(1.8..2.5);
        let ring = ngon(p.x, p.y, r, 10, 0.0);
        if !poly_inside(c, &ring) {
            continue;
        }
        let wall = if rng.random_bool(0.5) { (Plaster, mul(pick(rng, &pal.plaster), 0.75)) } else { (Wood, mul(pal.wood, 0.85)) };
        extrude(g, f, wall.0, &ring, 0.0, 2.0, wall.1);
        loft(g, f, Tarp, &ngon(p.x, p.y, r + 0.5, 10, 0.0), &ngon(p.x, p.y, 0.15, 10, 0.0), 2.0, 3.8, thatch);
        // The door faces the fire.
        let face = super::Face::new(p + (centre - p).normalize_or_zero().perp() * 0.6, p - (centre - p).normalize_or_zero().perp() * 0.6, (centre - p).normalize_or_zero());
        let door = super::Face::new(face.a + face.out * (r - 0.6), face.b + face.out * (r - 0.6), face.out);
        door.decal(g, f, Matte, 0.0, 0.9, 0.6, 0.8, 1.7, DARK);
    }
    // Vegetable rows along the back.
    for k in 0..4 {
        let v = c.fd * 0.5 - 2.0 - k as f32 * 1.4;
        let row = fit_inside(c, &rect(0.0, v, c.fw * 0.3, 0.4));
        if poly_inside(c, &row) {
            ground(g, f, Foliage, &row, 0.08, mul(pal.foliage, 0.9));
        }
    }
    for _ in 0..3 {
        if let Some(p) = c.random_inside(rng, 2.0) {
            g.part(Rust).cylinder(f.to_world(p.x, p.y), 0.35, 0.35, 0.0, 0.95, 10, rgba(mul(pal.metal_paint(0), 0.6)));
        }
    }
}

// ── suburb ───────────────────────────────────────────────────────────────────

/// Ordinary detached houses on lawns behind a front yard: a bungalow
/// behind chain-link for the cheap tier, a two-storey house with a garage
/// and a picket fence for the middle, a bigger one with hedges and a pool
/// for the top.
pub fn suburb(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lawn = if c.tier == Tier::Low { mix(LAWN, pal.foliage_dry, 0.5) } else { LAWN };
    yard(g, c, 0.2, Foliage, lawn);
    let (w, d) = (c.fw, c.fd);
    let n = ((w / 17.0) as i32).clamp(1, 3);
    let bay = w / n as f32;
    let storeys = match c.tier {
        Tier::Low => 1,
        Tier::Mid => c.floors.clamp(1, 2),
        Tier::High => 2,
    };
    let roof = match c.tier {
        Tier::Low => mul(pal.rust, 0.6),
        Tier::Mid => [0.30, 0.16, 0.12],
        Tier::High => [0.20, 0.22, 0.26],
    };
    for k in 0..n {
        let cu = -w * 0.5 + (k as f32 + 0.5) * bay;
        let garage = c.tier != Tier::Low && bay > 14.0;
        let hw = ((bay * 0.5) - if garage { 4.4 } else { 1.8 }).clamp(3.0, 7.5);
        let hd = (d * 0.3).clamp(3.5, 5.5);
        let cv = -d * 0.5 + 5.5 + hd;
        let hu = cu - if garage { 1.6 } else { 0.0 };
        let wall = match c.tier {
            Tier::Low => (Panel, mul(pick(rng, &pal.plaster), 0.8)),
            Tier::Mid => if rng.random_bool(0.5) { (Brick, pick(rng, &pal.brick)) } else { (Plaster, pick(rng, &pal.plaster)) },
            Tier::High => (Plaster, mix(pick(rng, &pal.plaster), WHITE, 0.4)),
        };
        let Some(h) = house(g, c, rng, hu, cv, hw, hd, 0.0, storeys, wall, roof, 1.8) else { continue };
        let hc = poly_center(&h);
        let right = h.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        if garage {
            let gp = fit_inside(c, &rect(right + 2.0, hc.y - hd * 0.3, 1.9, 3.0));
            if poly_inside(c, &gp) {
                extrude(g, f, wall.0, &gp, 0.0, 2.8, wall.1);
                extrude(g, f, Matte, &offset_poly(&gp, -0.2), 2.8, 3.0, mul(roof, 0.9));
                let gf = c.front_face(&gp);
                roller_door(g, c, &gf, 0.0, 2.8, 2.2, mul(WHITE, 0.85));
                // The drive from the street to the garage door.
                let front_v = -d * 0.5;
                let gfront = gp.iter().map(|p| p.y).fold(f32::MAX, f32::min);
                let drive = fit_inside(c, &rect(right + 2.0, (front_v + gfront) * 0.5, 1.5, ((gfront - front_v) * 0.5).max(0.5)));
                ground(g, f, Paving, &drive, 0.05, mul(pal.paving, 1.05));
                car(g, c, rng, Vec2::new(right + 2.0, (front_v + gfront) * 0.5), std::f32::consts::FRAC_PI_2, None);
            }
        } else if c.tier != Tier::Low || rng.random_bool(0.4) {
            car(g, c, rng, Vec2::new(right + 1.6, cv - hd - 1.0), std::f32::consts::FRAC_PI_2, None);
        }
        // Path to the door.
        let path = fit_inside(c, &rect(hu, (-d * 0.5 + cv - hd) * 0.5, 0.6, ((cv - hd + d * 0.5) * 0.5).max(0.3)));
        ground(g, f, Paving, &path, 0.06, mul(pal.paving, 1.15));
        // The front boundary.
        let (a, b) = (Vec2::new(cu - bay * 0.5 + 0.4, -d * 0.5 + 0.5), Vec2::new(cu + bay * 0.5 - 0.4, -d * 0.5 + 0.5));
        let gap_l = Vec2::new(hu - 0.9, a.y);
        let gap_r = Vec2::new(if garage { right + 3.6 } else { hu + 0.9 }, a.y);
        match c.tier {
            Tier::Low if c.all_inside(&[a, b]) => fence(g, c, a, b, 1.2, false),
            Tier::Low => {}
            Tier::Mid => {
                if c.all_inside(&[a, gap_l]) {
                    picket(g, c, a, gap_l, 1.0, WHITE);
                }
                if c.all_inside(&[gap_r, b]) {
                    picket(g, c, gap_r, b, 1.0, WHITE);
                }
            }
            Tier::High => {
                if c.all_inside(&[a, gap_l]) {
                    hedge(g, c, a, gap_l, 1.4, 0.8, pal.foliage);
                }
                if c.all_inside(&[gap_r, b]) {
                    hedge(g, c, gap_r, b, 1.4, 0.8, pal.foliage);
                }
            }
        }
        // Back garden: a tree, and the top tier's pool.
        let back = (cv + hd + d * 0.5) * 0.5;
        if c.tier == Tier::High && poly_inside(c, &rect(hu, back + 1.5, 3.9, 2.4)) {
            pool(g, c, hu, back + 1.5, 6.0, 3.0);
        } else {
            let p = Vec2::new(cu + rng.random_range(-bay * 0.3..bay * 0.3), back + 2.0);
            let canopy = rng.random_range(2.5..4.0);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > canopy {
                tree_in(g, f, pal, p, rng.random_range(4.0..6.5), canopy, pal.foliage);
            }
        }
        // A mailbox by the path.
        if c.inside_local(Vec2::new(hu + 1.2, -d * 0.5 + 1.0)) {
            f.bx(g, Metal, hu + 1.2, 0.6, -d * 0.5 + 1.0, Vec3::new(0.3, 1.2, 0.3), mul(pick(rng, &pal.metal_paint), 0.8));
        }
    }
}

// ── McMansion ────────────────────────────────────────────────────────────────

/// Two plots, one house: a two-storey main block with jutting gabled
/// wings, a columned porch, a three-car garage, a looping drive with a
/// fountain, a long pool behind on a patio, hedges and ornamental trees.
pub fn mcmansion(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.2, Foliage, LAWN_RICH);
    let (w, d) = (c.fw, c.fd);
    let wall = (Plaster, mix(pick(rng, &pal.plaster), CREAM, 0.6));
    let roof = [0.22, 0.20, 0.22];
    let hw = (w * 0.26).clamp(6.0, 13.0);
    let hd = (d * 0.2).clamp(5.0, 9.0);
    let cv = -d * 0.5 + 9.0 + hd;
    let main = house(g, c, rng, 0.0, cv, hw, hd, 0.0, 2, wall, roof, 3.2);
    let Some(main) = main else {
        super::residential::villas(g, c, rng);
        return;
    };
    let mc = poly_center(&main);
    // Wings either side, set forward, with their own gables.
    for side in [-1.0f32, 1.0] {
        // Clear of the main block, so no two walls share a plane.
        let wu = mc.x + side * (hw + 4.0);
        let _ = house(g, c, rng, wu, mc.y - 1.5, 3.4, hd * 0.75, 0.0, 2, wall, roof, 2.6);
    }
    // Columned porch with a pediment.
    let front = c.front_face(&main);
    if room(c, &front, -3.8, 3.8, 4.0) {
        for k in -2..=2 {
            front.bx(g, f, Stone, k as f32 * 1.4, 2.6, 2.4, Vec3::new(0.5, 5.2, 0.5), WHITE);
        }
        front.bx(g, f, Stone, 0.0, 5.4, 2.4, Vec3::new(7.4, 0.4, 3.0), WHITE);
        let ped = front.at(0.0, 2.4);
        let pr = rect(ped.x, ped.y, 3.7, 1.5);
        loft(g, f, Stone, &pr, &ridge(&pr), 5.6, 7.0, WHITE);
    }
    // Garage wing: three doors.
    let gu = mc.x + hw + 8.5;
    let gp = fit_inside(c, &rect(gu, mc.y - hd * 0.2, 4.6, 3.2));
    if poly_inside(c, &gp) {
        extrude(g, f, wall.0, &gp, 0.0, 3.0, wall.1);
        let ge = offset_poly(&gp, -0.4);
        loft(g, f, Panel, &ge, &ridge(&ge), 3.0, 4.6, roof);
        let gf = c.front_face(&gp);
        // Up to three doors, as many as the wing's face holds once fitted.
        let n = ((gf.len - 0.4) / 2.9).floor().clamp(0.0, 3.0) as i32;
        for k in 0..n {
            roller_door(g, c, &gf, (k as f32 - (n - 1) as f32 * 0.5) * 2.9, 2.5, 2.2, WHITE);
        }
    }
    // Looping drive with a fountain in the middle.
    let front_v = -d * 0.5 + 0.4;
    let drive_v = (front_v + main.iter().map(|p| p.y).fold(f32::MAX, f32::min)) * 0.5;
    let loop_outer = fit_inside(c, &rect(0.0, drive_v, (hw + 1.0).min(w * 0.45), (drive_v - front_v).max(1.5)));
    ground(g, f, Paving, &loop_outer, 0.05, mul(pal.paving, 1.2));
    let lawn_in = offset_poly(&loop_outer, 2.6);
    if lawn_in.len() >= 3 {
        ground(g, f, Foliage, &lawn_in, 0.07, LAWN_RICH);
        let fc = poly_center(&lawn_in);
        extrude(g, f, Stone, &ngon(fc.x, fc.y, 1.3, 12, 0.0), 0.0, 0.5, WHITE);
        extrude(g, f, Glow, &ngon(fc.x, fc.y, 1.0, 12, 0.0), 0.5, 0.55, mul(pal.water, 0.8));
        extrude(g, f, Stone, &ngon(fc.x, fc.y, 0.2, 8, 0.0), 0.5, 1.8, WHITE);
    }
    car(g, c, rng, Vec2::new(-2.5, drive_v), 0.0, None);
    car(g, c, rng, Vec2::new(gu, front_v + 3.0), std::f32::consts::FRAC_PI_2, None);
    // Patio and a long pool behind.
    let back_v = main.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let pool_v = back_v + 5.0;
    let patio = fit_inside(c, &rect(0.0, pool_v, (hw * 0.8).min(9.0), 3.6));
    let pool_w = (hw * 1.1).min(12.0);
    if poly_inside(c, &patio) && poly_inside(c, &rect(0.0, pool_v, pool_w * 0.5 + 0.9, 2.6)) {
        ground(g, f, Paving, &patio, 0.06, mul(pal.paving, 1.25));
        pool(g, c, 0.0, pool_v, pool_w, 3.4);
        for k in -2..=2 {
            f.bx(g, Panel, k as f32 * 1.6, 0.35, pool_v - 2.8, Vec3::new(0.7, 0.3, 1.8), WHITE);
        }
    }
    // Hedge round the sides and back, ornamental trees, gate posts.
    let fi = c.front_face_index(&yard_poly);
    for (i, face) in c.faces(&yard_poly).iter().enumerate() {
        if Some(i) != fi {
            hedge(g, c, face.a, face.b, 1.8, 0.9, pal.foliage);
        }
    }
    for _ in 0..rng.random_range(4..8) {
        if let Some(p) = c.random_inside(rng, 2.5) {
            if main.iter().all(|q| q.distance(p) > 4.0) && !point_in_polygon(p, &loop_outer) {
                tree_in(g, f, pal, p, rng.random_range(3.5..6.0), rng.random_range(1.6..2.6), mul(pal.foliage, 1.1));
            }
        }
    }
    for s in [-1.0f32, 1.0] {
        let p = Vec2::new(s * 3.0, front_v + 0.3);
        if c.all_inside(&rect(p.x, p.y, 0.5, 0.5)) {
            f.bx(g, Stone, p.x, 1.2, p.y, Vec3::new(0.7, 2.4, 0.7), WHITE);
            f.bx(g, Glow, p.x, 2.5, p.y, Vec3::new(0.4, 0.3, 0.4), mul(pal.lamp, 0.8));
        }
    }
}

// ── mansion ──────────────────────────────────────────────────────────────────

/// A billionaire's estate over the merged plots: a stone wall with a gate
/// and a guardhouse, a long drive to a forecourt with a fountain, a
/// three-storey house with wings and a colonnade, a formal garden of
/// hedges and gravel walks, a pool and pool house, a tennis court, a
/// helipad, and supercars at the door.
pub fn mansion(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.2, Foliage, LAWN_RICH);
    let (w, d) = (c.fw, c.fd);
    let stone = mix(pal.stone, CREAM, 0.5);
    // Wall round the grounds, a gate in the middle of the front.
    let fi = c.front_face_index(&yard_poly);
    let faces = c.faces(&yard_poly);
    for (i, face) in faces.iter().enumerate() {
        if Some(i) == fi {
            let side = (face.len - 6.0) * 0.5;
            if side > 0.5 {
                face.bx(g, f, Stone, -(3.0 + side * 0.5), 1.3, -0.3, Vec3::new(side, 2.6, 0.5), stone);
                face.bx(g, f, Stone, 3.0 + side * 0.5, 1.3, -0.3, Vec3::new(side, 2.6, 0.5), stone);
            }
            for s in [-3.2f32, 3.2] {
                face.bx(g, f, Stone, s, 1.8, -0.3, Vec3::new(0.9, 3.6, 0.9), stone);
                face.bx(g, f, Glow, s, 3.75, -0.3, Vec3::new(0.5, 0.3, 0.5), mul(pal.lamp, 0.9));
            }
            face.bx(g, f, Metal, 0.0, 1.4, -0.3, Vec3::new(5.4, 2.6, 0.1), DARK);
            // Guardhouse inside the gate.
            if room(c, face, 4.0, 7.0, -4.0) {
                face.bx(g, f, Stone, 5.5, 1.4, -2.6, Vec3::new(2.6, 2.8, 2.4), stone);
                face.bx(g, f, Glass, 5.5, 1.8, -1.35, Vec3::new(2.0, 1.0, 0.06), pal.glass_dark);
            }
        } else {
            face.bx(g, f, Stone, 0.0, 1.3, -0.3, Vec3::new(face.len, 2.6, 0.5), stone);
        }
    }
    // The house: a central block and two wings, set well back.
    let hw = (w * 0.2).clamp(8.0, 16.0);
    let hd = (d * 0.13).clamp(6.0, 10.0);
    let cv = (-d * 0.5 + d * 0.38).min(d * 0.5 - hd - 14.0).max(-d * 0.5 + hd + 12.0);
    let wall = (Stone, stone);
    let roof = [0.20, 0.22, 0.25];
    let main = house(g, c, rng, 0.0, cv, hw, hd, 0.0, 3, wall, roof, 3.6);
    let Some(main) = main else {
        mcmansion(g, c, rng);
        return;
    };
    let mc = poly_center(&main);
    for s in [-1.0f32, 1.0] {
        let _ = house(g, c, rng, mc.x + s * (hw + 5.8), mc.y + 1.5, 5.0, hd * 0.8, 0.0, 2, wall, roof, 3.0);
    }
    // Colonnade across the front.
    let front = c.front_face(&main);
    if room(c, &front, -front.len * 0.5, front.len * 0.5, 4.4) {
        let ncol = ((front.len - 2.0) / 2.2) as i32;
        for k in 0..=ncol {
            let t = -front.len * 0.5 + 1.0 + k as f32 * (front.len - 2.0) / ncol.max(1) as f32;
            front.bx(g, f, Stone, t, 3.6, 2.6, Vec3::new(0.6, 7.2, 0.6), WHITE);
        }
        front.bx(g, f, Stone, 0.0, 7.4, 2.6, Vec3::new(front.len, 0.5, 3.4), WHITE);
    }
    let steps = rect(mc.x, mc.y - hd - 3.0, 5.0, 1.4);
    if poly_inside(c, &offset_poly(&steps, -1.6)) {
        plinth(g, c, &steps, 3, 0.18, 0.5, Stone, WHITE);
    }
    // Drive from the gate to a forecourt with a fountain.
    let front_v = -d * 0.5 + 0.5;
    let fore_v = mc.y - hd - 7.0;
    let drive = fit_inside(c, &rect(0.0, (front_v + fore_v) * 0.5, 2.6, ((fore_v - front_v) * 0.5).max(1.0)));
    ground(g, f, Paving, &drive, 0.05, mix(GRAVEL, WHITE, 0.3));
    let court = ngon(0.0, fore_v, 7.5, 20, 0.0);
    if poly_inside(c, &court) {
        ground(g, f, Paving, &court, 0.06, mix(GRAVEL, WHITE, 0.3));
        extrude(g, f, Stone, &ngon(0.0, fore_v, 2.6, 16, 0.0), 0.0, 0.6, WHITE);
        extrude(g, f, Glow, &ngon(0.0, fore_v, 2.2, 16, 0.0), 0.6, 0.65, mul(pal.water, 0.9));
        extrude(g, f, Stone, &ngon(0.0, fore_v, 0.35, 8, 0.0), 0.6, 2.6, WHITE);
        extrude(g, f, Stone, &ngon(0.0, fore_v, 1.0, 12, 0.0), 2.0, 2.2, WHITE);
        // Supercars at the door.
        let paints = [[0.55, 0.02, 0.02], [0.9, 0.55, 0.0], [0.02, 0.02, 0.025], [0.85, 0.85, 0.82]];
        for (k, s) in [-4.6f32, 4.6].iter().enumerate() {
            let paint = paints[(k + rng.random_range(0..2)) % paints.len()];
            car(g, c, rng, Vec2::new(*s, fore_v + 1.0), std::f32::consts::FRAC_PI_2, Some(paint));
        }
    }
    for t in [-0.5f32, 0.5] {
        for k in 0..4 {
            let p = Vec2::new(t * 7.0, front_v + 4.0 + k as f32 * (fore_v - front_v - 8.0).max(0.0) / 4.0);
            if c.inside_local(p) {
                tree_in(g, f, pal, p, 7.0, 1.4, mul(pal.foliage, 0.9));
            }
        }
    }
    // Formal garden behind: a grid of low hedges on gravel walks.
    let back_v = main.iter().map(|p| p.y).fold(f32::MIN, f32::max) + 3.0;
    let garden_d = (d * 0.5 - back_v - 3.0).max(0.0);
    if garden_d > 8.0 {
        let gw = (w * 0.5 - 4.0).min(28.0);
        let gpoly = fit_inside(c, &rect(-gw * 0.35, back_v + garden_d * 0.5, gw * 0.55, garden_d * 0.5));
        ground(g, f, Paving, &gpoly, 0.05, mix(GRAVEL, WHITE, 0.4));
        let gc = poly_center(&gpoly);
        for i in -1..=1 {
            for j in -1..=1 {
                let p = Vec2::new(gc.x + i as f32 * gw * 0.32, gc.y + j as f32 * garden_d * 0.3);
                let bed = rect(p.x, p.y, gw * 0.12, garden_d * 0.1);
                if poly_inside(c, &bed) {
                    extrude(g, f, Foliage, &bed, 0.0, 0.9, mul(pal.foliage, 0.85));
                    extrude(g, f, Foliage, &rect(p.x, p.y, 0.4, 0.4), 0.9, 2.4, mul(pal.foliage, 0.75));
                }
            }
        }
        // Pool and pool house on the other side.
        let pu = gw * 0.42;
        if poly_inside(c, &rect(pu, back_v + 5.0, 6.0, 3.2)) {
            pool(g, c, pu, back_v + 5.0, 10.0, 4.4);
            let ph = fit_inside(c, &rect(pu, back_v + 10.5, 3.2, 2.0));
            if poly_inside(c, &ph) {
                extrude(g, f, Glass, &ph, 0.0, 3.0, mix(pal.glass_dark, WHITE, 0.2));
                extrude(g, f, Stone, &offset_poly(&ph, -0.5), 3.0, 3.3, WHITE);
            }
        }
        // Tennis court at the back.
        let tc = fit_inside(c, &rect(pu, back_v + garden_d - 8.0, 5.5, 11.0));
        if poly_inside(c, &tc) && garden_d > 26.0 {
            ground(g, f, Paving, &tc, 0.07, [0.10, 0.28, 0.20]);
            let tcc = poly_center(&tc);
            f.bx(g, Glow, tcc.x, 0.09, tcc.y, Vec3::new(10.0, 0.02, 0.08), mul(WHITE, 0.6));
            f.bx(g, Matte, tcc.x, 0.5, tcc.y, Vec3::new(10.5, 1.0, 0.05), DARK);
        }
    }
    // Helipad for the very rich, on the lawn to one side.
    let hp = Vec2::new(-(w * 0.5 - 9.0).max(0.0), mc.y);
    if c.tier == Tier::High && poly_inside(c, &ngon(hp.x, hp.y, 6.5, 12, 0.0)) && main.iter().all(|q| q.distance(hp) > 8.0) {
        helipad(g, c, hp, 0.0, 5.5);
    }
    for _ in 0..rng.random_range(6..12) {
        if let Some(p) = c.random_inside(rng, 3.0) {
            if main.iter().all(|q| q.distance(p) > 6.0) && !point_in_polygon(p, &court) && !point_in_polygon(p, &drive) {
                tree_in(g, f, pal, p, rng.random_range(6.0..10.0), rng.random_range(2.5..4.5), pal.foliage);
            }
        }
    }
}

// ── mega hotel ───────────────────────────────────────────────────────────────

/// A hotel grown over its neighbours: a podium with a valet drop-off under
/// a long lit canopy (cars queuing, a valet stand), two or three towers
/// joined by glass sky-bridges, a pool deck on the podium roof and an
/// infinity pool on the tallest tower.
pub fn mega_hotel(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let (w, d) = (c.fw, c.fd);
    yard(g, c, 0.2, Paving, mul(pal.paving, 1.1));
    let fh = c.fh();
    let podium_h = fh * 2.0;
    let pod = fit_inside(c, &offset_poly(&c.poly, 0.5));
    if pod.len() < 3 {
        return;
    }
    let glass = pick(rng, &pal.glass);
    let trim = if c.tier == Tier::High { [0.80, 0.68, 0.40] } else { mul(WHITE, 0.8) };
    extrude(g, f, Concrete, &pod, 0.0, podium_h, mul(pal.concrete(1), 1.05));
    for face in c.faces(&pod) {
        if face.len > 3.0 {
            band_windows(g, c, rng, &face, 0.0, 2, fh);
        }
    }
    // Valet drop-off: a long canopy over the front, cars in line, the stand.
    let front = c.front_face(&pod);
    let cw = (front.len * 0.6).min(28.0);
    let reach = c.reach_out(&front);
    if room(c, &front, -cw * 0.5, cw * 0.5, reach.min(6.0) - 0.3) {
        canopy(g, c, &front, 0.0, 4.2, cw, 6.0, trim);
    }
    if reach > 3.5 && room(c, &front, front.len * 0.32 - 0.6, front.len * 0.32 + 0.6, 1.6) {
        let n = ((front.len * 0.5) / 5.5) as i32;
        for k in 0..n.max(1) {
            let t = -front.len * 0.25 + k as f32 * 5.5;
            let p = front.at(t, 2.2);
            car(g, c, rng, p, front.yaw() - f.yaw(), None);
        }
        let s = front.at(front.len * 0.32, 1.2);
        f.bx(g, Wood, s.x, 0.6, s.y, Vec3::new(0.9, 1.2, 0.6), pal.wood);
        f.bx(g, Glow, s.x, 1.35, s.y, Vec3::new(0.8, 0.3, 0.1), mul(pick(rng, &pal.sign_colors), 1.0));
    }
    entrance(g, c, rng, &front, 0.0, 4.0, 3.4);
    sign_board(g, c, rng, &front, 0.0, podium_h - 1.2, (front.len * 0.4).min(16.0), 1.6);
    // Towers on the podium.
    let towers = if w > 60.0 { 3 } else { 2 };
    let tw = (w / towers as f32 * 0.5 - 4.0).clamp(4.0, 12.0);
    let td = (d * 0.25).clamp(5.0, 12.0);
    let tv = (d * 0.12).min(d * 0.5 - td - 3.0);
    let mut tops: Vec<(Vec2, f32, Vec<Vec2>)> = Vec::new();
    for k in 0..towers {
        let tu = -w * 0.5 + (k as f32 + 0.5) * w / towers as f32;
        let floors = (c.floors as f32 * (0.75 + 0.25 * ((k + 1) % 2) as f32)).max(6.0) as i32;
        let tp = fit_inside(c, &rect(tu, tv, tw, td));
        if !poly_inside(c, &tp) {
            continue;
        }
        let h = podium_h + floors as f32 * fh;
        extrude(g, f, Glass, &tp, podium_h, h, glass);
        curtain_lights(g, c, rng, &tp, podium_h, floors, fh);
        // Trim fins at the corners and a crown.
        for p in &tp {
            f.bx(g, Metal, p.x, (podium_h + h) * 0.5, p.y, Vec3::new(0.4, h - podium_h, 0.4), trim);
        }
        extrude(g, f, Metal, &offset_poly(&tp, -0.2), h, h + 1.2, trim);
        extrude(g, f, Glow, &offset_poly(&tp, 0.3), h + 1.2, h + 1.35, mul(pick(rng, &pal.glow_colors), 0.8));
        tops.push((poly_center(&tp), h, tp));
    }
    // Sky-bridges between neighbouring towers, at two heights.
    for k in 1..tops.len() {
        let (a, ha, _) = &tops[k - 1];
        let (b, hb, _) = &tops[k];
        let span = a.distance(*b) - 2.0 * tw;
        if span <= 0.5 {
            continue;
        }
        let mid = (*a + *b) * 0.5;
        for frac in [0.45f32, 0.8] {
            let y = podium_h + (ha.min(*hb) - podium_h) * frac;
            f.bx(g, Glass, mid.x, y, mid.y, Vec3::new(span + 1.0, 3.0, 4.0), mix(glass, WHITE, 0.25));
            f.bx(g, Glow, mid.x, y - 1.55, mid.y, Vec3::new(span + 1.0, 0.1, 3.6), mul(pal.lamp, 0.7));
        }
    }
    // Pool deck on the podium roof, behind the towers.
    let deck_v = (tv + td + d * 0.5) * 0.5;
    if poly_inside(c, &rect(0.0, deck_v, 6.0, 2.5)) && d * 0.5 - (tv + td) > 7.0 {
        f.bx(g, Paving, 0.0, podium_h + 0.1, deck_v, Vec3::new(14.0, 0.2, 6.0), mul(pal.paving, 1.2));
        f.bx(g, Glow, 0.0, podium_h + 0.25, deck_v, Vec3::new(11.0, 0.1, 3.6), mul(pal.water, 0.8));
        for k in -3..=3 {
            f.bx(g, Panel, k as f32 * 1.7, podium_h + 0.45, deck_v - 2.6, Vec3::new(0.7, 0.3, 1.8), WHITE);
        }
    }
    // Infinity pool on the tallest tower.
    if let Some((p, h, tp)) = tops.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        let (lo, hi) = bounds(tp);
        let (pw, pd) = ((hi.x - lo.x) * 0.8, (hi.y - lo.y) * 0.5);
        f.bx(g, Glass, p.x, h + 1.5, p.y, Vec3::new(pw + 0.6, 0.6, pd + 0.6), mix(glass, WHITE, 0.3));
        f.bx(g, Glow, p.x, h + 1.85, p.y, Vec3::new(pw, 0.12, pd), mul(pal.water, 0.95));
    }
}

// ── museum ───────────────────────────────────────────────────────────────────

/// A gallery grown into a museum: steps up to a long colonnaded hall
/// with a pediment, a glass dome over the atrium, a new wing (glass for the
/// top tier, a converted brick warehouse below it), banners and a sculpture
/// court with lit abstract pieces.
pub fn museum(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let (w, d) = (c.fw, c.fd);
    yard(g, c, 0.2, Paving, mix(pal.paving, WHITE, 0.25));
    let stone = mix(pal.stone, CREAM, 0.55);
    let fh = 5.0;
    let hall_w = (w * 0.32).clamp(8.0, 26.0);
    let hall_d = (d * 0.25).clamp(7.0, 16.0);
    let hv = -d * 0.5 + 9.0 + hall_d;
    let hall = fit_inside(c, &rect(-w * 0.12, hv, hall_w, hall_d));
    if !poly_inside(c, &hall) {
        super::civic::library(g, c, rng);
        return;
    }
    let h = fh * 2.0;
    extrude(g, f, Stone, &hall, 0.0, h, stone);
    cornice(g, c, &hall, h, 0.8, 0.5, Stone, WHITE);
    let hc = poly_center(&hall);
    let front = c.front_face(&hall);
    // Colonnade and pediment, banners between the columns.
    let n = ((front.len - 2.0) / 2.6) as i32;
    if room(c, &front, -front.len * 0.5, front.len * 0.5, 4.4) {
        for k in 0..=n {
            let t = -front.len * 0.5 + 1.0 + k as f32 * (front.len - 2.0) / n.max(1) as f32;
            front.bx(g, f, Stone, t, h * 0.5, 2.4, Vec3::new(0.8, h, 0.8), WHITE);
        }
        front.bx(g, f, Stone, 0.0, h + 0.3, 2.4, Vec3::new(front.len, 0.6, 3.6), WHITE);
        let pp = front.at(0.0, 2.4);
        let pr = rect(pp.x, pp.y, front.len * 0.5, 1.8);
        loft(g, f, Stone, &pr, &ridge(&pr), h + 0.6, h + 3.4, WHITE);
        for k in 0..n.min(6) {
            let t = -front.len * 0.5 + 2.3 + k as f32 * (front.len - 2.0) / n.max(1) as f32;
            front.decal(g, f, Glow, t, h * 0.6, 3.0, 1.0, 3.6, mul(pick(rng, &pal.sign_colors), 0.7));
        }
    }
    let steps = rect(hc.x, front.mid().y - 6.5, front.len * 0.45, 2.4);
    if poly_inside(c, &offset_poly(&steps, -1.0)) {
        plinth(g, c, &steps, 5, 0.2, 0.6, Stone, WHITE);
    }
    // Dome over the atrium: stacked frustums of glass ribbed with metal.
    let r = (inradius(&hall) * 0.85).clamp(2.0, 9.0);
    let steps = 5;
    for k in 0..steps {
        let a0 = k as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
        let a1 = (k + 1) as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
        let (r0, r1) = (r * a0.cos(), (r * a1.cos()).max(0.3));
        let (y0, y1) = (h + 1.0 + r * a0.sin(), h + 1.0 + r * a1.sin());
        // Sides only: the sections meet ring to ring, the first stands on
        // the drum, and only the last is capped.
        let (lo, hi) = (ngon(hc.x, hc.y, r0, 16, 0.0), ngon(hc.x, hc.y, r1, 16, 0.0));
        let glass = mix(pal.glass_dark, WHITE, 0.15);
        band(g, f, Glass, &lo, &hi, y0, y1, glass);
        if k + 1 == steps {
            g.part(Glass).cap(&f.poly3(&hi, y1), glam::Vec3::Y, rgba(glass));
        }
    }
    extrude(g, f, Stone, &ngon(hc.x, hc.y, r + 0.4, 16, 0.0), h, h + 1.0, WHITE);
    f.bx(g, Glow, hc.x, h + 1.0 + r + 0.3, hc.y, Vec3::new(0.6, 0.6, 0.6), mul(pal.lamp, 1.2));
    // The new wing.
    let wu = hc.x + hall_w + (w * 0.5 - hc.x - hall_w) * 0.5;
    let wing = fit_inside(c, &rect(wu, hv + 2.0, ((w * 0.5 - hc.x - hall_w) * 0.5 - 1.0).max(3.0), hall_d * 0.9));
    if poly_inside(c, &wing) {
        if c.tier == Tier::High {
            extrude(g, f, Glass, &wing, 0.0, h * 1.3, mix(pick(rng, &pal.glass), WHITE, 0.2));
            curtain_lights(g, c, rng, &wing, 0.0, 3, h * 1.3 / 3.0);
            extrude(g, f, Metal, &offset_poly(&wing, -0.4), h * 1.3, h * 1.3 + 0.5, WHITE);
        } else {
            extrude(g, f, Brick, &wing, 0.0, h, pick(rng, &pal.brick));
            for face in c.faces(&wing) {
                if face.len > 4.0 {
                    band_windows(g, c, rng, &face, 0.0, 2, fh);
                }
            }
            let we = offset_poly(&wing, -0.3);
            loft(g, f, Metal, &we, &ridge(&we), h, h + 2.0, mul(pal.steel, 0.8));
        }
    }
    // Sculpture court out front.
    for k in 0..rng.random_range(3..6) {
        let p = Vec2::new(-w * 0.35 + k as f32 * 5.0, -d * 0.5 + 4.0);
        if c.inside_local(p) {
            let yaw = rng.random_range(0.0..std::f32::consts::PI);
            f.bx(g, Stone, p.x, 0.4, p.y, Vec3::new(1.4, 0.8, 1.4), WHITE);
            f.bx_yaw(g, Metal, p.x, 2.0, p.y, Vec3::new(0.5, 2.6, 1.6), mul(pal.steel, 1.2), yaw);
            f.bx_yaw(g, Glow, p.x, 3.4, p.y, Vec3::new(0.3, 0.3, 1.8), mul(pick(rng, &pal.glow_colors), 0.6), yaw + 0.8);
        }
    }
    for _ in 0..4 {
        if let Some(p) = c.random_inside(rng, 2.0) {
            if !point_in_polygon(p, &hall) && !point_in_polygon(p, &wing) {
                tree_in(g, f, pal, p, 5.0, 2.0, pal.foliage);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{generate_building, BuildingRules, Extras};
    use crate::palette::Palette;
    use glam::Vec2;
    use nd_sim::estate::{lots_touch, merged_lot, Form};
    use nd_sim::roads::RoadNetwork;
    use nd_sim::zoning::{generate_lots, point_in_polygon, Zone, ZoneId};
    use nd_sim::{LandUse, Lot, Tier};
    use slotmap::SlotMap;

    fn lots(lu: LandUse) -> Vec<Lot> {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
        let zone = Zone { polygon: vec![Vec2::new(-200.0, -30.0), Vec2::new(200.0, -30.0), Vec2::new(200.0, 40.0), Vec2::new(-200.0, 40.0)], land_use: lu };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        lots.values().cloned().collect()
    }

    /// Two touching plots that make one compact block.
    fn pair(lu: LandUse) -> (Lot, Lot, Lot) {
        let all = lots(lu);
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                if lots_touch(a, b) {
                    if let Some(m) = merged_lot(a, &[b]) {
                        return (a.clone(), b.clone(), m);
                    }
                }
            }
        }
        panic!("no touching pair among {} plots", all.len());
    }

    fn points_in(g: &crate::surface::Geometry, poly: &[Vec2]) -> usize {
        g.iter().map(|(_, m)| m.positions.iter().filter(|p| point_in_polygon(Vec2::new(p[0], p[2]), poly)).count()).sum()
    }

    /// A building over merged plots is one building drawn on their combined
    /// footprint: geometry stands on both plots and stays within the merged
    /// outline (an oil field without a fence between its wells, a works or
    /// a hotel across the seam, one house over two gardens).
    #[test]
    fn merged_plots_are_drawn_as_one_building() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        for (lu, extras) in [
            (LandUse::Industrial, Extras { business: Some(nd_sim::BusinessKind::OilWell), ..Extras::default() }),
            (LandUse::Industrial, Extras { form: Some(Form::Merged), ..Extras::default() }),
            (LandUse::Commercial, Extras { business: Some(nd_sim::BusinessKind::Hotel), form: Some(Form::MegaHotel), ..Extras::default() }),
            (LandUse::Residential, Extras { form: Some(Form::McMansion), ..Extras::default() }),
            (LandUse::Residential, Extras { form: Some(Form::Mansion), ..Extras::default() }),
        ] {
            let (a, b, m) = pair(lu);
            assert!(m.area() >= (a.area() + b.area()) * 0.99, "the footprint covers both plots");
            let g = generate_building(&m, lu, 3, Tier::High, 1.0, 7, &rules, &pal, &extras);
            assert!(g.triangle_count() > 0);
            assert!(points_in(&g, &a.polygon) > 20 && points_in(&g, &b.polygon) > 20, "{lu:?} {:?}: drawn on both plots", extras.form);
            let outside: usize = g
                .iter()
                .map(|(_, m2)| m2.positions.iter().filter(|p| {
                    let q = Vec2::new(p[0], p[2]);
                    !point_in_polygon(q, &m.polygon) && nd_sim::geom::distance_to_outline(q, &m.polygon) > 1.2
                }).count())
                .sum();
            assert_eq!(outside, 0, "{lu:?} {:?}: nothing beyond the merged plot", extras.form);
        }
    }
}
