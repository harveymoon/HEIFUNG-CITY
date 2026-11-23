//! [r14-builders] Builder companies and the builders' merchant: the
//! numbers, the names and the day's books (`world/builders.rs` is the
//! `World` side).
//!
//! *Who builds a building.* Every building site is put out to tender when
//! its work starts (`World::tender_site`). A building contractor in town
//! (`BusinessKind::Contractor`: a yard, crews of its own staff, vans and
//! lorries) with a crew free and the site within [`LOCAL_REACH`] takes it
//! at its price ([`LOCAL_MARGIN`] over the job's estimate); with none free
//! the job goes to a firm from out of town at a markup
//! ([`OUT_OF_TOWN_MARKUP`]), its crews driving in. A local contractor's
//! crew are its staff, so the site itself hires nobody; an out-of-town
//! firm's site hires its labourers at the gate as every site did before.
//!
//! *Who pays and who buys.* Whoever is building (the plot's owner, or a
//! developer from out of town) pays the contractor in four instalments, one
//! as each quarter's materials are ordered. The contractor buys each
//! delivery's bill of materials (`World::site_load`: a list of good and
//! quantity, the stage's lines of r14-materials' bill): from its own yard stock,
//! a scrap yard, the cheapest of the builders' merchant and the makers in
//! town, or imported; it fetches them in its own lorries or hires a haulier.
//! It pays its crews out of its own money like any business, so a builder
//! with no work goes bust.
//!
//! *The builders' merchant* (`BusinessKind::BuildersMerchant`) keeps a trade
//! stock of every good a site bill names (`catalogue::site_materials`),
//! bought from the makers in town or imported in bulk ([`MERCHANT_BULK_IMPORT`],
//! cheaper than a single site's import), and sells it over the trade counter
//! at [`MERCHANT_TRADE_MARKUP`] to the builders, to sites and to any other
//! business restocking (it is in the seller index for those goods). Its
//! shop floor sells DIY and garden goods to residents (its recipe).
//!
//! Everything is saved with serde defaults.

use serde::{Deserialize, Serialize};

// ── tuning ───────────────────────────────────────────────────────────────────

/// Staff per crew: a contractor runs one site per crew at a time.
pub const CREW_SIZE: u16 = 4;
/// A contractor this far (m) from a site does not bid for it.
pub const LOCAL_REACH: f32 = 4500.0;
/// A local contractor's price over the job's estimate.
pub const LOCAL_MARGIN: f32 = 1.15;
/// A firm from out of town charges this over its labour (its crews drive
/// in and stay over), on top of the materials it buys for the site.
pub const OUT_OF_TOWN_MARKUP: f32 = 1.35;
/// What an estimate expects to pay for materials, against their base price
/// (between the makers' price and an import).
pub const MATERIAL_ESTIMATE: f32 = 1.3;
/// The labour in an estimate: a crew's pay per worker-day, as a share of
/// the site wage. [r15-money] The whole wage now (the contractor pays its
/// staff in full, skill and school on top: about 1.1 of the wage).
pub const LABOUR_SHARE: f32 = 1.1;
/// The merchant's trade price over a good's base price (scaled by how full
/// its racks are, [`MERCHANT_PRICE_SWING`]).
pub const MERCHANT_TRADE_MARKUP: f32 = 1.4;
/// ± share the trade price moves with the merchant's stock.
pub const MERCHANT_PRICE_SWING: f32 = 0.08;
/// What the merchant pays for a bulk import, against the base price (a
/// single site's import pays `market::IMPORT_MARKUP`).
pub const MERCHANT_BULK_IMPORT: f32 = 1.25;
/// Trade stock the merchant keeps of each material, per job.
pub const MERCHANT_STOCK_PER_JOB: f32 = 5.0;
/// A merchant reorders a material once its stock and what is on the way
/// fall under this share of the target.
pub const MERCHANT_REORDER: f32 = 0.6;
/// A contractor keeps this many units of each material in its yard per
/// crew, bought from the merchant each morning.
pub const YARD_UNITS_PER_CREW: f32 = 3.0;
/// One lorry of its own per this many crews (at least one).
pub const CREWS_PER_LORRY: u16 = 2;
/// [pressure] Sites a day one contractor takes off the firms from out of town.
pub const CONTRACTS_PER_CONTRACTOR: f32 = 1.5;
/// [pressure] Units of site materials a day one merchant keeps in stock for.
pub const UNITS_PER_MERCHANT: f32 = 300.0;
/// [pressure] People per merchant's worth of DIY trade.
pub const PEOPLE_PER_MERCHANT: f32 = 25_000.0;
/// Days of books kept.
pub const BOOK_DAYS: usize = 30;

/// Crews a contractor with `staff` workers runs.
pub fn crews(staff: usize) -> u16 {
    let n = (staff as u16) / CREW_SIZE;
    if n == 0 && staff >= 2 {
        1
    } else {
        n
    }
}

/// Lorries of its own a contractor with `crews` crews runs.
pub fn lorries(crews: u16) -> u16 {
    if crews == 0 {
        0
    } else {
        (crews / CREWS_PER_LORRY).max(1)
    }
}

