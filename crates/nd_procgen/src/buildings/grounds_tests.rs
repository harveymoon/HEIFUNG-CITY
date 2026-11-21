//! [r12-odd-lots] Small buildings on big sites, and piers only for uses
//! that use the water (Harvey's flag on Null Pointer Karaoke).

use super::sweep_tests::{faults, lot_of, rect};
use super::*;
use crate::surface::Lod;
use nd_sim::districts::Look;

fn tall_points(g: &Geometry) -> Vec<Vec2> {
    g.iter().flat_map(|(_, m)| m.positions.iter().filter(|p| p[1] > 2.5).map(|p| Vec2::new(p[0], p[2]))).collect()
}

fn all_points(g: &Geometry) -> Vec<Vec2> {
    g.iter().chain(Lod::DETAIL.into_iter().flat_map(|l| g.detail_iter(l))).flat_map(|(_, m)| m.positions.iter().map(|p| Vec2::new(p[0], p[2]))).collect()
}

/// The karaoke lot's shape: about 62 m along a street and 60 deep to the
/// water, its far corner cut off on the slant.
fn big_site() -> Lot {
    lot_of(&[Vec2::new(-31.0, 0.0), Vec2::new(31.0, 0.0), Vec2::new(31.0, 46.0), Vec2::new(-24.0, 60.0), Vec2::new(-31.0, 60.0)], 0.31, Vec2::new(-360.0, 590.0))
}

/// The size rule: a bar or a shack on a site over 2.5 lot areas stands at
/// the street on a plot of its use's size; the rest is grounds (a lawn
/// with trees), not a yard of paving; nothing is drawn off the site; a
/// normal lot is drawn as before.
#[test]
fn a_small_venue_on_a_huge_site_stands_at_the_street_in_grounds() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = big_site();
    assert!(grounds::roomy(&lot, LandUse::Entertainment), "{:.0} m² is roomy for leisure", lot.area());
    let (frame, _) = Frame::lot_local(&lot);
    let front_v = -frame.depth * 0.5;
    for (k, (style, business, look)) in [
        (Style::SeafoodShack, nd_sim::BusinessKind::Venue, Some(Look::HarbourPastel)),
        (Style::KaraokeBoxes, nd_sim::BusinessKind::KaraokeBox, None),
        (Style::Diner, nd_sim::BusinessKind::Diner, None),
    ]
    .into_iter()
    .enumerate()
    {
        let ex = Extras { business: Some(business), look, ..Extras::default() };
        let g = generate_styled_extras(&lot, LandUse::Entertainment, 3, Tier::Mid, 0.9, 40 + k as u64, style, &rules, &pal, &ex);
        // The building is the style drawn on the front plot; everything
        // else drawn is grounds: no walls, only trees, lamps, rails, benches.
        let front = grounds::front_plot(&lot, LandUse::Entertainment).expect("a front plot");
        let alone = generate_styled_extras(&front, LandUse::Entertainment, 3, Tier::Mid, 0.9, 40 + k as u64, style, &rules, &pal, &Extras { shore: None, ..ex });
        let mine: std::collections::HashSet<[u32; 3]> = alone.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p.map(f32::to_bits))).collect();
        for (s, m) in g.iter() {
            for p in m.positions.iter().filter(|p| p[1] > 2.5 && !mine.contains(&p.map(f32::to_bits))) {
                assert!(matches!(s, crate::surface::Surface::Foliage | crate::surface::Surface::Wood | crate::surface::Surface::Matte | crate::surface::Surface::Glow | crate::surface::Surface::Metal), "{style:?}: {s:?} {p:?} drawn in the grounds");
            }
        }
        let tall = tall_points(&alone);
        assert!(!tall.is_empty(), "{style:?}: a building");
        // Every wall within a use's depth and a half of the street.
        let deepest = tall.iter().map(|p| frame.to_local(*p).y - front_v).fold(0.0f32, f32::max);
        assert!(deepest < LandUse::Entertainment.lot_depth() * 1.6, "{style:?}: the building reaches {deepest:.1} m back");
        let nearest = tall.iter().map(|p| frame.to_local(*p).y - front_v).fold(f32::MAX, f32::min);
        assert!(nearest < 12.0, "{style:?}: set back {nearest:.1} m from the street");
        // The grounds: a lawn over the site, and trees.
        let lawn = g.get(crate::surface::Surface::Foliage);
        assert!(!lawn.positions.is_empty(), "{style:?}: lawn and trees");
        let bad = faults(&g, &lot, style);
        assert!(bad.is_empty(), "{style:?}: {bad:?}");
    }
    // A lot of the use's own size is drawn exactly as the style draws it.
    let small = lot_of(&rect(20.0, 24.0), 0.31, Vec2::new(-360.0, 590.0));
    assert!(!grounds::roomy(&small, LandUse::Entertainment));
    assert!(grounds::front_plot(&small, LandUse::Entertainment).is_none_or(|f| f.area() >= small.area() * 0.5));
}

