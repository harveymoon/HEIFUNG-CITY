//! [r4-patchwork] Patchwork's styles: drawn clean on every odd lot, at
//! every height, tier, condition and lighting mode; building and
//! demolition sites; no coplanar faces; bulbs, televisions, the fridge and
//! the fire barrel lit only after dark; within a triangle budget; drawn
//! only from the look's menus, the estates' forms build the compound, the
//! mechanic, the bike repairer and the tow yard their own styles; every
//! shack and room its own; stilt houses up on piles by the water with a
//! plank walk out over it, on the ground inland.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use crate::surface::{Lod, Surface};
use nd_sim::districts::Look;

fn patchwork(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::Patchwork), dynamic_lights, ..Extras::default() }
}

/// Every Patchwork style on every odd lot and a street lot, from one floor
/// to many, at every tier and condition, lights fixed and switchable, drawn
/// twice (deterministic) and checked as the main sweep checks every style.
#[test]
fn patchwork_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("big lot", test_lots(LandUse::Commercial)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::PATCHWORK.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 5, 7].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = patchwork(dynamic_lights);
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
    assert!(bad.is_empty(), "{} of {cases} patchwork cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Patchwork style at every stage.
#[test]
fn patchwork_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::PATCHWORK.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [2u8, 4, 7][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..patchwork(true) }, Extras { demolition: Some(p), ..patchwork(true) }] {
                    let g = generate_styled_extras(lot, LandUse::Residential, floors, Tier::Low, 0.6, 9 + li as u64, style, &rules, &pal, &ex);
                    for (kind, what) in faults(&g, lot, style) {
                        bad.push(format!("{style:?} on {name} site {:?} demo {:?}: {kind}: {what}", ex.construction, ex.demolition));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} faulty sites:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// No two differently coloured faces of a Patchwork building share a
/// plane (z-fighting), base and detail layers together: every sheet,
/// patch and room stands at its own depth.
#[test]
fn patchwork_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::PATCHWORK {
                for floors in [2u8, 4, 7] {
                    for tier in [Tier::Low, Tier::Mid, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.7, 42 + floors as u64, style, &rules, &pal, &patchwork(dynamic_lights));
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

/// Every Patchwork style lights up after dark and only then (bare bulbs,
/// a television's blue, the kiosk's fridge, a fire in a barrel, the
/// compound's floodlights: base glow marked `NIGHT_MARK`), keeps its
/// grain in the detail layers and stays within a triangle budget.
#[test]
fn patchwork_lights_after_dark_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let mut report = Vec::new();
    for style in Style::PATCHWORK {
        let mut night = 0;
        for seed in 0..4u64 {
            let g = generate_styled_extras(&lots[0], LandUse::Commercial, 4, Tier::Low, 0.7, 30 + seed, style, &rules, &pal, &patchwork(true));
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

/// The look's menus hold only Patchwork styles (industry too, not the
/// compound), a Patchwork building is drawn from them, no land use's own
/// menu holds one; the estates' forms build the compound in the look and
/// their own styles out of it; a mechanic and a bike shop there work
/// under a lean-to and a tow yard is a scrap yard, and out of it they keep
/// their usual ones.
#[test]
fn patchwork_menus_forms_and_trades() {
    use nd_sim::estate::Form;
    use nd_sim::BusinessKind as B;
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::Patchwork, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::PATCHWORK.contains(s) && *s != Style::BigManCompound && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 3, tier, seed, &rules, &patchwork(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::PATCHWORK.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    for (kind, lu, want, usual) in [(B::Mechanic, LandUse::Commercial, Style::LeanToWorkshop, Style::Mechanic), (B::BikeShop, LandUse::Commercial, Style::LeanToWorkshop, Style::BikeShop), (B::TowYard, LandUse::Industrial, Style::ScrapYard, Style::TruckLot)] {
        let lots = test_lots(lu);
        for seed in 0..4 {
            let ex = Extras { business: Some(kind), ..patchwork(false) };
            assert_eq!(choose_style(&lots[0], lu, 3, Tier::Low, seed, &rules, &ex), want, "{kind:?}");
            let out = Extras { business: Some(kind), ..Extras::default() };
            assert_eq!(choose_style(&lots[0], lu, 3, Tier::Low, seed, &rules, &out), usual, "{kind:?} out of the look");
        }
    }
    let lots = test_lots(LandUse::Residential);
    for form in [Form::McMansion, Form::Mansion] {
        let ex = Extras { form: Some(form), ..patchwork(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::BigManCompound);
        let plain = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &plain), Style::for_form(Some(form)).unwrap());
    }
}

/// Irregular: two seeds of the shacks on the same lot are not the same
/// shacks (rooms, sheets and roofs come from the seed), and a shack lot
/// is a crowd of small roofs at different heights, not one block.
#[test]
fn patchwork_shacks_are_each_their_own() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = test_lots(LandUse::Residential)[0].clone();
    let draw = |seed| generate_styled_extras(&lot, LandUse::Residential, 3, Tier::Low, 0.6, seed, Style::TinShacks, &rules, &pal, &patchwork(false));
    let (a, b) = (draw(5), draw(6));
    assert_ne!(a, b);
    // The tops of the walls and roofs: many different heights.
    let mut tops: Vec<i32> = a.iter().filter(|(s, _)| matches!(s, Surface::Rust | Surface::Metal | Surface::Panel | Surface::Wood)).flat_map(|(_, m)| m.positions.iter().map(|p| (p[1] * 20.0).round() as i32)).filter(|y| *y > 30).collect();
    tops.sort_unstable();
    tops.dedup();
    assert!(tops.len() > 20, "{} distinct heights", tops.len());
}

/// By the water the stilt houses stand up on piles, a plank walk runs out
/// over it on the seaward side and down into it, a canoe at its end;
/// inland they sit low on a plinth and nothing leaves the lot.
#[test]
fn stilt_houses_on_piles_by_the_water() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = test_lots(LandUse::Residential)[0].clone();
    let (frame, _) = Frame::lot_local(&lot);
    let shore = ShoreLook { seaward: frame.inward, drop: 1.6, beach: None };
    let beyond = |g: &Geometry| {
        let mut far = 0.0f32;
        for (_, m) in g.iter().chain(Lod::DETAIL.into_iter().flat_map(|l| g.detail_iter(l))) {
            for p in &m.positions {
                let q = Vec2::new(p[0], p[2]);
                if !nd_sim::zoning::point_in_polygon(q, &lot.polygon) && parts::distance_to_boundary(q, &lot.polygon) > 1.2 {
                    let d = frame.to_local(q);
                    let back = parts::bounds(&frame.local_poly(&lot)).1.y;
                    assert!(d.y > back - 1.5, "geometry off the lot away from the water at {d:?}");
                    far = far.max(d.y - back);
                }
            }
        }
        far
    };
    let wet = Extras { shore: Some(shore), ..patchwork(true) };
    for seed in [3u64, 8, 21] {
        let g = generate_styled_extras(&lot, LandUse::Residential, 3, Tier::Low, 0.6, seed, Style::StiltHouses, &rules, &pal, &wet);
        let out = beyond(&g);
        assert!(out > 4.0, "seed {seed}: a plank walk {out:.1} m out over the water");
        assert!(g.iter().any(|(_, m)| m.positions.iter().any(|p| p[1] < -1.6)), "seed {seed}: piles down into the water");
        let dry = generate_styled_extras(&lot, LandUse::Residential, 3, Tier::Low, 0.6, seed, Style::StiltHouses, &rules, &pal, &patchwork(true));
        assert!(beyond(&dry) < 1.3, "seed {seed}: inland, nothing off the lot");
        // The highest window head: the houses up on piles by the water
        // (the shacks round them are on the ground either way).
        let sill = |g: &Geometry| g.get(Surface::Glass).positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let (wet_sill, dry_sill) = (sill(&g), sill(&dry));
        assert!(wet_sill > dry_sill + 0.8, "seed {seed}: up on piles by the water ({wet_sill:.1} vs {dry_sill:.1})");
    }
}

/// The compound on a family's two plots is one house behind its wall; on
/// a billionaire's four it grows a third storey and a guesthouse: more of
/// it, and taller.
#[test]
fn big_man_compound_grows_on_an_estate() {
    use nd_sim::estate::Form;
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let small = sweep_tests::lot_of(&sweep_tests::rect(40.0, 36.0), 0.2, Vec2::new(40.0, 10.0));
    let big = sweep_tests::lot_of(&sweep_tests::rect(64.0, 52.0), 0.2, Vec2::new(40.0, 10.0));
    let draw = |lot: &Lot, form| generate_styled_extras(lot, LandUse::Residential, 2, Tier::High, 0.9, 11, Style::BigManCompound, &rules, &pal, &Extras { form: Some(form), ..patchwork(true) });
    let (a, b) = (draw(&small, Form::McMansion), draw(&big, Form::Mansion));
    let top = |g: &Geometry| g.get(Surface::Plaster).positions.iter().map(|p| p[1]).fold(0.0f32, f32::max);
    assert!(b.triangle_count() > a.triangle_count(), "{} vs {}", b.triangle_count(), a.triangle_count());
    assert!(top(&b) > top(&a) + 2.0, "{} vs {}", top(&b), top(&a));
}
