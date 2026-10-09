//! [dirty-industry] The dirty trades look it (`nd_sim::industry::IndustryKind::is_dirty`):
//! a smelter with its blast furnace, stacks, ore and slag heaps; a factory
//! farm of packed pens, battery sheds and a manure lagoon; a chemical works
//! of cell houses, tank rows and a poisoned settling pond. Stained ground
//! ([`soot_stains`]) goes under these and the older dirty styles (strip
//! mines, oil fields, refineries). Soot and rust, with the neon kept to
//! hot accents: glowing slag, furnace mouths, a toxic green vent.
//!
//! The renderer puts smoke over them from [`dirty_vents`], which knows
//! where each style's stacks stand without drawing it.

use super::parts::*;
use super::kit::{block, cow, fit, haul_truck, heap, rail_fence, R};
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

/// Furnace glow: hot orange, over-bright so it blooms.
const MOLTEN: [f32; 3] = [3.2, 1.2, 0.25];
/// A chemical works' sickly vent light.
const TOXIC: [f32; 3] = [0.9, 2.2, 0.35];
/// Soot.
const SOOT: [f32; 3] = [0.07, 0.065, 0.06];

/// What comes out of a vent, for the renderer's smoke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VentKind {
    /// Thick dark soot from a furnace or a cracker.
    Soot,
    /// A thin yellow-green chemical plume.
    Fumes,
    /// Low tan dust off a pit or a heap.
    Dust,
    /// A low brown haze off pens and a lagoon.
    Stench,
}

/// A vent's top in world coordinates (before the lot's pad) and what it
/// gives off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vent {
    pub pos: Vec3,
    pub kind: VentKind,
}

fn ok_site(p: Vec2, poly: &[Vec2], margin: f32) -> bool {
    nd_sim::zoning::point_in_polygon(p, poly) && distance_to_boundary(p, poly) >= margin
}

/// A smelter's stacks: two or three along the back, pulled in until they
/// stand clear of the lot edge. Lot-local (u, v) and height.
fn smelter_stacks(poly: &[Vec2]) -> Vec<(Vec2, f32)> {
    let (lo, hi) = bounds(poly);
    let c = Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
    let n = if hi.x - lo.x > 50.0 { 3 } else { 2 };
    let mut out = Vec::new();
    for k in 0..n {
        let mut p = Vec2::new(lo.x + (hi.x - lo.x) * (0.25 + 0.5 * k as f32 / (n - 1).max(1) as f32), hi.y - 5.0);
        for _ in 0..6 {
            if ok_site(p, poly, 3.0) {
                break;
            }
            p = p.lerp(c, 0.3);
        }
        if ok_site(p, poly, 3.0) && out.iter().all(|(q, _): &(Vec2, f32)| q.distance(p) > 4.0) {
            out.push((p, 30.0 + 6.0 * k as f32));
        }
    }
    out
}

/// A chemical works' vents: one stack and the vent over the cell house.
fn chemical_vents(poly: &[Vec2]) -> Vec<(Vec2, f32, VentKind)> {
    let (lo, hi) = bounds(poly);
    let c = Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
    let mut out = Vec::new();
    let mut p = Vec2::new(hi.x - 5.0, hi.y - 5.0);
    for _ in 0..6 {
        if ok_site(p, poly, 3.0) {
            break;
        }
        p = p.lerp(c, 0.3);
    }
    if ok_site(p, poly, 3.0) {
        out.push((p, 26.0, VentKind::Fumes));
    }
    out
}

