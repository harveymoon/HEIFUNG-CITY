//! Streets: carriageway, kerbs and pavements, markings and crossings,
//! junction pads, lamps, signal posts, bins, benches, shelters, bollards,
//! utility poles with sagging wires, and trees where the land is worth it.
//!
//! Roads are drawn as *runs*: chains of segments joined at plain bends
//! (nodes with exactly two roads of the same kind). A run gets one
//! continuous, mitred carriageway and pavement, so a curved road reads as
//! one road instead of a string of pieces.

use crate::buildings::parts::{bench_world, bin_world, bollard_world, lamp_post_world, mul, pick, railway_track, rgba, CHARCOAL, DARK};
use crate::mesh::{offset_path, path_length, point_along, Polyline};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::land_value::LandValue;
use nd_sim::rng_for;
use nd_sim::roads::{RoadKind, RoadNetwork, SegmentId, LANE_WIDTH, PATH_HALF_WIDTH, RAIL_HALF_WIDTH, SHOULDER};
use nd_sim::traffic::{LaneId, LaneKind, LaneNetwork};
use nd_sim::zoning::SIDEWALK;
use nd_sim::NodeId;
use rand::Rng;
use std::collections::{BTreeMap, HashMap, HashSet};

mod arrows;
mod draw;
mod gore;
mod junction;
mod kerb;
mod profile;
mod rail_path;
mod runs;
mod signals;
mod tunnel;
#[cfg(test)]
mod arrow_tests;
#[cfg(test)]
mod kerb_tests;
#[cfg(test)]
mod sweep_visual_tests;
#[cfg(test)]
mod tests;

pub use arrows::*;
pub use draw::generate_street;
use gore::*;
pub(crate) use kerb::*;
pub use profile::*;
use rail_path::*;
use runs::*;
pub use signals::*;
pub use tunnel::*;
use crate::depth::{ASPHALT_Y, KERB_H, MARK_Y};
use junction::{cul_de_sac, gore_ramp, mouths, pad_node, Mouth};

pub struct StreetInput<'a> {
    pub roads: &'a RoadNetwork,
    pub net: &'a LaneNetwork,
    pub land_value: &'a LandValue,
    pub palette: &'a Palette,
    pub seed: u64,
    /// Height of every road node (`World::road_z`); empty for a flat city.
    /// Each run is set on its own profile, so a viaduct is drawn at its
    /// height and the street under it at the ground's, whatever overlaps.
    pub heights: &'a [f32],
    /// Heights inside long streets between their nodes, per segment
    /// (`World::road_inner`); empty for straight lines between the nodes.
    pub inner: &'a [Vec<(f32, f32)>],
    /// Which roads to draw: all, those on and above the ground, or only the
    /// tunnels (shown on their own in the underground view).
    pub layer: StreetLayer,
    /// [footbridge] Middles of the crosswalks a footbridge replaced
    /// (`World::closed_crosswalks`): no zebra, beacons or walk lights there.
    pub closed: &'a [Vec2],
}

/// [footbridge] Is the crosswalk with its middle at `mid` one a footbridge
/// replaced?
pub fn crosswalk_closed(closed: &[Vec2], mid: Vec2) -> bool {
    closed.iter().any(|c| c.distance(mid) < 2.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StreetLayer {
    #[default]
    All,
    Surface,
    Underground,
}

impl StreetLayer {
    fn keeps(self, tunnel: bool) -> bool {
        match self {
            StreetLayer::All => true,
            StreetLayer::Surface => !tunnel,
            StreetLayer::Underground => tunnel,
        }
    }
}

/// Does `p` stand on a street's lanes at about height `z`? For the walk
/// view's clutter (`walk.rs`), as the street's own furniture.
pub(crate) fn on_carriageway(roads: &RoadNetwork, heights: &[f32], inner: &[Vec<(f32, f32)>], p: Vec2, z: f32) -> bool {
    junction::on_carriageway(roads, heights, inner, p, z, 0.05)
}

/// Vertex counts of every part: where the next piece of drawing starts.
pub(crate) fn marks(g: &Geometry) -> Vec<usize> {
    g.parts.iter().map(|m| m.positions.len()).collect()
}

/// Raise every vertex drawn since `from` by `h` of its (x, z).
pub(crate) fn lift_since(g: &mut Geometry, from: &[usize], h: impl Fn(Vec2) -> f32) {
    for (k, m) in g.parts.iter_mut().enumerate() {
        let start = from.get(k).copied().unwrap_or(0);
        for p in m.positions.iter_mut().skip(start) {
            p[1] += h(Vec2::new(p[0], p[2]));
        }
    }
}
