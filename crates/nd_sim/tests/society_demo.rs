//! [society] The demo district run eight days twice: as laid out (no
//! police, no jail, no trash depot) and with a police station, a jail, a
//! dispatch centre, a trash depot, a fire post and a hospital placed on
//! day one. Prints crime, homelessness and trash each day, and checks that
//! the society systems leave the demo's food and rent targets alone in both
//! runs, that the services make a difference, and that with them placed the
//! 911 calls are answered in time (police, fire and ambulance alike).
//!
//! cargo test --release -p nd_sim --test society_demo -- --nocapture

use nd_minds::RuleBasedMind;
use nd_sim::crime::Offence;
use nd_sim::services::{response_summary, IncidentKind, Outcome};
use nd_sim::{LandUse, Placeable, ServiceKind, Tier, World};

#[path = "demo_city.rs"]
#[allow(dead_code)]
mod demo;

fn place(w: &mut World, kind: ServiceKind, tier: Tier, uses: &[LandUse]) {
    place_near(w, kind, tier, uses, None)
}

/// Place on the first suitable lot, or the one nearest `near`.
fn place_near(w: &mut World, kind: ServiceKind, tier: Tier, uses: &[LandUse], near: Option<nd_sim::Vec2>) {
    let what = Placeable::Service(kind, tier);
    let ok = |w: &World, vacant: bool| {
        let fits = w.lots.iter().filter(|(id, l)| uses.contains(&l.land_use) && (!vacant || l.building.is_none()) && w.placement_problem(what, *id).is_none());
        match near {
            Some(p) => fits.min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id),
            None => fits.map(|(id, _)| id).next(),
        }
    };
    let lot = ok(w, true).or_else(|| ok(w, false)).unwrap_or_else(|| {
        let why: Vec<String> = w.lots.iter().filter(|(_, l)| uses.contains(&l.land_use)).filter_map(|(id, _)| w.placement_problem(what, id)).take(5).collect();
        panic!("nowhere for {kind:?} (treasury {:.0}): {why:?}", w.economy.treasury)
    });
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
    /// Per incident kind (fire, crime, medical), day 8: calls, answered
    /// within 30 minutes, and the minutes from call to arrival of those
    /// answered.
    response: [(usize, usize, Vec<u16>); 3],
}

fn run(services: bool) -> Day8 {
    let mut world = demo::demo_world();
    if services {
        place(&mut world, ServiceKind::Police, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
        place(&mut world, ServiceKind::Dispatch, Tier::Low, &[LandUse::Civic, LandUse::Commercial]);
        place(&mut world, ServiceKind::Jail, Tier::Low, &[LandUse::Civic, LandUse::Industrial]);
        place(&mut world, ServiceKind::TrashDepot, Tier::Mid, &[LandUse::Industrial, LandUse::Civic]);
        // Fire engines answer from their station, so the post goes in the
        // middle of the district, as a planner would put it.
        place_near(&mut world, ServiceKind::Fire, Tier::Low, &[LandUse::Civic, LandUse::Commercial, LandUse::Industrial], Some(nd_sim::Vec2::ZERO));
        place(&mut world, ServiceKind::Health, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
    }
    let mut minds = RuleBasedMind::default();
    let label = if services { "with services" } else { "bare demo" };
    let mut pick = 0x2545_F491_4F6C_DD1Du64;
    for day in 1..=8 {
        loop {
            world.tick(&mut minds);
            let minute = world.clock.minute_of_day();
            if minute == 0 {
                break;
            }
            // Day 8, services placed: a 911 drill on top of the day's own
            // calls, so the response figures rest on enough calls: a crime
            // every 90 minutes, someone taken ill every 90, a fire every 6 h.
            if services && day == 8 && minute < 22 * 60 {
                let mut next = |n: usize| {
                    pick = pick.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (pick >> 33) as usize % n.max(1)
                };
                let occupied: Vec<_> = world.buildings.iter().filter(|(_, b)| b.service.is_none() && b.construction.is_none() && (!b.residents.is_empty() || !b.workers.is_empty())).map(|(id, _)| id).collect();
                if minute % 90 == 17 && !occupied.is_empty() {
                    world.start_crime(occupied[next(occupied.len())]);
                }
                // Only what can burn: start_fire turns down parks, stations
                // and anything already alight, and a refused pick left the
                // day short of drill fires.
                let flammable: Vec<_> = occupied.iter().copied().filter(|&b| world.buildings.get(b).is_some_and(|b| b.land_use != nd_sim::LandUse::Park && b.rail.is_none())).collect();
                if minute % 360 == 200 && !flammable.is_empty() {
                    world.start_fire(flammable[next(flammable.len())]);
                }
                if minute % 90 == 62 {
                    let indoors: Vec<_> = world.citizens.iter().filter(|(_, c)| c.inside.is_some_and(|b| world.buildings.get(b).is_some_and(|b| b.service.is_none()))).map(|(id, _)| id).collect();
                    if !indoors.is_empty() {
                        world.fall_ill(indoors[next(indoors.len())]);
                    }
                }
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
    // Day 8's calls, up to half an hour before midnight (police calls
    // include road crashes; those on the highway beyond the city's streets
    // go to the next town's units): one the caller
    // abandoned before help could come (the patient left, the building came
    // down) is not counted against the services.
    let (day8, end) = (7 * 1440, 8 * 1440 - 30);
    let mut response: [(usize, usize, Vec<u16>); 3] = Default::default();
    for i in world.services.incidents.iter().filter(|i| i.reported.is_some_and(|r| r >= day8 && r <= end)) {
        if i.arrived.is_none() && i.outcome == Some(Outcome::Gone) {
            continue;
        }
        let r = &mut response[i.kind.slot()];
        r.0 += 1;
        if let Some(t) = i.response_ticks() {
            r.1 += (t <= 30) as usize;
            r.2.push(t as u16);
        }
    }
    // [emergency] Car chases over the run, and how they ended.
    let chases: Vec<String> = world.services.incidents.iter().filter_map(|i| i.chase.as_ref().map(|c| format!("{}: {:?} {:.0} m {} min", i.id, c.result, c.metres, c.ended.unwrap_or(world.clock.tick).saturating_sub(c.started)))).collect();
    println!("[{label}] chases {} (tagged {}): {:?}", world.services.stats.chases_total, world.services.stats.chases_tagged_total, chases);
    let s = &world.society;
    Day8 {
        response,
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
    // With the services placed, 911 works on day 8: at least 90 % of calls
    // answered within 30 minutes and a median response under 15, per kind.
    for kind in IncidentKind::ALL {
        let (calls, quick, mins) = &served.response[kind.index()];
        let (median, p90, _) = response_summary(mins, 30).unwrap_or((0, 0, 0.0));
        println!("served {}: {calls} calls, {} answered, {quick} within 30 min, median {median} p90 {p90} min", kind.label(), mins.len());
        assert!(*calls >= 3, "{}: only {calls} calls on day 8", kind.label());
        assert!(*quick as f32 >= 0.9 * *calls as f32, "{}: only {quick} of {calls} calls answered within 30 minutes", kind.label());
        assert!(median < 15, "{}: median response {median} min", kind.label());
    }
}
