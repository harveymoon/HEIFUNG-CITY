//! [tools] Raised roads on pillars, and the Intersection editor's edits:
//! lane permissions, banned lefts, signal timing, push-button crossings
//! and footbridges.

use super::*;
use crate::junctions::{CrossingUpgrade, JunctionRule, LeftArrow, SignalMode};
use crate::mind::IdleMind;
use crate::pillars::PILLAR_RADIUS;
use crate::traffic::{LaneKind, LEFT, RIGHT, STRAIGHT};

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn raised(points: &[Vec2], levels: &[f32], lanes: (u8, u8)) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: lanes.0, lanes_bwd: lanes.1, curved: false, kind: RoadKind::Street, elevation: levels.to_vec() }
}

/// Plan a gesture in a project of its own and implement it.
fn try_build(w: &mut World, g: Gesture) -> Result<(), String> {
    w.project = None;
    w.add_gesture(g);
    w.try_implement()
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

/// Streets at y = 0 and x = 0, a residential block north-east of the
/// crossroads, every third lot built on (one-storey buildings).
fn block() -> World {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_zone(square(8.0, 8.0, 200.0, 200.0), LandUse::Residential);
    let mut lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    lots.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x).then(w.lots[*a].center.y.total_cmp(&w.lots[*b].center.y)));
    for (i, lot) in lots.into_iter().enumerate() {
        if i % 3 == 0 {
            w.build_on(lot);
        }
    }
    let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in sites {
        w.finish_construction(id);
    }
    for b in w.buildings.values_mut() {
        b.floors = 1;
    }
    w
}

/// The row of lots along the north side of the street at y = 0: the y of
/// a line through their middles.
fn row_y(w: &World) -> f32 {
    let mut ys: Vec<f32> = w.lots.values().filter(|l| l.usable() && l.center.y > 0.0 && l.center.y < 60.0 && l.center.x > 20.0).map(|l| l.center.y).collect();
    assert!(ys.len() >= 4, "a row of lots: {}", ys.len());
    ys.sort_by(f32::total_cmp);
    ys[ys.len() / 2]
}

/// Lots the line y = `y` crosses between `x0` and `x1`, west to east.
fn lots_on_line(w: &World, y: f32, x0: f32, x1: f32) -> Vec<LotId> {
    let mut lots: Vec<LotId> = w
        .lots
        .iter()
        .filter(|(_, l)| l.usable() && l.center.x > x0 && l.center.x < x1)
        .filter(|(_, l)| l.polygon.iter().map(|p| p.y).fold(f32::MAX, f32::min) < y - 1.0 && l.polygon.iter().map(|p| p.y).fold(f32::MIN, f32::max) > y + 1.0)
        .map(|(id, _)| id)
        .collect();
    lots.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
    lots
}

/// Make up to `n` of `lots` hold one of the city's own structures (a bus
/// depot): built on if empty, then flagged.
fn protect(w: &mut World, lots: &[LotId], n: usize) -> Vec<BuildingId> {
    let mut out = Vec::new();
    for &lot in lots.iter().take(n) {
        if w.lots[lot].building.is_none() {
            w.build_on(lot);
        }
        let Some(b) = w.lots[lot].building else { continue };
        if w.buildings[b].construction.is_some() {
            w.finish_construction(b);
        }
        let bb = &mut w.buildings[b];
        bb.depot = true;
        bb.floors = 1;
        out.push(b);
    }
    out
}

fn ground_street_clearance(w: &World, p: Vec2) -> f32 {
    w.roads
        .alive_segments()
        .filter(|(sid, _)| w.roads.segment_level(*sid, 0.5) < 2.0)
        .map(|(sid, s)| {
            let (a, b) = w.roads.segment_points(sid);
            crate::roads::project_on_segment(p, a, b).1.distance(p) - s.half_width() - crate::zoning::SIDEWALK
        })
        .fold(f32::INFINITY, f32::min)
}

