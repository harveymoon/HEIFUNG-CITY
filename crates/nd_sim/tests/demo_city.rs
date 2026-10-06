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
    world.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 2, 2);
    world.add_road(&[Vec2::new(0.0, -700.0), Vec2::new(0.0, 700.0)], 2, 2);
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
