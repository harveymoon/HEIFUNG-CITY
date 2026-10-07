//! City services and emergencies: the data. Police, fire, health, schools,
//! a 911 dispatch centre and two attractions (monuments, stadiums), each
//! placed by the player in one of three tiers; the vehicles they run; the
//! incidents (fires, crimes, people taken ill) those vehicles answer; and
//! the catalogue the Buildings tool offers. The systems that drive all of
//! this live in `world/services.rs`.
//!
//! The life of an incident: it *starts* (a fire catches, a crime happens,
//! someone collapses), somebody nearby *calls 911* after a short delay, a
//! dispatcher at a dispatch centre *assigns* the nearest free unit (slower
//! without one on shift), the vehicle *drives* there over the street
//! network with lights and sirens, *works* on scene, *resolves* it (fire
//! out, an arrest, a patient to hospital) and *returns* to its station.
//! Unanswered fires grow, spread to neighbours and can burn a building down.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::CitizenId;
use crate::ports::PortKind;
use crate::power::PowerPlantKind;
use crate::rail::RailFacility;
use crate::traffic::CarId;
use crate::zoning::{LandUse, LotId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;

new_key_type! {
    pub struct UnitId;
}

// ── balance ────────────────────────────────────────────────────────────────

/// Coverage fades to nothing over the last this-many metres of a station's
/// road radius: inside `radius - COVERAGE_FADE` a lot is fully covered.
pub const COVERAGE_FADE: f32 = 200.0;
/// A lot counts as covered (for the coverage share and the alerts) at this.
pub const COVERED: f32 = 0.5;
/// Hourly chance a building catches fire, by tier, before condition, land
/// use and fire coverage. Cheap, worn buildings burn.
pub const FIRE_RISK: [f32; 3] = [4.0e-5, 2.5e-5, 1.5e-5];
/// Hourly chance of a crime at an occupied building, before unemployment,
/// land value, night and police coverage.
pub const CRIME_RISK: f32 = 2.0e-4;
/// Hourly chance a citizen indoors falls ill, before age, hunger and
/// healthcare coverage.
pub const SICK_RISK: f32 = 1.2e-4;
/// How much full coverage cuts each risk.
pub const FIRE_COVER_CUT: f32 = 0.7;
pub const CRIME_COVER_CUT: f32 = 0.75;
pub const SICK_COVER_CUT: f32 = 0.5;
/// Fire intensity gained per tick while nobody fights it (full in ~5.5 h).
pub const FIRE_GROWTH: f32 = 0.003;
/// Intensity an engine on scene knocks down per tick.
pub const ENGINE_RATE: f32 = 0.012;
/// Condition a building loses per tick at full intensity.
pub const FIRE_DAMAGE: f32 = 0.0012;
/// At or below this condition a burning building is gone.
pub const BURNED_DOWN: f32 = 0.08;
/// [services] Condition the city restores on one of its own structures
/// (stations, plants, depots, docks...) per day while it is damaged: a
/// gutted station is back in service in about a day and a half (past the
/// derelict line) and fully repaired in under four.
pub const REPAIR_PER_DAY: f32 = 0.25;
/// [services] What repairs cost: this share of the structure's build price
/// for the whole of its condition (a gutted hospital about half its price).
pub const REPAIR_SHARE: f32 = 0.5;
/// Fires above this intensity can jump to buildings this close (lot
/// centres), checked every `SPREAD_EVERY` ticks with `SPREAD_CHANCE` x
/// intensity per neighbour.
pub const SPREAD_INTENSITY: f32 = 0.5;
pub const SPREAD_RADIUS: f32 = 28.0;
/// Lots with corners this close count as touching.
pub const SPREAD_GAP: f32 = 4.0;
pub const SPREAD_EVERY: u64 = 20;
pub const SPREAD_CHANCE: f32 = 0.04;
/// Ticks from the call to the dispatch: a dispatcher on shift, a staffed
/// centre with nobody at the desk (on call), and no dispatch centre at all
/// (the call goes to the regional line).
pub const DISPATCH_ON_SHIFT: u64 = 2;
pub const DISPATCH_ON_CALL: u64 = 8;
pub const DISPATCH_REGIONAL: u64 = 20;
/// Ticks of work on scene: a crime, a patient (fires burn until out).
pub const CRIME_WORK: f32 = 25.0;
pub const MEDICAL_WORK: f32 = 12.0;
/// [driving] Ticks the police spend at a crash scene, by the case: someone
/// hurt (statements, the scene kept while the paramedics work); a driver to
/// arrest (drunk or through a red light: a test, the paperwork); or only a
/// written-off car in the road to clear for the tow truck. A crash with
/// nobody hurt and every car still drivable is never a call: the drivers
/// swap details and go (`World::handle_crash`).
pub const CRASH_WORK: f32 = 20.0;
pub const CRASH_WORK_ARREST: f32 = 25.0;
pub const CRASH_WORK_CLEAR: f32 = 10.0;
/// A crash this close to an open crash scene joins it rather than opening
/// a call of its own.
pub const CRASH_SCENE_JOIN: f32 = 60.0;
/// [driving] Chance the police arrest a driver at fault who was drunk or
/// ran a red, once they are on scene.
pub const DUI_ARREST: f32 = 0.6;
/// Chance an arrest is made when the police get there, quick or slow (a
/// response over `QUICK_RESPONSE` ticks after the crime).
pub const ARREST_QUICK: f32 = 0.75;
pub const ARREST_SLOW: f32 = 0.4;
pub const QUICK_RESPONSE: u64 = 30;
/// An incident nobody is sent to ends on its own after this many ticks
/// from the call: crimes go unsolved, patients get better (unhappily).
pub const CRIME_GIVE_UP: u64 = 180;
pub const MEDICAL_GIVE_UP: u64 = 480;
/// Hours a patient stays in hospital, a range.
pub const STAY_HOURS: (u64, u64) = (6, 24);
/// Daily wage bonus at full school coverage of a worker's home.
pub const EDUCATION_BONUS: f32 = 0.12;
/// Stadium ticket price, chance a stadium hosts an event on a given day
/// (by tier), and when the crowd sets off.
pub const TICKET_PRICE: f32 = 12.0;
pub const EVENT_CHANCE: [f64; 3] = [0.3, 0.4, 0.6];
pub const EVENT_MINUTE: u64 = 18 * 60;
/// How far a fan travels to a game.
pub const EVENT_REACH: f32 = 1600.0;
/// Pupils set off for school at this minute and stay until it closes.
pub const SCHOOL_MINUTE: u64 = 7 * 60 + 30;
/// A hospital stay, billed to the patient's household; what it cannot pay
/// the city covers.
pub const HOSPITAL_BILL: f32 = 60.0;
/// Days in jail for an arrest, a range; without a jail with room the suspect
/// is held at the police station this many hours and let go early.
pub const SENTENCE_DAYS: (u64, u64) = (2, 5);
pub const HELD_HOURS: u64 = 6;
/// With no crewed station of the kind but a dispatcher to ask, a unit from
/// out of town comes: this many minutes after dispatch, for a fee.
pub const OUT_OF_TOWN_TRAVEL: u64 = 45;
pub const OUT_OF_TOWN_FEE: [f32; 3] = [1_500.0, 600.0, 800.0];
/// Police cars between calls patrol their station's district: they drive
/// to a beat (a busy or crime-hit block) and park there this many ticks, a
/// range, before moving on; ambulances post at a standby point near where
/// people live. Units on patrol or standby, and units driving home, take
/// the next call from where they are. Fire engines wait in the station.
pub const PATROL_STAY: (u64, u64) = (25, 50);
pub const STANDBY_STAY: (u64, u64) = (90, 180);
/// A beat or standby point is picked within this share of the station's
/// coverage radius (straight line).
pub const PATROL_REACH: f32 = 0.85;
/// A call queued this long with every city unit of the kind busy goes to a
/// neighbouring town as mutual aid (with a dispatcher to ask), billed like
/// out-of-town help but quicker: their units are already on the road.
pub const MUTUAL_AID_WAIT: u64 = 40;
pub const MUTUAL_AID_TRAVEL: u64 = 20;
/// A responding unit is on scene once its car is this close to its stop.
pub const PULL_UP: f32 = 25.0;
/// A kind counts as saturated while it has no unit free; the report raises
/// an alert after this many saturated minutes in a day.
pub const SATURATED_ALERT_MIN: u32 = 60;
/// Rubble stays on a burned lot this many ticks before it can be rebuilt.
pub const RUBBLE_TICKS: u64 = 2 * 1440;
/// Resolved incidents kept for the record (the Emergencies window's history).
pub const KEEP_RESOLVED: usize = 500;

// ── what can be placed ─────────────────────────────────────────────────────

/// A service or attraction building.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceKind {
    Police,
    Fire,
    Health,
    School,
    Dispatch,
    Monument,
    Stadium,
    /// A jail: arrested suspects serve their sentence here.
    Jail,
    /// [society] A trash depot: garbage trucks collect from the buildings
    /// within its radius (`crate::trash`).
    TrashDepot,
    /// Drone taxi pad, vertiport or skyport (`crate::air`): placed under
    /// Transit, at the end so saved kinds keep their order.
    Vertiport,
    /// [parking] Parking enforcement office: officers in small cars patrol
    /// the streets within reach, ticket overstayers and call the tow truck
    /// (`crate::enforcement`).
    ParkingEnforcement,
}

