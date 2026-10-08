//! Regressions from review 2: what is keyed by id across a plan rebuild.
//!
//! M3: every rebuild cuts lots afresh and a fresh SlotMap hands out the
//! same keys in a new order, so a `LotId` kept elsewhere (rubble, camps,
//! footfall owed to shops) silently named a different lot afterwards.
//! M8: player lane arrows were keyed by raw segment and node ids, which a
//! rebuild renumbers: removing an unrelated earlier road lost them.

use nd_sim::traffic::{LEFT, RIGHT, STRAIGHT};
use nd_sim::{LandUse, Vec2, World, WorldConfig};

fn grid_town() -> World {
    let mut w = World::new(WorldConfig::default());
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        w.add_road(&[Vec2::new(-280.0, c), Vec2::new(280.0, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -280.0), Vec2::new(c, 280.0)], 1, 1);
    }
    w.add_zone(vec![Vec2::new(-275.0, -275.0), Vec2::new(275.0, -275.0), Vec2::new(275.0, 275.0), Vec2::new(-275.0, 275.0)], LandUse::Residential);
    w
}

#[test]
fn rubble_stays_on_its_lot_through_road_works() {
    let mut w = grid_town();
    let (lot, center) = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, l)| (id, l.center)).nth(5).expect("a lot");
    w.services.rubble.push((lot, center, w.clock.tick + 100_000));
    // Unrelated road works far from it, then a road through the middle.
    w.add_road(&[Vec2::new(-280.0, 70.0), Vec2::new(-140.0, 70.0)], 1, 1);
    w.add_road(&[Vec2::new(-280.0, -200.0), Vec2::new(280.0, -200.0)], 1, 1);
    let real = w.lots.iter().find(|(_, l)| l.contains(center)).map(|(id, _)| id);
    let (key, at, _) = w.services.rubble[0];
    assert_eq!(Some(key), real, "the rubble names the lot under it");
    assert!(w.lots[key].contains(at));
    assert!(w.services.is_rubble(real.unwrap()));
    assert_eq!(w.services.rubble.len(), 1);
}

#[test]
fn lane_arrows_stay_on_their_junction_when_ids_change() {
    let mut w = World::new(WorldConfig::default());
    let r0 = w.add_road(&[Vec2::new(-300.0, 300.0), Vec2::new(-100.0, 300.0)], 1, 1);
    let _ = w.add_road(&[Vec2::new(-200.0, 300.0), Vec2::new(-200.0, 400.0)], 1, 1);
    w.add_road(&[Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, -150.0), Vec2::new(0.0, 150.0)], 2, 2);
    // The west approach of the cross at the origin.
    let west = |w: &World| {
        w.traffic.net.approaches.iter().position(|ap| {
            let n = w.roads.node(ap.node).pos;
            let (a, b) = w.roads.segment_points(ap.segment);
            let far = if a.distance(n) < b.distance(n) { b } else { a };
            n.length() < 1.0 && far.x < -50.0
        })
    };
    let ai = west(&w).expect("west approach");
    let (seg, node) = (w.traffic.net.approaches[ai].segment, w.traffic.net.approaches[ai].node);
    let arrows = vec![LEFT, STRAIGHT | RIGHT];
    w.traffic.set_lane_use(seg, node, Some(arrows.clone())).unwrap();
    w.remove_gesture(r0);
    w.implement();
    let ai = west(&w).expect("west approach after");
    let ap = &w.traffic.net.approaches[ai];
    assert!(ap.segment != seg || ap.node != node, "the rebuild renumbered the approach (else this tests nothing)");
    assert!(ap.fixed, "the player's arrows are still set");
    assert_eq!(ap.arrows, arrows);
    assert_eq!(w.traffic.lane_use.len(), 1);
    assert_eq!((w.traffic.lane_use[0].segment, w.traffic.lane_use[0].node), (ap.segment, ap.node));
    // No other approach picked the rule up by a reused id.
    assert_eq!(w.traffic.net.approaches.iter().filter(|a| a.fixed).count(), 1);
    // And through a save it stays.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let ai = west(&back).unwrap();
    assert!(back.traffic.net.approaches[ai].fixed && back.traffic.net.approaches[ai].arrows == arrows);
}

