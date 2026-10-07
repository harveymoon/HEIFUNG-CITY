use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

/// How far into the red a citizen's savings may go. Below this, shops,
/// venues and car parks refuse them: without a floor, consumer debt grows
/// without limit and every purchase is money printed on the spot.
pub const OVERDRAFT: f32 = -200.0;
/// Daily gig income of someone without a job.
pub const GIG_INCOME: f32 = 40.0;

/// A named source or sink of treasury money.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Flow {
    /// A share of every citizen's daily income, by the tier of their home
    /// (`crate::tax`; 12% as standard).
    IncomeTax,
    /// A share of every open business's daily revenue, by its kind
    /// (`crate::tax`; 5% as standard).
    BusinessTax,
    BusFares,
    RailFares,
    PlantUpkeep,
    /// Electricity bought from the regional grid.
    GridImport,
    /// Buses and depots.
    TransitUpkeep,
    /// Stations and yards.
    RailUpkeep,
    /// Plants, depots, stations and yards placed by the player.
    Construction,
    /// Police, fire, health, schools, dispatch and attractions.
    ServiceUpkeep,
    /// Stadium tickets.
    Tickets,
    /// [parking] Parking tickets.
    ParkingFines,
    /// [parking] Fees at the parking lots the city runs.
    ParkingFees,
    /// [tourism] The city's tax on every hotel night (`crate::tourism`).
    TouristTax,
    /// [subway] Metro fares.
    SubwayFares,
    /// [subway] Metro stations, tunnels and train runs.
    SubwayUpkeep,
    /// [tax] Duty on goods imported from outside.
    ImportDuty,
    /// [tolls] Tolls paid at the city's toll booths.
    Tolls,
    /// [tolls] Staff and upkeep of the toll booths.
    TollUpkeep,
    /// [power] Surplus electricity sold to the regional grid
    /// (`crate::power::GRID_EXPORT_PRICE`).
    GridExport,
    /// [events] Resurfacing worn streets (`crate::road_wear`).
    RoadMaintenance,
    /// [events] Fines from busted street races and raves.
    Fines,
}

/// Number of `Flow`s; the ledger's row length.
pub const FLOWS: usize = 22;

impl Flow {
    pub const ALL: [Flow; FLOWS] = [Flow::IncomeTax, Flow::BusinessTax, Flow::BusFares, Flow::RailFares, Flow::PlantUpkeep, Flow::GridImport, Flow::TransitUpkeep, Flow::RailUpkeep, Flow::Construction, Flow::ServiceUpkeep, Flow::Tickets, Flow::ParkingFines, Flow::ParkingFees, Flow::TouristTax, Flow::SubwayFares, Flow::SubwayUpkeep, Flow::ImportDuty, Flow::Tolls, Flow::TollUpkeep, Flow::GridExport, Flow::RoadMaintenance, Flow::Fines];

    pub fn label(self) -> &'static str {
        match self {
            Flow::IncomeTax => "income tax",
            Flow::BusinessTax => "business tax",
            Flow::BusFares => "bus fares",
            Flow::RailFares => "rail fares",
            Flow::PlantUpkeep => "plant upkeep",
            Flow::GridImport => "grid import",
            Flow::TransitUpkeep => "transit upkeep",
            Flow::RailUpkeep => "rail upkeep",
            Flow::Construction => "construction",
            Flow::ServiceUpkeep => "service upkeep",
            Flow::Tickets => "tickets",
            Flow::ParkingFines => "parking fines",
            Flow::ParkingFees => "city parking fees",
            Flow::TouristTax => "tourist tax",
            Flow::SubwayFares => "subway fares",
            Flow::SubwayUpkeep => "subway upkeep",
            Flow::ImportDuty => "import duty",
            Flow::Tolls => "road tolls",
            Flow::TollUpkeep => "toll booth upkeep",
            Flow::GridExport => "grid export",
            Flow::RoadMaintenance => "road maintenance",
            Flow::Fines => "fines",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// What a ledger posting was for, one level finer than its `Flow`: the
/// Economy window's drill-down ("service upkeep" is which station, clinic
/// and car park; "income tax" is which class paid it). The UI names a
/// building by looking it up, so a demolished one shows as gone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    /// A building the city runs or charges: plant, service, depot, station,
    /// yard, port, car park, stadium.
    Building(crate::buildings::BuildingId),
    /// Income tax by the payer's income class.
    Class(crate::buildings::Tier),
    /// Business tax by the land use of the business's building.
    LandUse(LandUse),
    /// Fares by bus line.
    Route(crate::transit::RouteId),
    /// Anything else, named: the bus fleet, help from the next town, bills.
    Named(String),
}

/// One line of the itemised ledger: the total of every posting of `item`
/// under `flow` in the day, and how many postings made it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemEntry {
    pub flow: Flow,
    pub item: Item,
    pub amount: f32,
    #[serde(default)]
    pub count: u32,
}

