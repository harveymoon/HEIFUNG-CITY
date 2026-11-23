//! [r14-builders] Builder companies and the builders' merchant: a site is
//! let to a contractor in town with a crew free, else to a firm from out of
//! town at a markup; the contractor is paid by instalments and buys the
//! site's materials; the merchant restocks and sells to builders and to
//! residents; a builder with no work goes bust and its job is let again.

use super::*;
use crate::buildings::SiteBuilder;
use crate::mind::IdleMind;

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// A street with homes, shops and industry along it.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    w.add_zone(square(-550.0, 10.0, 400.0, 60.0), LandUse::Residential);
    w.add_zone(square(-100.0, 10.0, 250.0, 70.0), LandUse::Commercial);
    w.add_zone(square(200.0, 10.0, 350.0, 90.0), LandUse::Industrial);
    tick(&mut w, 2);
    w
}

fn free_lot(w: &World, lu: LandUse) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.building.is_none() && l.car_access()).map(|(id, _)| id).next().expect("a free lot")
}

/// A business of `kind` open on a free lot of `lu`, with `staff` people
/// on its books.
fn open(w: &mut World, lu: LandUse, kind: BusinessKind, staff: usize) -> BuildingId {
    let lot = free_lot(w, lu);
    let id = w.open_business(lot, kind, true).expect("opens");
    w.buildings[id].jobs = w.buildings[id].jobs.max(staff as u16);
    for k in 0..staff {
        let at = w.lots[lot].center;
        let c = w.citizens.insert(Citizen::newcomer(format!("Hand {k}"), 900 + k as u64, 30, None, None, at, 0));
        w.assign_job(c, id);
    }
    id
}

/// A building site on a free home lot.
fn site(w: &mut World) -> BuildingId {
    let lot = free_lot(w, LandUse::Residential);
    w.build_on(lot);
    let id = w.lots[lot].building.expect("a site");
    assert!(w.buildings[id].construction.is_some());
    id
}

fn contract(w: &World, site: BuildingId) -> crate::buildings::SiteContract {
    w.buildings[site].construction.as_ref().and_then(|c| c.contract.clone()).expect("a contract")
}

#[test]
fn a_site_is_let_to_a_contractor_in_town_who_brings_its_crew_and_buys_the_materials() {
    let mut w = town();
    let yard = open(&mut w, LandUse::Industrial, BusinessKind::Contractor, 8);
    assert_eq!(w.contractor_crews(yard), 2);
    // (Its own yard was built by a firm from out of town.)
    let imports0 = w.builders.today.import_loads;
    // The job is let as the site opens (its first order goes in at once).
    let s = site(&mut w);
    let k = contract(&w, s);
    assert_eq!(k.builder, SiteBuilder::Local(yard));
    let (materials, labour) = w.job_estimate(s);
    assert!((k.price - (materials + labour) * crate::builders::LOCAL_MARGIN).abs() < 1.0, "priced at the local margin: {} vs {}", k.price, (materials + labour) * crate::builders::LOCAL_MARGIN);
    assert_eq!(w.buildings[s].jobs, 0, "the contractor's own crew: the site hires nobody");
    assert_eq!(w.site_builder(s).map(|x| x.0), Some(w.buildings[yard].name.clone()));
    assert_eq!(w.builders.today.let_local, 1);
    // Tendering again changes nothing.
    w.tender_site(s);
    assert_eq!(w.builders.today.let_local, 1);
    // Its crews go to the site, not the yard.
    let hand = w.buildings[yard].workers[0];
    assert_eq!(w.crew_site(hand), Some(s));
    // The first quarter's order: the developer paid an instalment, the
    // contractor bought the stage's lines (imported, on one lorry: nobody
    // in town makes building materials).
    let z = w.buildings[yard].business.as_ref().unwrap();
    assert!((k.paid - k.price / 4.0).abs() < 1.0, "a quarter of the price paid: {}", k.paid);
    assert!(z.revenue_today >= k.paid - 1.0, "the contractor took it");
    assert!(z.spend_today > 0.0, "the contractor paid for the materials");
    let c = w.buildings[s].construction.as_ref().unwrap();
    assert!(c.ordered && !c.due.is_empty(), "the stage's loads booked on the site");
    assert_eq!(w.builders.today.import_loads, imports0 + 1);
    // The site gets built and the books say who built it.
    for _ in 0..(20 * 1440) {
        if w.buildings[s].construction.is_none() {
            break;
        }
        tick(&mut w, 1);
    }
    assert!(w.buildings[s].construction.is_none(), "the site opened");
    let done = w.builders.today.finished_local + w.builders.days.iter().map(|d| d.finished_local).sum::<u32>();
    assert!(done >= 1, "a site finished by a builder in town");
}

