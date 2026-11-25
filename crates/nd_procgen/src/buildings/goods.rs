//! [r17-goods] The new chains' looks (`nd_sim` round 17: drinks, medicine,
//! cement and concrete, timber, textiles): a brewery with its copper
//! kettles behind glass, a distillery's bonded warehouses and pagoda kiln,
//! a bottling plant, a drug maker's clean white plant, a cement works with
//! its preheater tower and long kiln, a concrete batching plant, a saw mill
//! with its log yard, a textile mill under a sawtooth roof, and the fashion
//! district's wholesalers with their racks and stalls out on the pavement.
//! Each is forced by its business (`super::industry_styles`).
//!
//! Layouts work in axis-aligned rectangles of the lot frame ([`R`]): `u`
//! along the street, `v` in from it, so the street side is `v0`.

use super::kit::{block, fit, heap, pane_row, R};
use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Copper, bright enough to read at dusk.
const COPPER: [f32; 3] = [0.72, 0.38, 0.18];
/// Pale limestone and cement dust.
const DUST: [f32; 3] = [0.74, 0.72, 0.66];
/// Bark and sawn wood.
const BARK: [f32; 3] = [0.3, 0.21, 0.13];
const SAWN: [f32; 3] = [0.76, 0.6, 0.4];

/// A plain works hall on `r`: walls, a shallow roof, roller doors on the
/// street face, grime, a lit sign board over the doors.
fn hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, h: f32, wall: [f32; 3], doors: bool) {
    let f = &c.frame;
    block(g, c, if c.tier.index() == 0 { WornPanel } else { Panel }, r, 0.0, h, wall);
    f.bx(g, Metal, r.c().x, h + 0.25, r.c().y, Vec3::new(r.w() + 0.3, 0.3, r.d() + 0.3), mul(c.palette.steel, 0.6));
    let front = r.front();
    if doors && r.w() > 6.0 {
        let n = ((r.w() / 7.0).floor() as i32).clamp(1, 3);
        let dw = r.w() / n as f32;
        for k in 0..n {
            let t = -r.w() * 0.5 + (k as f32 + 0.5) * dw;
            roller_door(g, c, &front, t, (dw - 1.5).min(4.5), (h - 1.0).min(4.5), mul(c.palette.steel, 0.7));
        }
    }
    for face in r.faces() {
        if face.len > 3.0 {
            grime(g, c, rng, &face, 0.0, h, 2.0 + 3.0 * c.wear);
        }
    }
    sign_board(g, c, rng, &front, 0.0, h - 0.6, (r.w() * 0.4).clamp(2.5, 9.0), 0.8);
}

/// A row of `n` vertical silos from `a` to `b` (lot-local), radius `rad`.
fn silos(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, n: i32, rad: f32, h: f32, color: [f32; 3]) {
    for k in 0..n {
        let p = a.lerp(b, if n > 1 { k as f32 / (n - 1) as f32 } else { 0.5 });
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > rad + 0.3 {
            tank_vertical(g, &c.frame, p, rad, 0.0, h, Metal, color);
        }
    }
}

/// Stacked pallets of crates or kegs at `p`.
fn pallets(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, n: i32, cols: &[[f32; 3]]) {
    let f = &c.frame;
    for k in 0..n {
        let q = p + Vec2::new((k % 3) as f32 * 1.4, (k / 3) as f32 * 1.4);
        if !c.inside_local(q) || distance_to_boundary(q, &c.lot_poly) < 1.0 {
            continue;
        }
        let tall = rng.random_range(1..=3) as f32;
        f.bx(g, Wood, q.x, 0.08, q.y, Vec3::new(1.2, 0.14, 1.2), c.palette.wood);
        f.bx(g, Matte, q.x, 0.15 + tall * 0.4, q.y, Vec3::new(1.1, tall * 0.8, 1.1), cols[k as usize % cols.len()]);
    }
}

// ── drinks ───────────────────────────────────────────────────────────────────

