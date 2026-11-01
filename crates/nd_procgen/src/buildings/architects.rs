//! [r6-architects] The architects' league: six landmark attractions in the
//! spirit of three modern masters, each fitted to the site it is put on.
//!
//! - Organic and Prairie (after Frank Lloyd Wright): the *ledge house*, pale
//!   concrete trays cantilevered from a stone hearth out over water, a drop
//!   or a pool of its own; the *prairie hall*, long low wings of roman brick
//!   under deep hipped eaves, art-glass ribbon windows and a broad chimney.
//! - White modernism (after Le Corbusier): the *hill chapel*, thick curved
//!   white walls pierced by coloured slots under a dark concrete roof that
//!   sails up to the view; the *sky slab*, a housing block raised on sculpted
//!   pillars, coloured loggias behind a concrete sun-screen, a shopping street
//!   half way up and a roof terrace.
//! - Nordic organic modern (after Alvar Aalto): the *wave library*, a white
//!   block with an undulating timber canopy and a reading hall fanning out to
//!   the view under round skylights; the *house of culture*, a red-brick fan
//!   hall with stepped clerestories over its roof round a courtyard of grass
//!   steps.
//!
//! Inspiration only: none copies a real building, and every name the game
//! gives them is invented (`ServiceKind::name`).
//!
//! **Fitted to the plot.** Each reads the site: its outline (the biggest
//! rectangle along its grain, [`best_plan`]), its street front, its size
//! (larger sites, bigger and richer buildings) and what lies off it
//! ([`SiteLook`]: open water, or the way the ground falls). The ledge house
//! reaches its terraces toward the water or down the slope, the chapel's
//! roof sails up toward the view, the fans open toward it, the slab runs
//! along the long side. A seeded stream varies the proportions, colours
//! and counts, so no two cities get the same copy.
//!
//! Detail (mullions, louvres, battens, railings, furniture) goes into the
//! LOD detail layers; the masses, the lit glazing and the neon stay in the
//! base mesh so the night city reads the same from afar.

use super::attractions::{fill_site, fits, front_lamps, neon_line, w3, wv};
use super::parts::*;
use super::{Ctx, Face};
use crate::mesh::MeshData;
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::services::ServiceKind;
use nd_sim::zoning::{point_in_polygon, polygon_area};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

// ── materials ────────────────────────────────────────────────────────────────

/// Warm sandstone laid in rough courses.
const SANDSTONE: [f32; 3] = [0.56, 0.47, 0.36];
/// The pale ochre concrete of the trays.
const TRAY: [f32; 3] = [0.80, 0.74, 0.60];
/// The deep red of the steel sashes.
const SASH_RED: [f32; 3] = [0.50, 0.14, 0.10];
/// Long thin roman brick, tawny.
const ROMAN_BRICK: [f32; 3] = [0.60, 0.42, 0.30];
const CREAM_STONE: [f32; 3] = [0.80, 0.75, 0.63];
const OCHRE_RENDER: [f32; 3] = [0.70, 0.58, 0.42];
const COPPER_GREEN: [f32; 3] = [0.22, 0.34, 0.29];
const DARK_TIMBER: [f32; 3] = [0.26, 0.17, 0.11];
/// White render, a little greyed by the city's air.
const WHITE: [f32; 3] = [0.84, 0.83, 0.79];
/// Raw board-marked concrete.
const BETON: [f32; 3] = [0.52, 0.51, 0.48];
const BETON_DARK: [f32; 3] = [0.36, 0.35, 0.33];
/// The loggias' painted side walls.
const LOGGIA: [[f32; 3]; 6] = [[0.70, 0.14, 0.10], [0.85, 0.66, 0.10], [0.14, 0.30, 0.60], [0.20, 0.46, 0.28], [0.86, 0.85, 0.80], [0.85, 0.42, 0.12]];
/// Red brick, a deep Nordic red.
const RED_BRICK: [f32; 3] = [0.50, 0.20, 0.14];
/// Birch-pale timber.
const BIRCH: [f32; 3] = [0.74, 0.58, 0.40];
const BIRCH_BARK: [f32; 3] = [0.86, 0.85, 0.80];
/// Dark patinated copper.
const COPPER_DARK: [f32; 3] = [0.26, 0.30, 0.26];
const GRANITE: [f32; 3] = [0.14, 0.14, 0.15];
const WATER: [f32; 3] = [0.10, 0.30, 0.36];

/// Window light colours: warm lamplight and the art-glass hues.
const LAMP: [f32; 3] = [1.9, 1.25, 0.6];
const LAMP_SOFT: [f32; 3] = [1.4, 1.0, 0.6];
const ART_AMBER: [f32; 3] = [2.0, 1.1, 0.3];
const ART_GREEN: [f32; 3] = [0.7, 1.5, 0.5];
/// Neon accents, kept to a line or two: these are fine buildings.
const AMBER_LINE: [f32; 3] = [2.4, 1.3, 0.35];
const CYAN_LINE: [f32; 3] = [0.25, 1.6, 2.0];
/// The chapel's coloured glass.
const SLOT_GLASS: [[f32; 3]; 5] = [[0.25, 0.45, 2.2], [2.2, 0.35, 0.25], [2.2, 1.7, 0.3], [0.4, 1.8, 0.6], [1.7, 1.6, 1.4]];

// ── the site ─────────────────────────────────────────────────────────────────

/// The architects' landmarks, which read what lies off their site.
pub const ARCHITECTS: [ServiceKind; 6] = [ServiceKind::LedgeHouse, ServiceKind::PrairieHall, ServiceKind::HillChapel, ServiceKind::SkySlab, ServiceKind::FanLibrary, ServiceKind::CultureHouse];

/// How far off its site an architects' landmark looks for water, metres.
pub const WATER_REACH: f32 = 60.0;
/// A fall of the ground below the pad that counts as a view downhill, metres.
pub const VIEW_FALL: f32 = 1.5;

/// [r6-architects] What lies off a landmark's site: which way it looks
/// out, at water or down a slope, and how far the ground drops there. Its
/// generator turns its terraces, fans and roof that way.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SiteLook {
    /// Unit vector (world) the building opens toward: open water within
    /// [`WATER_REACH`], else the way the ground falls; zero for neither.
    pub view: Vec2,
    /// How far the water is from the site's edge, metres (None: none near).
    pub water: Option<f32>,
    /// How far the ground falls below the pad just off the site on the
    /// view side, metres.
    pub fall: f32,
}

/// [r6-architects] The [`SiteLook`] of an architects' landmark (None for
/// anything else): the nearest water off its outline, else the lowest
/// ground in a ring round it.
pub fn site_look(world: &nd_sim::World, id: nd_sim::BuildingId) -> Option<SiteLook> {
    let b = world.buildings.get(id)?;
    if !ARCHITECTS.contains(&b.service?.kind) {
        return None;
    }
    let lot = world.lots.get(b.lot)?;
    Some(look_at(&lot.polygon, lot.center, world.lot_pad(b.lot), &world.landscape, |p| world.ground_height(p)))
}

/// The look from a site `poly` round `centre` whose pad stands at `pad`,
/// over `land` whose ground (pads, embankments) is `ground`.
pub fn look_at(poly: &[Vec2], centre: Vec2, pad: f32, land: &nd_sim::landscape::Landscape, ground: impl Fn(Vec2) -> f32) -> SiteLook {
    let n = poly.len();
    if n < 3 {
        return SiteLook::default();
    }
    let r = poly.iter().map(|p| p.distance(centre)).fold(0.0f32, f32::max);
    // The corners and the middles of the sides: a few probes, the shore
    // search walks every waterline.
    let mut water: Option<(f32, Vec2)> = None;
    if !land.is_flat() {
        for i in 0..n {
            for p in [poly[i], (poly[i] + poly[(i + 1) % n]) * 0.5] {
                if let Some(s) = land.nearest_shore(p, WATER_REACH) {
                    if water.is_none_or(|(d, _)| s.distance < d - 1e-3) {
                        water = Some((s.distance, s.seaward));
                    }
                }
            }
        }
    }
    let surface = |p: Vec2| ground(p).max(nd_sim::landscape::WATER_LEVEL);
    let (view, dist) = match water {
        Some((d, sea)) => (sea.normalize_or_zero(), Some(d)),
        None => {
            let mut low = (0.0f32, Vec2::ZERO);
            for k in 0..16 {
                let d = Vec2::from_angle(k as f32 / 16.0 * TAU);
                let fall = pad - surface(centre + d * (r + 10.0));
                if fall > low.0 + 1e-3 {
                    low = (fall, d);
                }
            }
            (if low.0 >= VIEW_FALL { low.1 } else { Vec2::ZERO }, None)
        }
    };
    let fall = if view == Vec2::ZERO { 0.0 } else { (pad - surface(centre + view * (r + 8.0))).clamp(0.0, 30.0) };
    SiteLook { view, water: dist, fall }
}

/// The site as a generator sees it: which way to look out (lot-local), the
/// water and the fall there, and how much bigger than the smallest site it
/// is (1 for the smallest; larger sites build bigger, richer buildings).
struct Site {
    view: Vec2,
    water: Option<f32>,
    fall: f32,
    k: f32,
    /// A view the sim gave (water or a slope), not a default.
    real: bool,
}

impl Site {
    fn of(c: &Ctx, rng: &mut impl Rng, base_area: f32) -> Site {
        let area = polygon_area(&c.lot_poly).abs();
        let k = (area / base_area).sqrt().clamp(0.85, 1.7);
        if let Some(s) = c.site.filter(|s| s.view.length_squared() > 0.25) {
            let v = c.frame.to_local(c.frame.center + s.view).normalize_or(Vec2::Y);
            return Site { view: v, water: s.water, fall: s.fall, k, real: true };
        }
        // No view off the site: open to the back of it, away from the
        // street, turned a little either way.
        Site { view: Vec2::from_angle(FRAC_PI_2 + rng.random_range(-0.45..0.45)), water: None, fall: 0.0, k, real: false }
    }

    /// Water close enough to build out over (the terraces may reach a
    /// little past the lot line there).
    fn over_water(&self) -> bool {
        self.water.is_some_and(|d| d < 30.0)
    }
}

/// A rectangle on the site: centre, unit long axis, half length along it
/// and half width across.
#[derive(Clone, Copy, Debug)]
struct Plan {
    c: Vec2,
    ax: Vec2,
    hl: f32,
    hw: f32,
}

impl Plan {
    fn across(&self) -> Vec2 {
        self.ax.perp()
    }

    fn at(&self, a: f32, b: f32) -> Vec2 {
        self.c + self.ax * a + self.across() * b
    }

    /// The rectangle `a0..a1` along by `b0..b1` across, counter-clockwise.
    fn quad(&self, a0: f32, a1: f32, b0: f32, b1: f32) -> Vec<Vec2> {
        vec![self.at(a0, b0), self.at(a1, b0), self.at(a1, b1), self.at(a0, b1)]
    }

    fn poly(&self) -> Vec<Vec2> {
        self.quad(-self.hl, self.hl, -self.hw, self.hw)
    }

    fn grown(&self, d: f32) -> Plan {
        Plan { hl: self.hl + d, hw: self.hw + d, ..*self }
    }
}

/// Every corner of `poly` and every point along its sides on the lot,
/// `margin` in from its edge.
fn sits(c: &Ctx, poly: &[Vec2], margin: f32) -> bool {
    poly.len() >= 3 && poly_inside(c, poly) && poly.iter().all(|p| distance_to_boundary(*p, &c.lot_poly) >= margin - 1e-3)
}

