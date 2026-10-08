//! [econ-followups] Bought-out businesses moving, parking that answers
//! circling drivers, the well-off buying property, industry lifting cheap
//! land, and the wind small-lot exception on unzoned ground.

use super::*;
use crate::estate::OwnerKind;
use crate::mind::IdleMind;
use crate::power::PowerPlantKind;
use crate::services::Placeable;

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1e6;
    w
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn built(w: &mut World, lot: LotId) -> Option<BuildingId> {
    w.build_on(lot);
    let b = w.lots[lot].building?;
    w.finish_construction(b);
    Some(b)
}

fn lots_of(w: &World, lu: LandUse) -> Vec<LotId> {
    w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.car_access() && l.building.is_none()).map(|(id, _)| id).collect()
}

/// An avenue with homes to the west and shops to the east, the homes built
/// and lived in, one shop open with staff.
fn high_street() -> (World, BuildingId) {
    let mut w = town();
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(rect(-650.0, 8.0, 500.0, 60.0), LandUse::Residential);
    w.add_zone(rect(50.0, 8.0, 600.0, 60.0), LandUse::Commercial);
    for lot in lots_of(&w, LandUse::Residential) {
        built(&mut w, lot);
    }
    let shop_lot = lots_of(&w, LandUse::Commercial)[0];
    w.build_on_as(shop_lot, Some(BusinessKind::Bakery)).expect("a bakery");
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(shop);
    run(&mut w, 900);
    (w, shop)
}

// ── 4. a bought-out business moves ──────────────────────────────────────

#[test]
fn a_business_bought_out_by_the_city_moves_with_its_owner_and_staff() {
    let (mut w, shop) = high_street();
    let staff = w.buildings[shop].workers.clone();
    assert!(!staff.is_empty(), "the bar hired people");
    let founder = w.citizens.iter().find(|(id, c)| c.can_work() && !staff.contains(id)).map(|(id, _)| id).unwrap();
    w.set_business_owner(shop, founder);
    let (name, lot) = (w.buildings[shop].name.clone(), w.buildings[shop].lot);
    w.buildings[shop].business.as_mut().unwrap().money = 2_000.0;
    let books_before = w.buildings[shop].business.as_ref().unwrap().deals;
    // The city puts a car park on its lot.
    w.build_city_parking(lot, crate::enforcement::LotSize::Small).expect("the city takes the lot");
    assert_eq!(w.estate.moving.len(), 1, "the bar packed up");
    w.relocate_waiting();
    assert!(w.estate.moving.is_empty());
    assert_eq!(w.estate.stats.relocated_total, 1);
    let new = w.buildings.iter().find(|(id, b)| *id != shop && b.name == name && b.business.is_some()).map(|(id, _)| id).expect("the bar opened a site elsewhere");
    let b = &w.buildings[new];
    assert_ne!(b.lot, lot);
    assert!(b.construction.is_some(), "a building site first");
    let z = b.business.as_ref().unwrap();
    assert_eq!(z.kind, BusinessKind::Bakery, "the same business");
    assert_eq!(z.deals, books_before, "with its books");
    assert!(z.money < 2_000.0 && z.money > 2_000.0 - super::relocate::move_cost(w.buildings[new].jobs.max(30)), "it paid the move: {}", z.money);
    let from = z.moved_from.clone().expect("it says where it came from");
    assert!(from.contains("city's new"), "{from}");
    assert_eq!(w.properties_of(founder), vec![new], "the owner owns the new place");
    assert!(w.social.news.iter().any(|n| n.text.contains(&name) && n.text.contains("moving")), "a news line");
    // The new building opens: the old staff still out of work come back.
    w.finish_construction(new);
    let back = staff.iter().filter(|c| w.citizens.get(**c).is_some_and(|c| c.work == Some(new))).count();
    assert!(back > 0, "staff hired back: {back} of {}", staff.len());
    // It survives a save.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.buildings[new].business.as_ref().unwrap().moved_from.as_deref(), Some(from.as_str()));
}

