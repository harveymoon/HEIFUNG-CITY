//! [r4-glass] Glass Campus, Founders' Campus's look: the garage start-up
//! grown into a district. Student flats with bright coloured panels and
//! deck-access galleries, co-living towers with glass lounges hung off them;
//! glass labs in exposed steel frames, a yard of cheap prefab start-up
//! sheds, atrium offices, a shared quad with a screen and a café pavilion;
//! for its small industry chamfered concrete research labs and fenced
//! server halls; and for the founders who cash out, a glass pavilion house.
//!
//! Lived in, not a render: the curtain walls are dark tinted glass smeared
//! and streaked (more with wear), the steel is exposed and rusting at its
//! feet, there is plant and cooling on every roof, cable trays along the
//! walls, bike racks full and hire scooters dropped wherever, a security
//! fence round the server halls. By night the offices light a floor at a
//! time (window lights the renderer switches), the atria and lounges glow
//! all night (base glow marked `NIGHT_MARK`, lit only after dark), the
//! lamps and floodlights come on, and the screens show as signs do.
//!
//! Lot-local as every style: `u` along the street, `v` in from it. Everything
//! comes from the building's seed, the same from every camera. Mullions,
//! rails, braces, ducts, racks and fans are mid detail; smears, seams,
//! cables, wheels and scooters near detail.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Extras, Face};
use crate::depth::PANE;
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Tinted curtain-wall glass: teal, bronze, blue-black, smoke.
const TINTS: [[f32; 3]; 4] = [[0.10, 0.16, 0.18], [0.17, 0.14, 0.11], [0.09, 0.11, 0.16], [0.14, 0.15, 0.16]];
/// What the weather smears onto the glass.
const SMEAR: [f32; 3] = [0.34, 0.33, 0.30];
/// The student flats' panels: orange, lime, cyan, magenta, yellow, blue.
const BRIGHTS: [[f32; 3]; 6] = [[0.92, 0.42, 0.10], [0.55, 0.80, 0.18], [0.10, 0.62, 0.78], [0.88, 0.20, 0.42], [0.96, 0.80, 0.12], [0.30, 0.36, 0.80]];
/// Spandrels and fins: graphite, warm grey, off-white.
const SPANDRELS: [[f32; 3]; 3] = [[0.16, 0.17, 0.18], [0.42, 0.41, 0.39], [0.72, 0.72, 0.70]];
/// Exposed structural steel: graphite, oxide red, galvanised.
const STEELS: [[f32; 3]; 3] = [[0.20, 0.21, 0.22], [0.40, 0.16, 0.12], [0.52, 0.54, 0.55]];
/// Hire scooters and bikes: lime, orange, cyan, pink.
const HIRE: [[f32; 3]; 4] = [[0.52, 0.86, 0.16], [0.96, 0.46, 0.10], [0.10, 0.70, 0.84], [0.92, 0.30, 0.56]];
/// An atrium or a lounge lit all night.
const ATRIUM: [f32; 3] = [1.6, 1.45, 1.15];
/// A co-living lounge: warmer.
const LOUNGE: [f32; 3] = [1.7, 1.15, 0.75];
/// A lamp or a floodlight.
const LAMP: [f32; 3] = [2.0, 1.85, 1.5];
/// Server status strips: cold cyan.
const STATUS: [f32; 3] = [0.35, 1.5, 1.9];

/// Tinted glass, greyed by the smears with wear.
fn tint(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mix(TINTS[rng.random_range(0..TINTS.len())], SMEAR, 0.08 + 0.3 * c.wear)
}

fn bright(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(mix(BRIGHTS[rng.random_range(0..BRIGHTS.len())], [0.55, 0.55, 0.52], 0.1 + 0.35 * c.wear), 1.0 - 0.2 * c.wear)
}

fn spandrel(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(SPANDRELS[rng.random_range(0..SPANDRELS.len())], 1.0 - 0.2 * c.wear)
}

fn steel(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mix(STEELS[rng.random_range(0..STEELS.len())], c.palette.rust, 0.35 * c.wear)
}

fn concrete(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(pick(rng, &c.palette.concrete), 1.0 - 0.2 * c.wear)
}

/// A light marked to show only after dark.
fn night_box(g: &mut Geometry, c: &Ctx, u: f32, y: f32, v: f32, size: Vec3, col: [f32; 3]) {
    let from = g.part(Glow).positions.len();
    c.frame.bx(g, Glow, u, y, v, size, mul(col, 1.0 - 0.3 * c.wear));
    mark_glow(g, from, NIGHT_MARK);
}

/// A slim lamp post at lot-local `p`, lit after dark; nothing where it
/// would hang over the street.
fn lamp(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 0.5 {
        return;
    }
    let f = &c.frame;
    g.part(Matte).cylinder(f.to_world(p.x, p.y), 0.07, 0.06, 0.0, h, 6, rgba(CHARCOAL));
    g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x, h + 0.05, p.y, Vec3::new(0.8, 0.08, 0.22), DARK));
    night_box(g, c, p.x, h - 0.04, p.y, Vec3::new(0.7, 0.05, 0.16), LAMP);
}

// ── the curtain wall ──────────────────────────────────────────────────────────

/// Curtain wall on one face from `y0`, `floors` of `fh`: a spandrel at
/// each slab, office floors lit behind the glass (one light per floor and
/// face, so the renderer switches a floor at a time; some only a stretch
/// lit), mullions (mid detail), smears and rain streaks (near).
#[allow(clippy::too_many_arguments)]
fn curtain_face(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, span_col: [f32; 3]) {
    let f = &c.frame;
    if face.len < 1.2 || floors < 1 {
        return;
    }
    let w = face.len - 0.2;
    let lit = c.lit_chance();
    for k in 0..floors {
        let yb = y0 + k as f32 * fh;
        face.decal(g, f, Panel, 0.0, yb + 0.3, 0.035, w, 0.6, span_col);
        if coin(rng, lit * 0.85) {
            let span = if coin(rng, 0.6) { w - 0.3 } else { rng.random_range(0.3..0.75) * w };
            let room = ((w - span) * 0.5).max(0.0);
            let t = if room > 0.01 { rng.random_range(-room..room) } else { 0.0 };
            let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.4..0.8));
            face.glaze(g, c, t, yb + 0.6 + (fh - 0.6) * 0.5, PANE, span, fh - 1.0, Pane::Lit(col));
        }
    }
    let h = floors as f32 * fh;
    let mullion = mul(SPANDRELS[0], 0.8);
    g.lod(Lod::Mid, |g| {
        let n = (face.len / 1.6).floor() as i32;
        for i in 1..n {
            let t = -face.len * 0.5 + i as f32 * face.len / n as f32;
            face.decal(g, f, Matte, t, y0 + h * 0.5, 0.065, 0.09, h - 0.1, mullion);
        }
    });
    // Smears and streaks, each at its own depth on the glass.
    let n = ((0.6 + 3.0 * c.wear) * face.len / 14.0).ceil() as i32;
    g.lod(Lod::Near, |g| {
        for i in 0..n.min(6) {
            let t = rng.random_range(-0.4..0.4) * face.len;
            let sh = rng.random_range(1.5..(h * 0.7).max(1.6));
            let col = mix(SMEAR, [0.5, 0.48, 0.42], rng.random::<f32>());
            face.decal(g, f, Matte, t, y0 + h - sh * 0.5, 0.0225 + i as f32 * 0.0011, rng.random_range(0.4..2.2), sh, col);
        }
    });
}

/// A curtain-walled block on `r`: the extrusion's walls are the tinted
/// glass, every face dressed by [`curtain_face`], a coping slab on top.
/// Returns its top.
#[allow(clippy::too_many_arguments)]
fn curtain_block(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y0: f32, floors: i32, fh: f32, glass: [f32; 3], span_col: [f32; 3]) -> f32 {
    let h = y0 + floors as f32 * fh;
    block(g, c, Glass, r, y0, h, glass);
    for face in r.faces() {
        curtain_face(g, c, rng, &face, y0, floors, fh, span_col);
    }
    block(g, c, Concrete, &r.inset(-0.12), h, h + 0.4, mul(span_col, 0.9));
    h + 0.4
}

/// Posts every metre and a half round a roof or a deck and two rails
/// (near detail).
fn rails(g: &mut Geometry, c: &Ctx, r: &R, y: f32, col: [f32; 3]) {
    let f = &c.frame;
    g.lod(Lod::Near, |g| {
        for face in r.faces() {
            let n = (face.len / 1.5).ceil().max(1.0) as i32;
            for i in 0..=n {
                let t = -face.len * 0.5 + i as f32 * face.len / n as f32;
                face.bx(g, f, Matte, t, y + 0.55, -0.1, Vec3::new(0.05, 1.1, 0.05), col);
            }
            for yy in [y + 0.5, y + 1.08] {
                face.bx(g, f, Matte, 0.0, yy, -0.1, Vec3::new(face.len, 0.04, 0.04), col);
            }
        }
    });
}

