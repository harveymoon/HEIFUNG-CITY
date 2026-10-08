//! Highway slip roads: found from the network's shape, fronting nothing,
//! walked by nobody, and driven with a limit that eases from highway to
//! street speed along them. Also where cars are drawn on them.

use super::*;
use crate::traffic::HIGHWAY_SPEED;

fn flat_world() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() })
}

/// An eastbound highway along y = 0, a two-way street along y = -160, an
/// off-ramp from the highway down to the street and an on-ramp back up,
/// and a one-way street in town south of the street. Zoned residential
/// all round, so parcels and lots are cut.
fn ramp_town() -> World {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 3, 0);
    w.add_road(&[Vec2::new(-500.0, -160.0), Vec2::new(500.0, -160.0)], 1, 1);
    // Off: leaves the carriageway at x = -300 and meets the street.
    w.add_road(&[Vec2::new(-300.0, -5.0), Vec2::new(-150.0, -60.0), Vec2::new(-80.0, -160.0)], 1, 0);
    // On: from the street up to the carriageway at x = 300.
    w.add_road(&[Vec2::new(80.0, -160.0), Vec2::new(150.0, -60.0), Vec2::new(300.0, -5.0)], 1, 0);
    // A one-way street in town: no ramp.
    w.add_road(&[Vec2::new(0.0, -160.0), Vec2::new(0.0, -400.0)], 1, 0);
    w.add_zone(vec![Vec2::new(-500.0, -10.0), Vec2::new(500.0, -10.0), Vec2::new(500.0, -450.0), Vec2::new(-500.0, -450.0)], LandUse::Residential);
    w
}

fn ramp_segments(w: &World) -> Vec<SegmentId> {
    w.roads.alive_segments().filter(|(_, s)| s.ramp).map(|(id, _)| id).collect()
}

/// Distance from `p` to the nearest ramp centreline.
fn to_ramp(w: &World, p: Vec2) -> f32 {
    ramp_segments(w)
        .into_iter()
        .map(|sid| {
            let (a, b) = w.roads.segment_points(sid);
            crate::geom::project_on_segment(p, a, b).1.distance(p)
        })
        .fold(f32::MAX, f32::min)
}

#[test]
fn slip_roads_are_found_and_one_way_streets_in_town_are_not() {
    let w = ramp_town();
    let ramps = ramp_segments(&w);
    assert!(ramps.len() >= 4, "both ramps, every piece: {ramps:?}");
    for (sid, s) in w.roads.alive_segments() {
        let (a, b) = w.roads.segment_points(sid);
        let mid = (a + b) * 0.5;
        if s.is_highway() || (s.lanes_fwd > 0 && s.lanes_bwd > 0) {
            assert!(!s.ramp, "{sid:?} is no ramp");
        }
        // The one-way street in town (x = 0, south of the street).
        if mid.x.abs() < 1.0 && mid.y < -170.0 {
            assert!(!s.ramp && !s.is_motorway(), "a one-way street in town is a street");
        }
        if s.ramp {
            assert!(s.is_motorway() && !s.is_highway());
        }
    }
    // Flags come back after a save and load.
    let w2 = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(ramp_segments(&w2).len(), ramps.len());
}

#[test]
fn slip_roads_front_no_parcels_and_nobody_walks_on_them() {
    let w = ramp_town();
    let ramps = ramp_segments(&w);
    assert!(!ramps.is_empty());
    for (_, lot) in w.lots.iter() {
        assert!(!ramps.contains(&lot.front_segment), "a lot fronts a ramp: {:?}", lot.center);
    }
    // The one-way street in town is fronted.
    assert!(w.lots.values().any(|l| {
        let s = w.roads.segment(l.front_segment);
        s.alive && s.lanes_bwd == 0 && s.lanes_fwd == 1 && !s.ramp
    }));
    // No pavement runs along a ramp: no walk node within a pavement's
    // reach of one, away from where it meets the street.
    let reach = LANE_WIDTH * 0.5 + crate::roads::SHOULDER + crate::zoning::SIDEWALK + 0.5;
    for n in &w.walk.nodes {
        if n.pos.y > -140.0 && n.pos.y < -10.0 {
            assert!(to_ramp(&w, n.pos) > reach, "a walk node beside a ramp at {:?}", n.pos);
        }
    }
    // And walkers still get about the town on either side.
    assert!(w.walk.nodes.iter().any(|n| n.pos.y < -150.0));
}

