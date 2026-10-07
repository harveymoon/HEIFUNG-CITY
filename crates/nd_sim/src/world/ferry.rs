//! [ferries] People on the boats to towns off the map (data and the
//! sailings themselves are in `crate::ports` and `world/water.rs`).
//!
//! A ferry terminal whose water reaches the edge of the map gets one or two
//! remote terminals in towns beyond that edge when it is built
//! (`link_remote_terminals`). Its ferries take turns between those towns
//! and the other terminals in town; a run to a town sails out over the
//! edge, stands at the far pier out of sight, and comes back. When it ties
//! up again (`ferry_returned`):
//!
//! - visitors who chose to come by water (`world/tourism.rs` books them
//!   `Aboard`) step off and walk (or take a cab) to their hotel;
//! - on the morning boats, commuters from over the water come for a shift
//!   at businesses near the terminal that are short of staff. They are
//!   visitors with a workplace instead of a hotel (`Tourist::commuter`):
//!   they count as staff while they are there, the business pays them, and
//!   they walk back to the pier for an evening boat home;
//! - residents back from a day trip get off and go home.
//!
//! Residents living near a terminal sometimes take a day out
//! (`plan_day_trips`): they travel to the terminal like anywhere else, board
//! (`board_day_trip`, paying the fare both ways) and are `Abroad` until an
//! evening boat brings them back. Everyone leaving waits on the pier
//! (`Ports::boarding`) for the next run out. All draws are seeded by
//! citizen or terminal and tick, so replays repeat.

use super::*;
use crate::landscape::MapEdge;
use crate::ports::{self, DayTrip, PortId, PortKind, RemoteTerminal};
use crate::tourism::{self as tm, Arrival, Goal, Tourist, TouristId, TouristState};

/// Salt for the ferry draws.
const SALT: u64 = 0xFE44_7001;

impl World {
    /// Give a ferry terminal its towns off the map: one or two, beyond the
    /// edges its water reaches (nearest first).
    pub(super) fn link_remote_terminals(&mut self, pid: PortId) {
        let berth = self.ports.ports[pid].berth;
        let edges: Vec<MapEdge> = self.landscape.water_routes_out(berth).into_iter().map(|(e, _)| e).collect();
        self.ports.ports[pid].remote = ports::remote_terminals(berth, &edges);
    }

    /// The towns off the map a terminal runs to (empty for anything else).
    pub fn ferry_towns(&self, b: BuildingId) -> Vec<RemoteTerminal> {
        self.ports.port_of(b).filter(|p| p.kind == PortKind::FerryTerminal).map_or(Vec::new(), |p| p.remote.clone())
    }

    /// The name of a terminal's remote town `k`.
    pub fn ferry_town_name(&self, b: BuildingId, k: u8) -> String {
        self.ferry_towns(b).get(k as usize).map_or_else(|| "the next town".to_string(), |t| t.name.clone())
    }

    /// [ferries] Commuters from over the water at work now, per business:
    /// they count as staff (`hourly_economy`).
    pub fn ferry_staff(&self) -> HashMap<BuildingId, u32> {
        let mut out = HashMap::new();
        for t in self.tourism.tourists.values() {
            if let (Some(w), TouristState::Inside { building, .. }) = (t.commuter, &t.state) {
                if *building == w {
                    *out.entry(w).or_insert(0) += 1;
                }
            }
        }
        out
    }

