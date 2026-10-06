//! Data for the map layers drawn over the city: colour ramps, a traffic
//! load figure per lane, vehicle classes and a heat grid that turns points
//! (people, polluters) into a smooth, translucent ground mesh.

use crate::mesh::MeshData;
use glam::Vec2;

/// Length of road one queued car takes up, bumper to bumper plus a gap.
pub const CAR_SPACE_M: f32 = 7.5;

/// Load of a lane in `0..1`: how full it is (cars times the space a queued
/// car needs over the lane length) and, while anyone is on it, how far below
/// the limit they move. An empty lane is 0, a stopped queue end to end 1.
pub fn traffic_load(cars: usize, length_m: f32, mean_speed: f32, limit: f32) -> f32 {
    if cars == 0 || length_m <= 0.0 {
        return 0.0;
    }
    let occupancy = (cars as f32 * CAR_SPACE_M / length_m).min(1.0);
    let slow = if limit > 0.0 { 1.0 - (mean_speed / limit).clamp(0.0, 1.0) } else { 0.0 };
    // Slow traffic on a near-empty lane is a car at the lights, not a jam.
    (0.55 * occupancy + 0.45 * slow * (occupancy * 3.0).min(1.0)).clamp(0.0, 1.0)
}

/// Free-flow green through yellow and orange to jammed red and magenta.
pub fn traffic_color(load: f32) -> [f32; 3] {
    ramp(&[(0.0, [0.1, 0.9, 0.35]), (0.3, [0.95, 0.9, 0.15]), (0.55, [1.0, 0.5, 0.08]), (0.8, [1.0, 0.1, 0.1]), (1.0, [0.95, 0.1, 0.7])], load)
}

/// Heat map colour for a value in `0..1`, with alpha: clear at zero, deep
/// blue, cyan, yellow, hot magenta-red at the top.
pub fn heat_color(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let c = ramp(&[(0.0, [0.1, 0.15, 0.6]), (0.3, [0.1, 0.65, 0.95]), (0.6, [0.95, 0.9, 0.2]), (0.85, [1.0, 0.35, 0.1]), (1.0, [1.0, 0.1, 0.5])], t);
    let a = if t <= 0.0 { 0.0 } else { 0.18 + 0.4 * t.sqrt() };
    [c[0], c[1], c[2], a]
}

/// Pollution and noise: clear, sickly olive to brown and dark red.
pub fn pollution_color(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let c = ramp(&[(0.0, [0.55, 0.6, 0.2]), (0.5, [0.65, 0.4, 0.12]), (1.0, [0.6, 0.08, 0.06])], t);
    let a = if t <= 0.0 { 0.0 } else { 0.15 + 0.45 * t.sqrt() };
    [c[0], c[1], c[2], a]
}

fn ramp(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let (t0, a) = w[0];
        let (t1, b) = w[1];
        if t <= t1 {
            let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
        }
    }
    stops.last().map_or([1.0; 3], |s| s.1)
}

/// What a vehicle is, for the traffic types layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VehicleClass {
    Car,
    Truck,
    Bus,
    Emergency,
}

impl VehicleClass {
    pub const ALL: [VehicleClass; 4] = [VehicleClass::Car, VehicleClass::Truck, VehicleClass::Bus, VehicleClass::Emergency];

