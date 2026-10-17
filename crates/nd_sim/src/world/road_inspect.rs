//! [road-inspect] The road inspector's side of the sim: the vehicles that
//! drive onto each street piece counted hour by hour
//! (`crate::road_traffic`), each piece's wear a day and last repair read off
//! at midnight, what is on a piece right now (`road_now`), the run of
//! pieces between two junctions it belongs to (`road_run`), and the
//! player's "Repair now", paid out of the treasury as road maintenance.
//!
//! Read-only towards the traffic and the wear model (`world/roadworks.rs`):
//! nothing here changes how roads wear or how cars drive, except Repair
//! now, which resurfaces a piece the way the crews would.

use super::*;
use crate::road_traffic::RoadNow;
use crate::road_wear::{self, lane_metres, wears};
use crate::traffic::LaneKind;

/// Vehicles slower than this stand in a queue, m/s.
const QUEUE_SPEED: f32 = 2.0;
/// Walkers and riders this far beyond a street's half width are on its
/// pavement.
const PAVEMENT: f32 = 4.0;
/// The longest run of pieces `road_run` follows.
const RUN_MAX: usize = 400;

impl World {
    /// After the traffic step: the vehicles that drove onto each piece this
    /// tick, into its hour.
    pub(super) fn log_road_traffic(&mut self) {
        if self.traffic.segment_entries.is_empty() {
            return;
        }
        let mut entries = std::mem::take(&mut self.traffic.segment_entries);
        self.road_log.remap(&self.roads);
        let hour = (self.clock.tick / 60) as u32;
        for &s in &entries {
            if let Some(l) = self.road_log.segs.get_mut(s as usize) {
                l.traffic.add(hour, 1);
            }
        }
        entries.clear();
        self.traffic.segment_entries = entries;
    }

    /// Midnight, after the crews: each piece's wear a day and repairs.
    pub(super) fn note_road_day(&mut self) {
        self.road_wear.remap(&self.roads);
        self.road_log.remap(&self.roads);
        let day = self.clock.day() as u32;
        let (roads, wear) = (&self.roads, &self.road_wear);
        self.road_log.note_midnight(day, |i| wears(roads, i).then(|| wear.wear_of(i)));
    }

    /// The piece's wear a day: as measured over its last whole day, else
    /// reckoned from its age and yesterday's traffic the way the nightly
    /// wear does. With whether it was measured.
    pub fn road_wear_rate(&self, sid: SegmentId) -> (f32, bool) {
        let i = sid.0 as usize;
        if let Some(r) = self.road_log.get(i).filter(|_| self.road_log.roads_version == self.roads.version).and_then(|l| l.wear_rate) {
            return (r, true);
        }
        if !wears(&self.roads, i) {
            return (0.0, false);
        }
        let load = self.road_wear.last_load.get(i).copied().or_else(|| {
            // No whole day yet: today's so far, stretched over the day.
            let frac = self.clock.day_fraction().max(0.05);
            self.road_wear.load.get(i).map(|l| l / frac)
        });
        let per_100m = (lane_metres(&self.roads, i) / 100.0).max(0.05);
        let mut d = road_wear::AGE_WEAR + road_wear::TRAFFIC_WEAR * load.unwrap_or(0.0) / per_100m;
        if self.roads.segments[i].bridge {
            d *= road_wear::BRIDGE_WEAR;
        }
        (d, false)
    }

    /// The piece's hourly counts (empty if no vehicle has driven onto it
    /// since the log was laid out for this network).
    pub fn road_hours(&self, sid: SegmentId) -> Option<&crate::road_traffic::SegLog> {
        (self.road_log.roads_version == self.roads.version).then(|| self.road_log.get(sid.0 as usize)).flatten()
    }

    /// What is on the piece right now: vehicles by kind, walkers and riders
    /// on it and its pavements, speeds and the queue.
    /// [K11] Whether vehicles can drive on `sid` at all: it has a car lane
    /// (a footpath or a pedestrians-only street has none, and no vehicle
    /// flow to count).
    pub fn carries_vehicles(&self, sid: SegmentId) -> bool {
        self.traffic.net.segment_lanes.get(&sid).is_some_and(|ls| ls.iter().any(|&l| matches!(self.traffic.net.lanes[l as usize].kind, LaneKind::Road { .. })))
    }