/// Rooftop plant inside `r` at `y`: chillers with their fans, round
/// cooling units, a duct run between them, a dish or a mast; rails round
/// the roof's edge.
fn roof_plant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32) {
    let f = &c.frame;
    let pal = c.palette;
    rails(g, c, &r.inset(0.25), y, mul(pal.steel, 0.7));
    let inner = r.inset(1.8);
    if inner.w() < 1.5 || inner.d() < 1.5 {
        return;
    }
    let slots = ((inner.w() / 4.6).floor() as i32).clamp(1, 5);
    let step = inner.w() / slots as f32;
    let plant = mul(pal.steel, 0.85 - 0.2 * c.wear);
    let mut prev: Option<Vec3> = None;
    for i in 0..slots {
        let u = inner.u0 + (i as f32 + 0.5) * step;
        let v = inner.c().y + rng.random_range(-0.3..0.3) * (inner.d() - 2.0).max(0.0);
        let p = Vec2::new(u, v);
        match rng.random_range(0..4) {
            0 | 1 => chiller(g, c, p, y, (step - 1.0).clamp(1.6, 3.6), 1.8f32.min(inner.d()), 0.0, plant),
            2 => {
                let w = f.to_world(p.x, p.y);
                g.part(Metal).cylinder_capped(w, 1.0, 0.9, y, y + 2.0, 10, false, true, rgba(plant));
                g.lod(Lod::Mid, |g| g.part(Matte).cylinder(w, 0.7, 0.7, y + 2.0, y + 2.25, 10, rgba(DARK)));
            }
            _ => {
                f.bx(g, Panel, p.x, y + 0.9, p.y, Vec3::new((step - 1.2).clamp(1.2, 2.6), 1.8, 1.4f32.min(inner.d())), mul(plant, 0.9));
                g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x, y + 0.9, p.y - 0.72f32.min(inner.d() * 0.5 + 0.01), Vec3::new(1.0, 1.2, 0.04), DARK));
            }
        }
        let top = f.at(u, y + 0.5, inner.v1 - 0.2);
        if let Some(q) = prev {
            g.lod(Lod::Mid, |g| g.part(Metal).beam(q, top, 0.45, rgba(mul(plant, 1.1))));
        }
        prev = Some(top);
    }
    if coin(rng, 0.5) {
        dish(g, f, Vec2::new(inner.u0 + 0.6, inner.v1 - 0.4), y, 0.7, pal.steel);
    } else {
        g.lod(Lod::Mid, |g| antenna_mast(g, c, Vec2::new(inner.u1 - 0.3, inner.v1 - 0.3), y, rng.random_range(3.0..6.0)));
    }
}

/// An exposed steel stair core on lot-local rectangle `r` from `y0`, `h`
/// tall: corner posts (base), a ring beam and a diagonal brace on each face
/// per floor (mid), flights inside (near), a lamp on each landing after
/// dark.
#[allow(clippy::too_many_arguments)]
fn steel_core(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y0: f32, h: f32, fh: f32) {
    let f = &c.frame;
    let col = steel(c, rng);
    for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u1, r.v1), (r.u0, r.v1)] {
        f.bx(g, Matte, u, y0 + h * 0.5, v, Vec3::new(0.3, h, 0.3), col);
    }
    let levels = ((h / fh).floor() as i32).max(1);
    let lh = h / levels as f32;
    g.lod(Lod::Mid, |g| {
        for k in 1..=levels {
            let y = y0 + k as f32 * lh;
            for face in r.faces() {
                face.bx(g, f, Matte, 0.0, y - 0.12, 0.0, Vec3::new(face.len, 0.22, 0.2), col);
                let (a, b) = (face.at(-face.len * 0.5, 0.0), face.at(face.len * 0.5, 0.0));
                let (a, b) = if k % 2 == 0 { (a, b) } else { (b, a) };
                g.part(Matte).beam(f.at2(a, y - lh + 0.2), f.at2(b, y - 0.25), 0.12, rgba(col));
            }
            f.bx(g, Metal, r.c().x, y - 0.3, r.v1 - 0.6, Vec3::new(r.w() - 0.4, 0.12, 1.0), mul(col, 0.8));
        }
    });
    g.lod(Lod::Near, |g| {
        for k in 0..levels {
            let y = y0 + k as f32 * lh;
            let (va, vb) = if k % 2 == 0 { (r.v0 + 0.4, r.v1 - 1.1) } else { (r.v1 - 1.1, r.v0 + 0.4) };
            let u = r.c().x + if k % 2 == 0 { -0.35 } else { 0.35 };
            g.part(Metal).slab(f.at(u, y + 0.1, va), f.at(u, y + lh - 0.35, vb), 0.8, 0.12, rgba(mul(col, 0.7)));
        }
    });
    for k in 1..=levels.min(16) {
        night_box(g, c, r.c().x, y0 + k as f32 * lh - 0.45, r.v1 - 0.6, Vec3::new(0.3, 0.06, 0.3), LAMP);
    }
}

/// A glass atrium `w` wide out `out` from a face at `t`, from `y0` to `y1`:
/// tinted glass by day; after dark lit all night (base glow just round the
/// glass, marked `NIGHT_MARK`), its frame and floor lines (mid) dark over
/// the light; a screen hung inside it on the larger ones.
#[allow(clippy::too_many_arguments)]
fn atrium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, out: f32, y0: f32, y1: f32, glass: [f32; 3], fh: f32, light: [f32; 3]) {
    let f = &c.frame;
    let h = y1 - y0;
    if w < 2.0 || h < 2.0 || out < 0.5 {
        return;
    }
    face.bx(g, f, Glass, t, y0 + h * 0.5, out * 0.5, Vec3::new(w, h, out), mul(glass, 1.2));
    let from = g.part(Glow).positions.len();
    face.bx(g, f, Glow, t, y0 + h * 0.5, out * 0.5 + 0.03, Vec3::new(w + 0.06, h + 0.06, out + 0.02), mul(light, 1.0 - 0.3 * c.wear));
    mark_glow(g, from, NIGHT_MARK);
    // A roof slab over it.
    face.bx(g, f, Concrete, t, y1 + 0.15, out * 0.5 + 0.05, Vec3::new(w + 0.4, 0.3, out + 0.3), mul(SPANDRELS[1], 0.9));
    let frame = mul(SPANDRELS[0], 0.7);
    let front = out + 0.08;
    g.lod(Lod::Mid, |g| {
        let n = ((w / 1.8).floor() as i32).max(1);
        for i in 0..=n {
            let tt = t - w * 0.5 + 0.05 + i as f32 * (w - 0.1) / n as f32;
            face.decal(g, f, Matte, tt, y0 + h * 0.5, front, 0.12, h, frame);
        }
        let floors = ((h / fh).floor() as i32).max(1);
        for k in 1..floors {
            face.decal(g, f, Matte, t, y0 + k as f32 * h / floors as f32, front + 0.004, w, 0.18, frame);
        }
    });
    if w > 6.0 && h > 8.0 {
        let screen = Face::from_edge(face.at(t - w * 0.5, out + 0.06), face.at(t + w * 0.5, out + 0.06));
        let screen = if screen.out.dot(face.out) < 0.0 { screen.reversed() } else { screen };
        led_screen(g, c, rng, &screen, 0.0, y0 + h * 0.62, (w * 0.5).min(8.0), (h * 0.22).min(4.0));
    }
}

/// A row of `n` bikes in a rack from lot-local `p` along `dir`: the rail
/// and frames mid detail, the wheels near. Only where it fits the lot.
fn bike_rack(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, dir: Vec2, n: i32) {
    let f = &c.frame;
    let len = n as f32 * 0.75;
    let end = p + dir * len;
    if n < 1 || !c.inside_local(p) || !c.inside_local(end) || distance_to_boundary(p, &c.lot_poly) < 1.0 || distance_to_boundary(end, &c.lot_poly) < 1.0 {
        return;
    }
    let yaw = dir.y.atan2(dir.x);
    let across = yaw + std::f32::consts::FRAC_PI_2;
    let m = (p + end) * 0.5;
    let rail = mul(c.palette.steel, 0.7);
    let cols: Vec<[f32; 3]> = (0..n).map(|_| if coin(rng, 0.4) { HIRE[rng.random_range(0..HIRE.len())] } else { pick(rng, &c.palette.metal_paint) }).collect();
    g.lod(Lod::Mid, |g| {
        f.bx_yaw(g, Matte, m.x, 0.45, m.y, Vec3::new(len, 0.06, 0.06), rail, yaw);
        for (i, col) in cols.iter().enumerate() {
            let q = p + dir * (i as f32 + 0.5) * 0.75;
            f.bx_yaw(g, Matte, q.x, 0.62, q.y, Vec3::new(1.5, 0.3, 0.05), mul(*col, 1.0 - 0.3 * c.wear), across);
        }
    });
    g.lod(Lod::Near, |g| {
        for i in 0..n {
            let q = p + dir * (i as f32 + 0.5) * 0.75;
            let side = Vec2::new(across.cos(), across.sin());
            for s in [-0.55f32, 0.55] {
                let w = q + side * s;
                let axle = f.dir3(dir) * 0.03;
                let a = f.at2(w, 0.34);
                g.part(Matte).tube(a - axle, a + axle, 0.33, 8, rgba(DARK));
            }
        }
    });
}

/// Hire scooters dropped about lot-local `p`: some on their stands, most
/// lying where they were left (near detail).
fn scooters(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, n: i32) {
    let f = &c.frame;
    let picks: Vec<(Vec2, f32, bool, [f32; 3])> = (0..n)
        .map(|_| {
            let q = p + Vec2::new(rng.random_range(-2.0..2.0), rng.random_range(-1.2..1.2));
            (q, rng.random_range(0.0..std::f32::consts::TAU), coin(rng, 0.35), HIRE[rng.random_range(0..HIRE.len())])
        })
        .collect();
    g.lod(Lod::Near, |g| {
        for (q, yaw, up, col) in picks {
            if !c.inside_local(q) || distance_to_boundary(q, &c.lot_poly) < 0.8 {
                continue;
            }
            let col = mul(col, 1.0 - 0.35 * c.wear);
            let d = Vec2::new(yaw.cos(), yaw.sin());
            if up {
                f.bx_yaw(g, Matte, q.x, 0.12, q.y, Vec3::new(1.05, 0.08, 0.18), mul(col, 0.4), yaw);
                let a = f.at2(q + d * 0.48, 0.15);
                g.part(Matte).beam(a, a + Vec3::Y * 1.0 + f.dir3(d) * 0.08, 0.05, rgba(col));
            } else {
                f.bx_yaw(g, Matte, q.x, 0.1, q.y, Vec3::new(1.05, 0.18, 0.08), mul(col, 0.4), yaw);
                let a = f.at2(q + d * 0.48, 0.12);
                let s = f.dir3(Vec2::new(-d.y, d.x));
                g.part(Matte).beam(a, a + s * 0.95 + f.dir3(d) * 0.08, 0.05, rgba(col));
            }
        }
    });
}

