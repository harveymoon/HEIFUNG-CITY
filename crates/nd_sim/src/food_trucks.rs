//! [r14-foodtrucks] Food trucks and street vendors: the data and the pure
//! rules. The systems are in `world/food_trucks.rs`; the facts about each
//! type (menu, price, ingredients, hours, where it likes to stand) are the
//! catalogue's `FOOD_TRUCKS` table.
//!
//! Harvey (2026-11-20): "ice-cream trucks, taco trucks, food venders. food
//! trucks go to popular parks or hang outside stadiums or events."
//!
//! A food truck is a small business run by a household (`BusinessKind::
//! FoodTruck`): an ice-cream van, a taco, noodle or kebab truck, a coffee
//! cart, a hot-dog or a fruit stand on a three-wheeler. It lives at its
//! owner's home (the yard or the street outside: out of sight in the sim's
//! terms), and every morning:
//!
//! - it plans the day (`World::plan_food_trucks`): from the town's crowds
//!   (`crowd_spots`: park visitors, beach-goers, the stadium's game, gigs
//!   and raves, the bars after dark, the offices at lunch) it picks one or
//!   two stints, each a spot and the hours there, the best for its menu and
//!   the weather (no ice cream in the snow), shared out so the trucks do
//!   not all pile onto one corner;
//! - the owner buys the day's ingredients from the town's grocers, farms,
//!   dairies and mills (wholesale from out of town when nobody has them);
//! - an adult of the household drives it out as a vehicle in the traffic,
//!   parks it in a legal kerb slot by the spot (or a trading pitch on a
//!   shopping street), opens the hatch and the awning, and sells: people
//!   near by (out in the park, coming out of the game, on a night out, on
//!   their lunch break, strolling past) queue at the hatch and buy a meal,
//!   which feeds them (`satiety`) and counts as eating out;
//! - at the end of the stint it packs up and drives on to the next spot, or
//!   home.
//!
//! The day's takings go to the household; the ingredients, a daily permit
//! (to the city) and the lease on the truck come out of them. A household
//! losing money for a week sells the truck. New trucks start up at night
//! where crowds went unfed (more people than trucks), up to one per
//! `PEOPLE_PER_TRUCK`.
//!
//! Everything random is a hash of the truck, the day and the hour, so the
//! same city makes the same choices.

use crate::buildings::BuildingId;
use crate::catalogue;
use crate::citizens::{CitizenId, HouseholdId};
use crate::roads::SegmentId;
use crate::traffic::CarId;
use crate::vehicle::{BodyType, Livery, VehicleClass, VehicleModel};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

slotmap::new_key_type! {
    /// A food truck, van, cart or stand.
    pub struct TruckId;
}

/// What it costs a household to start one: the deposit on the truck.
pub const TRUCK_DEPOSIT: f32 = 400.0;
/// What a household must have put by before it starts one.
pub const START_SAVINGS: f32 = 700.0;
/// The lease on the truck, a day.
pub const LEASE_PER_DAY: f32 = 9.0;
/// The city's street-trading permit, a day it trades.
pub const PERMIT_PER_DAY: f32 = 4.0;
/// Residents per food truck the town supports at most (one at least).
pub const PEOPLE_PER_TRUCK: usize = 1400;
/// New trucks a night, at most.
pub const NEW_PER_NIGHT: usize = 3;
/// Days in a row of losing money before the household sells up.
pub const LOSING_DAYS: u8 = 7;
/// A truck parks this near its spot's crowd, at most (m).
pub const PARK_REACH: f32 = 260.0;
/// People this near an open truck may stop for a bite (m, straight line).
pub const BITE_REACH: f32 = 220.0;
/// And someone out for a stroll, or looking for a meal.
pub const STROLL_REACH: f32 = 420.0;
/// Customers queued at (or walking to) one truck, at most.
pub const QUEUE_MAX: usize = 8;
/// Minutes a customer spends at the hatch (plus a little each ahead).
pub const SERVE_MINUTES: u64 = 4;
/// The shortest stint worth driving out for (hours).
pub const MIN_STINT: u8 = 2;
/// Stints a day, at most.
pub const MAX_STINTS: usize = 2;
/// A crowd smaller than this (people about at once) is no spot.
pub const MIN_CROWD: f32 = 6.0;
/// The hour trucks plan the day, and the hour they look again at the
/// evening (a gig or a rave booked since the morning).
pub const PLAN_HOUR: u8 = 5;
pub const REPLAN_HOUR: u8 = 15;
/// A truck holds the kerb slot it is heading for (no car takes it).
pub const SLOT_HOLD: bool = true;
/// Warm and dry enough for ice cream: degrees from which people want it.
pub const ICE_CREAM_TEMP: f32 = 15.0;