    pub fn label(self) -> &'static str {
        match self {
            VehicleClass::Car => "cars",
            VehicleClass::Truck => "trucks",
            VehicleClass::Bus => "buses",
            VehicleClass::Emergency => "emergency",
        }
    }

    /// Linear RGB, bright enough to glow a little.
    pub fn color(self) -> [f32; 3] {
        match self {
            VehicleClass::Car => [0.15, 0.75, 1.3],
            VehicleClass::Truck => [1.4, 0.6, 0.1],
            VehicleClass::Bus => [0.25, 1.3, 0.3],
            VehicleClass::Emergency => [1.5, 0.12, 0.2],
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Values on the corners of a grid of square cells, filled by splatting
/// points and drawn as one smooth translucent mesh.
#[derive(Clone, Debug)]
pub struct HeatGrid {
    pub origin: Vec2,
    pub cell: f32,
    /// Cells across and down; the values have one more of each.
    pub cols: usize,
    pub rows: usize,
    pub values: Vec<f32>,
}

impl HeatGrid {
    /// A grid over `lo..hi` with cells of at least `cell` metres, grown so
    /// there are at most `max_cells` of them.
    pub fn covering(lo: Vec2, hi: Vec2, cell: f32, max_cells: usize) -> Self {
        let size = (hi - lo).max(Vec2::splat(1.0));
        let mut cell = cell.max(1.0);
        while ((size.x / cell).ceil() * (size.y / cell).ceil()) as usize > max_cells.max(1) {
            cell *= 1.25;
        }
        let cols = (size.x / cell).ceil() as usize;
        let rows = (size.y / cell).ceil() as usize;
        Self { origin: lo, cell, cols, rows, values: vec![0.0; (cols + 1) * (rows + 1)] }
    }

    fn at(&self, i: usize, j: usize) -> f32 {
        self.values[j * (self.cols + 1) + i]
    }

    pub fn corner(&self, i: usize, j: usize) -> Vec2 {
        self.origin + Vec2::new(i as f32, j as f32) * self.cell
    }

    /// Add `w` at `p`, shared bilinearly between the four nearest corners.
    pub fn splat(&mut self, p: Vec2, w: f32) {
        let g = (p - self.origin) / self.cell;
        if g.x < 0.0 || g.y < 0.0 || g.x > self.cols as f32 || g.y > self.rows as f32 {
            return;
        }
        let (i, j) = ((g.x.floor() as usize).min(self.cols.saturating_sub(1)), (g.y.floor() as usize).min(self.rows.saturating_sub(1)));
        let (fx, fy) = ((g.x - i as f32).clamp(0.0, 1.0), (g.y - j as f32).clamp(0.0, 1.0));
        let n = self.cols + 1;
        self.values[j * n + i] += w * (1.0 - fx) * (1.0 - fy);
        self.values[j * n + i + 1] += w * fx * (1.0 - fy);
        self.values[(j + 1) * n + i] += w * (1.0 - fx) * fy;
        self.values[(j + 1) * n + i + 1] += w * fx * fy;
    }

    /// Add `w` falling off linearly to nothing at `radius` from `p`.
    pub fn splat_radius(&mut self, p: Vec2, radius: f32, w: f32) {
        let n = self.cols + 1;
        let r = (radius / self.cell).ceil() as i64;
        let g = ((p - self.origin) / self.cell).round();
        for dj in -r..=r {
            for di in -r..=r {
                let (i, j) = (g.x as i64 + di, g.y as i64 + dj);
                if i < 0 || j < 0 || i > self.cols as i64 || j > self.rows as i64 {
                    continue;
                }
                let d = self.corner(i as usize, j as usize).distance(p);
                if d < radius {
                    self.values[j as usize * n + i as usize] += w * (1.0 - d / radius);
                }
            }
        }
    }

    /// One pass of a 3x3 box blur, so single points read as soft blobs.
    pub fn blur(&mut self) {
        let n = self.cols + 1;
        let m = self.rows + 1;
        let src = self.values.clone();
        for j in 0..m {
            for i in 0..n {
                let mut sum = 0.0;
                let mut k = 0.0;
                for dj in -1i64..=1 {
                    for di in -1i64..=1 {
                        let (x, y) = (i as i64 + di, j as i64 + dj);
                        if x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < m {
                            sum += src[y as usize * n + x as usize];
                            k += 1.0;
                        }
                    }
                }
                self.values[j * n + i] = sum / k;
            }
        }
    }

    pub fn max(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }

    /// The grid as a ground mesh `y` above `height(p)`, coloured by
    /// `color(value / scale)`; cells whose four corners are all below 2% of
    /// the scale are left out.
    pub fn mesh(&self, scale: f32, y: f32, color: impl Fn(f32) -> [f32; 4], height: impl Fn(Vec2) -> f32) -> MeshData {
        let mut m = MeshData::default();
        if scale <= 0.0 {
            return m;
        }
        let n = self.cols + 1;
        let mut index: Vec<u32> = vec![u32::MAX; self.values.len()];
        let floor = scale * 0.02;
        for j in 0..self.rows {
            for i in 0..self.cols {
                let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                if corners.iter().all(|&(a, b)| self.at(a, b) < floor) {
                    continue;
                }
                let mut ids = [0u32; 4];
                for (k, &(a, b)) in corners.iter().enumerate() {
                    let slot = b * n + a;
                    if index[slot] == u32::MAX {
                        let p = self.corner(a, b);
                        index[slot] = m.positions.len() as u32;
                        m.positions.push([p.x, y + height(p), p.y]);
                        m.normals.push([0.0, 1.0, 0.0]);
                        m.colors.push(color((self.at(a, b) / scale).min(1.0)));
                        m.uvs.push([p.x, p.y]);
                        m.tangents.push([1.0, 0.0, 0.0, 1.0]);
                    }
                    ids[k] = index[slot];
                }
                // Counter-clockwise seen from above (+y): x then z.
                m.indices.extend_from_slice(&[ids[0], ids[2], ids[1], ids[0], ids[3], ids[2]]);
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_load_ranges() {
        assert_eq!(traffic_load(0, 100.0, 0.0, 14.0), 0.0, "empty lane");
        let free = traffic_load(1, 200.0, 13.0, 14.0);
        let jam = traffic_load(14, 100.0, 0.2, 14.0);
        let light_wait = traffic_load(1, 200.0, 0.0, 14.0);
        assert!(free < 0.1, "one car at speed: {free}");
        assert!(jam > 0.9, "a stopped queue end to end: {jam}");
        assert!(light_wait < 0.3, "one car at a red light is no jam: {light_wait}");
        assert!(traffic_load(8, 100.0, 7.0, 14.0) > traffic_load(4, 100.0, 7.0, 14.0));
        assert!(traffic_load(6, 100.0, 1.0, 14.0) > traffic_load(6, 100.0, 13.0, 14.0), "slower is worse");
        let (g, r) = (traffic_color(0.0), traffic_color(0.85));
        assert!(g[1] > g[0] && r[0] > r[1], "green when free, red when jammed");
    }

    #[test]
    fn heat_grid_conserves_and_draws_only_where_warm() {
        let mut h = HeatGrid::covering(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0), 10.0, 10_000);
        assert_eq!((h.cols, h.rows), (20, 20));
        h.splat(Vec2::new(3.0, 4.0), 5.0);
        h.splat(Vec2::new(-1000.0, 0.0), 5.0);
        let total: f32 = h.values.iter().sum();
        assert!((total - 5.0).abs() < 1e-4, "a splat keeps its weight; points off the grid are dropped: {total}");
        h.blur();
        assert!(h.max() > 0.0);
        let m = h.mesh(h.max(), 0.4, heat_color, |_| 0.0);
        assert!(!m.is_empty());
        assert!(m.indices.len() / 6 < 30, "only the warm cells are drawn: {}", m.indices.len() / 6);
        assert!(m.positions.iter().all(|p| p[1] == 0.4));
        let coarse = HeatGrid::covering(Vec2::ZERO, Vec2::new(2000.0, 2000.0), 5.0, 4000);
        assert!(coarse.cols * coarse.rows <= 4000, "grown to fit the cell budget");
        let mut r = HeatGrid::covering(Vec2::ZERO, Vec2::new(100.0, 100.0), 10.0, 1000);
        r.splat_radius(Vec2::new(50.0, 50.0), 30.0, 1.0);
        assert!(r.at(5, 5) > 0.9 && r.at(0, 0) == 0.0, "radial falloff");
        assert_eq!(heat_color(0.0)[3], 0.0, "zero is clear");
    }
}
