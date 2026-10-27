//! [stories] What a district's policies do in the sim (`crate::stories`).
//! Every amount moves through the ledger's door: levies are paid to the
//! treasury (`Flow::DistrictLevies`), support and grants paid by it
//! (`Flow::DistrictSpending`), itemised by district and policy, and
//! tallied on the district's own books for its panel.
//!
//! - **Rent support** (`daily_household`): the household pays its rent to
//!   the owner as ever, then the city pays it back the share; rent stress
//!   and choosing a home (`choose_home`) read the rent net of it.
//! - **Business rate, nightlife and pollution levies** (`daily_businesses`):
//!   a share of the day's takings, a cost the business carries (its cover
//!   shows it, as a tax over the standard does); relief is paid to it.
//! - **Late licence**: venues and bars open `LATE_HOURS` longer
//!   (`Business::late_hours`, set here daily); residents lose a little mood.
//! - **Pollution levy**: the district's dirty plants foul less
//!   (`refresh_dirty`).
//! - **Heritage grant**: paid daily to each standing building's owner; the
//!   buildings wear more slowly (`daily_update`).
//! - **Bed levy**: added to the tourist tax on the district's hotel nights.
//! - Growth: new buildings go up more or less readily (`Policies::pull`,
//!   in `develop`).
//!
//! Nothing here draws a random number.

use super::*;
use crate::stories::{Policies, Policy};

/// Mood a resident of a district with a late licence loses a night.
pub const LATE_NOISE_MOOD: f32 = 0.01;
/// How much of a building's wear a full heritage grant takes away.
pub const HERITAGE_WEAR_CUT: f32 = 0.75;
/// How much cleaner a district's dirty plants run per unit of pollution levy.
pub const LEVY_FILTERS: f32 = 2.0;

impl World {
    /// The district (id and policies) a building stands in, when its
    /// district has any policy.
    pub fn district_policies_of(&self, bid: BuildingId) -> Option<(u16, Policies)> {
        if self.districts.list.is_empty() {
            return None;
        }
        let lot = self.lots.get(self.buildings.get(bid)?.lot)?;
        self.districts.policies_at(lot.center)
    }

    /// [stories] Whether any district has a policy set (most cities none:
    /// every hook below is skipped then).
    pub fn any_district_policy(&self) -> bool {
        self.districts.list.iter().any(|d| !d.policies.is_none())
    }

    /// Tally `amount` (to the city positive) on district `id`'s books.
    fn book_district(&mut self, id: u16, p: Policy, amount: f32) {
        if let Some(d) = self.districts.get_mut(id) {
            d.books.today[p.index()] += amount;
        }
    }

    fn district_item(&self, id: u16, p: Policy) -> Item {
        let name = self.districts.get(id).map_or("a district", |d| d.name.as_str());
        Item::Named(format!("{name}: {}", p.label()))
    }

    /// `from` pays district `id`'s levy `p` to the city.
    pub(super) fn district_levy(&mut self, id: u16, p: Policy, from: Account, amount: f32) -> f32 {
        if amount <= 0.0 {
            return 0.0;
        }
        let item = self.district_item(id, p);
        let paid = self.pay_city(from, Flow::DistrictLevies, amount, Some(item));
        self.book_district(id, p, paid);
        paid
    }

    /// The city pays `to` under district `id`'s policy `p`.
    pub(super) fn district_pays(&mut self, id: u16, p: Policy, to: Account, amount: f32) -> f32 {
        if amount <= 0.0 {
            return 0.0;
        }
        let item = self.district_item(id, p);
        let paid = self.city_pays(to, Flow::DistrictSpending, amount, Some(item));
        self.book_district(id, p, -paid);
        paid
    }

    /// [stories] A business's district levies (or relief) on the day's
    /// `revenue`: what it paid net (relief negative), to add to its costs.
    pub(super) fn district_business_levies(&mut self, bid: BuildingId, revenue: f32) -> f32 {
        if revenue <= 0.0 {
            return 0.0;
        }
        let Some((id, pol)) = self.district_policies_of(bid) else { return 0.0 };
        let Some(b) = self.buildings.get(bid) else { return 0.0 };
        let Some(kind) = b.business.as_ref().map(|z| z.kind) else { return 0.0 };
        let lu = b.land_use;
        let acct = Account::Business(bid);
        let mut net = 0.0;
        if Self::is_nightlife(kind, lu) {
            net += self.district_levy(id, Policy::NightlifeLevy, acct, revenue * pol.nightlife_levy);
        } else if kind.industry().is_some() {
            net += self.district_levy(id, Policy::PollutionLevy, acct, revenue * pol.pollution_levy);
        } else if lu == LandUse::Commercial {
            if pol.business_rate > 0.0 {
                net += self.district_levy(id, Policy::BusinessRate, acct, revenue * pol.business_rate);
            } else if pol.business_rate < 0.0 {
                net -= self.district_pays(id, Policy::BusinessRate, acct, revenue * -pol.business_rate);
            }
        }
        net
    }

