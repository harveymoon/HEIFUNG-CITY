//! The dense city: Kowloon walled blocks, capsule hotels, stepped hillside
//! terraces, night market arcades and neon karaoke towers. Each changes
//! form with its floors.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::Tier;
use rand::seq::IndexedRandom;
use rand::Rng;

/// A wall finish for a self-built block: surface and faded colour.
fn patchwork(c: &Ctx, rng: &mut impl Rng) -> (Surface, [f32; 3]) {
    let pal = c.palette;
    let (s, col) = match rng.random_range(0..5) {
        0 | 1 => (Plaster, pick(rng, &pal.plaster)),
        2 => (Concrete, pick(rng, &pal.concrete)),
        3 => (Panel, pick(rng, &pal.panel)),
        _ => (Brick, pick(rng, &pal.brick)),
    };
    (s, mul(col, (1.0 - 0.3 * c.wear) * rng.random_range(0.8..1.05)))
}

// ── Kowloon walled block ─────────────────────────────────────────────────────

pub fn walled_city(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let fh = c.fh().min(2.9);
    let poly = &c.poly;
    let form = Form::of(c.floors);
    let (lo, hi) = bounds(poly);
    let (base, nu, nv) = match form {
        Form::Low => ((c.floors as i32).clamp(3, 5), ((hi.x - lo.x) / 5.5).clamp(2.0, 5.0) as usize, 1 + ((hi.y - lo.y) > 18.0) as usize),
        Form::Mid => ((c.floors as i32 + 3).clamp(7, 12), ((hi.x - lo.x) / 7.0).clamp(2.0, 4.0) as usize, ((hi.y - lo.y) / 9.0).clamp(1.0, 3.0) as usize),
        Form::High => ((c.floors as i32 + 4).clamp(11, 16), ((hi.x - lo.x) / 7.0).clamp(2.0, 4.0) as usize, ((hi.y - lo.y) / 9.0).clamp(1.0, 3.0) as usize),
    };
    let cells = split_cells(rng, poly, nu, nv);
    let cages = if c.tier == Tier::Low { 0.07 } else { 0.03 };
    for cell in &cells {
        let floors = (base + rng.random_range(-2..=2)).max(2);
        let h = floors as f32 * fh;
        let (s, col) = patchwork(c, rng);
        extrude(g, f, s, cell, 0.0, h, col);
        for face in c.faces(cell) {
            if face.len < 2.0 || !face_is_outer(&face, poly) {
                continue;
            }
            // Ground floor: shutters and shops on the street, blank elsewhere.
            if face.is_front {
                storefront(g, c, rng, &face, 0.0, fh);
            }
            dense_windows(g, c, rng, &face, fh, floors - 1, fh, 1.7, 0.16 + 0.15 * c.style.clutter, cages);
            grime(g, c, rng, &face, 0.0, h, 2.5);
            laundry_poles(g, c, rng, &face, fh, floors - 1, fh, 0.2);
            graffiti(g, c, rng, &face, 0.0, fh * 1.2, 1.0 + c.wear);
            if face.is_front && rng.random_bool(0.6) {
                let (t, y) = (rng.random_range(-face.len * 0.3..face.len * 0.3), fh + 1.0 + rng.random_range(0.0..fh * 2.0));
                hanging_sign(g, c, rng, &face, t, y, 2.2, 1.6);
            }
        }
        // Each roof is a different height: a step of parapets and junk.
        parapet(g, c, cell, h, 0.7, 0.2, s, mul(col, 0.9));
        antenna_forest(g, c, rng, &offset_poly(cell, 0.4), h, 1.0);
    }
    // Pipes and cable bundles strung across the front.
    let front = c.front_face(poly);
    for k in 0..2 {
        let y = fh * (1.6 + k as f32 * 1.3);
        let a = f.at2(front.at(-front.len * 0.45, 0.25), y);
        let b = f.at2(front.at(front.len * 0.45, 0.25), y + 0.4);
        g.part(Matte).wire(a, b, 0.6, 0.05, rgba(DARK));
    }
}

// ── capsule hotel ────────────────────────────────────────────────────────────

