//! Ready-made cities for the app, the tests and the probes. [review] The
//! demo district was laid out by hand in about ten places (the app, the
//! integration tests, every probe), and the copies had drifted; this is
//! the one layout they share.

use crate::{Gesture, LandUse, Vec2, World};

/// Lay out the demo district in the middle of `world`: a 5 x 5 street
/// grid with mixed zoning, a branch line with a freight yard and a station,
/// joined to the highway by a street and a pair of ramps (or, with no
/// highway, a road off the east edge), with a coal plant, a bus depot and
/// one bus line round the four central blocks (each only on a city that
/// has none yet). What the app's demo button lays.
pub fn demo_district(world: &mut World) {
    world.start_project();
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(-280.0, c), Vec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: crate::roads::RoadKind::Street, elevation: Vec::new(), car_only: None });
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -280.0), Vec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: crate::roads::RoadKind::Street, elevation: Vec::new(), car_only: None });
    }
    let cell = |cx: f32, cy: f32| {
        vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)]
    };
    let plan = [
        (-210.0, -210.0, LandUse::Industrial), (-70.0, -210.0, LandUse::Industrial), (70.0, -210.0, LandUse::Residential), (210.0, -210.0, LandUse::Residential),
        (-210.0, -70.0, LandUse::Commercial), (-70.0, -70.0, LandUse::Entertainment), (70.0, -70.0, LandUse::Commercial), (210.0, -70.0, LandUse::Residential),
        (-210.0, 70.0, LandUse::Residential), (-70.0, 70.0, LandUse::Park), (70.0, 70.0, LandUse::Civic), (210.0, 70.0, LandUse::Commercial),
        (-210.0, 210.0, LandUse::Residential), (-70.0, 210.0, LandUse::Residential), (70.0, 210.0, LandUse::Entertainment), (210.0, 210.0, LandUse::Residential),
    ];
    for (x, y, lu) in plan {
        world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
    }
    // A branch off the trunk line (a turnout, eased into curves when built), running behind the southern row.
    world.add_gesture(Gesture::Road { points: vec![Vec2::new(-420.0, crate::rail::TRUNK_Y), Vec2::new(-330.0, -410.0), Vec2::new(-280.0, -348.0), Vec2::new(-110.0, -348.0), Vec2::new(-70.0, -324.0), Vec2::new(150.0, -324.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: crate::roads::RoadKind::Rail, elevation: Vec::new(), car_only: None });
    world.implement();
    // The way in: a street north from the grid onto the highway (or,
    // with no highway, a road off the east edge of the map).
    if !world.link_to_highway(Vec2::new(0.0, 280.0)) {
        world.add_road(&[Vec2::new(280.0, 0.0), Vec2::new(crate::landscape::MAP_EDGE, 0.0)], 1, 1);
    }
    // The demo has the money for its showpieces.
    let short = 150_000.0 - world.economy.treasury;
    if short > 0.0 {
        world.grant_treasury(short);
    }
    // A freight yard and a station set down beside the branch; their
    // street sides land on the southern avenue, so both open at once.
    if world.facilities.is_empty() {
        use crate::RailFacility;
        let _ = world.place_rail_facility(RailFacility::FreightYard, Vec2::new(-200.0, -330.0));
        let _ = world.place_rail_facility(RailFacility::Station, Vec2::new(80.0, -310.0));
    }
    // A coal plant to start with: cheap power and a haze over the industrial corner.
    if world.buildings.values().any(|b| b.plant.is_some()) {
        return;
    }
    let lot = world
        .lots
        .iter()
        .filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0)
        // [plant-scale] The lot nearest a standard coal plant's site.
        .min_by(|a, b| (a.1.area() - crate::power::COAL_REFERENCE_AREA).abs().total_cmp(&(b.1.area() - crate::power::COAL_REFERENCE_AREA).abs()))
        .map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_power(lot, crate::PowerPlantKind::Coal);
    }
    // A bus depot on industrial land and one line round the four central blocks.
    if world.transit.routes.is_empty() {
        let lot = world
            .lots
            .iter()
            .filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.car_access() && l.area() >= crate::transit::DEPOT_MIN_AREA)
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
    }
}
