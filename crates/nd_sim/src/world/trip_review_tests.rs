//! [trip-review] Harvey: "I see some strange things happen with bikers when
//! in intersections, they seem to take some zig-zag moves that seem
//! unnatural." A bike's path through a junction was the kerb lines of the
//! ways in and out joined at their corner: past the junction and back,
//! sideways in its middle where the ways differ in width, and nonsense
//! where a junction's arms are a few metres long. It is a curve now
//! (`route_path`), riders along a street no longer take the footbridge
//! over a side street (`rides_along_street`), and a ride no longer starts
//! by going past the door to the street's far node and back
//! (`trim_route_ends`).

use super::*;
use crate::citizens::TravelMode;
use crate::junctions::CrossingUpgrade;

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn node_at(w: &World, p: Vec2) -> NodeId {
    w.roads.nodes.iter().position(|n| n.pos.distance(p) < 1.0 && !n.segments.is_empty()).map(|i| NodeId(i as u32)).expect("a road node there")
}

/// The path resampled every half metre, as unit directions.
fn directions(path: &[Vec2]) -> Vec<Vec2> {
    let mut pts = Vec::new();
    for w in path.windows(2) {
        let n = (w[0].distance(w[1]) / 0.5).ceil().max(1.0) as usize;
        for k in 0..n {
            pts.push(w[0].lerp(w[1], k as f32 / n as f32));
        }
    }
    pts.push(*path.last().unwrap());
    pts.windows(2).filter(|w| w[0].distance(w[1]) > 1e-3).map(|w| (w[1] - w[0]).normalize()).collect()
}

/// (net turn, sum of every turn's size), degrees.
fn turns(path: &[Vec2]) -> (f32, f32) {
    let d = directions(path);
    let mut sum = 0.0;
    let mut net = 0.0;
    for w in d.windows(2) {
        let a = w[0].angle_to(w[1]).to_degrees();
        sum += a.abs();
        net += a;
    }
    (net, sum)
}

/// A crossroads of `ew` lanes each way east-west and `ns` north-south.
fn crossroads(ew: u8, ns: u8) -> World {
    let mut w = flat();
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], ew, ew);
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], ns, ns);
    w
}

fn ride(w: &World, through: &[Vec2]) -> Vec<Vec2> {
    let nodes: Vec<NodeId> = through.iter().map(|&p| node_at(w, p)).collect();
    let mut full = Vec::new();
    for pair in nodes.windows(2) {
        let leg = w.roads.foot_path(pair[0], pair[1]).expect("a way");
        if full.last() == leg.first() {
            full.extend_from_slice(&leg[1..]);
        } else {
            full.extend(leg);
        }
    }
    w.route_path(&full, TravelMode::Bike)
}

#[test]
fn a_cyclist_turns_through_a_crossroads_smoothly_one_way_only() {
    for (ew, ns) in [(1, 1), (2, 2), (2, 1)] {
        let w = crossroads(ew, ns);
        let ends = [Vec2::new(-200.0, 0.0), Vec2::new(0.0, -200.0), Vec2::new(200.0, 0.0), Vec2::new(0.0, 200.0)];
        for (i, &a) in ends.iter().enumerate() {
            for (j, &b) in ends.iter().enumerate() {
                if i == j {
                    continue;
                }
                let path = ride(&w, &[a, Vec2::ZERO, b]);
                let (net, sum) = turns(&path);
                // One way round only: every turn has the sign of the whole.
                assert!(sum - net.abs() < 1.0, "{ew}x{ns} lanes, {a} to {b}: turned {sum:.1} degrees in all for a net {net:.1}: {path:?}");
                // Straight on: no sideways step at all; a turn: about 90.
                let straight = (i + 2) % 4 == j;
                if straight {
                    assert!(sum < 1.0, "{a} to {b} straight over: {sum:.1} degrees of turning: {path:?}");
                } else {
                    assert!((net.abs() - 90.0).abs() < 2.0, "{a} to {b}: a quarter turn, not {net:.1}");
                }
                // And never back the way it came: always making progress
                // towards the way out, never off the kerb side either.
                let goal = (b - a).normalize();
                for d in directions(&path) {
                    assert!(d.dot(goal) > -0.05, "{a} to {b}: a step backwards {d}: {path:?}");
                }
            }
        }
    }
}

