//! The dispatcher: free units, the call queue, sending units (and help from
//! out of town) to incidents.

use super::*;

impl World {
    /// Units free for a call, per unit kind (`u.kind as usize`): in
    /// the station with a crew to spare, on patrol or standby, or driving home.
    pub(super) fn free_units(&self) -> [usize; 3] {
        let out = self.units_out();
        let mut n = [0; 3];
        for u in self.services.units.values().filter(|u| self.unit_available(u, &out)) {
            n[u.kind as usize] += 1;
        }
        n
    }

    /// Units out of the station (crews aboard), per station.
    pub(in crate::world) fn units_out(&self) -> HashMap<BuildingId, usize> {
        let mut out: HashMap<BuildingId, usize> = HashMap::new();
        for u in self.services.units.values().filter(|u| u.out()) {
            *out.entry(u.station).or_default() += 1;
        }
        out
    }

    /// Free for the next call: a unit in the station counts only while the
    /// station has a crew for one more vehicle; units already out have theirs.
    pub(in crate::world) fn unit_available(&self, u: &Unit, out: &HashMap<BuildingId, usize>) -> bool {
        // A unit leaving town with its station gone takes no more calls.
        if u.leaving || !self.station_alive(u.station) {
            return false;
        }
        match u.state {
            UnitState::Idle => out.get(&u.station).copied().unwrap_or(0) < self.crewed_units(u.station),
            UnitState::Patrol | UnitState::Returning => true,
            _ => false,
        }
    }

    /// Whether the city has a crewed station for the kind.
    pub(super) fn city_has(&self, kind: IncidentKind) -> bool {
        self.services.units.values().any(|u| u.kind == kind.unit() && self.crewed_units(u.station) > 0)
    }

    /// Every tick: the calls past their dispatch time, most urgent first
    /// (`Incident::priority`, then call order), each get the nearest free
    /// unit. A call that finds none waits in the queue and goes out the tick
    /// a unit clears; a violent crime may pull a car off a theft; a call left
    /// waiting `MUTUAL_AID_WAIT` with every unit busy goes to the next town.
    /// Also keeps the queue and saturation figures.
    pub(super) fn dispatch_queue(&mut self) {
        let tick = self.clock.tick;
        let mut queue: Vec<(u8, u64, u32, IncidentKind)> = self
            .services
            .active()
            .filter(|i| i.stage == Stage::Reported && i.dispatch_at.is_some_and(|t| tick >= t))
            .map(|i| (i.priority(), i.reported.unwrap_or(i.started), i.id, i.kind))
            .collect();
        queue.sort_by_key(|q| (q.0, q.1, q.2));
        let mut free = self.free_units();
        let mut queued = [0u32; 3];
        for (prio, reported, id, kind) in queue {
            let k = kind.slot();
            let first = self.services.incident(id).and_then(|i| i.dispatch_at) == Some(tick);
            // A call no free unit could reach is retried every five minutes.
            let retry = first || tick % 5 == 0 || !self.services.route_failed.contains_key(&id);
            if (free[k] > 0 || first || !self.city_has(kind)) && retry {
                if self.try_dispatch(id) {
                    self.services.route_failed.remove(&id);
                    free = self.free_units();
                    continue;
                }
                if free[k] > 0 {
                    // Free units, but no road from any of them: a crash on
                    // the highway beyond the city's streets, a cut-off block.
                    // The next town's (or the highway patrol's) units take it.
                    self.services.route_failed.insert(id, tick);
                    if self.services.incident(id).and_then(|i| i.dispatcher).is_some() {
                        self.services.stats.unreachable_today += 1;
                        self.send_from_out_of_town(id, MUTUAL_AID_TRAVEL, true);
                        continue;
                    }
                }
            }
            if free[k] == 0 && prio == 0 && kind == IncidentKind::Crime && self.divert_for(id) {
                self.services.stats.diverted_today += 1;
                free = self.free_units();
                continue;
            }
            let dispatcher = self.services.incident(id).and_then(|i| i.dispatcher);
            if free[k] == 0 && dispatcher.is_some() && tick.saturating_sub(reported) >= MUTUAL_AID_WAIT && self.city_has(kind) {
                self.services.stats.mutual_aid_today += 1;
                self.send_from_out_of_town(id, MUTUAL_AID_TRAVEL, true);
                continue;
            }
            queued[k] += 1;
        }
        let has = IncidentKind::ALL.map(|kind| self.city_has(kind));
        let st = &mut self.services.stats;
        for k in 0..3 {
            st.queued[k] = queued[k];
            st.queue_max_today[k] = st.queue_max_today[k].max(queued[k]);
            if has[k] && free[k] == 0 {
                st.saturated_since[k].get_or_insert(tick);
                st.saturated_min_today[k] += 1;
            } else {
                st.saturated_since[k] = None;
            }
        }
    }

