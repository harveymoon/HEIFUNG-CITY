//! [r4-oldstone] The Old Stone styles (the Historic look; the Old Town
//! Charter): drawn clean on every odd lot and at every height, tier,
//! condition and lighting mode; no coplanar faces; within a triangle
//! budget with their small parts in the detail layers; gas lamps, lit
//! church windows and clock faces after dark; and only ever drawn from the
//! look's menus (or, for the stone manor, for a big house in the look).

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, lot_of, odd_lots, rect};
use super::*;
use nd_sim::districts::Look;

fn old(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::Historic), dynamic_lights, ..Extras::default() }
}

/// The odd lots, a street lot and the big lots the works and the manors
/// stand on (two and four plots).
fn lots() -> Vec<(&'static str, Lot)> {
    let mut v = odd_lots();
    v.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    v.push(("two plots", lot_of(&rect(44.0, 38.0), 0.21, Vec2::new(-80.0, 40.0))));
    v.push(("four plots", lot_of(&rect(75.0, 65.0), -0.6, Vec2::new(120.0, 260.0))));
    v
}

/// Every Old Stone style on every lot, from a cottage to a megatall, at
/// every tier and condition, lights fixed and switchable, drawn twice
/// (deterministic) and checked as the main sweep checks every style.
#[test]
fn old_stone_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = lots();
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::OLD_STONE.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 6, 12, 30, 250].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = old(dynamic_lights);
                    let g = generate_styled_extras(lot, lu, floors, tier, cond, 700 + k as u64, style, &rules, &pal, &ex);
                    cases += 1;
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} {floors}f {tier:?} {lu:?} dyn {dynamic_lights}: {kind}: {what}"));
                    }
                    if dynamic_lights && fi % 3 == 0 {
                        let again = generate_styled_extras(lot, lu, floors, tier, cond, 700 + k as u64, style, &rules, &pal, &ex);
                        if again != g {
                            bad.push(format!("{style:?} on {name} {floors}f: not deterministic"));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {cases} old stone cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Old Stone style.
#[test]
fn old_stone_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in lots().iter().enumerate() {
        for (si, style) in Style::OLD_STONE.into_iter().enumerate() {
            for k in [0, 2, 4] {
                let p = k as f32 / 4.0;
                let floors = [2u8, 3, 5][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..old(true) }, Extras { demolition: Some(p), ..old(true) }] {
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

/// No two differently coloured faces of an Old Stone building share a
/// plane (z-fighting): windows over surrounds over stone, the timber
/// framing over the render, arches, gables and crow steps, clock dials,
/// rose windows and their glow, pits, yards and pools of lamplight.
#[test]
fn old_stone_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    let big: Vec<Lot> = lots().into_iter().filter(|(n, _)| n.ends_with("plots")).map(|(_, l)| l).collect();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let mut ls: Vec<Lot> = test_lots(lu).into_iter().take(2).collect();
        ls.extend(big.iter().cloned());
        for lot in &ls {
            for style in Style::OLD_STONE {
                for floors in [1u8, 3, 8] {
                    for tier in [Tier::Low, Tier::High] {
                        for dynamic_lights in [false, true] {
                            for seed in [42u64, 43] {
                                let g = generate_styled_extras(lot, lu, floors, tier, 0.9, seed + floors as u64, style, &rules, &pal, &old(dynamic_lights));
                                for c in clash_list(&g) {
                                    bad.push(format!("{lu:?} {style:?} {floors}f {tier:?} dyn {dynamic_lights}: {}", c.what));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // One example per style and pair of surfaces, so each fault shows once.
    let mut seen = std::collections::BTreeMap::new();
    for b in &bad {
        let style = b.split_whitespace().nth(1).unwrap_or("");
        let pair = b.split(": ").nth(1).and_then(|x| x.split(" at ").next()).unwrap_or("");
        let pair: String = pair.split_whitespace().filter(|w| !w.contains(',')).collect::<Vec<_>>().join(" ");
        seen.entry(format!("{style} {pair}")).or_insert_with(|| b.clone());
    }
    assert!(bad.is_empty(), "{} coplanar clashes, e.g.:\n{}", bad.len(), seen.values().take(40).cloned().collect::<Vec<_>>().join("\n"));
}

/// Within a triangle budget, a share of it in the detail layers (so a far
/// view pays less), lit by gas lamps that only show after dark, and as low
/// as an old town is (rows of two to five storeys).
#[test]
fn old_stone_budget_detail_and_night_lamps() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut report = Vec::new();
    for (name, lot) in lots().into_iter().filter(|(n, _)| *n == "street lot" || n.ends_with("plots")) {
        for style in Style::OLD_STONE {
            for floors in [2u8, 6, 12] {
                let g = generate_styled_extras(&lot, LandUse::Commercial, floors, Tier::High, 0.7, 11, style, &rules, &pal, &old(true));
                let (tris, base) = (g.triangle_count(), g.base_triangle_count());
                report.push(format!("{style:?} on {name} {floors}f: {tris} tris, {base} base"));
                assert!(tris > 200, "{style:?} on {name}: only {tris} triangles");
                assert!(tris < 70_000, "{style:?} on {name} {floors}f: {tris} triangles");
                assert!(base * 10 < tris * 9, "{style:?} on {name} {floors}f: {base} of {tris} triangles in the base mesh");
                let night = g.get(crate::surface::Surface::Glow).colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01).count();
                assert!(night > 0, "{style:?} on {name} {floors}f: no gas lamp or night glow");
            }
        }
    }
    eprintln!("{}", report.join("\n"));
    let lot = &test_lots(LandUse::Residential)[0];
    let top = |g: &Geometry| g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0, f32::max);
    for style in [Style::StoneHouse, Style::GabledRow, Style::CourtyardHouse] {
        let g = generate_styled_extras(lot, LandUse::Residential, 12, Tier::Low, 1.0, 3, style, &rules, &pal, &old(false));
        assert!(top(&g) < 5.0 * 3.4 + 8.0, "{style:?} stays low: {:.1} m", top(&g));
    }
    // The clock tower's faces are lit after dark.
    let lot = &test_lots(LandUse::Commercial)[0];
    let g = generate_styled_extras(lot, LandUse::Commercial, 3, Tier::Mid, 0.3, 5, Style::ClockTower, &rules, &pal, &old(false));
    let lit = g.get(crate::surface::Surface::Glow).colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01 && c[0] > 1.6 && c[2] > 0.9).count();
    assert!(lit >= 4 * 16, "four lit clock dials: {lit} vertices");
}

/// The look's menus hold only Old Stone styles (for homes, shops, nights
/// out and industry), a building of the look is drawn from them, no land
/// use's own menu holds one, and an industry with a look of its own (a
/// refinery) keeps it in the Old Town.
#[test]
fn old_stone_menus() {
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::Historic, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::OLD_STONE.contains(s) && *s != Style::StoneManor && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 3, tier, seed, &rules, &old(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::OLD_STONE.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lot = &test_lots(LandUse::Industrial)[0];
    let ex = Extras { business: Some(nd_sim::BusinessKind::Refinery), ..old(false) };
    let s = choose_style(lot, LandUse::Industrial, 2, Tier::Mid, 1, &rules, &ex);
    assert!(!Style::OLD_STONE.contains(&s), "a refinery keeps its own look: {s:?}");
    let ex = Extras { business: Some(nd_sim::BusinessKind::Fab), ..old(false) };
    assert!(Style::OLD_STONE.contains(&choose_style(lot, LandUse::Industrial, 2, Tier::Mid, 1, &rules, &ex)));
}

/// [r4-oldstone] A family's big house or a billionaire's estate in the Old
/// Town is a stone manor; anywhere else the usual McMansion and mansion
/// (and in Foundry Row the loft estate).
#[test]
fn an_old_town_mansion_is_a_stone_manor() {
    use nd_sim::estate::Form;
    let rules = BuildingRules::default();
    let lot = lots().into_iter().find(|(n, _)| *n == "four plots").unwrap().1;
    for (form, plain) in [(Form::McMansion, Style::McMansion), (Form::Mansion, Style::Mansion)] {
        let ex = Extras { form: Some(form), ..old(false) };
        assert_eq!(choose_style(&lot, LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::StoneManor);
        let ex = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lot, LandUse::Residential, 3, Tier::High, 5, &rules, &ex), plain);
        let ex = Extras { form: Some(form), look: Some(Look::BrickSteel), ..Extras::default() };
        assert_eq!(choose_style(&lot, LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::LoftEstate);
    }
}
