//! [overlay] Shapes the overlays share: the selection gold, the rotation
//! that lays a gizmo circle flat on the ground, boxes round vehicles and
//! people, terrain-aware lot outlines, the "o" and "x" at a route's ends,
//! and the mesh pieces (bands, light walls, fills, ribbons) the mesh
//! overlays are built from.

use bevy::prelude::*;
use nd_procgen::MeshData;
use nd_sim::{BuildingId, Vec2 as SVec2, World};

/// The gold every selection is outlined in.
pub const GOLD: Color = Color::linear_rgb(3.2, 2.4, 0.7);

/// Lays a gizmo circle (drawn in its XY plane) flat on the ground: a
/// quarter turn about X, `Quat::from_rotation_x(-FRAC_PI_2)` (checked in
/// the tests below).
pub const FLAT: Quat = Quat::from_xyzw(-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2);

/// A flat circle's isometry at `at`.
pub fn flat_at(at: Vec3) -> Isometry3d {
    Isometry3d::new(at, FLAT)
}

/// A circle of radius `r` lying flat at `at`.
pub fn ring(gizmos: &mut Gizmos<impl GizmoConfigGroup>, at: Vec3, r: f32, col: Color) {
    gizmos.circle(flat_at(at), r, col);
}

/// `pts` closed into a loop: the first point again at the end.
pub fn closed(mut pts: Vec<Vec3>) -> Vec<Vec3> {
    if let Some(&first) = pts.first() {
        pts.push(first);
    }
    pts
}

/// A polygon outlined through `lift`, closed; nothing for fewer than
/// three points.
pub fn outline(gizmos: &mut Gizmos, poly: &[SVec2], lift: impl Fn(SVec2) -> Vec3, col: Color) {
    if poly.len() >= 3 {
        gizmos.linestrip(closed(poly.iter().map(|&p| lift(p)).collect()), col);
    }
}

/// The interpolated angle from `a` to `b`, the short way round.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + d * t
}

/// The corners of a box `2 * hl` long and `2 * hw` wide centred at `p`,
/// facing `heading`, at height `y`: front right, front left, back left,
/// back right.
pub fn box_corners(p: SVec2, heading: f32, hl: f32, hw: f32, y: f32) -> [Vec3; 4] {
    let fwd = SVec2::new(heading.cos(), heading.sin());
    let right = SVec2::new(-fwd.y, fwd.x);
    let corner = |a: f32, b: f32| {
        let q = p + fwd * a + right * b;
        Vec3::new(q.x, y, q.y)
    };
    [corner(hl, hw), corner(hl, -hw), corner(-hl, -hw), corner(-hl, hw)]
}

/// A rectangle round a vehicle's body, a little above the road, drawn twice
/// so it reads as a box.
pub fn vehicle_box(gizmos: &mut Gizmos, p: SVec2, heading: f32, length: f32, width: f32, y: f32, col: Color) {
    let [a, b, c, d] = box_corners(p, heading, length * 0.5, width * 0.5, y);
    let pts = [a, b, c, d, a];
    gizmos.linestrip(pts, col);
    gizmos.linestrip(pts.iter().map(|v| *v + Vec3::Y * 1.4), col);
}

/// An upright box round a person standing at `base` (ground level).
pub fn person_box(gizmos: &mut Gizmos, base: Vec3, col: Color) {
    let (r, h) = (0.7, 2.2);
    let ring = |y: f32| [Vec3::new(-r, y, -r), Vec3::new(r, y, -r), Vec3::new(r, y, r), Vec3::new(-r, y, r), Vec3::new(-r, y, -r)].map(|v| v + base);
    gizmos.linestrip(ring(0.05), col);
    gizmos.linestrip(ring(h), col);
    for (x, z) in [(-r, -r), (r, -r), (r, r), (-r, r)] {
        gizmos.line(base + Vec3::new(x, 0.05, z), base + Vec3::new(x, h, z), col);
    }
}

/// `y` metres above the built ground at each corner. [fix/review-app] It
/// was an absolute height: on relief maps outlines sank under the terrain.
pub fn lot_outline(gizmos: &mut Gizmos, w: &World, b: BuildingId, y: f32, col: Color) {
    // [land] the whole footprint of a building over merged plots
    let Some(lot) = w.footprint(b) else { return };
    let ground = |p: SVec2| if w.relief { w.ground_height(p) } else { 0.0 };
    outline(gizmos, &lot.polygon, |p| Vec3::new(p.x, y + ground(p), p.y), col);
}

/// [clarity] The ends of a selected route: a small "o" (a ring) where the
/// trip began and an "x" where it ends, standing a little above the road
/// so they read from any height.
pub fn route_ends(gizmos: &mut Gizmos, w: &World, origin: Option<SVec2>, end: Option<SVec2>) {
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y + w.surface_height(p), p.y);
    if let Some(o) = origin {
        let col = Color::linear_rgb(0.6, 3.0, 1.2);
        for y in [1.2, 1.5] {
            ring(gizmos, at(o, y), 3.0, col);
        }
        gizmos.line(at(o, 0.1), at(o, 1.2), col);
    }
    if let Some(e) = end {
        let col = Color::linear_rgb(3.2, 0.6, 2.4);
        let r = 3.2;
        for y in [1.2, 1.5] {
            gizmos.line(at(e + SVec2::new(-r, -r), y), at(e + SVec2::new(r, r), y), col);
            gizmos.line(at(e + SVec2::new(-r, r), y), at(e + SVec2::new(r, -r), y), col);
        }
        gizmos.line(at(e, 0.1), at(e, 1.2), col);
    }
}

