//! Homes: container piles and brick walk-ups for the poor, concrete slabs and
//! courtyard blocks for the middle, glass and terraces for the rich (the
//! glass castle lives in `towers`).

use super::parts::*;
use super::{Ctx, Face};
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::seq::IndexedRandom;
use rand::Rng;
use nd_sim::zoning::{point_in_polygon, polygon_area};

// ── the Stacks (their own module) ─────────────────────────────────────────────

pub use super::stacks::stacks;

// ── walk-up tenement ─────────────────────────────────────────────────────────

pub fn tenement(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).clamp(3, 7);
    let h = floors as f32 * fh;
    let brick = mul(pick(rng, &pal.brick), 1.0 - 0.25 * c.wear);
    let poly = &c.poly;
    extrude(g, f, Brick, poly, 0.0, h, brick);
    // string courses and a cornice
    for k in 1..floors {
        cornice(g, c, poly, k as f32 * fh, 0.18, 0.08, Concrete, mul(c.style.trim_color, 1.2));
    }
    cornice(g, c, poly, h - 0.3, 0.6, 0.35, Concrete, mul(c.style.trim_color, 1.25));
    parapet(g, c, poly, h, 0.8, 0.3, Brick, brick);

    let mut spec = WindowSpec::from_ctx(c);
    spec.w = 1.0;
    spec.h = 1.5;
    spec.spacing = 2.4;
    spec.ac = 0.35 + 0.3 * c.wear;
    spec.boarded = 0.15 + 0.5 * c.wear;
    let faces = c.faces(poly);
    for face in &faces {
        if face.len < 3.0 {
            continue;
        }
        windows(g, c, rng, face, 0.0, floors, fh, &spec);
        grime(g, c, rng, face, 0.0, h, 2.5);
        graffiti(g, c, rng, face, 0.0, fh, 1.3);
        downpipes(g, c, rng, face, h);
    }
    let front = c.front_face(poly);
    // Stoop and door.
    entrance(g, c, rng, &front, -front.len * 0.25, 1.6, 2.6);
    plinth_steps(g, c, &front, -front.len * 0.25);
    // Fire escape on the longest non-front face, or the front when there is nothing else.
    let fe = faces.iter().filter(|x| !x.is_front && x.len > 6.0).max_by(|a, b| a.len.total_cmp(&b.len)).copied().unwrap_or(front);
    fire_escape(g, c, rng, &fe, 0.0, 0.0, floors, fh);
    // Ground floor shop on big frontages.
    if front.len > 12.0 && rng.random_bool(0.6) {
        let sub = Face::from_edge(front.at(0.0, 0.0), front.at(front.len * 0.5, 0.0));
        storefront(g, c, rng, &sub, 0.0, fh);
    }
    roof_gear(g, c, rng, poly, h, 1.0 + c.style.clutter);
    // Laundry lines on the roof.
    let (lo, hi) = bounds(&offset_poly(poly, 1.5));
    if hi.x - lo.x > 6.0 {
        for k in 0..2 {
            let v = lo.y + (hi.y - lo.y) * (0.3 + 0.4 * k as f32);
            if !c.inside_local(Vec2::new(lo.x + 1.0, v)) || !c.inside_local(Vec2::new(hi.x - 1.0, v)) {
                continue;
            }
            let a = f.at(lo.x + 1.0, h + 2.0, v);
            let b = f.at(hi.x - 1.0, h + 2.0, v);
            f.bx(g, Matte, lo.x + 1.0, h + 1.0, v, Vec3::new(0.08, 2.0, 0.08), CHARCOAL);
            f.bx(g, Matte, hi.x - 1.0, h + 1.0, v, Vec3::new(0.08, 2.0, 0.08), CHARCOAL);
            g.part(Matte).wire(a, b, 0.2, 0.03, rgba(DARK));
            for i in 1..5 {
                let p = a.lerp(b, i as f32 / 5.0);
                g.part(Tarp).boxed(p - Vec3::Y * 0.45, Vec3::new(0.6, 0.7, 0.03), f.yaw(), rgba(mul(pick(rng, &pal.tarp_colors), 1.3)));
            }
        }
    }
    if rng.random_bool(0.4) {
        let p = Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
        let bw = (hi.x - lo.x).min(9.0);
        if [-0.5f32, 0.5].iter().all(|s| nd_sim::zoning::point_in_polygon(p + Vec2::new(s * bw, 0.0), poly)) {
            billboard(g, c, rng, p, 0.0, h, bw, 3.0);
        }
    }
}

