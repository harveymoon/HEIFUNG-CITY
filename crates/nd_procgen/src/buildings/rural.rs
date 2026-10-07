//! [economy] The new primary and dirty trades (`nd_sim::market::BusinessKind`):
//! cattle and chicken farms, a slaughterhouse, a strip mine, a private
//! trash company, fish gutting sheds and a fish-stick factory. Forced by
//! the business on the lot; size, materials and condition follow the tier.

use super::parts::*;
use super::trades::{block, fish_crates, fit, pane, pane_row, R};
use super::{Ctx, Face, Frame};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

// ── shared props ─────────────────────────────────────────────────────────────

/// A cow as a few boxes: body, head, two leg slabs and a patch on the coat.
pub(super) fn cow(g: &mut Geometry, f: &Frame, p: Vec2, yaw: f32, coat: [f32; 3], patch: Option<[f32; 3]>) {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    f.bx_yaw(g, Matte, p.x, 1.05, p.y, Vec3::new(1.9, 0.75, 0.8), coat, yaw);
    let head = p + d * 1.15;
    f.bx_yaw(g, Matte, head.x, 1.25, head.y, Vec3::new(0.5, 0.45, 0.4), mul(coat, 0.9), yaw);
    for s in [-0.65f32, 0.65] {
        let q = p + d * s;
        f.bx_yaw(g, Matte, q.x, 0.35, q.y, Vec3::new(0.2, 0.7, 0.66), mul(coat, 0.8), yaw);
    }
    if let Some(pc) = patch {
        let q = p - d * 0.2;
        f.bx_yaw(g, Matte, q.x, 1.08, q.y, Vec3::new(0.6, 0.78, 0.84), pc, yaw);
    }
}

/// A cow's coat: Holstein, brown or black.
fn coat(rng: &mut impl Rng) -> ([f32; 3], Option<[f32; 3]>) {
    match rng.random_range(0..4) {
        0 | 1 => ([0.85, 0.84, 0.8], Some([0.08, 0.08, 0.08])),
        2 => ([0.42, 0.24, 0.12], None),
        _ => ([0.1, 0.09, 0.08], Some([0.85, 0.84, 0.8])),
    }
}

/// Post-and-rail fence from `a` to `b` (lot-local).
pub(super) fn rail_fence(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let n = (len / 3.0).ceil() as i32;
    for k in 0..=n {
        let p = a.lerp(b, k as f32 / n as f32);
        f.bx(g, Wood, p.x, h * 0.5, p.y, Vec3::new(0.14, h, 0.14), color);
    }
    let yaw = (b - a).y.atan2((b - a).x);
    let m = (a + b) * 0.5;
    for y in [h * 0.45, h * 0.85] {
        f.bx_yaw(g, Wood, m.x, y, m.y, Vec3::new(len, 0.1, 0.06), color, yaw);
    }
}

/// Fence round a polygon, inset a little.
fn fence_round(g: &mut Geometry, c: &Ctx, poly: &[Vec2], h: f32, color: [f32; 3]) {
    let n = poly.len();
    for i in 0..n {
        rail_fence(g, c, poly[i], poly[(i + 1) % n], h, color);
    }
}

/// A gabled roof whose ridge runs along `v` (for buildings deep in `v`).
fn ridge_v(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, color: [f32; 3]) {
    let bottom = rect(r.c().x, r.c().y, r.w() * 0.5 + 0.3, r.d() * 0.5 + 0.2);
    let top = rect(r.c().x, r.c().y, 0.08, r.d() * 0.5 + 0.2);
    loft(g, &c.frame, Metal, &bottom, &top, y, y + rise, color);
}

/// Gabled roof whose ridge runs along `u`.
fn ridge_u(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, color: [f32; 3]) {
    let bottom = rect(r.c().x, r.c().y, r.w() * 0.5 + 0.2, r.d() * 0.5 + 0.3);
    let top = rect(r.c().x, r.c().y, r.w() * 0.5 + 0.2, 0.08);
    loft(g, &c.frame, Metal, &bottom, &top, y, y + rise, color);
}

/// A silo: a tank with a domed cap.
fn silo(g: &mut Geometry, c: &Ctx, p: Vec2, r: f32, h: f32, color: [f32; 3]) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < r + 0.3 {
        return;
    }
    let f = &c.frame;
    tank_vertical(g, f, p, r, 0.0, h, Metal, color);
    let w = f.to_world(p.x, p.y);
    g.part(Metal).cylinder(w, r * 1.02, r * 0.25, h, h + r * 0.7, 12, rgba(mul(color, 0.9)));
}

/// A feed silo on legs with a cone below: the chicken-farm kind.
fn feed_silo(g: &mut Geometry, c: &Ctx, p: Vec2, color: [f32; 3]) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.6 {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    for (du, dv) in [(-0.9, -0.9), (0.9, -0.9), (0.9, 0.9), (-0.9, 0.9)] {
        f.bx(g, Metal, p.x + du, 1.2, p.y + dv, Vec3::new(0.12, 2.4, 0.12), mul(color, 0.7));
    }
    g.part(Metal).cylinder(w, 0.15, 1.25, 0.7, 2.2, 10, rgba(color));
    g.part(Metal).cylinder(w, 1.25, 1.25, 2.2, 7.0, 10, rgba(color));
    g.part(Metal).cylinder(w, 1.25, 0.2, 7.0, 7.8, 10, rgba(mul(color, 0.9)));
}

/// A hay bale (round, stood on end).
fn bale(g: &mut Geometry, f: &Frame, p: Vec2, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    g.part(Tarp).cylinder(w, 0.75, 0.75, 0.0, 1.2, 8, rgba(color));
}