/// A brewery: a brewhouse with a tall glass front showing two copper
/// kettles (lit warm at night), a row of tall fermenting tanks beside it,
/// grain silos at the back, keg pallets and a drinks lorry bay.
pub fn brewery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.8));
    let area = R::of(&lot);
    let brick = pick(rng, &pal.brick);
    let h = 9.0 + 2.0 * c.tier.index() as f32;
    let want = R::new(area.u0 + 1.0, area.u0 + (area.w() * 0.5).max(10.0), area.v0 + 2.0, area.v0 + (area.d() * 0.55).max(10.0));
    if let Some(r) = fit(c, want, 7.0) {
        hall(g, c, rng, &r, h, brick, false);
        // The glass brewhouse front and its kettles behind.
        let front = r.front();
        let gw = (r.w() * 0.6).min(10.0);
        front.decal(g, f, Glass, 0.0, h * 0.45, 0.04, gw, h * 0.7, pal.glass_dark);
        front.decal(g, f, Matte, 0.0, h * 0.45, 0.02, gw + 0.4, h * 0.7 + 0.4, mul(brick, 0.5));
        for s in [-1.0f32, 1.0] {
            let p = front.at(s * gw * 0.22, -2.2);
            let w = f.to_world(p.x, p.y);
            g.part(Metal).cylinder_capped(w, 1.6, 1.6, 0.0, h * 0.5, 12, false, false, rgba(COPPER));
            g.part(Metal).cylinder_capped(w, 1.6, 0.4, h * 0.5, h * 0.72, 12, false, true, rgba(mul(COPPER, 0.9)));
            g.part(Glow).boxed(Vec3::new(w.x, h * 0.3, w.y), Vec3::new(0.4, 0.4, 0.4), 0.0, rgba([2.4, 1.3, 0.5]));
        }
        // Fermenting tanks beside it.
        let a = Vec2::new(r.u1 + 2.5, r.v0 + 2.5);
        let b = Vec2::new((r.u1 + 2.5 + 12.0).min(area.u1 - 2.0), r.v0 + 2.5);
        silos(g, c, a, b, 4, 1.4, h + 1.5, [0.82, 0.83, 0.84]);
    }
    // Grain silos at the back, kegs and crates in the yard.
    silos(g, c, Vec2::new(area.u0 + 3.0, area.v1 - 3.5), Vec2::new(area.u0 + 12.0, area.v1 - 3.5), 3, 2.0, 14.0, [0.7, 0.7, 0.68]);
    pallets(g, c, rng, Vec2::new(area.u1 - 9.0, area.v1 - 7.0), 6, &[[0.72, 0.74, 0.76], [0.55, 0.12, 0.08], [0.18, 0.35, 0.14]]);
}

