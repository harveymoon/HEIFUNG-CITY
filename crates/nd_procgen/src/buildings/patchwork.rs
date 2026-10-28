//! [r4-patchwork] Patchwork, the Riverbank Shantytown's look: the homes
//! nobody planned. Corrugated-iron shacks grown room by room round swept
//! dirt yards, plank houses up on piles by the water, self-built storeys
//! stacked on an old concrete base under water tanks; a market of stalls
//! under tarps, corner kiosks with a lit fridge and a hand-painted sign, a
//! tin-roofed shebeen with string lights; a scrap yard and workshops under
//! lean-tos; and for the one who made it, the big man's walled compound:
//! a bright-painted concrete house behind a wall and razor wire, a dish
//! and a generator, a car in the yard, and still a tin room out the back.
//!
//! Irregular, patched and lived in, never a joke: every room and sheet is
//! the seed's own (sizes, slopes, which way a roof falls), walls are
//! patched with rusted, painted and new sheets side by side, roofs are held
//! down with stones and tyres; laundry, dishes and tangled wires from a
//! leaning pole. After dark: bare bulbs over doors and strung over lanes,
//! the blue of a television in a window, a fire in a barrel (base glow
//! marked `NIGHT_MARK`, lit only after dark). Small parts (wires, laundry,
//! dishes, bars, roof stones, bulbs' glass) go in the detail layers.
//!
//! On a lot by the water (`Ctx::shore`) the stilt houses stand on piles and
//! run a plank walk out over it to a tied-up canoe; inland they stand on a
//! low plinth. Everything comes from the building's seed.

use super::harbour::{bulbs, gabled};
use super::kit::*;
use super::parts::*;
use super::shore::sea_local;
use super::{Ctx, Face};
use crate::depth::PANE;
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Rusted sheet: orange-brown to dark.
const RUSTS: [[f32; 3]; 4] = [[0.44, 0.24, 0.14], [0.52, 0.31, 0.18], [0.36, 0.21, 0.14], [0.58, 0.38, 0.24]];
/// New galvanised sheet.
const ZINC: [[f32; 3]; 2] = [[0.56, 0.58, 0.58], [0.64, 0.66, 0.65]];
/// Painted sheet and render, faded: blue, teal, red, ochre, green, pink, cream.
const PAINTS: [[f32; 3]; 7] = [[0.22, 0.38, 0.54], [0.18, 0.46, 0.42], [0.60, 0.18, 0.14], [0.78, 0.62, 0.20], [0.30, 0.48, 0.24], [0.72, 0.42, 0.46], [0.84, 0.80, 0.70]];
/// Weathered planks.
const PLANKS: [[f32; 3]; 3] = [[0.46, 0.36, 0.26], [0.38, 0.30, 0.22], [0.52, 0.44, 0.34]];
/// What sun and dust fade paint towards.
const DUST: [f32; 3] = [0.55, 0.52, 0.47];
/// Raw concrete block.
const BLOCK: [f32; 3] = [0.56, 0.55, 0.52];
/// The big man's paint: pink, turquoise, lime, yellow, orange.
const LOUD: [[f32; 3]; 5] = [[0.90, 0.46, 0.62], [0.20, 0.68, 0.66], [0.60, 0.80, 0.28], [0.94, 0.78, 0.24], [0.92, 0.52, 0.20]];
/// Plastic water tanks: black, blue, green, white.
const TANKS: [[f32; 3]; 4] = [[0.10, 0.10, 0.11], [0.12, 0.30, 0.62], [0.20, 0.36, 0.24], [0.82, 0.82, 0.78]];
/// Tarps: blue, orange, green, red, faded white.
const TARPS: [[f32; 3]; 5] = [[0.14, 0.30, 0.60], [0.82, 0.44, 0.12], [0.22, 0.46, 0.26], [0.62, 0.16, 0.14], [0.80, 0.80, 0.74]];
/// Washing on the line.
const CLOTHES: [[f32; 3]; 6] = [[0.85, 0.85, 0.82], [0.20, 0.36, 0.66], [0.80, 0.24, 0.20], [0.92, 0.76, 0.22], [0.26, 0.56, 0.36], [0.70, 0.40, 0.62]];
/// A bare bulb after dark.
const BULB: [f32; 3] = [2.4, 1.55, 0.7];
/// A television through a window.
const TV: [f32; 3] = [0.5, 0.8, 1.7];
/// Fire in a barrel.
const FIRE: [f32; 3] = [2.4, 1.0, 0.3];
/// A drinks fridge lit inside.
const FRIDGE: [f32; 3] = [1.4, 1.7, 1.9];

/// Lot-local (u, y, v).
fn p3(u: f32, y: f32, v: f32) -> Vec3 {
    Vec3::new(u, y, v)
}

fn any<const N: usize>(rng: &mut impl Rng, list: &[[f32; 3]; N]) -> [f32; 3] {
    list[rng.random_range(0..N)]
}

/// One sheet of the patchwork: rusted (more of them on a run-down lot),
/// new zinc, or painted and faded.
fn sheet(c: &Ctx, rng: &mut impl Rng) -> (Surface, [f32; 3]) {
    let k: f32 = rng.random_range(0.0..1.0);
    let rusty = 0.35 + 0.35 * c.wear;
    if k < rusty {
        (Rust, mul(any(rng, &RUSTS), rng.random_range(0.85..1.1)))
    } else if k < rusty + 0.22 {
        (Metal, mul(any(rng, &ZINC), 1.0 - 0.2 * c.wear))
    } else {
        (Metal, faded(c, rng))
    }
}

/// A painted colour, faded by dust and darkened with wear.
fn faded(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(mix(any(rng, &PAINTS), DUST, 0.3 + 0.35 * c.wear), 0.85 - 0.2 * c.wear)
}

/// Patches on a wall: one to three sheets of other stuff nailed over it,
/// each its own size and height, each at its own depth so overlapping ones never
/// share a plane.
fn patches(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32) {
    let f = &c.frame;
    let h = y1 - y0;
    if face.len < 1.4 || h < 0.8 {
        return;
    }
    let n = rng.random_range(1..=3).min((face.len / 1.2) as i32);
    for k in 0..n {
        let w = rng.random_range(0.8..(face.len * 0.5).clamp(0.9, 2.2));
        let t = rng.random_range(-(face.len - w) * 0.5..=(face.len - w) * 0.5);
        let ph = rng.random_range(h * 0.35..h * 0.9);
        let py = y0 + rng.random_range(0.05..(h - ph).max(0.06)) + ph * 0.5;
        let (s, col) = sheet(c, rng);
        face.decal(g, f, s, t, py, 0.012 + 0.0055 * k as f32, w, ph, col);
    }
}

/// A mono-pitch sheet roof over a rectangle, falling one of four ways,
/// eaves out `over`; stones and a tyre on it to hold it down (near
/// detail). Returns the top of the roof.
fn lean_roof(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32, over: f32) -> f32 {
    let f = &c.frame;
    let rise = rng.random_range(0.2..0.45);
    let (s, col) = sheet(c, rng);
    let col = mul(col, 0.9);
    let way = rng.random_range(0..4);
    let (a, b, w) = match way {
        0 => (p3(r.c().x, y + rise, r.v0 - over), p3(r.c().x, y, r.v1 + over), r.w() + 2.0 * over),
        1 => (p3(r.c().x, y + rise, r.v1 + over), p3(r.c().x, y, r.v0 - over), r.w() + 2.0 * over),
        2 => (p3(r.u0 - over, y + rise, r.c().y), p3(r.u1 + over, y, r.c().y), r.d() + 2.0 * over),
        _ => (p3(r.u1 + over, y + rise, r.c().y), p3(r.u0 - over, y, r.c().y), r.d() + 2.0 * over),
    };
    // A centimetre up: the slab's underside clears the walls' tops.
    g.part(s).slab(f.at(a.x, a.y + 0.07, a.z), f.at(b.x, b.y + 0.07, b.z), w, 0.06, rgba(col));
    g.lod(Lod::Near, |g| {
        for _ in 0..rng.random_range(1..4) {
            let t: f32 = rng.random_range(0.2..0.8);
            let q = a.lerp(b, t);
            let side = rng.random_range(-0.35..0.35) * w;
            let (du, dv) = if way < 2 { (side, 0.0) } else { (0.0, side) };
            if coin(rng, 0.3) {
                g.part(Matte).cylinder(f.to_world(q.x + du, q.z + dv), 0.32, 0.32, q.y + 0.08, q.y + 0.24, 8, rgba(DARK));
            } else {
                f.bx(g, Concrete, q.x + du, q.y + 0.16, q.z + dv, Vec3::new(0.28, 0.18, 0.24), mul(BLOCK, rng.random_range(0.7..1.0)));
            }
        }
    });
    y + rise + 0.07
}

/// A window: dark or lit glass (some with a television's blue after dark),
/// a rough frame (mid) and, on the meaner ones, a cloth for a curtain.
#[allow(clippy::too_many_arguments)]
fn hole_window(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    if coin(rng, 0.22) {
        tv(g, c, face, t, y, w, h);
    } else if coin(rng, c.lit_chance()) {
        let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.85));
        face.glaze(g, c, t, y, PANE, w, h, Pane::Lit(col));
    } else {
        face.glaze(g, c, t, y, PANE, w, h, Pane::Dark);
    }
    g.lod(Lod::Mid, |g| {
        face.window_frame(g, c, t, y, w, h, 0.12, mul(c.palette.wood, 0.6));
        if coin(rng, 0.4) {
            face.decal(g, f, Tarp, t - w * 0.3, y + h * 0.05, PANE + 0.024, w * 0.35, h * 0.85, any(rng, &CLOTHES));
        }
    });
}

/// A window with a television on behind it: dark glass by day, its blue
/// light after dark only.
fn tv(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    face.glaze(g, c, t, y, PANE, w, h, Pane::Dark);
    let from = g.part(Glow).positions.len();
    face.decal(g, &c.frame, Glow, t, y - h * 0.1, PANE + 0.012, w * 0.6, h * 0.55, mul(TV, 1.0 - 0.3 * c.wear));
    mark_glow(g, from, NIGHT_MARK);
}

/// A bare bulb on its flex out from a wall: the flex mid detail, the
/// glass near detail, its light base glow lit only after dark.
fn bare_bulb(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32) {
    let f = &c.frame;
    g.lod(Lod::Mid, |g| face.bx(g, f, Matte, t, y + 0.12, 0.14, Vec3::new(0.03, 0.03, 0.28), DARK));
    g.lod(Lod::Near, |g| face.bx(g, f, Matte, t, y, 0.28, Vec3::splat(0.07), [0.86, 0.84, 0.74]));
    let from = g.part(Glow).positions.len();
    // A touch big for a bulb, so a lit doorway reads from up the street.
    face.bx(g, f, Glow, t, y - 0.02, 0.28, Vec3::splat(0.19), mul(BULB, 1.25 - 0.25 * c.wear));
    mark_glow(g, from, NIGHT_MARK);
}

/// A door: planks, sheet or a cloth over the opening.
fn doorway(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32) {
    let f = &c.frame;
    let (s, col) = match rng.random_range(0..3) {
        0 => (Wood, mul(any(rng, &PLANKS), 0.8)),
        1 => (Tarp, any(rng, &CLOTHES)),
        _ => sheet(c, rng),
    };
    // Proud of the patches (at most 2.3 cm).
    face.decal(g, f, s, t, y0 + 0.95, 0.036, 0.8, 1.9, mul(col, 0.85));
}

/// Is the rectangle on the lot, clear of the street side's setback?
fn on_lot(c: &Ctx, r: &R) -> bool {
    poly_inside(c, &r.poly())
}

/// Is lot-local `p` on the lot at least `m` in?
fn spot(c: &Ctx, p: Vec2, m: f32) -> bool {
    c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) >= m
}

