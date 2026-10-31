//! [r4-edge-bridges] Harvey's flag 143: footbridges belong to their
//! junction. Moving the junction carries them along (stairs and span laid
//! again over the crossings as they are now), deleting a leg takes only the
//! footbridge over it (with a message), and an old save's footbridges are
//! attached to their junctions on load.

use super::*;
use crate::junctions::CrossingUpgrade;
use crate::walk::CrossingKind;

fn flat() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1.0e7;
    w
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// The junction crosswalk over the leg leaving the junction at `at` towards
/// `towards`.
fn crossing_towards(w: &World, at: Vec2, towards: Vec2) -> u32 {
    let n = w.roads.node_near(at, 1.0).expect("the junction");
    let want = (towards - at).normalize();
    w.walk
        .crossings
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == CrossingKind::Junction && c.node == Some(n))
        .max_by(|a, b| (a.1.mid - at).normalize().dot(want).total_cmp(&(b.1.mid - at).normalize().dot(want)))
        .map(|(k, _)| k as u32)
        .expect("a crosswalk on that leg")
}

/// The footbridges' junctions (the node each one's crossing is at).
fn bridged_nodes(w: &World) -> Vec<Option<NodeId>> {
    w.walk.footbridges.iter().map(|f| w.walk.crossings[f.crossing as usize].node).collect()
}

/// Drag the junction at `from` to `to` with the Edit tool and implement.
fn drag_junction(w: &mut World, from: Vec2, to: Vec2) {
    let plan = w.effective_plan().clone();
    let j = w.joint_at(&plan, from, 8.0).expect("the junction");
    w.set_gestures(j.moved(&plan, to), true);
    w.try_implement().expect("implemented");
}

/// Every footbridge stands over its own crossing, its walkway across it.
fn bridges_fit(w: &World) {
    for f in &w.walk.footbridges {
        let c = &w.walk.crossings[f.crossing as usize];
        let mid = (f.top[0] + f.top[1]) * 0.5;
        // (set a little back from the junction, clear of the corner)
        assert!(mid.distance(c.mid) < 4.0, "the span is over its crossing: {mid} vs {}", c.mid);
        let (s0, s1) = w.roads.segment_points(c.segment);
        let span = (f.top[1] - f.top[0]).normalize();
        assert!(span.dot((s1 - s0).normalize()).abs() < 0.2, "the span runs across the street");
        assert!(c.bridged && c.closed, "the crosswalk under it is replaced");
    }
}

#[test]
fn footbridges_follow_their_moved_junction() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, -150.0), v(0.0, 150.0)], 1, 1);
    let east = crossing_towards(&w, Vec2::ZERO, v(50.0, 0.0));
    w.set_crossing_upgrade(east, CrossingUpgrade::Footbridge).unwrap();
    let north = crossing_towards(&w, Vec2::ZERO, v(0.0, 50.0));
    w.set_crossing_upgrade(north, CrossingUpgrade::Footbridge).unwrap();
    assert_eq!(w.walk.footbridges.len(), 2);
    assert!(w.crosswalks.rules.iter().all(|r| r.anchor.as_ref().is_some_and(|a| a.junction.distance(Vec2::ZERO) < 0.5 && a.roads.len() == 2)), "both belong to the junction");
    let money = w.economy.treasury;

    // A little (as Harvey did), then a long way.
    for to in [v(6.0, 4.0), v(40.0, 30.0)] {
        let from = w.crosswalks.rules[0].anchor.as_ref().unwrap().junction;
        drag_junction(&mut w, from, to);
        let n = w.roads.node_near(to, 0.5).expect("the junction where it was dragged");
        assert_eq!(w.walk.footbridges.len(), 2, "both footbridges came along to {to}");
        assert!(bridged_nodes(&w).iter().all(|&b| b == Some(n)), "on the moved junction's crossings");
        bridges_fit(&w);
        for r in &w.crosswalks.rules {
            assert!(r.anchor.as_ref().unwrap().junction.distance(to) < 0.5);
        }
        // One east of the junction, one north, as built.
        let dirs: Vec<Vec2> = w.walk.footbridges.iter().map(|f| (w.walk.crossings[f.crossing as usize].mid - to).normalize()).collect();
        assert!(dirs.iter().any(|d| d.x > 0.9) && dirs.iter().any(|d| d.y > 0.9), "over the same legs: {dirs:?}");
        // The walk graph goes over them: straight across the east leg.
        let c = &w.walk.crossings[w.walk.footbridges.iter().find(|f| (w.walk.crossings[f.crossing as usize].mid - to).x > 0.0).unwrap().crossing as usize];
        let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
        let (_, nodes) = w.walk.route(a + (a - b).normalize() * 2.0, b + (b - a).normalize() * 2.0).expect("a way over");
        assert!(nodes.iter().any(|&k| w.walk.nodes[k as usize].level > 0.0), "over the bridge");
    }
    assert_eq!(w.economy.treasury, money, "nothing paid again for moving them");
    assert!(!w.events.iter().any(|e| matches!(e, SimEvent::CrossingRuleRemoved { .. })));
}

