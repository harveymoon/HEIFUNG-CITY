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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resource {
    Produce,
    Grain,
    Flour,
    BakedGoods,
    Meat,
    Dairy,
    Groceries,
    Parts,
    Electronics,
    Goods,
}

impl Resource {
    pub const ALL: [Resource; 10] = [
        Resource::Produce,
        Resource::Grain,
        Resource::Flour,
        Resource::BakedGoods,
        Resource::Meat,
        Resource::Dairy,
        Resource::Groceries,
        Resource::Parts,
        Resource::Electronics,
        Resource::Goods,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Resource::Produce => "produce",
            Resource::Grain => "grain",
            Resource::Flour => "flour",
            Resource::BakedGoods => "baked goods",
            Resource::Meat => "meat",
            Resource::Dairy => "dairy",
            Resource::Groceries => "groceries",
            Resource::Parts => "parts",
            Resource::Electronics => "electronics",
            Resource::Goods => "goods",
        }
    }

    /// Reference price per unit; the outer market trades around it.
    pub fn base_price(self) -> f32 {
        match self {
            Resource::Produce => 2.0,
            Resource::Grain => 1.0,
            Resource::Flour => 1.6,
            Resource::BakedGoods => 4.5,
            Resource::Meat => 7.0,
            Resource::Dairy => 3.0,
            Resource::Groceries => 6.0,
            Resource::Parts => 9.0,
            Resource::Electronics => 45.0,
            Resource::Goods => 60.0,
        }
    }

    /// Sold to citizens directly, not to other businesses.
    pub fn is_retail(self) -> bool {
        matches!(self, Resource::Groceries | Resource::BakedGoods | Resource::Goods)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BusinessKind {
    VertFarm,
    GrainFarm,
    ProteinVat,
    Dairy,
    Mill,
    Fab,
    Assembly,
    Grocery,
    Bakery,
    GadgetShop,
    Venue,
    Office,
    /// Sells parking by the visit; no goods.
    Parking,
}

/// What a business turns into what, per filled job per hour.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub inputs: &'static [(Resource, f32)],
    pub outputs: &'static [(Resource, f32)],
}

