//! [social] A conversation: what it is about and what it does to the tie.

use super::*;

impl World {
    /// The city's top concern this week, for small talk (refreshed hourly).
    pub(in crate::world) fn concern_of_the_week(&self) -> Option<crate::concerns::Concern> {
        self.concerns.ranked(self.clock.day()).first().map(|r| r.concern)
    }

    pub(super) fn pick_topic(&self, a: CitizenId, b: CitizenId, spot: Spot, salt: u64) -> (Topic, Option<CitizenId>, Option<BuildingId>) {
        let (ca, cb) = (&self.citizens[a], &self.citizens[b]);
        let ia = interests(ca.seed, ca.age, ca.class());
        let ib = interests(cb.seed, cb.age, cb.class());
        let shared = |k: GroupKind| ia[k.index()].min(ib[k.index()]);
        let mut cands: Vec<(Topic, f32, Option<CitizenId>, Option<BuildingId>)> = Vec::with_capacity(12);
        if ca.work.is_some() && cb.work.is_some() {
            cands.push((Topic::Work, if spot.setting == Setting::Work { 3.0 } else { 0.8 }, None, None));
        }
        let stress = |c: &Citizen| c.rent_stress_days > 0 || c.household.and_then(|h| self.households.get(h)).is_some_and(|h| h.rent_stress_days > 0);
        if ca.home.is_some() && cb.home.is_some() {
            cands.push((Topic::Rent, if stress(ca) || stress(cb) { 2.0 } else { 0.4 }, None, None));
        }
        if let Some(c) = self.social_concern {
            cands.push((Topic::City(c), 0.9, None, None));
        }
        if self.society.crime.offences_today() > 0 || !self.street_events.news.is_empty() {
            cands.push((Topic::News, 0.5, None, None));
        }
        let stadium = spot.setting == Setting::Stadium;
        cands.push((Topic::Sports, if stadium { 5.0 } else { 1.2 * shared(GroupKind::SportsClub) }, None, None));
        // A place both know and like (the one they are in, at a venue).
        let mut shared_place: Option<(BuildingId, f32)> = None;
        for p in &ca.places.places {
            if let Some(q) = cb.places.get(p.place) {
                let s = p.score.min(q.score);
                if shared_place.is_none_or(|(_, x)| s > x) {
                    shared_place = Some((p.place, s));
                }
            }
        }
        if let Some((p, _)) = shared_place {
            cands.push((Topic::Place, 1.4, None, Some(p)));
        } else if let (Some(p), Setting::Venue | Setting::Park | Setting::Bench) = (spot.place, spot.setting) {
            cands.push((Topic::Place, 0.6, None, Some(p)));
        }
        // Somebody they both know.
        if let (Some(ka), Some(kb)) = (self.social.circles.get(&a), self.social.circles.get(&b)) {
            let third = ka.ties.iter().filter(|t| t.who != b && t.familiarity > 0.15).filter(|t| kb.tie(t.who).is_some()).max_by(|x, y| x.affinity.abs().total_cmp(&y.affinity.abs())).map(|t| t.who);
            if let Some(t) = third {
                cands.push((Topic::Gossip, 1.3, Some(t), None));
            }
        }
        cands.push((Topic::Music, 2.0 * shared(GroupKind::Band), None, None));
        cands.push((Topic::Art, 1.6 * shared(GroupKind::StreetArt), None, None));
        cands.push((Topic::Books, 1.6 * shared(GroupKind::BookClub), None, None));
        cands.push((Topic::Weather, if spot.setting.is_open_air() { 1.0 } else { 0.25 }, None, None));
        let fam = |c: &Citizen| c.household.and_then(|h| self.households.get(h)).is_some_and(|h| h.size() > 1);
        if fam(ca) && fam(cb) {
            cands.push((Topic::Family, 0.5, None, None));
        }
        let total: f32 = cands.iter().map(|c| c.1.max(0.0)).sum();
        let mut r = hash01(salt, self.clock.tick, 0x70_91C) * total;
        for c in &cands {
            if r < c.1.max(0.0) {
                return (c.0, c.2, c.3);
            }
            r -= c.1.max(0.0);
        }
        (Topic::Weather, None, None)
    }