/// The biggest rectangle `margin` in from the lot's edge, along one of its
/// sides' directions (the site's grain) or the street's, with its long
/// side at most `max_aspect` times its short. None on a site with no room.
fn best_plan(c: &Ctx, margin: f32, max_aspect: f32) -> Option<Plan> {
    let poly = &c.lot_poly;
    let n = poly.len();
    let mut axes: Vec<Vec2> = vec![Vec2::X, Vec2::Y];
    for i in 0..n {
        let d = poly[(i + 1) % n] - poly[i];
        if d.length() > 6.0 {
            let d = d.normalize();
            if axes.iter().all(|a| a.dot(d).abs() < 0.995 && a.perp_dot(d).abs() > 0.06) {
                axes.push(d);
            }
        }
    }
    let anchor = interior_anchor(poly);
    let mut best: Option<Plan> = None;
    for ax in axes {
        let pe = ax.perp();
        let (mut alo, mut ahi, mut blo, mut bhi) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in poly {
            let (a, b) = ((*p - anchor).dot(ax), (*p - anchor).dot(pe));
            (alo, ahi, blo, bhi) = (alo.min(a), ahi.max(a), blo.min(b), bhi.max(b));
        }
        let start = Plan { c: anchor + ax * ((alo + ahi) * 0.5) + pe * ((blo + bhi) * 0.5), ax, hl: (ahi - alo) * 0.5 - margin, hw: (bhi - blo) * 0.5 - margin };
        // Shrink both ways, mostly the length, or mostly the width.
        for (kl, kw) in [(0.93f32, 0.93f32), (0.9, 0.98), (0.98, 0.9)] {
            let mut p = start;
            for _ in 0..40 {
                if p.hl < 2.0 || p.hw < 2.0 {
                    break;
                }
                if sits(c, &p.poly(), margin) {
                    let p = aspect(p, max_aspect);
                    if sits(c, &p.poly(), margin) && best.is_none_or(|b| p.hl * p.hw > b.hl * b.hw) {
                        best = Some(p);
                    }
                    break;
                }
                p.hl *= kl;
                p.hw *= kw;
                p.c += (anchor - p.c) * 0.12;
            }
        }
    }
    best.map(|p| if p.hw > p.hl { Plan { c: p.c, ax: p.ax.perp(), hl: p.hw, hw: p.hl } } else { p })
}

/// `p` cut down so its long side is at most `max` times its short.
fn aspect(p: Plan, max: f32) -> Plan {
    let (l, w) = (p.hl.max(p.hw), p.hl.min(p.hw));
    if l <= w * max {
        return p;
    }
    if p.hl >= p.hw { Plan { hl: w * max, ..p } } else { Plan { hw: w * max, ..p } }
}

/// A band `depth` deep along the street front, `setback` in from it, as
/// long as the site allows up to `max_hl` either side of the middle: where
/// a building that addresses the street stands. Its axis runs along the
/// street and `across()` points away from it.
fn street_band(c: &Ctx, setback: f32, depth: f32, max_hl: f32) -> Option<Plan> {
    let front = c.front_face(&c.lot_poly);
    let inward = -front.out;
    let anchor = interior_anchor(&c.lot_poly);
    let mut p = Plan { c: front.mid() + inward * (setback + depth * 0.5), ax: front.tangent, hl: (front.len * 0.5 - 1.0).min(max_hl), hw: depth * 0.5 };
    let along = |c0: Vec2, q: Vec2| (q - c0).dot(front.tangent);
    // Centre it on the site's deep middle, along the street.
    p.c += front.tangent * along(p.c, anchor).clamp(-p.hl * 0.5, p.hl * 0.5) * 0.5;
    for _ in 0..40 {
        if sits(c, &p.poly(), 0.6) {
            return Some(p);
        }
        p.hl *= 0.93;
        p.c += front.tangent * along(p.c, anchor) * 0.1;
        if p.hl < 4.0 {
            break;
        }
    }
    None
}

/// Distance from `p` along `d` to the lot's edge (0 when `p` is off it).
fn ray(c: &Ctx, p: Vec2, d: Vec2) -> f32 {
    let poly = &c.lot_poly;
    let n = poly.len();
    let mut best = f32::MAX;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let e = b - a;
        let den = d.perp_dot(e);
        if den.abs() < 1e-6 {
            continue;
        }
        let t = (a - p).perp_dot(e) / den;
        let u = (a - p).perp_dot(d) / den;
        if t >= 0.0 && (0.0..=1.0).contains(&u) {
            best = best.min(t);
        }
    }
    if best == f32::MAX || !c.inside_local(p) { 0.0 } else { best }
}

/// Counter-clockwise (positive area) copy of a lot-local polygon.
fn ccw(mut poly: Vec<Vec2>) -> Vec<Vec2> {
    if polygon_area(&poly) < 0.0 {
        poly.reverse();
    }
    poly
}

/// A low-pitched hipped roof over the rectangle `p` (its eaves already
/// out where they should be) springing at `y`, `rise` to the ridge: two
/// long slopes and two hips, a fascia board and a soffit seen from below.
#[allow(clippy::too_many_arguments)]
fn hip_roof(g: &mut Geometry, c: &Ctx, s: Surface, p: Plan, y: f32, rise: f32, col: [f32; 3], fascia: [f32; 3], soffit: [f32; 3]) {
    let f = &c.frame;
    let p = if p.hw > p.hl { Plan { c: p.c, ax: p.ax.perp(), hl: p.hw, hw: p.hl } } else { p };
    let ring = p.poly();
    // The soffit and the fascia board round the eaves.
    g.part(Wood).cap(&f.poly3(&ring, y - 0.3), Vec3::NEG_Y, rgba(soffit));
    g.part(Wood).band(&f.poly3(&ring, y - 0.3), &f.poly3(&ring, y), rgba(fascia));
    let rh = (p.hl - p.hw).max(0.0);
    let (r1, r2) = (w3(c, p.at(-rh, 0.0), y + rise), w3(c, p.at(rh, 0.0), y + rise));
    let [a, b, cc, d] = [ring[0], ring[1], ring[2], ring[3]].map(|q| w3(c, q, y));
    let (ax3, pe3) = (f.dir3(p.ax), f.dir3(p.across()));
    let slope = |h: Vec3, run: f32| (h * rise + Vec3::Y * run).normalize();
    let m = g.part(s);
    if rh > 0.05 {
        m.quad_facing([a, b, r2, r1], slope(-pe3, p.hw), rgba(col));
        m.quad_facing([cc, d, r1, r2], slope(pe3, p.hw), rgba(col));
    } else {
        m.tri_facing(a, b, r2, slope(-pe3, p.hw), rgba(col));
        m.tri_facing(cc, d, r2, slope(pe3, p.hw), rgba(col));
    }
    m.tri_facing(b, cc, r2, slope(ax3, p.hw), rgba(mul(col, 0.94)));
    m.tri_facing(d, a, r1, slope(-ax3, p.hw), rgba(mul(col, 0.94)));
}

/// A band of glazing along a face from `y0`, `h` tall: dark glass, panes
/// lit in `lit` colours by the window lights, and mullions every `every`
/// metres (detail).
#[allow(clippy::too_many_arguments)]
fn ribbon(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, h: f32, every: f32, lit: &[[f32; 3]], mullion: [f32; 3]) {
    if face.len < 1.0 || h < 0.3 {
        return;
    }
    let f = &c.frame;
    let w = face.len - 0.3;
    face.decal(g, f, Glass, 0.0, y0 + h * 0.5, 0.05, w, h, c.palette.glass_dark);
    let n = (w / every).round().max(1.0) as i32;
    let pw = w / n as f32;
    for k in 0..n {
        let t = -w * 0.5 + (k as f32 + 0.5) * pw;
        if coin(rng, c.lit_chance()) {
            face.glaze(g, c, t, y0 + h * 0.5, 0.08, pw * 0.86, h * 0.86, Pane::Lit(lit[rng.random_range(0..lit.len())]));
        }
    }
    g.lod(Lod::Mid, |g| {
        for k in 0..=n {
            let t = -w * 0.5 + k as f32 * pw;
            face.bx(g, f, Matte, t, y0 + h * 0.5, 0.1, Vec3::new(0.09, h, 0.12), mullion);
        }
        face.bx(g, f, Matte, 0.0, y0 + 0.04, 0.12, Vec3::new(w, 0.08, 0.2), mullion);
    });
}

/// Fan of triangles from `apex` to each pair of `ring` points (3D),
/// facing up or down: a roof that is not flat.
fn fan3(g: &mut Geometry, s: Surface, apex: Vec3, ring: &[Vec3], closed: bool, up: bool, col: [f32; 3]) {
    let n = ring.len();
    let out = if up { Vec3::Y } else { Vec3::NEG_Y };
    for i in 0..if closed { n } else { n.saturating_sub(1) } {
        g.part(s).tri_facing(apex, ring[i], ring[(i + 1) % n], out, rgba(col));
    }
}

/// A fan-shaped plan: from `apex` along `dir`, `half` radians either
/// side, between radii `r0` and `r1`, the arcs in `n` steps;
/// counter-clockwise. `r0` of zero gives a sector with its point.
fn fan_poly(apex: Vec2, dir: Vec2, half: f32, r0: f32, r1: f32, n: usize) -> Vec<Vec2> {
    let a0 = dir.to_angle();
    let mut out: Vec<Vec2> = (0..=n).map(|k| apex + Vec2::from_angle(a0 - half + 2.0 * half * k as f32 / n as f32) * r1).collect();
    if r0 < 0.05 {
        out.push(apex);
    } else {
        out.extend((0..=n).rev().map(|k| apex + Vec2::from_angle(a0 - half + 2.0 * half * k as f32 / n as f32) * r0));
    }
    ccw(out)
}

/// Trees in the free ground of the site: `n` tries, `conifers` of them
/// pines, the rest round (birch-barked for `birch`), clear of `taken`
/// (polygons with a margin each).
#[allow(clippy::too_many_arguments)]
fn woods(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, n: usize, conifers: f32, birch: bool, taken: &[(Vec<Vec2>, f32)], scale: f32) {
    let pal = c.palette;
    let mut planted: Vec<Vec2> = Vec::new();
    for _ in 0..n {
        let Some(p) = c.random_inside(rng, 2.0) else { continue };
        let clear = taken.iter().all(|(poly, m)| !point_in_polygon(p, poly) && distance_to_boundary(p, poly) > *m) && planted.iter().all(|q| q.distance(p) > 3.6);
        if !clear || !fits(c, p, 1.6, 0.2) {
            continue;
        }
        planted.push(p);
        let h = rng.random_range(5.5..9.0) * scale;
        let leaf = mul(pal.foliage, rng.random_range(0.75..1.05));
        if rng.random::<f32>() < conifers {
            tree_kind(g, wv(c, p), 0.0, TreeKind::Conifer, h * 1.2, rng.random_range(1.6..2.3), pal.bark, mul(leaf, 0.8));
        } else {
            let bark = if birch { BIRCH_BARK } else { pal.bark };
            tree_kind(g, wv(c, p), 0.0, TreeKind::Round, h * 0.75, rng.random_range(2.2..3.4), bark, leaf);
        }
    }
}

/// Lamplight behind glass that only shows after dark (`NIGHT_MARK`): the
/// homes in the housing block, the house's rooms as the guides close up.
/// `panes` are (face, t, y, w, h) at `d` proud of the face.
fn night_lights(g: &mut Geometry, c: &Ctx, panes: &[(Face, f32, f32, f32, f32)], d: f32, col: [f32; 3]) {
    let from = g.get(Glow).positions.len();
    for (face, t, y, w, h) in panes {
        face.decal(g, &c.frame, Glow, *t, *y, d, *w, *h, col);
    }
    mark_glow(g, from, super::NIGHT_MARK);
}

/// A low warm bollard light on the ground (lot-local).
fn bollard_light(g: &mut Geometry, c: &Ctx, p: Vec2) {
    if !fits(c, p, 0.3, 0.2) {
        return;
    }
    let w = wv(c, p);
    g.part(Matte).cylinder(w, 0.12, 0.12, 0.0, 0.75, 6, rgba(DARK));
    g.part(Glow).cylinder(w, 0.13, 0.13, 0.55, 0.72, 6, rgba(LAMP));
}

/// A path of pale paving from `a` to `b`, `w` wide.
fn walk(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, w: f32, col: [f32; 3]) {
    if a.distance(b) < 0.5 {
        return;
    }
    // As wide as the lot lets it be at both ends.
    let n = (b - a).normalize_or_zero().perp();
    let mut w = w;
    while w > 0.8 && ![a + n * w * 0.5, a - n * w * 0.5, b + n * w * 0.5, b - n * w * 0.5].iter().all(|p| c.inside_local(*p)) {
        w *= 0.8;
    }
    if w <= 0.8 {
        return;
    }
    let pts = [wv(c, a), wv(c, b)];
    g.part(Paving).ground_strip(&pts, w, 0.045, rgba(col));
}

/// The front face of the lot (towards the street) and the point on it
/// nearest `p`, pulled `d` in.
fn front_door_path(c: &Ctx, p: Vec2, d: f32) -> Vec2 {
    let front = c.front_face(&c.lot_poly);
    let t = (p - front.mid()).dot(front.tangent).clamp(-front.len * 0.45, front.len * 0.45);
    front.at(t, -d)
}

