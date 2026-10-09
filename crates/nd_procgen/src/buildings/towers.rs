//! Towers with a height-dependent form: Hong Kong pencil towers, Shanghai
//! twin slabs, Akira megastructures, Ghost-in-the-Shell glass terraces and
//! the glass castle. Each changes shape with its floors: low sites get the
//! walk-up or pavilion version, middle ones a podium and slab, tall ones a
//! tower with setbacks and a crown.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::seq::IndexedRandom;
use rand::Rng;

/// Pitched roof over a footprint with its ridge across `axis`.
fn pitched_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, ridge: f32, axis: Vec2, s: Surface, color: [f32; 3]) {
    let centre = poly_center(poly);
    let top: Vec<Vec2> = poly
        .iter()
        .map(|p| {
            let d = *p - centre;
            let along = d.dot(axis);
            clamp_into(poly, centre + d - axis * along * 0.94)
        })
        .collect();
    loft(g, &c.frame, s, &offset_poly(poly, -0.35), &top, y, y + ridge, color);
}

// ── pencil tower ─────────────────────────────────────────────────────────────

pub fn pencil_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let form = Form::of(c.floors);
    let poly = &c.poly;
    let front = c.front_face(poly);
    let wall = mul(*[pick(rng, &pal.plaster), pick(rng, &pal.concrete), pick(rng, &pal.panel)].choose(rng).unwrap(), 1.0 - 0.25 * c.wear);
    let wall_s = *[Plaster, Concrete, Panel].choose(rng).unwrap();
    let podium_floors = match form {
        Form::Low => 1,
        Form::Mid => 2,
        Form::High => 3,
    };
    let podium_h = podium_floors as f32 * fh;
    // Podium over the whole footprint: shops at the street, signs over them.
    extrude(g, f, Concrete, poly, 0.0, podium_h, mul(pick(rng, &pal.concrete), 0.85));
    storefront(g, c, rng, &front, 0.0, fh);
    if podium_floors > 1 {
        band_windows(g, c, rng, &front, fh, podium_floors - 1, fh);
    }
    for face in c.faces(poly) {
        if !face.is_front && face.len > 3.0 {
            grime(g, c, rng, &face, 0.0, podium_h, 2.0);
        }
    }
    let n_signs = ((front.len / 5.0) as i32).clamp(1, 4);
    for i in 0..n_signs {
        let t = -front.len * 0.4 + (i as f32 + 0.5) * front.len * 0.8 / n_signs as f32;
        let (y, reach, h) = (fh + 0.6 + rng.random_range(0.0..(podium_h + 1.0)), rng.random_range(1.6..2.8), rng.random_range(1.2..2.2));
        hanging_sign(g, c, rng, &front, t, y, reach, h);
    }

    // The tower: a slim slab standing on the front of the podium. On a low
    // site the whole footprint is a narrow walk-up instead.
    let (lo, hi) = bounds(poly);
    let tower = if form == Form::Low {
        poly.clone()
    } else {
        let tw = ((hi.x - lo.x) * 0.6).clamp(5.0, 12.0);
        let td = ((hi.y - lo.y) * 0.8).clamp(6.0, 20.0);
        let cu = (lo.x + hi.x) * 0.5 + rng.random_range(-1.0..1.0) * ((hi.x - lo.x) - tw).max(0.0) * 0.4;
        let t = clip_box(poly, cu - tw * 0.5, cu + tw * 0.5, lo.y, lo.y + td);
        if t.is_empty() { fit_inside(c, &scale_poly(poly, 0.6)) } else { t }
    };
    let floors = match form {
        Form::Low => (c.floors as i32).clamp(3, 4) - podium_floors,
        Form::Mid => (c.floors as i32 + 3 - podium_floors).max(5),
        Form::High => (c.floors as i32 + 6 - podium_floors).clamp(8, 34),
    };
    let set_floors = if form == Form::High { 3 } else { 0 };
    let shaft = floors - set_floors;
    let y0 = podium_h;
    extrude(g, f, wall_s, &tower, y0, y0 + shaft as f32 * fh, wall);
    let faces = c.faces(&tower);
    let cages = if c.tier == nd_sim::Tier::Low { 0.08 } else { 0.02 };
    for face in &faces {
        if face.len < 2.5 {
            continue;
        }
        dense_windows(g, c, rng, face, y0, shaft, fh, 1.9, 0.18 + 0.2 * c.style.clutter, cages);
        if !face.is_front {
            laundry_poles(g, c, rng, face, y0, shaft, fh, 0.12 + 0.3 * c.wear);
        }
        grime(g, c, rng, face, y0, y0 + shaft as f32 * fh, 1.5);
    }
    // Stacked bay windows up the front: the pencil tower's ribs.
    let tf = c.front_face(&tower);
    if form != Form::Low && tf.len > 5.0 {
        let room = c.reach_out(&tf);
        let proud = room.clamp(0.0, 0.6);
        for side in [-0.28f32, 0.28] {
            let t = side * tf.len;
            tf.bx(g, f, wall_s, t, y0 + shaft as f32 * fh * 0.5, proud * 0.5, Vec3::new(2.2, shaft as f32 * fh, proud.max(0.05) + 0.02), mul(wall, 1.12));
            for k in 0..shaft {
                tf.glaze(g, c, t, y0 + k as f32 * fh + fh * 0.55, proud + 0.04, 1.8, fh * 0.5, Pane::Dark);
            }
        }
    }
    let mut top = y0 + shaft as f32 * fh;
    let mut roof = tower.clone();
    if set_floors > 0 {
        // Setback floors and a crown frame with a lit outline.
        let upper = fit_inside(c, &scale_poly(&tower, 0.8));
        extrude(g, f, wall_s, &upper, top, top + set_floors as f32 * fh, mul(wall, 1.08));
        for face in c.faces(&upper) {
            if face.len > 2.5 {
                dense_windows(g, c, rng, &face, top, set_floors, fh, 2.0, 0.0, 0.0);
            }
        }
        roof_gear(g, c, rng, &tower, top, 0.6);
        top += set_floors as f32 * fh;
        roof = upper;
        let crown = offset_poly(&roof, -0.2);
        for p in &crown {
            f.bx(g, Panel, p.x, top + 2.0, p.y, Vec3::new(0.3, 4.0, 0.3), mul(pal.steel, 0.8));
        }
        glow_outline(g, c, &crown, top + 4.0, 0.25, mul(pick(rng, &pal.sign_colors), 0.9));
        glow_outline(g, c, &crown, top + 2.0, 0.12, mul(pick(rng, &pal.sign_colors), 0.7));
    } else {
        parapet(g, c, &tower, top, 0.9, 0.25, wall_s, mul(wall, 0.9));
    }
    antenna_forest(g, c, rng, &offset_poly(&roof, 0.6), top, if c.tier == nd_sim::Tier::Low { 0.8 } else { 0.35 });
    if poly_inside_poly(&rect(poly_center(&roof).x, poly_center(&roof).y, 1.3, 1.3), &roof) {
        water_tank(g, f, poly_center(&roof), top, 1.1, 2.2, Metal, mul(pal.steel, 0.8));
    }
    // Back of the podium roof: a terrace with tanks and plant.
    if form != Form::Low {
        roof_gear(g, c, rng, &offset_poly(poly, 0.5), podium_h, 0.4);
    }
}

