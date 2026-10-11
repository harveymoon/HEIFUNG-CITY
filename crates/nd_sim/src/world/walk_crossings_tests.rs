//! [walk-crossings] Harvey's flags G3, G4, G7, G11, G13, G17, G20: people
//! jaywalked beside footbridges, waited at crossings the bridge had taken
//! away, crossed highway ramps at grade, and the crosswalk tool could not
//! take a crossing away. Footbridges now win nearly every crossing near
//! them, walkers already on their way plan again, cyclists wheel their
//! bikes over, ramps are crossed only at proper crossings, a crosswalk can
//! be removed, and buses carry four bikes on their racks.

use super::*;
use crate::citizens::TravelMode;
use crate::junctions::{CrossingUpgrade, JunctionRule, SignalMode};
use crate::mind::IdleMind;
use crate::transit::{TransitLeg, TransitTrip, BUS_BIKE_RACK};
use crate::walk::CrossingKind;

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// A busy avenue (two lanes each way) along y = 0 with homes built out on
/// both sides, people living there, all of them indoors and still, and no
/// cars: only the walkers a test sets off move. Returns the world and the
/// avenue's first mid-block zebra.
fn avenue_town() -> (World, u32) {
    let mut w = flat();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
    w.add_zone(square(-220.0, 10.0, 440.0, 60.0), LandUse::Residential);
    w.add_zone(square(-220.0, -70.0, 440.0, 60.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for lot in lots {
        w.build_on(lot);
    }
    finish_all(&mut w);
    let mut guard = 0;
    while w.citizens.len() < 12 && guard < 48 {
        run(&mut w, 60);
        guard += 1;
    }
    assert!(w.citizens.len() >= 12, "people moved in: {}", w.citizens.len());
    still(&mut w);
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra).expect("a mid-block zebra") as u32;
    (w, cr)
}

/// Everyone indoors, deciding nothing; no cars.
fn still(w: &mut World) {
    for c in w.citizens.values_mut() {
        if let Some(h) = c.home {
            c.activity = Activity::AtHome;
            c.inside = Some(h);
        }
        c.next_decision_tick = u64::MAX;
    }
    let cars: Vec<CarId> = w.traffic.cars.keys().collect();
    for c in cars {
        w.traffic.remove_car(c);
    }
    w.peds.walkers.clear();
}

/// Seeds whose temperament jaywalks as often as anyone's does.
fn rebel_seeds(n: usize) -> Vec<u64> {
    (1u64..)
        .filter(|&s| {
            let (c, h) = crate::citizens::driver_traits(s);
            crate::pedestrians::jaywalk_chance(30, c, h, 0.0, false) >= 0.4
        })
        .take(n)
        .collect()
}

/// Send `id` out on foot from `from` to `to`, choosing their route as
/// they would (a jaywalker may plan to cut across).
fn send(w: &mut World, id: CitizenId, seed: Option<u64>, from: Vec2, to: Vec2) -> bool {
    {
        let c = &mut w.citizens[id];
        if let Some(s) = seed {
            c.seed = s;
        }
        c.age = 30;
        c.drunk = 0.0;
        c.inside = None;
        c.pos = from;
        c.prev_pos = from;
    }
    w.peds.walkers.remove(&id);
    let Some((path, _)) = w.walk_route_for(id, from, to, Purpose::Wander) else { return false };
    let c = &mut w.citizens[id];
    c.mode = TravelMode::Walk;
    c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
    true
}

fn walking(w: &World) -> usize {
    w.citizens.values().filter(|c| matches!(c.activity, Activity::Traveling { .. })).count()
}

/// The avenue's crossings within `r` of `at`: walkers at grade and over
/// footbridges.
fn uses_near(w: &World, at: Vec2, r: f32) -> (u32, u32) {
    let mut grade = 0;
    let mut bridge = 0;
    for (&k, u) in &w.peds.uses {
        let c = &w.walk.crossings[k as usize];
        if c.mid.distance(at) <= r && w.roads.segment(c.segment).lanes_fwd == 2 {
            grade += u.at_grade;
            bridge += u.bridge;
        }
    }
    (grade, bridge)
}

/// G4, G7, G17: with a footbridge over a busy avenue, nearly everyone
/// crossing near it goes over the bridge, the rebellious included: the
/// jaywalking short cuts beside it are gone and the crosswalks it
/// replaced are closed.
#[test]
fn a_footbridge_beside_a_busy_avenue_takes_nearly_every_crossing() {
    let (mut w, cr) = avenue_town();
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let mid = w.walk.crossings[cr as usize].mid;
    assert!(w.walk.crossings[cr as usize].closed, "the bridge replaced its zebra");
    // No jaywalk planned within the bridge's shadow.
    for c in w.walk.crossings.iter().filter(|c| c.kind == CrossingKind::Jay && c.mid.distance(mid) < crate::walk::JAY_BRIDGE_SHADOW) {
        assert!(c.closed, "a jaywalk {:.0} m from the bridge", c.mid.distance(mid));
    }
    // Every route between the two sides near the bridge, jaywalker or not.
    let mut over = 0;
    let mut total = 0;
    for dx in [-40.0f32, -20.0, 0.0, 20.0, 40.0] {
        for dx2 in [-30.0f32, 0.0, 30.0] {
            for jay in [false, true] {
                let (a, b) = (mid + Vec2::new(dx, 25.0), mid + Vec2::new(dx2, -25.0));
                let Some((path, nodes)) = w.walk.route_with(a, b, jay) else { continue };
                total += 1;
                if nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0) {
                    over += 1;
                } else {
                    let at_grade: Vec<Vec2> = path.windows(2).filter(|p| w.walk.crossing_between(p[0], p[1]).is_some()).map(|p| p[0]).collect();
                    assert!(at_grade.iter().all(|p| p.distance(mid) > 60.0), "crossed at grade near the bridge: {at_grade:?}");
                }
            }
        }
    }
    assert!(total >= 24 && over * 10 >= total * 9, "over the bridge {over} of {total}");
    // Now people walking it, half of them rebels: nearly every crossing of
    // the avenue near the bridge is over it.
    let ids: Vec<CitizenId> = w.citizens.keys().collect();
    let seeds = rebel_seeds(ids.len());
    let mut sent = 0;
    for (i, &id) in ids.iter().enumerate() {
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        let from = mid + Vec2::new(-50.0 + (i as f32 * 17.0) % 100.0, 30.0 * side);
        let to = mid + Vec2::new(50.0 - (i as f32 * 29.0) % 100.0, -30.0 * side);
        if send(&mut w, id, (i % 2 == 0).then_some(seeds[i]), from, to) {
            sent += 1;
        }
    }
    assert!(sent >= 10, "walkers sent: {sent}");
    for _ in 0..200 {
        run(&mut w, 1);
        if walking(&w) == 0 {
            break;
        }
    }
    let (grade, bridge) = uses_near(&w, mid, 70.0);
    assert!(bridge >= 10, "walkers over the bridge: {bridge}");
    assert!(bridge * 10 >= (grade + bridge) * 9, "over the bridge {bridge}, at grade {grade}");
    assert_eq!(w.peds.jaywalkers_today, 0, "nobody jaywalked beside the bridge");
}

