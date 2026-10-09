//! Streamline placement (after Jobard and Lefer, as used for streets by
//! Chen et al.): streets are traced along the major or minor eigenvector
//! field from seed points, kept `dsep` apart from streets running the same
//! way and stopped when they come within `dtest` of one, leave the city,
//! reach water or steep ground. Arterials go first, then collectors fill
//! the gaps between them, then local streets fill the blocks.

use super::field::FieldRaster;
use glam::Vec2;
use nd_sim::landscape::{Landscape, WaterKind};
use rand::Rng;
use std::collections::VecDeque;

/// Road hierarchy, highest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Main = 0,
    Major = 1,
    Minor = 2,
}

/// Which eigenvector a street follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Major = 0,
    Minor = 1,
}

impl Family {
    fn other(self) -> Family {
        match self {
            Family::Major => Family::Minor,
            Family::Minor => Family::Major,
        }
    }
}

/// A traced street.
#[derive(Clone, Debug)]
pub struct Line {
    pub pts: Vec<Vec2>,
    pub level: Level,
    pub family: Family,
    /// A loop: the last point is the first.
    pub closed: bool,
}

impl Line {
    pub fn length(&self) -> f32 {
        nd_sim::geom::path_length(&self.pts)
    }
}

/// Spacing rules for one level of the hierarchy.
#[derive(Clone, Copy, Debug)]
pub struct LevelParams {
    /// Separation between streets of the same family: [major, minor].
    pub dsep: [f32; 2],
    /// A street stops when it comes this close to one running the same way.
    pub dtest: f32,
    /// Shorter streets are dropped.
    pub min_len: f32,
    pub max_len: f32,
    /// Random seeds tried after the seed queues run dry.
    pub tries: usize,
}

/// Integration step, metres.
pub const STEP: f32 = 4.0;
/// Keep this far back from water.
const WATER_MARGIN: f32 = 12.0;
/// Steepest ground a street is laid on (rise over run).
const MAX_SLOPE: f32 = 0.14;
/// Half-width of the strip about the railway a crossing street must clear.
const RAIL_BAND: f32 = 16.0;
/// Longest river crossing an arterial bridges.
const MAX_BRIDGE: f32 = 220.0;

/// Where the city may go.
pub struct Site<'a> {
    pub landscape: &'a Landscape,
    /// The city's footprint: a wobbly ellipse.
    pub centre: Vec2,
    pub radii: Vec2,
    pub wobble_seed: u64,
    /// Hard box (the buildable square, south of the highway).
    pub lo: Vec2,
    pub hi: Vec2,
    pub rail_y: Option<f32>,
    /// Centres the streets close up towards: (position, radius).
    pub cores: Vec<(Vec2, f32)>,
    /// Plazas: open squares that streets keep out of (`PLAZA_CLEAR` round
    /// each) until a few of them are run in to the centre at the end.
    pub plazas: Vec<Vec2>,
    /// The lowest class of street that may cross the railway (at grade).
    pub rail_cross: Level,
}

/// Traced streets stay this far from a plaza's centre.
pub const PLAZA_CLEAR: f32 = 82.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ground {
    Ok,
    /// On the railway's strip: arterials and collectors may cross it.
    Rail,
    Water,
    Out,
}