/// A cable tray along a face at `y` from `t0` to `t1` (mid), cables
/// sagging out of it at one end (near).
fn cable_tray(g: &mut Geometry, c: &Ctx, face: &Face, t0: f32, t1: f32, y: f32) {
    let f = &c.frame;
    let len = t1 - t0;
    if len < 1.0 {
        return;
    }
    let col = mul(c.palette.steel, 0.8 - 0.25 * c.wear);
    g.lod(Lod::Mid, |g| {
        face.bx(g, f, Metal, (t0 + t1) * 0.5, y, 0.32, Vec3::new(len, 0.12, 0.4), col);
        let n = (len / 3.0).ceil() as i32;
        for i in 0..=n {
            let t = t0 + i as f32 * len / n as f32;
            face.bx(g, f, Matte, t, y - 0.12, 0.2, Vec3::new(0.06, 0.12, 0.42), mul(col, 0.7));
        }
    });
    g.lod(Lod::Near, |g| {
        let a = f.at2(face.at(t1 - 0.2, 0.4), y + 0.02);
        let b = f.at2(face.at(t1 - 1.2, 0.18), y - 2.2);
        g.part(Matte).wire(a, b, 0.4, 0.05, rgba(DARK));
        let b2 = f.at2(face.at(t1 - 0.6, 0.12), (y - 3.5).max(0.4));
        g.part(Matte).wire(a, b2, 0.2, 0.04, rgba([0.12, 0.10, 0.08]));
    });
}

/// A glazed entrance on a face at `t`: lit or dark doors, a canopy with a
/// light under it after dark, a doormat of paving.
fn campus_door(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, canopy: bool) {
    let f = &c.frame;
    let lit = coin(rng, 0.5 + 0.5 * c.lit_chance());
    face.glaze(g, c, t, 1.35, 0.075, w, 2.7, if lit { Pane::Lit(mul(ATRIUM, 0.55)) } else { Pane::Dark });
    g.lod(Lod::Mid, |g| face.window_frame(g, c, t, 1.35, w, 2.7, 0.22, mul(SPANDRELS[0], 0.9)));
    let reach = (c.reach_out(face) - 0.3).min(2.2);
    if canopy && reach > 0.8 {
        face.bx(g, f, Panel, t, 3.1, reach * 0.5, Vec3::new(w + 1.2, 0.22, reach), mul(SPANDRELS[1], 1.0 - 0.2 * c.wear));
        let p = face.at(t, reach * 0.5);
        night_box(g, c, p.x, 2.96, p.y, Vec3::new((w * 0.6).max(0.6), 0.04, (reach * 0.4).max(0.3)), LAMP);
    }
}

/// A small glass kiosk with a screen, for a lot too small for anything
/// else.
fn kiosk(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let a = R::of(&c.poly);
    let Some(r) = fit(c, R::new(a.c().x - 1.6, a.c().x + 1.6, a.v0 + 0.6, a.v0 + 3.4), 1.2) else {
        let p = a.c();
        c.frame.bx(g, Glass, p.x, 1.2, p.y, Vec3::new(1.0, 2.4, 1.0), tint(c, rng));
        return;
    };
    block(g, c, Glass, &r, 0.0, 2.8, tint(c, rng));
    block(g, c, Concrete, &r.inset(-0.15), 2.8, 3.1, mul(SPANDRELS[1], 0.9));
    let fr = r.front();
    campus_door(g, c, rng, &fr, 0.0, (fr.len * 0.5).min(1.4), true);
}

// ── homes ─────────────────────────────────────────────────────────────────────

/// Student flats: a slab of punched windows each with a bright coloured
/// panel beside it (colours by the column, a few out of step), deck-access
/// galleries along the back with doors and lamps, an exposed steel stair at
/// one end, a lit entrance, bins, a full bike rack and scooters out front;
/// plant and solar on the roof.
pub fn student_flats(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(2.8, 3.1);
    let area = R::of(&c.poly);
    // Deck-access galleries want a metre and three quarters behind the slab.
    let depth = (area.d() - 1.8).min(12.5);
    let v0 = area.v0 + (area.d() - 1.8 - depth).clamp(0.0, 3.0);
    let Some(b) = fit(c, R::new(area.u0 + 0.3, area.u1 - 0.3, v0, v0 + depth), 6.0) else { return kiosk(g, c, rng) };
    let floors = (c.floors as i32).clamp(2, 10);
    let h = floors as f32 * fh;
    let wall = mix(concrete(c, rng), [0.82, 0.80, 0.76], 0.4);
    block(g, c, Concrete, &b, 0.0, h, wall);
    // The galleries take the back's outer metre and a half when it is deep enough.
    let gallery = b.d() > 8.0 && c.inside_local(Vec2::new(b.u0, b.v1 + 1.7)) && c.inside_local(Vec2::new(b.u1, b.v1 + 1.7));
    let front = b.front();
    let cols = ((front.len - 1.0) / 2.7).floor() as i32;
    let palette: Vec<[f32; 3]> = (0..3).map(|_| bright(c, rng)).collect();
    if cols >= 1 {
        let step = (front.len - 1.0) / cols as f32;
        for k in 0..floors {
            let y = k as f32 * fh + fh * 0.55;
            for i in 0..cols {
                let t = -(front.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
                if k == 0 && i == cols / 2 {
                    campus_door(g, c, rng, &front, t, (step * 0.7).min(2.4), true);
                    continue;
                }
                let ww = (step * 0.45).min(1.3);
                let pane = if coin(rng, c.lit_chance()) { Pane::Lit(mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.9))) } else { Pane::Dark };
                front.glaze(g, c, t - step * 0.14, y, PANE, ww, fh * 0.55, pane);
                // The panel beside it: the column's colour, now and then another.
                let col = if coin(rng, 0.15) { bright(c, rng) } else { palette[(i as usize + k as usize / 3) % palette.len()] };
                front.decal(g, f, Panel, t + step * 0.27, y - 0.05, 0.04, (step * 0.32).min(0.9), fh * 0.82, col);
                g.lod(Lod::Mid, |g| front.window_frame(g, c, t - step * 0.14, y, ww, fh * 0.55, 0.14, mul(SPANDRELS[0], 1.2)));
            }
        }
    }
    for face in [b.left(), b.right()] {
        pane_row(g, c, rng, &face, 0.0, floors, fh, 3.2, 0.9, fh * 0.45, false);
        grime(g, c, rng, &face, 0.0, h, 0.4 + 1.6 * c.wear);
    }
    grime(g, c, rng, &front, 0.0, h, 0.3 + 1.2 * c.wear);
    // Deck-access galleries off the back: a slab and a rail each floor,
    // doors along the wall, a lamp by every other one.
    let back = b.back();
    if gallery {
        let slab = mul(wall, 0.85);
        let doors = ((back.len - 1.0) / 3.6).floor().max(1.0) as i32;
        let dstep = (back.len - 1.0) / doors as f32;
        for k in 1..floors {
            let y = k as f32 * fh;
            back.bx(g, f, Concrete, 0.0, y - 0.1, 0.75, Vec3::new(back.len - 0.1, 0.2, 1.5), slab);
            g.lod(Lod::Mid, |g| back.bx(g, f, Matte, 0.0, y + 0.55, 1.45, Vec3::new(back.len - 0.1, 1.1, 0.05), mul(pal.steel, 0.6)));
            for i in 0..doors {
                let t = -(back.len - 1.0) * 0.5 + (i as f32 + 0.5) * dstep;
                back.decal(g, f, Matte, t, y + 1.05, 0.04, 0.95, 2.1, mul(palette[i as usize % palette.len()], 0.8));
                if i % 2 == 0 {
                    let p = back.at(t + 0.9, 0.12);
                    night_box(g, c, p.x, y + 2.3, p.y, Vec3::new(0.25, 0.12, 0.12), LAMP);
                }
            }
        }
        for i in 0..doors {
            let t = -(back.len - 1.0) * 0.5 + (i as f32 + 0.5) * dstep;
            back.decal(g, f, Matte, t, 1.05, 0.04, 0.95, 2.1, mul(palette[i as usize % palette.len()], 0.8));
        }
    } else {
        pane_row(g, c, rng, &back, 0.0, floors, fh, 2.8, 0.9, fh * 0.45, false);
    }
    // An exposed stair at one end where the lot leaves room.
    let left = c.seed % 2 == 0;
    let su = if left { b.u0 - 1.9 } else { b.u1 + 1.9 };
    let sr = R::new(su - 1.5, su + 1.5, b.v1 - 4.4, b.v1 - 0.2);
    if poly_inside(c, &offset_poly(&sr.poly(), -0.3)) && floors >= 3 {
        steel_core(g, c, rng, &sr, 0.0, h - fh + 1.1, fh);
    }
    // The roof: a parapet, plant, a solar array.
    parapet(g, c, &b.poly(), h, 0.9, 0.2, Concrete, mul(wall, 0.9));
    let roof = b.inset(0.6);
    if roof.w() > 6.0 && roof.d() > 4.0 {
        let half = R::new(roof.u0, roof.c().x - 0.5, roof.v0, roof.v1);
        roof_plant(g, c, rng, &half, h);
        let sp = Vec2::new((roof.c().x + roof.u1) * 0.5, roof.c().y);
        solar_array(g, f, sp, h, (roof.w() * 0.4).min(10.0), (roof.d() - 2.0).clamp(1.0, 6.0), 0.0, pal.glass_dark);
    }
    // Out front: bikes, scooters, bins.
    let yard_v = (area.v0 + b.v0) * 0.5;
    if b.v0 - area.v0 > 2.6 {
        bike_rack(g, c, rng, Vec2::new(b.u0 + 1.2, yard_v), Vec2::X, ((b.w() * 0.3) / 0.75).floor().clamp(2.0, 10.0) as i32);
        let n = rng.random_range(2..5);
        scooters(g, c, rng, Vec2::new(b.u1 - 3.0, yard_v), n);
    } else {
        scooters(g, c, rng, Vec2::new(b.c().x + 3.0, b.v1 + 2.0), 2);
    }
    if b.v1 + 2.2 < area.v1 {
        for i in 0..3 {
            let p = Vec2::new(b.u0 + 1.0 + i as f32 * 1.0, b.v1 + 1.4 + if gallery { 1.6 } else { 0.0 });
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.6 {
                g.lod(Lod::Mid, |g| f.bx(g, Paint, p.x, 0.6, p.y, Vec3::new(0.8, 1.2, 0.8), mul(HIRE[(i + c.seed as usize) % HIRE.len()], 0.35)));
            }
        }
    }
}

