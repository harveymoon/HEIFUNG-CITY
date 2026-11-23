//! [r14-foodtrucks] Probe: load a save and run it on day by day with its
//! food trucks: how many, where each went by the hour, meals sold, the
//! hungry fed, the owners' money; and the town's hunger each midnight.
//!
//! cargo run --profile fast -p nd_sim --example food_truck_probe -- saves/city.ron 3 [on|off] [sunny]
//!
//! `off` switches the trucks off (to compare the hunger without them);
//! `sunny` holds the weather warm and dry (24 °C, clear) every hour.
//! SAVE_TO=path writes the city at the end (for screenshots); with
//! SAVE_AT_HOUR=h it writes it at h o'clock of the last day instead and stops.
//! FOUND=ice_cream,taco starts those trucks at once (types by key).

use nd_sim::food_trucks::{SpotKind, TruckPlace};
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let off = args.iter().any(|a| a == "off");
    let sunny = args.iter().any(|a| a == "sunny");
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    w.food_trucks.disabled = off;
    if let Ok(list) = std::env::var("FOUND") {
        for key in list.split(',') {
            let k = nd_sim::food_trucks::TruckKind::ALL.into_iter().find(|k| k.key() == key).expect("truck type");
            let id = w.found_food_truck(k, None).expect("found");
            println!("founded {} ({key})", w.food_trucks.trucks[id].name);
        }
    }
    let save_at: Option<u64> = std::env::var("SAVE_AT_HOUR").ok().and_then(|h| h.parse::<u64>().ok());
    println!(
        "start: day {} {:02}:{:02} pop {} hungry(<0.25) {} trucks {} weather {:.1}C cloud {:.2} {}",
        w.clock.day(),
        w.clock.hour(),
        w.clock.minute_of_day() % 60,
        w.citizens.len(),
        w.citizens.values().filter(|c| c.satiety < 0.25).count(),
        w.food_trucks.trucks.len(),
        w.weather.temp_c,
        w.weather.cloud,
        w.weather.label()
    );
    let t0 = std::time::Instant::now();
    for d in 0..days {
        let mut hungry_hours = 0usize;
        let mut sat_sum = 0.0f64;
        let mut n_hours = 0usize;
        loop {
            w.tick(&mut minds);
            w.drain_events();
            let m = w.clock.minute_of_day();
            // Right after the hourly weather step (minute 0), before the
            // trucks' next look at the sky (minute 1).
            if sunny && m % 60 == 0 {
                w.weather.temp_c = 24.0;
                w.weather.cloud = 0.1;
            }
            if m == 6 * 60 {
                let by: Vec<String> = SpotKind::ALL.iter().map(|k| format!("{} {}", k.label(), w.food_trucks.spots.iter().filter(|s| s.kind == *k).count())).collect();
                println!("  day {} crowd spots: {}", w.clock.day(), by.join(", "));
            }
            if m == 6 * 60 && d == 0 {
                println!("  morning plan ({} crowd spots):", w.food_trucks.spots.len());
                for s in w.food_trucks.spots.iter().take(40) {
                    println!("    spot {:9} {:40} crowd {:6.0} {:02}-{:02} at ({:.0},{:.0})", s.kind.label(), s.name, s.crowd, s.from, s.to, s.at.x, s.at.y);
                }
            }
            if m == 6 * 60 {
                for t in w.food_trucks.trucks.values() {
                    let plan: Vec<String> = t.plan.iter().map(|s| format!("{:02}-{:02} {} ({})", s.from, s.to, s.name, s.kind.label())).collect();
                    println!("  plan  {:28} {:14} {}", t.name, t.kind.label(), plan.join(" | "));
                }
            }
            if m % 60 == 30 {
                let h = w.citizens.values().filter(|c| c.satiety < 0.25).count();
                hungry_hours += h;
                sat_sum += w.citizens.values().map(|c| c.satiety as f64).sum::<f64>() / w.citizens.len().max(1) as f64;
                n_hours += 1;
                let open: Vec<String> = w
                    .food_trucks
                    .trucks
                    .values()
                    .filter(|t| t.place.open())
                    .map(|t| format!("{} @ {} [{} q, {} sold]", t.name, t.visits.last().map(|v| v.name.as_str()).unwrap_or("?"), 0, t.meals_today))
                    .collect();
                let road = w.food_trucks.trucks.values().filter(|t| matches!(t.place, TruckPlace::Road(_))).count();
                if !open.is_empty() || road > 0 {
                    println!("  {:02}:30 open {} on road {} queuing {}: {}", w.clock.hour(), open.len(), road, w.food_trucks.trips.len(), open.join("; "));
                }
            }
            if m == 1439 {
                break;
            }
            if save_at.is_some_and(|h| d + 1 == days && m == h * 60) {
                let out = std::env::var("SAVE_TO").expect("SAVE_TO with SAVE_AT_HOUR");
                std::fs::write(&out, w.to_save_ron().expect("save")).expect("write");
                println!("saved at {:02}:00 to {out}", w.clock.hour());
                return;
            }
        }
        let s = &w.food_trucks.stats;
        let by: Vec<String> = SpotKind::ALL.iter().map(|k| format!("{} {}", k.label(), s.meals_by_spot.get(k.index()).copied().unwrap_or(0))).collect();
        println!(
            "day {} done: trucks {} meals {} takings {:.0} fed-hungry {} too-cold {} | by spot: {} | hungry(<0.25) avg/hour {:.0} mean satiety {:.3} | food_need.hungry {} ({:.1}%) | {:.0}s",
            w.clock.day(),
            w.food_trucks.trucks.len(),
            s.meals_today,
            s.takings_today,
            s.fed_hungry_today,
            s.too_cold_today,
            by.join(", "),
            hungry_hours as f64 / n_hours.max(1) as f64,
            sat_sum / n_hours.max(1) as f64,
            w.food_need.hungry,
            w.food_need.share * 100.0,
            t0.elapsed().as_secs_f32()
        );
        for t in w.food_trucks.trucks.values() {
            let money = w.households.get(t.household).map_or(0.0, |h| h.money);
            let visits: Vec<String> = t.visits.iter().map(|v| format!("{} {:02}:{:02}-{} {} meals", v.name, (v.opened % 1440) / 60, v.opened % 60, v.closed.map_or("open".into(), |c| format!("{:02}:{:02}", (c % 1440) / 60, c % 60)), v.meals)).collect();
            let skipped: Vec<String> = t.plan.iter().filter_map(|s| match &s.state {
                nd_sim::food_trucks::StintState::Skipped(why) => Some(format!("{} ({why})", s.name)),
                _ => None,
            }).collect();
            println!("  truck {:28} {:14} meals {:3} takings {:6.0} costs {:6.0} household ${:7.0} stock {:4.0} | {} {}", t.name, t.kind.label(), t.meals_today, t.takings_today, t.costs_today, money, t.stock, visits.join(" | "), if skipped.is_empty() { String::new() } else { format!("| skipped: {}", skipped.join(", ")) });
        }
    }
    if let Ok(out) = std::env::var("SAVE_TO") {
        std::fs::write(&out, w.to_save_ron().expect("save")).expect("write");
        println!("saved to {out}");
    }
}
