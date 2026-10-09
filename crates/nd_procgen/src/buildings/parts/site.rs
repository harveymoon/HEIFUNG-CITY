//! Site furniture: trees, lamp posts, benches, bins and bollards.

#[allow(unused_imports)]
use super::*;

// ── site ─────────────────────────────────────────────────────────────────────

/// A tree on the lot for the palette's climate (round where it is dry or
/// mild, palms, conifers or birches elsewhere).
pub fn tree_in(g: &mut Geometry, f: &Frame, pal: &crate::palette::Palette, p: Vec2, h: f32, canopy: f32, leaf: [f32; 3]) {
    crate::climate::climate_tree(g, pal, f.to_world(p.x, p.y), h, canopy, leaf);
}

pub fn tree(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    tree_world(g, f.to_world(p.x, p.y), h, canopy, bark, leaf);
}

/// The one tree family. `Round` is the street and garden tree; the others
/// come from parks and open ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// Trunk with a two-tier faceted canopy.
    Round,
    /// Three stacked cones.
    Conifer,
    /// Leaning trunk with a crown of fronds.
    Palm,
    /// Bare tapering trunk with a few branches, no canopy.
    Dead,
}

/// A round tree on the ground at a world point.
pub fn tree_world(g: &mut Geometry, w: Vec2, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    tree_kind(g, w, 0.0, TreeKind::Round, h, canopy, bark, leaf);
}

/// Any tree, standing on ground at height `y` at a world point. Branch and
/// lean directions come from the position so no RNG is consumed: callers
/// that place thousands of trees keep their streams stable.
#[allow(clippy::too_many_arguments)]
pub fn tree_kind(g: &mut Geometry, w: Vec2, y: f32, kind: TreeKind, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    use std::f32::consts::TAU;
    match kind {
        TreeKind::Round => {
            g.part(Wood).cylinder(w, 0.22, 0.16, y, y + h, 7, rgba(bark));
            let c = g.part(Foliage);
            c.frustum(&MeshData::ngon(w, canopy * 0.5, 7, 0.0, y + h), &MeshData::ngon(w, canopy * 0.2, 7, 0.4, y + h + canopy * 0.8), rgba(leaf));
            c.frustum(&MeshData::ngon(w, canopy * 0.35, 7, 0.3, y + h - canopy * 0.3), &MeshData::ngon(w, canopy * 0.5, 7, 0.0, y + h), rgba(mul(leaf, 0.85)));
        }
        TreeKind::Conifer => {
            g.part(Wood).cylinder(w, 0.22, 0.12, y, y + h * 0.3, 6, rgba(bark));
            for k in 0..3 {
                let y0 = y + h * (0.25 + 0.22 * k as f32);
                let rr = canopy * (1.0 - 0.22 * k as f32);
                g.part(Foliage).frustum(&MeshData::ngon(w, rr, 8, 0.3 * k as f32, y0), &MeshData::ngon(w, rr * 0.12, 8, 0.3 * k as f32, y0 + h * 0.32), rgba(mul(leaf, 1.0 - 0.06 * k as f32)));
            }
        }
        TreeKind::Palm => {
            let lean = Vec2::new(w.y * 0.013, w.x * 0.011).normalize_or_zero() * 0.9;
            let top = Vec3::new(w.x + lean.x, y + h, w.y + lean.y);
            let mid = Vec3::new(w.x + lean.x * 0.5, y + h * 0.55, w.y + lean.y * 0.5);
            g.part(Wood).beam(Vec3::new(w.x, y, w.y), mid, 0.34, rgba(bark));
            g.part(Wood).beam(mid, top, 0.26, rgba(bark));
            for k in 0..7 {
                let a = k as f32 / 7.0 * TAU;
                let dir = Vec3::new(a.cos(), 0.35, a.sin()).normalize();
                let m = top + dir * 1.6;
                let tip = m + Vec3::new(dir.x, -0.5, dir.z).normalize() * 1.8;
                g.part(Foliage).beam(top, m, 0.5, rgba(leaf));
                g.part(Foliage).beam(m, tip, 0.32, rgba(mul(leaf, 0.9)));
            }
            g.part(Foliage).cylinder(Vec2::new(top.x, top.z), 0.5, 0.5, y + h - 0.4, y + h + 0.3, 6, rgba(mul(leaf, 0.8)));
        }
        TreeKind::Dead => {
            let wood = g.part(Wood);
            wood.cylinder(w, 0.18, 0.07, y - 0.1, y + h, 5, rgba(bark));
            let n = 2 + ((w.x * 0.37 + w.y * 0.61).abs() as i32 % 2);
            for k in 0..n {
                let a = k as f32 * 2.1 + w.x * 0.37 + w.y * 0.13;
                let len = (h * 0.3).clamp(0.8, 1.8);
                let from = Vec3::new(w.x, y + h * (0.55 + 0.12 * k as f32), w.y);
                let to = from + Vec3::new(a.cos() * len, len * 0.7, a.sin() * len);
                wood.beam(from, to, 0.08, rgba(bark));
            }
        }
    }
}

