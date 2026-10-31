//! The city's growth: the economy snapshot, land value, the daily
//! update (rents, upkeep, redevelopment) and development of vacant lots.

use super::*;

/// [pt-growth] The lie of the land (`World::splat_setting`): metres above
/// the town's middle ground for the full hill premium, the premium, and
/// what the low ground loses at as far below.
const SETTING_RISE: f32 = 30.0;
const SETTING_HILL: f32 = 0.08;
const SETTING_LOW: f32 = 0.03;

/// Takings over costs a kind of business must have shown before more of
/// it are built (1 is break-even).
pub(super) const COVER_TO_EXPAND: f32 = 1.05;
/// A kind of business whose shelves hold more than this many days of what
/// it makes is not selling it all: no more of it are built.
const MARKET_FULL_DAYS: f32 = 0.75;
/// Largest building a kind of business new to the town is tried in.
const TRIAL_MAX_JOBS: u16 = 20;

impl World {
    pub(super) fn update_economy(&mut self) {
        // [review] One pass over the citizens and one over the buildings
        // (there were about twenty); every tally adds up in the same slot
        // order as before, so the figures are the same to the bit.
        let e = &mut self.economy;
        e.population = self.citizens.len() as u32;
        let (mut unhoused, mut working_age, mut unemployed) = (0u32, 0u32, 0u32);
        let mut class_counts = [0u32; 3];
        let mut incomes: Vec<f32> = Vec::with_capacity(self.citizens.len());
        // Transit and rail counts, and what minds may ask for: read every
        // tick, so they are tallied here rather than in each step.
        let (mut bus_wait, mut rail_wait, mut riding, mut on_train) = (0u32, 0u32, 0u32, 0u32);
        let (mut metro_wait, mut on_metro) = (0u32, 0u32);
        for c in self.citizens.values() {
            unhoused += c.home.is_none() as u32;
            // Children and retirees are not looking for work.
            if c.can_work() {
                working_age += 1;
                unemployed += c.work.is_none() as u32;
            }
            // Classes and inequality are about earners: children have no income.
            if c.stage() != crate::citizens::LifeStage::Child {
                class_counts[c.class().index()] += 1;
                incomes.push(c.income);
            }
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
        e.unhoused = unhoused;
        e.working_age = working_age;
        e.unemployed = unemployed;
        e.households = self.households.len() as u32;
        e.household_sizes = [0; 6];
        let mut stressed = 0;
        for h in self.households.values() {
            e.household_sizes[h.size().min(5)] += 1;
            let rent = h.home.and_then(|b| self.buildings.get(b)).map(|b| b.rent);
            let income: f32 = h.members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum();
            if rent.is_some_and(|r| r > income * super::trips::RENT_STRESS_SHARE) {
                stressed += 1;
            }
        }
        e.rent_stressed_households = stressed;
        let (mut jobs_permanent, mut homes_total, mut homes_free, mut jobs_total, mut jobs_free) = (0u32, 0u32, 0u32, 0u32, 0u32);
        let (mut parking_capacity, mut parking_used, mut parking_lots, mut sites, mut homes_building, mut jobs_building) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
        let mut tier_counts = [0u32; 3];
        let mut rent_sum = [0.0f32; 3];
        let mut depots = 0u32;
        // [civic-demand] Civic floor space and park ground: open, going up.
        let (mut civic_m2, mut civic_m2_building, mut park_m2, mut park_m2_building) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let lots = &self.lots;
        let ground = |b: &Building| -> f32 { std::iter::once(b.lot).chain(b.plot.extra_lots.iter().copied()).filter_map(|l| lots.get(l)).map(|l| l.area()).sum() };
        for b in self.buildings.values_mut() {
            if b.operating() {
                jobs_permanent += b.jobs as u32;
            }
            homes_total += b.homes as u32;
            homes_free += b.free_homes() as u32;
            jobs_total += b.jobs as u32;
            jobs_free += b.free_jobs() as u32;
            parking_capacity += b.parking_capacity as u32;
            parking_used += b.parking_used as u32;
            parking_lots += b.is_parking() as u32;
            // [parking-market] Each lot's busiest moment today.
            if b.parking_used > b.parking_peak {
                b.parking_peak = b.parking_used;
            }
            if b.construction.is_some() {
                sites += 1;
                let (homes, jobs) = b.planned_capacity();
                homes_building += homes as u32;
                jobs_building += jobs as u32;
            }
            if !b.being_demolished() {
                let green = b.service.is_some_and(|s| matches!(s.kind, crate::services::ServiceKind::GrandPark | crate::services::ServiceKind::BotanicalGarden));
                if b.land_use == LandUse::Civic && !b.is_city_structure() {
                    let m2 = crate::economy::civic_floor_m2(ground(b), b.floors);
                    *if b.operating() { &mut civic_m2 } else { &mut civic_m2_building } += m2;
                } else if (b.land_use == LandUse::Park && !b.is_city_structure()) || green {
                    *if b.operating() { &mut park_m2 } else { &mut park_m2_building } += ground(b);
                }
            }
            if b.land_use == LandUse::Residential {
                tier_counts[b.tier.index()] += 1;
                rent_sum[b.tier.index()] += b.rent;
            }
            depots += b.working_depot() as u32;
        }
        e.jobs_permanent = jobs_permanent;
        e.homes_total = homes_total;
        e.homes_free = homes_free;
        e.jobs_total = jobs_total;
        e.jobs_free = jobs_free;
        e.parking_capacity = parking_capacity;
        e.parking_used = parking_used;
        e.parking_lots = parking_lots;
        e.sites = sites;
        e.homes_building = homes_building;
        e.jobs_building = jobs_building;
        // [civic-demand] A run-down park counts for less (half, at worst):
        // the parks' upkeep across the city, by their area.
        let (kept_area, area) = self.parks.parks.values().fold((0.0f32, 0.0f32), |(k, a), s| (k + s.kept.clamp(0.0, 1.0) * s.area, a + s.area));
        let kept = if area > 0.0 { kept_area / area } else { 1.0 };
        e.civic_m2 = civic_m2;
        e.civic_m2_building = civic_m2_building;
        e.park_m2 = park_m2 * (0.5 + 0.5 * kept);
        e.park_m2_building = park_m2_building;
        // What the vacant civic and park lots could hold.
        let (mut civic_zoned, mut park_zoned) = (0.0f32, 0.0f32);
        for l in self.lots.values() {
            if l.building.is_none() && l.usable() {
                match l.land_use {
                    LandUse::Civic => civic_zoned += l.area() * crate::economy::CIVIC_COVERAGE * crate::economy::CIVIC_ZONED_FLOORS,
                    LandUse::Park => park_zoned += l.area(),
                    _ => {}
                }
            }
        }
        e.civic_m2_zoned = civic_zoned;
        e.park_m2_zoned = park_zoned;
        e.tier_counts = tier_counts;
        for i in 0..3 {
            e.avg_rent[i] = if tier_counts[i] > 0 { rent_sum[i] / tier_counts[i] as f32 } else { 0.0 };
        }
        e.class_counts = class_counts;
        e.gini = Economy::gini(&mut incomes);
        e.avg_land_value = self.land_value.average();
        e.recompute();
        self.tally_subway(metro_wait, on_metro);
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
        // [freight] Ordered by rail and not yet in town: waiting out of town
        // or aboard a freight train on its way.
        r.freight_waiting = self.rail.orders.iter().map(|o| o.cargo.amount).sum::<f32>() + self.rail.trains.values().flat_map(|t| t.cargo.iter()).map(|c| c.amount).sum::<f32>();
        self.available = self.available_land_uses();
        self.groceries_for_sale = self.retail_available(Resource::Groceries);
        self.meals_for_sale = self.retail_available(Resource::BakedGoods);
        self.gyms_open = self.buildings.values().any(|b| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Gym && !z.closed));
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
                // [pt-growth] Road access saturates: a block on four
                // streets is not four times as good as on one.
                b.channel(Channel::Roads);
                for (sid, seg) in self.roads.alive_segments() {
                    let (a, bp) = self.roads.segment_points(sid);
                    let len = a.distance(bp);
                    let steps = (len / 24.0).ceil().max(1.0) as usize;
                    let w = if seg.lanes() >= 4 { 0.04 } else { 0.025 };
                    for i in 0..=steps {
                        b.splat(a.lerp(bp, i as f32 / steps as f32), 110.0, w);
                    }
                }
                b.channel(Channel::Other);
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
                b.channel(Channel::Blight);
                self.splat_society(&mut b);
                b.channel(Channel::Other);
                // [social] murals and community gardens
                self.splat_social(&mut b);
                // Rotor noise under busy flight paths and round the pads.
                self.splat_air_noise(&mut b);
                // [industry] fouled ground
                self.splat_contamination(&mut b);
                // [districts] eco and historic districts lift the land
                b.channel(Channel::Landmarks);
                self.splat_districts(&mut b);
                // [waterfront] beaches, and lots on the water
                b.channel(Channel::Shore);
                self.splat_shore(&mut b);
                // [parks] run-down parks sour the land round them
                b.channel(Channel::Blight);
                self.splat_parks(&mut b);
                b.channel(Channel::Other);
                // [land] NIMBY: the land round a mansion jumps, fading.
                for n in &self.estate.nimby {
                    b.splat(n.center, n.radius, n.strength);
                }
                // [supertall] an address by a landmark tower
                b.channel(Channel::Landmarks);
                self.splat_towers(&mut b);
                b.channel(Channel::Other);
                // [pt-growth] the lie of the land: the hillsides above
                // town are dearer from the start, the low ground cheaper
                self.splat_setting(&mut b);
            }
            4 => {
                // The player's rule: a long walk to any park or night out
                // makes a neighbourhood cheaper (`land_value::access_value`).
                let (n, cell, half) = b.cells();
                let (dp, dn) = self.amenity_fields();
                let (park, night) = self.amenity_minutes_from(n, cell, half, &dp, &dn);
                b.set_access(park, night);
                // [pt-walk] Kept for each lot's own read-out (`lot_access`).
                self.connectivity.access = Some(super::connectivity::AccessFields { walk_version: self.walk.version, park: dp, night: dn });
            }
            _ => {
                b.finish(crate::land_value::BASE, &mut self.land_value);
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

    /// [pt-walk] Metres on foot from every walk node to the nearest open
    /// park and the nearest night out (`f32::INFINITY` where none can be
    /// reached).
    pub(super) fn amenity_fields(&self) -> (Vec<f32>, Vec<f32>) {
        crate::prof_scope!("amenity_fields");
        // 1 = park, 2 = nightlife, by lot.
        let mut kind: slotmap::SecondaryMap<LotId, u8> = slotmap::SecondaryMap::new();
        for b in self.buildings.values() {
            if !b.operating() {
                continue;
            }
            let bit = match b.land_use {
                LandUse::Park => 1,
                LandUse::Entertainment if !b.is_derelict() && b.business.as_ref().is_none_or(|z| !z.closed) => 2,
                // [attractions] The grand park, the zoo, the garden and the
                // pier are somewhere to walk to; the opera, the pier and
                // the observatory somewhere to go out.
                _ => match b.service.map(|s| s.kind) {
                    Some(k) if k.is_landmark() => u8::from(k.has_grounds()) | if k.night_out() { 2 } else { 0 },
                    _ => 0,
                },
            };
            if bit == 0 {
                continue;
            }
            // [pt-walk] Every lot of a park laid over several.
            for &l in b.plot.extra_lots.iter() {
                if self.lots.contains_key(l) {
                    *kind.entry(l).unwrap().or_insert(0) |= bit;
                }
            }
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
        (self.walk.distances_from(&parks), self.walk.distances_from(&nights))
    }

    /// Minutes on foot (from the fields of `amenity_fields`) from every land value cell to the nearest park and
    /// the nearest open place to go out, along the walk network rather than
    /// as the crow flies: a park across a river or a motorway with no way
    /// over is far. Two multi-source Dijkstras over the walk graph (from
    /// every node in or fronting such a lot), then each cell takes the
    /// nearest node within `ACCESS_SNAP` plus the straight bit to it.
    /// Cells with no node near are `NO_DATA` (no access term at all), cells
    /// that reach none `NONE_REACHABLE`. Cost: O(nodes log nodes) twice and
    /// one grid lookup per cell, so it stays cheap in a big city.
    pub(super) fn amenity_minutes_from(&self, n: usize, cell: f32, half: f32, dp: &[f32], dn: &[f32]) -> (Vec<f32>, Vec<f32>) {
        crate::prof_scope!("amenity_minutes");
        use crate::land_value::{ACCESS_SNAP, NONE_REACHABLE, NO_DATA};
        let per_min = WALK_MPS * 60.0;
        let mut park = vec![NO_DATA; n * n];
        let mut night = vec![NO_DATA; n * n];
        for y in 0..n {
            for x in 0..n {
                let c = Vec2::new(-half + (x as f32 + 0.5) * cell, -half + (y as f32 + 0.5) * cell);
                // [pt-walk] The nearest node anyone walks from: a lone node
                // (a lot's loop that joins nothing) read as "no parks within
                // walking reach" right beside a park (F2, F30).
                let Some(k) = self.walk.nearest_within(c, ACCESS_SNAP).filter(|&k| !self.walk.adj[k as usize].is_empty()).or_else(|| self.walk.nearest_matching_within(c, ACCESS_SNAP, |_| true)) else { continue };
                let off = self.walk.nodes[k as usize].pos.distance(c);
                let minutes = |d: &[f32]| if d[k as usize].is_finite() { ((d[k as usize] + off) / per_min).min(NONE_REACHABLE - 1.0) } else { NONE_REACHABLE };
                park[y * n + x] = minutes(dp);
                night[y * n + x] = minutes(dn);
            }
        }
        (park, night)
    }

    /// [pt-growth] Some areas start richer than others before anything is
    /// built: ground well above the town's middle (views, air, out of the
    /// flood) is worth up to `SETTING_HILL` more, the low ground down to
    /// `SETTING_LOW` less. Measured per zoned lot against the median
    /// ground height of all of them; the waterfront has its own premium
    /// (`splat_shore`).
    pub(super) fn splat_setting(&self, b: &mut LandValueBuilder) {
        if self.lots.len() < 8 {
            return;
        }
        let mut hs: Vec<f32> = self.lots.values().map(|l| self.ground_height(l.center)).collect();
        hs.sort_by(|a, b| a.total_cmp(b));
        let mid = hs[hs.len() / 2];
        let (n, cell, half) = b.cells();
        let mut add = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                let c = Vec2::new(-half + (x as f32 + 0.5) * cell, -half + (y as f32 + 0.5) * cell);
                let rise = ((self.ground_height(c) - mid) / SETTING_RISE).clamp(-1.0, 1.0);
                add[y * n + x] = if rise >= 0.0 { rise * SETTING_HILL } else { rise * SETTING_LOW };
            }
        }
        b.add_cells(&add);
    }

