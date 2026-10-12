//! [fix/roads-tools] Junctions as one joint: mending on Implement, the
//! junction the Edit tool grabs, and dragging it with every road attached.

use super::*;

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn road(points: &[Vec2]) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None }
}

fn build(w: &mut World, gs: &[Gesture]) -> Vec<GestureId> {
    w.project = None;
    let ids = gs.iter().map(|g| w.add_gesture(g.clone())).collect();
    w.try_implement().expect("implemented");
    ids
}

fn pts(w: &World, id: GestureId) -> Vec<Vec2> {
    w.master.gestures[&id].points().to_vec()
}

/// The network the master lays now is the one built: mending changed
/// nothing on the map.
fn unchanged_on_the_map(w: &World) {
    let (roads, _) = w.lay_roads(&w.master);
    assert!(super::joints::same_geometry(&w.roads, &roads), "the mended plan lays the same network");
}

/// Two roads meeting at a right angle, one end dropped 2 m off the other's:
/// implementing puts both ends on the one junction, and dragging it moves
/// both roads together (one undo puts them back).
#[test]
fn an_l_corner_is_mended_and_moves_as_one() {
    let mut w = flat();
    let ids = build(&mut w, &[road(&[Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0)]), road(&[Vec2::new(1.5, 1.5), Vec2::new(0.0, 200.0)])]);
    let (a, b) = (ids[0], ids[1]);
    let n = w.roads.node_near(Vec2::ZERO, 3.0).expect("a junction");
    let q = w.roads.node(n).pos;
    assert_eq!(pts(&w, a)[1], q);
    assert_eq!(pts(&w, b)[0], q, "the end the network snapped is put on the junction");
    unchanged_on_the_map(&w);
    // The Edit tool grabs both.
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, q + Vec2::new(1.0, 1.0), 8.0).expect("a joint");
    assert_eq!(j.gestures().into_iter().collect::<Vec<_>>(), vec![a, b]);
    // Drag it as the tool does: a new step, then amended.
    let to = Vec2::new(30.0, 25.0);
    w.set_gestures(j.moved(&plan, q + Vec2::new(5.0, 5.0)), true);
    w.set_gestures(j.moved(&plan, to), false);
    assert_eq!(w.gesture(a).unwrap().points()[1], to);
    assert_eq!(w.gesture(b).unwrap().points()[0], to);
    w.try_implement().unwrap();
    let n2 = w.roads.node_near(to, 0.5).expect("one junction where it was dragged");
    assert_eq!(w.roads.node(n2).segments.iter().filter(|&&s| w.roads.try_segment(s).is_some()).count(), 2);
    assert!(w.roads.node_near(Vec2::ZERO, 3.0).is_none(), "nothing left at the old corner");
    // Undo (in a project) takes both back in one step.
    w.set_gestures(j.moved(&w.effective_plan().clone(), Vec2::new(60.0, 60.0)), true);
    assert!(w.undo());
    assert_eq!(w.gesture(a).unwrap().points()[1], to);
    assert_eq!(w.gesture(b).unwrap().points()[0], to);
}

