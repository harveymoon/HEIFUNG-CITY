//! Nightlife: shack bars strung with lights, windowless clubs behind giant
//! screens, cinemas with marquees, casinos and arenas.

use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

pub fn shack_bar(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.4, Dirt, pal.dirt);
    let front = c.front_face(&yard);
    // Main shack along the front, two or three smaller ones behind.
    let n = rng.random_range(2..=4);
    let mut shacks: Vec<(Vec2, f32, f32, f32)> = Vec::new(); // (centre, w, d, h)
    for i in 0..n {
        let (w, d, h) = if i == 0 { (rng.random_range(7.0..11.0), rng.random_range(5.0..7.0), rng.random_range(3.4..4.4)) } else { (rng.random_range(3.5..6.0), rng.random_range(3.0..5.0), rng.random_range(2.6..3.6)) };
        let p = if i == 0 { front.at(0.0, -d * 0.5 - 2.5) } else { match c.random_inside(rng, 3.5) { Some(p) => p, None => continue } };
        let (mut w, mut d) = (w, d);
        let mut fits = false;
        for _ in 0..8 {
            if rect(p.x, p.y, w * 0.5 + 0.8, d * 0.5 + 0.9).iter().all(|q| c.inside_local(*q)) {
                fits = true;
                break;
            }
            w *= 0.85;
            d *= 0.85;
        }
        if !fits || w < 2.5 {
            continue;
        }
        if shacks.iter().any(|(q, qw, qd, _)| (q.x - p.x).abs() < (qw + w) * 0.5 + 0.8 && (q.y - p.y).abs() < (qd + d) * 0.5 + 0.8) {
            continue;
        }
        let yaw = rng.random_range(-0.2..0.2);
        let col = mul(pick(rng, &pal.metal_paint), 1.1);
        f.bx_yaw(g, Metal, p.x, h * 0.5, p.y, Vec3::new(w, h, d), col, yaw);
        // tarp roof overhanging, plywood patches, a door and a window
        f.bx_yaw(g, Tarp, p.x, h + 0.1, p.y, Vec3::new(w + 1.4, 0.12, d + 1.6), pick(rng, &pal.tarp_colors), yaw);
        for _ in 0..rng.random_range(1..4) {
            let side = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
            f.bx_yaw(g, Wood, p.x + rng.random_range(-w * 0.3..w * 0.3), rng.random_range(0.8..h - 0.6), p.y + side * (d * 0.5 + 0.03), Vec3::new(rng.random_range(0.6..1.6), rng.random_range(0.6..1.4), 0.05), pal.plywood, yaw);
        }
        // The shack's road side, facing the road (the shack's own yaw is small
        // enough that the door and signs read as flush).
        let face = super::Face::new(p + Vec2::new(-w * 0.5, -d * 0.5), p + Vec2::new(w * 0.5, -d * 0.5), Vec2::new(0.0, -1.0));
        face.decal(g, f, Matte, w * 0.3, 1.1, 0.04, 1.0, 2.2, DARK);
        face.decal(g, f, Glow, -w * 0.2, 1.8, 0.05, 1.6, 0.9, mul(pick(rng, &pal.glow_colors), 0.9));
        // neon tube outline on the main shack
        if i == 0 {
            let nc = pick(rng, &pal.sign_colors);
            face.decal(g, f, Glow, 0.0, h - 0.15, 0.08, w * 0.96, 0.06, nc);
            face.decal(g, f, Glow, -w * 0.48, h * 0.5, 0.08, 0.06, h * 0.9, nc);
            face.decal(g, f, Glow, w * 0.48, h * 0.5, 0.08, 0.06, h * 0.9, nc);
            sign_board(g, c, rng, &face, 0.0, h + 1.0, w * 0.8, 0.9);
            if rng.random_bool(0.7) {
                vertical_sign(g, c, rng, &face, w * 0.5 + 0.6, 0.8, h + 1.2);
            }
            // speaker stacks by the door
            for s in [-1.0f32, 1.0] {
                face.bx(g, f, Matte, s * w * 0.42, 0.6, 0.7, Vec3::new(0.7, 1.2, 0.6), DARK);
            }
        } else if rng.random_bool(0.6) {
            sign_board(g, c, rng, &face, 0.0, h + 0.6, w * 0.7, 0.6);
        }
        dish(g, f, p + Vec2::new(w * 0.3, 0.0), h + 0.1, 0.6, pal.steel);
        shacks.push((p, w, d, h));
    }
    // Poles with string lights across the yard, a dance floor and a drum fire.
    let poles: Vec<Vec2> = (0..4).filter_map(|_| c.random_inside(rng, 1.5)).collect();
    for p in &poles {
        f.bx(g, Wood, p.x, 2.5, p.y, Vec3::new(0.16, 5.0, 0.16), pal.bark);
    }
    for w in poles.windows(2) {
        string_lights(g, f, Vec3::new(w[0].x, 4.8, w[0].y), Vec3::new(w[1].x, 4.8, w[1].y), 10, mul(pick(rng, &pal.glow_colors), 1.1));
    }
    if let (Some(a), Some((b, w, _, h))) = (poles.first(), shacks.first()) {
        string_lights(g, f, Vec3::new(a.x, 4.8, a.y), Vec3::new(b.x - w * 0.5, h + 0.1, b.y), 8, mul(pal.window_warm, 1.2));
    }
    if let Some(p) = c.random_inside(rng, 4.0) {
        if shacks.iter().all(|(q, w, d, _)| (q.x - p.x).abs() > w * 0.5 + 2.0 || (q.y - p.y).abs() > d * 0.5 + 2.0) {
            ground(g, f, Glow, &ngon(p.x, p.y, 3.0, 8, 0.0), 0.05, mul(pick(rng, &pal.led_colors), 0.25));
        }
    }
    drum_fire(g, f, pal, front.at(front.len * 0.3, -1.5));
    for _ in 0..2 {
        if let Some(p) = c.random_inside(rng, 2.0) {
            if shacks.iter().all(|(q, w, d, _)| (q.x - p.x).abs() > w * 0.5 + 2.5 || (q.y - p.y).abs() > d * 0.5 + 2.5) {
                let yaw = rng.random_range(0.0..3.0);
                let kind = if rng.random_bool(0.5) { Vehicle::Wreck } else { Vehicle::Car };
                let col = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, p, 0.0, yaw, kind, col);
            }
        }
    }
    stall(g, c, rng, &front, -front.len * 0.35, -2.0);
    // Tyres and crates against the fence.
    let faces = c.faces(&yard);
    for face in faces.iter().filter(|x| !x.is_front) {
        fence(g, c, face.a, face.b, 2.0, false);
    }
}

