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

// ── the Stacks ───────────────────────────────────────────────────────────────
//
// Shipping containers and old trailers piled up the way people would do it:
// dropped on the parcel at mixed orientations, each upper unit resting on
// units below (most of its footprint must be supported), overhanging corners
// propped on steel jacks, planks between neighbours, an external zigzag stair,
// and wires that run from a utility pole on the lot to the units.

#[derive(Clone, Copy)]
struct Unit {
    u: f32,
    v: f32,
    yaw: f32,
    len: f32,
    wid: f32,
    h: f32,
    level: i32,
    y0: f32,
    color: [f32; 3],
}

impl Unit {
    fn local_to_lot(&self, lu: f32, lv: f32) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(self.u + lu * c - lv * s, self.v + lu * s + lv * c)
    }

    fn corners(&self) -> [Vec2; 4] {
        let (hl, hw) = (self.len * 0.5, self.wid * 0.5);
        [self.local_to_lot(-hl, -hw), self.local_to_lot(hl, -hw), self.local_to_lot(hl, hw), self.local_to_lot(-hl, hw)]
    }

    fn contains(&self, p: Vec2) -> bool {
        let (s, c) = self.yaw.sin_cos();
        let d = p - Vec2::new(self.u, self.v);
        let lu = d.x * c + d.y * s;
        let lv = -d.x * s + d.y * c;
        lu.abs() <= self.len * 0.5 && lv.abs() <= self.wid * 0.5
    }

    fn top(&self) -> f32 {
        self.y0 + self.h
    }
}

fn units_overlap(a: &Unit, b: &Unit, margin: f32) -> bool {
    let ca = a.corners();
    let cb = b.corners();
    for axis in [ca[1] - ca[0], ca[3] - ca[0], cb[1] - cb[0], cb[3] - cb[0]] {
        let axis = axis.normalize_or_zero();
        if axis == Vec2::ZERO {
            continue;
        }
        let proj = |cs: &[Vec2; 4]| {
            let v: Vec<f32> = cs.iter().map(|p| p.dot(axis)).collect();
            (v.iter().cloned().fold(f32::MAX, f32::min), v.iter().cloned().fold(f32::MIN, f32::max))
        };
        let (a0, a1) = proj(&ca);
        let (b0, b1) = proj(&cb);
        if a1 + margin < b0 || b1 + margin < a0 {
            return false;
        }
    }
    true
}

