//! Ships: sailing, calling and ferry riders.

use super::*;

impl World {
    /// Sail ships, dispatch new ones on their headways, land freight and
    /// ferry passengers.
    pub(in crate::world) fn step_ships(&mut self) {
        if self.ports.ports.is_empty() && self.ports.ships.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        if tick % 10 == 3 {
            self.dispatch_ships();
        }
        let ids: Vec<crate::ports::ShipId> = self.ports.ships.keys().collect();
        let mut docked = Vec::new();
        let mut done = Vec::new();
        let mut returned = Vec::new();
        for id in ids {
            let ship = &mut self.ports.ships[id];
            ship.prev_s = ship.s;
            if ship.dwell > 0 {
                ship.dwell -= 1;
                continue;
            }
            let total = crate::geom::path_length(&ship.path);
            if ship.home {
                // [ferries] Unloaded at its own pier after a run: done.
                done.push(id);
                continue;
            }
            // [ferries] Out of sight beyond the map edge the run is hurried
            // along, so the next town is hours away, not half a day.
            let off_map = ship.remote.is_some() && {
                let p = ship.pose().0;
                p.x.abs().max(p.y.abs()) > crate::landscape::MAP_EDGE + 400.0
            };
            ship.s += ship.speed * if off_map { ports::OFF_MAP_SPEEDUP } else { 1.0 };
            if ship.remote.is_some() && ship.docked && ship.s >= total {
                // Back from the town off the map: tie up and let them off.
                ship.s = total;
                ship.home = true;
                ship.dwell = ports::FERRY_UNLOAD;
                returned.push(id);
                continue;
            }
            if !ship.docked && ship.s >= ship.berth_s {
                ship.s = ship.berth_s;
                ship.docked = true;
                ship.dwell = match ship.kind {
                    ShipKind::Freighter => ports::FREIGHTER_DWELL,
                    ShipKind::Ferry => ports::FERRY_DWELL,
                    // [economy] out on the fishing ground, or at anchor
                    ShipKind::FishingBoat | ShipKind::Yacht | ShipKind::Sailboat => crate::world::waterfront::boat_dwell(ship.kind),
                };
                docked.push(id);
            } else if ship.s >= total {
                done.push(id);
            }
        }
        for id in docked {
            self.ship_docked(id);
        }
        for id in returned {
            let (port, town) = {
                let s = &self.ports.ships[id];
                (s.port, s.remote.unwrap_or(0))
            };
            self.ferry_returned(port, town);
        }
        for id in done {
            self.ports.ships.remove(id);
        }
        self.ports.stats.ships_out = self.ports.ships.len() as u32;
    }