// ── Shanghai twin slabs ──────────────────────────────────────────────────────

pub fn twin_slab(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let form = Form::of(c.floors);
    let wall = mul(pick(rng, &pal.plaster), 1.05 - 0.25 * c.wear);
    let trim = mul(c.style.trim_color, 1.2);

    if form == Form::Low {
        // Lane houses: two terraced rows down a lane, a stone gate at the street.
        let mid = (lo.x + hi.x) * 0.5;
        let lane = 1.6;
        let rows = [clip_box(poly, lo.x, mid - lane, lo.y, hi.y), clip_box(poly, mid + lane, hi.x, lo.y, hi.y)];
        let floors = (c.floors as i32).clamp(2, 3);
        let brick = mul(pick(rng, &pal.brick), 0.9 - 0.2 * c.wear);
        for row in rows.iter().filter(|r| !r.is_empty()) {
            let (rlo, rhi) = bounds(row);
            let units = (((rhi.y - rlo.y) / 6.0).floor() as usize).clamp(1, 5);
            for cell in split_cells(rng, row, 1, units) {
                let h = floors as f32 * fh;
                extrude(g, f, Brick, &cell, 0.0, h, brick);
                cornice(g, c, &cell, h - 0.2, 0.3, 0.15, Concrete, trim);
                pitched_roof(g, c, &cell, h, 1.8, Vec2::new(0.0, 1.0), Brick, mul(pal.rust, 0.7));
                for face in c.faces(&cell) {
                    if face.len > 3.0 && face_is_outer(&face, poly) || (face.len > 3.0 && (face.mid().x - mid).abs() < lane + 0.6) {
                        let mut spec = WindowSpec::from_ctx(c);
                        spec.spacing = 2.4;
                        spec.ac = 0.2;
                        windows(g, c, rng, &face, 0.0, floors, fh, &spec);
                    }
                }
            }
        }
        // Shikumen gate over the lane mouth.
        let front_v = lo.y + 0.4;
        if c.inside_local(Vec2::new(mid - lane - 0.4, front_v)) && c.inside_local(Vec2::new(mid + lane + 0.4, front_v)) {
            let stone = mul(pal.stone, 1.2);
            for s in [-1.0f32, 1.0] {
                f.bx(g, Stone, mid + s * (lane + 0.2), 2.0, front_v, Vec3::new(0.5, 4.0, 0.6), stone);
            }
            f.bx(g, Stone, mid, 4.3, front_v, Vec3::new(lane * 2.0 + 1.2, 0.7, 0.7), stone);
            let face = Face::from_edge(Vec2::new(mid + lane, front_v - 0.36), Vec2::new(mid - lane, front_v - 0.36));
            face.decal(g, f, Glow, 0.0, 4.3, 0.02, lane * 1.6, 0.4, mul(pick(rng, &pal.sign_colors), 0.8));
            let far = Vec2::new(mid + lane * 0.5, (hi.y - 1.0).max(front_v + 2.0));
            let far = if c.inside_local(far) { far } else { Vec2::new(mid + lane * 0.5, front_v + 6.0) };
            if c.inside_local(far) {
                string_lights(g, f, Vec3::new(mid - lane * 0.5, 3.6, front_v + 1.0), Vec3::new(far.x, 3.6, far.y), 8, pal.lamp);
            }
        }
        return;
    }

    // Two slabs side by side (or one behind the other on a narrow lot).
    let side_by_side = hi.x - lo.x >= 20.0;
    let gap = rng.random_range(3.5..6.0);
    let slabs: Vec<Vec<Vec2>> = if side_by_side {
        let mid = (lo.x + hi.x) * 0.5;
        let d = ((hi.y - lo.y) * 0.85).min(18.0);
        [clip_box(poly, lo.x, mid - gap * 0.5, lo.y, lo.y + d), clip_box(poly, mid + gap * 0.5, hi.x, lo.y, lo.y + d)].into_iter().filter(|p| !p.is_empty()).collect()
    } else {
        let mid = (lo.y + hi.y) * 0.5;
        [clip_box(poly, lo.x, hi.x, lo.y, mid - gap * 0.5), clip_box(poly, lo.x, hi.x, mid + gap * 0.5, hi.y)].into_iter().filter(|p| !p.is_empty()).collect()
    };
    let slabs = if slabs.is_empty() { vec![fit_inside(c, &scale_poly(poly, 0.8))] } else { slabs };
    let floors = match form {
        Form::Mid => (c.floors as i32 + 2).max(7),
        _ => (c.floors as i32 + 5).clamp(12, 36),
    };
    let crown_kind = rng.random_range(0..3);
    let crown_col = mul(pick(rng, &pal.sign_colors), 0.9);
    let mut tops: Vec<(Vec2, f32)> = Vec::new();
    for (i, slab) in slabs.iter().enumerate() {
        // The second slab is a few floors shorter, Shanghai style.
        let fl = if i == 0 { floors } else { (floors - rng.random_range(0..=3)).max(5) };
        let h = fl as f32 * fh;
        extrude(g, f, Plaster, slab, 0.0, h, wall);
        for face in c.faces(slab) {
            if face.len < 3.0 {
                continue;
            }
            band_windows(g, c, rng, &face, 0.0, fl, fh);
            // Balcony bands on the long faces, one slab per floor.
            if face.len > 8.0 {
                let room = c.reach_out(&face).clamp(0.0, 1.1);
                if room > 0.3 {
                    for k in 1..fl {
                        face.bx(g, f, Concrete, 0.0, k as f32 * fh, room * 0.5, Vec3::new(face.len * 0.9, 0.18, room), trim);
                        // The railing 2 cm proud of the slab's edge, not in its face.
                        face.decal(g, f, Matte, 0.0, k as f32 * fh + 0.5, room + 0.02, face.len * 0.9, 0.85, mul(pal.steel, 0.5));
                    }
                }
            }
            grime(g, c, rng, &face, 0.0, h, 1.2);
        }
        let centre = poly_center(slab);
        let r = inradius(slab);
        match crown_kind {
            0 => {
                // Flying-saucer cap on a short drum, ringed with light.
                let cw = f.to_world(centre.x, centre.y);
                let cap_r = (r * 1.15).min(r + 2.0);
                g.part(Concrete).frustum(&MeshData::ngon(cw, r * 0.45, 10, 0.0, h), &MeshData::ngon(cw, r * 0.45, 10, 0.0, h + 3.0), rgba(trim));
                g.part(Panel).frustum(&MeshData::ngon(cw, cap_r * 0.7, 12, 0.0, h + 3.0), &MeshData::ngon(cw, cap_r, 12, 0.0, h + 3.8), rgba(mul(pal.steel, 0.9)));
                g.part(Panel).frustum(&MeshData::ngon(cw, cap_r, 12, 0.0, h + 3.8), &MeshData::ngon(cw, cap_r * 0.5, 12, 0.0, h + 4.6), rgba(mul(pal.steel, 0.8)));
                g.part(Glow).frustum(&MeshData::ngon(cw, cap_r + 0.05, 12, 0.0, h + 3.75), &MeshData::ngon(cw, cap_r + 0.05, 12, 0.0, h + 3.85), rgba(crown_col));
                tops.push((centre, h));
            }
            1 => {
                // Open crown frame: posts and two lit rings.
                let ring = offset_poly(slab, 0.4);
                for p in &ring {
                    f.bx(g, Panel, p.x, h + 2.5, p.y, Vec3::new(0.35, 5.0, 0.35), trim);
                }
                glow_outline(g, c, &ring, h + 5.0, 0.3, crown_col);
                glow_outline(g, c, &ring, h + 2.5, 0.15, mul(crown_col, 0.7));
                roof_gear(g, c, rng, slab, h, 0.5);
                tops.push((centre, h));
            }
            _ => {
                // Stepped pavilion hat: two shrinking tiers with flared eaves.
                let t1 = fit_inside(c, &scale_poly(slab, 0.7));
                extrude(g, f, Plaster, &t1, h, h + 3.0, mul(wall, 1.1));
                cornice(g, c, &t1, h + 3.0, 0.3, 0.8, Panel, mul(pal.rust, 0.8));
                let t2 = scale_poly(&t1, 0.6);
                loft(g, f, Panel, &offset_poly(&t2, -0.6), &scale_poly(&t2, 0.25), h + 3.2, h + 6.0, mul(pal.rust, 0.7));
                glow_outline(g, c, &offset_poly(&t1, -0.6), h + 3.1, 0.12, crown_col);
                tops.push((centre, h));
            }
        }
        let fr = c.front_face(slab);
        if fr.len > 4.0 {
            entrance(g, c, rng, &fr, 0.0, 2.6, 3.0);
        }
    }
    // A glazed link between the slabs, two-thirds up when tall.
    if tops.len() == 2 && form == Form::High {
        let y = tops[0].1.min(tops[1].1) * 0.66;
        let (a, b) = (tops[0].0, tops[1].0);
        sky_bridge(g, c, a, b, (y / fh).floor() * fh, 3.0, fh);
    }
    for k in 0..2 {
        let front = c.front_face(poly);
        lamp_post_in(g, c, front.at((k as f32 - 0.5) * front.len * 0.7, -1.0), 3.2, pal.lamp);
    }
}

