//! [r13-detail] The shared small props: one bicycle, the tree family, a
//! bench, a bin, planters and pots, and the washing. Built here once and
//! used by building styles, parks, grounds and the household layer
//! (`crate::yards`). Small look differences are parameters (colour, size,
//! how fine), not copies: a style that wants a bike calls [`bicycle`].
//!
//! Two ways to place them:
//!
//! - world-placed, drawn straight into the geometry being built
//!   ([`tree`], [`bench`], [`bin`], [`planter`], [`tub_tree`]). The small
//!   street furniture keeps its own band: a bench, a bin and a bike always
//!   go to the near layer (`crate::detail::DetailBand::Near`), whoever
//!   draws them; trees stay in the base mesh (they read from afar);
//!   planters go where the caller's `g.lod(..)` puts them;
//! - in their own frame ([`bicycle_local`], [`washing_run`]), placed with a
//!   [`Pose`]: the household layer keeps each look as one mesh the renderer
//!   instances; a style appends it where it stands ([`bicycle`]).
//!
//! What stays out: architecture-bound fixtures (sills, AC units, awnings,
//! fire escapes, the frames of laundry poles and lines) are the styles'
//! own. The washing on those lines is not: a style leaves a
//! [`crate::surface::WashLine`] where washing may hang and the household
//! layer hangs it, by household and weather.

use crate::buildings::kit::{ball_profile, dome_profile, revolve, sides_for, Axis};
use crate::buildings::Frame;
use crate::color::{mul, rgba, DARK};
use crate::mesh::MeshData;
use crate::surface::{Geometry, Lod, Surface};
use glam::{Mat3, Vec2, Vec3};
use nd_sim::hash::mix as hmix;
use std::f32::consts::TAU;
use Surface::{Foliage, Matte, Metal, Tarp, Wood};

use nd_sim::hash::mix01 as h01;

// ── placing ─────────────────────────────────────────────────────────────────

/// Where a prop drawn in its own frame stands: its foot (world) and its
/// turn. The frame: +x out (from the wall), +y up, +z along the wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub at: Vec3,
    pub rot: Mat3,
}

impl Pose {
    /// Facing sim heading `yaw` (its +x along (cos, sin) on the map), as the
    /// renderer turns an instanced prop.
    pub fn new(at: Vec3, yaw: f32) -> Self {
        Pose { at, rot: Mat3::from_rotation_y(-yaw) }
    }

    /// Its +z (a bike's length) along the map direction `dir`.
    pub fn along(at: Vec3, dir: Vec2) -> Self {
        Self::new(at, dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2)
    }

    /// Draw `local` here.
    pub fn place(&self, g: &mut Geometry, local: &Geometry) {
        append_xf(g, local, self.rot, self.at);
    }
}

/// Copy `src` into `dst` turned by `rot` and moved by `off` (into the
/// layer `dst` is drawing).
pub fn append_xf(dst: &mut Geometry, src: &Geometry, rot: Mat3, off: Vec3) {
    for (s, m) in src.iter() {
        let t = dst.part(s);
        let base = t.positions.len() as u32;
        for i in 0..m.positions.len() {
            t.positions.push((rot * Vec3::from(m.positions[i]) + off).to_array());
            t.normals.push((rot * Vec3::from(m.normals[i])).normalize_or_zero().to_array());
            t.colors.push(m.colors[i]);
            t.uvs.push(m.uvs[i]);
            let tg = m.tangents.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 1.0]);
            let r = rot * Vec3::new(tg[0], tg[1], tg[2]);
            t.tangents.push([r.x, r.y, r.z, tg[3]]);
        }
        t.indices.extend(m.indices.iter().map(|i| i + base));
    }
}

// ── the bicycle ─────────────────────────────────────────────────────────────

/// How a bicycle looks: its frame colour, its size (1 a grown-up's, ~0.6 a
/// child's), how fine (the household layer's, seen from a few metres, or a
/// rack's of many), and a cargo box in front.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bike {
    pub frame: [f32; 3],
    pub scale: f32,
    pub fine: bool,
    pub cargo: Option<[f32; 3]>,
}

impl Bike {
    pub fn plain(frame: [f32; 3]) -> Self {
        Bike { frame, scale: 1.0, fine: false, cargo: None }
    }
}

