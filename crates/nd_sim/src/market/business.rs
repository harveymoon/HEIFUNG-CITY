//! A business on its building: stock, money, prices and its day's books.

use super::*;

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
    /// Books closed since opening; the market reading
    /// (`Economy::business_cover`) skips the stocking-up first day.
    #[serde(default)]
    pub days_open: u16,
    /// A truck and bus lot: lorries parked here between jobs.
    #[serde(default)]
    pub fleet_trucks: u16,
    /// The lorries parked here, longest first: whose they are and since when.
    #[serde(default)]
    pub fleet_log: Vec<ParkedLorry>,
    /// The last [`Business::BOOK_DAYS`] days' books, oldest first: what it
    /// took and what it paid out each day (wages, inputs, extra tax), for
    /// the inspector's cash-flow sparkline. Written when the day closes.
    #[serde(default)]
    pub books: Vec<DayBook>,
    /// [economy] Why it opened, in words, when something in the city asked
    /// for it (a pressure, a dock the player built); `None` for businesses
    /// the market picked (`World::why_business` explains those).
    #[serde(default)]
    pub origin: Option<String>,
    /// [economy] Money given by visitors who loved the place (galleries).
    #[serde(default)]
    pub donations: f32,
    /// [economy] Doing well enough to want the plots next door: the land
    /// agent's hook for growth (a gallery into a museum). Nothing acts on
    /// it yet.
    #[serde(default)]
    pub wants_expand: bool,
    /// [camera] The resident who owns it, when one put their money in (a
    /// taxi firm's founder). The inspector pins it among their places.
    #[serde(default)]
    pub owner: Option<crate::citizens::CitizenId>,
    /// [relocate] Where it stood before a buyout moved it, in words
    /// ("Kessler Tower, by Lucky Bodega, after road works").
    #[serde(default)]
    pub moved_from: Option<String>,
    /// [relocate] Its old staff, hired back when the new building opens.
    #[serde(default)]
    pub rehire: Vec<crate::citizens::CitizenId>,
}

/// One closed day of a business's books.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DayBook {
    pub revenue: f32,
    pub spend: f32,
}

/// A lorry parked at a truck and bus lot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParkedLorry {
    /// The business it works for (null when unknown).
    pub home: BuildingId,
    pub since: u64,
    #[serde(default)]
    pub model: crate::vehicle::VehicleModel,
}

impl Business {
    pub const PARKING_KIND: BusinessKind = BusinessKind::Parking;

    pub fn new(kind: BusinessKind, jobs: u16) -> Self {
        Self {
            kind,
            stock: BTreeMap::new(),
            // [dirty-industry] Heavy plant that sells out of town opens
            // with an investor's money: its first loads only leave once two
            // days of output stand in the yard.
            money: (150.0 + jobs as f32 * 40.0) * if matches!(kind, BusinessKind::Smelter | BusinessKind::Feedlot | BusinessKind::ChemicalWorks) { 5.0 } else { 1.0 },
            revenue_today: 0.0,
            spend_today: 0.0,
            deals: 0,
            broke_days: 0,
            closed: false,
            inbound: BTreeMap::new(),
            days_open: 0,
            fleet_trucks: 0,
            fleet_log: Vec::new(),
            books: Vec::new(),
            origin: None,
            donations: 0.0,
            wants_expand: false,
            owner: None,
            moved_from: None,
            rehire: Vec::new(),
        }
    }

    /// [economy] Days of the last `n` closed that took less than they paid out.
    pub fn losing_days(&self, n: usize) -> usize {
        self.books.iter().rev().take(n).filter(|d| d.revenue < d.spend).count()
    }

    /// Days of books kept: a month is enough to read a trend, and the list
    /// stays small in the save.
    pub const BOOK_DAYS: usize = 30;

    /// Close a day's books into the bounded history.
    pub fn close_books(&mut self, revenue: f32, spend: f32) {
        if self.books.len() >= Self::BOOK_DAYS {
            self.books.remove(0);
        }
        self.books.push(DayBook { revenue, spend });
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
        // Scale by the scarcest input, and by the room left for the output:
        // with full shelves the business stops working its inputs instead of
        // burning stock it bought into output that has nowhere to go.
        let mut scale = 1.0f32;
        for (r, per) in recipe.inputs {
            let need = per * cap;
            if need > 0.0 {
                scale = scale.min(self.stock_of(*r) / need);
            }
        }
        for (r, per) in recipe.outputs {
            let make = per * cap;
            if make > 0.0 {
                let cap_stock = per * jobs as f32 * JOB_RATE * 96.0;
                scale = scale.min((cap_stock - self.stock_of(*r)).max(0.0) / make);
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
