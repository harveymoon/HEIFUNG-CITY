//! The services: their kinds and tiers, costs, jobs, reach and names.

use super::*;

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
    /// [megacorp] A university: a large campus that grows round itself
    /// (dorms, offices, a theatre, sports, frats, plazas) and draws
    /// students (`crate::corp::Campus`, `world/campus.rs`).
    University,
}

impl ServiceKind {
    pub const ALL: [ServiceKind; 12] = [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School, ServiceKind::Dispatch, ServiceKind::Monument, ServiceKind::Stadium, ServiceKind::Jail, ServiceKind::TrashDepot, ServiceKind::Vertiport, ServiceKind::ParkingEnforcement, ServiceKind::University];
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
            ServiceKind::University => "university",
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
            "university" | "campus" | "college" => Some(ServiceKind::University),
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
            ServiceKind::University => "university",
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
            ServiceKind::University => ["community college", "university", "institute of technology"],
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
            ServiceKind::University => Hours::new(8, 22),
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
            ServiceKind::University => 40_000.0,
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
            ServiceKind::University => 650.0,
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
            ServiceKind::University => 30,
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
            // [megacorp] Students enrolled.
            ServiceKind::University => [120, 320, 800][tier.index()],
            _ => 0,
        }
    }

    pub fn min_area(self, tier: Tier) -> f32 {
        let base = match self {
            ServiceKind::Monument => 250.0,
            // [big-sites] A stadium wants a big footprint: several plots,
            // joined when it is put down.
            ServiceKind::Stadium => 2400.0,
            ServiceKind::Jail => 450.0,
            ServiceKind::Dispatch => 250.0,
            ServiceKind::TrashDepot => 500.0,
            ServiceKind::Vertiport => 350.0,
            ServiceKind::ParkingEnforcement => 200.0,
            // [megacorp] A large campus.
            ServiceKind::University => 1100.0,
            _ => 400.0,
        };
        base * [0.75, 1.0, 1.6][tier.index()]
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
            ServiceKind::University => (360.0 * k, 0.14 * k),
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
            ServiceKind::University => {
                let uni = ["Ashgrove", "Vantmoor", "Kestrel Bay", "Halden", "Corvin", "Saltmarsh", "Ivel", "Larkspur", "Thornbury", "Westhollow"][n as usize % 10];
                format!("{uni} {}", ["Community College", "University", "Institute of Technology"][tier.index()])
            }
        }
    }
}

/// The service a building provides, stored on the building.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSite {
    pub kind: ServiceKind,
    pub tier: Tier,
}
