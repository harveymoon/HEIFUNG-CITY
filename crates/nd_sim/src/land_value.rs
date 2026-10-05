//! Land value: a coarse grid over the city that sums accessibility,
//! amenities, nuisances, traffic and the tier of neighbouring buildings. It
//! drives which tier gets built where, rents, and therefore who can live
//! where. Recomputed every couple of sim hours.

use glam::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandValue {
    pub cell: f32,
    pub half: f32,
    pub n: usize,
    pub values: Vec<f32>,
    pub version: u64,
}

impl LandValue {
    pub fn new(half: f32, cell: f32) -> Self {
        let n = ((half * 2.0) / cell).ceil() as usize;
        Self { cell, half, n, values: vec![0.3; n * n], version: 0 }
    }

    /// Set every cell (tests and scenarios).
    pub fn fill(&mut self, v: f32) {
        self.values.iter_mut().for_each(|x| *x = v);
        self.version += 1;
    }

    fn coords(&self, p: Vec2) -> (i32, i32) {
        (((p.x + self.half) / self.cell).floor() as i32, ((p.y + self.half) / self.cell).floor() as i32)
    }

    pub fn cell_center(&self, ix: usize, iy: usize) -> Vec2 {
        Vec2::new(-self.half + (ix as f32 + 0.5) * self.cell, -self.half + (iy as f32 + 0.5) * self.cell)
    }

    /// Value in `0..1` at a point (edge cells extend outward).
    pub fn at(&self, p: Vec2) -> f32 {
        let (x, y) = self.coords(p);
        let x = x.clamp(0, self.n as i32 - 1) as usize;
        let y = y.clamp(0, self.n as i32 - 1) as usize;
        self.values[y * self.n + x]
    }

    pub fn average(&self) -> f32 {
        if self.values.is_empty() {
            return 0.0;
        }
        self.values.iter().sum::<f32>() / self.values.len() as f32
    }
}

/// Accumulates influences and produces the next value grid.
pub struct LandValueBuilder {
    n: usize,
    cell: f32,
    half: f32,
    acc: Vec<f32>,
}

impl LandValueBuilder {
    pub fn new(lv: &LandValue) -> Self {
        Self { n: lv.n, cell: lv.cell, half: lv.half, acc: vec![0.0; lv.n * lv.n] }
    }

    /// Add `weight` at `center`, falling off linearly to zero at `radius`.
    pub fn splat(&mut self, center: Vec2, radius: f32, weight: f32) {
        let r_cells = (radius / self.cell).ceil() as i32;
        let cx = ((center.x + self.half) / self.cell).floor() as i32;
        let cy = ((center.y + self.half) / self.cell).floor() as i32;
        for dy in -r_cells..=r_cells {
            let y = cy + dy;
            if y < 0 || y >= self.n as i32 {
                continue;
            }
            for dx in -r_cells..=r_cells {
                let x = cx + dx;
                if x < 0 || x >= self.n as i32 {
                    continue;
                }
                let p = Vec2::new(-self.half + (x as f32 + 0.5) * self.cell, -self.half + (y as f32 + 0.5) * self.cell);
                let d = p.distance(center);
                if d < radius {
                    self.acc[y as usize * self.n + x as usize] += weight * (1.0 - d / radius);
                }
            }
        }
    }

    /// Clamp, blur once and write into `lv`.
    pub fn finish(self, base: f32, lv: &mut LandValue) {
        let n = self.n;
        let mut out = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                let mut sum = 0.0;
                let mut count = 0.0;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                        if xx >= 0 && yy >= 0 && xx < n as i32 && yy < n as i32 {
                            sum += self.acc[yy as usize * n + xx as usize];
                            count += 1.0;
                        }
                    }
                }
                out[y * n + x] = (base + sum / count).clamp(0.0, 1.0);
            }
        }
        lv.values = out;
        lv.version += 1;
    }
}
