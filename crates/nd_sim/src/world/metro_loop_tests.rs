//! [r4-metroloop] Walks beside a viaduct: one that starts on the ground
//! is not snapped up onto the deck's pavement, and a walk along the
//! ground is drawn on the ground however close a deck runs overhead.
//! (The waiting riders themselves are tested with the metro, in
//! `world/subway.rs`.)

use super::*;

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

/// A street along the ground at y = 0 and a raised street 8 m up running
/// beside it at `y` (down to the ground well beyond both ends), each with
/// a vertex every 40 m so their pavements have nodes all along.
fn street_and_viaduct(y: f32) -> World {
    let mut w = flat();
    let ground: Vec<Vec2> = (-8..=8).map(|k| Vec2::new(k as f32 * 40.0, 0.0)).collect();
    w.add_road(&ground, 1, 1);
    let mut pts = vec![Vec2::new(-560.0, y)];
    let mut levels = vec![0.0];
    for k in -10..=10 {
        pts.push(Vec2::new(k as f32 * 40.0, y));
        levels.push(8.0);
    }
    pts.push(Vec2::new(560.0, y));
    levels.push(0.0);
    w.project = None;
    w.add_gesture(Gesture::Road { points: pts, lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: levels, car_only: None });
    w.implement();
    assert!(w.relief, "a raised way");
    w
}

/// [r4-metroloop] Harvey's Steam Alley station: its platform (the middle
/// of the lot) lay nearer the pavement of the Kessler Drive viaduct 12 m
/// up than any on the ground, so every walk from it was snapped up there.
/// A walk from a point on the ground beside a deck starts on the ground;
/// one from up on the deck (a bus stop on it) still starts on the deck.
#[test]
fn a_walk_from_beside_a_viaduct_does_not_climb_onto_it() {
    let w = street_and_viaduct(24.0);
    assert!(w.walk.nodes.iter().any(|n| n.deck), "the viaduct's pavements are a deck");
    assert!(w.walk.nodes.iter().filter(|n| n.pos.y.abs() < 10.0).all(|n| !n.deck), "the street's are not");
    // Nearer the deck's pavement than the street's, out from under it.
    let p = Vec2::new(4.0, 14.0);
    let near = w.walk.nearest(p, 60.0).unwrap();
    assert!(w.walk.nodes[near as usize].deck, "the nearest node is up on the deck (the old snap)");
    assert!(!w.walk.under_deck(p));
    let start = w.walk.snap_end(p).unwrap();
    assert!(!w.walk.nodes[start as usize].deck, "starts on the ground: {:?}", w.walk.nodes[start as usize].pos);
    let (path, nodes) = w.walk.route(p, Vec2::new(300.0, -8.0)).expect("a walk along the street");
    assert!(nodes.iter().all(|&n| !w.walk.nodes[n as usize].deck), "never up on the viaduct: {path:?}");
    for leg in path.windows(2) {
        assert_eq!(w.way_height_on_leg(leg[0], leg[1], (leg[0] + leg[1]) * 0.5), None, "on the ground: {leg:?}");
    }
    // From up on the deck's carriageway: onto its pavement.
    let up = Vec2::new(4.0, 24.0 - 3.6);
    assert!(w.walk.under_deck(up));
    assert!(w.walk.nodes[w.walk.snap_end(up).unwrap() as usize].deck, "a walk from the deck starts on it");
    // And a node of the deck itself is where a walk planned again there
    // goes on from.
    let deck_node = w.walk.nodes.iter().position(|n| n.deck && n.pos.x.abs() < 50.0).unwrap() as u32;
    assert_eq!(w.walk.snap_end(w.walk.nodes[deck_node as usize].pos), Some(deck_node));
}

/// [r4-metroloop] A leg of the walk graph on the ground is drawn on the
/// ground: a deck running alongside within the reach of its pavement used
/// to lift whoever walked it up onto the deck ("jumping up and down on an
/// elevated road"), while a leg along the deck's own pavement is up there.
#[test]
fn a_walk_along_the_ground_beside_a_viaduct_stays_on_the_ground() {
    let w = street_and_viaduct(14.0);
    let ground = w.ground_height(Vec2::new(0.0, 0.0));
    let (path, _) = w.walk.route(Vec2::new(-200.0, 7.0), Vec2::new(200.0, 7.0)).expect("along the street");
    let mut checked = 0;
    for leg in path.windows(2) {
        let mid = (leg[0] + leg[1]) * 0.5;
        if (mid.y - 14.0).abs() > 9.0 || leg[0].distance(leg[1]) < 5.0 || w.walk.deck_at_node(leg[0]) != Some(false) {
            continue;
        }
        assert_eq!(w.walk.lifted_at_node(leg[0]), Some(false));
        assert_eq!(w.way_height_on_leg(leg[0], leg[1], mid), None, "at grade beside the deck: {leg:?}");
        checked += 1;
    }
    assert!(checked >= 4, "legs along the street within reach of the deck: {checked}");
    // Up on the deck's pavement: its height.
    let (up, nodes) = w.walk.route(Vec2::new(-200.0, 14.0), Vec2::new(200.0, 14.0)).expect("along the deck");
    assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].deck));
    let leg = up.windows(2).find(|l| w.walk.deck_at_node(l[0]) == Some(true) && w.walk.deck_at_node(l[1]) == Some(true) && l[0].distance(l[1]) > 5.0).expect("a leg on the deck");
    let z = w.way_height_on_leg(leg[0], leg[1], (leg[0] + leg[1]) * 0.5).expect("up on the deck");
    assert!(z > ground + 6.0, "{z} over {ground}");
}
