//! Parks, built from the sim's plan for the lot: its paths, benches and
//! features are placed by `nd_sim::park`, which looks at the whole
//! connected park; this file gives them a look that follows the land.
//!
//! Cheap land: bare dirt, weeds, a court with a bent hoop, a broken bench,
//! a drum fire. Middle: lawn, paved paths, a playground, a pond, a kiosk.
//! Dear land: a manicured park with a lake, a fountain, a bandstand, rides,
//! formal avenues and railings.

use super::parts::*;
use super::{Ctx, Face, Frame};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::park::{distance_to_boundary as edge_distance, ParkFeature, ParkFeatureSlot, ParkLayout};
use nd_sim::Tier;
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

pub fn scrub_lot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    park(g, c, rng);
}

pub fn garden(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    park(g, c, rng);
}

pub fn plaza(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    park(g, c, rng);
}

/// [parks] (D5) What the sim says of a park beyond its plan: the litter
/// on its ground and the dealers working it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParkLook {
    pub trash: f32,
    pub dealers: u8,
}

/// Everything a park needs to know while it is being drawn.
struct Park<'a> {
    layout: &'a ParkLayout,
    tier: Tier,
    /// Lot outline in world coordinates.
    lot: Vec<Vec2>,
    /// The lot's edges on the outside of the park (not shared with another
    /// lot of it), world coordinates. Trees and lamps keep clear of these
    /// only: a margin along a shared edge too left a bare strip down every
    /// seam, so a park of several lots read as a row of small ones.
    open_edges: Vec<(Vec2, Vec2)>,
    path_width: f32,
}

impl Park<'_> {
    /// Whether a world point lies on this lot, `margin` in from its edges on
    /// the outside of the park (the seams with the park's other lots need
    /// no margin: the park carries on over them).
    /// Parks are planned in world coordinates, unlike every other style, so
    /// the name says which space it takes (`Ctx::inside_local` is the other).
    fn inside_world(&self, w: Vec2, margin: f32) -> bool {
        if !nd_sim::zoning::point_in_polygon(w, &self.lot) {
            return false;
        }
        // The ends of the seams too: past a seam's end the neighbour lot's
        // own side runs off, and a canopy leaning round it would leave the
        // park. Over a seam itself half the margin: trees may lean across.
        self.open_edges.iter().all(|(a, b)| w.distance(project(w, *a, *b)) >= margin) && self.layout.seams.iter().all(|(a, b)| w.distance(*a) >= margin && w.distance(*b) >= margin && w.distance(project(w, *a, *b)) >= margin * 0.5)
    }

    /// Distance from a world point to the lot edge: how big a paved apron or
    /// a ring of tables can be before it spills onto the neighbours.
    fn room(&self, w: Vec2) -> f32 {
        if nd_sim::zoning::point_in_polygon(w, &self.lot) { edge_distance(w, &self.lot) } else { 0.0 }
    }

    /// Distance from a point to the nearest path centreline.
    fn path_distance(&self, w: Vec2) -> f32 {
        let mut best = f32::MAX;
        for path in &self.layout.paths {
            for s in path.windows(2) {
                best = best.min(w.distance(project(w, s[0], s[1])));
            }
        }
        best
    }

    fn clear_of_features(&self, w: Vec2, margin: f32) -> bool {
        self.layout.features.iter().all(|f| f.pos.distance(w) >= f.radius + margin)
    }

    fn clear_of_benches(&self, w: Vec2, margin: f32) -> bool {
        self.layout.benches.iter().all(|(b, _)| b.distance(w) >= margin)
    }
}

fn project(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 < 1e-6 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
}

/// [attractions] A park drawn from `Ctx::park` under a landmark's own
/// buildings (the grand park, the botanical garden).
pub(crate) fn draw(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    park(g, c, rng);
}

fn park(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let Some(layout) = c.park.as_ref() else { return };
    // The park's own tier, planned once for all its lots (from their
    // area-weighted land value), not the style the lot's building drew:
    // otherwise one lot of a park came out as bare dirt beside its
    // neighbour's lawn, and the park fell apart into plots.
    let tier = layout.tier;
    let f = &c.frame;
    let pal = c.palette;
    let lot: Vec<Vec2> = c.lot_poly.iter().map(|p| f.to_world(p.x, p.y)).collect();
    let open_edges = open_stretches(&lot, &layout.seams);
    let park = Park {
        layout,
        tier,
        lot,
        open_edges,
        path_width: match tier {
            Tier::Low => 1.6,
            Tier::Mid => 2.0,
            Tier::High => 2.4,
        },
    };
    let inner = offset_poly(&c.lot_poly, 0.3);
    // The lawn runs right up to (a hair over) the edges shared with the
    // park's other lots, so their ground meets without a seam; it keeps the
    // usual inset from the street and the neighbours.
    let lawn = if layout.shared_edges.is_empty() { inner.clone() } else { inset_edges(&c.lot_poly, |i| if layout.shared_edges.contains(&i) { -0.05 } else { 0.3 }) };
    // [parks] (D5) A nature preserve: meadow, a trail, woodland.
    if layout.preserve {
        wild(g, c, rng, &park, &lawn);
        return;
    }
    park_ground(g, c, rng, &park, &inner, &lawn);
    park_paths(g, c, &park);
    park_benches(g, c, rng, &park);
    for slot in &layout.features {
        feature(g, rng, &park, slot, pal);
    }

    // ── trees ────────────────────────────────────────────────────────────
    trees(g, rng, &park, pal);
    park_edges(g, c, rng, &park, &inner);
    // [parks] (D5) A park let go: its litter and its dealers.
    if let Some(look) = c.park_look {
        let_go(g, c, rng, &park, look);
    }
}

/// [parks] (D5) Litter over a neglected park's ground (paper, cans,
/// bags, by how much there is) and the dealers by its paths: hooded
/// figures standing about, a phone lit in a hand.
fn let_go(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, park: &Park, look: ParkLook) {
    let area = nd_sim::zoning::polygon_area(&park.lot).abs();
    let n = ((look.trash - 0.1).max(0.0) * area / 25.0).min(160.0) as usize;
    let colours = [[0.86, 0.85, 0.8], [0.75, 0.2, 0.18], [0.3, 0.55, 0.8], [0.2, 0.2, 0.22], [0.85, 0.75, 0.3]];
    for _ in 0..n {
        let Some(p) = c.random_inside(rng, 0.8) else { continue };
        let w = c.frame.to_world(p.x, p.y);
        let col = colours[rng.random_range(0..colours.len())];
        if rng.random_bool(0.2) {
            // A bag of rubbish.
            g.part(Tarp).boxed(Vec3::new(w.x, 0.22, w.y), Vec3::new(0.5, 0.44, 0.45), rng.random_range(0.0..TAU), rgba([0.12, 0.12, 0.13]));
        } else {
            g.part(Tarp).boxed(Vec3::new(w.x, 0.05, w.y), Vec3::new(0.28, 0.06, 0.2), rng.random_range(0.0..TAU), rgba(col));
        }
    }
    // The dealers, each by a path a little off it.
    let mut placed = 0;
    'paths: for path in &park.layout.paths {
        for seg in path.windows(2) {
            if placed >= look.dealers as usize {
                break 'paths;
            }
            let d = (seg[1] - seg[0]).normalize_or_zero();
            let w = seg[0].lerp(seg[1], 0.5) + d.perp() * (park.path_width * 0.5 + 1.0);
            if !park.inside_world(w, 0.5) {
                continue;
            }
            hooded_figure(g, w, d.perp().to_angle() + PI, rng);
            placed += 1;
        }
    }
}

