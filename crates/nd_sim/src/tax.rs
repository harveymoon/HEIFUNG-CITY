//! Taxes: a rate per land use and sub-type, the import duty, and what each
//! rate does to the city.
//!
//! The city taxes incomes by where people live (residential, per tier) and
//! business takings by what the business is (each shop kind, nightlife per
//! tier, each industry sub-zone). The defaults are the old flat rates (12% of
//! incomes, 5% of takings, no duty), and at the defaults nothing changes:
//! wages and prices in the sim were tuned as what people and firms keep
//! after the standard rate, so the treasury's standard take is collected at
//! source. Only the difference from the standard rate reaches a purse: a
//! household in a tier taxed at 20% pays the extra 8% of its income out of
//! its money every day, and a farm taxed at 0% keeps the 5% it used to pay.
//!
//! What a rate does (`World` applies it, see `world/develop.rs`,
//! `world/market.rs`, `world/trips.rs`):
//! - residential: the household's daily money, mood (heavy tax is felt),
//!   how many newcomers arrive and how readily homes of that tier are built;
//! - businesses: their money and their cover (takings over costs), which
//!   decides whether more of the kind are built; the land use's demand; the
//!   draw of that kind when a new lot picks what to become ([`TaxBook::pull`]);
//! - the import duty: imports cost the buyer that much more (the duty goes to
//!   the treasury), so kinds living on imports earn less and industry that
//!   makes what the town imports is drawn harder.

use crate::buildings::Tier;
use crate::industry::IndustryKind;
use crate::market::{BusinessKind, Resource};
use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

/// The standard income tax: the old flat rate, and the default.
pub const INCOME_TAX: f32 = 0.12;
/// The standard business tax on takings.
pub const BUSINESS_TAX: f32 = 0.05;
/// Highest rates the player may set.
pub const MAX_INCOME_TAX: f32 = 0.40;
pub const MAX_BUSINESS_TAX: f32 = 0.30;
pub const MAX_IMPORT_DUTY: f32 = 1.0;
/// How strongly a rate away from the standard pushes a category, per unit
/// of rate: residents feel income tax less sharply than firms feel a tax on
/// takings (moving house is harder than not opening a shop).
const RESIDENTIAL_SENSITIVITY: f32 = 4.0;
const BUSINESS_SENSITIVITY: f32 = 8.0;
/// Bounds on [`TaxBook::pull`], so no rate stops a category dead or makes
/// it boom without limit.
pub const PULL_MIN: f32 = 0.25;
pub const PULL_MAX: f32 = 1.6;
/// How much a full import duty adds to the draw of industry that makes what
/// the town imports (duty 1.0: 1 + this).
pub const DUTY_LOCAL_PULL: f32 = 1.5;

/// One line of the tax schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaxCategory {
    ResidentialLow,
    ResidentialMid,
    ResidentialHigh,
    Grocery,
    Bakery,
    GadgetShop,
    Parking,
    NightlifeLow,
    NightlifeMid,
    NightlifeHigh,
    Farming,
    Oil,
    Refinery,
    Manufacturing,
    Logistics,
    Fishing,
    Tech,
}

