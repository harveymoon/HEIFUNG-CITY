//! [r4-brick] The Brick & Steel look of the Foundry Row story: red-brick
//! worker rows, warehouse lofts with steel windows and rooftop water
//! towers, a converted mill, brick workshops, the corner pub, the union
//! hall and a loft estate for the district's rich (the McMansion and the
//! billionaire's mansion of a Foundry Row district). The works themselves
//! (saw-tooth sheds, the foundry, the gas works) are in `brick_works.rs`.
//!
//! The look is stylised and gritty: soot-darkened brick on a plinth of
//! blue engineering brick, stone sills and soldier-course lintels, steel
//! windows of many small panes, slate roofs with chimney stacks, painted
//! ghost signs on blank party walls, fire escapes and hoist beams. At
//! night the windows glow amber and the yards and walls are lit by
//! sodium-orange lamps (lit only after dark, `NIGHT_MARK`).
//!
//! Every choice comes from the building's seed: the mass and the windows
//! from its main stream, the per-window frame and pane grid from a hash
//! of the seed, the face and the window, so the same building is the same
//! from every camera. Sills, lintels, glazing bars, pots and braces go into
//! the detail layers ([`Lod`]) the renderer drops with distance.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::depth::{FRAME, PANE};
use crate::hash::hash2_salted;
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Common red bricks, from bright to sooty.
pub(super) const BRICKS: [[f32; 3]; 5] = [[0.42, 0.17, 0.11], [0.48, 0.21, 0.13], [0.36, 0.15, 0.11], [0.45, 0.24, 0.16], [0.32, 0.15, 0.12]];
/// Tar and gravel on a flat roof.
pub(super) const ROOFING: [f32; 3] = [0.15, 0.14, 0.14];
/// Blue engineering brick for plinths, copings and chimney bands.
pub(super) const ENGINEERING: [f32; 3] = [0.17, 0.15, 0.16];
/// Sills, lintels and copings.
pub(super) const STONE: [f32; 3] = [0.62, 0.58, 0.50];
/// Window steel: black, works green, oxide red, gunmetal.
pub(super) const STEELS: [[f32; 3]; 4] = [[0.11, 0.12, 0.13], [0.17, 0.24, 0.21], [0.32, 0.13, 0.10], [0.22, 0.24, 0.27]];
/// Welsh slate.
pub(super) const SLATE: [f32; 3] = [0.13, 0.14, 0.16];
/// A sodium street lamp.
pub(super) const SODIUM: [f32; 3] = [2.3, 1.1, 0.26];
/// The glow of a furnace mouth or a pour.
pub(super) const FURNACE: [f32; 3] = [2.6, 0.85, 0.18];
/// Soot.
pub(super) const SOOT: [f32; 3] = [0.09, 0.08, 0.08];
/// Weathered cedar of a water tower's tank.
const TANK_WOOD: [f32; 3] = [0.40, 0.30, 0.22];
/// Warm window lights: amber, tungsten, sodium spill.
pub(super) const WARM: [[f32; 3]; 4] = [[1.35, 0.85, 0.40], [1.25, 0.95, 0.55], [1.40, 0.72, 0.30], [1.15, 0.90, 0.62]];
/// Painted doors: works green, oxblood, black, faded blue.
const DOORS: [[f32; 3]; 4] = [[0.14, 0.26, 0.18], [0.36, 0.08, 0.07], [0.08, 0.08, 0.09], [0.20, 0.27, 0.36]];
/// Faded paint of a ghost sign: cream, white, ochre.
const GHOST: [[f32; 3]; 3] = [[0.70, 0.64, 0.50], [0.74, 0.72, 0.66], [0.66, 0.50, 0.24]];
/// Salt of the per-window hash.
const PANE_SALT: u64 = 0xB41C_57EE;
/// Depths off a wall: lintels and ghost signs under the frames ([`FRAME`]),
/// glazing bars just proud of the glass ([`PANE`]).
const LINTEL: f32 = 0.012;
const GHOST_INK: f32 = 0.016;
/// A carved band over the lintels and the grime (0.02), under the frames.
const BAND: f32 = 0.0215;
const BAR: f32 = PANE + 0.008;

/// One building's colours.
#[derive(Clone, Copy)]
pub(super) struct Paint {
    pub brick: [f32; 3],
    /// The plinth, copings and bands.
    pub dark: [f32; 3],
    pub trim: [f32; 3],
    /// Window steel.
    pub steel: [f32; 3],
    pub roof: [f32; 3],
    pub door: [f32; 3],
}

/// A building's paint: a brick darkened by soot as it wears.
pub(super) fn paint(c: &Ctx, rng: &mut impl Rng) -> Paint {
    let soot = 1.0 - 0.25 * c.wear;
    Paint {
        brick: mul(pick(rng, &BRICKS), soot * rng.random_range(0.92..1.06)),
        dark: mul(ENGINEERING, rng.random_range(0.9..1.15)),
        trim: mul(STONE, 1.0 - 0.3 * c.wear),
        steel: pick(rng, &STEELS),
        roof: mul(SLATE, rng.random_range(0.9..1.1)),
        door: pick(rng, &DOORS),
    }
}

/// A warm window light, or dark glass, by the building's lit chance.
pub(super) fn light(c: &Ctx, rng: &mut impl Rng) -> Pane {
    if coin(rng, c.lit_chance()) {
        Pane::Lit(mul(pick(rng, &WARM), rng.random_range(0.6..1.0)))
    } else {
        Pane::Dark
    }
}

/// A face's identity for the per-window hash: its middle, to the decimetre.
fn face_key(c: &Ctx, face: &Face) -> u64 {
    let m = face.mid();
    hash2_salted((m.x * 10.0).round() as i64 as u64, (m.y * 10.0).round() as i64 as u64, PANE_SALT ^ c.seed)
}

/// A steel window `w` by `h` centred `t` along a face at height `y`: a
/// steel frame, the glazing, and (detail) a soldier-course lintel, a stone
/// sill and glazing bars making `nu` by `nv` small panes.
#[allow(clippy::too_many_arguments)]
pub(super) fn steel_window(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, h: f32, nu: u32, nv: u32, pane: Pane, pt: &Paint) {
    let f = &c.frame;
    face.decal(g, f, Matte, t, y, FRAME, w + 0.16, h + 0.16, pt.steel);
    face.glaze(g, c, t, y, PANE, w, h, pane);
    g.lod(Lod::Mid, |g| {
        face.decal(g, f, Brick, t, y + h * 0.5 + 0.24, LINTEL, w + 0.5, 0.3, mul(pt.brick, 0.78));
        super::hk::wall_box(g, c, Stone, face, t, y - h * 0.5 - 0.12, 0.0, Vec3::new(w + 0.3, 0.12, 0.14), pt.trim);
    });
    g.lod(Lod::Near, |g| {
        for i in 1..nu {
            let bt = t - w * 0.5 + w * i as f32 / nu as f32;
            face.decal(g, f, Matte, bt, y, BAR, 0.06, h, pt.steel);
        }
        for j in 1..nv {
            let by = y - h * 0.5 + h * j as f32 / nv as f32;
            face.decal(g, f, Matte, t, by, BAR + 0.002, w, 0.06, pt.steel);
        }
    });
}

/// Steel windows across a face, `floors` storeys from `y0`, one a bay of
/// about `spacing`: each `wf` of its bay wide and `hf` of a storey high.
/// The pane grid changes a little from window to window (the hash).
#[allow(clippy::too_many_arguments)]
pub(super) fn window_rows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spacing: f32, wf: f32, hf: f32, pt: &Paint) {
    let usable = face.len - 1.2;
    let n = (usable / spacing).floor() as i32;
    if n < 1 || floors < 1 {
        return;
    }
    let step = usable / n as f32;
    let (w, h) = ((step * wf).min(3.6), (fh * hf).min(3.2));
    let key = face_key(c, face);
    for k in 0..floors {
        let y = y0 + k as f32 * fh + fh * 0.52;
        for i in 0..n {
            let t = -usable * 0.5 + (i as f32 + 0.5) * step;
            let hh = hash2_salted(key, ((k as u64) << 16) | i as u64, PANE_SALT);
            let nu = 2 + (hh % 3) as u32 * (w > 1.6) as u32;
            let nv = 3 + ((hh >> 8) % 2) as u32;
            let pane = light(c, rng);
            steel_window(g, c, face, t, y, w, h, nu, nv, pane, pt);
        }
    }
}