pub fn club(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let h = (c.floors as f32 * c.fh()).clamp(8.0, 14.0);
    let poly = &c.poly;
    let dark = mul(pick(rng, &pal.concrete), 0.55);
    extrude(g, f, Concrete, poly, 0.0, h, dark);
    let front = c.front_face(poly);
    // Giant screen covering most of the front, neon edge strips on every face.
    led_screen(g, c, rng, &front, 0.0, h * 0.55 + 1.5, (front.len * 0.8).min(22.0), h * 0.6);
    let nc = pick(rng, &pal.led_colors);
    for face in c.faces(poly) {
        face.decal(g, f, Glow, 0.0, h - 0.3, 0.05, face.len * 0.98, 0.12, nc);
        face.decal(g, f, Glow, 0.0, 0.4, 0.05, face.len * 0.98, 0.1, nc);
        if !face.is_front {
            grime(g, c, rng, &face, 0.0, h, 1.5);
        }
    }
    // Entrance with queue barriers, bouncer canopy and a vertical sign.
    entrance(g, c, rng, &front, -front.len * 0.25, 3.0, 3.2);
    for k in 0..5 {
        let t = -front.len * 0.25 + 2.5 + k as f32 * 1.4;
        front.bx(g, f, Matte, t, 0.5, 1.6, Vec3::new(0.08, 1.0, 0.08), pal.steel);
    }
    front.bx(g, f, Matte, -front.len * 0.25 + 5.3, 1.0, 1.6, Vec3::new(5.6, 0.05, 0.05), mul(pal.warning_light, 0.3));
    vertical_sign(g, c, rng, &front, front.len * 0.45, 2.0, h - 3.0);
    // Rooftop bar with string lights, speakers, and search lights.
    parapet(g, c, poly, h, 1.1, 0.3, Concrete, dark);
    let roof = offset_poly(poly, 2.0);
    if roof.len() >= 3 {
        let faces = c.faces(&roof);
        for w in faces.windows(2) {
            string_lights(g, f, Vec3::new(w[0].a.x, h + 3.0, w[0].a.y), Vec3::new(w[1].a.x, h + 3.0, w[1].a.y), 8, mul(pal.window_warm, 1.2));
            f.bx(g, Matte, w[0].a.x, h + 1.5, w[0].a.y, Vec3::new(0.1, 3.0, 0.1), DARK);
        }
        let center = poly_center(&roof);
        f.bx(g, Metal, center.x, h + 0.6, center.y, Vec3::new(4.0, 1.2, 1.5), DARK);
        for k in 0..3 {
            let p = center + Vec2::new((k as f32 - 1.0) * 3.0, 0.0);
            if nd_sim::zoning::point_in_polygon(p, &roof) {
                f.bx(g, Matte, p.x, h + 0.65, p.y, Vec3::new(0.6, 1.3, 0.6), CHARCOAL);
                f.bx(g, Glow, p.x, h + 1.45, p.y, Vec3::new(0.5, 0.3, 0.5), mul(pick(rng, &pal.led_colors), 0.5));
            }
        }
    }
    roof_gear(g, c, rng, poly, h, 0.5);
    bollards(g, c, &front, 5, 2.2);
}

