//! [K34] Shipping containers' lives, the systems (`crate::containers`
//! holds the records and rules): boxes made as they come in by rail, sea
//! and road; set down at the yards and the port; put on the lorries that
//! take their goods to the buyer, unpacked at cargo hubs; emptied, bought
//! off the lorry by a stack developer, or taken back to storage; the
//! owners' daily round (store, send out, sell, scrap); Stacks homes built
//! from the boxes bought; scrap yards cutting them into steel. Nothing
//! runs per tick: these hooks fire when a train, ship or lorry moves a box,
//! and an hourly and a daily pass do the rest.

use super::*;
use crate::containers::{self as cx, BoxHome, BoxId, BoxRec, Event, Goods, Owner, TruckBoxes, Where};
use crate::hash::{hash01, mix};
use crate::ports::PortKind;
use crate::rail::{BoxKind, BoxLoad};
use super::industry::{is_return_trip, return_cargo};
use slotmap::Key;

/// [K34] The kinds of place that keep boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepotKind {
    /// A container yard business: stores, buys and sells boxes.
    Yard,
    /// A cargo hub: unpacks boxes and keeps the empties.
    Hub,
    /// A freight yard on the railway.
    Rail,
    /// The shipping port.
    Port,
    /// A scrap yard: boxes waiting to be cut up.
    Scrap,
}

impl DepotKind {
    pub fn label(self) -> &'static str {
        match self {
            DepotKind::Yard => "container yard",
            DepotKind::Hub => "cargo hub",
            DepotKind::Rail => "freight yard",
            DepotKind::Port => "shipping port",
            DepotKind::Scrap => "scrap yard",
        }
    }
}

/// [K34] A box's life story, for the inspector and the API.
#[derive(Clone, Debug, Default)]
pub struct BoxStory {
    pub number: String,
    pub line: String,
    pub size: String,
    pub kind: String,
    pub origin: String,
    pub built_year_ago: f32,
    pub condition: String,
    pub wear: f32,
    pub marks: Vec<String>,
    pub owner: String,
    pub owner_id: Option<BuildingId>,
    pub now: String,
    pub now_id: Option<BuildingId>,
    pub car: Option<crate::traffic::CarId>,
    pub goods: Option<String>,
    pub visits: u16,
    /// (tick, what, where) oldest first.
    pub history: Vec<(u64, String, String, Option<BuildingId>)>,
}

/// [K34] The town's boxes by where they are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxCounts {
    pub total: u32,
    pub on_trains: u32,
    pub on_trucks: u32,
    pub stored: u32,
    pub stored_full: u32,
    pub on_sites: u32,
    pub in_homes: u32,
    pub at_scrap: u32,
}

/// [K34] The look a semi's trailer box is drawn with (`VehicleModel::boxed`):
/// its line and a wear step.
pub fn truck_look(line: u8, wear: f32) -> u8 {
    1 + line % 10 + 10 * ((wear * 4.0).round() as u8).min(4)
}

impl World {
    fn today(&self) -> i64 {
        self.clock.day() as i64
    }

