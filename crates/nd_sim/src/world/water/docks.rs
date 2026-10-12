//! Docks: placing them, their lots and links, and imports by sea.

use super::*;

impl World {
    /// Where a dock of `kind` would go for a click at `p`, or why not.
    pub fn plan_port(&self, kind: PortKind, p: Vec2) -> Result<Port, String> {
        let port = Port::plan(kind, p, &self.landscape)?;
        // [ferries] A ferry terminal's boats run to towns off the map, so
        // its water must reach the edge (a lake that does not is no good).
        if kind.is_ferry() && self.landscape.water_routes_out(port.berth).is_empty() {
            return Err("ferries need water that reaches the edge of the map: build the terminal on a river or the sea".into());
        }
        let fp = port.footprint();
        // Grown buildings under it are bought out on placement; a civic
        // structure there stops it.
        if let Some(b) = self.buildings_under(&fp).into_iter().filter_map(|(id, _)| self.buildings.get(id)).find(|b| Self::is_civic_structure(b)) {
            return Err(format!("{} stands there; bulldoze it first", b.name));
        }
        if self.facilities.values().any(|f| polygons_overlap(&f.footprint(), &fp)) || self.ports.ports.values().any(|o| polygons_overlap(&o.footprint(), &fp) || o.center.distance(port.center) < kind.size().0 * 0.6) {
            return Err("too close to another dock or station".into());
        }
        Ok(port)
    }