impl Site<'_> {
    pub fn inside(&self, p: Vec2) -> bool {
        if p.x < self.lo.x || p.y < self.lo.y || p.x > self.hi.x || p.y > self.hi.y {
            return false;
        }
        let d = (p - self.centre) / self.radii;
        let a = d.y.atan2(d.x);
        let wob = nd_sim::landscape::fbm(self.wobble_seed, Vec2::new(a.cos(), a.sin()) * 1.6 + Vec2::splat(5.0), 3) - 0.5;
        d.length() < 1.0 + wob * 0.35
    }

    /// 0..1: how central `p` is (1 at a core).
    pub fn core(&self, p: Vec2) -> f32 {
        self.cores.iter().map(|(c, r)| (-(p.distance(*c) / r).powi(2)).exp()).fold(0.0, f32::max)
    }

    pub fn steep(&self, p: Vec2) -> bool {
        let l = self.landscape;
        let e = 6.0;
        let gx = (l.height(p + Vec2::X * e) - l.height(p - Vec2::X * e)) / (2.0 * e);
        let gy = (l.height(p + Vec2::Y * e) - l.height(p - Vec2::Y * e)) / (2.0 * e);
        (gx * gx + gy * gy).sqrt() > MAX_SLOPE
    }

    pub fn wet(&self, p: Vec2, margin: f32) -> bool {
        !self.landscape.is_flat() && self.landscape.water_distance(p) < margin
    }

    pub fn ground(&self, p: Vec2) -> Ground {
        if !self.inside(p) || self.plazas.iter().any(|c| c.distance(p) < PLAZA_CLEAR) {
            return Ground::Out;
        }
        if self.wet(p, WATER_MARGIN) {
            return Ground::Water;
        }
        if self.steep(p) {
            return Ground::Out;
        }
        if self.rail_y.is_some_and(|y| (p.y - y).abs() < RAIL_BAND) {
            return Ground::Rail;
        }
        Ground::Ok
    }

    /// The separation multiplier: streets are closer together in the cores.
    pub fn density(&self, p: Vec2) -> f32 {
        1.0 - 0.22 * self.core(p)
    }
}

/// Points of placed streets, bucketed for distance queries, one grid per
/// family.
struct PointGrid {
    origin: Vec2,
    cell: f32,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<Vec2>>,
}

impl PointGrid {
    fn new(lo: Vec2, hi: Vec2, cell: f32) -> Self {
        let nx = ((hi.x - lo.x) / cell).ceil() as usize + 1;
        let ny = ((hi.y - lo.y) / cell).ceil() as usize + 1;
        Self { origin: lo, cell, nx, ny, cells: vec![Vec::new(); nx * ny] }
    }

    fn idx(&self, p: Vec2) -> (i64, i64) {
        let q = (p - self.origin) / self.cell;
        (q.x.floor() as i64, q.y.floor() as i64)
    }

    fn insert(&mut self, p: Vec2) {
        let (i, j) = self.idx(p);
        if i >= 0 && j >= 0 && (i as usize) < self.nx && (j as usize) < self.ny {
            let k = j as usize * self.nx + i as usize;
            self.cells[k].push(p);
        }
    }

    fn near(&self, p: Vec2, r: f32) -> bool {
        let (i0, j0) = self.idx(p - Vec2::splat(r));
        let (i1, j1) = self.idx(p + Vec2::splat(r));
        let r2 = r * r;
        for j in j0.max(0)..=j1.min(self.ny as i64 - 1) {
            for i in i0.max(0)..=i1.min(self.nx as i64 - 1) {
                if self.cells[j as usize * self.nx + i as usize].iter().any(|q| q.distance_squared(p) < r2) {
                    return true;
                }
            }
        }
        false
    }
}

pub struct Tracer<'a> {
    pub site: &'a Site<'a>,
    pub field: &'a FieldRaster,
    grids: [PointGrid; 2],
    pub lines: Vec<Line>,
}

impl<'a> Tracer<'a> {
    pub fn new(site: &'a Site<'a>, field: &'a FieldRaster) -> Self {
        let lo = site.lo - Vec2::splat(50.0);
        let hi = site.hi + Vec2::splat(50.0);
        Self { site, field, grids: [PointGrid::new(lo, hi, 25.0), PointGrid::new(lo, hi, 25.0)], lines: Vec::new() }
    }

    /// Place a street that did not come from the field (a waterfront road).
    pub fn add_line(&mut self, line: Line) {
        let g = &mut self.grids[line.family as usize];
        for w in line.pts.windows(2) {
            let n = (w[0].distance(w[1]) / STEP).ceil().max(1.0) as usize;
            for k in 0..n {
                g.insert(w[0].lerp(w[1], k as f32 / n as f32));
            }
        }
        if let Some(&p) = line.pts.last() {
            g.insert(p);
        }
        self.lines.push(line);
    }

    fn dir(&self, p: Vec2, fam: Family, prev: Vec2) -> Option<Vec2> {
        let t = self.field.sample(p);
        if t.norm() < 0.03 {
            return None;
        }
        let v = match fam {
            Family::Major => t.major(),
            Family::Minor => t.minor(),
        };
        Some(if v.dot(prev) < 0.0 { -v } else { v })
    }

