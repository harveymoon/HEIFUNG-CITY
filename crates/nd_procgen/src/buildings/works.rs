//! More industry: garage workshops on cheap land, Hong Kong flatted
//! factories in the middle, semiconductor fabs where land is dear.

use super::kit::*;
use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

// ── workshops ────────────────────────────────────────────────────────────────

pub fn workshops(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Dirt, mul(pal.dirt, 0.9));
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    // A row of bays along the frontage, a yard behind.
    let depth = ((hi.y - lo.y) * 0.55).clamp(6.0, 14.0);
    let row = clip_box(poly, lo.x, hi.x, lo.y, lo.y + depth);
    let row = if row.is_empty() { fit_inside(c, &scale_poly(poly, 0.7)) } else { row };
    let upper = c.floors >= 2;
    let fh = 4.2;
    let h = if upper { fh + 3.0 } else { fh };
    let (wall_s, wall) = if c.wear > 0.4 { (Rust, mul(pal.rust, 0.9)) } else { (Metal, mul(pick(rng, &pal.metal_paint), 0.7)) };
    extrude(g, f, Concrete, &row, 0.0, fh, mul(pick(rng, &pal.concrete), 0.8));
    if upper {
        extrude(g, f, wall_s, &offset_poly(&row, 0.4), fh, h, wall);
    }
    // Corrugated lean-to roof.
    extrude(g, f, Metal, &offset_poly(&row, -0.3), h, h + 0.25, mul(pal.steel, 0.55));
    let front = c.front_face(&row);
    let bays = ((front.len / 4.5).floor() as i32).clamp(1, 7);
    let bw = front.len / bays as f32;
    for i in 0..bays {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * bw;
        let open = rng.random_bool(0.35);
        if open {
            // An open bay: dark interior, a lit strip, a car on the ramp.
            front.decal(g, f, Matte, t, 1.7, 0.03, bw - 0.8, 3.4, DARK);
            front.decal(g, f, Glow, t, 3.2, 0.05, bw - 1.2, 0.15, mul(pal.lamp, 0.9));
        } else {
            roller_door(g, c, &front, t, bw - 0.8, 3.4, mul(pick(rng, &pal.metal_paint), 0.8 - 0.3 * c.wear));
        }
        if rng.random_bool(0.6) {
            sign_board(g, c, rng, &front, t, 3.9, (bw - 1.0).min(3.5), 0.5);
        }
        graffiti(g, c, rng, &front, 0.0, 3.4, 0.4 + c.wear);
    }
    if upper {
        // Gallery walkway and a rusted stair up to the rooms above.
        let room = c.reach_out(&front).clamp(0.0, 1.4);
        let (s, col) = outdoor_steel(c, mul(pal.steel, 0.6));
        if room > 0.6 {
            front.bx(g, f, s, 0.0, fh, room * 0.5, Vec3::new(front.len * 0.95, 0.12, room), col);
            front.decal(g, f, s, 0.0, fh + 0.55, room, front.len * 0.95, 1.0, col);
        }
        let mut spec = WindowSpec::from_ctx(c);
        spec.spacing = 3.0;
        spec.boarded = 0.2 + 0.5 * c.wear;
        windows(g, c, rng, &front, fh, 1, 3.0, &spec);
        let side = c.faces(&row).into_iter().filter(|x| !x.is_front && x.len > 4.0).max_by(|a, b| a.len.total_cmp(&b.len));
        if let Some(sf) = side {
            let room = c.reach_out(&sf);
            if room > 1.6 {
                let a = f.at2(sf.at(-sf.len * 0.4, 0.7), 0.1);
                let b = f.at2(sf.at(sf.len * 0.1, 0.7), fh);
                g.part(s).slab(a, b, 1.0, 0.18, rgba(col));
            }
        }
    }
    for face in c.faces(&row) {
        if !face.is_front && face.len > 3.0 {
            grime(g, c, rng, &face, 0.0, h, 2.0);
            downpipes(g, c, rng, &face, h);
        }
    }
    roof_gear(g, c, rng, &row, h + 0.25, 0.6 + c.style.clutter);
    // The yard: scrap, tyres, a car on blocks, drums and a van.
    let back = clip_box(&yard, lo.x - 2.0, hi.x + 2.0, lo.y + depth + 1.0, hi.y + 2.0);
    if !back.is_empty() && inradius(&back) > 2.5 {
        for _ in 0..3 {
            if let Some(p) = random_in_poly(rng, &back, 2.0) {
                match rng.random_range(0..3) {
                    0 => {
                        let (r, h) = (rng.random_range(1.2..2.2), rng.random_range(0.8..1.6));
                        scrap_pile(g, c, rng, p, r, h)
                    }
                    1 => {
                        let yaw = rng.random_range(0.0..std::f32::consts::TAU);
                        if vehicle_fits(c, p, yaw, Vehicle::Wreck) {
                            let col = pick(rng, &pal.car_colors);
                            vehicle(g, f, pal, rng, p, 0.0, yaw, Vehicle::Wreck, col);
                        }
                    }
                    _ => {
                        for k in 0..3 {
                            let q = p + Vec2::new(k as f32 * 0.7, 0.0);
                            if c.inside_local(q) {
                                let w = f.to_world(q.x, q.y);
                                g.part(Rust).cylinder(w, 0.3, 0.3, 0.0, 0.9, 8, rgba(pal.rust));
                            }
                        }
                    }
                }
            }
        }
        if c.wear > 0.5 {
            if let Some(p) = random_in_poly(rng, &back, 1.0) {
                drum_fire(g, f, pal, p);
            }
        }
    }
    fence_lot(g, c, &yard, 2.4, c.wear > 0.3);
}