impl TaxCategory {
    pub const COUNT: usize = 17;
    pub const ALL: [TaxCategory; Self::COUNT] = [
        TaxCategory::ResidentialLow,
        TaxCategory::ResidentialMid,
        TaxCategory::ResidentialHigh,
        TaxCategory::Grocery,
        TaxCategory::Bakery,
        TaxCategory::GadgetShop,
        TaxCategory::Parking,
        TaxCategory::NightlifeLow,
        TaxCategory::NightlifeMid,
        TaxCategory::NightlifeHigh,
        TaxCategory::Farming,
        TaxCategory::Oil,
        TaxCategory::Refinery,
        TaxCategory::Manufacturing,
        TaxCategory::Logistics,
        TaxCategory::Fishing,
        TaxCategory::Tech,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The land use whose group it is shown in. Private car parks stand on
    /// any land; they are listed with the shops.
    pub fn land_use(self) -> LandUse {
        use TaxCategory::*;
        match self {
            ResidentialLow | ResidentialMid | ResidentialHigh => LandUse::Residential,
            Grocery | Bakery | GadgetShop | Parking => LandUse::Commercial,
            NightlifeLow | NightlifeMid | NightlifeHigh => LandUse::Entertainment,
            _ => LandUse::Industrial,
        }
    }

    pub fn is_residential(self) -> bool {
        self.land_use() == LandUse::Residential
    }

    /// Short key for the API.
    pub fn key(self) -> &'static str {
        use TaxCategory::*;
        match self {
            ResidentialLow => "residential_low",
            ResidentialMid => "residential_mid",
            ResidentialHigh => "residential_high",
            Grocery => "grocery",
            Bakery => "bakery",
            GadgetShop => "gadget_shop",
            Parking => "parking",
            NightlifeLow => "nightlife_low",
            NightlifeMid => "nightlife_mid",
            NightlifeHigh => "nightlife_high",
            Farming => "farming",
            Oil => "oil",
            Refinery => "refinery",
            Manufacturing => "manufacturing",
            Logistics => "logistics",
            Fishing => "fishing",
            Tech => "tech",
        }
    }

    pub fn label(self) -> &'static str {
        use TaxCategory::*;
        match self {
            ResidentialLow => "low-tier homes",
            ResidentialMid => "middle-tier homes",
            ResidentialHigh => "high-tier homes",
            Grocery => "grocers",
            Bakery => "bakeries",
            GadgetShop => "gadget shops",
            Parking => "car parks",
            NightlifeLow => "dive bars & clubs",
            NightlifeMid => "clubs & cinemas",
            NightlifeHigh => "casinos & arenas",
            Farming => "farming",
            Oil => "oil extraction",
            Refinery => "refineries",
            Manufacturing => "manufacturing",
            Logistics => "logistics",
            Fishing => "fishing",
            Tech => "clean tech",
        }
    }

    /// The key, a few spellings of it, or a land use name for its first
    /// line ("industrial" is no single category: see [`TaxCategory::group`]).
    pub fn from_key(s: &str) -> Option<TaxCategory> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        if let Some(c) = Self::ALL.into_iter().find(|c| c.key() == s) {
            return Some(c);
        }
        if let Some(k) = IndustryKind::from_key(&s) {
            return Some(Self::of_industry(k));
        }
        Some(match s.as_str() {
            "residential_1" | "homes_low" | "res_low" => TaxCategory::ResidentialLow,
            "residential_2" | "homes_mid" | "res_mid" | "residential_middle" => TaxCategory::ResidentialMid,
            "residential_3" | "homes_high" | "res_high" => TaxCategory::ResidentialHigh,
            "groceries" | "grocer" => TaxCategory::Grocery,
            "bakeries" => TaxCategory::Bakery,
            "gadget" | "gadgets" | "gadget_shops" => TaxCategory::GadgetShop,
            "car_parks" | "parking_lots" => TaxCategory::Parking,
            "entertainment_low" | "venue_low" => TaxCategory::NightlifeLow,
            "entertainment_mid" | "venue_mid" => TaxCategory::NightlifeMid,
            "entertainment_high" | "venue_high" => TaxCategory::NightlifeHigh,
            _ => return None,
        })
    }

    /// Every category of a land use (a land use name), for setting a whole
    /// group at once.
    pub fn group(land_use: LandUse) -> Vec<TaxCategory> {
        Self::ALL.into_iter().filter(|c| c.land_use() == land_use).collect()
    }

    pub fn of_industry(k: IndustryKind) -> TaxCategory {
        match k {
            IndustryKind::Farming => TaxCategory::Farming,
            IndustryKind::OilExtraction => TaxCategory::Oil,
            IndustryKind::Refinery => TaxCategory::Refinery,
            IndustryKind::Manufacturing => TaxCategory::Manufacturing,
            IndustryKind::Logistics => TaxCategory::Logistics,
            IndustryKind::Fishing => TaxCategory::Fishing,
            IndustryKind::Tech => TaxCategory::Tech,
            // [economy] Strip mines are taxed with the other extraction.
            IndustryKind::Mining => TaxCategory::Oil,
        }
    }

    pub fn residential(tier: Tier) -> TaxCategory {
        match tier {
            Tier::Low => TaxCategory::ResidentialLow,
            Tier::Mid => TaxCategory::ResidentialMid,
            Tier::High => TaxCategory::ResidentialHigh,
        }
    }

    /// The line a business is taxed under; `None` for offices, which the
    /// city funds and taxes at the standard rate as before.
    pub fn of_business(kind: BusinessKind, tier: Tier) -> Option<TaxCategory> {
        if let Some(k) = kind.industry() {
            return Some(Self::of_industry(k));
        }
        Some(match kind {
            BusinessKind::Grocery => TaxCategory::Grocery,
            BusinessKind::Bakery => TaxCategory::Bakery,
            BusinessKind::GadgetShop => TaxCategory::GadgetShop,
            BusinessKind::Parking => TaxCategory::Parking,
            BusinessKind::Venue | BusinessKind::SeafoodRestaurant | BusinessKind::GolfRange | BusinessKind::Gallery | BusinessKind::Marina => match tier {
                Tier::Low => TaxCategory::NightlifeLow,
                Tier::Mid => TaxCategory::NightlifeMid,
                Tier::High => TaxCategory::NightlifeHigh,
            },
            // [economy] Shops by what they sell.
            BusinessKind::FishMarket => TaxCategory::Grocery,
            BusinessKind::CarDealer | BusinessKind::BikeShop | BusinessKind::Mechanic | BusinessKind::ArtStudio => TaxCategory::GadgetShop,
            _ => return None,
        })
    }

    /// The rate it starts at: today's flat rates.
    pub fn default_rate(self) -> f32 {
        if self.is_residential() {
            INCOME_TAX
        } else {
            BUSINESS_TAX
        }
    }

    pub fn max_rate(self) -> f32 {
        if self.is_residential() {
            MAX_INCOME_TAX
        } else {
            MAX_BUSINESS_TAX
        }
    }

    fn sensitivity(self) -> f32 {
        if self.is_residential() {
            RESIDENTIAL_SENSITIVITY
        } else {
            BUSINESS_SENSITIVITY
        }
    }
}