/// A co-living tower: a podium of a café and coworking floor behind glass
/// with a screen over the door; a slim tower of tinted curtain wall over
/// it, its shared lounges every few floors hung out as glass boxes lit all
/// night; an exposed braced stair core up one side; plant and rails on
/// the roof.
pub fn coliving_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let fh = c.fh().clamp(3.0, 3.3);
    let area = R::of(&c.poly);
    let Some(pod) = fit(c, R::new(area.u0 + 0.4, area.u1 - 0.4, area.v0 + 1.0, area.v0 + 1.0 + area.d().min(18.0)), 8.0) else { return student_flats(g, c, rng, e) };
    let glass = tint(c, rng);
    let span = spandrel(c, rng);
    let floors = (c.floors as i32).clamp(4, 30);
    let pod_floors = if floors > 8 { 2 } else { 1 };
    let ph = curtain_block(g, c, rng, &pod, 0.0, pod_floors, fh + 0.6, mul(glass, 1.15), mul(span, 1.1));
    let pfront = pod.front();
    campus_door(g, c, rng, &pfront, -pfront.len * 0.25, 2.6, true);
    if pfront.len > 10.0 {
        led_screen(g, c, rng, &pfront, pfront.len * 0.2, ph - 1.3, (pfront.len * 0.3).min(7.0), 1.4);
    }
    // The tower, set back from the podium's edges.
    let tw = (pod.w() * 0.62).clamp(7.0, 20.0);
    let td = (pod.d() * 0.7).clamp(7.0, 15.0);
    let left = c.seed % 2 == 0;
    let tu = if left { pod.u0 + 1.0 + tw * 0.5 } else { pod.u1 - 1.0 - tw * 0.5 };
    let tr = R::new(tu - tw * 0.5, tu + tw * 0.5, pod.v0 + 1.2, pod.v0 + 1.2 + td);
    let tr = R::new(tr.u0.max(pod.u0 + 0.6), tr.u1.min(pod.u1 - 0.6), tr.v0, tr.v1.min(pod.v1 - 0.6));
    let rest = floors - pod_floors;
    let top = curtain_block(g, c, rng, &tr, ph, rest, fh, glass, span);
    // Lounges: glass boxes off the front every three or four floors.
    let tfront = tr.front();
    let every = rng.random_range(3..5);
    let lw = (tfront.len * 0.45).clamp(3.0, 8.0);
    let lt = if left { tfront.len * 0.5 - lw * 0.5 - 0.3 } else { -tfront.len * 0.5 + lw * 0.5 + 0.3 };
    let reach = (tr.v0 - pod.v0 - 0.2).clamp(0.0, 2.2);
    let mut k = 2;
    while k < rest - 1 && reach > 0.6 {
        let y = ph + k as f32 * fh;
        atrium(g, c, rng, &tfront, lt, lw, reach, y + 0.05, y + fh - 0.1, glass, fh, LOUNGE);
        k += every;
    }
    // The stair core up the side away from the lounges.
    let cu = if left { tr.u1 + 1.6 } else { tr.u0 - 1.6 };
    let cr = R::new(cu - 1.3, cu + 1.3, tr.v0 + 1.0, (tr.v0 + 5.0).min(tr.v1 - 0.4));
    if cr.u0 > pod.u0 + 0.2 && cr.u1 < pod.u1 - 0.2 && cr.d() > 2.0 {
        steel_core(g, c, rng, &cr, ph, top - 0.4 - ph, fh);
    }
    roof_plant(g, c, rng, &tr.inset(0.2), top);
    // The podium roof: a terrace with rails and planters.
    let terrace = pod.inset(0.4);
    rails(g, c, &terrace, ph, mul(c.palette.steel, 0.7));
    if area.v0 + 0.8 < pod.v0 {
        let n = rng.random_range(2..5);
        scooters(g, c, rng, Vec2::new(pod.c().x + pfront.len * 0.15, (area.v0 + pod.v0) * 0.5), n);
    }
    let side = if left { pod.right() } else { pod.left() };
    cable_tray(g, c, &side, -side.len * 0.45, side.len * 0.3, ph - 0.6);
}

// ── the campus: labs, sheds, offices, the quad ───────────────────────────────

/// A glass lab: two to five floors of tinted curtain wall inside an
/// exposed steel frame (columns off the faces, X braces on the ends),
/// louvred vent bays, fume stacks and ducts over the roof, a gas bottle
/// cage and bikes by the door.
pub fn glass_lab(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let f = &c.frame;
    let fh = c.fh().clamp(3.6, 4.2);
    let area = R::of(&c.poly);
    let Some(b) = fit(c, R::new(area.u0 + 1.2, area.u1 - 1.2, area.v0 + 2.5, area.v0 + 2.5 + (area.d() - 3.5).min(22.0)), 6.0) else { return kiosk(g, c, rng) };
    let floors = (c.floors as i32).clamp(1, 5);
    let glass = tint(c, rng);
    let span = spandrel(c, rng);
    let top = curtain_block(g, c, rng, &b, 0.0, floors, fh, glass, span);
    let st = steel(c, rng);
    // The frame: columns a metre off every face, a beam round each floor.
    let frame = b.inset(-0.9);
    let mut cols: Vec<Vec2> = Vec::new();
    for face in frame.faces() {
        let n = ((face.len / 4.5).round() as i32).max(1);
        for i in 0..n {
            cols.push(face.at(-face.len * 0.5 + i as f32 * face.len / n as f32, 0.0));
        }
    }
    let inside_all = cols.iter().all(|p| c.inside_local(*p) && distance_to_boundary(*p, &c.lot_poly) > 0.2);
    if inside_all {
        for p in &cols {
            f.bx(g, Matte, p.x, top * 0.5, p.y, Vec3::new(0.32, top, 0.32), st);
        }
        g.lod(Lod::Mid, |g| {
            for k in 1..=floors {
                let y = k as f32 * fh;
                for face in frame.faces() {
                    face.bx(g, f, Matte, 0.0, y - 0.15, 0.0, Vec3::new(face.len + 0.3, 0.26, 0.24), st);
                }
            }
            // X braces on the two ends.
            for face in [frame.left(), frame.right()] {
                for k in 0..floors {
                    let (y0, y1) = (k as f32 * fh + 0.3, (k + 1) as f32 * fh - 0.3);
                    let (a, b2) = (face.at(-face.len * 0.5 + 0.2, 0.0), face.at(face.len * 0.5 - 0.2, 0.0));
                    g.part(Matte).beam(f.at2(a, y0), f.at2(b2, y1), 0.14, rgba(st));
                    g.part(Matte).beam(f.at2(b2, y0), f.at2(a, y1), 0.14, rgba(st));
                }
            }
        });
        // Rust at the feet of the columns.
        if c.wear > 0.3 {
            g.lod(Lod::Near, |g| {
                for p in &cols {
                    f.bx(g, Rust, p.x, 0.35, p.y, Vec3::new(0.36, 0.7, 0.36), mix(st, c.palette.rust, 0.7));
                }
            });
        }
    }
    // Louvred vent bays low on the sides.
    for face in [b.left(), b.right()] {
        if face.len < 5.0 {
            continue;
        }
        let lw = (face.len * 0.4).min(5.0);
        face.decal(g, f, Matte, 0.0, fh * 0.55, 0.075, lw, fh * 0.6, DARK);
        g.lod(Lod::Mid, |g| {
            let n = ((fh * 0.6) / 0.22).floor() as i32;
            for i in 0..n {
                face.bx(g, f, Metal, 0.0, fh * 0.25 + 0.11 + i as f32 * 0.22, 0.14, Vec3::new(lw, 0.05, 0.14), mul(span, 1.2));
            }
        });
    }
    let front = b.front();
    campus_door(g, c, rng, &front, -front.len * 0.2, 2.4, true);
    // The roof: fume stacks in a row, a duct, a chiller.
    let roof = b.inset(1.2);
    let stacks = ((roof.w() / 3.0).floor() as i32).clamp(1, 6);
    let sv = roof.v1 - 0.8;
    for i in 0..stacks {
        let u = roof.u0 + 0.6 + i as f32 * 3.0;
        let w = f.to_world(u, sv);
        let hh = rng.random_range(3.0..5.5);
        g.part(Metal).cylinder_capped(w, 0.3, 0.26, top, top + hh, 8, false, false, rgba(mul(c.palette.steel, 0.9)));
        g.lod(Lod::Mid, |g| g.part(Matte).cylinder(w, 0.42, 0.42, top + hh, top + hh + 0.25, 8, rgba(DARK)));
    }
    if roof.d() > 4.5 && roof.w() > 4.0 {
        chiller(g, c, Vec2::new(roof.c().x, roof.v0 + 1.4), top, (roof.w() * 0.5).min(5.0), 1.8, 0.0, mul(c.palette.steel, 0.85));
        g.lod(Lod::Mid, |g| pipe_run(g, f, &[Vec3::new(roof.u0 + 0.6, top + 1.2, sv - 0.6), Vec3::new(roof.u1 - 0.6, top + 1.2, sv - 0.6)], 0.3, mul(c.palette.steel, 0.95)));
    }
    rails(g, c, &b.inset(0.2), top, mul(c.palette.steel, 0.7));
    // A gas bottle cage and bikes by the door.
    let cage = Vec2::new(b.u1 - 1.2, area.v0 + 1.3);
    if c.inside_local(cage) && distance_to_boundary(cage, &c.lot_poly) > 0.9 && b.v0 - area.v0 > 2.0 {
        g.lod(Lod::Mid, |g| {
            f.bx(g, Matte, cage.x, 0.9, cage.y, Vec3::new(1.6, 1.8, 0.9), mul(c.palette.steel, 0.5));
        });
        g.lod(Lod::Near, |g| {
            for i in 0..4 {
                let w = f.to_world(cage.x - 0.6 + i as f32 * 0.4, cage.y);
                g.part(Paint).cylinder_capped(w, 0.13, 0.13, 0.0, 1.5, 6, false, true, rgba([[0.2, 0.4, 0.2], [0.8, 0.8, 0.8], [0.15, 0.2, 0.5], [0.7, 0.2, 0.1]][i]));
            }
        });
    }
    let n = rng.random_range(3..8);
    bike_rack(g, c, rng, Vec2::new(b.u0 + 0.8, area.v0 + 1.3), Vec2::X, n);
    let side = b.right();
    cable_tray(g, c, &side, -side.len * 0.4, side.len * 0.4, fh - 0.4);
}

