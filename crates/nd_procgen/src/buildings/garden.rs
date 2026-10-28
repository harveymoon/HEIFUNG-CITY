//! [garden] The Garden Suburb look (the Quiet Acres story): detached
//! houses of many plans and roofs in their own gardens behind picket
//! fences, hedges and low walls, with drives, garages and sheds; one-storey
//! bungalows with verandas and carports; a close of houses round a turning
//! circle (the cul-de-sac); a small shopping parade with flats over it, a
//! garden centre with its glasshouse and polytunnels, a corner café with
//! tables out; and the big garden house behind tall hedges that a family
//! (`estate::Form::McMansion`) or a billionaire (`Form::Mansion`) builds
//! here.
//!
//! Lived in rather than new: paint fades and pickets go missing with wear,
//! lawns dry out on the cheap land, bins wait by the drive, a bike lies on
//! the grass. After dark the streets stay dark: no lamp posts on the lots,
//! only a lantern by each door (lit after dusk, [`NIGHT_MARK`]) and the
//! windows.
//!
//! Lot-local as every style: `u` along the street, `v` in from it. A home
//! is laid out on a [`Plot`]: a rectangle and the side of it that faces its
//! street (the houses round a close face its lane, sideways), so one house
//! builder serves the street and the close. Small parts are detail
//! ([`Lod::Mid`], [`Lod::Near`]) drawn from a stream of their own
//! ([`stream`]), so the masses come from the building's main stream alone
//! and the far city pays only for walls, roofs, windows and hedges.

use super::kit::*;
use super::parts::*;
use super::{Ctx, Extras, Face};
use crate::hash::hash2_salted;
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::Tier;
use rand::Rng;
use std::f32::consts::FRAC_PI_2;

/// Salt of the detail stream.
const DETAIL_STREAM: u64 = 0x6A2D_0E;
/// Salt of the per-picket hash (which ones are missing).
const PICKET_SALT: u64 = 0x6A_91C4;

// Ground layers over the lawn (`yard`, 0.03): each its own height, so two
// that overlap never share a plane.
const BED_Y: f32 = 0.042;
const LANE_Y: f32 = 0.048;
const DRIVE_Y: f32 = 0.056;
const PATH_Y: f32 = 0.067;
const PATIO_Y: f32 = 0.079;
const LINE_Y: f32 = 0.061;
/// Facade depths (see `crate::depth`): a garage door over its frame
/// (0.03); a front door and its frame proud of the plinth a house stands on
/// (0.05, a manor's 0.06); glazing bars just proud of the panes (0.05).
const DOOR_D: f32 = 0.04;
const FRONT_FRAME_D: f32 = 0.072;
const FRONT_DOOR_D: f32 = 0.081;
const BAR_D: f32 = 0.064;
/// A house's storey.
const FH: f32 = 2.8;

/// The building's detail stream.
fn stream(c: &Ctx) -> nd_sim::SimRng {
    nd_sim::rng_for(c.seed, DETAIL_STREAM)
}

fn unit(h: u64, shift: u32) -> f32 {
    ((h >> shift) & 0xFFFF) as f32 / 65_536.0
}

// ── colours ──────────────────────────────────────────────────────────────────

/// Render: cream, magnolia, pale sage, blue-grey, buff.
const RENDERS: [[f32; 3]; 5] = [[0.84, 0.80, 0.70], [0.80, 0.78, 0.72], [0.70, 0.74, 0.66], [0.72, 0.74, 0.76], [0.80, 0.70, 0.56]];
/// Painted weatherboard: white, pale blue, sage, butter, grey.
const BOARDS: [[f32; 3]; 5] = [[0.86, 0.86, 0.82], [0.62, 0.72, 0.78], [0.62, 0.70, 0.60], [0.86, 0.80, 0.56], [0.56, 0.58, 0.58]];
/// Clay tiles and slates.
const TILES: [[f32; 3]; 4] = [[0.46, 0.22, 0.16], [0.38, 0.20, 0.15], [0.52, 0.31, 0.21], [0.34, 0.18, 0.14]];
const SLATES: [[f32; 3]; 3] = [[0.22, 0.23, 0.26], [0.29, 0.29, 0.31], [0.19, 0.20, 0.21]];
/// Front doors: red, navy, bottle green, black, yellow, teal, oak.
const DOORS: [[f32; 3]; 7] = [[0.55, 0.10, 0.10], [0.12, 0.16, 0.32], [0.12, 0.30, 0.18], [0.08, 0.08, 0.09], [0.80, 0.62, 0.16], [0.16, 0.42, 0.44], [0.42, 0.28, 0.16]];
const WHITE: [f32; 3] = [0.88, 0.87, 0.83];
const GARAGE_DOORS: [[f32; 3]; 4] = [[0.84, 0.83, 0.78], [0.40, 0.30, 0.22], [0.30, 0.34, 0.30], [0.62, 0.62, 0.60]];
/// Wheelie bins: green, black, blue, brown.
const BINS: [[f32; 3]; 4] = [[0.14, 0.32, 0.16], [0.08, 0.08, 0.09], [0.12, 0.22, 0.46], [0.34, 0.22, 0.12]];
/// Flowers in the beds.
const FLOWERS: [[f32; 3]; 5] = [[0.80, 0.20, 0.22], [0.92, 0.78, 0.22], [0.70, 0.40, 0.75], [0.92, 0.92, 0.88], [0.92, 0.52, 0.30]];
/// Shop fascias: green, maroon, navy, cream, black.
const FASCIAS: [[f32; 3]; 5] = [[0.14, 0.30, 0.20], [0.40, 0.10, 0.12], [0.12, 0.16, 0.30], [0.82, 0.78, 0.66], [0.08, 0.08, 0.09]];

/// One home's colours and materials.
#[derive(Clone, Copy)]
struct Paint {
    wall: Surface,
    wall_c: [f32; 3],
    roof: Surface,
    roof_c: [f32; 3],
    trim: [f32; 3],
    door: [f32; 3],
    fence: [f32; 3],
    lawn: [f32; 3],
    hedge: [f32; 3],
    leaf: [f32; 3],
    /// Chimneys, plinths and garden walls.
    brick: [f32; 3],
}

fn paint(c: &Ctx, rng: &mut impl Rng) -> Paint {
    let pal = c.palette;
    // Paint fades and goes grey with wear; brick and tile only darken.
    let fade = |col: [f32; 3]| mix(col, [0.55, 0.55, 0.53], 0.35 * c.wear);
    let brick = mul(pick(rng, &pal.brick), 1.0 - 0.15 * c.wear);
    let (wall, wall_c) = match rng.random_range(0..10) {
        0..=3 => (Brick, brick),
        4..=6 => (Plaster, fade(RENDERS[rng.random_range(0..RENDERS.len())])),
        7..=8 => (Wood, fade(BOARDS[rng.random_range(0..BOARDS.len())])),
        // Pebbledash.
        _ => (Concrete, fade(mix(RENDERS[4], [0.70, 0.68, 0.62], 0.5))),
    };
    let (roof, roof_c) = if rng.random_bool(0.6) { (Brick, TILES[rng.random_range(0..TILES.len())]) } else { (Panel, SLATES[rng.random_range(0..SLATES.len())]) };
    let trim = if rng.random_bool(0.8) { fade(WHITE) } else { fade([0.30, 0.28, 0.25]) };
    let door = fade(DOORS[rng.random_range(0..DOORS.len())]);
    let fence = if rng.random_bool(0.6) { fade(WHITE) } else { mul(pal.wood, rng.random_range(0.8..1.05)) };
    let foliage = pal.foliage;
    Paint {
        wall,
        wall_c,
        roof,
        roof_c: mul(roof_c, 1.0 - 0.12 * c.wear),
        trim,
        door,
        fence,
        lawn: lawn(c),
        hedge: mul(mix(foliage, [0.16, 0.30, 0.12], 0.5), 0.9),
        leaf: mix(foliage, [0.24, 0.40, 0.18], 0.4),
        brick,
    }
}

/// The lawn: greener on the better land, dried out on the cheap and the
/// neglected.
fn lawn(c: &Ctx) -> [f32; 3] {
    let dry = if c.tier == Tier::Low { 0.45 } else { 0.1 } + 0.4 * c.wear;
    mix(mix(c.palette.foliage, [0.26, 0.42, 0.18], 0.5), c.palette.foliage_dry, dry.min(0.8))
}

/// Where along `face` (from its middle) the lot-local point `p` lies.
fn face_t(face: &Face, p: Vec2) -> f32 {
    (p - face.mid()).dot(face.tangent)
}

// ── plots ────────────────────────────────────────────────────────────────────

/// A home's plot: a rectangle of the lot frame and the side of it that
/// faces its street (the index into [`R::faces`]: 0 the street at -v, 1 +u,
/// 2 +v, 3 -u). Positions on it are `s` along its front from the middle
/// and `k` in from the front.
#[derive(Clone, Copy, Debug)]
struct Plot {
    r: R,
    side: usize,
}

impl Plot {
    fn front(&self) -> Face {
        self.r.faces()[self.side]
    }
    fn width(&self) -> f32 {
        if self.side % 2 == 0 { self.r.w() } else { self.r.d() }
    }
    fn depth(&self) -> f32 {
        if self.side % 2 == 0 { self.r.d() } else { self.r.w() }
    }
    /// Lot-local point `s` along the front and `k` in from it.
    fn at(&self, s: f32, k: f32) -> Vec2 {
        self.front().at(s, -k)
    }
    /// The rectangle `s0..s1` along, `k0..k1` in.
    fn rect(&self, s0: f32, s1: f32, k0: f32, k1: f32) -> R {
        let (a, b) = (self.at(s0, k0), self.at(s1, k1));
        R::new(a.x, b.x, a.y, b.y)
    }
    /// A lot-local point in plot terms `(s, k)`.
    fn local(&self, p: Vec2) -> (f32, f32) {
        let f = self.front();
        ((p - f.mid()).dot(f.tangent), -(p - f.mid()).dot(f.out))
    }
    /// `(s0, s1, k0, k1)` of a rectangle of the frame.
    fn span(&self, r: &R) -> (f32, f32, f32, f32) {
        let pts = r.poly();
        let (mut s0, mut s1, mut k0, mut k1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in pts {
            let (s, k) = self.local(p);
            s0 = s0.min(s);
            s1 = s1.max(s);
            k0 = k0.min(k);
            k1 = k1.max(k);
        }
        (s0, s1, k0, k1)
    }
    /// The face of `r` turned `k` quarter turns from the plot's front
    /// (0 front, 1 and 3 the sides, 2 the back).
    fn face_of(&self, r: &R, k: usize) -> Face {
        r.faces()[(self.side + k) % 4]
    }
    /// The plot's front direction as a yaw in the lot frame (a car parked
    /// nose to the street).
    fn yaw_in(&self) -> f32 {
        let d = -self.front().out;
        d.y.atan2(d.x)
    }
}

/// Whether a rectangle stands wholly on the lot.
fn on_lot(c: &Ctx, r: &R) -> bool {
    poly_inside(c, &r.poly())
}

// ── roofs ────────────────────────────────────────────────────────────────────

/// The four corners of a rectangle about `cen`, `hl` along `ax` and `hs`
/// across it, counter-clockwise.
fn quad(cen: Vec2, ax: Vec2, hl: f32, hs: f32) -> Vec<Vec2> {
    let sx = ax.perp();
    vec![cen - ax * hl - sx * hs, cen + ax * hl - sx * hs, cen + ax * hl + sx * hs, cen - ax * hl + sx * hs]
}

/// A pitched roof over `r` with its ridge along `u` (`along_u`) or `v`:
/// eaves at `y`, the ridge `rise` over them, the slopes out past the walls.
/// Gabled: two slopes over gable ends in the wall's colour; hipped: a solid
/// from the eaves to a short ridge. Returns the ridge's height.
#[allow(clippy::too_many_arguments)]
fn pitched(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, along_u: bool, hip: bool, wall: (Surface, [f32; 3]), roof: (Surface, [f32; 3])) -> f32 {
    let f = &c.frame;
    let cen = r.c();
    let ax = if along_u { Vec2::X } else { Vec2::Y };
    let sx = ax.perp();
    let (l, s) = if along_u { (r.w() * 0.5, r.d() * 0.5) } else { (r.d() * 0.5, r.w() * 0.5) };
    let over = 0.4;
    let slope = rise / s.max(0.5);
    if hip {
        let ridge = (l - s).max(0.25);
        loft(g, f, roof.0, &quad(cen, ax, l + over, s + over), &quad(cen, ax, ridge, 0.06), y - over * slope, y + rise, roof.1);
    } else {
        for e in [-1.0f32, 1.0] {
            let end = cen + ax * l * e;
            let (a, b, top) = (f.at2(end - sx * s, y), f.at2(end + sx * s, y), f.at2(end, y + rise));
            g.part(wall.0).tri_facing(a, b, top, f.dir3(ax * e), rgba(wall.1));
        }
        for side in [-1.0f32, 1.0] {
            let eave = f.at2(cen + sx * side * (s + over), y - over * slope);
            g.part(roof.0).slab(eave, f.at2(cen, y + rise), 2.0 * (l + 0.3), 0.12, rgba(roof.1));
        }
        // Ridge tiles.
        let yaw = if along_u { 0.0 } else { FRAC_PI_2 };
        f.bx_yaw(g, roof.0, cen.x, y + rise + 0.03, cen.y, Vec3::new(2.0 * (l + 0.3), 0.12, 0.24), mul(roof.1, 0.8), yaw);
    }
    y + rise
}

/// A chimney stack on the lot at `p` from `y0` to `y1`, with its pots.
fn chimney(g: &mut Geometry, c: &Ctx, p: Vec2, y0: f32, y1: f32, w: f32, brick: [f32; 3]) {
    let f = &c.frame;
    f.bx(g, Brick, p.x, (y0 + y1) * 0.5, p.y, Vec3::new(w, y1 - y0, 0.6), brick);
    f.bx(g, Concrete, p.x, y1 + 0.05, p.y, Vec3::new(w + 0.12, 0.1, 0.72), mul(brick, 0.7));
    g.lod(Lod::Near, |g| {
        let n = if w > 1.0 { 2 } else { 1 };
        for k in 0..n {
            let q = p + Vec2::X * ((k as f32 - (n - 1) as f32 * 0.5) * 0.4);
            g.part(Concrete).cylinder(f.to_world(q.x, q.y), 0.12, 0.1, y1 + 0.1, y1 + 0.55, 6, rgba([0.52, 0.32, 0.24]));
        }
    });
}

// ── facades ──────────────────────────────────────────────────────────────────

/// A cottage window on a face: the pane (lit or dark, the base mesh), and
/// near the camera its frame, glazing bars and sill.
#[allow(clippy::too_many_arguments)]
fn window(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32, trim: [f32; 3]) {
    let lit = coin(rng, c.lit_chance() * 0.85);
    let warm = mul(c.palette.window_warm, rng.random_range(0.6..0.95));
    face.glaze(g, c, t, y, crate::depth::PANE, w, h, if lit { Pane::Lit(warm) } else { Pane::Dark });
    let f = &c.frame;
    g.lod(Lod::Near, |g| {
        face.window_frame(g, c, t, y, w, h, 0.16, trim);
        face.decal(g, f, Matte, t, y, BAR_D, 0.05, h, trim);
        face.decal(g, f, Matte, t, y + h * 0.12, BAR_D + 0.004, w, 0.05, trim);
        face.bx(g, f, Concrete, t, y - h * 0.5 - 0.08, 0.06, Vec3::new(w + 0.2, 0.08, 0.14), mul(trim, 0.9));
    });
}

/// Windows along a face, `storeys` rows, leaving out `skip` (a span along
/// the face) on the ground floor.
#[allow(clippy::too_many_arguments)]
fn window_rows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, storeys: u8, y0: f32, skip: Option<(f32, f32)>, trim: [f32; 3]) {
    let n = ((face.len - 1.0) / 2.7).floor() as i32;
    if n < 1 {
        return;
    }
    let step = (face.len - 1.0) / n as f32;
    let w = (step * 0.55).clamp(0.6, 1.3);
    for k in 0..storeys as i32 {
        let y = y0 + k as f32 * FH + FH * 0.55;
        for i in 0..n {
            let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
            if k == 0 && skip.is_some_and(|(a, b)| t > a - w * 0.5 - 0.3 && t < b + w * 0.5 + 0.3) {
                continue;
            }
            window(g, c, rng, face, t, y, w, 1.25, trim);
        }
    }
}

