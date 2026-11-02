//! [airport] Airports: the data, the layout and the rules (the systems are
//! in `world/airports.rs`).
//!
//! - **Placing one.** The Buildings tool's Transit menu offers a regional
//!   airfield (one runway, a small terminal, walk-out stands) and an
//!   international airport (two parallel runways, a terminal with jet
//!   bridges, a cargo terminal, hangars, a tower and car parks). A click
//!   fits the whole complex to the ground there: it tries every heading
//!   and takes the one with the least earthwork that clears everything
//!   (`World::plan_airport`). The site must be on the map, dry, not too
//!   hilly to level (`MAX_EARTHWORK`), off every road and civic structure,
//!   and the approach paths off both runway ends must be clear: no hill
//!   and no building rising through the approach surface
//!   (`approach_limit`, a slope of `APPROACH_SLOPE` from the runway end).
//!   Grown buildings on the site itself are bought out.
//! - **Height limits.** Once it stands, the approach surfaces are height
//!   limit zones: nothing new grows through them (`World::height_limit_at`).
//! - **Flights.** Planes come in from towns off the map and go back on a
//!   timetable made each midnight (`timetable`) from the city's demand:
//!   residents flying away, tourists, business travellers (`daily_pax`).
//!   A plane flies the approach in over the map edge, lands into the wind,
//!   rolls out, taxis to a stand, turns round (passengers, bags, fuel),
//!   pushes back, taxis to the far runway end, takes off and climbs out
//!   over the edge. Its pose at any moment is a pure function of its phase
//!   and the clock (`plane_pose`), so the renderer draws it exactly as the
//!   sim reckons it, and nothing about it needs saving but the phase.
//! - **Passengers** arrive and leave by car, taxi, the airport coach, and
//!   by rail, metro or bus when a station or stop is in walking reach of
//!   the terminal; tourists off the planes check into the city's hotels.
//! - **Air cargo.** Dear goods (electronics, art, a share of consumer
//!   goods at the international airport) come in by air and are trucked
//!   from the cargo terminal; the city's dear exports leave the same way.
//! - **Money.** Landing charges, a passenger fee and a cargo handling fee
//!   (`Flow::AirportFees`) against its upkeep; it employs its staff.
//! - **Noise.** Every landing and take-off is logged on the air noise grid
//!   (`crate::air::Air::noise`) along its path, and the flight paths take
//!   a strong bite out of the land value; homes under them complain.
//!
//! Airlines, towns and the airport's own name are fictional.

use crate::buildings::BuildingId;
use crate::market::Resource;
use crate::traffic::{CarId, Cargo};
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct AirportId;
}

// ── balance ────────────────────────────────────────────────────────────────

/// The approach surface: rises this much per metre out from the runway end.
pub const APPROACH_SLOPE: f32 = 1.0 / 40.0;
/// It starts this far beyond the runway end, this wide each side of the
/// centreline there, and widens by `APPROACH_SPLAY` per metre out.
pub const APPROACH_START: f32 = 60.0;
pub const APPROACH_SPLAY: f32 = 0.15;
/// And runs this far out (or to the map edge).
pub const APPROACH_LENGTH: f32 = 3000.0;
/// Most ground levelled away (the deepest cut or highest fill, from the
/// site's mean) before a site is too hilly.
pub const MAX_EARTHWORK: f32 = 40.0;
/// What the earthworks cost per cubic metre moved (the site's mean cut or
/// fill over its area).
pub const EARTHWORK_COST: f32 = 0.03;
/// Streams up to this wide are put in a culvert under the field.
pub const CULVERT_WIDTH: f32 = 40.0;
/// Clearing a street off the site, per metre (the works and what the
/// frontages are owed).
pub const CLEAR_COST_PER_M: f32 = 60.0;
/// Buying out a tall building in the approach path, each.
pub const OBSTACLE_COST: f32 = 6_000.0;
/// Metres the ground just off the runway ends may rise over the field (it is
/// graded with it); and the clearance hills keep under the glide path.
pub const APPROACH_GRADING: f32 = 25.0;
pub const GLIDE_CLEARANCE: f32 = 12.0;
/// The first stretch off each runway end is cut down with the field (as
/// much as the field itself may be).
pub const END_GRADING: f32 = 500.0;

/// How high the ground may rise `out` metres off a runway end, over the
/// field: the glide path (3 degrees, 15 m over the end) less the clearance,
/// never less than what the end's grading allows.
pub fn terrain_limit(out: f32) -> f32 {
    let graded = if out < END_GRADING { MAX_EARTHWORK } else { APPROACH_GRADING };
    (15.0 + (out + APPROACH_START) * GLIDE - GLIDE_CLEARANCE).max(graded)
}
/// Hills are looked for this far out (beyond, the approach is high enough
/// over the ground that only buildings count).
pub const APPROACH_TERRAIN: f32 = 1200.0;
/// Glide slope of the final approach (3 degrees) and the climb-out (7).
pub const GLIDE: f32 = 0.0524;
pub const CLIMB: f32 = 0.12;
/// Speeds, metres per second of motion (one tick shows
/// `traffic_seconds_per_tick` of it, like the cars).
pub const APPROACH_MPS: f32 = 72.0;
pub const LIFTOFF_MPS: f32 = 78.0;
pub const CLIMB_MPS: f32 = 88.0;
pub const TAXI_MPS: f32 = 11.0;
pub const PUSHBACK_MPS: f32 = 2.5;
/// Ticks between two movements on one runway.
pub const SEPARATION: u64 = 3;
/// Arrivals are timetabled from 06:00 to 22:30; nothing is scheduled to
/// land in the night curfew (a delayed flight still comes in).
pub const FIRST_ARRIVAL: u32 = 6 * 60;
pub const LAST_ARRIVAL: u32 = 22 * 60 + 30;
/// A flight held off the map this long is cancelled.
pub const CANCEL_AFTER: u64 = 180;
/// A flight this late counts as delayed.
pub const DELAYED_AFTER: u64 = 15;
/// Passenger demand per day: trips abroad per resident, per office job,
/// and the share of the city's tourist demand that comes by air.
pub const RESIDENT_TRIPS: f32 = 0.035;
pub const BUSINESS_TRIPS: f32 = 0.02;
pub const TOURIST_AIR_SHARE: f32 = 0.9;
/// Fees: per passenger leaving, and the cargo handling fee (share of the
/// value of a load through the cargo terminal).
pub const PAX_FEE: f32 = 2.5;
pub const CARGO_FEE: f32 = 0.04;
/// Dear exports flown out earn this much more.
pub const AIR_EXPORT_BONUS: f32 = 1.1;
/// Share of consumer goods the international airport flies in.
pub const GOODS_BY_AIR: f32 = 0.2;
/// A rail or metro station, or a bus stop, this close to the terminal
/// door serves the airport.
pub const TRANSIT_REACH: f32 = 450.0;
/// Most cars on the road to and from one airport at once.
pub const MAX_CARS: usize = 36;
/// Noise logged per movement on the air noise grid, at the runway end,
/// for a narrow-body jet; and how far out the path is logged.
pub const MOVEMENT_NOISE: f32 = 6.0;
pub const NOISE_REACH: f32 = 2600.0;
/// Land value taken off at the runway ends at full traffic (`FULL_NOISE`
/// movements a day), fading out along the flight paths.
pub const NOISE_LAND_HIT: f32 = 0.16;
pub const FULL_NOISE: f32 = 40.0;
/// Air noise logged in a home's cell above which its people complain.
pub const COMPLAIN_NOISE: f32 = 25.0;

