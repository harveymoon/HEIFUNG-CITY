//! [r12-grime] How much grime a building or a street carries, and how
//! that reaches the shader: soft edges and shader grime without a single
//! extra triangle.
//!
//! The renderer draws the walls, pavements and lot floors with a small
//! extension of their materials (`nd_app::render::weathering`) that reads
//! one pair per vertex from the mesh's second UV set, the **weathering
//! channel** ([`MeshData::aux`]):
//!
//! - `[0, 0]`: nothing (every vertex nobody marked).
//! - `[wear, ground]` with `wear` in `0..=1` (above zero): a **wall** of a
//!   building that stands on ground at height `ground` (world metres).
//!   The shader takes the height over that ground from the fragment: a
//!   grime band where the wall meets the ground, rain streaks under each
//!   storey's sills, damp and moss low on the shaded side, dirt in the
//!   corners the ambient occlusion finds, all scaled by `wear`.
//! - `[-wear, packed]` (below zero): **ground** `d` metres in from a soft
//!   edge (the back of a pavement, a lot's boundary) on a strip `span`
//!   wide ([`pack_edge`]): weeds and grass creeping over it near the
//!   edge, grime along the hard edge across (the kerb), patched and
//!   broken paving, dirt in corners. A strip is marked with the distance
//!   at each of its vertices, so the interpolation gives the true
//!   distance across it.
//!
//! What sets `wear`: [`building_grime`] (age, condition, tier, land use,
//! style: new, rich and glass stay clean; old, poor and industrial do
//! not) and [`street_grime`] (land value). What the climate adds (soot,
//! dust, damp, moss) is city-wide: [`climate_grime`].

use crate::buildings::adjacency::Family;
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface};
use crate::Style;
use nd_sim::{Climate, LandUse, Tier};

/// The least wear a marked vertex carries (zero means "not marked").
pub const MIN_WEAR: f32 = 0.02;
/// Days over which a new building takes on half its grime.
pub const HALF_GRIME_DAYS: f32 = 30.0;

/// How strongly grime takes on a surface: rough masonry and render take
/// the most, glass and painted frames little, shipping containers none
/// (their wear is their own, `aged_paint`), ground, glow and paint none.
pub fn takes_grime(s: Surface) -> f32 {
    use Surface::*;
    match s {
        Concrete | Plaster | Brick | Stone | WornConcrete | WornPlaster | WornBrick => 1.0,
        Panel | WornPanel | Metal | WornMetal | Rust | Wood => 0.8,
        Tarp | Matte => 0.45,
        Glass => 0.3,
        _ => 0.0,
    }
}

/// [r12-grime] A ground vertex's distance in from the soft edge `d` and
/// the span of the strip it is on (how far the hard edge on the other
/// side is: a pavement's kerb), packed into one number that still
/// interpolates linearly across a strip of constant span:
/// `d + 0.5 + 16 * round(span * 10)`, `d` and `span` under 15 m. The
/// shader takes `k = floor(v / 16)`, `span = k / 10`, `d = v - 16 k - 0.5`.
pub fn pack_edge(d: f32, span: f32) -> f32 {
    d.clamp(0.0, 15.0) + 0.5 + 16.0 * (span.clamp(0.0, 15.0) * 10.0).round()
}

/// The distance and span out of [`pack_edge`] (as the shader reads them).
pub fn unpack_edge(v: f32) -> (f32, f32) {
    let k = (v / 16.0).floor();
    (v - 16.0 * k - 0.5, k / 10.0)
}

/// A wall's weathering pair: its wear and the ground it stands on.
pub fn wall_aux(wear: f32, ground: f32) -> [f32; 2] {
    [wear.clamp(MIN_WEAR, 1.0), ground]
}

/// A ground vertex's weathering pair: the wear and its distance in from
/// the soft edge with the strip's span ([`pack_edge`]).
pub fn edge_aux(wear: f32, packed: f32) -> [f32; 2] {
    [-wear.clamp(MIN_WEAR, 1.0), packed]
}

/// Mark every wall part of a building's geometry (base and detail
/// layers) with its weathering pair; ground, glow and containers stay
/// unmarked. No vertex is added or moved.
pub fn mark_building(g: &mut Geometry, wear: f32, ground: f32) {
    let aux = wall_aux(wear, ground);
    for (s, part) in g.meshes_mut() {
        if takes_grime(s) > 0.0 && !part.positions.is_empty() {
            part.set_aux(0, aux);
        }
    }
}

