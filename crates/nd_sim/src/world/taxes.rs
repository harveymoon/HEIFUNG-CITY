//! [tax] The tax schedule as the city sees it: every line with its rate,
//! the buildings and people under it and what it brought in. The rates
//! live in `Economy::taxes` (`crate::tax`); where each one acts is marked
//! `[tax]` in `develop.rs`, `market.rs`, `trips.rs` and `industry.rs`.

use super::*;
use crate::tax::TaxCategory;

/// One line of the schedule, for the Taxes window and the API.
#[derive(Clone, Debug)]
pub struct TaxRow {
    pub category: TaxCategory,
    pub rate: f32,
    pub default_rate: f32,
    /// How the rate pulls on the category (1 at the standard rate).
    pub pull: f32,
    /// Finished buildings under the line (sites not counted).
    pub buildings: u32,
    /// Homes (residential) or jobs (businesses) in them.
    pub capacity: u32,
    /// Residents or workers.
    pub people: u32,
    pub today: f32,
    pub yesterday: f32,
    /// Running average of the daily take.
    pub recent: f32,
}

impl World {
    /// Every line of the tax schedule, in `TaxCategory::ALL` order.
    pub fn tax_breakdown(&self) -> Vec<TaxRow> {
        let t = &self.economy.taxes;
        let mut rows: Vec<TaxRow> = TaxCategory::ALL
            .iter()
            .map(|&c| TaxRow { category: c, rate: t.rate(c), default_rate: c.default_rate(), pull: t.pull(c), buildings: 0, capacity: 0, people: 0, today: t.today_of(c), yesterday: t.yesterday_of(c), recent: t.recent_of(c) })
            .collect();
        for (id, b) in self.buildings.iter() {
            if b.construction.is_some() {
                continue;
            }
            let line = if b.land_use == LandUse::Residential && b.homes > 0 {
                Some((TaxCategory::residential(b.tier), b.homes as u32, b.residents.len() as u32))
            } else {
                let public = self.vehicles.city_lots.contains_key(&id);
                b.business.as_ref().filter(|z| !z.closed && !public && b.land_use != LandUse::Civic).and_then(|z| TaxCategory::of_business(z.kind, b.tier)).map(|c| (c, b.jobs as u32, b.workers.len() as u32))
            };
            if let Some((c, cap, people)) = line {
                let r = &mut rows[c.index()];
                r.buildings += 1;
                r.capacity += cap;
                r.people += people;
            }
        }
        rows
    }

    /// Set the rate of one line (clamped); returns what was set.
    pub fn set_tax_rate(&mut self, c: TaxCategory, rate: f32) -> f32 {
        let r = self.economy.taxes.set_rate(c, rate);
        self.economy.recompute();
        r
    }

    /// Set the duty on imports (clamped to 0..1); returns what was set.
    pub fn set_import_duty(&mut self, duty: f32) -> f32 {
        self.economy.taxes.set_import_duty(duty)
    }

    /// [clarity] Set the tourist tax on a hotel night; the demand for
    /// visits follows at the next tourism update.
    pub fn set_tourist_tax(&mut self, rate: f32) -> f32 {
        self.economy.taxes.set_tourist_tax(rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::industry::IndustryKind;
    use crate::tax::{TaxCategory, BUSINESS_TAX, INCOME_TAX};

    /// A street with a few houses and a shop, a household in each house.
    fn town() -> (World, Vec<BuildingId>) {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Residential);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).take(3).collect();
        let mut homes = Vec::new();
        for lot in lots {
            w.build_on(lot);
            let b = w.lots[lot].building.unwrap();
            w.finish_construction(b);
            homes.push(b);
        }
        (w, homes)
    }

    #[test]
    fn a_household_pays_only_the_rate_over_the_standard_and_the_city_books_it() {
        let (mut w, homes) = town();
        // Settle a household by hand in the first home.
        for _ in 0..3 {
            w.immigrate();
        }
        let Some((hid, home)) = w.households.iter().find_map(|(id, h)| h.home.map(|b| (id, b))) else { panic!("someone moved in") };
        let _ = homes;
        let tier = w.buildings[home].tier;
        let cat = TaxCategory::residential(tier);
        let income = w.household_income(hid);
        assert!(income > 0.0);
        // Two identical worlds, one with that tier's tax raised.
        let mut taxed = w.clone();
        taxed.set_tax_rate(cat, 0.30);
        w.daily_household(hid);
        taxed.daily_household(hid);
        let diff = w.households[hid].money - taxed.households[hid].money;
        let members = w.households[hid].members.len();
        let pocket = (0..members).map(|_| 0.0).sum::<f32>();
        let _ = pocket;
        // Pocket money is drawn from the purse in both, so compare the
        // household's whole wealth: purse plus the members' pockets.
        let wealth = |w: &World| w.households[hid].money + w.households[hid].members.iter().map(|m| w.citizens[*m].savings).sum::<f32>();
        let lost = wealth(&w) - wealth(&taxed);
        assert!((lost - income * (0.30 - INCOME_TAX)).abs() < 0.05 * income + 0.01, "lost {lost}, expected {} (purse diff {diff})", income * (0.30 - INCOME_TAX));
        // The city's take follows the rate.
        let cid = w.households[hid].members[0];
        let before = (w.economy.treasury, taxed.economy.treasury);
        w.daily_citizen(cid);
        taxed.daily_citizen(cid);
        let inc = w.citizens[cid].income;
        assert!((w.economy.treasury - before.0 - inc * INCOME_TAX).abs() < 1e-2);
        assert!((taxed.economy.treasury - before.1 - inc * 0.30).abs() < 1e-2);
        assert!((taxed.economy.taxes.today_of(cat) - inc * 0.30).abs() < 1e-2, "booked under its line");
    }