/// A distillery: two long dark bonded warehouses, a still house with a
/// pagoda-roofed kiln and two copper pot stills under a lit window, casks.
pub fn distillery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Dirt, pal.gravel);
    let area = R::of(&lot);
    let stone = mix(pick(rng, &pal.plaster), [0.85, 0.84, 0.8], 0.4);
    // The still house at the front.
    let want = R::new(area.u0 + 1.5, area.u0 + (area.w() * 0.45).max(9.0), area.v0 + 2.0, area.v0 + 12.0);
    if let Some(r) = fit(c, want, 6.0) {
        hall(g, c, rng, &r, 8.0, stone, false);
        // The pagoda kiln: a stepped pyramid roof with a vent cap.
        let k = R::new(r.u0 + 0.5, r.u0 + 5.0, r.v0 + 0.5, r.v0 + 5.0);
        let kc = k.c();
        let w = f.to_world(kc.x, kc.y);
        g.part(Metal).cylinder(w, 3.2, 0.8, 8.5, 12.5, 4, rgba(mul(c.palette.steel, 0.4)));
        g.part(Matte).cylinder(w, 0.9, 0.9, 12.5, 13.6, 4, rgba(mul(c.palette.steel, 0.35)));
        let front = r.front();
        front.decal(g, f, Glass, r.w() * 0.15, 4.0, 0.04, (r.w() * 0.4).min(6.0), 4.5, pal.glass_dark);
        for s in [0.05f32, 0.3] {
            let p = front.at(r.w() * s, -2.0);
            let pw = f.to_world(p.x, p.y);
            g.part(Metal).cylinder_capped(pw, 1.3, 0.35, 0.0, 5.5, 10, false, true, rgba(COPPER));
            g.part(Glow).boxed(Vec3::new(pw.x, 1.5, pw.y), Vec3::new(0.35, 0.35, 0.35), 0.0, rgba([2.2, 1.0, 0.3]));
        }
    }
    // The bonded warehouses behind.
    let wd = (area.d() - 16.0).max(6.0) * 0.45;
    for k in 0..2 {
        let v0 = area.v0 + 14.0 + k as f32 * (wd + 2.0);
        if let Some(r) = fit(c, R::new(area.u0 + 1.0, area.u1 - 1.0, v0, v0 + wd), 5.0) {
            block(g, c, Stone, &r, 0.0, 6.5, mul(stone, 0.45));
            f.bx(g, Metal, r.c().x, 6.8, r.c().y, Vec3::new(r.w() + 0.2, 0.5, r.d() + 0.2), [0.12, 0.12, 0.13]);
        }
    }
    // Casks by the yard gate.
    for k in 0..6 {
        let p = Vec2::new(area.u1 - 3.0 - (k % 3) as f32 * 1.2, area.v0 + 3.0 + (k / 3) as f32 * 1.2);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.8 {
            let w = f.to_world(p.x, p.y);
            g.part(Wood).cylinder(w, 0.45, 0.45, 0.0, 1.0, 8, rgba([0.42, 0.26, 0.14]));
        }
    }
}

/// A bottling plant: a long clean shed with a glazed line window, syrup
/// tanks, pallets of crates and a lorry bay.
pub fn bottler(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.85));
    let area = R::of(&lot);
    let wall = mix(pick(rng, &pal.panel), [0.9, 0.9, 0.92], 0.5);
    let want = R::new(area.u0 + 1.0, area.u1 - 9.0, area.v0 + 3.0, area.v1 - 2.0);
    if let Some(r) = fit(c, want, 7.0) {
        hall(g, c, rng, &r, 8.0, wall, true);
        // The line window along the side: bottles going by under the lights.
        let side = r.left();
        side.decal(g, f, Glass, 0.0, 4.5, 0.04, side.len * 0.7, 1.6, pal.glass_dark);
        side.decal(g, f, Glow, 0.0, 4.5, 0.06, side.len * 0.66, 0.15, [0.3, 1.4, 2.2]);
    }
    silos(g, c, Vec2::new(area.u1 - 4.0, area.v0 + 4.0), Vec2::new(area.u1 - 4.0, area.v0 + 14.0), 3, 1.5, 7.0, [0.86, 0.86, 0.88]);
    pallets(g, c, rng, Vec2::new(area.u1 - 7.5, area.v1 - 8.0), 6, &[[0.85, 0.2, 0.15], [0.2, 0.45, 0.85], [0.95, 0.75, 0.15]]);
}

// ── medicine ─────────────────────────────────────────────────────────────────

