//! [parking] Parking rules, the officers who enforce them, the tow trucks
//! and the pound, and the parking lots the city runs. The data lives here;
//! the logic is `world/enforcement.rs`.
//!
//! Kerb parking is limited to `KERB_LIMIT` minutes on every street (a
//! per-street override is `Vehicles::kerb_rules`); residents parked within
//! `PERMIT_RADIUS` of home hold a permit and are exempt. Officers from a
//! parking enforcement office patrol the streets within its reach in small
//! cars. A car over its limit gets a ticket (`FINE`, from the household,
//! to the treasury as parking fines); still there `GRACE` minutes later, or
//! parked where parking is banned (a shop front, a clearway), it is towed.
//! A tow company (a business on industrial land) sends a truck that hooks
//! the car and carries it to its pound; with no company in town, one from
//! out of town comes from beyond the map edge, later and dearer. The owner
//! pays the fee (plus storage by the day) and fetches the car, or after
//! `AUCTION_AFTER` it is sold at auction. A car is never deleted.
//!
//! [boot] The player can have any private car clamped and towed
//! (`World::order_tow`): a car on the road pulls over to the kerb first and
//! its people get out and walk on; the clamp (`ParkSpot::booted`, drawn as
//! an orange clamp on a wheel) keeps it there, off the lanes so traffic
//! passes, until the tow truck takes it to the pound like any other.
//!
//! [clamp] Officers clamp too: a car still over its limit when the ticket's
//! grace has run out is clamped (`ParkSpot::booted`) with a clamp fine
//! (`CLAMP_FINE`) instead of being towed at once. The owner pays it (by
//! phone, when awake and able: the clamp comes off) or, after
//! `CLAMP_LIMIT`, the tow truck takes the car to the pound like any other,
//! the unpaid fine added to the release fee. Cars parked where parking is
//! banned are still towed at once (they are in the way).

use crate::buildings::BuildingId;
use crate::traffic::CarId;
use crate::vehicle::VehicleId;
use serde::{Deserialize, Serialize};

/// Minutes a car may stand at a kerb by default.
pub const KERB_LIMIT: u64 = 120;
/// Minutes after the ticket before the tow truck is called.
pub const GRACE: u64 = 60;
/// A parking ticket.
pub const FINE: f32 = 35.0;
/// Residents parked this close to home have a permit (no limit).
pub const PERMIT_RADIUS: f32 = 150.0;
/// What a tow company in town charges to release a towed car.
pub const TOW_FEE: f32 = 80.0;
/// And one from out of town.
pub const OUT_OF_TOWN_TOW_FEE: f32 = 180.0;
/// Pound storage, per day begun.
pub const STORAGE_PER_DAY: f32 = 15.0;
/// Minutes before a truck from out of town sets off (it has far to come).
pub const OUT_OF_TOWN_DELAY: u64 = 45;
/// [clamp] What getting an officer's clamp taken off costs.
pub const CLAMP_FINE: f32 = 90.0;
/// [clamp] Minutes a clamped car waits for its fine before it is towed.
pub const CLAMP_LIMIT: u64 = 6 * 60;
/// [clamp] Hourly chance an owner who can pay settles the clamp fine
/// (people put it off; the poor cannot pay at all).
pub const CLAMP_PAY_CHANCE: f32 = 0.35;
/// Tow trucks the out-of-town company has on the city's roads at once.
pub const OUT_OF_TOWN_TOW_TRUCKS: usize = 4;
/// Minutes a towed car waits in the pound before it is auctioned.
pub const AUCTION_AFTER: u64 = 3 * crate::clock::MINUTES_PER_DAY;
/// Tow trucks a tow company runs per four staff (at least one).
pub const TRUCKS_PER_STAFF: usize = 4;
/// How close an officer passes to spot a car.
pub const SPOT_RANGE: f32 = 25.0;
/// Hours officers are out on patrol.
pub const PATROL_HOURS: std::ops::Range<u8> = 7..21;

/// Why a car is being towed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TowReason {
    /// Over the kerb limit and still there after the ticket's grace.
    Overstay,
    /// Parked where parking is banned.
    Illegal,
    /// [boot] The player had it clamped and towed (`World::order_tow`).
    Ordered,
    /// [clamp] An officer clamped it and the fine was not paid in time
    /// (`CLAMP_LIMIT`): the job waits that long before the truck sets off.
    Unpaid,
}

impl TowReason {
    pub fn label(self) -> &'static str {
        match self {
            TowReason::Overstay => "over the time limit",
            TowReason::Illegal => "parked illegally",
            TowReason::Ordered => "clamped on the city's orders",
            TowReason::Unpaid => "clamped and the fine went unpaid",
        }
    }
}

/// Where a tow job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TowState {
    /// Called; the truck sets off at `until`.
    Waiting { until: u64 },
    /// The truck is on its way to the car.
    ToCar,
    /// The car is hooked on and going to the pound.
    ToPound,
}

