//! Tests for roundabouts in the plan: stamping one over a junction, what
//! implementing it does to roads, signals, lots and buildings, and undo.

use super::*;
use crate::roads::Ring;

fn flat() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

/// Two streets crossing at the origin: a signalled four-way junction.
fn crossroads() -> World {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w
}

fn origin_junction(w: &World) -> Option<NodeId> {
    w.roads.node_near(Vec2::ZERO, 2.0).filter(|&n| w.roads.street_degree(n) == 4)
}

fn signalled_at_origin(w: &World) -> bool {
    origin_junction(w).is_some_and(|n| w.traffic.net.node_intersection.get(n.0 as usize).and_then(|&i| w.traffic.net.intersections.get(i)).is_some_and(|i| i.signalized))
}

#[test]
fn a_roundabout_stamped_on_a_four_way_replaces_it_and_undo_brings_it_back() {
    let mut w = crossroads();
    assert!(signalled_at_origin(&w), "a signalled crossroads to start with");
    let master_before = w.master.clone();
    // Dropped a few metres off, it snaps onto the junction.
    let id = w.add_roundabout(Vec2::new(4.0, -3.0), 20.0, 1).expect("stamped");
    assert_eq!(w.gesture(id), Some(Gesture::Roundabout { center: Vec2::ZERO, radius: 20.0, lanes: 1, level: 0.0 }));
    // The project shows the ring, the four road ends it takes up and the
    // four joins, and nothing is built yet.
    let pv = w.preview();
    assert_eq!(pv.planned_rings.len(), 1);
    let fx = &pv.planned_rings[0].1;
    assert_eq!(fx.joins.len(), 4);
    assert_eq!(fx.cut.len(), 2, "each street loses its middle");
    assert_eq!(fx.cut_roads.len(), 2);
    assert!(signalled_at_origin(&w));
    // Undo takes the stamp back; redo puts it down again.
    assert!(w.undo());
    assert!(w.preview().planned_rings.is_empty());
    assert_eq!(w.effective_plan(), &master_before);
    assert!(w.redo());
    w.implement();

    assert_eq!(w.roads.rings.len(), 1);
    assert!(w.roads.node_near(Vec2::ZERO, 15.0).is_none(), "the junction went with the roads inside the ring");
    let net = &w.traffic.net;
    assert!(net.intersections.iter().all(|i| !i.signalized), "its signals are gone");
    let entries: Vec<NodeId> = net.rings.nodes.iter().filter(|(_, n)| !n.entries.is_empty()).map(|(id, _)| *id).collect();
    assert_eq!(entries.len(), 4, "four arms join the ring");
    // The streets still go everywhere: through the ring now.
    let end = |p: Vec2| w.roads.node_near(p, 1.0).unwrap();
    for (a, b) in [(Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)), (Vec2::new(0.0, -300.0), Vec2::new(-300.0, 0.0)), (Vec2::new(0.0, 300.0), Vec2::new(0.0, -300.0))] {
        assert!(w.roads.path(end(a), end(b)).is_some(), "{a} to {b}");
    }
    let route = |from: Vec2, to: Vec2| {
        let (fs, _, _) = w.roads.nearest_street(from).unwrap();
        let (ts, _, _) = w.roads.nearest_street(to).unwrap();
        net.find_route(fs, from, ts, to)
    };
    let (r, ..) = route(Vec2::new(200.0, -1.75), Vec2::new(-1.75, 200.0)).expect("east to north by car");
    assert!(r.iter().any(|l| net.rings.circulating.contains(l)), "round the ring");
    // No parcel sits on the ring road itself.
    let ring = w.roads.rings[0];
    assert!(w.parcels.parcels.iter().all(|p| p.center.length() < ring.inner_radius() + 0.5 || p.center.length() > ring.outer_radius() - 0.5));

    // Bulldozing the roundabout in a new project brings the crossroads,
    // and its signal, back.
    w.remove_gesture(id);
    w.implement();
    assert!(w.roads.rings.is_empty());
    assert!(signalled_at_origin(&w), "the crossroads is back with its signal");
    assert!(w.traffic.net.rings.is_empty());
}

