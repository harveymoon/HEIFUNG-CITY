//! [r4-hunger] Probe: load a save and run it on day by day, with the
//! town's food each day: people, the hungry, who left over hunger, the
//! grocers (open, stock, takings) and, at 20:00, what the hungry are doing
//! and remember.
//!
//! cargo run --profile fast -p nd_sim --example hunger_week -- saves/city.ron 7
//! QUIET=1 leaves out the 20:00 breakdown.

use nd_sim::citizens::{Activity, Purpose};
use nd_sim::market::Resource;
use nd_sim::{BusinessKind, World};
use nd_sim::citizens::CitizenId;
use std::collections::{BTreeMap, HashMap};

fn norm(s: &str) -> String {
    let s = s.split(" at ").next().unwrap_or(s);
    let s = s.split(" was ").next().unwrap_or(s);
    s.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect()
}

fn grocers(w: &World) -> (usize, usize, f32, f32, usize) {
    let (mut open, mut stocked, mut stock, mut takings, mut staffed) = (0, 0, 0.0, 0.0, 0);
    for b in w.buildings.values() {
        let Some(z) = &b.business else { continue };
        if z.kind != BusinessKind::Grocery || z.closed || !b.operating() {
            continue;
        }
        open += 1;
        let s = z.stock_of(Resource::Groceries);
        stock += s;
        if s >= 1.0 {
            stocked += 1;
        }
        if !b.workers.is_empty() {
            staffed += 1;
        }
        takings += z.revenue_today;
    }
    (open, stocked, stock, takings, staffed)
}

/// Per person, sampled every 10 minutes: when last seen at home, when the
/// trip under way started, and where they stood an hour ago.
#[derive(Default, Clone, Copy)]
struct Seen {
    home: u64,
    trip: Option<u64>,
    hour_ago: glam::Vec2,
}

fn on_the_move(a: &Activity) -> bool {
    !matches!(a, Activity::AtHome | Activity::Working | Activity::Visiting(_) | Activity::Sitting { .. } | Activity::Idle)
}

fn sample(w: &World, seen: &mut HashMap<CitizenId, Seen>, hourly: bool) {
    let t = w.clock.tick;
    for (id, c) in w.citizens.iter() {
        let s = seen.entry(id).or_insert(Seen { home: t, trip: None, hour_ago: c.pos });
        if matches!(c.activity, Activity::AtHome) {
            s.home = t;
        }
        if on_the_move(&c.activity) {
            s.trip.get_or_insert(t);
        } else {
            s.trip = None;
        }
        if hourly {
            s.hour_ago = c.pos;
        }
    }
}

fn hours(n: u64) -> &'static str {
    match n / 60 {
        0..=1 => "<2h",
        2..=5 => "2-6h",
        6..=11 => "6-12h",
        12..=23 => "12-24h",
        _ => ">=24h",
    }
}

