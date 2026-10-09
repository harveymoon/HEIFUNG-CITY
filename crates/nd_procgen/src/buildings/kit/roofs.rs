//! Pitched roofs: a ridge squashed out of a footprint, and gables on the
//! layout kit's rectangles.

use super::super::parts::*;
use super::super::Ctx;
use super::R;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::Vec2;

/// Shrink a polygon towards its centroid along one axis only, keeping
/// `keep` of its extent along it: which turns a footprint into a roof
/// ridge.
pub(crate) fn squash(poly: &[Vec2], axis: Vec2, keep: f32) -> Vec<Vec2> {
    let c = poly_center(poly);
    poly.iter()
        .map(|p| {
            let d = *p - c;
            let along = d.dot(axis);
            c + d - axis * along * keep
        })
        .collect()
}

/// A pitched roof over a footprint, its ridge across `axis`: the eaves
/// `overhang` out from the walls, the ridge the footprint squashed to
/// `keep` of its extent along `axis` (and held over the footprint).
#[allow(clippy::too_many_arguments)]
pub(crate) fn ridge_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, rise: f32, axis: Vec2, keep: f32, overhang: f32, s: Surface, color: [f32; 3]) {
    let top: Vec<Vec2> = squash(poly, axis, keep).into_iter().map(|p| clamp_into(poly, p)).collect();
    loft(g, &c.frame, s, &offset_poly(poly, -overhang), &top, y, y + rise, color);
}

/// A metal shed roof whose ridge runs across the street front.
pub(crate) fn gable_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, ridge: f32, color: [f32; 3]) {
    let axis = c.front_face(poly).out;
    ridge_roof(g, c, poly, y, ridge, axis, 1.0 - 0.04, 0.4, Metal, color);
}

/// A gabled roof on a rectangle, its ridge along `v` (for buildings deep
/// in `v`) or, with `along_u`, along `u`.
pub(crate) fn gable_on(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, along_u: bool, color: [f32; 3]) {
    let (bottom, top) = if along_u {
        (rect(r.c().x, r.c().y, r.w() * 0.5 + 0.2, r.d() * 0.5 + 0.3), rect(r.c().x, r.c().y, r.w() * 0.5 + 0.2, 0.08))
    } else {
        (rect(r.c().x, r.c().y, r.w() * 0.5 + 0.3, r.d() * 0.5 + 0.2), rect(r.c().x, r.c().y, 0.08, r.d() * 0.5 + 0.2))
    };
    loft(g, &c.frame, Metal, &bottom, &top, y, y + rise, color);
}
