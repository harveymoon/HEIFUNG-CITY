//! [plant-groups] Plant sizing, Sunfields and Wind Parks, the small-lot
//! exception and old saves.

use super::plant_groups::*;
use super::*;
use crate::zoning::LandUse;

fn square(x0: f32, y0: f32, s: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)]
}

/// Industrial blocks either side of a straight street, flat, rich.
fn town() -> World {
    let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    w.add_zone(square(10.0, -200.0, 190.0), LandUse::Industrial);
    w.economy.treasury = 1e8;
    w
}

/// Three usable lots in a row north of the street, a-b and b-c touching,
/// a-c not.
fn chain(w: &World) -> (LotId, LotId, LotId) {
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.center.y > 0.0 && l.area() >= PowerPlantKind::Wind.min_area()).map(|(id, _)| id).collect();
    lots.iter()
        .find_map(|&a| lots.iter().filter(|&&b| b != a && w.plant_lots_touch(a, b)).find_map(|&b| lots.iter().find(|&&c| c != a && c != b && w.plant_lots_touch(b, c) && !w.plant_lots_touch(a, c)).map(|&c| (a, b, c))))
        .expect("three parcels in a row")
}

fn total_mw(w: &World, ids: &[BuildingId]) -> f32 {
    ids.iter().map(|&id| w.plant_capacity_mw(&w.buildings[id])).sum()
}

fn cleared() -> crate::buildings::Demolition {
    crate::buildings::Demolition { progress: 1.0, days: 1.0, loads: 0, floors: 1, tier: Tier::Low, land_use: LandUse::Industrial, seed: 1, condition: 1.0, clear_lot: true, own_lot: false, look: None, reason: Default::default() }
}

#[test]
fn cost_upkeep_and_output_follow_the_lot_and_are_capped() {
    let w = town();
    let mut lots: Vec<(LotId, f32)> = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= PowerPlantKind::Wind.min_area()).map(|(id, l)| (id, l.area())).collect();
    lots.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (small, big) = (lots[0], *lots.last().unwrap());
    assert!(big.1 > small.1 * 1.2, "lots differ: {} {}", small.1, big.1);
    for kind in [PowerPlantKind::Solar, PowerPlantKind::Wind] {
        let (ps, pb) = (w.plant_preview(kind, small.0).unwrap(), w.plant_preview(kind, big.0).unwrap());
        assert!(pb.capacity_mw > ps.capacity_mw * 1.2 && pb.cost > ps.cost * 1.2 && pb.upkeep_per_day > ps.upkeep_per_day * 1.2, "{kind:?}: {ps:?} {pb:?}");
        assert!((ps.cost / ps.capacity_mw - pb.cost / pb.capacity_mw).abs() < 1.0, "{kind:?}: the same price per MW");
        // What is charged is what the preview quoted.
        let mut w2 = w.clone();
        w2.economy.treasury = 1e6;
        let before = w2.economy.treasury;
        let id = w2.place_on_lot(crate::services::Placeable::Plant(kind), big.0).unwrap();
        assert!((before - w2.economy.treasury - pb.cost).abs() < 0.5, "{kind:?}: charged {} quoted {}", before - w2.economy.treasury, pb.cost);
        assert!((w2.plant_capacity_mw(&w2.buildings[id]) - pb.capacity_mw).abs() < 0.01);
        // One lot only goes so far.
        assert_eq!(kind.size_scale(1e7), crate::power::MAX_LOT_SCALE);
    }
    // [freight] Hydro is sized when built, within its own range.
    assert_eq!(PowerPlantKind::Hydro.size_scale(50.0), PowerPlantKind::Hydro.size_range().0);
}

