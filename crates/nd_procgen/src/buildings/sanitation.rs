//! [society] The trash depot (`nd_sim::ServiceKind::TrashDepot`): a fenced
//! asphalt yard with a corrugated transfer hall (roller doors, a tipping
//! floor showing through the open bays), garbage trucks parked in a row,
//! skips and roll-off containers, a weighbridge hut at the gate and, at the
//! top tier, a recycling sorting tower with a conveyor up its side. A low
//! tier is little more than the yard, skips and a shed.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

const SANITATION_GREEN: [f32; 3] = [0.16, 0.42, 0.22];
const HAZARD: [f32; 3] = [0.95, 0.7, 0.08];

pub fn trash_depot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let _yard = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.15));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    // The transfer hall at the back, its open bays facing the yard.
    let share = [0.55, 0.7, 0.75][t];
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, hi.y - d * 0.22, w * share * 0.5, d * 0.2));
    let wall = mul(pick(rng, &pal.metal_paint), 0.75);
    let hh = [4.5, 7.0, 9.0][t];
    if hall.len() >= 3 && inradius(&hall) > 1.5 {
        extrude(g, f, WornMetal, &hall, 0.0, hh, wall);
        parapet(g, c, &hall, hh, 0.5, 0.2, Metal, mul(wall, 0.8));
        // Corrugation: ribs down every long face.
        for face in c.faces(&hall) {
            if face.len < 4.0 {
                continue;
            }
            let ribs = (face.len / 1.2) as i32;
            for k in 0..ribs {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / ribs as f32;
                face.bx(g, f, Metal, tt, hh * 0.5, 0.05, Vec3::new(0.08, hh * 0.98, 0.1), mul(wall, 0.85));
            }
            grime(g, c, rng, &face, 0.0, hh, 0.8);
        }
        // The yard side: bays, half shut, the tipping floor dark inside.
        let front = c.front_face(&hall);
        let bays = ([1, 2, 3][t] as i32).min(((front.len - 2.0) / 5.0).floor().max(1.0) as i32);
        let bw = ((front.len - 1.0) / bays as f32).min(5.2);
        for k in 0..bays {
            let tt = -front.len * 0.5 + 0.5 + (k as f32 + 0.5) * bw;
            if k % 2 == 0 {
                front.bx(g, f, Matte, tt, 2.3, 0.06, Vec3::new(bw - 0.8, 4.4, 0.08), DARK);
                front.bx(g, f, Metal, tt, 4.0, 0.12, Vec3::new(bw - 0.8, 1.2, 0.1), mul(pal.steel, 0.6));
            } else {
                roller_door(g, c, &front, tt, bw - 0.8, 4.4, mul(pal.steel, 0.55));
            }
            // Hazard stripes on the bay posts.
            front.bx(g, f, Matte, tt - bw * 0.5 + 0.25, 1.0, 0.15, Vec3::new(0.3, 2.0, 0.3), HAZARD);
        }
        // A green band and the sign.
        front.bx(g, f, Panel, 0.0, hh - 0.8, 0.12, Vec3::new(front.len * 0.9, 0.9, 0.1), SANITATION_GREEN);
        front.decal(g, f, Glow, 0.0, hh - 0.8, 0.2, (front.len * 0.4).min(8.0), 0.45, mul([0.4, 2.0, 0.6], 0.7));
        roof_gear(g, c, rng, &hall, hh, 0.15);
    }
    // Garbage trucks parked in a row facing the gate.
    let trucks = [1, 3, 5][t];
    let mut placed = 0;
    for k in 0..12 {
        if placed >= trucks {
            break;
        }
        let u = lo.x + 3.0 + k as f32 * 3.6;
        let p = Vec2::new(u, lo.y + d * 0.42);
        if vehicle_fits(c, p, -FRAC_PI_2, Vehicle::Truck) && hall.iter().all(|_| !nd_sim::zoning::point_in_polygon(p, &hall)) {
            vehicle(g, f, pal, rng, p, 0.0, -FRAC_PI_2, Vehicle::Truck, SANITATION_GREEN);
            placed += 1;
        }
    }
    // Skips and roll-off containers along one side, some heaped.
    let skips = [3, 4, 6][t];
    for k in 0..skips {
        let v = lo.y + 4.0 + k as f32 * 3.2;
        let p = Vec2::new(hi.x - 2.2, v);
        if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.6 || nd_sim::zoning::point_in_polygon(p, &hall) {
            continue;
        }
        let col = pick(rng, &[[0.55, 0.3, 0.1], [0.2, 0.32, 0.5], SANITATION_GREEN, [0.5, 0.45, 0.12]]);
        f.bx(g, Rust, p.x, 0.6, p.y, Vec3::new(2.6, 1.2, 1.6), mul(col, 0.85));
        for _ in 0..rng.random_range(1..4) {
            let q = p + Vec2::new(rng.random_range(-0.9..0.9), rng.random_range(-0.5..0.5));
            f.bx(g, Tarp, q.x, 1.35, q.y, Vec3::new(0.55, 0.45, 0.5), [0.06, 0.06, 0.07]);
        }
    }
    // A recycling tower with a conveyor running up to it.
    if t == 2 && hall.len() >= 3 {
        let (hl, hh2) = bounds(&hall);
        let tower = fit_inside(c, &rect(hl.x + 3.0, hh2.y - 3.0, 2.5, 2.5));
        if tower.len() >= 3 && inradius(&tower) > 1.2 {
            let th = hh + 8.0;
            extrude(g, f, Panel, &tower, 0.0, th, mul(pal.concrete(0), 0.9));
            parapet(g, c, &tower, th, 0.6, 0.2, Metal, mul(pal.steel, 0.7));
            let base = Vec2::new(hl.x + 9.0, lo.y + d * 0.35);
            if c.inside_local(base) {
                let top = interior_anchor(&tower);
                g.part(Metal).beam(f.at2(base, 0.6), f.at2(top, th - 1.0), 0.9, rgba(mul(pal.steel, 0.75)));
            }
            g.part(Glow).boxed(f.at2(interior_anchor(&tower), th + 0.5), Vec3::new(1.2, 0.3, 1.2), 0.0, rgba(mul([0.4, 2.0, 0.6], 0.8)));
        }
    }
    // Weighbridge hut and plate at the gate, a fence round the rest.
    let gate = Vec2::new(lo.x + w * 0.8, lo.y + 2.0);
    if c.inside_local(gate) && distance_to_boundary(gate, &c.lot_poly) > 1.2 {
        f.bx(g, Panel, gate.x, 1.3, gate.y, Vec3::new(2.4, 2.6, 2.0), mul(pal.panel(0), 0.9));
        f.bx(g, Glass, gate.x, 1.6, gate.y - 1.01, Vec3::new(1.6, 0.8, 0.04), pal.glass_dark);
        let plate = Vec2::new(gate.x - 4.5, gate.y + 2.0);
        if c.all_inside(&[Vec2::new(-1.7, -3.6), Vec2::new(1.7, -3.6), Vec2::new(-1.7, 3.6), Vec2::new(1.7, 3.6)].map(|o| plate + o)) {
            f.bx(g, Metal, plate.x, 0.08, plate.y, Vec3::new(3.2, 0.16, 7.0), mul(pal.steel, 0.6));
        }
    }
    let edge = offset_poly(&c.lot_poly, 0.6);
    let n = edge.len();
    for i in 0..n {
        let (a, b) = (edge[i], edge[(i + 1) % n]);
        // Leave the front open for the trucks.
        if (a.y - lo.y).abs() < 1.5 && (b.y - lo.y).abs() < 1.5 {
            continue;
        }
        fence(g, c, a, b, 2.4, t == 0);
    }
    floodlight(g, c, Vec2::new(lo.x + 2.0, lo.y + d * 0.55), 8.0);
}

