//! [r4-harbour] Harbour Pastel, The Promenade's look: a working harbour
//! front painted over for visitors. Pastel waterfront terraces with gables
//! to the street, converted boathouses, blocks with a lighthouse tower on
//! the corner; boardwalk cafés, seafood shacks, a striped-awning arcade
//! and boutique hotels with flags; for its small industry the fish market
//! hall and tall tarred net sheds; and for the families and billionaires
//! who buy up the front, a pastel villa with its own boathouse.
//!
//! Stylised and gritty, not seaside-postcard: the pastels are faded and
//! salt-streaked (greyer and darker with wear), white trim yellows, the
//! sheds are tar-black, there are nets, crates, buoys and pots in every
//! yard. After dark the front is strung with bulbs (base glow marked
//! `NIGHT_MARK`, lit only after dark, so a lit front reads from across
//! the bay), the windows are warm and the lighthouse tops glow.
//!
//! Where a lot touches the water (`Ctx::shore`) the boathouses, sheds,
//! cafés, the market and the villa run a small timber pier out over it on
//! piles, unless the business builds a jetty of its own there
//! (`World::jetty_kind_for`). Everything comes from the building's seed:
//! the same from every camera. Frames, shutters, sills, railings, chairs,
//! pots and the bulbs' daytime glass go in the detail layers.

use super::kit::*;
use super::parts::*;
use super::shore::sea_local;
use super::{Ctx, Extras, Face};
use crate::depth::{FRAME, PANE};
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Salt-faded pastels: mint, powder blue, butter, blush, lilac, seafoam,
/// peach, cream.
const PASTELS: [[f32; 3]; 8] = [[0.60, 0.74, 0.64], [0.58, 0.68, 0.78], [0.86, 0.78, 0.50], [0.84, 0.60, 0.58], [0.68, 0.62, 0.76], [0.54, 0.74, 0.72], [0.88, 0.66, 0.50], [0.86, 0.82, 0.70]];
/// What salt and sun turn them towards.
const SALT: [f32; 3] = [0.66, 0.66, 0.63];
/// Painted trim: white going cream.
const TRIM: [f32; 3] = [0.86, 0.85, 0.80];
/// Shutters and doors: navy, teal, harbour red, sage, ochre.
const SHUTTERS: [[f32; 3]; 5] = [[0.14, 0.20, 0.34], [0.10, 0.36, 0.38], [0.52, 0.14, 0.12], [0.36, 0.44, 0.34], [0.66, 0.48, 0.18]];
/// Roofs: blue slate, weathered red tin, tar, zinc.
const ROOFS: [[f32; 3]; 4] = [[0.24, 0.28, 0.32], [0.46, 0.22, 0.17], [0.13, 0.13, 0.14], [0.50, 0.53, 0.54]];
/// Tarred boards of the net sheds.
const TAR: [f32; 3] = [0.09, 0.09, 0.10];
/// Awning stripes: harbour red, navy, teal, yellow, coral (on white).
const STRIPES: [[f32; 3]; 5] = [[0.66, 0.14, 0.12], [0.14, 0.20, 0.40], [0.10, 0.44, 0.46], [0.88, 0.70, 0.18], [0.90, 0.46, 0.38]];
/// Flags and buoys: bright.
const BRIGHT: [[f32; 3]; 6] = [[0.85, 0.16, 0.14], [0.12, 0.28, 0.66], [0.95, 0.80, 0.16], [0.92, 0.92, 0.88], [0.12, 0.56, 0.42], [0.94, 0.46, 0.12]];
/// Nets drying: faded green, tan, blue.
const NETS: [[f32; 3]; 3] = [[0.26, 0.38, 0.30], [0.50, 0.40, 0.26], [0.22, 0.32, 0.44]];
/// A string bulb after dark.
const BULB: [f32; 3] = [2.3, 1.6, 0.75];
/// A lighthouse lamp.
const BEAM: [f32; 3] = [2.6, 2.2, 1.3];
/// A café or shack lit inside.
const INSIDE: [f32; 3] = [1.5, 1.05, 0.6];

/// Lot-local (u, y, v).
fn p3(u: f32, y: f32, v: f32) -> Vec3 {
    Vec3::new(u, y, v)
}

/// A pastel, salt-faded and darkened with wear.
fn pastel(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    let base = PASTELS[rng.random_range(0..PASTELS.len())];
    mul(mix(base, SALT, 0.05 + 0.3 * c.wear), 1.0 - 0.22 * c.wear)
}

fn trim(c: &Ctx) -> [f32; 3] {
    mul(mix(TRIM, [0.78, 0.72, 0.56], 0.5 * c.wear), 1.0 - 0.15 * c.wear)
}

fn shutter(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(SHUTTERS[rng.random_range(0..SHUTTERS.len())], 1.0 - 0.3 * c.wear)
}

fn roof_col(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(ROOFS[rng.random_range(0..ROOFS.len())], 1.0 - 0.15 * c.wear)
}

/// The business on the lot builds its own jetty on the water (the sim
/// draws that): no little pier of ours beside it.
fn own_jetty(e: &Extras) -> bool {
    e.business.and_then(nd_sim::World::jetty_kind_for).is_some()
}

/// Whether the water lies behind the lot (the usual waterfront lot).
fn water_behind(c: &Ctx) -> bool {
    c.shore.is_some_and(|s| s.seaward != Vec2::ZERO) && sea_local(c, c.shore).y > 0.6
}

// ── shared pieces ─────────────────────────────────────────────────────────────

/// A window on a face: dark or lit glass, a white frame and a sill (mid
/// detail), a glazing bar (near), and painted shutters either side.
#[allow(clippy::too_many_arguments)]
fn sash(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32, shut: Option<[f32; 3]>) {
    let f = &c.frame;
    let tr = trim(c);
    if coin(rng, c.lit_chance()) {
        let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.9));
        face.glaze(g, c, t, y, PANE, w, h, Pane::Lit(col));
    } else {
        face.glaze(g, c, t, y, PANE, w, h, Pane::Dark);
    }
    g.lod(Lod::Mid, |g| {
        face.window_frame(g, c, t, y, w, h, 0.18, tr);
        face.bx(g, f, Concrete, t, y - h * 0.5 - 0.13, 0.08, Vec3::new(w + 0.3, 0.08, 0.16), tr);
    });
    g.lod(Lod::Near, |g| {
        face.decal(g, f, Matte, t, y, PANE + 0.008, 0.05, h, tr);
        face.decal(g, f, Matte, t, y + h * 0.12, PANE + 0.008, w, 0.05, tr);
    });
    if let Some(sc) = shut {
        let sw = w * 0.5;
        if face.len > w + 2.0 * sw + 0.6 && (t.abs() + w * 0.5 + sw + 0.2) < face.len * 0.5 {
            g.lod(Lod::Mid, |g| {
                for s in [-1.0f32, 1.0] {
                    face.decal(g, f, Wood, t + s * (w * 0.5 + 0.12 + sw * 0.5), y, FRAME, sw, h + 0.1, sc);
                }
            });
        }
    }
}

/// A door with a fanlight over it and a step.
fn door(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, col: [f32; 3]) {
    let f = &c.frame;
    face.decal(g, f, Wood, t, 1.1, 0.04, 1.0, 2.2, col);
    let fan = if coin(rng, c.lit_chance()) { Pane::Lit(mul(INSIDE, 0.6)) } else { Pane::Dark };
    face.glaze(g, c, t, 2.5, PANE, 0.9, 0.35, fan);
    g.lod(Lod::Mid, |g| {
        face.window_frame(g, c, t, 1.5, 1.0, 2.9, 0.16, trim(c));
        // Its back 3 cm off the wall: a plinth or a deck may meet the wall there.
        face.bx(g, f, Concrete, t, 0.08, 0.32, Vec3::new(1.4, 0.16, 0.58), mul(pick(rng, &c.palette.concrete), 1.05));
    });
}

/// A string of bulbs on a sagging wire between lot-local anchors, about
/// `spacing` apart: the wire mid detail, each bulb's glass near detail,
/// its light base glow lit only after dark; a few dead on a run-down front.
fn bulbs(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec3, b: Vec3, sag: f32, spacing: f32) {
    let len = a.distance(b);
    if len < 1.0 {
        return;
    }
    let f = &c.frame;
    let (wa, wb) = (f.at(a.x, a.y, a.z), f.at(b.x, b.y, b.z));
    g.lod(Lod::Mid, |g| g.part(Matte).wire(wa, wb, sag, 0.025, rgba(DARK)));
    // The sag the wire itself is drawn with (`MeshData::wire` clamps it).
    let steep = ((b.y - a.y) / len).abs();
    let sag = if steep > 1e-3 { sag.min(0.8 * len / (4.0 * steep)) } else { sag };
    let n = ((len / spacing).round() as i32).max(2);
    for i in 1..n {
        let t = i as f32 / n as f32;
        let p = a.lerp(b, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t) + 0.07);
        let w = f.at(p.x, p.y, p.z);
        g.lod(Lod::Near, |g| g.part(Matte).boxed(w, Vec3::splat(0.08), f.yaw(), rgba([0.86, 0.84, 0.74])));
        if !coin(rng, 0.3 * c.wear) {
            let from = g.part(Glow).positions.len();
            g.part(Glow).boxed(w, Vec3::splat(0.14), f.yaw(), rgba(mul(BULB, 1.0 - 0.3 * c.wear)));
            mark_glow(g, from, NIGHT_MARK);
        }
    }
}

/// A lamp on a post: lit after dark.
fn lamp_post(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    let f = &c.frame;
    f.bx(g, Matte, p.x, h * 0.5, p.y, Vec3::new(0.12, h, 0.12), mul(SHUTTERS[0], 0.8));
    g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x, h + 0.06, p.y, Vec3::new(0.36, 0.1, 0.36), DARK));
    let from = g.part(Glow).positions.len();
    f.bx(g, Glow, p.x, h - 0.22, p.y, Vec3::new(0.28, 0.36, 0.28), mul(BULB, 0.9));
    mark_glow(g, from, NIGHT_MARK);
}

