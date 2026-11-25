//! [r17-goods] The two supply lines of the new goods that do not run
//! through a business's recipe (`world/goods.rs`):
//!
//! - **Hospitals' medicine.** Each hospital uses medicine on its wards (a
//!   little for every bed, more for each patient) and orders it each
//!   morning from a drug maker in town, delivered by lorry, else imported;
//!   the city pays. A hospital out of medicine keeps its patients longer.
//! - **Nuclear fuel.** A nuclear plant makes nothing until its first core
//!   is in: the fuel comes from out of town in a shielded flask on a
//!   low-loader, escorted front and back, when the plant is built and
//!   again before each core runs out (`FUEL_DAYS`). The city pays for each
//!   load. Other plants are not fuelled (Harvey 2026-11-22: "otherwise i
//!   dont care about fueling powerplants").

use crate::buildings::BuildingId;
use crate::zoning::LotId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Medicine a hospital uses a day per bed it has, empty or not (the
/// clinic's shelves, the wards' round).
pub const MEDICINE_PER_BED_DAY: f32 = 0.08;
/// ... and per patient in a bed, a day.
pub const MEDICINE_PER_PATIENT_DAY: f32 = 0.5;
/// Days of use a hospital keeps on its shelves (it orders up to this).
pub const MEDICINE_KEEP_DAYS: f32 = 3.0;
/// It orders when stock and loads on the way fall under this many days.
pub const MEDICINE_REORDER_DAYS: f32 = 1.5;
/// The hour a hospital's order goes out.
pub const MEDICINE_ORDER_HOUR: u64 = 7;
/// What a stay lasts against the usual when the hospital has run out.
pub const SHORT_STAY_FACTOR: f32 = 1.5;

/// Days a core of nuclear fuel runs a plant.
pub const FUEL_DAYS: u64 = 20;
/// The next convoy is sent this many days before the core runs out.
pub const FUEL_LEAD_DAYS: u64 = 2;
/// A plant still without its fuel tries again for a convoy after this many
/// hours (the last one found no road, or was lost).
pub const CONVOY_RETRY_HOURS: u64 = 12;
/// What one load of fuel costs the city.
pub const FUEL_LOAD_PRICE: f32 = 4000.0;

/// One nuclear plant's fuel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NuclearFuel {
    /// The tick its core runs out (0: never fuelled).
    pub until: u64,
    /// The tick the convoy under way was sent, if one is.
    pub convoy: Option<u64>,
    /// Loads delivered so far.
    pub loads: u32,
}

impl NuclearFuel {
    pub fn fuelled(&self, tick: u64) -> bool {
        self.until > tick
    }
}

/// The day's counts, for the report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoodsDay {
    /// Medicine the hospitals used, bought from drug makers in town, and
    /// imported.
    pub medicine_used: f32,
    pub medicine_local: f32,
    pub medicine_imported: f32,
    /// Hospitals that ran out of medicine at some hour.
    pub hospitals_short: u32,
    /// Nuclear convoys sent and arrived.
    pub convoys_sent: u32,
    pub convoys_arrived: u32,
}

/// The saved state (`World::goods`). Old saves load with none: their
/// hospitals start with a few days of medicine, their nuclear plants with
/// a fresh core.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GoodsBook {
    /// Medicine on each hospital's shelves.
    pub medicine: BTreeMap<BuildingId, f32>,
    /// Medicine ordered and on its way to each hospital.
    pub medicine_inbound: BTreeMap<BuildingId, f32>,
    /// Each nuclear plant's fuel, by its lot.
    pub nuclear: BTreeMap<LotId, NuclearFuel>,
    pub today: GoodsDay,
    pub yesterday: GoodsDay,
}

/// A hospital's use of medicine a day: its beds and its patients.
pub fn medicine_per_day(beds: u32, patients: u32) -> f32 {
    beds as f32 * MEDICINE_PER_BED_DAY + patients as f32 * MEDICINE_PER_PATIENT_DAY
}