/// One room of sheet (or ply) on a rectangle from `y0`, `h` high: patched
/// walls, a door on the face `door` (front 0, right 1, back 2, left 3) if
/// any, a window or two, its own lean roof (`roof`) or a flat lid to build
/// on. Returns the top.
#[allow(clippy::too_many_arguments)]
fn room(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y0: f32, h: f32, door: Option<usize>, roof: bool) -> f32 {
    let (s, col) = if coin(rng, 0.2) { (Wood, mul(c.palette.plywood, rng.random_range(0.7..0.95))) } else { sheet(c, rng) };
    block(g, c, s, r, y0, y0 + h, col);
    for (i, face) in r.faces().iter().enumerate() {
        patches(g, c, rng, face, y0, y0 + h);
        if door == Some(i) && face.len > 1.4 {
            let t = rng.random_range(-0.25..0.25) * (face.len - 1.0);
            doorway(g, c, rng, face, t, y0);
            if coin(rng, 0.7) {
                bare_bulb(g, c, face, t + 0.65, y0 + 2.05);
            }
            if face.len > 3.0 {
                let wt = if t > 0.0 { t - 1.3 } else { t + 1.3 };
                hole_window(g, c, rng, face, wt, y0 + 1.4, 0.7, 0.55);
            }
        } else if face.len > 2.2 && coin(rng, 0.45) {
            let t = rng.random_range(-0.2..0.2) * face.len;
            hole_window(g, c, rng, face, t, y0 + 1.45, 0.6, 0.5);
        }
    }
    if roof {
        lean_roof(g, c, rng, r, y0 + h, 0.25)
    } else {
        // A slab lid, a little over the walls, to build the next room on.
        block(g, c, Concrete, &R::new(r.u0 - 0.05, r.u1 + 0.05, r.v0 - 0.05, r.v1 + 0.05), y0 + h, y0 + h + 0.12, mul(BLOCK, 0.8));
        y0 + h + 0.12
    }
}

/// A plastic water tank at lot-local `p` standing on `y`, lid detail mid.
fn water_tank(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32, r: f32, h: f32) {
    let f = &c.frame;
    let col = mul(any(rng, &TANKS), 1.0 - 0.25 * c.wear);
    let w = f.to_world(p.x, p.y);
    g.part(Panel).cylinder_capped(w, r, r * 0.96, y, y + h, 10, false, true, rgba(col));
    g.lod(Lod::Mid, |g| g.part(Panel).cylinder_capped(w, r * 0.35, r * 0.35, y + h, y + h + 0.08, 8, false, true, rgba(mul(col, 0.8))));
}

/// A water tank up on a welded stand beside a house.
fn tank_stand(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, h: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.rust, 0.8);
    for (du, dv) in [(-0.55f32, -0.55f32), (0.55, -0.55), (0.55, 0.55), (-0.55, 0.55)] {
        f.bx(g, Rust, p.x + du, h * 0.5, p.y + dv, Vec3::new(0.08, h, 0.08), steel);
    }
    f.bx(g, Rust, p.x, h - 0.05, p.y, Vec3::new(1.3, 0.1, 1.3), steel);
    g.lod(Lod::Mid, |g| {
        let (a, b) = (f.at(p.x - 0.55, 0.2, p.y - 0.58), f.at(p.x + 0.55, h - 0.2, p.y - 0.58));
        g.part(Rust).beam(a, b, 0.05, rgba(steel));
    });
    water_tank(g, c, rng, p, h, 0.6, 1.1);
}

/// A satellite dish on a short post at lot-local `p` on `y` (mid detail).
fn small_dish(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32) {
    let col = mul([0.80, 0.80, 0.78], 1.0 - 0.3 * c.wear);
    let r = rng.random_range(0.35..0.55);
    g.lod(Lod::Mid, |g| dish(g, &c.frame, p, y, r, col));
}

/// A washing line between lot-local anchors: the line and the washing on
/// it (mid detail), a few gaps.
fn laundry(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec3, b: Vec3) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 1.2 {
        return;
    }
    let (wa, wb) = (f.at(a.x, a.y, a.z), f.at(b.x, b.y, b.z));
    let d = wb - wa;
    let yaw = d.z.atan2(d.x);
    g.lod(Lod::Mid, |g| {
        g.part(Matte).wire(wa, wb, 0.12, 0.02, rgba(CHARCOAL));
        let n = (len / 0.65) as i32;
        for i in 1..n {
            if coin(rng, 0.3) {
                continue;
            }
            let t = i as f32 / n as f32;
            let gh = rng.random_range(0.4..0.8);
            let p = wa.lerp(wb, t) - Vec3::Y * (0.12 * 4.0 * t * (1.0 - t) + gh * 0.5 + 0.02);
            let col = mul(any(rng, &CLOTHES), 1.0 - 0.2 * c.wear);
            g.part(Tarp).boxed(p, Vec3::new(rng.random_range(0.35..0.55), gh, 0.03), yaw, rgba(col));
        }
    });
}

/// A leaning timber pole with a crossbar, and wires from its top to each
/// of `to` (lot-local), sagging and doubled up here and there (mid).
fn pole_and_wires(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, h: f32, to: &[Vec3]) {
    let f = &c.frame;
    let lean = Vec2::new(rng.random_range(-0.25..0.25), rng.random_range(-0.25..0.25));
    let top = p3(p.x + lean.x, h, p.y + lean.y);
    g.part(Wood).beam(f.at(p.x, 0.0, p.y), f.at(top.x, top.y, top.z), 0.2, rgba(mul(c.palette.wood, 0.45)));
    g.part(Wood).beam(f.at(top.x - 0.7, h - 0.4, top.z), f.at(top.x + 0.7, h - 0.4, top.z), 0.1, rgba(mul(c.palette.wood, 0.4)));
    g.lod(Lod::Mid, |g| {
        let t = f.at(top.x, h - 0.45, top.z);
        for (k, q) in to.iter().enumerate() {
            let w = f.at(q.x, q.y, q.z);
            g.part(Matte).wire(t + f.dir3(Vec2::new(0.6 - 0.4 * (k % 4) as f32, 0.0)), w, rng.random_range(0.3..0.9), 0.025, rgba(DARK));
            if coin(rng, 0.35) {
                g.part(Matte).wire(t, w, rng.random_range(0.5..1.2), 0.02, rgba(DARK));
            }
        }
    });
}

/// A fire in an oil drum: the drum by day, its flames after dark.
fn fire_barrel(g: &mut Geometry, c: &Ctx, p: Vec2) {
    if !spot(c, p, 0.6) {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Rust).cylinder(w, 0.3, 0.3, 0.0, 0.9, 8, rgba(c.palette.rust));
    g.part(Matte).cylinder_capped(w, 0.28, 0.28, 0.72, 0.74, 8, false, true, rgba(DARK));
    let from = g.part(Glow).positions.len();
    g.part(Glow).boxed(Vec3::new(w.x, 0.95, w.y), Vec3::new(0.32, 0.38, 0.32), f.yaw() + 0.4, rgba(FIRE));
    mark_glow(g, from, NIGHT_MARK);
}

/// A run of sheets on posts from `a` to `b` (lot-local), each sheet its
/// own height, stuff and colour, a little out of line.
fn sheet_fence(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec2, b: Vec2, h: f32) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 0.8 {
        return;
    }
    let dir = (b - a) / len;
    let n = (len / 2.2).ceil() as i32;
    let step = len / n as f32;
    let yaw = dir.y.atan2(dir.x);
    for i in 0..n {
        let m = a + dir * (step * (i as f32 + 0.5));
        let sh = h * rng.random_range(0.85..1.1);
        let (s, col) = sheet(c, rng);
        // Each a little out of line with its neighbours (never flush).
        let jog = 0.012 * ((i % 3) as f32 - 1.0);
        let q = m + dir.perp() * jog;
        f.bx_yaw(g, s, q.x, sh * 0.5, q.y, Vec3::new(step + 0.05, sh, 0.04), col, yaw);
    }
    g.lod(Lod::Mid, |g| {
        for i in 0..=n {
            let q = a + dir * (step * i as f32) + dir.perp() * 0.14;
            f.bx(g, Wood, q.x, h * 0.5, q.y, Vec3::new(0.09, h + 0.1, 0.09), mul(c.palette.wood, 0.5));
        }
    });
}

/// A hand-painted sign: a board, two or three colour fields and dark
/// strokes for its letters (near detail). Not lit.
#[allow(clippy::too_many_arguments)]
fn painted_sign(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, d: f32, w: f32, h: f32) {
    let f = &c.frame;
    let ground_col = mul(any(rng, &[[0.86, 0.84, 0.76], [0.92, 0.80, 0.22], [0.80, 0.20, 0.16], [0.20, 0.36, 0.62], [0.26, 0.52, 0.30]]), 1.0 - 0.3 * c.wear);
    face.bx(g, f, Wood, t, y, d, Vec3::new(w, h, 0.05), ground_col);
    let band = mul(any(rng, &[[0.10, 0.10, 0.10], [0.80, 0.18, 0.14], [0.96, 0.92, 0.84], [0.18, 0.30, 0.62]]), 1.0 - 0.25 * c.wear);
    face.decal(g, f, Panel, t, y + h * 0.28, d + 0.03, w * 0.92, h * 0.28, band);
    g.lod(Lod::Near, |g| {
        let n = ((w / 0.32) as i32).max(2);
        for k in 0..n {
            if coin(rng, 0.15) {
                continue;
            }
            let tt = t - w * 0.4 + w * 0.8 * (k as f32 + 0.5) / n as f32;
            face.decal(g, f, Matte, tt, y - h * 0.12, d + 0.035, w * 0.8 / n as f32 * 0.6, h * 0.32, DARK);
        }
    });
}

/// A plastic chair (near detail).
fn chair(g: &mut Geometry, c: &Ctx, p: Vec2, col: [f32; 3]) {
    let f = &c.frame;
    g.lod(Lod::Near, |g| {
        f.bx(g, Panel, p.x, 0.44, p.y, Vec3::new(0.44, 0.06, 0.44), col);
        f.bx(g, Panel, p.x, 0.68, p.y + 0.2, Vec3::new(0.44, 0.48, 0.05), col);
        f.bx(g, Panel, p.x, 0.21, p.y, Vec3::new(0.38, 0.42, 0.38), mul(col, 0.85));
    });
}