fn plinth_steps(g: &mut Geometry, c: &Ctx, face: &Face, t: f32) {
    let col = mul(c.palette.stone, 1.3);
    for k in 0..3 {
        face.bx(g, &c.frame, Stone, t, 0.08 + k as f32 * 0.16, 0.6 - k as f32 * 0.2, Vec3::new(2.4, 0.16, 0.4), col);
    }
}

// ── brutalist block ──────────────────────────────────────────────────────────

pub fn brutalist(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let advert = rng.random_bool(0.45);
    brutalist_block(g, c, rng, false, advert);
}

/// Stacked concrete slabs with alternating overhangs, recessed window bands,
/// fins and optional LED adverts. `monumental` floats the mass on pilotis.
pub fn brutalist_block(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, monumental: bool, advert: bool) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let concrete = mul(pick(rng, &pal.concrete), 1.0 - 0.3 * c.wear);
    let floors = c.floors as i32;
    let slab_count = if floors >= 6 { rng.random_range(2..=3) } else if floors >= 3 { rng.random_range(1..=2) } else { 1 };
    let mut remaining = floors;
    let mut y = 0.0;
    let mut slabs: Vec<(Vec<Vec2>, f32, f32)> = Vec::new(); // (poly, y0, h)
    for i in 0..slab_count {
        let fl = if i == slab_count - 1 { remaining } else { (remaining / (slab_count - i)).max(1) };
        remaining -= fl;
        let h = fl as f32 * fh;
        let base = if i == 0 { c.poly.clone() } else { scale_poly(&c.poly, rng.random_range(0.72..0.95)) };
        let poly = if i == 0 { base } else { fit_inside(c, &shift_poly(&base, Vec2::new(rng.random_range(-1.8..1.8), rng.random_range(-1.2..1.2)))) };
        slabs.push((poly, y, h));
        y += h;
    }
    let ground_h = if monumental { fh * 1.6 } else { fh };

    for (si, (poly, y0, h)) in slabs.iter().enumerate() {
        let (y0, h) = (*y0, *h);
        if si == 0 && monumental {
            extrude(g, f, Concrete, poly, y0 + ground_h, y0 + h, concrete);
            pilotis(g, c, &offset_poly(poly, 0.8), y0, y0 + ground_h, 4.0, 0.9, Concrete, concrete);
            plinth(g, c, poly, 2, 0.3, 1.2, Concrete, mul(concrete, 0.85));
            // lit lobby core
            let core = scale_poly(poly, 0.45);
            extrude(g, f, Glass, &core, 0.0, ground_h, pal.glass_dark);
            curtain_lights(g, c, rng, &core, 0.0, 1, ground_h);
        } else {
            extrude(g, f, Concrete, poly, y0, y0 + h, concrete);
        }
        // Over the pilotis the floors start where the mass does (1.6 floor
        // heights up), not one floor up: the first band hung under it.
        let start = if si == 0 && monumental { ground_h } else { 0.0 };
        let fl = if start > 0.0 { ((h - start) / fh + 0.05).floor().max(0.0) as i32 } else { (h / fh).round() as i32 };
        for k in 0..fl {
            cornice(g, c, poly, y0 + start + k as f32 * fh + fh * 0.95, 0.35, 0.25, Concrete, mul(concrete, 1.08));
        }
        for face in c.faces(poly) {
            if face.len < 4.0 {
                continue;
            }
            band_windows(g, c, rng, &face, y0 + start, fl, fh);
            if face.is_front {
                let fins = ((face.len / 3.2) as i32).max(2);
                for k in 0..=fins {
                    let t = -face.len * 0.5 + k as f32 * (face.len / fins as f32);
                    face.bx(g, f, Concrete, t, y0 + h * 0.5, 0.3, Vec3::new(0.35, h, 0.6), mul(concrete, 1.05));
                }
            }
            grime(g, c, rng, &face, y0, y0 + h, 1.5);
        }
    }
    let top = y;
    let (tpoly, _, _) = slabs.last().unwrap().clone();
    roof_gear(g, c, rng, &tpoly, top, 0.8 + c.style.clutter);
    if coin(rng, c.rules.roof_antenna_chance) {
        antenna_mast(g, c, poly_center(&tpoly), top, rng.random_range(4.0..10.0));
    }
    if advert {
        let (p0, _, h0) = &slabs[0];
        let front = c.front_face(p0);
        let sw = (front.len * rng.random_range(0.35..0.6)).min(14.0);
        let sh = (sw * 0.56).min(h0 * 0.6);
        let st = rng.random_range(-(front.len * 0.5 - sw * 0.5).max(0.0)..=(front.len * 0.5 - sw * 0.5).max(0.0));
        led_screen(g, c, rng, &front, st, (h0 * 0.55).max(sh * 0.5 + fh), sw, sh);
        if rng.random_bool(0.5) {
            let (lo, hi) = bounds(&tpoly);
            let bw = ((hi.x - lo.x) * 0.7).min(16.0);
            let at = Vec2::new((lo.x + hi.x) * 0.5, lo.y + 2.5);
            if [-0.5f32, 0.5].iter().all(|s| nd_sim::zoning::point_in_polygon(at + Vec2::new(s * bw, 0.0), &tpoly)) {
                billboard(g, c, rng, at, 0.0, top, bw, ((hi.x - lo.x) * 0.28).min(6.0));
            }
        }
    }
    let front = c.front_face(&slabs[0].0);
    if !monumental {
        entrance(g, c, rng, &front, 0.0, 2.6, 3.0);
    }
}

