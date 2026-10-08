//! [social] People talking: who meets whom, how it goes, and what comes of
//! it (`crate::social` holds the data and the pure rules).
//!
//! - **Encounters** (`step_social`, every `ENCOUNTER_EVERY` ticks): everyone
//!   out and about is put in a spot: a bar or club, a stadium, a bench, a
//!   park, a workplace, a friend's home, a bus or train, a bus stop, a few
//!   metres of pavement. Within each spot a few pairs are drawn (a hash of
//!   the tick and the spot) and may get talking, more readily where people
//!   talk (bars, benches, a friend's sofa) and the more sociable they are;
//!   at most `TALK_BUDGET` conversations a round, so the cost is bounded
//!   whatever the city's size and never O(n²).
//! - **Conversations** (`converse`): a topic from their lives (work, rent,
//!   the city's concern of the week, the news, the game, a place they both
//!   know, a third person, a shared passion), and an outcome from their
//!   compatibility, how they already feel about each other, their moods and
//!   how touchy the subject is: agreement, laughter, flirting, a favour, an
//!   argument, insults, now and then (enemies, drink) a fight that the
//!   police are called to. Both ties move; notable turns are remembered.
//! - **What it changes** (hourly at :25, daily at midnight): friends meet
//!   up at a place they both like (one calls round and they walk there
//!   together), visit each other at home, go to a friend's band's gig;
//!   enemies steer clear of each other's places (`social_pull`, read by
//!   `choose_place`); friends find each other jobs and flats, lend money,
//!   partners move in together; gossip sours people on a third person and
//!   on places.
//! - **Groups**: friends with a shared passion form a band, a sports club,
//!   a book club, a street-art crew, a startup, a garden group or a
//!   neighbourhood association. Members, meetings and momentum; at a
//!   threshold they make something: a gig, a match, a reading, a mural, a
//!   garden on a vacant lot, a business, a petition to the player (and a
//!   protest when it is ignored). Rivalries inside split them.
//!
//! Every roll is a hash of who, when and a salt: the world's random stream
//! is never drawn here, so the same seed makes the same friends.

use super::*;
use crate::services::ServiceKind;
use crate::social::*;

/// Ticks between encounter rounds.
pub const ENCOUNTER_EVERY: u64 = 3;
/// Conversations a round at most.
pub const TALK_BUDGET: usize = 48;
/// Pairs looked at per spot per round.
const PAIRS_PER_SPOT: usize = 3;
/// Pavement cell for passing in the street, metres.
const STREET_CELL: f32 = 4.0;
/// Friends call round for each other within this, metres.
const PICKUP_REACH: f32 = 700.0;
/// A planned outing is given up after this.
const OUTING_TTL: u64 = 150;
/// Walking together lasts at most this.
const WALK_TOGETHER: u64 = 150;

fn key64<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

fn cell(p: Vec2, size: f32) -> u64 {
    let x = (p.x / size).floor() as i32 as u32 as u64;
    let y = (p.y / size).floor() as i32 as u32 as u64;
    (x << 32) | y
}

fn setting_code(s: Setting) -> u8 {
    match s {
        Setting::Venue => 0,
        Setting::Stadium => 1,
        Setting::Bench => 2,
        Setting::Park => 3,
        Setting::Work => 4,
        Setting::Home => 5,
        Setting::Transit => 6,
        Setting::Stop => 7,
        Setting::Street => 8,
    }
}

fn pair_key(a: u64, b: u64) -> u64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    lo ^ hi.rotate_left(29) ^ 0x5_0C1A
}

/// Two people of an age to talk: adults with anyone grown, teenagers with
/// those near their own age; no small children.
fn may_talk(a: &Citizen, b: &Citizen) -> bool {
    if a.age < 12 || b.age < 12 {
        return false;
    }
    if a.age < 18 || b.age < 18 {
        return (a.age as i32 - b.age as i32).abs() <= 4;
    }
    true
}

fn may_court(a: &Citizen, b: &Citizen) -> bool {
    a.age >= 18 && b.age >= 18 && (a.age as i32 - b.age as i32).abs() <= 12
}

fn surname(name: &str) -> &str {
    name.split_whitespace().last().unwrap_or(name)
}

