//! The demo district (the layout `nd_app`'s `seed_demo_district` lays out),
//! run headless with the rule-based mind for twenty days (eight before
//! [pt-growth]: see the unemployment check). Guards the food
//! chain, housing affordability, employment and business health during the
//! fast growth of a new town.
//!
//! cargo test --release -p nd_sim --test demo_city -- --nocapture

mod common;

use common::*;
use nd_minds::RuleBasedMind;
use nd_sim::{LandUse, Vec2, World};

/// One daily sample: (population, hungry share, starving share, share of
/// housed households paying over half their income in rent).
fn sample(w: &World) -> (usize, f32, f32, f32) {
    let pop = w.citizens.len();
    let n = pop.max(1) as f32;
    let hungry = w.citizens.values().filter(|c| c.satiety < 0.3).count() as f32 / n;
    let starving = w.citizens.values().filter(|c| c.hungry_days >= 2).count() as f32 / n;
    let shares: Vec<f32> = w.households.keys().filter_map(|h| w.household_rent_share(h)).collect();
    let stressed = shares.iter().filter(|s| **s > 0.5).count() as f32 / shares.len().max(1) as f32;
    (pop, hungry, starving, stressed)
}

/// Jobs and businesses: (working-age adults, of them without a job, open
/// businesses, of them broke).
fn sample_jobs(w: &World) -> (usize, usize, usize, usize) {
    let adults = w.citizens.values().filter(|c| c.can_work()).count();
    let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
    let open: Vec<_> = w.buildings.values().filter(|b| b.construction.is_none()).filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).collect();
    let broke = open.iter().filter(|z| z.broke_days > 0).count();
    (adults, idle, open.len(), broke)
}

