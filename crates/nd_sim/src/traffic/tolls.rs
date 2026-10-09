//! [tolls] Toll booths: their speed caps, the passes through them, and the
//! route cost of a tolled lane.

use super::*;

impl LaneNetwork {
    /// [toll-inspect] The booths a driver on `route` (from `s_start` along
    /// its first lane to `s_end` along its last) went round: those the way
    /// it would have taken with no tolls anywhere passes and `route` does
    /// not. Only booths near enough the trip to matter are looked for (the
    /// straight line there and on is no longer than the drive, with some
    /// slack), and only then is the toll-free way searched, so most trips
    /// cost nothing extra. No randomness.
    pub fn avoided_booths(&self, route: &[LaneId], s_start: f32, s_end: f32) -> Vec<u32> {
        let (Some(&first), Some(&last)) = (route.first(), route.last()) else { return Vec::new() };
        if !self.tolls || self.toll_points.is_empty() {
            return Vec::new();
        }
        let paid: Vec<u32> = route.iter().filter_map(|&l| self.lanes.get(l as usize).and_then(|l| l.toll)).map(|t| t.booth).collect();
        let (Some(a), Some(b)) = (self.lanes.get(first as usize), self.lanes.get(last as usize)) else { return Vec::new() };
        let (from, to) = (a.point_at(s_start).0, b.point_at(s_end).0);
        let drive = self.route_length(route, s_start) - (b.length - s_end).max(0.0);
        let reach = drive * 1.15 + 60.0;
        let near: Vec<u32> = self.toll_points.iter().filter(|(id, p)| !paid.contains(id) && from.distance(*p) + p.distance(to) <= reach).map(|(id, _)| *id).collect();
        if near.is_empty() {
            return Vec::new();
        }
        let Some((free, _, _)) = self.route_search(first, s_start, last, s_end, None, None, false) else { return Vec::new() };
        let mut out: Vec<u32> = free.iter().filter_map(|&l| self.lanes[l as usize].toll).map(|t| t.booth).filter(|id| near.contains(id)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// [tolls] Routing cost of the toll on a lane, in metres.
    pub fn toll_penalty(&self, lane: LaneId) -> f32 {
        if !self.tolls {
            return 0.0;
        }
        self.lanes.get(lane as usize).and_then(|l| l.toll).map_or(0.0, |t| t.price * crate::tolls::TOLL_AVERSION_M)
    }

    /// [tolls] Clear every lane's toll (before the world sets them again).
    pub fn clear_tolls(&mut self) {
        for l in &mut self.lanes {
            l.toll = None;
        }
        self.tolls = false;
        self.toll_points.clear();
    }
}

impl Traffic {
    /// The speed a car may do for the toll booth ahead of it on its route,
    /// if one is within `APPROACH_M`: the booth speed at the line (and just
    /// past it), rising gently with the distance before it so drivers brake
    /// in good time rather than at the last moment.
    pub(super) fn toll_cap(&self, car: &Car) -> Option<f32> {
        use crate::tolls::{APPROACH_M, EXIT_M};
        let mut acc = -car.s;
        let mut prev: Option<LaneId> = None;
        for &l in car.route.iter().skip(car.idx) {
            let lane = self.net.lanes.get(l as usize)?;
            if let Some(p) = prev {
                // A lane change: the lane beside covers the same stretch.
                if self.net.lanes[p as usize].is_beside(l) {
                    acc -= self.net.lanes[p as usize].length;
                }
            }
            if let Some(t) = lane.toll {
                let d = acc + t.s;
                if (-EXIT_M..=APPROACH_M).contains(&d) {
                    return Some(t.speed + d.max(0.0) * 0.3);
                }
            }
            acc += lane.length;
            if acc > APPROACH_M {
                return None;
            }
            prev = Some(l);
        }
        None
    }

    /// Cars past a booth line they came to from behind pay (the world is
    /// told through `toll_passes`); cars on an untolled lane forget the
    /// last booth. A car that started on a tolled lane beyond the line (out
    /// of a kerb space past the booth) owes nothing.
    pub(super) fn toll_gates(&mut self) {
        for (cid, c) in self.cars.iter_mut() {
            if c.entering || c.idx >= c.route.len() {
                continue;
            }
            let Some(lane) = self.net.lanes.get(c.route[c.idx] as usize) else { continue };
            match lane.toll {
                Some(t) => {
                    if c.toll_booth != t.booth {
                        // Joined this booth's lanes from the kerb: owes the
                        // toll only if it is still before the line.
                        c.toll_booth = t.booth;
                        c.toll_paid = c.s >= t.s;
                    } else if !c.toll_paid && c.s >= t.s {
                        c.toll_paid = true;
                        self.toll_passes.push(TollPass { car: cid, booth: t.booth, price: t.price });
                    }
                }
                None => {
                    c.toll_booth = 0;
                    c.toll_paid = false;
                }
            }
        }
    }
}
