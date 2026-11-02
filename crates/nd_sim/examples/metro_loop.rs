//! [r4-metroloop] Probe: load a save, run some hours and count transit
//! riders who leave a stop or platform without boarding (and how: by
//! giving up, or by setting off again from where they wait), riders who
//! come back to the stop they left, boardings by mode, trips that fail
//! and the door-to-door commute (home to work, however many tries).
//! `SPOT=1` only lists where each metro station's platform and entrance
//! join the walk graph (`ALL=1`: every station, not only those up on a
//! deck).
//!
//! cargo run --profile fast -p nd_sim --example metro_loop -- saves/autosave.ron 24 [name]

use nd_sim::citizens::{Activity, CitizenId};
use nd_sim::transit::TransitLeg;
use nd_sim::World;
use std::collections::BTreeMap;

fn leg_kind(l: &TransitLeg) -> &'static str {
    match l {
        TransitLeg::Bus { .. } => "bus",
        TransitLeg::Subway { .. } => "metro",
        TransitLeg::Rail { .. } => "rail",
        TransitLeg::Taxi => "taxi",
        TransitLeg::Air { .. } => "air",
        TransitLeg::Ferry { .. } => "ferry",
        TransitLeg::Cable { .. } => "cable",
    }
}

/// Where a leg is waited for: the stop's key, to tell "back at the same
/// stop".
fn leg_stop(l: &TransitLeg) -> String {
    match l {
        TransitLeg::Bus { route, board, .. } => format!("bus {route:?}/{board}"),
        TransitLeg::Subway { board, .. } => format!("metro {board:?}"),
        TransitLeg::Rail { board, .. } => format!("rail {board:?}"),
        TransitLeg::Air { from, .. } => format!("air {from:?}"),
        TransitLeg::Taxi => "taxi".to_string(),
        TransitLeg::Ferry { board, .. } => format!("ferry {board:?}"),
        TransitLeg::Cable { board, .. } => format!("cable {board:?}"),
    }
}

/// Is the walk node at `p` along a raised way (a viaduct or bridge deck,
/// or a way at `VIADUCT_LEVEL` or more there)? (segment, its level there)
fn on_deck(w: &World, p: glam::Vec2) -> Option<(nd_sim::roads::SegmentId, f32)> {
    for sid in w.roads.segments_near(p, 20.0) {
        let s = w.roads.segment(sid);
        let (a, b) = w.roads.segment_points(sid);
        let (t, q) = nd_sim::geom::project_on_segment(p, a, b);
        let (la, lb) = (w.roads.node(s.a).level, w.roads.node(s.b).level);
        let lv = la + (lb - la) * t;
        if q.distance(p) <= s.half_width() + 4.0 && (s.viaduct || s.bridge || lv >= nd_sim::roads::VIADUCT_LEVEL) && t > 0.0 && t < 1.0 {
            return Some((sid, lv));
        }
    }
    None
}

/// Every metro station: where its platform and entrance snap onto the
/// walk graph, and whether that node is up on a deck.
fn stations(w: &World) {
    let (mut plat_deck, mut ent_deck) = (0, 0);
    for s in w.metro_stations() {
        let (Some(p), Some(e)) = (w.metro_platform(s), w.building_entrance(s)) else { continue };
        let np = w.walk.nearest(p, 60.0).map(|i| w.walk.nodes[i as usize].pos);
        let ne = w.walk.nearest(e, 60.0).map(|i| w.walk.nodes[i as usize].pos);
        let dp = np.and_then(|q| on_deck(w, q));
        let de = ne.and_then(|q| on_deck(w, q));
        plat_deck += dp.is_some() as u32;
        ent_deck += de.is_some() as u32;
        if dp.is_some() || de.is_some() || std::env::var("ALL").is_ok() {
            println!("  station {s:?} {} platform ({:.0},{:.0}) -> node {:?} deck {:?} | entrance ({:.0},{:.0}) -> node {:?} deck {:?}", w.buildings[s].name, p.x, p.y, np.map(|q| (q.x as i32, q.y as i32)), dp, e.x, e.y, ne.map(|q| (q.x as i32, q.y as i32)), de);
        }
    }
    println!("stations {}: platform snaps onto a deck {plat_deck}, entrance {ent_deck}", w.metro_stations().len());
}

