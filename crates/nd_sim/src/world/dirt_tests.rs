//! Tests for `world/dirt.rs`: dirty industry fouls the ground and its
//! neighbours, the compatibility table steers new industry away, heavy
//! lorries wear the roads, parks and a clean-up mend the ground, and saves
//! from before all this still load.

use super::*;
use crate::landscape::LandscapeParams;
use crate::traffic::Cargo;

fn flat() -> World {
    World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::test_town() })
}

/// A long street along y = 0 with industry north of it and homes south of
/// it; industrial and residential lots, each sorted west to east.
fn strip() -> (World, Vec<LotId>, Vec<LotId>) {
    let mut w = flat();
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-650.0, 8.0), Vec2::new(650.0, 8.0), Vec2::new(650.0, 60.0), Vec2::new(-650.0, 60.0)], LandUse::Industrial);
    w.add_zone(vec![Vec2::new(-650.0, -60.0), Vec2::new(650.0, -60.0), Vec2::new(650.0, -8.0), Vec2::new(-650.0, -8.0)], LandUse::Residential);
    let sorted = |w: &World, lu: LandUse| {
        let mut v: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu).map(|(id, _)| id).collect();
        v.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
        v
    };
    let ind = sorted(&w, LandUse::Industrial);
    let res = sorted(&w, LandUse::Residential);
    (w, ind, res)
}

/// An open, fully staffed building on `lot` hosting `kind`.
fn make(w: &mut World, lot: LotId, kind: BusinessKind) -> BuildingId {
    if w.lots[lot].building.is_none() {
        w.build_on(lot);
    }
    let id = w.lots[lot].building.unwrap();
    w.finish_construction(id);
    let jobs = w.buildings[id].jobs.max(4);
    let b = &mut w.buildings[id];
    b.jobs = jobs;
    b.business = Some(Business::new(kind, jobs));
    b.workers = vec![CitizenId::default(); jobs as usize];
    b.powered = 1.0;
    id
}

fn nearest(w: &World, lots: &[LotId], x: f32) -> LotId {
    *lots.iter().min_by(|a, b| (w.lots[**a].center.x - x).abs().total_cmp(&(w.lots[**b].center.x - x).abs())).unwrap()
}

#[test]
fn the_table_sets_who_suffers() {
    use crate::industry::{exposure, sensitivity, IndustryKind as K, Neighbour as N};
    // Farms, homes, fishing and tech hate dirty industry; heavy industry shrugs.
    for dirty in [K::Smelting, K::Mining, K::Refinery, K::OilExtraction, K::FactoryFarming] {
        assert!(dirty.is_dirty(), "{dirty:?}");
        assert!(sensitivity(N::Industry(K::Farming), dirty) >= 0.6);
        assert!(sensitivity(N::Land(LandUse::Residential), dirty) >= 0.99);
        assert!(sensitivity(N::Industry(K::Logistics), dirty) <= 0.1);
        assert!(sensitivity(N::Industry(K::Smelting), dirty) == 0.0);
    }
    for clean in [K::Farming, K::Tech, K::Fishing, K::Logistics, K::Manufacturing] {
        assert!(!clean.is_dirty() && sensitivity(N::Industry(K::Farming), clean) == 0.0, "{clean:?}");
    }
    // A farm 80 m from a smelter is badly hit; 450 m away it is not.
    let near = exposure(N::Industry(K::Farming), &[(K::Smelting, 80.0)]);
    let far = exposure(N::Industry(K::Farming), &[(K::Smelting, 450.0)]);
    assert!(near > 0.5 && far == 0.0, "{near} {far}");
    // Two plants are worse than one, never past the cap.
    let two = exposure(N::Industry(K::Farming), &[(K::Smelting, 80.0), (K::Refinery, 100.0)]);
    assert!(two > near && two <= crate::industry::MAX_EXPOSURE);
    // A mercury works is the dirtiest neighbour.
    assert!(BusinessKind::ChemicalWorks.dirtiness() > BusinessKind::Refinery.dirtiness());
}

