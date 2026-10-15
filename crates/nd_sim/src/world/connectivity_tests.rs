//! [pt-walk] Stranded areas, failed trips and their alerts.

use super::connectivity::{AlertKind, Net};
use super::*;
use crate::mind::IdleMind;

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// A day for the sites to go up, opened, and another for people to move in.
fn settle(w: &mut World) {
    let mut minds = IdleMind;
    for _ in 0..2 {
        for _ in 0..(24 * 60) {
            w.tick(&mut minds);
        }
        finish_all(w);
    }
}

const T: f32 = crate::roads::TUNNEL_DEPTH;

/// Homes along a street at y = 0, and works along a street at y = 400
/// reached only by a tunnel (no pavement in a bore): Harvey's stranded
/// industrial estate.
fn tunnel_town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 400.0), Vec2::new(400.0, 400.0)], 1, 1);
    let pts = [Vec2::new(-300.0, 0.0), Vec2::new(-300.0, -130.0), Vec2::new(0.0, -130.0), Vec2::new(0.0, 530.0), Vec2::new(150.0, 530.0), Vec2::new(150.0, 400.0)];
    let had = w.project.take();
    w.add_gesture(Gesture::Road { points: pts.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, T, T, T, T, 0.0], car_only: None });
    w.implement();
    w.project = had;
    w.add_zone(vec![Vec2::new(-380.0, 10.0), Vec2::new(380.0, 10.0), Vec2::new(380.0, 120.0), Vec2::new(-380.0, 120.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(-380.0, 410.0), Vec2::new(100.0, 410.0), Vec2::new(100.0, 500.0), Vec2::new(-380.0, 500.0)], LandUse::Commercial);
    settle(&mut w);
    w.refresh_connectivity();
    w
}

fn north_lot(w: &World) -> LotId {
    w.lots.iter().filter(|(_, l)| l.building.is_some() && l.center.y > 300.0).map(|(id, _)| id).next().expect("a building up north")
}

#[test]
fn an_estate_reached_only_by_tunnel_is_stranded_on_foot_not_by_road() {
    let mut w = tunnel_town();
    let north = north_lot(&w);
    let south = w.lots.iter().filter(|(_, l)| l.building.is_some() && l.center.y < 200.0).map(|(id, _)| id).next().expect("homes down south");
    assert!(w.lot_cut_off(north, Net::Walk), "nobody walks through the tunnel");
    assert!(!w.lot_cut_off(north, Net::Road), "cars do");
    assert!(!w.lot_cut_off(south, Net::Walk));
    assert_eq!(w.lot_link_warnings(north), vec!["! not connected to the rest of the pedestrian grid".to_string()]);
    let area = w.stranded_areas().iter().find(|a| a.net == Net::Walk).expect("a stranded area");
    assert!(area.lots.contains(&north) && area.center.y > 300.0);
    assert!(area.describe(&w).contains("not connected to the rest of the pedestrian grid"));
    // The streets: the north one is cut off on foot, both are on the road network.
    let seg_at = |w: &World, p: Vec2| w.roads.nearest_where(p, |s| s.is_street() && !s.tunnel).unwrap().0;
    let n = seg_at(&w, Vec2::new(-200.0, 400.0));
    let s = seg_at(&w, Vec2::new(-200.0, 0.0));
    assert_eq!(w.segment_links(n).walk, Some(false));
    assert_eq!(w.segment_links(n).road, Some(true));
    assert_eq!(w.segment_links(s).walk, Some(true));
    assert!(w.connectivity_segments().iter().any(|(id, l)| *id == n && l.walk == Some(false)), "the overlay hook lists it");
    // A street round the east end joins them up.
    w.add_road(&[Vec2::new(400.0, 0.0), Vec2::new(400.0, 400.0)], 1, 1);
    w.tick(&mut IdleMind);
    let north = north_lot(&w);
    assert!(!w.lot_cut_off(north, Net::Walk), "joined: {:?}", w.stranded_areas().iter().map(|a| a.describe(&w)).collect::<Vec<_>>());
    assert!(w.stranded_areas().is_empty());
}

#[test]
fn a_cut_off_road_network_is_named_too() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 600.0), Vec2::new(0.0, 600.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-380.0, 10.0), Vec2::new(380.0, 10.0), Vec2::new(380.0, 120.0), Vec2::new(-380.0, 120.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(-380.0, 610.0), Vec2::new(-20.0, 610.0), Vec2::new(-20.0, 700.0), Vec2::new(-380.0, 700.0)], LandUse::Residential);
    settle(&mut w);
    w.refresh_connectivity();
    let lone = w.lots.iter().filter(|(_, l)| l.building.is_some() && l.center.y > 500.0).map(|(id, _)| id).next().expect("built up north");
    assert!(w.lot_cut_off(lone, Net::Walk) && w.lot_cut_off(lone, Net::Road));
    let seg = w.roads.nearest_where(Vec2::new(-200.0, 600.0), |s| s.is_street()).unwrap().0;
    assert_eq!(w.segment_link_warnings(seg).len(), 2, "{:?}", w.segment_link_warnings(seg));
}

