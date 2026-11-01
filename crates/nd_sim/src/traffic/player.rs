//! [r5-drive] The player's own car in Drive mode, as the traffic sees it.
//!
//! The player's car is not a `Car` of the traffic model: the model moves
//! cars along lanes on routes, a tick (several seconds of driving) at a
//! time, while the player steers freely, frame by frame, over kerbs and
//! across lanes. So the car stays in the app (`nd_app::drive`) and the
//! traffic is only told where it is (`Traffic::player`, set before each
//! tick, never saved): a body on the road that drivers see on their way
//! ahead, queue behind (the car-following model takes it as a leader),
//! and wait for at a junction it stands in. Pulled onto the shoulder far
//! enough to be out of a lane (parked), traffic goes by.
//!
//! The tick's substeps run over the seconds the renderer will show next,
//! while the player drives on, so the body is predicted along that time
//! from the speed and acceleration it had when the tick ran (a braking
//! player is seen stopping, not carrying on).
//!
//! Without a player (`None`, every run of the sim on its own, every test)
//! nothing here runs: the model is exactly what it was.

use super::*;

/// Half the player car's length and width (m).
pub const PLAYER_HALF_LEN: f32 = 2.3;
pub const PLAYER_HALF_WID: f32 = 0.95;
/// A lane's driver sees the player when the player's centre is this close
/// sideways to the lane's centre line: half its own width, half the
/// player's and a little clearance.
pub const PLAYER_LATERAL: f32 = PLAYER_HALF_WID + CAR_WIDTH * 0.5 + 0.35;
/// The prediction runs this far at most (s).
const PREDICT_MAX: f32 = 6.0;
/// Fastest the prediction lets the player go (m/s).
const PREDICT_TOP: f32 = 40.0;
/// How far ahead along its route a driver looks for the player (lanes).
const ROUTE_LANES: usize = 6;

/// Where the player's car is and how it is moving, with the road pieces
/// and junctions at its level (so a car on the street under a viaduct the
/// player drives over does not stop for it). Built by
/// `World::player_car`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerCar {
    pub pos: Vec2,
    /// Sim radians from +x.
    pub heading: f32,
    /// Speed along the heading (m/s, negative reversing).
    pub v: f32,
    /// Acceleration along the heading (m/s²).
    pub accel: f32,
    /// Road pieces under it at its level.
    pub segments: Vec<SegmentId>,
    /// Junctions it is in or at, at its level.
    pub nodes: Vec<NodeId>,
}

impl PlayerCar {
    /// Unit vector of the heading.
    pub fn dir(&self) -> Vec2 {
        Vec2::new(self.heading.cos(), self.heading.sin())
    }

    /// The speed `t` seconds on: the acceleration held, never through zero
    /// (braking stops it, it does not reverse it), within limits.
    pub fn speed_at(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, PREDICT_MAX);
        let v = self.v + self.accel * t;
        if self.v >= 0.0 {
            v.clamp(0.0, PREDICT_TOP)
        } else {
            v.clamp(-PREDICT_TOP, 0.0)
        }
    }

    /// Where it will be `t` seconds on (along its heading, as `speed_at`).
    pub fn predict(&self, t: f32) -> Vec2 {
        let t = t.clamp(0.0, PREDICT_MAX);
        // A few steps of the clamped speed: exact for constant speed and
        // close enough through a stop.
        const N: usize = 8;
        let h = t / N as f32;
        let mut s = 0.0;
        for k in 0..N {
            s += self.speed_at((k as f32 + 0.5) * h) * h;
        }
        self.pos + self.dir() * s
    }
}

impl Traffic {
    /// The player's body this far into the step: (centre, speed).
    pub(super) fn player_now(&self) -> Option<(Vec2, f32)> {
        self.player.as_ref().map(|p| (p.predict(self.player_clock), p.speed_at(self.player_clock)))
    }

    /// What the player's car asks of `car`: a gap to its rear and the
    /// speed it goes away at, if it stands on the car's way within the
    /// look-ahead; or a stop at the line when the player is in the
    /// junction the car is about to enter.
    pub(super) fn player_gap(&self, car: &Car, now: (Vec2, f32)) -> Option<(f32, f32)> {
        let p = self.player.as_ref()?;
        let (pos, v) = now;
        if car.pos.distance(pos) > LOOKAHEAD + 30.0 {
            return None;
        }
        let dir = p.dir();
        let mut acc = -car.s;
        for &lid in car.route.iter().skip(car.idx).take(ROUTE_LANES) {
            let Some(lane) = self.net.lanes.get(lid as usize) else { break };
            let relevant = match lane.kind {
                LaneKind::Road { segment, .. } => p.segments.contains(&segment),
                LaneKind::Connector { node, .. } => p.nodes.contains(&node),
            };
            if relevant {
                // The player in the junction ahead: wait at the line.
                if let LaneKind::Connector { node, .. } = lane.kind {
                    let r = self.net.node_radius.get(node.0 as usize).copied().unwrap_or(8.0);
                    // The connector's middle stands for the junction's.
                    let mid = lane.point_at(lane.length * 0.5).0;
                    if acc > 0.5 && pos.distance(mid) < r + 1.5 {
                        return Some(((acc - 1.0 + IDM_MIN_GAP).max(0.1), 0.0));
                    }
                }
                let (s_p, lat) = lane.project(pos);
                if lat < PLAYER_LATERAL {
                    let (_, ld) = lane.point_at(s_p);
                    let (c, s) = (dir.dot(ld).abs(), dir.perp_dot(ld).abs());
                    let half = c * PLAYER_HALF_LEN + s * PLAYER_HALF_WID;
                    let along = acc + s_p;
                    // Some of it ahead of the car (wholly behind: the car
                    // does not care). A car is drawn centred on where it
                    // is along its lane, so its nose is half a length on.
                    if along + half > 0.0 {
                        return Some(((along - half - car.length() * 0.5).max(0.1), (v * dir.dot(ld)).max(0.0)));
                    }
                }
            }
            acc += lane.length;
            if acc > LOOKAHEAD {
                break;
            }
        }
        None
    }
}