// ── the ledge house (organic: after Wright) ──────────────────────────────────

/// A house museum of pale concrete trays cantilevered from a stone hearth:
/// each tray crosses the one below, reaching toward the water, down the
/// slope, or out over a rock pool and cascade of its own on flat ground;
/// red steel sashes, a trellis over the way in, a wood round it.
pub fn ledge_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = Site::of(c, rng, 1600.0);
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.6));
    let Some(plan) = best_plan(c, 2.0, 3.0) else { return };
    // No view off the site: the trays reach along its length rather than
    // across a shallow plot.
    let mut v = site.view;
    if !site.real {
        let along = if ray(c, plan.c, plan.ax) >= ray(c, plan.c, -plan.ax) { plan.ax } else { -plan.ax };
        if ray(c, plan.c, along) > ray(c, plan.c, v) * 1.5 {
            v = Vec2::from_angle(along.to_angle() + rng.random_range(-0.15..0.15));
        }
    }
    let s = v.perp();
    let k = site.k.min(1.45);
    let centre = plan.c;
    // The hearth stands back from the view; the trays reach out from it.
    let back = ray(c, centre, -v);
    let core = centre - v * (back * 0.45).min(9.0);
    let reach = ray(c, core, v);
    if reach < 6.0 {
        // Too shallow toward the view: turn to the deeper way.
        return ledge_house_small(g, c, rng, plan);
    }
    let over = if site.over_water() { 2.4 } else { 0.0 };
    let side_room = ray(c, core, s).min(ray(c, core, -s));
    let levels = 2 + (k > 1.12) as usize + (k > 1.38) as usize;
    let fh = 3.5;
    let level_y = |i: usize| 2.9 + i as f32 * fh;
    let at = |a: f32, b: f32| core + v * a + s * b;
    let rect = |a0: f32, a1: f32, b0: f32, b1: f32| vec![at(a0, b0), at(a1, b0), at(a1, b1), at(a0, b1)];
    // A point may stand past the lot line only on the water side.
    let ok = |poly: &[Vec2]| {
        poly.iter().all(|p| {
            let inside = c.inside_local(*p) && distance_to_boundary(*p, &c.lot_poly) >= 0.6;
            inside || (over > 0.0 && (*p - core).dot(v) > reach * 0.6 && distance_to_boundary(*p, &c.lot_poly) <= over)
        }) && (0..poly.len()).all(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            (1..6).all(|j| {
                let q = a.lerp(b, j as f32 / 6.0);
                c.inside_local(q) || (over > 0.0 && (q - core).dot(v) > reach * 0.6 && distance_to_boundary(q, &c.lot_poly) <= over)
            })
        })
    };
    // The trays, bottom up: (from, to along the view; half width; sideways shift).
    let jog = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let wants: [(f32, f32, f32, f32); 4] = [
        (-3.0, reach - 0.8 + over, (7.5 * k).min(side_room - 1.0), 0.0),
        (reach * 0.1, reach * 0.5, (7.5 * k * 1.5).min(side_room - 0.8), jog * 1.4 * k),
        (-2.0, reach * 0.42, (5.0 * k).min(side_room), -jog * 1.2),
        (-2.5, reach * 0.25, 4.2 * k, jog * 0.8),
    ];
    let mut trays: Vec<(Vec<Vec2>, f32, f32, f32, f32)> = Vec::new();
    for (i, &(a0, mut a1, mut hb, sh)) in wants.iter().take(levels).enumerate() {
        let mut poly = rect(a0, a1, sh - hb, sh + hb);
        for _ in 0..60 {
            if ok(&poly) || a1 - a0 < 4.0 || hb < 2.0 {
                break;
            }
            // Narrower where the sides are what stick out, else shorter.
            if ok(&rect(a0, a1, sh - hb * 0.8, sh + hb * 0.8)) || a1 - a0 < 6.0 {
                hb *= 0.96;
            } else {
                a1 -= 0.6;
            }
            poly = rect(a0, a1, sh - hb, sh + hb);
        }
        if !ok(&poly) || a1 - a0 < 4.0 || hb < 2.0 {
            continue;
        }
        trays.push((poly, level_y(i), a1, hb, sh));
    }
    if trays.is_empty() {
        return ledge_house_small(g, c, rng, plan);
    }
    let top = trays.last().map_or(level_y(0), |t| t.1) + 1.0;
    // Glazed rooms under each tray, set back behind a deep terrace.
    for (i, &(_, y, a1, hb, sh)) in trays.iter().enumerate() {
        let (y0, y1) = if i == 0 { (0.0, y - 0.35) } else { (trays[i - 1].1 + 1.0, y - 0.35) };
        let a_end = if i == 0 { (a1 * 0.4).min(7.0 * k) } else { a1 - 3.2 };
        let a_start = if i == 0 { -2.0 } else { (trays[i].0[0] - core).dot(v).max(-2.0) + 0.5 };
        let room = rect(a_start, a_end.max(a_start + 1.0), sh - hb + 1.2, sh + hb - 1.2);
        if a_end > a_start + 1.0 && hb > 2.4 && sits(c, &room, 0.6) {
            extrude(g, f, Glass, &room, y0, y1, pal.glass_dark);
            let mut glow: Vec<(Face, f32, f32, f32, f32)> = Vec::new();
            for face in c.faces(&room) {
                ribbon(g, c, rng, &face, y0 + 0.15, (y1 - y0) - 0.3, 1.4, &[LAMP, LAMP_SOFT], SASH_RED);
                let n = (face.len / 1.4).floor() as i32;
                for j in 0..n {
                    if rng.random_bool(0.5) {
                        let t = -face.len * 0.5 + (j as f32 + 0.5) * face.len / n as f32;
                        glow.push((face, t, (y0 + y1) * 0.5, face.len / n as f32 * 0.8, (y1 - y0) * 0.7));
                    }
                }
            }
            night_lights(g, c, &glow, 0.095, mul(LAMP, 0.7));
        }
    }
    // The trays: thick pale lips, a warm line of light under the long
    // ones' fronts.
    for (i, (poly, y, a1, hb, sh)) in trays.iter().enumerate() {
        tray(g, c, poly, *y, mul(TRAY, 1.0 - 0.04 * i as f32));
        if i < 2 {
            let sh = *sh;
            let (p, q) = (at(*a1 - 0.2, sh - hb + 0.3), at(*a1 - 0.2, sh + hb - 0.3));
            g.part(Glow).tube(w3(c, p, y - 0.45), w3(c, q, y - 0.45), 0.06, 4, rgba(AMBER_LINE));
        }
    }
    // Stone piers carrying the lowest tray at its root, and over a drop
    // the rock it stands on down to the falling ground.
    let (y0, a10, hb0) = (trays[0].1, trays[0].2, trays[0].3);
    for b in [-1.0f32, 1.0] {
        let p = at((a10 * 0.32).min(6.0), b * (hb0 - 1.6));
        if fits(c, p, 0.9, 0.2) {
            g.part(Stone).boxed(w3(c, p, (y0 - 0.35) * 0.5), Vec3::new(1.5, y0 - 0.35, 1.5), f.yaw() + v.to_angle(), rgba(SANDSTONE));
        }
    }
    if site.real && site.fall > 0.6 {
        for b in [-1.0f32, 1.0] {
            let p = at(reach - 2.4, b * (hb0 * 0.5));
            if fits(c, p, 1.0, 0.3) {
                let h = site.fall + y0 - 0.35;
                g.part(Stone).boxed(w3(c, p, -site.fall + h * 0.5), Vec3::new(2.0, h, 2.4), f.yaw() + v.to_angle(), rgba(mul(SANDSTONE, 0.9)));
            }
        }
    }
    // The hearth: a tower of rough stone through every level, its cap of
    // tray concrete; a slot of light up the stair.
    let core_poly = rect(-3.9, -0.4, -2.0, 2.0);
    if sits(c, &core_poly, 0.4) {
        extrude(g, f, Stone, &core_poly, 0.0, top + 4.6, SANDSTONE);
        extrude(g, f, Concrete, &offset_poly(&core_poly, -0.35), top + 4.6, top + 5.0, TRAY);
        let back_face = Face::new(at(-3.9, -2.0), at(-3.9, 2.0), -v);
        back_face.decal(g, f, Glow, 0.0, top * 0.5 + 1.0, 0.04, 0.5, top - 1.0, mul(LAMP, 0.8));
        // A stone fin wall beside it, a level higher than the trays.
        let fin = rect(-3.0, reach * 0.3, 2.6, 3.3);
        if sits(c, &fin, 0.5) {
            extrude(g, f, Stone, &fin, 0.0, top - 0.6, mul(SANDSTONE, 0.95));
        }
    }
    // Under the cantilever: a rock pool and a cascade of its own, unless
    // it reaches out over real water.
    let mut taken: Vec<(Vec<Vec2>, f32)> = trays.iter().map(|t| (t.0.clone(), 2.5)).collect();
    taken.push((core_poly.clone(), 2.0));
    if !site.over_water() {
        let pc = at(reach * 0.62, 0.0);
        let r = (4.5 * k).min(distance_to_boundary(pc, &c.lot_poly) - 1.0).min(reach * 0.5);
        if r > 2.0 && c.inside_local(pc) {
            let blob: Vec<Vec2> = (0..12).map(|i| { let a = i as f32 / 12.0 * TAU; pc + (s * a.cos() * 1.2 + v * a.sin() * 0.8) * r * rng.random_range(0.8..1.0) }).collect();
            let blob = ccw(blob);
            ground(g, f, Matte, &blob, 0.06, WATER);
            neon_line(g, c, &blob, 0.07, true, mul(CYAN_LINE, 0.45));
            for (i, q) in blob.iter().enumerate() {
                if i % 2 == 0 {
                    let r = rng.random_range(0.5..1.1);
                    rock_world(g, rng, wv(c, *q), 0.0, r, mul(SANDSTONE, 0.8));
                }
            }
            // The cascade: stone ledges stepping down to the pool, a sheet
            // of falling water over each.
            for j in 0..3 {
                let q = pc - v * (r + 0.6 + j as f32 * 1.0);
                if fits(c, q, 1.6, 0.3) {
                    let h = 0.4 + j as f32 * 0.45;
                    g.part(Stone).boxed(w3(c, q, h * 0.5), Vec3::new(3.2, h, 1.0), f.yaw() + v.to_angle() + FRAC_PI_2, rgba(SANDSTONE));
                    let lip = Face::new(at((q - core).dot(v) + 0.51, -1.4), at((q - core).dot(v) + 0.51, 1.4), v);
                    lip.decal(g, f, Matte, 0.0, h * 0.5, 0.0, 2.6, h * 0.92, [0.62, 0.72, 0.74]);
                }
            }
            taken.push((blob, 1.5));
        }
    }
    // The way in: a flagged path from the street under a concrete trellis.
    let door = at(-3.6, 0.0) - v * 0.6;
    let gate = front_door_path(c, door, 1.0);
    walk(g, c, gate, door, 2.2, mul(CREAM_STONE, 0.9));
    let path_poly = vec![gate - (door - gate).perp().normalize_or_zero() * 1.5, gate + (door - gate).perp().normalize_or_zero() * 1.5, door + (door - gate).perp().normalize_or_zero() * 1.5, door - (door - gate).perp().normalize_or_zero() * 1.5];
    taken.push((ccw(path_poly), 1.0));
    let len = gate.distance(door);
    if len > 4.0 {
        let dir = (door - gate) / len;
        let tlen = len.min(12.0);
        g.lod(Lod::Mid, |g| {
            let mut d = len - tlen;
            while d < len {
                let p = gate + dir * d;
                if fits(c, p, 1.6, 0.0) {
                    g.part(Concrete).boxed(w3(c, p, 3.05), Vec3::new(3.4, 0.22, 0.24), f.yaw() + dir.to_angle() + FRAC_PI_2, rgba(TRAY));
                }
                d += 1.1;
            }
            for b in [-1.5f32, 1.5] {
                let (p, q) = (gate + dir * (len - tlen) + dir.perp() * b, door + dir.perp() * b);
                if fits(c, p, 0.2, 0.0) && fits(c, q, 0.2, 0.0) {
                    g.part(Concrete).beam(w3(c, p, 2.82), w3(c, q, 2.82), 0.24, rgba(TRAY));
                }
            }
        });
        for j in 0..3 {
            let p = gate + dir * (len * (j as f32 + 0.5) / 3.0) + dir.perp() * 1.8;
            bollard_light(g, c, p);
        }
    }
    woods(g, c, rng, (28.0 * k * k) as usize, 0.45, false, &taken, 1.0 + 0.2 * (k - 1.0));
}