#[test]
fn a_deleted_leg_takes_only_its_footbridge() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, -150.0), v(0.0, 0.0)], 1, 1);
    let north_road = w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    for towards in [v(50.0, 0.0), v(0.0, 50.0), v(-50.0, 0.0)] {
        let k = crossing_towards(&w, Vec2::ZERO, towards);
        w.set_crossing_upgrade(k, CrossingUpgrade::Footbridge).unwrap();
    }
    assert_eq!(w.walk.footbridges.len(), 3);
    let north_name = w.crosswalks.rules.iter().find_map(|r| r.anchor.as_ref().filter(|a| a.leg_dir.y > 0.9).map(|a| a.street.clone())).unwrap();
    w.events.clear();
    w.project = None;
    w.remove_gesture(north_road);
    w.try_implement().unwrap();
    assert_eq!(w.walk.footbridges.len(), 2, "the bridges over the other legs stay");
    assert_eq!(w.crosswalks.rules.len(), 2);
    let dirs: Vec<Vec2> = w.walk.footbridges.iter().map(|f| w.walk.crossings[f.crossing as usize].mid.normalize()).collect();
    assert!(dirs.iter().any(|d| d.x > 0.9) && dirs.iter().any(|d| d.x < -0.9), "east and west kept: {dirs:?}");
    bridges_fit(&w);
    let said: Vec<&SimEvent> = w.events.iter().filter(|e| matches!(e, SimEvent::CrossingRuleRemoved { .. })).collect();
    assert_eq!(said.len(), 1, "one message");
    match said[0] {
        SimEvent::CrossingRuleRemoved { upgrade, street, .. } => {
            assert_eq!(*upgrade, CrossingUpgrade::Footbridge);
            assert_eq!(*street, north_name);
        }
        _ => unreachable!(),
    }

    // The bulldozer taking the whole east stretch of a main road running
    // through a crossroads (the road is split; the part through the
    // junction may get a new id): the west footbridge stays, the east one
    // goes.
    let mut w = flat();
    let main = w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, -150.0), v(0.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    for towards in [v(50.0, 0.0), v(-50.0, 0.0)] {
        let k = crossing_towards(&w, Vec2::ZERO, towards);
        w.set_crossing_upgrade(k, CrossingUpgrade::Footbridge).unwrap();
    }
    assert_eq!(w.walk.footbridges.len(), 2);
    w.events.clear();
    w.project = None;
    let line = w.master.gestures[&main].road_polyline();
    let total: f32 = line.windows(2).map(|p| p[0].distance(p[1])).sum();
    let at0 = crate::plan::arc_of(&line, Vec2::ZERO);
    w.remove_road_piece(main, at0, total);
    w.try_implement().unwrap();
    let kept: Vec<Vec2> = w.walk.footbridges.iter().map(|f| w.walk.crossings[f.crossing as usize].mid).collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].x < 0.0, "the west one");
    bridges_fit(&w);
    assert_eq!(w.events.iter().filter(|e| matches!(e, SimEvent::CrossingRuleRemoved { .. })).count(), 1);
}

#[test]
fn an_old_saves_footbridges_join_their_junction() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, -150.0), v(0.0, 150.0)], 1, 1);
    let east = crossing_towards(&w, Vec2::ZERO, v(50.0, 0.0));
    w.set_crossing_upgrade(east, CrossingUpgrade::Footbridge).unwrap();
    // As an old save had them: no anchors.
    for r in &mut w.crosswalks.rules {
        r.anchor = None;
    }
    let text = w.to_save_ron().expect("save");
    assert!(!text.contains("anchor"), "a rule without one is written as before");
    let mut back = World::from_ron(&text).expect("load");
    assert_eq!(back.walk.footbridges.len(), 1);
    let a = back.crosswalks.rules[0].anchor.clone().expect("attached to its junction on load");
    assert!(a.junction.distance(Vec2::ZERO) < 0.5 && a.leg_dir.x > 0.9 && a.roads.len() == 2);
    // And it follows the junction from then on.
    drag_junction(&mut back, Vec2::ZERO, v(5.0, 3.0));
    assert_eq!(back.walk.footbridges.len(), 1);
    bridges_fit(&back);

    // A save whose junction was moved before footbridges were anchored:
    // its rule sits where the crosswalk was, off every crossing now. It
    // goes to the junction's crossing it clearly belongs to.
    let mut old = flat();
    old.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    old.add_road(&[v(0.0, -150.0), v(0.0, 150.0)], 1, 1);
    let east = crossing_towards(&old, Vec2::ZERO, v(50.0, 0.0));
    old.set_crossing_upgrade(east, CrossingUpgrade::Footbridge).unwrap();
    let was = old.crosswalks.rules[0].at;
    for r in &mut old.crosswalks.rules {
        r.anchor = None;
    }
    let mut moved = World::from_ron(&{
        // The junction moved 6 m east and 3 m north, the rule left behind.
        let mut m = World::from_ron(&old.to_save_ron().unwrap()).unwrap();
        m.crosswalks.rules.clear();
        drag_junction(&mut m, Vec2::ZERO, v(6.0, 3.0));
        assert!(m.walk.footbridges.is_empty());
        m.crosswalks.rules = old.crosswalks.rules.clone();
        m.to_save_ron().unwrap()
    })
    .unwrap();
    assert_eq!(moved.walk.footbridges.len(), 1, "the left-behind footbridge is back on its junction");
    let r = &moved.crosswalks.rules[0];
    assert!(r.anchor.as_ref().is_some_and(|a| a.junction.distance(v(6.0, 3.0)) < 0.5));
    assert!(r.at.distance(was) > 2.0 && r.at.distance(was) < crate::world::MIGRATE_REACH);
    bridges_fit(&moved);
    // Far off any junction (a rule 40 m from every crosswalk) it waits as before.
    moved.crosswalks.rules[0].anchor = None;
    moved.crosswalks.rules[0].at = v(100.0, 40.0);
    let back = World::from_ron(&moved.to_save_ron().unwrap()).unwrap();
    assert!(back.walk.footbridges.is_empty());
    assert_eq!(back.crosswalks.rules.len(), 1, "kept, doing nothing");
}
