//! [r14-materials] Bills of materials on a real site: each stage orders
//! its own lines, the work waits until every line of the stage is on site,
//! a lost load is ordered again (only what is missing), an older save's
//! parts still count, and the site's pile shrinks as the work uses it.

use super::*;
use crate::catalogue::{Stage, SiteFacts};
use crate::market::Resource;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

/// A fresh site in a test town with nothing yet ordered: the trucks the
/// build ordered are taken off the road and its record cleared.
fn site(lu: LandUse) -> (World, BuildingId) {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), lu);
    let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let id = w.lots[lot].building.unwrap();
    w.freight_queue.retain(|q| q.cargo.to != id);
    let cars: Vec<CarId> = w.traffic.cars.iter().filter(|(_, c)| c.cargo.is_some_and(|k| k.to == id)).map(|(k, _)| k).collect();
    for k in cars {
        w.traffic.remove_car(k);
    }
    let c = w.buildings[id].construction.as_mut().unwrap();
    let days = c.days;
    *c = Construction::new(days);
    (w, id)
}

fn site_of(w: &World, id: BuildingId) -> &Construction {
    w.buildings[id].construction.as_ref().unwrap()
}

/// Every load on its way to the site, as a lorry would bring it.
fn loads_due(w: &World, id: BuildingId) -> Vec<Resource> {
    let mut v: Vec<Resource> = site_of(w, id).due.iter().map(|d| d.via).collect();
    v.dedup();
    v
}

#[test]
fn a_site_orders_its_stage_and_waits_for_every_line() {
    let (mut w, id) = site(LandUse::Residential);
    let bill = w.site_bill(id);
    assert!(!bill.is_empty());
    for st in Stage::ALL {
        assert!(bill.iter().any(|l| l.stage == st), "a line for the {}", st.label());
    }
    w.order_materials(id);
    let c = site_of(&w, id);
    assert!(c.ordered, "{c:?}");
    // Every foundations line is on the road, nothing else.
    let want: Vec<(Resource, f32)> = bill.iter().filter(|l| l.stage == Stage::Foundations).map(|l| (l.good, l.qty)).collect();
    let mut due: Vec<(Resource, f32)> = c.due.iter().map(|d| (d.good, d.qty)).collect();
    due.sort_by_key(|x| x.0);
    let mut want_s = want.clone();
    want_s.sort_by_key(|x| x.0);
    assert_eq!(due.len(), want_s.len(), "{due:?} against {want_s:?}");
    for (a, b) in due.iter().zip(want_s.iter()) {
        assert_eq!(a.0, b.0);
        assert!((a.1 - b.1).abs() < 0.01);
    }
    assert!(c.due.iter().all(|d| d.stage == 0));
    assert!(c.waiting_for_materials());
    // The loads come one by one; the stage is supplied only with the last.
    let loads = loads_due(&w, id);
    for (k, via) in loads.iter().enumerate() {
        assert_eq!(site_of(&w, id).deliveries, 0, "not supplied before every load is in");
        w.deliver(Cargo { from: BuildingId::default(), to: id, resource: *via, amount: 1.0, price: 0.0 });
        if k + 1 < loads.len() {
            assert!(site_of(&w, id).ordered);
        }
    }
    let c = site_of(&w, id);
    assert_eq!(c.deliveries, 1, "{c:?}");
    assert!(!c.ordered && c.due.is_empty() && !c.waiting_for_materials());
    assert_eq!(c.delivered.len(), want.len(), "the pile holds the stage's lines");
    // Next: the frame's lines, not the foundations' again.
    let next = w.site_lines_to_order(id);
    assert!(!next.is_empty() && next.iter().all(|l| l.stage == Stage::Frame), "{next:?}");
}

#[test]
fn a_lost_load_is_ordered_again_and_only_it() {
    let (mut w, id) = site(LandUse::Commercial);
    // Foundations are in; the frame is ordered.
    w.order_materials(id);
    for via in loads_due(&w, id) {
        w.deliver(Cargo { from: BuildingId::default(), to: id, resource: via, amount: 1.0, price: 0.0 });
    }
    assert_eq!(site_of(&w, id).deliveries, 1);
    w.order_materials(id);
    let frame: Vec<Resource> = site_of(&w, id).due.iter().map(|d| d.good).collect();
    assert!(!frame.is_empty());
    // One line goes missing on the way (as when no road reached the site).
    let lost = site_of(&w, id).due[0];
    w.buildings[id].construction.as_mut().unwrap().due.retain(|d| d.good != lost.good);
    for via in loads_due(&w, id) {
        w.deliver(Cargo { from: BuildingId::default(), to: id, resource: via, amount: 1.0, price: 0.0 });
    }
    let c = site_of(&w, id);
    if frame.len() > 1 {
        assert_eq!(c.deliveries, 1, "the frame is not supplied with a line missing: {c:?}");
        assert!(!c.ordered, "the site may order again");
        let again = w.site_lines_to_order(id);
        assert_eq!(again.len(), 1, "{again:?}");
        assert_eq!(again[0].good, lost.good);
        assert!((again[0].qty - lost.qty).abs() < 0.01);
        w.order_materials(id);
        for via in loads_due(&w, id) {
            w.deliver(Cargo { from: BuildingId::default(), to: id, resource: via, amount: 1.0, price: 0.0 });
        }
    } else {
        w.order_materials(id);
        for via in loads_due(&w, id) {
            w.deliver(Cargo { from: BuildingId::default(), to: id, resource: via, amount: 1.0, price: 0.0 });
        }
    }
    assert_eq!(site_of(&w, id).deliveries, 2, "{:?}", site_of(&w, id));
}