/// One tray: a floor slab of pale concrete with a parapet round it, the
/// deck inside it open to the sky.
fn tray(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, col: [f32; 3]) {
    let f = &c.frame;
    extrude(g, f, Concrete, poly, y - 0.35, y, col);
    let inner = offset_poly(poly, 0.32);
    if inner.len() != poly.len() {
        extrude(g, f, Concrete, poly, y, y + 0.9, col);
        return;
    }
    let outer3 = f.poly3(poly, y);
    let outer_top = f.poly3(poly, y + 0.9);
    g.part(Concrete).band(&outer3, &outer_top, rgba(col));
    let mid = poly_center(poly);
    let n = poly.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let inward = f.dir3((mid - (inner[i] + inner[j]) * 0.5).normalize_or_zero());
        g.part(Concrete).quad_facing([w3(c, inner[i], y), w3(c, inner[j], y), w3(c, inner[j], y + 0.9), w3(c, inner[i], y + 0.9)], inward, rgba(mul(col, 0.92)));
        g.part(Concrete).quad_facing([w3(c, poly[i], y + 0.9), w3(c, poly[j], y + 0.9), w3(c, inner[j], y + 0.9), w3(c, inner[i], y + 0.9)], Vec3::Y, rgba(mul(col, 1.04)));
    }
    ground(g, f, Paving, &inner, y + 0.02, mul(col, 0.82));
}

/// The ledge house on a site too shallow for its trays to reach: two
/// crossing trays over a stone core, on the plan's long axis.
fn ledge_house_small(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, plan: Plan) {
    let f = &c.frame;
    let hl = plan.hl.min(12.0);
    let hw = plan.hw.min(6.0);
    let low = plan.quad(-hl, hl, -hw * 0.8, hw * 0.8);
    let room = plan.quad(-hl * 0.6, hl * 0.4, -hw * 0.6, hw * 0.6);
    extrude(g, f, Glass, &room, 0.0, 2.55, c.palette.glass_dark);
    for face in c.faces(&room) {
        ribbon(g, c, rng, &face, 0.15, 2.3, 1.4, &[LAMP, LAMP_SOFT], SASH_RED);
    }
    extrude(g, f, Concrete, &low, 2.55, 3.9, TRAY);
    let cross = plan.quad(-hl * 0.35, hl * 0.25, -hw, hw);
    extrude(g, f, Concrete, &cross, 5.6, 6.9, mul(TRAY, 0.96));
    let up = plan.quad(-hl * 0.3, hl * 0.2, -hw * 0.6, hw * 0.6);
    extrude(g, f, Glass, &up, 3.9, 5.6, c.palette.glass_dark);
    let core = plan.quad(-hl * 0.85, -hl * 0.6, -1.5, 1.5);
    extrude(g, f, Stone, &core, 0.0, 9.5, SANDSTONE);
}

// ── the prairie hall (after Wright) ──────────────────────────────────────────

/// Long low wings of roman brick on a stone water table, a ribbon of art
/// glass under deep hipped eaves, a raised clerestory over the middle and a
/// broad chimney; a cross wing toward the deeper side, terraces with stone
/// urns off the ends, planters along the front.
pub fn prairie_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = Site::of(c, rng, 2000.0);
    let k = site.k;
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.7));
    let Some(p) = best_plan(c, 1.0, 6.0) else { return };
    let eave = 2.8f32.min(p.hw * 0.25 + 0.8);
    let main = Plan { c: p.c, ax: p.ax, hl: (p.hl - eave - 0.4).min(34.0 * k), hw: (p.hw - eave - 0.4).clamp(2.5, 7.0) };
    if main.hl < 4.0 {
        return;
    }
    let roof_col = if rng.random_bool(0.7) { COPPER_GREEN } else { [0.33, 0.22, 0.17] };
    let y_eave = 4.3;
    let mut solids: Vec<(Vec<Vec2>, f32)> = Vec::new();
    prairie_wing(g, c, rng, main, y_eave, eave, roof_col);
    solids.push((main.grown(eave).poly(), 2.0));
    // The cross wing, toward whichever side has more room.
    let off = main.hl * [0.0, 0.32, -0.32][rng.random_range(0..3)];
    let root = main.at(off, 0.0);
    let (up, dn) = (ray(c, root, main.across()), ray(c, root, -main.across()));
    let side = if up >= dn { 1.0 } else { -1.0 };
    let room = up.max(dn) - main.hw - eave - 0.6;
    let reach = room.min(14.0 * k);
    if reach > 3.5 {
        let hw2 = (main.hw * 0.72).max(2.5);
        let mut len = reach;
        let mut cross = Plan { c: main.at(off, side * (main.hw + len) * 0.5), ax: main.across(), hl: (main.hw + len) * 0.5, hw: hw2 };
        for _ in 0..20 {
            if sits(c, &cross.grown(eave).poly(), 0.3) || len < 3.0 {
                break;
            }
            len -= 0.8;
            cross = Plan { c: main.at(off, side * (main.hw + len) * 0.5), ax: main.across(), hl: (main.hw + len) * 0.5, hw: hw2 };
        }
        if len >= 3.0 && sits(c, &cross.grown(eave).poly(), 0.3) {
            prairie_wing(g, c, rng, cross, y_eave - 0.6, eave * 0.9, roof_col);
            solids.push((cross.grown(eave).poly(), 2.0));
        }
    }
    // The upper storey over the middle, under its own eaves: the hall's
    // roofs step up in two tiers.
    let upper = Plan { c: main.c, ax: main.ax, hl: (main.hl * 0.36).max(4.0), hw: main.hw * 0.86 };
    // Its sill clears the low roof's ridge.
    let ridge = y_eave + (main.hw + eave) * 0.22;
    let ys = ridge - 0.6;
    let y1 = ys + 2.9;
    let poly = upper.poly();
    band(g, f, Brick, &poly, &poly, y_eave - 0.5, ys, ROMAN_BRICK);
    extrude(g, f, Stone, &upper.grown(0.16).poly(), ys, ys + 0.14, CREAM_STONE);
    let glass = upper.grown(-0.1).poly();
    band(g, f, Glass, &glass, &glass, ys + 0.14, ys + 1.9, c.palette.glass_dark);
    for face in c.faces(&glass) {
        ribbon(g, c, rng, &face, ys + 0.14, 1.76, 1.2, &[ART_AMBER, ART_GREEN, LAMP], DARK_TIMBER);
    }
    extrude(g, f, Stone, &upper.grown(0.16).poly(), ys + 1.9, ys + 2.04, CREAM_STONE);
    band(g, f, Plaster, &poly, &poly, ys + 2.04, y1 - 0.3, OCHRE_RENDER);
    let rise1 = (upper.hw + eave) * 0.22;
    hip_roof(g, c, Metal, upper.grown(eave), y1, rise1, roof_col, DARK_TIMBER, mul(OCHRE_RENDER, 0.8));
    for q in upper.grown(eave - 0.4).poly() {
        g.part(Glow).boxed(w3(c, q, y1 - 0.42), Vec3::splat(0.22), 0.0, rgba(LAMP));
    }
    // The chimney: a broad flat mass of brick through both roofs.
    let ch = Plan { c: main.at(upper.hl * 0.55 * if rng.random_bool(0.5) { 1.0 } else { -1.0 }, 0.0), ax: main.ax, hl: 1.3, hw: 2.9 };
    let ch_top = y1 + rise1 + 1.4;
    extrude(g, f, Brick, &ch.poly(), 0.0, ch_top, mul(ROMAN_BRICK, 0.92));
    extrude(g, f, Stone, &ch.grown(0.25).poly(), ch_top, ch_top + 0.35, CREAM_STONE);
    // Terraces off the ends: low walls round paving, an urn at each corner.
    for e in [-1.0f32, 1.0] {
        let depth = ray(c, main.at(e * main.hl, 0.0), main.ax * e) - 1.2;
        let d = depth.min(7.0);
        if d < 2.5 {
            continue;
        }
        let a0 = e * (main.hl + eave * 0.2);
        let a1 = e * (main.hl + d);
        let terr = ccw(main.quad(a0.min(a1), a0.max(a1), -main.hw - 0.8, main.hw + 0.8));
        if !sits(c, &terr, 0.3) {
            continue;
        }
        ground(g, f, Paving, &terr, 0.06, mul(CREAM_STONE, 0.85));
        solids.push((terr.clone(), 1.0));
        for b in [-1.0f32, 1.0] {
            let w0 = main.at(a0, b * (main.hw + 0.8));
            let w1 = main.at(a1, b * (main.hw + 0.8));
            g.part(Brick).strip_box(wv(c, w0), wv(c, w1), 0.0, 0.45, 0.0, 0.85, rgba(ROMAN_BRICK));
            g.lod(Lod::Mid, |g| g.part(Stone).strip_box(wv(c, w0), wv(c, w1), 0.0, 0.6, 0.85, 0.95, rgba(CREAM_STONE)));
            urn(g, c, w1);
        }
    }
    // Long planters along the front of the main wing.
    let front_b: f32 = if main.across().dot(Vec2::NEG_Y) > 0.0 { 1.0 } else { -1.0 };
    let pl0 = main.at(-main.hl * 0.8, front_b * (main.hw + eave + 1.0));
    let pl1 = main.at(main.hl * 0.8, front_b * (main.hw + eave + 1.0));
    if fits(c, pl0, 0.8, 0.2) && fits(c, pl1, 0.8, 0.2) {
        g.part(Brick).strip_box(wv(c, pl0), wv(c, pl1), 0.0, 1.2, 0.0, 0.6, rgba(ROMAN_BRICK));
        g.lod(Lod::Mid, |g| g.part(Foliage).strip_box(wv(c, pl0), wv(c, pl1), 0.0, 1.0, 0.6, 1.05, rgba(mul(pal.foliage, 0.85))));
    }
    // The way in from the street, lamps on piers.
    let door = main.at(0.0, front_b * (main.hw + 0.2));
    let gate = front_door_path(c, door, 0.8);
    walk(g, c, gate, door, 2.6, mul(CREAM_STONE, 0.9));
    for j in 0..2 {
        let q = gate.lerp(door, 0.25 + 0.5 * j as f32) + (door - gate).perp().normalize_or_zero() * 2.0;
        bollard_light(g, c, q);
    }
    woods(g, c, rng, (16.0 * k * k) as usize, 0.2, false, &solids, 1.0);
}

/// One prairie wing: the water table, brick to the sill, the art-glass
/// ribbon between brick piers, a frieze and the hipped roof `eave` out.
#[allow(clippy::too_many_arguments)]
fn prairie_wing(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, w: Plan, y_eave: f32, eave: f32, roof: [f32; 3]) {
    let f = &c.frame;
    let poly = w.poly();
    extrude(g, f, Stone, &w.grown(0.25).poly(), 0.0, 0.45, CREAM_STONE);
    band(g, f, Brick, &poly, &poly, 0.45, 1.35, ROMAN_BRICK);
    extrude(g, f, Stone, &w.grown(0.18).poly(), 1.35, 1.5, CREAM_STONE);
    let glass = w.grown(-0.12).poly();
    band(g, f, Glass, &glass, &glass, 1.5, 3.3, c.palette.glass_dark);
    for face in c.faces(&glass) {
        ribbon(g, c, rng, &face, 1.5, 1.8, 1.25, &[ART_AMBER, ART_GREEN, LAMP], DARK_TIMBER);
        // Brick piers every few panes, proud of the glass.
        let n = (face.len / 5.0).floor() as i32;
        for j in 0..=n {
            let t = -face.len * 0.5 + 0.35 + j as f32 * (face.len - 0.7) / n.max(1) as f32;
            face.bx(g, f, Brick, t, 2.4, 0.18, Vec3::new(0.7, 1.8, 0.62), mul(ROMAN_BRICK, 0.95));
        }
    }
    extrude(g, f, Stone, &w.grown(0.18).poly(), 3.3, 3.45, CREAM_STONE);
    band(g, f, Plaster, &poly, &poly, 3.45, y_eave - 0.3, OCHRE_RENDER);
    hip_roof(g, c, Metal, w.grown(eave), y_eave, (w.hw + eave) * 0.22, roof, DARK_TIMBER, mul(OCHRE_RENDER, 0.85));
    // Lights tucked under the eaves at the corners.
    for q in w.grown(eave - 0.5).poly() {
        g.part(Glow).boxed(w3(c, q, y_eave - 0.5), Vec3::new(0.4, 0.3, 0.4), 0.0, rgba(LAMP));
    }
}

