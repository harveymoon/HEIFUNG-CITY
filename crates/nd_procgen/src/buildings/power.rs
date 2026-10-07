//! Power plants: a coal station with stacks and a coal yard, a field of
//! hexagonal solar panels on sun-tracking pedestals, a farm of helical wind turbines, and a nuclear station with its
//! containment dome and cooling towers.

use super::parts::*;
use super::Ctx;
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

/// Height of the coal plant's boiler hall, which the stacks stand on.
const COAL_HALL_H: f32 = 22.0;
/// Height of a coal stack above the hall.
const COAL_STACK_H: f32 = 42.0;

/// Where a coal plant's stacks stand, in lot coordinates: two along the
/// back, pulled in towards the boiler hall until they have 5 m from the lot
/// edge, dropped if they never do.
fn stack_sites(lot_poly: &[Vec2]) -> Vec<Vec2> {
    let (lo, hi) = bounds(lot_poly);
    let hc = Vec2::new((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.68);
    let ok = |p: Vec2| nd_sim::zoning::point_in_polygon(p, lot_poly) && distance_to_boundary(p, lot_poly) >= 5.0;
    let mut out = Vec::new();
    for k in 0..2 {
        let mut p = Vec2::new(lo.x + (hi.x - lo.x) * (0.3 + 0.4 * k as f32), hi.y - 6.0);
        for _ in 0..6 {
            if ok(p) {
                break;
            }
            p = p.lerp(hc, 0.3);
        }
        if ok(p) {
            out.push(p);
        }
    }
    out
}

/// The tops of a coal plant's stacks on `lot`, in world coordinates with
/// the height above the lot pad: where the renderer starts the plumes.
pub fn coal_stacks(lot: &nd_sim::Lot) -> Vec<Vec3> {
    let (frame, poly) = local_lot(lot);
    stack_sites(&poly).into_iter().map(|p| {
        let w = frame.to_world(p.x, p.y);
        Vec3::new(w.x, COAL_HALL_H + COAL_STACK_H + 1.0, w.y)
    }).collect()
}

pub fn coal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.3));
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    // Boiler hall at the back, turbine hall in front of it.
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.68, (hi.x - lo.x) * 0.42, (hi.y - lo.y) * 0.26));
    let h = COAL_HALL_H;
    let panel = mul(pick(rng, &pal.panel), 0.75);
    extrude(g, f, Panel, &hall, 0.0, h, panel);
    cornice(g, c, &hall, h - 0.5, 1.0, 0.3, Panel, mul(panel, 0.8));
    for face in c.faces(&hall) {
        if face.len > 8.0 {
            band_windows(g, c, rng, &face, h * 0.6, 1, 4.0);
            grime(g, c, rng, &face, 0.0, h, 3.0);
        }
    }
    let turbine = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.3, (hi.x - lo.x) * 0.38, (hi.y - lo.y) * 0.16));
    extrude(g, f, Metal, &turbine, 0.0, 11.0, mul(pick(rng, &pal.metal_paint), 0.9));
    weeds(g, c, rng, &yard, 4);
    // Two tall stacks with a glowing top. The smoke is drawn by the
    // renderer as drifting translucent puffs (see `coal_stacks`).
    for p in stack_sites(&c.lot_poly) {
        chimney(g, f, p, 2.2, COAL_HALL_H, COAL_STACK_H, mul(pal.concrete(0), 0.7), pal.aviation_light);
    }
    // Coal pile, conveyor up to the boiler hall, rail-side hopper.
    let pile = Vec2::new(lo.x + (hi.x - lo.x) * 0.2, lo.y + (hi.y - lo.y) * 0.45);
    let pile_r = if c.inside_local(pile) { (distance_to_boundary(pile, &c.lot_poly) - 0.5).min(7.0) } else { 0.0 };
    if pile_r >= 3.0 {
        let w = f.to_world(pile.x, pile.y);
        g.part(Matte).frustum(&MeshData::ngon(w, pile_r, 10, 0.0, 0.0), &MeshData::ngon(w, pile_r * 0.3, 10, 0.3, pile_r * 0.7), rgba([0.07, 0.07, 0.07]));
        let a = f.at(pile.x, 5.0, pile.y);
        let hc = poly_center(&hall);
        let b = f.at(hc.x, h + 1.0, hc.y);
        g.part(Metal).beam(a, b, 1.6, rgba(mul(pal.steel, 0.6)));
        for t in [0.3f32, 0.6] {
            let p = a.lerp(b, t);
            g.part(Matte).boxed(Vec3::new(p.x, p.y * 0.5, p.z), Vec3::new(0.5, p.y, 0.5), f.yaw(), rgba(mul(pal.steel, 0.5)));
        }
    }
    // Transformer yard: fenced, insulators, bus bars.
    let ty = Vec2::new(hi.x - 7.0, lo.y + 6.0);
    if poly_inside(c, &rect(ty.x, ty.y, 5.3, 4.3)) {
        let pen = rect(ty.x, ty.y, 5.0, 4.0);
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.0, None, true);
        for k in 0..3 {
            let p = ty + Vec2::new((k as f32 - 1.0) * 3.0, 0.0);
            f.bx(g, Metal, p.x, 1.2, p.y, Vec3::new(1.6, 2.4, 1.2), mul(pal.metal_paint(1), 0.9));
            f.bx(g, Matte, p.x, 3.3, p.y, Vec3::new(0.15, 1.8, 0.15), [0.6, 0.6, 0.55]);
        }
        f.bx(g, Matte, ty.x, 4.3, ty.y, Vec3::new(8.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.4, true);
    floodlight(g, c, front.at(-front.len * 0.4, -1.5), 10.0);
    floodlight(g, c, front.at(front.len * 0.4, -1.5), 10.0);
    sign_board(g, c, rng, &front, 0.0, 3.0, (front.len * 0.4).min(10.0), 0.9);
}