// ── sizes ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AirportSize {
    /// One runway, a small terminal, stands passengers walk out to.
    Airfield,
    /// Parallel runways, a terminal with jet bridges, a cargo terminal.
    International,
}

impl AirportSize {
    pub const ALL: [AirportSize; 2] = [AirportSize::Airfield, AirportSize::International];

    pub fn label(self) -> &'static str {
        match self {
            AirportSize::Airfield => "regional airfield",
            AirportSize::International => "international airport",
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            AirportSize::Airfield => "Airfield (one runway)",
            AirportSize::International => "International airport (two runways)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AirportSize::Airfield => "airfield",
            AirportSize::International => "airport",
        }
    }

    pub fn from_key(s: &str) -> Option<AirportSize> {
        match s {
            "airfield" | "regional_airport" | "regional_airfield" | "airstrip" | "small_airport" => Some(AirportSize::Airfield),
            "airport" | "international_airport" | "international" | "large_airport" => Some(AirportSize::International),
            _ => None,
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            AirportSize::Airfield => 160_000.0,
            AirportSize::International => 520_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            AirportSize::Airfield => 1_300.0,
            AirportSize::International => 4_200.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            AirportSize::Airfield => 140,
            AirportSize::International => 620,
        }
    }

    /// Footprint along the runways and across them, metres.
    pub fn size(self) -> (f32, f32) {
        match self {
            AirportSize::Airfield => (1150.0, 300.0),
            AirportSize::International => (1760.0, 600.0),
        }
    }

    /// Fewest and most rotations (an arrival and its departure) a day.
    pub fn rotations(self) -> (u32, u32) {
        match self {
            AirportSize::Airfield => (3, 24),
            AirportSize::International => (8, 84),
        }
    }

    /// The planes it takes.
    pub fn fleet(self) -> &'static [PlaneKind] {
        match self {
            AirportSize::Airfield => &[PlaneKind::Turboprop, PlaneKind::RegionalJet],
            AirportSize::International => &[PlaneKind::RegionalJet, PlaneKind::Narrowbody, PlaneKind::Narrowbody, PlaneKind::Widebody],
        }
    }

    /// Its runways (as in its `layout`), without building the rest.
    pub fn runways(self) -> &'static [Runway] {
        match self {
            AirportSize::Airfield => &[Runway { y: -100.0, length: 900.0, width: 30.0 }],
            AirportSize::International => &[Runway { y: -245.0, length: 1500.0, width: 45.0 }, Runway { y: -125.0, length: 1500.0, width: 45.0 }],
        }
    }

    pub fn layout(self) -> Layout {
        match self {
            AirportSize::Airfield => airfield_layout(),
            AirportSize::International => international_layout(),
        }
    }
}

// ── planes ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaneKind {
    Turboprop,
    RegionalJet,
    Narrowbody,
    Widebody,
    Freighter,
}

impl PlaneKind {
    pub fn label(self) -> &'static str {
        match self {
            PlaneKind::Turboprop => "turboprop",
            PlaneKind::RegionalJet => "regional jet",
            PlaneKind::Narrowbody => "narrow-body jet",
            PlaneKind::Widebody => "wide-body jet",
            PlaneKind::Freighter => "freighter",
        }
    }

    pub fn seats(self) -> u32 {
        match self {
            PlaneKind::Turboprop => 48,
            PlaneKind::RegionalJet => 90,
            PlaneKind::Narrowbody => 180,
            PlaneKind::Widebody => 320,
            PlaneKind::Freighter => 0,
        }
    }

    /// Cargo units it carries (a passenger plane in its belly).
    pub fn cargo_units(self) -> f32 {
        match self {
            PlaneKind::Turboprop => 4.0,
            PlaneKind::RegionalJet => 8.0,
            PlaneKind::Narrowbody => 16.0,
            PlaneKind::Widebody => 45.0,
            PlaneKind::Freighter => 140.0,
        }
    }

    /// Length and wingspan, metres (the renderer draws them so).
    pub fn dims(self) -> (f32, f32) {
        match self {
            PlaneKind::Turboprop => (23.0, 27.0),
            PlaneKind::RegionalJet => (31.0, 28.0),
            PlaneKind::Narrowbody => (38.0, 35.0),
            PlaneKind::Widebody => (62.0, 60.0),
            PlaneKind::Freighter => (50.0, 48.0),
        }
    }

    pub fn landing_fee(self) -> f32 {
        match self {
            PlaneKind::Turboprop => 60.0,
            PlaneKind::RegionalJet => 110.0,
            PlaneKind::Narrowbody => 200.0,
            PlaneKind::Widebody => 420.0,
            PlaneKind::Freighter => 300.0,
        }
    }

    /// Ticks on the stand between arriving and the earliest departure.
    pub fn turnaround(self) -> u64 {
        match self {
            PlaneKind::Turboprop => 30,
            PlaneKind::RegionalJet => 40,
            PlaneKind::Narrowbody => 50,
            PlaneKind::Widebody => 85,
            PlaneKind::Freighter => 70,
        }
    }

    /// How loud it is against a narrow-body jet.
    pub fn noise(self) -> f32 {
        match self {
            PlaneKind::Turboprop => 0.45,
            PlaneKind::RegionalJet => 0.8,
            PlaneKind::Narrowbody => 1.0,
            PlaneKind::Widebody => 1.8,
            PlaneKind::Freighter => 1.6,
        }
    }

    pub fn is_jet(self) -> bool {
        self != PlaneKind::Turboprop
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Airlines (fictional). The last two fly freight.
pub const AIRLINES: [&str; 8] = ["Corvid Air", "Halide Airways", "Ferrowing", "Lumen Pacific", "Tessera Air", "Quill Regional", "Strata Freight", "Kitebox Cargo"];
const PASSENGER_AIRLINES: usize = 6;
/// Towns off the map (fictional). The first six are near enough for the
/// airfield's turboprops; the rest only the international airport serves.
pub const CITIES: [&str; 12] = ["Port Calder", "Varnholm", "Saint Orrin", "Marrow Bay", "Tiervale", "Lusk Harbour", "Okonde", "Ember Coast", "New Sarn", "Hollowmere", "Grayreach", "Sable Point"];

// ── the layout ─────────────────────────────────────────────────────────────

/// An axis-aligned box in the airport's own frame: x along the runways,
/// y across them (landside, the terminal's front, is +y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    pub fn center(&self) -> Vec2 {
        Vec2::new((self.x0 + self.x1) * 0.5, (self.y0 + self.y1) * 0.5)
    }
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.x1 - self.x0, self.y1 - self.y0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Runway {
    /// Centreline across the field, length, width.
    pub y: f32,
    pub length: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stand {
    /// Where the plane's centre stands, nose towards +y (the terminal).
    pub at: Vec2,
    /// A jet bridge from the terminal; a freight stand.
    pub bridge: bool,
    pub cargo: bool,
}

/// Where everything is on the field, in its own frame (`Rect`). Shared by
/// the sim (taxi routes, stands) and the renderer (what it draws).
#[derive(Clone, Debug)]
pub struct Layout {
    pub runways: Vec<Runway>,
    /// Which runway arrivals and departures use.
    pub arrivals: usize,
    pub departures: usize,
    /// Parallel taxiways: centreline y, from x0 to x1, width.
    pub taxiways: Vec<(f32, f32, f32, f32)>,
    /// Cross links at these x, from y0 up to y1.
    pub links: Vec<(f32, f32, f32)>,
    pub apron: Rect,
    /// The taxi lane along the apron the stands open onto.
    pub apron_lane: f32,
    pub stands: Vec<Stand>,
    pub terminal: Rect,
    pub terminal_height: f32,
    pub tower: Vec2,
    pub tower_height: f32,
    pub hangars: Vec<Rect>,
    pub cargo_shed: Rect,
    pub car_parks: Vec<(Rect, u8)>,
    pub fuel_farm: Rect,
}

impl Layout {
    pub fn passenger_stands(&self) -> impl Iterator<Item = (usize, &Stand)> {
        self.stands.iter().enumerate().filter(|(_, s)| !s.cargo)
    }

    /// The link nearest `x` (taxi routes cross over there).
    pub fn link_near(&self, x: f32, from_y: f32, to_y: f32) -> f32 {
        let (lo, hi) = (from_y.min(to_y), from_y.max(to_y));
        self.links.iter().filter(|l| l.1 <= lo + 0.5 && l.2 >= hi - 0.5).map(|l| l.0).min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs())).unwrap_or(x)
    }
}

