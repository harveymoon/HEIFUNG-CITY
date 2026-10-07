//! The tensor field streets follow (Chen et al. 2008, "Interactive
//! Procedural Street Modeling"). A 2D symmetric traceless tensor
//! `R [cos 2θ  sin 2θ; sin 2θ  -cos 2θ]` has two perpendicular eigenvectors:
//! the major one at angle θ and the minor one at θ + 90°. Streets traced
//! along the major field and the minor field cross at right angles, which
//! is what makes blocks.
//!
//! The field is a weighted sum of basis fields, each fading with distance
//! from where it is anchored: grids (one direction everywhere), radial
//! fields (rings round a centre, spokes out of it) and line fields (along a
//! river, a coast, a highway or a railway). Summing tensors (not angles)
//! blends them smoothly; where they cancel the field is degenerate and
//! streets stop. Noise can twist the result in places for organic streets.

use glam::Vec2;

/// A symmetric traceless tensor stored as `r (cos 2θ, sin 2θ)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tensor {
    pub a: f32,
    pub b: f32,
}

impl Tensor {
    pub fn from_angle(theta: f32, r: f32) -> Self {
        Self { a: r * (2.0 * theta).cos(), b: r * (2.0 * theta).sin() }
    }

    /// Strength: zero where the blend cancels out.
    pub fn norm(self) -> f32 {
        (self.a * self.a + self.b * self.b).sqrt()
    }

    pub fn angle(self) -> f32 {
        0.5 * self.b.atan2(self.a)
    }

    /// Unit major eigenvector (its sign is arbitrary).
    pub fn major(self) -> Vec2 {
        Vec2::from_angle(self.angle())
    }

    /// Unit minor eigenvector, perpendicular to the major one.
    pub fn minor(self) -> Vec2 {
        let m = self.major();
        Vec2::new(-m.y, m.x)
    }

    fn scaled(self, k: f32) -> Self {
        Self { a: self.a * k, b: self.b * k }
    }

    fn add(self, o: Self) -> Self {
        Self { a: self.a + o.a, b: self.b + o.b }
    }

    fn rotated(self, phi: f32) -> Self {
        // Rotating the eigenvectors by phi turns (cos 2θ, sin 2θ) by 2 phi.
        let (s, c) = (2.0 * phi).sin_cos();
        Self { a: self.a * c - self.b * s, b: self.a * s + self.b * c }
    }
}

/// One basis field and where it holds.
#[derive(Clone, Debug)]
pub enum Basis {
    /// Streets at `theta` (and across it) everywhere near `centre`.
    Grid { centre: Vec2, size: f32, theta: f32, weight: f32 },
    /// Rings round `centre` (major) and spokes out of it (minor).
    Radial { centre: Vec2, size: f32, weight: f32 },
    /// Streets along a polyline (major) and across it (minor), fading with
    /// distance from it.
    Line { points: Vec<Vec2>, size: f32, weight: f32 },
}

/// A patch of twisted streets: the field's angle turned by up to `angle`
/// radians, following smooth noise of wavelength `wavelength`.
#[derive(Clone, Debug)]
pub struct Twist {
    pub centre: Vec2,
    pub size: f32,
    pub angle: f32,
    pub wavelength: f32,
}

fn falloff(d: f32, size: f32) -> f32 {
    let x = d / size.max(1.0);
    (-x * x).exp()
}

/// Nearest point on a polyline to `p`: (distance, unit direction there).
fn nearest_on(points: &[Vec2], p: Vec2, reach: f32) -> Option<(f32, Vec2)> {
    let mut best: Option<(f32, Vec2)> = None;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Cheap reject: the segment's box is farther than `reach`.
        let lo = a.min(b) - Vec2::splat(reach);
        let hi = a.max(b) + Vec2::splat(reach);
        if p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y {
            continue;
        }
        let (_, q) = nd_sim::geom::project_on_segment(p, a, b);
        let d = q.distance(p);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, (b - a).normalize_or_zero()));
        }
    }
    best
}

#[derive(Clone, Debug, Default)]
pub struct TensorField {
    pub bases: Vec<Basis>,
    pub twists: Vec<Twist>,
    pub seed: u64,
}

