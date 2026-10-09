//! Commerce: shophouse rows and market halls at the bottom, retail blocks and
//! podium offices in the middle, corporate slabs, twisting towers and
//! cantilevered malls at the top.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

// ── shophouses ───────────────────────────────────────────────────────────────

pub fn shophouse(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let front = c.front_face(poly);
    let depth = inradius(poly).max(3.0) * 1.3;
    let base_floors = 1;
    extrude(g, f, Brick, poly, 0.0, base_floors as f32 * fh, mul(pick(rng, &pal.brick), 0.9));
    let units = ((front.len / 6.0).floor() as i32).clamp(1, 6);
    let uw = front.len / units as f32;
    let mut prev_h = 0.0;
    for i in 0..units {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * uw;
        let floors = (c.floors as i32 + rng.random_range(-1..=1)).clamp(2, 4);
        let h = floors as f32 * fh;
        let (s, col) = match rng.random_range(0..3) {
            0 => (Brick, pick(rng, &pal.brick)),
            1 => (Plaster, pick(rng, &pal.plaster)),
            _ => (Panel, pick(rng, &pal.panel)),
        };
        let col = mul(col, 1.0 - 0.3 * c.wear);
        // Upper floors of this unit, kept inside the footprint: on a wedge
        // or a notched lot the back corners are pulled in until they fit.
        let mut d = depth.min(c.fd * 0.9);
        while d > 3.0 && !(c.inside_local(front.at(t - uw * 0.5, -d)) && c.inside_local(front.at(t + uw * 0.5, -d))) {
            d *= 0.85;
        }
        front.bx(g, f, s, t, (fh + h) * 0.5, -d * 0.5, Vec3::new(uw - 0.1, h - fh, d), col);
        let sub = Face::from_edge(front.at(t - uw * 0.5, 0.0), front.at(t + uw * 0.5, 0.0));
        storefront(g, c, rng, &sub, 0.0, fh);
        let mut spec = WindowSpec::from_ctx(c);
        spec.w = 1.2;
        spec.h = 1.6;
        spec.spacing = 2.0;
        spec.ac = 0.5;
        spec.boarded = 0.1 + 0.5 * c.wear;
        windows(g, c, rng, &sub, fh, floors - 1, fh, &spec);
        // Parapet detail varies per unit.
        match rng.random_range(0..3) {
            0 => sub.bx(g, f, Concrete, 0.0, h + 0.3, -0.2, Vec3::new(uw - 0.1, 0.6, 0.5), mul(col, 1.15)),
            1 => sub.bx(g, f, s, 0.0, h + 0.5, -0.3, Vec3::new(uw * 0.5, 1.0, 0.6), col),
            _ => cornice_face(g, c, &sub, h - 0.2, 0.4, 0.3, mul(col, 1.2)),
        }
        if coin(rng, c.rules.vertical_sign_chance + 0.25) {
            vertical_sign(g, c, rng, &sub, uw * 0.4, fh + 0.5, (h - fh - 1.0).max(2.0));
        }
        grime(g, c, rng, &sub, fh, h, 2.0);
        // Roof tanks and dishes, a billboard on one unit.
        let roof_p = front.at(t, -d * 0.5);
        if rng.random_bool(0.5) {
            let (s, col) = if rng.random_bool(0.5) { (Rust, pal.rust) } else { (Metal, mul(pal.steel, 0.8)) };
            water_tank(g, f, roof_p + Vec2::new(0.0, 1.0), h, 0.9, 1.6, s, col);
        }
        if rng.random_bool(0.4) {
            dish(g, f, roof_p + Vec2::new(uw * 0.3, 0.0), h, 0.7, pal.steel);
        }
        if i == units / 2 && rng.random_bool(0.6) && c.inside_local(roof_p + Vec2::new(-uw * 0.45, -0.5)) && c.inside_local(roof_p + Vec2::new(uw * 0.45, -0.5)) {
            billboard(g, c, rng, roof_p + Vec2::new(0.0, -0.5), 0.0, h, (uw * 0.9).min(8.0), 2.8);
        }
        // Wires between neighbouring roofs.
        if i > 0 {
            let a = f.at(front.at(t - uw, 0.3).x, prev_h + 0.3, front.at(t - uw, 0.3).y);
            let b = f.at(front.at(t, 0.3).x, h + 0.3, front.at(t, 0.3).y);
            g.part(Matte).wire(a, b, 0.5, 0.03, rgba(DARK));
        }
        prev_h = h;
    }
    // Side and back faces: plain brick with a few windows.
    for face in c.faces(poly) {
        if face.is_front || face.len < 4.0 {
            continue;
        }
        let mut spec = WindowSpec::from_ctx(c);
        spec.boarded = 0.3 + 0.4 * c.wear;
        windows(g, c, rng, &face, 0.0, 1, fh, &spec);
        grime(g, c, rng, &face, 0.0, fh, 2.0);
        downpipes(g, c, rng, &face, fh);
    }
    // Pavement life: stalls and a parked van.
    if rng.random_bool(0.5) {
        stall(g, c, rng, &front, -front.len * 0.3, -2.5);
    }
}

