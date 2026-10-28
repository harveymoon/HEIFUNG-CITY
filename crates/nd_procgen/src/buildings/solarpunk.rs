//! [r4-solarpunk] Solarpunk Green, the Eco Village's look: a growers' and
//! engineers' co-op that has lived here a while. Timber-and-glass
//! terraces under planted roofs, solar-shingle townhouses, vine-covered
//! towers with balcony gardens; a glass market hall with a farm on the
//! roof behind it, co-op stores and repair cafés; for its small industry
//! vertical farms, greenhouse rows and a cargo-bike depot under a solar
//! canopy, with wind trees; and for the families and billionaires who buy
//! in, a planted solar villa with a rooftop garden.
//!
//! Stylised and gritty, not a brochure: timber silvers and darkens with
//! wear, copper has gone green, glass is dirty, roofs are overgrown,
//! panels are patched with odd ones of another make (and now and then a
//! tarp), cables run down the walls to battery boxes, rain tanks and water
//! butts stand at every corner. After dark the light is warm and low: lit
//! windows, bulbs strung over yards, the undersides of the solar canopies
//! glowing amber and the vertical farms' grow lights magenta (base glow
//! marked `NIGHT_MARK`, lit only after dark).
//!
//! Everything comes from the building's seed: the same from every camera.
//! Planters, tanks' lids, wind trees' branches and blades, frames, rails
//! and crates are mid detail; bikes, cables, seams and glazing bars near.

use super::harbour::{bulbs, gabled};
use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::depth::{FRAME, PANE};
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Timber cladding: silvered larch, honey cedar, dark cedar, oiled pine.
const TIMBER: [[f32; 3]; 4] = [[0.56, 0.52, 0.46], [0.62, 0.47, 0.31], [0.42, 0.32, 0.24], [0.54, 0.42, 0.30]];
/// What sun and rain turn timber towards.
const SILVER: [f32; 3] = [0.50, 0.50, 0.48];
/// Charred boards (shou sugi ban).
const CHARRED: [f32; 3] = [0.13, 0.12, 0.11];
/// Lime and clay renders: cream, oat, ochre clay, sage.
const RENDERS: [[f32; 3]; 4] = [[0.76, 0.70, 0.58], [0.68, 0.64, 0.54], [0.72, 0.56, 0.38], [0.62, 0.66, 0.56]];
/// Copper gone green, and one not yet.
const PATINA: [[f32; 3]; 3] = [[0.36, 0.56, 0.48], [0.30, 0.48, 0.42], [0.46, 0.58, 0.50]];
const COPPER: [f32; 3] = [0.58, 0.36, 0.22];
/// Photovoltaic glass, and the odd panel of another make that patched it.
const PV: [f32; 3] = [0.08, 0.11, 0.18];
const PV_PATCH: [[f32; 3]; 3] = [[0.14, 0.15, 0.17], [0.10, 0.16, 0.22], [0.18, 0.18, 0.20]];
/// Window frames and doors: bottle green, black, oxide red, oak, teal.
const FRAMES: [[f32; 3]; 5] = [[0.16, 0.26, 0.18], [0.10, 0.10, 0.11], [0.42, 0.20, 0.13], [0.46, 0.36, 0.24], [0.12, 0.32, 0.32]];
/// Rain tanks and water butts: green, black, galvanised, slate blue.
const TANKS: [[f32; 3]; 4] = [[0.18, 0.28, 0.18], [0.11, 0.11, 0.12], [0.52, 0.54, 0.52], [0.28, 0.34, 0.40]];
/// Bikes and crates: the bright things people own.
const BRIGHT: [[f32; 3]; 6] = [[0.80, 0.22, 0.14], [0.14, 0.40, 0.62], [0.90, 0.70, 0.16], [0.20, 0.52, 0.30], [0.88, 0.86, 0.80], [0.62, 0.30, 0.50]];
/// Flowers in the planters.
const BLOOMS: [[f32; 3]; 4] = [[0.84, 0.30, 0.22], [0.92, 0.74, 0.22], [0.62, 0.38, 0.70], [0.90, 0.88, 0.80]];
/// The underside of a solar canopy after dark.
const CANOPY_GLOW: [f32; 3] = [1.9, 1.32, 0.62];
/// A room lit inside, warm.
const INSIDE: [f32; 3] = [1.5, 1.02, 0.56];
/// Grow lights.
const GROW: [f32; 3] = [1.7, 0.42, 1.45];
/// A greenhouse's lamps: warm sodium.
const SODIUM: [f32; 3] = [1.9, 1.05, 0.38];

// Facade depths of the look (see `crate::depth`): the two greens of the
// creepers under grime (0.02) and frames (0.03); doors between frames and
// panes. Over a solar slope: patches, then seams.
const VINE_A: f32 = 0.011;
const VINE_B: f32 = 0.015;
const DOOR_D: f32 = 0.04;
const PATCH_D: f32 = 0.012;
const SEAM_D: f32 = 0.022;
// Ground layers over the yard (0.03), each its own height.
const MEADOW_Y: f32 = 0.038;
const PATH_Y: f32 = 0.046;
const GRAVEL_Y: f32 = 0.053;

/// Lot-local (u, y, v).
fn p3(u: f32, y: f32, v: f32) -> Vec3 {
    Vec3::new(u, y, v)
}

// ── materials ────────────────────────────────────────────────────────────────

/// Timber, silvered and darkened with wear.
fn timber(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    let base = TIMBER[rng.random_range(0..TIMBER.len())];
    mul(mix(base, SILVER, 0.12 + 0.45 * c.wear), 1.0 - 0.2 * c.wear)
}

fn render(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(RENDERS[rng.random_range(0..RENDERS.len())], 1.0 - 0.25 * c.wear)
}

/// Copper: green unless the building is new.
fn copper(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    if c.wear < 0.12 && coin(rng, 0.5) {
        COPPER
    } else {
        mul(PATINA[rng.random_range(0..PATINA.len())], 1.0 - 0.2 * c.wear)
    }
}

fn frame_col(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(FRAMES[rng.random_range(0..FRAMES.len())], 1.0 - 0.25 * c.wear)
}

/// Leaves: the palette's, drier on worn and cheap ground; `k` shades it.
fn leaf(c: &Ctx, k: f32) -> [f32; 3] {
    let dry = 0.1 + 0.4 * c.wear + if c.tier == nd_sim::Tier::Low { 0.15 } else { 0.0 };
    mul(mix(c.palette.foliage, c.palette.foliage_dry, dry.min(0.8)), k)
}

/// Glass with a film of dirt on it.
fn dirty_glass(c: &Ctx) -> [f32; 3] {
    mix(c.palette.glass_dark, [0.30, 0.33, 0.28], 0.2 + 0.35 * c.wear)
}

/// Whether a prop of radius `r` at lot-local `p` stands on the lot.
fn room(c: &Ctx, p: Vec2, r: f32) -> bool {
    c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > r
}

// ── shared pieces ─────────────────────────────────────────────────────────────

/// A window: lit or dark glass, its frame and sill (mid), a glazing bar
/// (near), and now and then a window box of herbs and flowers (mid).
#[allow(clippy::too_many_arguments)]
fn window(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32, fc: [f32; 3], boxes: f32) {
    let f = &c.frame;
    if coin(rng, c.lit_chance()) {
        let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.5..0.9));
        face.glaze(g, c, t, y, PANE, w, h, Pane::Lit(col));
    } else {
        face.glaze(g, c, t, y, PANE, w, h, Pane::Dark);
    }
    g.lod(Lod::Mid, |g| face.window_frame(g, c, t, y, w, h, 0.16, fc));
    g.lod(Lod::Near, |g| face.decal(g, f, Matte, t, y, PANE + 0.008, 0.05, h, fc));
    if coin(rng, boxes) {
        window_box(g, c, rng, face, t, y - h * 0.5 - 0.16, w + 0.1);
    }
}

/// A trough of herbs and flowers under a window or along a rail, leaves
/// trailing down the wall under it.
fn window_box(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32) {
    let f = &c.frame;
    let wood = mul(c.palette.wood, 0.7 - 0.2 * c.wear);
    let green = leaf(c, 0.95);
    let bloom = BLOOMS[rng.random_range(0..BLOOMS.len())];
    // Trailing leaves down the wall, never into the ground.
    let trail = rng.random_range(0.3..1.1f32).min(y - 0.5);
    g.lod(Lod::Mid, |g| {
        face.bx(g, f, Wood, t, y, 0.21, Vec3::new(w, 0.24, 0.3), wood);
        face.bx(g, f, Foliage, t, y + 0.2, 0.245, Vec3::new(w - 0.06, 0.26, 0.31), green);
    });
    g.lod(Lod::Near, |g| {
        face.bx(g, f, Foliage, t - w * 0.2, y + 0.36, 0.24, Vec3::new(w * 0.3, 0.12, 0.2), bloom);
        if trail > 0.2 {
            face.decal(g, f, Foliage, t + w * 0.15, y - 0.12 - trail * 0.5, 0.38, w * 0.5, trail, mul(green, 0.85));
        }
    });
}

/// Creepers down a face from `y1`: strands of two greens (one at each
/// creeper depth), `cover` the share of the face they hang over. The first
/// third go in the base so a planted wall reads green from across the city.
fn creepers(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32, cover: f32) {
    if face.len < 1.5 || y1 - y0 < 0.8 || cover <= 0.0 {
        return;
    }
    let f = &c.frame;
    let n = ((face.len / 1.5) * cover).ceil() as i32;
    let (a, b) = (leaf(c, 0.78), leaf(c, 0.62));
    for i in 0..n {
        let w = rng.random_range(0.5..1.4f32).min(face.len - 0.3);
        let t = rng.random_range(-(face.len - w) * 0.5..=(face.len - w) * 0.5);
        let len = (y1 - y0) * rng.random_range(0.25..0.95f32);
        let (d, col) = if i % 2 == 0 { (VINE_A, a) } else { (VINE_B, b) };
        let y = y1 - len * 0.5;
        if i * 3 < n {
            face.decal(g, f, Foliage, t, y, d, w, len, col);
        } else {
            g.lod(Lod::Mid, |g| face.decal(g, f, Foliage, t, y, d, w, len, col));
        }
    }
}

/// A rain tank at lot-local `p`: a ribbed drum on a plinth, its lid and a
/// tap (mid), a downpipe up to `pipe_to` (near).
fn rain_tank(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32, pipe_to: Option<Vec3>) {
    if !room(c, p, r + 0.15) {
        return;
    }
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    let col = mul(TANKS[rng.random_range(0..TANKS.len())], 1.0 - 0.25 * c.wear);
    let s = if col[0] > 0.4 && c.wear > 0.5 { Rust } else { Metal };
    g.part(s).cylinder_capped(w, r, r, 0.0, h, 10, false, true, rgba(col));
    g.lod(Lod::Mid, |g| {
        g.part(Matte).cylinder(w, r * 0.45, r * 0.4, h, h + 0.12, 8, rgba(mul(col, 0.7)));
        f.bx(g, Matte, p.x - r - 0.06, 0.35, p.y, Vec3::new(0.12, 0.08, 0.08), DARK);
    });
    if let Some(top) = pipe_to {
        g.lod(Lod::Near, |g| {
            let a = f.at(p.x, h + 0.1, p.y);
            let b = Vec3::new(a.x, top.y, a.z);
            g.part(Matte).beam(a, b, 0.07, rgba(CHARCOAL));
            g.part(Matte).beam(b, top, 0.07, rgba(CHARCOAL));
        });
    }
}