/// A figure in a dark hoodie, hands at a lit phone (world).
fn hooded_figure(g: &mut Geometry, w: Vec2, yaw: f32, rng: &mut impl Rng) {
    let hoodie = [[0.1, 0.1, 0.12], [0.2, 0.05, 0.06], [0.08, 0.12, 0.2]][rng.random_range(0..3)];
    g.part(Matte).boxed(Vec3::new(w.x, 0.45, w.y), Vec3::new(0.34, 0.9, 0.24), yaw, rgba([0.14, 0.14, 0.16]));
    g.part(Matte).boxed(Vec3::new(w.x, 1.25, w.y), Vec3::new(0.48, 0.7, 0.3), yaw, rgba(hoodie));
    g.part(Matte).boxed(Vec3::new(w.x, 1.75, w.y), Vec3::new(0.28, 0.3, 0.3), yaw, rgba(hoodie));
    let f = Vec2::from_angle(yaw + FRAC_PI_2) * 0.25;
    g.part(Glow).boxed(Vec3::new(w.x + f.x, 1.2, w.y + f.y), Vec3::new(0.08, 0.14, 0.02), yaw, rgba([0.6, 0.9, 1.4]));
}

/// [parks] (D5) A nature preserve: long meadow grass with wildflowers,
/// its paths a beaten trail, woodland thick round them, a wooden sign by
/// the way in.
fn wild(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, park: &Park, lawn: &[Vec2]) {
    let (f, pal) = (&c.frame, c.palette);
    ground(g, f, Foliage, lawn, 0.03, mul(pal.foliage, 0.8));
    // Tussocks and flowers.
    let area = nd_sim::zoning::polygon_area(&park.lot).abs();
    let flowers = [[0.9, 0.85, 0.3], [0.8, 0.35, 0.6], [0.95, 0.95, 0.92], [0.55, 0.45, 0.9]];
    for _ in 0..((area / 14.0) as usize).min(260) {
        let Some(p) = c.random_inside(rng, 0.6) else { continue };
        let w = f.to_world(p.x, p.y);
        if park.path_distance(w) < 1.4 {
            continue;
        }
        if rng.random_bool(0.3) {
            g.part(Foliage).boxed(Vec3::new(w.x, 0.12, w.y), Vec3::new(0.2, 0.24, 0.2), 0.0, rgba(flowers[rng.random_range(0..flowers.len())]));
        } else {
            g.part(Foliage).cylinder_capped(w, 0.5, 0.15, 0.0, rng.random_range(0.4..0.8), 5, false, true, rgba(mul(pal.foliage_dry, rng.random_range(0.8..1.1))));
        }
    }
    for path in &park.layout.paths {
        if path.len() >= 2 {
            g.part(Dirt).path_strip(path, 0.0, 1.3, 0.05, rgba(mul(pal.dirt, 0.8)));
        }
    }
    for (w, facing) in &park.layout.benches {
        bench_world(g, *w, facing + FRAC_PI_2, mul(pal.wood, 0.75), CHARCOAL);
    }
    // Woodland: trees close together, clear of the trail.
    let mut placed: Vec<Vec2> = Vec::new();
    for _ in 0..((area / 30.0) as usize + 4).min(160) {
        let Some(p) = c.random_inside(rng, 1.8) else { continue };
        let w = f.to_world(p.x, p.y);
        if park.path_distance(w) < 2.6 || placed.iter().any(|q| q.distance(w) < 4.2) || !park.inside_world(w, 1.5) {
            continue;
        }
        placed.push(w);
        let leaf = mul(pal.foliage, rng.random_range(0.7..1.0));
        if rng.random_bool(0.35) {
            tree_kind(g, w, 0.0, TreeKind::Conifer, rng.random_range(7.0..12.0), rng.random_range(2.0..3.0), pal.bark, leaf);
        } else {
            tree_kind(g, w, 0.0, TreeKind::Round, rng.random_range(6.0..10.0), rng.random_range(2.2..3.4), pal.bark, leaf);
        }
    }
    // A wooden sign by the way in.
    if let (true, Some(gate), Some(path)) = (park.layout.anchor, park.layout.entrances.first(), park.layout.paths.first()) {
        let dir = (path.get(1).copied().unwrap_or(*gate) - *gate).normalize_or_zero();
        let w = *gate + dir * 1.2 + dir.perp() * 1.8;
        if park.inside_world(w, 0.3) {
            for s in [-0.6f32, 0.6] {
                let q = w + dir.perp() * s;
                g.part(Wood).boxed(Vec3::new(q.x, 0.7, q.y), Vec3::new(0.12, 1.4, 0.12), 0.0, rgba(mul(pal.wood, 0.7)));
            }
            g.part(Wood).boxed(Vec3::new(w.x, 1.3, w.y), Vec3::new(1.5, 0.6, 0.08), dir.perp().to_angle(), rgba(mul(pal.wood, 0.9)));
        }
    }
}

/// The lawn (or dirt and scrapes, or a lawn and a forecourt) by tier.
fn park_ground(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, park: &Park, inner: &[Vec2], lawn: &[Vec2]) {
    let (f, pal, layout, tier) = (&c.frame, c.palette, park.layout, park.tier);
    match tier {
        Tier::Low => {
            ground(g, f, Dirt, lawn, 0.03, mul(pal.dirt, 0.92));
            // Patches of dry grass and bare scrapes.
            for _ in 0..rng.random_range(6..14) {
                if let Some(p) = c.random_inside(rng, 2.0) {
                    let r = rng.random_range(1.5f32..4.0).min(distance_to_boundary(p, &c.lot_poly) - 0.3);
                    let blob = ngon(p.x, p.y, r, 7, rng.random_range(0.0..TAU));
                    ground(g, f, Foliage, &blob, 0.04, mul(pal.foliage_dry, rng.random_range(0.8..1.1)));
                }
            }
            weeds(g, c, rng, inner, 24);
        }
        Tier::Mid => ground(g, f, Foliage, lawn, 0.03, mul(pal.foliage, 0.9)),
        Tier::High => {
            ground(g, f, Foliage, lawn, 0.03, mul(pal.foliage, 1.05));
            // A paved forecourt just inside the gate (of the park's main lot).
            let front = c.front_face(inner);
            if front.len > 6.0 && layout.anchor {
                front.bx(g, f, Paving, 0.0, 0.04, -1.2, Vec3::new((front.len * 0.6).min(14.0), 0.04, 2.4), mul(pal.paving, 1.15));
            }
        }
    }
}

/// The paths, and the lamps and bins along them (one lamp by the gate of
/// a run-down park).
fn park_paths(g: &mut Geometry, c: &Ctx, park: &Park) {
    let (pal, layout, tier) = (c.palette, park.layout, park.tier);
    let (surface, color, y) = match tier {
        Tier::Low => (Dirt, mul(pal.dirt, 0.72), 0.05),
        Tier::Mid => (Paving, pal.paving, 0.06),
        Tier::High => (Paving, mul(pal.paving, 1.18), 0.06),
    };
    for path in &layout.paths {
        if path.len() < 2 {
            continue;
        }
        g.part(surface).path_strip(path, 0.0, park.path_width, y, rgba(color));
        if tier == Tier::High {
            for side in [-1.0f32, 1.0] {
                g.part(Stone).path_strip(path, side * (park.path_width * 0.5 + 0.1), 0.2, y + 0.04, rgba(mul(pal.stone, 1.3)));
            }
        }
    }
    // Lamps and bins along the paths.
    if tier != Tier::Low {
        let lamp_gap = if tier == Tier::High { 13.0 } else { 16.0 };
        let mut s_lamp = lamp_gap * 0.4;
        let mut s_bin = 11.0;
        let mut side = 1.0f32;
        for path in &layout.paths {
            for seg in path.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let len = a.distance(b);
                if len < 1e-3 {
                    continue;
                }
                let d = (b - a) / len;
                let right = Vec2::new(-d.y, d.x);
                while s_lamp < len {
                    let w = a + d * s_lamp + right * side * (park.path_width * 0.5 + 0.7);
                    if park.inside_world(w, 0.6) && park.clear_of_features(w, 0.5) && park.clear_of_benches(w, 1.2) {
                        lamp_post_world(g, w, if tier == Tier::High { 4.0 } else { 3.4 }, pal.lamp);
                    }
                    side = -side;
                    s_lamp += lamp_gap;
                }
                s_lamp -= len;
                while s_bin < len {
                    let w = a + d * s_bin - right * side * (park.path_width * 0.5 + 0.5);
                    if park.inside_world(w, 0.5) && park.clear_of_features(w, 0.5) && park.clear_of_benches(w, 1.0) {
                        bin_world(g, w, mul(pal.steel, 0.5));
                    }
                    s_bin += 27.0;
                }
                s_bin -= len;
            }
        }
    } else if let (Some(gate), Some(path)) = (layout.entrances.first(), layout.paths.first()) {
        let dir = (path.get(1).copied().unwrap_or(*gate) - *gate).normalize_or_zero();
        let w = *gate + dir * 1.5 + dir.perp() * 1.6;
        if park.inside_world(w, 0.3) {
            lamp_post_world(g, w, 4.5, mul(pal.street_lamp, 0.4));
        }
    }
}

