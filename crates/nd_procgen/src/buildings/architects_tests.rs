//! [r6-architects] The architects' landmarks: each builds a clean mesh on
//! every shape of site (no faults, on its lot, detail in the LOD layers),
//! varies with the seed, grows with the site, and turns to what lies off
//! it: the ledge house reaches toward the water or downhill, the chapel's
//! roof rises to the view, the library's fan opens toward it, the slab
//! runs along the long side.

use super::architects::{look_at, SiteLook, ARCHITECTS};
use super::sweep_tests::{faults, lot_of, rect};
use super::*;
use crate::surface::Lod;
use nd_sim::services::ServiceKind;

fn draw(lot: &Lot, kind: ServiceKind, seed: u64, site: Option<SiteLook>) -> Geometry {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    generate_building(lot, LandUse::Civic, 3, Tier::Mid, 0.95, seed, &rules, &pal, &Extras { service: Some(kind), site, ..Extras::default() })
}

/// Sites the size a landmark is put on: a wide one, a deep one, a notched
/// one of joined plots, a big one and a skewed one.
fn sites() -> Vec<(&'static str, Lot)> {
    let at = Vec2::new(120.0, -60.0);
    let notched = [Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 34.0), Vec2::new(8.0, 34.0), Vec2::new(8.0, 52.0), Vec2::new(-30.0, 52.0)];
    vec![
        ("wide 64x34", lot_of(&rect(64.0, 34.0), 0.3, at)),
        ("deep 34x64", lot_of(&rect(34.0, 64.0), -0.6, at)),
        ("notched", lot_of(&notched, 1.1, at)),
        ("big 96x72", lot_of(&rect(96.0, 72.0), 0.0, at)),
        ("skewed", lot_of(&[Vec2::new(-26.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(22.0, 46.0), Vec2::new(-34.0, 40.0)], 2.2, at)),
    ]
}

fn points(g: &Geometry, s: Surface) -> Vec<Vec3> {
    g.get(s).positions.iter().map(|p| Vec3::from(*p)).collect()
}

fn centroid2(ps: &[Vec3]) -> Vec2 {
    let n = ps.len().max(1) as f32;
    ps.iter().fold(Vec2::ZERO, |a, p| a + Vec2::new(p.x, p.z)) / n
}

#[test]
fn every_architects_landmark_builds_clean_on_every_site() {
    for kind in ARCHITECTS {
        assert!(kind.is_landmark(), "{kind:?} is a landmark");
        let style = Style::for_service(kind);
        assert!(style.is_attraction(), "{style:?}: its neon follows the day");
        for (name, lot) in sites() {
            for seed in [3u64, 77, 1234] {
                let g = draw(&lot, kind, seed, None);
                let bad = faults(&g, &lot, style);
                assert!(bad.is_empty(), "{kind:?} on {name} seed {seed}: {bad:?}");
                let tris = g.triangle_count();
                assert!(tris > 600, "{kind:?} on {name}: only {tris} triangles");
                // The base mesh within the budget every building keeps
                // (`tests::geometry_stays_near_its_lot`), the detail layers
                // within two and a half times it.
                let base = g.base_triangle_count();
                let budget = 4000 + 300 * 3 + 2 * polygon_area(&lot.polygon).abs() as usize;
                assert!(base <= budget && (tris - base) * 2 <= 5 * budget, "{kind:?} on {name}: {base} base + {} detail triangles against a budget of {budget}", tris - base);
                assert!(g.get(Surface::Glow).positions.len() > 20, "{kind:?} on {name}: lit at night");
                let detail: usize = Lod::DETAIL.into_iter().flat_map(|l| g.detail_iter(l)).map(|(_, m)| m.indices.len() / 3).sum();
                assert!(detail > 0, "{kind:?} on {name}: its fine detail goes in the LOD layers");
            }
        }
    }
}

#[test]
fn no_two_cities_get_the_same_copy() {
    let lot = lot_of(&rect(70.0, 48.0), 0.4, Vec2::ZERO);
    for kind in ARCHITECTS {
        let a = draw(&lot, kind, 11, None);
        let b = draw(&lot, kind, 12, None);
        assert!(a == draw(&lot, kind, 11, None), "{kind:?}: the same seed draws the same building");
        assert_ne!(a.get(Surface::Glow).positions, b.get(Surface::Glow).positions, "{kind:?}: another seed varies it");
    }
}

/// The footprint of what stands above head height, square metres of the
/// bounding box (lot frame).
fn built_extent(g: &Geometry, lot: &Lot) -> f32 {
    let f = Frame::from_lot(lot);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for s in [Surface::Concrete, Surface::Plaster, Surface::Brick, Surface::Stone, Surface::Metal, Surface::Glass] {
        for p in &g.get(s).positions {
            if p[1] > 2.5 {
                let q = f.to_local(Vec2::new(p[0], p[2]));
                lo = lo.min(q);
                hi = hi.max(q);
            }
        }
    }
    ((hi.x - lo.x) * (hi.y - lo.y)).max(0.0)
}

#[test]
fn larger_sites_get_bigger_buildings() {
    for kind in ARCHITECTS {
        let small = lot_of(&rect(46.0, 40.0), 0.0, Vec2::ZERO);
        let large = lot_of(&rect(100.0, 80.0), 0.0, Vec2::ZERO);
        let (a, b) = (draw(&small, kind, 5, None), draw(&large, kind, 5, None));
        assert!(built_extent(&b, &large) > built_extent(&a, &small) * 1.3, "{kind:?}: {:.0} m² on the big site against {:.0} m²", built_extent(&b, &large), built_extent(&a, &small));
    }
}