/// A front door at `t` on `face` with its frame, step, hood and a lantern
/// beside it that lights after dark.
fn front_door(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, p: &Paint, hood: bool) {
    let f = &c.frame;
    face.decal(g, f, Matte, t, 1.12, FRONT_FRAME_D, 1.24, 2.26, p.trim);
    face.decal(g, f, Wood, t, 1.05, FRONT_DOOR_D, 0.98, 2.08, p.door);
    if c.reach_out(face) > 1.2 {
        face.bx(g, f, Concrete, t, 0.09, 0.35, Vec3::new(1.5, 0.18, 0.7), mul(c.palette.paving, 1.1));
    }
    if hood && c.reach_out(face) > 1.4 {
        // A little pitched hood on brackets.
        face.bx(g, f, Panel, t, 2.55, 0.5, Vec3::new(1.8, 0.1, 1.0), mul(p.roof_c, 0.95));
        g.lod(Lod::Mid, |g| {
            for s in [-0.75f32, 0.75] {
                face.bx(g, f, Wood, t + s, 2.3, 0.4, Vec3::new(0.08, 0.4, 0.7), p.trim);
            }
        });
    }
    porch_light(g, c, face, t + 0.95, 2.05);
}

/// A carriage lantern on a wall: dark by day; after dark (`NIGHT_MARK`) its
/// glass glows warm round the dark body.
fn porch_light(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, y, 0.16, Vec3::new(0.14, 0.24, 0.14), CHARCOAL);
    face.bx(g, f, Matte, t, y + 0.16, 0.16, Vec3::new(0.22, 0.06, 0.22), CHARCOAL);
    let from = g.get(Glow).colors.len();
    face.bx(g, f, Glow, t, y - 0.01, 0.16, Vec3::new(0.18, 0.2, 0.18), mul(c.palette.window_warm, 1.3));
    mark_glow(g, from, NIGHT_MARK);
}

// ── houses ───────────────────────────────────────────────────────────────────

/// How a house is roofed: ridge along the front (gabled or hipped) or
/// across it, its gable to the street.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RoofKind {
    Gable,
    Hip,
    FrontGable,
}

/// A house on `plot`: walls on `body` (`storeys` high, rooms in the roof
/// when `attic`), its roof, a front wing when given, windows round it, the
/// front door at `door_s` along the front, a chimney, gutters and an aerial
/// or a dish. Returns the point in front of the door.
#[allow(clippy::too_many_arguments)]
fn house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, d: &mut impl Rng, plot: &Plot, body: &R, storeys: u8, attic: bool, roof: RoofKind, wing: Option<R>, door_s: f32, p: &Paint) -> Vec2 {
    let f = &c.frame;
    let top = storeys as f32 * FH;
    let along_front = plot.side % 2 == 0;
    // A brick plinth just proud of the walls.
    extrude(g, f, Brick, &body.inset(-0.05).poly(), 0.0, 0.45, mul(p.brick, 0.85));
    extrude(g, f, p.wall, &body.poly(), 0.0, top, p.wall_c);
    let (s0, s1, k0, k1) = plot.span(body);
    let (bw, bd) = (s1 - s0, k1 - k0);
    let span = if roof == RoofKind::FrontGable { bw } else { bd } * 0.5;
    let pitch = if attic { rng.random_range(0.9..1.05) } else if roof == RoofKind::Hip { rng.random_range(0.5..0.65) } else { rng.random_range(0.6..0.8) };
    let rise = (span * pitch).clamp(1.4, 5.5);
    let ridge_along_u = if roof == RoofKind::FrontGable { !along_front } else { along_front };
    let wall = (p.wall, p.wall_c);
    let roof_m = (p.roof, p.roof_c);
    let ridge = pitched(g, c, body, top, rise, ridge_along_u, roof == RoofKind::Hip, wall, roof_m);
    // The wing: a gable to the street over a bay that stands out of the
    // front, its roof running back into the main one (its back gable buried
    // under the main ridge).
    let front = plot.face_of(body, 0);
    let mut wing_t = None;
    if let Some(w) = wing {
        let (ws0, ws1, wk0, _) = plot.span(&w);
        extrude(g, f, Brick, &w.inset(-0.05).poly(), 0.0, 0.45, mul(p.brick, 0.85));
        extrude(g, f, p.wall, &w.poly(), 0.0, top, p.wall_c);
        let wrise = ((ws1 - ws0) * 0.5 * 0.85).min(ridge - top - 0.3).max(0.8);
        pitched(g, c, &w, top, wrise, !along_front, false, wall, roof_m);
        window_rows(g, c, rng, &plot.face_of(&w, 0), storeys, 0.0, None, p.trim);
        for (k, ss) in [(1usize, ws1), (3, ws0)] {
            let side = plot.face_of(&w, k);
            if k0 - wk0 > 1.6 {
                let t = face_t(&side, plot.at(ss, (wk0 + k0) * 0.5));
                window(g, c, rng, &side, t, FH * 0.55, 0.6, 1.1, p.trim);
            }
        }
        let (ta, tb) = (face_t(&front, plot.at(ws0, k0)), face_t(&front, plot.at(ws1, k0)));
        wing_t = Some((ta.min(tb), ta.max(tb)));
    }
    // Windows round the body; the front leaves the door and the wing.
    let door_t = face_t(&front, plot.at(door_s, k0));
    let n = ((front.len - 1.0) / 2.7).floor() as i32;
    if n >= 1 {
        let step = (front.len - 1.0) / n as f32;
        let w = (step * 0.55).clamp(0.6, 1.3);
        for k in 0..storeys as i32 {
            let y = k as f32 * FH + FH * 0.55;
            for i in 0..n {
                let t = -(front.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
                if wing_t.is_some_and(|(a, b)| t > a - w * 0.5 - 0.2 && t < b + w * 0.5 + 0.2) {
                    continue;
                }
                if k == 0 && (t - door_t).abs() < w * 0.5 + 1.0 {
                    continue;
                }
                window(g, c, rng, &front, t, y, w, 1.25, p.trim);
            }
        }
    }
    for k in 1..4 {
        window_rows(g, c, rng, &plot.face_of(body, k), storeys, 0.0, None, p.trim);
    }
    front_door(g, c, &front, door_t, p, true);
    // Rooms in the roof: dormers on the front slope, or a gable window.
    if attic && rise > 2.0 {
        if roof == RoofKind::FrontGable {
            window(g, c, rng, &front, 0.0, top + rise * 0.4, 0.8, 1.0, p.trim);
        } else {
            let nd = if bw > 9.0 && wing.is_none() { 2 } else { 1 };
            for i in 0..nd {
                let s = s0 + bw * (i as f32 + 1.0) / (nd + 1) as f32;
                if wing_t.is_none() || (s - door_s).abs() < bw * 0.3 || nd == 1 {
                    dormer(g, c, rng, plot, s, k0, top, rise, span, p);
                }
            }
        }
    }
    // A chimney: up a gable end, or through the roof near the ridge.
    let (cs, ck) = match roof {
        RoofKind::Hip => ((s0 + s1) * 0.5 + bw * 0.18, (k0 + k1) * 0.5),
        RoofKind::Gable => (if rng.random_bool(0.5) { s0 + 0.5 } else { s1 - 0.5 }, (k0 + k1) * 0.5),
        RoofKind::FrontGable => ((s0 + s1) * 0.5, k1 - 0.5),
    };
    chimney(g, c, plot.at(cs, ck), ridge - 1.6, ridge + 0.9, 0.9, p.brick);
    // Gutters along the eaves and a downpipe at an end of each.
    let gc = mul(p.trim, 0.6);
    let eaves: [usize; 2] = if roof == RoofKind::FrontGable { [1, 3] } else { [0, 2] };
    g.lod(Lod::Near, |g| {
        for k in eaves {
            let fc = plot.face_of(body, k);
            fc.bx(g, f, Matte, 0.0, top - 0.1, 0.42, Vec3::new(fc.len + 0.6, 0.12, 0.12), gc);
            fc.bx(g, f, Matte, fc.len * 0.5 - 0.15, top * 0.5, 0.1, Vec3::new(0.08, top, 0.08), gc);
        }
    });
    // A TV aerial over the roof, or a dish on a side wall.
    let ap = plot.at((s0 + s1) * 0.5 - bw * 0.2, (k0 + k1) * 0.5);
    let aerial = d.random_bool(0.55);
    g.lod(Lod::Near, |g| {
        if aerial {
            g.part(Matte).cylinder(f.to_world(ap.x, ap.y), 0.03, 0.03, ridge - 0.2, ridge + 1.6, 4, rgba(CHARCOAL));
            f.bx(g, Matte, ap.x, ridge + 1.4, ap.y, Vec3::new(1.2, 0.04, 0.04), CHARCOAL);
            f.bx(g, Matte, ap.x, ridge + 1.4, ap.y, Vec3::new(0.04, 0.04, 0.6), CHARCOAL);
        } else {
            let side = plot.face_of(body, 1);
            dish(g, f, side.at(0.0, 0.3), top - 0.6, 0.35, [0.78, 0.78, 0.76]);
        }
    });
    plot.at(door_s, k0 - 0.4)
}

/// A dormer on the front slope at `s`: a box out of the roof back to the
/// ridge, a lead cap and a window.
#[allow(clippy::too_many_arguments)]
fn dormer(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, plot: &Plot, s: f32, k0: f32, eave: f32, rise: f32, span: f32, p: &Paint) {
    let r = plot.rect(s - 0.8, s + 0.8, k0 + 0.3, k0 + span);
    let h = (rise * 0.62).min(1.7);
    block(g, c, p.wall, &r, eave, eave + h, p.wall_c);
    block(g, c, Matte, &r.inset(-0.12), eave + h, eave + h + 0.1, mul(p.roof_c, 0.75));
    let face = plot.face_of(&r, 0);
    let wh = (h - 0.65).max(0.4);
    window(g, c, rng, &face, 0.0, eave + h - 0.2 - wh * 0.5, 0.9, wh, p.trim);
}

/// The kinds of home.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Home {
    /// One or two storeys (rooms in the roof on a third), any plan and roof.
    House,
    /// One storey, wide and low, a carport.
    Bungalow,
}