    /// Venues and bars: what the nightlife levy and late licence are about.
    pub fn is_nightlife(kind: BusinessKind, lu: LandUse) -> bool {
        lu == LandUse::Entertainment || matches!(kind, BusinessKind::Venue | BusinessKind::BeachBar)
    }

    /// [stories] Share of the rent of `home` the city pays back.
    pub fn rent_support_of(&self, home: BuildingId) -> f32 {
        self.district_policies_of(home).map_or(0.0, |(_, p)| p.rent_support)
    }

    /// [stories] The household's rent support for the day, paid into its
    /// purse; returns what was paid.
    pub(super) fn pay_rent_support(&mut self, hid: HouseholdId, home: BuildingId, rent: f32) -> f32 {
        let Some((id, pol)) = self.district_policies_of(home) else { return 0.0 };
        self.district_pays(id, Policy::RentSupport, Account::Household(hid), rent * pol.rent_support)
    }

    /// [stories] The bed levy on a night at `hotel` costing `price`, paid
    /// by `from`.
    pub(super) fn pay_bed_levy(&mut self, hotel: BuildingId, from: Account, price: f32) -> f32 {
        let Some((id, pol)) = self.district_policies_of(hotel) else { return 0.0 };
        self.district_levy(id, Policy::BedLevy, from, price * pol.bed_levy)
    }

    /// [stories] How readily a new building of `lu` goes up on `lot`.
    pub fn district_pull(&self, lot: LotId, lu: LandUse) -> f32 {
        self.lots.get(lot).and_then(|l| self.districts.policies_at(l.center)).map_or(1.0, |(_, p)| p.pull(lu))
    }

    /// [stories] Share of a building's wear its heritage grant takes away.
    pub fn heritage_wear_cut(&self, bid: BuildingId) -> f32 {
        self.district_policies_of(bid).map_or(0.0, |(_, p)| HERITAGE_WEAR_CUT * p.heritage_grant / crate::stories::MAX_HERITAGE_GRANT)
    }

    /// [stories] How much cleaner a dirty plant in the district runs.
    pub fn district_filters(&self, bid: BuildingId) -> f32 {
        self.district_policies_of(bid).map_or(1.0, |(_, p)| (1.0 - LEVY_FILTERS * p.pollution_levy).clamp(0.3, 1.0))
    }

    /// [stories] The day's district work, at midnight after the businesses
    /// and households settled: late licences on the venues, the noise on
    /// the neighbours, heritage grants to the owners. (The books roll with
    /// the ledger's day, `daily_update`.)
    pub(super) fn daily_stories(&mut self) {
        // Nothing to do with no district (and no licence left over from a
        // district since removed).
        if self.districts.list.is_empty() && self.buildings.values().all(|b| b.business.as_ref().is_none_or(|z| z.late_hours == 0)) {
            return;
        }
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let any = self.any_district_policy();
        for bid in ids {
            let pol = if any { self.district_policies_of(bid) } else { None };
            let late = pol.is_some_and(|(_, p)| p.late_hours);
            let b = &mut self.buildings[bid];
            let lu = b.land_use;
            if let Some(z) = b.business.as_mut() {
                let want = if late && Self::is_nightlife(z.kind, lu) { crate::stories::LATE_HOURS } else { 0 };
                if z.late_hours != want {
                    z.late_hours = want;
                }
            }
            let Some((id, p)) = pol else { continue };
            let b = &self.buildings[bid];
            if late && !b.residents.is_empty() {
                let residents = b.residents.clone();
                for c in residents {
                    if let Some(c) = self.citizens.get_mut(c) {
                        c.mood = (c.mood - LATE_NOISE_MOOD).max(0.0);
                    }
                }
            }
            let b = &self.buildings[bid];
            // A grant for every standing building with a private owner (the
            // city's own are kept by their upkeep; tenants' units are the
            // host's).
            if p.heritage_grant > 0.0 && b.operating() && b.is_grown() && !b.is_tenant() && !Self::is_civic_structure(b) {
                let owner = self.landlord_of(bid);
                self.district_pays(id, Policy::HeritageGrant, owner, p.heritage_grant);
            }
        }
    }
}