pub fn cinema(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = &c.poly;
    let lobby_h = c.fh() * 2.0;
    let hall_h = (c.floors as f32 * c.fh()).clamp(lobby_h + 4.0, 20.0);
    let col = mul(pick(rng, &pal.plaster), 1.0 - 0.25 * c.wear);
    extrude(g, f, Plaster, poly, 0.0, lobby_h, col);
    // Auditorium block set back from the front.
    let (lo, hi) = bounds(poly);
    let hall = fit_inside(c, &shift_poly(&scale_poly(poly, 0.78), Vec2::new(0.0, (hi.y - lo.y) * 0.12)));
    extrude(g, f, Panel, &hall, lobby_h, hall_h, mul(pick(rng, &pal.panel), 0.8));
    roof_gear(g, c, rng, &hall, hall_h, 0.6);
    let front = c.front_face(poly);
    // Marquee: deep canopy with a lit underside, a title board and poster frames.
    front.bx(g, f, Panel, 0.0, lobby_h * 0.62, 2.0, Vec3::new(front.len * 0.9, 0.5, 4.0), mul(c.style.trim_color, 0.9));
    front.bx(g, f, Glow, 0.0, lobby_h * 0.62 - 0.26, 2.0, Vec3::new(front.len * 0.85, 0.04, 3.6), mul(pal.window_warm, 0.9));
    front.bx(g, f, Matte, 0.0, lobby_h * 0.62 + 1.1, 3.9, Vec3::new(front.len * 0.86, 1.8, 0.2), DARK);
    front.decal(g, f, Glow, 0.0, lobby_h * 0.62 + 1.1, 4.02, front.len * 0.8, 1.4, mul([1.3, 1.3, 1.25], 0.9));
    for k in 0..4 {
        let t = -front.len * 0.4 + (k as f32 + 0.5) * front.len * 0.2;
        front.decal(g, f, Glow, t, lobby_h * 0.62 + 1.1, 4.04, front.len * 0.11, 0.9, mul(pick(rng, &pal.led_colors), 0.6));
    }
    let posters = ((front.len / 4.0) as i32).clamp(2, 6);
    for k in 0..posters {
        let t = -front.len * 0.45 + (k as f32 + 0.5) * front.len * 0.9 / posters as f32;
        if t.abs() < 2.5 {
            continue;
        }
        led_screen(g, c, rng, &front, t, 2.1, 1.6, 2.2);
    }
    entrance(g, c, rng, &front, 0.0, 4.0, 3.2);
    vertical_sign(g, c, rng, &front, front.len * 0.46, lobby_h + 0.5, hall_h - lobby_h - 1.0);
    for face in c.faces(poly) {
        if !face.is_front && face.len > 5.0 {
            band_windows(g, c, rng, &face, 0.0, 2, c.fh());
            grime(g, c, rng, &face, 0.0, lobby_h, 1.0);
        }
    }
    // Exits and fire stair on the hall.
    for face in c.faces(&hall) {
        if !face.is_front && face.len > 8.0 {
            fire_escape(g, c, rng, &face, 0.0, lobby_h, ((hall_h - lobby_h) / c.fh()) as i32, c.fh());
            break;
        }
    }
    bollards(g, c, &front, 5, 5.0);
}

