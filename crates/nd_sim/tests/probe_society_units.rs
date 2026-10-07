//! Regression (review 2, C1 and H3): what happens to an incident (and the
//! sim) when the station of the unit answering it disappears mid-job. The
//! review's probe burned the station down; civic structures no longer burn
//! down (H2), so here the station is bulldozed (removed outright, as the
//! player's bulldozer does). The tick must not panic, and no incident may
//! be left pinned to a unit that is gone. (The probes asserted the defects;
//! they now assert the fix.)
//!
//! cargo test --release -p nd_sim --test probe_society_units -- --nocapture --test-threads=1

use nd_minds::RuleBasedMind;
use nd_sim::services::{CrimeKind, IncidentKind, Outcome, Stage};
use nd_sim::{Activity, BuildingId, LandUse, Placeable, ServiceKind, Tier, World};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[path = "demo_city.rs"]
#[allow(dead_code)]
mod demo;

fn place(w: &mut World, kind: ServiceKind, tier: Tier, uses: &[LandUse]) -> BuildingId {
    let what = Placeable::Service(kind, tier);
    let lot = w
        .lots
        .iter()
        .filter(|(id, l)| uses.contains(&l.land_use) && l.building.is_none() && w.placement_problem(what, *id).is_none())
        .map(|(id, _)| id)
        .next()
        .or_else(|| w.lots.iter().filter(|(id, l)| uses.contains(&l.land_use) && w.placement_problem(what, *id).is_none()).map(|(id, _)| id).next())
        .expect("a lot");
    w.build_service(lot, kind, tier).unwrap()
}

fn warm_world() -> (World, RuleBasedMind) {
    let mut w = demo::demo_world();
    w.economy.treasury = 1e7;
    let mut minds = RuleBasedMind::default();
    // Grow a town first.
    for _ in 0..(4 * 1440 + 600) {
        w.tick(&mut minds);
    }
    (w, minds)
}

/// Take station `b` away at once, as the bulldozer does a civic structure.
fn bulldoze(w: &mut World, b: BuildingId) {
    let lot = w.buildings[b].lot;
    for c in w.buildings[b].workers.clone() {
        w.citizens[c].work = None;
    }
    w.buildings.remove(b);
    if let Some(l) = w.lots.get_mut(lot) {
        l.building = None;
    }
}

fn patient_candidate(w: &World, avoid: &[BuildingId]) -> Option<nd_sim::CitizenId> {
    w.citizens
        .iter()
        .filter(|(id, c)| matches!(c.activity, Activity::AtHome) && c.inside.is_some_and(|b| !avoid.contains(&b)) && !w.is_patient(*id))
        .map(|(id, _)| id)
        .next()
}

/// No active incident names a unit that does not exist (or no longer has it).
fn assert_no_orphans(w: &World) {
    for i in w.services.active() {
        if let Some(u) = i.unit {
            let ok = w.services.units.get(u).is_some_and(|x| x.incident == Some(i.id));
            assert!(ok || !matches!(i.stage, Stage::Dispatched | Stage::OnScene | Stage::Transporting), "incident {} {:?} names unit {:?} that is gone or off it", i.id, i.stage, u);
        }
    }
}

