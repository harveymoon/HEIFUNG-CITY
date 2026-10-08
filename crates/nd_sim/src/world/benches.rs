//! [benches] Park benches have seats. A bench (1.8 m, `bench_world` in
//! nd_procgen) seats `SEATS_PER_BENCH` side by side; a visitor walks to a
//! free seat and holds it from the moment they set off, so two people never
//! sit on top of each other. A park with every seat taken sends the next
//! visitor on to the nearest park that has room (or home, grumbling).
//! Strangers who sit side by side for a while may get talking and become
//! acquaintances (`PlaceBook::meet`): 10% plus 30% of their mean
//! sociability, so a shy pair rarely and a chatty pair often.
//!
//! The taken seats are kept in `World::seats` by seat position (to the
//! decimetre), which survives plan rebuilds that rename lots. A claim is
//! only trusted while its holder is still on that seat or on the way to
//! it, so a visit cut short by anything (a fire, a crash, a reload of an
//! old save) frees the seat without bookkeeping.

use super::*;
use crate::pedestrians::hash01;
use std::collections::BTreeMap;

/// Seats on one bench.
pub const SEATS_PER_BENCH: usize = 3;
/// Between seat centres along the bench, metres.
pub const SEAT_SPACING: f32 = 0.6;
/// Minutes side by side before strangers may get talking.
pub const CHAT_AFTER: u64 = 15;
/// How far a visitor turned away by a full park will go for another.
const OTHER_PARK_REACH: f32 = 600.0;
/// Someone turned away by a full park tries one other; turned away again
/// within this many ticks, they give up for now.
pub const TRY_AGAIN_AFTER: u64 = 60;

/// Key of a seat: its position to the decimetre.
pub type SeatKey = (i32, i32);

pub fn seat_key(p: Vec2) -> SeatKey {
    ((p.x * 10.0).round() as i32, (p.y * 10.0).round() as i32)
}

/// The seats of a bench at `pos` whose sitters face `facing`, left to right
/// along the bench (its long side runs across the facing).
pub fn seat_positions(pos: Vec2, facing: f32) -> [Vec2; SEATS_PER_BENCH] {
    let along = Vec2::from_angle(facing).perp();
    std::array::from_fn(|k| pos + along * (k as f32 - (SEATS_PER_BENCH - 1) as f32 * 0.5) * SEAT_SPACING)
}

/// Somebody on (or walking to) a seat.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Seat {
    pub who: CitizenId,
    /// Which bench: the key of its centre.
    pub bench: SeatKey,
    /// The way a sitter faces.
    pub facing: f32,
    /// Tick they sat down (or claimed it, until they arrive).
    pub since: u64,
}

/// Every claimed seat, and what came of the sitting.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Seats {
    pub taken: BTreeMap<SeatKey, Seat>,
    /// Bench neighbours who got talking, ever and today.
    pub met_total: u64,
    pub met_today: u32,
    /// Visitors who found a park full, ever and today.
    pub full_total: u64,
    pub full_today: u32,
    /// When each visitor last found a park full.
    pub turned_away: BTreeMap<CitizenId, u64>,
}

impl World {
    /// Is `seat` still held: its holder sitting there or walking to it?
    fn seat_held(&self, key: SeatKey, seat: &Seat) -> bool {
        let Some(c) = self.citizens.get(seat.who) else { return false };
        match &c.activity {
            Activity::Sitting { at, .. } => seat_key(*at) == key,
            Activity::Traveling { purpose: Purpose::Sit, path, .. } => path.last().is_some_and(|p| seat_key(*p) == key),
            _ => false,
        }
    }

    /// Is the seat at `key` free (never claimed, or its claim lapsed)?
    pub fn seat_free(&self, key: SeatKey) -> bool {
        self.seats.taken.get(&key).is_none_or(|s| !self.seat_held(key, s))
    }

    /// A free seat in `lot` (`pick` chooses among them): position, facing
    /// and the bench's key. `None` when the lot has no benches or all are full.
    pub fn free_seat(&self, lot: LotId, pick: usize) -> Option<(Vec2, f32, SeatKey)> {
        let mut free: Vec<(Vec2, f32, SeatKey)> = Vec::new();
        for &(node, _, facing) in self.walk.benches_in(lot) {
            let pos = self.walk.nodes[node as usize].pos;
            for s in seat_positions(pos, facing) {
                if self.seat_free(seat_key(s)) {
                    free.push((s, facing, seat_key(pos)));
                }
            }
        }
        (!free.is_empty()).then(|| free[pick % free.len()])
    }

