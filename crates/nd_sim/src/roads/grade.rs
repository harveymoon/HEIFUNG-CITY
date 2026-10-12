//! Grade separation: which ways cross without meeting (a highway over a
//! street, a viaduct, a tunnel), the classes that decide it, and the
//! crossings where they do not.

use super::*;

/// Ways whose levels at a crossing differ by this much or more pass one
/// over the other without a junction (headroom and deck).
pub const GRADE_SEPARATION: f32 = 4.5;

/// A way at least this far above the ground stands on a deck (a viaduct)
/// rather than on an embankment.
pub const VIADUCT_LEVEL: f32 = 2.5;

/// A way at least this far below the ground at both ends of a piece runs in
/// a covered tunnel; above it, in an open cutting (a portal ramp).
pub const TUNNEL_LEVEL: f32 = -5.0;

/// The level the Road tool's Tunnel preset draws at.
pub const TUNNEL_DEPTH: f32 = -8.0;

/// Do two ways crossing here pass one over the other? Highways only ever
/// meet other roads by a dropped end (a ramp); any two ways whose planned
/// levels differ by `GRADE_SEPARATION` or more do not meet at all.
pub fn grade_separated(highway_a: bool, level_a: f32, highway_b: bool, level_b: f32) -> bool {
    highway_a != highway_b || (level_a - level_b).abs() >= GRADE_SEPARATION
}

/// [interchange] The part a way plays where it crosses another: what it
/// may meet at a junction without an end dropped on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WayClass {
    /// A highway carriageway: meets only other carriageways.
    Highway,
    /// A slip road (`Segment::ramp`): meets the highway only by its ends
    /// and other slip roads (forks and merges), never a street or a path
    /// that merely crosses it. A street drawn under a highway with ramps
    /// used to cut every ramp it crossed into a junction, which ended the
    /// ramp there (`classify`) and turned the rest of it into a town street.
    Ramp,
    /// A one-way street being laid: it may become a slip road or stay a
    /// town street, so it meets both.
    OneWay,
    /// Every other street (two-way streets, town one-way streets, rings)
    /// and the railway: crosses a slip road on a bridge.
    Street,
    /// A path: crosses a slip road at grade, as people cross one at a
    /// ramp's end (a ramp lifted over every footpath near its foot would
    /// have to climb 4.5 m in a few metres).
    Foot,
}

impl WayClass {
    /// The class of a way of this kind and these lanes as it is being laid.
    pub fn laying(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8, ring: bool) -> Self {
        if kind == RoadKind::Path {
            WayClass::Foot
        } else if ring {
            WayClass::Street
        } else if is_highway_spec(kind, lanes_fwd, lanes_bwd) {
            WayClass::Highway
        } else if is_ramp_spec(kind, lanes_fwd, lanes_bwd) {
            WayClass::OneWay
        } else {
            WayClass::Street
        }
    }

    /// [w1-roads] [`WayClass::laying`] with the road's use: a street set to
    /// Mixed is a street whatever its lanes; one set to cars only is a
    /// slip road in the making when too narrow for a highway.
    pub fn laying_use(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8, ring: bool, car_only: Option<bool>) -> Self {
        match car_only {
            _ if kind != RoadKind::Street || ring => Self::laying(kind, lanes_fwd, lanes_bwd, ring),
            Some(false) => WayClass::Street,
            Some(true) if is_highway_spec(kind, lanes_fwd, lanes_bwd) => WayClass::Highway,
            Some(true) if (lanes_fwd == 0) != (lanes_bwd == 0) => WayClass::OneWay,
            _ => Self::laying(kind, lanes_fwd, lanes_bwd, ring),
        }
    }

    pub(super) fn rank(self) -> u8 {
        match self {
            WayClass::Highway => 2,
            WayClass::Ramp | WayClass::OneWay => 1,
            WayClass::Street | WayClass::Foot => 0,
        }
    }

    /// The class a vertex placed right on a node or a way (snapped there by
    /// the tool) meets it as: deliberately, so a street's control point on
    /// a ramp's end makes the ramp's terminal junction there.
    pub(super) fn snapped(self) -> Self {
        match self {
            WayClass::Street => WayClass::OneWay,
            c => c,
        }
    }
}

/// [interchange] [`grade_separated`] with slip roads: two ways crossing pass
/// one over the other unless they may meet at a junction. Levels 4.5 m or
/// more apart never meet; a highway meets only highways; a slip road meets
/// slip roads, one-way streets being laid (a ramp fork) and paths, never a
/// street or the railway that crosses it; everything else meets at grade.
pub fn separated(a: WayClass, level_a: f32, b: WayClass, level_b: f32) -> bool {
    use WayClass::*;
    if (level_a - level_b).abs() >= GRADE_SEPARATION {
        return true;
    }
    match (a, b) {
        (Highway, Highway) => false,
        (Highway, _) | (_, Highway) => true,
        (Ramp, Street) | (Street, Ramp) => true,
        _ => false,
    }
}

/// [interchange] Of two separated ways, does the first pass over? The one
/// drawn higher does; at the same level the highway goes over the street,
/// with slip roads: the way drawn higher goes
/// over; at about the same level the highway goes over a slip road and
/// either over a street or a path.
pub fn passes_over_class(a: WayClass, level_a: f32, b: WayClass, level_b: f32) -> bool {
    if (level_a - level_b).abs() >= 1.0 {
        level_a > level_b
    } else {
        a.rank() > b.rank()
    }
}

/// Where two ways cross without a junction: the upper one passes over the
/// lower. `upper` and `lower` are the segments with the parameter of the
/// crossing along each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossing {
    pub at: Vec2,
    pub upper: (SegmentId, f32),
    pub lower: (SegmentId, f32),
}

impl RoadNetwork {
    /// Every place two ways cross without a junction, one passing over the
    /// other: what `add_segment_between` left grade-separated. Where the
    /// upper way has its vertex over the crossing, the crossing is found
    /// once (on either piece beside the vertex).
    pub fn grade_crossings(&self) -> Vec<Crossing> {
        let mut out: Vec<Crossing> = Vec::new();
        const EPS: f32 = 1e-3;
        for (sid, s) in self.alive_segments() {
            let (p0, p1) = self.segment_points(sid);
            for oid in self.grid().in_box(p0.min(p1), p0.max(p1)) {
                let oid = SegmentId(oid);
                if oid <= sid {
                    continue;
                }
                let o = self.segment(oid);
                if !o.alive || o.a == s.a || o.a == s.b || o.b == s.a || o.b == s.b {
                    continue;
                }
                let (q0, q1) = self.segment_points(oid);
                let Some((t, u)) = segment_intersection_closed(p0, p1, q0, q1, EPS) else { continue };
                let (ls, lo) = (self.segment_level(sid, t), self.segment_level(oid, u));
                if !separated(s.class(), ls, o.class(), lo) {
                    continue;
                }
                let at = p0.lerp(p1, t);
                let (upper, lower) = if passes_over_class(s.class(), ls, o.class(), lo) { ((sid, t), (oid, u)) } else { ((oid, u), (sid, t)) };
                let shares = |x: SegmentId, y: SegmentId| {
                    let (a, b) = (self.segment(x), self.segment(y));
                    x == y || a.a == b.a || a.a == b.b || a.b == b.a || a.b == b.b
                };
                if out.iter().any(|c| c.at.distance(at) < 1.0 && shares(c.upper.0, upper.0) && shares(c.lower.0, lower.0)) {
                    continue;
                }
                out.push(Crossing { at, upper, lower });
            }
        }
        out
    }
}