// ── solar: hexagonal panels on dual-axis trackers ─────────────────────────

/// Circumradius of a tracker's hexagonal panel.
pub const SOLAR_PANEL_R: f32 = 2.2;
/// Spacing of the trackers on their triangular grid. With the panel's flat
/// sides facing its neighbours, flat (stowed) panels tile the field as a
/// honeycomb with a 0.4 m seam; tilted ones never touch, since every panel
/// turns the same way about its own pivot.
pub const SOLAR_PITCH: f32 = 4.2;
/// Height of the pivot the panel turns about: a panel tilted as far as it
/// goes still clears the ground by half a metre.
pub const SOLAR_HUB: f32 = 2.6;
/// Furthest a tracker tilts from flat, radians: the sun lower than this is
/// followed only in azimuth.
pub const SOLAR_MAX_TILT: f32 = 1.13;
/// Free ground between a panel's tip and the yard edge.
const SOLAR_EDGE: f32 = 1.0;
/// Strip along the road kept for the gate, the inverter and the sign.
const SOLAR_FRONT_CLEAR: f32 = 6.5;
/// Most trackers one parcel draws (the renderer animates each).
const SOLAR_MAX: usize = 600;

const PANEL_BLUE: [f32; 3] = [0.006, 0.016, 0.075];
const PANEL_CELL: [f32; 3] = [0.01, 0.026, 0.11];
const PANEL_BACK: [f32; 3] = [0.05, 0.055, 0.065];
const PANEL_RIM: [f32; 3] = [0.08, 0.55, 0.95];

/// One dual-axis tracker of a solar farm: where its pedestal stands (world
/// x/z) and the height of its pivot above the lot pad. The renderer turns
/// the panel about the pivot to face the sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolarTracker {
    pub pos: Vec2,
    pub hub: f32,
}

/// Tracker sites in lot coordinates: a triangular grid aligned with the
/// parcel, every panel tip [`SOLAR_EDGE`] inside the lot and the front
/// strip left free.
fn solar_sites(poly: &[Vec2]) -> Vec<Vec2> {
    let need = SOLAR_PANEL_R + SOLAR_EDGE + 0.5;
    let (lo, hi) = bounds(poly);
    let ok = |p: Vec2| nd_sim::zoning::point_in_polygon(p, poly) && distance_to_boundary(p, poly) >= need && p.y >= lo.y + SOLAR_FRONT_CLEAR + SOLAR_PANEL_R;
    let row = SOLAR_PITCH * 0.866;
    let rows = ((hi.y - lo.y) / row).ceil() as i32 + 1;
    let cols = ((hi.x - lo.x) / SOLAR_PITCH).ceil() as i32 + 1;
    let mut out = Vec::new();
    for r in 0..rows {
        let shift = if r % 2 == 1 { SOLAR_PITCH * 0.5 } else { 0.0 };
        for k in -1..=cols {
            let p = Vec2::new(lo.x + need + k as f32 * SOLAR_PITCH + shift, lo.y + SOLAR_FRONT_CLEAR + SOLAR_PANEL_R + r as f32 * row);
            if ok(p) {
                out.push(p);
            }
        }
    }
    out.truncate(SOLAR_MAX);
    out
}

/// The trackers of a solar farm on `lot`, in world coordinates: the same
/// sites the generator draws pedestals on, so the renderer puts a panel on
/// every one.
pub fn solar_trackers(lot: &nd_sim::Lot, field: Option<&PlantField>) -> Vec<SolarTracker> {
    let sites: Vec<Vec2> = match field {
        Some(field) => field.solar_sites(),
        None => {
            let (frame, poly) = local_lot(lot);
            solar_sites(&poly).into_iter().map(|p| frame.to_world(p.x, p.y)).collect()
        }
    };
    sites.into_iter().map(|pos| SolarTracker { pos, hub: SOLAR_HUB }).collect()
}

/// The hexagon's corners about `c` at height `y`, flat sides towards the
/// grid's neighbours along x (corners at 30 + 60k degrees).
fn hex(c: Vec2, r: f32, y: f32) -> Vec<Vec3> {
    MeshData::ngon(c, r, 6, std::f32::consts::FRAC_PI_6, y)
}

/// A tracker's panel lying flat about the origin (its pivot), for the
/// renderer to turn towards the sun: a dark blue hexagon of six
/// triangular cells with hairline seams, a thin backing tray, a cyan rim
/// light along its edge, a hub and the yoke down to the pedestal.
pub fn solar_head() -> Geometry {
    let mut g = Geometry::default();
    let r = SOLAR_PANEL_R;
    // Backing tray: a shallow hexagonal prism under the cells.
    g.part(Matte).frustum(&hex(Vec2::ZERO, r * 0.97, -0.16), &hex(Vec2::ZERO, r, -0.02), rgba(PANEL_BACK));
    // Six cells, each a triangle shrunk towards its own centre so the tray
    // shows between them as a dark seam.
    let corners = hex(Vec2::ZERO, r * 0.985, 0.0);
    for i in 0..6 {
        let (a, b) = (corners[i], corners[(i + 1) % 6]);
        let o = Vec3::ZERO;
        let m = (a + b + o) / 3.0;
        let s = |p: Vec3| m + (p - m) * 0.93 + Vec3::Y * 0.01;
        let shade = if i % 2 == 0 { PANEL_BLUE } else { PANEL_CELL };
        // Matte, not glass: the glass surface mirrors the bright sky and
        // the panels read pale; these should read deep blue.
        g.part(Matte).tri_facing(s(o), s(a), s(b), Vec3::Y, rgba(shade));
    }
    // Rim light: a thin glowing band round the edge, seen from above.
    let outer = hex(Vec2::ZERO, r, 0.015);
    let inner = hex(Vec2::ZERO, r - 0.07, 0.015);
    for i in 0..6 {
        let j = (i + 1) % 6;
        g.part(Glow).quad_facing([inner[i], outer[i], outer[j], inner[j]], Vec3::Y, rgba(mul(PANEL_RIM, 0.55)));
    }
    // Hub cap and the yoke down to the pedestal's bearing.
    g.part(Metal).frustum(&MeshData::ngon(Vec2::ZERO, 0.32, 6, 0.0, 0.0), &MeshData::ngon(Vec2::ZERO, 0.22, 6, 0.0, 0.07), rgba(GUNMETAL));
    g.part(Metal).cylinder(Vec2::ZERO, 0.12, 0.12, -0.42, -0.16, 6, rgba(GUNMETAL));
    g
}