#[test]
fn roads_crossing_or_ending_in_the_circle_are_joined_or_trimmed() {
    let mut w = flat();
    // A street passing off-centre through the circle, one ending inside
    // it, and one well clear of it.
    w.add_road(&[Vec2::new(-200.0, 8.0), Vec2::new(200.0, 8.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, -5.0)], 1, 1);
    w.add_road(&[Vec2::new(-200.0, 120.0), Vec2::new(200.0, 120.0)], 1, 1);
    w.add_roundabout(Vec2::new(0.0, 0.0), 24.0, 2).unwrap();
    w.implement();
    let ring = w.roads.rings[0];
    assert_eq!(ring.lanes, 2);
    let mut arms = 0;
    for (sid, seg) in w.roads.alive_segments() {
        let (a, b) = w.roads.segment_points(sid);
        if w.roads.ring_of_segment(sid).is_some() {
            assert_eq!((seg.lanes_fwd, seg.lanes_bwd), (2, 0), "one-way ring");
            continue;
        }
        // No street piece reaches inside the circle.
        for p in [a, b, (a + b) * 0.5] {
            assert!(p.distance(ring.center) > ring.radius - 1.7, "{p} is inside the ring");
        }
        if ring.on_circle(a) || ring.on_circle(b) {
            arms += 1;
        }
    }
    assert_eq!(arms, 3, "two arms of the crossing street and the trimmed stub");
    // The street clear of the ring is untouched.
    assert!(w.roads.node_near(Vec2::new(-200.0, 120.0), 0.5).is_some() && w.roads.node_near(Vec2::new(200.0, 120.0), 0.5).is_some());
    // A second ring on top of the first is refused; limits hold.
    assert!(w.add_roundabout(Vec2::new(30.0, 0.0), 20.0, 1).is_err());
    let id = w.add_roundabout(Vec2::new(500.0, 500.0), 900.0, 7).unwrap();
    assert_eq!(w.gesture(id), Some(Gesture::Roundabout { center: Vec2::new(500.0, 500.0), radius: crate::roads::RING_MAX_RADIUS, lanes: crate::roads::RING_MAX_LANES, level: 0.0 }));
}

#[test]
fn buildings_in_the_way_are_taken() {
    let mut w = crossroads();
    for (x, y) in [(6.0, 6.0), (-186.0, 6.0), (6.0, -186.0), (-186.0, -186.0)] {
        w.add_zone(square(x, y, 180.0), LandUse::Residential);
    }
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for lot in lots {
        w.build_on(lot);
    }
    let ids: Vec<BuildingId> = w.buildings.keys().collect();
    for id in &ids {
        w.finish_construction(*id);
    }
    let ring = Ring::new(Vec2::ZERO, 22.0, 1);
    let predicted: Vec<BuildingId> = w.roundabout_takes(ring).into_iter().filter_map(|l| w.lots[l].building).collect();
    assert!(predicted.len() >= 4, "the corner buildings are in the way: {}", predicted.len());
    let far: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| w.lots.get(b.lot).is_some_and(|l| l.polygon.iter().all(|p| p.length() > 150.0))).map(|(id, _)| id).collect();
    assert!(!far.is_empty());
    let old: Vec<(BuildingId, Vec2)> = far.iter().map(|b| (*b, w.lots[w.buildings[*b].lot].center)).collect();
    w.drain_events();
    w.add_roundabout(Vec2::ZERO, 22.0, 1).unwrap();
    w.implement();
    let events = w.drain_events();
    // Lots cut down to a sliver are expropriated; lots under the ring are
    // simply gone. Either way the building becomes a demolition site.
    assert!(events.iter().any(|e| matches!(e, SimEvent::Expropriated(..))), "some were bought out");
    for b in &predicted {
        assert!(w.buildings.get(*b).is_none_or(|x| x.being_demolished()), "{b:?} comes down");
    }
    for b in &far {
        let at = old.iter().find(|o| o.0 == *b).map(|o| o.1);
        assert!(w.buildings.get(*b).is_some_and(|x| !x.being_demolished()), "a building well away stays: {b:?} at {at:?}");
    }
    // No lot is left under the ring road.
    assert!(w.lots.values().all(|l| l.center.length() > ring.outer_radius() - 1.0 || l.center.length() < ring.inner_radius() + 1.0), "lots clear of the ring road");
}

