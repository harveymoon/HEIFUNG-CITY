//! The land under the city: a heightfield with hills, rivers and streams
//! that carry a current, lakes, a sea coast or islands, and geothermal hot
//! spots. Everything is generated from a seed and a few [`LandscapeParams`]
//! (a preset plus sliders), so the same numbers make the same map; the
//! generated features are saved with the world as well, so a later change to
//! the generator never moves a river under a built city.
//!
//! The simulation is two-dimensional. Heights matter in three places:
//! roads keep gentle grades and bridge water (`World::road_z`), lots on
//! water or on steep ground are not built on, and the renderer draws the
//! ground and lifts everything onto it through [`Ground`], a raster of the
//! *built* ground (natural terrain, cut and filled to the roads and levelled
//! under every lot).
//!
//! Water sits at [`WATER_LEVEL`]; land meets it at [`BANK_HEIGHT`].

use crate::geom::{distance_to_outline, offset_polyline, point_in_polygon, project_on_segment};
use crate::roads::RoadNetwork;
use crate::zoning::{Lot, LotId};
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use std::sync::OnceLock;

/// Height of every water surface.
pub const WATER_LEVEL: f32 = 0.0;
/// Height of the land at the water's edge.
pub const BANK_HEIGHT: f32 = 1.2;
/// Half the side of the generated square (the terrain the renderer draws).
pub const LAND_HALF: f32 = 1800.0;
/// The city basin; the outer hills rise beyond it, as on the old flat map.
pub const BASIN_RADIUS: f32 = 1000.0;
pub const OUTER_HILLS: f32 = 160.0;
/// The middle of the map stays dry so a new city always has room to start.
pub const CLEAR_RADIUS: f32 = 240.0;
/// Steepest ground a lot may be built on (rise over run across the lot).
pub const MAX_LOT_SLOPE: f32 = 0.2;
/// Slowest water a hydro plant can use, in metres per second.
pub const MIN_HYDRO_CURRENT: f32 = 0.8;
/// Height of a bridge deck above the water.
pub const BRIDGE_CLEARANCE: f32 = 7.0;
/// Where the highway runs: two carriageways either side of this line.
pub const HIGHWAY_Y: f32 = 420.0;
/// Half the distance between the highway's carriageway centrelines.
pub const HIGHWAY_HALF_GAP: f32 = 9.0;
pub const HIGHWAY_LANES: u8 = 3;

// ── the world beyond the map ────────────────────────────────────────────────
//
// The highway, the trunk line and any road the player draws to the edge of
// the map lead "elsewhere": they run on far past the playable square, out of
// the camera's sight, and that is where traffic from elsewhere appears and
// where traffic leaving is taken off. Nothing is ever spawned or removed
// where the player can see it.

/// A way whose end reaches this far from the centre (on either axis) has
/// reached the edge of the map and leads elsewhere.
pub const MAP_EDGE: f32 = LAND_HALF - 40.0;
/// The orbit camera's eye and target stay within this square.
pub const CAMERA_BOUND: f32 = LAND_HALF + 200.0;
/// Nothing is seen farther than this from the camera: the fog is solid.
pub const SIGHT: f32 = 2600.0;
/// Ways leading elsewhere run out to this square; their ends are where
/// vehicles from elsewhere enter and where those leaving are removed. It is
/// beyond `SIGHT` from anywhere the camera can be.
pub const WAY_REACH: f32 = 5000.0;
/// The ground is drawn out to this square, past anything in sight.
pub const FAR_GROUND: f32 = WAY_REACH + 300.0;

fn chebyshev(p: Vec2) -> f32 {
    p.x.abs().max(p.y.abs())
}

/// Is `p` out where the ways to elsewhere end (beyond anyone's sight)?
pub fn is_elsewhere(p: Vec2) -> bool {
    chebyshev(p) >= WAY_REACH - 1.0
}

/// A way drawn as `points`, with each end that reaches the edge of the map
/// carried on straight out to `WAY_REACH`. Ends already out there, ends
/// short of the edge and ends running along it are left as they are.
pub fn reach_elsewhere(points: &[Vec2]) -> Vec<Vec2> {
    let mut out = points.to_vec();
    if out.len() < 2 {
        return out;
    }
    let extend = |end: Vec2, prev: Vec2| -> Option<Vec2> {
        let r = chebyshev(end);
        if r < MAP_EDGE - 1.0 || r >= WAY_REACH - 1.0 {
            return None;
        }
        let d = (end - prev).normalize_or_zero();
        // Leaving the map, not running along its edge.
        let outward = if end.x.abs() >= end.y.abs() { d.x * end.x.signum() } else { d.y * end.y.signum() };
        if outward < 0.3 {
            return None;
        }
        let mut t = f32::INFINITY;
        for (pc, dc) in [(end.x, d.x), (end.y, d.y)] {
            if dc.abs() > 1e-4 {
                let k = (WAY_REACH * dc.signum() - pc) / dc;
                if k > 0.0 {
                    t = t.min(k);
                }
            }
        }
        t.is_finite().then(|| end + d * t)
    };
    if let Some(p) = extend(out[out.len() - 1], out[out.len() - 2]) {
        out.push(p);
    }
    if let Some(p) = extend(out[0], out[1]) {
        out.insert(0, p);
    }
    out
}

/// River ends at the edge of the map carried on straight out past sight.
fn extend_off_map(pts: &mut Vec<Vec2>) {
    if pts.len() < 2 {
        return;
    }
    let n = pts.len();
    if chebyshev(pts[n - 1]) >= LAND_HALF - 30.0 {
        let d = (pts[n - 1] - pts[n - 2]).normalize_or_zero();
        pts.push(pts[n - 1] + d * (WAY_REACH + 400.0));
    }
    if chebyshev(pts[0]) >= LAND_HALF - 30.0 {
        let d = (pts[0] - pts[1]).normalize_or_zero();
        let p = pts[0] + d * (WAY_REACH + 400.0);
        pts.insert(0, p);
    }
}

const CELL: f32 = 10.0;
/// How far from the water the banks blend into the hills.
const BANK_REACH: f32 = 160.0;
const FAR: f32 = BANK_REACH + 1.0;

// ── parameters ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LandscapePreset {
    /// A big river with tributaries through rolling hills.
    #[default]
    RiverValley,
    /// The city on a large island, smaller islands around it, sea between.
    Islands,
    /// Sea along the south, a river running down to it.
    Coast,
    /// A river fanning into channels across a low plain to the sea.
    Delta,
    /// Low, open land with a slow river, streams and a few lakes.
    Plains,
    /// No water and a flat basin: the original map.
    Flat,
}

impl LandscapePreset {
    pub const ALL: [LandscapePreset; 6] = [
        LandscapePreset::RiverValley,
        LandscapePreset::Islands,
        LandscapePreset::Coast,
        LandscapePreset::Delta,
        LandscapePreset::Plains,
        LandscapePreset::Flat,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LandscapePreset::RiverValley => "River valley",
            LandscapePreset::Islands => "Islands",
            LandscapePreset::Coast => "Coast",
            LandscapePreset::Delta => "Delta",
            LandscapePreset::Plains => "Plains",
            LandscapePreset::Flat => "Flat",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            LandscapePreset::RiverValley => "A wide river with tributaries through rolling hills.",
            LandscapePreset::Islands => "A large island for the city, smaller ones offshore.",
            LandscapePreset::Coast => "Sea to the south and a river running down to it.",
            LandscapePreset::Delta => "A river splitting into channels across a low plain.",
            LandscapePreset::Plains => "Low open land, a slow river, streams and lakes.",
            LandscapePreset::Flat => "The original flat basin: no water.",
        }
    }

    pub fn from_label(s: &str) -> Option<LandscapePreset> {
        let s = s.to_ascii_lowercase().replace([' ', '_', '-'], "");
        Self::ALL.into_iter().find(|p| p.label().to_ascii_lowercase().replace(' ', "") == s || format!("{p:?}").to_ascii_lowercase() == s)
    }
}

/// What the New Game window's levers set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LandscapeParams {
    pub preset: LandscapePreset,
    pub seed: u64,
    /// 0..1: river width, lakes, how far the sea reaches in.
    pub water: f32,
    /// 0..1: hills inside the basin and the height of the outer hills.
    pub relief: f32,
    /// 0..1: tributaries, streams and delta channels.
    pub branching: f32,
    /// Small islands offshore (coast, delta, islands presets).
    pub islands: u8,
    /// Lay the through highway at the start.
    pub highway: bool,
}

impl Default for LandscapeParams {
    fn default() -> Self {
        Self::preset(LandscapePreset::RiverValley, 7)
    }
}