/// A wind tree at lot-local `p`: a steel trunk branching near the top, a
/// little vertical-axis turbine on every branch end (trunk base, crown
/// mid). `y` the ground it stands on.
fn wind_tree(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, y: f32, h: f32) {
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    let steel = if coin(rng, 0.5) { mul([0.86, 0.86, 0.82], 1.0 - 0.3 * c.wear) } else { mul(c.palette.steel, 0.9) };
    let blade = mix([0.90, 0.90, 0.86], PATINA[0], 0.25 + 0.4 * c.wear);
    g.part(Metal).cylinder(w, 0.2, 0.09, y, y + h, 8, rgba(steel));
    let n = rng.random_range(5..9);
    let turn0 = rng.random_range(0.0..std::f32::consts::TAU);
    g.lod(Lod::Mid, |g| {
        for k in 0..n {
            let a = turn0 + k as f32 / n as f32 * std::f32::consts::TAU;
            let from = h * (0.62 + 0.12 * (k % 3) as f32);
            let reach = h * (0.16 + 0.05 * (k % 2) as f32);
            let dir = Vec2::new(a.cos(), a.sin());
            let q = p + dir * reach;
            let top = y + from + reach * 0.9;
            g.part(Metal).beam(f.at(p.x, y + from, p.y), f.at(q.x, top, q.y), 0.07, rgba(steel));
            g.part(Panel).cylinder_capped(f.to_world(q.x, q.y), 0.17, 0.12, top, top + 0.55, 6, true, true, rgba(blade));
        }
    });
}

/// A bike at lot-local `p` along `dir` (near): two wheels and a frame.
fn bike(g: &mut Geometry, c: &Ctx, p: Vec2, dir: Vec2, col: [f32; 3], cargo: bool) {
    let f = &c.frame;
    let d = dir.normalize_or_zero();
    let side = d.perp() * 0.025;
    let (back, front) = (p - d * 0.55, p + d * if cargo { 1.1 } else { 0.55 });
    g.lod(Lod::Near, |g| {
        for q in [back, front] {
            g.part(Matte).tube_capped(f.at2(q - side, 0.33), f.at2(q + side, 0.33), 0.32, 8, true, true, rgba(DARK));
        }
        g.part(Matte).beam(f.at2(back, 0.45), f.at2(p + d * 0.35, 0.8), 0.06, rgba(col));
        g.part(Matte).beam(f.at2(p - d * 0.15, 0.85), f.at2(p + d * 0.45, 0.95), 0.05, rgba(col));
        if cargo {
            let box_at = p + d * 0.75;
            f.bx_yaw(g, Wood, box_at.x, 0.62, box_at.y, Vec3::new(0.7, 0.42, 0.62), mul(c.palette.wood, 0.75), d.y.atan2(d.x));
        }
    });
}

/// A run of bikes from `a` towards `b`, each `step` apart, against a rail
/// (mid) where `rack`.
fn bikes(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec2, b: Vec2, step: f32, rack: bool) {
    let len = a.distance(b);
    if len < 0.8 || !c.spans_lot(a, b) {
        return;
    }
    let along = (b - a) / len;
    let n = ((len / step).floor() as i32 + 1).min(8);
    for i in 0..n {
        if !coin(rng, 0.75) {
            continue;
        }
        let p = a + along * (i as f32 * step);
        if !room(c, p, 1.0) {
            continue;
        }
        let col = mul(BRIGHT[rng.random_range(0..BRIGHT.len())], 1.0 - 0.35 * c.wear);
        bike(g, c, p, along.perp(), col, coin(rng, 0.2));
    }
    if rack {
        let f = &c.frame;
        g.lod(Lod::Mid, |g| g.part(Matte).beam(f.at2(a, 0.6), f.at2(a + along * ((n - 1).max(0) as f32 * step), 0.6), 0.05, rgba(mul(c.palette.steel, 0.8))));
    }
}

/// A mono-pitch roof over `r` from its eave at `y` on the `front` side (or
/// the back) up to `y + rise`: the wedge of wall under it in `wall`, the
/// roof a slab overhanging by `over`. Returns the slab's ends (eave, ridge
/// mid-points) and the up normal of its top.
#[allow(clippy::too_many_arguments)]
fn mono_pitch(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, low_front: bool, over: f32, s: Surface, roof: [f32; 3], wall_s: Surface, wall: [f32; 3]) -> (Vec3, Vec3, Vec3) {
    let f = &c.frame;
    let (v_lo, v_hi) = if low_front { (r.v0, r.v1) } else { (r.v1, r.v0) };
    let sgn = if low_front { 1.0 } else { -1.0 };
    let span = r.d();
    let slope = rise / span.max(0.1);
    // The slab's top runs through (v_lo - over, eave) and (v_hi + over, ridge).
    let a = f.at(r.c().x, y - slope * over + 0.05, v_lo - sgn * over);
    let b = f.at(r.c().x, y + rise + slope * over + 0.05, v_hi + sgn * over);
    g.part(s).slab(a, b, r.w() + 2.0 * over.min(0.3), 0.14, rgba(roof));
    // The wedge: the high wall and the two triangles.
    let hi = if low_front { r.back() } else { r.front() };
    hi.bx(g, f, wall_s, 0.0, y + rise * 0.5, -0.1, Vec3::new(hi.len, rise, 0.2), wall);
    for face in [r.left(), r.right()] {
        let (p0, p1) = (Vec2::new(face.a.x, v_lo), Vec2::new(face.a.x, v_hi));
        g.part(wall_s).tri_facing(f.at2(p0, y), f.at2(p1, y), f.at2(p1, y + rise), f.dir3(face.out), rgba(wall));
    }
    let n = (b - a).cross(f.dir3(Vec2::X)).normalize_or_zero();
    let n = if n.y < 0.0 { -n } else { n };
    (a, b, n)
}

/// Solar panels over a roof slope from `a` to `b` (its top's mid-points)
/// `w` across, `n` its up normal: the PV skin in the base, a few panels of
/// another make patched in and a tarp over a leak (mid), the seams (near).
#[allow(clippy::too_many_arguments)]
fn pv_skin(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec3, b: Vec3, w: f32, n: Vec3, margin: f32) {
    let f = &c.frame;
    let along = f.dir3(Vec2::X);
    let up = (b - a).normalize_or_zero();
    let len = a.distance(b) - 2.0 * margin;
    let w = w - 2.0 * margin;
    if len < 1.0 || w < 1.0 {
        return;
    }
    let mid = (a + b) * 0.5 + n * 0.012;
    g.part(Glass).decal(mid, along, up, w, len, n, rgba(PV));
    let (cols, rows) = (((w / 1.05).floor() as i32).max(1), ((len / 1.7).floor() as i32).max(1));
    let (pw, ph) = (w / cols as f32, len / rows as f32);
    let cell = |i: i32, j: i32| mid + along * (-w * 0.5 + (i as f32 + 0.5) * pw) + up * (-len * 0.5 + (j as f32 + 0.5) * ph);
    let patches = (rng.random_range(0.0..2.0) + 4.0 * c.wear) as i32;
    let mut picks = Vec::new();
    for _ in 0..patches.min(6) {
        let pick = (rng.random_range(0..cols), rng.random_range(0..rows), rng.random_range(0..PV_PATCH.len()));
        // One patch a cell: two of other makes in one would z-fight.
        if !picks.iter().any(|(i, j, _)| (*i, *j) == (pick.0, pick.1)) {
            picks.push(pick);
        }
    }
    let tarp = (c.wear > 0.45 && coin(rng, 0.5)).then(|| (rng.random_range(0..cols), rng.random_range(0..rows)));
    g.lod(Lod::Mid, |g| {
        for (i, j, k) in &picks {
            g.part(Glass).decal(cell(*i, *j) + n * PATCH_D, along, up, pw - 0.08, ph - 0.08, n, rgba(PV_PATCH[*k]));
        }
        if let Some((i, j)) = tarp {
            g.part(Tarp).decal(cell(i, j) + n * (PATCH_D + 0.005), along, up, pw * 1.6, ph * 1.3, n, rgba(mul([0.22, 0.34, 0.52], 1.0 - 0.3 * c.wear)));
        }
    });
    g.lod(Lod::Near, |g| {
        for i in 1..cols {
            g.part(Matte).decal(mid + along * (-w * 0.5 + i as f32 * pw) + n * SEAM_D, along, up, 0.04, len, n, rgba([0.55, 0.56, 0.58]));
        }
        for j in 1..rows {
            g.part(Matte).decal(mid + up * (-len * 0.5 + j as f32 * ph) + n * (SEAM_D + 0.004), along, up, w, 0.04, n, rgba([0.55, 0.56, 0.58]));
        }
    });
}

/// A planted flat roof over `r` at `y`: a timber upstand, sedum and
/// grasses (base), clumps of shrubs, a raised bed and a water butt (mid);
/// overgrown on a worn roof, leaves spilling over the edge.
fn planted_roof(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32) {
    let f = &c.frame;
    if r.w() < 2.0 || r.d() < 2.0 {
        return;
    }
    parapet(g, c, &r.poly(), y, 0.3, 0.15, Wood, mul(c.palette.wood, 0.55 - 0.15 * c.wear));
    let inner = r.inset(0.2);
    ground(g, f, Foliage, &inner.poly(), y + 0.1, leaf(c, 0.7));
    let n = ((inner.w() * inner.d() / 7.0) as i32).clamp(1, 10) + (4.0 * c.wear) as i32;
    let mut clumps = Vec::new();
    for i in 0..n {
        let s = rng.random_range(0.5..1.3f32);
        let p = Vec2::new(rng.random_range(inner.u0 + s * 0.5..=inner.u1 - s * 0.5), rng.random_range(inner.v0 + s * 0.5..=inner.v1 - s * 0.5));
        // Heights in 5 cm steps, 3 mm apart clump by clump: no two tops
        // that overlap share a plane.
        let h = 0.25 + (rng.random_range(0.0..0.55f32) / 0.05).floor() * 0.05 + i as f32 * 0.003;
        clumps.push((p, s, h, rng.random_range(0.75..1.05f32)));
    }
    let bed = (inner.w() > 4.0 && inner.d() > 3.0 && coin(rng, 0.6)).then(|| Vec2::new(inner.u0 + 1.2, inner.c().y));
    g.lod(Lod::Mid, |g| {
        for (p, s, h, k) in clumps {
            if inner.u1 - inner.u0 > s && inner.v1 - inner.v0 > s {
                f.bx(g, Foliage, p.x, y + 0.1 + h * 0.5, p.y, Vec3::new(s, h, s * 0.9), leaf(c, k));
            }
        }
        if let Some(p) = bed {
            // Its tops off the clumps' 5 cm steps (4.6 cm into a step,
            // past the clumps' 3 mm offsets).
            f.bx(g, Wood, p.x, y + 0.373, p.y, Vec3::new(1.2, 0.546, (inner.d() - 1.0).min(4.0)), mul(c.palette.wood, 0.6));
            f.bx(g, Foliage, p.x, y + 0.716, p.y, Vec3::new(1.0, 0.16, (inner.d() - 1.2).min(3.8)), leaf(c, 1.1));
        }
    });
    for face in c.faces(&r.poly()) {
        creepers(g, c, rng, &face, (y - 2.5).max(0.3), y + 0.3, 0.2 + 0.5 * c.wear);
    }
}