    /// `a` and `b` talk at `spot`; both ties move.
    pub(in crate::world) fn converse(&mut self, a: CitizenId, b: CitizenId, spot: Spot) {
        let tick = self.clock.tick;
        let (ca, cb) = (&self.citizens[a], &self.citizens[b]);
        let salt = pair_key(ca.seed, cb.seed);
        let compat = compatibility((ca.seed, ca.age, ca.class()), (cb.seed, cb.age, cb.class()));
        let (ta, tb) = (crate::places::traits(ca.seed), crate::places::traits(cb.seed));
        let tie_ab = self.social.circles.get(&a).and_then(|c| c.tie(b)).cloned();
        let tie_ba = self.social.circles.get(&b).and_then(|c| c.tie(a)).cloned();
        let aff = (tie_ab.as_ref().map_or(0.0, |t| t.affinity) + tie_ba.as_ref().map_or(0.0, |t| t.affinity)) * 0.5;
        let trust = (tie_ab.as_ref().map_or(0.2, |t| t.trust) + tie_ba.as_ref().map_or(0.2, |t| t.trust)) * 0.5;
        let mood = (ca.mood + cb.mood) * 0.5;
        let drunk = ca.drunk.max(cb.drunk);
        let (topic, about, topic_place) = self.pick_topic(a, b, spot, salt);
        let class_gap = (ca.class().index() as f32 - cb.class().index() as f32).abs() * 0.5;
        let x = 1.1 + 2.2 * compat + 1.7 * aff + 1.0 * (mood - 0.5) - topic.heat() * (0.3 + 1.2 * class_gap) - 0.6 * drunk;
        let p_good = 1.0 / (1.0 + (-x).exp());
        let roll = hash01(salt, tick, 0x0C7);
        let s = hash01(salt, tick, 0x5C7);
        let hed = (ta[2] + tb[2]) * 0.5;
        let single = |c: CitizenId| self.single_for(c, a, b);
        let partners = tie_ab.as_ref().is_some_and(|t| t.partner);
        let in_need = |c: &Citizen| c.work.is_none() && c.can_work() || c.savings < 0.0;
        let outcome = if roll < p_good {
            if may_court(ca, cb) && single(a) && single(b) && (partners || (compat > 0.1 && aff > 0.25)) && s < 0.12 + 0.2 * hed + if partners { 0.3 } else { 0.0 } {
                Outcome::Flirted
            } else if trust > 0.35 && (in_need(ca) || in_need(cb)) && s < 0.35 {
                Outcome::Favour
            } else if s < 0.45 + 0.25 * hed {
                Outcome::Laughed
            } else {
                Outcome::Agreed
            }
        } else {
            let rebel = crate::citizens::rebelliousness(ta[3].min(tb[3]), hed);
            let brawl_spot = matches!(spot.setting, Setting::Venue | Setting::Street | Setting::Park | Setting::Stadium | Setting::Stop);
            // A workplace feud boils over now and then too.
            let brawl = if brawl_spot { 0.12 + 0.25 * drunk } else if spot.setting == Setting::Work { 0.05 } else { 0.0 };
            if aff <= ENEMY_AFFINITY * 0.8 && (drunk > 0.2 || rebel > 0.5) && s < brawl {
                Outcome::Fight
            } else if (aff < -0.2 && s < 0.5) || s < 0.15 {
                Outcome::Insulted
            } else {
                Outcome::Argued
            }
        };
        // Both ties move, and drift towards how well they suit.
        let met = spot.setting.met();
        let mut turns: [(Bond, Bond); 2] = [(Bond::Stranger, Bond::Stranger); 2];
        for (k, (me, other)) in [(a, b), (b, a)].into_iter().enumerate() {
            let circle = self.social.circles.entry(me).or_default();
            let t = circle.tie_or_new(other, tick, met, spot.place);
            let before = t.bond();
            let fresh = 1.0 - 0.5 * t.familiarity;
            t.familiarity = (t.familiarity + 0.08 * (1.0 - t.familiarity)).min(1.0);
            t.affinity = (t.affinity + outcome.affinity() * fresh + 0.04 * (compat - t.affinity)).clamp(-1.0, 1.0);
            t.trust = (t.trust + outcome.trust()).clamp(0.0, 1.0);
            if outcome == Outcome::Flirted {
                t.spark = (t.spark + 0.18).min(1.0);
            }
            t.talks = t.talks.saturating_add(1);
            t.last = tick;
            turns[k] = (before, t.bond());
            circle.note(TalkNote { tick, with: other, topic, outcome, place: spot.place, setting: spot.setting });
        }
        // Moods.
        let dm = match outcome {
            Outcome::Fight => -0.12,
            o if o.is_good() => 0.02,
            _ => -0.025,
        };
        for id in [a, b] {
            if let Some(c) = self.citizens.get_mut(id) {
                c.mood = (c.mood + dm).clamp(0.0, 1.0);
            }
        }
        // Notable turns go in the memory stream.
        let (na, nb) = (self.citizens[a].name.clone(), self.citizens[b].name.clone());
        let place_name = spot.place.and_then(|p| self.buildings.get(p)).map(|x| x.name.clone());
        let at = place_name.as_ref().map_or(String::new(), |p| format!(" at {p}"));
        let (before, after) = turns[0];
        if after != before {
            match after {
                Bond::Friend if !before.is_friendly() => {
                    self.social.today.friendships += 1;
                    self.social.friendships_total += 1;
                    self.remember_both(a, b, |o| format!("{o} and I are friends now."));
                }
                Bond::CloseFriend => {
                    self.social.today.close_friendships += 1;
                    self.remember_both(a, b, |o| format!("{o} is one of my closest friends."));
                }
                Bond::Rival if !before.is_hostile() => {
                    self.social.today.feuds += 1;
                    self.social.feuds_total += 1;
                    self.remember_both(a, b, |o| format!("Can't stand {o}."));
                }
                Bond::Enemy => {
                    if !before.is_hostile() {
                        self.social.today.feuds += 1;
                        self.social.feuds_total += 1;
                    }
                    self.remember_both(a, b, |o| format!("{o} is dead to me."));
                }
                _ => {}
            }
        }
        // A couple.
        let spark = |me: CitizenId, other: CitizenId| self.social.circles.get(&me).and_then(|c| c.tie(other)).map_or(0.0, |t| if t.affinity >= 0.5 { t.spark } else { 0.0 });
        if outcome == Outcome::Flirted && !partners && spark(a, b) >= 0.55 && spark(b, a) >= 0.55 && self.single_for(a, a, b) && self.single_for(b, a, b) {
            for (me, other) in [(a, b), (b, a)] {
                if let Some(t) = self.social.circles.get_mut(&me).and_then(|c| c.tie_mut(other)) {
                    t.partner = true;
                }
            }
            self.social.today.couples += 1;
            self.social.couples_total += 1;
            self.remember_both(a, b, |o| format!("{o} and I are together now."));
        }
        // Gossip: the listener takes on some of the speaker's feeling for
        // the third person, as far as they trust the speaker.
        if let (Topic::Gossip, Some(c)) = (topic, about) {
            for (speaker, listener) in [(a, b), (b, a)] {
                let felt = self.social.circles.get(&speaker).and_then(|k| k.tie(c)).map_or(0.0, |t| t.affinity);
                let trust = self.social.circles.get(&listener).and_then(|k| k.tie(speaker)).map_or(0.0, |t| t.trust);
                if let Some(t) = self.social.circles.get_mut(&listener).and_then(|k| k.tie_mut(c)) {
                    t.affinity = (t.affinity + 0.12 * felt * trust).clamp(-1.0, 1.0);
                }
                if felt < -0.2 {
                    self.social.today.gossip += 1;
                }
            }
        }
        // Word of mouth about a place: a bad word costs it a little.
        if let (Topic::Place, Some(p)) = (topic, topic_place) {
            let worst = [a, b].iter().filter_map(|id| self.citizens.get(*id)).filter_map(|c| c.places.get(p)).map(|m| if m.never { -0.8 } else { m.score }).fold(1.0f32, f32::min);
            if worst < -0.2 {
                if let Some(r) = self.places.reputation.get_mut(&p) {
                    r.rating = (r.rating - 0.01).max(-1.0);
                }
                self.social.today.gossip += 1;
            }
        }
        // A favour: the better off helps the one in need.
        if outcome == Outcome::Favour {
            self.social.today.favours += 1;
        }
        // A fight: the police are called.
        let pos = self.talk_pos(a, b, spot);
        if outcome == Outcome::Fight {
            self.social.today.fights += 1;
            self.social.fights_total += 1;
            let target = spot.place.filter(|p| self.buildings.contains_key(*p)).or_else(|| self.building_nearest(pos));
            // [murder] Between enemies a fight can, rarely, go too far.
            let enemies = self.social.bond(a, b) == Bond::Enemy || self.social.bond(b, a) == Bond::Enemy;
            let deadly = enemies && target.is_some() && feud_turns_deadly(salt, tick);
            if let Some(t) = target {
                let o = if deadly { crate::crime::Offence::Murder } else { crate::crime::Offence::Assault };
                self.society.crime.stats.offences_today[o.index()] += 1;
                self.society.crime.stats.offences_total[o.index()] += 1;
                if !deadly {
                    self.report_crime(crate::services::CrimeKind::Assault, t, Some(a), Some(b));
                }
            }
            let where_ = place_name.clone().unwrap_or_else(|| "the street".into());
            for (me, o) in [(a, &nb), (b, &na)] {
                if let Some(c) = self.citizens.get_mut(me) {
                    c.remember(tick, format!("Got into a fight with {o} at {where_}."));
                    c.thought = format!("{o} had it coming.");
                    self.concerns.heard(me, c, crate::concerns::Concern::Safety, 0.3);
                }
            }
            if let (true, Some(t)) = (deadly, target) {
                self.feud_killing(a, b, t);
            }
        }
        let line = talk_line(topic, outcome, place_name.as_deref().or(topic_place.and_then(|p| self.buildings.get(p)).map(|b| b.name.as_str())), about.and_then(|c| self.citizens.get(c)).map(|c| c.name.as_str()), salt ^ tick);
        if outcome.is_good() && topic != Topic::Weather && hash01(salt, tick, 0x7400) < 0.25 {
            if let Some(c) = self.citizens.get_mut(a) {
                c.thought = format!("Good to talk {} with {nb}{at}.", topic.label());
            }
        }
        self.social.today.talks += 1;
        if outcome.is_good() {
            self.social.today.good_talks += 1;
        } else {
            self.social.today.bad_talks += 1;
        }
        self.social.talks_total += 1;
        self.social.push_talk(Talk { serial: 0, tick, a, b, topic, outcome, setting: spot.setting, place: spot.place, pos, line });
    }

