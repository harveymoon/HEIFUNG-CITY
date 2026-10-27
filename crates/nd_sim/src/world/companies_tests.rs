//! [companies] Tests of local companies: builders founded by residents in
//! place of the out-of-town developers, an old save's developers selling up
//! to them, growing up and merging, shops and yards growing, fleet
//! liveries, a corp's specialty and the company directory's rows.

use super::*;
use crate::bank::Account;
use crate::companies::*;
use crate::estate::{OwnerId, OwnerKind};
use crate::vehicle::Livery;

/// The committed legacy town, loaded.
fn legacy() -> World {
    World::from_ron(&super::legacy_save_text()).expect("the legacy town loads")
}

/// Ordinary buildings nobody owns in the legacy town, homes first, the
/// first `n`.
fn homes(w: &World, n: usize) -> Vec<BuildingId> {
    let free = |b: &Building| b.plot.owner.is_none() && b.is_grown() && !b.is_special() && b.land_use != LandUse::Park;
    let mut v: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| free(b) && b.homes > 0).map(|(id, _)| id).collect();
    v.extend(w.buildings.iter().filter(|(_, b)| free(b) && b.homes == 0).map(|(id, _)| id));
    v.truncate(n);
    v
}

/// A builder buys where the town presses: a resident who lives near the
/// site founds one, named after them, holding that one building; the next
/// site near it goes to the same firm (three times in four), and its
/// founder remembers starting it.
#[test]
fn a_resident_founds_the_builder_that_buys_the_first_block() {
    let mut w = legacy();
    let hs = homes(&w, 2);
    let lot = w.buildings[hs[0]].lot;
    let mut rng = crate::rng_for(7, 1);
    let o = w.builder_for(lot, &mut rng);
    let OwnerKind::Builder { founder } = w.estate.owners[o].kind else { panic!("a builder: {:?}", w.estate.owners[o].kind) };
    let c = w.citizens.get(founder).expect("founded by a resident in town");
    let surname = c.name.rsplit(' ').next().unwrap();
    assert!(w.estate.owners[o].name.starts_with(surname), "named after its founder: {} / {}", w.estate.owners[o].name, c.name);
    assert!(w.estate.owners[o].history.first().is_some_and(|n| n.text.contains(&c.name)), "its history starts with its founding");
    assert!(c.memories.iter().any(|m| m.text.contains(&w.estate.owners[o].name)), "the founder remembers");
    w.give_to(hs[0], o, 0.0);
    // The town has room for more builders, but a site next to this one's
    // holdings goes to it most of the time.
    let near = (0..8).filter(|k| {
        let mut r = crate::rng_for(11, *k);
        let mut w2 = w.clone();
        w2.builder_for(lot, &mut r) == o
    });
    assert!(near.count() >= 4, "the nearest builder wins most sites by its holdings");
    assert!(!w.estate.owners.values().any(|o| o.kind == OwnerKind::Developer), "nobody from out of town");
}

