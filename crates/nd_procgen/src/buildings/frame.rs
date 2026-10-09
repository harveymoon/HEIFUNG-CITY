//! The parcel frame and the faces of a footprint.

use super::*;

// ── geometry frame ───────────────────────────────────────────────────────────

/// Parcel-aligned coordinate frame: `u` runs along the frontage, `v` inward
/// from the road, origin at the centre of the parcel's bounding rectangle.
///
/// Generators work in two coordinate systems: lot-local (`Ctx::lot_poly`,
/// `Face`, `Frame::bx`) and world (`ParkLayout`, `RailLink`, the `*_world`
/// helpers). The identity frame from [`Frame::world`] lets a frame-taking
/// helper draw in world space; [`Frame::is_world`] tells which one a frame is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub center: Vec2,
    pub along: Vec2,
    pub inward: Vec2,
    pub width: f32,
    pub depth: f32,
}

impl Frame {
    pub fn from_lot(lot: &Lot) -> Self {
        let along = (lot.corners[1] - lot.corners[0]).normalize_or_zero();
        let inward = Vec2::new(-along.y, along.x);
        let center = (lot.corners[0] + lot.corners[1] + lot.corners[2] + lot.corners[3]) * 0.25;
        Self { center, along, inward, width: lot.width(), depth: lot.depth() }
    }

    /// The parcel's outline in this frame exactly as the styles see it
    /// (`Ctx::lot_poly`): cleaned at 0.3 m, the frame's own rectangle when
    /// too little of it survives, counter-clockwise. Everything drawn
    /// outside the mesh that must line up with it (smoke vents, solar
    /// trackers, turbines, parking bays) reads the outline from here.
    pub fn local_poly(&self, lot: &Lot) -> Vec<Vec2> {
        let mut poly = parts::clean_poly(&lot.polygon.iter().map(|p| self.to_local(*p)).collect::<Vec<_>>(), 0.3);
        if poly.len() < 3 {
            let (hw, hd) = (self.width * 0.5, self.depth * 0.5);
            poly = vec![Vec2::new(-hw, -hd), Vec2::new(hw, -hd), Vec2::new(hw, hd), Vec2::new(-hw, hd)];
        }
        if polygon_area(&poly) < 0.0 {
            poly.reverse();
        }
        poly
    }

    /// A lot's frame and its outline in it ([`Frame::local_poly`]).
    pub fn lot_local(lot: &Lot) -> (Frame, Vec<Vec2>) {
        let frame = Frame::from_lot(lot);
        let poly = frame.local_poly(lot);
        (frame, poly)
    }

    /// The identity frame: lot-local and world coordinates coincide, so the
    /// frame-taking helpers draw straight into world space (parks, rail and
    /// vacant lots lay out from world polygons).
    pub fn world() -> Self {
        Self { center: Vec2::ZERO, along: Vec2::X, inward: Vec2::Y, width: 0.0, depth: 0.0 }
    }

    pub fn is_world(&self) -> bool {
        self.center == Vec2::ZERO && self.along == Vec2::X && self.inward == Vec2::Y
    }

    pub fn yaw(&self) -> f32 {
        self.along.y.atan2(self.along.x)
    }

    /// Lot-local direction to world, without translation.
    pub fn dir3(&self, d: Vec2) -> Vec3 {
        let w = self.along * d.x + self.inward * d.y;
        Vec3::new(w.x, 0.0, w.y)
    }

    pub fn to_world(&self, u: f32, v: f32) -> Vec2 {
        self.center + self.along * u + self.inward * v
    }

