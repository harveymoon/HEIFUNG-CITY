//! The built ground: the terrain graded round the roads, embankments,
//! underpasses, terraces, retaining walls and decks.

use super::*;

/// Cell of the built-ground raster, in metres.
pub const GROUND_CELL: f32 = 4.0;

/// Within this many metres of a road's edge the ground blends from the road
/// back to the natural terrain.
pub const GROUND_FALLOFF: f32 = 22.0;

/// Side slopes of embankments and cuttings: metres across per metre up. The
/// blend from a road to the terrain widens with the height to make up so
/// that no side is steeper than this.
pub const EMBANKMENT_SLOPE: f32 = 2.5;

/// Least height of one way over another for an underpass: headroom and deck.
pub const UNDERPASS_CLEARANCE: f32 = 4.5;

/// Margin of an underpass cut beyond the lower way's corridor: one cell of
/// the drawn terrain (8 m), so its slope never reaches the pavement.
pub const UNDERPASS_MARGIN: f32 = 8.0;

/// Where two ways cross without a junction (`RoadNetwork::grade_crossings`:
/// a street under the highway, a street drawn over another) and neither is
/// on a deck of its own: the upper way spans the lower on a short deck, and
/// the ground under it is cut down to the lower way between abutments.
/// Found by `Ground::build`; the renderer draws the deck and the abutments.
#[derive(Clone, Debug)]
pub struct Underpass {
    /// Where the centrelines cross.
    pub at: Vec2,
    /// Unit directions of the way passing over and of the one passing under.
    pub over_dir: Vec2,
    pub under_dir: Vec2,
    /// Their heights at the crossing.
    pub over_z: f32,
    pub under_z: f32,
    /// Change of height per metre along each way (the upper way's beyond
    /// the crossing, along `over_dir`).
    pub over_grade: f32,
    pub under_grade: f32,
    /// The upper way's change of height per metre before the crossing: it
    /// differs from `over_grade` where the crossing is the crest of a way
    /// lifted over the lower one.
    pub over_grade_back: f32,
    /// Half the width of the upper carriageway (with its shoulder or pavement).
    pub over_half_width: f32,
    /// Half the width of its corridor (with the whole pavement or verge).
    pub over_corridor_half_width: f32,
    /// Half the width of the lower way's corridor (with its pavement).
    pub under_half_width: f32,
    /// Half the width of the cut across the lower way, and half its length
    /// along it (the upper way's embankment and its slopes).
    pub cut_half_width: f32,
    pub cut_half_length: f32,
}

impl Underpass {
    /// Sine of the crossing angle (1 when square), kept away from zero.
    pub fn sin_angle(&self) -> f32 {
        self.over_dir.perp_dot(self.under_dir).abs().max(0.25)
    }

    /// Position in the crossing's frame: (along the lower way, across it).
    pub fn local(&self, p: Vec2) -> (f32, f32) {
        let d = p - self.at;
        (d.dot(self.under_dir), d.perp_dot(self.under_dir))
    }

    /// Height of the lower way `along` metres from the crossing.
    pub fn under_z_at(&self, along: f32) -> f32 {
        self.under_z + self.under_grade * along
    }

    /// Height of the upper way `along` metres from the crossing.
    pub fn over_z_at(&self, along: f32) -> f32 {
        self.over_z + if along < 0.0 { self.over_grade_back } else { self.over_grade } * along
    }

    /// Inside the cut: the ground here is the lower way's.
    pub fn in_cut(&self, p: Vec2) -> bool {
        let (along, across) = self.local(p);
        along.abs() <= self.cut_half_length && across.abs() <= self.cut_half_width
    }
}

/// How far beyond its corridor a way standing `rise` metres above (or below)
/// the terrain blends back into it: wide enough that the steepest part of
/// the blend (a smoothstep is steepest at one and a half times its mean
/// slope) is no steeper than `EMBANKMENT_SLOPE`; at least `GROUND_FALLOFF`,
/// at most 160 m.
pub fn embankment_falloff(rise: f32) -> f32 {
    GROUND_FALLOFF.max(rise * EMBANKMENT_SLOPE * 1.5).min(160.0)
}

/// Half the corridor a way grades the ground for: carriageway plus pavement
/// (streets) or a metre of verge.
pub(super) fn corridor_half_width(seg: &crate::roads::Segment) -> f32 {
    seg.half_width() + if seg.is_street() { crate::zoning::SIDEWALK } else { 1.0 }
}

