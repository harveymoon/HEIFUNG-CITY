//! [ped-highway] Nobody is left standing on a highway or a slip road.
//!
//! Highways and their ramps have hard shoulders, not pavements: the walk
//! graph never runs along them (`crate::walk`), and a walker's trip starts
//! from the nearest way people walk (`origin_for`). Yet a vehicle can be
//! taken off the road anywhere (a cab or bus that vanished, a car stranded
//! by road works, an aircraft gone) and its people used to be stood where
//! it was: out on the carriageway, where no foot path starts, so every trip
//! they planned failed ("No way through") and they stood there for days.
//! Anyone set down is put on the nearest pavement instead (`kerbside`), and
//! a sweep moves anyone already out there (an older save) every few ticks.

use super::*;

/// Ticks between sweeps for people standing on a carriageway.
pub(super) const STRANDED_SWEEP_EVERY: u64 = 10;
/// How far the nearest pavement is looked for, metres.
const PAVEMENT_REACH: f32 = 150.0;

impl World {
    /// Is `p` out on a highway or slip road carriageway (at its level, not
    /// under a viaduct or over a tunnel), with no street or path crossing
    /// there that a walker could be standing on (a street passing under a
    /// highway keeps its pavements)?
    pub fn stranded_on_motorway(&self, p: Vec2) -> bool {
        let Some((sid, t, q)) = self.roads.nearest_where(p, |s| s.is_street() && s.is_motorway() && !s.tunnel) else { return false };
        let s = self.roads.segment(sid);
        if q.distance(p) > s.half_width() + 0.5 || self.roads.segment_level(sid, t).abs() >= crate::roads::GRADE_SEPARATION {
            return false;
        }
        match self.roads.nearest_where(p, |s| !s.is_rail() && !s.fronts_nothing()) {
            Some((w, _, wq)) => {
                let ws = self.roads.segment(w);
                let reach = if ws.is_street() { ws.half_width() + crate::zoning::SIDEWALK + 0.5 } else { ws.half_width() + 1.0 };
                wq.distance(p) > reach
            }
            None => true,
        }
    }

    /// The nearest place on foot to `p`: a node of the walk graph at
    /// ground level that is not itself out on a carriageway; `p` when there
    /// is none in reach.
    pub fn pavement_near(&self, p: Vec2) -> Vec2 {
        self.walk.nearest_matching(p, PAVEMENT_REACH, |i| !self.stranded_on_motorway(self.walk.nodes[i as usize].pos)).map_or(p, |i| self.walk.nodes[i as usize].pos)
    }

    /// Where someone set down at `p` stands: `p`, or the nearest pavement
    /// when `p` is out on a highway or ramp.
    pub(super) fn kerbside(&self, p: Vec2) -> Vec2 {
        if self.stranded_on_motorway(p) {
            self.pavement_near(p)
        } else {
            p
        }
    }

    /// Everyone standing about outdoors on a carriageway (loitering, or
    /// waiting for a cab there) walks to the nearest pavement and decides
    /// again. Returns how many were moved.
    pub(super) fn rescue_stranded(&mut self) -> usize {
        let tick = self.clock.tick;
        let out: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(_, c)| c.inside.is_none() && matches!(c.activity, Activity::Idle | Activity::Waiting { .. }))
            .filter(|(_, c)| self.stranded_on_motorway(c.pos))
            .map(|(id, _)| id)
            .collect();
        for &id in &out {
            let to = self.pavement_near(self.citizens[id].pos);
            let taxi = matches!(self.citizens[id].transit.map(|t| t.leg), Some(TransitLeg::Taxi));
            let c = &mut self.citizens[id];
            c.pos = to;
            c.prev_pos = to;
            if let Activity::Waiting { at } = &mut c.activity {
                // A cab rider keeps waiting, on the pavement; anyone else
                // (a stop that is no more) decides again.
                if taxi {
                    *at = to;
                } else {
                    c.transit = None;
                    c.activity = Activity::Idle;
                }
            }
            if matches!(c.activity, Activity::Idle) {
                c.next_decision_tick = c.next_decision_tick.min(tick + 1);
            }
        }
        // Cabs a save left parked on a highway or ramp go back to the yard.
        let cabs: Vec<crate::taxi::CabId> = self
            .taxis
            .cabs
            .iter()
            .filter(|(_, c)| match c.place {
                crate::taxi::CabPlace::Kerb { segment, .. } => self.roads.try_segment(segment).is_none_or(|s| s.fronts_nothing()),
                _ => false,
            })
            .map(|(id, _)| id)
            .collect();
        for id in cabs {
            let c = &mut self.taxis.cabs[id];
            c.place = crate::taxi::CabPlace::Yard;
            c.job = crate::taxi::CabJob::Idle;
            c.since = tick;
            self.taxis.stats.cabs_recovered_total += 1;
        }
        out.len()
    }
}
