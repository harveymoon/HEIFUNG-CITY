//! [social] What the city's crews and clubs make, as props: a street-art
//! crew's mural and a community garden on a vacant lot (raised beds of
//! greens and flowers, a shed, a water butt, a hand-painted sign).
//!
//! A mural is painted on the building's street face as drawn: the
//! building's own geometry is read for its walls facing the street
//! ([`facade_spot`]), windows, signs and fittings in front of them are
//! marked, and the largest clear stretch (else the wall around the
//! windows) takes the paint. The design ([`paint_mural`]) is procedural:
//! a crew keeps its palette, motif and tag from mural to mural (a synthwave
//! sun, a figure, a face, an eye, waves, a skyline), each mural lays them
//! out its own way; gradients come from vertex colours on a fine grid,
//! neon strokes and block lettering are crisp. A second, unlit copy of the
//! paint is the floodlit mural at night (the app shows it after dark), and
//! two lamps hang over it. Only a mural with no building (a vacant lot)
//! stands on a free-standing wall ([`mural_wall`]).
//!
//! Laid out in world coordinates from a seed, so each keeps its look.

use crate::color::{at, mix, mul, pick, rgba};
use crate::palette::Palette;
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::Rng;

/// Height of a lot's ground.
const LOT_Y: f32 = 0.04;

/// The paint: neon and street colours (linear).
pub(crate) const PAINT: [[f32; 3]; 10] = [
    [0.95, 0.12, 0.45],
    [0.1, 0.75, 0.95],
    [0.98, 0.75, 0.08],
    [0.35, 0.95, 0.25],
    [0.6, 0.2, 0.95],
    [0.98, 0.35, 0.08],
    [0.05, 0.25, 0.75],
    [0.9, 0.9, 0.85],
    [0.08, 0.08, 0.1],
    [0.85, 0.15, 0.12],
];

// ── murals ──────────────────────────────────────────────────────────────────

/// Surfaces paint goes on: walls, not glass.
const PAINTABLE: [Surface; 15] = [Concrete, Plaster, Brick, Stone, Panel, Metal, Rust, Wood, WornConcrete, WornPlaster, WornBrick, WornPanel, WornMetal, Container, WornContainer];
/// Side of a cell when reading a wall, metres.
const WALL_CELL: f32 = 0.5;
/// The biggest mural a crew paints, metres.
pub const MURAL_MAX_W: f32 = 16.0;
pub const MURAL_MAX_H: f32 = 12.0;
/// The smallest clear stretch of wall worth painting.
const MURAL_MIN_W: f32 = 3.0;
const MURAL_MIN_H: f32 = 2.4;
/// How far the paint stands off the wall (the night copy a little further).
const STANDOFF: f32 = 0.05;
const NIGHT_STANDOFF: f32 = 0.075;
/// A wall this far back from the lot front is still the street face.
const SETBACK_FREE: f32 = 3.0;

/// Where a mural goes: a rectangle on a wall, and which of its cells are
/// wall (not window, sign or fitting).
#[derive(Clone, Debug, PartialEq)]
pub struct FacadeSpot {
    /// Bottom-left corner of the painted area, on the wall's plane.
    pub origin: Vec3,
    /// Along the wall, left to right as seen from the street (horizontal unit).
    pub along: Vec3,
    /// The wall's outward normal (towards the street, horizontal unit).
    pub out: Vec3,
    pub width: f32,
    pub height: f32,
    /// Cells of [`WALL_CELL`], `cols` by `rows` from the bottom-left: true
    /// where paint can go.
    pub cols: usize,
    pub rows: usize,
    pub paint: Vec<bool>,
}

impl FacadeSpot {
    /// The whole rectangle is wall (a free-standing wall's face).
    pub fn open(origin: Vec3, along: Vec3, out: Vec3, width: f32, height: f32) -> Self {
        let cols = (width / WALL_CELL).ceil().max(1.0) as usize;
        let rows = (height / WALL_CELL).ceil().max(1.0) as usize;
        Self { origin, along, out, width, height, cols, rows, paint: vec![true; cols * rows] }
    }

    /// A point on the wall `u` along and `v` up from the bottom-left corner,
    /// standing `off` in front of it.
    pub fn at(&self, u: f32, v: f32, off: f32) -> Vec3 {
        self.origin + self.along * u + Vec3::Y * v + self.out * off
    }

    /// Whether paint can go at (`u`, `v`).
    pub fn paintable(&self, u: f32, v: f32) -> bool {
        if u < 0.0 || v < 0.0 || u > self.width || v > self.height {
            return false;
        }
        let c = ((u / WALL_CELL) as usize).min(self.cols - 1);
        let r = ((v / WALL_CELL) as usize).min(self.rows - 1);
        self.paint[r * self.cols + c]
    }

    /// Share of the rectangle that takes paint.
    pub fn clear_share(&self) -> f32 {
        self.paint.iter().filter(|p| **p).count() as f32 / self.paint.len().max(1) as f32
    }
}

/// The triangles of a mesh, as corner positions.
fn triangles(m: &MeshData) -> impl Iterator<Item = [Vec3; 3]> + '_ {
    m.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]].map(|i| Vec3::from_array(m.positions[i as usize])))
}

/// A wall plane facing the street: its normal, offset, and triangles.
struct Plane {
    n: Vec3,
    d: f32,
    area: f32,
    tris: Vec<[Vec3; 3]>,
}

fn inside_tri(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let cross = |o: Vec2, x: Vec2, y: Vec2| (x - o).perp_dot(y - o);
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    let neg = d1 < -1e-5 || d2 < -1e-5 || d3 < -1e-5;
    let pos = d1 > 1e-5 || d2 > 1e-5 || d3 > 1e-5;
    !(neg && pos)
}