/// Every grade-separated crossing where neither way is on a deck of its
/// own (a viaduct or a bridge over water) and one runs well above the other.
pub(super) fn find_underpasses(land: &Landscape, roads: &RoadNetwork, node_z: &[f32]) -> Vec<Underpass> {
    use crate::roads::SegmentId;
    let z = |n: crate::roads::NodeId| node_z.get(n.0 as usize).copied().unwrap_or(0.0);
    let seg_z = |sid: SegmentId, t: f32| {
        let s = roads.segment(sid);
        z(s.a) + (z(s.b) - z(s.a)) * t
    };
    // Height change per metre along `dir` leaving node `n` by a way of the
    // same kind other than `not`, if there is one.
    let beyond = |n: crate::roads::NodeId, not: SegmentId, kind: crate::roads::RoadKind| -> Option<f32> {
        let p = roads.node(n).pos;
        roads.node(n).segments.iter().copied().filter(|&o| o != not).find(|&o| roads.segment(o).alive && roads.segment(o).kind == kind).map(|o| {
            let s = roads.segment(o);
            let far = if s.a == n { s.b } else { s.a };
            (z(far) - z(n)) / roads.node(far).pos.distance(p).max(1e-3)
        })
    };
    let mut out = Vec::new();
    for c in roads.grade_crossings() {
        let (a, b) = (roads.segment(c.upper.0), roads.segment(c.lower.0));
        // Decks and tunnels need no cut: the deck spans, the tunnel is bored.
        if a.bridge || b.bridge || a.tunnel || b.tunnel {
            continue;
        }
        // Whichever is actually higher passes over: a street on a hill
        // crosses the highway in its valley on the deck.
        let (over, under) = if seg_z(c.upper.0, c.upper.1) >= seg_z(c.lower.0, c.lower.1) { (c.upper, c.lower) } else { (c.lower, c.upper) };
        let (over_z, under_z) = (seg_z(over.0, over.1), seg_z(under.0, under.1));
        if over_z - under_z < UNDERPASS_CLEARANCE - 0.05 {
            continue;
        }
        let (os, us) = (roads.segment(over.0), roads.segment(under.0));
        let (oa, ob) = roads.segment_points(over.0);
        let (ua, ub) = roads.segment_points(under.0);
        let (olen, ulen) = (oa.distance(ob).max(1e-3), ua.distance(ub).max(1e-3));
        let (over_dir, under_dir) = ((ob - oa) / olen, (ub - ua) / ulen);
        let along = (z(os.b) - z(os.a)) / olen;
        // At a vertex of the upper way the grade changes over the crossing.
        let (over_grade_back, over_grade) = if over.1 < 0.02 {
            (beyond(os.a, over.0, os.kind).map_or(along, |g| -g), along)
        } else if over.1 > 0.98 {
            (along, beyond(os.b, over.0, os.kind).unwrap_or(along))
        } else {
            (along, along)
        };
        let over_core = corridor_half_width(os);
        let under_core = corridor_half_width(us);
        let mut u = Underpass {
            at: c.at,
            over_dir,
            under_dir,
            over_z,
            under_z,
            over_grade,
            under_grade: (z(us.b) - z(us.a)) / ulen,
            over_grade_back,
            over_half_width: os.half_width() + if os.is_street() { crate::zoning::SIDEWALK * 0.5 } else { 0.5 },
            over_corridor_half_width: over_core,
            under_half_width: under_core,
            cut_half_width: under_core + UNDERPASS_MARGIN,
            cut_half_length: 0.0,
        };
        // The cut runs through the upper way's corridor and the slopes
        // either side of it, measured along the lower way.
        let rise = (over_z - land.height(c.at).min(under_z)).abs();
        u.cut_half_length = (over_core + embankment_falloff(rise)) / u.sin_angle() + UNDERPASS_MARGIN;
        out.push(u);
    }
    out
}

/// The ground as built: natural terrain, cut and filled to every road's
/// profile and levelled to a pad under every lot. The renderer draws the
/// terrain from it and sets everything that stands on the ground on it.
/// Derived (rebuilt with the roads), never saved.
#[derive(Clone, Default)]
pub struct Ground {
    pub(super) half: f32,
    pub(super) n: usize,
    pub(super) h: Vec<f32>,
    /// What holds each cell (a road, a cut, a pad), or free ground.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fix: Vec<Fix>,
    /// Lot pads, by lot.
    pub pads: std::collections::HashMap<LotId, f32>,
    /// Bridge decks: ends, half width (with the pavement), end heights.
    /// Underpass decks are among them, so what drives over one stays up.
    pub decks: Vec<(Vec2, Vec2, f32, f32, f32)>,
    /// Ways passing over others without a junction (the highway over a
    /// street, a street over the highway or another street).
    pub underpasses: Vec<Underpass>,
    /// How many of `decks` are water bridges; the rest are underpass decks
    /// carrying the highway over a street.
    pub bridge_decks: usize,
    /// Ways reaching past the raster or near its edge (the highway, the
    /// trunk line, roads out to elsewhere): ends, end heights and corridor
    /// half width. The ground beyond the raster is graded to them as inside
    /// it.
    pub(super) outer: Vec<(Vec2, Vec2, f32, f32, f32)>,
    /// `outer` by square of `OUTER_BUCKET` metres (over `FAR_GROUND`): the
    /// ways that may grade the ground in each.
    pub(super) outer_index: Vec<Vec<u32>>,
    /// [review] Lot edges held by a retaining wall: a corner lot levelled
    /// to its front street standing over a lower side street.
    pub retaining: Vec<Retaining>,
    /// The lots behind those walls: their outline, pad and bounds. Inside
    /// one the ground is its pad exactly, up to the wall's top (the raster
    /// alone ramped down to the side street across a cell).
    pub(super) terraces: Vec<Terrace>,
    /// `terraces` by square of `TERRACE_BUCKET` metres.
    pub(super) terrace_index: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

/// [review] A lot pad standing at least this far over a street it does not
/// front gets a retaining wall along that edge.
pub const RETAIN_MIN: f32 = 0.4;

pub(super) const TERRACE_BUCKET: f32 = 32.0;

/// [review] One edge of a lot held by a retaining wall, from `a` to `b`
/// along the lot's outline: the wall's foot is the side street's height at
/// each end (`foot_a`, `foot_b`), its top the lot's pad.
#[derive(Clone, Debug, PartialEq)]
pub struct Retaining {
    pub lot: LotId,
    pub a: Vec2,
    pub b: Vec2,
    pub foot_a: f32,
    pub foot_b: f32,
    pub top: f32,
    /// Unit vector from the edge into the lot.
    pub inward: Vec2,
}

#[derive(Clone, Debug)]
pub(super) struct Terrace {
    pub(super) polygon: Vec<Vec2>,
    pub(super) pad: f32,
    pub(super) lo: Vec2,
    pub(super) hi: Vec2,
    /// Indices into `Ground::retaining` of this lot's walls.
    pub(super) walls: Vec<u32>,
}

/// What holds a cell of the built-ground raster.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Fix {
    /// Graded between what holds the cells around it.
    #[default]
    Free,
    /// A road's corridor, at the road's height.
    Road,
    /// An underpass cut, at the lower way's height. Its sides are walls, so
    /// it does not grade the ground beside it.
    Cut,
    /// A sunken road between retaining walls: likewise.
    Wall,
    /// A lot, at its pad.
    Pad,
}

