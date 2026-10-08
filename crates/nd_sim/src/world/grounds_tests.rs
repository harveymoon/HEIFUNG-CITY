//! [park-paths] Paths joined to a park's walkways: the park paths are in
//! the walk graph, a path drawn to a park gate joins it, and the path is
//! cut back when the park goes.

use super::*;
use crate::walk::GroundsKind;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// An avenue along y = 0 with a park north of it, and a second street
/// along y = 220 that no street joins to the avenue.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 220.0), Vec2::new(400.0, 220.0)], 1, 1);
    // Painted with the brush, as a player would.
    let ids = w.parcels.in_polygon(&square(-200.0, 8.0, 120.0, 50.0));
    w.paint_parcels(&ids, Some(LandUse::Park), None);
    w.implement();
    assert!(w.lots.values().any(|l| l.land_use == LandUse::Park));
    w
}

/// Unzone the park's parcels and implement: the park goes.
fn unzone_park(w: &mut World) {
    let ids: Vec<usize> = (0..w.parcels.parcels.len()).filter(|&i| w.parcels.parcels[i].land_use == Some(LandUse::Park)).collect();
    assert!(!ids.is_empty());
    w.paint_parcels(&ids, None, None);
    w.implement();
}

/// The free snap point on the park's back (north) edge.
fn back_gate(w: &World) -> Vec2 {
    w.grounds_junctions()
        .iter()
        .filter(|j| j.kind == GroundsKind::Park && !j.attached)
        .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .map(|j| j.pos)
        .expect("the park shows a gate")
}

fn park_bench(w: &World) -> Vec2 {
    let (n, _, _) = w.walk.benches.first().copied().expect("the park has benches");
    w.walk.nodes[n as usize].pos
}

/// A pavement node of the y = 220 street, on its south side.
fn north_pavement(w: &World, x: f32) -> Vec2 {
    let n = w.walk.nearest_matching(Vec2::new(x, 220.0 - 8.0), 600.0, |n| w.walk.nodes[n as usize].lot.is_none()).expect("a pavement node");
    w.walk.nodes[n as usize].pos
}

#[test]
fn park_paths_are_in_the_walk_graph_and_reach_the_pavement() {
    let w = town();
    let park_edges = w.walk.adj.iter().flatten().filter(|e| e.park).count();
    assert!(park_edges > 0, "park paths are walk edges");
    let bench = park_bench(&w);
    let pavement = w.walk.nearest_matching(Vec2::new(0.0, 8.0), 600.0, |n| w.walk.nodes[n as usize].lot.is_none()).map(|n| w.walk.nodes[n as usize].pos).unwrap();
    let (route, _) = w.walk.route(pavement, bench).expect("a walk from the avenue into the park");
    assert!(route.len() > 2);
    // Gates show on the park's outer edges, clear of the avenue.
    let gates: Vec<_> = w.grounds_junctions().iter().filter(|j| j.kind == GroundsKind::Park).collect();
    assert!(!gates.is_empty(), "the park shows gates");
    for g in &gates {
        assert!(g.pos.y > 10.0, "no gate on the avenue side: {:?}", g.pos);
    }
}

#[test]
fn a_path_drawn_to_a_park_gate_joins_the_park() {
    let mut w = town();
    let gate = back_gate(&w);
    let bench = park_bench(&w);
    let start = north_pavement(&w, gate.x);
    assert!(w.walk.route(start, bench).is_none(), "the north street does not reach the park yet");

    let g = w.add_path(&[Vec2::new(gate.x, 220.0), gate]);
    assert_eq!(w.path_links.links.len(), 1, "the path end is joined to the park");
    assert_eq!(w.path_links.links[0].gesture, g);
    let j = w.grounds_junctions().iter().find(|j| j.attached).expect("the joined gate is listed");
    assert!(j.node.is_some() && j.pos.distance(gate) < 0.01);
    // The park laid a walkway out to it.
    let lot = j.lot.unwrap();
    assert!(w.lots[lot].park.as_ref().unwrap().entrances.iter().any(|e| e.distance(gate) < crate::walk::GROUNDS_REACH + 1.0), "an entrance by the path");
    let bench = park_bench(&w);
    let start = north_pavement(&w, gate.x + 30.0);
    let (route, _) = w.walk.route(start, bench).expect("people walk from the north street into the park by the path");
    assert!(route.iter().any(|p| p.distance(gate) < 3.0), "through the joined gate");
    // And it lasts through other works and a save.
    w.add_road(&[Vec2::new(-400.0, -200.0), Vec2::new(400.0, -200.0)], 1, 1);
    assert_eq!(w.path_links.links.len(), 1);
    let loaded = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(loaded.path_links.links.len(), 1);
    assert!(loaded.grounds_junctions().iter().any(|j| j.attached && j.node.is_some()));
    assert!(loaded.walk.route(start, park_bench(&loaded)).is_some());
}

#[test]
fn a_path_short_of_the_gate_is_not_joined() {
    let mut w = town();
    let gate = back_gate(&w);
    w.add_path(&[Vec2::new(gate.x, 220.0), gate + Vec2::new(0.0, 25.0)]);
    assert!(w.path_links.links.is_empty());
    assert!(w.walk.route(north_pavement(&w, gate.x + 30.0), park_bench(&w)).is_none());
}

