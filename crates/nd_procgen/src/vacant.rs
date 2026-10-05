//! Vacant lots: what a parcel looks like while nobody has built on it. The
//! land's value sets the mood. Cheap land lies dormant under dead grass,
//! weeds, rubbish and tyres behind a broken fence, with a tagged wall where a
//! neighbour's plot once stood. Middling land is dirt and gravel going wild:
//! grass, shrubs, rocks, the odd young tree, an estate agent's board. Dear
//! land is kept as a tidy parklet (mown lawn, a low hedge along the front, a
//! couple of young trees and a bench) while the owner waits for the right
//! offer.
//!
//! Everything is placed in world coordinates straight from the lot polygon,
//! and every piece checks the polygon before it is placed, so odd parcels
//! (triangles, slivers, concave corners) get the same treatment as a
//! rectangle without spilling onto the street.

use crate::buildings::parts::{bench_world, bin_world, coin, distance_to_boundary, graffiti_world, mix, mul, ngon, offset_poly, pick, random_in_poly, rgba, rock_world, scrub_world, tree_world, CHARCOAL, DARK};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::Lot;
use rand::Rng;
use std::f32::consts::TAU;

/// Land value below which a vacant lot is left to rot.
pub const LOW: f32 = 0.38;
/// Land value from which a vacant lot is kept as a parklet.
pub const HIGH: f32 = 0.66;

/// Geometry for a lot with no building and no park plan.
pub fn generate_vacant(lot: &Lot, land_value: f32, seed: u64, palette: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 29);
    let mut poly = lot.polygon.clone();
    if poly.len() < 3 {
        poly = lot.corners.to_vec();
    }
    if polygon_area(&poly) < 0.0 {
        poly.reverse();
    }
    let inner = offset_poly(&poly, 0.25);
    if inner.len() < 3 || polygon_area(&inner).abs() < 4.0 {
        return g;
    }
    let site = Site { inner, lot: poly, front: lot.front_edge, inward: -lot.front_dir.normalize_or_zero() };
    if land_value < LOW {
        dormant(&mut g, &site, &mut rng, palette);
    } else if land_value < HIGH {
        wild(&mut g, &site, &mut rng, palette);
    } else {
        parklet(&mut g, &site, &mut rng, palette);
    }
    g
}

/// The parcel in world coordinates.
struct Site {
    /// Exact outline, counter-clockwise.
    lot: Vec<Vec2>,
    /// Outline pulled in a little, for the ground.
    inner: Vec<Vec2>,
    /// The edge along the road.
    front: [Vec2; 2],
    /// Away from the road.
    inward: Vec2,
}

impl Site {
    fn area(&self) -> f32 {
        polygon_area(&self.lot).abs()
    }

    fn inside(&self, p: Vec2, margin: f32) -> bool {
        point_in_polygon(p, &self.lot) && distance_to_boundary(p, &self.lot) >= margin
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        self.lot.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }

    /// A random point at least `margin` from the edge, if one can be found.
    fn sample(&self, rng: &mut impl Rng, margin: f32) -> Option<Vec2> {
        random_in_poly(rng, &self.lot, margin)
    }

    /// Jittered grid of points inside the lot, `margin` from the edge.
    fn grid(&self, rng: &mut impl Rng, step: f32, margin: f32) -> Vec<Vec2> {
        let (lo, hi) = self.bounds();
        let mut out = Vec::new();
        let mut y = lo.y + step * 0.5;
        while y < hi.y {
            let mut x = lo.x + step * 0.5;
            while x < hi.x {
                let p = Vec2::new(x + rng.random_range(-0.4..0.4) * step, y + rng.random_range(-0.4..0.4) * step);
                if self.inside(p, margin) {
                    out.push(p);
                }
                x += step;
            }
            y += step;
        }
        out
    }