#[test]
fn with_no_crew_free_the_job_goes_to_a_firm_from_out_of_town_at_a_markup() {
    let mut w = town();
    // No contractor at all: out of town.
    let a = site(&mut w);
    let k = contract(&w, a);
    let SiteBuilder::OutOfTown { name, .. } = &k.builder else { panic!("out of town: {:?}", k.builder) };
    assert!(!name.is_empty());
    let (materials, labour) = w.job_estimate(a);
    assert!((k.price - (materials + labour * crate::builders::OUT_OF_TOWN_MARKUP)).abs() < 1.0);
    assert_eq!(w.buildings[a].jobs, Construction::crew(w.buildings[a].floors), "its labourers are hired at the gate");
    // A contractor with one crew takes one job; the next goes out of town.
    let yard = open(&mut w, LandUse::Industrial, BusinessKind::Contractor, 3);
    assert_eq!(w.contractor_crews(yard), 1);
    // (a, and the yard itself, went out of town.)
    assert_eq!((w.builders.today.let_local, w.builders.today.let_outside), (0, 2));
    let b = site(&mut w);
    let c = site(&mut w);
    w.tender_site(b);
    w.tender_site(c);
    assert_eq!(contract(&w, b).builder, SiteBuilder::Local(yard));
    assert!(matches!(contract(&w, c).builder, SiteBuilder::OutOfTown { .. }), "no crew free");
    assert_eq!((w.builders.today.let_local, w.builders.today.let_outside), (1, 3));
    // The firms from out of town were paid their fee as the materials were ordered.
    assert!(w.builders.today.outside_paid > 0.0);
    // A job let stays let: tendering again changes nothing.
    w.tender_site(c);
    assert!(matches!(contract(&w, c).builder, SiteBuilder::OutOfTown { .. }));
}

#[test]
fn the_merchant_restocks_and_sells_to_builders_and_residents() {
    let mut w = town();
    let merchant = open(&mut w, LandUse::Commercial, BusinessKind::BuildersMerchant, 4);
    // Cement: every site's foundations take some, and nobody makes it here.
    let parts = Resource::Cement;
    assert!(crate::catalogue::site_materials().contains(&parts));
    // It restocks its trade stock (a bulk import).
    w.builders_hourly();
    assert!(w.buildings[merchant].business.as_ref().unwrap().inbound_of(parts) > 0.0, "parts on order");
    for _ in 0..3000 {
        if w.buildings[merchant].business.as_ref().unwrap().stock_of(parts) > 5.0 {
            break;
        }
        tick(&mut w, 1);
    }
    let stock = w.buildings[merchant].business.as_ref().unwrap().stock_of(parts);
    assert!(stock > 5.0, "the racks filled: {stock}");
    // Its trade price beats a site's own import.
    let jobs = w.buildings[merchant].jobs;
    let price = w.buildings[merchant].business.as_ref().unwrap().sell_price(parts, jobs);
    assert!(price < parts.base_price() * IMPORT_MARKUP, "{price}");
    // A builder's site buys its load over the trade counter.
    // (The seller index is built on the hour: one opened this hour is in it from the next.)
    w.seller_index_hour = None;
    let yard = open(&mut w, LandUse::Industrial, BusinessKind::Contractor, 4);
    let rev0 = w.buildings[merchant].business.as_ref().unwrap().revenue_today;
    let loads0 = w.builders.today.merchant_loads;
    let s = site(&mut w);
    assert_eq!(contract(&w, s).builder, SiteBuilder::Local(yard));
    assert!(w.builders.today.merchant_loads > loads0, "bought from the merchant, not imported");
    assert!(w.buildings[merchant].business.as_ref().unwrap().revenue_today > rev0);
    assert!(w.builders.today.trade_sales > 0.0);
    // The builder fills its yard from the merchant in the morning.
    let sales0 = w.builders.today.trade_sales;
    // (Loads that were on their way to its own site when it opened.)
    w.buildings[yard].business.as_mut().unwrap().stock.clear();
    w.buildings[yard].business.as_mut().unwrap().inbound.clear();
    while w.clock.hour() != 7 {
        w.clock.tick += 60 - w.clock.tick % 60;
    }
    w.builders_hourly();
    let yard_in = w.buildings[yard].business.as_ref().unwrap().inbound_of(parts) + w.buildings[yard].business.as_ref().unwrap().stock_of(parts);
    assert!(yard_in > 0.0, "the yard's stock on its way");
    assert!(w.builders.today.trade_sales > sales0, "bought over the merchant's counter");
    // Residents shop there too: DIY goods off its shop floor.
    w.buildings[merchant].business.as_mut().unwrap().add(Resource::Goods, 10.0);
    assert!(w.buildings[merchant].business.as_ref().unwrap().sells(Resource::Goods));
    let at = w.lots[w.buildings[merchant].lot].center;
    let shopper = w.citizens.insert(Citizen::newcomer("Shopper".into(), 4242, 35, None, None, at, 0));
    w.citizens[shopper].savings = 500.0;
    let rev1 = w.buildings[merchant].business.as_ref().unwrap().revenue_today;
    w.buy(shopper, merchant, Resource::Goods);
    assert!(w.buildings[merchant].business.as_ref().unwrap().revenue_today > rev1, "a resident bought something");
    // Other trades restocking find it in the market too.
    w.refresh_seller_index();
    let found = w.best_seller(parts, at, 2.0, BuildingId::default());
    assert_eq!(found.map(|f| f.0), Some(merchant));
}

