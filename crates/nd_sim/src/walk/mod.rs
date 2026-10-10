//! The pedestrian network: pavements on both sides of every street, path
//! centres, crossings, a walkable setback loop around every usable lot, an
//! alley cross inside the Stacks, and gates between lots that share an
//! edge. Walkers route over this with A*, so people cut through blocks and
//! the shops they pass get walk-in trade.
//!
//! Streets are crossed only at crossings (`Crossing`): a crosswalk across
//! every arm of a junction of three or more streets, set back where
//! `nd_procgen::street` paints its zebra (signalised with the junction, see
//! `traffic/crosswalk.rs`); a zebra where a footpath meets a street and
//! every 120 m or so along a long block (`midblock_crossings`); and the end
//! of a dead end, unmarked. Pavements meet round the corners of a junction,
//! never through its middle. Opposite lot fronts are also joined by `Jay`
//! edges, which only jaywalkers' routes may use (`route_with`).

use crate::geom::{self, Grid};
use crate::roads::{NodeId, RoadKind, RoadNetwork, SegmentId};
use crate::zoning::{Lot, LotId, SIDEWALK};
use crate::{LandUse, LandValue, Tier};
use glam::Vec2;
use slotmap::SlotMap;
use std::collections::{BinaryHeap, HashMap};

mod build;
mod footbridge;
mod grounds;
mod route;
mod setbacks;

pub use build::*;
pub use footbridge::*;
pub use grounds::*;
pub use setbacks::*;

/// Inset of the walkable loop from the parcel edge.
const LOOP_INSET: f32 = 1.2;
/// Longest shared boundary needed before two lots get a gate.
const GATE_MIN_SHARED: f32 = 4.0;
/// How far a lot's front node may be from the pavement to connect.
const FRONT_REACH: f32 = 14.0;
/// [pt-walk] How far a landlocked lot with no other way out reaches for
/// the pavement.
const LANDLOCKED_REACH: f32 = 90.0;
/// Cell size of the node index; pavement nodes are a lot's width apart.
const NODE_CELL: f32 = 20.0;
/// Route cost multipliers. Park paths are pleasant, so routes prefer them
/// a little; interior edges (alleys, loops, gates) cost a little more so
/// people do not thread through back yards when the pavement is as short.
const PARK_FACTOR: f32 = 0.92;
const INTERIOR_FACTOR: f32 = 1.15;
/// Middle of a junction's zebras beyond its widest half width: the node
/// radius (widest half width + 1) plus 2.3 m, where `nd_procgen::street`
/// paints them.
pub const CROSSWALK_BACK: f32 = 3.3;
/// A roundabout arm's zebra is further out, past the splitter island.
pub const RING_CROSSWALK_BACK: f32 = 7.5;
/// Half the depth of a zebra along the street.
pub const CROSSWALK_HALF: f32 = 1.2;
/// The longest straight a junction corner's rounded kerb takes along each
/// arm (`nd_procgen::street`): the nose of a sharp fork is rounded tightly
/// rather than over tens of metres.
pub const CORNER_TANGENT_MAX: f32 = 4.0;
/// A run of street between junctions longer than this gets mid-block
/// zebras, about one every `MIDBLOCK_EVERY` metres.
pub const MIDBLOCK_MIN: f32 = 170.0;
pub const MIDBLOCK_EVERY: f32 = 120.0;
/// Jaywalking edges join lot fronts across a street at most this far apart
/// along it, and cost this many metres extra (the look and the dash).
const JAY_REACH: f32 = 12.0;
const JAY_PENALTY: f32 = 4.0;
/// [tools] What crossing at grade under a footbridge costs a walker over
/// its length, in metres: the wait for a gap or the light. Walkers take the
/// bridge when its stairs cost no more than this extra (`route_nodes_with`).
pub const BRIDGED_GRADE_PENALTY: f32 = 30.0;
/// `WalkEdge::cross` of an edge that crosses no street.
pub const NO_CROSSING: u32 = u32::MAX;

