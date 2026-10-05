use glam::{Vec2, Vec3};

/// Engine-independent triangle mesh with per-vertex colours. Colours may
/// exceed 1.0 for HDR glow when rendered through an unlit material.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn append(&mut self, other: &MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colors.extend_from_slice(&other.colors);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// A quad from four corners in counter-clockwise order (seen from the
    /// front). Normal is computed from the corners.
    pub fn quad(&mut self, corners: [Vec3; 4], color: [f32; 4]) {
        let n = (corners[1] - corners[0]).cross(corners[3] - corners[0]).normalize_or_zero();
        let base = self.positions.len() as u32;
        for c in corners {
            self.positions.push(c.to_array());
            self.normals.push(n.to_array());
            self.colors.push(color);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Axis-aligned box in local space rotated by `yaw` around Y and translated.
    pub fn boxed(&mut self, center: Vec3, size: Vec3, yaw: f32, color: [f32; 4]) {
        let h = size * 0.5;
        let (s, c) = yaw.sin_cos();
        let rot = |p: Vec3| Vec3::new(p.x * c - p.z * s, p.y, p.x * s + p.z * c) + center;
        let corner = |x: f32, y: f32, z: f32| rot(Vec3::new(x * h.x, y * h.y, z * h.z));
        // +Y top
        self.quad([corner(-1., 1., 1.), corner(1., 1., 1.), corner(1., 1., -1.), corner(-1., 1., -1.)], color);
        // -Y bottom
        self.quad([corner(-1., -1., -1.), corner(1., -1., -1.), corner(1., -1., 1.), corner(-1., -1., 1.)], color);
        // +Z
        self.quad([corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.)], color);
        // -Z
        self.quad([corner(1., -1., -1.), corner(-1., -1., -1.), corner(-1., 1., -1.), corner(1., 1., -1.)], color);
        // +X
        self.quad([corner(1., -1., 1.), corner(1., -1., -1.), corner(1., 1., -1.), corner(1., 1., 1.)], color);
        // -X
        self.quad([corner(-1., -1., -1.), corner(-1., -1., 1.), corner(-1., 1., 1.), corner(-1., 1., -1.)], color);
    }

    /// Flat polygon on the ground plane at height `y` (fan triangulation, so
    /// it must be convex or close to it).
    pub fn ground_polygon(&mut self, poly: &[Vec2], y: f32, color: [f32; 4]) {
        if poly.len() < 3 {
            return;
        }
        let base = self.positions.len() as u32;
        let ccw = nd_sim::zoning::polygon_area(poly) > 0.0;
        for p in poly {
            self.positions.push([p.x, y, p.y]);
            self.normals.push([0.0, 1.0, 0.0]);
            self.colors.push(color);
        }
        for i in 1..poly.len() as u32 - 1 {
            if ccw {
                self.indices.extend_from_slice([base, base + i + 1, base + i].as_slice());
            } else {
                self.indices.extend_from_slice([base, base + i, base + i + 1].as_slice());
            }
        }
    }

    /// A flat strip of `width` along a polyline on the ground.
    pub fn ground_strip(&mut self, points: &[Vec2], width: f32, y: f32, color: [f32; 4]) {
        for w in points.windows(2) {
            let d = (w[1] - w[0]).normalize_or_zero();
            let n = Vec2::new(-d.y, d.x) * width * 0.5;
            let a = w[0] + n;
            let b = w[1] + n;
            let c = w[1] - n;
            let e = w[0] - n;
            self.ground_polygon(&[a, b, c, e], y, color);
        }
    }

    /// Triangle with the normal flipped, if needed, to face `outward`.
    pub fn tri_facing(&mut self, a: Vec3, b: Vec3, c: Vec3, outward: Vec3, color: [f32; 4]) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        let (b, c, n) = if n.dot(outward) < 0.0 { (c, b, -n) } else { (b, c, n) };
        let base = self.positions.len() as u32;
        for p in [a, b, c] {
            self.positions.push(p.to_array());
            self.normals.push(n.to_array());
            self.colors.push(color);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Quad whose normal is made to face `outward`.
    pub fn quad_facing(&mut self, corners: [Vec3; 4], outward: Vec3, color: [f32; 4]) {
        let n = (corners[1] - corners[0]).cross(corners[3] - corners[0]).normalize_or_zero();
        if n.dot(outward) < 0.0 {
            self.quad([corners[0], corners[3], corners[2], corners[1]], color);
        } else {
            self.quad(corners, color);
        }
    }

    /// Solid between two polygons with the same vertex count (a tapered or
    /// leaning tower when `top` is a scaled, shifted copy of `bottom`).
    pub fn frustum(&mut self, bottom: &[Vec3], top: &[Vec3], color: [f32; 4]) {
        let n = bottom.len().min(top.len());
        if n < 3 {
            return;
        }
        let centroid = (bottom.iter().chain(top.iter()).fold(Vec3::ZERO, |acc, p| acc + *p)) / (2 * n) as f32;
        for i in 0..n {
            let j = (i + 1) % n;
            let mid = (bottom[i] + bottom[j] + top[j] + top[i]) * 0.25;
            self.quad_facing([bottom[i], bottom[j], top[j], top[i]], mid - centroid, color);
        }
        let top_c = top.iter().fold(Vec3::ZERO, |a, p| a + *p) / n as f32;
        let bot_c = bottom.iter().fold(Vec3::ZERO, |a, p| a + *p) / n as f32;
        for i in 0..n {
            let j = (i + 1) % n;
            self.tri_facing(top_c, top[i], top[j], Vec3::Y, color);
            self.tri_facing(bot_c, bottom[i], bottom[j], Vec3::NEG_Y, color);
        }
    }

    /// Regular polygon in the XZ plane at height `y`.
    pub fn ngon(center: Vec2, radius: f32, sides: usize, rotation: f32, y: f32) -> Vec<Vec3> {
        (0..sides)
            .map(|i| {
                let a = rotation + i as f32 / sides as f32 * std::f32::consts::TAU;
                Vec3::new(center.x + a.cos() * radius, y, center.y + a.sin() * radius)
            })
            .collect()
    }

    /// Rectangle corners in the XZ plane, rotated by `yaw`, at height `y`.
    pub fn rect(center: Vec2, half_w: f32, half_d: f32, yaw: f32, y: f32) -> Vec<Vec3> {
        let (s, c) = yaw.sin_cos();
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(u, v)| {
                let x = u * half_w;
                let z = v * half_d;
                Vec3::new(center.x + x * c - z * s, y, center.y + x * s + z * c)
            })
            .collect()
    }

    /// Thin square-section bar between two points in space (wires, braces).
    pub fn beam(&mut self, a: Vec3, b: Vec3, thickness: f32, color: [f32; 4]) {
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            return;
        }
        let dir = d / len;
        let helper = if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
        let p1 = dir.cross(helper).normalize() * thickness * 0.5;
        let p2 = dir.cross(p1).normalize() * thickness * 0.5;
        let c = [a + p1 + p2, a - p1 + p2, a - p1 - p2, a + p1 - p2];
        let e = [b + p1 + p2, b - p1 + p2, b - p1 - p2, b + p1 - p2];
        self.frustum(&c, &e, color);
    }
}
