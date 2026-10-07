//! [concerns] What the people of the city complain about, and how happy
//! they are, over time.
//!
//! Complaints are tagged where they happen: next to the memory a negative
//! event leaves (`Concerns::heard`, which quotes the memory just written),
//! or with `World::complain` for daily grievances that leave no memory
//! (rent stress, hunger, no job). Each is filed under a [`Concern`] with a
//! severity; a citizen counts once per concern per day, so a day's count
//! reads as "people who complained about this today". Counts are kept per
//! day in a ring of `CONCERN_DAYS`, with a few recent quotes per concern
//! (who said it and where) and the places complaints came from lately, for
//! the "where" of each concern.
//!
//! City happiness (mean mood, the unhappy and happy shares, mood by class)
//! is sampled every midnight into a long ring and every hour into a short
//! one. Everything here is saved with the world, so history survives a
//! load.

use std::collections::{HashSet, VecDeque};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::buildings::{BuildingId, Tier};
use crate::citizens::{Citizen, CitizenId};
use crate::clock::MINUTES_PER_DAY;
use crate::world::World;

/// Days of complaint counts kept.
pub const CONCERN_DAYS: usize = 120;
/// Days of daily happiness kept (the chart's "all").
pub const MOOD_DAYS: usize = 730;
/// Hours of hourly happiness kept.
pub const MOOD_HOURS: usize = 48;
/// Recent quotes kept per concern.
pub const EXAMPLES: usize = 8;
/// Recent complaint places kept per concern, for its hot spot.
pub const SPOTS: usize = 240;
/// Hot spot grid, metres.
pub const SPOT_CELL: f32 = 160.0;
/// Mood under this is unhappy, over `HAPPY` happy.
pub const UNHAPPY: f32 = 0.35;
pub const HAPPY: f32 = 0.7;
/// Minutes from the call to help on the scene beyond which the people
/// it came for complain about the service.
pub const SLOW_RESPONSE_TICKS: u64 = 40;
/// A repeat complaint this grave is quoted although it is not counted.
pub const QUOTE_AGAIN: f32 = 0.6;
/// A week, for "this week" against "last week".
pub const WEEK: u64 = 7;

/// What a complaint is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Concern {
    Housing,
    Food,
    Jobs,
    Safety,
    Trash,
    Noise,
    Commute,
    Parking,
    Power,
    Services,
    Nightlife,
    Displacement,
    Other,
    /// [clarity] Crashes on the street outside (`World::daily_dangerous_streets`).
    DangerousStreets,
    /// [clarity] No hospital in reach (`World::daily_healthcare`).
    Healthcare,
}

