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
    // [attractions] The city's landmark attractions, one of each per city
    // (`ServiceKind::is_landmark`): several plots joined into one site,
    // a real lift to the land value round them, and a draw for visitors.
    // Appended so saved kinds keep their order.
    /// An art museum: galleries round a sculpture court.
    ArtMuseum,
    /// A natural history and science museum: skeletons and a planetarium.
    ScienceMuseum,
    /// The city history and design museum.
    HistoryMuseum,
    /// A pleasure pier on the sea shore, built out over the water on
    /// piles: rides, a ferris wheel, an arcade and food stands.
    Pier,
    /// A grand city park over many plots: lakes, paths and pavilions.
    GrandPark,
    /// A zoo: enclosures, an aviary and a visitors' loop.
    Zoo,
    /// An observatory on high ground.
    Observatory,
    /// An aquarium: tanks, a shark tunnel and a touch pool.
    Aquarium,
    /// A botanical garden with a glass conservatory.
    BotanicalGarden,
    /// An opera and concert hall.
    ConcertHall,
}

impl ServiceKind {
    pub const ALL: [ServiceKind; 22] = [
        ServiceKind::Police,
        ServiceKind::Fire,
        ServiceKind::Health,
        ServiceKind::School,
        ServiceKind::Dispatch,
        ServiceKind::Monument,
        ServiceKind::Stadium,
        ServiceKind::Jail,
        ServiceKind::TrashDepot,
        ServiceKind::Vertiport,
        ServiceKind::ParkingEnforcement,
        ServiceKind::University,
        ServiceKind::ArtMuseum,
        ServiceKind::ScienceMuseum,
        ServiceKind::HistoryMuseum,
        ServiceKind::Pier,
        ServiceKind::GrandPark,
        ServiceKind::Zoo,
        ServiceKind::Observatory,
        ServiceKind::Aquarium,
        ServiceKind::BotanicalGarden,
        ServiceKind::ConcertHall,
    ];
    /// [attractions] The landmark attractions, in menu order.
    pub const LANDMARKS: [ServiceKind; 10] = [
        ServiceKind::ArtMuseum,
        ServiceKind::ScienceMuseum,
        ServiceKind::HistoryMuseum,
        ServiceKind::Pier,
        ServiceKind::GrandPark,
        ServiceKind::Zoo,
        ServiceKind::Observatory,
        ServiceKind::Aquarium,
        ServiceKind::BotanicalGarden,
        ServiceKind::ConcertHall,
    ];
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
            ServiceKind::ArtMuseum => "art_museum",
            ServiceKind::ScienceMuseum => "science_museum",
            ServiceKind::HistoryMuseum => "history_museum",
            ServiceKind::Pier => "pier",
            ServiceKind::GrandPark => "grand_park",
            ServiceKind::Zoo => "zoo",
            ServiceKind::Observatory => "observatory",
            ServiceKind::Aquarium => "aquarium",
            ServiceKind::BotanicalGarden => "botanical_garden",
            ServiceKind::ConcertHall => "concert_hall",
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
            "art_museum" | "art_gallery" | "gallery" => Some(ServiceKind::ArtMuseum),
            "science_museum" | "natural_history_museum" | "natural_history" | "planetarium" => Some(ServiceKind::ScienceMuseum),
            "history_museum" | "city_museum" | "design_museum" | "museum" => Some(ServiceKind::HistoryMuseum),
            "pier" | "pleasure_pier" | "amusement_pier" => Some(ServiceKind::Pier),
            "grand_park" | "central_park" | "city_park" => Some(ServiceKind::GrandPark),
            "zoo" => Some(ServiceKind::Zoo),
            "observatory" => Some(ServiceKind::Observatory),
            "aquarium" => Some(ServiceKind::Aquarium),
            "botanical_garden" | "botanic_garden" | "conservatory" | "botanical" => Some(ServiceKind::BotanicalGarden),
            "concert_hall" | "opera_house" | "opera" | "philharmonic" => Some(ServiceKind::ConcertHall),
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
            ServiceKind::ArtMuseum => "art museum",
            ServiceKind::ScienceMuseum => "natural history and science museum",
            ServiceKind::HistoryMuseum => "city history and design museum",
            ServiceKind::Pier => "pleasure pier",
            ServiceKind::GrandPark => "grand city park",
            ServiceKind::Zoo => "zoo",
            ServiceKind::Observatory => "observatory",
            ServiceKind::Aquarium => "aquarium",
            ServiceKind::BotanicalGarden => "botanical garden",
            ServiceKind::ConcertHall => "opera and concert hall",
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
            // [attractions] One size each: the label is the kind's.
            k => [k.label(); 3],
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
            // [attractions] Museums by day; the pier till one in the
            // morning; the park never shuts; the observatory from the
            // afternoon into the night, when the stars are out.
            ServiceKind::ArtMuseum => Hours::new(10, 19),
            ServiceKind::ScienceMuseum => Hours::new(9, 18),
            ServiceKind::HistoryMuseum => Hours::new(10, 18),
            ServiceKind::Pier => Hours::new(10, 1),
            ServiceKind::GrandPark => Hours::ALWAYS,
            ServiceKind::Zoo => Hours::new(9, 18),
            ServiceKind::Observatory => Hours::new(14, 0),
            ServiceKind::Aquarium => Hours::new(9, 20),
            ServiceKind::BotanicalGarden => Hours::new(8, 20),
            ServiceKind::ConcertHall => Hours::new(17, 0),
        }
    }

    pub fn is_attraction(self) -> bool {
        matches!(self, ServiceKind::Monument | ServiceKind::Stadium) || self.is_landmark()
    }

    /// [attractions] One of the landmark attractions: one per city, on a
    /// joined site, one size.
    pub fn is_landmark(self) -> bool {
        Self::LANDMARKS.contains(&self)
    }

    /// [attractions] A landmark people walk round (a park, a zoo, a
    /// garden, the pier): its own walkways and benches, joined to the
    /// walk graph and the Path tool's gates.
    pub fn has_grounds(self) -> bool {
        matches!(self, ServiceKind::GrandPark | ServiceKind::Zoo | ServiceKind::BotanicalGarden | ServiceKind::Pier)
    }

    /// [attractions] An afternoon out (citizens' park trips find it).
    pub fn day_out(self) -> bool {
        matches!(self, ServiceKind::ArtMuseum | ServiceKind::ScienceMuseum | ServiceKind::HistoryMuseum | ServiceKind::Pier | ServiceKind::GrandPark | ServiceKind::Zoo | ServiceKind::Aquarium | ServiceKind::BotanicalGarden)
    }

    /// [attractions] A night out (citizens' evenings find it).
    pub fn night_out(self) -> bool {
        matches!(self, ServiceKind::Pier | ServiceKind::Observatory | ServiceKind::ConcertHall)
    }

    /// [attractions] What a visit costs at the door (citizens and
    /// visitors alike, to the treasury); the park is free, the pier's
    /// rides are paid as they go.
    pub fn admission(self) -> f32 {
        match self {
            ServiceKind::ArtMuseum => 8.0,
            ServiceKind::ScienceMuseum => 10.0,
            ServiceKind::HistoryMuseum => 6.0,
            ServiceKind::Pier => 6.0,
            ServiceKind::Zoo => 14.0,
            ServiceKind::Observatory => 8.0,
            ServiceKind::Aquarium => 16.0,
            ServiceKind::BotanicalGarden => 6.0,
            ServiceKind::ConcertHall => 12.0,
            _ => 0.0,
        }
    }

    /// [attractions] How strongly a landmark draws visitors from out of
    /// town (tourism attractiveness points, `crate::tourism`).
    pub fn tourist_pull(self) -> f32 {
        match self {
            ServiceKind::ArtMuseum => 26.0,
            ServiceKind::ScienceMuseum => 28.0,
            ServiceKind::HistoryMuseum => 20.0,
            ServiceKind::Pier => 38.0,
            ServiceKind::GrandPark => 32.0,
            ServiceKind::Zoo => 34.0,
            ServiceKind::Observatory => 18.0,
            ServiceKind::Aquarium => 30.0,
            ServiceKind::BotanicalGarden => 22.0,
            ServiceKind::ConcertHall => 26.0,
            _ => 0.0,
        }
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
            ServiceKind::ArtMuseum => 60_000.0,
            ServiceKind::ScienceMuseum => 70_000.0,
            ServiceKind::HistoryMuseum => 45_000.0,
            ServiceKind::Pier => 85_000.0,
            ServiceKind::GrandPark => 90_000.0,
            ServiceKind::Zoo => 95_000.0,
            ServiceKind::Observatory => 55_000.0,
            ServiceKind::Aquarium => 75_000.0,
            ServiceKind::BotanicalGarden => 65_000.0,
            ServiceKind::ConcertHall => 85_000.0,
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
            ServiceKind::ArtMuseum => 520.0,
            ServiceKind::ScienceMuseum => 600.0,
            ServiceKind::HistoryMuseum => 420.0,
            ServiceKind::Pier => 780.0,
            ServiceKind::GrandPark => 900.0,
            ServiceKind::Zoo => 1100.0,
            ServiceKind::Observatory => 420.0,
            ServiceKind::Aquarium => 720.0,
            ServiceKind::BotanicalGarden => 640.0,
            ServiceKind::ConcertHall => 820.0,
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
            // [attractions] Curators, keepers, ride operators, gardeners,
            // ushers and the cafe.
            ServiceKind::ArtMuseum => 24,
            ServiceKind::ScienceMuseum => 28,
            ServiceKind::HistoryMuseum => 18,
            ServiceKind::Pier => 34,
            ServiceKind::GrandPark => 30,
            ServiceKind::Zoo => 40,
            ServiceKind::Observatory => 14,
            ServiceKind::Aquarium => 28,
            ServiceKind::BotanicalGarden => 22,
            ServiceKind::ConcertHall => 32,
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
            // [attractions] Visitors at once.
            ServiceKind::ArtMuseum => 260,
            ServiceKind::ScienceMuseum => 320,
            ServiceKind::HistoryMuseum => 200,
            ServiceKind::Pier => 600,
            ServiceKind::GrandPark => 1500,
            ServiceKind::Zoo => 700,
            ServiceKind::Observatory => 120,
            ServiceKind::Aquarium => 360,
            ServiceKind::BotanicalGarden => 500,
            ServiceKind::ConcertHall => 600,
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
            // [attractions] Each wants several plots (or one mega plot):
            // joined when it is put down, like a stadium's.
            k if k.is_landmark() => ATTRACTION_AREA[Self::LANDMARKS.iter().position(|x| *x == k).unwrap_or(0)],
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
            // [attractions] A landmark lifts a whole quarter: its own
            // land value channel (`Channel::Attractions`), weighed by how
            // well staffed it is.
            ServiceKind::ArtMuseum => (400.0, 0.32),
            ServiceKind::ScienceMuseum => (400.0, 0.32),
            ServiceKind::HistoryMuseum => (360.0, 0.28),
            ServiceKind::Pier => (460.0, 0.34),
            ServiceKind::GrandPark => (540.0, 0.42),
            ServiceKind::Zoo => (420.0, 0.28),
            ServiceKind::Observatory => (320.0, 0.26),
            ServiceKind::Aquarium => (400.0, 0.30),
            ServiceKind::BotanicalGarden => (440.0, 0.32),
            ServiceKind::ConcertHall => (440.0, 0.34),
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
            ServiceKind::ArtMuseum => ["Neon Museum of Modern Art", "Kowloon Kunsthalle", "Museum of Light and Signal", "The Chromatic Gallery", "Harbor Museum of Art"][n as usize % 5].to_string(),
            ServiceKind::ScienceMuseum => ["Museum of Natural History and Science", "The Fossil and Star Hall", "Ridgeline Science Museum", "Hall of Deep Time", "City Science Centre"][n as usize % 5].to_string(),
            ServiceKind::HistoryMuseum => match n % 3 {
                0 => format!("{place} City History Museum"),
                1 => "Museum of the City and Design".to_string(),
                _ => format!("{place} Design Museum"),
            },
            ServiceKind::Pier => format!("{place} Pleasure Pier"),
            ServiceKind::GrandPark => ["Central Park", "Grand Park", "The Commons", "Founders Park", "Lantern Park"][n as usize % 5].to_string(),
            ServiceKind::Zoo => format!("{place} Zoo"),
            ServiceKind::Observatory => ["Ridgeline Observatory", "Star Hill Observatory", "Griffin Observatory", "High Lantern Observatory"][n as usize % 4].to_string(),
            ServiceKind::Aquarium => format!("{place} Aquarium"),
            ServiceKind::BotanicalGarden => format!("{place} Botanical Garden"),
            ServiceKind::ConcertHall => ["Neon Opera House", "Harbor Philharmonic Hall", "The Glass Sails Concert Hall", "City Opera and Concert Hall"][n as usize % 4].to_string(),
        }
    }
}

/// [attractions] The smallest site each landmark attraction goes on, m²,
/// in `ServiceKind::LANDMARKS` order: well over an ordinary plot (a
/// parcel along a street is some 400 to 900 m²), so each takes several.
pub const ATTRACTION_AREA: [f32; 10] = [2200.0, 2600.0, 1800.0, 1200.0, 7000.0, 5200.0, 1500.0, 2400.0, 3800.0, 2600.0];

/// The service a building provides, stored on the building.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSite {
    pub kind: ServiceKind,
    pub tier: Tier,
}
