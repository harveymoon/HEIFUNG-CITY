//! [w1-roads] Run a citygen city a few days and say why vehicles stand at
//! the end: `w1_stall_probe <preset> <seed> [days]`. Not part of the game.
use nd_minds::RuleBasedMind;
use nd_procgen::citygen::{self, CityPreset};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preset = match args.get(1).map(|s| s.as_str()).unwrap_or("grid_plaza") {
        "coastal_river" => CityPreset::CoastalRiver,
        "old_town" => CityPreset::OldTown,
        "twin_centres" => CityPreset::TwinCentres,
        "perfect_city" => CityPreset::PerfectCity,
        _ => CityPreset::GridPlaza,
    };
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let days: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
    let (mut w, _, _) = citygen::generate_world(preset, seed);
    let mut minds = RuleBasedMind::default();
    for day in 1..=days {
        for t in 0..1440 {
            w.tick(&mut minds);
            if day == days && t % 120 == 0 {
                let t2 = &w.traffic;
                let st: Vec<_> = t2.cars.iter().filter(|(_, c)| c.v < 0.5 && c.stalled > 120.0).map(|(id, _)| id).collect();
                let mut why = std::collections::BTreeMap::new();
                for &id in &st {
                    *why.entry(t2.stop_reason(id).key()).or_insert(0) += 1;
                }
                println!("day {day} t {t}: cars {} stalled {} {:?}", t2.cars.len(), st.len(), why);
                let mut heads = std::collections::BTreeMap::new();
                for &id in &st {
                    let ch = t2.jam_chain(id);
                    if let Some(h) = ch.head() {
                        let e = heads.entry(h).or_insert((0usize, false));
                        e.0 = e.0.max(ch.cars.len());
                        e.1 |= ch.cycle;
                    }
                }
                for (h, (n, cyc)) in heads.iter().filter(|(_, (n, _))| *n >= 3) {
                    let c = &t2.cars[*h];
                    if t2.stop_reason(*h).key() == "red_light" {
                        let lane = &t2.net.lanes[c.route[c.idx] as usize];
                        if let Some(&k) = t2.net.node_intersection.get(lane.end_node.0 as usize) {
                            if let Some(i) = t2.net.intersections.get(k) {
                                let inc: Vec<_> = i.incoming.iter().filter(|(l, _)| *l == c.route[c.idx]).collect();
                                let last = lane.cars.last().map(|&l| (l, t2.cars[l].s, t2.cars[l].v));
                                println!("   LANE len {:.1} car s {:.1} idx {} lane cars {} last {:?} stop_line {:?}", lane.length, c.s, c.idx, lane.cars.len(), last, t2.peds.stop_lines.get(&c.route[c.idx]));
                                println!("   INTER {k} phases {:?} phase {} timer {:.1} greens {:?} arrow {:.1} green() {:.1} incoming-of-car {:?} walk {:?}", i.phases, i.phase, i.timer, i.greens, i.arrow, i.green(), inc, t2.peds.walk.get(k));
                            }
                        }
                    }
                    println!("   HEAD {h:?} queue {n}{} at ({:.0},{:.0}) v {:.1} stalled {:.0} -> {}", if *cyc { " CYCLE" } else { "" }, c.pos.x, c.pos.y, c.v, c.stalled, t2.stop_reason(*h).text(|o| format!("{o:?} at ({:.0},{:.0})", t2.cars.get(o).map_or(0.0, |x| x.pos.x), t2.cars.get(o).map_or(0.0, |x| x.pos.y))));
                }
            }
        }
    }
}