/// The best rectangle of `free` cells (`cols` x `rows`, bottom row first):
/// `(col, row, w, h)` in cells, scored by its paintable area (capped at the
/// largest mural) and how low it sits. None when nothing reaches the
/// smallest mural.
fn best_rect(free: &[bool], cols: usize, rows: usize, y0: f32) -> Option<(usize, usize, usize, usize, f32)> {
    let mut up = vec![0usize; cols * rows];
    for r in (0..rows).rev() {
        for c in 0..cols {
            if free[r * cols + c] {
                up[r * cols + c] = 1 + if r + 1 < rows { up[(r + 1) * cols + c] } else { 0 };
            }
        }
    }
    let (min_w, min_h) = ((MURAL_MIN_W / WALL_CELL).ceil() as usize, (MURAL_MIN_H / WALL_CELL).ceil() as usize);
    let max_w = (MURAL_MAX_W / WALL_CELL) as usize;
    let mut best: Option<(usize, usize, usize, usize, f32)> = None;
    for r in 0..rows {
        for c in 0..cols {
            let mut h = usize::MAX;
            for c2 in c..cols.min(c + max_w * 2) {
                h = h.min(up[r * cols + c2]);
                if h < min_h {
                    break;
                }
                let w = c2 - c + 1;
                if w < min_w {
                    continue;
                }
                let wm = (w as f32 * WALL_CELL).min(MURAL_MAX_W);
                let hm = (h as f32 * WALL_CELL).min(MURAL_MAX_H);
                // Low on the wall reads from the street; a squat strip less
                // than a panel.
                let bottom = y0 + r as f32 * WALL_CELL;
                let shape = (hm / wm).clamp(0.25, 1.0).sqrt();
                let score = wm * hm * shape / (1.0 + bottom.max(0.0) / 18.0);
                if best.is_none_or(|b| score > b.4) {
                    best = Some((c, r, w, h, score));
                }
            }
        }
    }
    best
}

/// [social] The place for a mural on a building as drawn: the walls of
/// `building` (its geometry, before it is raised onto its pad) that face
/// `facing` (the way to the street) near `front` (the middle of the lot's
/// street edge). Windows, glass, signs, balconies and anything else drawn
/// within 3 m in front of a wall are kept clear; the largest clear stretch
/// wins, lower and nearer the street preferred. When the windows leave no
/// clear stretch big enough, the wall around them is painted instead
/// (the spot's cells say where). None when the building has no wall to
/// the street (all glass, or nothing faces it).
pub fn facade_spot(building: &Geometry, front: Vec2, facing: Vec2) -> Option<FacadeSpot> {
    let out2 = facing.try_normalize()?;
    let mut planes = wall_planes(building, out2);
    planes.retain(|p| p.area >= MURAL_MIN_W * MURAL_MIN_H);
    planes.sort_by(|a, b| b.area.total_cmp(&a.area));
    planes.truncate(6);
    let front3 = Vec3::new(front.x, 0.0, front.y);
    let mut best: Option<(f32, FacadeSpot)> = None;
    for p in &planes {
        // The best scoring spot (the first of equals).
        if let Some((score, spot)) = plane_spot(building, p, front3) {
            if !best.as_ref().is_some_and(|b| b.0 >= score) {
                best = Some((score, spot));
            }
        }
    }
    best.map(|b| b.1)
}

/// The building's paintable walls facing `out2`, grouped by plane.
fn wall_planes(building: &Geometry, out2: Vec2) -> Vec<Plane> {
    let mut planes: Vec<Plane> = Vec::new();
    for s in PAINTABLE {
        for t in triangles(building.get(s)) {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            let area = n.length() * 0.5;
            if area < 1e-3 {
                continue;
            }
            let n = n / (area * 2.0);
            if n.y.abs() > 0.08 {
                continue;
            }
            let nh = Vec2::new(n.x, n.z).normalize_or_zero();
            if nh.dot(out2) < 0.8 {
                continue;
            }
            let n3 = Vec3::new(nh.x, 0.0, nh.y);
            let d = t[0].dot(n3);
            match planes.iter_mut().find(|p| p.n.dot(n3) > 0.998 && (p.d - d).abs() < 0.08) {
                Some(p) => {
                    p.area += area;
                    p.tris.push(t);
                }
                None => planes.push(Plane { n: n3, d, area, tris: vec![t] }),
            }
        }
    }
    planes
}

