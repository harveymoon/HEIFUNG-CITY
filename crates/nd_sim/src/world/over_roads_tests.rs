//! [w1-roads] Nothing is built over a road (flags H1, H3).

use super::over_roads::Footprint;
use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::ports::PortKind;
use crate::power::PowerPlantKind;
use crate::buildings::Tier;
use crate::services::{Placeable, ServiceKind};

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn valley(seed: u64) -> World {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, seed), ..WorldConfig::default() });
    w.economy.treasury = 5_000_000.0;
    w
}

/// A point on the main river nearest the middle of the map, its flow, the
/// unit vector from it towards the middle, and the river's width.
fn river_spot(w: &World) -> (Vec2, Vec2, Vec2, f32) {
    let r = &w.landscape.rivers[0];
    let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
    let (seg, _, _) = r.nearest(p);
    let flow = r.flow(seg);
    let mut n = Vec2::new(-flow.y, flow.x);
    if n.dot(-p) < 0.0 {
        n = -n;
    }
    (p, flow, n, r.width)
}

fn network_size(w: &World) -> (usize, usize) {
    (w.roads.nodes.len(), w.roads.alive_segments().count())
}

#[test]
fn a_marina_over_a_road_is_refused_and_lays_no_junction() {
    let mut w = valley(8);
    let (p, _, n, width) = river_spot(&w);
    let click = p + n * (width * 0.5 + 18.0);
    let port = w.plan_port(PortKind::MarinaLarge, click).expect("open shore takes a yacht club");
    // A street straight through the middle of its yard, along the shore.
    let along = Vec2::new(port.seaward.y, -port.seaward.x);
    w.add_road(&[port.center - along * 200.0, port.center + along * 200.0], 1, 1);
    let before = network_size(&w);
    let money = w.economy.treasury;
    let why = w.plan_port(PortKind::MarinaLarge, click).expect_err("the preview refuses it");
    assert!(why.contains("road runs across the site"), "{why}");
    let why = w.place_port(PortKind::MarinaLarge, click).expect_err("and so does the click");
    assert!(why.contains("nothing may be built over a road"), "{why}");
    assert_eq!(network_size(&w), before, "no stub, no junction");
    assert_eq!(w.economy.treasury, money, "nothing paid");
    assert!(w.ports.ports.is_empty());
    // Through the Build tool's door too.
    assert!(w.place_building(Placeable::Dock(PortKind::MarinaSmall), click).is_err());
}

#[test]
fn a_dock_beside_a_road_still_goes_down_and_its_stub_makes_no_crossroads() {
    let mut w = valley(8);
    let (p, _, n, width) = river_spot(&w);
    let click = p + n * (width * 0.5 + 18.0);
    let port = w.plan_port(PortKind::FerryTerminal, click).expect("open shore");
    // A street well inland, across the line of its access road.
    let along = Vec2::new(port.seaward.y, -port.seaward.x);
    let gate = port.front_mid() - port.seaward * crate::ports::STUB_GATE;
    let mid = gate - port.seaward * 20.0;
    w.add_road(&[mid - along * 150.0, mid + along * 150.0], 1, 1);
    let id = w.place_port(PortKind::FerryTerminal, click).expect("a road beside it is no bar");
    let stub = w.ports.port_of(id).unwrap().stub.expect("its own access road");
    // The stub stops on the street (a T), not through it.
    let d = crate::geom::project_on_segment(stub[1], mid - along * 150.0, mid + along * 150.0).1.distance(stub[1]);
    assert!(d < 1.5, "the stub ends on the street: {d:.1} m off; {stub:?}");
    for (sid, s) in w.roads.alive_segments() {
        if !s.is_street() {
            continue;
        }
        let n = w.roads.node(s.a);
        let degree = n.segments.iter().filter(|&&x| w.roads.segment(x).alive && w.roads.segment(x).is_street()).count();
        assert!(degree <= 3, "no four-way junction made by the stub at {:?} (segment {sid:?})", n.pos);
    }
}

#[test]
fn a_dam_across_a_bridge_is_refused() {
    let mut w = valley(8);
    let (p, flow, n, width) = river_spot(&w);
    // A plant's lot on the bank.
    let side = n * (width * 0.5 + 8.0);
    let lot = vec![p + side - flow * 30.0, p + side + flow * 30.0, p + side + n * 60.0 + flow * 30.0, p + side + n * 60.0 - flow * 30.0];
    let (line, half) = w.hydro_dam_line(&lot).expect("a river beside the lot");
    assert!(w.road_under(Footprint::Wall(&line, half), "dam", true).is_none(), "nothing in the way yet");
    // A bridge straight along where the dam's arch would go.
    let mid = line[line.len() / 2];
    let across = (line[line.len() - 1] - line[0]).normalize_or_zero();
    w.add_road(&[mid - across * (width + 60.0), mid + across * (width + 60.0)], 1, 1);
    let why = w.road_under(Footprint::Wall(&line, half), "dam", true).expect("the dam would stand on the bridge");
    assert!(why.contains("nothing may be built over a road"), "{why}");
    // A bridge well downstream is no bar.
    let mut w2 = valley(8);
    w2.add_road(&[mid - across * (width + 60.0) + flow * 120.0, mid + across * (width + 60.0) + flow * 120.0], 1, 1);
    assert!(w2.road_under(Footprint::Wall(&line, half), "dam", true).is_none());
}

/// An avenue with nothing zoned to the north, flat ground.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    w.add_zone(square(-590.0, -60.0, 1180.0, 52.0), LandUse::Residential);
    w
}

#[test]
fn a_road_planned_across_the_ground_refuses_a_building_there() {
    let mut w = town();
    let p = Vec2::new(-300.0, 14.0);
    assert!(w.placement_problem_at(Placeable::Plant(PowerPlantKind::Solar), p).is_ok(), "free unzoned ground");
    // A curved road planned (not built) through the parcels there.
    w.new_project(Some("bypass"));
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(-360.0, 70.0), Vec2::new(-300.0, 18.0), Vec2::new(-240.0, 70.0)], lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new() });
    let why = w.placement_problem_at(Placeable::Plant(PowerPlantKind::Solar), p).expect_err("a planned road runs there");
    assert!(why.contains("planned in this project"), "{why}");
    assert!(w.place_building(Placeable::Service(ServiceKind::Police, Tier::Low), p).is_err());
}

#[test]
fn a_footprint_by_the_kerb_is_not_over_the_road() {
    let w = town();
    // A lot's edge where parcels start: the corridor's edge.
    let (sid, _, _) = w.roads.nearest_street(Vec2::new(-70.0, 0.0)).unwrap();
    let hw = w.roads.segment(sid).corridor_half_width();
    let fp = square(-100.0, hw, 60.0, 40.0);
    assert!(w.road_under(Footprint::Area(&fp), "plant", true).is_none());
    // Over the carriageway: refused.
    let fp = square(-100.0, -2.0, 60.0, 40.0);
    assert!(w.road_under(Footprint::Area(&fp), "plant", true).is_some());
    // Every lot the town made passes.
    for l in w.lots.values() {
        assert!(w.road_under(Footprint::Area(&l.polygon), "x", true).is_none(), "{:?}", l.polygon);
    }
}
