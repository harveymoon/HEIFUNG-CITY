//! Resources, businesses and the market.
//!
//! Every non-residential building hosts a business with a recipe: inputs it
//! buys, outputs it makes, at a rate that scales with how many of its jobs
//! are filled. Stock, money and prices live on the business. The market is
//! just the set of businesses: a buyer asks for the cheapest seller of a
//! resource including haulage, strikes a deal, and a truck carries it. An
//! outer market imports and exports at worse prices so chains never deadlock.

use crate::buildings::BuildingId;
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use crate::clock::Hours;

mod business;
mod business_kind;
mod resource;

pub use business::*;
pub use business_kind::*;
pub use resource::*;

/// Throughput of one filled job per hour, in recipe units.
pub const JOB_RATE: f32 = 0.3;
/// Trucks allowed on the road at once; beyond this deliveries are instant.
pub const MAX_TRUCKS: usize = 60;

/// Import and export with the world outside the city. Imports cost more than
/// local goods, exports pay less, so a local supply chain is worth building.
pub const IMPORT_MARKUP: f32 = 1.7;
pub const EXPORT_DISCOUNT: f32 = 0.45;
/// Haulage cost per unit per metre, added to a seller's price when comparing.
pub const HAULAGE: f32 = 0.0012;

/// Best local seller of `r` for a buyer at `at`: cheapest delivered price.
pub fn best_seller<'a>(sellers: impl Iterator<Item = (BuildingId, &'a Business, u16, Vec2)>, r: Resource, at: Vec2, amount: f32) -> Option<(BuildingId, f32)> {
    let mut best: Option<(BuildingId, f32)> = None;
    for (id, b, jobs, pos) in sellers {
        if b.closed || !b.sells(r) || b.stock_of(r) < amount * 0.5 {
            continue;
        }
        let price = b.sell_price(r, jobs) + HAULAGE * pos.distance(at) * r.base_price();
        if best.map_or(true, |(_, p)| price < p) {
            best = Some((id, price));
        }
    }
    best
}

/// Per-resource market statistics for the UI.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketStats {
    pub avg_price: BTreeMap<Resource, f32>,
    pub stock: BTreeMap<Resource, f32>,
    pub deals_today: u32,
    pub imports_today: f32,
    pub exports_today: f32,
    pub businesses: u32,
    pub closed_total: u32,
    pub meals_sold_today: u32,
    pub hungry: u32,
    /// Groceries bought from outside today because local shelves ran short.
    #[serde(default)]
    pub grocery_imports_today: f32,
    /// Shoppers who found a shop shut or full today.
    #[serde(default)]
    pub turned_away_today: u32,
    /// [tenants] Empty units let to a new business, ever.
    #[serde(default)]
    pub tenants_let_total: u32,
    /// [tenants] Empty units taken by a business growing next door, ever.
    #[serde(default)]
    pub units_taken_total: u32,
}

#[cfg(test)]
mod tests;
