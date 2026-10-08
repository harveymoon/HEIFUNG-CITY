//! [events] Night life the council did not license: street races and
//! warehouse raves. This file holds what the city remembers about them and
//! the geometry (where cars park in a lot, where the queue stands); the
//! nights themselves are run by `world/street_events.rs`.
//!
//! *Street races*: on some nights the owners of fast cars (tuned, coupés,
//! expensive models, superbikes) meet in an empty parking lot, show their
//! cars off, then race each other over a route of city streets to another
//! lot, where they party. A neighbour may call the police; a unit that gets
//! there while they race or party busts it: some arrests, a fine for every
//! racer caught, and maybe a chase.
//!
//! *Warehouse raves*: where there is a culture for it (young hedonists or an
//! art scene living near the old warehouses, `RavePull`), a warehouse is
//! opened for a night: lights, a queue outside the door, noise for the
//! neighbours. Complaints may bring the police, who shut it down and fine
//! the promoter.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::traffic::{CarId, LaneId};
use crate::zoning::{Lot, LotId};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A young town has no scene yet: nights out like these start once the
/// city has been going this many days (the API can force one any time).
pub const SCENE_FROM_DAY: u64 = 10;

// ── races ─────────────────────────────────────────────────────────────────

/// Chance a night has a race when at least `MIN_RACERS` could come.
pub const RACE_NIGHT_CHANCE: f64 = 0.35;
/// When the night's race is rolled (minute of the day), and the window it
/// starts in after that.
pub const RACE_ROLL_MINUTE: u32 = 23 * 60;
pub const RACE_START_WINDOW: (u64, u64) = (30, 120);
pub const MIN_RACERS: usize = 3;
pub const MAX_RACERS: usize = 8;
/// Ticks the crews wait in the lot for stragglers before the race goes.
pub const RACE_GATHER: u64 = 40;
/// Minutes in the lot showing the cars off once the crew is in.
pub const SHOW_OFF: u64 = 10;
/// The longest a race may take before whoever is not in is left behind.
pub const RACE_MAX: u64 = 90;
/// How long the party at the finish runs, ticks.
pub const PARTY_TICKS: (u64, u64) = (50, 90);
/// How far (straight line, m) the finish lot is from the start.
pub const RACE_LENGTH: (f32, f32) = (450.0, 2400.0);
/// The shortest route by road a race is run over (m).
pub const RACE_MIN_ROUTE: f32 = 550.0;
/// A car this close to its lot's kerb point pulls in.
pub const LOT_REACH: f32 = 45.0;
/// Chance a racer still there is arrested when the police arrive, and the
/// most taken.
pub const RACE_ARREST_CHANCE: f32 = 0.4;
pub const RACE_MAX_ARRESTS: u32 = 3;
/// What each racer the police catch pays (from their household's money).
pub const RACE_FINE: f32 = 250.0;
/// Spectators at most.
pub const RACE_CROWD: usize = 18;

// ── moto packs ────────────────────────────────────────────────────────────

/// [moto] Chance an evening has a moto pack ride when the city has a crew,
/// the minute it is rolled and the window it starts in after that.
pub const MOTO_NIGHT_CHANCE: f32 = 0.25;
pub const MOTO_ROLL_MINUTE: u32 = 20 * 60;
pub const MOTO_START_WINDOW: (u64, u64) = (20, 150);
/// Local gang members on bikes in a pack, at most, and riders from out of
/// town (a range).
pub const MOTO_LOCALS: usize = 4;
pub const MOTO_OUTSIDERS: (u32, u32) = (3, 7);
/// Ticks the outsiders wait off the map for the locals to reach the meet.
pub const MOTO_GATHER: u64 = 45;
/// The longest a ride lasts before the stragglers are sent off.
pub const MOTO_MAX: u64 = 90;
/// A police car this close to a local rider may give chase (m), and the
/// chance it does (once per pack).
pub const MOTO_CHASE_REACH: f32 = 150.0;
pub const MOTO_CHASE_CHANCE: f32 = 0.6;
/// A rider further than this ahead of (or behind) the pack eases off (or
/// opens up) to keep the column loose but together (m).
pub const MOTO_SPREAD: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotoStage {
    /// Locals riding to the meet at the road end; outsiders off the map.
    Gathering,
    /// Outsiders rolling in; locals pull out as they come by.
    Rolling,
    /// The pack is on the streets.
    Riding,
}