    /// One building's pull on the land around it.
    pub(super) fn splat_building(b: &mut LandValueBuilder, bld: &Building, lots: &SlotMap<LotId, Lot>) {
        // [tenants] A tenant is part of its building's pull, not another.
        if bld.is_tenant() {
            return;
        }
        // [under-construction] A site lifts (or sours) nothing yet: the
        // park, the tower, the station, the smokestack count once open.
        if !bld.operating() {
            return;
        }
        {
            let Some(lot) = lots.get(bld.lot) else { return };
            let c = lot.center;
            // [pt-growth] Like adds to like with diminishing returns
            // (`land_value::Channel`); `Other` is the plain sum.
            match bld.land_use {
                LandUse::Park => {
                    b.channel(Channel::Parks);
                    b.splat(c, 180.0, 0.35);
                }
                LandUse::Civic => {
                    b.channel(Channel::Civic);
                    b.splat(c, 200.0, 0.2);
                }
                LandUse::Commercial => {
                    b.channel(Channel::Commerce);
                    b.splat(c, 120.0, 0.08);
                }
                LandUse::Entertainment => {
                    b.channel(Channel::Commerce);
                    b.splat(c, 110.0, 0.06);
                    // [r4-leisure-places] A theatre or a fine-dining room lifts the
            // street; a club, a bar, a drive-through is noise next door.
            if let Some(kind) = bld.business.as_ref().filter(|z| !z.closed).map(|z| z.kind) {
                if let Some((r, w)) = kind.land_value_lift() {
                    b.channel(Channel::Commerce);
                    b.splat(c, r, w);
                }
                let loud = kind.loudness();
                if kind.is_eat_or_go_out() && loud >= 0.4 {
                    b.splat_nuisance(c, 70.0 + 60.0 * loud, 0.02 * loud);
                }
            }
            b.channel(Channel::Other);
                    b.splat(c, 50.0, -0.08);
                }
                // [industry] by sub-zone: nuisance plus a lift on cheap land
                LandUse::Industrial => Self::splat_industry(b, bld, c),
                LandUse::Residential => {
                    b.channel(Channel::Homes);
                    match bld.tier {
                        Tier::High => b.splat(c, 130.0, 0.12),
                        Tier::Low => b.splat(c, 130.0, -0.07),
                        Tier::Mid => {}
                    }
                }
            }
            b.channel(Channel::Other);
            if bld.is_derelict() {
                b.splat(c, 100.0, -0.12);
            }
            // [economy] A marina lifts the streets round it.
            if let Some(k) = bld.port.filter(|k| k.is_marina()) {
                let (r, w) = super::waterfront::marina_lift(k);
                // [pt-growth] A landmark, not one more waterfront plot: the
                // shore channel is often full along a river already.
                b.channel(Channel::Landmarks);
                b.splat(c, r, w);
                b.channel(Channel::Other);
            }
            if let Some(kind) = bld.plant {
                // [plant-scale] by its size (1 on older saves' plants).
                let (r, w) = kind.pollution_at(if kind.sized_when_built() { bld.plant_scale.unwrap_or(1.0) } else { 1.0 });
                if r > 0.0 {
                    // [industry] a nuisance: homes feel it most
                    b.splat_nuisance(c, r, -w);
                }
            }
            if let Some(link) = &bld.rail {
                b.channel(Channel::Transit);
                match link.kind {
                    RailFacility::Station => b.splat(c, 220.0, 0.25),
                    RailFacility::FreightYard => {
                        b.channel(Channel::Other);
                        b.splat(c, 160.0, -0.12)
                    }
                    // [subway] The grand terminus draws the whole district.
                    RailFacility::Central => {
                        // [pt-growth] The district-wide draw is the city's
                        // one terminus: summed as it is, not capped with
                        // the bus stops and metro entrances.
                        b.channel(Channel::Other);
                        b.splat(c, 480.0, 0.32);
                        b.channel(Channel::Transit);
                        b.splat(c, 200.0, 0.2);
                    }
                }
                b.channel(Channel::Other);
            } else if bld.metro {
                // [subway] An entrance puts the block on the map.
                b.channel(Channel::Transit);
                b.splat(c, crate::subway::LAND_VALUE_RADIUS, crate::subway::LAND_VALUE_LIFT);
                b.channel(Channel::Other);
            }
            // Services reassure, landmarks draw (`services::ServiceKind::land_value`).
            if let Some(s) = bld.service {
                let (r, mut w) = s.kind.land_value(s.tier);
                // [megacorp] A good school (well staffed) lifts the homes
                // round it more than one short of teachers.
                if s.kind == crate::services::ServiceKind::School && bld.operating() {
                    w *= 0.8 + 0.4 * bld.staffing();
                }
                if s.kind == crate::services::ServiceKind::Vertiport {
                    // Rotor noise: a nuisance the homes around feel most.
                    b.splat_nuisance(c, r, -w);
                } else if s.kind.is_landmark() {
                    // [attractions] A landmark lifts the quarter round it
                    // through its own channel, by how well it is staffed
                    // (a shut museum lifts little).
                    b.channel(Channel::Attractions);
                    b.splat(c, r, w * (0.55 + 0.45 * bld.staffing()));
                    b.channel(Channel::Other);
                } else {
                    // [pt-growth] Landmarks (monuments, stadiums) draw as
                    // landmarks; the rest reassure as services.
                    b.channel(if w <= 0.0 { Channel::Other } else if s.kind.is_attraction() { Channel::Landmarks } else { Channel::Services });
                    b.splat(c, r, w);
                    b.channel(Channel::Other);
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
            .filter_map(|(id, b)| b.plant.map(|_| (id, self.plant_upkeep(b))).or(b.battery.as_ref().map(|bat| (id, bat.kind.upkeep_per_day()))))
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
        // [batteries] Savings roll over to yesterday's.
        self.economy.power.battery_savings_today = 0.0;
        for b in self.buildings.values_mut() {
            if let Some(bat) = b.battery.as_mut() {
                bat.savings_yesterday = std::mem::take(&mut bat.savings_today);
            }
        }
        self.economy.power.brownout_hours_today = 0;
        for b in self.buildings.values_mut() {
            b.unpowered_hours_today = 0;
        }
        // Buses and depots cost money; yesterday's fares were taken as they came.
        // [pt-transit] Each line pays for the bus-minutes it ran today (a
        // bus out all service day is one bus-day): counting the buses on
        // the road at midnight, after they had gone home, charged next to
        // nothing.
        let service = (transit::SERVICE_END - transit::SERVICE_START) as f32;
        let mut bus_days = 0.0;
        let lines: Vec<RouteId> = self.transit.routes.keys().collect();
        let mut line_upkeep: Vec<(RouteId, f32)> = Vec::with_capacity(lines.len());
        for rid in lines {
            let days = self.transit.routes[rid].bus_minutes_today as f32 / service;
            let cost = days * transit::BUS_UPKEEP_PER_DAY;
            bus_days += days;
            if cost > 0.0 {
                self.economy.debit_item(Flow::TransitUpkeep, cost, Item::Route(rid));
            }
            line_upkeep.push((rid, cost));
        }
        let depots: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.depot).map(|(id, _)| id).collect();
        let transit_upkeep = bus_days * transit::BUS_UPKEEP_PER_DAY + depots.len() as f32 * transit::DEPOT_UPKEEP_PER_DAY;
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
        // [ledger] The ledger's day closes with the treasury's.
        self.close_ledger_day();
        self.economy.ledger.roll();
        // [stories] Districts' policy books close with the ledger's day.
        for d in self.districts.list.iter_mut() {
            d.books.roll();
        }
        // [tax] The tax book closes with the ledger: what is collected below
        // (incomes, takings) is booked to the new day, as in the ledger.
        self.economy.taxes.roll();
        let rs = &mut self.rail.stats;
        rs.upkeep_today = rail_upkeep;
        rs.riders_today = 0;
        rs.fares_today = 0.0;
        rs.from_out_of_town_today = 0;
        rs.to_out_of_town_today = 0;
        rs.passenger_trains_today = 0;
        rs.freight_trains_today = 0;
        rs.freight_in_today = 0.0;
        self.economy.transit.riders_today = 0;
        self.economy.transit.fares_today = 0.0;
        for (rid, cost) in line_upkeep {
            if let Some(r) = self.transit.routes.get_mut(rid) {
                r.roll_books(cost);
                r.roll_day();
            }
        }
        // Buses that left the road without going home leave no timings behind.
        let cars = &self.traffic.cars;
        self.transit.departures.retain(|id, _| cars.contains_key(*id));
        // [parking-market] Tomorrow's fees by today's busiest hour.
        self.reprice_parking();
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
                let special = !b.is_grown() || self.district_frozen(b.lot)
                    // [supertall] a league tower is never torn down for a tier
                    || b.plot.tower.is_some()
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
                // [stories] A heritage grant keeps the repairs up.
                let decay = decay * (1.0 - self.heritage_wear_cut(bid));
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
                let refit = target > tier && !(tier == Tier::Low && target == Tier::High)
                    // [pt-growth] A full block of homes is not refitted up
                    // (fewer, dearer flats) while people cannot afford one.
                    && !(lu == LandUse::Residential && self.short_of_cheap_homes());
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
        // [tenants] Shut tenants out, empty units let, growing businesses
        // into the unit next door.
        self.daily_tenants();
        // Conditions moved: a station or yard may have gone derelict.
        self.refresh_facility_cache();
    }

    /// A park standing on a lot since rezoned for something else: it stays
    /// until development wants the ground, then it is cleared.
    pub(super) fn rezoned_park(&self, lot: &Lot) -> Option<BuildingId> {
        let b = lot.building?;
        let bld = self.buildings.get(b)?;
        (bld.land_use == LandUse::Park && lot.land_use != LandUse::Park && bld.operating()).then_some(b)
    }

    pub(super) fn develop(&mut self) {
        // [relocate] Businesses bought out of their buildings move first.
        self.relocate_waiting();
        // [review] Only lots something may go on (not promised, rubble or
        // under a yard): parking and taxi yards took these as they were.
        let vacant: Vec<LotId> = self.lots.iter().filter(|(id, l)| (l.building.is_none() || self.rezoned_park(l).is_some()) && self.site_clear(*id, SiteRule::STREET)).map(|(id, _)| id).collect();
        if vacant.is_empty() {
            return;
        }
        let mut built = 0;
        let start = self.rng.random_range(0..vacant.len());
        // Parking follows the drivers who could not find a space: once a few
        // are circling where the lots are full, someone opens one nearby.
        if built < self.config.max_builds_per_interval && self.maybe_open_parking(&vacant) {
            built += 1;
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
        let any_policy = self.any_district_policy();
        // [r4-leisure] Commercial lots the player set a sub-zone on wait
        // while the market wants none of its kinds.
        let subs_viable = if self.commercial_bias.is_empty() { [true; 4] } else { self.commercial_subs_viable(None) };
        let decks = self.roads.alive_segments().any(|(sid, _)| self.roads.spans_lots(sid));
        for i in 0..vacant.len() {
            if built >= self.config.max_builds_per_interval {
                break;
            }
            let lot_id = vacant[(start + i) % vacant.len()];
            let lu = self.lots[lot_id].land_use;
            if !viable[lu.index()] {
                continue;
            }
            if lu == LandUse::Commercial && !self.commercial_bias.is_empty() && self.commercial_sub_set(lot_id).is_some_and(|s| !subs_viable[s.index()]) {
                continue;
            }
            let mut chance = match lu {
                LandUse::Park => 0.05,
                // [r4-leisure] on auto in a nightlife district, as readily
                // as nights out are wanted
                LandUse::Commercial => self.commercial_demand_at(&self.lots[lot_id]) * 0.06,
                _ => self.economy.demand_for(lu) * 0.06,
            };
            // [tax] Homes of a tier taxed over the standard go up less
            // readily, under it more (the land's tier is the likely one).
            if lu == LandUse::Residential && !self.economy.taxes.is_standard() {
                let tier = Tier::for_land_value(self.lot_value(lot_id));
                chance *= self.economy.taxes.pull(crate::tax::TaxCategory::residential(tier));
            }
            // [dirty-industry] Nobody wants to build homes or shops in the
            // shadow of a smelter: the more exposed, the less likely.
            if matches!(lu, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment) && !self.dirty.is_empty() {
                let e = self.dirt_exposure_at(self.lots[lot_id].center, crate::industry::Neighbour::Land(lu), None);
                chance *= 1.0 - super::dirt::ADJACENCY_BUILD_HIT * e;
            }
            // [stories] A district's levies put builders off, its support
            // and relief draw them.
            if any_policy {
                chance *= self.district_pull(lot_id, lu);
            }
            // [r4-decks] Under a raised road the ground goes to low uses
            // first: few want a home in the shade and din of a deck.
            if decks && lu == LandUse::Residential && self.deck_cap(lot_id).is_some() {
                chance *= UNDER_DECK_HOMES;
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
        Some(self.kinds_viable(candidates, jobs_here))
    }

    /// [tenants] [`World::business_kinds_viable`] over a menu of kinds of
    /// one's own (a shopfront's).
    pub(crate) fn kinds_viable(&self, candidates: &[BusinessKind], jobs_here: Option<u16>) -> Vec<BusinessKind> {
        self.kinds_viable_as(candidates, jobs_here, false)
    }

    /// [r4-leisure-places] [`World::kinds_viable`] for an empty unit: a
    /// tenant opens at once, so trying a new kind of place there does not
    /// wait on the new places going up elsewhere.
    pub(crate) fn kinds_viable_as(&self, candidates: &[BusinessKind], jobs_here: Option<u16>, unit: bool) -> Vec<BusinessKind> {
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
        // [r4-leisure-places] New kinds of places to eat and go out are
        // tried one at a time, and a town carries one kind more for every
        // `leisure::PEOPLE_PER_KIND` people.
        let new_kind = |k: &BusinessKind| k.is_eat_or_go_out();
        let new_sites: u32 = BusinessKind::ALL.iter().filter(|k| new_kind(k)).map(|k| sites[k.index()]).sum();
        let new_kinds_open = BusinessKind::ALL.iter().filter(|k| new_kind(k) && open[k.index()] > 0).count() as u32;
        let pop = self.economy.population;
        let new_room = (unit || new_sites < (pop / crate::leisure::PEOPLE_PER_TRIAL).max(1)) && new_kinds_open + new_sites < pop / crate::leisure::PEOPLE_PER_KIND;
        let grocery_jobs_needed = self.grocery_jobs_needed();
        let mut out: Vec<BusinessKind> = Vec::new();
        for &kind in candidates {
            // [economy] Kinds that need their site (the rock, the catch,
            // livestock, a pressure) are never a market's blind pick.
            if kind.needs_site() {
                continue;
            }
            // [r4-leisure-places] A hamlet has its grocer, its baker and its
            // club; the diners, cinemas and gyms come once enough people
            // live here to fill them.
            if kind.is_eat_or_go_out() && self.economy.population < crate::leisure::MIN_TOWN {
                continue;
            }
            let i = kind.index();
            let ok = match kind {
                BusinessKind::Office => true,
                BusinessKind::Grocery if (jobs[i] as f32) < grocery_jobs_needed && self.economy.cover_of(kind).is_none_or(|c| c >= 0.85) => true,
                _ if open[i] == 0 && kind.is_eat_or_go_out() => new_room && jobs_here.is_none_or(|j| j <= TRIAL_MAX_JOBS),
                _ if open[i] == 0 => sites[i] == 0 && jobs_here.is_none_or(|j| j <= TRIAL_MAX_JOBS),
                _ => self.economy.cover_of(kind).is_some_and(|c| c >= COVER_TO_EXPAND) && room(i) && sites[i] <= open[i] / 4,
            };
            if ok && !out.contains(&kind) {
                out.push(kind);
            }
        }
        out
    }

    /// [r4-leisure-places] For probes: the kinds of `menu` the market would
    /// open now in a building of `jobs`.
    #[doc(hidden)]
    pub fn probe_kinds_viable(&self, menu: &[BusinessKind], jobs: Option<u16>) -> Vec<BusinessKind> {
        self.kinds_viable(menu, jobs)
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
        let candidates = BusinessKind::candidates(lu);
        if candidates.is_empty() {
            return None;
        }
        self.pick_business_kind_from(candidates, jobs)
    }

    /// [r4-leisure] [`World::pick_business_kind`] over a menu of its own
    /// (a commercial sub-zone's).
    pub(super) fn pick_business_kind_from(&mut self, candidates: &'static [BusinessKind], jobs: u16) -> Option<BusinessKind> {
        let viable = self.kinds_viable(candidates, Some(jobs));
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
        candidates.iter().copied().filter(|k| !k.needs_site()).max_by(|a, b| e.cover_of(*a).unwrap_or(1.0).total_cmp(&e.cover_of(*b).unwrap_or(1.0)))
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
        // [r4-leisure] within the sub-zone the player set
        let menu = if lu == LandUse::Commercial { self.commercial_sub_set(b.lot).map(|s| s.menu()) } else { None };
        let new = match menu {
            Some(m) => self.pick_business_kind_from(m, jobs),
            None => self.pick_business_kind(lu, jobs),
        };
        if let Some(new) = new.filter(|k| *k != kind) {
            self.set_business(bid, Some(Business::new(new, jobs)));
            self.fit_business_name(bid);
        }
    }

    /// [pt-visual] A building still carrying a generic generated name that
    /// does not fit its business ("Big Sky Laundromat" for a tow company,
    /// F38) takes a name for its trade. Hash-picked: no random draw.
    pub(crate) fn fit_business_name(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let Some(kind) = b.business.as_ref().map(|z| z.kind) else { return };
        // [r4-leisure-places] A diner that opens in a car dealer's old
        // building is not "No Refunds Motors".
        if names::name_misfits(&b.name, kind) || (kind.is_eat_or_go_out() && !names::name_says_trade(&b.name, kind)) {
            if let Some(n) = names::business_name(kind, b.seed) {
                b.name = n;
            }
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
    /// [land] Also a firm's branch (`World::open_branch`).
    pub fn build_on_as(&mut self, lot_id: LotId, forced: Option<BusinessKind>) -> Option<BuildingId> {
        self.build_on_named(lot_id, forced, None)
    }

    /// [relocate] [`World::build_on_as`] with the building's name given
    /// (no draw for one): a business moving keeps its name.
    pub(super) fn build_on_named(&mut self, lot_id: LotId, forced: Option<BusinessKind>, named: Option<String>) -> Option<BuildingId> {
        let Some(lot) = self.lots.get(lot_id) else { return None };
        // A park on ground rezoned for building goes when building begins.
        if let Some(park) = self.rezoned_park(lot) {
            self.vacate(park, Some("The park was cleared for new building after a rezoning.".to_string()), None);
            let plot = self.buildings.remove(park).map(|b| b.plot).unwrap_or_default();
            self.lots[lot_id].building = None;
            self.lots[lot_id].park = None;
            self.events.push(SimEvent::BuildingRemoved(park, lot_id));
            // [review] Extra plots and owner records go with it.
            self.estate_building_gone(park, &plot, lot_id);
        }
        let Some(lot) = self.lots.get(lot_id) else { return None };
        // [land] a plot someone bought is theirs
        if lot.building.is_some() || !self.site_clear(lot_id, SiteRule::STREET) {
            return None;
        }
        let lu = self.lots[lot_id].land_use;
        // [r4-leisure] Commercial land goes up as its sub-zone: the
        // player's, else the city's pick; leisure is a night out.
        let sub = if forced.is_none() && lu == LandUse::Commercial { self.commercial_sub_for_new_building(lot_id, None) } else { None };
        let lu = if sub == Some(crate::commercial::CommercialSub::Leisure) { LandUse::Entertainment } else { lu };
        // A sub-zone the player set keeps to its menu; [r4-leisure-places]
        // on auto the city's pick keeps to its own (with offices let in on
        // shop streets, `CommercialSub::auto_menu`): corner diners and noodle
        // bars by the homes, drive-throughs and food halls on the avenues.
        let auto = forced.is_none() && lu == LandUse::Commercial && self.commercial_sub_set(lot_id).is_none();
        // A hamlet's shops on auto come from the whole commercial menu, as
        // before (its places to eat wait for a town, `leisure::MIN_TOWN`).
        let sub = sub.filter(|s| !auto || *s == crate::commercial::CommercialSub::Offices || self.economy.population >= crate::leisure::MIN_TOWN);
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
            floors = l.floors_for(lu, floors, self.height_cap(lot_id));
        }
        let seed = self.fresh_seed();
        let given_name = named.is_some();
        let name = match named {
            Some(n) => n,
            None => names::building_name(&mut self.rng, lu),
        };
        let mut b = Building::new(lot_id, lu, floors, tier, seed, name, self.clock.tick, value);
        b.look = look;
        if !self.lots[lot_id].car_access() {
            b.parking_capacity = 0;
        }
        let mut kind = match forced {
            Some(k) => Some(k),
            None => self.business_for_new_building(lot_id, lu, b.jobs, sub, auto),
        };
        // [r4-decks] Under a raised road an office opens as a shop or a
        // market stall instead (the first shop kind the market wants), as
        // under Shanghai's and Hong Kong's walkways.
        if forced.is_none() && kind == Some(BusinessKind::Office) && self.deck_cap(lot_id).is_some() {
            const SHOPS: [BusinessKind; 4] = [BusinessKind::Grocery, BusinessKind::Bakery, BusinessKind::Cafe, BusinessKind::GadgetShop];
            if let Some(k) = self.kinds_viable(&SHOPS, Some(b.jobs)).first() {
                kind = Some(*k);
            }
        }
        // [malls] A wide, deep road plot may be a strip mall (a shop or an
        // office keeps one unit, the rest let); a big one a mall round its
        // store. Not where a pressure or a move asked for a building.
        let mall = if forced.is_none() && b.plot.tower.is_none() { self.mall_form_for(lot_id, b.tier, b.seed) } else { None };
        let mall = match (mall, kind) {
            (Some(crate::estate::Form::StripMall), Some(k)) if !super::tenants::fits_shopfront(k) && k != BusinessKind::Office => None,
            // [r4-leisure-places] A diner, a cinema, a drive-through stands
            // as itself (they open in malls as tenants).
            (Some(_), Some(k)) if k.is_eat_or_go_out() => None,
            (m, _) => m,
        };
        if let Some(form) = mall {
            if form == crate::estate::Form::Mall && !kind.is_some_and(super::tenants::fits_shopfront) {
                kind = Some(BusinessKind::Grocery);
            }
            b.plot.form = Some(form);
            b.tier = b.tier.min(Tier::Mid);
            b.floors = if form == crate::estate::Form::StripMall { 2 + (crate::hash::mix(b.seed, 0x57A1) % 2) as u8 } else { 2 };
            b.recompute_capacity();
        }
        if let Some(kind) = kind {
            // [industry] some sub-zones build low: wells, fields, yards
            if let Some(ik) = kind.industry() {
                b.floors = b.floors.min(ik.max_floors()).max(1);
                b.recompute_capacity();
            }
            // [economy] sheds, forecourts and ranges stay low too ([malls]
            // a shop in a mall has its unit, the mall its floors)
            if let Some(m) = kind.max_floors().filter(|_| mall.is_none()) {
                b.floors = b.floors.min(m).max(1);
                b.recompute_capacity();
            }
            let jobs = b.jobs;
            b.business = Some(Business::new(kind, jobs));
            // [waterfront] the shore's trades are named for the water
            if let Some(n) = crate::shore::trade_name(kind, b.seed) {
                b.name = n;
            }
            // [pt-visual] and every other trade by what it is (F38); a
            // [r4-leisure-places] place to eat or go out always (a cinema
            // is never "Neon Club")
            if names::name_misfits(&b.name, kind) || (kind.is_eat_or_go_out() && !given_name) {
                if let Some(n) = names::business_name(kind, b.seed) {
                    b.name = n;
                }
            }
        }
        // [r4-decks] Under a raised road: no taller than its deck allows (a
        // mall's floors are its own, set above).
        let fit = self.deck_fit(lot_id, b.floors);
        if fit < b.floors {
            b.floors = fit;
            b.recompute_capacity();
        }
        // Nothing opens overnight: the lot becomes a building site first.
        b.construction = Some(Construction::new(if mall.is_some() { Construction::days_for(b.floors, b.tier) } else { Construction::days_for(floors, tier) }));
        b.recompute_capacity();
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        // [malls] A mall is a local company's from the start (its store's
        // firm): the company that may grow it into a mega-mall.
        if mall == Some(crate::estate::Form::Mall) {
            let _ = self.firm_of(id);
            self.estate.stats.malls_total += 1;
        }
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
    fn business_for_new_building(&mut self, lot_id: LotId, lu: LandUse, jobs: u16, sub: Option<crate::commercial::CommercialSub>, auto: bool) -> Option<BusinessKind> {
        // [tourism x districts] hotels where visitors lack beds, and the
        // trades a district's policy favours
        if let Some(k) = self.district_business(lot_id, lu, jobs) {
            return Some(k);
        }
        // [megacorp] Pizza places and bars by a busy campus.
        if let Some(k) = self.campus_business(lot_id, lu) {
            return Some(k);
        }
        // [sweep-sim] Cafés and bookshops on dear downtown streets.
        if let Some(k) = self.downtown_business(lot_id, lu) {
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
        // [r4-leisure-places] A town's first night out is the club (none open
        // or going up): the rest of the leisure menu comes after it.
        // (A hamlet's leisure menu is the club alone anyway: `MIN_TOWN`.)
        if lu == LandUse::Entertainment && self.economy.population >= crate::leisure::MIN_TOWN && !self.buildings.values().any(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue && !z.closed)) {
            return Some(BusinessKind::Venue);
        }
        // [r4-leisure] Shops and offices from their sub-zone's menu.
        if let Some(s) = sub.filter(|_| lu == LandUse::Commercial) {
            return self.pick_business_kind_from(if auto { s.auto_menu() } else { s.menu() }, jobs);
        }
        self.pick_business_kind(lu, jobs)
    }
}

/// [r4-decks] How readily homes are built on ground under a raised road,
/// against elsewhere.
const UNDER_DECK_HOMES: f32 = 0.5;

/// [balance x industry] An industrial kind whose takings cover less than
/// this share of its costs is not built again unless the player asked for it.
const INDUSTRY_MIN_COVER: f32 = 0.8;

impl World {
    /// [pt-growth] For probes: where the land value of the built lots comes
    /// from, term by term (each summed alone, before the caps), as lines of
    /// quantiles (`examples/growth_probe.rs`).
    #[doc(hidden)]
    pub fn land_value_breakdown(&self) -> String {
        let w = self;
        let mut text = String::new();
        let pts: Vec<Vec2> = w.buildings.values().filter(|b| b.land_use != LandUse::Park).filter_map(|b| w.lots.get(b.lot)).map(|l| l.center).collect();
        let lv = &w.land_value;
        let mut report = |name: &str, fill: &dyn Fn(&mut LandValueBuilder)| {
            let mut b = LandValueBuilder::new(lv);
            fill(&mut b);
            let mut out = LandValue::new(lv.half, lv.cell);
            b.finish(0.0, &mut out);
            let mut v: Vec<f32> = pts.iter().map(|p| out.raw_at(*p)).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            let q = |f: f32| v[((v.len() - 1) as f32 * f) as usize];
            text.push_str(&format!("{name:<24} p10 {:>6.2} p50 {:>6.2} p90 {:>6.2} max {:>6.2}
", q(0.1), q(0.5), q(0.9), q(1.0)));
        };
        report("roads", &|b| {
            b.channel(Channel::Roads);
            for (sid, seg) in w.roads.alive_segments() {
                let (a, bp) = w.roads.segment_points(sid);
                let steps = (a.distance(bp) / 24.0).ceil().max(1.0) as usize;
                let wt = if seg.lanes() >= 4 { 0.04 } else { 0.025 };
                for i in 0..=steps {
                    b.splat(a.lerp(bp, i as f32 / steps as f32), 110.0, wt);
                }
            }
        });
        for lu in LandUse::ALL {
            report(&format!("{lu:?} (plain)"), &|b| {
                for bld in w.buildings.values().filter(|x| x.land_use == lu && x.service.is_none() && x.rail.is_none() && !x.metro && x.plant.is_none()) {
                    World::splat_building(b, bld, &w.lots);
                }
            });
        }
        report("services", &|b| {
            for bld in w.buildings.values().filter(|x| x.service.is_some()) {
                World::splat_building(b, bld, &w.lots);
            }
        });
        report("rail/metro/plants", &|b| {
            for bld in w.buildings.values().filter(|x| x.rail.is_some() || x.metro || x.plant.is_some()) {
                World::splat_building(b, bld, &w.lots);
            }
        });
        report("vacant+cars", &|b| {
            for lot in w.lots.values().filter(|l| l.building.is_none()) {
                b.splat(lot.center, 60.0, -0.03);
            }
            for car in w.traffic.cars.values() {
                b.splat(car.pos, 45.0, -0.004);
            }
        });
        report("society", &|b| {
            b.channel(Channel::Blight);
            w.splat_society(b)
        });
        report("social", &|b| w.splat_social(b));
        report("air", &|b| w.splat_air_noise(b));
        report("contamination", &|b| w.splat_contamination(b));
        report("districts", &|b| w.splat_districts(b));
        report("shore", &|b| w.splat_shore(b));
        report("parks", &|b| {
            b.channel(Channel::Blight);
            w.splat_parks(b)
        });
        report("towers", &|b| w.splat_towers(b));
        report("setting", &|b| w.splat_setting(b));
        report("access", &|b| {
            let (n, cell, half) = b.cells();
            let (dp, dn) = w.amenity_fields();
            let (p, q) = w.amenity_minutes_from(n, cell, half, &dp, &dn);
            b.set_access(p, q);
        });
        drop(report);
        text
    }
}
