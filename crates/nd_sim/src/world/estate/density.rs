//! [land] Pressure: how busy each part of town is, and the floors it
//! justifies.

use super::*;

impl World {
    /// Every hour: people present and moving in each cell, and the share of
    /// cars standing, folded into the running averages.
    pub(in crate::world) fn hourly_pressure(&mut self) {
        if !self.estate.pressure.ready() {
            self.estate.pressure = Pressure::new(self.grid_half(crate::estate::PRESSURE_CELL));
        }
        let p = &self.estate.pressure;
        let cells = p.value.len();
        let mut present = vec![0.0f32; cells];
        let mut cars = vec![0.0f32; cells];
        let mut stalled = vec![0.0f32; cells];
        // [pt-growth] Street life: everyone out on foot.
        let mut foot = vec![0.0f32; cells];
        for c in self.citizens.values() {
            if let Some(i) = p.index(c.pos) {
                present[i] += 1.0;
                // Walking or cycling through counts again: movement.
                if matches!(c.activity, Activity::Traveling { .. }) {
                    present[i] += 1.0;
                }
                // (Not the homeless at their camps: a camp is not a high street.)
                if c.home.is_some() && matches!(c.activity, Activity::Traveling { .. } | Activity::Idle | Activity::Sitting { .. } | Activity::Waiting { .. }) {
                    foot[i] += 1.0;
                }
            }
        }
        // Visitors out sightseeing are street life too.
        for t in self.tourism.tourists.values().filter(|t| t.is_visible()) {
            if let Some(i) = p.index(t.pos) {
                foot[i] += 1.0;
            }
        }
        for car in self.traffic.cars.values() {
            if let Some(i) = p.index(car.pos) {
                present[i] += 1.0;
                cars[i] += 1.0;
                if car.v < 0.5 {
                    stalled[i] += 1.0;
                }
            }
        }
        let share: Vec<f32> = cars.iter().zip(&stalled).map(|(c, s)| if *c > 0.0 { s / c } else { 0.0 }).collect();
        self.estate.pressure.sample(&present, &share);
        self.estate.pressure.sample_foot(&foot);
    }

    /// Daily: how well each cell moves people. Bus stops within 300 m and a
    /// station or metro entrance within 600 m raise it; cars standing in
    /// traffic lower it. Building height follows (`Pressure::floors_at`).
    pub(in crate::world) fn daily_mobility(&mut self) {
        if !self.estate.pressure.ready() {
            return;
        }
        let stops: Vec<Vec2> = self.transit.routes.values().flat_map(|r| r.stops.iter().map(|s| s.pos)).collect();
        let stations: Vec<Vec2> = self
            .buildings
            .values()
            .filter(|b| b.operating() && (b.metro || b.rail.as_ref().is_some_and(|l| l.kind != RailFacility::FreightYard)))
            .filter_map(|b| self.lots.get(b.lot).map(|l| l.center))
            .collect();
        let p = &mut self.estate.pressure;
        for i in 0..p.value.len() {
            let c = p.cell_center(i);
            let bus = (stops.iter().filter(|s| s.distance(c) < 300.0).count() as f32 / 3.0).min(1.0);
            let rail = if stations.iter().any(|s| s.distance(c) < 600.0) { 1.0 } else { 0.0 };
            let transit = 0.5 * bus + 0.5 * rail;
            p.mobility[i] = (0.8 + 0.5 * transit - 0.6 * p.congestion[i]).clamp(0.5, 1.5);
        }
    }

    /// [pt-growth] The land density score at `p` (people present and out
    /// on foot, smoothed): what developers build to, and the Land density
    /// map layer.
    pub fn land_density_at(&self, p: Vec2) -> f32 {
        self.estate.pressure.density_at(p)
    }

    /// Does pressure cap heights yet? Measured at least once, in a town of
    /// `PRESSURE_MIN_POP` people.
    pub fn pressure_active(&self) -> bool {
        let p = &self.estate.pressure;
        p.samples > 0 && (p.always || self.citizens.len() >= PRESSURE_MIN_POP)
    }

    /// Floors the pressure on a lot justifies (two where it is quiet), held
    /// to `NIMBY_HEIGHT_CAP` inside a billionaire's NIMBY radius.
    pub fn pressure_floors(&self, lot_id: LotId) -> u8 {
        let Some(lot) = self.lots.get(lot_id) else { return PRESSURE_MAX_FLOORS };
        let f = self.estate.pressure.floors_at(lot.center);
        if self.estate.in_nimby(lot.center) {
            f.min(NIMBY_HEIGHT_CAP)
        } else {
            f
        }
    }
}