#[test]
fn demo_city_stays_fed_housed_and_employed_through_fast_growth() {
    let t0 = std::time::Instant::now();
    let mut world = demo_world();
    let mut minds = RuleBasedMind::default();
    let mut peak = 0usize;
    // Businesses broke over the last three days sampled: one day's count
    // swings with the run (a single day went from 2 to 6 of 80 on a change
    // to how cars join lanes, with the days around it alike).
    let (mut broke_late, mut open_late) = (0usize, 0usize);
    let mut late_unemployment: Vec<f32> = Vec::new();
    for day in 1..=20 {
        // Sample at the first tick after midnight, as the remote history does.
        loop {
            world.tick(&mut minds);
            if world.clock.minute_of_day() == 0 {
                break;
            }
        }
        let (pop, hungry, starving, stressed) = sample(&world);
        let (adults, idle, open, broke) = sample_jobs(&world);
        let unemployment = idle as f32 / adults.max(1) as f32;
        let broke_share = broke as f32 / open.max(1) as f32;
        if day >= 18 {
            late_unemployment.push(unemployment);
        }
        if (6..=8).contains(&day) {
            broke_late += broke;
            open_late += open;
        }
        let e = &world.economy;
        println!(
            "        adults {adults} unemployed {idle} ({:.1}%) | businesses open {open} broke {broke} ({:.1}%) | jobs {} free {} building {} | homes {} free {} building {} | demand R {:.2} C {:.2} I {:.2}",
            unemployment * 100.0,
            broke_share * 100.0,
            e.jobs_total,
            e.jobs_free,
            e.jobs_building,
            e.homes_total,
            e.homes_free,
            e.homes_building,
            e.demand_for(LandUse::Residential),
            e.demand_for(LandUse::Commercial),
            e.demand_for(LandUse::Industrial),
        );
        println!(
            "day {day}: pop {pop} households {} sizes {:?} | hungry {:.1}% starving {:.1}% | rent-stressed households {:.1}% | displaced {} emigrated {} | {:.1}s",
            e.households,
            &e.household_sizes[1..],
            hungry * 100.0,
            starving * 100.0,
            stressed * 100.0,
            e.displaced_total,
            e.emigrated_total,
            t0.elapsed().as_secs_f32()
        );
        assert!(hungry < 0.10, "day {day}: {:.1}% hungry", hungry * 100.0);
        assert!(starving < 0.02, "day {day}: {:.1}% starving", starving * 100.0);
        peak = peak.max(pop);
        if day == 8 {
            assert!(stressed < 0.05, "day 8: {:.1}% of households rent-stressed", stressed * 100.0);
            // Homes and jobs grow in balance, and the businesses the town
            // grows are ones its market can carry.
            // [pt-growth] Re-baselined. On the saturated land value most of
            // the demo was high tier by day 8 (79 of 213 buildings): high
            // shops are 1.8x as tall with 4 jobs a floor, high homes hold
            // one household a floor, and cheap buildings on that "dear"
            // land were rebuilt every few days (100+ building sites, their
            // crews a fifth of the work). On the rescaled value the demo is
            // a poorer town (a dozen high-tier buildings), its commercial
            // land is all built by day 5 and its new jobs come from the
            // developers rebuilding the busy blocks taller, which takes a
            // week or more: unemployment runs 20-27% from day 7 to day 15,
            // then the rebuilt blocks open and it is under 2% from day 17
            // (baseline main: 13% day 6, 5% day 8, 12% day 11, 2% day 14;
            // this branch with the old scale put back: 0.7% day 8).
            // So day 8 bounds the dip, and the old bound is checked on the
            // days after it (below).
            assert!(unemployment < 0.25, "day 8: {:.1}% of working-age adults out of work", unemployment * 100.0);
            // 8%, not 5%: until the highway's map-edge lane funnel was fixed
            // (`Traffic::drop_dead_end_changes`), lorries queuing there kept
            // the road near `MAX_TRUCKS`, past which deliveries are instant,
            // and that hidden crutch kept businesses stocked. Over 24 seeds
            // the share at day 8 averaged 5.7% before the fix and 7.1% after,
            // spread 1% to 12% either way. 8.5% since [social]: friends
            // drawn to the venues where their friends are concentrate the
            // custom a little; over nine seeds the share went from 6.25%
            // (127 of 2,033 business-days) to 6.53% (136 of 2,084), and this
            // seed from 17 of 225 to 18 of 223.
            let late = broke_late as f32 / open_late.max(1) as f32;
            assert!(late < 0.085, "days 6-8: {broke_late} of {open_late} business-days broke ({broke} of {open} on day 8, {:.1}%)", broke_share * 100.0);
        }
    }
    let late = late_unemployment.iter().sum::<f32>() / late_unemployment.len().max(1) as f32;
    assert!(late < 0.08, "days 18-20: {:.1}% of working-age adults out of work on average", late * 100.0);
    assert!(peak > 1000, "the demo grows into a town ({peak})");
    let sizes = world.economy.household_sizes;
    assert!((1..=5).all(|n| sizes[n] > 0), "households of every size from one to five: {sizes:?}");
}

