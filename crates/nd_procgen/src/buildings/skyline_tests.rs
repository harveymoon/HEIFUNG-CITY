//! [r4-skyline] Skyline Luxe's styles: drawn clean on every odd lot, at
//! every height, tier, condition and lighting mode; building and
//! demolition sites; no coplanar faces; pools, soffits, bridges, lamps and
//! helipad lights lit only after dark and the tower floors switched by the
//! renderer; within a triangle budget, tall towers too (their grain in the
//! detail layers); as tall as the sim's floors; drawn only from the look's
//! menus, the estates' forms build the sky villa, hotels the helipad tower.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use crate::surface::Surface;
use nd_sim::districts::Look;

fn luxe(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::SkylineLuxe), dynamic_lights, ..Extras::default() }
}

/// The top of a building's mass (walls and glass, not masts and lights).
fn mass_top(g: &Geometry) -> f32 {
    [Surface::Glass, Surface::Concrete, Surface::Panel, Surface::Stone, Surface::WornConcrete, Surface::WornPanel].iter().flat_map(|s| g.get(*s).positions.iter().map(|p| p[1])).fold(0.0, f32::max)
}

/// Every Skyline Luxe style on every odd lot, a street lot and a big lot,
/// from one floor to many, at every tier and condition, lights fixed and
/// switchable, drawn twice (deterministic) and checked as the main sweep
/// checks every style.
#[test]
fn skyline_luxe_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("big lot", test_lots(LandUse::Commercial)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::SKYLINE_LUXE.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 6, 12, 40].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment][k % 3];
                for dynamic_lights in [false, true] {
                    let ex = luxe(dynamic_lights);
                    let g = generate_styled_extras(lot, lu, floors, tier, cond, 900 + k as u64, style, &rules, &pal, &ex);
                    cases += 1;
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} {floors}f {tier:?} {lu:?} dyn {dynamic_lights}: {kind}: {what}"));
                    }
                    if dynamic_lights && fi % 2 == 0 {
                        let again = generate_styled_extras(lot, lu, floors, tier, cond, 900 + k as u64, style, &rules, &pal, &ex);
                        if again != g {
                            bad.push(format!("{style:?} on {name} {floors}f: not deterministic"));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {cases} skyline luxe cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Skyline Luxe style at every stage.
#[test]
fn skyline_luxe_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::SKYLINE_LUXE.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [3u8, 8, 20][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..luxe(true) }, Extras { demolition: Some(p), ..luxe(true) }] {
                    let g = generate_styled_extras(lot, LandUse::Commercial, floors, Tier::High, 0.8, 9 + li as u64, style, &rules, &pal, &ex);
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} site {:?} demo {:?}: {kind}: {what}", ex.construction, ex.demolition));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} faulty sites:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// No two differently coloured faces of a Skyline Luxe building share a
/// plane (z-fighting), base and detail layers together.
#[test]
fn skyline_luxe_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::SKYLINE_LUXE {
                for floors in [2u8, 5, 12, 24] {
                    for tier in [Tier::Mid, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, style, &rules, &pal, &luxe(dynamic_lights));
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

/// Every Skyline Luxe style has light that shows only after dark (soffit
/// strips, pools, the bridge, lamps, the pad's lights: base glow marked
/// `NIGHT_MARK`), window lights the renderer switches a floor at a time,
/// keeps its grain in the detail layers and stays within a triangle
/// budget: at six floors as the other looks, and a forty-floor tower
/// within the city's budget for its height, its far (base) mesh no more
/// than a few thousand triangles over the six-floor one.
#[test]
fn skyline_luxe_lights_after_dark_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let lot = &lots[0];
    let area = nd_sim::zoning::polygon_area(&lot.polygon).abs() as usize;
    let mut report = Vec::new();
    for style in Style::SKYLINE_LUXE {
        let (mut night, mut windows) = (0, 0);
        for seed in 0..4u64 {
            let g = generate_styled_extras(lot, LandUse::Commercial, 6, Tier::High, 0.9, 30 + seed, style, &rules, &pal, &luxe(true));
            let glow = g.get(Surface::Glow);
            night += glow.colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01).count();
            windows += glow.colors.iter().filter(|c| c[3] >= WINDOW_LIGHT_MARK).count();
            assert!(g.has_detail(), "{style:?}: detail layers");
            let base = g.base_triangle_count();
            let all = g.triangle_count();
            assert!(base < 12_000, "{style:?}: {base} base triangles");
            assert!(all < 40_000, "{style:?}: {all} triangles");
            let tall = generate_styled_extras(lot, LandUse::Commercial, 40, Tier::High, 0.9, 30 + seed, style, &rules, &pal, &luxe(true));
            let (tb, ta) = (tall.base_triangle_count(), tall.triangle_count());
            assert!(tb < 4000 + 300 * 40 + 2 * area, "{style:?} 40f: {tb} base triangles");
            assert!(tb < base + 6000, "{style:?}: 40 floors {tb} base against 6 floors {base}");
            assert!(ta - tb < 3 * (4000 + 300 * 40 + 2 * area), "{style:?} 40f: {} detail triangles", ta - tb);
            if seed == 0 {
                report.push(format!("{style:?}: 6f {base} base + {} detail; 40f {tb} base + {} detail", all - base, ta - tb));
            }
        }
        assert!(night >= 24, "{style:?}: {night} night-only glow vertices over four seeds");
        assert!(windows > 0, "{style:?}: no switchable window lights");
    }
    eprintln!("{}", report.join("\n"));
}

/// The towers are as tall as the sim counts them: a refitted block keeps
/// its height, so the drawn mass rises with the floors and stays within a
/// storey and a half (plus the crown's parapet) of what the sim's floors
/// are; the low boutique row and the villa stay low.
#[test]
fn skyline_luxe_towers_follow_the_sims_floors() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = &test_lots(LandUse::Commercial)[0];
    for style in [Style::LuxeTower, Style::TwinTowers, Style::PoolTower, Style::HelipadTower, Style::MallPodium] {
        let mut last = 0.0;
        for floors in [6u8, 12, 24, 40] {
            let g = generate_styled_extras(lot, LandUse::Commercial, floors, Tier::High, 0.9, 7, style, &rules, &pal, &luxe(false));
            let top = mass_top(&g);
            let want = floors as f32 * rules.commercial.floor_height;
            assert!(top > last, "{style:?}: {floors} floors no taller ({top:.1} m) than fewer ({last:.1} m)");
            assert!((top - want).abs() < want * 0.25 + 6.0, "{style:?}: {floors} floors drawn {top:.1} m tall, the sim's {want:.1} m");
            last = top;
        }
    }
    let g = generate_styled_extras(lot, LandUse::Commercial, 3, Tier::High, 0.9, 7, Style::BoutiqueRow, &rules, &pal, &luxe(false));
    assert!(mass_top(&g) < 16.0, "the boutique row is low: {:.1} m", mass_top(&g));
}