#[derive(Default)]
struct Tally {
    boardings: BTreeMap<&'static str, u32>,
    /// Left the wait without boarding: (kind, how) → n.
    left: BTreeMap<(&'static str, &'static str), u32>,
    /// Set off again from the platform point itself (underground).
    from_platform: u32,
    /// Waited again at a stop they had left without boarding, same trip.
    loops: BTreeMap<&'static str, u32>,
    loopers: std::collections::BTreeSet<CitizenId>,
    failed: u32,
    /// Why they set off another way: (kind, hour, thought) → n.
    why: BTreeMap<String, u32>,
    commutes: Vec<f32>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let hours: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24);
    let watch = args.get(3).cloned();
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    println!("loaded day {} {:02}:{:02}, {} people", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len());
    stations(&w);
    if let Ok(xy) = std::env::var("SEG") {
        let v: Vec<f32> = xy.split(',').filter_map(|x| x.parse().ok()).collect();
        let p = glam::Vec2::new(v[0], v[1]);
        for sid in w.roads.segments_near(p, 60.0) {
            let s = w.roads.segment(sid);
            let (a, b) = w.roads.segment_points(sid);
            let (t, q) = nd_sim::geom::project_on_segment(p, a, b);
            println!("  seg {sid:?} ({:.0},{:.0})-({:.0},{:.0}) levels {:.1}/{:.1} viaduct {} bridge {} street {} hw {:.1} dist {:.1} t {t:.2} height there {:.1}", a.x, a.y, b.x, b.y, w.roads.node(s.a).level, w.roads.node(s.b).level, s.viaduct, s.bridge, s.is_street(), s.half_width(), q.distance(p), w.road_height(sid, t));
        }
        println!("  ground {:.1} relief {}", w.ground_height(p), w.relief);
        if let Some(n) = w.walk.nearest(p, 60.0) {
            let mut seen = vec![n];
            let mut front = vec![n];
            for depth in 0..4 {
                let mut next = Vec::new();
                for &u in &front {
                    let q = w.walk.nodes[u as usize].pos;
                    let edges: Vec<String> = w.walk.adj[u as usize].iter().map(|e| format!("{}{}", e.to, if e.cross != u32::MAX { "x" } else { "" })).collect();
                    println!("    d{depth} node {u} ({:.1},{:.1}) level {} edges {:?} way height {:?}", q.x, q.y, w.walk.nodes[u as usize].level, edges, w.way_height_on_leg(p, q, q));
                    for e in &w.walk.adj[u as usize] {
                        if !seen.contains(&e.to) {
                            seen.push(e.to);
                            next.push(e.to);
                        }
                    }
                }
                front = next;
            }
        }
    }
    if std::env::var("SPOT").is_ok() {
        return;
    }
    let watched: Option<CitizenId> = watch.as_ref().and_then(|n| w.citizens.iter().find(|(_, c)| &c.name == n).map(|(id, _)| id));
    let mut t = Tally::default();
    // Who waits where, last tick: (stop key, kind, since, on the platform).
    let mut waiting: BTreeMap<CitizenId, (String, &'static str, u64, bool)> = BTreeMap::new();
    // Stops left without boarding on the trip under way.
    let mut left_stops: BTreeMap<CitizenId, Vec<String>> = BTreeMap::new();
    // Left home at (tick), for the door-to-door commute.
    let mut left_home: BTreeMap<CitizenId, u64> = BTreeMap::new();
    let mut inside: BTreeMap<CitizenId, Option<nd_sim::buildings::BuildingId>> = w.citizens.iter().map(|(id, c)| (id, c.inside)).collect();
    let mut last_fail = w.connectivity.failed.back().map_or(0, |f| f.tick);
    let platforms: Vec<glam::Vec2> = w.metro_stations().iter().filter_map(|&s| w.metro_platform(s)).collect();
    let end = w.clock.tick + hours * 60;
    while w.clock.tick < end {
        w.tick(&mut minds);
        w.drain_events();
        let tick = w.clock.tick;
        t.failed += w.connectivity.failed.iter().filter(|f| f.tick > last_fail).count() as u32;
        last_fail = w.connectivity.failed.back().map_or(last_fail, |f| f.tick.max(last_fail));
        let mut now: BTreeMap<CitizenId, (String, &'static str, u64, bool)> = BTreeMap::new();
        for (id, c) in w.citizens.iter() {
            if let (Activity::Waiting { .. }, Some(tr)) = (&c.activity, c.transit) {
                let key = leg_stop(&tr.leg);
                now.insert(id, (key.clone(), leg_kind(&tr.leg), tr.since, c.on_platform()));
                if !waiting.contains_key(&id) && left_stops.get(&id).is_some_and(|v| v.contains(&key)) {
                    *t.loops.entry(leg_kind(&tr.leg)).or_default() += 1;
                    t.loopers.insert(id);
                    if Some(id) == watched || t.loops.values().sum::<u32>() <= 3 {
                        println!("  loop: {} {:?} back at {key} day {} {:02}:{:02}", c.name, id, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60);
                    }
                }
            }
        }
        for (id, (key, kind, since, plat)) in &waiting {
            if now.get(id).is_some_and(|n| &n.0 == key && n.2 == *since) {
                continue;
            }
            let Some(c) = w.citizens.get(*id) else { continue };
            let how = match &c.activity {
                Activity::Riding { .. } | Activity::OnMetro { .. } | Activity::OnTrain { .. } | Activity::Flying { .. } => {
                    *t.boardings.entry(kind).or_default() += 1;
                    left_stops.remove(id);
                    continue;
                }
                Activity::Passenger { .. } => "lift",
                Activity::Waiting { .. } if now.get(id).is_some_and(|n| &n.0 == key) => "wait again (since reset)",
                Activity::Waiting { .. } => "waits elsewhere",
                Activity::Traveling { .. } if c.transit.is_some() => "set off again by transit",
                Activity::Traveling { .. } => "set off another way",
                Activity::Driving { .. } => "drove",
                _ => "other",
            };
            *t.left.entry((kind, how)).or_default() += 1;
            if how == "set off another way" {
                *t.why.entry(format!("{kind} {:02}h {}", w.clock.hour(), c.thought)).or_default() += 1;
            }
            if *plat && matches!(c.activity, Activity::Traveling { .. }) {
                let start = match &c.activity {
                    Activity::Traveling { path, .. } => path.first().copied().unwrap_or(c.pos),
                    _ => c.pos,
                };
                if platforms.iter().any(|p| p.distance(start) < 1.0 || p.distance(c.prev_pos) < 1.0) {
                    t.from_platform += 1;
                    if let (Ok(near), Activity::Traveling { path, .. }) = (std::env::var("TRACE"), &c.activity) {
                        let v: Vec<f32> = near.split(',').filter_map(|x| x.parse().ok()).collect();
                        if t.from_platform < 400 && start.distance(glam::Vec2::new(v[0], v[1])) < 30.0 {
                            let pts: Vec<String> = path.iter().enumerate().take(30).map(|(k, q)| {
                                let from = if k > 0 { path[k - 1] } else { c.pos };
                                format!("({:.0},{:.0}){}", q.x, q.y, w.way_height_on_leg(from, *q, (from + *q) * 0.5).map_or(String::new(), |z| format!("^{z:.0}")))
                            }).collect();
                            println!("  {} from ({:.0},{:.0}) path {} pts: {}", c.name, start.x, start.y, path.len(), pts.join(" "));
                        }
                    }
                }
            }
            if Some(*id) == watched {
                println!("  {} left {key} ({how}) day {} {:02}:{:02} waited {} min", c.name, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, tick.saturating_sub(*since));
            }
            left_stops.entry(*id).or_default().push(key.clone());
        }
        waiting = now;
        // Door to door, home to work.
        for (id, c) in w.citizens.iter() {
            let before = inside.get(&id).copied().flatten();
            if before == c.inside {
                continue;
            }
            if before.is_some() && before == c.home && c.inside.is_none() {
                left_home.insert(id, tick);
                left_stops.remove(&id);
            }
            if c.inside.is_some() && c.inside == c.work {
                if let Some(t0) = left_home.remove(&id) {
                    t.commutes.push((tick - t0) as f32);
                }
            }
            if c.inside.is_some() {
                left_stops.remove(&id);
            }
            inside.insert(id, c.inside);
        }
        if tick % (6 * 60) == 0 {
            println!("day {} {:02}:00 waiting {} boardings {:?} loops {:?}", w.clock.day(), w.clock.hour(), waiting.len(), t.boardings, t.loops);
        }
    }
    println!("== {hours} h from the save");
    println!("boardings {:?}", t.boardings);
    for ((k, how), n) in &t.left {
        println!("left the wait without boarding: {k:6} {how:28} {n}");
    }
    let mut why: Vec<_> = t.why.iter().collect();
    why.sort_by(|a, b| b.1.cmp(a.1));
    for (k, n) in why.iter().take(15) {
        println!("   {n:5} {k}");
    }
    println!("set off again from a platform point: {}", t.from_platform);
    println!("back at a stop they had left (same trip): {:?}, people {}", t.loops, t.loopers.len());
    println!("failed trips: {}", t.failed);
    let n = t.commutes.len().max(1) as f32;
    let mut s = t.commutes.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    println!("home->work commutes {} mean {:.1} min median {:.0} p90 {:.0}", t.commutes.len(), t.commutes.iter().sum::<f32>() / n, s.get(s.len() / 2).copied().unwrap_or(0.0), s.get(s.len() * 9 / 10).copied().unwrap_or(0.0));
    println!("trips begun yesterday by mode {:?}", w.vehicles.stats.trips_yesterday);
}