/// [carlife] Over three days of the demo, no private car appears or
/// disappears except by its events (arrived with a household, bought, sold,
/// scrapped, emigrated), every car is parked or on the road exactly once,
/// and none is ever taken off the road for taking too long.
#[test]
fn demo_city_cars_only_come_and_go_by_their_events() {
    use nd_sim::{CarPlace, Errand};
    let mut world = demo_world();
    let mut minds = RuleBasedMind::default();
    let allowed_in = ["arrived_with_household", "bought", "bought_out_of_town"];
    let allowed_out = ["sold", "scrapped", "emigrated"];
    let mut prev: std::collections::HashMap<nd_sim::VehicleId, CarPlace> = Default::default();
    let mut peak_parked = 0;
    for _ in 0..3 * 1440 {
        world.tick(&mut minds);
        let v = &world.vehicles;
        assert_eq!(v.cars.len() as i64, v.stats.created_total as i64 - v.stats.destroyed_total as i64, "{:?}", v.stats.events_total);
        for k in v.stats.events_total.keys() {
            assert!(allowed_in.contains(&k.as_str()) || allowed_out.contains(&k.as_str()), "unexpected event {k}");
        }
        for (vid, car) in &v.cars {
            if let CarPlace::Driving(cid) = car.place {
                assert_eq!(world.traffic.cars.get(cid).and_then(|t| t.owned), Some(vid), "a driving car is on the road");
            }
        }
        for (cid, t) in &world.traffic.cars {
            if let Some(vid) = t.owned {
                assert_eq!(v.cars.get(vid).map(|c| c.place), Some(CarPlace::Driving(cid)), "a private car on the road has its record");
            }
        }
        // A car that left the record was being driven away (to be sold, or
        // out of town) the tick before, or went for scrap.
        for (id, place) in &prev {
            if !v.cars.contains_key(*id) {
                let was_leaving = matches!(place, CarPlace::Driving(_));
                assert!(was_leaving || v.stats.events_total.contains_key("scrapped") || v.stats.events_total.contains_key("emigrated"), "a parked car vanished");
            }
        }
        prev = v.cars.iter().map(|(id, c)| (id, c.place)).collect();
        peak_parked = peak_parked.max(v.counts(&world.traffic).0);
        // Lorries and vans are not deleted for taking long either.
        assert!(world.traffic.cars.values().all(|c| c.errand != Errand::Trip || c.owned.is_none() || c.citizen != nd_sim::CitizenId::default()), "a private car on a plain trip has its driver");
    }
    let s = &world.vehicles.stats;
    println!("cars {} created {} destroyed {} {:?} peak parked {peak_parked}", world.vehicles.cars.len(), s.created_total, s.destroyed_total, s.events_total);
    assert!(s.created_total > 50, "households drive in with their cars");
    assert!(peak_parked > 50, "and park them");
}

/// [parking] Every hour of five demo days, no two cars parked on the street
/// stand within 4.5 m of each other (each has a kerb slot of its own); with
/// a parking enforcement office and a tow company working from day two,
/// tickets are written, cars are towed and fetched, and none is lost.
#[test]
fn demo_city_parked_cars_never_overlap_and_towing_keeps_every_car() {
    use nd_sim::{BusinessKind, CarPlace, Placeable, ServiceKind, Tier};
    let mut world = demo_world();
    let mut minds = RuleBasedMind::default();
    let mut worst = f32::INFINITY;
    for t in 0..5 * 1440u32 {
        world.tick(&mut minds);
        if t == 1440 {
            let free = |w: &World, lu: LandUse| w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == lu && l.area() > 460.0).map(|(id, _)| id).next();
            if let Some(lot) = free(&world, LandUse::Commercial) {
                world.place_on_lot(Placeable::Service(ServiceKind::ParkingEnforcement, Tier::Mid), lot).expect("office");
            }
            if let Some(lot) = free(&world, LandUse::Residential) {
                world.place_on_lot(Placeable::ParkingLot(nd_sim::enforcement::LotSize::MultiStorey), lot).expect("city car park");
            }
        }
        if t % 60 != 59 {
            continue;
        }
        let spots: Vec<(nd_sim::VehicleId, nd_sim::ParkSpot)> = world.vehicles.cars.iter().filter_map(|(id, c)| match c.place { CarPlace::Parked(s) if s.kind.on_street() => Some((id, s)), _ => None }).collect();
        for i in 0..spots.len() {
            for j in i + 1..spots.len() {
                let d = spots[i].1.pos.distance(spots[j].1.pos);
                worst = worst.min(d);
                assert!(d >= 4.5, "day {} {}: cars {:?} and {:?} parked {d:.2} m apart", world.clock.day() + 1, world.clock.format_hhmm(), spots[i], spots[j]);
            }
        }
        let v = &world.vehicles;
        assert_eq!(v.cars.len() as i64, v.stats.created_total as i64 - v.stats.destroyed_total as i64, "no car lost: {:?}", v.stats.events_total);
    }
    let e = &world.enforcement.stats;
    let yards = world.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard)).count();
    println!("closest street-parked pair {worst:.2} m | tickets {} fines {:.0} tows {} (out of town {}) released {} auctioned {} | tow companies {yards} | city lots {}", e.tickets_total, e.fines_total, e.tows_total, e.tows_out_of_town_total, e.released_total, e.auctioned_total, world.vehicles.city_lots.len());
}