/// [companies] An old save's developer (an owner record of the old kind,
/// saved without the new fields) loads, stops buying, and sells its
/// buildings to local builders `RETIRE_PER_DAY` a day until it owns nothing
/// and leaves; the rent from then on goes to the builders, and the ledger
/// balances throughout.
#[test]
fn an_old_saves_developers_sell_up_to_local_builders() {
    let mut w = legacy();
    let hs = homes(&w, RETIRE_PER_DAY + 3);
    assert_eq!(hs.len(), RETIRE_PER_DAY + 3, "enough homes in the fixture");
    let dev = w.new_owner("Meridian Estates".into(), OwnerKind::Developer);
    for h in &hs {
        w.give_to(*h, dev, 0.0);
    }
    w.transfer64(Account::Outside, Account::Owner(dev), 5_000.0, Kind::Investment);
    // Saved, and the new fields stripped as an older build would have
    // written it.
    let text: String = w.to_ron().unwrap().lines().filter(|l| !matches!(l.trim(), "livery: None," | "history: []," | "grown: false," | "founder: None,")).map(|l| format!("{l}
")).collect();
    assert!(!text.contains("grown:"), "no new fields left");
    assert!(text.contains("Developer"), "the developer is in the save");
    let mut w = World::from_ron(&text).expect("an old save with developers loads");
    let dev = w.estate.owners.iter().find(|(_, o)| o.kind == OwnerKind::Developer).map(|(id, _)| id).expect("still there");
    assert_eq!(w.estate.owners[dev].properties.len(), hs.len());
    assert_eq!(w.ledger_sum(), 0);
    w.daily_companies();
    assert_eq!(w.estate.owners[dev].properties.len(), 3, "sold {RETIRE_PER_DAY} on the first day");
    let buyers: Vec<OwnerId> = hs.iter().filter_map(|h| w.buildings[*h].plot.owner).filter(|o| *o != dev).collect();
    assert_eq!(buyers.len(), RETIRE_PER_DAY);
    for b in &buyers {
        let o = &w.estate.owners[*b];
        assert!(matches!(o.kind, OwnerKind::Builder { founder } if w.citizens.contains_key(founder)), "bought by a local builder founded by a resident: {}", o.name);
        assert!(o.history.iter().any(|n| n.text.contains("Meridian Estates")), "its history says who it bought from");
    }
    assert_eq!(w.ledger_sum(), 0, "the sales balance");
    w.daily_companies();
    assert!(w.estate.owners[dev].properties.is_empty(), "sold the rest on the second day");
    w.prune_estate();
    assert!(!w.estate.owners.values().any(|o| o.kind == OwnerKind::Developer), "and left town");
    // Rent to a building it sold goes to the local builder now.
    let (tenant, home) = w.households.iter().find_map(|(id, h)| h.home.filter(|b| hs.contains(b)).map(|b| (id, b))).expect("a tenant in one of the homes");
    let owner = w.buildings[home].plot.owner.unwrap();
    assert_eq!(w.landlord_of(home), Account::Owner(owner));
    let before = w.estate.owners[owner].money;
    w.collect_rent(Account::Household(tenant), home, 100.0, Kind::Rent);
    assert!(w.estate.owners[owner].money > before, "the builder collects the rent");
    assert_eq!(w.ledger_sum(), 0);
}

/// [companies] A builder past `BUILDER_GROWN` buildings takes a grown-up
/// name once (and says so); a big builder buys out a small rival nearby,
/// its buildings and its books.
#[test]
fn builders_grow_up_and_merge() {
    let mut w = legacy();
    let hs = homes(&w, BUILDER_GROWN + 1);
    let at = w.buildings[hs[0]].lot;
    let big = w.found_builder(w.lots[at].center);
    for h in &hs[..BUILDER_GROWN] {
        w.give_to(*h, big, 0.0);
    }
    let old = w.estate.owners[big].name.clone();
    w.rename_grown_builders();
    let o = &w.estate.owners[big];
    assert!(o.grown && o.name != old && o.name.starts_with(surname_of(&old)), "{old} -> {}", o.name);
    assert!(o.history.last().is_some_and(|n| n.text.contains(&old)));
    let name = o.name.clone();
    w.rename_grown_builders();
    assert_eq!(w.estate.owners[big].name, name, "only once");
    // A small rival next door.
    let small = w.found_builder(w.lots[at].center);
    assert_ne!(small, big);
    w.give_to(hs[BUILDER_GROWN], small, 0.0);
    w.transfer64(Account::Outside, Account::Owner(small), 300.0, Kind::Investment);
    let big_money = w.estate.owners[big].money;
    // The daily roll decides the day; try days until one merges.
    for d in 0..40u64 {
        w.clock.tick = d * 1440;
        w.merge_builders();
        if w.estate.owners[small].properties.is_empty() {
            break;
        }
    }
    assert!(w.estate.owners[small].properties.is_empty(), "the small one was bought out");
    assert_eq!(w.buildings[hs[BUILDER_GROWN]].plot.owner, Some(big));
    assert!(w.estate.owners[big].money != big_money, "its books moved with it");
    assert!(w.estate.owners[big].history.iter().any(|n| n.text.contains("bought out")));
    assert_eq!(w.ledger_sum(), 0);
}

/// [companies] Every local company with vehicles gets a colour of its own:
/// two taxi firms near each other never share one, the colour sits on the
/// owner record when there is one, and a cab is drawn in it over the cab
/// livery (`Livery::Fleet`, base `Taxi`).
#[test]
fn fleet_companies_get_their_own_livery() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    let sq = |x0: f32| vec![Vec2::new(x0, 10.0), Vec2::new(x0 + 180.0, 10.0), Vec2::new(x0 + 180.0, 190.0), Vec2::new(x0, 190.0)];
    w.add_zone(sq(-650.0), LandUse::Commercial);
    w.add_zone(sq(-400.0), LandUse::Commercial);
    let yard = |w: &World, x: std::ops::Range<f32>| w.lots.iter().filter(|(id, l)| l.land_use == LandUse::Commercial && l.car_segment.is_some() && l.area() >= crate::taxi::YARD_MIN_AREA && x.contains(&l.center.x) && w.lot_free_for(*id, SiteRule::STREET)).map(|(id, _)| id).next();
    let lots = [yard(&w, -700.0..-460.0).expect("a yard lot"), yard(&w, -420.0..-200.0).expect("another")];
    let a = w.build_taxi_company(lots[0], "Ama Cabs".into()).expect("a yard");
    let b = w.build_taxi_company(lots[1], "Bea Cabs".into()).expect("another");
    let firm = w.new_owner("Ama Cabs".into(), OwnerKind::Firm { kind: BusinessKind::TaxiCompany });
    w.give_to(a, firm, 0.0);
    w.assign_liveries();
    let (ca, cb) = (w.fleet_colour(a).expect("a colour"), w.fleet_colour(b).expect("a colour"));
    assert_ne!(ca, cb, "rivals in different colours");
    let o = w.buildings[a].plot.owner.unwrap();
    assert_eq!(w.estate.owners[o].livery, Some(ca), "on the firm's record");
    assert_eq!(w.buildings[b].business.as_ref().unwrap().livery, Some(cb), "on the business when it has no owner record");
    let cab = w.taxis.fleet(a).next().unwrap().1.model;
    assert_eq!(cab.livery, Livery::Fleet(Livery::Taxi.code(), ca), "the cabs repainted");
    assert_eq!(cab.livery.base(), Livery::Taxi, "drawn as a cab");
    assert_eq!(w.fleet_livery(a, Livery::Taxi), Some(cab.livery));
    // Stable: a second pass changes nothing.
    w.assign_liveries();
    assert_eq!(w.fleet_colour(a), Some(ca));
}

/// [companies] A grocery doing well buys the plot next door first, then
/// opens another store across town once its site is full or has no
/// neighbour for sale (`shop_growth`); a corp grown out of it has the
/// groceries specialty and builds farms, yards and stores.
#[test]
fn a_shop_grows_next_door_then_across_town_and_its_corp_knows_food() {
    let w = legacy();
    let shop = w.buildings.iter().find(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Grocery) && !b.is_special()).map(|(id, _)| id);
    let Some(shop) = shop else { return };
    let step = w.shop_growth(shop).expect("a grocery grows");
    let b = &w.buildings[shop];
    let lu = w.lots[b.lot].land_use;
    let next_door = !w.plots_next_to(&b.plot.lots(b.lot), lu, b.floors + 1, b.plot.owner).is_empty();
    assert_eq!(step, if next_door { ShopGrowth::Neighbour } else { ShopGrowth::Branch });
    assert_ne!(step, ShopGrowth::Mall, "malls are the next round's");
    let s = Specialty::of_origin(BusinessKind::Grocery);
    assert_eq!(s, Specialty::Food);
    assert!(s.wants(BusinessKind::VertFarm) && s.wants(BusinessKind::Grocery) && !s.wants(BusinessKind::Hotel));
    // A tow company is a fleet yard: it grows its yard before a depot.
    assert!(yard_trade(BusinessKind::TowYard) && runs_fleet(BusinessKind::TowYard) && runs_fleet(BusinessKind::TaxiCompany));
}