// ── balcony slab ─────────────────────────────────────────────────────────────

pub fn balcony_slab(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).clamp(4, 14);
    let h = floors as f32 * fh;
    let wall = mul(pick(rng, &pal.plaster), 1.0 - 0.25 * c.wear);
    let poly = &c.poly;
    extrude(g, f, Plaster, poly, 0.0, h, wall);
    let slab = mul(pick(rng, &pal.concrete), 1.1);
    let rail = mul(c.style.trim_color, 0.8);
    for face in c.faces(poly) {
        if face.len < 6.0 {
            continue;
        }
        let n = (face.len / 4.2).floor().max(1.0) as i32;
        let step = face.len / n as f32;
        for k in 1..floors {
            let y = k as f32 * fh;
            for i in 0..n {
                let t = -face.len * 0.5 + (i as f32 + 0.5) * step;
                balcony(g, c, &face, t, y, step * 0.86, 1.5, slab, rail);
                // door and window behind
                face.decal(g, f, Matte, t, y + 1.2, 0.03, 2.2, 2.3, c.style.trim_color);
                if coin(rng, c.lit_chance()) {
                    let col = mul(pick(rng, &c.style.window_colors), 0.8);
                    face.glaze(g, c, t, y + 1.2, 0.05, 2.0, 2.1, Pane::Lit(col));
                } else {
                    face.glaze(g, c, t, y + 1.2, 0.05, 2.0, 2.1, Pane::Dark);
                }
                if coin(rng, c.style.clutter * 0.5) {
                    face.bx(g, f, Tarp, t + step * 0.2, y + 0.9, 1.0, Vec3::new(0.6, 0.5, 0.6), pick(rng, &pal.tarp_colors));
                }
            }
        }
        // ground floor
        let mut spec = WindowSpec::from_ctx(c);
        spec.spacing = 3.0;
        windows(g, c, rng, &face, 0.0, 1, fh, &spec);
        grime(g, c, rng, &face, 0.0, h, 1.2);
    }
    parapet(g, c, poly, h, 1.0, 0.25, Plaster, wall);
    // lift and stair core rising above the roof
    let core = fit_inside(c, &shift_poly(&scale_poly(poly, 0.22), Vec2::new(c.fw * 0.2, 0.0)));
    extrude(g, f, Concrete, &core, h, h + 3.5, mul(slab, 0.9));
    roof_gear(g, c, rng, poly, h, 0.6 + c.style.clutter * 0.5);
    let front = c.front_face(poly);
    entrance(g, c, rng, &front, 0.0, 2.4, 2.8);
    bollards(g, c, &front, 4, 2.0);
}

// ── courtyard block ──────────────────────────────────────────────────────────

pub fn courtyard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    courtyard_with(g, c, rng, false);
}

