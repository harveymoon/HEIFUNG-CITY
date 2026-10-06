//! Air transport, the data: drone taxis for the rich, air ambulances and
//! police helicopters off the helipads of the big stations, and the pads
//! they use. The systems live in `world/air.rs`.
//!
//! - **Pads.** A vertiport is placed by the player (Buildings tool, Transit:
//!   a drone pad, a vertiport or a skyport, 2 / 4 / 6 landing slots). Towers
//!   of the high tier on very dear land grow a rooftop pad by themselves
//!   (one slot). High-tier hospitals, fire and police stations already have
//!   a helipad on the roof.
//! - **Drone taxis.** The air taxi company keeps one drone per vertiport
//!   slot but one at each vertiport. Rich citizens fly when a pad is in
//!   walking reach of both ends of a long trip: walk to the pad, wait, a
//!   drone comes, they pay a steep fare, it climbs to cruise height, flies
//!   straight there (no roads), lands, and they walk the rest. A pad takes
//!   as many drones at once as it has slots; the rest hold in a circle and
//!   land in the order they arrived. Every flight drains the battery; a
//!   drone goes home to charge before it runs low.
//! - **Air ambulances.** A critical patient far from any hospital is
//!   flown: the helicopter lands on the roof (if there is a pad) or in the
//!   street, the crew works, then flies the patient to a hospital pad.
//! - **Police helicopters** circle crimes at night with a searchlight.
//! - **Noise.** Low and busy flying is logged on a coarse grid that takes a
//!   little off land value; people on the street who are not rich grumble.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::CitizenId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::{BTreeMap, HashMap};

new_key_type! {
    pub struct DroneId;
}

// ── balance ────────────────────────────────────────────────────────────────

/// Cruise heights above the ground, metres: taxis over the towers,
/// helicopters a little lower.
pub const TAXI_ALT: f32 = 150.0;
pub const HELI_ALT: f32 = 115.0;
/// Air speeds, metres per real second (sim motion runs at
/// `traffic_seconds_per_tick` seconds per tick, like cars).
pub const TAXI_MPS: f32 = 45.0;
pub const HELI_MPS: f32 = 55.0;
/// Climb and descent rate.
pub const VERTICAL_MPS: f32 = 12.0;
/// Within this of its pad a drone asks for a landing slot.
pub const APPROACH: f32 = 70.0;
/// Radius of the holding circle round a busy pad.
pub const HOLD_RADIUS: f32 = 55.0;
/// Ticks on the pad while a rider gets in or out.
pub const BOARD_TICKS: u64 = 2;
/// Ticks at least between landing at base and the next take-off.
pub const TURNAROUND: u64 = 3;
/// Walk at most this far to a pad, at each end.
pub const REACH: f32 = 170.0;
/// Trips shorter than this (straight line) are not flown.
pub const MIN_TRIP: f32 = 400.0;
/// Fare: a flag fall plus a rate per kilometre flown.
pub const FARE_BASE: f32 = 20.0;
pub const FARE_PER_KM: f32 = 16.0;
/// The city's levy on every fare (booked as business tax).
pub const LEVY: f32 = 0.15;
/// A rider gives up on a drone that has not been assigned in this many ticks.
pub const WAIT_GIVE_UP: u64 = 40;
/// Battery used per kilometre flown and per metre climbed; charge per tick
/// on the stand at base; the reserve every job must leave.
pub const BATTERY_PER_KM: f32 = 0.06;
pub const BATTERY_PER_CLIMB_M: f32 = 0.0004;
pub const CHARGE_PER_TICK: f32 = 0.03;
pub const RESERVE: f32 = 0.2;
/// Drones fly from 05:00 until 02:00.
pub const SERVICE_START: u32 = 5 * 60;
pub const SERVICE_END: u32 = 2 * 60;
/// A tower grows a rooftop pad on land this dear, and loses it below `ROOFTOP_KEEP`.
pub const ROOFTOP_LAND_VALUE: f32 = 0.8;
pub const ROOFTOP_KEEP: f32 = 0.72;
pub const ROOFTOP_MIN_FLOORS: u8 = 9;
/// Rich adults (own residents in full, neighbours within reach at half)
/// a tower needs before it grows a pad; and roughly how many towers share one.
pub const ROOFTOP_DEMAND: f32 = 8.0;
pub const ROOFTOP_DEMAND_REACH: f32 = 150.0;
pub const ROOFTOP_TOWERS_PER_PAD: f32 = 9.0;
/// A patient this far (straight line) from the nearest hospital is flown,
/// if critical.
pub const AMBULANCE_MIN_DIST: f32 = 500.0;
/// Ticks the air medics work on scene.
pub const MEDIC_WORK: u64 = 10;
/// Company running costs per drone per day.
pub const DRONE_COST_PER_DAY: f32 = 55.0;
/// Noise grid cell, metres; and the land value it takes at most.
pub const NOISE_CELL: f32 = 64.0;
pub const NOISE_MAX_PENALTY: f32 = 0.06;

