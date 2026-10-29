//! [fishing] The trawlers of the fishing ports and the grounds they fish.
//!
//! Every fishing port keeps a fleet: one trawler per berth on its quay
//! (`quay_moorings`). A boat that goes out leaves its berth empty. On a
//! working day a crewed boat casts off in the early morning (staggered
//! between 03:00 and 08:00), steams out to one of the port's fishing
//! grounds, trawls there for hours in slow sweeps with its nets out,
//! stopping now and then to haul them, and heads home to be back between
//! 12:00 and 17:00, where it unloads its catch onto the quay. The catch
//! is what the port sells: it grows with the minutes fished, is richer on
//! the grounds further out and poorer on water fouled by dirty industry.
//! A storm keeps the fleet in (and calls back any boat out).
//!
//! Fishing grounds are found once per port from the landscape: spots of
//! open water (the sea, big lakes, wide rivers) far enough from the shore,
//! reachable from the quay by water, clear of the shipping lanes and ferry
//! routes, picked where the map's seeded "fish" field is richest. Each
//! keeps the route to it from the quay, found on a grid over the water so
//! a boat never crosses land.
//!
//! The data here is saved with the world (`World::fishing`); the stepping
//! is `world/fishing.rs`. Nothing here draws a random number.

use crate::buildings::BuildingId;
use crate::landscape::Landscape;
use crate::ports::{Port, PortKind};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Steaming speed out and home, and trawling speed with the nets out, m/s.
pub const STEAM_MPS: f32 = 4.5;
pub const TRAWL_MPS: f32 = 1.5;
/// Boats cast off between `DEPART_FROM` and `DEPART_FROM + DEPART_SPREAD`
/// (minutes of the day); one that missed its time stays in after
/// `LAST_DEPARTURE`.
pub const DEPART_FROM: u32 = 3 * 60;
pub const DEPART_SPREAD: u32 = 5 * 60;
pub const LAST_DEPARTURE: u32 = 9 * 60;
/// Each aims to be home between `HOME_FROM` and `HOME_FROM + HOME_SPREAD`.
pub const HOME_FROM: u32 = 12 * 60;
pub const HOME_SPREAD: u32 = 5 * 60;
/// Anything still out at this hour heads in.
pub const CURFEW: u32 = 20 * 60;
/// Minutes of trawling between hauls, and a haul's length.
pub const HAUL_EVERY: u32 = 50;
pub const HAUL_MIN: u32 = 12;
/// Minutes at the quay unloading.
pub const UNLOAD_MIN: u32 = 40;
/// One day in this many a boat stays in for repairs.
pub const REST_DAY_IN: u64 = 8;
/// Minutes of fishing a typical trip gets, for the catch rate: the port
/// lands about what its recipe used to make when its boats fish this long.
pub const NOMINAL_FISH_MIN: f32 = 450.0;
/// Share of the catch lost on water fully fouled by dirty industry.
pub const FOUL_HIT: f32 = 0.7;
/// The water grid's cell, metres.
pub const GRID: f32 = 20.0;
/// Metres from the shore a boat keeps to on its way.
pub const PASSAGE_CLEAR: f32 = 7.0;
/// Metres of open water round a fishing ground's middle, at least.
pub const GROUND_CLEAR: f32 = 32.0;
/// Where there is no such water within reach (a river, a small lake), the
/// middle of the water at least this far from the banks.
pub const RIVER_GROUND_CLEAR: f32 = 16.0;
/// Route length to a ground, shortest and longest.
pub const GROUND_MIN_RUN: f32 = 300.0;
pub const GROUND_MAX_RUN: f32 = 2600.0;
/// Grounds keep this far from shipping lanes and ferry routes.
pub const LANE_CLEAR: f32 = 140.0;
/// Grounds per port, and how far apart they lie.
pub const MAX_GROUNDS: usize = 5;
pub const GROUND_SPACING: f32 = 320.0;

