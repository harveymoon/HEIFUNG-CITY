//! [freight] Freight probe: the demo district run headless with its
//! freight yard, printing the rail import queue (orders, units, oldest
//! wait), goods aboard trains, exports on the apron and freight trains,
//! hourly, and a summary of the longest wait. Used to check that goods
//! riding the trains do not starve the town.
//!
//! cargo run --profile fast --example freight_probe -p nd_sim -- [days]

use nd_minds::RuleBasedMind;
use nd_sim::{World, WorldConfig};

/// The same layout as `Sim::new` + `Sim::seed_demo_district` in nd_app.
pub fn demo_world() -> World {
    let mut world = World::new(WorldConfig::default());
    world.drain_events();
    nd_sim::scenarios::demo_district(&mut world);
    world
}


fn main() {
    let days: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut w = demo_world();
    let mut minds = RuleBasedMind::default();
    let (mut worst_wait, mut worst_units, mut worst_apron, mut trains) = (0u64, 0.0f32, 0.0f32, 0u32);
    let mut in_total = 0.0f32;
    let mut last_in = 0.0f32;
    for t in 0..days * 1440 {
        w.tick(&mut minds);
        let tick = w.clock.tick;
        let queued: f32 = w.rail.orders.iter().map(|o| o.cargo.amount).sum();
        let oldest = w.rail.orders.iter().map(|o| tick.saturating_sub(o.since)).max().unwrap_or(0);
        let aboard: f32 = w.rail.trains.values().flat_map(|t| t.cargo.iter()).map(|c| c.amount).sum();
        let apron: f32 = w.buildings.values().map(|b| b.rail_outbound).sum();
        worst_wait = worst_wait.max(oldest);
        worst_units = worst_units.max(queued);
        worst_apron = worst_apron.max(apron);
        let fin = w.rail.stats.freight_in_today;
        if fin < last_in {
            in_total += last_in;
        }
        last_in = fin;
        if t % 60 == 59 {
            trains = trains.max(w.rail.stats.freight_trains_today);
            println!("day {} {:02}:00 orders {:3} ({:6.0} u) oldest {:4} min | aboard {:6.0} | apron {:7.0} | freight trains today {:2} | in today {:7.0} | sitting {}",
                w.clock.day(), (tick / 60) % 24, w.rail.orders.len(), queued, oldest, aboard, apron, w.rail.stats.freight_trains_today, fin,
                w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Sitting { .. })).count());
        }
    }
    in_total += last_in;
    println!("SUMMARY longest wait {worst_wait} min, most waiting {worst_units:.0} u, biggest apron {worst_apron:.0} u, most freight trains a day {trains}, brought in {in_total:.0} u");
}