// ── Regressions from review 2 (H1 and the player's lost depot) ────────────
// A plan edit must not take the city's vehicles off the road. A zoning edit
// leaves the roads as they are and every vehicle where it was; a road edit
// carries each vehicle across to the rebuilt lanes (same lane geometry:
// same place, same route). The city's own structures are never taken by
// road works: a project that would take one is refused until it is moved.

use nd_sim::CarId;

fn snapshot(w: &World) -> Vec<(CarId, Vec2)> {
    w.traffic.cars.iter().map(|(id, c)| (id, c.pos)).collect()
}

#[test]
fn a_zone_edit_leaves_every_vehicle_where_it_is() {
    let (mut world, mut minds) = busy_city();
    let before = snapshot(&world);
    let lanes = world.traffic.net.version;
    let c = Vec2::new(-260.0, 600.0);
    let poly = vec![c, c + Vec2::new(30.0, 0.0), c + Vec2::new(30.0, 30.0), c + Vec2::new(0.0, 30.0)];
    world.add_zone(poly, LandUse::Residential).unwrap();
    let after = snapshot(&world);
    println!("zone edit: cars {} -> {}", before.len(), after.len());
    assert_eq!(before, after, "every vehicle still on the road, where it was");
    assert_eq!(world.traffic.net.version, lanes, "the lanes were not even rebuilt");
    // And it drives on.
    for _ in 0..60 {
        world.tick(&mut minds);
    }
}

#[test]
fn a_road_edit_carries_the_vehicles_across() {
    let (mut world, mut minds) = busy_city();
    let before = snapshot(&world);
    // A new street splitting the north-east residential block, joining the
    // grid at both ends (a T at each end reshapes the lanes there).
    world.add_road(&[Vec2::new(210.0, 140.0), Vec2::new(210.0, 280.0)], 1, 1);
    assert!(world.implement_refused.is_none(), "{:?}", world.implement_refused);
    let after = snapshot(&world);
    let kept = before.iter().filter(|(id, p)| after.iter().any(|(a, q)| a == id && q.distance(*p) < 0.5)).count();
    let carried = before.iter().filter(|(id, _)| world.traffic.cars.contains_key(*id)).count();
    println!("road edit: cars {} -> {} ({carried} carried, {kept} exactly where they were)", before.len(), after.len());
    // Only a vehicle with no way on from where it stands may leave the
    // road; before the fix every city vehicle did.
    assert!(carried * 100 >= before.len() * 95, "{carried} of {} carried", before.len());
    assert!(kept * 100 >= before.len() * 80, "{kept} of {} untouched", before.len());
    // The lane lists agree with the cars, and the city drives on.
    for (id, c) in world.traffic.cars.iter() {
        if !c.entering && !c.in_stunt() {
            assert!(world.traffic.net.lanes[c.route[c.idx] as usize].cars.contains(&id));
        }
    }
    for _ in 0..600 {
        world.tick(&mut minds);
    }
}

