//! [K34] Time the sim on a save: load it, run `days` days of ticks, and
//! print the mean and worst tick, the daily and hourly passes, and (in the
//! containers build) the town's shipping containers. Not part of the game.
//!
//! `box_probe <save.ron> [days=2] [out.ron]` (the city saved at the end)
use nd_sim::mind::IdleMind;
use nd_sim::World;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let t = Instant::now();
    let mut w = World::read_save(&args[1]).expect("load");
    println!("load {:.2}s day {} pop {}", t.elapsed().as_secs_f32(), w.clock.day(), w.citizens.len());
    report(&w, "after load");
    let mut minds = IdleMind;
    let n = days * 1440;
    let mut times: Vec<f32> = Vec::with_capacity(n as usize);
    let mut minute: Vec<u64> = Vec::with_capacity(n as usize);
    let t0 = Instant::now();
    for k in 0..n {
        minute.push(w.clock.tick % 60);
        let t = Instant::now();
        w.tick(&mut minds);
        times.push(t.elapsed().as_secs_f32() * 1000.0);
        if (k + 1) % 1440 == 0 {
            println!("day {} done, {:.1}s so far", w.clock.day(), t0.elapsed().as_secs_f32());
            report(&w, "");
            report_r8(&w);
        }
    }
    let total: f32 = times.iter().sum();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p = |q: f32| sorted[((sorted.len() as f32 - 1.0) * q) as usize];
    println!("ticks {n}: mean {:.3} ms, median {:.3}, p95 {:.3}, p99 {:.3}, max {:.1} ms; total {:.1}s", total / n as f32, p(0.5), p(0.95), p(0.99), sorted.last().unwrap(), total / 1000.0);
    // The minute of the day each tick was: the hourly :17 and midnight.
    let at = |m: u64| times.iter().zip(&minute).filter(|(_, mm)| **mm == m).map(|(t, _)| *t).sum::<f32>() / (n / 60).max(1) as f32;
    println!("mean at :17 {:.3} ms, at :16 {:.3} ms", at(17), at(16));
    if let Some(out) = args.get(3) {
        let f = std::io::BufWriter::new(std::fs::File::create(out).expect("save file"));
        w.write_ron_gz(f).expect("save");
        println!("saved {out}");
    }
}

fn report(w: &World, what: &str) {
    let c = w.box_counts();
    let s = &w.containers.stats;
    println!("  boxes {what}: total {} (trains {}, lorries {}, stored {} ({} full), sites {}, homes {}, scrap {}); since: in rail/sea/road {:?}, delivered {}, to stacks {} (off lorry {}), homes built {}, out-of-town for homes {}, sent out {}, sold {}, scrapped {}, steel {:.1}, awaiting scrap {}; depots {}", c.total, c.on_trains, c.on_trucks, c.stored, c.stored_full, c.on_sites, c.in_homes, c.at_scrap, s.arrived_total, s.delivered_total, s.sold_to_stacks_total, s.sold_off_truck_total, s.homes_built_total, s.bought_out_of_town_for_homes, s.sent_out_total, s.sold_out_of_town_total, s.scrapped_total, s.steel_total, s.awaiting_scrap, w.box_depots().len());
}


/// [r8] The lorries at the depots' bays, the carriers' and cranes' moves,
/// the forwarders and the boxes going out.
#[allow(dead_code)]
fn report_r8(w: &World) {
    let c = w.box_counts();
    let s = &w.containers.stats;
    let in_bays: usize = w.containers.depots.values().map(|y| y.bays.iter().flatten().count()).sum();
    let queued: usize = w.containers.depots.values().map(|y| y.queue.len()).sum();
    let going: usize = w.containers.boxes.values().filter(|r| r.bound_out).count();
    println!("  r8: on cranes {}, aboard ships {}, lorries in bays {in_bays} queued {queued} jobs {}, crane moves {}, exported in boxes {}, sold for scrap {}, surplus out {}, going out {going}, export units yday {:.0}", c.lifting, c.on_ships, w.containers.lorries.len(), s.crane_moves_total, s.exported_total, s.sold_for_scrap_total, s.surplus_out_total, s.export_units_yday);
    for (b, f) in &w.containers.forwarders {
        println!("  forwarder {}: boxes out {}, loads {}, firms {}, units {:.0}, fees {:.0} (today {:.0}), rounds {}, waiting {}", w.buildings.get(*b).map(|x| x.name.clone()).unwrap_or_default(), f.boxes_out, f.loads_total, f.firms.len(), f.units_total, f.fees_total, f.fees_today, f.rounds, f.pending.len());
    }
    let fw = w.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::FreightForwarder)).count();
    println!("  forwarder buildings {fw}");
}