impl LandscapeParams {
    /// The preset's own slider settings.
    pub fn preset(preset: LandscapePreset, seed: u64) -> Self {
        let (water, relief, branching, islands) = match preset {
            LandscapePreset::RiverValley => (0.45, 0.35, 0.35, 0),
            LandscapePreset::Islands => (0.7, 0.3, 0.2, 4),
            LandscapePreset::Coast => (0.5, 0.3, 0.25, 1),
            LandscapePreset::Delta => (0.6, 0.1, 0.7, 2),
            LandscapePreset::Plains => (0.25, 0.08, 0.4, 0),
            LandscapePreset::Flat => (0.0, 0.0, 0.0, 0),
        };
        // Every map has its highway from elsewhere to elsewhere.
        Self { preset, seed, water, relief, branching, islands, highway: true }
    }

    /// The old map: flat, dry, no highway. Tests that need open ground use it.
    pub fn flat() -> Self {
        Self { highway: false, ..Self::preset(LandscapePreset::Flat, 7) }
    }

    fn clamped(&self) -> Self {
        Self { water: self.water.clamp(0.0, 1.0), relief: self.relief.clamp(0.0, 1.0), branching: self.branching.clamp(0.0, 1.0), islands: self.islands.min(8), ..self.clone() }
    }
}

// ── features ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WaterKind {
    River,
    Stream,
    Lake,
    Sea,
}

impl WaterKind {
    pub fn label(self) -> &'static str {
        match self {
            WaterKind::River => "river",
            WaterKind::Stream => "stream",
            WaterKind::Lake => "lake",
            WaterKind::Sea => "sea",
        }
    }
}

/// A river or stream: a centreline from source to mouth (the water flows
/// from the first point to the last), a width and a current.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct River {
    pub points: Vec<Vec2>,
    pub width: f32,
    /// Metres per second.
    pub current: f32,
    pub stream: bool,
}

impl River {
    pub fn kind(&self) -> WaterKind {
        if self.stream { WaterKind::Stream } else { WaterKind::River }
    }

    /// Closest point on the centreline: (segment index, point, distance).
    pub fn nearest(&self, p: Vec2) -> (usize, Vec2, f32) {
        let mut best = (0, self.points[0], f32::INFINITY);
        for (i, w) in self.points.windows(2).enumerate() {
            let (_, q) = project_on_segment(p, w[0], w[1]);
            let d = q.distance(p);
            if d < best.2 {
                best = (i, q, d);
            }
        }
        best
    }

    /// Direction the water flows at segment `i`.
    pub fn flow(&self, i: usize) -> Vec2 {
        let i = i.min(self.points.len().saturating_sub(2));
        (self.points[i + 1] - self.points[i]).normalize_or_zero()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lake {
    pub outline: Vec<Vec2>,
    pub center: Vec2,
}

/// The sea: everything inside `outline` that is not on an island.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sea {
    pub outline: Vec<Vec2>,
    /// Land inside the outline.
    pub islands: Vec<Vec<Vec2>>,
    /// The real shore: the coast polyline (open) for a coast; empty when the
    /// shore is only the islands.
    pub coast: Vec<Vec2>,
    /// Which way is out to sea, for ships leaving the map.
    pub outward: SeaExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SeaExit {
    /// Straight south off the map.
    South,
    /// Radially away from the centre.
    Radial,
}

impl Sea {
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.outline) && !self.islands.iter().any(|i| point_in_polygon(p, i))
    }

    /// Where a ship at `p` leaves the map.
    pub fn exit_from(&self, p: Vec2) -> Vec2 {
        match self.outward {
            // Out of sight, so ships come and go where nobody sees them.
            SeaExit::South => Vec2::new(p.x, -WAY_REACH),
            SeaExit::Radial => p.normalize_or(Vec2::NEG_Y) * WAY_REACH,
        }
    }

    /// Shore polylines: the coast and every island outline (closed).
    fn shores(&self) -> Vec<Vec<Vec2>> {
        let mut out = Vec::new();
        if self.coast.len() >= 2 {
            out.push(self.coast.clone());
        }
        for i in &self.islands {
            let mut ring = i.clone();
            if let Some(&f) = i.first() {
                ring.push(f);
            }
            out.push(ring);
        }
        out
    }
}

/// Hot ground a geothermal plant can tap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HotSpot {
    pub pos: Vec2,
    pub radius: f32,
    /// 0..1: share of a geothermal plant's nameplate it supports.
    pub heat: f32,
}

/// Water at a point: what it is, which feature, how fast and which way it
/// flows, and how wide it is (rivers; lakes and the sea report their size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterHit {
    pub kind: WaterKind,
    pub index: usize,
    pub current: f32,
    pub width: f32,
    pub flow: Vec2,
}

/// The nearest water's edge to a point on land.
#[derive(Clone, Copy, Debug)]
pub struct Shore {
    /// A point on the waterline.
    pub point: Vec2,
    /// Unit vector from the land into the water.
    pub seaward: Vec2,
    pub distance: f32,
    pub water: WaterHit,
}

// ── the landscape ───────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Landscape {
    pub params: LandscapeParams,
    pub rivers: Vec<River>,
    pub lakes: Vec<Lake>,
    pub sea: Option<Sea>,
    pub hot_spots: Vec<HotSpot>,
    #[serde(skip)]
    raster: OnceLock<Raster>,
}

impl Default for Landscape {
    /// The flat map an old save had: no water, hills only outside the basin.
    fn default() -> Self {
        Self { params: LandscapeParams::flat(), rivers: Vec::new(), lakes: Vec::new(), sea: None, hot_spots: Vec::new(), raster: OnceLock::new() }
    }
}

impl std::fmt::Debug for Landscape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Landscape({:?}, {} rivers, {} lakes, sea: {}, {} hot spots)", self.params.preset, self.rivers.len(), self.lakes.len(), self.sea.is_some(), self.hot_spots.len())
    }
}

/// Heights and signed distance to water (negative in the water), sampled
/// every `CELL` metres over the generated square.
struct Raster {
    n: usize,
    h: Vec<f32>,
    d: Vec<f32>,
}