/// The types of food truck, in catalogue order. Appended only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TruckKind {
    /// The ice-cream van: cones and lollies, the jingle.
    #[default]
    IceCream,
    Taco,
    Noodle,
    Kebab,
    /// A coffee cart on a three-wheeler.
    Coffee,
    /// A hot-dog stand on a three-wheeler.
    HotDog,
    /// A fruit stand on a three-wheeler: cut fruit and juice.
    Fruit,
}

impl TruckKind {
    pub const COUNT: usize = 7;
    pub const ALL: [TruckKind; Self::COUNT] = [TruckKind::IceCream, TruckKind::Taco, TruckKind::Noodle, TruckKind::Kebab, TruckKind::Coffee, TruckKind::HotDog, TruckKind::Fruit];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> TruckKind {
        Self::ALL[i % Self::COUNT]
    }

    /// Its catalogue row: every fact about the type.
    pub fn row(self) -> &'static catalogue::TruckRow {
        catalogue::truck(self)
    }

    pub fn label(self) -> &'static str {
        self.row().label
    }

    pub fn key(self) -> &'static str {
        self.row().key
    }

    /// Small: a three-wheeler cart with an umbrella and stools (else a
    /// step van with a hatch and an awning).
    pub fn cart(self) -> bool {
        self.row().cart
    }

    /// Does it trade at `hour` (0..24)? Hours past midnight are kept as
    /// 24.. in the row.
    pub fn trades_at(self, hour: u8) -> bool {
        let (a, b) = self.row().hours;
        let h = hour as u16;
        let (a, b) = (a as u16, b as u16);
        (h >= a && h < b) || (h + 24 >= a && h + 24 < b)
    }

    /// How it looks on the road and at the kerb.
    pub fn model(self, number: u16) -> VehicleModel {
        VehicleModel { class: VehicleClass::Mid, body: BodyType::FoodTruck, wear: (number % 5 == 4) as u8, tuned: false, livery: Livery::FoodTruck(self as u8), paint: (number % crate::vehicle::PAINTS as u16) as u8, boxed: 0 }
    }
}

/// Where a crowd is: the kinds of spot a truck goes to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SpotKind {
    /// A busy park.
    #[default]
    Park,
    /// A beach with people on the sand.
    Beach,
    /// Outside the stadium on a game day.
    Stadium,
    /// A street of bars and clubs, late.
    Nightlife,
    /// An office district at lunch (and coffee in the morning).
    Offices,
    /// A gig or a rave: a festival crowd.
    Festival,
}

impl SpotKind {
    pub const COUNT: usize = 6;
    pub const ALL: [SpotKind; Self::COUNT] = [SpotKind::Park, SpotKind::Beach, SpotKind::Stadium, SpotKind::Nightlife, SpotKind::Offices, SpotKind::Festival];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            SpotKind::Park => "park",
            SpotKind::Beach => "beach",
            SpotKind::Stadium => "stadium",
            SpotKind::Nightlife => "nightlife",
            SpotKind::Offices => "offices",
            SpotKind::Festival => "event",
        }
    }

    /// Outdoors in the open: the weather keeps people away.
    pub fn outdoor(self) -> bool {
        matches!(self, SpotKind::Park | SpotKind::Beach)
    }
}

/// A crowd somewhere, from the town's own numbers: where, how many at once,
/// and the hours (`from..to`, past midnight as 24..).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrowdSpot {
    pub kind: SpotKind,
    pub at: Vec2,
    /// What it is called ("Harbour Park", "the stadium", "Neon Row").
    pub name: String,
    /// People about at once, at its busiest.
    pub crowd: f32,
    pub from: u8,
    pub to: u8,
    /// The building it is (a park, the stadium, a venue), if one.
    #[serde(default)]
    pub building: Option<BuildingId>,
}