/// How dirty a building looks, `0..=1`: the weather of its years (half
/// its grime in its first [`HALF_GRIME_DAYS`] days), more as its condition
/// falls, more on cheap land and in industry, less where the rich keep it
/// up, and little on glass towers and luxury homes; the capsule domes stay
/// spotless (as they do in `generate_building`).
pub fn building_grime(tier: Tier, condition: f32, age_days: f32, land_use: LandUse, style: Option<Style>) -> f32 {
    if style.is_some_and(|s| Style::CAPSULE.contains(&s)) {
        return 0.0;
    }
    let age = age_days.max(0.0);
    let years = age / (age + HALF_GRIME_DAYS);
    let neglect = (1.0 - condition).clamp(0.0, 1.0);
    let mut w = (0.18 + 0.55 * years) * (0.45 + 0.9 * neglect);
    w += match tier {
        Tier::Low => 0.16,
        Tier::Mid => 0.04,
        Tier::High => -0.08,
    };
    w += match land_use {
        LandUse::Industrial => 0.18,
        LandUse::Entertainment => 0.06,
        LandUse::Civic | LandUse::Park => -0.04,
        _ => 0.0,
    };
    let k = match style.map(|s| crate::buildings::adjacency::traits(s).0) {
        Some(Family::Spire | Family::Luxury) => 0.45,
        Some(Family::Office | Family::Tech) => 0.7,
        Some(Family::Heavy | Family::Logistics) => 1.25,
        Some(Family::Slum) => 1.2,
        _ => 1.0,
    };
    (w * k).clamp(0.0, 1.0)
}

/// How worn a street's pavements look, `0..=1`, by the land value along
/// it (0..1): weeds and patches on cheap streets, a trace on dear ones.
pub fn street_grime(land_value: f32) -> f32 {
    let v = land_value.clamp(0.0, 1.0);
    (0.75 - 0.65 * v).clamp(0.08, 0.8)
}

/// What the climate adds to everything's grime, city-wide (linear
/// colours).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrimeClimate {
    /// The dirt that collects at the foot of walls and in corners.
    pub grime: [f32; 3],
    /// Damp: dark wet staining and green growth low on the shaded side,
    /// `0..=1`.
    pub damp: f32,
    /// The colour moss and algae take.
    pub moss: [f32; 3],
    /// What creeps over paving edges: grass and weeds (or dust, ash).
    pub weeds: [f32; 3],
    /// How much grows there at all, `0..=1` (a desert's edges are dust).
    pub growth: f32,
}

pub fn climate_grime(c: Climate) -> GrimeClimate {
    match c {
        Climate::Temperate => GrimeClimate { grime: [0.045, 0.038, 0.030], damp: 0.75, moss: [0.035, 0.06, 0.022], weeds: [0.05, 0.085, 0.028], growth: 1.0 },
        Climate::Tropical => GrimeClimate { grime: [0.05, 0.036, 0.026], damp: 1.0, moss: [0.028, 0.065, 0.02], weeds: [0.045, 0.1, 0.025], growth: 1.0 },
        Climate::Desert => GrimeClimate { grime: [0.20, 0.155, 0.11], damp: 0.0, moss: [0.12, 0.10, 0.07], weeds: [0.16, 0.13, 0.08], growth: 0.25 },
        Climate::Arctic => GrimeClimate { grime: [0.05, 0.048, 0.045], damp: 0.35, moss: [0.04, 0.05, 0.035], weeds: [0.06, 0.07, 0.045], growth: 0.45 },
        Climate::Volcanic => GrimeClimate { grime: [0.022, 0.02, 0.019], damp: 0.55, moss: [0.035, 0.06, 0.025], weeds: [0.055, 0.095, 0.035], growth: 0.85 },
    }
}