/// A drug maker: a clean white plant of two blocks with ribbon windows, a
/// glazed link, roof plant and ducts, a green cross lit on the front,
/// a fenced tank compound.
pub fn drug_plant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.9));
    let area = R::of(&lot);
    let white = [0.88, 0.89, 0.9];
    let floors = (c.floors.clamp(2, 4)) as i32;
    let h = floors as f32 * 4.0;
    let want = R::new(area.u0 + 2.0, area.u1 - 2.0, area.v0 + 3.0, area.v0 + (area.d() * 0.55).max(10.0));
    if let Some(r) = fit(c, want, 8.0) {
        block(g, c, Panel, &r, 0.0, h, white);
        parapet(g, c, &r.poly(), h, 0.6, 0.25, Matte, mul(white, 0.85));
        for face in r.faces() {
            if face.len > 5.0 {
                pane_row(g, c, rng, &face, 1.4, floors, 4.0, 2.2, 1.8, 1.1, false);
            }
        }
        // Roof plant and ducts.
        let rc = r.c();
        f.bx(g, Metal, rc.x, h + 1.0, rc.y, Vec3::new((r.w() * 0.4).min(10.0), 2.0, (r.d() * 0.4).min(8.0)), [0.6, 0.62, 0.64]);
        chimney(g, f, Vec2::new(r.u1 - 2.0, r.v1 - 2.0), 0.5, h, 4.0, [0.75, 0.76, 0.78], pal.aviation_light);
        // The green cross on the front.
        let front = r.front();
        let y = h - 1.6;
        front.decal(g, f, Glow, -r.w() * 0.3, y, 0.08, 2.2, 0.7, [0.2, 2.6, 0.8]);
        front.decal(g, f, Glow, -r.w() * 0.3, y, 0.09, 0.7, 2.2, [0.2, 2.6, 0.8]);
    }
    // The tank compound at the back.
    let tc = Vec2::new(area.u1 - 6.0, area.v1 - 5.0);
    for k in 0..3 {
        let p = tc - Vec2::new(k as f32 * 3.2, 0.0);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 {
            tank_vertical(g, f, p, 1.2, 0.0, 4.5, Metal, [0.8, 0.82, 0.84]);
        }
    }
}

// ── cement and concrete ──────────────────────────────────────────────────────

/// A cement works: the preheater tower (a tall steel frame of cyclones),
/// the long rotary kiln on its piers sloping down from it, clinker and
/// cement silos, a stack, a limestone heap, everything grey with dust.
pub fn cement_works(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Dirt, mix(pal.dirt, DUST, 0.6));
    let area = R::of(&lot);
    let grey = mix(pal.concrete(0), DUST, 0.5);
    // The preheater tower at the back left.
    let tw = 9.0f32.min(area.w() * 0.3);
    let tower = R::new(area.u0 + 2.0, area.u0 + 2.0 + tw, area.v1 - 2.0 - tw, area.v1 - 2.0);
    let th = 32.0 + 6.0 * c.tier.index() as f32;
    if let Some(r) = fit(c, tower, 5.0) {
        // Four corner legs and floors, cyclones inside.
        for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u0, r.v1), (r.u1, r.v1)] {
            f.bx(g, Metal, u + if u == r.u0 { 0.3 } else { -0.3 }, th * 0.5, v + if v == r.v0 { 0.3 } else { -0.3 }, Vec3::new(0.5, th, 0.5), mul(pal.steel, 0.55));
        }
        for k in 1..6 {
            let y = th * k as f32 / 6.0;
            f.bx(g, Metal, r.c().x, y, r.c().y, Vec3::new(r.w(), 0.35, r.d()), mul(grey, 0.8));
        }
        let cw = f.to_world(r.c().x, r.c().y);
        for k in 0..3 {
            let y0 = 4.0 + k as f32 * th * 0.28;
            g.part(Metal).cylinder_capped(cw, r.w() * 0.28, r.w() * 0.12, y0 + 3.0, y0 + 7.0, 10, false, false, rgba(DUST));
            g.part(Metal).cylinder_capped(cw, r.w() * 0.28, r.w() * 0.28, y0 + 7.0, y0 + 9.0, 10, false, true, rgba(DUST));
        }
        block(g, c, Metal, &R::new(r.u0 + 0.6, r.u1 - 0.6, r.v0 + 0.6, r.v1 - 0.6), th, th + 3.0, grey);
        chimney(g, f, Vec2::new(r.u1 - 1.2, r.v0 + 1.2), 0.9, th + 3.0, 10.0, mul(grey, 0.7), pal.aviation_light);
        // The kiln: a long tube sloping down towards the street side.
        let p0 = Vec2::new(r.c().x + r.w() * 0.2, r.v0 - 0.5);
        let len = (area.d() * 0.55).clamp(10.0, 45.0);
        let p1 = Vec2::new(p0.x + len * 0.35, (p0.y - len).max(area.v0 + 4.0));
        let (a, b) = (f.at(p0.x, 7.0, p0.y), f.at(p1.x, 4.5, p1.y));
        if c.inside_local(p1) {
            g.part(Metal).tube_capped(a, b, 1.6, 14, true, true, rgba(mul(pal.steel, 0.45)));
            // Its tyres and piers.
            for t in [0.2f32, 0.55, 0.9] {
                let q = p0.lerp(p1, t);
                let y = 7.0 + (4.5 - 7.0) * t;
                let w = f.to_world(q.x, q.y);
                g.part(Concrete).boxed(Vec3::new(w.x, (y - 1.6) * 0.5, w.y), Vec3::new(2.2, y - 1.6, 2.2), 0.0, rgba(grey));
                let qa = f.at(q.x, y, q.y);
                let d = (b - a).normalize() * 0.25;
                g.part(Metal).tube_capped(qa - d, qa + d, 1.9, 14, true, true, rgba(mul(pal.steel, 0.35)));
            }
            // The hot end glows.
            let hot = b + (b - a).normalize() * 0.1;
            g.part(Glow).boxed(hot, Vec3::new(0.9, 0.9, 0.9), 0.0, rgba([3.0, 1.3, 0.3]));
        }
    }
    // Cement silos along the right, the limestone heap by the gate.
    silos(g, c, Vec2::new(area.u1 - 4.0, area.v1 - 4.0), Vec2::new(area.u1 - 4.0, area.v1 - 16.0), 3, 2.6, 22.0, mul(DUST, 0.95));
    heap(g, c, Vec2::new(area.u1 - 9.0, area.v0 + 8.0), 6.0, 4.0, mix(DUST, [0.8, 0.78, 0.7], 0.5));
    let _ = rng.random::<f32>();
}

