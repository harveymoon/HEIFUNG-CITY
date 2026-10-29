//! [r4-leisure] Commercial sub-zones: what kind of commercial land a lot
//! is. The player zones Commercial and picks a sub-zone in the Zone tool,
//! as for Industrial: **auto** (the default: the city picks from the
//! street, the homes near, land value, demand and the district story),
//! **neighbourhood**, **retail**, **offices** or **leisure**.
//!
//! Leisure is the old Entertainment zone: its land keeps the
//! `LandUse::Entertainment` value inside the simulation (clubs, nights out,
//! nightlife taxes, trash, land value and the building styles all key on
//! it, and saves store it), but the player sees and zones it as commercial
//! land with the leisure sub-zone ([`LandUse::zoned_as`]). Neighbourhood,
//! retail and offices are commercial land with a sub-zone set over the
//! ground ([`CommercialBias`], `World::commercial_bias`), as industry's.

use crate::market::BusinessKind;
use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

/// A commercial sub-zone. "Auto" is no sub-zone set (`None` where one is
/// held as an `Option`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommercialSub {
    /// Corner groceries, bakeries, cafés and small shops near homes.
    Neighbourhood,
    /// Bigger shops on the main roads.
    Retail,
    /// Office buildings and towers.
    Offices,
    /// Bars, clubs, venues and other nights out (`LandUse::Entertainment`).
    Leisure,
}

/// The sub-types the Zone tool offers for commercial land: (key, label).
/// "auto" lets the city decide lot by lot.
pub const COMMERCIAL_CHOICES: [(&str, &str); 5] = [("auto", "Auto (city decides)"), ("neighbourhood", "Neighbourhood"), ("retail", "Retail"), ("offices", "Offices"), ("leisure", "Leisure")];

impl CommercialSub {
    pub const ALL: [CommercialSub; 4] = [CommercialSub::Neighbourhood, CommercialSub::Retail, CommercialSub::Offices, CommercialSub::Leisure];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            CommercialSub::Neighbourhood => "neighbourhood",
            CommercialSub::Retail => "retail",
            CommercialSub::Offices => "offices",
            CommercialSub::Leisure => "leisure",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CommercialSub::Neighbourhood => "Neighbourhood",
            CommercialSub::Retail => "Retail",
            CommercialSub::Offices => "Offices",
            CommercialSub::Leisure => "Leisure",
        }
    }

    /// A key ("offices"), with the old zone's names for leisure
    /// ("entertainment", "nightlife") and US spellings. `None` for "auto"
    /// and anything unknown.
    pub fn from_key(k: &str) -> Option<CommercialSub> {
        match k.trim().to_ascii_lowercase().as_str() {
            "neighbourhood" | "neighborhood" | "local" => Some(CommercialSub::Neighbourhood),
            "retail" | "shops" => Some(CommercialSub::Retail),
            "offices" | "office" => Some(CommercialSub::Offices),
            "leisure" | "entertainment" | "nightlife" => Some(CommercialSub::Leisure),
            _ => None,
        }
    }

    /// Is `k` one of the choices (a sub-zone or "auto")?
    pub fn is_choice(k: &str) -> bool {
        k.trim().eq_ignore_ascii_case("auto") || Self::from_key(k).is_some()
    }

    /// The land use a lot of this sub-zone has in the simulation.
    pub fn land_use(self) -> LandUse {
        if self == CommercialSub::Leisure {
            LandUse::Entertainment
        } else {
            LandUse::Commercial
        }
    }

    /// The business kinds of this sub-zone today, for the panels (shops
    /// that only open as tenants or by a pressure are listed too).
    pub fn kinds(self) -> &'static [BusinessKind] {
        use BusinessKind as K;
        match self {
            CommercialSub::Neighbourhood => &[K::Grocery, K::Bakery, K::GadgetShop, K::Cafe, K::Bookshop],
            CommercialSub::Retail => &[K::GadgetShop, K::Grocery, K::CarDealer, K::BikeShop],
            CommercialSub::Offices => &[K::Office],
            CommercialSub::Leisure => &[K::Venue, K::GolfRange, K::BeachBar, K::Cafe],
        }
    }

    /// What a new building of this sub-zone may open as by the market:
    /// the land use's menu (`BusinessKind::candidates`) narrowed to the
    /// sub-zone, repeats kept (groceries count twice).
    pub fn menu(self) -> &'static [BusinessKind] {
        use BusinessKind as K;
        match self {
            CommercialSub::Neighbourhood => &[K::Grocery, K::Grocery, K::Bakery, K::GadgetShop],
            CommercialSub::Retail => &[K::GadgetShop, K::Grocery],
            CommercialSub::Offices => &[K::Office],
            CommercialSub::Leisure => BusinessKind::candidates(LandUse::Entertainment),
        }
    }

    /// One line on what the sub-zone grows, for the Zone tool.
    pub fn note(self) -> &'static str {
        match self {
            CommercialSub::Neighbourhood => "Corner groceries, bakeries, small shops; low, near homes.",
            CommercialSub::Retail => "Bigger shops on the main roads.",
            CommercialSub::Offices => "Office buildings and towers.",
            CommercialSub::Leisure => "Bars, clubs and venues: nightlife and jobs, some noise at night (the old Entertainment zone).",
        }
    }
}