/// Ways within this many metres of the raster's edge also grade the
/// ground beyond it.
pub(super) const OUTER_NEAR: f32 = 250.0;

/// How far from a way's corridor its side slopes are followed beyond the
/// raster (at 1 in `EMBANKMENT_SLOPE` that makes up 120 m).
pub(super) const OUTER_REACH: f32 = 300.0;

/// Width of the band along the raster's edge that eases into the ground
/// beyond it.
pub(super) const OUTER_BLEND: f32 = 48.0;

/// The toe and the top of every side slope are rounded over this many
/// metres of height.
pub(super) const TOE_ROUNDING: f32 = 1.5;

/// Side of the squares `Ground::outer_index` sorts the outer ways into.
pub(super) const OUTER_BUCKET: f32 = 128.0;

pub(super) const OUTER_BUCKETS: usize = (2.0 * FAR_GROUND / OUTER_BUCKET) as usize + 1;

pub(super) fn outer_bucket(p: Vec2) -> Option<usize> {
    let i = ((p.x + FAR_GROUND) / OUTER_BUCKET).floor();
    let j = ((p.y + FAR_GROUND) / OUTER_BUCKET).floor();
    let range = 0.0..OUTER_BUCKETS as f32;
    (range.contains(&i) && range.contains(&j)).then(|| j as usize * OUTER_BUCKETS + i as usize)
}

/// Every outer way, filed under each square its slopes may reach.
pub(super) fn index_outer(outer: &[(Vec2, Vec2, f32, f32, f32)]) -> Vec<Vec<u32>> {
    let mut index = vec![Vec::new(); OUTER_BUCKETS * OUTER_BUCKETS];
    let cell = |v: f32| (((v + FAR_GROUND) / OUTER_BUCKET).floor().max(0.0) as usize).min(OUTER_BUCKETS - 1);
    for (id, &(a, b, _, _, core)) in outer.iter().enumerate() {
        let reach = core + 40.0 + OUTER_REACH;
        let (lo, hi) = (a.min(b) - Vec2::splat(reach), a.max(b) + Vec2::splat(reach));
        for j in cell(lo.y)..=cell(hi.y) {
            for i in cell(lo.x)..=cell(hi.x) {
                // Only squares the corridor and its slopes come near.
                let c = Vec2::new(-FAR_GROUND + (i as f32 + 0.5) * OUTER_BUCKET, -FAR_GROUND + (j as f32 + 0.5) * OUTER_BUCKET);
                if project_on_segment(c, a, b).1.distance(c) <= reach + OUTER_BUCKET * std::f32::consts::FRAC_1_SQRT_2 {
                    index[j * OUTER_BUCKETS + i].push(id as u32);
                }
            }
        }
    }
    index
}

/// Polynomial smooth maximum: `max(a, b)` with the corner rounded over `k`.
pub(super) fn smooth_max(a: f32, b: f32, k: f32) -> f32 {
    let h = ((k - (a - b).abs()) / k).max(0.0);
    a.max(b) + h * h * k * 0.25
}

pub(super) fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    -smooth_max(-a, -b, k)
}

/// The free ground at a point: the natural height, filled up to `lo` (the
/// embankments around it) and cut down to `hi` (the cuttings), the toe of
/// each slope rounded into the terrain. Where two works ask for more than
/// their side slopes allow (`lo > hi`), the ground runs evenly between them.
pub(super) fn settle(natural: f32, lo: f32, hi: f32) -> f32 {
    if lo > hi {
        return 0.5 * (lo + hi);
    }
    let up = if lo.is_finite() { smooth_max(natural, lo, TOE_ROUNDING) } else { natural };
    let z = if hi.is_finite() { smooth_min(up, hi, TOE_ROUNDING) } else { up };
    z.clamp(lo, hi)
}

