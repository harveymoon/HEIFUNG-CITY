//! [r5-drive] What the player's car in Drive mode drives on: the road
//! under it at the level it is on (so a car on a viaduct stays up and one
//! in a tunnel stays down), the ground off the road, and what stops it
//! (buildings, water, a wall or a drop). The app's driving
//! (`nd_app::drive`) asks this every frame; `player_car` is what the
//! traffic is told (`Traffic::player`).

use super::*;
use crate::traffic::PlayerCar;

/// Highest step (m) the car takes up or down from one move to the next: a
/// kerb, a ramp's grade at speed. More is a wall, a drop off a deck or the
/// side of a cutting.
pub const DRIVE_STEP: f32 = 0.9;
/// A road piece is under the car this far past its half width (its
/// shoulder and kerb).
const ROAD_EDGE: f32 = 0.6;

/// The surface under a point of the car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DriveGround {
    /// On a road piece (street, avenue, highway, ramp, bridge, tunnel).
    Road { segment: SegmentId, height: f32 },
    /// In a junction.
    Junction { node: NodeId, height: f32 },
    /// Off the road: pavement, grass, a plaza.
    Off { height: f32 },
}

impl DriveGround {
    pub fn height(&self) -> f32 {
        match *self {
            DriveGround::Road { height, .. } | DriveGround::Junction { height, .. } | DriveGround::Off { height } => height,
        }
    }

    pub fn on_road(&self) -> bool {
        !matches!(self, DriveGround::Off { .. })
    }
}

/// What the car cannot drive into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveBlock {
    Building,
    Water,
    /// Too high a step: a wall, a drop off a deck, a cliff, a tunnel's side.
    Wall,
    /// The edge of the city's land.
    Edge,
}

impl DriveBlock {
    pub fn label(self) -> &'static str {
        match self {
            DriveBlock::Building => "a building",
            DriveBlock::Water => "the water",
            DriveBlock::Wall => "a wall",
            DriveBlock::Edge => "the edge of the map",
        }
    }
}

impl World {
    /// The road pieces and junctions under `p`, with their heights there:
    /// `(ground, sideways distance from the piece's centre line)`.
    fn drive_roads(&self, p: Vec2) -> Vec<(DriveGround, f32)> {
        let mut out = Vec::new();
        let r = 30.0;
        let lift = |h: f32| if self.relief { h } else { 0.0 };
        for sid in self.roads.segments_in_box(p - Vec2::splat(r), p + Vec2::splat(r)) {
            let seg = self.roads.segment(sid);
            if !seg.alive || !seg.is_street() {
                continue;
            }
            let (a, b) = self.roads.segment_points(sid);
            let (t, q) = crate::geom::project_on_segment(p, a, b);
            let d = q.distance(p);
            if d <= seg.half_width() + ROAD_EDGE {
                out.push((DriveGround::Road { segment: sid, height: lift(self.road_height(sid, t)) }, d));
            }
            for (node, end_t) in [(seg.a, 0.0), (seg.b, 1.0)] {
                let at = self.roads.node(node).pos;
                let radius = self.traffic.net.node_radius.get(node.0 as usize).copied().unwrap_or(0.0).max(seg.half_width());
                if at.distance(p) <= radius + ROAD_EDGE && !out.iter().any(|(g, _)| matches!(g, DriveGround::Junction { node: n, .. } if *n == node)) {
                    out.push((DriveGround::Junction { node, height: lift(self.road_height(sid, end_t)) }, at.distance(p)));
                }
            }
        }
        out
    }

    /// The road under `p` at the level nearest `floor` (within
    /// `DRIVE_STEP`), if any: a junction before the pieces meeting in it.
    pub fn drive_road_at(&self, p: Vec2, floor: f32) -> Option<DriveGround> {
        self.drive_roads(p)
            .into_iter()
            .filter(|(g, _)| (g.height() - floor).abs() <= DRIVE_STEP)
            .min_by(|(a, da), (b, db)| {
                let rank = |g: &DriveGround| if matches!(g, DriveGround::Junction { .. }) { 0 } else { 1 };
                ((a.height() - floor).abs() + 0.5 * rank(a) as f32 + da * 0.01).total_cmp(&((b.height() - floor).abs() + 0.5 * rank(b) as f32 + db * 0.01))
            })
            .map(|(g, _)| g)
    }

    /// How far out from the middle the player may drive: the land there is
    /// to drive on (as Walk mode walks).
    pub fn drive_limit(&self) -> f32 {
        let limit = (self.parcel_half() + 150.0).min(self.landscape.land_half());
        limit.max(self.config.half_extent + 150.0)
    }

    /// What the car finds at `p`, coming from a floor at `floor`: the road
    /// at that level, else the ground (if it is near that level, dry and
    /// clear of buildings), else what blocks it.
    pub fn drive_ground(&self, p: Vec2, floor: f32) -> Result<DriveGround, DriveBlock> {
        let limit = self.drive_limit();
        if p.x.abs() > limit || p.y.abs() > limit {
            return Err(DriveBlock::Edge);
        }
        if let Some(g) = self.drive_road_at(p, floor) {
            return Ok(g);
        }
        let height = self.surface_height_near(p, floor);
        if (height - floor).abs() > DRIVE_STEP {
            return Err(DriveBlock::Wall);
        }
        if self.relief && !self.landscape.is_flat() && self.landscape.is_water(p) && self.ground.deck_at(p).is_none() {
            return Err(DriveBlock::Water);
        }
        if self.building_lot_at(p) {
            return Err(DriveBlock::Building);
        }
        Ok(DriveGround::Off { height })
    }