/// The best rectangle to paint on one wall plane and its score: clear wall
/// if there is enough, else the wall painted round its windows, kept within
/// reach of the lot's front (`front3`).
fn plane_spot(building: &Geometry, p: &Plane, front3: Vec3) -> Option<(f32, FacadeSpot)> {
    let out = p.n;
    let along = Vec3::new(out.z, 0.0, -out.x);
    let tf = |q: Vec3| Vec2::new(q.dot(along), q.y);
    // The wall's extent, kept within reach of the lot's front.
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for t in &p.tris {
        for q in t {
            lo = lo.min(tf(*q));
            hi = hi.max(tf(*q));
        }
    }
    let mid_t = front3.dot(along);
    lo.x = lo.x.max(mid_t - 120.0);
    hi.x = hi.x.min(mid_t + 120.0);
    hi.y = hi.y.min(lo.y + 150.0);
    if hi.x - lo.x < MURAL_MIN_W || hi.y - lo.y < MURAL_MIN_H {
        return None;
    }
    let cols = ((hi.x - lo.x) / WALL_CELL).ceil() as usize;
    let rows = ((hi.y - lo.y) / WALL_CELL).ceil() as usize;
    let centre = |c: usize, r: usize| Vec2::new(lo.x + (c as f32 + 0.5) * WALL_CELL, lo.y + (r as f32 + 0.5) * WALL_CELL);
    let cell_range = |a: f32, b: f32, base: f32, n: usize| {
        let i0 = (((a - base) / WALL_CELL).floor().max(0.0) as usize).min(n);
        let i1 = (((b - base) / WALL_CELL).ceil().max(0.0) as usize).min(n);
        i0..i1
    };
    // Wall where a cell's middle is on one of the plane's triangles.
    let mut wall = vec![false; cols * rows];
    for t in &p.tris {
        let (a, b, c) = (tf(t[0]), tf(t[1]), tf(t[2]));
        let (tlo, thi) = (a.min(b).min(c), a.max(b).max(c));
        for r in cell_range(tlo.y, thi.y, lo.y, rows) {
            for col in cell_range(tlo.x, thi.x, lo.x, cols) {
                if inside_tri(centre(col, r), a, b, c) {
                    wall[r * cols + col] = true;
                }
            }
        }
    }
    // The cells whose middles lie in `a..b` (the one under the middle
    // when that is narrower than a cell).
    let centres = |a: f32, b: f32, base: f32, n: usize| {
        let i0 = ((a - base) / WALL_CELL - 0.5).ceil().max(0.0) as usize;
        let i1 = ((b - base) / WALL_CELL - 0.5).floor();
        if i1 < i0 as f32 {
            let m = (((a + b) * 0.5 - base) / WALL_CELL).floor();
            return if m >= 0.0 && (m as usize) < n { m as usize..m as usize + 1 } else { 0..0 };
        }
        i0.min(n)..((i1 as usize) + 1).min(n)
    };
    // Anything drawn just in front of the wall keeps the paint off.
    let mut blocked = vec![false; cols * rows];
    for (_, m) in building.iter() {
        for t in triangles(m) {
            let s = t.map(|q| q.dot(out) - p.d);
            let (smin, smax) = (s[0].min(s[1]).min(s[2]), s[0].max(s[1]).max(s[2]));
            if smax < 0.005 || smin > 3.0 {
                continue;
            }
            let (a, b, c) = (tf(t[0]), tf(t[1]), tf(t[2]));
            let (tlo, thi) = (a.min(b).min(c) - Vec2::splat(0.1), a.max(b).max(c) + Vec2::splat(0.1));
            for r in centres(tlo.y, thi.y, lo.y, rows) {
                for col in centres(tlo.x, thi.x, lo.x, cols) {
                    blocked[r * cols + col] = true;
                }
            }
        }
    }
    let free: Vec<bool> = wall.iter().zip(&blocked).map(|(w, b)| *w && !*b).collect();
    // Clear wall if there is enough; else the wall, painted round the windows.
    let (rect, around) = match best_rect(&free, cols, rows, lo.y) {
        Some(r) => (r, false),
        None => match best_rect(&wall, cols, rows, lo.y) {
            Some(r) => (r, true),
            None => return None,
        },
    };
    let (c0, r0, wc, hc, score) = rect;
    let setback = (front3.dot(out) - p.d).max(0.0);
    let mut score = score / (1.0 + (setback - SETBACK_FREE).max(0.0) / 8.0);
    if around {
        score *= 0.35 * free.iter().zip(&wall).filter(|(f, w)| **f && **w).count() as f32 / wall.iter().filter(|w| **w).count().max(1) as f32 + 0.05;
    }
    // The painted rectangle: centred in the clear stretch, a margin
    // round it, off the pavement.
    let rw = wc as f32 * WALL_CELL;
    let width = (rw - 0.3).min(MURAL_MAX_W);
    let mut bottom = lo.y + r0 as f32 * WALL_CELL + 0.15;
    if bottom < 0.3 {
        bottom = 0.3;
    }
    let top = (lo.y + (r0 + hc) as f32 * WALL_CELL - 0.15).min(bottom + MURAL_MAX_H);
    let height = top - bottom;
    if width < MURAL_MIN_W * 0.9 || height < MURAL_MIN_H * 0.8 {
        return None;
    }
    let left = lo.x + c0 as f32 * WALL_CELL + (rw - width) * 0.5;
    let origin = along * left + Vec3::Y * bottom + out * p.d;
    let mut spot = FacadeSpot::open(origin, along, out, width, height);
    for r in 0..spot.rows {
        for c in 0..spot.cols {
            let u = left + (c as f32 + 0.5) * WALL_CELL;
            let v = bottom + (r as f32 + 0.5) * WALL_CELL;
            let gc = (((u - lo.x) / WALL_CELL) as usize).min(cols - 1);
            let gr = (((v - lo.y) / WALL_CELL) as usize).min(rows - 1);
            spot.paint[r * spot.cols + c] = free[gr * cols + gc];
        }
    }
    if spot.clear_share() < 0.3 {
        return None;
    }
    Some((score, spot))
}

/// The paint: what a crew keeps from wall to wall.
#[derive(Clone, Debug)]
struct Crew {
    /// Background gradient ends, the motif's colours, a neon colour.
    bg: [[f32; 3]; 2],
    main: [[f32; 3]; 2],
    accent: [f32; 3],
    neon: [f32; 3],
    /// The signature motif (see [`Motif`]).
    motif: Motif,
    /// Rays behind the motif, a skyline or hills along the bottom.
    rays: bool,
    ground: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motif {
    Sun,
    Figure,
    Face,
    Eye,
    Waves,
    Rings,
}

const MOTIFS: [Motif; 6] = [Motif::Sun, Motif::Figure, Motif::Face, Motif::Eye, Motif::Waves, Motif::Rings];

/// Bright neon paint and deep grounds (linear).
pub(crate) const NEON: [[f32; 3]; 7] = [[0.95, 0.12, 0.45], [0.1, 0.75, 0.95], [0.98, 0.75, 0.08], [0.35, 0.95, 0.25], [0.6, 0.2, 0.95], [0.98, 0.35, 0.08], [0.1, 0.9, 0.75]];
pub(crate) const DEEP: [[f32; 3]; 6] = [[0.05, 0.03, 0.18], [0.02, 0.1, 0.22], [0.18, 0.02, 0.15], [0.03, 0.12, 0.1], [0.12, 0.04, 0.25], [0.2, 0.06, 0.04]];

impl Crew {
    fn of(crew: u64, pal: &Palette) -> Self {
        let (neon, deep) = (&pal.mural_neon, &pal.mural_deep);
        let mut rng = rng_for(crew, 0xC2E7);
        let main0 = pick(&mut rng, neon);
        let mut main1 = pick(&mut rng, neon);
        if main1 == main0 {
            main1 = at(neon, neon.iter().position(|c| *c == main0).unwrap_or(0) + 3);
        }
        Self {
            bg: [pick(&mut rng, deep), pick(&mut rng, deep)],
            main: [main0, main1],
            accent: pick(&mut rng, neon),
            neon: pick(&mut rng, neon),
            motif: MOTIFS[rng.random_range(0..MOTIFS.len())],
            rays: rng.random_bool(0.5),
            ground: rng.random_range(0..3),
        }
    }
}

/// One shape of the design, in metres on the wall.
#[derive(Clone, Debug)]
enum Shape {
    Disc { c: Vec2, r: f32 },
    Ellipse { c: Vec2, rx: f32, ry: f32 },
    Ring { c: Vec2, r: f32, w: f32 },
    Capsule { a: Vec2, b: Vec2, r: f32 },
    Rect { lo: Vec2, hi: Vec2 },
    Tri { a: Vec2, b: Vec2, c: Vec2 },
    Wave { v0: f32, amp: f32, k: f32, phase: f32, half: f32 },
    /// A synthwave sun: a disc cut by bars in its lower half.
    Sun { c: Vec2, r: f32 },
    Rays { c: Vec2, n: f32 },
}

impl Shape {
    fn contains(&self, p: Vec2) -> bool {
        match *self {
            Shape::Disc { c, r } => p.distance_squared(c) <= r * r,
            Shape::Ellipse { c, rx, ry } => {
                let d = (p - c) / Vec2::new(rx, ry);
                d.length_squared() <= 1.0
            }
            Shape::Ring { c, r, w } => (p.distance(c) - r).abs() <= w * 0.5,
            Shape::Capsule { a, b, r } => {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                p.distance_squared(a + ab * t) <= r * r
            }
            Shape::Rect { lo, hi } => p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y,
            Shape::Tri { a, b, c } => inside_tri(p, a, b, c),
            Shape::Wave { v0, amp, k, phase, half } => (p.y - (v0 + amp * (p.x * k + phase).sin())).abs() <= half,
            Shape::Sun { c, r } => {
                if p.distance_squared(c) > r * r {
                    return false;
                }
                let below = (c.y - p.y) / r;
                // Bars widen towards the bottom.
                below < 0.05 || ((below * 6.0).fract() > below * 0.55)
            }
            Shape::Rays { c, n } => {
                let a = (p - c).y.atan2((p - c).x) / std::f32::consts::TAU + 0.5;
                (a * n).fract() < 0.5
            }
        }
    }

