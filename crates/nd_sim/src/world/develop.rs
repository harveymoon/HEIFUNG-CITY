//! The city's growth: the economy snapshot, land value, the daily
//! update (rents, upkeep, redevelopment) and development of vacant lots.

use super::*;

/// Takings over costs a kind of business must have shown before more of
/// it are built (1 is break-even).
const COVER_TO_EXPAND: f32 = 1.05;
/// A kind of business whose shelves hold more than this many days of what
/// it makes is not selling it all: no more of it are built.
const MARKET_FULL_DAYS: f32 = 0.75;
/// Largest building a kind of business new to the town is tried in.
const TRIAL_MAX_JOBS: u16 = 20;

impl World {
    pub(super) fn update_economy(&mut self) {
        let e = &mut self.economy;
        e.population = self.citizens.len() as u32;
        e.unhoused = self.citizens.values().filter(|c| c.home.is_none()).count() as u32;
        // Children and retirees are not looking for work.
        e.working_age = self.citizens.values().filter(|c| c.can_work()).count() as u32;
        e.unemployed = self.citizens.values().filter(|c| c.work.is_none() && c.can_work()).count() as u32;
        e.jobs_permanent = self.buildings.values().filter(|b| b.construction.is_none()).map(|b| b.jobs as u32).sum();
        e.households = self.households.len() as u32;
        e.household_sizes = [0; 6];
        let mut stressed = 0;
        for (hid, h) in self.households.iter() {
            e.household_sizes[h.size().min(5)] += 1;
            let rent = h.home.and_then(|b| self.buildings.get(b)).map(|b| b.rent);
            let income: f32 = h.members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum();
            if rent.is_some_and(|r| r > income * super::trips::RENT_STRESS_SHARE) {
                stressed += 1;
            }
            let _ = hid;
        }
        e.rent_stressed_households = stressed;
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
        // Classes and inequality are about earners: children have no income.
        for c in self.citizens.values().filter(|c| c.stage() != crate::citizens::LifeStage::Child) {
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
        let (mut metro_wait, mut on_metro) = (0u32, 0u32);
        for c in self.citizens.values() {
            match c.activity {
                Activity::Waiting { .. } => match c.transit.map(|t| t.leg) {
                    Some(TransitLeg::Rail { .. }) => rail_wait += 1,
                    Some(TransitLeg::Subway { .. }) => metro_wait += 1,
                    _ => bus_wait += 1,
                },
                Activity::Riding { .. } => riding += 1,
                Activity::OnTrain { .. } => on_train += 1,
                Activity::OnMetro { .. } => on_metro += 1,
                _ => {}
            }
        }
        self.tally_subway(metro_wait, on_metro);
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
    /// buildings in two halves, then lots and cars, then walking access to
    /// parks and nightlife, then the blur and the rents. Buildings a strip boundary falls between are seen by exactly
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
                // [society] Crime hot spots, camps and overflowing bins.
                self.splat_society(&mut b);
                // Rotor noise under busy flight paths and round the pads.
                self.splat_air_noise(&mut b);
                // [industry] fouled ground
                self.splat_contamination(&mut b);
                // [districts] eco and historic districts lift the land
                self.splat_districts(&mut b);
            }
            4 => {
                // The player's rule: a long walk to any park or night out
                // makes a neighbourhood cheaper (`land_value::access_value`).
                let (n, cell, half) = b.cells();
                let (park, night) = self.amenity_minutes(n, cell, half);
                b.set_access(park, night);
            }
            _ => {
                b.finish(0.25, &mut self.land_value);
                let lv = &self.land_value;
                for bld in self.buildings.values_mut() {
                    if let Some(lot) = self.lots.get(bld.lot) {
                        // [industry] as the building's land use feels it
                        bld.update_rent(lv.for_use(lot.center, bld.land_use));
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

    /// Minutes on foot from every land value cell to the nearest park and
    /// the nearest open place to go out, along the walk network rather than
    /// as the crow flies: a park across a river or a motorway with no way
    /// over is far. Two multi-source Dijkstras over the walk graph (from
    /// every node in or fronting such a lot), then each cell takes the
    /// nearest node within `ACCESS_SNAP` plus the straight bit to it.
    /// Cells with no node near are `NO_DATA` (no access term at all), cells
    /// that reach none `NONE_REACHABLE`. Cost: O(nodes log nodes) twice and
    /// one grid lookup per cell, so it stays cheap in a big city.
    pub(super) fn amenity_minutes(&self, n: usize, cell: f32, half: f32) -> (Vec<f32>, Vec<f32>) {
        crate::prof_scope!("amenity_minutes");
        use crate::land_value::{ACCESS_SNAP, NONE_REACHABLE, NO_DATA};
        // 1 = park, 2 = nightlife, by lot.
        let mut kind: slotmap::SecondaryMap<LotId, u8> = slotmap::SecondaryMap::new();
        for b in self.buildings.values() {
            if b.construction.is_some() || b.demolition.is_some() {
                continue;
            }
            let bit = match b.land_use {
                LandUse::Park => 1,
                LandUse::Entertainment if !b.is_derelict() && b.business.as_ref().is_none_or(|z| !z.closed) => 2,
                _ => continue,
            };
            if self.lots.contains_key(b.lot) {
                *kind.entry(b.lot).unwrap().or_insert(0) |= bit;
            }
        }
        let mut parks: Vec<u32> = Vec::new();
        let mut nights: Vec<u32> = Vec::new();
        for (i, node) in self.walk.nodes.iter().enumerate() {
            let bits = node.lot.iter().chain(node.near.iter()).fold(0u8, |acc, l| acc | kind.get(*l).copied().unwrap_or(0));
            if bits & 1 != 0 {
                parks.push(i as u32);
            }
            if bits & 2 != 0 {
                nights.push(i as u32);
            }
        }
        let dp = self.walk.distances_from(&parks);
        let dn = self.walk.distances_from(&nights);
        let per_min = WALK_MPS * 60.0;
        let mut park = vec![NO_DATA; n * n];
        let mut night = vec![NO_DATA; n * n];
        for y in 0..n {
            for x in 0..n {
                let c = Vec2::new(-half + (x as f32 + 0.5) * cell, -half + (y as f32 + 0.5) * cell);
                let Some(k) = self.walk.nearest_within(c, ACCESS_SNAP) else { continue };
                let off = self.walk.nodes[k as usize].pos.distance(c);
                let minutes = |d: &[f32]| if d[k as usize].is_finite() { ((d[k as usize] + off) / per_min).min(NONE_REACHABLE - 1.0) } else { NONE_REACHABLE };
                park[y * n + x] = minutes(&dp);
                night[y * n + x] = minutes(&dn);
            }
        }
        (park, night)
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
                // [industry] by sub-zone: nuisance plus a lift on cheap land
                LandUse::Industrial => Self::splat_industry(b, bld, c),
                LandUse::Residential => match bld.tier {
                    Tier::High => b.splat(c, 130.0, 0.12),
                    Tier::Low => b.splat(c, 130.0, -0.07),
                    Tier::Mid => {}
                },
            }
            if bld.is_derelict() {
                b.splat(c, 100.0, -0.12);
            }
            // [economy] A marina lifts the streets round it.
            if let Some(k) = bld.port.filter(|k| k.is_marina()) {
                let (r, w) = super::waterfront::marina_lift(k);
                b.splat(c, r, w);
            }
            if let Some(kind) = bld.plant {
                let (r, w) = kind.pollution();
                if r > 0.0 {
                    // [industry] a nuisance: homes feel it most
                    b.splat_nuisance(c, r, -w);
                }
            }
            if let Some(link) = &bld.rail {
                match link.kind {
                    RailFacility::Station => b.splat(c, 220.0, 0.25),
                    RailFacility::FreightYard => b.splat(c, 160.0, -0.12),
                    // [subway] The grand terminus draws the whole district.
                    RailFacility::Central => {
                        b.splat(c, 480.0, 0.32);
                        b.splat(c, 200.0, 0.2);
                    }
                }
            } else if bld.metro {
                // [subway] An entrance puts the block on the map.
                b.splat(c, crate::subway::LAND_VALUE_RADIUS, crate::subway::LAND_VALUE_LIFT);
            }
            // Services reassure, landmarks draw (`services::ServiceKind::land_value`).
            if let Some(s) = bld.service {
                let (r, w) = s.kind.land_value(s.tier);
                if s.kind == crate::services::ServiceKind::Vertiport {
                    // Rotor noise: a nuisance the homes around feel most.
                    b.splat_nuisance(c, r, -w);
                } else {
                    b.splat(c, r, w);
                }
            }
        }
    }

    /// Once a sim day: buildings wear, upgrade or decay; citizens pay rent and
    /// react to it.
    pub(super) fn daily_update(&mut self) {
        crate::prof_scope!("daily_update");
        // Treasury: yesterday's taxes came in as they were earned; plant
        // and battery upkeep and imported and exported power are settled
        // now, then the books close. Each plant and battery is its own line
        // in the itemised ledger.
        let plants: Vec<(BuildingId, f32)> = self
            .buildings
            .iter()
            .filter_map(|(id, b)| b.plant.map(|k| (id, k.upkeep_per_day())).or(b.battery.as_ref().map(|bat| (id, bat.kind.upkeep_per_day()))))
            .collect();
        let upkeep: f32 = plants.iter().map(|(_, x)| x).sum();
        for (id, x) in plants {
            self.economy.debit_item(Flow::PlantUpkeep, x, Item::Building(id));
        }
        let grid = self.economy.power.import_cost_today;
        self.economy.debit_item(Flow::GridImport, grid, Item::Named("power bought from the regional grid".into()));
        let sold = self.economy.power.export_income_today;
        if sold > 0.0 {
            self.economy.credit_item(Flow::GridExport, sold, Item::Named("power sold to the regional grid".into()));
        }
        self.economy.power.export_income_today = 0.0;
        self.daily_ports();
        self.economy.power.upkeep_today = upkeep;
        self.economy.tax_today = 0.0;
        self.economy.power.import_cost_today = 0.0;
        self.economy.power.brownout_hours_today = 0;
        for b in self.buildings.values_mut() {
            b.unpowered_hours_today = 0;
        }
        // Buses and depots cost money; yesterday's fares were taken as they came.
        let bus_days = self.traffic.cars.values().filter(|c| c.bus.is_some()).count() as f32;
        let depots: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.depot).map(|(id, _)| id).collect();
        let transit_upkeep = bus_days * transit::BUS_UPKEEP_PER_DAY + depots.len() as f32 * transit::DEPOT_UPKEEP_PER_DAY;
        let fleet = format!("{bus_days:.0} buses on the lines at {:.0} a day", transit::BUS_UPKEEP_PER_DAY);
        self.economy.debit_item(Flow::TransitUpkeep, bus_days * transit::BUS_UPKEEP_PER_DAY, Item::Named(fleet));
        for id in depots {
            self.economy.debit_item(Flow::TransitUpkeep, transit::DEPOT_UPKEEP_PER_DAY, Item::Building(id));
        }
        self.economy.transit.upkeep_today = transit_upkeep;
        let rail_sites: Vec<(BuildingId, f32)> = self
            .buildings
            .iter()
            .filter_map(|(id, b)| b.rail.as_ref().map(|l| (id, l.kind.upkeep_per_day())))
            .collect();
        let rail_upkeep: f32 = rail_sites.iter().map(|(_, x)| x).sum();
        for (id, x) in rail_sites {
            self.economy.debit_item(Flow::RailUpkeep, x, Item::Building(id));
        }
        self.daily_tolls();
        self.economy.ledger.roll();
        // [tax] The tax book closes with the ledger: what is collected below
        // (incomes, takings) is booked to the new day, as in the ledger.
        self.economy.taxes.roll();
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
            r.roll_day();
        }
        // Buses that left the road without going home leave no timings behind.
        let cars = &self.traffic.cars;
        self.transit.departures.retain(|id, _| cars.contains_key(*id));
        self.economy.parking_street_today = 0;
        self.economy.parking_fees_today = 0.0;
        self.economy.materials_today = 0.0;
        self.economy.walk_trips_today = 0;
        self.economy.walk_in_sales_today = 0.0;
        for l in self.lots.values_mut() {
            l.footfall_today = 0;
        }
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for bid in ids {
            if self.buildings[bid].construction.is_some() {
                continue;
            }
            let (lu, tier, value, derelict_before, full, special) = {
                let b = &self.buildings[bid];
                let full = if b.land_use == LandUse::Residential { b.homes > 0 && b.free_homes() == 0 } else { b.jobs > 0 && b.free_jobs() == 0 };
                // [industry] land value as the land use feels it
                let value = if self.lots.contains_key(b.lot) { self.lot_value(b.lot) } else { b.land_value };
                // [districts] a Historic district holds what stands as it is
                let special = b.is_parking() || b.is_special() || self.district_frozen(b.lot)
                    // [taxis] a taxi yard and a cargo hub are what they are
                    || b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::TaxiCompany | BusinessKind::CargoHub));
                (b.land_use, b.tier, value, b.is_derelict(), full, special)
            };
            let decay = match tier {
                Tier::Low => 0.02,
                Tier::Mid => 0.008,
                Tier::High => 0.002,
            };
            // [services] The city's own structures do not wear out: their
            // daily upkeep is their maintenance. (They decayed to derelict
            // in weeks, out of service but still charged, and nothing ever
            // repaired them.) Fire damage is mended by `repair_civic`.
            if !Self::is_civic_structure(&self.buildings[bid]) {
                let b = &mut self.buildings[bid];
                b.condition = (b.condition - decay + if full { 0.006 } else { 0.0 }).clamp(0.05, 1.0);
            }
            let mut target = Tier::for_land_value(value);
            // [districts] the stacks stay cheap, campuses reach up
            if let Some(look) = self.buildings[bid].look {
                target = look.tier(target);
            }
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
                // Torn down and built again: tenants out now, a demolition
                // site, then a building site, then the new building.
                let lot = self.buildings[bid].lot;
                let floors = self.pick_floors(lot, lu, target);
                self.redevelop(bid, target, floors.max(1), value);
                continue;
            } else if lu != LandUse::Park && !special {
                // A one-step upgrade in place (refit, tenants stay). A cheap
                // building on dear land two tiers up is not refitted: it
                // waits for the redevelopment above, which re-lets it.
                let refit = target > tier && !(tier == Tier::Low && target == Tier::High);
                if refit && full && self.rng.random::<f32>() < 0.15 {
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
        self.daily_citizens();
        let hids: Vec<crate::citizens::HouseholdId> = self.households.keys().collect();
        for hid in hids {
            self.daily_household(hid);
        }
        // Unmet demand for each tier of housing fades over a few days.
        for d in self.economy.housing_demand_by_tier.iter_mut() {
            *d *= 0.6;
        }
        self.economy.roll_population(self.citizens.len() as u32);
        self.daily_businesses();
        // Conditions moved: a station or yard may have gone derelict.
        self.refresh_facility_cache();
    }

    /// A park standing on a lot since rezoned for something else: it stays
    /// until development wants the ground, then it is cleared.
    pub(super) fn rezoned_park(&self, lot: &Lot) -> Option<BuildingId> {
        let b = lot.building?;
        let bld = self.buildings.get(b)?;
        (bld.land_use == LandUse::Park && lot.land_use != LandUse::Park && bld.construction.is_none() && bld.demolition.is_none()).then_some(b)
    }

    pub(super) fn develop(&mut self) {
        let vacant: Vec<LotId> = self.lots.iter().filter(|(_, l)| (l.building.is_none() || self.rezoned_park(l).is_some()) && l.usable()).map(|(id, _)| id).collect();
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
        // [taxis] Trips that wanted a cab and found none: a citizen with
        // savings opens a taxi company near them.
        if built < self.config.max_builds_per_interval && self.maybe_found_taxi_company(&vacant) {
            built += 1;
        }
        // [economy] What the city is short of (`crate::pressure`): one
        // business a round, on the lot that suits it.
        if built < self.config.max_builds_per_interval && self.open_by_pressure(&vacant) {
            built += 1;
        }
        // Land uses whose businesses would all lose money are not built on:
        // a town that cannot sell more bread does not get another bakery.
        let viable: Vec<bool> = LandUse::ALL.iter().map(|&lu| self.business_kinds_viable(lu, None).is_none_or(|v| !v.is_empty())).collect();
        for i in 0..vacant.len() {
            if built >= self.config.max_builds_per_interval {
                break;
            }
            let lot_id = vacant[(start + i) % vacant.len()];
            let lu = self.lots[lot_id].land_use;
            if !viable[lu.index()] {
                continue;
            }
            let mut chance = match lu {
                LandUse::Park => 0.05,
                _ => self.economy.demand_for(lu) * 0.06,
            };
            // [tax] Homes of a tier taxed over the standard go up less
            // readily, under it more (the land's tier is the likely one).
            if lu == LandUse::Residential && !self.economy.taxes.is_standard() {
                let tier = Tier::for_land_value(self.lot_value(lot_id));
                chance *= self.economy.taxes.pull(crate::tax::TaxCategory::residential(tier));
            }
            if self.rng.random::<f32>() < chance {
                self.build_on(lot_id);
                built += 1;
            }
        }
    }

    /// The kinds of business a new building on `lu` land could open as and
    /// expect to pay its way, by what the market says (`None` when the land
    /// use has no businesses):
    /// - groceries while the grocers' jobs open or being built are short of
    ///   what the town will eat and the grocers in town are not losing money;
    /// - offices, which are publicly funded and take on the jobless;
    /// - any other kind with no business of it yet and none being built (a
    ///   first one to try the market), or whose businesses in town have paid
    ///   their way of late (`Economy::business_cover`), are selling what
    ///   they make (`MARKET_FULL_DAYS`) and are not already adding one more
    ///   for every few open. A first one is tried only in a
    ///   building of at most `TRIAL_MAX_JOBS` jobs (`jobs_here`, when known):
    ///   a tower full of bakers is no way to find out whether the town eats
    ///   more bread.
    pub(super) fn business_kinds_viable(&self, lu: LandUse, jobs_here: Option<u16>) -> Option<Vec<BusinessKind>> {
        let candidates = BusinessKind::candidates(lu);
        if candidates.is_empty() {
            return None;
        }
        let (mut open, mut sites, mut jobs) = ([0u32; BusinessKind::COUNT], [0u32; BusinessKind::COUNT], [0u32; BusinessKind::COUNT]);
        // Output on the shelves of each kind, against a day of what they
        // make at full staff: over most of a day and the market is full.
        let (mut stock, mut day) = ([0.0f32; BusinessKind::COUNT], [0.0f32; BusinessKind::COUNT]);
        for b in self.buildings.values() {
            let Some(z) = &b.business else { continue };
            if z.closed {
                continue;
            }
            let i = z.kind.index();
            if b.construction.is_some() {
                sites[i] += 1;
                jobs[i] += b.planned_capacity().1 as u32;
            } else {
                open[i] += 1;
                jobs[i] += b.jobs as u32;
                if let Some((r, per)) = z.kind.recipe().outputs.first() {
                    stock[i] += z.stock_of(*r);
                    day[i] += per * b.jobs as f32 * crate::market::JOB_RATE * 24.0;
                }
            }
        }
        let room = |i: usize| day[i] <= 0.0 || stock[i] < day[i] * MARKET_FULL_DAYS;
        let grocery_jobs_needed = self.grocery_jobs_needed();
        let mut out: Vec<BusinessKind> = Vec::new();
        for &kind in candidates {
            // [economy] Kinds that need their site (the rock, the catch,
            // livestock, a pressure) are never a market's blind pick.
            if kind.needs_site() {
                continue;
            }
            let i = kind.index();
            let ok = match kind {
                BusinessKind::Office => true,
                BusinessKind::Grocery if (jobs[i] as f32) < grocery_jobs_needed && self.economy.cover_of(kind).is_none_or(|c| c >= 0.85) => true,
                _ if open[i] == 0 => sites[i] == 0 && jobs_here.is_none_or(|j| j <= TRIAL_MAX_JOBS),
                _ => self.economy.cover_of(kind).is_some_and(|c| c >= COVER_TO_EXPAND) && room(i) && sites[i] <= open[i] / 4,
            };
            if ok && !out.contains(&kind) {
                out.push(kind);
            }
        }
        Some(out)
    }

    /// Grocers' jobs the town needs for the people it has and most of those
    /// the homes going up will bring: two thirds of everyone's groceries
    /// (the bakeries feed the rest) at what a grocer's job sells in a day.
    pub(super) fn grocery_jobs_needed(&self) -> f32 {
        let e = &self.economy;
        let per_home = if e.households > 0 { self.citizens.len() as f32 / e.households as f32 } else { 2.2 };
        let people = self.citizens.len() as f32 + e.homes_building as f32 * per_home * 0.6;
        let per_job_day = BusinessKind::Grocery.recipe().outputs[0].1 * crate::market::JOB_RATE * BusinessKind::Grocery.hours().span() as f32;
        people * crate::citizens::GROCERIES_PER_PERSON_DAY * 0.66 / per_job_day.max(0.1)
    }

    /// The business a new building on `lu` land opens as: a grocer first
    /// while the town is short of them, else one of the viable kinds, else
    /// (when a player or script builds anyway) the kind doing best.
    pub(super) fn pick_business_kind(&mut self, lu: LandUse, jobs: u16) -> Option<BusinessKind> {
        let viable = self.business_kinds_viable(lu, Some(jobs))?;
        if viable.contains(&BusinessKind::Grocery) && self.rng.random_bool(0.6) {
            return Some(BusinessKind::Grocery);
        }
        // [tax] At the standard rates every viable kind is as likely; else
        // each is weighted by how its tax line pulls.
        let tier = Tier::Mid;
        let t = &self.economy.taxes;
        let weights: Vec<f32> = viable.iter().map(|k| crate::tax::TaxCategory::of_business(*k, tier).map_or(1.0, |c| t.pull(c))).collect();
        if weights.iter().any(|w| *w != 1.0) {
            let total: f32 = weights.iter().sum();
            if total > 0.0 {
                let mut roll = self.rng.random::<f32>() * total;
                for (k, w) in viable.iter().zip(&weights) {
                    if roll < *w {
                        return Some(*k);
                    }
                    roll -= w;
                }
                return viable.last().copied();
            }
        }
        if let Some(kind) = viable.choose(&mut self.rng) {
            return Some(*kind);
        }
        let e = &self.economy;
        BusinessKind::candidates(lu).iter().copied().filter(|k| !k.needs_site()).max_by(|a, b| e.cover_of(*a).unwrap_or(1.0).total_cmp(&e.cover_of(*b).unwrap_or(1.0)))
    }

    /// A building rebuilt bigger reopens as whatever the market wants now,
    /// unless its business was paying its way: a bakery that lost money as
    /// a corner shop does not come back as a tower full of bakers. The staff
    /// keep their jobs; the new business starts with fresh capital.
    pub(super) fn reopen_after_rebuild(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let Some(kind) = b.business.as_ref().map(|z| z.kind) else { return };
        let (lu, jobs) = (b.land_use, b.jobs);
        if matches!(kind, BusinessKind::Office | BusinessKind::Parking | BusinessKind::TaxiCompany) || self.economy.cover_of(kind).is_none_or(|c| c >= COVER_TO_EXPAND) {
            return;
        }
        if let Some(new) = self.pick_business_kind(lu, jobs).filter(|k| *k != kind) {
            self.buildings[bid].business = Some(Business::new(new, jobs));
        }
    }

    /// The tier of new housing on land that supports `land_tier`. People
    /// who found nothing they could afford (`housing_demand_by_tier`) pull
    /// it down to the tier the most of them could pay for, more often the
    /// more of them there are; a queue of newcomers pulls it down one tier.
    pub(super) fn residential_tier(&mut self, land_tier: Tier) -> Tier {
        let mut tier = land_tier;
        if tier == Tier::Low {
            return tier;
        }
        let d = self.economy.housing_demand_by_tier;
        let want = Tier::ALL.into_iter().filter(|t| *t < tier).max_by(|a, b| d[a.index()].total_cmp(&d[b.index()]));
        let pressure = d[..tier.index()].iter().sum::<f32>();
        if let Some(want) = want {
            if pressure > 3.0 && self.rng.random_bool((pressure / 12.0).clamp(0.3, 0.85) as f64) {
                tier = want;
            }
        }
        if tier > Tier::Low && self.economy.waiting_immigrants > 6 && self.rng.random_bool(0.5) {
            tier = tier.down();
        }
        tier
    }

    pub fn build_on(&mut self, lot_id: LotId) {
        self.build_on_as(lot_id, None);
    }

    /// [economy] Build on a vacant lot; with `forced`, the business is that
    /// kind (a pressure's pick, `world/pressure.rs`) and the building keeps
    /// to the kind's height. Returns the new building site.
    pub fn build_on_as(&mut self, lot_id: LotId, forced: Option<BusinessKind>) -> Option<BuildingId> {
        let Some(lot) = self.lots.get(lot_id) else { return None };
        // A park on ground rezoned for building goes when building begins.
        if let Some(park) = self.rezoned_park(lot) {
            self.vacate(park, Some("The park was cleared for new building after a rezoning.".to_string()), None);
            self.buildings.remove(park);
            self.lots[lot_id].building = None;
            self.lots[lot_id].park = None;
            self.events.push(SimEvent::BuildingRemoved(park, lot_id));
        }
        let Some(lot) = self.lots.get(lot_id) else { return None };
        if lot.landlocked || lot.building.is_some() || self.under_facility(lot.center) || self.services.is_rubble(lot_id) {
            return None;
        }
        let lu = self.lots[lot_id].land_use;
        // [industry] land value as the land use feels it
        let value = self.lot_value(lot_id);
        let mut tier = Tier::for_land_value(value);
        if lu == LandUse::Residential {
            tier = self.residential_tier(tier);
        }
        // [districts] The district's policy: its architecture, and the
        // tier and height that go with it.
        let look = self.district_look_for(lot_id, lu);
        if let Some(l) = look {
            tier = l.tier(tier);
        }
        let mut floors = self.pick_floors(lot_id, lu, tier);
        if let Some(l) = look {
            floors = l.floors(floors, self.height_cap(lot_id));
        }
        let seed = self.fresh_seed();
        let name = names::building_name(&mut self.rng, lu);
        let mut b = Building::new(lot_id, lu, floors, tier, seed, name, self.clock.tick, value);
        b.look = look;
        if !self.lots[lot_id].car_access() {
            b.parking_capacity = 0;
        }
        let kind = match forced {
            Some(k) => Some(k),
            None => self.business_for_new_building(lot_id, lu, b.jobs),
        };
        if let Some(kind) = kind {
            // [industry] some sub-zones build low: wells, fields, yards
            if let Some(ik) = kind.industry() {
                b.floors = b.floors.min(ik.max_floors()).max(1);
                b.recompute_capacity();
            }
            // [economy] sheds, forecourts and ranges stay low too
            if let Some(m) = kind.max_floors() {
                b.floors = b.floors.min(m).max(1);
                b.recompute_capacity();
            }
            let jobs = b.jobs;
            b.business = Some(Business::new(kind, jobs));
        }
        // Nothing opens overnight: the lot becomes a building site first.
        b.construction = Some(Construction::new(Construction::days_for(floors, tier)));
        b.recompute_capacity();
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        self.order_materials(id);
        Some(id)
    }
}