pub fn capsule_hotel(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = &c.poly;
    let form = Form::of(c.floors);
    let pod = 2.5; // capsule height and width
    let floors = match form {
        Form::Low => (c.floors as i32 + 1).clamp(3, 5),
        Form::Mid => (c.floors as i32 + 3).clamp(7, 12),
        Form::High => (c.floors as i32 + 4).clamp(12, 18),
    };
    // One concrete core (two on a long lot) the pods plug into.
    let (lo, hi) = bounds(poly);
    let two = form != Form::Low && hi.x - lo.x > 28.0;
    let cores: Vec<Vec<Vec2>> = if two {
        let m = (lo.x + hi.x) * 0.5;
        let w = (hi.x - lo.x) * 0.18;
        [(lo.x + m) * 0.5, (m + hi.x) * 0.5].iter().map(|u| fit_inside(c, &rect(*u, (lo.y + hi.y) * 0.5, w, ((hi.y - lo.y) * 0.25).max(2.5)))).collect()
    } else {
        vec![fit_inside(c, &scale_poly(poly, 0.45))]
    };
    let core_col = mul(pick(rng, &pal.concrete), 0.95);
    let pod_cols = [mul(pick(rng, &pal.panel), 1.2), mul([0.82, 0.82, 0.8], 1.0 - 0.3 * c.wear)];
    let h = floors as f32 * pod;
    for core in &cores {
        extrude(g, f, Concrete, core, 0.0, h + 1.5, core_col);
        // Pods around the core on every floor above the lobby.
        for face in c.faces(core) {
            let room = distance_to_boundary(face.at(0.0, 0.0), poly).min(4.2);
            let depth = (room - 0.2).min(4.0);
            if face.len < pod || depth < 2.0 {
                continue;
            }
            let n = (face.len / (pod + 0.1)).floor() as i32;
            let col = pod_cols[rng.random_range(0..2)];
            for k in 1..floors {
                for i in 0..n {
                    if coin(rng, 0.08 + 0.2 * c.wear) {
                        continue; // a pod gone
                    }
                    let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / n as f32;
                    let y = k as f32 * pod + pod * 0.5;
                    // Endpoints of the pod must stand on the lot.
                    if !c.inside_local(face.at(t - pod * 0.5, depth)) || !c.inside_local(face.at(t + pod * 0.5, depth)) {
                        continue;
                    }
                    face.bx(g, f, Panel, t, y, depth * 0.5, Vec3::new(pod - 0.12, pod - 0.12, depth), col);
                    // The round window: a dark ring and a lit or dark pane.
                    face.decal(g, f, Matte, t, y, depth + 0.02, 1.3, 1.3, DARK);
                    if coin(rng, c.lit_chance()) {
                        let col = mul(pick(rng, &c.style.window_colors), 0.8);
                        face.glaze(g, c, t, y, depth + 0.04, 0.95, 0.95, Pane::Lit(col));
                    } else {
                        face.glaze(g, c, t, y, depth + 0.04, 0.95, 0.95, Pane::Dark);
                    }
                }
            }
        }
        roof_gear(g, c, rng, core, h + 1.5, 1.0);
    }
    // Lobby at the street, a vertical sign up the core.
    let front = c.front_face(poly);
    extrude(g, f, Glass, &offset_poly(poly, 0.3), 0.0, pod, pal.glass(0));
    storefront(g, c, rng, &front, 0.0, pod);
    let cf = c.front_face(&cores[0]);
    vertical_sign(g, c, rng, &cf, 0.0, pod * 1.5, (h - pod * 2.0).max(3.0));
    extrude(g, f, Concrete, &offset_poly(poly, 0.2), pod, pod + 0.3, core_col);
}

// ── hillside terraces ────────────────────────────────────────────────────────

