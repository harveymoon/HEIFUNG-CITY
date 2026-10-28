//! [r4-style] The Hong Kong detail pass (buildings/hk.rs) and the detail
//! layers it draws into.

use super::coplanar_tests::{clash_list, test_lots};
use super::*;
use crate::surface::Lod;

const HK: [Style; 2] = [Style::WalledCity, Style::PencilTower];

fn draw(lot: &Lot, style: Style, floors: u8, tier: Tier, condition: f32, seed: u64) -> Geometry {
    let extras = Extras { dynamic_lights: true, ..Extras::default() };
    generate_styled_extras(lot, LandUse::Residential, floors, tier, condition, seed, style, &BuildingRules::default(), &Palette::default(), &extras)
}

/// Every Hong Kong block and tower has its near detail (frames, AC units)
/// and its mid detail (shutter drums, awnings, cages), drawn the same every
/// time from its seed, and no glow ever goes into a detail layer: the lit
/// windows and signs read from across the city.
#[test]
fn hong_kong_styles_draw_detail_layers_deterministically() {
    let lots = test_lots(LandUse::Residential);
    for style in HK {
        for floors in [4u8, 8, 14] {
            for tier in [Tier::Low, Tier::Mid, Tier::High] {
                let g = draw(&lots[0], style, floors, tier, 0.6, 7 + floors as u64);
                assert!(g.detail_triangle_count(Lod::Near) > 0, "{style:?} {floors} {tier:?}: no near detail");
                assert!(g.detail_triangle_count(Lod::Mid) > 0, "{style:?} {floors} {tier:?}: no mid detail");
                for l in Lod::DETAIL {
                    assert!(g.detail_iter(l).all(|(s, _)| !s.is_glow()), "{style:?}: glow in the {l:?} layer");
                }
                assert!(g == draw(&lots[0], style, floors, tier, 0.6, 7 + floors as u64), "{style:?} {floors} {tier:?}: not the same twice");
            }
        }
    }
}

/// The styles that do not dress themselves draw no detail layers: their
/// meshes are what they were.
#[test]
fn other_styles_draw_no_detail() {
    let lots = test_lots(LandUse::Residential);
    for style in [Style::Tenement, Style::TwinSlab, Style::Stacks, Style::CapsuleHotel] {
        let g = draw(&lots[0], style, 8, Tier::Mid, 0.6, 3);
        assert!(!g.has_detail(), "{style:?} drew detail");
    }
}

/// The detail is most of a Hong Kong tower's triangles but stays in
/// budget: a 14-floor tower on the largest test lot, base plus detail,
/// under 60k triangles, and the base (what is drawn from afar) a fraction
/// of it. A city repeats these by the hundred.
#[test]
fn hong_kong_detail_stays_in_budget() {
    let lots = test_lots(LandUse::Residential);
    for style in HK {
        for tier in [Tier::Low, Tier::Mid, Tier::High] {
            let g = draw(&lots[0], style, 14, tier, 0.4, 11);
            let (base, mid, near) = (g.base_triangle_count(), g.detail_triangle_count(Lod::Mid), g.detail_triangle_count(Lod::Near));
            assert!(base + mid + near < 60_000, "{style:?} {tier:?}: {base} + {mid} + {near} triangles");
            assert!(near > base / 2, "{style:?} {tier:?}: near detail {near} against a base of {base}");
        }
    }
}

/// No detail part z-fights on a wall of the building it is on: the wall
/// clashes with the detail drawn are the wall clashes of the base alone.
/// (Roof tops are left out: `antenna_forest`, now mid detail, has always
/// let two tanks' lids meet at one height.)
#[test]
fn hong_kong_detail_adds_no_coplanar_faces() {
    let lots = test_lots(LandUse::Residential);
    for style in HK {
        for lot in lots.iter().take(3) {
            for floors in [4u8, 9, 14] {
                for tier in [Tier::Low, Tier::High] {
                    let g = draw(lot, style, floors, tier, 0.5, 42 + floors as u64);
                    let mut bare = g.clone();
                    bare.detail = [Vec::new(), Vec::new()];
                    let walls = |g: &Geometry| clash_list(g).into_iter().filter(|c| c.normal[1].abs() < 0.5).collect::<Vec<_>>();
                    let (all, base) = (walls(&g), walls(&bare));
                    assert!(all.len() == base.len(), "{style:?} {floors} {tier:?}: {} new clashes, e.g. {}", all.len() - base.len(), all.iter().find(|c| !base.iter().any(|b| b.what == c.what)).map_or(String::new(), |c| c.what.clone()));
                }
            }
        }
    }
}

/// Nothing hangs far out over the street: every detail vertex is within
/// 2.2 m of the lot (awnings and signs reach out over the pavement).
#[test]
fn hong_kong_detail_stays_by_its_lot() {
    let lots = test_lots(LandUse::Residential);
    for style in HK {
        for lot in lots.iter().take(4) {
            for floors in [4u8, 12] {
                let g = draw(lot, style, floors, Tier::Low, 0.3, 5);
                for l in Lod::DETAIL {
                    for (s, m) in g.detail_iter(l) {
                        for p in &m.positions {
                            let q = glam::Vec2::new(p[0], p[2]);
                            let off = if nd_sim::zoning::point_in_polygon(q, &lot.polygon) { 0.0 } else { parts::distance_to_boundary(q, &lot.polygon) };
                            assert!(off < 2.2, "{style:?} {floors}: a {s:?} vertex {off:.2} m off the lot");
                        }
                    }
                }
            }
        }
    }
}

/// A Hong Kong block coming down is cut as one: its detail folded into the
/// base, no AC unit left in the air over the cut.
#[test]
fn a_demolished_block_has_no_detail_left_standing() {
    let lots = test_lots(LandUse::Residential);
    let extras = Extras { demolition: Some(0.7), ..Extras::default() };
    for style in HK {
        let g = generate_styled_extras(&lots[0], LandUse::Residential, 12, Tier::Low, 0.3, 9, style, &BuildingRules::default(), &Palette::default(), &extras);
        assert!(!g.has_detail(), "{style:?}");
    }
}

/// Moving and weathering a geometry carry the detail layers along.
#[test]
fn detail_layers_follow_the_base() {
    let lots = test_lots(LandUse::Residential);
    let g = draw(&lots[0], Style::PencilTower, 10, Tier::Low, 0.3, 4);
    let mut up = g.clone();
    crate::terrain::raise_geometry(&mut up, 3.0);
    let lowest = |g: &Geometry| g.detail_iter(Lod::Near).flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(f32::MAX, f32::min);
    assert!((lowest(&up) - lowest(&g) - 3.0).abs() < 1e-3);
    let mut merged = Geometry::default();
    merged.append(&g);
    merged.append(&g);
    assert_eq!(merged.detail_triangle_count(Lod::Near), 2 * g.detail_triangle_count(Lod::Near));
    // A low tier tower is drawn weathered: its detail too.
    assert!(g.detail_iter(Lod::Near).all(|(s, _)| s.worn().is_none()), "unweathered surface left in the near layer");
}
