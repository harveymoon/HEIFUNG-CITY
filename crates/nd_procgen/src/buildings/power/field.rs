//! Plant fields: touching solar or wind plants drawn as one.

use super::*;

// ── plant fields: touching solar or wind plants drawn as one ──────────────

/// Share of an edge another member's edges must cover for it to count as an
/// inner seam of the field (no fence, no margin) rather than its outside.
pub(super) const SEAM_COVER: f32 = 0.6;
/// Panels and turbines keep this far from every member's road front: each
/// parcel keeps its gate, cabin and sign there.
pub(super) const FIELD_FRONT_CLEAR: f32 = 4.5;

/// Touching solar arrays or wind farms laid out as one installation
/// (`nd_sim::World::plant_cluster`). Every parcel of the field is still
/// generated on its own, into its own chunk, but all of them lay their
/// panel rows or turbine grid out in the *anchor* parcel's frame and test
/// against the whole field's outline, so rows run on across the seams
/// and nothing is drawn twice: a panel or turbine belongs to the parcel
/// its centre stands on. Seams get no fence and no margin.
#[derive(Clone, Debug)]
pub struct PlantField {
    frame: crate::buildings::Frame,
    seed: u64,
    /// Every parcel of the field (world outline, road-front edge).
    members: Vec<(Vec<Vec2>, [Vec2; 2])>,
    /// The one being drawn.
    own: usize,
    /// Edges on the field's outside, world coordinates.
    open: Vec<(Vec2, Vec2)>,
}

impl PlantField {
    /// `lots` are the field's parcels with the anchor first (the order
    /// `plant_cluster` returns), `own` the index of the one being drawn and
    /// `anchor_seed` the anchor plant's seed. `None` for a field of one: a
    /// lone plant keeps its own layout.
    pub fn new(lots: &[&nd_sim::Lot], own: usize, anchor_seed: u64) -> Option<Self> {
        if lots.len() < 2 || own >= lots.len() {
            return None;
        }
        let members: Vec<(Vec<Vec2>, [Vec2; 2])> = lots.iter().map(|l| (l.polygon.clone(), l.front_edge)).collect();
        let mut open = Vec::new();
        for (i, (poly, _)) in members.iter().enumerate() {
            let n = poly.len();
            for k in 0..n {
                let (a, b) = (poly[k], poly[(k + 1) % n]);
                if !Self::seam_of(&members, i, a, b) {
                    open.push((a, b));
                }
            }
        }
        Some(Self { frame: crate::buildings::Frame::from_lot(lots[0]), seed: anchor_seed, members, own, open })
    }

    /// Is the world edge `a`–`b` of member `i` mostly covered by edges of
    /// the other members?
    fn seam_of(members: &[(Vec<Vec2>, [Vec2; 2])], i: usize, a: Vec2, b: Vec2) -> bool {
        let len = a.distance(b);
        if len < 1e-3 {
            return true;
        }
        let mut cover = 0.0;
        for (j, (poly, _)) in members.iter().enumerate() {
            if j == i {
                continue;
            }
            let n = poly.len();
            for k in 0..n {
                if let Some((lo, hi)) = nd_sim::geom::shared_stretch(a, b, poly[k], poly[(k + 1) % n]) {
                    cover += hi - lo;
                }
            }
        }
        cover >= len * SEAM_COVER
    }

    /// Whether the world edge `a`–`b` of the parcel being drawn is a seam.
    pub fn is_seam(&self, a: Vec2, b: Vec2) -> bool {
        Self::seam_of(&self.members, self.own, a, b)
    }

    fn inside(&self, p: Vec2) -> bool {
        self.members.iter().any(|(poly, _)| nd_sim::zoning::point_in_polygon(p, poly))
    }

    fn owns(&self, p: Vec2) -> bool {
        nd_sim::zoning::point_in_polygon(p, &self.members[self.own].0)
    }

    /// Distance to the field's outside edge.
    fn clearance(&self, p: Vec2) -> f32 {
        self.open.iter().map(|(a, b)| p.distance(nd_sim::roads::project_on_segment(p, *a, *b).1)).fold(f32::MAX, f32::min)
    }

    /// Distance to the nearest member's road front.
    fn front_clearance(&self, p: Vec2) -> f32 {
        self.members.iter().map(|(_, [a, b])| p.distance(nd_sim::roads::project_on_segment(p, *a, *b).1)).fold(f32::MAX, f32::min)
    }

    /// Room for something at world point `p`: on the field, `edge` metres
    /// from its outside and clear of the road fronts.
    fn room(&self, p: Vec2, edge: f32) -> bool {
        self.inside(p) && self.clearance(p) >= edge && self.front_clearance(p) >= FIELD_FRONT_CLEAR
    }