/// A concrete batching plant: a row of cement silos on legs over a mixer
/// house, an inclined conveyor up from the aggregate bins, sand and gravel
/// heaps behind concrete block walls, a mixer lorry under the chute.
pub fn batching_plant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Dirt, mix(pal.gravel, DUST, 0.4));
    let area = R::of(&lot);
    let grey = mix(pal.concrete(0), DUST, 0.4);
    let mid = area.c();
    // The mixer house on legs, the silos beside it.
    let house = R::new(mid.x - 3.0, mid.x + 3.0, mid.y - 2.5, mid.y + 2.5);
    if let Some(r) = fit(c, house, 4.0) {
        for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u0, r.v1), (r.u1, r.v1)] {
            f.bx(g, Metal, u, 3.0, v, Vec3::new(0.4, 6.0, 0.4), mul(pal.steel, 0.5));
        }
        block(g, c, Metal, &r, 6.0, 11.0, [0.85, 0.75, 0.2]);
        // The chute and a mixer lorry standing under it.
        f.bx(g, Metal, r.c().x, 5.5, r.c().y, Vec3::new(0.8, 1.0, 0.8), [0.3, 0.3, 0.32]);
        let p = r.c();
        let w = f.to_world(p.x, p.y);
        g.part(Metal).cylinder_capped(w, 1.1, 0.6, 1.5, 3.4, 10, true, true, rgba([0.86, 0.86, 0.82]));
        f.bx(g, Matte, p.x, 0.8, p.y, Vec3::new(2.4, 1.0, 6.5), [0.85, 0.6, 0.04]);
        silos(g, c, Vec2::new(r.u1 + 3.0, r.v0 + 1.0), Vec2::new(r.u1 + 3.0 + 6.5, r.v0 + 1.0), 3, 1.5, 15.0, [0.82, 0.82, 0.8]);
        // The conveyor up from the bins.
        let bin = Vec2::new(r.u0 - 9.0, r.c().y);
        if c.inside_local(bin) {
            let (a, b) = (f.at(bin.x, 1.0, bin.y), f.at(r.u0, 10.0, r.c().y));
            g.part(Metal).beam(a, b, 0.8, rgba(mul(pal.steel, 0.6)));
        }
    }
    // Aggregate bays: block walls and heaps of sand and gravel.
    for (k, col) in [[0.72, 0.62, 0.42], [0.55, 0.53, 0.5], [0.66, 0.6, 0.5]].iter().enumerate() {
        let p = Vec2::new(area.u0 + 5.0 + k as f32 * 7.0, area.v1 - 5.0);
        heap(g, c, p, 3.0, 2.4, *col);
        if c.inside_local(p + Vec2::new(3.3, 0.0)) && distance_to_boundary(p + Vec2::new(3.3, 0.0), &c.lot_poly) > 3.3 {
            f.bx(g, Concrete, p.x + 3.3, 1.0, p.y, Vec3::new(0.6, 2.0, 6.0), grey);
        }
    }
    let _ = rng.random::<f32>();
}