#[test]
fn a_business_that_cannot_afford_the_move_closes_as_before() {
    let (mut w, shop) = high_street();
    let lot = w.buildings[shop].lot;
    let name = w.buildings[shop].name.clone();
    w.buildings[shop].business.as_mut().unwrap().money = 20.0;
    w.build_city_parking(lot, crate::enforcement::LotSize::Small).expect("the city takes the lot");
    assert!(w.estate.moving.is_empty(), "nothing to move with");
    w.relocate_waiting();
    assert!(!w.buildings.iter().any(|(_, b)| b.name == name && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Bakery)));
}

#[test]
fn a_developer_buyout_moves_the_business_and_the_new_building_opens_another() {
    let (mut w, shop) = high_street();
    w.buildings[shop].business.as_mut().unwrap().money = 5_000.0;
    w.buildings[shop].floors = 1;
    w.buildings[shop].built_tick = 0;
    w.buildings[shop].recompute_capacity();
    let name = w.buildings[shop].name.clone();
    let mut rng = crate::rng_for(3, 4);
    // Make the land dear enough that a bigger building pays.
    w.economy.demand[LandUse::Commercial.index()] = 0.9;
    if !w.buy_out_and_rebuild(shop, &mut rng) {
        // The sale did not pay on this ground; the hook is the same as the
        // city's (tested above). Nothing packed without a sale.
        assert!(w.estate.moving.is_empty());
        return;
    }
    assert_eq!(w.estate.moving.len(), 1);
    assert!(w.estate.moving[0].why.contains("rebuild"));
    w.relocate_waiting();
    assert!(w.buildings.iter().any(|(id, b)| id != shop && b.name == name && b.business.as_ref().is_some_and(|z| z.moved_from.is_some())));
}

#[test]
fn a_move_waits_for_a_lot_then_gives_up() {
    let (mut w, shop) = high_street();
    w.buildings[shop].business.as_mut().unwrap().money = 2_000.0;
    // No free commercial lot anywhere.
    for lot in lots_of(&w, LandUse::Commercial) {
        built(&mut w, lot);
    }
    let lot = w.buildings[shop].lot;
    w.build_city_parking(lot, crate::enforcement::LotSize::Small).unwrap();
    w.relocate_waiting();
    assert_eq!(w.estate.moving.len(), 1, "still looking");
    w.clock.tick += super::relocate::MOVE_WAIT_TICKS + 1;
    w.relocate_waiting();
    assert!(w.estate.moving.is_empty());
    assert_eq!(w.estate.stats.relocate_failed_total, 1);
}

// ── 2. parking pressure brings more parking ─────────────────────────────

/// A parking business on the commercial lot nearest `p`.
fn parking_near(w: &mut World, p: Vec2) -> BuildingId {
    let lot = lots_of(w, LandUse::Commercial).into_iter().min_by(|a, b| w.lots[*a].center.distance(p).total_cmp(&w.lots[*b].center.distance(p))).unwrap();
    let id = w.build_parking(lot).expect("a parking lot");
    w.finish_construction(id);
    // Opened a while ago, a modest lot (the town's few cars would cap a big one).
    w.buildings[id].built_tick = 0;
    w.buildings[id].parking_capacity = w.buildings[id].parking_capacity.min(20);
    w.clock.tick = w.clock.tick.max(3 * 1440);
    id
}

#[test]
fn a_full_lot_raises_its_fee_and_an_empty_one_cuts_it_within_bounds() {
    let (mut w, _) = high_street();
    let lot = parking_near(&mut w, Vec2::new(400.0, 30.0));
    let base = w.buildings[lot].parking_price;
    let cap = w.buildings[lot].parking_capacity;
    for _ in 0..30 {
        w.buildings[lot].parking_peak = cap;
        w.reprice_parking();
    }
    let dear = w.buildings[lot].parking_price;
    assert!(dear > base * 1.5 && dear <= base * 2.5 + 1e-3, "{base} -> {dear}");
    assert_eq!(w.buildings[lot].parking_peak, 0, "the day's peak starts again");
    for _ in 0..60 {
        w.buildings[lot].parking_peak = 0;
        w.reprice_parking();
    }
    let cheap = w.buildings[lot].parking_price;
    assert!(cheap < base && cheap >= base * 0.6 - 1e-3, "{base} -> {cheap}");
}