pub fn lamp_post(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, glow: [f32; 3]) {
    lamp_post_world(g, f.to_world(p.x, p.y), h, glow);
}

/// Lamp post on the lot, skipped when the point is off it or so close to
/// the edge that the lantern hangs over the street.
pub fn lamp_post_in(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32, glow: [f32; 3]) {
    if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.35 {
        lamp_post(g, &c.frame, p, h, glow);
    }
}

pub fn lamp_post_world(g: &mut Geometry, w: Vec2, h: f32, glow: [f32; 3]) {
    g.part(Matte).cylinder(w, 0.12, 0.08, 0.0, h, 8, rgba(CHARCOAL));
    g.part(Matte).boxed(Vec3::new(w.x, h + 0.15, w.y), Vec3::new(0.6, 0.3, 0.6), 0.0, rgba(CHARCOAL));
    g.part(Glow).boxed(Vec3::new(w.x, h - 0.05, w.y), Vec3::new(0.5, 0.15, 0.5), 0.0, rgba(glow));
    g.lamps.push(Vec3::new(w.x, h - 0.2, w.y));
}

pub fn bench_world(g: &mut Geometry, w: Vec2, yaw: f32, wood: [f32; 3], metal: [f32; 3]) {
    let m = g.part(Wood);
    m.boxed(Vec3::new(w.x, 0.45, w.y), Vec3::new(1.8, 0.08, 0.5), yaw, rgba(wood));
    let (s, c) = yaw.sin_cos();
    let back = Vec2::new(-s, c) * 0.22;
    m.boxed(Vec3::new(w.x + back.x, 0.8, w.y + back.y), Vec3::new(1.8, 0.45, 0.06), yaw, rgba(wood));
    let mm = g.part(Matte);
    for side in [-0.8f32, 0.8] {
        let o = Vec2::new(c, s) * side;
        mm.boxed(Vec3::new(w.x + o.x, 0.22, w.y + o.y), Vec3::new(0.08, 0.44, 0.5), yaw, rgba(metal));
    }
}

pub fn bin_world(g: &mut Geometry, w: Vec2, color: [f32; 3]) {
    g.part(Metal).cylinder(w, 0.35, 0.35, 0.0, 1.0, 10, rgba(color));
    g.part(Matte).cylinder(w, 0.37, 0.37, 1.0, 1.1, 10, rgba(DARK));
}

pub fn bollard_world(g: &mut Geometry, w: Vec2, color: [f32; 3]) {
    g.part(Matte).cylinder(w, 0.14, 0.12, 0.0, 0.9, 8, rgba(color));
}

/// Bollards `d` out from a face, pulled back to the lot edge when the
/// forecourt is shallower than asked for.
pub fn bollards(g: &mut Geometry, c: &Ctx, face: &Face, n: i32, d: f32) {
    let f = &c.frame;
    let d = d.min(c.reach_out(face) - 0.4);
    if d < -c.fd {
        return;
    }
    for i in 0..n {
        let t = -face.len * 0.4 + i as f32 * (face.len * 0.8 / (n - 1).max(1) as f32);
        face.bx(g, f, Matte, t, 0.45, d, Vec3::new(0.25, 0.9, 0.25), mul(c.palette.steel, 0.7));
    }
}
