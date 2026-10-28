//! Pillars under raised roads: where the columns of a viaduct stand.
//!
//! A raised way (one the player drew `VIADUCT_LEVEL` or more above the
//! ground) is carried on concrete columns. They are spaced about
//! `PILLAR_SPAN` apart, but each may be shifted along the way to dodge
//! what is underneath, within the structural limit `PILLAR_MAX_SPAN`
//! between supports (and `PILLAR_END_REACH` from a raised end):
//!
//! - never on a road, path or track running underneath (with its
//!   pavements), nor on a facility or one of the city's own structures;
//! - free ground first, then an empty lot (dropped from the zoning), and
//!   only when nothing else is in reach a grown building, which the city
//!   buys out as road works do.
//!
//! Where no site is in reach for longer than the structural limit the way
//! cannot be built: the plan lists the stretch (`PillarPlan::gaps`), the
//! road tool draws it red and implementing is refused with the reason.
//!
//! `plan` is the placement itself, over any `site` test: the world feeds it
//! the roads, lots and buildings (`World::pillar_site`), the tests a grid.

use crate::buildings::BuildingId;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Spacing aimed for between supports, metres.
pub const PILLAR_SPAN: f32 = 24.0;
/// The structural limit: no span between two supports longer than this.
pub const PILLAR_MAX_SPAN: f32 = 42.0;
/// Columns no closer than this (except where the deck lands on the ground).
pub const PILLAR_MIN_SPAN: f32 = 8.0;
/// A deck that ends in the air (at a junction with another raised way, or
/// a dead end) has a column at most this far from its end.
pub const PILLAR_END_REACH: f32 = 10.0;
/// Radius of a column with its footing: what must be clear of roads and
/// buildings.
pub const PILLAR_RADIUS: f32 = 1.6;
/// A way at least this high at both ends of a piece spans over the lots
/// under it instead of taking them: a one-storey building fits under its
/// deck. Lower raised pieces take their corridor like a road at grade.
pub const SPAN_LEVEL: f32 = 6.0;
/// What a deck and a roof take off the headroom under a raised way, and
/// the height of a storey, for how tall a building may stand under it.
pub const DECK_DEPTH: f32 = 1.2;
pub const ROOF_ALLOWANCE: f32 = 1.5;
pub const STOREY: f32 = 3.6;
/// Sample spacing along the way.
const STEP: f32 = 1.0;

/// Floors that fit under a deck `level` metres above the ground.
pub fn floors_under(level: f32) -> u8 {
    ((level - DECK_DEPTH - ROOF_ALLOWANCE) / STOREY).floor().clamp(0.0, 255.0) as u8
}

/// What stands where a column would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// Open ground (or water, or a park's lawn).
    Free,
    /// A zoned lot with nothing on it: the column takes it from the zoning.
    EmptyLot(LotId),
    /// A grown building: bought out and pulled down.
    Buyout(LotId, BuildingId),
    /// A road, path or track underneath (with its pavements).
    Road,
    /// Something the city never removes.
    Blocked,
}

impl Site {
    fn cost(self) -> f32 {
        match self {
            Site::Free => 1.0,
            Site::EmptyLot(_) => 14.0,
            Site::Buyout(..) => 70.0,
            Site::Road | Site::Blocked => f32::INFINITY,
        }
    }
}

/// One column.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pillar {
    pub at: Vec2,
    /// Unit direction of the way over it (the crosshead lies across it).
    pub dir: Vec2,
    /// Half width of the deck it carries.
    pub half_width: f32,
    /// The deck's planned level above the ground there.
    pub level: f32,
    /// Hash of the way it carries (`way_key`): a way drawn again the same
    /// keeps its columns where they stood.
    #[serde(default)]
    pub way: u64,
}

/// Where the columns of one raised way go, and what they cost.
#[derive(Clone, Debug, Default)]
pub struct PillarPlan {
    pub pillars: Vec<Pillar>,
    /// Grown buildings a column stands on: bought out (with their lot).
    pub buyouts: Vec<(LotId, BuildingId)>,
    /// Empty lots a column stands on.
    pub lots: Vec<LotId>,
    /// Stretches with no room for a column: from the last support to the
    /// next possible one, or to an end left in the air.
    pub gaps: Vec<Gap>,
}

