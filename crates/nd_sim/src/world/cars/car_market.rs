//! [carlife] Cars coming and going: bought, delivered, sold, emigrating.

use super::*;

impl World {
    pub(in crate::world) fn new_car(&mut self, owner: CitizenId, model: VehicleModel, place: CarPlace, event: CarEvent) -> VehicleId {
        let color = self.rng.random_range(0..8u8);
        let vid = self.vehicles.cars.insert(OwnedCar { owner, model, place, color, since: self.clock.tick, spare: false, usual: false });
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
        match self.dealer_near(at) {
            Some(dealer) => {
                let spot = self.building_spot(ParkKind::Dealer, dealer)?;
                let vid = self.new_car(id, model, CarPlace::Parked(spot), CarEvent::Bought);
                // The forecourt is not a parking space anyone counts.
                let name = self.buildings[dealer].name.clone();
                if let Some(bz) = self.buildings[dealer].business.as_mut() {
                    // The dealer's margin; the rest goes to the maker out of town.
                    bz.money += price * 0.25;
                    bz.revenue_today += price * 0.25;
                    bz.deals += 1;
                }
                self.deliver_car(vid);
                self.vehicles.stats.bought_today += 1;
                self.citizens[id].remember(tick, format!("Bought a {} at {name}.", model_label(&model)));
                Some(price)
            }
            None => {
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
        }
    }

    /// Drive a car waiting on a dealer's forecourt to its owner's home.
    pub(in crate::world) fn deliver_car(&mut self, vid: VehicleId) {
        let Some(owner) = self.vehicles.cars.get(vid).map(|c| c.owner) else { return };
        let Some(home) = self.citizens.get(owner).and_then(|c| c.home) else { return };
        let Some(lot) = self.building_lot(home) else { return };
        let Some((seg, pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return };
        self.drive_from_spot(vid, seg, pt, CitizenId::default(), Errand::Deliver);
    }

    /// Sell the citizen's car: the dealer's driver fetches it from where it
    /// is parked (out of town without a dealer). A car on the road is not
    /// sold. Returns the money got.
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
        let driven = match dealer {
            Some(d) => self.truck_stop(d).and_then(|(seg, pt)| self.drive_from_spot(vid, seg, pt, CitizenId::default(), Errand::Sale(d))),
            None => self.drive_out_of_town(vid, Errand::Sale(BuildingId::default())),
        };
        let paid = if driven.is_some() {
            if let Some(bz) = dealer.and_then(|d| self.buildings.get_mut(d)).and_then(|b| b.business.as_mut()) {
                bz.deals += 1;
            }
            worth
        } else {
            // Nobody can drive it away: it goes for scrap where it stands.
            self.drop_car(vid, CarEvent::Scrapped);
            worth * 0.2
        };
        // The owner is done with it either way.
        let c = &mut self.citizens[id];
        c.car = None;
        c.owns_car = false;
        c.vehicle = None;
        c.parked_at = None;
        c.remember(tick, reason.to_string());
        self.vehicles.stats.sold_today += 1;
        Some(paid)
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
