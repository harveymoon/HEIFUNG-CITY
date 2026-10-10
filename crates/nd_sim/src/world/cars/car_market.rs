//! [carlife] Cars coming and going: bought, delivered, sold, emigrating.

use super::*;
use crate::vehicle::VehicleClass;

/// [carlife] Cars a dealer's forecourt holds, the gap between them (m),
/// the days used stock stands before it goes to the trade out of town, the
/// hours its drivers work and the cars they fetch an hour.
pub(in crate::world) const FORECOURT_CARS: u16 = 12;
const FORECOURT_PITCH: f32 = 3.2;
const STOCK_DAYS: u64 = 3;
const DEALER_HOURS: (u64, u64) = (8, 19);
const FETCH_PER_HOUR: u32 = 3;
/// [carlife] Cars a dealer delivers an hour, and sold cars the trade out of
/// town fetches an hour across the city when dealers cannot keep up.
const DELIVER_PER_HOUR: u32 = 4;
const TRADE_PER_HOUR: u32 = 12;

impl World {
    pub(in crate::world) fn new_car(&mut self, owner: CitizenId, model: VehicleModel, place: CarPlace, event: CarEvent) -> VehicleId {
        let color = self.rng.random_range(0..8u8);
        let vid = self.vehicles.cars.insert(OwnedCar { owner, model, place, color, since: self.clock.tick, spare: false, usual: false, dealer: None });
        self.vehicles.note(event);
        if let Some(c) = self.citizens.get_mut(owner) {
            c.car = Some(vid);
            c.owns_car = true;
            c.vehicle = Some(model);
        }
        vid
    }

    /// A car leaves the city for good: sold, scrapped or driven away.
    pub(in crate::world) fn drop_car(&mut self, vid: VehicleId, event: CarEvent) {
        if self.vehicles.cars.get(vid).and_then(|c| c.spot()).is_some() {
            self.unpark(vid);
        }
        let Some(car) = self.vehicles.cars.remove(vid) else { return };
        self.vehicles.note(event);
        if let Some(c) = self.citizens.get_mut(car.owner).filter(|c| c.car == Some(vid)) {
            c.car = None;
            c.owns_car = false;
            c.parked_at = None;
        }
    }

