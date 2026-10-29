//! Ships: sailing, calling and ferry riders.

use super::*;

use crate::ports::FerryEstimate;

/// [r4-flags-sim] A ship coming in holds this far short of a berth another
/// ship is lying at, and the room it wants between hulls there.
const BERTH_HOLD: f32 = 140.0;
const BERTH_CLEAR: f32 = 6.0;
/// [r4-flags-sim] How far to the right of a route ferries and freighters
/// keep (`keep_to_starboard`), eased over `STARBOARD_EASE` from each end,
/// with `STARBOARD_BEAM` of water beyond; points every `STARBOARD_STEP`.
const STARBOARD_FERRY: f32 = 22.0;
const STARBOARD_FREIGHTER: f32 = 30.0;
const STARBOARD_EASE: f32 = 180.0;
const STARBOARD_BEAM: f32 = 10.0;
const STARBOARD_STEP: f32 = 60.0;

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
        // [r4-flags-sim] Where every big ship lies now, so one coming in
        // to a berth another is lying at (or about to leave) holds off
        // until it is clear instead of sailing into it (Harvey: "boats are
        // overlapping, there should be a queue").
        let lying: Vec<(crate::ports::ShipId, Vec2, f32)> = self.ports.ships.iter().filter(|(_, s)| s.kind.is_big()).map(|(id, s)| (id, s.pose().0, s.length())).collect();
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
                self.landscape.beyond_map_edge(p, 400.0)
            };
            if ship.kind.is_big() {
                // The berth it is making for: the far one, then home.
                let goal_s = if ship.docked { total } else { ship.berth_s };
                let (goal, len) = (ship.pose_at(goal_s).0, ship.length());
                let to_go = goal_s - ship.s;
                let busy = lying.iter().any(|&(o, p, l)| o != id && p.distance(goal) < 0.5 * (len + l) + BERTH_CLEAR);
                if busy && to_go > 0.0 && to_go <= BERTH_HOLD + ship.speed {
                    continue;
                }
            }
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
                    ShipKind::Ferry | ShipKind::CarFerry => ports::FERRY_DWELL,
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
        self.ports.stats.terminals = ports_now.iter().filter(|(_, p)| p.kind.is_ferry()).count() as u32;
        self.ports.stats.waiting_cargo = self.ports.inbound.len() as u32;
        // [economy] Marina and fishing boats do not count against the ships.
        let big_ships = self.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Freighter | ShipKind::Ferry | ShipKind::CarFerry)).count();
        for (pid, port) in &ports_now {
            if !matches!(port.kind, PortKind::ShippingPort | PortKind::FerryTerminal | PortKind::FerryTerminalLarge) {
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
            // [r4-flags-sim] Any big ship at the pier: a visitor from
            // another terminal lies there too.
            let at_pier = self.ports.ships.values().any(|s| s.kind.is_big() && s.pose().0.distance(port.berth) < 40.0);
            let busy = match port.kind {
                PortKind::ShippingPort => !mine.is_empty(),
                PortKind::FerryTerminal | PortKind::FerryTerminalLarge => at_pier || mine.len() >= ports::FERRIES_PER_TERMINAL,
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
                    let inbound: Vec<Vec2> = out.iter().rev().copied().collect();
                    let mut path = self.keep_to_starboard(&inbound, STARBOARD_FREIGHTER);
                    let berth_s = crate::geom::path_length(&path);
                    path.extend(self.keep_to_starboard(&out, STARBOARD_FREIGHTER).into_iter().skip(1));
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
                    self.ports.ships.insert(Ship { kind: ShipKind::Freighter, path, s: 0.0, prev_s: 0.0, speed, dwell: 0, port: b, to: None, cargo, passengers: 0, docked: false, berth_s, color, remote: None, home: false, quay, boxes_in, boxes_out, cars: 0 });
                    self.ports.ports[*pid].last_sailing = tick;
                }
                // [economy] Boats are sent by `World::step_boats`.
                PortKind::MarinaSmall | PortKind::MarinaLarge | PortKind::FishingPortSmall | PortKind::FishingPortLarge => {}
                PortKind::FerryTerminal | PortKind::FerryTerminalLarge => {
                    // [r4-timetable] The terminal's own timetable: its hours
                    // and the headway of the part of the day it is in.
                    if port.ferry_headway_at(minute).is_none_or(|h| tick.saturating_sub(port.last_sailing) < h) {
                        continue;
                    }
                    // [ferries] Take turns: a run to a town off the map,
                    // then one to the next terminal in town, and so on;
                    // with only one kind, that kind every time.
                    let others: Vec<(BuildingId, Vec2)> = ports_now.iter().filter(|(oid, o)| oid != pid && o.kind.is_ferry() && o.sails()).filter_map(|(_, o)| o.building.map(|ob| (ob, o.berth))).collect();
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
                    // [r4-flags-sim] Out and back each on its own right
                    // hand side, so boats meeting on a route pass apart.
                    let mut path = self.keep_to_starboard(&route, STARBOARD_FERRY);
                    let berth_s = crate::geom::path_length(&path);
                    let back: Vec<Vec2> = route.iter().rev().copied().collect();
                    path.extend(self.keep_to_starboard(&back, STARBOARD_FERRY).into_iter().skip(1));
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
                        out = leaving.min(port.kind.ferry_capacity());
                        out_prepaid = prepaid.min(out);
                        if leaving > out {
                            self.ports.boarding.insert(b, leaving - out);
                        }
                        if prepaid > out_prepaid {
                            self.ports.prepaid.insert(b, prepaid - out_prepaid);
                        }
                        self.ports.stats.remote_sailings_today += 1;
                    }
                    // [ferries] (D6) A large terminal's boats take more, and
                    // a car ferry drives cars aboard too.
                    let locals = (locals * port.kind.ferry_capacity() / ports::FERRY_CAPACITY).min(port.kind.ferry_capacity() - out);
                    let riders = out + locals;
                    let cars = if port.kind.car_deck() > 0 && reachable(b) && port.road_connected { self.ferry_cars(b, to, riders, port.kind.car_deck()) } else { 0 };
                    let fares = (riders - out_prepaid) as f32 * ports::FERRY_FARE + cars as f32 * ports::CAR_FERRY_FARE;
                    self.ports.stats.cars_ferried_today += cars;
                    // Ferry fares are public transport income, booked with the bus fares.
                    self.economy.credit_item(Flow::BusFares, fares, Item::Building(b));
                    self.ports.stats.ferry_riders_today += riders;
                    self.ports.stats.ferry_fares_today += fares;
                    self.ports.count_line_riders(b, to, remote, riders);
                    let tl = &mut self.ports.ports[*pid].timeline;
                    tl.board_untimed(minute / 60, riders);
                    tl.run(minute / 60, 1);
                    let kind = if port.kind.car_deck() > 0 { ShipKind::CarFerry } else { ShipKind::Ferry };
                    self.ports.ships.insert(Ship { kind, path, s: 0.0, prev_s: 0.0, speed, dwell: ports::FERRY_DWELL, port: b, to, cargo: Vec::new(), passengers: riders, docked: false, berth_s, color: 0, remote, home: false, quay: None, boxes_in: 0, boxes_out: 0, cars });
                    self.ports.ports[*pid].last_sailing = tick;
                    self.ports.ports[*pid].sailings += 1;
                }
            }
        }
    }

    /// [r4-flags-sim] `route` kept `side` metres to the right of the way
    /// it runs, eased in and out over `STARBOARD_EASE` from each end (it
    /// still leaves from and comes in to the pier), only where there is
    /// water there with room for the hull (half as far, or on the line
    /// itself, where the channel is narrower). Ships on the same water
    /// going opposite ways then pass each other instead of through.
    pub(super) fn keep_to_starboard(&self, route: &[Vec2], side: f32) -> Vec<Vec2> {
        if route.len() < 2 {
            return route.to_vec();
        }
        // A point every `STARBOARD_STEP` or so, so a long straight leg can bend out.
        let mut pts: Vec<Vec2> = vec![route[0]];
        for w in route.windows(2) {
            let n = (w[0].distance(w[1]) / STARBOARD_STEP).ceil().max(1.0) as usize;
            for k in 1..=n {
                pts.push(w[0].lerp(w[1], k as f32 / n as f32));
            }
        }
        let total = crate::geom::path_length(&pts);
        let edge = self.landscape.land_half() - 40.0;
        let wet = |p: Vec2| p.x.abs().max(p.y.abs()) > edge || self.landscape.is_water(p);
        let mut along = 0.0;
        let last = pts.len() - 1;
        let mut out = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            if i > 0 {
                along += pts[i].distance(pts[i - 1]);
            }
            let p = pts[i];
            let x = (along.min(total - along) / STARBOARD_EASE).clamp(0.0, 1.0);
            let ease = x * x * (3.0 - 2.0 * x);
            if i == 0 || i == last || ease <= 0.0 {
                out.push(p);
                continue;
            }
            let d = (pts[i + 1] - pts[i - 1]).normalize_or_zero();
            let right = Vec2::new(d.y, -d.x);
            let q = [1.0, 0.5].into_iter().map(|f| side * f * ease).find(|&o| wet(p + right * o) && wet(p + right * (o + STARBOARD_BEAM))).map_or(p, |o| p + right * o);
            out.push(q);
        }
        out
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

    /// [ferries] (D6) Cars driven aboard a car ferry: a share of the
    /// passengers bring their car, more on a run out of town; the deck
    /// takes `deck`. Seeded by the terminal and the tick.
    pub(super) fn ferry_cars(&self, from: BuildingId, to: Option<BuildingId>, riders: u32, deck: u32) -> u32 {
        let h = crate::hash::mix(self.clock.tick ^ 0xCA2F, { use slotmap::Key; from.data().as_ffi() });
        let share = if to.is_none() { 0.22 } else { 0.1 };
        ((riders as f32 * share) as u32 + (h % 5) as u32).min(deck)
    }

    /// [ferries] (D6) What a ferry terminal of `kind` at `p` would carry:
    /// the people within its reach, the towns off the map its water runs
    /// to, the terminals it would link, and riders (and cars) a day at
    /// today's numbers. None where it cannot go.
    pub fn ferry_estimate(&self, kind: PortKind, p: Vec2) -> Option<FerryEstimate> {
        if !kind.is_ferry() {
            return None;
        }
        let port = self.plan_port(kind, p).ok()?;
        let near = self.citizens.values().filter(|c| c.home.and_then(|h| self.building_entrance(h)).is_some_and(|q| q.distance(port.berth) < 500.0)).count() as u32;
        let towns = self.landscape.water_routes_out(port.berth).len().min(2) as u32;
        let terminals = self.ports.ports.values().filter(|o| o.kind.is_ferry() && o.sails() && self.landscape.water_route(port.berth, Some(o.berth)).is_some()).count() as u32;
        let sailings = if towns + terminals == 0 { 0 } else { ((ports::FERRY_SERVICE_END - ports::FERRY_SERVICE_START) as u64 / ports::FERRY_HEADWAY) as u32 };
        let per = (near / 12 + if towns > 0 { 4 } else { 2 } + 2) * kind.ferry_capacity() / ports::FERRY_CAPACITY;
        let riders = sailings * per.min(kind.ferry_capacity());
        let cars = if kind.car_deck() > 0 { sailings * ((per as f32 * if towns > 0 { 0.22 } else { 0.1 }) as u32 + 2).min(kind.car_deck()) } else { 0 };
        let fares = riders as f32 * ports::FERRY_FARE + cars as f32 * ports::CAR_FERRY_FARE;
        Some(FerryEstimate { near, towns, terminals, sailings, riders, cars, fares, upkeep: kind.upkeep_per_day() })
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
