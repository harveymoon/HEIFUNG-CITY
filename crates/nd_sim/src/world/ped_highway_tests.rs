//! [ped-highway] Nobody walks on, waits on or is left on a highway or a
//! slip road: the walk graph keeps off their carriageways (but at a ramp's
//! mouth, crossed as a crossing), cab stops are on streets, people set
//! down out there are put on the pavement, and a rider no cab comes for
//! gives up and goes another way.

use super::*;
use crate::citizens::TripMode;
use crate::mind::IdleMind;
use crate::taxi::{self, CabJob, CabPlace, TaxiRequest};
use crate::walk::NO_CROSSING;

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// An eastbound highway along y = 0, a two-way street along y = -160, an
/// off-ramp from the highway down to the street and an on-ramp back up, a
/// cross street south from the middle, and a footpath from the street
/// dropped onto the off-ramp's bend. Homes and shops south of the street.
fn interchange() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 3, 0);
    w.add_road(&[Vec2::new(-600.0, -160.0), Vec2::new(600.0, -160.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, -5.0), Vec2::new(-150.0, -60.0), Vec2::new(-80.0, -160.0)], 1, 0);
    w.add_road(&[Vec2::new(80.0, -160.0), Vec2::new(150.0, -60.0), Vec2::new(300.0, -5.0)], 1, 0);
    w.add_road(&[Vec2::new(0.0, -160.0), Vec2::new(0.0, -420.0)], 1, 1);
    w.add_path(&[Vec2::new(-300.0, -160.0), Vec2::new(-150.0, -60.0)]);
    w.add_zone(vec![Vec2::new(-560.0, -175.0), Vec2::new(-20.0, -175.0), Vec2::new(-20.0, -400.0), Vec2::new(-560.0, -400.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(20.0, -175.0), Vec2::new(560.0, -175.0), Vec2::new(560.0, -400.0), Vec2::new(20.0, -400.0)], LandUse::Commercial);
    w
}

/// The interchange with people living there and a shop built.
fn lived_in() -> World {
    let mut w = interchange();
    run(&mut w, 8 * 60);
    finish_all(&mut w);
    run(&mut w, 60);
    assert!(w.citizens.values().filter(|c| c.home.is_some() && c.can_work()).count() >= 4, "people moved in: {}", w.citizens.len());
    w
}

fn motorways(w: &World) -> Vec<SegmentId> {
    w.roads.alive_segments().filter(|(_, s)| s.is_motorway()).map(|(id, _)| id).collect()
}

/// A point on the off-ramp's carriageway, half way down.
fn on_the_ramp(w: &World) -> Vec2 {
    let sid = w.roads.alive_segments().find(|(_, s)| s.ramp).map(|(id, _)| id).expect("a ramp");
    let (a, b) = w.roads.segment_points(sid);
    a.lerp(b, 0.5)
}

/// Someone standing about outdoors at `p`, free to decide now.
fn stood_at(w: &mut World, p: Vec2, k: usize) -> CitizenId {
    let id = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id).nth(k).expect("a grown-up");
    if let Some(vid) = w.citizens[id].car {
        w.drop_car(vid, crate::vehicle::CarEvent::Sold);
    }
    let c = &mut w.citizens[id];
    c.owns_bike = false;
    c.inside = None;
    c.transit = None;
    c.pos = p;
    c.prev_pos = p;
    c.activity = Activity::Idle;
    c.next_decision_tick = u64::MAX;
    c.thought.clear();
    id
}

