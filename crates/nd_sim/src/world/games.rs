//! [games] Stadium game days.
//!
//! A stadium plays on the days its draw comes up (`Services::events_today`,
//! rolled each midnight by tier). On a game day:
//!
//! - the gates open at `GATES_OPEN`; local fans with a ticket set off at
//!   `EVENT_MINUTE` (`start_stadium_events`) by whatever way they travel;
//! - away fans and visitors drive in from out of town (`AWAY_CARS` by
//!   tier), each at a minute of its own before kick-off, and park at the
//!   ground (counted in `Games::parked`, drawn in rows on the plaza);
//! - kick-off at `KICKOFF`, the final whistle at `FINAL_WHISTLE`; then the
//!   away fans' cars drive off the map over the next half hour, and the
//!   crowd is gone by `CROWD_GONE`.
//!
//! `World::game_phase` says where a stadium is in that (the renderer lights
//! the pitch and fills the stands from it). Timings are hashes of the
//! stadium and the day; no world RNG.

use super::*;
use serde::{Deserialize, Serialize};

/// Gates open, kick-off, the final whistle, and the last of the crowd away.
pub const GATES_OPEN: u64 = 16 * 60 + 30;
pub const KICKOFF: u64 = 19 * 60;
pub const FINAL_WHISTLE: u64 = 21 * 60 + 15;
pub const CROWD_GONE: u64 = 22 * 60;
/// Away fans' cars driving in, by tier (sports ground, stadium, arena).
pub const AWAY_CARS: [u32; 3] = [10, 28, 64];
/// The last of them aims to be parked this long before kick-off.
const ARRIVE_BEFORE: u64 = 20;
/// No more come in while the roads carry this many vehicles.
const MAX_ON_ROAD: usize = 6000;
/// They drive off over this many minutes after the whistle.
const LEAVING_SPREAD: u64 = 35;
/// [w1-roads] Kerb spots by the ground they pull out from, and how far
/// apart (m).
const LEAVE_SPOTS: usize = 4;
const LEAVE_SPACING: f32 = 9.0;

/// Where a stadium is in its game day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamePhase {
    /// Gates open, the crowd coming in.
    Gates,
    /// The game on.
    Live,
    /// Final whistle: the crowd going home.
    Leaving,
}

impl GamePhase {
    pub fn label(self) -> &'static str {
        match self {
            GamePhase::Gates => "gates open, the crowd coming in",
            GamePhase::Live => "game on",
            GamePhase::Leaving => "final whistle, the crowd heading home",
        }
    }
}

/// An away fan's car on its way in.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FanCar {
    pub car: CarId,
    pub stadium: BuildingId,
}

/// Game-day traffic: away fans driving in, and their cars parked at each
/// ground. Old saves load with none.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Games {
    pub driving_in: Vec<FanCar>,
    pub parked: std::collections::BTreeMap<BuildingId, u32>,
    /// Away fans' cars in all, for the report.
    pub away_cars_total: u32,
}

fn key(b: BuildingId) -> u64 {
    slotmap::Key::data(&b).as_ffi()
}

impl World {
    /// [games] Where stadium `b` is in a game today (None: no game, or
    /// outside its hours).
    pub fn game_phase(&self, b: BuildingId) -> Option<GamePhase> {
        if !self.services.events_today.contains(&b) {
            return None;
        }
        match self.clock.minute_of_day() {
            m if (GATES_OPEN..KICKOFF).contains(&m) => Some(GamePhase::Gates),
            m if (KICKOFF..FINAL_WHISTLE).contains(&m) => Some(GamePhase::Live),
            m if (FINAL_WHISTLE..CROWD_GONE).contains(&m) => Some(GamePhase::Leaving),
            _ => None,
        }
    }

    /// [games] Away fans' cars parked at stadium `b`.
    pub fn game_cars_parked(&self, b: BuildingId) -> u32 {
        self.games.parked.get(&b).copied().unwrap_or(0)
    }

