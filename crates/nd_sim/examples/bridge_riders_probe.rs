//! [bridge-riders] Load a save and count, tick by tick, the cars braking
//! for people (`pedestrians` / `jaywalker`) and how many of them brake at a
//! crossing a footbridge replaced (closed), with who is "on" those
//! crossings (walkers or riders, up on a raised leg or not). Not part of the
//! game.
//! `bridge_riders_probe <save> [ticks] [x,y,r]`  (START_MIN=<minute of day>)
use glam::Vec2;
use nd_minds::{HybridMind, MockBackend};
use nd_sim::citizens::{Activity, TravelMode};
use nd_sim::World;
use std::collections::BTreeMap;

/// The first occupied crossing on a car's next lanes (as `yield_reason`
/// finds it): (crossing, marked).
fn crossing_ahead(w: &World, id: nd_sim::CarId) -> Option<(u32, bool)> {
    let t = &w.traffic;
    let c = t.cars.get(id)?;
    let mut base = -c.s;
    for k in c.idx..(c.idx + 3).min(c.route.len()) {
        let l = c.route[k];
        for (marked, m) in [(true, &t.peds.holds), (false, &t.peds.watch)] {
            if let Some(h) = m.get(&l).and_then(|hs| hs.iter().find(|h| base + h.s >= -0.5 && base + h.s < 40.0)) {
                return Some((h.crossing, marked));
            }
        }
        base += t.net.lanes[l as usize].length;
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let area: Option<(Vec2, f32)> = args.get(3).map(|s| {
        let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        (Vec2::new(v[0], v[1]), v[2])
    });
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    if let Some(m) = std::env::var("START_MIN").ok().and_then(|s| s.parse::<u64>().ok()) {
        let mut n = 0;
        while w.clock.minute_of_day() != m && n < 200_000 {
            w.tick(&mut minds);
            n += 1;
        }
        eprintln!("skipped {n} ticks to minute {m}");
    }
    let closed = |w: &World, cr: u32| w.walk.crossings.get(cr as usize).is_some_and(|c| c.closed);
    println!("day {} minute {} cars {} footbridges {}", w.clock.day(), w.clock.minute_of_day(), w.traffic.cars.len(), w.walk.footbridges.len());
    let mut braking = 0u64;
    let mut at_bridged = 0u64;
    let mut standing_bridged = 0u64;
    let mut occ_bridged: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut by_key: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut sample: Vec<String> = Vec::new();
    let mut per_cr: BTreeMap<u32, u64> = BTreeMap::new();
    for i in 0..ticks {
        w.tick(&mut minds);
        for (id, c) in w.traffic.cars.iter() {
            if area.is_some_and(|(p, r)| c.pos.distance(p) > r) {
                continue;
            }
            let r = w.traffic.stop_reason(id);
            let key = r.key();
            if key != "pedestrians" && key != "jaywalker" {
                continue;
            }
            braking += 1;
            *by_key.entry(key).or_default() += 1;
            if let Some((cr, _)) = crossing_ahead(&w, id).filter(|&(cr, _)| closed(&w, cr)) {
                at_bridged += 1;
                *per_cr.entry(cr).or_default() += 1;
                if c.v < 0.5 {
                    standing_bridged += 1;
                }
                if sample.len() < 12 && i % 20 == 0 {
                    sample.push(format!("tick +{i} car {id:?} at ({:.1},{:.1}) v {:.1} {key} crossing {cr} -> {}", c.pos.x, c.pos.y, c.v, r.text(|o| format!("{o:?}"))));
                }
            }
        }
        for &(cr, who) in &w.peds.occupants {
            if !closed(&w, cr) {
                continue;
            }
            let Some(c) = w.citizens.get(who) else { continue };
            let raised = match &c.activity {
                Activity::Traveling { path, idx, .. } if *idx > 0 && *idx < path.len() => w.walk.level_at_node(path[*idx - 1]) > 0.0 || w.walk.level_at_node(path[*idx]) > 0.0,
                _ => false,
            };
            let k = match (c.mode, raised) {
                (TravelMode::Bike, true) => "bike on raised leg",
                (TravelMode::Bike, false) => "bike at grade",
                (_, true) => "walker on raised leg",
                (_, false) => "walker at grade",
            };
            *occ_bridged.entry(k).or_default() += 1;
            if std::env::var("DIAG").is_ok() && k == "bike at grade" && i % 50 == 0 {
                if let Activity::Traveling { path, idx, .. } = &c.activity {
                    let cx = &w.walk.crossings[cr as usize];
                    let fb = w.walk.footbridges.iter().find(|f| f.crossing == cr);
                    let from = if *idx > 0 { path[*idx - 1] } else { c.pos };
                    let to = path.get(*idx).copied().unwrap_or(c.pos);
                    let near: Vec<String> = path.iter().skip(idx.saturating_sub(3)).take(7).map(|p| format!("({:.1},{:.1}){}", p.x, p.y, if w.walk.level_at_node(*p) > 0.0 { "^" } else { "" })).collect();
                    println!("DIAG tick {i} rider {who:?} at ({:.1},{:.1}) leg ({:.1},{:.1})->({:.1},{:.1}) crossing {cr} mid ({:.1},{:.1}) kind {:?} ring {} ramp {} bridge joined {:?} path {}", c.pos.x, c.pos.y, from.x, from.y, to.x, to.y, cx.mid.x, cx.mid.y, cx.kind, cx.ring, cx.ramp, fb.map(|f| f.joined), near.join(" "));
                }
            }
        }
    }
    println!("over {ticks} ticks (car-ticks): braking for people {braking} {by_key:?}; at a bridged crossing {at_bridged} (standing {standing_bridged})");
    println!("occupant-ticks on bridged crossings: {occ_bridged:?}");
    let mut top: Vec<_> = per_cr.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("car-ticks at bridged crossings, by crossing: {:?}", top.iter().take(12).map(|(c, n)| { let x = &w.walk.crossings[*c as usize]; format!("{c} ({:.0},{:.0}) {:?} ring {}: {n}", x.mid.x, x.mid.y, x.kind, x.ring) }).collect::<Vec<_>>());
    for s in sample {
        println!("  {s}");
    }
}
