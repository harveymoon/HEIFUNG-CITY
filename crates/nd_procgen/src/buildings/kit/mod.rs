//! Height forms and the dense-city kit the Hong Kong, Akira and
//! Ghost-in-the-Shell styles share: footprint cells, crammed windows with
//! cages and AC boxes, signs hung out over the pavement, antenna forests,
//! sky bridges, glowing crown outlines and laundry poles.

mod fences;
mod layout;
mod props;
mod roofs;
pub(crate) use fences::*;
pub(crate) use layout::*;
pub(crate) use props::*;
pub(crate) use roofs::*;

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{point_in_polygon, polygon_area};
use rand::Rng;

/// The form a style takes for its height: a walk-up when low, a podium and
/// slab in the middle, a tower with setbacks and a crown when high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Low,
    Mid,
    High,
}

impl Form {
    pub fn of(floors: u8) -> Self {
        match floors {
            0..=4 => Form::Low,
            5..=9 => Form::Mid,
            _ => Form::High,
        }
    }
}

/// Part of a lot-local polygon inside an axis-aligned box; empty when
/// nothing worth building remains.
pub fn clip_box(poly: &[Vec2], u0: f32, u1: f32, v0: f32, v1: f32) -> Vec<Vec2> {
    use nd_sim::zoning::clip_halfplane;
    let mut p = clip_halfplane(poly, Vec2::new(u0, 0.0), Vec2::new(1.0, 0.0));
    p = clip_halfplane(&p, Vec2::new(u1, 0.0), Vec2::new(-1.0, 0.0));
    p = clip_halfplane(&p, Vec2::new(0.0, v0), Vec2::new(0.0, 1.0));
    p = clip_halfplane(&p, Vec2::new(0.0, v1), Vec2::new(0.0, -1.0));
    let p = clean_poly(&p, 0.2);
    if p.len() >= 3 && polygon_area(&p).abs() > 1.0 { p } else { Vec::new() }
}

/// Split a footprint into a jittered grid of `nu` by `nv` cells; empty
/// cells (outside a triangular lot, say) are dropped.
pub fn split_cells(rng: &mut impl Rng, poly: &[Vec2], nu: usize, nv: usize) -> Vec<Vec<Vec2>> {
    let (lo, hi) = bounds(poly);
    let mut cuts = |a: f32, b: f32, n: usize| -> Vec<f32> {
        let mut v = vec![a];
        for k in 1..n {
            let t = k as f32 / n as f32 + rng.random_range(-0.15..0.15) / n as f32;
            v.push(a + (b - a) * t);
        }
        v.push(b);
        v
    };
    let us = cuts(lo.x, hi.x, nu.max(1));
    let vs = cuts(lo.y, hi.y, nv.max(1));
    let mut out = Vec::new();
    for i in 0..us.len() - 1 {
        for j in 0..vs.len() - 1 {
            let cell = clip_box(poly, us[i], us[i + 1], vs[j], vs[j + 1]);
            if !cell.is_empty() {
                out.push(cell);
            }
        }
    }
    out
}

/// Whether a face of a sub-volume lies on the outside of `outer` (and is
/// seen) rather than against a neighbouring volume.
pub fn face_is_outer(face: &Face, outer: &[Vec2]) -> bool {
    distance_to_boundary(face.mid(), outer) < 0.6
}

/// Whether every vertex of `inner` lies in `outer` (lot-local).
pub fn poly_inside_poly(inner: &[Vec2], outer: &[Vec2]) -> bool {
    inner.iter().all(|p| point_in_polygon(*p, outer))
}

