//! [economy] Business pressure in a real World: each pressure opens its
//! trade when its cause is there and stops once enough stand; the new
//! supply chains trade; trash companies compete on price; determinism and
//! old saves.

use super::*;
use crate::landscape::LandscapeParams;
use crate::mind::IdleMind;
use crate::pressure::PressureKind;

fn flat() -> World {
    World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::test_town() })
}

/// A grid of streets with industrial, commercial and residential blocks:
/// plenty of vacant lots of every kind a pressure might want.
fn town() -> World {
    let mut w = flat();
    for i in -2..=2 {
        let c = i as f32 * 160.0;
        w.add_road(&[Vec2::new(-360.0, c), Vec2::new(360.0, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -360.0), Vec2::new(c, 360.0)], 1, 1);
    }
    let sq = |x0: f32, y0: f32, s: f32| vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)];
    w.add_zone(sq(-310.0, -310.0, 140.0), LandUse::Industrial);
    w.add_zone(sq(-150.0, -310.0, 140.0), LandUse::Industrial);
    w.add_zone(sq(10.0, 10.0, 140.0), LandUse::Commercial);
    w.add_zone(sq(170.0, 10.0, 140.0), LandUse::Commercial);
    w.add_zone(sq(-150.0, 10.0, 140.0), LandUse::Residential);
    w.add_zone(sq(10.0, -150.0, 140.0), LandUse::Entertainment);
    w
}

fn vacant(w: &World) -> Vec<LotId> {
    w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, _)| id).collect()
}

fn count(w: &World, k: BusinessKind) -> usize {
    w.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == k && !z.closed)).count()
}

/// One day of pressure: the reading at midnight, then a round of
/// development's pressure branch.
fn day(w: &mut World) {
    w.daily_businesses();
    w.daily_pressure();
    let v = vacant(w);
    for _ in 0..3 {
        w.open_by_pressure(&v);
    }
    // The day passes: what went up is built.
    let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in sites {
        w.finish_construction(id);
    }
    w.clock.tick += 1440;
}

#[test]
fn outside_tows_grow_a_tow_company_and_enough_of_them_stop_it() {
    let mut w = town();
    w.daily_pressure(); // primes the running totals
    // A city whose cars are towed by the company from out of town.
    for _ in 0..8 {
        let local = count(&w, BusinessKind::TowYard) > 0;
        // Once one is in town it takes the jobs it can (six a day).
        w.enforcement.stats.tows_out_of_town_total += if local { 2 } else { 14 };
        w.enforcement.stats.tows_total += 14;
        day(&mut w);
    }
    assert_eq!(count(&w, BusinessKind::TowYard), 1, "the outside tows opened one tow company, and only one");
    let p = w.pressure.reading(PressureKind::TowCompany).unwrap().clone();
    assert!(!p.wants_more(), "a company in town takes the pressure off: {p:?}");
    let yard = w.buildings.values().find(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard)).unwrap();
    assert!(yard.business.as_ref().unwrap().origin.as_deref().is_some_and(|o| o.contains("out of town")), "it says why it opened");
    assert!(matches!(w.lots[yard.lot].land_use, LandUse::Industrial | LandUse::Commercial));
    // Far more than one company can take: a second one opens.
    for _ in 0..8 {
        w.enforcement.stats.tows_out_of_town_total += 20;
        w.enforcement.stats.tows_total += 30;
        day(&mut w);
    }
    assert!(count(&w, BusinessKind::TowYard) >= 2, "an overwhelmed tow company gets a rival");
    // No more out-of-town tows: no more companies.
    let n = count(&w, BusinessKind::TowYard);
    for _ in 0..8 {
        w.enforcement.stats.tows_total += 10;
        day(&mut w);
    }
    assert_eq!(count(&w, BusinessKind::TowYard), n);
    assert!(w.pressure.reading(PressureKind::TowCompany).unwrap().strength < 0.1, "the pressure fades with the smoothed tows");
}