/// A wide shallow stone urn on a pedestal, planted.
fn urn(g: &mut Geometry, c: &Ctx, p: Vec2) {
    if !fits(c, p, 0.8, 0.0) {
        return;
    }
    let w = wv(c, p);
    g.part(Stone).lathe(w, &[(0.3, 0.0), (0.25, 0.7), (0.9, 1.0), (1.0, 1.25)], 10, rgba(CREAM_STONE));
    g.lod(Lod::Mid, |g| g.part(Foliage).cylinder(w, 0.85, 0.5, 1.2, 1.7, 8, rgba(mul(c.palette.foliage, 0.9))));
}

// ── the hill chapel (after Le Corbusier) ─────────────────────────────────────

/// Thick curved white walls under a dark concrete roof that sails up to
/// the view, the light slipping in under it; one wall pierced by deep
/// slots of coloured glass, a round tower with a hood, an enamelled door,
/// a pulpit outside for the open-air services on the meadow.
pub fn hill_chapel(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = Site::of(c, rng, 1400.0);
    let k = site.k.min(1.4);
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.75));
    let Some(p) = best_plan(c, 4.0, 2.0) else { return };
    let hl = p.hl.min(13.0 * k).max(5.0);
    let hw = p.hw.min(8.5 * k).max(4.0);
    let plan = Plan { hl, hw, ..p };
    let corners = plan.poly();
    // Each wall bows: out, in, out a little, round.
    let mut bows = [2.6f32, -2.0, 0.9, 3.4];
    let spin = rng.random_range(0..4);
    bows.rotate_left(spin);
    for b in &mut bows {
        *b *= rng.random_range(0.75..1.2) * k.min(1.2);
    }
    // The wall facing the street is the thick one with the slots.
    let thick = (0..4).max_by(|&a, &b| {
        let o = |i: usize| Face::from_edge(corners[i], corners[(i + 1) % 4]).out.dot(Vec2::NEG_Y);
        o(a).total_cmp(&o(b))
    }).unwrap_or(0);
    let m = 8usize;
    // The bowed walls stay on the lot.
    let inside = offset_poly(&c.lot_poly, 0.8);
    let mut ring: Vec<Vec2> = Vec::new();
    let mut wall_of: Vec<usize> = Vec::new();
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let out = Face::from_edge(a, b).out;
        for j in 0..m {
            let t = j as f32 / m as f32;
            ring.push(clamp_into(&inside, a.lerp(b, t) + out * bows[i] * (t * PI).sin()));
            wall_of.push(i);
        }
    }
    let centre = plan.c;
    // The prow: the corner nearest the view; the roof sails up to it.
    let prow = corners.iter().copied().max_by(|a, b| (*a - centre).dot(site.view).total_cmp(&(*b - centre).dot(site.view))).unwrap_or(corners[0]);
    let pdir = (prow - centre).normalize_or(Vec2::Y);
    let half = (prow - centre).length().max(1.0);
    let lift = |q: Vec2| {
        let t = (((q - centre).dot(pdir) / half) * 0.5 + 0.5).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let wall_h = |q: Vec2| 4.4 + 7.4 * lift(q) * k.min(1.25);
    let bottom: Vec<Vec3> = ring.iter().map(|q| w3(c, *q, 0.0)).collect();
    let top: Vec<Vec3> = ring.iter().map(|q| w3(c, *q, wall_h(*q))).collect();
    g.part(Matte).band(&bottom, &top, rgba(WHITE));
    // The thick wall's inner face and its top, splayed slots through it.
    let thick_pts: Vec<usize> = (0..ring.len()).filter(|&i| wall_of[i] == thick).collect();
    for &i in &thick_pts {
        let j = (i + 1) % ring.len();
        let (a, b) = (ring[i], ring[j]);
        let (ha, hb) = (wall_h(a), wall_h(b));
        let seg = Face::from_edge(a, b);
        if seg.len < 0.8 {
            continue;
        }
        // Slots: a few per stretch, of every size, never overlapping.
        let mut used: Vec<(f32, f32, f32, f32)> = Vec::new();
        for _ in 0..9 {
            let sw = rng.random_range(0.45..1.4) * if rng.random_bool(0.2) { 1.7 } else { 1.0 };
            let shh = rng.random_range(0.8..2.6);
            let hmin = ha.min(hb);
            if seg.len < sw + 0.6 || hmin < shh + 1.6 {
                continue;
            }
            let t = rng.random_range(-(seg.len - sw) * 0.5 + 0.3..(seg.len - sw) * 0.5 - 0.3 + 1e-3);
            let y = rng.random_range(0.9 + shh * 0.5..hmin - 0.9 - shh * 0.5 + 1e-3);
            if used.iter().any(|(t2, y2, w2, h2)| (t - t2).abs() < (sw + w2) * 0.5 + 0.35 && (y - y2).abs() < (shh + h2) * 0.5 + 0.35) {
                continue;
            }
            used.push((t, y, sw, shh));
            // The splayed reveal, pale; the coloured pane deep in it.
            seg.decal(g, f, Plaster, t, y, 0.03, sw + 0.6, shh + 0.6, mul(WHITE, 0.62));
            let col = SLOT_GLASS[rng.random_range(0..SLOT_GLASS.len())];
            seg.decal(g, f, Glow, t, y, 0.055, sw, shh, mul(col, 1.3));
        }
    }
    // The other walls: a few small slots, a long one under the roof.
    for (i, j) in (0..ring.len()).map(|i| (i, (i + 1) % ring.len())) {
        if wall_of[i] == thick || i % 2 != 0 {
            continue;
        }
        let seg = Face::from_edge(ring[i], ring[j]);
        let h = wall_h(ring[i]).min(wall_h(ring[j]));
        if seg.len > 1.4 && h > 3.4 && rng.random_bool(0.7) {
            let col = SLOT_GLASS[rng.random_range(0..SLOT_GLASS.len())];
            let (sw, shh) = (rng.random_range(0.35..0.8), rng.random_range(0.9..2.0));
            let y = rng.random_range(1.4..h - 1.0 - shh * 0.5);
            seg.decal(g, f, Plaster, 0.0, y, 0.03, sw + 0.5, shh + 0.5, mul(WHITE, 0.62));
            seg.decal(g, f, Glow, 0.0, y, 0.055, sw, shh, mul(col, 1.2));
        }
    }
    // The light under the roof: a warm line in the gap all round.
    let gap: Vec<Vec3> = ring.iter().map(|q| w3(c, *q, wall_h(*q) + 0.32)).collect();
    g.part(Glow).band(&top, &gap, rgba(mul(LAMP, 0.85)));
    // The roof: a thick dark shell, its eaves out over the walls and
    // curled up toward the prow; its top sags in the middle.
    let keep = offset_poly(&c.lot_poly, 0.4);
    let under: Vec<(Vec2, f32)> = ring
        .iter()
        .map(|q| {
            let o = 0.7 + 2.6 * lift(*q);
            let out = (*q - centre).normalize_or_zero();
            (clamp_into(&keep, *q + out * o), wall_h(*q) + 0.32)
        })
        .collect();
    let thick_at = |q: Vec2| 0.9 + 2.4 * lift(q);
    let u3: Vec<Vec3> = under.iter().map(|(q, y)| w3(c, *q, *y)).collect();
    let t3: Vec<Vec3> = under.iter().map(|(q, y)| w3(c, *q, *y + thick_at(*q))).collect();
    g.part(Concrete).band(&u3, &t3, rgba(mul(BETON_DARK, 0.8)));
    let mean_u = under.iter().map(|(_, y)| *y).sum::<f32>() / under.len() as f32;
    fan3(g, Matte, w3(c, centre, mean_u + 0.1), &t3, true, true, [0.30, 0.29, 0.27]);
    fan3(g, Concrete, w3(c, centre, mean_u - 0.6), &u3, true, false, mul(BETON, 0.7));
    // Gutter spout at the prow, a stone basin under it.
    let spout = prow + pdir * 1.4;
    if fits(c, spout, 1.4, 0.2) {
        let y = wall_h(prow) + 1.5;
        g.part(Concrete).tube(w3(c, prow, y), w3(c, spout, y - 0.2), 0.35, 6, rgba(BETON_DARK));
        g.part(Concrete).cylinder_capped(wv(c, spout + pdir * 0.4), 1.2, 1.1, 0.0, 0.7, 10, false, true, rgba(BETON));
    }
    // The round tower at the low corner, its hood turned away from the view.
    let low = corners.iter().copied().min_by(|a, b| (*a - centre).dot(pdir).total_cmp(&(*b - centre).dot(pdir))).unwrap_or(corners[2]);
    let mut towers = vec![(low + (centre - low).normalize_or_zero() * 1.2, 2.5 * k.min(1.2), 15.0 + 3.5 * k)];
    if k > 1.1 || rng.random_bool(0.4) {
        let other = corners.iter().copied().filter(|q| *q != low && *q != prow).min_by(|a, b| (*a - centre).dot(pdir).total_cmp(&(*b - centre).dot(pdir))).unwrap_or(low);
        towers.push((other + (centre - other).normalize_or_zero() * 1.0, 1.7, 10.5));
    }
    for (tp, r, h) in towers {
        if !fits(c, tp, r * 1.2, 0.3) {
            continue;
        }
        let tw = wv(c, tp);
        g.part(Matte).cylinder_capped(tw, r, r * 0.97, 0.0, h, 20, false, false, rgba(WHITE));
        // The hood: the crown slants up toward its back.
        let face_ang = f.dir3(-pdir);
        let face2 = Vec2::new(face_ang.x, face_ang.z);
        let ring_lo: Vec<Vec3> = MeshData::ngon(tw, r * 0.97, 20, 0.0, h);
        let ring_hi: Vec<Vec3> = ring_lo.iter().map(|q| {
            let d = Vec2::new(q.x - tw.x, q.z - tw.y).normalize_or_zero();
            Vec3::new(q.x, h + 0.3 + 3.2 * (0.5 - 0.5 * d.dot(face2)), q.z)
        }).collect();
        g.part(Matte).band(&ring_lo, &ring_hi, rgba(WHITE));
        // A rounded hood over the slanted crown, leaning to its back.
        let hood = Vec2::new(tw.x - face2.x * r * 0.25, tw.y - face2.y * r * 0.25);
        let top = ring_hi.iter().map(|q| q.y).fold(f32::MIN, f32::max);
        let prof: Vec<(f32, f32)> = (0..=6).map(|j| { let a = j as f32 / 6.0 * FRAC_PI_2 * 0.92; (r * 0.86 * a.cos(), top - 1.2 + a.sin() * r * 0.9) }).collect();
        g.part(Matte).lathe(hood, &prof, 16, rgba(mul(WHITE, 0.96)));
        // The light falls in through the hood's mouth.
        let mouth = Vec3::new(tw.x + face2.x * (r * 0.98), h - 0.9, tw.y + face2.y * (r * 0.98));
        let along = Vec3::new(-face2.y, 0.0, face2.x);
        let out = Vec3::new(face2.x, 0.0, face2.y);
        g.part(Matte).decal(mouth + out * 0.03, along, Vec3::Y, r * 1.1, 2.6, out, rgba(mul(BETON_DARK, 0.6)));
        g.part(Glow).decal(mouth + out * 0.06, along, Vec3::Y, r * 0.7, 1.8, out, rgba(mul(LAMP, 0.8)));
    }
    // The enamelled door in the thick wall, near its low end.
    let ti = thick_pts[if (ring[thick_pts[1]] - centre).dot(pdir) < (ring[thick_pts[m - 2]] - centre).dot(pdir) { 1 } else { m - 2 }];
    let seg = Face::from_edge(ring[ti], ring[(ti + 1) % ring.len()]);
    if seg.len > 2.6 {
        let cols = [LOGGIA[0], LOGGIA[1], LOGGIA[2], LOGGIA[3], [0.9, 0.88, 0.82], LOGGIA[5]];
        for (q, col) in cols.iter().enumerate() {
            let (x, y) = ((q % 2) as f32, (q / 2) as f32);
            seg.decal(g, f, Matte, -0.53 + x * 1.06, 0.55 + y * 1.0, 0.045, 1.0, 0.94, *col);
        }
        let out = seg.at(0.0, 3.5);
        let gate = front_door_path(c, out, 0.8);
        walk(g, c, gate, seg.at(0.0, 0.6), 2.4, pal.gravel);
    }
    // The pulpit and altar outside under the prow, benches on the meadow.
    let pul = prow + pdir * 0.2 - (prow - centre).perp().normalize_or_zero() * 2.0;
    if fits(c, pul, 1.0, 0.3) {
        g.part(Concrete).boxed(w3(c, pul, 0.75), Vec3::new(1.6, 1.5, 1.2), f.yaw() + pdir.to_angle(), rgba(BETON));
    }
    let meadow = prow + pdir * 8.0;
    let mut taken: Vec<(Vec<Vec2>, f32)> = vec![(ccw(ring.clone()), 3.0)];
    if c.inside_local(meadow) {
        g.lod(Lod::Mid, |g| {
            for row in 0..4 {
                for col in -2..=2 {
                    let q = meadow + pdir * (row as f32 * 1.6) + pdir.perp() * (col as f32 * 2.2);
                    if fits(c, q, 1.2, 0.3) {
                        bench_world(g, wv(c, q), f.yaw() + pdir.perp().to_angle(), pal.wood, pal.steel);
                    }
                }
            }
        });
        taken.push((ccw(vec![meadow - pdir.perp() * 6.0, meadow + pdir.perp() * 6.0, meadow + pdir.perp() * 6.0 + pdir * 6.0, meadow - pdir.perp() * 6.0 + pdir * 6.0]), 1.0));
    }
    // The pyramid of stones from the old chapel, at a corner of the meadow.
    if let Some(q) = (0..8).map(|i| centre + Vec2::from_angle(i as f32 * 0.785 + 0.3) * (half + 7.0)).find(|q| fits(c, *q, 2.4, 0.5)) {
        for s in 0..4 {
            let w = 3.6 - s as f32 * 0.85;
            g.part(Stone).boxed(w3(c, q, 0.3 + s as f32 * 0.6), Vec3::new(w, 0.6, w), f.yaw() + 0.4, rgba(mul(SANDSTONE, 1.05)));
        }
        taken.push((vec![q - Vec2::splat(2.5), q + Vec2::new(2.5, -2.5), q + Vec2::splat(2.5), q + Vec2::new(-2.5, 2.5)], 1.0));
    }
    // Lamps round the walls, so the white catches the light after dark.
    for (i, q) in ring.iter().enumerate() {
        if i % 6 == 3 {
            let out = (*q - centre).normalize_or_zero();
            lamp_post_in(g, c, *q + out * 4.5, 4.5, pal.street_lamp);
        }
    }
    woods(g, c, rng, (10.0 * k * k) as usize, 0.5, false, &taken, 1.0);
}

