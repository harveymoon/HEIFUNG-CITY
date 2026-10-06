//! Tests for `world/industry.rs`: plant and industry land value, the
//! industry choice, the player's bias, contamination and fleet lots.

use super::*;
use crate::land_value::LandValue;
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn flat() -> World {
    World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::default() })
}

/// A street along y = 0 and an industrial strip north of it; lots sorted
/// west to east.
fn strip() -> (World, Vec<LotId>) {
    let mut w = flat();
    // A loop, so lorries can turn round.
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(400.0, 0.0), Vec2::new(400.0, 120.0), Vec2::new(-400.0, 120.0), Vec2::new(-400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-300.0, 8.0), Vec2::new(300.0, 8.0), Vec2::new(300.0, 60.0), Vec2::new(-300.0, 60.0)], LandUse::Industrial);
    let mut lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
    lots.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
    (w, lots)
}

fn open_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// An open building on `lot` hosting `kind`.
fn make(w: &mut World, lot: LotId, kind: BusinessKind) -> BuildingId {
    if w.lots[lot].building.is_none() {
        w.build_on(lot);
    }
    let id = w.lots[lot].building.unwrap();
    w.finish_construction(id);
    let jobs = w.buildings[id].jobs;
    w.buildings[id].business = Some(Business::new(kind, jobs));
    id
}

#[test]
fn plants_cost_land_value_coal_first_and_solar_least() {
    let mut order = PowerPlantKind::ALL.to_vec();
    order.sort_by(|a, b| b.pollution_load().total_cmp(&a.pollution_load()));
    assert_eq!(order[0], PowerPlantKind::Coal);
    assert_eq!(order[1], PowerPlantKind::Nuclear);
    assert_eq!(order[2], PowerPlantKind::Geothermal);
    assert_eq!(*order.last().unwrap(), PowerPlantKind::Solar);
    // A home 70 m from each kind of plant, on otherwise plain land.
    let home_value = |k: PowerPlantKind| {
        let mut lv = LandValue::new(400.0, 32.0);
        let mut b = LandValueBuilder::new(&lv);
        b.splat(Vec2::ZERO, 400.0, 0.3);
        let (r, wgt) = k.pollution();
        b.splat_nuisance(Vec2::ZERO, r, -wgt);
        b.finish(0.25, &mut lv);
        lv.for_use(Vec2::new(70.0, 0.0), LandUse::Residential)
    };
    let v: Vec<f32> = order.iter().map(|k| home_value(*k)).collect();
    for i in 1..v.len() {
        assert!(v[i] >= v[i - 1] - 1e-4, "{order:?} {v:?}");
    }
    assert!(v[0] < v[v.len() - 1] - 0.15, "coal is far worse than solar: {v:?}");
    assert!(PowerPlantKind::ALL.iter().all(|k| k.pollution().1 < 0.0), "every plant costs something");
}

#[test]
fn homes_by_a_coal_plant_suffer_more_than_industry() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
    let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
    w.update_land_value();
    let at = w.lots[big].center;
    let res = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).min_by(|a, b| a.1.center.distance(at).total_cmp(&b.1.center.distance(at))).map(|(id, _)| id).unwrap();
    let home_before = w.lot_value(res);
    w.economy.treasury = 100_000.0;
    w.build_power(big, PowerPlantKind::Coal).unwrap();
    w.update_land_value();
    let p = w.lots[res].center;
    let home = w.lot_value(res);
    let works = w.land_value.for_use(p, LandUse::Industrial);
    assert!(home < home_before - 0.1, "the plant costs the homes: {home_before} -> {home}");
    assert!(works > home + 0.05, "industry on the same spot minds less: {works} vs {home}");
}

/// The dry, gentle 160 m square near the middle of the map with the most
/// of `score`; returns its south-west corner.
fn best_square(w: &World, score: impl Fn(&GroundSample) -> f32) -> Vec2 {
    let mut best = (f32::NEG_INFINITY, Vec2::ZERO);
    for iy in -12..=12 {
        for ix in -12..=12 {
            let o = Vec2::new(ix as f32 * 50.0, iy as f32 * 50.0);
            let pts: Vec<Vec2> = (0..5).flat_map(|a| (0..5).map(move |b| (a, b))).map(|(a, b)| o + Vec2::new(a as f32 * 40.0, b as f32 * 15.0 - 45.0)).collect();
            if pts.iter().any(|p| w.landscape.water_distance(*p) < 50.0) || w.landscape.slope_over(&pts) > 0.12 {
                continue;
            }
            let s: f32 = pts.iter().map(|p| score(&w.ground_at(*p))).sum::<f32>() / pts.len() as f32;
            if s > best.0 {
                best = (s, o);
            }
        }
    }
    best.1
}