/// Where the treasury's money came from and went, by source and sink, for
/// today so far and for the whole of yesterday. Positive is income.
///
/// `items_*` break the rows down further where the posting said what it
/// was for (`Economy::credit_item`); postings made with plain `credit`
/// stay in the row only, so a row can be more than the sum of its items
/// and the UI shows the rest as "not itemised". One entry per item a day,
/// so the lists are bounded by the buildings, lines and classes in town.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(deserialize_with = "ledger_row")]
    pub today: [f32; FLOWS],
    #[serde(deserialize_with = "ledger_row")]
    pub yesterday: [f32; FLOWS],
    #[serde(default)]
    pub items_today: Vec<ItemEntry>,
    #[serde(default)]
    pub items_yesterday: Vec<ItemEntry>,
}

/// A ledger row from a save, which may predate the newer flows: shorter
/// rows are padded with zeros.
fn ledger_row<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[f32; FLOWS], D::Error> {
    struct Row;
    impl<'de> serde::de::Visitor<'de> for Row {
        type Value = [f32; FLOWS];
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "up to {FLOWS} numbers")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = [0.0; FLOWS];
            let mut i = 0;
            while let Some(x) = seq.next_element::<f32>()? {
                if i < FLOWS {
                    out[i] = x;
                }
                i += 1;
            }
            Ok(out)
        }
    }
    d.deserialize_tuple(FLOWS, Row)
}

/// A fixed-size array saved when the enum it is indexed by was shorter
/// (new business kinds since): missing entries take their default, extra
/// ones are dropped, so older saves keep loading.
pub(crate) fn padded<'de, D, T, const N: usize>(d: D) -> Result<[T; N], D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default + Copy,
{
    struct Pad<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: serde::Deserialize<'de> + Default + Copy, const N: usize> serde::de::Visitor<'de> for Pad<T, N> {
        type Value = [T; N];
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "up to {N} values")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = [T::default(); N];
            let mut i = 0;
            while let Some(x) = seq.next_element::<T>()? {
                if i < N {
                    out[i] = x;
                }
                i += 1;
            }
            Ok(out)
        }
    }
    d.deserialize_tuple(N, Pad::<T, N>(std::marker::PhantomData))
}

/// [economy] One value per business kind (`BusinessKind::index`). More
/// kinds than serde's arrays reach, so it is saved as a tuple by hand and
/// loads padded like the arrays before it (`padded`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindArray<T>(pub [T; crate::market::BusinessKind::COUNT]);

impl<T: Default + Copy> Default for KindArray<T> {
    fn default() -> Self {
        KindArray([T::default(); crate::market::BusinessKind::COUNT])
    }
}

impl<T> std::ops::Index<usize> for KindArray<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.0[i]
    }
}

impl<T> std::ops::IndexMut<usize> for KindArray<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.0[i]
    }
}

impl<T: Serialize> Serialize for KindArray<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut t = s.serialize_tuple(self.0.len())?;
        for x in &self.0 {
            t.serialize_element(x)?;
        }
        t.end()
    }
}

impl<'de, T: serde::Deserialize<'de> + Default + Copy> serde::Deserialize<'de> for KindArray<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        padded(d).map(KindArray)
    }
}

impl Ledger {
    pub fn today_of(&self, f: Flow) -> f32 {
        self.today[f.index()]
    }

    pub fn yesterday_of(&self, f: Flow) -> f32 {
        self.yesterday[f.index()]
    }

    /// Close the day's books.
    pub fn roll(&mut self) {
        self.yesterday = self.today;
        self.today = [0.0; FLOWS];
        self.items_yesterday = std::mem::take(&mut self.items_today);
    }

    /// Add a posting to today's itemised lines. A linear search: a day has
    /// at most a few hundred items and most postings land on a recent one.
    fn post_item(&mut self, flow: Flow, item: Item, amount: f32) {
        match self.items_today.iter_mut().rev().find(|e| e.flow == flow && e.item == item) {
            Some(e) => {
                e.amount += amount;
                e.count += 1;
            }
            None => self.items_today.push(ItemEntry { flow, item, amount, count: 1 }),
        }
    }