pub fn hillside_steps(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let form = Form::of(c.floors);
    let (lo, hi) = bounds(poly);
    let tiers = match form {
        Form::Low => 2,
        Form::Mid => 3 + (hi.y - lo.y > 24.0) as usize,
        Form::High => 5,
    };
    let per = match form {
        Form::Low => 1,
        Form::Mid => 2,
        Form::High => ((c.floors as usize + 3) / tiers).max(2),
    };
    let wall = mul(pick(rng, &pal.plaster), 1.15 - 0.3 * c.wear);
    let depth = (hi.y - lo.y) / tiers as f32;
    let mut stair_side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    for t in 0..tiers {
        // Each tier runs from its own front line to the back of the lot,
        // one step taller than the tier in front.
        let v0 = lo.y + t as f32 * depth;
        let band = clip_box(poly, lo.x, hi.x, v0, hi.y);
        if band.is_empty() {
            break;
        }
        let fl = (t + 1) * per;
        let h = fl as f32 * fh;
        let y0 = (fl - per) as f32 * fh;
        extrude(g, f, Plaster, &band, y0, h, wall);
        let front = c.front_face(&band);
        // The terrace face: glazing set back, a planted balustrade.
        let mut spec = WindowSpec::from_ctx(c);
        spec.w = 2.2;
        spec.h = fh * 0.7;
        spec.spacing = 3.0;
        spec.sill = fh * 0.15;
        spec.ac = 0.05;
        windows(g, c, rng, &front, y0, per as i32, fh, &spec);
        front.bx(g, f, Foliage, 0.0, h + 0.4, -0.5, Vec3::new(front.len * 0.9, 0.6, 0.8), mul(pal.foliage, rng.random_range(0.85..1.15)));
        front.bx(g, f, Concrete, 0.0, h + 0.15, -0.5, Vec3::new(front.len * 0.95, 0.3, 1.0), mul(wall, 0.9));
        if t > 0 && rng.random_bool(0.4) && front.len > 6.0 {
            let p = front.at(rng.random_range(-front.len * 0.3..front.len * 0.3), -2.5);
            if distance_to_boundary(p, &c.lot_poly) > 1.8 {
                tree_kind(g, f.to_world(p.x, p.y), y0, TreeKind::Round, 2.2, 1.8, pal.bark, pal.foliage);
            }
        }
        for face in c.faces(&band) {
            if !face.is_front && face.len > 3.0 && face_is_outer(&face, poly) {
                windows(g, c, rng, &face, y0, per as i32, fh, &WindowSpec::from_ctx(c));
                grime(g, c, rng, &face, y0, h, 1.0);
            }
        }
        // Outdoor stair up to this terrace, at alternate ends.
        if t > 0 {
            let u = if stair_side > 0.0 { hi.x - 0.8 } else { lo.x + 0.8 };
            let rise = per as f32 * fh;
            let p0 = Vec2::new(u, v0 - depth * 0.5);
            if c.inside_local(p0) && c.inside_local(Vec2::new(u, v0 - 0.2)) && c.inside_local(Vec2::new(u, v0 - depth * 0.85)) && distance_to_boundary(Vec2::new(u, v0 - depth * 0.85), &c.lot_poly) > 0.4 {
                let (s, col) = outdoor_steel(c, mul(pal.steel, 0.7));
                let a = f.at(u, y0 - rise + 0.1, v0 - depth * 0.85);
                let b = f.at(u, y0 + 0.1, v0 - 0.2);
                g.part(s).slab(a, b, 1.0, 0.2, rgba(col));
            }
            stair_side = -stair_side;
        }
    }
    let front = c.front_face(poly);
    entrance(g, c, rng, &front, 0.0, 2.4, fh * 0.85);
    if form == Form::High {
        roof_gear(g, c, rng, &clip_box(poly, lo.x, hi.x, lo.y + (tiers - 1) as f32 * depth, hi.y), (tiers * per) as f32 * fh, 0.6);
    }
}

// ── night market arcade ──────────────────────────────────────────────────────

