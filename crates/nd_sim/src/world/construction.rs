//! Building sites: crews, materials and opening day; and demolition
//! sites: wrecking crews, rubble trucks and the cleared lot.

use super::*;
use super::market::TruckOrigin;
use crate::buildings::{Demolition, SiteBlock};

/// A site this far into its build is only groundworks: a bulldozed one is
/// cleared at once rather than demolished.
const GROUNDWORKS: f32 = 0.08;
/// [pt-growth] Sites are stepped in this many slots of ten minutes each.
pub(super) const SITE_SLOTS: u64 = 6;

/// [pt-growth] The ten-minute slot of the hour in which a site's crew
/// reports progress: a hash of the building, so neighbouring sites step
/// at different minutes.
pub(super) fn site_slot(id: BuildingId) -> u64 {
    use slotmap::Key;
    crate::hash::mix(id.data().as_ffi(), 0x5173_51DE) % SITE_SLOTS
}

/// Colour of the dump trucks hauling rubble away.
const RUBBLE_TRUCK_COLOR: u8 = 5;

impl World {
    /// Every ten sim minutes, a sixth of the sites (each its own slot in the
    /// hour, [`site_slot`]): building sites advance, wait for materials at
    /// each quarter, and open when the work is done. [pt-growth] Crews work
    /// the working day only ([`Clock::is_work_hours`], when the cranes
    /// turn), so sites stand still at night; and as each site has its own
    /// minute, the city does not grow a storey all at once.
    pub(super) fn step_construction(&mut self) {
        let slot = (self.clock.tick / 10) % SITE_SLOTS;
        let working = self.clock.is_work_hours();
        // A pure demolition and a site with materials on order still see
        // the hour pass (trucks are ordered round the clock).
        self.step_demolition(slot, working);
        // A site still clearing what stood before waits for the wrecking
        // crew; its materials are ordered when the lot is clear.
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(id, b)| b.construction.is_some() && b.demolition.is_none() && site_slot(*id) == slot).map(|(id, _)| id).collect();
        // [r2-traffic] The street networks a truck from out of town can
        // drive (worked out once, only when some site steps now).
        let reach = if ids.is_empty() { None } else { Some(self.supply_networks()) };
        let tick = self.clock.tick;
        for bid in ids {
            // [r2-traffic] A plot no truck can get to does not build: it
            // says so (K6). Deliveries that found no way wait for
            // `retry_at` and are sent again.
            let builders_ok = reach.as_ref().is_none_or(|r| self.builders_reach(bid, r));
            let newly = {
                let c = self.buildings[bid].construction.as_mut().unwrap();
                let was = c.blocked;
                if !builders_ok {
                    c.blocked = Some(SiteBlock::Builders);
                } else if c.blocked == Some(SiteBlock::Builders) {
                    c.blocked = None;
                }
                (c.blocked.is_some() && c.blocked != was).then_some(c.blocked).flatten()
            };
            if let Some(why) = newly {
                self.site_blocked_alert(bid, why);
            }
            if !builders_ok {
                continue;
            }
            let (order, waiting, before, after) = {
                let c = self.buildings[bid].construction.as_mut().unwrap();
                let before = c.progress;
                // One load is kept on the road ahead of the work, so a
                // quarter only stalls when its truck is late.
                let order = !c.ordered && c.deliveries < Construction::DELIVERIES && c.deliveries <= c.stage() + 1 && tick >= c.retry_at;
                if c.waiting_for_materials() || !working {
                    (order, true, before, before)
                } else {
                    let ceiling = (c.stage() as f32 + 1.0) * 0.25;
                    c.progress = (c.progress + c.work_hour_step()).min(ceiling);
                    (order, false, before, c.progress)
                }
            };
            if order {
                self.order_materials(bid);
            }
            if waiting {
                continue;
            }
            if after >= 1.0 {
                self.finish_construction(bid);
            } else if (after * 10.0).floor() > (before * 10.0).floor() {
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
    }

    /// [r2-traffic] The street networks (`drive_component`) that reach a
    /// way in from elsewhere (a road off the map, the highway): where a
    /// truck from out of town, or a builder's van, can drive. Empty when
    /// the city has no way in by road at all (then nothing is held back:
    /// a test world, or a city that lives by rail).
    pub(super) fn supply_networks(&self) -> std::collections::HashSet<u32> {
        self.city_leaves().into_iter().filter_map(|n| self.drive_component(n)).collect()
    }

    /// [r2-traffic] Can the builders and their trucks drive to `bid`'s
    /// plot from the city's way in (`supply_networks`)? Homes on a footpath
    /// always can (crews and deliveries come on foot from the nearest
    /// street), as can a building spreading onto the land next door: only
    /// its own plot, the one on the street, is looked at.
    pub(super) fn builders_reach(&self, bid: BuildingId, nets: &std::collections::HashSet<u32>) -> bool {
        if nets.is_empty() {
            return true;
        }
        let Some(b) = self.buildings.get(bid) else { return true };
        let Some(lot) = self.lots.get(b.lot) else { return true };
        let path_home = b.land_use == crate::zoning::LandUse::Residential && !self.roads.try_segment(lot.front_segment).is_some_and(|s| s.is_street());
        if path_home {
            return true;
        }
        let Some((seg, _)) = self.truck_stop(bid) else { return false };
        let Some(s) = self.roads.try_segment(seg).filter(|s| s.alive && s.is_street()) else { return false };
        [s.a, s.b].into_iter().any(|n| self.drive_component(n).is_some_and(|c| nets.contains(&c)))
    }

    /// [r2-traffic] Building sites held up because no truck can get to
    /// them: the site, its plot's centre and why (for the marks on the map).
    pub fn blocked_sites(&self) -> Vec<(BuildingId, Vec2, SiteBlock)> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.demolition.is_none())
            .filter_map(|(id, b)| Some((id, self.lots.get(b.lot)?.center, b.construction.as_ref()?.blocked?)))
            .collect()
    }

    /// [r2-traffic] Tell the player once a site is held up: an alert (a
    /// toast; the inspector and the map say so while it lasts).
    pub(super) fn site_blocked_alert(&mut self, bid: BuildingId, why: SiteBlock) {
        let Some(b) = self.buildings.get(bid) else { return };
        let pos = self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center);
        let text = format!("{}: {} (the building site waits)", b.name, why.text());
        let tick = self.clock.tick;
        self.connectivity.push_alert(tick, crate::world::connectivity::AlertKind::SiteBlocked, pos, Some(bid), 1, text);
    }

    /// A truckload of parts for a building site: from a local fab when one
    /// has stock, else imported. The developer pays, so the money comes from
    /// outside the city like export income does.
    pub(super) fn order_materials(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let Some(lot) = self.lots.get(b.lot) else { return };
        let at = lot.center;
        let amount = 4.0 + b.floors as f32 * 2.0;
        let resource = Resource::Parts;
        if let Some(c) = self.buildings[bid].construction.as_mut() {
            c.ordered = true;
        }
        // [K34] Steel cut from old containers at a scrap yard in town goes
        // into the frame first.
        if let Some(yard) = self.scrap_steel_for(at, amount * 0.5) {
            let got = self.buildings[yard].business.as_mut().unwrap().take(Resource::Metal, amount * 0.5);
            let cost = got * Resource::Metal.base_price();
            if let Some(z) = self.buildings[yard].business.as_mut() {
                z.revenue_today += cost;
                z.deals += 1;
            }
            let payer = self.site_payer(bid);
            self.transfer(payer, Account::Business(yard), cost, Kind::Building);
            self.economy.market.deals_today += 1;
            self.ship(yard, bid, Resource::Metal, got, cost);
            return;
        }
        let seller = self.best_seller(resource, at, amount, bid);
        if let Some((seller_id, price)) = seller {
            let got = self.buildings[seller_id].business.as_mut().unwrap().take(resource, amount);
            if got > 0.0 {
                let cost = got * price;
                let sb = self.buildings[seller_id].business.as_mut().unwrap();
                sb.revenue_today += cost;
                sb.deals += 1;
                // [ledger] Paid by whoever is building: an owner with its
                // own money, else a developer from out of town.
                let payer = self.site_payer(bid);
                self.transfer(payer, Account::Business(seller_id), cost, Kind::Building);
                self.economy.market.deals_today += 1;
                self.ship(seller_id, bid, resource, got, cost);
                return;
            }
        }
        let cost = amount * resource.base_price() * IMPORT_MARKUP;
        self.economy.materials_today += cost;
        let payer = self.site_payer(bid);
        self.transfer(payer, Account::Outside, cost, Kind::Building);
        self.import(bid, resource, amount, 0.0);
    }

    /// Open a finished building: the real homes and jobs appear, the site
    /// crew is let go (and steps outside if it was on site). Public so tests
    /// and scripts can skip the wait.
    pub fn finish_construction(&mut self, bid: BuildingId) {
        // Whatever stood there comes down first.
        if self.buildings.get(bid).is_some_and(|b| b.demolition.is_some()) {
            self.finish_demolition(bid);
        }
        let tick = self.clock.tick;
        let (car_access, entrance) = match self.buildings.get(bid).and_then(|b| self.lots.get(b.lot)) {
            Some(l) => (l.car_access(), l.entrance()),
            None => (true, Vec2::ZERO),
        };
        let Some(b) = self.buildings.get_mut(bid) else { return };
        if b.construction.take().is_none() {
            return;
        }
        let crew = std::mem::take(&mut b.workers);
        b.recompute_capacity();
        if !car_access {
            b.parking_capacity = 0;
        }
        b.built_tick = tick;
        let name = b.name.clone();
        for cid in crew {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, Some(format!("Topped out {name}. Crew's laid off; looking for the next site.")));
            }
            let Some(c) = self.citizens.get_mut(cid) else { continue };
            if c.inside == Some(bid) {
                c.inside = None;
                c.pos = entrance;
                c.prev_pos = entrance;
                c.activity = Activity::Idle;
                c.next_decision_tick = tick + 1;
            }
        }
        // [K34] A stack developer's home is built from its boxes.
        self.box_home_finished(bid);
        self.events.push(SimEvent::BuildingChanged(bid));
        // [land] The buyer moves in; a mansion starts its NIMBY pull.
        self.estate_finished(bid);
        // [tenants] Cut into units to let, if it can be.
        self.plan_units(bid, false);
        // [megablocks] An arcology opens, its corp's own shops with it.
        self.megablock_opened(bid);
        // [relocate] A business that moved here takes its old staff back.
        self.rehire_moved_staff(bid);
        self.building_opened(bid);
    }

    /// [under-construction] `bid` has just opened (topped out, or its lot
    /// cleared): the lists that only list working buildings take it in now
    /// rather than at their next refresh.
    fn building_opened(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid).filter(|b| b.operating()) else { return };
        let (rail, power) = (b.rail.is_some() || b.metro, b.is_power());
        self.refresh_closed_grounds();
        if rail {
            self.refresh_facility_cache();
        }
        if power {
            self.refresh_power();
        }
    }
}

