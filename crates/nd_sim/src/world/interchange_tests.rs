//! [interchange] Interchanges: the diamond the Road tool's Interchange
//! preset lays, the same drawn by hand, a street added under a highway with
//! ramps, and road ends that must not join ways at other levels.

use super::*;
use crate::roads::{RoadKind, Segment, GRADE_SEPARATION};

fn flat_world() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() })
}

/// The highway as the game lays it: eastbound on the north (+y) side,
/// westbound on the south, three lanes each, 18 m apart.
fn highway(w: &mut World) {
    w.add_road(&[Vec2::new(-1500.0, 9.0), Vec2::new(1500.0, 9.0)], 3, 0);
    w.add_road(&[Vec2::new(1500.0, -9.0), Vec2::new(-1500.0, -9.0)], 3, 0);
}

fn alive_at(w: &World, n: NodeId) -> Vec<SegmentId> {
    w.roads.node(n).segments.iter().copied().filter(|&s| w.roads.segment(s).alive).collect()
}

fn two_way(s: &Segment) -> bool {
    s.is_street() && s.lanes_fwd > 0 && s.lanes_bwd > 0
}

/// No node where a highway carriageway meets a two-way street or a path:
/// the cross street passes under the highway and meets only its ramps.
fn no_at_grade_highway_junction(w: &World) {
    for (i, n) in w.roads.nodes.iter().enumerate() {
        let segs = alive_at(w, NodeId(i as u32));
        let hw = segs.iter().any(|&s| w.roads.segment(s).is_highway());
        let other = segs.iter().any(|&s| { let x = w.roads.segment(s); two_way(x) || x.kind == RoadKind::Path });
        assert!(!(hw && other), "a highway meets a street or a path at grade at {:?}: {:?}", n.pos, segs.iter().map(|&s| w.roads.segment(s).clone()).collect::<Vec<_>>());
    }
}

/// The ramp terminals: nodes with a two-way street and a ramp.
fn terminals(w: &World) -> Vec<NodeId> {
    (0..w.roads.nodes.len() as u32)
        .map(NodeId)
        .filter(|&n| {
            let segs = alive_at(w, n);
            segs.iter().any(|&s| w.roads.segment(s).ramp) && segs.iter().any(|&s| two_way(w.roads.segment(s)))
        })
        .collect()
}

/// Ramps arriving at (in) and leaving (out) node `n`.
fn ramps_in_out(w: &World, n: NodeId) -> (usize, usize) {
    let mut io = (0, 0);
    for s in alive_at(w, n) {
        let seg = w.roads.segment(s);
        if !seg.ramp {
            continue;
        }
        let arrives = (seg.b == n && seg.lanes_fwd > 0) || (seg.a == n && seg.lanes_bwd > 0);
        if arrives {
            io.0 += 1;
        } else {
            io.1 += 1;
        }
    }
    io
}

/// Nodes on a carriageway where a ramp joins it.
fn highway_joins(w: &World) -> Vec<NodeId> {
    (0..w.roads.nodes.len() as u32)
        .map(NodeId)
        .filter(|&n| {
            let segs = alive_at(w, n);
            segs.iter().any(|&s| w.roads.segment(s).is_highway()) && segs.iter().any(|&s| w.roads.segment(s).ramp)
        })
        .collect()
}

fn route(w: &World, from: Vec2, to: Vec2) -> bool {
    let (s0, _, p0) = w.roads.nearest_street(from).unwrap();
    let (s1, _, p1) = w.roads.nearest_street(to).unwrap();
    w.traffic.net.find_route(s0, p0, s1, p1).is_some()
}