pub fn night_market(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = offset_poly(&c.lot_poly, 0.6);
    let form = Form::of(c.floors);
    let (lo, hi) = bounds(&poly);
    ground(g, f, Paving, &poly, 0.04, mul(pal.paving, 0.8));
    let roof_y = 5.0;
    // Steel frame on posts over the whole lot, a ribbed roof above.
    let frame = offset_poly(&poly, 0.4);
    pilotis(g, c, &frame, 0.0, roof_y, 5.0, 0.25, Metal, mul(pal.steel, 0.6));
    let roof = offset_poly(&poly, 0.2);
    let tin = if c.wear > 0.4 { (Rust, pal.rust) } else { (Metal, mul(pick(rng, &pal.metal_paint), 0.75)) };
    if form == Form::Low {
        loft(g, f, tin.0, &roof, &scale_poly(&roof, 0.9), roof_y, roof_y + 1.2, tin.1);
    } else {
        // Upper floors over the market: a block bristling with signs.
        let fl = (c.floors as i32 - 1).clamp(2, 6);
        let fh = c.fh();
        let block = fit_inside(c, &offset_poly(&poly, 1.2));
        let (s, col) = patchwork(c, rng);
        extrude(g, f, s, &block, roof_y, roof_y + fl as f32 * fh, col);
        for face in c.faces(&block) {
            if face.len > 3.0 {
                dense_windows(g, c, rng, &face, roof_y, fl, fh, 2.0, 0.2, 0.02);
            }
            if face.is_front {
                for k in 0..fl.min(3) {
                    let t = rng.random_range(-face.len * 0.25..face.len * 0.25);
                    sign_board(g, c, rng, &face, t, roof_y + k as f32 * fh + fh * 0.5, (face.len * 0.4).min(7.0), 0.9);
                }
                vertical_sign(g, c, rng, &face, face.len * 0.42, roof_y + 0.5, fl as f32 * fh - 1.0);
            }
        }
        extrude(g, f, tin.0, &roof, roof_y - 0.3, roof_y, tin.1);
        antenna_forest(g, c, rng, &offset_poly(&block, 0.5), roof_y + fl as f32 * fh, 0.6);
    }
    // Entrance arch with a big lit name board.
    let front = c.front_face(&poly);
    front.bx(g, f, Matte, 0.0, roof_y + 0.2, -0.3, Vec3::new(front.len * 0.7, 1.4, 0.3), DARK);
    let sc = pick(rng, &pal.sign_colors);
    let letters = ((front.len * 0.6 / 1.2) as i32).clamp(2, 10);
    for i in 0..letters {
        let t = -front.len * 0.3 + (i as f32 + 0.5) * front.len * 0.6 / letters as f32;
        front.decal(g, f, Glow, t, roof_y + 0.2, -0.13, 0.8, 1.0, mul(sc, 1.1 - 0.5 * c.wear));
    }
    // Two rows of stalls down a central aisle, lanterns and string lights.
    let aisle_u = (lo.x + hi.x) * 0.5;
    let rows = [lo.x + 2.0, hi.x - 2.0];
    let mut v = lo.y + 3.0;
    while v < hi.y - 2.0 {
        for (ri, u) in rows.iter().enumerate() {
            let p = Vec2::new(*u, v);
            if !(c.inside_local(p + Vec2::new(-1.6, -1.2)) && c.inside_local(p + Vec2::new(1.6, 1.2)) && c.inside_local(p + Vec2::new(1.6, -1.2)) && c.inside_local(p + Vec2::new(-1.6, 1.2))) {
                continue;
            }
            let face = Face::from_edge(Vec2::new(*u, v - 1.4), Vec2::new(*u, v + 1.4));
            let face = if ri == 0 { face } else { face.reversed() };
            // Counter, canopy, goods and a lit strip.
            let tc = mul(pick(rng, &pal.tarp_colors), 0.9);
            face.bx(g, f, Wood, 0.0, 0.5, 0.0, Vec3::new(2.6, 1.0, 1.4), pal.wood);
            face.bx(g, f, Tarp, 0.0, 2.6, 0.2, Vec3::new(3.0, 0.1, 2.0), tc);
            face.bx(g, f, Tarp, 0.0, 1.15, 0.0, Vec3::new(2.2, 0.3, 1.0), pick(rng, &pal.awning_colors));
            face.decal(g, f, Glow, 0.0, 2.35, 1.21, 2.4, 0.3, mul(pick(rng, &pal.sign_colors), 0.9 - 0.4 * c.wear));
            if rng.random_bool(0.4) {
                // Steam from a cooking pot.
                face.bx(g, f, Metal, 0.6, 1.25, 0.2, Vec3::new(0.5, 0.4, 0.5), mul(pal.steel, 0.8));
                face.bx(g, f, Tarp, 0.6, 1.9, 0.2, Vec3::new(0.4, 0.8, 0.4), [0.75, 0.75, 0.78]);
            }
        }
        // Lantern pair over the aisle.
        let lc = if rng.random_bool(0.6) { [2.0, 0.35, 0.2] } else { pal.lamp };
        let lp = Vec2::new(aisle_u + rng.random_range(-0.8..0.8), v);
        if c.inside_local(lp) && distance_to_boundary(lp, &c.lot_poly) > 0.5 {
            f.bx(g, Glow, lp.x, roof_y - 1.0, lp.y, Vec3::new(0.5, 0.7, 0.5), lc);
        }
        v += 3.4;
    }
    let n_lines = ((hi.y - lo.y) / 6.0) as i32;
    for k in 0..n_lines.max(1) {
        let vv = lo.y + 2.0 + k as f32 * 6.0;
        if c.inside_local(Vec2::new(lo.x + 0.8, vv)) && c.inside_local(Vec2::new(hi.x - 0.8, vv + 2.0)) {
            string_lights(g, f, Vec3::new(lo.x + 0.8, roof_y - 0.4, vv), Vec3::new(hi.x - 0.8, roof_y - 0.4, vv + 2.0), 9, *[pal.lamp, [1.6, 0.5, 0.9]].choose(rng).unwrap());
        }
    }
    // Plastic tables and stools in the aisle.
    for _ in 0..((hi.y - lo.y) / 5.0) as i32 {
        let p = Vec2::new(aisle_u + rng.random_range(-0.6..0.6), rng.random_range(lo.y + 2.0..hi.y - 1.5));
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
            f.bx(g, Matte, p.x, 0.38, p.y, Vec3::new(0.7, 0.06, 0.7), mul(pick(rng, &pal.awning_colors), 1.1));
            f.bx(g, Matte, p.x, 0.19, p.y, Vec3::new(0.08, 0.38, 0.08), CHARCOAL);
            for s in [-0.6f32, 0.6] {
                f.bx(g, Matte, p.x + s, 0.22, p.y, Vec3::new(0.3, 0.44, 0.3), [0.7, 0.15, 0.12]);
            }
        }
    }
    for face in c.faces(&poly) {
        if !face.is_front && face.len > 3.0 {
            graffiti(g, c, rng, &face, 0.0, 2.5, 0.6 + c.wear);
        }
    }
}

