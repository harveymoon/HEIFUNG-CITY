//! [r4-flags-sim] Headless count of big ships (ferries, car ferries,
//! freighters) lying on top of each other: per tick, pairs closer than
//! half their summed lengths, split into at a berth (either docked or
//! within 60 m of a pier) and out on the water; and the time ferries hold
//! off a busy berth. `ship_overlap <save.ron> [ticks]`. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{ShipKind, World};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let (mut at_berth, mut on_water, mut ship_ticks, mut held) = (0u32, 0u32, 0u32, 0u32);
    let mut shown = 0;
    for _ in 0..ticks {
        w.tick(&mut minds);
        let berths: Vec<_> = w.ports.ports.values().map(|p| p.berth).collect();
        let big: Vec<_> = w.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Ferry | ShipKind::CarFerry | ShipKind::Freighter)).collect();
        ship_ticks += big.len() as u32;
        held += big.iter().filter(|s| s.s == s.prev_s && s.dwell == 0).count() as u32;
        for i in 0..big.len() {
            for j in i + 1..big.len() {
                let (a, b) = (big[i], big[j]);
                let (pa, pb) = (a.pose().0, b.pose().0);
                if pa.distance(pb) < 0.5 * (a.length() + b.length()) {
                    let near_pier = berths.iter().any(|q| q.distance(pa) < 60.0 || q.distance(pb) < 60.0);
                    if a.docked || b.docked || near_pier { at_berth += 1 } else { on_water += 1 }
                    if shown < 6 {
                        shown += 1;
                        println!("  tick {} {:?} at ({:.0},{:.0}) docked {} / {:?} at ({:.0},{:.0}) docked {} near_pier {near_pier}", w.clock.tick, a.kind, pa.x, pa.y, a.docked, b.kind, pb.x, pb.y, b.docked);
                    }
                }
            }
        }
    }
    println!("ship-ticks {ship_ticks}; overlapping pair-ticks at a berth {at_berth}, on the water {on_water}; ship-ticks held (not moving, not dwelling) {held}");
}