    /// Is `p` on the lot of a building (not a park)?
    fn building_lot_at(&self, p: Vec2) -> bool {
        self.buildings.values().any(|b| {
            b.land_use != LandUse::Park
                && self.lots.get(b.lot).is_some_and(|l| l.center.distance_squared(p) < 120.0 * 120.0 && crate::geom::point_in_polygon(p, &l.polygon))
        })
    }

    /// The player's car as the traffic sees it: where it is, how it moves,
    /// and the road pieces and junctions under it at its level.
    pub fn player_car(&self, pos: Vec2, heading: f32, v: f32, accel: f32, floor: f32) -> PlayerCar {
        let mut segments = Vec::new();
        let mut nodes = Vec::new();
        // The body reaches a little past its centre either side.
        let dir = Vec2::new(heading.cos(), heading.sin());
        for probe in [pos, pos + dir * crate::traffic::PLAYER_HALF_LEN, pos - dir * crate::traffic::PLAYER_HALF_LEN] {
            for (g, _) in self.drive_roads(probe) {
                if (g.height() - floor).abs() > 2.5 {
                    continue;
                }
                match g {
                    DriveGround::Road { segment, .. } if !segments.contains(&segment) => segments.push(segment),
                    DriveGround::Junction { node, .. } if !nodes.contains(&node) => nodes.push(node),
                    _ => {}
                }
            }
        }
        PlayerCar { pos, heading, v, accel, segments, nodes }
    }

    /// The name of the street the car is on (or in the junction of).
    pub fn drive_street_name(&self, g: &DriveGround) -> Option<String> {
        let sid = match *g {
            DriveGround::Road { segment, .. } => segment,
            DriveGround::Junction { node, .. } => self.roads.streets_at(node).next()?,
            DriveGround::Off { .. } => return None,
        };
        self.streets.of(sid).map(|s| s.name.clone())
    }

    /// The speed limit (m/s) of the road piece the car is on, the lane
    /// nearest it along its heading.
    pub fn drive_limit_at(&self, g: &DriveGround, pos: Vec2, heading: f32) -> Option<f32> {
        let DriveGround::Road { segment, .. } = *g else { return None };
        let dir = Vec2::new(heading.cos(), heading.sin());
        let lanes = self.traffic.net.segment_lanes.get(&segment)?;
        lanes
            .iter()
            .filter_map(|&l| self.traffic.net.lanes.get(l as usize))
            .filter(|l| (l.end() - l.start()).normalize_or_zero().dot(dir) > 0.0)
            .min_by(|a, b| a.project(pos).1.total_cmp(&b.project(pos).1))
            .map(|l| l.speed_limit)
    }

    /// [r5-drive] The lane the car drives along: the nearest lane of the
    /// road piece under it running its way, as (centre point nearest,
    /// direction there, sideways offset + right).
    pub fn drive_lane(&self, g: &DriveGround, pos: Vec2, heading: f32) -> Option<(Vec2, Vec2, f32)> {
        let DriveGround::Road { segment, .. } = *g else { return None };
        let dir = Vec2::new(heading.cos(), heading.sin());
        let lanes = self.traffic.net.segment_lanes.get(&segment)?;
        let lane = lanes
            .iter()
            .filter_map(|&l| self.traffic.net.lanes.get(l as usize))
            .filter(|l| (l.end() - l.start()).normalize_or_zero().dot(dir) > 0.3)
            .min_by(|a, b| a.project(pos).1.total_cmp(&b.project(pos).1))?;
        let (s, _) = lane.project(pos);
        let (q, d) = lane.point_at(s);
        let right = Vec2::new(-d.y, d.x);
        Some((q, d, (pos - q).dot(right)))
    }

    /// [r5-drive] Is the car in a tunnel: on a tunnel's piece, or in a
    /// junction whose streets are all in tunnels?
    pub fn drive_in_tunnel(&self, g: &DriveGround) -> bool {
        match *g {
            DriveGround::Road { segment, .. } => self.roads.try_segment(segment).is_some_and(|s| s.tunnel),
            DriveGround::Junction { node, .. } => {
                let mut any = false;
                for sid in self.roads.streets_at(node) {
                    if !self.roads.segment(sid).tunnel {
                        return false;
                    }
                    any = true;
                }
                any
            }
            DriveGround::Off { .. } => false,
        }
    }

    /// [r5-drive] The ways on through the junction at the end of the lane
    /// the car drives along (its connectors), for telling a red run.
    pub fn drive_lane_ahead(&self, g: &DriveGround, pos: Vec2, heading: f32) -> Option<Vec<crate::traffic::LaneId>> {
        let DriveGround::Road { segment, .. } = *g else { return None };
        let dir = Vec2::new(heading.cos(), heading.sin());
        let net = &self.traffic.net;
        let lane = net.segment_lanes.get(&segment)?.iter().copied().filter(|&l| net.lanes.get(l as usize).is_some_and(|l| (l.end() - l.start()).normalize_or_zero().dot(dir) > 0.3)).min_by(|&a, &b| net.lanes[a as usize].project(pos).1.total_cmp(&net.lanes[b as usize].project(pos).1))?;
        let conns: Vec<crate::traffic::LaneId> = net.lanes[lane as usize].next.iter().copied().filter(|&l| net.lanes.get(l as usize).is_some_and(|l| l.is_connector())).collect();
        Some(conns)
    }
}