#[test]
fn a_large_ring_leaves_its_middle_open_for_parcels_and_new_roads() {
    let mut w = crossroads();
    let id = w.add_roundabout(Vec2::ZERO, 1000.0, 1).unwrap();
    assert_eq!(w.gesture(id), Some(Gesture::Roundabout { center: Vec2::ZERO, radius: crate::roads::RING_MAX_RADIUS, lanes: 1, level: 0.0 }), "240 m across at most");
    // Undo and redo the stamp.
    assert!(w.undo());
    assert!(w.effective_plan().roundabouts().next().is_none());
    assert!(w.redo());
    assert!(w.effective_plan().roundabouts().next().is_some());
    w.discard_project();
    let ring = Ring::new(Vec2::ZERO, 100.0, 1);
    w.add_roundabout(Vec2::ZERO, 100.0, 1).unwrap();
    w.implement();
    assert_eq!(w.roads.rings, vec![ring]);
    // The old crossroads inside is cut out.
    assert!(w.roads.node_near(Vec2::ZERO, 20.0).is_none(), "the old junction went");
    // Usable parcels inside, along the inner edge of the ring.
    let inside: Vec<usize> = (0..w.parcels.len()).filter(|&i| w.parcels.parcels[i].center.length() < ring.inner_radius()).collect();
    assert!(inside.len() >= 10, "{} parcels inside", inside.len());
    assert!(inside.iter().any(|&i| w.parcels.parcels[i].accepts(LandUse::Residential)), "the inside can be zoned");
    // Zone and build there.
    w.add_zone(vec![Vec2::new(-80.0, -80.0), Vec2::new(80.0, -80.0), Vec2::new(80.0, 80.0), Vec2::new(-80.0, 80.0)], LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.center.length() < ring.inner_radius()).map(|(id, _)| id).collect();
    assert!(!lots.is_empty(), "lots inside the ring");
    w.build_on(lots[0]);
    assert!(w.lots[lots[0]].building.is_some(), "and a building on one");
    // A road drawn inside afterwards is kept and joins the ring at both ends.
    let road = w.add_road(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
    assert_eq!(w.roads.rings.len(), 1);
    let joins: Vec<NodeId> = [Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)].iter().map(|&p| w.roads.node_near(p, 1.0).expect("a join on the ring")).collect();
    for n in &joins {
        assert!(w.roads.ring_of_node(*n).is_some(), "joined to the ring");
        let rn = &w.traffic.net.rings.nodes[n];
        assert!(rn.entries.len() >= 2 && rn.exits.len() >= 2, "give-way entries and exits from inside and outside: {} {}", rn.entries.len(), rn.exits.len());
    }
    assert!(w.roads.nearest_street(Vec2::new(3.0, 0.0)).is_some_and(|(_, _, q)| q.distance(Vec2::ZERO) < 1.0), "the new road runs through the middle");
    let net = &w.traffic.net;
    let (fs, _, _) = w.roads.nearest_street(Vec2::new(-1.75, 0.0)).unwrap();
    let (ts, _, _) = w.roads.nearest_street(Vec2::new(250.0, -1.75)).unwrap();
    assert!(net.find_route(fs, Vec2::new(-1.75, 0.0), ts, Vec2::new(250.0, -1.75)).is_some(), "from inside out to an arm");
    // Bulldozing it gives the ground back as before.
    w.remove_gesture(road);
    w.implement();
    assert!(w.roads.nearest_street(Vec2::ZERO).is_some_and(|(_, _, q)| q.length() > 80.0));
}