/// One rider: their bike on the road, the local riding it (none for an
/// outsider), and whether they have peeled off home.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rider {
    pub car: CarId,
    pub citizen: Option<CitizenId>,
    /// Joined the ride (a local that reached the meet, or an outsider).
    #[serde(default)]
    pub riding: bool,
    /// The connector whose red it last ran (counted once).
    #[serde(default)]
    pub last_run: Option<LaneId>,
    /// The lane it was on last tick (for counting lane changes).
    #[serde(default)]
    pub lane: Option<LaneId>,
}

/// [moto] A pack of motorbikes riding through town: local gang members
/// (the takeover crew on two wheels) and a club from out of town.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MotoPack {
    pub id: u32,
    pub stage: MotoStage,
    /// The road end they meet at and come in by, the lane and the place
    /// on it the locals wait at.
    pub entry: crate::roads::NodeId,
    pub meet_lane: LaneId,
    pub meet_s: f32,
    pub meet: Vec2,
    /// The way through town: the meet to `via` (a nightlife street, else
    /// the middle of town), then off the map at `exit`.
    pub via: Vec2,
    pub via_name: String,
    pub exit: (LaneId, f32),
    pub place: String,
    pub started: u64,
    pub set_off: Option<u64>,
    pub riders: Vec<Rider>,
    pub to_spawn: u32,
    pub outsiders: u32,
    /// Locals who set out for the meet.
    #[serde(default)]
    pub locals: u32,
    /// The 911 call about them and the chase it became.
    pub incident: Option<u32>,
    pub chase: Option<u32>,
    #[serde(default)]
    pub chase_tried: bool,
    /// Reds run and lane changes through traffic, for the record.
    pub red_runs: u32,
    pub lane_changes: u32,
}

impl MotoPack {
    /// Riders still with the pack.
    pub fn riding(&self) -> usize {
        self.riders.iter().filter(|r| r.riding).count()
    }
}

// ── raves ─────────────────────────────────────────────────────────────────

/// When tonight's rave is rolled, when doors open, and when it would end
/// by itself (ticks after the doors, a range).
pub const RAVE_ROLL_MINUTE: u32 = 22 * 60;
pub const RAVE_DOORS_MINUTE: u32 = 23 * 60;
pub const RAVE_LENGTH: (u64, u64) = (240, 330);
/// Where the culture is counted from: homes within this of the warehouse.
pub const RAVE_REACH: f32 = 900.0;
/// A culture worth a rave: young hedonists / `YOUTH_PER` + artists /
/// `ARTISTS_PER` at least 1 (`RavePull::culture`).
pub const YOUTH_PER: f32 = 30.0;
pub const ARTISTS_PER: f32 = 6.0;
/// Chance a night with a pull of 1 has a rave (more with a stronger one).
pub const RAVE_NIGHT_CHANCE: f32 = 0.12;
/// A warehouse that held a rave waits this many days before the next, and
/// the city at least `RAVE_GAP_DAYS` between any two.
pub const RAVE_REST_DAYS: u64 = 4;
pub const RAVE_GAP_DAYS: u64 = 2;
/// The neighbours put up with the first hour; complaints start after it.
pub const NOISE_GRACE: u64 = 60;
/// Guests at most, and how far they come from.
pub const RAVE_GUESTS: usize = 70;
pub const RAVE_GUEST_REACH: f32 = 1600.0;
/// People the door lets in a tick.
pub const DOOR_RATE: usize = 2;
/// Neighbours within this hear it.
pub const NOISE_REACH: f32 = 220.0;
/// What the promoter pays when the police shut it down.
pub const RAVE_FINE: f32 = 400.0;
/// Paid on the door; half of it hires the place from whoever runs the
/// warehouse (their business's takings).
pub const RAVE_DOOR_FEE: f32 = 8.0;

/// Where a race is in its night.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaceStage {
    /// Cars driving to the meet and parking up in the lot.
    Gathering,
    /// Flat out along the route.
    Racing,
    /// Parked up at the finish lot, music on.
    Party,
}