/// A solar canopy over `r` at `y`: PV panels on a light steel frame on
/// posts standing on `foot`, tilted up to the back, a strip glowing amber
/// along its front edge and its underside lit after dark; patches and
/// seams as on a roof.
fn solar_canopy(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, foot: f32, y: f32) {
    let f = &c.frame;
    if r.w() < 1.2 || r.d() < 1.0 {
        return;
    }
    let steel = mul(c.palette.steel, 0.85 - 0.25 * c.wear);
    let rise = (r.d() * 0.12).min(0.8);
    let a = f.at(r.c().x, y, r.v0);
    let b = f.at(r.c().x, y + rise, r.v1);
    g.part(Glass).slab(a, b, r.w(), 0.12, rgba(PV));
    let n = (b - a).cross(f.dir3(Vec2::X)).normalize_or_zero();
    let n = if n.y < 0.0 { -n } else { n };
    pv_skin(g, c, rng, a, b, r.w(), n, 0.05);
    // Posts at the corners and every 5 m along the front and back.
    let k = ((r.w() / 5.0).ceil() as i32).max(1);
    for i in 0..=k {
        let u = r.u0 + 0.15 + i as f32 * (r.w() - 0.3) / k as f32;
        for (v, top) in [(r.v0 + 0.25, y - 0.12 + rise * 0.25 / r.d()), (r.v1 - 0.25, y + rise - 0.12 - rise * 0.25 / r.d())] {
            f.bx(g, Metal, u, (foot + top) * 0.5, v, Vec3::new(0.14, top - foot, 0.14), steel);
        }
    }
    // The glow: a strip along the front edge and the underside panel.
    let from = g.part(Glow).positions.len();
    f.bx(g, Glow, r.c().x, y - 0.17, r.v0 + 0.08, Vec3::new(r.w() - 0.2, 0.07, 0.07), mul(CANOPY_GLOW, 1.0 - 0.3 * c.wear));
    let under = (a + b) * 0.5 - n * 0.16;
    g.part(Glow).decal(under, f.dir3(Vec2::X), (b - a).normalize_or_zero(), r.w() - 0.4, a.distance(b) - 0.4, -n, rgba(mul(CANOPY_GLOW, 0.5 - 0.2 * c.wear)));
    mark_glow(g, from, NIGHT_MARK);
}

/// A low lamp on a timber post, lit after dark.
fn post_lamp(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    if !room(c, p, 0.3) {
        return;
    }
    let f = &c.frame;
    f.bx(g, Wood, p.x, h * 0.5, p.y, Vec3::new(0.14, h, 0.14), mul(c.palette.wood, 0.5));
    let from = g.part(Glow).positions.len();
    f.bx(g, Glow, p.x, h + 0.12, p.y, Vec3::new(0.2, 0.24, 0.2), mul(INSIDE, 1.1));
    mark_glow(g, from, NIGHT_MARK);
    g.lod(Lod::Mid, |g| f.bx(g, Matte, p.x, h + 0.29, p.y, Vec3::new(0.3, 0.06, 0.3), DARK));
}

/// A raised veg bed at lot-local `p`, `w` by `d` (mid): boards, soil,
/// rows of greens, a cane frame of beans on some.
fn veg_bed(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, w: f32, d: f32) {
    if !room(c, p, (w.max(d)) * 0.5 + 0.2) {
        return;
    }
    let f = &c.frame;
    let canes = coin(rng, 0.35);
    let wood = mul(c.palette.wood, 0.65 - 0.2 * c.wear);
    let green = leaf(c, rng.random_range(0.9..1.15));
    g.lod(Lod::Mid, |g| {
        f.bx(g, Wood, p.x, 0.2, p.y, Vec3::new(w, 0.4, d), wood);
        f.bx(g, Dirt, p.x, 0.405, p.y, Vec3::new(w - 0.1, 0.03, d - 0.1), [0.22, 0.17, 0.12]);
        let rows = ((d / 0.5).floor() as i32).max(1);
        for j in 0..rows {
            let v = p.y - d * 0.5 + (j as f32 + 0.5) * d / rows as f32;
            f.bx(g, Foliage, p.x, 0.5, v, Vec3::new(w - 0.25, 0.18, 0.22), green);
        }
        if canes {
            f.bx(g, Foliage, p.x, 1.1, p.y, Vec3::new(w - 0.4, 1.2, 0.18), mul(green, 0.85));
        }
    });
}

/// A small tree in a timber tub standing at `y` (mid): a balcony's.
fn potted_tree(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, col: [f32; 3]) {
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.lod(Lod::Mid, |g| {
        f.bx(g, Wood, p.x, y + 0.275, p.y, Vec3::new(0.7, 0.55, 0.7), mul(c.palette.wood, 0.55));
        g.part(Wood).cylinder(w, 0.07, 0.05, y + 0.6, y + 1.3, 5, rgba(c.palette.bark));
        g.part(Foliage).cylinder_capped(w, 0.55, 0.35, y + 1.1, y + 2.1, 7, true, true, rgba(col));
    });
}

/// A pile of produce crates at lot-local `p` (mid).
fn crates(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, n: i32) {
    if !room(c, p, 0.8) {
        return;
    }
    let f = &c.frame;
    let wood = mul(c.palette.wood, 0.8 - 0.2 * c.wear);
    let fill: Vec<[f32; 3]> = (0..n).map(|_| if coin(rng, 0.5) { leaf(c, 1.15) } else { BLOOMS[rng.random_range(0..2)] }).collect();
    g.lod(Lod::Mid, |g| {
        for (k, col) in fill.iter().enumerate() {
            let q = p + Vec2::new((k % 2) as f32 * 0.62, 0.0);
            let y = (k / 2) as f32 * 0.32;
            f.bx(g, Wood, q.x, y + 0.15, q.y, Vec3::new(0.56, 0.3, 0.4), wood);
            f.bx(g, Foliage, q.x, y + 0.315, q.y, Vec3::new(0.48, 0.04, 0.32), *col);
        }
    });
}

/// A cable from a roof edge down a wall to a battery box at its foot
/// (near; the box mid).
fn battery_box(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, top: f32) {
    if t.abs() + 0.5 > face.len * 0.5 {
        return;
    }
    let f = &c.frame;
    g.lod(Lod::Mid, |g| face.bx(g, f, Panel, t, 0.75, 0.22, Vec3::new(0.7, 1.1, 0.36), [0.70, 0.71, 0.68]));
    g.lod(Lod::Near, |g| {
        let a = f.at2(face.at(t + 0.2, 0.07), top);
        let b = f.at2(face.at(t + 0.2, 0.07), 1.3);
        g.part(Matte).beam(a, b, 0.04, rgba(DARK));
    });
}

/// A garden shed of boards for a lot too small for anything else, with a
/// planted roof and a water butt.
fn shed_home(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let a = R::of(&c.poly);
    let Some(r) = fit(c, R::new(a.c().x - 1.6, a.c().x + 1.6, a.v0 + 0.4, a.v0 + 3.2), 1.4) else { return };
    let col = timber(c, rng);
    block(g, c, Wood, &r, 0.0, 2.4, col);
    let fr = r.front();
    fr.decal(g, &c.frame, Wood, 0.0, 1.0, DOOR_D, (fr.len * 0.4).min(0.9), 2.0, frame_col(c, rng));
    planted_roof(g, c, rng, &r, 2.4);
}

// ── homes ─────────────────────────────────────────────────────────────────────

/// Timber-and-glass terraces: narrow houses clad in boards each its own
/// timber, glazed full width floor by floor between party-wall fins, a
/// balcony with planters here and there, under planted roofs (flat behind
/// an upstand or a mono-pitch of sedum); front gardens of veg beds, water
/// butts and bikes, creepers down the walls; bulbs over the gardens.
pub fn timber_terrace(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(2.9, 3.2);
    let area = R::of(&c.poly);
    let depth = if area.d() < 13.0 { area.d() } else { (area.d() * 0.5).clamp(8.0, 11.0) };
    let garden = if area.d() > depth + 4.5 { 2.6 } else { 0.0 };
    let Some(row) = fit(c, R::new(area.u0, area.u1, area.v0 + garden, area.v0 + garden + depth), 4.0) else { return shed_home(g, c, rng) };
    let n = ((row.w() / 6.2).round() as i32).max(1);
    let hw = row.w() / n as f32;
    let sloped = coin(rng, 0.5);
    let fin = mul(render(c, rng), 0.9);
    let floors = (c.floors as i32).clamp(2, 4);
    for i in 0..n {
        let hr = R::new(row.u0 + i as f32 * hw, row.u0 + (i + 1) as f32 * hw, row.v0, row.v1);
        let fl = (floors + if coin(rng, 0.3) { -1 } else { 0 }).max(2);
        let h = fl as f32 * fh;
        let wood = if coin(rng, 0.15) { mul(CHARRED, 1.0 + 0.6 * c.wear) } else { timber(c, rng) };
        let fc = frame_col(c, rng);
        block(g, c, Wood, &hr, 0.0, h, wood);
        let front = hr.front();
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        // The door, then the glass: the ground floor glazed beside it, the
        // floors over it glazed nearly wall to wall.
        let dt = side * (hw * 0.5 - 0.95);
        front.decal(g, f, Wood, dt, 1.1, DOOR_D, 1.0, 2.2, fc);
        let gw = (hw - 3.0).max(1.0);
        window(g, c, rng, &front, -side * 0.55, fh * 0.5, gw, fh * 0.72, fc, 0.0);
        for k in 1..fl {
            let y = k as f32 * fh + fh * 0.52;
            window(g, c, rng, &front, 0.0, y, hw - 1.4, fh * 0.66, fc, 0.25);
            if k == 1 && coin(rng, 0.45) {
                let rail = mul(pal.steel, 0.6);
                balcony(g, c, &front, 0.0, k as f32 * fh + 0.12, (hw - 1.6).min(4.0), 0.9, mul(wood, 0.8), rail);
                window_box(g, c, rng, &front, 0.0, k as f32 * fh + 0.36, (hw - 2.2).clamp(0.6, 3.0));
            }
        }
        let back = hr.back();
        pane_row(g, c, rng, &back, 0.0, fl, fh, 2.6, 1.1, fh * 0.5, true);
        creepers(g, c, rng, &front, 0.4, h, 0.15 + 0.4 * c.wear);
        creepers(g, c, rng, &back, 0.4, h, 0.3 + 0.4 * c.wear);
        grime(g, c, rng, &front, 0.0, h, 0.2 + 1.2 * c.wear);
        // The fin between this house and the next.
        if i > 0 {
            front.bx(g, f, Plaster, -hw * 0.5, h * 0.5 + 0.15, 0.2, Vec3::new(0.3, h + 0.3, 0.4), fin);
        }
        // The roof.
        if sloped {
            let rise = (hr.d() * 0.16).clamp(0.8, 1.8);
            let (a, b, nrm) = mono_pitch(g, c, &hr, h, rise, coin(rng, 0.5), 0.35, Wood, mul(pal.wood, 0.5), Wood, wood);
            if coin(rng, 0.3) {
                pv_skin(g, c, rng, a, b, hr.w(), nrm, 0.3);
            } else {
                let mid = (a + b) * 0.5 + nrm * 0.02;
                g.part(Foliage).decal(mid, f.dir3(Vec2::X), (b - a).normalize_or_zero(), hr.w() - 0.2, a.distance(b) - 0.2, nrm, rgba(leaf(c, 0.72)));
            }
        } else {
            planted_roof(g, c, rng, &hr, h);
        }
        // A rain tank by the back door.
        rain_tank(g, c, rng, Vec2::new(hr.c().x + side * (hw * 0.5 - 0.9), hr.v1 + 0.9), 0.55, 1.4, Some(f.at(hr.c().x + side * (hw * 0.5 - 0.9), h - 0.1, hr.v1 + 0.1)));
        // The front garden: a path, a veg bed, a butt, a bike.
        if garden > 0.0 {
            let pv = (row.v0 - 0.05 + area.v0) * 0.5;
            ground(g, f, Paving, &clip_box(&c.poly, hr.c().x + dt - 0.6, hr.c().x + dt + 0.6, area.v0 + 0.05, row.v0 - 0.02), PATH_Y, mul(pal.paving, 0.85));
            veg_bed(g, c, rng, Vec2::new(hr.c().x - side * 0.9, pv), (hw - 3.4).clamp(0.8, 2.4), 1.2);
            if coin(rng, 0.5) {
                bikes(g, c, rng, Vec2::new(hr.c().x + dt + side * 1.3, pv - 0.4), Vec2::new(hr.c().x + dt + side * 1.3, pv + 0.4), 0.8, false);
            }
        }
    }
    if garden > 0.0 {
        ground(g, f, Foliage, &clip_box(&c.poly, row.u0, row.u1, area.v0 + 0.05, row.v0 - 0.05), MEADOW_Y, leaf(c, 0.8));
        // Bulbs along the gardens' front, post to post.
        let posts = n.clamp(1, 6);
        for i in 0..posts {
            let (u0, u1) = (row.u0 + 0.4 + i as f32 * (row.w() - 0.8) / posts as f32, row.u0 + 0.4 + (i + 1) as f32 * (row.w() - 0.8) / posts as f32);
            let v = area.v0 + 0.4;
            if room(c, Vec2::new(u0, v), 0.2) && room(c, Vec2::new(u1, v), 0.2) {
                f.bx(g, Wood, u0, 1.2, v, Vec3::new(0.1, 2.4, 0.1), mul(pal.wood, 0.5));
                bulbs(g, c, rng, p3(u0, 2.3, v), p3(u1, 2.3, v), 0.3, 1.0);
            }
        }
    }
}

