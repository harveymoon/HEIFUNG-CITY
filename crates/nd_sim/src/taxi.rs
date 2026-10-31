//! Taxis on the road (the drones are `crate::air`): the data and the pure
//! rules. The systems are in `world/taxis.rs`.
//!
//! A taxi company is a commercial business (`BusinessKind::TaxiCompany`)
//! with a yard of bays. Nobody plans one: when enough trips would have gone
//! by cab and there was none to take (`Taxis::latent_today`), a citizen
//! with savings opens a company near where those people were, starting
//! with a few cabs, and it buys more while riders are left waiting.
//!
//! Every cab is a record with a place, like a private car: in its yard,
//! waiting at a rank or at the kerb, or on the road. It never vanishes: a
//! cab taken off the road by road works goes back to its yard, and the
//! cabs of a company that closes are sold and driven out of town.
//!
//! A rider walks to the kerb and waits; the nearest cab on duty comes,
//! picks them up, drives them to the kerb of their destination, takes the
//! fare and goes back to a rank. Shifts follow the night: most of the fleet
//! is out in the evening and the small hours (when the buses have stopped
//! and the bars let out), few at dawn; the rest stand in the yard.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::roads::SegmentId;
use crate::traffic::CarId;
use crate::vehicle::{BodyType, Livery, VehicleClass, VehicleModel};
use glam::Vec2;
use serde::{Deserialize, Serialize};

slotmap::new_key_type! {
    /// A cab of a taxi company.
    pub struct CabId;
}

/// Fare: the flag drop plus a rate per kilometre driven.
pub const FLAG_FARE: f32 = 4.0;
pub const FARE_PER_KM: f32 = 3.5;
/// No cab for hops shorter than this (straight line, metres).
pub const MIN_TRIP: f32 = 250.0;
/// A cab further than this (straight line) is not sent for a rider: at
/// city speeds it would be most of an hour coming.
pub const PICKUP_REACH: f32 = 1100.0;
/// Ticks a rider waits at the kerb for a cab that has not been assigned
/// before giving up; one on its way is waited for twice as long.
pub const WAIT_GIVE_UP: u64 = 30;
/// [ped-highway] Ticks a rider who gave up on a cab books no other: they
/// walk, ride or take the bus instead of standing at the kerb again.
pub const GAVE_UP_SHY: u64 = 240;
/// What a company pays for a cab.
pub const CAB_PRICE: f32 = 420.0;
/// Cabs a new company starts with.
pub const START_CABS: usize = 3;
/// Most cabs one company runs.
pub const MAX_CABS: usize = 20;
/// A rider who expects to wait longer than this for a cab does not hail
/// one (the trip counts as demand nobody served instead).
pub const MAX_EXPECTED_WAIT: f32 = 30.0;
/// Most cabs a company buys in one night.
pub const MAX_BUY_PER_DAY: usize = 4;
/// Missed riders per extra cab bought.
pub const MISSED_PER_CAB: u32 = 8;
/// People per taxi company the town supports at most (one at least).
pub const PEOPLE_PER_COMPANY: usize = 1500;
/// Cabs that wait at one rank, nose to tail.
pub const RANK_SLOTS: usize = 4;
/// Smallest lot a taxi yard opens on: room for a row of bays.
pub const YARD_MIN_AREA: f32 = 450.0;
/// Staff of a company: dispatchers and drivers on the rota.
pub const COMPANY_JOBS: u16 = 6;
/// Trips that would have gone by cab, with none to be had, in a day before
/// someone opens a company (more for each company already in town).
pub const FOUND_DEMAND: u32 = 10;
/// Missed riders in a day before a company buys another cab.
pub const GROW_MISSED: u32 = 3;

/// The fare for `metres` driven.
pub fn fare(metres: f32) -> f32 {
    FLAG_FARE + FARE_PER_KM * metres.max(0.0) / 1000.0
}

