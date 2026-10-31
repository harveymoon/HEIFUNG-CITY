//! Road network: a node/segment graph with snapping, intersection splitting
//! and A* pathfinding.

use crate::geom::Grid;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

pub use crate::geom::{project_on_segment, segment_intersection, segment_intersection_closed};

mod build;
mod grade;
mod path;
mod ramps;
mod ring;

pub use grade::*;
pub use ramps::*;
pub use ring::*;

/// Cell size of the segment index. Road pieces are 7 m (curves) to a few
/// hundred metres (straight roads); a nearest query reads a 3x3 block of
/// cells in the common case.
const SEGMENT_CELL: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SegmentId(pub u32);

/// Nodes closer than this are merged when roads are drawn.
pub const SNAP_RADIUS: f32 = 5.0;
/// [interchange] A vertex this close to a node was put on it (snapped by
/// the tool), not merely drawn near it.
pub const SNAP_EXACT: f32 = 1.0;
pub const LANE_WIDTH: f32 = 3.5;
/// Extra asphalt beyond the lanes (sidewalk / shoulder).
pub const SHOULDER: f32 = 1.5;
/// How far a road end may be dropped from an existing road and still join it.
pub const END_SNAP_RADIUS: f32 = 9.0;
/// [r4-flags-look] A way's end never snaps back onto its own pieces within
/// this distance along it (a curve's last samples lie closer together than
/// the snap radius); further back, it joins itself like any other road.
pub const OWN_END_REACH: f32 = 4.0 * END_SNAP_RADIUS;
/// [r4-crossings] The shortest piece a street is cut into where a path
/// ends on it or a way passes under it: closer than this to one of the
/// street's nodes, the path joins that node and the way below gets no
/// vertex over it. A piece shorter than a junction's radius (the widest
/// half width plus a metre) leaves its lanes no length: Harvey's ring had a
/// 1.9 m piece beside a junction, where an underpass passed under it.
pub const MIN_PIECE: f32 = 6.0;
/// [r4-flags-sim] Street nodes closer than this at about one level are one
/// junction (`RoadNetwork::weld_close_street_nodes`).
pub const WELD_RADIUS: f32 = 3.0;
/// [r4-crossings] A path ending on a street this close (along it) to where
/// another path already meets that street joins it there: two paths drawn
/// to either pavement a few steps apart meet the street at one point and
/// share one crossing, instead of cutting it twice.
pub const PATH_MEETS: f32 = 8.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub pos: Vec2,
    pub segments: Vec<SegmentId>,
    /// Height above the natural ground the player drew the way at here, in
    /// metres: 0 at grade, positive on a viaduct, negative in a cutting.
    /// Old saves load at grade.
    #[serde(default)]
    pub level: f32,
}

/// What a road carries. Streets have car lanes and pavements; paths are for
/// people on foot and on bikes only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoadKind {
    #[default]
    Street,
    Path,
    /// A railway: trains only, single track.
    Rail,
}