fn breakdown(w: &World, seen: &HashMap<CitizenId, Seen>) {
    let mut by: BTreeMap<String, u32> = BTreeMap::new();
    let mut mem: BTreeMap<String, u32> = BTreeMap::new();
    let hungry: Vec<_> = w.citizens.iter().filter(|(_, c)| c.satiety < 0.25).collect();
    let t = w.clock.tick;
    for (id, c) in &hungry {
        if let Some(s) = seen.get(id) {
            *by.entry(format!("last at home {}", hours(t - s.home))).or_default() += 1;
            if let Some(t0) = s.trip {
                *by.entry(format!("on the move for {}", hours(t - t0))).or_default() += 1;
                *by.entry(format!("on the move, mode {:?}, moved in the hour {}", c.mode, if c.pos.distance(s.hour_ago) < 5.0 { "<5m" } else { ">=5m" })).or_default() += 1;
            }
        }
        let hh = c.household.and_then(|h| w.households.get(h));
        let pantry = hh.map_or(c.groceries, |h| h.groceries);
        let money = hh.map_or(c.savings, |h| h.money + c.savings.max(0.0));
        let mut k = |s: String| *by.entry(s).or_default() += 1;
        k(format!("household {}", hh.is_some()));
        k(format!("has a home {}{}", c.home.is_some(), if c.home.is_none() { if hh.is_some_and(|h| w.society.homeless.bed_of(c.household.unwrap()).is_some()) { ", a bed in a camp or RV" } else { ", no bed" } } else { "" }));
        k(format!("pantry {}", if pantry < 1.0 { "empty" } else { "some" }));
        k(format!("money {}", if money < 0.0 { "<0" } else if money < 50.0 { "<50" } else { ">=50" }));
        k(format!("stage {:?}", c.stage()));
        let act = match &c.activity {
            Activity::Traveling { purpose, .. } => format!("traveling {purpose:?}"),
            Activity::Driving { purpose, .. } => format!("driving {purpose:?}"),
            a => format!("{a:?}").split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or("").to_string(),
        };
        k(format!("activity {act}"));
        if matches!(c.activity, Activity::AtHome) && pantry < 1.0 {
            k(format!("at home, empty pantry, asleep {}", c.asleep));
        }
        if let Activity::Traveling { purpose, path, idx, .. } = &c.activity {
            let left: f32 = path.windows(2).skip(*idx).map(|p| p[0].distance(p[1])).sum();
            let purpose = if matches!(purpose, Purpose::Home) { "home" } else { "elsewhere" };
            k(format!("on foot/bike {purpose}, left {}", if left < 500.0 { "<500m" } else if left < 1500.0 { "<1.5km" } else if left < 3000.0 { "<3km" } else { ">=3km" }));
        }
        k(format!("hungry_days {}", c.hungry_days));
        if matches!(c.activity, Activity::Idle | Activity::Sitting { .. }) || matches!(c.activity, Activity::Traveling { purpose: Purpose::Wander, .. }) {
            let th: String = c.thought.chars().map(|ch| if ch.is_ascii_digit() { '#' } else { ch }).take(60).collect();
            *mem.entry(format!("THOUGHT {}: {th}", format!("{:?}", c.activity).split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or(""))).or_default() += 1;
            k(format!("idle/sit/wander: next decision in {}", match c.next_decision_tick.saturating_sub(w.clock.tick) { 0 => "0", 1..=60 => "<1h", 61..=600 => "1-10h", _ => ">10h" }));
        }
        for m in c.memories.iter().rev().take(3) {
            *mem.entry(norm(&m.text)).or_default() += 1;
        }
    }
    println!("  20:00 hungry (satiety<0.25): {}", hungry.len());
    for (k, v) in &by {
        println!("    {k}: {v}");
    }
    let mut mem: Vec<_> = mem.into_iter().collect();
    mem.sort_by(|a, b| b.1.cmp(&a.1));
    println!("    memories (last 3 each):");
    for (k, v) in mem.iter().take(30) {
        println!("      {v:5}  {k}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(7);
    let quiet = std::env::var("QUIET").is_ok();
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    let pop0 = w.citizens.len();
    let (open, stocked, stock, _, _) = grocers(&w);
    println!("start: day {} {:02}:{:02} pop {pop0} hungry {} grocers {open} stocked {stocked} stock {stock:.0}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.values().filter(|c| c.satiety < 0.25).count());
    let mut left0 = w.economy.emigrated_by;
    let (mut hungry_sum, mut hungry_n, mut hungry_max) = (0usize, 0usize, 0usize);
    let mut total_hunger = 0u32;
    let mut seen: HashMap<CitizenId, Seen> = HashMap::new();
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        loop {
            w.tick(&mut minds);
            w.drain_events();
            let m = w.clock.minute_of_day();
            if m == 20 * 60 && !quiet {
                breakdown(&w, &seen);
            }
            if m % 10 == 0 {
                sample(&w, &mut seen, m % 60 == 0);
            }
            if m % 60 == 0 {
                let h = w.citizens.values().filter(|c| c.satiety < 0.25).count();
                hungry_sum += h;
                hungry_n += 1;
                hungry_max = hungry_max.max(h);
            }
            if m == 1439 {
                break;
            }
        }
        // The day's books, read the minute before midnight clears them.
        let e = &w.economy;
        let left: Vec<u32> = (0..left0.len()).map(|i| e.emigrated_by[i] - left0[i]).collect();
        left0 = e.emigrated_by;
        total_hunger += left[0];
        let (open, stocked, stock, takings, staffed) = grocers(&w);
        let mk = &e.market;
        let pantry_empty = w.households.values().filter(|h| h.groceries < 1.0).count();
        let unhoused: Vec<_> = w.households.iter().filter(|(_, h)| h.home.is_none() && !h.members.is_empty()).collect();
        let unhoused_people: usize = unhoused.iter().map(|(_, h)| h.members.len()).sum();
        let bedded = unhoused.iter().filter(|(id, _)| w.society.homeless.bed_of(*id).is_some()).count();
        println!("  unhoused: {} households ({unhoused_people} people), {bedded} with a camp/RV bed", unhoused.len());
        println!(
            "day {}: pop {} | hunger left {} (rent {} crime {} homeless {}) | hungry avg {} max {} now {} hungry_days>=1 {} | grocers {open} staffed {staffed} stocked {stocked} stock {stock:.0} takings {takings:.0} | meals sold {} grocery imports {:.0} turned away {} | pantries empty {pantry_empty}/{} | {:.0}s",
            w.clock.day(),
            w.citizens.len(),
            left[0],
            left[1],
            left[2],
            left[3],
            hungry_sum / hungry_n.max(1),
            hungry_max,
            w.citizens.values().filter(|c| c.satiety < 0.25).count(),
            w.citizens.values().filter(|c| c.hungry_days >= 1).count(),
            mk.meals_sold_today,
            mk.grocery_imports_today,
            mk.turned_away_today,
            w.households.len(),
            t0.elapsed().as_secs_f32()
        );
        (hungry_sum, hungry_n, hungry_max) = (0, 0, 0);
        // Over midnight into the next day.
        w.tick(&mut minds);
        w.drain_events();
    }
    println!("end: pop {pop0} -> {} ({:+}), left over hunger {total_hunger}", w.citizens.len(), w.citizens.len() as i64 - pop0 as i64);
}