/// The note for a choice key, "auto" included.
pub fn choice_note(key: &str) -> &'static str {
    CommercialSub::from_key(key).map_or("The city picks for each lot: quiet streets by homes grow neighbourhood shops, busy avenues retail, dear downtown offices, nightlife districts leisure.", CommercialSub::note)
}

/// The player's commercial sub-zone over an area: newer entries win.
/// `sub: None` hands the area back to the city ("auto") over an older one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommercialBias {
    pub polygon: Vec<glam::Vec2>,
    pub sub: Option<CommercialSub>,
}

/// The most area biases kept; the oldest go first.
pub const MAX_COMMERCIAL_BIASES: usize = 256;

/// What the city weighs when it picks a sub-zone for a lot on auto.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AutoSite {
    /// Car lanes of the street the lot fronts (both ways).
    pub lanes: u8,
    /// Homes within a short walk.
    pub homes_near: f32,
    /// Land value as commercial land feels it, `0..1`.
    pub land_value: f32,
    /// Demand for shops and offices, `0..1` (`Economy::demand_for(Commercial)`).
    pub shop_demand: f32,
    /// Demand for nights out, `0..1` (`Economy::demand_for(Entertainment)`).
    pub leisure_demand: f32,
    /// The district's story keeps late hours or is a nightlife story
    /// (Afterglow Mile, Lantern Lanes, ...).
    pub nightlife: bool,
    /// People in town: a night out on demand alone waits for a town big
    /// enough to fill one (`LEISURE_ON_DEMAND_POP`).
    pub population: u32,
    /// Share of working-age people out of work: offices take on the
    /// jobless, so a town short of work leans to them.
    pub unemployment: f32,
    /// A hash of the lot, to settle near ties without a random draw.
    pub seed: u64,
}

/// People a town needs before lots on auto turn to leisure on demand
/// alone (a nightlife story leans to it at any size).
pub const LEISURE_ON_DEMAND_POP: u32 = 400;

/// Homes within this many metres count as "near" for neighbourhood shops.
pub const HOMES_REACH: f32 = 160.0;

impl AutoSite {
    /// How well each sub-zone suits the site, in `CommercialSub::ALL`
    /// order: quiet streets by homes → neighbourhood, busy avenues →
    /// retail, dear land → offices, nightlife stories and a town hungry
    /// for nights out → leisure.
    pub fn scores(&self) -> [f32; 4] {
        let busy = match self.lanes {
            0..=2 => 0.0,
            3 => 0.5,
            _ => 1.0,
        };
        let homes = (self.homes_near / 120.0).clamp(0.0, 1.0);
        let dear = ((self.land_value - 0.55) / 0.3).clamp(0.0, 1.0);
        let jitter = |i: u64| crate::hash::unit(crate::hash::mix(self.seed, 0xC0_5B + i)) * 0.06;
        let neighbourhood = 0.35 + 0.55 * homes - 0.45 * busy - 0.3 * dear;
        let retail = 0.15 + 0.7 * busy + 0.1 * (1.0 - homes);
        let jobless = ((self.unemployment - 0.04) * 5.0).clamp(0.0, 1.0);
        let offices = 0.05 + 0.95 * dear + 0.15 * busy * dear + 0.9 * jobless;
        let hungry = if self.population >= LEISURE_ON_DEMAND_POP { (self.leisure_demand - self.shop_demand - 0.3).max(0.0) } else { 0.0 };
        let leisure = if self.nightlife { 0.75 } else { 0.0 } + 1.2 * hungry + 0.1 * busy;
        [neighbourhood + jitter(0), retail + jitter(1), offices + jitter(2), leisure + jitter(3)]
    }