// ── flatted factory ──────────────────────────────────────────────────────────

pub fn flatted_factory(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = &c.poly;
    let fh = 4.0;
    // Industrial floors are few in the sim; the form follows them: one is a
    // broad shed with docks, more stack into a slab of factory floors.
    let storeys = match c.floors {
        0 | 1 => 1,
        2 => 5,
        _ => (c.floors as i32 * 2 + 1).clamp(6, 14),
    };
    let h = storeys as f32 * fh;
    let wall = mul(pick(rng, &pal.concrete), 0.95 - 0.25 * c.wear);
    let (lo, hi) = bounds(poly);
    let body = if storeys == 1 { poly.clone() } else { clip_box(poly, lo.x, hi.x, lo.y + 4.0, hi.y) };
    let body = if body.is_empty() { fit_inside(c, &scale_poly(poly, 0.8)) } else { body };
    extrude(g, f, Concrete, &body, 0.0, h, wall);
    for face in c.faces(&body) {
        if face.len < 4.0 {
            continue;
        }
        if storeys > 1 {
            // Long ribbon windows per floor, painted spandrels.
            for k in 0..storeys {
                let y = k as f32 * fh;
                face.decal(g, f, Glass, 0.0, y + fh * 0.58, 0.03, face.len * 0.9, fh * 0.42, pal.glass_dark);
                if coin(rng, c.lit_chance() * 1.2) {
                    let span = face.len * rng.random_range(0.2..0.6);
                    face.decal(g, f, Glow, rng.random_range(-(face.len * 0.45 - span * 0.5)..=(face.len * 0.45 - span * 0.5)), y + fh * 0.58, 0.05, span, fh * 0.34, mul([1.1, 1.15, 1.2], 0.75));
                }
                face.decal(g, f, Panel, 0.0, y + fh * 0.18, 0.03, face.len, 0.3, mul(c.style.trim_color, 1.1));
            }
        }
        grime(g, c, rng, &face, 0.0, h, 2.0);
    }
    // Goods lift tower on one side, with the company name down it.
    let side = c.faces(&body).into_iter().filter(|x| !x.is_front && x.len > 6.0).max_by(|a, b| a.len.total_cmp(&b.len));
    if let Some(sf) = side {
        if storeys > 1 && c.reach_out(&sf) > 0.4 {
            sf.bx(g, f, Concrete, 0.0, (h + 3.0) * 0.5, 0.0, Vec3::new(4.0, h + 3.0, 2.4), mul(wall, 0.9));
            vertical_sign(g, c, rng, &sf, 0.0, h * 0.4, h * 0.5);
        }
    }
    // Loading docks at the street: levellers, roller doors, a lorry.
    let front = c.front_face(&body);
    let docks = ((front.len / 6.0).floor() as i32).clamp(1, 4);
    for i in 0..docks {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * front.len / docks as f32;
        roller_door(g, c, &front, t, 3.6, 3.8, mul(pick(rng, &pal.metal_paint), 0.8));
        let room = c.reach_out(&front).min(1.8);
        if room > 0.6 {
            front.bx(g, f, Concrete, t, 0.6, room * 0.5, Vec3::new(4.2, 1.2, room), mul(wall, 0.8));
            front.bx(g, f, Matte, t, 1.25, room * 0.5, Vec3::new(4.0, 0.1, room), [0.7, 0.55, 0.1]);
        }
    }
    canopy(g, c, &front, 0.0, 4.6, front.len * 0.9, 3.0, mul(pal.steel, 0.7));
    if storeys > 1 {
        let room_p = front.at(front.len * 0.2, 2.5);
        let yaw = front.yaw();
        if vehicle_fits(c, room_p, yaw, Vehicle::Truck) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, room_p, 0.0, yaw, Vehicle::Truck, col);
        }
        sign_board(g, c, rng, &front, 0.0, h - 1.4, (front.len * 0.6).min(14.0), 1.6);
    }
    roof_gear(g, c, rng, &body, h, 1.0);
    if storeys == 1 {
        // Saw-tooth roof lights on the shed.
        let n = ((hi.y - lo.y) / 6.0) as i32;
        for k in 0..n {
            let v = lo.y + 3.0 + k as f32 * 6.0;
            let strip = clip_box(&body, lo.x + 1.0, hi.x - 1.0, v - 0.8, v + 0.8);
            if !strip.is_empty() {
                extrude(g, f, Glass, &strip, h, h + 1.2, pal.glass(0));
            }
        }
    }
}

