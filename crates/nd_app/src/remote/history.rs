//! City history for the remote API's `history` command: a daily and an
//! hourly ring of headline numbers, kept by the app (not the save).
//!
//! Why here and not in `nd_sim`: the sim only keeps today and yesterday,
//! and a time series is a reviewing aid, not game state; keeping it out of
//! `World` leaves saves and determinism alone. The price is that history
//! starts when the app starts (or a city is loaded) and is lost on exit.
//!
//! Sampling: `History::observe` is called every frame from `SimSet::Sync`
//! and after every chunk of a remote `step`, which steps hour by hour so a
//! multi-day step still records every day and hour. `observe` itself only
//! compares clock numbers and copies a handful of counters; the per-day
//! sample (one pass over citizens and buildings) runs once a sim day.

use std::collections::VecDeque;

use bevy::prelude::Resource;
use nd_sim::World;
use serde_json::{json, Map, Value};

use super::view::round;

/// Days kept in the daily ring.
pub const DAILY_CAP: usize = 365;
/// Hours kept in the hourly ring.
pub const HOURLY_CAP: usize = 48;
/// An observation more than this many ticks after the previous one cannot
/// be ordinary play (a frame runs a few ticks, a remote `step` observes
/// every hour): the city was replaced by a load, so history starts over.
const JUMP_TICKS: u64 = 2 * 1440;

/// Daily series, in sample order. `day` is the 1-based day that just
/// began; the `*_yesterday` counters are the totals of the day before,
/// captured just before midnight.
pub const DAILY_KEYS: &[&str] = &[
    "day", "population", "households", "homes_total", "jobs_total", "jobs_filled", "treasury", "net_yesterday", "tax_yesterday",
    "gini", "avg_land_value", "mean_mood", "hungry", "unemployed", "unhoused", "waiting_immigrants",
    "businesses_open", "businesses_closed", "buildings", "under_construction", "power_served", "unpowered_buildings",
    "transit_riders_yesterday", "rail_riders_yesterday", "meals_sold_yesterday", "deals_yesterday",
    "emigrated_total", "displaced_total", "priced_out_total",
];

/// Hourly series: fast-moving numbers only.
pub const HOURLY_KEYS: &[&str] = &["day", "hour", "cars_on_road", "trucks", "buses", "hungry", "treasury", "waiting_for_transit", "unpowered_buildings"];

/// Today's running counters, refreshed on every observation so that the
/// last values before midnight survive the sim's daily reset.
#[derive(Default, Clone, Copy)]
struct Counters {
    transit_riders: u32,
    rail_riders: u32,
    meals_sold: u32,
    deals: u32,
}

impl Counters {
    fn read(w: &World) -> Self {
        Self { transit_riders: w.economy.transit.riders_today, rail_riders: w.rail.stats.riders_today, meals_sold: w.economy.market.meals_sold_today, deals: w.economy.market.deals_today }
    }
}

#[derive(Resource, Default)]
pub struct History {
    daily: VecDeque<Vec<f64>>,
    hourly: VecDeque<Vec<f64>>,
    /// Clock tick, day and hour index of the previous observation.
    last_tick: Option<u64>,
    last_day: u64,
    last_hour: u64,
    counters: Counters,
    /// Why the history last started over (startup or load), for the reply.
    started: String,
    started_tick: u64,
    /// `Sim::generation` the rings belong to.
    generation: u64,
}

impl History {
    /// Forget everything and take a first sample of the city as it is now.
    pub fn reset(&mut self, w: &World, why: &str) {
        *self = History { started: why.to_string(), started_tick: w.clock.tick, generation: self.generation, ..Default::default() };
        self.last_tick = Some(w.clock.tick);
        self.last_day = w.clock.day();
        self.last_hour = w.clock.tick / 60;
        self.counters = Counters::read(w);
        self.push_daily(w);
        self.push_hourly(w);
    }

    /// Start over if the city was replaced (loaded or a new game, from the
    /// API or the UI) since the rings were last reset: the old city's
    /// samples are not this one's.
    pub fn follow_city(&mut self, generation: u64, w: &World, why: &str) {
        if self.generation != generation {
            self.generation = generation;
            self.reset(w, why);
        }
    }

    /// Look at the city; record a sample if an hour or a day has turned
    /// since the last look. Cheap when nothing turned.
    pub fn observe(&mut self, w: &World) {
        let tick = w.clock.tick;
        let Some(last) = self.last_tick else {
            self.reset(w, "app start");
            return;
        };
        if tick < last || tick > last + JUMP_TICKS {
            self.reset(w, "city loaded");
            return;
        }
        if tick == last {
            return;
        }
        self.last_tick = Some(tick);
        let day = w.clock.day();
        if day != self.last_day {
            // `counters` still hold the last pre-midnight reading.
            self.last_day = day;
            self.push_daily(w);
        }
        self.counters = Counters::read(w);
        let hour = tick / 60;
        if hour != self.last_hour {
            self.last_hour = hour;
            self.push_hourly(w);
        }
    }

