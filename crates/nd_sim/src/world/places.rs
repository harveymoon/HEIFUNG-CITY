//! [places] Where people choose to go, how it went, and what the town
//! thinks of each place: the world side of `crate::places`.
//!
//! - `choose_place` / `choose_shop`: favourites weighed by experience and
//!   distance, sometimes somewhere new (curiosity; always when nothing is
//!   known), never a place sworn off, only places open and with room.
//! - `bring_company`: a household member or coworker sometimes comes along.
//! - `rate_visit`: on arrival the visit is rated (taste against character,
//!   events, luck, who else is there), folded into the citizen's
//!   `PlaceBook` and the place's `Reputation`, and notable ones remembered.
//! - `daily_places`: regulars and sworn-off customers counted per place.
//!
//! The randomness is drawn from streams seeded by the citizen and the
//! tick, so replays repeat and nothing else's rolls shift.

use super::*;
use super::trips::VENUE_COVER;
use crate::places::{self, PlaceCharacter, Reputation, Surroundings, Taste, Verdict, VisitEvent};

/// Softmax temperature of the choice: lower is pickier.
const CHOICE_TEMPERATURE: f32 = 0.2;
/// Metres that cost as much as one point of experience, per kind of trip.
const REACH_NIGHT: f32 = 700.0;
const REACH_PARK: f32 = 500.0;
const REACH_SHOP: f32 = 450.0;
/// Looking for somewhere new on foot: within this of where they are.
const EXPLORE_RADIUS: f32 = 450.0;
/// A habitual grocer is kept unless the best other one is this much
/// cheaper (cost ratio), plus a flat allowance.
const HABIT_SLACK: f32 = 1.3;
const HABIT_ALLOWANCE: f32 = 0.6;
/// Venue drinks cost this times `VENUE_TAB` by tier.
pub(super) fn tab_factor(tier: Tier) -> f32 {
    match tier {
        Tier::Low => 0.9,
        Tier::Mid => 1.0,
        Tier::High => 1.4,
    }
}

/// Who is inside each place now (visitors and people on park benches).
type Crowds = HashMap<BuildingId, Vec<CitizenId>>;

impl World {
    /// What a citizen likes in a place, today.
    pub fn taste_of(&self, cid: CitizenId) -> Option<Taste> {
        let c = self.citizens.get(cid)?;
        let hh = c.household.and_then(|h| self.households.get(h));
        let budget = c.savings.max(0.0) + hh.map_or(0.0, |h| h.money.max(0.0) / h.size().max(1) as f32);
        // Retirees and children have no wage: class by household means.
        let class = match hh {
            Some(h) if c.income <= 0.0 => {
                let n = h.members.iter().filter_map(|m| self.citizens.get(*m)).filter(|x| x.income > 0.0).count().max(1) as f32;
                let income: f32 = h.members.iter().filter_map(|m| self.citizens.get(*m)).map(|x| x.income).sum();
                Tier::for_income(if income > 0.0 { income / n } else { budget / 4.0 })
            }
            _ => c.class(),
        };
        Some(places::taste(c.seed, c.age, class, budget))
    }

    fn crowds(&self) -> Crowds {
        let mut out: Crowds = HashMap::new();
        for (id, c) in self.citizens.iter() {
            match c.activity {
                Activity::Visiting(b) | Activity::Sitting { park: b, .. } => out.entry(b).or_default().push(id),
                _ => {}
            }
        }
        out
    }

    /// This tick's crowds, from the cache or freshly counted; hand them
    /// back with [`keep_crowds`](Self::keep_crowds).
    fn take_crowds(&mut self) -> Crowds {
        let tick = self.clock.tick;
        match self.crowds_cache.take() {
            Some((t, c)) if t == tick => c,
            _ => self.crowds(),
        }
    }

    fn keep_crowds(&mut self, crowds: Crowds) {
        self.crowds_cache = Some((self.clock.tick, crowds));
    }