/// One prefab shed: a corrugated box on blocks, a glazed garage front lit
/// inside or shuttered, a hand-made sign, a vent and a cable over the roof.
#[allow(clippy::too_many_arguments)]
fn shed(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y0: f32, h: f32, col: [f32; 3]) {
    let f = &c.frame;
    block(g, c, Metal, r, y0, y0 + h, col);
    block(g, c, Metal, &r.inset(-0.08), y0 + h, y0 + h + 0.12, mul(col, 0.7));
    let front = r.front();
    let gw = (front.len - 1.2).clamp(1.5, 4.0);
    if coin(rng, 0.75 - 0.3 * c.wear) {
        front.glaze(g, c, 0.0, y0 + h * 0.45, PANE, gw, h * 0.7, Pane::Lit(mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.85))));
        g.lod(Lod::Mid, |g| {
            front.window_frame(g, c, 0.0, y0 + h * 0.45, gw, h * 0.7, 0.16, mul(col, 0.5));
            for k in 1..4 {
                front.decal(g, f, Matte, 0.0, y0 + h * 0.1 + k as f32 * h * 0.7 / 4.0, PANE + 0.008, gw, 0.05, mul(col, 0.5));
            }
        });
    } else {
        front.decal(g, f, Metal, 0.0, y0 + h * 0.45, 0.05, gw, h * 0.7, mul(pick(rng, &c.palette.metal_paint), 0.7));
    }
    if coin(rng, 0.6) {
        sign_board(g, c, rng, &front, 0.0, y0 + h - 0.25, (gw * 0.8).min(3.0), 0.3);
    }
    grime(g, c, rng, &front, y0, y0 + h, 0.5 + 2.0 * c.wear);
    g.lod(Lod::Mid, |g| {
        let p = r.c();
        f.bx(g, Metal, p.x + r.w() * 0.25, y0 + h + 0.45, p.y, Vec3::new(0.5, 0.7, 0.5), mul(c.palette.steel, 0.8));
    });
}

/// Start-up sheds: a yard of cheap prefab sheds and boxes, a few stacked
/// with a steel stair up, garage fronts lit late, signs by hand; a shared
/// deck with tables under a string of lights, bikes and scooters, a skip.
pub fn startup_sheds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 5.0 || area.d() < 5.0 {
        return kiosk(g, c, rng);
    }
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.3));
    let n = ((area.w() / 6.8).floor() as i32).clamp(1, 5);
    let slot = area.w() / n as f32;
    let sw = (slot - 1.2).clamp(3.0, 6.0);
    let sd = (area.d() * 0.45).clamp(3.5, 9.0);
    let v0 = area.v0 + (area.d() * 0.3).clamp(1.5, 8.0);
    let stack = c.floors >= 2;
    let mut placed = 0;
    let mut last = f32::MIN;
    for i in 0..n {
        let cu = area.u0 + (i as f32 + 0.5) * slot;
        let Some(r) = fit(c, R::new(cu - sw * 0.5, cu + sw * 0.5, v0, v0 + sd), 2.8) else { continue };
        if r.u0 < last + 0.6 {
            continue;
        }
        last = r.u1;
        let col = mix(pick(rng, &pal.metal_paint), [0.62, 0.62, 0.60], 0.3 + 0.3 * c.wear);
        let h = 2.9;
        shed(g, c, rng, &r, 0.0, h, col);
        placed += 1;
        if stack && i % 2 == 0 && r.w() > 3.5 {
            let up = R::new(r.u0 + 0.3, r.u1 - 0.3, r.v0 + 0.5, r.v1 - 0.2);
            let top_col = mix(pick(rng, &pal.metal_paint), [0.7, 0.7, 0.68], 0.4);
            shed(g, c, rng, &up, h + 0.12, 2.7, top_col);
            // A steel stair up its side.
            let su = if i == 0 { up.u0 - 0.6 } else { up.u1 + 0.6 };
            let foot = Vec2::new(su, up.v1 - 0.5);
            if c.inside_local(foot) && distance_to_boundary(foot, &c.lot_poly) > 0.6 && c.inside_local(Vec2::new(su, up.v0)) {
                let st = steel(c, rng);
                g.lod(Lod::Mid, |g| {
                    g.part(Metal).slab(f.at(su, 0.0, up.v1 - 0.4), f.at(su, h + 0.1, up.v0 + 0.6), 0.9, 0.14, rgba(st));
                    f.bx(g, Matte, su, (h + 1.1) * 0.5, up.v0 + 0.6, Vec3::new(0.08, h + 1.1, 0.08), st);
                });
            }
        }
    }
    if placed == 0 {
        return kiosk(g, c, rng);
    }
    // The shared deck in front: tables, a string of lights on two poles.
    let deck_v = (area.v0 + v0) * 0.5;
    if v0 - area.v0 > 3.0 {
        let dk = R::new(area.c().x - (area.w() * 0.3).min(6.0), area.c().x + (area.w() * 0.3).min(6.0), deck_v - 1.0, deck_v + 1.0);
        if let Some(dk) = fit(c, dk, 1.5) {
            block(g, c, Wood, &dk, 0.0, 0.18, mul(pal.wood, 0.8 - 0.3 * c.wear));
            for i in 0..((dk.w() / 3.0).floor() as i32).clamp(0, 4) {
                let p = Vec2::new(dk.u0 + 1.5 + i as f32 * 3.0, dk.c().y);
                g.lod(Lod::Mid, |g| table(g, f, p, 0.18, mul(pal.wood, 0.6), None));
            }
            let (a, b) = (Vec2::new(dk.u0 + 0.2, dk.v1 - 0.2), Vec2::new(dk.u1 - 0.2, dk.v1 - 0.2));
            for p in [a, b] {
                f.bx(g, Matte, p.x, 1.5, p.y, Vec3::new(0.1, 3.0, 0.1), CHARCOAL);
            }
            g.lod(Lod::Mid, |g| g.part(Matte).wire(f.at2(a, 2.9), f.at2(b, 2.9), 0.4, 0.025, rgba(DARK)));
            let k = ((dk.w() / 1.2).floor() as i32).clamp(2, 12);
            for i in 1..k {
                let t = i as f32 / k as f32;
                let p = a.lerp(b, t);
                night_box(g, c, p.x, 2.9 - 0.4 * 4.0 * t * (1.0 - t) - 0.08, p.y, Vec3::splat(0.13), [2.2, 1.6, 0.8]);
            }
        }
        let n = rng.random_range(2..6);
        scooters(g, c, rng, Vec2::new(area.u1 - 2.4, deck_v), n);
        let n = rng.random_range(2..5);
        bike_rack(g, c, rng, Vec2::new(area.u0 + 1.0, deck_v - 0.6), Vec2::X, n);
    }
    // A skip of rubbish behind.
    let sk = Vec2::new(area.u0 + 2.2, area.v1 - 1.6);
    if c.inside_local(sk) && distance_to_boundary(sk, &c.lot_poly) > 1.4 && sk.y > v0 + sd + 1.2 {
        f.bx(g, Metal, sk.x, 0.6, sk.y, Vec3::new(2.8, 1.2, 1.5), mul([0.70, 0.46, 0.10], 1.0 - 0.4 * c.wear));
        g.lod(Lod::Near, |g| scrap_pile(g, c, rng, sk, 0.9, 1.4));
    }
    lamp(g, c, Vec2::new(area.u1 - 1.0, area.v0 + 1.2), 4.0);
}