// ── mesh pieces ─────────────────────────────────────────────────────────────

/// One vertex of an overlay mesh (no texture: uv and tangent fixed).
pub fn push_vertex(m: &mut MeshData, p: Vec3, nrm: Vec3, color: [f32; 4]) {
    m.positions.push(p.to_array());
    m.normals.push(nrm.to_array());
    m.colors.push(color);
    m.uvs.push([0.0, 0.0]);
    m.tangents.push([1.0, 0.0, 0.0, 1.0]);
}

/// A band along a closed outline, `width` wide, `lift` over the ground.
pub fn outline_band(m: &mut MeshData, w: &World, poly: &[SVec2], width: f32, lift: f32, color: [f32; 4]) {
    let n = poly.len();
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        let d = (b - a).normalize_or_zero();
        if d == SVec2::ZERO {
            continue;
        }
        let side = SVec2::new(-d.y, d.x) * width * 0.5;
        let (a2, b2) = (a - d * width * 0.5, b + d * width * 0.5);
        let (ya, yb) = (w.ground_height(a) + lift, w.ground_height(b) + lift);
        let base = m.positions.len() as u32;
        push_vertex(m, Vec3::new(a2.x + side.x, ya, a2.y + side.y), Vec3::Y, color);
        push_vertex(m, Vec3::new(b2.x + side.x, yb, b2.y + side.y), Vec3::Y, color);
        push_vertex(m, Vec3::new(b2.x - side.x, yb, b2.y - side.y), Vec3::Y, color);
        push_vertex(m, Vec3::new(a2.x - side.x, ya, a2.y - side.y), Vec3::Y, color);
        m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// A wall of light up from an outline, fading to nothing at `height`.
pub fn light_wall(m: &mut MeshData, w: &World, poly: &[SVec2], height: f32, c: [f32; 3], strength: f32) {
    const ROWS: [(f32, f32); 4] = [(0.0, 0.8), (0.15, 0.45), (0.5, 0.12), (1.0, 0.0)];
    let n = poly.len();
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        let d = (b - a).normalize_or_zero();
        if d == SVec2::ZERO {
            continue;
        }
        let nrm = Vec3::new(-d.y, 0.0, d.x);
        let (ya, yb) = (w.ground_height(a) + 0.3, w.ground_height(b) + 0.3);
        let base = m.positions.len() as u32;
        for (t, alpha) in ROWS {
            let col = [c[0], c[1], c[2], alpha * strength];
            push_vertex(m, Vec3::new(a.x, ya + t * height, a.y), nrm, col);
            push_vertex(m, Vec3::new(b.x, yb + t * height, b.y), nrm, col);
        }
        for r in 0..ROWS.len() as u32 - 1 {
            let (i0, i1, i2, i3) = (base + r * 2, base + r * 2 + 1, base + r * 2 + 3, base + r * 2 + 2);
            m.indices.extend_from_slice(&[i0, i1, i2, i0, i2, i3]);
        }
    }
}

/// A lot's polygon filled just above the ground.
pub fn fill(m: &mut MeshData, w: &World, poly: &[SVec2], lift: f32, color: [f32; 4]) {
    if poly.len() < 3 {
        return;
    }
    let y = poly.iter().map(|&p| w.ground_height(p)).fold(f32::MIN, f32::max) + lift;
    m.ground_polygon(poly, y, color);
}

/// A flat ribbon along `pts` at height `y`: one vertex per `profile`
/// entry (offset across, alpha) at every point, coloured `rgb` with that
/// alpha, so a profile can fade out to the sides.
pub fn ribbon(pts: &[SVec2], profile: &[(f32, f32)], y: f32, rgb: [f32; 3]) -> MeshData {
    let mut m = MeshData::default();
    if pts.len() < 2 {
        return m;
    }
    for (i, &p) in pts.iter().enumerate() {
        let a = pts[i.saturating_sub(1)];
        let b = pts[(i + 1).min(pts.len() - 1)];
        let d = (b - a).normalize_or_zero();
        let side = SVec2::new(-d.y, d.x);
        for &(off, alpha) in profile {
            let q = p + side * off;
            m.positions.push([q.x, y, q.y]);
            m.normals.push([0.0, 1.0, 0.0]);
            m.colors.push([rgb[0], rgb[1], rgb[2], alpha]);
            m.uvs.push([0.0, 0.0]);
            m.tangents.push([1.0, 0.0, 0.0, 1.0]);
        }
    }
    let n = profile.len() as u32;
    for i in 0..pts.len() as u32 - 1 {
        for k in 0..n - 1 {
            let (a, b) = (i * n + k, (i + 1) * n + k);
            m.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant is bit for bit the rotation every overlay used to
    /// build, so circles land on exactly the same pixels.
    #[test]
    fn flat_is_the_quarter_turn() {
        let q = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        assert_eq!(FLAT.to_array().map(f32::to_bits), q.to_array().map(f32::to_bits));
    }

    #[test]
    fn lerp_angle_goes_the_short_way() {
        let pi = std::f32::consts::PI;
        assert!((lerp_angle(pi - 0.1, -pi + 0.1, 0.5).abs() - pi).abs() < 1e-4);
        assert!((lerp_angle(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
    }
}
