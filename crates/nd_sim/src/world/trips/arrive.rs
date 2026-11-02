//! Arriving where a trip was going.

use super::*;

impl World {
    pub(in crate::world) fn arrive(&mut self, id: CitizenId) {
        crate::prof_scope!("arrive");
        let tick = self.clock.tick;
        let (to, purpose) = match &self.citizens[id].activity {
            Activity::Traveling { to, purpose, .. } | Activity::Driving { to, purpose, .. } => (*to, *purpose),
            _ => return,
        };
        // [carlife] Reached the parked car: drive on from here. If it
        // cannot get there after all, go on some other way.
        // [parking] Only where the car is (a pound trip may pass a bus stop).
        let at_car = self.citizens[id].to_car.and_then(|tc| self.vehicles.cars.get(tc.car)).and_then(|c| c.spot()).is_none_or(|s| s.pos.distance(self.citizens[id].pos) < 60.0 || s.kind != crate::vehicle::ParkKind::Pound);
        if at_car && matches!(self.citizens[id].activity, Activity::Traveling { .. }) {
            if let Some(tc) = self.citizens[id].to_car.take() {
                if !self.start_drive(id, tc.car, tc.to, tc.purpose) {
                    // [passengers] A school run without the car: the
                    // child goes alone, the parent where they were going after.
                    let (to, purpose) = self.onward_after_failed_drive(id).unwrap_or((tc.to, tc.purpose));
                    self.travel_with(id, Some(to), purpose, true, false, false);
                }
                return;
            }
        }
        // [carlife] The journey is over (a stop is a pause in it): how did
        // it feel?
        if purpose != Purpose::ToStop {
            self.log_arrived(id, to);
            self.end_trip(id);
        }
        // The destination may have been demolished on the way.
        if let Some(b) = to {
            if !self.buildings.contains_key(b) {
                let c = &mut self.citizens[id];
                c.activity = Activity::Idle;
                c.inside = None;
                c.transit = None;
                c.pending_footfall.clear();
                c.thought = "It's gone. There's nothing here any more.".to_string();
                c.next_decision_tick = tick + 1;
                return;
            }
        }
        // [ferry-islands] At the terminal for a crossing: out onto the pier.
        if let (Purpose::ToStop, Some(b)) = (purpose, to) {
            if self.reach_pier(id, b) {
                return;
            }
        }
        // [ferries] At the terminal for a day out: aboard.
        if let Some(b) = to {
            if self.board_day_trip(id, b) {
                return;
            }
        }
        // The journey is over (a stop is a pause in it): the shops along
        // the way count the passer-by now.
        if purpose != Purpose::ToStop {
            self.settle_footfall(id);
        }
        // [r5-layered] At a bay under a deck: the stalls, hawkers, lock-ups
        // or court (`world/undercroft.rs`).
        if purpose == Purpose::Wander && to.is_none() && self.under_arrive(id) {
            return;
        }
        // A park is not a building to go inside: walk its paths to a free
        // seat on a bench ([benches]), held from now so nobody else takes it.
        // [under-construction] A park that is a site (or coming down) has
        // no benches to sit on: the visit ends at the gate, below.
        let park_open = to.and_then(|b| self.buildings.get(b)).is_some_and(|b| b.operating());
        if let (Purpose::Visit(LandUse::Park), Some(b), true) = (purpose, to, park_open) {
            let lot = self.buildings.get(b).map(|bld| bld.lot);
            let pick = self.rng.random_range(0..64usize);
            let has_benches = lot.is_some_and(|l| self.walk.benches_in(l).next().is_some());
            // [social] Beside a friend already sitting there, if there is room.
            let by_friend = lot.and_then(|l| self.seat_by_friend(id, l));
            if let Some((seat, facing, bench)) = by_friend.or_else(|| lot.and_then(|l| self.free_seat(l, pick))) {
                let here = self.citizens[id].pos;
                let mut path = match self.walk.route(here, seat) {
                    Some((pts, _)) => pts,
                    None => Vec::new(),
                };
                // The route ends at the bench's node; the seat is beside it.
                if path.last().is_none_or(|p| p.distance(seat) > 1e-3) {
                    path.push(seat);
                }
                self.claim_seat(id, seat, facing, bench);
                let c = &mut self.citizens[id];
                c.mode = TravelMode::Walk;
                c.activity = Activity::Traveling { to: Some(b), purpose: Purpose::Sit, path, idx: 0 };
                return;
            }
            if has_benches {
                // Every seat taken: on to the nearest park with room, or home.
                self.seats.full_today += 1;
                self.seats.full_total += 1;
                let name = self.buildings[b].name.clone();
                // One more try, not a tour of every park in town: someone
                // turned away in the last hour gives up.
                let tried = self.seats.turned_away.get(&id).is_some_and(|&t| tick < t + benches::TRY_AGAIN_AFTER);
                self.seats.turned_away.insert(id, tick);
                if let Some(other) = (!tried).then(|| self.park_with_room(id, b)).flatten() {
                    let other_name = self.buildings[other].name.clone();
                    let c = &mut self.citizens[id];
                    c.thought = format!("Not a free bench in {name}. Trying {other_name}.");
                    c.activity = Activity::Idle;
                    self.travel_to(id, Some(other), Purpose::Visit(LandUse::Park));
                    if self.citizens[id].activity.is_traveling() {
                        return;
                    }
                }
                let c = &mut self.citizens[id];
                c.remember(tick, format!("Every bench in {name} was taken."));
                c.thought = "Not a free bench anywhere. Another day.".to_string();
                c.mood = (c.mood - 0.02).max(0.0);
                c.activity = Activity::Idle;
                c.inside = None;
                // Not straight back to the park: give it half an hour.
                c.next_decision_tick = tick + 30;
                return;
            }
        }
        // Shops may have shut, or filled up, on the way; a place to go out
        // to may have closed down or be packed (it lets late-comers in
        // after hours: last orders).
        if let (Purpose::Visit(_) | Purpose::Shop(_), Some(b)) = (purpose, to) {
            let hour = self.clock.hour() as u8;
            let bld = &self.buildings[b];
            let hours_ok = matches!(purpose, Purpose::Visit(_)) || bld.opening_hours().contains(hour);
            if !hours_ok || !bld.takes_customers() {
                let open = bld.opening_hours();
                let shut = !hours_ok || bld.business.as_ref().is_some_and(|z| z.closed) || bld.is_derelict() || !bld.operating();
                let name = bld.name.clone();
                let c = &mut self.citizens[id];
                if shut {
                    c.remember(tick, format!("{name} was shut when I got there (open {})." , open.label()));
                    c.thought = "Shut. Should have checked the hours.".to_string();
                } else {
                    c.remember(tick, format!("{name} was packed. Couldn't get in."));
                    c.thought = "Packed to the doors. Another time.".to_string();
                }
                self.concerns.heard(id, c, crate::concerns::Concern::Nightlife, 0.2);
                c.mood = (c.mood - 0.03).max(0.0);
                c.activity = Activity::Idle;
                c.inside = None;
                c.next_decision_tick = tick + 3;
                self.economy.market.turned_away_today += 1;
                return;
            }
        }
        // [benches] The seat a sitter holds (the end of the walk, which the
        // walker reached a step to the side of, keeping right) is where they
        // sit and says which way they face.
        let (seat_facing, seat_at) = match (purpose, to) {
            (Purpose::Sit, Some(b)) => {
                let end = match &self.citizens[id].activity {
                    Activity::Traveling { path, .. } => path.last().copied(),
                    _ => None,
                };
                match end.and_then(|e| self.sit_down(id, e).map(|f| (f, e))) {
                    Some((f, e)) => (Some(f), Some(e)),
                    None => {
                        let at = self.citizens[id].pos;
                        let lot = self.buildings.get(b).map(|bld| bld.lot);
                        (lot.and_then(|l| self.lots.get(l)).and_then(|l| l.park.as_ref()).and_then(|p| p.benches.iter().min_by(|x, y| x.0.distance(at).total_cmp(&y.0.distance(at)))).map(|b| b.1), None)
                    }
                }
            }
            _ => (None, None),
        };
        // [pt-transit] A metro rider goes in at the entrance and down to
        // the platform to wait, out of sight of the street (they stood at
        // the kerb by the kiosk, "waiting for a ride" no car could give).
        let platform = if purpose == Purpose::ToStop {
            self.citizens[id].transit.and_then(|t| match t.leg {
                TransitLeg::Subway { board, .. } => self.metro_platform(board),
                // [cable] Up to the cable station's platform.
                TransitLeg::Cable { board, .. } => self.cable_platform(board),
                _ => None,
            })
        } else {
            None
        };
        let c = &mut self.citizens[id];
        // [ledger] A venue's cover, paid once the citizen is let go of below.
        let mut covers: Vec<(BuildingId, f32)> = Vec::new();
        match (purpose, to) {
            (Purpose::ToStop, Some(_)) => {
                if let Some(p) = platform {
                    c.pos = p;
                }
                c.activity = Activity::Waiting { at: c.pos };
                c.inside = None;
                c.prev_pos = c.pos;
                if let Some(t) = c.transit.as_mut() {
                    t.since = tick;
                }
                return;
            }
            (Purpose::Sit, Some(b)) => {
                if let Some(seat) = seat_at {
                    c.pos = seat;
                }
                let at = c.pos;
                let facing = seat_facing;
                c.activity = Activity::Sitting { at, park: b };
                c.inside = None;
                if let Some(h) = facing {
                    c.heading = h;
                    c.prev_heading = h;
                }
                c.prev_pos = c.pos;
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                    let name = bld.name.clone();
                    c.remember(tick, format!("Sat on a bench in {name}."));
                    c.mood = (c.mood + 0.08).min(1.0);
                }
            }
            (Purpose::Home, Some(b)) => {
                c.activity = Activity::AtHome;
                c.inside = Some(b);
            }
            (Purpose::Work, Some(b)) => {
                c.activity = Activity::Working;
                c.inside = Some(b);
            }
            (Purpose::Visit(_), Some(b)) => {
                // Venues charge at the door, and turn away anyone whose
                // savings are already through the floor.
                let cover = self.buildings.get(b).and_then(|bld| bld.business.as_ref()).filter(|bz| bz.kind == BusinessKind::Venue && !bz.closed).map(|_| VENUE_COVER);
                if cover.is_some_and(|cover| c.savings - cover < OVERDRAFT) {
                    c.activity = Activity::Idle;
                    c.inside = None;
                    c.mood = (c.mood - 0.04).max(0.0);
                    c.thought = "Turned away at the door. Skint.".to_string();
                    self.concerns.said(tick, id, c, crate::concerns::Concern::Jobs, 0.3, "Turned away at the door. Skint.");
                    c.next_decision_tick = tick + 10 + self.rng.random_range(0..20);
                    return;
                }
                c.activity = Activity::Visiting(b);
                c.inside = Some(b);
                let hour = self.clock.hour() as u8;
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                    let name = bld.name.clone();
                    let kind = bld.business.as_ref().map(|z| z.kind);
                    // [r4-leisure-places] Counted by kind and hour; each
                    // place is remembered for what it is.
                    if let Some(k) = kind {
                        self.economy.market.leisure.note(k, hour);
                    }
                    c.remember(tick, crate::leisure::visit_memory(kind, &name));
                    c.mood = (c.mood + 0.05).min(1.0);
                    // A kitchen feeds the hungry from what it cooked.
                    if let Some(bz) = bld.business.as_mut().filter(|z| z.kind.is_eatery() && !z.closed) {
                        if c.satiety < 0.7 {
                            let fed = if bz.kind.cooks_meals() {
                                bz.take(Resource::BakedGoods, crate::leisure::MEAL) > 0.0
                            } else {
                                // The fine-dining kitchen cooks to order.
                                bz.take(Resource::Meat, 0.15) + bz.take(Resource::Produce, 0.15) > 0.0
                            };
                            if fed {
                                c.satiety = (c.satiety + 0.5).min(1.0);
                                self.economy.market.meals_sold_today += 1;
                                self.economy.market.leisure.meals_in_today += 1;
                            }
                        }
                    }
                    if let (Some(cover), Some(bz)) = (cover, bld.business.as_mut()) {
                        covers.push((b, cover));
                        bz.revenue_today += cover;
                        // The kitchen feeds the hungry from its groceries.
                        if c.satiety < 0.7 && bz.take(Resource::Groceries, 0.5) > 0.0 {
                            c.satiety = (c.satiety + 0.45).min(1.0);
                            self.economy.market.meals_sold_today += 1;
                        }
                    }
                }
            }
            (Purpose::Shop(r), Some(b)) => {
                c.activity = Activity::Visiting(b);
                c.inside = Some(b);
                let hour = self.clock.hour() as u8;
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                    // [r4-leisure-places] A lunch at a noodle bar counts as a visit.
                    if let Some(k) = bld.business.as_ref().map(|z| z.kind) {
                        self.economy.market.leisure.note(k, hour);
                    }
                }
                c.next_decision_tick = tick + 10 + self.rng.random_range(0..20);
                self.buy(id, b, r);
                self.rate_visit(id, b);
                return;
            }
            _ => {
                c.activity = Activity::Idle;
                c.inside = None;
            }
        }
        c.next_decision_tick = tick + 30 + self.rng.random_range(0..90);
        for (b, cover) in covers {
            self.transfer(Account::Person(id), Account::Business(b), cover, Kind::Outings);
        }
        // [places] A night out, an afternoon in the park: how was it?
        match (purpose, &self.citizens[id].activity) {
            (Purpose::Visit(_), Activity::Visiting(b)) | (Purpose::Sit, Activity::Sitting { park: b, .. }) => {
                let b = *b;
                // [attractions] A landmark's ticket at the door (a skint
                // visitor walks the free parts), counted in its books.
                if self.buildings.get(b).is_some_and(|x| x.service.is_some_and(|s| s.kind.is_landmark())) {
                    let purse = (self.citizens[id].savings - OVERDRAFT).max(0.0);
                    self.admit_visitor(b, false, purse, Account::Person(id));
                    let c = &mut self.citizens[id];
                    if let Some(name) = self.buildings.get(b).map(|x| x.name.clone()) {
                        c.remember(tick, format!("A day out at {name}."));
                    }
                }
                self.rate_visit(id, b);
            }
            _ => {}
        }
    }
}
