//! [perfect-city] The Perfect AI City preset: the plan is deterministic,
//! the network and zoning pass the shared checks, every placement the
//! furnishing makes takes, the save round-trips, and a short run grows
//! with the lights on.
//!
//! cargo test --profile fast -p nd_procgen --test citygen_perfect_city -- --nocapture

mod citygen_common;

use citygen_common::{check_city, check_deterministic};
use nd_minds::RuleBasedMind;
use nd_procgen::citygen::{self, CityPreset};
use nd_sim::{LandUse, PowerPlantKind, RailFacility, ServiceKind, World};

const PRESET: CityPreset = CityPreset::PerfectCity;

#[test]
fn deterministic() {
    check_deterministic(PRESET);
}

#[test]
fn network_zoning_and_first_days() {
    check_city(PRESET, PRESET.default_seed());
}

fn count_service(w: &World, kind: ServiceKind) -> usize {
    w.buildings.values().filter(|b| b.service.as_ref().is_some_and(|s| s.kind == kind)).count()
}

/// Everything is laid down, nothing refused, the same twice, and the day-1
/// save loads back as the same city.
#[test]
fn furnished_and_saved() {
    let seed = PRESET.default_seed();
    let (w, plan, laid) = citygen::generate_world(PRESET, seed);
    for line in &laid.furnished {
        println!("  {line}");
    }
    let refused: Vec<&String> = laid.furnished.iter().filter(|l| l.contains("REFUSED") || l.contains("STILL OFF")).collect();
    assert!(refused.is_empty(), "refused placements: {refused:?}");
    assert!(w.implement_refused.is_none(), "{:?}", w.implement_refused);
    assert_eq!(plan.links.len(), 3, "three interchanges");
    let diamonds = laid.furnished.iter().filter(|l| l.starts_with("diamond interchange") && !l.contains("refused") && !l.contains("REFUSED")).count();
    assert_eq!(diamonds, 3, "three diamonds");
    assert!(w.citizens.is_empty(), "day 1 starts empty");
    assert!(w.economy.treasury >= 10_000_000.0, "a deep treasury: {}", w.economy.treasury);
    for kind in [ServiceKind::Police, ServiceKind::Fire, ServiceKind::Health, ServiceKind::School] {
        assert!(count_service(&w, kind) >= 2, "{kind:?}: {}", count_service(&w, kind));
    }
    for kind in [ServiceKind::University, ServiceKind::Dispatch, ServiceKind::Jail, ServiceKind::TrashDepot, ServiceKind::Landfill, ServiceKind::RecyclingPlant, ServiceKind::Incinerator] {
        assert!(count_service(&w, kind) >= 1, "no {kind:?}");
    }
    // [r6-perfect-city] Waste capacity for the grown city (~22k people make
    // ~65 t a day by day 90), all within the trucks' reach of every depot:
    // sorting and burning take a day's trash with room to spare, so the
    // landfill gets ash and residue only and nothing goes on the lorries
    // once the 14-day starter contract ends.
    let mut daily = 0.0;
    let mut tip = 0.0;
    let depots: Vec<nd_sim::Vec2> = w.buildings.values().filter(|b| b.service.is_some_and(|s| s.kind == ServiceKind::TrashDepot)).filter_map(|b| w.lots.get(b.lot)).map(|l| l.center).collect();
    for b in w.buildings.values() {
        let Some(s) = b.service.filter(|s| s.kind.is_waste()) else { continue };
        let at = w.lots[b.lot].center;
        for d in &depots {
            assert!(d.distance(at) <= nd_sim::waste::DUMP_REACH, "{:?} at {at} out of reach of the depot at {d}", s.kind);
        }
        match s.kind {
            ServiceKind::Landfill => tip += nd_sim::waste::site_capacity(s.kind, s.tier),
            _ => daily += nd_sim::waste::site_capacity(s.kind, s.tier),
        }
    }
    assert!(daily >= 150.0, "sorting and burning for a grown city: {daily} t a day");
    assert!(tip >= 6000.0, "a big landfill: {tip} t");
    let plants = |k: PowerPlantKind| w.buildings.values().filter(|b| b.plant == Some(k)).count();
    assert!(plants(PowerPlantKind::Solar) >= 1 && plants(PowerPlantKind::Wind) >= 1, "renewables");
    assert!(plants(PowerPlantKind::Coal) == 1, "one coal plant for base load");
    assert!(w.buildings.values().any(|b| b.battery.is_some()), "batteries for the evening peak");
    assert!(w.buildings.values().any(|b| b.rail.as_ref().is_some_and(|r| r.kind == RailFacility::Central)), "a central station");
    assert!(w.buildings.values().any(|b| b.rail.as_ref().is_some_and(|r| r.kind == RailFacility::FreightYard)), "a freight yard");
    assert_eq!(w.subway.lines.len(), 2, "two metro lines");
    assert!(w.transit.routes.len() >= 4, "bus lines: {}", w.transit.routes.len());
    assert!(w.districts.list.len() >= 6, "districts");
    assert!(w.lots.values().any(|l| l.shore.beach), "a public beach");
    // No homes in among the works downwind.
    let works = plan.areas.iter().filter(|a| a.0 == "works").map(|a| a.1).collect::<Vec<_>>();
    let homes_in_works = w.lots.values().filter(|l| l.land_use == LandUse::Residential && works.iter().any(|c| c.distance(l.center) < 150.0)).count();
    assert_eq!(homes_in_works, 0, "homes zoned in the middle of the works");

    // The same again.
    let (w2, _, laid2) = citygen::generate_world(PRESET, seed);
    assert_eq!(laid.furnished, laid2.furnished, "furnishing differs between runs");
    assert_eq!(w.buildings.len(), w2.buildings.len());

    // The day-1 save loads.
    let text = w.to_ron().expect("save");
    let back = World::from_ron(&text).expect("load");
    assert_eq!(back.buildings.len(), w.buildings.len());
    assert_eq!(back.lots.len(), w.lots.len());
    assert_eq!(back.subway.lines.len(), 2);
}