// ── the sky slab (after Le Corbusier) ────────────────────────────────────────

/// A long housing slab along the site's long side, raised on pairs of
/// tapering pillars: coloured loggias in a deep concrete sun-screen, a
/// shopping street of louvres half way up, blank ends with a stair tower,
/// a roof terrace with a funnel stack, a gym vault, a track and a pool.
pub fn sky_slab(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = Site::of(c, rng, 2400.0);
    let k = site.k;
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.72));
    let Some(p) = best_plan(c, 2.5, 8.0) else { return };
    let bay = 4.2;
    if p.hw < 4.0 || p.hl < bay * 1.6 {
        // Too small a site for the slab on its pillars: a stub of it.
        let poly = p.poly();
        extrude(g, f, Concrete, &poly, 0.0, 9.0, BETON);
        for face in c.faces(&poly) {
            ribbon(g, c, rng, &face, 3.5, 1.6, 1.4, &[LAMP], DARK);
        }
        return;
    }
    let hw = p.hw.min(10.0);
    let bays = ((p.hl * 2.0 - 2.0) / bay).floor().clamp(3.0, 30.0) as i32;
    let hl = bays as f32 * bay * 0.5;
    let slab = Plan { c: p.c, ax: p.ax, hl, hw };
    let floors = (c.floors as i32).clamp(6, 16).max((8.0 + 4.0 * (k - 1.0)).round() as i32).min(16);
    let fh = 3.0;
    let ph = 7.0;
    let y_top = ph + floors as f32 * fh;
    let poly = slab.poly();
    // The artificial ground: a deep transfer slab over the pillars.
    extrude(g, f, Concrete, &poly, ph - 1.2, ph, BETON_DARK);
    // The pillars: pairs of tapering piers, wide at the top.
    let pairs = (bays / 2).max(2);
    for i in 0..=pairs {
        let a = -hl + 1.2 + (2.0 * hl - 2.4) * i as f32 / pairs as f32;
        for b in [-1.0f32, 1.0] {
            let at = slab.at(a, b * hw * 0.5);
            let foot = rect_yaw(0.0, 0.0, 0.45, 0.8, 0.0);
            let head = rect_yaw(0.0, 0.0, 0.8, 1.6, 0.0);
            let ang = slab.ax.to_angle();
            let to = |q: &Vec2| at + Vec2::from_angle(ang).rotate(*q);
            let foot: Vec<Vec2> = foot.iter().map(to).collect();
            let head: Vec<Vec2> = head.iter().map(to).collect();
            loft(g, f, Concrete, &foot, &head, 0.0, ph - 1.2, BETON);
        }
    }
    // The lobby, glazed, under the middle.
    let lobby = slab.quad(-hl * 0.16, hl * 0.16, -hw * 0.3, hw * 0.3);
    extrude(g, f, Glass, &lobby, 0.0, 3.6, pal.glass_dark);
    for face in c.faces(&lobby) {
        ribbon(g, c, rng, &face, 0.1, 3.3, 1.5, &[LAMP], DARK);
    }
    // The block itself, then the screen on both long faces.
    band(g, f, Concrete, &poly, &poly, ph, y_top, BETON);
    let street = (floors / 2).max(2);
    let fronts = [Face::from_edge(poly[0], poly[1]), Face::from_edge(poly[2], poly[3])];
    let fin_d = 1.3;
    let mut homes: Vec<(Face, f32, f32, f32, f32)> = Vec::new();
    for face in &fronts {
        let w = face.len;
        // Loggias: a coloured side wall and the glazing behind each bay.
        for fl in 0..floors {
            let y = ph + fl as f32 * fh;
            if fl == street || fl == street + 1 {
                continue;
            }
            for b in 0..bays {
                let t = -w * 0.5 + (b as f32 + 0.5) * bay;
                let col = LOGGIA[((rng.random::<f32>() * 10.0) as usize) % LOGGIA.len()];
                face.decal(g, f, Plaster, t - bay * 0.5 + 0.45, y + fh * 0.55, 0.035, 0.5, fh - 0.4, col);
                face.decal(g, f, Glass, t + 0.25, y + fh * 0.62, 0.05, bay - 1.3, fh - 1.1, pal.glass_dark);
                if coin(rng, c.lit_chance() * 0.7) {
                    face.glaze(g, c, t + 0.25, y + fh * 0.62, 0.08, bay - 1.5, fh - 1.3, Pane::Lit(if rng.random_bool(0.7) { LAMP } else { LAMP_SOFT }));
                } else if rng.random_bool(0.45) {
                    homes.push((*face, t + 0.25, y + fh * 0.62, bay - 1.6, fh - 1.4));
                }
            }
        }
        // The shopping street: a band of glass behind dense louvres.
        let ys = ph + street as f32 * fh;
        face.decal(g, f, Glass, 0.0, ys + fh, 0.05, w - 0.4, fh * 2.0 - 0.4, pal.glass_dark);
        for b in 0..bays {
            let t = -w * 0.5 + (b as f32 + 0.5) * bay;
            if coin(rng, c.lit_chance()) {
                face.glaze(g, c, t, ys + fh, 0.08, bay - 0.6, fh * 2.0 - 0.8, Pane::Lit(LAMP));
            }
        }
        g.lod(Lod::Mid, |g| {
            let n = (w / 0.7) as i32;
            for j in 0..=n {
                let t = -w * 0.5 + 0.2 + j as f32 * (w - 0.4) / n as f32;
                face.bx(g, f, Concrete, t, ys + fh, 0.45, Vec3::new(0.12, fh * 2.0 - 0.2, 0.8), mul(BETON, 1.06));
            }
        });
        // The loggias' painted side walls, on the fins either side.
        for fl in 0..floors {
            if fl == street || fl == street + 1 {
                continue;
            }
            let y = ph + fl as f32 * fh + fh * 0.5 + 0.15;
            for b in 0..bays {
                let col = LOGGIA[(rng.random::<f32>() * LOGGIA.len() as f32) as usize % LOGGIA.len()];
                let side = if rng.random_bool(0.5) { -1.0f32 } else { 1.0 };
                let t = -w * 0.5 + (b as f32 + 0.5) * bay + side * (bay * 0.5 - 0.17);
                let at = face.at(t, fin_d * 0.5);
                let facing = f.dir3(face.tangent * -side);
                g.part(Plaster).decal(w3(c, at, y) + facing * 0.012, f.dir3(face.out), Vec3::Y, fin_d - 0.15, fh - 0.75, facing, rgba(col));
            }
        }
        // The sun-screen: a fin at every bay, a slab and a solid parapet
        // at every floor.
        for b in 0..=bays {
            let t = -w * 0.5 + b as f32 * bay;
            face.bx(g, f, Concrete, t, (ph + y_top) * 0.5, fin_d * 0.5, Vec3::new(0.32, y_top - ph, fin_d), BETON);
        }
        for fl in 0..=floors {
            let y = ph + fl as f32 * fh;
            face.bx(g, f, Concrete, 0.0, y + 0.14, fin_d * 0.5 + 0.02, Vec3::new(w + 0.32, 0.28, fin_d + 0.04), mul(BETON, 0.96));
            if fl < floors && fl != street && fl != street + 1 {
                face.bx(g, f, Concrete, 0.0, y + 0.75, fin_d + 0.07, Vec3::new(w + 0.32, 0.9, 0.14), mul(BETON, 1.04));
            }
        }
    }
    night_lights(g, c, &homes, 0.095, mul(LAMP, 0.75));
    // The blank ends: a few small windows, a stair tower on one.
    let ends = [Face::from_edge(poly[1], poly[2]), Face::from_edge(poly[3], poly[0])];
    for (e, face) in ends.iter().enumerate() {
        for _ in 0..(floors / 3) {
            let t = rng.random_range(-face.len * 0.35..face.len * 0.35);
            let y = ph + rng.random_range(1..floors) as f32 * fh + 1.2;
            face.decal(g, f, Glow, t, y, 0.05, 0.6, 0.6, mul(LAMP, 0.7));
        }
        if e == rng.random_range(0..2) {
            let tw = Plan { c: face.at(0.0, 2.1), ax: face.tangent, hl: 2.0, hw: 2.0 };
            if sits(c, &tw.poly(), 0.3) {
                extrude(g, f, Concrete, &tw.poly(), 0.0, y_top + 2.5, mul(BETON, 0.92));
                let out = Face::new(tw.at(-2.0, 2.0), tw.at(2.0, 2.0), face.out);
                out.decal(g, f, Glow, 0.0, (ph + y_top) * 0.5, 0.05, 0.4, y_top - ph - 2.0, mul(LAMP, 0.6));
            }
        }
    }
    // The roof terrace.
    ground(g, f, Concrete, &poly, y_top, mul(BETON, 0.9));
    let rim = slab.grown(-0.2).poly();
    band(g, f, Concrete, &rim, &rim, y_top, y_top + 1.4, mul(BETON, 0.95));
    let stack_at = slab.at(hl * 0.3 * if rng.random_bool(0.5) { 1.0 } else { -1.0 }, 0.0);
    let sw = wv(c, stack_at);
    g.part(Concrete).lathe(sw, &[(2.4, y_top), (2.0, y_top + 3.0), (1.5, y_top + 6.5)], 14, rgba(BETON));
    g.part(Matte).cap(&MeshData::ngon(sw, 1.5, 14, 0.0, y_top + 6.5), Vec3::Y, rgba(DARK));
    // The gym's vault.
    let vault = slab.at(-hl * 0.15 * if stack_at.distance(slab.at(-hl * 0.15, 0.0)) < 6.0 { -1.0 } else { 1.0 }, 0.0);
    let vr = (hw * 0.45).min(4.0);
    let vlen = (hl * 0.35).min(12.0);
    let rings: Vec<Vec<Vec3>> = [-vlen * 0.5, vlen * 0.5]
        .iter()
        .map(|a| {
            (0..=10)
                .map(|j| {
                    let ang = PI * j as f32 / 10.0;
                    let q = vault + slab.ax * *a + slab.across() * (ang.cos() * vr);
                    w3(c, q, y_top + ang.sin() * vr * 0.8)
                })
                .collect()
        })
        .collect();
    g.part(Concrete).strip(&rings, rgba(mul(BETON, 1.05)));
    for a in [-vlen * 0.5 - 0.02, vlen * 0.5 + 0.02] {
        let end = Face::new(vault + slab.ax * a - slab.across() * vr, vault + slab.ax * a + slab.across() * vr, slab.ax * a.signum());
        end.decal(g, f, Glow, 0.0, y_top + vr * 0.35, 0.03, vr * 1.2, vr * 0.5, mul(LAMP, 0.7));
    }
    // The track round the edge, a paddling pool and the roof bar's lights.
    let track = slab.grown(-1.4).poly();
    g.part(Paving).path_strip(&track.iter().chain(std::iter::once(&track[0])).map(|q| wv(c, *q)).collect::<Vec<_>>(), 0.0, 1.1, y_top + 0.03, rgba([0.55, 0.28, 0.2]));
    let pool = slab.quad(hl * 0.55, hl * 0.8, -hw * 0.35, hw * 0.35);
    ground(g, f, Matte, &pool, y_top + 0.06, [0.15, 0.5, 0.6]);
    neon_line(g, c, &pool, y_top + 0.1, true, mul(CYAN_LINE, 0.6));
    g.lod(Lod::Mid, |g| {
        let l3 = |q: Vec2| Vec3::new(q.x, y_top + 3.0, q.y);
        string_lights(g, f, l3(slab.at(-hl * 0.9, -hw * 0.6)), l3(slab.at(-hl * 0.5, hw * 0.6)), 10, AMBER_LINE);
        string_lights(g, f, l3(slab.at(-hl * 0.9, hw * 0.6)), l3(slab.at(-hl * 0.5, -hw * 0.6)), 10, AMBER_LINE);
    });
    // A line of light along the roof's rim.
    neon_line(g, c, &[poly[0], poly[1]], y_top + 1.45, false, mul(AMBER_LINE, 0.5));
    neon_line(g, c, &[poly[2], poly[3]], y_top + 1.45, false, mul(AMBER_LINE, 0.5));
    // The park round it: lawns, rows of trees, a way in.
    let door = slab.at(0.0, 0.0);
    let gate = front_door_path(c, door, 0.8);
    walk(g, c, gate, door, 3.0, mul(pal.paving, 1.05));
    let taken = vec![(slab.grown(2.0).poly(), 1.0), (ccw(vec![gate - Vec2::X * 2.0, gate + Vec2::X * 2.0, door + Vec2::X * 2.0, door - Vec2::X * 2.0]), 1.0)];
    woods(g, c, rng, (18.0 * k * k) as usize, 0.25, false, &taken, 1.2);
    front_lamps(g, c, 4);
}