/// A street and a strip of industrial zone at `o`; its vacant lots.
fn zone_at(w: &mut World, o: Vec2) -> Vec<LotId> {
    w.add_road(&[o + Vec2::new(-20.0, -50.0), o + Vec2::new(180.0, -50.0)], 1, 1);
    w.add_zone(vec![o + Vec2::new(0.0, -42.0), o + Vec2::new(160.0, -42.0), o + Vec2::new(160.0, 20.0), o + Vec2::new(0.0, 20.0)], LandUse::Industrial);
    w.lots
        .iter()
        .filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none())
        .filter(|(_, l)| l.center.distance(o + Vec2::new(80.0, -10.0)) < 120.0)
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn oil_ground_draws_wells_and_good_soil_draws_farms() {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::Plains, 7), ..WorldConfig::default() });
    let oil = best_square(&w, |g| g.oil);
    let lots = zone_at(&mut w, oil);
    assert!(!lots.is_empty());
    let oily: Vec<LotId> = lots.iter().copied().filter(|l| w.lot_ground(*l).unwrap().oil > 0.6).collect();
    assert!(!oily.is_empty(), "the map has an oil patch near {oil}");
    for l in &oily {
        let wt = w.industry_weights(*l).unwrap();
        assert_eq!(industry::favourite(&wt), IndustryKind::OilExtraction, "{:?} {wt:?}", w.lot_ground(*l));
        let share = wt[IndustryKind::OilExtraction.index()] / wt.iter().sum::<f32>();
        assert!(share > 0.6, "{share}");
    }
    let fertile = best_square(&w, |g| g.fertility - g.oil * 2.0);
    let lots = zone_at(&mut w, fertile);
    let good: Vec<LotId> = lots.iter().copied().filter(|l| w.lot_ground(*l).is_some_and(|g| g.fertility > 0.6 && g.oil < 0.1)).collect();
    assert!(!good.is_empty(), "the map has good soil near {fertile}");
    for l in &good {
        assert_eq!(industry::favourite(&w.industry_weights(*l).unwrap()), IndustryKind::Farming);
    }
}

#[test]
fn farms_keep_away_from_oil_wells() {
    let (mut w, lots) = strip();
    let mid = lots.len() / 2;
    let alone = industry::weights(&w.industry_site(lots[mid]).unwrap())[IndustryKind::Farming.index()];
    for l in [lots[mid - 1], lots[mid + 1]] {
        make(&mut w, l, BusinessKind::OilWell);
    }
    let site = w.industry_site(lots[mid]).unwrap();
    assert!(site.neighbours.iter().any(|(k, _)| *k == IndustryKind::OilExtraction));
    let beside = industry::weights(&site)[IndustryKind::Farming.index()];
    assert!(beside < alone * 0.4, "farming beside wells {beside} vs alone {alone}");
}

#[test]
fn the_players_bias_overrides_the_ground_and_retools() {
    let (mut w, lots) = strip();
    // Half the strip grows first, by the ground.
    for l in &lots[..lots.len() / 2] {
        w.build_on(*l);
    }
    open_all(&mut w);
    let area = vec![Vec2::new(-310.0, 0.0), Vec2::new(310.0, 0.0), Vec2::new(310.0, 70.0), Vec2::new(-310.0, 70.0)];
    let natural = lots[..lots.len() / 2].iter().filter(|l| w.lots[**l].building.and_then(|b| w.industry_of(b)) != Some(IndustryKind::Logistics)).count();
    let retooled = w.set_industry_bias(IndustryArea::Polygon(area.clone()), Some(IndustryKind::Logistics)).unwrap();
    assert_eq!(retooled, natural);
    for l in &lots[lots.len() / 2..] {
        w.build_on(*l);
    }
    for l in &lots {
        let b = w.lots[*l].building.unwrap();
        assert_eq!(w.industry_of(b), Some(IndustryKind::Logistics), "every building in the area is logistics");
    }
    assert_eq!(w.industry_bias_at(w.lots[lots[0]].center), Some(Some(IndustryKind::Logistics)));
    assert_eq!(industry::favourite(&w.industry_weights(lots[0]).unwrap()), IndustryKind::Logistics);
    // Clearing hands the ground back; a single lot can be set too.
    w.set_industry_bias(IndustryArea::Polygon(area), None).unwrap();
    assert_eq!(w.industry_bias_at(w.lots[lots[0]].center), Some(None));
    w.set_industry_bias(IndustryArea::Lot(lots[1]), Some(IndustryKind::Tech)).unwrap();
    assert_eq!(w.industry_of(w.lots[lots[1]].building.unwrap()), Some(IndustryKind::Tech));
    assert!(w.set_industry_bias(IndustryArea::Polygon(vec![Vec2::ZERO]), None).is_err());
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.industry_bias, w.industry_bias);
}