#[test]
fn circling_by_full_lots_draws_a_local_to_open_another_but_not_by_empty_ones() {
    let (mut w, _) = high_street();
    // The trouble is on the high street (the concerns heard nothing else).
    w.concerns = Default::default();
    w.economy.parking_shortage_at = Some(Vec2::new(300.0, 30.0));
    let first = w.parking_trouble_spot().unwrap();
    let full = parking_near(&mut w, first);
    // The trouble between the full lot and the next free ground.
    let here = w.building_lot(full).unwrap().center;
    let next = lots_of(&w, LandUse::Commercial).into_iter().map(|l| w.lots[l].center).min_by(|a, b| a.distance(here).total_cmp(&b.distance(here))).unwrap();
    let at = here.lerp(next, 0.5);
    w.economy.parking_shortage_at = Some(at);
    let rich = w.citizens.iter().find(|(_, c)| c.can_work()).map(|(id, _)| id).unwrap();
    match w.citizens[rich].household {
        Some(h) => w.households[h].money = 5_000.0,
        None => w.citizens[rich].savings = 5_000.0,
    }
    let ready = |w: &mut World| {
        w.economy.parking_street_today = 12;
        w.economy.parking_shortage_at = Some(at);
        let k = w.config.development_interval;
        w.clock.tick = (w.clock.tick / (3 * k) + 1) * 3 * k;
    };
    // The lot near the trouble had room: nobody opens another.
    ready(&mut w);
    w.buildings[full].parking_peak = 0;
    let vacant = lots_of(&w, LandUse::Commercial);
    assert!(!w.maybe_open_parking(&vacant), "drivers circled past free spaces");
    // Full at its busiest: a local opens one nearby.
    w.buildings[full].parking_peak = w.buildings[full].parking_capacity;
    let before: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.is_parking()).map(|(id, _)| id).collect();
    let vacant = lots_of(&w, LandUse::Commercial);
    assert!(w.maybe_open_parking(&vacant), "full lots and circling drivers: another lot");
    let new: Vec<BuildingId> = w.buildings.iter().filter(|(id, b)| b.is_parking() && !before.contains(id)).map(|(id, _)| id).collect();
    assert_eq!(new.len(), 1);
    let new = new[0];
    let d = w.lots[w.buildings[new].lot].center.distance(at);
    assert!(d <= 220.0, "near the trouble: {d} {:?}", w.building_lot(new).map(|l| l.center));
    let owner = w.owner_of(new).map(|(_, o)| o.kind.clone());
    assert!(matches!(owner, Some(OwnerKind::Person { .. }) | Some(OwnerKind::Household { .. })), "a local owns it: {owner:?}");
    assert!(w.buildings[new].business.as_ref().unwrap().money >= super::parking::PARKING_LOT_COST);
    assert!(w.economy.parking_street_today < 12, "the trouble counts as answered");
    // Not again right next to it within two days.
    ready(&mut w);
    w.buildings[new].parking_peak = w.buildings[new].parking_capacity;
    let vacant = lots_of(&w, LandUse::Commercial);
    assert!(!w.maybe_open_parking(&vacant), "no carpet of lots");
}