    /// The itemised lines of one flow from `items` (today's or yesterday's),
    /// biggest first, and what the row holds beyond them (postings that
    /// named no item).
    pub fn items_of<'a>(items: &'a [ItemEntry], row: &[f32; FLOWS], flow: Flow) -> (Vec<&'a ItemEntry>, f32) {
        let mut v: Vec<&ItemEntry> = items.iter().filter(|e| e.flow == flow).collect();
        v.sort_by(|a, b| b.amount.abs().total_cmp(&a.amount.abs()));
        let itemised: f32 = v.iter().map(|e| e.amount).sum();
        (v, row[flow.index()] - itemised)
    }

    /// Yesterday's entries, largest first, for a report.
    pub fn yesterday_sorted(&self) -> Vec<(Flow, f32)> {
        let mut v: Vec<(Flow, f32)> = Flow::ALL.iter().map(|&f| (f, self.yesterday[f.index()])).filter(|(_, x)| *x != 0.0).collect();
        v.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        v
    }
}

/// Deliberately simple demand model. Demand per land use is in `0..1` and
/// drives how quickly vacant lots develop.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Economy {
    pub demand: [f32; 6],
    pub population: u32,
    pub unhoused: u32,
    pub unemployed: u32,
    pub homes_total: u32,
    pub homes_free: u32,
    pub jobs_total: u32,
    pub jobs_free: u32,
    /// People who want to move in but found no home yet.
    pub waiting_immigrants: u32,
    /// Residential buildings per tier.
    pub tier_counts: [u32; 3],
    /// Citizens per income class.
    pub class_counts: [u32; 3],
    /// Average daily rent per residential tier.
    pub avg_rent: [f32; 3],
    /// Income inequality, 0 = equal, 1 = one person has everything.
    pub gini: f32,
    pub avg_land_value: f32,
    /// Citizens forced to move by rent, cumulative.
    pub displaced_total: u32,
    /// Citizens who gave up and left the city, cumulative.
    pub emigrated_total: u32,
    /// Immigrants turned away because nothing was affordable, cumulative.
    pub priced_out_total: u32,
    /// Market statistics: their only home (rail's are `Rail::stats`).
    pub market: crate::market::MarketStats,
    #[serde(default)]
    pub parking_capacity: u32,
    #[serde(default)]
    pub parking_used: u32,
    #[serde(default)]
    pub parking_lots: u32,
    /// Drivers who found no space today and left the car on the street.
    #[serde(default)]
    pub parking_street_today: u32,
    #[serde(default)]
    pub parking_fees_today: f32,
    /// Where the last driver gave up looking for a space.
    #[serde(default)]
    pub parking_shortage_at: Option<glam::Vec2>,
    /// Pedestrian trips started today and sales to passers-by.
    #[serde(default)]
    pub walk_trips_today: u32,
    #[serde(default)]
    pub walk_in_sales_today: f32,
    /// City money: taxes in, plants and imported power out.
    #[serde(default = "default_treasury")]
    pub treasury: f32,
    #[serde(default)]
    pub tax_today: f32,
    #[serde(default)]
    pub power: crate::power::PowerStats,
    /// Bus statistics: their only home. Older saves also carry a copy in
    /// `transit.stats`, which is ignored on load.
    #[serde(default)]
    pub transit: crate::transit::TransitStats,
    /// Building sites, and the homes and jobs they will add when they open.
    #[serde(default)]
    pub sites: u32,
    #[serde(default)]
    pub homes_building: u32,
    #[serde(default)]
    pub jobs_building: u32,
    /// Building materials bought from outside today, at import prices.
    #[serde(default)]
    pub materials_today: f32,
    /// Treasury sources and sinks by name.
    #[serde(default)]
    pub ledger: Ledger,
    /// Households in town, and how many have each size (index = members,
    /// 5 counts five or more; index 0 is unused).
    #[serde(default)]
    pub households: u32,
    #[serde(default)]
    pub household_sizes: [u32; 6],
    /// Households whose rent is over half their income.
    #[serde(default)]
    pub rent_stressed_households: u32,
    /// Households who wanted a home of each tier and found none they could
    /// afford (newcomers turned away, stressed tenants looking for cheaper);
    /// decays daily. Pulls new housing towards that tier.
    #[serde(default)]
    pub housing_demand_by_tier: [f32; 3],
    /// Population at the last midnight, and the day's growth as a share of it.
    #[serde(default)]
    pub population_yesterday: u32,
    #[serde(default)]
    pub growth_per_day: f32,
    /// Adults of working age (children and retirees neither work nor look
    /// for work); `unemployed` counts the ones of them without a job.
    #[serde(default)]
    pub working_age: u32,
    /// Jobs in finished buildings (site crews are temporary and not counted).
    #[serde(default)]
    pub jobs_permanent: u32,
    /// How well each kind of business pays its way (`BusinessKind::index`):
    /// takings over costs (wages and inputs), smoothed over a few days; 1 is
    /// break-even. Only kinds marked in `business_cover_seen` have a reading.
    #[serde(default)]
    pub business_cover: KindArray<f32>,
    #[serde(default)]
    pub business_cover_seen: KindArray<bool>,
    /// Daily and hourly records of the headline numbers, for the charts in
    /// the expanded info windows (`history.rs`). Bounded rings.
    #[serde(default)]
    pub history: crate::history::History,
    /// [tax] The tax schedule per land use and sub-type, the import duty,
    /// and what each line brought in. Old saves load at the standard rates.
    #[serde(default)]
    pub taxes: crate::tax::TaxBook,
}