// ── [r5-trash] the disposal sites ───────────────────────────────────────────

const BAG_COLORS: [[f32; 3]; 7] = [[0.05, 0.05, 0.06], [0.08, 0.14, 0.07], [0.12, 0.2, 0.42], [0.62, 0.6, 0.55], [0.4, 0.3, 0.18], [0.55, 0.12, 0.1], [0.07, 0.07, 0.08]];
const COVER_SOIL: [f32; 3] = [0.24, 0.2, 0.15];
const RAW_TRASH: [f32; 3] = [0.2, 0.18, 0.15];
const LEACHATE: [f32; 3] = [0.06, 0.08, 0.03];
const FURNACE: [f32; 3] = [2.4, 0.9, 0.2];
const SOOT: [f32; 3] = [0.13, 0.13, 0.14];

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// A rough ring of `n` points round `centre` (half sizes `h`), its radii
/// jittered, pulled in until every point lies inside the lot.
fn blob(c: &Ctx, rng: &mut impl Rng, centre: Vec2, h: Vec2, n: usize, jitter: f32) -> Vec<Vec2> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            // A squarish ellipse fills a rectangular site.
            let (ca, sa) = (a.cos(), a.sin());
            let k = 1.0 / (ca.abs().powf(3.0) + sa.abs().powf(3.0)).powf(1.0 / 3.0);
            let r = 1.0 - rng.random_range(0.0..jitter);
            let mut p = centre + Vec2::new(ca * h.x, sa * h.y) * k * r;
            for _ in 0..12 {
                if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.0 {
                    break;
                }
                p = centre + (p - centre) * 0.85;
            }
            p
        })
        .collect()
}

fn shrink(poly: &[Vec2], centre: Vec2, k: f32, shift: Vec2) -> Vec<Vec2> {
    poly.iter().map(|p| centre + shift + (*p - centre) * k).collect()
}

/// Bags, cartons, tyres and junk strewn over `poly` at height `y`: `n` of them.
fn junk(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32, n: usize) {
    let f = &c.frame;
    let (centre, r) = (poly_center(poly), poly_radius(poly).max(1.0));
    for _ in 0..n {
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        let p = centre + Vec2::new(a.cos(), a.sin()) * rng.random_range(0.0..1.0f32).sqrt() * r;
        if !nd_sim::zoning::point_in_polygon(p, poly) || !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.1 {
            continue;
        }
        let s = Vec3::new(rng.random_range(0.5..1.5), rng.random_range(0.3..0.9), rng.random_range(0.5..1.4));
        let col = mul(pick(rng, &BAG_COLORS), rng.random_range(0.6..1.05));
        let surf = if rng.random_bool(0.15) { Rust } else if rng.random_bool(0.5) { Tarp } else { Matte };
        f.bx_yaw(g, surf, p.x, y + s.y * 0.25, p.y, s, col, rng.random_range(0.0..3.0));
    }
}

/// The fence round a site, its front left open at `gate` (lot-local u) for
/// `half` metres either side (or all of the front when `half` is None).
fn site_fence(g: &mut Geometry, c: &Ctx, lo_v: f32, gate: Option<(f32, f32)>, h: f32, barbed: bool) {
    let edge = offset_poly(&c.lot_poly, 0.6);
    let n = edge.len();
    for i in 0..n {
        let (a, b) = (edge[i], edge[(i + 1) % n]);
        let front = (a.y - lo_v).abs() < 1.5 && (b.y - lo_v).abs() < 1.5;
        if front && gate.is_none_or(|(u, half)| ((a.x + b.x) * 0.5 - u).abs() < half) {
            continue;
        }
        fence(g, c, a, b, h, barbed);
    }
}

