//! Material buckets. Every primitive a generator emits goes into the mesh for
//! one `Surface`; the renderer gives each surface its own tiling texture,
//! normal map, roughness and metalness. Vertex colours still carry the hue, so
//! the same brick texture serves red brick, painted brick and soot-black brick.

use crate::mesh::MeshData;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    /// Board-formed concrete with tie holes and streaks.
    Concrete,
    /// Smooth render with hairline cracks.
    Plaster,
    /// Running-bond brick.
    Brick,
    /// Large irregular ashlar blocks.
    Stone,
    /// Composite cladding panels with seams and fixings.
    Panel,
    /// Corrugated or ribbed sheet metal.
    Metal,
    /// Heavily corroded steel.
    Rust,
    /// Curtain-wall glazing with mullions.
    Glass,
    /// Rough planks.
    Wood,
    /// Woven tarpaulin.
    Tarp,
    /// Road surface.
    Asphalt,
    /// Paving slabs.
    Paving,
    /// Bare packed earth and gravel.
    Dirt,
    /// Leaves.
    Foliage,
    /// Matte painted or plastic parts: frames, poles, grilles.
    Matte,
    /// Unlit emissive: windows, signs, lamps, screens.
    Glow,
    // Weathered variants of the everyday surfaces: the same texture with
    // stains, cracks, rust streaks and worn paint baked in. Cheap and
    // run-down buildings and streets are moved onto these by
    // [`Geometry::weather`], so grit is a material, not extra triangles.
    WornConcrete,
    WornPlaster,
    WornBrick,
    WornPanel,
    WornMetal,
    WornPaving,
    WornAsphalt,
    /// Shipping-container sheet: trapezoidal corrugation at the ISO pitch
    /// (about 0.28 m) on a large tile, so the paint's mottling does not
    /// repeat across one unit. Last in the enum so older indices stay put.
    Container,
    WornContainer,
    /// Road paint: lane lines, stop lines, zebras, lane arrows. Unlit like
    /// `Glow` (so markings read at night) but with a material of its own
    /// that is not dimmed in daylight: on the shared Glow material the
    /// markings fell to a third of their value by day and read dark grey
    /// on sunlit asphalt.
    Paint,
}

impl Surface {
    pub const ALL: [Surface; 26] = [
        Surface::Concrete,
        Surface::Plaster,
        Surface::Brick,
        Surface::Stone,
        Surface::Panel,
        Surface::Metal,
        Surface::Rust,
        Surface::Glass,
        Surface::Wood,
        Surface::Tarp,
        Surface::Asphalt,
        Surface::Paving,
        Surface::Dirt,
        Surface::Foliage,
        Surface::Matte,
        Surface::Glow,
        Surface::WornConcrete,
        Surface::WornPlaster,
        Surface::WornBrick,
        Surface::WornPanel,
        Surface::WornMetal,
        Surface::WornPaving,
        Surface::WornAsphalt,
        Surface::Container,
        Surface::WornContainer,
        Surface::Paint,
    ];

    /// The surfaces that have a weathered variant, paired with it.
    pub const WEATHERED: [(Surface, Surface); 8] = [
        (Surface::Concrete, Surface::WornConcrete),
        (Surface::Plaster, Surface::WornPlaster),
        (Surface::Brick, Surface::WornBrick),
        (Surface::Panel, Surface::WornPanel),
        (Surface::Metal, Surface::WornMetal),
        (Surface::Paving, Surface::WornPaving),
        (Surface::Asphalt, Surface::WornAsphalt),
        (Surface::Container, Surface::WornContainer),
    ];

    /// The clean surface a weathered one is made from (itself otherwise):
    /// tiling, roughness and bump follow the base material.
    pub fn base(self) -> Surface {
        Surface::WEATHERED.iter().find(|(_, w)| *w == self).map(|(b, _)| *b).unwrap_or(self)
    }

    /// The weathered variant of a surface, if it has one.
    pub fn worn(self) -> Option<Surface> {
        Surface::WEATHERED.iter().find(|(b, _)| *b == self).map(|(_, w)| *w)
    }