/// [pt-visual] A courtyard block; `luxe` is the high-end version a big
/// garden-tower lot gets (F41): white plaster, tall glass, a planted roof
/// on every wing and a pool in the garden court.
fn courtyard_with(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, luxe: bool) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).clamp(3, 8);
    let h = floors as f32 * fh;
    let wall = if luxe { mul(pick(rng, &pal.plaster), 1.25) } else { mul(pick(rng, &pal.brick), 1.0 - 0.2 * c.wear) };
    let skin = if luxe { Plaster } else { Brick };
    let poly = &c.poly;
    let mut spec = WindowSpec::from_ctx(c);
    spec.spacing = 2.6;
    spec.w = 1.2;
    spec.h = 1.6;
    if luxe {
        spec.w = 2.0;
        spec.h = fh * 0.62;
        spec.spacing = 2.8;
        spec.sill = (fh - spec.h) * 0.4;
        spec.ac = 0.0;
        spec.boarded = 0.0;
    }
    // [pt-visual] The wings are the ring between the lot's outline and the
    // courtyard: one quad per edge, sharing its inner corners with the next
    // wing, so every outer and inner face has a wall behind it (F39: wings
    // were boxes shortened on a wedge while their windows ran the whole
    // edge, so windows floated in the air at the sharp corners). A lot too
    // small or too twisted for a ring is one solid block.
    let Some((inner, depth)) = courtyard_ring(poly) else {
        extrude(g, f, skin, poly, 0.0, h, wall);
        for face in c.faces(poly) {
            if face.len >= 4.0 {
                windows(g, c, rng, &face, 0.0, floors, fh, &spec);
                grime(g, c, rng, &face, 0.0, h, 1.2);
            }
        }
        cornice(g, c, poly, h - 0.3, 0.5, 0.3, Concrete, mul(c.style.trim_color, 1.2));
        parapet(g, c, poly, h, 0.7, 0.3, skin, wall);
        let front = c.front_face(poly);
        entrance(g, c, rng, &front, 0.0, 2.4, 2.8);
        if luxe {
            green_roof(g, c, poly, h);
        } else {
            roof_gear(g, c, rng, poly, h, 0.8);
        }
        return;
    };
    let n = poly.len();
    let wings: Vec<Vec<Vec2>> = (0..n).map(|i| vec![poly[i], poly[(i + 1) % n], inner[(i + 1) % n], inner[i]]).collect();
    for wing in &wings {
        extrude(g, f, skin, wing, 0.0, h, wall);
    }
    for face in c.faces(poly) {
        if face.len < 4.0 {
            continue;
        }
        windows(g, c, rng, &face, 0.0, floors, fh, &spec);
        grime(g, c, rng, &face, 0.0, h, 1.2);
    }
    // Inner faces look onto the courtyard.
    for face in c.faces(&inner) {
        let inward_face = Face::from_edge(face.b, face.a);
        if inward_face.len < 3.0 {
            continue;
        }
        windows(g, c, rng, &inward_face, 0.0, floors, fh, &spec);
        for k in 1..floors {
            balcony(g, c, &inward_face, 0.0, k as f32 * fh, (inward_face.len * 0.9).min(6.0), 1.2, mul(pick(rng, &pal.concrete), 1.1), c.style.trim_color);
        }
    }
    cornice(g, c, poly, h - 0.3, 0.5, 0.3, Concrete, mul(c.style.trim_color, 1.2));
    parapet(g, c, poly, h, 0.7, 0.3, skin, wall);
    // The courtyard's own parapet, the wall seen from inside.
    let inner_rev: Vec<Vec2> = inner.iter().rev().copied().collect();
    parapet(g, c, &inner_rev, h, 0.7, 0.3, skin, wall);
    // Courtyard garden.
    ground(g, f, Paving, &inner, 0.05, pal.paving);
    let cc = poly_center(&inner);
    let r = inradius(&inner);
    if luxe && r > 6.0 && point_in_polygon(cc, &inner) {
        pool(g, c, cc.x, cc.y, (r * 0.9).min(14.0), (r * 0.5).min(6.0));
        for k in 0..6 {
            let a = k as f32 / 6.0 * std::f32::consts::TAU + 0.3;
            let p = cc + Vec2::new(a.cos(), a.sin()) * r * 0.75;
            if point_in_polygon(p, &inner) && distance_to_boundary(p, &inner) > 1.5 {
                tree_in(g, f, pal, p, 4.0, 3.0, pal.foliage);
            }
        }
    } else if r > 3.0 && point_in_polygon(cc, &inner) {
        tree_in(g, f, pal, cc, 4.5, 4.0, pal.foliage);
        for k in 0..3 {
            let a = k as f32 / 3.0 * std::f32::consts::TAU;
            let p = cc + Vec2::new(a.cos(), a.sin()) * r * 0.55;
            if !point_in_polygon(p, &inner) {
                continue;
            }
            lamp_post(g, f, p, 3.2, pal.lamp);
            bench_world(g, f.to_world(p.x + 0.8, p.y), f.yaw() + a, pal.wood, CHARCOAL);
        }
    }
    // Arched gateway on the front.
    let front = c.front_face(poly);
    front.bx(g, f, Matte, 0.0, 2.2, -depth * 0.5, Vec3::new(3.4, 4.4, depth + 0.3), DARK);
    front.bx(g, f, Glow, 0.0, 4.0, -depth * 0.5, Vec3::new(2.0, 0.1, 1.0), mul(pal.lamp, 0.8));
    // Roof gear on the wings' roofs, not over the courtyard: the two
    // longest wings carry it.
    let mut by_len: Vec<usize> = (0..n).collect();
    by_len.sort_by(|&a, &b| poly[(b + 1) % n].distance(poly[b]).total_cmp(&poly[(a + 1) % n].distance(poly[a])));
    if luxe {
        for wing in &wings {
            green_roof(g, c, wing, h);
        }
        return;
    }
    for &i in by_len.iter().take(2) {
        roof_gear(g, c, rng, &wings[i], h, 0.8);
    }
}