/// Which plot edges get a fence: the index into [`R::faces`].
type Fences = [bool; 4];

/// A home with its garden on `plot`: the house, garage or carport, drive,
/// path, front boundary, side and back fences, and the garden's furniture.
#[allow(clippy::too_many_arguments)]
fn home(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, d: &mut impl Rng, plot: &Plot, kind: Home, p: &Paint, fences: Fences) {
    let f = &c.frame;
    let (pw, pd) = (plot.width(), plot.depth());
    if pw < 7.0 || pd < 12.0 {
        // Too small for a house and a garden: a garden with a shed.
        garden_only(g, c, plot, p);
        return;
    }
    let storeys = if kind == Home::House && c.floors >= 2 { 2 } else { 1 };
    let attic = kind == Home::House && (c.floors >= 3 || (c.floors == 1 && rng.random_bool(0.5)));
    let garage_side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
    let garage = pw >= 13.0
        && match kind {
            Home::House => c.tier != Tier::Low || rng.random_bool(0.3),
            Home::Bungalow => true,
        };
    let gw = if garage { 3.4 } else { 0.0 };
    let max_w = if kind == Home::Bungalow { 14.0 } else { 11.0 };
    let hw = (pw - 2.4 - gw).clamp(5.0, max_w);
    let setback = (pd * 0.22).clamp(3.0, 7.0);
    let hd = (pd * 0.38).clamp(6.0, 9.5).min(pd - setback - 2.5).max(5.0);
    // The house sits off centre, away from the garage.
    let s_c = -garage_side * gw * 0.5;
    let mut body = plot.rect(s_c - hw * 0.5, s_c + hw * 0.5, setback, setback + hd);
    if !on_lot(c, &body) {
        match fit(c, body, 4.5) {
            Some(b) => body = b,
            None => {
                garden_only(g, c, plot, p);
                return;
            }
        }
    }
    let (s0, s1, k0, k1) = plot.span(&body);
    let bw = s1 - s0;
    let roll = rng.random_range(0..10);
    let roof = match kind {
        Home::Bungalow if roll <= 5 => RoofKind::Hip,
        Home::Bungalow => RoofKind::Gable,
        Home::House if roll <= 4 => RoofKind::Gable,
        Home::House if roll <= 7 => RoofKind::Hip,
        Home::House if bw < 10.0 => RoofKind::FrontGable,
        Home::House => RoofKind::Gable,
    };
    // A front wing on a house wide enough, away from the garage.
    let wing = if kind == Home::House && roof != RoofKind::FrontGable && bw >= 8.0 && k0 >= 3.2 && rng.random_bool(0.45) {
        let ww = (bw * 0.4).clamp(3.2, 5.0);
        let proj = rng.random_range(1.2..2.2f32).min(k0 - 1.8);
        let (a, b) = if garage_side > 0.0 { (s0 + 0.4, s0 + 0.4 + ww) } else { (s1 - 0.4 - ww, s1 - 0.4) };
        let w = plot.rect(a, b, k0 - proj, (k0 + k1) * 0.5);
        on_lot(c, &w).then_some(w)
    } else {
        None
    };
    let door_s = match wing {
        Some(w) => {
            let (ws0, ws1, _, _) = plot.span(&w);
            if garage_side > 0.0 { ((ws1 + s1) * 0.5).min(s1 - 1.0) } else { ((s0 + ws0) * 0.5).max(s0 + 1.0) }
        }
        None => (s0 + s1) * 0.5 + rng.random_range(-0.25..0.25) * (bw - 2.4).max(0.0),
    };
    let door_at = house(g, c, rng, d, plot, &body, storeys, attic, roof, wing, door_s, p);
    // Garage (a house) or carport (a bungalow) beside it, set back a little.
    let (gs0, gs1) = if garage_side > 0.0 { (s1, s1 + gw) } else { (s0 - gw, s0) };
    let gk0 = k0 + 0.6;
    let mut drive_to = None;
    if garage {
        let gr = plot.rect(gs0, gs1, gk0, gk0 + 5.8);
        if on_lot(c, &gr) {
            if kind == Home::House {
                block(g, c, p.wall, &gr, 0.0, 2.7, p.wall_c);
                if rng.random_bool(0.5) {
                    block(g, c, Matte, &gr.inset(-0.15), 2.7, 2.85, mul(p.roof_c, 0.7));
                } else {
                    pitched(g, c, &gr, 2.7, 1.3, plot.side % 2 != 0, false, (p.wall, p.wall_c), (p.roof, p.roof_c));
                }
                let gf = plot.face_of(&gr, 0);
                let col = mul(GARAGE_DOORS[rng.random_range(0..GARAGE_DOORS.len())], 1.0 - 0.3 * c.wear);
                gf.decal(g, f, Matte, 0.0, 1.1, crate::depth::FRAME, 2.6, 2.25, p.trim);
                gf.decal(g, f, Metal, 0.0, 1.05, DOOR_D, 2.4, 2.1, col);
            } else {
                carport(g, c, &gr, p);
            }
            drive_to = Some(((gs0 + gs1) * 0.5, gk0));
        }
    }
    // The drive (to the garage, or a parking pad beside the house).
    let (ds, dk) = drive_to.unwrap_or(((if garage_side > 0.0 { s1 + 1.6 } else { s0 - 1.6 }).clamp(-pw * 0.5 + 1.4, pw * 0.5 - 1.4), k0 + 0.5));
    let drive = plot.rect(ds - 1.4, ds + 1.4, 0.25, dk.max(1.0));
    let drive_ok = on_lot(c, &drive) && (drive_to.is_some() || !overlaps(&drive, &body));
    if drive_ok {
        ground(g, f, Paving, &drive.poly(), DRIVE_Y, mul(c.palette.paving, 0.95));
        if coin(d, if c.tier == Tier::Low { 0.45 } else { 0.7 }) {
            let q = plot.at(ds, (dk * 0.5).max(2.6));
            let yaw = plot.yaw_in();
            let col = pick(d, &c.palette.car_colors);
            g.lod(Lod::Mid, |g| {
                if vehicle_fits(c, q, yaw, Vehicle::Car) {
                    vehicle(g, f, c.palette, d, q, DRIVE_Y, yaw, Vehicle::Car, col);
                }
            });
        }
    }
    // The path to the door.
    let (dsd, dkd) = plot.local(door_at);
    let path = plot.rect(dsd - 0.55, dsd + 0.55, 0.25, dkd.max(0.6));
    if on_lot(c, &path) {
        ground(g, f, Paving, &path.poly(), PATH_Y, mul(c.palette.paving, 1.15));
    }
    // The front boundary, with gaps for the path and the drive.
    let mut gaps = vec![(dsd - 0.7, dsd + 0.7)];
    if drive_ok {
        gaps.push((ds - 1.5, ds + 1.5));
    }
    let style = rng.random_range(0..10);
    boundary(g, c, plot, 0.4, &gaps, style, p);
    // Side and back fences (low in front of the house, tall behind).
    side_fences(g, c, plot, fences, k0 + 0.5, style, p);
    back_garden(g, c, d, plot, k1, p);
    front_garden(g, c, d, plot, k0, &gaps, p);
}

/// Whether two rectangles overlap.
fn overlaps(a: &R, b: &R) -> bool {
    a.u0 < b.u1 && b.u0 < a.u1 && a.v0 < b.v1 && b.v0 < a.v1
}

/// A carport: four posts and a flat roof over the space beside a bungalow.
fn carport(g: &mut Geometry, c: &Ctx, r: &R, p: &Paint) {
    let f = &c.frame;
    block(g, c, Metal, &r.inset(-0.1), 2.45, 2.6, mul(p.roof_c, 0.9));
    g.lod(Lod::Mid, |g| {
        for q in r.inset(0.15).poly() {
            f.bx(g, Wood, q.x, 1.225, q.y, Vec3::new(0.14, 2.45, 0.14), p.trim);
        }
    });
}

/// A plot too small for a house: lawn, a shed and a tree.
fn garden_only(g: &mut Geometry, c: &Ctx, plot: &Plot, p: &Paint) {
    let f = &c.frame;
    let (pw, pd) = (plot.width(), plot.depth());
    let shed = plot.rect(-1.0, 1.0, pd * 0.5 - 0.9, pd * 0.5 + 0.9);
    if pw > 3.0 && pd > 3.0 && on_lot(c, &shed) {
        block(g, c, Wood, &shed, 0.0, 2.0, mul(c.palette.wood, 0.9));
        block(g, c, Matte, &shed.inset(-0.12), 2.0, 2.1, mul(p.roof_c, 0.6));
    }
    let q = plot.at(pw * 0.25, pd * 0.25);
    if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 1.6 && q.distance(shed.c()) > 2.8 {
        tree_in(g, f, c.palette, q, 3.5, 1.4, p.leaf);
    }
}

/// The front boundary along the plot's front, `inset` in, leaving `gaps`
/// (spans along it): a picket fence, a hedge, a low wall with a hedge
/// behind, or the lawn open to the street.
fn boundary(g: &mut Geometry, c: &Ctx, plot: &Plot, inset: f32, gaps: &[(f32, f32)], style: u32, p: &Paint) {
    let half = plot.width() * 0.5 - 0.2;
    let mut runs: Vec<(f32, f32)> = vec![(-half, half)];
    for &(a, b) in gaps {
        runs = runs.into_iter().flat_map(|(x, y)| if b <= x || a >= y { vec![(x, y)] } else { vec![(x, a), (b, y)] }).filter(|(x, y)| y - x > 0.4).collect();
    }
    for (a, b) in runs {
        let (pa, pb) = (plot.at(a, inset), plot.at(b, inset));
        if !c.spans_lot(pa, pb) {
            continue;
        }
        match style {
            0..=3 => picket(g, c, pa, pb, 1.0, p.fence),
            4..=5 => hedge(g, c, pa, pb, 1.1, 0.7, p.hedge),
            6..=7 => {
                low_wall(g, c, pa, pb, p.brick);
                let (qa, qb) = (plot.at(a + 0.2, inset + 0.6), plot.at(b - 0.2, inset + 0.6));
                if b - a > 0.8 && c.spans_lot(qa, qb) {
                    hedge(g, c, qa, qb, 1.3, 0.6, p.hedge);
                }
            }
            _ => {}
        }
    }
}

/// A picket fence from `a` to `b`: posts, two rails (from a few blocks
/// away), and near the camera the pickets, a few missing on a worn plot.
fn picket(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, col: [f32; 3]) {
    let f = &c.frame;
    let face = Face::from_edge(a, b);
    if face.len < 0.4 {
        return;
    }
    let posts = bays(face.len, 2.4);
    for k in 0..=posts {
        let t = -face.len * 0.5 + face.len * k as f32 / posts as f32;
        face.bx(g, f, Wood, t, (h + 0.1) * 0.5, 0.0, Vec3::new(0.1, h + 0.1, 0.1), col);
    }
    g.lod(Lod::Mid, |g| {
        for y in [h * 0.3, h * 0.72] {
            face.bx(g, f, Wood, 0.0, y, -0.07, Vec3::new(face.len, 0.07, 0.035), col);
        }
    });
    let key = hash2_salted((face.mid().x * 10.0) as i64 as u64, (face.mid().y * 10.0) as i64 as u64, PICKET_SALT ^ c.seed);
    g.lod(Lod::Near, |g| {
        let n = (face.len / 0.17).floor() as i32;
        for i in 0..n {
            let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / n as f32;
            let hk = hash2_salted(key, i as u64, PICKET_SALT);
            if unit(hk, 0) < c.wear * 0.18 {
                continue;
            }
            let ph = h * (0.92 + 0.08 * unit(hk, 16));
            face.bx(g, f, Wood, t, ph * 0.5, -0.1, Vec3::new(0.075, ph, 0.025), col);
        }
    });
}

/// A low brick garden wall with a coping.
fn low_wall(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, brick: [f32; 3]) {
    let f = &c.frame;
    let face = Face::from_edge(a, b);
    if face.len < 0.4 {
        return;
    }
    face.bx(g, f, Brick, 0.0, 0.3, 0.0, Vec3::new(face.len, 0.6, 0.24), brick);
    face.bx(g, f, Concrete, 0.0, 0.64, 0.0, Vec3::new(face.len + 0.04, 0.08, 0.32), [0.62, 0.60, 0.56]);
}