    /// A ferry back at `terminal` from remote town `town`: everyone aboard
    /// steps off.
    pub(super) fn ferry_returned(&mut self, terminal: BuildingId, town: u8) {
        let tick = self.clock.tick;
        let aboard: Vec<TouristId> = self.tourism.tourists.iter().filter(|(_, t)| matches!(t.state, TouristState::Aboard { terminal: x, .. } if x == terminal)).map(|(id, _)| id).collect();
        for id in aboard {
            self.disembark(id, terminal);
        }
        self.commuters_off_the_boat(terminal);
        let back: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Abroad { terminal: x, until, .. } if x == terminal && until <= tick)).map(|(id, _)| id).collect();
        for id in back {
            self.home_from_day_trip(id);
        }
        let _ = town;
    }

    /// A visitor or commuter steps off the boat at `terminal` and heads for
    /// the hotel or the job.
    pub(super) fn disembark(&mut self, id: TouristId, terminal: BuildingId) {
        let at = self.building_entrance(terminal);
        let Some(t) = self.tourism.tourists.get_mut(id) else { return };
        if let Some(p) = at {
            t.pos = p;
            t.prev_pos = p;
        }
        let (commuter, hotel) = (t.commuter, t.hotel);
        // The fare in, paid at the far end, to the city's ferries.
        self.economy.credit_item(Flow::BusFares, ports::FERRY_FARE, Item::Building(terminal));
        let s = &mut self.ports.stats;
        s.ferry_riders_today += 1;
        s.ferry_fares_today += ports::FERRY_FARE;
        match commuter {
            Some(work) => {
                s.commuters_in_today += 1;
                match self.building_entrance(work) {
                    Some(to) => self.tourist_leg(id, Goal::Work(work), to),
                    None => self.tourist_depart(id),
                }
            }
            None => {
                s.visitors_in_today += 1;
                let here = self.tourism.tourists[id].pos;
                let to = self.building_entrance(hotel).unwrap_or(here);
                self.tourist_leg(id, Goal::Hotel, to);
            }
        }
    }

    /// The morning boats bring commuters for businesses near the terminal
    /// that are short of staff: a share of the gaps, up to the morning's cap.
    fn commuters_off_the_boat(&mut self, terminal: BuildingId) {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        if !(5..10).contains(&hour) {
            return;
        }
        let done = self.ports.commuters_today.get(&terminal).copied().unwrap_or(0);
        if done >= ports::MAX_COMMUTERS || self.tourism.tourists.len() >= tm::MAX_TOURISTS {
            return;
        }
        let Some(at) = self.building_entrance(terminal) else { return };
        let here = self.ferry_commuters_by_work();
        let mut gaps: Vec<(BuildingId, u32)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.construction.is_none() && !b.is_derelict() && b.business.as_ref().is_some_and(|z| !z.closed))
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| l.center.distance(at) <= ports::FERRY_CATCHMENT && l.car_segment.is_some()))
            .filter_map(|(id, b)| {
                let short = (b.jobs as u32).saturating_sub(b.workers.len() as u32 + here.get(&id).copied().unwrap_or(0));
                // Day jobs only: the boats home stop at ten.
                (short > 0 && b.shift_for(0).contains(10)).then_some((id, short))
            })
            .collect();
        let total: u32 = gaps.iter().map(|g| g.1).sum();
        if total == 0 {
            return;
        }
        let want = ((total as f32 * 0.35).ceil() as u32).min(ports::MAX_COMMUTERS - done).min(12);
        let mut rng = crate::rng_for(self.config.seed ^ SALT, tick.wrapping_mul(977) ^ slotmap::Key::data(&terminal).as_ffi());
        let day = self.clock.day();
        for _ in 0..want {
            let total: u32 = gaps.iter().map(|g| g.1).sum();
            if total == 0 {
                break;
            }
            let mut r = rng.random_range(0..total);
            let mut pick = 0;
            for (i, g) in gaps.iter().enumerate() {
                if r < g.1 {
                    pick = i;
                    break;
                }
                r -= g.1;
            }
            gaps[pick].1 -= 1;
            let work = gaps[pick].0;
            let seed = rng.random::<u64>();
            let class = if rng.random_bool(0.6) { Tier::Low } else { Tier::Mid };
            let id = self.tourism.tourists.insert(Tourist {
                seed,
                age: rng.random_range(19..64u8),
                class,
                pos: at,
                prev_pos: at,
                heading: 0.0,
                prev_heading: 0.0,
                hotel: work,
                via: Arrival::Ferry,
                port: Some(terminal),
                arrived_tick: tick,
                leave_day: day,
                money: 20.0,
                spent: 0.0,
                state: TouristState::Aboard { terminal, since: tick },
                next_tick: u64::MAX,
                sights: 0,
                commuter: Some(work),
            });
            *self.ports.commuters_today.entry(terminal).or_insert(0) += 1;
            self.disembark(id, terminal);
        }
    }

    fn ferry_commuters_by_work(&self) -> HashMap<BuildingId, u32> {
        let mut out = HashMap::new();
        for t in self.tourism.tourists.values() {
            if let Some(w) = t.commuter {
                *out.entry(w).or_insert(0) += 1;
            }
        }
        out
    }

    /// A commuter at the door of the job: in for the shift, to about five.
    pub(super) fn commuter_at_work(&mut self, id: TouristId, work: BuildingId) {
        let tick = self.clock.tick;
        let open = self.buildings.get(work).is_some_and(|b| b.construction.is_none() && b.business.as_ref().is_some_and(|z| !z.closed));
        if !open {
            self.tourist_depart(id);
            return;
        }
        let Some(t) = self.tourism.tourists.get_mut(id) else { return };
        let mut rng = crate::rng_for(t.seed, tick ^ SALT);
        let five = tick - tick % 1440 + 17 * 60 + rng.random_range(0..60u64);
        let until = if five > tick + 60 { five } else { tick + 120 };
        t.state = TouristState::Inside { building: work, until };
        t.next_tick = until;
    }

    /// Shift over: the business pays the day's wage (it leaves town with
    /// them) and they walk back to the pier.
    pub(super) fn commuter_off_shift(&mut self, id: TouristId) {
        let Some(t) = self.tourism.tourists.get(id) else { return };
        let Some(work) = t.commuter else { return };
        if let Some(b) = self.buildings.get_mut(work) {
            if let Some(bz) = b.business.as_mut() {
                bz.money -= ports::COMMUTER_WAGE;
                bz.spend_today += ports::COMMUTER_WAGE;
            }
        }
        if let Some(p) = self.building_entrance(work) {
            let t = &mut self.tourism.tourists[id];
            t.pos = p;
            t.prev_pos = p;
            t.money += ports::COMMUTER_WAGE;
        }
        self.tourist_depart(id);
    }

    // ── residents' day trips ───────────────────────────────────────────────

    /// Every hour: in the morning, residents near a terminal with towns off
    /// the map may set off for a day out; trips that never got there are
    /// dropped; anyone stranded abroad (their terminal gone) comes home.
    pub(super) fn step_ferry_people(&mut self) {
        let tick = self.clock.tick;
        if tick % 60 != 21 {
            return;
        }
        let citizens = &self.citizens;
        self.ports.trippers.retain(|t| citizens.contains_key(t.citizen) && tick < t.since + 240);
        let stranded: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(_, c)| matches!(c.activity, Activity::Abroad { until, .. } if tick > until + 300))
            .map(|(id, _)| id)
            .collect();
        for id in stranded {
            self.home_from_day_trip(id);
        }
        let hour = self.clock.hour() as u8;
        if (9..13).contains(&hour) {
            self.plan_day_trips();
        }
    }

    fn plan_day_trips(&mut self) {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        let terminals = self.ports.remote_terminals();
        for terminal in terminals {
            let Some(at) = self.building_entrance(terminal) else { continue };
            let towns = self.ferry_towns(terminal).len();
            if towns == 0 {
                continue;
            }
            let mut left = ports::MAX_DAY_TRIPS.saturating_sub(self.ports.trips_today.get(&terminal).copied().unwrap_or(0));
            let candidates: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| {
                    matches!(c.activity, Activity::AtHome)
                        && c.inside.is_some()
                        && c.inside == c.home
                        && !c.asleep
                        && c.wakefulness > 0.5
                        && c.stage() != crate::citizens::LifeStage::Child
                        && c.savings >= 2.0 * ports::FERRY_FARE + 15.0
                        && c.home.and_then(|h| self.building_entrance(h)).is_some_and(|p| p.distance(at) <= ports::FERRY_CATCHMENT)
                        && !c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| (hour..20).any(|h| b.shift_for(c.seed).contains(h)))
                })
                .map(|(id, _)| id)
                .filter(|id| !self.services.in_custody(*id) && !self.is_patient(*id))
                .collect();
            for id in candidates {
                if left == 0 {
                    break;
                }
                let c = &self.citizens[id];
                let curiosity = crate::vehicle::traits_of(c.seed)[4];
                let mut rng = crate::rng_for(c.seed ^ tick.wrapping_mul(0x51_7E11), SALT);
                if rng.random::<f32>() >= 0.006 * (0.4 + curiosity) {
                    continue;
                }
                let town = rng.random_range(0..towns) as u8;
                let lu = self.buildings.get(terminal).map_or(LandUse::Civic, |b| b.land_use);
                self.travel_to(id, Some(terminal), Purpose::Visit(lu));
                let going = self.citizens.get(id).is_some_and(|c| c.activity.is_traveling() || c.to_car.is_some());
                if going {
                    let name = self.ferry_town_name(terminal, town);
                    self.citizens[id].thought = format!("Taking the ferry to {name} for the day.");
                    self.ports.trippers.push(DayTrip { citizen: id, terminal, town, since: tick });
                    *self.ports.trips_today.entry(terminal).or_insert(0) += 1;
                    left -= 1;
                }
            }
        }
    }

    /// Reached the terminal for a day trip: pay the fare there and back and
    /// board (the next run out takes them). False when this is not a day
    /// trip, or the ferry no longer runs off the map.
    pub(super) fn board_day_trip(&mut self, id: CitizenId, b: BuildingId) -> bool {
        let Some(k) = self.ports.trippers.iter().position(|t| t.citizen == id && t.terminal == b) else { return false };
        let trip = self.ports.trippers.remove(k);
        if !self.ports.remote_terminals().contains(&b) {
            return false;
        }
        let tick = self.clock.tick;
        let fare = 2.0 * ports::FERRY_FARE;
        self.economy.credit_item(Flow::BusFares, fare, Item::Building(b));
        self.ports.stats.ferry_fares_today += fare;
        self.ports.stats.day_trips_today += 1;
        *self.ports.boarding.entry(b).or_insert(0) += 1;
        self.settle_footfall(id);
        let name = self.ferry_town_name(b, trip.town);
        let mut rng = crate::rng_for(self.citizens[id].seed ^ tick, SALT ^ 0xB0A7);
        let back = tick - tick % 1440 + 17 * 60 + rng.random_range(0..150u64);
        let c = &mut self.citizens[id];
        c.savings -= fare;
        c.inside = None;
        c.activity = Activity::Abroad { terminal: b, town: trip.town, until: back.max(tick + 180) };
        c.thought = format!("On the ferry to {name}.");
        c.remember(tick, format!("Took the ferry to {name} for the day."));
        true
    }

    /// Back from a day out: off the boat and home (straight home when the
    /// terminal is gone).
    pub(super) fn home_from_day_trip(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get(id) else { return };
        let Activity::Abroad { terminal, town, .. } = c.activity else { return };
        let name = self.ferry_town_name(terminal, town);
        let home = c.home;
        let at = self.building_entrance(terminal).or_else(|| home.and_then(|h| self.building_entrance(h))).unwrap_or(c.pos);
        self.ports.stats.day_trippers_back_today += 1;
        self.ports.stats.ferry_riders_today += 1;
        let c = &mut self.citizens[id];
        c.pos = at;
        c.prev_pos = at;
        c.activity = Activity::Idle;
        c.inside = None;
        c.mood = (c.mood + 0.06).min(1.0);
        c.thought = format!("Good day out in {name}.");
        c.next_decision_tick = tick + 1;
        if self.buildings.contains_key(terminal) {
            self.travel_to(id, home, Purpose::Home);
        } else if let Some(h) = home {
            let c = &mut self.citizens[id];
            c.activity = Activity::AtHome;
            c.inside = Some(h);
        }
    }
}
