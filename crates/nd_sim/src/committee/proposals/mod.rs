//! [committee] Proposals: the actions the game can actually do, their
//! validation (a proposal is only tabled once it validates), the topics the
//! rules suggest to each member, how each member leans on a vote, the
//! candidate sites the code finds for a passed proposal (the model never
//! picks coordinates), and the mayor's implementation.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::report::{areas, find_area, in_area, thousands, Area, CityReport};
use super::{mix, Committee, CommitteeProject, MemoryEntry, Politician, ProjectStatus, Proposal, Scope, Temper, Vote};
use crate::buildings::Tier;
use crate::power::PowerPlantKind;
use crate::services::{Placeable, ServiceKind};
use crate::tax::TaxCategory;
use crate::zoning::{LandUse, ParcelKey};
use crate::World;

mod actions;
mod implement;
mod sites;
mod suggest;

pub use actions::*;
pub use sites::*;
pub use suggest::*;

/// What `World::remediate` charges per cell of full contamination
/// (`world/dirt.rs` `CLEANUP_COST`), for the estimate only: implementing
/// charges the real price.
const CLEANUP_COST_ESTIMATE: f32 = 450.0;
/// Biggest tax change one proposal may make, in percentage points.
pub const MAX_TAX_STEP_PCT: f32 = 5.0;

/// A structured proposal, as the members table it and models write it:
/// `{"action": "build_service", "service": "health", "near": "Northside"}`.
/// Free-text fields so a model's slightly-off spelling still parses; the
/// validation decides, and normalises what it accepts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Police, fire, health, school, trash depot or 911 dispatch.
    BuildService {
        service: String,
        #[serde(default)]
        tier: String,
        /// An area name; empty: wherever it is needed most.
        #[serde(default)]
        near: String,
    },
    /// Solar, wind or coal.
    BuildPower {
        plant: String,
        #[serde(default)]
        near: String,
    },
    BuildDepot {
        #[serde(default)]
        near: String,
    },
    /// Zone free ground in an area: residential, commercial, industrial,
    /// entertainment or park.
    Rezone { area: String, to: String },
    AddBusLine { from: String, to: String },
    /// A tax category key (`residential_mid`) or a land use group
    /// (`residential`, `commercial`, `industrial`, `entertainment`), raised or
    /// cut by `delta_pct` percentage points.
    ChangeTax { category: String, delta_pct: f32 },
    /// A toll booth's price (`toll`: its id; none: every booth).
    SetToll {
        #[serde(default)]
        toll: Option<u32>,
        price: f32,
    },
    /// Road maintenance funding (1.0 is what the network needs).
    RoadRepair { funding: f32 },
    /// Dig out the worst contaminated ground in an area.
    Remediate { area: String },
}

/// The action kinds and their fields, for prompts, the API's help and the
/// docs.
pub const ACTION_HELP: &str = r#"build_service {service: police|fire|health|school|trash_depot|dispatch, tier: low|mid|high, near: <area>}
build_power {plant: solar|wind|coal, near: <area>}
build_depot {near: <area>}
rezone {area: <area>, to: residential|commercial|industrial|entertainment|park}
add_bus_line {from: <area>, to: <area>}
change_tax {category: residential|commercial|industrial|entertainment|<category key>, delta_pct: -5..5}
set_toll {toll: <id or omit for all>, price: 0..50}
road_repair {funding: 0..1.5}
remediate {area: <area>}"#;

const SERVICES: [ServiceKind; 6] = [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School, ServiceKind::TrashDepot, ServiceKind::Dispatch];