impl Concern {
    pub const COUNT: usize = 15;
    pub const ALL: [Concern; Concern::COUNT] = [
        Concern::Housing,
        Concern::Food,
        Concern::Jobs,
        Concern::Safety,
        Concern::Trash,
        Concern::Noise,
        Concern::Commute,
        Concern::Parking,
        Concern::Power,
        Concern::Services,
        Concern::Nightlife,
        Concern::Displacement,
        Concern::DangerousStreets,
        Concern::Healthcare,
        Concern::Other,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Concern::Housing => "Rent & housing",
            Concern::Food => "Food",
            Concern::Jobs => "Jobs & money",
            Concern::Safety => "Safety & crime",
            Concern::Trash => "Trash & dirt",
            Concern::Noise => "Noise & pollution",
            Concern::Commute => "Commute & traffic",
            Concern::Parking => "Parking",
            Concern::Power => "Power",
            Concern::Services => "Services",
            Concern::Nightlife => "Nightlife & places",
            Concern::Displacement => "Displacement",
            Concern::Other => "Other",
            Concern::DangerousStreets => "Dangerous streets",
            Concern::Healthcare => "Healthcare",
        }
    }

    /// Stable key for the remote API.
    pub fn key(self) -> &'static str {
        match self {
            Concern::Housing => "housing",
            Concern::Food => "food",
            Concern::Jobs => "jobs",
            Concern::Safety => "safety",
            Concern::Trash => "trash",
            Concern::Noise => "noise",
            Concern::Commute => "commute",
            Concern::Parking => "parking",
            Concern::Power => "power",
            Concern::Services => "services",
            Concern::Nightlife => "nightlife",
            Concern::Displacement => "displacement",
            Concern::Other => "other",
            Concern::DangerousStreets => "dangerous_streets",
            Concern::Healthcare => "healthcare",
        }
    }

    pub fn from_key(k: &str) -> Option<Concern> {
        Concern::ALL.into_iter().find(|c| c.key() == k)
    }

    /// One line on what feeds it, for tooltips.
    pub fn about(self) -> &'static str {
        match self {
            Concern::Housing => "rent eating over half the income, moving out over rent, leaving town over it",
            Concern::Food => "going hungry, empty shelves, leaving town hungry",
            Concern::Jobs => "no job, laid off, too skint to buy or get in",
            Concern::Safety => "thefts, burglaries, assaults, murders, vandalism, fires, fleeing crime",
            Concern::Trash => "overflowing bins, rats",
            Concern::Noise => "drones overhead, street takeovers at night",
            Concern::Commute => "long trips, jams, transit that never came, crashes, no way through",
            Concern::Parking => "circling for a space, long searches",
            Concern::Power => "sitting in the dark in a brownout",
            Concern::Services => "slow police, fire and ambulance, nobody coming, no hospital bed",
            Concern::Nightlife => "places shut or packed, bad nights out, fights",
            Concern::Displacement => "homes demolished or expropriated, evictions",
            Concern::Other => "everything else",
            Concern::DangerousStreets => "crashes piling up on the street where they live",
            Concern::Healthcare => "the nearest hospital far away, or none at all",
        }
    }
}

/// One complaint, quoted: who, where, what.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Complaint {
    pub tick: u64,
    pub citizen: CitizenId,
    pub name: String,
    /// The building it is about, or where they were.
    pub building: Option<BuildingId>,
    pub pos: Vec2,
    pub concern: Concern,
    pub severity: f32,
    pub text: String,
}

/// One day's complaints: people per concern and their summed severity.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConcernDay {
    pub day: u64,
    pub counts: Vec<u32>,
    pub weight: Vec<f32>,
}

impl ConcernDay {
    fn new(day: u64) -> Self {
        Self { day, counts: vec![0; Concern::COUNT], weight: vec![0.0; Concern::COUNT] }
    }

    pub fn count(&self, c: Concern) -> u32 {
        self.counts.get(c.index()).copied().unwrap_or(0)
    }

    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
}

/// City happiness at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoodSample {
    pub tick: u64,
    pub people: u32,
    pub mean: f32,
    /// Shares of people under `UNHAPPY` and over `HAPPY`.
    pub unhappy: f32,
    pub happy: f32,
    /// Mean mood by class (`Tier` index); 0 where nobody is in the class.
    pub by_class: [f32; 3],
    pub class_people: [u32; 3],
}

impl MoodSample {
    pub fn of<'a>(tick: u64, citizens: impl Iterator<Item = &'a Citizen>) -> Self {
        let mut s = MoodSample { tick, ..Default::default() };
        let mut sum = 0.0f64;
        let mut class_sum = [0.0f64; 3];
        let (mut unhappy, mut happy) = (0u32, 0u32);
        for c in citizens {
            s.people += 1;
            sum += c.mood as f64;
            let k = c.class().index();
            class_sum[k] += c.mood as f64;
            s.class_people[k] += 1;
            unhappy += (c.mood < UNHAPPY) as u32;
            happy += (c.mood > HAPPY) as u32;
        }
        if s.people > 0 {
            let n = s.people as f64;
            s.mean = (sum / n) as f32;
            s.unhappy = (unhappy as f64 / n) as f32;
            s.happy = (happy as f64 / n) as f32;
        }
        for k in 0..3 {
            if s.class_people[k] > 0 {
                s.by_class[k] = (class_sum[k] / s.class_people[k] as f64) as f32;
            }
        }
        s
    }

    /// Mean mood of a class, if anyone is in it.
    pub fn class(&self, t: Tier) -> Option<f32> {
        (self.class_people[t.index()] > 0).then_some(self.by_class[t.index()])
    }

    pub fn day(&self) -> u64 {
        self.tick / MINUTES_PER_DAY
    }
}

