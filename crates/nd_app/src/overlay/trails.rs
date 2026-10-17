//! [pt-walk] Where each vehicle on the road has been: a breadcrumb trail
//! per car, laid as the sim moves them, for the thin "came from" line of a
//! selected car, from when the car took to the road (or the game was
//! loaded). [insight] The sim now keeps a coarser saved record of each trip
//! (`World::trip_replay`); the line takes that where these breadcrumbs do
//! not reach (`overlay::selection::past_points`).

use std::collections::HashMap;

use bevy::prelude::*;
use nd_sim::{CarId, Vec2 as SVec2};

use crate::sim::Sim;

/// A car moved at least this far since its last point to earn another.
const STEP: f32 = 4.0;
/// Points kept per car (a couple of kilometres).
const KEEP: usize = 500;
/// A jump this long (a car put somewhere else) starts the trail over.
const BREAK: f32 = 120.0;

/// Each car's trail: points, oldest first, and whether it was in a tunnel there.
#[derive(Resource, Default)]
pub struct CarTrails {
    pub trails: HashMap<CarId, Vec<(SVec2, bool)>>,
    last_tick: Option<u64>,
}

impl CarTrails {
    /// Lay one point for a car at `p`, if it moved on far enough.
    pub fn push(&mut self, id: CarId, p: SVec2, tunnel: bool) {
        let t = self.trails.entry(id).or_default();
        if let Some(&(q, _)) = t.last() {
            let d = q.distance(p);
            if d < STEP {
                return;
            }
            if d > BREAK {
                t.clear();
            }
        }
        t.push((p, tunnel));
        if t.len() > KEEP {
            t.remove(0);
        }
    }
}

/// After each sim step: a point for every car that moved, and the trails
/// of cars gone from the road dropped.
pub fn lay_car_trails(sim: Res<Sim>, mut trails: ResMut<CarTrails>) {
    let w = &sim.world;
    let tick = w.clock.tick;
    if trails.last_tick == Some(tick) {
        return;
    }
    // A load or a new city: tick numbers jumped back.
    if trails.last_tick.is_some_and(|t| tick < t) {
        trails.trails.clear();
    }
    trails.last_tick = Some(tick);
    for (id, car) in w.traffic.cars.iter() {
        trails.push(id, car.pos, w.car_in_tunnel(car));
    }
    if tick % 30 == 0 {
        let cars = &w.traffic.cars;
        trails.trails.retain(|id, _| cars.contains_key(*id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trail_takes_a_point_every_few_metres_and_restarts_on_a_jump() {
        let mut t = CarTrails::default();
        let id = CarId::default();
        for i in 0..10 {
            t.push(id, SVec2::new(i as f32, 0.0), false);
        }
        assert_eq!(t.trails[&id].len(), 3, "0, 4 and 8 m");
        t.push(id, SVec2::new(500.0, 0.0), true);
        assert_eq!(t.trails[&id], vec![(SVec2::new(500.0, 0.0), true)]);
        for i in 0..(KEEP + 20) {
            t.push(id, SVec2::new(500.0 + 5.0 * i as f32, 0.0), false);
        }
        assert_eq!(t.trails[&id].len(), KEEP);
    }
}