/// Check a built diamond across the highway at x = 0: two terminals on the
/// cross street (one per side, a ramp in and a ramp out at each, signals),
/// four joins on the carriageways, the street under both carriageways and
/// no at-grade junction between the highway and the street; every movement
/// possible.
fn check_diamond(w: &World) {
    no_at_grade_highway_junction(w);
    let t = terminals(w);
    assert_eq!(t.len(), 2, "one terminal per side: {:?}", t.iter().map(|&n| w.roads.node(n).pos).collect::<Vec<_>>());
    let ys: Vec<f32> = t.iter().map(|&n| w.roads.node(n).pos.y).collect();
    assert!(ys.iter().any(|&y| y > 20.0) && ys.iter().any(|&y| y < -20.0), "a terminal each side: {ys:?}");
    for &n in &t {
        assert_eq!(ramps_in_out(w, n), (1, 1), "an off-ramp in and an on-ramp out at {:?}", w.roads.node(n).pos);
        assert!(w.roads.node(n).level.abs() < 0.01, "the terminal is at the street's level");
        let k = w.traffic.net.node_intersection.get(n.0 as usize).copied().unwrap_or(usize::MAX);
        assert!(k != usize::MAX && w.traffic.net.intersections[k].signalized, "the terminal at {:?} runs signals", w.roads.node(n).pos);
    }
    let joins = highway_joins(w);
    assert_eq!(joins.len(), 4, "four ramp joins on the carriageways: {:?}", joins.iter().map(|&n| w.roads.node(n).pos).collect::<Vec<_>>());
    for &n in &joins {
        assert_eq!(alive_at(w, n).len(), 3, "a plain diverge or merge at {:?}", w.roads.node(n).pos);
    }
    // The street passes under both carriageways, which are lifted over it.
    let under: Vec<_> = w.roads.grade_crossings().into_iter().filter(|c| c.at.x.abs() < 30.0 && c.at.y.abs() < 12.0).collect();
    assert_eq!(under.len(), 2, "{under:?}");
    for c in &under {
        assert!(w.roads.segment(c.upper.0).is_highway() && two_way(w.roads.segment(c.lower.0)), "{c:?}");
        let clear = w.road_height(c.upper.0, c.upper.1) - w.road_height(c.lower.0, c.lower.1);
        assert!(clear >= crate::landscape::UNDERPASS_CLEARANCE - 0.05, "headroom {clear}");
    }
    // Off the highway into town either way, and back on in both directions.
    assert!(route(w, Vec2::new(-400.0, 9.0), Vec2::new(0.0, -180.0)), "eastbound off to the south");
    assert!(route(w, Vec2::new(-400.0, 9.0), Vec2::new(0.0, 180.0)), "eastbound off to the north");
    assert!(route(w, Vec2::new(400.0, -9.0), Vec2::new(0.0, 180.0)), "westbound off to the north");
    assert!(route(w, Vec2::new(0.0, -180.0), Vec2::new(400.0, 9.0)), "on eastbound from the south");
    assert!(route(w, Vec2::new(0.0, 180.0), Vec2::new(-400.0, -9.0)), "on westbound from the north");
}

#[test]
fn the_interchange_tool_lays_a_clean_diamond() {
    let mut w = flat_world();
    highway(&mut w);
    let (plan, ids) = w.add_diamond(Vec2::new(0.0, -220.0), Vec2::new(0.0, 220.0), (2, 2)).expect("a diamond fits");
    assert_eq!(ids.len(), 5, "the street and four ramps");
    assert!(!plan.kept_street);
    assert_eq!(plan.terminals.len(), 2);
    w.try_implement().expect("implemented");
    check_diamond(&w);
    // Every ramp joins its carriageway nearly tangent, on the outer side,
    // and stays clear of the cross street until its terminal.
    for (sid, s) in w.roads.alive_segments() {
        if !s.ramp {
            continue;
        }
        for n in [s.a, s.b] {
            let p = w.roads.node(n).pos;
            if terminals(&w).contains(&n) {
                continue;
            }
            assert!(p.x.abs() > 20.0, "a ramp node {p:?} of {sid:?} stands on the cross street");
            assert!(p.y.abs() >= 8.9, "a ramp node {p:?} between the carriageways");
        }
    }
}

#[test]
fn the_interchange_tool_keeps_a_street_already_under_the_highway() {
    let mut w = flat_world();
    highway(&mut w);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 2, 2);
    let segs = w.roads.alive_segments().count();
    // Drawn roughly along it.
    let (plan, ids) = w.add_diamond(Vec2::new(2.0, -200.0), Vec2::new(-1.0, 200.0), (2, 2)).expect("a diamond fits");
    assert!(plan.kept_street, "the street there is kept");
    assert_eq!(ids.len(), 4, "only the ramps");
    w.try_implement().expect("implemented");
    assert!(w.roads.alive_segments().count() > segs);
    check_diamond(&w);
}