/// A close-boarded fence from `a` to `b`, `h` high: a panel run, its posts
/// near the camera.
fn board_fence(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, col: [f32; 3]) {
    let f = &c.frame;
    let face = Face::from_edge(a, b);
    if face.len < 0.4 {
        return;
    }
    face.bx(g, f, Wood, 0.0, h * 0.5, 0.0, Vec3::new(face.len, h, 0.04), col);
    g.lod(Lod::Near, |g| {
        let n = bays(face.len, 1.8);
        for k in 0..=n {
            let t = -face.len * 0.5 + face.len * k as f32 / n as f32;
            face.bx(g, f, Wood, t, (h + 0.06) * 0.5, 0.06, Vec3::new(0.1, h + 0.06, 0.08), mul(col, 0.8));
        }
    });
}

/// The fences on the plot's sides and back (`fences`, by [`R::faces`]),
/// 0.15 m in from its edges so a neighbour's fence never shares their
/// plane: low in front of `k_house`, full height behind.
fn side_fences(g: &mut Geometry, c: &Ctx, plot: &Plot, fences: Fences, k_house: f32, style: u32, p: &Paint) {
    let (pw, pd) = (plot.width(), plot.depth());
    let wood = mul(c.palette.wood, 0.85 - 0.2 * c.wear);
    let hs = pw * 0.5 - 0.15;
    for k in 1..4usize {
        if !fences[(plot.side + k) % 4] {
            continue;
        }
        if k == 2 {
            let (a, b) = (plot.at(-hs, pd - 0.15), plot.at(hs, pd - 0.15));
            if c.spans_lot(a, b) {
                board_fence(g, c, a, b, 1.7, wood);
            }
            continue;
        }
        let s = if k == 1 { hs } else { -hs };
        let kh = k_house.min(pd - 0.2);
        let (a, b) = (plot.at(s, 0.45), plot.at(s, kh));
        if kh > 0.9 && c.spans_lot(a, b) {
            match style {
                0..=3 => picket(g, c, a, b, 0.9, p.fence),
                4..=7 => hedge(g, c, a, b, 1.0, 0.6, p.hedge),
                _ => {}
            }
        }
        let (a, b) = (plot.at(s, kh), plot.at(s, pd - 0.15));
        if c.spans_lot(a, b) {
            board_fence(g, c, a, b, 1.7, wood);
        }
    }
}

/// Behind the house: a patio and a table, a shed at the end, a tree, a
/// washing line, vegetable beds or a trampoline.
fn back_garden(g: &mut Geometry, c: &Ctx, d: &mut impl Rng, plot: &Plot, k_back: f32, p: &Paint) {
    let f = &c.frame;
    let (pw, pd) = (plot.width(), plot.depth());
    let room = pd - k_back;
    if room < 3.0 {
        return;
    }
    let patio = plot.rect(-pw * 0.25, pw * 0.15, k_back, k_back + 2.6f32.min(room - 0.5));
    if on_lot(c, &patio) {
        ground(g, f, Paving, &patio.poly(), PATIO_Y, mul(c.palette.paving, 1.05));
        let q = patio.c();
        let shade = coin(d, 0.4).then(|| pick(d, &c.palette.awning_colors));
        g.lod(Lod::Near, |g| table(g, f, q, PATIO_Y, mul(c.palette.wood, 0.9), shade));
    }
    // A shed in a back corner.
    let mut shed_s = None;
    if room > 6.0 {
        let s = if d.random_bool(0.5) { 1.0f32 } else { -1.0 } * (pw * 0.5 - 1.6);
        let r = plot.rect(s - 1.1, s + 1.1, pd - 2.4, pd - 0.6);
        if on_lot(c, &r) {
            let wood = mul(c.palette.wood, d.random_range(0.6..0.95));
            g.lod(Lod::Mid, |g| {
                block(g, c, Wood, &r, 0.0, 2.0, wood);
                block(g, c, Matte, &r.inset(-0.12), 2.0, 2.12, [0.12, 0.12, 0.13]);
                plot.face_of(&r, 0).decal(g, f, Wood, 0.0, 0.9, DOOR_D, 0.8, 1.8, mul(wood, 0.7));
            });
            shed_s = Some(s);
        }
    }
    // A tree, clear of the shed.
    if room > 5.0 {
        let s = d.random_range(-0.3..0.3) * pw;
        let q = plot.at(s, k_back + room * d.random_range(0.45..0.6));
        let canopy = d.random_range(1.8..3.0f32);
        let h = d.random_range(4.0..7.5);
        if shed_s.is_none_or(|x| (x - s).abs() > canopy + 1.4) && c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > canopy * 0.8 {
            tree_in(g, f, c.palette, q, h, canopy, p.leaf);
        }
    }
    // A washing line across the lawn.
    if room > 6.0 && coin(d, 0.6) {
        let (a, b) = (plot.at(-pw * 0.3, k_back + room * 0.35), plot.at(pw * 0.1, k_back + room * 0.42));
        if c.spans_lot(a, b) {
            let cloth: Vec<[f32; 3]> = (0..4).map(|_| pick(d, &c.palette.tarp_colors)).collect();
            g.lod(Lod::Near, |g| {
                for q in [a, b] {
                    f.bx(g, Matte, q.x, 0.95, q.y, Vec3::new(0.06, 1.9, 0.06), [0.6, 0.6, 0.58]);
                }
                g.part(Matte).wire(f.at2(a, 1.85), f.at2(b, 1.85), 0.15, 0.015, rgba([0.7, 0.7, 0.68]));
                let yaw = (b - a).y.atan2((b - a).x);
                for (i, col) in cloth.iter().enumerate() {
                    let q = a.lerp(b, (i as f32 + 1.0) / 5.0);
                    f.bx_yaw(g, Tarp, q.x, 1.4, q.y, Vec3::new(0.5, 0.6, 0.02), *col, yaw);
                }
            });
        }
    }
    // Vegetable beds down one side, or a trampoline.
    if room > 9.0 && pw > 9.0 {
        if coin(d, 0.5) {
            let s = shed_s.map_or(pw * 0.5 - 1.4, |x| -x.signum() * (pw * 0.5 - 1.4));
            let bed = plot.rect(s - 0.7, s + 0.7, k_back + 3.2, k_back + (room - 3.0).min(8.0));
            if on_lot(c, &bed) {
                ground(g, f, Dirt, &bed.poly(), BED_Y, mul(c.palette.dirt, 0.8));
                let (_, _, bk0, bk1) = plot.span(&bed);
                let leaf = mul(p.leaf, 1.1);
                g.lod(Lod::Mid, |g| {
                    let n = ((bk1 - bk0) / 0.9) as i32;
                    for i in 0..n {
                        let q = plot.at(s, bk0 + 0.45 + i as f32 * 0.9);
                        f.bx(g, Foliage, q.x, 0.2, q.y, Vec3::new(0.8, 0.32, 0.8), leaf);
                    }
                });
            }
        } else {
            let q = plot.at(-pw * 0.2, k_back + room * 0.72);
            if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 2.0 {
                g.lod(Lod::Near, |g| {
                    let w = f.to_world(q.x, q.y);
                    g.part(Tarp).cylinder(w, 1.7, 1.7, 0.85, 0.9, 12, rgba([0.10, 0.10, 0.11]));
                    g.part(Matte).cylinder(w, 1.8, 1.8, 0.8, 0.86, 12, rgba([0.15, 0.36, 0.62]));
                });
            }
        }
    }
}

/// In front of the house: a flower bed inside the boundary, a shrub or a
/// small tree, the bins by the drive, now and then a bike on the lawn.
fn front_garden(g: &mut Geometry, c: &Ctx, d: &mut impl Rng, plot: &Plot, k_house: f32, gaps: &[(f32, f32)], p: &Paint) {
    let f = &c.frame;
    let pw = plot.width();
    if k_house < 2.6 {
        return;
    }
    // A bed inside the boundary away from the gaps.
    let free: Vec<f32> = [-pw * 0.3, pw * 0.3].into_iter().filter(|s| gaps.iter().all(|(a, b)| s + 1.7 < *a || s - 1.7 > *b)).collect();
    if let Some(&s) = free.first() {
        let bed = plot.rect(s - 1.5, s + 1.5, 0.75, 1.5);
        if on_lot(c, &bed) {
            ground(g, f, Dirt, &bed.poly(), BED_Y, mul(c.palette.dirt, 0.75));
            let cols: Vec<[f32; 3]> = (0..6).map(|_| FLOWERS[d.random_range(0..FLOWERS.len())]).collect();
            g.lod(Lod::Near, |g| {
                for (i, col) in cols.iter().enumerate() {
                    let q = plot.at(s - 1.25 + i as f32 * 0.5, 1.12);
                    f.bx(g, Foliage, q.x, 0.18, q.y, Vec3::new(0.34, 0.3, 0.34), mul(p.leaf, 0.9));
                    f.bx(g, Foliage, q.x, 0.36, q.y, Vec3::new(0.24, 0.1, 0.24), *col);
                }
            });
        }
        // A shrub or a small tree behind it.
        let q = plot.at(s, (k_house * 0.55).max(2.3));
        if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 1.2 {
            if coin(d, 0.5) {
                tree_in(g, f, c.palette, q, d.random_range(2.6..3.8), d.random_range(0.9..1.3), p.leaf);
            } else {
                let h = d.random_range(0.8..1.4);
                g.lod(Lod::Mid, |g| f.bx_yaw(g, Foliage, q.x, h * 0.5, q.y, Vec3::new(1.3, h, 1.1), mul(p.hedge, 1.1), 0.4));
            }
        }
    }
    // The bins beside the drive (or the path).
    let s = gaps.last().map_or(0.0, |(_, b)| b + 0.6);
    let n = d.random_range(1..=3);
    let cols: Vec<[f32; 3]> = (0..n).map(|_| BINS[d.random_range(0..BINS.len())]).collect();
    let yaw = plot.yaw_in();
    g.lod(Lod::Near, |g| {
        for (i, col) in cols.iter().enumerate() {
            let bs = s + i as f32 * 0.7;
            let q = plot.at(bs, 1.1);
            if bs.abs() < pw * 0.5 - 0.9 && c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.6 {
                f.bx_yaw(g, Matte, q.x, 0.48, q.y, Vec3::new(0.7, 0.96, 0.58), *col, yaw);
                f.bx_yaw(g, Matte, q.x, 1.0, q.y, Vec3::new(0.76, 0.08, 0.62), mul(*col, 0.8), yaw);
            }
        }
    });
    // A child's bike dropped on the lawn.
    if coin(d, 0.25) {
        let q = plot.at(-pw * 0.12, k_house * 0.5);
        let col = pick(d, &c.palette.car_colors);
        if c.inside_local(q) {
            g.lod(Lod::Near, |g| {
                f.flat(g, Matte, q.x, 0.09, q.y, 1.1, 0.06, 0.6, col);
                for t in [-0.45f32, 0.45] {
                    let w = q + Vec2::new(0.6f32.cos(), 0.6f32.sin()) * t;
                    f.flat(g, Matte, w.x, 0.1, w.y, 0.5, 0.5, 0.6, CHARCOAL);
                }
            });
        }
    }
}

/// The lot's rectangle of the frame, a little in from its edges.
fn lot_rect(c: &Ctx) -> R {
    R::of(&c.lot_poly).inset(0.2)
}

/// Homes in a row along the street, a plot about `width` metres wide each.
fn row_of_homes(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, kind: Home, width: f32, bungalow_share: f64) {
    let mut d = stream(c);
    let lr = lot_rect(c);
    let n = ((lr.w() / width).floor() as i32).clamp(1, 4);
    let pw = lr.w() / n as f32;
    for k in 0..n {
        let r = R::new(lr.u0 + k as f32 * pw, lr.u0 + (k + 1) as f32 * pw, lr.v0, lr.v1);
        let p = paint(c, rng);
        let kind = if kind == Home::House && rng.random_bool(bungalow_share) { Home::Bungalow } else { kind };
        // Each plot fences its left side and back; the last its right too.
        home(g, c, rng, &mut d, &Plot { r, side: 0 }, kind, &p, [false, k == n - 1, true, true]);
    }
}

// ── the styles ───────────────────────────────────────────────────────────────

/// Detached houses in their gardens: one to four along the street, each its
/// own plan, roof, paint and garden.
pub fn garden_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    yard(g, c, 0.2, Foliage, lawn(c));
    row_of_homes(g, c, rng, Home::House, 15.0, 0.15);
}

/// One-storey bungalows with carports, hipped or gabled.
pub fn bungalow(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    yard(g, c, 0.2, Foliage, lawn(c));
    row_of_homes(g, c, rng, Home::Bungalow, 17.0, 0.0);
}

