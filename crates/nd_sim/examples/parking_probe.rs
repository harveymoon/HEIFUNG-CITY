//! Parking probe: the demo district run headless; every few hours lists
//! street-parked cars standing closer than a car length.
//!
//! cargo run --release --example parking_probe -p nd_sim -- [days]

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, RailFacility, Vec2, World, WorldConfig};
use nd_sim::{CarPlace, ParkKind};

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
    let days: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mut w = demo_world();
    let mut minds = RuleBasedMind::default();
    for t in 0..days * 1440 {
        w.tick(&mut minds);
        if t % 240 != 239 {
            continue;
        }
        let spots: Vec<_> = w.vehicles.cars.iter().filter_map(|(id, c)| match c.place { CarPlace::Parked(s) if s.kind.on_street() => Some((id, s)), _ => None }).collect();
        let mut pairs = std::collections::BTreeMap::<String, u32>::new();
        let mut example = None;
        for i in 0..spots.len() {
            for j in i + 1..spots.len() {
                let (a, b) = (&spots[i].1, &spots[j].1);
                if a.pos.distance(b.pos) < 4.5 {
                    let k = format!("{:?}/{:?} same_seg {} same_slot {}", a.kind, b.kind, a.segment == b.segment, a.segment == b.segment && a.side == b.side && a.slot == b.slot);
                    *pairs.entry(k).or_default() += 1;
                    example.get_or_insert((a.clone(), b.clone()));
                }
            }
        }
        let over = spots.iter().filter(|(_, s)| s.kind == ParkKind::Overflow).count();
        println!("day {} {:02}:00 street-parked {} overflow {} overlapping {:?}", w.clock.day() + 1, w.clock.hour(), spots.len(), over, pairs);
        if let Some((a, b)) = example {
            println!("   e.g. {:?}
        {:?}", a, b);
        }
    }
}