impl ServiceKind {
    pub const ALL: [ServiceKind; 11] = [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School, ServiceKind::Dispatch, ServiceKind::Monument, ServiceKind::Stadium, ServiceKind::Jail, ServiceKind::TrashDepot, ServiceKind::Vertiport, ServiceKind::ParkingEnforcement];
    /// The four that cover the city by road distance, in coverage-index order.
    pub const COVERING: [ServiceKind; 4] = [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School];

    /// Short machine name, used by the remote API.
    pub fn key(self) -> &'static str {
        match self {
            ServiceKind::Police => "police",
            ServiceKind::Fire => "fire",
            ServiceKind::Health => "health",
            ServiceKind::School => "school",
            ServiceKind::Dispatch => "dispatch",
            ServiceKind::Monument => "monument",
            ServiceKind::Stadium => "stadium",
            ServiceKind::Jail => "jail",
            ServiceKind::TrashDepot => "trash_depot",
            ServiceKind::Vertiport => "vertiport",
            ServiceKind::ParkingEnforcement => "parking_enforcement",
        }
    }

    pub fn from_key(s: &str) -> Option<ServiceKind> {
        match s {
            "police" | "police_station" => Some(ServiceKind::Police),
            "fire" | "fire_station" => Some(ServiceKind::Fire),
            "health" | "hospital" | "clinic" => Some(ServiceKind::Health),
            "school" | "education" => Some(ServiceKind::School),
            "dispatch" | "911" => Some(ServiceKind::Dispatch),
            "monument" => Some(ServiceKind::Monument),
            "stadium" => Some(ServiceKind::Stadium),
            "jail" | "prison" => Some(ServiceKind::Jail),
            "trash_depot" | "trash" | "garbage" | "sanitation" => Some(ServiceKind::TrashDepot),
            "vertiport" | "skyport" | "drone_pad" | "dronepad" | "vertipad" => Some(ServiceKind::Vertiport),
            "parking_enforcement" | "parking_office" | "parking_police" | "traffic_wardens" => Some(ServiceKind::ParkingEnforcement),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ServiceKind::Police => "police station",
            ServiceKind::Fire => "fire station",
            ServiceKind::Health => "hospital",
            ServiceKind::School => "school",
            ServiceKind::Dispatch => "911 dispatch centre",
            ServiceKind::Monument => "monument",
            ServiceKind::Stadium => "stadium",
            ServiceKind::Jail => "jail",
            ServiceKind::TrashDepot => "trash depot",
            ServiceKind::Vertiport => "vertiport",
            ServiceKind::ParkingEnforcement => "parking enforcement office",
        }
    }

    /// What the building is called at a tier.
    pub fn tier_label(self, tier: Tier) -> &'static str {
        let names: [&str; 3] = match self {
            ServiceKind::Police => ["police post", "police station", "police headquarters"],
            ServiceKind::Fire => ["fire post", "fire station", "fire headquarters"],
            ServiceKind::Health => ["clinic", "hospital", "medical centre"],
            ServiceKind::School => ["primary school", "high school", "college"],
            ServiceKind::Dispatch => ["dispatch office", "dispatch centre", "emergency operations centre"],
            ServiceKind::Monument => ["memorial", "monument", "landmark tower"],
            ServiceKind::Stadium => ["sports ground", "stadium", "mega-arena"],
            ServiceKind::Jail => ["lock-up", "jail", "penitentiary"],
            ServiceKind::TrashDepot => ["transfer yard", "trash depot", "waste management centre"],
            ServiceKind::Vertiport => ["drone pad", "vertiport", "skyport"],
            ServiceKind::ParkingEnforcement => ["parking warden post", "parking enforcement office", "parking authority"],
        };
        names[tier.index()]
    }

    /// When it is staffed and open: emergency services round the clock
    /// (three shifts), schools by day, stadiums from noon to midnight.
    pub fn hours(self) -> crate::market::Hours {
        use crate::market::Hours;
        match self {
            ServiceKind::Police | ServiceKind::Fire | ServiceKind::Health | ServiceKind::Dispatch | ServiceKind::Monument | ServiceKind::Jail | ServiceKind::Vertiport => Hours::ALWAYS,
            ServiceKind::School => Hours::new(8, 16),
            ServiceKind::Stadium => Hours::new(12, 0),
            ServiceKind::TrashDepot => Hours::new(6, 18),
            ServiceKind::ParkingEnforcement => Hours::new(7, 21),
        }
    }

    pub fn is_attraction(self) -> bool {
        matches!(self, ServiceKind::Monument | ServiceKind::Stadium)
    }

    /// Index into coverage arrays for the kinds that cover ground.
    pub fn coverage_index(self) -> Option<usize> {
        match self {
            ServiceKind::Police => Some(0),
            ServiceKind::Fire => Some(1),
            ServiceKind::Health => Some(2),
            ServiceKind::School => Some(3),
            _ => None,
        }
    }

    pub fn unit_kind(self) -> Option<UnitKind> {
        match self {
            ServiceKind::Police => Some(UnitKind::PoliceCar),
            ServiceKind::Fire => Some(UnitKind::FireEngine),
            ServiceKind::Health => Some(UnitKind::Ambulance),
            _ => None,
        }
    }

    pub fn build_cost(self, tier: Tier) -> f32 {
        let base = match self {
            ServiceKind::Police => 12_000.0,
            ServiceKind::Fire => 14_000.0,
            ServiceKind::Health => 18_000.0,
            ServiceKind::School => 10_000.0,
            ServiceKind::Dispatch => 8_000.0,
            ServiceKind::Monument => 15_000.0,
            ServiceKind::Stadium => 45_000.0,
            ServiceKind::Jail => 16_000.0,
            ServiceKind::TrashDepot => 9_000.0,
            ServiceKind::Vertiport => 20_000.0,
            ServiceKind::ParkingEnforcement => 6_000.0,
        };
        base * [0.5, 1.0, 2.2][tier.index()]
    }

    pub fn upkeep_per_day(self, tier: Tier) -> f32 {
        let base = match self {
            ServiceKind::Police => 350.0,
            ServiceKind::Fire => 380.0,
            ServiceKind::Health => 500.0,
            ServiceKind::School => 300.0,
            ServiceKind::Dispatch => 200.0,
            ServiceKind::Monument => 80.0,
            ServiceKind::Stadium => 700.0,
            ServiceKind::Jail => 420.0,
            ServiceKind::TrashDepot => 260.0,
            ServiceKind::Vertiport => 260.0,
            ServiceKind::ParkingEnforcement => 150.0,
        };
        base * [0.6, 1.0, 1.8][tier.index()]
    }

    pub fn jobs(self, tier: Tier) -> u16 {
        let base: u16 = match self {
            ServiceKind::Police => 10,
            ServiceKind::Fire => 12,
            ServiceKind::Health => 16,
            ServiceKind::School => 10,
            ServiceKind::Dispatch => 6,
            ServiceKind::Monument => 2,
            ServiceKind::Stadium => 20,
            ServiceKind::Jail => 10,
            // Two crew a truck (`trash::TRUCKS` per tier) and the yard.
            ServiceKind::TrashDepot => 10,
            ServiceKind::Vertiport => 8,
            // Two officers a car (`enforcement_cars`).
            ServiceKind::ParkingEnforcement => 6,
        };
        match tier {
            Tier::Low => base.div_ceil(2),
            Tier::Mid => base,
            Tier::High => base * 2,
        }
    }

    /// Road distance the service covers, in metres (0 for none).
    pub fn radius(self, tier: Tier) -> f32 {
        let base = match self {
            ServiceKind::Police => 650.0,
            ServiceKind::Fire => 600.0,
            ServiceKind::Health => 750.0,
            ServiceKind::School => 500.0,
            // Straight-line reach of its trucks (`crate::trash`), not road coverage.
            ServiceKind::TrashDepot => 800.0,
            // Straight-line reach of its patrols.
            ServiceKind::ParkingEnforcement => 700.0,
            _ => return 0.0,
        };
        base * [0.7, 1.0, 1.4][tier.index()]
    }

    /// Vehicles the station runs when fully staffed.
    pub fn units(self, tier: Tier) -> usize {
        if self.unit_kind().is_none() {
            return 0;
        }
        [1, 2, 4][tier.index()]
    }

    /// Beds (hospitals), seats (stadiums), places (schools).
    pub fn capacity(self, tier: Tier) -> u32 {
        match self {
            ServiceKind::Health => [6, 24, 70][tier.index()],
            ServiceKind::Stadium => [120, 320, 800][tier.index()],
            ServiceKind::Jail => [12, 40, 120][tier.index()],
            ServiceKind::School => [80, 240, 600][tier.index()],
            _ => 0,
        }
    }

    pub fn min_area(self, tier: Tier) -> f32 {
        let base = match self {
            ServiceKind::Monument => 250.0,
            ServiceKind::Stadium => 900.0,
            ServiceKind::Jail => 450.0,
            ServiceKind::Dispatch => 250.0,
            ServiceKind::TrashDepot => 500.0,
            ServiceKind::Vertiport => 350.0,
            ServiceKind::ParkingEnforcement => 200.0,
            _ => 400.0,
        };
        base * [0.75, 1.0, 1.6][tier.index()]
    }

    pub fn allowed_on(self, lu: LandUse) -> bool {
        match self {
            ServiceKind::Monument => lu != LandUse::Industrial,
            ServiceKind::Stadium => matches!(lu, LandUse::Entertainment | LandUse::Civic | LandUse::Commercial),
            ServiceKind::Jail | ServiceKind::TrashDepot => matches!(lu, LandUse::Civic | LandUse::Industrial),
            ServiceKind::Fire | ServiceKind::Vertiport | ServiceKind::ParkingEnforcement => lu != LandUse::Park,
            _ => lu != LandUse::Park && lu != LandUse::Industrial,
        }
    }

    /// Pull on the land value around it: (radius, weight). Coverage is the
    /// real benefit (lower risk); this is the reassurance of a station up
    /// the road, or the draw of a landmark.
    pub fn land_value(self, tier: Tier) -> (f32, f32) {
        let k = [0.7, 1.0, 1.4][tier.index()];
        match self {
            ServiceKind::Police => (300.0 * k, 0.05 * k),
            ServiceKind::Fire => (300.0 * k, 0.04 * k),
            ServiceKind::Health => (350.0 * k, 0.08 * k),
            ServiceKind::School => (300.0 * k, 0.09 * k),
            ServiceKind::Dispatch => (150.0, 0.02),
            ServiceKind::Monument => (220.0 * k, 0.18 * k),
            ServiceKind::Stadium => (320.0 * k, 0.10 * k),
            // Nobody wants to live by the jail.
            ServiceKind::Jail => (200.0 * k, -0.08 * k),
            // Lorries and the smell.
            ServiceKind::TrashDepot => (220.0 * k, -0.06 * k),
            // Rotor noise: the neighbours pay for the rich commuting overhead.
            ServiceKind::Vertiport => (160.0 * k, -0.04 * k),
            ServiceKind::ParkingEnforcement => (0.0, 0.0),
        }
    }

    /// Name for a new building of this kind.
    pub fn name(self, tier: Tier, n: u32) -> String {
        let place = ["Kowloon", "Harbor", "Neon", "Eastgate", "Sector 7", "Old Town", "Ridgeline", "Canal", "Shinjuku", "Southside"][n as usize % 10];
        match self {
            ServiceKind::Police => format!("{place} {}", ["Police Post", "Precinct", "Police HQ"][tier.index()]),
            ServiceKind::Fire => format!("{place} {}", ["Fire Post", "Fire Station", "Fire HQ"][tier.index()]),
            ServiceKind::Health => format!("{place} {}", ["Clinic", "General Hospital", "Medical Centre"][tier.index()]),
            ServiceKind::School => format!("{place} {}", ["Primary", "High School", "Polytechnic"][tier.index()]),
            ServiceKind::Dispatch => format!("{place} 911 {}", ["Office", "Dispatch", "Operations"][tier.index()]),
            ServiceKind::Monument => format!("{} of {place}", ["Memorial", "Monument", "Spire"][tier.index()]),
            ServiceKind::Stadium => format!("{place} {}", ["Grounds", "Stadium", "Mega-Arena"][tier.index()]),
            ServiceKind::Jail => format!("{place} {}", ["Lock-up", "County Jail", "Penitentiary"][tier.index()]),
            ServiceKind::TrashDepot => format!("{place} {}", ["Transfer Yard", "Sanitation Depot", "Waste Management"][tier.index()]),
            ServiceKind::Vertiport => format!("{place} {}", ["Drone Pad", "Vertiport", "Skyport"][tier.index()]),
            ServiceKind::ParkingEnforcement => format!("{place} {}", ["Warden Post", "Parking Enforcement", "Parking Authority"][tier.index()]),
        }
    }
}