    /// How far up the shape `p` is, `0..1`, for its gradient.
    fn rise(&self, p: Vec2) -> f32 {
        let (lo, hi) = match *self {
            Shape::Disc { c, r } | Shape::Sun { c, r } | Shape::Ring { c, r, .. } => (c.y - r, c.y + r),
            Shape::Ellipse { c, ry, .. } => (c.y - ry, c.y + ry),
            Shape::Capsule { a, b, r } => (a.y.min(b.y) - r, a.y.max(b.y) + r),
            Shape::Rect { lo, hi } => (lo.y, hi.y),
            Shape::Tri { a, b, c } => (a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y)),
            Shape::Wave { v0, amp, half, .. } => (v0 - amp - half, v0 + amp + half),
            Shape::Rays { .. } => return 0.5,
        };
        ((p.y - lo) / (hi - lo).max(1e-3)).clamp(0.0, 1.0)
    }
}

/// A shape with its paint: a gradient from `lo` at its bottom to `hi` at its top.
#[derive(Clone, Debug)]
struct Layer {
    shape: Shape,
    lo: [f32; 3],
    hi: [f32; 3],
    /// Mixed over what is beneath (`1`: covers it).
    alpha: f32,
}


/// A mural's design on a `w` by `h` wall: a background and its layers.
struct Design {
    bg: Option<([f32; 3], [f32; 3], Vec2)>,
    layers: Vec<Layer>,
    w: f32,
    h: f32,
}

impl Design {
    fn color(&self, p: Vec2) -> Option<[f32; 3]> {
        let mut col = self.bg.map(|(a, b, dir)| {
            let q = Vec2::new(p.x / self.w - 0.5, p.y / self.h - 0.5);
            mix(a, b, (q.dot(dir) + 0.5).clamp(0.0, 1.0))
        });
        for l in &self.layers {
            if l.shape.contains(p) {
                let c = mix(l.lo, l.hi, l.shape.rise(p));
                col = Some(match col {
                    Some(under) if l.alpha < 1.0 => mix(under, c, l.alpha),
                    _ => c,
                });
            }
        }
        col
    }
}

fn add(layers: &mut Vec<Layer>, shape: Shape, lo: [f32; 3], hi: [f32; 3]) {
    layers.push(Layer { shape, lo, hi, alpha: 1.0 });
}

/// Lay out a crew's design on a `w` by `h` wall.
fn design(crew: &Crew, w: f32, h: f32, rng: &mut impl Rng) -> Design {
    let s = w.min(h);
    let ang = rng.random_range(0.0..std::f32::consts::TAU);
    let mut layers = Vec::new();
    // The motif's middle: off to one side on a wide wall.
    let cx = if w > h * 1.3 { w * rng.random_range(0.28..0.72) } else { w * 0.5 };
    let cy = h * rng.random_range(0.5..0.62);
    let c = Vec2::new(cx, cy);
    let mut rays = None;
    if crew.rays {
        rays = Some(Layer { shape: Shape::Rays { c, n: rng.random_range(10.0..18.0f32).round() }, lo: crew.bg[1], hi: mul(crew.accent, 0.5), alpha: 0.35 });
    }
    // Ground: a skyline, hills or nothing.
    let mut ground = Vec::new();
    match crew.ground {
        0 => {
            let mut x = 0.0;
            while x < w {
                let bw = rng.random_range(0.5..1.4) * s * 0.12;
                let bh = rng.random_range(0.1..0.32) * h;
                ground.push(Shape::Rect { lo: Vec2::new(x, 0.0), hi: Vec2::new(x + bw, bh) });
                x += bw + rng.random_range(0.0..0.15) * s;
            }
        }
        1 => {
            let n = rng.random_range(2..5);
            for k in 0..n {
                let mx = w * (k as f32 + rng.random_range(0.2..0.8)) / n as f32;
                let mh = h * rng.random_range(0.18..0.38);
                let half = w / n as f32 * rng.random_range(0.6..1.0);
                ground.push(Shape::Tri { a: Vec2::new(mx - half, 0.0), b: Vec2::new(mx + half, 0.0), c: Vec2::new(mx, mh) });
            }
        }
        _ => {}
    }
    let dark = mul(crew.bg[0], 0.6);
    let r = s * rng.random_range(0.26..0.34);
    match crew.motif {
        Motif::Sun => {
            if let Some(l) = rays.take() {
                layers.push(l);
            }
            add(&mut layers, Shape::Sun { c, r }, crew.main[1], crew.main[0]);
        }
        Motif::Rings => {
            if let Some(l) = rays.take() {
                layers.push(l);
            }
            let n = rng.random_range(3..6);
            for k in 0..n {
                let rk = r * (1.0 - k as f32 / n as f32 * 0.8);
                let col = if k % 2 == 0 { crew.main[0] } else { crew.main[1] };
                add(&mut layers, Shape::Ring { c, r: rk, w: r * 0.12 }, col, mul(col, 0.8));
            }
            add(&mut layers, Shape::Disc { c, r: r * 0.15 }, crew.accent, crew.accent);
        }
        Motif::Face => {
            if let Some(l) = rays.take() {
                layers.push(l);
            }
            add(&mut layers, Shape::Disc { c, r }, crew.main[0], crew.main[1]);
            let eye = r * 0.16;
            for side in [-1.0, 1.0] {
                let e = c + Vec2::new(side * r * 0.38, r * 0.2);
                add(&mut layers, Shape::Ellipse { c: e, rx: eye * 1.3, ry: eye }, [0.95, 0.95, 0.9], [0.95, 0.95, 0.9]);
                add(&mut layers, Shape::Disc { c: e, r: eye * 0.55 }, dark, dark);
            }
            let m = c - Vec2::new(0.0, r * 0.38);
            add(&mut layers, Shape::Capsule { a: m - Vec2::new(r * 0.35, -r * 0.05), b: m + Vec2::new(r * 0.35, r * 0.05), r: r * 0.07 }, dark, dark);
        }
        Motif::Eye => {
            if let Some(l) = rays.take() {
                layers.push(l);
            }
            add(&mut layers, Shape::Ellipse { c, rx: r * 1.5, ry: r * 0.7 }, [0.92, 0.9, 0.88], [0.98, 0.96, 0.95]);
            add(&mut layers, Shape::Disc { c, r: r * 0.6 }, crew.main[0], crew.main[1]);
            add(&mut layers, Shape::Disc { c, r: r * 0.28 }, dark, dark);
            add(&mut layers, Shape::Disc { c: c + Vec2::new(r * 0.12, r * 0.14), r: r * 0.08 }, [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]);
        }
        Motif::Figure => {
            if let Some(l) = rays.take() {
                layers.push(l);
            }
            // A figure, arms up or out, standing on the ground.
            let fh = h * 0.8;
            let fx = cx;
            let head = Vec2::new(fx, fh * 0.88);
            let hip = Vec2::new(fx, fh * 0.42);
            let neck = Vec2::new(fx, fh * 0.74);
            let limb = fh * 0.055;
            let (lo, hi) = (crew.main[1], crew.main[0]);
            add(&mut layers, Shape::Capsule { a: hip, b: neck, r: limb * 1.6 }, lo, hi);
            add(&mut layers, Shape::Disc { c: head, r: fh * 0.085 }, hi, hi);
            let lift = rng.random_range(-0.2..1.0f32);
            for side in [-1.0, 1.0] {
                let sh = neck + Vec2::new(side * limb * 1.2, -limb);
                let hand = sh + Vec2::new(side * fh * 0.26, fh * 0.26 * lift);
                add(&mut layers, Shape::Capsule { a: sh, b: hand, r: limb }, lo, hi);
                add(&mut layers, Shape::Capsule { a: hip + Vec2::new(side * limb, 0.0), b: Vec2::new(fx + side * fh * 0.16, h * 0.04), r: limb * 1.1 }, lo, hi);
            }
        }
        Motif::Waves => {}
    }
    if let Some(l) = rays.take() {
        layers.insert(0, l);
    }
    if crew.motif == Motif::Waves || rng.random_bool(0.35) {
        let n = rng.random_range(2..5);
        for k in 0..n {
            let v0 = h * (0.15 + 0.6 * k as f32 / n as f32);
            let col = if k % 2 == 0 { crew.main[0] } else { crew.accent };
            add(&mut layers, Shape::Wave { v0, amp: h * 0.08, k: std::f32::consts::TAU / (w * rng.random_range(0.35..0.8)), phase: rng.random_range(0.0..6.0), half: h * 0.06 }, mul(col, 0.8), col);
        }
    }
    for g in ground {
        add(&mut layers, g, dark, mul(crew.bg[1], 0.8));
    }
    // A scatter of dots and stars.
    for _ in 0..rng.random_range(4..10) {
        let p = Vec2::new(rng.random_range(0.0..w), rng.random_range(h * 0.5..h));
        add(&mut layers, Shape::Disc { c: p, r: s * rng.random_range(0.012..0.03) }, crew.accent, crew.accent);
    }
    Design { bg: Some((crew.bg[0], crew.bg[1], Vec2::new(ang.cos(), ang.sin()))), layers, w, h }
}