/// Crammed small windows, two triangles each: lit, dark or boarded, an AC
/// box under some and a steel security cage over a few. The cheap facade
/// for walled blocks and pencil towers, where windows run in the hundreds.
#[allow(clippy::too_many_arguments)]
pub fn dense_windows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spacing: f32, ac: f32, cages: f32) {
    let usable = face.len - 0.8;
    let count = (usable / spacing).floor() as i32;
    if count < 1 || floors < 1 {
        return;
    }
    let step = usable / count as f32;
    let (w, h) = ((step * 0.62).min(1.3), (fh * 0.45).min(1.5));
    let f = &c.frame;
    let lit = c.lit_chance();
    let (cage_s, cage_c) = outdoor_steel(c, mul(c.palette.steel, 0.55));
    let room = c.reach_out(face);
    for k in 0..floors {
        let y = y0 + k as f32 * fh + fh * 0.52;
        for i in 0..count {
            let t = -usable * 0.5 + (i as f32 + 0.5) * step;
            let r: f32 = rng.random();
            if r < c.wear * 0.25 {
                face.glaze(g, c, t, y, 0.04, w, h, Pane::Boarded);
            } else if coin(rng, lit) {
                let wc = pick(rng, &c.style.window_colors);
                let col = mul(wc, rng.random_range(0.45..0.95));
                face.glaze(g, c, t, y, 0.04, w, h, Pane::Lit(col));
            } else {
                face.glaze(g, c, t, y, 0.04, w, h, Pane::Dark);
            }
            if room > -0.3 && coin(rng, cages) {
                // Birdcage over the window: a steel box proud of the wall.
                face.bx(g, f, cage_s, t, y - 0.1, 0.3, Vec3::new(w + 0.3, h + 0.5, 0.6), cage_c);
            } else if room > -0.3 && coin(rng, ac) {
                face.bx(g, f, Metal, t + w * 0.3, y - h * 0.5 - 0.3, 0.22, Vec3::new(0.7, 0.5, 0.4), mul(c.palette.steel, 0.9));
            }
        }
    }
}

/// A Hong Kong sign hung out from the facade at right angles, lit on both
/// faces, never reaching past the lot edge by more than a little.
#[allow(clippy::too_many_arguments)]
pub fn hanging_sign(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, reach: f32, h: f32) {
    let f = &c.frame;
    let reach = reach.min(c.reach_out(face) + 0.6);
    if reach < 0.8 {
        return;
    }
    let sc = pick(rng, &c.palette.sign_colors);
    let k = 1.0 - 0.5 * c.wear * rng.random::<f32>();
    // Backing board perpendicular to the wall, an arm above it.
    let side = Face::from_edge(face.at(t, 0.0), face.at(t, reach));
    side.bx(g, f, Matte, 0.0, y, 0.0, Vec3::new(reach, h, 0.14), DARK);
    side.bx(g, f, Matte, 0.0, y + h * 0.5 + 0.12, 0.0, Vec3::new(reach + 0.2, 0.08, 0.08), CHARCOAL);
    let cells = ((reach / 0.7).floor() as i32).max(1);
    for sf in [side, side.reversed()] {
        for i in 0..cells {
            if coin(rng, 0.1 + 0.3 * c.wear) {
                continue;
            }
            let tt = -reach * 0.5 + (i as f32 + 0.5) * reach / cells as f32;
            sf.decal(g, f, Glow, tt, y, 0.09, reach / cells as f32 * 0.8, h * 0.72, mul(sc, k));
        }
    }
}

/// A forest of antennas, dishes, tanks and shacks on a roof: the skyline
/// of a self-built block.
pub fn antenna_forest(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32, density: f32) {
    let f = &c.frame;
    let area = polygon_area(poly).abs();
    let n = ((area / 18.0) * density).clamp(1.0, 14.0) as i32;
    let pal = c.palette;
    for _ in 0..n {
        let Some(p) = random_in_poly(rng, poly, 0.6) else { continue };
        match rng.random_range(0..6) {
            0..=2 => {
                let h = rng.random_range(1.5..5.0);
                f.bx(g, Matte, p.x, y + h * 0.5, p.y, Vec3::new(0.06, h, 0.06), CHARCOAL);
                f.bx(g, Matte, p.x, y + h * 0.8, p.y, Vec3::new(rng.random_range(0.4..0.9), 0.05, 0.05), CHARCOAL);
            }
            3 => dish(g, f, p, y, rng.random_range(0.4..0.7), pal.steel),
            4 => {
                // Rooftop shack of sheet and plywood.
                let s = rng.random_range(1.4..2.6);
                if !poly_inside_poly(&rect(p.x, p.y, s * 0.5 + 0.2, s * 0.4 + 0.2), poly) {
                    continue;
                }
                let (surf, col) = if rng.random_bool(0.5) { (Metal, mul(pick(rng, &pal.metal_paint), 0.7)) } else { (Rust, pal.rust) };
                f.bx(g, surf, p.x, y + 1.0, p.y, Vec3::new(s, 2.0, s * 0.8), col);
                f.bx(g, Metal, p.x, y + 2.05, p.y, Vec3::new(s + 0.3, 0.1, s * 0.8 + 0.3), mul(pal.steel, 0.6));
            }
            _ => {
                if !poly_inside_poly(&rect(p.x, p.y, 1.2, 1.2), poly) {
                    continue;
                }
                let (s, col) = if rng.random_bool(0.6) { (Rust, pal.rust) } else { (Metal, mul(pal.steel, 0.8)) };
                water_tank(g, f, p, y, rng.random_range(0.6..0.9), rng.random_range(1.2..2.0), s, col);
            }
        }
    }
}