    /// Edges that are not the frontage, with their inward normals.
    fn side_edges(&self) -> Vec<(Vec2, Vec2, Vec2)> {
        let n = self.lot.len();
        let fm = (self.front[0] + self.front[1]) * 0.5;
        let c = nd_sim::zoning::polygon_centroid(&self.lot);
        (0..n)
            .filter_map(|i| {
                let a = self.lot[i];
                let b = self.lot[(i + 1) % n];
                if ((a + b) * 0.5).distance(fm) < 0.5 {
                    return None;
                }
                let d = (b - a).normalize_or_zero();
                // Counter-clockwise outline: the interior is on the left.
                let mut inward = Vec2::new(-d.y, d.x);
                if (c - (a + b) * 0.5).dot(inward) < 0.0 {
                    inward = -inward;
                }
                Some((a, b, inward))
            })
            .collect()
    }

    /// The frontage with its inward normal and a safe inset for a fence or
    /// hedge: along the edge but pulled in so it stays on the lot.
    fn front_line(&self, inset: f32) -> (Vec2, Vec2) {
        let a = self.front[0] + self.inward * inset;
        let b = self.front[1] + self.inward * inset;
        // Shorten the ends a little so corner lots do not poke out.
        let d = (b - a).normalize_or_zero();
        (a + d * 0.6, b - d * 0.6)
    }
}

fn bx(g: &mut Geometry, s: crate::surface::Surface, p: Vec2, y: f32, size: Vec3, yaw: f32, col: [f32; 3]) {
    g.part(s).boxed(Vec3::new(p.x, y, p.y), size, yaw, rgba(col));
}

/// A flat blob on the ground, kept off the lot edge.
fn patch(g: &mut Geometry, site: &Site, s: crate::surface::Surface, p: Vec2, r: f32, y: f32, rng: &mut impl Rng, col: [f32; 3]) {
    let r = r.min(distance_to_boundary(p, &site.lot) - 0.1);
    if r < 0.4 {
        return;
    }
    g.part(s).ground_polygon(&ngon(p.x, p.y, r, 7, rng.random_range(0.0..TAU)), y, rgba(col));
}

/// Chain-link fence from `a` to `b`. `broken` (0..1) removes posts and
/// panels; a missing panel sometimes lies on the ground just inside.
#[allow(clippy::too_many_arguments)]
fn fence_run(g: &mut Geometry, rng: &mut impl Rng, a: Vec2, b: Vec2, inward: Vec2, h: f32, broken: f32, post: [f32; 3], mesh: [f32; 3]) {
    let len = a.distance(b);
    if len < 1.0 {
        return;
    }
    let t = (b - a) / len;
    let yaw = t.y.atan2(t.x);
    let bays = (len / 2.6).ceil().max(1.0) as i32;
    let bay = len / bays as f32;
    for i in 0..=bays {
        if coin(rng, broken * 0.25) {
            continue;
        }
        let p = a + t * (i as f32 * bay);
        bx(g, Matte, p, h * 0.5, Vec3::new(0.08, h, 0.08), yaw, post);
    }
    for i in 0..bays {
        let m = a + t * ((i as f32 + 0.5) * bay);
        if coin(rng, broken * 0.5) {
            if rng.random_bool(0.5) {
                let q = m + inward * rng.random_range(0.8..1.6);
                bx(g, Matte, q, 0.03, Vec3::new(bay * 0.9, 0.03, h * 0.9), yaw + rng.random_range(-0.4..0.4), mesh);
            }
            continue;
        }
        bx(g, Matte, m, h * 0.3, Vec3::new(bay, 0.04, 0.04), yaw, post);
        bx(g, Matte, m, h * 0.92, Vec3::new(bay, 0.04, 0.04), yaw, post);
        bx(g, Matte, m, h * 0.5, Vec3::new(bay, h * 0.9, 0.015), yaw, mesh);
    }
}

