//! Roundabouts: the ring geometry, where it clips the roads that meet it,
//! and the layout of every ring on the map.

use super::*;

impl RoadNetwork {
    /// Is this polyline a roundabout's ring: a one-way street with every
    /// point on the circle of one of `rings` with its lane count?
    pub fn is_ring_way(&self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> bool {
        kind == RoadKind::Street && lanes_bwd == 0 && lanes_fwd > 0 && points.len() >= 3 && self.rings.iter().any(|r| r.lanes == lanes_fwd && points.iter().all(|&p| r.on_circle(p)))
    }

    /// The roundabout whose ring this segment is a piece of: a one-way
    /// street with both ends on the circle.
    pub fn ring_of_segment(&self, sid: SegmentId) -> Option<usize> {
        if self.rings.is_empty() {
            return None;
        }
        let s = self.segments.get(sid.0 as usize)?;
        if !s.alive || !s.is_street() || s.lanes_bwd != 0 || s.lanes_fwd == 0 {
            return None;
        }
        let (a, b) = (self.node(s.a).pos, self.node(s.b).pos);
        self.rings.iter().position(|r| s.lanes_fwd == r.lanes && r.on_circle(a) && r.on_circle(b))
    }

    /// The roundabout a node sits on (any node with a ring piece).
    pub fn ring_of_node(&self, n: NodeId) -> Option<usize> {
        if self.rings.is_empty() {
            return None;
        }
        self.node(n).segments.iter().find_map(|&s| self.ring_of_segment(s))
    }
}

/// Roundabout radius limits (centre to the middle of the ring road), metres:
/// 24 to 240 m across.
pub const RING_MIN_RADIUS: f32 = 12.0;

pub const RING_MAX_RADIUS: f32 = 120.0;

/// Most lanes a roundabout's ring may have.
pub const RING_MAX_LANES: u8 = 3;

/// Two arms whose joins on the circle are closer than this share one node.
pub(super) const RING_JOIN_MERGE: f32 = 8.0;

/// Target length of a ring piece between joins.
pub(super) const RING_PIECE: f32 = 10.0;

/// A roundabout: a one-way ring road round a central island. Traffic keeps
/// the island on its left (right-hand traffic: anticlockwise seen from
/// above, which is clockwise in the sim plane's x/y, since y maps to
/// world z).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ring {
    pub center: Vec2,
    /// Centre of the island to the middle of the ring road.
    pub radius: f32,
    /// Ring lanes, 1 to [`RING_MAX_LANES`].
    pub lanes: u8,
}

impl Ring {
    /// A ring with its radius and lanes brought within limits.
    pub fn new(center: Vec2, radius: f32, lanes: u8) -> Self {
        Self { center, radius: radius.clamp(RING_MIN_RADIUS, RING_MAX_RADIUS), lanes: lanes.clamp(1, RING_MAX_LANES) }
    }

    pub fn half_width(&self) -> f32 {
        self.lanes as f32 * LANE_WIDTH * 0.5 + SHOULDER
    }

    /// Radius of the ring's inner kerb: inside it the ground is open land.
    pub fn island_radius(&self) -> f32 {
        (self.radius - self.half_width() - 0.3).max(2.0)
    }

    /// Radius inside which the ground is clear of the ring road and its
    /// inner pavement.
    pub fn inner_radius(&self) -> f32 {
        (self.radius - self.half_width() - crate::zoning::SIDEWALK).max(0.0)
    }

    /// Speed on the ring, m/s: about 25 km/h on a small ring, faster on a
    /// big one (what a comfortable sideways pull allows), at most the
    /// street limit.
    pub fn speed(&self) -> f32 {
        (2.0 * self.radius).sqrt().clamp(7.0, 13.0)
    }

    /// Does `inner` sit wholly inside this ring with room between them for
    /// a lane and a pavement?
    pub fn holds(&self, inner: &Ring) -> bool {
        self.center.distance(inner.center) + inner.outer_radius() + LANE_WIDTH + crate::zoning::SIDEWALK <= self.inner_radius()
    }

