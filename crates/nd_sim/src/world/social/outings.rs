//! [social] Going out together: the pull of friends at a place, outings
//! planned and walked.

use super::*;

impl World {
    /// The pull of a place on `id` from the people in their life: friends
    /// there now or regulars there draw, an enemy there now (or whose place
    /// it is) repels, a friend's band playing tonight draws hard. Read by
    /// `choose_place`.
    pub(in crate::world) fn social_pull(&self, id: CitizenId, place: BuildingId, inside: &[CitizenId]) -> f32 {
        if !self.config.social {
            return 0.0;
        }
        let mut pull = 0.0;
        let tick = self.clock.tick;
        if let Some(g) = self.social.gig_at(place, tick).or_else(|| self.social.gigs.iter().find(|g| g.venue == place && g.from > tick && g.from < tick + 120)) {
            let rep = self.social.group(g.group).map_or(0.0, |x| x.reputation);
            pull += 0.35 + 0.3 * rep;
            if let Some(grp) = self.social.group(g.group) {
                if grp.members.contains(&id) {
                    pull += 2.0;
                } else if self.social.circles.get(&id).is_some_and(|c| grp.members.iter().any(|m| c.bond(*m).is_friendly())) {
                    pull += 0.9;
                }
            }
        }
        let Some(circle) = self.social.circles.get(&id) else { return pull };
        let (mut friends_here, mut foe_here, mut foe_regular) = (0, false, false);
        for t in &circle.ties {
            let bond = t.bond();
            if !bond.is_friendly() && bond != Bond::Enemy {
                continue;
            }
            // An acquaintance from the regular haunts already draws there
            // (`choose_place`): not twice.
            let known = self.citizens.get(id).is_some_and(|c| c.places.knows(t.who));
            let here = inside.contains(&t.who) && (!known || !bond.is_friendly());
            let regular = self.citizens.get(t.who).is_some_and(|x| x.places.get(place).is_some_and(|m| m.regular));
            if bond.is_friendly() {
                friends_here += here as u32;
            } else {
                foe_here |= here;
                foe_regular |= regular;
            }
        }
        // Friends who are regulars already draw through the acquaintances
        // in `choose_place`; here only friends there now count.
        pull += 0.1 * friends_here.min(2) as f32;
        if foe_here {
            pull -= 0.6;
        }
        if foe_regular {
            pull -= 0.2;
        }
        if foe_here || foe_regular {
            self.social.avoid_hits.bump();
        }
        pull
    }

    /// [benches] A free seat on the bench of a friend sitting in `lot`, if
    /// any: (seat, facing, bench).
    pub(in crate::world) fn seat_by_friend(&self, id: CitizenId, lot: LotId) -> Option<(Vec2, f32, benches::SeatKey)> {
        let circle = self.social.circles.get(&id)?;
        for t in circle.ties.iter().filter(|t| t.bond().is_friendly()) {
            let Some(Activity::Sitting { at, .. }) = self.citizens.get(t.who).map(|c| &c.activity) else { continue };
            let Some(seat) = self.seats.taken.get(&benches::seat_key(*at)).filter(|s| s.who == t.who) else { continue };
            for &(node, _, facing) in self.walk.benches_in(lot) {
                let pos = self.walk.nodes[node as usize].pos;
                if benches::seat_key(pos) != seat.bench {
                    continue;
                }
                for s in benches::seat_positions(pos, facing) {
                    if self.seat_free(benches::seat_key(s)) {
                        return Some((s, facing, seat.bench));
                    }
                }
            }
        }
        None
    }

    /// Close friends and a partner who could come along on an outing.
    pub(in crate::world) fn social_company(&self, id: CitizenId) -> Vec<CitizenId> {
        self.social.circles.get(&id).map_or(Vec::new(), |c| c.ties.iter().filter(|t| matches!(t.bond(), Bond::CloseFriend | Bond::Partner)).map(|t| t.who).take(4).collect())
    }