#[test]
fn a_smelter_fouls_the_ground_which_fades_only_slowly() {
    let (mut w, ind, _) = strip();
    let smelter = make(&mut w, ind[ind.len() / 2], BusinessKind::Smelter);
    let at = w.lots[w.buildings[smelter].lot].center;
    for _ in 0..24 * 14 {
        w.industry_hourly();
    }
    let dirty = w.contamination.at(at);
    assert!(dirty > 0.4, "two weeks of smelting poisons the ground: {dirty}");
    let ring = w.contamination.at(at + Vec2::new(90.0, 0.0));
    assert!(ring > 0.05 && ring < dirty, "less further out: {ring}");
    let total = w.contamination.within(at, 400.0);
    // The smelter shuts; a month on the ground is still poisoned.
    w.buildings[smelter].business.as_mut().unwrap().closed = true;
    for _ in 0..30 {
        w.industry_daily();
    }
    let month = w.contamination.within(at, 400.0);
    assert!(month > total * 0.6, "a month later most of it is still there: {total} -> {month}");
    assert!(month < total, "but it fades");
    // A year on it has mostly gone.
    for _ in 0..365 {
        w.industry_daily();
    }
    assert!(w.contamination.within(at, 400.0) < total * 0.15, "a year later: {}", w.contamination.within(at, 400.0));
}

#[test]
fn farms_and_homes_next_to_a_smelter_lose_output_and_value() {
    let (mut w, ind, res) = strip();
    let x0 = w.lots[ind[0]].center.x;
    let near_farm = make(&mut w, ind[1], BusinessKind::GrainFarm);
    let far_farm = make(&mut w, *ind.last().unwrap(), BusinessKind::GrainFarm);
    let near_home = nearest(&w, &res, x0);
    let far_home = *res.last().unwrap();
    w.update_land_value();
    let (home_near_before, home_far_before) = (w.lot_value(near_home), w.lot_value(far_home));
    let smelter = make(&mut w, ind[0], BusinessKind::Smelter);
    w.refresh_dirty();
    assert_eq!(w.dirty.len(), 1);
    assert_eq!(w.dirty[0].building, smelter);
    // The neighbours, worst first, with what it costs them.
    let n = w.unhappy_neighbours(near_farm);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].source, smelter);
    assert_eq!(n[0].what, "smelter");
    assert!(w.unhappy_neighbours(far_farm).is_empty());
    let cost = w.adjacency_cost(near_farm).unwrap();
    assert!(cost.contains("food output −") && cost.contains("land value −"), "{cost}");
    assert!(w.adjacency_output_factor(near_farm) < 0.8 && w.adjacency_output_factor(far_farm) == 1.0);
    // A day of farming: the near farm makes clearly less grain.
    for _ in 0..16 {
        for f in [near_farm, far_farm] {
            let jobs = w.buildings[f].jobs;
            w.buildings[f].business.as_mut().unwrap().produce(jobs, 1.0);
        }
        w.industry_hourly();
    }
    let grain = |w: &World, b: BuildingId| w.buildings[b].business.as_ref().unwrap().stock_of(Resource::Grain);
    let (gn, gf) = (grain(&w, near_farm), grain(&w, far_farm));
    assert!(gf > 0.0 && gn < gf * 0.85, "near {gn} vs far {gf}");
    // Homes and the farm itself are worth less next door.
    w.update_land_value();
    let home_near = w.lot_value(near_home);
    let home_far = w.lot_value(far_home);
    assert!(home_near < home_near_before - 0.1, "{home_near_before} -> {home_near}");
    assert!((home_far - home_far_before).abs() < 0.05, "far homes barely notice: {home_far_before} -> {home_far}");
    // A factory or a logistics yard on the same spot hardly minds.
    let farm_exposure = w.building_exposure(near_farm);
    w.buildings[near_farm].business = Some(Business::new(BusinessKind::ContainerYard, 4));
    assert!(w.building_exposure(near_farm) < farm_exposure * 0.2, "logistics shrugs it off");
}

