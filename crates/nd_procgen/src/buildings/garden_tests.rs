//! [garden] The Garden Suburb's styles: drawn clean on every odd lot, a
//! street lot and a wide one at every height, tier, condition and lighting
//! mode; building and demolition sites at every stage; no coplanar faces;
//! low (two storeys and a roof) and within a triangle budget, most of the
//! small parts in the detail layers; only ever drawn from the look's menus;
//! the family's and the billionaire's house in the look is its manor; and
//! the porch lanterns light only after dark.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, lot_of, odd_lots, rect};
use super::*;
use nd_sim::districts::Look;
use nd_sim::estate::Form;

fn garden(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::Garden), dynamic_lights, ..Extras::default() }
}

fn top(g: &Geometry) -> f32 {
    g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0, f32::max)
}

/// The lots the styles are tried on: the odd ones, a street lot, and a wide
/// deep one (a close, a parade, a manor).
fn lots() -> Vec<(&'static str, Lot)> {
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("60x55", lot_of(&rect(60.0, 55.0), -0.6, Vec2::new(-80.0, 140.0))));
    lots
}

/// The land use a Garden style is built on.
fn land_use(s: Style) -> LandUse {
    if matches!(s, Style::ShoppingParade | Style::GardenCentre | Style::CornerCafe) { LandUse::Commercial } else { LandUse::Residential }
}