fn airfield_layout() -> Layout {
    let stands = (0..4).map(|i| Stand { at: Vec2::new(-180.0 + i as f32 * 58.0, 18.0), bridge: false, cargo: false }).chain(std::iter::once(Stand { at: Vec2::new(205.0, 18.0), bridge: false, cargo: true })).collect();
    Layout {
        runways: AirportSize::Airfield.runways().to_vec(),
        arrivals: 0,
        departures: 0,
        taxiways: vec![(-50.0, -470.0, 470.0, 18.0)],
        links: vec![(-470.0, -100.0, -50.0), (-160.0, -100.0, -50.0), (160.0, -100.0, -50.0), (470.0, -100.0, -50.0), (-60.0, -50.0, -14.0), (120.0, -50.0, -14.0)],
        apron: Rect::new(-250.0, -24.0, 260.0, 62.0),
        apron_lane: -14.0,
        stands,
        terminal: Rect::new(-230.0, 62.0, 90.0, 102.0),
        terminal_height: 9.0,
        tower: Vec2::new(128.0, 84.0),
        tower_height: 24.0,
        hangars: vec![Rect::new(-470.0, -22.0, -360.0, 62.0), Rect::new(-345.0, -10.0, -270.0, 62.0)],
        cargo_shed: Rect::new(170.0, 62.0, 255.0, 104.0),
        car_parks: vec![(Rect::new(-240.0, 108.0, 160.0, 146.0), 1)],
        fuel_farm: Rect::new(300.0, 96.0, 380.0, 146.0),
    }
}

fn international_layout() -> Layout {
    let mut stands: Vec<Stand> = (0..10).map(|i| Stand { at: Vec2::new(-279.0 + i as f32 * 62.0, 92.0), bridge: true, cargo: false }).collect();
    stands.extend((0..3).map(|i| Stand { at: Vec2::new(470.0 + i as f32 * 64.0, 92.0), bridge: false, cargo: true }));
    Layout {
        runways: AirportSize::International.runways().to_vec(),
        arrivals: 0,
        departures: 1,
        taxiways: vec![(-185.0, -770.0, 770.0, 23.0), (-62.0, -770.0, 770.0, 23.0)],
        links: vec![
            (-770.0, -245.0, -62.0),
            (-470.0, -245.0, -62.0),
            (-160.0, -245.0, -62.0),
            (160.0, -245.0, -62.0),
            (470.0, -245.0, -62.0),
            (770.0, -245.0, -62.0),
            (-330.0, -62.0, 30.0),
            (0.0, -62.0, 30.0),
            (330.0, -62.0, 30.0),
            (560.0, -62.0, 30.0),
        ],
        apron: Rect::new(-360.0, -40.0, 680.0, 135.0),
        apron_lane: 30.0,
        stands,
        terminal: Rect::new(-345.0, 135.0, 345.0, 205.0),
        terminal_height: 21.0,
        tower: Vec2::new(392.0, 170.0),
        tower_height: 52.0,
        hangars: vec![Rect::new(-760.0, -30.0, -620.0, 120.0), Rect::new(-600.0, -10.0, -480.0, 120.0)],
        cargo_shed: Rect::new(430.0, 135.0, 690.0, 205.0),
        car_parks: vec![(Rect::new(-345.0, 215.0, -130.0, 290.0), 5), (Rect::new(-110.0, 222.0, 345.0, 290.0), 1), (Rect::new(-620.0, 140.0, -420.0, 290.0), 1)],
        fuel_farm: Rect::new(720.0, 140.0, 860.0, 290.0),
    }
}

// ── one airport ────────────────────────────────────────────────────────────

/// Where a plane is in its visit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanePhase {
    /// On final over the map edge down to the runway.
    Approach,
    /// Touched down, braking along the runway.
    Rollout,
    TaxiIn,
    /// On its stand: passengers, bags, fuel.
    AtStand,
    /// Pushed back and taxiing to the runway end.
    TaxiOut,
    /// At the runway end, waiting for the runway.
    HoldShort,
    Takeoff,
    /// Off the ground, climbing out over the map edge.
    Climb,
}

impl PlanePhase {
    pub fn label(self) -> &'static str {
        match self {
            PlanePhase::Approach => "on approach",
            PlanePhase::Rollout => "landing",
            PlanePhase::TaxiIn => "taxiing in",
            PlanePhase::AtStand => "at the stand",
            PlanePhase::TaxiOut => "taxiing out",
            PlanePhase::HoldShort => "holding short",
            PlanePhase::Takeoff => "taking off",
            PlanePhase::Climb => "climbing out",
        }
    }

    pub fn airborne(self) -> bool {
        matches!(self, PlanePhase::Approach | PlanePhase::Climb)
    }
}

/// A timetabled visit: in from a town off the map, out again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rotation {
    pub flight: u32,
    pub airline: u8,
    pub from: u8,
    pub to: u8,
    pub kind: PlaneKind,
    /// Timetabled touchdown and take-off ticks.
    pub arrive: u64,
    pub depart: u64,
    /// Share of the seats taken, each way.
    pub load: f32,
    /// On its way in (a plane flies it), done, or cancelled.
    pub started: bool,
    pub cancelled: bool,
}

impl Rotation {
    pub fn airline_name(&self) -> &'static str {
        AIRLINES[self.airline as usize % AIRLINES.len()]
    }
    pub fn call_sign(&self) -> String {
        format!("{} {}", self.airline_name(), self.flight)
    }
}

/// A plane on or over the field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub kind: PlaneKind,
    pub airline: u8,
    pub flight: u32,
    pub from: u8,
    pub to: u8,
    pub phase: PlanePhase,
    /// Tick the phase began and the tick it ends (`u64::MAX`: until let go).
    pub since: u64,
    pub until: u64,
    /// The stand it is given (index into `Layout::stands`).
    pub stand: u8,
    /// +1 or -1: which way along the runways it lands and takes off.
    pub dir: f32,
    pub depart: u64,
    pub pax_in: u32,
    pub pax_out: u32,
    pub cargo_in: f32,
    /// Minutes it was late in (and so far out).
    pub late_in: u32,
    /// Approach and climb-out lengths it flies (to beyond the map edge).
    pub approach_len: f32,
    pub climb_len: f32,
}

