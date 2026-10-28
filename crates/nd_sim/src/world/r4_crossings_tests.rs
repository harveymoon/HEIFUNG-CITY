//! [r4-crossings] Harvey's flag (day 16, his triple roundabout): paths
//! joined to the street chopped it up and piled crossings up a few metres
//! apart, walkers stood in the road beside his footbridges, a car floated
//! over a stub of street a metre long and cyclists rode up the footbridge
//! stairs. Path ends now meet the street at its nodes, ways under a street
//! cut no stub off it, neighbouring junctions share one crosswalk, walkers
//! go a short way round to a footbridge or underpass, cyclists walk their
//! bikes over, and an old save is laid again on load with its footbridges.

use super::*;
use crate::citizens::TravelMode;
use crate::junctions::CrossingUpgrade;
use crate::mind::IdleMind;
use crate::roads::{MIN_PIECE, PATH_MEETS};
use crate::walk::{CrossingKind, BRIDGED_GRADE_PENALTY};

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// The street pieces (alive) of the network, with their lengths.
fn street_pieces(w: &World) -> Vec<(SegmentId, f32)> {
    w.roads.alive_segments().filter(|(_, s)| s.is_street()).map(|(sid, _)| {
        let (a, b) = w.roads.segment_points(sid);
        (sid, a.distance(b))
    }).collect()
}

/// Road nodes where a path meets a street, near `p`.
fn path_meets_near(w: &World, p: Vec2, r: f32) -> usize {
    w.roads
        .nodes
        .iter()
        .filter(|n| n.pos.distance(p) <= r)
        .filter(|n| n.segments.iter().any(|&s| w.roads.segment(s).alive && w.roads.segment(s).kind == RoadKind::Path) && n.segments.iter().any(|&s| w.roads.segment(s).alive && w.roads.segment(s).is_street()))
        .count()
}

/// Open crossings (not jaywalks) over the street along y = 0 between x0 and x1.
fn open_over_main(w: &World, x0: f32, x1: f32) -> Vec<f32> {
    w.walk.crossings.iter().filter(|c| !c.closed && c.kind != CrossingKind::Jay && c.mid.y.abs() < 1.0 && c.mid.x > x0 && c.mid.x < x1).map(|c| c.mid.x).collect()
}

/// Harvey: "if a crosswalk is 6ft away and I drop a new sidewalk/path on
/// the other side of the road, it won't make two crosswalks 6 ft apart".
/// A path dropped on the far pavement a few steps along from where another
/// path meets the street joins it there: one point, one crossing.
#[test]
fn a_path_dropped_across_the_street_from_another_shares_its_crossing() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_path(&[v(60.0, 60.0), v(60.0, 6.5)]);
    assert_eq!(path_meets_near(&w, v(60.0, 0.0), 2.0), 1, "the first path meets the street");
    w.add_path(&[v(63.0, -60.0), v(63.0, -6.5)]);
    assert!(PATH_MEETS > 3.0);
    assert_eq!(path_meets_near(&w, v(61.5, 0.0), 15.0), 1, "the second path meets the street where the first does");
    assert!(street_pieces(&w).iter().all(|&(_, len)| len >= MIN_PIECE), "no stub: {:?}", street_pieces(&w));
    assert_eq!(open_over_main(&w, 40.0, 80.0).len(), 1, "one crossing there: {:?}", open_over_main(&w, 40.0, 80.0));
    // Both paths are joined to the walk graph over that crossing.
    let (pts, _) = w.walk.route(v(60.0, 60.0), v(63.0, -60.0)).expect("a walk from one path to the other");
    assert!(pts.len() >= 2);
}

/// A path dropped by the street a few metres from one of the street's
/// nodes meets the street at that node: no stub of street cut off.
#[test]
fn a_path_end_beside_a_street_node_cuts_no_stub() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    // Lands on the street 4.5 m from the junction.
    w.add_path(&[v(4.5, -80.0), v(4.5, -7.0)]);
    assert!(street_pieces(&w).iter().all(|&(_, len)| len >= MIN_PIECE), "no stub: {:?}", street_pieces(&w));
    let j = w.roads.node_near(v(0.0, 0.0), 0.5).expect("the junction");
    assert!(w.roads.node(j).segments.iter().any(|&s| w.roads.segment(s).alive && w.roads.segment(s).kind == RoadKind::Path), "the path joins the junction");
    // Further along it cuts the street as before, with its own crossing.
    w.add_path(&[v(110.0, -80.0), v(110.0, -7.0)]);
    assert_eq!(path_meets_near(&w, v(110.0, 0.0), 1.0), 1, "mid-block it meets the street where it was drawn");
    assert_eq!(open_over_main(&w, 100.0, 120.0).len(), 1, "and keeps its crossing there");
}

