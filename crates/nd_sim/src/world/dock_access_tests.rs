//! [dock-access] Docks need only the water's edge and stations only their
//! track: each lays a street stub of its own that a road joins later, and
//! until then it does what it can without sending anyone or anything to a
//! place nobody can reach.

use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::mind::IdleMind;
use crate::ports::PortKind;
use crate::rail::{RailFacility, TRUNK_Y};

fn valley(seed: u64) -> World {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, seed), ..WorldConfig::default() });
    w.economy.treasury = 1_000_000.0;
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

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

/// A city to join: a street inland from `from` and homes along it.
fn town_from(w: &mut World, from: Vec2, dir: Vec2) {
    let end = from + dir * 260.0;
    w.add_road(&[from, end], 1, 1);
    let side = Vec2::new(-dir.y, dir.x);
    let a = from + dir * 60.0 + side * 10.0;
    let b = from + dir * 240.0 + side * 10.0;
    w.add_zone(vec![a, b, b + side * 70.0, a + side * 70.0], LandUse::Residential);
}

/// Can a lorry get from the city's roads at `from` to `b`?
fn lorry_reaches(w: &World, from: Vec2, b: BuildingId) -> bool {
    let Some((to_seg, to_pt)) = w.truck_stop(b) else { return false };
    let Some((seg, _, q)) = w.roads.nearest_street(from) else { return false };
    w.traffic.net.find_route(seg, q, to_seg, to_pt).is_some()
}

#[test]
fn a_ferry_terminal_on_an_empty_shore_lays_an_access_road_and_joins_when_a_road_reaches_it() {
    let mut w = valley(8);
    let (p, _, n, width) = river_spot(&w);
    assert!(w.roads.nearest_street(p).is_none_or(|(_, _, q)| q.distance(p) > 200.0), "no street anywhere near the shore");
    assert!(w.pick_job().is_none(), "an empty map has no jobs");
    let id = w.place_port(PortKind::FerryTerminal, p + n * (width * 0.5 + 18.0)).expect("a ferry terminal needs only the water");
    let port = w.ports.port_of(id).unwrap().clone();
    let stub = port.stub.expect("it laid an access road of its own");
    let len = stub[0].distance(stub[1]);
    assert!((crate::ports::STUB_MIN - 0.1..=crate::ports::STUB_LENGTH + 0.1).contains(&len), "a short access road: {len}");
    assert!((stub[1] - stub[0]).dot(-port.seaward) > 0.5 * len, "it runs inland from the gate");
    assert!(!w.landscape.is_water(stub[1]), "its free end is on dry land");
    assert!(w.roads.node_near(stub[1], 2.5).is_some(), "the stub is a road in the network");
    assert!(!port.road_connected && !port.works() && port.sails(), "off the roads: the ferries sail, nothing else works");
    assert_eq!(w.road_access(id), Some(false));
    assert!(w.access_problem(id).is_some_and(|s| s.contains("not connected to the road network")));
    let ends = w.stub_ends_of(id);
    assert_eq!(ends.len(), 1, "one marked end: {ends:?}");
    assert!(ends[0].distance(stub[1]) < 2.5);
    assert!(w.unjoined_stubs().iter().any(|(b, e)| *b == id && e.len() == 1));
    // Nothing is sent to it: nobody is hired there.
    for _ in 0..20 {
        assert_ne!(w.pick_job(), Some(id), "a dock off the roads hires nobody");
    }
    // Ferries sail meanwhile, empty.
    let mut sailed = false;
    for _ in 0..1500 {
        tick(&mut w, 1);
        if let Some(s) = w.ports.ships.values().find(|s| s.port == id && s.kind == crate::ports::ShipKind::Ferry) {
            assert_eq!(s.passengers, 0, "nobody can reach the pier");
            sailed = true;
            break;
        }
    }
    assert!(sailed, "the ferries run on the water alone");
    // A road drawn onto the stub's end joins it to a town.
    let dir = (stub[1] - stub[0]).normalize();
    town_from(&mut w, stub[1], dir);
    let port = w.ports.port_of(id).unwrap();
    assert!(port.road_connected && port.works(), "joined once the road meets the stub");
    assert_eq!(w.access_problem(id), None);
    assert!(w.stub_ends_of(id).is_empty() && w.unjoined_stubs().iter().all(|(b, _)| *b != id), "no marker once joined");
    let lot = &w.lots[port.lot.unwrap()];
    let town_node = w.roads.node_near(stub[1] + dir * 260.0, 3.0).expect("the town's far end");
    assert_eq!(w.road_component(lot.access_node), w.road_component(town_node), "the dock and the town share a network");
    assert!(lorry_reaches(&w, stub[1] + dir * 200.0, id), "lorries can reach it now");
    let home = w.lots.values().find(|l| l.land_use == LandUse::Residential && l.usable()).map(|l| l.entrance()).expect("a home lot");
    assert!(w.walk.route(home, w.building_entrance(id).unwrap()).is_some(), "people can walk to the pier");
    assert!((0..40).any(|_| w.pick_job() == Some(id)), "and it hires");
}