/// A wheel: a tyre ring of `segs` tubes in the plane through `c` spanned
/// by `a` and `b` (unit vectors), radius `r`, and two spokes across.
#[allow(clippy::too_many_arguments)]
pub fn wheel(m: &mut MeshData, c: Vec3, a: Vec3, b: Vec3, r: f32, tyre: f32, segs: usize, col: [f32; 4], hub: [f32; 4]) {
    let p = |k: usize| c + (a * (k as f32 / segs as f32 * TAU).cos() + b * (k as f32 / segs as f32 * TAU).sin()) * r;
    for k in 0..segs {
        m.tube_capped(p(k), p(k + 1), tyre, 4, false, false, col);
    }
    m.tube_capped(c - a * r * 0.92, c + a * r * 0.92, 0.006, 3, false, false, hub);
    m.tube_capped(c - b * r * 0.92, c + b * r * 0.92, 0.006, 3, false, false, hub);
}

/// The one bicycle, upright along local z (wheels at z ±0.525 × scale), on
/// the ground at x 0. Fine: tyre rings and spokes; else solid wheels and
/// thinner tubes (a rack of twenty).
pub fn bicycle_local(g: &mut Geometry, look: Bike) {
    let s = look.scale;
    let f = rgba(look.frame);
    let steel = rgba([0.42, 0.42, 0.44]);
    let at = |z: f32, y: f32| Vec3::new(0.0, y * s, z * s);
    let r = 0.34;
    let front_z = if look.cargo.is_some() { 1.08 } else { 0.525 };
    let (rear, front) = (at(-0.525, r), at(front_z, r));
    let sides = if look.fine { 5 } else { 3 };
    let m = g.part(Matte);
    for c in [rear, front] {
        if look.fine {
            wheel(m, c, Vec3::Z, Vec3::Y, r * s, 0.022 * s.max(0.8), 10, rgba(DARK), steel);
        } else {
            m.tube_capped(c - Vec3::X * 0.025, c + Vec3::X * 0.025, r * s, 8, true, true, rgba(DARK));
        }
    }
    let (bb, seat, head, head_lo) = (at(-0.05, 0.28), at(-0.18, 0.82), at(0.38, 0.86), at(0.42, 0.7));
    for (a, b) in [(rear, bb), (rear, seat), (bb, seat), (seat, head), (bb, head_lo), (head_lo, front), (head, head_lo)] {
        m.tube(a, b, 0.018 * s.max(0.8), sides, f);
    }
    // Seat post and saddle, stem and bars, a chainring.
    let saddle = at(-0.21, 0.95);
    m.tube(seat, saddle, 0.012, 4.min(sides + 1), steel);
    m.boxed(saddle + Vec3::Y * 0.02 * s, Vec3::new(0.11, 0.045, 0.25) * s, 0.0, rgba(DARK));
    let bar = at(0.34, 0.99);
    m.tube(head, bar, 0.014, 4.min(sides + 1), steel);
    m.tube(bar - Vec3::X * 0.24 * s, bar + Vec3::X * 0.24 * s, 0.012, 4.min(sides + 1), steel);
    if look.fine {
        m.pipe(Vec2::new(0.04, bb.z), Vec2::X, 0.015, 0.09 * s, bb.y, 8, rgba([0.2, 0.2, 0.21]));
    }
    if let Some(col) = look.cargo {
        g.part(Wood).boxed(at(0.75, 0.62), Vec3::new(0.62, 0.42, 0.7) * s, 0.0, rgba(col));
    }
}

/// A bicycle standing at `pose` (its length along the pose's +z: see
/// [`Pose::along`]). Always near detail, whoever draws it.
pub fn bicycle(g: &mut Geometry, pose: Pose, look: Bike) {
    let mut b = Geometry::default();
    bicycle_local(&mut b, look);
    g.lod(Lod::Near, |g| pose.place(g, &b));
}

// ── trees ───────────────────────────────────────────────────────────────────

/// The one tree family. `Round` is the street and garden tree; the others
/// come from parks, open ground and the styles' looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// Trunk with a two-tier faceted canopy.
    Round,
    /// Three stacked cones.
    Conifer,
    /// Leaning trunk with a crown of fronds.
    Palm,
    /// Bare tapering trunk with a few branches, no canopy.
    Dead,
    /// A slim trunk and a smooth round crown (`canopy` its radius): the
    /// clipped garden tree of the dome and capsule looks.
    Ball,
}