/// The merchant's trade price for a good of base price `base`, with its
/// stock at `ratio` of the target: dearer when the racks are bare.
pub fn trade_price(base: f32, ratio: f32) -> f32 {
    let k = 1.0 + MERCHANT_PRICE_SWING * (1.0 - 2.0 * ratio.clamp(0.0, 1.0));
    base * MERCHANT_TRADE_MARKUP * k
}

// ── names ────────────────────────────────────────────────────────────────────

const OUT_PREFIX: &[&str] = &["Tri-County", "Interstate", "Valley", "Coastal", "Ridgeline", "Northgate", "Overland", "Lakeshore", "Redline", "Summit"];
const OUT_SUFFIX: &[&str] = &["Construction", "Contractors", "Building Group", "Civil & Building", "Builders"];

/// A firm from out of town's name, by hash: "Tri-County Contractors".
pub fn out_of_town_name(h: u64) -> String {
    let p = OUT_PREFIX[(crate::hash::mix(h, 0xB1D) % OUT_PREFIX.len() as u64) as usize];
    let s = OUT_SUFFIX[(crate::hash::mix(h, 0xB1E) % OUT_SUFFIX.len() as u64) as usize];
    format!("{p} {s}")
}

// ── the day's books ──────────────────────────────────────────────────────────

/// One day of the builders' and merchants' trade, for the probes, the API
/// and the economy pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildersDay {
    pub day: u32,
    /// Sites let to contractors in town / to firms from out of town.
    pub let_local: u32,
    pub let_outside: u32,
    /// Sites finished, of them built by a contractor in town, and the sum
    /// of their build times in days (from the tender).
    pub finished: u32,
    pub finished_local: u32,
    pub build_days: f32,
    /// Loads for sites by where they came from.
    pub yard_loads: u32,
    pub merchant_loads: u32,
    pub maker_loads: u32,
    pub scrap_loads: u32,
    pub import_loads: u32,
    /// Of the loads from a maker, fetched in the builder's own lorry.
    pub collected: u32,
    /// Units of materials bought for sites, and of them over a merchant's
    /// counter (the sites' own orders and the builders' yards).
    pub material_units: f32,
    pub merchant_units: f32,
    /// Money: instalments paid to contractors in town, fees paid to firms
    /// from out of town, the merchants' trade sales (to builders, sites and
    /// industry restocking) and their takings in all (closed at midnight).
    pub contracts_paid: f32,
    pub outside_paid: f32,
    pub trade_sales: f32,
    pub merchant_takings: f32,
    /// Contractors that went bust.
    pub busts: u32,
    /// At midnight: contractors open, their money and crews; merchants open.
    pub contractors: u32,
    pub contractor_money: f32,
    pub crews: u32,
    pub merchants: u32,
}

impl BuildersDay {
    /// Mean days from tender to opening of the sites finished.
    pub fn mean_build_days(&self) -> f32 {
        if self.finished == 0 {
            0.0
        } else {
            self.build_days / self.finished as f32
        }
    }
}

/// The builders' books: today's, the last [`BOOK_DAYS`] and the totals.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildersBook {
    pub today: BuildersDay,
    pub days: Vec<BuildersDay>,
    pub let_local_total: u32,
    pub let_outside_total: u32,
    pub busts_total: u32,
}

impl BuildersBook {
    /// Close today into the history.
    pub fn roll(&mut self, day: u32) {
        let mut t = std::mem::take(&mut self.today);
        t.day = day;
        self.let_local_total += t.let_local;
        self.let_outside_total += t.let_outside;
        self.busts_total += t.busts;
        if self.days.len() >= BOOK_DAYS {
            self.days.remove(0);
        }
        self.days.push(t);
    }

    /// The last closed day.
    pub fn yesterday(&self) -> Option<&BuildersDay> {
        self.days.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crews_and_lorries_follow_staff() {
        assert_eq!(crews(0), 0);
        assert_eq!(crews(1), 0);
        assert_eq!(crews(2), 1);
        assert_eq!(crews(8), 2);
        assert_eq!(crews(16), 4);
        assert_eq!(lorries(0), 0);
        assert_eq!(lorries(1), 1);
        assert_eq!(lorries(4), 2);
    }

    #[test]
    fn trade_price_is_dearer_on_bare_racks_and_under_a_site_import() {
        let full = trade_price(10.0, 1.0);
        let bare = trade_price(10.0, 0.0);
        assert!(bare > full);
        assert!(bare < 10.0 * crate::market::IMPORT_MARKUP, "the merchant beats a site's own import");
        assert!(full > 10.0 * MERCHANT_BULK_IMPORT, "and still makes something on its bulk imports");
    }

    #[test]
    fn out_of_town_names_are_stable() {
        assert_eq!(out_of_town_name(7), out_of_town_name(7));
        assert!(out_of_town_name(7).contains(' '));
    }

    #[test]
    fn books_roll() {
        let mut b = BuildersBook::default();
        b.today.let_local = 2;
        b.today.finished = 2;
        b.today.build_days = 5.0;
        assert_eq!(b.today.mean_build_days(), 2.5);
        b.roll(3);
        assert_eq!(b.yesterday().map(|d| d.day), Some(3));
        assert_eq!(b.let_local_total, 2);
        assert_eq!(b.today, BuildersDay::default());
    }
}
