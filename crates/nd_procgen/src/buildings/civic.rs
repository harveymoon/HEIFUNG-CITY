//! Civic buildings: bunker-like precincts, brutalist ministries, libraries,
//! courthouses and monuments.

use super::parts::*;
use super::Ctx;
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::seq::IndexedRandom;
use rand::Rng;

pub fn precinct(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = 2;
    let h = floors as f32 * fh;
    let poly = &c.poly;
    let concrete = mul(pick(rng, &pal.concrete), 0.8);
    extrude(g, f, Concrete, poly, 0.0, h, concrete);
    cornice(g, c, poly, h - 0.4, 0.8, 0.5, Concrete, mul(concrete, 1.1));
    parapet(g, c, poly, h, 1.2, 0.4, Concrete, concrete);
    let mut spec = WindowSpec::from_ctx(c);
    spec.w = 0.6;
    spec.h = 1.1;
    spec.spacing = 2.2;
    spec.sill = fh * 0.5;
    spec.boarded = 0.0;
    spec.ac = 0.1;
    spec.frame = CHARCOAL;
    let front = c.front_face(poly);
    for face in c.faces(poly) {
        if face.len > 3.0 {
            windows(g, c, rng, &face, 0.0, floors, fh, &spec);
            grime(g, c, rng, &face, 0.0, h, 1.0);
        }
    }
    // Sally port, barriers, blue lamps and a cold sign.
    roller_door(g, c, &front, front.len * 0.3, 3.6, 3.4, mul(pal.steel, 0.55));
    entrance(g, c, rng, &front, -front.len * 0.2, 2.0, 2.8);
    front.bx(g, f, Glow, -front.len * 0.2 - 1.6, 3.4, 0.3, Vec3::new(0.3, 0.3, 0.3), pal.police_light);
    front.bx(g, f, Glow, -front.len * 0.2 + 1.6, 3.4, 0.3, Vec3::new(0.3, 0.3, 0.3), pal.police_light);
    for k in 0..((front.len / 4.0) as i32).max(1) {
        let t = -front.len * 0.45 + k as f32 * 4.0 + 1.5;
        barrier(g, c, &front, t, 3.5, 3.0);
    }
    front.bx(g, f, Matte, 0.0, h + 0.4, 0.2, Vec3::new((front.len * 0.5).min(10.0), 1.0, 0.15), DARK);
    front.decal(g, f, Glow, 0.0, h + 0.4, 0.28, (front.len * 0.45).min(9.0), 0.6, mul([0.5, 0.9, 2.2], 0.7));
    // Mast, cameras, floodlights, yard with vans behind a barbed fence.
    antenna_mast(g, c, poly_center(poly) + Vec2::new(c.fw * 0.25, 0.0), h + 1.2, 14.0);
    for p in &c.poly {
        let inset = *p + (poly_center(poly) - *p).normalize_or_zero() * 0.6;
        f.bx(g, Matte, inset.x, h + 1.0, inset.y, Vec3::new(0.08, 0.6, 0.08), CHARCOAL);
        f.bx(g, Matte, inset.x, h + 1.3, inset.y, Vec3::new(0.25, 0.15, 0.4), DARK);
    }
    let yard = offset_poly(&c.lot_poly, 0.4);
    fence_lot(g, c, &yard, 2.6, true);
    for k in 0..3 {
        let p = Vec2::new(-c.fw * 0.35 + k as f32 * 3.0, c.fd * 0.5 + 3.0);
        if vehicle_fits(c, p, std::f32::consts::FRAC_PI_2, Vehicle::Van) && !nd_sim::zoning::point_in_polygon(p, poly) {
            vehicle(g, f, pal, rng, p, 0.0, std::f32::consts::FRAC_PI_2, Vehicle::Van, [0.08, 0.1, 0.16]);
        }
    }
    floodlight(g, c, front.at(front.len * 0.45, -1.5), 8.0);
    floodlight(g, c, front.at(-front.len * 0.45, -1.5), 8.0);
}

