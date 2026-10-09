//! [climate] How each climate looks: ground, shore, water and ice, plants,
//! snow, steam and what falls from the sky. **One table** ([`look`]) holds
//! every number, so the climates are tuned in one place.
//!
//! Desert is the game's original look: its row overrides nothing and the
//! renderer draws exactly what it always drew from `assets/palette.ron`.
//! The others keep the neon city's dark, low-key ground (the glow has to
//! carry the night), so the greens are deep and the snow is a soft grey-blue
//! that the sun brightens by day, not a white sheet that drowns the lights.

use crate::buildings::parts::{mix, mul, tree_kind, TreeKind};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::Vec2;
use nd_sim::Climate;

/// What grows on open ground and in streets and parks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flora {
    /// Scrub, rocks, dead trees; round street trees (the original).
    #[default]
    Dry,
    /// Leafy round trees, green bushes, grass.
    Leafy,
    /// Palms among lush round trees and dense bushes.
    Palms,
    /// Conifers, rocks, a few bare trees.
    Conifers,
    /// Moss tufts, lava rocks, the odd stunted birch.
    Moss,
}

impl Flora {
    /// Open-ground scatter density over the Desert's (part of the table).
    pub fn growth(self) -> f32 {
        match self {
            Flora::Dry => 1.0,
            Flora::Leafy => 1.5,
            Flora::Palms => 1.8,
            Flora::Conifers => 1.1,
            Flora::Moss => 0.8,
        }
    }
}

/// What falls when the sky is heavy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precip {
    None,
    Rain,
    Snow,
}

/// One climate's row of the table.
#[derive(Clone, Copy, Debug)]
pub struct ClimateLook {
    /// Open ground, its noise variation and the hills; `None` keeps the palette's.
    pub ground: Option<([f32; 3], [f32; 3], [f32; 3])>,
    /// The strip along the water (sand, shingle, black sand, ice).
    pub shore: Option<[f32; 3]>,
    /// The tint at a hot spot's heart.
    pub hot_ground: [f32; 3],
    /// Leaves and dry leaves; `None` keeps the palette's.
    pub foliage: Option<([f32; 3], [f32; 3])>,
    /// Bare earth (scrapes, sites, yards); `None` keeps the palette's.
    pub dirt: Option<[f32; 3]>,
    /// Water: (shallow, deep), linear; `None` keeps the palette's.
    pub water: Option<([f32; 3], [f32; 3])>,
    /// Lakes and the sea's edge are frozen (a look only: still water to the sim).
    pub ice: Option<[f32; 3]>,
    /// How far out from the shore the sea ice reaches, metres.
    pub sea_ice: f32,
    pub flora: Flora,
    /// 0..1: snow on roofs, rocks and anything facing up.
    pub snow: f32,
    pub precip: Precip,
    /// Steam plume size over the base (hot spots).
    pub steam: f32,
}

/// The table.
pub fn look(c: Climate) -> ClimateLook {
    match c {
        Climate::Desert => ClimateLook {
            ground: None,
            shore: None,
            hot_ground: [0.62, 0.55, 0.22],
            foliage: None,
            dirt: None,
            water: None,
            ice: None,
            sea_ice: 0.0,
            flora: Flora::Dry,
            snow: 0.0,
            precip: Precip::None,
            steam: 1.0,
        },
        Climate::Temperate => ClimateLook {
            ground: Some(([0.030, 0.052, 0.022], [0.016, 0.020, 0.010], [0.042, 0.060, 0.036])),
            shore: Some([0.20, 0.19, 0.15]),
            hot_ground: [0.55, 0.50, 0.24],
            foliage: Some(([0.08, 0.19, 0.06], [0.17, 0.19, 0.08])),
            dirt: Some([0.13, 0.15, 0.085]),
            water: Some(([0.06, 0.16, 0.17], [0.012, 0.05, 0.075])),
            ice: None,
            sea_ice: 0.0,
            flora: Flora::Leafy,
            snow: 0.0,
            precip: Precip::Rain,
            steam: 1.0,
        },
        Climate::Tropical => ClimateLook {
            ground: Some(([0.022, 0.070, 0.016], [0.018, 0.032, 0.006], [0.020, 0.075, 0.026])),
            shore: Some([0.62, 0.58, 0.44]),
            hot_ground: [0.58, 0.52, 0.20],
            foliage: Some(([0.06, 0.30, 0.06], [0.14, 0.28, 0.06])),
            dirt: Some([0.12, 0.15, 0.07]),
            water: Some(([0.05, 0.42, 0.40], [0.006, 0.07, 0.12])),
            ice: None,
            sea_ice: 0.0,
            flora: Flora::Palms,
            snow: 0.0,
            precip: Precip::Rain,
            steam: 0.8,
        },
        Climate::Arctic => ClimateLook {
            ground: Some(([0.34, 0.37, 0.42], [0.05, 0.05, 0.06], [0.42, 0.45, 0.50])),
            shore: Some([0.22, 0.24, 0.27]),
            hot_ground: [0.20, 0.17, 0.12],
            foliage: Some(([0.035, 0.08, 0.05], [0.10, 0.11, 0.08])),
            dirt: None,
            water: Some(([0.05, 0.11, 0.15], [0.008, 0.03, 0.06])),
            ice: Some([0.40, 0.50, 0.58]),
            sea_ice: 70.0,
            flora: Flora::Conifers,
            snow: 0.85,
            precip: Precip::Snow,
            steam: 1.6,
        },
        Climate::Volcanic => ClimateLook {
            ground: Some(([0.020, 0.021, 0.022], [0.016, 0.034, 0.010], [0.034, 0.048, 0.030])),
            shore: Some([0.012, 0.012, 0.014]),
            hot_ground: [0.50, 0.40, 0.12],
            foliage: Some(([0.05, 0.09, 0.035], [0.075, 0.08, 0.055])),
            dirt: Some([0.075, 0.07, 0.068]),
            water: Some(([0.04, 0.16, 0.19], [0.008, 0.04, 0.07])),
            ice: None,
            sea_ice: 0.0,
            flora: Flora::Moss,
            snow: 0.0,
            precip: Precip::Snow,
            steam: 2.4,
        },
    }
}