#[test]
fn new_farms_pick_clean_ground_away_from_dirty_industry() {
    let (mut w, ind, _) = strip();
    make(&mut w, ind[0], BusinessKind::Smelter);
    w.update_land_value();
    let share = |w: &World, lot: LotId, k: IndustryKind| {
        let wt = w.industry_weights(lot).unwrap();
        wt[k.index()] / wt.iter().sum::<f32>()
    };
    let (near, far) = (ind[2], ind[ind.len() - 2]);
    let (fn_, ff) = (share(&w, near, IndustryKind::Farming), share(&w, far, IndustryKind::Farming));
    assert!(fn_ < ff * 0.4, "farming by the smelter {fn_} vs on clean ground {ff}");
    // Heavy industry does not mind the company.
    let weight = |w: &World, lot: LotId, k: IndustryKind| w.industry_weights(lot).unwrap()[k.index()];
    let farm_ratio = weight(&w, near, IndustryKind::Farming) / weight(&w, far, IndustryKind::Farming);
    let works_ratio = weight(&w, near, IndustryKind::Manufacturing) / weight(&w, far, IndustryKind::Manufacturing);
    assert!(works_ratio > farm_ratio * 3.0, "manufacturing minds far less: {works_ratio} vs {farm_ratio}");
    // Poisoned ground alone keeps farms off too.
    w.contamination.add(w.lots[far].center, 60.0, 0.9);
    assert!(share(&w, far, IndustryKind::Farming) < ff * 0.5);
}

#[test]
fn smelters_follow_the_mines() {
    let (mut w, ind, _) = strip();
    let lot = ind[ind.len() / 2];
    let none = w.industry_weights(lot).unwrap()[IndustryKind::Smelting.index()];
    assert_eq!(none, 0.0, "no ore in reach, no smelter");
    make(&mut w, ind[0], BusinessKind::StripMine);
    let some = w.industry_weights(lot).unwrap()[IndustryKind::Smelting.index()];
    assert!(some > 0.0, "a mine in reach: {some}");
    // Two smelters to a mine, no more.
    make(&mut w, ind[1], BusinessKind::Smelter);
    make(&mut w, ind[2], BusinessKind::Smelter);
    assert_eq!(w.industry_weights(lot).unwrap()[IndustryKind::Smelting.index()], 0.0, "the mine's ore is spoken for");
}

#[test]
fn ore_lorries_wear_the_road_faster_than_cars() {
    use crate::road_wear::{vehicle_load, HEAVY};
    let cargo = |r: Resource, amount: f32| Cargo { from: BuildingId::default(), to: BuildingId::default(), resource: r, amount, price: 1.0 };
    assert_eq!(vehicle_load(None, false), 1.0);
    assert_eq!(vehicle_load(None, true), HEAVY);
    assert_eq!(vehicle_load(Some(&cargo(Resource::Groceries, 5.0)), false), HEAVY);
    assert!(vehicle_load(Some(&cargo(Resource::Ore, 5.0)), false) > 2.0 * HEAVY);
    assert_eq!(vehicle_load(Some(&cargo(Resource::Ore, 0.0)), false), HEAVY, "an empty lorry is just a lorry");
    // Two equal streets, a car on one and an ore lorry from the mine on the other.
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0)], 1, 1);
    w.set_road_funding(0.0);
    let seg_at = |w: &World, y: f32| w.roads.alive_segments().find(|(sid, _)| { let (a, b) = w.roads.segment_points(*sid); ((a + b) * 0.5).y == y }).map(|(sid, _)| sid).unwrap();
    let (car_seg, lorry_seg) = (seg_at(&w, 0.0), seg_at(&w, 200.0));
    let spawn = |w: &mut World, seg: SegmentId, y: f32, cargo: Option<Cargo>| {
        let (lanes, s0, s1) = w.traffic.net.find_route(seg, Vec2::new(-200.0, y), seg, Vec2::new(200.0, y)).unwrap();
        w.traffic.spawn_car(CitizenId::default(), cargo, lanes, s0, s1, 1)
    };
    spawn(&mut w, car_seg, 0.0, None);
    spawn(&mut w, lorry_seg, 200.0, Some(cargo(Resource::Ore, 8.0)));
    for _ in 0..50 {
        w.sample_road_load();
    }
    let before = (w.road_wear.wear_of(car_seg.0 as usize), w.road_wear.wear_of(lorry_seg.0 as usize));
    w.daily_roads();
    let car = w.road_wear.wear_of(car_seg.0 as usize) - before.0;
    let lorry = w.road_wear.wear_of(lorry_seg.0 as usize) - before.1;
    let age = crate::road_wear::AGE_WEAR;
    assert!(lorry - age > (car - age) * 8.0, "ore lorry {lorry} vs car {car}");
}