/// Landing slots of a vertiport by tier.
pub fn vertiport_slots(tier: Tier) -> u8 {
    [2, 4, 6][tier.index()]
}

/// Height of a vertiport's landing deck by tier (the procgen style draws it there).
pub fn vertiport_deck(tier: Tier) -> f32 {
    [0.4, 7.0, 15.0][tier.index()]
}

/// Taxi fare for a flight of `metres`.
pub fn fare(metres: f32) -> f32 {
    FARE_BASE + FARE_PER_KM * metres / 1000.0
}

/// Ticks a flight takes: climb from `h0`, cruise `metres`, descend to `h1`,
/// at `tsp` seconds per tick.
pub fn flight_ticks(metres: f32, h0: f32, h1: f32, cruise: f32, mps: f32, tsp: f32) -> f32 {
    let climb = (cruise - h0).max(0.0) / (VERTICAL_MPS * tsp);
    let fall = (cruise - h1).max(0.0) / (VERTICAL_MPS * tsp);
    climb + fall + metres / (mps * tsp)
}

// ── pads ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PadKind {
    /// Player-placed drone pad, vertiport or skyport: taxis' base.
    Vertiport,
    /// A dear tower's own pad: taxis pick up and drop off.
    Rooftop,
    /// High-tier hospital: air ambulances base and deliver here.
    Hospital,
    /// Fire headquarters: a rescue helicopter (flies as an air ambulance).
    Fire,
    /// Police headquarters: the police helicopter.
    Police,
}

impl PadKind {
    pub fn label(self) -> &'static str {
        match self {
            PadKind::Vertiport => "vertiport",
            PadKind::Rooftop => "rooftop pad",
            PadKind::Hospital => "hospital helipad",
            PadKind::Fire => "fire helipad",
            PadKind::Police => "police helipad",
        }
    }

    /// Taxis use vertiports and rooftop pads.
    pub fn for_taxis(self) -> bool {
        matches!(self, PadKind::Vertiport | PadKind::Rooftop)
    }
}

/// A place to land: derived from the buildings (`World::refresh_pads`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub building: BuildingId,
    pub kind: PadKind,
    pub pos: Vec2,
    /// Height of the deck above the lot, as the sim reckons it (the renderer
    /// uses the drawn pad's real height).
    pub height: f32,
    pub slots: u8,
}

// ── aircraft ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Craft {
    /// The air taxi company's eVTOL drone.
    Taxi,
    /// A hospital's (or the fire HQ's) helicopter.
    AirAmbulance,
    PoliceHeli,
}

impl Craft {
    pub fn label(self) -> &'static str {
        match self {
            Craft::Taxi => "drone taxi",
            Craft::AirAmbulance => "air ambulance",
            Craft::PoliceHeli => "police helicopter",
        }
    }

    pub fn cruise(self) -> f32 {
        match self {
            Craft::Taxi => TAXI_ALT,
            _ => HELI_ALT,
        }
    }

    pub fn mps(self) -> f32 {
        match self {
            Craft::Taxi => TAXI_MPS,
            _ => HELI_MPS,
        }
    }

    pub fn call_sign(self) -> &'static str {
        match self {
            Craft::Taxi => "Sky",
            Craft::AirAmbulance => "Air Medic",
            Craft::PoliceHeli => "Air",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// On the ground (a pad or a street), engines idle; charging at base.
    Parked,
    /// On a pad with the doors open.
    Boarding,
    Climbing,
    Cruising,
    /// Circling a busy pad, waiting for a slot.
    Holding,
    Descending,
    /// Police: circling a scene.
    Orbiting,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Parked => "parked",
            Phase::Boarding => "boarding",
            Phase::Climbing => "taking off",
            Phase::Cruising => "cruising",
            Phase::Holding => "holding",
            Phase::Descending => "landing",
            Phase::Orbiting => "circling",
        }
    }

    pub fn airborne(self) -> bool {
        !matches!(self, Phase::Parked | Phase::Boarding)
    }
}

