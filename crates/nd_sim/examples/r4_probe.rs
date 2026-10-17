//! [r4] Probe a save for the transit flags: the streets near a point, and
//! whether bus stops snap and route between given points. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example r4_probe -- <save.ron> segs x y r
//! cargo run --profile fast -p nd_sim --example r4_probe -- <save.ron> route x1 y1 x2 y2 [x3 y3 ...]
use glam::Vec2;
use nd_sim::transit::{BusStop, Transit};
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let w = World::read_save(&args[1]).expect("load");
    let f: Vec<f32> = args[3..].iter().map(|s| s.parse().unwrap()).collect();
    match args[2].as_str() {
        "segs" => {
            let (p, r) = (Vec2::new(f[0], f[1]), f[2]);
            for sid in w.roads.segments_in_box(p - Vec2::splat(r), p + Vec2::splat(r)) {
                let s = w.roads.segment(sid);
                let (a, b) = w.roads.segment_points(sid);
                let lanes = w.traffic.net.segment_lanes.get(&sid).map(|v| v.len()).unwrap_or(0);
                println!(
                    "{:?} {:?} ({:.0},{:.0})->({:.0},{:.0}) fwd {} bwd {} ring {} ramp {} hwy {} via {} tun {} car_only {:?} lanes {} nodes {:?}->{:?} name {:?}",
                    sid, s.kind, a.x, a.y, b.x, b.y, s.lanes_fwd, s.lanes_bwd, s.ring, s.ramp, s.is_highway(), s.viaduct, s.tunnel, s.car_only, lanes, s.a, s.b,
                    w.streets.of(sid).map(|s| s.name.clone())
                );
            }
        }
        "route" => {
            let pts: Vec<Vec2> = f.chunks(2).map(|c| Vec2::new(c[0], c[1])).collect();
            let stops: Vec<Option<BusStop>> = pts.iter().map(|&p| w.transit.stop_for_click(&w.roads, p)).collect();
            for (p, s) in pts.iter().zip(&stops) {
                match s {
                    Some(s) => {
                        let lane = w.traffic.net.nearest_lane_on(s.segment, s.pos);
                        println!("stop at {p:?} -> seg {:?} pos {:?} lane {:?}", s.segment, s.pos, lane);
                    }
                    None => println!("stop at {p:?} -> NONE"),
                }
            }
            let stops: Vec<BusStop> = stops.into_iter().flatten().collect();
            for (i, leg) in Transit::plan_legs(&w.traffic.net, &stops).into_iter().enumerate() {
                match leg {
                    Some((l, _, _)) => println!("leg {i}: {} lanes, {:.0} m", l.len(), nd_sim::geom::path_length(&w.traffic.net.polyline(&l, 0.0, 0.0))),
                    None => println!("leg {i}: NO ROAD"),
                }
            }
            for p in Transit::check_stops(&w.traffic.net, &stops) {
                println!("problem: {p}");
            }
        }
        "trains" => {
            // Run `ticks`, watching the trains and the level crossings near (x, y).
            let mut w = w;
            let (ticks, p) = (f[0] as u64, Vec2::new(f[1], f[2]));
            let mut minds = nd_minds::RuleBasedMind::default();
            let near: Vec<usize> = w.level_crossings().iter().enumerate().filter(|(_, c)| c.pos.distance(p) < 250.0).map(|(k, _)| k).collect();
            println!("crossings near: {:?}", near.iter().map(|&k| w.level_crossings()[k].pos).collect::<Vec<_>>());
            let mut shut = vec![0u32; near.len()];
            let mut shut_dwell = vec![0u32; near.len()];
            let (mut min_gap, mut overlaps, mut hard_stops) = (f32::MAX, 0, 0);
            let mut last_v: std::collections::HashMap<nd_sim::TrainId, f32> = Default::default();
            for _ in 0..ticks {
                w.tick(&mut minds);
                for (id, t) in w.rail.trains.iter() {
                    if let Some((g, _)) = w.rail.clear_ahead(id) {
                        min_gap = min_gap.min(g);
                        if g <= 0.0 {
                            overlaps += 1;
                        }
                    }
                    if last_v.get(&id).is_some_and(|&v| v > 10.0 && t.v < 1.0 && t.dwell <= 0.0) {
                        hard_stops += 1;
                    }
                    last_v.insert(id, t.v);
                }
                let dwelling = w.rail.trains.values().any(|t| t.dwell > 0.0);
                for (i, &k) in near.iter().enumerate() {
                    if w.level_crossings()[k].shut() {
                        shut[i] += 1;
                        if dwelling {
                            shut_dwell[i] += 1;
                        }
                    }
                }
            }
            println!("trains: min gap {min_gap:.1} m, overlaps {overlaps}, stops from >10 m/s in a tick {hard_stops}");
            for (i, &k) in near.iter().enumerate() {
                let c = &w.level_crossings()[k];
                println!("crossing {:?}: shut {} of {ticks} ticks ({} while a train stood at a call), trains {} vehicles held {} avg wait {:.1}s", c.pos, shut[i], shut_dwell[i], c.trains_total, c.vehicles_held, c.average_wait());
            }
            println!("rail: {:?} trains today p {} f {}", w.rail.trains.len(), w.rail.stats.passenger_trains_today, w.rail.stats.freight_trains_today);
            let a = &w.air.stats;
            println!("air: flights {} holds {} ground holds {} forced {} min battery {:.2}", a.flights_today, a.holds_today, a.ground_holds_today, a.forced_landings_today, w.air.drones.values().map(|d| d.battery).fold(1.0, f32::min));
        }
        _ => {}
    }
}