/// [r5-trash] A landfill (`nd_sim::ServiceKind::Landfill`): a fenced tip on
/// bare dirt, the mound behind growing with `c.waste_fill` in steep
/// terraces (old lifts capped with soil and weeds, the top lift raw black
/// garbage heaped with bags and junk, a compactor and a truck tipping on
/// it, a haul ramp up the front), a leachate pond, gas vents (a flare on a
/// big tip), sitting gulls, litter on the fence, the weighbridge and site
/// office. Empty, it is a lined pit.
pub fn landfill(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let fill = c.waste_fill.clamp(0.0, 1.0);
    yard(g, c, 0.4, Dirt, mul(COVER_SOIL, 0.8));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    // The cell: the back three quarters of the site.
    let mut centre = Vec2::new((lo.x + hi.x) * 0.5, lo.y + d * 0.58);
    if !c.inside_local(centre) {
        centre = interior_anchor(&c.lot_poly);
    }
    let mut half = Vec2::new(w * 0.5 - 4.0, d * 0.36).max(Vec2::splat(4.0));
    let mut base = blob(c, rng, centre, half, 22, 0.1);
    // An odd lot (an L, a triangle): the cell shrinks until it and its
    // berm lie on it.
    for _ in 0..20 {
        if poly_inside(c, &shrink(&base, centre, 1.06, Vec2::ZERO)) {
            break;
        }
        base = shrink(&base, centre, 0.9, Vec2::ZERO);
        half *= 0.9;
    }
    // The liner: a black membrane in the pit, a berm round it.
    ground(g, f, Matte, &base, 0.06, [0.03, 0.03, 0.035]);
    let berm = shrink(&base, centre, 1.05, Vec2::ZERO);
    loft(g, f, Dirt, &berm, &base, 0.0, 1.0, mul(COVER_SOIL, 0.95));
    let size = half.x.min(half.y);
    let top_h = (size * 0.55).min([11.0, 18.0, 26.0][t]);
    let height = 0.6 + fill * top_h;
    let lifts = 1 + (fill * 5.0).ceil() as usize;
    // The working face is at the front (towards the gate): the mound leans back.
    let lean = Vec2::new(0.0, d * 0.05);
    let mut ring = base.clone();
    let mut y = 1.0;
    if fill > 0.0 {
        for k in 0..lifts {
            let f1 = (k + 1) as f32 / lifts as f32;
            let kk = 1.0 - 0.5 * f1;
            let next = shrink(&base, centre, kk, lean * f1);
            let y1 = 1.0 + height * f1;
            // Old lifts are capped and greening; the top one is raw trash.
            let raw = k + 1 == lifts;
            let col = if raw { mul(RAW_TRASH, rng.random_range(0.9..1.1)) } else { lerp3(COVER_SOIL, [0.19, 0.25, 0.1], (1.0 - f1) * 0.7) };
            loft(g, f, Dirt, &ring, &next, y, y1, col);
            if !raw {
                // A bench: the next lift starts a little in.
                let bench = shrink(&base, centre, kk - 0.025, lean * f1);
                loft(g, f, Dirt, &next, &bench, y1, y1 + 0.05, mul(COVER_SOIL, 1.05));
                ring = bench;
                // Weeds and scrub on the capped bench.
                for _ in 0..6 {
                    let i = rng.random_range(0..next.len());
                    let p = next[i].lerp(centre, 0.02);
                    f.bx_yaw(g, Foliage, p.x, y1 + 0.3, p.y, Vec3::new(1.2, 0.6, 1.0), [0.16, 0.24, 0.08], rng.random_range(0.0..3.0));
                }
            } else {
                ring = next;
            }
            y = y1;
        }
    }
    let top = ring.clone();
    let top_y = y;
    let top_c = poly_center(&top);
    let r_top = poly_radius(&top).max(2.0);
    // The raw face: black garbage under heaps of bags, cartons and junk.
    ground(g, f, Matte, &top, top_y + 0.04, [0.09, 0.085, 0.08]);
    let bags = ((60.0 + 260.0 * fill) * [1.0, 1.6, 2.4][t]) as usize;
    junk(g, c, rng, &top, top_y, bags);
    // Heaps where the trucks tipped.
    for _ in 0..(2.0 + fill * 6.0) as usize {
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        let p = top_c + Vec2::new(a.cos(), a.sin()) * rng.random_range(0.0..r_top * 0.6);
        if !nd_sim::zoning::point_in_polygon(p, &top) {
            continue;
        }
        let hr = rng.random_range(2.0..4.5f32).min(r_top * 0.4);
        let heap = (0..8).map(|i| p + Vec2::from_angle(i as f32 / 8.0 * std::f32::consts::TAU) * hr * rng.random_range(0.8..1.1)).collect::<Vec<_>>();
        if !poly_inside(c, &heap) {
            continue;
        }
        let peak = shrink(&heap, p, 0.3, Vec2::ZERO);
        loft(g, f, Tarp, &heap, &peak, top_y, top_y + hr * 0.45, mul(pick(rng, &BAG_COLORS), 0.7));
    }
    if fill > 0.05 {
        // A haul ramp up the front of the mound.
        let foot = Vec2::new(centre.x - half.x * 0.35, lo.y + d * 0.2);
        let head = top_c + Vec2::new(-r_top * 0.4, -r_top * 0.5);
        if poly_inside(c, &[foot + Vec2::new(-3.0, 0.0), foot + Vec2::new(3.0, 0.0), head + Vec2::new(3.0, 0.0), head + Vec2::new(-3.0, 0.0)]) {
            g.part(Dirt).beam(f.at2(foot, 0.3), f.at2(head, top_y - 0.2), 5.0, rgba(mul(COVER_SOIL, 0.9)));
        }
        // A compactor (yellow, studded drums) and a truck tipping at the face.
        let cp = top_c + Vec2::new(r_top * 0.3, -r_top * 0.2);
        if nd_sim::zoning::point_in_polygon(cp, &top) {
            f.bx_yaw(g, Metal, cp.x, top_y + 1.3, cp.y, Vec3::new(2.6, 1.6, 4.0), HAZARD, 0.4);
            f.bx_yaw(g, Glass, cp.x, top_y + 2.6, cp.y, Vec3::new(1.6, 1.0, 1.6), pal.glass_dark, 0.4);
            for s in [-1.0f32, 1.0] {
                let q = cp + Vec2::new(0.4f32.sin(), 0.4f32.cos()) * 2.4 * s;
                f.bx_yaw(g, Rust, q.x, top_y + 0.8, q.y, Vec3::new(2.8, 1.6, 1.0), mul(pal.steel, 0.5), 0.4);
            }
        }
        let tp = top_c + Vec2::new(-r_top * 0.25, -r_top * 0.3);
        if nd_sim::zoning::point_in_polygon(tp, &top) {
            vehicle(g, f, pal, rng, tp, top_y, 0.3, Vehicle::GarbageTruck, SANITATION_GREEN);
        }
        // Gulls sitting on the heap (the renderer flies the rest).
        for _ in 0..(10.0 + 30.0 * fill) as usize {
            let a = rng.random_range(0.0..std::f32::consts::TAU);
            let p = top_c + Vec2::new(a.cos(), a.sin()) * rng.random_range(0.0..r_top * 0.9);
            if nd_sim::zoning::point_in_polygon(p, &top) {
                f.bx_yaw(g, Matte, p.x, top_y + 0.95, p.y, Vec3::new(0.2, 0.18, 0.42), [0.9, 0.9, 0.87], rng.random_range(0.0..3.0));
            }
        }
        // Gas vents in the capped lifts; a flare on a big tip.
        let vents = [3, 5, 8][t];
        for k in 0..vents {
            let a = k as f32 / vents as f32 * std::f32::consts::TAU + 0.3;
            let p = centre + Vec2::new(a.cos() * half.x, a.sin() * half.y) * 0.72;
            if !c.inside_local(p) {
                continue;
            }
            let vy = 1.0 + height * 0.3;
            g.part(Metal).cylinder(f.to_world(p.x, p.y), 0.2, 0.2, vy - 0.5, vy + 1.8, 6, rgba(mul(pal.steel, 0.55)));
        }
        if t >= 1 && fill > 0.3 {
            let p = Vec2::new(hi.x - 5.0, lo.y + d * 0.25);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
                let w0 = f.to_world(p.x, p.y);
                g.part(Metal).cylinder(w0, 0.4, 0.35, 0.0, 7.0, 8, rgba(mul(pal.steel, 0.5)));
                g.part(Glow).cylinder(w0, 0.35, 0.05, 7.0, 9.0, 6, rgba(FURNACE));
            }
        }
    }
    // The leachate pond at a front corner, black-green and still.
    let pond_c = Vec2::new(lo.x + w * 0.16, lo.y + d * 0.14);
    let pond = blob(c, rng, pond_c, Vec2::new((w * 0.11).clamp(3.0, 10.0), (d * 0.08).clamp(2.5, 6.0)), 12, 0.25);
    if poly_inside(c, &shrink(&pond, pond_c, 1.25, Vec2::ZERO)) && !pond.iter().any(|p| nd_sim::zoning::point_in_polygon(*p, &berm)) {
        loft(g, f, Dirt, &shrink(&pond, pond_c, 1.25, Vec2::ZERO), &pond, 0.0, 0.5, mul(COVER_SOIL, 0.7));
        ground(g, f, Glass, &pond, 0.42, LEACHATE);
    }
    // Litter blown against the fence and over the yard.
    for _ in 0..(40.0 + 120.0 * fill) as usize {
        let p = Vec2::new(rng.random_range(lo.x..hi.x.max(lo.x + 0.1)), rng.random_range(lo.y..hi.y.max(lo.y + 0.1)));
        let p = if rng.random_bool(0.6) { Vec2::new(p.x, if rng.random_bool(0.5) { lo.y + 1.0 } else { hi.y - 1.0 }) } else { p };
        if c.inside_local(p) && !nd_sim::zoning::point_in_polygon(p, &base) {
            f.bx_yaw(g, Tarp, p.x, 0.15 + rng.random_range(0.0..1.6), p.y, Vec3::new(0.45, 0.35, 0.05), pick(rng, &BAG_COLORS), rng.random_range(0.0..3.0));
        }
    }
    // The gate: weighbridge, a portacabin office, floodlights.
    let gate = Vec2::new(lo.x + w * 0.72, lo.y + 3.0);
    if c.inside_local(gate) && distance_to_boundary(gate, &c.lot_poly) > 1.2 {
        let plate = gate + Vec2::new(0.0, 3.0);
        if c.all_inside(&[Vec2::new(-1.7, -3.6), Vec2::new(1.7, -3.6), Vec2::new(-1.7, 3.6), Vec2::new(1.7, 3.6)].map(|o| plate + o)) {
            f.bx(g, Metal, plate.x, 0.08, plate.y, Vec3::new(3.2, 0.16, 7.0), mul(pal.steel, 0.55));
        }
        let cab = gate + Vec2::new(5.0, 1.0);
        if c.all_inside(&[Vec2::new(-3.1, -1.4), Vec2::new(3.1, -1.4), Vec2::new(-3.1, 1.4), Vec2::new(3.1, 1.4)].map(|o| cab + o)) {
            f.bx(g, WornPanel, cab.x, 1.3, cab.y, Vec3::new(6.0, 2.6, 2.6), mul(pal.panel(0), 0.75));
            f.bx(g, Glass, cab.x, 1.5, cab.y - 1.31, Vec3::new(2.5, 0.8, 0.04), pal.glass_dark);
            f.bx(g, Panel, cab.x, 2.75, cab.y - 1.32, Vec3::new(3.5, 0.5, 0.06), SANITATION_GREEN);
        }
    }
    site_fence(g, c, lo.y, Some((gate.x, 6.0)), 3.0, true);
    floodlight(g, c, Vec2::new(lo.x + 2.5, lo.y + d * 0.5), 9.0);
    floodlight(g, c, Vec2::new(hi.x - 2.5, lo.y + d * 0.5), 9.0);
}