#[test]
fn pillars_dodge_the_roads_and_buildings_under_a_raised_road() {
    let mut w = block();
    let y = row_y(&w);
    let row = lots_on_line(&w, y, 0.0, 220.0);
    assert!(row.len() >= 5, "lots under the line: {}", row.len());
    // A depot (one of the city's own structures) on one of them.
    let depot = protect(&mut w, &row[2..], 1).into_iter().next();
    let built: Vec<BuildingId> = w.buildings.keys().collect();
    assert!(row.iter().filter(|&&l| w.lots[l].building.is_some()).count() >= 2, "buildings under the road");
    // East-west over the row, over the street at x = 0, ramps at both ends.
    // (The ramps come down well clear of the block: a ramp at grade cuts
    // the parcels it passes like any street.)
    let pts = [Vec2::new(-330.0, y), Vec2::new(-230.0, y), Vec2::new(330.0, y), Vec2::new(430.0, y)];
    let levels = [0.0, 12.0, 12.0, 0.0];
    let preview = w.road_pillar_plan(&pts, &levels, 1, 1, RoadKind::Street);
    assert!(preview.ok(), "room for the columns: {:?}", preview.refusal());
    assert!(preview.buyouts.is_empty(), "empty lots and open ground were in reach: {:?}", preview.buyouts);
    // Columns at a plain 24 m spacing would have stood on a building or
    // the street: the plan moved them.
    let naive_hit = (0..=20).map(|k| -230.0 + k as f32 * 24.0).filter(|x| *x < 330.0).any(|x| {
        let p = Vec2::new(x, y);
        ground_street_clearance(&w, p) < PILLAR_RADIUS || w.lots.values().any(|l| l.building.is_some() && l.contains(p))
    });
    assert!(naive_hit, "the test needs obstacles in the way of a plain spacing");
    let before = w.buildings.len();
    try_build(&mut w, raised(&pts, &levels, (1, 1))).expect("implemented");
    assert!(w.pillars.len() >= 15, "columns along it: {}", w.pillars.len());
    for p in &w.pillars {
        assert!(ground_street_clearance(&w, p.at) > PILLAR_RADIUS - 0.01, "a column on the street at {}", p.at);
        for (_, l) in w.lots.iter().filter(|(_, l)| l.building.is_some()) {
            assert!(!l.contains(p.at), "a column on a building's lot at {}", p.at);
        }
    }
    // The low buildings under the deck stand, the depot too.
    assert_eq!(w.buildings.len(), before, "nothing was pulled down");
    for b in built {
        assert!(w.buildings.contains_key(b));
    }
    if let Some(d) = depot {
        let lot = w.buildings[d].lot;
        assert!(!w.pillars_on(&w.lots[lot].polygon), "no column on the depot's lot");
    }
    // Spans within the structural limit.
    let mut xs: Vec<f32> = w.pillars.iter().map(|p| p.at.x).collect();
    xs.sort_by(f32::total_cmp);
    for k in xs.windows(2) {
        assert!(k[1] - k[0] <= crate::pillars::PILLAR_MAX_SPAN + 0.01, "{xs:?}");
    }
    // A building too tall for the deck goes (bought out), and growth under
    // it is held to what fits.
    let lot = row.iter().copied().find(|&l| w.lots.get(l).is_some_and(|l| l.building.is_none())).expect("an empty lot under the deck");
    assert!(w.deck_cap(lot).is_some_and(|c| c <= crate::pillars::floors_under(12.0)), "{:?}", w.deck_cap(lot));
    assert!(w.height_cap(lot) <= crate::pillars::floors_under(12.0));
}

#[test]
fn a_raised_road_with_no_room_for_pillars_is_refused() {
    let mut w = block();
    // Straight along over the street at y = 0: every site is road. (Its
    // ends come down past the street's ends: an end dropped on the street
    // would land on it.)
    let pts = [Vec2::new(-420.0, 0.0), Vec2::new(-340.0, 0.0), Vec2::new(340.0, 0.0), Vec2::new(420.0, 0.0)];
    let levels = [0.0, 12.0, 12.0, 0.0];
    let g = raised(&pts, &levels, (1, 1));
    let plan = w.road_pillar_plan(&pts, &levels, 1, 1, RoadKind::Street);
    assert!(plan.gaps.iter().any(|g| !g.end), "a mid-span gap: {:?}", plan.gaps);
    assert!(!plan.ok(), "the preview shows the gap");
    let segs = w.roads.alive_segments().count();
    let err = try_build(&mut w, g).expect_err("refused");
    assert!(err.contains("pillar") || err.contains("column"), "{err}");
    assert!(w.project.is_some(), "the project stays pending");
    assert_eq!(w.roads.alive_segments().count(), segs, "nothing was built");
}

#[test]
fn pillars_never_take_a_protected_building() {
    let mut w = block();
    let y = row_y(&w);
    // Depots on every lot along 60 to 200 m east: no room there but on
    // them (and on the street at the far end).
    let row = lots_on_line(&w, y, 40.0, 215.0);
    let stations = protect(&mut w, &row, usize::MAX);
    assert!(stations.len() >= 4, "{}", stations.len());
    let pts = [Vec2::new(60.0, y), Vec2::new(200.0, y)];
    let plan = w.road_pillar_plan(&pts, &[12.0, 12.0], 1, 1, RoadKind::Street);
    assert!(plan.buyouts.iter().all(|(_, b)| !stations.contains(b)), "a depot would be bought out");
    assert!(!plan.ok(), "no room but on the depots");
    let err = try_build(&mut w, raised(&pts, &[12.0, 12.0], (1, 1))).expect_err("refused");
    assert!(err.contains("pillar") || err.contains("column"), "{err}");
    for s in stations {
        assert!(w.buildings.contains_key(s), "the depot stands");
    }
    // With the grown buildings instead, the same road is built: columns
    // buy out what is in the way.
    let mut w = block();
    let row = lots_on_line(&w, y, 40.0, 215.0);
    for &lot in &row {
        if w.lots[lot].building.is_none() {
            w.build_on(lot);
            if let Some(b) = w.lots[lot].building {
                w.finish_construction(b);
            }
        }
    }
    let plan = w.road_pillar_plan(&pts, &[12.0, 12.0], 1, 1, RoadKind::Street);
    assert!(plan.ok() && !plan.buyouts.is_empty(), "grown buildings are bought out: {:?}", plan.refusal());
    let taken: Vec<BuildingId> = plan.buyouts.iter().map(|x| x.1).collect();
    try_build(&mut w, raised(&pts, &[12.0, 12.0], (1, 1))).expect("built");
    for b in taken {
        assert!(!w.buildings.contains_key(b), "bought out for a column");
    }
}