/// A dumpster with a lid, or an open roll-off skip.
fn dumpster(g: &mut Geometry, f: &Frame, p: Vec2, yaw: f32, color: [f32; 3], skip: bool) {
    if skip {
        let w = f.to_world(p.x, p.y);
        g.part(Metal).boxed_open(Vec3::new(w.x, 0.8, w.y), Vec3::new(5.5, 1.6, 2.3), f.yaw() + yaw, rgba(color));
        f.bx_yaw(g, Dirt, p.x, 1.3, p.y, Vec3::new(5.0, 0.4, 1.9), [0.25, 0.22, 0.18], yaw);
    } else {
        f.bx_yaw(g, Metal, p.x, 0.65, p.y, Vec3::new(1.9, 1.2, 1.1), color, yaw);
        f.bx_yaw(g, Matte, p.x, 1.3, p.y, Vec3::new(1.95, 0.08, 1.15), mul(color, 0.6), yaw);
    }
}

/// A big dump truck for the mine: wheels, chassis, tipper and cab.
fn haul_truck(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, yaw: f32, paint: [f32; 3]) {
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    for (a, s) in [(2.0f32, 1.3f32), (2.0, -1.3), (-1.8, 1.3), (-1.8, -1.3)] {
        let q = p + d * a + n * s;
        f.bx_yaw(g, Matte, q.x, y + 0.85, q.y, Vec3::new(1.7, 1.7, 0.7), DARK, yaw);
    }
    f.bx_yaw(g, Matte, p.x, y + 1.6, p.y, Vec3::new(6.0, 0.6, 2.0), CHARCOAL, yaw);
    let bed = p - d * 0.6;
    f.bx_yaw(g, Metal, bed.x, y + 2.6, bed.y, Vec3::new(4.6, 1.6, 3.2), paint, yaw);
    let cab = p + d * 2.4 + n * 0.7;
    f.bx_yaw(g, Metal, cab.x, y + 2.7, cab.y, Vec3::new(1.2, 1.6, 1.4), mul(paint, 0.9), yaw);
    f.bx_yaw(g, Glass, cab.x + d.x * 0.62, y + 3.0, cab.y + d.y * 0.62, Vec3::new(0.04, 0.7, 1.1), c_glass(), yaw);
}

fn c_glass() -> [f32; 3] {
    [0.1, 0.13, 0.16]
}

/// A cone heap.
fn heap(g: &mut Geometry, c: &Ctx, p: Vec2, r: f32, h: f32, color: [f32; 3]) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < r * 0.8 {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Dirt).cylinder(w, r, r * 0.15, 0.0, h, 9, rgba(color));
}

// ── cattle farm ──────────────────────────────────────────────────────────────

/// A farmyard on the street with a red barn and silos, a fenced pasture
/// behind with cows grazing, hay bales and a trough. Bigger herd, a
/// farmhouse and a second silo as the tier rises.
pub fn cow_farm(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let grass = mix(pal.foliage, pal.foliage_dry, [0.55, 0.3, 0.15][t]);
    yard(g, c, 0.4, Foliage, grass);
    let area = R::of(&c.poly);
    let yard_d = (area.d() * 0.35).clamp(7.0, 16.0).min(area.d() * 0.6);
    let farmyard = super::kit::clip_box(&offset_poly(&c.lot_poly, 0.5), area.u0 - 3.0, area.u1 + 3.0, area.v0 - 3.0, area.v0 + yard_d);
    if !farmyard.is_empty() {
        ground(g, f, Dirt, &farmyard, 0.05, mix(pal.dirt, pal.foliage_dry, 0.3));
    }
    let fence_col = mul(pal.wood, [0.6, 0.8, 0.95][t]);
    // Barn.
    let bw = (area.w() * 0.45).clamp(6.0, 18.0);
    let want = R::new(area.u0 + 0.5, area.u0 + 0.5 + bw, area.v0 + 1.5, area.v0 + yard_d - 0.5);
    let barn_red = mix([0.5, 0.12, 0.08], pick(rng, &pal.metal_paint), 0.2);
    if let Some(r) = fit(c, want, 3.5) {
        let h = [4.0, 5.0, 6.0][t];
        block(g, c, if t == 0 { Wood } else { Panel }, &r, 0.0, h, barn_red);
        ridge_u(g, c, &r, h, (r.d() * 0.3).clamp(1.2, 3.5), [0.2, 0.2, 0.22]);
        let fr = r.front();
        fr.decal(g, f, Wood, 0.0, h * 0.4, 0.05, (fr.len * 0.4).min(4.0), h * 0.75, mul(barn_red, 0.7));
        // The white X on the barn doors.
        fr.decal(g, f, Matte, 0.0, h * 0.4, 0.07, (fr.len * 0.4).min(4.0), 0.12, [0.9, 0.9, 0.86]);
        fr.decal(g, f, Matte, 0.0, h * 0.4, 0.07, 0.12, h * 0.75, [0.9, 0.9, 0.86]);
        fr.bx(g, f, Glow, 0.0, h * 0.85, 0.2, Vec3::new(0.4, 0.2, 0.3), pal.street_lamp);
        // Silos by the barn.
        for k in 0..[1, 1, 2][t] {
            let p = Vec2::new(r.u1 + 2.2 + k as f32 * 3.8, r.c().y + 0.5);
            silo(g, c, p, 1.6, [8.0, 10.0, 13.0][t], mul(pal.steel, 1.1));
        }
        // Bales stacked against it.
        for k in 0..[3, 4, 6][t] {
            let p = Vec2::new(r.u0 + 1.0 + k as f32 * 1.6, r.v0 - 1.0);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.9 {
                bale(g, f, p, mix([0.75, 0.65, 0.35], pal.foliage_dry, 0.3));
            }
        }
    }
    if t > 0 {
        // A farmhouse at the other end of the yard.
        let hw = (area.w() * 0.18).clamp(4.0, 7.0);
        if let Some(r) = fit(c, R::new(area.u1 - 0.5 - hw * 2.0, area.u1 - 0.5, area.v0 + 1.5, area.v0 + (yard_d - 1.0).min(8.0)), 3.5) {
            let wall = pick(rng, &pal.plaster);
            block(g, c, Plaster, &r, 0.0, 3.0 * t as f32, wall);
            ridge_u(g, c, &r, 3.0 * t as f32, 1.8, [0.3, 0.18, 0.12]);
            pane_row(g, c, rng, &r.front(), 0.0, t as i32, 3.0, 2.4, 1.0, 1.2, true);
        }
    }
    // The pasture: fenced, cows on it, a trough.
    let pasture = super::kit::clip_box(&offset_poly(&c.lot_poly, 1.2), area.u0 - 3.0, area.u1 + 3.0, area.v0 + yard_d, area.v1 + 3.0);
    if !pasture.is_empty() {
        fence_round(g, c, &pasture, 1.3, fence_col);
        let herd = [5, 8, 12][t];
        let mut placed = 0;
        for _ in 0..herd * 3 {
            if placed >= herd {
                break;
            }
            if let Some(p) = random_in_poly(rng, &pasture, 1.6) {
                let (coat, patch) = coat(rng);
                cow(g, f, p, rng.random_range(0.0..TAU), coat, patch);
                placed += 1;
            }
        }
        let pc = poly_center(&pasture);
        if super::kit::poly_inside_poly(&rect(pc.x, pc.y, 1.5, 0.4), &pasture) {
            f.bx(g, Metal, pc.x, 0.4, pc.y, Vec3::new(3.0, 0.8, 0.8), mul(pal.steel, 0.8));
            f.bx(g, Matte, pc.x, 0.75, pc.y, Vec3::new(2.8, 0.04, 0.6), mul(pal.water, 1.3));
        }
    }
    let front = c.front_face(&c.poly);
    let tr = front.at(front.len * 0.15, -3.0);
    if vehicle_fits(c, tr, 0.0, Vehicle::Van) {
        vehicle(g, f, pal, rng, tr, 0.0, 0.0, Vehicle::Van, [0.2, 0.45, 0.15]);
    }
}

