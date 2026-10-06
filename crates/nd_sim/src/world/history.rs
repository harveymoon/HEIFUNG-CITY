//! Recording the city's history (`crate::history`): an hourly sample on
//! the hour and a daily record across midnight.
//!
//! A day is recorded in two halves because midnight resets the day's
//! counters part-way through: `open_day_record` reads the counters (riders,
//! trade, calls) before any of the midnight work runs, and
//! `close_day_record`, after it, adds what the midnight settles (upkeep,
//! the closed ledger, the treasury) and pushes the record.

use super::*;
use crate::history::{DayRecord, HourRecord};

impl World {
    /// The first half of the day just ending: the day's running counters,
    /// read before midnight resets them.
    pub(super) fn open_day_record(&self) -> DayRecord {
        let e = &self.economy;
        let st = &self.services.stats;
        DayRecord {
            // `SimClock::day` is 0-based; at this midnight it already names
            // the new day, which is the 1-based number of the one closing.
            day: self.clock.day() as u32,
            businesses: e.market.businesses,
            imports: e.market.imports_today,
            exports: e.market.exports_today,
            materials: e.materials_today,
            deals: e.market.deals_today,
            brownout_hours: e.power.brownout_hours_today,
            bus_riders: e.transit.riders_today,
            rail_riders: self.rail.stats.riders_today,
            ferry_riders: self.ports.stats.ferry_riders_today,
            air_riders: self.air.stats.riders_today,
            route_riders: self.transit.routes.iter().map(|(id, r)| (id, r.riders_today)).collect(),
            incidents: st.incidents_today,
            resolved: st.resolved_today,
            unanswered: st.unanswered_today,
            out_of_town_calls: st.out_of_town_calls_today,
            avg_response_min: st.avg_response(),
            ..Default::default()
        }
    }

    /// The second half: the closed ledger and the levels the day ended on.
    /// One pass over the citizens for the employed count.
    pub(super) fn close_day_record(&mut self, mut d: DayRecord) {
        let e = &self.economy;
        d.treasury = e.treasury;
        d.ledger = e.ledger.yesterday.to_vec();
        for it in &e.ledger.items_yesterday {
            match (it.flow, &it.item) {
                (Flow::IncomeTax, Item::Class(t)) => d.income_tax_by_class[*t as usize] += it.amount,
                (Flow::BusinessTax, Item::LandUse(lu)) => d.business_tax_by_use[lu.index()] += it.amount,
                _ => {}
            }
        }
        d.population = self.citizens.len() as u32;
        d.employed = self.citizens.values().filter(|c| c.work.is_some()).count() as u32;
        d.households = e.households;
        d.working_age = e.working_age;
        d.unemployed = e.unemployed;
        d.unhoused = e.unhoused;
        d.waiting_immigrants = e.waiting_immigrants;
        d.classes = e.class_counts;
        d.homes = e.homes_total;
        d.homes_filled = e.homes_total.saturating_sub(e.homes_free);
        d.jobs = e.jobs_total;
        d.jobs_filled = e.jobs_total.saturating_sub(e.jobs_free);
        d.sites = self.buildings.values().filter(|b| b.construction.is_some()).count() as u32;
        d.buildings = self.buildings.len() as u32 - d.sites;
        d.avg_land_value = e.avg_land_value;
        d.gini = e.gini;
        d.avg_rent = e.avg_rent;
        d.demand = e.demand;
        self.economy.history.push_day(d);
    }

    /// On the hour, after power is shared out.
    pub(super) fn record_hour(&mut self) {
        let e = &self.economy;
        let h = HourRecord {
            day: self.clock.day() as u32 + 1,
            hour: self.clock.hour() as u8,
            treasury: e.treasury,
            population: self.citizens.len() as u32,
            supply_mw: e.power.supply_mw,
            supply_by_kind: e.power.supply_by_kind,
            demand_mw: e.power.demand_mw,
            imported_mw: e.power.imported_mw,
            served: e.power.served,
            bus_waiting: e.transit.waiting,
            bus_riding: e.transit.riding,
            rail_riding: self.rail.stats.riding,
            cars: self.traffic.cars.len() as u32,
        };
        self.economy.history.push_hour(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::DAYS_KEPT;

    /// Two days of a small town with a police station: one record a day,
    /// numbered, with the ledger row and hourly samples; the service
    /// upkeep is itemised by building and the items add up to the row.
    #[test]
    fn days_and_hours_are_recorded() {
        let mut w = World::new(WorldConfig::test_town());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
        let sq = |x0: f32, y0: f32, s: f32| vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)];
        w.add_zone(sq(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(sq(-200.0, 10.0, 190.0), LandUse::Commercial);
        w.add_zone(sq(-200.0, -200.0, 190.0), LandUse::Civic);
        w.economy.treasury = 1e6;
        let civic: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic).map(|(id, _)| id).collect();
        let police = civic.iter().find_map(|&l| w.build_service(l, crate::services::ServiceKind::Police, Tier::Mid).ok()).expect("a police station fits on a civic lot");
        let mut minds = crate::mind::IdleMind;
        for _ in 0..(2 * 1440 + 61) {
            w.tick(&mut minds);
        }
        let h = &w.economy.history;
        assert_eq!(h.days.len(), 2);
        assert_eq!((h.days[0].day, h.days[1].day), (1, 2));
        assert!(h.days.len() <= DAYS_KEPT);
        assert_eq!(h.hours.len(), 49, "every hour from 01:00 on day 1 to 01:00 on day 3");
        let d = &h.days[1];
        assert_eq!(d.ledger, w.economy.ledger.yesterday.to_vec());
        assert!(d.demand_mwh > 0.0, "the day's energy was summed from the hours");
        let l = &w.economy.ledger;
        let (items, rest) = crate::economy::Ledger::items_of(&l.items_yesterday, &l.yesterday, Flow::ServiceUpkeep);
        assert!(items.iter().any(|e| e.item == Item::Building(police) && e.amount < 0.0), "the station's upkeep is its own line");
        assert!(rest.abs() < 0.01, "every service posting was itemised, {rest} left over");
    }
}