/// The service a building provides, stored on the building.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSite {
    pub kind: ServiceKind,
    pub tier: Tier,
}

/// Everything the Buildings tool can put down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placeable {
    Plant(PowerPlantKind),
    /// [power] A grid battery pack (`crate::power::BatteryKind`).
    Battery(crate::power::BatteryKind),
    Service(ServiceKind, Tier),
    BusDepot,
    Rail(RailFacility),
    /// A shipping port or ferry terminal at the water's edge.
    Dock(PortKind),
    /// [parking] A parking lot the city runs, in three sizes.
    ParkingLot(crate::enforcement::LotSize),
    /// [subway] A metro station: an entrance on a lot, the platform below.
    MetroStation,
    /// [tolls] A toll booth or plaza across a road (not on a lot:
    /// `World::place_toll`).
    Toll(crate::tolls::TollSize),
    /// A cargo hub (`crate::cargo_hub`): freight terminal for the imports
    /// and exports of the businesses around it.
    CargoHub,
}

/// The Buildings tool's sub-categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BuildCategory {
    #[default]
    Power,
    Services,
    Transit,
    Rail,
    Water,
    Attractions,
}

impl BuildCategory {
    pub const ALL: [BuildCategory; 6] = [BuildCategory::Power, BuildCategory::Services, BuildCategory::Transit, BuildCategory::Rail, BuildCategory::Water, BuildCategory::Attractions];

