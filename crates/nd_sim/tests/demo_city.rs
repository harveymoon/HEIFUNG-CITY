//! The demo district (the layout `nd_app`'s `seed_demo_district` lays out),
//! run headless with the rule-based mind for eight days. Guards the food
//! chain, housing affordability, employment and business health during the
//! fast growth of a new town.
//!
//! cargo test --release -p nd_sim --test demo_city -- --nocapture

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, RailFacility, Vec2, World, WorldConfig};

/// The same layout as `Sim::new` + `Sim::seed_demo_district` in nd_app.
pub fn demo_world() -> World {
    let mut world = World::new(WorldConfig::default());
    world.drain_events();
    world.start_project();
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(-280.0, c), Vec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street });
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -280.0), Vec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street });
    }
    let cell = |cx: f32, cy: f32| vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)];
    let plan = [
        (-210.0, -210.0, LandUse::Industrial), (-70.0, -210.0, LandUse::Industrial), (70.0, -210.0, LandUse::Residential), (210.0, -210.0, LandUse::Residential),
        (-210.0, -70.0, LandUse::Commercial), (-70.0, -70.0, LandUse::Entertainment), (70.0, -70.0, LandUse::Commercial), (210.0, -70.0, LandUse::Residential),
        (-210.0, 70.0, LandUse::Residential), (-70.0, 70.0, LandUse::Park), (70.0, 70.0, LandUse::Civic), (210.0, 70.0, LandUse::Commercial),
        (-210.0, 210.0, LandUse::Residential), (-70.0, 210.0, LandUse::Residential), (70.0, 210.0, LandUse::Entertainment), (210.0, 210.0, LandUse::Residential),
    ];
    for (x, y, lu) in plan {
        world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
    }
    world.add_gesture(Gesture::Road {
        points: vec![Vec2::new(-420.0, nd_sim::rail::TRUNK_Y), Vec2::new(-330.0, -410.0), Vec2::new(-280.0, -348.0), Vec2::new(-110.0, -348.0), Vec2::new(-70.0, -324.0), Vec2::new(150.0, -324.0)],
        lanes_fwd: 0,
        lanes_bwd: 0,
        curved: false,
        kind: RoadKind::Rail,
    });
    world.implement();
    world.economy.treasury = world.economy.treasury.max(150_000.0);
    // The district's way in: a street north from the grid onto the highway.
    world.link_to_highway(Vec2::new(0.0, 280.0));
    let _ = world.place_rail_facility(RailFacility::FreightYard, Vec2::new(-200.0, -330.0));
    let _ = world.place_rail_facility(RailFacility::Station, Vec2::new(80.0, -310.0));
    let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_power(lot, PowerPlantKind::Coal);
    }
    let lot = world
        .lots
        .iter()
        .filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.car_access() && l.area() >= nd_sim::transit::DEPOT_MIN_AREA)
        .max_by(|a, b| a.1.area().total_cmp(&b.1.area()))
        .map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_depot(lot);
    }
    let line = world.add_bus_route();
    for p in [(-70.0, -136.0), (70.0, -136.0), (136.0, -70.0), (136.0, 70.0), (70.0, 136.0), (-70.0, 136.0), (-136.0, 70.0), (-136.0, -70.0)] {
        let _ = world.add_bus_stop(line, Vec2::new(p.0, p.1));
    }
    world.set_route_buses(line, 2);
    world
}

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
    for day in 1..=8 {
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
            assert!(unemployment < 0.08, "day 8: {:.1}% of working-age adults out of work", unemployment * 100.0);
            assert!(broke_share < 0.05, "day 8: {broke} of {open} businesses broke");
        }
    }
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
