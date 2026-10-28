//! [r4-solarpunk] Solarpunk Green's styles: drawn clean on every odd lot,
//! at every height, tier, condition and lighting mode; building and
//! demolition sites; no coplanar faces; canopies, bulbs, grow lights and
//! greenhouses lit only after dark; within a triangle budget; drawn only
//! from the look's menus, the estates' forms build the villa, a vertical
//! farm and a bike shop their own styles, farms keep their farmsteads.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use crate::surface::Surface;
use nd_sim::districts::Look;

fn solarpunk(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::SolarpunkGreen), dynamic_lights, ..Extras::default() }
}

/// Every Solarpunk style on every odd lot and a street lot, from one floor
/// to many, at every tier and condition, lights fixed and switchable, drawn
/// twice (deterministic) and checked as the main sweep checks every style.
#[test]
fn solarpunk_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("big lot", test_lots(LandUse::Commercial)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::SOLARPUNK.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 5, 9].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = solarpunk(dynamic_lights);
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
    assert!(bad.is_empty(), "{} of {cases} solarpunk cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Solarpunk style at every stage.
#[test]
fn solarpunk_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::SOLARPUNK.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [2u8, 3, 5][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..solarpunk(true) }, Extras { demolition: Some(p), ..solarpunk(true) }] {
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

/// No two differently coloured faces of a Solarpunk building share a plane
/// (z-fighting), base and detail layers together.
#[test]
fn solarpunk_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::SOLARPUNK {
                for floors in [2u8, 3, 5, 8] {
                    for tier in [Tier::Low, Tier::Mid, Tier::High] {
                        for (cond, dynamic_lights) in [(0.9, false), (0.9, true), (0.3, true)] {
                            let g = generate_styled_extras(lot, lu, floors, tier, cond, 42 + floors as u64, style, &rules, &pal, &solarpunk(dynamic_lights));
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

/// Every Solarpunk style glows after dark and only then (a canopy's
/// underside, bulbs, grow lights, a greenhouse's lamps, a lamp by the gate:
/// base glow marked `NIGHT_MARK`, so nothing glows by day), keeps its grain
/// in the detail layers and stays within a triangle budget.
#[test]
fn solarpunk_lights_after_dark_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let mut report = Vec::new();
    for style in Style::SOLARPUNK {
        let mut night = 0;
        for seed in 0..4u64 {
            let g = generate_styled_extras(&lots[0], LandUse::Commercial, 4, Tier::Mid, 0.9, 30 + seed, style, &rules, &pal, &solarpunk(true));
            night += g.get(Surface::Glow).colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01).count();
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
    }
    eprintln!("{}", report.join("\n"));
}

/// The look's menus hold only Solarpunk styles (industry too, not the
/// villa), a Solarpunk building is drawn from them, a farm keeps its
/// farmstead in a Solarpunk district, no land use's own menu holds one;
/// the estates' forms build the villa in the look and their own styles out
/// of it; a vertical farm and a bike shop there build the look's own, and
/// out of it their usual ones.
#[test]
fn solarpunk_menus_forms_and_trades() {
    use nd_sim::estate::Form;
    use nd_sim::BusinessKind as B;
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::SolarpunkGreen, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::SOLARPUNK.contains(s) && *s != Style::SolarVilla && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 3, tier, seed, &rules, &solarpunk(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::SOLARPUNK.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lots = test_lots(LandUse::Industrial);
    for kind in [B::GrainFarm, B::Dairy] {
        let farm = Extras { business: Some(kind), ..solarpunk(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Industrial, 2, Tier::Low, 3, &rules, &farm), Style::Farmstead, "{kind:?}");
    }
    for (kind, lu, want, usual) in [(B::VertFarm, LandUse::Industrial, Style::VerticalFarm, Style::Farmstead), (B::BikeShop, LandUse::Commercial, Style::RepairCafe, Style::BikeShop)] {
        let lots = test_lots(lu);
        for seed in 0..4 {
            let ex = Extras { business: Some(kind), ..solarpunk(false) };
            assert_eq!(choose_style(&lots[0], lu, 3, Tier::Mid, seed, &rules, &ex), want, "{kind:?}");
            let out = Extras { business: Some(kind), ..Extras::default() };
            assert_eq!(choose_style(&lots[0], lu, 3, Tier::Mid, seed, &rules, &out), usual, "{kind:?} out of the look");
        }
    }
    let lots = test_lots(LandUse::Residential);
    for form in [Form::McMansion, Form::Mansion] {
        let ex = Extras { form: Some(form), ..solarpunk(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::SolarVilla);
        let plain = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &plain), Style::for_form(Some(form)).unwrap());
    }
}

/// The villa on a family's two plots is one house with its roof garden;
/// on a billionaire's four it grows a second wing and a second storey up
/// top: more of it, and taller.
#[test]
fn solar_villa_grows_on_an_estate() {
    use nd_sim::estate::Form;
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let small = sweep_tests::lot_of(&sweep_tests::rect(30.0, 40.0), 0.2, Vec2::new(40.0, 10.0));
    let big = sweep_tests::lot_of(&sweep_tests::rect(64.0, 52.0), 0.2, Vec2::new(40.0, 10.0));
    let draw = |lot: &Lot, form| generate_styled_extras(lot, LandUse::Residential, 2, Tier::High, 0.9, 11, Style::SolarVilla, &rules, &pal, &Extras { form: Some(form), ..solarpunk(true) });
    let (a, b) = (draw(&small, Form::McMansion), draw(&big, Form::Mansion));
    let top = |g: &Geometry| g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
    assert!(b.triangle_count() > a.triangle_count(), "{} vs {}", b.triangle_count(), a.triangle_count());
    assert!(top(&b) > top(&a) + 2.0, "{} vs {}", top(&b), top(&a));
}