fn cornice_face(g: &mut Geometry, c: &Ctx, face: &Face, y: f32, h: f32, proud: f32, color: [f32; 3]) {
    face.bx(g, &c.frame, Concrete, 0.0, y, proud * 0.5, Vec3::new(face.len, h, proud + 0.02), color);
}

// ── market hall ──────────────────────────────────────────────────────────────

pub fn market_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = &c.poly;
    let h = 6.0;
    let steel = mul(pick(rng, &pal.metal_paint), 1.0 - 0.3 * c.wear);
    ground(g, f, Concrete, poly, 0.05, mul(pick(rng, &pal.concrete), 0.85));
    pilotis(g, c, &offset_poly(poly, 0.5), 0.0, h, 5.0, 0.4, Matte, steel);
    // Roof: metal deck with a raised clerestory ridge.
    extrude(g, f, Metal, &offset_poly(poly, -0.4), h, h + 0.35, steel);
    let ridge = offset_poly(poly, inradius(poly) * 0.45);
    if ridge.len() >= 3 {
        extrude(g, f, Metal, &ridge, h + 0.35, h + 2.0, mul(steel, 0.9));
        extrude(g, f, Glow, &offset_poly(&ridge, -0.05), h + 0.8, h + 1.5, mul(pal.window_warm, 0.5));
    }
    // Low wall around three sides, open front.
    for face in c.faces(poly) {
        if face.is_front {
            continue;
        }
        face.bx(g, f, Brick, 0.0, 0.6, -0.3, Vec3::new(face.len, 1.2, 0.3), pick(rng, &pal.brick));
        // corrugated infill above the wall on the back
        face.bx(g, f, Metal, 0.0, 1.2 + (h - 1.2) * 0.5, -0.25, Vec3::new(face.len * 0.98, h - 1.2, 0.1), mul(steel, 0.9));
    }
    // Stalls in rows under the roof, facing the aisles.
    let (lo, hi) = bounds(&offset_poly(poly, 2.5));
    let rows = ((hi.y - lo.y) / 6.0).floor().max(1.0) as i32;
    for r in 0..rows {
        let v = lo.y + (r as f32 + 0.5) * (hi.y - lo.y) / rows as f32;
        let row = Face::from_edge(Vec2::new(lo.x, v + 1.2), Vec2::new(hi.x, v + 1.2));
        let n = ((hi.x - lo.x) / 5.0).floor().max(1.0) as i32;
        for k in 0..n {
            let t = -(hi.x - lo.x) * 0.5 + (k as f32 + 0.5) * (hi.x - lo.x) / n as f32;
            if rng.random_bool(0.2) {
                continue;
            }
            let p = row.at(t, 0.0);
            if c.inside_local(p) {
                stall(g, c, rng, &row, t, 0.0);
            }
        }
        // hanging lamps along the aisle
        for k in 0..n {
            let t = -(hi.x - lo.x) * 0.5 + (k as f32 + 0.5) * (hi.x - lo.x) / n as f32;
            let p = row.at(t, -2.4);
            if c.inside_local(p) {
                f.bx(g, Matte, p.x, h - 0.6, p.y, Vec3::new(0.03, 1.2, 0.03), DARK);
                f.bx(g, Glow, p.x, h - 1.3, p.y, Vec3::new(0.5, 0.2, 0.5), mul(pal.lamp, 0.9));
            }
        }
    }
    let front = c.front_face(poly);
    sign_board(g, c, rng, &front, 0.0, h + 0.9, (front.len * 0.5).min(14.0), 1.2);
    // Strings of bulbs under the roof, in rows back from the open front.
    for k in 0..3 {
        let a = front.at(-front.len * 0.45, -(1.0 + k as f32 * 4.0));
        let b = front.at(front.len * 0.45, -(1.0 + k as f32 * 4.0));
        if c.inside_local(a) && c.inside_local(b) {
            string_lights(g, f, Vec3::new(a.x, h - 0.2, a.y), Vec3::new(b.x, h - 0.2, b.y), 10, mul(pal.window_warm, 1.2));
        }
    }
    // Crates and a delivery van outside.
    let side = front.at(front.len * 0.42, -2.0);
    if c.inside_local(side) {
        for k in 0..3 {
            f.bx(g, Wood, side.x, 0.4 + k as f32 * 0.8, side.y, Vec3::new(1.0, 0.8, 1.0), pal.wood);
        }
    }
}

