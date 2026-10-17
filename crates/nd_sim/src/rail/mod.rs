//! Trains. A trunk line crosses the map from the start; the player lays
//! branches off it and puts stations and freight yards on lots beside the
//! track. Passenger trains run the line end to end on a headway, calling
//! at every station; freight trains come when a yard has goods waiting.
//! [freight] Imports ordered by rail wait out of town ([`Rail::orders`]),
//! are loaded onto the next freight train as it sets off from the edge of
//! the map, ride in on it and come off at their yard; exports are picked
//! up at the yards and leave on it. Wagons hold [`WAGON_CAPACITY`] each.
//! Trains are not saved: they are back on the next headway after a load,
//! and goods that were aboard go back to wait for it.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::roads::{project_on_segment, NodeId, RoadNetwork, SegmentId, END_SNAP_RADIUS, RAIL_HALF_WIDTH};
use crate::traffic::Cargo;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

pub use crate::geom::{path_length, point_along, project_on_poly};

mod facility;
mod freight;
mod track;
mod train;

pub use facility::*;
pub use freight::*;
pub use track::*;
pub use train::*;

new_key_type! {
    pub struct TrainId;
    pub struct FacilityId;
}

/// Where the trunk railway runs, in metres south of the centre.
pub const TRUNK_Y: f32 = -480.0;
/// Tightest curve a train takes; sharper control-point corners are eased.
pub const MIN_RADIUS: f32 = 120.0;
/// Length of the turnout: a branch runs beside the line it leaves before
/// merging, like a real switch, instead of meeting it square on.
pub const SWITCH_LEAD: f32 = 60.0;
/// Control-point turns below this are left as the spline makes them.
pub const EASE_TURN: f32 = 0.35;
/// How far off parallel to the street a platform may lie (radians).
pub const PLATFORM_SKEW: f32 = 0.6;
/// Track or street this close to a placed facility is what it fuses to.
pub const FUSE_RAIL: f32 = 40.0;
pub const FUSE_ROAD: f32 = 40.0;
/// A fused road must lie this close to the facility's road edge.
pub const ROAD_EDGE_FUSE: f32 = 14.0;
/// From the footprint edge to the track centreline.
pub const RAIL_GAP: f32 = 6.0;
/// From the footprint edge to a 1+1 street's centreline (lane, shoulder, pavement).
pub const ROAD_GAP: f32 = 7.6;
/// Connector stubs reach this far past the footprint at each end.
pub const STUB_LEAD: f32 = 24.0;
/// At most this many trains out at once.
pub const MAX_TRAINS: usize = 6;
/// How close to the track a lot must come to host a station or yard.
pub const STATION_REACH: f32 = 24.0;
pub const STATION_COST: f32 = 25_000.0;
pub const STATION_UPKEEP_PER_DAY: f32 = 300.0;
pub const STATION_JOBS: u16 = 8;
pub const STATION_PLATFORM: f32 = 70.0;
/// [subway] The Central Station: a grand hall over long intercity
/// platforms, with a metro station under it and a bus terminal at its
/// front. One per city.
pub const CENTRAL_COST: f32 = 120_000.0;
pub const CENTRAL_UPKEEP_PER_DAY: f32 = 1_100.0;
pub const CENTRAL_JOBS: u16 = 60;
pub const CENTRAL_PLATFORM: f32 = 120.0;
/// People walk further to a Central Station than to a halt.
pub const CENTRAL_REACH: f32 = 420.0;
pub const YARD_COST: f32 = 30_000.0;
pub const YARD_UPKEEP_PER_DAY: f32 = 400.0;
pub const YARD_JOBS: u16 = 20;
pub const YARD_MIN_AREA: f32 = 800.0;
pub const YARD_APRON: f32 = 90.0;
/// Sim minutes between passenger trains, and between freight trains.
pub const PASSENGER_HEADWAY: u64 = 120;
pub const FREIGHT_HEADWAY: u64 = 180;
pub const SERVICE_START: u32 = 5 * 60;
pub const SERVICE_END: u32 = 23 * 60;
pub const PASSENGER_SPEED: f32 = 25.0;
pub const FREIGHT_SPEED: f32 = 15.0;
/// Seconds a train stands at a station or yard.
pub const DWELL_SECONDS: f32 = 30.0;
pub const TRAIN_CAPACITY: usize = 200;
/// How far people walk to a station, and the shortest trip worth a train.
pub const RAIL_REACH: f32 = 240.0;
pub const RAIL_TRIP_MIN: f32 = 500.0;
pub const RAIL_FARE: f32 = 4.0;
/// Share of the import bill a buyer pays when goods come by rail.
pub const RAIL_IMPORT_FACTOR: f32 = 0.8;
/// Export income multiplier for towns with a freight yard.
pub const RAIL_EXPORT_BONUS: f32 = 1.2;
pub const LOCO_LEN: f32 = 18.0;
pub const COACH_LEN: f32 = 19.0;
pub const WAGON_LEN: f32 = 14.0;
/// [freight] Units of goods one wagon carries, and the shortest and
/// longest freight trains. The longest holds more than the largest single
/// order (400 units), so every order fits an empty train.
pub const WAGON_CAPACITY: f32 = 50.0;
pub const MIN_WAGONS: u8 = 5;
pub const MAX_WAGONS: u8 = 12;
/// [freight] An extra freight train may follow this soon after the last
/// when goods are left behind: more waiting than a train holds, or an order
/// that already missed a train.
pub const EXTRA_FREIGHT_GAP: u64 = 45;
/// [freight] An order still waiting out of town after this long goes by
/// road instead (a yard no train reaches, say).
pub const ORDER_GIVE_UP: u64 = FREIGHT_HEADWAY * 4;
pub const COUPLING: f32 = 1.2;
/// A train keeps this much clear track ahead.
pub const BLOCK: f32 = 90.0;
/// [r4] (K50) A train following another comes to a stand this far behind
/// its tail (or its head, coming the other way), braking for it from far
/// enough back to stop there, instead of running up to it at line speed
/// and stopping dead (or into it: at 4 traffic seconds a tick a passenger
/// train covers 100 m, more than the old block).
pub const SAFE_GAP: f32 = 30.0;
/// [r4] Service braking and acceleration, m/s² (an electric multiple
/// unit's, freight too: kept gentle enough to see): a train slows to a stand
/// at its calls, the switch and a train ahead, and picks up speed again.
pub const BRAKE: f32 = 1.0;
pub const ACCEL: f32 = 0.8;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RailStats {
    pub trains: u32,
    pub stations: u32,
    pub yards: u32,
    pub waiting: u32,
    pub riding: u32,
    pub riders_today: u32,
    pub fares_today: f32,
    pub passenger_trains_today: u32,
    pub freight_trains_today: u32,
    /// Units of goods brought in by rail today, and waiting at yards.
    pub freight_in_today: f32,
    pub freight_waiting: f32,
    pub upkeep_today: f32,
    /// Sim minutes until the next passenger train enters.
    pub next_passenger_in: u32,
    /// [pt-transit] Riders today off trains from out of town (visitors,
    /// commuters, residents back from a day out) and onto trains out.
    #[serde(default)]
    pub from_out_of_town_today: u32,
    #[serde(default)]
    pub to_out_of_town_today: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "RailSave", into = "RailSave")]