/// Grade the free cells of an `n`×`n` raster to the cells that are held
/// (`fix`, at `set`). Each held road or pad cell is the apex of two cones
/// of side slope 1 in `EMBANKMENT_SLOPE`: one below it (`lo`, the least the
/// ground around may be: an embankment) and one above it (`hi`, the most:
/// a cutting). `lo` and `hi` come in seeded with the exact cones beside
/// the road cores. The cones spread over free ground only, so a road or a
/// pad between two works shields one from the other. The natural ground is
/// clamped between them (`settle`) and the result smoothed once: side
/// slopes are even and never steeper than the limit, they blend into the
/// terrain, and where two works meet the ground runs from one to the other
/// instead of whichever is nearer winning cell by cell.
pub(super) fn grade_free_ground(n: usize, natural: &[f32], fix: &[Fix], set: &[f32], mut lo: Vec<f32>, mut hi: Vec<f32>) -> Vec<f32> {
    let step = GROUND_CELL / EMBANKMENT_SLOPE;
    let diag = step * std::f32::consts::SQRT_2;
    for k in 0..n * n {
        match fix[k] {
            Fix::Road | Fix::Pad => {
                lo[k] = set[k];
                hi[k] = set[k];
            }
            // A cut neither grades the ground beside it nor is graded.
            Fix::Cut | Fix::Wall => {
                lo[k] = f32::NEG_INFINITY;
                hi[k] = f32::INFINITY;
            }
            Fix::Free => {}
        }
    }
    // Chamfer sweeps, forward and back, repeated until nothing changes: the
    // cones reach every free cell along the shortest free path.
    let behind: [(isize, isize, f32); 4] = [(-1, -1, diag), (0, -1, step), (1, -1, diag), (-1, 0, step)];
    for _ in 0..24 {
        let mut changed = false;
        for pass in 0..2 {
            for idx in 0..n * n {
                let k = if pass == 0 { idx } else { n * n - 1 - idx };
                if fix[k] != Fix::Free {
                    continue;
                }
                let (i, j) = ((k % n) as isize, (k / n) as isize);
                let (mut l, mut h) = (lo[k], hi[k]);
                for &(di, dj, s) in &behind {
                    let (di, dj) = if pass == 0 { (di, dj) } else { (-di, -dj) };
                    let (ii, jj) = (i + di, j + dj);
                    if ii < 0 || jj < 0 || ii >= n as isize || jj >= n as isize {
                        continue;
                    }
                    let m = jj as usize * n + ii as usize;
                    l = l.max(lo[m] - s);
                    h = h.min(hi[m] + s);
                }
                if l > lo[k] + 1e-4 || h < hi[k] - 1e-4 {
                    lo[k] = l;
                    hi[k] = h;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let raw: Vec<f32> = (0..n * n).map(|k| if fix[k] == Fix::Free { settle(natural[k], lo[k], hi[k]) } else { set[k] }).collect();
    // One pass of smoothing over the free ground takes the crease off the
    // top and the foot of every slope. A cut's walls stay sharp.
    let mut h = raw.clone();
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            if fix[k] != Fix::Free {
                continue;
            }
            let (mut sum, mut wsum) = (0.0, 0.0);
            for dj in -1isize..=1 {
                for di in -1isize..=1 {
                    let (ii, jj) = (i as isize + di, j as isize + dj);
                    if ii < 0 || jj < 0 || ii >= n as isize || jj >= n as isize {
                        continue;
                    }
                    let m = jj as usize * n + ii as usize;
                    if matches!(fix[m], Fix::Cut | Fix::Wall) {
                        continue;
                    }
                    let w = if di == 0 && dj == 0 { 4.0 } else if di == 0 || dj == 0 { 2.0 } else { 1.0 };
                    sum += raw[m] * w;
                    wsum += w;
                }
            }
            h[k] = sum / wsum;
        }
    }
    h
}

impl std::fmt::Debug for Ground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ground({}x{})", self.n, self.n)
    }
}

