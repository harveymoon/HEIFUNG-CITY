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
        let (sid, at, _) = tolls::site(&self.roads, p, size)?;
        if let Some(b) = self.tolls.booths.iter().find(|b| b.pos.distance(at) < tolls::SPACING_M) {
            return Err(format!("too close to {} ({:.0} m); keep {:.0} m between booths", b.name, b.pos.distance(at), tolls::SPACING_M));
        }
        // [toll-fit] Priced by the road: a kiosk for every lane.
        let lanes = self.roads.segment(sid).lanes();
        let cost = size.cost_for(lanes);
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; {} across {lanes} lanes costs {:.0}", self.economy.treasury, crate::names::a(size.label()), cost));
        }
        self.economy.debit(Flow::Construction, cost);
        self.tolls.next_id = self.tolls.next_id.max(self.tolls.booths.iter().map(|b| b.id).max().unwrap_or(0)) + 1;
        let id = self.tolls.next_id;
        let name = format!("{} {}", if size == TollSize::Plaza { "Toll Plaza" } else { "Toll Gate" }, id);
        let mut booth = TollBooth::new(id, size, at, name);
        booth.paid_lanes = lanes;
        self.tolls.booths.push(booth);
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
                    // [toll-fit] The road was widened under it: the works pay
                    // for the kiosks across the new lanes.
                    let lanes = self.roads.segment(sid).lanes();
                    if b.paid_lanes > 0 && lanes > b.paid_lanes {
                        let more = b.size.cost_for(lanes) - b.size.cost_for(b.paid_lanes);
                        self.economy.debit_item(Flow::Construction, more, Item::Named(format!("{} kiosks", b.name)));
                    }
                    b.paid_lanes = lanes;
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
            self.traffic.net.toll_points.push((b.id, b.pos));
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
        // [toll-inspect] Who set off round a booth.
        if !self.traffic.toll_checks.is_empty() {
            self.check_toll_avoiders();
        }
        if self.traffic.toll_passes.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        for pass in std::mem::take(&mut self.traffic.toll_passes) {
            let Some(car) = self.traffic.cars.get(pass.car) else { continue };
            let free = car.bus.is_some() || car.siren || self.services.unit_of_car(pass.car).is_some();
            let truck = car.cargo.is_some();
            let price = if free { 0.0 } else { pass.price * if truck { tolls::TRUCK_FACTOR } else { 1.0 } };
            let Some(booth) = self.tolls.get_mut(pass.booth) else { continue };
            booth.count_pass(truck, free, price);
            if free {
                self.tolls.free_today += 1;
                continue;
            }
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

    /// [toll-inspect] The vehicles set off since the last tick: a paying
    /// one whose route goes round a booth it would otherwise have driven
    /// through counts as having avoided that booth. Buses keep to their
    /// line and city vehicles go free, so neither counts.
    fn check_toll_avoiders(&mut self) {
        let checks = std::mem::take(&mut self.traffic.toll_checks);
        if !self.traffic.net.tolls {
            return;
        }
        for (cid, s0) in checks {
            let Some(car) = self.traffic.cars.get(cid) else { continue };
            if car.bus.is_some() || car.siren || self.services.unit_of_car(cid).is_some() {
                continue;
            }
            for id in self.traffic.net.avoided_booths(&car.route, s0, car.s_end) {
                if let Some(b) = self.tolls.get_mut(id) {
                    b.count_avoided();
                }
            }
        }
    }

    /// [toll-inspect] On the hour: each booth's last hour goes on its chart.
    pub(super) fn hourly_tolls(&mut self) {
        if self.tolls.booths.is_empty() {
            return;
        }
        // The hour just ended (the midnight hour belongs to the day before).
        let ended = self.clock.tick.saturating_sub(1);
        let day = (ended / crate::clock::MINUTES_PER_DAY) as u32 + 1;
        let hour = ((ended % crate::clock::MINUTES_PER_DAY) / 60) as u8;
        self.tolls.close_hour(day, hour);
    }

    /// [toll-inspect] Vehicles queueing for booth `id` now (on its lanes
    /// and the lanes leading onto them, within `tolls::QUEUE_M` of the line
    /// and crawling), how far back the queue reaches in metres, and about
    /// how many game minutes the last of them will wait.
    pub fn toll_queue(&self, id: u32) -> Option<(u32, f32, f32)> {
        let b = self.tolls.get(id)?;
        let net = &self.traffic.net;
        let mut n = 0u32;
        let mut back = 0.0f32;
        let lanes = b.segment.and_then(|s| net.segment_lanes.get(&s)).cloned().unwrap_or_default();
        let mut look = |lane: crate::traffic::LaneId, before: f32| {
            for &c in &net.lanes[lane as usize].cars {
                let Some(car) = self.traffic.cars.get(c) else { continue };
                let d = before - car.s;
                if (0.0..=tolls::QUEUE_M).contains(&d) && car.v < tolls::QUEUE_SPEED {
                    n += 1;
                    back = back.max(d);
                }
            }
        };
        for l in lanes {
            let Some(t) = net.lanes[l as usize].toll.filter(|t| t.booth == id) else { continue };
            look(l, t.s);
            if t.s < tolls::QUEUE_M {
                for &p in &net.lanes[l as usize].prev {
                    look(p, t.s + net.lanes[p as usize].length);
                }
            }
        }
        // They creep up at about the booth's speed.
        let secs = back / b.size.speed();
        let minutes = secs / self.config.traffic_seconds_per_tick.max(1e-3);
        Some((n, back, minutes))
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
    use crate::tolls::TollHour;

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

    /// [toll-fit] One toll plaza, fitted to the road: it costs a kiosk a
    /// lane, and widening the road under it pays for the new kiosks.
    #[test]
    fn a_toll_is_priced_by_its_road_and_widening_pays_for_kiosks() {
        let mut w = flat();
        let narrow = w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0)], 2, 2);
        let t0 = w.economy.treasury;
        let a = w.place_toll(Vec2::new(0.0, 0.0), TollSize::Booth).unwrap();
        assert!((t0 - w.economy.treasury - TollSize::Booth.cost_for(2)).abs() < 1e-3, "two lanes: {}", t0 - w.economy.treasury);
        let t1 = w.economy.treasury;
        w.place_toll(Vec2::new(0.0, 200.0), TollSize::Booth).unwrap();
        assert!((t1 - w.economy.treasury - TollSize::Booth.cost_for(4)).abs() < 1e-3, "four lanes cost more");
        assert!(TollSize::Booth.cost_for(4) > TollSize::Booth.cost_for(2));
        let up0 = w.tolls.get(a).unwrap().upkeep_per_day();
        // The narrow road widened to four lanes: the plaza grows with it.
        let Some(Gesture::Road { points, curved, kind, elevation, car_only, .. }) = w.gesture(narrow) else { panic!("the road") };
        w.set_gesture(narrow, Gesture::Road { points, lanes_fwd: 2, lanes_bwd: 2, curved, kind, elevation, car_only }, true);
        let t2 = w.economy.treasury;
        w.implement();
        let b = w.tolls.get(a).expect("the booth stays on the widened road");
        assert_eq!(b.paid_lanes, 4);
        let paid = t2 - w.economy.treasury;
        assert!(paid >= TollSize::Booth.cost_for(4) - TollSize::Booth.cost_for(2) - 1e-3, "the works paid for the new kiosks ({paid:.0})");
        assert!(b.upkeep_per_day() > up0, "and their upkeep");
        assert_eq!(TollSize::OFFERED.len(), 1, "one kind of toll on offer");
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

    /// [toll-inspect] A lorry on its way from `from` to `to`.
    fn truck(w: &mut World, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, fp) = w.roads.nearest_where(from, |s| s.is_street()).unwrap();
        let (ts, _, tp) = w.roads.nearest_where(to, |s| s.is_street()).unwrap();
        let (route, s0, s1) = w.traffic.net.find_route(fs, fp, ts, tp).expect("route");
        let cargo = Cargo { from: BuildingId::default(), to: BuildingId::default(), resource: Resource::Grain, amount: 5.0, price: 10.0 };
        w.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, 0)
    }

    #[test]
    fn the_booth_counts_cars_and_lorries_and_their_takings() {
        let mut w = flat();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let id = w.place_toll(Vec2::new(0.0, 0.0), TollSize::Booth).unwrap();
        let price = TollSize::Booth.default_price();
        car(&mut w, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0));
        // Westbound, from the lane on that side.
        let (fs, _, _) = w.roads.nearest_where(Vec2::new(250.0, 0.0), |s| s.is_street()).unwrap();
        let back = w.traffic.net.segment_lanes[&fs].iter().copied().find(|&l| w.traffic.net.lanes[l as usize].points[0].x > 0.0).unwrap();
        let len = w.traffic.net.lanes[back as usize].length;
        w.traffic.spawn_car(CitizenId::default(), None, vec![back], 50.0, len - 50.0, 0);
        truck(&mut w, Vec2::new(-280.0, 0.0), Vec2::new(250.0, 0.0));
        for _ in 0..600 {
            w.step_traffic();
            if w.traffic.cars.is_empty() {
                break;
            }
        }
        let b = w.tolls.get(id).unwrap();
        assert_eq!(b.cars_today, 3, "two cars and a lorry through, both ways");
        assert_eq!(b.trucks_today, 1);
        assert_eq!(b.free_today, 0);
        assert_eq!(b.avoided_today, 0, "no way round: nobody avoided it");
        let want = price * 2.0 + price * tolls::TRUCK_FACTOR;
        assert!((b.revenue_today - want).abs() < 1e-3, "revenue {} != {want}", b.revenue_today);
        assert_eq!((b.cars_total, b.trucks_total), (3, 1));
        assert!((b.revenue_total - want).abs() < 1e-3);
        assert_eq!((b.hour.cars, b.hour.trucks), (3, 1), "and the hour under way");
        assert_eq!(w.tolls.paid_today, 3);
        assert_eq!(w.tolls.trucks_today, 1);
        let (queued, _, _) = w.toll_queue(id).unwrap();
        assert_eq!(queued, 0, "the road is empty again");
    }

    #[test]
    fn drivers_who_go_round_a_dear_toll_are_counted() {
        let mut w = flat();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(-200.0, 60.0), Vec2::new(200.0, 60.0), Vec2::new(200.0, 0.0)], 1, 1);
        let id = w.place_toll(Vec2::new(0.0, 0.0), TollSize::Booth).unwrap();
        w.set_toll_price(id, 5.0).unwrap();
        car(&mut w, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0));
        // A trip nowhere near the booth is not counted.
        car(&mut w, Vec2::new(-290.0, 0.0), Vec2::new(-260.0, 0.0));
        w.step_traffic();
        assert_eq!(w.tolls.get(id).unwrap().avoided_today, 1, "the dear toll was driven round");
        w.set_toll_price(id, 0.2).unwrap();
        car(&mut w, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0));
        w.step_traffic();
        let b = w.tolls.get(id).unwrap();
        assert_eq!(b.avoided_today, 1, "the cheap one is paid, not avoided");
        assert_eq!(b.avoided_total, 1);
        assert_eq!(b.hour.avoided, 1);
        // A bus on the same trip keeps to its line: not counted.
        w.set_toll_price(id, 5.0).unwrap();
        let bus = car(&mut w, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0));
        w.traffic.cars[bus].siren = true;
        w.step_traffic();
        assert_eq!(w.tolls.get(id).unwrap().avoided_today, 1, "an emergency vehicle is not counted");
    }

    #[test]
    fn the_hours_go_on_the_chart_and_the_day_rolls_at_midnight() {
        let mut w = flat();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let id = w.place_toll(Vec2::new(0.0, 0.0), TollSize::Booth).unwrap();
        // Day 3, the hour from 23:00.
        w.clock.tick = 2 * crate::clock::MINUTES_PER_DAY + 23 * 60 + 10;
        let b = w.tolls.get_mut(id).unwrap();
        b.count_pass(false, false, 2.0);
        b.count_pass(true, false, 4.0);
        b.count_avoided();
        // Midnight: the 23:00 hour of day 3 closes, and the day.
        w.clock.tick = 3 * crate::clock::MINUTES_PER_DAY;
        w.hourly_tolls();
        w.daily_tolls();
        let b = w.tolls.get(id).unwrap();
        let last = *b.hourly.back().unwrap();
        assert_eq!((last.day, last.hour), (3, 23));
        assert_eq!((last.cars, last.trucks, last.avoided), (2, 1, 1));
        assert!((last.revenue - 6.0).abs() < 1e-4);
        assert_eq!(b.hour, TollHour::default(), "a fresh hour");
        assert_eq!((b.cars_yesterday, b.trucks_yesterday, b.avoided_yesterday), (2, 1, 1));
        assert_eq!((b.cars_today, b.trucks_today, b.avoided_today), (0, 0, 0));
        assert!((b.revenue_yesterday - 6.0).abs() < 1e-4);
        // The next hour is day 4, 00:00; the chart keeps three days.
        w.clock.tick += 60;
        w.hourly_tolls();
        let first = *w.tolls.get(id).unwrap().hourly.back().unwrap();
        assert_eq!((first.day, first.hour, first.cars), (4, 0, 0));
        for _ in 0..100 {
            w.clock.tick += 60;
            w.hourly_tolls();
        }
        assert_eq!(w.tolls.get(id).unwrap().hourly.len(), tolls::HOURS_KEPT);
        // Old saves without the new books load with them empty.
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.tolls.get(id).unwrap().hourly.len(), tolls::HOURS_KEPT);
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