#[test]
fn trips_with_no_route_say_why_show_as_stuck_and_raise_an_alert() {
    let mut w = tunnel_town();
    let north = north_lot(&w);
    let target = w.lots[north].building.unwrap();
    let name = w.buildings[target].name.clone();
    // Someone on the south pavement with no car: no way there.
    let who: Vec<CitizenId> = w.citizens.keys().take(3).collect();
    assert_eq!(who.len(), 3, "people moved in");
    let tick0 = w.connectivity.alerts.len();
    for &id in &who {
        w.leave_building(id);
        let c = &mut w.citizens[id];
        c.inside = None;
        c.pos = Vec2::new(-100.0, 9.0);
        c.owns_car = false;
        c.owns_bike = false;
        c.activity = Activity::Idle;
        w.travel_with(id, Some(target), Purpose::Visit(LandUse::Commercial), false, true, false);
        let c = &w.citizens[id];
        assert!(matches!(c.activity, Activity::Idle), "no way: {:?}", c.activity.label());
        assert_eq!(c.thought, format!("No way through: {name} isn't connected to the rest of the pedestrian grid."));
        assert_eq!(w.activity_line(id), format!("stuck: no way to get to {name}"), "not merely loitering");
    }
    let alerts: Vec<_> = w.connectivity.alerts.iter().skip(tick0).filter(|a| a.kind == AlertKind::NoRoute).collect();
    assert_eq!(alerts.len(), 1, "the third failure today raises one alert");
    assert!(alerts[0].text.starts_with(&format!("No route found: 3 trips to {name} failed today")), "{}", alerts[0].text);
    // The stranded area itself was alerted once, with its people.
    assert_eq!(w.connectivity.alerts.iter().filter(|a| a.kind == AlertKind::Stranded(Net::Walk)).count(), 1);
}

/// Homes and a park side by side along a street, and an industrial estate
/// further along: for the walk-access and stroll tests.
fn park_town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-600.0, 10.0), Vec2::new(-200.0, 10.0), Vec2::new(-200.0, 110.0), Vec2::new(-600.0, 110.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(-200.0, 10.0), Vec2::new(-100.0, 10.0), Vec2::new(-100.0, 110.0), Vec2::new(-200.0, 110.0)], LandUse::Park);
    w.add_zone(vec![Vec2::new(-600.0, -10.0), Vec2::new(-100.0, -10.0), Vec2::new(-100.0, -110.0), Vec2::new(-600.0, -110.0)], LandUse::Commercial);
    w.add_zone(vec![Vec2::new(0.0, 10.0), Vec2::new(650.0, 10.0), Vec2::new(650.0, 160.0), Vec2::new(0.0, 160.0)], LandUse::Industrial);
    w.add_zone(vec![Vec2::new(0.0, -10.0), Vec2::new(650.0, -10.0), Vec2::new(650.0, -160.0), Vec2::new(0.0, -160.0)], LandUse::Industrial);
    settle(&mut w);
    w.update_land_value();
    w
}

/// [pt-walk] F2, F30: a home beside a park, and the park itself, read as a
/// park within a minute or two on foot, never "no parks within walking
/// reach".
#[test]
fn a_park_next_door_is_within_walking_reach() {
    let w = park_town();
    let park = w.buildings.values().find(|b| b.land_use == LandUse::Park && b.operating()).map(|b| b.lot).expect("a park");
    let a = w.lot_walk_access(park).expect("access on the park");
    assert_eq!(a.park_min, Some(0.0), "on the park: {}", a.describe());
    let pc = w.lots[park].center;
    let next = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.building.is_some()).min_by(|a, b| a.1.center.distance(pc).total_cmp(&b.1.center.distance(pc))).map(|(id, _)| id).expect("a home");
    let a = w.lot_walk_access(next).expect("access next door");
    assert!(a.park_min.is_some_and(|m| m <= 3.0), "next door: {}", w.lot_access_text(next).unwrap());
    assert!(!w.lot_access_text(next).unwrap().contains("no parks"));
    // Every home on the street reaches it.
    for (id, l) in w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.building.is_some()) {
        let a = w.lot_walk_access(id).expect("access");
        assert!(a.park_min.is_some(), "lot at {:?}: {}", l.center, a.describe());
    }
}

/// [pt-walk] F22: a stroll heads somewhere pleasant within a walk (the
/// park, the shops), not to any walk node in town: hardly ever into the
/// industrial estate next door, and never across the city.
#[test]
fn strolls_stay_near_and_shun_the_industrial_estate() {
    let w = park_town();
    let id = w.citizens.keys().next().expect("people");
    let here = Vec2::new(-60.0, 12.0);
    let mut industrial = 0;
    let mut n = 0;
    for draw in 0..300u64 {
        let Some(t) = w.wander_target(id, here, draw * 7919) else { continue };
        n += 1;
        assert!(t.distance(here) <= super::trips::WANDER_REACH + 1.0, "within a stroll: {t}");
        let lot = w.lots.values().min_by(|a, b| a.center.distance(t).total_cmp(&b.center.distance(t))).unwrap();
        if lot.land_use == LandUse::Industrial {
            industrial += 1;
        }
    }
    assert!(n > 250, "a target nearly every time: {n}");
    // About half the walk nodes in reach are on the estate's frontage.
    assert!(industrial * 10 < n, "{industrial} of {n} strolls into the industrial estate");
}

/// [park-access] Residents are counted by their walk to a park, a park lot
/// itself is never short of one, and homes without a park are counted.
#[test]
fn park_access_summary_counts_residents_by_walk() {
    use super::connectivity::{ParkAccessRow, ParkAccessSummary};
    let lot = LotId::default();
    let row = |minutes: Option<f32>, residents: u32| ParkAccessRow { lot, minutes, residents, park: false };
    let rows = [row(Some(3.0), 10), row(Some(8.0), 20), row(Some(14.0), 30), row(Some(40.0), 15), row(None, 25), row(None, 0)];
    let s = ParkAccessSummary::of(&rows);
    assert_eq!(s.residents, 100);
    assert_eq!((s.within_5, s.within_10, s.within_15, s.none), (10, 30, 60, 25));
    assert_eq!(s.homes_without, 1, "an empty lot is not a home");
    assert!((s.share(s.within_15) - 0.6).abs() < 1e-6);
    assert_eq!(ParkAccessSummary::default().share(0), 0.0);
}