/// Where a dirty business's smoke, fumes and dust come from on `lot`, in
/// world coordinates (add the lot's pad height). Empty for clean kinds.
pub fn dirty_vents(lot: &nd_sim::Lot, kind: nd_sim::BusinessKind) -> Vec<Vent> {
    use nd_sim::BusinessKind as B;
    let (frame, poly) = crate::buildings::Frame::lot_local(lot);
    let at = |p: Vec2, h: f32, kind: VentKind| {
        let w = frame.to_world(p.x, p.y);
        Vent { pos: Vec3::new(w.x, h, w.y), kind }
    };
    let (lo, hi) = bounds(&poly);
    let mid = Vec2::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
    match kind {
        B::Smelter => {
            let mut v: Vec<Vent> = smelter_stacks(&poly).into_iter().map(|(p, h)| at(p, h + 1.0, VentKind::Soot)).collect();
            v.push(at(Vec2::new(mid.x, lo.y + (hi.y - lo.y) * 0.2), 3.0, VentKind::Dust));
            v
        }
        B::ChemicalWorks => chemical_vents(&poly).into_iter().map(|(p, h, k)| at(p, h + 1.0, k)).collect(),
        // The furnace stack of the cracking unit (`industrial::refinery`).
        B::Refinery => {
            let p = Vec2::new(lo.x + 7.5, lo.y + 7.5);
            if ok_site(p, &poly, 2.0) {
                vec![at(p, 22.0, VentKind::Soot)]
            } else {
                vec![at(mid, 22.0, VentKind::Soot)]
            }
        }
        B::StripMine => vec![at(mid, 4.0, VentKind::Dust), at(Vec2::new(mid.x, lo.y + 6.0), 3.0, VentKind::Dust)],
        B::Feedlot => vec![at(Vec2::new(mid.x, hi.y - (hi.y - lo.y) * 0.2), 2.0, VentKind::Stench)],
        _ => Vec::new(),
    }
}

// ── shared ───────────────────────────────────────────────────────────────────

/// Stained ground over the yard: irregular blotches of soot, rust run-off
/// and oily or sickly puddles, each a hair above the last so none share a
/// plane. `amount` scales how many.
pub(super) fn soot_stains(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, yard: &[Vec2], amount: f32, sickly: bool) {
    let f = &c.frame;
    let pal = c.palette;
    // At most eleven, so the layers stay in 0.045..0.115 (each its own
    // height), under the feedlot's pens at 0.125.
    let n = (((4.0 + 6.0 * amount) * (0.8 + 0.4 * c.wear)) as i32).min(11);
    for k in 0..n {
        let Some(p) = random_in_poly(rng, yard, 2.0) else { continue };
        let r = rng.random_range(2.0..6.5) * (0.6 + 0.6 * amount);
        let blob: Vec<Vec2> = ngon(p.x, p.y, r, 9, rng.random_range(0.0..1.0)).into_iter().enumerate().map(|(i, q)| p + (q - p) * (0.7 + 0.3 * ((i * 7 + k as usize) % 5) as f32 / 4.0)).map(|q| clamp_into(yard, q)).collect();
        let col = match k % 4 {
            0 => SOOT,
            1 => mix(pal.rust_stain, SOOT, 0.5),
            2 if sickly => [0.32, 0.36, 0.08],
            _ => mul(pal.dirt, 0.45),
        };
        ground(g, f, Dirt, &blob, 0.045 + 0.007 * k as f32, col);
    }
}

/// A slag heap: a black cone with a crust, and a glowing run of fresh slag
/// down one side.
fn slag_heap(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < r * 0.8 {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Dirt).cylinder(w, r, r * 0.2, 0.0, h, 10, rgba([0.09, 0.085, 0.08]));
    g.part(Stone).cylinder(w, r * 0.45, r * 0.12, h * 0.6, h * 1.02, 8, rgba([0.16, 0.14, 0.13]));
    // The fresh pour: a hot streak down the flank.
    let a = rng.random_range(0.0..TAU);
    let d = Vec2::new(a.cos(), a.sin());
    let top = f.at(p.x + d.x * r * 0.2, h * 0.92, p.y + d.y * r * 0.2);
    let foot = f.at(p.x + d.x * r * 0.9, 0.25, p.y + d.y * r * 0.9);
    g.part(Glow).beam(top, foot, 0.35, rgba(MOLTEN));
}