/// The benches (broken ones and litter in a run-down park, and a drum fire).
fn park_benches(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, park: &Park) {
    let (f, pal, layout, tier) = (&c.frame, c.palette, park.layout, park.tier);
    for (i, (w, facing)) in layout.benches.iter().enumerate() {
        let yaw = facing + FRAC_PI_2;
        if tier == Tier::Low && (i + c.seed as usize) % 3 == 0 {
            broken_bench(g, *w, yaw, pal.plywood, CHARCOAL);
        } else {
            let wood = if tier == Tier::High { mul(pal.wood, 1.1) } else { pal.wood };
            bench_world(g, *w, yaw, wood, CHARCOAL);
        }
        if tier == Tier::Low && rng.random_bool(0.5) {
            // Litter.
            for _ in 0..rng.random_range(1..4) {
                let p = *w + Vec2::new(rng.random_range(-1.5..1.5), rng.random_range(-1.5..1.5));
                g.part(Tarp).boxed(Vec3::new(p.x, 0.06, p.y), Vec3::new(0.3, 0.12, 0.2), rng.random_range(0.0..TAU), rgba(pick(rng, &pal.tarp_colors)));
            }
        }
    }
    if tier == Tier::Low {
        if let Some((w, facing)) = layout.benches.first() {
            let p = *w + Vec2::from_angle(*facing) * 2.0;
            if park.inside_world(p, 0.8) {
                drum_fire(g, f, pal, f.to_local(p));
            }
        }
    }
}

/// What lines the edges (chain-link and tagged walls, hedges, railings)
/// with gaps at the entrances, and the gate.
fn park_edges(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, park: &Park, inner: &[Vec2]) {
    let (f, pal, layout, tier) = (&c.frame, c.palette, park.layout, park.tier);
    let faces = c.faces(inner);
    let front = c.front_face(inner);
    // Every entrance (the street gate and the gaps to neighbouring park
    // lots) gets an opening in whatever lines the edge.
    let gaps: Vec<Vec2> = layout.entrances.iter().map(|e| f.to_local(*e)).collect();
    // Edges shared with the rest of the park stay open; the offset polygon
    // keeps the vertex order, so face indices match polygon edges.
    let open = |i: usize| faces.len() == c.lot_poly.len() && layout.shared_edges.contains(&i);
    match tier {
        Tier::Low => {
            // Bits of chain-link, a tagged wall.
            for (i, face) in faces.iter().enumerate() {
                if face.is_front || face.len < 5.0 || open(i) {
                    continue;
                }
                if i % 2 == 0 {
                    fence(g, c, face.a, face.at(-face.len * 0.2, 0.0), 1.8, false);
                } else {
                    face.bx(g, f, Concrete, 0.0, 0.9, -0.2, Vec3::new(face.len * 0.8, 1.8, 0.3), mul(pick(rng, &pal.concrete), 0.8));
                    // Tagged on both sides: the inside faces the lot, the outside the neighbour.
                    let inner = Face::from_edge(face.at(face.len * 0.4, -0.35), face.at(-face.len * 0.4, -0.35));
                    graffiti(g, c, rng, &inner, 0.1, 1.7, 2.5);
                    let outer = Face::from_edge(face.at(-face.len * 0.4, -0.05), face.at(face.len * 0.4, -0.05));
                    graffiti(g, c, rng, &outer, 0.1, 1.7, 1.5);
                }
            }
        }
        Tier::Mid => {
            for (i, face) in faces.iter().enumerate() {
                if face.len < 3.0 || open(i) {
                    continue;
                }
                for (t0, t1) in edge_runs(face, &gaps, 2.0) {
                    hedge(g, c, face.at(t0, 0.0), face.at(t1, 0.0), if face.is_front { 1.1 } else { 1.3 }, 0.8, pal.foliage);
                }
            }
            // Gate pillars at the main entrance only.
            for s in [-1.0f32, 1.0] {
                if !layout.anchor {
                    break;
                }
                let p = front.at(s * 1.9, -0.2);
                f.bx(g, Stone, p.x, 0.7, p.y, Vec3::new(0.5, 1.4, 0.5), mul(pal.stone, 1.2));
            }
        }
        Tier::High => {
            for (i, face) in faces.iter().enumerate() {
                if face.len < 3.0 || open(i) {
                    continue;
                }
                for (t0, t1) in edge_runs(face, &gaps, 2.2) {
                    railing(g, c, face, t0, t1);
                }
            }
            if layout.anchor {
                for s in [-1.0f32, 1.0] {
                    let p = front.at(s * 2.4, -0.3);
                    f.bx(g, Stone, p.x, 1.0, p.y, Vec3::new(0.7, 2.0, 0.7), mul(pal.stone, 1.5));
                    f.bx(g, Glow, p.x, 2.15, p.y, Vec3::new(0.3, 0.3, 0.3), mul(pal.lamp, 0.8));
                }
                // A sign with the park's name by the gate.
                sign_board(g, c, rng, &front, -front.len * 0.3, 1.6, (front.len * 0.25).min(4.0), 0.6);
            }
        }
    }
}

/// The parts of the outline `poly` not covered by any of `seams` (stretches
/// the park's other lots also border), as world segments.
fn open_stretches(poly: &[Vec2], seams: &[(Vec2, Vec2)]) -> Vec<(Vec2, Vec2)> {
    let n = poly.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = a.distance(b);
        if len < 1e-3 {
            continue;
        }
        let dir = (b - a) / len;
        // Covered intervals along this edge, from the seams lying on it.
        let mut cover: Vec<(f32, f32)> = seams
            .iter()
            .filter(|(p, q)| (*p - a).perp_dot(dir).abs() < 0.3 && (*q - a).perp_dot(dir).abs() < 0.3)
            .map(|(p, q)| {
                let (t0, t1) = ((*p - a).dot(dir), (*q - a).dot(dir));
                (t0.min(t1).max(0.0), t0.max(t1).min(len))
            })
            .filter(|(lo, hi)| hi > lo)
            .collect();
        cover.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut t = 0.0f32;
        for (lo, hi) in cover {
            if lo > t + 0.01 {
                out.push((a + dir * t, a + dir * lo));
            }
            t = t.max(hi);
        }
        if t < len - 0.01 {
            out.push((a + dir * t, b));
        }
    }
    out
}

/// `poly` with each edge `i` moved in by `inset(i)` (negative: out), the
/// corners at the meeting of the moved edges (mitres capped at 1.5 m).
fn inset_edges(poly: &[Vec2], inset: impl Fn(usize) -> f32) -> Vec<Vec2> {
    let n = poly.len();
    if n < 3 {
        return poly.to_vec();
    }
    // Inward normal: the polygon's winding decides which side is in.
    let ccw = nd_sim::zoning::polygon_area(poly) > 0.0;
    let normal = |i: usize| {
        let d = (poly[(i + 1) % n] - poly[i]).normalize_or_zero();
        if ccw { d.perp() } else { -d.perp() }
    };
    (0..n)
        .map(|i| {
            let prev = (i + n - 1) % n;
            let (d1, d2) = ((poly[i] - poly[prev]).normalize_or_zero(), (poly[(i + 1) % n] - poly[i]).normalize_or_zero());
            let a = poly[prev] + normal(prev) * inset(prev);
            let b = poly[i] + normal(i) * inset(i);
            let denom = d1.perp_dot(d2);
            let q = if denom.abs() < 1e-4 { poly[i] + (normal(prev) * inset(prev) + normal(i) * inset(i)) * 0.5 } else { a + d1 * ((b - a).perp_dot(d2) / denom) };
            let off = q - poly[i];
            if off.length() > 1.5 { poly[i] + off.normalize_or_zero() * 1.5 } else { q }
        })
        .collect()
}