impl Clone for Raster {
    fn clone(&self) -> Self {
        Raster { n: self.n, h: self.h.clone(), d: self.d.clone() }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash2(seed: u64, x: i32, y: i32) -> f32 {
    let mut h = seed ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise in 0..1.
fn value_noise(seed: u64, p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let a = hash2(seed, x, y);
    let b = hash2(seed, x + 1, y);
    let c = hash2(seed, x, y + 1);
    let d = hash2(seed, x + 1, y + 1);
    let top = a + (b - a) * u.x;
    let bottom = c + (d - c) * u.x;
    top + (bottom - top) * u.y
}

/// Fractal noise in 0..1.
pub fn fbm(seed: u64, p: Vec2, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves {
        sum += value_noise(seed.wrapping_add(o as u64 * 1013), q) * amp;
        norm += amp;
        amp *= 0.5;
        q = q * 2.03 + Vec2::new(17.3, -9.1);
    }
    sum / norm
}

/// A wobbly closed outline around `c`.
fn blob(rng: &mut impl Rng, c: Vec2, r: f32, wobble: f32, n: usize) -> Vec<Vec2> {
    let (p1, p2): (f32, f32) = (rng.random_range(0.0..6.28), rng.random_range(0.0..6.28));
    let (k1, k2) = (rng.random_range(2..4) as f32, rng.random_range(4..7) as f32);
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let rr = r * (1.0 + wobble * (0.65 * (a * k1 + p1).sin() + 0.35 * (a * k2 + p2).sin()));
            c + Vec2::new(a.cos(), a.sin()) * rr
        })
        .collect()
}

/// Keep the stretch of a polyline inside the square of half-side `half`,
/// starting at the first point inside.
fn clip_to_box(pts: &[Vec2], half: f32) -> Vec<Vec2> {
    let inside = |p: &Vec2| p.x.abs() <= half && p.y.abs() <= half;
    let start = pts.iter().position(inside).unwrap_or(0);
    let mut out: Vec<Vec2> = Vec::new();
    if start > 0 {
        out.push(pts[start - 1]);
    }
    for p in &pts[start..] {
        out.push(*p);
        if !inside(p) {
            break;
        }
    }
    out
}

/// A meandering line along `dir` at `offset` across it, from `s0` to `s1`.
fn meander(rng: &mut impl Rng, dir: Vec2, offset: f32, s0: f32, s1: f32, amp: f32) -> Vec<Vec2> {
    let across = Vec2::new(-dir.y, dir.x);
    let (p1, p2): (f32, f32) = (rng.random_range(0.0..6.28), rng.random_range(0.0..6.28));
    let (k1, k2) = (std::f32::consts::TAU / rng.random_range(800.0..1100.0), std::f32::consts::TAU / rng.random_range(320.0..420.0));
    let mut out = Vec::new();
    let mut s = s0;
    while s <= s1 {
        let lat = offset + amp * (k1 * s + p1).sin() + amp * 0.22 * (k2 * s + p2).sin();
        out.push(dir * s + across * lat);
        s += 20.0;
    }
    out
}

/// Distance from `p` to a polyline.
fn polyline_distance(pts: &[Vec2], p: Vec2) -> f32 {
    pts.windows(2).map(|w| project_on_segment(p, w[0], w[1]).1.distance(p)).fold(f32::INFINITY, f32::min)
}

impl Landscape {
    /// Generate a landscape from its parameters. Deterministic.
    pub fn generate(params: &LandscapeParams) -> Self {
        let params = params.clamped();
        let mut l = Landscape { params: params.clone(), ..Landscape::default() };
        if params.preset == LandscapePreset::Flat {
            return l;
        }
        let mut rng = crate::rng_for(params.seed, 0x1A4D_5CA9);
        let water = params.water;
        let coast_y = -(1020.0 - water * 360.0);
        match params.preset {
            LandscapePreset::Coast | LandscapePreset::Delta => {
                l.sea = Some(Self::coast_sea(&mut rng, coast_y));
            }
            LandscapePreset::Islands => {
                l.sea = Some(Self::island_sea(&mut rng, water));
            }
            _ => {}
        }
        // The main river.
        let wants_river = match params.preset {
            LandscapePreset::Islands => false,
            LandscapePreset::Coast => water > 0.2,
            _ => true,
        };
        if wants_river {
            let width = match params.preset {
                LandscapePreset::Plains => 24.0 + water * 30.0,
                _ => 34.0 + water * 46.0,
            };
            let current = 0.55 + params.relief * 1.2 + rng.random_range(0.0..0.3);
            let angle: f32 = rng.random_range(-0.22..0.22);
            let dir = Vec2::new(angle.sin(), -angle.cos());
            let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let amp = rng.random_range(90.0..130.0);
            let offset = side * rng.random_range(430.0..600.0);
            let mut pts = meander(&mut rng, dir, offset, -2700.0, 2700.0, amp);
            // Keep the middle dry: push the river out until it clears.
            for _ in 0..8 {
                let near = pts.iter().map(|p| p.length()).fold(f32::INFINITY, f32::min);
                if near >= CLEAR_RADIUS + width * 0.5 + 40.0 {
                    break;
                }

                let shift = Vec2::new(-dir.y, dir.x) * side * 60.0;
                for p in pts.iter_mut() {
                    *p += shift;
                }
            }
            let mut pts = clip_to_box(&pts, LAND_HALF + 60.0);
            if let Some(sea) = &l.sea {
                // Runs into the sea and a little beyond the shore.
                if let Some(i) = pts.iter().position(|p| sea.contains(*p)) {
                    pts.truncate((i + 3).min(pts.len()));
                }
            }
            if params.preset == LandscapePreset::Delta && pts.len() > 10 {
                // Split some way above the coast into channels fanning out.
                let split = pts.iter().position(|p| p.y < coast_y + 330.0).unwrap_or(pts.len() / 2).max(2);
                let at = pts[split];
                let main_dir = (pts[split] - pts[split - 1]).normalize_or(Vec2::NEG_Y);
                pts.truncate(split + 1);
                let channels = 2 + (params.branching * 3.0).round() as usize;
                for k in 0..channels {
                    let spread = (k as f32 / (channels - 1).max(1) as f32 - 0.5) * 1.3;
                    let mut d = Vec2::from_angle(spread).rotate(main_dir);
                    let mut p = at;
                    let mut ch = vec![p];
                    for _ in 0..80 {
                        d = Vec2::from_angle(rng.random_range(-0.12..0.12)).rotate(d).normalize();
                        // Pulled back towards the sea.
                        d = (d + main_dir * 0.08).normalize();
                        p += d * 20.0;
                        ch.push(p);
                        if l.sea.as_ref().is_some_and(|s| s.contains(p)) && ch.len() > 4 {
                            p += d * 40.0;
                            ch.push(p);
                            break;
                        }
                    }
                    l.rivers.push(River { points: ch, width: width * 0.55, current, stream: false });
                }
            }
            extend_off_map(&mut pts);
            l.rivers.insert(0, River { points: pts, width, current, stream: false });
        }
        // Tributaries and streams.
        let tributaries = (params.branching * 4.0).round() as usize;
        if !l.rivers.is_empty() {
            for _ in 0..tributaries {
                let main = &l.rivers[0];
                let n = main.points.len();
                if n < 20 {
                    break;
                }
                let j = rng.random_range(n / 8..n - n / 8);
                let at = main.points[j];
                let up = -main.flow(j);
                let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
                let mut d = Vec2::from_angle(side * rng.random_range(0.7..1.3)).rotate(up);
                let big = params.branching > 0.55 && rng.random_bool(0.3);
                let width = if big { 22.0 + water * 12.0 } else { 9.0 + water * 9.0 + rng.random_range(0.0..4.0) };
                let length = rng.random_range(500.0..1400.0);
                let mut p = at;
                let mut line = vec![p];
                let mut s = 0.0;
                while s < length {
                    d = Vec2::from_angle(rng.random_range(-0.16..0.16)).rotate(d).normalize();
                    let guard = CLEAR_RADIUS + width * 0.5 + 50.0;
                    if (p + d * 20.0).length() < guard {
                        // Turn away from the middle of the map.
                        let out = p.normalize_or(Vec2::X);
                        d = (d + out * 0.9).normalize();
                    }
                    p += d * 20.0;
                    s += 20.0;
                    if p.x.abs() > LAND_HALF + 40.0 || p.y.abs() > LAND_HALF + 40.0 || l.sea.as_ref().is_some_and(|sea| sea.contains(p)) {
                        break;
                    }
                    line.push(p);
                }
                if line.len() < 6 {
                    continue;
                }
                line.reverse();
                extend_off_map(&mut line);
                let current = if big { 0.9 + params.relief * 1.5 } else { 1.2 + params.relief * 2.2 } + rng.random_range(0.0..0.6);
                l.rivers.push(River { points: line, width, current, stream: !big });
            }
        }
        // A stream on the big island.
        if params.preset == LandscapePreset::Islands && params.branching > 0.1 {
            let a: f32 = rng.random_range(0.0..6.28);
            let mut p = Vec2::from_angle(a) * (CLEAR_RADIUS + 120.0);
            let mut d = Vec2::from_angle(a);
            let mut line = vec![p];
            for _ in 0..80 {
                d = Vec2::from_angle(rng.random_range(-0.15..0.15)).rotate(d).normalize();
                p += d * 20.0;
                line.push(p);
                if l.sea.as_ref().is_some_and(|s| s.contains(p)) {
                    break;
                }
            }
            l.rivers.push(River { points: line, width: 12.0 + water * 6.0, current: 1.4 + params.relief * 2.0, stream: true });
        }
        // Lakes.
        let lakes = match params.preset {
            LandscapePreset::RiverValley | LandscapePreset::Plains => (water * 3.0).round() as usize,
            LandscapePreset::Coast => (water * 1.5).round() as usize,
            _ => 0,
        };
        let mut tries = 0;
        while l.lakes.len() < lakes && tries < 60 {
            tries += 1;
            let r = rng.random_range(50.0..(80.0 + water * 110.0));
            let c = Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(380.0..1150.0);
            if c.length() - r < CLEAR_RADIUS + 40.0 || (c.y - HIGHWAY_Y).abs() < r + 60.0 || (c.y - crate::rail::TRUNK_Y).abs() < r + 50.0 {
                continue;
            }
            if l.rivers.iter().any(|rv| polyline_distance(&rv.points, c) < r * 1.25 + rv.width + 40.0) || l.lakes.iter().any(|o| o.center.distance(c) < r * 2.5 + 120.0) || l.sea.as_ref().is_some_and(|s| s.contains(c) || polyline_distance(&s.coast, c) < r * 1.3 + 60.0) {
                continue;
            }
            let outline = blob(&mut rng, c, r, 0.2, 28);
            l.lakes.push(Lake { outline, center: c });
        }
        // Hot spots.
        let spots = 1 + rng.random_range(0..2) + (params.relief > 0.5) as usize;
        let mut tries = 0;
        while l.hot_spots.len() < spots && tries < 60 {
            tries += 1;
            let p = Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(280.0..880.0);
            if (p.y - HIGHWAY_Y).abs() < 70.0 || (p.y - crate::rail::TRUNK_Y).abs() < 60.0 || l.hot_spots.iter().any(|h| h.pos.distance(p) < 300.0) {
                continue;
            }
            if l.water_at(p).is_some() || l.nearest_shore(p, 90.0).is_some() {
                continue;
            }
            l.hot_spots.push(HotSpot { pos: p, radius: rng.random_range(45.0..70.0), heat: rng.random_range(0.7..1.0) });
        }
        l
    }