    /// [games] Today's away fans in and out: every minute from the gates
    /// opening to the crowd gone.
    pub(super) fn step_games(&mut self) {
        let minute = self.clock.minute_of_day();
        if minute == 0 {
            // A new day: anyone still parked from last night has gone.
            self.games.parked.clear();
        }
        // [w1-roads] Fans still parked when the crowd should be gone (the
        // street out was full) keep leaving until they are all away.
        let stragglers = minute > CROWD_GONE && self.games.parked.values().any(|n| *n > 0);
        if stragglers {
            let grounds: Vec<BuildingId> = self.games.parked.iter().filter(|(_, n)| **n > 0).map(|(b, _)| *b).collect();
            for b in grounds {
                for k in 0..LEAVE_SPOTS {
                    self.drive_out_fan(b, self.clock.day() ^ (minute << 8) ^ k as u64);
                }
            }
            return;
        }
        if !(GATES_OPEN..=CROWD_GONE).contains(&minute) || (self.services.events_today.is_empty() && self.games.driving_in.is_empty()) {
            return;
        }
        let day = self.clock.day();
        let games: Vec<BuildingId> = self.services.events_today.iter().copied().filter(|b| self.buildings.get(*b).is_some_and(|b| !b.is_derelict() && b.operating() && b.service.is_some())).collect();
        for b in games {
            let tier = self.buildings[b].service.map_or(Tier::Mid, |s| s.tier);
            let n = AWAY_CARS[tier.index()];
            let window = KICKOFF - ARRIVE_BEFORE - GATES_OPEN;
            if (GATES_OPEN..KICKOFF - ARRIVE_BEFORE).contains(&minute) {
                let due = (0..n).filter(|&k| GATES_OPEN + crate::hash::mix(key(b) ^ day.wrapping_mul(0x51AD), 0x6A3E_0000 + k as u64) % window == minute).count();
                for k in 0..due {
                    self.drive_in_fan(b, day ^ (minute << 8) ^ k as u64);
                }
            }
            if (FINAL_WHISTLE..FINAL_WHISTLE + LEAVING_SPREAD).contains(&minute) {
                // Spread evenly over what is left of the spread.
                let parked = self.game_cars_parked(b);
                let remaining = (FINAL_WHISTLE + LEAVING_SPREAD - minute) as u32;
                let go = parked.div_ceil(remaining);
                for k in 0..go {
                    self.drive_out_fan(b, day ^ (minute << 8) ^ k as u64);
                }
            }
        }
        if minute == CROWD_GONE {
            // Any still on their way in turn round and go home.
            for f in std::mem::take(&mut self.games.driving_in) {
                if self.traffic.cars.contains_key(f.car) {
                    self.retire_vehicle(f.car);
                }
            }
        }
    }

    /// An away fan drives in from a road end off the map (one picked by
    /// `salt`) to the stadium's street.
    fn drive_in_fan(&mut self, b: BuildingId, salt: u64) {
        if self.traffic.cars.len() >= MAX_ON_ROAD {
            return;
        }
        let Some((seg, pt)) = self.truck_stop(b) else { return };
        let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { return };
        let mut ends: Vec<(NodeId, SegmentId)> = self.highway_ends().0;
        ends.extend(self.city_leaves().into_iter().filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s))));
        if ends.is_empty() {
            return;
        }
        let start = (crate::hash::mix(key(b), salt) % ends.len() as u64) as usize;
        let route = (0..ends.len().min(4)).find_map(|i| {
            let (n, sid) = ends[(start + i) % ends.len()];
            self.route_in_from(n, sid, goal, s_end)
        });
        let Some((route, s0, s1)) = route else { return };
        let colour = (crate::hash::mix(salt, 0xC0) % 8) as u8;
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, colour);
        let mut rng = crate::vehicle::model_rng(self.clock.tick, slotmap::Key::data(&car).as_ffi());
        self.traffic.cars[car].model = crate::vehicle::VehicleModel::for_owner(30.0 + (salt % 60) as f32, 25 + (salt % 40) as u8, 0.5, &mut rng);
        self.games.driving_in.push(FanCar { car, stadium: b });
        self.games.away_cars_total += 1;
    }

    /// A parked away fan drives home: off the map by the nearest exit.
    fn drive_out_fan(&mut self, b: BuildingId, salt: u64) {
        if self.games.parked.get(&b).is_none_or(|n| *n == 0) {
            return;
        }
        let Some((seg, pt)) = self.truck_stop(b) else { return };
        let Some((lane, s0, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { return };
        // [w1-roads] Each car pulls out from a free spot along the kerb by
        // the ground: they all used to be set down on the one spot and stood
        // there waiting to pull out, an hour after the game (Harvey's city).
        // With every spot taken it stays parked for the next minute.
        let len = self.traffic.net.lanes[lane as usize].length;
        let waiting: Vec<f32> = self.traffic.cars.values().filter(|c| c.entering && c.route.get(c.idx) == Some(&lane)).map(|c| c.s).collect();
        let Some(s0) = (0..LEAVE_SPOTS).map(|k| s0 - k as f32 * LEAVE_SPACING).filter(|s| *s >= 2.0 && *s <= len).find(|s| waiting.iter().all(|w| (w - s).abs() > LEAVE_SPACING * 0.8)) else { return };
        if let Some(n) = self.games.parked.get_mut(&b) {
            *n -= 1;
        }
        let exits = self.any_exits(pt);
        let Some((route, s0, s1)) = exits.into_iter().take(4).find_map(|(g, se)| self.traffic.net.route_lanes(lane, s0, g, se)) else { return };
        let colour = (crate::hash::mix(salt, 0xC1) % 8) as u8;
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, colour);
        let mut rng = crate::vehicle::model_rng(self.clock.tick, slotmap::Key::data(&car).as_ffi());
        self.traffic.cars[car].model = crate::vehicle::VehicleModel::for_owner(30.0 + (salt % 60) as f32, 25 + (salt % 40) as u8, 0.5, &mut rng);
    }

    /// An away fan's car reached the ground: parked there. True if it was one.
    pub(super) fn fan_car_arrived(&mut self, car: CarId) -> bool {
        let Some(k) = self.games.driving_in.iter().position(|f| f.car == car) else { return false };
        let f = self.games.driving_in.swap_remove(k);
        self.traffic.remove_car(car);
        if self.buildings.contains_key(f.stadium) {
            *self.games.parked.entry(f.stadium).or_insert(0) += 1;
        }
        true
    }
}