/// Where a trawler is in its day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrawlerPhase {
    /// Tied up at its berth.
    #[default]
    InPort,
    /// Steaming out to the ground.
    Outbound,
    /// Trawling with the nets out.
    Fishing,
    /// Stopped, hauling the nets in (gulls round it).
    Hauling,
    /// Steaming home.
    Inbound,
    /// At the berth, unloading boxes of fish onto the quay.
    Unloading,
}

impl TrawlerPhase {
    pub fn label(self) -> &'static str {
        match self {
            TrawlerPhase::InPort => "in port",
            TrawlerPhase::Outbound => "steaming out",
            TrawlerPhase::Fishing => "fishing",
            TrawlerPhase::Hauling => "hauling nets",
            TrawlerPhase::Inbound => "returning",
            TrawlerPhase::Unloading => "unloading",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TrawlerPhase::InPort => "in_port",
            TrawlerPhase::Outbound => "outbound",
            TrawlerPhase::Fishing => "fishing",
            TrawlerPhase::Hauling => "hauling",
            TrawlerPhase::Inbound => "returning",
            TrawlerPhase::Unloading => "unloading",
        }
    }

    /// Out on the water (its berth empty).
    pub fn at_sea(self) -> bool {
        matches!(self, TrawlerPhase::Outbound | TrawlerPhase::Fishing | TrawlerPhase::Hauling | TrawlerPhase::Inbound)
    }

    /// Nets in the water.
    pub fn nets_out(self) -> bool {
        matches!(self, TrawlerPhase::Fishing | TrawlerPhase::Hauling)
    }

    /// Under way between the quay and the ground.
    pub fn steaming(self) -> bool {
        matches!(self, TrawlerPhase::Outbound | TrawlerPhase::Inbound)
    }
}

/// A spot of open water a port's boats fish.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FishingGround {
    pub center: Vec2,
    /// Room to trawl round the middle, metres.
    pub radius: f32,
    /// How rich it is, about `0.8..1.3` (further out is richer).
    pub richness: f32,
    /// The way out from the quay's offing to the middle, on water.
    pub route: Vec<Vec2>,
}

impl FishingGround {
    pub fn run(&self) -> f32 {
        crate::geom::path_length(&self.route)
    }
}

/// One trawler of a fishing port.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trawler {
    pub name: String,
    /// Its berth on the quay (index into `Fleet::moorings`).
    pub slot: u8,
    pub phase: TrawlerPhase,
    /// The leg it is on (out, the sweep it is trawling, home).
    pub path: Vec<Vec2>,
    pub s: f32,
    pub prev_s: f32,
    /// Minutes left hauling or unloading.
    pub left: u32,
    /// The ground it fishes this trip (index into `Fleet::grounds`), its
    /// middle and room to trawl.
    pub ground: Option<u8>,
    pub spot: Option<(Vec2, f32)>,
    /// The way it came out from its berth (it goes home the same way).
    pub out: Vec<Vec2>,
    /// Fish per minute fished on this trip's ground.
    pub rate: f32,
    /// Minutes fished this trip, and the sweeps done (each one turned a
    /// little from the last).
    pub fished: u32,
    pub sweeps: u32,
    /// Fish aboard.
    pub hold: f32,
    /// Landed today.
    pub catch_today: f32,
    /// The day it last went out.
    pub trip_day: Option<u64>,
    /// Minute of the day it means to be home by.
    pub home_by: u32,
}

impl Trawler {
    /// Where it is on its leg, and its heading.
    pub fn pose_at(&self, s: f32) -> (Vec2, Vec2) {
        crate::geom::point_along(&self.path, s)
    }
}