    pub fn label(self) -> &'static str {
        match self {
            BuildCategory::Power => "Power",
            BuildCategory::Services => "Services",
            BuildCategory::Transit => "Transit",
            BuildCategory::Rail => "Rail",
            BuildCategory::Water => "Water",
            BuildCategory::Attractions => "Attractions",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            BuildCategory::Power => "power",
            BuildCategory::Services => "services",
            BuildCategory::Transit => "transit",
            BuildCategory::Rail => "rail",
            BuildCategory::Water => "water",
            BuildCategory::Attractions => "attractions",
        }
    }

    /// What the category offers, in menu order (services at their middle
    /// tier; the tier is picked separately).
    pub fn items(self) -> Vec<Placeable> {
        match self {
            BuildCategory::Power => PowerPlantKind::ALL.iter().map(|&k| Placeable::Plant(k)).chain(crate::power::BatteryKind::ALL.iter().map(|&k| Placeable::Battery(k))).collect(),
            BuildCategory::Services => [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School, ServiceKind::Dispatch, ServiceKind::Jail, ServiceKind::TrashDepot, ServiceKind::ParkingEnforcement].iter().map(|&k| Placeable::Service(k, Tier::Mid)).collect(),
            BuildCategory::Transit => {
                let mut v = vec![Placeable::BusDepot, Placeable::MetroStation, Placeable::Service(ServiceKind::Vertiport, Tier::Mid)];
                v.extend(crate::enforcement::LotSize::ALL.iter().map(|&s| Placeable::ParkingLot(s)));
                v.extend(crate::tolls::TollSize::ALL.iter().map(|&s| Placeable::Toll(s)));
                v
            }
            BuildCategory::Rail => vec![Placeable::Rail(RailFacility::Station), Placeable::Rail(RailFacility::Central), Placeable::Rail(RailFacility::FreightYard), Placeable::CargoHub],
            BuildCategory::Water => PortKind::ALL.iter().map(|&k| Placeable::Dock(k)).collect(),
            BuildCategory::Attractions => vec![Placeable::Service(ServiceKind::Monument, Tier::Mid), Placeable::Service(ServiceKind::Stadium, Tier::Mid)],
        }
    }
}

impl Placeable {
    pub fn category(self) -> BuildCategory {
        match self {
            Placeable::Plant(_) | Placeable::Battery(_) => BuildCategory::Power,
            Placeable::Service(k, _) if k.is_attraction() => BuildCategory::Attractions,
            Placeable::Service(ServiceKind::Vertiport, _) => BuildCategory::Transit,
            Placeable::Service(..) => BuildCategory::Services,
            Placeable::BusDepot => BuildCategory::Transit,
            Placeable::Rail(_) => BuildCategory::Rail,
            Placeable::Dock(_) => BuildCategory::Water,
            Placeable::ParkingLot(_) => BuildCategory::Transit,
            Placeable::MetroStation => BuildCategory::Transit,
            Placeable::Toll(_) => BuildCategory::Transit,
            Placeable::CargoHub => BuildCategory::Rail,
        }
    }

    /// Whether the player picks a tier for it.
    pub fn has_tiers(self) -> bool {
        matches!(self, Placeable::Service(..))
    }

    pub fn with_tier(self, tier: Tier) -> Placeable {
        match self {
            Placeable::Service(k, _) => Placeable::Service(k, tier),
            other => other,
        }
    }

    pub fn label(self) -> String {
        match self {
            Placeable::Plant(k) => k.label().to_string(),
            Placeable::Battery(k) => k.label().to_string(),
            Placeable::Service(k, t) => k.tier_label(t).to_string(),
            Placeable::BusDepot => "bus depot".into(),
            Placeable::Rail(f) => f.label().to_lowercase(),
            Placeable::Dock(k) => k.label().to_string(),
            Placeable::ParkingLot(s) => s.label().to_string(),
            Placeable::MetroStation => "metro station".into(),
            Placeable::Toll(s) => s.label().to_string(),
            Placeable::CargoHub => "cargo hub".into(),
        }
    }