/// A road ending on another's middle (a T) and two roads crossing (an X):
/// the roads running through get a point at the junction on Implement, on
/// their line, so the map is unchanged and the junction has one point.
#[test]
fn a_tee_and_a_crossroads_get_one_point() {
    let mut w = flat();
    let ids = build(
        &mut w,
        &[
            road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)]),
            road(&[Vec2::new(-100.0, 0.0), Vec2::new(-100.0, 250.0)]),
            road(&[Vec2::new(120.0, -250.0), Vec2::new(120.0, 250.0)]),
        ],
    );
    let (main, tee, cross) = (ids[0], ids[1], ids[2]);
    unchanged_on_the_map(&w);
    let m = pts(&w, main);
    assert!(m.contains(&Vec2::new(-100.0, 0.0)) && m.contains(&Vec2::new(120.0, 0.0)), "the main road has a point at both junctions: {m:?}");
    assert!(pts(&w, cross).contains(&Vec2::new(120.0, 0.0)), "the crossing road has one at the crossroads");
    // Implementing again mends nothing more.
    assert!(w.junction_mends(&w.master).is_empty());
    // The crossroads moves as one: four legs meet where it is dragged.
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, Vec2::new(121.0, 1.0), 8.0).expect("the crossroads");
    assert_eq!(j.gestures().into_iter().collect::<Vec<_>>(), vec![main, cross]);
    let to = Vec2::new(140.0, 20.0);
    w.set_gestures(j.moved(&plan, to), true);
    w.try_implement().unwrap();
    let n = w.roads.node_near(to, 0.5).expect("the crossroads moved");
    assert_eq!(w.roads.node(n).segments.iter().filter(|&&s| w.roads.try_segment(s).is_some()).count(), 4);
    // The tee was not touched.
    assert!(w.roads.node_near(Vec2::new(-100.0, 0.0), 0.5).is_some());
    let _ = tee;
}

/// A road not mended yet (an old save) still moves with its junction: the
/// road running through gets its point when the drag starts.
#[test]
fn an_unmended_tee_still_moves_as_one() {
    let mut w = flat();
    let main = road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)]);
    let ids = build(&mut w, &[main.clone(), road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 250.0)])]);
    // Undo the mend, as a save from before it would be.
    w.master.gestures.insert(ids[0], main.clone());
    w.plan_changed();
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, Vec2::new(2.0, 2.0), 8.0).expect("the tee");
    assert_eq!(j.through.len(), 1);
    assert_eq!(j.points.len(), 1);
    let moved: BTreeMap<GestureId, Gesture> = j.moved(&plan, Vec2::new(0.0, 30.0)).into_iter().collect();
    assert_eq!(moved[&ids[0]].points(), &[Vec2::new(-300.0, 0.0), Vec2::new(0.0, 30.0), Vec2::new(300.0, 0.0)]);
    assert_eq!(moved[&ids[1]].points()[0], Vec2::new(0.0, 30.0));
}

/// A viaduct over a street meets it nowhere: no junction to drag there,
/// and mending leaves both alone.
#[test]
fn a_road_passing_over_is_no_part_of_the_junction() {
    let mut w = flat();
    let street = road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)]);
    let tee = road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, -200.0)]);
    let deck = Gesture::Road { points: vec![Vec2::new(-20.0, 300.0), Vec2::new(-20.0, -300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![10.0, 10.0], car_only: None };
    let ids = build(&mut w, &[street, tee, deck.clone()]);
    assert_eq!(w.master.gestures[&ids[2]], deck, "the viaduct is untouched");
    let plan = w.effective_plan().clone();
    if let Some(j) = w.joint_at(&plan, Vec2::new(-20.0, 0.0), 8.0) {
        assert!(!j.gestures().contains(&ids[2]), "the viaduct is not grabbed with the street");
    }
    let j = w.joint_at(&plan, Vec2::new(0.0, 0.0), 8.0).expect("the tee");
    assert!(!j.gestures().contains(&ids[2]));
}

/// The set of gestures a project step changes is amended in place while a
/// drag goes on, and a new drag is a new step.
#[test]
fn a_joint_drag_is_one_undo_step() {
    let mut w = flat();
    let ids = build(&mut w, &[road(&[Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0)]), road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 200.0)])]);
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, Vec2::ZERO, 8.0).unwrap();
    for (k, x) in [5.0f32, 10.0, 15.0].into_iter().enumerate() {
        w.set_gestures(j.moved(&plan, Vec2::new(x, 0.0)), k == 0);
    }
    assert_eq!(w.project.as_ref().unwrap().steps.len(), 1);
    assert!(w.undo());
    assert_eq!(w.gesture(ids[0]).unwrap().points()[1], Vec2::ZERO);
    assert_eq!(w.gesture(ids[1]).unwrap().points()[0], Vec2::ZERO);
}

