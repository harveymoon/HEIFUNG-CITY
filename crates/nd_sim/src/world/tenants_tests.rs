//! [tenants] Several businesses in one building (`world/tenants.rs`):
//! units from floors and street front, old saves cut into units with
//! nobody laid off, empty units let, rent by floor area to the owner,
//! tenants out when the building comes down, the books exact throughout.

use super::*;
use crate::bank::Account;
use crate::buildings::{Occupant, Unit};
use crate::mind::IdleMind;

fn plain(lu: LandUse, floors: u8, tier: Tier) -> Building {
    Building::new(LotId::default(), lu, floors, tier, 7, "Test Block".into(), 0, 0.5)
}

/// Units follow the building: a walk-up of homes is one thing, a block of
/// flats has shops along its street front (one per 14 m), an office block
/// a shopfront and its upper floors in ranges (eight at most), a
/// supertall a two-storey mall base; a forecourt or a shed is never split.
#[test]
fn units_follow_floors_and_street_front() {
    assert!(tenants::unit_layout(&plain(LandUse::Residential, 2, Tier::Mid), 30.0, 300.0).is_empty(), "a walk-up is homes only");
    let flats = tenants::unit_layout(&plain(LandUse::Residential, 5, Tier::Mid), 30.0, 300.0);
    assert_eq!(flats.len(), 2, "two shops along 30 m");
    assert!(flats.iter().all(|u| u.shopfront && u.lo == 0 && u.hi == 0 && u.vacant()));
    assert!((flats.iter().map(|u| u.area).sum::<f32>() - 300.0).abs() < 1e-3, "the ground floor between them");
    assert!(tenants::unit_layout(&plain(LandUse::Residential, 5, Tier::Mid), 0.0, 300.0).is_empty(), "no street front, no shops");
    let tower = tenants::unit_layout(&plain(LandUse::Commercial, 40, Tier::High), 20.0, 500.0);
    let floors: Vec<&Unit> = tower.iter().filter(|u| !u.shopfront).collect();
    assert!(floors.len() <= 8 && floors.len() >= 5, "offices in ranges: {}", floors.len());
    assert_eq!(floors.first().unwrap().lo, 1);
    assert_eq!(floors.last().unwrap().hi, 39, "every floor let");
    let mut mall = plain(LandUse::Residential, 60, Tier::High);
    mall.plot.tower = Some(crate::towers::Tower { height: 240.0, design: crate::towers::TowerDesign::DecoSpire, seed: 1, started_day: 0, topped_out_day: None, record_bid: false });
    let base = tenants::unit_layout(&mall, 25.0, 900.0);
    assert!(base.len() >= 4 && base.iter().all(|u| u.shopfront && u.hi == 1), "a mall base under the flats");
    let mut dealer = plain(LandUse::Commercial, 2, Tier::Mid);
    dealer.business = Some(Business::new(BusinessKind::CarDealer, 6));
    assert!(tenants::unit_layout(&dealer, 30.0, 300.0).is_empty(), "a forecourt is one business");
}

/// The units' jobs: an office block's own jobs spread over its units (the
/// same total however it is let); its own business has those of its own
/// units; an office takes the floors and the shops under it are let. On a
/// save from before units its business keeps units for all its staff.
#[test]
fn the_buildings_own_business_keeps_the_units_its_kind_wants() {
    let mut b = plain(LandUse::Commercial, 4, Tier::Mid);
    let all = b.jobs;
    b.units = tenants::unit_layout(&b, 28.0, 300.0);
    b.business = Some(Business::new(BusinessKind::Office, all));
    b.recompute_capacity();
    tenants::assign_main(&mut b, false, 0);
    b.recompute_capacity();
    assert!(b.units.iter().filter(|u| u.shopfront).all(|u| u.vacant()), "the shops under offices are to let");
    assert!(b.units.iter().filter(|u| !u.shopfront).all(|u| u.occupant == Occupant::Main));
    let sum: u16 = b.units.iter().map(|u| u.jobs).sum();
    assert!(sum.abs_diff(all) <= b.units.len() as u16, "the units hold the building's jobs: {sum} of {all}");
    assert!(b.jobs < all, "the office has its floors only");
    let mut old = plain(LandUse::Commercial, 4, Tier::Mid);
    old.business = Some(Business::new(BusinessKind::Office, all));
    old.units = tenants::unit_layout(&old, 28.0, 300.0);
    old.recompute_capacity();
    tenants::assign_main(&mut old, false, all);
    old.recompute_capacity();
    assert!(old.jobs >= all, "units enough for all its staff");
}