/// Every Garden style on every lot, at every tier and condition, lights
/// fixed and switchable, drawn twice (deterministic) and checked as the
/// main sweep checks every style.
#[test]
fn garden_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::GARDEN.into_iter().enumerate() {
        for (li, (name, lot)) in lots().iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 6, 30].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                for dynamic_lights in [false, true] {
                    let mut ex = garden(dynamic_lights);
                    if style == Style::GardenManor && k % 2 == 0 {
                        ex.form = Some(Form::Mansion);
                    }
                    let g = generate_styled_extras(lot, land_use(style), floors, tier, cond, 700 + k as u64, style, &rules, &pal, &ex);
                    cases += 1;
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} {floors}f {tier:?} dyn {dynamic_lights}: {kind}: {what}"));
                    }
                    if dynamic_lights && fi % 2 == 0 {
                        let again = generate_styled_extras(lot, land_use(style), floors, tier, cond, 700 + k as u64, style, &rules, &pal, &ex);
                        if again != g {
                            bad.push(format!("{style:?} on {name} {floors}f: not deterministic"));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {cases} garden cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Garden style at every stage.
#[test]
fn garden_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in lots().iter().enumerate() {
        for style in Style::GARDEN {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                for ex in [Extras { construction: Some(p), ..garden(true) }, Extras { demolition: Some(p), ..garden(true) }] {
                    let g = generate_styled_extras(lot, land_use(style), 2, Tier::Mid, 0.8, 9 + li as u64, style, &rules, &pal, &ex);
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} site {:?} demo {:?}: {kind}: {what}", ex.construction, ex.demolition));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} faulty sites:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// No two differently coloured faces of a Garden building share a plane
/// (z-fighting): doors over their frames over the plinth, panes and bars,
/// the lawn, beds, drives, paths and patios, fences against fences.
#[test]
fn garden_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    let mut lots: Vec<Lot> = test_lots(LandUse::Residential).into_iter().take(3).collect();
    lots.push(lot_of(&rect(60.0, 55.0), -0.6, Vec2::new(-80.0, 140.0)));
    for (li, lot) in lots.iter().enumerate() {
        for style in Style::GARDEN {
            for floors in [1u8, 2, 3] {
                for tier in Tier::ALL {
                    for dynamic_lights in [false, true] {
                        let seed = 42 + floors as u64 + li as u64 * 7 + tier as u64 * 3;
                        let g = generate_styled_extras(lot, land_use(style), floors, tier, 0.9, seed, style, &rules, &pal, &garden(dynamic_lights));
                        for c in clash_list(&g) {
                            bad.push(format!("lot {li} {style:?} {floors}f {tier:?} dyn {dynamic_lights}: {}", c.what));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} coplanar clashes, e.g.:\n{}", bad.len(), bad[..bad.len().min(20)].join("\n"));
}

/// The suburb stays low whatever floors the site counts (two storeys and
/// rooms in the roof, a chimney and an aerial over it), within a triangle
/// budget, and keeps most of its small parts out of the far view: the base
/// mesh is under half of a house's triangles.
#[test]
fn garden_styles_stay_low_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut report = Vec::new();
    for (name, lot) in [("street lot", test_lots(LandUse::Residential)[0].clone()), ("60x55", lot_of(&rect(60.0, 55.0), 0.0, Vec2::ZERO))] {
        for style in Style::GARDEN {
            for floors in [1u8, 2, 3, 12] {
                let ex = Extras { form: (style == Style::GardenManor).then_some(Form::Mansion), ..garden(true) };
                let g = generate_styled_extras(&lot, land_use(style), floors, Tier::High, 0.9, 11, style, &rules, &pal, &ex);
                let (h, tris, base) = (top(&g), g.triangle_count(), g.base_triangle_count());
                report.push(format!("{style:?} on {name} {floors}f: {h:.1} m, {tris} tris ({base} base, {} mid, {} near)", g.detail_triangle_count(crate::surface::Lod::Mid), g.detail_triangle_count(crate::surface::Lod::Near)));
                let max_h = if style == Style::GardenManor { 22.0 } else { 17.0 };
                assert!(h > 3.0 && h < max_h, "{style:?} {floors}f on {name}: {h:.1} m tall");
                assert!(tris < 90_000, "{style:?} {floors}f on {name}: {tris} triangles");
                if matches!(style, Style::GardenHouse | Style::Bungalow | Style::CulDeSac) {
                    assert!(base * 2 < tris, "{style:?} {floors}f on {name}: {base} of {tris} triangles in the base mesh");
                }
            }
        }
    }
    eprintln!("{}", report.join("\n"));
}

/// The look's menus hold only Garden styles (the manor is never on one),
/// a Garden building is drawn from them, and no land use's own menu holds
/// one; nights out keep the land use's looks.
#[test]
fn garden_menus() {
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::Garden, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::GARDEN.contains(s) && *s != Style::GardenManor && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 2, tier, seed, &rules, &garden(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    assert!(district::look_styles(Look::Garden, LandUse::Entertainment, Tier::Mid).is_none());
    for w in &rules.styles {
        assert!(!Style::GARDEN.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
}

/// [garden] A family's McMansion and a billionaire's mansion in the Garden
/// Suburb are its garden manor; elsewhere (no look, another look) they stay
/// the city's McMansion and mansion.
#[test]
fn mansions_in_the_garden_suburb_are_garden_manors() {
    let rules = BuildingRules::default();
    let lot = lot_of(&rect(60.0, 55.0), 0.3, Vec2::new(40.0, 40.0));
    for (form, own) in [(Form::McMansion, Style::McMansion), (Form::Mansion, Style::Mansion)] {
        let ex = Extras { form: Some(form), look: Some(Look::Garden), ..Extras::default() };
        assert_eq!(choose_style(&lot, LandUse::Residential, 2, Tier::High, 5, &rules, &ex), Style::GardenManor, "{form:?}");
        for look in [None, Some(Look::Suburban), Some(Look::Capsule)] {
            let ex = Extras { form: Some(form), look, ..Extras::default() };
            assert_eq!(choose_style(&lot, LandUse::Residential, 2, Tier::High, 5, &rules, &ex), own, "{form:?} {look:?}");
        }
    }
    // Other forms keep theirs in the suburb.
    let ex = Extras { form: Some(Form::StripMall), look: Some(Look::Garden), ..Extras::default() };
    assert_eq!(choose_style(&lot, LandUse::Commercial, 2, Tier::Mid, 5, &rules, &ex), Style::StripMall);
    // A billionaire's manor is grander than a family's on the same site.
    let pal = Palette::default();
    let draw = |form| generate_styled_extras(&lot, LandUse::Residential, 2, Tier::High, 0.9, 5, Style::GardenManor, &rules, &pal, &Extras { form: Some(form), ..garden(true) }).triangle_count();
    assert!(draw(Form::Mansion) >= draw(Form::McMansion));
}

/// The streets stay dark: no lamp posts on a Garden lot, and every house
/// has a lantern by its door that is lit only after dark (`NIGHT_MARK`).
#[test]
fn porch_lights_light_after_dark() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = test_lots(LandUse::Residential)[0].clone();
    for style in [Style::GardenHouse, Style::Bungalow, Style::GardenManor] {
        let g = generate_styled_extras(&lot, LandUse::Residential, 2, Tier::Mid, 0.9, 3, style, &rules, &pal, &garden(true));
        let night = g.get(crate::surface::Surface::Glow).colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 1e-3).count();
        assert!(night >= 24, "{style:?}: {night} vertices of night-only glow (one lantern is 24)");
        assert!(g.lamps.is_empty(), "{style:?}: lamp posts on a garden lot");
    }
}