/// [pt-visual] The courtyard of a courtyard block: the lot's outline offset
/// inward by the wing depth, every vertex kept so edge `i` of the courtyard
/// is the back of wing `i`. Shallower wings are tried before giving up;
/// `None` when no ring fits (the courtyard would turn inside out, cross
/// itself or leave the lot).
pub(crate) fn courtyard_ring(poly: &[Vec2]) -> Option<(Vec<Vec2>, f32)> {
    let n = poly.len();
    if n < 3 {
        return None;
    }
    let area = polygon_area(poly);
    let ir = inradius(poly);
    let depth0 = (ir * 0.42).clamp(5.0, 10.0);
    for depth in [depth0, depth0 * 0.8, 5.0f32.min(depth0)] {
        if depth > ir * 0.7 {
            continue;
        }
        let inner = offset_edges(poly, &vec![depth; n]);
        let a = polygon_area(&inner);
        if a.signum() != area.signum() || a.abs() < 30.0 || a.abs() >= area.abs() {
            continue;
        }
        if nd_sim::geom::self_intersects(&inner) || !inner.iter().all(|q| point_in_polygon(*q, poly) && distance_to_boundary(*q, poly) > 2.0) {
            continue;
        }
        // Every wing must keep its own side of the courtyard: no wing quad
        // may fold over (inner edge running backwards against its outer one).
        let folded = (0..n).any(|i| (poly[(i + 1) % n] - poly[i]).dot(inner[(i + 1) % n] - inner[i]) <= 0.0);
        if folded {
            continue;
        }
        return Some((inner, depth));
    }
    None
}

// ── crystal towers ───────────────────────────────────────────────────────────

