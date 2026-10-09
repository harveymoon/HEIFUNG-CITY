//! [plant-scale] Sites on unzoned ground sized to what the building needs,
//! thermal plants sized to their site within a cap, the preview the Build
//! hover shows equal to what the click charges and the plant makes, and
//! older saves' plants unchanged.

use super::*;
use crate::power::PowerPlantKind;
use crate::services::Placeable;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// An avenue with industry zoned along its south side and nothing zoned to
/// the north, flat ground, plenty of money.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    w.add_zone(square(-590.0, -80.0, 1180.0, 72.0), LandUse::Industrial);
    w
}

/// Points on free unzoned parcels north of the avenue, west to east.
fn unzoned_points(w: &World) -> Vec<Vec2> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for k in 0..110 {
        let p = Vec2::new(-560.0 + k as f32 * 10.0, 12.0);
        if let Some(i) = w.parcels.at(p) {
            let pc = &w.parcels.parcels[i];
            if pc.lot.is_none() && pc.land_use.is_none() && !seen.contains(&i) {
                seen.push(i);
                out.push(p);
            }
        }
    }
    assert!(out.len() > 10, "unzoned parcels north of the avenue: {}", out.len());
    out
}

fn area(poly: &[Vec2]) -> f32 {
    crate::geom::polygon_area(poly).abs()
}

#[test]
fn sites_on_unzoned_ground_join_only_what_the_building_needs() {
    let w = town();
    for kind in [PowerPlantKind::Coal, PowerPlantKind::Nuclear, PowerPlantKind::Solar] {
        let what = Placeable::Plant(kind);
        let mut joined = 0;
        for p in unzoned_points(&w) {
            let Ok(site) = w.placement_site(what, p) else { continue };
            assert!(site.lot.is_none());
            let a = area(&site.outline);
            if site.parcels.len() == 1 {
                continue;
            }
            joined += 1;
            assert!(a <= kind.max_area() + 1.0, "{kind:?}: a joined site of {a:.0} m² is past the {:.0} cap", kind.max_area());
            // Only enough: without its last parcel it would be short (no
            // parcel in it is as big as the need on its own).
            let biggest = site.parcels.iter().map(|&i| w.parcels.parcels[i].area).fold(0.0, f32::max);
            let fronted = site.parcels.iter().all(|&i| w.parcels.parcels[i].path_frontage);
            assert!(!fronted || a < kind.min_area() + biggest * 1.05 + 1.0, "{kind:?}: {a:.0} m² from {} parcels for a {:.0} m² need", site.parcels.len(), kind.min_area());
        }
        if kind != PowerPlantKind::Solar {
            assert!(joined > 0, "{kind:?}: some sites join parcels");
        }
    }
}

#[test]
fn thermal_plants_are_sized_to_their_site_within_a_cap() {
    for kind in [PowerPlantKind::Coal, PowerPlantKind::Nuclear, PowerPlantKind::Geothermal, PowerPlantKind::Hydro, PowerPlantKind::Tidal] {
        let (lo, hi) = kind.size_range();
        assert!(lo < 1.0 && hi > 1.5, "{kind:?}: {lo} {hi}");
        assert_eq!(kind.size_scale(kind.min_area()), lo);
        assert_eq!(kind.size_scale(1e7), hi, "{kind:?}: capped");
        assert!(kind.size_scale(kind.reference_area().unwrap()) == 1.0);
        assert!(kind.jobs_at(hi) > kind.jobs() && kind.jobs_at(lo) < kind.jobs());
    }
    // [freight] Hydro and tidal grow by whole turbine bays: three at the
    // nameplate, from one on the smallest site to eight on the largest.
    for kind in [PowerPlantKind::Hydro, PowerPlantKind::Tidal] {
        let (lo, hi) = kind.size_range();
        assert_eq!(PowerPlantKind::turbine_bays(1.0), 3);
        assert_eq!(PowerPlantKind::turbine_bays(lo), 1, "{kind:?}");
        assert_eq!(PowerPlantKind::turbine_bays(hi), 8, "{kind:?}");
    }
    // A bigger industrial lot, a bigger coal plant, at the same price per MW.
    let w = town();
    let mut lots: Vec<(LotId, f32)> = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= PowerPlantKind::Coal.min_area()).map(|(id, l)| (id, l.area())).collect();
    lots.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (small, big) = (lots[0], *lots.last().unwrap());
    let (ps, pb) = (w.plant_preview(PowerPlantKind::Coal, small.0).unwrap(), w.plant_preview(PowerPlantKind::Coal, big.0).unwrap());
    assert!(big.1 > small.1 * 1.1, "lots differ: {} {}", small.1, big.1);
    assert!(pb.capacity_mw > ps.capacity_mw && pb.cost > ps.cost && pb.upkeep_per_day > ps.upkeep_per_day, "{ps:?} {pb:?}");
    assert!((ps.cost / ps.capacity_mw - pb.cost / pb.capacity_mw).abs() < 1.0);
}