/// A recent complaint's place, for the hot spot.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Spot {
    pub pos: Vec2,
    pub day: u64,
}

/// A concern ranked: this week's people, last week's, all time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ranked {
    pub concern: Concern,
    pub this_week: u32,
    pub last_week: u32,
    pub today: u32,
    pub total: u64,
    pub severity: f32,
}

impl Ranked {
    /// Week on week change as a share of last week (`+1.0` is double); a
    /// concern new this week counts as `+1.0`.
    pub fn trend(&self) -> f32 {
        if self.last_week == 0 {
            if self.this_week > 0 {
                1.0
            } else {
                0.0
            }
        } else {
            self.this_week as f32 / self.last_week as f32 - 1.0
        }
    }

    /// -1 falling, 0 steady (within 15%), +1 rising.
    pub fn direction(&self) -> i8 {
        let t = self.trend();
        if t > 0.15 {
            1
        } else if t < -0.15 {
            -1
        } else {
            0
        }
    }
}

/// Everything the city has complained about lately, and how happy it is.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Concerns {
    /// One entry per day with complaints, oldest first, at most `CONCERN_DAYS`.
    pub days: VecDeque<ConcernDay>,
    /// Recent quotes per concern (`Concern::index`), newest last.
    pub examples: Vec<VecDeque<Complaint>>,
    /// Recent complaint places per concern, newest last.
    pub spots: Vec<VecDeque<Spot>>,
    /// All-time people-days per concern.
    pub totals: Vec<u64>,
    /// Happiness at each midnight, oldest first.
    pub mood_days: VecDeque<MoodSample>,
    /// Happiness each hour, the last `MOOD_HOURS`.
    pub mood_hours: VecDeque<MoodSample>,
    /// Who has complained about what today (one count each a day). Not
    /// saved: after a load, someone may count twice that day.
    #[serde(skip)]
    seen: HashSet<(CitizenId, u8)>,
    #[serde(skip)]
    seen_day: u64,
}

impl Concerns {
    fn today_mut(&mut self, day: u64) -> &mut ConcernDay {
        if self.days.back().is_none_or(|d| d.day < day) {
            self.days.push_back(ConcernDay::new(day));
            while self.days.len() > CONCERN_DAYS {
                self.days.pop_front();
            }
        }
        let d = self.days.back_mut().unwrap();
        if d.counts.len() < Concern::COUNT {
            d.counts.resize(Concern::COUNT, 0);
            d.weight.resize(Concern::COUNT, 0.0);
        }
        d
    }

    /// File a complaint. Returns false when this citizen already
    /// complained about this today: nothing is counted again, though a
    /// grave one (`QUOTE_AGAIN`) is still quoted.
    pub fn note(&mut self, c: Complaint) -> bool {
        let day = c.tick / MINUTES_PER_DAY;
        if day != self.seen_day {
            self.seen.clear();
            self.seen_day = day;
        }
        if self.examples.len() < Concern::COUNT {
            self.examples.resize_with(Concern::COUNT, VecDeque::new);
        }
        if self.spots.len() < Concern::COUNT {
            self.spots.resize_with(Concern::COUNT, VecDeque::new);
        }
        if self.totals.len() < Concern::COUNT {
            self.totals.resize(Concern::COUNT, 0);
        }
        let k = c.concern.index();
        let first = self.seen.insert((c.citizen, c.concern as u8));
        if first {
            let today = self.today_mut(day);
            today.counts[k] += 1;
            today.weight[k] += c.severity;
            self.totals[k] += 1;
            let spots = &mut self.spots[k];
            spots.push_back(Spot { pos: c.pos, day });
            while spots.len() > SPOTS {
                spots.pop_front();
            }
        } else if c.severity < QUOTE_AGAIN {
            return false;
        }
        let ex = &mut self.examples[k];
        ex.push_back(c);
        while ex.len() > EXAMPLES {
            ex.pop_front();
        }
        first
    }

