//! [bridge-riders] Harvey's flag (day 46, Zenith Circus): "i don't see
//! anyone in the road, everyone is taking the footpath", yet the cars under
//! the footbridge braked "for someone crossing the road". The bikes wheeled
//! over the bridge's deck (a step beside the old crosswalk) were taken to
//! be on the crosswalk under it. Nobody above a street (a footbridge, an
//! elevated path) or below it holds its traffic up.

use super::*;
use crate::citizens::TravelMode;
use crate::junctions::CrossingUpgrade;
use crate::mind::IdleMind;
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

/// A few homes north-west of the avenue with people in them, everyone
/// indoors and still, and no cars.
fn people(w: &mut World) {
    w.add_zone(square(-150.0, 10.0, 100.0, 60.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for lot in lots.into_iter().take(4) {
        w.build_on(lot);
    }
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
    let mut guard = 0;
    while w.citizens.len() < 2 && guard < 48 {
        run(w, 60);
        guard += 1;
    }
    assert!(w.citizens.len() >= 2, "people moved in: {}", w.citizens.len());
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
    w.peds.riders.clear();
}

fn node_at(w: &World, p: Vec2) -> NodeId {
    w.roads.nodes.iter().position(|n| n.pos.distance(p) < 1.0).map(|i| NodeId(i as u32)).expect("a road node there")
}

/// `id` sets off on `path` by `mode`.
fn set_off(w: &mut World, id: CitizenId, mode: TravelMode, path: Vec<Vec2>) {
    w.peds.walkers.remove(&id);
    w.peds.riders.remove(&id);
    let c = &mut w.citizens[id];
    c.inside = None;
    c.mode = mode;
    c.pos = path[0];
    c.prev_pos = path[0];
    c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
}

/// A car driving along the avenue through `at`, from `back` metres before it.
fn car_through(w: &mut World, at: Vec2, east: bool, back: f32) -> CarId {
    let d = if east { Vec2::X } else { -Vec2::X };
    let (from, to) = (at - d * back + Vec2::new(0.0, if east { -3.0 } else { 3.0 }), at + d * 150.0);
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, from, ts, to).expect("a route along the avenue");
    w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
}

/// Is any driver told to stop for crossing `cr` this tick?
fn held(w: &World, cr: u32) -> bool {
    w.traffic.peds.holds.values().chain(w.traffic.peds.watch.values()).flatten().any(|h| h.crossing == cr)
}

fn stops_for_people(w: &World, car: CarId) -> bool {
    w.traffic.cars.contains_key(car) && matches!(w.traffic.stop_reason(car), crate::traffic::StopReason::Pedestrians { .. } | crate::traffic::StopReason::Jaywalker { .. })
}

/// A cyclist wheeling the bike over a footbridge is never "on" the
/// crosswalk the bridge replaced, so the cars passing under it drive on.
#[test]
fn a_rider_wheeling_over_a_footbridge_does_not_stop_the_cars_under_it() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    w.add_path(&[Vec2::new(0.0, 120.0), Vec2::new(0.0, 0.0)]);
    w.add_path(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, -120.0)]);
    people(&mut w);
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra && c.mid.distance(Vec2::ZERO) < 3.0).expect("the path's zebra") as u32;
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    assert!(w.walk.crossings[cr as usize].closed, "the bridge replaced the zebra");
    let nodes = w.roads.foot_path(node_at(&w, Vec2::new(0.0, 120.0)), node_at(&w, Vec2::new(0.0, -120.0))).expect("a way along the path");
    let mut path = w.route_path(&nodes, TravelMode::Bike);
    w.bike_over_footbridges(&mut path);
    assert!(path.iter().any(|&p| w.walk.level_at_node(p) > 0.0), "over the bridge: {path:?}");
    let id = w.citizens.keys().next().unwrap();
    set_off(&mut w, id, TravelMode::Bike, path);
    let mut cars: Vec<CarId> = Vec::new();
    let (mut up, mut on_deck) = (0.0f32, 0);
    for t in 0..160 {
        // Cars both ways under the bridge, a fresh pair now and then.
        if t % 6 == 0 {
            cars.push(car_through(&mut w, Vec2::ZERO, true, 120.0));
            cars.push(car_through(&mut w, Vec2::ZERO, false, 120.0));
        }
        run(&mut w, 1);
        let c = &w.citizens[id];
        let level = w.walker_level(c, c.pos);
        up = up.max(level);
        if level > 3.0 {
            on_deck += 1;
        }
        assert!(!w.peds.occupants.iter().any(|&(k, who)| k == cr || who == id), "tick {t}: the rider at {:?} (up {level:.1} m) is taken to be in the road: {:?}", c.pos, w.peds.occupants);
        assert!(!held(&w, cr), "tick {t}: drivers told to stop for the bridged crossing");
        for &car in &cars {
            assert!(!stops_for_people(&w, car), "tick {t}: {car:?} stops for people: {}", w.stop_reason_text(car));
        }
        if !matches!(w.citizens[id].activity, Activity::Traveling { .. }) {
            break;
        }
    }
    assert!(up > 3.0 && on_deck > 0, "the rider went up onto the bridge: {up} ({on_deck} ticks)");
    assert_eq!(w.peds.uses.get(&cr).map_or(0, |u| u.bridge_bikes), 1, "counted over the bridge");
    assert!(!matches!(w.citizens[id].activity, Activity::Traveling { .. }), "the rider got there");
}