/// The brick body of a block: the walls on a plinth of engineering brick,
/// a string course every `course` floors and a stone-coped parapet.
#[allow(clippy::too_many_arguments)]
pub(super) fn brick_body(g: &mut Geometry, c: &Ctx, poly: &[Vec2], h: f32, floors: i32, fh: f32, course: i32, parapet_h: f32, pt: &Paint) {
    let f = &c.frame;
    extrude(g, f, Brick, poly, 0.0, h, pt.brick);
    cornice(g, c, poly, 0.45, 0.9, 0.06, Brick, pt.dark);
    if course > 0 {
        for k in (course..floors).step_by(course as usize) {
            cornice(g, c, poly, k as f32 * fh, 0.22, 0.07, Stone, pt.trim);
        }
    }
    if parapet_h > 0.0 {
        parapet(g, c, poly, h, parapet_h, 0.35, Brick, pt.brick);
        cornice(g, c, poly, h + parapet_h + 0.06, 0.12, 0.08, Stone, pt.trim);
        // The flat roof's tar and gravel inside the parapet.
        let roof = offset_poly(poly, 0.36);
        if roof.len() >= 3 {
            ground(g, f, Concrete, &roof, h + 0.03, mul(ROOFING, 1.0 + 0.3 * c.wear));
        }
    }
}

/// A slate roof on a rectangle: two slopes `rise` high meeting at a ridge
/// along u (`along_u`) or v, the eaves `eave` out over the long walls and
/// brick gables at the ends.
#[allow(clippy::too_many_arguments)]
pub(super) fn pitched(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, along_u: bool, eave: f32, roof: [f32; 3], gable: [f32; 3]) {
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
    for s in [-1.0f32, 1.0] {
        let be = bm + s * (half + eave);
        let n = (d(0.0, s) * rise + Vec3::Y * half).normalize();
        g.part(Panel).quad_facing([p(a0, ye, be), p(a1, ye, be), p(a1, y + rise, bm), p(a0, y + rise, bm)], n, col);
    }
    for (a, s) in [(a0, -1.0f32), (a1, 1.0)] {
        g.part(Brick).tri_facing(p(a, y, b0), p(a, y, b1), p(a, y + rise, bm), d(s, 0.0), rgba(gable));
    }
}

/// A house chimney stack at lot-local `p` standing from `y` (under the
/// roof) to `top`, with pots.
pub(super) fn chimney_stack(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, top: f32, w: f32, pt: &Paint) {
    let f = &c.frame;
    f.bx(g, Brick, p.x, (y + top) * 0.5, p.y, Vec3::new(w, top - y, 0.7), mul(pt.brick, 0.85));
    f.bx(g, Brick, p.x, top + 0.1, p.y, Vec3::new(w + 0.16, 0.2, 0.86), mul(pt.dark, 1.2));
    g.lod(Lod::Mid, |g| {
        let n = ((w / 0.5) as i32).clamp(1, 4);
        for k in 0..n {
            let u = p.x - w * 0.5 + (k as f32 + 0.5) * w / n as f32;
            let wp = f.to_world(u, p.y);
            g.part(Concrete).cylinder(wp, 0.14, 0.11, top + 0.2, top + 0.75, 6, rgba([0.55, 0.32, 0.24]));
        }
    });
}

/// A sodium lamp on a bracket off a face at height `y`: lit after dark.
pub(super) fn wall_lamp(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32) {
    let f = &c.frame;
    g.lod(Lod::Mid, |g| face.bx(g, f, Matte, t, y + 0.12, 0.38, Vec3::new(0.06, 0.06, 0.76), CHARCOAL));
    face.bx(g, f, Matte, t, y, 0.72, Vec3::new(0.46, 0.16, 0.34), CHARCOAL);
    let from = g.get(Glow).colors.len();
    face.bx(g, f, Glow, t, y - 0.2, 0.72, Vec3::new(0.3, 0.22, 0.22), SODIUM);
    mark_glow(g, from, NIGHT_MARK);
}

/// A sodium lamp post on the lot (skipped near the edge), lit after dark.
pub(super) fn yard_lamp(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 0.6 {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Matte).cylinder(w, 0.1, 0.07, 0.0, h, 6, rgba(CHARCOAL));
    f.bx(g, Matte, p.x, h + 0.06, p.y - 0.3, Vec3::new(0.3, 0.14, 0.8), CHARCOAL);
    let from = g.get(Glow).colors.len();
    f.bx(g, Glow, p.x, h - 0.14, p.y - 0.4, Vec3::new(0.26, 0.22, 0.56), SODIUM);
    // The pool of orange light it throws on the ground.
    let pool_c = Vec2::new(p.x, p.y - 0.4);
    let r = (h * 0.45).min(distance_to_boundary(pool_c, &c.lot_poly) - 0.2);
    if r > 1.0 {
        ground(g, f, Glow, &ngon(pool_c.x, pool_c.y, r, 12, 0.0), 0.065, mul(SODIUM, 0.11));
    }
    mark_glow(g, from, NIGHT_MARK);
}

/// A timber water tank on a steel stand at lot-local `p` on a roof at
/// `y`: four legs braced (detail), a platform, the barrel with its hoops
/// (detail) and a conical cap.
pub(super) fn water_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32, r: f32) {
    let f = &c.frame;
    let legs = rng.random_range(2.4..3.6);
    let steel = mul(STEELS[0], 1.4);
    let k = r * 0.72;
    let corners = [(-k, -k), (k, -k), (k, k), (-k, k)];
    for (du, dv) in corners {
        f.bx(g, Matte, p.x + du, y + legs * 0.5, p.y + dv, Vec3::new(0.16, legs, 0.16), steel);
    }
    g.lod(Lod::Near, |g| {
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            g.part(Matte).beam(f.at(p.x + a.0, y + 0.2, p.y + a.1), f.at(p.x + b.0, y + legs - 0.3, p.y + b.1), 0.06, rgba(steel));
            g.part(Matte).beam(f.at(p.x + b.0, y + 0.2, p.y + b.1), f.at(p.x + a.0, y + legs - 0.3, p.y + a.1), 0.06, rgba(steel));
        }
    });
    let w = f.to_world(p.x, p.y);
    let base = y + legs;
    let th = r * rng.random_range(1.4..1.8);
    g.part(Matte).cylinder(w, r + 0.3, r + 0.3, base - 0.16, base, 12, rgba(steel));
    let wood = mul(TANK_WOOD, rng.random_range(0.85..1.1) * (1.0 - 0.2 * c.wear));
    g.part(Wood).cylinder_capped(w, r, r * 0.96, base, base + th, 14, false, false, rgba(wood));
    g.part(Metal).cylinder(w, r + 0.14, 0.1, base + th, base + th + r * 0.6, 14, rgba(mul(SLATE, 0.8)));
    g.lod(Lod::Near, |g| {
        for j in 1..4 {
            let hy = base + th * j as f32 / 4.0;
            g.part(Matte).cylinder_capped(w, r + 0.03, r + 0.03, hy - 0.05, hy + 0.05, 14, false, false, rgba(CHARCOAL));
        }
        g.part(Matte).cylinder(w, 0.05, 0.05, base + th + r * 0.6, base + th + r * 0.6 + 0.6, 4, rgba(CHARCOAL));
    });
}

/// A tall works chimney of brick at lot-local `p`: square and tapering, a
/// band of blue brick, a corbelled cap black with soot and a red light on
/// a tall one.
pub(super) fn works_stack(g: &mut Geometry, c: &Ctx, p: Vec2, y0: f32, h: f32, base: f32, pt: &Paint) {
    let f = &c.frame;
    let top = base * 0.62;
    let at = |k: f32| base + (top - base) * k;
    let shaft = y0 + h * 0.9;
    loft_capped(g, f, Brick, &rect(p.x, p.y, base, base), &rect(p.x, p.y, top, top), y0, shaft, false, false, pt.brick);
    let cap = top * 1.22;
    extrude(g, f, Brick, &rect(p.x, p.y, cap, cap), shaft, y0 + h, mix(pt.brick, SOOT, 0.7));
    ground(g, f, Matte, &rect(p.x, p.y, top * 0.75, top * 0.75), y0 + h + 0.02, SOOT);
    g.lod(Lod::Mid, |g| {
        for kf in [0.3f32, 0.62] {
            let r = at(kf) + 0.06;
            let y = y0 + (shaft - y0) * kf;
            extrude(g, f, Brick, &rect(p.x, p.y, r, r), y, y + 0.6, pt.dark);
        }
    });
    if y0 + h > 22.0 {
        aviation_light_local(g, f, c.palette, p.x, y0 + h + 0.4, p.y, 0.35);
    }
}