pub fn ministry(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    super::residential::brutalist_block(g, c, rng, true, false);
    let f = &c.frame;
    let pal = c.palette;
    let front = c.front_face(&c.poly);
    // Flagpoles, an inscription band and a forecourt.
    for k in 0..3 {
        let p = front.at((k as f32 - 1.0) * 3.0, -3.0);
        if c.inside_local(p) {
            f.bx(g, Matte, p.x, 6.0, p.y, Vec3::new(0.12, 12.0, 0.12), pal.steel);
            f.bx(g, Glow, p.x, 0.3, p.y, Vec3::new(0.4, 0.1, 0.4), mul(pal.lamp, 0.8));
        }
    }
    front.decal(g, f, Glow, 0.0, c.fh() * 1.6 + 1.0, 0.3, (front.len * 0.6).min(16.0), 0.5, mul([1.2, 1.2, 1.15], 0.6));
    let fore = [front.at(-front.len * 0.5, 0.0), front.at(front.len * 0.5, 0.0), front.at(front.len * 0.5, -3.5), front.at(-front.len * 0.5, -3.5)];
    ground(g, f, Paving, &fore, 0.06, mul(pal.paving, 1.15));
    for k in 0..2 {
        let p = front.at((k as f32 - 0.5) * front.len * 0.6, -2.5);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
            bench_world(g, f.to_world(p.x, p.y), f.yaw(), pal.wood, CHARCOAL);
        }
    }
}

pub fn library(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let base_h = fh * 2.0;
    let stone = mul(pal.stone, 1.5);
    extrude(g, f, Stone, poly, 0.0, base_h, stone);
    plinth(g, c, poly, 3, 0.18, 0.5, Stone, mul(stone, 1.1));
    let mut spec = WindowSpec::from_ctx(c);
    spec.w = 1.2;
    spec.h = fh * 1.6;
    spec.spacing = 3.4;
    spec.sill = 0.4;
    spec.boarded = 0.0;
    spec.ac = 0.0;
    spec.lit = c.lit_chance() * 1.2;
    for face in c.faces(poly) {
        if face.len > 4.0 {
            windows(g, c, rng, &face, 0.0, 1, base_h, &spec);
        }
    }
    // Reading room: a glass box cantilevered off the base, on slim columns.
    let upper = fit_inside(c, &shift_poly(&scale_poly(poly, 0.8), Vec2::new(c.fw * 0.12, -c.fd * 0.1)));
    let upper_h = fh * 1.5;
    extrude(g, f, Concrete, &upper, base_h, base_h + 0.5, mul(pick(rng, &pal.concrete), 1.1));
    extrude(g, f, Glass, &offset_poly(&upper, 0.3), base_h + 0.5, base_h + 0.5 + upper_h, pick(rng, &pal.glass));
    curtain_lights(g, c, rng, &offset_poly(&upper, 0.3), base_h + 0.5, 1, upper_h);
    extrude(g, f, Concrete, &upper, base_h + 0.5 + upper_h, base_h + 1.0 + upper_h, mul(pick(rng, &pal.concrete), 1.1));
    for p in &upper {
        let inset = *p + (poly_center(&upper) - *p).normalize_or_zero() * 0.8;
        if !nd_sim::zoning::point_in_polygon(inset, poly) {
            f.bx(g, Matte, inset.x, base_h * 0.5, inset.y, Vec3::new(0.4, base_h, 0.4), pal.steel);
        }
    }
    // Roof garden on the base around the reading room.
    let roof = offset_poly(poly, 1.0);
    for face in c.faces(&roof) {
        if face.len > 6.0 {
            let p = face.at(0.0, -1.0);
            if !nd_sim::zoning::point_in_polygon(p, &offset_poly(&upper, -1.0)) {
                face.bx(g, f, Foliage, 0.0, base_h + 0.4, -1.0, Vec3::new(face.len * 0.7, 0.8, 1.2), pal.foliage);
            }
        }
    }
    parapet(g, c, poly, base_h, 0.6, 0.3, Stone, stone);
    let front = c.front_face(poly);
    entrance(g, c, rng, &front, 0.0, 3.5, base_h * 0.7);
    canopy(g, c, &front, 0.0, base_h * 0.72, 7.0, 3.0, mul(pal.steel, 0.9));
    sign_board(g, c, rng, &front, -front.len * 0.3, base_h - 1.0, (front.len * 0.3).min(8.0), 0.8);
    for k in 0..2 {
        let p = front.at((k as f32 - 0.5) * front.len * 0.7, -2.5);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.9 {
            tree(g, f, p, 4.0, 3.5, pal.bark, pal.foliage);
            bench_world(g, f.to_world(p.x + 2.0, p.y), f.yaw(), pal.wood, CHARCOAL);
        }
    }
    roof_gear(g, c, rng, &upper, base_h + 1.0 + upper_h, 0.4);
}