/// A household's plot in a cell: a first room, rooms added to it as the
/// family grew (lower, of whatever sheet there was), a room up on top of
/// it on a taller lot; a tank, a dish, washing. Pushes the rooms it built
/// and the eaves to wire up.
fn homestead(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, cell: &R, rooms: &mut Vec<R>, eaves: &mut Vec<Vec3>) {
    let f = &c.frame;
    let cw = rng.random_range(2.6..(cell.w() * 0.7).clamp(2.7, 4.4));
    let cd = rng.random_range(2.4..(cell.d() * 0.66).clamp(2.5, 4.0));
    let cu = rng.random_range(cell.u0 + cw * 0.5..=(cell.u1 - cw * 0.5).max(cell.u0 + cw * 0.5));
    let cv = rng.random_range(cell.v0 + cd * 0.5..=(cell.v1 - cd * 0.5).max(cell.v0 + cd * 0.5));
    let core = R::new(cu - cw * 0.5, cu + cw * 0.5, cv - cd * 0.5, cv + cd * 0.5);
    if !on_lot(c, &core) {
        return;
    }
    let h = rng.random_range(2.25..2.7);
    let up = c.floors >= 4 && coin(rng, 0.25 + 0.06 * c.floors as f32);
    let top = room(g, c, rng, &core, 0.0, h, Some(0), !up);
    rooms.push(core);
    eaves.push(p3(core.u0 + 0.2, h - 0.1, core.v0));
    // The topmost roof (its room and the middle of its slope).
    let (mut top_room, mut roof_y) = (core, (h + top) * 0.5);
    if up {
        // A room on the slab, set back, reached by a ladder.
        let ur = R::new(core.u0 + rng.random_range(0.0..0.5), core.u1 - rng.random_range(0.0..0.5), core.v0 + rng.random_range(0.2..0.6), core.v1 - rng.random_range(0.0..0.3));
        if ur.w() > 1.8 && ur.d() > 1.6 {
            let uh = rng.random_range(2.1..2.4);
            let ut = room(g, c, rng, &ur, top, uh, Some(0), true);
            eaves.push(p3(ur.u1 - 0.2, top + uh - 0.3, ur.v0));
            (top_room, roof_y) = (ur, (top + uh + ut) * 0.5);
            g.lod(Lod::Mid, |g| {
                let lu = core.u1 + 0.2;
                for s in [-0.22f32, 0.22] {
                    g.part(Wood).beam(f.at(lu, 0.0, core.c().y + s), f.at(lu, top + 0.9, core.c().y + s), 0.06, rgba(mul(c.palette.wood, 0.6)));
                }
                let mut y = 0.3;
                while y < top + 0.8 {
                    f.bx(g, Wood, lu, y, core.c().y, Vec3::new(0.05, 0.05, 0.44), mul(c.palette.wood, 0.6));
                    y += 0.35;
                }
            });
        }
    }
    // Rooms added on as the family grew.
    let adds = rng.random_range(1..=3) + (c.floors as i32 / 3).min(2);
    for _ in 0..adds {
        let side = rng.random_range(0..4);
        let (aw, ad): (f32, f32) = (rng.random_range(1.5..2.6), rng.random_range(1.6..2.8));
        let r = match side {
            0 => {
                let u = rng.random_range(core.u0..core.u1 - 0.8).min(core.u1 - aw.min(core.w()));
                R::new(u, u + aw.min(core.w()), core.v0 - ad, core.v0)
            }
            1 => {
                let v = rng.random_range(core.v0..core.v1 - 0.8).min(core.v1 - ad.min(core.d()));
                R::new(core.u1, core.u1 + aw, v, v + ad.min(core.d()))
            }
            2 => {
                let u = rng.random_range(core.u0..core.u1 - 0.8).min(core.u1 - aw.min(core.w()));
                R::new(u, u + aw.min(core.w()), core.v1, core.v1 + ad)
            }
            _ => {
                let v = rng.random_range(core.v0..core.v1 - 0.8).min(core.v1 - ad.min(core.d()));
                R::new(core.u0 - aw, core.u0, v, v + ad.min(core.d()))
            }
        };
        let inside_cell = r.u0 >= cell.u0 - 0.4 && r.u1 <= cell.u1 + 0.4 && r.v0 >= cell.v0 - 0.4 && r.v1 <= cell.v1 + 0.4;
        let clear = rooms.iter().all(|o| o == &core || r.u1 <= o.u0 + 0.01 || r.u0 >= o.u1 - 0.01 || r.v1 <= o.v0 + 0.01 || r.v0 >= o.v1 - 0.01);
        if r.w() < 1.2 || r.d() < 1.2 || !inside_cell || !clear || !on_lot(c, &r) {
            continue;
        }
        let door = (side != 2).then_some(side.min(3));
        let rh = h - rng.random_range(0.12..0.4);
        room(g, c, rng, &r, 0.0, rh, door, true);
        rooms.push(r);
    }
    // On the roof: a tank, a dish.
    if coin(rng, 0.4) && top_room.w() > 1.6 && top_room.d() > 1.6 {
        let p = Vec2::new(top_room.u0 + 0.7, top_room.v1 - 0.7);
        water_tank(g, c, rng, p, roof_y, 0.5, 0.95);
    }
    if coin(rng, 0.3) {
        small_dish(g, c, rng, Vec2::new(top_room.u1 - 0.4, top_room.v0 + 0.5), roof_y);
    }
    // Washing from the house to a post in the yard.
    if coin(rng, 0.55) {
        let a = p3(core.u0, 1.9, core.v0 - 0.15);
        let to = Vec2::new(core.u0 - rng.random_range(0.6..1.6), core.v0 - rng.random_range(1.8..3.2));
        if spot(c, to, 0.4) && rooms.iter().all(|o| !(to.x > o.u0 - 0.3 && to.x < o.u1 + 0.3 && to.y > o.v0 - 0.3 && to.y < o.v1 + 0.3)) {
            f.bx(g, Wood, to.x, 1.0, to.y, Vec3::new(0.08, 2.0, 0.08), mul(c.palette.wood, 0.5));
            laundry(g, c, rng, a, p3(to.x, 1.9, to.y));
        }
    }
}

/// Bare earth, a little darker where it is swept.
fn dirt(c: &Ctx) -> [f32; 3] {
    mul(mix(c.palette.lot_vacant, [0.42, 0.34, 0.25], 0.55), 0.9 - 0.1 * c.wear)
}

/// Somewhere in the yards clear of every room.
fn clear_spot(c: &Ctx, rng: &mut impl Rng, rooms: &[R], margin: f32) -> Option<Vec2> {
    for _ in 0..12 {
        let p = c.random_inside(rng, 1.0)?;
        if rooms.iter().all(|o| !(p.x > o.u0 - margin && p.x < o.u1 + margin && p.y > o.v0 - margin && p.y < o.v1 + margin)) {
            return Some(p);
        }
    }
    None
}

/// Cells over a rectangle about `cw` by `cd`, in rows from the street.
fn cells(area: &R, cw: f32, cd: f32) -> Vec<R> {
    let cols = ((area.w() / cw) as i32).clamp(1, 8);
    let rows = ((area.d() / cd) as i32).clamp(1, 8);
    let (w, d) = (area.w() / cols as f32, area.d() / rows as f32);
    let mut v = Vec::new();
    for j in 0..rows {
        for i in 0..cols {
            v.push(R::new(area.u0 + i as f32 * w, area.u0 + (i + 1) as f32 * w, area.v0 + j as f32 * d, area.v0 + (j + 1) as f32 * d));
        }
    }
    v
}

/// Households crowded over a rectangle: a plot each about 5.6 by 6 m, a
/// yard left here and there.
fn fill(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, area: &R, rooms: &mut Vec<R>, eaves: &mut Vec<Vec3>) {
    if area.w() < 3.5 || area.d() < 3.5 {
        return;
    }
    for cell in cells(area, 5.6, 6.0) {
        if coin(rng, 0.07) {
            continue;
        }
        homestead(g, c, rng, &cell.inset(0.35), rooms, eaves);
    }
}

/// The neighbourhood's wiring and fire: a pole at the street with wires
/// to the eaves, a barrel in a yard.
fn wire_up(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, rooms: &[R], eaves: &[Vec3]) {
    let area = R::of(&c.poly);
    let p = Vec2::new(rng.random_range(area.u0 + 0.5..area.u0 + area.w() * 0.4 + 0.6), area.v0 + 0.3);
    if spot(c, p, 0.3) && rooms.iter().all(|o| !(p.x > o.u0 - 0.5 && p.x < o.u1 + 0.5 && p.y > o.v0 - 0.5 && p.y < o.v1 + 0.5)) {
        let to: Vec<Vec3> = eaves.iter().filter(|e| Vec2::new(e.x, e.z).distance(p) < 16.0).take(7).copied().collect();
        let h = rng.random_range(6.0..7.5);
        pole_and_wires(g, c, rng, p, h, &to);
    }
    if let Some(q) = clear_spot(c, rng, rooms, 1.2) {
        fire_barrel(g, c, q);
    }
}

// ── homes ─────────────────────────────────────────────────────────────────────

/// Corrugated-iron shacks: a household to each patch of swept earth, its
/// first room grown into three or four of whatever sheet came to hand,
/// some with a room up on the roof; tanks and dishes on the roofs, washing
/// on lines to posts, a leaning pole with its tangle of wires, a fire in a
/// barrel in a yard.
pub fn tin_shacks(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let yard_poly = yard(g, c, 0.3, Dirt, dirt(c));
    let area = R::of(&c.poly);
    let mut rooms: Vec<R> = Vec::new();
    let mut eaves: Vec<Vec3> = Vec::new();
    fill(g, c, rng, &area, &mut rooms, &mut eaves);
    if rooms.is_empty() {
        // Too small a lot for a yard: one room on it.
        if let Some(r) = fit(c, R::new(area.c().x - 1.6, area.c().x + 1.6, area.c().y - 1.4, area.c().y + 1.4), 1.6) {
            room(g, c, rng, &r, 0.0, 2.4, Some(0), true);
        }
        return;
    }
    wire_up(g, c, rng, &rooms, &eaves);
    weeds(g, c, rng, &yard_poly, 4);
}

/// The water off a lot, if it lies behind it (the usual riverbank lot).
fn water_behind(c: &Ctx) -> bool {
    c.shore.is_some_and(|s| s.seaward != Vec2::ZERO) && sea_local(c, c.shore).y > 0.6
}

/// A rickety plank walk from where the lot meets the water out over it on
/// piles, its planks each a little askew, a canoe tied at its end.
fn plank_walk(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, from: Vec2, y: f32) {
    let Some(look) = c.shore else { return };
    let f = &c.frame;
    let sea = sea_local(c, c.shore);
    if !c.inside_local(from) {
        return;
    }
    let mut d = 0.0;
    while d < 300.0 && c.inside_local(from + sea * (d + 0.5)) {
        d += 0.5;
    }
    let root = from + sea * d;
    let across = sea.perp();
    let len = rng.random_range(6.0..9.0);
    let drop = look.drop.max(0.8);
    let wood = mul(c.palette.wood, 0.7 - 0.2 * c.wear);
    let yaw = sea.y.atan2(sea.x);
    let mut k = -1.0;
    while k < len {
        let w = rng.random_range(0.24..0.32);
        let p = root + sea * k + across * rng.random_range(-0.06..0.06);
        f.bx_yaw(g, Wood, p.x, y - 0.03, p.y, Vec3::new(w, 0.05, rng.random_range(1.1..1.35)), mul(wood, rng.random_range(0.8..1.1)), yaw + rng.random_range(-0.05..0.05));
        k += w + rng.random_range(0.03..0.09);
    }
    let pile = rgba(mul(c.palette.wood, 0.4));
    let mut k = 0.5;
    while k <= len {
        for s in [-1.0f32, 1.0] {
            let p = root + sea * k + across * s * 0.6;
            g.part(Wood).cylinder_capped(f.to_world(p.x, p.y), 0.09, 0.09, -drop - 1.2, y - 0.06, 6, false, true, pile);
        }
        k += 2.2;
    }
    g.lod(Lod::Mid, |g| {
        // A dugout canoe tied alongside the end.
        let boat = root + sea * (len - 1.6) + across * 1.3;
        f.bx_yaw(g, Wood, boat.x, -drop + 0.2, boat.y, Vec3::new(4.2, 0.4, 0.7), mul(PLANKS[1], 0.6), yaw);
        f.bx_yaw(g, Wood, boat.x, -drop + 0.41, boat.y, Vec3::new(3.6, 0.04, 0.5), mul(PLANKS[0], 0.4), yaw);
    });
}