/// [fix/roads-tools] The Road tool's circle: a closed curved road, laid as
/// one ring of pieces (every node joins two), running smoothly through its
/// start; its two ends are one joint.
#[test]
fn a_circle_road_is_a_closed_smooth_ring() {
    let mut w = flat();
    let pts = crate::plan::circle_points(Vec2::new(0.0, 0.0), 80.0);
    assert!(crate::plan::is_loop(&pts));
    let g = Gesture::Road { points: pts.clone(), lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new(), car_only: None };
    // Smooth through the start: every sample stays near the circle.
    for q in g.road_polyline() {
        assert!((q.length() - 80.0).abs() < 1.5, "{q} off the circle");
    }
    let ids = build(&mut w, &[g]);
    let live: Vec<usize> = w.roads.nodes.iter().filter(|n| n.pos.length() < 100.0).map(|n| n.segments.iter().filter(|&&s| w.roads.try_segment(s).is_some()).count()).filter(|&k| k > 0).collect();
    assert!(live.len() > 8 && live.iter().all(|&k| k == 2), "a closed ring: {live:?}");
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, pts[0], 8.0).unwrap();
    assert!(j.is_junction() && j.points.len() == 2 && j.gestures().len() == 1, "{j:?}");
    let moved: BTreeMap<GestureId, Gesture> = j.moved(&plan, Vec2::new(90.0, 0.0)).into_iter().collect();
    let p = moved[&ids[0]].points();
    assert_eq!(p[0], p[p.len() - 1], "still closed");
}

/// [w1-roads] A curved road with a street ending on its middle and another
/// crossing it: Implement gives the curve a control point at each
/// junction (built through it, a hair from the curve drawn), so the
/// junction is one point and moves as one.
#[test]
fn a_curved_road_through_junctions_gets_its_points() {
    let mut w = flat();
    let curve = Gesture::Road { points: vec![Vec2::new(-300.0, 0.0), Vec2::new(0.0, 80.0), Vec2::new(300.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new(), car_only: None };
    let drawn = curve.road_polyline();
    let ids = build(&mut w, &[curve, road(&[Vec2::new(-150.0, 200.0), Vec2::new(-150.0, 0.0)]), road(&[Vec2::new(150.0, 250.0), Vec2::new(150.0, -150.0)])]);
    let (c, tee, cross) = (ids[0], ids[1], ids[2]);
    unchanged_on_the_map(&w);
    let cp = pts(&w, c);
    for x in [-150.0f32, 150.0] {
        // The junction (the crossing road may snap onto a vertex of the curve).
        let n = (0..w.roads.nodes.len()).map(|i| NodeId(i as u32)).find(|&n| (w.roads.node(n).pos.x - x).abs() < 6.0 && w.roads.node(n).pos.y > 20.0 && w.roads.node(n).pos.y < 90.0 && w.roads.node(n).segments.iter().filter(|&&s| w.roads.try_segment(s).is_some()).count() >= 3).expect("a junction on the curve");
        let q = w.roads.node(n).pos;
        assert!(cp.contains(&q), "the curve has a point at the junction {q:?}: {cp:?}");
        let live = w.roads.node(n).segments.iter().filter(|&&s| w.roads.try_segment(s).is_some()).count();
        assert!(live >= 3, "still a junction");
    }
    // Built a hair from where it was drawn.
    let built = w.master.gestures[&c].road_polyline();
    for p in &built {
        assert!(crate::geom::distance_to_polyline(*p, &drawn) <= super::joints::CURVE_MEND_TOL + 1e-3);
    }
    // The junction moves as one: the curve and the road crossing it together.
    let q = pts(&w, tee).into_iter().find(|p| cp.contains(p)).expect("the crossing road shares the point");
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, q, 6.0).expect("the junction on the curve");
    assert!(j.gestures().contains(&c) && j.gestures().contains(&tee), "{:?}", j.gestures());
    assert!(j.through.is_empty(), "the curve has its own point there");
    let _ = cross;
}