pub fn courthouse(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).clamp(3, 5);
    let h = floors as f32 * fh;
    let poly = &c.poly;
    let stone = mul(pal.stone, 1.7);
    plinth(g, c, poly, 5, 0.2, 0.6, Stone, mul(stone, 1.05));
    let body = offset_poly(poly, 2.5);
    extrude(g, f, Stone, &body, 1.0, 1.0 + h, stone);
    cornice(g, c, &body, 1.0 + h - 0.6, 1.2, 0.8, Stone, mul(stone, 1.1));
    let mut spec = WindowSpec::from_ctx(c);
    spec.w = 1.4;
    spec.h = fh * 0.7;
    spec.spacing = 3.4;
    spec.boarded = 0.0;
    spec.ac = 0.0;
    spec.frame = mul(pal.steel, 0.8);
    for face in c.faces(&body) {
        if face.len > 4.0 && !face.is_front {
            windows(g, c, rng, &face, 1.0, floors, fh, &spec);
        }
    }
    // Colonnade across the front, pediment above.
    let front = c.front_face(&body);
    // The porch is as deep as the forecourt allows.
    let porch = (3.8f32).min(c.reach_out(&front) - 0.4).max(1.0);
    let cols = ((front.len / 3.2) as i32).clamp(3, 9);
    for k in 0..cols {
        let t = -front.len * 0.5 + 1.6 + k as f32 * (front.len - 3.2) / (cols - 1).max(1) as f32;
        let p = front.at(t, (porch - 2.0).max(0.8));
        if !c.inside_local(p) {
            continue;
        }
        let w = f.to_world(p.x, p.y);
        g.part(Stone).cylinder(w, 0.55, 0.5, 1.0, 1.0 + h - 1.2, 10, rgba(stone));
        g.part(Stone).boxed(Vec3::new(w.x, 1.0 + h - 1.0, w.y), Vec3::new(1.3, 0.5, 1.3), f.yaw(), rgba(mul(stone, 1.1)));
    }
    front.bx(g, f, Stone, 0.0, 1.0 + h - 0.3, porch * 0.5 - 0.1, Vec3::new(front.len, 1.2, porch), mul(stone, 1.1));
    front.bx(g, f, Stone, 0.0, 1.0 + h + 1.2, porch * 0.5 - 0.1, Vec3::new(front.len * 0.7, 1.8, porch - 0.4), mul(stone, 1.05));
    front.decal(g, f, Glow, 0.0, 1.0 + h + 1.2, porch - 0.28, front.len * 0.5, 0.6, mul([1.2, 1.2, 1.1], 0.5));
    windows(g, c, rng, &front, 1.0, floors, fh, &spec);
    entrance(g, c, rng, &front, 0.0, 4.0, fh * 1.4);
    // Dome.
    let dc = poly_center(&body);
    let dr = (inradius(&body) * 0.45).clamp(3.0, 9.0);
    let dw = f.to_world(dc.x, dc.y);
    g.part(Stone).cylinder(dw, dr, dr, 1.0 + h, 1.0 + h + 2.5, 16, rgba(stone));
    g.part(Glass).frustum(&MeshData::ngon(dw, dr, 16, 0.0, 1.0 + h + 2.5), &MeshData::ngon(dw, dr * 0.6, 16, 0.0, 1.0 + h + 2.5 + dr * 0.6), rgba(pick(rng, &pal.glass)));
    g.part(Glass).frustum(&MeshData::ngon(dw, dr * 0.6, 16, 0.0, 1.0 + h + 2.5 + dr * 0.6), &MeshData::ngon(dw, dr * 0.15, 16, 0.0, 1.0 + h + 2.5 + dr * 0.95), rgba(pick(rng, &pal.glass)));
    g.part(Glow).frustum(&MeshData::ngon(dw, dr * 0.99, 16, 0.0, 1.0 + h + 2.6), &MeshData::ngon(dw, dr * 0.9, 16, 0.0, 1.0 + h + 3.2), rgba(mul(pal.window_warm, 0.6)));
    g.part(Matte).cylinder(dw, 0.3, 0.1, 1.0 + h + 2.5 + dr * 0.95, 1.0 + h + 2.5 + dr * 0.95 + 3.0, 8, rgba(pal.steel));
    g.part(Glow).boxed(Vec3::new(dw.x, 1.0 + h + 2.5 + dr * 0.95 + 3.2, dw.y), Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
    // Forecourt: lamps, hedges, flag masts, a statue.
    let pf = c.front_face(poly);
    for k in 0..4 {
        let t = -pf.len * 0.45 + k as f32 * pf.len * 0.3;
        lamp_post_in(g, c, pf.at(t, -1.0), 4.0, pal.lamp);
    }
    for s in [-1.0f32, 1.0] {
        let p = pf.at(s * pf.len * 0.3, -3.0);
        if c.inside_local(p) {
            f.bx(g, Matte, p.x, 7.0, p.y, Vec3::new(0.12, 14.0, 0.12), pal.steel);
        }
    }
    let sp = pf.at(0.0, -3.2);
    if c.inside_local(sp) {
        f.bx(g, Stone, sp.x, 0.8, sp.y, Vec3::new(1.6, 1.6, 1.6), mul(stone, 0.8));
        f.bx(g, Matte, sp.x, 2.8, sp.y, Vec3::new(0.7, 2.4, 0.7), mul(pal.steel, 0.3));
        f.bx(g, Glow, sp.x, 0.05, sp.y - 1.4, Vec3::new(0.8, 0.06, 0.3), mul(pal.lamp, 0.9));
    }
}