    /// One fourth-order Runge-Kutta step along the field.
    fn step(&self, p: Vec2, fam: Family, prev: Vec2) -> Option<Vec2> {
        let h = STEP;
        let k1 = self.dir(p, fam, prev)?;
        let k2 = self.dir(p + k1 * h * 0.5, fam, k1)?;
        let k3 = self.dir(p + k2 * h * 0.5, fam, k1)?;
        let k4 = self.dir(p + k3 * h, fam, k1)?;
        let d = (k1 + 2.0 * k2 + 2.0 * k3 + k4).normalize_or_zero();
        // A turn this sharp in one step means a degenerate point.
        (d.dot(prev) > 0.75).then_some(d)
    }

    /// Carry an arterial straight over a river ahead of `p` going `d`, if
    /// the far bank is near and the crossing is square enough. Returns the
    /// deck's points, the far bank last.
    fn bridge(&self, p: Vec2, d: Vec2, level: Level) -> Option<Vec<Vec2>> {
        let land = self.site.landscape;
        let mut pts = Vec::new();
        let mut s = 0.0;
        let mut seen_river = false;
        while s < MAX_BRIDGE {
            s += STEP;
            let q = p + d * s;
            if let Some(hit) = land.water_at(q) {
                // Arterials bridge rivers; any street may bridge a stream.
                let crossable = match hit.kind {
                    WaterKind::River => level == Level::Main,
                    WaterKind::Stream => level == Level::Main || hit.width <= 25.0,
                    _ => false,
                };
                if !crossable || hit.flow.dot(d).abs() > 0.55 {
                    return None;
                }
                seen_river = true;
            }
            pts.push(q);
            if seen_river && !self.site.wet(q, WATER_MARGIN + 2.0) {
                return (self.site.ground(q) == Ground::Ok).then_some(pts);
            }
            if !self.site.inside(q) {
                return None;
            }
        }
        None
    }

    /// Trace one half of a street from `seed` going `d0`. `own` holds the
    /// points already traced for this street with their arc position.
    fn trace_half(&self, seed: Vec2, d0: Vec2, fam: Family, level: Level, lp: &LevelParams, own: &[(Vec2, f32)], sign: f32) -> (Vec<Vec2>, bool) {
        let mut out = Vec::new();
        let mut p = seed;
        let mut d = d0;
        let mut s = 0.0;
        let mut bridges = 0;
        while s < lp.max_len * 0.5 {
            let Some(nd) = self.step(p, fam, d) else { break };
            let next = p + nd * STEP;
            match self.site.ground(next) {
                Ground::Ok => {}
                Ground::Rail => {
                    // Only the classes the site lets cross (collectors and
                    // arterials, or arterials alone), and only squarely.
                    if level > self.site.rail_cross || nd.y.abs() < 0.85 {
                        break;
                    }
                }
                Ground::Water => {
                    if bridges >= 3 {
                        break;
                    }
                    let Some(deck) = self.bridge(p, nd, level) else { break };
                    bridges += 1;
                    s += deck.len() as f32 * STEP;
                    p = *deck.last().unwrap();
                    out.extend(deck);
                    continue;
                }
                Ground::Out => break,
            }
            let dtest = lp.dtest * self.site.density(next);
            if self.grids[fam as usize].near(next, dtest) {
                break;
            }
            s += STEP;
            // Back round to its own start: a ring closes.
            let cur = s * sign;
            let mut closes = false;
            let mut blocked = false;
            for &(q, sq) in own {
                if (cur - sq).abs() > dtest * 3.0 && q.distance(next) < dtest * 0.7 {
                    if sq.abs() < STEP * 1.5 && s > 200.0 {
                        closes = true;
                    } else {
                        blocked = true;
                    }
                    break;
                }
            }
            if closes {
                out.push(seed);
                return (out, true);
            }
            if blocked {
                break;
            }
            if out.len() >= 2 {
                // Own recent points on this half.
                let near_self = out[..out.len().saturating_sub((dtest * 3.0 / STEP) as usize)].iter().any(|q: &Vec2| q.distance(next) < dtest * 0.7);
                if near_self {
                    break;
                }
            }
            out.push(next);
            p = next;
            d = nd;
        }
        (out, false)
    }

