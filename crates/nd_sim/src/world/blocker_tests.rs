//! [tools] The city's own structures in a project's way: the check the plan
//! view shows before Implement is the check Implement refuses on, it names
//! the road and the structure and says where, and a road far from a city
//! building never takes it (Harvey's flag: a plan shown clear refused on a
//! geothermal plant 100 m off, with nothing outlined).

use super::*;
use crate::power::PowerPlantKind;
use crate::services::Placeable;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// An avenue with industry zoned along its south side and nothing zoned to
/// the north, flat ground, plenty of money. Returns the avenue's gesture.
fn town() -> (World, GestureId) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 50_000_000.0;
    let avenue = w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    w.add_zone(square(-590.0, -80.0, 1180.0, 72.0), LandUse::Industrial);
    (w, avenue)
}

/// Plants side by side on the unzoned ground north of the avenue, west of
/// `x_max`, as the player places them (sites of joined parcels).
fn plants(w: &mut World, x_max: f32, n: usize) -> Vec<BuildingId> {
    let mut out = Vec::new();
    for k in 0..200 {
        let p = Vec2::new(-560.0 + k as f32 * 5.0, 12.0);
        if p.x > x_max || out.len() == n {
            break;
        }
        if w.placement_problem_at(Placeable::Plant(PowerPlantKind::Coal), p).is_ok() {
            out.push(w.place_building(Placeable::Plant(PowerPlantKind::Coal), p).expect("placed"));
        }
    }
    assert!(!out.is_empty(), "the unzoned ground takes a plant");
    out
}

fn street_road(points: Vec<Vec2>) -> Gesture {
    Gesture::Road { points, lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: crate::roads::RoadKind::Street, elevation: Vec::new(), car_only: None }
}

/// What Implement answers for the active project now, without building it.
fn implement_answer(w: &World) -> Option<String> {
    let mut t = w.clone();
    t.try_implement().err()
}

#[test]
fn a_road_far_from_city_buildings_never_takes_one() {
    let (mut w, _) = town();
    let ids = plants(&mut w, 100.0, 4);
    // A loop of new streets well east of the plants, joining the avenue
    // and reshaping the unzoned block north of it that the plants stand on.
    w.start_project();
    w.add_gesture(street_road(vec![Vec2::new(300.0, 0.0), Vec2::new(300.0, 160.0), Vec2::new(420.0, 160.0), Vec2::new(420.0, 0.0)]));
    let check = w.active_project_check();
    assert!(check.blocks.is_empty() && check.refusal.is_none(), "shown clear: {:?}", check.refusal);
    assert_eq!(implement_answer(&w), None, "and it implements");
    w.try_implement().expect("implemented");
    for id in ids {
        let b = w.buildings.get(id).expect("every plant still stands");
        assert_eq!(w.lots[b.lot].building, Some(id));
    }
}

#[test]
fn the_check_before_implement_is_what_implement_refuses_on() {
    let (mut w, avenue) = town();
    let ids = plants(&mut w, -300.0, 1);
    let plant = ids[0];
    let name = w.buildings[plant].name.clone();
    // Taking out the only street the plant fronts: no road runs over it,
    // but the rebuild leaves it no lot.
    w.start_project();
    w.remove_gesture(avenue);
    assert!(w.project_blockers().is_empty(), "no road runs over it");
    let check = w.active_project_check();
    let k = check.blocks.iter().find(|k| k.building == plant).expect("the plant shows as a blocker before implementing");
    assert!(!k.runs_over);
    assert_eq!(k.road, Some(avenue), "the road named is the one taken out");
    let shown = check.refusal.clone().expect("shown refused before implementing");
    assert!(shown.contains(&name), "{shown}");
    assert!(shown.contains("taking out "), "{shown}");
    // Implement says exactly what the plan view said, and builds nothing.
    let refused = w.try_implement().expect_err("refused");
    assert_eq!(refused, shown);
    assert!(w.buildings.contains_key(plant) && w.project.is_some());
}

#[test]
fn the_refusal_names_the_road_and_the_structure() {
    let (mut w, _) = town();
    let plant = plants(&mut w, -300.0, 1)[0];
    let name = w.buildings[plant].name.clone();
    let lot = w.lots[w.buildings[plant].lot].clone();
    let avenue_name = w.roads.nearest_where(Vec2::new(lot.center.x, 0.0), |s| !s.is_rail()).and_then(|(sid, _, _)| w.streets.of(sid)).map(|s| s.name.clone()).expect("the avenue has a name");
    // A side street north off the avenue, straight through the plant.
    w.start_project();
    let gid = w.add_gesture(street_road(vec![Vec2::new(lot.center.x, 0.0), Vec2::new(lot.center.x, 260.0)]));
    let check = w.active_project_check();
    assert_eq!(check.blockers, vec![plant]);
    let k = &check.blocks[0];
    assert!(k.runs_over);
    assert_eq!(k.road, Some(gid));
    assert!(k.road_label.contains(&avenue_name), "{}", k.road_label);
    // The red stretch is the road's, over the plant's ground.
    for p in [k.a, k.b] {
        assert!(lot.contains(p) || crate::geom::distance_to_outline(p, &lot.polygon) < 8.0, "{p} on the plant's lot");
        assert!((p.x - lot.center.x).abs() < 0.5, "{p} on the planned road");
    }
    let refused = w.try_implement().expect_err("refused");
    assert_eq!(Some(refused.clone()), check.refusal);
    assert!(refused.contains(&name) && refused.contains(&avenue_name), "{refused}");
}