/// Harvey's own diamond (his city, day 23): ramps drawn first from the
/// carriageways to two points, then the street from one point to the other
/// and on north, under the highway. The ramps reach the street only 40 to
/// 50 m along the highway from where they leave it; with a fixed 40 m lead
/// each ran alongside the highway right up to the street under it and
/// hairpinned onto it over the street's own asphalt.
#[test]
fn a_hand_drawn_short_diamond_keeps_its_ramps_off_the_street() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-1500.0, 429.0), Vec2::new(1500.0, 429.0)], 3, 0);
    w.add_road(&[Vec2::new(1500.0, 411.0), Vec2::new(-1500.0, 411.0)], 3, 0);
    w.add_road(&[Vec2::new(682.92, 411.0), Vec2::new(632.09, 371.7)], 1, 0);
    w.add_road(&[Vec2::new(633.51, 371.73), Vec2::new(578.58, 411.0)], 1, 0);
    w.add_road(&[Vec2::new(566.81, 429.0), Vec2::new(608.12, 472.8)], 1, 0);
    w.add_road(&[Vec2::new(606.72, 471.3), Vec2::new(668.46, 429.0)], 1, 0);
    w.add_road(&[Vec2::new(608.12, 472.8), Vec2::new(632.09, 371.7), Vec2::new(635.78, 269.71)], 2, 2);
    no_at_grade_highway_junction(&w);
    let t = terminals(&w);
    assert_eq!(t.len(), 2, "the ramps meet the street at one node each side: {:?}", t.iter().map(|&n| w.roads.node(n).pos).collect::<Vec<_>>());
    for &n in &t {
        assert_eq!(ramps_in_out(&w, n), (1, 1));
    }
    // The street's line: x at a given y.
    let street_x = |y: f32| 608.12 + (632.09 - 608.12) * (y - 472.8) / (371.7 - 472.8);
    for (_, s) in w.roads.alive_segments().filter(|(_, s)| s.ramp) {
        for n in [s.a, s.b] {
            if t.contains(&n) {
                continue;
            }
            let p = w.roads.node(n).pos;
            // Between the carriageways' outer edges and the terminal, a ramp
            // node keeps the ramp's asphalt off the street's: half the
            // street (8.5 m) and half the ramp (3.25 m).
            let off = (p.x - street_x(p.y)).abs();
            let near_terminal = t.iter().any(|&m| w.roads.node(m).pos.distance(p) < 22.0);
            assert!(off > 11.75 || near_terminal, "a ramp node {p:?} {off:.1} m from the street's centre line");
        }
    }
}

/// A street drawn under a highway that already has a diamond, crossing an
/// off-ramp on its way: the ramp is not cut into a junction (which ended it
/// there and turned the rest into a town street); the street passes under
/// it, and the ramps stay as they were.
#[test]
fn a_street_added_under_a_highway_with_ramps_leaves_the_ramps_alone() {
    let mut w = flat_world();
    highway(&mut w);
    w.add_diamond(Vec2::new(0.0, -220.0), Vec2::new(0.0, 220.0), (2, 2)).unwrap();
    w.try_implement().unwrap();
    let ramp_pieces = |w: &World| -> Vec<(Vec2, Vec2)> { w.roads.alive_segments().filter(|(_, s)| s.ramp).map(|(id, _)| w.roads.segment_points(id)).collect() };
    let before = ramp_pieces(&w);
    // A diagonal street under the highway, crossing the eastbound off-ramp
    // and the westbound on-ramp.
    w.add_road(&[Vec2::new(-200.0, -200.0), Vec2::new(-60.0, 200.0)], 1, 1);
    // The ramps run where they ran (a piece may gain a vertex over the
    // street, to be lifted clear of it), still all ramps, joined to nothing
    // new: still two terminals and four joins (`check_diamond`).
    for (a, b) in ramp_pieces(&w) {
        for p in [a, b] {
            let off = before.iter().map(|&(x, y)| crate::geom::project_on_segment(p, x, y).1.distance(p)).fold(f32::INFINITY, f32::min);
            assert!(off < 0.3, "a ramp moved: {p:?} is {off:.1} m off where the ramps ran");
        }
    }
    check_diamond(&w);
    let crossings = w.roads.grade_crossings();
    assert!(crossings.iter().any(|c| w.roads.segment(c.upper.0).ramp && two_way(w.roads.segment(c.lower.0))), "the street passes under a ramp");
}