#[test]
fn no_walk_edge_lies_on_a_highway_or_ramp_carriageway() {
    let w = interchange();
    let ms = motorways(&w);
    assert!(ms.iter().any(|s| w.roads.segment(*s).ramp) && ms.iter().any(|s| w.roads.segment(*s).is_highway()));
    let g = &w.walk;
    for (i, n) in g.nodes.iter().enumerate() {
        assert!(!w.stranded_on_motorway(n.pos) || n.level != 0.0, "walk node {i} out on a carriageway at {:?}", n.pos);
        for e in &g.adj[i] {
            let m = g.nodes[e.to as usize].pos;
            if e.raised {
                continue;
            }
            // A ramp's mouth is crossed at a crossing; nothing else runs
            // over a carriageway.
            if e.cross != NO_CROSSING {
                continue;
            }
            for k in 1..8 {
                let q = n.pos.lerp(m, k as f32 / 8.0);
                assert!(!w.stranded_on_motorway(q), "walk edge {i}->{} runs over a carriageway at {q:?}", e.to);
            }
        }
    }
    // The footpath dropped onto the ramp ends short of it: no node at the
    // ramp's bend.
    let bend = Vec2::new(-150.0, -60.0);
    assert!(g.nodes.iter().all(|n| n.pos.distance(bend) > 3.0), "a walk node on the ramp's bend");
}

#[test]
fn taxi_stops_are_on_streets_never_on_a_ramp_or_highway() {
    let mut w = lived_in();
    let ramp = on_the_ramp(&w);
    for p in [ramp, ramp + Vec2::new(4.0, 0.0), Vec2::new(-150.0, -6.0), Vec2::new(200.0, -40.0)] {
        let id = stood_at(&mut w, p, 0);
        let Some((seg, at, wait_at)) = w.taxi_pickup(id, p) else { continue };
        let s = w.roads.segment(seg);
        assert!(s.is_street() && !s.fronts_nothing(), "a cab stop on {seg:?} (ramp {}, highway {})", s.ramp, s.is_highway());
        assert!(!w.stranded_on_motorway(at), "the stop {at:?} is on a carriageway");
        assert!(!w.stranded_on_motorway(wait_at), "the rider waits on a carriageway at {wait_at:?}");
        // The wait is on the pavement beside the stop.
        assert!(wait_at.distance(at) < s.half_width() + crate::zoning::SIDEWALK, "waits {:.1} m from the stop", wait_at.distance(at));
    }
}

#[test]
fn someone_stranded_on_a_ramp_is_put_on_the_pavement_and_sets_off() {
    let mut w = lived_in();
    let ramp = on_the_ramp(&w);
    assert!(w.stranded_on_motorway(ramp));
    // Dropped there by a vehicle that went (as a cab or bus taken off the
    // road used to): on the pavement instead.
    let a = stood_at(&mut w, ramp, 0);
    w.strand_driver(a, ramp);
    assert!(!w.stranded_on_motorway(w.citizens[a].pos), "set down on the carriageway at {:?}", w.citizens[a].pos);
    // Already out there (an older save): the sweep moves them, and they
    // walk somewhere rather than loiter for ever.
    let b = stood_at(&mut w, ramp + Vec2::new(1.0, 0.0), 1);
    w.citizens[b].next_decision_tick = w.clock.tick + 1;
    run(&mut w, kerbside::STRANDED_SWEEP_EVERY as usize + 1);
    assert!(!w.stranded_on_motorway(w.citizens[b].pos), "still on the ramp at {:?}", w.citizens[b].pos);
    // A trip planned from out on the ramp starts from the pavement, so a
    // foot path exists.
    let c = stood_at(&mut w, ramp + Vec2::new(0.0, 1.0), 2);
    let home = w.citizens[c].home.unwrap();
    w.travel_to(c, Some(home), Purpose::Home);
    assert_ne!(w.citizens[c].thought, "No way through. The grid doesn't connect.");
    assert!(matches!(w.citizens[c].activity, Activity::Traveling { .. } | Activity::Waiting { .. } | Activity::Driving { .. }), "{:?}", w.citizens[c].activity);
    // On load too.
    let d = stood_at(&mut w, ramp + Vec2::new(-1.0, 0.0), 3);
    let w2 = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(!w2.stranded_on_motorway(w2.citizens[d].pos), "a save's stranded walker stays out there");
}

