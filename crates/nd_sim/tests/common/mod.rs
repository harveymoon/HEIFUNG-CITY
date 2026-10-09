//! Shared fixtures for the integration tests: the demo district, a busy
//! copy of it, a mind whose answers arrive late, the ghost counts and
//! service placement. [review] They used to live in `demo_city.rs`, which
//! four other test files pulled in with `#[path]`, so its tests ran in five
//! binaries (five chances to flake, seven eight-day runs per suite).
#![allow(dead_code)]

use nd_minds::RuleBasedMind;
use nd_sim::{Activity, BuildingId, CitizenId, Decision, LandUse, MindContext, MindProvider, Placeable, ServiceKind, Tier, Vec2, World, WorldConfig};
use std::sync::OnceLock;

/// The same layout as `Sim::new` + `Sim::seed_demo_district` in nd_app.
pub fn demo_world() -> World {
    let mut world = World::new(WorldConfig::default());
    world.drain_events();
    nd_sim::scenarios::demo_district(&mut world);
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
