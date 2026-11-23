//! [r11-box-stock] Keeping the depots' stock within their room. A terminal
//! (the shipping port, a freight yard, a cargo hub) takes in every box
//! that lands or comes back empty, but only ships, trains and lorries take
//! them away again, so imports outrunning exports piled up (an 80-day city
//! held 1400 boxes at a port with room for 60, 2200 at a cargo hub).
//!
//! Now each hour a terminal over [`cx::TERMINAL_FILL`] of its room trucks
//! the overflow (its lines' empties, the ones not going out on the next
//! ship or train) to a container yard that keeps it for them on a storage
//! contract: the line pays the yard [`cx::STORAGE_FEE`] a box a day. The
//! terminal calls its boxes back when it runs low (empties for the town's
//! exports); after [`cx::STORAGE_DAYS`] the line sends a box home (back to
//! its terminal to ship out when there is room, else out of town by road).
//! Over its room with no yard to take the rest, the lines clear the backlog
//! out of town by road, lorries loaded at the back gate by hired reach
//! stackers (an older save works its surplus off so over a few days), and
//! the boxes no yard could keep make the pressure for one more container
//! yard: an entrepreneur opens one on an industrial lot, or turns a
//! shut-down industrial business into one. A depot's room is what its
//! drawn stacks hold where the renderer has told the sim
//! (`Containers::drawn_room`), so every box a depot holds is drawn.

use super::*;
use crate::containers::{self as cx, BoxId, Event, Owner, TruckBoxes, Where};
use super::containers::DepotKind;

/// A terminal: where boxes land and from where they leave town.
fn terminal(k: DepotKind) -> bool {
    matches!(k, DepotKind::Port | DepotKind::Rail | DepotKind::Hub)
}

/// Twenty-foot units a box takes on a chassis.
fn teu(forty: bool) -> u8 {
    if forty { 2 } else { 1 }
}

/// [r11-box-stock] A container yard's storage, for the inspector and the API.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StorageCard {
    /// Boxes kept for the lines here now, and how many more it takes.
    pub kept: usize,
    pub room: usize,
    pub fee: f32,
    pub book: cx::StorageBook,
}

impl World {
    /// [r11-box-stock] How many boxes depot `b` keeps at most: its ground's
    /// say (`box_capacity`), no more than its drawn stacks hold.
    pub fn box_room(&self, b: BuildingId, kind: DepotKind) -> usize {
        let cap = self.box_capacity(b, kind);
        match self.containers.drawn_room.get(&b) {
            Some(&n) => cap.min(n as usize),
            None => cap,
        }
    }

    /// [r11-box-stock] Boxes standing in depot `b`'s stack.
    pub fn depot_stock(&self, b: BuildingId) -> usize {
        self.containers.boxes.values().filter(|r| r.at.stack() == Some(b)).count()
    }

    /// [r11-box-stock] Boxes kept at the yards for terminal `t` (on their
    /// way there too).
    pub fn kept_away(&self, t: BuildingId) -> usize {
        self.containers.boxes.values().filter(|r| r.kept_for == Some(t)).count()
    }

    /// Boxes lorries are on their way to set down at `d` (their chassis,
    /// or a box they are sent to fetch for it).
    fn coming_to(&self, d: BuildingId) -> usize {
        let fetch: usize = self.containers.lorries.values().filter(|j| j.take_to == Some(d)).map(|j| j.take.len()).sum();
        let aboard: usize = self.containers.trucks.values().filter(|t| t.to == Some(d)).map(|t| t.boxes.len()).sum();
        fetch + aboard
    }

    /// [r11-box-stock] Room a container yard has left for the lines' boxes.
    fn storage_room(&self, y: BuildingId) -> usize {
        let room = (self.box_room(y, DepotKind::Yard) as f32 * cx::STORAGE_FILL) as usize;
        room.saturating_sub(self.depot_stock(y) + self.coming_to(y))
    }

