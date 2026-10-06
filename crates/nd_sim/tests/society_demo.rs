//! [society] The demo district run eight days twice: as laid out (no
//! police, no jail, no trash depot) and with a police station, a jail, a
//! dispatch centre and a trash depot placed on day one. Prints crime,
//! homelessness and trash each day, and checks that the society systems
//! leave the demo's food and rent targets alone in both runs and that the
//! services make a difference.
//!
//! cargo test --release -p nd_sim --test society_demo -- --nocapture

use nd_minds::RuleBasedMind;
use nd_sim::crime::Offence;
use nd_sim::{LandUse, Placeable, ServiceKind, Tier, World};

#[path = "demo_city.rs"]
#[allow(dead_code)]
mod demo;

fn place(w: &mut World, kind: ServiceKind, tier: Tier, uses: &[LandUse]) {
    let what = Placeable::Service(kind, tier);
    let ok = |w: &World, vacant: bool| w.lots.iter().filter(|(id, l)| uses.contains(&l.land_use) && (!vacant || l.building.is_none()) && w.placement_problem(what, *id).is_none()).map(|(id, _)| id).next();
    let lot = ok(w, true).or_else(|| ok(w, false)).unwrap_or_else(|| panic!("nowhere for {kind:?}"));
    w.build_service(lot, kind, tier).unwrap();
}

struct Day8 {
    pop: usize,
    offenders: usize,
    offences: [u32; 5],
    arrests: u32,
    reoffend: f32,
    homeless: usize,
    camps: usize,
    rvs: usize,
    overflowing: u32,
    collected_city: f32,
    collected_private: f32,
    produced: f32,
}

fn run(services: bool) -> Day8 {
    let mut world = demo::demo_world();
    if services {
        place(&mut world, ServiceKind::Police, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
        place(&mut world, ServiceKind::Dispatch, Tier::Low, &[LandUse::Civic, LandUse::Commercial]);
        place(&mut world, ServiceKind::Jail, Tier::Low, &[LandUse::Civic, LandUse::Industrial]);
        place(&mut world, ServiceKind::TrashDepot, Tier::Mid, &[LandUse::Industrial, LandUse::Civic]);
    }
    let mut minds = RuleBasedMind::default();
    let label = if services { "with services" } else { "bare demo" };
    for day in 1..=8 {
        loop {
            world.tick(&mut minds);
            if world.clock.minute_of_day() == 0 {
                break;
            }
        }
        let n = world.citizens.len().max(1) as f32;
        let hungry = world.citizens.values().filter(|c| c.satiety < 0.3).count() as f32 / n;
        let starving = world.citizens.values().filter(|c| c.hungry_days >= 2).count() as f32 / n;
        let shares: Vec<f32> = world.households.keys().filter_map(|h| world.household_rent_share(h)).collect();
        let stressed = shares.iter().filter(|s| **s > 0.5).count() as f32 / shares.len().max(1) as f32;
        let s = &world.society;
        println!(
            "[{label}] day {day}: pop {} | offenders {} offences {:?} arrests {} reoffend {:.0}% | homeless {} camps {} rvs {} | trash {:.0} kg made, city {:.0} private {:.0} | overflowing {} clients {} | hungry {:.1}% starving {:.1}% stressed {:.1}% | lv {:.2}",
            world.citizens.len(),
            s.crime.offenders.len(),
            s.crime.stats.offences_yesterday,
            s.crime.stats.arrests_total,
            s.crime.reoffend_rate() * 100.0,
            world.homeless_people(),
            s.homeless.camps.len(),
            s.homeless.rvs.len(),
            s.trash.stats.produced_yesterday,
            s.trash.stats.collected_city_yesterday,
            s.trash.stats.collected_private_yesterday,
            s.trash.stats.overflowing_buildings,
            s.trash.stats.private_clients,
            hungry * 100.0,
            starving * 100.0,
            stressed * 100.0,
            world.land_value.average(),
        );
        assert!(hungry < 0.10, "{label} day {day}: {:.1}% hungry", hungry * 100.0);
        assert!(starving < 0.02, "{label} day {day}: {:.1}% starving", starving * 100.0);
        if day == 8 {
            assert!(stressed < 0.05, "{label} day 8: {:.1}% rent-stressed", stressed * 100.0);
        }
    }
    let s = &world.society;
    Day8 {
        pop: world.citizens.len(),
        offenders: s.crime.offenders.len(),
        offences: s.crime.stats.offences_total,
        arrests: s.crime.stats.arrests_total,
        reoffend: s.crime.reoffend_rate(),
        homeless: world.homeless_people(),
        camps: s.homeless.camps.len(),
        rvs: s.homeless.rvs.len(),
        overflowing: s.trash.stats.overflowing_buildings,
        collected_city: s.trash.stats.collected_city_total,
        collected_private: s.trash.stats.collected_private_total,
        produced: s.trash.stats.produced_yesterday,
    }
}

#[test]
fn the_demo_with_and_without_police_jail_and_a_trash_depot() {
    let bare = run(false);
    let served = run(true);
    for (k, d) in [("bare", &bare), ("served", &served)] {
        println!(
            "{k}: pop {} offenders {} offences {} ({}) arrests {} reoffend {:.0}% homeless {} camps {} rvs {} overflowing {} city {:.0} kg private {:.0} kg (made {:.0} kg/day)",
            d.pop,
            d.offenders,
            d.offences.iter().sum::<u32>(),
            Offence::ALL.iter().map(|o| format!("{} {}", o.key(), d.offences[o.index()])).collect::<Vec<_>>().join(", "),
            d.arrests,
            d.reoffend * 100.0,
            d.homeless,
            d.camps,
            d.rvs,
            d.overflowing,
            d.collected_city,
            d.collected_private,
            d.produced
        );
    }
    assert!(bare.pop > 1000 && served.pop > 1000, "both grow into a town");
    assert!(bare.offenders > 0 && bare.offences.iter().sum::<u32>() > 0, "crime shows up in the bare demo");
    assert_eq!(bare.collected_city, 0.0, "no depot, no city trucks");
    assert!(served.collected_city > 0.0, "the depot's trucks collect");
    assert!(served.overflowing < bare.overflowing, "fewer overflowing bins with a depot ({} vs {})", served.overflowing, bare.overflowing);
    assert!(served.offenders < bare.offenders, "police and a jail hold crime back ({} vs {})", served.offenders, bare.offenders);
}