/// The stretches of a face (as offsets from its middle) that are not an
/// opening: every gap point near the face cuts out `half` metres each way.
fn edge_runs(face: &Face, gaps: &[Vec2], half: f32) -> Vec<(f32, f32)> {
    let mut runs = vec![(-face.len * 0.5, face.len * 0.5)];
    for gp in gaps {
        let rel = *gp - face.mid();
        let t = rel.dot(face.tangent);
        let off = rel.dot(face.out).abs();
        if off > 3.0 || t.abs() > face.len * 0.5 + 1.0 {
            continue;
        }
        let mut next = Vec::new();
        for (a, b) in runs {
            if t - half > a {
                next.push((a, (t - half).min(b)));
            }
            if t + half < b {
                next.push(((t + half).max(a), b));
            }
        }
        runs = next;
    }
    runs.into_iter().filter(|(a, b)| b - a > 1.0).collect()
}

/// Iron railing along a face between two offsets along it.
fn railing(g: &mut Geometry, c: &Ctx, face: &Face, t0: f32, t1: f32) {
    let f = &c.frame;
    let (t0, t1) = (t0.min(t1), t0.max(t1));
    if t1 - t0 < 1.0 {
        return;
    }
    let dark = [0.08, 0.08, 0.09];
    let n = ((t1 - t0) / 2.4).ceil().max(1.0) as i32;
    for k in 0..=n {
        let t = t0 + (t1 - t0) * k as f32 / n as f32;
        let p = face.at(t, -0.2);
        f.bx(g, Matte, p.x, 0.6, p.y, Vec3::new(0.1, 1.2, 0.1), dark);
    }
    let mid = (t0 + t1) * 0.5;
    face.bx(g, f, Matte, mid, 1.15, -0.2, Vec3::new(t1 - t0, 0.05, 0.05), dark);
    face.bx(g, f, Matte, mid, 0.35, -0.2, Vec3::new(t1 - t0, 0.05, 0.05), dark);
}

fn broken_bench(g: &mut Geometry, w: Vec2, yaw: f32, wood: [f32; 3], metal: [f32; 3]) {
    let (s, co) = yaw.sin_cos();
    // Seat slats, one gone; one leg; no back.
    g.part(Wood).boxed(Vec3::new(w.x, 0.42, w.y), Vec3::new(1.8, 0.06, 0.22), yaw, rgba(mul(wood, 0.8)));
    let o = Vec2::new(co, s) * 0.8;
    g.part(Matte).boxed(Vec3::new(w.x + o.x, 0.22, w.y + o.y), Vec3::new(0.08, 0.44, 0.5), yaw, rgba(metal));
    g.part(Matte).boxed(Vec3::new(w.x - o.x, 0.12, w.y - o.y), Vec3::new(0.08, 0.24, 0.5), yaw + 0.5, rgba(metal));
}

// ── trees ────────────────────────────────────────────────────────────────────

fn trees(g: &mut Geometry, rng: &mut impl Rng, park: &Park, pal: &Palette) {
    let area = nd_sim::zoning::polygon_area(&park.lot).abs();
    let (spacing, cap): (f32, usize) = match park.tier {
        Tier::Low => (13.0, 3 + (area / 1500.0) as usize),
        Tier::Mid => (7.0, (area / 55.0) as usize + 2),
        Tier::High => (7.5, (area / 65.0) as usize + 2),
    };
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    // Formal avenues: trees flanking the paths on dear land.
    if park.tier == Tier::High {
        for path in &park.layout.paths {
            for seg in path.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let len = a.distance(b);
                if len < 10.0 {
                    continue;
                }
                let d = (b - a) / len;
                let right = Vec2::new(-d.y, d.x);
                let mut s = 5.0;
                while s < len - 4.0 {
                    for side in [-1.0f32, 1.0] {
                        let w = a + d * s + right * side * (park.path_width * 0.5 + 2.4);
                        if park.inside_world(w, 2.6) && park.clear_of_features(w, 1.5) && park.clear_of_benches(w, 1.6) && placed.iter().all(|(p, _)| p.distance(w) > 5.0) && park.path_distance(w) > park.path_width * 0.5 + 1.6 {
                            crate::climate::climate_tree(g, pal, w, 5.5, 2.4, mul(pal.foliage, 0.95));
                            placed.push((w, 2.4));
                        }
                    }
                    s += 7.5;
                }
            }
        }
    }
    // Then fill the lawns from a jittered, shuffled grid.
    let (lo, hi) = bounds(&park.lot);
    let step = 3.0;
    let mut candidates: Vec<Vec2> = Vec::new();
    let mut v = lo.y + 1.0;
    while v < hi.y {
        let mut u = lo.x + 1.0;
        while u < hi.x {
            candidates.push(Vec2::new(u + rng.random_range(-1.2..1.2), v + rng.random_range(-1.2..1.2)));
            u += step;
        }
        v += step;
    }
    for i in (1..candidates.len()).rev() {
        let j = rng.random_range(0..=i);
        candidates.swap(i, j);
    }
    let mut count = 0;
    for w in candidates {
        if count >= cap {
            break;
        }
        let (kind, h, canopy) = match park.tier {
            Tier::Low => {
                if rng.random_bool(0.6) {
                    (0, rng.random_range(3.5..6.0), 1.2)
                } else {
                    (1, rng.random_range(2.5..4.0), rng.random_range(1.6..2.4))
                }
            }
            Tier::Mid => (1, rng.random_range(4.0..7.5), rng.random_range(2.6..4.2)),
            Tier::High => match rng.random_range(0..4) {
                0 => (2, rng.random_range(6.0..9.0), rng.random_range(2.0..2.8)),
                1 => (3, rng.random_range(6.0..8.5), 2.6),
                _ => (1, rng.random_range(5.0..8.0), rng.random_range(3.0..4.5)),
            },
        };
        // Room each kind needs to the lot edge: a palm's fronds reach 3.4 m.
        let reach = match kind {
            0 => 1.6,
            2 => canopy + 0.4,
            3 => 3.6,
            _ => canopy * 0.5 + 0.6,
        };
        if !park.inside_world(w, reach) || park.path_distance(w) < park.path_width * 0.5 + canopy * 0.6 + 0.6 || !park.clear_of_features(w, canopy * 0.5 + 1.0) || !park.clear_of_benches(w, 1.4) {
            continue;
        }
        if placed.iter().any(|(p, pc)| p.distance(w) < spacing.min(canopy + pc + 1.0).max(spacing * 0.6)) {
            continue;
        }
        let (tk, leaf) = match kind {
            0 => (TreeKind::Dead, pal.foliage),
            2 => (TreeKind::Conifer, mul(pal.foliage, 0.7)),
            3 => (TreeKind::Palm, mul(pal.foliage, 1.1)),
            _ => (TreeKind::Round, mul(pal.foliage, rng.random_range(0.8..1.15))),
        };
        if tk == TreeKind::Round {
            crate::climate::climate_tree(g, pal, w, h, canopy, leaf);
        } else {
            tree_kind(g, w, 0.0, tk, h, canopy, if tk == TreeKind::Dead { mul(pal.bark, 0.8) } else { pal.bark }, leaf);
        }
        placed.push((w, canopy));
        count += 1;
    }
}

// ── features ─────────────────────────────────────────────────────────────────

/// Features are planned by the sim to fit their radius on the lot; what each
/// one adds around that radius (paving, tables, a booth, steps) is checked
/// against the lot here so nothing lands on the neighbours.
fn feature(g: &mut Geometry, rng: &mut impl Rng, park: &Park, s: &ParkFeatureSlot, pal: &Palette) {
    let (w, r, yaw) = (s.pos, s.radius, s.heading);
    match s.kind {
        ParkFeature::Court => court(g, rng, park, w, r, yaw, park.tier, pal),
        ParkFeature::Skatepark => skatepark(g, w, r, yaw, pal),
        ParkFeature::Playground => playground(g, rng, w, r, yaw, park.tier, pal),
        ParkFeature::Pond => water(g, rng, w, r, park.tier, pal, false),
        ParkFeature::Lake => water(g, rng, w, r, park.tier, pal, true),
        ParkFeature::Fountain => fountain(g, park, w, r, pal),
        ParkFeature::Bandstand => bandstand(g, park, w, r, yaw, pal),
        ParkFeature::Carousel => carousel(g, rng, park, w, r, pal),
        ParkFeature::FerrisWheel => ferris_wheel(g, park, w, r, yaw, pal),
        ParkFeature::Kiosk => kiosk(g, rng, park, w, r, yaw, pal),
        ParkFeature::FlowerBed => flower_bed(g, rng, w, r, pal),
        ParkFeature::Statue => statue(g, park, w, r, yaw, pal),
        ParkFeature::DogRun => dog_run(g, w, r, yaw, pal),
        ParkFeature::Lawn => g.part(Foliage).ground_polygon(&circle(w, r, 16, 0.0), 0.045, rgba(mul(pal.foliage, 1.15))),
    }
}