impl Plane {
    pub fn airline_name(&self) -> &'static str {
        AIRLINES[self.airline as usize % AIRLINES.len()]
    }
    pub fn call_sign(&self) -> String {
        format!("{} {}", self.airline_name(), self.flight)
    }
    pub fn from_city(&self) -> &'static str {
        CITIES[self.from as usize % CITIES.len()]
    }
    pub fn to_city(&self) -> &'static str {
        CITIES[self.to as usize % CITIES.len()]
    }
}

/// An import flown in for a business in town.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AirLoad {
    pub cargo: Cargo,
    pub since: u64,
}

/// How passengers got to and from the airport, by `Access` index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Access {
    Car,
    Taxi,
    Coach,
    Rail,
    Metro,
    Bus,
}

impl Access {
    pub const ALL: [Access; 6] = [Access::Car, Access::Taxi, Access::Coach, Access::Rail, Access::Metro, Access::Bus];
    pub fn label(self) -> &'static str {
        match self {
            Access::Car => "car",
            Access::Taxi => "taxi",
            Access::Coach => "airport coach",
            Access::Rail => "rail",
            Access::Metro => "metro",
            Access::Bus => "bus",
        }
    }
}

/// One day's numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayStats {
    pub arrivals: u32,
    pub departures: u32,
    pub pax_in: u32,
    pub pax_out: u32,
    /// Of those coming in: visitors who checked into a hotel, and business
    /// travellers; of those going out, residents off on a trip.
    pub tourists_in: u32,
    pub business: u32,
    pub residents_out: u32,
    pub cargo_in: f32,
    pub cargo_out: f32,
    pub fees: f32,
    pub delayed: u32,
    pub delay_minutes: u32,
    pub cancelled: u32,
    pub complaints: u32,
    /// Passengers by way to and from the terminal (`Access`).
    pub access: [u32; 6],
}