/// Place `kind` at `p` and check the charge, the nameplate and the upkeep
/// are what the preview said.
fn place_as_previewed(w: &mut World, kind: PowerPlantKind, p: Vec2) -> (BuildingId, crate::world::PlantPreview) {
    let site = w.placement_site(Placeable::Plant(kind), p).expect("a site");
    let pv = w.plant_preview_site(kind, &site).expect("a preview");
    assert!(pv.problem.is_none(), "{kind:?} at {p}: {:?}", pv.problem);
    let before = w.economy.treasury;
    let id = w.place_building(Placeable::Plant(kind), p).expect("placed");
    let charged = before - w.economy.treasury;
    assert!((charged - pv.cost).abs() < 0.5, "{kind:?}: charged {charged} quoted {}", pv.cost);
    let b = &w.buildings[id];
    assert!((w.plant_capacity_mw(b) - pv.capacity_mw).abs() < 0.01, "{kind:?}: makes {} quoted {}", w.plant_capacity_mw(b), pv.capacity_mw);
    assert!((w.plant_upkeep(b) - pv.upkeep_per_day).abs() < 0.01, "{kind:?}: upkeep {} quoted {}", w.plant_upkeep(b), pv.upkeep_per_day);
    assert!((w.lots[b.lot].area() - pv.area).abs() < 0.5, "the lot made is the site previewed");
    (id, pv)
}

#[test]
fn the_preview_is_what_placing_a_coal_plant_charges_and_makes() {
    // On an existing lot.
    let mut w = town();
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= PowerPlantKind::Coal.min_area()).map(|(id, _)| id).next().unwrap();
    let c = w.lots[lot].center;
    let (id, pv) = place_as_previewed(&mut w, PowerPlantKind::Coal, c);
    assert_eq!(w.buildings[id].lot, lot);
    assert_eq!(w.buildings[id].plant_scale, Some(pv.scale), "the size is kept on the plant");
    assert_eq!(w.buildings[id].jobs, PowerPlantKind::Coal.jobs_at(pv.scale));
    // On unzoned ground (parcels joined into a site).
    let mut w = town();
    let p = unzoned_points(&w).into_iter().find(|&p| w.placement_problem_at(Placeable::Plant(PowerPlantKind::Coal), p).is_ok()).expect("unzoned ground takes a coal plant");
    let (id, pv) = place_as_previewed(&mut w, PowerPlantKind::Coal, p);
    assert!(pv.area <= PowerPlantKind::Coal.max_area() + 1.0 || w.parcels.at(p).is_some_and(|i| w.parcels.parcels[i].area >= PowerPlantKind::Coal.min_area()));
    assert!(w.plant_capacity_mw(&w.buildings[id]) >= PowerPlantKind::Coal.capacity_at(PowerPlantKind::Coal.size_range().0) - 0.01);
}