    /// Seats in `lot`, and how many are held.
    pub fn seats_in(&self, lot: LotId) -> (usize, usize) {
        let mut total = 0;
        let mut held = 0;
        for &(node, _, facing) in self.walk.benches_in(lot) {
            for s in seat_positions(self.walk.nodes[node as usize].pos, facing) {
                total += 1;
                if !self.seat_free(seat_key(s)) {
                    held += 1;
                }
            }
        }
        (total, held)
    }

    pub(super) fn claim_seat(&mut self, id: CitizenId, at: Vec2, facing: f32, bench: SeatKey) {
        let since = self.clock.tick;
        self.seats.taken.insert(seat_key(at), Seat { who: id, bench, facing, since });
    }

    /// The facing of the seat `id` holds at `at`, starting the sitting now.
    pub(super) fn sit_down(&mut self, id: CitizenId, at: Vec2) -> Option<f32> {
        let tick = self.clock.tick;
        let s = self.seats.taken.get_mut(&seat_key(at)).filter(|s| s.who == id)?;
        s.since = tick;
        Some(s.facing)
    }

    /// The nearest other park with a free seat within reach of `id`.
    pub(super) fn park_with_room(&self, id: CitizenId, except: BuildingId) -> Option<BuildingId> {
        let here = self.citizens.get(id)?.pos;
        let hour = self.clock.hour() as u8;
        let mut best: Option<(f32, BuildingId)> = None;
        for (bid, b) in self.buildings.iter() {
            if bid == except || b.land_use != LandUse::Park || !b.serves_at(hour) {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let d = lot.center.distance(here);
            if d > OTHER_PARK_REACH || best.is_some_and(|(bd, _)| d >= bd) {
                continue;
            }
            if self.citizens[id].places.is_never(bid) {
                continue;
            }
            let (total, held) = self.seats_in(b.lot);
            if held < total {
                best = Some((d, bid));
            }
        }
        best.map(|(_, b)| b)
    }

    /// `id` gets up from the bench: neighbours who sat beside them long
    /// enough may have got talking. Frees the seat.
    pub(super) fn stand_up(&mut self, id: CitizenId, at: Vec2, park: BuildingId) {
        let key = seat_key(at);
        let Some(mine) = self.seats.taken.get(&key).copied().filter(|s| s.who == id) else { return };
        self.seats.taken.remove(&key);
        let tick = self.clock.tick;
        let neighbours: Vec<(CitizenId, u64)> = self
            .seats
            .taken
            .iter()
            .filter(|(k, s)| s.bench == mine.bench && s.who != id && matches!(self.citizens.get(s.who).map(|c| &c.activity), Some(Activity::Sitting { at, .. }) if seat_key(*at) == **k))
            .map(|(_, s)| (s.who, s.since))
            .collect();
        let park_name = self.buildings.get(park).map_or_else(|| "the park".to_string(), |b| b.name.clone());
        for (other, since) in neighbours {
            if tick.saturating_sub(mine.since.max(since)) < CHAT_AFTER {
                continue;
            }
            let (a, b) = (&self.citizens[id], &self.citizens[other]);
            if a.household.is_some() && a.household == b.household {
                continue;
            }
            let known = a.places.knows(other);
            let social = (crate::places::traits(a.seed)[0] + crate::places::traits(b.seed)[0]) * 0.5;
            let chance = if known { 1.0 } else { 0.1 + 0.3 * social };
            let (lo, hi) = if a.seed < b.seed { (a.seed, b.seed) } else { (b.seed, a.seed) };
            if hash01(lo ^ hi.rotate_left(29), tick, 0xbe7c) >= chance {
                continue;
            }
            let (an, bn) = (a.name.clone(), b.name.clone());
            for (who, with, name) in [(id, other, bn), (other, id, an)] {
                let c = &mut self.citizens[who];
                c.places.meet(with, park, tick);
                if !known {
                    c.remember(tick, format!("Got talking to {name} on a bench in {park_name}."));
                    c.mood = (c.mood + 0.03).min(1.0);
                }
            }
            if !known {
                self.seats.met_today += 1;
                self.seats.met_total += 1;
            }
        }
    }

    /// Has the park on `lot` benches, and every seat taken?
    pub fn park_full(&self, lot: LotId) -> bool {
        let (total, held) = self.seats_in(lot);
        total > 0 && held >= total
    }

    /// Daily: forget lapsed claims and reset the day's counts.
    pub(super) fn daily_benches(&mut self) {
        let keep: Vec<SeatKey> = self.seats.taken.iter().filter(|(k, s)| self.seat_held(**k, s)).map(|(k, _)| *k).collect();
        self.seats.taken.retain(|k, _| keep.binary_search(k).is_ok());
        self.seats.met_today = 0;
        self.seats.full_today = 0;
        let tick = self.clock.tick;
        self.seats.turned_away.retain(|_, t| tick < *t + TRY_AGAIN_AFTER);
    }

    /// After a load: anyone sitting in a save from before seats holds the
    /// seat they are on.
    pub(super) fn seats_after_load(&mut self) {
        let tick = self.clock.tick;
        let sitting: Vec<(CitizenId, Vec2, f32)> = self.citizens.iter().filter_map(|(id, c)| if let Activity::Sitting { at, .. } = c.activity { Some((id, at, c.heading)) } else { None }).collect();
        for (id, at, heading) in sitting {
            let key = seat_key(at);
            if self.seats.taken.get(&key).is_none_or(|s| s.who != id) && self.seat_free(key) {
                self.seats.taken.insert(key, Seat { who: id, bench: key, facing: heading, since: tick });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    fn run(w: &mut World, ticks: usize) {
        let mut minds = IdleMind;
        for _ in 0..ticks {
            w.tick(&mut minds);
        }
    }

    /// A street with a small park, a second park further on, and homes;
    /// everyone indoors and deciding nothing. Returns the world and the
    /// two parks (near first).
    fn town() -> (World, BuildingId, BuildingId) {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 34.0), LandUse::Park);
        w.add_zone(square(150.0, 10.0, 60.0), LandUse::Park);
        w.add_zone(square(-300.0, 10.0, 290.0), LandUse::Residential);
        w.add_zone(square(-300.0, -300.0, 290.0), LandUse::Residential);
        let parks: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Park).map(|(id, _)| id).collect();
        assert!(parks.len() >= 2, "two parks: {}", parks.len());
        for (id, l) in w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, l)| (id, l.clone())).collect::<Vec<_>>() {
            let _ = l;
            w.build_on(id);
        }
        for &p in &parks {
            w.build_on(p);
        }
        let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
        }
        let mut guard = 0;
        while w.citizens.len() < 40 && guard < 48 {
            run(&mut w, 60);
            guard += 1;
        }
        assert!(w.citizens.len() >= 40, "people moved in: {}", w.citizens.len());
        for c in w.citizens.values_mut() {
            if let Some(h) = c.home {
                c.activity = Activity::AtHome;
                c.inside = Some(h);
            }
            c.next_decision_tick = u64::MAX;
            c.owns_bike = false;
            c.owns_car = false;
        }
        let mut by_x: Vec<(f32, BuildingId)> = parks.iter().map(|&l| (w.lots[l].center.x, w.lots[l].building.unwrap())).collect();
        by_x.sort_by(|a, b| a.0.total_cmp(&b.0));
        (w, by_x[0].1, by_x[1].1)
    }

    #[test]
    fn seats_sit_side_by_side_along_the_bench() {
        let s = seat_positions(Vec2::new(10.0, 5.0), 0.0);
        assert_eq!(s.len(), SEATS_PER_BENCH);
        // Facing +x: the bench runs along y.
        assert!((s[1] - Vec2::new(10.0, 5.0)).length() < 1e-5, "the middle seat is the bench's centre");
        assert!((s[0].distance(s[1]) - SEAT_SPACING).abs() < 1e-5 && (s[0].x - 10.0).abs() < 1e-5);
        assert_ne!(seat_key(s[0]), seat_key(s[2]));
    }

    #[test]
    fn nobody_shares_a_seat_and_a_full_park_sends_people_to_another() {
        let (mut w, small, big) = town();
        let lot = w.buildings[small].lot;
        let (seats, _) = w.seats_in(lot);
        assert!(seats >= SEATS_PER_BENCH, "the small park has a bench: {seats} seats");
        let visitors: Vec<CitizenId> = w.citizens.keys().take(seats + 4).collect();
        assert_eq!(visitors.len(), seats + 4, "enough people");
        for &id in &visitors {
            w.citizens[id].activity = Activity::Idle;
            w.citizens[id].inside = None;
            w.travel_to(id, Some(small), Purpose::Visit(LandUse::Park));
        }
        run(&mut w, 200);
        let sitting: Vec<(CitizenId, Vec2, BuildingId)> = visitors.iter().filter_map(|&id| if let Activity::Sitting { at, park } = w.citizens[id].activity { Some((id, at, park)) } else { None }).collect();
        let here = sitting.iter().filter(|s| s.2 == small).count();
        assert_eq!(here, seats, "every seat in the small park is taken, and no more: {here} of {seats}");
        for (i, a) in sitting.iter().enumerate() {
            for b in &sitting[i + 1..] {
                assert!(a.1.distance(b.1) >= SEAT_SPACING - 1e-3, "two people on one seat at {:?} and {:?}", a.1, b.1);
            }
        }
        let elsewhere = sitting.iter().filter(|s| s.2 == big).count();
        assert!(elsewhere >= 1, "the overflow went on to the other park: {elsewhere}");
        assert!(w.seats.full_total >= 4, "four found the small park full: {}", w.seats.full_total);
        let (_, held) = w.seats_in(lot);
        assert_eq!(held, seats);
        // One gets up: the seat is free again.
        let (first, _, _) = sitting.iter().find(|s| s.2 == small).copied().unwrap();
        let exit = w.leave_building(first);
        assert!(exit.is_none());
        assert_eq!(w.seats_in(lot).1, seats - 1);
    }

    #[test]
    fn bench_neighbours_may_get_talking() {
        let (mut w, small, _) = town();
        let lot = w.buildings[small].lot;
        let &(node, _, facing) = w.walk.benches.iter().find(|b| b.1 == lot).unwrap();
        let bench = w.walk.nodes[node as usize].pos;
        let seats = seat_positions(bench, facing);
        // Strangers from different households.
        let ids: Vec<CitizenId> = w.citizens.keys().collect();
        let a = ids[0];
        let b = *ids.iter().find(|&&x| w.citizens[x].household != w.citizens[a].household).unwrap();
        let mut met = 0;
        for trial in 0..80 {
            for (who, k) in [(a, 0), (b, 1)] {
                w.citizens[who].activity = Activity::Sitting { at: seats[k], park: small };
                w.claim_seat(who, seats[k], facing, seat_key(bench));
            }
            // Side by side long enough...
            w.clock.tick += CHAT_AFTER + 1 + trial;
            let known = w.citizens[a].places.knows(b);
            w.stand_up(a, seats[0], small);
            w.citizens[a].activity = Activity::Idle;
            assert!(w.seat_free(seat_key(seats[0])), "getting up frees the seat");
            if !known && w.citizens[a].places.knows(b) {
                met += 1;
                assert!(w.citizens[b].places.knows(a), "both remember");
                assert!(w.citizens[a].memories.iter().any(|m| m.text.contains("on a bench")));
                // Acquaintances meet again every time.
                let times = w.citizens[a].places.acquaintances.iter().find(|x| x.who == b).unwrap().times;
                w.citizens[a].activity = Activity::Sitting { at: seats[0], park: small };
                w.claim_seat(a, seats[0], facing, seat_key(bench));
                w.clock.tick += CHAT_AFTER + 1;
                w.stand_up(a, seats[0], small);
                w.citizens[a].activity = Activity::Idle;
                assert_eq!(w.citizens[a].places.acquaintances.iter().find(|x| x.who == b).unwrap().times, times + 1);
                break;
            }
        }
        assert_eq!(met, 1, "strangers side by side got talking within 80 sittings");
        // Too short a sit: nothing.
        let c = *ids.iter().find(|&&x| x != a && x != b && w.citizens[x].household != w.citizens[a].household && w.citizens[x].household != w.citizens[b].household).unwrap();
        for (who, k) in [(c, 0), (b, 2)] {
            w.citizens[who].activity = Activity::Sitting { at: seats[k], park: small };
            w.claim_seat(who, seats[k], facing, seat_key(bench));
        }
        w.clock.tick += 2;
        w.stand_up(c, seats[0], small);
        assert!(!w.citizens[c].places.knows(b), "two minutes on a bench makes no friends");
    }
}
