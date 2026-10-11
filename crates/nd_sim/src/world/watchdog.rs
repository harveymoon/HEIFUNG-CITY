//! [watchdog] No service vehicle stands still on a carriageway for long.
//!
//! Tow and recovery trucks, police, fire and ambulance cars on the road,
//! parking wardens, garbage trucks and city vehicles leaving town all have
//! jobs that end in "pull in and try again" when no road leads on: done
//! on a ramp or a highway that is a car standing in a live lane for hours
//! (G10, G15, G19, G20). Every `WATCH_EVERY` ticks each of them is looked
//! at: one that has not moved `WATCH_MOVED` metres in `STILL_REROUTE`
//! minutes, standing on a lane (not parked off it), is sent on again (its
//! job's way on, else off the map); still there after `STILL_CLEAR`, it is
//! taken off the road as the airlift would (`lift_off_road`), its job
//! settled. Counted in `Driving::watchdog_*`.

use super::*;
use crate::Errand;
use serde::{Deserialize, Serialize};

pub const WATCH_EVERY: u64 = 5;
/// Metres it must have moved to count as moving.
pub const WATCH_MOVED: f32 = 6.0;
/// Minutes standing before it is sent on again, and before it is cleared.
pub const STILL_REROUTE: u64 = 30;
pub const STILL_CLEAR: u64 = 90;

/// Where a watched vehicle was last seen moving, since when, and whether
/// it has been sent on already.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Watched {
    pub pos: Vec2,
    pub since: u64,
    pub rerouted: bool,
}

impl World {
    /// [watchdog] Is `car` a service vehicle the watchdog minds?
    fn watched_vehicle(&self, id: CarId, c: &crate::traffic::Car) -> bool {
        if c.bus.is_some() || c.cargo.is_some() || c.owned.is_some() {
            return false;
        }
        matches!(c.errand, Errand::Tow(_) | Errand::Recover(_) | Errand::Patrol(_) | Errand::Out | Errand::Return(_)) || self.driving.tows.iter().any(|t| t.truck == id) || self.services.unit_of_car(id).is_some()
    }

    pub(super) fn step_watchdog(&mut self) {
        let tick = self.clock.tick;
        if tick % WATCH_EVERY != 0 {
            return;
        }
        // Queued in the traffic (behind another vehicle, at a light, for a
        // full junction, giving way, or at the kerb for a gap to pull out)
        // it is a victim of a jam, not its cause: only standing for a
        // reason of its own (pulled in to try again, held to change lanes,
        // no reason at all) counts.
        let ids: Vec<(CarId, Vec2, bool, bool)> = self
            .traffic
            .cars
            .iter()
            .filter(|(id, c)| self.watched_vehicle(*id, c))
            .map(|(id, c)| {
                use crate::traffic::Wait;
                let queued = (matches!(c.wait, Wait::Leader | Wait::Signal | Wait::ExitFull | Wait::Yield) && !c.entering && c.hold <= 0.0) || (c.wait == Wait::Entering && c.hold <= 0.0);
                // A wreck is the wrecks' business (`step_wrecks`).
                let queued = queued || c.crashed();
                (id, c.pos, c.in_stunt(), queued)
            })
            .collect();
        let live: std::collections::BTreeSet<CarId> = ids.iter().map(|x| x.0).collect();
        self.driving.watch.retain(|k, _| live.contains(k));
        for (id, pos, stunt, queued) in ids {
            let w = self.driving.watch.entry(id).or_insert(Watched { pos, since: tick, rerouted: false });
            if w.pos.distance(pos) > WATCH_MOVED || queued {
                *w = Watched { pos, since: tick, rerouted: false };
                continue;
            }
            let still = tick - w.since;
            if still >= STILL_CLEAR {
                self.driving.watch.remove(&id);
                self.driving.watchdog_cleared_total += 1;
                self.lift_off_road(id, None);
                continue;
            }
            if still >= STILL_REROUTE && !w.rerouted && !stunt {
                w.rerouted = true;
                self.driving.watchdog_rerouted_total += 1;
                self.watchdog_send_on(id);
            }
        }
    }

    /// [watchdog] Send a standing service vehicle on: on with its job when
    /// a road leads there, else off the map.
    fn watchdog_send_on(&mut self, id: CarId) {
        let Some(c) = self.traffic.cars.get(id) else { return };
        let errand = c.errand;
        let on = match errand {
            Errand::Tow(job) => match self.enforcement.job(job).map(|j| j.state) {
                Some(crate::enforcement::TowState::ToPound) => self.route_to_pound(id, job),
                _ => {
                    let company = self.enforcement.job(job).and_then(|j| j.company);
                    // Not got to the car: the job waits for another truck.
                    if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job) {
                        j.truck = None;
                        j.state = crate::enforcement::TowState::Waiting { until: self.clock.tick + 10 };
                    }
                    self.tow_truck_home(id, company);
                    true
                }
            },
            Errand::Recover(wreck) => {
                for w in self.driving.wrecks.iter_mut().filter(|w| w.car == wreck && w.truck == Some(id)) {
                    w.truck = None;
                }
                self.recovery_truck_home(id, None);
                true
            }
            _ => {
                // Its own route again from where it stands.
                let goal = c.route.last().copied().map(|g| (g, c.s_end));
                goal.is_some_and(|(g, se)| self.traffic.redirect(id, g, se))
            }
        };
        if !on && self.traffic.cars.contains_key(id) {
            self.retire_vehicle(id);
        }
        // Sent on: it goes now, not when the wait it pulled in for is up.
        if let Some(c) = self.traffic.cars.get_mut(id) {
            c.hold = 0.0;
        }
    }
}