/// A stretch of raised way with no room for a column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    /// From the last support...
    pub a: Vec2,
    /// ...to the next possible one, or (for an end) the end itself.
    pub b: Vec2,
    /// The way ends in the air here (a dead end, or where it meets another
    /// raised way) with no column within `PILLAR_END_REACH` of its end.
    pub end: bool,
    /// What stands under that end is a road (else something the city keeps).
    pub over_road: bool,
    /// The deck's planned level over the stretch, metres above the ground.
    pub level: f32,
}

impl Gap {
    /// Why the way cannot be built, in words.
    pub fn reason(&self) -> String {
        if self.end {
            let under = if self.over_road { "over a road" } else { "over something the city keeps" };
            format!(
                "the road ends in the air {under} at ({:.0}, {:.0}), with no room for a column within {:.0} m of its end. End it on the ground, connect it to another raised road, or end it past what is below",
                self.b.x, self.b.y, PILLAR_END_REACH
            )
        } else {
            format!(
                "no room for a pillar between ({:.0}, {:.0}) and ({:.0}, {:.0}): roads or buildings the city keeps stand under the deck for more than the {:.0} m a span may be. Move the road, or raise it over a gap",
                self.a.x, self.a.y, self.b.x, self.b.y, PILLAR_MAX_SPAN
            )
        }
    }
}

impl PillarPlan {
    pub fn ok(&self) -> bool {
        self.gaps.is_empty()
    }

    /// Why the way cannot be built, if it cannot.
    pub fn refusal(&self) -> Option<String> {
        self.gaps.first().map(Gap::reason)
    }

    pub fn append(&mut self, other: PillarPlan) {
        self.pillars.extend(other.pillars);
        for b in other.buyouts {
            if !self.buyouts.contains(&b) {
                self.buyouts.push(b);
            }
        }
        for l in other.lots {
            if !self.lots.contains(&l) {
                self.lots.push(l);
            }
        }
        self.gaps.extend(other.gaps);
    }
}

/// [r4-decks] Whether a column at `p` stands on the stretch `g` (between
/// its ends, within a deck's width of the line joining them).
pub fn on_stretch(p: Vec2, g: &Gap) -> bool {
    let (t, q) = crate::geom::project_on_segment(p, g.a, g.b);
    t > 0.0 && t < 1.0 && q.distance(p) < 6.0
}

/// [r4-decks] Whether the columns `kept` on the stretch `g` hold it up:
/// no span from its last support through them to the next longer than
/// `PILLAR_MAX_SPAN`.
pub fn stretch_held(g: &Gap, kept: &[Pillar]) -> bool {
    if kept.is_empty() {
        return false;
    }
    let along = |p: Vec2| crate::geom::project_on_segment(p, g.a, g.b).0;
    let len = g.a.distance(g.b);
    let mut ts: Vec<f32> = kept.iter().map(|p| along(p.at)).collect();
    ts.push(0.0);
    ts.push(1.0);
    ts.sort_by(f32::total_cmp);
    ts.windows(2).all(|w| (w[1] - w[0]) * len <= PILLAR_MAX_SPAN + 0.01)
}

/// A key for a raised way: its points and levels, to the decimetre. Saved
/// (`Pillar::way`), so the hash is spelled out here (FNV-1a over the
/// quantised numbers, then a splitmix finish) and never changes with the
/// toolchain, as `std`'s `DefaultHasher` may.
pub fn way_key(pts: &[Vec2], levels: &[f32]) -> u64 {
    let q = |v: f32| (v * 10.0).round() as i64;
    let words = std::iter::once(pts.len() as i64).chain(pts.iter().flat_map(|p| [q(p.x), q(p.y)])).chain(levels.iter().map(|l| q(*l)));
    crate::hash::finish(crate::hash::fnv_bytes(words.flat_map(i64::to_le_bytes)))
}

/// The key saves made before `way_key` was spelled out hold (`std`'s
/// `DefaultHasher`), only to carry their columns over on load
/// (`migrate_way_keys`). If the toolchain changed that hash since, the
/// old columns are not recognised and are planned again.
pub fn legacy_way_key(pts: &[Vec2], levels: &[f32]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in pts {
        ((p.x * 10.0).round() as i64, (p.y * 10.0).round() as i64).hash(&mut h);
    }
    for l in levels {
        ((l * 10.0).round() as i64).hash(&mut h);
    }
    h.finish()
}

