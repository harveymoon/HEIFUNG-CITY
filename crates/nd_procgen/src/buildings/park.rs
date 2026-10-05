//! Open ground: scrub lots full of junk, planted gardens, paved plazas.

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

pub fn scrub_lot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = offset_poly(&c.lot_poly, 0.3);
    ground(g, f, Dirt, &poly, 0.03, mul(pal.dirt, 0.9));
    super::industrial::weeds(g, c, rng, &poly, 30);
    // Dead trees, a wreck, junk, a graffiti wall, a broken fence.
    for _ in 0..rng.random_range(1..4) {
        if let Some(p) = c.random_inside(rng, 2.0) {
            let w = f.to_world(p.x, p.y);
            let h = rng.random_range(3.0..6.0);
            g.part(Wood).cylinder(w, 0.25, 0.1, 0.0, h, 6, rgba(mul(pal.bark, 0.8)));
            for k in 0..3 {
                let a = k as f32 * 2.1 + rng.random_range(0.0..1.0);
                let dir = Vec3::new(a.cos(), 0.8, a.sin()).normalize();
                g.part(Wood).beam(Vec3::new(w.x, h * 0.7, w.y), Vec3::new(w.x, h * 0.7, w.y) + dir * rng.random_range(1.5..3.0), 0.12, rgba(mul(pal.bark, 0.8)));
            }
        }
    }
    if let Some(p) = c.random_inside(rng, 3.0) {
        let yaw = rng.random_range(0.0..3.0);
        let col = pick(rng, &pal.car_colors);
        vehicle(g, f, rng, p, yaw, Vehicle::Wreck, col);
    }
    if let Some(p) = c.random_inside(rng, 3.0) {
        scrap_pile(g, c, rng, p, 1.8, 1.2);
    }
    let front = c.front_face(&poly);
    let faces = c.faces(&poly);
    for (i, face) in faces.iter().enumerate() {
        if face.is_front {
            continue;
        }
        if i % 2 == 0 {
            // concrete wall with tags
            face.bx(g, f, Concrete, 0.0, 1.1, -0.2, Vec3::new(face.len, 2.2, 0.3), mul(pick(rng, &pal.concrete), 0.8));
            for _ in 0..rng.random_range(1..4) {
                let t = rng.random_range(-face.len * 0.4..face.len * 0.4);
                face.bx(g, f, Matte, t, rng.random_range(0.8..1.6), -0.03, Vec3::new(rng.random_range(1.0..3.0), rng.random_range(0.5..1.2), 0.03), pick(rng, &pal.awning_colors));
            }
        } else {
            fence(g, c, face.a, face.at(-face.len * 0.15, 0.0), 2.0, false);
            fence(g, c, face.at(face.len * 0.2, 0.0), face.b, 2.0, false);
        }
    }
    drum_fire(g, f, front.at(rng.random_range(-front.len * 0.3..front.len * 0.3), -2.5));
    lamp_post(g, f, front.at(-front.len * 0.4, -1.0), 5.0, mul(pal.street_lamp, 0.5));
    // mattress and a shopping trolley
    if let Some(p) = c.random_inside(rng, 2.0) {
        f.bx_yaw(g, Tarp, p.x, 0.12, p.y, Vec3::new(1.9, 0.24, 1.3), [0.55, 0.52, 0.45], rng.random_range(0.0..3.0));
    }
}

pub fn garden(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = offset_poly(&c.lot_poly, 0.3);
    ground(g, f, Foliage, &poly, 0.03, mul(pal.foliage, 0.85));
    // Path from the front gate to the centre and across.
    let front = c.front_face(&poly);
    let center = poly_center(&poly);
    let gate = front.at(0.0, 0.0);
    let path_dir = (center - gate).normalize_or_zero();
    let len = gate.distance(center) * 1.8;
    let pc = gate + path_dir * len * 0.5;
    f.bx_yaw(g, Paving, pc.x, 0.05, pc.y, Vec3::new(len, 0.06, 2.0), pal.paving, path_dir.y.atan2(path_dir.x));
    let cross = Vec2::new(-path_dir.y, path_dir.x);
    let cl = inradius(&poly) * 1.4;
    f.bx_yaw(g, Paving, center.x, 0.05, center.y, Vec3::new(cl, 0.06, 2.0), pal.paving, cross.y.atan2(cross.x));
    // Trees, benches, lamps, hedges on the edges, a pond and a playground.
    for _ in 0..rng.random_range(5..11) {
        if let Some(p) = c.random_inside(rng, 2.0) {
            if (p - gate).dot(cross).abs() > 2.0 || (p - gate).dot(path_dir) < 0.0 {
                tree(g, f, p, rng.random_range(3.0..6.5), rng.random_range(3.0..5.5), pal.bark, mul(pal.foliage, rng.random_range(0.8..1.2)));
            }
        }
    }
    for k in 0..4 {
        let a = k as f32 / 4.0 * std::f32::consts::TAU + 0.4;
        let p = center + Vec2::new(a.cos(), a.sin()) * 3.0;
        if c.inside(p) {
            bench_world(g, f.to_world(p.x, p.y), f.yaw() + a + std::f32::consts::FRAC_PI_2, pal.wood, CHARCOAL);
        }
        let lp = center + Vec2::new(a.cos(), a.sin()) * (inradius(&poly) * 0.6);
        if c.inside(lp) {
            lamp_post(g, f, lp, 3.5, pal.lamp);
        }
    }
    for face in c.faces(&poly) {
        if face.is_front {
            for s in [-1.0f32, 1.0] {
                if face.len > 6.0 {
                    hedge(g, c, face.at(s * 1.6, 0.0), face.at(s * face.len * 0.5, 0.0), 1.2, 0.8, pal.foliage);
                }
            }
        } else {
            hedge(g, c, face.a, face.b, 1.4, 0.8, pal.foliage);
        }
    }
    let pond = center + cross * (inradius(&poly) * 0.45);
    if c.inside(pond) && pond.distance(center) > 4.0 {
        pool(g, c, pond.x, pond.y, 5.0, 3.5);
    }
    let play = center - cross * (inradius(&poly) * 0.45);
    if c.inside(play) && play.distance(center) > 4.0 {
        let col = pick(rng, &pal.awning_colors);
        f.bx(g, Matte, play.x, 1.4, play.y, Vec3::new(0.12, 2.8, 0.12), col);
        f.bx(g, Matte, play.x + 2.4, 1.4, play.y, Vec3::new(0.12, 2.8, 0.12), col);
        f.bx(g, Matte, play.x + 1.2, 2.8, play.y, Vec3::new(2.6, 0.12, 0.12), col);
        for k in 0..2 {
            f.bx(g, Matte, play.x + 0.8 + k as f32 * 0.8, 0.5, play.y, Vec3::new(0.4, 0.08, 0.3), DARK);
            f.bx(g, Matte, play.x + 0.8 + k as f32 * 0.8, 1.6, play.y, Vec3::new(0.03, 2.3, 0.03), DARK);
        }
        f.bx_yaw(g, Metal, play.x + 1.0, 0.9, play.y + 3.0, Vec3::new(3.0, 0.2, 0.8), col, 0.0);
    }
    f.bx(g, Metal, gate.x, 0.6, gate.y - 0.5, Vec3::new(0.1, 1.2, 0.1), CHARCOAL);
}

