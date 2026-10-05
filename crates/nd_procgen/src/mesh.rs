use glam::{Vec2, Vec3};

/// Engine-independent triangle mesh with per-vertex colours and world-scaled
/// UVs. Colours may exceed 1.0 for HDR glow when rendered through an unlit
/// material. UVs are planar projections in metres (u horizontal, v up for
/// walls), so a repeating texture lines up across every face of a building.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Texture basis for a face: `u` runs horizontally, `v` runs up the wall (or
/// along -Z on horizontal faces), both in world metres.
fn uv_basis(n: Vec3) -> (Vec3, Vec3) {
    if n.y.abs() > 0.99 || n.length_squared() < 0.5 {
        (Vec3::X, Vec3::NEG_Z)
    } else {
        let u = Vec3::Y.cross(n).normalize_or_zero();
        if u == Vec3::ZERO {
            return (Vec3::X, Vec3::NEG_Z);
        }
        (u, n.cross(u))
    }
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
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    fn push(&mut self, p: Vec3, n: Vec3, color: [f32; 4], basis: (Vec3, Vec3)) {
        self.positions.push(p.to_array());
        self.normals.push(n.to_array());
        self.colors.push(color);
        self.uvs.push([p.dot(basis.0), p.dot(basis.1)]);
    }

    /// A quad from four corners in counter-clockwise order (seen from the
    /// front). Normal is computed from the corners.
    pub fn quad(&mut self, corners: [Vec3; 4], color: [f32; 4]) {
        let n = (corners[1] - corners[0]).cross(corners[3] - corners[0]).normalize_or_zero();
        if n == Vec3::ZERO {
            // Degenerate (zero-area) quad: nothing to draw, and its UVs would be NaN.
            return;
        }
        let basis = uv_basis(n);
        let base = self.positions.len() as u32;
        for c in corners {
            self.push(c, n, color, basis);
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

    /// Box without its bottom face, for things that sit on the ground.
    pub fn boxed_open(&mut self, center: Vec3, size: Vec3, yaw: f32, color: [f32; 4]) {
        let h = size * 0.5;
        let (s, c) = yaw.sin_cos();
        let rot = |p: Vec3| Vec3::new(p.x * c - p.z * s, p.y, p.x * s + p.z * c) + center;
        let corner = |x: f32, y: f32, z: f32| rot(Vec3::new(x * h.x, y * h.y, z * h.z));
        self.quad([corner(-1., 1., 1.), corner(1., 1., 1.), corner(1., 1., -1.), corner(-1., 1., -1.)], color);
        self.quad([corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.)], color);
        self.quad([corner(1., -1., -1.), corner(-1., -1., -1.), corner(-1., 1., -1.), corner(1., 1., -1.)], color);
        self.quad([corner(1., -1., 1.), corner(1., -1., -1.), corner(1., 1., -1.), corner(1., 1., 1.)], color);
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
        let basis = uv_basis(Vec3::Y);
        for p in poly {
            self.push(Vec3::new(p.x, y, p.y), Vec3::Y, color, basis);
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

    /// Raised strip (kerb, sidewalk, wall footing) running from `a` to `b`,
    /// shifted sideways by `offset` (positive = right of travel), `width`
    /// across, from height `y0` to `y1`. Top and all four sides are drawn.
    pub fn strip_box(&mut self, a: Vec2, b: Vec2, offset: f32, width: f32, y0: f32, y1: f32, color: [f32; 4]) {
        let d = (b - a).normalize_or_zero();
        if d == Vec2::ZERO {
            return;
        }
        let right = Vec2::new(-d.y, d.x);
        let c0 = a + right * (offset - width * 0.5);
        let c1 = b + right * (offset - width * 0.5);
        let c2 = b + right * (offset + width * 0.5);
        let c3 = a + right * (offset + width * 0.5);
        let lo = |p: Vec2| Vec3::new(p.x, y0, p.y);
        let hi = |p: Vec2| Vec3::new(p.x, y1, p.y);
        self.quad_facing([hi(c0), hi(c1), hi(c2), hi(c3)], Vec3::Y, color);
        let out_l = Vec3::new(-right.x, 0.0, -right.y);
        let out_r = -out_l;
        self.quad_facing([lo(c0), lo(c1), hi(c1), hi(c0)], out_l, color);
        self.quad_facing([lo(c3), lo(c2), hi(c2), hi(c3)], out_r, color);
        let fwd = Vec3::new(d.x, 0.0, d.y);
        self.quad_facing([lo(c1), lo(c2), hi(c2), hi(c1)], fwd, color);
        self.quad_facing([lo(c0), lo(c3), hi(c3), hi(c0)], -fwd, color);
    }

    /// Triangle with the normal flipped, if needed, to face `outward`.
    pub fn tri_facing(&mut self, a: Vec3, b: Vec3, c: Vec3, outward: Vec3, color: [f32; 4]) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        if n == Vec3::ZERO {
            return;
        }
        let (b, c, n) = if n.dot(outward) < 0.0 { (c, b, -n) } else { (b, c, n) };
        let basis = uv_basis(n);
        let base = self.positions.len() as u32;
        for p in [a, b, c] {
            self.push(p, n, color, basis);
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
        // Each cap faces away from the solid, whichever way it lies: a
        // horizontal pipe's ends, a descending ramp's, a vertical column's.
        let top_out = if (top_c - centroid).length_squared() > 1e-6 { top_c - centroid } else { Vec3::Y };
        let bot_out = if (bot_c - centroid).length_squared() > 1e-6 { bot_c - centroid } else { Vec3::NEG_Y };
        for i in 0..n {
            let j = (i + 1) % n;
            self.tri_facing(top_c, top[i], top[j], top_out, color);
            self.tri_facing(bot_c, bottom[i], bottom[j], bot_out, color);
        }
    }

    /// Vertical cylinder (or cone when `r1 != r0`) between two heights.
    pub fn cylinder(&mut self, center: Vec2, r0: f32, r1: f32, y0: f32, y1: f32, sides: usize, color: [f32; 4]) {
        self.frustum(&MeshData::ngon(center, r0, sides, 0.0, y0), &MeshData::ngon(center, r1, sides, 0.0, y1), color);
    }

    /// Horizontal cylinder along the direction `dir` (unit, in the ground
    /// plane), centred at `center`, at height `y`.
    pub fn pipe(&mut self, center: Vec2, dir: Vec2, len: f32, r: f32, y: f32, sides: usize, color: [f32; 4]) {
        let a = Vec3::new(center.x - dir.x * len * 0.5, y, center.y - dir.y * len * 0.5);
        let b = Vec3::new(center.x + dir.x * len * 0.5, y, center.y + dir.y * len * 0.5);
        self.tube(a, b, r, sides, color);
    }

    /// Round tube between two points in space.
    pub fn tube(&mut self, a: Vec3, b: Vec3, r: f32, sides: usize, color: [f32; 4]) {
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            return;
        }
        let dir = d / len;
        let helper = if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
        let p1 = dir.cross(helper).normalize();
        let p2 = dir.cross(p1).normalize();
        let ring = |c: Vec3| -> Vec<Vec3> {
            (0..sides)
                .map(|i| {
                    let t = i as f32 / sides as f32 * std::f32::consts::TAU;
                    c + (p1 * t.cos() + p2 * t.sin()) * r
                })
                .collect()
        };
        self.frustum(&ring(a), &ring(b), color);
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
    /// A flat slab from `a` to `b`, `width` across (horizontal) and `thick`
    /// deep, sitting on the a-b line: a ramp or a bridge deck.
    pub fn slab(&mut self, a: Vec3, b: Vec3, width: f32, thick: f32, color: [f32; 4]) {
        let d = b - a;
        let flat = Vec3::new(d.x, 0.0, d.z);
        if flat.length() < 1e-3 {
            return;
        }
        let side = flat.normalize().cross(Vec3::Y) * width * 0.5;
        let up = side.cross(d.normalize()).normalize();
        let up = if up.y < 0.0 { -up } else { up } * thick;
        let c = [a + side, a - side, a - side - up, a + side - up];
        let e = [b + side, b - side, b - side - up, b + side - up];
        self.frustum(&c, &e, color);
    }

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

    /// Flat strip along a polyline with mitred joins, shifted sideways by
    /// `offset` (positive = right of travel). Continuous through bends.
    pub fn path_strip(&mut self, points: &[Vec2], offset: f32, width: f32, y: f32, color: [f32; 4]) {
        let l = offset_path(points, offset - width * 0.5);
        let r = offset_path(points, offset + width * 0.5);
        for i in 0..l.len().saturating_sub(1) {
            self.ground_polygon(&[l[i], l[i + 1], r[i + 1], r[i]], y, color);
        }
    }

    /// Raised strip along a polyline with mitred joins (kerbs, pavements).
    pub fn path_box(&mut self, points: &[Vec2], offset: f32, width: f32, y0: f32, y1: f32, color: [f32; 4]) {
        let l = offset_path(points, offset - width * 0.5);
        let r = offset_path(points, offset + width * 0.5);
        let lo = |p: Vec2| Vec3::new(p.x, y0, p.y);
        let hi = |p: Vec2| Vec3::new(p.x, y1, p.y);
        let n = l.len();
        if n < 2 {
            return;
        }
        for i in 0..n - 1 {
            self.quad_facing([hi(l[i]), hi(l[i + 1]), hi(r[i + 1]), hi(r[i])], Vec3::Y, color);
            let d = (points.get(i + 1).copied().unwrap_or(points[i]) - points[i]).normalize_or_zero();
            let out_l = Vec3::new(d.y, 0.0, -d.x);
            self.quad_facing([lo(l[i]), lo(l[i + 1]), hi(l[i + 1]), hi(l[i])], out_l, color);
            self.quad_facing([lo(r[i]), lo(r[i + 1]), hi(r[i + 1]), hi(r[i])], -out_l, color);
        }
        let d0 = (points[1] - points[0]).normalize_or_zero();
        let d1 = (points[n - 1] - points[n - 2]).normalize_or_zero();
        self.quad_facing([lo(l[0]), lo(r[0]), hi(r[0]), hi(l[0])], Vec3::new(-d0.x, 0.0, -d0.y), color);
        self.quad_facing([lo(l[n - 1]), lo(r[n - 1]), hi(r[n - 1]), hi(l[n - 1])], Vec3::new(d1.x, 0.0, d1.y), color);
    }

    /// A wire that sags under its own weight between two anchors.
    pub fn wire(&mut self, a: Vec3, b: Vec3, sag: f32, thickness: f32, color: [f32; 4]) {
        let segs = 6;
        let mut prev = a;
        for i in 1..=segs {
            let t = i as f32 / segs as f32;
            let p = a.lerp(b, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t));
            self.beam(prev, p, thickness, color);
            prev = p;
        }
    }
}

/// Polyline shifted sideways by `offset` (positive = right of travel) with
/// mitred corners, limited so sharp bends do not spike.
pub fn offset_path(points: &[Vec2], offset: f32) -> Vec<Vec2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let d_in = if i > 0 { (points[i] - points[i - 1]).normalize_or_zero() } else { Vec2::ZERO };
        let d_out = if i + 1 < n { (points[i + 1] - points[i]).normalize_or_zero() } else { Vec2::ZERO };
        let d_in = if d_in == Vec2::ZERO { d_out } else { d_in };
        let d_out = if d_out == Vec2::ZERO { d_in } else { d_out };
        let n_in = Vec2::new(-d_in.y, d_in.x);
        let n_out = Vec2::new(-d_out.y, d_out.x);
        let m = (n_in + n_out).normalize_or_zero();
        let (m, scale) = if m == Vec2::ZERO { (n_in, 1.0) } else { (m, (1.0 / m.dot(n_in).max(1e-3)).min(2.0)) };
        out.push(points[i] + m * offset * scale);
    }
    out
}

/// Total length of a polyline.
pub fn path_length(points: &[Vec2]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Point and direction at arc length `s` along a polyline.
pub fn point_along(points: &[Vec2], s: f32) -> (Vec2, Vec2) {
    let mut acc = 0.0;
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        if len < 1e-4 {
            continue;
        }
        if s <= acc + len {
            let t = ((s - acc) / len).clamp(0.0, 1.0);
            return (w[0].lerp(w[1], t), (w[1] - w[0]) / len);
        }
        acc += len;
    }
    let n = points.len();
    if n >= 2 {
        (points[n - 1], (points[n - 1] - points[n - 2]).normalize_or_zero())
    } else {
        (points.first().copied().unwrap_or(Vec2::ZERO), Vec2::X)
    }
}