/// [park-paths] Gates on the edges of a lot `outline` for paths to be drawn
/// to: one every 40 m or so on each edge of 10 m or more (up to three),
/// `GATE_OUT` outside it, except on the edges in `skip` (shared with more
/// of the same park).
pub fn edge_gates(outline: &[Vec2], skip: &[usize]) -> Vec<Vec2> {
    let n = outline.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    for i in 0..n {
        if skip.contains(&i) {
            continue;
        }
        let (a, b) = (outline[i], outline[(i + 1) % n]);
        let len = a.distance(b);
        if len < 10.0 {
            continue;
        }
        let d = (b - a) / len;
        let k = ((len / 40.0).ceil() as usize).clamp(1, 3);
        for j in 0..k {
            let m = a + d * (len * (j as f32 + 0.5) / k as f32);
            let normal = Vec2::new(d.y, -d.x);
            let q = if geom::point_in_polygon(m + normal * 0.2, outline) { m - normal * GATE_OUT } else { m + normal * GATE_OUT };
            if !geom::point_in_polygon(q, outline) {
                out.push(q);
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct WalkNode {
    pub pos: Vec2,
    /// Lot this node lies inside, for lot-interior edges.
    pub lot: Option<LotId>,
    /// Lots whose frontage this node passes, for footfall.
    pub near: Vec<LotId>,
    /// [tools] Height above the ground, metres: 0 at grade, the walkway's
    /// height on a raised walkway (a footbridge, later a skywalk).
    pub level: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct WalkEdge {
    pub to: u32,
    pub len: f32,
    /// Inside a lot (alley, setback, gate) rather than on a public street.
    pub interior: bool,
    /// A park path: pleasant, so routes prefer it a little.
    pub park: bool,
    /// Index into `WalkGraph::crossings` when the edge crosses a street.
    pub cross: u32,
    /// [tools] Part of a raised walkway (its deck or its stairs): off the
    /// street, so nobody on it holds up traffic. Footbridges are the first;
    /// skywalks between buildings can join them (`WalkGraph::raised`).
    pub raised: bool,
    /// [under-construction] A player path's join onto public grounds
    /// (`add_grounds`): shut, like a park's walkways, while the grounds'
    /// building is still a site (`WalkGraph::closed`).
    pub gate: bool,
}

/// What kind of place a walker crosses a street.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossingKind {
    /// Across an arm of a junction of three or more streets: signalised
    /// when the junction is, a zebra otherwise.
    Junction,
    /// A painted zebra: a footpath over a street, or mid-block.
    Zebra,
    /// Unmarked but fair: the end of a dead end, a roundabout's own lanes.
    Informal,
    /// Straight over between opposite lot fronts: jaywalking.
    Jay,
}

/// One place a walker crosses a street: the edge between nodes `a` and `b`.
#[derive(Clone, Debug)]
pub struct Crossing {
    pub a: u32,
    pub b: u32,
    /// The street crossed.
    pub segment: SegmentId,
    /// The junction it belongs to, for a junction's crosswalk.
    pub node: Option<NodeId>,
    pub kind: CrossingKind,
    /// Its middle, on the street's centre line.
    pub mid: Vec2,
    /// [tools] A push-button signal: drivers get a red only after a walker
    /// presses the button (`traffic::crosswalk`, `world/pedestrians.rs`).
    pub button: bool,
    /// [tools] A footbridge spans it: walkers prefer the bridge.
    pub bridged: bool,
    /// [footbridge] The footbridge over it replaced it: the at-grade edge
    /// is gone from the walk graph, nothing is painted, no walk light, and
    /// drivers do not give way here (someone still out on it, on a route
    /// planned before, is watched like anyone in the road).
    pub closed: bool,
}

impl Crossing {
    /// Somewhere anyone may cross (minding the lights, if any).
    pub fn lawful(&self) -> bool {
        self.kind != CrossingKind::Jay
    }

    /// Painted on the road (a junction's crosswalk or a zebra): drivers give
    /// way to people on it. Elsewhere (a dead end, a roundabout's own lanes,
    /// straight over mid-block) nothing is painted and nobody stops: the
    /// walker finds a gap, at their own risk.
    pub fn marked(&self) -> bool {
        matches!(self.kind, CrossingKind::Junction | CrossingKind::Zebra) && !self.closed
    }
}

#[derive(Clone, Debug, Default)]
pub struct WalkGraph {
    pub nodes: Vec<WalkNode>,
    pub adj: Vec<Vec<WalkEdge>>,
    pub version: u64,
    /// Park benches: node, lot, and the direction a sitter faces.
    pub benches: Vec<(u32, LotId, f32)>,
    /// Every crossing; `WalkEdge::cross` indexes it.
    pub crossings: Vec<Crossing>,
    /// Grid over `nodes`, built at the end of `build`. `nearest` falls back
    /// to a scan if the node list has grown since.
    index: Grid,
    /// Crossing by the exact positions of its two ends, either way round,
    /// so a walker's path (a list of node positions) can find its crossings.
    cross_at: HashMap<[u32; 4], u32>,
    /// Indices into `benches` by lot, built with them, so a park's seats
    /// are counted without a scan of every bench in town.
    bench_lots: HashMap<LotId, Vec<u32>>,
    /// [tools] Every raised walkway edge once (lower node first), for
    /// drawing and for how high a walker on one stands.
    pub raised: Vec<(u32, u32)>,
    /// [tools] The footbridges: one per bridged crossing.
    pub footbridges: Vec<Footbridge>,
    /// [pt-walk] Corner decks joining two footbridges' ends (their tops).
    pub footbridge_links: Vec<[Vec2; 2]>,
    /// [tools] Level of each raised node by its exact position.
    raised_at: HashMap<(u32, u32), f32>,
    /// [park-paths] Where a player path may join the walkways of a park or
    /// other public grounds (`add_grounds`): the Path tool's snap points,
    /// and the ones a path is joined to.
    pub junctions: Vec<GroundsJunction>,
    /// [under-construction] Lots whose building is not open yet (a site,
    /// or coming down), sorted: their park walkways and grounds gates are
    /// shut to anyone not already inside (`route_nodes_with`). Kept by
    /// `World::refresh_closed_grounds`, not by `build`.
    pub closed: Vec<LotId>,
    /// [pt-walk] Connected component of each node over every edge (jaywalks
    /// and gates included), from `compute_components` once the graph is
    /// complete (`World::build_walk_graph`); empty until then. A route
    /// between two components is refused without a search.
    pub comp: Vec<u32>,
}

#[cfg(test)]
mod tests;
