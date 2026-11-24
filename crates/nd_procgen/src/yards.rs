//! [r12-yards] Where a household's things stand, and what they look like.
//!
//! `nd_sim::yards` says what a household puts out and whose it is. Here:
//!
//! - [`spots`] reads the drawn building (its geometry, before it is raised
//!   onto its pad) for the ground round it that is really free: a 0.5 m
//!   grid over the lot, every part that stands on the ground (walls,
//!   fences, sheds, trees, steps) and every roof or floor overhead marks
//!   its cells, and what is left inside the lot is open. From that the
//!   step by the front wall, the front garden or forecourt, the back yard,
//!   the pavement against a house built at the street, and on a block of
//!   flats its street-side windows (the switchable window lights).
//! - [`place`] stands each wish on a spot: deterministic (the order of the
//!   wishes and the spots, a hash for the look), so the same household on
//!   the same house always shows the same picture.
//! - [`prop_geometry`] draws each thing once, small procedural meshes the
//!   renderer instances wherever they stand: a bike, a child's bike on its
//!   side, a trike, a ball, toys, a hopscotch grid in chalk, pot plants and
//!   window boxes, a chair, washing on an airer, a line or a pole out of
//!   the window, a motorbike on a stand half taken apart, a workbench, a
//!   kettle grill or an oil-drum barbecue, a surfboard against the wall.
//! - [r13-detail] the washing on a building style's own lines and poles
//!   ([`WashLine`], `Spots::lines`): a style draws the line or the pole, a
//!   household with washing out hangs runs of it there ([`HUNG`]) before
//!   it would put out an airer or a pole of its own; so washing is only
//!   ever out by household and weather, never twice.
//! - [`pick_ray`]: which of them a click meets, so it leads to its owner.
//!
//! The meshes come from the shared prop library (`crate::props`: the one
//! bicycle, the pots, the garments), as the styles' do.

use crate::color::{mix, mul, rgba, DARK};
use crate::props::{self, append_xf, bicycle_local, garment, washing_run, wheel, Bike, Planter, CLOTH};
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface, WashLine};
use glam::{Mat3, Vec2, Vec3};
use nd_sim::citizens::{CitizenId, HouseholdId};
use nd_sim::hash::mix as hmix;
use nd_sim::yards::{Place, PropKind, Wish};
use nd_sim::zoning::point_in_polygon;
use nd_sim::Lot;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use Surface::{Foliage, Matte, Metal, Rust, Tarp, Wood};

/// Side of a cell of the free-ground grid, metres.
pub const CELL: f32 = 0.5;
/// Lots longer than this on a side get no ground spots (a park, a campus).
const MAX_SIDE: f32 = 90.0;
/// A part lower than this lies on the ground (lawn, paving, beds).
const GROUND_Y: f32 = 0.22;
/// A part reaching below this stands on the ground; one wholly above it
/// is overhead (a roof, an upper floor, a canopy, a tree's crown).
const LOW_Y: f32 = 2.2;
/// [r14-photofix] Farthest a garden or yard thing stands from the house:
/// metres beyond its front or back wall, and beside its side walls.
pub const YARD_REACH: f32 = 9.0;
/// How far round a standing part the ground is not free, metres.
const STANDOFF: f32 = 0.3;
/// Windows above this are too high to bother with.
pub const WINDOW_TOP: f32 = 30.0;
/// Most windows kept for a block.
const MAX_WINDOWS: usize = 160;
/// [r13-detail] Most style lines one home keeps for its washing.
const MAX_LINES: usize = 48;

/// A free place round a home: its foot (world x, height from the lot's
/// pad until [`Spots::raise`], world z), the way its things face (sim
/// heading: their +x, out from the wall towards the street), the free room
/// round it (radius, metres; a window's width), and whether it is on the
/// pavement outside the lot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub at: Vec3,
    pub yaw: f32,
    pub room: f32,
    pub pavement: bool,
}

/// The free places round one home.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spots {
    /// Against the front wall by the door, nearest the door first (or on
    /// the pavement against a house built at the street).
    pub step: Vec<Spot>,
    /// The front garden, drive or forecourt.
    pub front: Vec<Spot>,
    /// Behind the house, roomiest first.
    pub yard: Vec<Spot>,
    /// A block's street-side windows: the middle of each sill, facing out.
    pub windows: Vec<Spot>,
    /// [r13-detail] The building style's lines and poles washing may hang
    /// from ([`Geometry::washing`]).
    pub lines: Vec<WashLine>,
}

impl Spots {
    pub fn is_empty(&self) -> bool {
        self.step.is_empty() && self.front.is_empty() && self.yard.is_empty() && self.windows.is_empty() && self.lines.is_empty()
    }

    /// Lift the spots on the lot by its pad, and set the ones on the
    /// pavement at the height `pavement` gives for their point.
    pub fn raise(&mut self, pad: f32, pavement: impl Fn(Vec2) -> f32) {
        for s in self.step.iter_mut().chain(self.front.iter_mut()).chain(self.yard.iter_mut()).chain(self.windows.iter_mut()) {
            s.at.y = if s.pavement { pavement(Vec2::new(s.at.x, s.at.z)) } else { s.at.y + pad };
        }
        for l in self.lines.iter_mut() {
            l.a.y += pad;
            l.b.y += pad;
        }
    }

    /// Every spot, for the renderer's reach test.
    pub fn all(&self) -> impl Iterator<Item = &Spot> {
        self.step.iter().chain(self.front.iter()).chain(self.yard.iter()).chain(self.windows.iter())
    }
}

/// The free-ground grid of one lot, in its frontage frame: `u` along the
/// front (from its front-left corner), `v` into the lot (from the street).
struct Grid {
    nu: usize,
    c0: Vec2,
    u: Vec2,
    v: Vec2,
    /// Something stands here (a wall, a fence, a shed, a trunk, a step).
    solid: Vec<bool>,
    /// Something is overhead (a roof, a floor, a canopy).
    covered: Vec<bool>,
    /// Inside the lot, clear of its edge.
    inside: Vec<bool>,
}

impl Grid {
    fn idx(&self, i: usize, j: usize) -> usize {
        j * self.nu + i
    }

    /// The world point (sim plane) of a grid coordinate.
    fn world(&self, gu: f32, gv: f32) -> Vec2 {
        self.c0 + self.u * (gu * CELL) + self.v * (gv * CELL)
    }

    fn to_grid(&self, p: Vec2) -> Vec2 {
        let d = p - self.c0;
        Vec2::new(d.dot(self.u) / CELL, d.dot(self.v) / CELL)
    }

    fn open(&self, k: usize) -> bool {
        self.inside[k] && !self.solid[k] && !self.covered[k]
    }

