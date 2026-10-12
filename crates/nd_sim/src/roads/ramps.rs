//! Ramps and heights: which ways are ramps, their ends eased into the
//! roads they join, the height samples along a way, and the classification
//! of every piece once the network is built.

use super::*;

/// Whether a way with these lanes is a highway carriageway (see
/// [`Segment::is_highway`]).
pub fn is_highway_spec(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> bool {
    kind == RoadKind::Street && (lanes_fwd == 0 || lanes_bwd == 0) && lanes_fwd + lanes_bwd >= 3
}

/// [w1-roads] What a road carries, set on the road by the player (not read
/// from its lanes): walkers and bikes only (a path), cars and walkers (a
/// street: pavements, street speed, lots along it), or cars only (a
/// highway or slip road: no pavements, highway speed, fronts nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoadUse {
    Pedestrian,
    Mixed,
    CarOnly,
}

impl RoadUse {
    pub const ALL: [RoadUse; 3] = [RoadUse::Pedestrian, RoadUse::Mixed, RoadUse::CarOnly];

    pub fn label(self) -> &'static str {
        match self {
            RoadUse::Pedestrian => "Pedestrians only",
            RoadUse::Mixed => "Mixed",
            RoadUse::CarOnly => "Car-only",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            RoadUse::Pedestrian => "pedestrian",
            RoadUse::Mixed => "mixed",
            RoadUse::CarOnly => "car_only",
        }
    }

    pub fn parse(s: &str) -> Option<RoadUse> {
        match s.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "pedestrian" | "pedestrians" | "pedestrians_only" | "path" | "foot" => Some(RoadUse::Pedestrian),
            "mixed" | "street" => Some(RoadUse::Mixed),
            "car_only" | "cars_only" | "car" | "cars" | "highway" | "motorway" => Some(RoadUse::CarOnly),
            _ => None,
        }
    }

    /// The use of a way as built: its kind, and for a street the explicit
    /// car-only flag, or (`None`, roads from before it was set) what its
    /// lanes made it: a one-way street of three lanes or more was a highway.
    pub fn of(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8, car_only: Option<bool>) -> RoadUse {
        match kind {
            RoadKind::Path => RoadUse::Pedestrian,
            RoadKind::Rail => RoadUse::CarOnly,
            RoadKind::Street => match car_only {
                Some(true) => RoadUse::CarOnly,
                Some(false) => RoadUse::Mixed,
                None if is_highway_spec(kind, lanes_fwd, lanes_bwd) => RoadUse::CarOnly,
                None => RoadUse::Mixed,
            },
        }
    }
}

/// [w1-roads] A highway carriageway, with the road's own use: a one-way
/// street of three lanes or more that is not set to Mixed.
pub fn is_highway_use(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8, car_only: Option<bool>) -> bool {
    car_only != Some(false) && is_highway_spec(kind, lanes_fwd, lanes_bwd)
}

/// [w1-roads] A slip road in the making, with the road's own use: a one-way
/// street too narrow for a highway that is not set to Mixed.
pub fn is_ramp_use(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8, car_only: Option<bool>) -> bool {
    car_only != Some(false) && (is_ramp_spec(kind, lanes_fwd, lanes_bwd) || (car_only == Some(true) && kind == RoadKind::Street && (lanes_fwd == 0) != (lanes_bwd == 0) && !is_highway_spec(kind, lanes_fwd, lanes_bwd)))
}