#[test]
fn the_ledge_house_reaches_toward_the_water_and_only_there() {
    // The lot faces the street at -Y; water off its east side or its west.
    let lot = lot_of(&rect(60.0, 44.0), 0.0, Vec2::ZERO);
    let centre = lot.center;
    for (view, water) in [(Vec2::X, true), (Vec2::NEG_X, true), (Vec2::X, false)] {
        let site = SiteLook { view, water: water.then_some(8.0), fall: if water { 2.5 } else { 3.0 } };
        let g = draw(&lot, ServiceKind::LedgeHouse, 9, Some(site));
        let trays = points(&g, Surface::Concrete);
        let reach = |d: Vec2| trays.iter().map(|p| (Vec2::new(p.x, p.z) - centre).dot(d)).fold(f32::MIN, f32::max);
        let (toward, away) = (reach(view), reach(-view));
        assert!(toward > away + 8.0, "the trays reach toward the view {view:?}: {toward:.1} m that way, {away:.1} m the other");
        // Past the lot line only on the water side, and only a little.
        for p in g.iter().flat_map(|(_, m)| m.positions.iter()) {
            let q = Vec2::new(p[0], p[2]);
            if point_in_polygon(q, &lot.polygon) {
                continue;
            }
            let out = parts::distance_to_boundary(q, &lot.polygon);
            let toward = (q - centre).normalize_or_zero().dot(view);
            assert!(out <= 1.2 || (water && toward > 0.5 && out <= 3.0), "{view:?} water {water}: a vertex {out:.1} m off the lot at {q:?}");
        }
        // Over a drop it stands on stone piers down the falling ground.
        let low = g.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(low < -1.5, "{view:?}: piers down the drop, lowest {low:.1}");
    }
}

#[test]
fn the_chapel_roof_rises_to_the_view() {
    let lot = lot_of(&rect(50.0, 44.0), 0.0, Vec2::ZERO);
    let centre = lot.center;
    for view in [Vec2::X, Vec2::NEG_X, Vec2::Y] {
        let g = draw(&lot, ServiceKind::HillChapel, 21, Some(SiteLook { view, water: None, fall: 3.0 }));
        let top = points(&g, Surface::Concrete).into_iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
        let toward = (Vec2::new(top.x, top.z) - centre).dot(view);
        assert!(toward > 2.0, "the roof's high prow is toward {view:?}: {toward:.1}");
    }
}

#[test]
fn the_library_fan_opens_toward_the_view() {
    let lot = lot_of(&rect(70.0, 60.0), 0.0, Vec2::ZERO);
    let centre = lot.center;
    let left = Vec2::new(-0.7, 0.7).normalize();
    let right = Vec2::new(0.7, 0.7).normalize();
    let lean = |view: Vec2| {
        let g = draw(&lot, ServiceKind::FanLibrary, 4, Some(SiteLook { view, water: Some(20.0), fall: 1.0 }));
        // The skylights' glow sits on the fan's roof.
        let lights: Vec<Vec3> = points(&g, Surface::Glow).into_iter().filter(|p| p.y > 5.0 && p.y < 8.5).collect();
        (centroid2(&lights) - centre).x
    };
    assert!(lean(right) > lean(left) + 3.0, "the fan turns with the view: {:.1} against {:.1}", lean(right), lean(left));
}

#[test]
fn the_sky_slab_runs_along_the_long_side() {
    for (w, d) in [(110.0f32, 44.0f32), (44.0, 110.0)] {
        let lot = lot_of(&rect(w, d), 0.0, Vec2::ZERO);
        let g = draw(&lot, ServiceKind::SkySlab, 2, None);
        let f = Frame::from_lot(&lot);
        let high: Vec<Vec2> = points(&g, Surface::Concrete).into_iter().filter(|p| p.y > 15.0).map(|p| f.to_local(Vec2::new(p.x, p.z))).collect();
        let (lo, hi) = high.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
        let (ex, ey) = (hi.x - lo.x, hi.y - lo.y);
        if w > d {
            assert!(ex > ey * 2.0, "{w}x{d}: the slab runs along the frontage ({ex:.0} x {ey:.0})");
        } else {
            assert!(ey > ex * 2.0, "{w}x{d}: the slab runs back from the street ({ex:.0} x {ey:.0})");
        }
        assert!(high.len() > 100);
    }
}

#[test]
fn a_site_looks_downhill_when_the_ground_falls_away() {
    let land = nd_sim::landscape::Landscape::generate(&nd_sim::landscape::LandscapeParams::flat());
    let poly: Vec<Vec2> = rect(40.0, 40.0).into_iter().map(|p| p + Vec2::new(0.0, -20.0)).collect();
    // Level ground: no view.
    let flat = look_at(&poly, Vec2::ZERO, 5.0, &land, |_| 5.0);
    assert_eq!(flat.view, Vec2::ZERO);
    assert!(flat.water.is_none() && flat.fall == 0.0);
    // Ground falling away to the east: it looks east, and knows the drop.
    let slope = look_at(&poly, Vec2::ZERO, 5.0, &land, |p| 5.0 - p.x * 0.15);
    assert!(slope.view.dot(Vec2::X) > 0.9, "{:?}", slope.view);
    assert!(slope.fall > 3.0 && slope.water.is_none(), "{slope:?}");
}