/// A save from before units loads: every building that can be let in
/// parts is cut into units, its business keeping units for all its staff
/// (nobody is laid off, no business is lost), the rest to let; the books
/// balance, and the units survive a save.
#[test]
fn an_old_save_is_cut_into_units_and_nobody_loses_a_job() {
    let text = super::legacy_save_text();
    assert!(!text.contains("unit_of") && !text.contains("shopfront"), "the fixture is from before units");
    let w = World::from_ron(&text).expect("the old save loads");
    let cut: Vec<&Building> = w.buildings.values().filter(|b| !b.units.is_empty()).collect();
    assert!(!cut.is_empty(), "some buildings are let in parts");
    for b in w.buildings.values() {
        assert!(b.workers.len() <= b.jobs as usize || b.construction.is_some(), "{} keeps all its staff: {}/{}", b.name, b.workers.len(), b.jobs);
        assert!(!b.is_tenant(), "no tenants yet, only empty units");
    }
    for b in cut.iter().filter(|b| b.business.is_some()) {
        assert!(b.units.iter().any(|u| u.occupant == Occupant::Main), "{}: its business is still in it", b.name);
    }
    assert_eq!(w.ledger_sum(), 0);
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let n: usize = w.buildings.values().map(|b| b.units.len()).sum();
    assert_eq!(back.buildings.values().map(|b| b.units.len()).sum::<usize>(), n);
}

/// A shop opens in an empty unit: a business of its own (account, staff,
/// books), on its host's lot, paying rent for its floor area to the
/// host's owner; twice the floor, twice the rent. When the building comes
/// down the tenant goes with it (its staff let go, its account closed) and
/// the books balance at every step.
#[test]
fn a_tenant_rents_by_floor_area_and_leaves_with_its_building() {
    let mut w = World::from_ron(&super::legacy_save_text()).expect("loads");
    let (host, i) = w
        .buildings
        .iter()
        .find_map(|(id, b)| b.units.iter().position(|u| u.vacant() && u.shopfront).filter(|_| b.operating()).map(|i| (id, i)))
        .expect("an empty shopfront");
    let tid = w.open_tenant(host, i, Business::new(BusinessKind::Bakery, 3), None).expect("opens");
    assert_eq!(w.ledger_sum(), 0);
    assert!(w.buildings[tid].is_tenant() && w.buildings[tid].lot == w.buildings[host].lot);
    assert_eq!(w.buildings[host].units[i].occupant, Occupant::Tenant(tid));
    assert_eq!(w.tenants_of(host), vec![tid]);
    assert_eq!(w.host_of(tid), host);
    assert!(w.economy.bank.has(Account::Business(tid)), "its own till");
    assert_eq!(w.buildings[tid].jobs, w.buildings[host].units[i].jobs);
    let rent = w.shop_rent(tid);
    assert!(rent > 0.0, "a tenant pays rent");
    assert!((w.business_area(tid) - w.buildings[host].units[i].area).abs() < 1e-3);
    w.buildings.get_mut(host).unwrap().units[i].area *= 2.0;
    assert!((w.shop_rent(tid) - 2.0 * rent).abs() < 1e-3 * rent.max(1.0), "rent by floor area");
    let jobless = w.citizens.iter().find(|(_, c)| c.work.is_none() && c.can_work()).map(|(id, _)| id);
    if let Some(c) = jobless {
        w.assign_job(c, tid);
        assert_eq!(w.citizens[c].work, Some(tid));
    }
    w.demolish(host);
    w.sync_tenants();
    assert!(!w.buildings.contains_key(tid), "the tenant left with its building");
    if let Some(c) = jobless {
        assert_ne!(w.citizens[c].work, Some(tid));
    }
    assert_eq!(w.reconcile_ledger(), 0);
    assert_eq!(w.ledger_sum(), 0);
    assert!(!w.economy.bank.has(Account::Business(tid)), "its account closed");
}

/// Left to run, owners let empty units and the tenants trade, hire and pay
/// rent; the books stay exact to the unit with nothing moved outside the
/// door. Deterministic: two runs from the same save let the same units.
#[test]
fn units_are_let_over_days_with_the_books_exact() {
    let run = || {
        let mut w = World::from_ron(&super::legacy_save_text()).expect("loads");
        let mut minds = IdleMind;
        for _ in 0..3 * 1440 {
            w.tick(&mut minds);
        }
        w
    };
    let mut w = run();
    assert_eq!(w.reconcile_ledger(), 0);
    assert_eq!(w.ledger_sum(), 0);
    let tenants: Vec<(BuildingId, String)> = w.buildings.iter().filter(|(_, b)| b.is_tenant()).map(|(id, b)| (id, b.name.clone())).collect();
    assert!(w.economy.market.tenants_let_total > 0, "some units were let");
    for (t, _) in &tenants {
        let host = w.buildings[*t].unit_of.unwrap();
        assert!(w.buildings[host].units.iter().any(|u| u.occupant == Occupant::Tenant(*t)), "every tenant has its unit");
    }
    let again = run();
    let tenants2: Vec<(BuildingId, String)> = again.buildings.iter().filter(|(_, b)| b.is_tenant()).map(|(id, b)| (id, b.name.clone())).collect();
    assert_eq!(tenants, tenants2, "the same units let to the same shops");
}