/// A ramp: a one-way street too narrow to be a highway (slip roads on and
/// off the highway, and any one-way link).
pub fn is_ramp_spec(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> bool {
    kind == RoadKind::Street && (lanes_fwd == 0) != (lanes_bwd == 0) && !is_highway_spec(kind, lanes_fwd, lanes_bwd)
}

/// How far back from a merge (or on from a diverge) a ramp runs alongside
/// the highway before it joins.
pub const RAMP_LEAD: f32 = 40.0;

/// [interchange] The shortest lead a short ramp gets: a ramp whose far end
/// is close along the highway gets a lead of a share of that distance, so
/// it does not run alongside past the place it is going to.
pub const RAMP_LEAD_MIN: f32 = 15.0;

/// A ramp polyline with its ends on a highway eased in: where an end lies on
/// a highway carriageway and the ramp meets it at more than 15 degrees, a
/// lead point is put before the join (a merge) or after it (a diverge),
/// beside the carriageway, so the last piece runs alongside it and joins
/// nearly tangent; then the bend from the lead to the ramp's next point is
/// rounded (a few points on a curve leaving the lead along the highway).
/// `forward` is whether traffic runs in point order.
///
/// [interchange] The lead is `RAMP_LEAD` on a long ramp, but at most 45% of
/// how far the ramp's next point lies along the highway (not under
/// `RAMP_LEAD_MIN`): a fixed 40 m lead on a diamond whose ramps reach the
/// cross street 50 m along ran each ramp alongside the highway right up to
/// the street under it and then hairpinned onto it, over the street's own
/// asphalt. Easing is idempotent: an end whose next point already stands
/// beside the carriageway (a lead) is left alone, so a preview can ease
/// what the build eases again.
pub fn ease_ramp_ends(points: &[Vec2], forward: bool, roads: &RoadNetwork) -> Vec<Vec2> {
    let mut pts = points.to_vec();
    if pts.len() < 2 {
        return pts;
    }
    for end in [false, true] {
        let n = pts.len();
        let (i_end, i_next) = if end { (n - 1, n - 2) } else { (0, 1) };
        let p = pts[i_end];
        let Some((sid, _, q)) = roads.nearest_where(p, Segment::is_highway) else { continue };
        if q.distance(p) > END_SNAP_RADIUS {
            continue;
        }
        let s = roads.segment(sid);
        let (a, b) = roads.segment_points(sid);
        let travel = if s.lanes_fwd > 0 { (b - a).normalize_or_zero() } else { (a - b).normalize_or_zero() };
        let next = pts[i_next];
        let away = (next - q).normalize_or_zero();
        if away == Vec2::ZERO || travel.dot(away).abs() > 0.966 || next.distance(q) < 25.0 {
            continue;
        }
        // Traffic arrives at this end (a merge) or leaves from it (a diverge).
        let merge = end == forward;
        let along = if merge { -travel } else { travel };
        let side = Vec2::new(-travel.y, travel.x);
        let side = if side.dot(away) >= 0.0 { side } else { -side };
        // Beside the carriageway: half its width, half the ramp's, a margin.
        let gap = s.half_width() + LANE_WIDTH * 0.5 + SHOULDER + 1.0;
        let (ra, rb) = ((next - q).dot(along), (next - q).dot(side));
        // Already beside the carriageway, ahead of the join: a lead.
        if ra > 0.0 && rb <= gap + 0.5 {
            continue;
        }
        let lead_len = if ra > 0.0 { (ra * 0.45).max(rb * 0.25).clamp(RAMP_LEAD_MIN, RAMP_LEAD) } else { RAMP_LEAD };
        let lead = q + along * lead_len + side * gap;
        // The bend from the lead to the next point, rounded: a quadratic
        // curve leaving the lead along the highway.
        let rest = next - lead;
        let ahead = rest.dot(along);
        let mut bend: Vec<Vec2> = Vec::new();
        if ahead > 4.0 {
            // A gentle pull: a control point further on cut the bend in
            // toward a cross street the ramp turns onto.
            let c = lead + along * (ahead * 0.3);
            let k = ((rest.length() / 12.0).round() as usize).clamp(2, 6);
            for i in 1..k {
                let t = i as f32 / k as f32;
                bend.push(lead * (1.0 - t) * (1.0 - t) + c * (2.0 * (1.0 - t) * t) + next * (t * t));
            }
        }
        pts[i_end] = q;
        if end {
            // ..., next, bend (from next back to the lead), lead, q.
            let mut ins: Vec<Vec2> = bend.into_iter().rev().collect();
            ins.push(lead);
            for (k, v) in ins.into_iter().enumerate() {
                pts.insert(i_end + k, v);
            }
        } else {
            // q, lead, bend (from the lead on to next), next, ...
            let mut ins = vec![lead];
            ins.extend(bend);
            for (k, v) in ins.into_iter().enumerate() {
                pts.insert(1 + k, v);
            }
        }
    }
    pts
}

/// [interchange] A way's points and levels with its ramp ends eased in, as
/// the network lays it (`ease_ramp_ends`, levels carried onto the new
/// points): only one-way streets narrower than a highway that are not a
/// roundabout's ring. The world's rebuild and the previews both use this,
/// so the hologram shows the ramp as it will be built.
pub fn ease_ramp_way(points: &[Vec2], levels: &[f32], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind, roads: &RoadNetwork) -> (Vec<Vec2>, Vec<f32>) {
    if !is_ramp_spec(kind, lanes_fwd, lanes_bwd) || roads.is_ring_way(points, lanes_fwd, lanes_bwd, kind) {
        return (points.to_vec(), levels.to_vec());
    }
    let eased = ease_ramp_ends(points, lanes_fwd > 0, roads);
    if eased == points {
        return (eased, levels.to_vec());
    }
    let lv = crate::plan::carry_levels(points, levels, &eased);
    (eased, lv)
}

/// Spacing of the height samples along a long way (`sample_heights`).
pub const HEIGHT_SAMPLE: f32 = 120.0;

/// A polyline with a vertex wherever a leg longer than `spacing` crosses a
/// line of a world grid of that spacing (across its main direction), so
/// every long way gets a height to ease at least that often. The grid is
/// fixed in the world, so two parallel ways (the two carriageways of the
/// highway) get their samples side by side and are eased alike. No sample
/// falls within a fifth of the spacing of a leg's ends.
pub fn sample_heights(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(points.len());
    for (i, w) in points.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        if i == 0 {
            out.push(a);
        }
        let d = b - a;
        let len = d.length();
        if len > spacing {
            let along_x = d.x.abs() >= d.y.abs();
            let (c0, c1) = if along_x { (a.x, b.x) } else { (a.y, b.y) };
            let (lo, hi) = (c0.min(c1), c0.max(c1));
            let margin = spacing * 0.2 * (hi - lo) / len;
            let mut ts: Vec<f32> = Vec::new();
            let mut k = (lo / spacing).ceil() as i64;
            while (k as f32) * spacing < hi {
                let c = k as f32 * spacing;
                if c > lo + margin && c < hi - margin {
                    ts.push((c - c0) / (c1 - c0));
                }
                k += 1;
            }
            ts.sort_by(|x, y| x.total_cmp(y));
            out.extend(ts.into_iter().map(|t| a + d * t));
        }
        out.push(b);
    }
    if points.len() == 1 {
        out.push(points[0]);
    }
    out
}