/// A faded painted advert on a blank wall between `y0` and `y1`: a cream
/// ground, a dark band and blocky letters, worn by the weather.
pub(super) fn ghost_sign(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, y1: f32, w: f32, pt: &Paint) {
    let f = &c.frame;
    let h = y1 - y0;
    if h < 1.5 || w < 3.0 {
        return;
    }
    let ground_col = mix(pick(rng, &GHOST), pt.brick, 0.35 + 0.3 * c.wear);
    face.decal(g, f, Matte, t, (y0 + y1) * 0.5, LINTEL, w, h, ground_col);
    let ink = mix(pick(rng, &[[0.16, 0.12, 0.10], [0.42, 0.10, 0.08], [0.14, 0.20, 0.26]]), ground_col, 0.25);
    face.decal(g, f, Matte, t, y0 + h * 0.18, GHOST_INK, w * 0.92, h * 0.12, ink);
    // Two lines of blocky letters.
    for (line, ly) in [(0, y0 + h * 0.66), (1, y0 + h * 0.4)] {
        let n = rng.random_range(4..9);
        let lw = w * if line == 0 { 0.84 } else { 0.6 };
        let lh = h * if line == 0 { 0.22 } else { 0.14 };
        let step = lw / n as f32;
        for i in 0..n {
            if coin(rng, 0.12) {
                continue;
            }
            let lt = t - lw * 0.5 + (i as f32 + 0.5) * step;
            face.decal(g, f, Matte, lt, ly, GHOST_INK, step * 0.7, lh, ink);
        }
    }
}

/// Shops along a ground floor `fh` high: per shop a wide steel shop
/// window and a door, under one painted fascia board with each shop's
/// name in lit letters.
fn shop_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, fh: f32, pt: &Paint) {
    let f = &c.frame;
    let usable = face.len - 1.0;
    let n = (usable / 5.0).floor().max(1.0) as i32;
    let step = usable / n as f32;
    let board = pick(rng, &[[0.10, 0.18, 0.13], [0.28, 0.08, 0.06], [0.08, 0.08, 0.10], [0.20, 0.16, 0.10]]);
    face.decal(g, f, Wood, 0.0, fh * 0.9, BAND, usable, 0.55, board);
    let from = g.get(Glow).colors.len();
    for k in 0..n {
        let t = -usable * 0.5 + (k as f32 + 0.5) * step;
        let shop = Paint { door: pick(rng, &DOORS), ..*pt };
        steel_window(g, c, face, t - step * 0.12, fh * 0.4, step * 0.55, fh * 0.45, 4, 2, light(c, rng), &shop);
        door(g, c, face, t + step * 0.32, 0.9, 2.2, &shop, light(c, rng));
        let col = mul(pick(rng, &c.palette.sign_colors), 0.8);
        let letters = ((step * 0.6 / 0.45) as i32).clamp(2, 9);
        let lw = step * 0.6 / letters as f32;
        for i in 0..letters {
            face.decal(g, f, Glow, t - step * 0.3 + (i as f32 + 0.5) * lw, fh * 0.9, BAND + 0.004, lw * 0.7, 0.32, col);
        }
    }
    mark_glow(g, from, NEON_MARK);
}

/// A plain door `w` wide in a face at `t`, with a fanlight over it and a
/// stone step (detail).
fn door(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, w: f32, h: f32, pt: &Paint, fan: Pane) {
    let f = &c.frame;
    face.decal(g, f, Matte, t, h * 0.5 + 0.1, FRAME, w + 0.2, h + 0.1, mul(pt.steel, 1.2));
    face.decal(g, f, Wood, t, h * 0.5 + 0.1, PANE, w, h, pt.door);
    face.glaze(g, c, t, h + 0.45, PANE, w, 0.5, fan);
    g.lod(Lod::Mid, |g| face.bx(g, f, Stone, t, 0.08, 0.2, Vec3::new(w + 0.5, 0.16, 0.4), pt.trim));
}

/// Something low that fits any scrap of a lot: a one-storey brick shed
/// with a slate roof and a steel window.
fn small_shed(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pt: &Paint) {
    let r = R::of(&c.poly);
    let Some(r) = fit(c, r.inset(0.2), 2.0) else { return };
    let h = c.fh().min(3.4);
    block(g, c, Brick, &r, 0.0, h, pt.brick);
    pitched(g, c, &r, h, (r.d().min(r.w()) * 0.25).min(1.6), r.w() >= r.d(), 0.0, pt.roof, pt.brick);
    let front = r.front();
    if front.len > 1.6 {
        steel_window(g, c, &front, 0.0, h * 0.55, (front.len * 0.5).min(2.0), h * 0.4, 2, 3, light(c, rng), pt);
    }
}

// ── worker rows ──────────────────────────────────────────────────────────────

/// A terrace of red-brick worker houses along the street: one house every
/// five metres or so, each its own shade of brick, a door with a fanlight
/// and a step, steel sash windows, a slate roof over the row with a
/// chimney stack on every party wall, and back yards behind walls with an
/// outhouse and a washing line. Sodium lamps on a few fronts.
pub fn brick_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().min(3.4);
    let floors = (c.floors as i32).clamp(2, 4);
    let h = floors as f32 * fh;
    let area = R::of(&c.poly);
    let depth = if area.d() < 11.0 { area.d() } else { (area.d() * 0.5).clamp(8.0, 11.0) };
    let Some(row) = fit(c, R::new(area.u0, area.u1, area.v0, area.v0 + depth), 4.0) else {
        small_shed(g, c, rng, &pt);
        return;
    };
    let n = ((row.w() / 5.4).round() as i32).max(1);
    let hw = row.w() / n as f32;
    let rise = (row.d() * 0.32).clamp(1.2, 3.0);
    let ridge = h + rise;
    for i in 0..n {
        let hr = R::new(row.u0 + i as f32 * hw, row.u0 + (i + 1) as f32 * hw, row.v0, row.v1);
        let tone = mul(pt.brick, rng.random_range(0.9..1.08));
        block(g, c, Brick, &hr, 0.0, h, tone);
        let front = hr.front();
        // The door to one side, the windows beside it and over it.
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dt = side * hw * 0.27;
        door(g, c, &front, dt, 0.95, 2.2, &Paint { door: pick(rng, &DOORS), ..pt }, light(c, rng));
        let ww = (hw * 0.32).clamp(0.8, 1.4);
        steel_window(g, c, &front, -dt, fh * 0.52, ww, fh * 0.5, 2, 2, light(c, rng), &pt);
        for k in 1..floors {
            let y = k as f32 * fh + fh * 0.52;
            let pairs: &[f32] = if hw > 6.2 { &[-0.25, 0.25] } else { &[0.0] };
            for &tt in pairs {
                steel_window(g, c, &front, tt * hw, y, ww, fh * 0.5, 2, 2, light(c, rng), &pt);
            }
        }
        let back = hr.back();
        for k in 0..floors {
            steel_window(g, c, &back, side * hw * 0.2, k as f32 * fh + fh * 0.52, ww, fh * 0.45, 2, 2, light(c, rng), &pt);
        }
        if i > 0 {
            chimney_stack(g, c, Vec2::new(hr.u0, row.c().y), ridge - 0.6, ridge + 1.1, 1.0, &pt);
        }
        if i == n / 2 || coin(rng, 0.3) {
            wall_lamp(g, c, &front, -dt * 0.2, 2.9);
        }
        grime(g, c, rng, &front, 0.0, h, 1.0 + 2.0 * c.wear);
    }
    cornice(g, c, &row.poly(), 0.3, 0.6, 0.05, Brick, pt.dark);
    pitched(g, c, &row, h, rise, true, 0.3, pt.roof, pt.brick);
    for (face, end) in [(row.left(), -1.0f32), (row.right(), 1.0)] {
        if face.len > 3.0 {
            chimney_stack(g, c, Vec2::new(if end < 0.0 { row.u0 + 0.5 } else { row.u1 - 0.5 }, row.c().y), ridge - 0.8, ridge + 1.0, 0.9, &pt);
            downpipes(g, c, rng, &face, h);
        }
    }
    // Back yards: a wall between each, an outhouse at the end, washing.
    let back_room = area.v1 - row.v1;
    if back_room > 4.0 {
        let yd = back_room.min(9.0) - 0.4;
        let yw = 1.8;
        for i in 0..=n {
            let u = row.u0 + i as f32 * hw;
            let a = Vec2::new(u, row.v1 + 0.15);
            let b = Vec2::new(u, row.v1 + yd);
            if c.inside_local(a) && c.inside_local(b) {
                f.bx(g, Brick, u, yw * 0.5, (a.y + b.y) * 0.5, Vec3::new(0.24, yw, yd - 0.15), mul(pt.brick, 0.9));
            }
        }
        let wall_v = row.v1 + yd;
        if c.inside_local(Vec2::new(row.u0, wall_v)) && c.inside_local(Vec2::new(row.u1, wall_v)) {
            f.bx(g, Brick, row.c().x, yw * 0.5, wall_v, Vec3::new(row.w(), yw, 0.24), mul(pt.brick, 0.9));
        }
        for i in 0..n {
            let u = row.u0 + (i as f32 + 0.5) * hw;
            let p = Vec2::new(u + hw * 0.22, wall_v - 1.0);
            let shed = rect(p.x, p.y, (hw * 0.18).min(0.9), 0.7);
            if poly_inside(c, &shed) && hw > 3.0 {
                extrude(g, f, Brick, &shed, 0.0, 2.1, mul(pt.brick, 0.95));
                f.bx(g, Panel, p.x, 2.17, p.y, Vec3::new((hw * 0.36).min(1.8) + 0.2, 0.1, 1.6), pt.roof);
            }
            g.lod(Lod::Near, |g| {
                let (a, b) = (f.at(u - hw * 0.35, 1.9, row.v1 + yd * 0.45), f.at(u + hw * 0.3, 1.9, row.v1 + yd * 0.55));
                if c.spans_lot(Vec2::new(u - hw * 0.35, row.v1 + yd * 0.45), Vec2::new(u + hw * 0.3, row.v1 + yd * 0.55)) {
                    g.part(Matte).wire(a, b, 0.15, 0.02, rgba(DARK));
                    for j in 1..4 {
                        let q = a.lerp(b, j as f32 / 4.0);
                        g.part(Tarp).boxed(q - Vec3::Y * 0.35, Vec3::new(0.5, 0.55, 0.03), f.yaw(), rgba(mul(pick(rng, &c.palette.tarp_colors), 1.2)));
                    }
                }
            });
        }
        ground(g, f, Paving, &clip_box(&c.poly, row.u0, row.u1, row.v1 + 0.05, wall_v), 0.04, mul(c.palette.paving, 0.75));
    }
}