impl TensorField {
    /// The blended field at `p`, computed exactly.
    pub fn eval(&self, p: Vec2) -> Tensor {
        let mut t = Tensor::default();
        for b in &self.bases {
            match b {
                Basis::Grid { centre, size, theta, weight } => {
                    t = t.add(Tensor::from_angle(*theta, 1.0).scaled(weight * falloff(p.distance(*centre), *size)));
                }
                Basis::Radial { centre, size, weight } => {
                    let d = p - *centre;
                    let r = d.length();
                    if r > 1e-3 {
                        // Major along the ring: perpendicular to the spoke.
                        let theta = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
                        t = t.add(Tensor::from_angle(theta, 1.0).scaled(weight * falloff(r, *size)));
                    }
                }
                Basis::Line { points, size, weight } => {
                    if let Some((d, dir)) = nearest_on(points, p, size * 2.5) {
                        let w = weight * falloff(d, *size);
                        if w > 1e-4 {
                            t = t.add(Tensor::from_angle(dir.y.atan2(dir.x), 1.0).scaled(w));
                        }
                    }
                }
            }
        }
        for tw in &self.twists {
            let w = falloff(p.distance(tw.centre), tw.size);
            if w < 1e-3 {
                continue;
            }
            let n = nd_sim::landscape::fbm(self.seed ^ 0x7157, p / tw.wavelength.max(1.0), 2) * 2.0 - 1.0;
            t = t.rotated(n * tw.angle * w);
        }
        t
    }
}

/// The field sampled on a regular raster and read back bilinearly: the
/// streamline tracer samples it hundreds of thousands of times.
pub struct FieldRaster {
    origin: Vec2,
    cell: f32,
    nx: usize,
    ny: usize,
    data: Vec<Tensor>,
}

impl FieldRaster {
    pub fn new(field: &TensorField, lo: Vec2, hi: Vec2, cell: f32) -> Self {
        let nx = ((hi.x - lo.x) / cell).ceil() as usize + 2;
        let ny = ((hi.y - lo.y) / cell).ceil() as usize + 2;
        let mut data = Vec::with_capacity(nx * ny);
        for j in 0..ny {
            for i in 0..nx {
                data.push(field.eval(lo + Vec2::new(i as f32, j as f32) * cell));
            }
        }
        Self { origin: lo, cell, nx, ny, data }
    }

    pub fn sample(&self, p: Vec2) -> Tensor {
        let q = (p - self.origin) / self.cell;
        let fx = q.x.clamp(0.0, (self.nx - 1) as f32 - 1e-3);
        let fy = q.y.clamp(0.0, (self.ny - 1) as f32 - 1e-3);
        let (i, j) = (fx as usize, fy as usize);
        let (u, v) = (fx - i as f32, fy - j as f32);
        let at = |i: usize, j: usize| self.data[j * self.nx + i];
        let (t00, t10, t01, t11) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
        let a = (t00.a * (1.0 - u) + t10.a * u) * (1.0 - v) + (t01.a * (1.0 - u) + t11.a * u) * v;
        let b = (t00.b * (1.0 - u) + t10.b * u) * (1.0 - v) + (t01.b * (1.0 - u) + t11.b * u) * v;
        Tensor { a, b }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigenvectors_are_perpendicular_and_follow_the_bases() {
        let t = Tensor::from_angle(0.3, 1.0);
        assert!((t.angle() - 0.3).abs() < 1e-5);
        assert!(t.major().dot(t.minor()).abs() < 1e-5);
        let f = TensorField { bases: vec![Basis::Radial { centre: Vec2::ZERO, size: 1000.0, weight: 1.0 }], ..Default::default() };
        // East of the centre the rings run north-south.
        let m = f.eval(Vec2::new(100.0, 0.0)).major();
        assert!(m.x.abs() < 1e-3, "{m}");
        let f = TensorField { bases: vec![Basis::Line { points: vec![Vec2::new(-500.0, -500.0), Vec2::new(500.0, 500.0)], size: 200.0, weight: 1.0 }], ..Default::default() };
        let m = f.eval(Vec2::new(10.0, 0.0)).major();
        assert!((m.x.abs() - m.y.abs()).abs() < 1e-3, "{m}");
    }
}