    /// Mark the cells a triangle (grid coordinates) covers, or passes
    /// within `dil` cells of.
    fn raster(nu: usize, nv: usize, mark: &mut [bool], t: [Vec2; 3], dil: f32) {
        let lo = t[0].min(t[1]).min(t[2]) - Vec2::splat(dil);
        let hi = t[0].max(t[1]).max(t[2]) + Vec2::splat(dil);
        if hi.x < 0.0 || hi.y < 0.0 || lo.x > nu as f32 || lo.y > nv as f32 {
            return;
        }
        let (i0, j0) = (lo.x.floor().max(0.0) as usize, lo.y.floor().max(0.0) as usize);
        let (i1, j1) = ((hi.x.ceil() as usize).min(nu), (hi.y.ceil() as usize).min(nv));
        let area = (t[1] - t[0]).perp_dot(t[2] - t[0]);
        for j in j0..j1 {
            for i in i0..i1 {
                let p = Vec2::new(i as f32 + 0.5, j as f32 + 0.5);
                let inside = area.abs() > 1e-6 && {
                    let s = area.signum();
                    (0..3).all(|k| (t[(k + 1) % 3] - t[k]).perp_dot(p - t[k]) * s >= 0.0)
                };
                if inside || (0..3).any(|k| seg_dist(p, t[k], t[(k + 1) % 3]) <= dil) {
                    mark[j * nu + i] = true;
                }
            }
        }
    }
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-9 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

fn dist_to_outline(p: Vec2, poly: &[Vec2]) -> f32 {
    (0..poly.len()).map(|i| seg_dist(p, poly[i], poly[(i + 1) % poly.len()])).fold(f32::MAX, f32::min)
}

// `0..1` from a seed and a salt.
use nd_sim::hash::mix01 as h01;

/// Every mesh of a geometry, the base and both detail layers, but the glow.
fn solid_parts(g: &Geometry) -> impl Iterator<Item = &MeshData> {
    let base = g.parts.iter().zip(Surface::ALL).filter(|(_, s)| !s.is_glow()).map(|(m, _)| m);
    let detail = g.detail.iter().flat_map(|layer| layer.iter().zip(Surface::ALL).filter(|(_, s)| !s.is_glow()).map(|(m, _)| m));
    base.chain(detail)
}

/// The free places round a home drawn as `g` (world x/z, heights from its
/// pad) on `lot` (the building's footprint lot). `block`: a block of flats,
/// whose windows are wanted too. `seed` scatters the garden spots.
pub fn spots(g: &Geometry, lot: &Lot, block: bool, seed: u64) -> Spots {
    let mut out = Spots::default();
    let (c0, c1, c3) = (lot.corners[0], lot.corners[1], lot.corners[3]);
    let (w, d) = (c0.distance(c1), c0.distance(c3));
    let street = lot.front_dir.normalize_or_zero();
    let facing = street.y.atan2(street.x);
    if block {
        out.windows = windows(g, street);
    }
    out.lines = g.washing.iter().copied().filter(|l| l.len() > 0.5).take(MAX_LINES).collect();
    if w < 1.0 || d < 1.0 || w > MAX_SIDE || d > MAX_SIDE || street == Vec2::ZERO {
        return out;
    }
    let (nu, nv) = ((w / CELL).ceil() as usize, (d / CELL).ceil() as usize);
    let n = nu * nv;
    let mut grid = Grid { nu, c0, u: (c1 - c0) / w, v: (c3 - c0) / d, solid: vec![false; n], covered: vec![false; n], inside: vec![false; n] };
    for j in 0..nv {
        for i in 0..nu {
            let p = grid.world(i as f32 + 0.5, j as f32 + 0.5);
            let k = grid.idx(i, j);
            grid.inside[k] = point_in_polygon(p, &lot.polygon) && dist_to_outline(p, &lot.polygon) > 0.35;
        }
    }
    let dil = STANDOFF / CELL;
    for m in solid_parts(g) {
        for tri in m.indices.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| Vec3::from(m.positions[tri[k] as usize]));
            let (y0, y1) = (p[0].y.min(p[1].y).min(p[2].y), p[0].y.max(p[1].y).max(p[2].y));
            if y1 <= GROUND_Y {
                continue;
            }
            let t = p.map(|q| grid.to_grid(Vec2::new(q.x, q.z)));
            if y0 < LOW_Y {
                Grid::raster(nu, nv, &mut grid.solid, t, dil);
            } else {
                // Overhead: only what faces up or down (a roof, a slab), not
                // a wall's upper storeys (they are marked lower down).
                let nrm = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
                if nrm.y.abs() > 0.3 {
                    Grid::raster(nu, nv, &mut grid.covered, t, 0.0);
                }
            }
        }
    }
    // Free room round each open cell (a chamfer distance, metres).
    let mut room = vec![0.0f32; n];
    for k in 0..n {
        room[k] = if grid.open(k) { f32::MAX } else { 0.0 };
    }
    let (a, b) = (CELL, CELL * std::f32::consts::SQRT_2);
    for j in 0..nv {
        for i in 0..nu {
            let k = grid.idx(i, j);
            if room[k] == 0.0 {
                continue;
            }
            let mut r = room[k].min(if i == 0 || j == 0 || i + 1 == nu { a } else { f32::MAX });
            if i > 0 {
                r = r.min(room[k - 1] + a);
            }
            if j > 0 {
                r = r.min(room[k - nu] + a);
                if i > 0 {
                    r = r.min(room[k - nu - 1] + b);
                }
                if i + 1 < nu {
                    r = r.min(room[k - nu + 1] + b);
                }
            }
            room[k] = r;
        }
    }
    for j in (0..nv).rev() {
        for i in (0..nu).rev() {
            let k = grid.idx(i, j);
            if room[k] == 0.0 {
                continue;
            }
            let mut r = room[k].min(if i + 1 == nu || j + 1 == nv || i == 0 { a } else { f32::MAX });
            if i + 1 < nu {
                r = r.min(room[k + 1] + a);
            }
            if j + 1 < nv {
                r = r.min(room[k + nu] + a);
                if i + 1 < nu {
                    r = r.min(room[k + nu + 1] + b);
                }
                if i > 0 {
                    r = r.min(room[k + nu - 1] + b);
                }
            }
            room[k] = r;
        }
    }
    // The house: where the roofs are. Its front and back rows (a little in
    // from the extremes, so a porch canopy or a dormer does not count) and
    // its middle column, where the door most likely is.
    let mut rows: Vec<usize> = Vec::new();
    let mut cols: Vec<usize> = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            if grid.covered[grid.idx(i, j)] && grid.inside[grid.idx(i, j)] {
                rows.push(j);
                cols.push(i);
            }
        }
    }
    let pct = |v: &mut Vec<usize>, q: f32| -> Option<usize> {
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        Some(v[((v.len() - 1) as f32 * q).round() as usize])
    };
    let (front_row, back_row, door) = match (pct(&mut rows.clone(), 0.04), pct(&mut rows, 0.96), pct(&mut cols, 0.5)) {
        (Some(f), Some(b), Some(c)) => (f, b, c),
        // Nothing overhead: no house to stand against; the front half is
        // the front, the back half the yard.
        _ => (nv / 2, nv / 2, nu / 2),
    };
    // [r14-photofix] The house's side walls (`cols` is sorted by `pct`):
    // garden things stay within `YARD_REACH` of the house across the lot
    // too. On a big waterfront lot the roomiest open cells lay out in the
    // middle of the pad: Tide Towers' grill stood alone 30 m from the house
    // (photo report 2026-11-22, finding 3).
    let reach = (YARD_REACH / CELL).ceil() as usize;
    let (col_lo, col_hi) = (cols.first().copied().unwrap_or(0).saturating_sub(reach), cols.last().copied().unwrap_or(nu).saturating_add(reach));
    let spot_at = |i: usize, j: usize, yaw: f32, r: f32| Spot { at: { let p = grid.world(i as f32 + 0.5, j as f32 + 0.5); Vec3::new(p.x, 0.0, p.y) }, yaw, room: r, pavement: false };
    // The step: open cells just in front of the house with its wall right
    // behind them, nearest the door first.
    let mut step: Vec<(usize, usize, usize)> = Vec::new();
    if front_row > 0 {
        for j in front_row.saturating_sub(3)..front_row {
            for i in 0..nu {
                let k = grid.idx(i, j);
                if !grid.open(k) || room[k] < 0.4 {
                    continue;
                }
                let wall = (1..=2).any(|dj| j + dj < nv && { let q = grid.idx(i, j + dj); grid.solid[q] || grid.covered[q] });
                if wall {
                    step.push((i.abs_diff(door), i, j));
                }
            }
        }
    }
    step.sort_unstable();
    for (_, i, j) in step {
        let s = spot_at(i, j, facing, room[grid.idx(i, j)]);
        if out.step.len() < 4 && out.step.iter().all(|o| o.at.distance(s.at) >= 1.0) {
            out.step.push(s);
        }
    }
    // A house built at the street: its step is the pavement against it.
    if front_row as f32 * CELL < 1.3 {
        let mut along: Vec<(usize, usize)> = (0..nu).filter(|&i| (0..front_row.max(1) + 2).any(|j| j < nv && grid.covered[grid.idx(i, j)])).map(|i| (i.abs_diff(door), i)).collect();
        along.sort_unstable();
        let mut pav: Vec<Spot> = Vec::new();
        for (_, i) in along {
            let p = grid.world(i as f32 + 0.5, -0.9);
            let s = Spot { at: Vec3::new(p.x, 0.0, p.y), yaw: facing, room: 0.4, pavement: true };
            if pav.len() < 3 && pav.iter().all(|o| o.at.distance(s.at) >= 1.2) {
                pav.push(s);
            }
        }
        out.step.extend(pav);
    }
    // The front garden and the back yard: roomy open cells, scattered by a
    // hash, kept apart.
    let steps: Vec<Vec3> = out.step.iter().map(|s| s.at).collect();
    let pick = |from: usize, to: usize, min_room: f32, gap: f32, cap: usize, yaw: f32, salt: u64| -> Vec<Spot> {
        let mut cand: Vec<(u32, usize, usize)> = Vec::new();
        for j in from..to.min(nv) {
            for i in col_lo..nu.min(col_hi + 1) {
                let k = grid.idx(i, j);
                if grid.open(k) && room[k] >= min_room {
                    // Roomiest first, then the hash: a long spot holds a line.
                    let key = ((4.0 - room[k].min(4.0)) * 4.0) as u32 * 1000 + (h01(seed ^ salt, k as u64) * 999.0) as u32;
                    cand.push((key, i, j));
                }
            }
        }
        cand.sort_unstable();
        let mut got: Vec<Spot> = Vec::new();
        for (key, i, j) in cand {
            let jitter = (h01(seed, key as u64) - 0.5) * 0.9;
            let s = spot_at(i, j, yaw + jitter, room[grid.idx(i, j)]);
            if got.len() < cap && got.iter().all(|o| o.at.distance(s.at) >= gap) && steps.iter().all(|o| o.distance(s.at) >= 0.9) {
                got.push(s);
            }
        }
        got
    };
    out.front = pick(front_row.saturating_sub(1 + reach), front_row.saturating_sub(1), 0.6, 1.4, 8, facing, 0xf207);
    out.yard = pick(back_row + 2, back_row + 2 + reach, 0.7, 1.8, 8, facing + PI, 0xba4d);
    out
}