// ── warehouse lofts ──────────────────────────────────────────────────────────

/// What a loft block carries besides its walls.
#[derive(Clone, Copy)]
struct LoftDress {
    /// Shops or a loading dock on the ground floor.
    shops: bool,
    /// The blank party wall with its ghost sign.
    ghost: bool,
    water_tower: bool,
}

/// A brick warehouse block on `r`, `floors` high: brick piers between
/// bays (detail), big steel windows, stacked loading doors under a hoist
/// beam in the middle bay, a fire escape, a blank side with a ghost sign,
/// and on the roof a stair bulkhead and a water tower. Returns the roof
/// height.
fn loft_block(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, floors: i32, fh: f32, pt: &Paint, dress: LoftDress) -> f32 {
    let f = &c.frame;
    let h = floors as f32 * fh;
    let poly = r.poly();
    brick_body(g, c, &poly, h, floors, fh, 2, 0.9, pt);
    let bay = 4.2;
    let faces = r.faces();
    // The blank party wall: the left or right side, by the seed.
    let blank = if dress.ghost { Some(if coin(rng, 0.5) { 3 } else { 1 }) } else { None };
    for (i, face) in faces.iter().enumerate() {
        if face.len < 2.5 {
            continue;
        }
        let y0 = if i == 0 { fh } else { 0.0 };
        let fl = if i == 0 { floors - 1 } else { floors };
        if Some(i) == blank {
            // Windows only on the ground floor; a painted advert up high.
            window_rows(g, c, rng, face, 0.0, 1, fh, bay * 1.3, 0.5, 0.55, pt);
            if floors >= 3 {
                let y1 = h - 0.6;
                let y0 = (y1 - fh * 2.4).max(fh * 1.3);
                let w = (face.len * 0.7).min(16.0);
                ghost_sign(g, c, rng, face, 0.0, y0, y1, w, pt);
            }
        } else if i == 0 && face.len > bay * 2.2 {
            // The middle bay is the loading bay: doors stacked up to a hoist.
            let n = ((face.len - 1.2) / bay).floor().max(1.0) as i32;
            let step = (face.len - 1.2) / n as f32;
            let mid = n / 2;
            let key = face_key(c, face);
            for k in 0..fl {
                let y = y0 + k as f32 * fh + fh * 0.52;
                for b in 0..n {
                    let t = -(face.len - 1.2) * 0.5 + (b as f32 + 0.5) * step;
                    if b == mid {
                        let dw = (step * 0.5).min(2.2);
                        face.decal(g, f, Matte, t, y - fh * 0.06, FRAME, dw + 0.2, fh * 0.78, pt.steel);
                        face.decal(g, f, Wood, t, y - fh * 0.06, PANE, dw, fh * 0.72, mul(pt.door, 0.9));
                        continue;
                    }
                    let hh = hash2_salted(key, ((k as u64) << 16) | b as u64, PANE_SALT);
                    let w = (step * 0.62).min(3.4);
                    steel_window(g, c, face, t, y, w, (fh * 0.6).min(3.0), 3 + (hh % 2) as u32, 3 + ((hh >> 4) % 2) as u32, light(c, rng), pt);
                }
            }
            let ht = -(face.len - 1.2) * 0.5 + (mid as f32 + 0.5) * step;
            let reach = c.reach_out(face).min(1.4) - 0.3;
            if reach > 0.5 {
                g.lod(Lod::Mid, |g| {
                    face.bx(g, f, Matte, ht, h - 0.25, reach * 0.5, Vec3::new(0.22, 0.3, reach), pt.steel);
                    face.bx(g, f, Matte, ht, h - 1.5, reach - 0.1, Vec3::new(0.04, 2.2, 0.04), DARK);
                });
            }
        } else {
            window_rows(g, c, rng, face, y0, fl, fh, bay, 0.62, 0.6, pt);
        }
        // Brick piers between the bays, the height of the wall.
        if Some(i) != blank && face.len > bay * 1.5 {
            let n = ((face.len - 1.2) / bay).floor() as i32;
            let step = (face.len - 1.2) / n.max(1) as f32;
            g.lod(Lod::Mid, |g| {
                for b in 0..=n {
                    let t = -(face.len - 1.2) * 0.5 + b as f32 * step;
                    super::hk::wall_box(g, c, Brick, face, t, h * 0.5 + 0.45, 0.0, Vec3::new(0.5, h - 0.9, 0.16), mul(pt.brick, 0.93));
                }
            });
        }
        grime(g, c, rng, face, 0.0, h, 1.5 + 2.5 * c.wear);
    }
    // Ground floor: shopfronts or a loading dock with a steel canopy.
    let front = faces[0];
    if dress.shops && front.len > 6.0 {
        shop_row(g, c, rng, &front, fh, pt);
    } else if front.len > 6.0 {
        let dw = (front.len * 0.55).min(14.0);
        let room = c.reach_out(&front);
        if room > 2.4 {
            front.bx(g, f, Concrete, 0.0, 0.55, 1.0, Vec3::new(dw, 1.1, 2.0), mul(c.palette.concrete(0), 0.8));
            canopy(g, c, &front, 0.0, fh * 0.92, dw + 1.0, 2.2, pt.steel);
        }
        for k in [-1.0f32, 1.0] {
            roller_door(g, c, &front, k * dw * 0.25, (dw * 0.3).min(3.6), (fh * 0.8).min(3.2), mul(pt.steel, 1.6));
        }
    }
    if front.len > 4.0 {
        wall_lamp(g, c, &front, -front.len * 0.38, fh * 0.9);
        wall_lamp(g, c, &front, front.len * 0.38, fh * 0.9);
    }
    // A fire escape up the side that is not blank.
    let fe = if blank == Some(1) { 3 } else { 1 };
    if faces[fe].len > 7.0 && floors >= 3 && c.reach_out(&faces[fe]) > 1.6 {
        fire_escape(g, c, rng, &faces[fe], 0.0, fh, floors - 1, fh);
    }
    // The roof: a stair bulkhead, skylights, the water tower.
    let roof = offset_poly(&poly, 1.2);
    let (lo, hi) = bounds(&roof);
    if hi.x - lo.x > 5.0 && hi.y - lo.y > 5.0 {
        let bh = R::new(hi.x - 3.6, hi.x - 0.4, hi.y - 3.2, hi.y - 0.4);
        block(g, c, Brick, &bh, h, h + 2.8, mul(pt.brick, 0.9));
        pitched(g, c, &bh, h + 2.8, 0.8, true, 0.0, pt.roof, mul(pt.brick, 0.9));
        g.lod(Lod::Mid, |g| {
            let n = (((hi.x - lo.x) - 5.0) / 3.5) as i32;
            for k in 0..n.min(5) {
                let u = lo.x + 1.0 + k as f32 * 3.5;
                let sk = R::new(u, u + 2.2, lo.y + 0.8, lo.y + 2.6);
                if poly_inside_poly(&sk.poly(), &roof) {
                    pitched(g, c, &sk, h, 0.6, true, 0.0, mul(c.palette.glass_dark, 1.4), pt.steel);
                }
            }
        });
        if dress.water_tower && hi.x - lo.x > 7.0 {
            let r_t = ((hi.y - lo.y) * 0.16).clamp(1.3, 2.4);
            let p = Vec2::new(lo.x + r_t + 0.6, (lo.y + hi.y) * 0.5 + 1.0);
            if c.inside_local(p) {
                water_tower(g, c, rng, p, h, r_t);
            }
        }
    }
    h
}