/// A walker and a rider going along an elevated path over a street (right
/// beside a zebra on it) are not in its road: its drivers are never told
/// to stop for them.
#[test]
fn nobody_on_an_elevated_path_over_a_street_holds_up_its_traffic() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    // An at-grade footpath over the street a couple of metres beside where
    // the elevated one passes over it: its zebra.
    w.add_path(&[Vec2::new(40.0, 70.0), Vec2::new(2.0, 0.0)]);
    w.add_path(&[Vec2::new(2.0, 0.0), Vec2::new(40.0, -70.0)]);
    // The elevated path, up at 7 m over the street.
    let pts = vec![Vec2::new(0.0, 150.0), Vec2::new(0.0, 45.0), Vec2::new(0.0, -45.0), Vec2::new(0.0, -150.0)];
    let had = w.project.take();
    w.add_gesture(Gesture::Road { points: pts, lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Path, elevation: vec![0.0, 7.0, 7.0, 0.0], car_only: None });
    w.implement();
    w.project = had;
    people(&mut w);
    let (sid, t, _) = w.roads.nearest_where(Vec2::new(0.0, 0.0), |s| s.kind == RoadKind::Path && s.alive).expect("the elevated path");
    assert!(w.road_height(sid, t) > 5.0, "the path is up over the street: {}", w.road_height(sid, t));
    let zebra = w.walk.crossings.iter().position(|c| !c.closed && c.mid.distance(Vec2::new(2.0, 0.0)) < 3.0).expect("the at-grade path's crossing") as u32;
    let (a, b) = (node_at(&w, Vec2::new(0.0, 150.0)), node_at(&w, Vec2::new(0.0, -150.0)));
    // The rider along the elevated path (laid along its shoulder).
    let nodes = w.roads.foot_path(a, b).expect("a way along the elevated path");
    let mut ride = w.route_path(&nodes, TravelMode::Bike);
    w.bike_over_footbridges(&mut ride);
    // The walker along it too, by the walk graph.
    let (walk, _) = w.walk.route(Vec2::new(0.0, 150.0), Vec2::new(0.0, -150.0)).expect("a walk along the elevated path");
    assert!(walk.windows(2).all(|p| w.walk.crossing_between(p[0], p[1]).is_none()), "the walk crosses no street at grade: {walk:?}");
    let mut ids = w.citizens.keys();
    let (rider, walker) = (ids.next().unwrap(), ids.next().unwrap());
    set_off(&mut w, rider, TravelMode::Bike, ride);
    set_off(&mut w, walker, TravelMode::Walk, walk);
    let mut cars: Vec<CarId> = Vec::new();
    let mut high = [0.0f32; 2];
    for t in 0..400 {
        if t % 6 == 0 {
            cars.push(car_through(&mut w, Vec2::ZERO, true, 120.0));
            cars.push(car_through(&mut w, Vec2::ZERO, false, 120.0));
        }
        run(&mut w, 1);
        for (k, &id) in [rider, walker].iter().enumerate() {
            let c = &w.citizens[id];
            if let Some(z) = w.walker_way_height(c, c.pos) {
                high[k] = high[k].max(z);
            }
            assert!(!w.peds.occupants.iter().any(|&(_, who)| who == id), "tick {t}: {:?} at {:?} is taken to be in the road: {:?}", c.mode, c.pos, w.peds.occupants);
        }
        assert!(!held(&w, zebra), "tick {t}: drivers told to stop at the zebra beside the elevated path");
        for &car in &cars {
            assert!(!stops_for_people(&w, car), "tick {t}: {car:?} stops for people: {}", w.stop_reason_text(car));
        }
        if [rider, walker].iter().all(|&id| !matches!(w.citizens[id].activity, Activity::Traveling { .. })) {
            break;
        }
    }
    assert!(high.iter().all(|&z| z > 5.0), "both went along up on the path: {high:?}");
    for id in [rider, walker] {
        assert!(!matches!(w.citizens[id].activity, Activity::Traveling { .. }), "{:?} got there", w.citizens[id].mode);
    }
}