/// The tax schedule and what it brought in. Lives in `Economy::taxes`.
/// Rows are plain vectors indexed by [`TaxCategory::index`], padded with
/// the defaults when a save predates a category, so old saves load.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TaxBook {
    /// Rate per category (0..max); missing entries are the default.
    pub rates: Vec<f32>,
    /// Duty on goods imported from outside, as a share of their price.
    pub import_duty: f32,
    /// Tax collected per category today so far, and the whole of yesterday.
    pub today: Vec<f32>,
    pub yesterday: Vec<f32>,
    /// A running average of the daily take (about a week's memory).
    pub recent: Vec<f32>,
    /// Import duty collected today and yesterday.
    pub duty_today: f32,
    pub duty_yesterday: f32,
    /// Goods imported lately, by resource (`Resource::ALL` order), fading
    /// daily: what local industry could make instead.
    pub imported: Vec<f32>,
}

impl TaxBook {
    pub fn rate(&self, c: TaxCategory) -> f32 {
        self.rates.get(c.index()).copied().unwrap_or_else(|| c.default_rate())
    }

    /// Set a rate, clamped to `0..max`. Returns what was set.
    pub fn set_rate(&mut self, c: TaxCategory, rate: f32) -> f32 {
        let r = if rate.is_finite() { rate.clamp(0.0, c.max_rate()) } else { c.default_rate() };
        if self.rates.len() < TaxCategory::COUNT {
            let have = self.rates.len();
            self.rates.extend(TaxCategory::ALL[have..].iter().map(|c| c.default_rate()));
        }
        self.rates[c.index()] = r;
        r
    }

