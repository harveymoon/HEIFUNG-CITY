//! [r4-oldstone] Old Stone's small industry (the Old Town Charter's
//! works): the old mill with its water wheel and race, the tannery with
//! its pits and drying lofts, and the cobbled yards of the carters and
//! coopers. Drawn as `oldstone.rs` draws the town: weathered stone, black
//! oak, steep roofs, gas lamps lit after dark, the small parts in the
//! detail layers.

use super::kit::*;
use super::oldstone::{chimney, cobbles, cottage, gateway, lamp_post, paint, range, roof, steep, wall_lantern, window_rows, Keep, Paint, Range, Set, GAS, HEAD, IRON, TIMBER};
use super::parts::*;
use super::{Ctx, Face};
use crate::depth::{FRAME, PANE};
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

/// Mill-race water, dark and green.
const WATER: [f32; 3] = [0.10, 0.17, 0.18];
/// The liquors in the tan pits: lime, bark tan, dye.
const LIQUORS: [[f32; 3]; 4] = [[0.46, 0.45, 0.40], [0.24, 0.15, 0.08], [0.18, 0.11, 0.06], [0.30, 0.21, 0.09]];
/// Hides drying: raw, tanned, dyed.
const HIDES: [[f32; 3]; 4] = [[0.62, 0.50, 0.38], [0.46, 0.28, 0.16], [0.36, 0.20, 0.12], [0.30, 0.12, 0.08]];

/// A two-wheeled cart at lot-local `p`, turned `yaw` (radians from the
/// frame's u): the bed, the shafts down to the ground, the wheels (detail
/// spokes), a load of sacks or barrels.
pub(super) fn cart(g: &mut Geometry, c: &Ctx, p: Vec2, yaw: f32, pt: &Paint) {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let s = d.perp();
    let reach = [p + d * 2.4, p - d * 1.2, p + s * 1.0, p - s * 1.0];
    if reach.iter().any(|q| !c.inside_local(*q) || distance_to_boundary(*q, &c.lot_poly) < 0.3) {
        return;
    }
    let f = &c.frame;
    let wood = mul(pt.timber, 1.8);
    f.bx_yaw(g, Wood, p.x, 1.0, p.y, Vec3::new(2.2, 0.16, 1.3), wood, yaw);
    for side in [-1.0f32, 1.0] {
        f.bx_yaw(g, Wood, p.x + s.x * side * 0.63, 1.25, p.y + s.y * side * 0.63, Vec3::new(2.2, 0.4, 0.06), wood, yaw);
        let hub = p + s * side * 0.8;
        let ax = Axis::level(f, hub, hub + s * side, 0.62);
        revolve(g.part(Wood), &ax, &[(0.0, -0.06), (0.6, -0.06), (0.6, 0.06), (0.0, 0.06)], 12, None, rgba(mul(pt.timber, 1.3)));
        let a = f.at2(p + s * side * 0.55 + d * 1.0, 0.95);
        let b = f.at2(p + s * side * 0.45 + d * 2.3, 0.05);
        g.part(Wood).beam(a, b, 0.08, rgba(wood));
    }
    g.lod(Lod::Mid, |g| {
        for k in 0..3 {
            let q = p + d * (-0.6 + k as f32 * 0.6);
            g.part(Tarp).cylinder(f.to_world(q.x, q.y), 0.3, 0.26, 1.08, 1.6, 6, rgba([0.62, 0.56, 0.42]));
        }
    });
}

/// Barrels stood in a cluster round lot-local `p`.
fn barrels(g: &mut Geometry, c: &Ctx, p: Vec2, n: i32, pt: &Paint) {
    let f = &c.frame;
    for k in 0..n {
        let q = p + Vec2::new((k % 3) as f32 * 0.75, (k / 3) as f32 * 0.75);
        if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.5 {
            let w = f.to_world(q.x, q.y);
            g.part(Wood).cylinder_capped(w, 0.3, 0.3, 0.04, 0.92, 8, false, true, rgba(mul(pt.timber, 2.2)));
            g.lod(Lod::Near, |g| {
                for y in [0.22f32, 0.72] {
                    g.part(Matte).cylinder_capped(w, 0.315, 0.315, y, y + 0.06, 8, false, false, rgba(IRON));
                }
            });
        }
    }
}