/// [companies] The company directory lists every owner with what it is,
/// its properties and their value, biggest first; "Biggest companies"
/// leaves families in their own homes out and ranks by size, not takings.
#[test]
fn the_directory_lists_every_owner_by_size() {
    let mut w = legacy();
    let hs = homes(&w, 6);
    let builder = w.found_builder(w.building_lot(hs[0]).unwrap().center);
    for h in &hs[..5] {
        w.give_to(*h, builder, 0.0);
    }
    let hid = w.households.keys().next().unwrap();
    let family = w.new_owner("the Test family".into(), OwnerKind::Household { household: hid });
    w.give_to(hs[5], family, 0.0);
    let rows = w.company_summaries();
    let b = rows.iter().find(|r| r.owner == builder).expect("the builder is listed");
    assert_eq!(b.kind_key, "builder");
    assert_eq!(b.properties, 5);
    assert!(b.value > 0.0 && b.founder.is_some() && b.flagship.is_some());
    assert!(rows.iter().any(|r| r.owner == family && r.kind_key == "family"), "every owner, families too");
    for pair in rows.windows(2) {
        assert!(pair[0].size >= pair[1].size, "biggest first");
    }
    let big = w.biggest_companies(10);
    assert!(big.iter().any(|r| r.owner == builder));
    assert!(!big.iter().any(|r| r.kind_key == "family"), "families in their own house are not companies");
}

