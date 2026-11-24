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
use nd_sim::{LandUse, ServiceKind, Tier};

mod common;

use common::{place, place_near};

struct Day8 {
    /// The food and rent targets missed (hungry, starving, rent-stressed).
    fails: Vec<String>,
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

fn run(services: bool, seed: u64) -> Day8 {
    let mut world = common::demo_world_seeded(seed);
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
    let label = format!("{} seed {seed}", if services { "with services" } else { "bare demo" });
    let mut pick = 0x2545_F491_4F6C_DD1Du64;
    // A drill fire due while another fire still burns waits until it is
    // out: with one engine in town the second call measured the overlap,
    // not the response (fix/under-construction: a drill fire four minutes
    // after another left one call waiting 30 minutes for the engine).
    let mut fire_due = false;
    // The food and rent targets missed, checked by the caller.
    let mut fails: Vec<String> = Vec::new();
    for day in 1..=8 {
        loop {
            world.tick(&mut minds);
            let minute = world.clock.minute_of_day();
            if minute == 0 {
                break;
            }
            // Day 8, services placed: a 911 drill on top of the day's own
            // calls, so the response figures rest on enough calls: a crime
            // every 90 minutes, someone taken ill every 90, a fire every 3 h
            // (every 6 h gave a median of 4 fires, so one slow call moved it
            // by minutes and unrelated changes flipped the check).
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
                // The fire's place is drawn as a point over the district and
                // the flammable building nearest it burns: an index into the
                // list of occupied buildings reshuffled every drill fire with
                // any change in who lives where, and the median of seven
                // calls swung 13 to 15 minutes on which corners they landed.
                if minute % 180 == 110 {
                    fire_due = true;
                }
                let burning = world.services.incidents.iter().any(|i| i.kind == IncidentKind::Fire && i.active());
                if fire_due && !burning && !flammable.is_empty() {
                    fire_due = false;
                    let at = nd_sim::Vec2::new(next(560) as f32 - 280.0, next(560) as f32 - 280.0);
                    let near = |b: &nd_sim::BuildingId| world.building_entrance(*b).map_or(f32::MAX, |p| p.distance(at));
                    let b = *flammable.iter().min_by(|x, y| near(x).total_cmp(&near(y))).unwrap();
                    world.start_fire(b);
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
        if hungry >= 0.10 {
            fails.push(format!("{label} day {day}: {:.1}% hungry", hungry * 100.0));
        }
        if starving >= 0.02 {
            fails.push(format!("{label} day {day}: {:.1}% starving", starving * 100.0));
        }
        if day == 8 && stressed >= 0.05 {
            fails.push(format!("{label} day 8: {:.1}% rent-stressed", stressed * 100.0));
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
        fails,
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

/// [r16-ground] The seeds the demo is grown from (the default first). Crime
/// at one seed is a handful of offenders and offences, and unrelated
/// changes reshuffled them (offenders 9-12, offences 4-6 at the default
/// seed in round 14): `spread` over seeds 0-11 measured bare offenders
/// 5-19 (mean 11.6) against 0-9 served (4.0), offences 3-16 (9.1) against
/// 0-4 (1.75); served had fewer offences at all 12 seeds, fewer offenders
/// at 11 (seed 1: 9 vs 5, with offences 1 vs 3). Pooled over any three
/// seeds served/bare (offenders + offences) ran 0.06-0.66 (220 triples,
/// median 0.28); with police doing nothing it would sit round 1.
const SEEDS: [u64; 3] = [7, 3, 10];

/// Pooled over [`SEEDS`], the served town's offenders and offences may be
/// at most this share of the bare town's (all 220 triples measured pass).
const CRIME_SHARE_MAX: f32 = 0.75;

#[test]
fn the_demo_with_and_without_police_jail_and_a_trash_depot() {
    assert_eq!(SEEDS[0], nd_sim::WorldConfig::default().seed);
    // Each seed's two towns on threads of their own (the runs do not share
    // anything), so three seeds cost about what one did.
    let runs: Vec<(Day8, Day8)> = std::thread::scope(|sc| {
        let hs: Vec<_> = SEEDS.iter().map(|&seed| (sc.spawn(move || run(false, seed)), sc.spawn(move || run(true, seed)))).collect();
        hs.into_iter().map(|(b, s)| (b.join().unwrap(), s.join().unwrap())).collect()
    });
    let sum = |d: &Day8| d.offences.iter().sum::<u32>();
    for (seed, (bare, served)) in SEEDS.iter().zip(&runs) {
        for (k, d) in [("bare", bare), ("served", served)] {
            println!(
                "seed {seed} {k}: pop {} offenders {} offences {} ({}) arrests {} reoffend {:.0}% homeless {} camps {} rvs {} overflowing {} city {:.0} kg private {:.0} kg (made {:.0} kg/day)",
                d.pop,
                d.offenders,
                sum(d),
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
        assert!(bare.fails.is_empty() && served.fails.is_empty(), "seed {seed}: {:?} {:?}", bare.fails, served.fails);
        assert!(bare.pop > 1000 && served.pop > 1000, "seed {seed}: both grow into a town");
        assert_eq!(bare.collected_city, 0.0, "seed {seed}: no depot, no city trucks");
        assert!(served.collected_city > 0.0, "seed {seed}: the depot's trucks collect");
        assert!(served.overflowing < bare.overflowing, "seed {seed}: fewer overflowing bins with a depot ({} vs {})", served.overflowing, bare.overflowing);
        // At one seed, by either measure: fewer offences committed or
        // fewer offenders about (both worse at once is a regression).
        assert!(
            served.offenders < bare.offenders || sum(served) < sum(bare),
            "seed {seed}: police and a jail hold crime back (offenders {} vs {}, offences {} vs {})",
            served.offenders,
            bare.offenders,
            sum(served),
            sum(bare)
        );
    }
    // Over the seeds: crime shows up in the bare towns, and police and a
    // jail cut it well down (see `SEEDS` for the spread this rests on).
    let bare: u32 = runs.iter().map(|(b, _)| b.offenders as u32 + sum(b)).sum();
    let served: u32 = runs.iter().map(|(_, s)| s.offenders as u32 + sum(s)).sum();
    println!("offenders + offences over seeds {SEEDS:?}: bare {bare} served {served} ({:.2})", served as f32 / bare.max(1) as f32);
    assert!(runs.iter().all(|(b, _)| b.offenders > 0 && sum(b) > 0), "crime shows up in every bare demo");
    assert!(served as f32 <= CRIME_SHARE_MAX * bare as f32, "police and a jail hold crime back over the seeds: {served} vs {bare} offenders + offences");
    // With the services placed, 911 works on day 8, the calls of all seeds
    // together: at least 90 % answered within 30 minutes and a median
    // response of 15 at most, per kind (single seeds ran medians 8-16).
    for kind in IncidentKind::ALL {
        let mut mins: Vec<u16> = Vec::new();
        let (mut calls, mut quick) = (0, 0);
        for (seed, (_, served)) in SEEDS.iter().zip(&runs) {
            let (c, q, m) = &served.response[kind.index()];
            assert!(*c >= 3, "seed {seed} {}: only {c} calls on day 8", kind.label());
            calls += c;
            quick += q;
            mins.extend(m);
        }
        let (median, p90, _) = response_summary(&mins, 30).unwrap_or((0, 0, 0.0));
        println!("served {}: {calls} calls, {} answered, {quick} within 30 min, median {median} p90 {p90} min", kind.label(), mins.len());
        assert!(quick as f32 >= 0.9 * calls as f32, "{}: only {quick} of {calls} calls answered within 30 minutes", kind.label());
        assert!(median <= 15, "{}: median response {median} min", kind.label());
    }
}

/// [r16-ground] The spread of the crime and 911 figures over seeds, for
/// setting the checks above: `ND_SEEDS=0..12 cargo test --profile fast -p
/// nd_sim --test society_demo -- --ignored --nocapture spread`.
#[test]
#[ignore]
fn spread() {
    let seeds = std::env::var("ND_SEEDS").unwrap_or_else(|_| "0..8".into());
    let (a, b) = seeds.split_once("..").expect("ND_SEEDS=a..b");
    for seed in a.parse::<u64>().unwrap()..b.parse::<u64>().unwrap() {
        let bare = run(false, seed);
        let served = run(true, seed);
        let sum = |d: &Day8| d.offences.iter().sum::<u32>();
        let r: Vec<String> = IncidentKind::ALL
            .iter()
            .map(|k| {
                let (calls, quick, mins) = &served.response[k.index()];
                let (median, _, _) = response_summary(mins, 30).unwrap_or((0, 0, 0.0));
                format!("{} {calls}/{quick}/{median}", k.label())
            })
            .collect();
        println!(
            "SPREAD seed {seed}: pop {}/{} offenders {} vs {} offences {} vs {} arrests {} overflowing {} vs {} | {} | fails {:?} {:?}",
            bare.pop,
            served.pop,
            bare.offenders,
            served.offenders,
            sum(&bare),
            sum(&served),
            served.arrests,
            bare.overflowing,
            served.overflowing,
            r.join(", "),
            bare.fails,
            served.fails
        );
    }
}