    /// File a complaint in words other than a memory (or with none).
    pub fn said(&mut self, tick: u64, cid: CitizenId, c: &Citizen, concern: Concern, severity: f32, text: impl Into<String>) -> bool {
        self.note(complaint(tick, cid, c, concern, severity, text.into(), None))
    }

    /// File the memory a citizen has just been given under `concern`: the
    /// quote is their own words, the place where they are (or `at`).
    pub fn heard(&mut self, cid: CitizenId, c: &Citizen, concern: Concern, severity: f32) -> bool {
        self.heard_at(cid, c, concern, severity, None)
    }

    /// As `heard`, about a building at a known place.
    pub fn heard_at(&mut self, cid: CitizenId, c: &Citizen, concern: Concern, severity: f32, at: Option<(BuildingId, Vec2)>) -> bool {
        let Some(m) = c.memories.last() else { return false };
        let (tick, text) = (m.tick, m.text.clone());
        self.note(complaint(tick, cid, c, concern, severity, text, at))
    }

    /// People who complained about `concern` on days `from..=to`.
    pub fn count(&self, concern: Concern, from: u64, to: u64) -> u32 {
        self.days.iter().filter(|d| d.day >= from && d.day <= to).map(|d| d.count(concern)).sum()
    }

    fn weight(&self, concern: Concern, from: u64, to: u64) -> f32 {
        self.days.iter().filter(|d| d.day >= from && d.day <= to).map(|d| d.weight.get(concern.index()).copied().unwrap_or(0.0)).sum()
    }

    /// Every concern with complaints in the last two weeks, most
    /// complained about this week first (then last week, then severity).
    pub fn ranked(&self, today: u64) -> Vec<Ranked> {
        let week_from = today.saturating_sub(WEEK - 1);
        let mut out: Vec<Ranked> = Concern::ALL
            .into_iter()
            .map(|c| {
                let this_week = self.count(c, week_from, today);
                let last_week = if week_from == 0 { 0 } else { self.count(c, week_from.saturating_sub(WEEK), week_from - 1) };
                let w = self.weight(c, week_from, today);
                Ranked {
                    concern: c,
                    this_week,
                    last_week,
                    today: self.count(c, today, today),
                    total: self.totals.get(c.index()).copied().unwrap_or(0),
                    severity: if this_week > 0 { w / this_week as f32 } else { 0.0 },
                }
            })
            .filter(|r| r.this_week > 0 || r.last_week > 0)
            .collect();
        out.sort_by(|a, b| b.this_week.cmp(&a.this_week).then(b.last_week.cmp(&a.last_week)).then(b.severity.total_cmp(&a.severity)).then(a.concern.cmp(&b.concern)));
        out
    }

    /// Recent quotes about `concern`, newest first.
    pub fn quotes(&self, concern: Concern) -> impl Iterator<Item = &Complaint> {
        self.examples.get(concern.index()).into_iter().flat_map(|q| q.iter().rev())
    }