// ── chicken farm ─────────────────────────────────────────────────────────────

/// Long low chicken sheds side by side, ridge vents along their roofs and
/// fans in their gable ends, a pair of feed silos at the head of each, an
/// egg-packing shed on the street and a manure heap.
pub fn chicken_farm(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, Dirt, mix(pal.dirt, pal.gravel, 0.5));
    let area = R::of(&c.poly);
    let sw = [7.0, 9.0, 11.0][t];
    let gap = 5.0;
    let head = 6.0;
    let n = (((area.w() + gap) / (sw + gap)).floor() as i32).clamp(1, 5);
    let wall = mix([0.85, 0.85, 0.82], pick(rng, &pal.panel), 0.3);
    let silo_col = mul(pal.steel, 1.15);
    let used = n as f32 * sw + (n - 1) as f32 * gap;
    let u_start = area.c().x - used * 0.5;
    for k in 0..n {
        let u0 = u_start + k as f32 * (sw + gap);
        let want = R::new(u0, u0 + sw, area.v0 + head, area.v1 - 1.0);
        let Some(r) = fit(c, want, 3.0) else { continue };
        if r.d() < 6.0 {
            continue;
        }
        let h = 2.6;
        block(g, c, if t == 0 { WornPanel } else { Panel }, &r, 0.0, h, wall);
        ridge_v(g, c, &r, h, (r.w() * 0.15).clamp(0.8, 1.6), mul(pal.steel, [0.55, 0.7, 0.8][t]));
        // Ridge vents.
        let vents = (r.d() / 6.0) as i32;
        for i in 0..vents {
            let v = r.v0 + (i as f32 + 0.5) * r.d() / vents.max(1) as f32;
            f.bx(g, Metal, r.c().x, h + (r.w() * 0.15).clamp(0.8, 1.6) + 0.3, v, Vec3::new(0.8, 0.6, 0.8), mul(pal.steel, 0.7));
        }
        // Curtain vent strips down the long sides.
        for face in [r.left(), r.right()] {
            face.decal(g, f, Tarp, 0.0, 1.5, 0.04, face.len * 0.92, 0.7, [0.85, 0.75, 0.25]);
        }
        // Fans in the back gable end; a door and a lamp in the front one.
        let back = r.back();
        for i in 0..(r.w() / 2.0).floor().max(1.0) as i32 {
            back.decal(g, f, Matte, -r.w() * 0.5 + 1.0 + i as f32 * 2.0, 1.3, 0.05, 1.4, 1.4, DARK);
        }
        let fr = r.front();
        fr.decal(g, f, Matte, 0.0, 1.05, 0.05, 1.2, 2.1, mul(pal.steel, 0.5));
        fr.bx(g, f, Glow, 0.0, 2.3, 0.2, Vec3::new(0.3, 0.15, 0.25), pal.street_lamp);
        // Feed silos at the head with an auger into the shed.
        for s in [-1.0f32, 1.0] {
            let p = Vec2::new(r.c().x + s * (r.w() * 0.5 - 1.3), r.v0 - 2.2);
            feed_silo(g, c, p, silo_col);
        }
        let a = f.at(r.c().x + r.w() * 0.5 - 1.3, 6.5, r.v0 - 2.2);
        let b = f.at(r.c().x + r.w() * 0.25, h + 0.4, r.v0 + 1.0);
        g.part(Metal).beam(a, b, 0.2, rgba(mul(pal.steel, 0.8)));
    }
    // Egg-packing shed and van on the street side.
    let front = c.front_face(&c.poly);
    let p = front.at(-front.len * 0.3, -2.4);
    if poly_inside(c, &rect(p.x, p.y, 3.0, 1.6)) && t > 0 {
        let r = R::new(p.x - 3.0, p.x + 3.0, p.y - 1.6, p.y + 1.6);
        block(g, c, Panel, &r, 0.0, 3.0, [0.88, 0.88, 0.85]);
        pane(g, c, rng, &r.front(), -1.5, 1.6, 1.2, 1.0, false);
        sign_board(g, c, rng, &r.front(), 1.0, 2.6, 2.6, 0.4);
    }
    let van = front.at(front.len * 0.25, -2.8);
    if vehicle_fits(c, van, 0.0, Vehicle::Van) {
        vehicle(g, f, pal, rng, van, 0.0, 0.0, Vehicle::Van, [0.88, 0.88, 0.86]);
    }
    if let Some(p) = c.random_inside(rng, 3.0) {
        heap(g, c, Vec2::new(p.x, area.v1 - 3.0), 2.5, 1.6, mul(pal.dirt, 0.7));
    }
}