// ── Akira megastructure ──────────────────────────────────────────────────────

pub fn megastructure(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let form = Form::of(c.floors);
    let dark = mul(pick(rng, &pal.concrete), 0.55);
    let red = [1.8, 0.25, 0.12];
    let accent = if rng.random_bool(0.5) { red } else { mul(pick(rng, &pal.led_colors), 1.0) };

    if form != Form::High {
        // Stepped ziggurat with a drum rising out of its top terrace.
        let tiers = if form == Form::Low { 2 } else { 3 };
        let per = ((c.floors as i32 + 2) / tiers).max(2);
        let mut p = poly.clone();
        let mut y = 0.0;
        let step = (inradius(poly) * 0.18).clamp(1.2, 3.5);
        for t in 0..tiers {
            let h = per as f32 * fh;
            extrude(g, f, Concrete, &p, y, y + h, mul(dark, 1.0 + 0.08 * t as f32));
            for face in c.faces(&p) {
                if face.len > 3.0 {
                    band_windows(g, c, rng, &face, y, per, fh);
                }
            }
            glow_outline(g, c, &offset_poly(&p, -0.05), y + h - 0.3, 0.18, accent);
            y += h;
            let next = offset_poly(&p, step);
            if next.len() < 3 || inradius(&next) < 3.0 {
                break;
            }
            p = next;
        }
        let r = (inradius(&p) * 0.7).max(1.5);
        let cw = f.to_world(poly_center(&p).x, poly_center(&p).y);
        let dh = per as f32 * fh * 1.5;
        g.part(Concrete).frustum(&MeshData::ngon(cw, r, 14, 0.0, y), &MeshData::ngon(cw, r, 14, 0.0, y + dh), rgba(mul(dark, 1.2)));
        for k in 0..3 {
            let yy = y + dh * (0.25 + 0.3 * k as f32);
            g.part(Glow).frustum(&MeshData::ngon(cw, r + 0.05, 14, 0.0, yy), &MeshData::ngon(cw, r + 0.05, 14, 0.0, yy + 0.3), rgba(mul(c.style.window_color(k), 0.9)));
        }
        g.part(Panel).frustum(&MeshData::ngon(cw, r + 0.6, 14, 0.0, y + dh), &MeshData::ngon(cw, r * 0.8, 14, 0.0, y + dh + 1.0), rgba(mul(pal.steel, 0.6)));
        let front = c.front_face(poly);
        entrance(g, c, rng, &front, 0.0, 4.0, fh * 1.2);
        led_screen(g, c, rng, &front, front.len * 0.25, fh * 2.0, (front.len * 0.3).min(8.0), 3.0);
        return;
    }

    // Heavy base over the whole lot.
    let base_floors = 3;
    let base_h = base_floors as f32 * fh;
    extrude(g, f, Concrete, poly, 0.0, base_h, dark);
    for face in c.faces(poly) {
        if face.len > 3.0 {
            band_windows(g, c, rng, &face, 0.0, base_floors, fh);
        }
    }
    glow_outline(g, c, &offset_poly(poly, -0.05), base_h - 0.4, 0.25, accent);
    // Towers rise from cells of the base.
    let (lo, hi) = bounds(poly);
    let (nu, nv) = if hi.x - lo.x > hi.y - lo.y { (2, 1 + ((hi.y - lo.y) > 26.0) as usize) } else { (1 + ((hi.x - lo.x) > 26.0) as usize, 2) };
    let mut cells: Vec<Vec<Vec2>> = split_cells(rng, poly, nu, nv).into_iter().map(|p| fit_inside(c, &scale_poly(&p, 0.72))).filter(|p| p.len() >= 3 && inradius(p) > 2.0).collect();
    if cells.is_empty() {
        cells.push(fit_inside(c, &scale_poly(poly, 0.6)));
    }
    let total = (c.floors as i32 + 4).clamp(14, 40) - base_floors;
    let mut tops: Vec<(Vec2, f32)> = Vec::new();
    for (i, cell) in cells.iter().enumerate() {
        let fl = if i == 0 { total } else { (total as f32 * rng.random_range(0.55..0.9)) as i32 };
        let h = fl as f32 * fh;
        let y0 = base_h;
        let centre = poly_center(cell);
        match (i + rng.random_range(0..3)) % 3 {
            0 => {
                // Slab with a red stripe up its spine.
                extrude(g, f, Panel, cell, y0, y0 + h, mul(dark, 1.3));
                curtain_lights(g, c, rng, cell, y0, fl, fh);
                let fr = c.front_face(cell);
                fr.bx(g, f, Glow, 0.0, y0 + h * 0.5, 0.05, Vec3::new(0.5, h, 0.1), accent);
                // A wide flat cap that overhangs: no point on top.
                extrude(g, f, Concrete, &offset_poly(cell, -0.6), y0 + h, y0 + h + 1.4, mul(dark, 0.9));
                glow_outline(g, c, &offset_poly(cell, -0.6), y0 + h + 0.7, 0.15, accent);
                roof_gear(g, c, rng, cell, y0 + h + 1.4, 0.5);
                tops.push((centre, y0 + h));
            }
            1 => {
                // Drum tower with lit bands, a stepped cap.
                let r = inradius(cell).max(2.0);
                let cw = f.to_world(centre.x, centre.y);
                g.part(Concrete).frustum(&MeshData::ngon(cw, r, 16, 0.0, y0), &MeshData::ngon(cw, r * 0.92, 16, 0.0, y0 + h), rgba(mul(dark, 1.15)));
                let mut k = 1;
                while k < fl {
                    let yy = y0 + k as f32 * fh + fh * 0.4;
                    let rr = r - 0.08 * r * (k as f32 / fl as f32);
                    if coin(rng, c.lit_chance() * 1.4) {
                        let col = rgba(mul(pick(rng, &c.style.window_colors), 0.8));
                        window_light(g, c, |g| g.part(Glow).frustum(&MeshData::ngon(cw, rr + 0.04, 16, 0.0, yy), &MeshData::ngon(cw, rr + 0.04, 16, 0.0, yy + fh * 0.35), col));
                    }
                    k += 2;
                }
                g.part(Concrete).frustum(&MeshData::ngon(cw, r * 0.98, 16, 0.0, y0 + h), &MeshData::ngon(cw, r * 0.7, 16, 0.0, y0 + h + 2.0), rgba(mul(dark, 0.9)));
                g.part(Glow).frustum(&MeshData::ngon(cw, r * 0.7, 16, 0.0, y0 + h + 2.0), &MeshData::ngon(cw, r * 0.7, 16, 0.0, y0 + h + 2.4), rgba(accent));
                tops.push((centre, y0 + h));
            }
            _ => {
                // Stepped tower: two setbacks, lit window bands.
                let mut p = cell.clone();
                let mut y = y0;
                let parts = [0.5f32, 0.3, 0.2];
                for (k, share) in parts.iter().enumerate() {
                    let pf = ((fl as f32 * share).round() as i32).max(1);
                    let ph = pf as f32 * fh;
                    extrude(g, f, Panel, &p, y, y + ph, mul(dark, 1.2 + 0.05 * k as f32));
                    for face in c.faces(&p) {
                        if face.len > 3.0 {
                            band_windows(g, c, rng, &face, y, pf, fh);
                        }
                    }
                    glow_outline(g, c, &p, y + ph, 0.12, accent);
                    y += ph;
                    p = fit_inside(c, &scale_poly(&p, 0.78));
                }
                tops.push((centre, y));
            }
        }
    }
    // Sky bridges between the towers at a few heights.
    for i in 1..tops.len() {
        let (a, ha) = tops[i - 1];
        let (b, hb) = tops[i];
        let top = ha.min(hb) - fh * 2.0;
        let n = ((top - base_h) / (fh * 6.0)).floor().clamp(1.0, 3.0) as i32;
        for k in 0..n {
            let y = base_h + (top - base_h) * (k as f32 + 1.0) / (n as f32 + 1.0);
            sky_bridge(g, c, a, b, (y / fh).floor() * fh, 3.2, fh * 0.9);
        }
    }
    let front = c.front_face(poly);
    entrance(g, c, rng, &front, 0.0, 5.0, fh * 1.4);
    led_screen(g, c, rng, &front, -front.len * 0.3, base_h * 0.6, (front.len * 0.3).min(10.0), base_h * 0.4);
    bollards(g, c, &front, 6, 1.6);
}

