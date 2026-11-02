//! [r8] Container lorries as they work in real ports and yards: a tractor
//! with a bare chassis drives to the depot (the shipping port, a freight
//! yard, a cargo hub or a container yard), queues at the gate, backs into
//! one of the depot's drawn bays and waits there for its box; the depot's
//! carrier (a straddle carrier at the port and the yards) lifts the very
//! box from the stack onto its chassis, and off it drives. A lorry
//! bringing a box (an empty going back, a forwarder's box of exports) has
//! it lifted off into the stack the same way. Lorries homed at a depot go
//! back to its bays after a delivery and stand there, still on the map and
//! selectable, "parked at the port, waiting for a box", until the next
//! load or until they have stood idle long enough to go home.
//!
//! Hooks (one line each elsewhere): `market.rs::dispatch_truck` hands a
//! depot's boxed load to [`World::box_pickup`]; `industry.rs` lets
//! [`World::box_lorry_arrived`] take a lorry reaching a depot or a firm on
//! a forwarder's round; `tick.rs` runs [`World::step_box_yards`].

use super::*;
use crate::hash::mix;
use crate::containers::{self as cx, BoxId, CraneJob, DepotYard, Event, LorryJob, TruckBoxes, Where};
use super::containers::DepotKind;
use super::industry::{is_return_trip, return_cargo};
use slotmap::Key;

/// Seconds a lorry stays parked in a bay at most between looks (the
/// yard releases it sooner): long enough that it never pulls out itself.
const BAY_HOLD: f32 = 1.0e7;
/// Seconds a lorry takes pulling out of a bay once released.
const PULL_OUT: f32 = 12.0;
/// Minutes a box stands in the stack before the carrier takes it on.
const SETTLE: u64 = 2;
/// How far a truck lot may be from a depot for its parked lorries to be
/// sent for a box.
const LOT_REACH: f32 = 2000.0;

impl DepotKind {
    /// [r8] Lorry bays it has (and draws); a scrap yard takes boxes
    /// straight off the lorry.
    pub fn bays(self) -> usize {
        match self {
            DepotKind::Yard => 3,
            DepotKind::Hub => 3,
            DepotKind::Rail => 5,
            DepotKind::Port => 8,
            DepotKind::Scrap => 0,
        }
    }
}

/// TEU a box takes on a chassis.
fn teu(forty: bool) -> u8 {
    if forty { 2 } else { 1 }
}

impl World {
    /// [r8] A depot with lorry bays, working.
    pub fn bay_depot(&self, b: BuildingId) -> Option<DepotKind> {
        self.box_depot(b).filter(|k| k.bays() > 0)
    }

    fn yard_entry(&mut self, d: BuildingId) -> &mut DepotYard {
        let n = self.bay_depot(d).map_or(0, |k| k.bays());
        let y = self.containers.depots.entry(d).or_default();
        if y.bays.len() < n {
            y.bays.resize(n, None);
        }
        y
    }

    /// [r8] The bay a lorry stands in: its depot and the bay's number.
    pub fn lorry_bay(&self, car: CarId) -> Option<(BuildingId, usize)> {
        let d = self.containers.lorries.get(&car)?.depot;
        let y = self.containers.depots.get(&d)?;
        y.bays.iter().position(|b| *b == Some(car)).map(|k| (d, k))
    }

    /// [r8] Where a lorry stands in the queue at a depot's gate.
    pub fn lorry_queued(&self, car: CarId) -> Option<(BuildingId, usize)> {
        let d = self.containers.lorries.get(&car)?.depot;
        let y = self.containers.depots.get(&d)?;
        y.queue.iter().position(|c| *c == car).map(|k| (d, k))
    }

    /// [r8] Room left on a lorry's chassis (twenty-foot units, of two).
    fn chassis_room(&self, car: CarId) -> u8 {
        let used: u8 = self.containers.trucks.get(&car).map_or(0, |t| t.boxes.iter().filter_map(|id| self.containers.boxes.get(*id)).map(|r| teu(r.forty)).sum());
        2u8.saturating_sub(used)
    }

    fn box_fits(&self, car: CarId, id: BoxId) -> bool {
        self.containers.boxes.get(id).is_some_and(|r| teu(r.forty) <= self.chassis_room(car))
    }