// ── retail block ─────────────────────────────────────────────────────────────

pub fn retail_block(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    super::residential::brutalist_block(g, c, rng, false, true);
    let fh = c.fh();
    let front = c.front_face(&c.poly);
    storefront(g, c, rng, &front, 0.0, fh);
    for face in c.faces(&c.poly) {
        if !face.is_front && face.len > 10.0 && rng.random_bool(0.5) {
            storefront(g, c, rng, &face, 0.0, fh);
        }
    }
    if rng.random_bool(0.6) {
        vertical_sign(g, c, rng, &front, front.len * 0.42, fh + 1.0, (c.height() * 0.5).clamp(3.0, 12.0));
    }
}

// ── podium office ────────────────────────────────────────────────────────────

pub fn podium_office(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let podium_h = fh * 2.0;
    let glass = pick(rng, &pal.glass);
    extrude(g, f, Glass, poly, 0.0, podium_h, glass);
    curtain_lights(g, c, rng, poly, 0.0, 2, fh);
    extrude(g, f, Concrete, &offset_poly(poly, -0.4), podium_h, podium_h + 0.6, mul(pick(rng, &pal.concrete), 1.1));
    // Mullions on the podium.
    for face in c.faces(poly) {
        let n = (face.len / 2.5).floor() as i32;
        for k in 0..=n {
            let t = -face.len * 0.5 + k as f32 * (face.len / n.max(1) as f32);
            face.bx(g, f, Matte, t, podium_h * 0.5, 0.06, Vec3::new(0.12, podium_h, 0.12), c.style.trim_color);
        }
    }
    // Tower set back on the podium.
    let floors = (c.floors as i32 - 2).max(4);
    let tower = fit_inside(c, &shift_poly(&offset_poly(poly, 3.0), Vec2::new(0.0, 1.5)));
    let th = floors as f32 * fh;
    let panel = pick(rng, &pal.panel);
    extrude(g, f, Panel, &tower, podium_h + 0.6, podium_h + 0.6 + th, panel);
    for face in c.faces(&tower) {
        if face.len > 3.0 {
            band_windows(g, c, rng, &face, podium_h + 0.6, floors, fh);
        }
    }
    cornice(g, c, &tower, podium_h + 0.6 + th - 0.5, 1.0, 0.4, Panel, mul(panel, 0.8));
    roof_gear(g, c, rng, &tower, podium_h + 0.6 + th, 0.7);
    let tf = c.front_face(&tower);
    sign_board(g, c, rng, &tf, 0.0, podium_h + 0.6 + th - 2.2, (tf.len * 0.5).min(10.0), 1.4);
    // Plaza in the setback: trees, bollards, flagless masts.
    let front = c.front_face(poly);
    entrance(g, c, rng, &front, 0.0, 3.0, podium_h * 0.7);
    bollards(g, c, &front, 5, 1.6);
    for k in 0..2 {
        let p = front.at((k as f32 - 0.5) * front.len * 0.6, -1.4);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 {
            tree_in(g, f, pal, p, 3.5, 3.0, pal.foliage);
        }
    }
}

// ── corporate slab ───────────────────────────────────────────────────────────