/// 3 by 5 block letters, row by row from the top, bit 2 the left column.
fn glyph(ch: char) -> Option<[u8; 5]> {
    Some(match ch.to_ascii_uppercase() {
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [6, 1, 2, 4, 7],
        '3' => [6, 1, 2, 1, 6],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 6, 1, 6],
        '6' => [3, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 6],
        _ => return None,
    })
}

/// The crew's tag from its name: the first word that is not "the", in
/// capitals, at most eight letters.
pub fn crew_tag(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| !w.eq_ignore_ascii_case("the")).collect();
    let word = words.first().copied().unwrap_or(name);
    word.chars().filter(|c| glyph(*c).is_some()).take(8).collect::<String>().to_uppercase()
}

/// A quad on the wall from (`u0`, `v0`) to (`u1`, `v1`), `off` in front.
fn wall_quad(m: &mut MeshData, spot: &FacadeSpot, u0: f32, v0: f32, u1: f32, v1: f32, off: f32, col: [f32; 3]) {
    m.quad_facing([spot.at(u0, v0, off), spot.at(u1, v0, off), spot.at(u1, v1, off), spot.at(u0, v1, off)], spot.out, rgba(col));
}

/// A straight stroke on the wall, `w` wide.
fn stroke(m: &mut MeshData, spot: &FacadeSpot, a: Vec2, b: Vec2, w: f32, off: f32, col: [f32; 3]) {
    let d = (b - a).normalize_or_zero();
    if d == Vec2::ZERO {
        return;
    }
    let n = Vec2::new(-d.y, d.x) * (w * 0.5);
    let p = |q: Vec2| spot.at(q.x, q.y, off);
    m.quad_facing([p(a - n), p(b - n), p(b + n), p(a + n)], spot.out, rgba(col));
}

/// The design on a grid of vertex-coloured cells (gradients and soft
/// spray edges), skipping cells that are window.
fn paint_grid(m: &mut MeshData, spot: &FacadeSpot, d: &Design, off: f32, k: f32) {
    let cell = (spot.width.max(spot.height) / 40.0).max(0.15);
    let nx = (spot.width / cell).ceil().max(1.0) as usize;
    let ny = (spot.height / cell).ceil().max(1.0) as usize;
    let (cu, cv) = (spot.width / nx as f32, spot.height / ny as f32);
    let base = m.positions.len() as u32;
    let tangent = [spot.along.x, spot.along.y, spot.along.z, 1.0];
    let mut used = false;
    for j in 0..=ny {
        for i in 0..=nx {
            let (u, v) = (i as f32 * cu, j as f32 * cv);
            let c = d.color(Vec2::new(u, v)).unwrap_or([0.0; 3]);
            let p = spot.at(u, v, off);
            m.positions.push(p.to_array());
            m.normals.push(spot.out.to_array());
            m.colors.push(rgba(mul(c, k)));
            m.uvs.push([u, v]);
            m.tangents.push(tangent);
        }
    }
    let idx = |i: usize, j: usize| base + (j * (nx + 1) + i) as u32;
    for j in 0..ny {
        for i in 0..nx {
            if !spot.paintable((i as f32 + 0.5) * cu, (j as f32 + 0.5) * cv) {
                continue;
            }
            // Counter-clockwise seen from outside: along, then up.
            let (a, b, c, e) = (idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1));
            let n = (spot.along).cross(Vec3::Y);
            if n.dot(spot.out) > 0.0 {
                m.indices.extend_from_slice(&[a, b, c, a, c, e]);
            } else {
                m.indices.extend_from_slice(&[a, c, b, a, e, c]);
            }
            used = true;
        }
    }
    if !used {
        m.positions.truncate(base as usize);
        m.normals.truncate(base as usize);
        m.colors.truncate(base as usize);
        m.uvs.truncate(base as usize);
        m.tangents.truncate(base as usize);
    }
}