/// A brick warehouse turned into lofts: four to ten storeys of steel
/// windows over a loading dock or shops, a hoist beam, a fire escape, a
/// ghost sign on its blank side and a water tower on the roof.
pub fn warehouse_loft(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.4, 4.2);
    let floors = (c.floors as i32).clamp(3, 10);
    let area = R::of(&c.poly);
    let Some(r) = fit(c, area.inset(0.3), 6.0) else {
        small_shed(g, c, rng, &pt);
        return;
    };
    let shops = c.land_use != nd_sim::LandUse::Residential && coin(rng, 0.6);
    let dress = LoftDress { shops, ghost: coin(rng, 0.7), water_tower: coin(rng, 0.85) };
    loft_block(g, c, rng, &r, floors, fh, &pt, dress);
}

/// A converted cotton mill: a long loft block, a stair tower a storey and
/// more above it with a pyramid cap and a clock, a works chimney kept
/// beside it, and a water tower.
pub fn mill_lofts(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.6, 4.2);
    let floors = (c.floors as i32).clamp(4, 12);
    let area = R::of(&c.poly);
    // The tower stands at one end of the front, the block beside it.
    let tw = 6.0f32.min(area.w() * 0.25);
    let left = coin(rng, 0.5);
    let (tu0, bu0, bu1) = if left { (area.u0 + 0.4, area.u0 + 0.4 + tw, area.u1 - 0.4) } else { (area.u1 - 0.4 - tw, area.u0 + 0.4, area.u1 - 0.4 - tw) };
    let depth = (area.d() * 0.62).clamp(8.0, 26.0);
    let Some(body) = fit(c, R::new(bu0, bu1, area.v0 + 0.4, area.v0 + 0.4 + depth), 6.0) else {
        warehouse_loft(g, c, rng);
        return;
    };
    let dress = LoftDress { shops: c.land_use != nd_sim::LandUse::Residential, ghost: false, water_tower: coin(rng, 0.7) };
    let h = loft_block(g, c, rng, &body, floors, fh, &pt, dress);
    // The stair tower.
    if tw >= 4.0 {
        let want = R::new(tu0, tu0 + tw, body.v0 + 1.0, body.v0 + 1.0 + tw);
        if let Some(t) = fit(c, want, 3.5) {
            let th = h + fh * 1.6;
            let tp = t.poly();
            brick_body(g, c, &tp, th, floors + 1, fh, 2, 0.0, &pt);
            cornice(g, c, &tp, th - 0.15, 0.4, 0.2, Stone, pt.trim);
            let cap = offset_poly(&tp, -0.3);
            let tip = vec![t.c(); cap.len()].into_iter().enumerate().map(|(i, p)| p + (cap[i] - p) * 0.04).collect::<Vec<_>>();
            loft(g, f, Panel, &cap, &tip, th, th + t.w() * 0.7, pt.roof);
            for face in t.faces() {
                // A tall stair window, then the clock face up top.
                if face.out.y < 0.5 {
                    steel_window(g, c, &face, 0.0, th * 0.5, (face.len * 0.25).min(1.2), th * 0.55, 2, 8, light(c, rng), &pt);
                    let cy = th - 1.6;
                    face.decal(g, f, Stone, 0.0, cy, FRAME, 1.9, 1.9, pt.trim);
                    let from = g.get(Glow).colors.len();
                    face.decal(g, f, Glow, 0.0, cy, PANE, 1.5, 1.5, [1.6, 1.4, 0.95]);
                    mark_glow(g, from, NIGHT_MARK);
                    g.lod(Lod::Near, |g| {
                        face.decal(g, f, Matte, 0.0, cy + 0.3, BAR, 0.08, 0.6, DARK);
                        face.decal(g, f, Matte, 0.18, cy, BAR + 0.002, 0.42, 0.08, DARK);
                    });
                }
            }
        }
    }
    // The old works chimney behind.
    let sp = Vec2::new(if left { body.u1 - 3.0 } else { body.u0 + 3.0 }, body.v1 + 3.2);
    if c.inside_local(sp) && distance_to_boundary(sp, &c.lot_poly) > 2.4 {
        works_stack(g, c, sp, 0.0, (h * 1.25 + 8.0).clamp(24.0, 72.0), 1.6, &pt);
    }
    // A cobbled yard and lamps behind.
    let yard_poly = clip_box(&c.poly, area.u0, area.u1, body.v1 + 0.2, area.v1);
    if yard_poly.len() >= 3 {
        ground(g, f, Paving, &yard_poly, 0.04, mul(c.palette.paving, 0.7));
        let (lo, hi) = bounds(&yard_poly);
        yard_lamp(g, c, Vec2::new(lo.x + 2.0, (lo.y + hi.y) * 0.5), 5.0);
        yard_lamp(g, c, Vec2::new(hi.x - 2.0, (lo.y + hi.y) * 0.5), 5.0);
    }
}

// ── shops and halls ──────────────────────────────────────────────────────────

/// A brick workshop: a one- or two-storey shed with big doors, a painted
/// name board lit after dark, a saw-tooth or slate roof, a yard behind
/// with crates, drums and the firm's van, and a sodium lamp over the door.
pub fn brick_workshop(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.6, 4.4);
    let floors = (c.floors as i32).clamp(1, 2);
    let h = floors as f32 * fh + 0.6;
    let area = R::of(&c.poly);
    let depth = (area.d() * 0.62).clamp(6.0, 22.0);
    let Some(r) = fit(c, R::new(area.u0 + 0.3, area.u1 - 0.3, area.v0 + 0.3, area.v0 + 0.3 + depth), 4.0) else {
        small_shed(g, c, rng, &pt);
        return;
    };
    let poly = r.poly();
    brick_body(g, c, &poly, h, floors, fh, 0, 0.0, &pt);
    let front = r.front();
    // Big doors and a pedestrian door; a flat above on two storeys.
    let doors = ((front.len - 2.0) / 4.5).floor().clamp(1.0, 3.0) as i32;
    for k in 0..doors {
        let t = -front.len * 0.5 + 1.0 + (k as f32 + 0.5) * (front.len * 0.72 - 1.0) / doors as f32;
        roller_door(g, c, &front, t, (front.len * 0.72 / doors as f32 - 1.2).clamp(2.0, 3.8), (fh * 0.8).min(3.4), mul(pick(rng, &DOORS), 1.4));
    }
    door(g, c, &front, front.len * 0.36, 0.9, 2.2, &pt, light(c, rng));
    if floors > 1 {
        window_rows(g, c, rng, &front, fh, 1, fh, 3.0, 0.55, 0.55, &pt);
    }
    // The name board, lit by a lamp after dark.
    let bw = (front.len * 0.6).min(10.0);
    sign_board(g, c, rng, &front, 0.0, h - 0.55, bw, 0.7);
    wall_lamp(g, c, &front, front.len * 0.36, 2.9);
    for face in [r.left(), r.right(), r.back()] {
        if face.len > 3.0 {
            window_rows(g, c, rng, &face, 0.0, floors, fh, 3.6, 0.55, 0.5, &pt);
            grime(g, c, rng, &face, 0.0, h, 2.0 + 2.0 * c.wear);
        }
    }
    // Roof: two or three saw teeth on a wide shed, else slate.
    if r.w() > 9.0 && coin(rng, 0.55) {
        super::brick_works::saw_teeth(g, c, rng, &r, h, (r.w() / 3.0).clamp(3.0, 7.0), &pt, 0.35);
    } else {
        pitched(g, c, &r, h, (r.d() * 0.22).clamp(1.0, 3.0), true, 0.25, pt.roof, pt.brick);
    }
    // The yard behind.
    let yard_r = R::new(r.u0, r.u1, r.v1 + 0.4, area.v1 - 0.3);
    if yard_r.d() > 4.0 {
        let yp = clip_box(&c.poly, yard_r.u0, yard_r.u1, yard_r.v0, yard_r.v1);
        ground(g, f, Dirt, &yp, 0.04, mix(c.palette.dirt, SOOT, 0.3));
        let van = Vec2::new(yard_r.c().x - yard_r.w() * 0.2, yard_r.c().y);
        if vehicle_fits(c, van, 0.0, Vehicle::Van) {
            let col = pick(rng, &DOORS);
            vehicle(g, f, c.palette, rng, van, 0.04, 0.0, Vehicle::Van, col);
        }
        g.lod(Lod::Mid, |g| {
            for k in 0..4 {
                let p = Vec2::new(yard_r.u1 - 1.2 - (k % 2) as f32 * 1.3, yard_r.v0 + 1.2 + (k / 2) as f32 * 1.3);
                if c.inside_local(p) {
                    if k % 2 == 0 {
                        g.part(Rust).cylinder(f.to_world(p.x, p.y), 0.3, 0.3, 0.04, 0.95, 8, rgba(mul(c.palette.rust, 0.9)));
                    } else {
                        f.bx(g, Wood, p.x, 0.45, p.y, Vec3::new(1.0, 0.8, 1.0), mul(c.palette.wood, 0.8));
                    }
                }
            }
        });
        yard_lamp(g, c, Vec2::new(yard_r.u0 + 1.0, yard_r.c().y), 5.0);
    }
}