/// After a load: columns saved under a way's `legacy_way_key` take its
/// `way_key`. `ways` are the raised ways as drawn (points, levels). A
/// column already under a current key is left alone, so this can run on
/// every load.
pub fn migrate_way_keys<'a>(pillars: &mut [Pillar], ways: impl IntoIterator<Item = (&'a [Vec2], &'a [f32])>) {
    if pillars.is_empty() {
        return;
    }
    let mut current = std::collections::BTreeSet::new();
    let mut from_legacy = std::collections::BTreeMap::new();
    for (pts, levels) in ways {
        let key = way_key(pts, levels);
        current.insert(key);
        from_legacy.insert(legacy_way_key(pts, levels), key);
    }
    for p in pillars.iter_mut() {
        if current.contains(&p.way) {
            continue;
        }
        if let Some(&key) = from_legacy.get(&p.way) {
            p.way = key;
        }
    }
}

/// Does this way need columns anywhere?
pub fn is_raised(levels: &[f32]) -> bool {
    levels.iter().any(|&l| l >= crate::roads::VIADUCT_LEVEL)
}

struct Sample {
    s: f32,
    p: Vec2,
    dir: Vec2,
    level: f32,
}

fn samples(pts: &[Vec2], levels: &[f32]) -> Vec<Sample> {
    let mut out: Vec<Sample> = Vec::new();
    let mut acc = 0.0;
    for (i, w) in pts.windows(2).enumerate() {
        let len = w[0].distance(w[1]);
        if len < 1e-3 {
            continue;
        }
        let dir = (w[1] - w[0]) / len;
        let (la, lb) = (levels.get(i).copied().unwrap_or(0.0), levels.get(i + 1).copied().unwrap_or(0.0));
        let n = (len / STEP).ceil().max(1.0) as usize;
        let first = if out.is_empty() { 0 } else { 1 };
        for k in first..=n {
            let f = k as f32 / n as f32;
            out.push(Sample { s: acc + len * f, p: w[0].lerp(w[1], f), dir, level: la + (lb - la) * f });
        }
        acc += len;
    }
    out
}

