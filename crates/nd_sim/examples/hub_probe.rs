//! [r12-hunger] Probe: the cargo hubs' import pipeline. For each hub, the
//! loads it expects by rail or sea (count, units, by way, age) against the
//! loads really on their way to it, every few hours.
//!
//! cargo run --profile fast -p nd_sim --example hub_probe -- saves/city.ron 24

use nd_sim::World;

fn report(w: &World) {
    let now = w.clock.tick;
    println!("day {} {:02}:00", w.clock.day(), w.clock.hour());
    for (&h, st) in &w.cargo_hubs.hubs {
        let name = w.buildings.get(h).map_or("(gone)".to_string(), |b| b.name.clone());
        let ages: Vec<u64> = st.inbound.iter().map(|l| now.saturating_sub(l.since) / 60).collect();
        let units: f32 = st.inbound.iter().map(|l| l.amount).sum();
        let mut vias = std::collections::BTreeMap::new();
        for l in &st.inbound {
            *vias.entry(format!("{:?}", l.via)).or_insert(0) += 1;
        }
        // Loads on their way to the hub.
        let mut on_way = std::collections::BTreeMap::new();
        let mut n = |k: &'static str| *on_way.entry(k).or_insert(0) += 1;
        for (_, c) in &w.ports.inbound {
            if c.to == h {
                n("ports.inbound");
            }
        }
        for s in w.ports.ships.values() {
            for c in &s.cargo {
                if c.to == h {
                    n("ship");
                }
            }
        }
        for o in &w.rail.orders {
            if o.cargo.to == h {
                n("rail order");
            }
        }
        for t in w.rail.trains.values() {
            for c in &t.cargo {
                if c.to == h {
                    n("train");
                }
            }
        }
        for q in &w.freight_queue {
            if q.cargo.to == h {
                n("queued lorry");
            }
        }
        for c in w.traffic.cars.values() {
            if c.cargo.is_some_and(|c| c.to == h && c.amount > 0.0) {
                n("lorry");
            }
        }
        for j in w.containers.lorries.values() {
            if j.cargo.is_some_and(|c| c.to == h) {
                n("box lorry job");
            }
        }
        println!(
            "  {name}: inbound {} loads {units:.0} units {vias:?} age h min {} max {} | waiting {} trunk {} shed {} outbound {:.0} | on the way to it: {on_way:?}",
            st.inbound.len(),
            ages.iter().min().unwrap_or(&0),
            ages.iter().max().unwrap_or(&0),
            st.waiting.len(),
            st.trunk.len(),
            st.ready.len(),
            st.outbound,
        );
    }
    let s = &w.cargo_hubs.stats;
    println!("  hubs received today {} ({:.0} units), forwarded {}", s.received_today, s.received_units_today, s.forwarded_today);
    for (pid, p) in w.ports.ports.iter() {
        if p.kind == nd_sim::PortKind::ShippingPort {
            let waiting = w.ports.inbound.iter().filter(|(b, _)| Some(*b) == p.building).count();
            println!("  port {pid:?} building {:?}: last sailing {} h ago, {} loads waiting for a ship, ships out {}", p.building, now.saturating_sub(p.last_sailing) / 60, waiting, w.ports.ships.len());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(args.get(1).expect("save")).expect("load");
    let hours: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24);
    let mut minds = nd_minds::RuleBasedMind::default();
    report(&w);
    for h in 1..=hours {
        for _ in 0..60 {
            w.tick(&mut minds);
            w.drain_events();
        }
        if h % 6 == 0 {
            report(&w);
        }
    }
}