/// Plank houses on piles by the water (on a low plinth inland): each its
/// own height off the ground, under a sheet gable, a veranda in front with
/// a rail and steps, washing along it, a bulb by the door, a tank on a
/// stand; a plank walk out over the water to a canoe.
pub fn stilt_houses(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Dirt, dirt(c));
    let area = R::of(&c.poly);
    let wet = c.shore.is_some();
    let n = ((area.w() / 7.5).round() as i32).clamp(1, 6);
    let slot = area.w() / n as f32;
    let mut eaves: Vec<Vec3> = Vec::new();
    let mut rooms: Vec<R> = Vec::new();
    for i in 0..n {
        let hw = (slot - rng.random_range(1.2..2.6)).clamp(3.0, 7.0);
        let hd = (area.d() * 0.42).clamp(3.5, rng.random_range(5.5..8.0));
        let u = area.u0 + slot * (i as f32 + 0.5) + rng.random_range(-0.4..0.4);
        let v0 = if wet { area.v1 - hd - rng.random_range(0.2..1.0) } else { area.v0 + 2.2 + rng.random_range(0.0..1.2) };
        let Some(r) = fit(c, R::new(u - hw * 0.5, u + hw * 0.5, v0, v0 + hd), 3.0) else { continue };
        let lift = if wet { rng.random_range(1.5..2.4) } else { rng.random_range(0.5..0.65) };
        let wood = mul(any(rng, &PLANKS), 1.0 - 0.25 * c.wear);
        if wet {
            // Piles, braced.
            let (nu, nv) = (((r.w() / 2.4) as i32).max(1), ((r.d() / 2.4) as i32).max(1));
            for a in 0..=nu {
                for b in 0..=nv {
                    let p = Vec2::new(r.u0 + 0.2 + (r.w() - 0.4) * a as f32 / nu as f32, r.v0 + 0.2 + (r.d() - 0.4) * b as f32 / nv as f32);
                    g.part(Wood).cylinder(f.to_world(p.x, p.y), 0.12, 0.11, 0.0, lift - 0.15, 6, rgba(mul(pal.wood, 0.4)));
                }
            }
            g.lod(Lod::Mid, |g| {
                for b in [r.v0 + 0.2, r.v1 - 0.2] {
                    g.part(Wood).beam(f.at(r.u0 + 0.2, 0.2, b), f.at(r.u1 - 0.2, lift - 0.3, b), 0.07, rgba(mul(pal.wood, 0.45)));
                }
            });
        } else {
            block(g, c, Concrete, &r.inset(0.1), 0.0, lift - 0.15, mul(BLOCK, 0.75));
        }
        // The floor and the walls.
        block(g, c, Wood, &r, lift - 0.15, lift, mul(wood, 0.7));
        let wh = rng.random_range(2.4..2.8);
        let walls = R::new(r.u0 + 0.1, r.u1 - 0.1, r.v0 + 0.1, r.v1 - 0.1);
        block(g, c, Wood, &walls, lift, lift + wh, wood);
        for (k, face) in walls.faces().iter().enumerate() {
            if coin(rng, 0.6) {
                patches(g, c, rng, face, lift, lift + wh);
            }
            if k == 0 {
                doorway(g, c, rng, face, -face.len * 0.2, lift);
                bare_bulb(g, c, face, -face.len * 0.2 + 0.7, lift + 2.05);
                if face.len > 3.6 {
                    hole_window(g, c, rng, face, face.len * 0.22, lift + 1.4, 0.8, 0.6);
                }
            } else if face.len > 3.0 && coin(rng, 0.6) {
                hole_window(g, c, rng, face, 0.0, lift + 1.4, 0.7, 0.55);
            }
        }
        let (s, rc) = sheet(c, rng);
        let rise = (walls.w().min(walls.d()) * 0.22).clamp(0.7, 1.4);
        gabled(g, c, &walls, lift + wh, rise, walls.w() >= walls.d(), 0.35, s, rc, Wood, wood);
        eaves.push(p3(walls.u0 + 0.3, lift + wh - 0.1, walls.v0));
        rooms.push(r);
        // The veranda: deck, rail, steps or a ladder down.
        let ver = R::new(r.u0, r.u1, r.v0 - 1.5, r.v0);
        if on_lot(c, &ver) {
            block(g, c, Wood, &ver, lift - 0.12, lift - 0.02, mul(wood, 0.75));
            for s in [ver.u0 + 0.15, ver.u1 - 0.15] {
                f.bx(g, Wood, s, (lift - 0.12) * 0.5, ver.v0 + 0.15, Vec3::new(0.12, lift - 0.12, 0.12), mul(pal.wood, 0.45));
            }
            g.lod(Lod::Mid, |g| {
                let fr = ver.front();
                fr.bx(g, f, Wood, 0.6, lift + 0.85, -0.1, Vec3::new(fr.len - 1.4, 0.06, 0.06), mul(wood, 0.8));
                let n = ((fr.len - 1.4) / 0.9) as i32;
                for k in 0..=n {
                    fr.bx(g, f, Wood, -fr.len * 0.5 + 1.3 + k as f32 * 0.9, lift + 0.42, -0.1, Vec3::new(0.05, 0.85, 0.05), mul(wood, 0.8));
                }
                // Steps down at the door's end.
                let steps = ((lift - 0.1) / 0.3).ceil() as i32;
                for k in 0..steps {
                    let y = lift - 0.1 - (k as f32 + 0.5) * (lift - 0.1) / steps as f32;
                    fr.bx(g, f, Wood, -fr.len * 0.5 + 0.55, y, 0.15 + k as f32 * 0.28, Vec3::new(0.8, 0.05, 0.28), mul(wood, 0.7));
                }
            });
            laundry(g, c, rng, p3(ver.u0 + 0.9, lift + 2.0, ver.v0 + 0.15), p3(ver.u1 - 0.2, lift + 2.0, ver.v0 + 0.15));
        }
        if i % 2 == 0 && coin(rng, 0.6) {
            let tp = Vec2::new(r.u1 + 0.9, r.v1 - 1.0);
            if spot(c, tp, 0.8) && rooms.iter().all(|o| !(tp.x > o.u0 - 0.8 && tp.x < o.u1 + 0.8 && tp.y > o.v0 - 0.8 && tp.y < o.v1 + 0.8)) {
                tank_stand(g, c, rng, tp, if wet { lift + 0.4 } else { 2.0 });
            }
        }
        if coin(rng, 0.35) {
            small_dish(g, c, rng, Vec2::new(walls.u1 - 0.4, walls.v1 - 0.4), lift + wh);
        }
    }
    if rooms.is_empty() {
        return tin_shacks(g, c, rng);
    }
    // Shacks on the rest: in front of the houses by the water, behind them
    // inland.
    let houses = rooms.clone();
    let lo = houses.iter().map(|r| r.v0).fold(area.v1, f32::min);
    let hi = houses.iter().map(|r| r.v1).fold(area.v0, f32::max);
    let rest = if wet { R::new(area.u0, area.u1, area.v0, lo - 2.0) } else { R::new(area.u0, area.u1, hi + 1.0, area.v1) };
    fill(g, c, rng, &rest, &mut rooms, &mut eaves);
    wire_up(g, c, rng, &rooms, &eaves);
    if wet && water_behind(c) {
        // From behind the houses, down the middle to the water.
        plank_walk(g, c, rng, Vec2::new(area.c().x, hi + 0.3), 0.35);
    }
}

/// Self-built storeys on an old concrete base: a shop or a garage in the
/// base, then each floor what the family could afford (raw brick, block,
/// render, sheet), each a little out of line with the one under it; a tin
/// room and a terrace on top with tanks, a dish and washing, the bars for
/// the next floor sticking up; a ladder up the side; wires from a pole.
pub fn patch_stack(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Dirt, dirt(c));
    let area = R::of(&c.poly);
    let n = ((area.w() / 9.0).round() as i32).clamp(1, 3);
    let slot = area.w() / n as f32;
    let floors = (c.floors as i32).clamp(2, 6);
    let mut eaves: Vec<Vec3> = Vec::new();
    let mut rooms: Vec<R> = Vec::new();
    for i in 0..n {
        let bw = (slot - rng.random_range(0.6..1.8)).clamp(4.0, 10.0);
        let bd = (area.d() * 0.6).clamp(4.0, rng.random_range(8.0..12.0));
        // Each its own storey height: no two roofs side by side share a plane.
        let fh = 2.75 + 0.07 * i as f32 + rng.random_range(0.0..0.05);
        let (s0, s1) = (area.u0 + slot * i as f32 + 0.1, area.u0 + slot * (i + 1) as f32 - 0.1);
        let u = area.u0 + slot * (i as f32 + 0.5);
        let Some(base) = fit(c, R::new(u - bw * 0.5, u + bw * 0.5, area.v0 + 0.5, area.v0 + 0.5 + bd), 3.5) else { continue };
        rooms.push(base);
        // The concrete base: a roller door or a door and a window.
        block(g, c, Concrete, &base, 0.0, fh, mul(BLOCK, 0.85 - 0.15 * c.wear));
        let fr = base.front();
        if fr.len > 4.0 && coin(rng, 0.55) {
            roller_door(g, c, &fr, -fr.len * 0.15, (fr.len * 0.5).min(3.2), 2.3, mul(faded(c, rng), 0.9));
        } else {
            doorway(g, c, rng, &fr, -fr.len * 0.25, 0.0);
            hole_window(g, c, rng, &fr, fr.len * 0.18, 1.4, 1.0, 0.8);
        }
        bare_bulb(g, c, &fr, fr.len * 0.38, 2.45);
        grime(g, c, rng, &fr, 0.0, fh, 1.2);
        if coin(rng, 0.4) {
            graffiti(g, c, rng, &fr, 0.2, 1.8, 0.6);
        }
        // Storeys as they came.
        let mut below = base;
        let mut y = fh;
        for k in 1..floors {
            let last = k == floors - 1;
            let jog: [f32; 4] = [rng.random_range(-0.5..0.35), rng.random_range(-0.5..0.35), rng.random_range(-0.5..0.35), rng.random_range(-0.5..0.35)];
            let mut r = R::new((below.u0 + jog[0]).max(s0), (below.u1 - jog[1]).min(s1), below.v0 + jog[2], below.v1 - jog[3].max(0.0));
            if last {
                // Only part of the top: a tin room, the rest a terrace.
                let cut = r.w() * rng.random_range(0.4..0.65);
                r = if coin(rng, 0.5) { R::new(r.u0, r.u0 + cut, r.v0 + 0.4, r.v1) } else { R::new(r.u1 - cut, r.u1, r.v0 + 0.4, r.v1) };
            }
            let Some(r) = fit(c, r, 2.0) else { break };
            // The floor slab, a lip over the walls below.
            block(g, c, Concrete, &R::new(below.u0 - 0.04, below.u1 + 0.04, below.v0 - 0.04, below.v1 + 0.04), y, y + 0.16, mul(BLOCK, 0.78));
            let y0 = y + 0.16;
            if last {
                let top = room(g, c, rng, &r, y0, 2.4, Some(0), true);
                eaves.push(p3(r.u0 + 0.3, y0 + 2.2, r.v0));
                // The terrace: tanks, a dish, washing, the starter bars.
                let terr = if r.u0 > below.u0 + 1.0 { R::new(below.u0 + 0.3, r.u0 - 0.3, below.v0 + 0.3, below.v1 - 0.3) } else { R::new(r.u1 + 0.3, below.u1 - 0.3, below.v0 + 0.3, below.v1 - 0.3) };
                if terr.w() > 1.4 && terr.d() > 1.4 {
                    let tanks = rng.random_range(1..=3);
                    for t in 0..tanks {
                        let p = Vec2::new(terr.u0 + 0.6 + (t as f32) * 1.25, terr.v1 - 0.7);
                        if p.x < terr.u1 - 0.5 {
                            let th = rng.random_range(0.9..1.3);
                            water_tank(g, c, rng, p, y0, 0.55, th);
                        }
                    }
                    laundry(g, c, rng, p3(terr.u0 + 0.2, y0 + 1.7, terr.v0 + 0.5), p3(terr.u1 - 0.2, y0 + 1.7, terr.v0 + 0.6));
                    g.lod(Lod::Mid, |g| {
                        for (bu, bv) in [(terr.u0, terr.v0), (terr.u1, terr.v0), (terr.u0, terr.v1), (terr.u1, terr.v1)] {
                            for d in [-0.08f32, 0.08] {
                                f.bx(g, Rust, bu + d, y0 + 0.5, bv, Vec3::new(0.025, 1.0, 0.025), mul(pal.rust, 0.7));
                            }
                        }
                    });
                }
                small_dish(g, c, rng, Vec2::new(r.c().x, r.v1 - 0.4), top);
            } else {
                let (s, col) = match rng.random_range(0..4) {
                    0 => (Brick, mul(pick(rng, &pal.brick), rng.random_range(0.85..1.05))),
                    1 => (Concrete, mul(BLOCK, rng.random_range(0.8..1.0))),
                    2 => (Plaster, faded(c, rng)),
                    _ => sheet(c, rng),
                };
                block(g, c, s, &r, y0, y0 + fh - 0.16, col);
                for (fi, face) in r.faces().iter().enumerate() {
                    let n = ((face.len - 1.0) / 2.6).floor() as i32;
                    for j in 0..n.max(0) {
                        let t = -(face.len - 1.0) * 0.5 + (j as f32 + 0.5) * (face.len - 1.0) / n as f32 + rng.random_range(-0.3..0.3);
                        if coin(rng, 0.8) {
                            let (ww, wh) = (rng.random_range(0.7..1.2), rng.random_range(0.7..1.0));
                            hole_window(g, c, rng, face, t, y0 + 1.3, ww, wh);
                            if fi == 0 && coin(rng, 0.35) {
                                // Burglar bars.
                                g.lod(Lod::Mid, |g| {
                                    for b in -2..=2 {
                                        face.bx(g, f, Matte, t + b as f32 * 0.2, y0 + 1.3, 0.09, Vec3::new(0.025, 1.0, 0.025), DARK);
                                    }
                                });
                            }
                        }
                    }
                    if s != Rust && s != Metal && coin(rng, 0.5) {
                        patches(g, c, rng, face, y0, y0 + fh - 0.16);
                    }
                }
                grime(g, c, rng, &r.front(), y0, y0 + fh, 1.0);
            }
            below = r;
            y = y0 + fh - 0.16;
            if last {
                break;
            }
        }
        // A ladder up the side to the top.
        g.lod(Lod::Mid, |g| {
            let side = if i % 2 == 0 { base.right() } else { base.left() };
            let top = y.min(fh * 2.0);
            for s in [-0.22f32, 0.22] {
                let a = side.at(base.d() * -0.3 + s, 0.2);
                g.part(Rust).beam(f.at2(a, 0.0), f.at2(a, top), 0.05, rgba(mul(pal.rust, 0.7)));
            }
            let mut ry = 0.3;
            while ry < top {
                side.bx(g, f, Rust, base.d() * -0.3, ry, 0.2, Vec3::new(0.44, 0.04, 0.04), mul(pal.rust, 0.7));
                ry += 0.35;
            }
        });
        eaves.push(p3(base.u0 + 0.4, fh - 0.2, base.v0));
    }
    if rooms.is_empty() {
        return tin_shacks(g, c, rng);
    }
    let hi = rooms.iter().map(|r| r.v1).fold(area.v0, f32::max);
    fill(g, c, rng, &R::new(area.u0, area.u1, hi + 1.5, area.v1), &mut rooms, &mut eaves);
    wire_up(g, c, rng, &rooms, &eaves);
}

