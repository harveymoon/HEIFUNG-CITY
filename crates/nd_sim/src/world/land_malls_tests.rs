//! [land-assembly] [malls] Companies assembling the land behind the road
//! (`world/land_assembly.rs`) and strip malls, malls and mega-malls
//! (`world/malls.rs`): backland zoned and joined to a road plot's site,
//! strip mines and car parks spreading over it while they stay open, a
//! strip mall's row of shops and car park, a mall's own store keeping one
//! unit, a thriving mall buying the block, shoppers coming further for it,
//! old saves loading, and the books exact throughout.

use super::*;
use crate::bank::Account;
use crate::buildings::{Occupant, UnitRole};
use crate::estate::Form;

/// A block between two streets 300 m apart, zoned `lu` all over (its
/// backland too).
fn block(lu: LandUse) -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-200.0, 300.0), Vec2::new(200.0, 300.0)], 1, 1);
    let ids: Vec<usize> = (0..w.parcels.len())
        .filter(|&i| {
            let c = w.parcels.parcels[i].center;
            c.y > 0.0 && c.y < 300.0 && c.x.abs() < 190.0
        })
        .collect();
    w.project = None;
    w.paint_parcels(&ids, Some(lu), None);
    w.try_implement().expect("implemented");
    w
}

/// A road plot with a landlocked plot behind it that would join it into
/// one compact site.
fn front_with_backland(w: &World, lu: LandUse) -> (LotId, LotId) {
    let mut fronts: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == lu && !l.landlocked && l.car_access()).map(|(id, _)| id).collect();
    fronts.sort();
    for f in fronts {
        let back = w.plots_next_to(&[f], lu, 0, None).into_iter().map(|(_, l)| l).find(|l| w.lots[*l].landlocked && crate::estate::merged_lot(&w.lots[f], &[&w.lots[*l]]).is_some());
        if let Some(b) = back {
            return (f, b);
        }
    }
    panic!("no road plot with backland behind it in the block");
}

fn opened(w: &mut World, lot: LotId, kind: BusinessKind) -> BuildingId {
    let id = w.build_on_as(lot, Some(kind)).expect("a site");
    w.finish_construction(id);
    w.transfer(Account::Outside, Account::Business(id), 50_000.0, Kind::Investment);
    id
}

fn site_area(w: &World, bid: BuildingId) -> f32 {
    w.footprint(bid).map_or(0.0, |l| l.area())
}

/// Backland zoned industrial nothing builds on alone; a strip mine on the
/// road plot in front buys it, pays a deposit on its land, and digs its pit
/// over both without closing; the site is one, entered by the road plot.
#[test]
fn a_strip_mine_digs_into_the_backland_behind_its_road_plot() {
    let mut w = block(LandUse::Industrial);
    let (front, back) = front_with_backland(&w, LandUse::Industrial);
    assert!(!w.site_clear(back, SiteRule::STREET), "backland is not built on alone");
    let mine = opened(&mut w, front, BusinessKind::StripMine);
    let before = site_area(&w, mine);
    let money = w.buildings[mine].business.as_ref().unwrap().money;
    assert!(w.grow_site_in_place(mine), "the mine buys the land behind it");
    let b = &w.buildings[mine];
    assert!(b.plot.extra_lots.contains(&back), "the backland is part of its site");
    assert_eq!(w.lots[back].building, Some(mine));
    assert!(b.operating() && b.business.as_ref().is_some_and(|z| !z.closed), "it stays open while it grows");
    assert_eq!(b.plot.form, Some(Form::Merged));
    assert!(site_area(&w, mine) > before * 1.3, "one bigger site: {before:.0} -> {:.0} m²", site_area(&w, mine));
    let fp = w.footprint(mine).unwrap();
    assert!(!fp.landlocked && fp.front_edge == w.lots[front].front_edge, "entered by its road plot");
    assert!(w.buildings[mine].business.as_ref().unwrap().money < money, "it paid a deposit on the land");
    assert!(w.estate.stats.backland_plots_total >= 1);
    // It grows on up to its cap and no further.
    for _ in 0..12 {
        w.grow_site_in_place(mine);
    }
    assert!(w.buildings[mine].plot.lots(front).len() <= crate::estate::BIG_SITE_LOTS);
    assert_eq!(w.reconcile_ledger(), 0);
    assert_eq!(w.ledger_sum(), 0);
}