    /// Where `concern` came from most over the last `days`: the middle of
    /// the busiest `SPOT_CELL` square and how many complaints fell in it.
    pub fn hotspot(&self, concern: Concern, today: u64, days: u64) -> Option<(Vec2, u32)> {
        let spots = self.spots.get(concern.index())?;
        let from = today.saturating_sub(days.saturating_sub(1));
        let mut cells: std::collections::HashMap<(i32, i32), (Vec2, u32)> = std::collections::HashMap::new();
        for s in spots.iter().filter(|s| s.day >= from) {
            let key = ((s.pos.x / SPOT_CELL).floor() as i32, (s.pos.y / SPOT_CELL).floor() as i32);
            let e = cells.entry(key).or_insert((Vec2::ZERO, 0));
            e.0 += s.pos;
            e.1 += 1;
        }
        cells.into_iter().max_by(|a, b| a.1 .1.cmp(&b.1 .1).then(b.0.cmp(&a.0))).map(|(_, (sum, n))| (sum / n as f32, n))
    }

    /// Record happiness: a midnight sample (`daily`) or an hourly one.
    pub fn record_mood(&mut self, s: MoodSample, daily: bool) {
        let (ring, cap) = if daily { (&mut self.mood_days, MOOD_DAYS) } else { (&mut self.mood_hours, MOOD_HOURS) };
        if ring.back().is_some_and(|b| b.tick == s.tick) {
            return;
        }
        ring.push_back(s);
        while ring.len() > cap {
            ring.pop_front();
        }
    }
}

fn complaint(tick: u64, cid: CitizenId, c: &Citizen, concern: Concern, severity: f32, text: String, at: Option<(BuildingId, Vec2)>) -> Complaint {
    let (building, pos) = match at {
        Some((b, p)) => (Some(b), p),
        None => (c.inside.or(c.home), c.pos),
    };
    Complaint { tick, citizen: cid, name: c.name.clone(), building, pos, concern, severity: severity.clamp(0.0, 1.0), text }
}

impl World {
    /// A citizen complains about `concern` (a grievance that leaves no
    /// memory, or one worded differently from it).
    pub fn complain(&mut self, cid: CitizenId, concern: Concern, severity: f32, text: impl Into<String>) -> bool {
        let Some(c) = self.citizens.get(cid) else { return false };
        let k = complaint(self.clock.tick, cid, c, concern, severity, text.into(), None);
        self.concerns.note(k)
    }

    /// As `complain`, about a building (placed at its lot).
    pub fn complain_about(&mut self, cid: CitizenId, concern: Concern, severity: f32, text: impl Into<String>, b: BuildingId) -> bool {
        let at = self.building_lot(b).map(|l| (b, l.center));
        let Some(c) = self.citizens.get(cid) else { return false };
        let k = complaint(self.clock.tick, cid, c, concern, severity, text.into(), at);
        self.concerns.note(k)
    }