#[test]
fn an_old_saves_parts_load_still_counts() {
    let (mut w, id) = site(LandUse::Residential);
    // A save from before bills: a parts load on the road, nothing due.
    w.buildings[id].construction.as_mut().unwrap().ordered = true;
    w.deliver(Cargo { from: BuildingId::default(), to: id, resource: Resource::Parts, amount: 8.0, price: 0.0 });
    let c = site_of(&w, id);
    assert_eq!(c.deliveries, 1);
    assert!(!c.ordered);
    // An old save loads: the new fields default to empty.
    let ron = w.to_ron().unwrap();
    let back = World::from_ron(&ron).unwrap();
    assert_eq!(back.buildings[id].construction, w.buildings[id].construction);
}

#[test]
fn the_pile_is_used_up_as_the_stage_is_built() {
    let (mut w, id) = site(LandUse::Residential);
    w.order_materials(id);
    for via in loads_due(&w, id) {
        w.deliver(Cargo { from: BuildingId::default(), to: id, resource: via, amount: 1.0, price: 0.0 });
    }
    let full: f32 = site_of(&w, id).on_site().iter().map(|l| l.qty).sum();
    assert!(full > 0.0);
    w.buildings[id].construction.as_mut().unwrap().progress = 0.125;
    let half: f32 = site_of(&w, id).on_site().iter().map(|l| l.qty).sum();
    assert!((half - full * 0.5).abs() < 0.2, "{half} of {full}");
    w.buildings[id].construction.as_mut().unwrap().progress = 0.25;
    assert!(site_of(&w, id).on_site().is_empty(), "the foundations' pile is in the ground");
    // The inspector's view says so.
    let stages = w.site_materials(id);
    assert_eq!(stages.len(), 4);
    assert_eq!(stages[0].state, SiteStageState::Used);
    assert_eq!(stages[1].state, SiteStageState::ToOrder);
}

#[test]
fn site_lorries_wear_the_load() {
    let (mut w, id) = site(LandUse::Residential);
    let _ = &mut w;
    let mut rng = crate::vehicle::model_rng(1, 2);
    let m = crate::vehicle::VehicleModel::for_cargo(Resource::Timber, None, true, false, &mut rng);
    assert_eq!(m.body, crate::vehicle::BodyType::Flatbed);
    assert_eq!(crate::vehicle::Load::from_code(m.boxed), Some(crate::vehicle::Load::Timber));
    assert_eq!(crate::vehicle::chassis_of(m.boxed), None, "a flatbed is not a container rig");
    let m = crate::vehicle::VehicleModel::for_cargo(Resource::Cement, None, true, false, &mut rng);
    assert_eq!(m.body, crate::vehicle::BodyType::Mixer);
    let m = crate::vehicle::VehicleModel::for_cargo(Resource::Aggregate, None, true, false, &mut rng);
    assert_eq!(m.body, crate::vehicle::BodyType::Tipper);
    // The build type the site is built as.
    let f: SiteFacts = w.site_facts(id).unwrap();
    assert_eq!(w.build_type(id), Some(crate::catalogue::build_type(&f)));
}

#[test]
fn boxes_and_loads_say_what_and_how_much() {
    let g = crate::containers::Goods { resource: Resource::Aggregate, buyer: BuildingId::default(), amount: 25.0 };
    assert_eq!(g.describe(), "25 t sand and gravel");
    let old = crate::containers::Goods { amount: 0.0, ..g };
    assert_eq!(old.describe(), "sand and gravel", "an older save's box does not know how much");
    assert_eq!(crate::catalogue::describe_load(&[(Resource::Steel, 8.0), (Resource::Insulation, 3.0)]), "8 t structural steel, 3 pallets insulation");
    assert_eq!(crate::catalogue::describe_load(&[(Resource::Fuel, 0.0)]), "empty");
}