    /// Range of grid indices whose points can fall on the parcel being drawn,
    /// for a grid of `step` (x, y) in the anchor frame, with `pad` metres of
    /// slack round the parcel.
    fn grid_range(&self, step: Vec2, pad: f32) -> (std::ops::RangeInclusive<i32>, std::ops::RangeInclusive<i32>) {
        let local: Vec<Vec2> = self.members[self.own].0.iter().map(|p| self.frame.to_local(*p)).collect();
        let (lo, hi) = bounds(&local);
        let r = |lo: f32, hi: f32, s: f32| ((lo - pad) / s).floor() as i32..=((hi + pad) / s).ceil() as i32;
        (r(lo.x, hi.x, step.x), r(lo.y, hi.y, step.y))
    }

    /// Seams of the parcel being drawn, per edge of its lot-local outline.
    fn seams(&self, c: &Ctx) -> Vec<bool> {
        let n = c.lot_poly.len();
        let w = |p: Vec2| c.frame.to_world(p.x, p.y);
        (0..n).map(|i| self.is_seam(w(c.lot_poly[i]), w(c.lot_poly[(i + 1) % n]))).collect()
    }

    /// Solar tracker sites of the parcel being drawn (world positions): one
    /// triangular grid over the whole field, so the honeycomb runs on across
    /// the seams; a tracker belongs to the parcel its pedestal stands on.
    pub(super) fn solar_sites(&self) -> Vec<Vec2> {
        let need = SOLAR_PANEL_R + SOLAR_EDGE + 0.5;
        let row = SOLAR_PITCH * 0.866;
        let (ks, rs) = self.grid_range(Vec2::new(SOLAR_PITCH, row), SOLAR_PITCH);
        let mut out = Vec::new();
        for r in rs {
            for k in ks.clone() {
                let shift = if r.rem_euclid(2) == 1 { SOLAR_PITCH * 0.5 } else { 0.0 };
                let p = self.frame.to_world(k as f32 * SOLAR_PITCH + shift, r as f32 * row);
                if self.owns(p) && self.inside(p) && self.clearance(p) >= need && self.front_clearance(p) >= SOLAR_FRONT_CLEAR + SOLAR_PANEL_R {
                    out.push(p);
                }
            }
        }
        out.truncate(SOLAR_MAX);
        out
    }

    /// Turbine sites of the parcel being drawn (world position, height
    /// jitter, rotor phase): one staggered grid over the whole field, so
    /// spacing holds across the seams and the farm spreads over all of it.
    pub(super) fn wind_sites(&self) -> Vec<(Vec2, f32, f32)> {
        let need = TURBINE_RADIUS + TURBINE_CLEARANCE;
        let row = TURBINE_SPACING * 0.866;
        let (ks, rs) = self.grid_range(Vec2::new(TURBINE_SPACING, row), TURBINE_SPACING);
        let mut out = Vec::new();
        for r in rs {
            for k in ks.clone() {
                // Jitter keyed to the grid cell, so every parcel agrees on it.
                let mut rng = nd_sim::rng_for(self.seed ^ (((r as u32 as u64) << 32) | k as u32 as u64), 73);
                let shift = if r.rem_euclid(2) == 1 { TURBINE_SPACING * 0.5 } else { 0.0 };
                let local = Vec2::new(k as f32 * TURBINE_SPACING + shift + rng.random_range(-0.3..0.3), r as f32 * row + rng.random_range(-0.3..0.3));
                let dh = rng.random_range(-0.3f32..0.3);
                let phase = rng.random_range(0.0..std::f32::consts::TAU);
                let p = self.frame.to_world(local.x, local.y);
                if self.owns(p) && self.room(p, need) && self.front_clearance(p) >= need + 1.0 {
                    out.push((p, dh, phase));
                }
            }
        }
        out.truncate(48);
        out
    }
}

/// Yard and fence of a field parcel: the ground runs flush over the seams,
/// the fence only along the field's outside, with the gate on the front.
pub(super) fn field_yard(g: &mut Geometry, c: &Ctx, field: &PlantField, s: crate::surface::Surface, color: [f32; 3], fence_h: f32) -> Vec<Vec2> {
    let seams = field.seams(c);
    let insets: Vec<f32> = seams.iter().map(|&seam| if seam { 0.0 } else { 0.5 }).collect();
    let yard = offset_edges(&c.lot_poly, &insets);
    ground(g, &c.frame, s, &yard, 0.03, color);
    let front = c.front_face_index(&yard);
    let n = yard.len();
    for i in 0..n {
        if seams[i] {
            continue;
        }
        let (a, b) = (yard[i], yard[(i + 1) % n]);
        if front == Some(i) {
            gate(g, c, a, b, fence_h);
        } else {
            fence(g, c, a, b, fence_h, false);
        }
    }
    yard
}

/// A solar parcel that is part of a field: one tracker grid laid out in
/// the anchor's frame across the whole field, filling it to
/// [`SOLAR_EDGE`] of its outside fence; each parcel keeps its inverter and
/// sign by its gate.
pub(super) fn solar_field(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, field: &PlantField) {
    let pal = c.palette;
    let yard = field_yard(g, c, field, Dirt, mul(pal.gravel, 0.55), 2.0);
    let yaw = field.frame.yaw();
    for p in field.solar_sites() {
        solar_tracker(g, p, yaw, c.animated_rotors);
    }
    let front = c.front_face(&yard);
    solar_inverter(g, c, &front, 2.4);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}