// ── slaughterhouse ───────────────────────────────────────────────────────────

/// A windowless block with loading docks and refrigerated lorries backed up
/// to them, holding pens with a roof and a few head of cattle waiting, a
/// vent stack and chillers on the roof.
pub fn slaughterhouse(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.3));
    fence_lot(g, c, &lot, 2.4, t == 0);
    let area = R::of(&c.poly);
    let h = [6.0, 8.0, 10.0][t];
    let wall = mix([0.78, 0.78, 0.76], pick(rng, &pal.panel), 0.4);
    let bw = area.w() * 0.62;
    let want = R::new(area.u0, area.u0 + bw, area.v0 + area.d() * 0.3, area.v1);
    if let Some(r) = fit(c, want, 6.0) {
        block(g, c, if t == 0 { WornPanel } else { Panel }, &r, 0.0, h, wall);
        parapet(g, c, &r.poly(), h, 0.6, 0.2, Panel, mul(wall, 0.85));
        for face in r.faces() {
            if face.len < 4.0 {
                continue;
            }
            // Panel joints, no windows.
            let ribs = (face.len / 3.0) as i32;
            for k in 1..ribs {
                face.decal(g, f, Matte, -face.len * 0.5 + k as f32 * face.len / ribs as f32, h * 0.5, 0.03, 0.08, h * 0.98, mul(wall, 0.8));
            }
            grime(g, c, rng, &face, 0.0, h, [1.0, 0.5, 0.2][t]);
        }
        // Loading docks on the street face, lorries backed up to them.
        let fr = r.front();
        let docks = ((fr.len - 2.0) / 4.5).floor().clamp(1.0, 4.0) as i32;
        for k in 0..docks {
            let tt = -fr.len * 0.5 + 1.0 + (k as f32 + 0.5) * (fr.len - 2.0) / docks as f32;
            fr.bx(g, f, Concrete, tt, 0.6, 0.6, Vec3::new(3.6, 1.2, 1.2), mul(pal.concrete(0), 0.9));
            fr.decal(g, f, Matte, tt, 2.2, 0.05, 3.0, 3.0, DARK);
            fr.bx(g, f, Matte, tt, 3.9, 0.3, Vec3::new(3.4, 0.4, 0.6), CHARCOAL);
            fr.bx(g, f, Glow, tt + 1.5, 3.4, 0.1, Vec3::new(0.2, 0.2, 0.1), if k % 2 == 0 { pal.signal_green } else { pal.signal_red });
            let p = fr.at(tt, 6.4);
            if k % 2 == 0 && vehicle_fits(c, p, FRAC_PI_2, Vehicle::Lorry) {
                vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, Vehicle::Lorry, [0.9, 0.92, 0.94]);
            }
        }
        // Roof: chillers and a vent stack.
        for k in 0..[1, 2, 3][t] {
            let p = Vec2::new(r.u0 + (k as f32 + 0.5) * r.w() / 3.0, r.c().y + r.d() * 0.2);
            chiller(g, c, p, h, 2.6, 1.6, 0.0, mul(pal.steel, 0.9));
        }
        chimney(g, f, Vec2::new(r.u1 - 1.5, r.v1 - 1.5), 0.45, h, 7.0, mul(pal.steel, 0.65), pal.aviation_light);
        sign_board(g, c, rng, &fr, fr.len * 0.25, h - 1.0, (fr.len * 0.25).min(5.0), 0.5);
    }
    // Holding pens beside it.
    let pens = R::new(area.u0 + bw + 1.5, area.u1, area.v0 + area.d() * 0.35, area.v1);
    if let Some(p) = fit(c, pens, 4.0) {
        let rail = mul(pal.steel, 0.8);
        ground(g, f, Dirt, &p.poly(), 0.06, mul(pal.dirt, 0.8));
        for face in p.faces() {
            rail_fence(g, c, face.a, face.b, 1.4, rail);
        }
        let n = ((p.d() / 5.0) as i32).max(1);
        for k in 1..n {
            let v = p.v0 + k as f32 * p.d() / n as f32;
            rail_fence(g, c, Vec2::new(p.u0, v), Vec2::new(p.u1, v), 1.4, rail);
        }
        // A roof over half of it on posts.
        let roof = R::new(p.u0, p.u1, p.c().y, p.v1);
        f.bx(g, Metal, roof.c().x, 3.6, roof.c().y, Vec3::new(roof.w() + 0.4, 0.2, roof.d() + 0.4), mul(pal.steel, 0.6));
        for (u, v) in [(roof.u0, roof.v0), (roof.u1, roof.v0), (roof.u0, roof.v1), (roof.u1, roof.v1)] {
            f.bx(g, Metal, u, 1.8, v, Vec3::new(0.15, 3.6, 0.15), mul(pal.steel, 0.5));
        }
        for _ in 0..[4, 6, 8][t] {
            if let Some(q) = random_in_poly(rng, &p.inset(0.5).poly(), 1.0) {
                let (coat, patch) = coat(rng);
                cow(g, f, q, rng.random_range(0.0..TAU), coat, patch);
            }
        }
    }
    let front = c.front_face(&lot);
    floodlight(g, c, front.at(-front.len * 0.45, -1.2), 9.0);
    floodlight(g, c, front.at(front.len * 0.45, -1.2), 9.0);
}