impl CrowdSpot {
    /// Share of the busiest crowd about at `hour` (24.. past midnight).
    pub fn at_hour(&self, hour: u8) -> f32 {
        if hour < self.from || hour >= self.to {
            return 0.0;
        }
        match self.kind {
            // The lunch hour, a little coffee crowd before it.
            SpotKind::Offices => match hour {
                7..=10 => 0.45,
                11..=13 => 1.0,
                _ => 0.25,
            },
            SpotKind::Park | SpotKind::Beach => match hour {
                12..=16 => 1.0,
                10 | 11 | 17 => 0.7,
                _ => 0.4,
            },
            // Coming in at the gates, going home after the whistle.
            SpotKind::Stadium => match hour {
                16..=18 => 1.0,
                21 => 1.0,
                _ => 0.35,
            },
            SpotKind::Nightlife => match hour {
                22..=25 => 1.0,
                _ => 0.6,
            },
            SpotKind::Festival => 1.0,
        }
    }
}

/// One stint of a day's plan: a spot and the hours there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stint {
    pub kind: SpotKind,
    pub at: Vec2,
    pub name: String,
    pub from: u8,
    pub to: u8,
    /// The crowd it was picked for.
    pub crowd: f32,
    /// How it went: driven to, traded, skipped (and why).
    #[serde(default)]
    pub state: StintState,
    /// The day it was planned for (its hours count from that midnight).
    #[serde(default)]
    pub day: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum StintState {
    #[default]
    Planned,
    Driving,
    Trading,
    Done,
    Skipped(String),
}

impl StintState {
    pub fn label(&self) -> String {
        match self {
            StintState::Planned => "planned".into(),
            StintState::Driving => "driving there".into(),
            StintState::Trading => "trading".into(),
            StintState::Done => "done".into(),
            StintState::Skipped(why) => format!("skipped: {why}"),
        }
    }

    pub fn open(&self) -> bool {
        matches!(self, StintState::Planned | StintState::Driving | StintState::Trading)
    }
}

/// A kerb slot (or a trading pitch) a truck stands in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pitch {
    pub segment: SegmentId,
    pub side: u8,
    pub slot: u16,
    pub pos: Vec2,
    pub heading: f32,
    /// A legal kerb slot (else a trading pitch on a street whose kerb is
    /// for loading: shops and nightlife).
    pub legal: bool,
}

impl Pitch {
    /// The way the hatch faces: the pavement side (+Z of the drawn truck,
    /// the kerb side of `kerb_pose`).
    pub fn out(&self) -> Vec2 {
        Vec2::new(-self.heading.sin(), self.heading.cos())
    }

    pub fn fwd(&self) -> Vec2 {
        Vec2::new(self.heading.cos(), self.heading.sin())
    }

    /// Where queue place `k` stands: on the pavement in front of the
    /// hatch, then back along the kerb.
    pub fn queue_spot(&self, k: usize, cart: bool) -> Vec2 {
        let out = self.out();
        let hatch = self.pos + out * if cart { 2.0 } else { 2.3 } - self.fwd() * 0.3;
        hatch - self.fwd() * (0.85 * k as f32) + out * if k % 2 == 0 { 0.0 } else { 0.25 }
    }

    /// Where the person serving stands: inside at the hatch (a cart's
    /// vendor beside it on the pavement).
    pub fn vendor_spot(&self, cart: bool) -> Vec2 {
        if cart {
            self.pos + self.out() * 1.7 + self.fwd() * 1.1
        } else {
            self.pos + self.out() * 0.5
        }
    }
}

/// Where a truck is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum TruckPlace {
    /// At home: the yard or the street outside its owner's home.
    #[default]
    Base,
    /// At the kerb by a stint's spot: open (`open`) or not yet.
    Pitch { pitch: Pitch, open: bool },
    /// On the road, a car of the traffic model.
    Road(CarId),
}