// ── timber ───────────────────────────────────────────────────────────────────

/// A saw mill: a long open-sided saw shed with a lit band, the log yard
/// (stacks of logs, bark showing, pale ends), packs of sawn boards under
/// a lean-to, a sawdust heap and a log loader.
pub fn saw_mill(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Dirt, mix(pal.dirt, [0.5, 0.38, 0.24], 0.4));
    let area = R::of(&lot);
    // The saw shed along the front half.
    let want = R::new(area.u0 + 1.5, area.u1 - 1.5, area.v0 + 2.0, area.v0 + (area.d() * 0.35).clamp(8.0, 16.0));
    let mut back = area.v0 + 2.0;
    if let Some(r) = fit(c, want, 6.0) {
        back = r.v1 + 1.5;
        let wall = mix(pick(rng, &pal.panel), [0.45, 0.35, 0.25], 0.4);
        // Posts and a pitched roof; walls only at the ends.
        let n = ((r.w() / 5.0).floor() as i32).max(2);
        for k in 0..=n {
            let u = r.u0 + r.w() * k as f32 / n as f32;
            for v in [r.v0 + 0.2, r.v1 - 0.2] {
                f.bx(g, Metal, u.clamp(r.u0 + 0.2, r.u1 - 0.2), 3.5, v, Vec3::new(0.3, 7.0, 0.3), mul(pal.steel, 0.5));
            }
        }
        f.bx(g, Metal, r.c().x, 7.2, r.c().y, Vec3::new(r.w() + 0.4, 0.35, r.d() + 0.4), mul(wall, 0.7));
        block(g, c, WornPanel, &R::new(r.u0, r.u0 + 0.3, r.v0, r.v1), 0.0, 7.0, wall);
        block(g, c, WornPanel, &R::new(r.u1 - 0.3, r.u1, r.v0, r.v1), 0.0, 7.0, wall);
        // The saw line: a long bench with the blade housing.
        f.bx(g, Metal, r.c().x, 0.6, r.c().y, Vec3::new(r.w() * 0.8, 1.2, 1.6), [0.35, 0.36, 0.38]);
        f.bx(g, Metal, r.c().x, 1.7, r.c().y, Vec3::new(1.6, 1.4, 2.0), [0.85, 0.55, 0.1]);
        f.bx(g, Glow, r.c().x, 6.6, r.v0 + 0.25, Vec3::new(r.w() * 0.6, 0.12, 0.06), [2.4, 1.6, 0.6]);
        // Boards on the out-feed.
        for k in 0..3 {
            let u = r.u0 + 2.0 + k as f32 * 2.6;
            if u < r.u1 - 2.0 {
                f.bx(g, Wood, u, 0.55, r.v1 - 1.6, Vec3::new(2.2, 1.0, 1.2), SAWN);
            }
        }
    }
    // The log yard behind: stacks of logs lying along the lot.
    let rows = (((area.v1 - 2.0 - back) / 4.5).floor() as i32).clamp(0, 4);
    for row in 0..rows {
        let v = back + 2.2 + row as f32 * 4.5;
        let u0 = area.u0 + 3.0;
        let u1 = area.u1 - 10.0;
        if u1 - u0 < 6.0 {
            continue;
        }
        for layer in 0..3 {
            let y = 0.4 + layer as f32 * 0.7;
            for k in 0..(4 - layer) {
                let vv = v - 1.2 + k as f32 * 0.8 + layer as f32 * 0.4;
                let a = Vec2::new(u0, vv);
                let b = Vec2::new(u1, vv);
                if !(c.inside_local(a) && c.inside_local(b)) || distance_to_boundary(a, &c.lot_poly) < 0.5 || distance_to_boundary(b, &c.lot_poly) < 0.5 {
                    continue;
                }
                g.part(Wood).tube_capped(f.at(a.x, y, a.y), f.at(b.x, y, b.y), 0.36, 7, true, true, rgba(if (row + k + layer) % 2 == 0 { BARK } else { mul(BARK, 1.2) }));
            }
        }
    }
    // Board packs and the sawdust heap at the far end.
    pallets(g, c, rng, Vec2::new(area.u1 - 8.0, back + 2.0), 6, &[SAWN, mul(SAWN, 0.9)]);
    heap(g, c, Vec2::new(area.u1 - 5.0, area.v1 - 5.0), 3.5, 3.0, [0.82, 0.66, 0.42]);
}