    /// The building nearest a point (by lot centre), to name a hot spot.
    pub fn building_nearest(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings.iter().filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance_squared(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// Happiness right now.
    pub fn mood_now(&self) -> MoodSample {
        MoodSample::of(self.clock.tick, self.citizens.values())
    }

    /// Every tick, cheap: sample happiness at midnight and on the hour.
    pub(crate) fn step_concerns(&mut self) {
        crate::prof_scope!("step_concerns");
        let tick = self.clock.tick;
        if tick % 60 != 0 {
            return;
        }
        let s = self.mood_now();
        self.concerns.record_mood(s, false);
        if self.clock.minute_of_day() == 0 {
            self.concerns.record_mood(s, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(tick: u64, cid: CitizenId, concern: Concern) -> Complaint {
        Complaint { tick, citizen: cid, name: "T".into(), building: None, pos: Vec2::new(10.0, 10.0), concern, severity: 0.5, text: format!("{concern:?}") }
    }

    fn ids(n: usize) -> Vec<CitizenId> {
        let mut m: slotmap::SlotMap<CitizenId, ()> = slotmap::SlotMap::with_key();
        (0..n).map(|_| m.insert(())).collect()
    }

    #[test]
    fn complaints_are_counted_once_per_person_and_concern_a_day() {
        let p = ids(3);
        let mut k = Concerns::default();
        assert!(k.note(quote(10, p[0], Concern::Food)));
        assert!(!k.note(quote(20, p[0], Concern::Food)), "the same grumble twice in a day counts once");
        assert!(k.note(quote(30, p[0], Concern::Trash)));
        assert!(k.note(quote(40, p[1], Concern::Food)));
        assert_eq!(k.count(Concern::Food, 0, 0), 2);
        assert_eq!(k.count(Concern::Trash, 0, 0), 1);
        // A new day: the same person counts again.
        assert!(k.note(quote(MINUTES_PER_DAY + 5, p[0], Concern::Food)));
        assert_eq!(k.count(Concern::Food, 1, 1), 1);
        assert_eq!(k.totals[Concern::Food.index()], 3);
        assert_eq!(k.quotes(Concern::Food).count(), 3);
        assert_eq!(k.quotes(Concern::Food).next().unwrap().tick, MINUTES_PER_DAY + 5, "newest first");
    }

    #[test]
    fn the_day_ring_rolls_and_quotes_are_capped() {
        let p = ids(1);
        let mut k = Concerns::default();
        for day in 0..(CONCERN_DAYS as u64 + 30) {
            k.note(quote(day * MINUTES_PER_DAY + 1, p[0], Concern::Noise));
        }
        assert_eq!(k.days.len(), CONCERN_DAYS);
        assert_eq!(k.days.front().unwrap().day, 30, "the oldest days drop off");
        assert_eq!(k.count(Concern::Noise, 0, 29), 0);
        assert_eq!(k.totals[Concern::Noise.index()], CONCERN_DAYS as u64 + 30, "all-time totals keep counting");
        assert_eq!(k.examples[Concern::Noise.index()].len(), EXAMPLES);
        assert!(k.spots[Concern::Noise.index()].len() <= SPOTS);
    }

    #[test]
    fn ranking_orders_by_this_week_and_trends_against_last_week() {
        let p = ids(10);
        let mut k = Concerns::default();
        // Last week (days 0..7): lots of trash, a little food.
        for d in 0..7u64 {
            for &c in &p[..4] {
                k.note(quote(d * MINUTES_PER_DAY + 1, c, Concern::Trash));
            }
            k.note(quote(d * MINUTES_PER_DAY + 1, p[5], Concern::Food));
        }
        // This week (days 7..14): food rising, trash falling, noise new.
        for d in 7..14u64 {
            k.note(quote(d * MINUTES_PER_DAY + 1, p[0], Concern::Trash));
            for &c in &p[..3] {
                k.note(quote(d * MINUTES_PER_DAY + 1, c, Concern::Food));
            }
        }
        k.note(quote(13 * MINUTES_PER_DAY + 2, p[9], Concern::Noise));
        let r = k.ranked(13);
        let order: Vec<Concern> = r.iter().map(|x| x.concern).collect();
        assert_eq!(order, vec![Concern::Food, Concern::Trash, Concern::Noise]);
        assert_eq!((r[0].this_week, r[0].last_week), (21, 7));
        assert_eq!(r[0].direction(), 1);
        assert!((r[0].trend() - 2.0).abs() < 1e-6);
        assert_eq!((r[1].this_week, r[1].last_week), (7, 28));
        assert_eq!(r[1].direction(), -1);
        assert_eq!(r[2].direction(), 1, "new this week counts as rising");
        assert_eq!(r[2].today, 1);
    }

    #[test]
    fn the_hotspot_is_the_busiest_square() {
        let p = ids(6);
        let mut k = Concerns::default();
        for (i, &c) in p.iter().enumerate() {
            let mut q = quote(5, c, Concern::Parking);
            q.pos = if i < 4 { Vec2::new(500.0 + i as f32, 500.0) } else { Vec2::new(-300.0, 0.0) };
            k.note(q);
        }
        let (at, n) = k.hotspot(Concern::Parking, 0, 7).unwrap();
        assert_eq!(n, 4);
        assert!(at.distance(Vec2::new(501.5, 500.0)) < 1.0);
        assert!(k.hotspot(Concern::Power, 0, 7).is_none());
    }
}
