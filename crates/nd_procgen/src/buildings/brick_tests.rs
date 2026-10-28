//! [r4-brick] The Brick & Steel styles (Foundry Row): drawn clean on every
//! odd lot and at every height, tier, condition and lighting mode; no
//! coplanar faces; within a triangle budget with their small parts in the
//! detail layers; lit by sodium lamps after dark; and only ever drawn from
//! the look's menus (or, for the loft estate, for a big house in the look).

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, lot_of, odd_lots, rect};
use super::*;
use nd_sim::districts::Look;

fn brick(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::BrickSteel), dynamic_lights, ..Extras::default() }
}

/// The odd lots, a street lot and the big lots the works and the estates
/// stand on (two and four plots).
fn lots() -> Vec<(&'static str, Lot)> {
    let mut v = odd_lots();
    v.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    v.push(("two plots", lot_of(&rect(44.0, 38.0), 0.21, Vec2::new(-80.0, 40.0))));
    v.push(("four plots", lot_of(&rect(75.0, 65.0), -0.6, Vec2::new(120.0, 260.0))));
    v
}

/// Every Brick & Steel style on every lot, from a shed to a megatall, at
/// every tier and condition, lights fixed and switchable, drawn twice
/// (deterministic) and checked as the main sweep checks every style.
#[test]
fn brick_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = lots();
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::BRICK_STEEL.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 6, 12, 30, 250].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = brick(dynamic_lights);
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
    assert!(bad.is_empty(), "{} of {cases} brick cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Brick & Steel style.
#[test]
fn brick_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in lots().iter().enumerate() {
        for (si, style) in Style::BRICK_STEEL.into_iter().enumerate() {
            for k in [0, 2, 4] {
                let p = k as f32 / 4.0;
                let floors = [2u8, 5, 9][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..brick(true) }, Extras { demolition: Some(p), ..brick(true) }] {
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

/// No two differently coloured faces of a Brick & Steel building share a
/// plane (z-fighting): windows over frames over brick, lintels, ghost
/// signs, gables, the north lights and their glow, yards and pools.
#[test]
fn brick_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    let big: Vec<Lot> = lots().into_iter().filter(|(n, _)| n.ends_with("plots")).map(|(_, l)| l).collect();
    for lu in [LandUse::Residential, LandUse::Industrial] {
        let mut ls: Vec<Lot> = test_lots(lu).into_iter().take(2).collect();
        ls.extend(big.iter().cloned());
        for lot in &ls {
            for style in Style::BRICK_STEEL {
                for floors in [1u8, 3, 8] {
                    for tier in [Tier::Low, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, style, &rules, &pal, &brick(dynamic_lights));
                            for c in clash_list(&g) {
                                bad.push(format!("{lu:?} {style:?} {floors}f {tier:?} dyn {dynamic_lights}: {}", c.what));
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
/// view pays less), lit by sodium lamps that only show after dark, and as
/// tall as the look lets them be (rows low, lofts to ten storeys).
#[test]
fn brick_budget_detail_and_night_lamps() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut report = Vec::new();
    for (name, lot) in lots().into_iter().filter(|(n, _)| *n == "street lot" || n.ends_with("plots")) {
        for style in Style::BRICK_STEEL {
            for floors in [2u8, 6, 12] {
                let g = generate_styled_extras(&lot, LandUse::Commercial, floors, Tier::High, 0.7, 11, style, &rules, &pal, &brick(true));
                let (tris, base) = (g.triangle_count(), g.base_triangle_count());
                report.push(format!("{style:?} on {name} {floors}f: {tris} tris, {base} base"));
                assert!(tris > 200, "{style:?} on {name}: only {tris} triangles");
                assert!(tris < 70_000, "{style:?} on {name} {floors}f: {tris} triangles");
                if name != "street lot" || !matches!(style, Style::LoftEstate) {
                    assert!(base * 10 < tris * 9, "{style:?} on {name} {floors}f: {base} of {tris} triangles in the base mesh");
                }
                let night = g.get(crate::surface::Surface::Glow).colors.iter().filter(|c| (c[3] - NIGHT_MARK).abs() < 0.01).count();
                assert!(night > 0, "{style:?} on {name} {floors}f: no sodium lamp or night glow");
            }
        }
    }
    eprintln!("{}", report.join("\n"));
    let lot = &test_lots(LandUse::Residential)[0];
    let fh = rules.residential.floor_height;
    let top = |g: &Geometry| g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0, f32::max);
    let row = generate_styled_extras(lot, LandUse::Residential, 12, Tier::Low, 1.0, 3, Style::BrickRow, &rules, &pal, &brick(false));
    assert!(top(&row) < 4.0 * 3.4 + 6.0, "a worker row stays low: {:.1} m", top(&row));
    let loft = generate_styled_extras(lot, LandUse::Residential, 8, Tier::Mid, 1.0, 3, Style::WarehouseLoft, &rules, &pal, &brick(false));
    assert!(top(&loft) >= 8.0 * fh.max(3.4) * 0.95, "an eight-storey loft is {:.1} m", top(&loft));
}

/// The look's menus hold only Brick & Steel styles (for homes, shops,
/// nights out and industry), a building of the look is drawn from them,
/// no land use's own menu holds one, and an industry with a look of its
/// own (a refinery) keeps it in Foundry Row.
#[test]
fn brick_menus() {
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::BrickSteel, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::BRICK_STEEL.contains(s) && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 4, tier, seed, &rules, &brick(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::BRICK_STEEL.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lot = &test_lots(LandUse::Industrial)[0];
    let ex = Extras { business: Some(nd_sim::BusinessKind::Refinery), ..brick(false) };
    let s = choose_style(lot, LandUse::Industrial, 2, Tier::Mid, 1, &rules, &ex);
    assert!(!Style::BRICK_STEEL.contains(&s), "a refinery keeps its own look: {s:?}");
    // A fab (Foundry Row's own trade) takes the look.
    let ex = Extras { business: Some(nd_sim::BusinessKind::Fab), ..brick(false) };
    assert!(Style::BRICK_STEEL.contains(&choose_style(lot, LandUse::Industrial, 2, Tier::Mid, 1, &rules, &ex)));
}

/// [r4-brick] A family's big house or a billionaire's estate in Foundry
/// Row is a loft estate; anywhere else the usual McMansion and mansion.
#[test]
fn a_foundry_row_mansion_is_a_loft_estate() {
    use nd_sim::estate::Form;
    let rules = BuildingRules::default();
    let lot = lots().into_iter().find(|(n, _)| *n == "four plots").unwrap().1;
    for (form, plain) in [(Form::McMansion, Style::McMansion), (Form::Mansion, Style::Mansion)] {
        let ex = Extras { form: Some(form), ..brick(false) };
        assert_eq!(choose_style(&lot, LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::LoftEstate);
        let ex = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lot, LandUse::Residential, 3, Tier::High, 5, &rules, &ex), plain);
        let ex = Extras { form: Some(form), look: Some(Look::Capsule), ..Extras::default() };
        assert_eq!(choose_style(&lot, LandUse::Residential, 3, Tier::High, 5, &rules, &ex), plain);
    }
}