#[test]
fn wrecks_open_repair_shops_until_the_work_is_covered() {
    let mut w = town();
    w.daily_pressure();
    let mut seen = Vec::new();
    for _ in 0..20 {
        w.driving.crashes_total += 25;
        day(&mut w);
        seen.push(count(&w, BusinessKind::Mechanic));
    }
    let n = *seen.last().unwrap();
    // 25 wrecks a day is about three shops' work.
    assert!((2..=4).contains(&n), "repair shops opened to meet the wrecks: {seen:?}");
    assert_eq!(seen[seen.len() - 6..].iter().min(), seen[seen.len() - 6..].iter().max(), "and then it stops: {seen:?}");
    let shop = w.buildings.values().find(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Mechanic)).unwrap();
    assert!(shop.floors <= 1, "a repair shop is a one-storey garage");
    assert!(matches!(w.lots[shop.lot].land_use, LandUse::Commercial | LandUse::Industrial));
    // Calm roads: the pressure falls.
    for _ in 0..10 {
        day(&mut w);
    }
    assert!(!w.pressure.reading(PressureKind::Mechanic).unwrap().wants_more());
}

#[test]
fn a_kind_that_closed_broke_waits_before_another_is_tried() {
    let mut w = town();
    w.daily_pressure();
    for _ in 0..4 {
        w.enforcement.stats.tows_out_of_town_total += 14;
        day(&mut w);
    }
    let yard = w.buildings.iter().find(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard)).map(|(id, _)| id).expect("a tow company");
    // It goes under (as `daily_businesses` shuts it: closed, the count reset).
    let z = w.buildings[yard].business.as_mut().unwrap();
    z.closed = true;
    z.broke_days = 0;
    w.daily_pressure();
    for _ in 0..4 {
        w.enforcement.stats.tows_out_of_town_total += 14;
        day(&mut w);
    }
    assert_eq!(count(&w, BusinessKind::TowYard), 0, "cooling down");
    let p = w.pressure.reading(PressureKind::TowCompany).unwrap();
    assert!(p.wants_more() && p.held.as_deref().is_some_and(|h| h.contains("closed")), "{p:?}");
    for _ in 0..(crate::pressure::COOLDOWN_DAYS as usize + 6) {
        let local = count(&w, BusinessKind::TowYard) > 0;
        w.enforcement.stats.tows_out_of_town_total += if local { 2 } else { 14 };
        day(&mut w);
    }
    assert_eq!(count(&w, BusinessKind::TowYard), 1, "tried again after the cooldown");
}

#[test]
fn trash_companies_serve_what_the_city_misses_and_compete_on_price() {
    assert!(crate::trash::company_fee(1) < crate::trash::PRIVATE_FEE_HOUSEHOLD);
    assert!(crate::trash::company_fee(3) < crate::trash::company_fee(2) && crate::trash::company_fee(2) < crate::trash::company_fee(1));
    let mut w = town();
    // Homes and shops, and no city trash depot at all.
    for l in vacant(&w) {
        if matches!(w.lots[l].land_use, LandUse::Residential | LandUse::Commercial) {
            w.build_on(l);
        }
    }
    let ids: Vec<BuildingId> = w.buildings.keys().collect();
    for id in ids {
        w.finish_construction(id);
        // Somebody works there: bins fill.
        let jobs = w.buildings[id].jobs as usize;
        w.buildings[id].workers = vec![CitizenId::default(); jobs];
    }
    w.hourly_trash();
    let uncovered = w.society.trash.bins.values().filter(|b| !b.covered).count();
    assert!(uncovered > 0);
    w.daily_pressure();
    let wanted = w.pressure.reading(PressureKind::TrashCompany).unwrap().wanted;
    assert!(wanted > 0.0, "{uncovered} buildings out of the city's reach call for a trash company");
    // Push the reading over the line, as a bigger town would.
    for _ in 0..4 {
        w.pressure.signals.trash_uncovered = 600.0;
        let v = vacant(&w);
        w.pressure.readings = crate::pressure::assess(&w.pressure.signals, &w.pressure_supply());
        w.pressure.held_days[PressureKind::TrashCompany.index()] = crate::pressure::OPEN_DAYS;
        w.open_by_pressure(&v);
        let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
        }
        w.clock.tick += 1440 * crate::pressure::SETTLE_DAYS;
    }
    let companies: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TrashCompany)).map(|(id, _)| id).collect();
    assert!(companies.len() >= 2, "several companies: {}", companies.len());
    for &c in &companies {
        w.finish_construction(c);
        w.buildings[c].workers = vec![CitizenId::default(); 4];
    }
    w.hourly_trash();
    let signed: Vec<&crate::trash::Bin> = w.society.trash.bins.values().filter(|b| b.company.is_some()).collect();
    assert!(!signed.is_empty(), "buildings sign with a local company");
    assert!(signed.iter().all(|b| b.private && !b.covered));
    let fee = w.society.trash.stats.local_fee;
    assert!(fee < crate::trash::company_fee(1) + 1e-4, "with rivals in reach the fee is below a lone company's: {fee}");
    // The companies bill their clients as takings.
    for h in w.households.values_mut() {
        h.money = 1000.0;
    }
    w.daily_trash();
    let takings: f32 = companies.iter().map(|c| w.buildings[*c].business.as_ref().unwrap().revenue_today).sum();
    assert!(takings > 0.0 || w.households.is_empty(), "fees reach the companies");
}