/// A gabled roof on a rectangle: its two slopes (`s`) and the gable
/// triangles in the wall's own finish, the ridge along `u` (`along_u`) or
/// along `v`.
#[allow(clippy::too_many_arguments)]
fn gabled(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, along_u: bool, eave: f32, s: Surface, roof: [f32; 3], wall_s: Surface, wall: [f32; 3]) {
    let f = &c.frame;
    let p = |a: f32, yy: f32, b: f32| if along_u { f.at(a, yy, b) } else { f.at(b, yy, a) };
    let d = |a: f32, b: f32| if along_u { f.dir3(Vec2::new(a, b)) } else { f.dir3(Vec2::new(b, a)) };
    let (a0, a1, b0, b1) = if along_u { (r.u0, r.u1, r.v0, r.v1) } else { (r.v0, r.v1, r.u0, r.u1) };
    let half = (b1 - b0) * 0.5;
    if half < 0.3 || a1 - a0 < 0.3 || rise < 0.1 {
        return;
    }
    let bm = (b0 + b1) * 0.5;
    let ye = y - rise * eave / half;
    let col = rgba(roof);
    // The slopes run a little past the gables (verges).
    let (va, vb) = (a0 - 0.15, a1 + 0.15);
    for sd in [-1.0f32, 1.0] {
        let be = bm + sd * (half + eave);
        let n = (d(0.0, sd) * rise + Vec3::Y * half).normalize();
        g.part(s).quad_facing([p(va, ye, be), p(vb, ye, be), p(vb, y + rise, bm), p(va, y + rise, bm)], n, col);
        // The underside of the eave, so the roof has a thickness from below.
        g.part(s).quad_facing([p(va, ye - 0.08, be), p(va, y + rise - 0.08, bm), p(vb, y + rise - 0.08, bm), p(vb, ye - 0.08, be)], -n, rgba(mul(roof, 0.6)));
    }
    for (a, sg) in [(a0, -1.0f32), (a1, 1.0)] {
        g.part(wall_s).tri_facing(p(a, y, b0), p(a, y, b1), p(a, y + rise, bm), d(sg, 0.0), rgba(wall));
    }
}

/// A hipped roof on a rectangle: eaves `over` out at `y`, a short ridge
/// `rise` up along the longer side.
fn hipped(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, over: f32, s: Surface, col: [f32; 3]) {
    let f = &c.frame;
    let q = r.c();
    let (hw, hd) = (r.w() * 0.5, r.d() * 0.5);
    let bottom = rect(q.x, q.y, hw + over, hd + over);
    let (rw, rd) = if hw >= hd { ((hw - hd).max(0.3), 0.06) } else { (0.06, (hd - hw).max(0.3)) };
    loft(g, f, s, &bottom, &rect(q.x, q.y, rw, rd), y, y + rise, col);
}

/// A small timber pier out over the water from where the lot meets it,
/// on piles, with bollards, a ladder, a lamp at its end and a dinghy tied
/// alongside. Only on a lot that touches the water, and not where the
/// business builds its own jetty. `from` is a lot-local point to run it
/// out from (the lot's middle when `None`).
fn small_pier(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras, from: Option<Vec2>) {
    let Some(look) = c.shore else { return };
    if look.seaward == Vec2::ZERO || own_jetty(e) {
        return;
    }
    let f = &c.frame;
    let pal = c.palette;
    let sea = sea_local(c, c.shore);
    let start = from.unwrap_or_else(|| poly_center(&c.lot_poly));
    if !c.inside_local(start) {
        return;
    }
    let mut d = 0.0;
    while d < 300.0 && c.inside_local(start + sea * (d + 0.5)) {
        d += 0.5;
    }
    let root = start + sea * d;
    let across = sea.perp();
    let len = rng.random_range(7.0..11.0);
    let w = rng.random_range(1.8..2.4);
    let drop = look.drop.max(0.8);
    let wood = mul(pal.wood, 0.85 - 0.25 * c.wear);
    let deck = [root - across * w * 0.5 - sea * 1.0, root + across * w * 0.5 - sea * 1.0, root + across * w * 0.5 + sea * len, root - across * w * 0.5 + sea * len];
    // Counter-clockwise for the extrusion.
    let deck: Vec<Vec2> = if nd_sim::zoning::polygon_area(&deck) < 0.0 { deck.iter().rev().copied().collect() } else { deck.to_vec() };
    extrude(g, f, Wood, &deck, 0.02, 0.3, wood);
    let pile = rgba(mul(pal.wood, 0.45));
    let mut k = 1.5;
    while k <= len {
        for s in [-1.0f32, 1.0] {
            let p = root + sea * k + across * s * (w * 0.5 + 0.12);
            g.part(Wood).cylinder_capped(f.to_world(p.x, p.y), 0.14, 0.14, -drop - 1.5, 0.55, 6, false, true, pile);
        }
        k += 2.6;
    }
    let tip = root + sea * (len - 0.4);
    g.lod(Lod::Mid, |g| {
        // A ladder down to the water at the end.
        let l = tip + across * (w * 0.5 + 0.05);
        let (a, b) = (f.at(l.x, 0.3, l.y), f.at(l.x, -drop + 0.2, l.y));
        g.part(Matte).beam(a + f.dir3(sea) * 0.25, b + f.dir3(sea) * 0.25, 0.05, rgba(mul(pal.steel, 0.7)));
        g.part(Matte).beam(a - f.dir3(sea) * 0.25, b - f.dir3(sea) * 0.25, 0.05, rgba(mul(pal.steel, 0.7)));
        // A dinghy tied alongside.
        let boat = root + sea * (len * 0.55) - across * (w * 0.5 + 1.1);
        let yaw = sea.y.atan2(sea.x) - f.yaw();
        let hull = BRIGHT[rng.random_range(0..BRIGHT.len())];
        f.bx_yaw(g, Paint, boat.x, -drop + 0.25, boat.y, Vec3::new(3.6, 0.5, 1.3), mul(hull, 0.85 - 0.3 * c.wear), yaw);
        f.bx_yaw(g, Wood, boat.x, -drop + 0.52, boat.y, Vec3::new(3.0, 0.06, 1.0), wood, yaw);
    });
    g.lod(Lod::Near, |g| {
        for s in [-1.0f32, 1.0] {
            let b = root + sea * (len * 0.5) + across * s * (w * 0.5 - 0.2);
            g.part(Matte).cylinder(f.to_world(b.x, b.y), 0.13, 0.11, 0.3, 0.75, 6, rgba(DARK));
        }
    });
    lamp_post(g, c, tip - across * (w * 0.5 - 0.15), 3.0);
}

/// Yard clutter of a harbour: crates, buoys and a stack of lobster pots at
/// lot-local `p`.
fn harbour_clutter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 1.4 {
        return;
    }
    g.lod(Lod::Mid, |g| {
        // Lobster pots: half-barrels of net on a frame.
        for k in 0..rng.random_range(2..5) {
            let q = p + Vec2::new((k % 2) as f32 * 0.9, (k / 2) as f32 * 0.7);
            f.bx(g, Tarp, q.x, 0.25, q.y, Vec3::new(0.8, 0.5, 0.6), mul(NETS[k % NETS.len()], 1.0 - 0.3 * c.wear));
        }
    });
    g.lod(Lod::Near, |g| {
        for k in 0..3 {
            let q = p + Vec2::new(-0.9, k as f32 * 0.45);
            g.part(Paint).cylinder(f.to_world(q.x, q.y), 0.2, 0.2, 0.0, 0.42, 8, rgba(BRIGHT[(k + rng.random_range(0..3)) % BRIGHT.len()]));
        }
    });
}

/// A beach hut for a lot too small for anything else: pastel boards, a
/// striped door, a pitched roof.
fn hut(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let a = R::of(&c.poly);
    let Some(r) = fit(c, R::new(a.c().x - 1.4, a.c().x + 1.4, a.v0 + 0.5, a.v0 + 3.0), 1.2) else { return };
    let col = pastel(c, rng);
    block(g, c, Wood, &r, 0.0, 2.4, col);
    gabled(g, c, &r, 2.4, 0.8, false, 0.2, Metal, roof_col(c, rng), Wood, col);
    let fr = r.front();
    let st = STRIPES[rng.random_range(0..STRIPES.len())];
    fr.decal(g, &c.frame, Wood, 0.0, 1.0, 0.04, (fr.len * 0.5).min(1.0), 2.0, mul(st, 1.0 - 0.3 * c.wear));
}

// ── homes ─────────────────────────────────────────────────────────────────────