fn circle(w: Vec2, r: f32, n: usize, rot: f32) -> Vec<Vec2> {
    (0..n).map(|i| w + Vec2::from_angle(rot + i as f32 / n as f32 * TAU) * r).collect()
}

/// A rectangle's corners in world space, `hw` along the heading and `hd` across.
fn rect_world(w: Vec2, hw: f32, hd: f32, yaw: f32) -> [Vec2; 4] {
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    [w - d * hw - n * hd, w + d * hw - n * hd, w + d * hw + n * hd, w - d * hw + n * hd]
}

fn rail(g: &mut Geometry, a: Vec2, b: Vec2, y: f32, thick: f32, color: [f32; 3]) {
    g.part(Matte).beam(Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y), thick, rgba(color));
}

#[allow(clippy::too_many_arguments)]
fn court(g: &mut Geometry, rng: &mut impl Rng, park: &Park, w: Vec2, r: f32, yaw: f32, tier: Tier, pal: &Palette) {
    let (hw, hd) = (r * 0.92, r * 0.55);
    let worn = if tier == Tier::Low { 0.8 } else { 1.0 };
    g.part(Asphalt).boxed(Vec3::new(w.x, 0.03, w.y), Vec3::new(hw * 2.0, 0.06, hd * 2.0), yaw, rgba(mul(pal.asphalt, 1.35 * worn)));
    let line = mul(pal.lane_marker, 0.8 * worn);
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    let d3 = Vec3::new(d.x, 0.0, d.y);
    let n3 = Vec3::new(n.x, 0.0, n.y);
    // Painted lines: one quad each.
    let paint = |g: &mut Geometry, c: Vec2, along: Vec3, across: Vec3, len: f32| {
        g.part(Matte).decal(Vec3::new(c.x, 0.072, c.y), along, across, len, 0.08, Vec3::Y, rgba(line));
    };
    for s in [-1.0f32, 1.0] {
        paint(g, w + n * hd * s * 0.97, d3, n3, hw * 1.94);
        paint(g, w + d * hw * s * 0.97, n3, d3, hd * 1.94);
    }
    paint(g, w, n3, d3, hd * 1.94);
    g.part(Matte).cylinder(w, 1.8, 1.8, 0.065, 0.075, 16, rgba(line));
    for s in [-1.0f32, 1.0] {
        let base = w + d * (hw * s - 0.9 * s);
        let post = base + d * s * 0.9;
        if !park.inside_world(post, 0.3) {
            continue;
        }
        g.part(Matte).cylinder(post, 0.07, 0.07, 0.0, 3.3, 8, rgba(mul(pal.steel, 0.6)));
        let bent = tier == Tier::Low && s > 0.0;
        if bent {
            // Hoop gone, backboard hanging.
            g.part(Panel).boxed(Vec3::new(base.x, 2.8, base.y), Vec3::new(0.05, 1.0, 1.4), yaw + 0.4, rgba(mul(pal.plywood, 0.7)));
            continue;
        }
        g.part(Panel).boxed(Vec3::new(base.x, 3.2, base.y), Vec3::new(0.05, 1.05, 1.8), yaw, rgba([0.85, 0.85, 0.88]));
        let ring = base - d * s * 0.5;
        g.part(Matte).cylinder(ring, 0.45, 0.45, 3.02, 3.06, 12, rgba([0.95, 0.4, 0.1]));
    }
    if tier == Tier::Low {
        for _ in 0..rng.random_range(3..7) {
            let p = w + d * rng.random_range(-hw * 0.9..hw * 0.9) + n * rng.random_range(-hd * 0.9..hd * 0.9);
            let a = rng.random_range(0.0..TAU);
            g.part(Matte).decal(Vec3::new(p.x, 0.068, p.y), Vec3::new(a.cos(), 0.0, a.sin()), Vec3::new(-a.sin(), 0.0, a.cos()), rng.random_range(1.0..3.0), 0.08, Vec3::Y, rgba([0.05, 0.05, 0.05]));
        }
    }
}

fn skatepark(g: &mut Geometry, w: Vec2, r: f32, yaw: f32, pal: &Palette) {
    let (hw, hd) = (r * 0.9, r * 0.6);
    let concrete = mul(pal.concrete(0), 1.1);
    g.part(Concrete).boxed(Vec3::new(w.x, 0.05, w.y), Vec3::new(hw * 2.0, 0.1, hd * 2.0), yaw, rgba(concrete));
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    // Quarter pipes at both ends, a box and a rail in the middle.
    for s in [-1.0f32, 1.0] {
        let back = w + d * (hw - 0.2) * s;
        let front = w + d * (hw - 3.0) * s;
        let b0 = Vec3::new(front.x - n.x * hd * 0.8, 0.1, front.y - n.y * hd * 0.8);
        let b1 = Vec3::new(front.x + n.x * hd * 0.8, 0.1, front.y + n.y * hd * 0.8);
        let t0 = Vec3::new(back.x - n.x * hd * 0.8, 0.1, back.y - n.y * hd * 0.8);
        let t1 = Vec3::new(back.x + n.x * hd * 0.8, 0.1, back.y + n.y * hd * 0.8);
        g.part(Concrete).frustum(&[b0, b1, b1 + Vec3::Y * 0.05, b0 + Vec3::Y * 0.05], &[t0, t1, t1 + Vec3::Y * 1.8, t0 + Vec3::Y * 1.8], rgba(mul(concrete, 0.95)));
        g.part(Matte).beam(t0 + Vec3::Y * 1.8, t1 + Vec3::Y * 1.8, 0.08, rgba(mul(pal.steel, 0.8)));
    }
    g.part(Concrete).boxed(Vec3::new(w.x, 0.4, w.y), Vec3::new(3.0, 0.6, 1.2), yaw, rgba(mul(concrete, 0.9)));
    let ra = w + n * hd * 0.5 - d * 2.5;
    let rb = w + n * hd * 0.5 + d * 2.5;
    rail(g, ra, rb, 0.55, 0.06, mul(pal.steel, 0.8));
    for p in [ra, rb] {
        g.part(Matte).boxed(Vec3::new(p.x, 0.27, p.y), Vec3::new(0.06, 0.55, 0.06), 0.0, rgba(mul(pal.steel, 0.8)));
    }
}