/// Any tree, standing on ground at height `y` at a world point. Branch and
/// lean directions come from the position so no RNG is consumed: callers
/// that place thousands of trees keep their streams stable.
#[allow(clippy::too_many_arguments)]
pub fn tree(g: &mut Geometry, w: Vec2, y: f32, kind: TreeKind, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    match kind {
        TreeKind::Round => {
            g.part(Wood).cylinder(w, 0.22, 0.16, y, y + h, 7, rgba(bark));
            let c = g.part(Foliage);
            c.frustum(&MeshData::ngon(w, canopy * 0.5, 7, 0.0, y + h), &MeshData::ngon(w, canopy * 0.2, 7, 0.4, y + h + canopy * 0.8), rgba(leaf));
            c.frustum(&MeshData::ngon(w, canopy * 0.35, 7, 0.3, y + h - canopy * 0.3), &MeshData::ngon(w, canopy * 0.5, 7, 0.0, y + h), rgba(mul(leaf, 0.85)));
        }
        TreeKind::Conifer => {
            g.part(Wood).cylinder(w, 0.22, 0.12, y, y + h * 0.3, 6, rgba(bark));
            for k in 0..3 {
                let y0 = y + h * (0.25 + 0.22 * k as f32);
                let rr = canopy * (1.0 - 0.22 * k as f32);
                g.part(Foliage).frustum(&MeshData::ngon(w, rr, 8, 0.3 * k as f32, y0), &MeshData::ngon(w, rr * 0.12, 8, 0.3 * k as f32, y0 + h * 0.32), rgba(mul(leaf, 1.0 - 0.06 * k as f32)));
            }
        }
        TreeKind::Palm => {
            let lean = Vec2::new(w.y * 0.013, w.x * 0.011).normalize_or_zero() * 0.9;
            let top = Vec3::new(w.x + lean.x, y + h, w.y + lean.y);
            let mid = Vec3::new(w.x + lean.x * 0.5, y + h * 0.55, w.y + lean.y * 0.5);
            g.part(Wood).beam(Vec3::new(w.x, y, w.y), mid, 0.34, rgba(bark));
            g.part(Wood).beam(mid, top, 0.26, rgba(bark));
            for k in 0..7 {
                let a = k as f32 / 7.0 * TAU;
                let dir = Vec3::new(a.cos(), 0.35, a.sin()).normalize();
                let m = top + dir * 1.6;
                let tip = m + Vec3::new(dir.x, -0.5, dir.z).normalize() * 1.8;
                g.part(Foliage).beam(top, m, 0.5, rgba(leaf));
                g.part(Foliage).beam(m, tip, 0.32, rgba(mul(leaf, 0.9)));
            }
            g.part(Foliage).cylinder(Vec2::new(top.x, top.z), 0.5, 0.5, y + h - 0.4, y + h + 0.3, 6, rgba(mul(leaf, 0.8)));
        }
        TreeKind::Dead => {
            let wood = g.part(Wood);
            wood.cylinder(w, 0.18, 0.07, y - 0.1, y + h, 5, rgba(bark));
            let n = 2 + ((w.x * 0.37 + w.y * 0.61).abs() as i32 % 2);
            for k in 0..n {
                let a = k as f32 * 2.1 + w.x * 0.37 + w.y * 0.13;
                let len = (h * 0.3).clamp(0.8, 1.8);
                let from = Vec3::new(w.x, y + h * (0.55 + 0.12 * k as f32), w.y);
                let to = from + Vec3::new(a.cos() * len, len * 0.7, a.sin() * len);
                wood.beam(from, to, 0.08, rgba(bark));
            }
        }
        TreeKind::Ball => {
            let ax = Axis::vertical(&Frame::world(), w, y);
            revolve(g.part(Wood), &ax, &[(0.2, 0.0), (0.13, h - canopy * 0.5)], 6, None, rgba(bark));
            revolve(g.part(Foliage), &ax, &ball_profile(canopy, canopy * 0.92, h, 6), 10, None, rgba(leaf));
        }
    }
}