/// A block's street-side windows, from the switchable window lights
/// (`parts::window_light`): the middle of each sill, facing out, with the
/// window's width as its room, from the first floor up to [`WINDOW_TOP`].
fn windows(g: &Geometry, street: Vec2) -> Vec<Spot> {
    let m = g.get(Surface::Glow);
    let mark = crate::buildings::WINDOW_LIGHT_MARK;
    let mut out: Vec<Spot> = Vec::new();
    let mut group: Vec<u32> = Vec::new();
    let flush = |group: &mut Vec<u32>, out: &mut Vec<Spot>| {
        if group.len() < 3 {
            group.clear();
            return;
        }
        let pts: Vec<Vec3> = group.iter().map(|&i| Vec3::from(m.positions[i as usize])).collect();
        let nrm = group.iter().map(|&i| Vec3::from(m.normals[i as usize])).sum::<Vec3>().normalize_or_zero();
        group.clear();
        let flat = Vec2::new(nrm.x, nrm.z);
        if nrm.y.abs() > 0.2 || flat.length() < 0.5 {
            return;
        }
        let out_dir = flat.normalize();
        if out_dir.dot(street) < 0.35 {
            return;
        }
        let along = Vec2::new(-out_dir.y, out_dir.x);
        let (lo, hi) = pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let span = pts.iter().map(|p| Vec2::new(p.x, p.z).dot(along)).fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t), b.max(t)));
        let width = span.1 - span.0;
        if width < 0.5 || hi.y - lo.y < 0.6 || lo.y < 2.0 || lo.y > WINDOW_TOP {
            return;
        }
        let c = (lo + hi) * 0.5;
        let s = Spot { at: Vec3::new(c.x, lo.y, c.z), yaw: out_dir.y.atan2(out_dir.x), room: width, pavement: false };
        if out.iter().all(|o| o.at.distance(s.at) > 0.6) {
            out.push(s);
        }
    };
    for tri in m.indices.chunks_exact(3) {
        if m.colors.get(tri[0] as usize).is_none_or(|c| !(mark..mark + 1.0).contains(&c[3])) {
            flush(&mut group, &mut out);
            continue;
        }
        if !group.is_empty() && !tri.iter().any(|i| group.contains(i)) {
            flush(&mut group, &mut out);
        }
        for i in tri {
            if !group.contains(i) {
                group.push(*i);
            }
        }
        if out.len() >= MAX_WINDOWS {
            break;
        }
    }
    flush(&mut group, &mut out);
    // Lowest first, then along: flat numbers count up the block.
    out.sort_by(|a, b| a.at.y.total_cmp(&b.at.y).then(a.at.x.total_cmp(&b.at.x)).then(a.at.z.total_cmp(&b.at.z)));
    out
}

// ── placing ─────────────────────────────────────────────────────────────────

/// One thing stood on a spot: what it is and which of its looks, whose,
/// where its foot is (world, the pad's height added) and which way it
/// faces (sim heading of its +x).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub kind: PropKind,
    pub variant: u8,
    pub owner: CitizenId,
    pub household: HouseholdId,
    pub at: Vec3,
    pub yaw: f32,
}

