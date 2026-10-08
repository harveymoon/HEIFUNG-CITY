//! Cargo hubs, the systems (`crate::cargo_hub` holds the data and rules):
//! placing one, routing the imports of the businesses around it through
//! it (`import_via_hub`, called from `World::import`), consolidated semis
//! from the edge, freight yards and ports unloading into a hub beside
//! them, the hub's own lorries delivering on and parking in its bays, and
//! exports leaving in full semis.

use super::market::TruckOrigin;
use super::*;
use crate::cargo_hub::{self as hub, HubLoad, HubState, Via};
use crate::ports::PortKind;
use crate::services::Placeable;
use crate::vehicle::Errand;

impl World {
    /// Is `b` a working cargo hub?
    pub fn cargo_hub_works(&self, b: BuildingId) -> bool {
        self.cargo_hubs.hubs.contains_key(&b) && self.buildings.get(b).is_some_and(|bld| bld.operating() && !bld.is_derelict() && bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::CargoHub && !z.closed))
    }

    /// Place a cargo hub on a lot with a street (Buildings tool, Rail).
    pub fn build_cargo_hub(&mut self, lot_id: LotId) -> Result<BuildingId, String> {
        if let Some(problem) = self.placement_problem(Placeable::CargoHub, lot_id) {
            return Err(problem);
        }
        self.take_over_lot(lot_id, "cargo hub")?;
        self.economy.debit(Flow::Construction, hub::HUB_COST);
        let n = self.cargo_hubs.hubs.len() + 1;
        let id = self.place_special(lot_id, format!("Cargo Hub {n}"), hub::HUB_JOBS, 0, |b| {
            b.floors = 1;
            b.business = Some(Business::new(BusinessKind::CargoHub, hub::HUB_JOBS));
        });
        self.cargo_hubs.hubs.insert(id, HubState::default());
        Ok(id)
    }

    /// The working hub nearest a buyer, within reach.
    pub(super) fn hub_for(&self, buyer: BuildingId) -> Option<BuildingId> {
        let p = self.building_entrance(buyer)?;
        self.cargo_hubs
            .hubs
            .keys()
            .copied()
            .filter(|h| *h != buyer && self.cargo_hub_works(*h))
            .filter_map(|h| self.building_entrance(h).map(|q| (h, q.distance(p))))
            .filter(|(_, d)| *d <= hub::HUB_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(h, _)| h)
    }

    /// An import for `cargo.to` through `hub`: by sea when a shipping port
    /// works, else by rail when a freight yard does (the cheaper ways, as
    /// without a hub), else in a consolidated semi by road. The buyer pays
    /// the cheaper price; the load is addressed to the hub and goes on from
    /// there in the hub's own lorry.
    pub(super) fn import_via_hub(&mut self, hub_id: BuildingId, cargo: Cargo) {
        let tick = self.clock.tick;
        let port = self.nearest_port(hub_id);
        let yard = if port.is_none() { self.nearest_yard(hub_id) } else { None };
        let (via, factor) = match (port, yard) {
            (Some(_), _) => (Via::Sea, crate::ports::SHIP_IMPORT_FACTOR),
            (None, Some(_)) => (Via::Rail, rail::RAIL_IMPORT_FACTOR),
            _ => (Via::Road, hub::ROAD_IMPORT_FACTOR),
        };
        let refund = cargo.price * (1.0 - factor);
        if let Some(bz) = self.buildings.get_mut(cargo.to).and_then(|b| b.business.as_mut()) {
            bz.money += refund;
            bz.spend_today -= refund;
        }
        self.economy.market.imports_today -= refund;
        let price = cargo.price * factor;
        let load = HubLoad { buyer: cargo.to, resource: cargo.resource, amount: cargo.amount, price, since: tick, via };
        let to_hub = Cargo { to: hub_id, price, ..cargo };
        let state = self.cargo_hubs.hubs.entry(hub_id).or_default();
        match (via, port, yard) {
            (Via::Sea, Some(p), _) => {
                state.inbound.push(load);
                self.ports.inbound.push((p, Cargo { from: p, ..to_hub }));
            }
            (Via::Rail, _, Some(y)) => {
                state.inbound.push(load);
                self.rail.orders.push(rail::RailOrder { cargo: Cargo { from: y, ..to_hub }, since: tick });
            }
            _ => state.waiting.push(load),
        }
    }

    /// Goods arriving for a hub (`World::deliver`): an inbound rail or sea
    /// load, or the token a consolidated semi carries for its trunk. They go
    /// into the shed for the local lorries. False when `cargo` is not for a
    /// hub.
    pub(super) fn hub_receive(&mut self, cargo: &Cargo) -> bool {
        let Some(state) = self.cargo_hubs.hubs.get_mut(&cargo.to) else { return false };
        let s = &mut self.cargo_hubs.stats;
        if cargo.amount <= 0.0 {
            // The trunk semi is here.
            let trunk = std::mem::take(&mut state.trunk);
            state.trunk_car = None;
            for l in &trunk {
                s.received_today += 1;
                s.received_units_today += l.amount;
                s.received_total += 1;
            }
            state.ready.extend(trunk);
            return true;
        }
        match state.take_inbound(cargo.resource, cargo.amount) {
            Some(l) => {
                s.received_today += 1;
                s.received_units_today += l.amount;
                s.received_total += 1;
                state.ready.push(l);
            }
            // A load nobody here ordered (should not happen): into the shed
            // with no buyer, so it is not lost; it is exported on.
            None => state.outbound += cargo.amount,
        }
        true
    }

    /// A freight yard or port unloading `cargo`: straight into a hub on its
    /// siding or quay (no lorry), else onto a lorry as before.
    pub(super) fn unload_freight(&mut self, at: BuildingId, cargo: Cargo) {
        let beside = self.cargo_hubs.hubs.contains_key(&cargo.to)
            && self.cargo_hub_works(cargo.to)
            && self.building_entrance(at).zip(self.building_entrance(cargo.to)).is_some_and(|(a, b)| a.distance(b) <= hub::SIDING_REACH);
        if beside {
            self.deliver(cargo);
        } else {
            self.dispatch_truck(TruckOrigin::Building(at), cargo, 6);
        }
    }

    /// Exports leaving town this hour: through a hub they earn a little
    /// more and gather for a semi out.
    pub(super) fn hub_export_bonus(&self) -> f32 {
        if self.cargo_hubs.hubs.keys().any(|h| self.cargo_hub_works(*h)) { hub::EXPORT_BONUS } else { 1.0 }
    }

    pub(super) fn hub_note_exports(&mut self, units: f32) {
        let first = self.cargo_hubs.hubs.keys().copied().find(|h| self.cargo_hub_works(*h));
        if let Some(h) = first {
            self.cargo_hubs.hubs.get_mut(&h).unwrap().outbound += units;
        }
    }

    // ── the tick ───────────────────────────────────────────────────────────

    pub(super) fn step_cargo_hubs(&mut self) {
        if self.cargo_hubs.hubs.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            let s = &mut self.cargo_hubs.stats;
            s.long_haul_saved_yesterday = std::mem::take(&mut s.long_haul_saved_today);
            s.received_today = 0;
            s.received_units_today = 0.0;
            s.forwarded_today = 0;
            s.trunks_in_today = 0;
            s.trunks_out_today = 0;
            s.fees_today = 0.0;
            let n = self.cargo_hubs.hubs.keys().filter(|h| self.cargo_hub_works(**h)).count() as f32;
            self.economy.debit(Flow::RailUpkeep, hub::HUB_UPKEEP_PER_DAY * n);
        }
        let ids: Vec<BuildingId> = self.cargo_hubs.hubs.keys().copied().collect();
        for h in ids {
            if !self.buildings.contains_key(h) || !self.cargo_hub_works(h) {
                if !self.buildings.contains_key(h) || self.buildings[h].business.as_ref().is_none_or(|z| z.closed || z.kind != BusinessKind::CargoHub) || self.buildings[h].is_derelict() {
                    self.close_hub(h);
                }
                continue;
            }
            self.step_trunk(h, tick);
            if tick % 3 == 1 {
                self.forward_loads(h);
            }
            if tick % 30 == 11 {
                self.send_exports(h);
            }
        }
    }

    /// The consolidated semi: the one on its way (gone from the road with
    /// road works: its loads count as arrived), or the next one when due.
    fn step_trunk(&mut self, h: BuildingId, tick: u64) {
        let state = &self.cargo_hubs.hubs[&h];
        if let Some(car) = state.trunk_car {
            let alive = self.traffic.cars.get(car).is_some_and(|c| c.cargo.is_some_and(|x| x.to == h && x.amount <= 0.0));
            if !alive {
                self.hub_receive(&Cargo { from: BuildingId::default(), to: h, resource: Resource::Goods, amount: 0.0, price: 0.0 });
            }
            return;
        }
        if !state.trunk_due(tick) {
            return;
        }
        let loads = self.cargo_hubs.hubs.get_mut(&h).unwrap().fill_trunk();
        let n = loads.len() as u32;
        self.cargo_hubs.hubs.get_mut(&h).unwrap().trunk = loads;
        let token = Cargo { from: BuildingId::default(), to: h, resource: Resource::Goods, amount: 0.0, price: 0.0 };
        let route = self.truck_stop(h).and_then(|(s, p)| self.route_from_edge(s, p));
        let s = &mut self.cargo_hubs.stats;
        s.trunks_in_today += 1;
        s.long_haul_saved_today += n.saturating_sub(1);
        s.long_haul_saved_total += n.saturating_sub(1);
        match route {
            Some((route, s0, s1)) if self.truck_count() < crate::market::MAX_TRUCKS => {
                let car = self.traffic.spawn_car(CitizenId::default(), Some(token), route, s0, s1, 6);
                let mut rng = crate::vehicle::model_rng(tick, slotmap::Key::data(&car).as_ffi());
                self.traffic.cars[car].model = crate::vehicle::VehicleModel::for_cargo(Resource::Goods, None, false, true, &mut rng);
                self.cargo_hubs.hubs.get_mut(&h).unwrap().trunk_car = Some(car);
            }
            // No road in from the edge (or the roads are full of lorries):
            // the semi is as good as here.
            _ => {
                self.hub_receive(&token);
            }
        }
    }

    /// Loads in the shed go on to their buyers in the hub's own lorries
    /// (one of those parked in its bays when there is one).
    fn forward_loads(&mut self, h: BuildingId) {
        let n = self.cargo_hubs.hubs[&h].ready.len().min(hub::FORWARD_PER_ROUND);
        if n == 0 {
            return;
        }
        let loads: Vec<HubLoad> = self.cargo_hubs.hubs.get_mut(&h).unwrap().ready.drain(..n).collect();
        for l in loads {
            if !self.buildings.contains_key(l.buyer) {
                // The buyer went: its goods are exported on.
                self.cargo_hubs.hubs.get_mut(&h).unwrap().outbound += l.amount;
                continue;
            }
            self.unpark_truck(h);
            self.dispatch_truck(TruckOrigin::Building(h), Cargo { from: h, to: l.buyer, resource: l.resource, amount: l.amount, price: l.price }, 6);
            let fee = l.price * hub::HANDLING_FEE;
            if let Some(z) = self.buildings.get_mut(h).and_then(|b| b.business.as_mut()) {
                z.money += fee;
                z.revenue_today += fee;
                z.deals += 1;
            }
            let s = &mut self.cargo_hubs.stats;
            s.forwarded_today += 1;
            s.forwarded_total += 1;
            s.fees_today += fee;
        }
    }

    /// A semi's worth of exports gathered: it leaves for the map edge.
    fn send_exports(&mut self, h: BuildingId) {
        if self.cargo_hubs.hubs[&h].outbound < hub::EXPORT_TRUNK {
            return;
        }
        let Some((seg, pt)) = self.truck_stop(h) else { return };
        let Some((start, s0, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { return };
        let Some((route, s0, s1)) = self.edge_exits(pt).into_iter().find_map(|(goal, s_end)| self.traffic.net.route_lanes(start, s0, goal, s_end)) else { return };
        self.cargo_hubs.hubs.get_mut(&h).unwrap().outbound -= hub::EXPORT_TRUNK;
        self.cargo_hubs.stats.trunks_out_today += 1;
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 6);
        let mut rng = crate::vehicle::model_rng(self.clock.tick, slotmap::Key::data(&car).as_ffi());
        let c = &mut self.traffic.cars[car];
        c.model = crate::vehicle::VehicleModel::for_cargo(Resource::Goods, None, false, true, &mut rng);
        c.errand = Errand::Leave;
    }

    /// A hub that closed or went: whatever it held or expected is sent on
    /// to the buyers from where it is (the shed, the yard, the port, the
    /// map edge). Nothing is lost.
    fn close_hub(&mut self, h: BuildingId) {
        let Some(state) = self.cargo_hubs.hubs.remove(&h) else { return };
        let here = self.buildings.contains_key(h);
        let origin = if here { TruckOrigin::Building(h) } else { TruckOrigin::CityEdge };
        for l in state.ready.into_iter().chain(state.waiting).chain(state.trunk) {
            if self.buildings.contains_key(l.buyer) {
                self.dispatch_truck(origin, Cargo { from: if here { h } else { BuildingId::default() }, to: l.buyer, resource: l.resource, amount: l.amount, price: l.price }, 6);
            }
        }
        // Rail and sea loads addressed to the hub are readdressed to their buyers.
        for l in state.inbound {
            let retarget = |c: &mut Cargo| {
                if c.to == h && c.resource == l.resource && (c.amount - l.amount).abs() < 0.01 {
                    c.to = l.buyer;
                    true
                } else {
                    false
                }
            };
            let mut done = false;
            for (_, c) in self.ports.inbound.iter_mut() {
                if !done && retarget(c) {
                    done = true;
                }
            }
            for o in self.rail.orders.iter_mut() {
                if !done && retarget(&mut o.cargo) {
                    done = true;
                }
            }
            for ship in self.ports.ships.values_mut() {
                for c in ship.cargo.iter_mut() {
                    if !done && retarget(c) {
                        done = true;
                    }
                }
            }
            for t in self.rail.trains.values_mut() {
                for c in t.cargo.iter_mut() {
                    if !done && retarget(c) {
                        done = true;
                    }
                }
            }
        }
    }

    /// Working hubs near a point (for the freight yards and ports that are
    /// beside one).
    pub fn hubs_near(&self, p: Vec2, reach: f32) -> Vec<BuildingId> {
        self.cargo_hubs.hubs.keys().copied().filter(|h| self.cargo_hub_works(*h) && self.building_entrance(*h).is_some_and(|q| q.distance(p) <= reach)).collect()
    }

    /// Ports that work, for the hub inspector.
    pub fn hub_links(&self, h: BuildingId) -> (Option<BuildingId>, Option<BuildingId>) {
        let Some(p) = self.building_entrance(h) else { return (None, None) };
        let near = |b: &BuildingId| self.building_entrance(*b).is_some_and(|q| q.distance(p) <= hub::SIDING_REACH);
        let yard = self.yards.iter().copied().find(near);
        let port = self.ports.working(PortKind::ShippingPort).into_iter().find(near);
        (yard, port)
    }
}
