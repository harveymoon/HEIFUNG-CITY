//! Drivers: profiles (age, caution, drink, temper), crash state and causes,
//! gang roles and stunts, and road rage.

use super::*;

/// How a driver drives: chosen from age, personality and drunkenness when
/// the car sets off (`World::step_driving`). The default, unassigned
/// profile drives exactly like the plain model: no lag, no weave, no
/// crashes, every light obeyed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriverProfile {
    pub assigned: bool,
    /// Desired speed as a share of the limit.
    pub speed: f32,
    /// IDM time headway (s) and acceleration (m/s per s).
    pub headway: f32,
    pub accel: f32,
    /// Seconds to notice the car ahead changing speed.
    pub reaction: f32,
    /// `0..1`: how readily the driver stops for an amber or a red.
    pub compliance: f32,
    /// `0..1`: how eagerly they change lanes and overtake (0.5 is the plain model).
    pub aggression: f32,
    /// `0..1`: lane keeping and judgement. Low skill wanders and misjudges.
    pub skill: f32,
    /// `0..1`, copied from the driver when the trip started.
    pub drunk: f32,
    /// A takeover pack: lights mean nothing.
    pub ignores_signals: bool,
    /// Stable phase for the weave, so cars do not wander in step.
    pub phase: f32,
}

impl Default for DriverProfile {
    fn default() -> Self {
        Self { assigned: false, speed: 1.0, headway: IDM_HEADWAY, accel: IDM_ACCEL, reaction: 0.0, compliance: 1.0, aggression: 0.5, skill: 1.0, drunk: 0.0, ignores_signals: false, phase: 0.0 }
    }
}

impl DriverProfile {
    /// A private driver. `caution` and `hedonism` are personality traits
    /// (`nd_minds::Personality`), `knack` a natural talent `0..1`.
    /// Rebellious (careless, pleasure-seeking) drivers go faster, closer and
    /// through late ambers; the old drive slower with slower reactions;
    /// the young are quick but inexperienced; drink makes everything worse.
    pub fn for_driver(age: u8, caution: f32, hedonism: f32, drunk: f32, knack: f32, phase: f32) -> Self {
        let r = crate::citizens::rebelliousness(caution, hedonism);
        let age = age as f32;
        let old = ((age - 65.0) / 20.0).clamp(0.0, 1.0);
        let young = ((25.0 - age) / 7.0).clamp(0.0, 1.0);
        let d = drunk.clamp(0.0, 1.0);
        Self {
            assigned: true,
            speed: (0.9 + 0.22 * r + 0.06 * young - 0.22 * old + 0.1 * d).clamp(0.7, 1.3),
            headway: (1.7 - 1.0 * r - 0.2 * young + 0.6 * old - 0.3 * d).clamp(0.6, 2.4),
            accel: (1.1 + 0.8 * r - 0.4 * old).clamp(0.8, 2.2),
            reaction: 0.6 + 0.7 * old + 1.6 * d,
            compliance: (1.0 - 0.55 * r * r - 0.1 * young - 0.5 * d).clamp(0.2, 1.0),
            aggression: (0.25 + 0.7 * r + 0.1 * young).clamp(0.0, 1.0),
            skill: (0.6 + 0.35 * knack + 0.1 * (1.0 - young) - 0.25 * old - 0.6 * d).clamp(0.05, 1.0),
            drunk: d,
            ignores_signals: false,
            phase,
        }
    }

    /// Bus, truck and emergency drivers: steady and by the book.
    pub fn professional(phase: f32) -> Self {
        Self { assigned: true, speed: 1.0, headway: 1.4, accel: 1.3, reaction: 0.6, compliance: 0.98, aggression: 0.4, skill: 0.95, drunk: 0.0, ignores_signals: false, phase }
    }

    /// A takeover crew member in the pack: fast, close, weaving, no lights.
    pub fn pack(mut self) -> Self {
        self.speed = self.speed.max(1.3);
        self.headway = self.headway.min(0.6);
        self.accel = self.accel.max(2.2);
        self.aggression = 1.0;
        self.compliance = 0.0;
        self.ignores_signals = true;
        self.skill = self.skill.max(0.7);
        self
    }

    /// [events] A street racer flat out: faster than a takeover pack, hard
    /// on the throttle, no lights, still able to drive.
    pub fn racing(mut self) -> Self {
        self = self.pack();
        self.speed = 1.55;
        self.accel = 3.0;
        self.headway = 0.5;
        self.skill = self.skill.max(0.8);
        self
    }

