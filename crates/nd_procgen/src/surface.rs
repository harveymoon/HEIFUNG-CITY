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
}

impl Surface {
    pub const ALL: [Surface; 16] = [
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
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Metres covered by one texture repeat.
    pub fn tile(self) -> f32 {
        match self {
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
        }
    }

    pub fn roughness(self) -> f32 {
        match self {
            Surface::Glass => 0.12,
            Surface::Metal => 0.45,
            Surface::Panel => 0.5,
            Surface::Tarp => 0.55,
            Surface::Matte => 0.7,
            Surface::Rust => 0.95,
            Surface::Asphalt => 0.95,
            Surface::Dirt => 1.0,
            Surface::Foliage => 0.9,
            _ => 0.85,
        }
    }

    pub fn metallic(self) -> f32 {
        match self {
            Surface::Metal => 0.75,
            Surface::Panel => 0.5,
            Surface::Rust => 0.35,
            Surface::Glass => 0.3,
            Surface::Matte => 0.2,
            _ => 0.0,
        }
    }

    /// Strength of the generated normal map.
    pub fn bump(self) -> f32 {
        match self {
            Surface::Brick => 1.6,
            Surface::Stone => 2.0,
            Surface::Metal => 1.4,
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
        }
    }

    pub fn is_glow(self) -> bool {
        self == Surface::Glow
    }
}

/// Mesh data split by surface. Empty surfaces are skipped by the renderer.
#[derive(Clone, Debug)]
pub struct Geometry {
    pub parts: Vec<MeshData>,
}

impl Default for Geometry {
    fn default() -> Self {
        Self { parts: (0..Surface::ALL.len()).map(|_| MeshData::default()).collect() }
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
    }

    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|m| m.indices.len() / 3).sum()
    }
}