// ── Ghost-in-the-Shell glass terraces ────────────────────────────────────────

pub fn glass_terraces(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let poly = &c.poly;
    let form = Form::of(c.floors);
    let white = mul(pick(rng, &pal.concrete), 1.35);
    let glass = pick(rng, &pal.glass);
    let edge = mul(pick(rng, &pal.led_colors), 0.8);
    let total = match form {
        Form::Low => (c.floors as i32).clamp(2, 4),
        Form::Mid => (c.floors as i32 + 2).max(6),
        Form::High => (c.floors as i32 + 4).clamp(12, 40),
    };
    let block_floors = if form == Form::Low { 1 } else { rng.random_range(2..=4) };
    let mut y = 0.0;
    let mut built = 0;
    let mut prev = fit_inside(c, &scale_poly(poly, 0.9));
    let mut screen_done = false;
    let mut k = 0;
    while built < total {
        let fl = block_floors.min(total - built);
        let h = fl as f32 * fh;
        // Each block shifted and turned against the one below; the slab
        // under it is the union of both, so the overhang becomes a terrace.
        let scale = if form == Form::High { 0.85 - 0.25 * (built as f32 / total as f32) } else { rng.random_range(0.65..0.9) };
        let shift = Vec2::new(rng.random_range(-2.5..2.5), rng.random_range(-2.0..2.0));
        let turn = rng.random_range(-0.12..0.12);
        let block = fit_inside(c, &rotate_poly(&shift_poly(&scale_poly(poly, scale), shift), turn));
        if block.len() < 3 {
            break;
        }
        let plate = if k == 0 { offset_poly(&block, -0.6) } else { prev.clone() };
        let plate = fit_inside(c, &plate);
        extrude(g, f, Concrete, &plate, y, y + 0.4, white);
        glow_outline(g, c, &plate, y + 0.38, 0.08, edge);
        // Planters along the terrace edge where the plate reaches beyond the block.
        for face in c.faces(&plate) {
            if face.len > 3.0 && distance_to_boundary(face.mid(), &block) > 1.2 {
                face.bx(g, f, Foliage, 0.0, y + 0.75, -0.5, Vec3::new(face.len * 0.85, 0.7, 0.7), mul(pal.foliage, rng.random_range(0.8..1.2)));
                face.decal(g, f, Matte, 0.0, y + 0.95, 0.02, face.len * 0.95, 1.0, mul(pal.glass_dark, 1.5));
            }
        }
        extrude(g, f, Glass, &block, y + 0.4, y + h, glass);
        curtain_lights(g, c, rng, &block, y + 0.4, fl, fh);
        // Thin vertical mullions in white on the long faces.
        for face in c.faces(&block) {
            if face.len > 6.0 && rng.random_bool(0.5) {
                let n = (face.len / 2.0) as i32;
                for i in 0..=n {
                    let t = -face.len * 0.5 + i as f32 * face.len / n.max(1) as f32;
                    face.decal(g, f, Matte, t, y + 0.4 + (h - 0.4) * 0.5, 0.06, 0.12, h - 0.4, white);
                }
            }
            if !screen_done && built >= 2 && face.len > 8.0 && rng.random_bool(0.4) {
                led_screen(g, c, rng, &face, 0.0, y + h * 0.5, (face.len * 0.5).min(9.0), (h * 0.7).min(6.0));
                screen_done = true;
            }
        }
        prev = block;
        y += h;
        built += fl;
        k += 1;
    }
    // Roof: a slab, a garden and a pergola frame.
    extrude(g, f, Concrete, &prev, y, y + 0.4, white);
    glow_outline(g, c, &prev, y + 0.4, 0.1, edge);
    let roof = offset_poly(&prev, 1.0);
    if roof.len() >= 3 && inradius(&roof) > 2.5 {
        let pc = poly_center(&roof);
        tree_kind(g, f.to_world(pc.x, pc.y), y + 0.4, TreeKind::Round, 2.4, 2.4, pal.bark, pal.foliage);
        for p in &roof {
            f.bx(g, Matte, p.x, y + 1.9, p.y, Vec3::new(0.15, 3.0, 0.15), white);
        }
        glow_outline(g, c, &roof, y + 3.4, 0.08, mul(edge, 0.6));
    }
    let front = c.front_face(poly);
    entrance(g, c, rng, &front, 0.0, 3.5, fh * 0.9);
    for k in 0..3 {
        lamp_post_in(g, c, front.at(-front.len * 0.35 + k as f32 * front.len * 0.35, -1.2), 3.2, pal.lamp);
    }
}

