//! [lantern] Lantern Alley's styles: drawn clean on every odd lot, at
//! every height, tier, condition and lighting mode; building and
//! demolition sites; no coplanar faces; lanterns lit only after dark and
//! their paper in the detail layers; within a triangle budget; drawn only
//! from the look's menus, and the estates' forms build the compound.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use crate::surface::{Lod, Surface};
use nd_sim::districts::Look;

fn lantern(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::Lantern), dynamic_lights, ..Extras::default() }
}

/// Every Lantern style on every odd lot and a street lot, from one floor
/// to many, at every tier and condition, lights fixed and switchable, drawn
/// twice (deterministic) and checked as the main sweep checks every style.
#[test]
fn lantern_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("big lot", test_lots(LandUse::Commercial)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::LANTERN.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 5, 9].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = lantern(dynamic_lights);
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
    assert!(bad.is_empty(), "{} of {cases} lantern cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Lantern style at every stage.
#[test]
fn lantern_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::LANTERN.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [2u8, 3, 5][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..lantern(true) }, Extras { demolition: Some(p), ..lantern(true) }] {
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

/// No two differently coloured faces of a Lantern building share a plane
/// (z-fighting), base and detail layers together.
#[test]
fn lantern_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::LANTERN {
                for floors in [2u8, 3, 5] {
                    for tier in [Tier::Low, Tier::Mid, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, style, &rules, &pal, &lantern(dynamic_lights));
                            for c in clash_list(&g) {
                                bad.push(format!("{lu:?} {style:?} {floors}f {tier:?} dyn {dynamic_lights}: {}", c.what));
                            }
                        }
                    }
                }
            }
        }
    }
    // One line per kind of clash (style and the two finishes), with a count.
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

/// Every Lantern style hangs lanterns: their paper is detail (dropped far
/// off) and their light is base glow marked for after dark only, so the
/// lanes glow from across the city at night and nothing glows red by day.
/// Each stays within a triangle budget, most of its grain in the detail.
#[test]
fn lanterns_light_after_dark_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let mut report = Vec::new();
    for style in Style::LANTERN {
        let mut night = 0;
        for seed in 0..4u64 {
            let g = generate_styled_extras(&lots[0], LandUse::Commercial, 4, Tier::Mid, 0.9, 30 + seed, style, &rules, &pal, &lantern(true));
            night += g.get(Surface::Glow).colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01).count();
            assert!(g.detail_iter(Lod::Mid).any(|(s, _)| s == Surface::Tarp), "{style:?}: lantern paper in the mid detail");
            let base = g.base_triangle_count();
            let all = g.triangle_count();
            if seed == 0 {
                report.push(format!("{style:?}: {base} base + {} detail tris", all - base));
            }
            assert!(base < 12_000, "{style:?}: {base} base triangles");
            assert!(all < 40_000, "{style:?}: {all} triangles");
        }
        assert!(night >= 24, "{style:?}: {night} night-only glow vertices over four seeds");
    }
    eprintln!("{}", report.join("\n"));
}

/// The look's menus hold only Lantern styles (industry too), a Lantern
/// building is drawn from them, a farm keeps its own look in a Lantern
/// district, no land use's own menu holds one, and the estates' forms
/// build the compound in the look and their own styles out of it.
#[test]
fn lantern_menus_and_forms() {
    use nd_sim::estate::Form;
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::Lantern, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::LANTERN.contains(s) && *s != Style::LanternCompound && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 4, tier, seed, &rules, &lantern(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::LANTERN.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lots = test_lots(LandUse::Industrial);
    let farm = Extras { business: Some(nd_sim::BusinessKind::GrainFarm), ..lantern(false) };
    assert_eq!(choose_style(&lots[0], LandUse::Industrial, 2, Tier::Low, 3, &rules, &farm), Style::Farmstead);
    let lots = test_lots(LandUse::Residential);
    for form in [Form::McMansion, Form::Mansion] {
        let ex = Extras { form: Some(form), ..lantern(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::LanternCompound);
        let plain = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &plain), Style::for_form(Some(form)).unwrap());
    }
    let hotel = Extras { form: Some(Form::MegaHotel), ..lantern(false) };
    assert_eq!(choose_style(&lots[0], LandUse::Commercial, 12, Tier::High, 5, &rules, &hotel), Style::MegaHotel);
}

