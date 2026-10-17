//! Trains: their kinds, consists, calls and states, and the stretch of
//! track they occupy.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrainKind {
    Passenger,
    Freight,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Train {
    pub kind: TrainKind,
    /// Coaches or wagons behind the locomotive.
    pub cars: u8,
    pub nodes: Vec<NodeId>,
    pub poly: Vec<Vec2>,
    /// Head of the locomotive along the polyline, now and last tick.
    pub s: f32,
    #[serde(default)]
    pub prev_s: f32,
    pub speed: f32,
    /// Stops in order: the facility and where along the polyline it is.
    pub stops: Vec<(BuildingId, f32)>,
    pub next_stop: usize,
    pub dwell: f32,
    pub passengers: Vec<CitizenId>,
    pub cargo: Vec<Cargo>,
    pub color: u8,
    /// Standing because the block ahead is taken.
    pub held: bool,
    /// Arc range of the route that is off the trunk. Branches are single
    /// track with no passing loops, so one train holds the branch at a time.
    pub branch: Option<(f32, f32)>,
    /// [train-inspect] Units of the town's exports a freight train has
    /// picked up at the yards it called at, on their way out of town.
    #[serde(default)]
    pub exports: f32,
    /// [train-inspect] Units of imports a freight train has set down so far.
    #[serde(default)]
    pub unloaded: f32,
    /// [r4] (K50) Speed now, m/s: up to the line speed `speed`, braking to
    /// a stand at calls, the switch and behind another train.
    #[serde(default)]
    pub v: f32,
    /// [r4] (K33) The length of the stand it is making now (traffic
    /// seconds): a freight train stands as long as its cranes take.
    #[serde(default)]
    pub dwell_total: f32,
    /// [r4] (K33) The shipping containers on a freight train's wagons, and
    /// the ones being lifted off or on at the yard it stands at.
    #[serde(default)]
    pub boxes: Vec<super::BoxLoad>,
}

impl Train {
    pub fn car_len(&self) -> f32 {
        match self.kind {
            TrainKind::Passenger => COACH_LEN,
            TrainKind::Freight => WAGON_LEN,
        }
    }

    pub fn length(&self) -> f32 {
        LOCO_LEN + self.cars as f32 * (self.car_len() + COUPLING)
    }

    /// [freight] Units a freight train's wagons hold, and how much is
    /// aboard now (imports still to set down and exports picked up).
    pub fn freight_capacity(&self) -> f32 {
        self.cars as f32 * WAGON_CAPACITY
    }

    pub fn freight_aboard(&self) -> f32 {
        self.cargo.iter().map(|c| c.amount).sum::<f32>() + self.exports
    }

    pub fn total(&self) -> f32 {
        path_length(&self.poly)
    }

    pub fn head(&self) -> Vec2 {
        point_along(&self.poly, self.s).0
    }

    /// [r4] Traffic seconds to run `d` metres from the speed it has now,
    /// picking up speed at [`ACCEL`] to the line speed (no braking: a
    /// crossing ahead is run over, not stopped at).
    pub fn seconds_to(&self, d: f32) -> f32 {
        let (v0, vmax) = (self.v.clamp(0.0, self.speed.max(0.1)), self.speed.max(0.1));
        let t_acc = (vmax - v0) / ACCEL;
        let d_acc = (v0 + vmax) * 0.5 * t_acc;
        if d <= d_acc {
            // v0 t + a t² / 2 = d
            (-v0 + (v0 * v0 + 2.0 * ACCEL * d.max(0.0)).sqrt()) / ACCEL
        } else {
            t_acc + (d - d_acc) / vmax
        }
    }

    /// On a branch right now.
    pub fn on_branch(&self) -> bool {
        self.branch.is_some_and(|(a, b)| self.s > a && self.s - self.length() < b)
    }

    /// The train has run its route to the end.
    pub fn finished(&self) -> bool {
        self.next_stop >= self.stops.len() && self.s >= self.total() - 0.5
    }

    /// Centre and heading of vehicle `k` (0 is the locomotive).
    pub fn vehicle_pose(&self, k: usize) -> (Vec2, f32) {
        self.vehicle_pose_at(k, self.s)
    }

    /// The same with the head at `s`: between ticks the renderer blends.
    pub fn vehicle_pose_at(&self, k: usize, s: f32) -> (Vec2, f32) {
        let len = if k == 0 { LOCO_LEN } else { self.car_len() };
        let back = if k == 0 { 0.0 } else { LOCO_LEN + COUPLING + (k as f32 - 1.0) * (self.car_len() + COUPLING) };
        let s_front = s - back;
        let (a, _) = point_along(&self.poly, s_front);
        let (b, _) = point_along(&self.poly, s_front - len);
        let d = (a - b).normalize_or_zero();
        ((a + b) * 0.5, d.y.atan2(d.x))
    }
}