    /// [rage] This driver with `anger` (`0..1`) on top: closer to the car
    /// ahead, harder on the throttle, quicker to change lanes and overtake,
    /// a little faster, less inclined to stop for an amber, and sloppier
    /// (a slightly higher chance of misjudging a gap and crashing).
    pub fn angry(mut self, anger: f32) -> Self {
        let a = anger.clamp(0.0, 1.0);
        if a <= 0.0 || !self.assigned {
            return self;
        }
        self.headway = (self.headway * (1.0 - 0.45 * a)).max(0.5);
        self.accel = (self.accel * (1.0 + 0.5 * a)).min(2.6);
        self.aggression = (self.aggression + 0.4 * a).min(1.0);
        self.speed = (self.speed * (1.0 + 0.1 * a)).min(1.35);
        self.compliance = (self.compliance - 0.3 * a).max(0.1);
        self.skill = (self.skill - 0.12 * a).max(0.05);
        self
    }

    /// Sideways wander (m) around the lane centre.
    pub fn weave_amplitude(&self) -> f32 {
        if !self.assigned {
            return 0.0;
        }
        (1.0 - self.skill) * 0.35 + self.drunk * 1.3 + if self.ignores_signals { 0.45 } else { 0.0 }
    }

    /// Where the driver sits between slow (`-1`) and reckless (`1`), `0`
    /// an ordinary driver: desired speed, eagerness to overtake, how
    /// readily they stop for an amber, and how close they follow, each
    /// measured from a by-the-book driver and averaged. The inspector's
    /// slow–normal–reckless slider. Drink pushes it up through those.
    pub fn boldness(&self) -> f32 {
        if !self.assigned {
            return 0.0;
        }
        let speed = (self.speed - 1.0) / 0.3;
        let aggression = (self.aggression - 0.45) / 0.55;
        let ambers = (0.85 - self.compliance) / 0.65;
        let tailgating = (1.5 - self.headway) / 0.9;
        ((speed + aggression + ambers + tailgating) / 4.0).clamp(-1.0, 1.0)
    }

    pub fn label(&self) -> &'static str {
        if !self.assigned {
            "unassigned"
        } else if self.ignores_signals {
            "takeover crew"
        } else if self.drunk >= 0.35 {
            "drunk"
        } else if self.aggression > 0.72 {
            "reckless"
        } else if self.speed < 0.86 {
            "slow and careful"
        } else if self.skill < 0.55 {
            "sloppy"
        } else {
            "ordinary"
        }
    }
}

/// A car stopped by a crash.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CrashState {
    /// `CrashReport::id`.
    pub id: u32,
    /// Seconds before a minor crash drives on.
    pub hold: f32,
    /// Written off: stays until the scene is cleared and it is towed.
    pub wrecked: bool,
}

/// Where a takeover car is in the night.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GangPhase {
    /// Driving in to the meet.
    Joining,
    /// In the show at the junction (a stunt).
    Show,
    /// Leaving in convoy.
    Leaving,
    /// [events] Flat out along a street race's route; held at its end (the
    /// finish) for the party instead of parking.
    Racing,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GangRole {
    /// The takeover's id, or the race's (`race`).
    pub takeover: u32,
    pub phase: GangPhase,
    /// [events] A street racer (`crate::street_events`), not a takeover crew.
    #[serde(default)]
    pub race: bool,
    /// [moto] A rider in a moto pack (`World::start_moto_pack`).
    #[serde(default)]
    pub pack: bool,
}

/// What a car in a takeover show does, off the lanes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum StuntKind {
    /// Parked in the ring, nose to `face`, blocking the junction.
    Ring { at: Vec2, face: f32 },
    /// Spinning round `centre` with the tail out; `dir` +1 or -1.
    Donut { centre: Vec2, radius: f32, dir: f32 },
    /// [scene] A slow move off the lanes at work (a tow truck against a
    /// one-way street, or turning round): to `to` at `speed`, then on along
    /// `DriveState::path`, and at the end round to face `face`
    /// (`traffic/scene.rs`).
    Manoeuvre { to: Vec2, face: f32, speed: f32 },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Stunt {
    pub kind: StuntKind,
    /// Angle round a donut.
    pub angle: f32,
    /// Reached its spot.
    pub settled: bool,
}

impl Stunt {
    pub fn is_donut(&self) -> bool {
        matches!(self.kind, StuntKind::Donut { .. })
    }
}