    /// Short name for the menu, without the tier.
    pub fn menu_label(self) -> &'static str {
        match self {
            Placeable::Plant(k) => k.label(),
            Placeable::Battery(crate::power::BatteryKind::Small) => "Battery pack, small",
            Placeable::Battery(crate::power::BatteryKind::Medium) => "Battery pack, medium",
            Placeable::Battery(crate::power::BatteryKind::Large) => "Battery farm, large",
            Placeable::Service(k, _) => match k {
                ServiceKind::Police => "Police",
                ServiceKind::Fire => "Fire",
                ServiceKind::Health => "Health",
                ServiceKind::School => "Education",
                ServiceKind::Dispatch => "911 Dispatch",
                ServiceKind::Monument => "Monument",
                ServiceKind::Stadium => "Stadium",
                ServiceKind::Jail => "Jail",
                ServiceKind::TrashDepot => "Trash depot",
                ServiceKind::Vertiport => "Vertiport",
                ServiceKind::ParkingEnforcement => "Parking enforcement",
            },
            Placeable::ParkingLot(s) => s.menu_label(),
            Placeable::Toll(s) => s.menu_label(),
            Placeable::BusDepot => "Bus depot",
            Placeable::Rail(RailFacility::Station) => "Passenger station",
            Placeable::Rail(RailFacility::FreightYard) => "Freight yard",
            Placeable::Rail(RailFacility::Central) => "Central Station",
            Placeable::MetroStation => "Metro station",
            Placeable::Dock(PortKind::ShippingPort) => "Shipping port",
            Placeable::Dock(PortKind::FerryTerminal) => "Ferry terminal",
            Placeable::CargoHub => "Cargo hub",
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            Placeable::Plant(k) => k.build_cost(),
            Placeable::Battery(k) => k.build_cost(),
            Placeable::Service(k, t) => k.build_cost(t),
            Placeable::BusDepot => crate::transit::DEPOT_COST,
            Placeable::Rail(f) => f.cost(),
            Placeable::MetroStation => crate::subway::STATION_COST,
            Placeable::Dock(k) => k.cost(),
            Placeable::ParkingLot(s) => s.cost(),
            Placeable::Toll(s) => s.cost(),
            Placeable::CargoHub => crate::cargo_hub::HUB_COST,
        }
    }

    pub fn upkeep(self) -> f32 {
        match self {
            Placeable::Plant(k) => k.upkeep_per_day(),
            Placeable::Battery(k) => k.upkeep_per_day(),
            Placeable::Service(k, t) => k.upkeep_per_day(t),
            Placeable::BusDepot => crate::transit::DEPOT_UPKEEP_PER_DAY,
            Placeable::Rail(f) => f.upkeep_per_day(),
            Placeable::MetroStation => crate::subway::STATION_UPKEEP_PER_DAY,
            Placeable::Dock(k) => k.upkeep_per_day(),
            Placeable::ParkingLot(s) => s.upkeep_per_day(),
            Placeable::Toll(s) => s.upkeep_per_day(),
            Placeable::CargoHub => crate::cargo_hub::HUB_UPKEEP_PER_DAY,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            Placeable::Plant(k) => k.jobs(),
            Placeable::Battery(k) => k.jobs(),
            Placeable::Service(k, t) => k.jobs(t),
            Placeable::BusDepot => crate::transit::DEPOT_JOBS,
            Placeable::Rail(f) => f.jobs(),
            Placeable::MetroStation => crate::subway::STATION_JOBS,
            Placeable::Dock(k) => k.jobs(),
            Placeable::ParkingLot(s) => s.jobs(),
            // Collectors are part of the upkeep, not jobs in town.
            Placeable::Toll(_) => 0,
            Placeable::CargoHub => crate::cargo_hub::HUB_JOBS,
        }
    }

    /// Coverage radius to draw round the cursor (road metres; 0 for none).
    pub fn radius(self) -> f32 {
        match self {
            Placeable::Service(k, t) => k.radius(t),
            _ => 0.0,
        }
    }

    /// Smallest lot it goes on; rail pieces bring their own footprint.
    pub fn min_area(self) -> f32 {
        match self {
            Placeable::Plant(k) => k.min_area(),
            Placeable::Battery(k) => k.min_area(),
            Placeable::Service(k, t) => k.min_area(t),
            Placeable::BusDepot => crate::transit::DEPOT_MIN_AREA,
            Placeable::Rail(_) | Placeable::Dock(_) | Placeable::Toll(_) => 0.0,
            Placeable::ParkingLot(s) => s.min_area(),
            Placeable::MetroStation => crate::subway::STATION_MIN_AREA,
            Placeable::CargoHub => crate::cargo_hub::HUB_MIN_AREA,
        }
    }

    /// Whether it goes on a lot of this land use (rail pieces go anywhere).
    pub fn allowed_on(self, lu: LandUse) -> bool {
        match self {
            Placeable::Plant(k) => k.allowed_on(lu),
            Placeable::Battery(k) => k.allowed_on(lu),
            Placeable::Service(k, _) => k.allowed_on(lu),
            Placeable::BusDepot => !matches!(lu, LandUse::Residential | LandUse::Park),
            Placeable::Rail(_) | Placeable::Dock(_) | Placeable::Toll(_) => true,
            // Any road-adjacent lot but a park.
            Placeable::ParkingLot(_) => lu != LandUse::Park,
            // An entrance plaza fits anywhere, parks included.
            Placeable::MetroStation => true,
            // Freight belongs on working land.
            Placeable::CargoHub => !matches!(lu, LandUse::Residential | LandUse::Park),
        }
    }

    /// Parse a remote-API kind: plant names, `depot`, `station`,
    /// `freight_yard`, or a service key with an optional tier suffix
    /// (`police`, `fire:high`, `clinic` is a low-tier hospital).
    pub fn parse(kind: &str, tier: Option<Tier>) -> Option<Placeable> {
        let kind = kind.trim().to_ascii_lowercase();
        let (name, suffix) = match kind.split_once([':', '/']) {
            Some((a, b)) => (a.to_string(), Some(b.to_string())),
            None => (kind.clone(), None),
        };
        let tier = suffix.as_deref().and_then(parse_tier).or(tier);
        match name.as_str() {
            "coal" => Some(Placeable::Plant(PowerPlantKind::Coal)),
            "solar" => Some(Placeable::Plant(PowerPlantKind::Solar)),
            "wind" => Some(Placeable::Plant(PowerPlantKind::Wind)),
            "nuclear" => Some(Placeable::Plant(PowerPlantKind::Nuclear)),
            "hydro" => Some(Placeable::Plant(PowerPlantKind::Hydro)),
            "tidal" => Some(Placeable::Plant(PowerPlantKind::Tidal)),
            "geothermal" => Some(Placeable::Plant(PowerPlantKind::Geothermal)),
            "battery" => Some(Placeable::Battery(match tier {
                Some(Tier::Low) => crate::power::BatteryKind::Small,
                Some(Tier::High) => crate::power::BatteryKind::Large,
                _ => crate::power::BatteryKind::Medium,
            })),
            other if crate::power::BatteryKind::from_key(other).is_some() => crate::power::BatteryKind::from_key(other).map(Placeable::Battery),
            "shipping_port" | "port" => Some(Placeable::Dock(PortKind::ShippingPort)),
            "ferry_terminal" | "ferry" => Some(Placeable::Dock(PortKind::FerryTerminal)),
            "depot" | "bus_depot" => Some(Placeable::BusDepot),
            "station" => Some(Placeable::Rail(RailFacility::Station)),
            "freight_yard" | "yard" => Some(Placeable::Rail(RailFacility::FreightYard)),
            "central_station" | "central" | "main_station" => Some(Placeable::Rail(RailFacility::Central)),
            "metro_station" | "metro" | "subway_station" | "subway" => Some(Placeable::MetroStation),
            "cargo_hub" | "hub" | "cargo" => Some(Placeable::CargoHub),
            "clinic" => Some(Placeable::Service(ServiceKind::Health, tier.unwrap_or(Tier::Low))),
            other if crate::enforcement::LotSize::from_key(other).is_some() => crate::enforcement::LotSize::from_key(other).map(Placeable::ParkingLot),
            other if crate::tolls::TollSize::from_key(other).is_some() => crate::tolls::TollSize::from_key(other).map(Placeable::Toll),
            other => ServiceKind::from_key(other).map(|k| Placeable::Service(k, tier.unwrap_or(Tier::Mid))),
        }
    }
}

pub fn parse_tier(s: &str) -> Option<Tier> {
    match s.trim().to_ascii_lowercase().as_str() {
        "low" | "1" | "small" => Some(Tier::Low),
        "mid" | "middle" | "2" | "medium" => Some(Tier::Mid),
        "high" | "3" | "large" => Some(Tier::High),
        _ => None,
    }
}

// ── units and incidents ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitKind {
    FireEngine,
    PoliceCar,
    Ambulance,
}

