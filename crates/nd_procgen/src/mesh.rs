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
}