pub fn casino(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let podium_h = fh * 2.5;
    let dark = mul(pick(rng, &pal.panel), 0.5);
    extrude(g, f, Panel, poly, 0.0, podium_h, dark);
    // LED strips wrap the podium at three heights.
    for face in c.faces(poly) {
        for k in 0..3 {
            face.decal(g, f, Glow, 0.0, podium_h * (0.2 + 0.3 * k as f32), 0.05, face.len * 0.98, 0.25, mul(pick(rng, &pal.led_colors), 0.7));
        }
    }
    let front = c.front_face(poly);
    led_screen(g, c, rng, &front, -front.len * 0.22, podium_h * 0.6, (front.len * 0.4).min(14.0), podium_h * 0.55);
    led_screen(g, c, rng, &front, front.len * 0.26, podium_h * 0.6, (front.len * 0.3).min(10.0), podium_h * 0.55);
    // Tower: glossy loft with a glowing crown ring.
    let floors = (c.floors as i32).max(10);
    let tower = fit_inside(c, &shift_poly(&scale_poly(poly, 0.62), Vec2::new(0.0, c.fd * 0.1)));
    let th = floors as f32 * fh;
    let glass = pick(rng, &pal.glass);
    loft(g, f, Glass, &tower, &scale_poly(&tower, 0.9), podium_h, podium_h + th, glass);
    curtain_lights(g, c, rng, &tower, podium_h, floors, fh);
    let crown = scale_poly(&tower, 0.95);
    extrude(g, f, Glow, &offset_poly(&crown, -0.3), podium_h + th, podium_h + th + 1.6, mul(pick(rng, &pal.led_colors), 0.8));
    extrude(g, f, Panel, &scale_poly(&tower, 0.86), podium_h + th + 1.6, podium_h + th + 2.4, dark);
    sign_board(g, c, rng, &c.front_face(&crown), 0.0, podium_h + th - 2.5, (c.front_face(&crown).len * 0.8).min(16.0), 2.0);
    antenna_mast(g, c, poly_center(&tower), podium_h + th + 2.4, 6.0);
    // Porte-cochère, fountain, palms and limos.
    let reach = (c.reach_out(&front) - 0.5).min(9.0);
    if reach > 2.5 {
        front.bx(g, f, Panel, 0.0, podium_h * 0.5 + 0.5, reach * 0.5, Vec3::new(front.len * 0.5, 0.5, reach), dark);
        front.bx(g, f, Glow, 0.0, podium_h * 0.5 + 0.22, reach * 0.5, Vec3::new(front.len * 0.46, 0.04, reach - 0.4), mul(pal.window_warm, 0.9));
        for s in [-1.0f32, 1.0] {
            for d in [1.0, reach - 1.0] {
                front.bx(g, f, Matte, s * front.len * 0.24, podium_h * 0.25 + 0.25, d, Vec3::new(0.5, podium_h * 0.5 + 0.5, 0.5), mul(pal.steel, 0.9));
            }
        }
    }
    entrance(g, c, rng, &front, 0.0, 5.0, podium_h * 0.6);
    let fp = front.at(0.0, -4.0);
    if c.inside_local(fp) && c.inside_local(fp - Vec2::new(0.0, 3.0)) {
        pool(g, c, fp.x, fp.y, 5.0, 3.0);
        f.bx(g, Glow, fp.x, 1.2, fp.y, Vec3::new(0.3, 2.0, 0.3), mul(pal.water, 1.6));
    }
    for s in [-1.0f32, 1.0] {
        let p = front.at(s * front.len * 0.38, -2.5);
        if c.inside_local(p) {
            tree_kind(g, f.to_world(p.x, p.y), 0.0, TreeKind::Palm, 7.0, 2.2, pal.bark, pal.foliage);
            vehicle(g, f, pal, rng, front.at(s * front.len * 0.2, -3.0), 0.0, front.yaw(), Vehicle::Car, [0.05, 0.05, 0.06]);
        }
    }
    bollards(g, c, &front, 8, (reach + 1.0).max(1.0));
}

