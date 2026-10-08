//! [committee] Tests of the deterministic half: appointments, the alert
//! rules, old saves. Meetings, votes and projects are tested in nd_minds,
//! which drives them.

use super::rules::{firing, raise_alerts, RULES};
use super::*;
use crate::WorldConfig;

fn committee() -> Committee {
    let mut c = Committee::default();
    c.ensure(7, 0);
    c
}

#[test]
fn the_first_committee_is_seated_and_the_mayor_reshuffles_it() {
    let mut c = committee();
    assert_eq!(c.seated_ids().len(), SEATS, "six seats filled at the start");
    assert_eq!(c.next_meeting_day, MEETING_INTERVAL_DAYS);
    let scopes: Vec<Scope> = c.seated().iter().map(|p| p.scope).collect();
    for s in [Scope::Transit, Scope::Housing, Scope::Finance, Scope::Safety, Scope::Environment, Scope::Business] {
        assert!(scopes.contains(&s), "{s:?} has a seat");
    }
    // Dismiss: the seat empties, the record counts it.
    let out = c.seats[2].unwrap();
    assert_eq!(c.dismiss(out, 5), Ok(2));
    assert_eq!(c.seats[2], None);
    assert_eq!(c.member(out).unwrap().record.times_dismissed, 1);
    assert!(c.member(out).unwrap().record.seated_since.is_none());
    assert!(c.dismiss(out, 5).is_err(), "not seated any more");
    // Appoint into the empty seat.
    let new = c.candidates().iter().find(|p| p.id != out).unwrap().id;
    assert_eq!(c.appoint(new, None, 6), Ok(2));
    assert_eq!(c.member(new).unwrap().record.seated_since, Some(6));
    assert!(c.appoint(new, None, 6).is_err(), "already seated");
    assert!(c.appoint(new + 1000, None, 6).is_err(), "not in the pool");
    // Full: appointing needs a named seat, which replaces its holder.
    let another = c.candidates()[0].id;
    assert!(c.appoint(another, None, 7).is_err());
    let replaced = c.seats[0].unwrap();
    assert_eq!(c.appoint(another, Some(0), 7), Ok(0));
    assert!(c.seat_of(replaced).is_none());
    assert!(c.appoint(replaced, Some(SEATS), 7).is_err(), "no seventh seat");
    // A dismissed member keeps their memory.
    c.member_mut(replaced).unwrap().remember(MemoryEntry { proposal: "X".into(), ..Default::default() });
    c.appoint(replaced, Some(0), 8).unwrap();
    assert_eq!(c.member(replaced).unwrap().memory.len(), 1);
    assert_eq!(c.member(replaced).unwrap().record.times_appointed, 2);
}

#[test]
fn memory_is_bounded() {
    let mut p = generate_pool(3).remove(0);
    for i in 0..30 {
        p.remember(MemoryEntry { day: i, proposal: format!("P{i}"), ..Default::default() });
    }
    assert_eq!(p.memory.len(), MEMORY_KEPT);
    assert_eq!(p.memory.last().unwrap().proposal, "P29");
}

fn quiet_report() -> CityReport {
    CityReport {
        population: 1000,
        treasury: 100_000.0,
        income_per_day: 2000.0,
        spending_per_day: 1500.0,
        net_per_day: 500.0,
        homes: 400,
        homes_free: 40,
        riders_per_day: 300.0,
        riders_prev: 300.0,
        bus_lines: 2,
        depots: 1,
        coverage: [0.9; 4],
        power_served: 1.0,
        road_funding: 1.0,
        ..Default::default()
    }
}

#[test]
fn a_healthy_city_raises_no_alerts() {
    let c = committee();
    let f = firing(&quiet_report(), &c, 10);
    assert!(f.is_empty(), "{:?}", f.iter().map(|f| f.0.key).collect::<Vec<_>>());
}