impl UnitKind {
    pub fn label(self) -> &'static str {
        match self {
            UnitKind::FireEngine => "fire engine",
            UnitKind::PoliceCar => "police car",
            UnitKind::Ambulance => "ambulance",
        }
    }

    pub fn call_sign(self) -> &'static str {
        match self {
            UnitKind::FireEngine => "Engine",
            UnitKind::PoliceCar => "Car",
            UnitKind::Ambulance => "Medic",
        }
    }

    /// Workers it takes to put one on the road.
    pub fn crew(self) -> usize {
        match self {
            UnitKind::FireEngine => 3,
            UnitKind::PoliceCar | UnitKind::Ambulance => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IncidentKind {
    Fire,
    Crime,
    Medical,
    /// [driving] A bad road crash: the police clear the scene (the hurt
    /// get a `Medical` incident of their own, on the road).
    Crash,
}

impl IncidentKind {
    pub const ALL: [IncidentKind; 3] = [IncidentKind::Fire, IncidentKind::Crime, IncidentKind::Medical];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Slot in the per-service response figures (`ServiceStats::queued`,
    /// `response_min_today`, ...): fire, police, ambulance. Crashes count
    /// with the police, who clear the scene.
    pub fn slot(self) -> usize {
        self.unit() as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            IncidentKind::Fire => "fire",
            IncidentKind::Crime => "crime",
            IncidentKind::Medical => "medical",
            IncidentKind::Crash => "crash",
        }
    }

    pub fn unit(self) -> UnitKind {
        match self {
            IncidentKind::Fire => UnitKind::FireEngine,
            IncidentKind::Crime | IncidentKind::Crash => UnitKind::PoliceCar,
            IncidentKind::Medical => UnitKind::Ambulance,
        }
    }

    pub fn service(self) -> ServiceKind {
        match self {
            IncidentKind::Fire => ServiceKind::Fire,
            IncidentKind::Crime => ServiceKind::Police,
            IncidentKind::Medical => ServiceKind::Health,
            IncidentKind::Crash => ServiceKind::Police,
        }
    }
}

/// What kind of crime (`World::report_crime`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrimeKind {
    #[default]
    Robbery,
    Theft,
    Assault,
    Vandalism,
    /// [driving] A street takeover: an intersection held for donuts.
    StreetTakeover,
}

impl CrimeKind {
    pub const ALL: [CrimeKind; 4] = [CrimeKind::Robbery, CrimeKind::Theft, CrimeKind::Assault, CrimeKind::Vandalism];

    /// Robbery and assault put people in danger: they jump the queue.
    pub fn violent(self) -> bool {
        matches!(self, CrimeKind::Robbery | CrimeKind::Assault)
    }

    pub fn label(self) -> &'static str {
        match self {
            CrimeKind::Robbery => "robbery",
            CrimeKind::Theft => "theft",
            CrimeKind::Assault => "assault",
            CrimeKind::Vandalism => "vandalism",
            CrimeKind::StreetTakeover => "street takeover",
        }
    }
}

/// Someone involved in an incident, named as they were at the time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Involved {
    pub citizen: CitizenId,
    pub name: String,
    /// caller, dispatcher, victim, suspect, patient, resident
    pub role: String,
    /// Where they ended up: a hospital, a jail, the station, home.
    #[serde(default)]
    pub taken_to: Option<String>,
}

/// Fire damage to one building.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Damage {
    pub building: BuildingId,
    pub name: String,
    /// Condition lost, 0..1.
    pub lost: f32,
    pub destroyed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// Happening; nobody has called yet.
    Unreported,
    /// A 911 call is in; waiting for a dispatcher and a free unit.
    Reported,
    /// A unit is on its way.
    Dispatched,
    /// The unit is there and working.
    OnScene,
    /// An ambulance carrying the patient to hospital.
    Transporting,
    Resolved,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Unreported => "unreported",
            Stage::Reported => "awaiting dispatch",
            Stage::Dispatched => "unit en route",
            Stage::OnScene => "unit on scene",
            Stage::Transporting => "to hospital",
            Stage::Resolved => "resolved",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Extinguished,
    /// Burned until the building was gone.
    BurnedDown,
    Arrest,
    /// Police came, the suspect got away.
    Escaped,
    /// Nobody came.
    Unsolved,
    Admitted,
    /// No hospital bed: treated where they were.
    TreatedOnSite,
    /// Nobody came; they got better on their own.
    Recovered,
    /// The building or person is gone.
    Gone,
    /// [driving] Crash scene cleared, wrecks towed.
    Cleared,
    /// [services] A civic structure burned out: it stands, out of service,
    /// until the city has repaired it (civic buildings never burn down).
    Gutted,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Extinguished => "fire out",
            Outcome::BurnedDown => "burned down",
            Outcome::Arrest => "arrest made",
            Outcome::Escaped => "suspect escaped",
            Outcome::Unsolved => "unsolved",
            Outcome::Admitted => "admitted to hospital",
            Outcome::TreatedOnSite => "treated on site",
            Outcome::Recovered => "recovered without help",
            Outcome::Gone => "gone",
            Outcome::Cleared => "scene cleared",
            Outcome::Gutted => "gutted, under repair",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Incident {
    pub id: u32,
    pub kind: IncidentKind,
    pub building: BuildingId,
    pub pos: Vec2,
    /// The patient, or the victim of a crime.
    pub citizen: Option<CitizenId>,
    /// Whoever did it (crimes), if anyone in town.
    pub suspect: Option<CitizenId>,
    pub caller: Option<CitizenId>,
    pub dispatcher: Option<CitizenId>,
    pub stage: Stage,
    pub started: u64,
    /// When the 911 call goes in.
    pub report_at: u64,
    pub reported: Option<u64>,
    /// When the dispatcher has the job out.
    pub dispatch_at: Option<u64>,
    pub dispatched: Option<u64>,
    pub arrived: Option<u64>,
    pub resolved: Option<u64>,
    pub unit: Option<UnitId>,
    /// Fires: 0..1.
    pub intensity: f32,
    /// Ticks of work left on scene (crimes, patients).
    pub work_left: f32,
    pub outcome: Option<Outcome>,
    // ── the record, for the Emergencies window and the API ──
    /// Name of the building at the time.
    #[serde(default)]
    pub place: String,
    /// Crimes: what kind.
    #[serde(default)]
    pub crime: Option<CrimeKind>,
    /// Answered by a unit from out of town (slow, billed).
    #[serde(default)]
    pub outside: bool,
    /// When the out-of-town unit gets there.
    #[serde(default)]
    pub arrive_at: Option<u64>,
    /// Fee paid for out-of-town help.
    #[serde(default)]
    pub fee: f32,
    /// The unit's call sign and station ("Engine 2, Eastgate Fire HQ").
    #[serde(default)]
    pub unit_name: Option<String>,
    #[serde(default)]
    pub people: Vec<Involved>,
    /// Fires: every building it burned (spread fires have their own incident too).
    #[serde(default)]
    pub damage: Vec<Damage>,
    /// [driving] Out on the street at `pos` (a crash, someone hurt in
    /// one, a takeover), not inside `building`, which only names the place.
    #[serde(default)]
    pub on_road: bool,
}

impl Incident {
    pub fn active(&self) -> bool {
        self.stage != Stage::Resolved
    }

    /// Minutes from the call to the unit arriving.
    pub fn response_ticks(&self) -> Option<u64> {
        Some(self.arrived?.saturating_sub(self.reported?))
    }

    /// Minutes from dispatch to arrival, and from arrival to the end.
    pub fn travel_ticks(&self) -> Option<u64> {
        Some(self.arrived?.saturating_sub(self.dispatched?))
    }

    pub fn work_ticks(&self) -> Option<u64> {
        Some(self.resolved?.saturating_sub(self.arrived?))
    }

    /// Dispatch priority, 0 first: fires and violent crime (lives and
    /// property at stake), then the sick and hurt (crash victims included)
    /// and crash scenes blocking a street, then theft, vandalism and street
    /// takeovers.
    /// Equal priorities go in call order.
    pub fn priority(&self) -> u8 {
        match (self.kind, self.crime) {
            (IncidentKind::Fire, _) => 0,
            (IncidentKind::Crime, Some(c)) if c.violent() => 0,
            (IncidentKind::Medical, _) | (IncidentKind::Crash, _) => 1,
            (IncidentKind::Crime, _) => 2,
        }
    }

