//! [passengers] More than one person to a car. The logic is
//! `world/rides.rs`; this is what it keeps.
//!
//! A passenger is a citizen in `Activity::Passenger`: they walk beside the
//! driver to the car, ride in it (hidden, carried along, listed in the road
//! car's `occupants`) and walk on from wherever it parks. Rides come about
//! four ways: a household member or coworker brought along on an outing or
//! a date (`World::bring_company`), coworkers who live together or next
//! door sharing the drive to and from work, a parent dropping a child at
//! school on the way to work (a stop on the way: `OnwardLeg`), and the mode
//! choice itself, which offers a seat in the car of someone setting off
//! from the same place to the same part of town (`ModePlan::Lift`).

use crate::buildings::BuildingId;
use crate::citizens::{CitizenId, Purpose};
use serde::{Deserialize, Serialize};

/// Passengers a car takes besides the driver.
pub const CAR_SEATS: usize = 4;
/// And a motorcycle or scooter.
pub const PILLION_SEATS: usize = 1;
/// Passenger and driver must set off from this close together.
pub const TOGETHER: f32 = 60.0;
/// A lift is worth taking when the driver is going to within this of
/// where the passenger is going.
pub const LIFT_REACH: f32 = 300.0;

/// Seats for passengers in a vehicle of this model.
pub fn seats(model: &crate::vehicle::VehicleModel) -> usize {
    if model.body.two_wheeler() { PILLION_SEATS } else { CAR_SEATS }
}

/// A driver dropping someone off at `stop` on the way: once there, the
/// passengers for it get out and the car drives on to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnwardLeg {
    pub driver: CitizenId,
    pub stop: BuildingId,
    pub to: BuildingId,
    pub purpose: Purpose,
}

/// Counts for the report.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RideStats {
    /// People who rode as passengers today, and why.
    pub passengers_today: u32,
    pub outings_today: u32,
    pub carpools_today: u32,
    pub school_runs_today: u32,
    pub lifts_today: u32,
    pub passengers_yesterday: u32,
    pub passengers_total: u64,
    /// The most people seen in one car at once (driver included).
    pub fullest_car: u32,
}

impl RideStats {
    pub fn roll(&mut self) {
        self.passengers_yesterday = self.passengers_today;
        self.passengers_today = 0;
        self.outings_today = 0;
        self.carpools_today = 0;
        self.school_runs_today = 0;
        self.lifts_today = 0;
    }
}

/// Why someone is riding along, for the counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RideKind {
    Outing,
    Carpool,
    SchoolRun,
    Lift,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rides {
    /// Drivers on the way to drop someone off.
    pub onward: Vec<OnwardLeg>,
    pub stats: RideStats,
    /// Road cars given occupants last tick, to clear before the next.
    #[serde(skip)]
    pub cars: Vec<crate::traffic::CarId>,
}

impl Rides {
    pub fn note(&mut self, kind: RideKind) {
        let s = &mut self.stats;
        s.passengers_today += 1;
        s.passengers_total += 1;
        match kind {
            RideKind::Outing => s.outings_today += 1,
            RideKind::Carpool => s.carpools_today += 1,
            RideKind::SchoolRun => s.school_runs_today += 1,
            RideKind::Lift => s.lifts_today += 1,
        }
    }

    /// Where a driver goes after the stop at `stop`, taken off the list.
    pub fn take_onward(&mut self, driver: CitizenId) -> Option<OnwardLeg> {
        let k = self.onward.iter().position(|l| l.driver == driver)?;
        Some(self.onward.remove(k))
    }
}