/// Road ends join a way at their own level when there is one in reach: a
/// street end dropped where a viaduct passes over a street joins the
/// street, a raised end there joins the viaduct.
#[test]
fn snapping_never_joins_ways_at_different_levels() {
    let mut w = flat_world();
    // A street along y = 0 and a viaduct 8 m up along x = 0 over it.
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let had = w.project.take();
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(0.0, -400.0), Vec2::new(0.0, -250.0), Vec2::new(0.0, 250.0), Vec2::new(0.0, 400.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, 8.0, 8.0, 0.0] });
    w.try_implement().expect("the viaduct is built");
    w.project = had;
    // A ground street from the south-west, its end dropped right at the
    // crossing (on the viaduct's vertex over it, on the street under it).
    w.add_road(&[Vec2::new(-150.0, -150.0), Vec2::new(2.0, -2.0)], 1, 1);
    let end = w.roads.nodes.iter().enumerate().filter(|(i, n)| n.pos.distance(Vec2::new(1.0, -1.0)) < 6.0 && alive_at(&w, NodeId(*i as u32)).iter().any(|&s| { let (a, b) = w.roads.segment_points(s); a.distance(Vec2::new(-150.0, -150.0)) < 1.0 || b.distance(Vec2::new(-150.0, -150.0)) < 1.0 })).map(|(_, n)| n.level).collect::<Vec<_>>();
    assert_eq!(end.len(), 1, "the end landed on one node");
    assert!(end[0].abs() < 0.01, "on the street at grade, not the viaduct: level {}", end[0]);
    // Every node with pieces of both levels? None: nothing joins them.
    for (i, n) in w.roads.nodes.iter().enumerate() {
        let segs = alive_at(&w, NodeId(i as u32));
        // A piece may ramp between levels along its length (the viaduct's
        // own approach from the ground), but no node holds a ground street
        // and a deck piece that both stay up.
        let up = segs.iter().filter(|&&s| { let o = w.roads.segment(s); w.roads.node(o.a).level >= GRADE_SEPARATION && w.roads.node(o.b).level >= GRADE_SEPARATION }).count();
        let down = segs.iter().filter(|&&s| { let o = w.roads.segment(s); w.roads.node(o.a).level.abs() < 0.01 && w.roads.node(o.b).level.abs() < 0.01 }).count();
        assert!(up == 0 || down == 0, "a node at {:?} joins the deck and the street", n.pos);
    }
    // A raised end (8 m) dropped on the viaduct, by the ground under it,
    // joins the viaduct.
    let had = w.project.take();
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(300.0, -150.0), Vec2::new(150.0, -150.0), Vec2::new(2.0, -120.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, 8.0, 8.0] });
    w.try_implement().expect("the raised street is built");
    w.project = had;
    let raised_end = w.roads.nodes.iter().enumerate().find(|(i, _)| alive_at(&w, NodeId(*i as u32)).iter().any(|&s| { let (a, b) = w.roads.segment_points(s); a.distance(Vec2::new(150.0, -150.0)) < 1.0 && b.distance(Vec2::new(0.0, -120.0)) < 6.0 || b.distance(Vec2::new(150.0, -150.0)) < 1.0 && a.distance(Vec2::new(0.0, -120.0)) < 6.0 }));
    let (_, n) = raised_end.expect("the raised street's end node");
    assert!(n.level >= GRADE_SEPARATION, "the raised end joins the viaduct: level {}", n.level);
}

/// A footpath whose end is dropped on an on-ramp, near the street the ramp
/// leaves (Harvey's path, day 23): it lands on the street, not on the ramp,
/// so nobody is led onto the slip road.
#[test]
fn a_path_end_never_lands_on_a_ramp() {
    let mut w = flat_world();
    highway(&mut w);
    w.add_diamond(Vec2::new(0.0, -220.0), Vec2::new(0.0, 220.0), (2, 2)).unwrap();
    w.try_implement().unwrap();
    // The westbound on-ramp leaves the south terminal westwards; drop a
    // path end right on it 7 m from the street.
    let (sid, _, q) = w.roads.nearest_where(Vec2::new(-7.0, -64.0), |s| s.ramp).unwrap();
    assert!(w.roads.segment(sid).ramp);
    w.add_path(&[Vec2::new(-200.0, -150.0), q]);
    for (i, _) in w.roads.nodes.iter().enumerate() {
        let segs = alive_at(&w, NodeId(i as u32));
        let path_end = segs.iter().filter(|&&s| w.roads.segment(s).kind == RoadKind::Path).count() == 1;
        if path_end && segs.len() > 1 {
            // On the street (its node may be the ramps' terminal), never on
            // a ramp alone.
            assert!(segs.iter().any(|&s| { let x = w.roads.segment(s); x.is_street() && !x.is_motorway() }), "a path joins only a ramp or the highway at {:?}", w.roads.node(NodeId(i as u32)).pos);
        }
    }
    check_diamond(&w);
}