/// A terrace of narrow pastel houses on the front: each its own colour and
/// height, gables to the street or one long roof along it, a fanlit door,
/// a bay window on the wider ones, sash windows with shutters, a balcony
/// here and there, chimneys, salt streaks; bulbs strung along the front;
/// yards behind, and on the water a small pier off the back.
pub fn pastel_terrace(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(2.9, 3.3);
    let area = R::of(&c.poly);
    let depth = if area.d() < 12.0 { area.d() } else { (area.d() * 0.5).clamp(8.0, 11.0) };
    let Some(row) = fit(c, R::new(area.u0, area.u1, area.v0, area.v0 + depth), 4.0) else { return hut(g, c, rng) };
    let n = ((row.w() / 5.6).round() as i32).max(1);
    let hw = row.w() / n as f32;
    let gables = coin(rng, 0.6) && hw > 4.0;
    let roof = roof_col(c, rng);
    let plinth = mul(pick(rng, &pal.concrete), 0.8);
    let tr = trim(c);
    let mut top = 0.0f32;
    for i in 0..n {
        let hr = R::new(row.u0 + i as f32 * hw, row.u0 + (i + 1) as f32 * hw, row.v0, row.v1);
        let floors = if gables { (c.floors as i32 + rng.random_range(-1..=1)).clamp(2, 4) } else { (c.floors as i32).clamp(2, 4) };
        let h = floors as f32 * fh;
        top = top.max(h);
        let col = pastel(c, rng);
        block(g, c, Plaster, &hr, 0.0, h, col);
        let front = hr.front();
        // Its back a centimetre off the wall, clear of a bay window's.
        front.bx(g, f, Concrete, 0.0, 0.3, 0.045, Vec3::new(hr.w(), 0.6, 0.07), plinth);
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dt = side * (hw * 0.5 - 0.95).max(0.0);
        let dc = shutter(c, rng);
        door(g, c, rng, &front, dt, dc);
        let shut = coin(rng, 0.6).then(|| shutter(c, rng));
        let wt = -dt * 0.45;
        if hw > 5.0 && coin(rng, 0.55) {
            // A bay window out over the pavement's edge.
            let bw = (hw * 0.4).min(2.6);
            front.bx(g, f, Plaster, wt, fh * 0.45 + 0.3, 0.3, Vec3::new(bw, fh * 0.9, 0.6), col);
            let bay = Face::from_edge(front.at(wt - bw * 0.5, 0.6), front.at(wt + bw * 0.5, 0.6));
            sash(g, c, rng, &bay, 0.0, fh * 0.5 + 0.1, bw * 0.7, fh * 0.5, None);
            g.lod(Lod::Mid, |g| front.bx(g, f, Concrete, wt, fh * 0.9 + 0.32, 0.36, Vec3::new(bw + 0.2, 0.12, 0.76), tr));
        } else {
            sash(g, c, rng, &front, wt, fh * 0.55, 1.0, fh * 0.45, shut);
        }
        let per = if hw > 5.2 { 2 } else { 1 };
        for k in 1..floors {
            let y = k as f32 * fh + fh * 0.55;
            for j in 0..per {
                let t = if per == 1 { 0.0 } else { (j as f32 - 0.5) * hw * 0.48 };
                sash(g, c, rng, &front, t, y, 0.95, fh * 0.48, shut);
            }
            if k == 1 && coin(rng, 0.35) {
                balcony(g, c, &front, 0.0, k as f32 * fh + 0.12, (hw - 1.2).min(4.0), 0.8, tr, mul(tr, 0.95));
            }
        }
        let back = hr.back();
        pane_row(g, c, rng, &back, 0.0, floors, fh, 2.6, 0.9, fh * 0.45, true);
        grime(g, c, rng, &front, 0.0, h, 0.4 + 1.6 * c.wear);
        if gables {
            let rise = (hw * 0.45).clamp(1.4, 3.0);
            gabled(g, c, &hr, h, rise, false, 0.3, Panel, roof, Plaster, col);
            // An attic window in the gable.
            sash(g, c, rng, &front, 0.0, h + rise * 0.35, 0.6, 0.7, None);
            if i > 0 {
                f.bx(g, Brick, hr.u0, h + rise * 0.6, row.v1 - 1.2, Vec3::new(0.7, rise * 1.2 + 0.6, 0.6), mul(pal.brick(i as usize), 0.8));
            }
        } else {
            // Party-wall chimneys up through the one roof.
            if i > 0 {
                let rise = (row.d() * 0.3).clamp(1.2, 2.8);
                f.bx(g, Brick, hr.u0, h + rise * 0.7, row.c().y, Vec3::new(0.7, rise * 0.9 + 0.6, 0.6), mul(pal.brick(i as usize), 0.8));
            }
            cornice(g, c, &hr.poly(), h - 0.25, 0.25, 0.06, Concrete, tr);
        }
    }
    if !gables {
        let rise = (row.d() * 0.3).clamp(1.2, 2.8);
        gabled(g, c, &row, top, rise, true, 0.35, Panel, roof, Plaster, pastel(c, rng));
    }
    // Bulbs along the front, house to house, under the first-floor sills.
    let fr = row.front();
    for i in 0..n {
        let t0 = -fr.len * 0.5 + i as f32 * hw + 0.3;
        let (a, b) = (fr.at(t0, 0.35), fr.at(t0 + hw - 0.6, 0.35));
        bulbs(g, c, rng, p3(a.x, fh + 0.15, a.y), p3(b.x, fh + 0.15, b.y), 0.3, 0.9);
    }
    // Yards behind, a wall at their end, a pier off the back on the water.
    let back_room = area.v1 - row.v1;
    if back_room > 3.0 {
        let wall_v = row.v1 + back_room.min(10.0) - 0.3;
        ground(g, f, Paving, &clip_box(&c.poly, row.u0, row.u1, row.v1 + 0.05, wall_v), 0.04, mul(pal.paving, 0.8));
        for i in 0..n {
            let p = Vec2::new(row.u0 + (i as f32 + 0.3) * hw, row.v1 + 1.6);
            if coin(rng, 0.4) {
                harbour_clutter(g, c, rng, p);
            }
        }
    }
    if water_behind(c) {
        small_pier(g, c, rng, e, Some(Vec2::new(row.c().x, row.v1 + 0.5)));
    }
}

/// One boathouse: clapboard walls, its gable to the street and the water,
/// a tall glazed boat door at each end, a loft door under a hoist beam,
/// side windows, bulbs along the front rakes. Returns the wall top.
fn boathouse_shed(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, floors: i32) -> f32 {
    let f = &c.frame;
    let fh = c.fh().clamp(2.9, 3.3);
    let h = floors as f32 * fh;
    let col = pastel(c, rng);
    let tr = trim(c);
    block(g, c, Wood, r, 0.0, h, col);
    let rise = (r.w() * 0.42).clamp(1.5, 4.0);
    gabled(g, c, r, h, rise, false, 0.35, Metal, roof_col(c, rng), Wood, col);
    // Clapboard lines on the street gable.
    let front = r.front();
    g.lod(Lod::Near, |g| {
        let mut y = 0.6;
        while y < h - 0.2 {
            front.decal(g, f, Matte, 0.0, y, 0.012, front.len - 0.1, 0.035, mul(col, 0.78));
            y += 0.55;
        }
    });
    for (face, end) in [(front, 0), (r.back(), 1)] {
        // The old boat door, glazed now, under a lintel beam.
        let dw = (face.len * 0.5).clamp(1.6, 4.2);
        let dh = (h * 0.62).min(fh * 1.6);
        if coin(rng, c.lit_chance()) {
            face.glaze(g, c, 0.0, dh * 0.5 + 0.1, PANE, dw, dh, Pane::Lit(mul(INSIDE, rng.random_range(0.45..0.75))));
        } else {
            face.glaze(g, c, 0.0, dh * 0.5 + 0.1, PANE, dw, dh, Pane::Dark);
        }
        g.lod(Lod::Mid, |g| {
            face.window_frame(g, c, 0.0, dh * 0.5 + 0.1, dw, dh, 0.24, tr);
            for k in 1..4 {
                face.decal(g, f, Matte, -dw * 0.5 + dw * k as f32 / 4.0, dh * 0.5 + 0.1, PANE + 0.008, 0.07, dh, tr);
            }
        });
        // The loft door in the gable, the hoist beam over it.
        let ly = h + rise * 0.15;
        sash(g, c, rng, &face, 0.0, ly.min(h + rise * 0.3) - 0.2, 0.9, 1.2, None);
        if end == 0 {
            g.lod(Lod::Mid, |g| face.bx(g, f, Wood, 0.0, h + rise * 0.62, 0.6, Vec3::new(0.22, 0.22, 1.2), mul(c.palette.wood, 0.5)));
            // Bulbs up the rakes of the street gable.
            let (l, rr, top) = (face.at(-face.len * 0.5 - 0.1, 0.3), face.at(face.len * 0.5 + 0.1, 0.3), face.at(0.0, 0.3));
            let ye = h - 0.1;
            bulbs(g, c, rng, p3(l.x, ye, l.y), p3(top.x, h + rise - 0.2, top.y), 0.05, 0.7);
            bulbs(g, c, rng, p3(top.x, h + rise - 0.2, top.y), p3(rr.x, ye, rr.y), 0.05, 0.7);
        }
    }
    for face in [r.left(), r.right()] {
        pane_row(g, c, rng, &face, 0.0, floors, fh, 3.2, 1.0, 1.1, true);
        // A lifebuoy on the wall.
        if coin(rng, 0.5) && face.len > 4.0 {
            let p = face.at(-face.len * 0.3, 0.12);
            g.lod(Lod::Mid, |g| {
                let a = f.at(p.x, 1.6, p.y);
                g.part(Paint).tube(a - f.dir3(face.out) * 0.06, a + f.dir3(face.out) * 0.06, 0.38, 10, rgba([0.92, 0.36, 0.14]));
            });
        }
    }
    h
}

/// Converted boathouses: one to three clapboard sheds side by side (each
/// its own pastel) with glazed boat doors, a slipway and a dinghy on
/// trestles; on the water a deck and a pier off the back.
pub fn boathouses(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 5.0 || area.d() < 6.0 {
        return hut(g, c, rng);
    }
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.85));
    let n = ((area.w() / 9.5).floor() as i32).clamp(1, 3);
    let slot = area.w() / n as f32;
    let sw = (slot - 1.4).clamp(4.0, 9.0);
    let sd = (area.d() - 3.0).clamp(5.0, 16.0);
    let floors = (c.floors as i32).clamp(1, 2);
    let mut last = f32::MIN;
    for i in 0..n {
        let cu = area.u0 + (i as f32 + 0.5) * slot;
        let v0 = area.v0 + 1.0 + rng.random_range(0.0..1.2);
        let Some(r) = fit(c, R::new(cu - sw * 0.5, cu + sw * 0.5, v0, (v0 + sd).min(area.v1 - 0.4)), 3.5) else { continue };
        // One the lot's edge pushed into the last is left out.
        if r.u0 < last + 0.8 {
            continue;
        }
        last = r.u1;
        boathouse_shed(g, c, rng, &r, floors);
        // A slipway out of the back door to the lot's edge.
        let room = area.v1 - r.v1;
        if room > 1.5 {
            let ramp = R::new(r.c().x - 1.3, r.c().x + 1.3, r.v1, area.v1 - 0.1);
            ground(g, f, Concrete, &clip_box(&c.poly, ramp.u0, ramp.u1, ramp.v0, ramp.v1), 0.06, mul(pick(rng, &pal.concrete), 0.7));
        }
        // An upturned dinghy on trestles beside it.
        let side = Vec2::new(if i == 0 { r.u0 - 0.9 } else { r.u1 + 0.9 }, r.c().y);
        if (i == 0 || i == n - 1) && c.inside_local(side) && distance_to_boundary(side, &c.lot_poly) > 1.0 {
            let hull = BRIGHT[rng.random_range(0..BRIGHT.len())];
            g.lod(Lod::Mid, |g| {
                f.bx(g, Paint, side.x, 0.85, side.y, Vec3::new(1.1, 0.4, 3.4), mul(hull, 0.8 - 0.3 * c.wear));
                for s in [-1.0f32, 1.0] {
                    f.bx(g, Wood, side.x, 0.32, side.y + s * 1.1, Vec3::new(1.2, 0.64, 0.1), mul(pal.wood, 0.6));
                }
            });
        }
    }
    harbour_clutter(g, c, rng, Vec2::new(area.u0 + 1.6, area.v0 + 1.2));
    if water_behind(c) {
        small_pier(g, c, rng, e, None);
    }
}