/// Solar-shingle townhouses: two to four tall houses in render or charred
/// boards under steep mono-pitch roofs skinned in solar shingles to the
/// street (patched with odd panels, a tarp over a leak on a worn one), a
/// little solar canopy over each door, a battery box and its cable, a
/// rain tank at the side, a heat pump behind.
pub fn solar_townhouses(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(2.9, 3.2);
    let area = R::of(&c.poly);
    let depth = (area.d() - 3.0).clamp(4.0, 11.0).min(area.d());
    let set = ((area.d() - depth) * 0.35).clamp(0.0, 3.0);
    let Some(row) = fit(c, R::new(area.u0 + 0.4, area.u1 - 0.4, area.v0 + set, area.v0 + set + depth), 4.0) else { return shed_home(g, c, rng) };
    let n = ((row.w() / 6.5).round() as i32).clamp(1, 4);
    let hw = row.w() / n as f32;
    let floors = (c.floors as i32).clamp(2, 3);
    for i in 0..n {
        let hr = R::new(row.u0 + i as f32 * hw + 0.05, row.u0 + (i + 1) as f32 * hw - 0.05, row.v0, row.v1);
        let h = floors as f32 * fh;
        let charred = coin(rng, 0.35);
        let (ws, wall) = if charred { (Wood, mul(CHARRED, 1.0 + 0.8 * c.wear)) } else { (Plaster, render(c, rng)) };
        let fc = frame_col(c, rng);
        block(g, c, ws, &hr, 0.0, h, wall);
        let front = hr.front();
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dt = side * (hr.w() * 0.5 - 0.9);
        front.decal(g, f, Wood, dt, 1.1, DOOR_D, 1.0, 2.2, fc);
        // The porch canopy over the door, glowing under after dark.
        let reach = (c.reach_out(&front) - 0.3).min(1.4);
        if reach > 0.7 {
            let p = front.at(dt, reach * 0.5);
            solar_canopy(g, c, rng, &R::new(p.x - 0.8, p.x + 0.8, p.y - reach * 0.5, p.y + reach * 0.5 - 0.02), 0.0, 2.6);
        }
        window(g, c, rng, &front, -side * 0.7, fh * 0.55, (hr.w() - 3.2).max(0.9), fh * 0.5, fc, 0.4);
        for k in 1..floors {
            for s in [-1.0f32, 1.0] {
                window(g, c, rng, &front, s * hr.w() * 0.22, k as f32 * fh + fh * 0.55, (hr.w() * 0.3).min(1.6), fh * 0.55, fc, 0.3);
            }
        }
        pane_row(g, c, rng, &hr.back(), 0.0, floors, fh, 2.4, 1.0, fh * 0.5, true);
        grime(g, c, rng, &front, 0.0, h, 0.2 + 1.4 * c.wear);
        creepers(g, c, rng, &hr.back(), 0.3, h, 0.2 + 0.5 * c.wear);
        // The roof: steep, its slope to the street skinned in PV.
        let rise = (hr.d() * 0.55).clamp(2.6, 5.5);
        let roof = mul(pal.steel, 0.5);
        let (a, b, nrm) = mono_pitch(g, c, &hr, h, rise, true, 0.3, Metal, roof, ws, wall);
        pv_skin(g, c, rng, a, b, hr.w(), nrm, 0.15);
        // A window in the high back wall.
        let bk = hr.back();
        window(g, c, rng, &bk, 0.0, h + rise * 0.45, 1.0, (rise * 0.4).min(1.2), fc, 0.0);
        battery_box(g, c, &hr.back(), side * hr.w() * 0.3, h + rise - 0.2);
        let tp = Vec2::new(if i == 0 { hr.u0 - 0.75 } else { hr.u1 + 0.75 }, hr.c().y);
        if i == 0 || i == n - 1 {
            rain_tank(g, c, rng, tp, 0.6, 1.8, Some(f.at(tp.x, h - 0.1, hr.c().y)));
        }
        // A heat pump behind.
        let hp = Vec2::new(hr.c().x, hr.v1 + 0.6);
        if room(c, hp, 0.6) {
            g.lod(Lod::Mid, |g| {
                f.bx(g, Panel, hp.x, 0.5, hp.y, Vec3::new(1.0, 0.9, 0.4), [0.80, 0.80, 0.78]);
                g.part(Matte).cylinder(f.to_world(hp.x, hp.y + 0.205), 0.3, 0.3, 0.45, 0.47, 10, rgba(DARK));
            });
        }
    }
    // Gravel to the street, planters and bins.
    if set > 1.0 {
        ground(g, f, Dirt, &clip_box(&c.poly, row.u0, row.u1, area.v0 + 0.05, row.v0 - 0.05), GRAVEL_Y, mul(pal.paving, 0.7));
    }
    // Planters by the doors, clear of the porch canopies' posts.
    if set > 2.6 {
        for i in 0..n {
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let p = Vec2::new(row.u0 + (i as f32 + 0.5) * hw - side * 0.2, row.v0 - 1.6);
            if room(c, p, 0.6) {
                let col = leaf(c, 0.95);
                g.lod(Lod::Mid, |g| planter(g, f, p, 0.5, mul(c.palette.wood, 0.6), col));
            }
        }
    }
}

/// A vine-covered tower: a stained concrete frame glazed floor by floor,
/// deep balcony gardens wrapping its street side and one flank, each with
/// a trough of shrubs along its edge and creepers falling to the floor
/// below, a few trees in pots at the corners; a glass lobby and a bike
/// store under it; on the roof a greenhouse glowing after dark, a wind
/// tree, a water tank on legs and patched panels.
pub fn vine_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(3.0, 3.3);
    let area = R::of(&c.poly);
    let bw = (area.w() - 4.0).clamp(8.0, 24.0);
    let bd = (area.d() - 5.0).clamp(8.0, 18.0);
    let set = ((area.d() - bd) * 0.4).clamp(1.6, 5.0);
    let Some(b) = fit(c, R::new(area.c().x - bw * 0.5, area.c().x + bw * 0.5, area.v0 + set, area.v0 + set + bd), 7.0) else { return timber_terrace(g, c, rng) };
    let floors = (c.floors as i32).clamp(4, 10);
    let h = floors as f32 * fh;
    let conc = mul(pick(rng, &pal.concrete), 0.95 - 0.25 * c.wear);
    let fc = frame_col(c, rng);
    block(g, c, Concrete, &b, 0.0, h, conc);
    // The street floor: a glass lobby and the bike store's timber slats.
    let front = b.front();
    let lw = (front.len * 0.45).min(8.0);
    window(g, c, rng, &front, -front.len * 0.25, fh * 0.5, lw, fh * 0.72, fc, 0.0);
    front.decal(g, f, Wood, front.len * 0.25, fh * 0.45, DOOR_D, (front.len * 0.4).min(7.0), fh * 0.8, timber(c, rng));
    bikes(g, c, rng, front.at(front.len * 0.05, 1.2), front.at(front.len * 0.45, 1.2), 0.75, true);
    // The balcony faces: the street side and one flank.
    let flank = if c.seed % 2 == 0 { b.left() } else { b.right() };
    let other = if c.seed % 2 == 0 { b.right() } else { b.left() };
    let rail = mul(pal.steel, 0.55);
    let slab = mul(conc, 0.92);
    // Balconies as deep as the lot lets them be (1.5 m at most); a face
    // with less than 0.9 m before the lot's edge has none.
    let mut plain = vec![other, b.back()];
    for face in [front, flank] {
        let mut depth = 1.5f32;
        while depth >= 0.9 && !(c.inside_local(face.at(-face.len * 0.5, depth + 0.05)) && c.inside_local(face.at(face.len * 0.5, depth + 0.05))) {
            depth -= 0.1;
        }
        if depth < 0.9 {
            plain.push(face);
            continue;
        }
        let bays = ((face.len - 1.0) / 3.0).floor().max(1.0) as i32;
        let step = (face.len - 1.0) / bays as f32;
        for k in 1..floors {
            let y = k as f32 * fh;
            // The slab, its trough and shrubs (base: they make the tower green).
            face.bx(g, f, Concrete, 0.0, y, depth * 0.5, Vec3::new(face.len - 0.1, 0.24, depth), slab);
            let bare = coin(rng, 0.12 + 0.3 * c.wear);
            if !bare {
                face.bx(g, f, Foliage, 0.0, y + 0.42, depth - 0.32, Vec3::new(face.len - 0.5, 0.6, 0.5), leaf(c, rng.random_range(0.8..1.05)));
            }
            g.lod(Lod::Mid, |g| face.decal(g, f, Matte, 0.0, y + 0.75, depth + 0.01, face.len - 0.3, 0.9, rail));
            for i in 0..bays {
                let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
                window(g, c, rng, &face, t, y + fh * 0.5, step * 0.8, fh * 0.7, fc, 0.0);
            }
            // Creepers from the trough down towards the floor below.
            if !bare {
                let hang = Face::new(face.at(-face.len * 0.5, depth + 0.02), face.at(face.len * 0.5, depth + 0.02), face.out);
                creepers(g, c, rng, &hang, (y - fh + 0.4).max(0.5), y - 0.1, 0.55);
            } else if coin(rng, 0.5) {
                // A tarp over a bare balcony's rail.
                let t = rng.random_range(-(face.len * 0.3)..=face.len * 0.3);
                g.lod(Lod::Mid, |g| face.decal(g, f, Tarp, t, y + 0.7, depth + 0.02, 2.0, 0.8, mul([0.24, 0.36, 0.54], 1.0 - 0.3 * c.wear)));
            }
            // A tree in a pot at a corner, now and then.
            if coin(rng, 0.3) {
                let p = face.at(face.len * 0.5 - 0.9, depth * 0.5);
                potted_tree(g, c, p, y + 0.12, leaf(c, 0.9));
            }
        }
    }
    // The plain faces: windows, creepers up from the ground.
    for face in plain {
        let n = ((face.len - 1.0) / 2.8).floor() as i32;
        // (The street face's ground floor is the lobby.)
        for k in if face.is_front { 1 } else { 0 }..floors {
            for i in 0..n.max(0) {
                let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * (face.len - 1.0) / n as f32;
                window(g, c, rng, &face, t, k as f32 * fh + fh * 0.55, 1.2, fh * 0.55, fc, 0.15);
            }
        }
        let top = h * rng.random_range(0.3..0.8);
        creepers(g, c, rng, &face, 0.3, top, 0.35 + 0.4 * c.wear);
        grime(g, c, rng, &face, 0.0, h, 0.4 + 1.6 * c.wear);
    }
    parapet(g, c, &b.poly(), h, 0.9, 0.2, Concrete, mul(conc, 0.9));
    // The roof: a greenhouse, a wind tree, a tank on legs, panels.
    let roof = b.inset(0.6);
    let gh = R::new(roof.u0 + 0.3, roof.u0 + roof.w() * 0.5, roof.v0 + 0.3, roof.v1 - 0.3);
    if gh.w() > 2.5 && gh.d() > 2.5 {
        greenhouse(g, c, rng, &gh, h, 2.4, SODIUM);
    }
    let rest = R::new(gh.u1 + 0.6, roof.u1 - 0.3, roof.v0 + 0.3, roof.v1 - 0.3);
    if rest.w() > 2.5 && rest.d() > 2.5 {
        solar_canopy(g, c, rng, &R::new(rest.u0, rest.u1, rest.v0, rest.c().y), h, h + 2.4);
        wind_tree(g, c, rng, Vec2::new(rest.c().x, rest.v1 - 1.0), h, 6.0);
        let tp = Vec2::new(rest.u1 - 1.0, rest.v1 - 1.0);
        let col = mul(TANKS[2], 1.0 - 0.3 * c.wear);
        g.part(Metal).cylinder_capped(f.to_world(tp.x, tp.y), 0.8, 0.8, h + 1.2, h + 3.0, 10, true, true, rgba(col));
        g.lod(Lod::Mid, |g| {
            for s in [-1.0f32, 1.0] {
                f.bx(g, Metal, tp.x + s * 0.55, h + 0.6, tp.y, Vec3::new(0.1, 1.2, 0.1), CHARCOAL);
            }
        });
    }
}