// ── textiles ─────────────────────────────────────────────────────────────────

/// A textile mill: a long brick mill under a sawtooth roof whose north
/// lights glow at night, a square chimney, a loading dock with bales of
/// cotton and bolts of cloth.
pub fn textile_mill(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let lot = yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.75));
    let area = R::of(&lot);
    let brick = pick(rng, &pal.brick);
    let floors = c.floors.clamp(1, 4) as i32;
    let h = 4.5 * floors as f32;
    let want = R::new(area.u0 + 1.5, area.u1 - 1.5, area.v0 + 2.0, area.v1 - 7.0);
    if let Some(r) = fit(c, want, 7.0) {
        block(g, c, Brick, &r, 0.0, h, brick);
        for face in r.faces() {
            if face.len > 5.0 {
                pane_row(g, c, rng, &face, 1.2, floors, 4.5, 2.6, 1.6, 2.2, true);
            }
        }
        // The sawtooth: teeth across the roof, the glazed faces lit.
        let n = ((r.d() / 5.0).floor() as i32).max(1);
        let tw = r.d() / n as f32;
        for k in 0..n {
            let v = r.v0 + (k as f32 + 0.5) * tw;
            f.bx(g, Metal, r.c().x, h + 1.0, v, Vec3::new(r.w() - 0.2, 2.0, tw * 0.55), mul(pal.steel, 0.5));
            f.bx(g, Glow, r.c().x, h + 1.0, v + tw * 0.28, Vec3::new(r.w() - 0.6, 1.6, 0.05), [1.6, 1.4, 0.9]);
        }
        chimney(g, f, Vec2::new(r.u1 - 2.0, r.v1 - 2.0), 1.1, h, 14.0, mul(brick, 0.8), pal.aviation_light);
        sign_board(g, c, rng, &r.front(), 0.0, h - 1.0, (r.w() * 0.4).clamp(3.0, 10.0), 0.9);
    }
    // Bales and bolts on the dock at the back.
    pallets(g, c, rng, Vec2::new(area.u0 + 3.0, area.v1 - 4.5), 6, &[[0.92, 0.9, 0.85], [0.6, 0.15, 0.4], [0.15, 0.3, 0.6]]);
}