/// A fishing port's boats and grounds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fleet {
    pub kind: Option<PortKind>,
    pub boats: Vec<Trawler>,
    /// Berths on the quay: position and bow heading (radians).
    pub moorings: Vec<(Vec2, f32)>,
    pub grounds: Vec<FishingGround>,
    /// Where the boats gather off the quay head before heading out.
    pub offing: Vec2,
    pub landed_today: f32,
    pub landed_yesterday: f32,
    pub trips_today: u32,
    /// Why the boats stayed in today, if they did.
    pub kept_in: Option<String>,
    /// [sweep-visual] The fleet of a fishing company's private jetty
    /// (`shore::Jetty`, one trawler at its berth), not a fishing port's.
    pub jetty: bool,
}

impl Fleet {
    pub fn at_sea(&self) -> usize {
        self.boats.iter().filter(|b| b.phase.at_sea()).count()
    }

    /// The mooring of a boat (its berth, or the quay head if the quay has
    /// none).
    pub fn mooring(&self, slot: u8) -> (Vec2, f32) {
        self.moorings.get(slot as usize).copied().unwrap_or((self.offing, 0.0))
    }

    /// Where a boat is now (between the last tick and this at `frac`) and
    /// its heading.
    pub fn boat_pose(&self, k: usize, frac: f32) -> (Vec2, Vec2) {
        let b = &self.boats[k];
        if b.phase.at_sea() && b.path.len() >= 2 {
            let s = b.prev_s + (b.s - b.prev_s) * frac.clamp(0.0, 1.0);
            b.pose_at(s)
        } else {
            let (p, yaw) = self.mooring(b.slot);
            (p, Vec2::from_angle(yaw))
        }
    }
}

/// Every fishing port's fleet, saved with the world.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fishing {
    pub fleets: BTreeMap<BuildingId, Fleet>,
    /// What the grounds were found for (the docks), to find them again
    /// when a dock is built or pulled down.
    pub key: u64,
}

/// The fishing fleet's hash: [`crate::hash::mix2`] salted.
pub fn hash(a: u64, b: u64) -> u64 {
    crate::hash::mix2_salted(a, b, 0xF15F_15F1)
}

const NAMES: [&str; 24] = [
    "Morning Star", "Sea Lark", "Northern Dawn", "Kittiwake", "Silver Darling", "Good Intent", "Provider", "Ocean Harvest", "Brighter Morn", "Harvest Moon", "Boy Andrew", "Fair Wind",
    "Guiding Light", "Sea Holly", "Pilot Whale", "Endeavour", "Grey Heron", "Neon Tide", "Starlight", "Marigold", "Sardine Queen", "Tern", "Bonnie Lass", "Night Heron",
];

/// A boat's name, different for every berth of one port.
pub fn boat_name(port_seed: u64, slot: usize) -> String {
    let start = (hash(port_seed, 77) % NAMES.len() as u64) as usize;
    NAMES[(start + slot * 7) % NAMES.len()].to_string()
}

/// Length of the T-head across a fishing port's quay.
pub fn quay_head(kind: PortKind) -> f32 {
    if kind == PortKind::FishingPortLarge {
        56.0
    } else {
        36.0
    }
}

/// Length and beam of a trawler, metres.
pub const TRAWLER_SIZE: (f32, f32) = (16.0, 5.0);