/// Share of a company's fleet on duty at this hour: the evening and the
/// small hours are a cab's busiest (the buses stop at 23:30, the bars let
/// out), dawn the quietest. Off-duty cabs stand in the yard.
pub fn on_duty_share(hour: u8) -> f32 {
    match hour % 24 {
        0..=2 => 1.0,
        3 => 0.6,
        4 | 5 => 0.3,
        6..=9 => 0.7,
        10..=15 => 0.5,
        16..=19 => 0.8,
        _ => 1.0,
    }
}

/// Is it the time of night when walking home is grim and nothing else runs?
pub fn late_night(hour: u8) -> bool {
    !(5..22).contains(&(hour % 24))
}

/// Comfort of a cab ride (minute-equivalents, negative is a pleasure): a
/// seat with someone else driving, best to hedonists; at night a cab is
/// the safe way home; for someone drunk it is the way home.
pub fn comfort(hedonism: f32, drunk: bool, night: bool, frail: bool) -> f32 {
    let mut c = -3.0 - 5.0 * hedonism;
    if night {
        c -= 8.0;
    }
    if drunk {
        c -= 25.0;
    }
    if frail {
        c -= 4.0;
    }
    c
}

/// A cab's paint job: neon yellow with a checker band and a lit roof sign
/// (`Livery::Taxi`), on a sedan, hatchback or SUV by the cab's number.
pub fn cab_model(number: u16) -> VehicleModel {
    let body = match number % 5 {
        0 | 2 => BodyType::Sedan,
        1 => BodyType::Hatchback,
        3 => BodyType::Suv,
        _ => BodyType::Sedan,
    };
    VehicleModel { class: VehicleClass::Mid, body, wear: (number % 7 == 6) as u8, tuned: false, livery: Livery::Taxi, paint: (number % crate::vehicle::PAINTS as u16) as u8, boxed: 0 }
}

/// Where a cab is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CabPlace {
    /// In a bay of its company's yard.
    Yard,
    /// Waiting at a rank (a station, a busy venue, the yard gate): slot
    /// `slot` back from the head of the rank.
    Rank { rank: BuildingId, slot: u8, segment: SegmentId, pos: Vec2, heading: f32 },
    /// Stopped at the kerb: waiting for a rider who has not reached it yet,
    /// or set down with nowhere to go.
    Kerb { segment: SegmentId, pos: Vec2, heading: f32 },
    /// On the road, a car of the traffic model.
    Road(CarId),
}

impl CabPlace {
    pub fn label(&self) -> &'static str {
        match self {
            CabPlace::Yard => "yard",
            CabPlace::Rank { .. } => "rank",
            CabPlace::Kerb { .. } => "kerb",
            CabPlace::Road(_) => "road",
        }
    }

    /// Where it stands, when parked somewhere on the street.
    pub fn street_pose(&self) -> Option<(Vec2, f32)> {
        match *self {
            CabPlace::Rank { pos, heading, .. } | CabPlace::Kerb { pos, heading, .. } => Some((pos, heading)),
            _ => None,
        }
    }
}

/// What a cab is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum CabJob {
    /// Free for a fare.
    #[default]
    Idle,
    /// On the way to pick `rider` up (or waiting at the kerb for them).
    Pickup { rider: CitizenId },
    /// Carrying `rider`; the meter started at `from`.
    Fare { rider: CitizenId, from: Vec2 },
    /// Off to wait at a rank.
    ToRank { rank: BuildingId },
    /// Back to the yard (end of a shift, or the ranks are full).
    ToYard,
    /// Sold: driving out of town for good.
    Leaving,
}

impl CabJob {
    pub fn label(&self) -> &'static str {
        match self {
            CabJob::Idle => "free",
            CabJob::Pickup { .. } => "to pick up",
            CabJob::Fare { .. } => "carrying a fare",
            CabJob::ToRank { .. } => "to a rank",
            CabJob::ToYard => "to the yard",
            CabJob::Leaving => "sold, leaving town",
        }
    }

    /// Free to be sent for a rider.
    pub fn free(&self) -> bool {
        matches!(self, CabJob::Idle | CabJob::ToRank { .. } | CabJob::ToYard)
    }

    pub fn rider(&self) -> Option<CitizenId> {
        match *self {
            CabJob::Pickup { rider } | CabJob::Fare { rider, .. } => Some(rider),
            _ => None,
        }
    }
}

