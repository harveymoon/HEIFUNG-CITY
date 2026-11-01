//! [megablocks] Tests of mega-blocks: a corp assembles a block round land
//! it holds and builds over it in four stages; open, it houses its people
//! with shops, offices and sky gardens inside, and they live without
//! leaving; the gates; the land round it; saves.

use super::*;
use crate::estate::{Form, OwnerKind};
use crate::market::DayBook;
use crate::megablock::*;
use crate::mind::IdleMind;

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

/// Avenues at y = -100, 0 and 100 joined at both ends: shops between the
/// first two, homes between the last two (lots back to back from both
/// streets, so a block can be put together in two directions).
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1e7;
    for y in [-100.0f32, 0.0, 100.0] {
        w.add_road(&[Vec2::new(-460.0, y), Vec2::new(460.0, y)], 1, 1);
    }
    for x in [-460.0f32, 460.0] {
        w.add_road(&[Vec2::new(x, -100.0), Vec2::new(x, 0.0), Vec2::new(x, 100.0)], 1, 1);
    }
    // A way in from out of town.
    w.add_road(&[Vec2::new(460.0, 0.0), Vec2::new(760.0, 0.0)], 1, 1);
    w.add_zone(rect(-440.0, -92.0, 880.0, 84.0), LandUse::Commercial);
    w.add_zone(rect(-440.0, 8.0, 880.0, 84.0), LandUse::Residential);
    w
}

fn free_lots(w: &World, lu: LandUse) -> Vec<LotId> {
    let mut v: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.car_access() && l.building.is_none()).map(|(id, _)| id).collect();
    v.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x).then(a.cmp(b)));
    v
}

/// A mega-corp grown from an office firm at the west end of the shops,
/// holding a row of homes in the middle of the residential block.
fn corp_town() -> (World, usize) {
    let mut w = town();
    let shop_lots = free_lots(&w, LandUse::Commercial);
    let office = w.build_on_as(shop_lots[0], Some(BusinessKind::Office)).unwrap();
    w.finish_construction(office);
    let z = w.buildings[office].business.as_mut().unwrap();
    z.books = vec![DayBook { revenue: 20_000.0, spend: 9_000.0 }; 4];
    z.money = 50_000.0;
    let firm = w.new_owner("Okafor Trading".into(), OwnerKind::Firm { kind: BusinessKind::Office });
    w.give_to(office, firm, 0.0);
    let ci = w.promote_to_corp(office).expect("a corp");
    let owner = w.corps.corps[ci].owner;
    // Homes it holds, near the middle of the block.
    let homes: Vec<LotId> = free_lots(&w, LandUse::Residential).into_iter().filter(|l| w.lots[*l].center.x.abs() < 60.0).take(2).collect();
    for l in homes {
        w.build_on(l);
        let b = w.lots[l].building.unwrap();
        w.finish_construction(b);
        w.give_to(b, owner, 0.0);
    }
    (w, ci)
}

#[test]
fn a_corp_builds_its_mega_block_over_land_it_assembled() {
    let (mut w, ci) = corp_town();
    let owner = w.corps.corps[ci].owner;
    // The gates: a hamlet has no mega-blocks.
    assert!(w.megablock_refusal(ci).is_some_and(|r| r.contains("people")), "{:?}", w.megablock_refusal(ci));
    assert!(w.start_megablock(ci, false).is_err());
    let held: Vec<LotId> = w.estate.owners[owner].properties.iter().map(|p| w.buildings[*p].lot).collect();
    let bid = w.start_megablock(ci, true).expect("a block can be put together");
    let b = &w.buildings[bid];
    assert_eq!(b.plot.form, Some(Form::MegaBlock));
    assert_eq!(b.plot.owner, Some(owner), "the corp owns it");
    assert_eq!(w.corps.corps[ci].megablock, Some(bid));
    let m = b.plot.megablock.expect("its plan");
    assert!(m.homes >= 600, "it houses thousands: {} homes", m.homes);
    assert!(b.construction.as_ref().is_some_and(|c| c.days >= MEGABLOCK_BUILD_DAYS - 0.01), "a long build");
    let plots: Vec<LotId> = b.plot.lots(b.lot).into_iter().chain(b.plot.pending_lots.iter().copied()).collect();
    assert!(plots.len() >= 6, "a block of plots: {}", plots.len());
    assert!(plots.iter().any(|l| held.contains(l)), "round land it already held");
    let area: f32 = plots.iter().map(|l| w.lots[*l].area()).sum();
    assert!(area >= MEGABLOCK_MIN_AREA, "{area} m²");
    // Only one per corp.
    assert!(w.start_megablock(ci, true).is_err());
    // The report while it rises: the stage.
    let r = w.megablock_report(bid).unwrap();
    assert!(!r.open && r.stage.is_some());
    assert_eq!(r.homes, m.homes);
}

