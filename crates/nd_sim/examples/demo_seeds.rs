//! The demo district run for eight days at several seeds: unemployment,
//! broke businesses and displaced households at day 8. Not part of the game.
use nd_minds::RuleBasedMind;
use nd_sim::{World, WorldConfig};

fn main() {
    let seeds: Vec<u64> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let seeds = if seeds.is_empty() { (1..=6).collect() } else { seeds };
    let mut sum = 0.0;
    for &seed in &seeds {
        let mut w = World::new(WorldConfig { seed, ..WorldConfig::default() });
        w.drain_events();
        nd_sim::scenarios::demo_district(&mut w);
        let order: Vec<(i32, i32)> = w.lots.values().take(6).map(|l| (l.center.x as i32, l.center.y as i32)).collect();
        let mut minds = RuleBasedMind::default();
        for _ in 0..8 {
            loop {
                w.tick(&mut minds);
                if w.clock.minute_of_day() == 0 {
                    break;
                }
            }
        }
        let adults = w.citizens.values().filter(|c| c.can_work()).count();
        let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
        let u = idle as f32 / adults.max(1) as f32;
        sum += u;
        println!("seed {seed}: pop {} unemployed {:.1}% displaced {} homes {} jobs {} first lots {:?}", w.citizens.len(), u * 100.0, w.economy.displaced_total, w.economy.homes_total, w.economy.jobs_total, order);
    }
    println!("mean unemployment {:.1}%", sum / seeds.len() as f32 * 100.0);
}