    fn coast_sea(rng: &mut impl Rng, coast_y: f32) -> Sea {
        let (p1, p2): (f32, f32) = (rng.random_range(0.0..6.28), rng.random_range(0.0..6.28));
        let mut coast = Vec::new();
        let mut x = -LAND_HALF - 300.0;
        while x <= LAND_HALF + 300.0 {
            let y = coast_y + 55.0 * (x * 0.0021 + p1).sin() + 22.0 * (x * 0.0067 + p2).sin();
            coast.push(Vec2::new(x, y));
            x += 25.0;
        }
        let mut outline = coast.clone();
        outline.push(Vec2::new(LAND_HALF + 300.0, -LAND_HALF * 3.0));
        outline.push(Vec2::new(-LAND_HALF - 300.0, -LAND_HALF * 3.0));
        Sea { outline, islands: Vec::new(), coast, outward: SeaExit::South }
    }

    fn island_sea(rng: &mut impl Rng, water: f32) -> Sea {
        let big = LAND_HALF * 3.0;
        let outline = vec![Vec2::new(-big, -big), Vec2::new(big, -big), Vec2::new(big, big), Vec2::new(-big, big)];
        let r0 = 1080.0 - water * 380.0;
        let main = blob(rng, Vec2::ZERO, r0, 0.12, 72);
        Sea { outline, islands: vec![main], coast: Vec::new(), outward: SeaExit::Radial }
    }

    /// Small islands out in the sea; called after generation so they never
    /// sit on a river mouth.
    fn add_islands(&mut self) {
        let Some(sea) = self.sea.as_ref() else { return };
        let want = self.params.islands as usize;
        if want == 0 {
            return;
        }
        let mut rng = crate::rng_for(self.params.seed, 0x15_1A2D);
        let r0 = sea.islands.first().map(|m| m.iter().map(|p| p.length()).fold(0.0, f32::max)).unwrap_or(0.0);
        let mut made: Vec<(Vec2, f32)> = Vec::new();
        let mut tries = 0;
        while made.len() < want && tries < 200 {
            tries += 1;
            let r = rng.random_range(60.0..200.0);
            let c = match sea.outward {
                SeaExit::Radial => Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(r0 + 140.0 + r..LAND_HALF - r).max(r0 + 140.0 + r),
                SeaExit::South => Vec2::new(rng.random_range(-1500.0..1500.0), rng.random_range(-1700.0..-900.0)),
            };
            let shore_gap = if sea.coast.len() >= 2 { polyline_distance(&sea.coast, c) } else { f32::INFINITY };
            if !sea.contains(c) || shore_gap < r * 1.25 + 80.0 || made.iter().any(|(o, orr)| o.distance(c) < r + orr + 80.0) {
                continue;
            }
            if self.rivers.iter().any(|rv| polyline_distance(&rv.points, c) < r * 1.3 + rv.width + 60.0) {
                continue;
            }
            made.push((c, r));
        }
        let outlines: Vec<Vec<Vec2>> = made.iter().map(|&(c, r)| blob(&mut rng, c, r, 0.18, 32)).collect();
        if let Some(sea) = self.sea.as_mut() {
            sea.islands.extend(outlines);
        }
    }

    /// `generate` plus the offshore islands.
    pub fn new(params: &LandscapeParams) -> Self {
        let mut l = Self::generate(params);
        l.add_islands();
        l
    }

    pub fn is_flat(&self) -> bool {
        self.params.preset == LandscapePreset::Flat && self.rivers.is_empty() && self.lakes.is_empty() && self.sea.is_none()
    }

    /// Height of the land with no water carved into it.
    pub fn base_height(&self, p: Vec2) -> f32 {
        let seed = self.params.seed;
        let r = p.length();
        let t = ((r - BASIN_RADIUS) / (LAND_HALF - BASIN_RADIUS)).clamp(0.0, 1.0);
        let ramp = t * t * (3.0 - 2.0 * t);
        let n = fbm(seed ^ 0x51, p * 0.0025, 4);
        if self.params.preset == LandscapePreset::Flat {
            return ramp * OUTER_HILLS * (0.4 + 0.6 * n);
        }
        let relief = self.params.relief;
        let outer = ramp * OUTER_HILLS * (0.4 + 0.6 * n) * (0.45 + relief);
        let hills = fbm(seed ^ 0x77, p * 0.0016, 4);
        let ridges = fbm(seed ^ 0x99, p * 0.0045, 3);
        BANK_HEIGHT + 0.8 + relief * (30.0 * hills * hills + 6.0 * ridges) + outer
    }

    /// Exact test for water at a point.
    pub fn water_at(&self, p: Vec2) -> Option<WaterHit> {
        for (i, r) in self.rivers.iter().enumerate() {
            let (seg, _, d) = r.nearest(p);
            if d <= r.width * 0.5 {
                return Some(WaterHit { kind: r.kind(), index: i, current: r.current, width: r.width, flow: r.flow(seg) });
            }
        }
        for (i, l) in self.lakes.iter().enumerate() {
            if point_in_polygon(p, &l.outline) {
                let size = l.outline.iter().map(|q| q.distance(l.center)).fold(0.0, f32::max) * 2.0;
                return Some(WaterHit { kind: WaterKind::Lake, index: i, current: 0.0, width: size, flow: Vec2::ZERO });
            }
        }
        if let Some(sea) = &self.sea {
            if sea.contains(p) {
                return Some(WaterHit { kind: WaterKind::Sea, index: 0, current: 0.4, width: f32::INFINITY, flow: Vec2::ZERO });
            }
        }
        None
    }

    pub fn is_water(&self, p: Vec2) -> bool {
        self.water_at(p).is_some()
    }

    /// The nearest waterline within `max` metres of a point on land.
    pub fn nearest_shore(&self, p: Vec2, max: f32) -> Option<Shore> {
        if self.is_water(p) {
            return None;
        }
        let mut best: Option<(f32, Vec2)> = None;
        let mut consider = |d: f32, q: Vec2| {
            if d <= max && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        };
        for r in &self.rivers {
            let (_, c, d) = r.nearest(p);
            let dir = (p - c).normalize_or(Vec2::X);
            consider(d - r.width * 0.5, c + dir * (r.width * 0.5));
        }
        for l in &self.lakes {
            let n = l.outline.len();
            for i in 0..n {
                let (_, q) = project_on_segment(p, l.outline[i], l.outline[(i + 1) % n]);
                consider(q.distance(p), q);
            }
        }
        if let Some(sea) = &self.sea {
            for line in sea.shores() {
                for w in line.windows(2) {
                    let (_, q) = project_on_segment(p, w[0], w[1]);
                    consider(q.distance(p), q);
                }
            }
        }
        let (d, q) = best?;
        let seaward = (q - p).normalize_or(Vec2::X);
        // The water just past the line.
        let probe = q + seaward * 2.0;
        let water = self.water_at(probe).or_else(|| self.water_at(q + seaward * 6.0))?;
        Some(Shore { point: q, seaward, distance: d, water })
    }

    fn raster(&self) -> &Raster {
        self.raster.get_or_init(|| self.build_raster())
    }