// ── the old mill ─────────────────────────────────────────────────────────────

/// The old mill: a tall stone mill house of three or four storeys under a
/// steep roof, a lucam (the hoist housing) jutting from the top storey over
/// the loading door, a big wooden water wheel turning in its stone-lined
/// race down one side, sacks on a cart in a cobbled yard, the miller's
/// lamp at the door.
pub fn water_mill(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.0, 3.6);
    let floors = (c.floors as i32).clamp(3, 4);
    let area = R::of(&c.poly);
    let left = coin(rng, 0.5);
    // Room down one side for the wheel and its race.
    let race_w = 3.4;
    let w = (area.w() - race_w - 2.0).clamp(7.0, 16.0);
    let d = (area.d() * 0.5).clamp(8.0, 14.0).min(area.d() - 2.0);
    let (u0, u1) = if left { (area.u0 + race_w + 1.0, area.u0 + race_w + 1.0 + w) } else { (area.u1 - race_w - 1.0 - w, area.u1 - race_w - 1.0) };
    let Some(r) = fit(c, R::new(u0, u1, area.v0 + 2.0, area.v0 + 2.0 + d), 6.0) else {
        cobbled_yard(g, c, rng);
        return;
    };
    let rg = Range { r, floors, fh, timber: false, jetty: 0.0, along_u: false, rise: steep(r.w()).min(7.0), eave: 0.3, bay: 3.0, blank: [false; 4] };
    let keep = [Keep { face: 0, t: 0.0, half: 1.2, all: true }];
    let h = range(g, c, rng, &rg, &pt, &keep);
    let front = r.front();
    // Stacked loading doors up the front to the lucam.
    for k in 0..floors {
        let y = k as f32 * fh + fh * 0.45;
        front.decal(g, f, Stone, 0.0, y, FRAME, 1.9, fh * 0.75, pt.trim);
        front.decal(g, f, Wood, 0.0, y, PANE, 1.6, fh * 0.68, mul(pt.door, 0.8));
    }
    if c.reach_out(&front) > 1.6 {
        let lw = 2.6;
        front.bx(g, f, Wood, 0.0, h - fh * 0.5, 0.7, Vec3::new(lw, fh, 1.4), pt.timber);
        let lr = front.at(0.0, 0.7);
        roof(g, c, &R::new(lr.x - lw * 0.5 - 0.1, lr.x + lw * 0.5 + 0.1, lr.y - 0.8, lr.y + 0.8), h, 1.0, true, 0.1, pt.roof, (Wood, pt.timber));
        g.lod(Lod::Mid, |g| front.bx(g, f, Matte, 0.0, h - fh - 0.6, 1.0, Vec3::new(0.04, 1.4, 0.04), IRON));
    }
    wall_lantern(g, c, &front, 1.6, 2.6);
    let ridge = h + rg.rise;
    chimney(g, c, Vec2::new(r.c().x, r.v1 - 1.0), ridge - 2.0, ridge + 0.8, 1.1, &pt);
    // The race along the side and the wheel in it.
    let side = if left { r.left() } else { r.right() };
    let ru = if left { r.u0 - race_w * 0.5 - 0.1 } else { r.u1 + race_w * 0.5 + 0.1 };
    let race = fit(c, R::new(ru - race_w * 0.5, ru + race_w * 0.5, r.v0 - 1.0, r.v1 + 1.0), race_w * 0.8);
    let wr = (h * 0.3).clamp(2.6, 4.2).min(r.d() * 0.45);
    let hub = Vec2::new(ru, r.c().y);
    // The wheel's reach in plan must stand on the lot.
    let wheel_ok = poly_inside(c, &rect(hub.x, hub.y, race_w * 0.5, wr + 0.3));
    if let Some(rr) = race.filter(|rr| wheel_ok && rr.u0 <= hub.x - 0.9 && rr.u1 >= hub.x + 0.9) {
        let rp = rr.poly();
        ground(g, f, Glass, &rp, 0.05, WATER);
        for u in [rr.u0 + 0.2, rr.u1 - 0.2] {
            f.bx(g, Stone, u, 0.35, rr.c().y, Vec3::new(0.4, 0.7, rr.d()), pt.dark);
        }
        let width = (rr.w() - 1.2).min(1.8);
        let yc = wr + 0.1;
        let axis = Vec2::new(if left { 1.0 } else { -1.0 }, 0.0);
        let a = hub - axis * width * 0.5;
        let col = rgba(mul(pt.timber, 1.6));
        let n = 16;
        let pt_at = |k: i32, rad: f32, off: f32| {
            let ang = k as f32 / n as f32 * TAU;
            let q = a + axis * off;
            f.at2(q, yc) + f.dir3(Vec2::new(0.0, 1.0)) * ang.cos() * rad + Vec3::Y * ang.sin() * rad
        };
        for k in 0..n {
            // Two rims and the paddles across them.
            for off in [0.0, width] {
                g.part(Wood).beam(pt_at(k, wr, off), pt_at(k + 1, wr, off), 0.16, col);
            }
            g.part(Wood).beam(pt_at(k, wr - 0.15, -0.05), pt_at(k, wr - 0.15, width + 0.05), 0.3, col);
        }
        g.part(Wood).beam(pt_at(0, 0.0, -0.3), f.at2(side.at(0.0, 0.0) + Vec2::new(0.0, hub.y - side.mid().y), yc), 0.3, rgba(IRON));
        g.lod(Lod::Mid, |g| {
            for k in (0..n).step_by(2) {
                for off in [0.0, width] {
                    g.part(Wood).beam(pt_at(0, 0.0, off), pt_at(k, wr, off), 0.1, col);
                }
            }
        });
        // The head race's trough coming in over the wheel.
        let a3 = f.at2(Vec2::new(ru, rr.v1 - 0.3), yc + wr + 0.4);
        let b3 = f.at2(Vec2::new(ru, hub.y + 0.6), yc + wr + 0.3);
        if a3.distance(b3) > 1.0 {
            g.part(Wood).beam(a3, b3, 0.6, rgba(pt.timber));
        }
    }
    // The yard in front: setts, a cart of sacks, a lamp.
    let yp = clip_box(&c.poly, r.u0, r.u1, area.v0 + 0.3, r.v0 - 0.1);
    if yp.len() >= 3 {
        cobbles(g, c, &yp, 0.04, &pt);
    }
    let back = clip_box(&c.poly, r.u0, r.u1, r.v1 + 0.3, area.v1 - 0.3);
    if back.len() >= 3 && R::of(&back).d() > 4.0 {
        cobbles(g, c, &back, 0.04, &pt);
        let br = R::of(&back);
        cart(g, c, Vec2::new(br.c().x, br.c().y), 0.0, &pt);
        lamp_post(g, c, Vec2::new(br.u0 + 1.2, br.v0 + 1.2), 3.4);
    } else {
        lamp_post(g, c, Vec2::new(r.u0 + 0.8, area.v0 + 1.0), 3.4);
    }
}

