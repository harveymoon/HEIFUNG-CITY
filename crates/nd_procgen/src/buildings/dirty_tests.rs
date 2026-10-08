//! [dirty-industry] Tests for the dirty trades' looks and their vents.

use super::adjacency::{traits, Family};
use super::dirty::{dirty_vents, VentKind};
use super::*;
use nd_sim::world::{World, WorldConfig};
use nd_sim::BusinessKind as B;

const NEW: [Style; 3] = [Style::Smelter, Style::Feedlot, Style::ChemicalWorks];
const LOOKS: [(B, Style); 3] = [(B::Smelter, Style::Smelter), (B::Feedlot, Style::Feedlot), (B::ChemicalWorks, Style::ChemicalWorks)];

/// Industrial lots beside a street: the largest, a typical and an odd one.
fn sample_lots() -> Vec<Lot> {
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-60.0, -100.0), Vec2::new(10.0, 200.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-150.0, 8.0), Vec2::new(150.0, 8.0), Vec2::new(150.0, 90.0), Vec2::new(-150.0, 90.0)], LandUse::Industrial);
    let mut lots: Vec<Lot> = w.lots.values().filter(|l| l.usable()).cloned().collect();
    assert!(!lots.is_empty());
    lots.sort_by(|a, b| b.area().total_cmp(&a.area()));
    let mut out = vec![lots[0].clone(), lots[lots.len() / 2].clone()];
    if let Some(odd) = lots.iter().find(|l| l.polygon.len() != 4) {
        out.push(odd.clone());
    }
    out
}

#[test]
fn dirty_styles_build_at_every_tier_and_are_deterministic() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = sample_lots();
    for style in NEW {
        for (i, lot) in lots.iter().enumerate() {
            for tier in [Tier::Low, Tier::Mid, Tier::High] {
                let g = generate_styled(lot, LandUse::Industrial, 1, tier, 0.6, 11 + i as u64, style, &rules, &pal);
                let tris = g.triangle_count();
                assert!(tris > 100, "{style:?} {tier:?} on lot {i} drew only {tris} triangles");
                assert!(tris < 120_000, "{style:?} {tier:?} on lot {i} drew {tris} triangles");
                for (s, m) in g.iter() {
                    assert!(m.positions.iter().flatten().all(|v| v.is_finite()), "{style:?} {s:?} NaN");
                    assert!(m.indices.iter().all(|&k| (k as usize) < m.positions.len()), "{style:?} {s:?} bad index");
                }
            }
        }
        let a = generate_styled(&lots[0], LandUse::Industrial, 1, Tier::Mid, 0.6, 3, style, &rules, &pal);
        let b = generate_styled(&lots[0], LandUse::Industrial, 1, Tier::Mid, 0.6, 3, style, &rules, &pal);
        assert_eq!(a.triangle_count(), b.triangle_count(), "{style:?} is not deterministic");
        assert!(!style.label().is_empty());
    }
    // Indices of older styles are untouched: the block is at the end.
    assert_eq!(Style::ALL.len(), 111 + 6, "[supertall] appends one after this block, [waterfront] six");
    for (k, s) in NEW.iter().enumerate() {
        assert_eq!(s.index(), 107 + k);
        assert_eq!(Style::ALL[107 + k], *s);
    }
    assert_eq!(traits(Style::Smelter).0, Family::Heavy);
    assert_eq!(traits(Style::ChemicalWorks).0, Family::Heavy);
    assert_eq!(traits(Style::Feedlot).0, Family::Farm);
}

#[test]
fn dirty_businesses_wear_their_looks() {
    let rules = BuildingRules::default();
    let lot = &sample_lots()[0];
    for (kind, want) in LOOKS {
        for tier in [Tier::Low, Tier::Mid, Tier::High] {
            for look in [None, Some(nd_sim::districts::Look::Kowloon)] {
                let extras = Extras { business: Some(kind), look, ..Extras::default() };
                assert_eq!(choose_style(lot, LandUse::Industrial, 1, tier, 2, &rules, &extras), want, "{kind:?} {tier:?}");
            }
        }
    }
}

#[test]
fn dirty_plants_have_vents_inside_their_lot() {
    for lot in sample_lots() {
        let smelter = dirty_vents(&lot, B::Smelter);
        assert!(smelter.iter().any(|v| v.kind == VentKind::Soot), "a smelter smokes");
        for v in &smelter {
            assert!(nd_sim::zoning::point_in_polygon(Vec2::new(v.pos.x, v.pos.z), &lot.polygon), "{v:?} off the lot");
            assert!(v.pos.y > 0.0);
        }
        assert!(dirty_vents(&lot, B::StripMine).iter().all(|v| v.kind == VentKind::Dust));
        assert!(dirty_vents(&lot, B::Feedlot).iter().all(|v| v.kind == VentKind::Stench));
        assert!(dirty_vents(&lot, B::Grocery).is_empty());
        assert!(dirty_vents(&lot, B::GrainFarm).is_empty());
    }
}