// ── strip mine ───────────────────────────────────────────────────────────────

/// A terraced open pit filling the lot: benches stepping down from a high
/// spoil rim to a floor with standing water, haul trucks on the benches, a
/// conveyor up out of it to an ore heap, spoil heaps and a site cabin.
pub fn strip_mine(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Dirt, mix(pal.dirt, pal.stone, 0.3));
    let area = R::of(&c.poly);
    let fs = (area.d() * 0.22).clamp(4.0, 14.0);
    let pit_poly = super::kit::clip_box(&offset_poly(&c.lot_poly, 1.0), area.u0 - 3.0, area.u1 + 3.0, area.v0 + fs, area.v1 + 3.0);
    let pit_poly = if pit_poly.is_empty() { offset_poly(&c.lot_poly, 1.0) } else { pit_poly };
    let centre = interior_anchor(&pit_poly);
    let (lo, hi) = bounds(&pit_poly);
    let (rx, ry) = ((hi.x - lo.x) * 0.5, (hi.y - lo.y) * 0.5);
    let sides = 14;
    let outer: Vec<Vec2> = (0..sides)
        .map(|i| {
            let a = i as f32 / sides as f32 * TAU;
            let p = centre + Vec2::new(a.cos() * rx, a.sin() * ry);
            clamp_into(&pit_poly, p)
        })
        .collect();
    let levels = [3, 4, 5][t];
    let rim = [4.0, 6.0, 8.0][t];
    let ring = |k: f32| -> Vec<Vec2> { outer.iter().map(|p| centre + (*p - centre) * k).collect() };
    let world = |p: Vec2, y: f32| f.at2(p, y);
    let rock = mix(pal.stone, pal.dirt, 0.4);
    // Outer slope of the spoil rim.
    let top0 = 0.92;
    let ground_ring = ring(1.0);
    let rim_ring = ring(top0);
    for i in 0..sides {
        let j = (i + 1) % sides;
        let mid = (ground_ring[i] + ground_ring[j]) * 0.5;
        let out = f.dir3(mid - centre);
        g.part(Dirt).quad_facing([world(ground_ring[i], 0.0), world(ground_ring[j], 0.0), world(rim_ring[j], rim), world(rim_ring[i], rim)], out, rgba(mul(rock, 0.85)));
    }
    // Benches and risers down to the floor.
    let descent = 0.62;
    let step = descent / levels as f32;
    let mut a = top0;
    let mut bench_mid: Vec<(f32, f32)> = Vec::new();
    for k in 0..levels {
        let y = rim * (1.0 - k as f32 / levels as f32);
        let y_next = rim * (1.0 - (k + 1) as f32 / levels as f32);
        let b = a - step * 0.7;
        let a_next = b - step * 0.3;
        let (ra, rb, rn) = (ring(a), ring(b), ring(a_next));
        let strata = if k % 2 == 0 { mul(rock, 1.05) } else { mix(rock, [0.55, 0.42, 0.3], 0.35) };
        for i in 0..sides {
            let j = (i + 1) % sides;
            g.part(Dirt).quad_facing([world(ra[i], y), world(ra[j], y), world(rb[j], y), world(rb[i], y)], Vec3::Y, rgba(mul(strata, 1.1)));
            let mid = (rb[i] + rb[j]) * 0.5;
            let inward = f.dir3(centre - mid);
            g.part(Stone).quad_facing([world(rb[i], y), world(rb[j], y), world(rn[j], y_next), world(rn[i], y_next)], inward + Vec3::Y * 0.3, rgba(mul(strata, 0.8)));
        }
        bench_mid.push(((a + b) * 0.5, y));
        a = a_next;
    }
    // The floor, with a pool of water in it.
    let floor = ring(a);
    ground(g, f, Dirt, &floor, 0.05, mul(rock, 0.7));
    ground(g, f, Glass, &scale_poly(&floor, 0.45), 0.08, mul(pal.water, 1.4));
    // Haul trucks on the benches and the floor.
    let paint = [0.85, 0.62, 0.08];
    for k in 0..[1, 2, 3][t].min(bench_mid.len()) {
        let (s, y) = bench_mid[(k * 2 + 1).min(bench_mid.len() - 1)];
        let i = rng.random_range(0..sides);
        let p = centre + (outer[i] - centre) * s;
        let tangent = outer[(i + 1) % sides] - outer[i];
        if distance_to_boundary(p, &c.lot_poly) < 4.0 || !c.inside_local(p) {
            continue;
        }
        haul_truck(g, f, p, y, tangent.y.atan2(tangent.x), paint);
    }
    let fp = centre + (outer[sides / 3] - centre) * (a * 0.6);
    let yaw = rng.random_range(0.0..TAU);
    if c.inside_local(fp) && distance_to_boundary(fp, &c.lot_poly) > 4.0 {
        haul_truck(g, f, fp, 0.0, yaw, mul(paint, 0.9));
    }
    // The conveyor: from the floor over the rim to the ore heap out front.
    let rim_pt = centre + (outer[sides * 3 / 4] - centre) * top0;
    let heap_pt = Vec2::new(rim_pt.x.clamp(area.u0 + 3.0, area.u1 - 3.0), area.v0 + fs * 0.5);
    let heap_ok = c.inside_local(heap_pt) && distance_to_boundary(heap_pt, &c.lot_poly) > 2.0 && distance_to_boundary(rim_pt, &c.lot_poly) > 1.0;
    let pts = [Vec3::new(centre.x, 1.0, centre.y), Vec3::new(rim_pt.x, rim + 3.0, rim_pt.y), Vec3::new(heap_pt.x, 6.0, heap_pt.y)];
    for w in pts.windows(2).take(if heap_ok { 2 } else { 0 }) {
        let (a3, b3) = (f.at(w[0].x, w[0].y, w[0].z), f.at(w[1].x, w[1].y, w[1].z));
        g.part(Metal).slab(a3, b3, 1.2, 0.5, rgba(mul(pal.steel, 0.8)));
        g.part(Matte).slab(a3 + Vec3::Y * 0.3, b3 + Vec3::Y * 0.3, 0.9, 0.15, rgba([0.2, 0.18, 0.15]));
        let len = Vec2::new(w[0].x, w[0].z).distance(Vec2::new(w[1].x, w[1].z));
        let legs = (len / 6.0) as i32;
        for l in 1..=legs {
            let s = l as f32 / (legs + 1) as f32;
            let q = w[0].lerp(w[1], s);
            f.bx(g, Metal, q.x, q.y * 0.5, q.z, Vec3::new(0.25, q.y, 0.25), mul(pal.steel, 0.6));
        }
    }
    heap(g, c, heap_pt, (fs * 0.45).min(5.0), 5.0, mix([0.4, 0.3, 0.25], pal.stone, 0.3));
    // Spoil heaps and the site cabin on the front strip.
    for k in 0..[1, 2, 2][t] {
        let p = Vec2::new(area.u0 + 4.0 + k as f32 * 9.0, area.v0 + fs * 0.5);
        if p.distance(heap_pt) > 8.0 {
            heap(g, c, p, (fs * 0.4).min(4.5), 3.5, mul(rock, 0.9));
        }
    }
    let cab = Vec2::new(area.u1 - 4.0, area.v0 + 1.6);
    if poly_inside(c, &rect(cab.x, cab.y, 3.1, 1.3)) && cab.distance(heap_pt) > 6.0 {
        container_unit(g, c, rng, cab.x, cab.y, 0.0, 6.05, 2.44, 2.6, 0.0, [0.85, 0.65, 0.1]);
    }
    let front = c.front_face(&c.poly);
    floodlight(g, c, front.at(-front.len * 0.45, -0.8), 12.0);
    floodlight(g, c, front.at(front.len * 0.45, -0.8), 12.0);
}