#[test]
fn a_station_on_the_track_away_from_roads_sees_trains_call_and_opens_when_a_road_reaches_it() {
    let mut w = valley(8);
    // Open ground beside the trunk line, well away from the river.
    let spot = [-200.0f32, 200.0, -700.0, 700.0, 0.0]
        .iter()
        .map(|&x| Vec2::new(x, TRUNK_Y + 30.0))
        .find(|&q| (0..8).all(|k| !w.landscape.is_water(q + Vec2::new(k as f32 * 15.0 - 60.0, 0.0))) && w.landscape.rivers.iter().all(|r| r.nearest(q).2 > 160.0))
        .expect("dry ground by the trunk");
    let id = w.place_rail_facility(RailFacility::Station, spot).expect("a station needs only its track");
    let link = w.buildings[id].rail.clone().unwrap();
    assert!(link.rail_connected && !link.road_connected, "on the railway, off the roads: {link:?}");
    assert!(!w.stations.contains(&id) && w.halts.contains(&id), "a halt: trains call, nobody gets on or off");
    assert!(w.access_problem(id).is_some_and(|s| s.contains("trains call")));
    let ends = w.stub_ends_of(id);
    assert_eq!(ends.len(), 2, "both ends of its street stub are marked: {ends:?}");
    for _ in 0..20 {
        assert_ne!(w.pick_job(), Some(id));
    }
    // A passenger train calls at it.
    let mut called = false;
    for _ in 0..900 {
        tick(&mut w, 1);
        if w.rail.trains.values().any(|t| t.stops.iter().any(|s| s.0 == id)) {
            called = true;
            break;
        }
    }
    assert!(called, "passenger trains call at a station on the track alone");
    assert!(w.rail.trains.values().all(|t| t.passengers.is_empty()), "with nobody aboard");
    // A road to one marked end, with a town on it.
    let f = w.facilities.values().find(|f| f.building == Some(id)).unwrap().clone();
    town_from(&mut w, ends[0], -f.inward());
    let link = w.buildings[id].rail.clone().unwrap();
    assert!(link.works(), "joined: {link:?}");
    assert!(w.stations.contains(&id) && !w.halts.contains(&id));
    assert_eq!(w.access_problem(id), None);
    assert!(lorry_reaches(&w, ends[0] - f.inward() * 200.0, id));
}

#[test]
fn a_ferry_terminal_on_a_civic_zone_takes_its_ground_out_and_looks_like_itself() {
    let mut w = valley(8);
    let (p, flow, n, width) = river_spot(&w);
    let road_at = p + n * (width * 0.5 + 72.0);
    w.add_road(&[road_at - flow * 250.0, road_at + flow * 250.0], 1, 1);
    // Civic zoning from the street right down to the water.
    let bank = p + n * (width * 0.5 + 1.0);
    w.add_zone(vec![bank - flow * 200.0, bank + flow * 200.0, road_at - n * 8.0 + flow * 200.0, road_at - n * 8.0 - flow * 200.0], LandUse::Civic);
    let click = p + n * (width * 0.5 + 18.0);
    let plan = w.plan_port(PortKind::FerryTerminal, click).expect("a terminal fits there");
    let fp = plan.footprint();
    // Something grown on the civic lots it will stand on.
    let under: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Civic && (point_in_polygon(l.center, &fp) || polygons_overlap(&fp, &l.polygon))).map(|(id, _)| id).collect();
    assert!(!under.is_empty(), "civic lots under the terminal");
    let grown = under.iter().find_map(|&l| {
        w.build_on(l);
        w.lots[l].building
    });
    let g = grown.expect("a civic building grew there");
    w.finish_construction(g);
    let id = w.place_port(PortKind::FerryTerminal, click).expect("placed over the civic zone");
    let b = &w.buildings[id];
    assert_eq!(b.port, Some(PortKind::FerryTerminal));
    assert!(b.demolition.is_none(), "drawn as a ferry terminal at once, never as the civic building it replaced");
    let lot = &w.lots[b.lot];
    assert_eq!(lot.polygon, w.ports.port_of(id).unwrap().footprint(), "its own footprint, not the zone's lot");
    assert!(!w.zones.contains_key(lot.zone), "its ground is in no zone");
    let fp = lot.polygon.clone();
    for (lid, l) in w.lots.iter() {
        if lid != b.lot {
            assert!(!point_in_polygon(l.center, &fp), "a zoned lot left under the terminal");
        }
    }
    assert!(w.buildings.get(g).is_none_or(|x| x.lot != b.lot && !point_in_polygon(w.lots[x.lot].center, &fp)), "what grew there is gone");
    // Bulldozed, the ground goes back to the civic plan.
    w.bulldoze_building(id).expect("a dock can be bulldozed");
    assert!(w.ports.port_of(id).is_none() && !w.buildings.contains_key(id));
    let back: Vec<_> = w.lots.values().filter(|l| polygons_overlap(&fp, &l.polygon) || point_in_polygon(l.center, &fp)).map(|l| (l.land_use, w.zones.contains_key(l.zone), l.building.is_some())).collect();
    assert!(back.iter().any(|&(lu, zoned, _)| lu == LandUse::Civic && zoned), "the civic lots come back: {back:?} (of {} lots)", w.lots.len());
}

#[test]
fn a_dock_from_an_older_save_keeps_its_street_along_the_front() {
    let mut w = valley(12);
    let (p, _, n, width) = river_spot(&w);
    let id = w.place_port(PortKind::FerryTerminal, p + n * (width * 0.5 + 18.0)).expect("terminal");
    let text = w.to_ron().unwrap();
    // Drop the new field as an old save would not have it.
    let start = text.find("stub: ").expect("stub saved");
    let rest = &text[start + 6..];
    let len = if rest.starts_with("None") {
        4
    } else {
        let mut depth = 0;
        let mut end = 0;
        for (i, ch) in rest.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        end
    };
    let stripped = format!("{}{}", &text[..start], rest[len..].trim_start_matches(','));
    let back = World::from_ron(&stripped).expect("an old save loads");
    let port = back.ports.port_of(id).expect("the dock is there");
    assert!(port.stub.is_none());
    assert_eq!(port.road_line(), port.front_line(), "an old dock's street is the one along its front");
    assert_eq!(back.road_access(id), Some(false));
}
