//! [driving] Who is driving and what happens on the road: drinks at venues,
//! driver profiles from age, personality and drink, crashes (injuries, the
//! 911 call, police and ambulance, the tow truck) and street takeovers.
//!
//! The physics lives in `traffic.rs` (`DriverProfile`, crashes, stunts);
//! takeover data in `takeover.rs`. This file is the glue that reads and
//! writes the rest of the world. It draws from its own random streams
//! (`rng_for`), with one exception: a recovery truck sent in from the map
//! edge (`send_recovery`) is routed by `route_from_edge_to`, which
//! shuffles the world's stream.

use super::*;
use crate::citizens::{driver_traits, rebelliousness, DRUNK};
use crate::services::{CrimeKind, Outcome, Stage as IncidentStage};
use crate::takeover::{self, Stage, Takeover, Takeovers};

use crate::traffic::{CrashCause, CrashReport, DriverProfile, GangPhase, GangRole, LaneKind, StuntKind, BAD_CRASH};
use crate::vehicle::{BodyType, Errand, Livery, VehicleClass, VehicleModel};

mod drivers;
mod wrecks;
mod takeovers;
#[cfg(test)]
mod tests;

/// Drunkenness gained per tick at a venue (times `hedonism - 0.3`: the
/// sensible have a soda), and lost per tick everywhere else: a hedonist is
/// well gone after a couple of hours out and sober again by morning.
pub const DRINK_RATE: f32 = 0.004;
pub const SOBER_RATE: f32 = 0.0022;
/// Crash records kept for the API.
const KEEP_CRASHES: usize = 60;
/// Ticks before a wreck nobody reported is towed anyway.
const TOW_UNREPORTED: u64 = 45;
/// [services] Minutes between tries to tow a wreck no road leads away from.
const TOW_RETRY: u64 = 15;
/// [services] A wreck that no road has led away from for this long (a road
/// cut off from the rest of the network) is lifted off by crane at last:
/// the one place a vehicle still leaves the world mid-street, because the
/// alternative is a road blocked for good.
const TOW_GIVE_UP: u64 = 1440;
/// [services] A recovery truck not at its wreck this long after it was sent
/// (minutes; longer, up to `TOW_ARRIVE_CAP`, when its route should take
/// more than half of it: one from out of town can have kilometres of
/// highway to come), or standing in a jam `TOW_STALL_MIN` minutes,
/// is given up on: the wreck is hooked where it lies by a truck from the
/// far side (`tow`), and the late one turns back. A wreck must never stand
/// for hours waiting on one truck stuck in a jam.
const TOW_ARRIVE_MAX: u64 = 60;
const TOW_ARRIVE_CAP: u64 = 150;
const TOW_STALL_MIN: f32 = 20.0;
/// [services] Within this distance (m) of its wreck, or this far along its
/// route from it (`RECOVERY_ROUTE`: a wreck reached by a loop round the
/// block is near as the crow flies but not by road), a recovery truck may
/// leave the lane to get there (`Car::offlaw`): round the queue on the
/// opposing lane or along the kerb.
const RECOVERY_REACH: f32 = 250.0;
const RECOVERY_ROUTE: f32 = 1000.0;
/// [services] How near (m) the truck must stop to hook the wreck.
const HOOK_REACH: f32 = 30.0;
/// [services] A crash whose call stays open this long (minutes: its police
/// unit gone, say) has its tow called anyway; it used to wait for ever.
const TOW_CALL_MAX: u64 = 120;
/// [driving] Minutes a recovery crew takes to right a flipped car before
/// hooking it.
pub const RIGHTING_MIN: u64 = 6;