#[test]
fn nobody_with_the_money_means_no_lot_unless_the_trouble_is_bad() {
    let (mut w, _) = high_street();
    // The trouble is on the high street (the concerns heard nothing else).
    w.concerns = Default::default();
    w.economy.parking_shortage_at = Some(Vec2::new(300.0, 30.0));
    let first = w.parking_trouble_spot().unwrap();
    let full = parking_near(&mut w, first);
    // The trouble between the full lot and the next free ground.
    let here = w.building_lot(full).unwrap().center;
    let next = lots_of(&w, LandUse::Commercial).into_iter().map(|l| w.lots[l].center).min_by(|a, b| a.distance(here).total_cmp(&b.distance(here))).unwrap();
    let at = here.lerp(next, 0.5);
    w.economy.parking_shortage_at = Some(at);
    w.buildings[full].parking_peak = w.buildings[full].parking_capacity;
    for c in w.citizens.values_mut() {
        c.savings = 0.0;
    }
    for h in w.households.values_mut() {
        h.money = 0.0;
    }
    let k = w.config.development_interval;
    w.clock.tick = (w.clock.tick / (3 * k) + 1) * 3 * k;
    w.economy.parking_street_today = 5;
    w.economy.parking_shortage_at = Some(at);
    let vacant = lots_of(&w, LandUse::Commercial);
    assert!(!w.maybe_open_parking(&vacant), "no founder, mild trouble");
    w.economy.parking_street_today = 40;
    assert!(w.maybe_open_parking(&vacant), "an operator from out of town for real trouble");
}

// ── 5. the well-off own several places ──────────────────────────────────

#[test]
fn a_well_off_family_buys_a_business_then_a_second_home() {
    let (mut w, shop) = high_street();
    // The bar pays its way.
    {
        let z = w.buildings[shop].business.as_mut().unwrap();
        z.books = vec![crate::market::DayBook { revenue: 500.0, spend: 200.0 }; 4];
        z.closed = false;
    }
    // Middle blocks to buy.
    for b in w.buildings.values_mut().filter(|b| b.land_use == LandUse::Residential) {
        b.tier = Tier::Mid;
    }
    let hid = w.households.iter().find(|(_, h)| !h.members.is_empty() && h.home.is_some()).map(|(id, _)| id).unwrap();
    w.households[hid].money = 20_000.0;
    let members = w.households[hid].members.clone();
    for m in &members {
        w.citizens[*m].income = 20.0;
    }
    let earner = members[0];
    w.citizens[earner].income = 500.0;
    let income = w.household_income(hid);
    for _ in 0..40 {
        w.invest_rich();
        w.clock.tick += 1440;
        w.households[hid].money = 20_000.0;
    }
    let owned = w.properties_of(earner);
    assert!(owned.contains(&shop), "bought the bar: {owned:?}");
    let homes = owned.iter().filter(|b| w.buildings[**b].business.is_none() && w.buildings[**b].homes > 0).count();
    let blocks: Vec<(u16, Tier, bool, bool)> = w.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Residential).map(|(id, b)| (b.homes, b.tier, b.plot.owner.is_none(), w.buyable(id))).collect();
    assert!(homes >= 1, "and a second home: {owned:?} {blocks:?}");
    assert!(owned.len() <= super::rich::holdings_cap(income), "within what its income carries");
    // Every holding is pinned, and selecting one outlines the rest.
    let pins: Vec<BuildingId> = w.holdings_of(earner).into_iter().map(|(b, _)| b).collect();
    for b in &owned {
        assert!(pins.contains(b));
    }
    let lit = w.owned_with(shop);
    for b in &owned {
        assert!(lit.contains(b), "selecting the bar outlines {b:?}");
    }
    assert!(w.estate.stats.investments_total >= 2);
}

#[test]
fn a_modest_household_buys_nothing() {
    let (mut w, _) = high_street();
    for c in w.citizens.values_mut() {
        c.income = 90.0;
    }
    for h in w.households.values_mut() {
        h.money = 50_000.0;
    }
    for _ in 0..30 {
        w.invest_rich();
        w.clock.tick += 1440;
    }
    assert_eq!(w.estate.stats.investments_total, 0);
}