    /// [r8] Full boxes at depot `d` (or coming: aboard a freighter calling
    /// there, on the carrier) with `resource` for `to`.
    pub(super) fn boxes_due_at(&self, d: BuildingId, resource: Resource, to: BuildingId) -> usize {
        let ship_here = |s: &crate::ports::ShipId| self.containers.ships.get(s).is_some_and(|b| b.port == d);
        self.containers
            .boxes
            .values()
            .filter(|r| match r.at {
                Where::Stored(x) | Where::Lifting(x) => x == d,
                Where::Ship(s) => ship_here(&s),
                _ => false,
            })
            .filter(|r| r.goods.is_some_and(|g| g.resource == resource && (g.buyer.is_null() || g.buyer == to)))
            .count()
    }

    /// Full boxes stored at `d` (not on the carrier) for the load, oldest
    /// first, not already promised to a lorry there.
    fn pickable(&self, d: BuildingId, resource: Resource, to: BuildingId) -> Vec<BoxId> {
        let promised: std::collections::BTreeSet<BoxId> = self.containers.lorries.values().flat_map(|j| j.take.iter().copied()).collect();
        let mut v: Vec<(u64, BoxId)> = self
            .containers
            .boxes
            .iter()
            .filter(|(id, r)| r.at == Where::Stored(d) && r.since + SETTLE <= self.clock.tick && !promised.contains(id) && r.goods.is_some_and(|g| g.resource == resource && (g.buyer.is_null() || g.buyer == to)))
            .map(|(id, r)| (r.since, id))
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// [r8] A load leaving depot `d` in boxes: a container lorry fetches
    /// it (one standing idle in a bay, one sent over from a truck lot, or
    /// one that pulls up at the gate). False when the load is not in boxes
    /// here, or no lorry can come: the caller sends it as before.
    pub(super) fn box_pickup(&mut self, d: BuildingId, cargo: Cargo) -> bool {
        if !self.containers.live || is_return_trip(&cargo) || cargo.amount <= 0.0 || self.bay_depot(d).is_none() {
            return false;
        }
        if self.buildings.get(cargo.to).is_none_or(|b| b.construction.is_some()) {
            return false;
        }
        if self.boxes_due_at(d, cargo.resource, cargo.to) == 0 {
            return false;
        }
        let Some(car) = self.lorry_for(d) else { return false };
        let j = self.containers.lorries.entry(car).or_default();
        j.depot = d;
        j.cargo = Some(cargo);
        true
    }

    /// [r8] A lorry to send to depot `d` for a box: idle there already, from
    /// a truck lot near it, or a new one at its gate.
    pub(super) fn lorry_for(&mut self, d: BuildingId) -> Option<CarId> {
        // Idle in a bay, or idle in the queue.
        let idle = |w: &World, c: CarId| w.traffic.cars.contains_key(c) && w.containers.lorries.get(&c).is_some_and(|j| j.cargo.is_none() && j.take.is_empty() && j.round.is_none()) && !w.containers.trucks.contains_key(&c);
        if let Some(y) = self.containers.depots.get(&d) {
            let found = y.bays.iter().flatten().copied().find(|c| idle(self, *c)).or_else(|| y.queue.iter().copied().find(|c| idle(self, *c)));
            if found.is_some() {
                return found;
            }
        }
        if self.truck_count() >= crate::market::MAX_TRUCKS + super::market::BOX_TRUCKS_EXTRA {
            return None;
        }
        // Parked at a truck lot near the depot: it drives over.
        let at = self.building_lot(d)?.center;
        let lot = self
            .buildings
            .iter()
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind.is_truck_lot() && z.fleet_trucks > 0))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(at))))
            .filter(|(_, dist)| *dist <= LOT_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(id, _)| id);
        if let Some(lot) = lot {
            if let Some(car) = self.spawn_box_lorry(lot, d, d) {
                self.unpark_truck(lot);
                return Some(car);
            }
        }
        // A haulier's lorry pulls up at the gate.
        self.spawn_box_lorry(d, d, d)
    }

    /// [r8] Put a bare container rig on the road from `from` to depot or
    /// place `to`, homed at `home`. A lorry starting at the depot it is for
    /// arrives at once (at the gate).
    pub(super) fn spawn_box_lorry(&mut self, from: BuildingId, to: BuildingId, home: BuildingId) -> Option<CarId> {
        let (a_seg, a_pt) = self.truck_stop(from)?;
        let (b_seg, b_pt) = self.truck_stop(to)?;
        let (lanes, s0, s1) = self.traffic.net.find_route(a_seg, a_pt, b_seg, b_pt).filter(|r| !r.0.is_empty())?;
        let car = self.traffic.spawn_car(CitizenId::default(), Some(return_cargo(home, to)), lanes, s0, s1, 7);
        let h = mix(car.data().as_ffi(), 0x7EAC);
        let paint = (h % crate::vehicle::PAINTS as u64) as u8;
        self.traffic.cars[car].model = crate::vehicle::VehicleModel { class: crate::vehicle::VehicleClass::Mid, body: crate::vehicle::BodyType::Semi, wear: ((h >> 8) % 3) as u8, paint, boxed: crate::vehicle::CHASSIS_20, ..Default::default() };
        self.containers.lorries.insert(car, LorryJob { depot: to, ..Default::default() });
        Some(car)
    }

    /// [r8] A lorry reached the end of a leg. One bound for a depot's bays
    /// goes into a bay or the gate queue; one on a forwarder's round packs
    /// the firm's goods and drives on. True when handled here.
    pub(super) fn box_lorry_arrived(&mut self, car: CarId) -> bool {
        if !self.containers.live {
            return false;
        }
        let Some(cargo) = self.traffic.cars.get(car).and_then(|c| c.cargo) else { return false };
        if !is_return_trip(&cargo) {
            return false;
        }
        let at = cargo.to;
        // On a round: a firm's goods packed in.
        if let Some(stop) = self.containers.lorries.get(&car).and_then(|j| j.round.as_ref()).and_then(|r| r.stops.first().copied()).filter(|s| s.firm == at) {
            self.pack_at_firm(car, stop);
            return true;
        }
        let Some(_) = self.bay_depot(at) else { return false };
        let job_here = self.containers.lorries.get(&car).is_some_and(|j| j.depot == at);
        // Homed here (a lorry back from a delivery), or bringing boxes to set down.
        let boxes_here = self.containers.trucks.get(&car).is_some_and(|t| t.to.is_none_or(|b| b == at));
        if !(job_here || cargo.from == at || boxes_here) {
            return false;
        }
        self.into_depot(car, at)
    }

    /// [r8] Lorry `car` pulls into depot `d`: a free bay, else the queue at
    /// the gate. An idle one finding no room drives on.
    fn into_depot(&mut self, car: CarId, d: BuildingId) -> bool {
        let tick = self.clock.tick;
        let busy = self.containers.lorries.get(&car).is_some_and(|j| j.cargo.is_some() || !j.take.is_empty() || j.round.is_some()) || self.containers.trucks.contains_key(&car);
        let y = self.yard_entry(d);
        let free = y.bays.iter().position(|b| b.is_none());
        let queue_room = y.queue.len() < cx::GATE_QUEUE;
        if free.is_none() && !queue_room && !busy {
            return false;
        }
        let j = self.containers.lorries.entry(car).or_default();
        j.depot = d;
        j.since = tick;
        // A lorry of the old kind (its own trailer) homed here is a
        // container rig from now on: a bare chassis.
        if let Some(c) = self.traffic.cars.get_mut(car) {
            if crate::vehicle::chassis_of(c.model.boxed).is_none() {
                c.model.body = crate::vehicle::BodyType::Semi;
                c.model.boxed = crate::vehicle::CHASSIS_20;
            }
        }
        self.dress_truck(car);
        let y = self.containers.depots.get_mut(&d).unwrap();
        match free {
            Some(k) => {
                y.bays[k] = Some(car);
                self.traffic.pull_in(car, BAY_HOLD);
                if let Some(c) = self.traffic.cars.get_mut(car) {
                    c.bay = true;
                }
            }
            None => {
                y.queue.push(car);
                self.traffic.pull_in_queued(car, BAY_HOLD);
            }
        }
        true
    }

    /// [r8] Each minute, each depot with lorries: the queue moves up into
    /// free bays, the carrier finishes its move or starts the next, loaded
    /// lorries drive off, and idle ones that have stood long go.
    pub(super) fn step_box_yards(&mut self) {
        if self.containers.depots.is_empty() {
            return;
        }
        let depots: Vec<BuildingId> = self.containers.depots.keys().copied().collect();
        for d in depots {
            self.step_depot(d);
        }
    }

    fn step_depot(&mut self, d: BuildingId) {
        let tick = self.clock.tick;
        if self.bay_depot(d).is_none() {
            // The depot shut or was pulled down: its lorries drive away.
            let Some(y) = self.containers.depots.remove(&d) else { return };
            if let Some(job) = y.crane {
                self.crane_abort(d, job);
            }
            for c in y.bays.into_iter().flatten().chain(y.queue) {
                self.release_lorry(c, d);
            }
            return;
        }
        // Lorries gone from the road (towed, lifted off) leave their places.
        {
            let cars = &self.traffic.cars;
            let y = self.containers.depots.get_mut(&d).unwrap();
            for b in y.bays.iter_mut() {
                if b.is_some_and(|c| !cars.contains_key(c)) {
                    *b = None;
                }
            }
            y.queue.retain(|c| cars.contains_key(*c));
            if y.crane.is_some_and(|j| !cars.contains_key(j.car)) {
                let job = y.crane.take().unwrap();
                self.crane_abort(d, job);
            }
        }
        // Up from the gate into the free bays.
        loop {
            let y = self.containers.depots.get_mut(&d).unwrap();
            let Some(k) = y.bays.iter().position(|b| b.is_none()) else { break };
            if y.queue.is_empty() {
                break;
            }
            let c = y.queue.remove(0);
            y.bays[k] = Some(c);
            if let Some(car) = self.traffic.cars.get_mut(c) {
                car.bay = true;
            }
        }
        // The carrier's move.
        let mut finished = false;
        if let Some(job) = self.containers.depots[&d].crane {
            if tick >= job.start + cx::CARRIER_MINUTES {
                self.crane_finish(d, job);
                self.containers.depots.get_mut(&d).unwrap().crane = None;
                finished = true;
            }
        }
        // [perf] The looks for work (each a pass over the boxes) every
        // third minute, or as soon as the carrier is free.
        if !finished && (tick + (d.data().as_ffi() & 0xff)) % 3 != 0 {
            return;
        }
        if self.containers.depots[&d].crane.is_none() {
            if let Some(job) = self.next_crane_job(d) {
                self.crane_start(d, job);
            }
        }
        // Lorries done here drive off.
        let bays: Vec<CarId> = self.containers.depots[&d].bays.iter().flatten().copied().collect();
        let crane_car = self.containers.depots[&d].crane.map(|j| j.car);
        let waiting = !self.containers.depots[&d].queue.is_empty();
        for c in bays {
            if crane_car == Some(c) {
                continue;
            }
            self.maybe_depart(c, d, waiting);
        }
    }

    /// [r8] The next move for depot `d`'s carrier: the first lorry in bay
    /// order with a box to set down here, or one waiting for a box that is
    /// in the stack now.
    fn next_crane_job(&self, d: BuildingId) -> Option<CraneJob> {
        let y = self.containers.depots.get(&d)?;
        for (k, c) in y.bays.iter().enumerate() {
            let Some(c) = *c else { continue };
            let j = self.containers.lorries.get(&c);
            // A box aboard for here: off into the stack (not one it fetched,
            // nor one a round is still filling).
            let fetching = j.is_some_and(|j| j.cargo.is_some() || !j.take.is_empty() || j.round.as_ref().is_some_and(|r| !r.stops.is_empty()));
            if !fetching {
                if let Some(t) = self.containers.trucks.get(&c).filter(|t| t.to.is_none_or(|b| b == d)) {
                    if let Some(&id) = t.boxes.first() {
                        return Some(CraneJob { car: c, boxid: id, onto_truck: false, bay: k as u8, start: self.clock.tick });
                    }
                }
            }
            let Some(j) = j else { continue };
            // A box it was sent for, in the stack here.
            if let Some(&id) = j.take.iter().find(|id| self.containers.boxes.get(**id).is_some_and(|r| r.at == Where::Stored(d)) && self.box_fits(c, **id)) {
                return Some(CraneJob { car: c, boxid: id, onto_truck: true, bay: k as u8, start: self.clock.tick });
            }
            if let Some(cg) = j.cargo {
                let have = self.containers.trucks.get(&c).map_or(0, |t| t.boxes.len());
                if have < cx::boxes_for_load(cg.amount) {
                    if let Some(id) = self.pickable(d, cg.resource, cg.to).into_iter().find(|id| self.box_fits(c, *id)) {
                        return Some(CraneJob { car: c, boxid: id, onto_truck: true, bay: k as u8, start: self.clock.tick });
                    }
                }
            }
        }
        None
    }

    fn crane_start(&mut self, d: BuildingId, job: CraneJob) {
        if job.onto_truck {
            self.box_to(job.boxid, Where::Lifting(d), Event::Unloaded, d);
            // (the lift itself is not a log entry of its own)
            if let Some(r) = self.containers.boxes.get_mut(job.boxid) {
                r.log.pop();
            }
        } else {
            self.off_truck(Some(job.car), job.boxid);
            if let Some(r) = self.containers.boxes.get_mut(job.boxid) {
                r.at = Where::Lifting(d);
            }
            self.containers.touch();
            self.dress_truck(job.car);
        }
        self.containers.depots.get_mut(&d).unwrap().crane = Some(job);
    }

    fn crane_finish(&mut self, d: BuildingId, job: CraneJob) {
        self.containers.stats.crane_moves_total += 1;
        self.containers.depots.get_mut(&d).unwrap().moves += 1;
        if job.onto_truck {
            if !self.traffic.cars.contains_key(job.car) {
                self.box_to(job.boxid, Where::Stored(d), Event::Stored, d);
                return;
            }
            let to = self.containers.lorries.get(&job.car).and_then(|j| j.cargo.map(|c| c.to).or(j.take_to));
            self.box_to(job.boxid, Where::Truck(job.car), Event::CraneOn, to.unwrap_or_default());
            if let Some(j) = self.containers.lorries.get_mut(&job.car) {
                j.take.retain(|b| *b != job.boxid);
            }
            let take_to = self.containers.lorries.get(&job.car).and_then(|j| j.take_to);
            let t = self.containers.trucks.entry(job.car).or_insert_with(|| TruckBoxes { boxes: Vec::new(), to: take_to });
            t.boxes.push(job.boxid);
            self.dress_truck(job.car);
        } else {
            self.box_to(job.boxid, Where::Stored(d), Event::CraneOff, d);
            // A box of exports at a freight yard: the next train takes it.
            if self.box_depot(d) == Some(DepotKind::Rail) {
                if let Some(r) = self.containers.boxes.get(job.boxid).filter(|r| !r.export.is_empty()) {
                    let units = if r.forty { 2.0 } else { 1.0 } * crate::rail::TEU_UNITS;
                    if let Some(b) = self.buildings.get_mut(d) {
                        b.rail_outbound += units;
                    }
                }
            }
        }
    }

    /// The lorry under a move went away: the box goes where it was meant to.
    fn crane_abort(&mut self, d: BuildingId, job: CraneJob) {
        if self.containers.boxes.get(job.boxid).is_some_and(|r| r.at == Where::Lifting(d)) {
            if self.box_depot(d).is_some() {
                self.box_to(job.boxid, Where::Stored(d), Event::Stored, d);
            } else {
                let p = self.building_lot(d).map(|l| l.center).unwrap_or_default();
                if let Some(r) = self.containers.boxes.get_mut(job.boxid) {
                    r.at = Where::Stored(BuildingId::default());
                }
                self.box_to_storage(job.boxid, p);
            }
        }
    }

    /// [r8] Does lorry `c` in a bay at `d` drive off now, and where to?
    fn maybe_depart(&mut self, c: CarId, d: BuildingId, others_waiting: bool) {
        let tick = self.clock.tick;
        let Some(j) = self.containers.lorries.get(&c).cloned() else {
            // Not ours (an older save's lorry in a bay): it simply goes.
            self.release_lorry(c, d);
            return;
        };
        let aboard = self.containers.trucks.get(&c).map_or(0, |t| t.boxes.len());
        let waited = tick.saturating_sub(j.since);
        if let Some(cargo) = j.cargo {
            let want = cx::boxes_for_load(cargo.amount);
            let room = self.chassis_room(c) > 0;
            let more_here = !self.pickable(d, cargo.resource, cargo.to).is_empty();
            let more_coming = self.boxes_due_at(d, cargo.resource, cargo.to) > 0;
            let full = aboard >= want || !room;
            let enough = aboard > 0 && (full || !more_coming);
            if enough || (aboard == 0 && !more_coming) || waited >= cx::BOX_WAIT_MINUTES {
                self.depart_with_load(c, d, cargo);
                // What the chassis could not take follows on another lorry.
                if aboard > 0 && aboard < want && more_here {
                    let rest: Vec<BoxId> = self.pickable(d, cargo.resource, cargo.to).into_iter().take(want - aboard).collect();
                    self.haul_from_depot(d, cargo.to, rest);
                }
            }
            return;
        }
        if !j.take.is_empty() {
            // Its boxes went elsewhere meanwhile: on with what it has.
            let gone = j.take.iter().all(|id| self.containers.boxes.get(*id).is_none_or(|r| !matches!(r.at, Where::Stored(x) | Where::Lifting(x) if x == d)));
            if gone || waited >= cx::BOX_WAIT_MINUTES {
                if let Some(jj) = self.containers.lorries.get_mut(&c) {
                    jj.take.clear();
                }
            } else {
                return;
            }
        }
        if let Some(r) = j.round.as_ref() {
            if aboard > 0 && !r.stops.is_empty() {
                let to = r.stops[0].firm;
                self.depart(c, d, Some(to), None);
                return;
            }
            if aboard == 0 {
                // Its box set down here: home to the forwarder.
                let fw = r.forwarder;
                if let Some(jj) = self.containers.lorries.get_mut(&c) {
                    jj.round = None;
                }
                self.depart(c, d, Some(fw).filter(|b| self.buildings.contains_key(*b)), None);
                return;
            }
        }
        if aboard > 0 {
            let to = self.containers.trucks.get(&c).and_then(|t| t.to);
            if to.is_some_and(|b| b != d) || j.take_to.is_some() {
                self.depart(c, d, to.or(j.take_to), None);
            } else if to.is_none() && j.take_to.is_none() && self.containers.lorries.get(&c).is_some_and(|j| j.round.is_none()) && self.containers.depots[&d].crane.is_none() && waited > 120 {
                // A box it was to set down here and never was: out by road.
                self.depart(c, d, None, None);
            }
            return;
        }
        // Idle: stands a while, then goes home (or out of town).
        let limit = if others_waiting { 30 } else { cx::IDLE_MINUTES };
        if waited >= limit {
            self.release_lorry(c, d);
        }
    }

    /// [r8] Lorry `c` sets off from depot `d` with `cargo` for its buyer.
    fn depart_with_load(&mut self, c: CarId, d: BuildingId, cargo: Cargo) {
        if let Some(j) = self.containers.lorries.get_mut(&c) {
            j.cargo = None;
        }
        if !self.depart(c, d, Some(cargo.to), Some(cargo)) {
            // No road there: the load and its boxes go straight over.
            for id in self.boxes_on_truck(c) {
                if self.containers.boxes.get(id).is_some_and(|r| r.goods.is_some()) {
                    self.box_arrives(id, cargo.to, None);
                }
            }
            self.containers.trucks.remove(&c);
            self.dress_truck(c);
            self.deliver(cargo);
        }
    }

    /// [r8] Boxes at depot `d` to be taken to `to` on another lorry.
    fn haul_from_depot(&mut self, d: BuildingId, to: BuildingId, ids: Vec<BoxId>) {
        if ids.is_empty() {
            return;
        }
        match self.lorry_for(d) {
            Some(car) => {
                let j = self.containers.lorries.entry(car).or_default();
                j.depot = d;
                j.take = ids;
                j.take_to = Some(to);
            }
            None => {
                for id in ids {
                    self.box_arrives(id, to, None);
                }
            }
        }
    }

    /// [r8] Out of its bay (or the queue) at `d` and on to `to` (none:
    /// out of town), carrying `cargo` (none: an empty run). False when no
    /// road leads there: it stays where it is.
    fn depart(&mut self, c: CarId, d: BuildingId, to: Option<BuildingId>, cargo: Option<Cargo>) -> bool {
        if !self.traffic.cars.contains_key(c) {
            return false;
        }
        let to = to.filter(|b| !b.is_null());
        let home = self.traffic.cars[c].cargo.map(|g| g.from).unwrap_or(d);
        let ok = match to {
            Some(b) => self.head_for(c, b),
            None => self.redirect_out(c),
        };
        if !ok {
            return false;
        }
        self.leave_place(c, d);
        let car = &mut self.traffic.cars[c];
        car.cargo = Some(cargo.unwrap_or_else(|| return_cargo(home, to.unwrap_or_default())));
        car.bay = false;
        car.hold = PULL_OUT;
        car.stranded = false;
        self.begin_leg(c);
        self.car_departs_from(c, d);
        let bays_there = to.filter(|b| self.bay_depot(*b).is_some());
        if let Some(j) = self.containers.lorries.get_mut(&c) {
            j.since = 0;
            if let Some(b) = bays_there.filter(|_| j.round.as_ref().is_none_or(|r| r.stops.is_empty())) {
                j.depot = b;
            }
        }
        true
    }

    /// Take `c` out of depot `d`'s bays and queue.
    fn leave_place(&mut self, c: CarId, d: BuildingId) {
        if let Some(y) = self.containers.depots.get_mut(&d) {
            for b in y.bays.iter_mut() {
                if *b == Some(c) {
                    *b = None;
                }
            }
            y.queue.retain(|x| *x != c);
        }
    }

    /// [r8] An idle lorry leaves depot `d`: home to its business when that
    /// is elsewhere, else to a truck lot near, else out of town.
    fn release_lorry(&mut self, c: CarId, d: BuildingId) {
        let Some(car) = self.traffic.cars.get(c) else {
            self.leave_place(c, d);
            self.containers.lorries.remove(&c);
            return;
        };
        let home = car.cargo.map(|g| g.from).unwrap_or_default();
        let pos = self.building_lot(d).map_or(car.pos, |l| l.center);
        let home_ok = home != d && self.buildings.get(home).is_some_and(|h| h.operating() && !h.is_derelict());
        let dest = home_ok.then_some(home).or_else(|| self.fleet_lot_with_room(pos, super::industry::FLEET_REACH));
        self.containers.lorries.remove(&c);
        if !self.depart(c, d, dest, None) && !self.depart(c, d, None, None) {
            // Nowhere at all: it stays, and tries again later.
            self.containers.lorries.insert(c, LorryJob { depot: d, since: self.clock.tick, ..Default::default() });
        }
    }

    /// [r8] A forwarder's lorry at a firm on its round: the firm's export
    /// goods packed into the box, and on to the next firm or the depot.
    fn pack_at_firm(&mut self, car: CarId, stop: cx::ExportLoad) {
        let tick = self.clock.tick;
        let Some(round) = self.containers.lorries.get_mut(&car).and_then(|j| j.round.as_mut()) else { return };
        round.stops.remove(0);
        let fw = round.forwarder;
        let next = round.stops.first().map(|s| s.firm).unwrap_or(round.to);
        let to_depot = round.stops.is_empty();
        if let Some(&id) = self.containers.trucks.get(&car).and_then(|t| t.boxes.first()) {
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.export.push(stop);
                r.note(tick, Event::Packed, stop.firm);
            }
            self.containers.touch();
        }
        if let Some(f) = self.containers.forwarders.get_mut(&fw) {
            f.loads_total += 1;
            f.units_total += stop.amount;
            f.firms.insert(stop.firm);
        }
        let home = self.traffic.cars[car].cargo.map(|g| g.from).unwrap_or(fw);
        if self.head_for(car, next) {
            self.traffic.cars[car].cargo = Some(return_cargo(home, next));
            if to_depot {
                if let Some(j) = self.containers.lorries.get_mut(&car) {
                    j.depot = next;
                }
            }
            self.begin_leg(car);
            self.car_departs_from(car, stop.firm);
        } else if to_depot {
            // No road to the depot: the box goes out by road.
            if self.redirect_out(car) {
                self.traffic.cars[car].cargo = Some(return_cargo(home, BuildingId::default()));
            }
        }
        if self.traffic.cars.contains_key(car) {
            self.traffic.pull_in_queued(car, cx::PACK_SECONDS);
        }
    }

    /// [r8] Send lorry `car` from where it stands to building `b`'s kerb: the
    /// lane on its side of the street first, else the other way round (a
    /// firm behind it down the street is reached by going round).
    pub(super) fn head_for(&mut self, car: CarId, b: BuildingId) -> bool {
        let Some((seg, pt)) = self.truck_stop(b) else { return false };
        let mut lanes: Vec<(f32, crate::traffic::LaneId, f32)> = self.traffic.net.segment_lanes.get(&seg).map(|v| v.iter().map(|&l| {
            let (s, d) = self.traffic.net.lanes[l as usize].project(pt);
            (d, l, s)
        }).collect()).unwrap_or_default();
        lanes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        lanes.into_iter().any(|(_, l, s)| self.traffic.redirect(car, l, s))
    }

    /// [r8] What a container lorry is doing, for the inspector and the
    /// follow camera: "parked at the port, waiting for a box".
    pub fn lorry_status(&self, car: CarId) -> Option<String> {
        let j = self.containers.lorries.get(&car)?;
        let name = |b: BuildingId| self.buildings.get(b).map(|x| x.name.clone()).unwrap_or_else(|| "a depot".into());
        let place = |b: BuildingId| match self.box_depot(b) {
            Some(DepotKind::Port) => "the port".to_string(),
            Some(k) => format!("{} ({})", name(b), k.label()),
            None => name(b),
        };
        let aboard: Vec<String> = self.boxes_on_truck(car).iter().filter_map(|id| self.containers.boxes.get(*id)).map(|r| format!("{} ({})", r.number(), if r.forty { "40 ft" } else { "20 ft" })).collect();
        if let Some(c) = self.containers.depots.get(&j.depot).and_then(|y| y.crane).filter(|c| c.car == car) {
            let n = self.containers.boxes.get(c.boxid).map(|r| r.number()).unwrap_or_default();
            return Some(if c.onto_truck { format!("in a bay at {}: box {n} being lifted onto its chassis", place(j.depot)) } else { format!("in a bay at {}: box {n} being lifted off", place(j.depot)) });
        }
        if let Some((d, k)) = self.lorry_bay(car) {
            let what = if j.cargo.is_some() || !j.take.is_empty() {
                "waiting for its box"
            } else if !aboard.is_empty() {
                "waiting for the carrier to take its box"
            } else {
                "waiting for a box"
            };
            return Some(format!("parked at {} in bay {}, {what}", place(d), k + 1));
        }
        if let Some((d, k)) = self.lorry_queued(car) {
            return Some(format!("queued at the gate of {} ({} ahead)", place(d), k));
        }
        if let Some(r) = &j.round {
            let box_note = aboard.first().map(|b| format!(" with box {b}")).unwrap_or_default();
            return Some(if r.stops.is_empty() { format!("taking a box of exports{box_note} to {}", place(r.to)) } else { format!("collecting exports for {}{box_note}: {} firm{} to call at", name(r.forwarder), r.stops.len(), if r.stops.len() == 1 { "" } else { "s" }) });
        }
        if aboard.is_empty() {
            return Some(format!("empty chassis, on its way to {}", place(j.depot)));
        }
        Some(format!("carrying {}", aboard.join(", ")))
    }

    /// [r8] Note where a lorry went as it leaves the road, for the follow
    /// camera.
    pub(super) fn note_lorry(&mut self, car: CarId, text: String) {
        let tick = self.clock.tick;
        let n = &mut self.containers.lorry_notes;
        n.insert(car, (tick, text));
        if n.len() > 256 {
            let old: Vec<CarId> = {
                let mut v: Vec<(u64, CarId)> = n.iter().map(|(c, (t, _))| (*t, *c)).collect();
                v.sort();
                v.into_iter().take(64).map(|x| x.1).collect()
            };
            for c in old {
                n.remove(&c);
            }
        }
        self.containers.lorries.remove(&car);
        for y in self.containers.depots.values_mut() {
            for b in y.bays.iter_mut() {
                if *b == Some(car) {
                    *b = None;
                }
            }
            y.queue.retain(|x| *x != car);
        }
    }

    /// [r8] A place's name for a note.
    pub(super) fn place_label(&self, b: BuildingId) -> String {
        self.buildings.get(b).map(|x| x.name.clone()).unwrap_or_else(|| "a place since gone".into())
    }

    /// [r8] Where a lorry taken off the road went (the follow camera says it).
    pub fn lorry_note(&self, car: CarId) -> Option<&str> {
        self.containers.lorry_notes.get(&car).map(|n| n.1.as_str())
    }

    /// [r8] Jobs of lorries no longer on the road, dropped (hourly).
    pub(super) fn prune_box_lorries(&mut self) {
        let gone: Vec<CarId> = self.containers.lorries.keys().copied().filter(|c| !self.traffic.cars.contains_key(*c)).collect();
        for c in gone {
            self.containers.lorries.remove(&c);
        }
        let tick = self.clock.tick;
        self.containers.lorry_notes.retain(|_, (t, _)| tick.saturating_sub(*t) < 24 * 60);
    }
}