    /// The container yard nearest `p` (within reach) with room for the
    /// lines' boxes, but `but`.
    fn storage_yard_for(&self, p: Vec2, but: BuildingId) -> Option<(BuildingId, usize)> {
        self.box_depots()
            .into_iter()
            .filter(|d| d.1 == DepotKind::Yard && d.0 != but && d.2.distance(p) <= cx::STORAGE_REACH)
            .map(|d| (d.0, self.storage_room(d.0), d.2.distance(p)))
            .filter(|d| d.1 > 0)
            .min_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)))
            .map(|d| (d.0, d.1))
    }

    /// [r11-box-stock] The empties a terminal's lines may move off it now:
    /// theirs, not full, not boxes of exports, no lorry sent for them, not
    /// on a freighter's list; those not going out on the next ship or train
    /// first, then the oldest.
    fn movable_empties(&self, d: BuildingId) -> Vec<BoxId> {
        let promised: std::collections::BTreeSet<BoxId> = self.containers.lorries.values().flat_map(|j| j.take.iter().copied()).collect();
        let shipping: std::collections::BTreeSet<BoxId> = self.containers.ships.values().flat_map(|s| s.outbound.iter().copied()).collect();
        let mut v: Vec<(bool, u64, BoxId)> = self
            .containers
            .boxes
            .iter()
            .filter(|(id, r)| r.at == Where::Stored(d) && r.goods.is_none() && r.export.is_empty() && r.owner == Owner::Line && !promised.contains(id) && !shipping.contains(id))
            .map(|(id, r)| (r.bound_out, r.since, id))
            .collect();
        v.sort();
        v.into_iter().map(|x| x.2).collect()
    }

    /// Take boxes off the front of `ids` for one lorry: as many as fit on
    /// its chassis (a forty-footer, or two twenties).
    fn chassis_load(&self, ids: &mut Vec<BoxId>) -> Vec<BoxId> {
        let mut load = Vec::new();
        let mut used = 0u8;
        let mut k = 0;
        while k < ids.len() {
            let t = self.containers.boxes.get(ids[k]).map_or(2, |r| teu(r.forty));
            if used + t <= 2 {
                used += t;
                load.push(ids.remove(k));
            } else {
                k += 1;
            }
            if used >= 2 {
                break;
            }
        }
        load
    }

    /// [r11-box-stock] Mark boxes going to yard `y` as kept there for
    /// terminal `t`.
    fn keep_for(&mut self, ids: &[BoxId], t: BuildingId, y: BuildingId) {
        for &id in ids {
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.kept_for = Some(t);
                r.bound_out = false;
            }
        }
        self.containers.stats.to_storage_total += ids.len() as u32;
        self.containers.storage.entry(y).or_default().taken_in_total += ids.len() as u32;
    }

    /// [r11-box-stock] A lorry loaded at depot `d`'s back gate (a hired
    /// reach stacker, not the depot's own carrier): it drives `ids` to
    /// yard `to`, or out of town. False when no lorry can go.
    fn backlog_lorry(&mut self, d: BuildingId, ids: &[BoxId], to: Option<BuildingId>) -> bool {
        if ids.is_empty() || self.containers.haulers.len() >= cx::BACKLOG_LORRIES {
            return false;
        }
        let Some(car) = self.spawn_box_lorry(d, to.unwrap_or(d), d) else { return false };
        if to.is_none() && !self.depart(car, d, None, None) {
            // No road out of town: the lorry is not needed.
            self.traffic.remove_car(car);
            self.containers.lorries.remove(&car);
            return false;
        }
        for &id in ids {
            self.box_to(id, Where::Truck(car), Event::OnLorry, to.unwrap_or_default());
        }
        self.containers.trucks.insert(car, TruckBoxes { boxes: ids.to_vec(), to });
        self.containers.haulers.insert(car);
        self.dress_truck(car);
        true
    }

    /// [r11-box-stock] Lorry `car` has emptied its boxes at `at` and is to
    /// take them home to terminal `home`: when that is over its fill and a
    /// container yard near has room, the empties go there to be kept for
    /// their line (the lorry drives them over, then home).
    pub(super) fn empty_to_storage_instead(&mut self, car: CarId, at: BuildingId, home: BuildingId) {
        let Some(t) = self.containers.trucks.get(&car).filter(|t| t.to.is_none() && !t.boxes.is_empty()) else { return };
        let ids = t.boxes.clone();
        if !ids.iter().all(|id| self.containers.boxes.get(*id).is_some_and(|r| r.goods.is_none() && r.export.is_empty() && r.owner == Owner::Line)) {
            return;
        }
        let Some(kind) = self.box_depot(home).filter(|k| terminal(*k)) else { return };
        let fill = (self.box_room(home, kind) as f32 * cx::TERMINAL_FILL) as usize;
        if self.depot_stock(home) + self.coming_to(home) < fill {
            return;
        }
        let Some(p) = self.place_pos(at) else { return };
        let Some((y, n)) = self.storage_yard_for(p, home) else { return };
        if n < ids.len() {
            return;
        }
        if let Some(t) = self.containers.trucks.get_mut(&car) {
            t.to = Some(y);
        }
        self.keep_for(&ids, home, y);
    }

    /// [r11-box-stock] The hourly round (at :17, with the containers'):
    /// each terminal over its fill trucks the overflow to the yards (or,
    /// over its room with no yard to take it, out of town); one running low
    /// calls back the boxes kept for it.
    pub(super) fn hourly_box_stock(&mut self) {
        // Hired lorries gone (off the map, or home).
        let cars = &self.traffic.cars;
        let trucks = &self.containers.trucks;
        self.containers.haulers.retain(|c| cars.contains_key(*c) && trucks.contains_key(c));
        // The terminals furthest over their room first: the hired lorries
        // go where the backlog is worst.
        let mut depots: Vec<(usize, BuildingId, DepotKind, Vec2)> = self.box_depots().into_iter().filter(|d| terminal(d.1)).map(|(d, k, p)| (self.depot_stock(d).saturating_sub(self.box_room(d, k)), d, k, p)).collect();
        depots.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        // The hired lorries free this hour, shared out between the terminals
        // over their room by how far over each is (what one cannot use goes
        // to the next).
        let mut free = cx::BACKLOG_LORRIES.saturating_sub(self.containers.haulers.len());
        let mut over_room: usize = depots.iter().map(|d| d.0).sum();
        for &(_, d, kind, pos) in &depots {
            let room = self.box_room(d, kind);
            let target = (room as f32 * cx::TERMINAL_FILL) as usize;
            // Boxes lorries are already sent to fetch from here.
            let leaving: usize = self.containers.lorries.values().filter(|j| j.depot == d && j.take_to.is_some()).map(|j| j.take.len()).sum();
            let stock = self.depot_stock(d).saturating_sub(leaving);
            if stock > room {
                // Over its room with full boxes nobody has fetched: the
                // oldest are unpacked into the shed here (their goods go on
                // as loose freight), and the empties go with the rest.
                self.unpack_waiting(d, stock - room);
            }
            if stock > target {
                let mut ids = self.movable_empties(d);
                // Over its room: the backlog, at the back gate.
                let mut backlog = stock.saturating_sub(room);
                let share = if backlog > 0 { (free * backlog).div_ceil(over_room.max(1)).min(cx::BACKLOG_LORRIES_PER_HOUR) } else { 0 };
                over_room = over_room.saturating_sub(backlog);
                for _ in 0..share {
                    if backlog == 0 || ids.is_empty() {
                        break;
                    }
                    let load = self.chassis_load(&mut ids);
                    let to = self.storage_yard_for(pos, d).filter(|y| y.1 >= load.len()).map(|y| y.0);
                    // [r15-freight] Not out of town by road: the lines take
                    // what no yard keeps back by sea or rail
                    // (`reposition_surplus`), not two at a time in lorries.
                    if to.is_none() {
                        break;
                    }
                    if !self.backlog_lorry(d, &load, to) {
                        break;
                    }
                    free = free.saturating_sub(1);
                    match to {
                        Some(y) => self.keep_for(&load, d, y),
                        None => {
                            self.containers.stats.cleared_by_road_total += load.len() as u32;
                            self.containers.stats.overflow_today += load.len() as u32;
                        }
                    }
                    backlog = backlog.saturating_sub(load.len());
                }
                // Over its fill: to the yards through the bays.
                let mut over = self.depot_stock(d).saturating_sub(target + leaving);
                if self.bay_depot(d).is_some() {
                    for _ in 0..cx::SHIFT_LORRIES_PER_HOUR {
                        if over == 0 || ids.is_empty() {
                            break;
                        }
                        let Some((y, n)) = self.storage_yard_for(pos, d) else { break };
                        let mut load = self.chassis_load(&mut ids);
                        load.truncate(n);
                        if !self.haul_boxes(d, y, &load) {
                            break;
                        }
                        self.keep_for(&load, d, y);
                        over = over.saturating_sub(load.len());
                    }
                }
            } else if stock < (room as f32 * cx::CALL_BACK_FILL) as usize {
                self.call_back(d, pos, 1);
            }
        }
        self.reposition_surplus();
    }

    /// [r15-freight] (photo 2026-11-20 F2) No depot holds more than its
    /// room: whatever the lorries above could not move this hour, the
    /// lines' surplus empties over a depot's room go back to sea or rail
    /// with their next sailing or train, straight off the stack (a real
    /// terminal stops taking a line's empties when its blocks are full and
    /// the line repositions them). Harvey's day-80 save held 2,205 boxes at
    /// a cargo hub with room for 23, 1,375 at the port with room for 60:
    /// stacks drawn past the crane and boxes nowhere drawn at all, and up
    /// to 110 hired lorries a day carrying the backlog about. Returns how
    /// many went.
    pub(super) fn reposition_surplus(&mut self) -> usize {
        let mut gone = 0;
        let depots: Vec<(BuildingId, DepotKind)> = self.box_depots().into_iter().filter(|d| d.1 != DepotKind::Scrap).map(|d| (d.0, d.1)).collect();
        for (d, kind) in depots {
            let room = self.box_room(d, kind);
            let over = self.depot_stock(d).saturating_sub(room);
            if over == 0 {
                continue;
            }
            for id in self.movable_empties(d).into_iter().take(over) {
                self.box_gone(id, Event::SentOut);
                self.containers.stats.repositioned_total += 1;
                // No yard kept it: the pressure for a container yard.
                self.containers.stats.overflow_today += 1;
                gone += 1;
            }
        }
        gone
    }

    /// [r11-box-stock] Up to `n` full boxes standing at terminal `d` for
    /// [`cx::UNPACK_AFTER_HOURS`] or more, no lorry sent for them: unpacked
    /// where they stand, the oldest first.
    fn unpack_waiting(&mut self, d: BuildingId, n: usize) {
        let tick = self.clock.tick;
        let promised: std::collections::BTreeSet<BoxId> = self.containers.lorries.values().flat_map(|j| j.take.iter().copied()).collect();
        let mut v: Vec<(u64, BoxId)> = self
            .containers
            .boxes
            .iter()
            .filter(|(id, r)| r.at == Where::Stored(d) && r.goods.is_some() && !promised.contains(id) && tick.saturating_sub(r.since) >= cx::UNPACK_AFTER_HOURS * 60)
            .map(|(id, r)| (r.since, id))
            .collect();
        v.sort();
        for (_, id) in v.into_iter().take(n) {
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.goods = None;
                r.note(tick, Event::Unpacked, d);
            }
        }
        self.containers.touch();
    }

    /// [r11-box-stock] Terminal `t` calls back boxes kept for it at the
    /// yards: up to `lorries` lorries, the nearest yard's first.
    fn call_back(&mut self, t: BuildingId, pos: Vec2, lorries: usize) {
        let promised: std::collections::BTreeSet<BoxId> = self.containers.lorries.values().flat_map(|j| j.take.iter().copied()).collect();
        let mut kept: Vec<(f32, u64, BoxId, BuildingId)> = self
            .containers
            .boxes
            .iter()
            .filter(|(id, r)| r.kept_for == Some(t) && !promised.contains(id))
            .filter_map(|(id, r)| if let Where::Stored(y) = r.at { Some((self.place_pos(y)?.distance(pos), r.since, id, y)) } else { None })
            .collect();
        if kept.is_empty() {
            return;
        }
        kept.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let y = kept[0].3;
        let mut ids: Vec<BoxId> = kept.iter().filter(|k| k.3 == y).map(|k| k.2).collect();
        for _ in 0..lorries {
            let load = self.chassis_load(&mut ids);
            if load.is_empty() || !self.haul_boxes(y, t, &load) {
                break;
            }
            self.containers.stats.called_back_total += load.len() as u32;
            self.containers.storage.entry(y).or_default().returned_total += load.len() as u32;
        }
    }

    /// [r11-box-stock] Midnight: the lines pay the yards for the boxes kept
    /// there; a box kept its term goes home (back to its terminal when that
    /// has room, else out of town by road); the overflow no yard could take
    /// is counted for the pressure.
    pub(super) fn daily_box_stock(&mut self) {
        let tick = self.clock.tick;
        // A box no longer at a yard (or a lorry on its way) is kept no more.
        let stale: Vec<BoxId> = self
            .containers
            .boxes
            .iter()
            .filter(|(_, r)| r.kept_for.is_some())
            .filter(|(_, r)| r.owner != Owner::Line || !match r.at {
                Where::Stored(y) => self.box_depot(y) == Some(DepotKind::Yard),
                Where::Truck(_) | Where::Lifting(_) => true,
                _ => false,
            })
            .map(|(id, _)| id)
            .collect();
        for id in stale {
            self.containers.boxes[id].kept_for = None;
        }
        // The fees, yard by yard.
        let mut kept: BTreeMap<BuildingId, Vec<(u64, BoxId)>> = BTreeMap::new();
        for (id, r) in self.containers.boxes.iter() {
            if let (Some(_), Where::Stored(y)) = (r.kept_for, r.at) {
                kept.entry(y).or_default().push((r.since, id));
            }
        }
        for b in self.containers.storage.values_mut() {
            b.fees_yday = b.fees_today;
            b.fees_today = 0.0;
        }
        for (&y, v) in &kept {
            let fee = v.len() as f32 * cx::STORAGE_FEE;
            if let Some(z) = self.buildings.get_mut(y).and_then(|b| b.business.as_mut()) {
                z.revenue_today += fee;
            }
            self.transfer(Account::Outside, Account::Business(y), fee, crate::bank::Kind::Fees);
            let book = self.containers.storage.entry(y).or_default();
            book.fees_today = fee;
            book.fees_total += fee;
            self.containers.stats.storage_fees_total += fee;
        }
        // Kept their term: home, a yard's carrier load at a time. A yard
        // over its room (its stacks drawn smaller than its ground said)
        // sends the last come home too.
        let term = cx::STORAGE_DAYS * crate::clock::MINUTES_PER_DAY;
        for (y, mut v) in kept {
            v.sort();
            let excess = self.depot_stock(y).saturating_sub(self.box_room(y, DepotKind::Yard)).min(v.len());
            let late: Vec<BoxId> = v[v.len() - excess..].iter().map(|x| x.1).collect();
            let mut due: Vec<BoxId> = v.into_iter().filter(|(since, _)| tick.saturating_sub(*since) >= term).map(|x| x.1).take(12).collect();
            let late: Vec<BoxId> = late.into_iter().filter(|id| !due.contains(id)).collect();
            due.extend(late);
            for id in due {
                let Some(t) = self.containers.boxes.get(id).and_then(|r| r.kept_for) else { continue };
                let room_there = self.box_depot(t).is_some_and(|k| self.depot_stock(t) + self.coming_to(t) < (self.box_room(t, k) as f32 * cx::TERMINAL_FILL) as usize);
                if room_there && self.haul_boxes(y, t, &[id]) {
                    // Back to ship out from there.
                    if let Some(r) = self.containers.boxes.get_mut(id) {
                        r.bound_out = true;
                    }
                    self.containers.storage.entry(y).or_default().returned_total += 1;
                    continue;
                }
                if let Some(r) = self.containers.boxes.get_mut(id) {
                    r.kept_for = None;
                }
                self.containers.stats.cleared_by_road_total += 1;
                self.containers.storage.entry(y).or_default().returned_total += 1;
                self.haul_out(y, id, Event::SentOut);
            }
        }
        // What no yard could keep: over the terminals' fill, past the room
        // the yards have left.
        let mut over = 0usize;
        let mut free = 0usize;
        for (d, kind, _) in self.box_depots() {
            if terminal(kind) {
                over += self.depot_stock(d).saturating_sub((self.box_room(d, kind) as f32 * cx::TERMINAL_FILL) as usize);
            } else if kind == DepotKind::Yard {
                free += self.storage_room(d);
            }
        }
        let s = &mut self.containers.stats;
        s.overflow_yday = s.overflow_today + over.saturating_sub(free) as u32;
        s.overflow_today = 0;
    }

    /// [r11-box-stock] A container yard's storage: what it keeps for the
    /// lines, its room for more, its fee and its books.
    pub fn storage_card(&self, y: BuildingId) -> Option<StorageCard> {
        if self.box_depot(y) != Some(DepotKind::Yard) {
            return None;
        }
        let kept = self.containers.boxes.values().filter(|r| r.kept_for.is_some() && r.at == Where::Stored(y)).count();
        Some(StorageCard { kept, room: self.storage_room(y), fee: cx::STORAGE_FEE, book: self.containers.storage.get(&y).cloned().unwrap_or_default() })
    }

    /// [r11-box-stock] The pressure found no lot for a container yard: a
    /// shut container yard near the terminals reopens, or a shut-down
    /// industrial business is turned into one (its yard and sheds take the
    /// boxes), else one losing money lately (its owner lets the ground to
    /// the lines instead). Not another trade the boxes need (hubs,
    /// forwarders, scrap yards, truck lots), nor a sole works of its kind,
    /// nor a tower. Returns the building.
    pub(super) fn convert_to_storage_yard(&mut self, why: &str) -> Option<BuildingId> {
        let anchors: Vec<Vec2> = self.box_depots().into_iter().filter(|d| terminal(d.1)).map(|d| d.2).collect();
        if anchors.is_empty() {
            return None;
        }
        let supply = self.pressure_supply();
        let pick = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Industrial && b.operating() && !b.is_derelict() && !b.is_tenant() && b.port.is_none() && b.rail.is_none() && !b.is_power() && b.floors <= 3)
            .filter_map(|(id, b)| {
                let z = b.business.as_ref()?;
                let yard = z.kind == BusinessKind::ContainerYard;
                if matches!(z.kind, BusinessKind::CargoHub | BusinessKind::FreightForwarder | BusinessKind::ScrapYard) || z.kind.is_truck_lot() {
                    return None;
                }
                // A shut yard first, then other shut works, then works
                // losing money (and not the town's only one).
                let rank = if z.closed && yard {
                    0
                } else if z.closed {
                    1
                } else if !yard && (z.broke_days > 0 || z.losing_days(5) >= 3) && supply.open[z.kind.index()] > 1 {
                    2
                } else {
                    return None;
                };
                // Ground for the stacks (the whole plot, merged lots and all).
                let ground = self.building_lot(id).map_or(0.0, |l| crate::zoning::polygon_area(&l.polygon).abs());
                let lot = self.lots.get(b.lot)?;
                if ground < 600.0 || !lot.car_access() {
                    return None;
                }
                let d = anchors.iter().map(|a| a.distance(lot.center)).fold(f32::INFINITY, f32::min);
                (d <= cx::STORAGE_REACH).then_some((rank, -(ground as i64 / 200), d, id))
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)).then(a.3.cmp(&b.3)))
            .map(|x| x.3)?;
        let jobs = self.buildings[pick].jobs;
        let (kind, shut) = self.buildings[pick].business.as_ref().map(|z| (z.kind, z.closed)).unwrap_or((BusinessKind::ContainerYard, true));
        let old = format!("{} {}", if shut { "shut-down" } else { "struggling" }, kind.label());
        self.set_business(pick, Some(Business::new(BusinessKind::ContainerYard, jobs)));
        self.fit_business_name(pick);
        let reopened = kind == BusinessKind::ContainerYard;
        if let Some(z) = self.buildings[pick].business.as_mut() {
            z.origin = Some(if reopened { format!("A shut container yard reopened to keep the lines' overflow for a fee: {why}.") } else { format!("A {old} turned into a container yard: {why}.") });
        }
        self.events.push(SimEvent::BuildingChanged(pick));
        let (name, at) = (self.buildings[pick].name.clone(), self.building_lot(pick).map(|l| l.center).unwrap_or_default());
        let tick = self.clock.tick;
        let news = if reopened { format!("📦 {name} reopened, keeping the port's overflow of shipping containers for a fee.") } else { format!("📦 A {old} became {name}, keeping the port's overflow of shipping containers for a fee.") };
        self.social.push_news(tick, news, at, None);
        Some(pick)
    }
}
