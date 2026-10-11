//! Demo district, one seed: building sites, those waiting for parts, the
//! lorries queued and on the road, at 09:00 each day (`demo_trace [seed]`).
//! Not part of the game.
use nd_minds::RuleBasedMind;
use nd_sim::World;

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let mut w = World::new(nd_sim::WorldConfig { seed, ..nd_sim::WorldConfig::default() });
    w.drain_events();
    nd_sim::scenarios::demo_district(&mut w);
    let mut minds = RuleBasedMind::default();
    for _ in 0..(25 * 1440) {
        w.tick(&mut minds);
        if w.clock.minute_of_day() == 9 * 60 {
            let sites: Vec<_> = w.buildings.values().filter_map(|b| b.construction.as_ref()).collect();
            let waiting = sites.iter().filter(|c| c.waiting_for_materials()).count();
            let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1);
            let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
            let biz: Vec<_> = w.buildings.values().filter(|b| b.construction.is_none()).filter_map(|b| b.business.as_ref()).collect();
            let closed = biz.iter().filter(|z| z.closed).count();
            let reasons: std::collections::BTreeMap<String, usize> = w.buildings.values().filter_map(|b| b.demolition.as_ref()).fold(Default::default(), |mut m, d| { *m.entry(format!("{:?}", d.reason)).or_default() += 1; m });
            println!("day {:2}: sites {:3} waiting {:3} queued {:3} lorries {:3} unemployed {:.1}% biz {} closed {} demolitions {:?}", w.clock.day(), sites.len(), waiting, w.queued_trucks(), w.traffic.cars.values().filter(|c| c.cargo.is_some()).count(), idle as f32 / adults as f32 * 100.0, biz.len(), closed, reasons);
        }
    }
}