/// [land-assembly] A strip mine needs a plot a little bigger than a works
/// yard: the market never picks one for a small plot.
#[test]
fn no_strip_mine_on_a_small_plot() {
    let mut w = block(LandUse::Industrial);
    let small: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Industrial && !l.landlocked && l.area() < crate::industry::STRIP_MINE_MIN_AREA).map(|(id, _)| id).collect();
    assert!(!small.is_empty(), "the block has small yards");
    w.industry_bias.push(crate::industry::IndustryBias { polygon: vec![Vec2::new(-500.0, -500.0), Vec2::new(500.0, -500.0), Vec2::new(500.0, 500.0), Vec2::new(-500.0, 500.0)], kind: Some(crate::industry::IndustryKind::Mining) });
    for l in small {
        assert_ne!(w.pick_industrial_business(l), Some(BusinessKind::StripMine));
    }
}

/// A surface car park grows outward over the plots behind its road plot:
/// its bays are those its whole site fits, then and after a reload.
#[test]
fn a_car_park_grows_over_the_plots_behind_it() {
    let mut w = block(LandUse::Commercial);
    let (front, back) = front_with_backland(&w, LandUse::Commercial);
    let park = w.build_parking(front).expect("a car park");
    w.buildings[park].floors = 0;
    w.buildings[park].parking_capacity = crate::parking_layout::surface_capacity(&w.lots[front]);
    w.transfer(Account::Outside, Account::Business(park), 50_000.0, Kind::Investment);
    let before = w.buildings[park].parking_capacity;
    assert!(w.grow_site_in_place(park));
    assert!(w.buildings[park].plot.extra_lots.contains(&back));
    let after = w.buildings[park].parking_capacity;
    assert!(after > before, "more bays: {before} -> {after}");
    assert_eq!(after, crate::parking_layout::surface_capacity(&w.footprint(park).unwrap()));
    let back_in = World::from_ron(&w.to_ron().unwrap()).expect("reloads");
    assert_eq!(back_in.buildings[park].parking_capacity, after, "a reload keeps the grown car park's bays");
    assert_eq!(w.ledger_sum(), 0);
}

/// A strip mall: a row of shopfronts (one per 7 m of street front, three
/// to eight) under offices, ten to twenty spaces out front; its own shop
/// keeps one shopfront and the rest are to let.
#[test]
fn a_strip_mall_is_a_row_of_shops_behind_a_small_car_park() {
    let mut b = Building::new(LotId::default(), LandUse::Commercial, 2, Tier::Low, 11, "Palm Plaza".into(), 0, 0.3);
    b.plot.form = Some(Form::StripMall);
    b.business = Some(Business::new(BusinessKind::Grocery, 6));
    b.units = super::tenants::unit_layout(&b, 40.0, 600.0);
    let shops = b.units.iter().filter(|u| u.shopfront).count();
    assert_eq!(shops, 5, "40 m of street front: five shops");
    assert_eq!(b.units.iter().filter(|u| !u.shopfront).count(), 1, "offices over them");
    b.recompute_capacity();
    super::tenants::assign_main(&mut b, false, 0);
    assert_eq!(b.units.iter().filter(|u| u.occupant == Occupant::Main).count(), 1, "its own shop has one unit");
    assert!((10..=20).contains(&b.parking_capacity), "a small car park: {}", b.parking_capacity);
    let wide = super::tenants::unit_layout(&b, 200.0, 600.0).iter().filter(|u| u.shopfront).count();
    assert_eq!(wide, super::malls::STRIP_MAX_SHOPS);
}