/// Looks of each kind ([`prop_geometry`]'s `variant`).
pub fn variants(kind: PropKind) -> u8 {
    match kind {
        PropKind::Bike | PropKind::Surfboard => 4,
        PropKind::KidBike | PropKind::Ball | PropKind::Chair => 3,
        // Pots on the ground 0..3, window boxes 3..5.
        PropKind::Plant => 5,
        // An airer 0..3, a line 3..6, a pole from the window 6..9, a run
        // on a style's line 9..12.
        PropKind::Laundry => 12,
        PropKind::Trampoline => 1,
        _ => 2,
    }
}

/// The first window-box look of [`PropKind::Plant`].
pub const WINDOW_BOX: u8 = 3;
/// The first washing-line and window-pole looks of [`PropKind::Laundry`].
pub const LINE: u8 = 3;
pub const POLE: u8 = 6;
/// [r13-detail] The first look of a run of washing hung on a building
/// style's own line or pole ([`WashLine`]).
pub const HUNG: u8 = 9;
/// Most runs one line holds.
const MAX_RUNS: usize = 6;
/// Free room a washing line wants round its middle, metres.
const LINE_ROOM: f32 = 1.4;

/// Stand each wish on a spot of `spots`. A household's window is chosen
/// by its place in the building (`Wish::flat`) and `seed`; bikes and
/// boards of a block stand by its door. Wishes with nowhere to go are
/// left out. Deterministic.
pub fn place(wishes: &[Wish], spots: &Spots, seed: u64) -> Vec<Placed> {
    let mut step: Vec<Spot> = spots.step.clone();
    let mut front: Vec<Spot> = spots.front.clone();
    let mut yard: Vec<Spot> = spots.yard.clone();
    let mut flats: Vec<(u16, usize)> = Vec::new();
    let mut taken: Vec<bool> = vec![false; spots.windows.len()];
    let mut hung: Vec<bool> = vec![false; spots.lines.len()];
    let mut out = Vec::new();
    for w in wishes {
        let look = |n: u8, base: u8| base + (hmix(w.seed, 0x100c) % n as u64) as u8;
        // [r13-detail] Washing goes on the building's own lines and poles
        // first: a flat's on a pole, a house's on a line, else the other.
        if w.kind == PropKind::Laundry {
            if let Some(i) = free_line(&spots.lines, &hung, w.place == Place::Window, hmix(seed, w.flat as u64 ^ 0x11e5)) {
                hung[i] = true;
                let l = spots.lines[i];
                let d = l.b - l.a;
                let yaw = d.z.atan2(d.x) - FRAC_PI_2;
                let n = ((l.len() / props::RUN).floor() as usize).clamp(1, MAX_RUNS);
                for k in 0..n {
                    let v = HUNG + (hmix(w.seed, 0x40 + k as u64) % 3) as u8;
                    out.push(Placed { kind: w.kind, variant: v, owner: w.owner, household: w.household, at: l.at((k as f32 + 0.5) / n as f32), yaw });
                }
                continue;
            }
        }
        // A thing put down takes the ground round it: the spots within its
        // reach are dropped (a 3 m washing line, a bench and its sawhorse).
        let mut put = |s: Spot, variant: u8, yaw: f32, pools: [&mut Vec<Spot>; 3]| {
            let r = room_taken(w.kind, variant);
            for pool in pools {
                pool.retain(|o| o.pavement != s.pavement || o.at.distance(s.at) >= r);
            }
            out.push(Placed { kind: w.kind, variant, owner: w.owner, household: w.household, at: s.at, yaw });
        };
        let take = |pool: &mut Vec<Spot>| if pool.is_empty() { None } else { Some(pool.remove(0)) };
        let ground = |pool: &mut Vec<Spot>, other: &mut Vec<Spot>| take(pool).or_else(|| take(other));
        match (w.place, w.kind) {
            (Place::Window, PropKind::Plant | PropKind::Laundry) => {
                if spots.windows.is_empty() {
                    continue;
                }
                let n = spots.windows.len();
                let win = match flats.iter().find(|(f, _)| *f == w.flat) {
                    Some(&(_, i)) => i,
                    None => {
                        let mut i = (hmix(seed, w.flat as u64) % n as u64) as usize;
                        for _ in 0..n {
                            if !taken[i] {
                                break;
                            }
                            i = (i + 1) % n;
                        }
                        if taken[i] {
                            continue;
                        }
                        taken[i] = true;
                        flats.push((w.flat, i));
                        i
                    }
                };
                let s = spots.windows[win];
                let v = if w.kind == PropKind::Plant { look(2, WINDOW_BOX) } else { look(3, POLE) };
                put(s, v, s.yaw, [&mut step, &mut front, &mut yard]);
            }
            (Place::Window, _) | (Place::Step, _) => {
                if let Some(s) = ground(&mut step, &mut front) {
                    put(s, look(variants(w.kind).min(3), 0), s.yaw, [&mut step, &mut front, &mut yard]);
                }
            }
            (Place::Front, _) => {
                if let Some(s) = ground(&mut front, &mut step) {
                    put(s, look(variants(w.kind).min(3), 0), s.yaw, [&mut step, &mut front, &mut yard]);
                }
            }
            (Place::Yard, PropKind::Laundry) => {
                // The roomiest yard spot holds a line along the house; a small
                // one an airer.
                let best = yard.iter().enumerate().max_by(|a, b| a.1.room.total_cmp(&b.1.room).then(b.0.cmp(&a.0))).map(|(i, _)| i);
                let s = match best {
                    Some(i) => Some(yard.remove(i)),
                    None => take(&mut front),
                };
                if let Some(s) = s {
                    let line = s.room >= LINE_ROOM;
                    let yaw = if line { (s.yaw / FRAC_PI_2).round() * FRAC_PI_2 } else { s.yaw };
                    put(s, if line { look(3, LINE) } else { look(3, 0) }, yaw, [&mut step, &mut front, &mut yard]);
                }
            }
            (Place::Yard, PropKind::Trampoline) => {
                // Only in a back yard with room for it.
                let best = yard.iter().enumerate().filter(|(_, s)| s.room >= TRAMPOLINE_ROOM).max_by(|a, b| a.1.room.total_cmp(&b.1.room).then(b.0.cmp(&a.0))).map(|(i, _)| i);
                if let Some(i) = best {
                    let s = yard.remove(i);
                    put(s, 0, s.yaw, [&mut step, &mut front, &mut yard]);
                }
            }
            (Place::Yard, _) => {
                if let Some(s) = ground(&mut yard, &mut front) {
                    put(s, look(variants(w.kind).min(3), 0), s.yaw, [&mut step, &mut front, &mut yard]);
                }
            }
        }
    }
    out
}

/// The free line of `lines` (not `hung`) a household's washing goes on:
/// one of the kind asked for (`window`: a pole) first, from a place the
/// household's `pick` hash says, else one of the other kind.
fn free_line(lines: &[WashLine], hung: &[bool], window: bool, pick: u64) -> Option<usize> {
    let n = lines.len();
    if n == 0 {
        return None;
    }
    let start = (pick % n as u64) as usize;
    for want in [window, !window] {
        for k in 0..n {
            let i = (start + k) % n;
            if !hung[i] && lines[i].window == want {
                return Some(i);
            }
        }
    }
    None
}

/// A trampoline wants this much free room round its middle, metres.
const TRAMPOLINE_ROOM: f32 = 1.9;