    /// Radius of the ground the ring takes, pavement included.
    pub fn outer_radius(&self) -> f32 {
        self.radius + self.half_width() + crate::zoning::SIDEWALK
    }

    /// Is `p` on the ring's circle (a ring vertex, or a point a road end
    /// split a ring piece at)?
    pub fn on_circle(&self, p: Vec2) -> bool {
        let d = p.distance(self.center);
        d > self.radius - 1.6 && d < self.radius + 0.6
    }

    /// Do two rings fight over ground? Apart, or one nested inside the
    /// other with clearance, they do not.
    pub fn overlaps(&self, other: &Ring) -> bool {
        self.center.distance(other.center) < self.outer_radius() + other.outer_radius() && !self.holds(other) && !other.holds(self)
    }

    /// Direction of travel at a point of the ring (island on the left).
    pub fn travel_dir(&self, p: Vec2) -> Vec2 {
        let r = (p - self.center).normalize_or_zero();
        Vec2::new(r.y, -r.x)
    }

    /// Split a polyline at the circle: the pieces outside (their ends on
    /// the circle where they were cut) and the pieces inside, in order.
    pub fn clip(&self, pts: &[Vec2]) -> (Vec<Vec<Vec2>>, Vec<Vec<Vec2>>) {
        let (c, r) = (self.center, self.radius);
        let inside = |p: Vec2| p.distance(c) < r;
        if pts.len() < 2 {
            return (vec![pts.to_vec()], Vec::new());
        }
        let mut outside_pieces: Vec<Vec<Vec2>> = Vec::new();
        let mut inside_pieces: Vec<Vec<Vec2>> = Vec::new();
        let mut cur: Vec<Vec2> = vec![pts[0]];
        let mut cur_in = inside(pts[0]);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            // Circle crossings strictly inside the leg, in order.
            let d = q - p;
            let f = p - c;
            let (a, b, cc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - r * r);
            let disc = b * b - 4.0 * a * cc;
            if a > 1e-6 && disc > 0.0 {
                let sq = disc.sqrt();
                for t in [(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)] {
                    if t > 1e-4 && t < 1.0 - 1e-4 {
                        let x = p + d * t;
                        cur.push(x);
                        let piece = std::mem::replace(&mut cur, vec![x]);
                        if cur_in { inside_pieces.push(piece) } else { outside_pieces.push(piece) }
                        cur_in = !cur_in;
                    }
                }
            }
            cur.push(q);
        }
        if cur_in { inside_pieces.push(cur) } else { outside_pieces.push(cur) }
        outside_pieces.retain(|v| path_len(v) > 1.0);
        inside_pieces.retain(|v| v.len() >= 2);
        (outside_pieces, inside_pieces)
    }

    /// The ring road as a closed polyline (first point repeated last) in
    /// the direction of travel, with a vertex at every join (snapped
    /// together where two are closer than a few metres) and pieces of about
    /// ten metres between. Returns the polyline and the joins as placed.
    pub fn polyline(&self, joins: &[Vec2]) -> (Vec<Vec2>, Vec<Vec2>) {
        use std::f32::consts::TAU;
        let (c, r) = (self.center, self.radius);
        let ang = |p: Vec2| {
            let a = (p - c).y.atan2((p - c).x);
            if a < 0.0 { a + TAU } else { a }
        };
        let mut a: Vec<f32> = joins.iter().map(|&p| ang(p)).collect();
        a.sort_by(|x, y| y.total_cmp(x));
        // Merge joins closer than RING_JOIN_MERGE along the circle.
        let min_da = RING_JOIN_MERGE / r;
        let mut merged: Vec<f32> = Vec::new();
        for x in a {
            if merged.last().is_some_and(|&l| l - x < min_da) {
                continue;
            }
            merged.push(x);
        }
        if merged.len() > 1 && (merged[merged.len() - 1] + TAU - merged[0]) < min_da {
            merged.pop();
        }
        let at = |t: f32| c + Vec2::new(t.cos(), t.sin()) * r;
        let mut out: Vec<Vec2> = Vec::new();
        if merged.is_empty() {
            let n = ((TAU * r / RING_PIECE).round() as usize).max(8);
            for k in 0..=n {
                out.push(at(-TAU * k as f32 / n as f32));
            }
            return (out, Vec::new());
        }
        for i in 0..merged.len() {
            let a0 = merged[i];
            let a1 = if i + 1 < merged.len() { merged[i + 1] } else { merged[0] - TAU };
            let span = a0 - a1; // angles decrease along the ring
            let k = ((span * r / RING_PIECE).floor() as usize).max(1);
            for j in 0..k {
                out.push(at(a0 - span * j as f32 / k as f32));
            }
        }
        out.push(out[0]);
        (out, merged.into_iter().map(at).collect())
    }
}