impl TruckPlace {
    pub fn label(&self) -> &'static str {
        match self {
            TruckPlace::Base => "at home",
            TruckPlace::Pitch { open: true, .. } => "open for business",
            TruckPlace::Pitch { .. } => "parked",
            TruckPlace::Road(_) => "on the road",
        }
    }

    pub fn pitch(&self) -> Option<&Pitch> {
        match self {
            TruckPlace::Pitch { pitch, .. } => Some(pitch),
            _ => None,
        }
    }

    pub fn open(&self) -> bool {
        matches!(self, TruckPlace::Pitch { open: true, .. })
    }
}

/// What a truck is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum TruckJob {
    #[default]
    Idle,
    /// Driving to stint `stint`'s pitch.
    ToPitch { stint: u8, pitch: Pitch },
    /// Driving home.
    Home,
    /// Sold: driving out of town for good.
    Leaving,
}

impl TruckJob {
    pub fn label(&self) -> &'static str {
        match self {
            TruckJob::Idle => "",
            TruckJob::ToPitch { .. } => "driving out to its spot",
            TruckJob::Home => "driving home",
            TruckJob::Leaving => "sold, leaving town",
        }
    }
}

/// One spot a truck traded at today: when, where and how it went.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub kind: SpotKind,
    pub name: String,
    pub at: Vec2,
    pub opened: u64,
    pub closed: Option<u64>,
    pub meals: u32,
    pub takings: f32,
}

/// A food truck.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Truck {
    pub kind: TruckKind,
    pub name: String,
    pub household: HouseholdId,
    /// The owner's home: where it is kept.
    pub base: BuildingId,
    /// Its number in the town's trucks (from 1): paint and wear.
    pub number: u16,
    pub model: VehicleModel,
    pub place: TruckPlace,
    #[serde(default)]
    pub job: TruckJob,
    /// Who of the household is driving it and serving today.
    #[serde(default)]
    pub crew: Option<CitizenId>,
    /// The one of the household who runs it (no other job), when there is.
    #[serde(default)]
    pub operator: Option<CitizenId>,
    /// Today's plan.
    #[serde(default)]
    pub plan: Vec<Stint>,
    /// Meals' worth of ingredients aboard.
    #[serde(default)]
    pub stock: f32,
    /// Where it traded today (and yesterday's, rolled at midnight).
    #[serde(default)]
    pub visits: Vec<Visit>,
    #[serde(default)]
    pub visits_yesterday: Vec<Visit>,
    #[serde(default)]
    pub meals_today: u32,
    #[serde(default)]
    pub meals_yesterday: u32,
    #[serde(default)]
    pub meals_total: u64,
    #[serde(default)]
    pub takings_today: f32,
    #[serde(default)]
    pub takings_yesterday: f32,
    #[serde(default)]
    pub takings_total: f64,
    /// What today's ingredients, permit and lease cost.
    #[serde(default)]
    pub costs_today: f32,
    #[serde(default)]
    pub costs_yesterday: f32,
    /// Bought in town today (meals' worth) and from out of town.
    #[serde(default)]
    pub bought_local_today: f32,
    #[serde(default)]
    pub bought_wholesale_today: f32,
    /// Days in a row the takings did not cover the costs.
    #[serde(default)]
    pub losing_days: u8,
    /// Hungry people (satiety under a quarter) it fed today.
    #[serde(default)]
    pub fed_hungry_today: u32,
    /// Tick it started trading.
    #[serde(default)]
    pub since: u64,
    #[serde(default)]
    pub last_pos: Vec2,
    /// Tick it took up its place or job.
    #[serde(default)]
    pub since_job: u64,
}

impl Truck {
    /// The stint under way or next: the first one not done or skipped.
    pub fn current_stint(&self) -> Option<(usize, &Stint)> {
        self.plan.iter().enumerate().find(|(_, s)| s.state.open())
    }

    pub fn pos(&self) -> Vec2 {
        match self.place {
            TruckPlace::Pitch { pitch, .. } => pitch.pos,
            _ => self.last_pos,
        }
    }
}