/// A rust-streaked hall: block, roof monitor, grime and soot runs down
/// every face.
fn sooty_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, h: f32, wall: [f32; 3]) {
    let f = &c.frame;
    block(g, c, Rust, r, 0.0, h, wall);
    // A roof monitor along the ridge, vented.
    let mon = R::new(r.u0 + r.w() * 0.15, r.u1 - r.w() * 0.15, r.c().y - 1.5, r.c().y + 1.5);
    block(g, c, Metal, &mon, h, h + 2.2, mul(wall, 0.7));
    for face in r.faces() {
        if face.len < 3.0 {
            continue;
        }
        grime(g, c, rng, &face, 0.0, h, 4.0);
        // Soot fanning up from the vents under the eaves.
        let runs = (face.len / 6.0) as i32;
        for k in 0..runs {
            let t = -face.len * 0.5 + (k as f32 + 0.5) * face.len / runs.max(1) as f32;
            face.decal(g, f, Matte, t, h - 1.6, 0.03 + 0.004 * k as f32, 2.4, 3.0, SOOT);
        }
    }
}

// ── smelter ──────────────────────────────────────────────────────────────────

/// A smelter and foundry: a soot-black yard with an ore heap by the gate,
/// a long rusted casting hall with glowing doors, a blast furnace tower
/// with its skip hoist, two or three tall stacks, slag heaps out back and a
/// haul truck bringing ore.
pub fn smelter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let lot = yard(g, c, 0.4, Dirt, mix(pal.dirt, SOOT, 0.55));
    soot_stains(g, c, rng, &lot, 1.0, false);
    fence_lot(g, c, &lot, 2.6, true);
    let area = R::of(&c.poly);
    let front = c.front_face(&lot);
    // The casting hall across the middle.
    let hall_d = (area.d() * 0.32).clamp(8.0, 22.0);
    let want = R::new(area.u0 + 2.0, area.u0 + area.w() * 0.62, area.v0 + area.d() * 0.3, area.v0 + area.d() * 0.3 + hall_d);
    let h = [9.0, 11.0, 14.0][t];
    let rust = mix(pal.rust, SOOT, 0.35);
    if let Some(r) = fit(c, want, 6.0) {
        sooty_hall(g, c, rng, &r, h, rust);
        // Doors glowing with the pour inside.
        let fr = r.front();
        let doors = ((fr.len - 2.0) / 7.0).floor().clamp(1.0, 3.0) as i32;
        for k in 0..doors {
            let tt = -fr.len * 0.5 + (k as f32 + 0.5) * fr.len / doors as f32;
            fr.decal(g, f, Matte, tt, 2.6, 0.05, 4.4, 5.2, DARK);
            fr.decal(g, f, Glow, tt, 1.6, 0.08, 3.6, 3.0, mul(MOLTEN, 0.7));
        }
        sign_board(g, c, rng, &fr, fr.len * 0.3, h - 1.5, (fr.len * 0.3).min(7.0), 0.7);
    }
    // The blast furnace: a fat tower with ring platforms and a glowing tap
    // hole, and the inclined skip hoist that feeds it.
    let bf = Vec2::new(area.u0 + area.w() * 0.78, area.v0 + area.d() * 0.48);
    let bf_r = [2.6, 3.2, 4.0][t];
    if c.inside_local(bf) && distance_to_boundary(bf, &c.lot_poly) > bf_r + 2.0 {
        let w = f.to_world(bf.x, bf.y);
        let bh = [18.0, 24.0, 30.0][t];
        g.part(Rust).cylinder(w, bf_r * 1.15, bf_r, 0.0, bh * 0.35, 14, rgba(mul(rust, 0.9)));
        g.part(Metal).cylinder(w, bf_r, bf_r * 0.7, bh * 0.35, bh, 14, rgba(mul(pal.steel, 0.55)));
        for k in 1..(bh / 6.0) as i32 {
            let y = k as f32 * 6.0;
            g.part(Matte).cylinder(w, bf_r + 0.8, bf_r + 0.8, y, y + 0.18, 14, rgba(mul(pal.warning_light, 0.3)));
        }
        // Downcomer pipes off the top, where there is room beside it.
        let down = Vec2::new(bf.x - bf_r - 3.0, bf.y);
        if c.inside_local(down) && distance_to_boundary(down, &c.lot_poly) > 2.0 {
            pipe_run(g, f, &[Vec3::new(bf.x, bh, bf.y), Vec3::new(down.x, bh + 2.0, down.y), Vec3::new(down.x, 4.0, down.y)], 0.7, mul(pal.steel, 0.5));
        }
        // The tap hole, and a ladle of molten iron under it.
        f.bx(g, Glow, bf.x, 1.2, bf.y - bf_r - 0.2, Vec3::new(1.2, 1.0, 0.3), MOLTEN);
        let ladle = Vec2::new(bf.x, bf.y - bf_r - 2.6);
        if c.inside_local(ladle) && distance_to_boundary(ladle, &c.lot_poly) > 1.6 {
            let lw = f.to_world(ladle.x, ladle.y);
            g.part(Metal).cylinder(lw, 1.3, 1.0, 0.0, 2.0, 10, rgba(CHARCOAL));
            g.part(Glow).cylinder(lw, 1.15, 1.15, 2.0, 2.08, 10, rgba(MOLTEN));
        }
        // Skip hoist down to the stock house.
        let foot = Vec2::new(bf.x + bf_r + 9.0, bf.y + 2.0);
        if c.inside_local(foot) {
            let a = f.at(foot.x, 1.0, foot.y);
            let b = f.at(bf.x + bf_r * 0.6, bh * 0.92, bf.y);
            g.part(Metal).slab(a, b, 1.6, 0.6, rgba(mul(pal.steel, 0.6)));
        }
        aviation_light_local(g, f, pal, bf.x, bh + 0.6, bf.y, 0.35);
    }
    // Stacks along the back: tall, soot-streaked, red lights at the top.
    for (p, sh) in smelter_stacks(&c.lot_poly) {
        let w = f.to_world(p.x, p.y);
        g.part(Concrete).cylinder(w, 1.6, 1.1, 0.0, sh, 12, rgba(mix(pal.concrete(0), SOOT, 0.35)));
        g.part(Matte).cylinder(w, 1.12, 1.05, sh * 0.82, sh, 12, rgba(SOOT));
        for y in [sh * 0.45, sh * 0.7] {
            g.part(Matte).cylinder(w, 1.4, 1.35, y, y + 0.5, 12, rgba([0.75, 0.12, 0.08]));
        }
        aviation_light(g, pal, Vec3::new(w.x, sh + 0.4, w.y), 0.4);
    }
    // Ore heap by the gate, slag heaps at the back.
    let ore = front.at(front.len * 0.28, -(area.d() * 0.14).clamp(5.0, 9.0));
    heap(g, c, ore, (area.d() * 0.12).clamp(3.0, 6.0), 4.5, [0.42, 0.22, 0.14]);
    for k in 0..[1, 2, 3][t] {
        let p = Vec2::new(area.u0 + 5.0 + k as f32 * 9.0, area.v1 - 6.0);
        let (r, h) = (rng.random_range(3.0..5.0), rng.random_range(3.0..6.0));
        slag_heap(g, c, rng, p, r, h);
    }
    // A conveyor from the ore heap to the furnace's stock house.
    let stock = Vec2::new(bf.x + bf_r + 9.0, bf.y + 2.0);
    if c.inside_local(stock) && c.inside_local(ore) {
        let a = f.at(ore.x, 4.0, ore.y);
        let b = f.at(stock.x, 6.0, stock.y);
        g.part(Metal).slab(a, b, 1.0, 0.4, rgba(mul(pal.steel, 0.7)));
    }
    // A haul truck of ore at the gate.
    let tr = front.at(-front.len * 0.2, -4.0);
    if c.inside_local(tr) && distance_to_boundary(tr, &c.lot_poly) > 4.0 {
        haul_truck(g, f, tr, 0.0, 0.0, [0.72, 0.5, 0.1]);
    }
    for p in [front.at(-front.len * 0.45, -1.5), front.at(front.len * 0.45, -1.5)] {
        floodlight(g, c, p, 12.0);
    }
}