/// A greenhouse over `r` standing at `y`: low walls, dirty glass sides and
/// a gabled glass roof along its longer side, white glazing bars (mid), a
/// glow after dark in `light` round its sides (grow lamps inside).
fn greenhouse(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32, h: f32, light: [f32; 3]) {
    let f = &c.frame;
    let glass = dirty_glass(c);
    let rot = mul(c.palette.wood, 0.55);
    block(g, c, Wood, r, y, y + 0.5, rot);
    block(g, c, Glass, r, y + 0.5, y + h, glass);
    let along_u = r.w() >= r.d();
    let rise = (if along_u { r.d() } else { r.w() } * 0.3).clamp(0.6, 2.2);
    gabled(g, c, r, y + h, rise, along_u, 0.08, Glass, mix(glass, [0.6, 0.66, 0.6], 0.15), Glass, glass);
    let lit = coin(rng, 0.75 - 0.4 * c.wear);
    if lit {
        let from = g.part(Glow).positions.len();
        for face in r.faces() {
            face.decal(g, f, Glow, 0.0, y + 0.5 + (h - 0.5) * 0.5, 0.02, face.len - 0.2, (h - 0.7).max(0.3), mul(light, 0.55));
        }
        mark_glow(g, from, NIGHT_MARK);
    }
    g.lod(Lod::Mid, |g| {
        for face in r.faces() {
            let k = ((face.len / 1.3).floor() as i32).max(1);
            for i in 1..k {
                face.decal(g, f, Matte, -face.len * 0.5 + i as f32 * face.len / k as f32, y + 0.5 + (h - 0.5) * 0.5, 0.032, 0.05, h - 0.5, [0.80, 0.82, 0.78]);
            }
        }
    });
}

// ── shops ─────────────────────────────────────────────────────────────────────

/// A glass market hall: a brick plinth, iron and dirty glass walls lit
/// inside, a gabled glass roof with a ridge vent; stalls of crates under a
/// solar canopy out front; behind it a two-storey timber block with a farm
/// on its roof (beds in rows, a greenhouse, a tank, a wind tree).
pub fn glass_market_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 9.0 || area.d() < 10.0 {
        return coop_store(g, c, rng);
    }
    let fore = (area.d() * 0.2).clamp(3.0, 7.0);
    let hw = (area.w() - 1.0).min(36.0);
    let hd = ((area.d() - fore) * 0.6).clamp(8.0, 20.0);
    let Some(hall) = fit(c, R::new(area.c().x - hw * 0.5, area.c().x + hw * 0.5, area.v0 + fore, area.v0 + fore + hd), 6.0) else { return coop_store(g, c, rng) };
    let h = 5.5;
    let iron = mul(PATINA[1], 0.75 - 0.2 * c.wear);
    let vent = copper(c, rng);
    block(g, c, Brick, &hall, 0.0, 1.0, mul(pal.brick(0), 0.8));
    block(g, c, Glass, &hall, 1.0, h, dirty_glass(c));
    let rise = (hall.d() * 0.25).clamp(2.0, 4.5);
    gabled(g, c, &hall, h, rise, true, 0.4, Glass, mix(dirty_glass(c), [0.5, 0.55, 0.5], 0.15), Glass, dirty_glass(c));
    // The ridge vent and the iron frame.
    g.lod(Lod::Mid, |g| {
        f.bx(g, Metal, hall.c().x, h + rise + 0.25, hall.c().y, Vec3::new(hall.w() * 0.8, 0.5, 1.2), vent);
        for face in hall.faces() {
            let k = ((face.len / 3.0).floor() as i32).max(1);
            for i in 0..=k {
                let t = (-face.len * 0.5 + i as f32 * face.len / k as f32).clamp(-face.len * 0.5 + 0.1, face.len * 0.5 - 0.1);
                face.decal(g, f, Matte, t, (1.0 + h) * 0.5, FRAME, 0.16, h - 1.0, iron);
            }
            face.decal(g, f, Matte, 0.0, h - 0.1, FRAME, face.len, 0.2, iron);
        }
    });
    // Lit inside: the stalls seen through the glass.
    let front = hall.front();
    let bays = ((front.len / 3.0).floor() as i32).max(1);
    for i in 0..bays {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * front.len / bays as f32;
        if coin(rng, 0.8 - 0.3 * c.wear) {
            front.glaze(g, c, t, 2.4, PANE, front.len / bays as f32 - 0.5, 2.6, Pane::Lit(mul(INSIDE, rng.random_range(0.55..0.85))));
        }
    }
    for face in [hall.left(), hall.right(), hall.back()] {
        let k = ((face.len / 4.0).floor() as i32).max(1);
        for i in 0..k {
            let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / k as f32;
            if coin(rng, 0.6 - 0.3 * c.wear) {
                face.glaze(g, c, t, 2.4, PANE, face.len / k as f32 - 0.6, 2.4, Pane::Lit(mul(INSIDE, rng.random_range(0.5..0.8))));
            }
        }
    }
    sign_board(g, c, rng, &front, 0.0, h + 0.5, (front.len * 0.4).min(9.0), 0.8);
    // The forecourt: stalls under a canopy.
    let fc_r = R::new(hall.u0 + 0.5, hall.u1 - 0.5, area.v0 + 0.6, hall.v0 - 0.6);
    if let Some(fr) = fit(c, fc_r, 2.2) {
        ground(g, f, Paving, &fr.poly(), PATH_Y, mul(pal.paving, 0.9));
        let cw = (fr.w() * 0.6).min(14.0);
        solar_canopy(g, c, rng, &R::new(fr.c().x - cw * 0.5, fr.c().x + cw * 0.5, fr.v0 + 0.2, fr.v1 - 0.2), 0.0, 3.0);
        let k = ((cw - 1.0) / 2.2).floor() as i32;
        for i in 0..k.clamp(0, 6) {
            crates(g, c, rng, Vec2::new(fr.c().x - cw * 0.5 + 1.0 + i as f32 * 2.2, fr.c().y), 2 + i % 3);
        }
        bikes(g, c, rng, Vec2::new(fr.u0 + 0.6, fr.v0 + 0.5), Vec2::new(fr.u0 + 0.6, fr.v1 - 0.5), 0.8, false);
    }
    // The block behind and its roof farm.
    let back = R::new(hall.u0 + 0.4, hall.u1 - 0.4, hall.v1 + 1.2, (hall.v1 + 1.2 + (area.v1 - hall.v1 - 1.6).min(14.0)).min(area.v1 - 0.4));
    if let Some(bk) = fit(c, back, 5.0) {
        let fh = 3.3;
        let bh = 2.0 * fh;
        let wood = timber(c, rng);
        block(g, c, Wood, &bk, 0.0, bh, wood);
        for face in [bk.left(), bk.right(), bk.back()] {
            pane_row(g, c, rng, &face, 0.0, 2, fh, 2.8, 1.2, fh * 0.5, true);
            creepers(g, c, rng, &face, 0.3, bh, 0.3 + 0.4 * c.wear);
        }
        parapet(g, c, &bk.poly(), bh, 1.0, 0.15, Wood, mul(wood, 0.8));
        roof_farm(g, c, rng, &bk.inset(0.4), bh);
    }
}

