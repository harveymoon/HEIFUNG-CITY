//! Citizens: daily upkeep, immigration and emigration, jobs, decisions,
//! journeys (`travel_to`, `arrive`, `wander`) and footfall.

use super::*;
use crate::citizens::{Household, HouseholdId, LifeStage, TripMode, GROCERIES_PER_PERSON_DAY, PENSION, POCKET_MONEY};
use crate::vehicle::{VehicleClass, VehicleModel};

mod household;
mod population;
mod decide;
#[cfg(test)]
pub(in crate::world) use decide::WANDER_REACH;
mod arrive;
mod travel;

/// Most households in one arrival, however empty the town.
const MAX_ARRIVALS: u32 = 12;
/// How long (ticks) a late mind answer for someone on their way is still
/// acted on once they arrive: an hour. Older, the moment it answered has
/// passed and the citizen decides afresh.
pub(super) const DEFERRED_INTENT_TTL: u64 = 60;
/// Share of working-age adults out of work above which, with no job free,
/// households mostly stop arriving.
const JOBLESS_SLOWS_ARRIVALS: f32 = 0.05;
/// Households look for homes whose rent is at most this share of income.
pub const AFFORDABLE_SHARE: f32 = 0.35;
/// Rent over this share of income is stress: they move somewhere cheaper,
/// or leave once the money is gone.
pub const RENT_STRESS_SHARE: f32 = 0.5;

/// Cover charge at a venue's door.
pub(super) const VENUE_COVER: f32 = 15.0;

/// What a trip asks of the mode choice: from where to where, for what,
/// and which modes are allowed this time.
#[derive(Clone, Copy, Debug)]
pub struct TripQuery {
    pub from: Vec2,
    pub to: Vec2,
    pub target: BuildingId,
    pub purpose: Purpose,
    pub allow_transit: bool,
    pub allow_car: bool,
    pub allow_bike: bool,
}

/// How a trip would be made, concretely.
#[derive(Clone, Copy, Debug)]
pub enum ModePlan {
    Walk,
    Bike,
    /// A bus ride; [bike-racks] `bike`: a cyclist riding to the stop and
    /// putting the bike on the bus's rack.
    Bus { route: RouteId, board: usize, alight: usize, stop: Vec2, bike: bool },
    Train { board: BuildingId, alight: BuildingId, stop: Vec2 },
    /// [subway] A metro ride on one line, walking to `stop` (the entrance).
    Subway { line: crate::subway::MetroLineId, board: BuildingId, alight: BuildingId, stop: Vec2 },
    /// The citizen's car, parked at `at`.
    Car { car: crate::vehicle::VehicleId, at: Vec2 },
    /// [passengers] A seat in `driver`'s car, setting off from here.
    Lift { driver: CitizenId },
    /// [taxis] Hail a cab: wait at `wait_at` for one coming to `at` on
    /// `segment` (`world/taxis.rs`).
    Taxi { segment: SegmentId, at: Vec2, wait_at: Vec2 },
    /// [ferry-islands] Across the water by ferry from `board`'s pier
    /// (`stop`, where to wait) to `alight`'s (`world/islands.rs`).
    Ferry { board: BuildingId, alight: BuildingId, stop: Vec2 },
    /// [cable] A cable car ride on one line, walking to `stop` (the
    /// station's entrance).
    Cable { line: crate::cable::CableLineId, board: crate::cable::CableStationId, alight: crate::cable::CableStationId, stop: Vec2 },
}

/// One way to make a trip and what it would cost the traveller, in
/// minute-equivalents: `minutes` weighed by ambition, `money` by income,
/// `comfort` by personality (negative is a pleasure), and the remembered
/// `pain` of the mode times `PAIN_WEIGHT`.
#[derive(Clone, Copy, Debug)]
pub struct ModeOption {
    pub mode: TripMode,
    pub minutes: f32,
    pub money: f32,
    pub comfort: f32,
    pub pain: f32,
    pub cost: f32,
    pub plan: ModePlan,
}

/// Minute-equivalents one remembered bad half-hour adds to a mode.
pub const PAIN_WEIGHT: f32 = 20.0;