pub fn crystal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let height = c.height().max(fh * 8.0);
    let lit_chance = c.lit_chance();
    let shards = rng.random_range(1..=3);
    let max_r = (inradius(&c.poly) * 0.95).max(3.0);
    let mut shard_centers: Vec<(Vec2, Vec2, f32)> = Vec::new();

    let podium_h = fh * 1.2;
    extrude(g, f, Stone, &c.poly, 0.0, podium_h, mul(pal.stone, 1.4));
    let front = c.front_face(&c.poly);
    front.decal(g, f, Glow, 0.0, podium_h * 0.5, 0.05, front.len * 0.8, podium_h * 0.5, mul(c.style.window_color(0), 0.8));
    let pc = poly_center(&c.poly);

    for s in 0..shards {
        let main = s == 0;
        let r = if main { max_r * rng.random_range(0.7..0.95) } else { max_r * rng.random_range(0.35..0.6) };
        let h = if main { height } else { height * rng.random_range(0.35..0.8) };
        let sides = *[4usize, 5, 6, 6, 7, 8].choose(rng).unwrap();
        let rot = rng.random_range(0.0..std::f32::consts::TAU);
        let local = if main { pc } else { pc + Vec2::new(rng.random_range(-max_r * 0.6..max_r * 0.6), rng.random_range(-max_r * 0.6..max_r * 0.6)) };
        let center = f.to_world(local.x, local.y);
        let taper = rng.random_range(0.25..0.75);
        let lean = Vec2::new(rng.random_range(-0.12..0.12), rng.random_range(-0.12..0.12)) * h;
        // The top ring must stay over the podium: lean at most what the
        // taper frees up.
        let top_center = center + lean.clamp_length_max((max_r / 0.95 - r * taper - 0.3).clamp(0.0, r * 0.8));
        let bottom = MeshData::ngon(center, r, sides, rot, podium_h);
        let top = MeshData::ngon(top_center, r * taper, sides, rot + rng.random_range(-0.4..0.4), podium_h + h);
        let glass = pick(rng, &pal.glass);
        g.part(Glass).frustum(&bottom, &top, rgba(mul(glass, 1.0 - 0.3 * c.wear)));
        shard_centers.push((center, top_center, h));
        let floors_here = (h / fh) as i32;
        for k in 1..floors_here {
            if !coin(rng, lit_chance) {
                continue;
            }
            let t0 = k as f32 / floors_here as f32;
            let t1 = t0 + 0.35 / floors_here as f32;
            let ring = |t: f32| -> Vec<Vec3> {
                bottom
                    .iter()
                    .zip(top.iter())
                    .map(|(b, tp)| {
                        let p = b.lerp(*tp, t);
                        let cc = Vec3::new(center.x, 0.0, center.y).lerp(Vec3::new(top_center.x, 0.0, top_center.y), t);
                        p + Vec3::new(p.x - cc.x, 0.0, p.z - cc.z) * 0.02
                    })
                    .collect()
            };
            let wc = pick(rng, &c.style.window_colors);
            let col = rgba(mul(wc, rng.random_range(0.5..0.9)));
            window_light(g, c, |g| g.part(Glow).frustum(&ring(t0), &ring(t1), col));
        }
        let crown = top.iter().fold(Vec3::ZERO, |a, p| a + *p) / top.len() as f32;
        if main {
            // A faceted, lit cap rather than a needle: the top ring lifted
            // into a short shallow prism with a glowing seam.
            let cap_top = MeshData::ngon(top_center, r * taper * 0.55, sides, rot + 0.3, podium_h + h + (r * taper * 0.6).clamp(1.5, 5.0));
            g.part(Glass).frustum(&top, &cap_top, rgba(mul(glass, 1.15)));
            g.part(Glow).frustum(&top.iter().map(|p| *p + Vec3::Y * 0.02).collect::<Vec<_>>(), &top.iter().map(|p| *p + Vec3::Y * 0.22).collect::<Vec<_>>(), rgba(mul(pal.window_cold, 0.8)));
            let top_y = cap_top[0].y;
            aviation_light(g, pal, Vec3::new(crown.x, top_y + 0.15, crown.z), 0.4);
        } else {
            aviation_light(g, pal, Vec3::new(crown.x, podium_h + h + 0.15, crown.z), 0.4);
        }
    }
    if shard_centers.len() >= 2 && rng.random_bool(0.6) {
        let (b0, t0, h0) = shard_centers[0];
        let (b1, t1, h1) = shard_centers[1];
        let y = podium_h + h0.min(h1) * rng.random_range(0.3..0.6);
        let p0 = b0.lerp(t0, (y - podium_h) / h0);
        let p1 = b1.lerp(t1, (y - podium_h) / h1);
        let mid = (p0 + p1) * 0.5;
        let d = p1 - p0;
        g.part(Glass).boxed(Vec3::new(mid.x, y, mid.y), Vec3::new(d.length() + 2.0, 2.2, 2.5), d.y.atan2(d.x), rgba(pal.glass(0)));
        g.part(Matte).boxed(Vec3::new(mid.x, y - 1.2, mid.y), Vec3::new(d.length() + 2.0, 0.3, 2.6), d.y.atan2(d.x), rgba(pal.steel));
    }
    for k in 0..4 {
        let t = -front.len * 0.375 + k as f32 * front.len * 0.25;
        front.bx(g, f, Glow, t, 0.45, -0.8, Vec3::new(0.25, 0.9, 0.25), mul(pal.lamp, 0.7));
    }
    entrance(g, c, rng, &front, 0.0, 3.0, podium_h * 0.8);
}

// ── garden tower ─────────────────────────────────────────────────────────────

