//! [supertall] Tests of the skyscraper league: towers only on dear, dense
//! downtown land; site assembly buys grown neighbours and never the city's
//! buildings; height limits hold; the record climbs when allowed; old saves
//! load; and it all runs the same twice.

use super::*;
use crate::estate::{Form, OwnerKind};
use crate::mind::IdleMind;
use crate::towers::*;

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn town() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn built(w: &mut World, lot: LotId) -> Option<BuildingId> {
    w.build_on(lot);
    let b = w.lots[lot].building?;
    w.finish_construction(b);
    Some(b)
}

/// A downtown in the west (offices both sides of the avenue, ten floors,
/// land worth 0.9) and a cheap low strip in the east (two floors, 0.3).
fn downtown() -> World {
    let mut w = town();
    w.add_road(&[Vec2::new(-760.0, 0.0), Vec2::new(760.0, 0.0)], 2, 2);
    w.add_zone(rect(-700.0, 10.0, 560.0, 90.0), LandUse::Commercial);
    w.add_zone(rect(-700.0, -100.0, 560.0, 90.0), LandUse::Commercial);
    w.add_zone(rect(300.0, 10.0, 360.0, 90.0), LandUse::Commercial);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access()).map(|(id, _)| id).collect();
    for lot in lots {
        let west = w.lots[lot].center.x < 0.0;
        if let Some(b) = built(&mut w, lot) {
            let bld = &mut w.buildings[b];
            bld.floors = if west { 10 } else { 2 };
            bld.tier = if west { Tier::High } else { Tier::Low };
            bld.built_tick = 0;
            bld.recompute_capacity();
        }
    }
    let lv = &mut w.land_value;
    for iy in 0..lv.n {
        for ix in 0..lv.n {
            let c = lv.cell_center(ix, iy);
            lv.values[iy * lv.n + ix] = if c.x < 0.0 { 0.9 } else { 0.3 };
        }
    }
    w.clock = crate::clock::SimClock::starting_at(5, 1, 0);
    w.economy.demand[LandUse::Commercial.index()] = 0.8;
    w.skyline.always = true;
    w
}

fn rich(w: &mut World) {
    if w.skyline.developers.is_empty() {
        w.daily_towers();
    }
    for d in &mut w.skyline.developers {
        d.capital = 5e9;
    }
}

fn tower_of(w: &World, b: BuildingId) -> Tower {
    w.buildings[b].plot.tower.expect("a league tower")
}

#[test]
fn a_tower_rises_only_on_dear_dense_land() {
    let mut w = downtown();
    let sites = w.tower_sites(false);
    assert!(!sites.is_empty(), "downtown offers sites");
    assert!(sites.iter().all(|s| w.lots[s.lot].center.x < 0.0), "only downtown: the cheap low strip offers none");
    // No money, no tower.
    w.daily_towers();
    for d in &mut w.skyline.developers {
        d.capital = 0.0;
    }
    assert!(w.start_tower(TowerOrder::default()).is_err(), "a developer needs the money");
    rich(&mut w);
    let bid = w.start_tower(TowerOrder::default()).expect("a tower breaks ground");
    let b = &w.buildings[bid];
    assert!(w.lots[b.lot].center.x < 0.0, "downtown");
    assert_eq!(b.plot.form, Some(Form::Tower));
    assert_eq!(b.tier, Tier::High);
    let t = tower_of(&w, bid);
    assert!(t.height >= SKYSCRAPER_M, "in the league: {}", t.height);
    assert!(b.construction.as_ref().is_some_and(|c| c.days >= 20.0), "a long build");
    assert!(matches!(w.owner_of(bid).map(|o| &o.1.kind), Some(OwnerKind::Developer)), "owned by its developer");
    assert!(w.skyline.news.last().is_some_and(|n| n.headline.contains("breaks ground")), "{:?}", w.skyline.news.last());
    // Cheap land everywhere: nothing.
    let mut poor = downtown();
    poor.land_value.fill(0.3);
    rich(&mut poor);
    assert!(poor.tower_sites(false).is_empty());
    assert!(poor.start_tower(TowerOrder::default()).is_err());
}