#[test]
fn the_preview_is_what_placing_solar_charges_and_makes_alone_and_joining() {
    let kind = PowerPlantKind::Solar;
    // Alone, on a lot.
    let mut w = town();
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= kind.min_area()).map(|(id, _)| id).next().unwrap();
    let c = w.lots[lot].center;
    let (_, pv) = place_as_previewed(&mut w, kind, c);
    assert!(pv.join.is_none());

    // On unzoned ground: one farm, then the parcel next door joins it.
    let mut w = town();
    let pts = unzoned_points(&w);
    let first = pts.iter().copied().find(|&p| w.placement_problem_at(Placeable::Plant(kind), p).is_ok()).unwrap();
    let (a, pa) = place_as_previewed(&mut w, kind, first);
    assert!(pa.join.is_none());
    let next = unzoned_points(&w).into_iter().find(|&p| {
        w.placement_site(Placeable::Plant(kind), p).ok().and_then(|s| w.plant_preview_site(kind, &s)).is_some_and(|v| v.problem.is_none() && v.join.is_some())
    });
    let next = next.expect("an unzoned parcel touching the farm");
    let alone = w.plant_capacity_mw(&w.buildings[a]);
    let (b, pb) = place_as_previewed(&mut w, kind, next);
    let j = pb.join.clone().unwrap();
    let group = w.plant_group_of(b).expect("they run as one");
    assert_eq!(Some(group.name.clone()), j.joins, "the Sunfield it said it would join");
    let together: f32 = group.members.iter().map(|&id| w.plant_capacity_mw(&w.buildings[id])).sum();
    assert!((together - j.capacity_mw).abs() < 0.01, "together {together} quoted {}", j.capacity_mw);
    assert!((j.before_mw - alone).abs() < 0.01, "before {} was {alone}", j.before_mw);
    assert!(j.capacity_mw - j.before_mw > pb.capacity_mw * 0.99, "adds at least its own nameplate (the bonus lifts the rest)");
}

#[test]
fn older_saves_plants_keep_their_output() {
    let mut w = town();
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= PowerPlantKind::Coal.min_area()).map(|(id, _)| id).collect();
    let coal = w.build_power(lots[0], PowerPlantKind::Coal).unwrap();
    let nuke_lot = lots.iter().copied().find(|&l| w.lots[l].area() >= PowerPlantKind::Nuclear.min_area()).unwrap_or(lots[1]);
    let nuke = w.build_power(nuke_lot, PowerPlantKind::Nuclear).ok();
    let new_mw = w.plant_capacity_mw(&w.buildings[coal]);
    // Saved before plants had a size: no `plant_scale` in the file.
    let mut old = w.clone();
    old.buildings[coal].plant_scale = None;
    if let Some(n) = nuke {
        old.buildings[n].plant_scale = None;
    }
    let text = old.to_ron().unwrap();
    let stripped = text.replace("plant_scale:None,", "").replace("plant_scale: None,", "");
    assert!(!stripped.contains("plant_scale:None") && !stripped.contains("plant_scale: None"), "the field is gone from the old file");
    let loaded = World::from_ron(&stripped).unwrap();
    let b = &loaded.buildings[coal];
    assert_eq!(b.plant_scale, None);
    assert_eq!(loaded.plant_capacity_mw(b), PowerPlantKind::Coal.capacity_mw(), "an old coal plant still makes 120 MW");
    assert_eq!(loaded.plant_upkeep(b), PowerPlantKind::Coal.upkeep_per_day());
    if let Some(n) = nuke {
        assert_eq!(loaded.plant_capacity_mw(&loaded.buildings[n]), PowerPlantKind::Nuclear.capacity_mw());
    }
    // A new plant keeps the size it was built at through a save.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.plant_capacity_mw(&back.buildings[coal]), new_mw);
}

#[test]
fn a_coal_plant_is_not_put_on_a_strip_along_the_kerb() {
    let strip = square(0.0, 0.0, 54.0, 12.0);
    assert!(crate::zoning::site_thickness(&strip) < PowerPlantKind::Coal.min_thickness());
    assert!(crate::zoning::site_thickness(&square(0.0, 0.0, 24.0, 25.0)) >= PowerPlantKind::Coal.min_thickness(), "a 600 m² square will do");
    let w = town();
    for p in unzoned_points(&w) {
        if let Ok(site) = w.placement_problem_at(Placeable::Plant(PowerPlantKind::Coal), p) {
            assert!(crate::zoning::site_thickness(&site.outline) >= PowerPlantKind::Coal.min_thickness(), "a coal site at {p} is a strip");
        }
    }
}