    /// Build a shipping port or ferry terminal at the water's edge near
    /// `p`: it gets a pier, a lot and building of its own, and a street stub
    /// along its landward side unless a street is already there.
    pub fn place_port(&mut self, kind: PortKind, p: Vec2) -> Result<BuildingId, String> {
        let port = self.plan_port(kind, p)?;
        if self.economy.treasury < kind.cost() {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), kind.cost()));
        }
        for (_, lot) in self.buildings_under(&port.footprint()) {
            self.take_over_lot(lot, kind.label())?;
        }
        self.economy.debit_item(Flow::Construction, kind.cost(), Item::Named(kind.label().to_string()));
        let probe = port.front_mid() - port.seaward * rail::ROAD_GAP;
        let fused = self.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= rail::ROAD_EDGE_FUSE);
        // [dock-access] No road needed: a dock not set down by a street gets
        // a short access road of its own from its gate inland, a dead end
        // the player draws a road onto to join it to the network.
        let mut port = port;
        if !fused {
            port.stub = Some(self.dock_stub(&port));
        }
        let stub = port.stub;
        let pid = self.ports.ports.insert(port);
        let had_project = self.project.take();
        if let Some(stub) = stub {
            self.add_gesture(Gesture::Road { points: stub.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        }
        if self.project.is_some() {
            self.implement();
        } else {
            self.rebuild_from_master();
        }
        self.project = had_project;
        let Some(lot_id) = self.ports.ports[pid].lot else {
            self.ports.ports.remove(pid);
            return Err("the dock found no street to front".into());
        };
        let name = {
            const NAMES: [&str; 12] = ["Kessler", "Morrow", "Saltmarsh", "Greyhaven", "Tideway", "Ironbank", "Halcyon", "Wharfside", "Neon Bay", "Driftmoor", "Calloway", "Ashgate"];
            let n = *NAMES.choose(&mut self.rng).unwrap();
            match kind {
                PortKind::ShippingPort => format!("Port of {n}"),
                PortKind::FerryTerminal => format!("{n} Ferry"),
                PortKind::FerryTerminalLarge => format!("{n} Ferry Port"),
                // [economy]
                PortKind::MarinaSmall => format!("{n} Marina"),
                PortKind::MarinaLarge => format!("{n} Yacht Club"),
                PortKind::FishingPortSmall => format!("{n} Fish Quay"),
                PortKind::FishingPortLarge => format!("{n} Fishing Harbour"),
            }
        };
        let id = self.place_special(lot_id, name, kind.jobs(), kind.jobs() / 3, |b| b.port = Some(kind));
        // [dock-access] A dock stands on its own footprint and is drawn as
        // itself from the start: what it bought out is cleared at once,
        // never shown as the old building (in the zone's lot shape) coming down.
        self.buildings[id].demolition = None;
        // [economy] A marina or fishing port is run by an operator on the
        // city's quay (`world/waterfront.rs`).
        self.lease_dock(id, kind, false);
        self.ports.ports[pid].building = Some(id);
        // [ferries] Its towns off the map, across the edges its water reaches.
        if kind.is_ferry() {
            self.link_remote_terminals(pid);
        }
        self.ports.ports[pid].last_sailing = self.clock.tick.saturating_sub(ports::FREIGHT_HEADWAY);
        self.refresh_port_links();
        self.events.push(SimEvent::LotsChanged);
        Ok(id)
    }

    /// [dock-access] The access road for a dock: from just off the middle
    /// of its landward side, straight inland if that is clear, else turned
    /// up to 60 degrees either way, else shorter. Clear means dry ground,
    /// not too steep, off every other dock's and station's footprint and
    /// off the lots of standing buildings (all but those the dock takes
    /// over). Falls back to the shortest straight one.
    pub fn dock_stub(&self, port: &Port) -> [Vec2; 2] {
        let gate = port.front_mid() - port.seaward * ports::STUB_GATE;
        let inland = -port.seaward;
        let own = port.footprint();
        let blocked_lots: Vec<Vec<Vec2>> = self
            .buildings
            .values()
            .filter_map(|b| self.lots.get(b.lot))
            .filter(|l| !(point_in_polygon(l.center, &own) || polygons_overlap(&own, &l.polygon)))
            .map(|l| l.polygon.clone())
            .collect();
        let others = self.structure_footprints();
        let clear = |end: Vec2| {
            let n = (gate.distance(end) / 3.0).ceil().max(1.0) as usize;
            (0..=n).all(|i| {
                let q = gate.lerp(end, i as f32 / n as f32);
                !self.landscape.is_water(q) && !others.iter().any(|fp| point_in_polygon(q, fp)) && !blocked_lots.iter().any(|lp| point_in_polygon(q, lp)) && (i == 0 || !point_in_polygon(q, &own))
            }) && self.landscape.slope_over(&[gate, end]) <= crate::landscape::MAX_LOT_SLOPE * 1.5
        };
        let turn = |deg: f32| {
            let (s, c) = deg.to_radians().sin_cos();
            Vec2::new(inland.x * c - inland.y * s, inland.x * s + inland.y * c)
        };
        for len in [ports::STUB_LENGTH, ports::STUB_LENGTH * 0.7, ports::STUB_MIN] {
            for deg in [0.0f32, 25.0, -25.0, 45.0, -45.0, 60.0, -60.0] {
                let end = gate + turn(deg) * len;
                if clear(end) {
                    return [gate, end];
                }
            }
        }
        [gate, gate + inland * ports::STUB_MIN]
    }

    /// [dock-access] Pull down a dock and its pier: its ships and the goods
    /// and people waiting there go, and the zoned lots under its footprint
    /// come back from the plan. Its street stays.
    pub(in crate::world) fn bulldoze_dock(&mut self, bid: BuildingId) -> Result<String, String> {
        let pid = self.ports.ports.iter().find(|(_, p)| p.building == Some(bid)).map(|(id, _)| id).ok_or("no such dock")?;
        let name = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        self.ports.ports.remove(pid);
        self.ports.ships.retain(|_, s| s.port != bid && s.to != Some(bid));
        let stranded: Vec<Cargo> = self.ports.inbound.iter().filter(|(p, _)| *p == bid).map(|(_, c)| *c).collect();
        self.ports.inbound.retain(|(p, _)| *p != bid);
        self.ports.boarding.remove(&bid);
        self.ports.prepaid.remove(&bid);
        self.demolish(bid);
        let had_project = self.project.take();
        self.rebuild_from_master();
        self.project = had_project;
        for c in stranded {
            self.dispatch_truck(super::market::TruckOrigin::CityEdge, c, 6);
        }
        self.events.push(SimEvent::LotsChanged);
        Ok(format!("{name} bulldozed with its pier"))
    }

    /// Buildings whose lots lie under or across a footprint.
    pub(super) fn buildings_under(&self, fp: &[Vec2]) -> Vec<(BuildingId, LotId)> {
        self.buildings.iter().filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, fp) || polygons_overlap(fp, &l.polygon))).map(|(id, b)| (id, b.lot)).collect()
    }

    /// The lot a dock stands on: its footprint, fronting the nearest street.
    pub(in crate::world) fn port_lot(&self, pid: PortId) -> Option<Lot> {
        let p = &self.ports.ports[pid];
        let corners = p.corners();
        let probe = p.front_mid() - p.seaward * rail::ROAD_GAP;
        let (front_segment, front_t, car_point) = self.roads.nearest_street(probe)?;
        Some(Lot {
            zone: ZoneId::default(),
            land_use: p.kind.land_use(),
            polygon: corners.to_vec(),
            corners,
            front_edge: [corners[0], corners[1]],
            center: p.center,
            front_dir: -p.seaward,
            front_segment,
            front_t,
            access_node: self.roads.nearest_endpoint(front_segment, front_t),
            building: None,
            landlocked: false,
            footfall_today: 0,
            footfall_hour: 0,
            car_segment: Some(front_segment),
            car_point,
            park: None,
            shore: Default::default(),
        })
    }

    /// Recompute whether each dock's street reaches the city and ships can
    /// reach its berth from off the map. Drops docks whose building is gone.
    pub(in crate::world) fn refresh_port_links(&mut self) {
        let gone: Vec<PortId> = self.ports.ports.iter().filter(|(_, p)| p.building.is_some_and(|b| !self.buildings.contains_key(b))).map(|(id, _)| id).collect();
        for id in gone {
            self.ports.ports.remove(id);
        }
        let (walk_comps, drive_comps) = self.network_components();
        let ids: Vec<PortId> = self.ports.ports.keys().collect();
        for id in ids {
            let (lot, berth) = (self.ports.ports[id].lot, self.ports.ports[id].berth);
            let node = lot.and_then(|l| self.lots.get(l)).map(|l| l.access_node);
            let road = node.and_then(|n| self.drive_component(n)).is_some_and(|c| drive_comps.contains(&c));
            let walk = node.and_then(|n| self.road_component(n)).is_some_and(|c| walk_comps.contains(&c));
            let out = self.landscape.water_route(berth, None).is_some();
            let p = &mut self.ports.ports[id];
            p.road_connected = road;
            p.walk_connected = walk;
            p.reaches_elsewhere = out;
            // [ferries] A terminal from a save before remote towns gets its own.
            if p.kind.is_ferry() && p.remote.is_empty() && out {
                self.link_remote_terminals(id);
            }
        }
        // Goods waiting at a dock that no longer works go by road.
        let working: Vec<BuildingId> = self.ports.working(PortKind::ShippingPort);
        let stranded: Vec<Cargo> = {
            let (keep, lost): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ports.inbound).into_iter().partition(|(b, _)| working.contains(b));
            self.ports.inbound = keep;
            lost.into_iter().map(|(_, c)| c).collect()
        };
        for c in stranded {
            self.dispatch_truck(super::market::TruckOrigin::CityEdge, c, 6);
        }
    }

    /// The working shipping port nearest a building, for imports by sea.
    pub(in crate::world) fn nearest_port(&self, to: BuildingId) -> Option<BuildingId> {
        self.nearest_by_entrance(to, self.ports.working(PortKind::ShippingPort))
    }

    /// [review] An import cheaper than the road price (by sea, rail, quay or
    /// hub): the difference goes back to the buyer, who paid the road price
    /// when ordering, and off the day's import bill.
    pub(in crate::world) fn refund_import(&mut self, buyer: BuildingId, refund: f32) {
        if let Some(bz) = self.buildings.get_mut(buyer).and_then(|b| b.business.as_mut()) {
            bz.money += refund;
            bz.spend_today -= refund;
        }
        self.economy.market.imports_today -= refund;
    }

    /// Order goods by sea: the buyer pays the cheaper sea price and the load
    /// waits at the port for the next freighter. False when there is no port.
    pub(in crate::world) fn import_by_sea(&mut self, to: BuildingId, cargo: Cargo) -> bool {
        let Some(port) = self.nearest_port(to) else { return false };
        let refund = cargo.price * (1.0 - ports::SHIP_IMPORT_FACTOR);
        self.refund_import(to, refund);
        self.ports.inbound.push((port, Cargo { from: port, price: cargo.price * ports::SHIP_IMPORT_FACTOR, ..cargo }));
        true
    }

    /// Export income multiplier from docks.
    pub(in crate::world) fn sea_export_bonus(&self) -> f32 {
        if self.ports.working(PortKind::ShippingPort).is_empty() { 1.0 } else { ports::SHIP_EXPORT_BONUS }
    }

    /// Daily books for the docks: upkeep and fresh counters.
    pub(in crate::world) fn daily_ports(&mut self) {
        let docks: Vec<(Option<BuildingId>, PortKind, f32)> = self.ports.ports.values().map(|p| (p.building, p.kind, p.kind.upkeep_per_day())).collect();
        for (b, kind, upkeep) in docks {
            let item = b.map_or_else(|| Item::Named(kind.label().to_string()), Item::Building);
            self.economy.debit_item(Flow::TransitUpkeep, upkeep, item);
        }
        let s = &mut self.ports.stats;
        s.freight_landed_today = 0;
        s.ferry_riders_today = 0;
        s.ferry_fares_today = 0.0;
        s.remote_sailings_today = 0;
        s.visitors_in_today = 0;
        s.visitors_out_today = 0;
        s.commuters_in_today = 0;
        s.commuters_out_today = 0;
        s.day_trips_today = 0;
        s.day_trippers_back_today = 0;
        self.ports.commuters_today.clear();
        self.ports.trips_today.clear();
        self.ports.line_riders.clear();
    }
}