/// A cab of a taxi company.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cab {
    pub company: BuildingId,
    /// Its number in the company's fleet (from 1), for its paint and name.
    pub number: u16,
    pub model: VehicleModel,
    pub place: CabPlace,
    #[serde(default)]
    pub job: CabJob,
    /// On the rota this hour; off-duty cabs stand in the yard.
    #[serde(default)]
    pub on_duty: bool,
    /// Where it was last seen (for a rider set down if the road goes).
    #[serde(default)]
    pub last_pos: Vec2,
    /// Tick it took up its place or job.
    #[serde(default)]
    pub since: u64,
}

/// A rider waiting for a cab, and the kerb they wait at.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TaxiRequest {
    pub rider: CitizenId,
    pub segment: SegmentId,
    /// The point on the street the cab drives to.
    pub at: Vec2,
    pub since: u64,
    #[serde(default)]
    pub cab: Option<CabId>,
}

/// The day's numbers, for the Transit window and the API.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TaxiStats {
    pub rides_today: u32,
    pub rides_yesterday: u32,
    pub fares_today: f32,
    pub fares_yesterday: f32,
    /// Rides by someone drunk, and rides in the small hours.
    pub drunk_rides_today: u32,
    pub night_rides_today: u32,
    /// Riders who gave up waiting at the kerb.
    pub gave_up_today: u32,
    pub gave_up_yesterday: u32,
    /// Minutes riders waited at the kerb, over the rides today.
    pub wait_minutes_today: f32,
    /// Metres driven with a fare aboard today.
    pub fare_metres_today: f32,
    pub companies_founded_total: u32,
    pub cabs_bought_total: u32,
    pub cabs_sold_total: u32,
    /// Cabs taken off the road by road works and put back in their yard.
    pub cabs_recovered_total: u32,
}

/// Every cab, every rider waiting, and the demand nobody served.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Taxis {
    pub cabs: slotmap::SlotMap<CabId, Cab>,
    #[serde(default)]
    pub requests: Vec<TaxiRequest>,
    #[serde(default)]
    pub stats: TaxiStats,
    /// Trips today (and yesterday) that would have gone by cab had there
    /// been one in reach: the signal a company opens on.
    #[serde(default)]
    pub latent_today: u32,
    #[serde(default)]
    pub latent_yesterday: u32,
    /// Sum of where those trips started, to open the company near them.
    #[serde(default)]
    pub latent_sum: Vec2,
    /// Riders each company had no cab for today and yesterday (missed
    /// riders make it buy cabs).
    #[serde(default)]
    pub missed: std::collections::BTreeMap<BuildingId, (u32, u32)>,
    /// Rides each company gave today and yesterday.
    #[serde(default)]
    pub rides_by_company: std::collections::BTreeMap<BuildingId, (u32, u32)>,
    /// [ped-highway] Riders who gave up waiting for a cab, until the tick
    /// they would book one again (`GAVE_UP_SHY`).
    #[serde(default)]
    pub shy: std::collections::BTreeMap<CitizenId, u64>,
}

impl Taxis {
    /// Cabs of `company`.
    pub fn fleet(&self, company: BuildingId) -> impl Iterator<Item = (CabId, &Cab)> {
        self.cabs.iter().filter(move |(_, c)| c.company == company)
    }

    pub fn fleet_size(&self, company: BuildingId) -> usize {
        self.fleet(company).count()
    }

    /// The cab a rider is in or waiting for.
    pub fn cab_for(&self, rider: CitizenId) -> Option<CabId> {
        self.cabs.iter().find(|(_, c)| c.job.rider() == Some(rider)).map(|(id, _)| id)
    }