fn default_treasury() -> f32 {
    60_000.0
}

impl Economy {
    /// Money into the treasury, booked under `flow`.
    pub fn credit(&mut self, flow: Flow, amount: f32) {
        self.treasury += amount;
        self.ledger.today[flow.index()] += amount;
    }

    /// Money out of the treasury, booked under `flow`.
    pub fn debit(&mut self, flow: Flow, amount: f32) {
        self.credit(flow, -amount);
    }

    /// `credit`, and also itemised: what the money was for. Zero amounts
    /// are skipped so an idle building does not crowd the drill-down.
    pub fn credit_item(&mut self, flow: Flow, amount: f32, item: Item) {
        self.credit(flow, amount);
        if amount != 0.0 {
            self.ledger.post_item(flow, item, amount);
        }
    }

    /// `debit`, itemised.
    pub fn debit_item(&mut self, flow: Flow, amount: f32, item: Item) {
        self.credit_item(flow, -amount, item);
    }

    /// How far ahead the food chain orders: one plus the day's growth, so a
    /// town growing by a third a day stocks a third more. Capped at double.
    pub fn food_order_factor(&self) -> f32 {
        (1.0 + self.growth_per_day).clamp(1.0, 2.0)
    }

    /// Close the day's population count.
    pub fn roll_population(&mut self, population: u32) {
        let base = self.population_yesterday.max(50) as f32;
        self.growth_per_day = (population as f32 - self.population_yesterday as f32) / base;
        self.population_yesterday = population;
    }

    pub fn demand_for(&self, lu: LandUse) -> f32 {
        self.demand[lu.index()]
    }

    /// Share of working-age adults without a job.
    pub fn unemployment_share(&self) -> f32 {
        if self.working_age == 0 {
            0.0
        } else {
            self.unemployed as f32 / self.working_age as f32
        }
    }

    /// Workers the town will have once the homes standing empty and the
    /// homes being built fill up, against the jobs it will have once its
    /// sites open, as a share of those workers: positive when jobs will be
    /// short, negative when homes will be. Households bring children and
    /// retirees, so it counts adults per home, not people.
    pub fn job_gap(&self) -> f32 {
        let adults = self.working_age as f32;
        let adults_per_home = if self.households > 0 { (adults / self.households as f32).clamp(0.5, 3.0) } else { 1.3 };
        let workers = adults + (self.homes_free + self.homes_building) as f32 * adults_per_home;
        let jobs = (self.jobs_permanent + self.jobs_building) as f32;
        ((workers - jobs) / workers.max(50.0)).clamp(-1.0, 1.0)
    }

    /// The smoothed cover of a kind of business, once it has one.
    pub fn cover_of(&self, kind: crate::market::BusinessKind) -> Option<f32> {
        let i = kind.index();
        self.business_cover_seen[i].then_some(self.business_cover[i])
    }

    /// Fold one day's takings and costs of a kind into its cover.
    pub fn record_cover(&mut self, kind: crate::market::BusinessKind, takings: f32, costs: f32) {
        if costs <= 0.0 {
            return;
        }
        let i = kind.index();
        let today = (takings / costs).clamp(0.0, 4.0);
        self.business_cover[i] = if self.business_cover_seen[i] { self.business_cover[i] * 0.6 + today * 0.4 } else { today };
        self.business_cover_seen[i] = true;
    }

