//! [r5-trash] Load a save, run a few days, print trash numbers per day.
use nd_sim::mind::IdleMind;
fn main() {
    let path = std::env::args().nth(1).expect("save path");
    let days: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mut w = nd_sim::World::read_save(&path).unwrap();
    let mut minds = IdleMind;
    for d in 0..days {
        for _ in 0..1440 {
            w.tick(&mut minds);
        }
        let t = &w.society.trash;
        let over = t.bins.values().filter(|b| b.overflow() > 0.0).count();
        let unemployed = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
        println!("day+{} pop {} unemployed_adults {} treasury {:.0} produced_y {:.0} collected_y {:.0} overflowing {} complaints_y {}", d + 1, w.citizens.len(), unemployed, w.economy.treasury, t.stats.produced_yesterday, t.stats.collected_city_yesterday, over, t.stats.complaints_yesterday);
    }
}