/// G17: someone already on their way to a crosswalk when it becomes a
/// footbridge plans again at the kerb and goes over the bridge, rather
/// than waiting there for a gap and running across.
#[test]
fn walkers_on_their_way_plan_again_when_a_crossing_becomes_a_footbridge() {
    let (mut w, cr) = avenue_town();
    let c = w.walk.crossings[cr as usize].clone();
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    let id = w.citizens.keys().next().unwrap();
    let from = a + (a - b).normalize() * 20.0 + Vec2::new(30.0, 0.0);
    let to = b + (b - a).normalize() * 20.0 + Vec2::new(30.0, 0.0);
    assert!(send(&mut w, id, None, from, to));
    let Activity::Traveling { path, .. } = &w.citizens[id].activity else { panic!("walking") };
    assert!(path.windows(2).any(|p| w.walk.crossing_between(p[0], p[1]) == Some(cr)), "planned over the zebra");
    run(&mut w, 1);
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let mut up = 0.0f32;
    for _ in 0..120 {
        run(&mut w, 1);
        let ci = &w.citizens[id];
        up = up.max(w.walker_level(ci, ci.pos));
        if !matches!(ci.activity, Activity::Traveling { .. }) {
            break;
        }
    }
    assert!(w.peds.replans_total >= 1, "planned again");
    assert!(up > 3.0, "went over the bridge: {up}");
    assert_eq!(w.peds.uses.get(&cr).map_or(0, |u| u.at_grade), 0, "nobody crossed at grade under the bridge");
    assert!(!matches!(w.citizens[id].activity, Activity::Traveling { .. }), "arrived");
}