impl BusinessKind {
    pub fn label(self) -> &'static str {
        match self {
            BusinessKind::VertFarm => "vertical farm",
            BusinessKind::GrainFarm => "grain farm",
            BusinessKind::ProteinVat => "protein vat",
            BusinessKind::Dairy => "dairy",
            BusinessKind::Mill => "mill",
            BusinessKind::Fab => "parts fab",
            BusinessKind::Assembly => "assembly plant",
            BusinessKind::Grocery => "grocery",
            BusinessKind::Bakery => "bakery",
            BusinessKind::GadgetShop => "gadget shop",
            BusinessKind::Venue => "venue",
            BusinessKind::Office => "office",
            BusinessKind::Parking => "parking",
        }
    }

    pub fn recipe(self) -> Recipe {
        match self {
            BusinessKind::VertFarm => Recipe { inputs: &[], outputs: &[(Resource::Produce, 3.0)] },
            BusinessKind::GrainFarm => Recipe { inputs: &[], outputs: &[(Resource::Grain, 4.0)] },
            BusinessKind::ProteinVat => Recipe { inputs: &[(Resource::Grain, 1.0)], outputs: &[(Resource::Meat, 1.0)] },
            BusinessKind::Dairy => Recipe { inputs: &[(Resource::Grain, 0.5)], outputs: &[(Resource::Dairy, 1.5)] },
            BusinessKind::Mill => Recipe { inputs: &[(Resource::Grain, 2.0)], outputs: &[(Resource::Flour, 2.0)] },
            BusinessKind::Fab => Recipe { inputs: &[], outputs: &[(Resource::Parts, 1.0)] },
            BusinessKind::Assembly => Recipe { inputs: &[(Resource::Parts, 2.0)], outputs: &[(Resource::Electronics, 0.4)] },
            BusinessKind::Grocery => Recipe {
                inputs: &[(Resource::Produce, 1.0), (Resource::Flour, 0.4), (Resource::Meat, 0.3), (Resource::Dairy, 0.4)],
                outputs: &[(Resource::Groceries, 3.0)],
            },
            BusinessKind::Bakery => Recipe { inputs: &[(Resource::Flour, 1.0)], outputs: &[(Resource::BakedGoods, 2.0)] },
            BusinessKind::GadgetShop => Recipe { inputs: &[(Resource::Electronics, 0.5)], outputs: &[(Resource::Goods, 0.5)] },
            BusinessKind::Venue => Recipe { inputs: &[(Resource::Groceries, 0.3)], outputs: &[] },
            BusinessKind::Office => Recipe { inputs: &[], outputs: &[] },
            BusinessKind::Parking => Recipe { inputs: &[], outputs: &[] },
        }
    }

    /// Candidate kinds for a land use, picked at construction.
    pub fn candidates(land_use: LandUse) -> &'static [BusinessKind] {
        match land_use {
            LandUse::Industrial => &[BusinessKind::VertFarm, BusinessKind::GrainFarm, BusinessKind::ProteinVat, BusinessKind::Dairy, BusinessKind::Mill, BusinessKind::Fab, BusinessKind::Assembly],
            LandUse::Commercial => &[BusinessKind::Grocery, BusinessKind::Grocery, BusinessKind::Bakery, BusinessKind::GadgetShop, BusinessKind::Office],
            LandUse::Entertainment => &[BusinessKind::Venue],
            LandUse::Civic => &[BusinessKind::Office],
            _ => &[],
        }
    }

    /// Stock a business tries to keep of each input, in hours of production.
    pub fn target_hours(self) -> f32 {
        match self {
            BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Venue => 36.0,
            _ => 24.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Business {
    pub kind: BusinessKind,
    pub stock: BTreeMap<Resource, f32>,
    pub money: f32,
    /// Revenue and spending today, for the inspector.
    pub revenue_today: f32,
    pub spend_today: f32,
    pub deals: u32,
    /// Days in a row the business could not pay its way.
    pub broke_days: u8,
    pub closed: bool,
    /// Deliveries on their way, so the business does not reorder.
    #[serde(default)]
    pub inbound: BTreeMap<Resource, f32>,
}

impl Business {
    pub const PARKING_KIND: BusinessKind = BusinessKind::Parking;

    pub fn new(kind: BusinessKind, jobs: u16) -> Self {
        Self {
            kind,
            stock: BTreeMap::new(),
            money: 150.0 + jobs as f32 * 40.0,
            revenue_today: 0.0,
            spend_today: 0.0,
            deals: 0,
            broke_days: 0,
            closed: false,
            inbound: BTreeMap::new(),
        }
    }

    pub fn inbound_of(&self, r: Resource) -> f32 {
        self.inbound.get(&r).copied().unwrap_or(0.0)
    }

    pub fn stock_of(&self, r: Resource) -> f32 {
        self.stock.get(&r).copied().unwrap_or(0.0)
    }

    pub fn add(&mut self, r: Resource, amount: f32) {
        *self.stock.entry(r).or_insert(0.0) += amount;
    }

    pub fn take(&mut self, r: Resource, amount: f32) -> f32 {
        let have = self.stock_of(r);
        let taken = amount.min(have);
        self.stock.insert(r, have - taken);
        taken
    }

    /// Units of each output the business can make per hour at `staffing` (0..1).
    pub fn hourly_capacity(&self, jobs: u16, staffing: f32) -> f32 {
        jobs as f32 * staffing * JOB_RATE
    }

    /// Target stock of an input: enough for `target_hours` at full staffing.
    pub fn target_input(&self, r: Resource, jobs: u16) -> f32 {
        let per_hour = self.kind.recipe().inputs.iter().find(|(x, _)| *x == r).map(|(_, a)| *a).unwrap_or(0.0);
        per_hour * jobs as f32 * JOB_RATE * self.kind.target_hours()
    }

    /// Selling price of an output: base, up when stock is scarce, down when it
    /// piles up.
    pub fn sell_price(&self, r: Resource, jobs: u16) -> f32 {
        let per_hour = self.kind.recipe().outputs.iter().find(|(x, _)| *x == r).map(|(_, a)| *a).unwrap_or(1.0);
        let target = (per_hour * jobs as f32 * JOB_RATE * 24.0).max(1.0);
        let ratio = (self.stock_of(r) / target).clamp(0.0, 3.0);
        let k = 1.5 - 0.4 * ratio;
        r.base_price() * k.clamp(0.6, 1.5)
    }

    pub fn sells(&self, r: Resource) -> bool {
        self.kind.recipe().outputs.iter().any(|(x, _)| *x == r)
    }

    pub fn buys(&self, r: Resource) -> bool {
        self.kind.recipe().inputs.iter().any(|(x, _)| *x == r)
    }

    /// Run one hour of production, limited by inputs on hand.
    pub fn produce(&mut self, jobs: u16, staffing: f32) -> f32 {
        let recipe = self.kind.recipe();
        if recipe.outputs.is_empty() || jobs == 0 {
            return 0.0;
        }
        let cap = self.hourly_capacity(jobs, staffing);
        // Scale by the scarcest input.
        let mut scale = 1.0f32;
        for (r, per) in recipe.inputs {
            let need = per * cap;
            if need > 0.0 {
                scale = scale.min(self.stock_of(*r) / need);
            }
        }
        let scale = scale.clamp(0.0, 1.0);
        if scale <= 0.0 {
            return 0.0;
        }
        for (r, per) in recipe.inputs {
            self.take(*r, per * cap * scale);
        }
        let mut made = 0.0;
        for (r, per) in recipe.outputs {
            // Outputs cap at a few days of production so stock does not grow forever.
            let cap_stock = per * jobs as f32 * JOB_RATE * 96.0;
            let amount = (per * cap * scale).min((cap_stock - self.stock_of(*r)).max(0.0));
            self.add(*r, amount);
            made += amount;
        }
        made
    }
}

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
}
