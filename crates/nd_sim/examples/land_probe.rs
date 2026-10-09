//! [land] Probe: the demo district (as `tests/demo_city.rs` lays it out)
//! for some days, printing the pressure grid, heights and the land market.
//!
//! cargo run --profile fast -p nd_sim --example land_probe -- 8

use nd_minds::RuleBasedMind;
use nd_sim::{World, WorldConfig};

fn demo_world() -> World {
    let mut world = World::new(WorldConfig::default());
    world.drain_events();
    nd_sim::scenarios::demo_district(&mut world);
    world
}

fn main() {
    let mut world = demo_world();
    let mut minds = RuleBasedMind::default();
    let days: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    for day in 1..=days {
        loop {
            world.tick(&mut minds);
            if world.clock.minute_of_day() == 0 {
                break;
            }
        }
        let p = &world.estate.pressure;
        let mut vals: Vec<f32> = world.lots.values().filter(|l| l.building.is_some()).map(|l| p.at(l.center)).collect();
        vals.sort_by(|a, b| a.total_cmp(b));
        let q = |f: f32| vals.get(((vals.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(0.0);
        let mut caps: Vec<u8> = world.lots.keys().filter(|l| world.lots[*l].building.is_some()).map(|l| world.height_cap(l)).collect();
        caps.sort();
        let mut floors = [0u32; 8];
        for b in world.buildings.values().filter(|b| b.construction.is_none()) {
            floors[(b.floors as usize / 3).min(7)] += 1;
        }
        let s = &world.estate.stats;
        let mob: f32 = p.mobility.iter().sum::<f32>() / p.mobility.len().max(1) as f32;
        let (pop, gini, ratio, ok) = world.elite_gate();
        println!(
            "day {day}: pop {pop} | pressure p10 {:.1} p50 {:.1} p90 {:.1} max {:.1} | cap p50 {} max {} | floors by 3s {:?} | mob {mob:.2} | buyouts {} merges {} branches {} mcm {} | gini {gini:.2} top/med {ratio:.1} gate {ok}",
            q(0.1),
            q(0.5),
            q(0.9),
            q(1.0),
            caps.get(caps.len() / 2).copied().unwrap_or(0),
            caps.last().copied().unwrap_or(0),
            floors,
            s.buyouts_total,
            s.merges_total,
            s.branches_total,
            s.mcmansions_total,
        );
    }
}