use crate::geom::path_length as path_len;

/// Roads reshaped by roundabouts, as built.
#[derive(Clone, Debug, Default)]
pub struct RingLayout {
    /// Per ring: the ring, its closed polyline and where roads join it.
    pub rings: Vec<(Ring, Vec<Vec2>, Vec<Vec2>)>,
    /// Pieces of each input road outside every ring, by input index.
    pub kept: Vec<(usize, Vec<Vec2>)>,
    /// Parts of roads inside a ring: what the works take up.
    pub cut: Vec<(usize, Vec<Vec2>)>,
}

/// Lay roundabouts over roads. Each ring cuts away the parts inside its
/// circle of the roads older than it (the old junction goes; the ends left
/// on the circle join the ring); roads newer than the ring are kept whole
/// inside and out and join it where they cross it. Every ring gets a vertex
/// at every join. `rings` and `roads` carry their age (gesture order) and
/// roads whether a ring may cut them at all (railways and grade-separated
/// highways pass untouched). Rings that overlap an earlier one are left out;
/// nested rings with room between them are fine.
pub fn layout_rings(rings: &[(Ring, u64)], roads: &[(Vec<Vec2>, bool, u64)]) -> RingLayout {
    let mut chosen: Vec<(Ring, u64)> = Vec::new();
    for r in rings {
        if !chosen.iter().any(|o| o.0.overlaps(&r.0)) {
            chosen.push(*r);
        }
    }
    let on = |ring: &Ring, p: Vec2| (p.distance(ring.center) - ring.radius).abs() < 1e-2;
    let mut out = RingLayout::default();
    let mut joins: Vec<Vec<Vec2>> = vec![Vec::new(); chosen.len()];
    for (i, (pts, clip, age)) in roads.iter().enumerate() {
        if !clip {
            out.kept.push((i, pts.clone()));
            continue;
        }
        let mut pieces = vec![pts.clone()];
        for (k, (ring, ring_age)) in chosen.iter().enumerate() {
            let older = age < ring_age;
            let mut next = Vec::new();
            for p in pieces {
                let (outside, inside) = ring.clip(&p);
                let kept_inside: Vec<Vec<Vec2>> = if older { Vec::new() } else { inside.iter().filter(|v| v.len() >= 2 && path_len(v) > 1.0).cloned().collect() };
                for o in outside.iter().chain(kept_inside.iter()) {
                    for end in [o[0], *o.last().unwrap()] {
                        if on(ring, end) {
                            joins[k].push(end);
                        }
                    }
                }
                if older {
                    out.cut.extend(inside.into_iter().map(|c| (i, c)));
                }
                next.extend(outside);
                next.extend(kept_inside);
            }
            pieces = next;
        }
        out.kept.extend(pieces.into_iter().map(|p| (i, p)));
    }
    // Rings get their vertices; arm ends move onto the vertex they share.
    for (k, (ring, _)) in chosen.iter().enumerate() {
        let (poly, placed) = ring.polyline(&joins[k]);
        for (_, pts) in out.kept.iter_mut() {
            let last = pts.len() - 1;
            for end in [0, last] {
                let p = pts[end];
                if on(ring, p) {
                    if let Some(q) = placed.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))) {
                        pts[end] = *q;
                    }
                }
            }
        }
        out.rings.push((*ring, poly, placed));
    }
    out
}