impl Ground {
    /// Build over the square of half-side `half`.
    pub fn build(land: &Landscape, roads: &RoadNetwork, node_z: &[f32], inner: &[Vec<(f32, f32)>], lots: &SlotMap<LotId, Lot>, half: f32) -> Ground {
        let mut pads = std::collections::HashMap::new();
        for (id, lot) in lots.iter() {
            pads.insert(id, lot_pad(roads, node_z, inner, lot, land));
        }
        let mut decks: Vec<(Vec2, Vec2, f32, f32, f32)> = roads
            .alive_segments()
            .filter(|(_, s)| s.bridge)
            .map(|(id, s)| {
                let (a, b) = roads.segment_points(id);
                let hw = s.half_width() + if s.is_street() { crate::zoning::SIDEWALK } else { 0.5 };
                (a, b, hw, node_z.get(s.a.0 as usize).copied().unwrap_or(0.0), node_z.get(s.b.0 as usize).copied().unwrap_or(0.0))
            })
            .collect();
        let outer: Vec<(Vec2, Vec2, f32, f32, f32)> = roads
            .alive_segments()
            .filter(|(_, s)| !s.bridge && !s.tunnel)
            .filter_map(|(id, s)| {
                let (a, b) = roads.segment_points(id);
                (chebyshev(a).max(chebyshev(b)) > half - OUTER_NEAR).then(|| {
                    (a, b, node_z.get(s.a.0 as usize).copied().unwrap_or(0.0), node_z.get(s.b.0 as usize).copied().unwrap_or(0.0), corridor_half_width(s))
                })
            })
            .collect();
        let underpasses = find_underpasses(land, roads, node_z);
        let bridge_decks = decks.len();
        for u in &underpasses {
            // The upper way's deck spans the cut and a cell beyond it, in
            // two halves meeting over the crossing (it may be a crest).
            let reach = (u.cut_half_width + GROUND_CELL) / u.sin_angle();
            let (a, b) = (u.at - u.over_dir * reach, u.at + u.over_dir * reach);
            // As wide as its corridor, so its lamps and rails stay up too.
            let hw = u.over_corridor_half_width + 0.5;
            decks.push((a, u.at, hw, u.over_z_at(-reach), u.over_z));
            decks.push((u.at, b, hw, u.over_z, u.over_z_at(reach)));
        }
        if land.is_flat() {
            // No raster: the flat map's ground is level, and a way below
            // it runs out of sight.
            return Ground { half, n: 0, h: Vec::new(), fix: Vec::new(), pads, decks, underpasses, bridge_decks, outer: Vec::new(), outer_index: Vec::new(), retaining: Vec::new(), terraces: Vec::new(), terrace_index: Default::default() };
        }
        let n = (half * 2.0 / GROUND_CELL) as usize + 1;
        let at = |i: usize, j: usize| Vec2::new(-half + i as f32 * GROUND_CELL, -half + j as f32 * GROUND_CELL);
        let natural: Vec<f32> = (0..n * n).map(|k| land.height(at(k % n, k / n))).collect();
        // What holds each cell: the road cores, the underpass cuts and the
        // lot pads are set to their own heights; the free ground between is
        // graded to them (`grade_free_ground`).
        let mut fix = vec![Fix::Free; n * n];
        let mut set = vec![0.0f32; n * n];
        // Free cells just beside a road core take their bounds from the
        // exact distance to it, so a core drawn on the grid leaves no
        // staircase along its edge.
        let mut lo = vec![f32::NEG_INFINITY; n * n];
        let mut hi = vec![f32::INFINITY; n * n];
        // Never carve the built ground below the waterline beside the
        // water; a cutting away from it may go deeper.
        let floor_at = |p: Vec2, z: f32| if z < WATER_LEVEL + 0.4 && land.water_distance(p) < 40.0 { WATER_LEVEL + 0.4 } else { z };
        let rise = 1.0 / EMBANKMENT_SLOPE;
        let range = |lo: f32, hi: f32| -> (usize, usize) {
            let a = (((lo + half) / GROUND_CELL).floor().max(0.0) as usize).min(n - 1);
            let b = (((hi + half) / GROUND_CELL).ceil().max(0.0) as usize).min(n - 1);
            (a, b)
        };
        let seed_band = GROUND_CELL * 1.5;
        for (sid, seg) in roads.alive_segments() {
            // Water bridges leave the ground alone; a viaduct only grades
            // it where its deck comes down near it (the foot of a ramp).
            if (seg.bridge && !seg.viaduct) || seg.tunnel {
                continue;
            }
            let foot = seg.viaduct;
            // A road the player sank (a cutting, a portal ramp) is held by
            // retaining walls: the ground comes down to it within metres.
            let walled = roads.node(seg.a).level < 0.0 || roads.node(seg.b).level < 0.0;
            let (a, b) = (roads.node(seg.a).pos, roads.node(seg.b).pos);
            let (za, zb) = (node_z.get(seg.a.0 as usize).copied().unwrap_or(0.0), node_z.get(seg.b.0 as usize).copied().unwrap_or(0.0));
            let core = corridor_half_width(seg) + if walled { GROUND_CELL } else { 0.0 };
            // A road the player sank is held by retaining walls: it holds its
            // own cells and grades nothing beside it.
            let reach = if walled { core } else { core + seed_band };
            // Where this way passes over another, the ground under it is the
            // other's.
            let dir = (b - a).normalize_or_zero();
            // A tunnel portal at either end: the cutting's ground stops a
            // little way into the hill there (`PORTAL_HOLD`).
            let portal = |n: crate::roads::NodeId| walled && roads.node(n).segments.iter().any(|&o| roads.segment(o).alive && roads.segment(o).tunnel);
            let (portal_a, portal_b) = (portal(seg.a), portal(seg.b));
            let cuts: Vec<&Underpass> = underpasses.iter().filter(|u| u.over_dir.dot(dir).abs() > 0.999 && project_on_segment(u.at, a, b).1.distance(u.at) < 0.5).collect();
            let (i0, i1) = range(a.x.min(b.x) - reach, a.x.max(b.x) + reach);
            let (j0, j1) = range(a.y.min(b.y) - reach, a.y.max(b.y) + reach);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = at(i, j);
                    let (t, q) = project_on_segment(p, a, b);
                    let d = q.distance(p);
                    if d > reach || cuts.iter().any(|u| u.in_cut(p)) {
                        continue;
                    }
                    // Past a portal only the road's own corridor is held,
                    // and only `PORTAL_HOLD` in, under the portal's lid.
                    let past = if portal_b { (p - b).dot(dir) } else { f32::MIN }.max(if portal_a { (a - p).dot(dir) } else { f32::MIN });
                    if past > 0.0 && (past > PORTAL_HOLD || d > corridor_half_width(seg) + 0.5) {
                        continue;
                    }
                    let k = j * n + i;
                    // Through the way's inner samples on a long street.
                    let z = if inner.get(sid.0 as usize).is_some_and(|v| !v.is_empty()) { way_height(roads, node_z, inner, sid, t) } else { za + (zb - za) * t };
                    // A viaduct grades the ground only where its deck comes
                    // down near it (the foot of a ramp).
                    if foot && z - natural[k] > 2.0 {
                        continue;
                    }
                    let z = floor_at(p, z);
                    if d <= core {
                        // Where cores overlap at about one height (a junction,
                        // a ramp merging) the lower way holds, so the ground
                        // stays under both surfaces: the higher one held, and
                        // a ramp a few centimetres above the carriageway it
                        // joins raised the ground through the carriageway's
                        // asphalt (the bare wedge in the gore). Ways well
                        // apart (an embankment beside a road) keep the higher.
                        if fix[k] == Fix::Free || fix[k] == Fix::Wall {
                            set[k] = z;
                        } else if (z - set[k]).abs() < 1.5 {
                            set[k] = set[k].min(z);
                        } else if z > set[k] {
                            set[k] = z;
                        }
                        fix[k] = if walled && fix[k] != Fix::Road { Fix::Wall } else { Fix::Road };
                    } else {
                        let e = (d - core) * rise;
                        lo[k] = lo[k].max(z - e);
                        hi[k] = hi[k].min(z + e);
                    }
                }
            }
        }
        // The cuts: level with the lower way across their whole width. Their
        // sides are the abutments' walls, so they hold their own cells but
        // do not grade the ground beside them (that stays the upper way's
        // embankment).
        for u in &underpasses {
            let r = u.cut_half_length.max(u.cut_half_width) * 1.5;
            let (i0, i1) = range(u.at.x - r, u.at.x + r);
            let (j0, j1) = range(u.at.y - r, u.at.y + r);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = at(i, j);
                    if u.in_cut(p) {
                        let k = j * n + i;
                        fix[k] = Fix::Cut;
                        set[k] = floor_at(p, u.under_z_at(u.local(p).0));
                    }
                }
            }
        }
        for (id, lot) in lots.iter() {
            let pad = pads[&id];
            let (lo_b, hi_b) = crate::geom::bounds(&lot.polygon);
            let (i0, i1) = range(lo_b.x - 2.0, hi_b.x + 2.0);
            let (j0, j1) = range(lo_b.y - 2.0, hi_b.y + 2.0);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = at(i, j);
                    let inside = point_in_polygon(p, &lot.polygon) || distance_to_outline(p, &lot.polygon) < 2.0;
                    let k = j * n + i;
                    // A road's corridor keeps its own height: the margin
                    // round a lot reaches over the pavement, and a corner
                    // lot levelled to its higher street used to lift the
                    // side street's kerb and lanes to its pad.
                    if inside && !matches!(fix[k], Fix::Road | Fix::Wall) {
                        fix[k] = Fix::Pad;
                        set[k] = floor_at(p, pad);
                    }
                }
            }
        }
        let h = grade_free_ground(n, &natural, &fix, &set, lo, hi);
        let outer_index = index_outer(&outer);
        let mut g = Ground { half, n, h, fix, pads, decks, underpasses, bridge_decks, outer, outer_index, retaining: Vec::new(), terraces: Vec::new(), terrace_index: Default::default() };
        g.blend_into_outer(land);
        g.find_retaining(roads, node_z, inner, lots);
        g
    }

    /// [review] Every lot edge along a street the lot does not front where
    /// the pad stands `RETAIN_MIN` or more over that street: a corner lot
    /// levelled to its front street used to ramp down across a ground cell
    /// to the side street's pavement (the raster cannot step). The edge
    /// gets a retaining wall (drawn by the app), and inside the lot the
    /// ground is held at the pad right up to it.
    pub(super) fn find_retaining(&mut self, roads: &RoadNetwork, node_z: &[f32], inner: &[Vec<(f32, f32)>], lots: &SlotMap<LotId, Lot>) {
        const SAMPLES: [f32; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];
        for (id, lot) in lots.iter() {
            let Some(&pad) = self.pads.get(&id) else { continue };
            let m = lot.polygon.len();
            if m < 3 {
                continue;
            }
            let mut walls = Vec::new();
            for k in 0..m {
                let (a, b) = (lot.polygon[k], lot.polygon[(k + 1) % m]);
                if a.distance(b) < 1.0 {
                    continue;
                }
                // The nearest street beside a point of the edge, if one is
                // (a side street may be cut into several segments along it).
                let street_at = |x: Vec2| {
                    let reach = Vec2::splat(20.0);
                    roads
                        .segments_in_box(x - reach, x + reach)
                        .filter(|&sid| {
                            let sg = roads.segment(sid);
                            sid != lot.front_segment && sg.alive && sg.is_street() && !sg.bridge && !sg.tunnel && !sg.viaduct && roads.node(sg.a).level >= 0.0 && roads.node(sg.b).level >= 0.0
                        })
                        .filter_map(|sid| {
                            let (p, q) = roads.segment_points(sid);
                            let (t, on) = project_on_segment(x, p, q);
                            let d = on.distance(x);
                            (d <= corridor_half_width(roads.segment(sid)) + 1.0).then_some((d, sid, t, on))
                        })
                        .min_by(|u, v| u.0.total_cmp(&v.0))
                };
                let found: Vec<(Vec2, f32, Vec2)> = SAMPLES.iter().filter_map(|&f| street_at(a.lerp(b, f)).map(|(_, sid, t, on)| (a.lerp(b, f), way_height(roads, node_z, inner, sid, t), on))).collect();
                if found.len() < 4 {
                    continue;
                }
                // The street's height at either end of the edge, from the
                // samples nearest them.
                let (first, last) = (found[0], found[found.len() - 1]);
                let (fa, fb) = (first.1, last.1);
                if pad - fa.max(fb) < RETAIN_MIN {
                    continue;
                }
                // Into the lot: away from the street.
                let d = (b - a).normalize_or_zero();
                let mut inward = Vec2::new(-d.y, d.x);
                if (first.2 - first.0).dot(inward) > 0.0 {
                    inward = -inward;
                }
                walls.push(self.retaining.len() as u32);
                self.retaining.push(Retaining { lot: id, a, b, foot_a: fa, foot_b: fb, top: pad, inward });
            }
            if walls.is_empty() {
                continue;
            }
            let (lo, hi) = crate::geom::bounds(&lot.polygon);
            let t = self.terraces.len() as u32;
            let bucket = |v: f32| (v / TERRACE_BUCKET).floor() as i32;
            for bx in bucket(lo.x - 0.1)..=bucket(hi.x + 0.1) {
                for by in bucket(lo.y - 0.1)..=bucket(hi.y + 0.1) {
                    self.terrace_index.entry((bx, by)).or_default().push(t);
                }
            }
            self.terraces.push(Terrace { polygon: lot.polygon.clone(), pad, lo, hi, walls });
        }
    }

    /// The pad of a lot behind a retaining wall that `p` lies in (or on one
    /// of its walls), if any.
    pub(super) fn terrace_at(&self, p: Vec2) -> Option<f32> {
        if self.terraces.is_empty() {
            return None;
        }
        let key = ((p.x / TERRACE_BUCKET).floor() as i32, (p.y / TERRACE_BUCKET).floor() as i32);
        for &t in self.terrace_index.get(&key)? {
            let tr = &self.terraces[t as usize];
            if p.x < tr.lo.x - 0.05 || p.y < tr.lo.y - 0.05 || p.x > tr.hi.x + 0.05 || p.y > tr.hi.y + 0.05 {
                continue;
            }
            let on_wall = || {
                tr.walls.iter().any(|&w| {
                    let r = &self.retaining[w as usize];
                    project_on_segment(p, r.a, r.b).1.distance(p) < 0.05
                })
            };
            if point_in_polygon(p, &tr.polygon) || on_wall() {
                return Some(tr.pad);
            }
        }
        None
    }

    /// The last stretch of the raster eases into the ground beyond it
    /// (`outer_height`), so there is no seam where the two meet.
    pub(super) fn blend_into_outer(&mut self, land: &Landscape) {
        let n = self.n;
        for j in 0..n {
            for i in 0..n {
                let p = Vec2::new(-self.half + i as f32 * GROUND_CELL, -self.half + j as f32 * GROUND_CELL);
                let r = chebyshev(p);
                if r < self.half - OUTER_BLEND {
                    continue;
                }
                let k = j * n + i;
                let w = smoothstep(self.half - OUTER_BLEND, self.half, r);
                self.h[k] = self.h[k] * (1.0 - w) + self.outer_height(land, p) * w;
            }
        }
    }

    /// The ground beyond the raster: natural terrain, graded to the ways
    /// that run out there with the same side slopes as inside it. Past the
    /// map edge each corridor gets a wide level shoulder, so the coarse far
    /// ground never rises over the road.
    pub(super) fn outer_height(&self, land: &Landscape, p: Vec2) -> f32 {
        let natural = land.height(p);
        let far = smoothstep(LAND_HALF - 100.0, LAND_HALF + 200.0, chebyshev(p));
        let floor = WATER_LEVEL + 0.4;
        let rise = 1.0 / EMBANKMENT_SLOPE;
        let mut on = f32::NEG_INFINITY;
        // Ways in reach: ends, core half width, foot of the perpendicular, height there.
        let mut near: Vec<(Vec2, Vec2, f32, Vec2, f32)> = Vec::new();
        let all: Vec<u32>;
        let ids = match outer_bucket(p) {
            Some(c) if !self.outer_index.is_empty() => &self.outer_index[c],
            _ => {
                all = (0..self.outer.len() as u32).collect();
                &all
            }
        };
        for &id in ids {
            let (a, b, za, zb, core) = self.outer[id as usize];
            let core = core + 40.0 * far;
            let reach = core + OUTER_REACH;
            if p.x < a.x.min(b.x) - reach || p.x > a.x.max(b.x) + reach || p.y < a.y.min(b.y) - reach || p.y > a.y.max(b.y) + reach {
                continue;
            }
            let (t, q) = project_on_segment(p, a, b);
            let d = q.distance(p);
            if d > reach {
                continue;
            }
            let z = (za + (zb - za) * t).max(floor);
            if d <= core {
                on = on.max(z);
            }
            near.push((a, b, core, q, z));
        }
        if on > f32::NEG_INFINITY {
            return on;
        }
        // As inside the raster, a way's side slope stops at another way's
        // corridor: the far carriageway does not grade the ground beyond
        // the near one.
        let shielded = |k: usize| {
            let q = near[k].3;
            near.iter().enumerate().any(|(m, &(a, b, other_core, _, _))| m != k && project_on_segment(q, a, b).1.distance(q) > other_core && crate::geom::segment_distance_between(p, q, a, b) < other_core)
        };
        // The highest embankment and the deepest cutting not shielded.
        let mut cones: Vec<(f32, usize)> = near.iter().enumerate().map(|(k, &(_, _, core, q, _))| ((p.distance(q) - core) * rise, k)).collect();
        cones.sort_by(|x, y| (near[y.1].4 - y.0).total_cmp(&(near[x.1].4 - x.0)));
        let lo = cones.iter().find(|c| !shielded(c.1)).map_or(f32::NEG_INFINITY, |c| near[c.1].4 - c.0);
        cones.sort_by(|x, y| (near[x.1].4 + x.0).total_cmp(&(near[y.1].4 + y.0)));
        let hi = cones.iter().find(|c| !shielded(c.1)).map_or(f32::INFINITY, |c| near[c.1].4 + c.0);
        settle(natural, lo, hi)
    }

    /// Height of a bridge deck at `p`, if `p` is on one.
    pub fn deck_at(&self, p: Vec2) -> Option<f32> {
        self.deck_in(&self.decks, p)
    }

    /// Heights of every deck over `p` (a viaduct over a street over a
    /// cutting has two).
    pub fn decks_at(&self, p: Vec2) -> Vec<f32> {
        let mut out = Vec::new();
        for &(a, b, hw, za, zb) in &self.decks {
            if p.x < a.x.min(b.x) - hw || p.x > a.x.max(b.x) + hw || p.y < a.y.min(b.y) - hw || p.y > a.y.max(b.y) + hw {
                continue;
            }
            let (t, q) = project_on_segment(p, a, b);
            if q.distance(p) <= hw {
                out.push(za + (zb - za) * t);
            }
        }
        out
    }

    /// Height of a water bridge at `p`, ignoring the underpass decks that
    /// carry the highway over streets: what drives along the street below
    /// stays down in its cutting.
    pub fn bridge_at(&self, p: Vec2) -> Option<f32> {
        self.deck_in(&self.decks[..self.bridge_decks.min(self.decks.len())], p)
    }

    pub(super) fn deck_in(&self, decks: &[(Vec2, Vec2, f32, f32, f32)], p: Vec2) -> Option<f32> {
        // A deck whose span `p` is alongside wins over one it is only past
        // the end of (two halves of a deck meet over a crossing).
        let mut past_end = None;
        for &(a, b, hw, za, zb) in decks {
            if p.x < a.x.min(b.x) - hw || p.x > a.x.max(b.x) + hw || p.y < a.y.min(b.y) - hw || p.y > a.y.max(b.y) + hw {
                continue;
            }
            let (t, q) = project_on_segment(p, a, b);
            if q.distance(p) <= hw {
                let z = za + (zb - za) * t;
                if t > 0.0 && t < 1.0 {
                    return Some(z);
                }
                past_end.get_or_insert(z);
            }
        }
        past_end
    }

    /// Height of the built ground (natural terrain outside the raster).
    pub fn height(&self, land: &Landscape, p: Vec2) -> f32 {
        if self.n < 2 || p.x.abs() >= self.half || p.y.abs() >= self.half {
            return if land.is_flat() { land.base_height(p) } else { self.outer_height(land, p) };
        }
        if let Some(pad) = self.terrace_at(p) {
            return pad;
        }
        let n = self.n;
        let fx = ((p.x + self.half) / GROUND_CELL).clamp(0.0, (n - 1) as f32 - 1e-3);
        let fy = ((p.y + self.half) / GROUND_CELL).clamp(0.0, (n - 1) as f32 - 1e-3);
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let a = self.h[j * n + i];
        let b = self.h[j * n + i + 1];
        let c = self.h[(j + 1) * n + i];
        let d = self.h[(j + 1) * n + i + 1];
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        top + (bot - top) * ty
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

/// How far past a tunnel portal the open cutting's ground is held at the
/// road's level: two cells, so the height field's slope up into the hill
/// starts behind the portal's dark mouth (`nd_procgen::terrain`), which
/// closes the stretch with a lid. Before, the cutting's core held a disc
/// some 14 m into the hill and the hillside showed through the arch.
pub const PORTAL_HOLD: f32 = GROUND_CELL * 2.0;

/// The level a lot is graded to: its street's height at the frontage.
pub fn lot_pad(roads: &RoadNetwork, node_z: &[f32], inner: &[Vec<(f32, f32)>], lot: &Lot, land: &Landscape) -> f32 {
    if land.is_flat() {
        return 0.0;
    }
    match roads.try_segment(lot.front_segment) {
        Some(seg) if !seg.bridge => way_height(roads, node_z, inner, lot.front_segment, lot.front_t),
        _ => land.height(lot.center).max(BANK_HEIGHT),
    }
}

/// Height of a way at `t` along segment `sid`: between its nodes' heights
/// (`node_z`), through the segment's inner samples where it has any
/// (`inner[sid]`, `(t, z)` in rising `t`; see `World::road_inner`).
pub fn way_height(roads: &RoadNetwork, node_z: &[f32], inner: &[Vec<(f32, f32)>], sid: crate::roads::SegmentId, t: f32) -> f32 {
    let Some(seg) = roads.try_segment(sid) else { return 0.0 };
    let za = node_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
    let zb = node_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
    let t = t.clamp(0.0, 1.0);
    let samples = inner.get(sid.0 as usize).map(|v| v.as_slice()).unwrap_or(&[]);
    if samples.is_empty() {
        return za + (zb - za) * t;
    }
    let (mut t0, mut z0) = (0.0f32, za);
    for &(t1, z1) in samples.iter().chain(std::iter::once(&(1.0, zb))) {
        if t <= t1 {
            let span = t1 - t0;
            return if span > 1e-6 { z0 + (z1 - z0) * (t - t0) / span } else { z1 };
        }
        (t0, z0) = (t1, z1);
    }
    zb
}
