//! Bridges: what new decks cost.

use super::*;

impl World {
    /// Pay for bridge deck laid since the last rebuild.
    pub(in crate::world) fn charge_bridges(&mut self) {
        // Only deck on the map is paid for: the ways run on elsewhere at
        // elsewhere's expense.
        let edge = crate::landscape::LAND_HALF;
        let metres: f32 = self.roads.alive_segments().filter(|(_, s)| s.bridge).map(|(id, _)| {
            let (a, b) = self.roads.segment_points(id);
            let inside = (0..8).filter(|k| {
                let p = a.lerp(b, (*k as f32 + 0.5) / 8.0);
                p.x.abs() <= edge && p.y.abs() <= edge
            }).count();
            a.distance(b) * inside as f32 / 8.0
        }).sum();
        let new = metres - self.bridge_metres;
        if new > 1.0 {
            self.economy.debit_item(Flow::Construction, new * BRIDGE_COST_PER_M, Item::Named("bridges".into()));
        }
        self.bridge_metres = metres;
    }

    /// Metres of bridge a road along `points` would need, and the cost.
    pub fn bridge_cost(&self, points: &[Vec2]) -> (f32, f32) {
        let mut wet = 0.0;
        for w in points.windows(2) {
            let len = w[0].distance(w[1]);
            let steps = (len / 4.0).ceil().max(1.0) as usize;
            for k in 0..steps {
                if self.landscape.is_water(w[0].lerp(w[1], (k as f32 + 0.5) / steps as f32)) {
                    wet += len / steps as f32;
                }
            }
        }
        let m = if wet > 0.0 { wet + ABUTMENT * 2.0 } else { 0.0 };
        (m, m * BRIDGE_COST_PER_M)
    }
}
