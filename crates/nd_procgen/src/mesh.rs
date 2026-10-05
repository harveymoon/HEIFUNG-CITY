use glam::{Vec2, Vec3};

pub use nd_sim::rail::{path_length, point_along};

/// Engine-independent triangle mesh with per-vertex colours and world-scaled
/// UVs. Colours may exceed 1.0 for HDR glow when rendered through an unlit
/// material. UVs are planar projections in metres (u horizontal, v up for
/// walls), so a repeating texture lines up across every face of a building.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    /// Tangent per vertex (xyz, handedness w): the texture `u` axis, so no
    /// mikktspace pass is needed on the renderer side.
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// Texture basis for a face: `u` runs horizontally, `v` runs up the wall,
/// both in world metres. A horizontal face takes `edge` (its own first
/// edge) as `u`, so the top of a rotated box or a mitred pavement follows
/// the object instead of the world axes; only `ground_polygon` passes none
/// and keeps the world axes, which is what a lot's ground wants.
fn uv_basis(n: Vec3, edge: Option<Vec3>) -> (Vec3, Vec3) {
    if n.y.abs() > 0.99 || n.length_squared() < 0.5 {
        if let Some(e) = edge {
            let u = (e - n * e.dot(n)).normalize_or_zero();
            if u != Vec3::ZERO {
                return (u, n.cross(u));
            }
        }
        (Vec3::X, Vec3::NEG_Z)
    } else {
        let u = Vec3::Y.cross(n).normalize_or_zero();
        if u == Vec3::ZERO {
            return (Vec3::X, Vec3::NEG_Z);
        }
        (u, n.cross(u))
    }
}

/// Unnormalised Newell normal of a closed ring: its direction follows the
/// right-hand rule round the ring, its length is twice the enclosed area.
fn newell(ring: &[Vec3]) -> Vec3 {
    let n = ring.len();
    let mut acc = Vec3::ZERO;
    for i in 0..n {
        acc += ring[i].cross(ring[(i + 1) % n]);
    }
    acc
}

fn centroid(ring: &[Vec3]) -> Vec3 {
    ring.iter().fold(Vec3::ZERO, |a, p| a + *p) / ring.len().max(1) as f32
}

fn cross2(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a - o).perp_dot(b - o)
}