/// A close: a short lane off the street to a turning circle with a planted
/// island, houses down both sides facing the lane and round its head. A
/// lot too small for a close is a row of houses.
pub fn cul_de_sac(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    yard(g, c, 0.2, Foliage, lawn(c));
    let lr = lot_rect(c);
    let lane_w = 5.6;
    let circle = 7.0;
    let side_len = ((lr.d() - 27.5) * 0.55).max(13.0);
    let head = lr.v0 + 1.0 + side_len + circle;
    let cu = lr.c().x;
    let lane = R::new(cu - lane_w * 0.5, cu + lane_w * 0.5, lr.v0, head);
    let ring = ngon(cu, head, circle, 20, 0.0);
    let head_v = head + circle + 0.3;
    if lr.w() < 34.0 || lr.d() < 41.0 || lr.v1 - head_v < 12.0 || !on_lot(c, &lane) || !poly_inside(c, &ring) {
        row_of_homes(g, c, rng, Home::House, 15.0, 0.2);
        return;
    }
    let mut d = stream(c);
    // The lane and the turning circle with its island, kerbs, a name post.
    ground(g, f, Asphalt, &lane.poly(), LANE_Y, mul(c.palette.asphalt, 1.05));
    ground(g, f, Asphalt, &ring, LANE_Y + 0.004, mul(c.palette.asphalt, 1.05));
    ground(g, f, Foliage, &ngon(cu, head, 2.4, 14, 0.0), BED_Y + 0.02, mul(lawn(c), 1.1));
    tree_in(g, f, c.palette, Vec2::new(cu, head), 5.5, 1.8, mix(c.palette.foliage, [0.24, 0.40, 0.18], 0.4));
    g.lod(Lod::Mid, |g| {
        for s in [-1.0f32, 1.0] {
            let x = cu + s * (lane_w * 0.5 - 0.1);
            let len = (head - circle * 0.8 - lr.v0 - 0.4).max(0.2);
            f.bx(g, Concrete, x, 0.07, lr.v0 + 0.2 + len * 0.5, Vec3::new(0.2, 0.14, len), c.palette.curb);
        }
    });
    let sign = Vec2::new(cu - lane_w * 0.5 - 0.5, lr.v0 + 1.0);
    g.lod(Lod::Mid, |g| {
        f.bx(g, Matte, sign.x, 0.6, sign.y, Vec3::new(0.08, 1.2, 0.08), CHARCOAL);
        f.bx(g, Matte, sign.x, 1.25, sign.y, Vec3::new(1.2, 0.32, 0.05), WHITE);
    });
    // Plots down each side facing the lane, and round the head facing the
    // circle; trees on the lawns beside the circle.
    let mut plots: Vec<(Plot, Fences)> = Vec::new();
    let n_side = ((side_len / 13.0).floor() as i32).clamp(1, 3);
    let step = side_len / n_side as f32;
    for i in 0..n_side {
        let (v0, v1) = (lr.v0 + 1.0 + i as f32 * step, lr.v0 + 1.0 + (i + 1) as f32 * step);
        // Left of the lane its front is +u (side 1); right of it -u (side 3).
        plots.push((Plot { r: R::new(lr.u0, cu - lane_w * 0.5, v0, v1), side: 1 }, [i == 0, false, true, true]));
        plots.push((Plot { r: R::new(cu + lane_w * 0.5, lr.u1, v0, v1), side: 3 }, [i == 0, true, true, false]));
    }
    let n_head = ((lr.w() / 16.0).floor() as i32).clamp(2, 4);
    let hw = lr.w() / n_head as f32;
    for i in 0..n_head {
        let r = R::new(lr.u0 + i as f32 * hw, lr.u0 + (i + 1) as f32 * hw, head_v, lr.v1);
        plots.push((Plot { r, side: 0 }, [false, i == n_head - 1, true, true]));
    }
    for s in [-1.0f32, 1.0] {
        let q = Vec2::new(cu + s * (circle + (lr.w() * 0.5 - circle) * 0.5), head);
        if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 3.0 {
            tree_in(g, f, c.palette, q, d.random_range(5.0..7.5), 2.4, mix(c.palette.foliage, [0.24, 0.40, 0.18], 0.4));
        }
    }
    for (plot, fences) in &plots {
        let p = paint(c, rng);
        let kind = if rng.random_bool(0.25) { Home::Bungalow } else { Home::House };
        home(g, c, rng, &mut d, plot, kind, &p, *fences);
    }
}

// ── shops ────────────────────────────────────────────────────────────────────

/// A row of small shops with flats over them behind a forecourt of parking
/// bays: shopfronts between pilasters, a fascia with each shop's board, a
/// few awnings, sash windows upstairs, a flat roof behind a parapet or a
/// tiled roof with chimneys; bins and a back door behind.
pub fn shopping_parade(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.2, Paving, mul(pal.paving, 0.95));
    let lr = lot_rect(c);
    let fore = if lr.d() > 18.0 { (lr.d() * 0.3).clamp(5.5, 9.0) } else { 1.5 };
    let depth = (lr.d() - fore - 2.5).clamp(5.0, 13.0);
    let Some(block) = fit(c, R::new(lr.u0 + 0.6, lr.u1 - 0.6, lr.v0 + fore, lr.v0 + fore + depth), 4.0) else { return };
    let storeys = c.floors.clamp(2, 3);
    let shop_h = 3.6;
    let top = shop_h + (storeys - 1) as f32 * FH;
    let brick = rng.random_bool(0.6);
    let (ws, wc) = if brick { (Brick, mul(pick(rng, &pal.brick), 1.0 - 0.15 * c.wear)) } else { (Plaster, RENDERS[rng.random_range(0..RENDERS.len())]) };
    extrude(g, f, ws, &block.poly(), 0.0, top, wc);
    let trim = mix(WHITE, [0.5, 0.5, 0.48], 0.4 * c.wear);
    let front = block.front();
    let units = ((front.len / 6.2).floor() as i32).clamp(1, 8);
    let uw = front.len / units as f32;
    // The fascia across every shop.
    front.bx(g, f, Matte, 0.0, shop_h - 0.4, 0.2, Vec3::new(front.len, 0.75, 0.4), mul(trim, 0.85));
    for i in 0..units {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * uw;
        // A pilaster at each party wall (and the ends).
        for e in if i == units - 1 { vec![-0.5f32, 0.5] } else { vec![-0.5] } {
            front.bx(g, f, ws, t + uw * e, (shop_h - 0.8) * 0.5, 0.12, Vec3::new(0.4, shop_h - 0.8, 0.24), wc);
        }
        let shut = coin(rng, c.wear * 0.4);
        if shut {
            front.decal(g, f, Metal, t, 1.4, 0.05, uw - 0.8, 2.6, mul(pick(rng, &pal.metal_paint), 0.7));
        } else {
            front.decal(g, f, Matte, t, 0.3, 0.03, uw - 0.6, 0.6, mul(trim, 0.5));
            let lit = coin(rng, 0.55 + 0.4 * c.lit_chance());
            let col = mul(pick(rng, &[pal.window_warm, [1.2, 1.15, 1.0], [1.1, 1.05, 0.85]]), 0.75);
            front.glaze(g, c, t - 0.5, 1.7, crate::depth::PANE, uw - 2.0, 2.2, if lit { Pane::Lit(col) } else { Pane::Dark });
            front.decal(g, f, Wood, t + uw * 0.5 - 0.95, 1.1, DOOR_D, 0.95, 2.2, mul(FASCIAS[rng.random_range(0..FASCIAS.len())], 1.2));
        }
        // The shop's board on the fascia, lettered in light.
        let board = FASCIAS[rng.random_range(0..FASCIAS.len())];
        front.decal(g, f, Matte, t, shop_h - 0.4, 0.405, uw - 0.9, 0.55, board);
        let sc = mul(pick(rng, &pal.sign_colors), if shut { 0.25 } else { 0.7 });
        let letters = ((uw - 1.6) / 0.42).floor().clamp(2.0, 10.0) as i32;
        for k in 0..letters {
            let lt = t - (letters - 1) as f32 * 0.21 + k as f32 * 0.42;
            front.decal(g, f, Glow, lt, shop_h - 0.4, 0.42, 0.28, 0.32, sc);
        }
        if !shut && rng.random_bool(0.45) {
            let col = mul(pick(rng, &pal.awning_colors), 1.0 - 0.35 * c.wear);
            g.lod(Lod::Mid, |g| awning_striped(g, c, &front, t, shop_h - 0.85, uw - 0.8, col));
        }
        // Sash windows upstairs.
        for k in 1..storeys as i32 {
            let y = shop_h + (k - 1) as f32 * FH + FH * 0.55;
            for s in [-0.25f32, 0.25] {
                window(g, c, rng, &front, t + s * uw, y, 0.9, 1.4, trim);
            }
        }
    }
    // Side and back windows and back doors.
    for face in [block.left(), block.right()] {
        window_rows_from(g, c, rng, &face, storeys - 1, shop_h, trim);
    }
    let back = block.back();
    window_rows_from(g, c, rng, &back, storeys - 1, shop_h, trim);
    for i in 0..units {
        let t = -back.len * 0.5 + (i as f32 + 0.5) * back.len / units as f32;
        back.decal(g, f, Wood, t, 1.05, DOOR_D, 0.95, 2.1, mul(c.palette.wood, 0.7));
    }
    // The roof: a parapet, or tiles with a chimney on each party wall.
    if rng.random_bool(0.5) {
        parapet(g, c, &block.poly(), top, 0.7, 0.3, ws, wc);
        block_cap(g, c, &block, top);
    } else {
        let rise = (block.d() * 0.5 * 0.55).min(4.0);
        let ridge = pitched(g, c, &block, top, rise, true, false, (ws, wc), (Brick, TILES[rng.random_range(0..TILES.len())]));
        for i in 1..units {
            let u = block.u0 + i as f32 * block.w() / units as f32;
            chimney(g, c, Vec2::new(u, block.c().y), ridge - 1.2, ridge + 1.0, 1.2, if brick { wc } else { pick_first(&pal.brick) });
        }
    }
    let mut d = stream(c);
    // The forecourt: bays nose to the shops, some cars, a bench, a bike rack.
    if fore > 5.0 {
        let court = R::new(block.u0, block.u1, lr.v0 + 0.3, block.v0 - 0.4);
        if on_lot(c, &court) {
            ground(g, f, Asphalt, &court.poly(), LANE_Y, mul(pal.asphalt, 1.1));
            let n = ((court.w() - 1.0) / 2.6).floor() as i32;
            let cols: Vec<Option<[f32; 3]>> = (0..n).map(|_| coin(&mut d, 0.5).then(|| pick(&mut d, &pal.car_colors))).collect();
            let bay_d = (court.d() - 1.0).min(5.0);
            g.lod(Lod::Mid, |g| {
                for i in 0..=n {
                    let u = court.c().x - n as f32 * 1.3 + i as f32 * 2.6;
                    f.flat(g, Matte, u, LINE_Y, court.v1 - bay_d * 0.5, 0.1, bay_d, 0.0, [0.86, 0.86, 0.80]);
                }
                for (i, col) in cols.iter().enumerate() {
                    if let Some(col) = col {
                        // Nose a metre short of the shopfronts.
                        let q = Vec2::new(court.c().x - n as f32 * 1.3 + (i as f32 + 0.5) * 2.6, court.v1 - 3.0);
                        if vehicle_fits(c, q, FRAC_PI_2, Vehicle::Car) {
                            vehicle(g, f, pal, &mut d, q, LANE_Y, FRAC_PI_2, Vehicle::Car, *col);
                        }
                    }
                }
            });
        }
    }
    // A post box at the end of the parade.
    g.lod(Lod::Near, |g| {
        let q = Vec2::new(block.u0 + 0.35, block.v0 - 0.3);
        if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.4 {
            let w = f.to_world(q.x, q.y);
            g.part(Matte).cylinder(w, 0.26, 0.26, 0.0, 1.3, 10, rgba([0.70, 0.08, 0.08]));
            g.part(Matte).cylinder(w, 0.3, 0.3, 1.3, 1.42, 10, rgba([0.70, 0.08, 0.08]));
        }
    });
    // The service yard behind: bins.
    let yard_v = block.v1 + 1.2;
    let cols: Vec<[f32; 3]> = (0..units).map(|_| BINS[d.random_range(0..BINS.len())]).collect();
    g.lod(Lod::Near, |g| {
        for (i, col) in cols.iter().enumerate() {
            let q = Vec2::new(block.u0 + 0.9 + i as f32 * block.w() / units as f32, yard_v);
            if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.6 {
                f.bx(g, Metal, q.x, 0.65, q.y, Vec3::new(1.3, 1.3, 1.0), *col);
            }
        }
    });
}

/// Windows round a face from `y0`, `storeys` rows (the flats over shops).
fn window_rows_from(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, storeys: u8, y0: f32, trim: [f32; 3]) {
    if storeys > 0 {
        window_rows(g, c, rng, face, storeys, y0, None, trim);
    }
}

/// A flat roof's felt over a block (inside its parapet).
fn block_cap(g: &mut Geometry, c: &Ctx, r: &R, top: f32) {
    block(g, c, Matte, &r.inset(0.31), top, top + 0.06, [0.16, 0.16, 0.17]);
}