fn pick<'a>(list: &'a [&'a str], h: u64) -> &'a str {
    list[(h % list.len() as u64) as usize]
}

fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// What a spot is: the setting, the building (if any) and a point for the
/// bubble.
#[derive(Clone, Copy)]
pub(super) struct Spot {
    pub(super) setting: Setting,
    pub(super) place: Option<BuildingId>,
}

impl World {
    // ── the tick ───────────────────────────────────────────────────────────

    /// Every tick: outings under way; every few, a round of encounters.
    pub(super) fn step_social(&mut self) {
        if !self.config.social {
            return;
        }
        let tick = self.clock.tick;
        if !self.social.outings.is_empty() {
            self.step_outings();
        }
        if !self.social.walking_with.is_empty() && tick % 10 == 0 {
            let citizens = &self.citizens;
            self.social.walking_with.retain(|id, (_, until)| tick < *until && citizens.get(*id).is_some_and(|c| c.activity.is_traveling()));
        }
        if tick % ENCOUNTER_EVERY == 1 {
            crate::prof!("social_encounters", self.encounters());
        }
    }

    /// The citizen's people (empty when they know nobody).
    pub fn circle_of(&self, cid: CitizenId) -> Option<&Circle> {
        self.social.circles.get(&cid)
    }

    /// Pace for a walker: the slower of the two when walking with a friend.
    pub(super) fn social_pace(&self, id: CitizenId, pace: f32) -> f32 {
        match self.social.walking_with.get(&id) {
            Some((f, _)) => match self.peds.walkers.get(f) {
                Some(w) if self.citizens.get(*f).is_some_and(|c| matches!(c.activity, Activity::Traveling { .. })) => pace.min(w.pace),
                _ => pace,
            },
            None => pace,
        }
    }

    pub(super) fn encounters(&mut self) {
        let tick = self.clock.tick;
        // Homes with a guest in: only there do the people at home count
        // (neighbours at home in a block rarely meet, and most people are
        // at home, so leaving them out keeps the round cheap).
        let mut guests: Vec<u64> = self
            .citizens
            .values()
            .filter_map(|c| match c.activity {
                Activity::Visiting(b) if self.buildings.get(b).is_some_and(|x| x.land_use == LandUse::Residential) => Some(key64(b)),
                _ => None,
            })
            .collect();
        guests.sort_unstable();
        guests.dedup();
        let mut spots: Vec<(u8, u64, CitizenId)> = Vec::with_capacity(self.citizens.len() / 2);
        for (id, c) in self.citizens.iter() {
            if c.age < 12 || c.asleep {
                continue;
            }
            let s = match &c.activity {
                Activity::Visiting(b) => {
                    let Some(bld) = self.buildings.get(*b) else { continue };
                    let setting = match bld.service {
                        Some(s) if s.kind == ServiceKind::Stadium => Setting::Stadium,
                        Some(_) => Setting::Street,
                        None if bld.is_parking() => Setting::Street,
                        None if bld.land_use == LandUse::Residential => Setting::Home,
                        None if bld.land_use == LandUse::Park => Setting::Park,
                        None => Setting::Venue,
                    };
                    (setting, key64(*b))
                }
                Activity::AtHome => match c.inside.map(key64) {
                    Some(k) if guests.binary_search(&k).is_ok() => (Setting::Home, k),
                    _ => continue,
                },
                Activity::Sitting { at, park } => match self.seats.taken.get(&benches::seat_key(*at)) {
                    Some(s) if s.who == id => (Setting::Bench, ((s.bench.0 as u32 as u64) << 32) | s.bench.1 as u32 as u64),
                    _ => (Setting::Park, key64(*park)),
                },
                Activity::Working => match c.inside {
                    Some(b) => (Setting::Work, key64(b)),
                    None => continue,
                },
                Activity::Riding { bus } => (Setting::Transit, key64(*bus)),
                Activity::OnTrain { train } => (Setting::Transit, key64(*train) ^ (1 << 62)),
                Activity::OnMetro { train } => (Setting::Transit, key64(*train) ^ (2 << 62)),
                Activity::Waiting { at } => (Setting::Stop, cell(*at, 8.0)),
                Activity::Traveling { .. } if c.mode == TravelMode::Walk => (Setting::Street, cell(c.pos, STREET_CELL)),
                Activity::Idle => (Setting::Street, cell(c.pos, STREET_CELL)),
                _ => continue,
            };
            spots.push((setting_code(s.0), s.1, id));
        }
        spots.sort_unstable();
        // Spot boundaries.
        let mut groups: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < spots.len() {
            let mut j = i + 1;
            while j < spots.len() && spots[j].0 == spots[i].0 && spots[j].1 == spots[i].1 {
                j += 1;
            }
            if j - i >= 2 {
                groups.push((i, j));
            }
            i = j;
        }
        if groups.is_empty() {
            return;
        }
        let start = (mix(tick, 0x5E7) % groups.len() as u64) as usize;
        let mut budget = TALK_BUDGET;
        for k in 0..groups.len() {
            if budget == 0 {
                break;
            }
            let (a, b) = groups[(start + k) % groups.len()];
            let n = b - a;
            let (code, key, _) = spots[a];
            let attempts = (n / 2).clamp(1, PAIRS_PER_SPOT);
            for t in 0..attempts {
                if budget == 0 {
                    break;
                }
                let h = mix(tick ^ key.rotate_left(7), code as u64 * 31 + t as u64);
                let i = (h % n as u64) as usize;
                let j = (i + 1 + ((h >> 20) % (n as u64 - 1)) as usize) % n;
                let (p, q) = (spots[a + i].2, spots[a + j].2);
                if self.try_talk(p, q, code) {
                    budget -= 1;
                }
            }
        }
    }

    fn spot_of(&self, id: CitizenId, code: u8) -> Spot {
        let c = &self.citizens[id];
        let setting = [Setting::Venue, Setting::Stadium, Setting::Bench, Setting::Park, Setting::Work, Setting::Home, Setting::Transit, Setting::Stop, Setting::Street][code as usize];
        let place = match &c.activity {
            Activity::Visiting(b) | Activity::Sitting { park: b, .. } => Some(*b),
            Activity::AtHome | Activity::Working => c.inside,
            _ => None,
        };
        Spot { setting, place }
    }

    /// Two people sharing a spot may get talking. True if they did.
    fn try_talk(&mut self, a: CitizenId, b: CitizenId, code: u8) -> bool {
        let tick = self.clock.tick;
        let (Some(ca), Some(cb)) = (self.citizens.get(a), self.citizens.get(b)) else { return false };
        if a == b || !may_talk(ca, cb) || (ca.household.is_some() && ca.household == cb.household) {
            return false;
        }
        let spot = self.spot_of(a, code);
        let tie = self.social.circles.get(&a).and_then(|c| c.tie(b));
        if tie.is_some_and(|t| tick < t.last + PAIR_COOLDOWN) {
            return false;
        }
        let soc = (crate::places::traits(ca.seed)[0] + crate::places::traits(cb.seed)[0]) * 0.5;
        let mut chance = spot.setting.chattiness() * (0.35 + 1.1 * soc);
        // At home: a guest talks with the hosts; neighbours rarely meet.
        if spot.setting == Setting::Home && !matches!(ca.activity, Activity::Visiting(_)) && !matches!(cb.activity, Activity::Visiting(_)) {
            chance *= 0.06;
        }
        if let Some(t) = tie {
            let bond = t.bond();
            if bond.is_friendly() {
                chance *= 1.0 + 2.0 * t.familiarity;
            } else if bond == Bond::Enemy {
                // Enemies keep apart, unless the drink does the talking.
                chance *= 0.3 + ca.drunk.max(cb.drunk);
            } else {
                chance *= 1.0 + t.familiarity;
            }
        }
        chance *= 1.0 + 0.8 * ca.drunk.max(cb.drunk);
        if hash01(pair_key(ca.seed, cb.seed), tick, 0x7A1C) >= chance.min(0.9) {
            return false;
        }
        self.converse(a, b, spot);
        true
    }

    // ── a conversation ─────────────────────────────────────────────────────

    /// The city's top concern this week, for small talk (refreshed hourly).
    pub(super) fn concern_of_the_week(&self) -> Option<crate::concerns::Concern> {
        self.concerns.ranked(self.clock.day()).first().map(|r| r.concern)
    }

    fn pick_topic(&self, a: CitizenId, b: CitizenId, spot: Spot, salt: u64) -> (Topic, Option<CitizenId>, Option<BuildingId>) {
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
    pub(super) fn converse(&mut self, a: CitizenId, b: CitizenId, spot: Spot) {
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
    fn single_for(&self, c: CitizenId, a: CitizenId, b: CitizenId) -> bool {
        self.social.circles.get(&c).and_then(|k| k.partner()).is_none_or(|p| p == a || p == b)
    }

    /// Where a conversation shows: between two people outdoors, or at the
    /// door of the place they are in.
    fn talk_pos(&self, a: CitizenId, b: CitizenId, spot: Spot) -> Vec2 {
        let (pa, pb) = (self.citizens[a].pos, self.citizens[b].pos);
        if spot.setting.is_open_air() || pa.distance(pb) < 6.0 {
            return (pa + pb) * 0.5;
        }
        spot.place.and_then(|p| self.building_entrance(p)).unwrap_or((pa + pb) * 0.5)
    }

    fn remember_both(&mut self, a: CitizenId, b: CitizenId, text: impl Fn(&str) -> String) {
        let tick = self.clock.tick;
        let (na, nb) = (self.citizens.get(a).map(|c| c.name.clone()).unwrap_or_default(), self.citizens.get(b).map(|c| c.name.clone()).unwrap_or_default());
        if let Some(c) = self.citizens.get_mut(a) {
            c.remember(tick, text(&nb));
        }
        if let Some(c) = self.citizens.get_mut(b) {
            c.remember(tick, text(&na));
        }
    }

    // ── what ties change ───────────────────────────────────────────────────

    /// The pull of a place on `id` from the people in their life: friends
    /// there now or regulars there draw, an enemy there now (or whose place
    /// it is) repels, a friend's band playing tonight draws hard. Read by
    /// `choose_place`.
    pub(super) fn social_pull(&self, id: CitizenId, place: BuildingId, inside: &[CitizenId]) -> f32 {
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
        let (mut friends_here, mut friends_regular, mut foe_here, mut foe_regular) = (0, 0, false, false);
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
                friends_regular += regular as u32;
            } else {
                foe_here |= here;
                foe_regular |= regular;
            }
        }
        // Friends who are regulars already draw through the acquaintances
        // in `choose_place`; here only friends there now count.
        let _ = friends_regular;
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
    pub(super) fn seat_by_friend(&self, id: CitizenId, lot: LotId) -> Option<(Vec2, f32, benches::SeatKey)> {
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
    pub(super) fn social_company(&self, id: CitizenId) -> Vec<CitizenId> {
        self.social.circles.get(&id).map_or(Vec::new(), |c| c.ties.iter().filter(|t| matches!(t.bond(), Bond::CloseFriend | Bond::Partner)).map(|t| t.who).take(4).collect())
    }

    fn free_for_outing(&self, cid: CitizenId) -> bool {
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

    /// Hourly at :25: friends plan outings and call on each other, groups
    /// meet, bands play, neighbourhoods protest.
    pub(super) fn hourly_social(&mut self) {
        if !self.config.social {
            return;
        }
        crate::prof_scope!("hourly_social");
        self.social_concern = self.concern_of_the_week();
        let hour = self.clock.hour();
        if (11..=22).contains(&hour) {
            self.plan_outings();
        }
        if hour == 18 {
            self.group_meetings();
        }
        if hour == 22 {
            self.tally_meetings();
        }
        if hour == 19 {
            self.start_gigs();
        }
        if hour == 11 {
            self.start_protests();
        }
        if (20..=23).contains(&hour) {
            self.count_gig_crowds();
        }
        let tick = self.clock.tick;
        self.social.gigs.retain(|g| g.until + 1440 > tick);
    }

    /// A shared favourite of `a` and `b` that is open now: a venue or a
    /// park both like (or one's regular spot the other has nothing against).
    fn shared_place(&self, a: CitizenId, b: CitizenId) -> Option<(BuildingId, LandUse)> {
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
            // Never where an enemy of either is now.
            best = Some((score, p.place, p.land_use));
        }
        best.map(|(_, b, lu)| (b, lu))
    }

    /// Friends who are both free meet up: at a place they both like (one
    /// calling round for the other when they live near, so they walk there
    /// together), or one visits the other at home.
    pub(super) fn plan_outings(&mut self) {
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
    fn step_outings(&mut self) {
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

    // ── groups meeting and playing ─────────────────────────────────────────

    /// Members free this evening go to the group's place, on its night.
    fn group_meetings(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let meets: Vec<(u32, BuildingId, Vec<CitizenId>)> = self
            .social
            .groups
            .iter()
            .filter(|g| g.active() && mix(g.id as u64, day) % 3 == 0)
            .filter_map(|g| g.base.map(|b| (g.id, b, g.members.clone())))
            .collect();
        for (gid, base, members) in meets {
            let Some(bld) = self.buildings.get(base) else { continue };
            if !bld.serves_at(18) {
                continue;
            }
            let lu = if bld.land_use == LandUse::Park { LandUse::Park } else { LandUse::Entertainment };
            let gname = self.social.group(gid).map(|g| g.name.clone()).unwrap_or_default();
            for m in members {
                if !self.free_for_outing(m) || hash01(key64(m), tick, 0x3EE7) < 0.25 {
                    continue;
                }
                self.travel_to(m, Some(base), Purpose::Visit(lu));
                if let Some(c) = self.citizens.get_mut(m) {
                    if c.activity.is_traveling() || c.to_car.is_some() {
                        c.thought = format!("{gname} meets tonight.");
                    }
                }
            }
        }
    }

    /// Late evening: who turned up at their group's place.
    fn tally_meetings(&mut self) {
        let day = self.clock.day();
        let mut updates: Vec<(u32, Vec<CitizenId>)> = Vec::new();
        for g in self.social.groups.iter().filter(|g| g.active()) {
            let Some(base) = g.base else { continue };
            let here: Vec<CitizenId> = g.members.iter().copied().filter(|m| self.citizens.get(*m).is_some_and(|c| matches!(c.activity, Activity::Visiting(b) | Activity::Sitting { park: b, .. } if b == base))).collect();
            if here.len() >= 2 {
                updates.push((g.id, here));
            }
        }
        let tick = self.clock.tick;
        for (gid, here) in updates {
            if let Some(g) = self.social.group_mut(gid) {
                g.momentum += 0.12 * here.len() as f32;
                g.meetings += 1;
                g.last_met = day;
            }
            // Members who meet grow closer.
            for (i, &x) in here.iter().enumerate() {
                for &y in here.iter().skip(i + 1) {
                    for (me, other) in [(x, y), (y, x)] {
                        let t = self.social.circles.entry(me).or_default().tie_or_new(other, tick, Met::Group, None);
                        t.familiarity = (t.familiarity + 0.04).min(1.0);
                        t.affinity = (t.affinity + 0.03).min(1.0);
                        t.last = tick;
                    }
                }
            }
        }
    }

    /// 19:25: tonight's gigs: the band goes to play.
    fn start_gigs(&mut self) {
        let tick = self.clock.tick;
        let tonight: Vec<(u32, BuildingId)> = self.social.gigs.iter().filter(|g| g.from > tick && g.from <= tick + 60).map(|g| (g.group, g.venue)).collect();
        for (gid, venue) in tonight {
            let members = self.social.group(gid).map(|g| g.members.clone()).unwrap_or_default();
            for m in members {
                if self.free_for_outing(m) {
                    self.travel_to(m, Some(venue), Purpose::Visit(LandUse::Entertainment));
                    if let Some(c) = self.citizens.get_mut(m) {
                        c.thought = "Gig tonight. Hope somebody turns up.".to_string();
                    }
                }
            }
        }
    }

    /// 11:25: associations whose petitions were ignored rally.
    fn start_protests(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let rallies: Vec<(u32, u32, BuildingId)> = self.social.petitions.iter().filter(|p| p.state == PetitionState::Protested && p.settled == Some(day)).filter_map(|p| p.rally.map(|r| (p.id, p.group, r))).collect();
        for (_, gid, rally) in rallies {
            let Some(g) = self.social.group(gid) else { continue };
            let mut marchers = g.members.clone();
            // Members' friends come along.
            for m in g.members.clone() {
                if let Some(c) = self.social.circles.get(&m) {
                    marchers.extend(c.ties.iter().filter(|t| t.bond().is_friendly()).map(|t| t.who).take(2));
                }
            }
            marchers.sort();
            marchers.dedup();
            let lu = self.buildings.get(rally).map_or(LandUse::Civic, |b| b.land_use);
            let gname = g.name.clone();
            for m in marchers {
                if !self.free_for_outing(m) || hash01(key64(m), tick, 0x9A11) < 0.3 {
                    continue;
                }
                self.travel_to(m, Some(rally), Purpose::Visit(lu));
                if let Some(c) = self.citizens.get_mut(m) {
                    if c.activity.is_traveling() {
                        c.thought = format!("Marching with {gname}. They'll have to listen now.");
                        c.remember(tick, format!("Joined the {gname} protest."));
                    }
                }
            }
        }
    }

    // ── midnight ───────────────────────────────────────────────────────────

    pub(super) fn daily_social(&mut self) {
        if !self.config.social {
            return;
        }
        crate::prof_scope!("daily_social");
        self.prune_social();
        self.fade_ties();
        self.friends_help();
        self.loans();
        self.move_in_together();
        self.daily_groups();
        self.check_petitions();
        self.social.today.avoided = self.social.avoid_hits.take();
        let day = self.clock.day();
        self.social.roll_day(day);
    }

    /// Citizens gone: their circles and ties to them go too.
    fn prune_social(&mut self) {
        let citizens = &self.citizens;
        self.social.circles.retain(|id, _| citizens.contains_key(*id));
        for c in self.social.circles.values_mut() {
            c.ties.retain(|t| citizens.contains_key(t.who));
            c.talks.retain(|t| citizens.contains_key(t.with));
        }
        self.social.walking_with.retain(|a, (b, _)| citizens.contains_key(*a) && citizens.contains_key(*b));
        self.social.outings.retain(|o| citizens.contains_key(o.caller) && citizens.contains_key(o.friend));
        let day = self.clock.day();
        for g in self.social.groups.iter_mut() {
            g.members.retain(|m| citizens.contains_key(*m));
        }
        self.social.groups.retain(|g| g.folded.is_none_or(|d| day < d + 20));
        let groups: std::collections::BTreeSet<u32> = self.social.groups.iter().filter(|g| g.active()).map(|g| g.id).collect();
        for c in self.social.circles.values_mut() {
            c.groups.retain(|g| groups.contains(g));
        }
        self.social.circles.retain(|_, c| !c.is_empty());
    }

    /// Ties fade without contact; a couple living together keeps in touch.
    pub(super) fn fade_ties(&mut self) {
        let tick = self.clock.tick;
        let hh: BTreeMap<CitizenId, Option<HouseholdId>> = self.social.circles.keys().map(|id| (*id, self.citizens.get(*id).and_then(|c| c.household))).collect();
        let mut broke_up: Vec<(CitizenId, CitizenId)> = Vec::new();
        for (id, c) in self.social.circles.iter_mut() {
            let mine = hh.get(id).copied().flatten();
            for t in c.ties.iter_mut() {
                let together = mine.is_some() && self.citizens.get(t.who).is_some_and(|x| x.household == mine);
                if together {
                    t.last = tick;
                    continue;
                }
                let days = tick.saturating_sub(t.last) / 1440;
                if days >= 2 {
                    t.familiarity *= 0.97;
                }
                if days >= 5 {
                    t.affinity *= if t.affinity < 0.0 { 0.985 } else { 0.97 };
                    t.spark *= 0.8;
                }
                if t.partner && days >= 6 && t.affinity < 0.35 {
                    t.partner = false;
                    broke_up.push((*id, t.who));
                }
            }
            // A word exchanged once is forgotten in a few days unless it
            // left a mark; known faces fade slowly.
            c.ties.retain(|t| t.partner || t.affinity.abs() >= 0.15 || if t.talks <= 1 { tick < t.last + 3 * 1440 } else { t.familiarity >= 0.05 });
        }
        for (a, b) in broke_up {
            if let Some(t) = self.social.circles.get_mut(&b).and_then(|c| c.tie_mut(a)) {
                t.partner = false;
            }
            let other = self.citizens.get(b).map(|c| c.name.clone()).unwrap_or_default();
            if let Some(c) = self.citizens.get_mut(a) {
                c.remember(tick, format!("{other} and I drifted apart."));
            }
        }
    }

    /// Friends put in a word: a job where they work, a flat where they live.
    pub(super) fn friends_help(&mut self) {
        let tick = self.clock.tick;
        // Jobs.
        let jobless: Vec<CitizenId> = self.social.circles.keys().copied().filter(|id| self.citizens.get(*id).is_some_and(|c| c.work.is_none() && c.can_work())).collect();
        for id in jobless {
            let ties: Vec<(f32, CitizenId)> = self.social.circles[&id].ties.iter().filter(|t| t.bond().is_friendly() && t.trust >= 0.3).map(|t| (t.affinity, t.who)).collect();
            let mut best: Option<(f32, BuildingId, CitizenId)> = None;
            for (aff, f) in ties {
                let Some(w) = self.citizens.get(f).and_then(|c| c.work) else { continue };
                let Some(b) = self.buildings.get(w) else { continue };
                if b.free_jobs() == 0 || b.business.as_ref().is_some_and(|z| z.closed) || !b.operating() {
                    continue;
                }
                if best.is_none_or(|x| aff > x.0) {
                    best = Some((aff, w, f));
                }
            }
            if let Some((_, w, f)) = best {
                self.assign_job(id, w);
                let (fname, bname) = (self.citizens[f].name.clone(), self.buildings[w].name.clone());
                self.citizens[id].remember(tick, format!("{fname} put in a word for me at {bname}."));
                if let Some(t) = self.social.circles.get_mut(&id).and_then(|c| c.tie_mut(f)) {
                    t.trust = (t.trust + 0.1).min(1.0);
                    t.affinity = (t.affinity + 0.08).min(1.0);
                }
                self.social.today.jobs_found += 1;
            }
        }
        // Flats: a household squeezed by the rent hears of somewhere
        // cheaper from a friend who lives there.
        let squeezed: Vec<HouseholdId> = self.households.iter().filter(|(_, h)| h.rent_stress_days >= 2 && h.home.is_some() && h.moved_day < self.clock.day()).map(|(id, _)| id).collect();
        for hid in squeezed {
            let Some(hh) = self.households.get(hid) else { continue };
            let Some(cur) = hh.home.and_then(|h| self.buildings.get(h)) else { continue };
            let cur_rent = cur.rent;
            let income = self.household_income(hid);
            let members = hh.members.clone();
            let mut found: Option<(BuildingId, CitizenId)> = None;
            'm: for m in &members {
                let Some(c) = self.social.circles.get(m) else { continue };
                for t in c.ties.iter().filter(|t| t.bond().is_friendly()) {
                    let Some(h) = self.citizens.get(t.who).and_then(|x| x.home) else { continue };
                    let Some(b) = self.buildings.get(h) else { continue };
                    if Some(h) != hh.home && b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.rent < cur_rent * 0.9 && b.rent <= income * trips::RENT_STRESS_SHARE {
                        found = Some((h, t.who));
                        break 'm;
                    }
                }
            }
            if let Some((h, f)) = found {
                let fname = self.citizens[f].name.clone();
                self.move_household(hid, h, &format!("Moved to {{}}: {fname} told us about a cheaper flat."));
                self.social.today.homes_found += 1;
            }
        }
    }

    /// The money a citizen can lend or must borrow against: their
    /// household's purse (pockets are pooled into it every midnight, before
    /// this runs), or their own savings when they have no household.
    fn loan_purse(&self, id: CitizenId) -> Option<(Option<HouseholdId>, f32)> {
        let c = self.citizens.get(id)?;
        Some(match c.household.and_then(|h| self.households.get(h).map(|x| (h, x.money))) {
            Some((h, m)) => (Some(h), m),
            None => (None, c.savings),
        })
    }

    fn add_to_loan_purse(&mut self, id: CitizenId, amount: f32) {
        let Some(c) = self.citizens.get_mut(id) else { return };
        match c.household.and_then(|h| self.households.get_mut(h)) {
            Some(h) => h.money += amount,
            None => c.savings += amount,
        }
    }

    /// The broke borrow from friends who trust them; debts are paid back
    /// when they can be, and a debt left too long sours the friendship.
    /// [review] Money is read from the household purse: `daily_household`
    /// pools every pocket into it first, so pockets alone never qualified.
    pub(super) fn loans(&mut self) {
        let tick = self.clock.tick;
        let broke: Vec<CitizenId> = self.social.circles.keys().copied().filter(|id| self.citizens.get(*id).is_some_and(|c| c.age >= 18) && self.loan_purse(*id).is_some_and(|p| p.1 < -10.0)).collect();
        for id in broke {
            // A household member may already have borrowed for it.
            let Some((own_hh, purse)) = self.loan_purse(id) else { continue };
            if purse >= -10.0 {
                continue;
            }
            let need = (-purse + 20.0).min(120.0);
            let lender = self.social.circles[&id]
                .ties
                .iter()
                .filter(|t| t.bond().is_friendly())
                .filter_map(|t| {
                    let trust = self.social.circles.get(&t.who).and_then(|c| c.tie(id)).map_or(0.0, |x| if x.lent > 0.0 { 0.0 } else { x.trust });
                    let (their_hh, money) = self.loan_purse(t.who)?;
                    let same = own_hh.is_some() && their_hh == own_hh;
                    (!same && trust >= 0.4 && money > need + 100.0).then_some((t.who, money))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|x| x.0);
            let Some(l) = lender else { continue };
            self.add_to_loan_purse(l, -need);
            self.add_to_loan_purse(id, need);
            if let Some(t) = self.social.circles.get_mut(&l).and_then(|c| c.tie_mut(id)) {
                t.lent += need;
            }
            let (ln, bn) = (self.citizens[l].name.clone(), self.citizens[id].name.clone());
            self.citizens[id].remember(tick, format!("{ln} lent me ${need:.0}. I owe them."));
            self.citizens[l].remember(tick, format!("Lent {bn} ${need:.0}."));
            self.social.today.loans += 1;
        }
        // Paying back.
        let debts: Vec<(CitizenId, CitizenId, f32, u64)> = self.social.circles.iter().flat_map(|(l, c)| c.ties.iter().filter(|t| t.lent > 0.0).map(move |t| (*l, t.who, t.lent, t.last))).collect();
        for (l, b, owed, _) in debts {
            let Some((_, money)) = self.loan_purse(b) else { continue };
            if money > owed + 60.0 {
                self.add_to_loan_purse(b, -owed);
                self.add_to_loan_purse(l, owed);
                if let Some(t) = self.social.circles.get_mut(&l).and_then(|c| c.tie_mut(b)) {
                    t.lent = 0.0;
                    t.trust = (t.trust + 0.1).min(1.0);
                }
                let ln = self.citizens.get(l).map(|c| c.name.clone()).unwrap_or_default();
                self.citizens[b].remember(tick, format!("Paid {ln} back."));
            } else if hash01(key64(l) ^ key64(b), self.clock.day(), 0xDE87) < 0.1 {
                if let Some(t) = self.social.circles.get_mut(&l).and_then(|c| c.tie_mut(b)) {
                    t.trust = (t.trust - 0.1).max(0.0);
                    t.affinity = (t.affinity - 0.08).max(-1.0);
                }
            }
        }
    }

    /// A partner living alone moves in with the other, if the home has room
    /// in the household (five at most).
    pub(super) fn move_in_together(&mut self) {
        let tick = self.clock.tick;
        let couples: Vec<(CitizenId, CitizenId)> = self.social.circles.iter().filter_map(|(a, c)| c.partner().map(|b| (*a, b))).filter(|(a, b)| a < b).collect();
        for (a, b) in couples {
            let (Some(ca), Some(cb)) = (self.citizens.get(a), self.citizens.get(b)) else { continue };
            let (Some(ha), Some(hb)) = (ca.household, cb.household) else { continue };
            if ha == hb {
                continue;
            }
            let together = self.social.circles.get(&a).and_then(|c| c.tie(b)).is_some_and(|t| t.affinity >= 0.6 && tick.saturating_sub(t.since) >= 2 * 1440);
            if !together {
                continue;
            }
            let size = |h: HouseholdId| self.households.get(h).map_or(9, |x| x.size());
            // The one living alone moves.
            let (mover, into) = if size(ha) == 1 && size(hb) < 5 {
                (a, hb)
            } else if size(hb) == 1 && size(ha) < 5 {
                (b, ha)
            } else {
                continue;
            };
            if self.move_into_household(mover, into) {
                self.social.today.moved_in += 1;
                let home = self.households.get(into).and_then(|h| h.home).and_then(|h| self.buildings.get(h)).map(|b| b.name.clone()).unwrap_or_default();
                let other = if mover == a { b } else { a };
                let on = self.citizens[other].name.clone();
                self.citizens[mover].remember(tick, format!("Moved in with {on} at {home}."));
                let mn = self.citizens[mover].name.clone();
                self.citizens[other].remember(tick, format!("{mn} moved in with us."));
            }
        }
    }

    /// `cid` (alone in their household) joins household `into`: their home
    /// is let go, their money comes along.
    fn move_into_household(&mut self, cid: CitizenId, into: HouseholdId) -> bool {
        let Some(old) = self.citizens.get(cid).and_then(|c| c.household) else { return false };
        if self.households.get(old).is_none_or(|h| h.size() != 1) || !self.properties_of(cid).is_empty() {
            return false;
        }
        let Some(new_home) = self.households.get(into).and_then(|h| h.home) else { return false };
        if !self.buildings.contains_key(new_home) {
            return false;
        }
        let (money, groceries, old_home) = {
            let h = &self.households[old];
            (h.money, h.groceries, h.home)
        };
        let at_home = self.citizens[cid].inside.is_some() && self.citizens[cid].inside == old_home;
        if at_home {
            self.leave_building(cid);
            self.citizens[cid].activity = Activity::Idle;
        }
        if let Some(b) = old_home.and_then(|h| self.buildings.get_mut(h)) {
            b.residents.retain(|x| *x != cid);
        }
        self.remove_household(old);
        let h = &mut self.households[into];
        h.members.push(cid);
        h.money += money;
        h.groceries += groceries;
        self.buildings[new_home].residents.push(cid);
        let c = &mut self.citizens[cid];
        c.household = Some(into);
        c.home = Some(new_home);
        c.rent_stress_days = 0;
        if at_home {
            self.travel_to(cid, Some(new_home), Purpose::Home);
        }
        true
    }

    // ── groups ─────────────────────────────────────────────────────────────

    pub(super) fn daily_groups(&mut self) {
        self.form_groups();
        self.recruit();
        self.split_groups();
        self.group_momentum();
        self.produce();
        self.fold_groups();
    }

    fn interests_of(&self, c: CitizenId) -> [f32; 7] {
        self.citizens.get(c).map_or([0.0; 7], |x| interests(x.seed, x.age, x.class()))
    }

    fn groups_of(&self, c: CitizenId) -> Vec<u32> {
        self.social.circles.get(&c).map_or(Vec::new(), |k| k.groups.clone())
    }

    fn in_group_of_kind(&self, c: CitizenId, kind: GroupKind) -> bool {
        self.groups_of(c).iter().any(|g| self.social.group(*g).is_some_and(|x| x.kind == kind))
    }

    /// Friends with a passion in common start something.
    pub(super) fn form_groups(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let active = self.social.active_groups().count();
        let cap = (self.citizens.len() / 45).max(2);
        if active >= cap {
            return;
        }
        let ids: Vec<CitizenId> = self.social.circles.iter().filter(|(_, c)| c.friends().count() >= 2 && c.groups.len() < MAX_GROUPS_EACH).map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return;
        }
        let start = (mix(day, 0x6A0) % ids.len() as u64) as usize;
        let mut formed = 0;
        let mut per_kind = [0u32; 7];
        for g in self.social.active_groups() {
            per_kind[g.kind.index()] += 1;
        }
        for k in 0..ids.len().min(800) {
            if formed >= 2 || active + formed >= cap {
                break;
            }
            let f = ids[(start + k) % ids.len()];
            let Some(fc) = self.citizens.get(f) else { continue };
            if fc.age < 15 {
                continue;
            }
            let mine = self.interests_of(f);
            // Strongest passion that is not taken care of yet.
            // A city with five bands already has less room for a sixth.
            let mut kinds: Vec<(f32, GroupKind)> = GroupKind::ALL.iter().map(|k| (mine[k.index()], *k)).filter(|(v, k)| *v >= 0.45 && !self.in_group_of_kind(f, *k)).map(|(v, k)| (v - 0.06 * per_kind[k.index()] as f32, k)).collect();
            kinds.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let Some(&(_, kind)) = kinds.first() else { continue };
            if hash01(fc.seed, day, 0x6F0 + kind.index() as u64) > 0.35 {
                continue;
            }
            let home = fc.home.and_then(|h| self.building_lot(h)).map(|l| l.center);
            let mut crew: Vec<CitizenId> = vec![f];
            let friends: Vec<CitizenId> = self.social.circles[&f].friends().map(|t| t.who).collect();
            for m in friends {
                if crew.len() >= 5 {
                    break;
                }
                let Some(mc) = self.citizens.get(m) else { continue };
                if mc.age < 15 && kind != GroupKind::Band && kind != GroupKind::SportsClub {
                    continue;
                }
                if self.interests_of(m)[kind.index()] < 0.3 || self.in_group_of_kind(m, kind) || self.groups_of(m).len() >= MAX_GROUPS_EACH {
                    continue;
                }
                // A neighbourhood is people who live near each other.
                if kind == GroupKind::Neighbourhood {
                    let theirs = mc.home.and_then(|h| self.building_lot(h)).map(|l| l.center);
                    if !matches!((home, theirs), (Some(x), Some(y)) if x.distance(y) <= 450.0) {
                        continue;
                    }
                }
                crew.push(m);
            }
            if crew.len() < kind.min_members() {
                continue;
            }
            let id = self.social.next_group;
            self.social.next_group += 1;
            let name = self.group_name(kind, f, id);
            let base = self.group_base(kind, &crew);
            let rep = 0.1 * crew.iter().map(|m| self.interests_of(*m)[kind.index()]).sum::<f32>() / crew.len() as f32;
            self.social.groups.push(Group { id, kind, name: name.clone(), founded: day, founder: f, members: crew.clone(), momentum: 0.2, reputation: rep, base, works: Vec::new(), split_from: None, last_met: day, meetings: 0, idle_days: 0, folded: None });
            for &m in &crew {
                self.social.circles.entry(m).or_default().groups.push(id);
                if let Some(c) = self.citizens.get_mut(m) {
                    c.remember(tick, if m == f { format!("Started {name}, a {}, with friends.", kind.label()) } else { format!("Joined {name}, a {}.", kind.label()) });
                }
            }
            let pos = home.unwrap_or(Vec2::ZERO);
            let fname = self.citizens[f].name.clone();
            self.social.push_news(tick, format!("{} {fname} and friends started {name}, a {}.", kind.icon(), kind.label()), pos, Some(id));
            self.social.today.groups_formed += 1;
            formed += 1;
            per_kind[kind.index()] += 1;
        }
    }

    /// Where a group meets: a venue (or for gardeners, sportspeople and
    /// associations a park) most of its members like.
    fn group_base(&self, kind: GroupKind, members: &[CitizenId]) -> Option<BuildingId> {
        let want = |b: &Building| match kind {
            GroupKind::SportsClub | GroupKind::Garden => b.land_use == LandUse::Park,
            GroupKind::Neighbourhood => b.land_use == LandUse::Park || b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::Bakery | BusinessKind::Cafe)),
            GroupKind::BookClub => b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::Bakery | BusinessKind::Venue | BusinessKind::Cafe | BusinessKind::Bookshop)),
            _ => b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue),
        };
        let mut score: BTreeMap<BuildingId, f32> = BTreeMap::new();
        for m in members {
            let Some(c) = self.citizens.get(*m) else { continue };
            for p in c.places.places.iter().filter(|p| !p.never && p.score > -0.1) {
                if self.buildings.get(p.place).is_some_and(|b| want(b)) {
                    *score.entry(p.place).or_insert(0.0) += 1.0 + p.score + if p.regular { 0.5 } else { 0.0 };
                }
            }
        }
        if let Some((b, _)) = score.into_iter().max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0))) {
            return Some(b);
        }
        // Nothing liked in common: the nearest such place to the first member.
        let at = members.first().and_then(|m| self.citizens.get(*m)).and_then(|c| c.home).and_then(|h| self.building_lot(h)).map(|l| l.center)?;
        self.buildings.iter().filter(|(_, b)| want(b) && b.operating()).filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(at)))).min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0))).map(|x| x.0)
    }

    fn street_of_home(&self, c: CitizenId) -> Option<String> {
        let lot = self.citizens.get(c)?.home.and_then(|h| self.building_lot(h))?;
        self.streets.of(lot.front_segment).map(|s| s.name.clone())
    }

    fn group_name(&self, kind: GroupKind, founder: CitizenId, id: u32) -> String {
        let h = mix(self.config.seed ^ id as u64, 0x6A3E);
        let street = self.street_of_home(founder).unwrap_or_else(|| "Market Street".into());
        let word = street.split_whitespace().next().unwrap_or("Market").to_string();
        let fname = self.citizens.get(founder).map(|c| surname(&c.name).to_string()).unwrap_or_default();
        let name = match kind {
            GroupKind::Band => format!(
                "The {} {}",
                pick(&["Static", "Velvet", "Chrome", "Broken", "Midnight", "Electric", "Hollow", "Paper", "Feral", "Signal", "Neon", "Rust"], h),
                pick(&["Saints", "Moths", "Wires", "Echoes", "Ghosts", "Foxes", "Lanterns", "Sirens", "Circuits", "Dogs", "Comets", "Vandals"], h >> 8)
            ),
            GroupKind::SportsClub => format!("{word} {}", pick(&["FC", "Runners", "Ballers", "Strikers", "Athletic", "Rovers"], h)),
            GroupKind::BookClub => format!("The {word} {}", pick(&["Readers", "Book Circle", "Page Turners", "Night Readers"], h)),
            GroupKind::StreetArt => format!("{} {}", pick(&["Spray", "Rust", "Ultraviolet", "Concrete", "Gutter", "Prism", "Neon", "Drip"], h), pick(&["Crew", "Collective", "Kids", "Syndicate"], h >> 8)),
            GroupKind::Startup => format!("{fname} {}", pick(&["Labs", "& Co.", "Works", "Systems", "Collective"], h)),
            GroupKind::Garden => format!("{word} {}", pick(&["Growers", "Green Hands", "Garden Club", "Allotments"], h)),
            GroupKind::Neighbourhood => format!("{street} Residents' Association"),
        };
        if self.social.groups.iter().any(|g| g.name == name) {
            format!("{name} {}", id + 1)
        } else {
            name
        }
    }

    /// Members' friends who share the passion join (one a group a day).
    fn recruit(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let groups: Vec<(u32, GroupKind, Vec<CitizenId>, String)> = self.social.active_groups().filter(|g| g.members.len() < MAX_MEMBERS).map(|g| (g.id, g.kind, g.members.clone(), g.name.clone())).collect();
        for (gid, kind, members, name) in groups {
            if hash01(gid as u64, day, 0x4EC) > 0.45 {
                continue;
            }
            let mut cands: Vec<(f32, CitizenId, CitizenId)> = Vec::new();
            for m in &members {
                let Some(c) = self.social.circles.get(m) else { continue };
                for t in c.friends() {
                    if members.contains(&t.who) || self.groups_of(t.who).len() >= MAX_GROUPS_EACH || self.in_group_of_kind(t.who, kind) {
                        continue;
                    }
                    let Some(x) = self.citizens.get(t.who) else { continue };
                    if x.age < 15 {
                        continue;
                    }
                    let i = self.interests_of(t.who)[kind.index()];
                    // Nobody joins a group with an enemy in it.
                    let foe = self.social.circles.get(&t.who).is_some_and(|k| members.iter().any(|y| k.bond(*y) == Bond::Enemy));
                    if i >= 0.3 && !foe {
                        cands.push((i + t.affinity, t.who, *m));
                    }
                }
            }
            cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let Some(&(_, new, via)) = cands.first() else { continue };
            if let Some(g) = self.social.group_mut(gid) {
                g.members.push(new);
                g.momentum += 0.1;
            }
            self.social.circles.entry(new).or_default().groups.push(gid);
            let vn = self.citizens.get(via).map(|c| c.name.clone()).unwrap_or_default();
            if let Some(c) = self.citizens.get_mut(new) {
                c.remember(tick, format!("{vn} got me into {name}."));
            }
        }
    }

    /// Rivals inside a group: one walks out, taking their friends; enough
    /// of them start their own.
    fn split_groups(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let groups: Vec<(u32, GroupKind, Vec<CitizenId>, String)> = self.social.active_groups().map(|g| (g.id, g.kind, g.members.clone(), g.name.clone())).collect();
        for (gid, kind, members, name) in groups {
            // The worst pair.
            let mut worst: Option<(f32, CitizenId, CitizenId)> = None;
            for &x in &members {
                let Some(c) = self.social.circles.get(&x) else { continue };
                for &y in &members {
                    if x >= y {
                        continue;
                    }
                    let a = c.tie(y).map_or(0.0, |t| t.affinity);
                    if a <= RIVAL_AFFINITY && worst.is_none_or(|w| a < w.0) {
                        worst = Some((a, x, y));
                    }
                }
            }
            let Some((a, x, y)) = worst else { continue };
            if let Some(g) = self.social.group_mut(gid) {
                g.momentum = (g.momentum - 0.15).max(0.0);
            }
            if hash01(gid as u64, day, 0x5917) > 0.15 + (-a - 0.25).max(0.0) {
                continue;
            }
            // Who leaves: the one with fewer friends in the group.
            let friends_in = |p: CitizenId| self.social.circles.get(&p).map_or(0, |c| members.iter().filter(|m| c.bond(**m).is_friendly()).count());
            let (leaver, stayer) = if friends_in(x) < friends_in(y) || (friends_in(x) == friends_in(y) && x > y) { (x, y) } else { (y, x) };
            let feel = |p: CitizenId, q: CitizenId| self.social.circles.get(&p).and_then(|c| c.tie(q)).map_or(0.0, |t| t.affinity);
            let mut out: Vec<CitizenId> = vec![leaver];
            for &m in &members {
                if m != leaver && m != stayer && feel(m, leaver) > feel(m, stayer) + 0.1 {
                    out.push(m);
                }
            }
            if out.len() * 2 >= members.len() + 1 && out.len() > 1 {
                out.truncate(members.len() / 2);
            }
            if let Some(g) = self.social.group_mut(gid) {
                g.members.retain(|m| !out.contains(m));
            }
            for m in &out {
                if let Some(c) = self.social.circles.get_mut(m) {
                    c.groups.retain(|g| *g != gid);
                }
            }
            let ln = self.citizens.get(leaver).map(|c| c.name.clone()).unwrap_or_default();
            let sn = self.citizens.get(stayer).map(|c| c.name.clone()).unwrap_or_default();
            let pos = self.home_point(leaver).unwrap_or(Vec2::ZERO);
            if out.len() >= kind.min_members() {
                let id = self.social.next_group;
                self.social.next_group += 1;
                let new_name = self.group_name(kind, leaver, id);
                let base = self.group_base(kind, &out);
                self.social.groups.push(Group { id, kind, name: new_name.clone(), founded: day, founder: leaver, members: out.clone(), momentum: 0.3, reputation: 0.0, base, works: Vec::new(), split_from: Some(gid), last_met: day, meetings: 0, idle_days: 0, folded: None });
                for m in &out {
                    self.social.circles.entry(*m).or_default().groups.push(id);
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, format!("Left {name} over {sn}. Started {new_name}."));
                    }
                }
                self.social.push_news(tick, format!("{} {name} split: {ln} walked out after a row with {sn} and started {new_name}.", kind.icon()), pos, Some(id));
            } else {
                for m in &out {
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, format!("Quit {name}. Couldn't stand {sn} any more."));
                    }
                }
                self.social.push_news(tick, format!("{} {ln} quit {name} after a row with {sn}.", kind.icon()), pos, Some(gid));
            }
            self.social.today.splits += 1;
        }
    }

    /// Momentum: members who care keep it going; it cools by itself.
    fn group_momentum(&mut self) {
        let day = self.clock.day();
        let updates: Vec<(u32, f32)> = self
            .social
            .active_groups()
            .map(|g| {
                let care: f32 = g.members.iter().map(|m| self.interests_of(*m)[g.kind.index()]).sum();
                let met_lately = if day.saturating_sub(g.last_met) <= 3 { 0.1 } else { 0.0 };
                (g.id, 0.08 * care + met_lately)
            })
            .collect();
        for (gid, gain) in updates {
            if let Some(g) = self.social.group_mut(gid) {
                g.momentum = g.momentum * 0.95 + gain;
                if g.momentum < 0.15 || g.members.len() < 2 {
                    g.idle_days += 1;
                } else {
                    g.idle_days = 0;
                }
            }
        }
    }

    fn fold_groups(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let folding: Vec<(u32, String, GroupKind, Vec<CitizenId>)> = self.social.active_groups().filter(|g| g.members.len() < 2 || g.idle_days >= 10).map(|g| (g.id, g.name.clone(), g.kind, g.members.clone())).collect();
        for (gid, name, kind, members) in folding {
            if let Some(g) = self.social.group_mut(gid) {
                g.folded = Some(day);
            }
            for m in &members {
                if let Some(c) = self.social.circles.get_mut(m) {
                    c.groups.retain(|g| *g != gid);
                }
                if let Some(c) = self.citizens.get_mut(*m) {
                    c.remember(tick, format!("{name} is finished."));
                }
            }
            let pos = members.first().and_then(|m| self.home_point(*m)).unwrap_or(Vec2::ZERO);
            self.social.push_news(tick, format!("{} {name} folded.", kind.icon()), pos, Some(gid));
            self.social.today.groups_folded += 1;
        }
        // Gardens built over, murals whose wall came down.
        let lots = &self.lots;
        let gone_gardens: Vec<Garden> = self.social.gardens.iter().filter(|g| lots.get(g.lot).is_none_or(|l| l.building.is_some() || !l.usable())).cloned().collect();
        self.social.gardens.retain(|g| lots.get(g.lot).is_some_and(|l| l.building.is_none() && l.usable()));
        for g in gone_gardens {
            let members = self.social.group(g.group).map(|x| x.members.clone()).unwrap_or_default();
            for m in members {
                if let Some(c) = self.citizens.get_mut(m) {
                    c.remember(tick, "They built over our garden.".to_string());
                    c.mood = (c.mood - 0.05).max(0.0);
                }
            }
        }
        let buildings = &self.buildings;
        self.social.murals.retain(|m| m.building.is_none_or(|b| buildings.contains_key(b)) && lots.contains_key(m.lot));
    }

    /// Groups with momentum to spare make something.
    pub(super) fn produce(&mut self) {
        let ready: Vec<(u32, GroupKind)> = self.social.active_groups().filter(|g| g.momentum >= g.kind.threshold() && g.members.len() >= g.kind.min_members()).map(|g| (g.id, g.kind)).collect();
        for (gid, kind) in ready {
            let made = match kind {
                GroupKind::Band => self.make_gig(gid),
                GroupKind::SportsClub | GroupKind::BookClub => self.make_event(gid, kind),
                GroupKind::StreetArt => self.make_mural(gid),
                GroupKind::Startup => self.make_business(gid),
                GroupKind::Garden => self.make_garden(gid),
                GroupKind::Neighbourhood => self.make_petition(gid),
            };
            if let Some(g) = self.social.group_mut(gid) {
                if made {
                    g.momentum -= g.kind.threshold() * 0.8;
                    g.reputation = (g.reputation + 0.08).min(1.0);
                } else {
                    // Nothing to be done for now: it simmers.
                    g.momentum = g.momentum.min(g.kind.threshold() * 1.3);
                }
            }
            if made {
                self.social.today.works += 1;
            }
        }
    }

    fn group_pos(&self, gid: u32) -> Vec2 {
        let g = self.social.group(gid);
        g.and_then(|g| g.base).and_then(|b| self.building_lot(b)).map(|l| l.center).or_else(|| g.and_then(|g| self.home_point(g.founder))).unwrap_or(Vec2::ZERO)
    }

    /// A band books a night at its venue (or the one its members like best).
    fn make_gig(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let Some(g) = self.social.group(gid) else { return false };
        let venue = g.base.filter(|b| self.buildings.get(*b).is_some_and(|x| x.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue && !z.closed)));
        let venue = venue.or_else(|| self.group_base(GroupKind::Band, &g.members));
        let Some(venue) = venue else { return false };
        if self.social.gigs.iter().any(|x| x.venue == venue && x.from / 1440 == day) {
            return false;
        }
        let from = day * 1440 + 20 * 60;
        let name = g.name.clone();
        self.social.gigs.push(Gig { group: gid, venue, from, until: from + 210, crowd: 0 });
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Gig { venue, day });
        }
        let vname = self.buildings.get(venue).map(|b| b.name.clone()).unwrap_or_default();
        let pos = self.building_lot(venue).map_or(Vec2::ZERO, |l| l.center);
        let tick = self.clock.tick;
        self.social.push_news(tick, format!("♪ {name} play {vname} tonight at 20:00."), pos, Some(gid));
        // A good night for the venue's name.
        let rep = self.social.group(gid).map_or(0.0, |g| g.reputation);
        let r = self.places.reputation.entry(venue).or_default();
        r.add((0.4 + rep).min(1.0));
        r.great_nights += 1;
        true
    }

    /// A match in the park, a reading at the café: the place's name rises
    /// and the members are pleased with themselves.
    fn make_event(&mut self, gid: u32, kind: GroupKind) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        let Some(place) = g.base.filter(|b| self.buildings.contains_key(*b)) else { return false };
        let (name, members, rep) = (g.name.clone(), g.members.clone(), g.reputation);
        let pname = self.buildings[place].name.clone();
        let r = self.places.reputation.entry(place).or_default();
        r.add((0.3 + rep).min(1.0));
        let (work, text, memory) = match kind {
            GroupKind::SportsClub => (Work::Match { park: place, day }, format!("⚽ {name} played a match in {pname}; half the street came to watch."), format!("{name} played in {pname}. What a game.")),
            _ => (Work::Reading { place, day }, format!("📖 {name} held a reading at {pname}."), format!("Read at the {name} evening at {pname}.")),
        };
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(work);
        }
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                c.mood = (c.mood + 0.05).min(1.0);
                c.remember(tick, memory.clone());
            }
        }
        let pos = self.building_lot(place).map_or(Vec2::ZERO, |l| l.center);
        self.social.push_news(tick, text, pos, Some(gid));
        true
    }

    /// A street-art crew paints a wall near its members: art where it is
    /// cheap and lively, unwelcome where the land is dear.
    fn make_mural(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        let at = self.group_pos(gid);
        let painted: std::collections::BTreeSet<LotId> = self.social.murals.iter().map(|m| m.lot).collect();
        let mut cands: Vec<(f32, LotId, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.service.is_none() && b.rail.is_none() && b.port.is_none() && !b.is_power() && b.land_use != LandUse::Park)
            .filter(|(_, b)| matches!(b.land_use, LandUse::Commercial | LandUse::Industrial | LandUse::Entertainment) || (b.land_use == LandUse::Residential && b.tier != Tier::High))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, b.lot, l)))
            .filter(|(_, lid, l)| !painted.contains(lid) && l.front_edge[0].distance(l.front_edge[1]) >= 10.0 && l.center.distance(at) <= 600.0)
            .map(|(id, lid, l)| (l.center.distance(at) + 300.0 * hash01(key64(lid), day, 0x3A1), lid, id))
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let Some(&(_, lot_id, bid)) = cands.first() else { return false };
        let l = &self.lots[lot_id];
        let along = (l.front_edge[1] - l.front_edge[0]).normalize_or_zero();
        let len = l.front_edge[0].distance(l.front_edge[1]);
        let inward = (l.center - (l.front_edge[0] + l.front_edge[1]) * 0.5).normalize_or_zero();
        let width = (len * 0.4).clamp(4.0, 8.0);
        let side = if hash01(key64(lot_id), 0, 0x51DE) < 0.5 { -1.0 } else { 1.0 };
        let pos = (l.front_edge[0] + l.front_edge[1]) * 0.5 + along * side * (len * 0.5 - width * 0.5 - 1.0).max(0.0) + inward * 0.9;
        let quality = (g.reputation + 0.2 * self.interests_of(g.founder)[GroupKind::StreetArt.index()]).clamp(-1.0, 1.0);
        let name = g.name.clone();
        let members = g.members.clone();
        self.social.murals.push(Mural { group: gid, lot: lot_id, building: Some(bid), seed: mix(key64(lot_id), gid as u64), day, quality, pos, along, facing: -inward, width, height: 3.4 });
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Mural { lot: lot_id, day });
        }
        let bname = self.buildings[bid].name.clone();
        self.social.push_news(tick, format!("🎨 {name} painted a mural on {bname}."), pos, Some(gid));
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                c.remember(tick, format!("We painted the wall at {bname}. Best thing we've done."));
                c.mood = (c.mood + 0.05).min(1.0);
            }
        }
        // The people who live or work there have a view on it.
        let dear = self.land_value.at(pos) > 0.6;
        let people: Vec<CitizenId> = self.buildings[bid].residents.iter().chain(self.buildings[bid].workers.iter()).copied().take(12).collect();
        for p in people {
            if let Some(c) = self.citizens.get_mut(p) {
                let likes = interests(c.seed, c.age, c.class())[GroupKind::StreetArt.index()] > 0.35 && !dear;
                c.remember(tick, if likes { format!("Somebody painted a mural on {bname}. Love it.") } else { format!("Somebody sprayed all over the wall at {bname}.") });
            }
        }
        true
    }

    /// A garden on a vacant lot near the members.
    fn make_garden(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        if self.social.gardens.iter().any(|x| x.group == gid) {
            // One garden each; tending it keeps the name up.
            return false;
        }
        let at = self.group_pos(gid);
        let used: std::collections::BTreeSet<LotId> = self.social.gardens.iter().map(|x| x.lot).collect();
        let lot = self
            .lots
            .iter()
            .filter(|(id, l)| !used.contains(id) && l.area() >= 150.0 && l.land_use != LandUse::Park && l.center.distance(at) <= 700.0)
            .filter(|(id, _)| self.lot_free_for(*id, SiteRule::STREET))
            .min_by(|a, b| a.1.center.distance(at).total_cmp(&b.1.center.distance(at)).then(a.0.cmp(&b.0)))
            .map(|(id, l)| (id, l.center));
        let Some((lot, pos)) = lot else { return false };
        let name = g.name.clone();
        let members = g.members.clone();
        self.social.gardens.push(Garden { group: gid, lot, seed: mix(key64(lot), gid as u64), day, pos });
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Garden { lot, day });
        }
        let street = self.streets.of(self.lots[lot].front_segment).map(|s| s.name.clone()).unwrap_or_else(|| "the street".into());
        self.social.push_news(tick, format!("🌱 {name} turned the empty lot on {street} into a community garden."), pos, Some(gid));
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                c.remember(tick, format!("Planted our garden on {street}."));
                c.mood = (c.mood + 0.06).min(1.0);
            }
        }
        self.events.push(SimEvent::LotsChanged);
        true
    }

    /// A startup with enough money between its members opens a real
    /// business on a vacant lot near them.
    fn make_business(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        if g.works.iter().any(|w| matches!(w, Work::Business { .. })) {
            return false;
        }
        let members = g.members.clone();
        let founder = g.founder;
        let name = g.name.clone();
        let wealth = |c: &Citizen| c.savings.max(0.0) + c.household.and_then(|h| self.households.get(h)).map_or(0.0, |h| h.money.max(0.0) * 0.5);
        let pot: f32 = members.iter().filter_map(|m| self.citizens.get(*m)).map(wealth).sum();
        if pot < 700.0 {
            return false;
        }
        // What to open: from the founders' tastes.
        let i = self.interests_of(founder);
        let hed = self.citizens.get(founder).map_or(0.5, |c| crate::places::traits(c.seed)[2]);
        let kind = if i[GroupKind::Band.index()] > 0.5 || hed > 0.7 {
            BusinessKind::Venue
        } else if mix(gid as u64, 0x5A7) % 3 == 0 {
            BusinessKind::Bakery
        } else if mix(gid as u64, 0x5A8) % 2 == 0 {
            BusinessKind::GadgetShop
        } else {
            BusinessKind::Office
        };
        let lu = if kind == BusinessKind::Venue { LandUse::Entertainment } else { LandUse::Commercial };
        let at = self.home_point(founder).unwrap_or(Vec2::ZERO);
        let lot = self
            .lots
            .iter()
            .filter(|(id, l)| l.land_use == lu && l.car_access() && self.lot_free_for(*id, SiteRule::STREET) && !self.social.gardens.iter().any(|g| g.lot == *id))
            .map(|(id, l)| (id, l.center.distance(at)))
            .filter(|x| x.1 <= 1500.0)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|x| x.0);
        let Some(lot) = lot else { return false };
        // The stake: a share of everyone's savings.
        let stake = (pot * 0.4).min(2500.0);
        let shares: Vec<(CitizenId, f32)> = members.iter().filter_map(|m| self.citizens.get(*m).map(|c| (*m, stake * wealth(c) / pot.max(1.0)))).collect();
        let Some(bid) = self.build_on_as(lot, Some(kind)) else { return false };
        for (m, share) in shares {
            let m = &m;
            let from_pocket = share.min(self.citizens[*m].savings.max(0.0));
            self.citizens[*m].savings -= from_pocket;
            let rest = share - from_pocket;
            if let Some(h) = self.citizens[*m].household.and_then(|h| self.households.get_mut(h)) {
                h.money -= rest;
            }
        }
        let bname = format!("{name}");
        if let Some(b) = self.buildings.get_mut(bid) {
            b.name = bname.clone();
            if let Some(z) = b.business.as_mut() {
                z.money += stake;
                let fname = self.citizens.get(founder).map(|c| c.name.clone()).unwrap_or_default();
                z.origin = Some(format!("Founded by {fname} and friends ({} people pooling ${stake:.0}).", members.len()));
            }
        }
        self.set_business_owner(bid, founder);
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Business { building: bid, day });
        }
        for m in &members {
            if let Some(c) = self.citizens.get_mut(*m) {
                c.remember(tick, format!("We put our savings together and are opening {bname}, a {}.", kind.label()));
            }
        }
        let pos = self.lots[lot].center;
        self.social.push_news(tick, format!("💡 {bname}: friends pooled ${stake:.0} to open a {} near their homes.", kind.label()), pos, Some(gid));
        true
    }

    /// What a neighbourhood association wants most, and why.
    fn neighbourhood_ask(&self, gid: u32) -> Option<(Ask, String, Vec2)> {
        let g = self.social.group(gid)?;
        let at = self.home_point(g.founder)?;
        let n = g.members.len().max(1) as f32;
        let stressed = g.members.iter().filter(|m| self.citizens.get(**m).is_some_and(|c| c.rent_stress_days > 0 || c.household.and_then(|h| self.households.get(h)).is_some_and(|h| h.rent_stress_days > 0))).count() as f32;
        let jobless = g.members.iter().filter(|m| self.citizens.get(**m).is_some_and(|c| c.can_work() && c.work.is_none())).count() as f32;
        let heat = self.society.crime.heat_at(at, crate::crime::FEAR_RADIUS);
        let lot = self.citizens.get(g.founder)?.home.and_then(|h| self.buildings.get(h)).map(|b| b.lot);
        let police = lot.map_or(0.0, |l| self.services.coverage_of(l, ServiceKind::Police));
        let near_misses = self.peds.recent.iter().filter(|p| p.pos.distance(at) < 300.0).count() as f32;
        let mess: f32 = self.society.trash.bins.iter().filter(|(b, _)| self.building_lot(**b).is_some_and(|l| l.center.distance(at) < 250.0)).map(|(_, x)| x.overflow().min(2.0)).sum();
        let park = self.buildings.values().filter(|b| b.land_use == LandUse::Park && b.operating()).filter_map(|b| self.lots.get(b.lot)).any(|l| l.center.distance(at) < 400.0);
        let noise = self.land_value.nuisance_at(at);
        let street = self.street_of_home(g.founder).unwrap_or_else(|| "our street".into());
        let mut asks: Vec<(f32, Ask, String)> = vec![
            (heat * 0.6 + (1.0 - police) * 0.4 * (heat > 0.3) as u8 as f32, Ask::MorePolice, format!("more police on {street}: {} break-ins and fights this month", (heat * 3.0).round().max(1.0))),
            (stressed / n * 1.2, Ask::LowerRent, format!("something done about rents round {street}: {} of us can barely pay", stressed as u32)),
            (near_misses * 0.25, Ask::Crossing, format!("a safe crossing on {street}: people keep nearly getting hit")),
            (mess * 0.3, Ask::CleanStreets, format!("the bins on {street} emptied: the rubbish is piling up")),
            (if park { 0.0 } else { 0.55 }, Ask::APark, format!("a park near {street}: nowhere for the kids to play")),
            (jobless / n * 1.1, Ask::Jobs, format!("jobs near {street}: {} of us are out of work", jobless as u32)),
            (noise * 1.5, Ask::Quiet, format!("peace and quiet on {street}: the noise is unbearable")),
        ];
        asks.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.key().cmp(b.1.key())));
        let (w, ask, why) = asks.into_iter().next()?;
        (w >= 0.3).then_some((ask, why, at))
    }

    fn ask_met(&self, p: &Petition) -> bool {
        let Some(g) = self.social.group(p.group) else { return false };
        match p.ask {
            Ask::MorePolice => self.lots.iter().filter(|(_, l)| l.center.distance(p.at) < 60.0).any(|(id, _)| self.services.coverage_of(id, ServiceKind::Police) >= 0.6),
            Ask::LowerRent => g.members.iter().all(|m| self.citizens.get(*m).is_none_or(|c| c.household.and_then(|h| self.households.get(h)).is_none_or(|h| h.rent_stress_days == 0))),
            Ask::Crossing => !self.peds.recent.iter().any(|x| x.pos.distance(p.at) < 300.0 && x.tick + 1440 * 2 > self.clock.tick),
            Ask::CleanStreets => !self.society.trash.bins.iter().any(|(b, x)| x.overflow() > 0.3 && self.building_lot(*b).is_some_and(|l| l.center.distance(p.at) < 250.0)),
            Ask::APark => self.buildings.values().filter(|b| b.land_use == LandUse::Park && b.operating()).filter_map(|b| self.lots.get(b.lot)).any(|l| l.center.distance(p.at) < 400.0),
            Ask::Jobs => g.members.iter().all(|m| self.citizens.get(*m).is_none_or(|c| !c.can_work() || c.work.is_some())),
            Ask::Quiet => self.land_value.nuisance_at(p.at) < 0.1,
        }
    }

    /// A neighbourhood association petitions the player.
    fn make_petition(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        if self.social.petitions.iter().any(|p| p.group == gid && p.state == PetitionState::Open) {
            return false;
        }
        let Some((ask, why, at)) = self.neighbourhood_ask(gid) else { return false };
        // Not the same thing again within the week.
        if self.social.petitions.iter().any(|p| p.group == gid && p.ask == ask && p.day + 7 > day) {
            return false;
        }
        let Some(g) = self.social.group(gid) else { return false };
        let (name, members, founder) = (g.name.clone(), g.members.clone(), g.founder);
        let signatures = members.len() as u32 + members.iter().filter_map(|m| self.social.circles.get(m)).map(|c| c.friends().count() as u32).sum::<u32>();
        let text = format!("{name} asks the city for {why}. {signatures} signatures.");
        let rally = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.land_use != LandUse::Residential && !b.is_power())
            // City hall's steps first, then a park, then anywhere public.
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(at) + match b.land_use { LandUse::Civic => 0.0, LandUse::Park => 400.0, _ => 900.0 })))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|x| x.0);
        let id = self.social.next_petition;
        self.social.next_petition += 1;
        self.social.petitions.push(Petition { id, group: gid, day, ask, text: text.clone(), at, signatures, state: PetitionState::Open, rally, settled: None });
        if self.social.petitions.len() > 40 {
            self.social.petitions.remove(0);
        }
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Petition { id, day });
        }
        if let Some(c) = self.citizens.get_mut(founder) {
            c.remember(tick, format!("We sent the city our petition: {why}."));
            let c = &self.citizens[founder];
            self.concerns.said(tick, founder, c, ask.concern(), 0.7, text.clone());
        }
        self.social.push_news(tick, format!("🏠 Petition: {text}"), at, Some(gid));
        true
    }

    /// Petitions answered, ignored into a protest, or given up.
    pub(super) fn check_petitions(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let open: Vec<Petition> = self.social.petitions.iter().filter(|p| matches!(p.state, PetitionState::Open | PetitionState::Protested)).cloned().collect();
        for p in open {
            let met = self.ask_met(&p);
            let gname = self.social.group(p.group).map(|g| g.name.clone()).unwrap_or_default();
            let members = self.social.group(p.group).map(|g| g.members.clone()).unwrap_or_default();
            let state = if met {
                PetitionState::Answered
            } else if p.state == PetitionState::Open && day >= p.day + 3 && p.rally.is_some() && self.social.group(p.group).is_some_and(|g| g.momentum >= 0.6) {
                PetitionState::Protested
            } else if day >= p.day + 8 {
                PetitionState::Lapsed
            } else {
                continue;
            };
            if let Some(x) = self.social.petitions.iter_mut().find(|x| x.id == p.id) {
                x.state = state;
                x.settled = Some(day);
            }
            match state {
                PetitionState::Answered => {
                    for m in &members {
                        if let Some(c) = self.citizens.get_mut(*m) {
                            c.mood = (c.mood + 0.1).min(1.0);
                            c.remember(tick, "The city listened to our petition.".to_string());
                        }
                    }
                    if let Some(g) = self.social.group_mut(p.group) {
                        g.reputation = (g.reputation + 0.15).min(1.0);
                    }
                    self.social.push_news(tick, format!("🏠 The city answered {gname}'s petition."), p.at, Some(p.group));
                }
                PetitionState::Protested => {
                    let rally = p.rally.and_then(|r| self.buildings.get(r)).map(|b| b.name.clone()).unwrap_or_default();
                    if let Some(g) = self.social.group_mut(p.group) {
                        g.works.push(Work::Protest { petition: p.id, day });
                    }
                    self.social.push_news(tick, format!("✊ {gname} will protest at {rally} today: nobody answered their petition."), p.at, Some(p.group));
                    for m in &members {
                        if let Some(c) = self.citizens.get_mut(*m) {
                            c.remember(tick, format!("Nobody answered. We march on {rally} tomorrow."));
                            self.concerns.heard(*m, c, p.ask.concern(), 0.5);
                        }
                    }
                }
                _ => {
                    for m in &members {
                        if let Some(c) = self.citizens.get_mut(*m) {
                            c.remember(tick, "Our petition went nowhere.".to_string());
                            c.mood = (c.mood - 0.04).max(0.0);
                        }
                    }
                }
            }
        }
        // A protest is held the morning after it is called.
        for p in self.social.petitions.iter_mut().filter(|p| p.state == PetitionState::Protested && p.settled == Some(day)) {
            p.settled = Some(day + 1);
        }
    }

    /// [land] Murals and gardens on the land value: a good mural livens a
    /// cheap street and grates on a dear one; gardens are liked.
    pub(super) fn splat_social(&self, b: &mut LandValueBuilder) {
        if !self.config.social {
            return;
        }
        for m in &self.social.murals {
            let dear = self.land_value.at(m.pos) > 0.6;
            let w = 0.02 * m.quality - if dear { 0.015 } else { 0.0 };
            b.splat(m.pos, 70.0, w);
        }
        for g in &self.social.gardens {
            b.splat(g.pos, 80.0, 0.025);
        }
    }

    /// Tonight's gig crowd (the venue's visitors while the band plays).
    fn count_gig_crowds(&mut self) {
        let tick = self.clock.tick;
        let on: Vec<(usize, BuildingId)> = self.social.gigs.iter().enumerate().filter(|(_, g)| g.on_at(tick)).map(|(i, g)| (i, g.venue)).collect();
        for (i, v) in on {
            let n = self.buildings.get(v).map_or(0, |b| b.visitors as u32);
            let g = &mut self.social.gigs[i];
            g.crowd = g.crowd.max(n);
        }
    }

    /// For tests: a tie between two people as `a` sees it.
    pub fn tie_between(&self, a: CitizenId, b: CitizenId) -> Option<&Tie> {
        self.social.circles.get(&a)?.tie(b)
    }
}