/// Half the corridor of a single track: ballast shoulder to ballast shoulder.
pub const RAIL_HALF_WIDTH: f32 = 2.4;
/// Half the width of a path, in metres.
pub const PATH_HALF_WIDTH: f32 = 1.6;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub a: NodeId,
    pub b: NodeId,
    /// Lanes running a -> b.
    pub lanes_fwd: u8,
    /// Lanes running b -> a.
    pub lanes_bwd: u8,
    pub alive: bool,
    #[serde(default)]
    pub kind: RoadKind,
    /// Carried over water on a deck (set by the world from the landscape
    /// when the network is rebuilt).
    #[serde(default)]
    pub bridge: bool,
    /// Drawn above the ground (`VIADUCT_LEVEL` or more at an end): a deck
    /// on pillars rather than an embankment. Viaducts are `bridge`s too.
    #[serde(default)]
    pub viaduct: bool,
    /// Drawn deep below the ground at both ends (`TUNNEL_LEVEL` or lower):
    /// a bored tunnel. It cuts no parcels, fronts no lots, has no
    /// pavements, and leaves whatever stands above it alone.
    #[serde(default)]
    pub tunnel: bool,
    /// A piece of a roundabout's ring (`RoadNetwork::rings`). A ring is a
    /// one-way street of up to three lanes, so without this a three-lane
    /// ring would pass for a highway carriageway (grade-separated, fast,
    /// fronting nothing). Set as the ring is laid and on load
    /// (`RoadNetwork::classify`).
    #[serde(default)]
    pub ring: bool,
    /// A highway slip road (`RoadNetwork::classify`): a one-way street
    /// joined to a highway carriageway, up to where it meets a two-way
    /// street. Like the highway it fronts nothing (no parcels, no kerb
    /// parking), has hard shoulders instead of pavements and nobody walks
    /// on it; its lanes ease their speed from the highway's to the
    /// street's along its length (`traffic::LaneNetwork::ramp_profiles`).
    #[serde(default)]
    pub ramp: bool,
    /// [w1-roads] The road's use as the player set it: `Some(true)` cars
    /// only, `Some(false)` mixed (a street with pavements, whatever its
    /// lanes), `None` a road from before it was set, its use read from its
    /// lanes as it always was (`RoadUse::of`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_only: Option<bool>,
}

impl Segment {
    pub fn lanes(&self) -> u8 {
        self.lanes_fwd + self.lanes_bwd
    }

    pub fn is_street(&self) -> bool {
        self.kind == RoadKind::Street
    }

    pub fn is_rail(&self) -> bool {
        self.kind == RoadKind::Rail
    }

    /// A one-way street of three or more lanes: a highway carriageway. It is
    /// fast, fronts no lots, and is grade-separated: other ways cross it
    /// without a junction unless an end is dropped on it (a ramp).
    pub fn is_highway(&self) -> bool {
        !self.ring && is_highway_use(self.kind, self.lanes_fwd, self.lanes_bwd, self.car_only)
    }

    /// [w1-roads] What it carries (`RoadUse::of`).
    pub fn road_use(&self) -> RoadUse {
        if self.ring {
            return RoadUse::Mixed;
        }
        RoadUse::of(self.kind, self.lanes_fwd, self.lanes_bwd, self.car_only)
    }

    /// A highway carriageway or one of its slip roads: fast road that
    /// fronts nothing and has no pavements.
    pub fn is_motorway(&self) -> bool {
        // [w1-roads] A road set to cars only fronts nothing whatever its
        // lanes; one set to Mixed is a street even joined to a highway.
        match self.car_only {
            Some(car_only) => car_only && self.kind == RoadKind::Street && !self.ring,
            None => self.ramp || self.is_highway(),
        }
    }

    /// [interchange] Its part in grade separation (`separated`).
    pub fn class(&self) -> WayClass {
        if self.is_highway() {
            WayClass::Highway
        } else if self.ramp {
            WayClass::Ramp
        } else if self.kind == RoadKind::Path {
            WayClass::Foot
        } else {
            WayClass::Street
        }
    }

    /// A way for traffic only: the highway, its slip roads and tunnels.
    /// None of them fronts parcels, takes kerb parking (legal or not),
    /// carries walkers or crossings, or gets RVs, camps or litter beside
    /// it. A tunnel is drawn on its own profile underground, so a car
    /// "parked" at its kerb used to stand on the ground above the bore.
    /// [r4] (K3) A bus stop may stand at its kerb: any street but a bored
    /// tunnel or a highway carriageway (slip roads have a shoulder to pull
    /// onto; one-way streets a kerb on either side).
    pub fn stop_allowed(&self) -> bool {
        self.is_street() && !self.tunnel && !self.is_highway()
    }

    pub fn fronts_nothing(&self) -> bool {
        self.is_motorway() || self.tunnel
    }

