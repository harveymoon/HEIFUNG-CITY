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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_the_half_extent_and_fill_bumps_the_version() {
        let mut lv = LandValue::new(400.0, 32.0);
        assert_eq!(lv.n, 25, "800 m at 32 m cells rounds up to 25");
        assert_eq!(lv.values.len(), 25 * 25);
        assert_eq!(lv.version, 0);
        assert!((lv.at(Vec2::ZERO) - 0.3).abs() < 1e-6, "fresh land is worth a little");
        lv.fill(0.75);
        assert_eq!(lv.version, 1);
        assert!(lv.values.iter().all(|&v| v == 0.75));
        assert_eq!(lv.average(), 0.75);
        // Points off the grid read the edge cell rather than panicking.
        assert_eq!(lv.at(Vec2::new(10_000.0, -10_000.0)), 0.75);
        lv.values[0] = 0.1;
        assert_eq!(lv.at(Vec2::new(-10_000.0, -10_000.0)), 0.1, "the far south-west corner is cell 0");
        assert!(lv.cell_center(0, 0).distance(Vec2::new(-384.0, -384.0)) < 1e-4);
        assert_eq!(lv.at(lv.cell_center(0, 0)), 0.1);
        assert_eq!(LandValue::new(10.0, 32.0).average(), 0.3);
    }

    #[test]
    fn the_builder_splats_blurs_and_clamps() {
        let mut lv = LandValue::new(400.0, 32.0);
        let before = lv.version;
        let mut b = LandValueBuilder::new(&lv);
        b.splat(Vec2::ZERO, 150.0, 2.0);
        b.splat(Vec2::new(300.0, 300.0), 60.0, -5.0);
        b.finish(0.3, &mut lv);
        assert_eq!(lv.version, before + 1);
        assert!(lv.values.iter().all(|v| (0.0..=1.0).contains(v)), "clamped to 0..1");
        let centre = lv.at(Vec2::ZERO);
        let edge = lv.at(Vec2::new(120.0, 0.0));
        let far = lv.at(Vec2::new(-300.0, -300.0));
        assert!(centre > edge && edge > far, "value falls off from the splat: {centre} {edge} {far}");
        assert!((far - 0.3).abs() < 1e-4, "untouched land is the base");
        assert_eq!(lv.at(Vec2::new(300.0, 300.0)), 0.0, "a nuisance drags land to the floor");
        // A splat off the grid touches nothing.
        let mut b = LandValueBuilder::new(&lv);
        b.splat(Vec2::new(5000.0, 5000.0), 100.0, 9.0);
        let mut other = LandValue::new(400.0, 32.0);
        b.finish(0.3, &mut other);
        assert!(other.values.iter().all(|&v| (v - 0.3).abs() < 1e-6));
    }
}