    #[test]
    fn the_breakdown_counts_buildings_per_line() {
        let (w, homes) = town();
        let rows = w.tax_breakdown();
        assert_eq!(rows.len(), TaxCategory::COUNT);
        let res: u32 = rows.iter().filter(|r| r.category.is_residential()).map(|r| r.buildings).sum();
        assert_eq!(res as usize, homes.len());
        assert!(rows.iter().all(|r| r.pull == 1.0 && r.rate == r.default_rate));
    }

    #[test]
    fn a_heavier_business_tax_costs_the_business_and_lowers_its_cover() {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Industrial);
        let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
        w.build_on(lot);
        let b = w.lots[lot].building.unwrap();
        w.finish_construction(b);
        let kind = w.buildings[b].business.as_ref().unwrap().kind;
        let cat = TaxCategory::of_business(kind, w.buildings[b].tier).expect("industry is taxed by sub-zone");
        {
            let z = w.buildings[b].business.as_mut().unwrap();
            z.revenue_today = 1000.0;
            z.spend_today = 500.0;
            z.days_open = 3;
            z.money = 5000.0;
        }
        let mut taxed = w.clone();
        taxed.set_tax_rate(cat, 0.25);
        let t0 = (w.economy.treasury, taxed.economy.treasury);
        w.daily_businesses();
        taxed.daily_businesses();
        let m = |w: &World| w.buildings[b].business.as_ref().unwrap().money;
        assert!((m(&w) - m(&taxed) - 1000.0 * (0.25 - BUSINESS_TAX)).abs() < 0.5, "the business pays the extra 20%");
        assert!((w.economy.treasury - t0.0 - 1000.0 * BUSINESS_TAX).abs() < 0.5);
        assert!((taxed.economy.treasury - t0.1 - 1000.0 * 0.25).abs() < 0.5);
        assert!(taxed.economy.cover_of(kind).unwrap() < w.economy.cover_of(kind).unwrap(), "the market reads it as a worse business");
    }

    #[test]
    fn taxing_oil_and_sparing_farms_shifts_new_industry_to_farms() {
        // The same ground, many draws: how often each sub-zone is picked.
        let site = crate::industry::IndustrySite { ground: crate::ground::GroundSample { fertility: 0.5, oil: 0.7, ..Default::default() }, land_value: 0.3, ..Default::default() };
        let base = crate::industry::weights(&site);
        let mut t = crate::tax::TaxBook::default();
        t.set_rate(TaxCategory::Oil, 0.25);
        t.set_rate(TaxCategory::Farming, 0.0);
        let mut taxed = base;
        for k in IndustryKind::ALL {
            taxed[k.index()] *= t.pull(TaxCategory::of_industry(k));
        }
        let share = |w: &[f32; crate::industry::IndustryKind::COUNT], k: IndustryKind| w[k.index()] / w.iter().sum::<f32>();
        assert!(share(&taxed, IndustryKind::OilExtraction) < share(&base, IndustryKind::OilExtraction) * 0.5);
        assert!(share(&taxed, IndustryKind::Farming) > share(&base, IndustryKind::Farming) * 1.3);
    }

    #[test]
    fn heavy_income_tax_slows_arrivals_and_cuts_residential_demand() {
        let (mut w, _) = town();
        w.update_economy();
        let d0 = w.economy.demand_for(LandUse::Residential);
        for tier in Tier::ALL {
            w.set_tax_rate(TaxCategory::residential(tier), 0.35);
        }
        w.update_economy();
        assert!(w.economy.demand_for(LandUse::Residential) < d0 * 0.6, "demand falls");
        assert!(w.residential_tax_pull() < 0.5);
        let mut base = w.clone();
        for tier in Tier::ALL {
            base.set_tax_rate(TaxCategory::residential(tier), INCOME_TAX);
        }
        let (a, b): (u32, u32) = (0..200).fold((0, 0), |(a, b), _| (a + w.arrivals(40), b + base.arrivals(40)));
        assert!((a as f32) < b as f32 * 0.6, "fewer arrive under the heavy tax: {a} vs {b}");
    }

    #[test]
    fn import_duty_raises_the_cost_of_imports_and_goes_to_the_treasury() {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Commercial);
        let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
        w.build_on(lot);
        let b = w.lots[lot].building.unwrap();
        w.finish_construction(b);
        // A gadget shop with no local supplier imports electronics.
        let jobs = w.buildings[b].jobs;
        w.buildings[b].business = Some(Business::new(BusinessKind::GadgetShop, jobs));
        let mut dutied = w.clone();
        dutied.set_import_duty(0.5);
        let m0 = w.buildings[b].business.as_ref().unwrap().money;
        w.hourly_economy();
        dutied.hourly_economy();
        let spent = m0 - w.buildings[b].business.as_ref().unwrap().money;
        let spent_d = m0 - dutied.buildings[b].business.as_ref().unwrap().money;
        let imports = w.economy.market.imports_today;
        assert!(imports > 0.0, "the shop imported");
        assert!(dutied.economy.taxes.duty_today > 0.0);
        assert!((dutied.economy.ledger.today_of(Flow::ImportDuty) - dutied.economy.taxes.duty_today).abs() < 1e-3);
        assert!(spent_d > spent + imports * 0.4, "the duty is paid on top: {spent_d} vs {spent} (imports {imports})");
        assert!(dutied.economy.taxes.imported_of(Resource::Electronics) > 0.0, "the import is remembered");
    }
}