pub fn arena(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    if nd_sim::zoning::polygon_area(&c.lot_poly).abs() < 1400.0 {
        return casino(g, c, rng);
    }
    let f = &c.frame;
    let pal = c.palette;
    let plaza = yard(g, c, 0.5, Paving, pal.paving);
    let center = poly_center(&c.poly);
    let r = (inradius(&c.poly) * 0.92).max(4.0);
    let h = (c.floors as f32 * c.fh()).clamp(12.0, 26.0);
    let bowl = ngon(center.x, center.y, r, 18, 0.0);
    let panel = pick(rng, &pal.panel);
    // Outward-leaning shell with a lit band and a roof ring.
    loft(g, f, Panel, &bowl, &scale_poly(&bowl, 1.12), 0.0, h * 0.7, panel);
    loft(g, f, Panel, &scale_poly(&bowl, 1.12), &scale_poly(&bowl, 1.05), h * 0.7, h, mul(panel, 0.85));
    extrude(g, f, Glow, &scale_poly(&bowl, 1.125), h * 0.35, h * 0.42, mul(pick(rng, &pal.led_colors), 0.7));
    extrude(g, f, Glow, &scale_poly(&bowl, 1.08), h * 0.74, h * 0.77, mul(pal.window_cold, 0.6));
    extrude(g, f, Metal, &scale_poly(&bowl, 1.05), h, h + 1.2, mul(pal.steel, 0.9));
    let roof = scale_poly(&bowl, 1.0);
    extrude(g, f, Metal, &roof, h + 1.2, h + 1.5, mul(panel, 0.7));
    // Structural ribs around the shell.
    for p in &bowl {
        let dir = (*p - center).normalize_or_zero();
        let a = f.at2(*p + dir * 0.3, 0.0);
        let b = f.at2(*p + dir * (r * 0.12 + 0.3), h * 0.7);
        g.part(Matte).beam(a, b, 0.6, rgba(mul(pal.steel, 0.8)));
    }
    // Front gates and a big screen.
    let front = c.front_face(&c.poly);
    // Gates on the bowl's road side, facing the road.
    let gate = super::Face::new(center + Vec2::new(-r * 0.4, -r), center + Vec2::new(r * 0.4, -r), Vec2::new(0.0, -1.0));
    for k in 0..3 {
        gate.bx(g, f, Matte, (k as f32 - 1.0) * r * 0.25, 1.8, 0.1, Vec3::new(2.4, 3.6, 0.3), DARK);
        gate.bx(g, f, Glow, (k as f32 - 1.0) * r * 0.25, 3.9, 0.3, Vec3::new(2.2, 0.3, 0.1), mul(pal.lamp, 0.9));
    }
    led_screen(g, c, rng, &gate, 0.0, h * 0.55, (r * 0.9).min(16.0), (r * 0.5).min(8.0));
    // Pylons with floodlights, trees and bollards on the plaza.
    for p in &plaza {
        let inset = *p + (center - *p).normalize_or_zero() * 2.5;
        if c.inside_local(inset) && inset.distance(center) > r * 1.2 + 2.0 {
            floodlight(g, c, inset, 14.0);
        }
    }
    for face in c.faces(&plaza) {
        if face.len > 10.0 {
            for k in 0..3 {
                let p = face.at(-face.len * 0.3 + k as f32 * face.len * 0.3, -2.0);
                if c.inside_local(p) && p.distance(center) > r * 1.15 + 2.5 {
                    tree(g, f, p, 3.5, 3.0, pal.bark, pal.foliage);
                }
            }
        }
    }
    bollards(g, c, &front, 8, -1.2);
}