/// H1: a car on a street that a new junction splits goes on from the new
/// lane under it to the same destination; a car on a street that is
/// removed is the only kind taken off the road.
#[test]
fn a_car_on_a_split_street_drives_on_and_only_removed_streets_lose_cars() {
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    let side = w.add_road(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
    let spawn = |w: &mut World, y: f32| {
        let (from, _, _) = w.roads.nearest_street(Vec2::new(-300.0, y)).unwrap();
        let (to, _, _) = w.roads.nearest_street(Vec2::new(300.0, y)).unwrap();
        let (route, s0, s1) = w.traffic.net.find_route(from, Vec2::new(-300.0, y), to, Vec2::new(300.0, y)).unwrap();
        let id = w.traffic.spawn_car(nd_sim::CitizenId::default(), None, route, s0, s1, 0);
        let c = &mut w.traffic.cars[id];
        // Mid-street, well along its way.
        let lane = c.route[0];
        c.s = 250.0;
        let (p, d) = w.traffic.net.lanes[lane as usize].point_at(250.0);
        c.pos = p;
        c.heading = d.y.atan2(d.x);
        id
    };
    let main = spawn(&mut w, 0.0);
    let doomed = spawn(&mut w, 200.0);
    let (pos, goal) = {
        let c = &w.traffic.cars[main];
        (c.pos, w.traffic.net.lanes[*c.route.last().unwrap() as usize].point_at(c.s_end).0)
    };
    // A cross street through the main street just behind the car, and the
    // side street removed, in one project.
    w.start_project();
    w.add_gesture(nd_sim::Gesture::Road { points: vec![Vec2::new(-100.0, -100.0), Vec2::new(-100.0, 100.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() });
    w.remove_gesture(side);
    w.try_implement().unwrap();
    let c = w.traffic.cars.get(main).expect("still on the road");
    assert!(c.pos.distance(pos) < 1.0);
    let lane = &w.traffic.net.lanes[c.route[c.idx] as usize];
    assert!(lane.project(pos).1 < 1.0, "on the lane under it");
    let end = w.traffic.net.lanes[*c.route.last().unwrap() as usize].point_at(c.s_end).0;
    assert!(end.distance(goal) < 2.0, "bound for the same place");
    assert!(lane.cars.contains(&main));
    assert!(!w.traffic.cars.contains_key(doomed), "its street is gone");
}

/// A real save (a copy; `ND_SAVE=path`): loads, takes a zoning edit with
/// every vehicle left in place, then a road edit with nearly every vehicle
/// carried across, and runs on. Ignored by default: it needs the file.
/// `ND_SAVE=... cargo test --release -p nd_sim --test rebuild_regressions -- --ignored --nocapture`
#[test]
#[ignore]
fn a_saved_city_keeps_its_traffic_through_plan_edits() {
    // [review] Run by hand (`--ignored`) it must test something: without
    // the file it fails with the hint instead of passing quietly.
    let path = std::env::var("ND_SAVE").expect("set ND_SAVE to a save to run this (see the doc comment)");
    let text = std::fs::read_to_string(&path).expect("read the save");
    let mut w = World::from_ron(&text).expect("the save loads");
    let mut minds = nd_minds::RuleBasedMind::default();
    for _ in 0..30 {
        w.tick(&mut minds);
    }
    let before: Vec<_> = w.traffic.cars.iter().map(|(id, c)| (id, c.pos)).collect();
    // Zone a small block out in the country, away from everything.
    let far = (-800..=800).step_by(100).flat_map(|x| (-800..=800).step_by(100).map(move |y| Vec2::new(x as f32, y as f32))).find(|p| w.roads.nearest_on_network(*p).is_none_or(|(_, _, q)| q.distance(*p) > 120.0)).expect("open land");
    w.add_zone(vec![far, far + Vec2::new(30.0, 0.0), far + Vec2::new(30.0, 30.0), far + Vec2::new(0.0, 30.0)], LandUse::Residential);
    let after: Vec<_> = w.traffic.cars.iter().map(|(id, c)| (id, c.pos)).collect();
    println!("zone edit: {} -> {} vehicles", before.len(), after.len());
    assert_eq!(before, after);
    // A short street out there too (no vehicle is on it).
    w.add_road(&[far + Vec2::new(-40.0, -20.0), far + Vec2::new(70.0, -20.0)], 1, 1);
    let carried = before.iter().filter(|(id, _)| w.traffic.cars.contains_key(*id)).count();
    println!("road edit: {carried} of {} carried", before.len());
    assert!(carried * 100 >= before.len() * 95);
    for _ in 0..1440 {
        w.tick(&mut minds);
    }
}
