//! The vehicles on the lanes: cars, their cargo and buses, why they wait,
//! and where one is drawn.

use super::*;

/// Freight carried by a truck between two businesses.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cargo {
    /// The seller; a null key for goods imported from out of town.
    pub from: BuildingId,
    pub to: BuildingId,
    pub resource: Resource,
    pub amount: f32,
    /// Money the buyer owes on delivery.
    pub price: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Car {
    /// Driver for private cars; a null key for trucks.
    pub citizen: CitizenId,
    pub cargo: Option<Cargo>,
    pub route: Vec<LaneId>,
    pub idx: usize,
    pub s: f32,
    pub s_end: f32,
    pub v: f32,
    pub pos: Vec2,
    pub heading: f32,
    pub prev_pos: Vec2,
    pub prev_heading: f32,
    pub color: u8,
    /// Seconds since the route was last planned; a very long one is
    /// replanned from where the car stands (never removed).
    #[serde(default)]
    pub age: f32,
    /// A bus on a line.
    #[serde(default)]
    pub bus: Option<Bus>,
    /// Seconds left standing still (a bus at a stop).
    #[serde(default)]
    pub hold: f32,
    /// Sideways offset (metres, + to the right) from the lane centre while
    /// drifting into a new lane; decays to zero.
    #[serde(default)]
    pub lateral: f32,
    /// Seconds the car has stood still without a reason of its own (a bus
    /// dwelling at a stop is not counted).
    #[serde(default)]
    pub stalled: f32,
    /// What held the car back on the last physics step.
    #[serde(default)]
    pub wait: Wait,
    /// Seconds before the car may change lanes by choice or replan again.
    #[serde(default)]
    pub cooldown: f32,
    /// Waiting at the kerb for a gap to pull out into its first lane; not
    /// yet in the lane's list, so nobody queues behind it.
    #[serde(default)]
    pub entering: bool,
    /// The vehicle this one is waiting for (its leader, the tail of a full
    /// exit, the car it lets out of the kerb, the car covering its kerb
    /// spot), for finding wait-for cycles.
    #[serde(skip)]
    pub blocked_by: Option<CarId>,
    /// Tries at another way since it last moved (backs the next one off).
    #[serde(default)]
    pub replans: u8,
    /// Squeezing past a left turner waiting at the head of a one-lane
    /// approach, on the shoulder to its right.
    #[serde(default)]
    pub passing: bool,
    /// Seconds stood waiting before the left turn ahead (reset once made).
    #[serde(default)]
    pub turn_wait: f32,
    /// Of those, seconds at the stop line yielding to oncoming traffic.
    #[serde(default)]
    pub yielded: f32,
    /// [services] An emergency vehicle with lights and sirens: it runs red
    /// lights and passes queues, and the cars ahead of it pull aside.
    #[serde(default)]
    pub siren: bool,
    /// What it looks like (body, class, wear, livery); drawing only.
    #[serde(default)]
    pub model: crate::vehicle::VehicleModel,
    /// [driving] Who is at the wheel and what has happened to the car:
    /// driver profile, crash, takeover role, stunt, weave.
    #[serde(default)]
    pub drive: DriveState,
    /// [carlife] The private car this is (`World::vehicles`), if any.
    #[serde(default)]
    pub owned: Option<crate::vehicle::VehicleId>,
    /// [carlife] What it is doing beyond its route: looking for a space,
    /// going to be sold, a truck going back to base (`world/cars.rs`).
    #[serde(default)]
    pub errand: crate::vehicle::Errand,
    /// [tolls] The booth whose lanes the car is on (0: none), and whether
    /// it has paid there. Reset once it is on an untolled lane, so passing
    /// the same booth on a later trip pays again.
    #[serde(default)]
    pub toll_booth: u32,
    #[serde(default)]
    pub toll_paid: bool,
    /// The glide of a lane change: the sideways offset it began with
    /// (0: none under way, the offset simply settles), the metres it takes
    /// and the metres driven so far. The offset eases out and back in
    /// along a half cosine over that distance, the nose turned along it.
    #[serde(default)]
    pub glide_from: f32,
    #[serde(default)]
    pub glide_len: f32,
    #[serde(default)]
    pub glide_done: f32,
    /// [passengers] Everyone riding along besides the driver (`citizen`),
    /// kept by `World::step_passengers`.
    #[serde(default)]
    pub occupants: Vec<CitizenId>,
}

/// Why a car is not driving freely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wait {
    #[default]
    Free,
    /// Following (or queued behind) another vehicle.
    Leader,
    /// At a red light.
    Signal,
    /// Yielding at an unsignalled junction.
    Yield,
    /// The lane beyond the junction is full.
    ExitFull,
    /// At the end of its lane, waiting for a gap to change lanes.
    LaneChange,
    /// A bus standing at a stop.
    Dwell,
    /// At the kerb, waiting for a gap to pull out.
    Entering,
    /// Pulling up at the destination.
    Arriving,
    /// [driving] Stopped after a crash.
    Crashed,
    /// [driving] The junction ahead is blocked (a street takeover).
    Blocked,
    /// [rail-crossing] At a level crossing's stop line: a train is coming.
    RailCrossing,
}