pub fn slab_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).max(10);
    let glass = pick(rng, &pal.glass);
    let base = c.poly.clone();
    // Up to two setbacks.
    let steps = rng.random_range(1..=3);
    let mut y = 0.0;
    let mut poly = base.clone();
    let mut remaining = floors;
    for s in 0..steps {
        let fl = if s == steps - 1 { remaining } else { (remaining as f32 * rng.random_range(0.45..0.65)) as i32 };
        remaining -= fl;
        let h = fl as f32 * fh;
        extrude(g, f, Glass, &poly, y, y + h, glass);
        curtain_lights(g, c, rng, &poly, y, fl, fh);
        // vertical fins
        for face in c.faces(&poly) {
            let n = (face.len / 3.0).floor() as i32;
            for k in 0..=n {
                let t = -face.len * 0.5 + k as f32 * (face.len / n.max(1) as f32);
                face.bx(g, f, Matte, t, y + h * 0.5, 0.1, Vec3::new(0.2, h, 0.25), c.style.trim_color);
            }
        }
        extrude(g, f, Panel, &offset_poly(&poly, -0.3), y + h, y + h + 0.5, mul(pick(rng, &pal.panel), 0.9));
        y += h + 0.5;
        if s < steps - 1 {
            poly = fit_inside(c, &shift_poly(&scale_poly(&poly, rng.random_range(0.65..0.85)), Vec2::new(rng.random_range(-1.5..1.5), rng.random_range(-1.0..1.0))));
            roof_gear(g, c, rng, &poly, y, 0.3);
        }
    }
    // Crown: plant screen and aviation lights.
    let crown = offset_poly(&poly, 1.0);
    extrude(g, f, Panel, &crown, y, y + 2.5, mul(pick(rng, &pal.panel), 0.7));
    for p in &poly {
        aviation_light(g, pal, f.at2(*p, y + 0.3), 0.4);
    }
    // With a helipad on top the mast stands at the edge, not through the pad.
    // A pad needs a roof wide enough for its 2 m minimum radius.
    let pad = rng.random_bool(0.5) && inradius(&poly) >= 3.4;
    let mast = if pad && !poly.is_empty() { poly_center(&poly).lerp(poly[0], 0.85) } else { poly_center(&poly) };
    antenna_mast(g, c, mast, y + 2.5, 9.0);
    if pad {
        helipad(g, c, poly_center(&poly), y + 2.5, (inradius(&poly) * 0.6).clamp(2.0, 6.0));
    }
    let front = c.front_face(&base);
    entrance(g, c, rng, &front, 0.0, 4.0, fh * 1.5);
    canopy(g, c, &front, 0.0, fh * 1.6, front.len * 0.6, 4.0, mul(pal.steel, 0.9));
    bollards(g, c, &front, 6, 2.0);
    sign_board(g, c, rng, &front, 0.0, fh * 2.6, (front.len * 0.3).min(8.0), 1.0);
}

// ── twist tower ──────────────────────────────────────────────────────────────

pub fn twist_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).max(12);
    let glass = pick(rng, &pal.glass);
    let podium_h = fh * 1.5;
    extrude(g, f, Stone, &c.poly, 0.0, podium_h, mul(pal.stone, 1.3));
    let base = fit_inside(c, &scale_poly(&c.poly, 0.8));
    let total_twist = rng.random_range(0.6..1.6) * if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let taper = rng.random_range(0.55..0.85);
    let mut prev = base.clone();
    let mut y = podium_h;
    for k in 0..floors {
        let t = (k + 1) as f32 / floors as f32;
        let plate = fit_inside(c, &rotate_poly(&scale_poly(&base, 1.0 - (1.0 - taper) * t), total_twist * t));
        loft(g, f, Glass, &prev, &plate, y, y + fh, glass);
        // floor plate edge
        // The plate's edge: a band without caps, which would lie in the
        // glass's own top and the next floor's bottom.
        band(g, f, Panel, &offset_poly(&prev, -0.12), &offset_poly(&plate, -0.12), y + fh - 0.25, y + fh, mul(pick(rng, &pal.panel), 0.8));
        if k % 2 == 0 {
            // Lit at mid-floor, on the facade where the glass is there.
            let mid: Vec<Vec2> = if prev.len() == plate.len() { prev.iter().zip(&plate).map(|(a, b)| a.lerp(*b, 0.5)).collect() } else { prev.clone() };
            curtain_lights(g, c, rng, &mid, y, 1, fh);
        }
        prev = plate;
        y += fh;
    }
    extrude(g, f, Panel, &prev, y, y + 0.6, mul(pal.steel, 0.8));
    // A cut glass crown, not a needle.
    let top = super::kit::crystal_crown(g, c, rng, &prev, y + 0.6, glass);
    aviation_light(g, pal, f.at2(poly_center(&prev), top + 0.3), 0.4);
    band(g, f, Glow, &offset_poly(&prev, -0.05), &offset_poly(&prev, -0.05), y - 0.6, y, mul(c.style.window_color(0), 0.9));
    let front = c.front_face(&c.poly);
    entrance(g, c, rng, &front, 0.0, 3.5, podium_h * 0.8);
    bollards(g, c, &front, 5, 1.6);
    for k in 0..3 {
        lamp_post_in(g, c, front.at(-front.len * 0.35 + k as f32 * front.len * 0.35, -1.5), 3.5, pal.lamp);
    }
}