#[test]
fn rezoning_the_park_cuts_the_path_back() {
    let mut w = town();
    let gate = back_gate(&w);
    let bend = Vec2::new(gate.x, 120.0);
    let g = w.add_path(&[Vec2::new(gate.x, 220.0), bend, gate]);
    assert_eq!(w.path_links.links.len(), 1);
    w.events.clear();
    unzone_park(&mut w);
    // (Re-cutting the block once the path is shorter may hand a corner
    // parcel its older paint back: parcel keys follow the cut, not this.)
    let near_gate = |l: &Lot| l.land_use == LandUse::Park && (l.contains(gate) || crate::geom::distance_to_outline(gate, &l.polygon) <= crate::walk::GROUNDS_REACH);
    assert!(!w.lots.values().any(near_gate), "the park by the gate is gone");
    match w.master.gestures.get(&g) {
        Some(crate::plan::Gesture::Road { points, .. }) => assert_eq!(points, &vec![Vec2::new(gate.x, 220.0), bend], "cut back to its last point"),
        other => panic!("the path is kept, shorter: {other:?}"),
    }
    assert!(w.path_links.links.is_empty());
    assert!(!w.grounds_junctions().iter().any(|j| j.attached), "no join left");
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::PathTrimmed { removed: false, .. })));
    // No street-side walk node is left at the old gate.
    assert!(w.walk.nearest_within(gate, 2.0).is_none_or(|n| w.walk.nodes[n as usize].lot.is_some()), "nothing dangles where the gate was");
}

#[test]
fn a_two_point_path_to_a_vanished_park_is_removed() {
    let mut w = town();
    let gate = back_gate(&w);
    let g = w.add_path(&[Vec2::new(gate.x, 220.0), gate]);
    w.events.clear();
    unzone_park(&mut w);
    assert!(!w.master.gestures.contains_key(&g), "nothing left of it");
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::PathTrimmed { removed: true, .. })));
}

#[test]
fn a_save_from_before_path_links_loads() {
    let w = town();
    let ron = w.to_ron().unwrap();
    let at = ron.find("path_links:").expect("the field is saved");
    // Cut the field out, as a save from before it would be.
    let open = at + ron[at..].find('(').unwrap();
    let mut depth = 0;
    let mut end = open;
    for (i, c) in ron[open..].char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let rest = ron[end..].trim_start_matches(',');
    let old = format!("{}{}", &ron[..at], rest);
    assert!(!old.contains("path_links"));
    let loaded = World::from_ron(&old).expect("an old save loads");
    assert!(loaded.path_links.links.is_empty());
    assert!(loaded.grounds_junctions().iter().any(|j| j.kind == GroundsKind::Park), "and shows the park's gates");
}

#[test]
fn a_path_joined_to_a_stadium_goes_when_the_stadium_is_bulldozed() {
    use crate::services::{Placeable, ServiceKind};
    let mut w = town();
    let stadium = w.place_building(Placeable::Service(ServiceKind::Stadium, Tier::Low), Vec2::new(150.0, 25.0)).expect("a stadium north of the avenue");
    let gate = w
        .grounds_junctions()
        .iter()
        .filter(|j| j.kind == GroundsKind::Stadium && !j.attached)
        .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .map(|j| j.pos)
        .expect("the stadium grounds show a gate");
    let g = w.add_path(&[Vec2::new(gate.x, 220.0), gate]);
    assert_eq!(w.path_links.links.len(), 1, "joined to the stadium grounds");
    assert_eq!(w.path_links.links[0].kind, GroundsKind::Stadium);
    let lot = w.buildings[stadium].lot;
    let inside = w.walk.nearest_matching(w.lots[lot].center, 80.0, |n| w.walk.nodes[n as usize].lot == Some(lot)).map(|n| w.walk.nodes[n as usize].pos).unwrap();
    assert!(w.walk.route(north_pavement(&w, gate.x + 30.0), inside).is_some(), "the path reaches the grounds");
    w.events.clear();
    w.bulldoze_building(stadium).unwrap();
    assert!(!w.master.gestures.contains_key(&g), "the path to it is removed");
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::PathTrimmed { removed: true, grounds: GroundsKind::Stadium, .. })));
}

/// [sweep-visual] The park's walkway is paved right out to a joined path's
/// end (a step outside the edge), straight on from the path: no strip of
/// grass is left between the two.
#[test]
fn the_park_walkway_meets_a_joined_path_end() {
    let mut w = town();
    let gate = back_gate(&w);
    w.add_path(&[Vec2::new(gate.x, 220.0), gate]);
    let lot = w.grounds_junctions().iter().find(|j| j.attached).and_then(|j| j.lot).expect("joined");
    let park = w.lots[lot].park.as_ref().unwrap();
    let spoke = park.paths.iter().find(|p| p[0].distance(gate) < 0.05).expect("a walkway starts at the path's end");
    // Its next point is just inside the edge, square to it (the path runs
    // north-south here, so straight on means the same x).
    assert!((spoke[1].x - gate.x).abs() < 0.3, "straight on from the path: {:?} -> {:?}", spoke[0], spoke[1]);
    assert!(crate::geom::point_in_polygon(spoke[1], &w.lots[lot].polygon));
    // And the walk graph still takes people in by it.
    let start = north_pavement(&w, gate.x + 30.0);
    assert!(w.walk.route(start, park_bench(&w)).is_some());
}