/// Clipped hedge from `a` to `b`, `w` thick, standing on the inward side.
fn hedge_run(g: &mut Geometry, a: Vec2, b: Vec2, inward: Vec2, h: f32, w: f32, col: [f32; 3]) {
    let len = a.distance(b);
    if len < 0.8 {
        return;
    }
    let t = (b - a) / len;
    let m = (a + b) * 0.5 + inward * (w * 0.5 + 0.1);
    bx(g, Foliage, m, h * 0.5, Vec3::new(len, h, w), t.y.atan2(t.x), col);
}

/// Estate agent's board on a post, facing the road just inside the frontage.
fn sale_board(g: &mut Geometry, site: &Site, rng: &mut impl Rng, pal: &Palette, bright: bool) {
    let (a, b) = site.front_line(1.2);
    let len = a.distance(b);
    if len < 3.0 {
        return;
    }
    let p = a.lerp(b, rng.random_range(0.15..0.3));
    if !site.inside(p, 0.8) {
        return;
    }
    let d = (b - a) / len;
    let yaw = d.y.atan2(d.x);
    let post = if bright { mul(pal.steel, 0.8) } else { pal.bark };
    bx(g, if bright { Matte } else { Wood }, p, 1.2, Vec3::new(0.1, 2.4, 0.1), yaw, post);
    let board = if bright { pick(rng, &pal.panel) } else { mul(pal.plywood, 0.9) };
    bx(g, Panel, p, 2.0, Vec3::new(1.3, 0.9, 0.05), yaw, board);
    // A line of lettering towards the road.
    let face = p - site.inward * 0.04;
    bx(g, Matte, face, 2.1, Vec3::new(0.9, 0.16, 0.02), yaw, if bright { pick(rng, &pal.awning_colors) } else { DARK });
    bx(g, Matte, face, 1.85, Vec3::new(0.6, 0.1, 0.02), yaw, DARK);
}

// ── low value: dormant and trashy ────────────────────────────────────────────