// ── cantilever mall ──────────────────────────────────────────────────────────

pub fn cantilever_mall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh() * 1.3;
    let boxes = rng.random_range(3..=5).min((c.floors as i32).max(3));
    let mut below = c.poly.clone();
    let mut y = 0.0;
    for i in 0..boxes {
        let h = fh * if i == 0 { 1.4 } else { 1.0 };
        let shift = if i == 0 { Vec2::ZERO } else { Vec2::new(rng.random_range(-0.3..0.3) * c.fw, rng.random_range(-0.25..0.25) * c.fd) };
        let plate = if i == 0 { below.clone() } else { shift_poly(&scale_poly(&c.poly, rng.random_range(0.6..0.9)), shift) };
        let plate = fit_inside(c, &plate);
        // Keep at least half the plate over the box below; otherwise pull it back.
        let plate = {
            let mut p = plate;
            for _ in 0..6 {
                let (lo, hi) = bounds(&p);
                let mut inside = 0;
                let mut total = 0;
                for ix in 0..5 {
                    for iy in 0..5 {
                        let q = Vec2::new(lo.x + (ix as f32 + 0.5) / 5.0 * (hi.x - lo.x), lo.y + (iy as f32 + 0.5) / 5.0 * (hi.y - lo.y));
                        if nd_sim::zoning::point_in_polygon(q, &p) {
                            total += 1;
                            if nd_sim::zoning::point_in_polygon(q, &below) {
                                inside += 1;
                            }
                        }
                    }
                }
                if total == 0 || inside as f32 >= total as f32 * 0.55 {
                    break;
                }
                let d = poly_center(&below) - poly_center(&p);
                p = shift_poly(&p, d * 0.3);
            }
            // Pulling it back may have pushed it over the lot edge.
            fit_inside(c, &p)
        };
        let col = pick(rng, &pal.panel);
        let s = if i % 2 == 0 { Panel } else { Glass };
        extrude(g, f, s, &plate, y, y + h, if s == Glass { pick(rng, &pal.glass) } else { col });
        if s == Glass {
            curtain_lights(g, c, rng, &plate, y, 1, h);
        } else {
            // LED strip along the box edges
            let strip = mul(pick(rng, &pal.led_colors), 0.28);
            for face in c.faces(&plate) {
                face.decal(g, f, Glow, 0.0, y + h - 0.15, 0.04, face.len * 0.98, 0.07, strip);
            }
        }
        // Columns under corners that overhang the box below.
        if i > 0 {
            for p in &plate {
                let inset = toward(*p, poly_center(&plate), 0.8);
                if !nd_sim::zoning::point_in_polygon(inset, &below) {
                    let base_y = if c.inside_local(inset) { 0.0 } else { y };
                    f.bx(g, Matte, inset.x, (base_y + y) * 0.5, inset.y, Vec3::new(0.5, y - base_y, 0.5), mul(pal.steel, 0.8));
                }
            }
        }
        y += h;
        below = plate;
    }
    roof_gear(g, c, rng, &below, y, 0.6);
    let front = c.front_face(&c.poly);
    led_screen(g, c, rng, &front, 0.0, fh * 1.4 + 3.0, (front.len * 0.6).min(18.0), (front.len * 0.3).min(8.0));
    storefront(g, c, rng, &front, 0.0, fh * 1.4);
    // Escalator tube up the front, inside the forecourt.
    let td = (1.0f32).min(c.reach_out(&front) - 1.4);
    let a = f.at2(front.at(-front.len * 0.3, td), 1.0);
    let b = f.at2(front.at(front.len * 0.1, td), fh * 1.4 + 1.0);
    g.part(Glass).beam(a, b, 2.4, rgba(pick(rng, &pal.glass)));
    g.part(Glow).beam(a + Vec3::Y * 0.4, b + Vec3::Y * 0.4, 0.3, rgba(mul(pal.window_cold, 0.7)));
    bollards(g, c, &front, 6, 2.0);
}