// ── glass castle ─────────────────────────────────────────────────────────────
//
// What used to be a stone keep behind crenellations is now a cluster of
// faceted glass prisms, like a geode broken open: several crystals of
// different heights leaning together, each with a cut, faceted crown. A
// glass fence and a lit gatehouse at the street, a reflecting pool inside.

pub fn glass_castle(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let form = Form::of(c.floors);
    let lot = offset_poly(&c.lot_poly, 0.8);
    let front = c.front_face(&lot);
    ground(g, f, Paving, &lot, 0.04, mul(pal.paving, 1.15));
    // Glass fence on steel posts, open at the gate.
    let gate_w = 5.0;
    for face in c.faces(&lot) {
        let is_gate = face.a.distance(front.a) < 0.1;
        let n = (face.len / 2.5).floor().max(1.0) as i32;
        let seg = face.len / n as f32;
        for i in 0..n {
            let t = -face.len * 0.5 + (i as f32 + 0.5) * seg;
            if is_gate && t.abs() < gate_w * 0.5 + 0.5 {
                continue;
            }
            face.decal(g, f, Glass, t, 1.1, -0.15, seg - 0.1, 2.2, pal.glass(0));
            face.bx(g, f, Metal, t + seg * 0.5, 1.15, -0.15, Vec3::new(0.12, 2.3, 0.12), mul(pal.steel, 1.1));
        }
    }
    // Gatehouse: a glass box with a lit canopy.
    let gt = gate_w * 0.5 + 2.0;
    if [(-1.6f32, -0.8f32), (1.6, -0.8), (1.6, -4.0), (-1.6, -4.0)].iter().all(|(dt, dd)| c.inside_local(front.at(gt + dt, *dd))) {
        front.bx(g, f, Glass, gt, 1.6, -2.4, Vec3::new(2.6, 3.2, 2.6), pal.glass(1));
        front.bx(g, f, Glow, gt, 3.3, -2.4, Vec3::new(3.0, 0.15, 3.0), mul(pal.lamp, 0.9));
    }
    bollards(g, c, &front, 4, 1.2);

    // The crystals.
    let site = offset_poly(&c.lot_poly, 4.0);
    let site = if site.len() >= 3 && inradius(&site) > 3.0 { site } else { fit_inside(c, &scale_poly(&c.poly, 0.7)) };
    let pc = poly_center(&site);
    let ir = inradius(&site).max(3.0);
    let tall = match form {
        Form::Low => (c.floors as f32).clamp(3.0, 4.0) * fh,
        Form::Mid => (c.floors as f32 + 2.0) * fh,
        Form::High => (c.floors as f32 + 4.0).min(40.0) * fh,
    };
    let n = match form {
        Form::Low => rng.random_range(3..=4),
        _ => rng.random_range(3..=5),
    };
    let glass_cols = [pick(rng, &pal.glass), pick(rng, &pal.glass)];
    let mut placed: Vec<(Vec2, f32, f32)> = Vec::new();
    for i in 0..n {
        let main = i == 0;
        let r = if main { ir * rng.random_range(0.45..0.6) } else { ir * rng.random_range(0.25..0.42) };
        let a = i as f32 / n as f32 * std::f32::consts::TAU + rng.random_range(-0.4..0.4);
        let off = if main { Vec2::ZERO } else { Vec2::new(a.cos(), a.sin()) * (ir - r) * rng.random_range(0.55..0.95) };
        let centre = pc + off;
        let h = if main { tall } else { tall * rng.random_range(0.35..0.75) };
        let sides = *[5usize, 6, 7].choose(rng).unwrap();
        let rot = rng.random_range(0.0..std::f32::consts::TAU);
        let base = fit_inside(c, &ngon(centre.x, centre.y, r, sides, rot));
        if base.len() < 3 {
            continue;
        }
        let mid_poly = rotate_poly(&scale_poly(&base, 0.92), rng.random_range(-0.15..0.15));
        let mid_poly = fit_inside(c, &mid_poly);
        let glass = mul(glass_cols[i % 2], 1.0 - 0.25 * c.wear);
        // Two faceted stages: the body tilts its facets as it rises.
        loft(g, f, Glass, &base, &mid_poly, 0.0, h * 0.55, glass);
        loft(g, f, Glass, &mid_poly, &fit_inside(c, &scale_poly(&mid_poly, 0.95)), h * 0.55, h, mul(glass, 1.08));
        let body_top = fit_inside(c, &scale_poly(&mid_poly, 0.95));
        // Lit floor seams.
        let floors = (h / fh) as i32;
        for k in (1..floors).step_by(if form == Form::High { 2 } else { 1 }) {
            if coin(rng, c.lit_chance()) {
                let y = k as f32 * fh;
                // On the facets at this height: the lofts turn and taper
                // from one plan to the next, so the seam mixes the two.
                let mix_plan = |a: &[Vec2], b: &[Vec2], t: f32| -> Vec<Vec2> { if a.len() == b.len() { a.iter().zip(b).map(|(p, q)| p.lerp(*q, t)).collect() } else { scale_poly(a, 1.0 - (1.0 - 0.95) * t) } };
                let ring = if y < h * 0.55 { mix_plan(&base, &mid_poly, y / (h * 0.55)) } else { mix_plan(&mid_poly, &body_top, (y - h * 0.55) / (h * 0.45)) };
                let ring = offset_poly(&ring, -0.04);
                let col = mul(pick(rng, &c.style.window_colors), 0.75);
                window_light(g, c, |g| extrude(g, f, Glow, &ring, y, y + 0.2, col));
            }
        }
        crystal_crown(g, c, rng, &body_top, h, glass);
        placed.push((centre, h, r));
    }
    // Glass bridges between the main crystal and the next, low down.
    if placed.len() >= 2 && form != Form::Low {
        let (a, ha, _) = placed[0];
        let (b, hb, _) = placed[1];
        let y = (ha.min(hb) * 0.5 / fh).floor() * fh;
        sky_bridge(g, c, a, b, y.max(fh * 2.0), 2.6, fh * 0.9);
    }
    // Reflecting pool in front of the crystals, lamps along the drive.
    let pool_c = front.at(-front.len * 0.22, -6.0);
    if poly_inside(c, &rect(pool_c.x, pool_c.y, 3.4, 1.9)) && !placed.iter().any(|(p, _, r)| p.distance(pool_c) < r + 3.5) {
        pool(g, c, pool_c.x, pool_c.y, 6.0, 3.2);
    }
    for k in 0..2 {
        lamp_post_in(g, c, front.at((k as f32 - 0.5) * gate_w * 1.6, -3.5), 3.0, pal.lamp);
    }
}