// ── trash company ────────────────────────────────────────────────────────────

/// A private trash company: a fenced yard with its garbage trucks parked
/// in a row in the company's colour, dumpsters and skips stacked, a
/// maintenance shed and a cabin office at the gate.
pub fn trash_company(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let lot = yard(g, c, 0.4, if t == 0 { WornAsphalt } else { Asphalt }, mul(pal.asphalt, 1.25));
    fence_lot(g, c, &lot, 2.2, t == 0);
    let area = R::of(&c.poly);
    let livery = [[0.15, 0.45, 0.2], [0.9, 0.45, 0.08], [0.1, 0.3, 0.65], [0.88, 0.88, 0.85]][rng_for(c.seed, 0x7A54).random_range(0..4usize)];
    // Shed along the back.
    let sd = (area.d() * 0.3).clamp(5.0, 12.0);
    let want = R::new(area.u0, area.u0 + area.w() * 0.55, area.v1 - sd, area.v1);
    let truck_v1 = if let Some(r) = fit(c, want, 4.0) {
        let h = [5.0, 6.0, 7.0][t];
        let wall = mul(pick(rng, &pal.metal_paint), 0.8);
        super::industrial::shed(g, c, rng, &r.poly(), h, wall, if t == 0 { WornMetal } else { Metal });
        r.front().decal(g, f, Matte, 0.0, h - 0.8, 0.05, r.w() * 0.9, 0.5, livery);
        r.v0
    } else {
        area.v1
    };
    // Garbage trucks nose to the street in a row.
    let n = [3, 5, 7][t];
    let yaw = -FRAC_PI_2;
    let v = (truck_v1 - 5.6).min(area.v0 + area.d() * 0.55).max(area.v0 + 5.5);
    let mut u = area.u0 + 2.0;
    let mut parked = 0;
    while u < area.u1 - 1.5 && parked < n {
        let p = Vec2::new(u, v);
        if vehicle_fits(c, p, yaw, Vehicle::GarbageTruck) {
            vehicle(g, f, pal, rng, p, 0.0, yaw, Vehicle::GarbageTruck, livery);
            parked += 1;
        }
        u += 3.4;
    }
    // Dumpsters and skips in the far corner.
    let corner = Vec2::new(area.u1 - 3.5, area.v1 - 2.0);
    for k in 0..[4, 6, 8][t] {
        let p = corner + Vec2::new(-(k % 3) as f32 * 2.2, -(k / 3) as f32 * 1.6);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.2 {
            dumpster(g, f, p, 0.0, if k % 2 == 0 { livery } else { mul(livery, 0.7) }, false);
        }
    }
    let skip = Vec2::new(area.u1 - 4.0, area.v0 + area.d() * 0.3);
    if poly_inside(c, &rect(skip.x, skip.y, 3.0, 1.4)) {
        dumpster(g, f, skip, 0.0, mul(pal.rust, 1.2), true);
    }
    // Cabin office at the gate, a sign and lights.
    let front = c.front_face(&lot);
    let office = Vec2::new(area.u1 - 3.5, area.v0 + 1.6);
    if poly_inside(c, &rect(office.x, office.y, 3.1, 1.3)) {
        container_unit(g, c, rng, office.x, office.y, 0.0, 6.05, 2.44, 2.6, 0.0, livery);
        let of = Face::new(office + Vec2::new(-3.0, -1.23), office + Vec2::new(3.0, -1.23), Vec2::new(0.0, -1.0));
        pane(g, c, rng, &of, -1.2, 1.5, 1.2, 0.8, false);
        sign_board(g, c, rng, &of, 0.8, 3.1, 3.5, 0.5);
    }
    floodlight(g, c, front.at(-front.len * 0.45, -1.2), 9.0);
    floodlight(g, c, Vec2::new(area.u0 + 1.0, area.v1 - 1.0), 9.0);
    if t == 2 {
        // Wash bay: a canopy and a hose gantry.
        let p = Vec2::new(area.u0 + area.w() * 0.75, area.v1 - sd * 0.5);
        if poly_inside(c, &rect(p.x, p.y, 3.0, 5.0)) {
            f.bx(g, Metal, p.x, 5.0, p.y, Vec3::new(6.0, 0.3, 10.0), mul(pal.steel, 0.8));
            for (du, dv) in [(-2.8, -4.8), (2.8, -4.8), (-2.8, 4.8), (2.8, 4.8)] {
                f.bx(g, Metal, p.x + du, 2.5, p.y + dv, Vec3::new(0.25, 5.0, 0.25), mul(pal.steel, 0.6));
            }
            f.bx(g, Glow, p.x, 4.8, p.y, Vec3::new(0.3, 0.06, 6.0), pal.lamp);
        }
    }
}