/// The corner pub: a chamfered corner with the door in it, a ground floor
/// of glazed tiles and big warm windows under a lettered fascia, a pub
/// sign hung on a bracket, brick upstairs with chimney stacks and a slate
/// roof, and a beer garden behind under strings of bulbs.
pub fn corner_pub(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.4, 4.0);
    let floors = (c.floors as i32).clamp(2, 3);
    let h = floors as f32 * fh;
    let area = R::of(&c.poly);
    let depth = (area.d() * 0.6).clamp(7.0, 16.0);
    let wide = (area.w() - 0.6).min(18.0);
    let left = coin(rng, 0.5);
    let (u0, u1) = if left { (area.u0 + 0.3, area.u0 + 0.3 + wide) } else { (area.u1 - 0.3 - wide, area.u1 - 0.3) };
    let Some(r) = fit(c, R::new(u0, u1, area.v0 + 0.3, area.v0 + 0.3 + depth), 5.0) else {
        small_shed(g, c, rng, &pt);
        return;
    };
    // The footprint with its street corner cut off.
    let ch = (r.w().min(r.d()) * 0.22).clamp(1.2, 2.6);
    let poly = if left {
        vec![Vec2::new(r.u0 + ch, r.v0), Vec2::new(r.u1, r.v0), Vec2::new(r.u1, r.v1), Vec2::new(r.u0, r.v1), Vec2::new(r.u0, r.v0 + ch)]
    } else {
        vec![Vec2::new(r.u0, r.v0), Vec2::new(r.u1 - ch, r.v0), Vec2::new(r.u1, r.v0 + ch), Vec2::new(r.u1, r.v1), Vec2::new(r.u0, r.v1)]
    };
    let tiles = pick(rng, &[[0.10, 0.22, 0.16], [0.30, 0.07, 0.06], [0.10, 0.12, 0.18], [0.36, 0.26, 0.12]]);
    // Tiled ground floor, a fascia, brick above.
    extrude(g, f, Panel, &poly, 0.0, fh, tiles);
    extrude(g, f, Brick, &poly, fh, h, pt.brick);
    cornice(g, c, &poly, fh + 0.05, 0.7, 0.18, Panel, mul(tiles, 0.8));
    cornice(g, c, &poly, h - 0.3, 0.5, 0.25, Stone, pt.trim);
    // A hipped slate roof over it all.
    let axis = if r.w() >= r.d() { Vec2::Y } else { Vec2::X };
    ridge_roof(g, c, &poly, h, (r.w().min(r.d()) * 0.3).clamp(1.5, 3.5), axis, 0.15, 0.3, Panel, pt.roof);
    let faces = c.faces(&poly);
    let corner = faces.iter().copied().min_by(|a, b| a.len.total_cmp(&b.len)).unwrap();
    for face in &faces {
        let is_corner = face.len == corner.len && face.mid() == corner.mid();
        if is_corner {
            // The door in the cut corner, a lamp over it.
            door(g, c, face, 0.0, (face.len * 0.55).min(1.4), 2.4, &Paint { door: mul(tiles, 0.7), ..pt }, Pane::Lit(WARM[0]));
            wall_lamp(g, c, face, 0.0, 3.1);
            continue;
        }
        if face.len < 2.0 {
            continue;
        }
        let street = face.out.y < -0.5 || face.out.x.abs() > 0.5 && face.mid().y < r.c().y;
        // Big pub windows on the street sides, warm whenever they are lit.
        let n = ((face.len - 1.0) / 2.8).floor() as i32;
        if street && n >= 1 {
            let step = (face.len - 1.0) / n as f32;
            for k in 0..n {
                let t = -(face.len - 1.0) * 0.5 + (k as f32 + 0.5) * step;
                let pane = if coin(rng, (c.lit_chance() + 0.3).min(1.0)) { Pane::Lit(mul(WARM[(k as usize) % 2], 1.1)) } else { Pane::Dark };
                steel_window(g, c, face, t, fh * 0.5, step * 0.7, fh * 0.55, 2, 3, pane, &Paint { steel: mul(tiles, 0.6), ..pt });
            }
            // The lettered fascia: gilt letters on the tile band.
            let from = g.get(Glow).colors.len();
            let lw = face.len * 0.6;
            let letters = ((lw / 0.7) as i32).clamp(3, 14);
            for i in 0..letters {
                if coin(rng, 0.1) {
                    continue;
                }
                let lt = -lw * 0.5 + (i as f32 + 0.5) * lw / letters as f32;
                face.decal(g, f, Glow, lt, fh + 0.05, 0.22, lw / letters as f32 * 0.62, 0.38, [1.5, 1.15, 0.45]);
            }
            mark_glow(g, from, NEON_MARK);
        }
        if floors > 1 && face.len > 2.6 {
            window_rows(g, c, rng, face, fh, floors - 1, fh, 2.8, 0.5, 0.55, &pt);
        }
        grime(g, c, rng, face, fh, h, 1.0 + 2.0 * c.wear);
    }
    // The pub sign on a bracket off the corner: a board between two lamps.
    let fr = r.front();
    let st = if left { -fr.len * 0.5 + ch + 0.9 } else { fr.len * 0.5 - ch - 0.9 };
    if c.reach_out(&fr) > 1.2 {
        g.lod(Lod::Mid, |g| fr.bx(g, f, Matte, st, fh + 1.6, 0.6, Vec3::new(0.08, 0.08, 1.2), CHARCOAL));
        let board = pick(rng, &[[0.30, 0.08, 0.06], [0.08, 0.18, 0.12], [0.08, 0.08, 0.10]]);
        // At right angles to the face: two faces of a thin board.
        let p = fr.at(st, 1.0);
        let yaw = fr.yaw() + std::f32::consts::FRAC_PI_2;
        f.bx_yaw(g, Wood, p.x, fh + 0.95, p.y, Vec3::new(1.0, 1.1, 0.08), board, yaw);
        let from = g.get(Glow).colors.len();
        f.bx_yaw(g, Glow, p.x, fh + 0.95, p.y, Vec3::new(0.7, 0.6, 0.1), mul(pick(rng, &WARM), 1.2), yaw);
        mark_glow(g, from, NEON_MARK);
    }
    // Chimneys on the party walls.
    let ry = h + (r.w().min(r.d()) * 0.3).clamp(1.5, 3.5);
    for u in [r.u0 + 1.6, r.u1 - 1.6] {
        chimney_stack(g, c, Vec2::new(u, r.c().y), h - 0.5, ry + 0.9, 1.2, &pt);
    }
    // The beer garden behind under strings of warm bulbs, a barrel or two.
    let back = R::new(r.u0, r.u1, r.v1 + 0.4, (area.v1 - 0.3).min(r.v1 + 12.0));
    if back.d() > 4.0 {
        let gp = clip_box(&c.poly, back.u0, back.u1, back.v0, back.v1);
        ground(g, f, Paving, &gp, 0.04, mul(c.palette.paving, 0.8));
        // A low brick wall round the garden.
        for face in c.faces(&gp) {
            if face.len > 1.0 && face.out.y > -0.5 {
                face.bx(g, f, Brick, 0.0, 0.6, -0.15, Vec3::new(face.len, 1.2, 0.3), pt.brick);
            }
        }
        let n = ((back.w() - 2.0) / 3.0).floor() as i32;
        for k in 0..n.min(5) {
            let p = Vec2::new(back.u0 + 1.8 + k as f32 * 3.0, back.v0 + (back.d() * 0.5).min(4.0));
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.6 {
                table(g, f, p, 0.04, mul(c.palette.wood, 0.75), None);
            }
        }
        let (a, b) = (Vec2::new(back.u0 + 0.8, back.v0 + 0.6), Vec2::new(back.u1 - 0.8, back.v0 + (back.d() - 0.8).min(7.0)));
        if c.spans_lot(a, b) {
            string_lights(g, f, Vec3::new(a.x, fh * 0.8, a.y), Vec3::new(b.x, 2.6, b.y), ((back.w() / 1.2) as i32).clamp(4, 14), WARM[1]);
        }
        for k in 0..2 {
            let p = Vec2::new(r.u1 - 1.0 - k as f32 * 0.8, back.v0 + 0.6);
            if c.inside_local(p) {
                g.part(Wood).cylinder(f.to_world(p.x, p.y), 0.32, 0.36, 0.04, 0.95, 8, rgba(mul(c.palette.wood, 0.6)));
            }
        }
    }
}