#[test]
fn pillars_and_intersection_settings_survive_save_and_load() {
    let mut w = block();
    let y = row_y(&w);
    try_build(&mut w, raised(&[Vec2::new(-330.0, y), Vec2::new(-230.0, y), Vec2::new(330.0, y), Vec2::new(430.0, y)], &[0.0, 12.0, 12.0, 0.0], (1, 1))).unwrap();
    assert!(!w.pillars.is_empty());
    let k = w.junction_at(Vec2::ZERO, 2.0).expect("the crossroads");
    let mut rule = w.junction_view(k).rule;
    rule.signal = SignalMode::Signals;
    rule.left_arrow = LeftArrow::Always;
    rule.arrow_seconds = 9.0;
    rule.set_green(Vec2::new(1.0, 0.0), Some(33.0));
    w.set_junction_rule(rule.clone()).unwrap();
    w.economy.treasury = 1.0e6;
    let cr = w.junction_view(k).crossings[0].index;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let pillars = w.pillars.clone();
    let text = w.to_ron().unwrap();
    let w2 = World::from_ron(&text).unwrap();
    assert_eq!(w2.pillars, pillars);
    assert_eq!(w2.traffic.junction_rules, vec![rule]);
    assert_eq!(w2.crosswalks.rules.len(), 1);
    assert_eq!(w2.walk.footbridges.len(), 1, "the footbridge is back on the walk graph");
    let k2 = w2.junction_at(Vec2::ZERO, 2.0).unwrap();
    let inter = &w2.traffic.net.intersections[k2];
    assert!(inter.signalized && inter.arrow_mode == LeftArrow::Always && (inter.arrow_len - 9.0).abs() < 1e-3);
    assert!(inter.greens.iter().any(|g| (*g - 33.0).abs() < 1e-3), "{:?}", inter.greens);
}

/// A crossroads of two-lane-each-way streets.
fn crossroads() -> World {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 2, 2);
    w
}

fn west_arm(w: &World) -> (usize, SegmentId, NodeId) {
    let k = w.junction_at(Vec2::ZERO, 2.0).expect("a junction");
    let v = w.junction_view(k);
    let a = v.approaches.iter().find(|a| a.dir.x < -0.9).expect("the west arm");
    (k, a.segment, v.node.unwrap())
}

/// The route from the west arm to the `to` arm, and the turns its
/// connectors make (and from which lane index).
fn route_turns(w: &World, to: Vec2) -> Option<Vec<(i8, u8)>> {
    // (Right-hand traffic with the right of heading d at (-d.y, d.x):
    // eastbound lanes lie north of the centre line.)
    let (from_seg, _, _) = w.roads.nearest_where(Vec2::new(-150.0, 2.0), |s| s.is_street())?;
    let (to_seg, _, _) = w.roads.nearest_where(to, |s| s.is_street())?;
    let (route, _, _) = w.traffic.net.find_route(from_seg, Vec2::new(-150.0, 4.0), to_seg, to)?;
    let lanes = &w.traffic.net.lanes;
    Some(
        route
            .windows(2)
            .filter(|p| matches!(lanes[p[1] as usize].kind, LaneKind::Connector { .. }))
            .map(|p| (lanes[p[1] as usize].turn, match lanes[p[0] as usize].kind { LaneKind::Road { index, .. } => index, _ => 99 }))
            .collect(),
    )
}

#[test]
fn edited_lane_permissions_and_a_banned_left_change_routing() {
    let mut w = crossroads();
    let (k, west, _) = west_arm(&w);
    // Eastbound, a left turn goes to -y (southbound lanes at x > 0).
    let south = Vec2::new(4.0, -150.0);
    let east = Vec2::new(150.0, 4.0);
    let left = route_turns(&w, south).expect("a route south");
    assert!(left.first().is_some_and(|&(t, _)| t < 0), "eastbound to the south arm turns left: {left:?}");
    // Default two-lane arrows: L on the left lane, S+R on the right. Make
    // the left lane L+S and the right lane R only: straight on now goes
    // from the left lane.
    let straight = route_turns(&w, east).unwrap();
    assert!(straight.iter().any(|&(t, i)| t == 0 && i == 1), "straight from the right lane by default: {straight:?}");
    w.set_lane_permissions(k, west, Some(vec![LEFT | STRAIGHT, RIGHT])).unwrap();
    let straight = route_turns(&w, east).unwrap();
    assert!(straight.iter().all(|&(t, i)| t != 0 || i == 0), "straight only from the left lane now: {straight:?}");
    // Ban the left from the west arm: the route north no longer turns
    // left there (it finds another way or none).
    let mut rule = w.junction_view(k).rule;
    rule.set_left_forbidden(Vec2::new(-1.0, 0.0), true);
    w.set_junction_rule(rule).unwrap();
    let v = w.junction_view(k);
    let wa = v.approaches.iter().find(|a| a.segment == west).unwrap();
    assert!(wa.no_left && wa.exits & LEFT == 0 && wa.arrows.iter().all(|a| a & LEFT == 0), "{:?}", wa.arrows);
    assert!(wa.arrows.iter().any(|a| a & STRAIGHT != 0) && wa.arrows.iter().any(|a| a & RIGHT != 0));
    match route_turns(&w, south) {
        None => {}
        Some(t) => assert!(!t.first().is_some_and(|&(turn, _)| turn < 0), "turned left at the banned junction: {t:?}"),
    }
}

