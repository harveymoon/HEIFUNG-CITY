//! [r1-roads] Probes on a copy of a save for the road and path tool flags
//! (K5, K8/K9, K13, K42, K47/K48). Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example r1_probe -- <save.ron> <cmd> [args]`
//! - `parking x y`: the city's parking lots near a point
//! - `roads x y r`: the ways near a point with their levels, heights, flags
//! - `cross x y r`: the walk graph's crossings and footbridges near a point
//! - `parcels x y r`: parcels near a point and why they cannot be zoned
use nd_sim::{Vec2, World};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let num = |i: usize| args[i].parse::<f32>().unwrap();
    let p = Vec2::new(num(3), num(4));
    let r = args.get(5).and_then(|s| s.parse::<f32>().ok()).unwrap_or(40.0);
    match args[2].as_str() {
        "parking" => {
            for (b, c) in w.vehicles.city_lots.iter() {
                let Some(bd) = w.buildings.get(*b) else { continue };
                let Some(l) = w.lots.get(bd.lot) else { continue };
                if l.center.distance(p) > 150.0 {
                    continue;
                }
                println!("{:?} {} size {:?} area {:.0} center ({:.0},{:.0}) poly {:?}", b, bd.name, c.size, l.area(), l.center.x, l.center.y, l.polygon.iter().map(|v| (v.x.round(), v.y.round())).collect::<Vec<_>>());
            }
        }
        "roads" => {
            for sid in w.roads.segments_near(p, r) {
                let s = w.roads.segment(sid);
                let (a, b) = w.roads.segment_points(sid);
                let (na, nb) = (w.roads.node(s.a), w.roads.node(s.b));
                let name = w.streets.of(sid).map(|x| x.name.clone()).unwrap_or_default();
                println!(
                    "seg {} {:?} {name} lanes {}+{} ({:.0},{:.0})->({:.0},{:.0}) levels {:.1}/{:.1} z {:.1}/{:.1} ground {:.1}/{:.1} bridge {} viaduct {} tunnel {} ramp {} hw {:.1} deg {}/{}",
                    sid.0, s.kind, s.lanes_fwd, s.lanes_bwd, a.x, a.y, b.x, b.y, na.level, nb.level,
                    w.road_z[s.a.0 as usize], w.road_z[s.b.0 as usize], w.landscape.height(a), w.landscape.height(b),
                    s.bridge, s.viaduct, s.tunnel, s.ramp, s.half_width(), na.segments.len(), nb.segments.len()
                );
            }
            for pl in w.pillars.iter().filter(|x| x.at.distance(p) < r) {
                let on = w.roads.nearest_where(pl.at, |s| s.is_street()).map(|(sid, _, q)| (sid.0, q.distance(pl.at)));
                println!("pillar at ({:.1},{:.1}) level {:.1} way {} nearest street {:?}", pl.at.x, pl.at.y, pl.level, pl.way, on);
            }
        }
        "cross" => {
            for (k, c) in w.walk.crossings.iter().enumerate() {
                if c.mid.distance(p) > r {
                    continue;
                }
                let name = w.streets.of(c.segment).map(|x| x.name.clone()).unwrap_or_default();
                println!("crossing {k} {:?} seg {} {name} mid ({:.1},{:.1}) node {:?} closed {} bridged {} button {} shut {}", c.kind, c.segment.0, c.mid.x, c.mid.y, c.node.map(|n| n.0), c.closed, c.bridged, c.button, c.shut);
            }
            for f in &w.walk.footbridges {
                if f.top[0].distance(p) > r && f.top[1].distance(p) > r {
                    continue;
                }
                println!("footbridge over {} top {:?} foot {:?} joined {:?}", f.crossing, f.top, f.foot, f.joined);
            }
            for rule in w.crosswalks.rules.iter().filter(|x| x.at.distance(p) < r) {
                println!("rule at ({:.1},{:.1}) dir {:?} {:?} keep {}", rule.at.x, rule.at.y, rule.dir, rule.upgrade, rule.keep_crosswalk);
            }
        }
        "parcels" => {
            for i in w.parcels.in_circle(p, r) {
                let q = &w.parcels.parcels[i];
                println!(
                    "parcel {i} c ({:.0},{:.0}) area {:.0} n{} street {:?} path {:?} lu {:?} why {:?} refusal {:?}",
                    q.center.x, q.center.y, q.area, q.polygon.len(), q.street_frontage, q.path_frontage, q.land_use, w.no_parcel_why(q.center), w.parcel_refusal(i, Some(nd_sim::LandUse::Residential))
                );
                println!("   poly {:?}", q.polygon.iter().map(|v| (v.x.round(), v.y.round())).collect::<Vec<_>>());
                if let Some(l) = q.lot.and_then(|l| w.lots.get(l)) {
                    println!("   lot {:?} {:?} landlocked {} building {:?} city {}", q.lot, l.land_use, l.landlocked, l.building.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()), l.building.is_some_and(|b| w.is_city_building(b)));
                }
            }
        }
        "plan" => {
            let mut plans: Vec<(String, nd_sim::plan::Plan)> = vec![("master".into(), w.master.clone())];
            for pr in w.project.iter().chain(w.parked_projects.iter()) {
                plans.push((format!("project {} {}", pr.id, pr.name), pr.apply_to(&w.master)));
            }
            for (label, plan) in plans {
                for (id, g) in &plan.gestures {
                    if let nd_sim::plan::Gesture::Road { points, kind, elevation, curved, .. } = g {
                        if points.iter().any(|q| q.distance(p) < r) && (label == "master" || w.master.gestures.get(id) != Some(g)) {
                            println!("{label} {:?} {:?} curved {curved} pts {:?} elev {:?}", id, kind, points.iter().map(|v| (v.x.round(), v.y.round())).collect::<Vec<_>>(), elevation);
                        }
                    }
                }
            }
        }
        "drawpath" => {
            // drawpath x0 y0 x1 y1 [x y ...]: a path at grade, built, then the
            // ground along it, its heights and any columns.
            let pts: Vec<Vec2> = args[3..].chunks(2).filter(|c| c.len() == 2).map(|c| Vec2::new(c[0].parse().unwrap(), c[1].parse().unwrap())).collect();
            let refuse = w.road_refusal(&pts, &[], nd_sim::roads::RoadKind::Path, 0, 0);
            let warn = w.road_grade_warnings(&pts, &[], nd_sim::roads::RoadKind::Path, 0, 0);
            println!("refusal {refuse:?} warnings {warn:?}");
            for k in 0..=20 {
                let q = pts[0].lerp(pts[pts.len() - 1], k as f32 / 20.0);
                print!("({:.0},{:.0}) natural {:.1}; ", q.x, q.y, w.landscape.height(q));
            }
            println!();
            let before = w.pillars.len();
            w.add_path(&pts);
            println!("pillars {} -> {}", before, w.pillars.len());
            for (id, l) in w.lots.iter() {
                if pts.iter().any(|q| q.distance(l.center) < 45.0) {
                    let fk = w.roads.try_segment(l.front_segment).map(|s| s.kind);
                    println!("lot {:?} {:?} center ({:.0},{:.0}) pad {:.1} natural {:.1} front {:?} landlocked {} building {}", id, l.land_use, l.center.x, l.center.y, w.lot_pad(id), w.landscape.height(l.center), fk, l.landlocked, l.building.is_some());
                }
            }
            let (lo, hi) = nd_sim::geom::bounds(&pts);
            for (sid, s) in w.roads.alive_segments() {
                if s.kind != nd_sim::roads::RoadKind::Path {
                    continue;
                }
                let (a, b) = w.roads.segment_points(sid);
                if a.x < lo.x - 10.0 || a.x > hi.x + 10.0 || a.y < lo.y - 10.0 || a.y > hi.y + 10.0 {
                    continue;
                }
                let (na, nb) = (w.roads.node(s.a), w.roads.node(s.b));
                for k in 0..=10 {
                    let t = k as f32 / 10.0;
                    let q = a.lerp(b, t);
                    print!("[{:.1} way {:.1} built {:.1} nat {:.1}] ", t, w.road_height(sid, t), w.ground_height(q), w.landscape.height(q));
                }
                println!();
                println!("path seg ({:.0},{:.0})->({:.0},{:.0}) level {:.1}/{:.1} z {:.1}/{:.1} natural {:.1}/{:.1} built {:.1}/{:.1} bridge {} viaduct {} inner {:?}", a.x, a.y, b.x, b.y, na.level, nb.level, w.road_z[s.a.0 as usize], w.road_z[s.b.0 as usize], w.landscape.height(a), w.landscape.height(b), w.ground_height(a), w.ground_height(b), s.bridge, s.viaduct, w.road_inner.get(sid.0 as usize).map(|v| v.iter().map(|(t, z)| ((t * 100.0).round() / 100.0, (z * 10.0).round() / 10.0)).collect::<Vec<_>>()));
            }
        }
        _ => println!("unknown"),
    }
}