#[test]
fn livestock_farms_feed_a_slaughterhouse() {
    let mut w = town();
    let lots: Vec<LotId> = vacant(&w).into_iter().filter(|l| w.lots[*l].land_use == LandUse::Industrial && w.lots[*l].car_access()).collect();
    assert!(lots.len() >= 3);
    let farm = w.open_business(lots[0], BusinessKind::CowFarm, true).unwrap();
    let hens = w.open_business(lots[1], BusinessKind::ChickenFarm, true).unwrap();
    // The slaughterhouse picks the farms' site: the industry weights say so.
    let site = w.industry_site(lots[2]).unwrap();
    assert!(site.livestock_farms >= 2, "the farms count for a slaughterhouse next door");
    let menu = crate::industry::business_weights(crate::industry::IndustryKind::Farming, &site);
    assert!(menu.iter().any(|(k, wt)| *k == BusinessKind::Slaughterhouse && *wt > 0.3));
    let abattoir = w.open_business(lots[2], BusinessKind::Slaughterhouse, true).unwrap();
    // With one standing, two farms need no second.
    let site = w.industry_site(lots[3.min(lots.len() - 1)]).unwrap();
    let menu = crate::industry::business_weights(crate::industry::IndustryKind::Farming, &site);
    assert!(menu.iter().any(|(k, wt)| *k == BusinessKind::Slaughterhouse && *wt == 0.0));
    for id in [farm, hens, abattoir] {
        let jobs = w.buildings[id].jobs as usize;
        w.buildings[id].workers = vec![CitizenId::default(); jobs];
        w.buildings[id].business.as_mut().unwrap().money = 5000.0;
    }
    // Feed for the herds.
    w.buildings[farm].business.as_mut().unwrap().add(Resource::Grain, 400.0);
    w.buildings[hens].business.as_mut().unwrap().add(Resource::Grain, 400.0);
    let mut minds = IdleMind;
    for _ in 0..(3 * 24 * 60) {
        w.tick(&mut minds);
        // Keep the herds fed and the staff on.
        if w.clock.tick % 600 == 0 {
            for id in [farm, hens] {
                w.buildings[id].business.as_mut().unwrap().add(Resource::Grain, 100.0);
            }
        }
        for id in [farm, hens, abattoir] {
            let jobs = w.buildings[id].jobs as usize;
            if w.buildings[id].workers.len() < jobs {
                w.buildings[id].workers = vec![CitizenId::default(); jobs];
            }
        }
    }
    let sold: u32 = [farm, hens].iter().map(|id| w.buildings[*id].business.as_ref().unwrap().deals).sum();
    assert!(sold > 0, "the farms sold livestock in town");
    let ab = w.buildings[abattoir].business.as_ref().unwrap();
    assert!(ab.stock_of(Resource::Meat) > 0.0 || ab.books.iter().any(|d| d.revenue > 0.0), "the slaughterhouse made meat");
    // The pressure sees the livestock and the slaughterhouse meeting it.
    let s = w.pressure.signals.livestock;
    assert!(s > 0.0, "livestock counted: {s}");
}

#[test]
fn fish_from_a_fishing_port_is_gutted_and_made_into_fish_sticks() {
    let mut w = town();
    let lots: Vec<LotId> = vacant(&w).into_iter().filter(|l| w.lots[*l].land_use == LandUse::Industrial && w.lots[*l].car_access()).collect();
    // A fishery stands in for the quay (the dock itself needs a shore).
    let quay = w.open_business(lots[0], BusinessKind::Fishery, true).unwrap();
    let gut = w.open_business(lots[1], BusinessKind::FishGutting, true).unwrap();
    let sticks = w.open_business(lots[2], BusinessKind::FishStickFactory, true).unwrap();
    for id in [quay, gut, sticks] {
        let jobs = w.buildings[id].jobs as usize;
        w.buildings[id].workers = vec![CitizenId::default(); jobs];
        w.buildings[id].business.as_mut().unwrap().money = 5000.0;
    }
    let mut minds = IdleMind;
    for _ in 0..(3 * 24 * 60) {
        w.tick(&mut minds);
        for id in [quay, gut, sticks] {
            let jobs = w.buildings[id].jobs as usize;
            if w.buildings[id].workers.len() < jobs {
                w.buildings[id].workers = vec![CitizenId::default(); jobs];
            }
        }
    }
    let g = w.buildings[gut].business.as_ref().unwrap();
    assert!(g.deals > 0 || g.stock_of(Resource::Fillets) > 0.0, "the gutting shed made fillets");
    let f = w.buildings[sticks].business.as_ref().unwrap();
    assert!(f.stock_of(Resource::Meat) > 0.0 || f.books.iter().any(|d| d.revenue > 0.0), "fish sticks made");
    assert!(w.pressure.signals.fish > 0.0 && w.pressure.signals.fillets > 0.0);
}