/// The ground a thing takes round its foot: no other thing stands within
/// this, metres.
fn room_taken(kind: PropKind, variant: u8) -> f32 {
    match kind {
        PropKind::Laundry if (LINE..POLE).contains(&variant) => 2.1,
        PropKind::Trampoline => 2.3,
        PropKind::Tools => 1.7,
        PropKind::ProjectBike => 1.5,
        // A bike is 1.75 m from tyre to tyre: two stand side by side.
        PropKind::Bike => 1.9,
        PropKind::Surfboard | PropKind::Chalk | PropKind::KidBike => 1.2,
        PropKind::Grill | PropKind::Laundry => 1.0,
        _ => 0.7,
    }
}

/// Where a thing's middle is above its foot, and how near a click must
/// pass to it, metres (generous for the small ones).
pub fn reach(kind: PropKind, variant: u8) -> (f32, f32) {
    match kind {
        PropKind::Bike | PropKind::ProjectBike => (0.5, 0.75),
        PropKind::KidBike | PropKind::Trike | PropKind::Toys => (0.15, 0.55),
        PropKind::Ball => (0.11, 0.4),
        PropKind::Chalk => (0.0, 0.9),
        PropKind::Plant if variant >= WINDOW_BOX => (-0.05, 0.6),
        PropKind::Plant => (0.35, 0.5),
        PropKind::Chair => (0.45, 0.5),
        PropKind::Laundry if variant >= HUNG => (-0.3, 0.6),
        PropKind::Laundry if variant >= POLE => (-0.4, 0.8),
        PropKind::Trampoline => (0.8, 1.8),
        PropKind::Laundry if variant >= LINE => (1.3, 1.5),
        PropKind::Laundry => (0.6, 0.75),
        PropKind::Tools => (0.6, 0.85),
        PropKind::Grill => (0.55, 0.5),
        PropKind::Surfboard => (1.0, 0.6),
    }
}

/// Which of `props` (kind, look, foot) a ray from `o` along `d` passes
/// nearest within its [`reach`], and how far along: the index and `t`.
pub fn pick_ray(props: &[(PropKind, u8, Vec3)], o: Vec3, d: Vec3) -> Option<(usize, f32)> {
    let d = d.normalize_or_zero();
    let mut best: Option<(usize, f32, f32)> = None;
    for (i, &(kind, variant, at)) in props.iter().enumerate() {
        let (up, r) = reach(kind, variant);
        let c = at + Vec3::Y * up;
        let t = (c - o).dot(d);
        if t <= 0.0 {
            continue;
        }
        let miss = (o + d * t).distance(c);
        if miss > r {
            continue;
        }
        // The nearest along the ray, a near miss beating a far one only
        // when it is not much farther off.
        let score = t + miss * 2.0;
        if best.is_none_or(|b| score < b.2) {
            best = Some((i, t, score));
        }
    }
    best.map(|(i, t, _)| (i, t))
}

// ── the meshes ──────────────────────────────────────────────────────────────