/// An atrium office: a slab of tinted curtain wall on a recessed ground
/// floor, a full-height glass atrium on the front lit all night with a
/// screen hung in it, a brand-blind entrance canopy, plant on the roof, a
/// cable tray down the side, bikes and barriers out front.
pub fn atrium_office(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let fh = c.fh().clamp(3.4, 3.8);
    let area = R::of(&c.poly);
    let Some(b) = fit(c, R::new(area.u0 + 0.8, area.u1 - 0.8, area.v0 + 3.0, area.v0 + 3.0 + (area.d() - 4.0).min(24.0)), 8.0) else { return glass_lab(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(3, 20);
    let glass = tint(c, rng);
    let span = spandrel(c, rng);
    // A recessed ground floor under the slab, then the slab.
    let ground_r = b.inset(0.9);
    block(g, c, Glass, &ground_r, 0.0, fh + 0.6, mul(glass, 1.25));
    for face in ground_r.faces() {
        if face.len > 2.0 {
            let col = mul(pick(rng, &c.style.window_colors), 0.6);
            face.glaze(g, c, 0.0, (fh + 0.6) * 0.5, PANE, face.len - 0.6, fh - 0.4, if coin(rng, c.lit_chance()) { Pane::Lit(col) } else { Pane::Dark });
        }
    }
    let top = curtain_block(g, c, rng, &b, fh + 0.6, floors - 1, fh, glass, span);
    // The slab's soffit columns.
    for (u, v) in [(b.u0 + 0.4, b.v0 + 0.4), (b.u1 - 0.4, b.v0 + 0.4), (b.u0 + 0.4, b.v1 - 0.4), (b.u1 - 0.4, b.v1 - 0.4)] {
        f.bx(g, Concrete, u, (fh + 0.6) * 0.5, v, Vec3::new(0.5, fh + 0.6, 0.5), mul(span, 1.1));
    }
    // The atrium on the front, the full height or near it.
    let front = b.front();
    let aw = (front.len * 0.38).clamp(4.0, 14.0);
    let at = if c.seed % 3 == 0 { 0.0 } else if c.seed % 3 == 1 { -front.len * 0.5 + aw * 0.5 + 1.0 } else { front.len * 0.5 - aw * 0.5 - 1.0 };
    let reach = (b.v0 - area.v0 - 0.8).clamp(0.0, 2.4);
    let ay1 = if floors > 6 { top - fh * rng.random_range(1..3) as f32 } else { top - 0.4 };
    atrium(g, c, rng, &front, at, aw, reach, 0.0, ay1, glass, fh, ATRIUM);
    // The entrance beside it.
    let dt = if at <= 0.0 { at + aw * 0.5 + 2.4 } else { at - aw * 0.5 - 2.4 };
    let gfront = ground_r.front();
    if dt.abs() < gfront.len * 0.5 - 1.2 {
        campus_door(g, c, rng, &gfront, dt, 2.4, true);
    }
    roof_plant(g, c, rng, &b.inset(0.2), top);
    let side = if at > 0.0 { b.left() } else { b.right() };
    cable_tray(g, c, &side, -side.len * 0.4, side.len * 0.35, fh + 0.2);
    if b.v0 - area.v0 > 2.6 {
        let n = rng.random_range(4..10);
        bike_rack(g, c, rng, Vec2::new(area.u0 + 1.5, area.v0 + 1.4), Vec2::X, n);
        let n = rng.random_range(2..5);
        scooters(g, c, rng, Vec2::new(area.u1 - 3.0, area.v0 + 1.5), n);
        let fr = R::new(area.u0, area.u1, area.v0, b.v0).front();
        barrier(g, c, &fr, fr.len * 0.15, 0.9, 1.8);
    }
    lamp(g, c, Vec2::new(area.u0 + 0.9, area.v0 + 2.6), 4.5);
    lamp(g, c, Vec2::new(area.u1 - 0.9, area.v0 + 2.6), 4.5);
}

/// A shared quad: low glass and concrete wings round a lawn (on three
/// sides, or two on a shallow lot), trees and benches, paths across, a
/// big screen on a steel frame at the far end, a café pavilion lit late,
/// lamps along the paths, bikes and scooters everywhere.
pub fn campus_quad(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(3.3, 3.7);
    let area = R::of(&c.poly);
    if area.w() < 18.0 || area.d() < 16.0 {
        return glass_lab(g, c, rng, e);
    }
    let floors = (c.floors as i32).clamp(1, 4);
    let glass = tint(c, rng);
    let span = spandrel(c, rng);
    let ww = (area.w() * 0.2).clamp(6.0, 11.0);
    let (u0, u1, v0, v1) = (area.u0 + 0.6, area.u1 - 0.6, area.v0 + 1.0, area.v1 - 0.6);
    let wings = [R::new(u0, u0 + ww, v0, v1), R::new(u1 - ww, u1, v0, v1), R::new(u0 + ww + 0.3, u1 - ww - 0.3, v1 - ww.min((v1 - v0) * 0.35), v1)];
    let mut tops = Vec::new();
    for (i, w) in wings.iter().enumerate() {
        if i == 2 && w.w() < 6.0 {
            continue;
        }
        let Some(w) = fit(c, *w, 5.0) else { continue };
        let fl = if i == 2 { floors } else { (floors - (i as i32 % 2)).max(1) };
        let top = curtain_block(g, c, rng, &w, 0.0, fl, fh, if i == 1 { mul(glass, 1.15) } else { glass }, span);
        roof_plant(g, c, rng, &w.inset(0.3), top);
        tops.push((w, top));
    }
    // The lawn, paths and trees.
    let lawn = fit(c, R::new(u0 + ww + 0.3, u1 - ww - 0.3, v0, v1 - ww.min((v1 - v0) * 0.35) - 0.3), 4.0);
    if let Some(lawn) = lawn {
        ground(g, f, Foliage, &lawn.poly(), 0.03, mul(pal.foliage, 0.8 - 0.25 * c.wear));
        ground(g, f, Paving, &R::new(lawn.c().x - 1.2, lawn.c().x + 1.2, lawn.v0, lawn.v1).poly(), 0.05, mul(pal.paving, 1.05));
        ground(g, f, Paving, &R::new(lawn.u0, lawn.u1, lawn.c().y - 1.0, lawn.c().y + 1.0).poly(), 0.06, mul(pal.paving, 0.95));
        for (i, p) in [(lawn.u0 + 2.2, lawn.v0 + 2.5), (lawn.u1 - 2.2, lawn.v0 + 2.5), (lawn.u0 + 2.2, lawn.v1 - 2.5), (lawn.u1 - 2.2, lawn.v1 - 2.5)].iter().enumerate() {
            let p = Vec2::new(p.0, p.1);
            if lawn.w() > 7.0 && lawn.d() > 7.0 && c.inside_local(p) {
                tree(g, f, p, rng.random_range(4.5..7.0), 1.8, pal.bark, mul(pal.foliage, 0.95 - 0.2 * c.wear));
                if i % 2 == 0 {
                    g.lod(Lod::Mid, |g| bench_world(g, f.to_world(p.x + 1.6, p.y), f.yaw(), mul(pal.wood, 0.7), CHARCOAL));
                }
            }
        }
        // The screen on its frame at the far end of the lawn, facing the street.
        let sv = lawn.v1 - 1.0;
        if lawn.w() > 8.0 {
            let sw = (lawn.w() * 0.45).min(9.0);
            let st = steel(c, rng);
            for s in [-1.0f32, 1.0] {
                f.bx(g, Matte, lawn.c().x + s * sw * 0.5, 2.6, sv, Vec3::new(0.3, 5.2, 0.3), st);
            }
            let face = Face::new(Vec2::new(lawn.c().x - sw * 0.5, sv - 0.2), Vec2::new(lawn.c().x + sw * 0.5, sv - 0.2), Vec2::new(0.0, -1.0));
            led_screen(g, c, rng, &face, 0.0, 3.6, sw - 0.6, (sw * 0.4).min(3.0));
        }
        // The café pavilion on the lawn's street corner.
        let cp = R::new(lawn.u0 + 0.6, lawn.u0 + 0.6 + (lawn.w() * 0.3).clamp(3.0, 6.0), lawn.v0 + 0.6, lawn.v0 + 0.6 + 3.6);
        if lawn.w() > 10.0 && lawn.d() > 10.0 {
            block(g, c, Glass, &cp, 0.0, 3.0, mul(glass, 1.3));
            let from = g.part(Glow).positions.len();
            block(g, c, Glow, &cp.inset(-0.03), 0.2, 2.75, mul(LOUNGE, 0.75 - 0.25 * c.wear));
            mark_glow(g, from, NIGHT_MARK);
            block(g, c, Concrete, &cp.inset(-0.5), 3.0, 3.25, mul(span, 1.1));
            for i in 0..2 {
                let p = Vec2::new(cp.u1 + 1.4 + i as f32 * 2.2, cp.c().y);
                if p.x < lawn.c().x - 1.6 {
                    g.lod(Lod::Mid, |g| table(g, f, p, 0.03, mul(pal.wood, 0.7), Some(BRIGHTS[(i + c.seed as usize) % BRIGHTS.len()])));
                }
            }
        }
        lamp(g, c, Vec2::new(lawn.c().x + 1.8, lawn.c().y + 1.8), 4.2);
        lamp(g, c, Vec2::new(lawn.c().x - 1.8, lawn.c().y - 1.8), 4.2);
        // Bikes along the lawn's street edge, short of the wing beyond it.
        let room = ((lawn.u1 - 0.6 - (lawn.c().x + 1.8)) / 0.75).floor() as i32;
        let n = rng.random_range(3..7).min(room);
        bike_rack(g, c, rng, Vec2::new(lawn.c().x + 1.8, lawn.v0 + 0.8), Vec2::X, n);
        let n = rng.random_range(3..6);
        scooters(g, c, rng, Vec2::new(lawn.c().x, lawn.c().y + 1.2), n);
    }
    for (w, _) in tops.iter().take(2) {
        let fr = w.front();
        campus_door(g, c, rng, &fr, 0.0, 2.2, true);
    }
}

// ── the campus's small industry ───────────────────────────────────────────────

/// Corners cut off a rectangle by `k`: the chamfered concrete mass.
fn chamfered(r: &R, k: f32) -> Vec<Vec2> {
    let k = k.min(r.w() * 0.3).min(r.d() * 0.3);
    vec![
        Vec2::new(r.u0 + k, r.v0),
        Vec2::new(r.u1 - k, r.v0),
        Vec2::new(r.u1, r.v0 + k),
        Vec2::new(r.u1, r.v1 - k),
        Vec2::new(r.u1 - k, r.v1),
        Vec2::new(r.u0 + k, r.v1),
        Vec2::new(r.u0, r.v1 - k),
        Vec2::new(r.u0, r.v0 + k),
    ]
}

/// A research lab: a chamfered concrete mass with panel seams and ribbon
/// windows, tall louvred vent bays, fume stacks and plant on the roof, a
/// glass entrance box, a gas bottle cage, a barrier at the gate and a
/// cable tray along the side.
pub fn research_lab(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let fh = c.fh().clamp(3.8, 4.4);
    let area = R::of(&c.poly);
    let Some(b) = fit(c, R::new(area.u0 + 1.5, area.u1 - 1.5, area.v0 + 3.0, area.v0 + 3.0 + (area.d() - 4.5).min(26.0)), 7.0) else { return glass_lab(g, c, rng, e) };
    let floors = (c.floors as i32).clamp(2, 6);
    let h = floors as f32 * fh;
    let conc = mix(concrete(c, rng), [0.62, 0.62, 0.60], 0.3);
    let poly = chamfered(&b, 2.4);
    extrude(g, f, Concrete, &poly, 0.0, h, conc);
    cornice(g, c, &poly, h - 0.3, 0.4, 0.15, Concrete, mul(conc, 0.85));
    let louvre = mul(SPANDRELS[0], 1.0 - 0.2 * c.wear);
    for face in c.faces(&poly) {
        if face.len < 3.0 {
            // A cut corner: a tall vent panel.
            g.lod(Lod::Mid, |g| {
                let n = ((h - 1.6) / 0.3).floor() as i32;
                for i in 0..n {
                    face.bx(g, f, Metal, 0.0, 1.0 + i as f32 * 0.3, 0.12, Vec3::new(face.len - 0.6, 0.06, 0.16), louvre);
                }
            });
            face.decal(g, f, Matte, 0.0, h * 0.5, 0.04, face.len - 0.5, h - 1.6, DARK);
            continue;
        }
        // Ribbon windows on every floor.
        for k in 0..floors {
            let y = k as f32 * fh + fh * 0.6;
            let w = face.len - 2.0;
            face.glaze(g, c, 0.0, y, 0.04, w, 0.9, Pane::Dark);
            if coin(rng, c.lit_chance() * 0.8) {
                let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.9));
                // Its own depth: its dark copy clear of a door's lit glass.
                let lw = w * rng.random_range(0.3..0.8);
                let lt = rng.random_range(-0.5..0.5) * (w - lw);
                face.glaze(g, c, lt, y, PANE + 0.02, lw, 0.7, Pane::Lit(col));
            }
        }
        // Panel seams (near) and grime.
        g.lod(Lod::Near, |g| {
            let mut y = 1.2;
            let mut i = 0;
            while y < h - 0.5 {
                if i % 2 == 0 || y % fh > 1.0 {
                    face.decal(g, f, Matte, 0.0, y, 0.012, face.len - 0.3, 0.03, mul(conc, 0.6));
                }
                y += 0.9;
                i += 1;
            }
        });
        grime(g, c, rng, &face, 0.0, h, 0.6 + 2.0 * c.wear);
    }
    // The glass entrance box on the front.
    let front = Face::new(Vec2::new(b.u0 + 2.4, b.v0), Vec2::new(b.u1 - 2.4, b.v0), Vec2::new(0.0, -1.0));
    let et = if c.seed % 2 == 0 { -front.len * 0.25 } else { front.len * 0.25 };
    // Out over the lot only, both its front corners (a slanted street cuts in).
    let mut reach = (b.v0 - area.v0 - 0.6).clamp(0.0, 2.0);
    while reach > 0.8 && !(c.inside_local(front.at(et - 2.3, reach + 0.3)) && c.inside_local(front.at(et + 2.3, reach + 0.3))) {
        reach -= 0.2;
    }
    if reach > 0.8 {
        let glass = tint(c, rng);
        front.bx(g, f, Glass, et, 2.0, reach * 0.5, Vec3::new(4.0, 4.0, reach), mul(glass, 1.3));
        front.bx(g, f, Concrete, et, 4.15, reach * 0.5, Vec3::new(4.4, 0.3, reach + 0.3), conc);
        let p = front.at(et, reach + 0.05);
        night_box(g, c, p.x, 3.7, p.y, Vec3::new(2.6, 0.12, 0.06), LAMP);
    }
    campus_door(g, c, rng, &front, et, 2.0, false);
    // Roof: stacks along the back, plant forward.
    let roof = b.inset(2.6);
    if roof.w() > 2.0 && roof.d() > 2.0 {
        let n = ((roof.w() / 3.5).floor() as i32).clamp(1, 6);
        for i in 0..n {
            let u = roof.u0 + (i as f32 + 0.5) * roof.w() / n as f32;
            let w = f.to_world(u, roof.v1 - 0.3);
            let hh = rng.random_range(4.0..7.0);
            g.part(Metal).cylinder_capped(w, 0.4, 0.34, h, h + hh, 8, false, false, rgba(mul(c.palette.steel, 0.95)));
            g.lod(Lod::Mid, |g| g.part(Matte).cylinder(w, 0.55, 0.55, h + hh, h + hh + 0.3, 8, rgba(DARK)));
        }
        if roof.d() > 4.0 {
            let pr = R::new(roof.u0, roof.u1, roof.v0, roof.v1 - 1.6);
            roof_plant(g, c, rng, &pr, h);
        }
    }
    // A gas bottle cage, the barrier, the tray, a lamp.
    let side = b.right();
    cable_tray(g, c, &side, -side.len * 0.35, side.len * 0.35, fh * 0.9);
    let fr = R::new(area.u0, area.u1, area.v0, b.v0).front();
    barrier(g, c, &fr, -fr.len * 0.3, 1.0, 2.0);
    lamp(g, c, Vec2::new(area.u1 - 1.0, area.v0 + 1.5), 5.0);
    let cage = Vec2::new(b.u1 + 0.9, b.v1 - 2.0);
    if c.inside_local(cage) && distance_to_boundary(cage, &c.lot_poly) > 0.7 {
        g.lod(Lod::Mid, |g| f.bx(g, Matte, cage.x, 0.9, cage.y, Vec3::new(0.9, 1.8, 2.0), mul(c.palette.steel, 0.5)));
    }
}