// ── trade and nights out ──────────────────────────────────────────────────────

/// A stall: four posts, a counter with its goods, a tarp sagging over it.
fn stall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R) {
    let f = &c.frame;
    let h = rng.random_range(2.1..2.5);
    let wood = mul(c.palette.wood, rng.random_range(0.5..0.75));
    for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u0, r.v1), (r.u1, r.v1)] {
        f.bx(g, Wood, u, h * 0.5, v, Vec3::new(0.08, h, 0.08), wood);
    }
    // The counter along the lane side.
    f.bx(g, Wood, r.c().x, 0.45, r.v0 + 0.35, Vec3::new(r.w() - 0.3, 0.9, 0.6), mul(wood, 1.2));
    g.lod(Lod::Mid, |g| {
        let n = ((r.w() - 0.4) / 0.55) as i32;
        for k in 0..n {
            let u = r.u0 + 0.45 + k as f32 * 0.55;
            let col = mul(any(rng, &[[0.86, 0.30, 0.16], [0.40, 0.60, 0.22], [0.92, 0.74, 0.22], [0.70, 0.20, 0.30], [0.84, 0.82, 0.76], [0.30, 0.40, 0.70]]), 0.9);
            f.bx(g, Tarp, u, 0.98, r.v0 + 0.35, Vec3::new(0.45, 0.16, 0.45), col);
        }
    });
    // A bare bulb hung under the tarp over the counter.
    let (bu, bv) = (r.c().x + rng.random_range(-0.4..0.4), r.v0 + 0.6);
    g.lod(Lod::Mid, |g| f.bx(g, Matte, bu, h - 0.4, bv, Vec3::new(0.02, 0.5, 0.02), DARK));
    let from = g.part(Glow).positions.len();
    f.bx(g, Glow, bu, h - 0.72, bv, Vec3::splat(0.14), mul(BULB, 1.0 - 0.25 * c.wear));
    mark_glow(g, from, NIGHT_MARK);
    // The tarp: two slopes meeting low in the middle (it sags), seen from
    // above and below.
    let col = rgba(mul(any(rng, &TARPS), 1.0 - 0.3 * c.wear));
    let under = rgba(mul(any(rng, &TARPS), 0.55));
    let o = 0.25;
    let (y0, ym) = (h + 0.05, h - 0.18);
    let mid = r.c().x;
    for (ua, ub) in [(r.u0 - o, mid), (mid, r.u1 + o)] {
        let ya = if ua < mid { y0 } else { ym };
        let yb = if ub > mid { y0 } else { ym };
        let q = [f.at(ua, ya, r.v0 - o), f.at(ub, yb, r.v0 - o), f.at(ub, yb, r.v1 + o), f.at(ua, ya, r.v1 + o)];
        g.part(Tarp).quad_facing(q, Vec3::Y, col);
        let q2 = q.map(|p| p - Vec3::Y * 0.02);
        g.part(Tarp).quad_facing([q2[0], q2[3], q2[2], q2[1]], -Vec3::Y, under);
    }
}

/// A market of stalls under tarps: rows of them back to back down lanes
/// in from the street, a sheet back on some, crates and sacks, bulbs
/// strung over the lanes, a generator at the end.
pub fn tarp_market(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let yard_poly = yard(g, c, 0.3, Dirt, mul(dirt(c), 1.05));
    let area = R::of(&c.poly);
    let lane = 2.6;
    let sd = 2.4;
    let mut rooms: Vec<R> = Vec::new();
    // Rows of stalls across the lot, a lane in front of each pair.
    let mut v = area.v0 + lane;
    let mut row = 0;
    while v + sd <= area.v1 - 0.3 {
        let mut u = area.u0 + 0.2;
        while u + 2.2 <= area.u1 - 0.2 {
            let w = rng.random_range(2.2f32..3.2).min(area.u1 - 0.2 - u);
            let r = R::new(u + 0.1, u + w - 0.1, v, v + sd);
            if on_lot(c, &r) && !coin(rng, 0.08) {
                stall(g, c, rng, &r);
                rooms.push(r);
                if coin(rng, 0.4) {
                    let back = if row % 2 == 0 { r.back() } else { r.front() };
                    let (s, col) = sheet(c, rng);
                    back.bx(g, f, s, 0.0, 1.0, -0.06, Vec3::new(r.w(), 2.0, 0.04), col);
                }
            }
            u += w;
        }
        // Back to back, then a lane.
        v += sd + if row % 2 == 0 { 0.1 } else { lane };
        row += 1;
    }
    if rooms.is_empty() {
        return corner_kiosk(g, c, rng);
    }
    // Bulbs strung down the lanes, over the tarps' edges, from a row's
    // first stall to its last.
    let mut strung = 0;
    let mut i = 0;
    while i < rooms.len() && strung < 6 {
        let first = rooms[i];
        let mut j = i;
        while j + 1 < rooms.len() && (rooms[j + 1].v0 - first.v0).abs() < 0.1 {
            j += 1;
        }
        let last = rooms[j];
        if j > i && coin(rng, 0.8) {
            bulbs(g, c, rng, p3(first.u0 + 0.2, 2.75, first.v0 - 0.9), p3(last.u1 - 0.2, 2.75, last.v0 - 0.9), 0.35, 1.1);
            strung += 1;
        }
        i = j + 1;
    }
    // Crates and sacks in the lanes, a generator, a fire.
    g.lod(Lod::Near, |g| {
        for r in rooms.iter().step_by(3) {
            let p = Vec2::new(r.u0 + 0.4, r.v0 - 0.45);
            if spot(c, p, 0.4) {
                for k in 0..rng.random_range(1..4) {
                    f.bx(g, Panel, p.x, 0.17 + k as f32 * 0.32, p.y, Vec3::new(0.5, 0.3, 0.36), any(rng, &[[0.80, 0.20, 0.16], [0.20, 0.40, 0.70], [0.30, 0.55, 0.30], [0.90, 0.75, 0.20]]));
                }
            }
        }
    });
    let gp = Vec2::new(area.u1 - 0.9, area.v0 + 1.0);
    if spot(c, gp, 0.6) {
        f.bx(g, Metal, gp.x, 0.4, gp.y, Vec3::new(1.0, 0.8, 0.6), mul([0.80, 0.62, 0.14], 1.0 - 0.3 * c.wear));
        g.lod(Lod::Mid, |g| g.part(Matte).cylinder(f.to_world(gp.x + 0.3, gp.y), 0.05, 0.05, 0.8, 1.3, 6, rgba(DARK)));
    }
    if let Some(q) = clear_spot(c, rng, &rooms, 0.8) {
        fire_barrel(g, c, q);
    }
    weeds(g, c, rng, &yard_poly, 3);
}