    /// What it was, in a word or two ("fire", "robbery", "medical").
    pub fn title(&self) -> &'static str {
        match (self.kind, self.crime) {
            (IncidentKind::Crime, Some(c)) => c.label(),
            (k, _) => k.label(),
        }
    }

    /// Note someone involved (once per role).
    pub fn involve(&mut self, citizen: CitizenId, name: &str, role: &str) {
        if !self.people.iter().any(|p| p.citizen == citizen && p.role == role) {
            self.people.push(Involved { citizen, name: name.to_string(), role: role.to_string(), taken_to: None });
        }
    }

    /// Where someone involved was taken.
    pub fn taken(&mut self, citizen: CitizenId, to: &str) {
        for p in self.people.iter_mut().filter(|p| p.citizen == citizen) {
            p.taken_to = Some(to.to_string());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitState {
    /// In the station.
    Idle,
    Responding,
    OnScene,
    Transporting,
    Returning,
    /// Police on patrol, ambulances on standby: crewed and free, out in the
    /// district (driving to the next beat, or parked on it).
    Patrol,
}

impl UnitState {
    pub fn label(self) -> &'static str {
        match self {
            UnitState::Idle => "in station",
            UnitState::Patrol => "on patrol",
            UnitState::Responding => "responding",
            UnitState::OnScene => "on scene",
            UnitState::Transporting => "transporting",
            UnitState::Returning => "returning",
        }
    }
}

/// One vehicle and its crew.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub kind: UnitKind,
    pub station: BuildingId,
    /// Number at its station, from 1.
    pub number: u8,
    pub state: UnitState,
    /// The car on the road while driving.
    pub car: Option<CarId>,
    pub incident: Option<u32>,
    /// Where it stands when not driving (on scene, at the hospital door).
    pub pos: Vec2,
    pub heading: f32,
    /// Hospital it is driving a patient to.
    pub hospital: Option<BuildingId>,
    /// On patrol, parked on a beat: when it moves on.
    #[serde(default)]
    pub patrol_until: u64,
    /// [services] Its station is gone: it is driving off the map, where it
    /// leaves the world (it takes no calls any more).
    #[serde(default)]
    pub leaving: bool,
}

impl Unit {
    /// On a job: responding, on scene, transporting. Units in the station,
    /// on patrol or driving home are free for the next call.
    pub fn busy(&self) -> bool {
        matches!(self.state, UnitState::Responding | UnitState::OnScene | UnitState::Transporting)
    }

    /// Out of the station (so a crew is aboard).
    pub fn out(&self) -> bool {
        self.state != UnitState::Idle
    }

    /// Lights and sirens: on the way to a job or carrying a patient.
    pub fn lights(&self) -> bool {
        matches!(self.state, UnitState::Responding | UnitState::Transporting | UnitState::OnScene)
    }
}

/// Someone in custody until `until`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Custody {
    pub citizen: CitizenId,
    /// The jail, or the police station holding them.
    pub at: BuildingId,
    pub jailed: bool,
    pub until: u64,
}

/// A patient in a hospital bed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Admission {
    pub citizen: CitizenId,
    pub hospital: BuildingId,
    pub until: u64,
}

/// Figures for the panel, the report and the alerts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceStats {
    /// Share of homes covered, per covering kind (police, fire, health, school).
    pub coverage: [f32; 4],
    pub incidents_today: [u32; 3],
    pub resolved_today: [u32; 3],
    pub unanswered_today: u32,
    pub arrests_today: u32,
    pub response_ticks_today: u64,
    pub responses_today: u32,
    /// Average minutes from call to arrival, yesterday.
    pub avg_response_yesterday: f32,
    pub units: u32,
    pub units_busy: u32,
    pub fires_burning: u32,
    pub burned_down_total: u32,
    pub in_hospital: u32,
    pub waiting_for_ambulance: u32,
    pub upkeep_today: f32,
    pub tickets_today: f32,
    pub stadium_crowd_today: u32,
    pub dispatchers_on_shift: u32,
    pub jailed: u32,
    pub held_at_stations: u32,
    pub released_early_total: u32,
    pub out_of_town_calls_today: u32,
    pub out_of_town_fees_today: f32,
    /// Children who went to school today.
    pub pupils_today: u32,
    /// Hospital bills paid by households today, and what the city covered.
    pub bills_paid_today: f32,
    pub bills_covered_today: f32,
    /// Workers' wage bonus from schooling, summed over the city, today.
    pub education_bonus_today: f32,
    // ── response, per incident kind (fire, crime, medical) ──
    /// Minutes from call to arrival of each call answered today, and yesterday's.
    pub response_min_today: [Vec<u16>; 3],
    pub response_min_yesterday: [Vec<u16>; 3],
    /// Calls past their dispatch time waiting for a unit, now; the most today.
    pub queued: [u32; 3],
    pub queue_max_today: [u32; 3],
    /// Minutes today every unit of the kind was busy, and since when the
    /// current spell has lasted (None when a unit is free).
    pub saturated_min_today: [u32; 3],
    pub saturated_since: [Option<u64>; 3],
    /// Yesterday's saturated minutes, for the alert after midnight.
    pub saturated_min_yesterday: [u32; 3],
    /// Calls that gave up unanswered today, per kind, by why nobody came.
    pub unanswered_why_today: [UnansweredWhy; 3],
    /// Mutual aid from the next town while every unit was busy, today.
    pub mutual_aid_today: u32,
    /// Units pulled off a lower-priority call for a more urgent one, today.
    pub diverted_today: u32,
    /// Units on patrol or standby, now.
    pub on_patrol: u32,
    /// Calls no free unit of ours had a road to (handed to the next town), today.
    pub unreachable_today: u32,
}

/// Why calls went unanswered.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UnansweredWhy {
    /// Every unit of the kind was out on other calls the whole wait.
    pub all_units_busy: u32,
    /// A unit was free but no road led there.
    pub no_route: u32,
    /// No crewed station of the kind and no dispatcher to call for help.
    pub no_station: u32,
}

/// Median and 90th percentile of a list of minutes, and the share at or
/// under `within`.
pub fn response_summary(v: &[u16], within: u16) -> Option<(u16, u16, f32)> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    let at = |q: f32| s[((s.len() - 1) as f32 * q).round() as usize];
    Some((at(0.5), at(0.9), s.iter().filter(|m| **m <= within).count() as f32 / s.len() as f32))
}

impl ServiceStats {
    /// Average minutes from call to arrival today (yesterday's if none yet).
    pub fn avg_response(&self) -> f32 {
        if self.responses_today > 0 {
            self.response_ticks_today as f32 / self.responses_today as f32
        } else {
            self.avg_response_yesterday
        }
    }
}

/// Everything the services system owns.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Services {
    pub units: SlotMap<UnitId, Unit>,
    /// Active incidents and the last few resolved ones.
    pub incidents: Vec<Incident>,
    pub next_id: u32,
    pub admissions: Vec<Admission>,
    /// People in custody: held at a police station or serving time in a jail.
    pub custody: Vec<Custody>,
    /// Arrests per citizen, ever: the criminal record.
    pub record: Vec<(CitizenId, u32)>,
    /// Burned lots: rubble until the tick, nothing built before.
    pub rubble: Vec<(LotId, Vec2, u64)>,
    /// Stadiums hosting an event today.
    pub events_today: Vec<BuildingId>,
    pub stats: ServiceStats,
    /// Coverage per lot, per covering kind; rebuilt when roads or service
    /// buildings change.
    #[serde(skip)]
    pub coverage: HashMap<LotId, [f32; 4]>,
    #[serde(skip)]
    pub coverage_key: Option<u64>,
    /// Unit by the car it drives, rebuilt as units move.
    #[serde(skip)]
    pub by_car: HashMap<CarId, UnitId>,
    /// Queued calls no free unit could reach, and when that was found (they
    /// are retried every five minutes rather than every tick).
    #[serde(skip)]
    pub route_failed: HashMap<u32, u64>,
    /// The services' own random stream (see `World::srng`).
    #[serde(skip)]
    pub rng: Option<crate::SimRng>,
}