/// A small tree in a square timber tub standing at `y` (a balcony's or a
/// terrace's): the tub turned `yaw` (radians, as `MeshData::boxed`).
#[allow(clippy::too_many_arguments)]
pub fn tub_tree(g: &mut Geometry, w: Vec2, y: f32, yaw: f32, tub: [f32; 3], bark: [f32; 3], leaf: [f32; 3]) {
    g.part(Wood).boxed(Vec3::new(w.x, y + 0.275, w.y), Vec3::new(0.7, 0.55, 0.7), yaw, rgba(tub));
    g.part(Wood).cylinder(w, 0.07, 0.05, y + 0.6, y + 1.3, 5, rgba(bark));
    g.part(Foliage).cylinder_capped(w, 0.55, 0.35, y + 1.1, y + 2.1, 7, true, true, rgba(leaf));
}

// ── planters and pots ───────────────────────────────────────────────────────

/// What a planter is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Planter {
    /// A round tub with a clipped ball of shrub (`r` its radius).
    Tub,
    /// A square concrete trough of shrubs, `2r` a side, turned `yaw`.
    Trough { yaw: f32 },
    /// A flower pot `2r` tall with a plant in it, by `seed` tall or bushy
    /// and in flower or not.
    Pot { seed: u64 },
}

/// A planter of `r` at a world point on ground at `y`.
pub fn planter(g: &mut Geometry, w: Vec2, y: f32, kind: Planter, r: f32, tub: [f32; 3], leaf: [f32; 3]) {
    match kind {
        Planter::Tub => {
            let ax = Axis::vertical(&Frame::world(), w, y);
            revolve(g.part(Matte), &ax, &[(r * 0.85, 0.0), (r, 0.5), (r * 0.82, 0.62), (0.0, 0.62)], sides_for(r).min(12), None, rgba(tub));
            revolve(g.part(Foliage), &ax, &dome_profile(r * 0.78, 0.55, r * 0.75, 3), 10, None, rgba(leaf));
        }
        Planter::Trough { yaw } => {
            g.part(Surface::Concrete).boxed(Vec3::new(w.x, y + 0.3, w.y), Vec3::new(r * 2.0, 0.6, r * 2.0), yaw, rgba(tub));
            g.part(Foliage).boxed(Vec3::new(w.x, y + 0.75, w.y), Vec3::new(r * 1.7, 0.4, r * 1.7), yaw, rgba(leaf));
        }
        Planter::Pot { seed } => {
            let h = r * 2.0;
            let c = w;
            let lift = |p: &[(f32, f32)]| -> Vec<(f32, f32)> { p.iter().map(|&(a, b)| (a, b + y)).collect() };
            g.part(Matte).cylinder(c, r * 0.8, r, y, y + h, 8, rgba(tub));
            g.part(Matte).cylinder_capped(c, r * 1.04, r * 1.06, y + h - 0.04, y + h, 8, false, true, rgba(mul(tub, 0.85)));
            let lf = g.part(Foliage);
            if h01(seed, 1) < 0.4 {
                lf.lathe(c, &lift(&[(r * 0.5, h), (r * 1.5, h + 0.25), (r * 1.2, h + 0.55), (0.0, h + 0.75)]), 6, rgba(leaf));
            } else {
                lf.lathe(c, &lift(&[(r * 0.9, h - 0.02), (r * 1.6, h + 0.12), (r * 1.1, h + 0.26), (0.0, h + 0.32)]), 7, rgba(leaf));
                // A few flowers on the top.
                if h01(seed, 2) < 0.6 {
                    let fl = [[0.62, 0.08, 0.10], [0.75, 0.62, 0.15], [0.62, 0.30, 0.50]][(hmix(seed, 3) % 3) as usize];
                    for k in 0..4 {
                        let a = k as f32 * 1.7 + h01(seed, 4) * TAU;
                        let q = c + Vec2::from_angle(a) * r * 0.9;
                        g.part(Matte).boxed(Vec3::new(q.x, y + h + 0.2, q.y), Vec3::splat(0.05), a, rgba(fl));
                    }
                }
            }
        }
    }
}

// ── street furniture ────────────────────────────────────────────────────────