use crate::roads::LANE_WIDTH;

/// Every ramp lane's limits, in driving order from the highway (an
/// off-ramp) or to it (an on-ramp): `(start, end)` limits of each lane
/// on the route of a car driven down it.
fn route_limits(w: &World, route: &[crate::traffic::LaneId]) -> Vec<(f32, f32, bool)> {
    route
        .iter()
        .map(|&l| {
            let lane = &w.traffic.net.lanes[l as usize];
            (lane.speed_limit, lane.limit_end, lane.ramp)
        })
        .collect()
}

#[test]
fn ramp_limits_ease_from_highway_to_street_speed() {
    let w = ramp_town();
    let net = &w.traffic.net;
    // Off: from the highway west of the diverge to the street.
    let from = Vec2::new(-600.0, 3.5);
    let to = Vec2::new(-20.0, -161.75);
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, ..) = net.find_route(fs, from, ts, to).expect("off-ramp route");
    let limits = route_limits(&w, &route);
    let on_ramp: Vec<(f32, f32)> = limits.iter().filter(|l| l.2).map(|l| (l.0, l.1)).collect();
    assert!(on_ramp.len() >= 3, "the route uses the ramp: {limits:?}");
    // Starts near highway speed, ends at street speed, never rises, and
    // no step between one lane's end and the next one's start.
    // [interchange] The ramp's bend after its lead is rounded (a curve of
    // short pieces, `ease_ramp_ends`), whose radius caps the limit sooner
    // than the single kink did: drivers ease off from the taper on.
    assert!(on_ramp[0].0 > HIGHWAY_SPEED - 6.0, "{on_ramp:?}");
    assert!(on_ramp.last().unwrap().1 < 14.0, "{on_ramp:?}");
    for w2 in on_ramp.windows(2) {
        assert!(w2[1].0 <= w2[0].1 + 0.5 && (w2[1].0 - w2[0].1).abs() < 3.0, "a step in the limit: {on_ramp:?}");
    }
    for l in &on_ramp {
        assert!(l.1 <= l.0 + 0.01, "the limit falls along the off-ramp: {on_ramp:?}");
    }
    // On: rising toward the highway.
    let from = Vec2::new(20.0, -161.75);
    let to = Vec2::new(700.0, 3.5);
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, ..) = net.find_route(fs, from, ts, to).expect("on-ramp route");
    let on_ramp: Vec<(f32, f32)> = route_limits(&w, &route).iter().filter(|l| l.2).map(|l| (l.0, l.1)).collect();
    assert!(on_ramp.len() >= 3);
    assert!(on_ramp[0].0 < 14.0 && on_ramp.last().unwrap().1 > HIGHWAY_SPEED - 6.0, "{on_ramp:?}");
    for l in &on_ramp {
        assert!(l.1 >= l.0 - 0.01, "the limit rises along the on-ramp: {on_ramp:?}");
    }
}