/// The palette with the climate's ground, plants and water put in. Desert
/// returns it unchanged.
pub fn palette_for(base: &Palette, c: Climate) -> Palette {
    let l = look(c);
    let mut p = base.clone();
    if let Some((g, v, h)) = l.ground {
        p.ground = g;
        p.ground_variation = v;
        p.hills = h;
        p.lot_vacant = mix(g, base.lot_vacant, 0.35);
    }
    if let Some((f, d)) = l.foliage {
        p.foliage = f;
        p.foliage_dry = d;
    }
    if let Some(d) = l.dirt {
        p.dirt = d;
    }
    p.flora = l.flora;
    p.snow = l.snow;
    p
}

/// The colour of snow lying on a surface.
pub const SNOW: [f32; 3] = [0.62, 0.66, 0.72];

/// Lay snow on everything in `geo` that faces up (roofs, rock tops,
/// canopies, ledges), by `amount`: vertex colours are pulled towards
/// [`SNOW`]. Lamps, signs, windows and roads are left alone.
pub fn snow_cover(geo: &mut Geometry, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    for s in Surface::ALL {
        if matches!(s, Surface::Glow | Surface::Glass | Surface::Asphalt | Surface::Paving) {
            continue;
        }
        snow_mesh(geo.part(s), amount);
    }
}

fn snow_mesh(m: &mut MeshData, amount: f32) {
    for (n, c) in m.normals.iter().zip(m.colors.iter_mut()) {
        let up = ((n[1] - 0.3) / 0.45).clamp(0.0, 1.0) * amount;
        if up > 0.0 {
            let rgb = mix([c[0], c[1], c[2]], SNOW, up);
            c[0] = rgb[0];
            c[1] = rgb[1];
            c[2] = rgb[2];
        }
    }
}

/// A street, park or garden tree for the palette's climate, standing at
/// `w` on the ground: what used to be a round tree everywhere. The kind
/// comes from a hash of the position, so no RNG is drawn and the callers'
/// streams stay as they were.
pub fn climate_tree(g: &mut Geometry, pal: &Palette, w: Vec2, h: f32, canopy: f32, leaf: [f32; 3]) {
    let y = 0.0;
    let r = hash01(w);
    match pal.flora {
        Flora::Dry | Flora::Leafy => tree_kind(g, w, y, TreeKind::Round, h, canopy, pal.bark, leaf),
        Flora::Palms => {
            if r < 0.6 {
                tree_kind(g, w, y, TreeKind::Palm, h * 1.7 + 1.5, canopy, mul(pal.bark, 1.3), mul(leaf, 1.1));
            } else {
                tree_kind(g, w, y, TreeKind::Round, h * 1.1, canopy * 1.15, pal.bark, leaf);
            }
        }
        Flora::Conifers => tree_kind(g, w, y, TreeKind::Conifer, h * 1.8 + 1.0, canopy * 0.7, pal.bark, leaf),
        Flora::Moss => {
            if r < 0.5 {
                tree_kind(g, w, y, TreeKind::Conifer, h * 1.3, canopy * 0.55, pal.bark, leaf);
            } else {
                // A stunted birch: pale bark, small crown.
                tree_kind(g, w, y, TreeKind::Round, h * 0.8, canopy * 0.7, [0.55, 0.53, 0.5], mix(leaf, [0.2, 0.24, 0.08], 0.3));
            }
        }
    }
}

/// Can a steam vent stand at `p`: off every road and track and not under a
/// building? (Hot spots predate the city; their vents keep to open ground.)
pub fn vent_clear(world: &nd_sim::World, p: Vec2) -> bool {
    let roads = &world.roads;
    if let Some((sid, _, q)) = roads.nearest_where(p, |_| true) {
        if q.distance(p) < roads.segment(sid).half_width() + 4.0 {
            return false;
        }
    }
    !world.lots.values().any(|l| l.building.is_some() && nd_sim::geom::point_in_polygon(p, &l.polygon))
}

/// 0..1 from a position, for picks that must not draw from an RNG.
pub use crate::hash::hash01_sin as hash01;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desert_keeps_the_palette() {
        let base = Palette::default();
        let d = palette_for(&base, Climate::Desert);
        assert_eq!(d.ground, base.ground);
        assert_eq!(d.foliage, base.foliage);
        assert_eq!(d.flora, Flora::Dry);
        assert_eq!(d.snow, 0.0);
        let a = palette_for(&base, Climate::Arctic);
        assert!(a.ground[0] > base.ground[0] * 3.0, "snow is pale");
        assert_eq!(a.flora, Flora::Conifers);
    }

    #[test]
    fn snow_lies_on_roofs_only() {
        let mut g = Geometry::default();
        g.part(Surface::Concrete).boxed(glam::Vec3::new(0.0, 5.0, 0.0), glam::Vec3::splat(4.0), 0.0, [0.3, 0.3, 0.3, 1.0]);
        snow_cover(&mut g, 1.0);
        let m = g.part(Surface::Concrete);
        for (n, c) in m.normals.iter().zip(&m.colors) {
            if n[1] > 0.9 {
                assert!(c[0] > 0.5, "roof snowed on");
            } else {
                assert!((c[0] - 0.3).abs() < 1e-6, "walls stay");
            }
        }
    }
}