impl RoadNetwork {
    /// Set the derived flags of every piece: `ring` for the pieces of a
    /// roundabout, `ramp` for highway slip roads. A slip road is found from
    /// the highway outwards: a one-way street (not a ring, not a highway)
    /// leaving a node on a highway, followed through plain bends and
    /// ramp forks and merges (nodes where only one-way streets meet, three
    /// at most) until it reaches a two-way street, a ring or another
    /// highway. So a one-way street in town is never a ramp, nor is a
    /// one-way couplet that happens to start at a slip road's end.
    pub fn classify(&mut self) {
        for i in 0..self.segments.len() {
            let ring = self.ring_of_segment(SegmentId(i as u32)).is_some();
            self.segments[i].ring = ring;
            self.segments[i].ramp = false;
        }
        let one_way = |s: &Segment| s.alive && s.is_street() && !s.ring && !s.is_highway() && s.car_only != Some(false) && (s.lanes_fwd == 0) != (s.lanes_bwd == 0);
        let mut stack: Vec<SegmentId> = Vec::new();
        for n in &self.nodes {
            if n.segments.iter().any(|&s| self.segments[s.0 as usize].alive && self.segments[s.0 as usize].is_highway()) {
                stack.extend(n.segments.iter().copied().filter(|&s| one_way(&self.segments[s.0 as usize])));
            }
        }
        while let Some(sid) = stack.pop() {
            if self.segments[sid.0 as usize].ramp {
                continue;
            }
            self.segments[sid.0 as usize].ramp = true;
            let (a, b) = (self.segments[sid.0 as usize].a, self.segments[sid.0 as usize].b);
            for n in [a, b] {
                let alive: Vec<SegmentId> = self.streets_at(n).collect();
                if alive.len() > 3 || !alive.iter().all(|&s| one_way(&self.segments[s.0 as usize])) {
                    continue;
                }
                stack.extend(alive.into_iter().filter(|&s| !self.segments[s.0 as usize].ramp));
            }
        }
    }
}