pub fn monument(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let plaza = yard(g, c, 0.5, Paving, mul(pal.paving, 1.2));
    let center = poly_center(&c.poly);
    let r = (inradius(&c.poly) * 0.28).clamp(1.5, 6.0);
    let h = rng.random_range(35.0..70.0);
    let sides = *[3usize, 4, 5, 6].choose(rng).unwrap_or(&4);
    let stone = mul(pal.stone, 1.6);
    // Stepped base, tapered spire, glow rings.
    extrude(g, f, Stone, &ngon(center.x, center.y, r * 2.2, sides * 2, 0.3), 0.0, 1.2, stone);
    extrude(g, f, Stone, &ngon(center.x, center.y, r * 1.6, sides * 2, 0.3), 1.2, 2.4, stone);
    let base = ngon(center.x, center.y, r, sides, 0.0);
    loft(g, f, Panel, &base, &scale_poly(&base, 0.08), 2.4, 2.4 + h, mul(pick(rng, &pal.panel), 1.1));
    let rings = (h / 9.0) as i32;
    for k in 1..rings {
        let t = k as f32 / rings as f32;
        let rr = r * (1.0 - 0.92 * t) + 0.08;
        extrude(g, f, Glow, &ngon(center.x, center.y, rr, sides, 0.0), 2.4 + t * h, 2.4 + t * h + 0.4, mul(pal.window_cold, 0.8));
    }
    g.part(Glow).boxed(f.at2(center, 2.4 + h + 0.4), Vec3::splat(0.6), 0.0, rgba(pal.aviation_light));
    // Reflecting pool in front, pylons, trees in a grid, benches, uplights.
    let front = c.front_face(&c.poly);
    let pp = front.at(0.0, -(inradius(&c.poly) * 0.5));
    let pw = (front.len * 0.5).min(16.0);
    if c.inside_local(pp + Vec2::new(pw * 0.5, 2.0)) && c.inside_local(pp - Vec2::new(pw * 0.5, 2.0)) && pp.distance(center) > r * 2.5 + 3.0 {
        pool(g, c, pp.x, pp.y, pw, 3.0);
    }
    for k in 0..sides {
        let a = k as f32 / sides as f32 * std::f32::consts::TAU;
        let p = center + Vec2::new(a.cos(), a.sin()) * (r * 3.2);
        if c.inside_local(p) {
            f.bx(g, Stone, p.x, 2.5, p.y, Vec3::new(0.8, 5.0, 0.8), stone);
            f.bx(g, Glow, p.x, 0.1, p.y, Vec3::new(1.0, 0.1, 1.0), mul(pal.lamp, 0.8));
        }
    }
    let (lo, hi) = bounds(&offset_poly(&plaza, 2.0));
    let mut v = lo.y + 2.0;
    while v < hi.y {
        let mut u = lo.x + 2.0;
        while u < hi.x {
            let p = Vec2::new(u, v);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 && p.distance(center) > r * 3.6 + 2.0 && (pp.distance(p) > pw * 0.6 + 2.5) {
                tree(g, f, p, 3.5, 3.0, pal.bark, pal.foliage);
                if rng.random_bool(0.4) {
                    bench_world(g, f.to_world(p.x + 1.5, p.y), f.yaw(), pal.wood, CHARCOAL);
                }
            }
            u += 8.0;
        }
        v += 8.0;
    }
    for k in 0..3 {
        lamp_post_in(g, c, front.at(-front.len * 0.35 + k as f32 * front.len * 0.35, -1.5), 4.5, pal.lamp);
    }
}