/// The town's numbers, for the report and the API.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TruckStats {
    pub meals_today: u32,
    pub meals_yesterday: u32,
    pub takings_today: f32,
    pub takings_yesterday: f32,
    /// Meals to someone hungry (satiety under a quarter).
    pub fed_hungry_today: u32,
    pub fed_hungry_yesterday: u32,
    /// Trucks open at each hour today, and yesterday.
    pub open_by_hour: Vec<u16>,
    pub open_by_hour_yesterday: Vec<u16>,
    /// Meals sold at each kind of spot today and yesterday.
    pub meals_by_spot: Vec<u32>,
    pub meals_by_spot_yesterday: Vec<u32>,
    /// Ice-cream stints given up for the cold or the wet, today.
    pub too_cold_today: u32,
    pub too_cold_yesterday: u32,
    pub founded_total: u32,
    pub sold_total: u32,
    pub meals_total: u64,
}

/// Every food truck, the customers on their way to one, and the numbers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FoodTrucks {
    pub trucks: slotmap::SlotMap<TruckId, Truck>,
    /// Customers walking to (or queuing at) a truck.
    pub trips: BTreeMap<CitizenId, TruckId>,
    /// Tick of each customer's last meal from a truck (pruned daily).
    pub last_meal: BTreeMap<CitizenId, u64>,
    pub stats: TruckStats,
    /// The crowds at the last plan, for the API and the inspector.
    pub spots: Vec<CrowdSpot>,
    /// Trucks numbered so far.
    pub numbered: u16,
    /// Dev: no trucks start, and nobody buys from one (to compare a town
    /// with and without them).
    pub disabled: bool,
}

impl FoodTrucks {
    /// The truck `c` drives or serves at, if any.
    pub fn crewed_by(&self, c: CitizenId) -> Option<TruckId> {
        self.trucks.iter().find(|(_, t)| t.crew == Some(c)).map(|(id, _)| id)
    }

    /// Is this kerb slot taken by a truck (standing there or heading for it)?
    pub fn holds(&self, seg: SegmentId, side: u8, slot: u16) -> bool {
        self.trucks.values().any(|t| {
            let p = match (t.place, t.job) {
                (TruckPlace::Pitch { pitch, .. }, _) => Some(pitch),
                (_, TruckJob::ToPitch { pitch, .. }) => Some(pitch),
                _ => None,
            };
            p.is_some_and(|p| p.segment == seg && p.side == side && p.slot == slot)
        })
    }

    /// Trucks open for business: id, pitch and type.
    pub fn open_trucks(&self) -> impl Iterator<Item = (TruckId, &Truck, Pitch)> {
        self.trucks.iter().filter_map(|(id, t)| match t.place {
            TruckPlace::Pitch { pitch, open: true } => Some((id, t, pitch)),
            _ => None,
        })
    }

    /// Customers at or walking to truck `id`.
    pub fn queue_len(&self, id: TruckId) -> usize {
        self.trips.values().filter(|t| **t == id).count()
    }
}

/// How the weather sends people out (and in): `0..1.2` for a spot in the
/// open, by the temperature and the rain or snow.
pub fn outdoor_factor(temp_c: f32, wet: bool) -> f32 {
    let warm = ((temp_c - 2.0) / 18.0).clamp(0.08, 1.2);
    if wet { warm * 0.25 } else { warm }
}

/// How much a crowd at `spot` wants what truck type `k` sells at `hour`,
/// in this weather: the crowd times the type's liking for the spot, the
/// weather for spots in the open, ice cream only when warm and dry, coffee
/// more in the cold.
pub fn appeal(k: TruckKind, spot: &CrowdSpot, hour: u8, temp_c: f32, wet: bool) -> f32 {
    let row = k.row();
    let mut a = spot.crowd * spot.at_hour(hour) * row.affinity[spot.kind.index()];
    if spot.kind.outdoor() {
        a *= outdoor_factor(temp_c, wet);
    }
    if row.warm_only && !ice_cream_weather(temp_c, wet) {
        a *= 0.02;
    }
    if row.cold_lift && temp_c < 10.0 {
        a *= 1.3;
    }
    a
}