    pub fn road_now(&self, sid: SegmentId) -> RoadNow {
        let mut out = RoadNow::default();
        let Some(seg) = self.roads.try_segment(sid).filter(|s| s.alive) else { return out };
        let (a, b) = self.roads.segment_points(sid);
        out.lane_m = lane_metres(&self.roads, sid.0 as usize);
        let rough = road_wear::slow_factor(road_wear::roughness(self.road_wear.wear_of(sid.0 as usize)));
        let (mut v_sum, mut limit_sum, mut lanes) = (0.0f32, 0.0f32, 0u32);
        for &lid in self.traffic.net.segment_lanes.get(&sid).map(|v| v.as_slice()).unwrap_or(&[]) {
            let lane = &self.traffic.net.lanes[lid as usize];
            if !matches!(lane.kind, LaneKind::Road { .. }) {
                continue;
            }
            limit_sum += (lane.speed_limit + lane.limit_end) * 0.5;
            lanes += 1;
            // Cars are in increasing position: the front of the lane last.
            let mut queue_open = true;
            for &cid in lane.cars.iter().rev() {
                let Some(car) = self.traffic.cars.get(cid) else { continue };
                if car.cargo.is_some() {
                    out.lorries += 1;
                } else if car.bus.is_some() {
                    out.buses += 1;
                } else if car.siren || self.services.unit_of_car(cid).is_some() {
                    out.sirens += 1;
                } else {
                    out.cars += 1;
                }
                v_sum += car.v;
                if queue_open && car.v < QUEUE_SPEED {
                    out.queued += 1;
                    out.queue_m = out.queue_m.max(lane.length - car.s + car.length());
                } else {
                    queue_open = false;
                }
            }
        }
        if lanes > 0 {
            out.limit = limit_sum / lanes as f32 * rough;
        }
        if out.vehicles() > 0 {
            out.mean_speed = v_sum / out.vehicles() as f32;
        }
        // Walkers and riders along it (not across a junction at either end).
        if !seg.is_motorway() && !seg.tunnel {
            let reach = seg.half_width() + PAVEMENT;
            let (lo, hi) = (a.min(b) - Vec2::splat(reach), a.max(b) + Vec2::splat(reach));
            for c in self.citizens.values() {
                if !matches!(c.activity, Activity::Traveling { .. }) {
                    continue;
                }
                let p = c.pos;
                if p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y {
                    continue;
                }
                let (t, q) = crate::geom::project_on_segment(p, a, b);
                if !(0.0..=1.0).contains(&t) || q.distance(p) > reach {
                    continue;
                }
                match c.mode {
                    crate::citizens::TravelMode::Bike => out.bikes += 1,
                    crate::citizens::TravelMode::Walk => out.walkers += 1,
                }
            }
        }
        out
    }

    /// The run of pieces `sid` belongs to: on along the same kind of way
    /// through every node that only joins two pieces, to the junctions (or
    /// dead ends) at both ends. In order from one end to the other.
    pub fn road_run(&self, sid: SegmentId) -> Vec<SegmentId> {
        let Some(s0) = self.roads.try_segment(sid).filter(|s| s.alive) else { return Vec::new() };
        let kind = s0.kind;
        let walk = |from: NodeId| -> Vec<SegmentId> {
            let (mut out, mut node, mut prev) = (Vec::new(), from, sid);
            while out.len() < RUN_MAX {
                let ways: Vec<(NodeId, SegmentId)> = self.roads.neighbors(node).collect();
                if ways.len() != 2 {
                    break;
                }
                let Some(&(next_node, next)) = ways.iter().find(|(_, s)| *s != prev) else { break };
                if next == sid || self.roads.segment(next).kind != kind {
                    break;
                }
                out.push(next);
                prev = next;
                node = next_node;
            }
            out
        };
        let mut back = walk(s0.a);
        back.reverse();
        back.push(sid);
        back.extend(walk(s0.b));
        back
    }

    /// [road-inspect] "Repair now": resurface the piece at once, paying
    /// what the crews would (`road_wear::repair_cost`) out of the treasury,
    /// booked as road maintenance. What it cost.
    pub fn repair_road_now(&mut self, sid: SegmentId) -> Result<f32, String> {
        self.road_wear.remap(&self.roads);
        let i = sid.0 as usize;
        if self.roads.try_segment(sid).is_none_or(|s| !s.alive) {
            return Err("no such road".into());
        }
        if !wears(&self.roads, i) {
            return Err("the city's crews do not look after this way (paths, track, bored tunnels, highways and roads past the map edge)".into());
        }
        let wear = self.road_wear.wear_of(i);
        if wear < 0.005 {
            return Err("it is like new already".into());
        }
        let cost = road_wear::repair_cost(lane_metres(&self.roads, i), wear);
        if self.economy.treasury < cost {
            return Err(format!("not enough money: resurfacing it costs {cost:.0}"));
        }
        self.economy.debit_item(Flow::RoadMaintenance, cost, Item::Named("Repairs ordered".into()));
        self.road_wear.wear[i] = 0.0;
        self.road_wear.repaired_total += 1;
        self.road_wear.version += 1;
        for lane in self.traffic.net.lanes.iter_mut() {
            if matches!(lane.kind, LaneKind::Road { segment, .. } if segment == sid) {
                lane.rough = 0.0;
            }
        }
        self.road_log.remap(&self.roads);
        if let Some(l) = self.road_log.segs.get_mut(i) {
            l.repaired_day = Some(self.clock.day() as u32);
            l.repaired_whole = true;
            l.repaired_by_order = true;
            // No fair day's wear to measure until the next whole day.
            l.wear_midnight = None;
        }
        Ok(cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zoning::LandUse;

    fn square(x: f32, y: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)]
    }