#[test]
fn signal_timing_edits_change_the_phases() {
    let mut w = crossroads();
    let (k, _, _) = west_arm(&w);
    assert!(w.traffic.net.intersections[k].signalized);
    let measure = |w: &mut World| -> f32 {
        // Seconds the east-west phase holds its green, on the fixed cycle.
        let inter = &w.traffic.net.intersections[k];
        let dirs = w.traffic.net.approach_dirs(&w.roads, k);
        let ew = inter.phases.iter().position(|g| g.iter().any(|&ap| dirs[ap].1.x.abs() > 0.9)).unwrap();
        // Wait for another phase, then time the next east-west green.
        for _ in 0..2000 {
            if w.traffic.net.intersections[k].phase != ew {
                break;
            }
            w.traffic.step(0.5, 1);
        }
        let mut green = 0.0;
        let mut seen = false;
        for _ in 0..2000 {
            w.traffic.step(0.5, 1);
            let inter = &w.traffic.net.intersections[k];
            if inter.phase == ew && inter.timer < inter.arrow + inter.green() {
                green += 0.5;
                seen = true;
            } else if seen {
                break;
            }
        }
        green
    };
    let before = measure(&mut w);
    let mut rule = w.junction_view(k).rule;
    rule.set_green(Vec2::new(1.0, 0.0), Some(40.0));
    rule.set_green(Vec2::new(-1.0, 0.0), Some(40.0));
    rule.left_arrow = LeftArrow::Never;
    w.set_junction_rule(rule.clone()).unwrap();
    let after = measure(&mut w);
    assert!(after > before + 15.0, "the green grew: {before} -> {after}");
    let v = w.junction_view(k);
    assert!(v.phases.iter().any(|p| (p.green - 40.0).abs() < 1e-3));
    // Unsignalised: no signal at all.
    rule.signal = SignalMode::Unsignalised;
    w.set_junction_rule(rule.clone()).unwrap();
    assert!(!w.traffic.net.intersections[k].signalized);
    assert!(w.traffic.net.lanes.iter().all(|l| l.green), "every light green (none) at an unsignalised junction");
    // Back to auto.
    w.set_junction_rule(JunctionRule::new(rule.at)).unwrap();
    assert!(w.traffic.net.intersections[k].signalized);
    assert!(w.traffic.junction_rules.is_empty());
}

/// A long street with a mid-block zebra and lots to walk between.
fn zebra_town() -> (World, u32) {
    let mut w = flat();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(-200.0, 8.0, 400.0, 60.0), LandUse::Residential);
    w.add_zone(square(-200.0, -68.0, 400.0, 60.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for lot in lots {
        w.build_on(lot);
    }
    let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in sites {
        w.finish_construction(id);
    }
    let mut guard = 0;
    while w.citizens.is_empty() && guard < 48 {
        run(&mut w, 60);
        guard += 1;
    }
    assert!(!w.citizens.is_empty(), "people moved in");
    // Everyone stays in: only the test's own walker moves.
    for c in w.citizens.values_mut() {
        if let Some(h) = c.home {
            c.activity = Activity::AtHome;
            c.inside = Some(h);
        }
        c.next_decision_tick = u64::MAX;
    }
    let cars: Vec<CarId> = w.traffic.cars.keys().collect();
    for c in cars {
        w.traffic.remove_car(c);
    }
    let cr = w.walk.crossings.iter().position(|c| c.kind == crate::walk::CrossingKind::Zebra).expect("a mid-block zebra") as u32;
    (w, cr)
}

/// A car driving east along y = 0 from x = -350.
fn east_car(w: &mut World) -> CarId {
    let (seg, _, _) = w.roads.nearest_where(Vec2::new(-350.0, 0.0), |s| s.is_street()).unwrap();
    let (goal, _, _) = w.roads.nearest_where(Vec2::new(350.0, 0.0), |s| s.is_street()).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(seg, Vec2::new(-350.0, 2.0), goal, Vec2::new(350.0, 2.0)).expect("a route east");
    let citizen = w.citizens.keys().next().unwrap_or_default();
    w.traffic.spawn_car(citizen, None, route, s0, s1, 0)
}

#[test]
fn a_push_button_crossing_stops_traffic_only_when_pressed() {
    let (mut w, cr) = zebra_town();
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Button).unwrap();
    let cr = w.walk.crossings.iter().position(|c| c.button).expect("the button crossing") as u32;
    // Nobody at it: traffic is never held.
    run(&mut w, 2);
    let car = east_car(&mut w);
    let mut slowest = f32::MAX;
    for _ in 0..40 {
        run(&mut w, 1);
        assert!(w.traffic.peds.holds.values().all(|h| h.iter().all(|x| x.crossing != cr)), "held with nobody there");
        if let Some(c) = w.traffic.cars.get(car) {
            if c.pos.x > -150.0 && c.pos.x < 150.0 {
                slowest = slowest.min(c.v);
            }
        }
    }
    assert!(slowest > 3.0, "the car never slowed for the empty crossing: {slowest}");
    // Someone presses: the light goes red and traffic is held.
    let c = &w.walk.crossings[cr as usize];
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    let id = w.citizens.keys().next().expect("someone lives here");
    {
        let ci = &mut w.citizens[id];
        ci.inside = None;
        ci.mode = TravelMode::Walk;
        ci.pos = a;
        ci.prev_pos = a;
        ci.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path: vec![a, a, b], idx: 1 };
        ci.next_decision_tick = u64::MAX;
    }
    let mut red = false;
    let mut held = false;
    for _ in 0..20 {
        run(&mut w, 1);
        red |= w.button_light(cr).red();
        held |= w.traffic.peds.holds.values().any(|h| h.iter().any(|x| x.crossing == cr));
    }
    assert!(w.peds.button_presses_total >= 1, "the button was pressed");
    assert!(red && held, "red {red}, traffic held {held}");
}

