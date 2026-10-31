//! [pt-visual] Playtest round 2026-10-10, section H: courtyard blocks with
//! no floating windows (F39), monuments that look like monuments (F40), a
//! big low garden tower drawn as a garden courtyard (F41), the brutalist
//! civic menu, and the beach boardwalk.

use super::sweep_tests::{lot_of, odd_lots, rect};
use super::*;
use crate::surface::Surface;
use nd_sim::zoning::point_in_polygon;
use parts::{green_roof, poly_center, solar_roof};

fn ctx<'a>(lot: &Lot, style: Style, lu: LandUse, floors: u8, tier: Tier, rules: &'a BuildingRules, pal: &'a Palette) -> Ctx<'a> {
    let finish = Finish { rules: rules.for_land_use(lu), style: rules.style(tier), palette: pal };
    Ctx::new(lot, style, lu, floors, tier, 1.0, 7, finish, &Extras::default())
}

/// Distance from `p` (lot-local) to the nearest edge of `poly`.
fn to_edges(p: Vec2, poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| nd_sim::geom::project_on_segment(p, poly[i], poly[(i + 1) % n]).1.distance(p)).fold(f32::MAX, f32::min)
}

fn wedges() -> Vec<(&'static str, Lot)> {
    let at = Vec2::new(-120.0, 80.0);
    let mut v = odd_lots();
    v.push(("wedge", lot_of(&[Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(-24.0, 46.0)], 0.9, at)));
    v.push(("big wedge", lot_of(&[Vec2::new(-45.0, 0.0), Vec2::new(45.0, 0.0), Vec2::new(30.0, 70.0)], -0.4, at)));
    v.push(("block", lot_of(&rect(48.0, 40.0), 0.2, at)));
    v.push(("trapezoid", lot_of(&[Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(14.0, 36.0), Vec2::new(-20.0, 36.0)], 1.3, at)));
    v
}

/// F39: every pane of a courtyard block lies on a wall: the lot's outline
/// or the courtyard's.
#[test]
fn courtyard_windows_sit_on_walls() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    for (name, lot) in wedges() {
        for (tier, floors) in [(Tier::Mid, 3u8), (Tier::Low, 5), (Tier::Mid, 8)] {
            let c = ctx(&lot, Style::Courtyard, LandUse::Residential, floors, tier, &rules, &pal);
            let mut g = Geometry::default();
            let mut rng = nd_sim::rng_for(7, 1);
            residential::courtyard(&mut g, &c, &mut rng);
            let inner = residential::courtyard_ring(&c.poly).map(|r| r.0);
            // Facades only: the roof's gear (dishes, panels) stands above them.
            let roof = c.fh() * (floors as f32).clamp(3.0, 8.0) - 0.3;
            for (s, min_y) in [(Surface::Glass, 0.5f32), (Surface::Glow, 4.2)] {
                for p in &g.get(s).positions {
                    if p[1] < min_y || p[1] > roof {
                        continue;
                    }
                    let q = c.frame.to_local(Vec2::new(p[0], p[2]));
                    let d = to_edges(q, &c.poly).min(inner.as_ref().map_or(f32::MAX, |r| to_edges(q, r)));
                    assert!(d < 0.8, "{name} {tier:?} {floors}: a {s:?} pane at {p:?} stands {d:.2} m off any wall");
                }
            }
        }
    }
}

/// The courtyard ring keeps every wing on its own side and inside the lot.
#[test]
fn courtyard_rings_are_sound() {
    for (name, lot) in wedges() {
        let (_, poly) = Frame::lot_local(&lot);
        if let Some((inner, depth)) = residential::courtyard_ring(&poly) {
            assert_eq!(inner.len(), poly.len(), "{name}");
            assert!(depth >= 4.0, "{name}: wings {depth} m deep");
            assert!(inner.iter().all(|q| point_in_polygon(*q, &poly)), "{name}: courtyard leaves the lot");
            assert!(!nd_sim::geom::self_intersects(&inner), "{name}: courtyard crosses itself");
        }
    }
}

/// F40: a monument is tall, has no helipad and no office windows, and the
/// tiers and seeds give more than one design.
#[test]
fn monuments_look_like_monuments() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = lot_of(&rect(40.0, 36.0), 0.3, Vec2::new(50.0, 50.0));
    for tier in Tier::ALL {
        let mut tallest = 0.0f32;
        for seed in 0..12u64 {
            let extras = Extras { service: Some(nd_sim::services::ServiceKind::Monument), ..Extras::default() };
            let g = generate_building(&lot, LandUse::Civic, 1, tier, 1.0, seed, &rules, &pal, &extras);
            assert!(g.pads.is_empty(), "{tier:?} seed {seed}: a helipad on a monument");
            let top = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
            tallest = tallest.max(top);
            assert!(top > 4.0, "{tier:?} seed {seed}: the monument is {top:.1} m tall");
        }
        assert!(tallest > 9.0, "{tier:?}: nothing on the menu stands over 9 m ({tallest:.1})");
        assert!(monuments::designs(tier).len() >= 3, "{tier:?}: too few designs");
    }
}