#[test]
fn it_rises_in_four_stages_and_opens_with_its_corps_shops() {
    let (mut w, ci) = corp_town();
    let bid = w.force_megablock(None, Some(0.6)).expect("forced to the towers stage");
    let c = w.buildings[bid].construction.as_ref().expect("still a site");
    assert_eq!(Stage::of_progress(c.progress), Stage::Towers);
    assert_eq!(w.buildings[bid].homes, 0, "nobody lives on a site");
    let bid2 = w.force_megablock(None, None).unwrap();
    assert_eq!(bid, bid2);
    let b = &w.buildings[bid];
    assert!(b.operating(), "open");
    let m = b.plot.megablock.unwrap();
    assert!(m.opened_day.is_some());
    assert_eq!(b.homes, m.homes, "its homes are its plan's");
    assert!(b.parking_capacity > 100, "car decks: {}", b.parking_capacity);
    let r = w.megablock_report(bid).unwrap();
    assert_eq!(r.shops, m.shops as usize);
    assert_eq!(r.market_counters, MARKET_COUNTERS);
    assert_eq!(r.offices, m.offices as usize);
    assert!(r.tenants.len() >= 7, "the corp's own market, gym, café, bar, offices: {:?}", r.tenants);
    assert!(r.tenants.iter().any(|t| t.2 == BusinessKind::Grocery.label()), "a market inside");
    assert!(r.jobs >= 200, "jobs inside: {}", r.jobs);
    // Every tenant stands on the block's lot.
    for t in w.tenants_of(bid) {
        assert_eq!(w.megablock_of(t), Some(bid));
    }
    assert!(w.corps.corps[ci].megablock == Some(bid));
    assert!(w.megablock_brands().contains(&w.corps.corps[ci].owner));
}

#[test]
fn the_land_round_it_is_worth_more_and_shaded_close_in() {
    let (mut w, _) = corp_town();
    let bid = w.force_megablock(None, None).unwrap();
    let c = w.footprint(bid).unwrap().center;
    let mut lv = crate::land_value::LandValue::new(1200.0, 16.0);
    let mut b = crate::land_value::LandValueBuilder::new(&lv);
    w.splat_megablocks(&mut b);
    b.finish(0.3, &mut lv);
    let street_away = c + Vec2::new(230.0, 0.0);
    assert!(lv.at(street_away) > 0.3 + 0.005 && lv.nuisance_at(street_away) == 0.0, "a street away: lifted, no shade: {} {}", lv.at(street_away), lv.nuisance_at(street_away));
    let next_door = c + Vec2::new(50.0, 0.0);
    assert!(lv.nuisance_at(next_door) > 0.0, "next door: in its shade");
    assert!(lv.for_use(next_door, LandUse::Residential) < lv.at(next_door), "homes in its shade feel it");
}

#[test]
fn the_plan_survives_a_save_and_old_plots_load_without_one() {
    let (mut w, _) = corp_town();
    let bid = w.force_megablock(None, None).unwrap();
    let plot = w.buildings[bid].plot.clone();
    let s = ron::to_string(&plot).unwrap();
    let back: crate::estate::Plot = ron::from_str(&s).unwrap();
    assert_eq!(back.megablock, plot.megablock);
    let old: crate::estate::Plot = ron::from_str("(extra_lots: [], form: None, owner: None)").unwrap();
    assert!(old.megablock.is_none());
    let stats: crate::megablock::MegaBlockStats = ron::from_str("()").unwrap();
    assert_eq!(stats.started_total, 0);
}

#[test]
fn the_same_town_assembles_the_same_block() {
    let site = |_: ()| {
        let (mut w, ci) = corp_town();
        let bid = w.start_megablock(ci, true).unwrap();
        let b = &w.buildings[bid];
        let mut v: Vec<LotId> = b.plot.lots(b.lot).into_iter().chain(b.plot.pending_lots.iter().copied()).collect();
        v.sort();
        (v, b.plot.megablock.unwrap().homes)
    };
    assert_eq!(site(()), site(()));
}