    pub fn set_import_duty(&mut self, duty: f32) -> f32 {
        self.import_duty = if duty.is_finite() { duty.clamp(0.0, MAX_IMPORT_DUTY) } else { 0.0 };
        self.import_duty
    }

    /// Every rate back to the standard, no duty.
    pub fn reset(&mut self) {
        self.rates.clear();
        self.import_duty = 0.0;
    }

    /// The rate over the standard (negative under it): what reaches purses.
    pub fn surcharge(&self, c: TaxCategory) -> f32 {
        self.rate(c) - c.default_rate()
    }

    /// Is every rate the standard one? Then the sim runs exactly as it did
    /// before taxes could be set (the draws for the effects are skipped).
    pub fn is_standard(&self) -> bool {
        TaxCategory::ALL.iter().all(|&c| self.rate(c) == c.default_rate())
    }

    /// How a category's rate pulls on it: 1 at the standard rate, below 1
    /// when it is taxed harder (fewer of it are built, fewer arrive), above
    /// when it is let off. `exp(-(rate - standard) * sensitivity)`, bounded.
    pub fn pull(&self, c: TaxCategory) -> f32 {
        let d = self.surcharge(c);
        if d == 0.0 {
            return 1.0;
        }
        (-d * c.sensitivity()).exp().clamp(PULL_MIN, PULL_MAX)
    }

    /// The mean pull of a land use's categories (1 for land uses without any).
    pub fn land_use_pull(&self, lu: LandUse) -> f32 {
        let cats = TaxCategory::group(lu);
        if cats.is_empty() {
            return 1.0;
        }
        cats.iter().map(|&c| self.pull(c)).sum::<f32>() / cats.len() as f32
    }

    /// Book tax collected under a category.
    pub fn collect(&mut self, c: TaxCategory, amount: f32) {
        if self.today.len() < TaxCategory::COUNT {
            self.today.resize(TaxCategory::COUNT, 0.0);
        }
        self.today[c.index()] += amount;
    }

    pub fn today_of(&self, c: TaxCategory) -> f32 {
        self.today.get(c.index()).copied().unwrap_or(0.0)
    }

    pub fn yesterday_of(&self, c: TaxCategory) -> f32 {
        self.yesterday.get(c.index()).copied().unwrap_or(0.0)
    }

    pub fn recent_of(&self, c: TaxCategory) -> f32 {
        self.recent.get(c.index()).copied().unwrap_or(0.0)
    }

    /// Note goods bought in from outside.
    pub fn note_import(&mut self, r: Resource, amount: f32) {
        if self.imported.len() < Resource::ALL.len() {
            self.imported.resize(Resource::ALL.len(), 0.0);
        }
        if let Some(i) = Resource::ALL.iter().position(|x| *x == r) {
            self.imported[i] += amount;
        }
    }

    /// Goods of a resource imported lately.
    pub fn imported_of(&self, r: Resource) -> f32 {
        Resource::ALL.iter().position(|x| *x == r).and_then(|i| self.imported.get(i)).copied().unwrap_or(0.0)
    }

    /// The extra draw of an industry sub-zone under the import duty: kinds
    /// that make something the town has been importing are wanted more,
    /// the more so the higher the duty.
    pub fn import_substitution(&self, k: IndustryKind) -> f32 {
        if self.import_duty <= 0.0 {
            return 1.0;
        }
        let makes_imports = k.businesses().iter().any(|b| b.recipe().outputs.iter().any(|(r, _)| self.imported_of(*r) > 1.0));
        if makes_imports {
            1.0 + self.import_duty * DUTY_LOCAL_PULL
        } else {
            1.0
        }
    }