pub fn garden_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    // [pt-visual] A big lot with few floors is a garden courtyard, not a
    // squat terraced pile with a tree on its roof (F41).
    if c.floors < 7 && inradius(&c.poly) > 11.0 && courtyard_ring(&c.poly).is_some() {
        courtyard_with(g, c, rng, true);
        return;
    }
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    // [r4-flags-look] (flag day 46 at (-427,329), "this building is coming
    // through my elevated road") As many floors as the building has: a
    // one-storey garden house under a walkway deck (held to one floor by
    // the deck's clearance) was drawn six storeys high, up through the deck.
    let floors = (c.floors as i32).clamp(1, 22);
    let white = mul(pick(rng, &pal.plaster), 1.2);
    let base = c.poly.clone();
    let step = (inradius(&base) * 0.7 / floors as f32).clamp(0.12, 0.6);
    let twist = rng.random_range(-0.03..0.03);
    let mut prev = base.clone();
    for k in 0..floors {
        let y0 = k as f32 * fh;
        let plate = rotate_poly(&offset_poly(&base, step * k as f32), twist * k as f32);
        let plate = if plate.len() == base.len() { plate } else { scale_poly(&base, 1.0 - 0.6 * k as f32 / floors as f32) };
        let plate = fit_inside(c, &plate);
        // floor slab proud of the glass, glass walls set back
        extrude(g, f, Concrete, &plate, y0, y0 + 0.35, white);
        let glass_poly = offset_poly(&plate, 1.6);
        extrude(g, f, Glass, &glass_poly, y0 + 0.35, y0 + fh, pick(rng, &pal.glass));
        curtain_lights(g, c, rng, &glass_poly, y0 + 0.35, 1, fh - 0.35);
        // terrace planting along the slab edge
        for face in c.faces(&plate) {
            if face.len < 3.0 {
                continue;
            }
            face.bx(g, f, Matte, 0.0, y0 + 0.9, -0.1, Vec3::new(face.len * 0.96, 1.1, 0.04), mul(pal.glass_dark, 1.6));
            if rng.random_bool(0.7) {
                face.bx(g, f, Foliage, 0.0, y0 + 0.7, -0.6, Vec3::new(face.len * 0.8, 0.7, 0.8), mul(pal.foliage, rng.random_range(0.8..1.2)));
            }
            if rng.random_bool(0.25) && face.len > 5.0 {
                let p = face.at(rng.random_range(-face.len * 0.3..face.len * 0.3), -1.4);
                tree_kind(g, f.to_world(p.x, p.y), y0 + 0.35, TreeKind::Round, 2.2, 1.9, pal.bark, pal.foliage);
            }
        }
        prev = plate;
    }
    let top = floors as f32 * fh;
    extrude(g, f, Concrete, &prev, top, top + 0.35, white);
    let roof = offset_poly(&prev, 1.0);
    // A low garden house (under a deck, say) has a planted roof, no pool
    // terrace, tree or mast standing up off it.
    let low = floors < 4;
    if roof.len() >= 3 && inradius(&roof) > 3.0 && !low {
        pool(g, c, poly_center(&roof).x, poly_center(&roof).y, inradius(&roof) * 0.8, inradius(&roof) * 0.5);
        let w = f.to_world(poly_center(&roof).x + inradius(&roof) * 0.6, poly_center(&roof).y);
        tree_kind(g, w, top + 0.35, TreeKind::Round, 1.9, 2.6, pal.bark, pal.foliage);
    }
    if !low {
        antenna_mast(g, c, poly_center(&prev), top + 0.35, 4.0);
    } else if prev.len() >= 3 && inradius(&prev) > 2.0 {
        green_roof(g, c, &prev, top + 0.35);
    }
    let front = c.front_face(&base);
    entrance(g, c, rng, &front, 0.0, 3.2, 3.4);
    for k in 0..3 {
        lamp_post_in(g, c, front.at(-front.len * 0.35 + k as f32 * front.len * 0.35, -1.5), 3.0, pal.lamp);
    }
}

// ── villa cluster ────────────────────────────────────────────────────────────