#[test]
fn rings_nest_with_room_between_and_partial_overlaps_are_refused() {
    let mut w = crossroads();
    w.add_roundabout(Vec2::ZERO, 110.0, 2).unwrap();
    w.implement();
    // Concentric but too close, and half over the edge: refused.
    assert!(w.add_roundabout(Vec2::ZERO, 104.0, 1).is_err());
    assert!(w.add_roundabout(Vec2::new(95.0, 0.0), 20.0, 1).is_err());
    // Well inside: fine, and joined to the outer ring by a new road.
    w.add_roundabout(Vec2::new(10.0, 0.0), 30.0, 1).unwrap();
    w.implement();
    assert_eq!(w.roads.rings.len(), 2);
    w.add_road(&[Vec2::new(40.0, 0.0), Vec2::new(110.0, 0.0)], 1, 1);
    let inner = w.roads.node_near(Vec2::new(40.0, 0.0), 1.0).unwrap();
    let outer = w.roads.node_near(Vec2::new(110.0, 0.0), 1.0).unwrap();
    assert_eq!(w.roads.ring_of_node(inner), Some(1));
    assert_eq!(w.roads.ring_of_node(outer), Some(0));
    let far = w.roads.node_near(Vec2::new(300.0, 0.0), 1.0).unwrap();
    assert!(w.roads.path(far, inner).is_some(), "out on the avenue to the inner ring");
}

/// [w1-roads] A raised roundabout over a street: the ramps drawn up to it
/// at its level join it, the street under it passes beneath untouched, and
/// it is saved with its level.
#[test]
fn a_raised_roundabout_takes_the_roads_at_its_level_and_passes_over_the_rest() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    // A street at grade straight through where the ring goes.
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let ring_id = w.add_roundabout_at(Vec2::new(0.0, 0.0), 30.0, 1, 8.0).expect("stamped");
    assert!(matches!(w.gesture(ring_id), Some(Gesture::Roundabout { level, .. }) if level == 8.0));
    // Roads up to it from north and south, rising to its level.
    for sy in [1.0f32, -1.0] {
        w.add_gesture(Gesture::Road { points: vec![Vec2::new(0.0, sy * 250.0), Vec2::new(0.0, sy * 120.0), Vec2::new(0.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, 8.0, 8.0], car_only: None });
    }
    w.try_implement().expect("implemented");
    let ring = w.roads.rings.first().copied().expect("a ring");
    assert_eq!(ring.level, 8.0);
    // Every ring node stands at its level.
    let ring_nodes: Vec<NodeId> = (0..w.roads.nodes.len() as u32).map(NodeId).filter(|&n| w.roads.ring_of_node(n).is_some()).collect();
    assert!(!ring_nodes.is_empty());
    for &n in &ring_nodes {
        assert!((w.roads.node(n).level - 8.0).abs() < 0.01, "ring node at {}", w.roads.node(n).level);
    }
    // The two raised arms join it; the street at grade is not cut.
    let joins = ring_nodes.iter().filter(|&&n| w.roads.street_degree(n) >= 3).count();
    assert_eq!(joins, 2, "the two ramps join the ring");
    let street_whole = w.roads.alive_segments().any(|(sid, s)| {
        let (a, b) = w.roads.segment_points(sid);
        s.is_street() && a.y.abs() < 0.5 && b.y.abs() < 0.5 && a.x.min(b.x) < -40.0 && a.x.max(b.x) > 40.0 && w.roads.node(s.a).level == 0.0
    }) || {
        // or still running under it in pieces at grade
        w.roads.alive_segments().filter(|(sid, s)| {
            let (a, b) = w.roads.segment_points(*sid);
            s.is_street() && a.y.abs() < 0.5 && b.y.abs() < 0.5 && (a.x.abs() < 30.0 || b.x.abs() < 30.0)
        }).all(|(_, s)| w.roads.node(s.a).level == 0.0 && w.roads.node(s.b).level == 0.0)
    };
    assert!(street_whole, "the street passes under at grade");
    let ron = w.to_ron().unwrap();
    let w2 = World::from_ron(&ron).unwrap();
    assert_eq!(w2.roads.rings.first().map(|r| r.level), Some(8.0));
}