#[test]
fn straight_over_from_a_wide_street_to_a_narrow_one_shifts_smoothly() {
    // An avenue (2+2) going on east as a street (1+1) past a crossroads.
    let mut w = flat();
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
    for (a, b) in [(Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)), (Vec2::new(200.0, 0.0), Vec2::new(-200.0, 0.0))] {
        let path = ride(&w, &[a, Vec2::ZERO, b]);
        let goal = (b - a).normalize();
        let dirs = directions(&path);
        // The old path stepped straight sideways 3.5 m in the junction's
        // middle; a smooth shift never points more than 30 degrees off.
        for d in &dirs {
            assert!(d.dot(goal) > 30f32.to_radians().cos(), "{a} to {b}: {d} is a sideways jump: {path:?}");
        }
        // At most one S: left then right (or the other way), no more.
        let mut signs = Vec::new();
        for p in dirs.windows(2) {
            let t = p[0].angle_to(p[1]).to_degrees();
            if t.abs() > 0.05 && signs.last().is_none_or(|&s: &f32| s != t.signum()) {
                signs.push(t.signum());
            }
        }
        assert!(signs.len() <= 2, "{a} to {b}: the heading swung {} ways: {path:?}", signs.len());
    }
}

#[test]
fn a_dog_leg_over_a_short_stub_is_one_curve_not_a_zig_zag() {
    // Two junctions 8 m apart on an avenue: a street north from one, a
    // street south from the other, the stub between them shorter than the
    // avenue is wide.
    let mut w = flat();
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, -200.0)], 1, 1);
    w.add_road(&[Vec2::new(8.0, 0.0), Vec2::new(8.0, 200.0)], 1, 1);
    for (a, b) in [(Vec2::new(0.0, -200.0), Vec2::new(8.0, 200.0)), (Vec2::new(8.0, 200.0), Vec2::new(0.0, -200.0))] {
        let path = ride(&w, &[a, Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0), b]);
        let goal = (b - a).normalize();
        for d in directions(&path) {
            assert!(d.dot(goal) > 0.0, "{a} to {b}: back the way it came ({d}): {path:?}");
        }
    }
}

#[test]
fn riding_along_a_street_past_a_side_street_footbridge_stays_on_the_street() {
    // A T: an avenue east-west, a street north from it; the street's
    // crosswalk at the junction replaced by a footbridge.
    let mut w = flat();
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 200.0)], 1, 1);
    let side = w.walk.crossings.iter().position(|c| c.node.is_some_and(|n| w.roads.node(n).pos.distance(Vec2::ZERO) < 1.0) && c.mid.y > 3.0).expect("the side street's crosswalk") as u32;
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(side, CrossingUpgrade::Footbridge).unwrap();
    assert!(w.walk.crossings[side as usize].closed, "the bridge replaced the crosswalk");
    for (a, b) in [(Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)), (Vec2::new(200.0, 0.0), Vec2::new(-200.0, 0.0))] {
        let mut path = ride(&w, &[a, Vec2::ZERO, b]);
        w.bike_over_footbridges(&mut path);
        assert!(path.iter().all(|&p| w.walk.level_at_node(p) <= 0.0), "{a} to {b} along the avenue: up the side street's footbridge: {path:?}");
    }
    // Riding up the side street and over the avenue on its line is still
    // nothing to do with that bridge either: it spans the side street.
    let mut path = ride(&w, &[Vec2::new(0.0, 200.0), Vec2::ZERO, Vec2::new(200.0, 0.0)]);
    w.bike_over_footbridges(&mut path);
    assert!(path.iter().all(|&p| w.walk.level_at_node(p) <= 0.0), "turning out of the side street: {path:?}");
}

#[test]
fn a_ride_starts_and_ends_level_with_the_door() {
    // The route runs east from a node 60 m west of the door; the door is
    // abreast of its first leg, the destination abreast of its last.
    let mut path = vec![Vec2::new(-60.0, 4.0), Vec2::new(100.0, 4.0), Vec2::new(100.0, 200.0), Vec2::new(-50.0, 200.0)];
    World::trim_route_ends(&mut path, Vec2::new(0.0, -10.0), Vec2::new(20.0, 212.0));
    assert!(path[0].distance(Vec2::new(0.0, 4.0)) < 1e-3, "starts level with the door, not 60 m back: {}", path[0]);
    assert!(path[3].distance(Vec2::new(20.0, 200.0)) < 1e-3, "ends level with the door, not 70 m on: {}", path[3]);
    // A door beyond the leg's end, or far off it, leaves the route alone.
    let mut path = vec![Vec2::new(-60.0, 4.0), Vec2::new(100.0, 4.0), Vec2::new(100.0, 200.0)];
    let before = path.clone();
    World::trim_route_ends(&mut path, Vec2::new(-80.0, -10.0), Vec2::new(160.0, 100.0));
    assert_eq!(path, before);
}