/// A corner kiosk at the street: a painted sheet box with its hatch up
/// behind a grille, a lit drinks fridge inside, a hand-painted sign on the
/// roof, crates and plastic chairs under a parasol, a bulb over the hatch;
/// the family's shacks behind it.
pub fn corner_kiosk(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Dirt, dirt(c));
    let area = R::of(&c.poly);
    let left = c.seed % 2 == 0;
    let (kw, kd) = (rng.random_range(3.0..4.2), rng.random_range(2.4..3.0));
    let u = if left { area.u0 + 0.4 + kw * 0.5 } else { area.u1 - 0.4 - kw * 0.5 };
    let Some(k) = fit(c, R::new(u - kw * 0.5, u + kw * 0.5, area.v0 + 0.4, area.v0 + 0.4 + kd), 2.0) else { return tin_shacks(g, c, rng) };
    let h = 2.6;
    // A drinks maker's livery, faded: a red, yellow or blue box with a
    // white band.
    let livery = mul(any(rng, &[[0.74, 0.12, 0.10], [0.92, 0.72, 0.12], [0.14, 0.30, 0.66], [0.16, 0.50, 0.30]]), 1.0 - 0.3 * c.wear);
    block(g, c, Panel, &k, 0.0, h, livery);
    let fr = k.front();
    fr.decal(g, f, Panel, 0.0, 0.5, 0.012, fr.len, 0.25, mul([0.92, 0.92, 0.88], 1.0 - 0.3 * c.wear));
    for face in [k.right(), k.left(), k.back()] {
        patches(g, c, rng, &face, 0.0, h);
    }
    // The hatch: dark inside, a lit fridge, a counter, the grille.
    let hw = (fr.len * 0.6).min(2.4);
    fr.decal(g, f, Matte, 0.0, 1.6, 0.012, hw, 1.0, DARK);
    fr.decal(g, f, Metal, hw * 0.25, 1.55, 0.024, 0.62, 0.9, [0.84, 0.84, 0.82]);
    let from = g.part(Glow).positions.len();
    fr.decal(g, f, Glow, hw * 0.25, 1.55, 0.036, 0.5, 0.78, FRIDGE);
    mark_glow(g, from, NIGHT_MARK);
    fr.bx(g, f, Wood, 0.0, 1.08, 0.18, Vec3::new(hw + 0.2, 0.06, 0.36), mul(pal.wood, 0.8));
    g.lod(Lod::Mid, |g| {
        let n = (hw / 0.18) as i32;
        for b in 0..=n {
            fr.bx(g, f, Matte, -hw * 0.5 + b as f32 * hw / n as f32, 1.6, 0.075, Vec3::new(0.02, 1.0, 0.02), DARK);
        }
        // The flap propped up over the hatch.
        let a = f.at2(fr.at(0.0, 0.02), 2.15);
        let b = f.at2(fr.at(0.0, 0.85), 2.45);
        g.part(Panel).slab(a, b, hw + 0.2, 0.04, rgba(mul(livery, 0.85)));
    });
    bare_bulb(g, c, &fr, hw * 0.5 + 0.3, 2.3);
    lean_roof(g, c, rng, &k, h, 0.3);
    // The sign over the hatch on the roof's front.
    painted_sign(g, c, rng, &fr, 0.0, h + 0.75, 0.1, (fr.len * 0.9).min(3.6), 0.7);
    g.lod(Lod::Mid, |g| {
        for s in [-1.0f32, 1.0] {
            fr.bx(g, f, Wood, s * (fr.len * 0.4), h + 0.25, 0.14, Vec3::new(0.06, 0.6, 0.06), mul(pal.wood, 0.5));
        }
    });
    // A parasol and chairs at the street, crates of empties.
    let pp = Vec2::new(if left { k.u1 + 1.6 } else { k.u0 - 1.6 }, k.v0 + 0.9);
    if spot(c, pp, 1.2) {
        g.lod(Lod::Mid, |g| {
            f.bx(g, Matte, pp.x, 1.1, pp.y, Vec3::new(0.05, 2.2, 0.05), CHARCOAL);
            g.part(Tarp).cylinder(f.to_world(pp.x, pp.y), 1.2, 0.08, 1.95, 2.35, 8, rgba(mul(livery, 1.1)));
        });
        let chair_col = any(rng, &[[0.86, 0.86, 0.82], [0.20, 0.42, 0.70], [0.80, 0.22, 0.18]]);
        chair(g, c, pp + Vec2::new(0.6, 0.0), chair_col);
        chair(g, c, pp - Vec2::new(0.6, 0.1), chair_col);
    }
    g.lod(Lod::Near, |g| {
        let cp = Vec2::new(if left { k.u1 + 0.4 } else { k.u0 - 0.4 }, k.v1 - 0.4);
        if spot(c, cp, 0.4) {
            for j in 0..rng.random_range(2..5) {
                f.bx(g, Panel, cp.x, 0.15 + j as f32 * 0.29, cp.y, Vec3::new(0.42, 0.28, 0.32), mul([0.70, 0.16, 0.12], 0.9));
            }
        }
    });
    // The family's shacks behind.
    let back = R::new(area.u0, area.u1, k.v1 + 1.2, area.v1);
    let mut rooms = vec![k];
    let mut eaves = vec![p3(k.u0 + 0.2, h - 0.1, k.v0)];
    if back.d() > 3.5 {
        fill(g, c, rng, &back, &mut rooms, &mut eaves);
    }
    wire_up(g, c, rng, &rooms, &eaves);
}

/// A shebeen: a long sheet hall under a tin gable, painted across the
/// front, a lit serving hatch and the football on a television; a lean-to
/// over tables and plastic chairs; the yard strung with bulbs to poles at
/// the street; a speaker stack, crates of empties, a fire in a barrel.
pub fn shebeen(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.3, Dirt, dirt(c));
    let area = R::of(&c.poly);
    let hw = (area.w() * 0.7).clamp(4.0, 13.0);
    let hd = (area.d() * 0.35).clamp(3.5, 7.0);
    let v0 = area.v0 + (area.d() * 0.35).clamp(3.5, 7.0);
    let Some(hall) = fit(c, R::new(area.c().x - hw * 0.5, area.c().x + hw * 0.5, v0, v0 + hd), 3.0) else { return corner_kiosk(g, c, rng) };
    let h = 3.0;
    let (s, col) = sheet(c, rng);
    block(g, c, s, &hall, 0.0, h, col);
    let fr = hall.front();
    // Painted across the front, peeling.
    let paint = faded(c, rng);
    fr.decal(g, f, Panel, 0.0, h * 0.5, 0.012, fr.len * 0.96, h * 0.9, paint);
    for face in [hall.right(), hall.left(), hall.back()] {
        patches(g, c, rng, &face, 0.0, h);
    }
    doorway(g, c, rng, &fr, -fr.len * 0.3, 0.0);
    // The hatch, lit inside after dark.
    let hatch = (fr.len * 0.3).min(2.6);
    fr.glaze(g, c, fr.len * 0.1, 1.55, PANE, hatch, 0.9, Pane::Dark);
    let from = g.part(Glow).positions.len();
    fr.decal(g, f, Glow, fr.len * 0.1, 1.55, PANE + 0.012, hatch * 0.9, 0.8, mul(BULB, 0.55));
    mark_glow(g, from, NIGHT_MARK);
    if fr.len > 6.0 {
        tv(g, c, &fr, fr.len * 0.36, 1.7, 0.9, 0.7);
    }
    let (rs, rc) = sheet(c, rng);
    let rise = (hall.d() * 0.18).clamp(0.6, 1.2);
    gabled(g, c, &hall, h, rise, true, 0.3, rs, rc, s, col);
    painted_sign(g, c, rng, &fr, 0.0, h + rise * 0.5 + 0.2, 0.36, (fr.len * 0.5).min(4.5), 0.8);
    // The lean-to over the tables.
    let lean = R::new(hall.u0 + 0.3, hall.u1 - 0.3, hall.v0 - 3.2, hall.v0);
    let mut poles: Vec<Vec2> = Vec::new();
    if on_lot(c, &lean) {
        let (ls, lc) = sheet(c, rng);
        g.part(ls).slab(f.at(lean.c().x, h - 0.15, lean.v1), f.at(lean.c().x, 2.35, lean.v0 - 0.2), lean.w(), 0.05, rgba(mul(lc, 0.9)));
        let n = ((lean.w() / 3.0) as i32).max(1);
        for k in 0..=n {
            let u = lean.u0 + 0.1 + (lean.w() - 0.2) * k as f32 / n as f32;
            f.bx(g, Wood, u, 1.15, lean.v0 + 0.1, Vec3::new(0.1, 2.3, 0.1), mul(pal.wood, 0.45));
            poles.push(Vec2::new(u, lean.v0 + 0.1));
        }
        g.lod(Lod::Mid, |g| {
            let n = ((lean.w() - 1.0) / 2.4) as i32;
            for k in 0..n.max(0) {
                let p = Vec2::new(lean.u0 + 1.4 + k as f32 * 2.4, lean.c().y);
                f.bx(g, Panel, p.x, 0.72, p.y, Vec3::new(0.9, 0.05, 0.9), [0.86, 0.86, 0.82]);
                f.bx(g, Panel, p.x, 0.36, p.y, Vec3::new(0.1, 0.72, 0.1), [0.86, 0.86, 0.82]);
            }
        });
        let ch = any(rng, &[[0.86, 0.86, 0.82], [0.20, 0.42, 0.70], [0.80, 0.22, 0.18], [0.30, 0.55, 0.30]]);
        let n = ((lean.w() - 1.0) / 2.4) as i32;
        for k in 0..n.clamp(0, 5) {
            let p = Vec2::new(lean.u0 + 1.4 + k as f32 * 2.4, lean.c().y);
            chair(g, c, p + Vec2::new(0.0, -0.7), ch);
            chair(g, c, p + Vec2::new(0.7, 0.2), ch);
        }
        bare_bulb(g, c, &fr, fr.len * 0.1, 2.5);
    }
    // String lights from the lean-to out to poles at the street.
    for (k, p) in poles.iter().enumerate().step_by(2) {
        let q = Vec2::new(p.x + rng.random_range(-1.0..1.0), area.v0 + 0.6);
        if spot(c, q, 0.3) {
            f.bx(g, Wood, q.x, 1.4, q.y, Vec3::new(0.1, 2.8, 0.1), mul(pal.wood, 0.45));
            bulbs(g, c, rng, p3(p.x, 2.25, p.y), p3(q.x, 2.75, q.y), 0.3, 0.8);
            if k == 0 {
                fire_barrel(g, c, q + Vec2::new(1.2, 1.0));
            }
        }
    }
    // A speaker stack by the door, crates of empties stacked by the wall.
    let sp = fr.at(-fr.len * 0.45, 0.6);
    if spot(c, sp, 0.5) {
        g.lod(Lod::Mid, |g| {
            for j in 0..2 {
                f.bx(g, Matte, sp.x, 0.4 + j as f32 * 0.78, sp.y, Vec3::new(0.6, 0.76, 0.5), DARK);
            }
        });
    }
    g.lod(Lod::Near, |g| {
        let side = hall.right();
        for j in 0..4 {
            for i in 0..rng.random_range(1..4) {
                let p = side.at(-side.len * 0.3 + j as f32 * 0.5, 0.35);
                if spot(c, p, 0.3) {
                    f.bx(g, Panel, p.x, 0.15 + i as f32 * 0.29, p.y, Vec3::new(0.32, 0.28, 0.42), [0.66, 0.14, 0.10]);
                }
            }
        }
    });
    let mut rooms = vec![hall];
    let mut eaves = vec![p3(hall.u0 + 0.3, h - 0.2, hall.v0)];
    fill(g, c, rng, &R::new(area.u0, area.u1, hall.v1 + 1.5, area.v1), &mut rooms, &mut eaves);
    if rooms.len() > 1 {
        wire_up(g, c, rng, &rooms, &eaves);
    }
    weeds(g, c, rng, &yard_poly, 4);
}

// ── works ─────────────────────────────────────────────────────────────────────