#[test]
fn walkers_on_a_footbridge_never_hold_traffic() {
    let (mut w, cr) = zebra_town();
    w.economy.treasury = 1.0e6;
    let mid = w.walk.crossings[cr as usize].mid;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    assert_eq!(w.walk.footbridges.len(), 1);
    assert!(w.walk.raised.len() >= 3, "stairs, deck, stairs");
    assert!(w.walk.nodes.iter().any(|n| n.level >= crate::junctions::FOOTBRIDGE_LEVEL - 0.01));
    let cr = w.walk.crossings.iter().position(|c| c.bridged).unwrap() as u32;
    let c = &w.walk.crossings[cr as usize];
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    // From one side of the street to the other near the bridge: the route
    // climbs over it rather than crossing at grade.
    let fb = w.walk.footbridges[0].clone();
    let along = (fb.foot[0] - fb.top[0]).normalize();
    let (path, nodes) = w.walk.route(a + along * 20.0, b + along * 20.0).expect("a route over");
    assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0), "over the bridge: {path:?}");
    assert!(path.windows(2).all(|p| w.walk.crossing_between(p[0], p[1]) != Some(cr)), "not across the zebra");
    let id = w.citizens.keys().next().unwrap();
    {
        let ci = &mut w.citizens[id];
        ci.inside = None;
        ci.mode = TravelMode::Walk;
        ci.pos = path[0];
        ci.prev_pos = path[0];
        ci.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path: path.clone(), idx: 0 };
        ci.next_decision_tick = u64::MAX;
    }
    let car = east_car(&mut w);
    let mut up = 0.0f32;
    for _ in 0..30 {
        run(&mut w, 1);
        assert!(w.traffic.peds.holds.values().all(|h| h.iter().all(|x| x.crossing != cr)), "a walker on the bridge held traffic");
        let ci = &w.citizens[id];
        up = up.max(w.walker_level(ci, ci.pos));
    }
    assert!(up > 3.0, "the walker climbed onto the bridge: {up}");
    let _ = (car, mid);
}

/// A save from before this branch (no pillars, junction rules, crosswalk
/// upgrades or button counts) still loads.
#[test]
fn a_save_from_before_the_tools_loads() {
    let w = crossroads();
    let text = w.to_ron().unwrap();
    // Drop each field (its value up to the comma at its own depth).
    fn drop_field(text: &str, key: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        let pat = format!("{key}:");
        while let Some(i) = rest.find(&pat) {
            let before_ok = i == 0 || !rest[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            if !before_ok {
                out.push_str(&rest[..i + pat.len()]);
                rest = &rest[i + pat.len()..];
                continue;
            }
            out.push_str(&rest[..i]);
            let mut depth = 0i32;
            let mut end = rest.len();
            for (j, c) in rest[i..].char_indices() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => {
                        end = i + j;
                        break;
                    }
                    ')' | ']' | '}' => depth -= 1,
                    ',' if depth == 0 => {
                        end = i + j + 1;
                        break;
                    }
                    _ => {}
                }
            }
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    }
    let mut old = text.clone();
    for key in ["pillars", "junction_rules", "crosswalks", "button_presses_total", "button_reds_total"] {
        old = drop_field(&old, key);
    }
    assert!(!old.contains("junction_rules") && !old.contains("crosswalks"), "the fields were dropped");
    let w2 = World::from_ron(&old).expect("an old save loads");
    assert!(w2.pillars.is_empty() && w2.traffic.junction_rules.is_empty() && w2.crosswalks.rules.is_empty());
    assert_eq!(w2.roads.alive_segments().count(), w.roads.alive_segments().count());
}

