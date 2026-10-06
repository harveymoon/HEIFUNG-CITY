//! [tolls] Toll booths on the roads: placing and removing them, finding
//! their road again after every rebuild, putting them on the lanes, and
//! charging the vehicles that drive through. The data types live in
//! `crate::tolls`; the lane-level part (slowing down, noticing who passed)
//! in `traffic.rs`.

use super::*;
use crate::tolls::{self, TollBooth, TollSize};
use crate::traffic::LaneToll;

impl World {
    /// Put a toll booth (or plaza) on the road at `p`, paid from the
    /// treasury. It sits across the whole carriageway, both directions.
    pub fn place_toll(&mut self, p: Vec2, size: TollSize) -> Result<u32, String> {
        let (_, at, _) = tolls::site(&self.roads, p, size)?;
        if let Some(b) = self.tolls.booths.iter().find(|b| b.pos.distance(at) < tolls::SPACING_M) {
            return Err(format!("too close to {} ({:.0} m); keep {:.0} m between booths", b.name, b.pos.distance(at), tolls::SPACING_M));
        }
        if self.economy.treasury < size.cost() {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, size.label(), size.cost()));
        }
        self.economy.debit(Flow::Construction, size.cost());
        self.tolls.next_id = self.tolls.next_id.max(self.tolls.booths.iter().map(|b| b.id).max().unwrap_or(0)) + 1;
        let id = self.tolls.next_id;
        let name = format!("{} {}", if size == TollSize::Plaza { "Toll Plaza" } else { "Toll Gate" }, id);
        self.tolls.booths.push(TollBooth {
            id,
            size,
            pos: at,
            price: size.default_price(),
            name,
            cars_today: 0,
            revenue_today: 0.0,
            cars_yesterday: 0,
            revenue_yesterday: 0.0,
            cars_total: 0,
            revenue_total: 0.0,
            segment: None,
            dir: Vec2::X,
        });
        self.apply_tolls();
        Ok(id)
    }

    /// Take a booth away (no refund: the money went into the concrete).
    pub fn remove_toll(&mut self, id: u32) -> Result<(), String> {
        let before = self.tolls.booths.len();
        self.tolls.booths.retain(|b| b.id != id);
        if self.tolls.booths.len() == before {
            return Err("no such toll booth".into());
        }
        self.apply_tolls();
        Ok(())
    }

    /// Set the toll at a booth (clamped to `0..MAX_PRICE`); returns it.
    pub fn set_toll_price(&mut self, id: u32, price: f32) -> Result<f32, String> {
        let b = self.tolls.get_mut(id).ok_or("no such toll booth")?;
        b.price = if price.is_finite() { price.clamp(0.0, tolls::MAX_PRICE) } else { b.size.default_price() };
        let set = b.price;
        self.apply_tolls();
        Ok(set)
    }

    /// Find every booth's road piece again and put the booths on its lanes.
    /// Called after every lane network rebuild and whenever a booth changes.
    /// A booth whose road is gone (or no longer has room for it) is removed.
    pub(crate) fn apply_tolls(&mut self) {
        self.traffic.net.clear_tolls();
        let mut keep = Vec::with_capacity(self.tolls.booths.len());
        for mut b in std::mem::take(&mut self.tolls.booths) {
            match tolls::site(&self.roads, b.pos, b.size) {
                Ok((sid, at, dir)) if at.distance(b.pos) < 12.0 => {
                    b.segment = Some(sid);
                    b.pos = at;
                    b.dir = dir;
                    keep.push(b);
                }
                // The road went, or no longer has room for it.
                _ => {}
            }
        }
        for b in &keep {
            let Some(sid) = b.segment else { continue };
            let lanes = self.traffic.net.segment_lanes.get(&sid).cloned().unwrap_or_default();
            for l in lanes {
                let lane = &mut self.traffic.net.lanes[l as usize];
                let (s, _) = lane.project(b.pos);
                lane.toll = Some(LaneToll { booth: b.id, s, price: b.price, speed: b.size.speed() });
                self.traffic.net.tolls = true;
            }
        }
        self.tolls.booths = keep;
        self.tolls.version += 1;
    }

    /// Charge the vehicles that drove through a booth this tick. Buses and
    /// emergency vehicles go free; lorries pay `TRUCK_FACTOR` times the toll
    /// out of their business's money (the seller's, or the buyer's for an
    /// import); a citizen's car out of the driver's pocket (and the trip
    /// remembers it, which the next mode choice weighs); anyone from out of
    /// town (through traffic, out-of-town lorries) pays and is gone.
    pub(super) fn settle_tolls(&mut self) {
        if self.traffic.toll_passes.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        for pass in std::mem::take(&mut self.traffic.toll_passes) {
            let Some(car) = self.traffic.cars.get(pass.car) else { continue };
            let free = car.bus.is_some() || car.siren || self.services.unit_of_car(pass.car).is_some();
            let Some(booth) = self.tolls.get_mut(pass.booth) else { continue };
            booth.cars_today += 1;
            booth.cars_total += 1;
            if free {
                self.tolls.free_today += 1;
                continue;
            }
            let truck = car.cargo.is_some();
            let price = pass.price * if truck { tolls::TRUCK_FACTOR } else { 1.0 };
            booth.revenue_today += price;
            booth.revenue_total += price;
            let name = booth.name.clone();
            self.tolls.paid_today += 1;
            if truck {
                self.tolls.trucks_today += 1;
            }
            let (driver, cargo) = (car.citizen, car.cargo);
            if price > 0.0 {
                self.economy.credit(Flow::Tolls, price);
            }
            if let Some(c) = self.citizens.get_mut(driver) {
                c.savings -= price;
                c.commute.toll += price;
                if price > 0.0 && (tick + pass.booth as u64) % 9 == 0 {
                    c.remember(tick, format!("Paid {price:.0} at {name}. Every trip costs more now."));
                }
            } else if let Some(cargo) = cargo {
                let payer = [cargo.from, cargo.to].into_iter().find(|b| self.buildings.get(*b).is_some_and(|b| b.business.is_some()));
                if let Some(bz) = payer.and_then(|b| self.buildings.get_mut(b)).and_then(|b| b.business.as_mut()) {
                    bz.money -= price;
                    bz.spend_today += price;
                }
            }
        }
    }

    /// Midnight: booth staff and upkeep are paid, the day's counts close.
    pub(super) fn daily_tolls(&mut self) {
        let upkeep = self.tolls.upkeep_per_day();
        if upkeep > 0.0 {
            self.economy.debit(Flow::TollUpkeep, upkeep);
        }
        self.tolls.roll();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::LandscapeParams;

    fn flat() -> World {
        let mut w = World::new(WorldConfig { landscape: LandscapeParams::flat(), emergencies: false, society: false, ..WorldConfig::test_town() });
        w.economy.treasury = 100_000.0;
        w
    }

    /// A car on its way from `from` to `to` along the streets.
    fn car(w: &mut World, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, fp) = w.roads.nearest_where(from, |s| s.is_street()).unwrap();
        let (ts, _, tp) = w.roads.nearest_where(to, |s| s.is_street()).unwrap();
        let (route, s0, s1) = w.traffic.net.find_route(fs, fp, ts, tp).expect("route");
        w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
    }

    #[test]
    fn a_car_slows_through_the_booth_and_pays_the_toll() {
        let mut w = flat();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let id = w.place_toll(Vec2::new(0.0, 3.0), TollSize::Booth).expect("placed on the road");
        assert!(w.traffic.net.lanes.iter().filter(|l| l.toll.is_some()).count() >= 2, "both directions are tolled");
        let t0 = w.economy.treasury;
        let c = car(&mut w, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0));
        let mut slowest_at_booth = f32::MAX;
        let mut fastest = 0.0f32;
        for _ in 0..400 {
            w.step_traffic();
            let Some(car) = w.traffic.cars.get(c) else { break };
            fastest = fastest.max(car.v);
            // Sampled once a tick (several metres a tick even at booth speed).
            if car.pos.x.abs() < 10.0 {
                slowest_at_booth = slowest_at_booth.min(car.v);
            }
        }
        let booth = w.tolls.get(id).unwrap();
        assert_eq!(booth.cars_today, 1, "one car through");
        assert!((booth.revenue_today - TollSize::Booth.default_price()).abs() < 1e-4);
        assert!((w.economy.treasury - t0 - booth.revenue_today).abs() < 1e-3, "into the treasury");
        assert!((w.economy.ledger.today_of(Flow::Tolls) - booth.revenue_today).abs() < 1e-3, "under its own flow");
        assert!(slowest_at_booth <= TollSize::Booth.speed() + 3.5, "slowed through the booth ({slowest_at_booth} m/s)");
        assert!(fastest > 8.0, "and drove at road speed elsewhere ({fastest})");
    }

    #[test]
    fn drivers_go_round_a_dear_toll_and_through_a_cheap_one() {
        let mut w = flat();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(-200.0, 60.0), Vec2::new(200.0, 60.0), Vec2::new(200.0, 0.0)], 1, 1);
        let id = w.place_toll(Vec2::new(0.0, 0.0), TollSize::Booth).unwrap();
        let tolled = |w: &World| {
            let (fs, _, fp) = w.roads.nearest_where(Vec2::new(-250.0, 0.0), |s| s.is_street()).unwrap();
            let (ts, _, tp) = w.roads.nearest_where(Vec2::new(250.0, 0.0), |s| s.is_street()).unwrap();
            let (route, _, _) = w.traffic.net.find_route(fs, fp, ts, tp).unwrap();
            route.iter().any(|&l| w.traffic.net.lanes[l as usize].toll.is_some())
        };
        // The detour is 120 m: a toll of 5 (600 m) is worth avoiding...
        w.set_toll_price(id, 5.0).unwrap();
        assert!(!tolled(&w), "a dear toll is driven round");
        // ...one of 0.2 (24 m) is not.
        w.set_toll_price(id, 0.2).unwrap();
        assert!(tolled(&w), "a cheap one is paid");
    }

    #[test]
    fn booths_survive_road_works_and_saves_and_go_with_their_road() {
        let mut w = flat();
        let road = w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        assert!(w.place_toll(Vec2::new(-120.0, 0.0), TollSize::Plaza).is_ok());
        assert!(w.place_toll(Vec2::new(-100.0, 0.0), TollSize::Booth).is_err(), "too close to the plaza");
        assert!(w.place_toll(Vec2::new(0.0, 80.0), TollSize::Booth).is_err(), "not on a road");
        let id = w.place_toll(Vec2::new(100.0, 0.0), TollSize::Booth).unwrap();
        // Another road elsewhere: the booths stay and are back on the lanes.
        w.add_road(&[Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0)], 1, 1);
        assert_eq!(w.tolls.booths.len(), 2);
        assert!(w.traffic.net.lanes.iter().any(|l| l.toll.is_some_and(|t| t.booth == id)));
        // A save keeps them, and the load puts them back on the lanes.
        w.set_toll_price(id, 4.5).unwrap();
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.tolls.booths.len(), 2);
        assert!(back.traffic.net.lanes.iter().any(|l| l.toll.is_some_and(|t| t.booth == id && t.price == 4.5)));
        // The road bulldozed: the booths go with it.
        w.remove_gesture(road);
        w.implement();
        assert!(w.tolls.booths.is_empty(), "no road, no booth");
        assert!(!w.traffic.net.tolls);
    }

    #[test]
    fn upkeep_is_paid_at_midnight() {
        let mut w = flat();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.place_toll(Vec2::new(0.0, 0.0), TollSize::Plaza).unwrap();
        w.daily_tolls();
        assert_eq!(w.economy.ledger.today_of(Flow::TollUpkeep), -TollSize::Plaza.upkeep_per_day());
    }
}