/// A server hall: a windowless panel shed with seam fins, a status strip
/// lit after dark, rows of chillers on the roof and dry coolers in the
/// yard, generator boxes and a fuel tank, cable trays, behind a security
/// fence with barbed wire, a gate and floodlights.
pub fn server_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 10.0 || area.d() < 10.0 {
        return kiosk(g, c, rng);
    }
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.4));
    let h = (c.floors as f32 * 4.5).clamp(8.0, 16.0);
    let Some(b) = fit(c, R::new(area.u0 + 3.0, area.u1 - 3.0, area.v0 + 4.0, area.v1 - 3.5), 6.0) else { return kiosk(g, c, rng) };
    let b = R::new(b.u0, b.u1 - (b.w() * 0.22).clamp(0.0, 8.0), b.v0, b.v1);
    let panel = mul(pick(rng, &pal.panel), 0.95 - 0.25 * c.wear);
    block(g, c, Panel, &b, 0.0, h, panel);
    cornice(g, c, &b.poly(), h - 0.5, 0.6, 0.25, Panel, mul(panel, 0.8));
    for face in b.faces() {
        // Seam fins (mid), the status strip after dark, a louvre band.
        let n = (face.len / 5.0).floor() as i32;
        g.lod(Lod::Mid, |g| {
            for k in 1..n {
                let t = -face.len * 0.5 + k as f32 * face.len / n as f32;
                face.bx(g, f, Matte, t, h * 0.5 + 0.4, 0.08, Vec3::new(0.22, h - 1.8, 0.16), mul(panel, 0.6));
            }
        });
        let from = g.part(Glow).positions.len();
        face.decal(g, f, Glow, 0.0, h - 1.1, 0.05, face.len * 0.94, 0.09, mul(STATUS, 0.7 - 0.3 * c.wear));
        mark_glow(g, from, NIGHT_MARK);
        face.decal(g, f, Matte, 0.0, 2.3, 0.045, face.len * 0.85, 1.1, mul(panel, 0.45));
        grime(g, c, rng, &face, 0.0, h, 0.4 + 1.8 * c.wear);
    }
    let front = b.front();
    // The loading door and the service door, within the front.
    let dw = (front.len * 0.35).min(4.0);
    if dw > 1.6 {
        roller_door(g, c, &front, -front.len * 0.5 + dw * 0.5 + 0.6, dw, 4.2, mul(panel, 0.7));
    }
    if front.len > 6.0 {
        face_door(g, c, &front, front.len * 0.2);
    }
    cable_tray(g, c, &front, -front.len * 0.45, front.len * 0.45, h - 2.2);
    // Roof: rows of chillers.
    let roof = b.inset(2.0);
    let rows = ((roof.d() / 5.0).floor() as i32).clamp(1, 4);
    let cols = ((roof.w() / 5.0).floor() as i32).clamp(1, 6);
    for r in 0..rows {
        for k in 0..cols {
            let p = Vec2::new(roof.u0 + (k as f32 + 0.5) * roof.w() / cols as f32, roof.v0 + (r as f32 + 0.5) * roof.d() / rows as f32);
            chiller(g, c, p, h, 3.6f32.min(roof.w() / cols as f32 - 0.6).max(1.2), 1.8, 0.0, mul(pal.steel, 0.9));
        }
    }
    rails(g, c, &b.inset(0.3), h, mul(pal.steel, 0.6));
    // The plant yard beside the hall: dry coolers, generators, a fuel tank.
    let yu0 = b.u1 + 1.2;
    let yu1 = area.u1 - 2.6;
    if yu1 - yu0 > 2.4 {
        let n = ((b.d() - 1.0) / 3.2).floor() as i32;
        for i in 0..n.clamp(0, 6) {
            let p = Vec2::new((yu0 + yu1) * 0.5, b.v0 + 1.6 + i as f32 * 3.2);
            if !(c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.0) {
                continue;
            }
            if i % 3 == 2 {
                container_unit(g, c, rng, p.x, p.y, std::f32::consts::FRAC_PI_2, 2.6, (yu1 - yu0 - 0.4).min(2.2), 2.5, 0.0, mul([0.86, 0.84, 0.78], 1.0 - 0.3 * c.wear));
            } else {
                f.bx(g, Metal, p.x, 1.1, p.y, Vec3::new((yu1 - yu0 - 0.4).min(2.2), 2.2, 2.6), mul(pal.steel, 0.8));
                g.lod(Lod::Mid, |g| {
                    for s in [-0.6f32, 0.6] {
                        g.part(Matte).cylinder(f.to_world(p.x, p.y + s), 0.5, 0.5, 2.2, 2.4, 10, rgba(DARK));
                    }
                });
            }
        }
        let tp = Vec2::new((yu0 + yu1) * 0.5, b.v1 + 1.6);
        if c.inside_local(tp) && distance_to_boundary(tp, &c.lot_poly) > 1.6 {
            tank_horizontal(g, f, tp, 0.0, (yu1 - yu0).min(5.0), 0.9, 1.3, mul([0.80, 0.78, 0.70], 1.0 - 0.3 * c.wear));
        }
    }
    // The security fence round the lot, the gate on the street; floodlights.
    let ring = offset_poly(&c.lot_poly, 0.5);
    if ring.len() >= 3 {
        fence_lot(g, c, &ring, 2.6, true);
    }
    for p in [Vec2::new(area.u0 + 1.4, area.v0 + 1.6), Vec2::new(area.u1 - 1.4, area.v1 - 1.6), Vec2::new(area.u0 + 1.4, area.v1 - 1.6)] {
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.0 {
            f.bx(g, Matte, p.x, 3.5, p.y, Vec3::new(0.22, 7.0, 0.22), mul(pal.steel, 0.6));
            g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x, 7.1, p.y, Vec3::new(1.0, 0.3, 0.4), CHARCOAL));
            night_box(g, c, p.x, 6.9, p.y, Vec3::new(0.9, 0.08, 0.32), LAMP);
        }
    }
}

