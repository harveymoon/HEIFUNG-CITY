//! [freight] Freight probe: the demo district run headless with its
//! freight yard, printing the rail import queue (orders, units, oldest
//! wait), goods aboard trains, exports on the apron and freight trains,
//! hourly, and a summary of the longest wait. Used to check that goods
//! riding the trains do not starve the town.
//!
//! cargo run --profile fast --example freight_probe -p nd_sim -- [days]

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
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(-280.0, c), Vec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -280.0), Vec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
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
        elevation: Vec::new(),
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
    let (mut worst_wait, mut worst_units, mut worst_apron, mut trains) = (0u64, 0.0f32, 0.0f32, 0u32);
    let mut in_total = 0.0f32;
    let mut last_in = 0.0f32;
    for t in 0..days * 1440 {
        w.tick(&mut minds);
        let tick = w.clock.tick;
        let queued: f32 = w.rail.orders.iter().map(|o| o.cargo.amount).sum();
        let oldest = w.rail.orders.iter().map(|o| tick.saturating_sub(o.since)).max().unwrap_or(0);
        let aboard: f32 = w.rail.trains.values().flat_map(|t| t.cargo.iter()).map(|c| c.amount).sum();
        let apron: f32 = w.buildings.values().map(|b| b.rail_outbound).sum();
        worst_wait = worst_wait.max(oldest);
        worst_units = worst_units.max(queued);
        worst_apron = worst_apron.max(apron);
        let fin = w.rail.stats.freight_in_today;
        if fin < last_in {
            in_total += last_in;
        }
        last_in = fin;
        if t % 60 == 59 {
            trains = trains.max(w.rail.stats.freight_trains_today);
            println!("day {} {:02}:00 orders {:3} ({:6.0} u) oldest {:4} min | aboard {:6.0} | apron {:7.0} | freight trains today {:2} | in today {:7.0} | sitting {}",
                w.clock.day(), (tick / 60) % 24, w.rail.orders.len(), queued, oldest, aboard, apron, w.rail.stats.freight_trains_today, fin,
                w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Sitting { .. })).count());
        }
    }
    in_total += last_in;
    println!("SUMMARY longest wait {worst_wait} min, most waiting {worst_units:.0} u, biggest apron {worst_apron:.0} u, most freight trains a day {trains}, brought in {in_total:.0} u");
}