/// A park bench at a world point, its seat along `yaw`; `broken`: one slat
/// gone, no back, a leg buckled (a run-down park's). Always near detail
/// ([`DetailBand::Near`](crate::detail::DetailBand)), whoever draws it.
pub fn bench(g: &mut Geometry, w: Vec2, yaw: f32, wood: [f32; 3], metal: [f32; 3], broken: bool) {
    g.lod(Lod::Near, |g| bench_in(g, w, yaw, wood, metal, broken));
}

fn bench_in(g: &mut Geometry, w: Vec2, yaw: f32, wood: [f32; 3], metal: [f32; 3], broken: bool) {
    let (s, c) = yaw.sin_cos();
    if broken {
        g.part(Wood).boxed(Vec3::new(w.x, 0.42, w.y), Vec3::new(1.8, 0.06, 0.22), yaw, rgba(mul(wood, 0.8)));
        let o = Vec2::new(c, s) * 0.8;
        g.part(Matte).boxed(Vec3::new(w.x + o.x, 0.22, w.y + o.y), Vec3::new(0.08, 0.44, 0.5), yaw, rgba(metal));
        g.part(Matte).boxed(Vec3::new(w.x - o.x, 0.12, w.y - o.y), Vec3::new(0.08, 0.24, 0.5), yaw + 0.5, rgba(metal));
        return;
    }
    let m = g.part(Wood);
    m.boxed(Vec3::new(w.x, 0.45, w.y), Vec3::new(1.8, 0.08, 0.5), yaw, rgba(wood));
    let back = Vec2::new(-s, c) * 0.22;
    m.boxed(Vec3::new(w.x + back.x, 0.8, w.y + back.y), Vec3::new(1.8, 0.45, 0.06), yaw, rgba(wood));
    let mm = g.part(Matte);
    for side in [-0.8f32, 0.8] {
        let o = Vec2::new(c, s) * side;
        mm.boxed(Vec3::new(w.x + o.x, 0.22, w.y + o.y), Vec3::new(0.08, 0.44, 0.5), yaw, rgba(metal));
    }
}

/// A litter bin at a world point (near detail, as a bench).
pub fn bin(g: &mut Geometry, w: Vec2, color: [f32; 3]) {
    g.lod(Lod::Near, |g| {
        g.part(Metal).cylinder(w, 0.35, 0.35, 0.0, 1.0, 10, rgba(color));
        g.part(Matte).cylinder(w, 0.37, 0.37, 1.0, 1.1, 10, rgba(DARK));
    });
}

// ── washing ─────────────────────────────────────────────────────────────────

/// Washing as it is: greyed whites, faded denim, black, a worn red or
/// mustard now and then (linear, kept dull so it reads in the sun). Three
/// sets of five.
pub const CLOTH: [[[f32; 3]; 5]; 3] = [
    [[0.42, 0.41, 0.38], [0.10, 0.13, 0.20], [0.30, 0.08, 0.07], [0.18, 0.20, 0.17], [0.40, 0.33, 0.22]],
    [[0.08, 0.08, 0.09], [0.38, 0.34, 0.30], [0.10, 0.20, 0.23], [0.44, 0.43, 0.40], [0.24, 0.12, 0.16]],
    [[0.46, 0.45, 0.42], [0.38, 0.27, 0.10], [0.13, 0.15, 0.24], [0.22, 0.06, 0.07], [0.33, 0.33, 0.31]],
];

/// A garment hung from a line at `top` (the line along `along`), `w`
/// across and `drop` down, a shirt, trousers or a towel by `kind`.
pub fn garment(g: &mut Geometry, top: Vec3, along: Vec3, w: f32, drop: f32, col: [f32; 3], kind: u64) {
    let face = Vec3::Y.cross(along).normalize_or_zero();
    let yaw = along.z.atan2(along.x);
    let m = g.part(Tarp);
    match kind % 3 {
        // Trousers: two legs.
        0 => {
            for s in [-1.0, 1.0] {
                m.boxed(top + along * (s * w * 0.24) - Vec3::Y * drop * 0.5, Vec3::new(w * 0.44, drop, 0.02), yaw, rgba(col));
            }
        }
        // A shirt: body and sleeves.
        1 => {
            m.boxed(top - Vec3::Y * drop * 0.5, Vec3::new(w, drop, 0.02), yaw, rgba(col));
            m.boxed(top - Vec3::Y * 0.12 + face * 0.001, Vec3::new(w * 1.6, 0.16, 0.018), yaw, rgba(mul(col, 0.92)));
        }
        // A towel or a sheet.
        _ => m.boxed(top - Vec3::Y * drop * 0.5, Vec3::new(w, drop, 0.015), yaw, rgba(col)),
    }
}