pub fn stacks(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let steel = mul(pal.steel, 0.45 - 0.15 * c.wear);
    let lit_chance = c.lit_chance();
    let (lo, hi) = bounds(&c.lot_poly);
    let area = nd_sim::zoning::polygon_area(&c.lot_poly).abs();

    ground(g, f, Dirt, &c.lot_poly, 0.03, pal.dirt);
    // Alleys: trodden paving through the middle of the lot both ways.
    // Laid in 2 m pieces so they stop at the edge of odd-shaped lots.
    let mut v = lo.y + 1.0;
    while v < hi.y {
        if poly_inside(c, &rect(0.0, v, 1.1, 1.0)) {
            f.flat(g, Paving, 0.0, 0.05, v, 2.2, 2.0, 0.0, mul(pal.paving, 0.8));
        }
        v += 2.0;
    }
    let mut u = lo.x + 1.0;
    while u < hi.x {
        if poly_inside(c, &rect(u, 0.0, 1.0, 1.1)) {
            f.flat(g, Paving, u, 0.05, 0.0, 2.0, 2.2, 0.0, mul(pal.paving, 0.8));
        }
        u += 2.0;
    }

    let mut units: Vec<Unit> = Vec::new();
    let ground_target = ((area / 40.0) as i32).clamp(2, 10);
    let levels = ((c.floors as f32 * 1.3) as i32).clamp(2, 8);
    let mut prev_count = ground_target;
    for level in 0..levels {
        let target = if level == 0 { ground_target } else { ((prev_count as f32 * rng.random_range(0.6..0.9)).round() as i32).max(1) };
        let mut placed = 0;
        let mut attempts = 0;
        while placed < target && attempts < 90 {
            attempts += 1;
            let trailer = rng.random_bool(0.2);
            let (len, wid, h) = if trailer { (rng.random_range(6.5..8.5), 2.7, 2.9) } else if rng.random_bool(0.3) { (12.1, 2.44, 2.6) } else { (6.05, 2.44, 2.6) };
            let yaw = match rng.random_range(0..10) {
                0..=5 => 0.0,
                6..=8 => std::f32::consts::FRAC_PI_2,
                _ => rng.random_range(-0.45..0.45),
            };
            let (u, v, y0) = if level == 0 {
                (rng.random_range(lo.x..hi.x), rng.random_range(lo.y..hi.y), 0.0)
            } else {
                let below: Vec<&Unit> = units.iter().filter(|x| x.level == level - 1).collect();
                let Some(support) = below.choose(rng) else { break };
                let reach = support.len.max(len) * 0.45;
                let off = Vec2::new(rng.random_range(-reach..reach), rng.random_range(-2.0..2.0));
                let off = if support.yaw.abs() > 0.5 { Vec2::new(off.y, off.x) } else { off };
                (support.u + off.x, support.v + off.y, support.top())
            };
            let color = if trailer { mul([0.68, 0.66, 0.60], rng.random_range(0.6..0.95)) } else { pick(rng, &pal.container_colors) };
            let cand = Unit { u, v, yaw, len, wid, h, level, y0, color };
            // Stay inside the parcel (an overhang of half a metre is fine).
            if cand.corners().iter().any(|p| !c.inside_local(*p) && distance_to_boundary(*p, &c.lot_poly) > 0.6) {
                continue;
            }
            // Keep the alleys clear: a cross through the middle of the pile
            // that people walk and that joins the neighbours' alleys.
            if level == 0 && cand.corners().iter().any(|p| p.x.abs() < 1.3 || p.y.abs() < 1.3) && (cand.corners().iter().any(|p| p.x < -1.3) && cand.corners().iter().any(|p| p.x > 1.3) || cand.corners().iter().any(|p| p.y < -1.3) && cand.corners().iter().any(|p| p.y > 1.3)) {
                continue;
            }
            if units.iter().any(|o| o.level == level && units_overlap(o, &cand, 0.35)) {
                continue;
            }
            if level > 0 {
                let mut inside = 0;
                for i in 0..5 {
                    for j in 0..3 {
                        let lu = (i as f32 + 0.5) / 5.0 * len - len * 0.5;
                        let lv = (j as f32 + 0.5) / 3.0 * wid - wid * 0.5;
                        let p = cand.local_to_lot(lu, lv);
                        if units.iter().any(|o| o.level == level - 1 && o.contains(p)) {
                            inside += 1;
                        }
                    }
                }
                if (inside as f32) < 15.0 * 0.55 {
                    continue;
                }
            }
            units.push(cand);
            placed += 1;
        }
        if placed == 0 {
            break;
        }
        prev_count = placed;
    }

    for unit in &units {
        // Each unit jitters and fades its own paint in `container_unit`.
        let col = mul(unit.color, 1.0 - 0.3 * c.wear);
        let yc = unit.y0 + unit.h * 0.5;
        container_unit(g, c, rng, unit.u, unit.v, unit.yaw, unit.len, unit.wid, unit.h, unit.y0, col);
        // Tags along a long side of the lower units, where a spray can reaches.
        if unit.level <= 1 && rng.random_bool(0.55) {
            let s = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let ga = unit.local_to_lot(s * unit.len * 0.5, s * unit.wid * 0.5);
            let gb = unit.local_to_lot(-s * unit.len * 0.5, s * unit.wid * 0.5);
            graffiti(g, c, rng, &Face::from_edge(ga, gb), unit.y0 + 0.15, unit.y0 + unit.h - 0.15, 1.6);
        }
        let place = |g: &mut Geometry, s, lu: f32, y: f32, lv: f32, size: Vec3, color: [f32; 3]| {
            let p = unit.local_to_lot(lu, lv);
            f.bx_yaw(g, s, p.x, y, p.y, size, color, unit.yaw);
        };
        let side = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
        // Openings cut in the long side: a window and a door, as decals
        // proud of the corrugation (the frame, then the pane or leaf).
        let along = f.dir3(Vec2::new(unit.yaw.cos(), unit.yaw.sin()));
        let out = f.dir3(Vec2::new(-unit.yaw.sin(), unit.yaw.cos())) * side;
        let opening = |g: &mut Geometry, s, lu: f32, y: f32, d: f32, w: f32, h: f32, color: [f32; 3]| {
            g.part(s).decal(f.at2(unit.local_to_lot(lu, side * (unit.wid * 0.5 + d)), y), along, Vec3::Y, w, h, out, rgba(color));
        };
        let win_u = rng.random_range(-unit.len * 0.3..unit.len * 0.3);
        if coin(rng, lit_chance) {
            let wc = if rng.random_bool(0.3) { [0.5, 0.75, 1.1] } else { pick(rng, &c.style.window_colors) };
            opening(g, Matte, win_u, yc + 0.3, 0.05, 1.1, 0.9, DARK);
            window_light(g, c, |g| opening(g, Glow, win_u, yc + 0.3, 0.06, 0.9, 0.7, mul(wc, 0.9)));
        }
        if unit.len > 5.0 && coin(rng, 0.7) {
            // The front door, at the other end of the wall from the window.
            let du = -win_u.signum() * (unit.len * 0.5 - 1.0);
            let dh = (unit.h - 0.6).min(2.0);
            let door_col = if rng.random_bool(0.5) { mul(col, 0.7) } else { mul(pick(rng, &pal.tarp_colors), 0.6) };
            opening(g, Matte, du, unit.y0 + 0.16 + dh * 0.5 + 0.03, 0.05, 1.0, dh + 0.06, DARK);
            opening(g, Matte, du, unit.y0 + 0.16 + dh * 0.5, 0.06, 0.82, dh, door_col);
        }
        if coin(rng, 0.25 + 0.4 * c.wear) {
            place(g, Wood, rng.random_range(-unit.len * 0.3..unit.len * 0.3), yc + rng.random_range(-0.5..0.5), -side * (unit.wid * 0.5 + 0.05), Vec3::new(rng.random_range(0.6..1.4), rng.random_range(0.6..1.2), 0.05), pal.plywood);
        }
        if rng.random_bool(0.25) {
            let tc = mul(pick(rng, &pal.tarp_colors), 0.8);
            let lu = rng.random_range(-unit.len * 0.25..unit.len * 0.25);
            let tl = rng.random_range(2.0..(unit.len - 2.0 * lu.abs()).clamp(2.1, 4.0));
            place(g, Tarp, lu, yc + unit.h * 0.15, side * (unit.wid * 0.5 + 0.1), Vec3::new(tl, unit.h * 0.75, 0.08), tc);
        }
        let exposed = !units.iter().any(|o| o.level == unit.level + 1 && o.contains(unit.local_to_lot(0.0, 0.0)));
        if exposed {
            if rng.random_bool(0.45) {
                let tc = mul(pick(rng, &pal.tarp_colors), 0.85);
                // A sheet no longer than the roof it lies on.
                let lu = rng.random_range(-unit.len * 0.2..unit.len * 0.2);
                let tl = rng.random_range(2.5..(unit.len - 2.0 * lu.abs()).max(2.6));
                place(g, Tarp, lu, unit.top() + 0.08, 0.0, Vec3::new(tl, 0.1, unit.wid * 0.9), tc);
            }
            if rng.random_bool(0.4) {
                let p = unit.local_to_lot(rng.random_range(-unit.len * 0.35..unit.len * 0.35), 0.0);
                dish(g, f, p, unit.top(), 0.7, pal.steel);
            }
            if rng.random_bool(0.35) {
                place(g, Metal, rng.random_range(-unit.len * 0.35..unit.len * 0.35), unit.top() + 0.45, rng.random_range(-0.6..0.6), Vec3::new(0.9, 0.9, 0.9), if rng.random_bool(0.5) { c.palette.rust } else { [0.2, 0.3, 0.6] });
            }
        }
        if rng.random_bool(0.25) {
            let lu = rng.random_range(-unit.len * 0.3..unit.len * 0.3);
            let lv = side * (unit.wid * 0.5 + 0.35);
            if c.inside_local(unit.local_to_lot(lu, lv + side * 0.3)) {
                place(g, Metal, lu, yc - 0.6, lv, Vec3::new(0.8, 0.5, 0.6), if rng.random_bool(0.4) { c.palette.rust } else { [0.5, 0.5, 0.5] });
            }
        }
    }

    // Rusted I-beam props under unsupported corners; timber cribbing where
    // the gap is too short for a beam. Two tall props at one end of a unit
    // are tied into a frame (a header and, higher up, a cross brace); a lone
    // tall one gets a raking strut.
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    for unit in units.iter().filter(|x| x.level > 0) {
        let mut props: [Option<(Vec2, f32)>; 4] = [None; 4];
        for (ci, corner) in unit.corners().into_iter().enumerate() {
            let inset = corner + (Vec2::new(unit.u, unit.v) - corner).normalize_or_zero() * 0.35;
            if units.iter().any(|o| o.level == unit.level - 1 && o.contains(inset)) {
                continue;
            }
            let base = units.iter().filter(|o| o.level < unit.level && o.contains(inset)).map(|o| o.top()).fold(0.0, f32::max);
            let h = unit.y0 - base;
            // Neighbouring units' corners share a prop.
            if placed.iter().any(|(q, y)| q.distance(inset) < 0.7 && (y - unit.y0).abs() < 0.1) {
                continue;
            }
            if h > 0.6 {
                ibeam_support(g, c, rng, inset, unit.yaw, base, unit.y0, base <= 0.0);
                props[ci] = Some((inset, base));
                placed.push((inset, unit.y0));
            } else if h > 0.25 {
                f.bx_yaw(g, Wood, inset.x, base + h * 0.5, inset.y, Vec3::new(0.5, h, 0.3), mul(pal.wood, 0.7), unit.yaw);
            }
        }
        let brace = mul(pal.rust, 0.8);
        // Corners 0 and 3 are one end of the unit, 1 and 2 the other.
        for (i, j) in [(0usize, 3usize), (1, 2)] {
            match (props[i], props[j]) {
                (Some((a, ba)), Some((b, bb))) if (ba - bb).abs() < 0.1 && unit.y0 - ba > 2.0 => {
                    // The plane of the frame, for the braces' faces.
                    let n = f.dir3((b - a).perp()).normalize_or_zero();
                    let top = unit.y0 - 0.12;
                    let m = g.part(Rust);
                    flat_bar(m, f.at2(a, top), f.at2(b, top), 0.2, n, brace);
                    if unit.y0 - ba > 3.2 {
                        flat_bar(m, f.at2(a, ba + 0.5), f.at2(b, top - 0.3), 0.08, n, brace);
                        flat_bar(m, f.at2(b, bb + 0.5), f.at2(a, top - 0.3), 0.08, n, brace);
                    }
                }
                _ => {
                    for (p, base) in [props[i], props[j]].into_iter().flatten() {
                        if unit.y0 - base > 3.2 {
                            // Raking strut from two-thirds up to the ground (or roof)
                            // a metre and a half in under the unit.
                            let foot = p + (Vec2::new(unit.u, unit.v) - p).normalize_or_zero() * 1.5;
                            if units.iter().filter(|o| o.level < unit.level && o.contains(foot)).map(|o| o.top()).fold(0.0, f32::max) == base {
                                let n = f.dir3((foot - p).perp()).normalize_or_zero();
                                flat_bar(g.part(Rust), f.at2(p, base + (unit.y0 - base) * 0.66), f.at2(foot, base + 0.05), 0.12, n, brace);
                            }
                        }
                    }
                }
            }
        }
    }
    // Planks between neighbours.
    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let (a, b) = (units[i], units[j]);
            if a.level != b.level || a.level == 0 {
                continue;
            }
            let ca = Vec2::new(a.u, a.v);
            let cb = Vec2::new(b.u, b.v);
            let dist = ca.distance(cb);
            let reach = (a.len.max(a.wid) + b.len.max(b.wid)) * 0.5;
            if dist > reach + 2.2 || dist < reach * 0.6 {
                continue;
            }
            let mid = (ca + cb) * 0.5;
            if a.contains(mid) || b.contains(mid) {
                continue;
            }
            let d = cb - ca;
            let span = (dist - reach * 0.7).max(1.0);
            f.bx_yaw(g, Wood, mid.x, a.y0 + 0.08, mid.y, Vec3::new(span, 0.12, 0.9), pal.wood, d.y.atan2(d.x));
        }
    }
    // Stair up one side.
    let top_level = units.iter().map(|x| x.level).max().unwrap_or(0);
    if top_level >= 1 {
        // Against a unit on the lot's edge, wherever the lot actually is.
        let sv = rng.random_range((lo.y + hi.y) * 0.5 - 3.0..(lo.y + hi.y) * 0.5 + 3.0);
        let mut su = lo.x + 1.4;
        while su < hi.x - 2.0 && !(c.inside_local(Vec2::new(su - 1.0, sv - 2.0)) && c.inside_local(Vec2::new(su + 2.5, sv + 2.0)) && c.inside_local(Vec2::new(su - 1.0, sv + 2.0)) && c.inside_local(Vec2::new(su + 2.5, sv - 2.0))) {
            su += 1.0;
        }
        if su < hi.x - 2.0 {
            stair_zigzag(g, c, su, sv, top_level, 2.6, 3.4, steel);
        }
    }
    // Utility pole and wires that reach real units.
    let pole = {
        let mut p = Vec2::new(hi.x - 1.0, lo.y + 0.8);
        let anchor = interior_anchor(&c.lot_poly);
        for _ in 0..8 {
            if c.inside_local(p) {
                break;
            }
            p = p.lerp(anchor, 0.25);
        }
        p
    };
    let pole_h = 9.0;
    f.bx(g, Wood, pole.x, pole_h * 0.5, pole.y, Vec3::new(0.3, pole_h, 0.3), pal.bark);
    f.bx(g, Wood, pole.x, pole_h - 0.6, pole.y, Vec3::new(1.8, 0.12, 0.12), pal.bark);
    f.bx(g, Glow, pole.x, pole_h - 1.6, pole.y - 0.5, Vec3::new(0.5, 0.25, 0.4), pal.street_lamp);
    let pole_top = f.at(pole.x, pole_h - 0.6, pole.y);
    let upper: Vec<&Unit> = units.iter().filter(|x| x.level >= 1).collect();
    for unit in upper.choose_multiple(rng, 3) {
        let corner = unit.corners()[rng.random_range(0..4)];
        if !c.spans_lot(pole, corner) {
            continue;
        }
        g.part(Matte).wire(pole_top, f.at2(corner, unit.top() + 0.1), 0.6, 0.05, rgba(DARK));
    }
    for _ in 0..rng.random_range(2..5) {
        let (Some(a), Some(b)) = (upper.choose(rng), upper.choose(rng)) else { break };
        if std::ptr::eq(*a, *b) {
            continue;
        }
        let pa = Vec2::new(a.u, a.v);
        let pb = Vec2::new(b.u, b.v);
        if pa.distance(pb) < 3.0 || !c.spans_lot(pa, pb) {
            continue;
        }
        let from = f.at2(pa, a.top() + 0.1);
        let to = f.at2(pb, b.top() + 0.1);
        g.part(Matte).wire(from, to, 0.4, 0.04, rgba(DARK));
        if rng.random_bool(0.5) {
            for k in 1..4 {
                let t = k as f32 / 4.0;
                let p = from.lerp(to, t) - Vec3::Y * (0.4 * 4.0 * t * (1.0 - t));
                let tc = pick(rng, &pal.tarp_colors);
                g.part(Tarp).boxed(Vec3::new(p.x, p.y - 0.4, p.z), Vec3::new(0.5, 0.7, 0.04), f.yaw() + rng.random_range(-0.5..0.5), rgba(mul(tc, 1.4)));
            }
        }
    }
    // Street level: a drum fire and junk.
    let front = c.front_face(&c.lot_poly);
    if rng.random_bool(0.5) {
        drum_fire(g, f, pal, front.at(rng.random_range(-front.len * 0.3..front.len * 0.3), -1.6));
    }
    if rng.random_bool(0.4) {
        let p = front.at(rng.random_range(-front.len * 0.3..front.len * 0.3), -4.0);
        let yaw = front.yaw() + rng.random_range(-0.3..0.3);
        if vehicle_fits(c, p, yaw, Vehicle::Wreck) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, yaw, Vehicle::Wreck, col);
        }
    }
}

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
                    window_light(g, c, |g| face.decal(g, f, Glow, t, y + 1.2, 0.05, 2.0, 2.1, col));
                } else {
                    face.decal(g, f, Glass, t, y + 1.2, 0.05, 2.0, 2.1, pal.glass_dark);
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
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).clamp(3, 8);
    let h = floors as f32 * fh;
    let wall = mul(pick(rng, &pal.brick), 1.0 - 0.2 * c.wear);
    let poly = &c.poly;
    let depth = (inradius(poly) * 0.42).clamp(5.0, 10.0);
    let inner = offset_poly(poly, depth);
    let mut spec = WindowSpec::from_ctx(c);
    spec.spacing = 2.6;
    spec.w = 1.2;
    spec.h = 1.6;
    // Wings along every edge. A wing is a box behind its face; where the
    // lot narrows (a wedge, a notch) the box is shortened until its back
    // corners are on the lot.
    for face in c.faces(poly) {
        if face.len < 4.0 {
            continue;
        }
        let mut half = face.len * 0.5 + 0.2;
        while half > 1.5 && !(c.inside_local(face.at(-half, -depth)) && c.inside_local(face.at(half, -depth))) {
            half -= 0.5;
        }
        face.bx(g, f, Brick, 0.0, h * 0.5, -depth * 0.5, Vec3::new(half * 2.0, h, depth), wall);
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
    parapet(g, c, poly, h, 0.7, 0.3, Brick, wall);
    // Courtyard garden.
    ground(g, f, Paving, &inner, 0.05, pal.paving);
    let cc = poly_center(&inner);
    let r = inradius(&inner);
    if r > 3.0 {
        tree_in(g, f, pal, cc, 4.5, 4.0, pal.foliage);
        for k in 0..3 {
            let a = k as f32 / 3.0 * std::f32::consts::TAU;
            let p = cc + Vec2::new(a.cos(), a.sin()) * r * 0.55;
            lamp_post(g, f, p, 3.2, pal.lamp);
            bench_world(g, f.to_world(p.x + 0.8, p.y), f.yaw() + a, pal.wood, CHARCOAL);
        }
    }
    // Arched gateway on the front.
    let front = c.front_face(poly);
    front.bx(g, f, Matte, 0.0, 2.2, -depth * 0.5, Vec3::new(3.4, 4.4, depth + 0.3), DARK);
    front.bx(g, f, Glow, 0.0, 4.0, -depth * 0.5, Vec3::new(2.0, 0.1, 1.0), mul(pal.lamp, 0.8));
    roof_gear(g, c, rng, poly, h, 0.8);
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
            g.part(Glow).boxed(Vec3::new(crown.x, top_y + 0.15, crown.z), Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
        } else {
            g.part(Glow).boxed(Vec3::new(crown.x, podium_h + h + 0.15, crown.z), Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
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
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let floors = (c.floors as i32).clamp(6, 22);
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
    if roof.len() >= 3 && inradius(&roof) > 3.0 {
        pool(g, c, poly_center(&roof).x, poly_center(&roof).y, inradius(&roof) * 0.8, inradius(&roof) * 0.5);
        let w = f.to_world(poly_center(&roof).x + inradius(&roof) * 0.6, poly_center(&roof).y);
        tree_kind(g, w, top + 0.35, TreeKind::Round, 1.9, 2.6, pal.bark, pal.foliage);
    }
    antenna_mast(g, c, poly_center(&prev), top + 0.35, 4.0);
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