/// Place the columns of a raised way along `pts` (the built polyline) at
/// `levels`, `half_width` its deck's half width. `site` says what stands at
/// a point; `held` are columns already standing (another way's, or this
/// way's from before) that carry this deck too where they are under it.
pub fn plan(pts: &[Vec2], levels: &[f32], half_width: f32, way: u64, site: &dyn Fn(Vec2, f32) -> Site, held: &[Vec2]) -> PillarPlan {
    let mut out = PillarPlan::default();
    if pts.len() < 2 || !is_raised(levels) {
        return out;
    }
    let sm = samples(pts, levels);
    let n = sm.len();
    if n < 2 {
        return out;
    }
    let total = sm[n - 1].s;
    // What each sample is: ground (no column needed), held up already, a
    // site with its cost, or blocked.
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Ground,
        Held,
        Site(Site),
    }
    let kinds: Vec<Kind> = sm
        .iter()
        .map(|x| {
            if x.level < crate::roads::VIADUCT_LEVEL {
                Kind::Ground
            } else if held.iter().any(|h| h.distance(x.p) < 3.0) {
                Kind::Held
            } else {
                Kind::Site(site(x.p, x.level))
            }
        })
        .collect();
    let support = |k: Kind| !matches!(k, Kind::Site(Site::Blocked | Site::Road));
    let pillar_cost = |k: Kind| match k {
        Kind::Ground | Kind::Held => 0.0,
        Kind::Site(s) => s.cost(),
    };
    // Nodes of the search: a virtual start, every sample that can carry the
    // deck, a virtual end. A raised end must have a column within reach.
    let start_s = if kinds[0] == Kind::Ground { 0.0 } else { -(PILLAR_MAX_SPAN - PILLAR_END_REACH) };
    let end_s = if kinds[n - 1] == Kind::Ground { total } else { total + (PILLAR_MAX_SPAN - PILLAR_END_REACH) };
    let mut idx: Vec<usize> = vec![usize::MAX];
    idx.extend((0..n).filter(|&i| support(kinds[i])));
    idx.push(usize::MAX - 1);
    let m = idx.len();
    let pos_s = |k: usize| match idx[k] {
        usize::MAX => start_s,
        x if x == usize::MAX - 1 => end_s,
        i => sm[i].s,
    };
    let kind_of = |k: usize| match idx[k] {
        usize::MAX => Kind::Ground,
        x if x == usize::MAX - 1 => Kind::Ground,
        i => kinds[i],
    };
    // Shortest path: a span costs by how far it is from the ideal, a
    // column by its site; a span past the limit only where nothing can
    // stand in between (a gap), and then dearly.
    let span_cost = |g: f32| if g <= PILLAR_SPAN { (PILLAR_SPAN - g) * 0.12 } else { (g - PILLAR_SPAN) * 0.5 };
    let mut cost = vec![f32::INFINITY; m];
    let mut prev = vec![usize::MAX; m];
    cost[0] = 0.0;
    for k in 1..m {
        let sk = pos_s(k);
        let kk = kind_of(k);
        let mut best = (f32::INFINITY, usize::MAX);
        for j in (0..k).rev() {
            let sj = pos_s(j);
            let g = sk - sj;
            let kj = kind_of(j);
            let both_ground = matches!(kj, Kind::Ground | Kind::Held) && matches!(kk, Kind::Ground | Kind::Held);
            let c = if g <= PILLAR_MAX_SPAN + 1e-3 {
                if g < PILLAR_MIN_SPAN && !both_ground && !(matches!(kj, Kind::Ground) || matches!(kk, Kind::Ground)) {
                    continue;
                }
                // Supports on the ground are continuous: no cost between them.
                if both_ground && kj == Kind::Ground && kk == Kind::Ground && g <= STEP * 1.5 { 0.0 } else { span_cost(g) }
            } else if j + 1 == k {
                // Nothing can stand between these two: a gap.
                1.0e5 + g
            } else {
                break;
            };
            let total_c = cost[j] + c + pillar_cost(kk);
            if total_c < best.0 {
                best = (total_c, j);
            }
        }
        cost[k] = best.0;
        prev[k] = best.1;
    }
    // Back along the cheapest chain.
    let mut chain = Vec::new();
    let mut k = m - 1;
    while k != usize::MAX && k != 0 {
        chain.push(k);
        k = prev[k];
    }
    chain.push(0);
    chain.reverse();
    let point_at = |s: f32| -> Vec2 {
        if s <= 0.0 {
            sm[0].p
        } else if s >= total {
            sm[n - 1].p
        } else {
            let i = sm.partition_point(|x| x.s < s).min(n - 1);
            sm[i].p
        }
    };
    // What stands under an end, for the message.
    let road_near = |from: usize, step: i64| {
        let mut i = from as i64;
        while i >= 0 && (i as usize) < n && (sm[i as usize].s - sm[from].s).abs() <= PILLAR_END_REACH {
            if kinds[i as usize] == Kind::Site(Site::Road) {
                return true;
            }
            i += step;
        }
        false
    };
    let level_at = |s: f32| -> f32 {
        let i = sm.partition_point(|x| x.s < s.clamp(0.0, total)).min(n - 1);
        sm[i].level
    };
    for w in chain.windows(2) {
        let (j, k) = (w[0], w[1]);
        let g = pos_s(k) - pos_s(j);
        let level = level_at((pos_s(j) + pos_s(k)) * 0.5);
        if g > PILLAR_MAX_SPAN + 1e-3 {
            // A gap into the virtual end (or out of the virtual start) is
            // an end in the air with no column in reach.
            if k == m - 1 && kinds[n - 1] != Kind::Ground {
                out.gaps.push(Gap { a: point_at(pos_s(j)), b: sm[n - 1].p, end: true, over_road: road_near(n - 1, -1), level });
            } else if j == 0 && kinds[0] != Kind::Ground {
                out.gaps.push(Gap { a: point_at(pos_s(k)), b: sm[0].p, end: true, over_road: road_near(0, 1), level });
            } else {
                out.gaps.push(Gap { a: point_at(pos_s(j)), b: point_at(pos_s(k)), end: false, over_road: false, level });
            }
        }
    }
    for &k in &chain {
        let i = idx[k];
        if i >= n {
            continue;
        }
        let Kind::Site(s) = kinds[i] else { continue };
        let x = &sm[i];
        out.pillars.push(Pillar { at: x.p, dir: x.dir, half_width, level: x.level, way });
        match s {
            Site::EmptyLot(l) if !out.lots.contains(&l) => out.lots.push(l),
            Site::Buyout(l, b) if !out.buyouts.iter().any(|x| x.1 == b) => out.buyouts.push((l, b)),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(len: f32, level: f32) -> (Vec<Vec2>, Vec<f32>) {
        (vec![Vec2::new(0.0, 0.0), Vec2::new(len, 0.0)], vec![level, level])
    }

    #[test]
    fn spaced_about_the_ideal_on_open_ground() {
        let (p, l) = straight(200.0, 10.0);
        let plan = plan(&p, &l, 6.0, 1, &|_, _| Site::Free, &[]);
        assert!(plan.ok());
        let mut xs: Vec<f32> = plan.pillars.iter().map(|q| q.at.x).collect();
        xs.sort_by(f32::total_cmp);
        assert!(xs[0] <= PILLAR_END_REACH && *xs.last().unwrap() >= 200.0 - PILLAR_END_REACH, "{xs:?}");
        for w in xs.windows(2) {
            assert!(w[1] - w[0] <= PILLAR_MAX_SPAN && w[1] - w[0] >= PILLAR_MIN_SPAN, "{xs:?}");
        }
    }

    #[test]
    fn columns_dodge_a_road_and_a_building() {
        let (p, l) = straight(200.0, 10.0);
        // A street across at x = 40..60, a building at 90..110 (bought out
        // only as a last resort), free elsewhere.
        let site = |q: Vec2, _: f32| {
            if (40.0..60.0).contains(&q.x) {
                Site::Blocked
            } else if (85.0..115.0).contains(&q.x) {
                Site::Buyout(LotId::default(), BuildingId::default())
            } else {
                Site::Free
            }
        };
        let plan = plan(&p, &l, 6.0, 1, &site, &[]);
        assert!(plan.ok());
        assert!(plan.buyouts.is_empty(), "a free site was in reach");
        assert!(plan.pillars.iter().all(|q| !(40.0..60.0).contains(&q.at.x) && !(85.0..115.0).contains(&q.at.x)));
    }

    #[test]
    fn a_long_blocked_stretch_is_a_gap() {
        let (p, l) = straight(200.0, 10.0);
        let plan = plan(&p, &l, 6.0, 1, &|q: Vec2, _| if (50.0..120.0).contains(&q.x) { Site::Blocked } else { Site::Free }, &[]);
        assert!(!plan.ok());
        assert!(plan.refusal().is_some());
        let g = plan.gaps[0];
        assert!(!g.end && g.a.x <= 50.0 && g.b.x >= 120.0, "{g:?}");
    }

    #[test]
    fn ramp_feet_need_no_columns() {
        let p = vec![Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0), Vec2::new(140.0, 0.0), Vec2::new(200.0, 0.0)];
        let l = vec![0.0, 10.0, 10.0, 0.0];
        let plan = plan(&p, &l, 6.0, 1, &|_, _| Site::Free, &[]);
        assert!(plan.ok());
        assert!(plan.pillars.iter().all(|q| q.level >= crate::roads::VIADUCT_LEVEL));
        assert!(plan.pillars.len() >= 3);
    }

    /// The key is saved, so it is pinned: the same way gives the same key
    /// on any toolchain. (A change here orphans every saved column.)
    #[test]
    fn way_keys_are_pinned() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(120.0, 35.5)];
        let levels = [0.0, 8.0];
        assert_eq!(way_key(&pts, &levels), way_key(&pts, &levels));
        assert_ne!(way_key(&pts, &levels), way_key(&pts, &[0.0, 8.5]));
        assert_eq!(way_key(&pts, &levels), PINNED, "way_key changed: {:#x}", way_key(&pts, &levels));
    }
    const PINNED: u64 = 0x6aab_3054_84e0_9126;

    #[test]
    fn columns_saved_under_the_old_key_take_the_new_one() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(120.0, 35.5)];
        let levels = [0.0, 8.0];
        let col = |way| Pillar { at: Vec2::ZERO, dir: Vec2::X, half_width: 5.0, level: 8.0, way };
        let mut cols = [col(legacy_way_key(&pts, &levels)), col(way_key(&pts, &levels)), col(12345)];
        migrate_way_keys(&mut cols, [(&pts[..], &levels[..])]);
        assert_eq!(cols.map(|c| c.way), [way_key(&pts, &levels), way_key(&pts, &levels), 12345]);
    }
}