    pub fn is_worn(self) -> bool {
        self.base() != self
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Metres covered by one texture repeat.
    pub fn tile(self) -> f32 {
        match self.base() {
            Surface::Concrete => 3.0,
            Surface::Plaster => 3.0,
            Surface::Brick => 1.0,
            Surface::Stone => 3.0,
            Surface::Panel => 2.4,
            Surface::Metal => 1.0,
            Surface::Rust => 2.0,
            Surface::Glass => 1.5,
            Surface::Wood => 1.0,
            Surface::Tarp => 1.0,
            Surface::Asphalt => 4.0,
            Surface::Paving => 2.0,
            Surface::Dirt => 6.0,
            Surface::Foliage => 1.0,
            Surface::Matte => 1.0,
            Surface::Glow => 1.0,
            // 20 corrugations of 0.28 m.
            Surface::Container => 5.6,
            _ => 1.0,
        }
    }

    pub fn roughness(self) -> f32 {
        if self.is_worn() {
            return (self.base().roughness() + 0.08).min(1.0);
        }
        match self {
            Surface::Glass => 0.12,
            Surface::Metal => 0.45,
            Surface::Container => 0.55,
            Surface::Panel => 0.5,
            Surface::Tarp => 0.55,
            Surface::Matte => 0.7,
            Surface::Rust => 0.95,
            Surface::Asphalt => 0.8,
            Surface::Dirt => 1.0,
            Surface::Foliage => 0.9,
            _ => 0.85,
        }
    }

    pub fn metallic(self) -> f32 {
        if self.is_worn() {
            return self.base().metallic() * 0.7;
        }
        match self {
            Surface::Metal => 0.75,
            Surface::Container => 0.45,
            Surface::Panel => 0.5,
            Surface::Rust => 0.35,
            Surface::Glass => 0.3,
            Surface::Matte => 0.2,
            _ => 0.0,
        }
    }

    /// Strength of the generated normal map.
    pub fn bump(self) -> f32 {
        if self.is_worn() {
            return self.base().bump() * 1.25;
        }
        match self {
            Surface::Brick => 1.6,
            Surface::Stone => 2.0,
            Surface::Metal => 1.4,
            Surface::Container => 1.5,
            Surface::Rust => 1.2,
            Surface::Wood => 1.0,
            Surface::Paving => 1.2,
            Surface::Concrete => 0.8,
            Surface::Panel => 0.9,
            Surface::Dirt => 1.4,
            Surface::Foliage => 1.5,
            Surface::Tarp => 0.6,
            Surface::Asphalt => 0.7,
            Surface::Plaster => 0.4,
            Surface::Glass => 0.3,
            Surface::Matte => 0.2,
            Surface::Glow => 0.0,
            _ => 0.5,
        }
    }

    /// Texels across one tile. The container sheet spreads its tile over
    /// 5.6 m, so it gets twice the usual resolution to keep ~90 px a metre.
    pub fn texels(self, default: usize) -> usize {
        if self.base() == Surface::Container { default * 2 } else { default }
    }

    /// Unlit decals with no texture: the Glow surface and road paint.
    pub fn is_glow(self) -> bool {
        matches!(self, Surface::Glow | Surface::Paint)
    }
}

/// Mesh data split by surface. Empty surfaces are skipped by the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub parts: Vec<MeshData>,
    /// Landing pads drawn (helipads, rooftop decks, vertiport pads): the
    /// centre of each deck's top, world coordinates. The renderer lands
    /// aircraft on them.
    pub pads: Vec<glam::Vec3>,
    /// Tower crane heads left out of the mesh for the renderer to slew
    /// (`Ctx::animated_cranes`), in world position.
    pub cranes: Vec<crate::buildings::CraneRig>,
}

impl Default for Geometry {
    fn default() -> Self {
        Self { parts: (0..Surface::ALL.len()).map(|_| MeshData::default()).collect(), pads: Vec::new(), cranes: Vec::new() }
    }
}

impl Geometry {
    pub fn part(&mut self, s: Surface) -> &mut MeshData {
        &mut self.parts[s.index()]
    }

    pub fn get(&self, s: Surface) -> &MeshData {
        &self.parts[s.index()]
    }

    pub fn iter(&self) -> impl Iterator<Item = (Surface, &MeshData)> {
        Surface::ALL.iter().copied().zip(self.parts.iter()).filter(|(_, m)| !m.is_empty())
    }

    pub fn append(&mut self, other: &Geometry) {
        for (a, b) in self.parts.iter_mut().zip(other.parts.iter()) {
            a.append(b);
        }
        self.pads.extend_from_slice(&other.pads);
        self.cranes.extend(other.cranes.iter().cloned());
    }

    /// Move every surface that has a weathered variant onto it: the whole
    /// building or street reads as run down without a single extra triangle.
    pub fn weather(&mut self) {
        for (base, worn) in Surface::WEATHERED {
            let m = std::mem::take(&mut self.parts[base.index()]);
            self.parts[worn.index()].append(&m);
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|m| m.indices.len() / 3).sum()
    }
}
