//! [r4-crossings] Run a save and measure one spot of the street: how its
//! street is cut up (junctions, pieces, crossings), the queues and trip
//! times through it, walkers crossing at grade against over footbridges,
//! and riders or cars drawn off their surfaces. Not part of the game.
//! `crossing_area <save.ron> x y [radius] [ticks] [citizen name]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::citizens::{Activity, TravelMode};
use nd_sim::roads::{NodeId, RoadKind};
use nd_sim::World;
use glam::Vec2;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn topology(w: &World, c: Vec2, rad: f32) {
    let r = &w.roads;
    let (mut junctions, mut path_meets, mut tiny, mut pieces) = (0, 0, 0, 0);
    for (i, n) in r.nodes.iter().enumerate() {
        if n.pos.distance(c) > rad {
            continue;
        }
        let id = NodeId(i as u32);
        let deg = r.street_degree(id);
        let paths = n.segments.iter().filter(|&&s| r.segment(s).alive && r.segment(s).kind == RoadKind::Path).count();
        if deg >= 3 {
            junctions += 1;
        } else if deg >= 1 && paths > 0 {
            path_meets += 1;
        }
    }
    for (sid, s) in r.alive_segments() {
        let (a, b) = r.segment_points(sid);
        if s.is_street() && (a.distance(c) <= rad || b.distance(c) <= rad) {
            pieces += 1;
            if a.distance(b) < 5.0 {
                tiny += 1;
            }
        }
    }
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for k in &w.walk.crossings {
        if k.mid.distance(c) <= rad && k.kind != nd_sim::walk::CrossingKind::Jay {
            let key = format!("{:?}{}", k.kind, if k.closed { "-closed" } else { "" });
            *kinds.entry(key).or_default() += 1;
        }
    }
    let bridges = w.walk.footbridges.iter().filter(|f| w.walk.crossings[f.crossing as usize].mid.distance(c) <= rad).count();
    println!("topology: street junctions {junctions}, path-on-street nodes {path_meets}, street pieces {pieces} (under 5 m: {tiny}), crossings {kinds:?}, footbridges {bridges}");
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&a[1]).expect("load");
    let c = Vec2::new(a[2].parse().unwrap(), a[3].parse().unwrap());
    let rad: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let ticks: u64 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(600);
    let who = a.get(6).cloned().unwrap_or_else(|| "Ilya Fontaine".into());
    topology(&w, c, rad);
    let detail = std::env::var("DETAIL").is_ok();
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    // The followed driver's car, by name.
    let mut followed = w.traffic.cars.iter().find(|(_, car)| w.citizens.get(car.citizen).is_some_and(|p| p.name == who)).map(|(id, car)| (id, car.drive.trip_start, car.drive.expected_min));
    if let Some((id, start, exp)) = followed {
        println!("{who}: car {id:?} trip started tick {start} ({} min ago), expected {exp:.0} min", w.clock.tick - start);
    }
    let (mut standing_sum, mut standing_max, mut samples) = (0usize, 0usize, 0usize);
    let mut reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
    // Cars through the spot: first and last tick seen in it.
    let mut inside: HashMap<nd_sim::CarId, (u64, u64)> = HashMap::new();
    let mut done_dwell: Vec<u64> = Vec::new();
    let (mut at_grade, mut over_bridge): (BTreeSet<_>, BTreeSet<_>) = (BTreeSet::new(), BTreeSet::new());
    let (mut rider_on_bridge, mut rider_aloft, mut car_aloft, mut rider_flying) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut worst_rider = 0.0f32;
    let mut worst_car = 0.0f32;
    for _ in 0..ticks {
        w.tick(&mut minds);
        let now = w.clock.tick;
        if let Some((id, _, _)) = followed {
            if detail && now % 10 == 0 {
                if let Some(car) = w.traffic.cars.get(id) {
                    println!("  [{now}] followed at ({:.0},{:.0}) v {:.1} stalled {:.0} why {} idx {}/{}", car.pos.x, car.pos.y, car.v, car.stalled, w.traffic.stop_reason(id).key(), car.idx, car.route.len());
                }
            }
        }
        if detail && now % 60 == 0 {
            let mut hs: BTreeMap<String, usize> = BTreeMap::new();
            for (id, car) in w.traffic.cars.iter() {
                if car.pos.distance(c) <= rad && car.v < 0.5 && car.stalled > 60.0 {
                    if let Some(h) = w.traffic.jam_chain(id).head() {
                        let hc = &w.traffic.cars[h];
                        *hs.entry(format!("head {h:?} at ({:.0},{:.0}) {} lane {}", hc.pos.x, hc.pos.y, w.traffic.stop_reason(h).key(), hc.route.get(hc.idx).copied().unwrap_or(0))).or_default() += 1;
                    }
                }
            }
            println!("  [{now}] queues {hs:?}");
        }
        if let Some((id, start, exp)) = followed {
            if w.traffic.cars.get(id).map_or(true, |car| car.drive.trip_start != start) {
                println!("{who}: trip over at tick {now}: {} min against {exp:.0} expected", now - start);
                followed = None;
            }
        }
        let mut standing = 0;
        let mut seen: BTreeSet<nd_sim::CarId> = BTreeSet::new();
        for (id, car) in w.traffic.cars.iter() {
            if car.pos.distance(c) > rad {
                continue;
            }
            seen.insert(id);
            let e = inside.entry(id).or_insert((now, now));
            e.1 = now;
            if car.v < 0.5 && car.stalled > 60.0 {
                standing += 1;
                *reasons.entry(w.traffic.stop_reason(id).key()).or_default() += 1;
            }
            let lift = w.car_height(car) - w.ground_height(car.pos);
            if lift.abs() > 1.0 {
                if detail && car_aloft.len() < 12 && !car_aloft.contains(&id) {
                    let lane = car.route.get(car.idx).copied().unwrap_or(0);
                    println!("  car {id:?} at ({:.1},{:.1}) lift {lift:.1} car_h {:.1} ground {:.1} lane {lane} kind {:?}", car.pos.x, car.pos.y, w.car_height(car), w.ground_height(car.pos), w.traffic.net.lanes[lane as usize].kind);
                }
                car_aloft.insert(id);
                worst_car = worst_car.max(lift.abs());
            }
        }
        inside.retain(|id, (first, last)| {
            if seen.contains(id) {
                return true;
            }
            done_dwell.push(*last - *first + 1);
            false
        });
        standing_sum += standing;
        standing_max = standing_max.max(standing);
        samples += 1;
        for &(cr, cid) in &w.peds.occupants {
            if w.walk.crossings.get(cr as usize).is_some_and(|k| k.mid.distance(c) <= rad) {
                at_grade.insert(cid);
            }
        }
        for (cid, p) in w.citizens.iter() {
            if p.inside.is_some() || p.pos.distance(c) > rad {
                continue;
            }
            let Activity::Traveling { path, idx, .. } = &p.activity else { continue };
            if *idx == 0 || *idx >= path.len() {
                continue;
            }
            let lvl = w.walker_level(p, p.pos);
            let ground = w.ground_height(p.pos);
            let z = w.walker_way_height(p, p.pos).unwrap_or(ground) + lvl;
            match p.mode {
                TravelMode::Walk => {
                    if lvl > 0.5 {
                        over_bridge.insert(cid);
                    }
                }
                TravelMode::Bike => {
                    if lvl > 0.5 {
                        rider_on_bridge.insert(cid);
                        // Rising to (or dropping from) a deck straight off a
                        // point that is no foot of any stairs: in the air.
                        let (a, b) = (path[*idx - 1], path[*idx]);
                        let foot = |q: Vec2| w.walk.footbridges.iter().any(|f| f.foot.iter().any(|&x| x.distance(q) < 0.05 && !f.top.contains(&x))) || w.walk.nodes.iter().any(|n| n.level == 0.0 && n.pos.distance(q) < 0.05 && false);
                        let (la, lb) = (w.walk.level_at_node(a), w.walk.level_at_node(b));
                        if (la == 0.0 && lb > 0.0 && !foot(a)) || (lb == 0.0 && la > 0.0 && !foot(b)) {
                            if detail && rider_flying.len() < 12 && !rider_flying.contains(&cid) {
                                println!("  flying rider {cid:?} at ({:.1},{:.1}) lvl {lvl:.1} leg ({:.1},{:.1}) {la:.1} -> ({:.1},{:.1}) {lb:.1}", p.pos.x, p.pos.y, a.x, a.y, b.x, b.y);
                            }
                            rider_flying.insert(cid);
                        }
                    } else if z - ground > 1.5 && w.walker_way_height(p, p.pos).is_some() {
                        if detail && rider_aloft.len() < 12 && !rider_aloft.contains(&cid) {
                            println!("  rider {cid:?} at ({:.1},{:.1}) z {:.1} ground {:.1} leg ({:.1},{:.1})->({:.1},{:.1})", p.pos.x, p.pos.y, z, ground, path[*idx - 1].x, path[*idx - 1].y, path[*idx].x, path[*idx].y);
                        }
                        rider_aloft.insert(cid);
                        worst_rider = worst_rider.max(z - ground);
                    }
                }
            }
        }
    }
    let mean_dwell = if done_dwell.is_empty() { 0.0 } else { done_dwell.iter().sum::<u64>() as f32 / done_dwell.len() as f32 };
    let slow = done_dwell.iter().filter(|&&d| d >= 3).count();
    println!(
        "{ticks} ticks: standing>60s mean {:.1} max {standing_max}, reasons {reasons:?}; cars through {} mean {:.2} min in the spot, {} took 3+ min",
        standing_sum as f32 / samples.max(1) as f32,
        done_dwell.len(),
        mean_dwell,
        slow
    );
    println!("walkers crossing at grade {} / up on a footbridge {}; riders up on a footbridge {} (rising off no stairs {}), riders aloft off it {} (worst {:.1} m); cars off their road {} (worst {:.1} m)", at_grade.len(), over_bridge.len(), rider_on_bridge.len(), rider_flying.len(), rider_aloft.len(), worst_rider, car_aloft.len(), worst_car);
    topology(&w, c, rad);
}