#[test]
fn neighbours_of_dirty_industry_are_unhappy_and_parks_and_cleanups_mend_the_ground() {
    let (mut w, ind, res) = strip();
    let x0 = w.lots[ind[0]].center.x;
    make(&mut w, ind[0], BusinessKind::ChemicalWorks);
    let home_lot = nearest(&w, &res, x0);
    w.build_on(home_lot);
    let home = w.lots[home_lot].building.unwrap();
    w.finish_construction(home);
    for _ in 0..10 {
        if !w.buildings[home].residents.is_empty() {
            break;
        }
        w.immigrate();
    }
    let c = *w.buildings[home].residents.first().expect("someone moved in");
    w.citizens[c].mood = 0.8;
    w.refresh_dirty();
    w.clock.tick += 1440 * 3;
    w.industry_daily();
    assert!(w.citizens[c].mood < 0.8, "living by the chemical works wears you down");
    let heard = w.concerns.count(crate::concerns::Concern::Noise, 0, w.clock.day() + 1);
    assert!(heard >= 1, "and people say so");
    // A park slowly cleans the ground under it.
    let park_lot = *res.last().unwrap();
    w.lots[park_lot].land_use = LandUse::Park;
    w.build_on(park_lot);
    let park = w.lots[park_lot].building.expect("a park");
    w.finish_construction(park);
    assert_eq!(w.buildings[park].land_use, LandUse::Park);
    let spot = w.lots[park_lot].center;
    w.contamination.add(spot, 20.0, 0.8);
    let before = w.contamination.within(spot, 60.0);
    let mut alone = w.contamination.clone();
    for _ in 0..20 {
        w.daily_dirt();
        alone.daily();
        w.contamination.daily();
    }
    assert!(w.contamination.within(spot, 60.0) < alone.within(spot, 60.0) * 0.8, "the park cleans: {} vs {}", w.contamination.within(spot, 60.0), alone.within(spot, 60.0));
    assert!(w.contamination.within(spot, 60.0) < before);
    // A clean-up, paid for.
    w.economy.treasury = 50_000.0;
    let (cost, removed) = w.remediate(spot, 80.0).unwrap();
    assert!(cost > 0.0 && removed > 0.0);
    assert!(w.contamination.within(spot, 80.0) < 0.01);
    assert!(w.economy.treasury < 50_000.0 - cost + 1.0);
    assert!(w.remediate(spot, 80.0).is_err(), "nothing left to clean");
}

#[test]
fn saves_from_before_dirty_industry_load() {
    let (mut w, ind, _) = strip();
    make(&mut w, ind[0], BusinessKind::StripMine);
    make(&mut w, ind[3], BusinessKind::GrainFarm);
    w.refresh_dirty();
    assert_eq!(w.dirty.len(), 1);
    let text = w.to_ron().unwrap();
    // The same save without the new field reads its dirty plants again.
    let start = text.find("dirty: [").expect("the field is saved");
    let mut depth = 0;
    let mut end = start;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if text[end..].starts_with(',') {
        end += 1;
    }
    let old = format!("{}{}", &text[..start], &text[end..]);
    assert!(!old.contains("dirty: ["));
    let back = World::from_ron(&old).unwrap();
    // (Its made-up workers do not survive the load, so it reads as idle.)
    assert_eq!(back.dirty.len(), 1);
    assert_eq!((back.dirty[0].building, back.dirty[0].as_kind, back.dirty[0].reach), (w.dirty[0].building, w.dirty[0].as_kind, w.dirty[0].reach));
    // And a save with the field comes back exactly.
    let again = World::from_ron(&text).unwrap();
    assert_eq!(again.dirty, w.dirty);
    // Old kinds keep their places: new kinds were appended.
    assert_eq!(BusinessKind::ALL[30], BusinessKind::StripMine);
    assert_eq!(BusinessKind::ALL[BusinessKind::ChemicalWorks.index()], BusinessKind::ChemicalWorks);
    assert_eq!(BusinessKind::ChemicalWorks.index(), 42, "[waterfront] kinds appended after it");
    assert_eq!(IndustryKind::ALL[7], IndustryKind::Mining);
    assert_eq!(Resource::ALL[17], Resource::Art);
}

