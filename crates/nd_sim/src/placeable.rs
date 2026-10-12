//! What the Buildings tool can place: every service, plant, rail
//! facility, port and the rest, with its category, cost, jobs, footprint
//! limits and the land it may stand on. [review] Moved out of
//! `services.rs`: it spans power, rail, ports and tolls; `services`
//! re-exports it, so `crate::services::Placeable` still names it.

use crate::buildings::Tier;
use crate::ports::PortKind;
use crate::power::PowerPlantKind;
use crate::rail::RailFacility;
use crate::services::ServiceKind;
use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

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
            BuildCategory::Services => [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School, ServiceKind::University, ServiceKind::Dispatch, ServiceKind::Jail, ServiceKind::TrashDepot, ServiceKind::ParkingEnforcement].iter().map(|&k| Placeable::Service(k, Tier::Mid)).collect(),
            BuildCategory::Transit => {
                let mut v = vec![Placeable::BusDepot, Placeable::MetroStation, Placeable::Service(ServiceKind::Vertiport, Tier::Mid)];
                v.extend(crate::enforcement::LotSize::OFFERED.iter().map(|&s| Placeable::ParkingLot(s)));
                v.extend(crate::tolls::TollSize::OFFERED.iter().map(|&s| Placeable::Toll(s)));
                v
            }
            BuildCategory::Rail => vec![Placeable::Rail(RailFacility::Station), Placeable::Rail(RailFacility::Central), Placeable::Rail(RailFacility::FreightYard), Placeable::CargoHub],
            BuildCategory::Water => PortKind::ALL.iter().map(|&k| Placeable::Dock(k)).collect(),
            // [attractions] The monument and stadium, then the landmarks (one of each per city).
            BuildCategory::Attractions => [ServiceKind::Monument, ServiceKind::Stadium].into_iter().chain(ServiceKind::LANDMARKS).map(|k| Placeable::Service(k, Tier::Mid)).collect(),
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

    /// Whether the player picks a tier for it. [attractions] A landmark
    /// attraction comes in one size.
    pub fn has_tiers(self) -> bool {
        matches!(self, Placeable::Service(k, _) if !k.is_landmark())
    }

    pub fn with_tier(self, tier: Tier) -> Placeable {
        match self {
            Placeable::Service(k, _) if k.is_landmark() => Placeable::Service(k, Tier::Mid),
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
                ServiceKind::University => "University",
                ServiceKind::ArtMuseum => "Art museum",
                ServiceKind::ScienceMuseum => "Natural history & science museum",
                ServiceKind::HistoryMuseum => "City history & design museum",
                ServiceKind::Pier => "Pleasure pier (sea shore)",
                ServiceKind::GrandPark => "Grand city park",
                ServiceKind::Zoo => "Zoo",
                ServiceKind::Observatory => "Observatory (high ground)",
                ServiceKind::Aquarium => "Aquarium",
                ServiceKind::BotanicalGarden => "Botanical garden",
                ServiceKind::ConcertHall => "Opera & concert hall",
                ServiceKind::AmusementPark => "Amusement park",
                ServiceKind::ConventionCentre => "Convention centre",
                ServiceKind::Cathedral => "Cathedral",
                ServiceKind::CasinoResort => "Casino resort",
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
            Placeable::Dock(PortKind::FerryTerminalLarge) => "Ferry terminal, large (car ferries)",
            Placeable::Dock(PortKind::MarinaSmall) => "Marina, small",
            Placeable::Dock(PortKind::MarinaLarge) => "Marina, large (yachts)",
            Placeable::Dock(PortKind::FishingPortSmall) => "Fishing port, small",
            Placeable::Dock(PortKind::FishingPortLarge) => "Fishing port, large",
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

    /// [plant-scale] How thick its site must be (twice the area over the
    /// perimeter), m; 0 for any shape.
    pub fn min_thickness(self) -> f32 {
        match self {
            Placeable::Plant(k) => k.min_thickness(),
            // [attractions] A park, a zoo or a garden is not a strip along
            // the kerb: room for lakes, enclosures and a glasshouse.
            Placeable::Service(ServiceKind::GrandPark, _) => 16.0,
            Placeable::Service(ServiceKind::Zoo, _) => 14.0,
            Placeable::Service(ServiceKind::BotanicalGarden, _) => 12.0,
            Placeable::Service(k, _) if k.is_landmark() => 9.0,
            _ => 0.0,
        }
    }

    /// [plant-scale] Most unzoned ground free placement joins into a site
    /// for it, m²: a plant's [`crate::power::PowerPlantKind::max_area`],
    /// for anything else three times what it needs (a works yard at least).
    pub fn max_site_area(self) -> f32 {
        match self {
            Placeable::Plant(k) => k.max_area(),
            _ => (self.min_area() * 3.0).max(1200.0),
        }
    }

    /// [attractions] How many neighbouring plots a site for it may take in
    /// (`grow_site_steps`): a landmark spans a score of small plots.
    pub fn join_steps(self) -> usize {
        match self {
            Placeable::Service(k, _) if k.is_landmark() => 48,
            _ => 12,
        }
    }

    /// Whether it goes on a lot of this land use. [free-placement] The zone
    /// is no bar to the city's own buildings: the ground leaves the zone
    /// when one goes down (unzoned ground takes them too). Only park land
    /// is kept, but for a metro entrance (a kiosk on a plaza fits in a
    /// park); rail pieces, docks and tolls are not on lots at all. What
    /// does matter physically is checked elsewhere: road access, a street
    /// out front, the lot's size, the site (river, coast, hot spot).
    pub fn allowed_on(self, lu: LandUse) -> bool {
        match self {
            Placeable::Rail(_) | Placeable::Dock(_) | Placeable::Toll(_) | Placeable::MetroStation => true,
            _ => lu != LandUse::Park,
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
            "ferry_terminal_large" | "large_ferry" | "car_ferry" | "ferry_port" => Some(Placeable::Dock(PortKind::FerryTerminalLarge)),
            "depot" | "bus_depot" => Some(Placeable::BusDepot),
            "station" => Some(Placeable::Rail(RailFacility::Station)),
            "freight_yard" | "yard" => Some(Placeable::Rail(RailFacility::FreightYard)),
            "central_station" | "central" | "main_station" => Some(Placeable::Rail(RailFacility::Central)),
            "metro_station" | "metro" | "subway_station" | "subway" => Some(Placeable::MetroStation),
            "cargo_hub" | "hub" | "cargo" => Some(Placeable::CargoHub),
            "clinic" => Some(Placeable::Service(ServiceKind::Health, tier.unwrap_or(Tier::Low))),
            other if crate::enforcement::LotSize::from_key(other).is_some() => crate::enforcement::LotSize::from_key(other).map(Placeable::ParkingLot),
            other if crate::tolls::TollSize::from_key(other).is_some() => crate::tolls::TollSize::from_key(other).map(Placeable::Toll),
            other => ServiceKind::from_key(other).map(|k| Placeable::Service(k, tier.unwrap_or(Tier::Mid))).map(|p| p.with_tier(tier.unwrap_or(Tier::Mid))),
        }
    }
}

pub fn parse_tier(s: &str) -> Option<Tier> {
    Tier::from_key(s)
}
