//! Cargo hubs: the data and the rules (systems in `world/cargo_hubs.rs`).
//!
//! A cargo hub is a freight terminal the player places with the Buildings
//! tool (as in Cities: Skylines). Without one, every load a business imports
//! by road comes in its own long-haul semi from the map edge, through the
//! streets to the buyer's door; rail and sea imports are trucked one lorry
//! per order from the yard or the port. With a hub within reach of a buyer:
//!
//! - road imports are consolidated: orders wait at the hub until a semi is
//!   full (`TRUNK_LOAD`) or the oldest has waited `TRUNK_WAIT`, then one
//!   semi brings them all from the edge to the hub, and they come a little
//!   cheaper (`ROAD_IMPORT_FACTOR`);
//! - rail and sea imports for the hub's buyers are addressed to the hub; a
//!   freight yard or shipping port within `SIDING_REACH` unloads straight
//!   into it (its own siding or quay), farther ones truck them over;
//! - from the hub, its own local lorries (delivery vans and box trucks)
//!   take each load to its buyer and come back to park in the hub's bays;
//! - exports gather at the hub and leave in full semis, earning a little
//!   more (`EXPORT_BONUS`).
//!
//! The market code (`world/market.rs`) is unchanged in what it buys and
//! sells: the hub only decides how imported goods travel. Every load is
//! accounted for: a hub that closes hands what it holds on to the buyers.

use crate::buildings::BuildingId;
use crate::market::Resource;
use crate::traffic::CarId;
use serde::{Deserialize, Serialize};

pub const HUB_COST: f32 = 9000.0;
pub const HUB_UPKEEP_PER_DAY: f32 = 140.0;
pub const HUB_JOBS: u16 = 24;
pub const HUB_MIN_AREA: f32 = 900.0;
/// Buyers within this distance of a hub get their imports through it.
pub const HUB_REACH: f32 = 1800.0;
/// A freight yard or shipping port this close unloads into the hub.
pub const SIDING_REACH: f32 = 500.0;
/// Units one long-haul semi brings in.
pub const TRUNK_LOAD: f32 = 60.0;
/// Ticks the oldest road order waits for a semi to fill before it goes.
pub const TRUNK_WAIT: u64 = 30;
/// Consolidated road freight costs this share of the import price.
pub const ROAD_IMPORT_FACTOR: f32 = 0.9;
/// The hub's handling fee: a share of each load's value forwarded.
pub const HANDLING_FEE: f32 = 0.03;
/// Exports through a hub earn this much more.
pub const EXPORT_BONUS: f32 = 1.06;
/// Units of exports per semi leaving.
pub const EXPORT_TRUNK: f32 = 80.0;
/// Loads put on local lorries per dispatch round.
pub const FORWARD_PER_ROUND: usize = 4;

/// How a load reaches the hub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Via {
    /// In a consolidated semi from the map edge.
    #[default]
    Road,
    /// On a freight train, through a freight yard.
    Rail,
    /// On a freighter, through a shipping port.
    Sea,
}

impl Via {
    pub fn label(self) -> &'static str {
        match self {
            Via::Road => "road",
            Via::Rail => "rail",
            Via::Sea => "sea",
        }
    }
}

/// One business's import order passing through a hub.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HubLoad {
    pub buyer: BuildingId,
    pub resource: Resource,
    pub amount: f32,
    /// What the buyer paid for it.
    pub price: f32,
    /// Tick it was ordered.
    pub since: u64,
    #[serde(default)]
    pub via: Via,
}

/// What one hub holds and has on the way.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HubState {
    /// Road orders waiting for a semi to fill.
    pub waiting: Vec<HubLoad>,
    /// Road orders aboard the semi on its way in, and that semi.
    pub trunk: Vec<HubLoad>,
    pub trunk_car: Option<CarId>,
    /// Rail and sea orders on their way (queued at a yard or port).
    pub inbound: Vec<HubLoad>,
    /// In the shed, waiting for a local lorry.
    pub ready: Vec<HubLoad>,
    /// Export units gathered for the next semi out.
    pub outbound: f32,
}

impl HubState {
    /// Units held or expected, by stage: waiting, aboard the trunk semi,
    /// inbound by rail or sea, in the shed.
    pub fn units(&self) -> [f32; 4] {
        let sum = |v: &[HubLoad]| v.iter().map(|l| l.amount).sum::<f32>() + 0.0;
        [sum(&self.waiting), sum(&self.trunk), sum(&self.inbound), sum(&self.ready)]
    }

