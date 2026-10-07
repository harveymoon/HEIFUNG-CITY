//! [land] Probe: the demo district (as `tests/demo_city.rs` lays it out)
//! for some days, printing the pressure grid, heights and the land market.
//!
//! cargo run --profile fast -p nd_sim --example land_probe -- 8

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, RailFacility, Vec2, World, WorldConfig};

fn demo_world() -> World {
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
    let mut world = demo_world();
    let mut minds = RuleBasedMind::default();
    let days: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    for day in 1..=days {
        loop {
            world.tick(&mut minds);
            if world.clock.minute_of_day() == 0 {
                break;
            }
        }
        let p = &world.estate.pressure;
        let mut vals: Vec<f32> = world.lots.values().filter(|l| l.building.is_some()).map(|l| p.at(l.center)).collect();
        vals.sort_by(|a, b| a.total_cmp(b));
        let q = |f: f32| vals.get(((vals.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(0.0);
        let mut caps: Vec<u8> = world.lots.keys().filter(|l| world.lots[*l].building.is_some()).map(|l| world.height_cap(l)).collect();
        caps.sort();
        let mut floors = [0u32; 8];
        for b in world.buildings.values().filter(|b| b.construction.is_none()) {
            floors[(b.floors as usize / 3).min(7)] += 1;
        }
        let s = &world.estate.stats;
        let mob: f32 = p.mobility.iter().sum::<f32>() / p.mobility.len().max(1) as f32;
        let (pop, gini, ratio, ok) = world.elite_gate();
        println!(
            "day {day}: pop {pop} | pressure p10 {:.1} p50 {:.1} p90 {:.1} max {:.1} | cap p50 {} max {} | floors by 3s {:?} | mob {mob:.2} | buyouts {} merges {} branches {} mcm {} | gini {gini:.2} top/med {ratio:.1} gate {ok}",
            q(0.1),
            q(0.5),
            q(0.9),
            q(1.0),
            caps.get(caps.len() / 2).copied().unwrap_or(0),
            caps.last().copied().unwrap_or(0),
            floors,
            s.buyouts_total,
            s.merges_total,
            s.branches_total,
            s.mcmansions_total,
        );
    }
}