    /// Trace a whole street through `seed`, both ways.
    pub fn trace(&self, seed: Vec2, fam: Family, level: Level, lp: &LevelParams) -> Option<Line> {
        let t = self.field.sample(seed);
        if t.norm() < 0.03 {
            return None;
        }
        let d0 = match fam {
            Family::Major => t.major(),
            Family::Minor => t.minor(),
        };
        let (fwd, closed) = self.trace_half(seed, d0, fam, level, lp, &[(seed, 0.0)], 1.0);
        let mut pts = Vec::new();
        if closed {
            pts.push(seed);
            pts.extend(fwd);
        } else {
            let own: Vec<(Vec2, f32)> = std::iter::once((seed, 0.0)).chain(fwd.iter().enumerate().map(|(i, &p)| (p, (i + 1) as f32 * STEP))).collect();
            let (back, _) = self.trace_half(seed, -d0, fam, level, lp, &own, -1.0);
            pts.extend(back.into_iter().rev());
            pts.push(seed);
            pts.extend(fwd);
        }
        let line = Line { pts, level, family: fam, closed };
        (line.pts.len() >= 2 && line.length() >= lp.min_len).then_some(line)
    }

    fn seed_ok(&self, p: Vec2, fam: Family, lp: &LevelParams) -> bool {
        self.site.ground(p) == Ground::Ok && !self.grids[fam as usize].near(p, lp.dsep[fam as usize] * self.site.density(p) * 0.9)
    }

    /// Seeds a new street of `fam` sets up: points along it for streets
    /// across it, and points a separation away on either side for streets
    /// alongside it.
    fn seeds_from(&self, line: &Line, lp: &LevelParams, queues: &mut [VecDeque<Vec2>; 2]) {
        let fam = line.family;
        let along = (lp.dsep[fam.other() as usize] * 0.5 / STEP).max(1.0) as usize;
        for (i, w) in line.pts.windows(2).enumerate() {
            if i % along != 0 {
                continue;
            }
            let d = (w[1] - w[0]).normalize_or_zero();
            let n = Vec2::new(-d.y, d.x);
            queues[fam.other() as usize].push_back(w[0]);
            let off = lp.dsep[fam as usize] * self.site.density(w[0]);
            queues[fam as usize].push_back(w[0] + n * off);
            queues[fam as usize].push_back(w[0] - n * off);
        }
    }

    /// Fill the city with streets of one level. `anchors` seed the first
    /// streets (arterials through the centres).
    pub fn fill(&mut self, level: Level, lp: &LevelParams, anchors: &[Vec2], rng: &mut impl Rng) {
        let mut queues: [VecDeque<Vec2>; 2] = [VecDeque::new(), VecDeque::new()];
        for &a in anchors {
            queues[0].push_back(a);
            queues[1].push_back(a);
        }
        // Streets already placed (higher levels) seed this one.
        let existing: Vec<Line> = self.lines.clone();
        for l in &existing {
            self.seeds_from(l, lp, &mut queues);
        }
        let mut fails = 0;
        let mut turn = 0usize;
        while fails < lp.tries {
            turn += 1;
            let pick = if !queues[turn % 2].is_empty() {
                turn % 2
            } else if !queues[(turn + 1) % 2].is_empty() {
                (turn + 1) % 2
            } else {
                2
            };
            let (fam, seed) = if pick < 2 {
                (if pick == 0 { Family::Major } else { Family::Minor }, queues[pick].pop_front().unwrap())
            } else {
                fails += 1;
                let p = Vec2::new(rng.random_range(self.site.lo.x..self.site.hi.x), rng.random_range(self.site.lo.y..self.site.hi.y));
                (if turn % 2 == 0 { Family::Major } else { Family::Minor }, p)
            };
            if !self.seed_ok(seed, fam, lp) {
                continue;
            }
            if let Some(line) = self.trace(seed, fam, level, lp) {
                self.seeds_from(&line, lp, &mut queues);
                self.add_line(line);
            }
        }
    }
}