/// The berths of a fishing port's quay (with its T-head): trawlers on the
/// head's seaward face first, then alongside the quay, then on the head's
/// landward arms; only where a boat lies wholly on water, at most
/// `PortKind::berths()`. Position and bow heading (radians). Deterministic
/// from the port; the renderer draws the moored boats here.
pub fn quay_moorings(port: &Port, land: &Landscape) -> Vec<(Vec2, f32)> {
    let kind = port.kind;
    if !kind.is_fishing() {
        return Vec::new();
    }
    let berths = kind.berths() as usize;
    let (_, width) = kind.pier();
    let [root, tip] = port.pier;
    let len = root.distance(tip);
    if len < 2.0 {
        return Vec::new();
    }
    let along = (tip - root) / len;
    let across = Vec2::new(-along.y, along.x);
    let mut s_water = 0.0;
    while s_water < len && !land.is_water(root + along * s_water) {
        s_water += 1.0;
    }
    let (l, b) = TRAWLER_SIZE;
    let afloat = |p: Vec2, dir: Vec2| -> bool {
        let n = Vec2::new(-dir.y, dir.x);
        [p, p + dir * l * 0.5, p - dir * l * 0.5, p + n * b * 0.5, p - n * b * 0.5].iter().all(|q| land.is_water(*q))
    };
    let yaw_of = |d: Vec2| d.y.atan2(d.x);
    let head = quay_head(kind);
    let hw = width;
    let mut slots: Vec<(Vec2, f32)> = Vec::new();
    let n_sea = ((head / (l + 2.0)).floor() as i32).max(1);
    for i in 0..n_sea {
        let x = -head * 0.5 + (i as f32 + 0.5) * head / n_sea as f32;
        slots.push((tip + along * (hw * 0.5 + b * 0.5 + 0.5) + across * x, yaw_of(if i % 2 == 0 { across } else { -across })));
    }
    let limit = len - hw * 0.5 - b - 1.5;
    let mut s = s_water + l * 0.5 + 1.0;
    while s + l * 0.5 <= limit {
        for side in [1.0f32, -1.0] {
            slots.push((root + along * s + across * side * (width * 0.5 + b * 0.5 + 0.5), yaw_of(along)));
        }
        s += l + 2.0;
    }
    let arm = head * 0.5 - width * 0.5;
    let n_arm = (arm / (l + 2.0)).floor() as i32;
    for side in [1.0f32, -1.0] {
        for i in 0..n_arm {
            let x = width * 0.5 + 1.0 + (i as f32 + 0.5) * (l + 2.0);
            slots.push((tip - along * (hw * 0.5 + b * 0.5 + 0.5) + across * side * x, yaw_of(across * side)));
        }
    }
    let mut out = Vec::new();
    for m in slots {
        if out.len() >= berths {
            break;
        }
        if afloat(m.0, Vec2::from_angle(m.1)) {
            out.push(m);
        }
    }
    out
}

/// Where boats gather off a fishing port's quay head: out past the head,
/// on water.
pub fn offing(port: &Port, land: &Landscape) -> Vec2 {
    let [root, tip] = port.pier;
    let along = (tip - root).normalize_or_zero();
    let (_, hw) = port.kind.pier();
    for d in [24.0, 18.0, 13.0] {
        let p = tip + along * (hw * 0.5 + d);
        if land.is_water(p) {
            return p;
        }
    }
    port.berth
}

/// The way from a berth out to the offing, round the quay head: straight
/// out from the head's seaward face, else along the quay, round the end
/// of the head and out. All on water (falls back to a straight line).
pub fn leave_quay(port: &Port, mooring: Vec2, offing: Vec2, land: &Landscape) -> Vec<Vec2> {
    let [root, tip] = port.pier;
    let along = (tip - root).normalize_or_zero();
    let across = Vec2::new(-along.y, along.x);
    let (_, hw) = port.kind.pier();
    let head = quay_head(port.kind);
    let rel = mooring - tip;
    let a = rel.dot(along);
    let x = rel.dot(across);
    let pts = if a > 0.0 {
        vec![mooring, mooring + along * 10.0, offing]
    } else {
        let side = if x >= 0.0 { 1.0 } else { -1.0 };
        let wide = head * 0.5 + 10.0;
        let p1 = tip + along * a + across * side * wide;
        let p2 = tip + along * (hw * 0.5 + 12.0) + across * side * wide;
        vec![mooring, p1, p2, offing]
    };
    let ok = pts.windows(2).all(|w| line_wet(land, w[0], w[1], 3.0, 0.0));
    let mut pts = if ok { pts } else { vec![mooring, offing] };
    pts.dedup_by(|a, b| a.distance(*b) < 0.5);
    pts
}