/// A mall is many shops round its car park, its store an anchor; a
/// mega-mall has two anchors, a food court and more shops.
#[test]
fn malls_and_mega_malls_are_cut_into_many_shops() {
    let mall = super::malls::mall_layout(Form::Mall, 2, 60.0, 1000.0);
    assert!(mall.len() >= super::malls::MALL_MIN_SHOPS && mall.iter().all(|u| u.shopfront));
    assert!(mall.iter().any(|u| u.lo == 1), "shops upstairs too");
    let mega = super::malls::mall_layout(Form::MegaMall, 2, 90.0, 2400.0);
    assert_eq!(mega.iter().filter(|u| u.role == UnitRole::Anchor).count(), 2);
    assert_eq!(mega.iter().filter(|u| u.role == UnitRole::FoodCourt).count(), super::malls::FOOD_COURT_COUNTERS);
    assert!(mega.iter().filter(|u| u.role == UnitRole::Shop).count() >= super::malls::MEGA_MIN_SHOPS);
    let mut b = Building::new(LotId::default(), LandUse::Commercial, 2, Tier::Mid, 5, "Galleria".into(), 0, 0.4);
    b.plot.form = Some(Form::MegaMall);
    b.business = Some(Business::new(BusinessKind::GadgetShop, 10));
    b.units = mega;
    b.recompute_capacity();
    super::tenants::assign_main(&mut b, false, 0);
    let own: Vec<_> = b.units.iter().filter(|u| u.occupant == Occupant::Main).collect();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].role, UnitRole::Anchor, "its store is an anchor");
}

/// A mall its company owns, let well and open a week, buys the commercial
/// plots round it and rebuilds as a mega-mall; its shops then draw people
/// from three times as far. The books stay exact.
#[test]
fn a_thriving_mall_buys_the_block_and_becomes_a_mega_mall() {
    let mut w = block(LandUse::Commercial);
    let (front, _) = front_with_backland(&w, LandUse::Commercial);
    let id = w.build_on_as(front, Some(BusinessKind::Grocery)).expect("a site");
    w.buildings[id].plot.form = Some(Form::Mall);
    w.buildings[id].floors = 2;
    w.finish_construction(id);
    w.firm_of(id);
    assert!(!w.buildings[id].units.is_empty(), "cut into shops");
    // Let all but one of its shops.
    let vacant: Vec<usize> = w.buildings[id].units.iter().enumerate().filter(|(_, u)| u.vacant()).map(|(i, _)| i).collect();
    for &i in vacant.iter().skip(1) {
        w.open_tenant(id, i, Business::new(BusinessKind::Bakery, 2), None).expect("let");
    }
    assert!(!w.mall_thriving(id), "not before it has traded a week");
    w.clock.tick += 8 * 1440;
    assert!(w.mall_thriving(id));
    assert!(w.shop_reach(id) > 1.0 && w.shop_reach(w.tenants_of(id)[0]) > 1.0, "a mall draws from further");
    assert!(w.grow_mega_mall(id), "it buys the block");
    let b = &w.buildings[id];
    assert_eq!(b.plot.form, Some(Form::MegaMall));
    assert!(b.plot.extra_lots.len() + b.plot.pending_lots.len() >= 2, "plots bought round it");
    assert!(b.construction.is_some(), "rebuilding");
    w.finish_construction(id);
    w.sync_tenants();
    let b = &w.buildings[id];
    assert!(b.units.iter().any(|u| u.role == UnitRole::Anchor) && b.units.iter().any(|u| u.role == UnitRole::FoodCourt));
    assert_eq!(w.shop_reach(id), super::malls::MEGA_MALL_REACH);
    let s = w.mall_stats();
    assert_eq!(s.mega_malls, 1);
    assert!(s.vacant > 0);
    assert_eq!(w.reconcile_ledger(), 0);
    assert_eq!(w.ledger_sum(), 0);
}

/// An old save (no unit roles, none of the new estate counters) loads: its
/// units are shops, its counters zero.
#[test]
fn an_old_save_loads_with_shops_and_no_malls() {
    let w = World::from_ron(&super::legacy_save_text()).expect("loads");
    let text: String = w.to_ron().unwrap().lines().filter(|l| {
        let t = l.trim_start();
        !(t.starts_with("role:") || t.starts_with("site_plots_total:") || t.starts_with("backland_plots_total:") || t.starts_with("malls_total:") || t.starts_with("mega_malls_total:"))
    }).map(|l| format!("{l}\n")).collect();
    assert!(!text.contains("role: "), "no new fields left");
    let back = World::from_ron(&text).expect("an old save loads");
    assert!(back.buildings.values().flat_map(|b| b.units.iter()).all(|u| u.role == UnitRole::Shop));
    assert_eq!(back.estate.stats.mega_malls_total, 0);
    assert_eq!(back.mall_stats().mega_malls, 0);
    assert_eq!(back.ledger_sum(), 0);
}