/// The fashion district's wholesaler: a two or three storey row of shop
/// units with roller shutters up, bolts and boxes stacked inside, and out
/// on the pavement racks of clothes, mannequins and stalls under awnings
/// in loud colours (LA's Fashion District, Harvey 2026-11-22).
pub fn fashion_wholesale(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.lot_poly);
    let floors = c.floors.clamp(2, 3) as i32;
    let h = floors as f32 * 3.6;
    // The shop fronts on the lot line, so what they put out stands on the pavement.
    let want = R::new(area.u0 + 0.3, area.u1 - 0.3, area.v0 + 0.3, (area.v0 + 0.3 + (area.d() - 1.0).clamp(6.0, 18.0)).min(area.v1 - 0.3));
    let Some(r) = fit(c, want, 5.0) else { return };
    let wall = pick(rng, &pal.plaster);
    block(g, c, Plaster, &r, 0.0, h, wall);
    parapet(g, c, &r.poly(), h, 0.8, 0.25, Matte, mul(wall, 0.8));
    let front = r.front();
    for face in r.faces() {
        if face.len > 4.0 && !face.is_front {
            grime(g, c, rng, &face, 0.0, h, 2.0 + 2.0 * c.wear);
        }
    }
    pane_row(g, c, rng, &front, 3.6 + 1.0, floors - 1, 3.6, 2.4, 1.4, 1.5, true);
    // Shop units along the front: dark openings (shutters up), stock inside,
    // a lit sign over each.
    let units = ((r.w() / 5.0).floor() as i32).max(1);
    let uw = r.w() / units as f32;
    let loud = [[0.95, 0.2, 0.55], [0.15, 0.75, 0.9], [0.95, 0.8, 0.1], [0.6, 0.2, 0.9], [0.2, 0.85, 0.35]];
    for k in 0..units {
        let t = -r.w() * 0.5 + (k as f32 + 0.5) * uw;
        front.decal(g, f, Matte, t, 1.5, 0.03, uw - 0.6, 2.8, [0.05, 0.05, 0.06]);
        front.decal(g, f, Matte, t, 2.95, 0.05, uw - 0.4, 0.25, mul(pal.steel, 0.6));
        let sc = loud[(k as usize + rng.random_range(0..5)) % loud.len()];
        front.decal(g, f, Glow, t, 3.3, 0.08, uw - 0.8, 0.45, mul(sc, 1.6));
        // Out on the pavement (beyond the lot line, a step at most): a rack
        // of clothes on a rail, a mannequin, a stall under an awning.
        let d = 0.55;
        front.bx(g, f, Metal, t, 1.6, d, Vec3::new(uw * 0.55, 0.05, 0.05), [0.6, 0.6, 0.62]);
        for s in [-1.0f32, 1.0] {
            front.bx(g, f, Metal, t + s * uw * 0.27, 0.8, d, Vec3::new(0.05, 1.6, 0.05), [0.6, 0.6, 0.62]);
        }
        let hangers = ((uw * 0.55) / 0.3) as i32;
        for i in 0..hangers {
            let tt = t - uw * 0.26 + i as f32 * 0.3 + 0.15;
            let col = loud[(i as usize + k as usize * 2) % loud.len()];
            front.bx(g, f, Tarp, tt, 1.05, d, Vec3::new(0.06, 1.0, 0.5), mul(col, 0.8));
        }
        if k % 2 == 0 {
            // A mannequin by the door.
            let p = front.at(t + uw * 0.38, 0.4);
            let w = f.to_world(p.x, p.y);
            g.part(Matte).cylinder(w, 0.18, 0.12, 0.0, 1.2, 6, rgba([0.85, 0.82, 0.78]));
            g.part(Tarp).cylinder(w, 0.28, 0.2, 0.7, 1.45, 6, rgba(loud[k as usize % loud.len()]));
            g.part(Matte).cylinder(w, 0.1, 0.1, 1.45, 1.7, 6, rgba([0.85, 0.82, 0.78]));
        } else {
            // A stall's awning over boxes of shirts and scarves.
            front.bx(g, f, Tarp, t + uw * 0.1, 2.3, 0.2, Vec3::new(uw * 0.8, 0.08, 1.1), mul(pick(rng, &pal.awning_colors), 0.95));
            front.bx(g, f, Wood, t + uw * 0.1, 0.45, 0.4, Vec3::new(uw * 0.6, 0.9, 0.7), pal.wood);
            front.bx(g, f, Tarp, t + uw * 0.1, 0.95, 0.4, Vec3::new(uw * 0.55, 0.12, 0.6), loud[(k as usize + 3) % loud.len()]);
        }
    }
    // A string of bunting lights along the frontage.
    let (a, b) = (front.at(-r.w() * 0.5 + 0.3, 0.2), front.at(r.w() * 0.5 - 0.3, 0.2));
    string_lights(g, f, Vec3::new(a.x, 3.7, a.y), Vec3::new(b.x, 3.7, b.y), units * 3, pick(rng, &pal.sign_colors));
}