/// [driving] Per-car state of the driving model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DriveState {
    pub profile: DriverProfile,
    pub crash: Option<CrashState>,
    pub gang: Option<GangRole>,
    pub stunt: Option<Stunt>,
    /// Sideways wander (m, + right) drawn on top of `lateral`.
    pub weave: f32,
    /// Leader speed as the driver perceives it (lags by the reaction time).
    pub seen_lead_v: f32,
    /// Committed to running the red on this connector.
    pub running: Option<LaneId>,
    /// The lane it was on at the last substep, and seconds on it: a car
    /// that has just merged or changed lanes is not yet "following".
    pub lane: Option<LaneId>,
    pub on_lane: f32,
    /// Poses at the end of every physics substep of the last tick, the
    /// tick's start first, for drawing the car along its real path.
    #[serde(skip)]
    pub poses: Vec<(Vec2, f32)>,
    /// [rage] The drive under way: the tick it set off and the minutes the
    /// route should have taken, judged at departure (0: not timed yet;
    /// `World::step_rage` times every car the tick after it appears).
    pub trip_start: u64,
    pub expected_min: f32,
    /// [rage] The driver's own profile before any anger, kept so anger can
    /// be applied on top of it. `None`: a professional or a takeover pack,
    /// who do not get road rage.
    pub calm: Option<DriverProfile>,
    /// [rage] Patience and temper `0..1`, from the driver's personality.
    pub patience: f32,
    pub temper: f32,
    /// [rage] How angry the driver is now, `0..1`, and the worst it got
    /// this trip (fed into how the trip felt).
    pub anger: f32,
    pub anger_peak: f32,
    /// [bridge-riders] The anger is a street racer's who lost, not a late
    /// driver's: sore on the drive home (timed afresh from the race).
    pub sore_loser: bool,
    /// [emergency] The move of a vehicle driving off-law (lights and
    /// sirens, or fleeing them): round a queue, on the kerb, through a red
    /// (`traffic/siren.rs`). Planned afresh every step.
    #[serde(skip)]
    pub offlaw: OffLaw,
    /// [emergency] A suspect fleeing the police in this car: reckless,
    /// off-law, never pulls over (`world/chases.rs`).
    pub fleeing: bool,
    /// [emergency] A police car in pursuit: the car it is chasing. It drives
    /// at it rather than round it; touching it is the tag.
    pub chasing: Option<CarId>,
    /// [services] A recovery vehicle (a tow truck going to a wreck) with its
    /// amber beacons on: drawn flashing, but nobody pulls over for it.
    pub amber: bool,
    /// [services] That vehicle is close to the wreck: it may take the
    /// off-law moves (`traffic/siren.rs`) round the queue in its way, but
    /// not run red lights. Set by the world every tick.
    pub recovering: bool,
    /// [scene] The waypoints of a manoeuvre still to come after the one
    /// it is driving to (`StuntKind::Manoeuvre`).
    pub path: Vec<Vec2>,
    /// [scene] Sideways ease (m, + right) round a scene at the roadside
    /// (`traffic/scene.rs`), drawn on top of the lane.
    #[serde(skip)]
    pub dodge: f32,
}

/// [rage] How angry (`0..1`) a driver gets `elapsed` minutes into a drive
/// that should have taken `expected`. Anger starts once the drive runs
/// past `expected * (1.8 + 1.6 * patience - 0.4 * temper)` plus five
/// minutes (so a short hop doubling, or the usual rush-hour crawl, is
/// shrugged off), builds over a stretch as long again as the trip scaled
/// by patience (at least twelve minutes), and tops out lower for the
/// even-tempered. Tuned on the demo district: at the evening peak about
/// one driver in ten running late gets properly angry, not a third. Pure and
/// deterministic: the same trip and person always give the same anger.
pub fn road_rage(elapsed: f32, expected: f32, patience: f32, temper: f32) -> f32 {
    if expected <= 0.0 || elapsed <= expected {
        return 0.0;
    }
    let (patience, temper) = (patience.clamp(0.0, 1.0), temper.clamp(0.0, 1.0));
    let start = expected * (1.8 + 1.6 * patience - 0.4 * temper) + 5.0;
    let ramp = (expected * (1.0 + patience)).max(12.0);
    ((elapsed - start) / ramp).clamp(0.0, 1.0) * (0.55 + 0.45 * temper)
}

/// Why two cars (or one) crashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrashCause {
    /// Into the back of the car ahead.
    RearEnd,
    /// Ran a red into cross traffic.
    RedLight,
    /// Weaved off the road (into a pole, the kerb, a parked car).
    OffRoad,
    /// Drifted over the line into the car alongside.
    Sideswipe,
}

impl CrashCause {
    pub fn label(self) -> &'static str {
        match self {
            CrashCause::RearEnd => "rear-end",
            CrashCause::RedLight => "ran a red light",
            CrashCause::OffRoad => "ran off the road",
            CrashCause::Sideswipe => "sideswipe",
        }
    }
}

/// A crash the traffic model decided on this tick, for the world to turn
/// into injuries, 911 calls and tow trucks.
#[derive(Clone, Debug)]
pub struct CrashReport {
    pub id: u32,
    /// The car at fault first.
    pub cars: Vec<CarId>,
    pub pos: Vec2,
    /// Closing speed, m/s.
    pub speed: f32,
    pub cause: CrashCause,
    /// `0..1`; from `BAD_CRASH` up cars are written off and people may be hurt.
    pub severity: f32,
}

/// Severity at which a crash writes cars off and may hurt people.
pub const BAD_CRASH: f32 = 0.4;

/// Hardest a profiled driver can brake (m/s per s): physics, not choice.
pub(super) const MAX_BRAKE: f32 = 8.0;

/// Seconds of a red that are still amber.
pub(super) const AMBER: f32 = 2.0;

/// Closing speed (m/s) below which touching bumpers is no crash.
pub(super) const BUMP_SPEED: f32 = 1.5;