/// [clarity] Walkers take the footbridge over the crosswalk when it costs
/// only a little more, but not when it is a real detour: from the far side
/// of the crosswalk (away from the stairs) they cross at grade.
/// [footbridge] Only where the player kept the crosswalk under it.
#[test]
fn walkers_take_a_footbridge_only_when_it_is_no_big_detour() {
    let (mut w, cr) = zebra_town();
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    w.set_crosswalk_kept(cr, true).unwrap();
    assert!(!w.walk.crossings[cr as usize].closed && w.walk.crossings[cr as usize].marked(), "the crosswalk kept");
    assert!(w.closed_crosswalks().is_empty());
    let cr = w.walk.crossings.iter().position(|c| c.bridged).unwrap() as u32;
    let c = &w.walk.crossings[cr as usize];
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    let fb = w.walk.footbridges[0].clone();
    let along = (fb.foot[0] - fb.top[0]).normalize();
    let climbs = |w: &World, from: Vec2, to: Vec2| {
        let (_, nodes) = w.walk.route(from, to).expect("a way across");
        nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0)
    };
    // On the stairs' side: a few metres more, so over the bridge.
    assert!(climbs(&w, a + along * 15.0, b + along * 15.0), "takes the bridge when it is on the way");
    // From the other side, the bridge would be a long way round.
    assert!(!climbs(&w, a - along * 40.0, b - along * 40.0), "no big detour for a bridge");
}

/// The crossing's at-grade walk edges (either way round).
fn grade_edges(w: &World, cr: u32) -> usize {
    w.walk.adj.iter().flatten().filter(|e| e.cross == cr).count()
}

/// [footbridge] Harvey's flag: "I added a footbridge but the crosswalk is
/// still there and no one is using the bridge and traffic is still
/// stopping for people". A footbridge replaces the crosswalk under it: the
/// at-grade edge leaves the walk graph (so every route over the street
/// there climbs the bridge, even from the side away from the stairs), the
/// zebra is not painted and drivers have nothing to give way at.
#[test]
fn a_footbridge_replaces_the_zebra_under_it() {
    let (mut w, cr) = zebra_town();
    w.economy.treasury = 1.0e6;
    assert_eq!(grade_edges(&w, cr), 2, "a zebra to start with");
    let mid = w.walk.crossings[cr as usize].mid;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let c = w.walk.crossings[cr as usize].clone();
    assert!(c.bridged && c.closed && !c.marked(), "replaced: {c:?}");
    assert_eq!(grade_edges(&w, cr), 0, "no way across at grade");
    assert_eq!(w.closed_crosswalks(), vec![mid], "the painter leaves it out");
    let v = w.intersection_view(mid).unwrap();
    assert!(v.crossings.iter().any(|x| x.index == cr && x.closed && !x.kept && x.upgrade == CrossingUpgrade::Footbridge));
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    let fb = w.walk.footbridges[0].clone();
    let along = (fb.foot[0] - fb.top[0]).normalize();
    for (from, to) in [(a + along * 15.0, b + along * 15.0), (a - along * 40.0, b - along * 40.0), (a, b)] {
        let (path, nodes) = w.walk.route(from, to).expect("a way across");
        assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0), "over the bridge from {from}: {path:?}");
        assert!(path.windows(2).all(|p| w.walk.crossing_between(p[0], p[1]) != Some(cr)), "never across at grade");
    }
    // Kept again on request, and replaced again.
    w.set_crosswalk_kept(cr, true).unwrap();
    assert_eq!(grade_edges(&w, cr), 2);
    assert!(w.closed_crosswalks().is_empty());
    w.set_crosswalk_kept(cr, false).unwrap();
    assert_eq!(grade_edges(&w, cr), 0);
    // Taking the bridge away brings the zebra back.
    w.set_crossing_upgrade(cr, CrossingUpgrade::None).unwrap();
    assert_eq!(grade_edges(&w, cr), 2);
    assert!(w.walk.footbridges.is_empty() && w.closed_crosswalks().is_empty());
}

/// [footbridge] At a signalised crossroads of avenues (two lanes each way),
/// a footbridge over one arm takes that arm's crosswalk away with its walk
/// light and stop line; walkers from corner to corner climb over, and the
/// other arms keep theirs.
#[test]
fn a_footbridge_over_an_avenue_arm_replaces_the_junction_crosswalk() {
    let mut w = crossroads();
    let (k, west, node) = west_arm(&w);
    let mut rule = w.junction_view(k).rule;
    rule.signal = SignalMode::Signals;
    w.set_junction_rule(rule).unwrap();
    run(&mut w, 1);
    let v = w.junction_view(k);
    assert!(v.signalized);
    let west_cross = v.crossings.iter().find(|c| w.walk.crossings[c.index as usize].segment == west).expect("the west arm's crosswalk");
    let cr = west_cross.index;
    assert!(west_cross.signalised, "it had walk lights");
    let lanes: Vec<_> = w.peds.crossings[cr as usize].lanes.iter().map(|x| x.0).collect();
    assert!(lanes.iter().any(|l| w.traffic.peds.stop_lines.contains_key(l)), "and a stop line");
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    run(&mut w, 1);
    let c = w.walk.crossings[cr as usize].clone();
    assert!(c.closed && c.node == Some(node) && c.kind == crate::walk::CrossingKind::Junction);
    assert_eq!(grade_edges(&w, cr), 0);
    assert!(w.peds.crossings[cr as usize].signal.is_none(), "no walk light");
    assert!(lanes.iter().all(|l| !w.traffic.peds.stop_lines.contains_key(l)), "no stop line before it");
    let v = w.junction_view(k);
    assert!(v.crossings.iter().filter(|x| x.index != cr).all(|x| !x.closed && x.signalised), "the other arms keep theirs");
    let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
    let (path, nodes) = w.walk.route(a, b).expect("corner to corner");
    assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0), "over the bridge: {path:?}");
    assert!(path.windows(2).all(|p| w.walk.crossing_between(p[0], p[1]) != Some(cr)));
}

