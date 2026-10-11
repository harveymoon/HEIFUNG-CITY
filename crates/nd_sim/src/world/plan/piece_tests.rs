//! [pt-ux] The Bulldoze tool takes one piece of a road (junction to
//! junction), not the whole road.

use super::*;

fn flat() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

/// The live street piece nearest `p`.
fn piece_near(w: &World, p: Vec2) -> crate::roads::SegmentId {
    let (i, _) = w
        .roads
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.alive)
        .map(|(i, s)| (i, crate::geom::distance_to_polyline(p, &[w.roads.node(s.a).pos, w.roads.node(s.b).pos])))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    crate::roads::SegmentId(i as u32)
}

/// A long street crossed by two side streets: bulldozing between the two
/// crossings takes only that block; the road keeps its ends and its name.
#[test]
fn bulldozing_takes_one_block_of_a_road() {
    let mut w = flat();
    let main = w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-100.0, -200.0), Vec2::new(-100.0, 200.0)], 1, 1);
    w.add_road(&[Vec2::new(100.0, -200.0), Vec2::new(100.0, 200.0)], 1, 1);
    let name = |w: &World, p: Vec2| w.streets.of(piece_near(w, p)).map(|s| s.name.clone());
    let east_name = name(&w, Vec2::new(200.0, 0.0));
    assert!(east_name.is_some());
    let click = Vec2::new(10.0, 1.0);
    let (s0, s1) = w.road_piece_span(main, Some(piece_near(&w, click)), click).expect("a span");
    assert!((s0 - 200.0).abs() < 1.5 && (s1 - 400.0).abs() < 1.5, "the block between the crossings: {s0}..{s1}");
    assert_eq!(w.remove_road_piece(main, s0, s1), 2);
    w.implement();
    // The block is gone, the two ends stay, still named as before.
    let gap = piece_near(&w, click);
    let seg = w.roads.segment(gap);
    assert!(crate::geom::distance_to_polyline(click, &[w.roads.node(seg.a).pos, w.roads.node(seg.b).pos]) > 5.0, "nothing left in the gap");
    assert!(w.roads.node_near(Vec2::new(-300.0, 0.0), 2.0).is_some() && w.roads.node_near(Vec2::new(300.0, 0.0), 2.0).is_some());
    assert_eq!(name(&w, Vec2::new(200.0, 0.0)), east_name, "the road keeps its name");
    // One undo puts the block back.
    let mut w2 = flat();
    let main = w2.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w2.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
    let click = Vec2::new(-150.0, 0.0);
    let (s0, s1) = w2.road_piece_span(main, Some(piece_near(&w2, click)), click).unwrap();
    assert!(s0.abs() < 1.5 && (s1 - 300.0).abs() < 1.5, "a dead end to the crossing: {s0}..{s1}");
    assert_eq!(w2.remove_road_piece(main, s0, s1), 1);
    assert_eq!(w2.gesture(main).unwrap().points(), &[Vec2::new(0.0, 0.0), Vec2::new(300.0, 0.0)]);
    assert!(w2.undo());
    // [fix/roads-tools] As implemented: mended with a point at the crossing.
    assert_eq!(w2.gesture(main).unwrap().points(), &[Vec2::new(-300.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(300.0, 0.0)]);
}

/// A road with no junction on it goes whole.
#[test]
fn a_lone_road_goes_whole() {
    let mut w = flat();
    let id = w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let click = Vec2::new(50.0, 0.0);
    let (s0, s1) = w.road_piece_span(id, Some(piece_near(&w, click)), click).unwrap();
    assert_eq!(w.remove_road_piece(id, s0, s1), 0);
    assert_eq!(w.gesture(id), None);
}