#[test]
fn a_cab_parked_on_a_ramp_goes_back_to_its_yard() {
    let mut w = lived_in();
    let lot = w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable() && l.land_use == LandUse::Commercial && l.car_segment.is_some() && l.area() >= taxi::YARD_MIN_AREA).map(|(id, _)| id).next().expect("a vacant shop lot");
    let company = w.build_taxi_company(lot, "Ramp Cabs".into()).expect("a company");
    finish_all(&mut w);
    let sid = w.roads.alive_segments().find(|(_, s)| s.ramp).map(|(id, _)| id).unwrap();
    let cab = w.taxis.fleet(company).map(|(id, _)| id).next().unwrap();
    let pos = on_the_ramp(&w);
    w.taxis.cabs[cab].place = CabPlace::Kerb { segment: sid, pos, heading: 0.0 };
    w.taxis.cabs[cab].job = CabJob::Idle;
    run(&mut w, kerbside::STRANDED_SWEEP_EVERY as usize + 1);
    assert!(!matches!(w.taxis.cabs[cab].place, CabPlace::Kerb { segment, .. } if segment == sid), "the cab still stands on the ramp");
}

#[test]
fn a_rider_no_cab_comes_for_gives_up_and_goes_another_way() {
    let mut w = lived_in();
    let lot = w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable() && l.land_use == LandUse::Commercial && l.car_segment.is_some() && l.area() >= taxi::YARD_MIN_AREA).map(|(id, _)| id).next().expect("a vacant shop lot");
    let company = w.build_taxi_company(lot, "Slow Cabs".into()).expect("a company");
    finish_all(&mut w);
    let r = stood_at(&mut w, Vec2::new(-300.0, -172.0), 0);
    let home = w.citizens[r].home.unwrap();
    let (seg, at, wait_at) = w.taxi_pickup(r, w.citizens[r].pos).expect("a kerb");
    w.hail_taxi(r, seg, at, wait_at, home, Purpose::Home, true);
    // No cab can come: every one off duty in the yard.
    for (_, c) in w.taxis.cabs.iter_mut().filter(|(_, c)| c.company == company) {
        c.on_duty = false;
        c.place = CabPlace::Yard;
        c.job = CabJob::Idle;
    }
    w.taxis.requests.retain(|q| q.rider == r);
    assert!(w.taxis.requests.iter().any(|q: &TaxiRequest| q.rider == r));
    let mut gave_up = false;
    let mut minds = IdleMind;
    for _ in 0..(taxi::WAIT_GIVE_UP * 6) {
        for (_, c) in w.taxis.cabs.iter_mut() {
            c.on_duty = false;
        }
        w.tick(&mut minds);
        if !w.taxis.requests.iter().any(|q| q.rider == r) && !matches!(w.citizens[r].transit.map(|t| t.leg), Some(TransitLeg::Taxi)) {
            gave_up = true;
            break;
        }
    }
    assert!(gave_up, "still waiting for a cab: {:?}", w.citizens[r].activity);
    assert!(w.citizens[r].memories.iter().any(|m| m.text.contains("cab that never came")));
    // Off another way, and no new booking for a while.
    assert!(!matches!(w.citizens[r].activity, Activity::Waiting { .. }), "{:?}", w.citizens[r].activity);
    let from = w.citizens[r].pos;
    let to = w.building_entrance(home).unwrap();
    let q = super::trips::TripQuery { from, to, target: home, purpose: Purpose::Home, allow_transit: true, allow_car: true, allow_bike: true };
    assert!(w.mode_options(r, &q).iter().all(|o| o.mode != TripMode::Taxi), "books another cab straight away");
}

#[test]
fn a_trip_beside_a_footpath_dropped_onto_a_ramp_does_not_start_on_the_ramp() {
    let mut w = lived_in();
    // Beside the footpath, a few metres short of where it meets the ramp.
    let id = w.citizens.keys().next().unwrap();
    let p = Vec2::new(-160.0, -62.0);
    let c = &mut w.citizens[id];
    c.inside = None;
    c.pos = p;
    let (sid, _, node) = w.origin_for(id).expect("an origin");
    assert!(!w.roads.segment(sid).fronts_nothing(), "starts on a carriageway");
    let on_ramp = w.roads.node(node).segments.iter().any(|&s| w.roads.segment(s).alive && w.roads.segment(s).fronts_nothing());
    assert!(!on_ramp, "the trip starts from the ramp's node at {:?}", w.roads.node(node).pos);
}