#[test]
fn site_assembly_buys_grown_neighbours_never_city_buildings() {
    let mut w = downtown();
    rich(&mut w);
    // A police station in the middle of downtown: the city's own.
    let mid = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Commercial && l.center.x < -300.0 && l.center.x > -500.0 && l.center.y > 0.0 && l.building.is_some()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id).unwrap();
    let old = w.lots[mid].building.unwrap();
    let _ = w.bulldoze_building(old);
    w.finish_demolition(old);
    let police = w.build_service(mid, crate::services::ServiceKind::Police, Tier::Mid).expect("a police station");
    w.finish_construction(police);
    let near = w.lots[mid].center;
    let bid = w.start_tower(TowerOrder { height: Some(420.0), near: Some(near), force: false, design: None }).expect("a supertall");
    let t = tower_of(&w, bid);
    assert_eq!(t.league(), League::Supertall);
    let b = &w.buildings[bid];
    let plots: Vec<LotId> = b.plot.extra_lots.iter().chain(&b.plot.pending_lots).copied().collect();
    assert!(!plots.is_empty(), "a supertall needs more than one plot");
    assert!(!plots.contains(&mid) && b.lot != mid, "never the city's police station");
    assert!(w.buildings.contains_key(police) && w.buildings[police].demolition.is_none(), "which stands untouched");
    let area: f32 = std::iter::once(b.lot).chain(plots.iter().copied()).map(|l| w.lots[l].area()).sum();
    assert!(area >= League::Supertall.min_area(), "a big enough site: {area}");
    // The grown neighbours were bought and are coming down.
    for l in &b.plot.pending_lots {
        let other = w.lots[*l].building.expect("still standing until cleared");
        assert!(w.buildings[other].demolition.is_some(), "bought and coming down");
        assert!(w.buildings[other].residents.is_empty() && w.buildings[other].workers.len() <= crate::buildings::Demolition::crew(10) as usize);
    }
    // Finished: one building over the merged plots.
    w.finish_towers();
    let b = &w.buildings[bid];
    assert!(b.construction.is_none() && b.plot.pending_lots.is_empty());
    assert_eq!(b.plot.extra_lots.len(), plots.len(), "every bought plot joined");
    let fp = w.footprint(bid).unwrap();
    assert!(fp.area() > w.lots[b.lot].area() * 1.5, "drawn on the assembled site");
    assert!(b.jobs > 100 && b.jobs <= 1500, "offices by the floor, capped: {}", b.jobs);
    assert!(w.skyline_rank(bid) == Some(1), "the tallest");
}

#[test]
fn height_limits_hold() {
    let mut w = downtown();
    rich(&mut w);
    w.set_height_limit(Some(120.0));
    assert!(w.start_tower(TowerOrder::default()).is_err(), "under 150 m: no league at all");
    let lot = w.lots.iter().find(|(_, l)| l.land_use == LandUse::Commercial && l.center.x < 0.0).map(|(id, _)| id).unwrap();
    assert!(w.height_cap(lot) as f32 * FLOOR_M[LandUse::Commercial.index()] <= 120.0, "ordinary buildings too");
    w.set_height_limit(Some(220.0));
    let bid = w.start_tower(TowerOrder::default()).expect("a skyscraper under the limit");
    assert!(tower_of(&w, bid).height <= 220.0);
    assert!(w.start_tower(TowerOrder { height: Some(400.0), ..Default::default() }).is_err(), "nor when asked for more");
    // A district limit: the whole downtown held to 100 m.
    w.set_height_limit(None);
    let d = w.create_district(Some("Old Quarter".into())).unwrap();
    w.paint_district_polygon(&rect(-760.0, -120.0, 700.0, 240.0), d).unwrap();
    w.set_district_policy(d, super::districts::PolicyChange { height_limit: Some(Some(100.0)), ..Default::default() }).unwrap();
    assert!(w.tower_sites(false).is_empty(), "the district's limit holds");
    assert!(w.start_tower(TowerOrder::default()).is_err());
}

