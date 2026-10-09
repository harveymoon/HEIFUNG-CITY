//! Rail freight: orders, wagons and manifests.

use super::*;

/// [freight] Imports ordered by rail, waiting out of town for a freight
/// train: the load (`from` is the yard it comes off at) and the tick it was
/// ordered.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RailOrder {
    pub cargo: Cargo,
    #[serde(default)]
    pub since: u64,
}

/// [freight] Load a freight train at the edge of the map: the orders for
/// `yards` (the ones it calls at), oldest first, as many as fit in
/// `capacity`. An order that does not fit is left for the next train and
/// smaller ones behind it may go instead; an empty train always takes the
/// first, so no order is too big to ever leave. Orders for other yards stay.
pub fn fill_wagons(orders: &mut Vec<RailOrder>, yards: &[BuildingId], capacity: f32) -> Vec<Cargo> {
    let mut aboard: Vec<Cargo> = Vec::new();
    let mut load = 0.0f32;
    orders.sort_by_key(|o| o.since);
    orders.retain(|o| {
        if !yards.contains(&o.cargo.from) {
            return true;
        }
        if load + o.cargo.amount <= capacity + 1e-3 || aboard.is_empty() {
            load += o.cargo.amount;
            aboard.push(o.cargo);
            false
        } else {
            true
        }
    });
    aboard
}

/// [freight] Wagons for a freight train with `units` to carry.
pub fn wagons_for(units: f32) -> u8 {
    ((units / WAGON_CAPACITY).ceil().max(0.0) as u32).clamp(MIN_WAGONS as u32, MAX_WAGONS as u32) as u8
}

/// [train-inspect] One load of goods a freight train brings in from out of
/// town: what and how much, the yard it comes off at, and who it is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreightLoad {
    pub resource: crate::market::Resource,
    pub amount: f32,
    /// What the buyer pays for it.
    pub price: f32,
    /// The yard it is set down at.
    pub yard: BuildingId,
    /// The business that ordered it.
    pub buyer: BuildingId,
    /// The cargo hub it passes through on the way to the buyer, if any.
    pub hub: Option<BuildingId>,
}

/// [train-inspect] Everything on a freight train and still to be picked up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FreightManifest {
    /// [freight] Imports aboard (loaded at the edge of the map), to be set
    /// down at the yards ahead, in call order.
    pub loads: Vec<FreightLoad>,
    /// [freight] Wagons, the units they hold and the units aboard.
    pub wagons: u8,
    pub capacity: f32,
    pub aboard: f32,
    /// Units of the town's exports aboard, going out of town.
    pub exports_aboard: f32,
    /// Exports waiting at the yards ahead, picked up when it calls (as
    /// much as the wagons have room for then).
    pub exports_to_collect: Vec<(BuildingId, f32)>,
    /// Units of imports already set down at the yards it called at.
    pub unloaded: f32,
}

impl FreightManifest {
    pub fn import_units(&self) -> f32 {
        self.loads.iter().map(|l| l.amount).sum()
    }
}