/// G4: a cyclist whose way crosses the street where a footbridge replaced
/// the crossing wheels the bike over the bridge.
#[test]
fn a_cyclist_wheels_the_bike_over_a_footbridge() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    w.add_path(&[Vec2::new(0.0, 120.0), Vec2::new(0.0, 0.0)]);
    w.add_path(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, -120.0)]);
    w.add_zone(square(-150.0, 10.0, 140.0, 60.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for lot in lots.into_iter().take(4) {
        w.build_on(lot);
    }
    finish_all(&mut w);
    let mut guard = 0;
    while w.citizens.is_empty() && guard < 48 {
        run(&mut w, 60);
        guard += 1;
    }
    still(&mut w);
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra && c.mid.distance(Vec2::ZERO) < 3.0).expect("the path's zebra") as u32;
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    // The bike's way: down the path, over the street, on down the path.
    let from = w.roads.nodes.iter().position(|n| n.pos.distance(Vec2::new(0.0, 120.0)) < 1.0).map(|i| NodeId(i as u32)).unwrap();
    let to = w.roads.nodes.iter().position(|n| n.pos.distance(Vec2::new(0.0, -120.0)) < 1.0).map(|i| NodeId(i as u32)).unwrap();
    let nodes = w.roads.foot_path(from, to).expect("a way along the path");
    let mut path = w.route_path(&nodes, TravelMode::Bike);
    w.bike_over_footbridges(&mut path);
    assert!(path.iter().any(|&p| w.walk.level_at_node(p) > 0.0), "over the bridge: {path:?}");
    let id = w.citizens.keys().next().unwrap();
    {
        let c = &mut w.citizens[id];
        c.inside = None;
        c.mode = TravelMode::Bike;
        c.pos = path[0];
        c.prev_pos = path[0];
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
    }
    let mut up = 0.0f32;
    for _ in 0..120 {
        run(&mut w, 1);
        let c = &w.citizens[id];
        up = up.max(w.walker_level(c, c.pos));
        if !matches!(c.activity, Activity::Traveling { .. }) {
            break;
        }
    }
    assert!(up > 3.0, "the rider went up onto the bridge: {up}");
    assert_eq!(w.peds.uses.get(&cr).map_or(0, |u| u.bridge_bikes), 1, "counted over the bridge");
}

/// The crossroads of a two-lane street and a highway's slip roads: an
/// eastbound highway at y = 0, a street at y = -160 with an off-ramp down
/// to it and an on-ramp back up, homes and shops south of the street.
fn interchange() -> World {
    let mut w = flat();
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 3, 0);
    w.add_road(&[Vec2::new(-600.0, -160.0), Vec2::new(600.0, -160.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, -5.0), Vec2::new(-150.0, -60.0), Vec2::new(-80.0, -160.0)], 1, 0);
    w.add_road(&[Vec2::new(80.0, -160.0), Vec2::new(150.0, -60.0), Vec2::new(300.0, -5.0)], 1, 0);
    w.add_road(&[Vec2::new(0.0, -160.0), Vec2::new(0.0, -420.0)], 1, 1);
    w.add_zone(square(-560.0, -400.0, 540.0, 225.0), LandUse::Residential);
    w.add_zone(square(20.0, -400.0, 540.0, 225.0), LandUse::Commercial);
    // A pavement north of the street too, between the ramps.
    w.add_zone(square(-60.0, -150.0, 120.0, 40.0), LandUse::Commercial);
    w
}