/// [pt-walk] F19: footbridges over every arm of a crossroads do not stand
/// in each other's way: where two meet at a corner they join with a
/// corner deck (one flight of stairs per corner), so the four make a ring
/// people walk all the way round up on the decks.
#[test]
fn footbridges_round_a_junction_join_into_a_ring() {
    let mut w = crossroads();
    let k = w.junction_at(Vec2::ZERO, 2.0).expect("a junction");
    w.economy.treasury = 1.0e7;
    let arms: Vec<u32> = w.junction_view(k).crossings.iter().map(|c| c.index).collect();
    assert_eq!(arms.len(), 4);
    for &cr in &arms {
        w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap_or_else(|e| panic!("arm {cr}: {e}"));
        run(&mut w, 1);
    }
    assert_eq!(w.walk.footbridges.len(), 4);
    assert_eq!(w.walk.footbridge_links.len(), 4, "a corner deck at each corner: {:?}", w.walk.footbridge_links);
    let stairs: usize = w.walk.footbridges.iter().map(|f| f.joined.iter().filter(|j| !**j).count()).sum();
    assert_eq!(stairs, 4, "one flight down at each corner");
    // Round the ring on the decks: from one bridge's top to the opposite
    // bridge's, never coming down.
    let tops: Vec<u32> = w.walk.footbridges.iter().map(|f| f.nodes[2]).collect();
    let far = *tops.iter().max_by(|a, b| w.walk.nodes[**a as usize].pos.distance(w.walk.nodes[tops[0] as usize].pos).total_cmp(&w.walk.nodes[**b as usize].pos.distance(w.walk.nodes[tops[0] as usize].pos))).unwrap();
    let nodes = w.walk.route_nodes(tops[0], far).expect("round the ring");
    assert!(nodes.iter().all(|&n| w.walk.nodes[n as usize].level > 0.0), "all the way up on the decks: {:?}", nodes.iter().map(|&n| w.walk.nodes[n as usize].level).collect::<Vec<_>>());
    // And every corner reaches the ground by its stairs.
    let (a, b) = (w.walk.nodes[w.walk.crossings[arms[0] as usize].a as usize].pos, w.walk.nodes[w.walk.crossings[arms[2] as usize].b as usize].pos);
    let (_, nodes) = w.walk.route(a, b).expect("corner to corner");
    assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0));
}

/// [footbridge] Cars never give way to someone up on the deck, though the
/// deck stands right over the lanes (the walker's 2D position is on the
/// street; only crossings at grade hold traffic).
#[test]
fn cars_do_not_yield_to_a_walker_on_the_deck() {
    let (mut w, cr) = zebra_town();
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let fb = w.walk.footbridges[0].clone();
    let (t0, t1) = (w.walk.nodes[fb.nodes[2] as usize].pos, w.walk.nodes[fb.nodes[3] as usize].pos);
    // To and fro on the deck the whole time.
    let mut path = Vec::new();
    for _ in 0..30 {
        path.push(t0);
        path.push(t1);
    }
    let id = w.citizens.keys().next().unwrap();
    {
        let ci = &mut w.citizens[id];
        ci.inside = None;
        ci.mode = TravelMode::Walk;
        ci.pos = t0;
        ci.prev_pos = t0;
        ci.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 1 };
        ci.next_decision_tick = u64::MAX;
    }
    let car = east_car(&mut w);
    let mid = w.walk.crossings[cr as usize].mid;
    let mut passed = false;
    let mut slowest_near = f32::MAX;
    for _ in 0..200 {
        run(&mut w, 1);
        assert!(w.traffic.peds.holds.is_empty() && w.traffic.peds.watch.is_empty(), "held for someone on the deck: {:?}", w.traffic.peds.holds);
        let ci = &w.citizens[id];
        assert!(w.walker_level(ci, ci.pos) > crate::junctions::FOOTBRIDGE_LEVEL - 0.01, "up on the deck");
        match w.traffic.cars.get(car) {
            Some(c) if c.pos.x < mid.x + 5.0 => {
                if c.pos.x > mid.x - 30.0 {
                    slowest_near = slowest_near.min(c.v);
                }
            }
            _ => {
                passed = true;
                break;
            }
        }
    }
    assert!(passed, "the car drove under the bridge");
    assert!(slowest_near > 3.0, "it did not slow for the walker overhead: {slowest_near}");
}