    /// Take the inbound load a delivered `amount` of `resource` is (the
    /// oldest that matches), if any.
    pub fn take_inbound(&mut self, resource: Resource, amount: f32) -> Option<HubLoad> {
        let i = self.inbound.iter().position(|l| l.resource == resource && (l.amount - amount).abs() < 0.01)?;
        Some(self.inbound.remove(i))
    }

    /// The next semi's worth of waiting road orders, oldest first.
    pub fn fill_trunk(&mut self) -> Vec<HubLoad> {
        let mut load = 0.0;
        let mut n = 0;
        for l in &self.waiting {
            if n > 0 && load + l.amount > TRUNK_LOAD {
                break;
            }
            load += l.amount;
            n += 1;
        }
        self.waiting.drain(..n).collect()
    }

    /// Is a semi due: full, or the oldest order waited long enough?
    pub fn trunk_due(&self, tick: u64) -> bool {
        self.trunk_car.is_none() && !self.waiting.is_empty() && (self.waiting.iter().map(|l| l.amount).sum::<f32>() >= TRUNK_LOAD || tick.saturating_sub(self.waiting[0].since) >= TRUNK_WAIT)
    }
}

/// The day's freight numbers, for the Transit window and the API.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HubStats {
    /// Loads into the hubs today (and units).
    pub received_today: u32,
    pub received_units_today: f32,
    /// Loads sent on to their buyers on local lorries today.
    pub forwarded_today: u32,
    /// Semis in from the edge with consolidated road orders, and out with exports.
    pub trunks_in_today: u32,
    pub trunks_out_today: u32,
    /// Road orders that came in a shared semi instead of one each: long-haul
    /// trips through the city saved.
    pub long_haul_saved_today: u32,
    pub long_haul_saved_yesterday: u32,
    pub fees_today: f32,
    pub received_total: u32,
    pub forwarded_total: u32,
    pub long_haul_saved_total: u32,
}

/// Every hub's state and the freight numbers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CargoHubs {
    #[serde(default)]
    pub hubs: std::collections::BTreeMap<BuildingId, HubState>,
    #[serde(default)]
    pub stats: HubStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(amount: f32, since: u64) -> HubLoad {
        HubLoad { buyer: BuildingId::default(), resource: Resource::Goods, amount, price: amount * 60.0, since, via: Via::Road }
    }

    #[test]
    fn a_semi_goes_when_full_or_when_the_oldest_has_waited() {
        let mut h = HubState::default();
        assert!(!h.trunk_due(100));
        h.waiting.push(load(10.0, 100));
        assert!(!h.trunk_due(110), "not full, not waited");
        assert!(h.trunk_due(100 + TRUNK_WAIT), "waited long enough");
        h.waiting.push(load(TRUNK_LOAD, 105));
        assert!(h.trunk_due(106), "full");
        h.trunk_car = Some(CarId::default());
        assert!(!h.trunk_due(500), "one semi at a time");
    }

    #[test]
    fn a_trunk_takes_a_semi_load_oldest_first() {
        let mut h = HubState::default();
        for i in 0..10 {
            h.waiting.push(load(15.0, i));
        }
        let t = h.fill_trunk();
        assert_eq!(t.len(), 4);
        assert!(t.iter().map(|l| l.amount).sum::<f32>() <= TRUNK_LOAD);
        assert_eq!(t[0].since, 0);
        assert_eq!(h.waiting.len(), 6);
        // A single load bigger than a semi still goes (alone).
        let mut h = HubState::default();
        h.waiting.push(load(200.0, 0));
        assert_eq!(h.fill_trunk().len(), 1);
    }

    #[test]
    fn inbound_loads_are_matched_by_what_arrives() {
        let mut h = HubState::default();
        h.inbound.push(HubLoad { resource: Resource::Fuel, ..load(12.0, 1) });
        h.inbound.push(load(12.0, 2));
        assert!(h.take_inbound(Resource::Parts, 12.0).is_none());
        let l = h.take_inbound(Resource::Goods, 12.0).unwrap();
        assert_eq!(l.since, 2);
        assert_eq!(h.inbound.len(), 1);
        assert_eq!(h.units()[2], 12.0);
    }
}