/// A striped canvas awning over a shopfront: a sloped blind, a valance, arms.
fn awning_striped(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, col: [f32; 3]) {
    let f = &c.frame;
    // Out over the pavement no further than the lot (both front corners).
    let mut reach = c.reach_out(face).min(1.6) - 0.1;
    while reach >= 0.6 && !(c.inside_local(face.at(t - w * 0.5, reach)) && c.inside_local(face.at(t + w * 0.5, reach))) {
        reach -= 0.2;
    }
    if reach < 0.6 {
        return;
    }
    let a = f.at2(face.at(t, 0.05), y + 0.35);
    let b = f.at2(face.at(t, reach), y - 0.2);
    // `slab` hangs below the a→b line: the canvas, its stripes just over it.
    g.part(Tarp).slab(b, a, w, 0.04, rgba(col));
    let n = ((w / 0.5) as i32).max(2);
    let up = (a - b).normalize();
    let side = f.dir3(face.tangent);
    let normal = side.cross(up).normalize();
    let normal = if normal.y < 0.0 { -normal } else { normal };
    for k in 0..n {
        if k % 2 == 1 {
            let off = -w * 0.5 + (k as f32 + 0.5) * w / n as f32;
            let centre = (a + b) * 0.5 + side * off + normal * 0.005;
            g.part(Tarp).decal(centre, side, up, w / n as f32, a.distance(b), normal, rgba(mix(col, WHITE, 0.75)));
        }
    }
    face.bx(g, f, Tarp, t, y - 0.32, reach, Vec3::new(w, 0.25, 0.03), col);
}

/// A garden centre: a timber shop with a sign, a glasshouse, polytunnels,
/// tables of plants in a fenced yard, pallets of compost and a gravel car
/// park at the front.
pub fn garden_centre(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let p = paint(c, rng);
    yard(g, c, 0.2, Dirt, mix(pal.gravel, pal.dirt, 0.3));
    let lr = lot_rect(c);
    let mut d = stream(c);
    let park_d = if lr.d() > 24.0 { (lr.d() * 0.28).clamp(6.0, 12.0) } else { 0.0 };
    // The shop: a long timber shed under a pitched roof, at one side.
    let shop_w = (lr.w() * 0.45).clamp(6.0, 20.0);
    let shop_d = (lr.d() * 0.3).clamp(5.0, 10.0);
    let left = rng.random_bool(0.5);
    let su = if left { lr.u0 + 0.6 + shop_w * 0.5 } else { lr.u1 - 0.6 - shop_w * 0.5 };
    let shop = R::new(su - shop_w * 0.5, su + shop_w * 0.5, lr.v0 + park_d + 0.8, lr.v0 + park_d + 0.8 + shop_d);
    let Some(shop) = fit(c, shop, 4.0) else {
        garden_only(g, c, &Plot { r: lr, side: 0 }, &p);
        return;
    };
    let wood = mul(pal.wood, 1.0 - 0.25 * c.wear);
    block(g, c, Wood, &shop, 0.0, 3.4, wood);
    pitched(g, c, &shop, 3.4, (shop.d() * 0.5 * 0.55).min(3.0), true, false, (Wood, wood), (Metal, [0.20, 0.30, 0.22]));
    let front = shop.front();
    let trim = WHITE;
    let n = ((front.len - 2.0) / 2.4).floor().max(1.0) as i32;
    for i in 0..n {
        let t = -front.len * 0.5 + 1.0 + (i as f32 + 0.5) * (front.len - 2.0) / n as f32;
        if i == n / 2 {
            // Glass doors, lit by day and into the evening.
            front.decal(g, f, Matte, t, 1.2, crate::depth::FRAME, 2.2, 2.5, trim);
            front.glaze(g, c, t, 1.15, crate::depth::PANE, 2.0, 2.3, Pane::Lit(mul(pal.window_warm, 0.7)));
        } else {
            window(g, c, rng, &front, t, 1.7, 1.6, 1.6, trim);
        }
    }
    // The sign across the gable side, a board with lit letters.
    let sc = mul(pick(rng, &pal.sign_colors), 0.7);
    front.decal(g, f, Matte, 0.0, 3.0, 0.1, (front.len * 0.6).min(8.0), 0.6, [0.14, 0.30, 0.20]);
    let letters = ((front.len * 0.6).min(8.0) / 0.5).floor() as i32;
    for k in 0..letters {
        front.decal(g, f, Glow, -(letters - 1) as f32 * 0.25 + k as f32 * 0.5, 3.0, 0.12, 0.34, 0.36, sc);
    }
    // Beside the shop: two rows of plant tables along the yard's front, the
    // glasshouse behind them.
    let tu0 = if left { shop.u1 + 1.2 } else { lr.u0 + 1.0 };
    let tu1 = if left { lr.u1 - 1.0 } else { shop.u0 - 1.2 };
    let mut back_v = shop.v1 + 3.0;
    let gw = (tu1 - tu0).min(18.0);
    if gw > 5.0 {
        let gu = if left { tu0 + gw * 0.5 } else { tu1 - gw * 0.5 };
        let gv0 = shop.v0 + 4.6;
        let gr = R::new(gu - gw * 0.5, gu + gw * 0.5, gv0, gv0 + (shop_d + 1.0).min(lr.v1 - gv0 - 1.0).max(4.0));
        if on_lot(c, &gr) {
            glasshouse(g, c, &gr, 2.6);
            back_v = back_v.max(gr.v1 + 2.0);
        }
    }
    // Polytunnels at the back.
    let room = lr.v1 - back_v - 0.5;
    if room > 6.0 {
        let n = ((lr.w() - 2.0) / 6.5).floor().clamp(1.0, 4.0) as i32;
        for i in 0..n {
            let u = lr.u0 + 1.0 + (i as f32 + 0.5) * (lr.w() - 2.0) / n as f32;
            let len = room.min(18.0) - 0.5;
            let (a, b) = (Vec2::new(u, back_v), Vec2::new(u, back_v + len));
            if c.spans_lot(a + Vec2::new(2.6, 0.0), b + Vec2::new(2.6, 0.0)) && c.spans_lot(a - Vec2::new(2.6, 0.0), b - Vec2::new(2.6, 0.0)) {
                polytunnel(g, c, a, b, 2.4);
            }
        }
    }
    // The plant tables, pallets of compost by the door.
    let mut tables: Vec<(Vec2, [f32; 3])> = Vec::new();
    if tu1 - tu0 > 3.0 {
        for j in 0..2 {
            let v = shop.v0 + 0.8 + j as f32 * 1.7;
            let n = ((tu1 - tu0) / 2.2).floor() as i32;
            for i in 0..n {
                let q = Vec2::new(tu0 + 1.1 + i as f32 * 2.2, v);
                tables.push((q, FLOWERS[d.random_range(0..FLOWERS.len())]));
            }
        }
    }
    let leaf = p.leaf;
    g.lod(Lod::Mid, |g| {
        for (q, _) in &tables {
            if c.all_inside(&[*q + Vec2::new(1.0, 0.5), *q - Vec2::new(1.0, 0.5)]) {
                f.bx(g, Wood, q.x, 0.7, q.y, Vec3::new(1.9, 0.08, 1.0), mul(c.palette.wood, 0.8));
                f.bx(g, Foliage, q.x, 0.86, q.y, Vec3::new(1.7, 0.24, 0.8), leaf);
            }
        }
    });
    g.lod(Lod::Near, |g| {
        for (q, col) in &tables {
            if c.all_inside(&[*q + Vec2::new(1.0, 0.5), *q - Vec2::new(1.0, 0.5)]) {
                for s in [-0.6f32, 0.0, 0.6] {
                    f.bx(g, Foliage, q.x + s, 1.02, q.y, Vec3::new(0.3, 0.1, 0.5), *col);
                }
                for s in [-0.85f32, 0.85] {
                    f.bx(g, Matte, q.x + s, 0.33, q.y, Vec3::new(0.06, 0.66, 0.9), CHARCOAL);
                }
            }
        }
        // Compost on pallets by the shop door.
        let base = front.at(front.len * 0.5 - 1.2, 1.6);
        for k in 0..3 {
            let q = base + Vec2::new(0.0, k as f32 * 0.0) - front.tangent * (k as f32 * 1.3);
            if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.8 {
                f.bx(g, Wood, q.x, 0.07, q.y, Vec3::new(1.1, 0.14, 1.1), mul(c.palette.wood, 0.7));
                f.bx(g, Tarp, q.x, 0.45, q.y, Vec3::new(1.0, 0.62, 1.0), [0.16, 0.30, 0.18]);
            }
        }
    });
    // The car park and its cars; a picket fence along the street.
    if park_d > 0.0 {
        let pk = R::new(lr.u0 + 0.5, lr.u1 - 0.5, lr.v0 + 0.5, lr.v0 + park_d);
        if on_lot(c, &pk) {
            ground(g, f, Dirt, &pk.poly(), LANE_Y, mix(pal.gravel, WHITE, 0.15));
            let n = ((pk.w() - 1.0) / 2.6).floor() as i32;
            let cols: Vec<Option<[f32; 3]>> = (0..n).map(|_| coin(&mut d, 0.45).then(|| pick(&mut d, &pal.car_colors))).collect();
            g.lod(Lod::Mid, |g| {
                for (i, col) in cols.iter().enumerate() {
                    if let Some(col) = col {
                        let q = Vec2::new(pk.u0 + 1.3 + i as f32 * 2.6, pk.c().y);
                        if vehicle_fits(c, q, FRAC_PI_2, Vehicle::Car) {
                            vehicle(g, f, pal, &mut d, q, LANE_Y, FRAC_PI_2, Vehicle::Car, *col);
                        }
                    }
                }
            });
        }
    }
    let gaps = [(-2.2f32, 2.2f32)];
    boundary(g, c, &Plot { r: lr, side: 0 }, 0.3, &gaps, 0, &p);
    // A sign on posts at the entrance.
    let sq = Vec2::new(lr.c().x + 3.2, lr.v0 + 0.9);
    if c.inside_local(sq) {
        for s in [-0.9f32, 0.9] {
            f.bx(g, Wood, sq.x + s, 1.0, sq.y, Vec3::new(0.12, 2.0, 0.12), mul(pal.wood, 0.8));
        }
        f.bx(g, Wood, sq.x, 1.75, sq.y, Vec3::new(2.0, 0.7, 0.08), [0.14, 0.30, 0.20]);
        f.bx(g, Glow, sq.x, 1.75, sq.y - 0.05, Vec3::new(1.5, 0.3, 0.02), sc);
    }
}

/// A glasshouse on `r`: glass walls and a glass ridge roof on a low brick
/// wall, white glazing bars near the camera.
fn glasshouse(g: &mut Geometry, c: &Ctx, r: &R, h: f32) {
    let f = &c.frame;
    let glass = mix(c.palette.glass_dark, [0.55, 0.66, 0.62], 0.5);
    block(g, c, Brick, &r.inset(-0.04), 0.0, 0.5, mul(pick_first(&c.palette.brick), 0.9));
    block(g, c, Glass, r, 0.5, h, glass);
    let along_u = r.w() >= r.d();
    let s = if along_u { r.d() } else { r.w() } * 0.5;
    let ax = if along_u { Vec2::X } else { Vec2::Y };
    let l = if along_u { r.w() } else { r.d() } * 0.5;
    loft(g, f, Glass, &quad(r.c(), ax, l, s), &quad(r.c(), ax, l, 0.05), h, h + s * 0.7, glass);
    g.lod(Lod::Mid, |g| {
        let n = ((2.0 * l) / 1.2).floor() as i32;
        for i in 0..=n {
            let q = r.c() + ax * (-l + 2.0 * l * i as f32 / n.max(1) as f32);
            for side in [-1.0f32, 1.0] {
                let p = q + ax.perp() * side * (s + 0.03);
                f.bx(g, Matte, p.x, (h + 0.5) * 0.5, p.y, Vec3::new(0.06, h - 0.5, 0.06), WHITE);
            }
        }
        let yaw = if along_u { 0.0 } else { FRAC_PI_2 };
        f.bx_yaw(g, Matte, r.c().x, h + s * 0.7 + 0.04, r.c().y, Vec3::new(2.0 * l, 0.08, 0.14), WHITE, yaw);
    });
}

fn pick_first(v: &[[f32; 3]]) -> [f32; 3] {
    v.first().copied().unwrap_or([0.5, 0.3, 0.22])
}

