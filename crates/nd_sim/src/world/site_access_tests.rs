//! [r2-traffic] Building sites no truck can get to (Harvey's flag K6: an
//! island's buildings went up with no delivery truck ever seen): they do
//! not build, say so, and go on once a road leads there.

use super::*;
use super::connectivity::AlertKind;
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

#[test]
fn a_site_no_truck_can_reach_waits_says_so_and_builds_once_a_road_leads_there() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(0.0, 0.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    // An "island": a ring of streets joined to nothing.
    let ring = [Vec2::new(200.0, 200.0), Vec2::new(500.0, 200.0), Vec2::new(500.0, 500.0), Vec2::new(200.0, 500.0)];
    for k in 0..4 {
        w.add_road(&[ring[k], ring[(k + 1) % 4]], 1, 1);
    }
    w.add_zone(square(210.0, 210.0, 160.0), LandUse::Commercial);
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.center.x > 150.0).map(|(id, _)| id).next();
    let lot = lot.unwrap_or_else(|| panic!("a lot on the island: {:?}", w.lots.values().filter(|l| l.center.x > 150.0).map(|l| (l.center, l.land_use, l.landlocked, l.usable())).collect::<Vec<_>>()));
    w.build_on(lot);
    let id = w.lots[lot].building.unwrap();
    run(&mut w, 24 * 60);
    let c = w.buildings[id].construction.clone().expect("still a site");
    assert_eq!(c.progress, 0.0, "nothing built on parts nobody brought: {c:?}");
    assert_eq!(c.deliveries, 0, "no load dropped on the site by magic");
    assert_eq!(c.blocked, Some(crate::SiteBlock::Builders), "{c:?}");
    assert!(w.blocked_sites().iter().any(|s| s.0 == id), "marked on the map");
    assert!(w.connectivity.alerts.iter().any(|a| a.kind == AlertKind::SiteBlocked && a.building == Some(id)), "an alert");
    // A road to the island: the trucks come and the work goes on.
    w.add_road(&[Vec2::new(0.0, 0.0), Vec2::new(200.0, 200.0)], 1, 1);
    let mut trucks = 0;
    for _ in 0..24 * 60 {
        run(&mut w, 1);
        trucks += w.traffic.cars.values().filter(|v| v.cargo.is_some_and(|g| g.to == id)).count().min(1);
        if w.buildings[id].construction.as_ref().is_none_or(|c| c.deliveries >= 1 && c.progress > 0.0) {
            break;
        }
    }
    let c = w.buildings[id].construction.clone();
    assert!(c.as_ref().is_none_or(|c| c.blocked.is_none() && c.deliveries >= 1 && c.progress > 0.0), "{c:?}");
    assert!(trucks > 0, "a delivery truck drove there");
}