#[test]
fn cars_slow_smoothly_down_an_off_ramp_without_stopping() {
    let w = ramp_town();
    let mut t = w.traffic.clone();
    let from = Vec2::new(-700.0, 3.5);
    let to = Vec2::new(-20.0, -161.75);
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, s0, s1) = t.net.find_route(fs, from, ts, to).unwrap();
    let id = t.spawn_car(Default::default(), None, route, s0, s1, 0);
    // Up to speed on the highway first.
    t.cars[id].v = HIGHWAY_SPEED;
    t.cars[id].entering = false;
    let dt = 0.5;
    let mut on_ramp: Vec<(f32, f32)> = Vec::new();
    let mut prev_v = t.cars[id].v;
    let mut worst_brake: f32 = 0.0;
    for _ in 0..400 {
        if t.step(dt, 1).contains(&id) {
            break;
        }
        let c = &t.cars[id];
        let lane = &t.net.lanes[c.route[c.idx] as usize];
        if lane.ramp {
            on_ramp.push((c.v, lane.limit_at(c.s)));
            worst_brake = worst_brake.max((prev_v - c.v) / dt);
        }
        prev_v = c.v;
    }
    assert!(on_ramp.len() > 10, "the car drove down the ramp");
    // Never stopped or crawled on the ramp (it slows to turn into the
    // street at its end), never braked hard, and slowed down it in one
    // sweep, not in steps of braking and pulling away again.
    let min_v = on_ramp.iter().map(|x| x.0).fold(f32::MAX, f32::min);
    assert!(min_v > 5.0, "it kept moving down the ramp (slowest {min_v:.1} m/s): {on_ramp:?}");
    assert!(worst_brake < 3.6, "it braked at most firmly ({worst_brake:.1} m/s²): {on_ramp:?}");
    let rises = on_ramp.windows(2).filter(|w| w[1].0 > w[0].0 + 0.3).count();
    assert!(rises <= 3, "no stop-go down the ramp: {on_ramp:?}");
    // And it came off the highway fast, and reached the street slowed.
    assert!(on_ramp[0].0 > 18.0, "{on_ramp:?}");
    assert!(on_ramp.last().unwrap().0 < 14.5, "{on_ramp:?}");
}

#[test]
fn a_car_crosses_several_short_lanes_in_one_step_without_stalling() {
    // A curved one-way street of short pieces: a car at speed covers more
    // than a piece (and its connector) every step, and must be drawn
    // moving on at an even pace, not waiting at each piece's end.
    let mut w = flat_world();
    let pts: Vec<Vec2> = (0..=40).map(|k| {
        let a = k as f32 / 40.0 * std::f32::consts::FRAC_PI_2;
        Vec2::new(200.0 * a.cos(), 200.0 * a.sin())
    }).collect();
    w.add_road(&pts, 1, 0);
    let mut t = w.traffic.clone();
    let (fs, _, _) = w.roads.nearest_street(pts[1]).unwrap();
    let (ts, _, _) = w.roads.nearest_street(pts[38]).unwrap();
    let (route, s0, s1) = t.net.find_route(fs, pts[1], ts, pts[38]).unwrap();
    let id = t.spawn_car(Default::default(), None, route, s0, s1, 0);
    t.cars[id].entering = false;
    t.cars[id].v = 13.0;
    let mut steps: Vec<f32> = Vec::new();
    let mut last = t.cars[id].pos;
    for _ in 0..40 {
        t.step(1.0, 1);
        let c = &t.cars[id];
        if c.v > 10.0 && c.idx + 2 < c.route.len() {
            steps.push(c.pos.distance(last));
        }
        last = c.pos;
    }
    assert!(steps.len() > 5, "{steps:?}");
    let (lo, hi) = steps.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &d| (lo.min(d), hi.max(d)));
    assert!(hi - lo < 0.35 * hi, "an even pace through the short pieces: {steps:?}");
}

#[test]
fn a_car_is_drawn_at_the_height_of_the_road_where_it_is_drawn() {
    // A street climbing onto a deck: the height the renderer lifts a car
    // to follows its drawn position through the tick, not the tick's end.
    let mut w = flat_world();
    let a = Vec2::new(-300.0, -300.0);
    let b = Vec2::new(-300.0, 300.0);
    let had = w.project.take();
    w.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, 20.0] });
    w.implement();
    w.project = had;
    assert!(w.relief);
    let mut t = w.traffic.clone();
    // Northbound: the right-hand lane is west of the centreline.
    let (from, to) = (a + Vec2::new(-1.75, 30.0), b + Vec2::new(-1.75, -30.0));
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("a route up the street");
    let id = t.spawn_car(Default::default(), None, route, s0, s1, 0);
    t.cars[id].entering = false;
    t.cars[id].v = 13.0;
    for _ in 0..3 {
        t.step(4.0, 8);
    }
    w.traffic = t;
    let car = &w.traffic.cars[id];
    for k in 0..=8 {
        let (p, _) = car.pose_at(k as f32 / 8.0);
        let (sid, tt, _) = w.roads.nearest_street(p).unwrap();
        let road = w.road_height(sid, tt);
        assert!((w.car_height_at(car, p) - road).abs() < 0.05, "drawn at the road's height at {p:?}");
    }
}