/// A polytunnel from `a` to `b`: a half-round of milky plastic on hoops.
fn polytunnel(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, r: f32) {
    let len = a.distance(b);
    if len < 2.0 {
        return;
    }
    let ax = Axis::level(&c.frame, a, b, 0.0);
    revolve(g.part(Tarp), &ax, &[(r, 0.0), (r, len)], 12, Some((0.0, std::f32::consts::PI)), rgba([0.84, 0.86, 0.82]));
    // The ends closed with a door: a half-disc each end.
    let ends: Vec<(f32, f32)> = vec![(0.0, 0.02), (r - 0.02, 0.02)];
    let _ = ends;
    for (p, dir) in [(a, (b - a) / len), (b, (a - b) / len)] {
        let face = Face::new(p + dir.perp() * r, p - dir.perp() * r, -dir);
        let n = 8;
        let centre = c.frame.at2(p, 0.0);
        let along = c.frame.dir3(face.tangent);
        let out = c.frame.dir3(face.out);
        for k in 0..n {
            let (t0, t1) = (k as f32 / n as f32 * std::f32::consts::PI, (k + 1) as f32 / n as f32 * std::f32::consts::PI);
            let (p0, p1) = (centre + along * (r * t0.cos()) + Vec3::Y * (r * t0.sin()), centre + along * (r * t1.cos()) + Vec3::Y * (r * t1.sin()));
            g.part(Tarp).tri_facing(centre, p0, p1, out, rgba([0.80, 0.82, 0.78]));
        }
        face.decal(g, &c.frame, Wood, 0.0, 0.95, 0.02, 1.0, 1.9, mul(c.palette.wood, 0.8));
    }
}

/// A corner café: a small two-storey building near the street with big
/// lit windows, a striped awning, tables out on a terrace under string
/// lights, planters; the flat over it under a gable to the street.
pub fn corner_cafe(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let p = paint(c, rng);
    yard(g, c, 0.2, Foliage, p.lawn);
    let lr = lot_rect(c);
    let w = (lr.w() * 0.55).clamp(6.0, 14.0);
    let dd = (lr.d() * 0.4).clamp(6.0, 11.0);
    let left = rng.random_bool(0.5);
    let u = if left { lr.u0 + 0.5 + w * 0.5 } else { lr.u1 - 0.5 - w * 0.5 };
    let setback = if lr.d() > 14.0 { 2.0 } else { 0.8 };
    let Some(body) = fit(c, R::new(u - w * 0.5, u + w * 0.5, lr.v0 + setback, lr.v0 + setback + dd), 4.0) else { return };
    let storeys = c.floors.clamp(1, 2);
    let shop_h = 3.4;
    let top = shop_h + (storeys - 1) as f32 * FH;
    extrude(g, f, p.wall, &body.poly(), 0.0, top, p.wall_c);
    let roof_k = rng.random_bool(0.5);
    let ridge = pitched(g, c, &body, top, (body.w().min(body.d()) * 0.5 * 0.75).min(4.0), !roof_k || body.w() > body.d() * 1.4, false, (p.wall, p.wall_c), (p.roof, p.roof_c));
    chimney(g, c, Vec2::new(body.c().x + body.w() * 0.2, body.c().y), ridge - 1.4, ridge + 0.9, 0.9, p.brick);
    // Big windows on the street front and the terrace side, lit warm.
    let terrace_side = if left { body.right() } else { body.left() };
    for face in [body.front(), terrace_side] {
        let n = ((face.len - 1.0) / 2.6).floor().max(1.0) as i32;
        for i in 0..n {
            let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * (face.len - 1.0) / n as f32;
            if face.is_front && i == n - 1 && n > 1 {
                front_door(g, c, &face, t, &p, false);
                continue;
            }
            let lit = coin(rng, 0.6 + 0.4 * c.lit_chance());
            face.decal(g, f, Matte, t, 1.55, crate::depth::FRAME, 2.1, 2.3, p.trim);
            face.glaze(g, c, t, 1.55, crate::depth::PANE, 1.9, 2.1, if lit { Pane::Lit(mul(pal.window_warm, 0.85)) } else { Pane::Dark });
        }
        // A fascia with the café's name in light.
        face.bx(g, f, Matte, 0.0, shop_h - 0.35, 0.12, Vec3::new(face.len - 0.4, 0.5, 0.2), mul(p.door, 0.9));
        let sc = mul(pick(rng, &pal.sign_colors), 0.7);
        let letters = ((face.len * 0.5) / 0.45).floor().clamp(2.0, 9.0) as i32;
        for k in 0..letters {
            face.decal(g, f, Glow, -(letters - 1) as f32 * 0.225 + k as f32 * 0.45, shop_h - 0.35, 0.23, 0.3, 0.3, sc);
        }
        let col = mul(pick(rng, &pal.awning_colors), 1.0 - 0.3 * c.wear);
        g.lod(Lod::Mid, |g| awning_striped(g, c, &face, 0.0, shop_h - 0.85, face.len - 1.0, col));
    }
    for face in [body.back(), if left { body.left() } else { body.right() }] {
        window_rows(g, c, rng, &face, 1, 0.0, None, p.trim);
    }
    if storeys > 1 {
        for face in body.faces() {
            window_rows_from(g, c, rng, &face, storeys - 1, shop_h, p.trim);
        }
    }
    // The terrace beside it: paving, tables with parasols, string lights.
    let tu0 = if left { body.u1 + 0.3 } else { lr.u0 + 0.6 };
    let tu1 = if left { lr.u1 - 0.6 } else { body.u0 - 0.3 };
    if tu1 - tu0 > 3.0 {
        let terrace = R::new(tu0, tu1, body.v0 - 0.5, body.v1.min(lr.v1 - 1.0));
        if on_lot(c, &terrace) {
            ground(g, f, Paving, &terrace.poly(), PATIO_Y, mul(pal.paving, 1.1));
            let mut d = stream(c);
            let cols: Vec<[f32; 3]> = (0..8).map(|_| pick(&mut d, &pal.awning_colors)).collect();
            let nu = ((terrace.w() - 1.0) / 2.6).floor().max(1.0) as i32;
            let nv = ((terrace.d() - 1.0) / 2.8).floor().clamp(1.0, 3.0) as i32;
            g.lod(Lod::Mid, |g| {
                for i in 0..nu {
                    for j in 0..nv {
                        let q = Vec2::new(terrace.u0 + (i as f32 + 0.5) * terrace.w() / nu as f32, terrace.v0 + (j as f32 + 0.5) * terrace.d() / nv as f32);
                        table(g, f, q, PATIO_Y, mul(pal.wood, 0.9), Some(cols[((i + j * nu) as usize) % cols.len()]));
                    }
                }
            });
            // String lights from the café's eaves to a post at the far corner.
            let post = Vec2::new(if left { terrace.u1 - 0.3 } else { terrace.u0 + 0.3 }, terrace.v0 + 0.3);
            f.bx(g, Wood, post.x, 1.5, post.y, Vec3::new(0.12, 3.0, 0.12), mul(pal.wood, 0.8));
            let eave_u = if left { body.u1 + 0.1 } else { body.u0 - 0.1 };
            for k in 0..2 {
                let v = body.v0 + 1.0 + k as f32 * (body.d() - 2.0);
                string_lights(g, f, Vec3::new(eave_u, 2.9, v), Vec3::new(post.x, 2.9, post.y), 8, mul(pal.window_warm, 1.2));
            }
            // Planters along the street edge.
            let n = (terrace.w() / 2.0).floor() as i32;
            g.lod(Lod::Mid, |g| {
                for i in 0..n {
                    let q = Vec2::new(terrace.u0 + 1.0 + i as f32 * 2.0, terrace.v0 - 0.2);
                    if c.inside_local(q) {
                        f.bx(g, Wood, q.x, 0.25, q.y, Vec3::new(1.2, 0.5, 0.5), mul(pal.wood, 0.7));
                        f.bx(g, Foliage, q.x, 0.62, q.y, Vec3::new(1.1, 0.3, 0.45), p.leaf);
                    }
                }
            });
        }
    }
    // A chalk board by the door, a bike against the wall.
    g.lod(Lod::Near, |g| {
        let fr = body.front();
        let q = fr.at(-fr.len * 0.25, 0.8);
        if c.inside_local(q) {
            f.bx_yaw(g, Wood, q.x, 0.5, q.y, Vec3::new(0.6, 1.0, 0.06), [0.10, 0.11, 0.10], 0.0);
        }
    });
    // Behind: bins and a tree.
    let q = Vec2::new(body.c().x, body.v1 + 2.0);
    if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 2.5 {
        tree_in(g, f, pal, q, 5.0, 2.0, p.leaf);
    }
}

// ── the garden manor ─────────────────────────────────────────────────────────