#[test]
fn a_builder_with_no_work_goes_bust_and_its_job_is_let_again() {
    let mut w = town();
    let yard = open(&mut w, LandUse::Industrial, BusinessKind::Contractor, 4);
    let s = site(&mut w);
    assert_eq!(contract(&w, s).builder, SiteBuilder::Local(yard));
    // Deep in debt for five nights: it shuts.
    for _ in 0..6 {
        w.buildings[yard].business.as_mut().unwrap().money = -5_000.0;
        w.daily_businesses();
    }
    assert!(w.buildings[yard].business.as_ref().unwrap().closed, "bust");
    assert!(w.builders.today.busts >= 1);
    // Its site goes back out to tender (out of town now).
    assert!(w.needs_tender(s));
    w.tender_site(s);
    assert!(matches!(contract(&w, s).builder, SiteBuilder::OutOfTown { .. }));
}

#[test]
fn a_contract_survives_a_save_and_old_sites_load_without_one() {
    let mut w = town();
    let yard = open(&mut w, LandUse::Industrial, BusinessKind::Contractor, 4);
    let s = site(&mut w);
    let untendered = site(&mut w);
    // A site from before the builders: no contract.
    w.buildings[untendered].construction.as_mut().unwrap().contract = None;
    assert!(w.needs_tender(untendered));
    let k = contract(&w, s);
    let back = World::from_ron(&w.to_ron().unwrap()).expect("loads");
    assert_eq!(back.buildings[s].construction.as_ref().unwrap().contract.as_ref(), Some(&k));
    assert!(back.buildings[untendered].construction.as_ref().unwrap().contract.is_none());
    assert_eq!(back.builders, w.builders);
    assert_eq!(back.buildings[yard].business.as_ref().unwrap().kind, BusinessKind::Contractor);
}

#[test]
fn the_pressures_ask_for_a_contractor_and_a_merchant_when_jobs_go_out_of_town() {
    let mut s = crate::pressure::Signals::default();
    s.outside_contracts = 6.0;
    s.site_material_units = 900.0;
    let supply = crate::pressure::Supply::default();
    let r = crate::pressure::assess(&s, &supply);
    let get = |k: crate::pressure::PressureKind| r.iter().find(|p| p.kind == k).unwrap().clone();
    assert!(get(crate::pressure::PressureKind::Contractor).wants_more());
    assert!(get(crate::pressure::PressureKind::BuildersMerchant).wants_more());
    assert_eq!(crate::pressure::PressureKind::Contractor.business(), Some(BusinessKind::Contractor));
}