/// G11: nobody walks over a slip road's mouth except at a proper crossing:
/// shut at a junction without signals, open with the junction's walk
/// lights once it has them, and a footbridge can be put over it.
#[test]
fn no_walker_crosses_a_highway_ramp_except_at_a_proper_crossing() {
    let mut w = interchange();
    run(&mut w, 2);
    let ramps: Vec<u32> = w.walk.crossings.iter().enumerate().filter(|(_, c)| c.ramp).map(|(k, _)| k as u32).collect();
    assert!(!ramps.is_empty(), "the slip roads' mouths are crossings");
    for &k in &ramps {
        let c = &w.walk.crossings[k as usize];
        let signals = c.node.and_then(|n| w.traffic.net.node_intersection.get(n.0 as usize).copied()).and_then(|i| w.traffic.net.intersections.get(i)).is_some_and(|i| i.signalized);
        // Only an off-ramp's traffic waits at a red for the walkers.
        let sg = w.roads.segment(c.segment);
        let n = c.node.unwrap();
        let leaving = if sg.b == n { sg.lanes_bwd } else { sg.lanes_fwd };
        assert_eq!(c.shut, !(signals && leaving == 0), "a ramp mouth is open only with walk lights, and never an on-ramp's");
    }
    // The on-ramp's mouth is shut even with signals.
    let on = ramps.iter().copied().find(|&k| w.walk.crossings[k as usize].mid.x > 0.0).expect("the on-ramp's mouth");
    let on_node = w.walk.crossings[on as usize].node.unwrap();
    let mut rule = JunctionRule::new(w.roads.node(on_node).pos);
    rule.signal = SignalMode::Signals;
    w.set_junction_rule(rule).unwrap();
    assert!(w.walk.crossings[on as usize].shut, "nobody walks over an on-ramp's mouth, signals or not");
    // No route anywhere crosses a shut ramp mouth.
    let pts: Vec<Vec2> = (0..12).map(|i| Vec2::new(-500.0 + i as f32 * 90.0, if i % 2 == 0 { -175.0 } else { -145.0 })).collect();
    for &a in &pts {
        for &b in &pts {
            let Some((path, _)) = w.walk.route_with(a, b, true) else { continue };
            for p in path.windows(2) {
                if let Some(k) = w.walk.crossing_between(p[0], p[1]) {
                    let c = &w.walk.crossings[k as usize];
                    assert!(!(c.ramp && c.shut), "a route over a ramp's mouth with no walk light at {:?}", c.mid);
                }
            }
        }
    }
    // Signals at the off-ramp's junction: its mouth opens, with walk lights.
    let k = ramps.iter().copied().find(|&k| w.walk.crossings[k as usize].mid.x < 0.0).expect("the off-ramp's mouth");
    let node = w.walk.crossings[k as usize].node.unwrap();
    let at = w.roads.node(node).pos;
    let mut rule = JunctionRule::new(at);
    rule.signal = SignalMode::Signals;
    w.set_junction_rule(rule).unwrap();
    let k = w.walk.crossings.iter().position(|c| c.ramp && c.node == Some(node)).unwrap();
    assert!(!w.walk.crossings[k].shut, "open with the junction's walk lights");
    assert!(w.peds.crossings[k].signal.is_some(), "and lit by them");
    // Signals off again, and a footbridge over the mouth instead: its
    // stairs stand on the streets' pavements, not on the ramp.
    let mut rule = JunctionRule::new(at);
    rule.signal = SignalMode::Unsignalised;
    w.set_junction_rule(rule).unwrap();
    assert!(w.walk.crossings[k].shut, "shut again without signals");
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(k as u32, CrossingUpgrade::Footbridge).unwrap();
    let fb = w.walk.footbridges.iter().find(|f| w.walk.crossings[f.crossing as usize].ramp).expect("a footbridge over the ramp's mouth");
    for p in fb.foot {
        assert!(!w.stranded_on_motorway(p) && !w.in_carriageway(p), "a stair foot out in the road at {p:?}");
    }
}