/// [r5-trash] Where an incinerator's stack stands on a lot (lot-local),
/// its radius and height: the drawing and the smoke share it.
fn incinerator_stack(lot_poly: &[Vec2], tier: nd_sim::Tier) -> (Vec2, f32, f32) {
    let (lo, hi) = bounds(lot_poly);
    let t = tier.index();
    let p = Vec2::new(lo.x + (hi.x - lo.x) * 0.8, lo.y + (hi.y - lo.y) * 0.74);
    (p, [2.0, 2.6, 3.2][t], [38.0, 52.0, 70.0][t])
}

/// [r5-trash] Where an incinerator's smoke leaves its stack, for the
/// renderer's plume (`render/haze.rs`).
pub fn incinerator_vents(lot: &nd_sim::Lot, tier: nd_sim::Tier) -> Vec<super::CoalVent> {
    let (frame, poly) = super::Frame::lot_local(lot);
    let (p, r, h) = incinerator_stack(&poly, tier);
    // No stack drawn on a lot too thin for it: no smoke either.
    if !nd_sim::zoning::point_in_polygon(p, &poly) || distance_to_boundary(p, &poly) <= r * 1.5 + 0.8 {
        return Vec::new();
    }
    let w = frame.to_world(p.x, p.y);
    vec![super::CoalVent { top: Vec3::new(w.x, h + 0.6, w.y), r: r * 0.7, steam: false }]
}

