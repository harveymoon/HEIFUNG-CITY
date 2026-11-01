//! [r5-stairs] Footbridge stairs fit their pavement: a straight flight
//! where the block is long, a switchback (down and back round a landing)
//! where it is shorter, a spiral tower where it is short. Walkers climb the
//! stairs' real walk line (its length and climb in the walk graph), riders
//! wheel their bikes over every point of it, and the stairs are laid again
//! for the room there is when their junction moves.

use super::*;
use crate::junctions::{CrossingUpgrade, FOOTBRIDGE_LEVEL};
use crate::walk::{CrossingKind, StairKind};

fn flat() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1.0e7;
    w
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// The junction crosswalk at `at` over the leg towards `towards`.
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

/// A long street along x with side streets at x = 0 and x = `block`; a
/// footbridge at the x = 0 junction over the leg towards the other.
fn bridged_block(block: f32) -> World {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, -120.0), v(0.0, 120.0)], 1, 1);
    w.add_road(&[v(block, -120.0), v(block, 120.0)], 1, 1);
    let k = crossing_towards(&w, Vec2::ZERO, v(block, 0.0));
    w.set_crossing_upgrade(k, CrossingUpgrade::Footbridge).unwrap();
    assert_eq!(w.walk.footbridges.len(), 1);
    w
}

/// The walk graph's stairs are their walk line: a node at every point, at
/// its level, each piece as long as the walk along its slope.
fn stairs_walk_their_line(w: &World) {
    for f in &w.walk.footbridges {
        for (i, st) in f.stairs.iter().enumerate() {
            if f.joined[i] {
                continue;
            }
            assert_eq!(st.path[0].0, f.top[i]);
            assert_eq!(st.foot(), Some(f.foot[i]));
            assert!((st.path[0].1 - FOOTBRIDGE_LEVEL).abs() < 1e-4 && st.path.last().unwrap().1 == 0.0);
            let mut walked = 0.0;
            for p in st.path.windows(2) {
                assert!(p[1].1 <= p[0].1 + 1e-4, "always down from the top");
                let a = w.walk.nodes.iter().position(|n| n.pos == p[0].0 && (n.level - p[0].1).abs() < 1e-4).expect("a node at every point") as u32;
                let b = w.walk.nodes.iter().position(|n| n.pos == p[1].0 && (n.level - p[1].1).abs() < 1e-4).expect("a node at every point") as u32;
                let e = w.walk.adj[a as usize].iter().find(|e| e.to == b).expect("joined in order");
                assert!(e.raised);
                walked += e.len;
            }
            assert!((walked - st.walk_length()).abs() < 0.05, "the walk graph walks the stair's length: {walked} vs {}", st.walk_length());
            // Never shorter than the climb at the flights' grade.
            assert!(walked >= FOOTBRIDGE_LEVEL / crate::walk::STAIR_GRADE * 0.95, "{:?}: {walked} m", st.kind);
            for &(q, _) in &st.path {
                assert!(!w.in_carriageway(q), "{:?} out in the road at {q}", st.kind);
            }
        }
    }
}

#[test]
fn stairs_fit_the_pavement_they_have() {
    for (block, want) in [(120.0, StairKind::Straight), (31.0, StairKind::Switchback), (26.0, StairKind::Spiral)] {
        let w = bridged_block(block);
        let f = &w.walk.footbridges[0];
        assert_eq!([f.stairs[0].kind, f.stairs[1].kind], [want; 2], "a {block} m block");
        stairs_walk_their_line(&w);
        // All of it on the pavement between the junctions.
        for st in &f.stairs {
            for &(q, _) in &st.path {
                assert!(q.x > 0.0 && q.x < block, "{want:?} stands off its block at {q}");
            }
        }
        // Walkers cross by it.
        let c = &w.walk.crossings[f.crossing as usize];
        let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
        let (path, nodes) = w.walk.route(a + v(2.0, 0.0), b + v(2.0, 0.0)).expect("a way over");
        assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level >= FOOTBRIDGE_LEVEL - 0.01), "over the bridge: {path:?}");
        let up = nodes.iter().filter(|&&n| w.walk.nodes[n as usize].level > 0.0).count();
        let points = f.stairs.iter().map(|s| s.path.len()).sum::<usize>();
        assert!(up + 2 >= points, "up every point of the stairs ({up} of {points})");
    }
}