impl World {
    /// Every sim hour: wrecking crews take buildings down from the top,
    /// rubble trucks leave as the debris piles up, and finished sites hand
    /// the lot over (vacant, or to the building waiting for it).
    /// [pt-growth] In the site's own slot of the hour, and in working hours
    /// only, as the builders.
    pub(super) fn step_demolition(&mut self, slot: u64, working: bool) {
        if !working {
            return;
        }
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(id, b)| b.demolition.is_some() && site_slot(*id) == slot).map(|(id, _)| id).collect();
        for bid in ids {
            let (before, after, haul) = {
                let d = self.buildings[bid].demolition.as_mut().unwrap();
                let before = d.progress;
                d.progress = (d.progress + d.work_hour_step()).min(1.0);
                let haul = d.loads < d.loads_due();
                if haul {
                    d.loads += 1;
                }
                (before, d.progress, haul)
            };
            if haul {
                self.haul_rubble(bid);
            }
            if after >= 1.0 {
                self.finish_demolition(bid);
            } else if (after * 10.0).floor() > (before * 10.0).floor() {
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
    }

    /// [pt-growth] Say why `bid` is coming down, if it is and nobody said.
    pub(super) fn tag_demolition(&mut self, bid: BuildingId, reason: crate::buildings::DemolitionReason) {
        if let Some(d) = self.buildings.get_mut(bid).and_then(|b| b.demolition.as_mut()) {
            if d.reason == crate::buildings::DemolitionReason::Unknown {
                d.reason = reason;
            }
        }
    }

    /// A dump truck of rubble leaves the site: to the nearest freight yard
    /// when the city has one (the rubble goes out by rail), else off the
    /// edge of the map. The site does not wait for it.
    fn haul_rubble(&mut self, bid: BuildingId) {
        let amount = 6.0 + self.buildings.get(bid).and_then(|b| b.demolition.as_ref()).map_or(0.0, |d| d.floors as f32 * 2.0);
        if let Some(yard) = self.nearest_yard(bid) {
            // Nothing is sold and nothing stocked: the yard takes it off the
            // developer's hands.
            let cargo = Cargo { from: bid, to: yard, resource: Resource::Parts, amount: 0.0, price: 0.0 };
            self.dispatch_truck(TruckOrigin::Building(bid), cargo, RUBBLE_TRUCK_COLOR);
            return;
        }
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            return;
        }
        let route = (|| {
            let (from_seg, from_pt) = self.truck_stop(bid)?;
            // The road out of town furthest from the centre, among those
            // the site's own street reaches.
            let comp = self.road_component(self.roads.segment(from_seg).a);
            let entry = self.city_leaves().into_iter().filter(|&n| self.road_component(n) == comp).max_by(|a, b| self.roads.node(*a).pos.length().total_cmp(&self.roads.node(*b).pos.length()))?;
            let node = self.roads.node(entry);
            let seg = *node.segments.iter().find(|&&s| self.roads.segment(s).alive)?;
            self.traffic.net.find_route(from_seg, from_pt, seg, node.pos)
        })();
        if let Some((route, s0, s1)) = route {
            // Bound out of town: a null buyer, so arriving delivers nothing.
            let cargo = Cargo { from: bid, to: BuildingId::default(), resource: Resource::Parts, amount, price: 0.0 };
            self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, RUBBLE_TRUCK_COLOR);
        }
    }

    /// The last of the rubble is gone. A building that was only coming
    /// down is removed and its lot left vacant (a lot made for the site
    /// goes with it); one being replaced moves on to building its
    /// successor, and a civic structure that took the lot over appears.
    /// Public so tests and scripts can skip the wait.
    pub fn finish_demolition(&mut self, bid: BuildingId) {
        let tick = self.clock.tick;
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let Some(d) = b.demolition.take() else { return };
        if !d.clear_lot {
            if let Some(c) = b.construction.as_mut() {
                // Loads that turned up for the old building are not parts.
                c.deliveries = 0;
                c.ordered = false;
            }
            let site = b.construction.is_some();
            if site {
                // From wrecking crew to building crew.
                b.recompute_capacity();
                b.built_tick = tick;
            }
            self.events.push(SimEvent::BuildingChanged(bid));
            if site {
                self.order_materials(bid);
            } else {
                self.building_opened(bid);
            }
            return;
        }
        let lot = b.lot;
        let name = b.name.clone();
        let crew = std::mem::take(&mut b.workers);
        for cid in crew {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, Some(format!("{name} is down and carted off. Crew's laid off; looking for the next job.")));
            }
        }
        let entrance = self.lots.get(lot).filter(|l| l.building == Some(bid)).map(|l| l.entrance());
        for c in self.citizens.values_mut().filter(|c| c.inside == Some(bid)) {
            c.inside = None;
            c.activity = Activity::Idle;
            c.next_decision_tick = tick + 1;
            if let Some(p) = entrance {
                c.pos = p;
                c.prev_pos = p;
            }
        }
        let plot = self.buildings.remove(bid).map(|b| b.plot).unwrap_or_default();
        if self.lots.get(lot).is_some_and(|l| l.building == Some(bid)) {
            self.lots[lot].building = None;
            let made_for_site = d.own_lot
                && !self.zones.contains_key(self.lots[lot].zone)
                && !self.facilities.values().any(|f| f.lot == Some(lot))
                && !self.ports.ports.values().any(|p| p.lot == Some(lot));
            if made_for_site {
                self.lots.remove(lot);
                self.rebuild_walk();
                self.events.push(SimEvent::LotsChanged);
            }
        }
        self.events.push(SimEvent::BuildingRemoved(bid, lot));
        // [land] Its extra plots are free; a plot someone bought joins them.
        self.estate_building_gone(bid, &plot, lot);
    }

    /// Called by `demolish_with`: turn the building into a demolition site
    /// instead of removing it. False when it should go at once: civic
    /// structures and car parks (they appear at once too), sites still at
    /// groundworks, and buildings whose ground is gone under a road or
    /// another building.
    pub(super) fn begin_demolition(&mut self, bid: BuildingId, at: Option<Vec2>, memory: Option<&str>) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        if Self::is_civic_structure(b) || b.is_parking() {
            return false;
        }
        let already = b.demolition.is_some();
        if !already && b.construction.as_ref().is_some_and(|c| c.progress < GROUNDWORKS) {
            return false;
        }
        // The parcel to fence off: its own, or after a rebuild whatever
        // parcel now covers the spot it stood on.
        let (lot, own_lot) = match at {
            None if self.lots.get(b.lot).is_some_and(|l| l.building == Some(bid)) => (b.lot, b.demolition.as_ref().is_some_and(|d| d.own_lot)),
            None => return false,
            Some(p) => match self.demolition_lot(bid, p) {
                Some(found) => found,
                None => return false,
            },
        };
        if !already {
            self.vacate(bid, memory.map(str::to_string), at);
        }
        let b = &mut self.buildings[bid];
        let mut d = match b.demolition.take() {
            Some(d) => d,
            None => {
                let mut d = Demolition::of(b, true);
                if let Some(c) = &b.construction {
                    // A site part-built: only what stands comes down.
                    let built = ((c.progress - GROUNDWORKS) / 0.84).clamp(0.0, 1.0);
                    d.floors = ((b.floors as f32 * built).round() as u8).max(1);
                    d.days = Demolition::days_for(d.floors);
                }
                d
            }
        };
        // A redevelopment interrupted half way: nothing follows any more.
        d.clear_lot = true;
        d.own_lot = own_lot;
        b.demolition = Some(d);
        // The placeholder keeps it out of every home, job, shop and market
        // search, exactly as a building site is.
        if b.construction.is_none() {
            b.construction = Some(Construction::new(1.0));
        }
        b.business = None;
        b.high_land_days = 0;
        b.lot = lot;
        b.recompute_capacity();
        self.lots[lot].building = Some(bid);
        self.events.push(SimEvent::BuildingChanged(bid));
        true
    }

    /// Where a demolition site stands after a rebuild: the parcel that now
    /// covers the spot when it is free, or a parcel made for the purpose
    /// when the zone was erased. `None` (clear it at once) when another
    /// building holds the spot or a road runs over it; a demolition site
    /// already there takes on this one's rubble.
    fn demolition_lot(&mut self, bid: BuildingId, at: Vec2) -> Option<(LotId, bool)> {
        let hit = self.lots.iter().find(|(_, l)| l.contains(at)).map(|(id, l)| (id, l.building));
        match hit {
            Some((id, None)) => Some((id, false)),
            Some((id, Some(other))) if other == bid => Some((id, false)),
            Some((_, Some(other))) => {
                let floors = self.buildings.get(bid).map_or(1, |b| b.demolition.as_ref().map_or(b.floors, |d| d.floors));
                if let Some(d) = self.buildings.get_mut(other).and_then(|o| o.demolition.as_mut()) {
                    d.floors = d.floors.max(floors);
                    d.days = d.days.max(Demolition::days_for(d.floors));
                }
                None
            }
            None => {
                let lu = self.buildings.get(bid).map_or(LandUse::Residential, |b| b.demolition.as_ref().map_or(b.land_use, |d| d.land_use));
                let lot = self.rubble_lot(at, lu)?;
                Some((self.lots.insert(lot), true))
            }
        }
    }

    /// A parcel of the usual size for `lu`, centred where a building stood,
    /// facing the nearest street or path. `None` when a road now runs
    /// through the spot.
    fn rubble_lot(&self, at: Vec2, lu: LandUse) -> Option<Lot> {
        let (sid, t, q) = self.roads.nearest_on_network(at)?;
        let clearance = q.distance(at) - self.roads.segment(sid).half_width() - crate::zoning::SIDEWALK;
        if clearance < 4.0 {
            return None;
        }
        let front_dir = (q - at).normalize_or_zero();
        if front_dir == Vec2::ZERO {
            return None;
        }
        let (hw, hd) = (lu.lot_width() * 0.5, (lu.lot_depth() * 0.5).min(clearance));
        let inward = -front_dir;
        let along = Vec2::new(inward.y, -inward.x);
        let corners = [at - along * hw - inward * hd, at + along * hw - inward * hd, at + along * hw + inward * hd, at - along * hw + inward * hd];
        let entrance = corners[0].lerp(corners[1], 0.5);
        let (car_segment, car_point) = if self.roads.segment(sid).is_street() {
            (Some(sid), entrance)
        } else {
            match self.roads.nearest_street(entrance) {
                Some((cs, _, cq)) if cq.distance(entrance) < 260.0 => (Some(cs), cq),
                _ => (None, entrance),
            }
        };
        Some(Lot {
            zone: ZoneId::default(),
            land_use: lu,
            polygon: corners.to_vec(),
            corners,
            front_edge: [corners[0], corners[1]],
            center: at,
            front_dir,
            front_segment: sid,
            front_t: t,
            access_node: self.roads.nearest_endpoint(sid, t),
            building: None,
            landlocked: false,
            footfall_today: 0,
            footfall_hour: 0,
            car_segment,
            car_point,
            park: None,
            shore: Default::default(),
        })
    }

    /// Everyone leaves a building that is coming down: households lose
    /// their home (with `memory`, or a plain note), staff are laid off,
    /// anyone inside steps out.
    pub(super) fn vacate(&mut self, bid: BuildingId, memory: Option<String>, at: Option<Vec2>) {
        // [tenants] The businesses renting units leave with everyone else.
        if self.buildings.get(bid).is_some_and(|b| !b.units.is_empty()) {
            self.evict_tenants(bid, "its building coming down");
        }
        let tick = self.clock.tick;
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let residents = std::mem::take(&mut b.residents);
        let households = std::mem::take(&mut b.households);
        let workers = std::mem::take(&mut b.workers);
        b.visitors = 0;
        let name = b.name.clone();
        let lot = b.lot;
        let note = memory.unwrap_or_else(|| format!("{name} was demolished. Homeless again."));
        for cid in residents {
            if let Some(c) = self.citizens.get_mut(cid) {
                if c.home == Some(bid) {
                    c.home = None;
                    c.remember(tick, note.clone());
                    self.concerns.heard(cid, c, crate::concerns::Concern::Displacement, 0.8);
                    self.economy.displaced_total += 1;
                }
            }
        }
        for hid in households {
            if let Some(h) = self.households.get_mut(hid) {
                if h.home == Some(bid) {
                    h.home = None;
                }
            }
        }
        for cid in workers {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, None);
            }
        }
        let out = self.lots.get(lot).filter(|l| l.building == Some(bid)).map(|l| l.entrance()).or(at).unwrap_or(Vec2::ZERO);
        for c in self.citizens.values_mut().filter(|c| c.inside == Some(bid)) {
            c.inside = None;
            c.pos = out;
            c.prev_pos = out;
            c.activity = Activity::Idle;
            c.next_decision_tick = tick + 1;
        }
    }

    /// Tear a building down and put up its successor on the same lot: the
    /// tenants and staff leave now, the wrecking crew clears the old
    /// building, then the new one (at `tier`, `floors` high) is built.
    pub(super) fn redevelop(&mut self, bid: BuildingId, tier: Tier, floors: u8, value: f32) {
        let Some(b) = self.buildings.get(bid) else { return };
        let memory = format!("{} was torn down and rebuilt for richer tenants. The rent went up with it.", b.name);
        self.redevelop_with(bid, tier, floors, value, memory, false);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Redevelopment);
    }

    /// [land] `redevelop` with the residents' `memory` given, and for a
    /// firm rebuilding bigger, `keep_business` (no change of trade).
    pub(super) fn redevelop_with(&mut self, bid: BuildingId, tier: Tier, floors: u8, value: f32, memory: String, keep_business: bool) {
        let Some(b) = self.buildings.get(bid) else { return };
        // A site already coming down keeps its demolition; a site still
        // being built has only a storey to clear.
        let old = match (&b.construction, &b.demolition) {
            (_, Some(d)) => d.clone(),
            (Some(_), None) => {
                let mut d = Demolition::of(b, false);
                d.floors = 1;
                d.days = Demolition::days_for(1);
                d
            }
            (None, None) => Demolition::of(b, false),
        };
        // [r4-decks] Rebuilt under a raised road: no taller than its deck
        // allows over any of its plots.
        let floors = match self.deck_cap_plots(&std::iter::once(b.lot).chain(b.plot.extra_lots.iter().copied()).collect::<Vec<_>>()) {
            Some(cap) => floors.min(cap.max(1)),
            None => floors,
        };
        self.vacate(bid, Some(memory), None);
        let seed = self.fresh_seed();
        let tick = self.clock.tick;
        let b = &mut self.buildings[bid];
        b.high_land_days = 0;
        b.tier = tier;
        b.floors = floors.max(1);
        b.condition = 1.0;
        b.built_tick = tick;
        b.seed = seed;
        b.demolition = Some(old);
        b.construction = Some(Construction::new(Construction::days_for(b.floors, tier)));
        b.recompute_capacity();
        b.update_rent(value);
        // The business that reopens in it: the same one only if it paid its
        // way (decided now, as the old one closes for the works).
        if !keep_business {
            self.reopen_after_rebuild(bid);
        }
        self.events.push(SimEvent::BuildingChanged(bid));
    }

    /// A civic structure is being set down on `lot_id`. If a demolition site
    /// stands there (the take-over just started it), the structure inherits
    /// the clearing: the site building goes, the structure works from the
    /// start, and its lot shows the demolition until the rubble is gone.
    pub(super) fn take_clearing(&mut self, lot_id: LotId) -> Option<Demolition> {
        let old = self.lots.get(lot_id)?.building?;
        let mut d = self.buildings.get(old)?.demolition.clone()?;
        // The wrecking crew goes with the site; the structure hires its own.
        let crew = std::mem::take(&mut self.buildings[old].workers);
        for cid in crew {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(old)) {
                self.lay_off(cid, None);
            }
        }
        let (plot, primary) = self.buildings.remove(old).map(|b| (b.plot, b.lot)).unwrap_or_default();
        self.lots[lot_id].building = None;
        if self.lots.get(primary).is_some_and(|l| l.building == Some(old)) {
            self.lots[primary].building = None;
        }
        self.events.push(SimEvent::BuildingRemoved(old, lot_id));
        // [review] The structure takes this plot whoever had bought it; the
        // site's extra plots, owner, reservations and mansion entry are
        // released as for any building gone (they kept the dead id).
        self.drop_reservation(lot_id);
        self.estate_building_gone(old, &plot, primary);
        d.clear_lot = false;
        d.own_lot = false;
        Some(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    fn town() -> World {
        World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
    }

    fn vacant_near(w: &World, p: Vec2, lu: LandUse) -> LotId {
        w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).expect("a vacant lot")
    }

    /// A finished building on the vacant lot nearest `p`, with people
    /// living or working in it.
    fn occupied(w: &mut World, p: Vec2, lu: LandUse) -> (LotId, BuildingId) {
        let lot = vacant_near(w, p, lu);
        w.build_on(lot);
        let bid = w.lots[lot].building.unwrap();
        w.finish_construction(bid);
        let mut minds = IdleMind;
        for _ in 0..400 {
            w.tick(&mut minds);
        }
        (lot, bid)
    }

    /// Tick until `done`, at most `limit` ticks; the ticks it took.
    fn run_until(w: &mut World, limit: u64, mut done: impl FnMut(&World) -> bool) -> u64 {
        let mut minds = IdleMind;
        for t in 0..limit {
            if done(w) {
                return t;
            }
            w.tick(&mut minds);
        }
        limit
    }

    #[test]
    fn a_bulldozed_building_is_a_demolition_site_for_days_then_clear() {
        let mut w = town();
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        let zone = w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential).unwrap();
        let (_, bid) = occupied(&mut w, Vec2::new(100.0, 20.0), LandUse::Residential);
        let residents = w.buildings[bid].residents.clone();
        assert!(!residents.is_empty(), "people moved in");
        let floors = w.buildings[bid].floors;
        w.drain_events();

        // Bulldoze the zone: its parcels are gone, the building is not.
        w.remove_gesture(zone);
        w.implement();
        let b = w.buildings.get(bid).expect("it does not vanish");
        assert!(b.being_demolished(), "it is a demolition site");
        assert!(b.residents.is_empty() && b.households.is_empty() && b.homes == 0, "nobody lives in a demolition site");
        assert_eq!(b.jobs, Demolition::crew(floors), "the wrecking crew are its jobs");
        for c in &residents {
            assert!(w.citizens.get(*c).is_none_or(|c| c.home.is_none()), "residents leave at once");
        }
        let site_lot = b.lot;
        assert_eq!(w.lots[site_lot].building, Some(bid), "it holds a parcel made for the site");
        assert!(!w.zones.contains_key(w.lots[site_lot].zone));
        assert!(w.lots.values().filter(|l| l.building.is_some()).all(|l| !w.zones.contains_key(l.zone)), "only sites stand where the zone was");
        let days = b.demolition.as_ref().unwrap().days;
        assert!((1.0..=3.0).contains(&days), "1-3 days: {days}");

        // Trucks leave as it comes down; the parcel stays fenced off.
        let mut trucks = 0;
        let mut seen_progress = 0.0f32;
        // [pt-growth] The crew works 08:00-17:00: the days' work is done in
        // that many working hours from now.
        let start = w.clock.tick;
        let due = w.clock.after_work_hours(days * crate::clock::WORK_HOURS_PER_DAY);
        let ticks = run_until(&mut w, 4 * 1440, |w| {
            if let Some(d) = w.buildings.get(bid).and_then(|b| b.demolition.as_ref()) {
                assert!(d.progress >= seen_progress, "the work only goes forward");
                seen_progress = d.progress;
                trucks = trucks.max(w.traffic.cars.values().filter(|c| c.cargo.is_some_and(|c| c.from == bid)).count());
            }
            !w.buildings.contains_key(bid)
        });
        let expected = due - start;
        assert!(ticks + 120 >= expected && ticks <= expected + 120, "cleared after {ticks} ticks, expected about {expected}");
        assert!(trucks > 0, "rubble trucks were on the road");
        assert!(!w.lots.contains_key(site_lot), "the parcel made for the site goes with the rubble");
        assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::BuildingRemoved(b, _) if *b == bid)));
    }

    #[test]
    fn nothing_is_built_on_a_lot_while_its_building_comes_down() {
        let mut w = town();
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
        let (lot, bid) = occupied(&mut w, Vec2::new(100.0, 20.0), LandUse::Commercial);
        let staff = w.buildings[bid].workers.clone();
        w.demolish(bid);
        assert!(w.buildings[bid].business.is_none(), "the business goes with the staff");
        for c in &staff {
            assert!(w.citizens.get(*c).is_none_or(|c| c.work != Some(bid)), "staff are laid off at once");
        }
        let loads = Demolition::loads_for(w.buildings[bid].demolition.as_ref().unwrap().floors);
        let mut minds = IdleMind;
        let mut hauled = 0;
        while w.buildings.contains_key(bid) {
            w.build_on(lot);
            assert_eq!(w.lots[lot].building, Some(bid), "the lot cannot be developed while it is a site");
            hauled = w.buildings[bid].demolition.as_ref().unwrap().loads;
            w.tick(&mut minds);
        }
        assert_eq!(hauled, loads, "every load was hauled before the lot was handed over");
        assert!(w.lots[lot].building.is_none(), "then it is vacant");
        w.build_on(lot);
        assert!(w.lots[lot].building.is_some_and(|b| b != bid), "and can be built on again");
    }

    #[test]
    fn a_civic_take_over_works_at_once_and_shows_the_clearing_first() {
        let mut w = town();
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
        let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.area() >= transit::DEPOT_MIN_AREA).map(|(id, _)| id).next().expect("a big lot");
        w.build_on(lot);
        let works = w.lots[lot].building.unwrap();
        w.finish_construction(works);
        run_until(&mut w, 400, |_| false);
        let old_floors = w.buildings[works].floors;
        let staff = w.buildings[works].workers.clone();
        let depot = w.build_depot(lot).expect("depot placed");
        assert!(!w.buildings.contains_key(works), "the works hands its clearing to the depot");
        for c in &staff {
            assert!(w.citizens.get(*c).is_none_or(|c| c.work != Some(works)));
        }
        let d = w.buildings[depot].demolition.clone().expect("the depot inherits the clearing");
        assert!(w.buildings[depot].depot && !d.clear_lot && d.floors == old_floors, "what is shown coming down is the old works");
        assert!(w.buildings[depot].construction.is_none(), "a civic structure works from the start");
        assert_eq!(w.lots[lot].building, Some(depot));
        run_until(&mut w, 4 * 1440, |w| w.buildings[depot].demolition.is_none());
        let b = &w.buildings[depot];
        assert!(b.demolition.is_none() && b.depot && b.jobs == transit::DEPOT_JOBS, "cleared, and the depot stands as placed");
    }

    #[test]
    fn redevelopment_is_a_demolition_then_a_building_site_then_the_new_building() {
        let mut w = town();
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.land_value.fill(0.1);
        let (_, bid) = occupied(&mut w, Vec2::new(100.0, 20.0), LandUse::Residential);
        let (old_floors, old_seed) = (w.buildings[bid].floors, w.buildings[bid].seed);
        assert_eq!(w.buildings[bid].tier, Tier::Low);
        w.buildings[bid].high_land_days = REDEVELOP_DAYS - 1;
        w.land_value.fill(0.95);
        w.daily_update();
        let b = &w.buildings[bid];
        let d = b.demolition.clone().expect("the old building comes down first");
        assert!(!d.clear_lot && d.floors == old_floors && d.seed == old_seed && d.tier == Tier::Low);
        assert!(b.tier > Tier::Low && b.construction.as_ref().is_some_and(|c| c.progress == 0.0), "its successor waits");
        assert!(b.residents.is_empty() && b.homes == 0);
        // Clearing: the building site does not start or take deliveries.
        let mut minds = IdleMind;
        while w.buildings[bid].demolition.is_some() {
            let c = w.buildings[bid].construction.as_ref().unwrap();
            assert!(c.progress == 0.0 && !c.ordered, "no building work before the lot is clear");
            w.land_value.fill(0.95);
            w.tick(&mut minds);
        }
        let c = w.buildings[bid].construction.as_ref().expect("then a building site");
        assert!(c.deliveries <= 1, "parts are ordered once the lot is clear");
        assert_eq!(w.buildings[bid].jobs, Construction::crew(w.buildings[bid].floors), "the building crew replaces the wrecking crew");
        run_until(&mut w, 15 * 1440, |w| w.buildings[bid].construction.is_none());
        let b = &w.buildings[bid];
        assert!(b.construction.is_none() && b.homes > 0, "and the new building opens");
    }

    #[test]
    fn a_demolition_survives_save_and_load_and_old_saves_load() {
        let mut w = town();
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        let (lot, bid) = occupied(&mut w, Vec2::new(100.0, 20.0), LandUse::Residential);
        // A save from before demolition sites has no field for them.
        let old = w.to_ron().unwrap().replace("demolition: None,", "");
        assert!(!old.contains("demolition"));
        assert!(World::from_ron(&old).is_ok_and(|o| o.buildings.values().all(|b| b.demolition.is_none())));
        w.demolish(bid);
        run_until(&mut w, 600, |_| false);
        let before = w.buildings[bid].demolition.clone().unwrap();
        assert!(before.progress > 0.1 && before.progress < 0.9, "{}", before.progress);
        let mut back = World::from_ron(&w.to_ron().unwrap()).expect("loads");
        assert_eq!(back.buildings[bid].demolition.as_ref(), Some(&before));
        assert_eq!(back.lots[lot].building, Some(bid));
        assert!(back.buildings[bid].being_demolished() && back.buildings[bid].homes == 0);
        run_until(&mut back, 4 * 1440, |w| !w.buildings.contains_key(bid));
        assert!(!back.buildings.contains_key(bid) && back.lots[lot].building.is_none(), "it clears after loading");
    }

    #[test]
    fn a_station_over_several_buildings_clears_them_on_its_own_ground() {
        let mut w = town();
        w.economy.treasury = 500_000.0;
        w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Commercial);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.center.distance(Vec2::new(240.0, 40.0)) < 60.0).map(|(id, _)| id).collect();
        assert!(lots.len() >= 2);
        let mut shops = Vec::new();
        for l in lots {
            w.build_on(l);
            let b = w.lots[l].building.unwrap();
            w.finish_construction(b);
            shops.push(b);
        }
        let station = w.place_rail_facility(crate::rail::RailFacility::Station, Vec2::new(240.0, 40.0)).expect("station");
        let fp = w.facilities.values().find(|f| f.building == Some(station)).unwrap().footprint();
        // [dock-access] Drawn as itself at once: the shops are cleared away,
        // the station never shows as their demolition.
        assert!(w.buildings[station].demolition.is_none(), "the station stands at once on its own footprint");
        // Every building and lot agree, and nothing else stands on the
        // station's ground: the shops under it went into its clearing.
        for (id, b) in w.buildings.iter() {
            assert_eq!(w.lots[b.lot].building, Some(id));
            if id != station {
                assert!(!point_in_polygon(w.lots[b.lot].center, &fp), "a site left on the station's ground");
            }
        }
        assert!(w.buildings[station].rail.is_some());
    }
}