/// The crew's tag in block letters along the bottom or top of the mural,
/// with a drop shadow; pixels over a window are left out.
fn lettering(day: &mut MeshData, night: &mut MeshData, spot: &FacadeSpot, tag: &str, top: bool, col: [f32; 3]) {
    let glyphs: Vec<[u8; 5]> = tag.chars().filter_map(glyph).collect();
    if glyphs.is_empty() {
        return;
    }
    let cols = glyphs.len() as f32 * 4.0 - 1.0;
    let px = (spot.width * 0.8 / cols).min(spot.height * 0.16 / 5.0).min(0.35);
    if px < 0.06 {
        return;
    }
    let u0 = (spot.width - cols * px) * 0.5;
    // The band the letters go in must be clear wall: the preferred edge,
    // else the other one, else no lettering (a tag broken by windows reads
    // as noise).
    let band_clear = |v0: f32| {
        let steps = ((cols * px) / 0.25).ceil() as usize;
        (0..=steps).all(|i| (0..=4).all(|k| spot.paintable(u0 + i as f32 * cols * px / steps as f32, v0 + k as f32 * px * 1.25)))
    };
    let at_top = spot.height - px * 5.0 - spot.height * 0.06;
    let at_bottom = spot.height * 0.06;
    let (first, second) = if top { (at_top, at_bottom) } else { (at_bottom, at_top) };
    let v0 = if band_clear(first) {
        first
    } else if band_clear(second) {
        second
    } else {
        return;
    };
    let shadow = Vec2::new(px * 0.3, -px * 0.3);
    for (k, g) in glyphs.iter().enumerate() {
        for (row, bits) in g.iter().enumerate() {
            for c in 0..3 {
                if bits & (4 >> c) == 0 {
                    continue;
                }
                let u = u0 + (k as f32 * 4.0 + c as f32) * px;
                let v = v0 + (4 - row) as f32 * px;
                if !spot.paintable(u + px * 0.5, v + px * 0.5) {
                    continue;
                }
                wall_quad(day, spot, u + shadow.x, v + shadow.y, u + px + shadow.x, v + px + shadow.y, STANDOFF + 0.008, [0.02, 0.02, 0.03]);
                wall_quad(day, spot, u, v, u + px, v + px, STANDOFF + 0.014, col);
                wall_quad(night, spot, u, v, u + px, v + px, NIGHT_STANDOFF + 0.008, mul(col, 0.9));
            }
        }
    }
}

/// [social] A mural painted on `spot`: the day paint (Matte, with neon
/// strokes in Glow and two lamps over it) and the night paint (the same
/// design unlit and floodlit, for the app to show after dark). `crew`
/// seeds the crew's palette, motif and style, `seed` this mural's layout;
/// `quality` (`-1..1`) is art or a tag: poor crews scrawl.
/// [r6-perf] `quality` is read only against -0.2, 0.1 and 0.35: the
/// renderer keeps a painted facade while its band holds (`render/social.rs`).
pub fn paint_mural(spot: &FacadeSpot, quality: f32, seed: u64, crew: u64, tag: &str, pal: &Palette) -> (Geometry, Geometry) {
    let mut day = Geometry::default();
    let mut night = Geometry::default();
    let mut rng = rng_for(seed, 0x3A1A);
    let style = Crew::of(crew, pal);
    let (w, h) = (spot.width, spot.height);
    if quality < -0.2 {
        // A tag: scrawls and the crew's name on the bare wall.
        for _ in 0..rng.random_range(4..8) {
            let a = Vec2::new(rng.random_range(0.0..w), rng.random_range(0.3..(h * 0.7).max(0.4)));
            let b = a + Vec2::new(rng.random_range(0.6..2.2), rng.random_range(-0.4..0.4));
            if spot.paintable(a.x, a.y) && spot.paintable(b.x.min(w), b.y.clamp(0.0, h)) {
                stroke(day.part(Matte), spot, a, b, rng.random_range(0.1..0.25), STANDOFF, pick(&mut rng, &pal.mural_paint));
            }
        }
        lettering(day.part(Matte), night.part(Glow), spot, tag, false, style.main[0]);
        return (day, night);
    }
    let d = design(&style, w, h, &mut rng);
    paint_grid(day.part(Matte), spot, &d, STANDOFF, 1.0);
    // Floodlit at night: the same paint, unlit and a little dimmer.
    paint_grid(night.part(Glow), spot, &d, NIGHT_STANDOFF, 0.55);
    lettering(day.part(Matte), night.part(Glow), spot, tag, rng.random_bool(0.5), [0.98, 0.95, 0.85]);
    // Neon: strokes round the motif and a line or two across the good ones.
    if quality > 0.1 {
        let glow = mul(style.neon, 1.8);
        let n = rng.random_range(1..3);
        for k in 0..n {
            let v = h * (0.12 + 0.76 * (k as f32 + rng.random_range(0.2..0.8)) / n as f32);
            let u0 = w * rng.random_range(0.03..0.25);
            let u1 = w * rng.random_range(0.75..0.97);
            // Broken where a window is.
            let steps = ((u1 - u0) / 0.5).ceil() as usize;
            for s in 0..steps {
                let a = u0 + s as f32 * (u1 - u0) / steps as f32;
                let b = u0 + (s + 1) as f32 * (u1 - u0) / steps as f32;
                if spot.paintable((a + b) * 0.5, v) {
                    stroke(day.part(Glow), spot, Vec2::new(a, v), Vec2::new(b, v), 0.07, NIGHT_STANDOFF + 0.02, glow);
                }
            }
        }
        // A glowing frame round the painted area's clear corners.
        let segs = 48;
        let r = w.min(h) * 0.42;
        let c = Vec2::new(w * 0.5, h * 0.55);
        if quality > 0.35 {
            for s in 0..segs {
                let a0 = s as f32 / segs as f32 * std::f32::consts::TAU;
                let a1 = (s + 1) as f32 / segs as f32 * std::f32::consts::TAU;
                let (p0, p1) = (c + Vec2::new(a0.cos(), a0.sin()) * r, c + Vec2::new(a1.cos(), a1.sin()) * r);
                let mid = (p0 + p1) * 0.5;
                if mid.x > 0.1 && mid.x < w - 0.1 && mid.y > 0.1 && mid.y < h - 0.1 && spot.paintable(mid.x, mid.y) {
                    stroke(day.part(Glow), spot, p0, p1, 0.06, NIGHT_STANDOFF + 0.02, mul(style.accent, 1.6));
                }
            }
        }
    }
    // Two lamps on short arms over the top edge, lighting it at night.
    for t in [0.25, 0.75] {
        let p = spot.at(w * t, h + 0.25, 0.45);
        let yaw = spot.along.z.atan2(spot.along.x);
        day.part(Matte).boxed(p, Vec3::new(0.35, 0.16, 0.3), yaw, [0.08, 0.08, 0.09, 1.0]);
        day.part(Matte).boxed(spot.at(w * t, h + 0.25, 0.2), Vec3::new(0.06, 0.06, 0.45), yaw, [0.1, 0.1, 0.1, 1.0]);
        day.part(Glow).decal(p - Vec3::Y * 0.085, spot.along, spot.out, 0.3, 0.24, -Vec3::Y, [1.6, 1.5, 1.3, 1.0]);
    }
    (day, night)
}