/// [r5-trash] An incinerator (waste-to-energy): a tall soot-dark boiler
/// house at the back, the low tipping hall in front with its bunker doors
/// and a glowing band, trucks queueing at them, the flue-gas scrubbers
/// (towers, a duct bridge) and the banded stack, the air-cooled condenser
/// on its legs with its fans, the furnace glowing through slits, ash skips.
pub fn incinerator(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.0));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let (sp, sr, sh) = incinerator_stack(&c.lot_poly, c.tier);
    let boiler_h = [20.0, 28.0, 36.0][t];
    let hall_h = boiler_h * 0.45;
    // The boiler house: the back of the site, left of the stack.
    let boiler = fit_inside(c, &rect(lo.x + w * 0.38, lo.y + d * 0.68, w * 0.28, d * 0.2));
    let mut tipping_front: Option<Face> = None;
    if boiler.len() >= 3 && inradius(&boiler) > 2.0 && poly_inside(c, &boiler) {
        extrude(g, f, WornMetal, &boiler, 0.0, boiler_h, SOOT);
        parapet(g, c, &boiler, boiler_h, 0.8, 0.25, Metal, mul(SOOT, 0.8));
        for face in c.faces(&boiler) {
            if face.len < 5.0 {
                continue;
            }
            // Ribs, furnace light through tall slits, soot.
            let ribs = (face.len / 3.0) as i32;
            for k in 0..ribs {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / ribs as f32;
                face.bx(g, f, Metal, tt, boiler_h * 0.5, 0.08, Vec3::new(0.25, boiler_h * 0.98, 0.15), mul(SOOT, 1.3));
                if k % 2 == 1 {
                    face.decal(g, f, Glow, tt, boiler_h * 0.5, 0.18, 0.3, boiler_h * 0.45, mul(FURNACE, 0.35));
                }
            }
            grime(g, c, rng, &face, 0.0, boiler_h, 1.0);
        }
        // A penthouse and fans on the roof.
        let (bl, bh) = bounds(&boiler);
        let ph = fit_inside(c, &rect((bl.x + bh.x) * 0.5, (bl.y + bh.y) * 0.5, (bh.x - bl.x) * 0.25, (bh.y - bl.y) * 0.3));
        if ph.len() >= 3 && poly_inside(c, &ph) {
            extrude(g, f, Metal, &ph, boiler_h, boiler_h + 4.0, mul(SOOT, 1.2));
        }
        // The tipping hall in front, low, its bays open to the yard.
        let hall = fit_inside(c, &rect((bl.x + bh.x) * 0.5, bl.y - d * 0.12, (bh.x - bl.x) * 0.5 + 2.0, d * 0.12));
        if hall.len() >= 3 && inradius(&hall) > 1.5 && poly_inside(c, &hall) {
            extrude(g, f, WornPanel, &hall, 0.0, hall_h, [0.32, 0.3, 0.28]);
            parapet(g, c, &hall, hall_h, 0.5, 0.2, Metal, mul(SOOT, 1.4));
            let front = c.front_face(&hall);
            let bays = [2, 3, 4][t].min(((front.len - 2.0) / 5.5).floor().max(1.0) as i32);
            for k in 0..bays {
                let tt = -front.len * 0.5 + (k as f32 + 0.5) * front.len / bays as f32;
                front.bx(g, f, Matte, tt, 2.8, 0.06, Vec3::new(4.2, 5.4, 0.08), DARK);
                front.bx(g, f, Matte, tt - 2.3, 1.1, 0.15, Vec3::new(0.3, 2.2, 0.3), HAZARD);
                front.bx(g, f, Matte, tt + 2.3, 1.1, 0.15, Vec3::new(0.3, 2.2, 0.3), HAZARD);
            }
            front.bx(g, f, Panel, 0.0, hall_h - 0.9, 0.12, Vec3::new(front.len * 0.85, 1.0, 0.1), [0.35, 0.12, 0.06]);
            front.decal(g, f, Glow, 0.0, hall_h - 0.9, 0.2, (front.len * 0.55).min(14.0), 0.45, mul([2.2, 0.8, 0.25], 0.9));
            tipping_front = Some(front);
            for face in c.faces(&hall) {
                grime(g, c, rng, &face, 0.0, hall_h, 0.8);
            }
        }
        // Ducts from the boiler to the scrubbers and the stack, on a bridge.
        let duct_y = boiler_h * 0.7;
        let mid_v = (bl.y + bh.y) * 0.5;
        let sx = sp.x - sr * 3.0;
        let duct = [Vec2::new(bh.x, mid_v), Vec2::new(sx, mid_v), Vec2::new(sx, sp.y), Vec2::new(sp.x - sr, sp.y)];
        if duct.iter().all(|p| c.inside_local(*p) && distance_to_boundary(*p, &c.lot_poly) > 1.4) {
            pipe_run(g, f, &duct.map(|p| Vec3::new(p.x, duct_y, p.y)), 1.2, mul(pal.steel, 0.55));
        }
        // Scrubber towers between the boiler and the stack.
        for k in 0..[2, 2, 3][t] {
            let p = Vec2::new(sx - 1.0, lo.y + d * 0.42 - k as f32 * 4.6);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.6 {
                tank_vertical(g, f, p, 2.0, 0.0, boiler_h * 0.75, Metal, mul(pal.steel, 0.85));
            }
        }
    }
    // The air-cooled condenser: a deck on legs, fans on top.
    let acc = Vec2::new(lo.x + w * 0.14, lo.y + d * 0.62);
    let acc_h = boiler_h * 0.5;
    if c.all_inside(&[acc + Vec2::new(-5.0, -7.0), acc + Vec2::new(5.0, 7.0), acc + Vec2::new(-5.0, 7.0), acc + Vec2::new(5.0, -7.0)]) {
        for du in [-4.5f32, 4.5] {
            for dv in [-6.0f32, 0.0, 6.0] {
                f.bx(g, Matte, acc.x + du, acc_h * 0.5, acc.y + dv, Vec3::new(0.4, acc_h, 0.4), mul(pal.steel, 0.5));
            }
        }
        f.bx(g, Metal, acc.x, acc_h + 1.0, acc.y, Vec3::new(10.0, 2.0, 14.0), mul(pal.steel, 0.65));
        for k in 0..6 {
            let p = acc + Vec2::new(if k % 2 == 0 { -2.5 } else { 2.5 }, -4.6 + (k / 2) as f32 * 4.6);
            g.part(Matte).cylinder(f.to_world(p.x, p.y), 2.0, 2.0, acc_h + 2.0, acc_h + 2.3, 12, rgba(DARK));
        }
    }
    // The stack.
    if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > sr * 1.5 + 0.8 {
        let ws = f.to_world(sp.x, sp.y);
        g.part(Concrete).cylinder(ws, sr * 1.45, sr * 1.3, 0.0, 3.0, 16, rgba(mul(pal.concrete(0), 0.6)));
        g.part(Concrete).cylinder_capped(ws, sr, sr * 0.78, 3.0, sh * 0.88, 16, false, false, rgba(mul(pal.concrete(0), 0.72)));
        g.part(Matte).cylinder_capped(ws, sr * 0.78, sr * 0.74, sh * 0.88, sh, 16, false, true, rgba(SOOT));
        g.part(Matte).cylinder(ws, sr * 0.78 + 0.6, sr * 0.78 + 0.6, sh - 5.0, sh - 4.6, 16, rgba(DARK));
        for (k, col) in [[0.65, 0.1, 0.08], [0.85, 0.85, 0.82], [0.65, 0.1, 0.08]].iter().enumerate() {
            let y0 = sh * 0.72 + k as f32 * sh * 0.045;
            let rr = sr - (sr - sr * 0.78) * (y0 / (sh * 0.88)) + 0.04;
            g.part(Panel).cylinder(ws, rr, rr, y0, y0 + sh * 0.045, 16, rgba(*col));
        }
        for k in 0..3 {
            let a = k as f32 / 3.0 * std::f32::consts::TAU;
            for y in [sh - 1.0, sh * 0.55] {
                g.part(Glow).boxed(Vec3::new(ws.x + a.cos() * (sr * 0.8 + 0.3), y, ws.y + a.sin() * (sr * 0.8 + 0.3)), Vec3::splat(0.5), 0.0, rgba([2.5, 0.15, 0.1]));
            }
        }
    }
    // Trucks queueing at the tipping hall.
    if let Some(front) = tipping_front {
        for k in 0..[1, 2, 3][t] {
            let p = front.at(-front.len * 0.3 + k as f32 * 7.0, 7.0);
            if vehicle_fits(c, p, FRAC_PI_2, Vehicle::GarbageTruck) {
                vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, Vehicle::GarbageTruck, SANITATION_GREEN);
            }
        }
    }
    // Ash skips by the side; the fence.
    for k in 0..3 {
        let p = Vec2::new(lo.x + 3.0, lo.y + d * 0.25 + k as f32 * 3.4);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 {
            f.bx(g, Rust, p.x, 0.7, p.y, Vec3::new(2.6, 1.4, 3.0), mul(c.palette.rust, 0.8));
            f.bx(g, Dirt, p.x, 1.45, p.y, Vec3::new(2.3, 0.3, 2.7), [0.32, 0.31, 0.3]);
        }
    }
    site_fence(g, c, lo.y, None, 2.6, true);
    floodlight(g, c, Vec2::new(lo.x + 2.0, lo.y + d * 0.5), 9.0);
}