/// The scrap yard: a sheet fence round it with a gap for a gate, heaps of
/// scrap and wrecks, sacks of cans and bottles in rows, a sorting shed
/// under a lean-to with its scale, a bulb and a fire in a barrel.
pub fn scrap_yard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.3, Dirt, mul(dirt(c), 0.8));
    let fence = offset_poly(&c.lot_poly, 0.5);
    let fi = c.front_face_index(&fence);
    for (i, face) in c.faces(&fence).iter().enumerate() {
        if Some(i) == fi && face.len > 8.0 {
            // A gate gap in the street side.
            let gap = 2.6;
            sheet_fence(g, c, rng, face.a, face.at(-gap, 0.0), 2.2);
            sheet_fence(g, c, rng, face.at(gap, 0.0), face.b, 2.2);
        } else {
            sheet_fence(g, c, rng, face.a, face.b, 2.2);
        }
    }
    let area = R::of(&c.poly);
    let mut rooms: Vec<R> = Vec::new();
    // The sorting shed at the back.
    let sw = (area.w() * 0.4).clamp(4.0, 10.0);
    if let Some(shed) = fit(c, R::new(area.u1 - sw - 0.5, area.u1 - 0.5, area.v1 - 4.5, area.v1 - 0.5), 3.0) {
        let back = R::new(shed.u0, shed.u1, shed.v1 - 2.0, shed.v1);
        room(g, c, rng, &back, 0.0, 2.6, Some(0), true);
        let fr = back.front();
        let (s, col) = sheet(c, rng);
        g.part(s).slab(f.at(shed.c().x, 2.75, back.v0), f.at(shed.c().x, 2.3, shed.v0 - 0.2), shed.w(), 0.05, rgba(col));
        for u in [shed.u0 + 0.1, shed.u1 - 0.1] {
            f.bx(g, Wood, u, 1.15, shed.v0, Vec3::new(0.1, 2.3, 0.1), mul(pal.wood, 0.45));
        }
        g.lod(Lod::Mid, |g| {
            // The scale: a platform and its dial post.
            f.bx(g, Metal, shed.c().x, 0.08, shed.c().y - 0.3, Vec3::new(1.2, 0.16, 1.0), mul(pal.steel, 0.7));
            f.bx(g, Matte, shed.c().x + 0.7, 0.6, shed.c().y - 0.3, Vec3::new(0.12, 1.2, 0.12), CHARCOAL);
        });
        bare_bulb(g, c, &fr, fr.len * 0.3, 2.3);
        rooms.push(shed);
    }
    // Sacks of cans and bottles in rows by the shed.
    g.lod(Lod::Mid, |g| {
        for j in 0..rng.random_range(4..10) {
            let p = Vec2::new(area.u1 - 1.0 - (j % 5) as f32 * 1.0, area.v1 - 5.5 - (j / 5) as f32 * 1.0);
            if spot(c, p, 0.7) {
                let col = any(rng, &[[0.86, 0.86, 0.80], [0.20, 0.36, 0.66], [0.70, 0.66, 0.50]]);
                let sh = rng.random_range(0.75..0.95);
                f.bx_yaw(g, Tarp, p.x, sh * 0.5, p.y, Vec3::new(0.85, sh, 0.85), mul(col, 1.0 - 0.3 * c.wear), rng.random_range(-0.3..0.3));
            }
        }
    });
    // Heaps and wrecks over the rest.
    let heaps = ((area.w() * area.d()) / 220.0).clamp(1.0, 5.0) as i32;
    for _ in 0..heaps {
        if let Some(p) = clear_spot(c, rng, &rooms, 3.6) {
            if spot(c, p, 3.0) {
                let (pr, ph) = (rng.random_range(2.0..3.2), rng.random_range(1.4..2.4));
                scrap_heap(g, c, rng, p, pr, ph);
                rooms.push(R::new(p.x - 3.0, p.x + 3.0, p.y - 3.0, p.y + 3.0));
            }
        }
    }
    for _ in 0..2 {
        if let Some(p) = clear_spot(c, rng, &rooms, 2.5) {
            let yaw = rng.random_range(-0.5..0.5);
            if vehicle_fits(c, p, yaw, Vehicle::Wreck) {
                let paint = mul(pick(rng, &pal.car_colors), 0.6);
            vehicle(g, f, pal, rng, p, 0.0, yaw, Vehicle::Wreck, paint);
                rooms.push(R::new(p.x - 2.6, p.x + 2.6, p.y - 2.6, p.y + 2.6));
            }
        }
    }
    if let Some(p) = clear_spot(c, rng, &rooms, 1.0) {
        if spot(c, p, 1.0) {
            tyres(g, f, p, rng.random_range(3..7));
        }
    }
    if let Some(p) = clear_spot(c, rng, &rooms, 1.0) {
        fire_barrel(g, c, p);
    }
    weeds(g, c, rng, &yard_poly, 8);
}

/// A heap of scrap at lot-local `p`: bent sheet, drums, fridges, doors and
/// tarps tipped any way, higher in the middle; each piece a few
/// millimetre apart from the others, so no two tops share a plane. Tyres
/// on the side.
fn scrap_heap(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32) {
    let f = &c.frame;
    let n = (r * r * 1.3) as i32 + 5;
    for k in 0..n {
        let d = rng.random_range(0.0..r);
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        let q = p + Vec2::new(a.cos(), a.sin()) * d;
        let size = Vec3::new(rng.random_range(0.5..2.0), rng.random_range(0.2..1.0), rng.random_range(0.5..1.8));
        if !spot(c, q, size.x.max(size.z) * 0.75) {
            continue;
        }
        // Tops on a 2 cm grid, each piece its own 0.6 mm above it (and
        // clear of the tyres' tops, on even centimetres).
        let top = (1.0 - d / r) * h * rng.random_range(0.3..1.0) + size.y;
        let y = (top / 0.02).floor() * 0.02 + 0.007 + k.min(17) as f32 * 0.0006 - size.y;
        let (s, col) = match rng.random_range(0..4) {
            0 | 1 => sheet(c, rng),
            2 => (Tarp, mul(any(rng, &TARPS), 0.7)),
            _ => (Metal, mul([0.80, 0.80, 0.76], 0.7)),
        };
        f.bx_yaw(g, s, q.x, y + size.y * 0.5, q.y, size, mul(col, 0.85), rng.random_range(0.0..3.0));
    }
    let tp = p + Vec2::new(r * 0.7, 0.0);
    if spot(c, tp, 0.6) {
        tyres(g, f, tp, rng.random_range(2..5));
    }
}

/// A workshop under a lean-to: a block shed with its sign, a deep sheet
/// roof on posts over the work in front (a car up on blocks, the bench,
/// an engine hoist, drums and tyres), a bulb under it; wrecks for parts
/// behind.
pub fn lean_to_workshop(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard_poly = yard(g, c, 0.3, Dirt, mul(dirt(c), 0.85));
    let area = R::of(&c.poly);
    let sw = (area.w() * 0.7).clamp(5.0, 14.0);
    let v0 = area.v0 + (area.d() * 0.35).clamp(4.5, 7.0);
    let Some(shed) = fit(c, R::new(area.c().x - sw * 0.5, area.c().x + sw * 0.5, v0, v0 + (area.d() * 0.3).clamp(3.5, 6.0)), 3.0) else { return tin_shacks(g, c, rng) };
    let h = 3.0;
    let half = coin(rng, 0.5);
    block(g, c, Concrete, &shed, 0.0, h, mul(BLOCK, 0.85 - 0.15 * c.wear));
    let fr = shed.front();
    if half {
        // Painted to waist height, the rest raw block.
        fr.decal(g, f, Panel, 0.0, 0.6, 0.012, fr.len, 1.2, faded(c, rng));
    }
    roller_door(g, c, &fr, -fr.len * 0.15, (fr.len * 0.45).min(3.6), 2.4, mul(faded(c, rng), 0.85));
    hole_window(g, c, rng, &fr, fr.len * 0.33, 1.6, 1.0, 0.7);
    for face in [shed.right(), shed.left(), shed.back()] {
        patches(g, c, rng, &face, 0.0, h);
    }
    grime(g, c, rng, &fr, 0.0, h, 1.5);
    lean_roof(g, c, rng, &shed, h, 0.2);
    painted_sign(g, c, rng, &fr, 0.0, h - 0.35, 0.06, (fr.len * 0.55).min(4.0), 0.55);
    // The lean-to.
    let lean = R::new(shed.u0, shed.u1, shed.v0 - (shed.v0 - area.v0 - 0.6).clamp(2.5, 5.0), shed.v0);
    let mut busy: Vec<R> = vec![shed];
    if on_lot(c, &lean) && lean.d() > 2.0 {
        let (s, col) = sheet(c, rng);
        g.part(s).slab(f.at(lean.c().x, h - 0.2, lean.v1), f.at(lean.c().x, 2.5, lean.v0 - 0.2), lean.w(), 0.05, rgba(mul(col, 0.9)));
        let n = ((lean.w() / 3.5) as i32).max(1);
        for k in 0..=n {
            let u = lean.u0 + 0.1 + (lean.w() - 0.2) * k as f32 / n as f32;
            f.bx(g, Rust, u, 1.25, lean.v0 + 0.1, Vec3::new(0.1, 2.5, 0.1), mul(pal.rust, 0.8));
        }
        bare_bulb(g, c, &fr, fr.len * 0.15, 2.6);
        // A car up on blocks under it.
        let cp = Vec2::new(lean.c().x - lean.w() * 0.15, lean.c().y);
        if vehicle_fits(c, cp, 0.0, Vehicle::Car) && lean.w() > 6.0 {
            for (du, dv) in [(-1.3f32, -0.7f32), (1.3, -0.7), (-1.3, 0.7), (1.3, 0.7)] {
                f.bx(g, Concrete, cp.x + du, 0.15, cp.y + dv, Vec3::new(0.4, 0.3, 0.3), mul(BLOCK, 0.8));
            }
            let paint = mul(pick(rng, &pal.car_colors), 0.75);
            vehicle(g, f, pal, rng, cp, 0.3, 0.0, Vehicle::Wreck, paint);
        }
        g.lod(Lod::Mid, |g| {
            // The bench along the wall, an engine hoist.
            let bu = lean.u1 - 1.4;
            f.bx(g, Wood, bu, 0.45, lean.v1 - 0.5, Vec3::new(2.2, 0.9, 0.7), mul(pal.wood, 0.6));
            let hp = Vec2::new(lean.u1 - 2.6, lean.c().y);
            for s in [-0.5f32, 0.5] {
                g.part(Rust).beam(f.at(hp.x + s, 0.0, hp.y - 0.4), f.at(hp.x, 2.0, hp.y), 0.08, rgba(mul(pal.rust, 0.8)));
            }
            g.part(Rust).beam(f.at(hp.x, 2.0, hp.y), f.at(hp.x - 1.2, 1.7, hp.y), 0.07, rgba(mul(pal.rust, 0.8)));
            g.part(Matte).wire(f.at(hp.x - 1.2, 1.7, hp.y), f.at(hp.x - 1.2, 0.9, hp.y), 0.0, 0.02, rgba(DARK));
        });
        g.lod(Lod::Near, |g| {
            for k in 0..3 {
                let p = Vec2::new(lean.u0 + 0.5 + k as f32 * 0.7, lean.v1 - 0.5);
                g.part(Metal).cylinder(f.to_world(p.x, p.y), 0.29, 0.29, 0.0, 0.88, 8, rgba(any(rng, &[[0.20, 0.36, 0.62], [0.62, 0.16, 0.12], [0.18, 0.18, 0.18]])));
            }
        });
        busy.push(lean);
    }
    let tp = Vec2::new(area.u0 + 1.0, area.v0 + 1.2);
    if spot(c, tp, 0.8) {
        tyres(g, f, tp, rng.random_range(3..6));
    }
    // The family's shacks behind, past a yard of parts.
    let mut eaves = Vec::new();
    fill(g, c, rng, &R::new(area.u0, area.u1, shed.v1 + 6.0, area.v1), &mut busy, &mut eaves);
    // Wrecks for parts behind.
    if let Some(p) = clear_spot(c, rng, &busy, 2.5) {
        let yaw = rng.random_range(-0.6..0.6);
        if p.y > shed.v1 && vehicle_fits(c, p, yaw, Vehicle::Wreck) {
            let paint = mul(pick(rng, &pal.car_colors), 0.55);
            vehicle(g, f, pal, rng, p, 0.0, yaw, Vehicle::Wreck, paint);
        }
    }
    if let Some(p) = clear_spot(c, rng, &busy, 1.0) {
        fire_barrel(g, c, p);
    }
    weeds(g, c, rng, &yard_poly, 6);
}

/// The trades with a look of their own in a Patchwork district: the
/// mechanic and the bike repairer work under a lean-to, the tow yard is a
/// scrap yard.
pub fn trade_style(kind: Option<nd_sim::BusinessKind>) -> Option<super::Style> {
    use super::Style;
    use nd_sim::BusinessKind as B;
    match kind? {
        B::Mechanic | B::BikeShop => Some(Style::LeanToWorkshop),
        B::TowYard => Some(Style::ScrapYard),
        _ => None,
    }
}

// ── the big man's compound ───────────────────────────────────────────────────

