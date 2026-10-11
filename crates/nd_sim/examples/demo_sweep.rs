//! Demo district over several seeds for twenty days: unemployment on day 8
//! and days 18-20 (`demo_city`'s borderline checks), to judge a change by
//! more than one seed (`demo_sweep [seed0] [n]`). Not part of the game.
use nd_minds::RuleBasedMind;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let s0: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let n: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let (mut d8s, mut lates) = (Vec::new(), Vec::new());
    for seed in s0..s0 + n {
        let mut w = World::new(nd_sim::WorldConfig { seed, ..nd_sim::WorldConfig::default() });
        w.drain_events();
        nd_sim::scenarios::demo_district(&mut w);
        let mut minds = RuleBasedMind::default();
        let (mut d8, mut late) = (0.0f32, 0.0f32);
        for day in 1..=20 {
            loop {
                w.tick(&mut minds);
                if w.clock.minute_of_day() == 0 {
                    break;
                }
            }
            let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1) as f32;
            let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count() as f32;
            if day == 8 {
                d8 = idle / adults;
            }
            if day >= 18 {
                late += idle / adults / 3.0;
            }
        }
        println!("seed {seed}: day 8 {:.1}%  days 18-20 {:.1}%  pop {}", d8 * 100.0, late * 100.0, w.citizens.len());
        d8s.push(d8);
        lates.push(late);
    }
    let avg = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32 * 100.0;
    println!("AVG day 8 {:.1}%  days 18-20 {:.1}%", avg(&d8s), avg(&lates));
}