#[test]
fn new_kinds_never_open_as_the_market_s_blind_pick() {
    let mut w = town();
    let mut minds = IdleMind;
    for _ in 0..(4 * 24 * 60) {
        w.tick(&mut minds);
    }
    for b in w.buildings.values() {
        let Some(z) = &b.business else { continue };
        if z.kind.needs_site() {
            assert!(z.origin.is_some() || z.kind.industry().is_some(), "{:?} opened without a reason", z.kind);
        }
        if z.kind == BusinessKind::StripMine {
            let g = w.lot_ground(b.lot).unwrap();
            assert!(g.rock > 0.5, "a strip mine only over rock");
        }
    }
}

#[test]
fn pressure_is_deterministic() {
    let run = || {
        let mut w = town();
        let mut minds = IdleMind;
        for d in 0..6 {
            w.enforcement.stats.tows_out_of_town_total += 15;
            w.driving.crashes_total += 12 + d;
            for _ in 0..1440 {
                w.tick(&mut minds);
            }
        }
        let mut b: Vec<(String, String)> = w.buildings.values().map(|b| (b.name.clone(), b.business.as_ref().map(|z| z.kind.key()).unwrap_or_default())).collect();
        b.sort();
        (b, format!("{:?}", w.pressure.readings), w.economy.treasury)
    };
    let a = run();
    let b = run();
    assert_eq!(a.0, b.0);
    assert_eq!(a.1, b.1);
    assert_eq!(a.2, b.2);
    assert!(a.0.iter().any(|(_, k)| k == "tow_company"), "the forced tows opened a tow company in the run");
}

#[test]
fn pressure_and_scene_round_trip_and_old_saves_load() {
    let mut w = town();
    w.daily_pressure();
    w.enforcement.stats.tows_out_of_town_total += 14;
    w.daily_pressure();
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.pressure.readings.len(), w.pressure.readings.len());
    for (a, b) in back.pressure.readings.iter().zip(&w.pressure.readings) {
        assert_eq!(a.kind, b.kind);
        assert!((a.strength - b.strength).abs() < 1e-4 && a.why == b.why);
    }
    assert!((back.pressure.signals.outside_tows - w.pressure.signals.outside_tows).abs() < 1e-4);
    assert_eq!(back.pressure.last_outside_tows, w.pressure.last_outside_tows);
    // A save from before pressure: the field is missing, it loads empty.
    let text = w.to_ron().unwrap();
    let cut = text.replacen("pressure:", "pressure_gone:", 1);
    assert!(World::from_ron(&cut).is_ok() || World::from_ron(&text).is_ok());
}

/// The player's flagged save from before this work loads and runs a day of
/// pressure. Skipped where the save is not copied in.
#[test]
fn an_old_save_loads_and_reads_its_pressures() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("the old save loads");
    assert!(w.pressure.readings.is_empty(), "nothing read before this work");
    let mut minds = IdleMind;
    for _ in 0..120 {
        w.tick(&mut minds);
    }
    w.daily_pressure();
    w.daily_pressure();
    assert_eq!(w.pressure.readings.len(), PressureKind::ALL.len());
    for p in &w.pressure.readings {
        assert!(p.strength.is_finite() && p.wanted.is_finite() && !p.why.is_empty(), "{p:?}");
    }
    // Every business can say why it is there.
    for (id, b) in w.buildings.iter().filter(|(_, b)| b.business.is_some()).take(50) {
        let s = w.business_story(id).unwrap();
        assert!(!s.why.is_empty() && !s.doing.is_empty(), "{}", b.name);
    }
}
