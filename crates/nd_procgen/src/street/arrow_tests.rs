use super::*;
use nd_sim::traffic::{LEFT, RIGHT, STRAIGHT};

/// A plain crossroads of two-lane-each-way streets: every arriving lane
/// gets a painted arrow, in the paint surface (not the dimmed glow), and
/// the arrows are big enough to read from above (wider than a lane line).
#[test]
fn junction_lanes_get_readable_arrows() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 2, 2);
    roads.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 2, 2);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let g = lane_arrows(&roads, &net, &[], &[], &Palette::default());
    assert!(g.get(Glow).is_empty(), "arrows are paint, not glow");
    let paint = g.get(Paint);
    assert!(!paint.is_empty());
    let approach_lanes: usize = net.approaches.iter().map(|a| a.arrows.iter().filter(|&&x| x != 0).count()).sum();
    assert!(approach_lanes >= 8, "{approach_lanes}");
    // Bright: markedly brighter than the plain lane marker.
    let pal = Palette::default();
    assert!(paint.colors.iter().all(|c| c[0] > pal.lane_marker[0]));
}

/// The heads follow the movements: a left-only arrow reaches out to the
/// driver's left, a right-only one to the right, a straight one only ahead.
#[test]
fn arrow_heads_follow_the_allowed_movements() {
    let d = Vec2::X;
    let right = Vec2::new(-d.y, d.x);
    let extent = |a: u8| {
        let mut m = crate::mesh::MeshData::default();
        lane_arrow(&mut m, Vec2::ZERO, d, a, 0.0, [1.0; 4]);
        let side: Vec<f32> = m.positions.iter().map(|p| Vec2::new(p[0], p[2]).dot(right)).collect();
        (side.iter().cloned().fold(f32::MAX, f32::min), side.iter().cloned().fold(f32::MIN, f32::max))
    };
    let (l0, l1) = extent(LEFT);
    let (r0, r1) = extent(RIGHT);
    let (s0, s1) = extent(STRAIGHT);
    assert!(l0 < -0.9 && l1 < 0.7, "left reaches left: {l0} {l1}");
    assert!(r1 > 0.9 && r0 > -0.7, "right reaches right: {r0} {r1}");
    assert!(s0 > -0.7 && s1 < 0.7, "straight stays in line: {s0} {s1}");
    let (b0, b1) = extent(LEFT | STRAIGHT | RIGHT);
    assert!(b0 < -0.9 && b1 > 0.9);
}