pub struct Rail {
    pub trains: SlotMap<TrainId, Train>,
    pub stats: RailStats,
    pub last_passenger_tick: u64,
    pub last_freight_tick: u64,
    /// [freight] Imports ordered by rail, waiting out of town for the next
    /// freight train to their yard.
    pub orders: Vec<RailOrder>,
}

/// [freight] What a save keeps of the railway. Trains are not saved, but
/// the imports aboard them are: they go back to wait out of town and ride
/// in on the next train, so a save never loses an order.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RailSave {
    #[serde(default)]
    stats: RailStats,
    #[serde(default)]
    last_passenger_tick: u64,
    #[serde(default)]
    last_freight_tick: u64,
    #[serde(default)]
    orders: Vec<RailOrder>,
}

impl From<RailSave> for Rail {
    fn from(s: RailSave) -> Self {
        Rail { trains: SlotMap::with_key(), stats: s.stats, last_passenger_tick: s.last_passenger_tick, last_freight_tick: s.last_freight_tick, orders: s.orders }
    }
}

impl From<Rail> for RailSave {
    fn from(r: Rail) -> Self {
        let mut orders = r.orders;
        // Aboard a train: back in the queue, ordered when that train left.
        let since = r.last_freight_tick;
        for t in r.trains.values() {
            orders.extend(t.cargo.iter().map(|c| RailOrder { cargo: *c, since }));
        }
        RailSave { stats: r.stats, last_passenger_tick: r.last_passenger_tick, last_freight_tick: r.last_freight_tick, orders }
    }
}