/// [sweep-visual] A private fishing jetty's one berth: its trawler lies
/// alongside, bow out along the jetty.
pub fn jetty_mooring(j: &crate::shore::Jetty) -> (Vec2, f32) {
    let d = (j.tip - j.root).normalize_or(j.seaward);
    (j.berth, d.y.atan2(d.x))
}

/// [sweep-visual] Where a jetty's trawler heads out to before making for
/// its grounds: out past the jetty's end, on water.
pub fn jetty_offing(j: &crate::shore::Jetty, land: &Landscape) -> Vec2 {
    let out = j.seaward.normalize_or((j.tip - j.root).normalize_or_zero());
    for d in [30.0, 22.0, 15.0, 9.0] {
        let p = j.tip + out * d;
        if land.is_water(p) && line_wet(land, j.berth, p, 3.0, 0.0) {
            return p;
        }
    }
    j.berth
}

/// [sweep-visual] The way from a jetty's berth out to its offing: astern
/// off the berth a few metres, then out (a straight line where that is not
/// all on water).
pub fn leave_jetty(j: &crate::shore::Jetty, offing: Vec2, land: &Landscape) -> Vec<Vec2> {
    let off = j.berth + j.seaward.normalize_or_zero() * 8.0;
    let pts = vec![j.berth, off, offing];
    let mut pts = if pts.windows(2).all(|w| line_wet(land, w[0], w[1], 3.0, 0.0)) { pts } else { vec![j.berth, offing] };
    pts.dedup_by(|a, b| a.distance(*b) < 0.5);
    pts
}

/// Every sample along `a → b` (each `step` metres) is water at least
/// `clear` metres from the shore.
pub fn line_wet(land: &Landscape, a: Vec2, b: Vec2, step: f32, clear: f32) -> bool {
    let n = (a.distance(b) / step).ceil().max(1.0) as i32;
    (0..=n).all(|k| {
        let p = a.lerp(b, k as f32 / n as f32);
        land.is_water(p) && (clear <= 0.0 || -land.water_distance(p) >= clear)
    })
}

/// A grid over the map's water: each cell's distance from the shore.
pub struct WaterGrid {
    pub n: usize,
    pub half: f32,
    pub clear: Vec<f32>,
}

impl WaterGrid {
    pub fn build(land: &Landscape) -> Self {
        // [r4-tiles] Over the land bought too (the map's edge less 20 m).
        let half = land.land_half() - 60.0;
        let n = ((half * 2.0) / GRID).ceil() as usize;
        let mut clear = vec![0.0; n * n];
        if !land.is_flat() {
            for j in 0..n {
                for i in 0..n {
                    let p = Self::center_of(half, i, j);
                    let d = land.water_distance(p);
                    if d < 0.0 {
                        clear[j * n + i] = -d;
                    }
                }
            }
        }
        Self { n, half, clear }
    }

    fn center_of(half: f32, i: usize, j: usize) -> Vec2 {
        Vec2::new(-half + (i as f32 + 0.5) * GRID, -half + (j as f32 + 0.5) * GRID)
    }

    pub fn center(&self, k: usize) -> Vec2 {
        Self::center_of(self.half, k % self.n, k / self.n)
    }

    pub fn cell(&self, p: Vec2) -> Option<usize> {
        let i = ((p.x + self.half) / GRID).floor();
        let j = ((p.y + self.half) / GRID).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.n || j as usize >= self.n {
            return None;
        }
        Some(j as usize * self.n + i as usize)
    }