impl DayStats {
    pub fn movements(&self) -> u32 {
        self.arrivals + self.departures
    }
    pub fn avg_delay(&self) -> f32 {
        let n = self.arrivals + self.departures;
        if n == 0 { 0.0 } else { self.delay_minutes as f32 / n as f32 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AirportStats {
    pub today: DayStats,
    pub yesterday: DayStats,
    pub pax_total: u64,
    pub movements_total: u64,
    /// Homes under the flight paths loud enough to complain (as of midnight).
    pub homes_in_noise: u32,
    /// Passengers a day the city would fly, as of the last timetable.
    pub demand: f32,
}

/// One airport: where it lies, what it has on the field, its timetable.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Airport {
    pub size: AirportSize,
    pub center: Vec2,
    /// Unit direction of the runways (+x of its own frame).
    pub along: Vec2,
    /// The levelled ground's height.
    pub level: f32,
    pub name: String,
    #[serde(default)]
    pub building: Option<BuildingId>,
    #[serde(default)]
    pub lot: Option<LotId>,
    /// The forecourt road laid with it, if no street ran along its front.
    #[serde(default)]
    pub stub: Option<[Vec2; 2]>,
    #[serde(default)]
    pub road_connected: bool,
    /// +1 or -1: which way it lands and takes off (into the wind).
    #[serde(default = "one")]
    pub dir: f32,
    #[serde(default)]
    pub rotations: Vec<Rotation>,
    #[serde(default)]
    pub planes: Vec<Plane>,
    /// Imports waiting for a flight in, and landed ones waiting for a lorry.
    #[serde(default)]
    pub cargo_waiting: Vec<AirLoad>,
    #[serde(default)]
    pub cargo_landed: Vec<AirLoad>,
    /// Export units at the cargo terminal for the next flights out.
    #[serde(default)]
    pub cargo_out: f32,
    /// Its cars on the road (passengers to and from the terminal).
    #[serde(default)]
    pub cars: Vec<CarId>,
    #[serde(default)]
    pub stats: AirportStats,
    #[serde(default)]
    pub next_flight: u32,
    /// What levelling its site cost (paid on top of the building).
    #[serde(default)]
    pub earthwork_cost: f32,
    /// Metres of street a plan clears off its site (`plan_airport_with`).
    #[serde(default)]
    pub cleared_m: f32,
    /// One end closed by hills: planes always land along this direction
    /// (towards the closed end) and take off the other way.
    #[serde(default)]
    pub one_way: Option<f32>,
}

fn one() -> f32 {
    1.0
}

impl Airport {
    pub fn new(size: AirportSize, center: Vec2, along: Vec2, level: f32, name: String) -> Airport {
        Airport {
            size,
            center,
            along: along.normalize_or(Vec2::X),
            level,
            name,
            building: None,
            lot: None,
            stub: None,
            road_connected: false,
            dir: 1.0,
            rotations: Vec::new(),
            planes: Vec::new(),
            cargo_waiting: Vec::new(),
            cargo_landed: Vec::new(),
            cargo_out: 0.0,
            cars: Vec::new(),
            stats: AirportStats::default(),
            next_flight: 100,
            earthwork_cost: 0.0,
            cleared_m: 0.0,
            one_way: None,
        }
    }

    /// Across the runways, towards the terminal.
    pub fn across(&self) -> Vec2 {
        Vec2::new(-self.along.y, self.along.x)
    }

    /// A point of its own frame in the world.
    pub fn world(&self, local: Vec2) -> Vec2 {
        self.center + self.along * local.x + self.across() * local.y
    }

    /// A world point in its own frame.
    pub fn local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        Vec2::new(d.dot(self.along), d.dot(self.across()))
    }

    /// A heading in its own frame (radians, from +x) as a world heading.
    pub fn world_heading(&self, local: f32) -> f32 {
        local + self.along.y.atan2(self.along.x)
    }

    /// Corners: front-left, front-right, back-right, back-left, with the
    /// landside (+y) as the front, as the lot code expects them.
    pub fn corners(&self) -> [Vec2; 4] {
        let (w, d) = self.size.size();
        let (hw, hd) = (w * 0.5, d * 0.5);
        [self.world(Vec2::new(-hw, hd)), self.world(Vec2::new(hw, hd)), self.world(Vec2::new(hw, -hd)), self.world(Vec2::new(-hw, -hd))]
    }

    pub fn footprint(&self) -> Vec<Vec2> {
        self.corners().to_vec()
    }

    /// Middle of the landside edge: the terminal's door.
    pub fn front_mid(&self) -> Vec2 {
        self.world(Vec2::new(0.0, self.size.size().1 * 0.5))
    }

    /// The forecourt road along the landside.
    pub fn forecourt(&self) -> [Vec2; 2] {
        let (w, d) = self.size.size();
        let y = d * 0.5 + crate::rail::ROAD_GAP;
        let hw = w * 0.5 - 40.0;
        [self.world(Vec2::new(-hw, y)), self.world(Vec2::new(hw, y))]
    }

    pub fn layout(&self) -> Layout {
        self.size.layout()
    }

    /// The runway ends (world) of runway `r`: the `-x` end first.
    pub fn runway_ends(&self, r: &Runway) -> [Vec2; 2] {
        [self.world(Vec2::new(-r.length * 0.5, r.y)), self.world(Vec2::new(r.length * 0.5, r.y))]
    }

    /// Stands taken by a plane on or bound for them.
    pub fn stand_taken(&self, s: usize) -> bool {
        self.planes.iter().any(|p| p.stand as usize == s && !matches!(p.phase, PlanePhase::TaxiOut | PlanePhase::HoldShort | PlanePhase::Takeoff | PlanePhase::Climb))
    }

    /// A free stand for `kind` (freighters on the freight stands; the rest
    /// on the passenger ones). Lowest first, so stands fill from one end.
    pub fn free_stand(&self, kind: PlaneKind) -> Option<usize> {
        let lay = self.layout();
        let cargo = kind == PlaneKind::Freighter;
        (0..lay.stands.len()).find(|&s| lay.stands[s].cargo == cargo && !self.stand_taken(s))
    }

    /// The height limit the approach surfaces put at `p`, metres above the
    /// runways' level, if `p` is under one.
    pub fn approach_limit(&self, p: Vec2) -> Option<f32> {
        approach_limit(self.size, self.center, self.along, p)
    }

    /// The line it flies on final, and out: the runway end, the far point
    /// out along the centreline (`len` metres out) for runway `r` landing
    /// (or taking off) along `dir`.
    pub fn final_line(&self, r: &Runway, dir: f32, len: f32) -> (Vec2, Vec2) {
        let th = self.world(Vec2::new(-dir * r.length * 0.5, r.y));
        (th, th - self.along * dir * len)
    }
}

/// The approach-surface limit for an airport of `size` at `center` along
/// `along` (see `Airport::approach_limit`).
pub fn approach_limit(size: AirportSize, center: Vec2, along: Vec2, p: Vec2) -> Option<f32> {
    let across = Vec2::new(-along.y, along.x);
    let d = p - center;
    let (x, y) = (d.dot(along), d.dot(across));
    let mut best: Option<f32> = None;
    for r in size.runways() {
        let half = r.length * 0.5;
        let out = x.abs() - half - APPROACH_START;
        if out < 0.0 || out > APPROACH_LENGTH {
            continue;
        }
        let inner = if size == AirportSize::International { 150.0 } else { 75.0 };
        let half_w = inner + out * APPROACH_SPLAY;
        if (y - r.y).abs() > half_w {
            continue;
        }
        let lim = out * APPROACH_SLOPE;
        best = Some(best.map_or(lim, |b: f32| b.min(lim)));
    }
    best
}

/// The approach surfaces as quads (world), for drawing: per runway end,
/// the near edge's two corners and the far edge's two.
pub fn approach_quads(size: AirportSize, center: Vec2, along: Vec2, reach: f32) -> Vec<[Vec2; 4]> {
    let across = Vec2::new(-along.y, along.x);
    let inner = if size == AirportSize::International { 150.0 } else { 75.0 };
    let mut out = Vec::new();
    for r in size.runways() {
        for s in [-1.0f32, 1.0] {
            let x0 = s * (r.length * 0.5 + APPROACH_START);
            let x1 = s * (r.length * 0.5 + APPROACH_START + reach);
            let w1 = inner + reach * APPROACH_SPLAY;
            let p = |x: f32, y: f32| center + along * x + across * (r.y + y);
            out.push([p(x0, -inner), p(x0, inner), p(x1, w1), p(x1, -w1)]);
        }
    }
    out
}

// ── the city's demand and the timetable ───────────────────────────────────

/// Passengers a day each way the city would fly: residents off on trips,
/// visitors (`TOURIST_AIR_SHARE` of the tourist demand) and business
/// travellers (by office jobs).
pub fn daily_pax(residents: usize, tourist_demand: f32, office_jobs: u32) -> f32 {
    residents as f32 * RESIDENT_TRIPS + tourist_demand.max(0.0) * TOURIST_AIR_SHARE + office_jobs as f32 * BUSINESS_TRIPS
}

/// A small mixing hash for the timetable (no world RNG draws).
pub fn mix(a: u64, b: u64) -> u64 {
    let mut x = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// The day's rotations for an airport of `size`: enough seats for `pax`
/// passengers a day each way (within the size's fewest and most), plus a
/// freighter or two for `cargo` units a day. Arrivals are spread over the
/// day from `FIRST_ARRIVAL` to `LAST_ARRIVAL` with morning and evening
/// banks, only those after `from_tick` (a timetable made mid-day starts
/// now). Deterministic in `seed` and `day`.
pub fn timetable(size: AirportSize, day: u64, from_tick: u64, pax: f32, cargo: f32, seed: u64, first_flight: u32) -> Vec<Rotation> {
    let fleet = size.fleet();
    let avg_seats = fleet.iter().map(|k| k.seats() as f32).sum::<f32>() / fleet.len() as f32;
    let (lo, hi) = size.rotations();
    let wanted = (pax / (avg_seats * 0.8)).round() as u32;
    let n = wanted.clamp(lo, hi);
    let load = (pax / (n as f32 * avg_seats).max(1.0)).clamp(0.4, 0.96);
    let freighters = if size == AirportSize::International { ((cargo / 120.0).ceil() as u32).clamp(1, 4) } else if cargo > 20.0 { 1 } else { 0 };
    let day0 = day * 1440;
    let span = (LAST_ARRIVAL - FIRST_ARRIVAL) as f32;
    let mut out = Vec::new();
    let total = n + freighters;
    for i in 0..total {
        let h = mix(seed ^ day.wrapping_mul(0x5851_F42D), i as u64 + 1);
        let freight = i >= n;
        // Banks: the passenger flights bunch round 08:00 and 18:00.
        let u = (i as f32 + 0.5) / n.max(1) as f32;
        let bank = u + 0.06 * (u * std::f32::consts::TAU * 2.0).sin();
        let minute = if freight {
            // Freighters come at the edges of the day.
            FIRST_ARRIVAL as f32 + span * [0.02, 0.97, 0.5, 0.85][((i - n) % 4) as usize]
        } else {
            FIRST_ARRIVAL as f32 + span * bank.clamp(0.0, 1.0) + (unit(h) - 0.5) * 12.0
        };
        let arrive = day0 + minute.clamp(FIRST_ARRIVAL as f32, LAST_ARRIVAL as f32) as u64;
        if arrive < from_tick {
            continue;
        }
        let kind = if freight { PlaneKind::Freighter } else { fleet[(mix(h, 7) % fleet.len() as u64) as usize] };
        let airline = if freight { (PASSENGER_AIRLINES + (mix(h, 3) % 2) as usize) as u8 } else { (mix(h, 3) % PASSENGER_AIRLINES as u64) as u8 };
        let towns = if size == AirportSize::International { CITIES.len() } else { 6 };
        let from = (mix(h, 5) % towns as u64) as u8;
        let to = ((from as u64 + 1 + mix(h, 9) % (towns as u64 - 1)) % towns as u64) as u8;
        let depart = arrive + kind.turnaround() + 25 + mix(h, 11) % 20;
        out.push(Rotation { flight: first_flight + i * 2, airline, from, to, kind, arrive, depart, load: if freight { 1.0 } else { (load + (unit(mix(h, 13)) - 0.5) * 0.1).clamp(0.3, 1.0) }, started: false, cancelled: false });
    }
    out.sort_by_key(|r| (r.arrive, r.flight));
    out
}

// ── where a plane is ──────────────────────────────────────────────────────

/// Where a plane is and how it sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanePose {
    pub pos: Vec2,
    /// Height above the field's level.
    pub alt: f32,
    /// World heading, radians from +x (the way the nose points).
    pub heading: f32,
    /// Nose up, radians.
    pub pitch: f32,
    /// Ground speed, m/s.
    pub speed: f32,
    pub gear_down: bool,
}

/// A path of points in the airport's own frame, walked at constant speed.
/// The first `reverse` metres are pushed back (the nose stays put).
#[derive(Clone, Debug, Default)]
pub struct TaxiPath {
    pub pts: Vec<Vec2>,
    pub reverse: f32,
}

impl TaxiPath {
    pub fn length(&self) -> f32 {
        self.pts.windows(2).map(|w| w[0].distance(w[1])).sum()
    }