// ── the tannery ──────────────────────────────────────────────────────────────

/// The tannery: a long stone range with a slatted drying loft under its
/// roof, the hides hung on rails in the yard, rows of stone-kerbed pits of
/// lime and bark liquor, barrels and a bark stack, a stone chimney, a lamp.
pub fn tannery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.0, 3.6);
    let area = R::of(&c.poly);
    let d = (area.d() * 0.32).clamp(6.0, 10.0);
    let Some(r) = fit(c, R::new(area.u0 + 0.4, area.u1 - 0.4, area.v1 - 0.4 - d, area.v1 - 0.4), 5.0) else {
        cobbled_yard(g, c, rng);
        return;
    };
    // Ground floor of stone, the drying loft of slats above it.
    let rg = Range { r, floors: 1, fh, timber: false, jetty: 0.0, along_u: true, rise: steep(r.d()).min(5.0), eave: 0.4, bay: 3.0, blank: [false, false, true, false] };
    block(g, c, Stone, &r, 0.0, fh, pt.stone);
    let lh = fh * 0.9;
    block(g, c, Wood, &r, fh, fh + lh, mul(pt.timber, 1.2));
    let top = fh + lh;
    cornice(g, c, &r.poly(), fh + 0.1, 0.22, 0.08, Wood, pt.timber);
    for (i, face) in r.faces().iter().enumerate() {
        if face.len < 2.0 {
            continue;
        }
        if i != 2 {
            window_rows(g, c, rng, face, 0.0, 1, fh, 3.4, 0.4, 0.45, &pt, Set::Stone, &[(0.0, 1.5, false)]);
        }
        // The slatted loft: dark gaps between louvres (detail), wide openings.
        face.decal(g, f, Matte, 0.0, fh + lh * 0.5, HEAD, face.len - 0.8, lh - 0.4, DARK);
        g.lod(Lod::Mid, |g| {
            for k in 0..((lh - 0.4) / 0.3) as i32 {
                face.decal(g, f, Wood, 0.0, fh + 0.35 + k as f32 * 0.3, TIMBER, face.len - 0.8, 0.16, mul(pt.timber, 1.6));
            }
        });
        grime(g, c, rng, face, 0.0, fh, 1.5 + 2.0 * c.wear);
    }
    roof(g, c, &r, top, rg.rise, true, rg.eave, pt.roof, (Wood, mul(pt.timber, 1.2)));
    let front = r.front();
    gateway(g, c, &front, 0.0, 2.2, 1.9, mul(pt.door, 0.75), &pt, true);
    wall_lantern(g, c, &front, 2.2, 2.6);
    chimney(g, c, Vec2::new(r.u1 - 1.4, r.c().y), top + rg.rise - 1.5, top + rg.rise + 2.4, 1.2, &pt);
    // The yard: setts, the pits, the hide rails.
    let yr = R::new(area.u0 + 0.4, area.u1 - 0.4, area.v0 + 0.4, r.v0 - 0.4);
    if yr.d() > 4.0 {
        let yp = clip_box(&c.poly, yr.u0, yr.u1, yr.v0, yr.v1);
        cobbles(g, c, &yp, 0.04, &pt);
        let rows = ((yr.d() - 3.0) / 2.6).floor().clamp(0.0, 4.0) as i32;
        let cols = ((yr.w() * 0.55) / 2.4).floor().clamp(0.0, 6.0) as i32;
        for j in 0..rows {
            for i in 0..cols {
                let p = Vec2::new(yr.u0 + 1.6 + i as f32 * 2.4, yr.v1 - 1.6 - j as f32 * 2.6);
                let pit = rect(p.x, p.y, 0.95, 1.05);
                if !poly_inside(c, &pit) {
                    continue;
                }
                extrude(g, f, Stone, &rect(p.x, p.y, 1.05, 1.15), 0.0, 0.35, pt.trim);
                ground(g, f, Matte, &pit, 0.37, LIQUORS[(i + j) as usize % LIQUORS.len()]);
            }
        }
        // Hides on rails on the other side.
        let hu0 = yr.u0 + 1.6 + cols as f32 * 2.4;
        let n = ((yr.u1 - 0.8 - hu0) / 1.8) as i32;
        for k in 0..n.clamp(0, 4) {
            let u = hu0 + 0.6 + k as f32 * 1.8;
            let (a, b) = (Vec2::new(u, yr.v0 + 1.0), Vec2::new(u, yr.v1 - 1.0));
            if !c.spans_lot(a, b) || b.y - a.y < 2.0 {
                continue;
            }
            for v in [a.y, b.y] {
                f.bx(g, Wood, u, 1.2, v, Vec3::new(0.12, 2.4, 0.12), pt.timber);
            }
            f.bx(g, Wood, u, 2.4, (a.y + b.y) * 0.5, Vec3::new(0.1, 0.1, b.y - a.y), pt.timber);
            let m = ((b.y - a.y) / 1.1) as i32;
            for j in 0..m {
                let v = a.y + 0.6 + j as f32 * 1.1;
                f.bx(g, Tarp, u, 1.75, v, Vec3::new(0.04, 1.2, 0.9), HIDES[(k + j) as usize % HIDES.len()]);
            }
        }
        barrels(g, c, Vec2::new(yr.u0 + 0.8, yr.v0 + 0.8), 5, &pt);
        g.lod(Lod::Mid, |g| {
            // The bark stack under a lean-to.
            let p = Vec2::new(yr.u1 - 1.6, yr.v0 + 1.2);
            if poly_inside(c, &rect(p.x, p.y, 1.2, 0.8)) {
                f.bx(g, Wood, p.x, 0.5, p.y, Vec3::new(2.2, 1.0, 1.4), [0.36, 0.24, 0.14]);
            }
        });
        lamp_post(g, c, Vec2::new(yr.c().x, yr.v0 + 1.0), 3.4);
    }
}