/// A tracker's pedestal and footing, and (when the renderer is not turning
/// the panels) a flat panel on top.
fn solar_tracker(g: &mut Geometry, w: Vec2, yaw: f32, animated: bool) {
    g.part(Concrete).boxed(Vec3::new(w.x, 0.12, w.y), Vec3::new(0.7, 0.24, 0.7), yaw, rgba([0.36, 0.36, 0.35]));
    g.part(Metal).boxed(Vec3::new(w.x, (SOLAR_HUB - 0.4) * 0.5, w.y), Vec3::new(0.22, SOLAR_HUB - 0.4, 0.22), yaw, rgba(GUNMETAL));
    if !animated {
        // One quad pair: the panel stowed flat, the tray under it.
        let top = hex(w, SOLAR_PANEL_R, SOLAR_HUB);
        g.part(Matte).cap(&top, Vec3::Y, rgba(PANEL_BLUE));
        let under = hex(w, SOLAR_PANEL_R * 0.97, SOLAR_HUB - 0.12);
        g.part(Matte).cap(&under, -Vec3::Y, rgba(PANEL_BACK));
    }
}

/// The inverter pavilion by the gate: a dark low box with a cyan light
/// strip, its transformer beside it.
fn solar_inverter(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, front: &super::Face, d: f32) {
    let f = &c.frame;
    let cab = front.at(front.len * 0.3, -d);
    if !c.inside_local(cab) {
        return;
    }
    let cb = fit_inside(c, &rect(cab.x, cab.y, 2.6, 1.3));
    extrude(g, f, Panel, &cb, 0.0, 2.6, mul(GUNMETAL, 1.5));
    let (lo, hi) = bounds(&cb);
    let face = super::Face::new(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(0.0, -1.0));
    if face.len > 1.5 {
        decal(g, f, Glow, &face, 0.0, 2.2, 0.03, face.len * 0.8, 0.08, mul(PANEL_RIM, 0.7));
        decal(g, f, Matte, &face, -face.len * 0.25, 1.05, 0.02, 0.9, 2.0, MATTE_BLACK);
    }
    let tx = cab + Vec2::new(4.2, 0.0);
    if c.inside_local(tx) && distance_to_boundary(tx, &c.lot_poly) > 1.0 {
        f.bx(g, Metal, tx.x, 1.0, tx.y, Vec3::new(1.4, 2.0, 1.0), mul(c.palette.metal_paint(1), 0.9));
    }
    let _ = rng;
}

/// A solar farm: dark blue hexagonal panels on dual-axis trackers packed
/// in a honeycomb over dark gravel, an inverter pavilion at the gate and a
/// fence. With `Ctx::animated_rotors` the panels are left to the renderer,
/// which turns them to face the sun and lays them flat at night.
pub fn solar(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    if let Some(field) = c.field {
        return solar_field(g, c, rng, field);
    }
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Dirt, mul(pal.gravel, 0.55));
    for p in solar_sites(&c.lot_poly) {
        solar_tracker(g, f.to_world(p.x, p.y), f.yaw(), c.animated_rotors);
    }
    let front = c.front_face(&yard);
    solar_inverter(g, c, rng, &front, 3.0);
    fence_lot(g, c, &yard, 2.0, false);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}

// ── plant fields: touching solar or wind plants drawn as one ──────────────

/// Share of an edge another member's edges must cover for it to count as an
/// inner seam of the field (no fence, no margin) rather than its outside.
const SEAM_COVER: f32 = 0.6;
/// Panels and turbines keep this far from every member's road front: each
/// parcel keeps its gate, cabin and sign there.
const FIELD_FRONT_CLEAR: f32 = 4.5;