/// [dirty-industry] The Bulldoze tool's clean-up: the quote names area, cost
/// and days before the click; the crew digs the stain out ring by ring over
/// those days and leaves; a second order on the same spot is refused.
#[test]
fn a_cleanup_crew_digs_the_ground_out_over_a_few_days() {
    let mut w = flat();
    let spot = Vec2::new(100.0, 100.0);
    assert!(w.cleanup_quote(spot, 80.0).is_err(), "clean ground: nothing to quote");
    w.contamination.add(spot, 90.0, 0.8);
    let before = w.contamination.within(spot, 80.0);
    let q = w.cleanup_quote(spot, 80.0).expect("a quote");
    assert!((q.cost - before * CLEANUP_COST).abs() < 1.0, "450 per cell of full contamination: {q:?}");
    assert!(q.cells > 0 && (2..=6).contains(&q.days), "{q:?}");
    // Too dear: refused before the click, and saying why.
    let t = w.economy.treasury;
    w.economy.treasury = q.cost * 0.5;
    assert!(w.cleanup_quote(spot, 80.0).unwrap_err().contains("costs"));
    w.economy.treasury = t;
    let placed = w.start_cleanup(spot, 80.0).expect("ordered");
    assert_eq!(placed, q);
    assert!((t - w.economy.treasury - q.cost).abs() < 1.0, "paid up front");
    assert_eq!(w.cleanups.len(), 1);
    assert!(w.cleanup_quote(spot + Vec2::new(10.0, 0.0), 60.0).unwrap_err().contains("already"));
    // Half way: the middle is clean, the rim not yet.
    for _ in 0..q.days * 12 {
        w.hourly_cleanups();
    }
    let j = w.cleanups[0].clone();
    assert!(j.progress() > 0.45 && j.progress() < 0.55);
    assert_eq!(w.contamination.at(spot), 0.0, "dug out from the middle");
    assert!(w.contamination.within(spot, 80.0) > 0.0, "the rim is still to do");
    for _ in 0..q.days * 12 {
        w.hourly_cleanups();
    }
    assert!(w.cleanups.is_empty(), "the crew has left");
    assert!(w.contamination.within(spot, 80.0) < 0.01, "clean: {}", w.contamination.within(spot, 80.0));
    // Saved and loaded mid-job.
    w.contamination.add(spot, 60.0, 0.5);
    w.start_cleanup(spot, 60.0).unwrap();
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.cleanups, w.cleanups);
}

/// [dirty-industry] Contamination drifts a little downwind and down rivers:
/// bounded (nothing is made), deterministic, and only where it lies.
#[test]
fn contamination_drifts_downwind_and_downstream() {
    use crate::ground::Contamination;
    use crate::landscape::River;
    let spot = Vec2::new(0.0, 0.0);
    let east = Vec2::new(96.0, 0.0);
    let west = Vec2::new(-96.0, 0.0);
    // Wind to the east: more lands east than west.
    let mut c = Contamination::new(600.0, 32.0);
    c.add(spot, 40.0, 0.9);
    let total = c.total();
    let mut d = c.clone();
    for _ in 0..20 {
        c.daily();
        c.drift(Vec2::new(0.8, 0.0), &[]);
        d.daily();
        d.drift(Vec2::new(0.8, 0.0), &[]);
    }
    assert_eq!(c, d, "deterministic");
    assert!(c.at(east) > c.at(west) * 1.2, "downwind {} vs upwind {}", c.at(east), c.at(west));
    assert!(c.total() <= total, "drift makes nothing");
    // A river flowing north carries it north, not south.
    let river = River { points: vec![Vec2::new(0.0, -500.0), Vec2::new(0.0, 500.0)], width: 30.0, current: 0.8, stream: false };
    let mut r = Contamination::new(600.0, 32.0);
    r.add(spot, 20.0, 0.9);
    for _ in 0..15 {
        r.daily();
        r.drift(Vec2::ZERO, std::slice::from_ref(&river));
    }
    let (north, south) = (r.at(Vec2::new(0.0, 160.0)), r.at(Vec2::new(0.0, -160.0)));
    assert!(north > south * 2.0 && north > 0.0, "downstream {north} vs upstream {south}");
    // Far off the river and with no wind, drift does nothing.
    let mut still = Contamination::new(600.0, 32.0);
    still.add(Vec2::new(300.0, 300.0), 30.0, 0.9);
    let before = still.clone();
    still.drift(Vec2::ZERO, std::slice::from_ref(&river));
    assert_eq!(still.cells, before.cells);
}
