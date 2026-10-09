//! Dispatch: how supply, batteries and the regional grid meet demand each
//! hour.

use super::*;

/// Power the regional grid will sell the city, in megawatts, and its mean
/// price per megawatt-hour (the price moves with the hour and the weather:
/// [`import_price`]). Enough for a village, not a city.
pub const GRID_IMPORT_MW: f32 = 60.0;

pub const GRID_IMPORT_PRICE: f32 = 2.5;

/// What the regional grid will take off the city's hands, in megawatts,
/// and what it pays per megawatt-hour on average ([`export_price`]): well
/// under the import price (the
/// region buys at wholesale and sells at retail), so exporting pays for a
/// surplus but buying to sell back never does.
pub const GRID_EXPORT_MW: f32 = 120.0;

pub const GRID_EXPORT_PRICE: f32 = 0.5;

/// How an hour's grid balance is met, worked out by [`dispatch`] from the
/// supply, the demand and the batteries' headroom; the world applies it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dispatch {
    /// Into the batteries (grid side), MW, and the share of every
    /// battery's room that is used (`0..1`).
    pub charge_mw: f32,
    pub charge_share: f32,
    /// Out of the batteries, MW, and the share of every battery's
    /// available output that is used.
    pub discharge_mw: f32,
    pub discharge_share: f32,
    pub exported_mw: f32,
    pub imported_mw: f32,
    pub curtailed_mw: f32,
    /// [batteries] Of `imported_mw`, what was bought to charge the
    /// batteries (part of `charge_mw` too).
    pub grid_charge_mw: f32,
}

/// The merit order: a surplus charges the batteries first, is exported
/// with what is left (up to the export line) and curtailed beyond that; a
/// shortfall drains the batteries first and is imported with what is left
/// (up to the import line). Every battery takes or gives the same share of
/// its own headroom, so packs fill and empty together. `room` and
/// `available` are the batteries' summed headroom in MW.
pub fn dispatch(supply: f32, demand: f32, room: f32, available: f32) -> Dispatch {
    let mut d = Dispatch::default();
    let net = supply - demand;
    if net > 0.0 {
        d.charge_mw = net.min(room);
        d.charge_share = if room > 0.0 { d.charge_mw / room } else { 0.0 };
        let left = net - d.charge_mw;
        d.exported_mw = left.min(GRID_EXPORT_MW);
        d.curtailed_mw = left - d.exported_mw;
    } else if net < 0.0 {
        let need = -net;
        d.discharge_mw = need.min(available);
        d.discharge_share = if available > 0.0 { d.discharge_mw / available } else { 0.0 };
        d.imported_mw = (need - d.discharge_mw).min(GRID_IMPORT_MW);
    }
    d
}

/// [batteries] [`dispatch`] with the batteries playing the day's prices
/// (`mode`): a surplus always charges them first; in a cheap hour they
/// also buy from the regional grid (within its import line) to fill up;
/// in a shortfall they deliver first only in a dear hour, otherwise the
/// city imports first and they give only what the import line cannot
/// (reliability before money: nobody sits in the dark to save a charge).
pub fn dispatch_tou(supply: f32, demand: f32, room: f32, available: f32, mode: BatteryMode) -> Dispatch {
    if mode == BatteryMode::Deliver {
        return dispatch(supply, demand, room, available);
    }
    let mut d = Dispatch::default();
    let net = supply - demand;
    if net >= 0.0 {
        d = dispatch(supply, demand, room, available);
        if mode == BatteryMode::Charge {
            let buy = (room - d.charge_mw).max(0.0).min(GRID_IMPORT_MW);
            d.grid_charge_mw = buy;
            d.imported_mw += buy;
            d.charge_mw += buy;
        }
    } else {
        let need = -net;
        d.imported_mw = need.min(GRID_IMPORT_MW);
        d.discharge_mw = (need - d.imported_mw).min(available);
        if mode == BatteryMode::Charge && d.discharge_mw <= 0.0 {
            let buy = room.min(GRID_IMPORT_MW - d.imported_mw).max(0.0);
            d.grid_charge_mw = buy;
            d.imported_mw += buy;
            d.charge_mw = buy;
        }
    }
    d.charge_share = if room > 0.0 { (d.charge_mw / room).min(1.0) } else { 0.0 };
    d.discharge_share = if available > 0.0 { (d.discharge_mw / available).min(1.0) } else { 0.0 };
    d
}

impl Dispatch {
    /// [batteries] What the batteries saved this hour, in money: imports
    /// they spared (what they delivered, at the import price) less what
    /// was bought from the grid to charge them, less the exports they
    /// cost (surplus they took that the export line could have sold).
    pub fn battery_savings(&self, import_price: f32, export_price: f32) -> f32 {
        let from_surplus = (self.charge_mw - self.grid_charge_mw).max(0.0);
        let forgone = from_surplus.min((GRID_EXPORT_MW - self.exported_mw).max(0.0));
        self.discharge_mw * import_price - self.grid_charge_mw * import_price - forgone * export_price
    }
}