/// The big garden house a family (two plots) or a billionaire (up to four)
/// builds in the suburb: behind tall clipped hedges with brick gate piers
/// and lanterns, a gravel drive to a turning circle, a wide house of two
/// storeys and rooms in a hipped roof with tall chimneys, wings either
/// side, bay windows and a columned porch; lawns, a walled kitchen garden
/// with a glasshouse, a pond, old trees, and on the grandest a tennis court
/// and a garage block.
pub fn garden_manor(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let f = &c.frame;
    let pal = c.palette;
    let grand = e.form == Some(nd_sim::estate::Form::Mansion);
    let mut p = paint(c, rng);
    p.roof = Panel;
    p.roof_c = SLATES[rng.random_range(0..SLATES.len())];
    if p.wall == Wood || p.wall == Concrete {
        p.wall = Brick;
        p.wall_c = p.brick;
    }
    p.lawn = mix(pal.foliage, [0.20, 0.40, 0.14], 0.6);
    yard(g, c, 0.2, Foliage, p.lawn);
    let lr = lot_rect(c);
    let mut d = stream(c);
    // Tall hedges all round, a gap with gate piers in the middle of the front.
    let ring = lr.inset(0.5).poly();
    let gate_w = 5.0;
    for (i, face) in c.faces(&ring).iter().enumerate() {
        if i == 0 && face.len > gate_w + 1.0 {
            let side = (face.len - gate_w) * 0.5;
            for s in [-1.0f32, 1.0] {
                let (a, b) = (face.at(s * gate_w * 0.5, 0.0), face.at(s * face.len * 0.5, 0.0));
                if c.spans_lot(a, b) {
                    hedge(g, c, a, b, 2.6, 1.1, p.hedge);
                }
                let pier = face.at(s * (gate_w * 0.5 + 0.1), 0.0);
                if c.inside_local(pier) {
                    f.bx(g, Brick, pier.x, 1.4, pier.y, Vec3::new(0.7, 2.8, 0.7), p.brick);
                    f.bx(g, Concrete, pier.x, 2.86, pier.y, Vec3::new(0.84, 0.12, 0.84), [0.70, 0.68, 0.62]);
                    let from = g.get(Glow).colors.len();
                    f.bx(g, Glow, pier.x, 3.15, pier.y, Vec3::new(0.3, 0.42, 0.3), mul(pal.window_warm, 1.2));
                    mark_glow(g, from, NIGHT_MARK);
                    f.bx(g, Matte, pier.x, 3.42, pier.y, Vec3::new(0.38, 0.1, 0.38), CHARCOAL);
                }
            }
            let _ = side;
            g.lod(Lod::Mid, |g| {
                for s in [-1.0f32, 1.0] {
                    let q = face.at(s * gate_w * 0.25, -0.2);
                    if c.inside_local(q) {
                        f.bx_yaw(g, Matte, q.x, 0.95, q.y, Vec3::new(gate_w * 0.45, 1.7, 0.05), [0.10, 0.11, 0.10], face.yaw() + s * 0.5);
                    }
                }
            });
        } else if face.len > 0.5 && c.spans_lot(face.a, face.b) {
            hedge(g, c, face.a, face.b, 2.6, 1.1, p.hedge);
        }
    }
    // The house.
    let hw = (lr.w() * 0.42).clamp(12.0, if grand { 32.0 } else { 22.0 });
    let hd = (lr.d() * 0.2).clamp(8.5, 13.0);
    let fore = (lr.d() * 0.3).clamp(12.0, 26.0);
    let plot = Plot { r: lr, side: 0 };
    let Some(body) = fit(c, plot.rect(-hw * 0.5, hw * 0.5, fore, fore + hd), 6.0) else {
        // Too small a site for the house: a garden house of the ordinary kind.
        home(g, c, rng, &mut d, &plot, Home::House, &p, [true, true, true, true]);
        return;
    };
    let (s0, s1, k0, k1) = plot.span(&body);
    let storeys = 2;
    let top = storeys as f32 * FH + 0.6;
    extrude(g, f, Stone, &body.inset(-0.06).poly(), 0.0, 0.6, [0.66, 0.64, 0.58]);
    extrude(g, f, p.wall, &body.poly(), 0.0, top, p.wall_c);
    let rise = (body.d() * 0.5 * 0.85).min(5.5);
    let ridge = pitched(g, c, &body, top, rise, true, true, (p.wall, p.wall_c), (p.roof, p.roof_c));
    // A string course and quoins in stone.
    block(g, c, Stone, &body.inset(-0.04), FH + 0.2, FH + 0.4, [0.74, 0.72, 0.66]);
    // Wings either side, set back, gabled to the front.
    for side in [-1.0f32, 1.0] {
        let ww = (hw * 0.28).clamp(5.0, 9.0);
        let (a, b) = if side < 0.0 { (s0 - ww, s0) } else { (s1, s1 + ww) };
        let w = plot.rect(a, b, k0 + 1.5, k1 - 0.5);
        if !on_lot(c, &w) {
            continue;
        }
        let wtop = FH * 2.0;
        extrude(g, f, p.wall, &w.poly(), 0.0, wtop, p.wall_c);
        pitched(g, c, &w, wtop, (ww * 0.5 * 0.9).min(rise - 0.5), false, false, (p.wall, p.wall_c), (p.roof, p.roof_c));
        for k in 0..4 {
            if (k == 1 && side < 0.0) || (k == 3 && side > 0.0) {
                continue;
            }
            window_rows(g, c, rng, &plot.face_of(&w, k), 2, 0.0, None, p.trim);
        }
        let cp = plot.at((a + b) * 0.5, k1 - 1.2);
        chimney(g, c, cp, wtop, wtop + (ww * 0.45).min(rise) + 1.2, 1.0, p.brick);
    }
    // Two bays up the front, the porch between them.
    let front = plot.face_of(&body, 0);
    let bay_t = front.len * 0.3;
    for t in [-bay_t, bay_t] {
        let q = front.at(t, 0.6);
        f.bx_yaw(g, p.wall, q.x, (top - 0.4) * 0.5, q.y, Vec3::new(3.0, top - 0.4, 1.2), p.wall_c, front.yaw());
        let bay = Face::new(front.at(t - 1.5, 1.2), front.at(t + 1.5, 1.2), front.out);
        for k in 0..storeys as i32 {
            window(g, c, rng, &bay, 0.0, k as f32 * FH + FH * 0.55 + 0.3, 1.6, 1.5, p.trim);
        }
        f.bx_yaw(g, p.roof, q.x, top - 0.3, q.y, Vec3::new(3.3, 0.2, 1.5), p.roof_c, front.yaw());
    }
    let n = ((front.len - 2.0) / 2.6).floor() as i32;
    for k in 0..storeys as i32 {
        for i in 0..n.max(0) {
            let t = -(front.len - 2.0) * 0.5 + (i as f32 + 0.5) * (front.len - 2.0) / n as f32;
            if (t.abs() - bay_t).abs() < 2.4 || (k == 0 && t.abs() < 1.6) {
                continue;
            }
            window(g, c, rng, &front, t, k as f32 * FH + FH * 0.55 + 0.3, 1.2, 1.5, p.trim);
        }
    }
    for k in 1..4 {
        window_rows(g, c, rng, &plot.face_of(&body, k), storeys, 0.3, None, p.trim);
    }
    // The porch: columns and a pediment-less flat roof, lanterns either side.
    if c.reach_out(&front) > 3.0 {
        for s in [-1.2f32, 1.2] {
            front.bx(g, f, Stone, s, 1.5, 2.0, Vec3::new(0.36, 3.0, 0.36), WHITE);
        }
        front.bx(g, f, Stone, 0.0, 3.15, 1.2, Vec3::new(3.2, 0.3, 2.6), WHITE);
        front.bx(g, f, Stone, 0.0, 0.15, 1.2, Vec3::new(3.6, 0.3, 2.8), [0.70, 0.68, 0.62]);
    }
    front_door(g, c, &front, 0.0, &p, false);
    porch_light(g, c, &front, -2.2, 2.4);
    // Dormers and chimneys on the main roof.
    let nd = ((s1 - s0) / 5.0).floor().clamp(1.0, 5.0) as i32;
    for i in 0..nd {
        let s = s0 + (s1 - s0) * (i as f32 + 1.0) / (nd + 1) as f32;
        if (s - (s0 + s1) * 0.5).abs() < 0.5 && nd > 1 {
            continue;
        }
        dormer(g, c, rng, &plot, s, k0, top, rise, (k1 - k0) * 0.5, &p);
    }
    for s in [s0 + (s1 - s0) * 0.22, s1 - (s1 - s0) * 0.22] {
        chimney(g, c, plot.at(s, (k0 + k1) * 0.5), ridge - 1.8, ridge + 1.5, 1.4, p.brick);
    }
    // The gravel drive to a turning circle before the door, its island
    // planted with a tree.
    let gravel = mix(pal.gravel, WHITE, 0.25);
    let drive = plot.rect(-1.8, 1.8, 0.6, (k0 - 6.0).max(1.0));
    if on_lot(c, &drive) {
        ground(g, f, Dirt, &drive.poly(), DRIVE_Y, gravel);
    }
    let cc = plot.at(0.0, k0 - 6.0);
    let circle = ngon(cc.x, cc.y, 5.5, 18, 0.0);
    if poly_inside(c, &circle) && k0 > 10.0 {
        ground(g, f, Dirt, &circle, DRIVE_Y + 0.003, gravel);
        ground(g, f, Foliage, &ngon(cc.x, cc.y, 2.2, 14, 0.0), PATH_Y, mul(p.lawn, 1.1));
        tree_in(g, f, pal, cc, 5.0, 1.6, p.leaf);
        let col = pick(&mut d, &pal.car_colors);
        let q = plot.at(3.6, k0 - 4.2);
        g.lod(Lod::Mid, |g| {
            if vehicle_fits(c, q, 0.0, Vehicle::Car) {
                vehicle(g, f, pal, &mut d, q, DRIVE_Y, plot.yaw_in() + FRAC_PI_2, Vehicle::Car, col);
            }
        });
    }
    // Old trees on the front lawns.
    for s in [-1.0f32, 1.0] {
        let q = plot.at(s * (lr.w() * 0.32), k0 * 0.45);
        let canopy = (k0 * 0.18).clamp(2.0, 4.5);
        if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > canopy {
            tree_in(g, f, pal, q, canopy * 2.6, canopy, mul(p.leaf, 0.9));
        }
    }
    // Behind: a terrace, lawn, the walled kitchen garden with its
    // glasshouse, a pond, the tennis court and garage block on a big site.
    let back = lr.v1 - body.v1;
    let terrace = plot.rect(s0 + 1.0, s1 - 1.0, k1, k1 + 3.0f32.min(back - 1.0));
    if back > 4.0 && on_lot(c, &terrace) {
        ground(g, f, Paving, &terrace.poly(), PATIO_Y, mul(pal.paving, 1.15));
        g.lod(Lod::Mid, |g| {
            let (ts0, ts1, tk0, _) = plot.span(&terrace);
            let n = ((ts1 - ts0) / 3.0).floor() as i32;
            for i in 0..n {
                let q = plot.at(ts0 + 1.5 + i as f32 * 3.0, tk0 + 1.5);
                if i % 2 == 0 {
                    f.bx(g, Stone, q.x, 0.4, q.y, Vec3::new(0.6, 0.8, 0.6), [0.72, 0.70, 0.64]);
                    f.bx(g, Foliage, q.x, 1.0, q.y, Vec3::new(0.7, 0.5, 0.7), p.leaf);
                }
            }
        });
    }
    let garden_k = k1 + 5.0;
    let garden_d = lr.d() - garden_k - 1.5;
    if garden_d > 10.0 {
        // The walled kitchen garden in one back corner.
        let kw = (lr.w() * 0.35).clamp(8.0, 22.0);
        let kd = (garden_d * 0.6).clamp(8.0, 20.0);
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let ks = side * (lr.w() * 0.5 - 1.6 - kw * 0.5);
        let kg = plot.rect(ks - kw * 0.5, ks + kw * 0.5, lr.d() - 1.6 - kd, lr.d() - 1.6);
        if on_lot(c, &kg) {
            for (i, face) in kg.faces().iter().enumerate() {
                if i == plot.side {
                    // A doorway in the middle of its front wall.
                    for s in [-1.0f32, 1.0] {
                        let (a, b) = (face.at(s * 0.8, 0.0), face.at(s * face.len * 0.5, 0.0));
                        let fa = Face::from_edge(a, b);
                        fa.bx(g, f, Brick, 0.0, 1.0, 0.0, Vec3::new(fa.len, 2.0, 0.3), p.brick);
                    }
                } else {
                    face.bx(g, f, Brick, 0.0, 1.0, -0.15, Vec3::new(face.len, 2.0, 0.3), p.brick);
                }
            }
            let inner = kg.inset(1.2);
            let beds = ((inner.w() + inner.d()) / 4.0).floor().clamp(2.0, 6.0) as i32;
            for i in 0..beds {
                let (bs0, bs1, bk0, bk1) = plot.span(&inner);
                let k = bk0 + (i as f32 + 0.5) * (bk1 - bk0) / beds as f32;
                let bed = plot.rect(bs0, bs1, k - 0.6, k + 0.6);
                if on_lot(c, &bed) {
                    ground(g, f, Dirt, &bed.poly(), BED_Y, mul(pal.dirt, 0.75));
                    let leaf = mul(p.leaf, 1.1);
                    g.lod(Lod::Mid, |g| {
                        let n = ((bs1 - bs0) / 1.0) as i32;
                        for j in 0..n {
                            let q = plot.at(bs0 + 0.5 + j as f32, k);
                            f.bx(g, Foliage, q.x, 0.2, q.y, Vec3::new(0.6, 0.34, 0.8), leaf);
                        }
                    });
                }
            }
            // A glasshouse against its back wall.
            let gh = plot.rect(ks - kw * 0.3, ks + kw * 0.3, lr.d() - 1.6 - 3.4, lr.d() - 1.6 - 0.4);
            if on_lot(c, &gh) && kd > 9.0 {
                glasshouse(g, c, &gh, 2.6);
            }
        }
        // A pond on the lawn on the other side.
        let pq = plot.at(-side * lr.w() * 0.22, garden_k + garden_d * 0.4);
        let pr = (garden_d * 0.15).clamp(2.0, 4.5);
        if poly_inside(c, &ngon(pq.x, pq.y, pr + 0.6, 14, 0.0)) {
            ground(g, f, Stone, &ngon(pq.x, pq.y, pr + 0.5, 14, 0.2), BED_Y, [0.56, 0.55, 0.50]);
            ground(g, f, Glass, &ngon(pq.x, pq.y, pr, 14, 0.2), PATH_Y, mix(pal.water, [0.10, 0.16, 0.14], 0.5));
        }
        // The tennis court and garage block on the grandest sites.
        if grand && garden_d > 30.0 {
            let tc = plot.rect(-side * lr.w() * 0.22 - 6.0, -side * lr.w() * 0.22 + 6.0, garden_k + garden_d - 25.0, garden_k + garden_d - 1.0);
            if on_lot(c, &tc) {
                ground(g, f, Paving, &tc.poly(), PATIO_Y, [0.16, 0.34, 0.24]);
                let tcc = tc.c();
                g.lod(Lod::Mid, |g| {
                    f.flat(g, Matte, tcc.x, PATIO_Y + 0.01, tcc.y, 10.0, 0.08, 0.0, WHITE);
                    f.bx(g, Matte, tcc.x, 0.5, tcc.y, Vec3::new(11.0, 1.0, 0.04), CHARCOAL);
                    fence(g, c, tc.poly()[0], tc.poly()[1], 3.0, false);
                    fence(g, c, tc.poly()[2], tc.poly()[3], 3.0, false);
                });
            }
        }
    }
    if grand || lr.w() > 60.0 {
        // A garage block beside the drive, three doors.
        let gs = if lr.w() * 0.5 - hw * 0.5 - 12.0 > 8.0 { -(hw * 0.5 + 12.0) } else { hw * 0.5 + 12.0 };
        let gr = plot.rect(gs - 5.0, gs + 5.0, k0 + 1.0, k0 + 7.0);
        if on_lot(c, &gr) && !overlaps(&gr, &body) {
            block(g, c, p.wall, &gr, 0.0, 3.0, p.wall_c);
            pitched(g, c, &gr, 3.0, 1.8, true, true, (p.wall, p.wall_c), (p.roof, p.roof_c));
            let gf = plot.face_of(&gr, 0);
            for k in -1..=1 {
                gf.decal(g, f, Wood, k as f32 * 3.1, 1.15, DOOR_D, 2.6, 2.3, mul(pal.wood, 0.75));
            }
            let dr = plot.rect(gs - 5.0, gs + 5.0, k0 - 4.0, k0 + 0.9);
            if on_lot(c, &dr) {
                ground(g, f, Dirt, &dr.poly(), DRIVE_Y, gravel);
            }
        }
    }
    // Trees scattered on the front lawns, clear of the drive and the circle.
    for _ in 0..d.random_range(3..8) {
        let q = Vec2::new(d.random_range(lr.u0..lr.u1), d.random_range(lr.v0..lr.v1));
        let canopy = d.random_range(2.0..3.6f32);
        let (s, k) = plot.local(q);
        let clear = k < k0 - 2.0 - canopy && s.abs() > 7.5 + canopy && k > 3.5;
        if clear && c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > canopy + 1.2 {
            tree_in(g, f, pal, q, d.random_range(6.0..10.0), canopy, mul(p.leaf, d.random_range(0.85..1.05)));
        }
    }
}