fn dormant(g: &mut Geometry, site: &Site, rng: &mut impl Rng, pal: &Palette) {
    let area = site.area();
    g.part(Dirt).ground_polygon(&site.inner, 0.03, rgba(mul(pal.dirt, 0.78)));
    // Dead grass in patches, bare scrapes between.
    for _ in 0..((area / 50.0) as i32).clamp(3, 10) {
        if let Some(p) = site.sample(rng, 1.0) {
            let (r, k) = (rng.random_range(1.2..3.4), rng.random_range(0.7..1.05));
            patch(g, site, Foliage, p, r, 0.04, rng, mul(pal.foliage_dry, k));
        }
    }
    // Weeds.
    for _ in 0..((area / 30.0) as i32).clamp(4, 10) {
        if let Some(p) = site.sample(rng, 0.5) {
            let s = rng.random_range(0.35..1.0);
            bx(g, Foliage, p, s * 0.3, Vec3::new(s, s * 0.6, s * 0.8), rng.random_range(0.0..TAU), mul(pal.foliage_dry, rng.random_range(0.7..1.1)));
        }
    }
    // Rubbish: bags, boxes, sheets and bits of board.
    for _ in 0..((area / 40.0) as i32).clamp(3, 8) {
        if let Some(p) = site.sample(rng, 0.5) {
            let yaw = rng.random_range(0.0..TAU);
            match rng.random_range(0..5) {
                0 | 1 => bx(g, Tarp, p, 0.18, Vec3::new(rng.random_range(0.4..0.8), 0.36, rng.random_range(0.35..0.6)), yaw, if rng.random_bool(0.5) { [0.07, 0.07, 0.08] } else { pick(rng, &pal.tarp_colors) }),
                2 => bx(g, Wood, p, 0.04, Vec3::new(rng.random_range(0.6..1.4), 0.08, rng.random_range(0.5..1.0)), yaw, mul(pal.plywood, 0.75)),
                3 => bx(g, Metal, p, 0.2, Vec3::new(rng.random_range(0.4..0.9), 0.4, rng.random_range(0.4..0.7)), yaw, mul(pal.steel, 0.55)),
                _ => bx(g, Tarp, p, 0.03, Vec3::new(rng.random_range(1.0..2.2), 0.06, rng.random_range(0.8..1.6)), yaw, mul(pick(rng, &pal.tarp_colors), 0.7)),
            }
        }
    }
    // Tyres, lying flat in short stacks.
    for _ in 0..rng.random_range(1..3) {
        if let Some(p) = site.sample(rng, 0.8) {
            for k in 0..rng.random_range(1..4) {
                let q = p + Vec2::new(rng.random_range(-0.08..0.08), rng.random_range(-0.08..0.08));
                g.part(Matte).cylinder(q, 0.38, 0.38, k as f32 * 0.24, k as f32 * 0.24 + 0.22, 6, rgba(DARK));
            }
        }
    }
    // Scrap: rusted sheet and a drum or two.
    for _ in 0..rng.random_range(1..3) {
        if let Some(p) = site.sample(rng, 1.2) {
            for _ in 0..rng.random_range(2..5) {
                let q = p + Vec2::new(rng.random_range(-0.9..0.9), rng.random_range(-0.9..0.9));
                let size = Vec3::new(rng.random_range(0.5..1.6), rng.random_range(0.1..0.5), rng.random_range(0.3..1.2));
                bx(g, Rust, q, size.y * 0.5, size, rng.random_range(0.0..TAU), mul(pal.rust, rng.random_range(0.7..1.1)));
            }
            if rng.random_bool(0.6) {
                let q = p + Vec2::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0));
                if rng.random_bool(0.5) {
                    g.part(Rust).cylinder(q, 0.3, 0.3, 0.0, 0.9, 8, rgba(pal.rust));
                } else {
                    let d = Vec2::from_angle(rng.random_range(0.0..TAU));
                    g.part(Rust).pipe(q, d, 0.9, 0.3, 0.3, 8, rgba(mul(pal.rust, 0.9)));
                }
            }
        }
    }
    // A broken fence along the front, stretches of it gone.
    let (a, b) = site.front_line(0.45);
    if a.distance(b) > 3.0 {
        let broken = rng.random_range(0.4..0.9);
        fence_run(g, rng, a, b, site.inward, 1.7, broken, mul(pal.steel, 0.45), pal.fence_mesh);
    }
    // A tagged wall on one side, left over from whatever stood here.
    let sides: Vec<_> = site.side_edges().into_iter().filter(|(a, b, _)| a.distance(*b) > 6.0).collect();
    if !sides.is_empty() && rng.random_bool(0.6) {
        let (a, b, inward) = sides[rng.random_range(0..sides.len())];
        let len = a.distance(b);
        let d = (b - a) / len;
        let span = (len * rng.random_range(0.4..0.75)).min(18.0);
        let start = rng.random_range(0.4..(len - span - 0.4).max(0.41));
        let wa = a + d * start + inward * 0.35;
        let wb = wa + d * span;
        let mid = (wa + wb) * 0.5;
        if site.inside(wa, 0.1) && site.inside(wb, 0.1) {
            let h = rng.random_range(1.6..2.4);
            bx(g, Concrete, mid, h * 0.5, Vec3::new(span, h, 0.3), d.y.atan2(d.x), mul(pick(rng, &pal.concrete), 0.8));
            // The wall spans 0.2..0.5 m in from the edge. Spray the side that
            // faces into the lot (walked against the lot's direction so the
            // painted side is on the right) and, less, the side the neighbour sees.
            graffiti_world(g, rng, wb + inward * 0.5, wa + inward * 0.5, 0.05, h - 0.1, 2.5, 0.8, pal);
            graffiti_world(g, rng, wa + inward * 0.2, wb + inward * 0.2, 0.05, h - 0.1, 1.0, 0.8, pal);
        }
    }
    // A drum fire for whoever sleeps here.
    if rng.random_bool(0.3) {
        if let Some(p) = site.sample(rng, 1.5) {
            g.part(Rust).cylinder(p, 0.3, 0.3, 0.0, 0.9, 8, rgba(pal.rust));
            bx(g, Glow, p, 1.0, Vec3::new(0.3, 0.25, 0.3), 0.0, [2.0, 0.9, 0.3]);
        }
    }
}