/// The union hall: a tall hall with its gable to the street on a stone
/// plinth with steps, a stone pediment line and a round window in the
/// gable, tall windows between brick pilasters, red banners hung down the
/// front, a carved name band, globe lamps at the door and a flag on the
/// roof.
pub fn union_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pt = paint(c, rng);
    let fh = c.fh().clamp(3.6, 4.4);
    let floors = (c.floors as i32).clamp(2, 4);
    let h = floors as f32 * fh + 1.0;
    let area = R::of(&c.poly);
    let w = (area.w() - 1.0).clamp(4.0, 24.0);
    let d = (area.d() * 0.75).clamp(8.0, 34.0);
    let Some(r) = fit(c, R::new(area.c().x - w * 0.5, area.c().x + w * 0.5, area.v0 + 1.6, area.v0 + 1.6 + d), 5.0) else {
        small_shed(g, c, rng, &pt);
        return;
    };
    let poly = r.poly();
    brick_body(g, c, &poly, h, floors, fh, 0, 0.0, &pt);
    plinth(g, c, &poly, 2, 0.25, 0.35, Stone, pt.trim);
    cornice(g, c, &poly, h - 0.15, 0.4, 0.22, Stone, pt.trim);
    let rise = (r.w() * 0.3).clamp(1.5, 5.0);
    pitched(g, c, &r, h, rise, false, 0.35, pt.roof, pt.brick);
    let front = r.front();
    // The round window in the gable and the stone line under it.
    let gy = h + rise * 0.38;
    let gr = (rise * 0.28).min(1.1);
    if gr > 0.4 {
        face_disc(g, c, &front, 0.0, gy, gr + 0.18, FRAME, pt.trim);
        let pane = if coin(rng, c.lit_chance()) { Pane::Lit(WARM[1]) } else { Pane::Dark };
        let col = match pane {
            Pane::Lit(col) => col,
            _ => c.palette.glass_dark,
        };
        if matches!(pane, Pane::Lit(_)) {
            window_light(g, c, |g| face_disc_s(g, c, &front, 0.0, gy, gr, PANE, Glow, col));
        } else {
            face_disc_s(g, c, &front, 0.0, gy, gr, PANE, Glass, col);
        }
    }
    // Tall windows between pilasters, the door in the middle.
    let dw = (front.len * 0.2).clamp(1.6, 3.0);
    for (i, face) in r.faces().iter().enumerate() {
        if face.len < 3.0 {
            continue;
        }
        let n = ((face.len - 1.0) / 3.4).floor().max(1.0) as i32;
        let step = (face.len - 1.0) / n as f32;
        for k in 0..n {
            let t = -(face.len - 1.0) * 0.5 + (k as f32 + 0.5) * step;
            if i == 0 && t.abs() < dw * 0.5 + (step * 0.25).min(0.9) + 0.4 {
                continue;
            }
            let wh = (h - 2.4).min(fh * 1.6);
            steel_window(g, c, face, t, 1.0 + wh * 0.5 + 0.4, (step * 0.5).min(1.8), wh, 2, 6, light(c, rng), &pt);
            if i == 0 && k + 1 < n {
                g.lod(Lod::Mid, |g| super::hk::wall_box(g, c, Brick, face, t + step * 0.5, h * 0.5, 0.0, Vec3::new(0.6, h - 0.4, 0.2), mul(pt.brick, 0.9)));
            }
        }
        if i != 0 {
            grime(g, c, rng, face, 0.0, h, 1.5 + 2.0 * c.wear);
        }
    }
    // The doors under a stone hood, globe lamps either side.
    door(g, c, &front, 0.0, dw, 3.0, &Paint { door: DOORS[1], ..pt }, light(c, rng));
    front.bx(g, f, Stone, 0.0, 3.55, 0.4, Vec3::new(dw + 1.0, 0.25, 0.8), pt.trim);
    for s in [-1.0f32, 1.0] {
        let from = g.get(Glow).colors.len();
        front.bx(g, f, Glow, s * (dw * 0.5 + 0.8), 2.9, 0.35, Vec3::new(0.36, 0.36, 0.36), [1.7, 1.45, 0.9]);
        mark_glow(g, from, NIGHT_MARK);
    }
    // The carved name band over the door: stone, letters cut dark.
    let bw = (front.len * 0.7).min(14.0);
    front.decal(g, f, Stone, 0.0, h - 1.3, BAND, bw, 0.9, pt.trim);
    let letters = ((bw / 0.75) as i32).clamp(4, 16);
    for i in 0..letters {
        let lt = -bw * 0.45 + (i as f32 + 0.5) * bw * 0.9 / letters as f32;
        front.decal(g, f, Matte, lt, h - 1.3, BAND + 0.002, bw * 0.9 / letters as f32 * 0.6, 0.5, mul(pt.trim, 0.35));
    }
    // Red banners down the front between the windows.
    let banner = pick(rng, &[[0.62, 0.06, 0.05], [0.55, 0.10, 0.06], [0.10, 0.18, 0.42]]);
    let n = ((front.len - 1.0) / 3.4).floor().max(1.0) as i32;
    if n >= 3 {
        let step = (front.len - 1.0) / n as f32;
        for s in [-1.0f32, 1.0] {
            let t = s * step;
            front.decal(g, f, Tarp, t, h - 3.4, 0.09, 0.9, (h - 5.0).max(2.0).min(5.0), banner);
            g.lod(Lod::Mid, |g| front.bx(g, f, Matte, t, h - 1.95, 0.12, Vec3::new(1.2, 0.06, 0.12), CHARCOAL));
        }
    }
    // A flag on the ridge.
    let fp = Vec2::new(r.c().x, r.v0 + 1.0);
    flagpole(g, f, fp, h + rise * 0.8 + 4.0, banner, false);
    yard_lamp(g, c, Vec2::new(r.u0 - 0.2, area.v0 + 1.0), 5.5);
}

/// A disc on a face (16 sides) `d` proud of it in Stone.
fn face_disc(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, r: f32, d: f32, col: [f32; 3]) {
    face_disc_s(g, c, face, t, y, r, d, Stone, col);
}

/// A disc on a face in surface `s`.
#[allow(clippy::too_many_arguments)]
fn face_disc_s(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, r: f32, d: f32, s: crate::surface::Surface, col: [f32; 3]) {
    let f = &c.frame;
    let centre = f.at2(face.at(t, d), y);
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    let ring: Vec<Vec3> = (0..16).map(|i| {
        let a = i as f32 / 16.0 * std::f32::consts::TAU;
        centre + along * a.cos() * r + Vec3::Y * a.sin() * r
    }).collect();
    g.part(s).cap(&ring, out, rgba(col));
}

// ── the loft estate ──────────────────────────────────────────────────────────