    pub fn to_local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        Vec2::new(d.dot(self.along), d.dot(self.inward))
    }

    pub fn at(&self, u: f32, y: f32, v: f32) -> Vec3 {
        let p = self.to_world(u, v);
        Vec3::new(p.x, y, p.y)
    }

    pub fn at2(&self, p: Vec2, y: f32) -> Vec3 {
        self.at(p.x, y, p.y)
    }

    /// Box in parcel coordinates: centre (u, y, v) and size (along, up, inward).
    pub fn bx(&self, g: &mut Geometry, s: Surface, u: f32, y: f32, v: f32, size: Vec3, color: [f32; 3]) {
        let p = self.to_world(u, v);
        g.part(s).boxed(Vec3::new(p.x, y, p.y), size, self.yaw(), parts::rgba(color));
    }

    /// Box rotated by `extra` radians relative to the frame.
    pub fn bx_yaw(&self, g: &mut Geometry, s: Surface, u: f32, y: f32, v: f32, size: Vec3, color: [f32; 3], extra: f32) {
        let p = self.to_world(u, v);
        g.part(s).boxed(Vec3::new(p.x, y, p.y), size, self.yaw() + extra, parts::rgba(color));
    }

    /// Flat painted mark lying on the ground at height `y`: `w` along the
    /// frame (plus `extra` yaw) and `d` across. One upward quad.
    #[allow(clippy::too_many_arguments)]
    pub fn flat(&self, g: &mut Geometry, s: Surface, u: f32, y: f32, v: f32, w: f32, d: f32, extra: f32, color: [f32; 3]) {
        let yaw = self.yaw() + extra;
        let along = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        let across = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
        g.part(s).decal(self.at(u, y, v), along, across, w, d, Vec3::Y, parts::rgba(color));
    }

    /// Lot-local polygon to world points at height `y`.
    pub fn poly3(&self, poly: &[Vec2], y: f32) -> Vec<Vec3> {
        poly.iter().map(|p| self.at2(*p, y)).collect()
    }
}

/// One edge of a footprint, with helpers to place things on or in front of it.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub a: Vec2,
    pub b: Vec2,
    pub tangent: Vec2,
    /// Outward normal (lot-local).
    pub out: Vec2,
    pub len: f32,
    /// Faces the road.
    pub is_front: bool,
}

impl Face {
    /// The edge a→b with its outside on the right of travel, as the edges
    /// of a counter-clockwise footprint have.
    pub fn from_edge(a: Vec2, b: Vec2) -> Self {
        let len = a.distance(b);
        let tangent = if len > 1e-4 { (b - a) / len } else { Vec2::X };
        let out = Vec2::new(tangent.y, -tangent.x);
        Self { a, b, tangent, out, len, is_front: out.dot(Vec2::new(0.0, -1.0)) > 0.6 }
    }

    /// The edge a→b facing the side `out` points to. The endpoints are
    /// swapped when needed so `tangent`, `out`, `a` and `b` always agree,
    /// which a struct literal override of one field cannot promise.
    pub fn new(a: Vec2, b: Vec2, out: Vec2) -> Self {
        let f = Self::from_edge(a, b);
        if f.out.dot(out) < 0.0 { f.reversed() } else { f }
    }

    /// The same edge seen from the other side.
    pub fn reversed(&self) -> Self {
        Self::from_edge(self.b, self.a)
    }

    pub fn mid(&self) -> Vec2 {
        (self.a + self.b) * 0.5
    }

    /// Point `t` along the face (0 at the middle) and `d` outward.
    pub fn at(&self, t: f32, d: f32) -> Vec2 {
        self.mid() + self.tangent * t + self.out * d
    }

    pub fn yaw(&self) -> f32 {
        self.tangent.y.atan2(self.tangent.x)
    }

    /// Box centred `t` along the face, `d` outward, at height `y`; `size.x`
    /// runs along the face and `size.z` through it.
    #[allow(clippy::too_many_arguments)]
    pub fn bx(&self, g: &mut Geometry, f: &Frame, s: Surface, t: f32, y: f32, d: f32, size: Vec3, color: [f32; 3]) {
        let p = self.at(t, d);
        f.bx_yaw(g, s, p.x, y, p.y, size, color, self.yaw());
    }

    /// One quad `w` wide and `h` high, `d` proud of the face and facing the
    /// way the face does: panes, frames, signs, stains, anything flat that
    /// is only ever seen from the front. See [`parts::decal`].
    #[allow(clippy::too_many_arguments)]
    pub fn decal(&self, g: &mut Geometry, f: &Frame, s: Surface, t: f32, y: f32, d: f32, w: f32, h: f32, color: [f32; 3]) {
        parts::decal(g, f, s, self, t, y, d, w, h, color);
    }
}
