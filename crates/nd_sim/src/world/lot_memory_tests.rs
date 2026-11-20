//! [r11-lot-memory] Lots remember what stood there (`crate::lot_memory`,
//! `world/lot_memory.rs`): a building pulled down and a trade changed are
//! recorded on the lot, saved and loaded; an old save loads with none; the
//! same city run twice remembers the same.

use super::*;
use crate::lot_memory::TraceKind;
use crate::mind::IdleMind;

fn legacy() -> World {
    World::from_ron(&super::legacy_save_text()).expect("loads")
}

/// A standing block of homes (no business, no units) and its lot.
fn standing_home(w: &World) -> (BuildingId, LotId) {
    w.buildings
        .iter()
        .find(|(_, b)| b.operating() && b.land_use == LandUse::Residential && b.business.is_none() && b.units.is_empty() && b.plot.extra_lots.is_empty() && b.floors >= 2)
        .map(|(id, b)| (id, b.lot))
        .expect("a block of homes")
}

#[test]
fn an_old_save_loads_with_no_history() {
    let w = legacy();
    assert!(w.lot_memory.is_empty());
    assert!(!super::legacy_save_text().contains("lot_memory"), "the fixture is from before lot memory");
}

/// Pulled down: the lot keeps its outline, height, name and the day it
/// went, once the rubble is gone (not while it is coming down).
#[test]
fn a_demolished_building_is_remembered_on_its_lot() {
    let mut w = legacy();
    let (bid, lot) = standing_home(&w);
    let (name, floors, seed) = (w.buildings[bid].name.clone(), w.buildings[bid].floors, w.buildings[bid].seed);
    let outline = w.lots[lot].polygon.clone();
    w.demolish(bid);
    assert!(w.buildings[bid].being_demolished());
    assert!(w.lot_history(lot).is_empty(), "nothing yet while it comes down");
    w.clock.tick += 3 * 1440;
    w.finish_demolition(bid);
    assert!(!w.buildings.contains_key(bid));
    let h = w.lot_history(lot);
    assert_eq!(h.len(), 1);
    let t = h[0];
    assert_eq!(t.kind, TraceKind::Demolished);
    assert_eq!((t.name.as_str(), t.floors, t.seed), (name.as_str(), floors, seed));
    assert_eq!(t.tick, w.clock.tick);
    assert_eq!(t.outline.len(), outline.len());
    assert!(t.outline.iter().zip(&outline).all(|(a, b)| a.distance(*b) < 1e-3));
    assert!(t.line().contains("pulled down") && t.line().contains(&name), "{}", t.line());
}

/// A redevelopment: the old building is remembered when its successor's
/// construction takes over, with the old name and trade, not the new.
#[test]
fn a_redevelopment_remembers_the_old_building() {
    let mut w = legacy();
    let (bid, lot) = standing_home(&w);
    let (name, floors) = (w.buildings[bid].name.clone(), w.buildings[bid].floors);
    let value = w.buildings[bid].land_value;
    w.redevelop(bid, Tier::High, floors + 4, value);
    w.buildings.get_mut(bid).unwrap().name = "Chrome Heights".into();
    w.finish_demolition(bid);
    assert!(w.buildings[bid].construction.is_some(), "the successor is going up");
    let h = w.lot_history(lot);
    assert_eq!(h.len(), 1);
    assert_eq!((h[0].name.as_str(), h[0].floors), (name.as_str(), floors));
    assert_eq!(h[0].reason, crate::buildings::DemolitionReason::Redevelopment);
}

/// A trade changed in a standing building: what it was, what it became.
#[test]
fn a_change_of_trade_is_remembered() {
    let mut w = legacy();
    let (bid, kind) = w
        .buildings
        .iter()
        .find(|(_, b)| b.operating() && !b.is_tenant() && b.business.as_ref().is_some_and(|z| z.kind != BusinessKind::Grocery && !z.kind.needs_site()))
        .map(|(id, b)| (id, b.business.as_ref().unwrap().kind))
        .expect("a business");
    let (name, lot) = (w.buildings[bid].name.clone(), w.buildings[bid].lot);
    let jobs = w.buildings[bid].jobs;
    w.set_business(bid, Some(crate::market::Business::new(BusinessKind::Grocery, jobs)));
    // Noted once, though the shopfront sign follows it in the hour.
    w.sync_tenants();
    let h = w.lot_history(lot);
    assert_eq!(h.len(), 1, "{:?}", h.iter().map(|t| t.line()).collect::<Vec<_>>());
    assert_eq!((h[0].kind, h[0].business, h[0].now, h[0].building), (TraceKind::Changed, Some(kind), Some(BusinessKind::Grocery), bid));
    assert_eq!(h[0].name, name);
    assert!(h[0].line().contains("became a grocery"), "{}", h[0].line());
    // The same trade again is no change.
    w.set_business(bid, Some(crate::market::Business::new(BusinessKind::Grocery, jobs)));
    assert_eq!(w.lot_history(lot).len(), 1);
}

/// Saved with the world and loaded back the same.
#[test]
fn history_is_saved_and_loaded() {
    let mut w = legacy();
    let (bid, lot) = standing_home(&w);
    w.demolish(bid);
    w.finish_demolition(bid);
    assert_eq!(w.lot_history(lot).len(), 1);
    let before = w.lot_memory.traces().to_vec();
    let text = w.to_ron().expect("saves");
    let back = World::from_ron(&text).expect("loads");
    let after = back.lot_memory.traces();
    assert_eq!(after.len(), before.len());
    for (a, b) in before.iter().zip(after) {
        assert_eq!((a.id, a.kind, a.tick, a.floors, &a.name, a.seed, a.reason, a.building), (b.id, b.kind, b.tick, b.floors, &b.name, b.seed, b.reason, b.building));
        assert!(a.at.distance(b.at) < 0.08);
    }
    assert_eq!(back.lot_history(back.lots.keys().find(|l| back.lots[*l].contains(before[0].at)).unwrap()).len(), 1);
}

/// The same city run twice remembers the same, and a few entries at most
/// stay on one lot.
#[test]
fn deterministic_and_bounded_per_lot() {
    let run = || {
        let mut w = legacy();
        let mut minds = IdleMind;
        // Pull a few down and let the city run on (shops change hands).
        let homes: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.operating() && b.business.is_none() && b.units.is_empty()).map(|(id, _)| id).take(3).collect();
        for b in homes {
            w.demolish(b);
        }
        for _ in 0..2 * 1440 {
            w.tick(&mut minds);
        }
        w
    };
    let (a, b) = (run(), run());
    assert!(a.lot_memory.traces().iter().filter(|t| t.kind == TraceKind::Demolished).count() >= 3, "{:?}", a.lot_memory.traces().iter().map(|t| t.line()).collect::<Vec<_>>());
    assert_eq!(a.lot_memory, b.lot_memory);
    for l in a.lots.values() {
        assert!(a.lot_memory.on_lot(l).len() <= crate::lot_memory::PER_LOT);
    }
}

/// A building site cleared before it opened leaves nothing to remember.
#[test]
fn a_site_that_never_opened_is_not_remembered() {
    let mut w = legacy();
    let (bid, lot) = standing_home(&w);
    let value = w.buildings[bid].land_value;
    // Rebuilt, then its new site rebuilt again before it opened.
    w.redevelop(bid, Tier::High, 6, value);
    w.finish_demolition(bid);
    assert_eq!(w.lot_history(lot).len(), 1, "the old block");
    w.redevelop(bid, Tier::High, 8, value);
    w.finish_demolition(bid);
    assert_eq!(w.lot_history(lot).len(), 1, "not the site that never opened");
}