/// A farm on a flat roof over `r` at `y`: a deck, raised beds in rows
/// (base: they read from the street), a lean-to greenhouse, a tank, a wind
/// tree, bulbs over the path.
fn roof_farm(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32) {
    let f = &c.frame;
    if r.w() < 3.0 || r.d() < 3.0 {
        return;
    }
    ground(g, f, Wood, &r.poly(), y + 0.06, mul(c.palette.wood, 0.6 - 0.2 * c.wear));
    let gh_w = (r.w() * 0.28).clamp(2.0, 6.0);
    let beds = R::new(r.u0 + 0.4, r.u1 - gh_w - 0.6, r.v0 + 0.4, r.v1 - 0.4);
    if beds.w() > 1.5 && beds.d() > 1.2 {
        let rows = ((beds.d() / 1.6).floor() as i32).max(1);
        let wood = mul(c.palette.wood, 0.7 - 0.2 * c.wear);
        for j in 0..rows {
            let v = beds.v0 + (j as f32 + 0.5) * beds.d() / rows as f32;
            f.bx(g, Wood, beds.c().x, y + 0.36, v, Vec3::new(beds.w(), 0.6, 0.9), wood);
            f.bx(g, Foliage, beds.c().x, y + 0.78, v, Vec3::new(beds.w() - 0.2, 0.3, 0.75), leaf(c, rng.random_range(0.9..1.2)));
        }
        let (a, b) = (Vec2::new(beds.u0, beds.c().y), Vec2::new(beds.u1, beds.c().y));
        bulbs(g, c, rng, p3(a.x, y + 2.2, a.y), p3(b.x, y + 2.2, b.y), 0.3, 1.0);
        for p in [a, b] {
            f.bx(g, Wood, p.x, y + 1.15, p.y, Vec3::new(0.08, 2.2, 0.08), mul(c.palette.wood, 0.5));
        }
    }
    let gh = R::new(r.u1 - gh_w, r.u1 - 0.2, r.v0 + 0.3, r.v1 - 0.3);
    if gh.w() > 1.5 && gh.d() > 1.5 {
        greenhouse(g, c, rng, &gh, y + 0.06, 2.2, SODIUM);
    }
    if r.d() > 5.0 && coin(rng, 0.7) {
        wind_tree(g, c, rng, Vec2::new(r.u0 + 1.2, r.v1 - 1.0), y, 5.0);
    }
}

/// A co-op store: a low timber shop set back behind a forecourt under a
/// solar canopy, its window lit and its name painted over it, produce on
/// trestles and crates, a chalkboard, bikes and a cargo bike, a flat over
/// it under a planted roof, a rain tank at the side.
pub fn coop_store(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh().clamp(3.0, 3.4);
    let area = R::of(&c.poly);
    let fore = (area.d() * 0.3).clamp(0.0, 6.0);
    let sw = (area.w() - 1.2).min(18.0);
    let sd = (area.d() - fore - 0.5).min(12.0);
    let Some(s) = fit(c, R::new(area.c().x - sw * 0.5, area.c().x + sw * 0.5, area.v0 + fore, area.v0 + fore + sd), 3.5) else { return shed_home(g, c, rng) };
    let floors = (c.floors as i32).clamp(1, 2);
    let h = floors as f32 * fh;
    let wood = timber(c, rng);
    let fc = frame_col(c, rng);
    block(g, c, Wood, &s, 0.0, h, wood);
    let front = s.front();
    // The shop front: big windows either side of the door.
    let gw = ((front.len - 1.6) * 0.5 - 0.4).max(0.6);
    front.decal(g, f, Wood, 0.0, 1.1, DOOR_D, 1.2, 2.2, fc);
    for k in [-1.0f32, 1.0] {
        let t = k * (0.8 + 0.2 + gw * 0.5);
        if coin(rng, 0.85 - 0.4 * c.wear) {
            front.glaze(g, c, t, 1.45, PANE, gw, 1.9, Pane::Lit(mul(INSIDE, rng.random_range(0.6..0.9))));
        } else {
            front.glaze(g, c, t, 1.45, PANE, gw, 1.9, Pane::Dark);
        }
        g.lod(Lod::Mid, |g| front.window_frame(g, c, t, 1.45, gw, 1.9, 0.2, fc));
    }
    sign_board(g, c, rng, &front, 0.0, fh - 0.4, (front.len * 0.6).min(6.0), 0.5);
    if floors > 1 {
        for k in 1..floors {
            for j in [-1.0f32, 1.0] {
                window(g, c, rng, &front, j * front.len * 0.25, k as f32 * fh + fh * 0.55, (front.len * 0.25).min(1.6), fh * 0.55, fc, 0.5);
            }
        }
    }
    for face in [s.left(), s.right(), s.back()] {
        pane_row(g, c, rng, &face, 0.0, floors, fh, 3.0, 1.0, fh * 0.45, true);
        creepers(g, c, rng, &face, 0.3, h, 0.25 + 0.4 * c.wear);
        grime(g, c, rng, &face, 0.0, h, 0.2 + 1.2 * c.wear);
    }
    planted_roof(g, c, rng, &s, h);
    battery_box(g, c, &s.right(), 0.0, h - 0.1);
    rain_tank(g, c, rng, Vec2::new(s.u0 - 0.8, s.v1 - 1.0), 0.6, 1.8, Some(f.at(s.u0 - 0.8, h - 0.1, s.v1 - 0.9)));
    // The forecourt under its canopy.
    if fore > 2.2 {
        let fr = R::new(s.u0, s.u1, area.v0 + 0.4, s.v0 - 0.1);
        if let Some(fr) = fit(c, fr, 1.8) {
            ground(g, f, Paving, &fr.poly(), PATH_Y, mul(pal.paving, 0.85));
            solar_canopy(g, c, rng, &R::new(fr.u0 + 0.2, fr.u1 - 0.2, fr.v0 + 0.3, fr.v1 - 0.05), 0.0, 2.8);
            let k = ((fr.w() - 3.0) / 1.6).floor() as i32;
            for i in 0..k.clamp(0, 5) {
                crates(g, c, rng, Vec2::new(fr.u0 + 1.6 + i as f32 * 1.6, fr.c().y), 2 + (i % 2));
            }
            // A chalkboard by the door.
            let cb = front.at(1.1, 0.6);
            if room(c, cb, 0.4) {
                g.lod(Lod::Mid, |g| f.bx_yaw(g, Matte, cb.x, 0.55, cb.y, Vec3::new(0.6, 1.1, 0.06), [0.14, 0.16, 0.15], 0.3));
            }
            bikes(g, c, rng, Vec2::new(fr.u1 - 0.6, fr.v0 + 0.4), Vec2::new(fr.u1 - 0.6, fr.v1 - 0.4), 0.8, true);
        }
    }
}

/// A repair café in an old workshop: corrugated or board walls under a
/// mono-pitch roof with a patched array, the roller door up on a lit
/// workshop (benches, bikes on hooks), its name hand-painted and lit; a
/// container on the yard for the café hatch, tables under bulbs.
pub fn repair_cafe(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    let yard_d = (area.d() * 0.3).clamp(0.0, 7.0);
    let sw = (area.w() - 1.0).min(16.0);
    let sd = (area.d() - yard_d - 0.4).min(14.0);
    let Some(s) = fit(c, R::new(area.c().x - sw * 0.5 + if area.w() > 20.0 { 2.5 } else { 0.0 }, area.c().x + sw * 0.5, area.v0 + yard_d, area.v0 + yard_d + sd), 3.5) else { return shed_home(g, c, rng) };
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.75));
    let h = (c.fh() * 1.6).clamp(4.2, 5.6);
    let corrugated = coin(rng, 0.55);
    let (ws, wall) = if corrugated { (if c.wear > 0.4 { Rust } else { Metal }, mul(pick(rng, &pal.metal_paint), 0.8 - 0.25 * c.wear)) } else { (Wood, timber(c, rng)) };
    block(g, c, ws, &s, 0.0, h, wall);
    let rise = (s.d() * 0.14).clamp(0.8, 2.0);
    let (a, b, n) = mono_pitch(g, c, &s, h, rise, false, 0.3, Metal, mul(pal.steel, 0.55), ws, wall);
    // The array on the roof: rows of frames, odd ones in.
    pv_skin(g, c, rng, a, b, s.w(), n, 0.4);
    let front = s.front();
    // The roller door up on the lit workshop.
    let dw = (front.len * 0.5).clamp(2.0, 5.0);
    let dt = -front.len * 0.5 + dw * 0.5 + 0.6;
    front.glaze(g, c, dt, 1.6, PANE, dw, 3.0, Pane::Lit(mul(INSIDE, 0.85 - 0.3 * c.wear)));
    g.lod(Lod::Mid, |g| {
        front.bx(g, f, Metal, dt, 3.3, 0.18, Vec3::new(dw + 0.3, 0.45, 0.36), mul(pal.steel, 0.6));
        // The bench in the door and a bike on a stand.
        front.bx(g, f, Wood, dt, 0.85, 0.7, Vec3::new(dw * 0.6, 0.08, 0.8), mul(pal.wood, 0.75));
        for sd in [-1.0f32, 1.0] {
            front.bx(g, f, Wood, dt + sd * dw * 0.27, 0.42, 0.7, Vec3::new(0.08, 0.84, 0.7), mul(pal.wood, 0.6));
        }
    });
    let rest = front.len - dw - 1.2;
    if rest > 1.5 {
        let wt = dt + dw * 0.5 + 0.6 + rest * 0.5;
        let fc = frame_col(c, rng);
        window(g, c, rng, &front, wt, 1.6, (rest - 0.8).max(0.6), 1.6, fc, 0.0);
        // Bikes on hooks on the wall by the window.
        let wall_bikes = ((rest - 0.6) / 0.9).floor().clamp(0.0, 4.0) as i32;
        let col = BRIGHT[rng.random_range(0..BRIGHT.len())];
        g.lod(Lod::Near, |g| {
            for i in 0..wall_bikes {
                let t = wt - (rest - 0.6) * 0.5 + 0.45 + i as f32 * 0.9;
                let p = front.at(t, 0.35);
                for y in [3.2f32, 2.3] {
                    g.part(Matte).tube_capped(f.at2(p - front.tangent * 0.02, y), f.at2(p + front.tangent * 0.02, y), 0.3, 8, true, true, rgba(DARK));
                }
                g.part(Matte).beam(f.at2(p, 3.2), f.at2(p, 2.3), 0.05, rgba(mul(col, 1.0 - 0.3 * c.wear)));
            }
        });
    }
    // The name, painted and lit.
    sign_board(g, c, rng, &front, 0.0, h - 0.6, (front.len * 0.6).min(7.0), 0.6);
    for face in [s.left(), s.right(), s.back()] {
        pane_row(g, c, rng, &face, 0.0, 1, h, 3.2, 1.4, 1.2, true);
        grime(g, c, rng, &face, 0.0, h, 0.4 + 1.6 * c.wear);
        creepers(g, c, rng, &face, 0.3, h, 0.2 + 0.5 * c.wear);
    }
    battery_box(g, c, &s.right(), 0.0, h - 0.1);
    // A container for the café hatch, on the yard's side when there is room.
    let side_room = s.u0 - area.u0;
    if side_room > 3.0 && s.d() > 6.5 {
        let u = s.u0 - 1.5;
        let v = s.c().y;
        if room(c, Vec2::new(u, v - 3.2), 0.2) && room(c, Vec2::new(u, v + 3.2), 0.2) {
            container_unit(g, c, rng, u, v, std::f32::consts::FRAC_PI_2, CONTAINER_20, CONTAINER_W, CONTAINER_H, 0.0, mul(BRIGHT[1 + (c.seed % 3) as usize], 0.8 - 0.25 * c.wear));
        }
    }
    // The yard: tables under bulbs strung to the eave.
    if yard_d > 2.6 {
        let yr = R::new(s.u0 + 0.5, s.u1 - 0.5, area.v0 + 0.6, s.v0 - 0.7);
        let k = ((yr.w() - 1.0) / 2.8).floor() as i32;
        for i in 0..k.clamp(0, 5) {
            let p = Vec2::new(yr.u0 + 1.4 + i as f32 * 2.8, yr.c().y);
            if room(c, p, 1.2) {
                g.lod(Lod::Mid, |g| table(g, f, p, 0.0, mul(pal.wood, 0.75), None));
            }
        }
        for sd in [-1.0f32, 1.0] {
            let pole = Vec2::new(yr.c().x + sd * (yr.w() * 0.5 - 0.2), yr.v0 + 0.2);
            if room(c, pole, 0.2) {
                f.bx(g, Wood, pole.x, 1.5, pole.y, Vec3::new(0.12, 3.0, 0.12), mul(pal.wood, 0.5));
                let eave = front.at(sd * front.len * 0.3, 0.3);
                bulbs(g, c, rng, p3(pole.x, 2.9, pole.y), p3(eave.x, (h - 0.5).min(3.6), eave.y), 0.35, 0.9);
            }
        }
    }
}