    pub(super) fn free_for_outing(&self, cid: CitizenId) -> bool {
        let hour = self.clock.hour() as u8;
        let Some(c) = self.citizens.get(cid) else { return false };
        let on_shift = c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.shift_for(c.seed).contains(hour));
        matches!(c.activity, Activity::AtHome | Activity::Idle)
            && !c.asleep
            && c.wakefulness > crate::citizens::TIRED + 0.15
            && !on_shift
            && c.to_car.is_none()
            && c.age >= 14
            && c.home.is_some()
            && !self.services.in_custody(cid)
            && !self.is_patient(cid)
    }

    /// A shared favourite of `a` and `b` that is open now: a venue or a
    /// park both like (or one's regular spot the other has nothing against).
    pub(super) fn shared_place(&self, a: CitizenId, b: CitizenId) -> Option<(BuildingId, LandUse)> {
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        let (ca, cb) = (self.citizens.get(a)?, self.citizens.get(b)?);
        // A friend's band tonight first.
        for g in self.social.gigs.iter().filter(|g| g.from <= tick + 90 && g.until > tick) {
            let Some(grp) = self.social.group(g.group) else { continue };
            let draws = |c: CitizenId| grp.members.contains(&c) || self.social.circles.get(&c).is_some_and(|k| grp.members.iter().any(|m| k.bond(*m).is_friendly()));
            if (draws(a) || draws(b)) && !ca.places.is_never(g.venue) && !cb.places.is_never(g.venue) {
                return Some((g.venue, LandUse::Entertainment));
            }
        }
        let mut best: Option<(f32, BuildingId, LandUse)> = None;
        for p in &ca.places.places {
            if !matches!(p.land_use, LandUse::Entertainment | LandUse::Park) || p.never {
                continue;
            }
            let other = cb.places.get(p.place);
            if other.is_some_and(|m| m.never) {
                continue;
            }
            // A place only one of them knows will do if they love it.
            let score = p.score.min(other.map_or(if p.regular || p.score > 0.3 { p.score * 0.5 } else { -1.0 }, |m| m.score)) + if p.regular { 0.2 } else { 0.0 };
            if score < 0.0 || best.as_ref().is_some_and(|x| x.0 >= score) {
                continue;
            }
            let Some(bld) = self.buildings.get(p.place) else { continue };
            if !bld.serves_at(hour) || (p.land_use == LandUse::Park && self.park_full(bld.lot)) {
                continue;
            }
            best = Some((score, p.place, p.land_use));
        }
        best.map(|(_, b, lu)| (b, lu))
    }

    /// Friends who are both free meet up: at a place they both like (one
    /// calling round for the other when they live near, so they walk there
    /// together), or one visits the other at home.
    pub(in crate::world) fn plan_outings(&mut self) {
        let tick = self.clock.tick;
        let hour = self.clock.hour();
        let evening = (17..=22).contains(&hour);
        let cap = (self.citizens.len() / 120).max(2);
        let mut planned = 0;
        let busy: std::collections::BTreeSet<CitizenId> = self.social.outings.iter().flat_map(|o| [o.caller, o.friend]).collect();
        let ids: Vec<CitizenId> = self.social.circles.iter().filter(|(_, c)| c.ties.iter().any(|t| t.bond().is_friendly())).map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return;
        }
        let start = (mix(tick, 0x0D7) % ids.len() as u64) as usize;
        let mut taken: std::collections::BTreeSet<CitizenId> = busy;
        for k in 0..ids.len() {
            if planned >= cap {
                break;
            }
            let a = ids[(start + k) % ids.len()];
            if taken.contains(&a) || !self.free_for_outing(a) {
                continue;
            }
            let Some(c) = self.citizens.get(a) else { continue };
            // Daytime outings are for those with the day free.
            if !evening && c.work.is_some() {
                continue;
            }
            let soc = crate::places::traits(c.seed)[0];
            if hash01(c.seed, tick, 0x0F7) >= 0.08 + 0.2 * soc {
                continue;
            }
            // The friend: the closest bond first, then the warmest.
            let mut friends: Vec<(u8, f32, CitizenId)> = self.social.circles[&a].ties.iter().filter(|t| t.bond().is_friendly()).map(|t| (t.bond().rank(), -t.affinity, t.who)).collect();
            friends.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)).then(x.2.cmp(&y.2)));
            let Some(b) = friends.iter().map(|f| f.2).find(|f| !taken.contains(f) && self.free_for_outing(*f) && self.citizens.get(*f).is_some_and(|x| x.household != c.household)) else { continue };
            let (home_a, home_b) = (self.citizens[a].home, self.citizens[b].home);
            let near = match (home_a.and_then(|h| self.building_lot(h)), home_b.and_then(|h| self.building_lot(h))) {
                (Some(x), Some(y)) => x.center.distance(y.center) <= PICKUP_REACH,
                _ => false,
            };
            let (na, nb) = (self.citizens[a].name.clone(), self.citizens[b].name.clone());
            let b_home = self.citizens[b].inside.filter(|i| Some(*i) == home_b);
            match self.shared_place(a, b) {
                Some((place, lu)) => {
                    let pname = self.buildings.get(place).map(|x| x.name.clone()).unwrap_or_default();
                    if let (true, Some(h)) = (near, b_home) {
                        // Call round, then walk there together.
                        self.travel_to(a, Some(h), Purpose::Visit(LandUse::Residential));
                        if !self.citizens[a].activity.is_traveling() {
                            continue;
                        }
                        self.social.outings.push(Outing { caller: a, friend: b, place, pickup: Some(h), tick });
                        let cb = &mut self.citizens[b];
                        cb.next_decision_tick = cb.next_decision_tick.max(tick + 60);
                        cb.thought = format!("{na} is calling round. We're off to {pname}.");
                        self.citizens[a].thought = format!("Calling for {nb}, then {pname}.");
                    } else {
                        self.travel_to(a, Some(place), Purpose::Visit(lu));
                        self.travel_to(b, Some(place), Purpose::Visit(lu));
                        self.citizens[a].thought = format!("Meeting {nb} at {pname}.");
                        self.citizens[b].thought = format!("Meeting {na} at {pname}.");
                    }
                    self.citizens[a].remember(tick, format!("Met up with {nb} at {pname}."));
                    self.citizens[b].remember(tick, format!("Met up with {na} at {pname}."));
                    self.social.today.meetups += 1;
                }
                None => {
                    // Round to theirs.
                    let Some(h) = b_home else { continue };
                    if !near && hash01(c.seed, tick, 0x1107) > 0.3 {
                        continue;
                    }
                    self.travel_to(a, Some(h), Purpose::Visit(LandUse::Residential));
                    if !self.citizens[a].activity.is_traveling() {
                        continue;
                    }
                    self.citizens[a].thought = format!("Popping round to {nb}'s.");
                    let cb = &mut self.citizens[b];
                    cb.next_decision_tick = cb.next_decision_tick.max(tick + 90);
                    cb.thought = format!("{na} is coming over.");
                    self.social.today.home_visits += 1;
                }
            }
            taken.insert(a);
            taken.insert(b);
            planned += 1;
        }
    }

    /// Callers who have reached their friend's door set off together.
    pub(super) fn step_outings(&mut self) {
        let tick = self.clock.tick;
        let mut keep = Vec::new();
        for o in std::mem::take(&mut self.social.outings) {
            let Some(h) = o.pickup else { continue };
            let caller = self.citizens.get(o.caller).map(|c| c.activity.clone());
            let arrived = matches!(caller, Some(Activity::Visiting(b)) if b == h);
            let on_way = matches!(caller, Some(Activity::Traveling { to: Some(b), .. }) | Some(Activity::Driving { to: Some(b), .. }) if b == h) || self.citizens.get(o.caller).is_some_and(|c| c.to_car.is_some_and(|t| t.to == h));
            if !arrived {
                if on_way && tick < o.tick + OUTING_TTL {
                    keep.push(o);
                }
                continue;
            }
            let friend_in = self.citizens.get(o.friend).is_some_and(|c| c.inside == Some(h) && matches!(c.activity, Activity::AtHome | Activity::Visiting(_)) && !c.asleep);
            let lu = self.buildings.get(o.place).map(|b| if b.land_use == LandUse::Park { LandUse::Park } else { LandUse::Entertainment });
            let Some(lu) = lu else { continue };
            if !friend_in {
                // Nobody in: they go on alone.
                self.travel_to(o.caller, Some(o.place), Purpose::Visit(lu));
                continue;
            }
            self.travel_to(o.caller, Some(o.place), Purpose::Visit(lu));
            self.travel_to(o.friend, Some(o.place), Purpose::Visit(lu));
            let walking = |w: &World, c: CitizenId| w.citizens.get(c).is_some_and(|x| matches!(x.activity, Activity::Traveling { .. }) && x.mode == TravelMode::Walk);
            if walking(self, o.caller) && walking(self, o.friend) {
                self.social.walking_with.insert(o.caller, (o.friend, tick + WALK_TOGETHER));
                self.social.walking_with.insert(o.friend, (o.caller, tick + WALK_TOGETHER));
            }
        }
        self.social.outings = keep;
    }

    /// Is `cid` walking with a friend right now? (For the renderer.)
    pub fn walking_with(&self, cid: CitizenId) -> Option<CitizenId> {
        let (f, until) = self.social.walking_with.get(&cid)?;
        (self.clock.tick < *until).then_some(*f)
    }
}