/// G3 and Harvey's list: "remove crosswalk" takes a crossing away; walkers
/// go round by another, nothing is painted there, and nobody jaywalks
/// right beside it.
#[test]
fn a_removed_crosswalk_blocks_crossing_there() {
    let (mut w, cr) = avenue_town();
    let c = w.walk.crossings[cr as usize].clone();
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    let (from, to) = (a + (a - b).normalize() * 15.0, b + (b - a).normalize() * 15.0);
    let (before, _) = w.walk.route(from, to).expect("a way across");
    assert!(before.windows(2).any(|p| w.walk.crossing_between(p[0], p[1]) == Some(cr)), "across the zebra");
    w.set_crossing_upgrade(cr, CrossingUpgrade::Removed).unwrap();
    let k = w.walk.crossings.iter().position(|x| x.mid.distance(c.mid) < 0.5 && x.kind == CrossingKind::Zebra).unwrap() as u32;
    assert!(w.walk.crossings[k as usize].closed);
    assert!(w.closed_crosswalks().iter().any(|m| m.distance(c.mid) < 0.5), "not painted");
    let v = w.intersection_view(c.mid).expect("the crosswalk opens in the editor");
    assert!(v.crossings.iter().any(|x| x.index == k && x.upgrade == CrossingUpgrade::Removed));
    for jay in [false, true] {
        let (after, _) = w.walk.route_with(from, to, jay).expect("still a way across, round by another crossing");
        for p in after.windows(2) {
            if let Some(x) = w.walk.crossing_between(p[0], p[1]) {
                let x = &w.walk.crossings[x as usize];
                assert!(x.mid.distance(c.mid) > crate::walk::JAY_REMOVED_SHADOW - 0.5, "crossed {:.0} m from the removed crosswalk", x.mid.distance(c.mid));
            }
        }
    }
    // Saved and loaded, it stays removed; back to plain, it is back.
    let mut w2 = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(w2.walk.crossings[k as usize].closed, "still removed after a reload");
    w2.set_crossing_upgrade(k, CrossingUpgrade::None).unwrap();
    assert!(!w2.walk.crossings[k as usize].closed, "a plain crosswalk again");
}

/// Harvey's list: each bus carries four bikes on its front rack. Six
/// cyclists with bikes wait at a stop: four board, two wait for the next
/// bus, which takes them.
#[test]
fn buses_carry_four_bikes_and_the_rest_wait_for_the_next() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 140.0), Vec2::new(400.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(300.0, 0.0), Vec2::new(300.0, 140.0)], 1, 1);
    w.add_zone(square(-390.0, 10.0, 80.0, 120.0), LandUse::Residential);
    w.add_zone(square(310.0, 10.0, 80.0, 120.0), LandUse::Commercial);
    w.add_zone(square(-100.0, -150.0, 200.0, 140.0), LandUse::Industrial);
    let route = w.add_bus_route();
    for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(296.0, 70.0), Vec2::new(200.0, 136.0)] {
        w.add_bus_stop(route, p).expect("stop snaps to the street");
    }
    let depot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= crate::transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
    w.build_depot(depot).expect("depot built");
    let home = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
    w.build_on(home);
    finish_all(&mut w);
    let mut guard = 0;
    while (w.citizens.is_empty() || !w.traffic.cars.values().any(|c| c.bus.is_some())) && guard < 48 {
        run(&mut w, 60);
        guard += 1;
    }
    let bus = w.traffic.cars.iter().find(|(_, c)| c.bus.is_some()).map(|(id, _)| id).expect("a bus is out");
    let template = w.citizens.values().next().expect("someone moved in").clone();
    let stop = w.transit.routes[route].stops[0].pos;
    let target = w.buildings.keys().next().unwrap();
    let riders: Vec<CitizenId> = (0..6)
        .map(|i| {
            w.citizens.insert(Citizen {
                home: None,
                work: None,
                household: None,
                car: None,
                seed: 9000 + i,
                inside: None,
                pos: stop,
                prev_pos: stop,
                activity: Activity::Waiting { at: stop },
                transit: Some(TransitTrip { leg: TransitLeg::Bus { route, board: 0, alight: 2 }, target, purpose: Purpose::Wander, since: 1, bike: true }),
                next_decision_tick: u64::MAX,
                ..template.clone()
            })
        })
        .collect();
    // The bus pulls in at the stop, empty (the waiting list is read afresh).
    let call = |w: &mut World| {
        w.waiting_cache = None;
        let b = w.traffic.cars[bus].bus.as_mut().unwrap();
        b.next_stop = 0;
        b.to_depot = false;
        b.passengers.clear();
        w.bus_arrived(bus);
    };
    call(&mut w);
    let aboard = |w: &World| riders.iter().filter(|r| matches!(w.citizens[**r].activity, Activity::Riding { .. })).count();
    assert_eq!(aboard(&w), BUS_BIKE_RACK, "four bikes on the rack");
    assert_eq!(riders.iter().filter(|r| matches!(w.citizens[**r].activity, Activity::Waiting { .. })).count(), 2, "two wait for the next bus");
    assert_eq!(w.transit.bike_rack_full_total, 2);
    // The next bus (the same one, come round again with its rack empty).
    for r in &riders {
        if matches!(w.citizens[*r].activity, Activity::Riding { .. }) {
            w.citizens[*r].activity = Activity::Idle;
            w.citizens[*r].transit = None;
        }
    }
    call(&mut w);
    assert_eq!(aboard(&w), 2, "the two left behind board the next bus");
    assert_eq!(w.transit.bike_rack_rides_total, 6);
}