#[test]
fn oil_wells_foul_the_ground_which_costs_value_and_slowly_recovers() {
    let (mut w, lots) = strip();
    let well = make(&mut w, lots[lots.len() / 2], BusinessKind::OilWell);
    let jobs = w.buildings[well].jobs as usize;
    w.buildings[well].workers = vec![CitizenId::default(); jobs];
    let at = w.lots[w.buildings[well].lot].center;
    w.update_land_value();
    let before = w.land_value.for_use(at, LandUse::Industrial);
    for _ in 0..24 * 10 {
        w.industry_hourly();
    }
    let dirty = w.contamination.at(at);
    assert!(dirty > 0.5, "ten days of pumping: {dirty}");
    w.update_land_value();
    let after = w.land_value.for_use(at, LandUse::Industrial);
    assert!(after < before - 0.1, "{before} -> {after}");
    let other = lots[lots.len() / 2 + 1];
    assert!(w.industry_site(other).unwrap().contamination > 0.2, "the next lot is fouled too");
    // The well closes; the ground recovers over weeks, not hours.
    w.buildings[well].business.as_mut().unwrap().closed = true;
    w.industry_daily();
    let a_day = w.contamination.at(at);
    assert!(a_day > dirty * 0.85, "a day later still fouled: {a_day}");
    for _ in 0..60 {
        w.industry_daily();
    }
    assert!(w.contamination.at(at) < dirty * 0.25, "two months later mostly recovered: {}", w.contamination.at(at));
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.contamination, w.contamination);
}

#[test]
fn idle_trucks_park_at_a_fleet_lot_and_go_out_from_it() {
    let (mut w, lots) = strip();
    assert!(lots.len() >= 6);
    let fleet = make(&mut w, lots[0], BusinessKind::FleetLot);
    let seller = make(&mut w, lots[1], BusinessKind::GrainFarm);
    let buyer = make(&mut w, lots[lots.len() - 1], BusinessKind::Mill);
    assert!(w.fleet_capacity(fleet) >= 4);
    // A delivery lands: the lorry heads for the fleet lot instead of vanishing.
    w.truck_arrived(Cargo { from: seller, to: buyer, resource: Resource::Grain, amount: 5.0, price: 9.0 });
    assert_eq!(w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Grain), 5.0, "delivered first");
    let (a, b) = (w.truck_stop(buyer).unwrap(), w.truck_stop(fleet).unwrap());
    assert_eq!(w.fleet_parked(fleet), 0, "not parked on the spot: {a:?} {b:?} {:?} {}", w.traffic.net.find_route(a.0, a.1, b.0, b.1), w.traffic.net.lanes.len());
    let returning = w.traffic.cars.values().filter(|c| c.cargo.as_ref().is_some_and(|k| is_return_trip(k) && k.to == fleet)).count();
    assert_eq!(returning, 1, "one empty lorry on its way to park");
    let mut minds = IdleMind;
    for _ in 0..600 {
        w.tick(&mut minds);
        if w.fleet_parked(fleet) > 0 {
            break;
        }
    }
    assert_eq!(w.fleet_parked(fleet), 1, "it parked");
    // The seller next door sends its next load out in that lorry.
    assert_eq!(w.fleet_origin(seller), fleet);
    assert_eq!(w.fleet_parked(fleet), 0);
    assert_eq!(w.fleet_origin(seller), seller, "no lorry left: the seller's own");
    // A full lot turns lorries back to their business instead.
    let cap = w.fleet_capacity(fleet);
    w.buildings[fleet].business.as_mut().unwrap().fleet_trucks = cap;
    assert!(w.fleet_lot_with_room(w.lots[lots[1]].center, FLEET_REACH).is_none());
    w.truck_arrived(Cargo { from: seller, to: buyer, resource: Resource::Grain, amount: 1.0, price: 2.0 });
    assert!(w.traffic.cars.values().any(|c| c.cargo.as_ref().is_some_and(|k| is_return_trip(k) && k.to == seller)), "home to the farm");
    // A depot next door can overflow its buses into the lot.
    w.economy.treasury = 100_000.0;
    let depot = w.build_depot(lots[2]).unwrap();
    assert!(w.depot_overflow(depot) >= 2);
}