/// One car to be towed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TowJob {
    pub id: u32,
    pub car: VehicleId,
    /// The tow company (a business); `None`: a company from out of town.
    pub company: Option<BuildingId>,
    pub truck: Option<CarId>,
    pub state: TowState,
    pub reason: TowReason,
    pub since: u64,
    /// [tow-moves] The truck comes the last stretch against a one-way
    /// street (`world/tow_moves.rs`).
    #[serde(default)]
    pub contra: Option<crate::world::tow_moves::Contra>,
}

/// An enforcement car out on its rounds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Patrol {
    pub car: CarId,
    pub office: BuildingId,
}

/// Counts for the report.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnforcementStats {
    pub tickets_today: u32,
    pub fines_today: f32,
    pub tows_today: u32,
    pub tows_out_of_town_today: u32,
    pub released_today: u32,
    pub release_fees_today: f32,
    pub auctioned_today: u32,
    pub city_lot_fees_today: f32,
    pub tickets_total: u32,
    pub fines_total: f32,
    pub tows_total: u32,
    pub tows_out_of_town_total: u32,
    pub released_total: u32,
    pub auctioned_total: u32,
    pub wrecks_towed_total: u32,
    /// [boot] Cars the player had clamped and towed.
    pub boots_today: u32,
    pub boots_total: u32,
    /// [clamp] Cars officers clamped, clamp fines paid (and what they
    /// brought in), and clamped cars towed for want of payment.
    pub clamps_today: u32,
    pub clamps_total: u32,
    pub clamps_paid_today: u32,
    pub clamps_paid_total: u32,
    pub clamp_fines_today: f32,
    pub clamps_towed_total: u32,
    /// [tow-moves] Tow trucks that drove the last stretch against a
    /// one-way street, and that turned round in the street.
    pub contraflow_total: u32,
    pub u_turns_total: u32,
}

/// Officers on patrol, tow jobs, and the counts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Enforcement {
    pub patrols: Vec<Patrol>,
    pub jobs: Vec<TowJob>,
    pub next_job: u32,
    pub stats: EnforcementStats,
}

impl Enforcement {
    pub fn job_for(&self, car: VehicleId) -> Option<&TowJob> {
        self.jobs.iter().find(|j| j.car == car)
    }

    pub fn job(&self, id: u32) -> Option<&TowJob> {
        self.jobs.iter().find(|j| j.id == id)
    }
}

/// The three sizes of parking lot the player can place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LotSize {
    /// A small surface lot.
    Small,
    /// A large surface lot.
    Large,
    /// A multi-storey car park.
    MultiStorey,
}

impl LotSize {
    pub const ALL: [LotSize; 3] = [LotSize::Small, LotSize::Large, LotSize::MultiStorey];

    pub fn key(self) -> &'static str {
        match self {
            LotSize::Small => "parking_small",
            LotSize::Large => "parking_large",
            LotSize::MultiStorey => "parking_garage",
        }
    }

    pub fn from_key(s: &str) -> Option<LotSize> {
        match s {
            "parking_small" | "small_lot" | "parking_lot_small" => Some(LotSize::Small),
            "parking_large" | "large_lot" | "parking_lot_large" | "parking_lot" => Some(LotSize::Large),
            "parking_garage" | "multi_storey" | "multistorey" | "car_park" | "parking_multi_storey" => Some(LotSize::MultiStorey),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LotSize::Small => "small parking lot",
            LotSize::Large => "large parking lot",
            LotSize::MultiStorey => "multi-storey car park",
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            LotSize::Small => "Parking lot (small)",
            LotSize::Large => "Parking lot (large)",
            LotSize::MultiStorey => "Car park (multi-storey)",
        }
    }

    /// Spaces.
    pub fn capacity(self) -> u16 {
        match self {
            LotSize::Small => 12,
            LotSize::Large => 30,
            LotSize::MultiStorey => 120,
        }
    }

    /// Decks drawn (0: a surface lot).
    pub fn floors(self) -> u8 {
        match self {
            LotSize::Small | LotSize::Large => 0,
            LotSize::MultiStorey => 4,
        }
    }

    pub fn min_area(self) -> f32 {
        match self {
            LotSize::Small => 250.0,
            LotSize::Large => 600.0,
            LotSize::MultiStorey => 450.0,
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            LotSize::Small => 2_500.0,
            LotSize::Large => 6_000.0,
            LotSize::MultiStorey => 22_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            LotSize::Small => 20.0,
            LotSize::Large => 50.0,
            LotSize::MultiStorey => 160.0,
        }
    }

    /// Attendants (paid by the city).
    pub fn jobs(self) -> u16 {
        match self {
            LotSize::Small => 1,
            LotSize::Large => 2,
            LotSize::MultiStorey => 4,
        }
    }

    /// The default fee per car parked.
    pub fn default_price(self) -> f32 {
        match self {
            LotSize::Small | LotSize::Large => 2.0,
            LotSize::MultiStorey => 3.0,
        }
    }
}

/// A parking lot the city runs: its size and the fee the player set.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CityLot {
    pub size: LotSize,
    pub price: f32,
}