/// What an aircraft is doing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Job {
    Idle,
    /// Back to base to park and charge.
    Home,
    /// Pick `rider` up at pad `from` and fly them to pad `to`.
    Taxi { rider: CitizenId, from: BuildingId, to: BuildingId, aboard: bool },
    /// Air ambulance: fly to the patient, work until `work_until`, fly them
    /// to `hospital`.
    Medic { incident: u32, patient: CitizenId, hospital: Option<BuildingId>, aboard: bool, work_until: u64 },
    /// Police: circle the scene of `incident` until it is over.
    Patrol { incident: u32 },
}

impl Job {
    pub fn label(&self) -> &'static str {
        match self {
            Job::Idle => "idle",
            Job::Home => "returning to base",
            Job::Taxi { aboard: false, .. } => "on the way to a pickup",
            Job::Taxi { aboard: true, .. } => "carrying a passenger",
            Job::Medic { aboard: false, .. } => "flying to a patient",
            Job::Medic { aboard: true, .. } => "flying a patient to hospital",
            Job::Patrol { .. } => "over a crime scene",
        }
    }
}

/// One drone or helicopter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Drone {
    pub craft: Craft,
    /// Number at its base, from 1.
    pub number: u16,
    /// The pad building it belongs to.
    pub base: BuildingId,
    pub pos: Vec2,
    /// Height above the ground under it.
    pub alt: f32,
    pub heading: f32,
    pub prev_pos: Vec2,
    pub prev_alt: f32,
    pub prev_heading: f32,
    pub phase: Phase,
    /// Ticks the current ground phase lasts until (boarding, turnaround).
    pub until: u64,
    /// The pad it is flying to, if a pad (else a street landing at `goal`).
    pub dest: Option<BuildingId>,
    pub goal: Vec2,
    /// Height to land at: a pad's deck, or 0 in the street.
    pub goal_alt: f32,
    /// The pad whose slot it holds (descending, on it, or climbing off it).
    pub at: Option<BuildingId>,
    /// Which landing slot of that pad.
    pub slot: u8,
    /// Tick it started holding (first come, first landed).
    pub hold_since: u64,
    /// 0..1.
    pub battery: f32,
    pub job: Job,
    pub flights_today: u32,
    pub km_today: f32,
    pub flights_total: u32,
}

impl Drone {
    pub fn new(craft: Craft, number: u16, base: BuildingId, pad: &Pad) -> Self {
        Self {
            craft,
            number,
            base,
            pos: pad.pos,
            alt: pad.height,
            heading: 0.0,
            prev_pos: pad.pos,
            prev_alt: pad.height,
            prev_heading: 0.0,
            phase: Phase::Parked,
            until: 0,
            dest: Some(base),
            goal: pad.pos,
            goal_alt: pad.height,
            at: Some(base),
            slot: 0,
            hold_since: 0,
            battery: 1.0,
            job: Job::Idle,
            flights_today: 0,
            km_today: 0.0,
            flights_total: 0,
        }
    }

    /// Searchlight, beacons and strobes: anything in the air at night, the
    /// police helicopter's light over a scene.
    pub fn searching(&self) -> bool {
        self.craft == Craft::PoliceHeli && self.phase == Phase::Orbiting
    }

    /// The passenger or patient aboard, if any.
    pub fn aboard(&self) -> Option<CitizenId> {
        match self.job {
            Job::Taxi { rider, aboard: true, .. } => Some(rider),
            Job::Medic { patient, aboard: true, .. } => Some(patient),
            _ => None,
        }
    }
}

/// The air taxi company: a private business with its own books.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AirCompany {
    pub name: String,
    pub money: f32,
    pub revenue_today: f32,
    pub revenue_yesterday: f32,
    pub costs_today: f32,
}

impl Default for AirCompany {
    fn default() -> Self {
        Self { name: "Halo Skyways".into(), money: 5_000.0, revenue_today: 0.0, revenue_yesterday: 0.0, costs_today: 0.0 }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AirStats {
    pub flights_today: u32,
    pub riders_today: u32,
    pub fares_today: f32,
    pub flights_yesterday: u32,
    pub riders_yesterday: u32,
    pub fares_yesterday: f32,
    pub riders_total: u32,
    /// Riders who gave up waiting for a drone today.
    pub gave_up_today: u32,
    /// Times a drone had to hold for a slot today.
    pub holds_today: u32,
    pub medic_flights_today: u32,
    pub medic_flights_total: u32,
    pub police_flights_today: u32,
    /// Grumbles from the street today.
    pub grumbles_today: u32,
}

/// Everything the air system owns.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Air {
    pub drones: SlotMap<DroneId, Drone>,
    /// Towers with a rooftop pad (kept while the land stays dear).
    pub rooftops: Vec<BuildingId>,
    /// Riders who set off for a pad and have no drone yet, first come first served.
    pub pending: Vec<CitizenId>,
    pub company: AirCompany,
    pub stats: AirStats,
    /// Flying logged per noise cell (`NOISE_CELL`), halved every midnight.
    pub noise: BTreeMap<(i32, i32), f32>,
    /// Pads, rebuilt from the buildings every few ticks.
    #[serde(skip)]
    pub pads: Vec<Pad>,
    #[serde(skip)]
    pub pads_key: u64,
    /// Last tick each citizen grumbled about a drone, so once a day at most.
    #[serde(skip)]
    pub grumbled: HashMap<CitizenId, u64>,
    #[serde(skip)]
    pub rng: Option<crate::SimRng>,
}

impl Air {
    pub fn pad(&self, b: BuildingId) -> Option<&Pad> {
        self.pads.iter().find(|p| p.building == b)
    }

