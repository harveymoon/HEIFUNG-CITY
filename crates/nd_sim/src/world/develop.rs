//! The city's growth: the economy snapshot, land value, the daily
//! update (rents, upkeep, redevelopment) and development of vacant lots.

use super::*;

impl World {
    pub(super) fn update_economy(&mut self) {
        let e = &mut self.economy;
        e.population = self.citizens.len() as u32;
        e.unhoused = self.citizens.values().filter(|c| c.home.is_none()).count() as u32;
        e.unemployed = self.citizens.values().filter(|c| c.work.is_none()).count() as u32;
        e.homes_total = self.buildings.values().map(|b| b.homes as u32).sum();
        e.homes_free = self.buildings.values().map(|b| b.free_homes() as u32).sum();
        e.jobs_total = self.buildings.values().map(|b| b.jobs as u32).sum();
        e.jobs_free = self.buildings.values().map(|b| b.free_jobs() as u32).sum();
        e.parking_capacity = self.buildings.values().map(|b| b.parking_capacity as u32).sum();
        e.parking_used = self.buildings.values().map(|b| b.parking_used as u32).sum();
        e.parking_lots = self.buildings.values().filter(|b| b.is_parking()).count() as u32;
        e.sites = self.buildings.values().filter(|b| b.construction.is_some()).count() as u32;
        let (hb, jb) = self.buildings.values().filter(|b| b.construction.is_some()).fold((0u32, 0u32), |(h, j), b| {
            let (homes, jobs) = b.planned_capacity();
            (h + homes as u32, j + jobs as u32)
        });
        e.homes_building = hb;
        e.jobs_building = jb;
        let mut tier_counts = [0u32; 3];
        let mut rent_sum = [0.0f32; 3];
        for b in self.buildings.values().filter(|b| b.land_use == LandUse::Residential) {
            tier_counts[b.tier.index()] += 1;
            rent_sum[b.tier.index()] += b.rent;
        }
        e.tier_counts = tier_counts;
        for i in 0..3 {
            e.avg_rent[i] = if tier_counts[i] > 0 { rent_sum[i] / tier_counts[i] as f32 } else { 0.0 };
        }
        let mut class_counts = [0u32; 3];
        let mut incomes: Vec<f32> = Vec::with_capacity(self.citizens.len());
        for c in self.citizens.values() {
            class_counts[c.class().index()] += 1;
            incomes.push(c.income);
        }
        e.class_counts = class_counts;
        e.gini = Economy::gini(&mut incomes);
        e.avg_land_value = self.land_value.average();
        e.recompute();
        // Transit and rail counts, and what minds may ask for: read every
        // tick, so they are tallied here rather than in each step.
        let (mut bus_wait, mut rail_wait, mut riding, mut on_train) = (0u32, 0u32, 0u32, 0u32);
        for c in self.citizens.values() {
            match c.activity {
                Activity::Waiting { .. } => {
                    if c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Rail { .. })) {
                        rail_wait += 1;
                    } else {
                        bus_wait += 1;
                    }
                }
                Activity::Riding { .. } => riding += 1,
                Activity::OnTrain { .. } => on_train += 1,
                _ => {}
            }
        }
        let depots = self.buildings.values().filter(|b| b.depot && !b.is_derelict()).count() as u32;
        let t = &mut self.economy.transit;
        t.buses = self.traffic.cars.values().filter(|c| c.bus.is_some()).count() as u32;
        t.depots = depots;
        t.depot_capacity = depots * transit::DEPOT_CAPACITY;
        t.waiting = bus_wait + rail_wait;
        t.riding = riding;
        let r = &mut self.rail.stats;
        r.waiting = rail_wait;
        r.riding = on_train;
        r.trains = self.rail.trains.len() as u32;
        r.stations = self.stations.len() as u32;
        r.yards = self.yards.len() as u32;
        r.freight_waiting = self.yards.iter().filter_map(|y| self.buildings.get(*y)).map(|b| b.rail_inbound.iter().map(|c| c.amount).sum::<f32>()).sum();
        self.available = self.available_land_uses();
        self.groceries_for_sale = self.retail_available(Resource::Groceries);
        self.meals_for_sale = self.retail_available(Resource::BakedGoods);
    }

    /// Sum accessibility, amenities, nuisances, traffic and neighbours into the
    /// land value grid, then refresh the rent of every building. Done in
    /// `LAND_VALUE_STRIPS` strips on consecutive ticks: roads, then the
    /// buildings in two halves, then lots and cars, then the blur and the
    /// rents. Buildings a strip boundary falls between are seen by exactly
    /// one strip, in slot order, so the result does not depend on timing.
    pub(super) fn update_land_value_strip(&mut self, strip: u8) {
        if strip == 0 {
            self.land_value_work.builder = Some(LandValueBuilder::new(&self.land_value));
        }
        let Some(mut b) = self.land_value_work.builder.take() else { return };
        match strip {
            0 => {
                for (sid, seg) in self.roads.alive_segments() {
                    let (a, bp) = self.roads.segment_points(sid);
                    let len = a.distance(bp);
                    let steps = (len / 24.0).ceil().max(1.0) as usize;
                    let w = if seg.lanes() >= 4 { 0.04 } else { 0.025 };
                    for i in 0..=steps {
                        b.splat(a.lerp(bp, i as f32 / steps as f32), 110.0, w);
                    }
                }
            }
            1 | 2 => {
                let n = self.buildings.len();
                let half = n.div_ceil(2);
                let range = if strip == 1 { 0..half } else { half..n };
                for bld in self.buildings.values().skip(range.start).take(range.end - range.start) {
                    Self::splat_building(&mut b, bld, &self.lots);
                }
            }
            3 => {
                for lot in self.lots.values() {
                    if lot.building.is_none() {
                        b.splat(lot.center, 60.0, -0.03);
                    }
                }
                for car in self.traffic.cars.values() {
                    b.splat(car.pos, 45.0, -0.004);
                }
            }
            _ => {
                b.finish(0.25, &mut self.land_value);
                let lv = &self.land_value;
                for bld in self.buildings.values_mut() {
                    if let Some(lot) = self.lots.get(bld.lot) {
                        bld.update_rent(lv.at(lot.center));
                    }
                }
                return;
            }
        }
        self.land_value_work.builder = Some(b);
    }

    /// Run every strip at once, for tests and scripts that need the grid now.
    pub fn update_land_value(&mut self) {
        for strip in 0..LAND_VALUE_STRIPS as u8 {
            self.update_land_value_strip(strip);
        }
    }

    /// One building's pull on the land around it.
    pub(super) fn splat_building(b: &mut LandValueBuilder, bld: &Building, lots: &SlotMap<LotId, Lot>) {
        {
            let Some(lot) = lots.get(bld.lot) else { return };
            let c = lot.center;
            match bld.land_use {
                LandUse::Park => b.splat(c, 180.0, 0.35),
                LandUse::Civic => b.splat(c, 200.0, 0.2),
                LandUse::Commercial => b.splat(c, 120.0, 0.08),
                LandUse::Entertainment => {
                    b.splat(c, 110.0, 0.06);
                    b.splat(c, 50.0, -0.08);
                }
                LandUse::Industrial => b.splat(c, 220.0, -0.3),
                LandUse::Residential => match bld.tier {
                    Tier::High => b.splat(c, 130.0, 0.12),
                    Tier::Low => b.splat(c, 130.0, -0.07),
                    Tier::Mid => {}
                },
            }
            if bld.is_derelict() {
                b.splat(c, 100.0, -0.12);
            }
            if let Some(kind) = bld.plant {
                let (r, w) = kind.pollution();
                if r > 0.0 {
                    b.splat(c, r, w);
                }
            }
            if let Some(link) = &bld.rail {
                match link.kind {
                    RailFacility::Station => b.splat(c, 220.0, 0.25),
                    RailFacility::FreightYard => b.splat(c, 160.0, -0.12),
                }
            }
        }
    }

    /// Once a sim day: buildings wear, upgrade or decay; citizens pay rent and
    /// react to it.
    pub(super) fn daily_update(&mut self) {
        // Treasury: yesterday's taxes came in as they were earned; plant
        // upkeep and imported power are settled now, then the books close.
        let upkeep: f32 = self.buildings.values().filter_map(|b| b.plant).map(|k| k.upkeep_per_day()).sum();
        self.economy.debit(Flow::PlantUpkeep, upkeep);
        let grid = self.economy.power.import_cost_today;
        self.economy.debit(Flow::GridImport, grid);
        self.economy.power.upkeep_today = upkeep;
        self.economy.tax_today = 0.0;
        self.economy.power.import_cost_today = 0.0;
        self.economy.power.brownout_hours_today = 0;
        for b in self.buildings.values_mut() {
            b.unpowered_hours_today = 0;
        }
        // Buses and depots cost money; yesterday's fares were taken as they came.
        let bus_days = self.traffic.cars.values().filter(|c| c.bus.is_some()).count() as f32;
        let depots = self.buildings.values().filter(|b| b.depot).count() as f32;
        let transit_upkeep = bus_days * transit::BUS_UPKEEP_PER_DAY + depots * transit::DEPOT_UPKEEP_PER_DAY;
        self.economy.debit(Flow::TransitUpkeep, transit_upkeep);
        self.economy.transit.upkeep_today = transit_upkeep;
        let stations = self.buildings.values().filter(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::Station)).count() as f32;
        let yards = self.buildings.values().filter(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard)).count() as f32;
        let rail_upkeep = stations * rail::STATION_UPKEEP_PER_DAY + yards * rail::YARD_UPKEEP_PER_DAY;
        self.economy.debit(Flow::RailUpkeep, rail_upkeep);
        self.economy.ledger.roll();
        let rs = &mut self.rail.stats;
        rs.upkeep_today = rail_upkeep;
        rs.riders_today = 0;
        rs.fares_today = 0.0;
        rs.passenger_trains_today = 0;
        rs.freight_trains_today = 0;
        rs.freight_in_today = 0.0;
        self.economy.transit.riders_today = 0;
        self.economy.transit.fares_today = 0.0;
        for r in self.transit.routes.values_mut() {
            r.riders_today = 0;
        }
        self.economy.parking_street_today = 0;
        self.economy.parking_fees_today = 0.0;
        self.economy.materials_today = 0.0;
        self.economy.walk_trips_today = 0;
        self.economy.walk_in_sales_today = 0.0;
        for l in self.lots.values_mut() {
            l.footfall_today = 0;
        }
        let tick = self.clock.tick;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for bid in ids {
            if self.buildings[bid].construction.is_some() {
                continue;
            }
            let (lu, tier, value, derelict_before, full, special) = {
                let b = &self.buildings[bid];
                let full = if b.land_use == LandUse::Residential { b.homes > 0 && b.free_homes() == 0 } else { b.jobs > 0 && b.free_jobs() == 0 };
                let value = self.lots.get(b.lot).map(|l| self.land_value.at(l.center)).unwrap_or(b.land_value);
                let special = b.is_parking() || b.plant.is_some() || b.depot || b.rail.is_some();
                (b.land_use, b.tier, value, b.is_derelict(), full, special)
            };
            let decay = match tier {
                Tier::Low => 0.02,
                Tier::Mid => 0.008,
                Tier::High => 0.002,
            };
            {
                let b = &mut self.buildings[bid];
                b.condition = (b.condition - decay + if full { 0.006 } else { 0.0 }).clamp(0.05, 1.0);
            }
            let target = Tier::for_land_value(value);
            let mut changed = false;
            // A cheap building on dear land does not last: after three days
            // in the high band it is redeveloped at the land's tier, so the
            // rent rises and those who cannot pay it move on.
            let redevelop = if lu != LandUse::Park && !special {
                let b = &mut self.buildings[bid];
                b.high_land_days = if tier == Tier::Low && target == Tier::High { b.high_land_days.saturating_add(1) } else { 0 };
                b.high_land_days >= REDEVELOP_DAYS
            } else {
                false
            };
            if redevelop {
                let lot = self.buildings[bid].lot;
                let floors = self.pick_floors(lot, lu, target);
                let b = &mut self.buildings[bid];
                b.high_land_days = 0;
                b.tier = target;
                b.floors = floors.max(1);
                b.condition = 1.0;
                b.built_tick = tick;
                let name = b.name.clone();
                let residents = b.residents.clone();
                for cid in residents {
                    if let Some(c) = self.citizens.get_mut(cid) {
                        c.remember(tick, format!("{name} was torn down and rebuilt for richer tenants. The rent went up with it."));
                    }
                }
                changed = true;
            } else if lu != LandUse::Park && !special {
                if target > tier && full && self.rng.random::<f32>() < 0.15 {
                    let b = &mut self.buildings[bid];
                    b.tier = tier.up();
                    b.condition = 1.0;
                    changed = true;
                } else if target < tier && self.rng.random::<f32>() < 0.06 {
                    let b = &mut self.buildings[bid];
                    b.tier = tier.down();
                    b.condition = (b.condition * 0.8).max(0.2);
                    changed = true;
                }
            }
            let derelict_after = self.buildings[bid].is_derelict();
            if changed || derelict_before != derelict_after {
                let seed = self.fresh_seed();
                let b = &mut self.buildings[bid];
                b.seed = seed;
                b.recompute_capacity();
                b.update_rent(value);
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            self.daily_citizen(cid);
        }
        self.daily_businesses();
        // Conditions moved: a station or yard may have gone derelict.
        self.refresh_facility_cache();
    }

    pub(super) fn develop(&mut self) {
        let vacant: Vec<LotId> = self.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, _)| id).collect();
        if vacant.is_empty() {
            return;
        }
        let mut built = 0;
        let start = self.rng.random_range(0..vacant.len());
        // Parking follows the drivers who could not find a space: once a few
        // are circling, someone opens a lot nearby.
        if self.economy.parking_street_today >= 4 && self.rng.random_bool(0.6) {
            if let Some(at) = self.economy.parking_shortage_at {
                // The vacant lot nearest the trouble spot, on land that is not
                // precious and not a park or civic site.
                let mut candidates: Vec<(f32, LotId)> = vacant
                    .iter()
                    .copied()
                    .filter(|&l| !matches!(self.lots[l].land_use, LandUse::Park | LandUse::Civic))
                    .filter(|&l| Tier::for_land_value(self.land_value.at(self.lots[l].center)) != Tier::High)
                    .map(|l| (self.lots[l].center.distance(at), l))
                    .collect();
                candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (d, lot_id) in candidates.into_iter().take(6) {
                    if d > 220.0 {
                        break;
                    }
                    if self.build_parking(lot_id).is_some() {
                        self.economy.parking_street_today = self.economy.parking_street_today.saturating_sub(8);
                        built += 1;
                        break;
                    }
                }
            }
        }
        for i in 0..vacant.len() {
            if built >= self.config.max_builds_per_interval {
                break;
            }
            let lot_id = vacant[(start + i) % vacant.len()];
            let lu = self.lots[lot_id].land_use;
            let chance = match lu {
                LandUse::Park => 0.05,
                _ => self.economy.demand_for(lu) * 0.06,
            };
            if self.rng.random::<f32>() < chance {
                self.build_on(lot_id);
                built += 1;
            }
        }
    }

    pub fn build_on(&mut self, lot_id: LotId) {
        let Some(lot) = self.lots.get(lot_id) else { return };
        if lot.landlocked || lot.building.is_some() || self.under_facility(lot.center) {
            return;
        }
        let lu = self.lots[lot_id].land_use;
        let value = self.land_value.at(self.lots[lot_id].center);
        let mut tier = Tier::for_land_value(value);
        // People waiting for somewhere affordable pull new housing down a tier.
        if lu == LandUse::Residential && self.economy.waiting_immigrants > 6 && tier > Tier::Low && self.rng.random_bool(0.5) {
            tier = tier.down();
        }
        let floors = self.pick_floors(lot_id, lu, tier);
        let seed = self.fresh_seed();
        let name = names::building_name(&mut self.rng, lu);
        let mut b = Building::new(lot_id, lu, floors, tier, seed, name, self.clock.tick, value);
        if !self.lots[lot_id].car_access() {
            b.parking_capacity = 0;
        }
        if let Some(kind) = BusinessKind::candidates(lu).choose(&mut self.rng) {
            let jobs = b.jobs;
            b.business = Some(Business::new(*kind, jobs));
        }
        // Nothing opens overnight: the lot becomes a building site first.
        b.construction = Some(Construction::new(Construction::days_for(floors, tier)));
        b.recompute_capacity();
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        self.order_materials(id);
    }
}