impl Car {
    /// Where to draw the car a fraction `f` of the way through the last
    /// tick. A tick is several seconds of driving (`traffic_seconds_per_tick`),
    /// so a straight line from the old pose to the new one cuts every
    /// corner by metres; this follows the poses recorded at each physics
    /// substep instead, with a cubic tangent to the car's heading between
    /// them, so a car turning at a junction is drawn on its arc.
    pub fn pose_at(&self, f: f32) -> (Vec2, f32) {
        let poses = &self.drive.poses;
        let n = poses.len();
        if n < 2 {
            return (self.prev_pos.lerp(self.pos, f), lerp_angle(self.prev_heading, self.heading, f));
        }
        let x = f.clamp(0.0, 1.0) * (n - 1) as f32;
        let k = (x.floor() as usize).min(n - 2);
        let t = x - k as f32;
        let ((p0, h0), (p1, h1)) = (poses[k], poses[k + 1]);
        let d = p0.distance(p1);
        let heading = lerp_angle(h0, h1, t);
        if d > 30.0 {
            return (p1, h1);
        }
        let (m0, m1) = (Vec2::new(h0.cos(), h0.sin()) * d, Vec2::new(h1.cos(), h1.sin()) * d);
        let chord = (p1 - p0) / d.max(1e-4);
        // Hermite only where the car moves the way it points (not a donut,
        // not reversing out of a ring).
        if d < 0.05 || self.drive.stunt.is_some() || m0.dot(chord) < 0.5 * d || m1.dot(chord) < 0.5 * d {
            return (p0.lerp(p1, t), heading);
        }
        let (t2, t3) = (t * t, t * t * t);
        let p = p0 * (2.0 * t3 - 3.0 * t2 + 1.0) + m0 * (t3 - 2.0 * t2 + t) + p1 * (-2.0 * t3 + 3.0 * t2) + m1 * (t3 - t2);
        (p, heading)
    }

    /// How far (m) a long vehicle swings out at the middle of a turn.
    pub(super) fn swing(&self) -> f32 {
        use crate::vehicle::BodyType as B;
        if self.bus.is_some() {
            return 1.0;
        }
        match self.model.body {
            B::Semi | B::ArticulatedBus => 1.1,
            B::BubbleBus => 1.0,
            B::GarbageTruck | B::PanelTruck | B::BoxVan => 0.6,
            _ => 0.0,
        }
    }

    /// Stopped by a crash (minor or written off).
    pub fn crashed(&self) -> bool {
        self.drive.crash.is_some()
    }

    /// Off the lanes, in a takeover show.
    pub fn in_stunt(&self) -> bool {
        self.drive.stunt.is_some()
    }

    /// [rage] Traffic seconds the rest of the route should take in free
    /// flow: every lane at 90% of its limit, plus a few seconds per
    /// junction for lights and turns and a little for pulling out.
    pub fn expected_seconds(&self, net: &LaneNetwork) -> f32 {
        let mut t = 15.0;
        for (k, &l) in self.route.iter().enumerate().skip(self.idx) {
            let Some(lane) = net.lanes.get(l as usize) else { continue };
            let len = if k == self.idx { (lane.length - self.s).max(0.0) } else { lane.length };
            t += len / (lane.speed_limit * 0.9).max(2.0);
            if lane.is_connector() {
                t += 10.0;
            }
        }
        t
    }

    pub fn length(&self) -> f32 {
        if self.bus.is_some() { BUS_LENGTH } else { CAR_LENGTH }
    }

    /// Desired speed on a lane with this limit.
    pub(super) fn desired_speed(&self, limit: f32) -> f32 {
        if self.siren {
            return limit * if self.drive.chasing.is_some() { PURSUIT_SPEED_FACTOR } else { SIREN_SPEED_FACTOR };
        }
        let base = if self.cargo.is_some() { limit * TRUCK_SPEED_FACTOR } else { limit };
        base * self.drive.profile.speed
    }

    /// IDM acceleration with this driver's headway and acceleration.
    pub(super) fn idm(&self, v0: f32, gap: f32, dv: f32) -> f32 {
        let p = &self.drive.profile;
        idm_with(self.v, v0, gap, dv, p.headway, p.accel)
    }

    /// The sideways offset a glide under way puts the car at now.
    pub(super) fn glide_at(&self) -> f32 {
        let u = (self.glide_done / self.glide_len.max(0.1)).min(1.0);
        self.glide_from * 0.5 * (1.0 + (std::f32::consts::PI * u).cos())
    }

    /// The route asks for a change into the lane beside this one next.
    pub(super) fn change_pending(&self, net: &LaneNetwork) -> Option<LaneId> {
        let next = *self.route.get(self.idx + 1)?;
        net.lanes[self.route[self.idx] as usize].is_beside(next).then_some(next)
    }
}

/// What a bus is doing on its line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bus {
    pub route: crate::transit::RouteId,
    /// Stop the bus is driving to; `usize::MAX` when heading for the depot.
    pub next_stop: usize,
    pub passengers: Vec<CitizenId>,
    /// Finish the loop at the next stop and go home.
    pub to_depot: bool,
    pub depot: crate::buildings::BuildingId,
}