#[test]
fn spare_cars_stay_in_the_garage_through_a_save_and_go_with_their_owner() {
    let (mut w, _) = high_street();
    let cid = w.citizens.iter().find(|(_, c)| c.home.is_some()).map(|(id, _)| id).unwrap();
    let home = w.citizens[cid].home.unwrap();
    let models = [super::estate::supercar(1), super::estate::supercar(2)];
    let used = w.buildings[home].parking_used;
    w.garage_spares(cid, home, &models);
    assert_eq!(w.spare_cars_of(cid).len(), 2);
    assert_eq!(w.buildings[home].parking_used, used + 2, "in the house's own spaces");
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.spare_cars_of(cid).len(), 2, "a load keeps them");
    // The owner leaves: the garage is sold off.
    w.citizens.remove(cid);
    w.prune_spare_cars();
    assert!(w.vehicles.cars.values().all(|c| c.owner != cid || !c.spare));
}

// ── 3. industry lifts cheap land ────────────────────────────────────────

#[test]
fn working_industry_lifts_cheap_land_a_little() {
    let mut w = town();
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(rect(-150.0, 8.0, 300.0, 70.0), LandUse::Industrial);
    let lot = lots_of(&w, LandUse::Industrial)[0];
    w.build_on_as(lot, Some(BusinessKind::Fab)).expect("a clean works");
    let works = w.lots[lot].building.unwrap();
    w.finish_construction(works);
    // Across the road, where nothing else is.
    let p = w.lots[lot].center + Vec2::new(0.0, -70.0);
    w.buildings[works].business.as_mut().unwrap().closed = true;
    w.update_land_value();
    let closed = w.land_value.at(p);
    w.buildings[works].business.as_mut().unwrap().closed = false;
    w.update_land_value();
    let open = w.land_value.at(p);
    assert!(open > closed, "jobs lift the land: {closed} -> {open}");
    assert!(open - closed <= crate::land_value::MAX_LIFT + 1e-4, "a little: {closed} -> {open}");
}

// ── 6. the wind small-lot exception on unzoned ground ───────────────────

#[test]
fn a_small_unzoned_site_may_join_the_wind_park_next_door() {
    let kind = PowerPlantKind::Wind;
    let mut w = town();
    w.economy.treasury = 5_000_000.0;
    w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    let p = (0..100).map(|k| Vec2::new(-500.0 + k as f32 * 10.0, 12.0)).find(|&p| w.placement_problem_at(Placeable::Plant(kind), p).is_ok()).expect("unzoned ground takes a wind farm");
    let farm = w.place_building(Placeable::Plant(kind), p).expect("placed on unzoned ground");
    let lot = &w.lots[w.buildings[farm].lot];
    let (lo, hi) = lot.polygon.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(a, b), q| (a.min(*q), b.max(*q)));
    // A 12 by 20 m scrap of unzoned ground on its east side, on the avenue:
    // too small for a wind farm alone (240 m² against 400).
    let outline = rect(hi.x, lo.y, 12.0, 20.0_f32.min(hi.y - lo.y));
    let alone = crate::geom::polygon_area(&outline).abs();
    assert!(alone < kind.min_area());
    let site = super::placement::PlacementSite { lot: None, parcels: Vec::new(), outline: outline.clone(), leaves_zone: None };
    assert!(w.small_plant_site_joins(kind, &outline, None).is_some(), "it touches the farm");
    assert_eq!(w.site_problem(Placeable::Plant(kind), &site), None, "joining the park lets it go in");
    // The same scrap away from any farm is refused for its size.
    let lone = rect(hi.x + 200.0, lo.y, 12.0, 20.0_f32.min(hi.y - lo.y));
    let site = super::placement::PlacementSite { lot: None, parcels: Vec::new(), outline: lone, leaves_zone: None };
    let why = w.site_problem(Placeable::Plant(kind), &site).expect("refused alone");
    assert!(why.contains("too small"), "{why}");
}
