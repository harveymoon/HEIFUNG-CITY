//! Site furniture: trees, lamp posts, benches, bins and bollards, placed
//! through the lot's [`Frame`]. [r13-detail] Trees, benches and bins are
//! drawn by the shared prop library (`crate::props`).

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

/// [r13-detail] The tree family lives in the shared prop library.
pub use crate::props::TreeKind;

/// A round tree on the ground at a world point.
pub fn tree_world(g: &mut Geometry, w: Vec2, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    tree_kind(g, w, 0.0, TreeKind::Round, h, canopy, bark, leaf);
}

/// Any tree, standing on ground at height `y` at a world point
/// ([`crate::props::tree`]).
#[allow(clippy::too_many_arguments)]
pub fn tree_kind(g: &mut Geometry, w: Vec2, y: f32, kind: TreeKind, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    crate::props::tree(g, w, y, kind, h, canopy, bark, leaf);
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
    crate::props::bench(g, w, yaw, wood, metal, false);
}

pub fn bin_world(g: &mut Geometry, w: Vec2, color: [f32; 3]) {
    crate::props::bin(g, w, color);
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