/// F41: a big low garden tower is a garden courtyard with planted roofs,
/// not a terraced pile with a tree on top.
#[test]
fn a_big_low_garden_tower_is_a_garden_courtyard() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = lot_of(&rect(60.0, 52.0), 0.1, Vec2::new(0.0, 0.0));
    let c = ctx(&lot, Style::GardenTower, LandUse::Residential, 3, Tier::High, &rules, &pal);
    let mut g = Geometry::default();
    let mut rng = nd_sim::rng_for(3, 1);
    residential::garden_tower(&mut g, &c, &mut rng);
    let top = g.get(Surface::Plaster).positions.iter().map(|p| p[1]).fold(0.0f32, f32::max);
    assert!(top > 8.0 && top < 20.0, "a 3-floor garden court stands {top:.1} m");
    // Planting on the roofs.
    let roof_green = g.get(Surface::Foliage).positions.iter().filter(|p| p[1] > top - 0.5).count();
    assert!(roof_green > 20, "only {roof_green} planted vertices on the roofs");
    // And a courtyard: the centre of the lot is open to the sky.
    let mid = poly_center(&c.poly);
    let over_mid = g.get(Surface::Plaster).positions.iter().filter(|p| c.frame.to_local(Vec2::new(p[0], p[2])).distance(mid) < 3.0 && p[1] > 1.0).count();
    assert_eq!(over_mid, 0, "the middle of the court is built over");
}

/// Civic land has its brutalist styles on the menu at every tier.
#[test]
fn civic_menus_have_the_brutalist_styles() {
    let rules = BuildingRules::default();
    let has = |tier: Tier, s: Style| rules.options(LandUse::Civic, tier).iter().any(|o| o.style == s);
    assert!(has(Tier::Low, Style::CivicBunker));
    assert!(has(Tier::Mid, Style::CivicTower) && has(Tier::Mid, Style::Panopticon));
    assert!(has(Tier::High, Style::CivicTower) && has(Tier::High, Style::Panopticon));
    let pal = Palette::default();
    for (name, lot) in odd_lots() {
        for s in [Style::CivicBunker, Style::CivicTower, Style::Panopticon] {
            let g = generate_styled_extras(&lot, LandUse::Civic, 6, Tier::Mid, 1.0, 5, s, &rules, &pal, &Extras::default());
            assert!(g.triangle_count() > 0 || lot.depth().min(lot.width()) < 4.0, "{name} {s:?} drew nothing");
        }
    }
}

/// Green and solar roofs stay on their roof.
#[test]
fn green_and_solar_roofs_stay_on_the_roof() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    for (name, lot) in wedges() {
        let c = ctx(&lot, Style::PodiumOffice, LandUse::Commercial, 6, Tier::High, &rules, &pal);
        for solar in [false, true] {
            let mut g = Geometry::default();
            if solar {
                solar_roof(&mut g, &c, &c.poly, 10.0);
            } else {
                green_roof(&mut g, &c, &c.poly, 10.0);
            }
            for (_, m) in g.iter() {
                for p in &m.positions {
                    let q = c.frame.to_local(Vec2::new(p[0], p[2]));
                    assert!(point_in_polygon(q, &c.poly) || to_edges(q, &c.poly) < 0.3, "{name} solar={solar}: {p:?} off the roof");
                    assert!(p[1] >= 10.0 && p[1] < 11.5, "{name}: roof planting at {}", p[1]);
                }
            }
        }
    }
}

/// [r4-flags-look] (flag day 46 at (-427,329), "this building is coming
/// through my elevated road") A one-storey garden tower (held to one floor
/// under a walkway deck) stands one storey high: it was drawn six floors
/// and a mast tall, up through the deck.
#[test]
fn a_one_storey_garden_tower_is_one_storey_high() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = lot_of(&rect(18.0, 16.0), 0.3, Vec2::new(-40.0, 25.0));
    for floors in [1u8, 2] {
        let c = ctx(&lot, Style::GardenTower, LandUse::Residential, floors, Tier::High, &rules, &pal);
        let mut g = Geometry::default();
        let mut rng = nd_sim::rng_for(9, 1);
        residential::garden_tower(&mut g, &c, &mut rng);
        let top = g.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| p[1]).fold(0.0f32, f32::max);
        let allowed = floors as f32 * c.fh() + 1.5;
        assert!(top > 2.0 && top <= allowed, "a {floors}-floor garden tower stands {top:.1} m (at most {allowed:.1})");
    }
}
