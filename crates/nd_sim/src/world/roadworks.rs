//! [events] Road wear and maintenance (`crate::road_wear`): the traffic
//! over each street piece is sampled every few minutes; at midnight every
//! piece wears by age and that traffic, and the crews resurface what they
//! can out of the day's allowance, in the order the council keeps (dear
//! land first). The lanes then carry each piece's roughness for the
//! traffic model. No random numbers.

use super::*;
use crate::road_wear::{self, lane_metres, priority, repair_cost, roughness, wears, MaintenanceDay, AGE_WEAR, BRIDGE_WEAR, HEAVY, REPAIR_AT, TRAFFIC_WEAR};
use crate::traffic::LaneKind;

/// A worn piece in the crews' queue.
#[derive(Clone, Debug)]
pub struct RepairJob {
    pub segment: SegmentId,
    pub wear: f32,
    pub land_value: f32,
    pub priority: f32,
    pub cost: f32,
}

impl World {
    /// Count the vehicles on each street piece now (lorries and buses as
    /// `HEAVY` cars): the traffic that wears it.
    pub(super) fn sample_road_load(&mut self) {
        self.road_wear.remap(&self.roads);
        let n = self.road_wear.load.len();
        for car in self.traffic.cars.values() {
            if car.in_stunt() {
                continue;
            }
            let Some(&lane) = car.route.get(car.idx) else { continue };
            let Some(l) = self.traffic.net.lanes.get(lane as usize) else { continue };
            if let LaneKind::Road { segment, .. } = l.kind {
                let i = segment.0 as usize;
                if i < n {
                    self.road_wear.load[i] += if car.cargo.is_some() || car.bus.is_some() { HEAVY } else { 1.0 };
                }
            }
        }
    }

    /// The crews' queue as it stands: every piece worn past `REPAIR_AT`,
    /// first in line first.
    pub fn repair_queue(&self) -> Vec<RepairJob> {
        let mut jobs: Vec<RepairJob> = (0..self.roads.segments.len())
            .filter(|&i| wears(&self.roads, i) && self.road_wear.wear_of(i) >= REPAIR_AT)
            .map(|i| {
                let wear = self.road_wear.wear_of(i);
                let mid = self.road_wear.mids.get(i).copied().unwrap_or_default();
                let lv = self.land_value.at(mid);
                RepairJob { segment: SegmentId(i as u32), wear, land_value: lv, priority: priority(wear, lv), cost: repair_cost(lane_metres(&self.roads, i), wear) }
            })
            .collect();
        jobs.sort_by(|a, b| b.priority.total_cmp(&a.priority).then(a.segment.0.cmp(&b.segment.0)));
        jobs
    }

    /// Midnight: every piece wears a day (age, the day's traffic, bridges
    /// more), then the crews work down the queue until the day's allowance
    /// is spent; the cost is booked as road maintenance.
    pub(super) fn daily_roads(&mut self) {
        self.road_wear.remap(&self.roads);
        let n = self.roads.segments.len();
        for i in 0..n {
            if !wears(&self.roads, i) {
                continue;
            }
            let lanes = (self.roads.segments[i].lanes_fwd + self.roads.segments[i].lanes_bwd).max(1) as f32;
            // Traffic per lane: a two-lane road carrying the same cars as
            // a one-lane one wears each lane half as much.
            let traffic = self.road_wear.load.get(i).copied().unwrap_or(0.0) / lanes;
            let mut d = AGE_WEAR + TRAFFIC_WEAR * traffic;
            if self.roads.segments[i].bridge {
                d *= BRIDGE_WEAR;
            }
            self.road_wear.wear[i] = (self.road_wear.wear[i] + d).min(1.0);
        }
        for l in self.road_wear.load.iter_mut() {
            *l = 0.0;
        }
        let allowance = self.road_wear.allowance(&self.roads);
        let mut left = allowance;
        let mut day = MaintenanceDay { day: self.clock.day(), allowance, ..Default::default() };
        let mut lv_sum = 0.0;
        let mut worked = 0u32;
        let queue = self.repair_queue();
        for job in &queue {
            if job.cost > left {
                // The queue is the queue: the crews do not skip ahead to a
                // cheaper job, they patch what the money left covers and
                // carry on there tomorrow.
                let i = job.segment.0 as usize;
                let per_wear = lane_metres(&self.roads, i) * road_wear::REPAIR_COST;
                if per_wear > 0.0 && left > 0.0 {
                    self.road_wear.wear[i] = (self.road_wear.wear[i] - left / per_wear).max(0.0);
                    left = 0.0;
                    lv_sum += job.land_value;
                    worked += 1;
                }
                break;
            }
            left -= job.cost;
            let i = job.segment.0 as usize;
            self.road_wear.wear[i] = 0.0;
            day.repaired += 1;
            day.repaired_lane_m += lane_metres(&self.roads, i);
            lv_sum += job.land_value;
            worked += 1;
        }
        day.spent = allowance - left;
        day.waiting = (queue.len() as u32).saturating_sub(day.repaired);
        if worked > 0 {
            day.repaired_land_value = lv_sum / worked as f32;
        }
        if day.spent > 0.0 {
            self.economy.debit(Flow::RoadMaintenance, day.spent);
        }
        self.road_wear.repaired_total += day.repaired;
        self.road_wear.spent_today = 0.0;
        self.road_wear.days.push(day);
        if self.road_wear.days.len() > 30 {
            self.road_wear.days.remove(0);
        }
        self.road_wear.version += 1;
        self.apply_road_wear();
    }

    /// Give every lane its piece's roughness (after a rebuild, a load and
    /// each night).
    pub(crate) fn apply_road_wear(&mut self) {
        self.road_wear.remap(&self.roads);
        let wear = &self.road_wear;
        for lane in self.traffic.net.lanes.iter_mut() {
            lane.rough = match lane.kind {
                LaneKind::Road { segment, .. } => roughness(wear.wear_of(segment.0 as usize)),
                LaneKind::Connector { .. } => 0.0,
            };
        }
    }

    /// Set the road maintenance funding level (0 to `MAX_FUNDING`, 1 is
    /// what the network needs).
    pub fn set_road_funding(&mut self, funding: f32) -> f32 {
        self.road_wear.funding = funding.clamp(0.0, road_wear::MAX_FUNDING);
        self.road_wear.funding
    }

    /// Wear of the piece `seg` (0..1).
    pub fn road_wear_of(&self, seg: SegmentId) -> f32 {
        self.road_wear.wear_of(seg.0 as usize)
    }

    /// [events] Mean wear over the street pieces whose land value is in each
    /// third (cheap, middle, dear), weighted by lane-metres: who lives with
    /// the potholes.
    pub fn road_wear_by_land_value(&self) -> [(f32, f32); 3] {
        let mut acc = [(0.0f32, 0.0f32); 3];
        for i in 0..self.roads.segments.len() {
            if !wears(&self.roads, i) {
                continue;
            }
            let mid = self.road_wear.mids.get(i).copied().unwrap_or_default();
            let lv = self.land_value.at(mid);
            let k = Tier::for_land_value(lv) as usize;
            let l = lane_metres(&self.roads, i);
            acc[k].0 += self.road_wear.wear_of(i) * l;
            acc[k].1 += l;
        }
        acc.map(|(w, l)| (if l > 0.0 { w / l } else { 0.0 }, l))
    }
}