// ── fish gutting ─────────────────────────────────────────────────────────────

/// Gutting and filleting sheds: a long shed, an open-sided gutting canopy
/// over steel tables and blue tubs, an ice plant with its chute, crates of
/// fish and a refrigerated van, the paving wet and dark.
pub fn fish_gutting(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.4, if t == 0 { WornPaving } else { Paving }, mul(pal.paving, 0.7));
    let area = R::of(&c.poly);
    let wall = mix([0.7, 0.78, 0.82], pick(rng, &pal.panel), 0.3);
    // The long shed across the back.
    let sd = (area.d() * 0.4).clamp(5.0, 14.0);
    if let Some(r) = fit(c, R::new(area.u0, area.u1 - area.w() * 0.25, area.v1 - sd, area.v1), 4.0) {
        let h = [4.0, 5.0, 6.0][t];
        super::industrial::shed(g, c, rng, &r.poly(), h, wall, if t == 0 { WornMetal } else { Metal });
        let fr = r.front();
        pane_row(g, c, rng, &fr, 0.0, 1, h, 4.0, 2.0, 0.9, false);
        sign_board(g, c, rng, &fr, fr.len * 0.3, h - 0.7, (fr.len * 0.25).min(5.0), 0.5);
        super::trades::fish_sign(g, c, &fr, -fr.len * 0.3, h - 0.9, 0.3, 1.2, [0.3, 0.9, 1.3]);
    }
    // The gutting canopy: roof on posts over tables and tubs.
    let cw = (area.w() * 0.5).clamp(5.0, 14.0);
    if let Some(r) = fit(c, R::new(area.u0 + 1.0, area.u0 + 1.0 + cw, area.v0 + 2.0, area.v0 + 2.0 + (area.d() - sd - 3.0).clamp(4.0, 8.0)), 3.5) {
        f.bx(g, Metal, r.c().x, 3.5, r.c().y, Vec3::new(r.w() + 0.4, 0.2, r.d() + 0.4), mul(pal.steel, 0.75));
        for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u0, r.v1), (r.u1, r.v1)] {
            f.bx(g, Metal, u, 1.75, v, Vec3::new(0.15, 3.5, 0.15), mul(pal.steel, 0.5));
        }
        let tables = ((r.w() - 1.0) / 2.6).floor().max(1.0) as i32;
        for k in 0..tables {
            let u = r.u0 + 0.5 + (k as f32 + 0.5) * (r.w() - 1.0) / tables as f32;
            f.bx(g, Metal, u, 0.9, r.c().y, Vec3::new(2.0, 0.08, 1.0), [0.75, 0.77, 0.8]);
            f.bx(g, Metal, u, 0.45, r.c().y, Vec3::new(1.8, 0.9, 0.08), mul(pal.steel, 0.6));
            let w = f.to_world(u + 0.6, r.c().y + 1.2);
            g.part(Matte).cylinder(w, 0.45, 0.4, 0.0, 0.7, 8, rgba([0.1, 0.35, 0.7]));
            f.bx(g, Glow, u, 3.35, r.c().y, Vec3::new(1.2, 0.06, 0.2), mul(pal.lamp, 1.1));
        }
    }
    // Ice plant on the other side: a white block, an ice chute, chillers.
    let iw = (area.w() * 0.2).clamp(3.0, 6.0);
    if let Some(r) = fit(c, R::new(area.u1 - iw, area.u1, area.v1 - sd - 0.5, area.v1), 3.0) {
        let h = [5.0, 6.0, 7.5][t];
        block(g, c, Panel, &r, 0.0, h, [0.86, 0.88, 0.9]);
        chiller(g, c, r.c(), h, (r.w() * 0.6).min(2.4), 1.4, 0.0, mul(pal.steel, 0.9));
        let a = f.at(r.c().x, h - 1.0, r.v0);
        if c.inside_local(Vec2::new(r.c().x, r.v0 - 3.0)) {
            let b = f.at(r.c().x, 1.6, r.v0 - 2.5);
            g.part(Metal).beam(a, b, 0.6, rgba(mul(pal.steel, 0.9)));
        }
    }
    for _ in 0..[4, 6, 8][t] {
        if let Some(p) = c.random_inside(rng, 1.5) {
            if p.y < area.v1 - sd - 0.5 {
                { let n = rng.random_range(1..4); fish_crates(g, c, rng, p, n); }
            }
        }
    }
    let front = c.front_face(&c.poly);
    let van = front.at(front.len * 0.25, -1.8);
    if vehicle_fits(c, van, 0.0, Vehicle::Van) {
        vehicle(g, f, pal, rng, van, 0.0, 0.0, Vehicle::Van, [0.9, 0.92, 0.95]);
    }
    if t > 0 {
        let lorry = Vec2::new(area.u1 - 6.0, area.v0 + 3.0);
        if vehicle_fits(c, lorry, 0.0, Vehicle::Lorry) {
            vehicle(g, f, pal, rng, lorry, 0.0, 0.0, Vehicle::Lorry, [0.88, 0.9, 0.93]);
        }
    }
    floodlight(g, c, front.at(-front.len * 0.45, -1.0), 7.0);
}

