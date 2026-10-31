//! The parts toolkit every style is assembled from: footprint helpers,
//! facades, roofs, site furniture and props. Everything is placed in parcel
//! coordinates through the [`Frame`], except the `*_world` variants used by
//! the street generator (and by parks and rail, which lay out from world
//! polygons and pass [`Frame::world`]).
//!
//! Flat detail that is only ever seen from one side (panes, frames, signs,
//! stains, painted lines) is a [`decal`]: one quad lifted a few centimetres
//! off its wall, not a closed box. A box spends twelve triangles where two
//! do the job, and a tower has thousands of these.

use super::{Ctx, Face, Frame};
use crate::mesh::MeshData;
use crate::depth::{FRAME, PANE};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{point_in_polygon, polygon_area, polygon_centroid};
use rand::Rng;

use Surface::*;

// ── colours and chance ───────────────────────────────────────────────────────

/// `random_bool` with the probability clamped: rule files and wear
/// arithmetic can produce chances a little over one, and `random_bool`
/// panics on them.
pub fn coin(rng: &mut impl Rng, p: f32) -> bool {
    let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
    rng.random_bool(p as f64)
}

/// Vertex alpha at or above this marks a window light in the Glow mesh; the
/// fraction above it is the window's rank in `0..1`. The renderer lights a
/// building's windows whose rank is below its lit share and swaps the mark
/// for that share's lookup (opaque glow ignores alpha anyway).
pub const WINDOW_LIGHT_MARK: f32 = 2.0;
/// [w1-render] Vertex alpha marking a landmark's neon in the Glow mesh:
/// the renderer dims it by day like the signs (it is drawn far brighter, to
/// carry at night, and glared on through the afternoon).
pub const NEON_MARK: f32 = 1.5;
/// Vertex alpha marking glow lit only after dark (an observatory's beam):
/// the renderer leaves it out by day.
pub const NIGHT_MARK: f32 = 1.75;

/// Mark the glow drawn since vertex `from` with `mark` ([`NEON_MARK`],
/// [`NIGHT_MARK`]), leaving window lights and anything marked already.
pub fn mark_glow(g: &mut Geometry, from: usize, mark: f32) {
    for c in g.part(Glow).colors.iter_mut().skip(from) {
        if c[3] < NEON_MARK - 0.01 {
            c[3] = mark;
        }
    }
}

pub use crate::depth::LIGHT_GLASS_BEHIND;

/// What fills one window opening.
#[derive(Clone, Copy, Debug)]
pub enum Pane {
    /// A light in this colour (switched by the renderer with dynamic lights).
    Lit([f32; 3]),
    /// Dark glass.
    Dark,
    /// Plywood over the opening.
    Boarded,
}

impl Face {
    /// One window opening `w` by `h`, centred `t` along the face at height
    /// `y` and `d` proud of it: a light (through [`window_light`]), dark
    /// glass or plywood. Every facade's glazing goes through here, so the
    /// light's dark glass, its mark and the depths are decided in one place.
    #[allow(clippy::too_many_arguments)]
    pub fn glaze(&self, g: &mut Geometry, c: &Ctx, t: f32, y: f32, d: f32, w: f32, h: f32, pane: Pane) {
        let f = &c.frame;
        match pane {
            Pane::Lit(col) => window_light(g, c, |g| self.decal(g, f, Glow, t, y, d, w, h, col)),
            Pane::Dark => self.decal(g, f, Glass, t, y, d, w, h, c.palette.glass_dark),
            Pane::Boarded => self.decal(g, f, Wood, t, y, d, w, h, c.palette.plywood),
        }
    }

    /// A window's frame: `pad` wider and taller than its opening, at the
    /// frame depth.
    #[allow(clippy::too_many_arguments)]
    pub fn window_frame(&self, g: &mut Geometry, c: &Ctx, t: f32, y: f32, w: f32, h: f32, pad: f32, color: [f32; 3]) {
        self.decal(g, &c.frame, Matte, t, y, crate::depth::FRAME, w + pad, h + pad, color);
    }
}

/// Draw a window light with `emit` (which adds Glow). With
/// [`Ctx::dynamic_lights`] the new glow vertices are marked with
/// [`WINDOW_LIGHT_MARK`] plus a rank hashed from their position, and a
/// copy in dark glass is laid just behind them, so the window still reads as
/// glazing when the renderer switches the light off. Without it this is
/// just `emit`.
pub fn window_light(g: &mut Geometry, c: &Ctx, emit: impl FnOnce(&mut Geometry)) {
    let (vstart, istart) = (g.get(Glow).positions.len(), g.get(Glow).indices.len());
    emit(g);
    if !c.dynamic_lights || g.get(Glow).positions.len() == vstart {
        return;
    }
    let (glow, glass) = glow_and_glass(g);
    let rank = crate::hash::rank01(glow.positions[vstart]);
    for col in &mut glow.colors[vstart..] {
        col[3] = WINDOW_LIGHT_MARK + rank;
    }
    // The dark glass copy goes straight onto the glass mesh, no temporary.
    let dark = rgba(c.palette.glass_dark);
    let base = glass.positions.len() as u32;
    for i in vstart..glow.positions.len() {
        let (p, n) = (glow.positions[i], glow.normals[i]);
        glass.positions.push([p[0] - n[0] * LIGHT_GLASS_BEHIND, p[1] - n[1] * LIGHT_GLASS_BEHIND, p[2] - n[2] * LIGHT_GLASS_BEHIND]);
        glass.normals.push(n);
        glass.colors.push(dark);
        glass.uvs.push(glow.uvs[i]);
        glass.tangents.push(glow.tangents[i]);
    }
    glass.indices.extend(glow.indices[istart..].iter().map(|i| i - vstart as u32 + base));
}

/// The glow and glass meshes of a geometry, both borrowed at once.
fn glow_and_glass(g: &mut Geometry) -> (&mut MeshData, &mut MeshData) {
    let (a, b) = (Glow.index(), Glass.index());
    if a < b {
        let (lo, hi) = g.parts.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = g.parts.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}

pub use crate::color::{mix, mul, pick, rgba, CHARCOAL, DARK};

/// A red aviation light: a glowing cube of side `size` centred on the world
/// point `at`, square to the world axes.
pub fn aviation_light(g: &mut Geometry, pal: &Palette, at: Vec3, size: f32) {
    g.part(Glow).boxed(at, Vec3::splat(size), 0.0, rgba(pal.aviation_light));
}

/// A red aviation light at lot-local (u, y, v), turned with the frame.
#[allow(clippy::too_many_arguments)]
pub fn aviation_light_local(g: &mut Geometry, f: &Frame, pal: &Palette, u: f32, y: f32, v: f32, size: f32) {
    f.bx(g, Glow, u, y, v, Vec3::splat(size), pal.aviation_light);
}

pub(crate) use super::kit::{fence, fence_lot, fence_poly, gate, hedge};

mod polygons;
mod solids;
mod facades;
mod roofs;
mod site;
mod plant;
mod boxes;
mod props;
mod graffiti;
mod ground;

pub use polygons::*;
pub use solids::*;
pub use facades::*;
pub use roofs::*;
pub use site::*;
pub use plant::*;
pub use boxes::*;
pub use props::*;
pub use graffiti::*;
pub use ground::*;