// ── cobbled yards ────────────────────────────────────────────────────────────

/// A carter's or cooper's yard: an L of low stone workshops with great oak
/// doors, a cobbled yard entered under an arched gateway in a high wall, a
/// cart, stacked barrels, a pump and trough, a lamp post.
pub fn cobbled_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.0, 3.6);
    let area = R::of(&c.poly);
    let Some(site) = fit(c, area.inset(0.4), 6.0) else {
        cottage(g, c, rng, &pt);
        return;
    };
    let floors = (c.floors as i32).clamp(1, 2);
    let wd = (site.d() * 0.3).clamp(5.0, 8.0);
    let left = coin(rng, 0.5);
    // The back range and the side range.
    let back = R::new(site.u0, site.u1, site.v1 - wd, site.v1);
    let sw = (site.w() * 0.3).clamp(5.0, 8.0);
    let side = if left { R::new(site.u0, site.u0 + sw, site.v0 + 3.0, back.v0) } else { R::new(site.u1 - sw, site.u1, site.v0 + 3.0, back.v0) };
    for (k, (r, along_u)) in [(back, true), (side, false)].into_iter().enumerate() {
        if r.w() < 3.5 || r.d() < 3.5 {
            continue;
        }
        let span = if along_u { r.d() } else { r.w() };
        // The side range's back butts on the back range.
        let blank = if k == 0 { [false, false, false, false] } else { [false, false, true, false] };
        let rg = Range { r, floors, fh: fh * 1.1, timber: floors > 1 && coin(rng, 0.4), jetty: 0.0, along_u, rise: steep(span).min(4.5), eave: 0.3, bay: 3.2, blank };
        let inner = if k == 0 { r.front() } else if left { r.right() } else { r.left() };
        let nd = ((inner.len - 1.0) / 4.0).floor().clamp(1.0, 4.0) as i32;
        let step = (inner.len - 1.0) / nd as f32;
        let doors: Vec<(f32, f32)> = (0..nd).map(|i| (-(inner.len - 1.0) * 0.5 + (i as f32 + 0.5) * step, 1.4)).collect();
        let fi = if k == 0 { 0 } else if left { 1 } else { 3 };
        let keep: Vec<Keep> = doors.iter().map(|(t, w)| Keep { face: fi, t: *t, half: *w, all: false }).collect();
        range(g, c, rng, &rg, &pt, &keep);
        for (t, _) in &doors {
            inner.decal(g, f, Stone, *t, 1.4, FRAME, 2.8, 2.8, pt.trim);
            inner.decal(g, f, Wood, *t, 1.3, PANE, 2.4, 2.6, mul(pt.door, 0.8));
            g.lod(Lod::Near, |g| {
                inner.decal(g, f, Wood, *t, 1.3, PANE + 0.008, 0.08, 2.6, pt.timber);
                inner.decal(g, f, Wood, *t, 1.6, PANE + 0.012, 2.4, 0.1, pt.timber);
            });
        }
        wall_lantern(g, c, &inner, doors.first().map_or(0.0, |d| d.0) + 1.7, 2.8);
    }
    // The yard: setts, the wall and gateway to the street.
    let yr = R::new(if left { side.u1 } else { site.u0 }, if left { site.u1 } else { side.u0 }, site.v0, back.v0);
    let yp = clip_box(&c.poly, site.u0, site.u1, site.v0, back.v0);
    cobbles(g, c, &yp, 0.04, &pt);
    let wall = Face::new(Vec2::new(site.u0, site.v0 + 0.3), Vec2::new(site.u1, site.v0 + 0.3), Vec2::new(0.0, -1.0));
    let gw = 3.4f32.min(wall.len * 0.3);
    let gt = yr.c().x - wall.mid().x;
    for (a, b) in [(-wall.len * 0.5, gt - gw * 0.5 - 0.4), (gt + gw * 0.5 + 0.4, wall.len * 0.5)] {
        if b - a > 0.4 {
            wall.bx(g, f, Stone, (a + b) * 0.5, 1.4, 0.0, Vec3::new(b - a, 2.8, 0.45), pt.stone);
        }
    }
    // The arch over the gate: piers, the lintel block, the name.
    for s in [-1.0f32, 1.0] {
        wall.bx(g, f, Stone, gt + s * (gw * 0.5 + 0.2), 1.9, 0.0, Vec3::new(0.5, 3.8, 0.6), pt.trim);
    }
    wall.bx(g, f, Stone, gt, 4.1, 0.0, Vec3::new(gw + 1.1, 0.7, 0.6), pt.trim);
    let from = g.get(Glow).colors.len();
    wall.bx(g, f, Glow, gt, 4.75, -0.0, Vec3::new(0.3, 0.42, 0.3), GAS);
    mark_glow(g, from, NIGHT_MARK);
    g.lod(Lod::Mid, |g| {
        let letters = ((gw / 0.5) as i32).clamp(3, 8);
        for i in 0..letters {
            wall.decal(g, f, Matte, gt - gw * 0.4 + (i as f32 + 0.5) * gw * 0.8 / letters as f32, 4.1, 0.31, gw * 0.8 / letters as f32 * 0.6, 0.3, IRON);
        }
    });
    if yr.w() > 5.0 && yr.d() > 6.0 {
        cart(g, c, Vec2::new(yr.c().x, yr.c().y), FRAC_PI_2 * 0.3, &pt);
        barrels(g, c, Vec2::new(yr.u1 - 2.6, back.v0 - 1.0), 6, &pt);
        // The pump and the trough.
        let p = Vec2::new(yr.u0 + 1.2, yr.v0 + 2.6);
        if poly_inside(c, &rect(p.x + 0.8, p.y, 1.2, 0.5)) {
            f.bx(g, Stone, p.x + 0.8, 0.35, p.y, Vec3::new(2.0, 0.7, 0.8), pt.trim);
            ground(g, f, Glass, &rect(p.x + 0.8, p.y, 0.85, 0.28), 0.71, WATER);
            g.part(Matte).cylinder(f.to_world(p.x - 0.4, p.y), 0.12, 0.1, 0.0, 1.5, 6, rgba(IRON));
            g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x - 0.1, 1.35, p.y, Vec3::new(0.6, 0.08, 0.08), IRON));
        }
        lamp_post(g, c, Vec2::new(yr.c().x + 1.5, yr.v0 + 1.2), 3.4);
    }
}
