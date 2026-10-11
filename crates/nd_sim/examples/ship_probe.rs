//! Headless freighter probe: a freighter's berthing and the quay crane's
//! work at the berth (`ship_probe <save.ron> [ticks]`). Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    for _ in 0..ticks {
        w.tick(&mut minds);
        for s in w.ports.ships.values().filter(|s| s.kind == nd_sim::ShipKind::Freighter) {
            let port = w.ports.ports.values().find(|p| p.building == Some(s.port));
            let work = port.and_then(|p| nd_sim::ports::crane_work(p, s, 0.5));
            if s.docked {
                println!("tick {} at {:?} dwell {} in {} out {} quay {:?} work {:?}", w.clock.tick, s.pose().0, s.dwell, s.boxes_in, s.boxes_out, s.quay.is_some(), work.map(|c| (c.n, c.of, c.carrying, c.landing)));
            }
        }
    }
}
