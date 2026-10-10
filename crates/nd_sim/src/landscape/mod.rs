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

use crate::geom::{distance_to_outline, offset_polyline, point_in_polygon, project_on_segment, smoothstep};
use crate::roads::RoadNetwork;
use crate::zoning::{Lot, LotId};
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use std::sync::OnceLock;

mod built_ground;
mod features;
mod generate;
mod noise;
mod params;
mod raster;
mod routing;

pub use built_ground::*;
pub use features::*;
pub use noise::*;
pub use params::*;
use raster::*;
pub use raster::LotGround;
pub use routing::*;

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
/// Steepest bank a dock or port may stand on (rise over run across its
/// footprint). Lots go steeper: see [`MAX_GRADED_SLOPE`].
pub const MAX_LOT_SLOPE: f32 = 0.2;
/// [island-lots] Steepest ground a lot is still built on (rise over run
/// between any two points of it): its pad is levelled to its street and
/// the ground round it cut and filled (`Ground`), terraced down a hillside
/// lot by lot. Anything steeper is a cliff.
pub const MAX_GRADED_SLOPE: f32 = 0.7;
/// [island-lots] Most the natural ground may rise or fall across one lot
/// and still be levelled into a terrace: on a steep hillside only shallow
/// lots fit, on gentle ground deep yards do.
pub const MAX_LOT_RELIEF: f32 = 18.0;
/// [hillside] Steeper than [`MAX_GRADED_SLOPE`] is a hillside, not yet a
/// cliff: homes, shops and clubs are built there on terraces (a cliffside
/// villa, a Hong Kong stair street), not works yards or civic grounds. Past
/// this it is a sheer cliff and nothing is built.
pub const MAX_TERRACE_SLOPE: f32 = 1.8;
/// [hillside] Most rise across a terraced hillside lot.
pub const MAX_TERRACE_RELIEF: f32 = 40.0;
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
    /// [review] The water graph and the sea-way grid, kept between
    /// routing calls (they were rebuilt on every one).
    #[serde(skip)]
    water_cache: WaterCache,
}

impl Default for Landscape {
    /// The flat map an old save had: no water, hills only outside the basin.
    fn default() -> Self {
        Self { params: LandscapeParams::flat(), rivers: Vec::new(), lakes: Vec::new(), sea: None, hot_spots: Vec::new(), raster: OnceLock::new(), water_cache: WaterCache::default() }
    }
}

impl std::fmt::Debug for Landscape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Landscape({:?}, {} rivers, {} lakes, sea: {}, {} hot spots)", self.params.preset, self.rivers.len(), self.lakes.len(), self.sea.is_some(), self.hot_spots.len())
    }
}

#[cfg(test)]
mod tests;