    /// A violent crime with every police car busy: pull the nearest car that
    /// is still on its way to a theft or vandalism; that call goes back in
    /// the queue.
    pub(super) fn divert_for(&mut self, id: u32) -> bool {
        let Some(at) = self.services.incident(id).map(|i| i.pos) else { return false };
        let Some(to) = self.incident_access(id) else { return false };
        let mut cands: Vec<(f32, UnitId, u32)> = Vec::new();
        for (uid, u) in self.services.units.iter() {
            if u.kind != UnitKind::PoliceCar || u.state != UnitState::Responding {
                continue;
            }
            let Some(other) = u.incident.and_then(|o| self.services.incident(o)) else { continue };
            if other.priority() < 2 || other.stage != Stage::Dispatched {
                continue;
            }
            let p = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
            cands.push((p.distance(at), uid, other.id));
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        let tick = self.clock.tick;
        for (_, uid, other) in cands {
            if !self.send_unit(uid, id, to) {
                continue;
            }
            if let Some(o) = self.services.incident_mut(other) {
                o.stage = Stage::Reported;
                o.unit = None;
                o.dispatched = None;
                o.unit_name = None;
                o.dispatch_at = Some(tick);
            }
            return true;
        }
        false
    }

    /// Where a unit pulls up for an incident.
    pub(super) fn incident_access(&self, id: u32) -> Option<(SegmentId, Vec2)> {
        let inc = self.services.incident(id)?;
        // Street incidents (crashes, the hurt in them, takeovers): at the spot.
        let on_street = if inc.on_road { self.roads.nearest_street(inc.pos).map(|(s, _, q)| (s, q)) } else { None };
        // [fire-scene] An engine pulls up on the building's side, clear of
        // the junctions.
        let fire = if inc.kind == IncidentKind::Fire && !inc.on_road { self.fire_access(inc.building) } else { None };
        on_street.or(fire).or_else(|| self.building_lot(inc.building).and_then(|l| self.lot_access(l))).or_else(|| self.roads.nearest_street(inc.pos).map(|(s, _, q)| (s, q)))
    }

    /// Who answers 911 right now and how long the job takes to go out: a
    /// dispatcher at the desk, one on call, or the regional line.
    pub fn dispatcher(&self) -> (Option<CitizenId>, u64) {
        let centres: Vec<(BuildingId, &Building)> = self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == ServiceKind::Dispatch) && !b.is_derelict() && b.operating()).collect();
        let mut on_call = None;
        for (id, b) in &centres {
            for &w in &b.workers {
                if self.citizens.get(w).is_some_and(|c| c.inside == Some(*id)) {
                    let fast = if b.tier == Tier::High { DISPATCH_ON_SHIFT - 1 } else { DISPATCH_ON_SHIFT };
                    return (Some(w), fast);
                }
                on_call = on_call.or(Some(w));
            }
        }
        match on_call {
            Some(w) => (Some(w), DISPATCH_ON_CALL),
            None => (None, DISPATCH_REGIONAL),
        }
    }

    pub(super) fn report(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        let bname = self.buildings.get(inc.building).map(|b| b.name.clone()).unwrap_or_default();
        // The caller: someone inside, else someone who lives or works there.
        let inside: Vec<CitizenId> = self.citizens.iter().filter(|(cid, c)| c.inside == Some(inc.building) && Some(*cid) != inc.citizen && Some(*cid) != inc.suspect).map(|(cid, _)| cid).collect();
        let caller = if inc.on_road {
            // [driving] Whoever saw it from the street.
            inc.caller
        } else {
            inside.choose(self.srng()).copied().or_else(|| self.buildings.get(inc.building).and_then(|b| b.residents.first().or(b.workers.first()).copied())).or(if inc.kind == IncidentKind::Crime { inc.citizen } else { None })
        };
        let (dispatcher, delay) = self.dispatcher();
        {
            let i = self.services.incident_mut(id).unwrap();
            i.stage = Stage::Reported;
            i.reported = Some(tick);
            i.caller = caller;
            i.dispatcher = dispatcher;
            i.dispatch_at = Some(tick + delay);
        }
        let named: Vec<(CitizenId, String, &str)> = [(caller, "caller"), (dispatcher, "dispatcher")].into_iter().filter_map(|(c, role)| c.and_then(|c| self.citizens.get(c).map(|x| (c, x.name.clone(), role)))).collect();
        if let Some(i) = self.services.incident_mut(id) {
            for (c, n, role) in named {
                i.involve(c, &n, role);
            }
        }
        let what = match inc.kind {
            IncidentKind::Fire => format!("smoke pouring out of {bname}"),
            IncidentKind::Crime => format!("a {} at {bname}", inc.crime.map_or("robbery", |c| c.label())),
            IncidentKind::Medical if inc.on_road => format!("{} hurt in a crash by {bname}", inc.citizen.and_then(|c| self.citizens.get(c)).map_or("someone".to_string(), |c| c.name.clone())),
            IncidentKind::Medical => format!("{} collapsed at {bname}", inc.citizen.and_then(|c| self.citizens.get(c)).map_or("someone".to_string(), |c| c.name.clone())),
            IncidentKind::Crash => format!("a bad crash by {bname}"),
        };
        if let Some(c) = caller.and_then(|c| self.citizens.get_mut(c)) {
            c.remember(tick, format!("Called 911: {what}."));
            c.thought = "I called 911.".to_string();
        }
        self.events.push(SimEvent::EmergencyCall(id));
    }

    /// Send the nearest free unit of the right kind: one in a station with a
    /// crew to spare, on patrol or standby, or on its way home, each from
    /// where it is. With no crewed station of the kind in the city, a
    /// dispatcher calls the next town. True when help is on the way.
    pub(super) fn try_dispatch(&mut self, id: u32) -> bool {
        let Some(inc) = self.services.incident(id).cloned() else { return false };
        // No crewed station of the kind in the whole city: a dispatcher can
        // ask a neighbouring town, which sends a unit slowly and bills us.
        if !self.city_has(inc.kind) {
            if inc.dispatcher.is_some() {
                self.send_from_out_of_town(id, OUT_OF_TOWN_TRAVEL, false);
                return true;
            }
            return false;
        }
        let Some(to) = self.incident_access(id) else { return false };
        let uk = inc.kind.unit();
        let out = self.units_out();
        let mut free: Vec<(f32, UnitId)> = self
            .services
            .units
            .iter()
            .filter(|(_, u)| u.kind == uk && self.unit_available(u, &out))
            .map(|(uid, u)| (u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos).distance(inc.pos), uid))
            .collect();
        free.sort_by(|a, b| a.0.total_cmp(&b.0));
        free.into_iter().any(|(_, uid)| self.send_unit(uid, id, to))
    }

    /// The quickest way for lights and sirens from `from` to `to`: a unit
    /// may turn round in the street and pull up on either side, so the
    /// route is planned from and to both sides of the road and the shortest
    /// taken. (From the lane it happened
    /// to face, a car parked a block from a call drove round the block, and
    /// sometimes round two.)
    pub(super) fn siren_route(&self, from: (SegmentId, Vec2), to: (SegmentId, Vec2)) -> Option<(Vec<crate::traffic::LaneId>, f32, f32)> {
        let net = &self.traffic.net;
        let length = |r: &(Vec<crate::traffic::LaneId>, f32, f32)| r.0.iter().map(|l| net.lanes[*l as usize].length).sum::<f32>() - r.1 - net.lanes[*r.0.last().unwrap() as usize].length + r.2;
        // A point and its mirror across the street's centre line.
        let sides = |(seg, p): (SegmentId, Vec2)| -> Vec<Vec2> {
            match self.roads.nearest_street(p).filter(|(s, _, q)| *s == seg && q.distance(p) > 0.5) {
                Some((_, _, q)) => vec![p, q * 2.0 - p],
                None => vec![p],
            }
        };
        // ([fire-scene] A fire engine may come up either side: it crosses
        // to the building's kerb as it pulls in, `park_engine`.)
        let ends = sides(to);
        sides(from)
            .into_iter()
            .flat_map(|a| ends.iter().map(move |b| (a, *b)))
            .filter_map(|(a, b)| net.find_route(from.0, a, to.0, b))
            .filter(|r| !r.0.is_empty())
            .min_by(|a, b| length(a).total_cmp(&length(b)))
    }

    /// Help from the next town: billed, and there in `travel` minutes.
    /// `mutual` for mutual aid (the city has units, all busy).
    pub(super) fn send_from_out_of_town(&mut self, id: u32, travel: u64, mutual: bool) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        let uk = inc.kind.unit();
        let fee = OUT_OF_TOWN_FEE[inc.kind.slot()];
        self.economy.debit_item(Flow::ServiceUpkeep, fee, Item::Named(format!("{} from the next town", uk.label())));
        self.services.stats.out_of_town_calls_today += 1;
        self.services.stats.out_of_town_fees_today += fee;
        if let Some(d) = inc.dispatcher.and_then(|d| self.citizens.get_mut(d)) {
            d.remember(tick, if mutual { format!("Every {} we have was out. Asked the next town to cover {}.", uk.label(), inc.place) } else { format!("No {} of ours to send to {}. Called the next town over.", uk.label(), inc.place) });
        }
        self.services.route_failed.remove(&id);
        let i = self.services.incident_mut(id).unwrap();
        i.stage = Stage::Dispatched;
        i.dispatched = Some(tick);
        i.outside = true;
        i.fee = fee;
        i.arrive_at = Some(tick + travel);
        i.unit_name = Some(if mutual { format!("{} from the next town (mutual aid)", uk.label()) } else { format!("{} from out of town", uk.label()) });
        self.events.push(SimEvent::UnitDispatched(id));
    }

    /// Put `uid` on the road to incident `id` with lights and sirens, from
    /// its station or from wherever it is driving or parked. False if no
    /// road leads there.
    pub(super) fn send_unit(&mut self, uid: UnitId, id: u32, to: (SegmentId, Vec2)) -> bool {
        let tick = self.clock.tick;
        let Some(u) = self.services.units.get(uid) else { return false };
        let (station, state) = (u.station, u.state);
        let here = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
        let from = if state == UnitState::Idle { self.building_lot(station).and_then(|l| self.lot_access(l)) } else { self.roads.nearest_street(here).map(|(s, _, _)| (s, here)) };
        let Some(from) = from else { return false };
        let Some((lanes, s0, s1)) = self.siren_route(from, to) else { return false };
        self.pull_off_road(uid);
        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        self.traffic.cars[car].siren = true;
        self.services.by_car.insert(car, uid);
        let (call, number) = {
            let u = &mut self.services.units[uid];
            u.state = UnitState::Responding;
            u.car = Some(car);
            u.incident = Some(id);
            u.hospital = None;
            (u.kind.call_sign(), u.number)
        };
        let sname = self.buildings.get(station).map(|b| b.name.clone()).unwrap_or_default();
        let (kind, bid, dispatcher) = {
            let i = self.services.incident_mut(id).unwrap();
            i.stage = Stage::Dispatched;
            i.dispatched = Some(tick);
            i.unit = Some(uid);
            i.unit_name = Some(format!("{call} {number}, {sname}"));
            (i.kind, i.building, i.dispatcher)
        };
        let bname = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        if let Some(d) = dispatcher.and_then(|d| self.citizens.get_mut(d)) {
            d.remember(tick, format!("Took a 911 call: {} at {bname}. Sent {call} {number} from {sname}.", kind.label()));
        }
        self.events.push(SimEvent::UnitDispatched(id));
        true
    }

    /// [fire-scene] Send `uid` to fire `id` as its second engine (the
    /// second alarm): lights and sirens, the incident's own unit and stage
    /// left as they are. False if no road leads there.
    pub(in crate::world) fn send_second(&mut self, uid: UnitId, id: u32, to: (SegmentId, Vec2)) -> bool {
        let tick = self.clock.tick;
        let Some(u) = self.services.units.get(uid) else { return false };
        let (station, state) = (u.station, u.state);
        let here = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
        let from = if state == UnitState::Idle { self.building_lot(station).and_then(|l| self.lot_access(l)) } else { self.roads.nearest_street(here).map(|(s, _, _)| (s, here)) };
        let Some(from) = from else { return false };
        let Some((lanes, s0, s1)) = self.siren_route(from, to) else { return false };
        self.pull_off_road(uid);
        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        self.traffic.cars[car].siren = true;
        self.services.by_car.insert(car, uid);
        let (call, number) = {
            let u = &mut self.services.units[uid];
            u.state = UnitState::Responding;
            u.car = Some(car);
            u.incident = Some(id);
            u.hospital = None;
            u.patrol_until = 0;
            (u.kind.call_sign(), u.number)
        };
        let sname = self.buildings.get(station).map(|b| b.name.clone()).unwrap_or_default();
        let (bid, dispatcher) = {
            let i = self.services.incident_mut(id).unwrap();
            i.second = Some(uid);
            (i.building, i.dispatcher)
        };
        let bname = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        if let Some(d) = dispatcher.and_then(|d| self.citizens.get_mut(d)) {
            d.remember(tick, format!("Second alarm at {bname}. Sent {call} {number} from {sname} as well."));
        }
        true
    }
}
