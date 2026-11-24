//! [r15-lots] Why lots are landlocked, slivers or bare. Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example lots_probe -- <mode> <save.ron> [args]`
//! - `stats <save>`: landlocked lots by reason, the empty landlocked area,
//!   how many still name a street, sliver lots, flag/recomputed mismatches.
//! - `list <save> [n]`: the n largest empty landlocked lots with reasons.
//! - `at <save> x y`: the lot under a point and why it is what it is.
//! - `day <save>`: a day of play (pop, treasury).
use nd_sim::zoning::{landlocked_why, Lot};
use nd_sim::{LandUse, Vec2, World};
use std::collections::BTreeMap;

fn why(w: &World, l: &Lot) -> String {
    if let Some(g) = w.landscape.lot_ground_problem(&l.polygon, l.center) {
        return format!("ground: {g}");
    }
    match landlocked_why(l.land_use, &l.polygon, &w.roads) {
        Some(r) => r.to_string(),
        None => "none".into(),
    }
}

/// Sharpest corner of the outline in degrees (convex or not).
fn min_angle(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    let mut best = 180.0f32;
    for i in 0..n {
        let a = poly[(i + n - 1) % n] - poly[i];
        let c = poly[(i + 1) % n] - poly[i];
        if a.length() < 0.5 || c.length() < 0.5 {
            continue;
        }
        let ang = a.normalize().dot(c.normalize()).clamp(-1.0, 1.0).acos().to_degrees();
        best = best.min(ang);
    }
    best
}

fn compact(l: &Lot) -> f32 {
    let p = nd_sim::geom::perimeter(&l.polygon);
    l.area() / (p * p)
}

fn stats(w: &World) {
    let lots = w.lots.len();
    let ll: Vec<&Lot> = w.lots.values().filter(|l| l.landlocked).collect();
    let empty: Vec<&&Lot> = ll.iter().filter(|l| l.building.is_none()).collect();
    let empty_dead: Vec<&&Lot> = empty.iter().filter(|l| l.land_use != LandUse::Park && l.park.is_none()).copied().collect();
    let area: f32 = empty_dead.iter().map(|l| l.area()).sum();
    println!("lots {lots}; landlocked {} ({:.1} ha); empty landlocked non-park {} ({:.1} ha)", ll.len(), ll.iter().map(|l| l.area()).sum::<f32>() / 1e4, empty_dead.len(), area / 1e4);
    let mut by: BTreeMap<String, (usize, f32)> = BTreeMap::new();
    for l in &empty_dead {
        let e = by.entry(why(w, l)).or_default();
        e.0 += 1;
        e.1 += l.area();
    }
    for (k, (n, a)) in &by {
        println!("  empty dead by reason {k:<60} {n:>5} {:>7.2} ha", a / 1e4);
    }
    let named = empty_dead.iter().filter(|l| l.car_access()).count();
    println!("  of which report car_access (front_segment == car_segment): {named}");
    let stale = w.lots.values().filter(|l| l.landlocked && why(w, l) == "none").count();
    let stale_other = w.lots.values().filter(|l| !l.landlocked && why(w, l) != "none" && l.building.is_none()).count();
    println!("flag stale: landlocked but no reason now {stale}; usable empty lots with a reason {stale_other}");
    // Slivers: zoned lots with a corner under 20 degrees or compactness under the bar.
    let sliver = |l: &Lot| min_angle(&l.polygon) < 20.0 || compact(l) < 0.03;
    let sl: Vec<&Lot> = w.lots.values().filter(|l| l.land_use != LandUse::Park && sliver(l)).collect();
    println!("sliver lots (corner < 20 deg or compactness < 0.03): {} ({} built, {} landlocked)", sl.len(), sl.iter().filter(|l| l.building.is_some()).count(), sl.iter().filter(|l| l.landlocked).count());
    // Floating: a building whose own (primary) lot has no way at any edge.
    let floating = w
        .buildings
        .values()
        .filter(|b| !b.is_special() && w.lots.get(b.lot).is_some_and(|l| landlocked_why(LandUse::Residential, &l.polygon, &w.roads) == Some("no_frontage")))
        .count();
    println!("buildings whose own lot fronts no way (floating): {floating}");
    // Shard pads: a building alone on a lot with a corner under 20 degrees
    // (its gravel was the whole shard; the renderer now draws its apron).
    let shards = w.buildings.values().filter(|b| !b.is_special() && b.plot.extra_lots.is_empty() && w.lots.get(b.lot).is_some_and(|l| min_angle(&l.polygon) < 20.0)).count();
    println!("buildings alone on a shard lot (corner < 20 deg): {shards}");
    let built_ll = w.lots.values().filter(|l| l.landlocked && l.building.is_some()).count();
    println!("built landlocked lots (backland in a site): {built_ll}");
}

