//! The world beyond the map: a new map has only the highway and the trunk
//! line, both running on out of sight; people get in only once the city is
//! joined to them; and nothing is spawned or removed where the camera sees.

use super::*;
use crate::landscape::{is_elsewhere, LandscapeParams, LandscapePreset, CAMERA_BOUND, MAP_EDGE, SIGHT, WAY_REACH};
use crate::mind::IdleMind;

/// How far `p` is from the nearest place the camera can be.
fn beyond_camera(p: Vec2) -> f32 {
    let dx = (p.x.abs() - CAMERA_BOUND).max(0.0);
    let dy = (p.y.abs() - CAMERA_BOUND).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

fn real_world(preset: LandscapePreset) -> World {
    World::new(WorldConfig { emergencies: false, landscape: LandscapeParams::preset(preset, 7), ..WorldConfig::default() })
}

#[test]
fn a_new_map_has_only_the_highway_and_the_trunk_line_from_elsewhere_to_elsewhere() {
    assert!(WAY_REACH - CAMERA_BOUND > SIGHT, "the ends of the ways are past sight from anywhere the camera goes");
    for preset in LandscapePreset::ALL {
        let w = real_world(preset);
        let mut highway = 0;
        let mut trunk = 0;
        for (id, s) in w.roads.alive_segments() {
            let (a, b) = w.roads.segment_points(id);
            match s.kind {
                RoadKind::Street => {
                    assert!(s.is_highway(), "{preset:?}: a street that is not the highway: {a:?}-{b:?}");
                    assert!(((a.y - HIGHWAY_Y_).abs() - crate::landscape::HIGHWAY_HALF_GAP).abs() < 0.5, "{preset:?}: highway off its line");
                    highway += 1;
                }
                RoadKind::Rail => {
                    assert!((a.y - TRUNK_Y).abs() < 0.5 && (b.y - TRUNK_Y).abs() < 0.5, "{preset:?}: track off the trunk line");
                    trunk += 1;
                }
                RoadKind::Path => panic!("{preset:?}: a path on a new map"),
            }
        }
        assert!(highway >= 2 && trunk >= 1, "{preset:?}: highway {highway} trunk {trunk}");
        // Every end is out of sight: no junction, no crossroads, no stub.
        for n in w.roads.leaf_nodes().into_iter().chain(w.roads.rail_leaf_nodes()) {
            let p = w.roads.node(n).pos;
            assert!(is_elsewhere(p), "{preset:?}: a way ends on the map at {p:?}");
            assert!(beyond_camera(p) > SIGHT, "{preset:?}: an end in sight at {p:?}");
        }
        let (entries, exits) = w.highway_ends();
        assert_eq!((entries.len(), exits.len()), (2, 2), "{preset:?}");
        assert!(w.lots.is_empty() && w.buildings.is_empty());
        assert!(!w.has_way_in(), "{preset:?}: nothing to move into and no way in yet");
    }
}
const HIGHWAY_Y_: f32 = crate::landscape::HIGHWAY_Y;

#[test]
fn a_road_drawn_to_the_map_edge_runs_on_out_of_sight_and_leads_elsewhere() {
    let mut w = real_world(LandscapePreset::Flat);
    w.add_road(&[Vec2::new(0.0, -100.0), Vec2::new(MAP_EDGE + 10.0, -100.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, 100.0), Vec2::new(900.0, 100.0)], 1, 1);
    let leaves: Vec<Vec2> = w.roads.leaf_nodes().into_iter().map(|n| w.roads.node(n).pos).collect();
    assert!(leaves.iter().any(|p| (p.x - WAY_REACH).abs() < 1.0 && (p.y + 100.0).abs() < 1.0), "carried on to {WAY_REACH}: {leaves:?}");
    assert!(leaves.iter().any(|p| p.distance(Vec2::new(900.0, 100.0)) < 1.0), "a road short of the edge just ends");
    let ends: Vec<Vec2> = w.city_leaves().into_iter().map(|n| w.roads.node(n).pos).collect();
    assert!(ends.iter().all(|p| is_elsewhere(*p)), "only far ends lead elsewhere: {ends:?}");
    assert!(ends.iter().any(|p| (p.y + 100.0).abs() < 1.0), "the road off the edge is a way in: {ends:?}");
}

#[test]
fn nobody_moves_in_until_the_town_is_joined_to_the_highway_and_traffic_comes_and_goes_out_of_sight() {
    let mut w = real_world(LandscapePreset::Flat);
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-300.0, 10.0), Vec2::new(300.0, 10.0), Vec2::new(300.0, 200.0), Vec2::new(-300.0, 200.0)], LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).take(6).collect();
    assert!(!lots.is_empty());
    for lot in lots {
        w.build_on(lot);
        if let Some(b) = w.lots[lot].building {
            w.finish_construction(b);
        }
    }
    assert!(w.buildings.values().any(|b| b.free_homes() > 0));
    assert!(!w.has_way_in());
    let mut minds = IdleMind;
    // Vehicles from elsewhere (through traffic, lorries, newcomers' cars)
    // are first seen out of sight; through traffic is last seen out of
    // sight too. Cars pulling out of a space in town are not arrivals.
    let check = |w: &World, seen: &mut HashMap<CarId, (Vec2, bool)>| -> usize {
        let mut newcomers = 0;
        for (id, c) in w.traffic.cars.iter() {
            if !seen.contains_key(&id) {
                let arriving = c.owned.is_none_or(|v| w.vehicles.cars.get(v).is_some_and(|o| o.since == w.clock.tick));
                if arriving {
                    assert!(beyond_camera(c.pos) > SIGHT, "a vehicle appeared in sight at {:?}", c.pos);
                    newcomers += c.owned.is_some() as usize;
                }
            }
            seen.insert(id, (c.pos, World::is_through_car(c)));
        }
        let gone: Vec<CarId> = seen.keys().copied().filter(|id| !w.traffic.cars.contains_key(*id)).collect();
        for id in gone {
            let (p, through) = seen.remove(&id).unwrap();
            if through {
                assert!(beyond_camera(p) > SIGHT - 150.0, "through traffic taken off in sight at {p:?}");
            }
        }
        newcomers
    };
    let mut seen: HashMap<CarId, (Vec2, bool)> = HashMap::new();
    for _ in 0..600 {
        w.tick(&mut minds);
        check(&w, &mut seen);
    }
    assert!(w.citizens.is_empty(), "nobody can get in with no road from elsewhere");
    assert!(w.through_cars() > 0, "the highway carries its own traffic all the while");
    let before = w.through_cars();
    assert!(w.link_to_highway(Vec2::new(400.0, 0.0)));
    assert!(w.has_way_in());
    // The rebuild took every car off and put the through traffic back where
    // it was (under new ids): none of it vanished.
    assert_eq!(w.through_cars(), before, "through traffic carries on across a rebuild");
    seen = w.traffic.cars.iter().map(|(id, c)| (id, (c.pos, World::is_through_car(c)))).collect();
    let mut newcomer_cars = 0;
    for _ in 0..900 {
        w.tick(&mut minds);
        newcomer_cars += check(&w, &mut seen);
        if w.citizens.len() >= 4 && w.traffic.cars.values().any(|c| c.owned.is_some()) {
            break;
        }
    }
    assert!(!w.citizens.is_empty(), "people move in by the highway once the town is joined to it");
    assert!(newcomer_cars > 0, "newcomers drove in along the highway from out of sight");
}