fn playground(g: &mut Geometry, rng: &mut impl Rng, w: Vec2, r: f32, yaw: f32, tier: Tier, pal: &Palette) {
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    let bright = pick(rng, &pal.awning_colors);
    let bright2 = pick(rng, &pal.awning_colors);
    // Soft surface and a low fence with a gap on the path side.
    g.part(Matte).cylinder(w, r, r, 0.0, 0.06, 20, rgba(if tier == Tier::High { [0.55, 0.25, 0.2] } else { [0.72, 0.6, 0.42] }));
    for k in 0..18 {
        let a = k as f32 / 18.0 * TAU;
        let rel = (a - yaw).rem_euclid(TAU);
        if rel < 0.5 || rel > TAU - 0.5 {
            continue;
        }
        let p = w + Vec2::from_angle(a) * (r - 0.3);
        g.part(Wood).boxed(Vec3::new(p.x, 0.4, p.y), Vec3::new(0.1, 0.8, 0.1), a, rgba(pal.wood));
        let rel2 = (a + TAU / 18.0 - yaw).rem_euclid(TAU);
        if rel2 > 0.5 && rel2 < TAU - 0.5 {
            let q = w + Vec2::from_angle(a + TAU / 18.0) * (r - 0.3);
            rail(g, p, q, 0.7, 0.06, pal.wood);
        }
    }
    // Swings: an A-frame with two seats.
    let sc = w - d * (r * 0.45);
    let (sa, sb) = (sc - n * 1.8, sc + n * 1.8);
    for p in [sa, sb] {
        for side in [-0.7f32, 0.7] {
            let foot = p + d * side;
            g.part(Matte).beam(Vec3::new(foot.x, 0.0, foot.y), Vec3::new(p.x, 2.6, p.y), 0.09, rgba(bright));
        }
    }
    g.part(Matte).beam(Vec3::new(sa.x, 2.6, sa.y), Vec3::new(sb.x, 2.6, sb.y), 0.09, rgba(bright));
    for k in [-0.9f32, 0.9] {
        let p = sc + n * k;
        for side in [-0.25f32, 0.25] {
            let q = p + n * side;
            g.part(Matte).beam(Vec3::new(q.x, 2.55, q.y), Vec3::new(q.x, 0.55, q.y), 0.02, rgba([0.3, 0.3, 0.3]));
        }
        g.part(Matte).boxed(Vec3::new(p.x, 0.5, p.y), Vec3::new(0.3, 0.05, 0.5), yaw, rgba(DARK));
    }
    // Slide: a ladder up to a platform and a sloped chute down.
    let top = w + d * (r * 0.4) + n * (r * 0.25);
    let foot = top + d * 3.0;
    for side in [-0.4f32, 0.4] {
        let p = top + n * side;
        g.part(Matte).boxed(Vec3::new(p.x, 1.0, p.y), Vec3::new(0.08, 2.0, 0.08), yaw, rgba(bright2));
    }
    g.part(Panel).boxed(Vec3::new(top.x, 2.0, top.y), Vec3::new(1.0, 0.08, 1.0), yaw, rgba(bright2));
    g.part(Metal).slab(Vec3::new(top.x, 2.0, top.y), Vec3::new(foot.x, 0.3, foot.y), 0.8, 0.08, rgba([0.75, 0.76, 0.78]));
    for k in 0..4 {
        let p = top - d * 0.5;
        g.part(Matte).boxed(Vec3::new(p.x, 0.4 + k as f32 * 0.45, p.y), Vec3::new(0.05, 0.05, 0.7), yaw, rgba(bright2));
    }
    // Climbing frame, seesaw, spring riders, a sandpit.
    let cf = w + d * (r * 0.4) - n * (r * 0.3);
    for (dx, dz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let p = cf + d * dx + n * dz;
        g.part(Matte).boxed(Vec3::new(p.x, 1.1, p.y), Vec3::new(0.07, 2.2, 0.07), yaw, rgba(bright));
    }
    for y in [1.0f32, 2.2] {
        for s in [-1.0f32, 1.0] {
            rail(g, cf + d * s - n, cf + d * s + n, y, 0.06, bright);
            rail(g, cf - d + n * s, cf + d + n * s, y, 0.06, bright);
        }
    }
    let ss = w - n * (r * 0.55);
    g.part(Matte).boxed(Vec3::new(ss.x, 0.3, ss.y), Vec3::new(0.3, 0.6, 0.3), yaw, rgba(DARK));
    g.part(Wood).slab(Vec3::new(ss.x - d.x * 1.5, 0.2, ss.y - d.y * 1.5), Vec3::new(ss.x + d.x * 1.5, 0.95, ss.y + d.y * 1.5), 0.3, 0.06, rgba(bright2));
    for k in [-1.0f32, 1.0] {
        let p = w + n * (r * 0.55) + d * k * 1.2;
        g.part(Matte).cylinder(p, 0.12, 0.12, 0.0, 0.45, 6, rgba(mul(pal.steel, 0.7)));
        g.part(Matte).boxed(Vec3::new(p.x, 0.65, p.y), Vec3::new(0.8, 0.35, 0.35), yaw + k, rgba(pick(rng, &pal.awning_colors)));
    }
    if r > 5.0 {
        let sp = w + d * (r * 0.1) + n * (r * 0.55);
        g.part(Dirt).boxed(Vec3::new(sp.x, 0.08, sp.y), Vec3::new(2.4, 0.16, 2.4), yaw, rgba([0.8, 0.72, 0.52]));
    }
}

fn water(g: &mut Geometry, rng: &mut impl Rng, w: Vec2, r: f32, tier: Tier, pal: &Palette, lake: bool) {
    let n = if lake { 18 } else { 12 };
    let radii: Vec<f32> = (0..n).map(|_| r * rng.random_range(0.78..1.0)).collect();
    let rot = rng.random_range(0.0..TAU);
    let shape = |k: f32| -> Vec<Vec2> { (0..n).map(|i| w + Vec2::from_angle(rot + i as f32 / n as f32 * TAU) * (radii[i] * k)).collect() };
    let rim = shape(1.0);
    let water_poly = shape(0.93);
    if tier == Tier::Low {
        g.part(Dirt).ground_polygon(&rim, 0.06, rgba(mul(pal.dirt, 0.6)));
    } else {
        g.part(Stone).ground_polygon(&rim, 0.08, rgba(mul(pal.stone, 1.25)));
    }
    g.part(Glow).ground_polygon(&water_poly, 0.1, rgba(mul(pal.water, if tier == Tier::High { 0.8 } else { 0.65 })));
    // Reeds and lily pads.
    for i in 0..n {
        if rng.random_bool(0.45) {
            let p = w + Vec2::from_angle(rot + i as f32 / n as f32 * TAU) * (radii[i] * 0.88);
            for _ in 0..3 {
                let q = p + Vec2::new(rng.random_range(-0.4..0.4), rng.random_range(-0.4..0.4));
                g.part(Foliage).boxed(Vec3::new(q.x, 0.6, q.y), Vec3::new(0.06, 1.2, 0.06), 0.0, rgba(mul(pal.foliage_dry, 1.1)));
            }
        }
    }
    for _ in 0..(if lake { 8 } else { 4 }) {
        let p = w + Vec2::from_angle(rng.random_range(0.0..TAU)) * (r * rng.random_range(0.2..0.7));
        g.part(Foliage).cylinder(p, 0.35, 0.35, 0.1, 0.13, 7, rgba(mul(pal.foliage, 0.9)));
    }
    if lake {
        // Jetty with a boat tied up, and an island with a tree.
        let a = rot + 0.3;
        let edge = w + Vec2::from_angle(a) * (r * 0.95);
        let end = w + Vec2::from_angle(a) * (r * 0.45);
        g.part(Wood).slab(Vec3::new(edge.x, 0.5, edge.y), Vec3::new(end.x, 0.5, end.y), 1.4, 0.12, rgba(pal.wood));
        for t in [0.2f32, 0.6, 1.0] {
            let p = edge.lerp(end, t);
            for s in [-0.6f32, 0.6] {
                let q = p + Vec2::from_angle(a).perp() * s;
                g.part(Wood).cylinder(q, 0.08, 0.08, 0.0, 0.7, 6, rgba(mul(pal.wood, 0.8)));
            }
        }
        let boat = end + Vec2::from_angle(a).perp() * 1.5;
        g.part(Wood).boxed(Vec3::new(boat.x, 0.25, boat.y), Vec3::new(1.1, 0.35, 3.0), a, rgba(mul(pick(rng, &pal.awning_colors), 0.9)));
        let island = w + Vec2::from_angle(a + PI) * (r * 0.45);
        g.part(Dirt).cylinder(island, 2.2, 2.2, 0.1, 0.28, 10, rgba(pal.dirt));
        crate::climate::climate_tree(g, pal, island, 5.0, 2.6, pal.foliage);
    }
}