    /// Point and direction of travel `s` metres along.
    pub fn at(&self, s: f32) -> (Vec2, Vec2) {
        let mut left = s.max(0.0);
        for w in self.pts.windows(2) {
            let len = w[0].distance(w[1]);
            if left <= len || len < 1e-3 {
                let d = (w[1] - w[0]).normalize_or(Vec2::X);
                return (w[0] + d * left.min(len), d);
            }
            left -= len;
        }
        let n = self.pts.len();
        if n >= 2 {
            let d = (self.pts[n - 1] - self.pts[n - 2]).normalize_or(Vec2::X);
            (self.pts[n - 1], d)
        } else {
            (self.pts.first().copied().unwrap_or_default(), Vec2::X)
        }
    }

    /// The nose's direction `s` along: the travel direction smoothed round
    /// the corners (looking a little ahead and behind), against it while
    /// pushed back.
    pub fn facing(&self, s: f32) -> Vec2 {
        if s < self.reverse {
            return -self.at(s).1;
        }
        let a = self.at(s - 9.0).0;
        let b = self.at(s + 9.0).0;
        let d = b - a;
        if d.length() < 1e-3 { self.at(s).1 } else { d.normalize() }
    }
}

/// The taxi route in: off the arrival runway where it slowed down, along
/// the taxiways, over to the stand's lane and nose in.
pub fn taxi_in_path(size: AirportSize, dir: f32, stand: usize) -> TaxiPath {
    let lay = size.layout();
    let r = lay.runways[lay.arrivals];
    let st = lay.stands[stand.min(lay.stands.len() - 1)];
    let exit_x = rollout_end_x(&r, dir);
    // The link nearest where it stopped, off towards the terminal.
    let first_twy = lay.taxiways.iter().map(|t| t.0).filter(|y| *y > r.y).fold(f32::MAX, f32::min);
    let x_off = lay.link_near(exit_x, r.y, first_twy);
    let mut pts = vec![Vec2::new(exit_x, r.y), Vec2::new(x_off, r.y), Vec2::new(x_off, first_twy)];
    // Along the taxiway to the link up to the apron nearest the stand.
    let lane = lay.apron_lane;
    let apron_twy = lay.taxiways.iter().map(|t| t.0).fold(f32::MIN, f32::max);
    if apron_twy > first_twy + 1.0 {
        let xc = lay.link_near(st.at.x, first_twy, apron_twy);
        pts.push(Vec2::new(xc, first_twy));
        pts.push(Vec2::new(xc, apron_twy));
    }
    let xa = lay.link_near(st.at.x, apron_twy, lane);
    pts.push(Vec2::new(xa, apron_twy));
    pts.push(Vec2::new(xa, lane));
    pts.push(Vec2::new(st.at.x, lane));
    pts.push(st.at);
    dedup(&mut pts);
    TaxiPath { pts, reverse: 0.0 }
}

/// The route out: pushed back off the stand onto the lane, along to the
/// apron link, down to the departure runway's taxiway, along it to the
/// runway end it takes off from, and lined up there.
pub fn taxi_out_path(size: AirportSize, dir: f32, stand: usize) -> TaxiPath {
    let lay = size.layout();
    let r = lay.runways[lay.departures];
    let st = lay.stands[stand.min(lay.stands.len() - 1)];
    let lane = lay.apron_lane;
    let apron_twy = lay.taxiways.iter().map(|t| t.0).fold(f32::MIN, f32::max);
    let start_x = -dir * r.length * 0.5;
    let back = (st.at.y - lane).abs();
    let xa = lay.link_near(st.at.x - dir * 40.0, apron_twy, lane);
    let mut pts = vec![st.at, Vec2::new(st.at.x, lane), Vec2::new(xa, lane), Vec2::new(xa, apron_twy)];
    // The taxiway beside the departure runway (the one just landside of it).
    let twy = lay.taxiways.iter().map(|t| t.0).filter(|y| *y > r.y).fold(f32::MAX, f32::min);
    if (twy - apron_twy).abs() > 1.0 {
        let xc = lay.link_near(xa, twy, apron_twy);
        pts.push(Vec2::new(xc, apron_twy));
        pts.push(Vec2::new(xc, twy));
    }
    // To the end and onto the runway.
    let end_link = lay.link_near(start_x, r.y, twy);
    pts.push(Vec2::new(end_link, twy));
    pts.push(Vec2::new(end_link, r.y));
    pts.push(Vec2::new(start_x + dir * 20.0, r.y));
    dedup(&mut pts);
    TaxiPath { pts, reverse: back }
}

fn dedup(pts: &mut Vec<Vec2>) {
    pts.dedup_by(|a, b| a.distance(*b) < 0.5);
}

/// Where along the arrival runway (its own x) a landing has slowed to a
/// taxi: two thirds of the way down.
pub fn rollout_end_x(r: &Runway, dir: f32) -> f32 {
    // The threshold is at -dir * length / 2; 0.68 of the length on from it.
    dir * r.length * 0.18
}

/// Touchdown, along the runway from its threshold.
pub const TOUCHDOWN: f32 = 180.0;

/// Ticks each phase lasts at `tsp` seconds of motion per tick.
pub fn phase_ticks(size: AirportSize, phase: PlanePhase, plane: &Plane, tsp: f32) -> u64 {
    let lay = size.layout();
    let tsp = tsp.max(0.1);
    let t = |metres: f32, mps: f32| ((metres / (mps * tsp)).ceil() as u64).max(1);
    match phase {
        PlanePhase::Approach => t(plane.approach_len + TOUCHDOWN, APPROACH_MPS),
        PlanePhase::Rollout => {
            let r = lay.runways[lay.arrivals];
            let d = (rollout_end_x(&r, plane.dir) - (-plane.dir * r.length * 0.5 + plane.dir * TOUCHDOWN)).abs();
            t(d, APPROACH_MPS * 0.5)
        }
        PlanePhase::TaxiIn => t(taxi_in_path(size, plane.dir, plane.stand as usize).length(), TAXI_MPS),
        PlanePhase::TaxiOut => {
            let p = taxi_out_path(size, plane.dir, plane.stand as usize);
            t(p.reverse, PUSHBACK_MPS) + t(p.length() - p.reverse, TAXI_MPS)
        }
        PlanePhase::Takeoff => t(takeoff_roll(size), LIFTOFF_MPS * 0.5),
        PlanePhase::Climb => t(plane.climb_len, CLIMB_MPS),
        PlanePhase::AtStand | PlanePhase::HoldShort => 1,
    }
}

/// Metres of runway a take-off rolls before it lifts off.
pub fn takeoff_roll(size: AirportSize) -> f32 {
    let lay = size.layout();
    lay.runways[lay.departures].length * 0.55
}

/// Where `plane` of the airport `apt` is at `t` (ticks, with the fraction
/// to the next): a pure function of its phase, so the sim and the
/// renderer agree and nothing but the phase is saved.
/// How far through its phase `plane` is at `t`, 0..1.
pub fn phase_u(plane: &Plane, t: f32) -> f32 {
    if plane.until == u64::MAX || plane.until <= plane.since { 0.0 } else { ((t - plane.since as f32) / (plane.until - plane.since) as f32).clamp(0.0, 1.0) }
}

/// Is `plane` being pushed back off its stand at `t` (a tug at its nose)?
pub fn pushing_back(apt: &Airport, plane: &Plane, t: f32) -> bool {
    if plane.phase != PlanePhase::TaxiOut {
        return false;
    }
    let path = taxi_out_path(apt.size, plane.dir, plane.stand as usize);
    let rev = path.reverse / PUSHBACK_MPS;
    let fwd = (path.length() - path.reverse) / TAXI_MPS;
    phase_u(plane, t) * (rev + fwd) < rev
}

pub fn plane_pose(apt: &Airport, plane: &Plane, t: f32) -> PlanePose {
    let lay = apt.layout();
    let dir = plane.dir;
    let u = phase_u(plane, t);
    let base = apt.world_heading(0.0);
    let heading_of = |local: Vec2| apt.world_heading(local.y.atan2(local.x));
    let along_heading = if dir > 0.0 { base } else { base + std::f32::consts::PI };
    match plane.phase {
        PlanePhase::Approach => {
            let r = lay.runways[lay.arrivals];
            let th = Vec2::new(-dir * r.length * 0.5, r.y);
            let total = plane.approach_len + TOUCHDOWN;
            // Distance still to fly to the touchdown point.
            let to_go = total * (1.0 - u);
            let x = th.x + dir * (TOUCHDOWN - to_go);
            let past_th = TOUCHDOWN - to_go;
            let alt = if past_th < 0.0 { 15.0 + (-past_th) * GLIDE } else { 15.0 * (1.0 - past_th / TOUCHDOWN).max(0.0).powf(1.6) };
            PlanePose { pos: apt.world(Vec2::new(x, r.y)), alt, heading: along_heading, pitch: if past_th < 0.0 { 0.05 } else { 0.09 }, speed: APPROACH_MPS, gear_down: to_go < 2600.0 }
        }
        PlanePhase::Rollout => {
            let r = lay.runways[lay.arrivals];
            let x0 = -dir * r.length * 0.5 + dir * TOUCHDOWN;
            let x1 = rollout_end_x(&r, dir);
            let k = 1.0 - (1.0 - u) * (1.0 - u);
            let x = x0 + (x1 - x0) * k;
            PlanePose { pos: apt.world(Vec2::new(x, r.y)), alt: 0.0, heading: along_heading, pitch: (0.06 * (1.0 - u * 6.0)).max(0.0), speed: APPROACH_MPS * (1.0 - u).max(0.1), gear_down: true }
        }
        PlanePhase::TaxiIn => {
            let path = taxi_in_path(apt.size, dir, plane.stand as usize);
            let s = path.length() * u;
            let (p, _) = path.at(s);
            PlanePose { pos: apt.world(p), alt: 0.0, heading: heading_of(path.facing(s)), pitch: 0.0, speed: TAXI_MPS, gear_down: true }
        }
        PlanePhase::AtStand => {
            let st = lay.stands[(plane.stand as usize).min(lay.stands.len() - 1)];
            PlanePose { pos: apt.world(st.at), alt: 0.0, heading: apt.world_heading(std::f32::consts::FRAC_PI_2), pitch: 0.0, speed: 0.0, gear_down: true }
        }
        PlanePhase::TaxiOut => {
            let path = taxi_out_path(apt.size, dir, plane.stand as usize);
            // Pushback slowly, then taxi: time split by the speeds.
            let tsp_rev = path.reverse / PUSHBACK_MPS;
            let tsp_fwd = (path.length() - path.reverse) / TAXI_MPS;
            let tt = u * (tsp_rev + tsp_fwd);
            let s = if tt < tsp_rev { tt * PUSHBACK_MPS } else { path.reverse + (tt - tsp_rev) * TAXI_MPS };
            let (p, _) = path.at(s);
            PlanePose { pos: apt.world(p), alt: 0.0, heading: heading_of(path.facing(s)), pitch: 0.0, speed: if tt < tsp_rev { PUSHBACK_MPS } else { TAXI_MPS }, gear_down: true }
        }
        PlanePhase::HoldShort | PlanePhase::Takeoff => {
            let r = lay.runways[lay.departures];
            let x0 = -dir * r.length * 0.5 + dir * 20.0;
            let roll = takeoff_roll(apt.size);
            let k = if plane.phase == PlanePhase::HoldShort { 0.0 } else { u * u };
            let pitch = if plane.phase == PlanePhase::Takeoff { (u - 0.8).max(0.0) * 0.6 } else { 0.0 };
            PlanePose { pos: apt.world(Vec2::new(x0 + dir * roll * k, r.y)), alt: 0.0, heading: along_heading, pitch, speed: LIFTOFF_MPS * u, gear_down: true }
        }
        PlanePhase::Climb => {
            let r = lay.runways[lay.departures];
            let x0 = -dir * r.length * 0.5 + dir * (20.0 + takeoff_roll(apt.size));
            let d = plane.climb_len * u;
            // A little shallower in the first few hundred metres.
            let alt = d * CLIMB * (d / (d + 250.0)).max(0.15);
            PlanePose { pos: apt.world(Vec2::new(x0 + dir * d, r.y)), alt, heading: along_heading, pitch: 0.15, speed: CLIMB_MPS, gear_down: d < 500.0 }
        }
    }
}

// ── everything ─────────────────────────────────────────────────────────────

/// Every airport in town.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Airports {
    pub airports: SlotMap<AirportId, Airport>,
    /// Buildings with people in, for the cars to and from the terminal
    /// (made again each day; not saved).
    #[serde(skip)]
    pub targets: Vec<BuildingId>,
    #[serde(skip)]
    pub targets_day: u64,
}