/// Harvey's ring: an underpass path passing under the street 1.9 m from a
/// junction cut a 1.9 m stub off it (and its side street a 1.4 m one),
/// shorter than the junction itself; a sedan on it was drawn floating. The
/// junction's own node stands for the vertex over the way below.
#[test]
fn a_way_under_a_street_beside_its_junction_cuts_no_stub() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    w.project = None;
    w.add_gesture(Gesture::Road { points: vec![v(-60.0, -80.0), v(-2.0, -1.0), v(2.0, 1.5), v(60.0, 80.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Path, elevation: vec![-10.0; 4], car_only: None });
    w.try_implement().expect("the underpass is built");
    assert!(street_pieces(&w).iter().all(|&(_, len)| len >= MIN_PIECE), "no stub: {:?}", street_pieces(&w));
    assert!(w.roads.grade_crossings().iter().any(|c| c.at.length() < 4.0), "the path passes under the street");
}

/// Two junctions 14 m apart along a street set their crosswalks over it
/// a couple of metres apart, each set back towards the other (Harvey's
/// ring had three crossings within 3.6 m): the first serves.
#[test]
fn neighbouring_junctions_share_one_crosswalk() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    w.add_road(&[v(14.0, 0.0), v(14.0, -150.0)], 1, 1);
    let between = open_over_main(&w, 0.5, 13.5);
    assert_eq!(between.len(), 1, "one crosswalk between the two junctions: {between:?}");
    // The far sides keep theirs.
    assert_eq!(open_over_main(&w, -20.0, -0.5).len(), 1);
    assert_eq!(open_over_main(&w, 14.5, 30.0).len(), 1);
}

/// Harvey: "walkers should prioritize an overpass nearby instead of a
/// crosswalk if they can". A crosswalk over the same street within
/// `BRIDGE_DRAW` of a footbridge (and beyond what it replaces outright)
/// costs a route the bridged crossing's penalty; walkers from beside it go
/// over the bridge.
#[test]
fn walkers_prefer_a_footbridge_a_short_way_off() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    w.add_road(&[v(45.0, 0.0), v(45.0, 150.0)], 1, 1);
    let east_of = |w: &World, x: f32| w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Junction && c.mid.y.abs() < 1.0 && (c.mid.x - x).abs() < 1.5).expect("a crosswalk there") as u32;
    let bridge_at = east_of(&w, 8.3);
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(bridge_at, CrossingUpgrade::Footbridge).unwrap();
    let far = &w.walk.crossings[east_of(&w, 53.3) as usize];
    assert!(!far.closed && far.near_bridge, "the crosswalk 45 m on stays, dearer");
    assert!(far.route_cost(false).is_some_and(|c| c >= BRIDGED_GRADE_PENALTY));
    // Further off it costs nothing extra.
    let off = w.walk.crossings.iter().find(|c| c.kind == CrossingKind::Junction && c.mid.y.abs() < 1.0 && c.mid.x < -5.0).expect("the crosswalk west of the first junction");
    assert!(!off.near_bridge || off.mid.x > -50.0);
    // Straight over from x = 34: the crosswalk is 19 m on, the bridge 26 m
    // back. Over the bridge (it used to be the crosswalk).
    let (pts, nodes) = w.walk.route(v(34.0, 7.0), v(34.0, -7.0)).expect("a way over");
    assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0), "over the bridge: {pts:?}");
}