    /// Close the day's books: today's take becomes yesterday's and is
    /// folded into the running average; the memory of imports fades.
    pub fn roll(&mut self) {
        self.today.resize(TaxCategory::COUNT, 0.0);
        self.recent.resize(TaxCategory::COUNT, 0.0);
        for i in 0..TaxCategory::COUNT {
            self.recent[i] += (self.today[i] - self.recent[i]) * 0.25;
        }
        self.yesterday = std::mem::replace(&mut self.today, vec![0.0; TaxCategory::COUNT]);
        self.duty_yesterday = std::mem::take(&mut self.duty_today);
        for x in &mut self.imported {
            *x *= 0.5;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_old_flat_rates_and_pull_nothing() {
        let t = TaxBook::default();
        assert!(t.is_standard());
        assert_eq!(t.rate(TaxCategory::ResidentialMid), 0.12);
        assert_eq!(t.rate(TaxCategory::Oil), 0.05);
        for c in TaxCategory::ALL {
            assert_eq!(t.pull(c), 1.0);
            assert_eq!(t.surcharge(c), 0.0);
        }
        assert_eq!(t.import_substitution(IndustryKind::Farming), 1.0);
    }

    #[test]
    fn a_higher_rate_pulls_less_and_a_lower_one_more() {
        let mut t = TaxBook::default();
        t.set_rate(TaxCategory::Oil, 0.20);
        t.set_rate(TaxCategory::Farming, 0.0);
        assert!(t.pull(TaxCategory::Oil) < 0.5, "oil taxed at 20% is shunned: {}", t.pull(TaxCategory::Oil));
        assert!(t.pull(TaxCategory::Farming) > 1.2, "untaxed farming is drawn: {}", t.pull(TaxCategory::Farming));
        assert_eq!(t.pull(TaxCategory::Tech), 1.0, "others are untouched");
        assert!(!t.is_standard());
        assert_eq!(t.set_rate(TaxCategory::ResidentialLow, 5.0), MAX_INCOME_TAX, "clamped");
        t.reset();
        assert!(t.is_standard());
    }

    #[test]
    fn categories_round_trip_their_keys_and_cover_the_businesses() {
        for c in TaxCategory::ALL {
            assert_eq!(TaxCategory::from_key(c.key()), Some(c));
        }
        assert_eq!(TaxCategory::from_key("oil_extraction"), Some(TaxCategory::Oil));
        assert_eq!(TaxCategory::of_business(BusinessKind::GrainFarm, Tier::Low), Some(TaxCategory::Farming));
        assert_eq!(TaxCategory::of_business(BusinessKind::TowYard, Tier::Low), Some(TaxCategory::Logistics));
        assert_eq!(TaxCategory::of_business(BusinessKind::Venue, Tier::High), Some(TaxCategory::NightlifeHigh));
        assert_eq!(TaxCategory::of_business(BusinessKind::Office, Tier::Mid), None);
        assert_eq!(TaxCategory::group(LandUse::Industrial).len(), 7);
    }

    #[test]
    fn the_books_roll_and_an_old_save_loads() {
        let mut t = TaxBook::default();
        t.collect(TaxCategory::Bakery, 10.0);
        t.duty_today = 3.0;
        t.roll();
        assert_eq!(t.yesterday_of(TaxCategory::Bakery), 10.0);
        assert_eq!(t.today_of(TaxCategory::Bakery), 0.0);
        assert!((t.recent_of(TaxCategory::Bakery) - 2.5).abs() < 1e-5);
        assert_eq!(t.duty_yesterday, 3.0);
        // A schedule saved with fewer categories reads the rest as standard.
        let old: TaxBook = ron::from_str("(rates: [0.2, 0.1])").unwrap();
        assert_eq!(old.rate(TaxCategory::ResidentialLow), 0.2);
        assert_eq!(old.rate(TaxCategory::Tech), BUSINESS_TAX);
        let empty: TaxBook = ron::from_str("()").unwrap();
        assert!(empty.is_standard());
    }

    #[test]
    fn duty_draws_industry_that_replaces_imports() {
        let mut t = TaxBook::default();
        t.note_import(Resource::Crude, 50.0);
        assert_eq!(t.import_substitution(IndustryKind::OilExtraction), 1.0, "no duty, no pull");
        t.set_import_duty(0.5);
        assert!(t.import_substitution(IndustryKind::OilExtraction) > 1.5);
        assert_eq!(t.import_substitution(IndustryKind::Tech), 1.0);
    }
}