// ── the wave library (after Aalto) ───────────────────────────────────────────

/// A white block on a dark granite plinth along the street, a timber wave
/// rippling along its front over the door, and behind it a reading hall
/// that fans out toward the view under round skylights, clerestory light
/// along its arc; a stair tower; birches.
pub fn fan_library(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = Site::of(c, rng, 2000.0);
    let k = site.k;
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.7));
    let Some(p) = best_plan(c, 2.0, 4.0) else { return };
    // The block along the street.
    let depth = (p.hw * 0.7).clamp(5.0, 10.0);
    let Some(block) = street_band(c, 3.5, depth, (26.0 * k).max(6.0)) else { return };
    let front_dir = c.front_face(&c.lot_poly).out;
    let (blen, bc) = (block.hl, block.c);
    let bpoly = block.poly();
    let bh = 8.6 + 1.6 * (k - 1.0).max(0.0);
    extrude(g, f, Stone, &block.grown(0.12).poly(), 0.0, 0.7, GRANITE);
    band(g, f, Matte, &bpoly, &bpoly, 0.7, bh, WHITE);
    ground(g, f, Matte, &bpoly, bh, [0.2, 0.2, 0.21]);
    let cop = block.grown(0.15).poly();
    band(g, f, Metal, &cop, &cop, bh - 0.05, bh + 0.35, COPPER_DARK);
    for face in c.faces(&bpoly) {
        if face.len > 6.0 {
            ribbon(g, c, rng, &face, 4.6, 1.6, 1.1, &[LAMP, LAMP_SOFT], DARK);
        }
    }
    // The fan behind, toward the view (or straight back).
    let back = -front_dir;
    let dir = if site.view.dot(back) > 0.1 { site.view } else { back };
    let apex = bc + back * (depth * 0.5 - 1.5);
    let half = rng.random_range(0.55..0.75);
    let mut r1 = (ray(c, apex, dir) - 1.5).min(30.0 * k);
    let mut fan = fan_poly(apex, dir, half, 0.0, r1, 12);
    for _ in 0..30 {
        if sits(c, &fan, 0.8) || r1 < 8.0 {
            break;
        }
        r1 -= 1.0;
        fan = fan_poly(apex, dir, half, 0.0, r1, 12);
    }
    let mut solids = vec![(block.grown(1.5).poly(), 1.5)];
    if r1 >= 8.0 && sits(c, &fan, 0.8) {
        // Highest over the desks by the block, falling toward the arc.
        let h_at = |q: Vec2| 7.2 - 2.0 * (q.distance(apex) / r1).clamp(0.0, 1.0);
        let b3: Vec<Vec3> = fan.iter().map(|q| w3(c, *q, 0.0)).collect();
        let t3: Vec<Vec3> = fan.iter().map(|q| w3(c, *q, h_at(*q))).collect();
        g.part(Matte).band(&b3, &t3, rgba(WHITE));
        let arc: Vec<Vec3> = fan.iter().filter(|q| q.distance(apex) > 0.5).map(|q| w3(c, *q, h_at(*q) + 0.02)).collect();
        fan3(g, Metal, w3(c, apex, 7.22), &arc, false, true, COPPER_DARK);
        // The clerestory along the arc, timber battens below it.
        for w in fan.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a.distance(apex) < r1 * 0.9 || b.distance(apex) < r1 * 0.9 {
                continue;
            }
            let seg = Face::from_edge(a, b);
            let h = h_at(a);
            ribbon(g, c, rng, &seg, h - 2.0, 1.4, 1.0, &[LAMP, LAMP_SOFT], DARK);
            g.lod(Lod::Mid, |g| {
                let n = (seg.len / 0.35) as i32;
                for j in 0..n {
                    let t = -seg.len * 0.5 + (j as f32 + 0.5) * seg.len / n as f32;
                    seg.bx(g, f, Wood, t, (h - 2.2) * 0.5 + 0.1, 0.1, Vec3::new(0.09, h - 2.3, 0.14), BIRCH);
                }
            });
        }
        // Round skylights on the roof in rings across the fan, glowing at
        // night.
        let mut spots: Vec<Vec2> = Vec::new();
        let mut rr = (r1 * 0.28).max(4.0);
        while rr < r1 - 2.0 {
            let n = ((2.0 * half * rr) / 3.4).floor().max(1.0) as i32;
            for j in 0..n {
                let aa = dir.to_angle() - half + 2.0 * half * (j as f32 + 0.5) / n as f32;
                spots.push(apex + Vec2::from_angle(aa) * rr);
            }
            rr += 3.4;
        }
        for q in spots {
            if !point_in_polygon(q, &fan) || distance_to_boundary(q, &fan) < 1.2 {
                continue;
            }
            let y = h_at(q);
            let qw = wv(c, q);
            g.part(Matte).cylinder_capped(qw, 0.85, 0.75, y - 0.1, y + 0.6, 10, false, false, rgba(mul(WHITE, 0.95)));
            g.part(Glow).cap(&MeshData::ngon(qw, 0.7, 10, 0.0, y + 0.62), Vec3::Y, rgba(mul(LAMP, 0.65)));
        }
        solids.push((fan.clone(), 2.0));
        // A reading terrace of grass steps at the arc.
        let steps_at = apex + dir * (r1 + 2.5);
        if fits(c, steps_at, 2.0, 0.3) {
            for s in 0..3 {
                let r0 = r1 + 0.6 + s as f32 * 1.2;
                let ring = fan_poly(apex, dir, half * 0.6, r0, r0 + 1.2, 8);
                if sits(c, &ring, 0.3) {
                    extrude(g, f, Stone, &ring, 0.0, 0.42 - s as f32 * 0.13, mul(CREAM_STONE, 0.85));
                }
            }
        }
    }
    // The wave over the door: battens on a rippling line, a timber ribbon
    // along their tops lit from beneath.
    let fs = front_dir.dot(block.across()).signum();
    let front = Face::new(block.at(-blen, fs * depth * 0.5), block.at(blen, fs * depth * 0.5), front_dir);
    let wave_len = (front.len * 0.7).min(24.0);
    let room = c.reach_out(&front).min(4.0);
    if room > 1.6 {
        let amp = (room - 1.0).min(1.4) * 0.5;
        let reach = 0.8 + amp;
        let pts: Vec<(Vec2, f32)> = (0..=(wave_len / 0.4) as i32)
            .map(|j| {
                let t = -wave_len * 0.5 + j as f32 * 0.4;
                let wv_ = (t / wave_len * TAU * 1.5).sin();
                (front.at(t, reach + amp * wv_), 3.6 + 0.6 * (t / wave_len * TAU).cos())
            })
            .collect();
        let top: Vec<Vec2> = pts.iter().map(|(q, _)| wv(c, *q)).collect();
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            g.part(Wood).quad_facing([w3(c, a.0, a.1 - 0.5), w3(c, b.0, b.1 - 0.5), w3(c, b.0, b.1), w3(c, a.0, a.1)], f.dir3(front.out), rgba(BIRCH));
            g.part(Wood).quad_facing([w3(c, a.0, a.1), w3(c, b.0, b.1), w3(c, b.0 - front.out * 1.2, b.1), w3(c, a.0 - front.out * 1.2, a.1)], Vec3::Y, rgba(mul(BIRCH, 0.85)));
        }
        let _ = top;
        g.lod(Lod::Mid, |g| {
            for (q, h) in pts.iter().step_by(2) {
                g.part(Wood).boxed(w3(c, *q, (h - 0.5) * 0.5), Vec3::new(0.08, h - 0.5, 0.08), f.yaw(), rgba(mul(BIRCH, 0.9)));
            }
        });
        let lit: Vec<Vec2> = pts.iter().map(|(q, _)| *q - front.out * 0.6).collect();
        neon_line(g, c, &lit, 3.0, false, mul(AMBER_LINE, 0.55));
        entrance(g, c, rng, &front, 0.0, 4.0, 3.0);
    }
    // The stair tower at one end of the block.
    let e = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let st = Plan { c: block.at(e * (blen - 2.0), 0.0) + back * (depth * 0.5 + 1.6), ax: block.ax, hl: 2.0, hw: 1.8 };
    if sits(c, &st.poly(), 0.5) {
        extrude(g, f, Matte, &st.poly(), 0.0, bh + 3.2, WHITE);
        band(g, f, Metal, &st.grown(0.12).poly(), &st.grown(0.12).poly(), bh + 3.0, bh + 3.4, COPPER_DARK);
        let fc = Face::new(st.at(-2.0, -1.8), st.at(2.0, -1.8), -st.across());
        fc.decal(g, f, Glow, 0.0, (bh + 3.0) * 0.5, 0.05, 0.5, bh + 1.0, mul(LAMP, 0.6));
        solids.push((st.grown(1.0).poly(), 1.0));
    }
    let gate = front_door_path(c, front.at(0.0, 0.5), 0.6);
    walk(g, c, gate, front.at(0.0, 0.5), 3.2, mul(CREAM_STONE, 0.9));
    woods(g, c, rng, (24.0 * k * k) as usize, 0.2, true, &solids, 1.0);
    front_lamps(g, c, 3);
}

// ── the house of culture (after Aalto) ───────────────────────────────────────