impl Airports {
    pub fn by_building(&self, b: BuildingId) -> Option<(AirportId, &Airport)> {
        self.airports.iter().find(|(_, a)| a.building == Some(b))
    }

    pub fn is_empty(&self) -> bool {
        self.airports.is_empty()
    }

    /// The tightest approach-surface limit at `p` over every airport, in
    /// metres above sea level (its level plus the surface's height there).
    pub fn height_limit(&self, p: Vec2) -> Option<f32> {
        self.airports.values().filter_map(|a| a.approach_limit(p).map(|l| l + a.level)).reduce(f32::min)
    }
}

/// Goods flown in: dear and wanted fast. Consumer goods only in part, at
/// an international airport (`GOODS_BY_AIR`).
pub fn flies(r: Resource) -> bool {
    matches!(r, Resource::Electronics | Resource::Art)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apt(size: AirportSize) -> Airport {
        Airport::new(size, Vec2::new(100.0, -50.0), Vec2::new(0.8, 0.6), 2.0, "Test Field".into())
    }

    #[test]
    fn frames_round_trip_and_the_terminal_faces_landside() {
        let a = apt(AirportSize::International);
        let p = Vec2::new(321.0, -77.0);
        assert!(a.world(a.local(p)).distance(p) < 1e-3);
        let lay = a.layout();
        assert!(lay.terminal.y0 > lay.apron_lane && lay.runways.iter().all(|r| r.y < lay.apron.y0));
        let (w, d) = a.size.size();
        for r in &lay.runways {
            assert!(r.length < w - 40.0 && r.y.abs() < d * 0.5);
        }
        for s in &lay.stands {
            assert!(s.at.x > lay.apron.x0 && s.at.x < lay.apron.x1 && s.at.y > lay.apron.y0 && s.at.y < lay.apron.y1, "{s:?}");
        }
    }

    #[test]
    fn the_approach_surface_rises_away_from_the_runway() {
        for size in AirportSize::ALL {
            let a = apt(size);
            let r = a.layout().runways[0];
            let out = |m: f32| a.world(Vec2::new(r.length * 0.5 + APPROACH_START + m, r.y));
            let near = a.approach_limit(out(200.0)).unwrap();
            let far = a.approach_limit(out(2000.0)).unwrap();
            assert!(near < far && (far - 2000.0 * APPROACH_SLOPE).abs() < 1.0, "{near} {far}");
            assert!(a.approach_limit(a.world(Vec2::new(r.length * 0.5 + 500.0, r.y + 2000.0))).is_none(), "well off to the side");
            assert!(a.approach_limit(a.center).is_none(), "on the field itself");
        }
    }

    #[test]
    fn timetables_grow_with_demand_and_are_deterministic() {
        let small = timetable(AirportSize::International, 3, 0, 200.0, 0.0, 9, 100);
        let big = timetable(AirportSize::International, 3, 0, 9000.0, 300.0, 9, 100);
        assert!(big.len() > small.len() * 3, "{} vs {}", big.len(), small.len());
        assert_eq!(big, timetable(AirportSize::International, 3, 0, 9000.0, 300.0, 9, 100));
        assert_ne!(big, timetable(AirportSize::International, 4, 0, 9000.0, 300.0, 9, 100).into_iter().map(|mut r| {
            r.arrive -= 1440;
            r.depart -= 1440;
            r
        }).collect::<Vec<_>>());
        let (lo, hi) = AirportSize::International.rotations();
        assert!(small.iter().filter(|r| r.kind != PlaneKind::Freighter).count() as u32 >= lo);
        assert!(big.iter().filter(|r| r.kind != PlaneKind::Freighter).count() as u32 <= hi);
        assert!(big.iter().any(|r| r.kind == PlaneKind::Freighter));
        for r in &big {
            let m = (r.arrive % 1440) as u32;
            assert!((FIRST_ARRIVAL..=LAST_ARRIVAL).contains(&m), "arrives at {m}");
            assert!(r.depart >= r.arrive + r.kind.turnaround());
            assert_ne!(r.from, r.to);
        }
        // Mid-day: only what is still to come.
        let late = timetable(AirportSize::International, 3, 3 * 1440 + 15 * 60, 9000.0, 300.0, 9, 100);
        assert!(late.len() < big.len() && late.iter().all(|r| r.arrive >= 3 * 1440 + 15 * 60));
        // The airfield takes only its own planes, to the near towns.
        for r in timetable(AirportSize::Airfield, 1, 0, 3000.0, 40.0, 2, 100) {
            assert!(r.kind == PlaneKind::Freighter || AirportSize::Airfield.fleet().contains(&r.kind));
            assert!(r.from < 6 && r.to < 6);
        }
    }

    #[test]
    fn demand_rises_with_people_visitors_and_offices() {
        let base = daily_pax(10_000, 50.0, 2_000);
        assert!(daily_pax(20_000, 50.0, 2_000) > base);
        assert!(daily_pax(10_000, 150.0, 2_000) > base);
        assert!(daily_pax(10_000, 50.0, 6_000) > base);
    }

    #[test]
    fn taxi_paths_join_the_runway_to_the_stand() {
        for size in AirportSize::ALL {
            let lay = size.layout();
            for dir in [1.0f32, -1.0] {
                for (s, st) in lay.stands.iter().enumerate() {
                    let p = taxi_in_path(size, dir, s);
                    assert_eq!(*p.pts.last().unwrap(), st.at);
                    assert!((p.pts[0].y - lay.runways[lay.arrivals].y).abs() < 0.01);
                    assert!(p.length() > 50.0);
                    let q = taxi_out_path(size, dir, s);
                    assert_eq!(q.pts[0], st.at);
                    let end = *q.pts.last().unwrap();
                    assert!((end.y - lay.runways[lay.departures].y).abs() < 0.01);
                    assert!(end.x * dir < 0.0, "takes off from the far end, into the wind");
                    // Pushed back nose to the terminal.
                    assert!(q.facing(1.0).y > 0.9);
                }
            }
        }
    }

    #[test]
    fn a_plane_flies_in_over_the_edge_lands_and_climbs_out_the_other_way() {
        let a = apt(AirportSize::International);
        let mut p = Plane { kind: PlaneKind::Narrowbody, airline: 0, flight: 101, from: 0, to: 1, phase: PlanePhase::Approach, since: 0, until: 0, stand: 2, dir: 1.0, depart: 0, pax_in: 0, pax_out: 0, cargo_in: 0.0, late_in: 0, approach_len: 3000.0, climb_len: 3000.0 };
        p.until = phase_ticks(a.size, PlanePhase::Approach, &p, 4.0);
        let start = plane_pose(&a, &p, 0.0);
        let end = plane_pose(&a, &p, p.until as f32);
        assert!(start.alt > 140.0 && end.alt < 0.5, "{} {}", start.alt, end.alt);
        assert!(start.gear_down == false && end.gear_down);
        let lay = a.layout();
        let r = lay.runways[lay.arrivals];
        let th = a.world(Vec2::new(-r.length * 0.5, r.y));
        assert!(a.local(start.pos).x < a.local(th).x - 2500.0, "comes in from the -x side when landing towards +x");
        // Rolls on towards +x and slows.
        p.phase = PlanePhase::Rollout;
        p.until = phase_ticks(a.size, PlanePhase::Rollout, &p, 4.0);
        let r0 = plane_pose(&a, &p, 0.0);
        let r1 = plane_pose(&a, &p, p.until as f32);
        assert!(a.local(r1.pos).x > a.local(r0.pos).x && r1.speed < r0.speed);
        // At the stand, nose in.
        p.phase = PlanePhase::AtStand;
        let at = plane_pose(&a, &p, 0.0);
        assert!(at.pos.distance(a.world(lay.stands[2].at)) < 0.01);
        // The climb goes out along +x and up.
        p.phase = PlanePhase::Climb;
        p.since = 0;
        p.until = phase_ticks(a.size, PlanePhase::Climb, &p, 4.0);
        let c0 = plane_pose(&a, &p, 0.0);
        let c1 = plane_pose(&a, &p, p.until as f32);
        assert!(c1.alt > 250.0 && a.local(c1.pos).x > a.local(c0.pos).x + 2500.0);
        // Taxi phases end where the next begins.
        p.phase = PlanePhase::TaxiIn;
        p.until = 10;
        let ti = plane_pose(&a, &p, 10.0);
        assert!(ti.pos.distance(at.pos) < 0.01);
    }

    #[test]
    fn stands_are_given_one_plane_each() {
        let mut a = apt(AirportSize::Airfield);
        let s = a.free_stand(PlaneKind::Turboprop).unwrap();
        a.planes.push(Plane { kind: PlaneKind::Turboprop, airline: 0, flight: 1, from: 0, to: 1, phase: PlanePhase::TaxiIn, since: 0, until: 9, stand: s as u8, dir: 1.0, depart: 0, pax_in: 0, pax_out: 0, cargo_in: 0.0, late_in: 0, approach_len: 2000.0, climb_len: 2000.0 });
        assert_ne!(a.free_stand(PlaneKind::Turboprop), Some(s));
        let cargo = a.free_stand(PlaneKind::Freighter).unwrap();
        assert!(a.layout().stands[cargo].cargo);
        let n = a.layout().passenger_stands().count();
        for i in 1..n {
            let s = a.free_stand(PlaneKind::Turboprop).unwrap();
            a.planes.push(Plane { stand: s as u8, flight: i as u32, ..a.planes[0].clone() });
        }
        assert_eq!(a.free_stand(PlaneKind::Turboprop), None, "full");
    }
}