// ── fish-stick factory ───────────────────────────────────────────────────────

/// A clad factory with band windows, a windowless freezer block beside it
/// with chillers on its roof, a chimney, reefer lorries at the dock and a
/// big glowing fish-stick sign up on the roof.
pub fn fish_stick_factory(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    let lot = yard(g, c, 0.4, Asphalt, mul(pal.asphalt, 1.3));
    fence_lot(g, c, &lot, 2.4, false);
    let area = R::of(&c.poly);
    let h = [7.0, 9.0, 11.0][t];
    let wall = mix(pick(rng, &pal.panel), [0.2, 0.35, 0.6], 0.25);
    let split = area.u0 + area.w() * 0.6;
    let top_v = area.v0 + area.d() * 0.28;
    let mut sign_at: Option<(R, f32)> = None;
    if let Some(r) = fit(c, R::new(area.u0, split, top_v, area.v1), 5.0) {
        block(g, c, if t == 0 { WornPanel } else { Panel }, &r, 0.0, h, wall);
        parapet(g, c, &r.poly(), h, 0.6, 0.2, Panel, mul(wall, 0.85));
        let fr = r.front();
        band_windows(g, c, rng, &fr, 0.4, (h / 4.0) as i32, 4.0);
        // The glass door 13 mm behind the window lights (0.05) so the two
        // never share a plane where a lit band crosses it.
        fr.decal(g, f, Glass, -fr.len * 0.3, 1.3, 0.037, 2.0, 2.6, pal.glass_dark);
        fr.bx(g, f, Glow, -fr.len * 0.3, 2.9, 0.15, Vec3::new(1.0, 0.12, 0.2), pal.lamp);
        for face in [r.left(), r.back()] {
            band_windows(g, c, rng, &face, 0.4, 1, 4.0);
        }
        chimney(g, f, Vec2::new(r.u0 + 2.0, r.v1 - 2.0), 0.7, h, [12.0, 15.0, 18.0][t], mul(pal.steel, 0.65), pal.aviation_light);
        roof_gear(g, c, rng, &r.poly(), h, 0.3);
        sign_at = Some((r, h));
    }
    // The freezer block: taller, white, ribbed, chillers.
    if let Some(r) = fit(c, R::new(split + 1.0, area.u1, top_v + 3.0, area.v1), 4.0) {
        let fh = h + 2.0;
        block(g, c, Panel, &r, 0.0, fh, [0.9, 0.91, 0.92]);
        for face in r.faces() {
            let ribs = (face.len / 2.0) as i32;
            for k in 1..ribs {
                face.decal(g, f, Matte, -face.len * 0.5 + k as f32 * face.len / ribs as f32, fh * 0.5, 0.03, 0.1, fh * 0.98, [0.78, 0.8, 0.82]);
            }
        }
        for k in 0..2 {
            chiller(g, c, Vec2::new(r.c().x, r.v0 + (k as f32 + 0.5) * r.d() / 2.0), fh, (r.w() * 0.6).min(3.0), 1.6, 0.0, mul(pal.steel, 0.9));
        }
        // Docks with reefer lorries.
        let fr = r.front();
        fr.decal(g, f, Matte, 0.0, 2.0, 0.05, 3.0, 3.0, DARK);
        fr.bx(g, f, Glow, 1.7, 3.6, 0.1, Vec3::new(0.2, 0.2, 0.1), pal.signal_green);
        let p = fr.at(0.0, 6.2);
        if vehicle_fits(c, p, FRAC_PI_2, Vehicle::Lorry) {
            vehicle(g, f, pal, rng, p, 0.0, FRAC_PI_2, Vehicle::Lorry, [0.9, 0.92, 0.95]);
        }
        // A pipe bridge across to the factory.
        if let Some((fr, _)) = sign_at {
            pipe_run(g, f, &[Vec3::new(fr.u1, h - 1.5, r.c().y), Vec3::new(r.u0, h - 1.5, r.c().y)], 0.35, mul(pal.steel, 0.8));
        }
    }
    // The fish-stick sign on the factory roof, facing the street.
    if let Some((r, h)) = sign_at {
        let sw = (r.w() * 0.6).clamp(5.0, 14.0);
        let sh = sw * 0.35;
        let y0 = h + 1.0;
        let v = r.v0 + 1.0;
        for s in [-0.4f32, 0.4] {
            f.bx(g, Metal, r.c().x + s * sw, y0 * 0.5 + h * 0.5, v + 0.3, Vec3::new(0.25, y0 - h + sh * 0.5, 0.25), mul(pal.steel, 0.6));
        }
        let face = Face::new(Vec2::new(r.c().x - sw * 0.5, v), Vec2::new(r.c().x + sw * 0.5, v), Vec2::new(0.0, -1.0));
        face.bx(g, f, Matte, 0.0, y0 + sh * 0.5, -0.1, Vec3::new(sw, sh, 0.2), [0.1, 0.18, 0.4]);
        let k = 1.0 - 0.5 * c.wear;
        // Three golden fish sticks, stacked crooked.
        for i in 0..3 {
            let tt = -sw * 0.18 + i as f32 * 0.04 * sw;
            let y = y0 + sh * (0.25 + 0.25 * i as f32);
            face.decal(g, f, Glow, tt, y, 0.02, sw * 0.42, sh * 0.17, mul([1.6, 0.95, 0.25], k));
            face.decal(g, f, Glow, tt - sw * 0.2, y, 0.03, sw * 0.03, sh * 0.13, mul([1.2, 0.6, 0.15], k));
        }
        // A fish beside them.
        super::trades::fish_sign(g, c, &face, sw * 0.3, y0 + sh * 0.5, 0.03, sh * 0.6, [0.3, 0.9, 1.4]);
    }
    let front = c.front_face(&lot);
    for p in [front.at(-front.len * 0.45, -1.2), front.at(front.len * 0.45, -1.2)] {
        floodlight(g, c, p, 9.0);
    }
}
