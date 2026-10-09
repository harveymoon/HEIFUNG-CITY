//! Solar farms: hexagonal panels on dual-axis trackers.

use super::*;

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
pub(super) const SOLAR_EDGE: f32 = 1.0;
/// Strip along the road kept for the gate, the inverter and the sign.
pub(super) const SOLAR_FRONT_CLEAR: f32 = 6.5;
/// Most trackers one parcel draws (the renderer animates each).
pub(super) const SOLAR_MAX: usize = 600;

pub(super) const PANEL_BLUE: [f32; 3] = [0.006, 0.016, 0.075];
pub(super) const PANEL_CELL: [f32; 3] = [0.01, 0.026, 0.11];
pub(super) const PANEL_BACK: [f32; 3] = [0.05, 0.055, 0.065];
pub(super) const PANEL_RIM: [f32; 3] = [0.08, 0.55, 0.95];

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
pub(super) fn solar_sites(poly: &[Vec2]) -> Vec<Vec2> {
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
            let (frame, poly) = crate::buildings::Frame::lot_local(lot);
            solar_sites(&poly).into_iter().map(|p| frame.to_world(p.x, p.y)).collect()
        }
    };
    sites.into_iter().map(|pos| SolarTracker { pos, hub: SOLAR_HUB }).collect()
}

/// The hexagon's corners about `c` at height `y`, flat sides towards the
/// grid's neighbours along x (corners at 30 + 60k degrees).
pub(super) fn hex(c: Vec2, r: f32, y: f32) -> Vec<Vec3> {
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
pub(super) fn solar_tracker(g: &mut Geometry, w: Vec2, yaw: f32, animated: bool) {
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
pub(super) fn solar_inverter(g: &mut Geometry, c: &Ctx, front: &crate::buildings::Face, d: f32) {
    let f = &c.frame;
    let cab = front.at(front.len * 0.3, -d);
    if !c.inside_local(cab) {
        return;
    }
    let cb = fit_inside(c, &rect(cab.x, cab.y, 2.6, 1.3));
    extrude(g, f, Panel, &cb, 0.0, 2.6, mul(GUNMETAL, 1.5));
    let (lo, hi) = bounds(&cb);
    let face = crate::buildings::Face::new(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(0.0, -1.0));
    if face.len > 1.5 {
        decal(g, f, Glow, &face, 0.0, 2.2, 0.03, face.len * 0.8, 0.08, mul(PANEL_RIM, 0.7));
        decal(g, f, Matte, &face, -face.len * 0.25, 1.05, 0.02, 0.9, 2.0, MATTE_BLACK);
    }
    let tx = cab + Vec2::new(4.2, 0.0);
    if c.inside_local(tx) && distance_to_boundary(tx, &c.lot_poly) > 1.0 {
        f.bx(g, Metal, tx.x, 1.0, tx.y, Vec3::new(1.4, 2.0, 1.0), mul(c.palette.metal_paint(1), 0.9));
    }
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
    solar_inverter(g, c, &front, 3.0);
    fence_lot(g, c, &yard, 2.0, false);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}
