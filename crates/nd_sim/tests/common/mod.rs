//! Shared fixtures for the integration tests: the demo district, a busy
//! copy of it, a mind whose answers arrive late, the ghost counts and
//! service placement. [review] They used to live in `demo_city.rs`, which
//! four other test files pulled in with `#[path]`, so its tests ran in five
//! binaries (five chances to flake, seven eight-day runs per suite).
#![allow(dead_code)]

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Activity, BuildingId, CitizenId, Decision, Gesture, LandUse, MindContext, MindProvider, Placeable, PowerPlantKind, RailFacility, ServiceKind, Tier, Vec2, World, WorldConfig};
use std::sync::OnceLock;

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
    let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).min_by(|a, b| (a.1.area() - nd_sim::power::COAL_REFERENCE_AREA).abs().total_cmp(&(b.1.area() - nd_sim::power::COAL_REFERENCE_AREA).abs())).map(|(id, _)| id);
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

/// The demo run with the rule-based mind until at least 40 city cars are
/// on the road (up to 40 000 ticks). Built once per test binary and cloned
/// for each test that wants it; the mind starts afresh.
pub fn busy_city() -> (World, RuleBasedMind) {
    static BUSY: OnceLock<World> = OnceLock::new();
    let world = BUSY
        .get_or_init(|| {
            let mut world = demo_world();
            let mut minds = RuleBasedMind::default();
            let mut ticks = 0;
            while world.traffic.cars.values().filter(|c| !World::is_through_car(c)).count() < 40 && ticks < 40_000 {
                world.tick(&mut minds);
                ticks += 1;
            }
            world
        })
        .clone();
    assert!(world.traffic.cars.values().filter(|c| !World::is_through_car(c)).count() >= 40, "the demo city gets busy");
    (world, RuleBasedMind::default())
}

/// The rule-based mind, plus answers that arrive "late" through `poll`,
/// as an LLM's do in the app's `HybridMind`.
pub struct Late {
    pub rules: RuleBasedMind,
    pub queue: Vec<(CitizenId, Decision)>,
}

impl Late {
    pub fn new() -> Self {
        Late { rules: RuleBasedMind::default(), queue: Vec::new() }
    }
}

impl MindProvider for Late {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        self.rules.decide(ctx)
    }
    fn poll(&mut self) -> Vec<(CitizenId, Decision)> {
        std::mem::take(&mut self.queue)
    }
    fn name(&self) -> &'static str {
        "late"
    }
}

/// (bus riders not riding it, private cars driven by someone not in them,
/// train riders not on it)
pub fn ghosts(w: &World) -> (usize, usize, usize) {
    let mut bus = 0;
    let mut car_ghost = 0;
    for (cid, car) in w.traffic.cars.iter() {
        if let Some(b) = &car.bus {
            bus += b.passengers.iter().filter(|p| !w.citizens.get(**p).is_some_and(|c| matches!(c.activity, Activity::Riding { bus } if bus == cid))).count();
        }
        if car.bus.is_none() && car.cargo.is_none() && car.owned.is_some() && w.citizens.get(car.citizen).is_some_and(|c| !matches!(c.activity, Activity::Driving { car, .. } if car == cid)) {
            car_ghost += 1;
        }
    }
    let train = w.rail.trains.iter().map(|(tid, t)| t.passengers.iter().filter(|p| !w.citizens.get(**p).is_some_and(|c| matches!(c.activity, Activity::OnTrain { train } if train == tid))).count()).sum();
    (bus, car_ghost, train)
}

/// Place a service on the first lot of `uses` that takes it (a vacant one
/// first).
pub fn place(w: &mut World, kind: ServiceKind, tier: Tier, uses: &[LandUse]) -> BuildingId {
    place_near(w, kind, tier, uses, None)
}

/// [`place`], on the suitable lot nearest `near` when given.
pub fn place_near(w: &mut World, kind: ServiceKind, tier: Tier, uses: &[LandUse], near: Option<Vec2>) -> BuildingId {
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
    w.build_service(lot, kind, tier).unwrap()
}