    /// [walk-networks] People may walk (and ride) along it: a footpath at
    /// any depth, or a street with pavements. A tunnel fronts nothing, but
    /// a footpath bored underground is a pedestrian tunnel, and a street
    /// tunnel the player set to Mixed has walkways along its bore. Never
    /// the highway, its slip roads, a car-only road or the railway.
    pub fn walkable(&self) -> bool {
        match self.kind {
            RoadKind::Rail => false,
            RoadKind::Path => true,
            RoadKind::Street => !self.is_motorway() && (!self.tunnel || self.car_only == Some(false)),
        }
    }

    pub fn half_width(&self) -> f32 {
        half_width_spec(self.kind, self.lanes_fwd, self.lanes_bwd)
    }

    /// Half the corridor the way takes from the land: the carriageway and
    /// its pavement (streets), or a metre of verge. Where parcels start and
    /// how wide the built ground is graded.
    pub fn corridor_half_width(&self) -> f32 {
        self.half_width() + if self.is_street() { crate::zoning::SIDEWALK } else { 1.0 }
    }
}

/// Half the width of a way of `kind` with these lanes, as built
/// ([`Segment::half_width`]); also a ring's and a planned road's.
pub fn half_width_spec(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> f32 {
    match kind {
        RoadKind::Street => (lanes_fwd + lanes_bwd) as f32 * LANE_WIDTH * 0.5 + SHOULDER,
        RoadKind::Path => PATH_HALF_WIDTH,
        RoadKind::Rail => RAIL_HALF_WIDTH,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoadNetwork {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
    /// Incremented on every change so renderers can cheaply detect updates.
    pub version: u64,
    /// Grid over the alive segments, built on the first nearest query after
    /// a change and dropped by the next change. Every mutation goes through
    /// `link` or `split_segment`, which reset it.
    #[serde(skip)]
    index: OnceLock<Grid>,
    /// Roundabouts built into the network (set by the world's rebuild):
    /// their ring pieces are one-way streets on the circle.
    #[serde(default)]
    pub rings: Vec<Ring>,
    /// Set while a ring's own polyline is being laid, so its pieces are
    /// flagged `ring` and never treated as a highway while they are cut in.
    #[serde(skip)]
    laying_ring: bool,
    /// [w1-roads] The use of the way being laid (`add_polyline_use`).
    #[serde(skip)]
    laying_use: Option<bool>,
}

impl RoadNetwork {
    /// The segment index, built if a change dropped it. Items are segment
    /// indices; dead segments get an empty box and are never returned.
    fn grid(&self) -> &Grid {
        self.index.get_or_init(|| {
            Grid::new(
                SEGMENT_CELL,
                self.segments.iter().map(|s| {
                    if s.alive {
                        let (a, b) = (self.node(s.a).pos, self.node(s.b).pos);
                        (a.min(b), a.max(b))
                    } else {
                        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN))
                    }
                }),
            )
        })
    }

    /// Closest point on any alive segment that passes `keep`. Exact ties go
    /// to the lower segment id, as the linear scan this replaces did.
    pub fn nearest_where(&self, p: Vec2, keep: impl Fn(&Segment) -> bool) -> Option<(SegmentId, f32, Vec2)> {
        let (i, _) = self.grid().nearest(p, |i| {
            let seg = &self.segments[i as usize];
            if !seg.alive || !keep(seg) {
                return f32::INFINITY;
            }
            let (a, b) = (self.node(seg.a).pos, self.node(seg.b).pos);
            p.distance(project_on_segment(p, a, b).1)
        })?;
        let sid = SegmentId(i);
        let (a, b) = self.segment_points(sid);
        let (t, q) = project_on_segment(p, a, b);
        Some((sid, t, q))
    }
    /// [footbridge] Live segments whose bounding box comes within `r` of
    /// `p` (a box query on the segment grid: never searches further out).
    pub fn segments_near(&self, p: Vec2, r: f32) -> Vec<SegmentId> {
        self.grid().in_box(p - Vec2::splat(r), p + Vec2::splat(r)).into_iter().filter(|&i| self.segments[i as usize].alive).map(SegmentId).collect()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn segment(&self, id: SegmentId) -> &Segment {
        &self.segments[id.0 as usize]
    }

    /// A segment by id, or None when the id is from another network (ids
    /// are renumbered by every rebuild) or the segment is gone.
    pub fn try_segment(&self, id: SegmentId) -> Option<&Segment> {
        self.segments.get(id.0 as usize).filter(|s| s.alive)
    }

    pub fn segment_points(&self, id: SegmentId) -> (Vec2, Vec2) {
        let s = self.segment(id);
        (self.node(s.a).pos, self.node(s.b).pos)
    }

    pub fn alive_segments(&self) -> impl Iterator<Item = (SegmentId, &Segment)> {
        self.segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.alive)
            .map(|(i, s)| (SegmentId(i as u32), s))
    }

    pub fn is_empty(&self) -> bool {
        !self.segments.iter().any(|s| s.alive)
    }

    pub fn node_near(&self, p: Vec2, radius: f32) -> Option<NodeId> {
        let mut best: Option<(f32, NodeId)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let d = n.pos.distance(p);
            if d <= radius && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, NodeId(i as u32)));
            }
        }
        best.map(|(_, id)| id)
    }

    /// [tools] A raised piece high enough at both ends to span over the
    /// lots under it (`pillars::SPAN_LEVEL`): it takes no corridor from
    /// the parcels; its columns stand where there is room
    /// (`crate::pillars`).
    pub fn spans_lots(&self, sid: SegmentId) -> bool {
        let s = self.segment(sid);
        !s.tunnel && self.node(s.a).level >= crate::pillars::SPAN_LEVEL && self.node(s.b).level >= crate::pillars::SPAN_LEVEL
    }

    /// Level of a segment `t` of the way from its `a` end.
    pub fn segment_level(&self, sid: SegmentId, t: f32) -> f32 {
        let s = self.segment(sid);
        let (la, lb) = (self.node(s.a).level, self.node(s.b).level);
        la + (lb - la) * t.clamp(0.0, 1.0)
    }

    /// Closest point on any alive way that `kind` can join.
    pub fn nearest_of_kind(&self, p: Vec2, kind: RoadKind) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_where(p, |seg| Self::joins(seg.kind, kind))
    }

    /// Closest point on the railway.
    pub fn nearest_rail(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_of_kind(p, RoadKind::Rail)
    }

    /// Number of alive streets at a node (paths do not count).
    pub fn street_degree(&self, n: NodeId) -> usize {
        self.streets_at(n).count()
    }

    /// The alive streets at a node (paths and track left out), in the
    /// node's own order.
    pub fn streets_at(&self, n: NodeId) -> impl Iterator<Item = SegmentId> + '_ {
        self.node(n).segments.iter().copied().filter(|&s| self.segment(s).alive && self.segment(s).is_street())
    }

    /// The unit direction of segment `sid` away from `node`: from its `a`
    /// end when `node` is `a`, else from its `b` end.
    pub fn away_dir(&self, sid: SegmentId, node: NodeId) -> Vec2 {
        let (a, b) = self.segment_points(sid);
        (if self.segment(sid).a == node { b - a } else { a - b }).normalize_or_zero()
    }

    /// Closest point on any alive street (paths excluded): where a car can be.
    pub fn nearest_street(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_where(p, Segment::is_street)
    }

    /// Closest point on any alive street or path (never the railway).
    pub fn nearest_on_network(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_where(p, |seg| !seg.is_rail())
    }

    /// Alive segments whose bounding box touches `(lo, hi)`, ascending.
    pub fn segments_in_box(&self, lo: Vec2, hi: Vec2) -> impl Iterator<Item = SegmentId> + '_ {
        self.grid().in_box(lo, hi).into_iter().map(SegmentId).filter(move |&s| self.segments[s.0 as usize].alive)
    }

    /// Endpoint of `sid` closest to parameter `t`.
    pub fn nearest_endpoint(&self, sid: SegmentId, t: f32) -> NodeId {
        let s = self.segment(sid);
        if t < 0.5 {
            s.a
        } else {
            s.b
        }
    }
}

#[cfg(test)]
mod tests;