// ── factory farm ─────────────────────────────────────────────────────────────

/// A factory farm: long battery sheds with fans and feed silos, a feedlot
/// of bare mud packed with cattle, a brown manure lagoon at the back and a
/// feed mill tower; the whole yard muddy and stained.
pub fn feedlot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let mud = mix(pal.dirt, [0.2, 0.14, 0.08], 0.55);
    let lot = yard(g, c, 0.4, Dirt, mud);
    soot_stains(g, c, rng, &lot, 0.6, true);
    let area = R::of(&c.poly);
    let rail = mul(pal.steel, 0.7);
    // Battery sheds along the left, long and low.
    let sheds_w = area.w() * 0.42;
    let n = (((sheds_w + 3.0) / 10.0).floor() as i32).clamp(1, 3);
    let wall = mix([0.82, 0.8, 0.74], SOOT, 0.25);
    for k in 0..n {
        let u0 = area.u0 + 1.5 + k as f32 * (sheds_w / n as f32);
        let want = R::new(u0, u0 + sheds_w / n as f32 - 2.5, area.v0 + 6.0, area.v1 - 2.0);
        let Some(r) = fit(c, want, 4.0) else { continue };
        let h = 3.0;
        block(g, c, WornPanel, &r, 0.0, h, wall);
        let rise = (r.w() * 0.12).clamp(0.6, 1.4);
        let bottom = rect(r.c().x, r.c().y, r.w() * 0.5 + 0.2, r.d() * 0.5 + 0.3);
        let top = rect(r.c().x, r.c().y, 0.08, r.d() * 0.5 + 0.3);
        loft(g, f, Metal, &bottom, &top, h, h + rise, mul(pal.steel, 0.5));
        // Extractor fans in the back end, stained grilles down the sides.
        let back = r.back();
        for i in 0..(r.w() / 2.2).floor().max(1.0) as i32 {
            back.decal(g, f, Matte, -r.w() * 0.5 + 1.1 + i as f32 * 2.2, 1.4, 0.05, 1.5, 1.5, DARK);
        }
        for face in [r.left(), r.right()] {
            face.decal(g, f, Matte, 0.0, 1.6, 0.04, face.len * 0.9, 0.6, mul(wall, 0.55));
            face.decal(g, f, Matte, 0.0, 0.5, 0.05, face.len * 0.9, 1.0, mix(mud, SOOT, 0.3));
        }
        // A feed silo at its head.
        let p = Vec2::new(r.c().x, r.v0 - 2.0);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.8 {
            let w = f.to_world(p.x, p.y);
            g.part(Metal).cylinder(w, 0.2, 1.3, 0.8, 2.4, 10, rgba(mul(pal.steel, 1.1)));
            g.part(Metal).cylinder(w, 1.3, 1.3, 2.4, 8.0, 10, rgba(mul(pal.steel, 1.1)));
            g.part(Metal).cylinder(w, 1.3, 0.2, 8.0, 8.8, 10, rgba(pal.steel));
        }
    }
    // The feedlot: bare mud in pens, packed.
    let pens_want = R::new(area.u0 + sheds_w + 2.0, area.u1 - 1.5, area.v0 + 5.0, area.v0 + area.d() * 0.62);
    if let Some(p) = fit(c, pens_want, 5.0) {
        // Above every soot stain layer (0.045..0.115), not in one of them.
        ground(g, f, Dirt, &p.poly(), 0.125, mix(mud, SOOT, 0.25));
        for face in p.faces() {
            rail_fence(g, c, face.a, face.b, 1.4, rail);
        }
        let cols = ((p.w() / 8.0) as i32).max(1);
        for k in 1..cols {
            let u = p.u0 + k as f32 * p.w() / cols as f32;
            rail_fence(g, c, Vec2::new(u, p.v0), Vec2::new(u, p.v1), 1.4, rail);
        }
        // A feed trough along the front of the pens.
        f.bx(g, Concrete, p.c().x, 0.4, p.v0 + 0.8, Vec3::new(p.w() - 1.0, 0.8, 0.8), mul(pal.concrete(0), 0.7));
        let herd = [10, 16, 24][t];
        let (lo, hi) = (Vec2::new(p.u0 + 1.2, p.v0 + 2.0), Vec2::new(p.u1 - 1.2, p.v1 - 1.2));
        if hi.x > lo.x && hi.y > lo.y {
            for _ in 0..herd {
                let q = Vec2::new(rng.random_range(lo.x..hi.x), rng.random_range(lo.y..hi.y));
                let yaw = if coin(rng, 0.7) { -FRAC_PI_2 + rng.random_range(-0.4..0.4) } else { rng.random_range(0.0..TAU) };
                let coat = if coin(rng, 0.5) { ([0.32, 0.2, 0.12], None) } else { ([0.12, 0.1, 0.09], None) };
                cow(g, f, q, yaw, coat.0, coat.1);
            }
        }
    }
    // The manure lagoon: a brown-green pond with a berm.
    let lagoon_want = R::new(area.u0 + sheds_w + 2.0, area.u1 - 2.0, area.v0 + area.d() * 0.68, area.v1 - 2.0);
    if let Some(l) = fit(c, lagoon_want, 4.0) {
        let pond = ngon(l.c().x, l.c().y, 1.0, 10, 0.3).into_iter().map(|q| l.c() + (q - l.c()) * Vec2::new(l.w() * 0.46, l.d() * 0.42)).collect::<Vec<_>>();
        let berm = scale_poly(&pond, 1.12);
        extrude(g, f, Dirt, &berm, 0.0, 0.7, mix(mud, SOOT, 0.2));
        ground(g, f, Glass, &pond, 0.74, [0.2, 0.17, 0.06]);
        ground(g, f, Matte, &scale_poly(&pond, 0.55), 0.76, [0.26, 0.28, 0.07]);
    }
    // The feed mill tower by the gate.
    let front = c.front_face(&lot);
    let mill = front.at(-front.len * 0.32, -4.5);
    if poly_inside(c, &rect(mill.x, mill.y, 2.2, 2.2)) {
        let r = R::new(mill.x - 2.0, mill.x + 2.0, mill.y - 2.0, mill.y + 2.0);
        block(g, c, WornPanel, &r, 0.0, [12.0, 15.0, 18.0][t], mix([0.7, 0.68, 0.62], SOOT, 0.2));
        r.front().decal(g, f, Glow, 0.0, 9.0, 0.05, 3.0, 0.5, [1.4, 0.9, 0.3]);
    }
    let lorry = front.at(front.len * 0.3, -3.0);
    if vehicle_fits(c, lorry, 0.0, Vehicle::Lorry) {
        vehicle(g, f, pal, rng, lorry, 0.0, 0.0, Vehicle::Lorry, [0.55, 0.42, 0.25]);
    }
    for p in [front.at(-front.len * 0.45, -1.2), front.at(front.len * 0.45, -1.2)] {
        floodlight(g, c, p, 8.0);
    }
}