/// A few words of what was said.
fn talk_line(topic: Topic, outcome: Outcome, place: Option<&str>, about: Option<&str>, h: u64) -> String {
    let place = place.unwrap_or("that place");
    let about = about.unwrap_or("them");
    let good = outcome.is_good();
    let pickn = |xs: &[&str]| xs[(mix(h, 0x11E) % xs.len() as u64) as usize].to_string();
    match outcome {
        Outcome::Flirted => return pickn(&["Can I buy you a drink?", "You look nice tonight.", "Same time tomorrow?"]),
        Outcome::Favour => return pickn(&["I'll put a word in for you.", "Let me help with that.", "I know someone who's hiring."]),
        Outcome::Fight => return pickn(&["Say that again!", "Outside. Now.", "You want some?"]),
        Outcome::Insulted => return pickn(&["Get lost.", "Who asked you?", "You're a joke."]),
        _ => {}
    }
    match (topic, good) {
        (Topic::Work, true) => pickn(&["Work's been all right.", "Long shift, but fine.", "The boss finally said thanks."]),
        (Topic::Work, false) => pickn(&["You call that a job?", "Your lot never work.", "Easy for you to say."]),
        (Topic::Rent, true) => pickn(&["Rent's a joke round here.", "They put it up again.", "Can't afford this place."]),
        (Topic::Rent, false) => pickn(&["Move somewhere cheaper, then.", "Rents are fair.", "Stop moaning."]),
        (Topic::City(c), true) => format!("Somebody should fix {}.", Topic::City(c).label()),
        (Topic::City(c), false) => format!("{}? That's nonsense.", capital(Topic::City(c).label())),
        (Topic::News, true) => pickn(&["Did you hear what happened?", "Crazy night, wasn't it?", "Saw it on the news."]),
        (Topic::News, false) => pickn(&["You believe everything.", "That's not what happened.", "Fake news."]),
        (Topic::Sports, true) => pickn(&["What a game!", "We'll win the next one.", "Did you see that goal?"]),
        (Topic::Sports, false) => pickn(&["Your team is rubbish.", "The ref was bought.", "Worst game ever."]),
        (Topic::Place, true) => format!("{place} is the best."),
        (Topic::Place, false) => format!("{place}? Never again."),
        (Topic::Gossip, true) => format!("Heard about {about}?"),
        (Topic::Gossip, false) => format!("Leave {about} out of it."),
        (Topic::Music, true) => pickn(&["Heard the new stuff?", "We should start a band.", "That track's on repeat."]),
        (Topic::Music, false) => pickn(&["That's not music.", "You've got no taste."]),
        (Topic::Art, true) => pickn(&["Seen the new mural?", "That gallery's brilliant."]),
        (Topic::Art, false) => pickn(&["It's just graffiti.", "Art's a scam."]),
        (Topic::Books, true) => pickn(&["Finished that book yet?", "You have to read this."]),
        (Topic::Books, false) => pickn(&["Books are boring.", "Never read it, never will."]),
        (Topic::Weather, true) => pickn(&["Nice out, isn't it?", "Bit muggy today.", "Rain later, they say."]),
        (Topic::Weather, false) => pickn(&["Wrong, as usual.", "Who cares about the weather?"]),
        (Topic::Family, true) => pickn(&["How are the kids?", "Family's well, thanks."]),
        (Topic::Family, false) => pickn(&["Mind your own family.", "None of your business."]),
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// [murder] Does this fight between enemies (pair `salt`, at `tick`) end in
/// a death? `FEUD_KILL_CHANCE` of them, by a hash: no draws.
pub(crate) fn feud_turns_deadly(salt: u64, tick: u64) -> bool {
    hash01(salt, tick, 0x4D_0D3E) < crate::crime::FEUD_KILL_CHANCE
}