/// A steel service door with a lamp over it, lit after dark.
fn face_door(g: &mut Geometry, c: &Ctx, face: &Face, t: f32) {
    let f = &c.frame;
    face.decal(g, f, Metal, t, 1.1, 0.04, 1.1, 2.2, mul(c.palette.steel, 0.55));
    let p = face.at(t, 0.2);
    night_box(g, c, p.x, 2.6, p.y, Vec3::new(0.3, 0.15, 0.2), LAMP);
}

// ── the big house ─────────────────────────────────────────────────────────────

/// A family's or a billionaire's glass pavilion: a long glass box on a
/// concrete plinth under a thin flat roof that floats out over a terrace on
/// slim steel columns, rooms lit behind the glass; on an estate a second
/// pavilion cantilevered over the first; a lap pool along the terrace, a
/// reflecting pond, a lawn and trees, a board-marked concrete wall to the
/// street with a gate, a car on the drive, lamps along the path.
pub fn glass_pavilion(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(site) = fit(c, R::of(&offset_poly(&c.lot_poly, 0.3)), 12.0).map(|r| r.inset(1.0)) else { return student_flats(g, c, rng, e) };
    let big = site.w() * site.d() > 1300.0;
    let fh = 3.6;
    let glass = mix(TINTS[rng.random_range(0..TINTS.len())], [0.30, 0.34, 0.36], 0.35 + 0.2 * c.wear);
    let conc = mix(concrete(c, rng), [0.74, 0.72, 0.68], 0.4);
    ground(g, f, Foliage, &offset_poly(&c.lot_poly, 0.4), 0.03, mul(pal.foliage, 0.8 - 0.25 * c.wear));
    let pw = (site.w() * 0.62).min(if big { 34.0 } else { 24.0 });
    let pd = (site.d() * 0.3).clamp(7.0, if big { 13.0 } else { 10.0 });
    let pv0 = site.v0 + (site.d() * 0.3).clamp(5.0, 12.0);
    let house = R::new(site.c().x - pw * 0.5, site.c().x + pw * 0.5, pv0, pv0 + pd);
    if house.v1 > site.v1 - 3.0 {
        return student_flats(g, c, rng, e);
    }
    // The plinth and terrace, the glass box, rooms lit behind the glass.
    let plinth = R::new(house.u0 - 3.0, house.u1 + 3.0, house.v0 - 3.0, house.v1 + 1.0);
    let plinth = fit(c, plinth, 6.0).unwrap_or(house);
    block(g, c, Concrete, &plinth, 0.0, 0.6, conc);
    block(g, c, Glass, &house, 0.6, 0.6 + fh, glass);
    for face in house.faces() {
        let n = ((face.len - 1.0) / 3.0).floor() as i32;
        for i in 0..n.max(0) {
            let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * (face.len - 1.0) / n as f32;
            if coin(rng, c.lit_chance() * 0.8) {
                let col = mul(pal.window_warm, rng.random_range(0.5..0.85));
                face.glaze(g, c, t, 0.6 + fh * 0.45, PANE, (face.len - 1.0) / n as f32 - 0.25, fh * 0.8, Pane::Lit(col));
            }
        }
        g.lod(Lod::Mid, |g| {
            for i in 0..=n.max(0) {
                let t = -face.len * 0.5 + 0.3 + i as f32 * (face.len - 0.6) / n.max(1) as f32;
                face.decal(g, f, Matte, t, 0.6 + fh * 0.5, 0.065, 0.07, fh, DARK);
            }
        });
    }
    // The floating roof and its columns.
    let roof = R::new(house.u0 - 2.5, house.u1 + 2.5, house.v0 - 2.8, house.v1 + 0.8);
    let roof = fit(c, roof, 6.0).unwrap_or(house.inset(-0.5));
    let ry = 0.6 + fh;
    block(g, c, Concrete, &roof, ry, ry + 0.35, mul(conc, 1.05));
    let col_c = mul(SPANDRELS[0], 1.0);
    for (u, v) in [(roof.u0 + 0.4, roof.v0 + 0.4), (roof.u1 - 0.4, roof.v0 + 0.4), (roof.c().x, roof.v0 + 0.4)] {
        f.bx(g, Matte, u, (0.6 + ry) * 0.5, v, Vec3::new(0.16, ry - 0.6, 0.16), col_c);
    }
    // A strip of light under the eave after dark.
    night_box(g, c, roof.c().x, ry - 0.04, roof.v0 + 0.9, Vec3::new(roof.w() - 2.0, 0.04, 0.12), LAMP);
    // The estate's upper pavilion, cantilevered across the first.
    if big {
        let up = R::new(house.u0 + pw * 0.35, house.u1 + 1.5f32.min(roof.u1 - house.u1 - 0.3), house.v0 + 1.0, house.v1 + 0.5f32.min(roof.v1 - house.v1 - 0.2));
        let y0 = ry + 0.35;
        block(g, c, Glass, &up, y0, y0 + fh, mul(glass, 0.9));
        for face in up.faces() {
            if coin(rng, c.lit_chance()) && face.len > 3.0 {
                let col = mul(pal.window_warm, rng.random_range(0.5..0.85));
                face.glaze(g, c, 0.0, y0 + fh * 0.45, PANE, face.len - 1.6, fh * 0.7, Pane::Lit(col));
            }
        }
        block(g, c, Concrete, &up.inset(-0.6), y0 + fh, y0 + fh + 0.3, conc);
        rails(g, c, &R::new(roof.u0 + 0.3, up.u0 - 0.6, roof.v0 + 0.3, roof.v1 - 0.3), ry + 0.35, mul(SPANDRELS[0], 1.1));
    }
    // The lap pool along the terrace, the pond.
    let pool_r = R::new(plinth.u0 + 0.8, plinth.u0 + 0.8 + (pw * 0.6).min(18.0), plinth.v0 - 3.6, plinth.v0 - 0.8);
    if pool_r.v0 > site.v0 + 2.0 {
        pool(g, c, pool_r.c().x, pool_r.c().y, pool_r.w(), pool_r.d() - 1.2);
    }
    // The wall to the street, the gate, the drive, a car, lamps.
    let wall_v = site.v0 + 0.25;
    let gw = 4.2;
    let gu = site.u1 - gw * 0.5 - 2.0;
    for (u0, u1) in [(site.u0, gu - gw * 0.5), (gu + gw * 0.5, site.u1)] {
        if u1 - u0 > 0.8 {
            f.bx(g, Concrete, (u0 + u1) * 0.5, 1.0, wall_v, Vec3::new(u1 - u0, 2.0, 0.35), mul(conc, 0.85));
        }
    }
    g.lod(Lod::Near, |g| {
        let mut y = 0.3;
        while y < 2.0 {
            f.bx(g, Matte, (site.u0 + gu - gw * 0.5) * 0.5, y, wall_v - 0.19, Vec3::new((gu - gw * 0.5 - site.u0).max(0.1), 0.02, 0.02), mul(conc, 0.6));
            y += 0.4;
        }
    });
    let drive = R::new(gu - gw * 0.5 + 0.4, gu + gw * 0.5 - 0.4, wall_v + 0.2, house.v0 - 0.5);
    if drive.d() > 3.0 {
        ground(g, f, Paving, &drive.poly(), 0.05, mul(pal.paving, 0.85));
        let cp = Vec2::new(drive.c().x, drive.v1 - 3.0);
        if drive.d() > 7.0 && vehicle_fits(c, cp, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
            vehicle(g, f, pal, rng, cp, 0.05, std::f32::consts::FRAC_PI_2, Vehicle::Car, [[0.08, 0.08, 0.09], [0.82, 0.82, 0.80], [0.30, 0.32, 0.34]][(c.seed % 3) as usize]);
        }
        for k in 0..3 {
            lamp(g, c, Vec2::new(drive.u0 - 0.6, drive.v0 + 2.0 + k as f32 * (drive.d() - 3.0) / 2.0), 1.0);
        }
    }
    // Trees about the lawn behind and beside.
    for i in 0..if big { 6 } else { 3 } {
        let p = Vec2::new(site.u0 + 2.0 + (i as f32 * 0.37).fract() * (site.w() - 4.0), house.v1 + 3.0 + (i % 3) as f32 * ((site.v1 - house.v1 - 4.0).max(0.0) / 3.0));
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.5 && p.y < site.v1 - 1.0 {
            tree(g, f, p, rng.random_range(5.0..8.0), 2.4, pal.bark, mul(pal.foliage, 1.0 - 0.2 * c.wear));
        }
    }
    let pond = R::new(site.u0 + 1.5, (site.u0 + 1.5 + site.w() * 0.25).min(house.u0 - 1.0), house.v1 + 1.5, (house.v1 + 5.0).min(site.v1 - 1.0));
    if pond.w() > 3.0 && pond.d() > 2.0 {
        f.bx(g, Glow, pond.c().x, 0.08, pond.c().y, Vec3::new(pond.w(), 0.1, pond.d()), mul(pal.water, 0.55));
    }
}

/// [r4-glass] The style a trade builds in a Glass Campus district where the
/// look has one for it: the assembly halls (clean high-tech industry) are
/// server halls there rather than fab plants and data centres; the
/// cheapest keep their workshops.
pub fn trade_style(kind: Option<nd_sim::BusinessKind>, tier: nd_sim::Tier) -> Option<super::Style> {
    use nd_sim::BusinessKind as B;
    match kind? {
        B::Assembly if tier != nd_sim::Tier::Low => Some(super::Style::ServerHall),
        _ => None,
    }
}