// ── chemical works ───────────────────────────────────────────────────────────

/// A chemical works (chlor-alkali, mercury cells): long cell houses with a
/// green-lit vent, rows of tanks with hazard bands, pipe racks, a stack
/// with a sickly glow and a poisoned settling pond; the ground stained
/// yellow and black.
pub fn chemical_works(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let lot = yard(g, c, 0.4, Asphalt, mix(pal.asphalt, SOOT, 0.3));
    soot_stains(g, c, rng, &lot, 1.0, true);
    fence_lot(g, c, &lot, 2.8, true);
    let area = R::of(&c.poly);
    // Cell houses along the front half.
    let wall = mix(pick(rng, &pal.panel), [0.55, 0.58, 0.5], 0.4);
    let want = R::new(area.u0 + 2.0, area.u0 + area.w() * 0.55, area.v0 + 4.0, area.v0 + area.d() * 0.45);
    let h = [6.0, 7.5, 9.0][t];
    if let Some(r) = fit(c, want, 6.0) {
        block(g, c, WornPanel, &r, 0.0, h, wall);
        let mon = R::new(r.u0 + 1.0, r.u1 - 1.0, r.c().y - 1.0, r.c().y + 1.0);
        block(g, c, Metal, &mon, h, h + 1.4, mul(wall, 0.6));
        // The roof vent lit an unhealthy green.
        mon.front().decal(g, f, Glow, 0.0, h + 0.7, 0.04, mon.w() * 0.8, 0.35, TOXIC);
        for face in r.faces() {
            if face.len > 4.0 {
                grime(g, c, rng, &face, 0.0, h, 3.0);
            }
        }
        // Hazard diamonds by the doors.
        let fr = r.front();
        for k in 0..2 {
            let tt = (k as f32 - 0.5) * fr.len * 0.5;
            fr.decal(g, f, Matte, tt, 2.0, 0.05, 2.6, 3.2, DARK);
            fr.decal(g, f, Glow, tt + 1.9, 3.0, 0.06, 0.7, 0.7, [1.6, 1.2, 0.1]);
        }
        sign_board(g, c, rng, &fr, fr.len * 0.3, h - 1.0, (fr.len * 0.3).min(6.0), 0.6);
    }
    // Tanks in rows behind, banded yellow and black.
    let (lo, _) = bounds(&offset_poly(&lot, 3.0));
    let rows = [1, 2, 2][t];
    for row in 0..rows {
        let v = area.v0 + area.d() * (0.58 + 0.18 * row as f32);
        let mut u = lo.x + 3.0;
        let mut k = 0;
        while u < area.u0 + area.w() * 0.62 && k < 5 {
            let p = Vec2::new(u, v);
            let r = 2.0 + 0.4 * ((k + row) % 2) as f32;
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > r + 1.0 {
                let th = 5.0 + ((k * 3 + row) % 3) as f32;
                tank_vertical(g, f, p, r, 0.0, th, Metal, mix(pal.steel, [0.85, 0.85, 0.8], 0.4));
                let w = f.to_world(p.x, p.y);
                g.part(Matte).cylinder(w, r * 1.02, r * 1.02, th * 0.55, th * 0.55 + 0.5, 14, rgba([0.95, 0.75, 0.05]));
                // A rust bleed from the valve.
                g.part(Rust).cylinder(w, r * 1.015, r * 1.015, 0.0, 1.2, 14, rgba(pal.rust_stain));
            }
            u += 6.0;
            k += 1;
        }
    }
    // Pipe rack from the tanks to the cell houses.
    let rack_v = area.v0 + area.d() * 0.5;
    if let Some(rack) = fit(c, R::new(lo.x + 2.0, area.u0 + area.w() * 0.6, rack_v - 1.5, rack_v + 2.3), 3.0) {
        let (u0, u1) = (rack.u0 + 0.6, rack.u1 - 0.6);
        pipe_run(g, f, &[Vec3::new(u0, 5.0, rack_v), Vec3::new(u1, 5.0, rack_v)], 0.3, mul(pal.steel, 0.75));
        pipe_run(g, f, &[Vec3::new(u0, 4.0, rack_v + 0.8), Vec3::new(u1, 4.0, rack_v + 0.8)], 0.22, [0.7, 0.62, 0.15]);
    }
    // The stack, with a sickly light at the top.
    for (p, sh, _) in chemical_vents(&c.lot_poly) {
        chimney(g, f, p, 0.9, 0.0, sh, mix(pal.steel, SOOT, 0.4), TOXIC);
        let w = f.to_world(p.x, p.y);
        g.part(Matte).cylinder(w, 0.95, 0.9, sh * 0.85, sh * 0.85 + 0.6, 10, rgba([0.95, 0.75, 0.05]));
    }
    // The settling pond: a lurid green-yellow with a dark rim.
    let pond_want = R::new(area.u0 + area.w() * 0.66, area.u1 - 2.0, area.v0 + 4.0, area.v0 + area.d() * 0.55);
    if let Some(l) = fit(c, pond_want, 5.0) {
        let pond = ngon(l.c().x, l.c().y, 1.0, 9, 0.2).into_iter().map(|q| l.c() + (q - l.c()) * Vec2::new(l.w() * 0.45, l.d() * 0.42)).collect::<Vec<_>>();
        extrude(g, f, Concrete, &scale_poly(&pond, 1.1), 0.0, 0.6, mix(pal.concrete(0), SOOT, 0.4));
        ground(g, f, Glass, &pond, 0.64, [0.45, 0.6, 0.06]);
        ground(g, f, Glow, &scale_poly(&pond, 0.35), 0.66, [0.35, 0.8, 0.12]);
    }
    let front = c.front_face(&lot);
    for p in [front.at(-front.len * 0.45, -1.5), front.at(front.len * 0.45, -1.5)] {
        floodlight(g, c, p, 10.0);
    }
}