/// Draw one look of a thing, in its own frame: its foot at the origin, +x
/// facing out (from the wall, towards the street), +y up, z along the wall.
/// A window's things (`Plant` from [`WINDOW_BOX`], `Laundry` from
/// [`POLE`]) hang off the wall plane at the origin, the sill.
pub fn prop_geometry(kind: PropKind, variant: u8) -> Geometry {
    let mut g = Geometry::default();
    let v = variant as u64;
    let seed = hmix(kind.index() as u64 * 131 + 7, v);
    match kind {
        PropKind::Bike => {
            const FRAMES: [[f32; 3]; 4] = [[0.42, 0.07, 0.05], [0.09, 0.18, 0.30], [0.11, 0.11, 0.12], [0.30, 0.34, 0.28]];
            let mut b = Geometry::default();
            bicycle_local(&mut b, Bike { fine: true, ..Bike::plain(FRAMES[v as usize % 4]) });
            // Leaning on the wall behind it.
            let lean = Mat3::from_rotation_z(0.16);
            append_xf(&mut g, &b, lean, Vec3::new(0.32, 0.0, 0.0));
        }
        PropKind::KidBike => {
            const FRAMES: [[f32; 3]; 3] = [[0.62, 0.12, 0.30], [0.12, 0.42, 0.62], [0.70, 0.55, 0.08]];
            let mut b = Geometry::default();
            bicycle_local(&mut b, Bike { scale: 0.58, fine: true, ..Bike::plain(FRAMES[v as usize % 3]) });
            // Stabilisers.
            let m = b.part(Matte);
            for s in [-1.0f32, 1.0] {
                m.tube(Vec3::new(0.0, 0.2, -0.3), Vec3::new(s * 0.17, 0.08, -0.32), 0.008, 3, rgba([0.4, 0.4, 0.42]));
                m.pipe(Vec2::new(s * 0.18, -0.32), Vec2::X, 0.03, 0.07, 0.07, 6, rgba(DARK));
            }
            // Dropped on its side.
            let side = Mat3::from_rotation_z(-FRAC_PI_2);
            append_xf(&mut g, &b, Mat3::from_rotation_y(h01(seed, 9) * 1.2 - 0.6) * side, Vec3::new(0.0, 0.2, 0.0));
        }
        PropKind::Trike => {
            let col = [[0.62, 0.10, 0.08], [0.10, 0.30, 0.62]][v as usize % 2];
            let m = g.part(Matte);
            m.boxed(Vec3::new(0.0, 0.22, 0.0), Vec3::new(0.16, 0.06, 0.45), 0.0, rgba(col));
            m.boxed(Vec3::new(0.0, 0.3, -0.08), Vec3::new(0.2, 0.05, 0.18), 0.0, rgba(DARK));
            m.tube(Vec3::new(0.0, 0.2, 0.2), Vec3::new(0.0, 0.42, 0.24), 0.018, 5, rgba(col));
            m.tube(Vec3::new(-0.16, 0.44, 0.24), Vec3::new(0.16, 0.44, 0.24), 0.014, 4, rgba([0.75, 0.75, 0.72]));
            m.pipe(Vec2::new(0.0, 0.26), Vec2::X, 0.05, 0.13, 0.13, 10, rgba([0.85, 0.85, 0.82]));
            for s in [-1.0f32, 1.0] {
                m.pipe(Vec2::new(s * 0.17, -0.2), Vec2::X, 0.04, 0.1, 0.1, 10, rgba([0.85, 0.85, 0.82]));
            }
            m.tube(Vec3::new(-0.17, 0.1, -0.2), Vec3::new(0.17, 0.1, -0.2), 0.012, 4, rgba(DARK));
        }
        PropKind::Ball => {
            let col = [[0.78, 0.78, 0.74], [0.62, 0.10, 0.08], [0.72, 0.36, 0.10]][v as usize % 3];
            let r = 0.11;
            g.part(Matte).lathe(Vec2::ZERO, &[(0.0, 0.0), (r * 0.7, r * 0.3), (r, r), (r * 0.7, r * 1.7), (0.0, 2.0 * r)], 8, rgba(col));
        }
        PropKind::Toys => {
            let m = g.part(Matte);
            let bright = [[0.70, 0.12, 0.10], [0.12, 0.38, 0.65], [0.80, 0.62, 0.10], [0.20, 0.52, 0.22]];
            m.cylinder(Vec2::new(0.25, 0.1), 0.08, 0.1, 0.0, 0.14, 8, rgba(bright[v as usize % 4]));
            m.boxed(Vec3::new(0.38, 0.015, 0.26), Vec3::new(0.05, 0.03, 0.22), 0.4, rgba(bright[(v as usize + 2) % 4]));
            for k in 0..4 {
                let q = Vec2::from_angle(k as f32 * 2.1 + v as f32) * (0.25 + 0.08 * k as f32);
                m.boxed(Vec3::new(q.x - 0.1, 0.04, q.y - 0.15), Vec3::splat(0.08), k as f32, rgba(bright[(k + v as usize) % 4]));
            }
            // A toy lorry.
            m.boxed(Vec3::new(-0.2, 0.08, 0.3), Vec3::new(0.3, 0.1, 0.13), 0.9, rgba(bright[(v as usize + 1) % 4]));
            m.boxed(Vec3::new(-0.27, 0.15, 0.22), Vec3::new(0.11, 0.08, 0.12), 0.9, rgba([0.2, 0.2, 0.22]));
        }
        PropKind::Chalk => {
            // A hopscotch grid in chalk running out towards the street.
            let col = [[0.78, 0.60, 0.66], [0.75, 0.72, 0.50], [0.58, 0.66, 0.78]];
            let m = g.part(Matte);
            let line = |m: &mut MeshData, a: Vec2, b: Vec2, c: [f32; 3]| {
                let d = (b - a).normalize_or_zero();
                let n = Vec2::new(-d.y, d.x) * 0.02;
                let p = |q: Vec2| Vec3::new(q.x, 0.012, q.y);
                m.quad([p(a - n), p(a + n), p(b + n), p(b - n)], [c[0], c[1], c[2], 1.0]);
            };
            let s = 0.42;
            let rows: [u8; 6] = [1, 1, 2, 1, 2, 1];
            for (r, &k) in rows.iter().enumerate() {
                let x0 = r as f32 * s - 1.2;
                let c = col[(r + v as usize) % 3];
                let half = k as f32 * s * 0.5;
                for (a, b) in [((x0, -half), (x0 + s, -half)), ((x0, half), (x0 + s, half)), ((x0, -half), (x0, half)), ((x0 + s, -half), (x0 + s, half))] {
                    line(m, Vec2::new(a.0, a.1), Vec2::new(b.0, b.1), c);
                }
                if k == 2 {
                    line(m, Vec2::new(x0, 0.0), Vec2::new(x0 + s, 0.0), c);
                }
            }
            // A sun scribbled by the side.
            let c = col[(v as usize + 1) % 3];
            let o = Vec2::new(0.2, 0.75);
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                line(m, o + Vec2::from_angle(a) * 0.12, o + Vec2::from_angle(a + TAU / 8.0) * 0.12, c);
                line(m, o + Vec2::from_angle(a) * 0.17, o + Vec2::from_angle(a) * 0.27, c);
            }
        }
        PropKind::Plant if variant >= WINDOW_BOX => {
            // A window box on two brackets under the sill.
            let wood = [[0.30, 0.20, 0.12], [0.22, 0.24, 0.26]][(variant - WINDOW_BOX) as usize % 2];
            let m = g.part(if variant == WINDOW_BOX { Wood } else { Metal });
            m.boxed(Vec3::new(0.14, -0.12, 0.0), Vec3::new(0.2, 0.18, 0.85), 0.0, rgba(wood));
            for s in [-0.3f32, 0.3] {
                g.part(Metal).tube(Vec3::new(0.01, -0.36, s), Vec3::new(0.2, -0.21, s), 0.012, 4, rgba(DARK));
            }
            let leaf = [0.13, 0.26, 0.10];
            for k in 0..4 {
                let z = -0.32 + k as f32 * 0.21;
                g.part(Foliage).lathe(Vec2::new(0.14, z), &[(0.09, -0.03), (0.13, 0.06), (0.08, 0.16), (0.0, 0.2)], 6, rgba(mix(leaf, [0.2, 0.32, 0.12], h01(seed, k))));
                if h01(seed, 10 + k) < 0.6 {
                    let fl = [[0.66, 0.10, 0.10], [0.80, 0.70, 0.30], [0.70, 0.40, 0.55]][(hmix(seed, k) % 3) as usize];
                    g.part(Matte).boxed(Vec3::new(0.2, 0.12, z + 0.04), Vec3::splat(0.05), 0.3, rgba(fl));
                }
            }
        }
        PropKind::Plant => {
            let pots = [[0.44, 0.21, 0.12], [0.38, 0.36, 0.34], [0.30, 0.16, 0.10]];
            let leaf = [0.14, 0.27, 0.10];
            let pot = pots[v as usize % 3];
            props::planter(&mut g, Vec2::new(0.25, 0.0), 0.0, Planter::Pot { seed }, 0.17, pot, leaf);
            props::planter(&mut g, Vec2::new(0.2, 0.42), 0.0, Planter::Pot { seed: seed ^ 1 }, 0.12, pots[(v as usize + 1) % 3], mix(leaf, [0.22, 0.32, 0.14], 0.5));
            if v != 1 {
                props::planter(&mut g, Vec2::new(0.45, -0.32), 0.0, Planter::Pot { seed: seed ^ 2 }, 0.1, pots[(v as usize + 2) % 3], leaf);
            }
        }
        PropKind::Chair => {
            // Faces the street, its back to the wall: a white plastic chair
            // gone grey, a wooden kitchen chair, a folding camp chair.
            let col = [[0.62, 0.62, 0.58], [0.32, 0.21, 0.12], [0.16, 0.28, 0.20]][v as usize % 3];
            let m = g.part(if v == 1 { Wood } else { Matte });
            let yaw = h01(seed, 3) * 0.5 - 0.25;
            let r = Mat3::from_rotation_y(yaw);
            let o = Vec3::new(0.42, 0.0, 0.0);
            m.boxed(o + r * Vec3::new(0.0, 0.44, 0.0), Vec3::new(0.44, 0.04, 0.44), -yaw, rgba(col));
            m.boxed(o + r * Vec3::new(-0.21, 0.68, 0.0), Vec3::new(0.04, 0.44, 0.42), -yaw, rgba(mul(col, 0.95)));
            for (x, z) in [(-0.19, -0.19), (-0.19, 0.19), (0.19, -0.19), (0.19, 0.19)] {
                m.tube(o + r * Vec3::new(x, 0.0, z), o + r * Vec3::new(x * 0.9, 0.43, z * 0.9), 0.016, 4, rgba(mul(col, 0.85)));
            }
        }
        PropKind::Laundry if variant >= HUNG => {
            // [r13-detail] A run of washing on a building style's own line
            // or pole, the line through the origin along z.
            washing_run(&mut g, (variant - HUNG) as usize, seed);
        }
        PropKind::Trampoline => {
            // A round mat on a blue-padded ring, on four legs.
            let r = 1.7;
            g.part(Tarp).cylinder(Vec2::ZERO, r, r, 0.85, 0.9, 14, rgba([0.10, 0.10, 0.11]));
            g.part(Matte).cylinder(Vec2::ZERO, r + 0.1, r + 0.1, 0.8, 0.86, 14, rgba([0.15, 0.36, 0.62]));
            for k in 0..4 {
                let d = Vec2::from_angle(k as f32 * FRAC_PI_2 + 0.4) * r;
                g.part(Metal).tube(Vec3::new(d.x, 0.0, d.y), Vec3::new(d.x, 0.82, d.y), 0.03, 4, rgba([0.40, 0.40, 0.42]));
            }
        }
        PropKind::Laundry if variant >= POLE => {
            // A pole out of the window, the washing pegged along it.
            let cl = CLOTH[(variant - POLE) as usize % 3];
            g.part(Metal).tube(Vec3::new(0.0, -0.15, 0.0), Vec3::new(1.35, -0.1, 0.0), 0.018, 5, rgba([0.40, 0.40, 0.38]));
            g.part(Metal).tube(Vec3::new(0.0, -0.75, 0.0), Vec3::new(0.7, -0.13, 0.0), 0.01, 4, rgba([0.30, 0.30, 0.30]));
            for k in 0..4u64 {
                let x = 0.3 + k as f32 * 0.28;
                garment(&mut g, Vec3::new(x, -0.12, 0.0), Vec3::X, 0.24, 0.42 + 0.12 * h01(seed, k), cl[k as usize % 5], hmix(seed, k));
            }
        }
        PropKind::Laundry if variant >= LINE => {
            // Two T-posts and a sagging line along the house, 3 m.
            let cl = CLOTH[(variant - LINE) as usize % 3];
            let post = rgba([0.36, 0.36, 0.34]);
            let half = 1.45;
            for s in [-half, half] {
                g.part(Metal).tube(Vec3::new(0.0, 0.0, s), Vec3::new(0.0, 1.85, s), 0.025, 5, post);
                g.part(Metal).tube(Vec3::new(-0.25, 1.82, s), Vec3::new(0.25, 1.82, s), 0.018, 4, post);
            }
            for x in [-0.22f32, 0.22] {
                g.part(Matte).wire(Vec3::new(x, 1.8, -half), Vec3::new(x, 1.8, half), 0.1, 0.012, rgba([0.6, 0.6, 0.58]));
                let n = 5;
                for k in 0..n {
                    let t = (k as f32 + 0.6 + h01(seed, k as u64 + 20 + (x > 0.0) as u64 * 9) * 0.4) / (n as f32 + 0.6);
                    if t > 0.95 {
                        continue;
                    }
                    let z = -half + t * 2.0 * half;
                    let sag = 0.1 * 4.0 * t * (1.0 - t);
                    let (w, drop) = (0.32 + 0.22 * h01(seed, k as u64 + 40), 0.4 + 0.3 * h01(seed, k as u64 + 50));
                    garment(&mut g, Vec3::new(x, 1.8 - sag, z), Vec3::Z, w, drop, cl[(k + (x > 0.0) as usize * 2) % 5], hmix(seed, k as u64 + 60));
                }
            }
        }
        PropKind::Laundry => {
            // A folding airer, the washing over its rails.
            let cl = CLOTH[v as usize % 3];
            let frame = rgba([0.62, 0.62, 0.60]);
            let m = g.part(Metal);
            for x in [-0.32f32, 0.32] {
                for z in [-0.55f32, 0.55] {
                    m.tube(Vec3::new(x, 0.0, z), Vec3::new(x * 0.15, 0.95, z), 0.012, 4, frame);
                }
                for y in [0.45f32, 0.75] {
                    let xx = x * (1.0 - y / 0.95 * 0.85);
                    m.tube(Vec3::new(xx, y, -0.55), Vec3::new(xx, y, 0.55), 0.008, 3, frame);
                }
            }
            m.tube(Vec3::new(0.0, 0.95, -0.55), Vec3::new(0.0, 0.95, 0.55), 0.01, 4, frame);
            for k in 0..3u64 {
                let z = -0.36 + k as f32 * 0.36;
                garment(&mut g, Vec3::new(0.02, 0.96, z), Vec3::Z, 0.3, 0.4 + 0.12 * h01(seed, k), cl[k as usize % 5], 2);
                let side = if k % 2 == 0 { -0.16 } else { 0.16 };
                garment(&mut g, Vec3::new(side, 0.76, z + 0.1), Vec3::Z, 0.26, 0.3, cl[(k as usize + 3) % 5], hmix(seed, k));
            }
        }
        PropKind::ProjectBike => {
            // A motorbike on a paddock stand, its front wheel off and lying
            // by it, the fork on a crate; oil tray, toolbox, a sheet of
            // cardboard under the lot.
            let tank = [[0.36, 0.08, 0.06], [0.10, 0.10, 0.11], [0.24, 0.28, 0.18]][v as usize % 2 + (seed as usize % 2)];
            let steel = rgba([0.42, 0.42, 0.44]);
            g.part(Matte).quad([Vec3::new(-0.7, 0.008, -1.0), Vec3::new(-0.7, 0.008, 1.0), Vec3::new(0.8, 0.008, 1.0), Vec3::new(0.8, 0.008, -1.0)], rgba([0.36, 0.29, 0.20]));
            let m = g.part(Metal);
            let rear = Vec3::new(0.0, 0.36, -0.62);
            wheel(m, rear, Vec3::Z, Vec3::Y, 0.31, 0.055, 12, rgba(DARK), steel);
            // The front wheel off, lying flat.
            wheel(m, Vec3::new(0.62, 0.06, 0.45), Vec3::Z, Vec3::X, 0.3, 0.055, 12, rgba(DARK), steel);
            m.boxed(Vec3::new(0.0, 0.42, -0.02), Vec3::new(0.28, 0.32, 0.42), 0.0, rgba([0.18, 0.18, 0.19]));
            m.boxed(Vec3::new(0.0, 0.78, 0.12), Vec3::new(0.28, 0.2, 0.5), 0.0, rgba(tank));
            m.boxed(Vec3::new(0.0, 0.8, -0.4), Vec3::new(0.26, 0.1, 0.5), 0.0, rgba(DARK));
            m.tube(Vec3::new(0.12, 0.32, 0.1), Vec3::new(0.16, 0.3, -0.85), 0.04, 6, rgba([0.30, 0.28, 0.26]));
            for (a, b) in [(Vec3::new(0.0, 0.68, -0.25), rear), (Vec3::new(0.0, 0.3, -0.1), rear), (Vec3::new(0.0, 0.7, 0.35), Vec3::new(0.0, 0.32, 0.55))] {
                m.tube(a, b, 0.022, 5, steel);
            }
            // The stand, the crate under the fork.
            m.tube(Vec3::new(-0.2, 0.0, -0.75), Vec3::new(-0.15, 0.3, -0.66), 0.015, 4, rgba([0.55, 0.12, 0.08]));
            m.tube(Vec3::new(0.2, 0.0, -0.75), Vec3::new(0.15, 0.3, -0.66), 0.015, 4, rgba([0.55, 0.12, 0.08]));
            g.part(Wood).boxed(Vec3::new(0.0, 0.15, 0.58), Vec3::new(0.36, 0.3, 0.3), 0.1, rgba([0.40, 0.30, 0.18]));
            g.part(Metal).boxed(Vec3::new(0.0, 0.02, 0.0), Vec3::new(0.45, 0.04, 0.4), 0.0, rgba([0.08, 0.08, 0.08]));
            g.part(Metal).boxed(Vec3::new(0.62, 0.11, -0.55), Vec3::new(0.24, 0.22, 0.46), 0.15, rgba([0.55, 0.10, 0.08]));
            g.part(Metal).boxed(Vec3::new(0.5, 0.02, -0.15), Vec3::new(0.2, 0.02, 0.03), 0.6, steel);
        }
        PropKind::Tools => {
            // A workbench with a vice and a sawhorse, offcuts against it.
            let wood = [[0.40, 0.29, 0.17], [0.30, 0.24, 0.17]][v as usize % 2];
            let w = g.part(Wood);
            w.boxed(Vec3::new(0.0, 0.88, 0.0), Vec3::new(0.65, 0.06, 1.5), 0.0, rgba(wood));
            w.boxed(Vec3::new(0.0, 0.25, 0.0), Vec3::new(0.55, 0.03, 1.35), 0.0, rgba(mul(wood, 0.8)));
            for (x, z) in [(-0.28, -0.68), (-0.28, 0.68), (0.28, -0.68), (0.28, 0.68)] {
                w.boxed(Vec3::new(x, 0.43, z), Vec3::new(0.07, 0.86, 0.07), 0.0, rgba(mul(wood, 0.85)));
            }
            // Sawhorse.
            w.boxed(Vec3::new(1.0, 0.62, 0.1), Vec3::new(0.09, 0.09, 1.0), 0.0, rgba(wood));
            for z in [-0.35f32, 0.55] {
                for s in [-1.0f32, 1.0] {
                    w.tube(Vec3::new(1.0, 0.6, z), Vec3::new(1.0 + s * 0.22, 0.0, z), 0.03, 4, rgba(mul(wood, 0.8)));
                }
            }
            w.boxed(Vec3::new(-0.42, 0.6, -0.3), Vec3::new(0.03, 1.2, 0.2), 0.0, rgba(mul(wood, 1.1)));
            let m = g.part(Metal);
            m.boxed(Vec3::new(0.18, 0.98, 0.62), Vec3::new(0.14, 0.14, 0.2), 0.0, rgba([0.16, 0.24, 0.36]));
            m.boxed(Vec3::new(0.05, 0.94, -0.3), Vec3::new(0.3, 0.06, 0.04), 0.4, rgba([0.40, 0.40, 0.42]));
            m.boxed(Vec3::new(-0.1, 0.93, 0.1), Vec3::new(0.05, 0.04, 0.28), 0.0, rgba([0.55, 0.12, 0.08]));
            m.boxed(Vec3::new(0.1, 0.11, -0.3), Vec3::new(0.24, 0.2, 0.42), 0.0, rgba([0.55, 0.10, 0.08]));
        }
        PropKind::Grill => {
            if v == 0 {
                // A kettle grill on three legs.
                let k = rgba([0.07, 0.07, 0.07]);
                let m = g.part(Metal);
                m.lathe(Vec2::ZERO, &[(0.06, 0.52), (0.22, 0.58), (0.28, 0.72), (0.29, 0.78)], 10, k);
                m.lathe(Vec2::ZERO, &[(0.29, 0.79), (0.27, 0.9), (0.18, 0.99), (0.0, 1.03)], 10, k);
                m.cylinder(Vec2::ZERO, 0.03, 0.03, 1.03, 1.07, 6, rgba([0.4, 0.4, 0.4]));
                for a in 0..3 {
                    let d = Vec2::from_angle(a as f32 * TAU / 3.0 + 0.5);
                    m.tube(Vec3::new(d.x * 0.18, 0.6, d.y * 0.18), Vec3::new(d.x * 0.3, 0.0, d.y * 0.3), 0.014, 4, rgba([0.45, 0.45, 0.46]));
                }
                m.cylinder(Vec2::ZERO, 0.16, 0.16, 0.22, 0.24, 8, rgba([0.4, 0.4, 0.4]));
            } else {
                // Half an oil drum on a frame, a chimney at the back.
                let r = g.part(Rust);
                r.pipe(Vec2::ZERO, Vec2::Y, 0.82, 0.27, 0.75, 10, rgba([0.30, 0.16, 0.09]));
                r.boxed(Vec3::new(0.0, 1.02, 0.0), Vec3::new(0.5, 0.02, 0.8), 0.0, rgba([0.08, 0.08, 0.08]));
                r.tube(Vec3::new(-0.15, 0.95, -0.35), Vec3::new(-0.15, 1.45, -0.35), 0.05, 6, rgba([0.18, 0.12, 0.08]));
                for (x, z) in [(-0.2, -0.3), (-0.2, 0.3), (0.2, -0.3), (0.2, 0.3)] {
                    r.tube(Vec3::new(x, 0.55, z), Vec3::new(x * 1.2, 0.0, z * 1.1), 0.018, 4, rgba([0.22, 0.20, 0.18]));
                }
            }
            g.part(Wood).boxed(Vec3::new(0.0, 0.62, 0.62), Vec3::new(0.4, 0.04, 0.4), 0.0, rgba([0.36, 0.26, 0.15]));
            g.part(Wood).boxed(Vec3::new(0.0, 0.31, 0.62), Vec3::new(0.06, 0.62, 0.06), 0.0, rgba([0.30, 0.22, 0.13]));
        }
        PropKind::Surfboard => {
            // A board 2.05 m long on its tail, leaning back on the wall.
            const COLS: [[f32; 3]; 4] = [[0.74, 0.72, 0.64], [0.78, 0.60, 0.16], [0.16, 0.48, 0.50], [0.62, 0.16, 0.12]];
            let col = COLS[v as usize % 4];
            let len = 2.05;
            let n = 14;
            let outline: Vec<Vec2> = (0..n)
                .map(|k| {
                    let t = k as f32 / n as f32 * TAU;
                    // y along the board 0..len, z across: a nose narrower
                    // than the tail.
                    let y = (1.0 - t.cos()) * 0.5 * len;
                    let w = 0.27 * t.sin() * (1.0 - 0.35 * (y / len).powi(2));
                    Vec2::new(y, w)
                })
                .collect();
            let mut b = Geometry::default();
            let ring = |x: f32| -> Vec<Vec3> { outline.iter().map(|p| Vec3::new(x, p.x, p.y)).collect() };
            b.part(Matte).frustum(&ring(-0.035), &ring(0.035), rgba(col));
            b.part(Matte).quad([Vec3::new(0.037, 0.3, -0.03), Vec3::new(0.037, 0.3, 0.03), Vec3::new(0.037, 1.9, 0.03), Vec3::new(0.037, 1.9, -0.03)], rgba(mul(col, 0.4)));
            append_xf(&mut g, &b, Mat3::from_rotation_z(0.2), Vec3::new(0.45, 0.0, 0.0));
        }
    }
    g
}

#[cfg(test)]
#[path = "yards_tests.rs"]
mod tests;