/// [companies] A mega-corp grown out of a grocery builds what its roots
/// know on the land it buys: grocery stores (or bakeries) on commercial
/// plots, farms and distribution yards on industrial ones.
#[test]
fn a_corp_builds_by_its_specialty() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    let rect = |x0: f32, y0: f32, wd: f32, h: f32| vec![Vec2::new(x0, y0), Vec2::new(x0 + wd, y0), Vec2::new(x0 + wd, y0 + h), Vec2::new(x0, y0 + h)];
    w.add_zone(rect(-650.0, 8.0, 600.0, 60.0), LandUse::Commercial);
    w.add_zone(rect(100.0, 8.0, 500.0, 120.0), LandUse::Industrial);
    let free = |w: &World, lu: LandUse| -> Vec<LotId> { w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.car_access() && l.building.is_none()).map(|(id, _)| id).collect() };
    let shop = w.build_on_as(free(&w, LandUse::Commercial)[0], Some(BusinessKind::Grocery)).expect("a grocery");
    w.finish_construction(shop);
    let ci = w.promote_to_corp(shop).expect("promoted");
    assert_eq!(w.corp_specialty(ci), Specialty::Food);
    let c = free(&w, LandUse::Commercial).into_iter().find(|l| w.lots[*l].building.is_none()).expect("a commercial plot");
    let b = w.corp_build_on(c, ci).expect("built");
    let k = w.buildings[b].business.as_ref().unwrap().kind;
    assert!(Specialty::Food.builds(LandUse::Commercial).contains(&k), "a food trade on commercial land: {k:?}");
    let i = free(&w, LandUse::Industrial)[0];
    let b = w.corp_build_on(i, ci).expect("built");
    let k = w.buildings[b].business.as_ref().unwrap().kind;
    assert!(Specialty::Food.builds(LandUse::Industrial).contains(&k), "a farm or a distribution yard on industrial land: {k:?}");
}