#[test]
fn alert_rules_fire_on_their_thresholds() {
    let c = committee();
    let fires = |r: &CityReport, key: &str| firing(r, &c, 10).iter().any(|f| f.0.key == key);
    let mut r = quiet_report();
    r.riders_per_day = 241.0;
    assert!(!fires(&r, "transit_riders_down"), "down 19.7%: quiet");
    r.riders_per_day = 239.0;
    assert!(fires(&r, "transit_riders_down"), "down 20.3%: alert");
    let mut r = quiet_report();
    r.unpowered = 1;
    assert!(fires(&r, "utilities_unpowered"), "one dark building is enough");
    let mut r = quiet_report();
    r.rent_burden = 0.34;
    assert!(!fires(&r, "housing_rent_burden"));
    r.rent_burden = 0.36;
    assert!(fires(&r, "housing_rent_burden"));
    let mut r = quiet_report();
    r.crime_prev = 4.0;
    r.crime_per_day = 4.9;
    assert!(!fires(&r, "safety_crime_up"));
    r.crime_per_day = 5.0;
    assert!(fires(&r, "safety_crime_up"), "up 25%");
    let mut r = quiet_report();
    r.net_per_day = -60.0;
    assert!(fires(&r, "finance_deficit"));
    let mut r = quiet_report();
    r.unemployment = 0.11;
    assert!(fires(&r, "business_unemployment"));
    let mut r = quiet_report();
    r.coverage[0] = 0.4;
    assert!(fires(&r, "safety_police_coverage"));
    // Contamination spreading compares with a week-old sample.
    let mut c2 = committee();
    c2.push_sample(Sample { day: 2, contaminated_cells: 10, ..Default::default() });
    let mut r = quiet_report();
    r.contaminated_cells = 11;
    assert!(!firing(&r, &c2, 10).iter().any(|f| f.0.key == "environment_contamination_spreading"));
    r.contaminated_cells = 12;
    assert!(firing(&r, &c2, 10).iter().any(|f| f.0.key == "environment_contamination_spreading"));
    for rule in RULES {
        assert!(!rule.when.is_empty() && !rule.key.is_empty());
    }
}

#[test]
fn alerts_come_from_the_member_who_watches_and_respect_cooldowns() {
    let mut c = committee();
    let mut r = quiet_report();
    r.unpowered = 3;
    r.coverage[0] = 0.3;
    // Nobody holds Utilities in the first committee: no alert for the dark
    // buildings; Safety is seated and raises police cover.
    let ids = raise_alerts(&mut c, &r, 10);
    let rules: Vec<&str> = c.alerts.iter().map(|a| a.rule.as_str()).collect();
    assert!(rules.contains(&"safety_police_coverage"));
    assert!(!rules.contains(&"utilities_unpowered"), "no Utilities member, no alert");
    let a = c.alerts.iter().find(|a| a.rule == "safety_police_coverage").unwrap();
    assert_eq!(c.member(a.member).unwrap().scope, Scope::Safety);
    assert!(!a.text.is_empty() && !a.facts.is_empty());
    assert_eq!(ids.len(), c.alerts.len());
    // Cooldown: not again the next day.
    assert!(raise_alerts(&mut c, &r, 11).is_empty());
    // Seat a Utilities member: now the dark buildings are reported.
    let u = c.pool.iter().find(|p| p.scope == Scope::Utilities).unwrap().id;
    c.appoint(u, Some(5), 11).unwrap();
    let ids = raise_alerts(&mut c, &r, 12);
    assert!(ids.iter().any(|id| c.alerts.iter().any(|a| a.id == *id && a.rule == "utilities_unpowered" && a.member == u)));
    // After the cooldown the police alert returns.
    assert!(raise_alerts(&mut c, &r, 24).iter().any(|id| c.alerts.iter().any(|a| a.id == *id && a.rule == "safety_police_coverage")));
}

#[test]
fn an_old_save_loads_with_no_committee_and_sets_one_up() {
    let w = World::new(WorldConfig::default());
    let text = w.to_ron().unwrap();
    let at = text.rfind("committee:").expect("the field is saved");
    let mut old = text[..at].trim_end().trim_end_matches(',').to_string();
    old.push(')');
    assert!(!old.contains("committee"));
    let mut back = World::from_ron(&old).expect("an old save loads");
    assert!(!back.committee.is_set_up());
    back.committee_ensure();
    assert_eq!(back.committee.seated_ids().len(), SEATS);
    // Same world seed, same pool.
    let mut again = World::from_ron(&old).unwrap();
    again.committee_ensure();
    assert_eq!(back.committee.pool, again.committee.pool);
}

#[test]
fn the_report_reads_an_empty_city() {
    let w = World::new(WorldConfig::default());
    let r = CityReport::build(&w);
    assert_eq!(r.population, 0);
    assert!(r.to_text().contains("CITY REPORT"));
    assert_eq!(r.areas.len(), 5, "Downtown and four sides");
}