    pub fn recompute(&mut self) {
        let pop = self.population.max(1) as f32;
        // Homes and jobs already going up count against the pressure, so a
        // boom does not start a site on every vacant lot while the first ones
        // are still scaffolding.
        let housing_pressure = ((self.waiting_immigrants as f32 + self.unhoused as f32) - self.homes_building as f32 * 0.5).max(0.0) / 6.0;
        // Homes and jobs are built in balance: the jobs the town will need
        // once its homes fill (the gap) and the adults out of work today pull
        // commercial and industrial development; joblessness and a coming
        // shortage of jobs hold back new housing, and a coming shortage of
        // workers for the jobs on offer pulls it.
        let unemployment = self.unemployment_share();
        let gap = self.job_gap();
        let job_pressure = (gap.max(0.0) * 1.5 + (unemployment - 0.03).max(0.0) * 2.0).min(1.0);
        let home_pull = (-gap).max(0.0) * 0.8;
        let home_damp = (1.0 - (unemployment - 0.04).max(0.0) * 4.0 - gap.max(0.0) * 1.5).clamp(0.2, 1.0);
        let entertainment_pressure = (pop / 40.0).min(1.0);
        // New venues open only while the ones in town pay their way.
        let venue_room = self.cover_of(crate::market::BusinessKind::Venue).map_or(1.0, |c| ((c - 0.8) / 0.4).clamp(0.1, 1.0));

        let mut d = [0.0f32; 6];
        // Residential demand: a trickle so the city starts, held back while
        // work is short, plus the people waiting for a home (never held back:
        // they are here already).
        d[LandUse::Residential.index()] = ((0.35 + home_pull) * home_damp + housing_pressure).min(1.0);
        d[LandUse::Commercial.index()] = (0.2 + job_pressure * 1.2).min(1.0);
        d[LandUse::Industrial.index()] = (0.15 + job_pressure).min(1.0);
        d[LandUse::Entertainment.index()] = ((0.1 + entertainment_pressure * 0.8) * venue_room).min(1.0);
        d[LandUse::Civic.index()] = (0.1 + (pop / 150.0)).min(1.0);
        d[LandUse::Park.index()] = 0.5;
        // [tax] A land use taxed over the standard is less wanted, one let
        // off is wanted more (1 at the standard rates: no change).
        for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment] {
            let pull = self.taxes.land_use_pull(lu);
            if pull != 1.0 {
                d[lu.index()] = (d[lu.index()] * pull).min(1.0);
            }
        }
        self.demand = d;
    }

    /// Gini coefficient of a list of incomes.
    pub fn gini(incomes: &mut Vec<f32>) -> f32 {
        if incomes.len() < 2 {
            return 0.0;
        }
        incomes.sort_by(|a, b| a.total_cmp(b));
        let n = incomes.len() as f32;
        let total: f32 = incomes.iter().sum();
        if total <= 0.0 {
            return 0.0;
        }
        let weighted: f32 = incomes.iter().enumerate().map(|(i, &x)| (i as f32 + 1.0) * x).sum();
        ((2.0 * weighted) / (n * total) - (n + 1.0) / n).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_drives_the_food_order_factor() {
        let mut e = Economy::default();
        e.roll_population(0);
        assert_eq!(e.food_order_factor(), 1.0);
        e.roll_population(400);
        assert_eq!(e.food_order_factor(), 2.0, "a boom from nothing is capped at double");
        e.roll_population(600);
        assert!((e.food_order_factor() - 1.5).abs() < 1e-4);
        e.roll_population(500);
        assert_eq!(e.food_order_factor(), 1.0, "shrinking never orders less than today");
    }

    #[test]
    fn a_ledger_saved_before_the_service_flows_loads_padded() {
        let old = "(today: (1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0), yesterday: (9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0))";
        let l: Ledger = ron::from_str(old).unwrap();
        assert_eq!(l.today_of(Flow::Construction), 9.0);
        assert_eq!(l.today_of(Flow::ServiceUpkeep), 0.0);
        let back: Ledger = ron::from_str(&ron::to_string(&l).unwrap()).unwrap();
        assert_eq!(back.yesterday, l.yesterday);
    }
}

#[cfg(test)]
mod padded_tests {
    #[derive(serde::Deserialize)]
    struct Saved {
        #[serde(deserialize_with = "super::padded")]
        cover: [f32; 4],
    }

    #[test]
    fn arrays_saved_before_new_kinds_load_padded() {
        let short: Saved = ron::from_str("(cover: (0.5, 0.25))").unwrap();
        assert_eq!(short.cover, [0.5, 0.25, 0.0, 0.0]);
        let long: Saved = ron::from_str("(cover: (1.0, 2.0, 3.0, 4.0, 5.0))").unwrap();
        assert_eq!(long.cover, [1.0, 2.0, 3.0, 4.0]);
    }
}