/// A red-brick concert hall shaped like a fan, ribbed along its curved
/// back, its copper roof climbing in steps with clerestory glass in every
/// riser; a brick office wing along the street with ribbon windows, a
/// glazed foyer under a timber wave between them, a courtyard of grass
/// steps opening toward the view.
pub fn culture_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = Site::of(c, rng, 2800.0);
    let k = site.k;
    fill_site(g, c, Foliage, 0.03, mul(pal.foliage, 0.72));
    let Some(p) = best_plan(c, 2.0, 3.2) else { return };
    // The office wing along the street at one end of the site.
    let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let wdepth = (p.hw.min(p.hl) * 0.42).clamp(5.0, 9.0);
    let Some(street) = street_band(c, 2.5, wdepth, 60.0) else { return };
    let front_dir = c.front_face(&c.lot_poly).out;
    let back = -front_dir;
    let ax = street.ax;
    let wlen = (street.hl * 0.4).clamp(5.0, 22.0);
    let wing = Plan { c: street.c + ax * side * (street.hl - wlen), ax, hl: wlen, hw: wdepth * 0.5 };
    let wpoly = wing.poly();
    let floors = if k > 1.2 { 4 } else { 3 };
    let wh = 1.0 + floors as f32 * 3.2;
    let mut solids: Vec<(Vec<Vec2>, f32)> = Vec::new();
    let mut keep_out: Vec<Vec<Vec2>> = Vec::new();
    if sits(c, &wpoly, 0.5) {
        extrude(g, f, Stone, &wing.grown(0.1).poly(), 0.0, 0.6, GRANITE);
        band(g, f, Brick, &wpoly, &wpoly, 0.6, wh, RED_BRICK);
        ground(g, f, Matte, &wpoly, wh, [0.2, 0.2, 0.21]);
        let cop = wing.grown(0.15).poly();
        band(g, f, Metal, &cop, &cop, wh - 0.05, wh + 0.4, COPPER_DARK);
        for face in c.faces(&wpoly) {
            if face.len > 5.0 {
                for fl in 0..floors {
                    ribbon(g, c, rng, &face, 1.4 + fl as f32 * 3.2, 1.5, 1.2, &[LAMP, LAMP_SOFT], [0.85, 0.84, 0.8]);
                }
            }
        }
        solids.push((wing.grown(1.0).poly(), 1.0));
        keep_out.push(wing.grown(0.6).poly());
    }
    // The foyer at the wing's inner end: glass under a timber wave.
    let inner_end = wing.c - ax * side * wlen;
    let fo = Plan { c: inner_end - ax * side * 5.0 + back * 0.6, ax, hl: 5.0, hw: wdepth * 0.5 - 0.6 };
    let fpoly = fo.poly();
    let fs = front_dir.dot(fo.across()).signum();
    if sits(c, &fpoly, 0.5) {
        extrude(g, f, Glass, &fpoly, 0.0, 4.6, pal.glass_dark);
        ground(g, f, Matte, &fpoly, 4.62, [0.2, 0.2, 0.21]);
        for face in c.faces(&fpoly) {
            ribbon(g, c, rng, &face, 0.1, 4.2, 1.6, &[LAMP], DARK);
        }
        // The wave: a rippling timber canopy out over the doors.
        let n = 20;
        let edge = |j: usize| fo.at(-fo.hl - 1.0 + (2.0 * fo.hl + 2.0) * j as f32 / n as f32, fs * fo.hw);
        let y = |j: usize| 5.2 + 0.75 * (j as f32 / n as f32 * TAU * 1.25).sin();
        let reach = c.reach_out(&Face::new(fo.at(-fo.hl, fs * fo.hw), fo.at(fo.hl, fs * fo.hw), front_dir)).min(4.0) - 0.4;
        if reach > 1.0 {
            for j in 0..n {
                let (a, b) = (edge(j), edge(j + 1));
                let (ya, yb) = (y(j), y(j + 1));
                let q = [w3(c, a - front_dir * 1.0, ya), w3(c, b - front_dir * 1.0, yb), w3(c, b + front_dir * reach, yb - 0.35), w3(c, a + front_dir * reach, ya - 0.35)];
                g.part(Wood).quad_facing(q, Vec3::Y, rgba(BIRCH));
                g.part(Wood).quad_facing(q.map(|v| v - Vec3::Y * 0.18), Vec3::NEG_Y, rgba(mul(BIRCH, 0.75)));
                g.part(Wood).quad_facing([q[3] - Vec3::Y * 0.18, q[2] - Vec3::Y * 0.18, q[2], q[3]], f.dir3(front_dir), rgba(mul(BIRCH, 0.9)));
            }
            let under: Vec<Vec2> = (0..=n).map(|j| edge(j) + front_dir * (reach - 0.3)).collect();
            neon_line(g, c, &under, 4.55, false, mul(AMBER_LINE, 0.45));
        }
        solids.push((fo.grown(2.0).poly(), 0.5));
        keep_out.push(fo.grown(0.4).poly());
    }
    // The hall: a fan from behind the foyer, opening away from the wing
    // and toward the view.
    let away = -ax * side;
    let lean = if site.real { site.view.dot(away).clamp(-0.4, 1.0) } else { rng.random_range(0.3..0.8) };
    let dir = (back + away * lean.max(0.15)).normalize_or(back);
    let half = rng.random_range(0.42..0.58);
    let r0 = 3.0;
    let clear = |keep_out: &[Vec<Vec2>], poly: &[Vec2]| keep_out.iter().all(|o| !poly.iter().any(|q| point_in_polygon(*q, o)) && !o.iter().any(|q| point_in_polygon(*q, poly)));
    // Where the hall fits biggest: behind the foyer, the wing or the
    // middle of the street front, opening toward the view, straight back
    // or away from the wing. The site's shape decides.
    let mut dirs = vec![dir, back, (back + away).normalize_or(back)];
    if site.real && site.view.dot(back) > -0.3 {
        dirs.push(site.view);
    }
    let apexes = [fo.c + back * (fo.hw + 1.4), wing.c + back * (wing.hw + 1.6), street.c + back * (street.hw + 2.0)];
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    for &a in &apexes {
        for &d in &dirs {
            let mut r = (ray(c, a, d) - 2.0).min(34.0 * k);
            while r >= 12.0 {
                let poly = fan_poly(a, d, half, r0, r, 12);
                if sits(c, &poly, 0.8) && clear(&keep_out, &poly) {
                    break;
                }
                r -= 1.0;
            }
            if r >= 12.0 && best.is_none_or(|(b, _, _)| r > b + 0.5) {
                best = Some((r, a, d));
            }
        }
    }
    let (r1, apex, dir) = best.unwrap_or((0.0, fo.c, back));
    let fan = fan_poly(apex, dir, half, r0, r1.max(r0 + 1.0), 12);
    if r1 >= 12.0 && sits(c, &fan, 0.8) && clear(&keep_out, &fan) {
        // The roof climbs in steps toward the wide end.
        let steps = (3.0 + 1.5 * k).round().clamp(3.0, 6.0) as usize;
        let rad = |i: usize| r0 + (r1 - r0) * i as f32 / steps as f32;
        let y_of = |i: usize| 7.0 + i as f32 * (8.0 * k.min(1.3) / steps as f32);
        for i in 0..steps {
            let sector = fan_poly(apex, dir, half, rad(i), rad(i + 1), 12);
            let y = y_of(i + 1);
            band(g, f, Brick, &sector, &sector, 0.0, y, RED_BRICK);
            ground(g, f, Metal, &sector, y, mul(COPPER_DARK, 1.0 + 0.05 * i as f32));
            // The riser above the step before: clerestory glass facing the
            // stage, its light at night.
            if i > 0 {
                let y0 = y_of(i);
                let arc: Vec<Vec2> = (0..=12).map(|j| apex + Vec2::from_angle(dir.to_angle() - half + 2.0 * half * j as f32 / 12.0) * rad(i)).collect();
                // The hall is in use into the night: its clerestories glow
                // after dark whatever the window lights say.
                let mut glow: Vec<(Face, f32, f32, f32, f32)> = Vec::new();
                for w in arc.windows(2) {
                    let seg = Face::new(w[0], w[1], apex - (w[0] + w[1]) * 0.5);
                    seg.decal(g, f, Glass, 0.0, (y0 + y) * 0.5, 0.05, seg.len + 0.02, y - y0 - 0.15, pal.glass_dark);
                    glow.push((seg, 0.0, (y0 + y) * 0.5, seg.len * 0.9, (y - y0) * 0.75));
                }
                night_lights(g, c, &glow, 0.08, mul(LAMP, 0.85));
            }
        }
        // Tall slits down both flanks, lit at night.
        for sg in [-1.0f32, 1.0] {
            let fl = Vec2::from_angle(dir.to_angle() + sg * half);
            let out = Vec2::from_angle(dir.to_angle() + sg * (half + FRAC_PI_2));
            let side = Face::new(apex + fl * r0, apex + fl * r1, out);
            let n = ((r1 - r0) / 2.6).floor() as i32;
            for j in 1..n {
                let t = -side.len * 0.5 + j as f32 * side.len / n as f32;
                let rr = r0 + (r1 - r0) * (j as f32 / n as f32);
                let hy = y_of(((rr - r0) / (r1 - r0) * steps as f32).floor() as usize + 1).min(y_of(steps));
                side.decal(g, f, Glass, t, hy * 0.45, 0.05, 0.7, hy * 0.6, pal.glass_dark);
                if coin(rng, 0.3 + 0.6 * c.lit_chance()) {
                    side.glaze(g, c, t, hy * 0.45, 0.08, 0.5, hy * 0.55, Pane::Lit(LAMP_SOFT));
                }
            }
        }
        // The curved back: brick ribs, a copper coping.
        let ytop = y_of(steps);
        let arc: Vec<Vec2> = (0..=12).map(|j| apex + Vec2::from_angle(dir.to_angle() - half + 2.0 * half * j as f32 / 12.0) * r1).collect();
        for w in arc.windows(2) {
            let seg = Face::new(w[0], w[1], (w[0] + w[1]) * 0.5 - apex);
            g.lod(Lod::Mid, |g| {
                let n = (seg.len / 1.3).round().max(1.0) as i32;
                for j in 0..n {
                    let t = -seg.len * 0.5 + (j as f32 + 0.5) * seg.len / n as f32;
                    seg.bx(g, f, Brick, t, ytop * 0.5, 0.2, Vec3::new(0.42, ytop - 0.2, 0.4), mul(RED_BRICK, 0.9));
                }
            });
            g.part(Metal).strip_box(wv(c, w[0]), wv(c, w[1]), 0.0, 0.7, ytop, ytop + 0.4, rgba(COPPER_DARK));
        }
        // The stage house: a tall brick box at the narrow end.
        let sh = Plan { c: apex + dir * (r0 + 1.4), ax: dir.perp(), hl: (r0 + 1.4) * half.sin() + 2.2, hw: 2.6 };
        if sits(c, &sh.poly(), 0.5) {
            extrude(g, f, Brick, &sh.poly(), 0.0, ytop + 1.6, mul(RED_BRICK, 0.95));
            ground(g, f, Metal, &sh.grown(0.1).poly(), ytop + 1.62, COPPER_DARK);
        }
        solids.push((fan.clone(), 2.0));
        keep_out.push(fan.clone());
    }
    // The courtyard behind the wing: grass steps rising away from it.
    for s in 0..4 {
        let q = wing.c + back * (wdepth * 0.5 + 3.0 + s as f32 * 1.7);
        let pl = Plan { c: q, ax, hl: wlen * 0.8, hw: 0.85 };
        let poly = pl.poly();
        if !sits(c, &poly, 0.6) || !clear(&keep_out, &poly) {
            break;
        }
        let y = 0.4 + s as f32 * 0.42;
        extrude(g, f, Stone, &poly, 0.0, y - 0.06, mul(CREAM_STONE, 0.8));
        ground(g, f, Foliage, &pl.grown(-0.06).poly(), y, mul(pal.foliage, 0.85));
        solids.push((pl.grown(0.6).poly(), 0.5));
    }
    // The forecourt: paving from the street to the foyer's doors.
    let door = fo.at(0.0, fs * fo.hw);
    let gate = front_door_path(c, door, 0.4);
    walk(g, c, gate, door, 7.0, mul(CREAM_STONE, 0.85));
    solids.push((ccw(vec![gate - ax * 4.0, gate + ax * 4.0, door + ax * 4.0, door - ax * 4.0]), 0.5));
    woods(g, c, rng, (22.0 * k * k) as usize, 0.5, true, &solids, 1.0);
    front_lamps(g, c, 4);
}