#[test]
fn a_river_bank_draws_fishing() {
    let w = World::new(WorldConfig::default());
    let mut bank = None;
    'scan: for iy in -40..40 {
        for ix in -40..40 {
            let p = Vec2::new(ix as f32 * 20.0, iy as f32 * 20.0);
            let d = w.landscape.water_distance(p);
            if d > 8.0 && d < 25.0 {
                bank = Some(p);
                break 'scan;
            }
        }
    }
    let bank = bank.expect("the river valley has a river");
    let g = w.ground_at(bank);
    assert!(g.water > 0.8, "{g:?}");
    let site = IndustrySite { ground: g, land_value: 0.3, ..Default::default() };
    assert!(industry::weights(&site)[IndustryKind::Fishing.index()] > 1.0);
    let inland = IndustrySite { ground: w.ground_at(Vec2::new(0.0, 0.0)), land_value: 0.3, ..Default::default() };
    assert!(industry::weights(&inland)[IndustryKind::Fishing.index()] < 0.5);
}

/// Diagnostic, not a check: a town with plenty of industrial land run for
/// six days, printing how each industry kind fares.
#[test]
#[ignore]
fn industry_mix_diagnostic() {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 7), ..WorldConfig::default() });
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        w.add_road(&[Vec2::new(-280.0, c), Vec2::new(280.0, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -280.0), Vec2::new(c, 280.0)], 1, 1);
    }
    let cell = |cx: f32, cy: f32| vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)];
    for (x, y) in [(-210.0, -210.0), (-70.0, -210.0), (70.0, -210.0), (210.0, -210.0)] {
        w.add_zone(cell(x, y), LandUse::Industrial);
    }
    for (x, y) in [(-210.0, -70.0), (-70.0, -70.0), (70.0, 70.0), (210.0, 70.0), (-210.0, 210.0), (-70.0, 210.0)] {
        w.add_zone(cell(x, y), LandUse::Residential);
    }
    for (x, y) in [(70.0, -70.0), (210.0, -70.0), (-210.0, 70.0)] {
        w.add_zone(cell(x, y), LandUse::Commercial);
    }
    let mut minds = IdleMind;
    let mut peak_returning = 0;
    let mut deals = 0;
    for t in 0..(6 * 24 * 60) {
        w.tick(&mut minds);
        peak_returning = peak_returning.max(w.traffic.cars.values().filter(|c| c.cargo.as_ref().is_some_and(is_return_trip)).count());
        if t % 1440 == 1430 {
            deals += w.economy.market.deals_today;
        }
    }
    println!("peak empty lorries heading home or to a lot: {peak_returning}; local deals {deals}");
    let mut rows: std::collections::BTreeMap<&str, (u32, u32, f32)> = Default::default();
    for b in w.buildings.values() {
        let Some(z) = &b.business else { continue };
        let e = rows.entry(z.kind.label()).or_default();
        e.0 += 1;
        e.1 += z.closed as u32;
        e.2 += z.money;
    }
    for (k, (n, closed, money)) in rows {
        println!("{k:>22}: {n:3} ({closed} closed) mean money {:.0}", money / n as f32);
    }
    println!("contamination cells {} max {:.2}; trucks {} returning {}", w.contamination.cells.len(), w.contamination.max(), w.traffic.cars.values().filter(|c| c.cargo.is_some()).count(), w.traffic.cars.values().filter(|c| c.cargo.as_ref().is_some_and(is_return_trip)).count());
}