    fn push_daily(&mut self, w: &World) {
        let e = &w.economy;
        let n = w.citizens.len().max(1) as f64;
        let mood: f64 = w.citizens.values().map(|c| c.mood as f64).sum::<f64>() / n;
        let households = w.households.len();
        let (mut open, mut closed, mut built, mut sites) = (0u32, 0u32, 0u32, 0u32);
        for b in w.buildings.values() {
            if b.construction.is_some() {
                sites += 1;
            } else {
                built += 1;
            }
            if let Some(z) = &b.business {
                if z.closed {
                    closed += 1;
                } else {
                    open += 1;
                }
            }
        }
        let ledger_net: f32 = e.ledger.yesterday.iter().sum();
        let tax = e.ledger.yesterday_of(nd_sim::economy::Flow::IncomeTax) + e.ledger.yesterday_of(nd_sim::economy::Flow::BusinessTax);
        let c = self.counters;
        let row = vec![
            (w.clock.day() + 1) as f64,
            w.citizens.len() as f64,
            households as f64,
            e.homes_total as f64,
            e.jobs_total as f64,
            (e.jobs_total - e.jobs_free.min(e.jobs_total)) as f64,
            round(e.treasury),
            round(ledger_net),
            round(tax),
            round(e.gini),
            round(e.avg_land_value),
            (mood * 1000.0).round() / 1000.0,
            e.market.hungry as f64,
            e.unemployed as f64,
            e.unhoused as f64,
            e.waiting_immigrants as f64,
            open as f64,
            closed as f64,
            built as f64,
            sites as f64,
            round(e.power.served),
            e.power.unpowered_buildings as f64,
            c.transit_riders as f64,
            c.rail_riders as f64,
            c.meals_sold as f64,
            c.deals as f64,
            e.emigrated_total as f64,
            e.displaced_total as f64,
            e.priced_out_total as f64,
        ];
        debug_assert_eq!(row.len(), DAILY_KEYS.len());
        // One sample per day: a reset sample and the midnight sample of the
        // same day collapse into the later one.
        if self.daily.back().is_some_and(|r| r[0] == row[0]) {
            self.daily.pop_back();
        }
        self.daily.push_back(row);
        if self.daily.len() > DAILY_CAP {
            self.daily.pop_front();
        }
    }

    fn push_hourly(&mut self, w: &World) {
        let (mut cars, mut trucks, mut buses) = (0u32, 0u32, 0u32);
        for car in w.traffic.cars.values() {
            if car.bus.is_some() {
                buses += 1;
            } else if car.cargo.is_some() {
                trucks += 1;
            } else {
                cars += 1;
            }
        }
        let row = vec![
            (w.clock.day() + 1) as f64,
            w.clock.hour() as f64,
            cars as f64,
            trucks as f64,
            buses as f64,
            w.economy.market.hungry as f64,
            round(w.economy.treasury),
            (w.economy.transit.waiting + w.rail.stats.waiting) as f64,
            w.economy.power.unpowered_buildings as f64,
        ];
        if self.hourly.back().is_some_and(|r| r[0] == row[0] && r[1] == row[1]) {
            self.hourly.pop_back();
        }
        self.hourly.push_back(row);
        if self.hourly.len() > HOURLY_CAP {
            self.hourly.pop_front();
        }
    }

    /// The `history` reply: columnar series for the asked keys over the
    /// last `days` days (and all kept hours), plus a first/last/min/max
    /// summary per key so a reader sees the trend without scanning arrays.
    pub fn to_json(&self, keys: Option<&[String]>, days: Option<usize>) -> Result<Value, String> {
        if let Some(keys) = keys {
            for k in keys {
                if !DAILY_KEYS.contains(&k.as_str()) && !HOURLY_KEYS.contains(&k.as_str()) {
                    return Err(format!("unknown history key {k:?}; daily keys: {}; hourly keys: {}", DAILY_KEYS.join(", "), HOURLY_KEYS.join(", ")));
                }
            }
        }
        let wanted = |k: &str| k == "day" || k == "hour" || keys.is_none_or(|ks| ks.iter().any(|x| x == k));
        let skip = days.map_or(0, |d| self.daily.len().saturating_sub(d));
        let daily: Vec<&Vec<f64>> = self.daily.iter().skip(skip).collect();
        let hourly: Vec<&Vec<f64>> = self.hourly.iter().collect();
        Ok(json!({
            "note": "kept by the running app, not in saves: starts empty at app start and again on every load; daily samples are taken at the first observation after midnight (the *_yesterday counters cover the day before)",
            "started": { "why": self.started, "day": self.started_tick / 1440 + 1 },
            "daily": series(DAILY_KEYS, &daily, &wanted),
            "hourly": series(HOURLY_KEYS, &hourly, &wanted),
        }))
    }
}

/// `{samples, series: {key: [..]}, trend: {key: {first, last, change, min, max}}}`.
fn series(keys: &[&str], rows: &[&Vec<f64>], wanted: &dyn Fn(&str) -> bool) -> Value {
    let mut s = Map::new();
    let mut trend = Map::new();
    for (i, k) in keys.iter().enumerate() {
        if !wanted(k) {
            continue;
        }
        let col: Vec<f64> = rows.iter().map(|r| r[i]).collect();
        if *k != "day" && *k != "hour" && !col.is_empty() {
            let first = col[0];
            let last = col[col.len() - 1];
            let min = col.iter().copied().fold(f64::INFINITY, f64::min);
            let max = col.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            trend.insert(k.to_string(), json!({ "first": num(first), "last": num(last), "change": num(((last - first) * 100.0).round() / 100.0), "min": num(min), "max": num(max) }));
        }
        // Whole numbers as integers: counts read as counts.
        s.insert(k.to_string(), Value::Array(col.iter().map(|&x| num(x)).collect()));
    }
    json!({ "samples": rows.len(), "series": s, "trend": trend })
}

fn num(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9e15 { json!(x as i64) } else { json!(x) }
}