pub fn plaza(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = offset_poly(&c.lot_poly, 0.3);
    ground(g, f, Paving, &poly, 0.04, mul(pal.paving, 1.15));
    let center = poly_center(&poly);
    let r = inradius(&poly);
    // Fountain with jets, a sculpture, planters with hedges and trees in a grid.
    pool(g, c, center.x, center.y, (r * 0.5).min(8.0), (r * 0.5).min(8.0));
    for k in 0..5 {
        let a = k as f32 / 5.0 * std::f32::consts::TAU;
        let p = center + Vec2::new(a.cos(), a.sin()) * (r * 0.18).min(2.8);
        f.bx(g, Glow, p.x, 1.0, p.y, Vec3::new(0.2, 1.6, 0.2), mul(pal.water, 1.6));
    }
    let sc = center + Vec2::new(r * 0.55, 0.0);
    if c.inside(sc) {
        let base = ngon(sc.x, sc.y, 1.2, 4, 0.4);
        extrude(g, f, Stone, &base, 0.0, 1.0, mul(pal.stone, 1.6));
        loft(g, f, Panel, &scale_poly(&base, 0.6), &shift_poly(&scale_poly(&base, 0.2), Vec2::new(1.5, 0.8)), 1.0, 7.0, mul(pal.steel, 1.1));
        f.bx(g, Glow, sc.x, 0.06, sc.y - 1.8, Vec3::new(1.2, 0.06, 0.4), mul(pal.lamp, 0.9));
    }
    let (lo, hi) = bounds(&offset_poly(&poly, 2.0));
    let mut v = lo.y + 1.5;
    while v < hi.y {
        let mut u = lo.x + 1.5;
        while u < hi.x {
            let p = Vec2::new(u, v);
            if c.inside(p) && p.distance(center) > (r * 0.5).min(8.0) * 0.8 + 2.5 && p.distance(sc) > 3.0 {
                f.bx(g, Concrete, p.x, 0.3, p.y, Vec3::new(2.2, 0.6, 2.2), mul(pick(rng, &pal.concrete), 1.1));
                tree(g, f, p, 3.0, 3.2, pal.bark, pal.foliage);
                f.bx(g, Glow, p.x + 0.9, 0.62, p.y, Vec3::new(0.3, 0.05, 0.3), mul(pal.lamp, 0.7));
                if rng.random_bool(0.5) {
                    bench_world(g, f.to_world(p.x, p.y + 1.6), f.yaw(), pal.wood, CHARCOAL);
                }
            }
            u += 7.0;
        }
        v += 7.0;
    }
    let front = c.front_face(&poly);
    for k in 0..3 {
        lamp_post(g, f, front.at(-front.len * 0.35 + k as f32 * front.len * 0.35, -1.5), 6.0, pal.lamp);
    }
    // Kiosk: glass box with a lit interior.
    let kp = front.at(front.len * 0.38, -3.5);
    if c.inside(kp) {
        let kb = rect(kp.x, kp.y, 1.5, 1.2);
        extrude(g, f, Glass, &kb, 0.0, 2.8, pick(rng, &pal.glass));
        extrude(g, f, Glow, &scale_poly(&kb, 0.8), 0.9, 2.2, mul(pal.window_warm, 0.6));
        extrude(g, f, Panel, &offset_poly(&kb, -0.2), 2.8, 3.1, mul(pal.steel, 0.9));
    }
    bollards(g, c, &front, 6, -0.8);
}