impl Services {
    pub fn incident(&self, id: u32) -> Option<&Incident> {
        self.incidents.iter().find(|i| i.id == id)
    }

    pub fn incident_mut(&mut self, id: u32) -> Option<&mut Incident> {
        self.incidents.iter_mut().find(|i| i.id == id)
    }

    pub fn active(&self) -> impl Iterator<Item = &Incident> {
        self.incidents.iter().filter(|i| i.active())
    }

    /// The unit driving `car`, if it is an emergency vehicle.
    pub fn unit_of_car(&self, car: CarId) -> Option<UnitId> {
        self.by_car.get(&car).copied()
    }

    /// Burning buildings and how hard they burn.
    pub fn fires(&self) -> impl Iterator<Item = (BuildingId, f32)> + '_ {
        self.active().filter(|i| i.kind == IncidentKind::Fire && i.intensity > 0.0).map(|i| (i.building, i.intensity))
    }

    pub fn is_burning(&self, b: BuildingId) -> bool {
        self.active().any(|i| i.kind == IncidentKind::Fire && i.building == b)
    }

    /// Arrests on a citizen's record.
    pub fn arrests_of(&self, c: CitizenId) -> u32 {
        self.record.iter().find(|r| r.0 == c).map_or(0, |r| r.1)
    }

    /// People serving time in each jail: (jail, inmates).
    pub fn jail_occupancy(&self) -> Vec<(BuildingId, u32)> {
        let mut out: Vec<(BuildingId, u32)> = Vec::new();
        for c in self.custody.iter().filter(|c| c.jailed) {
            match out.iter_mut().find(|o| o.0 == c.at) {
                Some(o) => o.1 += 1,
                None => out.push((c.at, 1)),
            }
        }
        out
    }

    pub fn in_custody(&self, c: CitizenId) -> bool {
        self.custody.iter().any(|x| x.citizen == c)
    }

    pub fn is_rubble(&self, lot: LotId) -> bool {
        self.rubble.iter().any(|r| r.0 == lot)
    }

    /// Coverage of a lot by one covering kind, `0..1`.
    pub fn coverage_of(&self, lot: LotId, kind: ServiceKind) -> f32 {
        match kind.coverage_index() {
            Some(k) => self.coverage.get(&lot).map_or(0.0, |c| c[k]),
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buildings_tool_categories_hold_every_placeable_once() {
        let mut seen: Vec<Placeable> = Vec::new();
        for cat in BuildCategory::ALL {
            let items = cat.items();
            assert!(!items.is_empty(), "{cat:?} offers nothing");
            for p in items {
                assert_eq!(p.category(), cat, "{p:?} listed under {cat:?}");
                assert!(!seen.contains(&p), "{p:?} listed twice");
                // [tolls] A toll booth is no building: its collectors are in the upkeep.
                let staffed = p.jobs() > 0 || matches!(p, Placeable::Toll(_));
                assert!(p.cost() > 0.0 && p.upkeep() > 0.0 && staffed, "{p:?} has no cost, upkeep or jobs");
                seen.push(p);
            }
        }
        // Power is a sub-category with every plant and every battery size; every service kind is somewhere.
        assert_eq!(BuildCategory::Power.items().len(), PowerPlantKind::ALL.len() + crate::power::BatteryKind::ALL.len());
        for k in ServiceKind::ALL {
            assert!(seen.iter().any(|p| matches!(p, Placeable::Service(s, _) if *s == k)), "{k:?} is not on the menu");
        }
        assert!(seen.contains(&Placeable::BusDepot));
        assert!(crate::tolls::TollSize::ALL.iter().all(|&t| BuildCategory::Transit.items().contains(&Placeable::Toll(t))), "toll booths under Transit");
        assert!(seen.contains(&Placeable::Rail(RailFacility::Station)) && seen.contains(&Placeable::Rail(RailFacility::FreightYard)));
        assert!(seen.contains(&Placeable::Dock(PortKind::ShippingPort)) && seen.contains(&Placeable::Dock(PortKind::FerryTerminal)), "docks live under Water");
        assert_eq!(BuildCategory::Attractions.items().len(), 2);
    }

    #[test]
    fn tiers_scale_cost_capacity_and_reach() {
        for k in ServiceKind::ALL {
            let [l, m, h] = Tier::ALL.map(|t| Placeable::Service(k, t));
            assert!(l.cost() < m.cost() && m.cost() < h.cost(), "{k:?} cost");
            assert!(l.upkeep() < m.upkeep() && m.upkeep() < h.upkeep(), "{k:?} upkeep");
            assert!(l.jobs() <= m.jobs() && m.jobs() < h.jobs(), "{k:?} jobs");
            assert!(k.min_area(Tier::Low) < k.min_area(Tier::High));
            if k.coverage_index().is_some() {
                assert!(l.radius() < m.radius() && m.radius() < h.radius(), "{k:?} radius");
                assert!(l.radius() > COVERAGE_FADE, "{k:?} low tier must cover something fully");
            } else if k != ServiceKind::TrashDepot && k != ServiceKind::ParkingEnforcement {
                assert_eq!(m.radius(), 0.0);
            }
            if k.unit_kind().is_some() {
                assert!(k.units(Tier::Low) < k.units(Tier::High));
                // A fully staffed station can crew every vehicle it has.
                for t in Tier::ALL {
                    assert!(k.jobs(t) as usize >= k.units(t) * k.unit_kind().unwrap().crew(), "{k:?} {t:?} cannot crew its units");
                }
            }
        }
    }

    #[test]
    fn remote_kinds_parse_with_tiers() {
        assert_eq!(Placeable::parse("police", None), Some(Placeable::Service(ServiceKind::Police, Tier::Mid)));
        assert_eq!(Placeable::parse("fire:high", None), Some(Placeable::Service(ServiceKind::Fire, Tier::High)));
        assert_eq!(Placeable::parse("hospital", Some(Tier::Low)), Some(Placeable::Service(ServiceKind::Health, Tier::Low)));
        assert_eq!(Placeable::parse("clinic", None), Some(Placeable::Service(ServiceKind::Health, Tier::Low)));
        assert_eq!(Placeable::parse("Stadium/LOW", None), Some(Placeable::Service(ServiceKind::Stadium, Tier::Low)));
        assert_eq!(Placeable::parse("solar", Some(Tier::High)), Some(Placeable::Plant(PowerPlantKind::Solar)));
        assert_eq!(Placeable::parse("yard", None), Some(Placeable::Rail(RailFacility::FreightYard)));
        assert_eq!(Placeable::parse("geothermal", None), Some(Placeable::Plant(PowerPlantKind::Geothermal)));
        assert_eq!(Placeable::parse("ferry", None), Some(Placeable::Dock(PortKind::FerryTerminal)));
        assert_eq!(Placeable::parse("vertiport", None), Some(Placeable::Service(ServiceKind::Vertiport, Tier::Mid)));
        assert_eq!(Placeable::parse("skyport:high", None), Some(Placeable::Service(ServiceKind::Vertiport, Tier::High)));
        assert_eq!(Placeable::Service(ServiceKind::Vertiport, Tier::Low).category(), BuildCategory::Transit);
        assert_eq!(Placeable::parse("castle", None), None);
    }
}