/// Warm and dry enough for ice cream.
pub fn ice_cream_weather(temp_c: f32, wet: bool) -> bool {
    !wet && temp_c >= ICE_CREAM_TEMP
}

/// The best stints for a truck of type `k` living at `base` from `from`
/// o'clock on: for each spot, the hours both it and the type trade (at
/// least `MIN_STINT`, cut to the busiest `max_len`), scored by the crowd's
/// appeal over them, less the drive, and less again for every truck
/// already going there (`taken`: spot index -> trucks). Up to
/// `max`, an hour apart for the drive and clear of the hours already
/// `busy`. `temp_at` gives the temperature at an hour; `wet` is now.
#[allow(clippy::too_many_arguments)]
pub fn pick_stints(k: TruckKind, base: Vec2, from: u8, spots: &[CrowdSpot], taken: &BTreeMap<usize, u32>, temp_at: &dyn Fn(u8) -> f32, wet: bool, salt: u64, busy: &[(u8, u8)], max: usize) -> Vec<(usize, u8, u8, f32)> {
    let row = k.row();
    let (ha, hb) = row.hours;
    let mut options: Vec<(f32, usize, u8, u8)> = Vec::new();
    for (i, s) in spots.iter().enumerate() {
        let a = s.from.max(ha).max(from);
        let b = s.to.min(hb);
        if b < a + MIN_STINT {
            continue;
        }
        // The busiest window of up to `max_len` hours in a..b.
        let len = (b - a).min(row.max_len.max(MIN_STINT));
        let mut best: Option<(f32, u8)> = None;
        for start in a..=(b - len) {
            let v: f32 = (start..start + len).map(|h| appeal(k, s, h, temp_at(h % 24), wet)).sum();
            if best.is_none_or(|(bv, _)| v > bv) {
                best = Some((v, start));
            }
        }
        let Some((v, start)) = best else { continue };
        let per_hour = v / len as f32;
        if per_hour < MIN_CROWD * 0.15 {
            continue;
        }
        let km = base.distance(s.at) / 1000.0;
        let crowded = 1.0 + 0.9 * taken.get(&i).copied().unwrap_or(0) as f32;
        let jitter = 1.0 + 0.15 * crate::hash::hash01(salt, i as u64, 0xF00D);
        let score = v / crowded / (1.0 + 0.25 * km) * jitter;
        options.push((score, i, start, start + len));
    }
    options.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
    let mut out: Vec<(usize, u8, u8, f32)> = Vec::new();
    for (score, i, a, b) in options {
        if out.len() >= max {
            break;
        }
        // An hour clear of every stint chosen, for the drive between.
        if out.iter().any(|&(_, oa, ob, _)| a < ob + 1 && oa < b + 1) || busy.iter().any(|&(oa, ob)| a < ob + 1 && oa < b + 1) {
            continue;
        }
        // The same spot twice is one stint.
        if out.iter().any(|o| o.0 == i) {
            continue;
        }
        out.push((i, a, b, score));
    }
    out.sort_by_key(|o| o.1);
    out
}

/// A name for a truck: its owner's family name and the type.
pub fn truck_name(kind: TruckKind, family: &str, seed: u64) -> String {
    let words: &[&str] = match kind {
        TruckKind::IceCream => &["Mr {} Whippy", "{}'s Ices", "Frosty {}", "{} Cones"],
        TruckKind::Taco => &["Tacos {}", "{}'s Taqueria", "El {} Taco Truck", "Taco {}"],
        TruckKind::Noodle => &["{} Noodle Wagon", "{}'s Wok on Wheels", "Slurp {}", "{} Ramen Van"],
        TruckKind::Kebab => &["{}'s Kebabs", "Kebab {}", "{} Grill Van", "Late {} Doner"],
        TruckKind::Coffee => &["{} Coffee Cart", "Bean {}", "{}'s Brew", "Cup of {}"],
        TruckKind::HotDog => &["{}'s Dogs", "Top Dog {}", "{} Frank Stand", "Hot {} Dogs"],
        TruckKind::Fruit => &["{}'s Fruit", "Fresh {}", "{} Juice Cart", "{} Fruit Stand"],
    };
    words[(seed % words.len() as u64) as usize].replace("{}", family)
}