/// [freight] A river along the back of the industrial lots, fast enough
/// for hydro.
fn river_town() -> World {
    let mut w = town();
    let back = w.lots.values().filter(|l| l.usable()).flat_map(|l| l.polygon.iter().map(|p| p.y)).fold(0.0f32, f32::min);
    w.landscape.rivers.push(crate::landscape::River { points: vec![Vec2::new(-700.0, back - 15.0), Vec2::new(700.0, back - 15.0)], width: 20.0, current: 2.0, stream: false });
    w
}

#[test]
fn hydro_plants_are_sized_to_their_site_and_old_ones_keep_their_output() {
    let kind = PowerPlantKind::Hydro;
    let w = river_town();
    let mut lots: Vec<(LotId, f32)> = w.lots.iter().filter(|(id, l)| l.usable() && l.area() >= kind.min_area() && w.plant_preview(kind, *id).is_some_and(|p| p.problem.is_none())).map(|(id, l)| (id, l.area())).collect();
    lots.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert!(lots.len() >= 2, "lots on the river: {}", lots.len());
    let (small, big) = (lots[0], *lots.last().unwrap());
    assert!(big.1 > small.1 * 1.1, "lots differ: {} {}", small.1, big.1);
    let (ps, pb) = (w.plant_preview(kind, small.0).unwrap(), w.plant_preview(kind, big.0).unwrap());
    assert!(ps.problem.is_none() && pb.problem.is_none(), "{:?} {:?}", ps.problem, pb.problem);
    assert!(pb.capacity_mw > ps.capacity_mw && pb.cost > ps.cost, "{ps:?} {pb:?}");
    assert!((ps.site - 1.25).abs() < 0.01, "the current still counts: {}", ps.site);
    let mut w = w;
    let a = w.build_power(small.0, kind).unwrap();
    let b = w.build_power(big.0, kind).unwrap();
    assert_eq!(w.buildings[a].plant_scale, Some(ps.scale), "the size is kept on the plant");
    assert!((w.plant_capacity_mw(&w.buildings[b]) - pb.capacity_mw).abs() < 0.01);
    assert!((w.plant_output_mw(&w.buildings[b]) - pb.capacity_mw * 1.25).abs() < 0.05, "output is the nameplate times the current");
    // Saved before hydro had a size: the plant keeps its 70 MW.
    let mut old = w.clone();
    old.buildings[b].plant_scale = None;
    let text = old.to_ron().unwrap().replace("plant_scale:None,", "").replace("plant_scale: None,", "");
    let loaded = World::from_ron(&text).unwrap();
    assert_eq!(loaded.plant_capacity_mw(&loaded.buildings[b]), kind.capacity_mw());
    assert_eq!(loaded.plant_upkeep(&loaded.buildings[b]), kind.upkeep_per_day());
    // A new one keeps the size it was built at.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.buildings[a].plant_scale, Some(ps.scale));
}

/// [review] `build_power` copied the preview's site checks minus the
/// thickness one, so the click put heavy plants where the hover refused.
#[test]
fn building_a_plant_refuses_what_the_preview_refuses() {
    let mut w = town();
    // A lot squeezed to a long strip along the kerb: big enough, too thin.
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.car_access()).map(|(id, _)| id).next().unwrap();
    let c = w.lots[lot].center;
    w.lots[lot].polygon = square(c.x - 60.0, -18.0, 120.0, 10.0);
    let strip = &w.lots[lot];
    assert!(strip.area() >= PowerPlantKind::Coal.min_area() && crate::zoning::site_thickness(&strip.polygon) < PowerPlantKind::Coal.min_thickness());
    let problem = w.placement_problem(Placeable::Plant(PowerPlantKind::Coal), lot);
    assert!(problem.as_deref().is_some_and(|p| p.contains("too narrow")), "{problem:?}");
    let built = w.build_power(lot, PowerPlantKind::Coal);
    assert!(built.is_err(), "the click refuses it too: {built:?}");
    assert_eq!(built.err(), problem);
}