/// [footbridge] A footbridge from a save made before it replaced its
/// crosswalk (no `keep_crosswalk` on its rule) loads with the crosswalk
/// gone.
#[test]
fn an_old_saves_footbridge_loses_its_crosswalk_on_load() {
    let (mut w, cr) = zebra_town();
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
    let text = w.to_ron().unwrap();
    assert!(text.contains("keep_crosswalk"));
    let old: String = text.lines().filter(|l| !l.contains("keep_crosswalk")).collect::<Vec<_>>().join("\n");
    assert!(!old.contains("keep_crosswalk"), "the field was dropped");
    let w2 = World::from_ron(&old).expect("an old save loads");
    assert_eq!(w2.walk.footbridges.len(), 1);
    let c = &w2.walk.crossings[cr as usize];
    assert!(c.bridged && c.closed, "the crosswalk went on load");
    assert_eq!(grade_edges(&w2, cr), 0);
    // A kept crosswalk stays kept through a save.
    w.set_crosswalk_kept(cr, true).unwrap();
    let w3 = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(!w3.walk.crossings[cr as usize].closed && grade_edges(&w3, cr) == 2);
}

/// Harvey's gesture 423, exactly: a raised street (6 m) whose last point
/// is dropped on a highway carriageway. The network lands that end on the
/// carriageway at its level, so the deck ramps down to it and needs no
/// column at its end. The preview, the plan view's check and implementing
/// agree: it is built, with no gap. (It used to be planned at the drawn
/// 6 m to its end, which wanted a column over the carriageway and refused
/// with a 16 m "gap" ending at the junction.)
#[test]
fn a_raised_street_ramping_down_onto_a_highway_carriageway_is_built() {
    let mut w = flat();
    // The highway: both carriageways, a slip road joining the westbound,
    // and the carriageway piece the street lands on (as in his city).
    w.add_road(&[Vec2::new(-1000.0, 429.0), Vec2::new(1000.0, 429.0)], 3, 0);
    w.add_road(&[Vec2::new(1000.0, 411.0), Vec2::new(-1000.0, 411.0)], 3, 0);
    w.add_road(&[Vec2::new(-298.66, 381.0), Vec2::new(-448.66, 411.0)], 1, 0);
    w.add_road(&[Vec2::new(-250.0, 377.0), Vec2::new(-353.77, 369.26), Vec2::new(-480.0, 348.66), Vec2::new(-600.0, 329.0)], 3, 0);
    // The street the gesture starts from.
    w.add_road(&[Vec2::new(-400.34, 440.0), Vec2::new(-506.26, 470.83), Vec2::new(-600.0, 500.0)], 1, 1);
    let pts = [Vec2::new(-506.26, 470.83), Vec2::new(-367.15, 457.9), Vec2::new(-383.77, 386.03), Vec2::new(-391.44, 363.54)];
    let levels = [0.0, 6.0, 6.0, 6.0];
    // Laid as built: the last point lands on the carriageway at grade.
    let (_, built) = w.roads.as_built(&pts, &levels, RoadKind::Street);
    assert!(built[3].abs() < 0.5, "the end lands at the carriageway's level: {built:?}");
    let plan = w.road_pillar_plan(&pts, &levels, 1, 1, RoadKind::Street);
    assert!(plan.ok(), "the preview finds room: {:?}", plan.gaps);
    w.project = None;
    w.add_gesture(raised(&pts, &levels, (1, 1)));
    let check = w.active_project_check();
    assert!(check.pillar_gaps.is_empty() && check.pillar_refusal.is_none(), "the plan view is clear: {:?}", check.pillar_gaps);
    w.try_implement().expect("implemented");
    assert!(w.pillar_gaps.is_empty(), "{:?}", w.pillar_gaps);
    assert!(!w.pillars.is_empty(), "the raised part stands on columns");
}

/// An end left in the air with nothing to stand on near it is refused with
/// that reason, and the preview, the plan view and implementing agree.
#[test]
fn an_end_in_the_air_is_flagged_everywhere_with_the_end_reason() {
    let mut w = flat();
    // A wide carriageway pair; the street ends between them at 6 m, more
    // than a snap away from either, with no room for a column near its end.
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 4, 0);
    w.add_road(&[Vec2::new(300.0, 20.0), Vec2::new(-300.0, 20.0)], 4, 0);
    let pts = [Vec2::new(0.0, 220.0), Vec2::new(0.0, 120.0), Vec2::new(0.0, 40.0), Vec2::new(0.0, 10.0)];
    let levels = [0.0, 6.0, 6.0, 6.0];
    let (_, built) = w.roads.as_built(&pts, &levels, RoadKind::Street);
    assert!((built[3] - 6.0).abs() < 1e-3, "the end stays in the air: {built:?}");
    let plan = w.road_pillar_plan(&pts, &levels, 1, 1, RoadKind::Street);
    let g = plan.gaps.iter().find(|g| g.end).expect("the end is flagged in the preview");
    assert!(g.over_road && g.b.distance(Vec2::new(0.0, 10.0)) < 1.0, "{g:?}");
    let why = plan.refusal().unwrap();
    assert!(why.contains("ends in the air over a road") && why.contains("(0, 10)") && !why.contains("42 m"), "{why}");
    w.project = None;
    w.add_gesture(raised(&pts, &levels, (1, 1)));
    let shown = w.active_project_check().pillar_refusal.expect("a reason before implementing");
    assert!(shown.contains("ends in the air"), "{shown}");
    let segs = w.roads.alive_segments().count();
    assert_eq!(w.try_implement().expect_err("refused"), shown);
    assert_eq!(w.roads.alive_segments().count(), segs, "nothing was built");
}