/// A free-standing painted wall `width` x `height` at `pos` (on the
/// ground), running along `along`, its painted face towards `facing`: the
/// mural on a vacant lot (or where a building has no wall to the street).
/// Returns the day and night geometry, as [`paint_mural`].
#[allow(clippy::too_many_arguments)]
pub fn mural_wall(pos: Vec2, along: Vec2, facing: Vec2, width: f32, height: f32, quality: f32, seed: u64, crew: u64, tag: &str, pal: &Palette) -> (Geometry, Geometry) {
    let along = along.try_normalize().unwrap_or(Vec2::X);
    let mut out = facing.try_normalize().unwrap_or(Vec2::new(-along.y, along.x));
    out = (out - along * out.dot(along)).try_normalize().unwrap_or(Vec2::new(-along.y, along.x));
    let yaw = along.y.atan2(along.x);
    let thick = 0.3;
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let base = Vec3::new(pos.x, LOT_Y, pos.y);
    let mut wall = Geometry::default();
    // The wall itself, with a coping on top.
    wall.part(WornConcrete).boxed(base + Vec3::Y * (height * 0.5), Vec3::new(width, height, thick), yaw, rgba([0.42, 0.41, 0.4]));
    wall.part(Concrete).boxed(base + Vec3::Y * (height + 0.06), Vec3::new(width + 0.1, 0.12, thick + 0.12), yaw, rgba([0.5, 0.5, 0.48]));
    // The face as seen from the street: left to right is along `out` x up.
    let right = Vec3::new(o3.z, 0.0, -o3.x);
    let origin = base + o3 * (thick * 0.5) - right * (width * 0.5) + Vec3::Y * 0.1;
    let spot = FacadeSpot::open(origin, right, o3, width, height - 0.2);
    let (day, night) = paint_mural(&spot, quality, seed, crew, tag, pal);
    wall.append(&day);
    (wall, night)
}

#[cfg(test)]
mod mural_tests {
    use super::*;

    /// A box building `w` wide, `h` tall, its front face at z = 0 facing -z,
    /// with a row of windows (glass recessed, frames standing out) on every
    /// floor of its right half.
    fn block(w: f32, h: f32) -> Geometry {
        let mut g = Geometry::default();
        g.part(Plaster).boxed(Vec3::new(0.0, h * 0.5, 6.0), Vec3::new(w, h, 12.0), 0.0, [0.5; 4]);
        let mut y = 1.5;
        while y + 1.5 < h {
            let mut x = 1.0;
            while x + 1.0 < w * 0.5 {
                g.part(Glow).decal(Vec3::new(x, y, -0.03), Vec3::X, Vec3::Y, 1.2, 1.4, -Vec3::Z, [1.0; 4]);
                g.part(Matte).boxed(Vec3::new(x, y - 0.75, -0.05), Vec3::new(1.4, 0.1, 0.1), 0.0, [0.2, 0.2, 0.2, 1.0]);
                x += 2.5;
            }
            y += 3.0;
        }
        g
    }

    #[test]
    fn a_mural_goes_on_the_street_wall_clear_of_the_windows() {
        let g = block(24.0, 12.0);
        let spot = facade_spot(&g, Vec2::new(0.0, -4.0), Vec2::new(0.0, -1.0)).expect("a wall to paint");
        // On the front wall's plane, facing the street.
        assert!(spot.out.dot(Vec3::NEG_Z) > 0.99, "{:?}", spot.out);
        assert!(spot.origin.z.abs() < 0.01, "flush with the wall: {}", spot.origin.z);
        // The left half has no windows: it is painted, as big as fits.
        assert!(spot.width >= 8.0 && spot.height >= 8.0, "{} x {}", spot.width, spot.height);
        assert!(spot.height <= 12.0);
        assert_eq!(spot.clear_share(), 1.0);
        for u in [0.1, spot.width * 0.5, spot.width - 0.1] {
            let p = spot.at(u, 1.0, 0.0);
            assert!(p.x <= 0.2, "stays off the windowed half: {p:?}");
            assert!(p.x >= -12.01);
        }
        // Painted: day paint on the wall plane in front of it, a night copy.
        let (day, night) = paint_mural(&spot, 0.6, 7, 3, "VOLT", &Palette::default());
        assert!(day.get(Matte).positions.len() > 100);
        assert!(!night.get(Glow).is_empty());
        // (The lamps over the top stand out on arms.)
        let top = spot.origin.y + spot.height + 0.01;
        let zs: Vec<f32> = day.get(Matte).positions.iter().filter(|p| p[1] <= top).map(|p| p[2]).collect();
        assert!(zs.iter().all(|z| *z <= -0.04), "paint stands just off the wall");
    }