// [industry] Business choice for a new building, kept apart from the
// development logic above: industrial lots choose by sub-zone (ground,
// neighbours, the player's bias; see `world/industry.rs`), everything else
// draws from its land use's menu as before.
impl World {
    /// The business for a new building. Industrial lots follow their
    /// ground, neighbours and the player's bias ([industry]); the market
    /// reading ([balance]) vetoes an unforced industrial kind that is
    /// losing money and picks every other land use.
    fn business_for_new_building(&mut self, lot_id: LotId, lu: LandUse, jobs: u16) -> Option<BusinessKind> {
        // [tourism x districts] hotels where visitors lack beds, and the
        // trades a district's policy favours
        if let Some(k) = self.district_business(lot_id, lu, jobs) {
            return Some(k);
        }
        if lu == LandUse::Industrial {
            let forced = self.lots.get(lot_id).and_then(|l| self.industry_bias_at(l.center)).flatten().is_some();
            if let Some(k) = self.pick_industrial_business(lot_id) {
                if forced || self.economy.cover_of(k).is_none_or(|c| c >= INDUSTRY_MIN_COVER) {
                    return Some(k);
                }
            }
        }
        self.pick_business_kind(lu, jobs)
    }
}

/// [balance x industry] An industrial kind whose takings cover less than
/// this share of its costs is not built again unless the player asked for it.
const INDUSTRY_MIN_COVER: f32 = 0.8;