#[test]
fn touching_solar_lots_run_as_one_sunfield_and_split_when_one_goes() {
    let mut w = town();
    let (a, b, c) = chain(&w);
    let pa = w.build_power(a, PowerPlantKind::Solar).unwrap();
    let alone = w.plant_capacity_mw(&w.buildings[pa]);
    assert!(w.plant_group_of(pa).is_none() && w.plant_groups().is_empty());
    // The preview next door names what it joins.
    let pv = w.plant_preview(PowerPlantKind::Solar, b).unwrap();
    let name = pv.join.as_ref().and_then(|j| j.joins.clone()).expect("joins the field next door");
    assert!(name.ends_with(" Sunfield"), "{name}");
    let pb = w.build_power(b, PowerPlantKind::Solar).unwrap();
    let pc = w.build_power(c, PowerPlantKind::Solar).unwrap();
    let g = w.plant_group_of(pa).expect("a group");
    assert_eq!(g.name, name, "named by its founder, kept as it grows");
    assert_eq!(g.members.len(), 3);
    for id in [pb, pc] {
        assert_eq!(w.plant_group_of(id).unwrap(), g, "every member sees the same group");
    }
    assert!((g.area - [a, b, c].iter().map(|&l| w.lots[l].area()).sum::<f32>()).abs() < 0.1);
    // The output is the members' combined, with the shared-connection bonus.
    let base: f32 = [pa, pb, pc].iter().map(|&id| PowerPlantKind::Solar.capacity_mw() * w.plant_scale(&w.buildings[id])).sum();
    assert!((total_mw(&w, &[pa, pb, pc]) - base * group_bonus(3)).abs() < 0.01);
    assert!(w.plant_capacity_mw(&w.buildings[pa]) > alone, "joining lifts a member a little");
    // The plant table: one row for the park, with its lots.
    let rows = w.plant_rows();
    assert_eq!(rows.len(), 1, "{:?}", rows.iter().map(|r| &r.name).collect::<Vec<_>>());
    let r = &rows[0];
    assert_eq!((r.name.as_str(), r.members.len(), r.id), (name.as_str(), 3, g.anchor()));
    assert!((r.capacity_mw - total_mw(&w, &[pa, pb, pc])).abs() < 0.01);
    assert!((r.upkeep_per_day - [pa, pb, pc].iter().map(|&id| w.plant_upkeep(&w.buildings[id])).sum::<f32>()).abs() < 0.01);
    // The grid counts the bonus too.
    w.allocate_power();
    let sum: f32 = [pa, pb, pc].iter().map(|&id| w.plant_output_mw(&w.buildings[id])).sum();
    assert!((w.economy.power.supply_by_kind[PowerPlantKind::Solar.index()] - sum).abs() < 0.01);
    // The middle lot goes: a and c no longer touch, so no group is left.
    w.buildings[pb].demolition = Some(cleared());
    w.finish_demolition(pb);
    assert!(!w.buildings.contains_key(pb) || w.buildings[pb].plant.is_none());
    assert!(w.plant_group_of(pa).is_none() && w.plant_group_of(pc).is_none());
    assert!(w.plant_groups().is_empty());
    assert_eq!(w.plant_rows().len(), 2);
    assert!((w.plant_capacity_mw(&w.buildings[pa]) - alone).abs() < 1e-3, "back to its own output");
}

#[test]
fn plants_across_a_road_or_of_another_kind_do_not_group() {
    let mut w = town();
    // The closest pair of lots either side of the street.
    let ok = |l: &crate::zoning::Lot| l.usable() && l.area() >= PowerPlantKind::Solar.min_area();
    let north: Vec<LotId> = w.lots.iter().filter(|(_, l)| ok(l) && l.center.y > 0.0).map(|(id, _)| id).collect();
    let south: Vec<LotId> = w.lots.iter().filter(|(_, l)| ok(l) && l.center.y < 0.0).map(|(id, _)| id).collect();
    let d = |p: &(LotId, LotId)| w.lots[p.0].center.distance(w.lots[p.1].center);
    let (n, s) = north.iter().flat_map(|&n| south.iter().map(move |&s| (n, s))).min_by(|a, b| d(a).total_cmp(&d(b))).unwrap();
    assert!(!w.plant_lots_touch(n, s), "a street is between them");
    let pn = w.build_power(n, PowerPlantKind::Solar).unwrap();
    w.build_power(s, PowerPlantKind::Solar).unwrap();
    assert!(w.plant_group_of(pn).is_none());
    // A narrow lot line (a hedge, a ditch) still joins; a wider gap or a
    // path through it does not.
    let a = square(0.0, 0.0, 30.0);
    let b = square(31.5, 0.0, 30.0);
    let far = square(36.0, 0.0, 30.0);
    let empty = RoadNetwork::default();
    assert!(lots_touch(&a, &b, &empty) && !lots_touch(&a, &far, &empty));
    assert!(!lots_touch(&a, &square(30.0, 30.0, 30.0), &empty), "corners only");
    let mut roads = RoadNetwork::default();
    roads.add_polyline_kind(&[Vec2::new(30.75, -50.0), Vec2::new(30.75, 80.0)], 1, 1, crate::roads::RoadKind::Path);
    assert!(!lots_touch(&a, &b, &roads), "a path runs between them");
    // A wind farm beside a solar field is its own plant.
    let (a, b, _) = chain(&w);
    let mut w2 = town();
    let sa = w2.build_power(a, PowerPlantKind::Solar).unwrap();
    w2.build_power(b, PowerPlantKind::Wind).unwrap();
    assert!(w2.plant_group_of(sa).is_none());
    assert_eq!(w2.plant_rows().len(), 2);
}

