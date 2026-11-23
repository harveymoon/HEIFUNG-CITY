//! Pitched roofs: a ridge squashed out of a footprint, and gables on the
//! layout kit's rectangles.

use super::super::parts::*;
use super::super::Ctx;
use super::R;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::Vec2;
use nd_sim::zoning::{point_in_polygon, polygon_area};

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
/// [r15-lots] (F9) An odd hall (a footprint that fills under 90% of its
/// rectangle along the front: merged plots, a lot clipped by a curve) gets
/// a flat roof behind a parapet: squashed towards its centroid, the ridge
/// of an irregular outline ran off to a corner and the roof read as one
/// skewed wedge (Meridian Fab 13 over three merged plots).
pub(crate) fn gable_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, ridge: f32, color: [f32; 3]) {
    let axis = c.front_face(poly).out;
    if !rectangular_enough(poly, axis) {
        flat_hall_roof(g, c, poly, y, color);
        return;
    }
    ridge_roof(g, c, poly, y, ridge, axis, 1.0 - 0.04, 0.4, Metal, color);
}

/// [r15-lots] (F9) Does a footprint fill at least 90% of its rectangle
/// aligned to `axis` (a ridge over it then runs straight)?
pub(crate) fn rectangular_enough(poly: &[Vec2], axis: Vec2) -> bool {
    if poly.len() < 3 || axis == Vec2::ZERO {
        return false;
    }
    let side = Vec2::new(-axis.y, axis.x);
    let span = |d: Vec2| poly.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.dot(d)), hi.max(p.dot(d))));
    let ((u0, u1), (v0, v1)) = (span(axis), span(side));
    let rect = (u1 - u0) * (v1 - v0);
    rect > 1e-3 && polygon_area(poly).abs() / rect >= 0.9
}

/// [r15-lots] (F9) A flat roof on an odd hall: a deck over the whole
/// footprint, a parapet round it and a row of roof lights.
fn flat_hall_roof(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, color: [f32; 3]) {
    extrude(g, &c.frame, Metal, poly, y, y + 0.2, mul(color, 0.8));
    parapet(g, c, poly, y + 0.2, 0.7, 0.25, Metal, color);
    let centre = poly_center(poly);
    if point_in_polygon(centre, poly) {
        let r = (inradius(poly) * 0.6).clamp(0.8, 6.0);
        extrude(g, &c.frame, Glass, &rect(centre.x, centre.y, r, r * 0.35), y + 0.2, y + 0.6, [0.55, 0.6, 0.62]);
    }
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