/// Length of line one [`washing_run`] takes, metres.
pub const RUN: f32 = 1.1;

/// A run of washing pegged on a line through the origin along local z,
/// [`RUN`] long, hanging down from it: three or four garments in cloth set
/// `set`, cut by `seed`. What the household layer hangs on a style's lines
/// and poles ([`crate::surface::WashLine`]).
pub fn washing_run(g: &mut Geometry, set: usize, seed: u64) {
    let cl = CLOTH[set % 3];
    let n = 3 + (hmix(seed, 1) % 2) as usize;
    let step = RUN / n as f32;
    for k in 0..n {
        let z = -RUN * 0.5 + step * (k as f32 + 0.5);
        let w = (step * (0.7 + 0.25 * h01(seed, 10 + k as u64))).min(0.5);
        let drop = 0.35 + 0.3 * h01(seed, 20 + k as u64);
        garment(g, Vec3::new(0.0, 0.0, z), Vec3::Z, w, drop, cl[(k + set) % 5], hmix(seed, 30 + k as u64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tris(g: &Geometry) -> usize {
        g.triangle_count()
    }

    #[test]
    fn one_bike_coarse_for_racks_fine_for_the_door() {
        let mut fine = Geometry::default();
        bicycle_local(&mut fine, Bike { fine: true, ..Bike::plain([0.4, 0.1, 0.1]) });
        let mut coarse = Geometry::default();
        bicycle_local(&mut coarse, Bike::plain([0.4, 0.1, 0.1]));
        assert!(tris(&coarse) * 2 < tris(&fine), "coarse {} fine {}", tris(&coarse), tris(&fine));
        assert!(tris(&coarse) < 200, "a rack bike stays cheap: {}", tris(&coarse));
        // Placed along +x the bike runs along x.
        let mut g = Geometry::default();
        bicycle(&mut g, Pose::along(Vec3::new(10.0, 0.0, 5.0), Vec2::X), Bike::plain([0.4, 0.1, 0.1]));
        assert_eq!(g.iter().count(), 0, "a placed bike is near detail only");
        let xs: Vec<f32> = g.detail_iter(Lod::Near).flat_map(|(_, m)| m.positions.iter().map(|p| p[0])).collect();
        let span = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!(span > 1.4, "length along x: {span}");
    }

    #[test]
    fn a_washing_run_hangs_below_its_line_and_keeps_its_length() {
        let mut g = Geometry::default();
        washing_run(&mut g, 1, 42);
        for (_, m) in g.iter() {
            for p in &m.positions {
                assert!(p[1] <= 0.01 && p[1] > -0.8, "{p:?}");
                assert!(p[2].abs() <= RUN * 0.5 + 0.2, "{p:?}");
            }
        }
        assert!(tris(&g) > 0);
    }

    #[test]
    fn every_tree_and_planter_stands_on_its_ground() {
        for kind in [TreeKind::Round, TreeKind::Conifer, TreeKind::Palm, TreeKind::Dead, TreeKind::Ball] {
            let mut g = Geometry::default();
            tree(&mut g, Vec2::new(3.0, 4.0), 2.0, kind, 4.0, 2.0, [0.3, 0.2, 0.1], [0.1, 0.3, 0.1]);
            let lo = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(f32::MAX, f32::min);
            assert!((1.85..=2.05).contains(&lo), "{kind:?} foot at {lo}");
        }
        for kind in [Planter::Tub, Planter::Trough { yaw: 0.3 }, Planter::Pot { seed: 7 }] {
            let mut g = Geometry::default();
            planter(&mut g, Vec2::ZERO, 1.0, kind, 0.4, [0.5; 3], [0.1, 0.3, 0.1]);
            let lo = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(f32::MAX, f32::min);
            assert!((lo - 1.0).abs() < 0.02, "{kind:?} foot at {lo}");
        }
    }
}