impl RaceStage {
    pub fn label(self) -> &'static str {
        match self {
            RaceStage::Gathering => "gathering",
            RaceStage::Racing => "racing",
            RaceStage::Party => "party",
        }
    }
}

/// A lot used as a meet or a finish: where cars pull in from, and where.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spot {
    pub lot: LotId,
    pub name: String,
    pub centre: Vec2,
    /// The kerb point cars reach it from.
    pub kerb: Vec2,
    /// Towards the street.
    pub front: Vec2,
    /// Width along the street and depth back from it (m).
    pub width: f32,
    pub depth: f32,
}

impl Spot {
    pub fn of(lot_id: LotId, lot: &Lot, name: String) -> Self {
        let front = if lot.front_dir.length_squared() > 0.0 { lot.front_dir.normalize() } else { Vec2::X };
        let along = Vec2::new(-front.y, front.x);
        let (mut w0, mut w1, mut d0, mut d1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &p in &lot.polygon {
            let q = p - lot.center;
            w0 = w0.min(q.dot(along));
            w1 = w1.max(q.dot(along));
            d0 = d0.min(q.dot(front));
            d1 = d1.max(q.dot(front));
        }
        let (width, depth) = if w0 <= w1 { (w1 - w0, d1 - d0) } else { (20.0, 20.0) };
        Self { lot: lot_id, name, centre: lot.center, kerb: if lot.car_segment.is_some() { lot.car_point } else { lot.center + front * depth * 0.5 }, front, width, depth }
    }

    /// Bay `k` of `n`: rows across the lot facing the street, nose in, a
    /// car's width and a half apart (squeezed to fit a small lot).
    pub fn bay(&self, k: usize, n: usize) -> (Vec2, f32) {
        let along = Vec2::new(-self.front.y, self.front.x);
        let per_row = ((self.width * 0.8 / 3.4).floor() as usize).clamp(2, 8);
        let rows = n.div_ceil(per_row).max(1);
        let (row, col) = (k / per_row, k % per_row);
        let in_row = if row + 1 == rows { n - row * per_row } else { per_row }.max(1);
        let spacing = (self.width * 0.8 / per_row as f32).clamp(2.8, 4.0);
        let x = (col as f32 - (in_row as f32 - 1.0) * 0.5) * spacing;
        let depth = (self.depth * 0.7).max(6.0);
        let y = depth * 0.5 - 4.0 - row as f32 * 6.5;
        let y = y.max(-depth * 0.5 + 2.5);
        let p = self.centre + along * x + self.front * y;
        // Nose towards the street, so the crowd behind sees the lights.
        let face = self.front.y.atan2(self.front.x) + 0.12 * if k % 2 == 0 { 1.0 } else { -1.0 };
        (p, face)
    }

    /// Where onlooker `k` stands: round the back and sides of the lot.
    pub fn onlooker(&self, k: usize) -> Vec2 {
        let along = Vec2::new(-self.front.y, self.front.x);
        let depth = (self.depth * 0.7).max(6.0);
        let x = ((k % 6) as f32 - 2.5) * 2.2;
        let y = -depth * 0.5 + 0.8 + (k / 6) as f32 * 1.1;
        self.centre + along * x + self.front * y
    }
}

/// One street race under way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Race {
    pub id: u32,
    pub stage: RaceStage,
    pub meet: Spot,
    pub finish: Spot,
    pub started: u64,
    /// When the flag dropped, and when the party ends.
    pub go_at: Option<u64>,
    pub ends_at: Option<u64>,
    /// The racers and their cars (same order).
    pub racers: Vec<CitizenId>,
    pub cars: Vec<CarId>,
    /// The route every racer drives, and its length (m).
    pub route: Vec<LaneId>,
    pub route_m: f32,
    /// Finishers in order.
    pub finished: Vec<CarId>,
    /// Bays handed out in the current lot.
    pub bays: usize,
    pub crowd: Vec<CitizenId>,
    pub incident: Option<u32>,
    pub arrests: u32,
    pub fined: u32,
    pub fines: f32,
    /// The incident of a chase that came out of the bust.
    pub chase: Option<u32>,
}