fn fountain(g: &mut Geometry, park: &Park, w: Vec2, r: f32, pal: &Palette) {
    let stone = mul(pal.stone, 1.4);
    let apron = (r + 1.6).min(park.room(w) - 0.2);
    if apron > r {
        g.part(Paving).ground_polygon(&circle(w, apron, 20, 0.0), 0.045, rgba(mul(pal.paving, 1.2)));
    }
    g.part(Stone).cylinder(w, r, r, 0.0, 0.7, 20, rgba(stone));
    g.part(Glow).cylinder(w, r - 0.35, r - 0.35, 0.7, 0.74, 20, rgba(mul(pal.water, 0.8)));
    g.part(Stone).cylinder(w, r * 0.38, r * 0.32, 0.7, 1.7, 12, rgba(stone));
    g.part(Glow).cylinder(w, r * 0.3, r * 0.3, 1.7, 1.74, 12, rgba(mul(pal.water, 0.9)));
    g.part(Stone).cylinder(w, r * 0.16, r * 0.12, 1.7, 2.5, 10, rgba(stone));
    let jet = mul(pal.water, 1.5);
    g.part(Glow).boxed(Vec3::new(w.x, 3.2, w.y), Vec3::new(0.14, 1.6, 0.14), 0.0, rgba(jet));
    for k in 0..6 {
        let p = w + Vec2::from_angle(k as f32 / 6.0 * TAU) * (r * 0.6);
        g.part(Glow).boxed(Vec3::new(p.x, 1.3, p.y), Vec3::new(0.08, 1.2, 0.08), 0.0, rgba(jet));
    }
}

fn bandstand(g: &mut Geometry, park: &Park, w: Vec2, r: f32, yaw: f32, pal: &Palette) {
    let stone = mul(pal.stone, 1.3);
    g.part(Stone).cylinder(w, r, r, 0.0, 0.8, 8, rgba(stone));
    // Steps on the path side.
    let step = w + Vec2::from_angle(yaw) * (r + 0.4);
    if park.inside_world(step, 0.7) {
        g.part(Stone).boxed(Vec3::new(step.x, 0.2, step.y), Vec3::new(2.4, 0.4, 1.2), yaw, rgba(stone));
    }
    let mut cols: Vec<Vec3> = Vec::new();
    for k in 0..8 {
        let p = w + Vec2::from_angle(k as f32 / 8.0 * TAU + yaw + PI / 8.0) * (r * 0.85);
        g.part(Panel).cylinder(p, 0.14, 0.12, 0.8, 4.0, 8, rgba([0.9, 0.9, 0.86]));
        cols.push(Vec3::new(p.x, 4.0, p.y));
    }
    let wf = Frame::world();
    for k in 0..8 {
        let (a, b) = (cols[k], cols[(k + 1) % 8]);
        if k != 3 {
            g.part(Matte).beam(Vec3::new(a.x, 1.7, a.z), Vec3::new(b.x, 1.7, b.z), 0.05, rgba([0.2, 0.2, 0.22]));
        }
        string_lights(g, &wf, a - Vec3::Y * 0.3, b - Vec3::Y * 0.3, 6, mul(pal.window_warm, 1.2));
    }
    g.part(Metal).frustum(&MeshData::ngon(w, r * 1.08, 8, yaw + PI / 8.0, 4.0), &MeshData::ngon(w, 0.3, 8, yaw + PI / 8.0, 6.2), rgba(mul(pal.metal_paint(0), 0.9)));
    g.part(Glow).boxed(Vec3::new(w.x, 6.5, w.y), Vec3::new(0.3, 0.6, 0.3), 0.0, rgba(mul(pal.lamp, 0.9)));
}

fn carousel(g: &mut Geometry, rng: &mut impl Rng, park: &Park, w: Vec2, r: f32, pal: &Palette) {
    let a = pick(rng, &pal.awning_colors);
    let b = pick(rng, &pal.awning_colors);
    let apron = (r + 1.2).min(park.room(w) - 0.2);
    if apron > r {
        g.part(Paving).ground_polygon(&circle(w, apron, 16, 0.0), 0.045, rgba(mul(pal.paving, 1.1)));
    }
    g.part(Wood).cylinder(w, r, r, 0.0, 0.5, 16, rgba(mul(pal.wood, 1.1)));
    g.part(Metal).cylinder(w, 0.45, 0.45, 0.5, 4.6, 10, rgba(pal.metal_paint(0)));
    // Striped canopy from alternating wedges.
    for k in 0..12 {
        let a0 = k as f32 / 12.0 * TAU;
        let a1 = (k + 1) as f32 / 12.0 * TAU;
        let bottom = [Vec3::new(w.x, 4.6, w.y), Vec3::new(w.x + a0.cos() * r * 1.1, 4.6, w.y + a0.sin() * r * 1.1), Vec3::new(w.x + a1.cos() * r * 1.1, 4.6, w.y + a1.sin() * r * 1.1)];
        let top = [Vec3::new(w.x, 6.6, w.y), Vec3::new(w.x + a0.cos() * 0.3, 6.6, w.y + a0.sin() * 0.3), Vec3::new(w.x + a1.cos() * 0.3, 6.6, w.y + a1.sin() * 0.3)];
        g.part(Tarp).frustum(&bottom, &top, rgba(if k % 2 == 0 { a } else { b }));
        let p = w + Vec2::from_angle(a0) * (r * 1.08);
        g.part(Glow).boxed(Vec3::new(p.x, 4.5, p.y), Vec3::new(0.2, 0.2, 0.2), 0.0, rgba(mul(pal.window_warm, 1.3)));
    }
    for k in 0..8 {
        let ang = k as f32 / 8.0 * TAU;
        let p = w + Vec2::from_angle(ang) * (r * 0.68);
        g.part(Metal).cylinder(p, 0.04, 0.04, 0.5, 4.6, 6, rgba([0.9, 0.85, 0.5]));
        let y = 1.4 + 0.4 * ((k % 2) as f32);
        g.part(Matte).boxed(Vec3::new(p.x, y, p.y), Vec3::new(1.3, 0.55, 0.4), ang + FRAC_PI_2, rgba(pick(rng, &pal.awning_colors)));
        let head = p + Vec2::from_angle(ang + FRAC_PI_2) * 0.75;
        g.part(Matte).boxed(Vec3::new(head.x, y + 0.45, head.y), Vec3::new(0.45, 0.4, 0.3), ang + FRAC_PI_2, rgba([0.9, 0.9, 0.88]));
    }
}

fn ferris_wheel(g: &mut Geometry, park: &Park, w: Vec2, r: f32, yaw: f32, pal: &Palette) {
    let rw = r * 0.85;
    let hub_y = rw + 2.5;
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    let steel = pal.metal_paint(1);
    g.part(Concrete).boxed(Vec3::new(w.x, 0.1, w.y), Vec3::new(rw * 1.2, 0.2, 6.0), yaw, rgba(mul(pal.concrete(0), 1.1)));
    let hub = Vec3::new(w.x, hub_y, w.y);
    let n3 = Vec3::new(n.x, 0.0, n.y);
    for side in [-1.0f32, 1.0] {
        for along in [-0.35f32, 0.35] {
            let foot = w + n * side * 2.4 + d * rw * along;
            g.part(Metal).beam(Vec3::new(foot.x, 0.0, foot.y), hub + n3 * side, 0.3, rgba(steel));
        }
    }
    g.part(Metal).tube(hub - n3 * 1.2, hub + n3 * 1.2, 0.3, 8, rgba(steel));
    let spokes = 16;
    let rim: Vec<Vec3> = (0..spokes)
        .map(|k| {
            let a = k as f32 / spokes as f32 * TAU;
            hub + Vec3::new(d.x * a.cos() * rw, a.sin() * rw, d.y * a.cos() * rw)
        })
        .collect();
    for k in 0..spokes {
        let (a, b) = (rim[k], rim[(k + 1) % spokes]);
        for side in [-0.6f32, 0.6] {
            let o = n3 * side;
            g.part(Metal).tube(a + o, b + o, 0.09, 6, rgba(steel));
            g.part(Metal).tube(hub + o, a + o, 0.06, 5, rgba(mul(steel, 0.9)));
        }
        // Gondola hanging from the rim.
        let gp = a - Vec3::Y * 0.9;
        g.part(Matte).boxed(gp, Vec3::new(1.3, 1.1, 1.0), yaw, rgba(pal.awning(k)));
        g.part(Glow).boxed(a, Vec3::new(0.22, 0.22, 0.22), 0.0, rgba(mul(pal.window_warm, 1.4)));
    }
    // Ticket booth, where the lot has room for it.
    let booth = w + n * 4.5 + d * rw * 0.5;
    if park.inside_world(booth, 1.4) {
        g.part(Panel).boxed(Vec3::new(booth.x, 1.2, booth.y), Vec3::new(2.0, 2.4, 1.6), yaw, rgba(mul(pal.panel(0), 0.9)));
        g.part(Glow).decal(Vec3::new(booth.x, 1.4, booth.y) - n3 * 0.81, Vec3::new(d.x, 0.0, d.y), Vec3::Y, 1.2, 0.6, -n3, rgba(mul(pal.window_warm, 0.8)));
    }
}