    #[test]
    fn a_windowed_wall_is_painted_round_its_windows() {
        // Windows across the whole front: no clear stretch.
        let mut g = Geometry::default();
        g.part(Brick).boxed(Vec3::new(0.0, 6.0, 6.0), Vec3::new(12.0, 12.0, 12.0), 0.0, [0.5; 4]);
        let mut y = 1.5;
        while y < 12.0 {
            let mut x = -5.0;
            while x < 5.5 {
                g.part(Glow).decal(Vec3::new(x, y, -0.03), Vec3::X, Vec3::Y, 1.0, 1.2, -Vec3::Z, [1.0; 4]);
                x += 2.0;
            }
            y += 2.4;
        }
        let spot = facade_spot(&g, Vec2::new(0.0, -4.0), Vec2::new(0.0, -1.0)).expect("still painted");
        let share = spot.clear_share();
        assert!(share > 0.3 && share < 1.0, "paint round the windows: {share}");
        let (day, _) = paint_mural(&spot, 0.6, 1, 1, "AB", &Palette::default());
        // No painted cell lies well inside a window.
        let m = day.get(Matte);
        let mut painted = 0;
        for t in m.indices.chunks_exact(3) {
            let c = t.iter().map(|i| Vec3::from_array(m.positions[*i as usize])).sum::<Vec3>() / 3.0;
            if c.z > -0.045 || c.z < -0.07 {
                continue;
            }
            painted += 1;
            let mut y = 1.5;
            while y < 12.0 {
                let mut x = -5.0;
                while x < 5.5 {
                    assert!((c.x - x).abs() > 0.5 - 0.35 || (c.y - y).abs() > 0.6 - 0.35, "paint over the window at ({x}, {y}): {c:?}");
                    x += 2.0;
                }
                y += 2.4;
            }
        }
        assert!(painted > 50);
    }

    #[test]
    fn nothing_to_paint_on_glass_or_backwards() {
        let mut g = Geometry::default();
        g.part(Glass).boxed(Vec3::new(0.0, 10.0, 6.0), Vec3::new(20.0, 20.0, 12.0), 0.0, [0.5; 4]);
        assert!(facade_spot(&g, Vec2::new(0.0, -4.0), Vec2::new(0.0, -1.0)).is_none(), "a glass tower has no wall to paint");
        let small = block(2.0, 2.0);
        assert!(facade_spot(&small, Vec2::new(0.0, -4.0), Vec2::new(0.0, -1.0)).is_none(), "too small");
    }

    #[test]
    fn a_crew_keeps_its_style_and_each_wall_its_own_layout() {
        let spot = FacadeSpot::open(Vec3::ZERO, Vec3::X, Vec3::NEG_Z, 10.0, 6.0);
        let (a, _) = paint_mural(&spot, 0.5, 11, 4, "NEON", &Palette::default());
        let (b, _) = paint_mural(&spot, 0.5, 11, 4, "NEON", &Palette::default());
        assert_eq!(a.get(Matte).colors, b.get(Matte).colors, "deterministic");
        let (c, _) = paint_mural(&spot, 0.5, 12, 4, "NEON", &Palette::default());
        assert_ne!(a.get(Matte).colors, c.get(Matte).colors, "another wall, another layout");
        assert_eq!(Crew::of(4, &Palette::default()).bg, Crew::of(4, &Palette::default()).bg);
        assert_eq!(crew_tag("The Volt Saints"), "VOLT");
        assert_eq!(crew_tag("Kru-9 Collective"), "KRU9");
        // A tag is scrawls and letters, no painted ground.
        let (t, _) = paint_mural(&spot, -0.6, 11, 4, "NEON", &Palette::default());
        assert!(t.get(Matte).positions.len() < a.get(Matte).positions.len() / 4);
    }
}
/// A community garden on a lot around `center`: rows of raised beds square
/// to `front` (the way to the street), a shed and a sign.
pub fn garden(center: Vec2, front: Vec2, size: f32, seed: u64) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 0x6A4D);
    let out = front.try_normalize().unwrap_or(Vec2::Y);
    let along = Vec2::new(-out.y, out.x);
    let yaw = along.y.atan2(along.x);
    let at = |u: f32, v: f32, h: f32| {
        let p = center + along * u + out * v;
        Vec3::new(p.x, LOT_Y + h, p.y)
    };
    let span = (size * 0.38).clamp(4.0, 14.0);
    let rows = ((span * 2.0 / 2.4) as i32).clamp(2, 6);
    let beds = ((span * 2.0 / 3.4) as i32).clamp(2, 5);
    let greens = [[0.08, 0.35, 0.06], [0.15, 0.45, 0.08], [0.05, 0.28, 0.1], [0.25, 0.4, 0.05]];
    let flowers = [[0.9, 0.2, 0.35], [0.95, 0.75, 0.1], [0.6, 0.2, 0.85], [0.95, 0.45, 0.1]];
    for r in 0..rows {
        let v = -span + 1.2 + r as f32 * 2.4;
        for b in 0..beds {
            let u = -span + 1.7 + b as f32 * 3.4;
            g.part(Wood).boxed_open(at(u, v, 0.2), Vec3::new(2.6, 0.4, 1.2), yaw, rgba([0.35, 0.22, 0.12]));
            g.part(Dirt).decal(at(u, v, 0.41), Vec3::new(along.x, 0.0, along.y), Vec3::new(out.x, 0.0, out.y), 2.4, 1.0, Vec3::Y, rgba([0.18, 0.12, 0.07]));
            // Plants: little tufts, now and then flowers.
            let n = rng.random_range(3..6);
            for k in 0..n {
                let du = -1.0 + k as f32 * 2.0 / (n - 1).max(1) as f32;
                let h = rng.random_range(0.2..0.6);
                let col = if rng.random_bool(0.25) { pick(&mut rng, &flowers) } else { pick(&mut rng, &greens) };
                g.part(Foliage).boxed(at(u + du, v + rng.random_range(-0.25..0.25), 0.4 + h * 0.5), Vec3::new(0.35, h, 0.35), yaw + k as f32, rgba(col));
            }
        }
    }
    // A shed, a water butt, a sign by the street.
    g.part(Wood).boxed(at(span - 1.2, span - 1.2, 1.1), Vec3::new(2.0, 2.2, 1.6), yaw, rgba([0.22, 0.32, 0.25]));
    g.part(Matte).cylinder(center + along * (span - 2.6) + out * (span - 0.8), 0.35, 0.35, LOT_Y, LOT_Y + 0.9, 8, rgba([0.1, 0.2, 0.12]));
    g.part(Wood).boxed(at(-span + 0.6, span + 0.4, 0.6), Vec3::new(0.1, 1.2, 0.1), yaw, rgba([0.3, 0.2, 0.1]));
    g.part(Matte).decal(at(-span + 0.6, span + 0.46, 1.25), Vec3::new(along.x, 0.0, along.y), Vec3::Y, 1.2, 0.55, Vec3::new(out.x, 0.0, out.y), rgba([0.9, 0.85, 0.6]));
    g
}