impl Race {
    /// The racer at the wheel of `car`.
    pub fn racer_of(&self, car: CarId) -> Option<CitizenId> {
        self.cars.iter().position(|&c| c == car).and_then(|k| self.racers.get(k).copied())
    }

    /// Where `car` placed (1-based), if it has finished.
    pub fn place_of(&self, car: CarId) -> Option<usize> {
        self.finished.iter().position(|&c| c == car).map(|k| k + 1)
    }
}

/// One warehouse rave under way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rave {
    pub id: u32,
    pub building: BuildingId,
    pub place: String,
    pub centre: Vec2,
    /// The door, and the way the queue runs from it along the pavement.
    pub door: Vec2,
    pub queue_dir: Vec2,
    pub started: u64,
    pub ends_at: u64,
    /// Who put it on (fined if it is shut down).
    pub promoter: Option<CitizenId>,
    /// Everyone who came (or is coming), the queue in order, and who is in.
    pub guests: Vec<CitizenId>,
    pub queue: Vec<CitizenId>,
    pub inside: Vec<CitizenId>,
    /// The most inside at once.
    pub peak: u32,
    /// Neighbours who complained, and the call it brought.
    pub complaints: u32,
    pub incident: Option<u32>,
    pub arrests: u32,
    pub fines: f32,
    /// Taken on the door.
    #[serde(default)]
    pub takings: f32,
    /// What made the culture here (for the record).
    pub pull: RavePull,
}

impl Rave {
    /// Queue place `k`: along the pavement from the door.
    pub fn queue_spot(&self, k: usize) -> Vec2 {
        let side = Vec2::new(-self.queue_dir.y, self.queue_dir.x);
        self.door + self.queue_dir * (1.5 + k as f32 * 0.85) + side * if k % 2 == 0 { 0.15 } else { -0.15 }
    }
}

/// Why a warehouse could hold a rave: the culture living round it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RavePull {
    /// Young hedonistic adults living within `RAVE_REACH`.
    pub youth: u32,
    /// Artists living within reach.
    pub artists: u32,
    /// The city has an art scene (`Signals::art_scene`).
    pub art_scene: bool,
    /// Homes within `NOISE_REACH`: who will hear it.
    pub neighbours: u32,
}

impl RavePull {
    /// How strong the culture is here: 1 is enough for a rave.
    pub fn culture(&self) -> f32 {
        self.youth as f32 / YOUTH_PER + self.artists as f32 / ARTISTS_PER + if self.art_scene { 0.25 } else { 0.0 }
    }

    pub fn has_scene(&self) -> bool {
        self.culture() >= 1.0
    }

    /// The night's chance of a rave here.
    pub fn night_chance(&self) -> f32 {
        if !self.has_scene() {
            return 0.0;
        }
        (RAVE_NIGHT_CHANCE * self.culture().min(2.5)).min(0.3)
    }

    /// Why (not) in words.
    pub fn why(&self) -> String {
        let scene = if self.art_scene { ", the city's art scene" } else { "" };
        if self.has_scene() {
            format!("{} young night owls and {} artists live round it{scene}", self.youth, self.artists)
        } else {
            format!("no scene: {} young night owls and {} artists round it{scene} (needs about {:.0} or {:.0})", self.youth, self.artists, YOUTH_PER, ARTISTS_PER)
        }
    }
}

/// A warehouse weighed for tonight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaveSpot {
    pub building: BuildingId,
    pub name: String,
    pub pos: Vec2,
    pub pull: RavePull,
    /// The day it last held one.
    pub last_rave_day: Option<u64>,
}

/// What kind of night out it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Race,
    Rave,
    /// [moto] A motorbike gang riding through town as a pack.
    MotoPack,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            EventKind::Race => "street race",
            EventKind::Rave => "warehouse rave",
            EventKind::MotoPack => "moto pack",
        }
    }
}