#[test]
fn an_ambulance_whose_hospital_is_bulldozed_while_on_scene_finishes_the_job() {
    let (mut w, mut minds) = warm_world();
    let h1 = place(&mut w, ServiceKind::Health, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
    let h2 = place(&mut w, ServiceKind::Health, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
    let _d = place(&mut w, ServiceKind::Dispatch, Tier::Low, &[LandUse::Civic, LandUse::Commercial]);
    for _ in 0..240 {
        w.tick(&mut minds);
    }
    println!("pop {} crewed h1 {} h2 {}", w.citizens.len(), w.crewed_units(h1), w.crewed_units(h2));
    for attempt in 0..30 {
        let Some(p) = patient_candidate(&w, &[h1, h2]) else { panic!("no patient") };
        let Some(id) = w.fall_ill(p) else { continue };
        let mut on_scene = None;
        for _ in 0..400 {
            w.tick(&mut minds);
            let i = w.services.incident(id).unwrap();
            if i.stage == Stage::OnScene && i.unit.is_some() {
                on_scene = i.unit;
                break;
            }
            if !i.active() {
                break;
            }
        }
        let Some(uid) = on_scene else { continue };
        let station = w.services.units[uid].station;
        let other = if station == h1 { h2 } else { h1 };
        println!("attempt {attempt}: incident {id} on scene, unit from {:?}; bulldozing the station, other hospital {:?} has beds", station, other);
        w.services.incident_mut(id).unwrap().work_left = 60.0;
        bulldoze(&mut w, station);
        let r = catch_unwind(AssertUnwindSafe(|| {
            for t in 0..400 {
                w.tick(&mut minds);
                assert_no_orphans(&w);
                if t % 20 == 0 {
                    let i = w.services.incident(id).unwrap();
                    println!("  t+{t}: unit exists {} incident {:?} work_left {:.0}", w.services.units.contains_key(uid), i.stage, i.work_left);
                }
            }
        }));
        if let Err(e) = r {
            let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            panic!("tick panicked after the ambulance's station was bulldozed: {msg}");
        }
        let i = w.services.incident(id).unwrap();
        println!("incident {:?} {:?}", i.stage, i.outcome);
        assert!(!i.active(), "the patient was seen to");
        assert!(matches!(i.outcome, Some(Outcome::Admitted | Outcome::TreatedOnSite | Outcome::Gone)), "{:?}", i.outcome);
        return;
    }
    panic!("never got an ambulance on scene");
}

#[test]
fn calls_are_not_orphaned_when_their_units_station_is_bulldozed() {
    let (mut w, mut minds) = warm_world();
    let police = place(&mut w, ServiceKind::Police, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
    let hosp = place(&mut w, ServiceKind::Health, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
    let _d = place(&mut w, ServiceKind::Dispatch, Tier::Low, &[LandUse::Civic, LandUse::Commercial]);
    for _ in 0..240 {
        w.tick(&mut minds);
    }
    println!("pop {} crewed police {} hospital {}", w.citizens.len(), w.crewed_units(police), w.crewed_units(hosp));
    // A theft at a shop, and someone taken ill at home.
    let shop = w.buildings.iter().filter(|(_, b)| b.business.is_some() && b.service.is_none() && b.construction.is_none()).map(|(id, _)| id).next().unwrap();
    let crime = w.report_crime(CrimeKind::Theft, shop, None, None).unwrap();
    let patient = patient_candidate(&w, &[police, hosp]).unwrap();
    let med = w.fall_ill(patient).unwrap();
    let (mut crime_unit, mut med_unit) = (None, None);
    for _ in 0..300 {
        w.tick(&mut minds);
        let c = w.services.incident(crime).unwrap();
        if crime_unit.is_none() && c.stage == Stage::Dispatched && c.unit.is_some() {
            crime_unit = c.unit;
            bulldoze(&mut w, police);
        }
        let m = w.services.incident(med).unwrap();
        if med_unit.is_none() && m.stage == Stage::Dispatched && m.unit.is_some() {
            med_unit = m.unit;
            bulldoze(&mut w, hosp);
        }
        if crime_unit.is_some() && med_unit.is_some() {
            break;
        }
    }
    println!("crime unit {:?} medical unit {:?}", crime_unit, med_unit);
    assert!(crime_unit.is_some() && med_unit.is_some(), "both calls were answered before the stations went");
    for _ in 0..2000 {
        w.tick(&mut minds);
        assert_no_orphans(&w);
    }
    for (name, id) in [("crime", crime), ("medical", med)] {
        let i = w.services.incident(id).unwrap();
        println!("{name} incident {id}: stage {:?} active {} unit {:?} outcome {:?} kind {:?}", i.stage, i.active(), i.unit, i.outcome, i.kind);
        if i.kind == IncidentKind::Medical {
            let c = &w.citizens[patient];
            println!("  patient inside {:?} activity {:?} next_decision_tick {} (now {}) is_patient {}", c.inside, c.activity, c.next_decision_tick, w.clock.tick, w.is_patient(patient));
        }
    }
    let stuck: Vec<u32> = [crime, med].into_iter().filter(|id| w.services.incident(*id).is_some_and(|i| i.active())).collect();
    assert!(stuck.is_empty(), "incidents {stuck:?} still active 2000 ticks after their unit's station was bulldozed");
    // The units of the bulldozed stations left town (at the map edge) or
    // are on their way out; none is still on duty for a station that is gone.
    for (_, u) in w.services.units.iter().filter(|(_, u)| u.station == police || u.station == hosp) {
        assert!(u.leaving, "a unit of a bulldozed station is leaving town: {:?}", u.state);
    }
}
