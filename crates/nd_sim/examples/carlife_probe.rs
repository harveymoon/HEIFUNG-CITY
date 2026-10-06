//! Car-life probe: the demo district run headless, printing car counts,
//! parking occupancy and the share of trips by mode, sampled every ten
//! minutes. Used to compare before/after the persistent-car work.
//!
//! cargo run --release --example carlife_probe -p nd_sim -- [days]

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Activity, Gesture, LandUse, PowerPlantKind, RailFacility, Vec2, World, WorldConfig};

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


fn main() {
    let days: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut w = demo_world();
    let mut minds = RuleBasedMind::default();
    for day in 1..=days {
        // [walk, bike, bus, train, car] person-samples on the move.
        let mut modes = [0u64; 5];
        let mut peak_private = 0usize;
        let mut occ_sum = 0.0f64;
        let mut samples = 0u32;
        for t in 0..1440 {
            w.tick(&mut minds);
            if t % 10 == 0 {
                for c in w.citizens.values() {
                    match &c.activity {
                        Activity::Traveling { .. } => modes[if c.mode == nd_sim::citizens::TravelMode::Bike { 1 } else { 0 }] += 1,
                        Activity::Waiting { .. } | Activity::Riding { .. } => modes[2] += 1,
                        Activity::OnTrain { .. } => modes[3] += 1,
                        Activity::Driving { .. } => modes[4] += 1,
                        _ => {}
                    }
                }
                let private = w.traffic.cars.values().filter(|c| w.citizens.contains_key(c.citizen)).count();
                peak_private = peak_private.max(private);
                let e = &w.economy;
                if e.parking_capacity > 0 {
                    occ_sum += e.parking_used as f64 / e.parking_capacity as f64;
                }
                samples += 1;
            }
        }
        let total: u64 = modes.iter().sum::<u64>().max(1);
        let pct = |k: usize| modes[k] as f64 * 100.0 / total as f64;
        let e = &w.economy;
        let owners = w.citizens.values().filter(|c| c.owns_car).count();
        let bikes = w.citizens.values().filter(|c| c.owns_bike).count();
        println!(
            "day {day}: pop {} car owners {owners} bike owners {bikes} | on road now {} (private peak {peak_private}) | travel samples walk {:.0}% bike {:.0}% bus {:.0}% train {:.0}% car {:.0}% | parking {}/{} avg occ {:.0}% lots {} circling {} fees {:.0}",
            w.citizens.len(),
            w.traffic.cars.len(),
            pct(0), pct(1), pct(2), pct(3), pct(4),
            e.parking_used, e.parking_capacity, occ_sum * 100.0 / samples.max(1) as f64, e.parking_lots, e.parking_street_today, e.parking_fees_today,
        );
        print_extra(&w);
    }
}

fn print_extra(w: &World) {
    let v = &w.vehicles;
    let (parked, driving, searching) = v.counts(&w.traffic);
    let st = &v.stats;
    let t: u32 = st.trips_yesterday.iter().sum::<u32>().max(1);
    let share = |k: usize| st.trips_yesterday[k] as f32 * 100.0 / t as f32;
    let (kerb_total, kerb_used) = w.kerb_occupancy();
    let private_on_road = w.traffic.cars.values().filter(|c| c.owned.is_some()).count();
    let owners = w.citizens.values().filter(|c| c.car.is_some()).count();
    let n = w.citizens.len().max(1) as f32;
    let pain = |m: nd_sim::citizens::TripMode| w.citizens.values().map(|c| c.commute.pain_of(m)).sum::<f32>() / n;
    use nd_sim::citizens::TripMode::*;
    let far_from_home = w.citizens.values().filter(|c| c.inside.is_some() && c.inside == c.home).filter_map(|c| {
        let car = v.cars.get(c.car?)?;
        let spot = car.spot()?;
        Some(spot.pos.distance(w.building_entrance(c.home?)?) > 320.0)
    }).filter(|f| *f).count();
    let owners_home = w.citizens.values().filter(|c| c.car.is_some() && c.inside.is_some() && c.inside == c.home).count();
    let car_owner_pain = w.citizens.values().filter(|c| c.car.is_some()).map(|c| c.commute.pain_of(Car)).sum::<f32>() / owners.max(1) as f32;
    let park_pain = w.citizens.values().filter(|c| c.car.is_some()).map(|c| c.commute.parking_pain).sum::<f32>() / owners.max(1) as f32;
    println!("         owners at home {owners_home}, car out of reach {far_from_home} | owners' car pain {car_owner_pain:.2} parking pain {park_pain:.2}");
    println!(
        "         cars: records {} (parked {parked} driving {driving} searching {searching}) owners {owners} on-road owned {private_on_road} | created {} destroyed {} {:?} | trips yesterday {} walk {:.0}% bike {:.0}% bus {:.0}% train {:.0}% car {:.0}% | kerbs {kerb_used}/{kerb_total} | circled {} search {:.0} min | parked h/l/k/o {:?} | fleet parked {} bought {} | pain walk {:.2} bike {:.2} bus {:.2} car {:.2}",
        v.cars.len(), st.created_total, st.destroyed_total, st.events_total, t, share(0), share(1), share(2), share(3), share(4),
        st.circled_today, st.search_seconds_today / w.config.traffic_seconds_per_tick, st.parked_today,
        v.fleet_parked.values().map(|n| *n as u32).sum::<u32>(), st.fleet_bought_total,
        pain(Walk), pain(Bike), pain(Bus), pain(Car),
    );
}