/// A round lighthouse tower at lot-local `p`: banded walls, little
/// windows, a gallery with a rail, a glazed lamp room glowing after dark,
/// a cap. `y0` its base, `top` the gallery floor.
fn lighthouse(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, y0: f32, top: f32) {
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    let sides = 16;
    let tr = trim(c);
    let band = mul(if coin(rng, 0.5) { STRIPES[0] } else { pastel(c, rng) }, 1.0 - 0.2 * c.wear);
    let k = ((top - y0) / 2.4).round().max(1.0) as i32;
    let step = (top - y0) / k as f32;
    // Tapering a tenth up its height, band by band.
    let at = |y: f32| r * (1.0 - 0.1 * (y - y0) / (top - y0).max(0.1));
    for i in 0..k {
        let col = if i % 2 == 0 { tr } else { band };
        let (ya, yb) = (y0 + i as f32 * step, y0 + (i + 1) as f32 * step);
        g.part(Plaster).cylinder_capped(w, at(ya), at(yb), ya, yb, sides, false, i == k - 1, rgba(col));
    }
    // Small windows up the street side.
    let n = (k - 1).max(1);
    for i in 0..n {
        let a = -std::f32::consts::FRAC_PI_2 + (i as f32 - (n - 1) as f32 * 0.5) * 0.5;
        let dir = Vec2::new(a.cos(), a.sin());
        let y = y0 + (i as f32 + 0.6) * step;
        let rr = at(y);
        let (pa, pb) = (p + Vec2::new(-dir.y, dir.x) * 0.4 + dir * rr, p - Vec2::new(-dir.y, dir.x) * 0.4 + dir * rr);
        let face = Face::from_edge(pb, pa);
        let face = if face.out.dot(dir) < 0.0 { face.reversed() } else { face };
        if y < top - 0.8 {
            face.glaze(g, c, 0.0, y, 0.1, 0.5, 0.8, if coin(rng, c.lit_chance()) { Pane::Lit(mul(INSIDE, 0.7)) } else { Pane::Dark });
        }
    }
    // The gallery and its rail.
    g.part(Concrete).cylinder(w, r + 0.6, r + 0.6, top, top + 0.2, sides, rgba(tr));
    g.lod(Lod::Mid, |g| g.part(Matte).cylinder_capped(w, r + 0.55, r + 0.55, top + 0.9, top + 1.0, sides, false, false, rgba(DARK)));
    g.lod(Lod::Near, |g| {
        for i in 0..12 {
            let a = i as f32 / 12.0 * std::f32::consts::TAU;
            let q = p + Vec2::new(a.cos(), a.sin()) * (r + 0.55);
            g.part(Matte).cylinder(f.to_world(q.x, q.y), 0.03, 0.03, top + 0.2, top + 0.9, 4, rgba(DARK));
        }
    });
    // The lamp room: glass, the light inside after dark, the cap.
    let lr = r * 0.62;
    // The glass by day; after dark the light just round it, so the lamp
    // room reads from across the bay.
    g.part(Glass).cylinder_capped(w, lr, lr, top + 0.2, top + 2.2, 10, false, false, rgba(c.palette.glass_dark));
    let from = g.part(Glow).positions.len();
    g.part(Glow).cylinder_capped(w, lr * 1.04, lr * 1.04, top + 0.35, top + 2.05, 10, false, false, rgba(mul(BEAM, 1.0 - 0.3 * c.wear)));
    mark_glow(g, from, NIGHT_MARK);
    let cap = mul(if coin(rng, 0.5) { STRIPES[0] } else { SHUTTERS[1] }, 1.0 - 0.25 * c.wear);
    g.part(Metal).cylinder(w, lr + 0.25, 0.08, top + 2.2, top + 3.3, 10, rgba(cap));
    g.lod(Lod::Mid, |g| g.part(Matte).cylinder(w, 0.05, 0.05, top + 3.3, top + 4.0, 4, rgba(DARK)));
}