    /// The sub-zone the city picks, among those `allowed` (the ones whose
    /// businesses the market would open; `None` when none is).
    pub fn pick(&self, allowed: [bool; 4]) -> Option<CommercialSub> {
        let s = self.scores();
        CommercialSub::ALL.into_iter().filter(|c| allowed[c.index()]).max_by(|a, b| s[a.index()].total_cmp(&s[b.index()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> AutoSite {
        AutoSite { lanes: 2, homes_near: 0.0, land_value: 0.4, shop_demand: 0.5, leisure_demand: 0.5, nightlife: false, population: 5000, unemployment: 0.03, seed: 7 }
    }

    const ALL: [bool; 4] = [true; 4];

    #[test]
    fn auto_picks_by_street_homes_value_and_story() {
        // A quiet street lined with homes: the corner shop.
        let quiet = AutoSite { homes_near: 150.0, ..site() };
        assert_eq!(quiet.pick(ALL), Some(CommercialSub::Neighbourhood));
        // A busy avenue: the bigger shops.
        let avenue = AutoSite { lanes: 4, homes_near: 20.0, ..site() };
        assert_eq!(avenue.pick(ALL), Some(CommercialSub::Retail));
        // Dear downtown land: offices.
        let downtown = AutoSite { lanes: 4, land_value: 0.95, ..site() };
        assert_eq!(downtown.pick(ALL), Some(CommercialSub::Offices));
        // A town short of work: offices, which take on the jobless.
        let jobless = AutoSite { homes_near: 150.0, unemployment: 0.25, ..site() };
        assert_eq!(jobless.pick(ALL), Some(CommercialSub::Offices));
        // A nightlife story leans leisure on an ordinary street.
        let strip = AutoSite { nightlife: true, ..site() };
        assert_eq!(strip.pick(ALL), Some(CommercialSub::Leisure));
        // A town hungry for nights out too, without a story.
        let hungry = AutoSite { leisure_demand: 1.0, shop_demand: 0.1, ..site() };
        assert_eq!(hungry.pick(ALL), Some(CommercialSub::Leisure));
        // ... but not in a hamlet.
        assert_ne!(AutoSite { population: 60, ..hungry }.pick(ALL), Some(CommercialSub::Leisure));
        // Leisure stays out while venues would not pay (not allowed), and
        // nothing is picked when the market wants nothing.
        assert_ne!(strip.pick([true, true, true, false]), Some(CommercialSub::Leisure));
        assert_eq!(strip.pick([false; 4]), None);
    }

    #[test]
    fn auto_pick_is_deterministic_and_seed_only_breaks_ties() {
        let a = AutoSite { homes_near: 150.0, ..site() };
        for seed in 0..50 {
            let s = AutoSite { seed, ..a };
            assert_eq!(s.pick(ALL), s.pick(ALL));
            assert_eq!(s.pick(ALL), Some(CommercialSub::Neighbourhood), "seed {seed}");
        }
    }

    #[test]
    fn keys_round_trip_and_entertainment_is_leisure() {
        for c in CommercialSub::ALL {
            assert_eq!(CommercialSub::from_key(c.key()), Some(c));
            assert!(COMMERCIAL_CHOICES.iter().any(|(k, _)| *k == c.key()));
        }
        assert_eq!(COMMERCIAL_CHOICES[0].0, "auto");
        assert_eq!(CommercialSub::from_key("auto"), None);
        assert!(CommercialSub::is_choice("auto"));
        assert_eq!(CommercialSub::from_key("Entertainment"), Some(CommercialSub::Leisure));
        assert_eq!(CommercialSub::Leisure.land_use(), LandUse::Entertainment);
        assert_eq!(CommercialSub::Offices.land_use(), LandUse::Commercial);
        // The development menus stay within the land use's menu.
        for c in CommercialSub::ALL {
            for k in c.menu() {
                assert!(BusinessKind::candidates(c.land_use()).contains(k), "{c:?} {k:?}");
            }
        }
    }
}