    fn build_raster(&self) -> Raster {
        let n = (LAND_HALF * 2.0 / CELL) as usize + 1;
        let at = |i: usize, j: usize| Vec2::new(-LAND_HALF + i as f32 * CELL, -LAND_HALF + j as f32 * CELL);
        let mut d = vec![FAR; n * n];
        let cell_range = |lo: f32, hi: f32| -> (usize, usize) {
            let a = (((lo + LAND_HALF) / CELL).floor().max(0.0) as usize).min(n - 1);
            let b = (((hi + LAND_HALF) / CELL).ceil().max(0.0) as usize).min(n - 1);
            (a, b)
        };
        // Rivers: distance to the centreline less the half width.
        for r in &self.rivers {
            let hw = r.width * 0.5;
            for w in r.points.windows(2) {
                let reach = hw + BANK_REACH;
                let (i0, i1) = cell_range(w[0].x.min(w[1].x) - reach, w[0].x.max(w[1].x) + reach);
                let (j0, j1) = cell_range(w[0].y.min(w[1].y) - reach, w[0].y.max(w[1].y) + reach);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let p = at(i, j);
                        let s = project_on_segment(p, w[0], w[1]).1.distance(p) - hw;
                        let k = j * n + i;
                        if s < d[k] {
                            d[k] = s;
                        }
                    }
                }
            }
        }
        // Lakes and the sea: distance to the outline, negative inside.
        let shore_d = |ring: &[Vec2], closed: bool, inside: &dyn Fn(Vec2) -> bool, d: &mut Vec<f32>| {
            let m = ring.len();
            let segs = if closed { m } else { m.saturating_sub(1) };
            let mut u = vec![FAR; n * n];
            let mut touched: Vec<usize> = Vec::new();
            for s in 0..segs {
                let (a, b) = (ring[s], ring[(s + 1) % m]);
                let (i0, i1) = cell_range(a.x.min(b.x) - BANK_REACH, a.x.max(b.x) + BANK_REACH);
                let (j0, j1) = cell_range(a.y.min(b.y) - BANK_REACH, a.y.max(b.y) + BANK_REACH);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let p = at(i, j);
                        let k = j * n + i;
                        let dist = project_on_segment(p, a, b).1.distance(p);
                        if dist < u[k] {
                            if u[k] >= FAR {
                                touched.push(k);
                            }
                            u[k] = dist;
                        }
                    }
                }
            }
            for k in touched {
                let p = at(k % n, k / n);
                let s = if inside(p) { -u[k] } else { u[k] };
                if s < d[k] {
                    d[k] = s;
                }
            }
        };
        for l in &self.lakes {
            let outline = l.outline.clone();
            shore_d(&l.outline, true, &|p| point_in_polygon(p, &outline), &mut d);
            // Deep inside a big lake, beyond the reach of its outline.
            for (k, v) in d.iter_mut().enumerate() {
                if *v >= FAR && point_in_polygon(at(k % n, k / n), &l.outline) {
                    *v = -FAR;
                }
            }
        }
        if let Some(sea) = &self.sea {
            for line in sea.shores() {
                let closed = false;
                shore_d(&line, closed, &|p| sea.contains(p), &mut d);
            }
            for (k, v) in d.iter_mut().enumerate() {
                if *v >= FAR && sea.contains(at(k % n, k / n)) {
                    *v = -FAR;
                }
            }
        }
        let mut h = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let k = j * n + i;
                h[k] = self.carve(self.base_height(at(i, j)), d[k]);
            }
        }
        Raster { n, h, d }
    }

    /// Land height given the base height and the signed distance to water.
    fn carve(&self, base: f32, d: f32) -> f32 {
        if d >= FAR {
            return base;
        }
        if d < 0.0 {
            return WATER_LEVEL - 1.2 - (-d).min(10.0) * 0.35;
        }
        // A flat strip along the water, then a slope no steeper than about
        // one in ten up to the hills.
        let rise = (base - BANK_HEIGHT).max(0.0);
        let reach = (50.0 + 3.0 * rise).clamp(50.0, BANK_REACH - 20.0);
        BANK_HEIGHT + rise * smoothstep(18.0, 18.0 + reach, d)
    }

    fn sample(&self, grid: impl Fn(&Raster) -> &Vec<f32>, p: Vec2) -> f32 {
        let r = self.raster();
        let n = r.n;
        let fx = ((p.x + LAND_HALF) / CELL).clamp(0.0, (n - 1) as f32 - 1e-3);
        let fy = ((p.y + LAND_HALF) / CELL).clamp(0.0, (n - 1) as f32 - 1e-3);
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let g = grid(r);
        let a = g[j * n + i];
        let b = g[j * n + i + 1];
        let c = g[(j + 1) * n + i];
        let d = g[(j + 1) * n + i + 1];
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        top + (bot - top) * ty
    }

    /// Natural ground height (bed height under water).
    pub fn height(&self, p: Vec2) -> f32 {
        if self.is_flat() {
            return self.base_height(p);
        }
        if p.x.abs() > LAND_HALF || p.y.abs() > LAND_HALF {
            return self.carve(self.base_height(p), self.far_water_distance(p));
        }
        self.sample(|r| &r.h, p)
    }

    /// Signed distance to water beyond the raster (the rivers carried off
    /// the map, the open sea), capped like the raster's.
    fn far_water_distance(&self, p: Vec2) -> f32 {
        let mut d = FAR;
        for r in &self.rivers {
            let hw = r.width * 0.5;
            let reach = hw + BANK_REACH;
            for w in r.points.windows(2) {
                if p.x < w[0].x.min(w[1].x) - reach || p.x > w[0].x.max(w[1].x) + reach || p.y < w[0].y.min(w[1].y) - reach || p.y > w[0].y.max(w[1].y) + reach {
                    continue;
                }
                d = d.min(project_on_segment(p, w[0], w[1]).1.distance(p) - hw);
            }
        }
        if let Some(sea) = &self.sea {
            let mut u = FAR;
            let rings = std::iter::once(&sea.outline).chain(sea.islands.iter());
            for ring in rings {
                let m = ring.len();
                for k in 0..m {
                    let (a, b) = (ring[k], ring[(k + 1) % m]);
                    if p.x < a.x.min(b.x) - BANK_REACH || p.x > a.x.max(b.x) + BANK_REACH || p.y < a.y.min(b.y) - BANK_REACH || p.y > a.y.max(b.y) + BANK_REACH {
                        continue;
                    }
                    u = u.min(project_on_segment(p, a, b).1.distance(p));
                }
            }
            let s = if sea.contains(p) { -u } else { u };
            d = d.min(s);
        }
        d
    }

    /// Signed distance to the nearest water, negative in the water, capped
    /// at a couple of hundred metres. Read from the raster.
    pub fn water_distance(&self, p: Vec2) -> f32 {
        if self.is_flat() {
            return FAR;
        }
        self.sample(|r| &r.d, p)
    }

    /// Steepest rise over run between any two of `pts`.
    pub fn slope_over(&self, pts: &[Vec2]) -> f32 {
        let hs: Vec<f32> = pts.iter().map(|p| self.height(*p)).collect();
        let mut worst: f32 = 0.0;
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                let run = pts[i].distance(pts[j]);
                if run > 4.0 {
                    worst = worst.max((hs[i] - hs[j]).abs() / run);
                }
            }
        }
        worst
    }

    /// The river or stream beside an outline (a lot), within `reach` metres:
    /// (river index, current, nearest point on its bank).
    pub fn river_beside(&self, outline: &[Vec2], reach: f32) -> Option<(usize, f32, Vec2)> {
        let mut best: Option<(f32, usize, Vec2)> = None;
        for (i, r) in self.rivers.iter().enumerate() {
            for p in outline {
                let (_, c, d) = r.nearest(*p);
                let gap = d - r.width * 0.5;
                if gap <= reach && best.is_none_or(|(bd, ..)| gap < bd) {
                    best = Some((gap, i, c));
                }
            }
        }
        best.map(|(_, i, c)| (i, self.rivers[i].current, c))
    }

    /// Is an outline on the sea shore (within `reach` metres of the sea)?
    pub fn on_coast(&self, outline: &[Vec2], reach: f32) -> bool {
        outline.iter().any(|p| self.nearest_shore(*p, reach).is_some_and(|s| s.water.kind == WaterKind::Sea))
    }

    /// The hot spot under a point, if any.
    pub fn hot_spot_at(&self, p: Vec2) -> Option<&HotSpot> {
        self.hot_spots.iter().find(|h| h.pos.distance(p) <= h.radius + 25.0)
    }

    /// Water outlines for cutting lots: river strips (in short pieces so a
    /// bend never folds over), lakes, and the sea with its islands as holes.
    pub fn water_polygons(&self) -> Vec<(Vec<Vec2>, Vec<Vec<Vec2>>)> {
        let mut out = Vec::new();
        for r in &self.rivers {
            let hw = r.width * 0.5;
            let n = r.points.len();
            let mut start = 0;
            while start + 1 < n {
                let end = (start + 6).min(n - 1);
                let piece = &r.points[start..=end];
                let left = offset_polyline(piece, hw + 0.5);
                let mut right = offset_polyline(piece, -hw - 0.5);
                right.reverse();
                let mut ring = left;
                ring.extend(right);
                out.push((ring, Vec::new()));
                // Round the joint so consecutive pieces overlap.
                let joint = piece[piece.len() - 1];
                out.push(((0..12).map(|k| joint + Vec2::from_angle(k as f32 / 12.0 * std::f32::consts::TAU) * (hw + 0.5)).collect(), Vec::new()));
                start = end;
            }
        }
        for l in &self.lakes {
            out.push((l.outline.clone(), Vec::new()));
        }
        if let Some(sea) = &self.sea {
            out.push((sea.outline.clone(), sea.islands.clone()));
        }
        out
    }

    /// A route by water from `from` to "elsewhere" (off the map), or to
    /// `to`, as a polyline. Rivers are followed along their centrelines,
    /// tributaries join the river they flow into, river mouths open onto the
    /// sea, and the sea is crossed in straight lines. `None` when the two
    /// are not on connected water (a lake, or different rivers that never
    /// meet) or `from` is not on or by the water.
    pub fn water_route(&self, from: Vec2, to: Option<Vec2>) -> Option<Vec<Vec2>> {
        let g = WaterGraph::build(self);
        g.route(self, from, to)
    }
}