// ── middle value: dirt and gravel going wild ─────────────────────────────────

fn wild(g: &mut Geometry, site: &Site, rng: &mut impl Rng, pal: &Palette) {
    let area = site.area();
    g.part(Dirt).ground_polygon(&site.inner, 0.03, rgba(mix(pal.dirt, pal.gravel, 0.45)));
    for _ in 0..((area / 70.0) as i32).clamp(2, 6) {
        if let Some(p) = site.sample(rng, 1.0) {
            let (r, k) = (rng.random_range(1.5..3.5), rng.random_range(1.0..1.15));
            patch(g, site, Dirt, p, r, 0.04, rng, mul(pal.gravel, k));
        }
    }
    // Wild grass, greener than the dead stuff next door.
    let grass = mix(pal.foliage_dry, pal.foliage, 0.5);
    for _ in 0..((area / 40.0) as i32).clamp(3, 10) {
        if let Some(p) = site.sample(rng, 1.0) {
            let (r, k) = (rng.random_range(1.2..3.0), rng.random_range(0.8..1.1));
            patch(g, site, Foliage, p, r, 0.045, rng, mul(grass, k));
        }
    }
    for _ in 0..((area / 30.0) as i32).clamp(3, 12) {
        if let Some(p) = site.sample(rng, 0.5) {
            let s = rng.random_range(0.4..1.0);
            bx(g, Foliage, p, s * 0.3, Vec3::new(s, s * 0.7, s * 0.8), rng.random_range(0.0..TAU), mul(grass, rng.random_range(0.8..1.15)));
        }
    }
    // Shrubs, rocks and the odd young tree on a loose grid.
    let mut spots = site.grid(rng, 5.0, 1.4);
    spots.retain(|_| rng.random_bool(0.55));
    let mut trees = 0;
    for p in spots {
        let kind = rng.random_range(0..10);
        let size: f32 = rng.random();
        let tint: f32 = rng.random();
        match kind {
            0..=4 => scrub_world(g, rng, p, 0.02, 0.6 + size * 0.7, mix(pal.foliage_dry, pal.foliage, 0.2 + tint * 0.5)),
            5..=7 => rock_world(g, rng, p, 0.0, 0.3 + size * 0.6, mul(pal.stone, 0.9 + tint * 0.3)),
            _ if trees < 2 && site.inside(p, 2.0) => {
                trees += 1;
                tree_world(g, p, rng.random_range(2.2..3.4), rng.random_range(1.8..2.8), pal.bark, mul(pal.foliage, rng.random_range(0.85..1.1)));
            }
            _ => {}
        }
    }
    // A post-and-wire boundary marker at the front corners, and a board.
    let (a, b) = site.front_line(0.5);
    if a.distance(b) > 3.0 {
        for p in [a, b] {
            if site.inside(p, 0.1) {
                bx(g, Wood, p, 0.6, Vec3::new(0.1, 1.2, 0.1), 0.0, mul(pal.bark, 1.1));
            }
        }
    }
    if rng.random_bool(0.5) {
        sale_board(g, site, rng, pal, false);
    }
}

// ── high value: a tidy parklet ───────────────────────────────────────────────