    /// [K34] What kind of box depot `b` is, if it is one that works.
    pub fn box_depot(&self, b: BuildingId) -> Option<DepotKind> {
        let bld = self.buildings.get(b)?;
        if !bld.operating() || bld.is_derelict() || bld.is_tenant() {
            return None;
        }
        if let Some(z) = bld.business.as_ref().filter(|z| !z.closed) {
            match z.kind {
                BusinessKind::ScrapYard => return Some(DepotKind::Scrap),
                BusinessKind::ContainerYard => return Some(DepotKind::Yard),
                BusinessKind::CargoHub if self.cargo_hubs.hubs.contains_key(&b) => return Some(DepotKind::Hub),
                _ => {}
            }
        }
        if bld.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard && l.works()) {
            return Some(DepotKind::Rail);
        }
        if bld.port == Some(PortKind::ShippingPort) && bld.construction.is_none() {
            return Some(DepotKind::Port);
        }
        None
    }

    /// [K34] Every working depot: (building, kind, lot centre), in key order.
    pub fn box_depots(&self) -> Vec<(BuildingId, DepotKind, Vec2)> {
        self.buildings.keys().filter_map(|id| Some((id, self.box_depot(id)?, self.building_lot(id)?.center))).collect()
    }

    /// [K34] Lot centres of the places that store boxes (not scrap yards):
    /// where a scrap yard is wanted.
    pub(super) fn box_depot_spots(&self) -> Vec<Vec2> {
        self.box_depots().into_iter().filter(|d| d.1 != DepotKind::Scrap).map(|d| d.2).collect()
    }

    /// [K34] How many boxes depot `b` holds before its owners send the
    /// extra out: by its ground.
    pub fn box_capacity(&self, b: BuildingId, kind: DepotKind) -> usize {
        let area = self.building_lot(b).map_or(0.0, |l| crate::zoning::polygon_area(&l.polygon).abs());
        let n = match kind {
            DepotKind::Yard => area / 16.0,
            DepotKind::Hub => area / 45.0,
            DepotKind::Rail => 24.0 + area / 120.0,
            DepotKind::Port => 60.0,
            DepotKind::Scrap => area / 40.0,
        };
        (n as usize).clamp(6, 400)
    }

    fn nearest_depot(&self, p: Vec2, want: impl Fn(DepotKind) -> bool) -> Option<BuildingId> {
        self.box_depots().into_iter().filter(|d| want(d.1)).min_by(|a, b| a.2.distance_squared(p).total_cmp(&b.2.distance_squared(p)).then(a.0.cmp(&b.0))).map(|d| d.0)
    }

    fn place_pos(&self, b: BuildingId) -> Option<Vec2> {
        self.building_lot(b).map(|l| l.center)
    }

    /// [K34] A new box coming into town (its line, number, origin and age
    /// from a hash of the city's seed and the count of boxes made).
    pub(super) fn make_box(&mut self, kind: BoxKind, forty: bool, at: Where, goods: Option<Goods>, first: Event, place: BuildingId) -> BoxId {
        let tick = self.clock.tick;
        let day = self.today();
        let st = &mut self.containers.stats;
        st.made += 1;
        let h = mix(self.config.seed ^ 0xC047_A1E5, st.made);
        let (line, serial, origin, years) = cx::arrival(h);
        let knocks = (years * 4.0 * (0.6 + crate::hash::unit(mix(h, 17)))) as u16;
        let forty = forty && kind != BoxKind::Tank;
        let mut r = BoxRec { line, serial, forty, kind, origin, built: (day - (years * 365.0) as i64) as i32, knocks, owner: Owner::Line, at, goods, since: tick, visits: 1, log: Vec::new() };
        r.note(tick, first, place);
        match first {
            Event::ArrivedRail => st.arrived_today[0] += 1,
            Event::ArrivedSea => st.arrived_today[1] += 1,
            Event::ArrivedRoad => st.arrived_today[2] += 1,
            _ => {}
        }
        match first {
            Event::ArrivedRail => st.arrived_total[0] += 1,
            Event::ArrivedSea => st.arrived_total[1] += 1,
            Event::ArrivedRoad => st.arrived_total[2] += 1,
            _ => {}
        }
        self.containers.touch();
        self.containers.boxes.insert(r)
    }

    /// [K34] Move box `id` to `at`, noting why; a lift or a ride is a knock.
    fn box_to(&mut self, id: BoxId, at: Where, what: Event, place: BuildingId) {
        let tick = self.clock.tick;
        let Some(r) = self.containers.boxes.get_mut(id) else { return };
        if r.at != at {
            r.knocks = r.knocks.saturating_add(1);
        }
        r.at = at;
        r.since = tick;
        r.note(tick, what, place);
        self.containers.touch();
    }

    /// [K34] A box leaves town (or the records): counted, then dropped.
    fn box_gone(&mut self, id: BoxId, what: Event) {
        if self.containers.boxes.remove(id).is_some() {
            let s = &mut self.containers.stats;
            match what {
                Event::SentOut => s.sent_out_total += 1,
                Event::SoldOutOfTown => s.sold_out_of_town_total += 1,
                _ => {}
            }
            self.containers.touch();
        }
    }

    // ── coming in ────────────────────────────────────────────────────────────

    /// [K34] A freight train loading at the edge of the map: a record for
    /// each import box on its wagons (from out of town, full).
    pub(super) fn boxes_board_train(&mut self, boxes: &mut [BoxLoad]) {
        if !self.containers.live {
            return;
        }
        for b in boxes.iter_mut().filter(|b| b.id.is_none() && b.yard.is_some()) {
            let goods = b.resource.map(|resource| Goods { resource, buyer: BuildingId::default() });
            let id = self.make_box(b.kind, b.forty, Where::Train, goods, Event::ArrivedRail, BuildingId::default());
            // Not arrived yet: the first entry is rewritten when it is set down.
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.log.clear();
                self.containers.stats.arrived_today[0] -= 1;
                self.containers.stats.arrived_total[0] -= 1;
            }
            b.id = Some(id);
        }
    }

    /// [K34] A freight train calling at yard `at`: its boxes for here are
    /// set down (`off`), and empties stored here go out on it as exports
    /// (as many as `on` slots; the records of those put on are returned in
    /// order, `None` where the yard has no empty to spare).
    pub(super) fn boxes_train_call(&mut self, at: BuildingId, off: &[BoxId], on: usize) -> Vec<Option<BoxId>> {
        if !self.containers.live {
            return vec![None; on];
        }
        for &id in off {
            let first = self.containers.boxes.get(id).is_some_and(|r| r.log.is_empty());
            self.box_to(id, Where::Stored(at), if first { Event::ArrivedRail } else { Event::Unloaded }, at);
            if first {
                self.containers.stats.arrived_today[0] += 1;
                self.containers.stats.arrived_total[0] += 1;
            }
        }
        let out = self.empties_to_send(at, on);
        let mut v: Vec<Option<BoxId>> = out.into_iter().map(Some).collect();
        for &id in v.iter().flatten() {
            self.box_to(id, Where::Train, Event::SentOut, at);
            self.containers.stats.sent_out_total += 1;
        }
        v.resize(on, None);
        v
    }

    /// [K34] Up to `n` empties at `at` its line can send out (not the
    /// yard's own stock, not one fit only for scrap), oldest first.
    fn empties_to_send(&self, at: BuildingId, n: usize) -> Vec<BoxId> {
        if n == 0 {
            return Vec::new();
        }
        let day = self.today();
        let mut v: Vec<(u64, BoxId)> = self.containers.boxes.iter().filter(|(_, r)| r.at == Where::Stored(at) && r.goods.is_none() && r.owner == Owner::Line && !r.for_scrap(day)).map(|(id, r)| (r.since, id)).collect();
        v.sort();
        v.into_iter().take(n).map(|(_, id)| id).collect()
    }

    /// [K34] A freighter landing `cargo` at port `port`: its boxes, full,
    /// on the quay.
    pub(super) fn boxes_landed_by_sea(&mut self, port: BuildingId, cargo: &Cargo) {
        if !self.containers.live || cargo.amount <= 0.0 {
            return;
        }
        let kind = BoxKind::for_resource(Some(cargo.resource));
        let n = cx::boxes_for_load(cargo.amount);
        for k in 0..n {
            let forty = kind != BoxKind::Tank && (n - k >= 2 || crate::hash::unit(mix(self.containers.stats.made, 3)) < 0.6);
            self.make_box(kind, forty, Where::Stored(port), Some(Goods { resource: cargo.resource, buyer: BuildingId::default() }), Event::ArrivedSea, port);
        }
    }

    /// [K34] A freighter leaving port `port` with `n` boxes of exports:
    /// that many empties go out on it.
    pub(super) fn boxes_out_by_sea(&mut self, port: BuildingId, n: usize) {
        if !self.containers.live {
            return;
        }
        for id in self.empties_to_send(port, n) {
            self.box_gone(id, Event::SentOut);
        }
    }

    // ── lorries ─────────────────────────────────────────────────────────────

    /// Full boxes at `from` holding `resource` for `to` (or for whoever
    /// there ordered it), oldest first, at most `n`.
    fn full_boxes_at(&self, from: BuildingId, resource: Resource, to: BuildingId, n: usize) -> Vec<BoxId> {
        let mut v: Vec<(u64, BoxId)> = self
            .containers
            .boxes
            .iter()
            .filter(|(_, r)| r.at == Where::Stored(from) && r.goods.is_some_and(|g| g.resource == resource && (g.buyer.is_null() || g.buyer == to)))
            .map(|(id, r)| (r.since, id))
            .collect();
        v.sort();
        v.into_iter().take(n).map(|(_, id)| id).collect()
    }

    /// [K34] Does a load from `from` ride in boxes stored there?
    pub(super) fn has_full_boxes(&self, from: BuildingId, resource: Resource, to: BuildingId) -> bool {
        self.containers.live && self.containers.boxes.values().any(|r| r.at == Where::Stored(from) && r.goods.is_some_and(|g| g.resource == resource && (g.buyer.is_null() || g.buyer == to)))
    }

    /// [K34] A lorry just put on the road with `cargo`: from out of town, a
    /// semi brings its box in with it; from a yard or the port, the boxes
    /// holding the load ride on it.
    pub(super) fn boxes_on_dispatch(&mut self, car: crate::traffic::CarId, origin_edge: bool, cargo: &Cargo, to_site: bool) {
        if !self.containers.live || is_return_trip(cargo) || cargo.amount <= 0.0 || to_site {
            return;
        }
        let ids = if origin_edge {
            let kind = BoxKind::for_resource(Some(cargo.resource));
            let forty = crate::hash::unit(mix(self.containers.stats.made, 5)) < 0.3;
            vec![self.make_box(kind, forty, Where::Truck(car), Some(Goods { resource: cargo.resource, buyer: cargo.to }), Event::ArrivedRoad, cargo.to)]
        } else {
            let ids = self.full_boxes_at(cargo.from, cargo.resource, cargo.to, cx::boxes_for_load(cargo.amount));
            for &id in &ids {
                self.box_to(id, Where::Truck(car), Event::OnLorry, cargo.to);
            }
            ids
        };
        if ids.is_empty() {
            return;
        }
        self.containers.trucks.insert(car, TruckBoxes { boxes: ids, to: None });
        self.dress_truck(car);
    }

    /// [K34] The lorry's look follows the first box aboard: a semi with
    /// that box on its trailer, or its own look again when it has none.
    fn dress_truck(&mut self, car: crate::traffic::CarId) {
        let day = self.today();
        let look = self.containers.trucks.get(&car).and_then(|t| t.boxes.first()).and_then(|id| self.containers.boxes.get(*id)).map(|r| truck_look(r.line, r.wear(day)));
        if let Some(c) = self.traffic.cars.get_mut(car) {
            match look {
                Some(l) => {
                    c.model.body = crate::vehicle::BodyType::Semi;
                    c.model.boxed = l;
                }
                None => c.model.boxed = 0,
            }
        }
    }

    /// [K34] A load delivered with no lorry (no route, or the roads full of
    /// trucks): its boxes go straight to the buyer.
    pub(super) fn boxes_follow(&mut self, cargo: &Cargo) {
        if !self.containers.live || is_return_trip(cargo) || cargo.amount <= 0.0 {
            return;
        }
        for id in self.full_boxes_at(cargo.from, cargo.resource, cargo.to, cx::boxes_for_load(cargo.amount)) {
            self.box_arrives(id, cargo.to, None);
        }
    }

    /// [K34] A lorry with boxes delivering at `cargo.to` (before the goods
    /// are handed over).
    pub(super) fn boxes_arrive(&mut self, car: crate::traffic::CarId, cargo: &Cargo) {
        let Some(tb) = self.containers.trucks.get(&car).cloned() else { return };
        for id in tb.boxes {
            if self.containers.boxes.get(id).is_some_and(|r| r.goods.is_some()) {
                self.box_arrives(id, cargo.to, Some(car));
            }
        }
        self.dress_truck(car);
    }

    /// [K34] Box `id` with goods has reached `to`. At a cargo hub it is
    /// unpacked and kept; at the buyer it is emptied, and then an old one
    /// goes for scrap, a stack developer may buy it, or it goes back to
    /// storage: on the lorry `car` when there is one, else at once.
    fn box_arrives(&mut self, id: BoxId, to: BuildingId, car: Option<crate::traffic::CarId>) {
        let day = self.today();
        if self.cargo_hubs.hubs.contains_key(&to) {
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.goods = None;
            }
            self.box_to(id, Where::Stored(to), Event::Unpacked, to);
            self.off_truck(car, id);
            return;
        }
        let tick = self.clock.tick;
        let Some(r) = self.containers.boxes.get_mut(id) else { return };
        r.goods = None;
        r.note(tick, Event::Delivered, to);
        let for_scrap = r.for_scrap(day);
        self.containers.stats.delivered_total += 1;
        self.containers.stats.delivered_today += 1;
        let at = self.place_pos(to).unwrap_or_default();
        // An old box is sent straight to be cut up when there is a yard.
        let bound = if for_scrap {
            self.nearest_depot(at, |k| k == DepotKind::Scrap).inspect(|&y| {
                if let Some(r) = self.containers.boxes.get_mut(id) {
                    r.owner = Owner::Scrap(y);
                    r.note(tick, Event::ToScrap, y);
                }
            })
        } else {
            self.developer_for(at).inspect(|&site| self.sell_to_developer(id, site, Event::BoughtOffTruck))
        };
        match (car, bound) {
            (Some(c), Some(b)) => {
                if let Some(t) = self.containers.trucks.get_mut(&c) {
                    t.to = Some(b);
                }
            }
            (Some(_), None) => {}
            (None, Some(b)) => self.box_lands(id, b),
            (None, None) => self.box_to_storage(id, at),
        }
        self.containers.touch();
    }

    fn off_truck(&mut self, car: Option<crate::traffic::CarId>, id: BoxId) {
        let Some(c) = car else { return };
        if let Some(t) = self.containers.trucks.get_mut(&c) {
            t.boxes.retain(|b| *b != id);
            if t.boxes.is_empty() {
                self.containers.trucks.remove(&c);
            }
        }
    }

    /// [K34] After the delivery: a lorry whose box was bought (or is going
    /// for scrap) takes it there before it drives on.
    pub(super) fn boxes_after_delivery(&mut self, car: crate::traffic::CarId, at: BuildingId, home: BuildingId) {
        let Some(to) = self.containers.trucks.get(&car).and_then(|t| t.to) else {
            self.boxes_check_truck(car, at);
            return;
        };
        if !self.traffic.cars.contains_key(car) {
            self.boxes_check_truck(car, at);
            return;
        }
        let ok = (|| {
            let (seg, pt) = self.truck_stop(to)?;
            let (goal, s_end, _) = self.traffic.net.nearest_lane_on(seg, pt)?;
            self.traffic.redirect(car, goal, s_end).then_some(())
        })()
        .is_some();
        if ok {
            self.traffic.cars[car].cargo = Some(return_cargo(home, to));
            self.begin_leg(car);
        } else {
            // No road there: handed over where it stands.
            self.boxes_drop(car, to);
        }
    }

    /// [K34] A lorry's trip ended at `at` (an empty run): the boxes bound
    /// here come off (onto a home site or the scrap pile), and empties are
    /// set down when this is a depot.
    pub(super) fn boxes_drop(&mut self, car: crate::traffic::CarId, at: BuildingId) {
        let Some(tb) = self.containers.trucks.get(&car).cloned() else { return };
        let depot = self.box_depot(at);
        if tb.to == Some(at) || (tb.to.is_none() && depot.is_some_and(|k| k != DepotKind::Scrap)) {
            self.containers.trucks.remove(&car);
            for id in tb.boxes {
                if tb.to == Some(at) {
                    self.box_lands(id, at);
                } else {
                    self.box_to(id, Where::Stored(at), Event::Stored, at);
                }
            }
            self.dress_truck(car);
        }
    }

    /// [K34] A box bought or sent for scrap reaching `to`: onto the home
    /// site, the finished home, the scrap pile, else into storage.
    fn box_lands(&mut self, id: BoxId, to: BuildingId) {
        let tick = self.clock.tick;
        let c = &mut self.containers;
        if let Some(h) = c.homes.get_mut(&to) {
            if !h.done || (h.boxes.len() as u16) < h.want {
                h.boxes.push(id);
                if let Some(r) = c.boxes.get_mut(id) {
                    r.at = if h.done { Where::Home(to) } else { Where::Site(to) };
                    r.since = tick;
                    r.knocks = r.knocks.saturating_add(1);
                    if h.done {
                        r.note(tick, Event::BuiltIntoHome, to);
                    }
                }
                let done = h.done;
                c.touch();
                if done {
                    self.events.push(SimEvent::BuildingChanged(to));
                }
                return;
            }
        }
        if self.box_depot(to) == Some(DepotKind::Scrap) {
            self.box_to(id, Where::Scrap(to), Event::ToScrap, to);
            return;
        }
        let at = self.place_pos(to).unwrap_or_default();
        self.box_to_storage(id, at);
    }

    /// [K34] An empty with nowhere particular to go: into the nearest
    /// depot (not a scrap yard), else back out of town.
    fn box_to_storage(&mut self, id: BoxId, near: Vec2) {
        if let Some(r) = self.containers.boxes.get_mut(id) {
            if matches!(r.owner, Owner::Home(_) | Owner::Scrap(_)) {
                r.owner = Owner::Line;
            }
        }
        match self.nearest_depot(near, |k| k != DepotKind::Scrap) {
            Some(d) => self.box_to(id, Where::Stored(d), Event::Stored, d),
            None => self.box_gone(id, Event::SentOut),
        }
    }

    /// [K34] A lorry with boxes left the road at `at` (parked, home, or off
    /// the map): what it still carries is set down there, or leaves town
    /// with it at the map's edge.
    pub(super) fn boxes_check_truck(&mut self, car: crate::traffic::CarId, at: BuildingId) {
        if self.traffic.cars.contains_key(car) {
            return;
        }
        let Some(tb) = self.containers.trucks.remove(&car) else { return };
        for id in tb.boxes {
            match tb.to {
                Some(to) if self.buildings.contains_key(to) => self.box_lands(id, to),
                _ if at.is_null() || !self.buildings.contains_key(at) => {
                    // Off the map with its lorry; a box bought for a home is
                    // the developer's, so it goes to the home instead.
                    self.box_gone(id, Event::SentOut)
                }
                _ => {
                    let p = self.place_pos(at).unwrap_or_default();
                    self.box_to_storage(id, p);
                }
            }
        }
    }

    /// [K34] Put boxes on an empty lorry from `from` to `to` (a home site
    /// or a scrap yard). False when no lorry can go (none spare, or no
    /// road): the caller hands them over directly.
    fn haul_boxes(&mut self, from: BuildingId, to: BuildingId, ids: &[BoxId]) -> bool {
        if ids.is_empty() || self.truck_count() >= crate::market::MAX_TRUCKS {
            return false;
        }
        let route = (|| {
            let (a_seg, a_pt) = self.truck_stop(from)?;
            let (b_seg, b_pt) = self.truck_stop(to)?;
            self.traffic.net.find_route(a_seg, a_pt, b_seg, b_pt)
        })();
        let Some((lanes, s0, s1)) = route.filter(|r| !r.0.is_empty()) else { return false };
        let car = self.traffic.spawn_car(CitizenId::default(), Some(return_cargo(from, to)), lanes, s0, s1, 7);
        let paint = (mix(car.data().as_ffi(), 7) % crate::vehicle::PAINTS as u64) as u8;
        self.traffic.cars[car].model = crate::vehicle::VehicleModel { class: crate::vehicle::VehicleClass::Mid, body: crate::vehicle::BodyType::Semi, wear: 1, paint, ..Default::default() };
        for &id in ids {
            self.box_to(id, Where::Truck(car), Event::OnLorry, to);
        }
        self.containers.trucks.insert(car, TruckBoxes { boxes: ids.to_vec(), to: Some(to) });
        self.dress_truck(car);
        true
    }

    // ── stack developers and their homes ──────────────────────────────────────

    /// [K34] Does a stack developer build the home going up on `b`? Low-rent
    /// homes in the districts that pile boxes (or none), a share of them,
    /// by the building's seed.
    fn box_home_candidate(&self, b: BuildingId) -> bool {
        let Some(bld) = self.buildings.get(b) else { return false };
        use crate::districts::Look;
        bld.land_use == LandUse::Residential
            && bld.tier == Tier::Low
            && !bld.is_tenant()
            && matches!(bld.look, None | Some(Look::Kowloon) | Some(Look::Patchwork))
            && bld.plot.form.is_none()
            && hash01(bld.seed, 0xB0C5_403E, 0) < cx::BOX_HOME_SHARE
    }

    /// [K34] Boxes the home on `b` is built from.
    fn home_want(&self, b: BuildingId) -> u16 {
        let Some(bld) = self.buildings.get(b) else { return 0 };
        let area = self.building_lot(b).map_or(0.0, |l| crate::zoning::polygon_area(&l.polygon).abs());
        cx::boxes_for_home(area, bld.floors)
    }

    /// [K34] The stack developer site nearest `p` (within reach) still
    /// short of boxes.
    fn developer_for(&self, p: Vec2) -> Option<BuildingId> {
        let mut coming: BTreeMap<BuildingId, u16> = BTreeMap::new();
        for r in self.containers.boxes.values() {
            if let Owner::Home(h) = r.owner {
                if !matches!(r.at, Where::Site(_) | Where::Home(_)) {
                    *coming.entry(h).or_default() += 1;
                }
            }
        }
        self.containers
            .homes
            .iter()
            .filter(|(b, h)| !h.done && self.buildings.get(**b).is_some_and(|x| x.construction.is_some()) && (h.boxes.len() as u16 + coming.get(*b).copied().unwrap_or(0)) < h.want)
            .filter_map(|(b, _)| Some((*b, self.place_pos(*b)?.distance(p))))
            .filter(|(_, d)| *d <= cx::DEVELOPER_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(b, _)| b)
    }

    /// [K34] A stack developer building `site` buys box `id`: the money to
    /// its owner (a yard in town, or the line out of town).
    fn sell_to_developer(&mut self, id: BoxId, site: BuildingId, how: Event) {
        let day = self.today();
        let tick = self.clock.tick;
        let Some(r) = self.containers.boxes.get_mut(id) else { return };
        let price = r.price(day);
        let seller = match r.owner {
            Owner::Yard(y) if self.buildings.contains_key(y) => Account::Business(y),
            _ => Account::Outside,
        };
        r.owner = Owner::Home(site);
        r.note(tick, how, site);
        self.containers.stats.sold_to_stacks_total += 1;
        if how == Event::BoughtOffTruck {
            self.containers.stats.sold_off_truck_total += 1;
        }
        let payer = self.site_payer(site);
        if let Account::Business(y) = seller {
            if let Some(z) = self.buildings.get_mut(y).and_then(|b| b.business.as_mut()) {
                z.revenue_today += price;
            }
        }
        self.transfer(payer, seller, price, crate::bank::Kind::Building);
    }

    /// [K34] A building just opened: a stack developer's home takes its
    /// boxes in (those still missing bought out of town), and is drawn
    /// from them.
    pub(super) fn box_home_finished(&mut self, b: BuildingId) {
        if !self.containers.live {
            return;
        }
        if !self.containers.homes.contains_key(&b) {
            // Built faster than the hourly round saw it: decided now.
            let mut cand = self.box_home_candidate(b);
            if cand {
                // (`box_home_candidate` asks for a site; this one just opened.)
                cand = self.buildings.get(b).is_some_and(|x| x.construction.is_none());
            }
            if !cand {
                return;
            }
            let want = self.home_want(b);
            self.containers.homes.insert(b, BoxHome { want, boxes: Vec::new(), done: false });
        }
        let tick = self.clock.tick;
        let (want, have) = {
            let h = &self.containers.homes[&b];
            (h.want, h.boxes.clone())
        };
        // What is on its way stays its owners': the home is complete.
        let late: Vec<BoxId> = self.containers.boxes.iter().filter(|(id, r)| r.owner == Owner::Home(b) && !have.contains(id)).map(|(id, _)| id).collect();
        for id in late {
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.owner = Owner::Line;
            }
            for t in self.containers.trucks.values_mut() {
                if t.boxes.contains(&id) && t.to == Some(b) {
                    t.to = None;
                }
            }
        }
        let mut boxes = have;
        while (boxes.len() as u16) < want {
            let kind = if crate::hash::unit(mix(self.containers.stats.made, 9)) < 0.12 { BoxKind::Reefer } else { BoxKind::Dry };
            let forty = crate::hash::unit(mix(self.containers.stats.made, 10)) < 0.3;
            let id = self.make_box(kind, forty, Where::Home(b), None, Event::BoughtOutOfTown, b);
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.owner = Owner::Home(b);
            }
            self.containers.stats.bought_out_of_town_for_homes += 1;
            boxes.push(id);
        }
        for &id in &boxes {
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.at = Where::Home(b);
                r.owner = Owner::Home(b);
                r.since = tick;
                r.note(tick, Event::BuiltIntoHome, b);
            }
        }
        let h = self.containers.homes.get_mut(&b).unwrap();
        h.boxes = boxes;
        h.done = true;
        self.containers.stats.homes_built_total += 1;
        self.containers.touch();
    }

    // ── the rounds ───────────────────────────────────────────────────────────

    /// [K34] The hourly round: boxes of lorries and trains gone, new stack
    /// developer sites, and boxes trucked from the yards to them.
    pub(super) fn hourly_containers(&mut self) {
        if !self.containers.live {
            return;
        }
        // Trains gone off the map with their boxes.
        let on_trains: std::collections::BTreeSet<BoxId> = self.rail.trains.values().flat_map(|t| t.boxes.iter().filter_map(|b| b.id)).collect();
        let gone: Vec<BoxId> = self.containers.boxes.iter().filter(|(id, r)| r.at == Where::Train && !on_trains.contains(id)).map(|(id, _)| id).collect();
        for id in gone {
            // (Counted as sent out when they went on.)
            self.containers.boxes.remove(id);
            self.containers.touch();
        }
        // Lorries no longer on the road.
        let lost: Vec<crate::traffic::CarId> = self.containers.trucks.keys().copied().filter(|c| !self.traffic.cars.contains_key(*c)).collect();
        for car in lost {
            let last = self.containers.trucks[&car].boxes.first().and_then(|id| self.containers.boxes.get(*id)).and_then(|r| r.log.last()).map(|s| s.2).unwrap_or_default();
            self.boxes_check_truck(car, last);
        }
        // New developer sites.
        let sites: Vec<BuildingId> = self.buildings.iter().filter(|(id, b)| b.construction.is_some() && b.land_use == LandUse::Residential && !self.containers.homes.contains_key(id)).map(|(id, _)| id).collect();
        for b in sites {
            if self.box_home_candidate(b) {
                let want = self.home_want(b);
                self.containers.homes.insert(b, BoxHome { want, boxes: Vec::new(), done: false });
            }
        }
        // Homes pulled down (or being rebuilt): their boxes salvaged.
        let razed: Vec<BuildingId> = self.containers.homes.iter().filter(|(b, h)| !self.buildings.contains_key(**b) || (h.done && self.buildings.get(**b).is_some_and(|x| x.construction.is_some() || x.land_use != LandUse::Residential))).map(|(b, _)| *b).collect();
        for b in razed {
            self.salvage_home(b);
        }
        // Developers short of boxes buy them at the yards.
        let short: Vec<(BuildingId, usize)> = self
            .containers
            .homes
            .iter()
            .filter(|(b, h)| !h.done && self.buildings.get(**b).is_some_and(|x| x.construction.is_some()))
            .map(|(b, h)| {
                let coming = self.containers.boxes.values().filter(|r| r.owner == Owner::Home(*b) && !matches!(r.at, Where::Site(_))).count();
                (*b, (h.want as usize).saturating_sub(h.boxes.len() + coming))
            })
            .filter(|s| s.1 > 0)
            .collect();
        for (site, need) in short {
            self.developer_buys(site, need.min(cx::HAULS_PER_HOUR));
        }
    }

    /// [K34] The developer of `site` buys up to `n` empties at the nearest
    /// yard that has them (sound boxes, the cheaper worn ones first) and
    /// has them trucked over.
    fn developer_buys(&mut self, site: BuildingId, n: usize) {
        let Some(p) = self.place_pos(site) else { return };
        let day = self.today();
        let mut best: Option<(f32, BuildingId)> = None;
        for (d, kind, pos) in self.box_depots() {
            if kind == DepotKind::Scrap || pos.distance(p) > cx::DEVELOPER_REACH {
                continue;
            }
            // What the yard can spare over the stock it keeps.
            let spare = self.containers.boxes.values().filter(|r| r.at == Where::Stored(d) && r.goods.is_none() && !matches!(r.owner, Owner::Home(_)) && !r.for_scrap(day)).count();
            let has = spare as f32 > self.box_capacity(d, kind) as f32 * cx::DEPOT_RESERVE;
            if has && best.is_none_or(|(bd, bid)| pos.distance(p) < bd || (pos.distance(p) == bd && d < bid)) {
                best = Some((pos.distance(p), d));
            }
        }
        let Some((_, depot)) = best else { return };
        let mut pick: Vec<(i32, u64, BoxId)> = self
            .containers
            .boxes
            .iter()
            .filter(|(_, r)| r.at == Where::Stored(depot) && r.goods.is_none() && !matches!(r.owner, Owner::Home(_)) && !r.for_scrap(day))
            .map(|(id, r)| (-((r.wear(day) * 100.0) as i32), r.since, id))
            .collect();
        pick.sort();
        let keep = self.box_depot(depot).map_or(0.0, |k| self.box_capacity(depot, k) as f32 * cx::DEPOT_RESERVE).ceil() as usize;
        let n = n.min(pick.len().saturating_sub(keep));
        let ids: Vec<BoxId> = pick.into_iter().take(n).map(|p| p.2).collect();
        for &id in &ids {
            self.sell_to_developer(id, site, Event::SoldToDeveloper);
        }
        if !self.haul_boxes(depot, site, &ids) {
            for id in ids {
                self.box_lands(id, site);
            }
        }
    }

    /// [K34] A box home pulled down: worn boxes to the scrap yard, the rest
    /// back into storage.
    fn salvage_home(&mut self, b: BuildingId) {
        let Some(h) = self.containers.homes.remove(&b) else { return };
        let day = self.today();
        let near = self.containers.boxes.get(h.boxes.first().copied().unwrap_or_default()).and_then(|r| r.log.last()).and_then(|s| self.place_pos(s.2)).unwrap_or_default();
        let scrap = self.nearest_depot(near, |k| k == DepotKind::Scrap);
        for id in h.boxes {
            let Some(r) = self.containers.boxes.get_mut(id) else { continue };
            r.owner = Owner::Line;
            let old = r.for_scrap(day);
            match scrap.filter(|_| old) {
                Some(y) => {
                    if let Some(r) = self.containers.boxes.get_mut(id) {
                        r.owner = Owner::Scrap(y);
                    }
                    self.box_to(id, Where::Scrap(y), Event::Salvaged, y);
                }
                None => match self.nearest_depot(near, |k| k != DepotKind::Scrap) {
                    Some(d) => self.box_to(id, Where::Stored(d), Event::Salvaged, d),
                    None => self.box_gone(id, Event::SoldOutOfTown),
                },
            }
        }
    }

    /// [K34] The daily round, at midnight: the scrap yards cut up what
    /// they hold; the owners go through their empties (scrap the worn-out
    /// ones, a yard buys the older ones its lines leave, the extra goes back
    /// out, some are sold out of town); a full box nobody fetched is
    /// unpacked where it stands.
    pub(super) fn daily_containers(&mut self) {
        if !self.containers.live {
            return;
        }
        let day = self.today();
        let tick = self.clock.tick;
        self.cut_up_boxes();
        let depots = self.box_depots();
        let scrap_yards: Vec<(BuildingId, Vec2)> = depots.iter().filter(|d| d.1 == DepotKind::Scrap).map(|d| (d.0, d.2)).collect();
        // Room on each scrap yard's pile today: what it cuts up in a day.
        let mut room: BTreeMap<BuildingId, usize> = scrap_yards.iter().map(|&(y, _)| {
            let pile = self.containers.boxes.values().filter(|r| r.at == Where::Scrap(y) || (r.owner == Owner::Scrap(y) && matches!(r.at, Where::Truck(_)))).count();
            (y, self.box_capacity(y, DepotKind::Scrap).saturating_sub(pile))
        }).collect();
        let mut due_scrap = 0u32;
        let mut awaiting = 0u32;
        let mut to_scrap: BTreeMap<(BuildingId, BuildingId), Vec<BoxId>> = BTreeMap::new();
        for (d, kind, pos) in depots.iter().copied() {
            if kind == DepotKind::Scrap {
                continue;
            }
            let cap = self.box_capacity(d, kind);
            let ids = self.containers.stored_at(d);
            let mut count = ids.len();
            for id in ids {
                let Some(r) = self.containers.boxes.get(id) else { continue };
                let days = tick.saturating_sub(r.since) / crate::clock::MINUTES_PER_DAY;
                let h = mix(r.seed(), day as u64);
                if r.goods.is_some() {
                    if days >= 2 {
                        // Nobody came for it: unpacked here.
                        let r = self.containers.boxes.get_mut(id).unwrap();
                        r.goods = None;
                        r.note(tick, Event::Unpacked, d);
                    }
                    continue;
                }
                if matches!(r.owner, Owner::Home(_) | Owner::Scrap(_)) {
                    continue;
                }
                if r.for_scrap(day) {
                    match scrap_yards.iter().filter(|(y, _)| room.get(y).is_some_and(|n| *n > 0)).min_by(|a, b| a.1.distance_squared(pos).total_cmp(&b.1.distance_squared(pos)).then(a.0.cmp(&b.0))) {
                        Some(&(y, _)) => {
                            *room.get_mut(&y).unwrap() -= 1;
                            to_scrap.entry((d, y)).or_default().push(id)
                        }
                        None if !scrap_yards.is_empty() => {
                            // The scrap yards are full today: it waits its turn.
                            awaiting += 1;
                        }
                        None => {
                            awaiting += 1;
                            if days <= 1 {
                                due_scrap += 1;
                            }
                            if days >= cx::SCRAP_WAIT_DAYS {
                                self.box_gone(id, Event::SoldOutOfTown);
                                count -= 1;
                            }
                        }
                    }
                    continue;
                }
                let line_owned = r.owner == Owner::Line;
                let wear = r.wear(day);
                if line_owned && kind == DepotKind::Yard && days >= cx::YARD_BUYS_AFTER_DAYS && wear > 0.35 {
                    // The line lets its older boxes go to the yard.
                    let price = r.price(day) * 0.6;
                    let r = self.containers.boxes.get_mut(id).unwrap();
                    r.owner = Owner::Yard(d);
                    r.note(tick, Event::BoughtByYard, d);
                    self.containers.stats.bought_by_yards_total += 1;
                    if let Some(z) = self.buildings.get_mut(d).and_then(|b| b.business.as_mut()) {
                        z.spend_today += price;
                    }
                    self.transfer(Account::Business(d), Account::Outside, price, crate::bank::Kind::Imports);
                    continue;
                }
                let over = count > cap;
                let roll = crate::hash::unit(h);
                if line_owned && (over || (days >= 2 && roll < 0.12)) {
                    // Repositioned: the line wants it elsewhere.
                    self.box_gone(id, Event::SentOut);
                    count -= 1;
                } else if !line_owned && (over || roll < 0.05) {
                    // The yard sells it to a buyer out of town.
                    let price = r.price(day);
                    if let Some(z) = self.buildings.get_mut(d).and_then(|b| b.business.as_mut()) {
                        z.revenue_today += price;
                    }
                    self.transfer(Account::Outside, Account::Business(d), price, crate::bank::Kind::Exports);
                    self.box_gone(id, Event::SoldOutOfTown);
                    count -= 1;
                }
            }
        }
        // Worn-out boxes to the scrap yards, a lorry at a time.
        for ((from, yard), ids) in to_scrap {
            for chunk in ids.chunks(2) {
                for &id in chunk {
                    if let Some(r) = self.containers.boxes.get_mut(id) {
                        r.owner = Owner::Scrap(yard);
                    }
                }
                if !self.haul_boxes(from, yard, chunk) {
                    for &id in chunk {
                        self.box_to(id, Where::Scrap(yard), Event::ToScrap, yard);
                    }
                }
            }
        }
        let s = &mut self.containers.stats;
        s.due_scrap_today = due_scrap;
        s.awaiting_scrap = awaiting;
        s.arrived_today = [0; 3];
        s.delivered_today = 0;
        s.scrapped_today = 0;
    }

    /// [K34] The scrap yards cut up the boxes on their piles: the steel
    /// into their stock (to sell to building sites and out of town), the
    /// scrap price to the box's last owner.
    fn cut_up_boxes(&mut self) {
        let tick = self.clock.tick;
        let piles: Vec<(BoxId, BuildingId)> = self.containers.boxes.iter().filter_map(|(id, r)| if let Where::Scrap(y) = r.at { Some((id, y)) } else { None }).collect();
        for (id, y) in piles {
            if self.box_depot(y) != Some(DepotKind::Scrap) {
                // The yard closed: the pile goes for scrap out of town.
                self.box_gone(id, Event::SoldOutOfTown);
                continue;
            }
            let Some(r) = self.containers.boxes.get(id) else { continue };
            // A box lands in the morning and is cut up the next night.
            if tick.saturating_sub(r.since) < crate::clock::MINUTES_PER_DAY / 2 {
                continue;
            }
            let steel = r.steel();
            let pay = steel * cx::SCRAP_PRICE_PER_STEEL;
            let seller = match r.log.iter().rev().find_map(|s| (s.1 == Event::BoughtByYard).then_some(s.2)) {
                Some(yd) if self.buildings.contains_key(yd) => Account::Business(yd),
                _ => Account::Outside,
            };
            if let Some(z) = self.buildings.get_mut(y).and_then(|b| b.business.as_mut()) {
                z.add(Resource::Metal, steel);
                z.spend_today += pay;
            }
            self.transfer(Account::Business(y), seller, pay, crate::bank::Kind::Supplies);
            if let Some(r) = self.containers.boxes.get_mut(id) {
                r.note(tick, Event::CutUp, y);
            }
            self.containers.boxes.remove(id);
            let s = &mut self.containers.stats;
            s.scrapped_total += 1;
            s.scrapped_today += 1;
            s.steel_total += steel;
            self.containers.touch();
        }
    }

    /// [K34] The scrap yard with steel for a building site at `p`: the
    /// nearest with `amount` or more in stock.
    pub(super) fn scrap_steel_for(&self, p: Vec2, amount: f32) -> Option<BuildingId> {
        self.box_depots()
            .into_iter()
            .filter(|d| d.1 == DepotKind::Scrap)
            .filter(|d| self.buildings.get(d.0).and_then(|b| b.business.as_ref()).is_some_and(|z| z.stock_of(Resource::Metal) >= amount))
            .min_by(|a, b| a.2.distance_squared(p).total_cmp(&b.2.distance_squared(p)).then(a.0.cmp(&b.0)))
            .map(|d| d.0)
    }

    // ── loading an older save ────────────────────────────────────────────────

    /// [K34] A save from before the records: its depots get a plausible
    /// stock of empties (mixed lines, ages and wear, some of them old enough
    /// for scrap), and the boxes on its freight trains their records.
    pub(super) fn containers_after_load(&mut self) {
        if self.containers.live {
            return;
        }
        self.containers.live = true;
        let tick = self.clock.tick;
        for (d, kind, _) in self.box_depots() {
            let cap = self.box_capacity(d, kind);
            let fill = match kind {
                DepotKind::Yard => 0.55,
                DepotKind::Port => 0.45,
                DepotKind::Rail => 0.4,
                DepotKind::Hub => 0.3,
                DepotKind::Scrap => 0.35,
            };
            let n = (cap as f32 * fill).round() as usize;
            for k in 0..n {
                let h = mix(slotmap::Key::data(&d).as_ffi(), k as u64);
                let u = crate::hash::unit(mix(h, 1));
                let kind_box = if u < 0.1 { BoxKind::Reefer } else if u < 0.14 { BoxKind::Tank } else if u < 0.18 { BoxKind::OpenTop } else { BoxKind::Dry };
                let forty = crate::hash::unit(mix(h, 2)) < 0.6;
                let at = if kind == DepotKind::Scrap { Where::Scrap(d) } else { Where::Stored(d) };
                let id = self.make_box(kind_box, forty, at, None, Event::Counted, d);
                let r = self.containers.boxes.get_mut(id).unwrap();
                r.since = tick.saturating_sub((crate::hash::unit(mix(h, 3)) * 6.0 * crate::clock::MINUTES_PER_DAY as f32) as u64);
                if kind == DepotKind::Scrap {
                    r.knocks = r.knocks.saturating_add(160);
                    r.owner = Owner::Scrap(d);
                } else if kind == DepotKind::Yard && crate::hash::unit(mix(h, 4)) < 0.4 {
                    r.owner = Owner::Yard(d);
                }
            }
        }
        let trains: Vec<crate::rail::TrainId> = self.rail.trains.keys().collect();
        for t in trains {
            let mut boxes = std::mem::take(&mut self.rail.trains[t].boxes);
            for b in boxes.iter_mut().filter(|b| b.id.is_none()) {
                let goods = b.resource.map(|resource| Goods { resource, buyer: BuildingId::default() });
                b.id = Some(self.make_box(b.kind, b.forty, Where::Train, goods, Event::Counted, BuildingId::default()));
            }
            self.rail.trains[t].boxes = boxes;
        }
    }

    // ── for the inspector ────────────────────────────────────────────────────

    pub fn box_counts(&self) -> BoxCounts {
        let mut c = BoxCounts::default();
        for r in self.containers.boxes.values() {
            c.total += 1;
            match r.at {
                Where::Train => c.on_trains += 1,
                Where::Truck(_) => c.on_trucks += 1,
                Where::Stored(_) => {
                    c.stored += 1;
                    if r.goods.is_some() {
                        c.stored_full += 1;
                    }
                }
                Where::Site(_) => c.on_sites += 1,
                Where::Home(_) => c.in_homes += 1,
                Where::Scrap(_) => c.at_scrap += 1,
            }
        }
        c
    }

    fn place_name(&self, b: BuildingId) -> String {
        if b.is_null() {
            return "out of town".to_string();
        }
        self.buildings.get(b).map(|x| x.name.clone()).unwrap_or_else(|| "a place since gone".to_string())
    }

    /// [K34] The boxes on the lorry `car`.
    pub fn boxes_on_truck(&self, car: crate::traffic::CarId) -> Vec<BoxId> {
        self.containers.trucks.get(&car).map(|t| t.boxes.clone()).unwrap_or_default()
    }

    /// [K34] Box `id`'s life story.
    pub fn box_story(&self, id: BoxId) -> Option<BoxStory> {
        let r = self.containers.boxes.get(id)?;
        let day = self.today();
        let line = cx::LINES[r.line as usize % cx::LINES.len()];
        let m = r.marks(day);
        let mut marks = Vec::new();
        if m.patches > 0 {
            marks.push(format!("{} patched panel{}", m.patches, if m.patches == 1 { "" } else { "s" }));
        }
        if m.dents > 0 {
            marks.push(format!("{} dent{}", m.dents, if m.dents == 1 { "" } else { "s" }));
        }
        if m.scuffed {
            marks.push("number scuffed".to_string());
        }
        if m.repainted {
            marks.push("number painted over and stencilled again".to_string());
        }
        let (owner, owner_id) = match r.owner {
            Owner::Line => (line.name.to_string(), None),
            Owner::Yard(y) => (self.place_name(y), Some(y)),
            Owner::Home(h) => (format!("the stack developer of {}", self.place_name(h)), Some(h)),
            Owner::Scrap(y) => (self.place_name(y), Some(y)),
        };
        let mut car = None;
        let (now, now_id) = match r.at {
            Where::Train => ("on a freight train".to_string(), None),
            Where::Truck(c) => {
                car = Some(c);
                let to = self.traffic.cars.get(c).and_then(|x| x.cargo).map(|g| g.to).filter(|b| !b.is_null());
                (match to {
                    Some(b) => format!("on a lorry to {}", self.place_name(b)),
                    None => "on a lorry".to_string(),
                }, to)
            }
            Where::Stored(b) => (format!("{} at {}", if r.goods.is_some() { "stored full" } else { "stored empty" }, self.place_name(b)), Some(b)),
            Where::Site(b) => (format!("on the building site of {}", self.place_name(b)), Some(b)),
            Where::Home(b) => (format!("part of the home {}", self.place_name(b)), Some(b)),
            Where::Scrap(b) => (format!("on the scrap pile at {}", self.place_name(b)), Some(b)),
        };
        Some(BoxStory {
            number: r.number(),
            line: line.name.to_string(),
            size: if r.forty { "40 ft".to_string() } else { "20 ft".to_string() },
            kind: r.kind.label().to_string(),
            origin: cx::PORTS[r.origin as usize % cx::PORTS.len()].to_string(),
            built_year_ago: r.age_years(day),
            condition: r.condition_label(day).to_string(),
            wear: r.wear(day),
            marks,
            owner,
            owner_id,
            now,
            now_id,
            car,
            goods: r.goods.map(|g| g.resource.label().to_string()),
            visits: r.visits,
            history: r.log.iter().map(|s| (s.0, s.1.label().to_string(), self.place_name(s.2), (!s.2.is_null()).then_some(s.2))).collect(),
        })
    }
}