#[test]
fn ramp_easing_is_idempotent_and_shows_in_the_preview() {
    let mut w = flat_world();
    highway(&mut w);
    let pts = [Vec2::new(-200.0, 9.0), Vec2::new(-120.0, 80.0)];
    let (eased, lv) = crate::roads::ease_ramp_way(&pts, &[], 1, 0, RoadKind::Street, &w.roads);
    assert!(eased.len() > 2, "a lead and a rounded bend: {eased:?}");
    assert_eq!(lv.len(), eased.len());
    let (again, _) = crate::roads::ease_ramp_way(&eased, &lv, 1, 0, RoadKind::Street, &w.roads);
    assert_eq!(again, eased, "easing twice changes nothing");
    // The lead never runs past where the ramp is going along the highway.
    for p in &eased {
        assert!(p.x <= -120.0 + 0.01, "{p:?} past the ramp's far end");
    }
    // A two-way street is never eased.
    let (plain, _) = crate::roads::ease_ramp_way(&pts, &[], 1, 1, RoadKind::Street, &w.roads);
    assert_eq!(plain, pts.to_vec());
}

#[test]
fn the_interchange_tool_refuses_with_the_spot() {
    let mut w = flat_world();
    highway(&mut w);
    // Not across the highway.
    let r = w.plan_diamond(Vec2::new(0.0, 100.0), Vec2::new(0.0, 300.0), (2, 2)).unwrap_err();
    assert!(r.why.contains("does not cross"), "{}", r.why);
    // Too short to reach its terminals.
    let r = w.plan_diamond(Vec2::new(0.0, -40.0), Vec2::new(0.0, 220.0), (2, 2)).unwrap_err();
    assert!(r.why.contains("too short") && r.at.distance(Vec2::new(0.0, -40.0)) < 1.0, "{} at {:?}", r.why, r.at);
    // A street in the way of the eastbound off-ramp (north side, west of
    // the crossing): refused at the crossing of the two.
    w.add_road(&[Vec2::new(-90.0, 24.0), Vec2::new(-90.0, 300.0)], 1, 1);
    let r = w.plan_diamond(Vec2::new(0.0, -220.0), Vec2::new(0.0, 220.0), (2, 2)).unwrap_err();
    assert!(r.why.contains("off-ramp would cross"), "{}", r.why);
    assert!((r.at.x + 90.0).abs() < 1.0 && r.at.y > 9.0, "the spot is where they cross: {:?}", r.at);
    // Nothing was added to any project.
    assert!(w.project.as_ref().is_none_or(|p| p.is_empty()));
}

/// [road-polish leftover] At a sharp fork the stop line is painted out past
/// the rounded kerb (`walk::crosswalk_setback`); the lanes now end where
/// cars stop at any junction, 4 m in from that line, not at the junction's
/// radius metres past it.
#[test]
fn cars_stop_at_a_sharp_forks_painted_line() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    let fork = Vec2::new(0.0, 0.0);
    w.add_road(&[fork, fork + Vec2::from_angle(25f32.to_radians()) * 150.0], 1, 1);
    let n = w.roads.node_near(fork, 0.5).unwrap();
    let net = &w.traffic.net;
    let r = net.node_radius[n.0 as usize];
    let mut sharp = 0;
    for s in alive_at(&w, n) {
        let want = crate::walk::crosswalk_setback(&w.roads, n, s) + crate::walk::CROSSWALK_HALF + 0.5 - 4.0;
        for &l in net.segment_lanes.get(&s).unwrap() {
            let lane = &net.lanes[l as usize];
            if lane.end_node != n {
                continue;
            }
            let d = lane.points.last().unwrap().distance(fork);
            // The lane's end lies off the centre line by up to half a lane.
            assert!(d + 0.1 >= want.max(r) - crate::roads::LANE_WIDTH && d <= want.max(r) + crate::roads::LANE_WIDTH, "lane end {d:.1} m out, the line wants {want:.1} (radius {r:.1})");
            if want > r + 1.0 {
                sharp += 1;
                assert!(d > r + 1.0, "a sharp arm's lane ends out past the radius: {d:.1} vs {r:.1}");
            }
        }
    }
    assert!(sharp >= 1, "the fork's sharp arms stop back");
    // Still a junction cars can drive through.
    assert!(route(&w, Vec2::new(-150.0, 0.0), fork + Vec2::from_angle(25f32.to_radians()) * 120.0));
}