impl Rail {
    pub fn in_service(minute_of_day: u32) -> bool {
        (SERVICE_START..SERVICE_END).contains(&minute_of_day)
    }

    /// Whether `train` must wait: another train's head or tail stands on
    /// this train's track within [`BLOCK`] metres ahead of its head.
    ///
    /// Each point is projected onto this train's polyline once, taking the
    /// single closest point. That assumes a route never passes the same
    /// place twice (no loops, no out-and-back over the same track), which
    /// holds for the trunk-and-branch railway: a train runs the line from
    /// one end to the other. A route that did fold back on itself could
    /// pick the wrong pass and miss a block.
    pub fn blocked(&self, id: TrainId) -> bool {
        self.blocked_by(id).is_some()
    }

    /// [train-inspect] The train whose head or tail stands in `id`'s block
    /// (see [`Rail::blocked`]).
    pub fn blocked_by(&self, id: TrainId) -> Option<TrainId> {
        self.clear_ahead(id).filter(|&(gap, _)| gap < BLOCK).map(|(_, o)| o)
    }

    /// [r4] (K50) The clear track ahead of `id`'s head, and the train that
    /// ends it: the nearest head or tail of another train standing on
    /// `id`'s route ahead of it, 0 when another train's body already spans
    /// its head. `None` when the line ahead is clear.
    pub fn clear_ahead(&self, id: TrainId) -> Option<(f32, TrainId)> {
        let me = self.trains.get(id)?;
        let mut best: Option<(f32, TrainId)> = None;
        for (oid, other) in &self.trains {
            if oid == id {
                continue;
            }
            let head = other.head();
            let tail = point_along(&other.poly, other.s - other.length()).0;
            let on = |p: Vec2| project_on_poly(&me.poly, p).filter(|&(_, d)| d < 3.0).map(|(s, _)| s);
            let (sh, st) = (on(head), on(tail));
            let gap = match (sh, st) {
                // Its body reaches over this train's head.
                (Some(a), Some(b)) if a.min(b) <= me.s && a.max(b) > me.s => Some(0.0),
                _ => [sh, st].into_iter().flatten().filter(|&s| s > me.s).map(|s| s - me.s).min_by(f32::total_cmp),
            };
            if let Some(g) = gap {
                if best.is_none_or(|(b, _)| g < b) {
                    best = Some((g, oid));
                }
            }
        }
        best
    }

    /// [freight] Units of imports waiting out of town for `yard`, and the
    /// number of orders.
    pub fn orders_for(&self, yard: BuildingId) -> (usize, f32) {
        self.orders.iter().filter(|o| o.cargo.from == yard).fold((0, 0.0), |(n, u), o| (n + 1, u + o.cargo.amount))
    }

    /// [freight] Units of imports aboard freight trains for `yard`.
    pub fn aboard_for(&self, yard: BuildingId) -> f32 {
        self.trains.values().flat_map(|t| t.cargo.iter()).filter(|c| c.from == yard).fold(0.0, |a, c| a + c.amount)
    }

    pub fn state(&self, id: TrainId) -> Option<TrainState> {
        let t = self.trains.get(id)?;
        if let Some(at) = t.dwelling_at() {
            return Some(TrainState::Dwelling { at, left: t.dwell });
        }
        if t.held {
            if let Some(by) = self.blocked_by(id) {
                return Some(TrainState::HeldBlock { by: Some(by) });
            }
            if t.branch.is_some_and(|(a, _)| t.s <= a + 0.5) {
                return Some(TrainState::HeldBranch);
            }
            return Some(TrainState::HeldBlock { by: None });
        }
        Some(if t.next_stop >= t.stops.len() { TrainState::Leaving } else { TrainState::Running })
    }
}

#[cfg(test)]
mod tests;