fn list(w: &World, n: usize) {
    let mut v: Vec<(nd_sim::zoning::LotId, &Lot)> = w.lots.iter().filter(|(_, l)| l.landlocked && l.building.is_none() && l.land_use != LandUse::Park).collect();
    v.sort_by(|a, b| b.1.area().total_cmp(&a.1.area()));
    for (id, l) in v.into_iter().take(n) {
        println!(
            "{id:?} {:?} {:.0} m2 at ({:.0},{:.0}) car {} front_seg {} why {} angle {:.0} compact {:.3} shore {:.0}",
            l.land_use,
            l.area(),
            l.center.x,
            l.center.y,
            l.car_access(),
            l.front_segment.0,
            why(w, l),
            min_angle(&l.polygon),
            compact(l),
            l.shore.frontage
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let w = World::read_save(&args[2]).expect("load");
    eprintln!("loaded in {:?}", t.elapsed());
    match args[1].as_str() {
        "stats" => stats(&w),
        "list" => list(&w, args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40)),
        "at" => {
            let p = Vec2::new(args[3].parse().unwrap(), args[4].parse().unwrap());
            for (id, l) in w.lots.iter().filter(|(_, l)| l.contains(p) || l.center.distance(p) < 15.0) {
                println!(
                    "{id:?} {:?} {:.0} m2 c ({:.1},{:.1}) landlocked {} why {} car {} front_seg {} car_seg {:?} bld {:?} angle {:.0} compact {:.3}\n  poly {:?}",
                    l.land_use,
                    l.area(),
                    l.center.x,
                    l.center.y,
                    l.landlocked,
                    why(&w, l),
                    l.car_access(),
                    l.front_segment.0,
                    l.car_segment.map(|s| s.0),
                    l.building.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()),
                    min_angle(&l.polygon),
                    compact(l),
                    l.polygon.iter().map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>()
                );
            }
        }
        "site" => {
            let p = Vec2::new(args[3].parse().unwrap(), args[4].parse().unwrap());
            let Some((bid, b)) = w.buildings.iter().filter(|(_, b)| w.lots.get(b.lot).is_some()).min_by(|a, b| w.lots[a.1.lot].center.distance(p).total_cmp(&w.lots[b.1.lot].center.distance(p))) else { return };
            let l = &w.lots[b.lot];
            println!("{bid:?} {} lot {:?} c ({:.0},{:.0}) construction {:?}", b.name, b.lot, l.center.x, l.center.y, b.construction.as_ref().map(|c| (c.progress, c.deliveries, c.blocked)));
            println!("  workers {} jobs {} business closed {:?}", b.workers.len(), b.jobs, b.business.as_ref().map(|z| z.closed));
            println!("  lot car_seg {:?} car_point ({:.1},{:.1}) front_seg {} entrance ({:.1},{:.1}) landlocked {}", l.car_segment.map(|s| s.0), l.car_point.x, l.car_point.y, l.front_segment.0, l.entrance().x, l.entrance().y, l.landlocked);
            for line in w.site_reach_debug(bid) {
                println!("  {line}");
            }
            let blocked: Vec<_> = w.buildings.iter().filter(|(_, b)| b.construction.as_ref().is_some_and(|c| c.blocked.is_some())).map(|(id, b)| (id, b.name.clone())).collect();
            println!("blocked sites {}: {:?}", blocked.len(), blocked.iter().take(10).collect::<Vec<_>>());
            let staffed = w.buildings.values().filter(|b| b.construction.is_some() && !b.workers.is_empty()).count();
            println!("sites with staff on the payroll: {staffed}");
        }
        "node" => {
            let net = &w.traffic.net;
            for a in &args[3..] {
                let n: usize = a.parse().unwrap();
                let node = &w.roads.nodes[n];
                println!("node {n} at ({:.1},{:.1}) level {} ring? segs:", node.pos.x, node.pos.y, node.level);
                for &sid in &node.segments {
                    let s = w.roads.segment(sid);
                    println!("  seg {} {:?} alive {} a {} b {} lanes {}/{} ring {} ramp {} viaduct {} bridge {}", sid.0, s.kind, s.alive, s.a.0, s.b.0, s.lanes_fwd, s.lanes_bwd, s.ring, s.ramp, s.viaduct, s.bridge);
                    for &lid in net.segment_lanes.get(&sid).map(|v| v.as_slice()).unwrap_or(&[]) {
                        let l = &net.lanes[lid as usize];
                        let into = l.end_node.0 as usize == n;
                        println!("     lane {lid} {:?} ends here {into} trap {} next {:?}", l.kind, net.trap[lid as usize], l.next.iter().map(|&x| (x, format!("{:?}", net.lanes[x as usize].kind), net.lanes[x as usize].next.clone())).collect::<Vec<_>>());
                    }
                }
            }
        }
        "day" => {
            let mut w = w;
            let mut minds = nd_minds::RuleBasedMind::default();
            println!("start: pop {} bld {} treasury {:.0}", w.citizens.len(), w.buildings.len(), w.economy.treasury);
            for h in 0..24 {
                for _ in 0..60 {
                    w.tick(&mut minds);
                }
                if h % 6 == 5 {
                    println!("hour {:>2}: pop {} bld {} treasury {:.0}", h + 1, w.citizens.len(), w.buildings.len(), w.economy.treasury);
                }
            }
        }
        m => println!("unknown mode {m}"),
    }
}