#[test]
fn a_lot_too_small_for_a_wind_farm_may_join_one_next_door() {
    let mut w = town();
    let (a, b, _) = chain(&w);
    // Shrink b away from a, keeping the side they share, until it is too
    // small for a wind farm on its own.
    let shared: Vec<Vec2> = w.lots[b].polygon.iter().copied().filter(|&p| crate::geom::distance_to_outline(p, &w.lots[a].polygon) < 0.1).collect();
    assert!(shared.len() >= 2, "b shares a side with a");
    let anchor = shared.iter().fold(Vec2::ZERO, |s, p| s + *p) / shared.len() as f32;
    let need = PowerPlantKind::Wind.min_area();
    for k in [0.6f32, 0.4, 0.3, 0.2, 0.12] {
        let poly: Vec<Vec2> = w.lots[b].polygon.iter().map(|&p| if shared.contains(&p) { p } else { anchor + (p - anchor) * k }).collect();
        let mut probe = w.lots[b].clone();
        probe.polygon = poly;
        if probe.area() < need {
            w.lots[b] = probe;
            break;
        }
    }
    assert!(w.lots[b].area() < need, "b shrunk below {need}: {}", w.lots[b].area());
    assert!(w.plant_lots_touch(a, b));
    let kind = PowerPlantKind::Wind;
    let what = crate::services::Placeable::Plant(kind);
    // Alone it is too small.
    assert!(w.placement_problem(what, b).is_some_and(|e| e.contains("too small")));
    assert!(w.build_power(b, kind).is_err());
    assert!(w.small_plant_lot_joins(kind, b).is_none());
    // A solar farm next door does not make room for it.
    let mut w3 = w.clone();
    w3.build_power(a, PowerPlantKind::Solar).unwrap();
    assert!(w3.placement_problem(what, b).is_some_and(|e| e.contains("too small")));
    // With a wind farm next door it joins it, and the preview says so.
    let pa = w.build_power(a, kind).unwrap();
    let pv = w.plant_preview(kind, b).unwrap();
    assert!(pv.small_lot_joins && pv.problem.is_none(), "{pv:?}");
    let park = pv.join.as_ref().and_then(|j| j.joins.clone()).unwrap();
    assert!(park.ends_with(" Wind Park"), "{park}");
    assert_eq!(w.small_plant_lot_joins(kind, b), Some(park.clone()));
    let pb = w.place_on_lot(what, b).expect("joins the park");
    let g = w.plant_group_of(pb).unwrap();
    assert_eq!((g.name.as_str(), g.members.len()), (park.as_str(), 2));
    assert!(g.members.contains(&pa));
    assert!(w.plant_capacity_mw(&w.buildings[pb]) > 0.0);
}

#[test]
fn a_save_keeps_its_parks_and_their_output() {
    let mut w = town();
    let (a, b, c) = chain(&w);
    let ids: Vec<BuildingId> = [a, b, c].iter().map(|&l| w.build_power(l, PowerPlantKind::Wind).unwrap()).collect();
    let before = total_mw(&w, &ids);
    let groups = w.plant_groups();
    assert_eq!(groups.len(), 1);
    let text = w.to_ron().unwrap();
    assert!(!text.contains("Wind Park"), "groups are derived, not saved");
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.plant_groups(), groups);
    assert!((total_mw(&back, &ids) - before).abs() < 1e-3);
    let rows = back.plant_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].members.len(), 3);
}