// ── small industry ────────────────────────────────────────────────────────────

/// A vertical farm: a block of growing floors glazed in bands of dirty
/// green glass (racks of greens behind, near), magenta grow lights through
/// them after dark; a loading bay, pipes, a cargo bike; on the roof water
/// tanks, wind trees and panels.
pub fn vertical_farm(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    let bw = (area.w() - 3.0).clamp(6.0, 30.0);
    let bd = (area.d() - 5.0).clamp(6.0, 24.0);
    let Some(b) = fit(c, R::new(area.c().x - bw * 0.5, area.c().x + bw * 0.5, area.v0 + 3.0, area.v0 + 3.0 + bd), 5.0) else { return greenhouse_rows(g, c, rng) };
    yard(g, c, 0.3, Concrete, mul(pick(rng, &pal.concrete), 0.75));
    let fh = 3.8;
    let floors = (c.floors as i32).clamp(3, 8);
    let h = floors as f32 * fh;
    let panel = mul(mix([0.70, 0.72, 0.68], PATINA[0], 0.3), 1.0 - 0.3 * c.wear);
    block(g, c, Panel, &b, 0.0, h, panel);
    let green_glass = mix(dirty_glass(c), [0.18, 0.30, 0.20], 0.45);
    let lit = coin(rng, 0.9 - 0.3 * c.wear);
    for face in b.faces() {
        let y0 = if face.is_front { 1 } else { 0 };
        for k in y0..floors {
            let y = k as f32 * fh + fh * 0.55;
            face.decal(g, f, Glass, 0.0, y, PANE, face.len - 0.8, fh * 0.62, green_glass);
            if lit {
                let from = g.part(Glow).positions.len();
                face.decal(g, f, Glow, 0.0, y, PANE + 0.006, face.len - 1.0, fh * 0.5, mul(GROW, 0.6));
                mark_glow(g, from, NIGHT_MARK);
            }
            g.lod(Lod::Near, |g| {
                for j in 0..3 {
                    face.decal(g, f, Matte, 0.0, y - fh * 0.24 + j as f32 * fh * 0.24, PANE + 0.012, face.len - 0.9, 0.05, CHARCOAL);
                }
            });
        }
        grime(g, c, rng, &face, 0.0, h, 0.4 + 1.5 * c.wear);
    }
    // The street floor: the loading bay and a door; the leaf over it.
    let front = b.front();
    let dw = (front.len * 0.35).clamp(2.5, 5.0);
    roller_door(g, c, &front, -front.len * 0.25, dw, 3.2, mul(pick(rng, &pal.metal_paint), 0.75));
    front.decal(g, f, Wood, front.len * 0.15, 1.1, DOOR_D, 1.1, 2.2, frame_col(c, rng));
    sign_board(g, c, rng, &front, front.len * 0.15, fh - 0.4, (front.len * 0.35).min(6.0), 0.6);
    bike(g, c, front.at(front.len * 0.32, 1.4), front.tangent, mul(BRIGHT[0], 0.9), true);
    // Pipes up the flank.
    let flank = b.right();
    g.lod(Lod::Mid, |g| {
        for k in 0..2 {
            let p = flank.at(-flank.len * 0.3 + k as f32 * 0.35, 0.2);
            g.part(Metal).cylinder(f.to_world(p.x, p.y), 0.12, 0.12, 0.0, h + 0.6, 6, rgba(mul(pal.steel, 0.8)));
        }
    });
    parapet(g, c, &b.poly(), h, 0.8, 0.2, Panel, mul(panel, 0.9));
    // The roof.
    let roof = b.inset(0.8);
    let tanks = ((roof.w() / 3.0).floor() as i32).clamp(1, 3);
    for i in 0..tanks {
        let p = Vec2::new(roof.u0 + 1.2 + i as f32 * 2.6, roof.v1 - 1.2);
        if p.x < roof.u1 - 1.0 {
            let col = mul(TANKS[(i as usize + 1) % TANKS.len()], 1.0 - 0.3 * c.wear);
            g.part(Metal).cylinder_capped(f.to_world(p.x, p.y), 1.0, 1.0, h, h + 2.6, 10, false, true, rgba(col));
        }
    }
    if roof.d() > 5.0 {
        solar_canopy(g, c, rng, &R::new(roof.u0, roof.u1, roof.v0, roof.v0 + (roof.d() * 0.45).min(6.0)), h, h + 2.0);
    }
    if roof.w() > 6.0 && roof.d() > 4.0 {
        wind_tree(g, c, rng, Vec2::new(roof.u1 - 1.5, roof.v1 - 1.5), h, 5.5);
    }
}

/// Greenhouse rows: two to five long glasshouses side by side on gravel,
/// their lamps glowing sodium after dark, a board packing shed at the
/// street with crates, compost bays, rain tanks and a wind tree.
pub fn greenhouse_rows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 5.0 || area.d() < 6.0 {
        return shed_home(g, c, rng);
    }
    yard(g, c, 0.3, Dirt, mul(pal.paving, 0.62));
    // The packing shed on the street.
    let shed_d = (area.d() * 0.22).clamp(3.0, 7.0);
    let shed_w = (area.w() * 0.5).clamp(4.0, 16.0);
    let mut v_start = area.v0 + 1.0;
    if let Some(s) = fit(c, R::new(area.u0 + 0.6, area.u0 + 0.6 + shed_w, area.v0 + 0.8, area.v0 + 0.8 + shed_d), 3.0) {
        let wood = timber(c, rng);
        block(g, c, Wood, &s, 0.0, 3.4, wood);
        gabled(g, c, &s, 3.4, (s.d() * 0.3).clamp(0.8, 2.0), true, 0.3, Metal, mul(pal.steel, 0.55), Wood, wood);
        let fr = s.front();
        roller_door(g, c, &fr, -fr.len * 0.2, (fr.len * 0.4).min(3.4), 2.6, mul(pick(rng, &pal.metal_paint), 0.7));
        sign_board(g, c, rng, &fr, fr.len * 0.25, 2.5, (fr.len * 0.35).min(4.0), 0.5);
        crates(g, c, rng, Vec2::new(s.u1 + 1.0, s.c().y), 4);
        v_start = s.v1 + 1.2;
    }
    // The glasshouses, end on to the street.
    let gw = rng.random_range(5.0..7.0f32);
    let gap = 1.6;
    let n = (((area.w() - 1.0 + gap) / (gw + gap)).floor() as i32).clamp(1, 5);
    let total = n as f32 * gw + (n - 1) as f32 * gap;
    let u0 = area.c().x - total * 0.5;
    let len = (area.v1 - v_start - 2.5).min(32.0);
    let mut last = f32::MIN;
    for i in 0..n {
        let u = u0 + i as f32 * (gw + gap);
        let Some(r) = fit(c, R::new(u, u + gw, v_start, v_start + len), 3.0) else { continue };
        if r.u0 < last + 0.8 {
            continue;
        }
        last = r.u1;
        greenhouse(g, c, rng, &r, 0.0, 2.6, SODIUM);
    }
    // At the back: compost bays, tanks, a wind tree.
    let bv = area.v1 - 1.4;
    let compost = Vec2::new(area.u0 + 2.0, bv);
    if room(c, compost, 1.4) {
        g.lod(Lod::Mid, |g| {
            for k in 0..3 {
                let p = compost + Vec2::new(k as f32 * 1.3, 0.0);
                f.bx(g, Wood, p.x, 0.45, p.y, Vec3::new(1.2, 0.9, 1.2), mul(pal.wood, 0.5));
                f.bx(g, Dirt, p.x, 0.75, p.y, Vec3::new(1.0, 0.4, 1.0), [0.20, 0.15, 0.10]);
            }
        });
    }
    rain_tank(g, c, rng, Vec2::new(area.u1 - 1.4, bv), 0.9, 2.2, None);
    let wt = Vec2::new(area.c().x, bv);
    if room(c, wt, 2.4) {
        wind_tree(g, c, rng, wt, 0.0, 6.5);
    }
}

/// A cargo-bike depot: a long open shed whose roof is a solar canopy on
/// steel posts, glowing under after dark, rows of cargo bikes on charge
/// (each post's status light green), a timber back wall, a container
/// office with a lit window, tanks, pallets and wind trees in the yard.
pub fn bike_depot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let area = R::of(&c.poly);
    if area.w() < 6.0 || area.d() < 7.0 {
        return greenhouse_rows(g, c, rng);
    }
    yard(g, c, 0.3, Concrete, mul(pick(rng, &pal.concrete), 0.72));
    let sd = (area.d() * 0.4).clamp(5.0, 10.0);
    let sw = (area.w() - 1.2).min(40.0);
    let Some(s) = fit(c, R::new(area.c().x - sw * 0.5, area.c().x + sw * 0.5, area.v1 - sd - 0.6, area.v1 - 0.6), 4.0) else { return greenhouse_rows(g, c, rng) };
    // The back wall of boards.
    let wood = timber(c, rng);
    s.back().bx(g, f, Wood, 0.0, 1.6, -0.15, Vec3::new(s.w(), 3.2, 0.3), wood);
    creepers(g, c, rng, &R::new(s.u0, s.u1, s.v0, s.v1 - 0.3).back().reversed(), 0.3, 3.2, 0.3);
    solar_canopy(g, c, rng, &s, 0.0, 3.4);
    // Bikes in rows under it, nose to the back wall; a post with a light per bay.
    let bays = ((s.w() - 1.0) / 1.4).floor() as i32;
    let mut cols = Vec::new();
    for _ in 0..bays.clamp(0, 16) {
        cols.push((coin(rng, 0.7), mul(BRIGHT[rng.random_range(0..BRIGHT.len())], 1.0 - 0.3 * c.wear)));
    }
    for (i, (here, col)) in cols.into_iter().enumerate() {
        let u = s.u0 + 1.0 + i as f32 * 1.4;
        if here && i < 8 {
            bike(g, c, Vec2::new(u, s.v1 - 2.0), Vec2::new(0.0, 1.0), col, true);
        }
        if i % 2 == 0 {
            let p = Vec2::new(u, s.v1 - 0.6);
            f.bx(g, Panel, p.x, 0.6, p.y, Vec3::new(0.3, 1.2, 0.25), [0.74, 0.76, 0.72]);
            let from = g.part(Glow).positions.len();
            f.bx(g, Glow, p.x, 1.05, p.y - 0.14, Vec3::new(0.12, 0.08, 0.04), [0.4, 1.9, 0.6]);
            mark_glow(g, from, NIGHT_MARK);
        }
    }
    // The container office at the front corner.
    let ou = area.u0 + 1.8;
    let ov = area.v0 + 4.2;
    if area.w() > 10.0 && s.v0 - area.v0 > 6.0 && room(c, Vec2::new(ou, ov - 3.1), 0.2) && room(c, Vec2::new(ou, ov + 3.1), 0.2) {
        container_unit(g, c, rng, ou, ov, std::f32::consts::FRAC_PI_2, CONTAINER_20, CONTAINER_W, CONTAINER_H, 0.0, mul(PATINA[0], 0.8 - 0.25 * c.wear));
        let face = Face::new(Vec2::new(ou + CONTAINER_W * 0.5, ov - 1.0), Vec2::new(ou + CONTAINER_W * 0.5, ov + 1.0), Vec2::X);
        face.glaze(g, c, 0.0, 1.5, 0.08, 1.4, 0.9, Pane::Lit(mul(INSIDE, 0.8)));
        planted_roof(g, c, rng, &R::new(ou - CONTAINER_W * 0.5 + 0.1, ou + CONTAINER_W * 0.5 - 0.1, ov - 2.9, ov + 2.9), CONTAINER_H + 0.02);
    }
    // The yard: wind trees, tanks, pallets of crates.
    let mid_v = (area.v0 + s.v0) * 0.5;
    let trees = ((area.w() / 14.0).floor() as i32).clamp(1, 3);
    for i in 0..trees {
        let p = Vec2::new(area.u0 + (i as f32 + 0.5) * area.w() / trees as f32 + 2.0, mid_v);
        if room(c, p, 2.4) {
            let h = rng.random_range(5.5..7.5);
            wind_tree(g, c, rng, p, 0.0, h);
        }
    }
    rain_tank(g, c, rng, Vec2::new(s.u1 - 1.0, s.v0 - 1.2), 0.8, 2.0, None);
    crates(g, c, rng, Vec2::new(s.u1 - 3.2, s.v0 - 1.0), 4);
    post_lamp(g, c, Vec2::new(area.c().x, area.v0 + 0.9), 1.0);
}