/// Touching solar arrays or wind farms laid out as one installation
/// (`nd_sim::World::plant_cluster`). Every parcel of the field is still
/// generated on its own, into its own chunk, but all of them lay their
/// panel rows or turbine grid out in the *anchor* parcel's frame and test
/// against the whole field's outline, so rows run on across the seams
/// and nothing is drawn twice: a panel or turbine belongs to the parcel
/// its centre stands on. Seams get no fence and no margin.
#[derive(Clone, Debug)]
pub struct PlantField {
    frame: super::Frame,
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
        Some(Self { frame: super::Frame::from_lot(lots[0]), seed: anchor_seed, members, own, open })
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
    fn solar_sites(&self) -> Vec<Vec2> {
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
    fn wind_sites(&self) -> Vec<(Vec2, f32, f32)> {
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
fn field_yard(g: &mut Geometry, c: &Ctx, field: &PlantField, s: crate::surface::Surface, color: [f32; 3], fence_h: f32) -> Vec<Vec2> {
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
fn solar_field(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, field: &PlantField) {
    let pal = c.palette;
    let yard = field_yard(g, c, field, Dirt, mul(pal.gravel, 0.55), 2.0);
    let yaw = field.frame.yaw();
    for p in field.solar_sites() {
        solar_tracker(g, p, yaw, c.animated_rotors);
    }
    let front = c.front_face(&yard);
    solar_inverter(g, c, rng, &front, 2.4);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}

/// Height of a wind turbine's mast: the rotor sits on top of it.
pub const TURBINE_MAST: f32 = 18.0;
/// Height of the helical rotor.
pub const TURBINE_ROTOR_H: f32 = 11.0;
/// Radius the helical blades sweep.
pub const TURBINE_RADIUS: f32 = 3.0;
/// Spacing between turbines in a farm: about five rotor diameters along a
/// row, rows staggered.
pub const TURBINE_SPACING: f32 = 15.0;
/// Free ground between a blade tip and the lot edge.
pub const TURBINE_CLEARANCE: f32 = 2.0;

/// One helical vertical-axis turbine of a wind farm: where it stands (world
/// x/z), the height its rotor starts at above the lot pad, and the rotor's
/// starting angle. The rotor turns about the vertical through `pos`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turbine {
    pub pos: Vec2,
    pub hub: f32,
    pub phase: f32,
}

const GRAPHITE: [f32; 3] = [0.075, 0.08, 0.085];
const GUNMETAL: [f32; 3] = [0.16, 0.17, 0.18];
const MATTE_BLACK: [f32; 3] = [0.025, 0.025, 0.03];

/// The parcel outline in the lot's own frame, cleaned and counter-clockwise,
/// as the building generators see it.
fn local_lot(lot: &nd_sim::Lot) -> (super::Frame, Vec<Vec2>) {
    let frame = super::Frame::from_lot(lot);
    let mut poly = clean_poly(&lot.polygon.iter().map(|p| frame.to_local(*p)).collect::<Vec<_>>(), 0.3);
    if poly.len() < 3 {
        let (hw, hd) = (frame.width * 0.5, frame.depth * 0.5);
        poly = vec![Vec2::new(-hw, -hd), Vec2::new(hw, -hd), Vec2::new(hw, hd), Vec2::new(-hw, hd)];
    }
    if nd_sim::zoning::polygon_area(&poly) < 0.0 {
        poly.reverse();
    }
    (frame, poly)
}

/// Turbine sites in lot coordinates with a tiny height jitter and a rotor
/// phase: staggered rows `TURBINE_SPACING` apart, centred on the parcel,
/// keeping every blade tip `TURBINE_CLEARANCE` inside the lot and the front
/// strip free for the cabin. A lot too small for a row still gets one
/// turbine where there is the most room, if anywhere.
fn wind_sites(poly: &[Vec2], seed: u64) -> Vec<(Vec2, f32, f32)> {
    let need = TURBINE_RADIUS + TURBINE_CLEARANCE;
    let (lo, hi) = bounds(poly);
    let ok = |p: Vec2| nd_sim::zoning::point_in_polygon(p, poly) && distance_to_boundary(p, poly) >= need && p.y > lo.y + 5.0;
    let row = TURBINE_SPACING * 0.866;
    let rows = (((hi.y - lo.y) - 2.0 * need) / row).floor().max(0.0) as i32 + 1;
    let cols = (((hi.x - lo.x) - 2.0 * need) / TURBINE_SPACING).floor().max(0.0) as i32 + 1;
    let y0 = (lo.y + hi.y) * 0.5 - (rows - 1) as f32 * row * 0.5;
    let x0 = (lo.x + hi.x) * 0.5 - (cols - 1) as f32 * TURBINE_SPACING * 0.5;
    let mut rng = nd_sim::rng_for(seed, 71);
    let mut out: Vec<(Vec2, f32, f32)> = Vec::new();
    for r in 0..rows {
        let shift = if r % 2 == 1 { TURBINE_SPACING * 0.5 } else { 0.0 };
        for k in -1..=cols {
            let p = Vec2::new(x0 + k as f32 * TURBINE_SPACING + shift, y0 + r as f32 * row);
            // Tiny jitter only: a farm reads as one make of turbine.
            let p = p + Vec2::new(rng.random_range(-0.4..0.4), rng.random_range(-0.4..0.4));
            let dh = rng.random_range(-0.3f32..0.3);
            let phase = rng.random_range(0.0..std::f32::consts::TAU);
            if ok(p) && out.iter().all(|(q, _, _)| q.distance(p) >= TURBINE_SPACING * 0.95) {
                out.push((p, dh, phase));
            }
        }
    }
    if out.is_empty() {
        let mut best: Option<(f32, Vec2)> = None;
        let mut v = lo.y;
        while v <= hi.y {
            let mut u = lo.x;
            while u <= hi.x {
                let p = Vec2::new(u, v);
                if ok(p) {
                    let d = distance_to_boundary(p, poly);
                    if best.is_none_or(|(bd, _)| d > bd) {
                        best = Some((d, p));
                    }
                }
                u += 1.0;
            }
            v += 1.0;
        }
        if let Some((_, p)) = best {
            out.push((p, 0.0, rng.random_range(0.0..std::f32::consts::TAU)));
        }
    }
    out.truncate(48);
    out
}

/// The turbines of a wind farm on `lot`, in world coordinates. Depends only
/// on the parcel, the seed and the field it is part of, so the renderer finds the masts the generator
/// drew and puts a spinning rotor on each.
pub fn wind_turbines(lot: &nd_sim::Lot, seed: u64, field: Option<&PlantField>) -> Vec<Turbine> {
    if let Some(field) = field {
        return field.wind_sites().into_iter().map(|(p, dh, phase)| Turbine { pos: p, hub: TURBINE_MAST + dh, phase }).collect();
    }
    let (frame, poly) = local_lot(lot);
    wind_sites(&poly, seed).into_iter().map(|(p, dh, phase)| Turbine { pos: frame.to_world(p.x, p.y), hub: TURBINE_MAST + dh, phase }).collect()
}

/// A helical (Gorlov-type) vertical-axis rotor standing on the origin: a
/// shaft, three graphite blades twisted a third of a turn about it, struts
/// at the bottom, middle and top, a red aviation light on the cap and small
/// cyan tip lights that trace the spin at night. Starts at `y = 0`; the
/// caller lifts it onto the mast and turns it about the vertical.
pub fn wind_rotor(phase: f32) -> Geometry {
    let mut g = Geometry::default();
    let h = TURBINE_ROTOR_H;
    let r = TURBINE_RADIUS;
    let tau = std::f32::consts::TAU;
    g.part(Metal).cylinder(Vec2::ZERO, 0.32, 0.26, 0.0, h + 0.6, 10, rgba(GUNMETAL));
    g.part(Matte).cylinder(Vec2::ZERO, 0.55, 0.45, h + 0.6, h + 1.1, 10, rgba(MATTE_BLACK));
    let steps = 14;
    let twist = tau / 3.0;
    let chord = 0.55;
    let thick = 0.09;
    for b in 0..3 {
        let a0 = phase + b as f32 * tau / 3.0;
        let angle = |t: f32| a0 + twist * t;
        let point = |t: f32| {
            let a = angle(t);
            Vec3::new(a.cos() * r, 0.4 + t * (h - 0.8), a.sin() * r)
        };
        let rings: Vec<Vec<Vec3>> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let a = angle(t);
                let c = point(t);
                let radial = Vec3::new(a.cos(), 0.0, a.sin());
                let tangent = Vec3::new(-a.sin(), 0.0, a.cos());
                [(1.0f32, 0.0f32), (0.35, 1.0), (-0.45, 1.0), (-1.0, 0.0), (-0.45, -1.0), (0.35, -1.0)]
                    .iter()
                    .map(|(u, v)| c + tangent * (u * chord) + radial * (v * thick))
                    .collect()
            })
            .collect();
        g.part(Matte).strip(&rings, rgba(GRAPHITE));
        for t in [0.0f32, 0.5, 1.0] {
            let p = point(t);
            g.part(Metal).beam(Vec3::new(0.0, p.y, 0.0), p, 0.14, rgba(GUNMETAL));
        }
        let tip = point(1.0) + Vec3::Y * 0.15;
        g.part(Glow).boxed(tip, Vec3::splat(0.16), 0.0, rgba([0.25, 1.4, 1.8]));
    }
    g.part(Glow).boxed(Vec3::new(0.0, h + 1.3, 0.0), Vec3::splat(0.35), 0.0, [2.4, 0.15, 0.1, 1.0]);
    g
}

/// A wind farm: helical vertical-axis turbines in staggered rows on dark
/// masts, a cabin at the gate and a fence. With `Ctx::animated_rotors` the
/// rotors are left to the renderer, which spins them.
pub fn wind(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = match c.field {
        Some(field) => field_yard(g, c, field, Foliage, mul(pal.foliage_dry, 1.1), 1.6),
        None => yard(g, c, 0.5, Foliage, mul(pal.foliage_dry, 1.1)),
    };
    weeds(g, c, rng, &yard, 10);
    let toward_front = f.dir3(Vec2::new(0.0, -1.0));
    let sites: Vec<(Vec2, f32, f32)> = match c.field {
        Some(field) => field.wind_sites(),
        None => wind_sites(&c.lot_poly, c.seed).into_iter().map(|(p, dh, phase)| (f.to_world(p.x, p.y), dh, phase)).collect(),
    };
    for (w, dh, phase) in sites {
        let hub = TURBINE_MAST + dh;
        // Footing, a tapered gunmetal mast, a bearing housing on top.
        g.part(Concrete).cylinder(w, 2.2, 2.2, 0.0, 0.5, 12, rgba(mul(pal.concrete(0), 0.8)));
        g.part(Metal).cylinder(w, 0.95, 0.55, 0.5, hub - 0.6, 12, rgba(GUNMETAL));
        g.part(Matte).cylinder(w, 0.75, 0.6, hub - 0.6, hub, 12, rgba(MATTE_BLACK));
        // Service door facing the gate, with a small warm lamp over it.
        let door = Vec3::new(w.x, 1.4, w.y) + toward_front * 0.97;
        g.part(Matte).decal(door, f.dir3(Vec2::X), Vec3::Y, 0.7, 1.8, toward_front, rgba(MATTE_BLACK));
        g.part(Glow).boxed(door + Vec3::Y * 1.2 + toward_front * 0.1, Vec3::new(0.25, 0.08, 0.12), f.yaw(), rgba(mul(pal.street_lamp, 0.8)));
        if !c.animated_rotors {
            let rotor = wind_rotor(phase);
            for (s, m) in rotor.iter() {
                let mut m = m.clone();
                for pos in &mut m.positions {
                    pos[0] += w.x;
                    pos[1] += hub;
                    pos[2] += w.y;
                }
                g.part(s).append(&m);
            }
        }
    }
    let front = c.front_face(&yard);
    let cab = front.at(front.len * 0.3, -3.0);
    if c.inside_local(cab) {
        let cb = fit_inside(c, &rect(cab.x, cab.y, 2.0, 1.6));
        extrude(g, f, Panel, &cb, 0.0, 2.6, mul(GUNMETAL, 1.4));
    }
    if c.field.is_none() {
        fence_lot(g, c, &yard, 1.6, false);
    }
}

// ── grid batteries ─────────────────────────────────────────────────────────

/// A battery module: a container-sized cabinet, along the frame x by deep
/// by high.
const MODULE: Vec3 = Vec3::new(6.0, 2.9, 2.4);
/// Module spacing along a row and between rows (an aisle to open the
/// doors into).
const MODULE_PITCH: f32 = 6.8;
const MODULE_ROW: f32 = 5.4;
/// Status lights on every module's front: one per fifth of charge.
pub const BATTERY_LIGHTS: usize = 5;

/// What a battery's status lights show: lit lights for the share of charge
/// (one per fifth, rounded up, so a sliver of charge still shows one) and
/// their colour for what it is doing.
pub fn battery_lights(level: f32, flow_mw: f32) -> (usize, [f32; 3]) {
    let lit = ((level.clamp(0.0, 1.0) * BATTERY_LIGHTS as f32) - 1e-3).ceil().max(0.0) as usize;
    let col = if flow_mw < -0.05 {
        // Charging: cold cyan.
        [0.15, 1.1, 1.6]
    } else if flow_mw > 0.05 {
        // Delivering: amber.
        [1.7, 0.85, 0.15]
    } else {
        // Holding: green.
        [0.3, 1.4, 0.45]
    };
    (lit.min(BATTERY_LIGHTS), col)
}

/// A grid battery: rows of white container-sized battery modules on
/// plinths, each with fans on the roof, door seams and a row of status
/// lights on its front showing the charge (cyan while charging, amber
/// while delivering, green holding); inverter skids at the row ends, a
/// control cabin at the gate and a fence. How many modules: by the kind,
/// as many as fit.
pub fn battery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let (kind, level, flow) = c.battery.unwrap_or((nd_sim::BatteryKind::Medium, 0.6, 0.0));
    let most = match kind {
        nd_sim::BatteryKind::Small => 2,
        nd_sim::BatteryKind::Medium => 8,
        nd_sim::BatteryKind::Large => 36,
    };
    let yard = yard(g, c, 0.5, Paving, mul(pal.concrete(0), 0.75));
    let inner = offset_poly(&yard, 1.2);
    let (lo, hi) = bounds(&inner);
    let (lit, light) = battery_lights(level, flow);
    let body = [0.78, 0.8, 0.82];
    let half = Vec2::new(MODULE.x * 0.5 + 0.2, MODULE.z * 0.5 + 0.2);
    let fits = |p: Vec2| {
        let r = rect(p.x, p.y, half.x, half.y);
        r.iter().all(|q| nd_sim::zoning::point_in_polygon(*q, &inner)) && distance_to_boundary(p, &inner) >= half.y
    };
    let mut placed = 0;
    let mut v = lo.y + 6.0 + half.y;
    let mut rows: Vec<(f32, f32)> = Vec::new();
    while v < hi.y - half.y && placed < most {
        let mut u = lo.x + half.x;
        let mut row_end: Option<f32> = None;
        while u < hi.x - half.x && placed < most {
            let p = Vec2::new(u, v);
            if fits(p) {
                // Plinth, cabinet, a dark band of louvres along the top.
                f.bx(g, Concrete, p.x, 0.15, p.y, Vec3::new(MODULE.x + 0.3, 0.3, MODULE.z + 0.3), mul(pal.concrete(0), 0.9));
                f.bx(g, Panel, p.x, 0.3 + MODULE.y * 0.5, p.y, MODULE, body);
                for s in [-1.0f32, 1.0] {
                    // Two roof fans.
                    f.bx(g, Matte, p.x + s * 1.5, 0.3 + MODULE.y + 0.05, p.y, Vec3::new(1.1, 0.1, 1.1), MATTE_BLACK);
                }
                let face = super::Face::new(Vec2::new(p.x - MODULE.x * 0.5, p.y - MODULE.z * 0.5), Vec2::new(p.x + MODULE.x * 0.5, p.y - MODULE.z * 0.5), Vec2::new(0.0, -1.0));
                for t in [-1.5f32, 0.0, 1.5] {
                    decal(g, f, Matte, &face, t - 0.75, 1.5, 0.02, 0.04, 2.3, [0.3, 0.32, 0.34]);
                }
                decal(g, f, Matte, &face, 0.0, 0.3 + MODULE.y - 0.25, 0.02, MODULE.x - 0.3, 0.22, [0.12, 0.13, 0.15]);
                // A thin light line under the louvres, in the status colour,
                // and the status lights near the right-hand end, on both
                // long faces so the yard reads from either side.
                let back = super::Face::new(Vec2::new(p.x - MODULE.x * 0.5, p.y + MODULE.z * 0.5), Vec2::new(p.x + MODULE.x * 0.5, p.y + MODULE.z * 0.5), Vec2::new(0.0, 1.0));
                for side in [&face, &back] {
                    decal(g, f, Glow, side, 0.0, 0.3 + MODULE.y - 0.42, 0.025, MODULE.x - 0.6, 0.05, mul(light, 0.45));
                    for k in 0..BATTERY_LIGHTS {
                        let (s, col) = if k < lit { (Glow, light) } else { (Matte, [0.06, 0.07, 0.08]) };
                        decal(g, f, s, side, 1.7 + k as f32 * 0.24, 1.9, 0.03, 0.16, 0.16, col);
                    }
                }
                placed += 1;
                row_end = Some(u + half.x);
            }
            u += MODULE_PITCH;
        }
        if let Some(e) = row_end {
            rows.push((e, v));
        }
        v += MODULE_ROW;
    }
    // An inverter skid at the end of each row where it fits: a grey box
    // with radiator fins.
    for (e, v) in rows {
        let p = Vec2::new(e + 1.4, v);
        if poly_inside(c, &rect(p.x, p.y, 1.1, 1.0)) && nd_sim::zoning::point_in_polygon(p, &inner) {
            f.bx(g, Metal, p.x, 1.0, p.y, Vec3::new(1.6, 2.0, 1.4), mul(pal.metal_paint(1), 0.8));
            for k in 0..3 {
                f.bx(g, Metal, p.x - 0.5 + k as f32 * 0.5, 1.0, p.y - 0.75, Vec3::new(0.06, 1.6, 0.2), mul(pal.steel, 0.6));
            }
        }
    }
    let front = c.front_face(&yard);
    let cab = front.at(front.len * 0.3, -3.0);
    if c.inside_local(cab) {
        let cb = fit_inside(c, &rect(cab.x, cab.y, 2.0, 1.3));
        extrude(g, f, Panel, &cb, 0.0, 2.7, mul(GUNMETAL, 1.6));
        let (blo, bhi) = bounds(&cb);
        let face = super::Face::new(Vec2::new(blo.x, blo.y), Vec2::new(bhi.x, blo.y), Vec2::new(0.0, -1.0));
        if face.len > 1.5 {
            decal(g, f, Glow, &face, 0.0, 2.35, 0.03, face.len * 0.8, 0.07, mul(light, 0.6));
        }
    }
    fence_lot(g, c, &yard, 2.2, false);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}