    fn town() -> World {
        let mut w = World::new(WorldConfig { seed: 4, emergencies: false, ..WorldConfig::test_town() });
        w.economy.treasury = 1_000_000.0;
        w.add_road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 200.0)], 1, 1);
        w.add_zone(square(-490.0, 8.0, 480.0, 60.0), LandUse::Residential);
        w.add_zone(square(10.0, 8.0, 480.0, 60.0), LandUse::Commercial);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).take(16).collect();
        for l in lots {
            w.build_on(l);
            if let Some(b) = w.lots[l].building {
                w.finish_construction(b);
            }
        }
        w
    }

    fn street_piece(w: &World) -> SegmentId {
        (0..w.roads.segments.len()).find(|&i| wears(&w.roads, i)).map(|i| SegmentId(i as u32)).expect("a street")
    }

    #[test]
    fn repair_now_pays_and_resurfaces() {
        let mut w = town();
        let sid = street_piece(&w);
        w.road_wear.remap(&w.roads);
        w.road_wear.wear[sid.0 as usize] = 0.7;
        w.apply_road_wear();
        // Small enough for f32 pennies to add up.
        w.economy.treasury = 50_000.0;
        let expect = road_wear::repair_cost(lane_metres(&w.roads, sid.0 as usize), 0.7);
        let (before, ledger) = (w.economy.treasury, w.economy.ledger.today_of(Flow::RoadMaintenance));
        let paid = w.repair_road_now(sid).expect("repaired");
        assert!((paid - expect).abs() < 1e-3, "paid the crews' price: {paid} vs {expect}");
        assert!((before - w.economy.treasury - paid).abs() < 1e-2, "out of the treasury");
        assert!((w.economy.ledger.today_of(Flow::RoadMaintenance) - (ledger - paid)).abs() < 1e-2, "booked as road maintenance");
        assert_eq!(w.road_wear_of(sid), 0.0);
        let lanes = &w.traffic.net.segment_lanes[&sid];
        assert!(lanes.iter().all(|&l| w.traffic.net.lanes[l as usize].rough == 0.0), "smooth for the traffic at once");
        let log = w.road_hours(sid).expect("logged");
        assert_eq!((log.repaired_day, log.repaired_by_order), (Some(w.clock.day() as u32), true));
        // Nothing left to repair, and no money: refused.
        assert!(w.repair_road_now(sid).is_err());
        w.road_wear.wear[sid.0 as usize] = 0.5;
        w.economy.treasury = 1.0;
        assert!(w.repair_road_now(sid).unwrap_err().contains("not enough money"));
        assert_eq!(w.road_wear_of(sid), 0.5, "unpaid, untouched");
    }

    #[test]
    fn vehicles_driving_onto_a_piece_are_counted_by_the_hour() {
        let mut w = town();
        let mut minds = crate::mind::IdleMind;
        for _ in 0..(10 * 60) {
            w.tick(&mut minds);
        }
        let total: u32 = (0..w.roads.segments.len()).filter_map(|i| w.road_hours(SegmentId(i as u32))).map(|l| l.traffic.counts.iter().map(|&c| c as u32).sum::<u32>()).sum();
        assert!(total > 0, "somebody drove onto a street in ten hours");
        // Every count sits in an hour that has been.
        let now = (w.clock.tick / 60) as u32;
        for i in 0..w.roads.segments.len() {
            if let Some(l) = w.road_hours(SegmentId(i as u32)).filter(|l| !l.traffic.counts.is_empty()) {
                assert!(l.traffic.hour <= now);
            }
        }
    }

    #[test]
    fn a_run_reaches_the_junctions() {
        let w = town();
        let sid = street_piece(&w);
        let run = w.road_run(sid);
        assert!(run.contains(&sid));
        // Every piece in the run is the same kind, and no inner node is a junction.
        for pair in run.windows(2) {
            let (s, t) = (w.roads.segment(pair[0]), w.roads.segment(pair[1]));
            let shared = [s.a, s.b].into_iter().find(|n| *n == t.a || *n == t.b).expect("consecutive pieces meet");
            assert_eq!(w.roads.neighbors(shared).count(), 2);
        }
    }

    /// [K11] A footpath carries no vehicles (its card shows people, not a
    /// vehicle flow of zeros); a street does.
    #[test]
    fn a_footpath_carries_no_vehicles() {
        let mut w = town();
        w.add_path(&[Vec2::new(-100.0, -60.0), Vec2::new(-100.0, -10.0)]);
        w.implement();
        let street = street_piece(&w);
        assert!(w.carries_vehicles(street));
        let path = (0..w.roads.segments.len()).map(|i| SegmentId(i as u32)).find(|&s| w.roads.segment(s).alive && w.roads.segment(s).kind == crate::roads::RoadKind::Path).expect("the path");
        assert!(!w.carries_vehicles(path));
    }
}
