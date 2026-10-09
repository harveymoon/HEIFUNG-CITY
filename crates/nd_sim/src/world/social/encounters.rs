//! [social] Who meets whom: encounters on the street and in places, and
//! whether they get talking.

use super::*;

impl World {
    /// The citizen's people (empty when they know nobody).
    pub fn circle_of(&self, cid: CitizenId) -> Option<&Circle> {
        self.social.circles.get(&cid)
    }

    /// Pace for a walker: the slower of the two when walking with a friend.
    pub(in crate::world) fn social_pace(&self, id: CitizenId, pace: f32) -> f32 {
        match self.social.walking_with.get(&id) {
            Some((f, _)) => match self.peds.walkers.get(f) {
                Some(w) if self.citizens.get(*f).is_some_and(|c| matches!(c.activity, Activity::Traveling { .. })) => pace.min(w.pace),
                _ => pace,
            },
            None => pace,
        }
    }

    pub(in crate::world) fn encounters(&mut self) {
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

    pub(super) fn spot_of(&self, id: CitizenId, code: u8) -> Spot {
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
    pub(super) fn try_talk(&mut self, a: CitizenId, b: CitizenId, code: u8) -> bool {
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
}