/// A written-off car waiting for the scene to clear.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wreck {
    pub car: CarId,
    pub crash: u32,
    pub incident: Option<u32>,
    pub since: u64,
    /// [services] No road led from the wreck to a yard or off the map when
    /// the tow was due: the next try is not before this tick.
    #[serde(default)]
    pub retry_at: u64,
    /// [services] When the first tow was due (0: not yet).
    #[serde(default)]
    pub due: u64,
    /// [services] The recovery truck on its way (`Errand::Recover`), the
    /// tick it was sent and the tow company it is from (`None`: out of
    /// town, in from the map edge).
    #[serde(default)]
    pub truck: Option<CarId>,
    #[serde(default)]
    pub sent: u64,
    #[serde(default)]
    pub company: Option<BuildingId>,
    /// [driving] A bad crash can flip the car: 0 on its wheels, 1 on its
    /// side, 2 on its roof (drawn so). The recovery truck rights it before
    /// hooking it: `righted_at` is the tick it will be back on its wheels
    /// (0: not started).
    #[serde(default)]
    pub flipped: u8,
    #[serde(default)]
    pub righted_at: u64,
}

/// A tow truck hauling a wreck to the yard (`model` is what it carries).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tow {
    pub truck: CarId,
    pub model: VehicleModel,
    pub yard: Option<BuildingId>,
}

/// A crash, for the record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrashRecord {
    pub id: u32,
    pub tick: u64,
    pub pos: Vec2,
    pub cause: CrashCause,
    pub severity: f32,
    pub drivers: Vec<CitizenId>,
    pub injured: Vec<CitizenId>,
    pub drunk: bool,
    pub incident: Option<u32>,
    pub place: String,
}

/// The driving side of the city: counts, wrecks, tows, takeovers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Driving {
    pub crashes_today: u32,
    pub injuries_today: u32,
    pub red_runs_today: u32,
    pub drunk_trips_today: u32,
    /// [rage] Drivers who lost their temper (anger past a half) today.
    pub road_rage_today: u32,
    pub crashes_total: u32,
    pub towed_total: u32,
    pub wrecks: Vec<Wreck>,
    pub tows: Vec<Tow>,
    pub recent: Vec<CrashRecord>,
    pub takeovers: Takeovers,
    /// [tow-moves] Tow trucks off the lanes against a one-way or turning
    /// round (`world/tow_moves.rs`).
    pub moves: Vec<super::tow_moves::TowMove>,
    /// [rage] Illegal U-turns by seething drivers, today and in all.
    pub rage_u_turns_today: u32,
    pub rage_u_turns_total: u32,
    /// [watchdog] Service vehicles watched for standing still on the
    /// road, and how many were sent on or cleared (`world/watchdog.rs`).
    pub watch: std::collections::BTreeMap<CarId, super::watchdog::Watched>,
    pub watchdog_rerouted_total: u32,
    pub watchdog_cleared_total: u32,
}

/// A stable `0..1` from a car id, for its weave phase and a stranger's traits.
fn car_hash(id: CarId, salt: u64) -> f32 {
    let x = slotmap::Key::data(&id).as_ffi().wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    let x = (x ^ (x >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((x ^ (x >> 32)) % 10_000) as f32 / 10_000.0
}

impl World {
    /// Everything driving does in a tick: after the cars have moved.
    pub(super) fn step_driving(&mut self) {
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            let d = &mut self.driving;
            d.crashes_today = 0;
            d.injuries_today = 0;
            d.red_runs_today = 0;
            d.drunk_trips_today = 0;
            d.road_rage_today = 0;
            d.rage_u_turns_today = 0;
        }
        self.driving.red_runs_today += std::mem::take(&mut self.traffic.red_runs);
        if tick % 5 == 0 {
            self.step_drink(5);
        }
        self.assign_drivers();
        self.step_rage();
        // [rage] Some of the seething turn round illegally.
        if tick % 10 == 6 {
            self.rage_u_turns();
        }
        let crashes = std::mem::take(&mut self.traffic.crashes);
        for c in crashes {
            self.handle_crash(c);
        }
        self.step_wrecks();
        self.step_takeovers();
    }
}