/// A spiral's walk is longer than a straight flight (round and round),
/// a switchback's a little longer for its landing; both take far less of
/// the pavement.
#[test]
fn compact_stairs_take_less_pavement_and_walk_further() {
    let length = |block: f32| {
        let w = bridged_block(block);
        let f = &w.walk.footbridges[0];
        let st = &f.stairs[0];
        let reach = st.path.iter().map(|p| (p.0 - f.top[0]).dot(st.along)).fold(0.0f32, f32::max);
        (st.kind, st.walk_length(), reach)
    };
    let (k0, straight, r0) = length(120.0);
    let (k1, switchback, r1) = length(31.0);
    let (k2, spiral, r2) = length(26.0);
    assert_eq!((k0, k1, k2), (StairKind::Straight, StairKind::Switchback, StairKind::Spiral));
    assert!(switchback > straight && spiral > straight, "walks: straight {straight:.1}, switchback {switchback:.1}, spiral {spiral:.1}");
    assert!(r1 < r0 * 0.8 && r2 < r1, "reach along the pavement: {r0:.1}, {r1:.1}, {r2:.1}");
}

/// Riders wheel their bikes up the stairs and over: a cyclist's route over
/// the bridge passes every point of a spiral, and every leg of it on the
/// stairs is walked (`wheeling_bike`), so no rider is drawn climbing in the
/// air beside it.
#[test]
fn riders_wheel_their_bikes_round_a_spiral() {
    let w = bridged_block(26.0);
    let f = w.walk.footbridges[0].clone();
    assert_eq!(f.stairs[0].kind, StairKind::Spiral);
    let over = f.over_from(0);
    assert_eq!(over.len(), f.stairs[0].path.len() + f.stairs[1].path.len());
    assert_eq!(over[0], f.foot[0]);
    assert_eq!(*over.last().unwrap(), f.foot[1]);
    for leg in over.windows(2) {
        assert!(w.leg_raised(leg[0], leg[1]), "a leg on the stairs not raised: {leg:?}");
        let mid = (leg[0] + leg[1]) * 0.5;
        assert!(w.walk.level_on_leg(leg[0], leg[1], mid) > 0.0);
    }
}

/// The stairs belong to their junction's crossing: moved so the block is
/// long, the spiral is laid again as a straight flight; moved back, a
/// spiral again.
#[test]
fn stairs_refit_when_the_junction_moves() {
    let mut w = bridged_block(26.0);
    assert_eq!(w.walk.footbridges[0].stairs[0].kind, StairKind::Spiral);
    let drag = |w: &mut World, from: Vec2, to: Vec2| {
        let plan = w.effective_plan().clone();
        let j = w.joint_at(&plan, from, 8.0).expect("the junction");
        w.set_gestures(j.moved(&plan, to), true);
        w.try_implement().expect("implemented");
    };
    drag(&mut w, Vec2::ZERO, v(-100.0, 0.0));
    assert_eq!(w.walk.footbridges.len(), 1, "the bridge went with its junction");
    let f = &w.walk.footbridges[0];
    assert!(w.walk.crossings[f.crossing as usize].node == w.roads.node_near(v(-100.0, 0.0), 0.5));
    assert_eq!(f.stairs.clone().map(|s| s.kind), [StairKind::Straight; 2], "a long block now");
    stairs_walk_their_line(&w);
    drag(&mut w, v(-100.0, 0.0), Vec2::ZERO);
    assert_eq!(w.walk.footbridges[0].stairs.clone().map(|s| s.kind), [StairKind::Spiral; 2]);
    stairs_walk_their_line(&w);
}

/// Two footbridges facing each other along a block share its pavement:
/// the second one's stairs fit in what the first leaves.
#[test]
fn two_bridges_on_one_block_share_its_pavement() {
    let mut w = bridged_block(40.0);
    let k = crossing_towards(&w, v(40.0, 0.0), Vec2::ZERO);
    w.set_crossing_upgrade(k, CrossingUpgrade::Footbridge).expect("room for a second, with compact stairs");
    assert_eq!(w.walk.footbridges.len(), 2);
    stairs_walk_their_line(&w);
    let kinds: Vec<StairKind> = w.walk.footbridges.iter().map(|f| f.stairs[0].kind).collect();
    assert!(kinds.iter().any(|k| *k != StairKind::Straight), "{kinds:?}");
    // No stair stands in another's way.
    let (a, b) = (&w.walk.footbridges[0], &w.walk.footbridges[1]);
    for sa in &a.stairs {
        for sb in &b.stairs {
            for p in &sa.path {
                for q in &sb.path {
                    assert!(p.0.distance(q.0) > 1.5 || (p.0 - q.0).dot(sa.out).abs() > 3.0, "stairs overlap at {} / {}", p.0, q.0);
                }
            }
        }
    }
}

