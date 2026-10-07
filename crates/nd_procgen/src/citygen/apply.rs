//! Laying a generated plan out in the game: every street becomes a road
//! gesture with its class's lanes, every block a zone gesture, all in one
//! project, implemented; then the town is joined to the highway by ramps
//! and given a power plant, as the demo district is.

use super::{CityPlan, CityPreset};
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, Vec2, World, WorldConfig};

/// What laying a plan out made.
#[derive(Clone, Debug, Default)]
pub struct LaidCity {
    pub roads: usize,
    pub zones: usize,
    /// Highway interchanges built.
    pub links: usize,
    pub lots: usize,
    pub way_in: bool,
}

/// Lay `plan` out in `world` (a fresh one, on the landscape the plan was
/// generated for).
pub fn lay_city(world: &mut World, plan: &CityPlan) -> LaidCity {
    let mut out = LaidCity::default();
    world.start_project();
    for r in &plan.roads {
        let (lanes_fwd, lanes_bwd) = r.class.lanes();
        world.add_gesture(Gesture::Road { points: r.points.clone(), lanes_fwd, lanes_bwd, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        out.roads += 1;
    }
    for z in &plan.zones {
        world.add_gesture(Gesture::Zone { polygon: z.polygon.clone(), land_use: z.land_use });
        out.zones += 1;
    }
    // The interchanges go in the same project: one rebuild for the lot.
    for &l in &plan.links {
        if let Some(gestures) = world.highway_link(l) {
            for g in gestures {
                world.add_gesture(g);
            }
            out.links += 1;
        }
    }
    world.implement();
    if !world.has_way_in() {
        // No interchange took: a road off the east edge of the map from the
        // easternmost street end.
        let east = world.roads.alive_segments().flat_map(|(id, _)| { let (a, b) = world.roads.segment_points(id); [a, b] }).filter(|p| p.y < super::NORTH_LIMIT).max_by(|a, b| a.x.total_cmp(&b.x));
        if let Some(p) = east {
            world.add_road(&[p, Vec2::new(nd_sim::landscape::MAP_EDGE, p.y)], 1, 1);
        }
    }
    // A starting town has the money for its plant.
    world.economy.treasury = world.economy.treasury.max(150_000.0);
    if !world.buildings.values().any(|b| b.plant.is_some()) {
        let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).max_by(|a, b| a.1.area().total_cmp(&b.1.area()).then(b.1.center.x.total_cmp(&a.1.center.x))).map(|(id, _)| id);
        if let Some(lot) = lot {
            let _ = world.build_power(lot, PowerPlantKind::Coal);
        }
    }
    out.lots = world.lots.len();
    out.way_in = world.has_way_in();
    out
}

/// A new world on the preset's landscape with its generated city laid out
/// on it (what the `generate_city` command and the tests start from).
pub fn generate_world(preset: CityPreset, seed: u64) -> (World, CityPlan, LaidCity) {
    let landscape = preset.landscape(seed);
    let mut world = World::new(WorldConfig { seed, landscape, ..WorldConfig::default() });
    world.drain_events();
    let plan = super::generate(preset, seed, &world.landscape);
    let laid = lay_city(&mut world, &plan);
    (world, plan, laid)
}