/// A glazed bridge between two lot-local points with its floor at `y`, and
/// a lit strip along its belly.
pub fn sky_bridge(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let (wa, wb) = (f.to_world(a.x, a.y), f.to_world(b.x, b.y));
    let d = wb - wa;
    if d.length() < 1.0 {
        return;
    }
    let mid = (wa + wb) * 0.5;
    let yaw = d.y.atan2(d.x);
    g.part(Glass).boxed(Vec3::new(mid.x, y + h * 0.5, mid.y), Vec3::new(d.length(), h, w), yaw, rgba(pal.glass(1)));
    g.part(Panel).boxed(Vec3::new(mid.x, y - 0.25, mid.y), Vec3::new(d.length(), 0.5, w + 0.3), yaw, rgba(mul(pal.steel, 0.7)));
    g.part(Glow).boxed(Vec3::new(mid.x, y - 0.52, mid.y), Vec3::new(d.length(), 0.06, w * 0.3), yaw, rgba(mul(pal.led(0), 0.8)));
}


/// An open crown frame on a roof at `y`: a post `post_h` tall and
/// `post_w` square at every corner of `ring`, and a glowing outline at
/// each (height above `y`, thickness, colour) of `rings`.
#[allow(clippy::too_many_arguments)]
pub fn crown_frame(g: &mut Geometry, c: &Ctx, ring: &[Vec2], y: f32, post_h: f32, post_w: f32, s: Surface, post_col: [f32; 3], rings: &[(f32, f32, [f32; 3])]) {
    let f = &c.frame;
    for p in ring {
        f.bx(g, s, p.x, y + post_h * 0.5, p.y, Vec3::new(post_w, post_h, post_w), post_col);
    }
    for &(dy, thick, col) in rings {
        glow_outline(g, c, ring, y + dy, thick, col);
    }
}

/// Glowing line along the edges of a footprint at height `y`: crown
/// frames, terrace edges and the light lines of a lit-up skyline.
pub fn glow_outline(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, thick: f32, color: [f32; 3]) {
    for face in c.faces(poly) {
        face.bx(g, &c.frame, Glow, 0.0, y, 0.02, Vec3::new(face.len, thick, thick), color);
    }
}

/// Laundry poles poking out of windows with washing on them.
#[allow(clippy::too_many_arguments)]
pub fn laundry_poles(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, chance: f32) {
    let f = &c.frame;
    let reach = (c.reach_out(face) + 0.6).min(1.6);
    if reach < 0.8 || face.len < 3.0 {
        return;
    }
    for k in 1..floors {
        if !coin(rng, chance) {
            continue;
        }
        let t = rng.random_range(-face.len * 0.4..face.len * 0.4);
        let y = y0 + k as f32 * fh + fh * 0.2;
        face.bx(g, f, Matte, t, y, reach * 0.5, Vec3::new(0.05, 0.05, reach), CHARCOAL);
        for i in 0..2 {
            let d = reach * (0.35 + 0.4 * i as f32);
            face.bx(g, f, Tarp, t, y - 0.35, d, Vec3::new(0.05, 0.6, 0.45), mul(pick(rng, &c.palette.tarp_colors), 1.2));
        }
    }
}

/// Faceted glass crown: the top ring pulled in and lifted in a few
/// crystalline steps, each a little rotated, with a lit seam between them.
/// No needle: the tower ends in a cut gem, not a spike.
pub fn crystal_crown(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32, glass: [f32; 3]) -> f32 {
    let f = &c.frame;
    let steps = rng.random_range(2..=3);
    let mut prev = poly.to_vec();
    let mut y = y;
    let r = inradius(poly).max(1.0);
    for k in 0..steps {
        let h = (r * rng.random_range(0.35..0.6)).clamp(1.5, 7.0);
        let shrink = rng.random_range(0.62..0.8);
        let top = fit_inside(c, &rotate_poly(&scale_poly(&prev, shrink), rng.random_range(-0.25..0.25)));
        loft(g, f, Glass, &prev, &top, y, y + h, mul(glass, 1.0 + 0.15 * k as f32));
        glow_outline(g, c, &top, y + h, 0.12, mul(c.palette.window_cold, 0.9));
        prev = top;
        y += h;
    }
    // A flat cut top: a lit lantern of glass rather than a point.
    extrude(g, f, Glow, &scale_poly(&prev, 0.9), y, y + 0.15, mul(c.style.window_color(0), 0.7));
    y + 0.15
}