    /// A newcomer drives in from out of town with their car, to `home`.
    /// False when there is no road in; they come without it.
    pub(in crate::world) fn arrive_by_car(&mut self, id: CitizenId, model: VehicleModel, home: BuildingId) -> bool {
        let Some(lot) = self.building_lot(home) else { return false };
        let Some((ct_seg, ct_pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return false };
        let Some((route, s0, s1)) = self.route_from_edge(ct_seg, ct_pt) else { return false };
        let color = self.rng.random_range(0..8u8);
        let car = self.traffic.spawn_car(id, None, route, s0, s1, color);
        let vid = self.new_car(id, model, CarPlace::Driving(car), CarEvent::ArrivedWithHousehold);
        let (pos, heading) = {
            let c = &mut self.traffic.cars[car];
            c.model = model;
            c.owned = Some(vid);
            (c.pos, c.heading)
        };
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        c.pos = pos;
        c.prev_pos = pos;
        c.heading = heading;
        c.activity = Activity::Driving { car, to: Some(home), purpose: Purpose::Home };
        c.commute.mode = Some(TripMode::Car);
        c.commute.started = tick;
        c.commute.distance = pos.distance(ct_pt);
        true
    }

    /// The car dealer for a buyer at `p`: an open shop selling goods with a
    /// street in front (the gadget shops double as dealers), else any open
    /// shop. `None` when the town has none: cars come from out of town.
    pub(in crate::world) fn dealer_near(&self, p: Vec2) -> Option<BuildingId> {
        self.specialist_near(p, BusinessKind::CarDealer, CAR_DEALER_REACH).or_else(|| self.any_shop_near(p))
    }

    /// [economy] Where a bike is bought: a bike shop in reach, else as a
    /// car (a goods or gadget shop doubles).
    pub(in crate::world) fn bike_shop_near(&self, p: Vec2) -> Option<BuildingId> {
        self.specialist_near(p, BusinessKind::BikeShop, BIKE_SHOP_REACH).or_else(|| self.any_shop_near(p))
    }

    /// [economy] The nearest open business of `kind` with a street in
    /// front within `reach`.
    pub(super) fn specialist_near(&self, p: Vec2, kind: BusinessKind, reach: f32) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == kind && !z.closed))
            .filter_map(|(id, b)| self.lots.get(b.lot).filter(|l| l.car_access()).map(|l| (l.center.distance(p), id)))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id)| id)
    }

    pub(super) fn any_shop_near(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Commercial && b.operating() && b.port.is_none() && b.business.as_ref().is_some_and(|z| !z.closed && !b.is_parking()))
            .filter(|(id, _)| self.building_lot(*id).is_some_and(|l| l.car_access()))
            .filter_map(|(id, b)| {
                let goods = b.business.as_ref().is_some_and(|z| z.sells(Resource::Goods) || z.sells(Resource::Electronics));
                self.lots.get(b.lot).map(|l| (l.center.distance(p) + if goods { 0.0 } else { 600.0 }, id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// Buy a car: at a dealer in town (it is delivered home from the
    /// forecourt), else from out of town (driven in from the map edge).
    /// Returns what was paid, or `None` when it could not be done.
    pub(in crate::world) fn buy_car(&mut self, id: CitizenId, model: VehicleModel, price: f32) -> Option<f32> {
        let c = self.citizens.get(id)?;
        if c.car.is_some() {
            return None;
        }
        let home = c.home?;
        let at = self.building_entrance(home)?;
        let tick = self.clock.tick;
        // [carlife] At a dealer in town: a used car off its forecourt, or a
        // new one put there; either waits on the forecourt until the
        // dealer's driver delivers it (a few an hour, `hourly_vehicles`).
        // With the forecourt full it comes from out of town.
        if let Some(dealer) = self.dealer_near(at) {
            if let Some(vid) = self.take_stock(dealer, model.class) {
                let used = self.vehicles.cars[vid].model;
                let paid = price.min(used.price() * 0.6);
                let car = &mut self.vehicles.cars[vid];
                car.owner = id;
                car.dealer = None;
                car.since = tick;
                let c = &mut self.citizens[id];
                c.car = Some(vid);
                c.owns_car = true;
                c.vehicle = Some(used);
                let name = self.buildings[dealer].name.clone();
                if let Some(bz) = self.buildings[dealer].business.as_mut() {
                    bz.money += paid * 0.5;
                    bz.revenue_today += paid * 0.5;
                    bz.deals += 1;
                }
                self.vehicles.stats.bought_today += 1;
                self.citizens[id].remember(tick, format!("Bought a used {} at {name}.", model_label(&used)));
                return Some(paid);
            }
            if let Some(spot) = self.free_forecourt(dealer) {
                self.new_car(id, model, CarPlace::Parked(spot), CarEvent::Bought);
                // The forecourt is not a parking space anyone counts.
                let name = self.buildings[dealer].name.clone();
                if let Some(bz) = self.buildings[dealer].business.as_mut() {
                    // The dealer's margin; the rest goes to the maker out of town.
                    bz.money += price * 0.25;
                    bz.revenue_today += price * 0.25;
                    bz.deals += 1;
                }
                self.vehicles.stats.bought_today += 1;
                self.citizens[id].remember(tick, format!("Bought a {} at {name}.", model_label(&model)));
                return Some(price);
            }
        }
        let lot = self.building_lot(home)?;
        let (ct_seg, ct_pt) = lot.car_segment.map(|s| (s, lot.car_point))?;
        let (route, s0, s1) = self.route_from_edge(ct_seg, ct_pt)?;
        let color = self.rng.random_range(0..8u8);
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, color);
        let vid = self.new_car(id, model, CarPlace::Driving(car), CarEvent::BoughtOutOfTown);
        let c = &mut self.traffic.cars[car];
        c.model = model;
        c.owned = Some(vid);
        c.errand = Errand::Deliver;
        self.vehicles.stats.bought_today += 1;
        self.citizens[id].remember(tick, format!("Ordered a {} from out of town. It's being driven in.", model_label(&model)));
        Some(price)
    }

    /// Drive a car waiting on a dealer's forecourt to its owner's home.
    pub(in crate::world) fn deliver_car(&mut self, vid: VehicleId) {
        let Some(owner) = self.vehicles.cars.get(vid).map(|c| c.owner) else { return };
        let Some(home) = self.citizens.get(owner).and_then(|c| c.home) else { return };
        let Some(lot) = self.building_lot(home) else { return };
        let Some((seg, pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return };
        self.drive_from_spot(vid, seg, pt, CitizenId::default(), Errand::Deliver);
    }

    /// Sell the citizen's car: it becomes the dealer's (out of town without
    /// a dealer in reach) and stays where it is parked until the dealer's
    /// driver fetches it during working hours (`hourly_dealers`), so a
    /// morning's sales do not all set off in one line at the same minute.
    /// A car on the road is not sold. Returns the money got.
    pub(in crate::world) fn sell_car(&mut self, id: CitizenId, reason: &str) -> Option<f32> {
        let vid = self.citizens.get(id)?.car?;
        let car = self.vehicles.cars.get(vid)?;
        let spot = *car.spot()?;
        // [parking] Not while it is on a tow truck or in the pound.
        if matches!(spot.kind, ParkKind::Hooked | ParkKind::Pound) || spot.booted {
            return None;
        }
        let worth = (car.model.price() * 0.5).min(400.0);
        let tick = self.clock.tick;
        let dealer = self.dealer_near(spot.pos);
        if let Some(bz) = dealer.and_then(|d| self.buildings.get_mut(d)).and_then(|b| b.business.as_mut()) {
            bz.deals += 1;
        }
        // The owner is done with it: it is the dealer's now. (A space it
        // holds at home is let go of only when it is fetched.)
        let car = &mut self.vehicles.cars[vid];
        car.owner = CitizenId::default();
        car.dealer = Some(dealer.unwrap_or_default());
        car.since = tick;
        car.spare = false;
        car.usual = false;
        let c = &mut self.citizens[id];
        c.car = None;
        c.owns_car = false;
        c.vehicle = None;
        c.parked_at = None;
        c.remember(tick, reason.to_string());
        self.vehicles.stats.sold_today += 1;
        Some(worth)
    }

    /// [carlife] Where the `n`th car stands on dealer `d`'s forecourt: rows
    /// inside its lot behind the frontage, facing the street.
    pub(in crate::world) fn forecourt_spot(&self, d: BuildingId, n: u16) -> Option<ParkSpot> {
        let mut spot = self.building_spot(ParkKind::Dealer, d)?;
        let lot = self.building_lot(d)?;
        let inward = (-lot.front_dir).normalize_or_zero();
        let along = Vec2::new(-inward.y, inward.x);
        let front = (lot.front_edge[0] + lot.front_edge[1]) * 0.5;
        let width = lot.front_edge[0].distance(lot.front_edge[1]);
        let per_row = ((width - 4.0) / FORECOURT_PITCH).floor().max(1.0) as u16;
        let (row, col) = (n / per_row, n % per_row);
        let lateral = (col as f32 - (per_row - 1) as f32 * 0.5) * FORECOURT_PITCH;
        let p = front + inward * (4.0 + row as f32 * 6.0) + along * lateral;
        spot.pos = if crate::geom::point_in_polygon(p, &lot.polygon) { p } else { lot.center };
        spot.heading = lot.front_dir.y.atan2(lot.front_dir.x);
        spot.slot = n;
        Some(spot)
    }

    /// [carlife] Cars on dealer `d`'s forecourt (its used stock).
    pub(in crate::world) fn forecourt_cars(&self, d: BuildingId) -> Vec<VehicleId> {
        self.vehicles.cars.iter().filter(|(_, c)| c.dealer == Some(d) && c.spot().is_some_and(|s| s.kind == ParkKind::Dealer && s.building == Some(d))).map(|(id, _)| id).collect()
    }

    /// [carlife] A free place on the forecourt of `d`, if there is room.
    fn free_forecourt(&self, d: BuildingId) -> Option<ParkSpot> {
        let taken: std::collections::HashSet<u16> = self.vehicles.cars.values().filter_map(|c| c.spot().filter(|s| s.kind == ParkKind::Dealer && s.building == Some(d)).map(|s| s.slot)).collect();
        let n = (0..FORECOURT_CARS).find(|n| !taken.contains(n))?;
        self.forecourt_spot(d, n)
    }

    /// [carlife] Put a car that is the dealer's onto forecourt spot `spot`.
    fn onto_forecourt(&mut self, vid: VehicleId, d: BuildingId, spot: ParkSpot) {
        let car = &mut self.vehicles.cars[vid];
        car.owner = CitizenId::default();
        car.dealer = Some(d);
        car.place = CarPlace::Driving(CarId::default());
        self.park_at(vid, spot);
    }

    /// [carlife] A car being sold reached the end of its route: onto the
    /// dealer's forecourt as used stock; at the map edge (sold to the trade
    /// out of town) it leaves the city; with the forecourt full it drives
    /// on out of town. It never vanishes in front of the dealer.
    pub(in crate::world) fn sale_arrived(&mut self, car_id: CarId, d: BuildingId, vid: Option<VehicleId>) {
        let at_edge = self.at_map_edge(car_id);
        let pos = self.traffic.cars.get(car_id).map(|c| c.pos).unwrap_or_default();
        let Some(vid) = vid.filter(|v| self.vehicles.cars.contains_key(*v)) else {
            if !at_edge && self.redirect_out(car_id) {
                self.traffic.cars[car_id].errand = Errand::Sale(BuildingId::default());
                return;
            }
            self.traffic.remove_car(car_id);
            return;
        };
        let dealer_here = self.buildings.contains_key(d);
        if dealer_here {
            if let Some(spot) = self.free_forecourt(d) {
                self.traffic.remove_car(car_id);
                self.onto_forecourt(vid, d, spot);
                return;
            }
        }
        if at_edge {
            self.traffic.remove_car(car_id);
            self.drop_car(vid, CarEvent::Sold);
            return;
        }
        if self.redirect_out(car_id) {
            self.traffic.cars[car_id].errand = Errand::Sale(BuildingId::default());
            return;
        }
        // No way out from here: it parks (on the forecourt, squeezed in,
        // or by the road) and the trade fetches it another day.
        self.traffic.remove_car(car_id);
        let n = if dealer_here { self.forecourt_cars(d).len() as u16 } else { 0 };
        match dealer_here.then(|| self.forecourt_spot(d, n)).flatten() {
            Some(spot) => self.onto_forecourt(vid, d, spot),
            None => {
                let car = &mut self.vehicles.cars[vid];
                car.owner = CitizenId::default();
                car.dealer = Some(BuildingId::default());
                car.place = CarPlace::Driving(CarId::default());
                self.park_stranded(vid, pos);
            }
        }
    }

    /// [carlife] Hourly in working hours: dealers' drivers fetch the cars
    /// they bought (a few an hour each, each car from an hour of its own),
    /// and stock that has stood on a forecourt for `STOCK_DAYS` goes out of
    /// town to the trade. A car whose dealer has closed is fetched by the
    /// trade instead.
    pub(in crate::world) fn hourly_dealers(&mut self) {
        self.recall_stuck_deliveries();
        let hour = self.clock.hour();
        if !(DEALER_HOURS.0..DEALER_HOURS.1).contains(&hour) {
            return;
        }
        let tick = self.clock.tick;
        let day = self.clock.day();
        let span = DEALER_HOURS.1 - DEALER_HOURS.0;
        // Cars sold waiting on forecourts go out to their new owners, a
        // few an hour from each dealer.
        let waiting: Vec<(VehicleId, Option<BuildingId>)> = self.vehicles.cars.iter().filter(|(_, c)| c.dealer.is_none()).filter_map(|(id, c)| c.spot().filter(|s| s.kind == ParkKind::Dealer).map(|s| (id, s.building))).collect();
        let mut sent: std::collections::HashMap<Option<BuildingId>, u32> = std::collections::HashMap::new();
        for (vid, d) in waiting {
            let n = sent.entry(d).or_insert(0);
            if *n >= DELIVER_PER_HOUR {
                continue;
            }
            *n += 1;
            self.deliver_car(vid);
        }
        let mut fetched: std::collections::HashMap<BuildingId, u32> = std::collections::HashMap::new();
        let mut traded = 0u32;
        let jobs: Vec<(VehicleId, BuildingId, ParkSpot)> = self.vehicles.cars.iter().filter_map(|(id, c)| Some((id, c.dealer?, *c.spot()?))).collect();
        for (vid, d, spot) in jobs {
            if matches!(spot.kind, ParkKind::Hooked | ParkKind::Pound) || spot.booted {
                continue;
            }
            let h = crate::hash::mix2(slotmap::Key::data(&vid).as_ffi(), day);
            let open = self.buildings.get(d).is_some_and(|b| b.operating());
            if spot.kind == ParkKind::Dealer && spot.building == Some(d) && open {
                // Stock: to the trade once it has stood long enough.
                if tick.saturating_sub(spot.since) < STOCK_DAYS * 1440 || hour != DEALER_HOURS.0 + h % span {
                    continue;
                }
                if self.drive_out_of_town(vid, Errand::Sale(BuildingId::default())).is_none() {
                    self.drop_car(vid, CarEvent::Sold);
                }
                continue;
            }
            // Waiting to be fetched: each car from an hour of its own on.
            if hour < DEALER_HOURS.0 + h % (span - 1) {
                continue;
            }
            // Sold a day ago and still not fetched (more sold than the
            // dealer's drivers fetch, or it could not get through): the
            // trade out of town takes it, a few an hour across the city,
            // each off the map by its nearest road out.
            if tick.saturating_sub(self.vehicles.cars[vid].since) > 1440 {
                if traded < TRADE_PER_HOUR {
                    traded += 1;
                    if self.drive_out_of_town(vid, Errand::Sale(BuildingId::default())).is_none() {
                        self.drop_car(vid, CarEvent::Scrapped);
                    }
                }
                continue;
            }
            let n = fetched.entry(d).or_insert(0);
            if *n >= FETCH_PER_HOUR {
                continue;
            }
            *n += 1;
            let stop = if open { self.truck_stop(d) } else { None };
            let driven = match stop {
                Some((seg, pt)) => self.drive_from_spot(vid, seg, pt, CitizenId::default(), Errand::Sale(d)),
                None => None,
            };
            let driven = driven.or_else(|| self.drive_out_of_town(vid, Errand::Sale(BuildingId::default())));
            if driven.is_none() {
                // Nobody can drive it away: it goes for scrap where it stands.
                self.drop_car(vid, CarEvent::Scrapped);
            }
        }
    }

    /// [carlife] Dealer traffic stuck in a jam it made (a player's save:
    /// hundreds of deliveries queued to pull out at one dealer's door and
    /// over a thousand sold cars crawling for days towards it, from before
    /// trades were spread over the day): a delivery stood at the kerb for
    /// half an hour goes back onto the forecourt to be delivered in turn; a
    /// sold car stood still that long (or four times as long on the way as it
    /// should be) pulls in where it is, to be fetched
    /// by the trade later. Neither leaves the city.
    fn recall_stuck_deliveries(&mut self) {
        let tick = self.clock.tick;
        let stuck: Vec<(CarId, VehicleId, Vec2, bool)> = self
            .traffic
            .cars
            .iter()
            .filter(|(_, c)| c.hold <= 0.0 && !c.in_stunt() && !c.crashed())
            .filter(|(_, c)| {
                let overdue = c.drive.expected_min > 0.0 && tick.saturating_sub(c.drive.trip_start) as f32 > (c.drive.expected_min * 4.0).max(120.0);
                (c.errand == Errand::Deliver && c.entering && c.stalled > 1800.0) || (matches!(c.errand, Errand::Sale(_)) && (c.stalled > 1800.0 || overdue))
            })
            .filter_map(|(id, c)| Some((id, c.owned?, c.pos, c.errand == Errand::Deliver)))
            .collect();
        for (cid, vid, pos, delivery) in stuck {
            if !self.vehicles.cars.contains_key(vid) {
                continue;
            }
            if delivery {
                let Some(d) = self.dealer_near(pos) else { continue };
                let n = self.vehicles.cars.values().filter(|c| c.spot().is_some_and(|s| s.kind == ParkKind::Dealer && s.building == Some(d))).count() as u16;
                let Some(spot) = self.free_forecourt(d).or_else(|| self.forecourt_spot(d, n)) else { continue };
                self.traffic.remove_car(cid);
                self.vehicles.cars[vid].place = CarPlace::Driving(CarId::default());
                self.park_at(vid, spot);
            } else {
                let errand = self.traffic.cars[cid].errand;
                self.traffic.remove_car(cid);
                let car = &mut self.vehicles.cars[vid];
                car.owner = CitizenId::default();
                if car.dealer.is_none() {
                    car.dealer = Some(match errand {
                        Errand::Sale(d) => d,
                        _ => BuildingId::default(),
                    });
                }
                car.place = CarPlace::Driving(CarId::default());
                self.park_stranded(vid, pos);
            }
        }
    }

    /// [carlife] A used car off dealer `d`'s forecourt for a buyer wanting
    /// `class`: the same class if there is one, else any.
    fn take_stock(&self, d: BuildingId, class: VehicleClass) -> Option<VehicleId> {
        let stock = self.forecourt_cars(d);
        stock.iter().copied().find(|v| self.vehicles.cars[*v].model.class == class).or_else(|| stock.first().copied())
    }

    /// The owner is leaving town: the car goes with them, off the map.
    pub(in crate::world) fn car_emigrates(&mut self, id: CitizenId) {
        let Some(vid) = self.citizens.get(id).and_then(|c| c.car) else { return };
        let Some(car) = self.vehicles.cars.get(vid) else { return };
        match car.place {
            // [parking] On a tow truck or in a pound out of town: left
            // behind, it leaves the record with its owner.
            CarPlace::Parked(s) if s.kind == ParkKind::Hooked || (s.kind == ParkKind::Pound && s.building.is_none()) => {
                self.drop_car(vid, CarEvent::Emigrated);
            }
            // [boot] Clamped: left for the tow truck; unclaimed at the
            // pound, it is auctioned and driven away.
            CarPlace::Parked(s) if s.booted => {}
            CarPlace::Parked(_) => {
                if self.drive_out_of_town(vid, Errand::Leave).is_none() {
                    // No road out: the car leaves the record with its owner.
                    self.drop_car(vid, CarEvent::Emigrated);
                }
            }
            CarPlace::Driving(cid) => {
                let ok = self.traffic.cars.contains_key(cid) && self.redirect_out(cid);
                if ok {
                    let c = &mut self.traffic.cars[cid];
                    c.errand = Errand::Leave;
                    c.citizen = CitizenId::default();
                } else {
                    self.traffic.remove_car(cid);
                    self.drop_car(vid, CarEvent::Emigrated);
                }
            }
        }
        if let Some(c) = self.citizens.get_mut(id) {
            c.car = None;
            c.owns_car = false;
        }
    }

    /// Buy a bike at a shop near home (goods or gadgets), or online from
    /// out of town. Returns the price.
    pub(in crate::world) fn buy_bike(&mut self, id: CitizenId, reason: &str) -> f32 {
        let tick = self.clock.tick;
        let at = self.citizens[id].home.and_then(|h| self.building_entrance(h)).unwrap_or(self.citizens[id].pos);
        let shop = self.bike_shop_near(at);
        let name = match shop {
            Some(s) => {
                if let Some(bz) = self.buildings[s].business.as_mut() {
                    bz.money += BIKE_PRICE * 0.4;
                    bz.revenue_today += BIKE_PRICE * 0.4;
                    bz.deals += 1;
                }
                self.buildings[s].name.clone()
            }
            None => "a shop out of town".to_string(),
        };
        let c = &mut self.citizens[id];
        c.owns_bike = true;
        c.remember(tick, format!("{reason} Bought a bike at {name}."));
        self.vehicles.stats.bikes_bought_today += 1;
        BIKE_PRICE
    }
}