// ── water routing ───────────────────────────────────────────────────────────

/// Nodes along every river centreline; edges along them and where a
/// tributary meets the river it flows into. `exit` marks river ends at the
/// edge of the map; `sea` marks nodes out in the sea.
struct WaterGraph {
    nodes: Vec<Vec2>,
    adj: Vec<Vec<usize>>,
    exit: Vec<bool>,
    sea: Vec<bool>,
}

impl WaterGraph {
    fn build(l: &Landscape) -> WaterGraph {
        let mut g = WaterGraph { nodes: Vec::new(), adj: Vec::new(), exit: Vec::new(), sea: Vec::new() };
        let mut ranges = Vec::new();
        for r in &l.rivers {
            let start = g.nodes.len();
            for (k, p) in r.points.iter().enumerate() {
                g.nodes.push(*p);
                g.adj.push(Vec::new());
                let edge = p.x.abs() >= LAND_HALF - 30.0 || p.y.abs() >= LAND_HALF - 30.0;
                g.exit.push(edge && (k == 0 || k + 1 == r.points.len()));
                g.sea.push(l.sea.as_ref().is_some_and(|s| s.contains(*p)));
                if k > 0 {
                    let i = g.nodes.len() - 1;
                    g.adj[i].push(i - 1);
                    g.adj[i - 1].push(i);
                }
            }
            ranges.push(start..g.nodes.len());
        }
        // Ends of one river that lie in another join it.
        for (ri, r) in l.rivers.iter().enumerate() {
            for end in [ranges[ri].start, ranges[ri].end - 1] {
                let p = g.nodes[end];
                for (rj, o) in l.rivers.iter().enumerate() {
                    if rj == ri {
                        continue;
                    }
                    let (seg, _, d) = o.nearest(p);
                    if d <= o.width * 0.5 + r.width * 0.5 + 25.0 {
                        let k = ranges[rj].start + seg;
                        g.adj[end].push(k);
                        g.adj[k].push(end);
                    }
                }
            }
        }
        g
    }