pub fn nuclear(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.4));
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let concrete = mul(pal.concrete(0), 1.15);
    // Containment: a cylinder with a dome.
    let cc = Vec2::new(lo.x + (hi.x - lo.x) * 0.3, lo.y + (hi.y - lo.y) * 0.6);
    let r = ((hi.x - lo.x).min(hi.y - lo.y) * 0.16).clamp(6.0, 14.0).min(distance_to_boundary(cc, &yard) - 0.5);
    if r >= 2.5 && c.inside_local(cc) {
        let w = f.to_world(cc.x, cc.y);
        g.part(Concrete).cylinder(w, r, r, 0.0, r * 1.8, 20, rgba(concrete));
        let mut prev = MeshData::ngon(w, r, 20, 0.0, r * 1.8);
        for k in 1..=5 {
            let t = k as f32 / 5.0;
            let a = t * std::f32::consts::FRAC_PI_2;
            // The last ring keeps a small radius so no side quad collapses.
            let ring = MeshData::ngon(w, (r * a.cos()).max(r * 0.05), 20, 0.0, r * 1.8 + r * 0.9 * a.sin());
            g.part(Concrete).frustum(&prev, &ring, rgba(mul(concrete, 1.0 - 0.05 * k as f32)));
            prev = ring;
        }
        g.part(Glow).boxed(Vec3::new(w.x, r * 2.75, w.y), Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
    }
    // Turbine and reactor service halls.
    let hall = fit_inside(c, &rect(lo.x + (hi.x - lo.x) * 0.7, lo.y + (hi.y - lo.y) * 0.6, (hi.x - lo.x) * 0.18, (hi.y - lo.y) * 0.3));
    let panel = mul(pick(rng, &pal.panel), 0.95);
    extrude(g, f, Panel, &hall, 0.0, 16.0, panel);
    cornice(g, c, &hall, 15.5, 1.0, 0.3, Panel, mul(panel, 0.8));
    for face in c.faces(&hall) {
        if face.len > 6.0 {
            band_windows(g, c, rng, &face, 10.0, 1, 4.0);
        }
    }
    // Cooling towers: hyperboloids from stacked frustums, with a steam plume.
    for k in 0..2 {
        let p = Vec2::new(lo.x + (hi.x - lo.x) * (0.25 + 0.5 * k as f32), lo.y + (hi.y - lo.y) * 0.18);
        if !c.inside_local(p) {
            continue;
        }
        let w = f.to_world(p.x, p.y);
        let rb = (r * 0.75).min(distance_to_boundary(p, &yard) - 1.5);
        if rb < 2.5 {
            continue;
        }
        let profile = [(0.0f32, 1.0f32), (0.3, 0.78), (0.6, 0.62), (0.8, 0.6), (1.0, 0.68)];
        let th = rb * 3.6;
        for seg in profile.windows(2) {
            let a = MeshData::ngon(w, rb * seg[0].1, 18, 0.0, th * seg[0].0);
            let b = MeshData::ngon(w, rb * seg[1].1, 18, 0.0, th * seg[1].0);
            g.part(Concrete).frustum(&a, &b, rgba(mul(concrete, 0.95)));
        }
        for s in 0..4 {
            let y = th + 1.0 + s as f32 * 4.0;
            g.part(Matte).cylinder(w + Vec2::new(s as f32 * 0.5, 0.0), rb * 0.5 + s as f32 * 0.4, rb * 0.55 + s as f32 * 0.4, y, y + 3.5, 10, rgba([0.9, 0.9, 0.92]));
        }
    }
    // Switchyard, double fence, floodlights, gatehouse.
    let sy = Vec2::new(hi.x - 8.0, hi.y - 7.0);
    if poly_inside(c, &rect(sy.x, sy.y, 6.3, 5.3)) {
        let pen = rect(sy.x, sy.y, 6.0, 5.0);
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.2, None, true);
        for k in 0..4 {
            let p = sy + Vec2::new((k as f32 - 1.5) * 3.0, 0.0);
            f.bx(g, Metal, p.x, 1.3, p.y, Vec3::new(1.8, 2.6, 1.3), mul(pal.metal_paint(1), 0.9));
            f.bx(g, Matte, p.x, 4.0, p.y, Vec3::new(0.15, 2.6, 0.15), [0.6, 0.6, 0.55]);
        }
        f.bx(g, Matte, sy.x, 5.3, sy.y, Vec3::new(11.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
        f.bx(g, Matte, sy.x, 5.3, sy.y + 1.2, Vec3::new(11.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 3.0, true);
    fence_lot(g, c, &offset_poly(&yard, 1.5), 2.4, true);
    for p in [front.at(-front.len * 0.42, -2.0), front.at(front.len * 0.42, -2.0), Vec2::new(lo.x + 3.0, hi.y - 3.0), Vec2::new(hi.x - 3.0, lo.y + 3.0)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 12.0);
        }
    }
    let gate = front.at(front.len * 0.3, -3.0);
    if c.inside_local(gate) {
        let house = fit_inside(c, &rect(gate.x, gate.y, 2.0, 1.6));
        extrude(g, f, Panel, &house, 0.0, 3.0, mul(panel, 0.9));
        curtain_lights(g, c, rng, &house, 1.0, 1, 1.6);
    }
    bollards(g, c, &front, 6, -1.4);
    sign_board(g, c, rng, &front, 0.0, 3.2, (front.len * 0.4).min(10.0), 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(w: f32, d: f32) -> Vec<Vec2> {
        vec![Vec2::new(-w * 0.5, -d * 0.5), Vec2::new(w * 0.5, -d * 0.5), Vec2::new(w * 0.5, d * 0.5), Vec2::new(-w * 0.5, d * 0.5)]
    }

    #[test]
    fn turbine_count_scales_with_lot_area_and_all_fit() {
        let mut last = 0;
        for side in [24.0f32, 40.0, 60.0, 90.0, 130.0] {
            let poly = square(side, side);
            let sites = wind_sites(&poly, 9);
            assert!(!sites.is_empty(), "a {side} m lot holds a turbine");
            assert!(sites.len() >= last, "{side} m: {} turbines, fewer than a smaller lot's {last}", sites.len());
            last = sites.len();
            for (p, dh, _) in &sites {
                assert!(nd_sim::zoning::point_in_polygon(*p, &poly));
                assert!(distance_to_boundary(*p, &poly) >= TURBINE_RADIUS + TURBINE_CLEARANCE - 1e-3, "{side} m: rotor reaches past the lot");
                assert!(dh.abs() <= 0.3, "turbines are one size");
            }
            for (i, (a, _, _)) in sites.iter().enumerate() {
                for (b, _, _) in &sites[i + 1..] {
                    assert!(a.distance(*b) >= TURBINE_SPACING * 0.95 - 1e-3, "{side} m: turbines too close");
                }
            }
        }
        assert!(last >= 20, "a 130 m farm holds a grid of turbines, got {last}");
        assert!(wind_sites(&square(8.0, 8.0), 1).is_empty(), "no turbine on a lot too small for its blades");
    }

    /// Trackers pack densely (about one per 16 m² of open field), keep
    /// their pitch, keep every panel tip inside the lot even fully tilted,
    /// and leave the front strip free; triangle lots work too.
    #[test]
    fn solar_trackers_pack_a_honeycomb_inside_the_lot() {
        for (w, d) in [(30.0f32, 30.0f32), (60.0, 45.0), (120.0, 80.0)] {
            let poly = square(w, d);
            let sites = solar_sites(&poly);
            assert!(!sites.is_empty(), "{w}x{d} holds trackers");
            let open = (w - 2.0 * (SOLAR_PANEL_R + SOLAR_EDGE)) * (d - SOLAR_FRONT_CLEAR - 2.0 * SOLAR_PANEL_R - SOLAR_EDGE);
            assert!(sites.len() as f32 >= open / 20.0, "{w}x{d}: {} trackers for {open:.0} m²", sites.len());
            for (i, p) in sites.iter().enumerate() {
                assert!(distance_to_boundary(*p, &poly) >= SOLAR_PANEL_R + SOLAR_EDGE - 1e-3);
                assert!(p.y >= -d * 0.5 + SOLAR_FRONT_CLEAR);
                for q in &sites[i + 1..] {
                    assert!(p.distance(*q) >= SOLAR_PITCH - 1e-3);
                }
            }
        }
        let tri = vec![Vec2::new(-50.0, -30.0), Vec2::new(60.0, -30.0), Vec2::new(-10.0, 50.0)];
        for p in solar_sites(&tri) {
            assert!(nd_sim::zoning::point_in_polygon(p, &tri) && distance_to_boundary(p, &tri) >= SOLAR_PANEL_R);
        }
        // The panel turned as far as it goes stays above the ground and
        // within its circle.
        assert!(SOLAR_HUB - SOLAR_PANEL_R * SOLAR_MAX_TILT.sin() > 0.4);
        let head = solar_head();
        let reach = head.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt()).fold(0.0f32, f32::max);
        assert!(reach <= SOLAR_PANEL_R + 1e-3, "panel reaches {reach}");
        assert!(head.triangle_count() < 120, "{} triangles a tracker", head.triangle_count());
        assert!(!head.get(Glow).positions.is_empty(), "a rim light");
    }

    #[test]
    fn battery_lights_count_fifths_and_colour_by_flow() {
        assert_eq!(battery_lights(0.0, 0.0).0, 0);
        assert_eq!(battery_lights(0.01, 0.0).0, 1, "a sliver of charge shows");
        assert_eq!(battery_lights(0.2, 0.0).0, 1);
        assert_eq!(battery_lights(0.21, 0.0).0, 2);
        assert_eq!(battery_lights(1.0, 0.0).0, BATTERY_LIGHTS);
        let (_, charging) = battery_lights(0.5, -5.0);
        let (_, giving) = battery_lights(0.5, 5.0);
        let (_, idle) = battery_lights(0.5, 0.0);
        assert!(charging != giving && giving != idle && idle != charging);
    }

    #[test]
    fn turbine_layout_is_deterministic_and_triangle_lots_stay_contained() {
        let tri = vec![Vec2::new(-50.0, -30.0), Vec2::new(60.0, -30.0), Vec2::new(-10.0, 50.0)];
        assert_eq!(wind_sites(&tri, 4), wind_sites(&tri, 4));
        for (p, _, _) in wind_sites(&tri, 4) {
            assert!(distance_to_boundary(p, &tri) >= TURBINE_RADIUS + TURBINE_CLEARANCE - 1e-3);
        }
        let rotor = wind_rotor(0.3);
        let reach = rotor.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt()).fold(0.0f32, f32::max);
        assert!(reach <= TURBINE_RADIUS + 0.7, "blades sweep {reach} m, past the clearance");
    }
}
