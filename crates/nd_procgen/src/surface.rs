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
    /// Tactile paving at a dropped kerb: a square grid of truncated domes
    /// in the texture, so a pad is two triangles however many dots it has.
    Tactile,
}

impl Surface {
    pub const ALL: [Surface; 27] = [
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
        Surface::Tactile,
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
            // 4 x 4 domes.
            Surface::Tactile => 0.6,
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
            // [r8] Painted steel is no mirror: at 0.45 a navy or oxblood
            // box took the sky's colour and read white (Harvey's "mostly
            // white" stacks).
            Surface::Container => 0.06,
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
            Surface::Tactile => 2.2,
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

/// [r4-style] How close the camera must be for a part of a building to be
/// drawn. The base mesh is always drawn; the two detail layers are spawned
/// as meshes of their own that the renderer drops with distance, so a city
/// of thousands of AC units, frames and pipes only pays for the ones near
/// the camera. Every layer is generated every time, from the same seed: the
/// detail is a permanent part of the building, only hidden when far off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Lod {
    /// Always drawn: the mass, the lit windows and every glowing sign.
    #[default]
    Base,
    /// Reads from a few blocks away: awnings, cages, tanks, shacks, cores'
    /// steel. Drawn to [`Lod::MID_RANGE`].
    Mid,
    /// The grain seen up close: AC units, frames, laundry, pipes, railings,
    /// streaks. Drawn to [`Lod::NEAR_RANGE`].
    Near,
}

impl Lod {
    /// The detail layers, in [`Geometry::detail`] order.
    pub const DETAIL: [Lod; 2] = [Lod::Mid, Lod::Near];
    /// Camera distance (m, to the middle of the chunk's detail) past which
    /// mid detail is dropped.
    pub const MID_RANGE: f32 = 700.0;
    /// Camera distance past which near detail is dropped.
    pub const NEAR_RANGE: f32 = 320.0;

    /// Index into [`Geometry::detail`], or `None` for the base mesh.
    pub fn layer(self) -> Option<usize> {
        match self {
            Lod::Base => None,
            Lod::Mid => Some(0),
            Lod::Near => Some(1),
        }
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
    /// [look] Lamp heads drawn (`lamp_post_world`): the centre under the
    /// lantern, world coordinates. The renderer hangs real lights on the
    /// ones near the camera at night.
    pub lamps: Vec<glam::Vec3>,
    /// [games] A stadium's game-day dressing (crowds, players, the cars of
    /// the away fans), left out of the mesh for the renderer to show on a
    /// game day.
    pub games: Vec<crate::buildings::GameRig>,
    /// [K34] Where a depot stacks its real boxes (`Ctx::real_boxes`): the
    /// renderer stands the boxes stored there in these, in order.
    pub box_slots: Vec<crate::buildings::BoxSlot>,
    /// [r4-style] The detail layers ([`Lod::DETAIL`] order), each split by
    /// surface like `parts`; empty until a generator draws into one (with
    /// [`Geometry::lod`]). Never holds glow: lit windows and signs stay in
    /// the base mesh so the night city reads the same from afar.
    pub detail: [Vec<MeshData>; 2],
    /// Where [`Geometry::part`] sends what is drawn now.
    lod: Lod,
}

impl Default for Geometry {
    fn default() -> Self {
        Self { parts: (0..Surface::ALL.len()).map(|_| MeshData::default()).collect(), pads: Vec::new(), cranes: Vec::new(), lamps: Vec::new(), games: Vec::new(), box_slots: Vec::new(), detail: [Vec::new(), Vec::new()], lod: Lod::Base }
    }
}

impl Geometry {
    /// The mesh for surface `s` in the layer being drawn ([`Geometry::lod`]):
    /// every primitive a generator emits comes through here. Glow always
    /// goes to the base mesh.
    pub fn part(&mut self, s: Surface) -> &mut MeshData {
        match self.lod.layer().filter(|_| !s.is_glow()) {
            None => &mut self.parts[s.index()],
            Some(k) => {
                let layer = &mut self.detail[k];
                if layer.is_empty() {
                    layer.resize_with(Surface::ALL.len(), MeshData::default);
                }
                &mut layer[s.index()]
            }
        }
    }

    /// Draw `body` into detail layer `lod` (back to the layer before after).
    pub fn lod<R>(&mut self, lod: Lod, body: impl FnOnce(&mut Geometry) -> R) -> R {
        let was = std::mem::replace(&mut self.lod, lod);
        let r = body(self);
        self.lod = was;
        r
    }

    /// One detail layer by surface (empty surfaces skipped).
    pub fn detail_iter(&self, lod: Lod) -> impl Iterator<Item = (Surface, &MeshData)> {
        let layer = lod.layer().map_or(&[][..], |k| &self.detail[k][..]);
        Surface::ALL.iter().copied().zip(layer.iter()).filter(|(_, m)| !m.is_empty())
    }

    /// Whether any detail was drawn.
    pub fn has_detail(&self) -> bool {
        self.detail.iter().any(|l| l.iter().any(|m| !m.is_empty()))
    }

    /// Fold the detail layers into the base mesh (a building coming down is
    /// cut as one).
    pub fn flatten_detail(&mut self) {
        for layer in self.detail.iter_mut() {
            for (a, b) in self.parts.iter_mut().zip(std::mem::take(layer).iter()) {
                a.append(b);
            }
        }
    }

    /// Every mesh, base and detail, for passes that move or recolour all of
    /// them (raising onto a pad, snow).
    pub fn meshes_mut(&mut self) -> impl Iterator<Item = (Surface, &mut MeshData)> {
        let base = Surface::ALL.iter().copied().zip(self.parts.iter_mut());
        let [mid, near] = &mut self.detail;
        base.chain(Surface::ALL.iter().copied().zip(mid.iter_mut())).chain(Surface::ALL.iter().copied().zip(near.iter_mut()))
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
        for (mine, theirs) in self.detail.iter_mut().zip(other.detail.iter()) {
            if theirs.is_empty() {
                continue;
            }
            if mine.is_empty() {
                mine.resize_with(Surface::ALL.len(), MeshData::default);
            }
            for (a, b) in mine.iter_mut().zip(theirs.iter()) {
                a.append(b);
            }
        }
        self.pads.extend_from_slice(&other.pads);
        self.cranes.extend(other.cranes.iter().cloned());
        self.lamps.extend_from_slice(&other.lamps);
        self.games.extend(other.games.iter().cloned());
        self.box_slots.extend_from_slice(&other.box_slots);
    }

    /// Move every surface that has a weathered variant onto it: the whole
    /// building or street reads as run down without a single extra triangle.
    pub fn weather(&mut self) {
        for layer in std::iter::once(&mut self.parts).chain(self.detail.iter_mut().filter(|l| !l.is_empty())) {
            for (base, worn) in Surface::WEATHERED {
                let m = std::mem::take(&mut layer[base.index()]);
                layer[worn.index()].append(&m);
            }
        }
    }

    /// Triangles in the base mesh and every detail layer.
    pub fn triangle_count(&self) -> usize {
        self.base_triangle_count() + self.detail.iter().flatten().map(|m| m.indices.len() / 3).sum::<usize>()
    }

    /// Triangles in the base mesh only (what is drawn from far off).
    pub fn base_triangle_count(&self) -> usize {
        self.parts.iter().map(|m| m.indices.len() / 3).sum()
    }

    /// Triangles in one detail layer.
    pub fn detail_triangle_count(&self, lod: Lod) -> usize {
        lod.layer().map_or(0, |k| self.detail[k].iter().map(|m| m.indices.len() / 3).sum())
    }
}