fn parklet(g: &mut Geometry, site: &Site, rng: &mut impl Rng, pal: &Palette) {
    g.part(Foliage).ground_polygon(&site.inner, 0.03, rgba(mul(pal.foliage, 1.05)));
    // Low hedge along the front with a gap in the middle for the path.
    let (a, b) = site.front_line(0.5);
    let len = a.distance(b);
    let d = (b - a).normalize_or_zero();
    let gap = if len > 7.0 { 1.0 } else { 0.0 };
    let m = (a + b) * 0.5;
    if len > 3.0 {
        let hedge = mul(pal.foliage, 0.85);
        if gap > 0.0 {
            hedge_run(g, a, m - d * gap, site.inward, 0.9, 0.7, hedge);
            hedge_run(g, m + d * gap, b, site.inward, 0.9, 0.7, hedge);
        } else {
            hedge_run(g, a, b, site.inward, 0.9, 0.7, hedge);
        }
    }
    // A paved path from the gap to a bench facing the road.
    let mut keep_clear: Vec<(Vec2, f32)> = Vec::new();
    if gap > 0.0 {
        let start = m + site.inward * 0.3;
        let mut reach = 4.0;
        while reach > 2.0 && !site.inside(start + site.inward * (reach + 1.2), 0.8) {
            reach -= 0.5;
        }
        if reach > 2.0 {
            let end = start + site.inward * reach;
            g.part(Paving).ground_strip(&[start, end + site.inward * 0.6], 1.4, 0.05, rgba(mul(pal.paving, 1.15)));
            let seat = end + site.inward * 1.0;
            // The bench's back is on its left side; turn it so that is inward.
            let yaw = (-site.inward.x).atan2(site.inward.y);
            bench_world(g, seat, yaw, pal.wood, CHARCOAL);
            keep_clear.push((seat, 2.0));
            let bin = seat + Vec2::new(-site.inward.y, site.inward.x) * 1.6;
            if site.inside(bin, 0.6) {
                bin_world(g, bin, mul(pal.metal_paint(0), 0.9));
            }
            // A low lamp by the path.
            let lamp = start + site.inward * (reach * 0.5) + Vec2::new(site.inward.y, -site.inward.x) * 1.2;
            if site.inside(lamp, 0.5) {
                bx(g, Matte, lamp, 0.6, Vec3::new(0.12, 1.2, 0.12), 0.0, CHARCOAL);
                bx(g, Glow, lamp, 1.22, Vec3::new(0.25, 0.1, 0.25), 0.0, mul(pal.lamp, 0.6));
            }
            for k in 0..=4 {
                keep_clear.push((start.lerp(end, k as f32 / 4.0), 1.4));
            }
        }
    }
    // Two or three young trees, off the path and the bench.
    let mut spots = site.grid(rng, 4.5, 2.2);
    spots.retain(|p| keep_clear.iter().all(|(c, r)| p.distance(*c) > *r));
    let want = rng.random_range(2..4);
    let mut placed = 0;
    // Spread them out: take spots far from the ones already taken.
    let mut taken: Vec<Vec2> = Vec::new();
    while placed < want && !spots.is_empty() {
        let idx = if taken.is_empty() {
            rng.random_range(0..spots.len())
        } else {
            (0..spots.len()).max_by(|&i, &j| {
                let di = taken.iter().map(|t| t.distance(spots[i])).fold(f32::MAX, f32::min);
                let dj = taken.iter().map(|t| t.distance(spots[j])).fold(f32::MAX, f32::min);
                di.total_cmp(&dj)
            }).unwrap_or(0)
        };
        let p = spots.swap_remove(idx);
        taken.push(p);
        placed += 1;
        tree_world(g, p, rng.random_range(2.8..4.2), rng.random_range(2.4..3.4), pal.bark, mul(pal.foliage, rng.random_range(0.9..1.15)));
        // A tidy ring of bark around the trunk.
        patch(g, site, Dirt, p, 0.7, 0.04, rng, mul(pal.dirt, 0.8));
    }
    // A clipped shrub or two where there is room.
    for _ in 0..rng.random_range(1..3) {
        if let Some(p) = site.sample(rng, 1.5) {
            if keep_clear.iter().all(|(c, r)| p.distance(*c) > *r) && taken.iter().all(|t| t.distance(p) > 2.0) {
                let (r, k) = (rng.random_range(0.5..0.9), rng.random_range(0.8..1.0));
                scrub_world(g, rng, p, 0.02, r, mul(pal.foliage, k));
            }
        }
    }
    sale_board(g, site, rng, pal, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{scatter_nature, TerrainConfig};
    use nd_sim::roads::RoadNetwork;
    use nd_sim::zoning::{generate_lots, Zone, ZoneId};
    use nd_sim::LandUse;
    use slotmap::SlotMap;

    /// Lots cut by straight, diagonal and curved roads, like the style sweep.
    fn many_lots() -> Vec<Lot> {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
        let curve: Vec<Vec2> = (0..=20)
            .map(|i| {
                let t = i as f32 / 20.0;
                Vec2::new(-300.0 + 600.0 * t, 80.0 + 60.0 * (t * TAU).sin())
            })
            .collect();
        roads.add_polyline(&curve, 1, 1);
        let zone = Zone { polygon: vec![Vec2::new(-300.0, -20.0), Vec2::new(300.0, -20.0), Vec2::new(300.0, 180.0), Vec2::new(-300.0, 180.0)], land_use: LandUse::Residential };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        lots.values().filter(|l| l.usable()).cloned().collect()
    }

    /// Every band dresses every odd parcel without leaving it, and stays cheap.
    #[test]
    fn vacant_lots_stay_on_their_parcel() {
        let pal = Palette::default();
        let lots = many_lots();
        assert!(lots.len() > 10);
        for (band, lv) in [("low", 0.2f32), ("mid", 0.5), ("high", 0.8)] {
            let mut worst = 0.0f32;
            let mut tris = (0usize, 0usize);
            for (i, lot) in lots.iter().enumerate() {
                let g = generate_vacant(lot, lv, i as u64 * 31 + 7, &pal);
                let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for p in &lot.polygon {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                let n = g.triangle_count();
                tris.0 += n;
                tris.1 = tris.1.max(n);
                assert!(n > 0, "{band}: lot {i} got nothing");
                for (s, m) in g.iter() {
                    assert_eq!(m.positions.len(), m.uvs.len());
                    for p in &m.positions {
                        assert!(p.iter().all(|v| v.is_finite()), "{band} {s:?} NaN");
                        assert!(p[1] > -0.6, "{band} {s:?} below ground at {p:?}");
                        let q = Vec2::new(p[0], p[2]);
                        let over = (lo - q).max(q - hi).max_element();
                        if over > worst {
                            worst = over;
                        }
                        assert!(over <= 1.0, "{band} {s:?} lot {i} overshoots {over:.2} m at {q}");
                    }
                }
            }
            eprintln!("{band}: {} lots, avg {} tris, max {} tris, worst overshoot {worst:.2} m", lots.len(), tris.0 / lots.len(), tris.1);
            assert!(tris.1 < 2500, "{band}: {} triangles on one lot", tris.1);
        }
    }

    /// The scatter respects the caller's clearance and stays within budget.
    #[test]
    fn scatter_keeps_clear_and_is_bounded() {
        let pal = Palette::default();
        let cfg = TerrainConfig::default();
        let g = scatter_nature(&cfg, &pal, 3, &|p: Vec2| p.x > 0.0);
        let tris = g.triangle_count();
        assert!(tris > 1000, "{tris} triangles is too bare");
        assert!(tris < 400_000, "{tris} triangles is too much scatter");
        for (s, m) in g.iter() {
            for p in &m.positions {
                assert!(p.iter().all(|v| v.is_finite()), "{s:?} NaN");
                assert!(p[0] > -3.0, "{s:?} placed on cleared ground at x {}", p[0]);
            }
        }
    }
}