fn kiosk(g: &mut Geometry, rng: &mut impl Rng, park: &Park, w: Vec2, r: f32, yaw: f32, pal: &Palette) {
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    let apron = (r + 1.0).min(park.room(w) - 0.2);
    if apron > r * 0.5 {
        g.part(Paving).ground_polygon(&circle(w, apron, 14, 0.0), 0.045, rgba(mul(pal.paving, 1.05)));
    }
    let k = w - d * (r * 0.35);
    g.part(Glass).boxed(Vec3::new(k.x, 1.4, k.y), Vec3::new(3.0, 2.8, 2.4), yaw, rgba(pick(rng, &pal.glass)));
    g.part(Glow).boxed(Vec3::new(k.x, 1.5, k.y), Vec3::new(2.4, 1.3, 1.9), yaw, rgba(mul(pal.window_warm, 0.6)));
    g.part(Panel).boxed(Vec3::new(k.x, 2.95, k.y), Vec3::new(3.4, 0.3, 2.8), yaw, rgba(mul(pal.steel, 0.9)));
    g.part(Matte).boxed(Vec3::new(k.x, 2.55, k.y) + Vec3::new(d.x, 0.0, d.y) * 1.52, Vec3::new(0.05, 0.5, 2.0), yaw, rgba(pick(rng, &pal.led_colors)));
    // Café tables with stools and a parasol each.
    for (i, (du, dv)) in [(0.5f32, -1.1f32), (0.5, 1.1), (0.95, 0.0)].iter().enumerate() {
        let t = w + d * (r * du) + n * (r * dv);
        if !park.inside_world(t, 1.3) {
            continue; // the parasol would hang over the fence
        }
        g.part(Metal).cylinder(t, 0.05, 0.05, 0.0, 0.75, 6, rgba(mul(pal.steel, 0.7)));
        g.part(Metal).cylinder(t, 0.5, 0.5, 0.75, 0.8, 10, rgba(mul(pal.steel, 0.8)));
        for s in 0..3 {
            let p = t + Vec2::from_angle(s as f32 / 3.0 * TAU + i as f32) * 0.8;
            g.part(Metal).cylinder(p, 0.18, 0.18, 0.42, 0.47, 8, rgba(mul(pal.steel, 0.75)));
            g.part(Metal).cylinder(p, 0.03, 0.03, 0.0, 0.42, 5, rgba(mul(pal.steel, 0.6)));
        }
        g.part(Metal).cylinder(t, 0.03, 0.03, 0.8, 2.4, 5, rgba(mul(pal.steel, 0.6)));
        g.part(Tarp).frustum(&MeshData::ngon(t, 1.2, 8, 0.0, 2.1), &MeshData::ngon(t, 0.05, 8, 0.0, 2.5), rgba(pick(rng, &pal.awning_colors)));
    }
}

fn flower_bed(g: &mut Geometry, rng: &mut impl Rng, w: Vec2, r: f32, pal: &Palette) {
    g.part(Stone).cylinder(w, r, r, 0.0, 0.35, 12, rgba(mul(pal.stone, 1.3)));
    g.part(Dirt).cylinder(w, r - 0.2, r - 0.2, 0.35, 0.42, 12, rgba(mul(pal.dirt, 0.8)));
    g.part(Foliage).cylinder(w, r - 0.35, r - 0.35, 0.42, 0.62, 12, rgba(mul(pal.foliage, 0.95)));
    let colors: Vec<[f32; 3]> = pal.awning_colors.iter().chain(pal.led_colors.iter()).copied().collect();
    for _ in 0..(r * 6.0) as i32 {
        let p = w + Vec2::from_angle(rng.random_range(0.0..TAU)) * rng.random_range(0.0..r - 0.5);
        g.part(Matte).boxed(Vec3::new(p.x, 0.72, p.y), Vec3::new(0.22, 0.2, 0.22), rng.random_range(0.0..TAU), rgba(mul(pick(rng, &colors), 1.2)));
    }
}

fn statue(g: &mut Geometry, park: &Park, w: Vec2, r: f32, yaw: f32, pal: &Palette) {
    let stone = mul(pal.stone, 1.5);
    let apron = (r + 1.0).min(park.room(w) - 0.2);
    if apron > r * 0.5 {
        g.part(Paving).ground_polygon(&circle(w, apron, 12, 0.0), 0.045, rgba(mul(pal.paving, 1.15)));
    }
    g.part(Stone).boxed(Vec3::new(w.x, 0.7, w.y), Vec3::new(1.6, 1.4, 1.6), yaw, rgba(stone));
    g.part(Stone).boxed(Vec3::new(w.x, 1.5, w.y), Vec3::new(1.2, 0.2, 1.2), yaw, rgba(mul(stone, 1.05)));
    let bronze = pal.bronze;
    g.part(Metal).frustum(&MeshData::ngon(w, 0.45, 8, yaw, 1.6), &MeshData::ngon(w, 0.3, 8, yaw, 3.3), rgba(bronze));
    g.part(Metal).cylinder(w, 0.22, 0.2, 3.3, 3.75, 8, rgba(bronze));
    let arm = w + Vec2::from_angle(yaw) * 0.5;
    g.part(Metal).beam(Vec3::new(w.x, 3.0, w.y), Vec3::new(arm.x, 3.4, arm.y), 0.16, rgba(bronze));
    let up = w - Vec2::from_angle(yaw) * 1.0;
    g.part(Glow).boxed(Vec3::new(up.x, 0.08, up.y), Vec3::new(0.4, 0.06, 0.4), 0.0, rgba(mul(pal.lamp, 0.8)));
}

fn dog_run(g: &mut Geometry, w: Vec2, r: f32, yaw: f32, pal: &Palette) {
    let corners = rect_world(w, r * 0.9, r * 0.6, yaw);
    g.part(Dirt).ground_polygon(&corners, 0.04, rgba(mul(pal.dirt, 1.05)));
    let dark = [0.12, 0.12, 0.13];
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let len = a.distance(b);
        let n = (len / 2.5).ceil().max(1.0) as i32;
        for k in 0..=n {
            let p = a.lerp(b, k as f32 / n as f32);
            g.part(Matte).boxed(Vec3::new(p.x, 0.6, p.y), Vec3::new(0.07, 1.2, 0.07), yaw, rgba(dark));
        }
        // A gate gap in the path-side fence.
        if i == 1 {
            rail(g, a, a.lerp(b, 0.4), 1.1, 0.04, dark);
            rail(g, a.lerp(b, 0.6), b, 1.1, 0.04, dark);
        } else {
            rail(g, a, b, 1.1, 0.04, dark);
        }
        rail(g, a, b, 0.5, 0.04, dark);
    }
    let d = Vec2::from_angle(yaw);
    let n = d.perp();
    for k in [-0.4f32, 0.4] {
        let p = w + d * (r * k);
        rail(g, p - n * 0.8, p + n * 0.8, 0.45, 0.05, mul(pal.wood, 1.1));
    }
    let tunnel = w + n * (r * 0.3);
    g.part(Matte).tube(Vec3::new(tunnel.x - d.x * 1.5, 0.45, tunnel.y - d.y * 1.5), Vec3::new(tunnel.x + d.x * 1.5, 0.45, tunnel.y + d.y * 1.5), 0.45, 10, rgba(mul(pal.awning(0), 0.9)));
    let bowl = w - n * (r * 0.4);
    g.part(Metal).cylinder(bowl, 0.25, 0.25, 0.0, 0.12, 8, rgba(mul(pal.steel, 0.9)));
}