#[test]
fn road_works_never_take_a_civic_structure() {
    let (mut world, _) = busy_city();
    let depot = world.buildings.iter().find(|(_, b)| b.depot).map(|(id, _)| id).expect("the demo city has a depot");
    let at = world.lots[world.buildings[depot].lot].center;
    let n_before = world.buildings.len();
    let master = world.master.clone();
    // A road straight through the depot's yard.
    world.start_project();
    let gid = world.add_gesture(nd_sim::Gesture::Road { points: vec![at + Vec2::new(-80.0, 0.0), at + Vec2::new(80.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() });
    let blockers = world.project_blockers();
    assert!(blockers.contains(&depot), "outlined while planned: {blockers:?}");
    let refused = world.try_implement().expect_err("refused");
    println!("{refused}");
    assert!(refused.contains(&world.buildings[depot].name));
    assert!(world.project.is_some(), "the project stays pending");
    assert_eq!(world.master.gestures, master.gestures, "nothing built");
    assert!(world.buildings.contains_key(depot) && world.buildings.len() == n_before);
    // Moved clear (far out of town), it goes through.
    world.set_gesture(gid, nd_sim::Gesture::Road { points: vec![Vec2::new(-260.0, 600.0), Vec2::new(-100.0, 600.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() }, true);
    assert!(world.project_blockers().is_empty());
    world.try_implement().expect("implemented once moved");
    assert!(world.buildings.contains_key(depot));
}

// ── Regression from review 2 (M4): late mind answers mid-trip ─────────────
// `MindProvider::poll` answers (an LLM mind thinking it over) used to be
// applied to citizens who were driving or riding: their car drove on
// empty, or they left a bus still listing them. Now the thought lands at
// once and the intent waits for their next decision.

#[test]
fn late_mind_answers_wait_until_the_trip_is_over() {
    use nd_sim::{Activity, Decision, Intent};
    let mut w = demo_world();
    let mut m = Late::new();
    for _ in 0..1440 * 2 {
        w.tick(&mut m);
    }
    let mut injected = 0;
    let mut worst = (0, 0, 0);
    for i in 0..720u64 {
        if i % 5 == 0 {
            let movers: Vec<nd_sim::CitizenId> = w.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Driving { .. } | Activity::Riding { .. } | Activity::OnTrain { .. })).map(|(id, _)| id).take(2).collect();
            for id in movers {
                m.queue.push((id, Decision { intent: Intent::GoHome, thought: "model says: go home".into(), memory: None }));
                injected += 1;
            }
        }
        w.tick(&mut m);
        let g = ghosts(&w);
        worst = (worst.0.max(g.0), worst.1.max(g.1), worst.2.max(g.2));
    }
    println!("answers injected {injected}: worst ghosts {worst:?}");
    assert!(injected > 50);
    assert_eq!(worst, (0, 0, 0));
    assert!(w.citizens.values().any(|c| c.thought == "model says: go home"), "the thought still lands");
}

// ── Regression from review 2 (M7): removing the stop a bus is bound for ───
// The bus used to be re-aimed at the stop behind it and teleported there on
// arrival. Now it keeps on to the stop after the removed one, from its lane.

#[test]
fn removing_the_next_stop_never_teleports_the_bus() {
    let mut w = demo_world();
    let mut minds = RuleBasedMind::default();
    let mut found = None;
    for _ in 0..30_000 {
        w.tick(&mut minds);
        found = w.traffic.cars.iter().find_map(|(id, c)| c.bus.as_ref().filter(|b| b.next_stop != usize::MAX && b.next_stop >= 2 && c.hold <= 0.0 && !c.entering).map(|b| (id, b.route, b.next_stop)));
        if found.is_some() {
            break;
        }
    }
    let (car, route, k) = found.expect("a bus on its way to stop >= 2");
    let n = w.transit.routes[route].stops.len();
    w.remove_bus_stop(route, k);
    assert_eq!(w.traffic.cars[car].bus.as_ref().unwrap().next_stop, if k + 1 >= n { 0 } else { k }, "bound for the stop after the removed one");
    // A tick moves a vehicle at most as far as its speed takes it (4 s at
    // up to a street's limit, with a margin for speeders); a teleport moves
    // it without the speed to match.
    let dt = w.config.traffic_seconds_per_tick;
    let (mut last, mut last_v) = (w.traffic.cars[car].pos, w.traffic.cars[car].v);
    let mut stops_made = 0;
    let mut next = w.traffic.cars[car].bus.as_ref().unwrap().next_stop;
    for t in 0..3000 {
        w.tick(&mut minds);
        let Some(c) = w.traffic.cars.get(car) else { break };
        let jump = c.pos.distance(last);
        let reach = last_v.max(c.v) * dt * 1.3 + 4.0;
        assert!(jump <= reach, "tick {t}: bus jumped {jump:.0} m at {:.1}..{:.1} m/s", last_v, c.v);
        (last, last_v) = (c.pos, c.v);
        let now = c.bus.as_ref().unwrap().next_stop;
        if now != next {
            stops_made += 1;
            next = now;
        }
    }
    assert!(stops_made >= 1, "it served the next stop");
}