/// The look's menus hold only Skyline Luxe styles (not the villa), a
/// Skyline Luxe building is drawn from them, it dresses no industry, no
/// land use's own menu holds one; the estates' forms build the sky villa
/// in the look and their own styles out of it; a hotel there is the
/// helipad tower, flagged for a pad or not, and out of it a hotel; a
/// flagged tower keeps the look (the helipad tower).
#[test]
fn skyline_luxe_menus_forms_and_trades() {
    use nd_sim::estate::Form;
    use nd_sim::BusinessKind as B;
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::SkylineLuxe, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::SKYLINE_LUXE.contains(s) && *s != Style::SkyVilla && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 8, tier, seed, &rules, &luxe(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    assert!(district::look_styles(Look::SkylineLuxe, LandUse::Industrial, Tier::Mid).is_none());
    for w in &rules.styles {
        assert!(!Style::SKYLINE_LUXE.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lots = test_lots(LandUse::Commercial);
    for pad in [false, true] {
        let hotel = Extras { business: Some(B::Hotel), rooftop_pad: pad, ..luxe(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Commercial, 12, Tier::High, 3, &rules, &hotel), Style::HelipadTower);
        let out = Extras { business: Some(B::Hotel), rooftop_pad: pad, ..Extras::default() };
        assert!(!Style::SKYLINE_LUXE.contains(&choose_style(&lots[0], LandUse::Commercial, 12, Tier::High, 3, &rules, &out)));
    }
    let flagged = Extras { rooftop_pad: true, ..luxe(false) };
    assert_eq!(choose_style(&lots[0], LandUse::Commercial, 14, Tier::High, 5, &rules, &flagged), Style::HelipadTower);
    let lots = test_lots(LandUse::Residential);
    for form in [Form::McMansion, Form::Mansion] {
        let ex = Extras { form: Some(form), ..luxe(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 2, Tier::High, 5, &rules, &ex), Style::SkyVilla);
        let plain = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 2, Tier::High, 5, &rules, &plain), Style::for_form(Some(form)).unwrap());
    }
}

/// The helipad tower carries its own pad for the air taxis on its roof;
/// the twin towers' sky bridge is lit after dark between them, above the
/// podium; the pool tower's water glows only after dark.
#[test]
fn skyline_luxe_pads_bridges_and_pools() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = &test_lots(LandUse::Commercial)[0];
    let g = generate_styled_extras(lot, LandUse::Commercial, 20, Tier::High, 0.9, 11, Style::HelipadTower, &rules, &pal, &luxe(true));
    assert_eq!(g.pads.len(), 1, "one helipad");
    assert!(g.pads[0].y > 50.0, "on the roof: {:.1} m", g.pads[0].y);
    let wide = test_lots(LandUse::Commercial).into_iter().find(|l| l.width() > 30.0).expect("a wide lot");
    let g = generate_styled_extras(&wide, LandUse::Residential, 24, Tier::High, 0.9, 11, Style::TwinTowers, &rules, &pal, &luxe(true));
    let night_high = g.get(Surface::Glow).positions.iter().zip(&g.get(Surface::Glow).colors).filter(|(p, c)| (c[3] - NIGHT_MARK).abs() < 0.01 && p[1] > 20.0).count();
    assert!(night_high >= 8, "a lit bridge up between the towers: {night_high} vertices");
    let g = generate_styled_extras(lot, LandUse::Residential, 12, Tier::High, 0.9, 11, Style::PoolTower, &rules, &pal, &luxe(true));
    let pool = g.get(Surface::Glow).positions.iter().zip(&g.get(Surface::Glow).colors).filter(|(p, c)| (c[3] - NIGHT_MARK).abs() < 0.01 && p[1] > 30.0).count();
    assert!(pool >= 8, "a pool lit after dark on the roof: {pool} vertices");
}