    pub fn has_rooftop(&self, b: BuildingId) -> bool {
        self.rooftops.contains(&b)
    }

    /// Is the building a vertiport, a rooftop or a helipad?
    pub fn is_pad(&self, b: BuildingId) -> bool {
        self.pads.iter().any(|p| p.building == b)
    }

    /// Drones holding a slot of pad `b`.
    pub fn slots_used(&self, b: BuildingId) -> usize {
        self.drones.values().filter(|d| d.at == Some(b)).count()
    }

    /// The free slot index at `b`, if any.
    pub fn free_slot(&self, b: BuildingId) -> Option<u8> {
        let pad = self.pad(b)?;
        (0..pad.slots).find(|s| !self.drones.values().any(|d| d.at == Some(b) && d.slot == *s))
    }

    /// The drone a citizen is flying in or waiting for.
    pub fn drone_for(&self, c: CitizenId) -> Option<DroneId> {
        self.drones.iter().find(|(_, d)| matches!(d.job, Job::Taxi { rider, .. } if rider == c) || matches!(d.job, Job::Medic { patient, aboard: true, .. } if patient == c)).map(|(id, _)| id)
    }

    pub fn taxis(&self) -> usize {
        self.drones.values().filter(|d| d.craft == Craft::Taxi).count()
    }

    /// Noise logged at a point, 0 when quiet.
    pub fn noise_at(&self, p: Vec2) -> f32 {
        self.noise.get(&noise_cell(p)).copied().unwrap_or(0.0)
    }
}

pub fn noise_cell(p: Vec2) -> (i32, i32) {
    ((p.x / NOISE_CELL).floor() as i32, (p.y / NOISE_CELL).floor() as i32)
}

pub fn noise_cell_center(c: (i32, i32)) -> Vec2 {
    Vec2::new((c.0 as f32 + 0.5) * NOISE_CELL, (c.1 as f32 + 0.5) * NOISE_CELL)
}

/// Land value taken off by `noise` logged in a cell.
pub fn noise_penalty(noise: f32) -> f32 {
    (0.004 * noise.max(0.0).sqrt()).min(NOISE_MAX_PENALTY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fares_rise_with_distance_and_flights_beat_driving_long_trips() {
        assert!(fare(2000.0) > fare(1000.0) && fare(1000.0) > FARE_BASE);
        // A 2 km flight from a ground pad to a tower roof against a car at
        // an average 9 m/s over a 1.3x longer road route.
        let air = flight_ticks(2000.0, 0.4, 40.0, TAXI_ALT, TAXI_MPS, 4.0);
        let road = 2000.0 * 1.3 / (9.0 * 4.0);
        assert!(air < road * 0.6, "air {air} ticks, road {road}");
    }

    #[test]
    fn noise_penalty_is_small_and_capped() {
        assert_eq!(noise_penalty(0.0), 0.0);
        assert!(noise_penalty(25.0) > 0.0 && noise_penalty(25.0) < 0.03);
        assert_eq!(noise_penalty(1e9), NOISE_MAX_PENALTY);
    }

    #[test]
    fn slots_are_counted_per_pad() {
        let mut air = Air::default();
        let b = BuildingId::default();
        air.pads.push(Pad { building: b, kind: PadKind::Rooftop, pos: Vec2::ZERO, height: 30.0, slots: 1 });
        assert_eq!(air.free_slot(b), Some(0));
        let pad = air.pads[0];
        air.drones.insert(Drone::new(Craft::Taxi, 1, b, &pad));
        assert_eq!(air.free_slot(b), None, "one drone fills a rooftop pad");
        assert_eq!(air.slots_used(b), 1);
    }
}