/// The pairs of a mesh, for tests: one per vertex.
pub fn aux_of(m: &MeshData) -> Vec<[f32; 2]> {
    m.aux_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// [r12-grime] Old, poor and industrial buildings carry more grime
    /// than new, rich and glass ones; a capsule dome none; always in 0..1.
    #[test]
    fn grime_follows_age_condition_tier_and_style() {
        let new_rich = building_grime(Tier::High, 1.0, 2.0, LandUse::Commercial, None);
        let old_poor = building_grime(Tier::Low, 0.3, 200.0, LandUse::Residential, None);
        assert!(old_poor > new_rich + 0.4, "{old_poor} vs {new_rich}");
        // Age alone, condition alone, tier alone, industry alone.
        let base = |tier, cond, days, lu| building_grime(tier, cond, days, lu, None);
        assert!(base(Tier::Mid, 0.8, 150.0, LandUse::Residential) > base(Tier::Mid, 0.8, 3.0, LandUse::Residential));
        assert!(base(Tier::Mid, 0.3, 60.0, LandUse::Residential) > base(Tier::Mid, 0.9, 60.0, LandUse::Residential));
        assert!(base(Tier::Low, 0.8, 60.0, LandUse::Residential) > base(Tier::High, 0.8, 60.0, LandUse::Residential));
        assert!(base(Tier::Mid, 0.8, 60.0, LandUse::Industrial) > base(Tier::Mid, 0.8, 60.0, LandUse::Commercial));
        // A glass tower stays cleaner than a walk-up of the same age.
        let glass = building_grime(Tier::Mid, 0.7, 90.0, LandUse::Commercial, Some(Style::GlassTerraces));
        let plain = building_grime(Tier::Mid, 0.7, 90.0, LandUse::Commercial, None);
        assert!(glass < plain * 0.6, "{glass} vs {plain}");
        assert_eq!(building_grime(Tier::Low, 0.0, 999.0, LandUse::Industrial, Some(Style::DomeHouse)), 0.0);
        for tier in [Tier::Low, Tier::Mid, Tier::High] {
            for cond in [0.0, 0.5, 1.0] {
                for days in [0.0, 10.0, 1e5] {
                    for lu in LandUse::ALL {
                        let w = building_grime(tier, cond, days, lu, Some(Style::ALL[(days as usize) % Style::ALL.len()]));
                        assert!((0.0..=1.0).contains(&w), "{w}");
                    }
                }
            }
        }
    }

    #[test]
    fn cheap_streets_are_worn_and_dear_ones_nearly_clean() {
        assert!(street_grime(0.05) > 0.6);
        assert!(street_grime(1.0) <= 0.12);
        assert!(street_grime(0.5) < street_grime(0.2));
    }

    /// Marking a building touches its walls only and moves nothing; the
    /// channel stays one pair per vertex through an append.
    #[test]
    fn marking_walls_leaves_ground_glow_and_geometry_alone() {
        let mut g = Geometry::default();
        g.part(Surface::Brick).quad_facing([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y], Vec3::Z, [1.0; 4]);
        g.part(Surface::Glow).quad_facing([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y], Vec3::Z, [1.0; 4]);
        g.part(Surface::Paving).quad_facing([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, 1.0), Vec3::Z], Vec3::Y, [1.0; 4]);
        let before = g.clone();
        mark_building(&mut g, 0.6, 12.5);
        assert_eq!(aux_of(g.get(Surface::Brick)), vec![[0.6, 12.5]; 4]);
        assert!(g.get(Surface::Glow).aux.is_empty() && g.get(Surface::Paving).aux.is_empty());
        assert_eq!(g.get(Surface::Brick).positions, before.get(Surface::Brick).positions);
        // A clean building still marks (the least wear), so its walls take
        // the climate's ground band.
        let mut h = Geometry::default();
        h.part(Surface::Concrete).quad_facing([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y], Vec3::Z, [1.0; 4]);
        mark_building(&mut h, 0.0, 0.0);
        assert_eq!(aux_of(h.get(Surface::Concrete))[0][0], MIN_WEAR);
        // Appended after unmarked vertices: padded with nothing.
        let mut m = MeshData::default();
        m.quad_facing([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y], Vec3::Z, [1.0; 4]);
        m.append(g.get(Surface::Brick));
        let a = aux_of(&m);
        assert_eq!(a.len(), 8);
        assert_eq!(&a[..4], &[[0.0; 2]; 4]);
        assert_eq!(&a[4..], &[[0.6, 12.5]; 4]);
        // An unmarked mesh appended after keeps the channel's length right.
        m.quad_facing([Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y], Vec3::Z, [1.0; 4]);
        assert_eq!(aux_of(&m).len(), 12);
        assert_eq!(aux_of(&m)[11], [0.0; 2]);
    }

    #[test]
    fn climates_differ_where_it_shows() {
        let desert = climate_grime(Climate::Desert);
        let wet = climate_grime(Climate::Tropical);
        assert_eq!(desert.damp, 0.0);
        assert!(wet.damp > desert.damp && wet.growth > desert.growth);
        // Desert grime is pale dust, volcanic grime black soot.
        let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
        assert!(lum(desert.grime) > lum(climate_grime(Climate::Volcanic).grime) * 4.0);
    }
}