/// Bales of crushed cans, plastics, card and paper in a block of rows.
fn bales(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, at: Vec2, cols: usize, rows: usize) {
    let f = &c.frame;
    let tones = [[0.55, 0.56, 0.58], [0.15, 0.35, 0.7], [0.48, 0.36, 0.22], [0.75, 0.75, 0.7], [0.6, 0.2, 0.15], [0.2, 0.5, 0.25]];
    for r in 0..rows {
        let col = tones[rng.random_range(0..tones.len())];
        for k in 0..cols {
            let p = at + Vec2::new(k as f32 * 1.3, r as f32 * 1.3);
            if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.2 {
                continue;
            }
            for s in 0..rng.random_range(1..4) {
                f.bx_yaw(g, Tarp, p.x, 0.6 + s as f32 * 1.15, p.y, Vec3::new(1.2, 1.1, 1.2), mul(col, rng.random_range(0.75..1.1)), rng.random_range(-0.06..0.06));
            }
        }
    }
}

/// [r5-trash] A recycling plant: a long sorting shed with a green band and
/// a row of roof lights, its belts showing through the open side, belts up
/// to hoppers and a baler tower, a mountain of mixed recyclables at the
/// front where the trucks tip, blocks of bales in every colour, glass
/// bunkers in three, a front loader and skips.
pub fn recycling_plant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, Concrete, mul(pal.concrete(0), 0.62));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let hh = [6.5, 8.5, 10.5][t];
    let shed = fit_inside(c, &rect((lo.x + hi.x) * 0.5, hi.y - d * 0.27, w * 0.4, d * 0.21));
    let wall = [0.26, 0.29, 0.3];
    if shed.len() >= 3 && inradius(&shed) > 2.0 {
        extrude(g, f, WornMetal, &shed, 0.0, hh, wall);
        let (sl, sh) = bounds(&shed);
        // Sawtooth roof lights.
        let teeth = ((sh.x - sl.x) / 6.0).floor() as i32;
        for k in 0..teeth {
            let u = sl.x + 3.0 + k as f32 * 6.0;
            f.bx(g, Glass, u, hh + 0.8, (sl.y + sh.y) * 0.5, Vec3::new(0.15, 1.6, (sh.y - sl.y - 1.0).max(0.5)), pal.glass_dark);
            f.bx(g, Metal, u + 1.5, hh + 0.4, (sl.y + sh.y) * 0.5, Vec3::new(3.0, 0.8, (sh.y - sl.y - 1.0).max(0.5)), mul(wall, 0.8));
        }
        for face in c.faces(&shed) {
            if face.len < 4.0 {
                continue;
            }
            // A green band round the shed, ribs, grime.
            face.bx(g, f, Panel, 0.0, hh - 1.0, 0.1, Vec3::new(face.len, 1.2, 0.1), [0.12, 0.45, 0.22]);
            let ribs = (face.len / 1.4) as i32;
            for k in 0..ribs {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / ribs as f32;
                face.bx(g, f, Metal, tt, (hh - 1.6) * 0.5, 0.05, Vec3::new(0.08, hh - 1.6, 0.1), mul(wall, 0.85));
            }
            grime(g, c, rng, &face, 0.0, hh, 0.8);
        }
        let front = c.front_face(&shed);
        // The open side: belts lit inside, mixed waste riding on them.
        front.bx(g, f, Matte, 0.0, (hh - 1.8) * 0.5, 0.06, Vec3::new(front.len * 0.82, hh - 1.8, 0.08), DARK);
        for k in 0..[2, 3, 4][t] {
            let y = 1.2 + k as f32 * 1.4;
            front.bx(g, f, Matte, 0.0, y, 0.4, Vec3::new(front.len * 0.8, 0.12, 1.2), mul(pal.steel, 0.5));
            for j in 0..(front.len * 0.8) as i32 {
                if rng.random_bool(0.7) {
                    let tt = -front.len * 0.4 + j as f32 + rng.random_range(0.0..0.6);
                    front.bx(g, f, Matte, tt, y + 0.2, 0.4, Vec3::new(0.4, 0.25, 0.4), pick(rng, &BAG_COLORS));
                }
            }
            front.decal(g, f, Glow, 0.0, y + 0.6, 0.17, front.len * 0.7, 0.08, [1.6, 1.5, 1.2]);
        }
        front.decal(g, f, Glow, 0.0, hh - 1.0, 0.22, (front.len * 0.35).min(9.0), 0.5, [0.5, 2.0, 0.8]);
        // Belts up to the hopper and a baler tower over the shed.
        let tower = fit_inside(c, &rect(sh.x - 3.5, (sl.y + sh.y) * 0.5, 2.5, 2.5));
        if tower.len() >= 3 {
            extrude(g, f, WornPanel, &tower, 0.0, hh + 7.0, mul(pal.concrete(0), 0.8));
            g.part(Glow).boxed(f.at(sh.x - 3.5, hh + 7.3, (sl.y + sh.y) * 0.5), Vec3::new(1.0, 0.3, 1.0), 0.0, rgba([0.4, 2.0, 0.6]));
        }
        for (k, u) in [sl.x + 4.0, (sl.x + sh.x) * 0.5].iter().enumerate() {
            let base = Vec2::new(*u, lo.y + d * 0.3 + k as f32 * 3.0);
            let hop = Vec2::new(*u, sl.y + 2.0);
            if c.inside_local(base) {
                g.part(Metal).beam(f.at2(base, 0.6), f.at2(hop, hh + 2.5), 1.0, rgba(mul(pal.steel, 0.7)));
                f.bx(g, Metal, hop.x, hh + 3.0, hop.y, Vec3::new(3.0, 2.2, 3.0), mul(pal.steel, 0.6));
            }
        }
    }
    // The tipping heap at the front: mixed recyclables piled high.
    let heap_c = Vec2::new(lo.x + w * 0.3, lo.y + d * 0.22);
    let heap = blob(c, rng, heap_c, Vec2::new((w * 0.12).clamp(3.0, 9.0), (d * 0.08).clamp(2.5, 6.0)), 12, 0.3);
    let peak = shrink(&heap, heap_c, 0.35, Vec2::ZERO);
    if poly_inside(c, &heap) {
        loft(g, f, Tarp, &heap, &peak, 0.0, [2.5, 3.5, 4.5][t], [0.3, 0.32, 0.36]);
        junk(g, c, rng, &peak, [2.5, 3.5, 4.5][t], 40 + 30 * t);
        junk(g, c, rng, &heap, 0.4, 60 + 40 * t);
    }
    // Bale blocks along the yard.
    bales(g, c, rng, Vec2::new(lo.x + w * 0.52, lo.y + 3.5), (w * 0.18 / 1.3) as usize, [2, 3, 4][t]);
    bales(g, c, rng, Vec2::new(lo.x + 3.0, lo.y + d * 0.4), 5, [3, 4, 5][t]);
    // Glass bunkers: green, brown, clear.
    for (k, col) in [[0.1, 0.4, 0.15], [0.35, 0.2, 0.08], [0.7, 0.75, 0.75]].iter().enumerate() {
        let p = Vec2::new(hi.x - 4.0, lo.y + 4.0 + k as f32 * 4.2);
        if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 2.2 {
            continue;
        }
        f.bx(g, Concrete, p.x + 1.8, 1.0, p.y, Vec3::new(0.4, 2.0, 3.8), mul(pal.concrete(0), 0.7));
        let mound = rect(p.x, p.y, 1.3, 1.6);
        loft(g, f, Glass, &mound, &shrink(&mound, p, 0.4, Vec2::ZERO), 0.0, 1.4, *col);
    }
    // A front loader at the heap, a truck tipping.
    let fl = heap_c + Vec2::new(w * 0.14, 1.0);
    if vehicle_fits(c, fl, 0.5, Vehicle::Truck) {
        f.bx_yaw(g, Metal, fl.x, 1.1, fl.y, Vec3::new(2.0, 1.6, 3.6), HAZARD, 0.5);
        f.bx_yaw(g, Glass, fl.x, 2.2, fl.y, Vec3::new(1.4, 0.9, 1.4), pal.glass_dark, 0.5);
        f.bx_yaw(g, Metal, fl.x - 0.5f32.sin() * 2.3, 0.5, fl.y - 0.5f32.cos() * 2.3, Vec3::new(2.4, 0.9, 0.6), mul(pal.steel, 0.5), 0.5);
    }
    let tr = heap_c + Vec2::new(-w * 0.12, 2.0);
    if vehicle_fits(c, tr, FRAC_PI_2, Vehicle::GarbageTruck) {
        vehicle(g, f, pal, rng, tr, 0.0, FRAC_PI_2, Vehicle::GarbageTruck, SANITATION_GREEN);
    }
    site_fence(g, c, lo.y, None, 2.4, false);
    floodlight(g, c, Vec2::new(lo.x + 2.0, lo.y + d * 0.5), 8.0);
}