/// A style that fills its big site (a cinema's hall) keeps the site:
/// only buildings that left most of it bare move to the street.
#[test]
fn a_building_that_fills_its_site_keeps_it() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = lot_of(&rect(46.0, 40.0), 0.2, Vec2::new(100.0, 40.0));
    assert!(grounds::roomy(&lot, LandUse::Entertainment));
    let ex = Extras { business: Some(nd_sim::BusinessKind::Cinema), ..Extras::default() };
    let g = generate_styled_extras(&lot, LandUse::Entertainment, 3, Tier::Mid, 0.9, 5, Style::Multiplex, &rules, &pal, &ex);
    assert!(grounds::cover(&g, &lot) >= grounds::MAX_COVER, "the hall covers {:.2} of its site", grounds::cover(&g, &lot));
}

/// The dock rule: on a lot on the water, a karaoke bar, an office or a
/// shop draws no pier; a seafood restaurant, a boat rental's look and a
/// home do (the restaurant's shack runs its pier out; a home keeps a
/// boat). Nothing at all is drawn out over the water for the venue.
#[test]
fn only_uses_that_use_the_water_run_a_pier_out_over_it() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = super::coplanar_tests::test_lots(LandUse::Residential)[0].clone();
    let (frame, local) = Frame::lot_local(&lot);
    let back = parts::bounds(&local).1.y;
    let shore = ShoreLook { seaward: frame.inward, drop: 1.6, beach: None };
    let out = |g: &Geometry| all_points(g).iter().map(|q| frame.to_local(*q).y - back).fold(0.0f32, f32::max);
    let harbour = |business| Extras { shore: Some(shore), look: Some(Look::HarbourPastel), business, ..Extras::default() };
    for style in [Style::SeafoodShack, Style::BoardwalkCafe, Style::AwningArcade, Style::BoutiqueHotel] {
        for (lu, kind) in [(LandUse::Entertainment, nd_sim::BusinessKind::Venue), (LandUse::Commercial, nd_sim::BusinessKind::Office), (LandUse::Commercial, nd_sim::BusinessKind::Grocery)] {
            let g = generate_styled_extras(&lot, lu, 2, Tier::Mid, 0.9, 77, style, &rules, &pal, &harbour(Some(kind)));
            assert!(out(&g) < 1.3, "{style:?} for a {kind:?}: a pier {:.1} m out", out(&g));
        }
    }
    let g = generate_styled_extras(&lot, LandUse::Commercial, 1, Tier::Mid, 0.9, 77, Style::SeafoodShack, &rules, &pal, &harbour(Some(nd_sim::BusinessKind::SeafoodRestaurant)));
    assert!(out(&g) > 5.0, "the seafood restaurant's pier");
    let g = generate_styled_extras(&lot, LandUse::Residential, 2, Tier::Mid, 0.9, 77, Style::Boathouses, &rules, &pal, &harbour(None));
    assert!(out(&g) > 5.0, "the boathouses' pier");
    assert!(nd_sim::shore::uses_water(nd_sim::BusinessKind::BoatRental) && nd_sim::shore::uses_water(nd_sim::BusinessKind::FishMarket));
    assert!(!nd_sim::shore::uses_water(nd_sim::BusinessKind::Venue) && !nd_sim::shore::uses_water(nd_sim::BusinessKind::KaraokeBox) && !nd_sim::shore::uses_water(nd_sim::BusinessKind::Office));
}

/// On the water the grounds run a promenade along the shore (paving,
/// a rail, benches): drawn on the site, never out over the water.
#[test]
fn grounds_on_the_water_keep_a_promenade_on_the_site() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = big_site();
    let (frame, _) = Frame::lot_local(&lot);
    let shore = ShoreLook { seaward: frame.inward, drop: 1.6, beach: None };
    let ex = Extras { shore: Some(shore), look: Some(Look::HarbourPastel), business: Some(nd_sim::BusinessKind::Venue), ..Extras::default() };
    let g = generate_styled_extras(&lot, LandUse::Entertainment, 4, Tier::High, 0.95, 9, Style::SeafoodShack, &rules, &pal, &ex);
    for q in all_points(&g) {
        assert!(nd_sim::zoning::point_in_polygon(q, &lot.polygon) || parts::distance_to_boundary(q, &lot.polygon) < 1.3, "drawn off the site at {q:?}");
    }
    // The rail along the water: metal over the last metres of the site.
    let back = |q: Vec2| frame.to_local(q).y;
    let deep = all_points(&g).into_iter().map(back).fold(f32::MIN, f32::max);
    assert!(deep > frame.depth * 0.5 - 2.0, "the promenade reaches the water");
}
