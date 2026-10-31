//! Citizens' decisions each tick: the minds' answers applied.

use super::*;

/// [pt-walk] How far a stroll goes, as the crow flies, and how many
/// places are weighed for it.
pub const WANDER_REACH: f32 = 450.0;
pub const WANDER_CANDIDATES: u64 = 12;

impl World {
    pub(in crate::world) fn step_citizens(&mut self, minds: &mut dyn MindProvider) {
        let tick = self.clock.tick;
        // Tallied by `update_economy`, not every tick.
        let available = self.available;
        let population = self.citizens.len() as u32;
        let groceries_for_sale = self.groceries_for_sale;
        let meals_for_sale = self.meals_for_sale;
        let hour = self.clock.hour() as u8;
        let ids: Vec<CitizenId> = self.citizens.keys().collect();
        // [pedestrians] Walkers keep right, keep their distance and mind the
        // crossings (`world/pedestrians.rs`).
        let mut feet = self.feet_begin();

        for id in ids {
            // Walking.
            let on_foot = self.citizens[id].mode == TravelMode::Walk && matches!(self.citizens[id].activity, Activity::Traveling { .. });
            // [cyclists] Bikes stop for red lights and people on crossings.
            let riding = self.citizens[id].mode == TravelMode::Bike && matches!(self.citizens[id].activity, Activity::Traveling { .. });
            let arrived = if on_foot {
                self.step_walker(id, &mut feet)
            } else if riding {
                self.step_rider(id, &mut feet)
            } else {
                let c = &mut self.citizens[id];
                c.prev_pos = c.pos;
                c.prev_heading = c.heading;
                let speed = if c.mode == TravelMode::Bike { self.config.bike_speed } else { self.config.walk_speed };
                if let Activity::Traveling { path, idx, .. } = &mut c.activity {
                    let mut remaining = speed;
                    while remaining > 0.0 && *idx < path.len() {
                        let target = path[*idx];
                        let d = c.pos.distance(target);
                        if d <= remaining {
                            c.pos = target;
                            *idx += 1;
                            remaining -= d;
                        } else {
                            let dir = (target - c.pos) / d;
                            c.pos += dir * remaining;
                            c.heading = dir.y.atan2(dir.x);
                            remaining = 0.0;
                        }
                    }
                    *idx >= path.len()
                } else {
                    false
                }
            };
            if arrived {
                self.arrive(id);
                continue;
            }

            // Decisions. [r4-metroloop] Not while waiting for a ride either:
            // a rider whose decision fell due on the platform planned the
            // same trip again from there, walked a loop and came back to
            // wait again (Harvey's day-46 city: 3,000 metro riders a day),
            // and the wait that began afresh each time never reached
            // `WAIT_GIVE_UP`.
            if self.citizens[id].next_decision_tick > tick || self.citizens[id].activity.is_traveling() || self.citizens[id].waiting_for_ride() {
                continue;
            }
            // A late answer that came while they were on their way.
            if let Some((intent, at)) = self.deferred_intents.remove(&id) {
                if tick.saturating_sub(at) <= DEFERRED_INTENT_TTL {
                    self.citizens[id].next_decision_tick = tick + 20 + self.rng.random_range(0..40);
                    self.apply_decision(id, Decision { intent, thought: String::new(), memory: None });
                    continue;
                }
            }
            let decision = {
                let c = &self.citizens[id];
                let hh = c.household.and_then(|h| self.households.get(h));
                let on_shift = c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.business.as_ref().is_none_or(|z| !z.closed) && b.shift_for(c.seed).contains(hour));
                let ctx = MindContext {
                    id,
                    citizen: c,
                    clock: &self.clock,
                    has_home: c.home.is_some(),
                    has_work: c.work.is_some(),
                    home_name: c.home.and_then(|h| self.buildings.get(h)).map(|b| b.name.as_str()),
                    work_name: c.work.and_then(|w| self.buildings.get(w)).map(|b| b.name.as_str()),
                    available,
                    population,
                    satiety: c.satiety,
                    groceries: c.groceries,
                    groceries_for_sale,
                    meals_for_sale,
                    stage: c.stage(),
                    wakefulness: c.wakefulness,
                    asleep: c.asleep,
                    at_home: c.home.is_some() && c.inside == c.home,
                    on_shift,
                    household_size: hh.map_or(1, |h| h.size()),
                    pantry_low: hh.is_some_and(|h| h.groceries < h.pantry_target() * 0.5),
                    regular_spot: c.places.regular_of(LandUse::Entertainment).and_then(|p| self.buildings.get(p.place)).filter(|b| b.serves_at(hour)).map(|b| b.name.as_str()),
                    places_known: c.places.known(LandUse::Entertainment),
                    workout: self.gyms_open && crate::leisure::workout_hour(self.clock.hour()) && {
                        let [_, ambition, _, caution, _] = crate::places::traits(c.seed);
                        crate::leisure::gym_goer(c.seed, ambition, caution) && crate::leisure::gym_day(c.seed, self.clock.day())
                    },
                };
                minds.decide(&ctx)
            };
            match decision {
                Some(d) => {
                    self.citizens[id].next_decision_tick = tick + 20 + self.rng.random_range(0..40);
                    self.apply_decision(id, d);
                }
                None => self.citizens[id].next_decision_tick = tick + 5,
            }
        }
        self.feet_end(feet);
    }

    /// A mind's late answer (`MindProvider::poll`, an LLM thinking it over
    /// while the citizen carried on). Someone driving, riding or walking a
    /// trip cannot act on it now: setting off on foot would leave their car
    /// driving on empty or them still on the bus's list. So they think the
    /// thought now and act at their next decision, if the answer is still
    /// fresh then (`DEFERRED_INTENT_TTL`). [r4-metroloop] So does someone
    /// waiting for a ride.
    pub(in crate::world) fn apply_late_decision(&mut self, id: CitizenId, d: Decision) {
        let busy = self.citizens.get(id).is_some_and(|c| c.activity.is_traveling() || c.to_car.is_some() || c.waiting_for_ride());
        if !busy {
            self.apply_decision(id, d);
            return;
        }
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        if !d.thought.is_empty() && d.thought != c.thought {
            c.thought = d.thought;
            self.events.push(SimEvent::NewThought(id));
        }
        if let Some(m) = d.memory {
            c.remember(tick, m);
        }
        self.deferred_intents.insert(id, (d.intent, tick));
    }

    pub(in crate::world) fn apply_decision(&mut self, id: CitizenId, d: Decision) {
        let tick = self.clock.tick;
        {
            let c = &mut self.citizens[id];
            if !d.thought.is_empty() && d.thought != c.thought {
                c.thought = d.thought.clone();
                self.events.push(SimEvent::NewThought(id));
            }
            if let Some(m) = d.memory {
                c.remember(tick, m);
            }
        }
        let (home, work, inside) = {
            let c = &self.citizens[id];
            (c.home, c.work, c.inside)
        };
        match d.intent {
            Intent::Stay => {}
            Intent::GoHome => {
                if let Some(h) = home {
                    if inside != Some(h) {
                        self.travel_to(id, Some(h), Purpose::Home);
                    }
                }
            }
            Intent::GoWork => {
                if let Some(w) = work {
                    if inside != Some(w) {
                        self.travel_to(id, Some(w), Purpose::Work);
                    }
                }
            }
            Intent::Visit(lu) => {
                // Somewhere that is open: not a site, not a shut business,
                // not a derelict shell.
                // Somewhere that is open now and has room: not a site, not a
                // shut business, not a derelict shell, not packed.
                // [places] Favourites, sometimes somewhere new, never a
                // place sworn off; sometimes with company.
                let target = self.choose_place(id, lu, false);
                if let Some(b) = target {
                    if inside != Some(b) {
                        self.travel_to(id, Some(b), Purpose::Visit(lu));
                        self.bring_company(id, b, Purpose::Visit(lu));
                    }
                }
            }
            Intent::Explore(lu) => match self.choose_place(id, lu, true) {
                Some(b) if inside != Some(b) => self.travel_to(id, Some(b), Purpose::Visit(lu)),
                Some(_) => {}
                None => self.wander(id),
            },
            Intent::Wander => self.wander(id),
            Intent::Shop(r) => {
                let here = self.citizens[id].pos;
                if let Some(b) = self.choose_shop(id, r, here) {
                    if inside != Some(b) {
                        self.travel_to(id, Some(b), Purpose::Shop(r));
                    } else {
                        self.buy(id, b, r);
                    }
                }
            }
        }
    }

    /// Someone leaving home takes a meal from the pantry for later, if the
    /// pantry has one to spare.
    pub(in crate::world) fn pack_meal(&mut self, id: CitizenId) {
        crate::prof_scope!("pack_meal");
        let c = &self.citizens[id];
        if c.packed_meal >= 1.0 || c.home.is_none() || c.inside != c.home {
            return;
        }
        let Some(hh) = c.household.and_then(|h| self.households.get_mut(h)) else { return };
        if hh.groceries >= 1.0 {
            hh.groceries -= 1.0;
            let left = hh.groceries;
            let c = &mut self.citizens[id];
            c.packed_meal += 1.0;
            c.groceries = left;
        }
    }

    pub(in crate::world) fn wander(&mut self, id: CitizenId) {
        self.up_from_platform(id);
        let Some((_, _, start)) = self.origin_for(id) else { return };
        if self.roads.nodes.is_empty() {
            return;
        }
        // Bike owners who are comfortable take joy rides to a park when there is one.
        let (owns_bike, class) = {
            let c = &self.citizens[id];
            (c.owns_bike, c.class())
        };
        if owns_bike && (class == Tier::High || self.rng.random_bool(0.35)) {
            if let Some(park) = self.choose_place(id, LandUse::Park, false) {
                self.travel_to(id, Some(park), Purpose::Visit(LandUse::Park));
                return;
            }
        }
        let mode = if owns_bike && self.rng.random_bool(0.5) { TravelMode::Bike } else { TravelMode::Walk };
        let here = self.citizens[id].pos;
        // [pt-walk] One draw, as before, seeds the choice of where to go
        // (`wander_target`): somewhere pleasant within a stroll, not any
        // node in town (which sent people across the city into the
        // industrial estates).
        let draw = self.rng.random_range(0..self.walk.nodes.len().max(1) as u64);
        let target = self.wander_target(id, here, draw);
        let walk_route = if mode == TravelMode::Walk && !self.walk.nodes.is_empty() {
            target.and_then(|t| self.walk_route_for(id, here, t, Purpose::Wander))
        } else {
            None
        };
        let path = match walk_route {
            Some((pts, walk_nodes)) => {
                self.note_footfall(id, &walk_nodes);
                self.leave_building(id);
                pts
            }
            None => {
                let Some(end) = self.wander_road_end(start, target.unwrap_or(here), draw) else { return };
                let Some(nodes) = self.roads.foot_path(start, end) else { return };
                self.leave_building(id);
                self.route_path(&nodes, mode)
            }
        };
        let c = &mut self.citizens[id];
        c.mode = mode;
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
    }

    /// [pt-walk] Where a stroll from `here` heads: a dozen walk nodes on
    /// the walker's own network within [`WANDER_REACH`], picked by hashes
    /// of `draw`, and the most pleasant of them taken: by a park best, by
    /// shops and night life, homes fine, industry and nuisance (fumes,
    /// noise, fouled ground) shunned. None when nothing is in reach.
    pub(in crate::world) fn wander_target(&self, id: CitizenId, here: Vec2, draw: u64) -> Option<Vec2> {
        let seed = self.citizens.get(id).map_or(0, |c| c.seed);
        let from = self.walk.nearest_within(here, 60.0);
        let comp = from.and_then(|k| self.walk.component(k));
        let r = Vec2::splat(WANDER_REACH);
        let near: Vec<u32> = self
            .walk
            .nodes_in_box(here - r, here + r)
            .into_iter()
            .filter(|&i| {
                let n = &self.walk.nodes[i as usize];
                n.level == 0.0 && !self.walk.adj[i as usize].is_empty() && n.pos.distance(here) <= WANDER_REACH && n.pos.distance(here) >= 40.0 && (comp.is_none() || self.walk.component(i) == comp) && !self.walk.node_closed(i)
            })
            .collect();
        if near.is_empty() {
            return None;
        }
        let mut best: Option<(f32, u32)> = None;
        for k in 0..WANDER_CANDIDATES {
            let h = crate::hash::mix2_salted(seed ^ draw, k, 0x3a7d_e7);
            let i = near[(h % near.len() as u64) as usize];
            let score = self.stroll_appeal(i) + crate::hash::hash01(h, draw, 0x51) * 0.8;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| self.walk.nodes[i as usize].pos)
    }

    /// [pt-walk] How pleasant walk node `i` is to stroll to.
    pub(in crate::world) fn stroll_appeal(&self, i: u32) -> f32 {
        let n = &self.walk.nodes[i as usize];
        let mut s = 0.0f32;
        for l in n.lot.iter().chain(n.near.iter()) {
            let Some(lot) = self.lots.get(*l) else { continue };
            let open = lot.building.and_then(|b| self.buildings.get(b)).is_some_and(|b| b.operating());
            s += match (lot.land_use, open) {
                (LandUse::Park, true) => 3.0,
                (LandUse::Entertainment, true) => 1.5,
                (LandUse::Commercial, true) => 1.2,
                (LandUse::Civic, _) => 0.6,
                (LandUse::Residential, _) => 0.3,
                (LandUse::Industrial, _) => -3.0,
                _ => 0.0,
            };
        }
        s - 6.0 * self.land_value.nuisance_at(n.pos)
    }

    /// [pt-walk] A road node for a ride or a walk by the streets: near
    /// `to`, picked by `draw` among the few closest, on the way from
    /// `start`'s network.
    pub(in crate::world) fn wander_road_end(&self, start: NodeId, to: Vec2, draw: u64) -> Option<NodeId> {
        let comp = self.road_component(start);
        let mut near: Vec<(f32, u32)> = self
            .roads
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, n)| !n.segments.is_empty() && *i as u32 != start.0 && (comp.is_none() || self.road_component(NodeId(*i as u32)) == comp))
            .map(|(i, n)| (n.pos.distance(to), i as u32))
            .filter(|(d, _)| *d <= WANDER_REACH)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        near.truncate(6);
        if near.is_empty() {
            return None;
        }
        Some(NodeId(near[(draw % near.len() as u64) as usize].1))
    }
}