    /// The cab driving the traffic car `car`.
    pub fn cab_of_car(&self, car: CarId) -> Option<CabId> {
        self.cabs.iter().find(|(_, c)| c.place == CabPlace::Road(car)).map(|(id, _)| id)
    }

    /// Cabs parked at `rank` (and on their way to it).
    pub fn at_rank(&self, rank: BuildingId) -> usize {
        self.cabs.values().filter(|c| matches!(c.place, CabPlace::Rank { rank: r, .. } if r == rank) || c.job == CabJob::ToRank { rank }).count()
    }

    pub fn note_missed(&mut self, company: BuildingId) {
        self.missed.entry(company).or_default().0 += 1;
    }

    /// Counts by what the cabs are doing: yard, rank, kerb, road;
    /// carrying, on the way to a pickup.
    pub fn counts(&self) -> TaxiCounts {
        let mut k = TaxiCounts::default();
        for c in self.cabs.values() {
            match c.place {
                CabPlace::Yard => k.in_yard += 1,
                CabPlace::Rank { .. } => k.at_ranks += 1,
                CabPlace::Kerb { .. } => k.at_kerb += 1,
                CabPlace::Road(_) => k.on_road += 1,
            }
            match c.job {
                CabJob::Fare { .. } => k.carrying += 1,
                CabJob::Pickup { .. } => k.to_pickup += 1,
                _ => {}
            }
            if c.on_duty {
                k.on_duty += 1;
            }
        }
        k.cabs = self.cabs.len();
        k
    }
}

/// What the fleet is doing right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaxiCounts {
    pub cabs: usize,
    pub on_duty: usize,
    pub in_yard: usize,
    pub at_ranks: usize,
    pub at_kerb: usize,
    pub on_road: usize,
    pub carrying: usize,
    pub to_pickup: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fares_rise_with_distance() {
        assert_eq!(fare(0.0), FLAG_FARE);
        assert!((fare(2000.0) - (FLAG_FARE + 2.0 * FARE_PER_KM)).abs() < 1e-4);
        assert!(fare(3000.0) > fare(1000.0));
        assert_eq!(fare(-5.0), FLAG_FARE);
    }

    #[test]
    fn the_night_shift_is_the_big_one() {
        assert_eq!(on_duty_share(23), 1.0);
        assert_eq!(on_duty_share(1), 1.0);
        assert!(on_duty_share(5) < on_duty_share(12));
        assert!(on_duty_share(12) < on_duty_share(18));
        for h in 0..24 {
            assert!(on_duty_share(h) > 0.0, "someone is always on call at {h}:00");
        }
        assert!(late_night(23) && late_night(2) && !late_night(12) && !late_night(5));
    }

    #[test]
    fn drunk_and_night_riders_like_a_cab_most() {
        let day = comfort(0.5, false, false, false);
        let night = comfort(0.5, false, true, false);
        let drunk = comfort(0.5, true, true, false);
        assert!(night < day && drunk < night);
        assert!(comfort(1.0, false, false, false) < comfort(0.0, false, false, false), "hedonists enjoy being driven");
    }

    #[test]
    fn cabs_wear_the_taxi_livery() {
        for n in 1..20 {
            let m = cab_model(n);
            assert_eq!(m.livery, Livery::Taxi);
            assert!(!m.body.two_wheeler() && !m.body.is_truck());
        }
    }

    #[test]
    fn old_saves_load_without_taxis() {
        let s: TaxiStats = ron::from_str("()").unwrap();
        assert_eq!(s.rides_today, 0);
        let mut t = Taxis::default();
        t.cabs.insert(Cab { company: BuildingId::default(), number: 1, model: cab_model(1), place: CabPlace::Yard, job: CabJob::Idle, on_duty: false, last_pos: Vec2::ZERO, since: 0 });
        let back: Taxis = ron::from_str(&ron::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.cabs.len(), 1);
        assert_eq!(back.counts().in_yard, 1);
    }
}