/// [train-inspect] A call a train has still to make: the stop, where it is
/// along the route, and about how many sim minutes until the train is there
/// (running time at line speed plus the stand at each call on the way; a
/// signal or a busy branch can make it later).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Call {
    pub stop: BuildingId,
    pub s: f32,
    pub minutes: f32,
}

/// [train-inspect] What a train is doing right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrainState {
    /// Standing at a stop, with this many traffic seconds left.
    Dwelling { at: BuildingId, left: f32 },
    /// Stopped at a signal: the block ahead is taken (by this train, when
    /// it can be told).
    HeldBlock { by: Option<TrainId> },
    /// Waiting at the switch while another train is on the single-track branch.
    HeldBranch,
    /// Under way to its next call.
    Running,
    /// Every call made: running on out of town at the end of the line.
    Leaving,
}

impl Train {
    /// The calls still ahead, in order, skipping stops `alive` says are gone
    /// (the train runs past those). A stop the train stands at now is done,
    /// not ahead. `dwell` is the stand at each call in traffic seconds (the
    /// railway's [`DWELL_SECONDS`], the metro's own), and `seconds_per_tick`
    /// turns traffic seconds into sim minutes.
    pub fn calls_ahead(&self, dwell: f32, seconds_per_tick: f32, alive: impl Fn(BuildingId) -> bool) -> Vec<Call> {
        self.timetable(dwell, seconds_per_tick, alive).0
    }

    /// Sim minutes until the train runs off the end of its route, at line
    /// speed with a stand at each call on the way.
    pub fn minutes_to_end(&self, dwell: f32, seconds_per_tick: f32, alive: impl Fn(BuildingId) -> bool) -> f32 {
        self.timetable(dwell, seconds_per_tick, alive).1
    }

    pub(super) fn timetable(&self, dwell: f32, seconds_per_tick: f32, alive: impl Fn(BuildingId) -> bool) -> (Vec<Call>, f32) {
        let tick = seconds_per_tick.max(0.01);
        let speed = self.speed.max(0.1);
        let mut secs = 0.0f32;
        let mut s = self.s;
        let mut first = self.next_stop;
        if self.dwell > 0.0 {
            secs += self.dwell;
            first += 1;
        }
        let mut calls = Vec::new();
        for &(stop, at) in self.stops.iter().skip(first) {
            if !alive(stop) {
                continue;
            }
            secs += (at - s).max(0.0) / speed;
            calls.push(Call { stop, s: at, minutes: secs / tick });
            secs += dwell;
            s = s.max(at);
        }
        secs += (self.total() - s).max(0.0) / speed;
        (calls, secs / tick)
    }

    /// The stop the train stands at, if it is standing at one.
    pub fn dwelling_at(&self) -> Option<BuildingId> {
        (self.dwell > 0.0).then(|| self.stops.get(self.next_stop).map(|s| s.0)).flatten()
    }

    /// The stops already called at (or run past), in order.
    pub fn calls_made(&self) -> &[(BuildingId, f32)] {
        &self.stops[..self.next_stop.min(self.stops.len())]
    }

    /// The rest of the route from the head at `s` to the end, and the part
    /// already run from the start to `s`.
    pub fn route_ahead(&self, s: f32) -> Vec<Vec2> {
        sub_path(&self.poly, s, self.total())
    }

    pub fn route_behind(&self, s: f32) -> Vec<Vec2> {
        sub_path(&self.poly, 0.0, s)
    }
}

/// [train-inspect] The stretch of a polyline between arc lengths `s0` and
/// `s1` (clamped to it, in order): the points at both ends and every vertex
/// strictly between.
pub fn sub_path(pts: &[Vec2], s0: f32, s1: f32) -> Vec<Vec2> {
    if pts.is_empty() {
        return Vec::new();
    }
    let total = path_length(pts);
    let (s0, s1) = (s0.min(s1).clamp(0.0, total), s0.max(s1).clamp(0.0, total));
    let mut out = vec![point_along(pts, s0).0];
    let mut acc = 0.0;
    for w in pts.windows(2) {
        acc += w[0].distance(w[1]);
        if acc > s0 && acc < s1 {
            out.push(w[1]);
        }
    }
    out.push(point_along(pts, s1).0);
    out
}

/// [r4] (K33) Arc length of the first point after `s` where `pts` doubles
/// back on itself (a spur's end, where a train turns round), if any.
pub fn turn_point_after(pts: &[Vec2], s: f32) -> Option<f32> {
    let mut acc = 0.0;
    for i in 1..pts.len().saturating_sub(1) {
        acc += pts[i - 1].distance(pts[i]);
        let (a, b) = ((pts[i] - pts[i - 1]).normalize_or_zero(), (pts[i + 1] - pts[i]).normalize_or_zero());
        if acc > s + 0.01 && a.dot(b) < -0.5 {
            return Some(acc);
        }
    }
    None
}