    pub(super) fn dispatch_ships(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let ports_now: Vec<(PortId, Port)> = self.ports.ports.iter().map(|(id, p)| (id, p.clone())).collect();
        self.ports.stats.ports = ports_now.iter().filter(|(_, p)| p.kind == PortKind::ShippingPort).count() as u32;
        self.ports.stats.terminals = ports_now.iter().filter(|(_, p)| p.kind == PortKind::FerryTerminal).count() as u32;
        self.ports.stats.waiting_cargo = self.ports.inbound.len() as u32;
        // [economy] Marina and fishing boats do not count against the ships.
        let big_ships = self.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Freighter | ShipKind::Ferry)).count();
        for (pid, port) in &ports_now {
            if !matches!(port.kind, PortKind::ShippingPort | PortKind::FerryTerminal) {
                continue;
            }
            // [dock-access] A ferry terminal's boats run on the water alone.
            if !port.sails() || big_ships >= ports::MAX_SHIPS {
                continue;
            }
            let Some(b) = port.building else { continue };
            if self.buildings.get(b).is_none_or(|x| x.is_derelict()) {
                continue;
            }
            // [ferries] A terminal runs two boats, one away while the next
            // sails, but never two at its pier at once.
            let mine: Vec<&Ship> = self.ports.ships.values().filter(|s| s.port == b).collect();
            let at_pier = mine.iter().any(|s| s.pose().0.distance(port.berth) < 40.0);
            let busy = match port.kind {
                PortKind::ShippingPort => !mine.is_empty(),
                PortKind::FerryTerminal => at_pier || mine.len() >= ports::FERRIES_PER_TERMINAL,
                _ => true,
            };
            if busy {
                continue;
            }
            match port.kind {
                PortKind::ShippingPort => {
                    let waiting = self.ports.inbound.iter().any(|(p, _)| *p == b);
                    let due = tick.saturating_sub(port.last_sailing) >= ports::FREIGHT_HEADWAY && (waiting || tick.saturating_sub(port.last_sailing) >= ports::FREIGHT_HEADWAY * 2);
                    if !due {
                        continue;
                    }
                    let Some(out) = self.landscape.water_route(port.berth, None) else { continue };
                    // In from elsewhere, to the berth, and back out.
                    let mut path: Vec<Vec2> = out.iter().rev().copied().collect();
                    let berth_s = crate::geom::path_length(&path);
                    path.extend(out.iter().skip(1).copied());
                    let cargo: Vec<Cargo> = {
                        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ports.inbound).into_iter().partition(|(p, _)| *p == b);
                        self.ports.inbound = rest;
                        mine.into_iter().map(|(_, c)| c).collect()
                    };
                    let speed = self.config.metres_per_tick(ports::FREIGHTER_SPEED);
                    let color = self.rng.random_range(0..6);
                    // [berthing] Broadside at the pier head; the crane lands
                    // its imports and loads the city's exports.
                    let from = out.get(1).copied().unwrap_or(port.berth + port.seaward);
                    let quay = Some(port.quay_for(from));
                    let boxes_in = cargo.iter().map(|c| ports::boxes_for(c.amount)).sum::<u32>().min(ports::MAX_BOXES);
                    let boxes_out = ((self.ports.outbound / ports::CONTAINER_UNITS) as u32).min(ports::MAX_BOXES);
                    self.ports.outbound = (self.ports.outbound - boxes_out as f32 * ports::CONTAINER_UNITS).max(0.0);
                    self.ports.ships.insert(Ship { kind: ShipKind::Freighter, path, s: 0.0, prev_s: 0.0, speed, dwell: 0, port: b, to: None, cargo, passengers: 0, docked: false, berth_s, color, remote: None, home: false, quay, boxes_in, boxes_out });
                    self.ports.ports[*pid].last_sailing = tick;
                }
                // [economy] Boats are sent by `World::step_boats`.
                PortKind::MarinaSmall | PortKind::MarinaLarge | PortKind::FishingPortSmall | PortKind::FishingPortLarge => {}
                PortKind::FerryTerminal => {
                    if !(ports::FERRY_SERVICE_START..ports::FERRY_SERVICE_END).contains(&minute) || tick.saturating_sub(port.last_sailing) < ports::FERRY_HEADWAY {
                        continue;
                    }
                    // [ferries] Take turns: a run to a town off the map,
                    // then one to the next terminal in town, and so on;
                    // with only one kind, that kind every time.
                    let others: Vec<(BuildingId, Vec2)> = ports_now.iter().filter(|(oid, o)| oid != pid && o.kind == PortKind::FerryTerminal && o.sails()).filter_map(|(_, o)| o.building.map(|ob| (ob, o.berth))).collect();
                    let turn = port.sailings as usize;
                    let remote_turn = !port.remote.is_empty() && (others.is_empty() || turn % 2 == 0);
                    let mut chosen: Option<(Option<BuildingId>, Option<u8>, Vec<Vec2>)> = None;
                    if remote_turn {
                        let k = (turn / 2) % port.remote.len();
                        let edge = port.remote[k].edge;
                        chosen = self.landscape.water_routes_out(port.berth).into_iter().find(|(e, _)| *e == edge).map(|(_, r)| (None, Some(k as u8), r));
                    }
                    if chosen.is_none() {
                        let start = turn / 2;
                        for k in 0..others.len() {
                            let (ob, ob_berth) = others[(start + k) % others.len()];
                            if let Some(route) = self.landscape.water_route(port.berth, Some(ob_berth)) {
                                chosen = Some((Some(ob), None, route));
                                break;
                            }
                        }
                    }
                    let Some((to, remote, route)) = chosen else { continue };
                    let berth_s = crate::geom::path_length(&route);
                    let mut path = route.clone();
                    path.extend(route.iter().rev().skip(1).copied());
                    let speed = self.config.metres_per_tick(ports::FERRY_SPEED);
                    // Locals hopping across town, and on a run off the map
                    // whoever is waiting at the pier to leave.
                    // [dock-access] Nobody can get to or away from a pier
                    // whose terminal is off the roads: those boats sail empty.
                    let reachable = |x: BuildingId| ports_now.iter().any(|(_, o)| o.building == Some(x) && o.works());
                    let locals = if reachable(b) && to.is_none_or(reachable) { self.ferry_riders(b, to) } else { 0 };
                    // [review] Whoever waits on the pier boards first; the
                    // day-trippers among them paid both legs at the terminal
                    // (`board_day_trip`) and pay nothing more. Those the boat
                    // cannot take wait for the next run (they vanished).
                    let (mut out, mut out_prepaid) = (0, 0);
                    if remote.is_some() {
                        let leaving = self.ports.boarding.remove(&b).unwrap_or(0);
                        let prepaid = self.ports.prepaid.remove(&b).unwrap_or(0).min(leaving);
                        out = leaving.min(ports::FERRY_CAPACITY);
                        out_prepaid = prepaid.min(out);
                        if leaving > out {
                            self.ports.boarding.insert(b, leaving - out);
                        }
                        if prepaid > out_prepaid {
                            self.ports.prepaid.insert(b, prepaid - out_prepaid);
                        }
                        self.ports.stats.remote_sailings_today += 1;
                    }
                    let locals = locals.min(ports::FERRY_CAPACITY - out);
                    let riders = out + locals;
                    let fares = (riders - out_prepaid) as f32 * ports::FERRY_FARE;
                    // Ferry fares are public transport income, booked with the bus fares.
                    self.economy.credit_item(Flow::BusFares, fares, Item::Building(b));
                    self.ports.stats.ferry_riders_today += riders;
                    self.ports.stats.ferry_fares_today += fares;
                    self.ports.count_line_riders(b, to, remote, riders);
                    self.ports.ships.insert(Ship { kind: ShipKind::Ferry, path, s: 0.0, prev_s: 0.0, speed, dwell: ports::FERRY_DWELL, port: b, to, cargo: Vec::new(), passengers: riders, docked: false, berth_s, color: 0, remote, home: false, quay: None, boxes_in: 0, boxes_out: 0 });
                    self.ports.ports[*pid].last_sailing = tick;
                    self.ports.ports[*pid].sailings += 1;
                }
            }
        }
    }

    /// People living near either end of a sailing who take the ferry.
    pub(super) fn ferry_riders(&mut self, from: BuildingId, to: Option<BuildingId>) -> u32 {
        let ends: Vec<Vec2> = [Some(from), to].into_iter().flatten().filter_map(|b| self.building_entrance(b)).collect();
        let near = self.citizens.values().filter(|c| c.home.and_then(|h| self.building_entrance(h)).is_some_and(|p| ends.iter().any(|e| e.distance(p) < 500.0))).count() as u32;
        // Out to elsewhere draws a few more locals (visitors, commuters
        // and day trippers are counted as they board).
        let base = if to.is_none() { 4 } else { 2 };
        (near / 12 + base + self.rng.random_range(0..6)).min(ports::FERRY_CAPACITY)
    }

    pub(super) fn ship_docked(&mut self, id: crate::ports::ShipId) {
        let Some(ship) = self.ports.ships.get_mut(id) else { return };
        if ship.kind == ShipKind::Freighter {
            let port = ship.port;
            let cargo = std::mem::take(&mut ship.cargo);
            self.ports.stats.freight_landed_today += cargo.len() as u32;
            for c in cargo {
                // Into a cargo hub on the quay, else a lorry.
                self.unload_freight(port, c);
            }
        }
    }
}