/// The jam explanation names the person a car stops for.
#[test]
fn a_car_stopping_for_someone_names_them() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    w.add_path(&[Vec2::new(0.0, 120.0), Vec2::new(0.0, 0.0)]);
    w.add_path(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, -120.0)]);
    people(&mut w);
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra && c.mid.distance(Vec2::ZERO) < 3.0).expect("the path's zebra") as u32;
    let id = w.citizens.keys().next().unwrap();
    let name = w.citizens[id].name.clone();
    let (walk, _) = w.walk.route(Vec2::new(0.0, 60.0), Vec2::new(0.0, -60.0)).expect("a walk over the zebra");
    set_off(&mut w, id, TravelMode::Walk, walk);
    let mut named = None;
    for _ in 0..120 {
        if w.peds.occupants.iter().any(|&(k, who)| k == cr && who == id) {
            // Slowed to a crawl there, with a car coming up to the zebra.
            w.config.walk_speed = 0.02;
            let car = car_through(&mut w, Vec2::ZERO, true, 35.0);
            for _ in 0..30 {
                run(&mut w, 1);
                if let Some((p, _)) = w.stopped_for(car).filter(|_| w.traffic.cars[car].v < 0.5) {
                    named = Some((p, w.stop_reason_text(car)));
                    break;
                }
            }
            break;
        }
        run(&mut w, 1);
    }
    let (p, text) = named.expect("a car stopped for the walker on the zebra");
    assert_eq!(p, id);
    assert!(text.contains(&name), "named: {text}");
}

/// [r1-roads] (section C) Footbridges joined round a junction into a ring
/// of decks are ridden over too: a bike crossing an arm goes up and round
/// on the decks as the walkers do (ends joined at a corner deck used to be
/// left out, and the bikes crossed at grade under the ring).
#[test]
fn a_bike_crossing_an_arm_under_a_ring_of_decks_goes_over_it() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 2, 2);
    w.economy.treasury = 1.0e7;
    let k = w.junction_at(Vec2::ZERO, 2.0).expect("a junction");
    let arms: Vec<u32> = w.junction_view(k).crossings.iter().map(|c| c.index).collect();
    for &cr in &arms {
        w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
        run(&mut w, 1);
    }
    assert_eq!(w.walk.footbridge_links.len(), 4, "a ring of decks");
    // Straight over an arm whose bridge is joined at a corner (no stairs
    // of its own there).
    let f = w.walk.footbridges.iter().find(|f| f.joined.iter().any(|j| *j)).expect("a joined bridge");
    let c = &w.walk.crossings[f.crossing as usize];
    let (s0, s1) = w.roads.segment_points(c.segment);
    let across = (s1 - s0).normalize_or_zero().perp();
    let (from, to) = (c.mid + across * 25.0, c.mid - across * 25.0);
    let mut path = vec![from, to];
    w.bike_over_footbridges(&mut path);
    assert!(path.iter().any(|&p| w.walk.level_at_node(p) > 0.0), "up on the decks: {path:?}");
    assert!(path.len() > 2 && path[0] == from && *path.last().unwrap() == to);
    // Once over, it is not spliced again.
    let again = path.clone();
    w.bike_over_footbridges(&mut path);
    assert_eq!(path, again);
}

/// [r1-roads] (section C) A rider who set off before the footbridge went
/// up (its path straight over the old zebra) takes the bridge once it
/// stands, as a walker re-plans.
#[test]
fn a_rider_on_the_way_when_a_footbridge_goes_up_takes_it() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    w.add_path(&[Vec2::new(0.0, 120.0), Vec2::new(0.0, 0.0)]);
    w.add_path(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, -120.0)]);
    people(&mut w);
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra && c.mid.distance(Vec2::ZERO) < 3.0).expect("the path's zebra") as u32;
    let nodes = w.roads.foot_path(node_at(&w, Vec2::new(0.0, 120.0)), node_at(&w, Vec2::new(0.0, -120.0))).expect("a way along the path");
    let path = w.route_path(&nodes, TravelMode::Bike);
    assert!(path.iter().all(|&p| w.walk.level_at_node(p) == 0.0), "planned at grade");
    let id = w.citizens.keys().next().unwrap();
    set_off(&mut w, id, TravelMode::Bike, path);
    run(&mut w, 3);
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let mut up = 0.0f32;
    for _ in 0..200 {
        run(&mut w, 1);
        let c = &w.citizens[id];
        up = up.max(w.walker_level(c, c.pos));
        if !matches!(c.activity, Activity::Traveling { .. }) {
            break;
        }
    }
    assert!(up > 3.0, "over the bridge: highest {up:.1} m");
}
