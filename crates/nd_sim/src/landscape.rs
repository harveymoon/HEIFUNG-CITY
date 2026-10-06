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
        Self { preset, seed, water, relief, branching, islands, highway: preset != LandscapePreset::Flat }
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
            SeaExit::South => Vec2::new(p.x, -LAND_HALF - 80.0),
            SeaExit::Radial => p.normalize_or(Vec2::NEG_Y) * (LAND_HALF * 1.45),
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
            return self.base_height(p);
        }
        self.sample(|r| &r.h, p)
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

/// The ground as built: natural terrain, cut and filled to every road's
/// profile and levelled to a pad under every lot. The renderer draws the
/// terrain from it and sets everything that stands on the ground on it.
/// Derived (rebuilt with the roads), never saved.
#[derive(Clone, Default)]
pub struct Ground {
    half: f32,
    n: usize,
    h: Vec<f32>,
    /// Lot pads, by lot.
    pub pads: std::collections::HashMap<LotId, f32>,
    /// Bridge decks: ends, half width (with the pavement), end heights.
    pub decks: Vec<(Vec2, Vec2, f32, f32, f32)>,
}

impl std::fmt::Debug for Ground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ground({}x{})", self.n, self.n)
    }
}

impl Ground {
    /// Build over the square of half-side `half`.
    pub fn build(land: &Landscape, roads: &RoadNetwork, node_z: &[f32], lots: &SlotMap<LotId, Lot>, half: f32) -> Ground {
        let mut pads = std::collections::HashMap::new();
        for (id, lot) in lots.iter() {
            pads.insert(id, lot_pad(roads, node_z, lot, land));
        }
        let decks: Vec<(Vec2, Vec2, f32, f32, f32)> = roads
            .alive_segments()
            .filter(|(_, s)| s.bridge)
            .map(|(id, s)| {
                let (a, b) = roads.segment_points(id);
                let hw = s.half_width() + if s.is_street() { crate::zoning::SIDEWALK } else { 0.5 };
                (a, b, hw, node_z.get(s.a.0 as usize).copied().unwrap_or(0.0), node_z.get(s.b.0 as usize).copied().unwrap_or(0.0))
            })
            .collect();
        if land.is_flat() {
            return Ground { half, n: 0, h: Vec::new(), pads, decks };
        }
        let n = (half * 2.0 / GROUND_CELL) as usize + 1;
        let at = |i: usize, j: usize| Vec2::new(-half + i as f32 * GROUND_CELL, -half + j as f32 * GROUND_CELL);
        let mut h: Vec<f32> = (0..n * n).map(|k| land.height(at(k % n, k / n))).collect();
        let mut w = vec![0.0f32; n * n];
        let mut target = vec![0.0f32; n * n];
        let range = |lo: f32, hi: f32| -> (usize, usize) {
            let a = (((lo + half) / GROUND_CELL).floor().max(0.0) as usize).min(n - 1);
            let b = (((hi + half) / GROUND_CELL).ceil().max(0.0) as usize).min(n - 1);
            (a, b)
        };
        for (_, seg) in roads.alive_segments() {
            if seg.bridge {
                continue;
            }
            let (a, b) = (roads.node(seg.a).pos, roads.node(seg.b).pos);
            let (za, zb) = (node_z.get(seg.a.0 as usize).copied().unwrap_or(0.0), node_z.get(seg.b.0 as usize).copied().unwrap_or(0.0));
            let core = seg.half_width() + if seg.is_street() { crate::zoning::SIDEWALK } else { 1.0 };
            let reach = core + GROUND_FALLOFF;
            let (i0, i1) = range(a.x.min(b.x) - reach, a.x.max(b.x) + reach);
            let (j0, j1) = range(a.y.min(b.y) - reach, a.y.max(b.y) + reach);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = at(i, j);
                    let (t, q) = project_on_segment(p, a, b);
                    let d = q.distance(p);
                    if d > reach {
                        continue;
                    }
                    let wt = if d <= core { 1.0 } else { 1.0 - smoothstep(core, reach, d) };
                    let k = j * n + i;
                    if wt > w[k] + 1e-4 || (wt >= 1.0 && w[k] >= 1.0 && za + (zb - za) * t > target[k]) {
                        w[k] = wt;
                        target[k] = za + (zb - za) * t;
                    }
                }
            }
        }
        for (id, lot) in lots.iter() {
            let pad = pads[&id];
            let (lo, hi) = crate::geom::bounds(&lot.polygon);
            let (i0, i1) = range(lo.x - 2.0, hi.x + 2.0);
            let (j0, j1) = range(lo.y - 2.0, hi.y + 2.0);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = at(i, j);
                    let inside = point_in_polygon(p, &lot.polygon) || distance_to_outline(p, &lot.polygon) < 2.0;
                    if inside {
                        let k = j * n + i;
                        w[k] = 1.0;
                        target[k] = pad;
                    }
                }
            }
        }
        for k in 0..n * n {
            if w[k] > 0.0 {
                // Never carve the built ground below the waterline.
                let t = target[k].max(WATER_LEVEL + 0.4);
                h[k] = h[k] * (1.0 - w[k]) + t * w[k];
            }
        }
        Ground { half, n, h, pads, decks }
    }

    /// Height of a bridge deck at `p`, if `p` is on one.
    pub fn deck_at(&self, p: Vec2) -> Option<f32> {
        for &(a, b, hw, za, zb) in &self.decks {
            if p.x < a.x.min(b.x) - hw || p.x > a.x.max(b.x) + hw || p.y < a.y.min(b.y) - hw || p.y > a.y.max(b.y) + hw {
                continue;
            }
            let (t, q) = project_on_segment(p, a, b);
            if q.distance(p) <= hw {
                return Some(za + (zb - za) * t);
            }
        }
        None
    }

    /// Height of the built ground (natural terrain outside the raster).
    pub fn height(&self, land: &Landscape, p: Vec2) -> f32 {
        if self.n < 2 || p.x.abs() >= self.half || p.y.abs() >= self.half {
            return if land.is_flat() { land.base_height(p) } else { land.height(p) };
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

/// The level a lot is graded to: its street's height at the frontage.
pub fn lot_pad(roads: &RoadNetwork, node_z: &[f32], lot: &Lot, land: &Landscape) -> f32 {
    if land.is_flat() {
        return 0.0;
    }
    match roads.try_segment(lot.front_segment) {
        Some(seg) if !seg.bridge => {
            let za = node_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
            let zb = node_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
            za + (zb - za) * lot.front_t.clamp(0.0, 1.0)
        }
        _ => land.height(lot.center).max(BANK_HEIGHT),
    }
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
}