/// How one ended, for the record and the news.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: u32,
    pub kind: EventKind,
    pub place: String,
    pub pos: Vec2,
    /// A race's finish (the after-party).
    #[serde(default)]
    pub finish: Option<String>,
    pub started: u64,
    pub ended: u64,
    /// Racers or guests, and the crowd round a race.
    pub people: u32,
    pub crowd: u32,
    /// The winner of a race.
    #[serde(default)]
    pub winner: Option<String>,
    pub police: bool,
    pub arrests: u32,
    pub fines: f32,
    #[serde(default)]
    pub complaints: u32,
    #[serde(default)]
    pub chase: bool,
    /// A rave's culture round the warehouse that night (`RavePull::culture`).
    #[serde(default)]
    pub culture: f32,
}

/// One line of city news (the app toasts them, the API lists them).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct News {
    pub seq: u32,
    pub tick: u64,
    pub kind: EventKind,
    pub headline: String,
    pub pos: Vec2,
}

/// Everything the city remembers about its unlicensed nights.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreetEvents {
    pub next_id: u32,
    pub race: Option<Race>,
    /// The tick tonight's race starts, if one is planned.
    pub race_planned: Option<u64>,
    pub race_rolled_day: u64,
    pub rave: Option<Rave>,
    /// Tonight's rave: the warehouse and the doors tick.
    pub rave_planned: Option<(BuildingId, u64)>,
    pub rave_rolled_day: u64,
    /// The warehouses weighed at the last roll, best first.
    pub rave_spots: Vec<RaveSpot>,
    /// Day each warehouse last held a rave.
    pub rave_days: Vec<(BuildingId, u64)>,
    pub history: Vec<EventRecord>,
    pub news: Vec<News>,
    pub news_seq: u32,
    pub races_total: u32,
    pub raves_total: u32,
    pub busts_total: u32,
    pub fines_total: f32,
    /// [moto] The pack riding now, tonight's plan and the day it was rolled.
    pub moto: Option<MotoPack>,
    pub moto_planned: Option<u64>,
    pub moto_rolled_day: u64,
    pub packs_total: u32,
    pub pack_chases_total: u32,
}

impl StreetEvents {
    pub fn push_news(&mut self, tick: u64, kind: EventKind, headline: String, pos: Vec2) -> u32 {
        self.news_seq += 1;
        let seq = self.news_seq;
        self.news.push(News { seq, tick, kind, headline, pos });
        if self.news.len() > 40 {
            self.news.remove(0);
        }
        seq
    }

    pub fn news_item(&self, seq: u32) -> Option<&News> {
        self.news.iter().rev().find(|n| n.seq == seq)
    }

    pub fn record(&mut self, r: EventRecord) {
        self.history.push(r);
        if self.history.len() > 40 {
            self.history.remove(0);
        }
    }

    pub fn last_rave_day(&self, b: BuildingId) -> Option<u64> {
        self.rave_days.iter().find(|x| x.0 == b).map(|x| x.1)
    }

    /// [moto] The pack `car` rides in, if any.
    pub fn pack_of_car(&self, car: CarId) -> Option<&MotoPack> {
        self.moto.as_ref().filter(|p| p.riders.iter().any(|r| r.car == car))
    }

    /// The race `car` is in, if any.
    pub fn race_of_car(&self, car: CarId) -> Option<&Race> {
        self.race.as_ref().filter(|r| r.cars.contains(&car))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rave_needs_a_culture() {
        let none = RavePull { youth: 10, artists: 1, ..Default::default() };
        assert!(!none.has_scene() && none.night_chance() == 0.0);
        let youth = RavePull { youth: 40, ..Default::default() };
        assert!(youth.has_scene() && youth.night_chance() > 0.1);
        let art = RavePull { artists: 4, youth: 12, art_scene: true, ..Default::default() };
        assert!(art.has_scene());
    }

    #[test]
    fn bays_stay_in_the_lot_and_apart() {
        let s = Spot { lot: LotId::default(), name: "Lot 7".into(), centre: Vec2::ZERO, kerb: Vec2::new(0.0, 20.0), front: Vec2::Y, width: 30.0, depth: 30.0 };
        let bays: Vec<Vec2> = (0..8).map(|k| s.bay(k, 8).0).collect();
        for (i, a) in bays.iter().enumerate() {
            assert!(a.x.abs() <= 15.0 && a.y.abs() <= 15.0, "bay {i} at {a}");
            for b in &bays[i + 1..] {
                assert!(a.distance(*b) > 2.5);
            }
        }
    }
}
