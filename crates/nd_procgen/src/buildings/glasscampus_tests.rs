//! [r4-glass] Glass Campus's styles: drawn clean on every odd lot, at
//! every height, tier, condition and lighting mode; building and
//! demolition sites; no coplanar faces; atria, lounges, lamps and
//! floodlights lit only after dark and the office floors switched by the
//! renderer; within a triangle budget; drawn only from the look's menus,
//! the estates' forms build the glass pavilion, the assembly halls server
//! halls; and the server hall stands behind a fence round its lot.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use crate::surface::Surface;
use nd_sim::districts::Look;

fn campus(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::GlassCampus), dynamic_lights, ..Extras::default() }
}

/// Every Glass Campus style on every odd lot, a street lot and a big lot,
/// from one floor to many, at every tier and condition, lights fixed and
/// switchable, drawn twice (deterministic) and checked as the main sweep
/// checks every style.
#[test]
fn glass_campus_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("big lot", test_lots(LandUse::Commercial)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::GLASS_CAMPUS.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 6, 12, 40].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = campus(dynamic_lights);
                    let g = generate_styled_extras(lot, lu, floors, tier, cond, 700 + k as u64, style, &rules, &pal, &ex);
                    cases += 1;
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} {floors}f {tier:?} {lu:?} dyn {dynamic_lights}: {kind}: {what}"));
                    }
                    if dynamic_lights && fi % 2 == 0 {
                        let again = generate_styled_extras(lot, lu, floors, tier, cond, 700 + k as u64, style, &rules, &pal, &ex);
                        if again != g {
                            bad.push(format!("{style:?} on {name} {floors}f: not deterministic"));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {cases} glass campus cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Glass Campus style at every stage.
#[test]
fn glass_campus_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::GLASS_CAMPUS.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [2u8, 4, 9][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..campus(true) }, Extras { demolition: Some(p), ..campus(true) }] {
                    let g = generate_styled_extras(lot, LandUse::Commercial, floors, Tier::Mid, 0.8, 9 + li as u64, style, &rules, &pal, &ex);
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} site {:?} demo {:?}: {kind}: {what}", ex.construction, ex.demolition));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} faulty sites:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// No two differently coloured faces of a Glass Campus building share a
/// plane (z-fighting), base and detail layers together.
#[test]
fn glass_campus_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::GLASS_CAMPUS {
                for floors in [2u8, 4, 9] {
                    for tier in [Tier::Low, Tier::Mid, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, style, &rules, &pal, &campus(dynamic_lights));
                            for c in clash_list(&g) {
                                bad.push(format!("{lu:?} {style:?} {floors}f {tier:?} dyn {dynamic_lights}: {}", c.what));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut kinds: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    for b in &bad {
        let style = b.split(' ').nth(1).unwrap_or("");
        let what: String = b.split(": ").nth(1).unwrap_or("").split(" at ").next().unwrap_or("").chars().filter(|c| c.is_alphabetic() || *c == ' ').collect();
        let e = kinds.entry(format!("{style} {what}")).or_insert((0, b.clone()));
        e.0 += 1;
    }
    let lines: Vec<String> = kinds.values().map(|(n, e)| format!("{n:5} x {e}")).collect();
    assert!(bad.is_empty(), "{} coplanar clashes:\n{}", bad.len(), lines.join("\n"));
}

/// Every Glass Campus style has light that shows only after dark (an
/// atrium, a lounge, lamps, floodlights: base glow marked `NIGHT_MARK`, so
/// nothing of it glows by day and it reads from across the city at night),
/// the towers and offices window lights the renderer switches, keeps its
/// grain in the detail layers and stays within a triangle budget.
#[test]
fn glass_campus_lights_after_dark_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let mut report = Vec::new();
    for style in Style::GLASS_CAMPUS {
        let (mut night, mut windows) = (0, 0);
        for seed in 0..4u64 {
            let g = generate_styled_extras(&lots[0], LandUse::Commercial, 6, Tier::Mid, 0.9, 30 + seed, style, &rules, &pal, &campus(true));
            let glow = g.get(Surface::Glow);
            night += glow.colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01).count();
            windows += glow.colors.iter().filter(|c| c[3] >= WINDOW_LIGHT_MARK).count();
            assert!(g.has_detail(), "{style:?}: detail layers");
            let base = g.base_triangle_count();
            let all = g.triangle_count();
            if seed == 0 {
                report.push(format!("{style:?}: {base} base + {} detail tris", all - base));
            }
            assert!(base < 12_000, "{style:?}: {base} base triangles");
            assert!(all < 40_000, "{style:?}: {all} triangles");
        }
        assert!(night >= 24, "{style:?}: {night} night-only glow vertices over four seeds");
        if style != Style::ServerHall {
            assert!(windows > 0, "{style:?}: no switchable window lights");
        }
    }
    eprintln!("{}", report.join("\n"));
}

/// The look's menus hold only Glass Campus styles (industry too, not the
/// pavilion), a Glass Campus building is drawn from them, a farm keeps its
/// own look in a Glass Campus district, no land use's own menu holds one;
/// the estates' forms build the pavilion in the look and their own styles
/// out of it; the assembly halls there are server halls (the cheapest keep
/// theirs), and out of it their usual ones.
#[test]
fn glass_campus_menus_forms_and_trades() {
    use nd_sim::estate::Form;
    use nd_sim::BusinessKind as B;
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::GlassCampus, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::GLASS_CAMPUS.contains(s) && *s != Style::GlassPavilion && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 3, tier, seed, &rules, &campus(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::GLASS_CAMPUS.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lots = test_lots(LandUse::Industrial);
    let farm = Extras { business: Some(B::GrainFarm), ..campus(false) };
    assert_eq!(choose_style(&lots[0], LandUse::Industrial, 2, Tier::Low, 3, &rules, &farm), Style::Farmstead);
    for tier in [Tier::Mid, Tier::High] {
        let hall = Extras { business: Some(B::Assembly), ..campus(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Industrial, 2, tier, 3, &rules, &hall), Style::ServerHall);
        let out = Extras { business: Some(B::Assembly), ..Extras::default() };
        assert!(!Style::GLASS_CAMPUS.contains(&choose_style(&lots[0], LandUse::Industrial, 2, tier, 3, &rules, &out)));
    }
    let cheap = Extras { business: Some(B::Assembly), ..campus(false) };
    assert!(!Style::GLASS_CAMPUS.contains(&choose_style(&lots[0], LandUse::Industrial, 2, Tier::Low, 3, &rules, &cheap)));
    let lots = test_lots(LandUse::Residential);
    for form in [Form::McMansion, Form::Mansion] {
        let ex = Extras { form: Some(form), ..campus(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::GlassPavilion);
        let plain = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &plain), Style::for_form(Some(form)).unwrap());
    }
}

/// The server hall's security fence runs round its lot: fence posts and
/// mesh near every edge of the parcel, barbed wire on top.
#[test]
fn server_hall_stands_behind_a_fence() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = test_lots(LandUse::Industrial)[0].clone();
    let g = generate_styled_extras(&lot, LandUse::Industrial, 2, Tier::Mid, 0.9, 5, Style::ServerHall, &rules, &pal, &campus(true));
    let edge = |p: &[f32; 3]| parts::distance_to_boundary(Vec2::new(p[0], p[2]), &lot.polygon) < 1.0;
    let wire = g.get(Surface::Rust).positions.iter().filter(|p| p[1] > 2.6 && edge(p)).count();
    assert!(wire > 20, "barbed wire on the fence: {wire} vertices");
    let near_edge = g.get(Surface::Matte).positions.iter().filter(|p| edge(p) && p[1] > 1.0).count();
    assert!(near_edge > 100, "fence along the lot's edges: {near_edge} vertices");
}