#[test]
fn the_record_climbs_when_the_city_allows() {
    let mut w = downtown();
    let mut heights = Vec::new();
    for _ in 0..3 {
        rich(&mut w);
        let bid = w.start_tower(TowerOrder::default()).expect("a project");
        heights.push(tower_of(&w, bid).height);
        w.finish_towers();
        w.clock.tick += 5 * 1440;
    }
    assert!(heights.windows(2).all(|p| p[1] > p[0]), "each one-ups the last: {heights:?}");
    assert_eq!(w.skyline.records.len(), 3, "three records: {:?}", w.skyline.records);
    assert!(w.skyline.news.iter().any(|n| n.headline.contains("tops out") && n.headline.contains("the city's tallest")), "{:?}", w.skyline.news);
    assert!(w.skyline.record() >= heights[2] - 0.01);
    // A limit at the record: the next one cannot beat it.
    let record = w.skyline.record();
    w.set_height_limit(Some(record));
    rich(&mut w);
    if let Ok(bid) = w.start_tower(TowerOrder::default()) {
        assert!(tower_of(&w, bid).height <= record, "held under the limit");
        w.finish_towers();
    }
    assert_eq!(w.skyline.record(), record, "the record stands");
}

#[test]
fn towers_are_left_alone_by_the_rest_of_the_market() {
    let mut w = downtown();
    rich(&mut w);
    let bid = w.start_tower(TowerOrder { height: Some(250.0), ..Default::default() }).unwrap();
    w.finish_towers();
    assert!(!w.buyable(bid), "no buyout of a tower");
    let floors = w.buildings[bid].floors;
    let mut minds = IdleMind;
    for _ in 0..(3 * 1440) {
        w.tick(&mut minds);
    }
    let b = w.buildings.get(bid).expect("still standing");
    assert!(b.plot.tower.is_some() && b.floors == floors && b.demolition.is_none(), "unchanged after three days");
}

#[test]
fn deterministic_and_saved() {
    let run = || {
        let mut w = downtown();
        let mut minds = IdleMind;
        for d in 0..6 {
            w.daily_towers();
            for dv in &mut w.skyline.developers {
                dv.capital += 2e8;
            }
            for _ in 0..1440 {
                w.tick(&mut minds);
            }
            let _ = d;
        }
        w
    };
    let (a, b) = (run(), run());
    let sig = |w: &World| w.league_towers().iter().map(|id| (w.buildings[*id].name.clone(), tower_of(w, *id).height.to_bits(), w.buildings[*id].plot.extra_lots.len())).collect::<Vec<_>>();
    assert_eq!(sig(&a), sig(&b), "the same towers twice");
    assert!(!a.league_towers().is_empty(), "some tower started over six days");
    let text = a.to_ron().unwrap();
    let back = World::from_ron(&text).expect("loads");
    assert_eq!(sig(&back), sig(&a));
    assert_eq!(back.skyline, a.skyline);
}

/// A real save from before the league (`saves/flags/flag_patch.ron` copied
/// into the worktree; skipped when absent): it loads with no limit and no
/// towers, and runs through a day.
#[test]
fn an_old_save_loads_without_the_league() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../saves/flags/flag_patch.ron");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no {}", path.display());
        return;
    };
    let mut w = World::from_ron(&text).expect("the old save loads");
    assert_eq!(w.skyline, crate::towers::Skyline::default());
    assert!(w.buildings.values().all(|b| b.plot.tower.is_none()));
    let mut minds = IdleMind;
    let start = w.clock.day();
    while w.clock.day() == start || w.clock.minute_of_day() < 30 {
        w.tick(&mut minds);
    }
}