    fn nearest(&self, l: &Landscape, p: Vec2) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (ri, r) in l.rivers.iter().enumerate() {
            let (seg, _, d) = r.nearest(p);
            if d <= r.width * 0.5 + 80.0 && best.is_none_or(|(bd, _)| d < bd) {
                let base: usize = l.rivers[..ri].iter().map(|r| r.points.len()).sum();
                best = Some((d, base + seg));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Dijkstra from `start` until `goal(node)`. Distances are exact (a
    /// rounded key once let a zero-length edge point two nodes at each other
    /// and the path never ended).
    fn search(&self, start: usize, goal: impl Fn(usize) -> bool) -> Option<Vec<usize>> {
        #[derive(PartialEq)]
        struct Item(f32, usize);
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
            }
        }
        let n = self.nodes.len();
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![usize::MAX; n];
        let mut done = vec![false; n];
        let mut heap = std::collections::BinaryHeap::new();
        dist[start] = 0.0;
        heap.push(Item(0.0, start));
        while let Some(Item(du, u)) = heap.pop() {
            if done[u] {
                continue;
            }
            done[u] = true;
            if goal(u) {
                let mut path = vec![u];
                let mut c = u;
                while prev[c] != usize::MAX && path.len() <= n {
                    c = prev[c];
                    path.push(c);
                }
                path.reverse();
                return Some(path);
            }
            for &v in &self.adj[u] {
                let nd = du + self.nodes[u].distance(self.nodes[v]);
                if !done[v] && nd < dist[v] {
                    dist[v] = nd;
                    prev[v] = u;
                    heap.push(Item(nd, v));
                }
            }
        }
        None
    }

    fn route(&self, l: &Landscape, from: Vec2, to: Option<Vec2>) -> Option<Vec<Vec2>> {
        let in_sea = |p: Vec2| l.sea.as_ref().is_some_and(|s| s.contains(p));
        let lake_of = |p: Vec2| l.lakes.iter().position(|k| point_in_polygon(p, &k.outline));
        // The sea and lakes are crossed straight.
        if let Some(t) = to {
            if (in_sea(from) && in_sea(t)) || lake_of(from).is_some_and(|a| lake_of(t) == Some(a)) {
                return Some(vec![from, t]);
            }
        } else if in_sea(from) {
            return Some(vec![from, l.sea.as_ref()?.exit_from(from)]);
        }
        let start = self.nearest(l, from);
        let mut out = vec![from];
        let Some(start) = start else {
            // Out in the sea with a river goal: straight to its mouth below.
            if in_sea(from) {
                if let Some(t) = to {
                    let goal = self.nearest(l, t)?;
                    let path = self.search(goal, |k| self.sea[k])?;
                    out.extend(path.iter().rev().map(|&k| self.nodes[k]));
                    out.push(t);
                    return Some(out);
                }
            }
            return None;
        };
        match to {
            None => {
                let path = self.search(start, |k| self.exit[k] || self.sea[k])?;
                out.extend(path.iter().map(|&k| self.nodes[k]));
                let last = *path.last()?;
                if self.sea[last] {
                    let p = self.nodes[last];
                    out.push(l.sea.as_ref()?.exit_from(p));
                } else {
                    // Carry on past the edge.
                    let p = self.nodes[last];
                    let q = self.nodes[*path.get(path.len().wrapping_sub(2)).unwrap_or(&last)];
                    out.push(p + (p - q).normalize_or(p.normalize_or(Vec2::X)) * 120.0);
                }
            }
            Some(t) => {
                if in_sea(t) {
                    let path = self.search(start, |k| self.sea[k])?;
                    out.extend(path.iter().map(|&k| self.nodes[k]));
                } else {
                    let goal = self.nearest(l, t)?;
                    let path = self.search(start, |k| k == goal)?;
                    out.extend(path.iter().map(|&k| self.nodes[k]));
                }
                out.push(t);
            }
        }
        Some(out)
    }
}

// ── the built ground ────────────────────────────────────────────────────────

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
fn corridor_half_width(seg: &crate::roads::Segment) -> f32 {
    seg.half_width() + if seg.is_street() { crate::zoning::SIDEWALK } else { 1.0 }
}

/// Every grade-separated crossing where neither way is on a deck of its
/// own (a viaduct or a bridge over water) and one runs well above the other.
fn find_underpasses(land: &Landscape, roads: &RoadNetwork, node_z: &[f32]) -> Vec<Underpass> {
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
    half: f32,
    n: usize,
    h: Vec<f32>,
    /// What holds each cell (a road, a cut, a pad), or free ground.
    #[cfg_attr(not(test), allow(dead_code))]
    fix: Vec<Fix>,
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
    outer: Vec<(Vec2, Vec2, f32, f32, f32)>,
    /// `outer` by square of `OUTER_BUCKET` metres (over `FAR_GROUND`): the
    /// ways that may grade the ground in each.
    outer_index: Vec<Vec<u32>>,
}

/// What holds a cell of the built-ground raster.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fix {
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
const OUTER_NEAR: f32 = 250.0;
/// How far from a way's corridor its side slopes are followed beyond the
/// raster (at 1 in `EMBANKMENT_SLOPE` that makes up 120 m).
const OUTER_REACH: f32 = 300.0;
/// Width of the band along the raster's edge that eases into the ground
/// beyond it.
const OUTER_BLEND: f32 = 48.0;
/// The toe and the top of every side slope are rounded over this many
/// metres of height.
const TOE_ROUNDING: f32 = 1.5;

/// Side of the squares `Ground::outer_index` sorts the outer ways into.
const OUTER_BUCKET: f32 = 128.0;
const OUTER_BUCKETS: usize = (2.0 * FAR_GROUND / OUTER_BUCKET) as usize + 1;

fn outer_bucket(p: Vec2) -> Option<usize> {
    let i = ((p.x + FAR_GROUND) / OUTER_BUCKET).floor();
    let j = ((p.y + FAR_GROUND) / OUTER_BUCKET).floor();
    let range = 0.0..OUTER_BUCKETS as f32;
    (range.contains(&i) && range.contains(&j)).then(|| j as usize * OUTER_BUCKETS + i as usize)
}

/// Every outer way, filed under each square its slopes may reach.
fn index_outer(outer: &[(Vec2, Vec2, f32, f32, f32)]) -> Vec<Vec<u32>> {
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

/// Least distance between the segments `p0`-`p1` and `q0`-`q1`.
fn segment_distance(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> f32 {
    if crate::roads::segment_intersection(p0, p1, q0, q1).is_some() {
        return 0.0;
    }
    let d = |p: Vec2, a: Vec2, b: Vec2| project_on_segment(p, a, b).1.distance(p);
    d(p0, q0, q1).min(d(p1, q0, q1)).min(d(q0, p0, p1)).min(d(q1, p0, p1))
}

/// Polynomial smooth maximum: `max(a, b)` with the corner rounded over `k`.
fn smooth_max(a: f32, b: f32, k: f32) -> f32 {
    let h = ((k - (a - b).abs()) / k).max(0.0);
    a.max(b) + h * h * k * 0.25
}

fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    -smooth_max(-a, -b, k)
}

/// The free ground at a point: the natural height, filled up to `lo` (the
/// embankments around it) and cut down to `hi` (the cuttings), the toe of
/// each slope rounded into the terrain. Where two works ask for more than
/// their side slopes allow (`lo > hi`), the ground runs evenly between them.
fn settle(natural: f32, lo: f32, hi: f32) -> f32 {
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
fn grade_free_ground(n: usize, natural: &[f32], fix: &[Fix], set: &[f32], mut lo: Vec<f32>, mut hi: Vec<f32>) -> Vec<f32> {
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
            return Ground { half, n: 0, h: Vec::new(), fix: Vec::new(), pads, decks, underpasses, bridge_decks, outer: Vec::new(), outer_index: Vec::new() };
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
                    if inside {
                        let k = j * n + i;
                        fix[k] = Fix::Pad;
                        set[k] = floor_at(p, pad);
                    }
                }
            }
        }
        let h = grade_free_ground(n, &natural, &fix, &set, lo, hi);
        let outer_index = index_outer(&outer);
        let mut g = Ground { half, n, h, fix, pads, decks, underpasses, bridge_decks, outer, outer_index };
        g.blend_into_outer(land);
        g
    }

    /// The last stretch of the raster eases into the ground beyond it
    /// (`outer_height`), so there is no seam where the two meet.
    fn blend_into_outer(&mut self, land: &Landscape) {
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
    fn outer_height(&self, land: &Landscape, p: Vec2) -> f32 {
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
            near.iter().enumerate().any(|(m, &(a, b, other_core, _, _))| m != k && project_on_segment(q, a, b).1.distance(q) > other_core && segment_distance(p, q, a, b) < other_core)
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

    fn deck_in(&self, decks: &[(Vec2, Vec2, f32, f32, f32)], p: Vec2) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landscapes_are_deterministic_and_presets_differ() {
        for preset in LandscapePreset::ALL {
            let p = LandscapeParams::preset(preset, 42);
            let a = Landscape::new(&p);
            let b = Landscape::new(&p);
            assert_eq!(format!("{:?}", a.rivers.iter().map(|r| r.points.clone()).collect::<Vec<_>>()), format!("{:?}", b.rivers.iter().map(|r| r.points.clone()).collect::<Vec<_>>()), "{preset:?}");
            assert_eq!(a.lakes.len(), b.lakes.len());
            for k in 0..40 {
                let q = Vec2::new(k as f32 * 83.0 - 1600.0, (k * 37 % 29) as f32 * 110.0 - 1500.0);
                assert_eq!(a.height(q), b.height(q), "{preset:?} at {q:?}");
                assert_eq!(a.is_water(q), b.is_water(q));
            }
            // The middle of the map is always dry land.
            for k in 0..36 {
                let q = Vec2::from_angle(k as f32 * 0.1745) * (CLEAR_RADIUS - 10.0);
                assert!(!a.is_water(q), "{preset:?}: water at {q:?} in the clear zone");
                assert!(!a.is_water(q * 0.5));
            }
        }
        let valley = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 1));
        assert!(!valley.rivers.is_empty() && valley.sea.is_none());
        let other = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 2));
        assert_ne!(valley.rivers[0].points[10], other.rivers[0].points[10], "the seed moves the river");
        let isl = Landscape::new(&LandscapeParams::preset(LandscapePreset::Islands, 1));
        assert!(isl.sea.as_ref().is_some_and(|s| s.islands.len() >= 2), "a big island and offshore ones");
        assert!(isl.is_water(Vec2::new(1500.0, 1500.0)));
        let delta = Landscape::new(&LandscapeParams::preset(LandscapePreset::Delta, 3));
        assert!(delta.rivers.len() >= 3, "channels fan out");
        let flat = Landscape::new(&LandscapeParams::flat());
        assert!(flat.is_flat() && flat.height(Vec2::new(100.0, 50.0)) == 0.0);
    }

    #[test]
    fn water_routes_always_end() {
        // Junctions, delta channels and duplicate points make zero-length
        // edges; every route must still come back, and be short.
        for preset in [LandscapePreset::Delta, LandscapePreset::RiverValley, LandscapePreset::Coast, LandscapePreset::Islands, LandscapePreset::Plains] {
            for seed in 1..8 {
                let l = Landscape::new(&LandscapeParams { branching: 0.9, ..LandscapeParams::preset(preset, seed) });
                let probes: Vec<Vec2> = l.rivers.iter().flat_map(|r| r.points.iter().step_by(9).copied()).collect();
                for (k, p) in probes.iter().enumerate() {
                    if let Some(r) = l.water_route(*p, None) {
                        assert!(r.len() < 2000, "{preset:?}/{seed}: {} points", r.len());
                    }
                    if let Some(q) = probes.get(k * 7 % probes.len()) {
                        if let Some(r) = l.water_route(*p, Some(*q)) {
                            assert!(r.len() < 2000);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rivers_carry_a_current_and_route_off_the_map() {
        let l = Landscape::new(&LandscapeParams { branching: 0.8, ..LandscapeParams::preset(LandscapePreset::RiverValley, 5) });
        let main = &l.rivers[0];
        let mid = main.points[main.points.len() / 2];
        let hit = l.water_at(mid).expect("water on the centreline");
        assert_eq!(hit.kind, WaterKind::River);
        assert!(hit.current > 0.3 && hit.flow.length() > 0.9);
        assert!(l.height(mid) < WATER_LEVEL, "the bed is below the water");
        let bank = mid + Vec2::new(main.width, 0.0).rotate(hit.flow.perp());
        assert!(l.height(bank) >= BANK_HEIGHT - 0.5);
        // Streams run faster than the river.
        if let Some(s) = l.rivers.iter().find(|r| r.stream) {
            assert!(s.current > main.current * 0.9);
            assert!(s.width < main.width);
        }
        let route = l.water_route(mid, None).expect("a way off the map");
        let end = *route.last().unwrap();
        assert!(end.x.abs() > LAND_HALF - 60.0 || end.y.abs() > LAND_HALF - 60.0, "ends at the edge: {end:?}");
        // Shore finding from a point on land beside it.
        let shore = l.nearest_shore(bank + hit.flow.perp() * 30.0, 120.0).or_else(|| l.nearest_shore(bank - hit.flow.perp() * 30.0, 120.0));
        assert!(shore.is_some());
    }

    /// The starter avenue crosses the highway, which stands on a high
    /// embankment on the default map: it passes under in a cut, the highway
    /// keeps its height on a deck over it, and the embankment either side
    /// slopes no steeper than about one in two.
    #[test]
    fn streets_pass_under_the_highway_and_embankments_slope() {
        let mut w = crate::world::World::new(crate::world::WorldConfig::default());
        w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 2, 2);
        w.add_road(&[Vec2::new(0.0, -700.0), Vec2::new(0.0, 700.0)], 2, 2);
        let cuts: Vec<&Underpass> = w.ground.underpasses.iter().filter(|u| u.at.x.abs() < 1.0 && (u.at.y - HIGHWAY_Y).abs() < 20.0).collect();
        assert_eq!(cuts.len(), 2, "the avenue passes under both carriageways");
        for u in cuts {
            assert!(u.over_z - u.under_z >= UNDERPASS_CLEARANCE, "{u:?}");
            // In the cut the ground is the avenue's, under the deck too.
            for along in [-20.0f32, -5.0, 0.0, 5.0, 20.0] {
                let p = u.at + u.under_dir * along;
                assert!((w.ground_height(p) - u.under_z_at(along)).abs() < 0.3, "ground at {p} is {}, the avenue {}", w.ground_height(p), u.under_z_at(along));
            }
            // What drives along the highway stays up on its deck.
            assert!((w.surface_height(u.at + u.over_dir * 6.0) - u.over_z_at(6.0)).abs() < 0.3, "{} vs {} {u:?} decks {:?}", w.surface_height(u.at + u.over_dir * 6.0), u.over_z_at(6.0), w.ground.decks_at(u.at + u.over_dir * 6.0));
        }
        // Away from the avenue, across the embankment: no cliff (2.5 across
        // per metre up is 0.4; the raster and the blend allow a little more).
        for x in [-300.0f32, 200.0] {
            let mut worst = 0.0f32;
            let mut prev = w.ground_height(Vec2::new(x, HIGHWAY_Y - 140.0));
            for k in 1..=70 {
                let y = HIGHWAY_Y - 140.0 + k as f32 * 4.0;
                let h = w.ground_height(Vec2::new(x, y));
                // The two carriageways may stand at different heights: the
                // median between them is not an embankment side.
                if (y - HIGHWAY_Y).abs() > HIGHWAY_HALF_GAP + 10.0 {
                    worst = worst.max((h - prev).abs() / 4.0);
                }
                prev = h;
            }
            assert!(worst < 0.55, "embankment at x = {x} as steep as {worst:.2}");
        }
    }

    /// The demo district's roads, zoning, rail branch, highway link and rail
    /// facilities (the layout `nd_app`'s `seed_demo_district` lays out) on
    /// the default map.
    fn demo_ground_world() -> crate::world::World {
        use crate::roads::RoadKind;
        use crate::{Gesture, LandUse};
        let mut w = crate::world::World::new(crate::world::WorldConfig::default());
        w.start_project();
        for i in -2..=2 {
            let c = i as f32 * 140.0;
            w.add_gesture(Gesture::Road { points: vec![Vec2::new(-280.0, c), Vec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
            w.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -280.0), Vec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        }
        let cell = |cx: f32, cy: f32| vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)];
        let uses = [LandUse::Industrial, LandUse::Residential, LandUse::Commercial, LandUse::Park];
        for (k, (x, y)) in [-210.0f32, -70.0, 70.0, 210.0].iter().flat_map(|&x| [-210.0f32, -70.0, 70.0, 210.0].map(move |y| (x, y))).enumerate() {
            w.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: uses[k % uses.len()] });
        }
        w.add_gesture(Gesture::Road {
            points: vec![Vec2::new(-420.0, crate::rail::TRUNK_Y), Vec2::new(-330.0, -410.0), Vec2::new(-280.0, -348.0), Vec2::new(-110.0, -348.0), Vec2::new(-70.0, -324.0), Vec2::new(150.0, -324.0)],
            lanes_fwd: 0,
            lanes_bwd: 0,
            curved: false,
            kind: RoadKind::Rail,
            elevation: Vec::new(),
        });
        w.implement();
        w.economy.treasury = w.economy.treasury.max(150_000.0);
        assert!(w.link_to_highway(Vec2::new(0.0, 280.0)), "the demo's ramps join the highway");
        let _ = w.place_rail_facility(crate::RailFacility::FreightYard, Vec2::new(-200.0, -330.0));
        let _ = w.place_rail_facility(crate::RailFacility::Station, Vec2::new(80.0, -310.0));
        w
    }

    /// The built ground is smooth: between free cells (not a road, a cut or
    /// a pad) it is never steeper than 1 in 1.5, no free cell stands up or
    /// sinks as a spike against all eight neighbours, the ground beside every
    /// road and pad runs down (or up) to it no steeper than that either, and
    /// there is no seam where the raster meets the ground beyond it.
    #[test]
    fn built_ground_has_no_cliffs_spikes_or_seams() {
        let w = demo_ground_world();
        let t0 = std::time::Instant::now();
        let again = Ground::build(&w.landscape, &w.roads, &w.road_z, &w.road_inner, &w.lots, w.config.half_extent + 200.0);
        println!("ground built in {:?}", t0.elapsed());
        assert_eq!(again.h, w.ground.h);
        let g = &w.ground;
        let n = g.n;
        assert!(n > 2);
        let at = |i: usize, j: usize| Vec2::new(-g.half + i as f32 * GROUND_CELL, -g.half + j as f32 * GROUND_CELL);
        let limit = 1.0 / 1.5;
        let (mut worst, mut worst_at) = (0.0f32, Vec2::ZERO);
        let (mut worst_held, mut held_at) = (0.0f32, Vec2::ZERO);
        let mut spikes = Vec::new();
        // Walls stand at underpass cuts, sunken roads and tunnel portals, and
        // under a viaduct the ground is left as it was: none of that is
        // earthwork.
        let skip: Vec<bool> = (0..n * n).map(|k| matches!(g.fix[k], Fix::Cut | Fix::Wall) || !g.decks_at(at(k % n, k / n)).is_empty()).collect();
        for j in 1..n - 1 {
            for i in 1..n - 1 {
                let k = j * n + i;
                let mut walled = skip[k];
                let mut above = 0;
                let mut below = 0;
                for (di, dj) in [(-1isize, -1isize), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                    let m = (j as isize + dj) as usize * n + (i as isize + di) as usize;
                    let run = GROUND_CELL * if di != 0 && dj != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                    let s = (g.h[k] - g.h[m]).abs() / run;
                    if g.h[k] > g.h[m] + 1.0 {
                        above += 1;
                    }
                    if g.h[k] < g.h[m] - 1.0 {
                        below += 1;
                    }
                    walled |= skip[m];
                    if g.fix[k] != Fix::Free || skip[k] || skip[m] {
                        continue;
                    }
                    // The hills are as steep as they were made; only the
                    // works are held to the limit.
                    let (pk, pm) = (at(i, j), at((i as isize + di) as usize, (j as isize + dj) as usize));
                    let natural = (w.landscape.height(pk) - w.landscape.height(pm)).abs() / run;
                    let s = if s <= natural + 0.02 { 0.0 } else { s };
                    if g.fix[m] == Fix::Free {
                        if s > worst {
                            (worst, worst_at) = (s, at(i, j));
                        }
                    } else if s > worst_held {
                        (worst_held, held_at) = (s, at(i, j));
                    }
                }
                if g.fix[k] == Fix::Free && !walled && (above == 8 || below == 8) {
                    spikes.push(at(i, j));
                }
            }
        }
        println!("steepest free ground 1 in {:.2} at {worst_at}; beside a road or pad 1 in {:.2} at {held_at}; {} spikes", 1.0 / worst, 1.0 / worst_held, spikes.len());
        if std::env::var("ND_GROUND_DEBUG").is_ok() {
            for c in [worst_at, held_at] {
                let ci = ((c.x + g.half) / GROUND_CELL).round() as isize;
                let cj = ((c.y + g.half) / GROUND_CELL).round() as isize;
                for dj in -3isize..=3 {
                    let row: Vec<String> = (-3isize..=3).map(|di| {
                        let k = (cj + dj) as usize * n + (ci + di) as usize;
                        format!("{:7.2}{}|{:6.1}", g.h[k], match g.fix[k] { Fix::Free => ' ', Fix::Road => 'R', Fix::Cut => 'C', Fix::Wall => 'W', Fix::Pad => 'P' }, g.outer_height(&w.landscape, at((ci + di) as usize, (cj + dj) as usize)))
                    }).collect();
                    println!("y {:7.1}: {}", at(0, (cj + dj) as usize).y, row.join(" "));
                }
                println!();
                for o in &g.outer {
                    if project_on_segment(c, o.0, o.1).1.distance(c) < 80.0 {
                        println!("outer {:?}", o);
                    }
                }
            }
        }
        assert!(worst <= limit, "free ground as steep as 1 in {:.2} at {worst_at}", 1.0 / worst);
        assert!(worst_held <= limit, "ground beside a road or pad as steep as 1 in {:.2} at {held_at}", 1.0 / worst_held);
        assert!(spikes.is_empty(), "spikes at {spikes:?}");
        // The ways keep their heights through the grading: no embankment as
        // high as the hills at the map edge across the basin (the rail
        // branch behind the plant stood 50 m up against the lots).
        for p in [Vec2::new(-200.0, -348.0), Vec2::new(-200.0, crate::rail::TRUNK_Y), Vec2::new(-120.0, HIGHWAY_Y - HIGHWAY_HALF_GAP)] {
            let rise = w.ground_height(p) - w.landscape.height(p);
            assert!(rise.abs() < 12.0, "the way at {p} stands {rise:.1} m off the land");
        }
        // No seam where the raster meets the ground beyond it, nor at the
        // edge of the drawn map.
        for edge in [g.half, LAND_HALF] {
            for k in 0..60 {
                let along = -1700.0 + k as f32 * 57.0;
                for p in [Vec2::new(edge, along), Vec2::new(-edge, along), Vec2::new(along, edge), Vec2::new(along, -edge)] {
                    let out = p + p.signum() * Vec2::new((p.x.abs() >= edge - 0.1) as u8 as f32, (p.y.abs() >= edge - 0.1) as u8 as f32) * 2.0;
                    let inn = p - (out - p);
                    let (a, b) = (w.ground_height(inn), w.ground_height(out));
                    assert!((a - b).abs() < 4.0 * limit, "a seam at {p}: {a:.2} inside, {b:.2} outside");
                }
            }
        }
    }
}