    /// The nearest passable cell to `p`, within a few cells.
    fn start_cell(&self, p: Vec2) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        let c = self.cell(p)?;
        let (ci, cj) = ((c % self.n) as i64, (c / self.n) as i64);
        for dj in -4i64..=4 {
            for di in -4i64..=4 {
                let (i, j) = (ci + di, cj + dj);
                if i < 0 || j < 0 || i as usize >= self.n || j as usize >= self.n {
                    continue;
                }
                let k = j as usize * self.n + i as usize;
                if self.clear[k] >= PASSAGE_CLEAR {
                    let d = self.center(k).distance(p);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, k));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Shortest water distances from `from` to every passable cell (and
    /// the parent of each), preferring the middle of the water.
    pub fn flood(&self, from: Vec2, max: f32) -> Option<(usize, Vec<f32>, Vec<u32>)> {
        let start = self.start_cell(from)?;
        let n = self.n;
        let mut dist = vec![f32::INFINITY; n * n];
        let mut parent = vec![u32::MAX; n * n];
        let mut heap = std::collections::BinaryHeap::new();
        dist[start] = 0.0;
        heap.push(std::cmp::Reverse((0u64, start)));
        let diag = std::f32::consts::SQRT_2;
        while let Some(std::cmp::Reverse((dk, k))) = heap.pop() {
            let d = dk as f32 / 16.0;
            if d > dist[k] + 1e-3 || d > max {
                continue;
            }
            let (i, j) = ((k % n) as i64, (k / n) as i64);
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (a, b) = (i + di, j + dj);
                if a < 0 || b < 0 || a as usize >= n || b as usize >= n {
                    continue;
                }
                let q = b as usize * n + a as usize;
                if self.clear[q] < PASSAGE_CLEAR {
                    continue;
                }
                // Diagonals may not cut a corner of land.
                if di != 0 && dj != 0 && (self.clear[j as usize * n + a as usize] < PASSAGE_CLEAR || self.clear[b as usize * n + i as usize] < PASSAGE_CLEAR) {
                    continue;
                }
                let step = if di != 0 && dj != 0 { diag } else { 1.0 } * GRID;
                // Keep off the banks where the water allows.
                let shy = if self.clear[q] < 25.0 { 1.4 } else { 1.0 };
                let nd = d + step * shy;
                if nd < dist[q] {
                    dist[q] = nd;
                    parent[q] = k as u32;
                    heap.push(std::cmp::Reverse(((nd * 16.0) as u64, q)));
                }
            }
        }
        Some((start, dist, parent))
    }

    /// The cell path from the flood's start to `to`, as points, pulled
    /// straight where the water allows, starting at `from`.
    pub fn route(&self, land: &Landscape, from: Vec2, parent: &[u32], to: usize) -> Vec<Vec2> {
        let mut cells = vec![to];
        let mut k = to;
        while parent[k] != u32::MAX && cells.len() < 100_000 {
            k = parent[k] as usize;
            cells.push(k);
        }
        cells.reverse();
        let mut pts: Vec<Vec2> = std::iter::once(from).chain(cells.iter().map(|c| self.center(*c))).collect();
        pts.dedup_by(|a, b| a.distance(*b) < 0.5);
        // String-pull: from each kept point, run on while the water stays
        // in clear sight.
        let mut out = vec![pts[0]];
        let mut i = 0;
        while i + 1 < pts.len() {
            let mut j = i + 1;
            let clear = if i == 0 { 0.0 } else { PASSAGE_CLEAR - 2.0 };
            while j + 1 < pts.len() && line_wet(land, pts[i], pts[j + 1], 6.0, clear) {
                j += 1;
            }
            out.push(pts[j]);
            i = j;
        }
        out
    }
}

/// Distance from `p` to a polyline.
use crate::geom::distance_to_polyline as to_line;

/// The fishing grounds for a port whose boats set out from `offing`:
/// up to `MAX_GROUNDS` spots of open water reachable from there by water,
/// `GROUND_MIN_RUN..GROUND_MAX_RUN` away, clear of `lanes` (shipping and
/// ferry routes) and other docks, where the map's fish field is richest.
/// Deterministic from the landscape seed.
pub fn find_grounds(land: &Landscape, grid: &WaterGrid, offing: Vec2, lanes: &[Vec<Vec2>], docks: &[Vec2]) -> Vec<FishingGround> {
    let Some((_, dist, parent)) = grid.flood(offing, GROUND_MAX_RUN * 1.3) else { return Vec::new() };
    // Open water first; on a river or a small lake, its middle will do.
    for least in [GROUND_CLEAR, RIVER_GROUND_CLEAR] {
        let out = pick_grounds(land, grid, offing, lanes, docks, &dist, &parent, least);
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

#[allow(clippy::too_many_arguments)]
fn pick_grounds(land: &Landscape, grid: &WaterGrid, offing: Vec2, lanes: &[Vec<Vec2>], docks: &[Vec2], dist: &[f32], parent: &[u32], least: f32) -> Vec<FishingGround> {
    let seed = land.params.seed ^ 0xF15B_0A75;
    let edge = land.land_half() - 160.0;
    let mut cands: Vec<(f32, usize, f32, f32)> = Vec::new();
    for (k, &d) in dist.iter().enumerate() {
        if !d.is_finite() || !(GROUND_MIN_RUN..=GROUND_MAX_RUN).contains(&d) {
            continue;
        }
        let clear = grid.clear[k];
        if clear < least {
            continue;
        }
        let p = grid.center(k);
        if p.x.abs() > edge || p.y.abs() > edge {
            continue;
        }
        if lanes.iter().any(|l| to_line(p, l) < LANE_CLEAR) || docks.iter().any(|q| q.distance(p) < 200.0) {
            continue;
        }
        let fish = crate::landscape::fbm(seed, p / 420.0, 3);
        let far = smooth((d - GROUND_MIN_RUN) / (GROUND_MAX_RUN - GROUND_MIN_RUN));
        let room = (clear / 120.0).min(1.0);
        cands.push((fish + 0.25 * far + 0.15 * room, k, d, fish));
    }
    cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<FishingGround> = Vec::new();
    for (_, k, d, fish) in cands {
        if out.len() >= MAX_GROUNDS {
            break;
        }
        let p = grid.center(k);
        if out.iter().any(|g| g.center.distance(p) < GROUND_SPACING) {
            continue;
        }
        let route = grid.route(land, offing, parent, k);
        if route.len() < 2 || !route.windows(2).all(|w| line_wet(land, w[0], w[1], 6.0, 0.0)) {
            continue;
        }
        let far = smooth((d - GROUND_MIN_RUN) / (GROUND_MAX_RUN - GROUND_MIN_RUN));
        let richness = (0.82 + 0.35 * far + 0.3 * (fish - 0.5)).clamp(0.75, 1.3);
        let radius = (grid.clear[k] - 10.0).clamp(8.0, 140.0);
        out.push(FishingGround { center: p, radius, richness, route });
    }
    out
}

fn smooth(t: f32) -> f32 {
    crate::geom::smoothstep(0.0, 1.0, t)
}

/// One sweep over a ground: from the middle out in a zig-zag across it and
/// back to the middle, turned by `turn` radians. Every point keeps clear of
/// the shore (shrunk until it does).
pub fn sweep(land: &Landscape, c: Vec2, radius: f32, turn: f32) -> Vec<Vec2> {
    let u = Vec2::from_angle(turn);
    let v = Vec2::new(-u.y, u.x);
    let mut r = radius;
    for _ in 0..5 {
        let pts = vec![
            c,
            c - u * r + v * (0.45 * r),
            c - u * (0.35 * r) - v * (0.45 * r),
            c + u * (0.35 * r) + v * (0.45 * r),
            c + u * r - v * (0.45 * r),
            c + u * (0.7 * r) + v * (0.8 * r),
            c,
        ];
        if pts.windows(2).all(|w| line_wet(land, w[0], w[1], 8.0, 6.0)) {
            return pts;
        }
        r *= 0.7;
    }
    vec![c, c + u * 6.0, c + v * 6.0, c]
}

/// Length of a path.
pub fn path_len(pts: &[Vec2]) -> f32 {
    crate::geom::path_length(pts)
}