    fn surroundings_with(&self, b: BuildingId, inside: &[CitizenId]) -> Surroundings {
        let class: Vec<f32> = inside.iter().filter_map(|c| self.citizens.get(*c)).map(|c| c.class().index() as f32).collect();
        let at = self.building_lot(b).map_or(Vec2::ZERO, |l| l.center);
        Surroundings {
            visitors: self.buildings.get(b).map_or(0, |x| x.visitors.max(inside.len() as u16)),
            mean_class: if class.is_empty() { None } else { Some(class.iter().sum::<f32>() / class.len() as f32) },
            heat: self.society.crime.heat_at(at, crate::crime::FEAR_RADIUS),
            overflow: self.society.trash.bin(b).map_or(0.0, |x| x.overflow()),
        }
    }

    /// What a place is like right now; `None` for anything that is not a
    /// venue, a shop at the door or a park.
    pub fn place_character(&self, b: BuildingId) -> Option<PlaceCharacter> {
        let bld = self.buildings.get(b).filter(|x| places::is_place(x))?;
        let inside: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Visiting(x) | Activity::Sitting { park: x, .. } if x == b)).map(|(id, _)| id).collect();
        Some(places::character(bld, self.surroundings_with(b, &inside)))
    }

    pub fn reputation(&self, b: BuildingId) -> Option<&Reputation> {
        self.places.reputation.get(&b)
    }

    /// Acquaintances of `cid` who are regulars at `b`, and how many of
    /// them are there now.
    fn friends_at(&self, cid: CitizenId, b: BuildingId, inside: &[CitizenId]) -> (usize, usize) {
        let Some(c) = self.citizens.get(cid) else { return (0, 0) };
        let mut regulars = 0;
        let mut there = 0;
        for a in &c.places.acquaintances {
            if inside.contains(&a.who) {
                there += 1;
            }
            if self.citizens.get(a.who).is_some_and(|x| x.places.get(b).is_some_and(|p| p.regular)) {
                regulars += 1;
            }
        }
        (regulars, there)
    }

    /// Where to go for `lu` (a night out, a park, anywhere of that use).
    /// Favourites weighed by how it went and how far, sometimes somewhere
    /// new; `explore` looks only at unknown places near by first ("looking
    /// for a drink"). Open places with room only, never a sworn-off one.
    pub(super) fn choose_place(&mut self, id: CitizenId, lu: LandUse, explore: bool) -> Option<BuildingId> {
        crate::prof_scope!("choose_place");
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        let here = self.citizens.get(id)?.pos;
        let taste = self.taste_of(id)?;
        let crowds = self.take_crowds();
        let reach = match lu {
            LandUse::Park => REACH_PARK,
            LandUse::Entertainment => REACH_NIGHT,
            _ => REACH_SHOP,
        };
        // (building, utility, known, distance)
        let mut known: Vec<(BuildingId, f32, f32)> = Vec::new();
        let mut fresh: Vec<(BuildingId, f32, f32)> = Vec::new();
        let empty: Vec<CitizenId> = Vec::new();
        for (bid, b) in self.buildings.iter() {
            // [economy] A night out also finds the restaurants, driving
            // ranges, galleries and marina clubhouses on other land.
            let outing = lu == LandUse::Entertainment && b.business.as_ref().is_some_and(|z| z.kind.is_outing() && z.kind != BusinessKind::Venue);
            if (b.land_use != lu && !outing) || !b.serves_at(hour) {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            // [benches] A park with every seat taken is no place to go now.
            if lu == LandUse::Park && self.park_full(b.lot) {
                continue;
            }
            let d = lot.center.distance(here);
            let memory = self.citizens[id].places.get(bid);
            if memory.is_some_and(|m| m.never) {
                continue;
            }
            let inside = crowds.get(&bid).unwrap_or(&empty);
            let (regular_friends, friends_there) = self.friends_at(id, bid, inside);
            let mut social = 0.08 * regular_friends.min(3) as f32 + 0.15 * friends_there.min(2) as f32;
            // [social] Friends draw, enemies repel, a friend's band tonight.
            social += self.social_pull(id, bid, inside);
            // [economy] An opening tonight draws a crowd, and the guests most.
            if let Some(o) = self.scene.opening_at(bid, tick) {
                social += if o.invited.contains(&id) { 1.2 } else { 0.4 };
            }
            match memory {
                Some(m) => {
                    let u = 1.1 * m.score + if m.regular { 0.35 } else { 0.0 } + 0.1 + social - d / reach;
                    known.push((bid, u, d));
                }
                None => {
                    let impression = if places::is_place(b) { places::fit(&taste, &places::character(b, self.surroundings_with(bid, inside))) } else { 0.0 };
                    let pull = self.places.reputation.get(&bid).map_or(0.0, |r| r.pull());
                    fresh.push((bid, 0.8 * impression + pull + social - d / reach, d));
                }
            }
        }
        self.keep_crowds(crowds);
        if known.is_empty() && fresh.is_empty() {
            return None;
        }
        let c = &self.citizens[id];
        let mut rng = crate::rng_for(c.seed ^ tick.wrapping_mul(0x2545_F491), 0x91ACE);
        let has_regular = c.places.regular_of(lu).is_some();
        let novelty = if has_regular { taste.novelty * 0.4 } else { taste.novelty };
        let exploring = !fresh.is_empty() && (explore || known.is_empty() || rng.random::<f32>() < novelty);
        let pool: Vec<(BuildingId, f32, f32)> = if exploring {
            let near: Vec<_> = fresh.iter().copied().filter(|x| x.2 <= EXPLORE_RADIUS).collect();
            if explore && !near.is_empty() {
                near
            } else {
                fresh
            }
        } else if known.is_empty() {
            fresh
        } else {
            known
        };
        let pick = softmax_pick(&pool, &mut rng)?;
        if exploring {
            self.places.explorations_total += 1;
        }
        // Say where, when it is a habit or a find.
        if let Some(b) = self.buildings.get(pick) {
            let name = b.name.clone();
            let c = &mut self.citizens[id];
            let regular = c.places.get(pick).is_some_and(|m| m.regular);
            if lu == LandUse::Entertainment {
                if regular {
                    c.thought = format!("{name}. My usual.");
                } else if exploring {
                    c.thought = format!("Never been to {name}. Let's see.");
                } else if explore {
                    c.thought = format!("Nowhere new round here. {name} it is.");
                } else if c.thought.ends_with("My usual.") {
                    c.thought = format!("Not the usual tonight. {name}.");
                }
            }
        }
        Some(pick)
    }

    /// Where to buy `r`. Groceries: the habitual shop unless another is
    /// far cheaper or it has nothing; treats: like a place to go, with
    /// the price in it. Sworn-off shops only when nothing else is open.
    pub(super) fn choose_shop(&mut self, id: CitizenId, r: Resource, at: Vec2) -> Option<BuildingId> {
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        let taste = self.taste_of(id)?;
        let mut cands: Vec<(BuildingId, f32, f32, f32)> = Vec::new(); // id, cost, distance, price
        for (bid, b) in self.buildings.iter() {
            let Some(bz) = &b.business else { continue };
            if !b.operating() || bz.closed || !bz.sells(r) || bz.stock_of(r) < 1.0 || !b.serves_at(hour) {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let d = lot.center.distance(at);
            let price = bz.sell_price(r, b.jobs);
            cands.push((bid, price + d * 0.02, d, price));
        }
        let book = &self.citizens.get(id)?.places;
        let allowed: Vec<_> = cands.iter().copied().filter(|x| !book.is_never(x.0)).collect();
        let cands = if allowed.is_empty() { cands } else { allowed };
        let best = cands.iter().min_by(|a, b| a.1.total_cmp(&b.1)).copied()?;
        if r == Resource::Groceries {
            let habit = cands
                .iter()
                .filter_map(|x| book.get(x.0).filter(|m| m.visits >= 2 && m.score > -0.1).map(|m| (*x, m.score + if m.regular { 0.3 } else { 0.0 })))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(x, _)| x);
            return Some(match habit {
                Some(h) if h.1 <= best.1 * HABIT_SLACK + HABIT_ALLOWANCE => h.0,
                _ => best.0,
            });
        }
        let base = r.base_price();
        let thrift = 0.3 + 0.5 * taste.cheap.max(0.0);
        let mut known = Vec::new();
        let mut fresh = Vec::new();
        for &(bid, _, d, price) in &cands {
            let price_pen = (price / base - 1.0) * thrift;
            match book.get(bid) {
                Some(m) => known.push((bid, 1.0 * m.score + if m.regular { 0.3 } else { 0.0 } - price_pen - d / REACH_SHOP, d)),
                None => {
                    let impression = self.place_character_quick(bid).map_or(0.0, |ch| places::fit(&taste, &ch));
                    let pull = self.places.reputation.get(&bid).map_or(0.0, |r| r.pull());
                    fresh.push((bid, 0.6 * impression + pull - price_pen - d / REACH_SHOP, d));
                }
            }
        }
        let c = &self.citizens[id];
        let mut rng = crate::rng_for(c.seed ^ tick.wrapping_mul(0x2545_F491), 0x5409);
        let exploring = !fresh.is_empty() && (known.is_empty() || rng.random::<f32>() < taste.novelty);
        let pool = if exploring || known.is_empty() { fresh } else { known };
        softmax_pick(&pool, &mut rng)
    }

    /// The character from the building and the street alone (the crowd's
    /// class left out): cheap enough for every candidate shop.
    fn place_character_quick(&self, b: BuildingId) -> Option<PlaceCharacter> {
        let bld = self.buildings.get(b).filter(|x| places::is_place(x))?;
        Some(places::character(bld, self.surroundings_with(b, &[])))
    }

    /// Someone going out may bring a household member or a coworker who
    /// is free: home or idle, awake, off shift, not in a cell or a ward.
    /// Children come to the park only, with family.
    pub(super) fn bring_company(&mut self, id: CitizenId, target: BuildingId, purpose: Purpose) {
        let Purpose::Visit(lu) = purpose else { return };
        if !matches!(lu, LandUse::Entertainment | LandUse::Park) {
            return;
        }
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        let Some(c) = self.citizens.get(id) else { return };
        let sociability = places::traits(c.seed)[0];
        let mut rng = crate::rng_for(c.seed ^ tick.wrapping_mul(0x9E37), 0xC0_4A11);
        if rng.random::<f32>() >= 0.05 + 0.2 * sociability {
            return;
        }
        let name = c.name.clone();
        let mut pool: Vec<(CitizenId, bool)> = Vec::new();
        if let Some(h) = c.household.and_then(|h| self.households.get(h)) {
            pool.extend(h.members.iter().filter(|m| **m != id).map(|m| (*m, true)));
        }
        if let Some(w) = c.work.and_then(|w| self.buildings.get(w)) {
            pool.extend(w.workers.iter().filter(|m| **m != id).take(16).map(|m| (*m, false)));
        }
        // [social] Close friends and a partner too.
        pool.extend(self.social_company(id).into_iter().map(|m| (m, false)));
        let free: Vec<CitizenId> = pool
            .into_iter()
            .filter(|&(m, family)| {
                let Some(x) = self.citizens.get(m) else { return false };
                let grown = x.stage() != crate::citizens::LifeStage::Child;
                let on_shift = x.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.shift_for(x.seed).contains(hour));
                matches!(x.activity, Activity::AtHome | Activity::Idle)
                    && !x.asleep
                    && x.wakefulness > crate::citizens::TIRED + 0.1
                    && !on_shift
                    && (grown || (family && lu == LandUse::Park))
                    && !x.places.is_never(target)
                    && x.home.is_some()
            })
            .map(|(m, _)| m)
            .filter(|m| !self.services.in_custody(*m) && !self.is_patient(*m))
            .collect();
        let Some(&mate) = free.choose(&mut rng) else { return };
        let place = self.buildings.get(target).map(|b| b.name.clone()).unwrap_or_default();
        {
            let x = &mut self.citizens[mate];
            x.thought = format!("Going to {place} with {name}.");
            x.next_decision_tick = tick + 60;
        }
        // [passengers] Setting off together from home by car: in the
        // passenger seat. Otherwise they make their own way there.
        if self.join_ride(mate, id, target, purpose, crate::rides::RideKind::Outing, true) {
            self.places.outings_together_total += 1;
            self.places.outings_together_today += 1;
            return;
        }
        self.travel_to(mate, Some(target), purpose);
        if self.citizens.get(mate).is_some_and(|x| x.activity.is_traveling() || x.to_car.is_some()) {
            self.places.outings_together_total += 1;
            self.places.outings_together_today += 1;
        }
    }

    /// Rate a visit just begun at `b` (a venue, shop or park).
    pub(super) fn rate_visit(&mut self, id: CitizenId, b: BuildingId) {
        crate::prof_scope!("rate_visit");
        let tick = self.clock.tick;
        let Some(bld) = self.buildings.get(b) else { return };
        if !places::is_place(bld) {
            return;
        }
        let (name, lu, is_venue, bseed) = (bld.name.clone(), bld.land_use, bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue), bld.seed);
        let Some(taste) = self.taste_of(id) else { return };
        let crowds = self.take_crowds();
        // Still there (the cache is from the start of the tick).
        let here = |cid: CitizenId| self.citizens.get(cid).is_some_and(|c| matches!(c.activity, Activity::Visiting(x) | Activity::Sitting { park: x, .. } if x == b));
        let inside: Vec<CitizenId> = crowds.get(&b).map_or(Vec::new(), |v| v.iter().copied().filter(|&cid| cid != id && here(cid)).collect());
        self.keep_crowds(crowds);
        let ch = places::character(&self.buildings[b], self.surroundings_with(b, &inside));
        let c = &self.citizens[id];
        let family: Vec<CitizenId> = c.household.and_then(|h| self.households.get(h)).map_or(Vec::new(), |h| h.members.clone());
        let company = inside.iter().filter(|o| family.contains(o) || (c.work.is_some() && self.citizens.get(**o).is_some_and(|x| x.work == c.work))).count();
        let friends: Vec<CitizenId> = inside.iter().copied().filter(|o| c.places.knows(*o)).collect();
        let was_regular = c.places.get(b).is_some_and(|m| m.regular);
        let mut rng = crate::rng_for(c.seed ^ tick.rotate_left(17), bseed ^ 0x7A7E);
        let rating = places::rate(&taste, &ch, friends.len(), company > 0, &mut rng);
        let exp = rating.experience;
        // Regulars of this place who are here get to know each other.
        let repeat_here = |x: &Citizen| x.places.get(b).is_some_and(|m| m.visits >= 2);
        let mut met: Vec<CitizenId> = Vec::new();
        if c.places.get(b).is_some_and(|m| m.visits >= 1) {
            met = inside.iter().copied().filter(|o| self.citizens.get(*o).is_some_and(|x| repeat_here(x)) && !family.contains(o)).collect();
            met.truncate(2);
        }
        let friend_name = friends.first().and_then(|f| self.citizens.get(*f)).map(|x| x.name.clone());
        let c = &mut self.citizens[id];
        let verdict = c.places.record(b, lu, exp, tick);
        let regular_now = c.places.get(b).is_some_and(|m| m.regular);
        for &o in &met {
            c.places.meet(o, b, tick);
        }
        c.mood = (c.mood + 0.06 * exp).clamp(0.0, 1.0);
        let night = ch.vibe.is_night_out();
        let has = |e: VisitEvent| rating.events.contains(&e);
        let text = match verdict {
            Verdict::Found if night => Some(format!("Found {name}. Best night in months, will be back.")),
            Verdict::Found if lu == LandUse::Park => Some(format!("Found {name}. Lovely spot, I'll come again.")),
            Verdict::Found => Some(format!("Found {name}. My kind of shop.")),
            Verdict::BecameRegular if night => Some(format!("{name} is my place now. They know my order.")),
            Verdict::BecameRegular if lu == LandUse::Park => Some(format!("{name} is my park now. Same bench every time.")),
            Verdict::BecameRegular => Some(format!("{name} is my shop now.")),
            Verdict::NeverAgain => {
                let why = if has(VisitEvent::Fight) {
                    " A fight broke out."
                } else if has(VisitEvent::RudeStaff) {
                    " The staff were vile."
                } else if has(VisitEvent::Overpriced) {
                    " Daylight robbery."
                } else if has(VisitEvent::TooCrowded) {
                    " Couldn't move for people."
                } else {
                    " Not my kind of place."
                };
                Some(format!("Never going back to {name}.{why}"))
            }
            Verdict::Lapsed => Some(format!("{name} isn't what it used to be.")),
            Verdict::Ordinary if has(VisitEvent::Fight) => Some(format!("A fight broke out at {name}.")),
            Verdict::Ordinary if has(VisitEvent::GreatMusic) && exp > 0.6 => Some(format!("Great music at {name} tonight.")),
            Verdict::Ordinary if has(VisitEvent::MetFriend) && friend_name.is_some() => Some(format!("Ran into {} at {name}.", friend_name.clone().unwrap_or_default())),
            Verdict::Ordinary => None,
        };
        // [concerns] A bad night or a place gone downhill.
        let gripe = match verdict {
            Verdict::NeverAgain if has(VisitEvent::Fight) => Some(0.5),
            Verdict::NeverAgain => Some(0.3),
            Verdict::Lapsed => Some(0.2),
            Verdict::Ordinary if has(VisitEvent::Fight) => Some(0.4),
            _ => None,
        };
        if let Some(t) = text {
            c.thought = t.clone();
            c.remember(tick, t);
            if let Some(s) = gripe {
                self.concerns.heard(id, c, crate::concerns::Concern::Nightlife, s);
            }
        }
        for &o in &met {
            if let Some(x) = self.citizens.get_mut(o) {
                x.places.meet(id, b, tick);
            }
        }
        let rep = self.places.reputation.entry(b).or_default();
        rep.add(exp);
        rep.visits_today += 1;
        if was_regular || regular_now {
            rep.regular_visits_today += 1;
            if is_venue {
                rep.regular_revenue_today += VENUE_COVER;
            }
        }
        if exp >= places::GREAT {
            rep.great_nights += 1;
        }
        if has(VisitEvent::Fight) {
            rep.fights += 1;
        }
        match verdict {
            Verdict::NeverAgain => self.places.never_again_total += 1,
            Verdict::BecameRegular => self.places.regulars_made_total += 1,
            _ => {}
        }
    }

    /// Midnight: forget places that are gone, count each place's regulars
    /// and sworn-off customers, start the day's tallies.
    pub(super) fn daily_places(&mut self) {
        crate::prof_scope!("daily_places");
        let buildings = &self.buildings;
        let alive = |b: BuildingId| buildings.get(b).is_some_and(places::is_place);
        for c in self.citizens.values_mut() {
            c.places.retain_places(alive);
        }
        let citizens = &self.citizens;
        self.places.reputation.retain(|b, _| buildings.get(*b).is_some_and(places::is_place));
        for r in self.places.reputation.values_mut() {
            r.regulars = 0;
            r.never_again = 0;
            r.visits_yesterday = r.visits_today;
            r.regular_revenue_yesterday = r.regular_revenue_today;
            r.visits_today = 0;
            r.regular_visits_today = 0;
            r.regular_revenue_today = 0.0;
        }
        for c in citizens.values() {
            for p in &c.places.places {
                if let Some(r) = self.places.reputation.get_mut(&p.place) {
                    if p.regular {
                        r.regulars += 1;
                    }
                    if p.never {
                        r.never_again += 1;
                    }
                }
            }
        }
        self.places.outings_together_today = 0;
    }

    /// The citizen's favourite places, best first: (building, memory).
    pub fn favourite_places(&self, cid: CitizenId) -> Vec<(BuildingId, crate::places::PlaceMemory)> {
        let Some(c) = self.citizens.get(cid) else { return Vec::new() };
        let mut v: Vec<_> = c.places.places.iter().filter(|p| !p.never).map(|p| (p.place, p.clone())).collect();
        v.sort_by(|a, b| (b.1.regular, b.1.score).partial_cmp(&(a.1.regular, a.1.score)).unwrap_or(std::cmp::Ordering::Equal));
        v
    }
}

/// Pick one of `pool` (building, utility, distance) with probability
/// proportional to `exp(utility / CHOICE_TEMPERATURE)`.
fn softmax_pick(pool: &[(BuildingId, f32, f32)], rng: &mut impl Rng) -> Option<BuildingId> {
    let top = pool.iter().map(|x| x.1).fold(f32::NEG_INFINITY, f32::max);
    if !top.is_finite() {
        return pool.first().map(|x| x.0);
    }
    let w: Vec<f32> = pool.iter().map(|x| ((x.1 - top) / CHOICE_TEMPERATURE).exp()).collect();
    let total: f32 = w.iter().sum();
    let mut r = rng.random::<f32>() * total;
    for (x, wi) in pool.iter().zip(&w) {
        if r < *wi {
            return Some(x.0);
        }
        r -= wi;
    }
    pool.last().map(|x| x.0)
}