/// [r4-brick] A Foundry Row family's big house (two plots) or a
/// billionaire's estate (up to four): a brick warehouse made over, behind a
/// brick wall with iron gates between lamp-lit piers. Huge steel windows,
/// a glass penthouse on the roof with a planted terrace, the water tower
/// and the works chimney kept, a cobbled forecourt with the cars, a pool
/// in the old loading yard, a garden of birches and a string of lights.
/// On the bigger lot a second warehouse wing becomes the garage and a
/// gallery, and an old gantry stands over the pool.
pub fn loft_estate(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let pt = Paint { brick: mul(pick(rng, &BRICKS[..4]), 1.05), ..paint(c, rng) };
    let fh = c.fh().clamp(3.8, 4.4);
    let area = R::of(&c.poly);
    let big = area.w() * area.d() > 2600.0;
    let floors = (c.floors as i32).clamp(3, if big { 5 } else { 4 });
    let yard_poly = yard(g, c, 0.2, Paving, mul(pal.paving, 0.62));
    // The house: the warehouse in the back half.
    let w = (area.w() * if big { 0.5 } else { 0.62 }).clamp(10.0, 40.0);
    let d = (area.d() * 0.36).clamp(9.0, 22.0);
    let hu = if big { area.u0 + 3.0 + w * 0.5 } else { area.c().x };
    let hv = area.v1 - 3.0 - d * 0.5;
    let Some(house) = fit(c, R::new(hu - w * 0.5, hu + w * 0.5, hv - d * 0.5, hv + d * 0.5), 6.0) else {
        warehouse_loft(g, c, rng);
        return;
    };
    let dress = LoftDress { shops: false, ghost: coin(rng, 0.5), water_tower: false };
    let h = loft_block(g, c, rng, &house, floors, fh, &pt, dress);
    // The glass penthouse set back on the roof, its terrace planted.
    let ph = house.inset(2.2);
    if ph.w() > 4.0 && ph.d() > 4.0 {
        let pr = R::new(ph.u0, ph.u0 + ph.w() * 0.6, ph.v0 + ph.d() * 0.25, ph.v1);
        extrude(g, f, Glass, &pr.poly(), h, h + 3.4, mul(pal.glass_dark, 1.3));
        extrude(g, f, Matte, &pr.inset(-0.25).poly(), h + 3.4, h + 3.65, CHARCOAL);
        for face in pr.faces() {
            let n = (face.len / 2.2) as i32;
            for k in 0..=n {
                let t = -face.len * 0.5 + k as f32 * face.len / n.max(1) as f32;
                face.bx(g, f, Matte, t.clamp(-face.len * 0.5 + 0.06, face.len * 0.5 - 0.06), h + 1.7, 0.04, Vec3::new(0.1, 3.4, 0.1), CHARCOAL);
            }
            if face.out.y < 0.5 {
                let lit = coin(rng, c.lit_chance());
                if lit {
                    window_light(g, c, |g| face.decal(g, f, Glow, 0.0, h + 1.6, 0.03, face.len - 0.6, 2.6, mul(WARM[3], 0.9)));
                }
            }
        }
        // Planters along the terrace edge, a rail round the roof.
        g.lod(Lod::Mid, |g| {
            let tr = R::new(pr.u1 + 0.6, ph.u1, ph.v0, ph.v1);
            if tr.w() > 1.5 {
                for k in 0..((tr.d() / 2.4) as i32).max(1) {
                    let p = Vec2::new(tr.u1 - 0.5, tr.v0 + 1.2 + k as f32 * 2.4);
                    f.bx(g, Concrete, p.x, h + 0.35, p.y, Vec3::new(0.8, 0.7, 1.8), mul(pal.concrete(0), 0.7));
                    tree(g, f, p, 1.8, 0.7, pal.bark, mul(pal.foliage, 1.1));
                }
            }
        });
    }
    // The water tower and the works chimney, kept.
    let wt = Vec2::new(house.u1 - 2.6, house.v1 - 2.6);
    if house.w() > 9.0 {
        water_tower(g, c, rng, wt, h, 1.8);
    }
    let sp = Vec2::new(house.u0 + 2.0, house.v1 - 2.0);
    if house.w() > 12.0 {
        works_stack(g, c, sp, h, (h * 0.9).clamp(12.0, 26.0), 1.2, &pt);
    }
    // The second wing on a big lot: a low warehouse, garage doors.
    let mut wing_r: Option<R> = None;
    if big {
        let ww = (area.u1 - house.u1 - 6.0).min(26.0);
        if ww > 8.0 {
            let want = R::new(house.u1 + 4.0, house.u1 + 4.0 + ww, house.v1 - (d * 0.75).min(16.0), house.v1);
            if let Some(wr) = fit(c, want, 6.0) {
                let wh = 2.0 * fh;
                brick_body(g, c, &wr.poly(), wh, 2, fh, 0, 0.6, &pt);
                let wf = wr.front();
                let n = ((wf.len - 1.0) / 3.6).floor().clamp(1.0, 4.0) as i32;
                for k in 0..n {
                    let t = -(wf.len - 1.0) * 0.5 + (k as f32 + 0.5) * (wf.len - 1.0) / n as f32;
                    roller_door(g, c, &wf, t, 2.8, 2.6, mul(pt.steel, 1.8));
                }
                window_rows(g, c, rng, &wf, fh, 1, fh, 3.0, 0.6, 0.6, &pt);
                for face in [wr.left(), wr.right(), wr.back()] {
                    window_rows(g, c, rng, &face, 0.0, 2, fh, 3.6, 0.6, 0.55, &pt);
                }
                super::brick_works::saw_teeth(g, c, rng, &wr, wh + 0.6, 4.0, &pt, 0.8);
                wing_r = Some(wr);
            }
        }
    }
    // The pool in the old loading yard in front of the house, a gantry over it.
    let pool_v = house.v0 - 5.0;
    let pool_w = (house.w() * 0.55).min(18.0);
    let pool_c = Vec2::new(house.c().x, pool_v);
    if poly_inside(c, &rect(pool_c.x, pool_c.y, pool_w * 0.5 + 1.2, 2.8)) {
        ground(g, f, Stone, &rect(pool_c.x, pool_c.y, pool_w * 0.5 + 1.2, 2.8), 0.06, mul(pt.trim, 1.15));
        pool(g, c, pool_c.x, pool_c.y, pool_w, 3.4);
        if big {
            let gy = 6.5;
            g.lod(Lod::Mid, |g| {
                let steel = mul(pt.steel, 2.0);
                for s in [-1.0f32, 1.0] {
                    let u = pool_c.x + s * (pool_w * 0.5 + 0.8);
                    for dv in [-2.4f32, 2.4] {
                        f.bx(g, Matte, u, gy * 0.5, pool_v + dv, Vec3::new(0.3, gy, 0.3), steel);
                    }
                    f.bx(g, Matte, u, gy, pool_v, Vec3::new(0.4, 0.5, 5.2), steel);
                }
                f.bx(g, Metal, pool_c.x, gy + 0.5, pool_v, Vec3::new(pool_w + 2.4, 0.7, 0.8), steel);
            });
        }
        for k in 0..((pool_w / 2.0) as i32).min(6) {
            let p = Vec2::new(pool_c.x - pool_w * 0.5 + 1.0 + k as f32 * 2.0, pool_v + 2.3);
            g.lod(Lod::Mid, |g| f.bx(g, Wood, p.x, 0.3, p.y, Vec3::new(0.7, 0.24, 1.8), mul(pal.wood, 0.9)));
        }
    }
    // The forecourt: a drive, the cars, birches in the lawn.
    let drive_v = area.v0 + 6.0;
    for k in 0..if big { 3 } else { 2 } {
        let p = Vec2::new(area.u0 + 6.0 + k as f32 * 4.0, drive_v);
        let yaw = std::f32::consts::FRAC_PI_2;
        if vehicle_fits(c, p, yaw, Vehicle::Car) {
            let col = pick(rng, &[[0.06, 0.06, 0.07], [0.42, 0.08, 0.06], [0.20, 0.30, 0.26], [0.70, 0.70, 0.68]]);
            vehicle(g, f, pal, rng, p, 0.06, yaw, Vehicle::Car, col);
        }
    }
    let lawn = R::new(area.c().x + 2.0, area.u1 - 2.0, area.v0 + 2.0, pool_v - 3.6);
    if lawn.w() > 4.0 && lawn.d() > 3.0 {
        let lp = clip_box(&yard_poly, lawn.u0, lawn.u1, lawn.v0, lawn.v1);
        if lp.len() >= 3 {
            ground(g, f, Foliage, &lp, 0.08, mix(pal.foliage, [0.30, 0.42, 0.22], 0.5));
            for _ in 0..rng.random_range(3..7) {
                if let Some(p) = random_in_poly(rng, &lp, 1.5) {
                    tree_in(g, f, pal, p, rng.random_range(5.0..8.0), rng.random_range(1.2..2.0), pal.foliage);
                }
            }
        }
    }
    let (a, b) = (Vec2::new(house.u0 + 1.0, house.v0 - 1.0), Vec2::new(house.u1 - 1.0, house.v0 - 1.0));
    if c.spans_lot(a, b) {
        string_lights(g, f, Vec3::new(a.x, 3.2, a.y), Vec3::new(b.x, 3.2, b.y), ((house.w() / 1.4) as i32).clamp(4, 18), WARM[1]);
    }
    // The wall round it, piers with lamps at the gate.
    let fi = c.front_face_index(&yard_poly);
    for (i, face) in c.faces(&yard_poly).iter().enumerate() {
        if face.len < 1.0 {
            continue;
        }
        if Some(i) == fi {
            // Two runs of wall either side of the gate.
            let gw = 6.0f32.min(face.len * 0.4);
            for s in [-1.0f32, 1.0] {
                let len = face.len * 0.5 - gw * 0.5;
                if len > 0.5 {
                    face.bx(g, f, Brick, s * (gw * 0.5 + len * 0.5), 1.1, -0.2, Vec3::new(len, 2.2, 0.4), pt.brick);
                }
                face.bx(g, f, Brick, s * gw * 0.5, 1.5, -0.3, Vec3::new(0.7, 3.0, 0.7), mul(pt.brick, 0.85));
                let from = g.get(Glow).colors.len();
                face.bx(g, f, Glow, s * gw * 0.5, 3.25, -0.3, Vec3::new(0.34, 0.4, 0.34), SODIUM);
                mark_glow(g, from, NIGHT_MARK);
            }
            gate(g, c, face.at(-gw * 0.5 + 0.4, -0.2), face.at(gw * 0.5 - 0.4, -0.2), 2.4);
        } else {
            face.bx(g, f, Brick, 0.0, 1.1, -0.2, Vec3::new(face.len, 2.2, 0.4), pt.brick);
        }
    }
    if let Some(wr) = wing_r {
        yard_lamp(g, c, Vec2::new(wr.c().x, wr.v0 - 2.0), 5.5);
    }
    yard_lamp(g, c, Vec2::new(area.u0 + 2.5, area.v0 + 2.5), 5.5);
}