pub fn villas(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let poly = yard(g, c, 0.6, Foliage, mul(pal.foliage, 0.8));
    // Hedge and a gate along the parcel edge.
    let front = c.front_face(&poly);
    let front_i = c.front_face_index(&poly);
    for (i, face) in c.faces(&poly).iter().enumerate() {
        if Some(i) == front_i {
            let side = (face.len - 4.0) * 0.5;
            if side > 1.0 {
                hedge(g, c, face.a, face.at(-2.0, 0.0), 1.8, 1.0, pal.foliage);
                hedge(g, c, face.at(2.0, 0.0), face.b, 1.8, 1.0, pal.foliage);
            }
            for s in [-2.2f32, 2.2] {
                face.bx(g, f, Stone, s, 1.1, -0.5, Vec3::new(0.5, 2.2, 0.5), mul(pal.stone, 1.3));
                face.bx(g, f, Glow, s, 2.3, -0.5, Vec3::new(0.3, 0.2, 0.3), mul(pal.lamp, 0.8));
            }
            face.bx(g, f, Matte, -1.0, 1.0, -0.5, Vec3::new(1.9, 1.8, 0.06), CHARCOAL);
        } else {
            hedge(g, c, face.a, face.b, 2.0, 1.0, pal.foliage);
        }
    }
    // Houses.
    let n = ((nd_sim::zoning::polygon_area(&poly).abs() / 260.0) as i32).clamp(1, 4);
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    for _ in 0..n * 8 {
        if placed.len() as i32 >= n {
            break;
        }
        let Some(p) = c.random_inside(rng, 6.0) else { break };
        if placed.iter().any(|(q, _)| q.distance(p) < 12.0) {
            continue;
        }
        let yaw = rng.random_range(-0.35..0.35);
        placed.push((p, yaw));
        let w = rng.random_range(8.0..12.0);
        let d = rng.random_range(7.0..10.0);
        let wall = mul(pick(rng, &pal.plaster), 1.15);
        let h1 = 3.4;
        let two = rng.random_bool(0.7);
        let house = rect_yaw(p.x, p.y, w * 0.5, d * 0.5, yaw);
        let house = fit_inside(c, &house);
        extrude(g, f, Plaster, &house, 0.0, h1, wall);
        extrude(g, f, Concrete, &offset_poly(&house, -0.3), h1, h1 + 0.3, mul(wall, 0.9));
        let mut spec = WindowSpec::from_ctx(c);
        spec.w = 2.6;
        spec.h = 2.2;
        spec.spacing = 3.6;
        spec.sill = 0.6;
        spec.ac = 0.0;
        spec.boarded = 0.0;
        for face in c.faces(&house) {
            windows(g, c, rng, &face, 0.0, 1, h1, &spec);
        }
        if two {
            let upper = fit_inside(c, &shift_poly(&scale_poly(&house, 0.7), Vec2::new(rng.random_range(-1.5..1.5), rng.random_range(-1.0..1.0))));
            extrude(g, f, Plaster, &upper, h1 + 0.3, h1 + 0.3 + h1, mul(wall, 0.95));
            extrude(g, f, Concrete, &offset_poly(&upper, -0.3), h1 * 2.0 + 0.3, h1 * 2.0 + 0.6, mul(wall, 0.9));
            for face in c.faces(&upper) {
                windows(g, c, rng, &face, h1 + 0.3, 1, h1, &spec);
            }
            solar_array(g, f, poly_center(&upper), h1 * 2.0 + 0.6, 3.0, 2.0, yaw, pal.glass_dark);
        }
        let hf = c.front_face(&house);
        // The porch canopy stands out from the door: only where there is lot.
        if [-1.6f32, 0.0, 1.6].iter().all(|t| c.inside_local(hf.at(*t, 1.6))) {
            entrance(g, c, rng, &hf, 0.0, 1.4, 2.4);
        }
        if rng.random_bool(0.6) {
            let pp = p + Vec2::new(0.0, d * 0.5 + 4.0);
            if poly_inside(c, &rect(pp.x, pp.y, 4.4, 2.7)) {
                pool(g, c, pp.x, pp.y, 7.0, 3.5);
            }
        }
        if rng.random_bool(0.6) {
            let pp = p + Vec2::new(-w * 0.5 - 3.0, -1.0);
            if vehicle_fits(c, pp, yaw + std::f32::consts::FRAC_PI_2, Vehicle::Car) {
                let col = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, pp, 0.0, yaw + std::f32::consts::FRAC_PI_2, Vehicle::Car, col);
            }
        }
    }
    for _ in 0..rng.random_range(3..7) {
        if let Some(p) = c.random_inside(rng, 3.2) {
            if placed.iter().all(|(q, _)| q.distance(p) > 8.0) {
                tree_in(g, f, pal, p, rng.random_range(4.0..7.0), rng.random_range(3.5..6.0), pal.foliage);
            }
        }
    }
    // Drive from the gate.
    let drive = [front.at(-1.0, -3.0), front.at(1.0, -3.0), front.at(1.0, -0.2), front.at(-1.0, -0.2)];
    ground(g, f, Paving, &drive, 0.05, mul(pal.paving, 1.1));
    lamp_post_in(g, c, front.at(-3.5, -3.0), 3.0, pal.lamp);
}