    /// [murder] `killer` kills `victim`, an enemy, in a fight at `at`: a
    /// homicide whose suspect the police know (`World::murder`).
    pub(crate) fn feud_killing(&mut self, killer: CitizenId, victim: CitizenId, at: BuildingId) -> Option<u32> {
        let tick = self.clock.tick;
        let vname = self.citizens.get(victim)?.name.clone();
        if let Some(c) = self.citizens.get_mut(killer) {
            c.remember(tick, format!("The feud with {vname} ended the worst way."));
        }
        self.murder(killer, victim, at, true)
    }

    /// `c` has no partner, or it is `a` or `b`.
    pub(super) fn single_for(&self, c: CitizenId, a: CitizenId, b: CitizenId) -> bool {
        self.social.circles.get(&c).and_then(|k| k.partner()).is_none_or(|p| p == a || p == b)
    }

    /// Where a conversation shows: between two people outdoors, or at the
    /// door of the place they are in.
    pub(super) fn talk_pos(&self, a: CitizenId, b: CitizenId, spot: Spot) -> Vec2 {
        let (pa, pb) = (self.citizens[a].pos, self.citizens[b].pos);
        if spot.setting.is_open_air() || pa.distance(pb) < 6.0 {
            return (pa + pb) * 0.5;
        }
        spot.place.and_then(|p| self.building_entrance(p)).unwrap_or((pa + pb) * 0.5)
    }

    pub(super) fn remember_both(&mut self, a: CitizenId, b: CitizenId, text: impl Fn(&str) -> String) {
        let tick = self.clock.tick;
        let (na, nb) = (self.citizens.get(a).map(|c| c.name.clone()).unwrap_or_default(), self.citizens.get(b).map(|c| c.name.clone()).unwrap_or_default());
        if let Some(c) = self.citizens.get_mut(a) {
            c.remember(tick, text(&nb));
        }
        if let Some(c) = self.citizens.get_mut(b) {
            c.remember(tick, text(&na));
        }
    }
}