/// A week of growth: people move in every day, the lights stay on at the
/// evening peak and the treasury holds.
#[test]
fn grows_with_the_lights_on() {
    let (mut w, _, _) = citygen::generate_world(PRESET, PRESET.default_seed());
    let mut minds = RuleBasedMind::default();
    let mut served_min: f32 = 1.0;
    let mut pops = Vec::new();
    for _ in 0..7 {
        for m in 0..1440 {
            w.tick(&mut minds);
            if m % 30 == 0 {
                served_min = served_min.min(w.economy.power.served);
            }
        }
        pops.push(w.citizens.len());
        println!("day {}: pop {} served_min {served_min:.3} treasury {:.0}", w.clock.day(), w.citizens.len(), w.economy.treasury);
    }
    assert!(*pops.last().unwrap() > 2_000, "only {} people after a week: {pops:?}", pops.last().unwrap());
    assert!(pops.windows(2).skip(2).all(|p| p[1] > p[0]), "growth stalled: {pops:?}");
    assert!(served_min > 0.98, "a blackout: {served_min}");
    assert!(w.economy.treasury > 10_000_000.0, "the treasury ran down: {}", w.economy.treasury);
}

/// [r6-perfect-city] The long check (about 50 minutes: run it by hand, or
/// see the example `perfect_city 90`): two months on, past the trash
/// starter contract, the city is big, mostly housed and working, the
/// trash all goes somewhere in town and the landfill has years left.
///
/// cargo test --profile fast -p nd_procgen --test citygen_perfect_city -- --ignored --nocapture
#[test]
#[ignore]
fn holds_up_for_two_months() {
    let (mut w, _, _) = citygen::generate_world(PRESET, PRESET.default_seed());
    let mut minds = RuleBasedMind::default();
    let mut served_min: f32 = 1.0;
    // Unemployment swings while whole blocks rebuild (the seed-5 city's
    // worst was ~14% around day 39 in testing, back under 10% by day 50):
    // the check is for a collapse, not the swing.
    let mut worst_jobless: f32 = 0.0;
    for _ in 0..60 {
        for m in 0..1440 {
            w.tick(&mut minds);
            if m % 30 == 0 {
                served_min = served_min.min(w.economy.power.served);
            }
        }
        let d = &w.society.trash.disposal;
        let road = d.t_yesterday[nd_sim::waste::Dest::Road.index()];
        println!(
            "day {}: pop {} homeless {} jobless {:.1}% trash {:.0} t (road {road:.0}) landfill {:?} treasury {:.0}",
            w.clock.day(),
            w.citizens.len(),
            w.homeless_people(),
            w.economy.unemployment_share() * 100.0,
            d.yesterday_t(),
            w.waste_sites().iter().filter_map(|s| w.landfill_fill(s.id)).collect::<Vec<_>>(),
            w.economy.treasury
        );
        if w.clock.day() >= 20 {
            worst_jobless = worst_jobless.max(w.economy.unemployment_share());
        }
        if w.clock.day() > 16 {
            assert!(road < 1.0, "day {}: {road} t on the lorries after the starter contract", w.clock.day());
        }
    }
    let pop = w.citizens.len();
    assert!(pop > 18_000, "only {pop} people after two months");
    assert!(w.homeless_people() * 100 < pop * 2, "{} sleeping rough of {pop}", w.homeless_people());
    assert!(worst_jobless < 0.2, "unemployment reached {:.1}%", worst_jobless * 100.0);
    assert!(w.economy.unemployment_share() < 0.12, "unemployment at day 60: {:.1}%", w.economy.unemployment_share() * 100.0);
    assert!(served_min > 0.98, "a blackout: {served_min}");
    for s in w.waste_sites() {
        if let Some(f) = w.landfill_fill(s.id) {
            assert!(f < 0.15, "the landfill is {:.0}% full after two months", f * 100.0);
        }
    }
    assert!(w.economy.treasury > 40_000_000.0, "the treasury ran down: {}", w.economy.treasury);
}