// ── the big house ─────────────────────────────────────────────────────────────

/// A family's or a billionaire's planted solar villa: a long low house of
/// timber and glass, its roof a garden (beds, shrubs, a pergola terrace
/// under bulbs), an upper storey stepped back under a solar canopy that
/// glows after dark; a natural swimming pond, an orchard and a meadow,
/// veg beds, a wind tree, rain tanks; a gabion wall to the street with a
/// timber gate and lamps. On an estate's four plots a second wing makes an
/// L, with a greenhouse and an upper storey of its own.
pub fn solar_villa(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(site) = fit(c, R::of(&offset_poly(&c.lot_poly, 0.3)), 12.0).map(|r| r.inset(1.0)) else { return timber_terrace(g, c, rng) };
    let big = site.w() * site.d() > 1300.0;
    let fh = 3.3;
    let wood = timber(c, rng);
    let fc = frame_col(c, rng);
    // The meadow over the lot, a mown lawn round the house.
    ground(g, f, Foliage, &offset_poly(&c.lot_poly, 0.4), 0.03, leaf(c, 0.75));
    let hw = (site.w() * 0.62).min(if big { 30.0 } else { 22.0 });
    let hd = (site.d() * 0.32).clamp(8.0, if big { 14.0 } else { 11.0 });
    let v0 = site.v0 + (site.d() * 0.2).clamp(4.0, 9.0);
    let house = R::new(site.c().x - hw * 0.5, site.c().x + hw * 0.5, v0, v0 + hd);
    if house.v1 > site.v1 - 5.0 {
        return timber_terrace(g, c, rng);
    }
    let lower = fh;
    block(g, c, Wood, &house, 0.0, lower, wood);
    // Glass along the garden and the street, timber between.
    for face in c.faces(&house.poly()) {
        let n = ((face.len - 1.0) / 2.6).floor() as i32;
        for i in 0..n.max(0) {
            let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * (face.len - 1.0) / n as f32;
            if face.is_front && t.abs() < 1.3 {
                continue;
            }
            window(g, c, rng, &face, t, lower * 0.5, 1.9, lower * 0.72, fc, 0.0);
        }
        creepers(g, c, rng, &face, 0.3, lower, 0.25 + 0.3 * c.wear);
    }
    let front = house.front();
    front.decal(g, f, Wood, 0.0, 1.15, DOOR_D, 1.6, 2.3, mul(fc, 1.2));
    // The upper storey, stepped back, its own canopy over it.
    let up_w = (house.w() * 0.5).max(6.0).min(house.w() - 2.0);
    let upper = R::new(house.u1 - up_w - 0.8, house.u1 - 0.8, house.v0 + 1.2, house.v1 - 0.8);
    let storeys_up = if big { 2 } else { 1 };
    let top = lower + storeys_up as f32 * fh;
    let render_col = render(c, rng);
    block(g, c, Plaster, &upper, lower, top, render_col);
    for face in upper.faces() {
        let n = ((face.len - 1.0) / 2.4).floor() as i32;
        for k in 0..storeys_up {
            for i in 0..n.max(0) {
                let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * (face.len - 1.0) / n as f32;
                window(g, c, rng, &face, t, lower + k as f32 * fh + fh * 0.5, 1.6, fh * 0.7, fc, 0.2);
            }
        }
        grime(g, c, rng, &face, lower, top, 0.2 + 1.0 * c.wear);
    }
    solar_canopy(g, c, rng, &upper.inset(0.3), top, top + 1.2);
    // The roof garden over the rest of the lower storey: planted, a
    // pergola terrace under bulbs.
    let garden = R::new(house.u0, upper.u0 - 0.05, house.v0, house.v1);
    if garden.w() > 2.0 {
        planted_roof(g, c, rng, &garden, lower);
        let pg = garden.inset(1.0);
        if pg.w() > 2.0 && pg.d() > 2.0 {
            let posts = [Vec2::new(pg.u0, pg.v0), Vec2::new(pg.u1, pg.v0), Vec2::new(pg.u1, pg.v1), Vec2::new(pg.u0, pg.v1)];
            for p in posts {
                f.bx(g, Wood, p.x, lower + 1.25, p.y, Vec3::new(0.14, 2.3, 0.14), mul(pal.wood, 0.55));
            }
            g.lod(Lod::Mid, |g| {
                let k = ((pg.w() / 0.8).floor() as i32).max(1);
                for i in 0..=k {
                    let u = pg.u0 + i as f32 * pg.w() / k as f32;
                    f.bx(g, Wood, u, lower + 2.45, pg.c().y, Vec3::new(0.08, 0.12, pg.d() + 0.4), mul(pal.wood, 0.6));
                }
            });
            bulbs(g, c, rng, p3(pg.u0, lower + 2.3, pg.v0), p3(pg.u1, lower + 2.3, pg.v0), 0.3, 0.9);
            bulbs(g, c, rng, p3(pg.u0, lower + 2.3, pg.v1), p3(pg.u1, lower + 2.3, pg.v1), 0.3, 0.9);
        }
    }
    // The estate's wing: back towards the garden, a greenhouse at its end.
    if big {
        let wing = R::new(house.u0, house.u0 + (house.w() * 0.3).clamp(6.0, 10.0), house.v1, (house.v1 + site.d() * 0.3).min(site.v1 - 8.0));
        if wing.d() > 4.0 {
            block(g, c, Wood, &wing, 0.0, lower, wood);
            for face in [wing.left(), wing.right()] {
                pane_row(g, c, rng, &face, 0.0, 1, fh, 2.6, 1.8, fh * 0.65, true);
            }
            planted_roof(g, c, rng, &wing, lower);
            let gh = R::new(wing.u0, wing.u1, wing.v1 + 0.8, wing.v1 + 6.0);
            if let Some(gh) = fit(c, gh, 3.0) {
                greenhouse(g, c, rng, &gh, 0.0, 2.6, SODIUM);
            }
        }
    }
    // The pond behind the house, reeds round it.
    let pond_v = house.v1 + 4.5;
    let pond_w = (site.w() * 0.3).clamp(5.0, 12.0);
    let pu = site.c().x + site.w() * 0.15;
    if pond_v + 2.5 < site.v1 - 2.0 {
        pool(g, c, pu, pond_v, pond_w, 4.0);
        let reed = leaf(c, 0.6);
        g.lod(Lod::Mid, |g| {
            for s in [-1.0f32, 1.0] {
                f.bx(g, Foliage, pu + s * (pond_w * 0.5 + 0.35), 0.5, pond_v, Vec3::new(0.5, 0.8, 3.0), reed);
            }
        });
    }
    // Veg beds, the orchard, a wind tree, tanks.
    for i in 0..3 {
        veg_bed(g, c, rng, Vec2::new(site.u0 + 2.0, house.v1 + 2.0 + i as f32 * 1.8), 2.4, 1.1);
    }
    let trees = if big { 6 } else { 3 };
    for i in 0..trees {
        let p = Vec2::new(site.u1 - 2.0 - (i % 3) as f32 * 4.0, site.v1 - 2.5 - (i / 3) as f32 * 4.0);
        if room(c, p, 2.5) && p.y > house.v1 + 1.5 {
            tree_kind(g, f.to_world(p.x, p.y), 0.0, TreeKind::Round, rng.random_range(2.2..3.4), 2.6, pal.bark, leaf(c, 1.0));
        }
    }
    let wt = Vec2::new(site.u0 + 2.2, site.v0 + 2.6);
    if room(c, wt, 2.3) {
        wind_tree(g, c, rng, wt, 0.0, 6.5);
    }
    rain_tank(g, c, rng, Vec2::new(house.u1 + 1.0, house.v1 - 1.0), 0.8, 2.2, Some(f.at(house.u1 + 1.0, lower - 0.1, house.v1 - 0.2)));
    // The gabion wall to the street, a gate, lamps.
    let wall_v = site.v0 + 0.3;
    let gw = 3.6;
    let stone = mul(pick(rng, &pal.concrete), 0.85);
    for (u0, u1) in [(site.u0, site.c().x - gw * 0.5), (site.c().x + gw * 0.5, site.u1)] {
        if u1 - u0 > 0.8 {
            f.bx(g, Stone, (u0 + u1) * 0.5, 0.5, wall_v, Vec3::new(u1 - u0, 1.0, 0.5), stone);
            g.lod(Lod::Mid, |g| f.bx(g, Foliage, (u0 + u1) * 0.5, 1.2, wall_v + 0.05, Vec3::new(u1 - u0 - 0.2, 0.42, 0.42), leaf(c, 0.8)));
        }
    }
    ground(g, f, Paving, &R::new(site.c().x - gw * 0.5 + 0.4, site.c().x + gw * 0.5 - 0.4, wall_v + 0.3, house.v0 - 0.1).poly(), PATH_Y, mul(pal.paving, 0.95));
    for s in [-1.0f32, 1.0] {
        post_lamp(g, c, Vec2::new(site.c().x + s * (gw * 0.5 - 0.1), wall_v + 0.7), 1.1);
    }
}

/// [r4-solarpunk] The style a trade builds in a Solarpunk Green district,
/// where the look has one for it: a vertical farm is a vertical farm, not
/// a farmstead; a bike shop is the repair café.
pub fn trade_style(kind: Option<nd_sim::BusinessKind>) -> Option<super::Style> {
    use super::Style;
    use nd_sim::BusinessKind as B;
    match kind? {
        B::VertFarm => Some(Style::VerticalFarm),
        B::BikeShop => Some(Style::RepairCafe),
        _ => None,
    }
}
