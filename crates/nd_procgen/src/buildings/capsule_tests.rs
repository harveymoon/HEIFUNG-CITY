//! [capsule] The Capsule district's styles: drawn clean on every odd lot
//! and at every height, tier, condition and lighting mode; building and
//! demolition sites at every stage; no coplanar faces; as tall as their
//! floors; within a triangle budget; and only ever drawn from the look's
//! menus.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use nd_sim::districts::Look;

fn capsule(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::Capsule), dynamic_lights, ..Extras::default() }
}

fn top(g: &Geometry) -> f32 {
    g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0, f32::max)
}

/// Every Capsule style on every odd lot, from a shed to a megatall, at
/// every tier and condition, lights fixed and switchable, drawn twice
/// (deterministic) and checked as the main sweep checks every style.
#[test]
fn capsule_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::CAPSULE.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 6, 12, 30, 80, 250].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment][k % 3];
                for dynamic_lights in [false, true] {
                    let ex = capsule(dynamic_lights);
                    let g = generate_styled_extras(lot, lu, floors, tier, cond, 500 + k as u64, style, &rules, &pal, &ex);
                    cases += 1;
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} {floors}f {tier:?} {lu:?} dyn {dynamic_lights}: {kind}: {what}"));
                    }
                    if dynamic_lights && fi % 3 == 0 {
                        let again = generate_styled_extras(lot, lu, floors, tier, cond, 500 + k as u64, style, &rules, &pal, &ex);
                        if again != g {
                            bad.push(format!("{style:?} on {name} {floors}f: not deterministic"));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {cases} capsule cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Capsule style at every stage.
#[test]
fn capsule_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::CAPSULE.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [2u8, 8, 30][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..capsule(true) }, Extras { demolition: Some(p), ..capsule(true) }] {
                    let g = generate_styled_extras(lot, LandUse::Residential, floors, Tier::Mid, 0.8, 9 + li as u64, style, &rules, &pal, &ex);
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} site {:?} demo {:?}: {kind}: {what}", ex.construction, ex.demolition));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} faulty sites:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// No two differently coloured faces of a Capsule building share a plane
/// (z-fighting): portholes over their frames over the curved walls, the
/// stripes, the lawn, paths and pads.
#[test]
fn capsule_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::CAPSULE {
                for floors in [1u8, 3, 9, 24] {
                    for tier in [Tier::Mid, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, style, &rules, &pal, &capsule(dynamic_lights));
                            for c in clash_list(&g) {
                                bad.push(format!("{lu:?} {style:?} {floors}f {tier:?} dyn {dynamic_lights}: {}", c.what));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} coplanar clashes, e.g.:\n{}", bad.len(), bad[..bad.len().min(20)].join("\n"));
}

/// A Capsule building is as tall as its floors (a high-tier tower stays a
/// tower), and stays within a triangle budget.
#[test]
fn capsule_heights_follow_floors_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let lot = &lots[0];
    let fh = rules.commercial.floor_height;
    let mut report = Vec::new();
    for style in Style::CAPSULE {
        for floors in [2u8, 6, 12, 30] {
            let g = generate_styled_extras(lot, LandUse::Commercial, floors, Tier::High, 1.0, 7, style, &rules, &pal, &capsule(true));
            let h = top(&g);
            // The low styles keep to six floors (as the villas keep to two).
            let low = matches!(style, Style::DomeHouse | Style::SaucerHouse | Style::TwinDomes | Style::DomePods);
            let want = floors.min(if low { 6 } else { 250 }) as f32 * fh;
            let tris = g.triangle_count();
            report.push(format!("{style:?} {floors}f: {h:.0} m (floors {want:.0} m), {tris} tris"));
            assert!(h >= want * 0.85, "{style:?} {floors} floors only {h:.1} m tall (want {want:.1})");
            assert!(h <= want * 1.6 + 24.0, "{style:?} {floors} floors {h:.1} m tall (want {want:.1})");
            assert!(tris < 60_000, "{style:?} {floors} floors: {tris} triangles");
        }
    }
    eprintln!("{}", report.join("\n"));
}

/// A Capsule tower the sim gave a rooftop pad keeps the look and draws
/// its own deck (one pad, high up).
#[test]
fn capsule_towers_carry_their_pads() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    for tier in [Tier::Mid, Tier::High] {
        let ex = Extras { rooftop_pad: true, ..capsule(true) };
        let s = choose_style(&lots[0], LandUse::Commercial, 20, tier, 3, &rules, &ex);
        assert!(Style::CAPSULE.contains(&s) && s.pad_capable(), "{tier:?}: {s:?}");
        for style in [Style::CapsuleTower, Style::SaucerStack, Style::StiltTower] {
            let g = generate_styled_extras(&lots[0], LandUse::Commercial, 20, tier, 1.0, 5, style, &rules, &pal, &ex);
            assert_eq!(g.pads.len(), 1, "{style:?}");
            assert!(g.pads[0].y > 50.0, "{style:?} pad at {}", g.pads[0].y);
            assert!(faults(&g, &lots[0], style).is_empty() && clash_list(&g).is_empty(), "{style:?}");
        }
    }
}

/// The look's menus hold only Capsule styles, a Capsule building is drawn
/// from them, and no land use's own menu holds one.
#[test]
fn capsule_menus() {
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::Capsule, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::CAPSULE.contains(s) && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 6, tier, seed, &rules, &capsule(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::CAPSULE.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
}