// ── semiconductor fab ────────────────────────────────────────────────────────

pub fn fab_plant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Paving, mul(pal.paving, 1.2));
    let poly = offset_poly(&yard, 2.5);
    let poly = if poly.len() >= 3 && inradius(&poly) > 3.0 { poly } else { c.poly.clone() };
    let white = [0.86, 0.87, 0.88];
    let form = Form::of(c.floors * 3);
    // The cleanroom: a big white box; tall sites stack a second, smaller one.
    let h1 = match form {
        Form::Low => 9.0,
        Form::Mid => 14.0,
        Form::High => 18.0,
    };
    let (lo, hi) = bounds(&poly);
    let main = clip_box(&poly, lo.x, hi.x, lo.y + (hi.y - lo.y) * 0.25, hi.y);
    let main = if main.is_empty() { fit_inside(c, &scale_poly(&poly, 0.75)) } else { main };
    extrude(g, f, Panel, &main, 0.0, h1, white);
    glow_outline(g, c, &offset_poly(&main, -0.05), h1 - 0.6, 0.18, mul(pick(rng, &pal.led_colors), 0.9));
    for face in c.faces(&main) {
        if face.len > 6.0 {
            // Thin window slot at the office level only: cleanrooms are blind.
            face.decal(g, f, Glass, 0.0, 3.2, 0.03, face.len * 0.85, 0.9, pal.glass_dark);
            if coin(rng, c.lit_chance() + 0.3) {
                face.decal(g, f, Glow, 0.0, 3.2, 0.05, face.len * 0.6, 0.6, mul([1.0, 1.1, 1.25], 0.8));
            }
        }
    }
    let mut top = h1;
    if form == Form::High {
        let upper = fit_inside(c, &scale_poly(&main, 0.6));
        extrude(g, f, Panel, &upper, h1, h1 + 8.0, mul(white, 0.95));
        glow_outline(g, c, &upper, h1 + 7.6, 0.15, mul(pick(rng, &pal.led_colors), 0.8));
        top = h1 + 8.0;
    }
    // Exhaust stacks in a row along the roof, and chillers.
    let (mlo, mhi) = bounds(&main);
    let n = (((mhi.x - mlo.x) / 5.0) as i32).clamp(2, 7);
    for i in 0..n {
        let p = Vec2::new(mlo.x + (i as f32 + 0.5) * (mhi.x - mlo.x) / n as f32, mhi.y - 2.5);
        if nd_sim::zoning::point_in_polygon(p, &offset_poly(&main, 1.0)) {
            let w = f.to_world(p.x, p.y);
            g.part(Metal).cylinder(w, 0.5, 0.45, top, top + rng.random_range(5.0..9.0), 10, rgba(mul(pal.steel, 1.1)));
        }
    }
    for _ in 0..3 {
        if let Some(p) = random_in_poly(rng, &offset_poly(&main, 2.0), 0.5) {
            chiller(g, c, p, h1, 3.0, 2.0, 0.0, mul(pal.steel, 0.9));
        }
    }
    // Pipe rack across the front court to a support building.
    let support = clip_box(&poly, lo.x, lo.x + (hi.x - lo.x) * 0.35, lo.y, lo.y + (hi.y - lo.y) * 0.22);
    if !support.is_empty() && inradius(&support) > 1.5 {
        extrude(g, f, Panel, &support, 0.0, 5.0, mul(white, 0.9));
        let a = poly_center(&support);
        let b = Vec2::new(a.x + (hi.x - lo.x) * 0.3, mlo.y + 0.5);
        for k in 0..3 {
            let y = 4.0 + k as f32 * 0.5;
            pipe_run(g, f, &[Vec3::new(a.x, y, a.y), Vec3::new(a.x, y, b.y), Vec3::new(b.x, y, b.y)], 0.18, *[mul(pal.steel, 1.0), [0.2, 0.45, 0.8], [0.85, 0.75, 0.2]].get(k).unwrap());
        }
    }
    // Security: a fence, a gatehouse, floodlights.
    fence_lot(g, c, &yard, 2.6, true);
    let front = c.front_face(&yard);
    let gp = front.at(front.len * 0.3, -3.0);
    if c.inside_local(gp) && distance_to_boundary(gp, &c.lot_poly) > 1.6 {
        f.bx(g, Glass, gp.x, 1.3, gp.y, Vec3::new(2.4, 2.6, 2.0), pal.glass(0));
        f.bx(g, Panel, gp.x, 2.7, gp.y, Vec3::new(2.8, 0.2, 2.4), white);
    }
    for p in offset_poly(&yard, 1.5).iter().step_by(2) {
        floodlight(g, c, *p, 7.0);
    }
}