/// A block of flats with a lighthouse-style tower on its street corner:
/// pastel walls, sash windows with shutters, a café and doors on the
/// street under striped awnings, a cornice and a parapet, bulbs strung
/// from the tower's foot along the front.
pub fn lighthouse_corner(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let fh = c.fh().clamp(2.9, 3.3);
    let area = R::of(&c.poly);
    let depth = area.d().min(14.0);
    let Some(b) = fit(c, R::new(area.u0, area.u1, area.v0, area.v0 + depth), 6.0) else { return pastel_terrace(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(3, 6);
    let h = floors as f32 * fh;
    let col = pastel(c, rng);
    let tr = trim(c);
    block(g, c, Plaster, &b, 0.0, h, col);
    let shut = coin(rng, 0.7).then(|| shutter(c, rng));
    let front = b.front();
    // The street floor: a café or shops under striped awnings, a door.
    let bays = ((front.len / 4.2).floor() as i32).max(1);
    let bw = front.len / bays as f32;
    let stripe_col = STRIPES[rng.random_range(0..STRIPES.len())];
    for i in 0..bays {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * bw;
        if i == bays / 2 {
            let dc = shutter(c, rng);
            door(g, c, rng, &front, t, dc);
            continue;
        }
        face_shop(g, c, rng, &front, t, bw, fh, stripe_col);
    }
    for face in c.faces(&b.poly()) {
        let y0 = if face.is_front { fh } else { 0.0 };
        let fl = if face.is_front { floors - 1 } else { floors };
        let n = ((face.len - 1.0) / 2.6).floor() as i32;
        if n < 1 {
            continue;
        }
        let step = (face.len - 1.0) / n as f32;
        for k in 0..fl {
            for i in 0..n {
                let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
                sash(g, c, rng, &face, t, y0 + k as f32 * fh + fh * 0.55, (step * 0.42).min(1.1), fh * 0.48, if face.is_front { shut } else { None });
            }
        }
        grime(g, c, rng, &face, 0.0, h, 0.3 + 1.5 * c.wear);
    }
    cornice(g, c, &b.poly(), h - 0.3, 0.3, 0.08, Concrete, tr);
    parapet(g, c, &b.poly(), h, 0.8, 0.2, Plaster, mul(col, 0.92));
    // The tower on the corner, a storey or two over the roof.
    // Its gallery (0.6 m out) over the lot: a narrower tower where the
    // corner is tight.
    let left = c.seed % 2 == 0;
    let mut r = (b.w().min(b.d()) * 0.18).clamp(2.0, 3.2);
    let at = |r: f32| Vec2::new(if left { b.u0 + r * 0.8 } else { b.u1 - r * 0.8 }, b.v0 + r * 0.8);
    while r > 1.6 && !(c.inside_local(at(r)) && distance_to_boundary(at(r), &c.lot_poly) > r - 0.2) {
        r -= 0.2;
    }
    let p = at(r);
    if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > r - 0.2 {
        lighthouse(g, c, rng, p, r, 0.0, h + fh * 2.2);
        let end = front.at(if left { front.len * 0.5 - 0.4 } else { -front.len * 0.5 + 0.4 }, 0.4);
        let foot = Vec2::new(p.x + if left { r + 0.1 } else { -r - 0.1 }, b.v0 - 0.4);
        bulbs(g, c, rng, p3(foot.x, fh + 0.2, foot.y), p3(end.x, fh + 0.2, end.y), 0.4, 0.9);
    }
    hk::roof_clutter(g, c, rng, &offset_poly(&b.poly(), 0.3), h, 0.8);
    if water_behind(c) {
        small_pier(g, c, rng, e, Some(Vec2::new(b.c().x, b.v1 + 0.5)));
    }
}

/// A shopfront on a face at `t`, `w` wide: lit or shuttered glass, a
/// fascia board with its name lit, and a striped awning over it.
#[allow(clippy::too_many_arguments)]
fn face_shop(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, fh: f32, stripe: [f32; 3]) {
    let f = &c.frame;
    let gw = (w * 0.78).max(1.0);
    let gh = fh * 0.58;
    if coin(rng, 0.8 - 0.4 * c.wear) {
        face.glaze(g, c, t, gh * 0.5 + 0.35, PANE, gw, gh, Pane::Lit(mul(INSIDE, rng.random_range(0.55..0.85))));
    } else {
        face.decal(g, f, Metal, t, gh * 0.5 + 0.35, 0.04, gw, gh, mul(pick(rng, &c.palette.metal_paint), 0.7));
    }
    g.lod(Lod::Mid, |g| face.window_frame(g, c, t, gh * 0.5 + 0.35, gw, gh, 0.2, trim(c)));
    sign_board(g, c, rng, face, t, fh - 0.25, (gw * 0.8).min(4.0), 0.36);
    // Out over the lot only, at both its ends (a slanted street cuts in).
    let mut reach = (c.reach_out(face) - 0.2).min(1.6);
    while reach > 0.7 && !(c.inside_local(face.at(t - gw * 0.5, reach + 0.1)) && c.inside_local(face.at(t + gw * 0.5, reach + 0.1))) {
        reach -= 0.2;
    }
    if reach > 0.7 {
        let col = mul([0.92, 0.9, 0.84], 1.0 - 0.3 * c.wear);
        g.lod(Lod::Mid, |g| hk::awning(g, c, face, t, gh + 0.55, gw, reach - 0.3, col, Some(mul(stripe, 1.0 - 0.3 * c.wear))));
    }
}

// ── the front: cafés, shacks, the arcade, hotels ─────────────────────────────

/// A boardwalk café: a raised timber deck along the street with tables
/// under striped umbrellas and a rail, bulbs strung on posts over it, the
/// café behind under a striped awning with its windows lit, a flat or a
/// gallery over it, a terrace to the water at the back.
pub fn boardwalk_cafe(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(3.0, 3.4);
    let area = R::of(&c.poly);
    if area.w() < 6.0 || area.d() < 9.0 {
        return seafood_shack(g, c, rng, e);
    }
    let dd = (area.d() * 0.3).clamp(3.5, 6.0);
    let bw = (area.w() - 1.0).min(20.0);
    let bd = (area.d() - dd - 0.5).min(10.0);
    let Some(b) = fit(c, R::new(area.c().x - bw * 0.5, area.c().x + bw * 0.5, area.v0 + dd, area.v0 + dd + bd), 4.0) else { return seafood_shack(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(1, 2);
    let h = floors as f32 * fh;
    let col = pastel(c, rng);
    let tr = trim(c);
    block(g, c, Plaster, &b, 0.0, h, col);
    let wood = mul(pal.wood, 0.95 - 0.3 * c.wear);
    // The deck.
    let deck = R::new(b.u0, b.u1, area.v0 + 0.2, b.v0);
    if let Some(dk) = fit(c, deck, 1.5) {
        block(g, c, Wood, &dk, 0.0, 0.25, wood);
        g.lod(Lod::Near, |g| {
            let mut u = dk.u0 + 0.6;
            while u < dk.u1 - 0.3 {
                f.flat(g, Matte, u, 0.255, dk.c().y, 0.03, dk.d() - 0.1, 0.0, mul(wood, 0.6));
                u += 0.9;
            }
        });
        // The rail along its street edge, with a gap for the steps.
        let fr = dk.front();
        g.lod(Lod::Mid, |g| {
            for s in [-1.0f32, 1.0] {
                let len = fr.len * 0.5 - 1.0;
                if len > 0.5 {
                    fr.bx(g, f, Wood, s * (1.0 + len * 0.5), 1.1, -0.1, Vec3::new(len, 0.08, 0.08), tr);
                }
            }
        });
        // Tables under umbrellas.
        let n = ((dk.w() - 1.5) / 3.0).floor() as i32;
        for i in 0..n.clamp(0, 6) {
            let p = Vec2::new(dk.u0 + 1.5 + i as f32 * 3.0, dk.c().y);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
                let um = (i % 2 == 0 || dk.d() > 4.5).then(|| STRIPES[(i as usize + c.seed as usize) % STRIPES.len()]);
                g.lod(Lod::Mid, |g| table(g, f, p, 0.25, mul(pal.wood, 0.75), um));
            }
        }
        // Posts along the edge and bulbs from post to post and to the café.
        let posts = ((dk.w() / 4.0).floor() as i32).max(1);
        let mut prev: Option<Vec2> = None;
        for i in 0..=posts {
            let p = Vec2::new(dk.u0 + 0.3 + i as f32 * (dk.w() - 0.6) / posts as f32, dk.v0 + 0.3);
            f.bx(g, Wood, p.x, 1.6, p.y, Vec3::new(0.14, 3.2, 0.14), mul(tr, 0.9));
            if let Some(q) = prev {
                bulbs(g, c, rng, p3(q.x, 3.05, q.y), p3(p.x, 3.05, p.y), 0.35, 0.9);
            }
            if i % 2 == 0 {
                bulbs(g, c, rng, p3(p.x, 3.05, p.y), p3(p.x, fh - 0.1, b.v0 - 0.15), 0.3, 1.0);
            }
            prev = Some(p);
        }
    }
    // The café front: big lit windows under a striped awning, a door, the name.
    let front = b.front();
    let st = STRIPES[rng.random_range(0..STRIPES.len())];
    let n = ((front.len / 3.4).floor() as i32).max(1);
    let w = front.len / n as f32;
    for i in 0..n {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * w;
        if i == n / 2 {
            let dc = shutter(c, rng);
            door(g, c, rng, &front, t, dc);
        } else {
            lit_room(g, c, rng, &front, t, w, fh);
        }
    }
    let reach = (dd - 0.6).min(2.4);
    g.lod(Lod::Mid, |g| hk::awning(g, c, &front, 0.0, fh - 0.35, front.len - 0.4, reach, mul([0.92, 0.9, 0.84], 1.0 - 0.3 * c.wear), Some(mul(st, 1.0 - 0.3 * c.wear))));
    sign_board(g, c, rng, &front, 0.0, fh + 0.35, (front.len * 0.5).min(6.0), 0.5);
    if floors > 1 {
        pane_row(g, c, rng, &front, fh, floors - 1, fh, 2.4, 1.0, fh * 0.45, true);
    }
    for face in [b.left(), b.right(), b.back()] {
        pane_row(g, c, rng, &face, 0.0, floors, fh, 3.0, 1.0, fh * 0.42, true);
        grime(g, c, rng, &face, 0.0, h, 0.3 + 1.5 * c.wear);
    }
    if coin(rng, 0.5) {
        gabled(g, c, &b, h, (b.d() * 0.22).clamp(1.0, 2.4), true, 0.4, Metal, roof_col(c, rng), Plaster, col);
    } else {
        parapet(g, c, &b.poly(), h, 0.9, 0.18, Plaster, mul(col, 0.9));
        // A roof terrace: umbrellas and a string of bulbs.
        let rt = b.inset(1.2);
        if rt.w() > 3.0 && rt.d() > 2.5 {
            g.lod(Lod::Mid, |g| table(g, f, rt.c(), h, mul(pal.wood, 0.75), Some(st)));
            bulbs(g, c, rng, p3(rt.u0, h + 2.4, rt.v0), p3(rt.u1, h + 2.4, rt.v0), 0.3, 1.0);
        }
    }
    // A terrace to the water behind.
    let room = area.v1 - b.v1;
    if room > 3.0 {
        let tr_r = R::new(b.u0, b.u1, b.v1 + 0.1, area.v1 - 0.3);
        if let Some(t) = fit(c, tr_r, 2.0) {
            block(g, c, Wood, &t, 0.0, 0.2, mul(wood, 0.95));
            let k = ((t.w() - 1.5) / 3.2).floor() as i32;
            for i in 0..k.clamp(0, 5) {
                let p = Vec2::new(t.u0 + 1.6 + i as f32 * 3.2, t.c().y);
                if distance_to_boundary(p, &c.lot_poly) > 1.0 {
                    g.lod(Lod::Mid, |g| table(g, f, p, 0.2, mul(pal.wood, 0.75), None));
                }
            }
            bulbs(g, c, rng, p3(t.u0 + 0.2, 2.8, t.v1 - 0.2), p3(t.u1 - 0.2, 2.8, t.v1 - 0.2), 0.4, 0.9);
            for u in [t.u0 + 0.2, t.u1 - 0.2] {
                f.bx(g, Wood, u, 1.45, t.v1 - 0.2, Vec3::new(0.12, 2.9, 0.12), mul(tr, 0.9));
            }
        }
    }
    if water_behind(c) {
        small_pier(g, c, rng, e, None);
    }
}

/// Glazing for a café or shop window: a big lit room (or dark glass on a
/// run-down front), its frame.
#[allow(clippy::too_many_arguments)]
fn lit_room(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, fh: f32) {
    let gw = w * 0.8;
    let gh = fh * 0.62;
    let y = gh * 0.5 + 0.3;
    if coin(rng, 0.85 - 0.4 * c.wear) {
        face.glaze(g, c, t, y, PANE, gw, gh, Pane::Lit(mul(INSIDE, rng.random_range(0.6..0.9))));
    } else {
        face.glaze(g, c, t, y, PANE, gw, gh, Pane::Dark);
    }
    g.lod(Lod::Mid, |g| face.window_frame(g, c, t, y, gw, gh, 0.22, trim(c)));
}

/// A seafood shack: a tarred or pastel board shack under a tin roof, a lit
/// serving hatch with its flap propped up, menu boards, a fish on a board
/// over the roof, picnic tables, crates of ice, nets and buoys on the
/// wall, a smoker; bulbs from the hatch out to poles.
pub fn seafood_shack(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    let sw = (area.w() * 0.6).clamp(3.0, 11.0);
    let sd = (area.d() * 0.4).clamp(2.5, 6.0);
    let v0 = area.v0 + (area.d() * 0.25).clamp(0.5, 5.0);
    let Some(r) = fit(c, R::new(area.c().x - sw * 0.5, area.c().x + sw * 0.5, v0, v0 + sd), 2.2) else { return hut(g, c, rng) };
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.8));
    let tarred = coin(rng, 0.45);
    let col = if tarred { mul(TAR, 1.2) } else { pastel(c, rng) };
    let h = 2.9;
    block(g, c, Wood, &r, 0.0, h, col);
    // A tin roof falling to the back.
    let tin = if c.wear > 0.45 { (Rust, pal.rust) } else { (Metal, roof_col(c, rng)) };
    let roof = R::new(r.u0 - 0.4, r.u1 + 0.4, r.v0 - 0.6, r.v1 + 0.4);
    if let Some(rf) = fit(c, roof, 2.0) {
        g.part(tin.0).slab(f.at(rf.c().x, h + 0.7, rf.v0), f.at(rf.c().x, h + 0.05, rf.v1), rf.w(), 0.08, rgba(tin.1));
    }
    let front = r.front();
    // The hatch: a lit opening, the counter, the flap propped over it.
    let hw = (front.len * 0.55).min(4.0);
    front.glaze(g, c, 0.0, 1.55, PANE, hw, 1.0, Pane::Lit(mul(INSIDE, 0.85 - 0.3 * c.wear)));
    front.bx(g, f, Wood, 0.0, 1.0, 0.25, Vec3::new(hw + 0.3, 0.1, 0.5), mul(pal.wood, 0.8));
    g.lod(Lod::Mid, |g| {
        let a = f.at2(front.at(0.0, 0.05), 2.15);
        let b = f.at2(front.at(0.0, 1.0), 2.45);
        g.part(Wood).slab(a, b, hw, 0.05, rgba(mul(col, 0.9)));
    });
    // Menu boards either side, lit after dark.
    for s in [-1.0f32, 1.0] {
        let t = s * (hw * 0.5 + 0.55);
        if t.abs() + 0.45 < front.len * 0.5 {
            front.decal(g, f, Matte, t, 1.6, 0.03, 0.8, 1.0, DARK);
            let from = g.part(Glow).positions.len();
            front.decal(g, f, Glow, t, 1.6, 0.045, 0.6, 0.8, mul([1.6, 1.5, 1.2], 0.55));
            mark_glow(g, from, NIGHT_MARK);
        }
    }
    // The fish over the roof, on its board.
    let sign = mul(pick(rng, &pal.sign_colors), 1.0 - 0.4 * c.wear);
    front.bx(g, f, Wood, 0.0, h + 1.15, -0.2, Vec3::new((front.len * 0.6).min(3.6), 0.9, 0.1), mul(col, 0.8));
    let board = Face::new(front.at(-1.0, -0.14), front.at(1.0, -0.14), front.out);
    fish_sign(g, c, &board, 0.0, h + 1.15, 0.03, 0.7, sign);
    // Nets and buoys on a side wall.
    let side = if c.seed % 2 == 0 { r.left() } else { r.right() };
    g.lod(Lod::Mid, |g| {
        side.decal(g, f, Tarp, 0.0, 1.8, 0.03, (side.len * 0.7).min(3.0), 1.4, mul(NETS[(c.seed % 3) as usize], 1.0 - 0.3 * c.wear));
    });
    g.lod(Lod::Near, |g| {
        for k in 0..3 {
            let p = side.at(-0.8 + k as f32 * 0.8, 0.2);
            g.part(Paint).cylinder(f.to_world(p.x, p.y), 0.16, 0.16, 1.0 + (k % 2) as f32 * 0.3, 1.35 + (k % 2) as f32 * 0.3, 8, rgba(BRIGHT[(k + 1) % BRIGHT.len()]));
        }
    });
    // A smoker out the back: a drum and its stack.
    let sp = Vec2::new(r.u1 - 0.8, r.v1 + 1.0);
    if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > 0.8 {
        g.part(Rust).cylinder(f.to_world(sp.x, sp.y), 0.4, 0.4, 0.0, 1.0, 8, rgba(pal.rust));
        g.part(Matte).cylinder(f.to_world(sp.x, sp.y), 0.1, 0.1, 1.0, 2.4, 6, rgba(DARK));
    }
    // Picnic tables in front, bulbs from the eave to poles at the street.
    let yard_r = R::new(area.u0 + 0.8, area.u1 - 0.8, area.v0 + 0.4, r.v0 - 0.8);
    if yard_r.d() > 1.6 {
        let n = ((yard_r.w() - 1.0) / 2.8).floor() as i32;
        for i in 0..n.clamp(0, 4) {
            let p = Vec2::new(yard_r.u0 + 1.4 + i as f32 * 2.8, yard_r.c().y);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.1 {
                g.lod(Lod::Mid, |g| {
                    let wd = mul(pal.wood, 0.7);
                    f.bx(g, Wood, p.x, 0.74, p.y, Vec3::new(1.8, 0.06, 0.8), wd);
                    for s in [-1.0f32, 1.0] {
                        f.bx(g, Wood, p.x, 0.44, p.y + s * 0.7, Vec3::new(1.8, 0.05, 0.3), wd);
                        f.bx(g, Wood, p.x + s * 0.7, 0.36, p.y, Vec3::new(0.08, 0.72, 1.6), mul(wd, 0.8));
                    }
                });
            }
        }
        for s in [-1.0f32, 1.0] {
            let pole = Vec2::new(r.c().x + s * (r.w() * 0.5 + 0.5), yard_r.v0 + 0.3);
            if c.inside_local(pole) && distance_to_boundary(pole, &c.lot_poly) > 0.3 {
                f.bx(g, Wood, pole.x, 1.5, pole.y, Vec3::new(0.12, 3.0, 0.12), mul(pal.wood, 0.5));
                let eave = front.at(0.0, 0.3);
                bulbs(g, c, rng, p3(eave.x, h + 0.2, eave.y), p3(pole.x, 2.9, pole.y), 0.35, 0.9);
            }
        }
    }
    fish_crates(g, c, rng, Vec2::new(r.u0 - 1.0, r.v0 + 0.8), 3);
    fish_crates(g, c, rng, Vec2::new(r.u0 - 1.0, r.v0 + 2.0), 2);
    harbour_clutter(g, c, rng, Vec2::new(r.u1 + 1.6, r.v1 - 0.5));
    if water_behind(c) {
        small_pier(g, c, rng, e, None);
    }
}

/// A striped-awning arcade: a pastel row whose street floor stands back
/// behind white columns, each bay a shop under its own striped awning with
/// its name lit over the arch; flats over it with shutters and little
/// balconies; a cornice, a parapet with a name pediment; bulbs strung from
/// column to column.
pub fn awning_arcade(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(3.0, 3.5);
    let area = R::of(&c.poly);
    let depth = area.d().min(14.0);
    let Some(b) = fit(c, R::new(area.u0, area.u1, area.v0, area.v0 + depth), 5.0) else { return seafood_shack(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(2, 4);
    let h = floors as f32 * fh;
    let col = pastel(c, rng);
    let tr = trim(c);
    let ad = if b.d() > 8.0 { 2.6 } else { 0.0 };
    block(g, c, Plaster, &R::new(b.u0, b.u1, b.v0 + ad, b.v1), 0.0, fh, mul(col, 0.95));
    block(g, c, Plaster, &R::new(b.u0, b.u1, b.v0, b.v1), fh, h, col);
    let upper = b.front();
    let shopf = R::new(b.u0, b.u1, b.v0 + ad, b.v1).front();
    let bays = ((upper.len / 3.8).floor() as i32).max(1);
    let bw = upper.len / bays as f32;
    let first = rng.random_range(0..STRIPES.len());
    for i in 0..bays {
        let t = -upper.len * 0.5 + (i as f32 + 0.5) * bw;
        let st = STRIPES[(first + i as usize) % STRIPES.len()];
        if ad > 0.0 {
            face_shop(g, c, rng, &shopf, t, bw - 0.5, fh, st);
        } else {
            face_shop(g, c, rng, &upper, t, bw - 0.5, fh, st);
        }
    }
    if ad > 0.0 {
        // The columns and the beam over them; paving under the arcade.
        for i in 0..=bays {
            let t = -upper.len * 0.5 + i as f32 * bw;
            let t = t.clamp(-upper.len * 0.5 + 0.25, upper.len * 0.5 - 0.25);
            upper.bx(g, f, Concrete, t, fh * 0.5, -0.25, Vec3::new(0.42, fh, 0.42), tr);
        }
        ground(g, f, Paving, &R::new(b.u0, b.u1, b.v0 + 0.05, b.v0 + ad - 0.05).poly(), 0.035, mul(pal.paving, 0.95));
        for i in 0..bays {
            let t0 = -upper.len * 0.5 + i as f32 * bw + 0.3;
            let (a, bb) = (upper.at(t0, -0.25), upper.at(t0 + bw - 0.6, -0.25));
            bulbs(g, c, rng, p3(a.x, fh - 0.3, a.y), p3(bb.x, fh - 0.3, bb.y), 0.35, 0.8);
        }
    }
    let shut = coin(rng, 0.6).then(|| shutter(c, rng));
    for k in 1..floors {
        for i in 0..bays {
            let t = -upper.len * 0.5 + (i as f32 + 0.5) * bw;
            sash(g, c, rng, &upper, t, k as f32 * fh + fh * 0.55, (bw * 0.35).min(1.1), fh * 0.5, shut);
            if k == 1 && coin(rng, 0.4) {
                g.lod(Lod::Mid, |g| balcony(g, c, &upper, t, k as f32 * fh + 0.12, (bw * 0.5).min(1.8), 0.5, tr, mul(SHUTTERS[0], 0.9)));
            }
        }
    }
    for face in [b.left(), b.right(), b.back()] {
        pane_row(g, c, rng, &face, fh, floors - 1, fh, 3.0, 1.0, fh * 0.45, true);
        grime(g, c, rng, &face, 0.0, h, 0.3 + 1.5 * c.wear);
    }
    grime(g, c, rng, &upper, fh, h, 0.3 + 1.5 * c.wear);
    cornice(g, c, &b.poly(), h - 0.3, 0.3, 0.08, Concrete, tr);
    parapet(g, c, &b.poly(), h, 0.8, 0.2, Plaster, mul(col, 0.9));
    // A pediment with the arcade's name in the middle of the front.
    let pw = (upper.len * 0.3).clamp(3.0, 9.0);
    upper.bx(g, f, Plaster, 0.0, h + 1.2, 0.12, Vec3::new(pw, 1.6, 0.24), mul(col, 0.92));
    sign_board(g, c, rng, &upper, 0.0, h + 1.2, pw * 0.7, 0.55);
    hk::roof_clutter(g, c, rng, &offset_poly(&b.poly(), 0.3), h, 0.8);
}

/// A boutique hotel: a pastel block of French windows on balconies, a
/// portico with its lit name and bulbs along the canopy, flags on poles
/// before it, a zinc mansard top floor with dormers, a roof terrace with
/// umbrellas and a flag; on the water a terrace and a pier.
pub fn boutique_hotel(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(3.0, 3.4);
    let area = R::of(&c.poly);
    let setback = (area.d() * 0.15).clamp(0.0, 3.0);
    let bw = (area.w() - 1.0).min(30.0);
    let bd = (area.d() - setback).min(16.0);
    let Some(b) = fit(c, R::new(area.c().x - bw * 0.5, area.c().x + bw * 0.5, area.v0 + setback, area.v0 + setback + bd), 6.0) else { return lighthouse_corner(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(3, 7);
    let body = floors - 1;
    let h = body as f32 * fh;
    let col = pastel(c, rng);
    let tr = trim(c);
    block(g, c, Plaster, &b, 0.0, h, col);
    let front = b.front();
    // The lobby: glazed, the portico in the middle.
    let n = ((front.len / 3.0).floor() as i32).max(1);
    let step = front.len / n as f32;
    for i in 0..n {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * step;
        lit_room(g, c, rng, &front, t, step, fh);
    }
    let pw = (front.len * 0.3).clamp(3.0, 7.0);
    let reach = c.reach_out(&front).min(3.0) - 0.3;
    if reach > 1.0 {
        front.bx(g, f, Concrete, 0.0, fh - 0.1, reach * 0.5, Vec3::new(pw, 0.3, reach), tr);
        for s in [-1.0f32, 1.0] {
            front.bx(g, f, Concrete, s * (pw * 0.5 - 0.2), (fh - 0.25) * 0.5, reach - 0.25, Vec3::new(0.3, fh - 0.25, 0.3), tr);
        }
        let canopy = Face::new(front.at(-pw * 0.5, reach), front.at(pw * 0.5, reach), front.out);
        sign_board(g, c, rng, &canopy, 0.0, fh + 0.35, pw * 0.75, 0.4);
        let (a, bb) = (front.at(-pw * 0.5, reach + 0.05), front.at(pw * 0.5, reach + 0.05));
        bulbs(g, c, rng, p3(a.x, fh - 0.35, a.y), p3(bb.x, fh - 0.35, bb.y), 0.0, 0.45);
    }
    // Flags on poles before it.
    let flags = ((front.len / 6.0).floor() as i32).clamp(1, 4);
    for i in 0..flags {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * front.len / flags as f32;
        let p = front.at(t, (reach * 0.5).max(0.6));
        if (t.abs() > pw * 0.5 + 0.6 || reach <= 1.0) && c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.9 {
            flagpole(g, f, p, 7.5, mul(BRIGHT[(i as usize + c.seed as usize) % BRIGHT.len()], 1.0 - 0.3 * c.wear), false);
        }
    }
    // French windows on balconies, floor after floor.
    let shut = coin(rng, 0.5).then(|| shutter(c, rng));
    let m = ((front.len - 1.0) / 3.2).floor().max(1.0) as i32;
    let ms = (front.len - 1.0) / m as f32;
    let rail = if coin(rng, 0.5) { tr } else { mul(SHUTTERS[0], 0.8) };
    for k in 1..body {
        for i in 0..m {
            let t = -(front.len - 1.0) * 0.5 + (i as f32 + 0.5) * ms;
            sash(g, c, rng, &front, t, k as f32 * fh + fh * 0.45, (ms * 0.42).min(1.3), fh * 0.66, shut);
            g.lod(Lod::Mid, |g| balcony(g, c, &front, t, k as f32 * fh + 0.12, (ms * 0.7).min(2.6), 0.7, tr, rail));
        }
    }
    for fc in [b.left(), b.right(), b.back()] {
        let sea_side = water_behind(c) && fc.out.y > 0.6;
        let mm = ((fc.len - 1.0) / 3.2).floor() as i32;
        for k in 0..body {
            for i in 0..mm.max(0) {
                let t = -(fc.len - 1.0) * 0.5 + (i as f32 + 0.5) * (fc.len - 1.0) / mm as f32;
                sash(g, c, rng, &fc, t, k as f32 * fh + fh * 0.5, 1.0, fh * 0.55, None);
                if sea_side && k > 0 {
                    g.lod(Lod::Mid, |g| balcony(g, c, &fc, t, k as f32 * fh + 0.12, 2.2, 0.8, tr, rail));
                }
            }
        }
        grime(g, c, rng, &fc, 0.0, h, 0.3 + 1.4 * c.wear);
    }
    grime(g, c, rng, &front, fh, h, 0.3 + 1.4 * c.wear);
    cornice(g, c, &b.poly(), h - 0.35, 0.35, 0.1, Concrete, tr);
    // The mansard top floor: zinc sloping in, dormers on the street.
    let top = b.inset(0.8);
    let zinc = mul(ROOFS[3], 1.0 - 0.2 * c.wear);
    loft(g, f, Metal, &b.inset(0.1).poly(), &top.poly(), h, h + fh, zinc);
    let tf = top.front();
    let dn = ((tf.len - 2.0) / 3.4).floor() as i32;
    for i in 0..dn.max(0) {
        let t = -(tf.len - 2.0) * 0.5 + (i as f32 + 0.5) * (tf.len - 2.0) / dn as f32;
        tf.bx(g, f, Metal, t, h + fh * 0.5, 0.25, Vec3::new(1.4, 1.8, 0.9), zinc);
        let df = Face::new(tf.at(t - 0.7, 0.7), tf.at(t + 0.7, 0.7), tf.out);
        sash(g, c, rng, &df, 0.0, h + fh * 0.48, 0.8, 1.1, None);
    }
    // The roof terrace on the flat over the mansard.
    let rt = top.inset(0.6);
    if rt.w() > 3.0 && rt.d() > 3.0 {
        let y = h + fh;
        parapet(g, c, &top.poly(), y, 0.6, 0.15, Metal, mul(zinc, 0.9));
        g.lod(Lod::Mid, |g| {
            table(g, f, Vec2::new(rt.u0 + 1.2, rt.c().y), y, mul(pal.wood, 0.75), Some(STRIPES[0]));
            table(g, f, Vec2::new(rt.u1 - 1.2, rt.c().y), y, mul(pal.wood, 0.75), Some(STRIPES[1]));
        });
        bulbs(g, c, rng, p3(rt.u0, y + 2.3, rt.v1), p3(rt.u1, y + 2.3, rt.v1), 0.35, 1.0);
        flagpole(g, f, Vec2::new(rt.c().x, rt.v0 + 0.3), 5.0, mul(BRIGHT[(c.seed % 6) as usize], 1.0 - 0.3 * c.wear), false);
    }
    if water_behind(c) {
        small_pier(g, c, rng, e, Some(Vec2::new(b.c().x, b.v1 + 0.5)));
    }
}

// ── small industry ────────────────────────────────────────────────────────────

/// The fish market hall: a long painted-iron hall with a glazed clerestory
/// along its ridge, roller doors up on a lit, wet floor, a big fish over
/// the door, crates of ice and pallets on the apron, bulbs along the eaves,
/// a quay and a pier behind on the water.
pub fn fish_market_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 8.0 || area.d() < 9.0 {
        return net_sheds(g, c, rng, e);
    }
    yard(g, c, 0.3, Concrete, mul(pick(rng, &pal.concrete), 0.7));
    let hw = (area.w() - 2.0).min(40.0);
    let apron = (area.d() * 0.25).clamp(3.0, 8.0);
    let hd = (area.d() - apron - 1.0).min(22.0);
    let Some(r) = fit(c, R::new(area.c().x - hw * 0.5, area.c().x + hw * 0.5, area.v0 + apron, area.v0 + apron + hd), 5.0) else { return net_sheds(g, c, rng, e) };
    let h = 6.0;
    let iron = mul(mix(SHUTTERS[1], SALT, 0.25), 1.0 - 0.25 * c.wear);
    let tr = trim(c);
    // A brick plinth, painted iron and boards above it.
    block(g, c, Brick, &r, 0.0, 1.2, mul(pal.brick(0), 0.75));
    block(g, c, Panel, &r, 1.2, h, iron);
    let rise = (r.d() * 0.22).clamp(2.0, 4.0);
    gabled(g, c, &r, h, rise, true, 0.5, Metal, roof_col(c, rng), Panel, iron);
    // The clerestory along the ridge.
    let cw = r.w() * 0.8;
    let cl = R::new(r.c().x - cw * 0.5, r.c().x + cw * 0.5, r.c().y - 1.3, r.c().y + 1.3);
    block(g, c, Glass, &cl, h + rise * 0.55, h + rise + 0.6, mul(pal.glass_dark, 1.3));
    loft(g, f, Metal, &cl.inset(-0.3).poly(), &R::new(cl.u0 - 0.3, cl.u1 + 0.3, cl.c().y - 0.05, cl.c().y + 0.05).poly(), h + rise + 0.6, h + rise + 1.3, roof_col(c, rng));
    let front = r.front();
    let doors = ((front.len / 6.5).floor() as i32).clamp(1, 5);
    let dw = (front.len / doors as f32 - 1.6).min(4.5);
    for i in 0..doors {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * front.len / doors as f32;
        if coin(rng, 0.7 - 0.3 * c.wear) {
            // Up: the lit floor inside.
            front.glaze(g, c, t, 1.7, PANE, dw, 3.4, Pane::Lit(mul([1.2, 1.25, 1.2], 0.6)));
            front.bx(g, f, Matte, t, 3.55, 0.08, Vec3::new(dw + 0.4, 0.3, 0.14), CHARCOAL);
        } else {
            roller_door(g, c, &front, t, dw, 3.4, mul(pick(rng, &pal.metal_paint), 0.75));
        }
    }
    // The name and the fish over the middle door.
    sign_board(g, c, rng, &front, 0.0, h - 0.8, (front.len * 0.4).min(10.0), 0.7);
    let sign = mul(pick(rng, &pal.sign_colors), 1.0 - 0.4 * c.wear);
    // Standing on the eave.
    fish_sign(g, c, &front, 0.0, h + 0.38, 0.4, 1.1, sign);
    for face in [r.left(), r.right(), r.back()] {
        pane_row(g, c, rng, &face, 2.6, 1, 3.0, 4.0, 2.4, 1.2, false);
        grime(g, c, rng, &face, 0.0, h, 0.6 + 1.6 * c.wear);
    }
    grime(g, c, rng, &front, 0.0, h, 0.6 + 1.6 * c.wear);
    // Bulbs along the street eave.
    let (a, b) = (front.at(-front.len * 0.5 + 0.3, 0.6), front.at(front.len * 0.5 - 0.3, 0.6));
    bulbs(g, c, rng, p3(a.x, h - 0.3, a.y), p3(b.x, h - 0.3, b.y), 0.0, 1.2);
    g.lod(Lod::Mid, |g| front.bx(g, f, Concrete, 0.0, h - 0.1, 0.3, Vec3::new(front.len, 0.2, 0.6), tr));
    // The apron: crates of ice, pallets, an ice machine.
    let ap = R::new(r.u0, r.u1, area.v0 + 0.5, r.v0 - 0.6);
    if ap.d() > 1.5 {
        let k = ((ap.w() - 2.0) / 3.5).floor() as i32;
        for i in 0..k.clamp(0, 6) {
            let p = Vec2::new(ap.u0 + 1.5 + i as f32 * 3.5, ap.c().y);
            fish_crates(g, c, rng, p, 1 + (i % 3));
        }
        let ice = Vec2::new(ap.u1 - 1.0, ap.c().y);
        if c.inside_local(ice) && distance_to_boundary(ice, &c.lot_poly) > 1.0 {
            f.bx(g, Metal, ice.x, 1.1, ice.y, Vec3::new(1.4, 2.2, 1.2), [0.80, 0.82, 0.84]);
        }
    }
    // A quay along the back on the water, crates and a pier.
    if water_behind(c) {
        let q = R::new(r.u0, r.u1, r.v1 + 0.2, area.v1 - 0.2);
        if q.d() > 1.0 {
            g.lod(Lod::Mid, |g| {
                for i in 0..((q.w() / 6.0).floor() as i32).clamp(0, 6) {
                    let p = Vec2::new(q.u0 + 3.0 + i as f32 * 6.0, q.v1 - 0.4);
                    if c.inside_local(p) {
                        g.part(Matte).cylinder(f.to_world(p.x, p.y), 0.16, 0.13, 0.0, 0.6, 6, rgba(DARK));
                    }
                }
            });
        }
        small_pier(g, c, rng, e, None);
    }
}

/// Net sheds: a row of tall, narrow, tar-black timber sheds (two or three
/// lofts high under steep gables, doors at the foot, a loft door at the
/// top), nets hung to dry on poles behind them, pots, buoys and crates,
/// one bulb over each door; a pier off the back on the water.
pub fn net_sheds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 3.6 || area.d() < 4.0 {
        return hut(g, c, rng);
    }
    yard(g, c, 0.3, Dirt, mul(pal.paving, 0.6));
    let sw = rng.random_range(3.0..3.8);
    let gap = rng.random_range(0.9..1.6);
    let n = (((area.w() + gap) / (sw + gap)).floor() as i32).clamp(1, 8);
    let total = n as f32 * sw + (n - 1) as f32 * gap;
    let u0 = area.c().x - total * 0.5;
    let mut last = f32::MIN;
    for i in 0..n {
        let u = u0 + i as f32 * (sw + gap);
        let sd = rng.random_range(3.8f32..5.4).min(area.d() - 1.0);
        let v0 = area.v0 + rng.random_range(0.4..1.4);
        let Some(r) = fit(c, R::new(u, u + sw, v0, v0 + sd), 2.4) else { continue };
        // A shed the lot's edge pushed into the last one is left out.
        if r.u0 < last + 0.6 {
            continue;
        }
        last = r.u1;
        let lofts = rng.random_range(2..=3);
        let h = lofts as f32 * 2.6;
        let col = mul(TAR, 1.0 + 0.6 * c.wear + rng.random_range(0.0..0.3));
        block(g, c, Wood, &r, 0.0, h, col);
        gabled(g, c, &r, h, (r.w() * 0.55).clamp(1.4, 2.4), false, 0.25, Wood, mul(col, 0.85), Wood, col);
        let front = r.front();
        // Boards, the door with its bulb, the loft door.
        g.lod(Lod::Near, |g| {
            let k = (front.len / 0.3).floor() as i32;
            for j in 1..k {
                front.decal(g, f, Matte, -front.len * 0.5 + j as f32 * 0.3, h * 0.5, 0.012, 0.03, h - 0.1, mul(col, 0.6));
            }
        });
        let dc = mul(SHUTTERS[rng.random_range(0..SHUTTERS.len())], 0.8 - 0.3 * c.wear);
        front.decal(g, f, Wood, 0.0, 1.05, 0.04, 1.1, 2.1, dc);
        front.decal(g, f, Wood, 0.0, h - 1.0, 0.04, 0.9, 1.2, dc);
        if coin(rng, 0.4) {
            front.glaze(g, c, 0.0, h - 2.4, PANE, 0.5, 0.5, if coin(rng, c.lit_chance()) { Pane::Lit(mul(INSIDE, 0.6)) } else { Pane::Dark });
        }
        let from = g.part(Glow).positions.len();
        front.bx(g, f, Glow, 0.0, 2.45, 0.18, Vec3::new(0.16, 0.2, 0.16), mul(BULB, 0.8));
        mark_glow(g, from, NIGHT_MARK);
        g.lod(Lod::Mid, |g| front.bx(g, f, Wood, 0.0, h + 0.4, 0.45, Vec3::new(0.16, 0.16, 0.9), mul(pal.wood, 0.4)));
        // Nets drying on two poles behind it.
        let nv = r.v1 + 1.4;
        let (pa, pb) = (Vec2::new(r.u0, nv), Vec2::new(r.u1, nv));
        if c.inside_local(pa) && c.inside_local(pb) && distance_to_boundary(pa, &c.lot_poly) > 0.4 && distance_to_boundary(pb, &c.lot_poly) > 0.4 {
            for p in [pa, pb] {
                f.bx(g, Wood, p.x, 1.6, p.y, Vec3::new(0.12, 3.2, 0.12), mul(pal.wood, 0.5));
            }
            let net = mul(NETS[rng.random_range(0..NETS.len())], 1.0 - 0.3 * c.wear);
            g.lod(Lod::Mid, |g| {
                f.bx(g, Tarp, r.c().x, 2.0, nv, Vec3::new(r.w() - 0.3, 2.0, 0.04), net);
            });
        }
    }
    harbour_clutter(g, c, rng, Vec2::new(area.u0 + 1.4, area.v1 - 1.6));
    harbour_clutter(g, c, rng, Vec2::new(area.u1 - 2.4, area.v1 - 1.6));
    if water_behind(c) {
        small_pier(g, c, rng, e, None);
    }
}

// ── the big house ─────────────────────────────────────────────────────────────

/// A family's or a billionaire's pastel waterfront villa: a stucco house
/// of two storeys (three on an estate) under a hipped roof, a columned
/// veranda with a balcony over it, shuttered windows all round, a
/// lighthouse belvedere on an estate; a white wall and gate to the street;
/// a terrace, a lawn and a pool behind, palms; a boathouse at the back
/// with a pier off it on the water; bulbs round the terrace.
pub fn pastel_villa(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(site) = fit(c, R::of(&offset_poly(&c.lot_poly, 0.3)), 12.0).map(|r| r.inset(1.0)) else { return pastel_terrace(g, c, rng, e) };
    let big = site.w() * site.d() > 1300.0;
    let fh = 3.4;
    let storeys = if big { 3 } else { 2 };
    let h = storeys as f32 * fh;
    let col = pastel(c, rng);
    let tr = trim(c);
    let shut = shutter(c, rng);
    // The lawn over the lot.
    ground(g, f, Foliage, &offset_poly(&c.lot_poly, 0.4), 0.03, mul(pal.foliage, 0.75 - 0.2 * c.wear));
    let vw = (site.w() * if big { 0.6 } else { 0.62 }).min(if big { 30.0 } else { 22.0 });
    let vd = (site.d() * 0.38).clamp(8.0, if big { 16.0 } else { 13.0 });
    let v0 = site.v0 + (site.d() * 0.18).clamp(3.5, 9.0);
    let house = R::new(site.c().x - vw * 0.5, site.c().x + vw * 0.5, v0, v0 + vd);
    if house.v1 > site.v1 - 4.0 {
        return pastel_terrace(g, c, rng, e);
    }
    block(g, c, Plaster, &house, 0.0, h, col);
    cornice(g, c, &house.poly(), h - 0.35, 0.35, 0.1, Concrete, tr);
    for k in 1..storeys {
        cornice(g, c, &house.poly(), k as f32 * fh - 0.1, 0.2, 0.06, Concrete, tr);
    }
    hipped(g, c, &house, h, (house.d() * 0.28).clamp(2.0, 4.0), 0.7, Panel, roof_col(c, rng));
    for face in c.faces(&house.poly()) {
        let n = ((face.len - 1.0) / 2.8).floor() as i32;
        for k in 0..storeys {
            for i in 0..n.max(0) {
                let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * (face.len - 1.0) / n as f32;
                if face.is_front && k == 0 && (t.abs() < 1.0) {
                    door(g, c, rng, &face, t, shut);
                    continue;
                }
                sash(g, c, rng, &face, t, k as f32 * fh + fh * 0.5, 1.1, fh * 0.56, Some(shut));
            }
        }
        grime(g, c, rng, &face, 0.0, h, 0.2 + 1.2 * c.wear);
    }
    // The veranda: a deck, white columns, a roof that is the balcony over it.
    let vr = R::new(house.u0 + 0.6, house.u1 - 0.6, house.v0 - 2.6, house.v0);
    if let Some(vr) = fit(c, vr, 1.5) {
        block(g, c, Wood, &vr, 0.0, 0.3, mul(pal.wood, 0.9));
        let fr = vr.front();
        let cols = ((fr.len / 2.8).floor() as i32).max(1);
        for i in 0..=cols {
            let t = -fr.len * 0.5 + 0.2 + i as f32 * (fr.len - 0.4) / cols as f32;
            fr.bx(g, f, Concrete, t, 0.3 + (fh - 0.5) * 0.5, -0.25, Vec3::new(0.3, fh - 0.5, 0.3), tr);
        }
        block(g, c, Concrete, &vr, fh - 0.2, fh + 0.05, tr);
        g.lod(Lod::Mid, |g| {
            fr.decal(g, f, Matte, 0.0, fh + 0.55, 0.02, fr.len - 0.1, 0.9, tr);
        });
        let (a, b) = (fr.at(-fr.len * 0.5 + 0.2, -0.25), fr.at(fr.len * 0.5 - 0.2, -0.25));
        bulbs(g, c, rng, p3(a.x, fh - 0.4, a.y), p3(b.x, fh - 0.4, b.y), 0.3, 0.8);
    }
    // The estate's belvedere: a lighthouse at the house's corner.
    if big {
        let p = Vec2::new(house.u1 - 1.8, house.v1 - 1.8);
        lighthouse(g, c, rng, p, 2.4, 0.0, h + fh * 1.3);
    }
    // A white wall and gate posts along the street.
    let wall_v = site.v0 + 0.2;
    let gw = 4.0;
    for (u0, u1) in [(site.u0, site.c().x - gw * 0.5), (site.c().x + gw * 0.5, site.u1)] {
        if u1 - u0 > 0.8 {
            f.bx(g, Plaster, (u0 + u1) * 0.5, 0.6, wall_v, Vec3::new(u1 - u0, 1.2, 0.3), tr);
        }
    }
    for s in [-1.0f32, 1.0] {
        let u = site.c().x + s * (gw * 0.5 + 0.2);
        f.bx(g, Plaster, u, 0.95, wall_v, Vec3::new(0.5, 1.9, 0.5), mul(tr, 0.95));
        lamp_post(g, c, Vec2::new(u, wall_v + 0.6), 2.4);
    }
    ground(g, f, Paving, &R::new(site.c().x - gw * 0.5 + 0.4, site.c().x + gw * 0.5 - 0.4, wall_v + 0.2, house.v0 - 2.7).poly(), 0.05, mul(pal.paving, 1.1));
    // Behind: the terrace, the pool, palms, bulbs round the terrace.
    let back = R::new(house.u0, house.u1, house.v1, (house.v1 + 6.0).min(site.v1));
    if back.d() > 2.0 {
        ground(g, f, Paving, &back.poly(), 0.06, mul(pal.paving, 1.15));
        let posts = [Vec2::new(back.u0 + 0.3, back.v1 - 0.3), Vec2::new(back.u1 - 0.3, back.v1 - 0.3)];
        for p in posts {
            f.bx(g, Wood, p.x, 1.4, p.y, Vec3::new(0.12, 2.8, 0.12), tr);
        }
        bulbs(g, c, rng, p3(posts[0].x, 2.7, posts[0].y), p3(posts[1].x, 2.7, posts[1].y), 0.5, 1.0);
        bulbs(g, c, rng, p3(back.u0 + 0.3, h * 0.45, back.v0 + 0.1), p3(posts[0].x, 2.7, posts[0].y), 0.3, 1.0);
        bulbs(g, c, rng, p3(back.u1 - 0.3, h * 0.45, back.v0 + 0.1), p3(posts[1].x, 2.7, posts[1].y), 0.3, 1.0);
        for i in 0..((back.w() / 4.0).floor() as i32).clamp(0, 5) {
            let p = Vec2::new(back.u0 + 2.0 + i as f32 * 4.0, back.c().y);
            g.lod(Lod::Mid, |g| table(g, f, p, 0.06, mul(pal.wood, 0.8), (i % 2 == 0).then_some(STRIPES[(i as usize) % STRIPES.len()])));
        }
    }
    let pool_v = back.v1 + 3.5;
    if pool_v + 3.0 < site.v1 - 2.0 {
        pool(g, c, site.c().x - site.w() * 0.15, pool_v, (site.w() * 0.3).clamp(5.0, 12.0), 4.0);
    }
    for i in 0..if big { 4 } else { 2 } {
        let p = Vec2::new(if i % 2 == 0 { site.u0 + 1.5 } else { site.u1 - 1.5 }, site.v0 + 3.0 + (i / 2) as f32 * (site.d() * 0.5));
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 3.0 {
            tree_kind(g, f.to_world(p.x, p.y), 0.0, TreeKind::Palm, rng.random_range(5.0..7.5), 3.2, pal.bark, mul(pal.foliage, 1.05));
        }
    }
    // The boathouse at the back.
    let bw = (site.w() * 0.3).clamp(5.0, 9.0);
    let bd = (site.d() * 0.22).clamp(5.0, 10.0);
    let bu = site.u1 - bw * 0.5 - 1.0;
    let br = R::new(bu - bw * 0.5, bu + bw * 0.5, site.v1 - bd, site.v1);
    if br.v0 > back.v1 + 1.0 {
        if let Some(br) = fit(c, br, 3.5) {
            boathouse_shed(g, c, rng, &br, 1);
            if water_behind(c) {
                small_pier(g, c, rng, e, Some(Vec2::new(br.c().x, br.v1 - 0.5)));
            }
            return;
        }
    }
    if water_behind(c) {
        small_pier(g, c, rng, e, None);
    }
}

/// [r4-harbour] The style a trade builds in a Harbour Pastel district,
/// where the look has one for it: a hotel is a boutique hotel, a seafood
/// restaurant a seafood shack, a fish market the market hall.
pub fn trade_style(kind: Option<nd_sim::BusinessKind>) -> Option<super::Style> {
    use nd_sim::BusinessKind as B;
    use super::Style;
    match kind? {
        B::Hotel => Some(Style::BoutiqueHotel),
        B::SeafoodRestaurant => Some(Style::SeafoodShack),
        B::FishMarket => Some(Style::FishMarketHall),
        _ => None,
    }
}

use super::hk;