/// A cyclist on a long ride may choose the bus with the bike on its rack.
#[test]
fn a_cyclist_facing_a_long_ride_may_put_the_bike_on_the_bus() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-800.0, 0.0), Vec2::new(800.0, 0.0)], 1, 1);
    w.add_zone(square(-780.0, 10.0, 100.0, 60.0), LandUse::Residential);
    w.add_zone(square(680.0, 10.0, 100.0, 60.0), LandUse::Commercial);
    w.add_zone(square(-100.0, -150.0, 200.0, 140.0), LandUse::Industrial);
    let route = w.add_bus_route();
    for p in [Vec2::new(-730.0, 4.0), Vec2::new(730.0, 4.0)] {
        w.add_bus_stop(route, p).expect("a stop");
    }
    let depot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= crate::transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
    w.build_depot(depot).expect("depot built");
    let home = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
    let shop = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).map(|(id, _)| id).unwrap();
    w.build_on(home);
    w.build_on(shop);
    finish_all(&mut w);
    let mut guard = 0;
    while w.citizens.is_empty() && guard < 48 {
        run(&mut w, 60);
        guard += 1;
    }
    let id = w.citizens.keys().next().expect("someone moved in");
    w.citizens[id].owns_bike = true;
    let q = crate::world::trips::TripQuery { from: w.lots[shop].entrance(), to: w.lots[home].entrance(), target: w.lots[home].building.expect("a home"), purpose: Purpose::Home, allow_transit: true, allow_car: false, allow_bike: true };
    let opts = w.mode_options(id, &q);
    let with_bike = opts.iter().find(|o| matches!(o.plan, crate::world::trips::ModePlan::Bus { bike: true, .. })).expect("the bus with the bike is an option");
    let ride = opts.iter().find(|o| matches!(o.plan, crate::world::trips::ModePlan::Bike)).expect("riding all the way is one");
    assert!(with_bike.comfort < ride.comfort, "the rack spares the long slog: {} vs {}", with_bike.comfort, ride.comfort);
}

/// G4: somebody sleeping rough who ends up in a street's lanes is moved to
/// the pavement and never lies down in the road (where they looked hurt).
#[test]
fn nobody_lies_about_in_a_streets_lanes() {
    let (mut w, _) = avenue_town();
    let id = w.citizens.keys().next().unwrap();
    let p = Vec2::new(30.0, 2.0);
    assert!(w.in_carriageway(p), "the avenue's lanes");
    {
        let c = &mut w.citizens[id];
        c.inside = None;
        c.pos = p;
        c.prev_pos = p;
        c.activity = Activity::Idle;
    }
    run(&mut w, crate::world::kerbside::STRANDED_SWEEP_EVERY as usize + 1);
    let c = &w.citizens[id];
    assert!(!w.in_carriageway(c.pos), "still in the road at {:?}", c.pos);
}