/// A path passing under the street draws walkers off the crosswalks near
/// it as a footbridge does.
#[test]
fn crosswalks_by_an_underpass_cost_more() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(30.0, 0.0), v(30.0, 150.0)], 1, 1);
    w.add_road(&[v(150.0, 0.0), v(150.0, 150.0)], 1, 1);
    w.project = None;
    w.add_gesture(Gesture::Road { points: vec![v(0.0, -60.0), v(0.0, 60.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Path, elevation: vec![-10.0, -10.0], car_only: None });
    w.try_implement().expect("the underpass is built");
    let near = w.walk.crossings.iter().find(|c| !c.closed && c.kind == CrossingKind::Junction && c.mid.y.abs() < 1.0 && (c.mid.x - 21.7).abs() < 1.5).expect("the crosswalk by the underpass");
    assert!(near.near_bridge, "dearer by the underpass");
    let far = w.walk.crossings.iter().find(|c| !c.closed && c.kind == CrossingKind::Junction && c.mid.y.abs() < 1.0 && (c.mid.x - 141.7).abs() < 1.5).expect("a crosswalk far off");
    assert!(!far.near_bridge, "not far off");
}

/// Harvey: bikes "flying up in the air". A cyclist going over a footbridge
/// wheels the bike up the stairs and over the deck: drawn on foot there.
#[test]
fn a_cyclist_on_a_footbridge_walks_the_bike() {
    let mut w = flat();
    w.add_road(&[v(-300.0, 0.0), v(300.0, 0.0)], 2, 2);
    w.add_path(&[v(0.0, 120.0), v(0.0, 0.0)]);
    w.add_path(&[v(0.0, 0.0), v(0.0, -120.0)]);
    w.add_zone(vec![v(-150.0, 10.0), v(-50.0, 10.0), v(-50.0, 70.0), v(-150.0, 70.0)], LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for lot in lots.into_iter().take(3) {
        w.build_on(lot);
    }
    let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in sites {
        w.finish_construction(id);
    }
    let mut minds = IdleMind;
    let mut guard = 0;
    while w.citizens.is_empty() && guard < 2880 {
        w.tick(&mut minds);
        guard += 1;
    }
    let id = w.citizens.keys().next().expect("someone moved in");
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra && c.mid.distance(Vec2::ZERO) < 3.0).expect("the path's zebra") as u32;
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let a = w.roads.node_near(v(0.0, 120.0), 1.0).unwrap();
    let b = w.roads.node_near(v(0.0, -120.0), 1.0).unwrap();
    let nodes = w.roads.foot_path(a, b).expect("a way along the path");
    let mut path = w.route_path(&nodes, TravelMode::Bike);
    w.bike_over_footbridges(&mut path);
    let k = path.iter().position(|&p| w.walk.level_at_node(p) > 0.0).expect("over the bridge");
    let c = &mut w.citizens[id];
    c.inside = None;
    c.mode = TravelMode::Bike;
    c.pos = path[k - 1].lerp(path[k], 0.5);
    c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path: path.clone(), idx: k };
    assert!(w.wheeling_bike(&w.citizens[id]), "up the stairs on foot");
    let c = &mut w.citizens[id];
    c.pos = path[1].lerp(path[0], 0.5);
    c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 1 };
    assert!(!w.wheeling_bike(&w.citizens[id]), "riding along the path");
}

/// An old save whose roads were laid by older rules (here: a stub the plan
/// no longer lays, and a footbridge kept by a crossing middle that moved)
/// is laid again from the plan on load, and keeps its footbridge.
#[test]
fn an_old_save_is_laid_again_and_keeps_its_footbridges() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 150.0)], 1, 1);
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Junction && c.mid.y.abs() < 1.0 && c.mid.x > 0.0).expect("the crosswalk east of the junction") as u32;
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    assert_eq!(w.walk.footbridges.len(), 1);
    // As an older build left it: a piece the plan no longer lays (here a
    // stray path no gesture draws), and the bridge's rule at a crosswalk
    // middle a few metres in from where the crosswalk is now.
    w.roads.add_polyline_kind(&[v(2.0, 0.0), v(2.0, -30.0)], 0, 0, RoadKind::Path);
    for r in &mut w.crosswalks.rules {
        r.at.x -= 5.0;
    }
    let text = w.to_save_ron().expect("save");
    let back = World::from_ron(&text).expect("load");
    assert!(!back.roads.alive_segments().any(|(_, s)| s.kind == RoadKind::Path), "the stray piece is gone: laid from the plan");
    assert!(street_pieces(&back).iter().all(|&(_, len)| len >= MIN_PIECE), "no stub: {:?}", street_pieces(&back));
    assert_eq!(back.walk.footbridges.len(), 1, "the footbridge is kept");
    let f = &back.walk.footbridges[0];
    assert!(back.walk.crossings[f.crossing as usize].mid.distance(w.walk.crossings[cr as usize].mid) < 1.0, "over the crosswalk it was built on");
    // A save laid as its plan lays it loads unchanged.
    let again = World::from_ron(&back.to_save_ron().unwrap()).unwrap();
    assert_eq!(again.roads.nodes.len(), back.roads.nodes.len());
}