/// Whether every turn of a simple polygon goes the same way.
pub fn is_convex(poly: &[Vec2]) -> bool {
    let n = poly.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let c = cross2(poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

fn point_in_tri(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = cross2(a, b, p);
    let d2 = cross2(b, c, p);
    let d3 = cross2(c, a, p);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Ear-clipping triangulation of a simple polygon in either winding. The
/// triangles come back as vertex indices in the polygon's own winding. Lots
/// are concave (L-shapes, notched corners), so a centroid fan would put
/// triangles outside the outline and flip their normals; this never does.
/// When no ear can be found (a degenerate or self-touching outline) the
/// rest is fanned so the caller still gets a closed surface.
pub fn triangulate(poly: &[Vec2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    if n < 3 {
        return out;
    }
    let area: f32 = (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum();
    let orient = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut idx: Vec<usize> = (0..n).collect();
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (i0, i1, i2) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[i0], poly[i1], poly[i2]);
            if cross2(a, b, c) * orient <= 1e-7 {
                continue; // reflex or collinear corner: not an ear
            }
            let blocked = idx.iter().any(|&j| j != i0 && j != i1 && j != i2 && point_in_tri(poly[j], a, b, c));
            if blocked {
                continue;
            }
            out.push([i0, i1, i2]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    // Whatever is left (a triangle, or an outline too tangled to clip).
    for k in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[k], idx[k + 1]]);
    }
    out
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
        self.tangents.extend_from_slice(&other.tangents);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    fn push(&mut self, p: Vec3, n: Vec3, color: [f32; 4], basis: (Vec3, Vec3)) {
        self.positions.push(p.to_array());
        self.normals.push(n.to_array());
        self.colors.push(color);
        self.uvs.push([p.dot(basis.0), p.dot(basis.1)]);
        self.tangents.push([basis.0.x, basis.0.y, basis.0.z, 1.0]);
    }

    /// A quad from four corners in counter-clockwise order (seen from the
    /// front). Normal is computed from the corners.
    pub fn quad(&mut self, corners: [Vec3; 4], color: [f32; 4]) {
        let n = (corners[1] - corners[0]).cross(corners[3] - corners[0]).normalize_or_zero();
        if n == Vec3::ZERO {
            // Degenerate (zero-area) quad: nothing to draw, and its UVs would be NaN.
            return;
        }
        let basis = uv_basis(n, Some(corners[1] - corners[0]));
        let base = self.positions.len() as u32;
        for c in corners {
            self.push(c, n, color, basis);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// One quad, `w` along `along` and `h` along `up`, centred at `center`
    /// and facing `facing`: a window pane, a painted line, a poster. Two
    /// triangles where a closed box would spend twelve on faces nobody sees.
    pub fn decal(&mut self, center: Vec3, along: Vec3, up: Vec3, w: f32, h: f32, facing: Vec3, color: [f32; 4]) {
        let a = along.normalize_or_zero() * (w * 0.5);
        let u = up.normalize_or_zero() * (h * 0.5);
        self.quad_facing([center - a - u, center + a - u, center + a + u, center - a + u], facing, color);
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

    /// Flat polygon on the ground plane at height `y`, facing up. Convex
    /// outlines are fanned; concave ones (most lots) are ear-clipped so no
    /// triangle lands outside the outline. UVs follow the world axes so the
    /// ground texture is continuous from one lot to the next.
    pub fn ground_polygon(&mut self, poly: &[Vec2], y: f32, color: [f32; 4]) {
        if poly.len() < 3 {
            return;
        }
        let base = self.positions.len() as u32;
        // A counter-clockwise outline in (x, z) seen from +Y winds clockwise
        // once y becomes z, so its triangles need reversing to face up.
        let ccw = nd_sim::zoning::polygon_area(poly) > 0.0;
        let basis = uv_basis(Vec3::Y, None);
        for p in poly {
            self.push(Vec3::new(p.x, y, p.y), Vec3::Y, color, basis);
        }
        let mut tri = |a: u32, b: u32, c: u32| {
            if ccw {
                self.indices.extend_from_slice(&[base + a, base + c, base + b]);
            } else {
                self.indices.extend_from_slice(&[base + a, base + b, base + c]);
            }
        };
        if is_convex(poly) {
            for i in 1..poly.len() as u32 - 1 {
                tri(0, i, i + 1);
            }
        } else {
            for t in triangulate(poly) {
                tri(t[0] as u32, t[1] as u32, t[2] as u32);
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
        let basis = uv_basis(n, Some(b - a));
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

    /// Flat face closing a ring, facing `out`. The ring's vertices are
    /// pushed once and shared by every triangle: `n` vertices for a convex
    /// ring instead of three per triangle. Concave rings are ear-clipped.
    pub fn cap(&mut self, ring: &[Vec3], out: Vec3, color: [f32; 4]) {
        let n = ring.len();
        if n < 3 {
            return;
        }
        let mut normal = newell(ring);
        if normal.length_squared() < 1e-12 {
            return;
        }
        let flip = normal.dot(out) < 0.0;
        if flip {
            normal = -normal;
        }
        let normal = normal.normalize();
        let basis = uv_basis(normal, Some(ring[1] - ring[0]));
        let flat: Vec<Vec2> = ring.iter().map(|p| Vec2::new(p.dot(basis.0), p.dot(basis.1))).collect();
        let base = self.positions.len() as u32;
        for p in ring {
            self.push(*p, normal, color, basis);
        }
        let mut tri = |a: usize, b: usize, c: usize| {
            let (b, c) = if flip { (c, b) } else { (b, c) };
            self.indices.extend_from_slice(&[base + a as u32, base + b as u32, base + c as u32]);
        };
        if is_convex(&flat) {
            for i in 1..n - 1 {
                tri(0, i, i + 1);
            }
        } else {
            for t in triangulate(&flat) {
                tri(t[0], t[1], t[2]);
            }
        }
    }

    /// Solid between two rings with the same vertex count (a tapered or
    /// leaning tower when `top` is a scaled, shifted copy of `bottom`).
    /// Side quads are oriented from the ring's winding, not from the
    /// centroid, so a concave footprint (an L, a notched lot) gets outward
    /// sides on every edge; the caps face along and against the axis.
    pub fn frustum(&mut self, bottom: &[Vec3], top: &[Vec3], color: [f32; 4]) {
        let n = bottom.len().min(top.len());
        if n < 3 {
            return;
        }
        let top_c = centroid(top);
        let bot_c = centroid(bottom);
        let axis = {
            let a = top_c - bot_c;
            if a.length_squared() > 1e-8 { a.normalize() } else { Vec3::Y }
        };
        // Which way round the ring goes, seen along the axis. A ring whose
        // Newell normal points along the axis has its quads [b_i, b_j, t_j,
        // t_i] facing out; the other winding needs them reversed.
        let wind = (newell(bottom) + newell(top)).dot(axis);
        let whole = (bottom.iter().chain(top.iter()).fold(Vec3::ZERO, |a, p| a + *p)) / (2 * n) as f32;
        for i in 0..n {
            let j = (i + 1) % n;
            if wind.abs() < 1e-6 {
                // Degenerate ring (a line): fall back to the centroid rule.
                let mid = (bottom[i] + bottom[j] + top[j] + top[i]) * 0.25;
                self.quad_facing([bottom[i], bottom[j], top[j], top[i]], mid - whole, color);
            } else if wind > 0.0 {
                self.quad([bottom[i], bottom[j], top[j], top[i]], color);
            } else {
                self.quad([bottom[i], top[i], top[j], bottom[j]], color);
            }
        }
        self.cap(top, axis, color);
        self.cap(bottom, -axis, color);
    }

    /// Smooth tube through a chain of rings: a wire, a hose, a bent pipe.
    /// Ring vertices are shared by the quads on either side (normals point
    /// out from the ring's centre), and only the two ends are capped, so a
    /// six-segment wire is 52 triangles rather than six capped beams.
    pub fn strip(&mut self, rings: &[Vec<Vec3>], color: [f32; 4]) {
        let k = rings.len();
        let n = rings.iter().map(|r| r.len()).min().unwrap_or(0);
        if k < 2 || n < 3 {
            return;
        }
        let centres: Vec<Vec3> = rings.iter().map(|r| centroid(&r[..n])).collect();
        let axis0 = (centres[1] - centres[0]).normalize_or_zero();
        let wind = newell(&rings[0][..n]).dot(axis0);
        let base = self.positions.len() as u32;
        let mut along = 0.0;
        for (ri, ring) in rings.iter().enumerate() {
            if ri > 0 {
                along += centres[ri].distance(centres[ri - 1]);
            }
            let mut round = 0.0;
            for i in 0..n {
                let radial = (ring[i] - centres[ri]).normalize_or_zero();
                let tangent = (ring[(i + 1) % n] - ring[i]).normalize_or_zero();
                self.positions.push(ring[i].to_array());
                self.normals.push(radial.to_array());
                self.colors.push(color);
                self.uvs.push([along, round]);
                self.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
                round += ring[i].distance(ring[(i + 1) % n]);
            }
        }
        for r in 0..k - 1 {
            let a = base + (r * n) as u32;
            let b = base + ((r + 1) * n) as u32;
            for i in 0..n as u32 {
                let j = (i + 1) % n as u32;
                if wind >= 0.0 {
                    self.indices.extend_from_slice(&[a + i, a + j, b + j, a + i, b + j, b + i]);
                } else {
                    self.indices.extend_from_slice(&[a + i, b + i, b + j, a + i, b + j, a + j]);
                }
            }
        }
        self.cap(&rings[0][..n], -axis0, color);
        let axis1 = (centres[k - 1] - centres[k - 2]).normalize_or_zero();
        self.cap(&rings[k - 1][..n], axis1, color);
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
        let (p1, p2) = perpendiculars(dir);
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

    /// Thin square-section bar between two points in space (braces, rails,
    /// stair stringers).
    pub fn beam(&mut self, a: Vec3, b: Vec3, thickness: f32, color: [f32; 4]) {
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            return;
        }
        let dir = d / len;
        let (p1, p2) = perpendiculars(dir);
        let (p1, p2) = (p1 * thickness * 0.5, p2 * thickness * 0.5);
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

    /// A wire that sags under its own weight between two anchors: one
    /// square-section ring strip through seven points.
    pub fn wire(&mut self, a: Vec3, b: Vec3, sag: f32, thickness: f32, color: [f32; 4]) {
        let segs = 6;
        if a.distance(b) < 1e-3 {
            return;
        }
        let pts: Vec<Vec3> = (0..=segs)
            .map(|i| {
                let t = i as f32 / segs as f32;
                a.lerp(b, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t))
            })
            .collect();
        // One pair of perpendiculars for the whole wire so the square section
        // does not twist between rings.
        let (p1, p2) = perpendiculars((b - a).normalize());
        let (p1, p2) = (p1 * thickness * 0.5, p2 * thickness * 0.5);
        let rings: Vec<Vec<Vec3>> = pts.iter().map(|p| vec![*p + p1 + p2, *p - p1 + p2, *p - p1 - p2, *p + p1 - p2]).collect();
        self.strip(&rings, color);
    }
}

/// Two unit vectors perpendicular to `dir` and to each other.
fn perpendiculars(dir: Vec3) -> (Vec3, Vec3) {
    let helper = if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let p1 = dir.cross(helper).normalize();
    let p2 = dir.cross(p1).normalize();
    (p1, p2)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Triangles of a closed convex solid whose geometric normal points at
    /// the solid's centroid.
    fn inward_facing(m: &MeshData) -> usize {
        let c = m.positions.iter().fold(Vec3::ZERO, |a, p| a + Vec3::from(*p)) / m.positions.len() as f32;
        m.indices
            .chunks(3)
            .filter(|t| {
                let a = Vec3::from(m.positions[t[0] as usize]);
                let b = Vec3::from(m.positions[t[1] as usize]);
                let cc = Vec3::from(m.positions[t[2] as usize]);
                let n = (b - a).cross(cc - a);
                n.length() > 1e-7 && n.dot((a + b + cc) / 3.0 - c) < 0.0
            })
            .count()
    }

    fn l_shape() -> Vec<Vec2> {
        vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 4.0), Vec2::new(4.0, 4.0), Vec2::new(4.0, 10.0), Vec2::new(0.0, 10.0)]
    }

    #[test]
    fn convex_solids_face_outward_whichever_way_they_lie() {
        let w = [1.0; 4];
        let cases: Vec<(&str, MeshData)> = vec![
            ("descending beam", { let mut m = MeshData::default(); m.beam(Vec3::new(0.0, 5.0, 0.0), Vec3::new(10.0, 2.0, 0.0), 0.5, w); m }),
            ("ascending beam", { let mut m = MeshData::default(); m.beam(Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 5.0, 0.0), 0.5, w); m }),
            ("horizontal pipe", { let mut m = MeshData::default(); m.pipe(Vec2::ZERO, Vec2::X, 10.0, 0.5, 2.0, 8, w); m }),
            ("ascending slab", { let mut m = MeshData::default(); m.slab(Vec3::new(0.0, 0.2, 0.0), Vec3::new(10.0, 3.2, 0.0), 5.0, 0.4, w); m }),
            ("descending slab", { let mut m = MeshData::default(); m.slab(Vec3::new(0.0, 3.2, 0.0), Vec3::new(10.0, 0.2, 0.0), 5.0, 0.4, w); m }),
            ("cylinder", { let mut m = MeshData::default(); m.cylinder(Vec2::ZERO, 1.0, 1.0, 0.0, 3.0, 12, w); m }),
            ("cone", { let mut m = MeshData::default(); m.cylinder(Vec2::ZERO, 1.0, 0.05, 0.0, 3.0, 12, w); m }),
            ("box", { let mut m = MeshData::default(); m.boxed(Vec3::ZERO, Vec3::ONE, 0.3, w); m }),
            ("dish", { let mut m = MeshData::default(); m.frustum(&MeshData::ngon(Vec2::ZERO, 0.14, 10, 0.0, 1.0), &MeshData::ngon(Vec2::new(0.0, 0.3), 0.7, 10, 0.0, 1.4), w); m }),
            ("clockwise ring", { let mut m = MeshData::default(); let mut b = MeshData::ngon(Vec2::ZERO, 2.0, 6, 0.0, 0.0); b.reverse(); let t: Vec<Vec3> = b.iter().map(|p| *p + Vec3::Y * 3.0).collect(); m.frustum(&b, &t, w); m }),
        ];
        for (label, m) in &cases {
            assert!(m.indices.len() % 3 == 0 && !m.is_empty(), "{label} is empty");
            assert_eq!(inward_facing(m), 0, "{label} has inward-facing triangles");
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{label} has NaN");
            assert_eq!(m.positions.len(), m.uvs.len());
            assert_eq!(m.positions.len(), m.tangents.len());
        }
        // Shared cap rings: a 12-gon cylinder is 12 side quads (48 verts) plus two 12-vertex caps.
        let mut m = MeshData::default();
        m.cylinder(Vec2::ZERO, 1.0, 1.0, 0.0, 3.0, 12, w);
        assert_eq!(m.positions.len(), 48 + 24);
        assert_eq!(m.indices.len() / 3, 24 + 20);
    }

    #[test]
    fn concave_footprints_are_clipped_not_fanned() {
        let w = [1.0; 4];
        let l = l_shape();
        let mut m = MeshData::default();
        m.ground_polygon(&l, 0.0, w);
        assert_eq!(m.indices.len() / 3, 4);
        let mut area = 0.0;
        for t in m.indices.chunks(3) {
            let a = Vec3::from(m.positions[t[0] as usize]);
            let b = Vec3::from(m.positions[t[1] as usize]);
            let c = Vec3::from(m.positions[t[2] as usize]);
            let n = (b - a).cross(c - a);
            assert!(n.y > 0.0, "ground triangle faces down");
            area += n.length() * 0.5;
        }
        assert!((area - 64.0).abs() < 1e-3, "clipped area {area} should be the L's 64 m²");
        // Every side of the extruded L faces away from the solid along its own edge.
        let l3: Vec<Vec3> = l.iter().map(|p| Vec3::new(p.x, 0.0, p.y)).collect();
        let top: Vec<Vec3> = l3.iter().map(|p| *p + Vec3::Y * 5.0).collect();
        let mut m = MeshData::default();
        m.frustum(&l3, &top, w);
        for i in 0..6 {
            let e = l[(i + 1) % 6] - l[i];
            let out = Vec3::new(e.y, 0.0, -e.x);
            let n = Vec3::from(m.normals[i * 4]);
            assert!(n.dot(out) > 0.0, "side {i} of the L points into the solid");
        }
        // The caps cover the L exactly: four triangles each, all facing along the axis.
        let caps: Vec<&[u32]> = m.indices[6 * 6..].chunks(3).collect();
        assert_eq!(caps.len(), 8);
        for t in caps {
            let a = Vec3::from(m.positions[t[0] as usize]);
            let b = Vec3::from(m.positions[t[1] as usize]);
            let c = Vec3::from(m.positions[t[2] as usize]);
            let n = (b - a).cross(c - a);
            let expect = if a.y > 2.5 { 1.0 } else { -1.0 };
            assert!(n.y * expect > 0.0, "cap triangle faces into the solid");
        }
        let tris = triangulate(&l);
        assert_eq!(tris.len(), 4);
        let cw: Vec<Vec2> = l.iter().rev().copied().collect();
        assert_eq!(triangulate(&cw).len(), 4);
    }

    #[test]
    fn wire_is_one_strip_with_end_caps() {
        let mut m = MeshData::default();
        m.wire(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 0.5, 0.04, [1.0; 4]);
        // 7 rings of 4 shared vertices, 6 × 4 quads, two 4-vertex caps.
        assert_eq!(m.positions.len(), 28 + 8);
        assert_eq!(m.indices.len() / 3, 48 + 4);
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn horizontal_quads_follow_their_own_edge() {
        let mut m = MeshData::default();
        let yaw = 0.7f32;
        m.boxed(Vec3::new(5.0, 1.0, 3.0), Vec3::new(4.0, 2.0, 2.0), yaw, [1.0; 4]);
        // The top face is first: its tangent is the box's rotated x axis.
        let t = Vec3::from([m.tangents[0][0], m.tangents[0][1], m.tangents[0][2]]);
        let expect = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        assert!(t.dot(expect).abs() > 0.999, "top tangent {t} should follow the box, not the world");
        // A ground polygon keeps the world axes.
        let mut g = MeshData::default();
        g.ground_polygon(&[Vec2::new(0.0, 0.0), Vec2::new(3.0, 1.0), Vec2::new(2.0, 4.0)], 0.0, [1.0; 4]);
        assert_eq!(g.tangents[0], [1.0, 0.0, 0.0, 1.0]);
    }
}