// ── neon tower ───────────────────────────────────────────────────────────────

pub fn neon_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let form = Form::of(c.floors);
    let (lo, hi) = bounds(&c.poly);
    // Narrow: a karaoke tower takes the front of the lot, a car lift behind.
    let td = ((hi.y - lo.y) * 0.7).clamp(8.0, 22.0);
    let body = clip_box(&c.poly, lo.x, hi.x, lo.y, lo.y + td);
    let body = if body.is_empty() { c.poly.clone() } else { body };
    let floors = match form {
        Form::Low => (c.floors as i32).clamp(3, 4),
        Form::Mid => (c.floors as i32 + 2).max(6),
        Form::High => (c.floors as i32 + 3).clamp(10, 24),
    };
    let h = floors as f32 * fh;
    let dark = mul(pick(rng, &pal.panel), 0.45);
    extrude(g, f, Panel, &body, 0.0, h, dark);
    let front = c.front_face(&body);
    storefront(g, c, rng, &front, 0.0, fh);
    // The front is signs from the first floor up: stacked boards of
    // different widths, a screen, and vertical blades on the corners.
    let mut y = fh + 0.6;
    let mut screen = false;
    while y < h - 1.5 {
        let bh = rng.random_range(1.0f32..2.2).min(h - y - 0.5);
        if !screen && bh > 1.6 && rng.random_bool(0.35) && front.len > 6.0 {
            led_screen(g, c, rng, &front, 0.0, y + bh * 0.5 + 0.6, front.len * 0.7, bh + 1.0);
            y += bh + 1.6;
            screen = true;
            continue;
        }
        let w = front.len * rng.random_range(0.45..0.85);
        let t = rng.random_range(-(front.len - w) * 0.5..=(front.len - w) * 0.5);
        sign_board(g, c, rng, &front, t, y + bh * 0.5, w, bh);
        y += bh + rng.random_range(0.3..1.0);
    }
    for side in [-1.0f32, 1.0] {
        vertical_sign(g, c, rng, &front, side * (front.len * 0.5 - 0.7), fh + 0.5, (h - fh - 1.0).max(2.0));
    }
    for face in c.faces(&body) {
        if face.is_front || face.len < 3.0 {
            continue;
        }
        band_windows(g, c, rng, &face, fh, floors - 1, fh);
        if face_is_outer(&face, &c.poly) && rng.random_bool(0.5) {
            hanging_sign(g, c, rng, &face, -face.len * 0.3, fh * 2.0, 2.0, 3.0);
        }
        grime(g, c, rng, &face, 0.0, h, 1.5);
    }
    // Crown: a lit billboard frame, or a ring of light when tall.
    if form == Form::High {
        let ring = offset_poly(&body, 0.3);
        let lit = [mul(pick(rng, &pal.sign_colors), 1.0), mul(pick(rng, &pal.sign_colors), 0.8)];
        crown_frame(g, c, &ring, h, 5.0, 0.25, Matte, CHARCOAL, &[(5.0, 0.35, lit[0]), (1.0, 0.2, lit[1])]);
    } else {
        let p = poly_center(&body);
        let bw = (front.len * 0.8).min(9.0);
        if c.inside_local(p + Vec2::new(-bw * 0.5, 0.0)) && c.inside_local(p + Vec2::new(bw * 0.5, 0.0)) {
            billboard(g, c, rng, p, 0.0, h, bw, 3.0);
        }
    }
    roof_gear(g, c, rng, &body, h, 0.5);
}