/// The big man's compound, for a family's two plots or a billionaire's
/// four: a block wall painted round the lot with razor wire coiled on top
/// and a steel gate; inside, on concrete, a bright-painted concrete house
/// of two storeys (three on an estate) with a columned porch, a balcony,
/// bars on the windows, a big dish on the roof and tanks on a stand; a
/// caged generator, a car; and still a tin room at the back and an
/// extension half built, the bars sticking out of it. Floodlights on the
/// wall and a bulb over the gate after dark.
pub fn big_man_compound(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(site) = fit(c, R::of(&offset_poly(&c.lot_poly, 0.6)), 12.0) else { return patch_stack(g, c, rng) };
    let big = site.w() * site.d() > 1800.0;
    // Concrete inside the walls.
    ground(g, f, Paving, &offset_poly(&c.lot_poly, 0.4), 0.03, mul(BLOCK, 0.75 - 0.1 * c.wear));
    let paint = mul(any(rng, &LOUD), 1.0 - 0.3 * c.wear);
    let trim = mul([0.92, 0.90, 0.84], 1.0 - 0.25 * c.wear);
    // The wall, its gate and the wire.
    let wall_h = 2.5;
    let wall = offset_poly(&c.lot_poly, 0.5);
    let fi = c.front_face_index(&wall);
    let wall_col = mul(mix(paint, trim, 0.55), 0.95);
    let mut gate_at: Option<(Face, f32)> = None;
    for (i, face) in c.faces(&wall).iter().enumerate() {
        let runs: Vec<(f32, f32)> = if Some(i) == fi && face.len > 10.0 {
            let gt = rng.random_range(-0.25..0.25) * face.len;
            gate_at = Some((*face, gt));
            vec![(-face.len * 0.5, gt - 2.0), (gt + 2.0, face.len * 0.5)]
        } else {
            vec![(-face.len * 0.5, face.len * 0.5)]
        };
        for (t0, t1) in runs {
            if t1 - t0 < 0.3 {
                continue;
            }
            let t = (t0 + t1) * 0.5;
            face.bx(g, f, Plaster, t, wall_h * 0.5, -0.12, Vec3::new(t1 - t0, wall_h, 0.24), wall_col);
            face.decal(g, f, Panel, t, 0.45, 0.012, t1 - t0, 0.9, mul(paint, 0.8));
            g.lod(Lod::Mid, |g| {
                // Razor wire: a coil along the top on brackets.
                let a = f.at2(face.at(t0 + 0.2, -0.12), wall_h + 0.3);
                let b = f.at2(face.at(t1 - 0.2, -0.12), wall_h + 0.3);
                g.part(Metal).tube(a, b, 0.17, 5, rgba(mul(pal.steel, 0.9)));
                let n = ((t1 - t0) / 3.0) as i32;
                for k in 0..=n {
                    let tt = t0 + 0.2 + (t1 - t0 - 0.4) * k as f32 / n.max(1) as f32;
                    face.bx(g, f, Metal, tt, wall_h + 0.2, -0.12, Vec3::new(0.04, 0.4, 0.04), DARK);
                }
            });
        }
    }
    if let Some((face, gt)) = gate_at {
        let gc = mul(any(rng, &[[0.10, 0.10, 0.10], [0.70, 0.60, 0.20], [0.20, 0.28, 0.40]]), 1.0 - 0.2 * c.wear);
        for s in [-1.0f32, 1.0] {
            face.bx(g, f, Metal, gt + s * 1.0, 1.15, -0.12, Vec3::new(1.95, 2.3, 0.06), gc);
            // Over the wall's ends, not flush with them.
            face.bx(g, f, Plaster, gt + s * 2.1, 1.4, -0.12, Vec3::new(0.3, 2.8, 0.3), trim);
        }
        bare_bulb(g, c, &face, gt + 2.1, 2.7);
    }
    // The house.
    let hw = (site.w() * 0.55).clamp(9.0, if big { 22.0 } else { 15.0 });
    let hd = (site.d() * 0.38).clamp(7.0, if big { 14.0 } else { 10.0 });
    let v0 = site.v0 + (site.d() * 0.3).clamp(5.0, 12.0);
    let Some(house) = fit(c, R::new(site.c().x - hw * 0.5, site.c().x + hw * 0.5, v0, v0 + hd), 6.0) else { return patch_stack(g, c, rng) };
    let fh = 3.2;
    let floors = if big { 3 } else { 2 };
    for k in 0..floors {
        let y0 = k as f32 * fh;
        // The top storey of an estate is smaller, a room on the roof.
        let r = if k == 2 { R::new(house.u0 + house.w() * 0.35, house.u1 - 0.8, house.v0 + 1.5, house.v1 - 0.8) } else { house };
        block(g, c, Plaster, &r, y0, y0 + fh, paint);
        // A trim band at each floor.
        block(g, c, Concrete, &R::new(r.u0 - 0.06, r.u1 + 0.06, r.v0 - 0.06, r.v1 + 0.06), y0 + fh - 0.27, y0 + fh - 0.02, trim);
        for (fi, face) in r.faces().iter().enumerate() {
            let n = ((face.len - 1.5) / 3.0).floor() as i32;
            for j in 0..n.max(0) {
                let t = -(face.len - 1.5) * 0.5 + (j as f32 + 0.5) * (face.len - 1.5) / n as f32;
                if k == 0 && fi == 0 && t.abs() < 1.6 {
                    continue;
                }
                if coin(rng, c.lit_chance()) {
                    face.glaze(g, c, t, y0 + 1.5, PANE, 1.3, 1.3, Pane::Lit(mul(pick(rng, &c.style.window_colors), rng.random_range(0.6..0.9))));
                } else if coin(rng, 0.2) {
                    tv(g, c, face, t, y0 + 1.5, 1.3, 1.3);
                } else {
                    face.glaze(g, c, t, y0 + 1.5, PANE, 1.3, 1.3, Pane::Dark);
                }
                g.lod(Lod::Mid, |g| {
                    face.window_frame(g, c, t, y0 + 1.5, 1.3, 1.3, 0.14, trim);
                    // Burglar bars over every window.
                    for b in -3..=3 {
                        face.bx(g, f, Matte, t + b as f32 * 0.19, y0 + 1.5, 0.1, Vec3::new(0.025, 1.3, 0.025), DARK);
                    }
                });
            }
        }
    }
    let top_y = floors.min(2) as f32 * fh;
    // A parapet round the flat roof.
    cornice(g, c, &house.poly(), top_y + 0.3, 0.6, 0.08, Plaster, trim);
    // The porch: two round columns and a slab over the door.
    let fr = house.front();
    fr.decal(g, f, Wood, 0.0, 1.25, 0.03, 1.4, 2.5, mul(pal.wood, 0.55));
    fr.bx(g, f, Concrete, 0.0, fh - 0.2, 1.4, Vec3::new(3.6, 0.2, 2.8), trim);
    for s in [-1.0f32, 1.0] {
        let p = fr.at(s * 1.5, 2.6);
        g.part(Plaster).cylinder(f.to_world(p.x, p.y), 0.18, 0.16, 0.0, fh - 0.3, 10, rgba(trim));
    }
    // The balcony over it.
    balcony(g, c, &fr, 0.0, fh + 0.05, 4.0, 1.1, trim, mul(trim, 0.9));
    // On the roof: the big dish; tanks on a stand beside the house.
    dish(g, f, Vec2::new(house.u0 + 1.5, house.v1 - 1.5), top_y, 1.3, mul([0.86, 0.86, 0.84], 1.0 - 0.25 * c.wear));
    let tp = Vec2::new(house.u1 + 1.6, house.v1 - 1.2);
    if spot(c, tp, 1.4) {
        tank_stand(g, c, rng, tp, 3.2);
        if big {
            tank_stand(g, c, rng, tp - Vec2::new(0.0, 1.6), 3.2);
        }
    }
    // The generator in its cage, by the wall.
    let gp = Vec2::new(site.u0 + 1.4, house.v0 - 1.4);
    if spot(c, gp, 1.2) {
        f.bx(g, Metal, gp.x, 0.55, gp.y, Vec3::new(1.6, 1.1, 0.9), mul([0.30, 0.46, 0.24], 1.0 - 0.3 * c.wear));
        g.lod(Lod::Mid, |g| {
            g.part(Matte).cylinder(f.to_world(gp.x + 0.5, gp.y), 0.06, 0.06, 1.1, 1.8, 6, rgba(DARK));
            for (du, dv) in [(-1.0f32, -0.7f32), (1.0, -0.7), (1.0, 0.7), (-1.0, 0.7)] {
                f.bx(g, Matte, gp.x + du, 0.8, gp.y + dv, Vec3::new(0.04, 1.6, 0.04), CHARCOAL);
            }
            f.bx(g, Matte, gp.x, 1.62, gp.y, Vec3::new(2.04, 0.04, 1.44), CHARCOAL);
        });
    }
    // The car on the drive.
    let cp = Vec2::new(house.u1 - 2.5, (site.v0 + house.v0) * 0.5);
    if vehicle_fits(c, cp, std::f32::consts::FRAC_PI_2, Vehicle::Car) && cp.y < house.v0 - 3.0 {
        let paint = any(rng, &[[0.06, 0.06, 0.07], [0.80, 0.80, 0.82], [0.30, 0.04, 0.06]]);
        vehicle(g, f, pal, rng, cp, 0.03, std::f32::consts::FRAC_PI_2, Vehicle::Car, paint);
    }
    // Still informal: a tin room at the back, an extension half built.
    let mut rooms = vec![house];
    let back = R::new(site.u0 + 0.6, site.u0 + 0.6 + rng.random_range(3.0..4.5), site.v1 - 3.6, site.v1 - 0.6);
    if back.v0 > house.v1 + 1.0 && on_lot(c, &back) {
        room(g, c, rng, &back, 0.0, 2.4, Some(0), true);
        rooms.push(back);
        laundry(g, c, rng, p3(back.u1, 1.9, back.v0 + 0.3), p3(house.u0 + 1.0, 1.9, house.v1 + 0.3));
    }
    let ext = R::new(house.u1, house.u1 + 3.0, house.v0 + 1.0, house.v0 + 4.0);
    if on_lot(c, &ext) && ext.u1 < site.u1 - 0.4 {
        block(g, c, Concrete, &ext, 0.0, 1.6, mul(BLOCK, 0.9));
        g.lod(Lod::Mid, |g| {
            for (u, v) in [(ext.u1 - 0.15, ext.v0 + 0.15), (ext.u1 - 0.15, ext.v1 - 0.15)] {
                for d in [-0.07f32, 0.07] {
                    f.bx(g, Rust, u + d, 2.2, v, Vec3::new(0.025, 1.2, 0.025), mul(pal.rust, 0.7));
                }
            }
        });
    }
    // Floodlights on the wall's corners, lit after dark.
    for p in [Vec2::new(site.u0 + 0.3, site.v0 + 0.3), Vec2::new(site.u1 - 0.3, site.v1 - 0.3)] {
        if spot(c, p, 0.2) {
            f.bx(g, Matte, p.x, wall_h + 0.5, p.y, Vec3::new(0.1, 1.0, 0.1), CHARCOAL);
            let from = g.part(Glow).positions.len();
            f.bx(g, Glow, p.x, wall_h + 1.05, p.y, Vec3::new(0.4, 0.22, 0.3), [2.2, 2.1, 1.8]);
            mark_glow(g, from, NIGHT_MARK);
        }
    }
    if big {
        // An estate's guesthouse, painted to match.
        let gw = (site.w() * 0.25).clamp(5.0, 9.0);
        let gh = R::new(site.u1 - gw - 0.6, site.u1 - 0.6, site.v1 - 7.0, site.v1 - 0.8);
        if gh.u0 > house.u1 + 4.0 && on_lot(c, &gh) {
            block(g, c, Plaster, &gh, 0.0, fh, mix(paint, trim, 0.3));
            cornice(g, c, &gh.poly(), fh + 0.25, 0.5, 0.08, Plaster, trim);
            for face in gh.faces() {
                if face.len > 3.0 {
                    face.glaze(g, c, 0.0, 1.5, PANE, 1.2, 1.2, if coin(rng, c.lit_chance()) { Pane::Lit(mul(pick(rng, &c.style.window_colors), 0.7)) } else { Pane::Dark });
                }
            }
            rooms.push(gh);
        }
    }
    let _ = rooms;
}