/// [r5-trash] A transfer station: a tall tipping shed over the compactor
/// pit (its hopper and ram housing on the side), sealed waste containers in
/// green stacked in rows under a gantry, a rail siding with box wagons
/// along the back, trailers waiting at the loading bay.
pub fn transfer_station(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.15));
    let (lo, hi) = bounds(&c.poly);
    let (w, d) = (hi.x - lo.x, hi.y - lo.y);
    let hh = [9.0, 12.0, 15.0][t];
    let shed = fit_inside(c, &rect(lo.x + w * 0.27, lo.y + d * 0.52, w * 0.22, d * 0.25));
    let wall = [0.2, 0.3, 0.26];
    if shed.len() >= 3 && inradius(&shed) > 2.0 {
        extrude(g, f, WornMetal, &shed, 0.0, hh, wall);
        parapet(g, c, &shed, hh, 0.5, 0.2, Metal, mul(wall, 0.8));
        for face in c.faces(&shed) {
            if face.len < 4.0 {
                continue;
            }
            let ribs = (face.len / 1.3) as i32;
            for k in 0..ribs {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / ribs as f32;
                face.bx(g, f, Metal, tt, hh * 0.5, 0.05, Vec3::new(0.08, hh * 0.98, 0.1), mul(wall, 0.8));
            }
            grime(g, c, rng, &face, 0.0, hh, 1.0);
        }
        let front = c.front_face(&shed);
        front.bx(g, f, Matte, 0.0, 3.4, 0.06, Vec3::new((front.len - 2.0).clamp(1.0, 12.0), 6.8, 0.08), DARK);
        front.bx(g, f, Matte, -(front.len - 2.0).clamp(1.0, 12.0) * 0.5 - 0.3, 1.2, 0.15, Vec3::new(0.35, 2.4, 0.35), HAZARD);
        front.bx(g, f, Panel, 0.0, hh - 0.8, 0.12, Vec3::new(front.len * 0.85, 0.9, 0.1), SANITATION_GREEN);
        front.decal(g, f, Glow, 0.0, hh - 0.8, 0.2, (front.len * 0.4).min(9.0), 0.45, mul([0.4, 2.0, 0.6], 0.8));
        roof_gear(g, c, rng, &shed, hh, 0.2);
        // The compactor's ram housing on the side, a box at its mouth.
        let (sl, sh) = bounds(&shed);
        let ram = Vec2::new(sh.x + 3.0, (sl.y + sh.y) * 0.5);
        if c.all_inside(&[Vec2::new(-2.6, -2.1), Vec2::new(2.6, -2.1), Vec2::new(-2.6, 2.1), Vec2::new(2.6, 2.1)].map(|o| ram + o)) {
            f.bx(g, Metal, ram.x, 1.6, ram.y, Vec3::new(5.0, 3.2, 4.0), HAZARD);
            f.bx(g, Metal, ram.x - 1.0, 4.0, ram.y, Vec3::new(3.0, 1.6, 3.0), mul(pal.steel, 0.6));
        }
    }
    // Sealed waste boxes in rows, two or three high, under a gantry.
    let u0 = lo.x + w * 0.66;
    let rows = [2, 3, 4][t];
    let (mut v_lo, mut v_hi) = (f32::MAX, f32::MIN);
    for r in 0..rows {
        let v = lo.y + d * 0.3 + r as f32 * 3.0;
        let p = Vec2::new(u0, v);
        if !(c.inside_local(p + Vec2::new(6.5, 0.0)) && c.inside_local(p - Vec2::new(6.5, 0.0)) && distance_to_boundary(p, &c.lot_poly) > 2.0) {
            continue;
        }
        v_lo = v_lo.min(v);
        v_hi = v_hi.max(v);
        for s in 0..(1 + (r + t) % 3) {
            let col = mul(SANITATION_GREEN, rng.random_range(0.65..1.0));
            container_unit(g, c, rng, p.x, p.y, 0.0, 12.0, 2.5, 2.6, s as f32 * 2.6, col);
        }
    }
    if v_lo <= v_hi {
        for u in [u0 - 7.5, u0 + 7.5] {
            let corners = [Vec2::new(u - 2.5, v_lo - 3.8), Vec2::new(u + 2.5, v_lo - 3.8), Vec2::new(u + 2.5, v_hi + 3.8), Vec2::new(u - 2.5, v_hi + 3.8)];
            if poly_inside(c, &corners) {
                gantry_crane(g, c, u, v_lo - 2.5, v_hi + 2.5, 10.0);
            }
        }
    }
    // The siding along the back: sleepers, rails, box wagons.
    let rv = hi.y - 3.0;
    let (u_a, u_b) = (lo.x + 3.0, hi.x - 3.0);
    if u_b - u_a > 20.0 && poly_inside(c, &rect((u_a + u_b) * 0.5, rv, (u_b - u_a) * 0.5, 1.7)) {
        let len = u_b - u_a;
        f.bx(g, Dirt, (u_a + u_b) * 0.5, 0.1, rv, Vec3::new(len, 0.2, 3.2), [0.3, 0.28, 0.26]);
        for s in [-0.72f32, 0.72] {
            f.bx(g, Metal, (u_a + u_b) * 0.5, 0.3, rv + s, Vec3::new(len, 0.15, 0.1), mul(pal.steel, 0.7));
        }
        let wagons = ((len - 4.0) / 14.0).floor() as i32;
        for k in 0..wagons.min([2, 3, 5][t]) {
            let u = u_a + 8.0 + k as f32 * 14.0;
            f.bx(g, Matte, u, 0.9, rv, Vec3::new(13.0, 0.5, 2.6), DARK);
            let col = mul(SANITATION_GREEN, rng.random_range(0.7..1.0));
            container_unit(g, c, rng, u, rv, 0.0, 12.0, 2.5, 2.6, 1.15, col);
        }
    }
    // Trailers waiting at the bay.
    for k in 0..[1, 2, 2][t] {
        let tr = Vec2::new(lo.x + w * 0.2 + k as f32 * 5.0, lo.y + d * 0.18);
        if vehicle_fits(c, tr, -FRAC_PI_2, Vehicle::Truck) {
            vehicle(g, f, pal, rng, tr, 0.0, -FRAC_PI_2, Vehicle::Truck, SANITATION_GREEN);
        }
    }
    site_fence(g, c, lo.y, None, 2.4, false);
    floodlight(g, c, Vec2::new(hi.x - 2.0, lo.y + d * 0.5), 9.0);
    floodlight(g, c, Vec2::new(lo.x + 2.0, lo.y + d * 0.5), 9.0);
}
