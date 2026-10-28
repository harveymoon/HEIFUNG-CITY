//! [r4-harbour] Harbour Pastel's styles: drawn clean on every odd lot, at
//! every height, tier, condition and lighting mode; building and
//! demolition sites; no coplanar faces; bulbs and lamp rooms lit only after
//! dark; within a triangle budget; drawn only from the look's menus, the
//! estates' forms build the villa, the front's trades their own styles;
//! and on a lot that touches the water a small pier runs out over it,
//! except where the business builds its own jetty.

use super::coplanar_tests::{clash_list, test_lots};
use super::sweep_tests::{faults, odd_lots};
use super::*;
use crate::surface::{Lod, Surface};
use nd_sim::districts::Look;

fn harbour(dynamic_lights: bool) -> Extras<'static> {
    Extras { look: Some(Look::HarbourPastel), dynamic_lights, ..Extras::default() }
}

/// Every Harbour style on every odd lot and a street lot, from one floor
/// to many, at every tier and condition, lights fixed and switchable, drawn
/// twice (deterministic) and checked as the main sweep checks every style.
#[test]
fn harbour_styles_draw_clean_everywhere() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut lots = odd_lots();
    lots.push(("street lot", test_lots(LandUse::Residential)[0].clone()));
    lots.push(("big lot", test_lots(LandUse::Commercial)[0].clone()));
    let mut bad: Vec<String> = Vec::new();
    let mut cases = 0;
    for (si, style) in Style::HARBOUR.into_iter().enumerate() {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 3, 5, 9].into_iter().enumerate() {
                let k = si * 13 + li * 5 + fi;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial][k % 4];
                for dynamic_lights in [false, true] {
                    let ex = harbour(dynamic_lights);
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
    assert!(bad.is_empty(), "{} of {cases} harbour cases faulty:\n{}", bad.len(), bad[..bad.len().min(30)].join("\n"));
}

/// Building and demolition sites of every Harbour style at every stage.
#[test]
fn harbour_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (si, style) in Style::HARBOUR.into_iter().enumerate() {
            for k in 0..=4 {
                let p = k as f32 / 4.0;
                let floors = [2u8, 3, 5][(si + li + k) % 3];
                for ex in [Extras { construction: Some(p), ..harbour(true) }, Extras { demolition: Some(p), ..harbour(true) }] {
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

/// No two differently coloured faces of a Harbour building share a plane
/// (z-fighting), base and detail layers together.
#[test]
fn harbour_styles_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for style in Style::HARBOUR {
                for floors in [2u8, 3, 5] {
                    for tier in [Tier::Low, Tier::Mid, Tier::High] {
                        for dynamic_lights in [false, true] {
                            let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, style, &rules, &pal, &harbour(dynamic_lights));
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

/// Every Harbour style strings bulbs or lights a lamp that glows only
/// after dark (base glow marked `NIGHT_MARK`, so a lit front reads from
/// across the bay and nothing glows by day), keeps its grain in the detail
/// layers and stays within a triangle budget.
#[test]
fn harbour_lights_after_dark_within_budget() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = test_lots(LandUse::Commercial);
    let mut report = Vec::new();
    for style in Style::HARBOUR {
        let mut night = 0;
        for seed in 0..4u64 {
            let g = generate_styled_extras(&lots[0], LandUse::Commercial, 4, Tier::Mid, 0.9, 30 + seed, style, &rules, &pal, &harbour(true));
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

/// The look's menus hold only Harbour styles (industry too, not the villa),
/// a Harbour building is drawn from them, a farm keeps its own look in a
/// Harbour district, no land use's own menu holds one; the estates' forms
/// build the villa in the look and their own styles out of it; a hotel, a
/// seafood restaurant and a fish market there build the look's own, and
/// out of it their usual ones.
#[test]
fn harbour_menus_forms_and_trades() {
    use nd_sim::estate::Form;
    use nd_sim::BusinessKind as B;
    let rules = BuildingRules::default();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        for tier in Tier::ALL {
            let menu = district::look_styles(Look::HarbourPastel, lu, tier).expect("a menu");
            assert!(menu.iter().all(|(s, w)| Style::HARBOUR.contains(s) && *s != Style::PastelVilla && *w > 0.0), "{lu:?} {tier:?}");
            let lots = test_lots(lu);
            for seed in 0..12u64 {
                let s = choose_style(&lots[0], lu, 3, tier, seed, &rules, &harbour(false));
                assert!(menu.iter().any(|(m, _)| *m == s), "{lu:?} {tier:?}: {s:?}");
            }
        }
    }
    for w in &rules.styles {
        assert!(!Style::HARBOUR.contains(&w.style), "{:?} {:?} menu has {:?}", w.land_use, w.tier, w.style);
    }
    let lots = test_lots(LandUse::Industrial);
    let farm = Extras { business: Some(B::GrainFarm), ..harbour(false) };
    assert_eq!(choose_style(&lots[0], LandUse::Industrial, 2, Tier::Low, 3, &rules, &farm), Style::Farmstead);
    let lots = test_lots(LandUse::Residential);
    for form in [Form::McMansion, Form::Mansion] {
        let ex = Extras { form: Some(form), ..harbour(false) };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &ex), Style::PastelVilla);
        let plain = Extras { form: Some(form), ..Extras::default() };
        assert_eq!(choose_style(&lots[0], LandUse::Residential, 3, Tier::High, 5, &rules, &plain), Style::for_form(Some(form)).unwrap());
    }
    let hotel = Extras { form: Some(Form::MegaHotel), ..harbour(false) };
    assert_eq!(choose_style(&lots[0], LandUse::Commercial, 12, Tier::High, 5, &rules, &hotel), Style::MegaHotel);
    let lots = test_lots(LandUse::Commercial);
    for (kind, want, usual) in [(B::Hotel, Style::BoutiqueHotel, None), (B::SeafoodRestaurant, Style::SeafoodShack, Some(Style::SeafoodRestaurant)), (B::FishMarket, Style::FishMarketHall, Some(Style::FishMarket))] {
        for seed in 0..4 {
            let ex = Extras { business: Some(kind), ..harbour(false) };
            assert_eq!(choose_style(&lots[0], LandUse::Commercial, 4, Tier::Mid, seed, &rules, &ex), want, "{kind:?}");
            let out = Extras { business: Some(kind), ..Extras::default() };
            let s = choose_style(&lots[0], LandUse::Commercial, 4, Tier::Mid, seed, &rules, &out);
            assert!(!Style::HARBOUR.contains(&s), "{kind:?} out of the look: {s:?}");
            if let Some(u) = usual {
                assert_eq!(s, u);
            }
        }
    }
}

/// On a lot that touches the water (the shore behind it) the boathouses,
/// net sheds, the market hall and the villa run a small pier out over it:
/// geometry beyond the lot's back, on the seaward side only, down to the
/// water on piles. Inland, nothing leaves the lot; and a business with a
/// jetty of its own (a fish market) gets no pier of ours beside it.
#[test]
fn harbour_piers_run_out_over_the_water() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = test_lots(LandUse::Residential)[0].clone();
    let (frame, _) = Frame::lot_local(&lot);
    // Seaward: the lot's inward direction (the water behind it).
    let shore = ShoreLook { seaward: frame.inward, drop: 1.6, beach: None };
    let beyond = |g: &Geometry| {
        let mut far = 0.0f32;
        for (_, m) in g.iter().chain(Lod::DETAIL.into_iter().flat_map(|l| g.detail_iter(l))) {
            for p in &m.positions {
                let q = Vec2::new(p[0], p[2]);
                // Eaves and awnings may lean over the line a little.
                if !nd_sim::zoning::point_in_polygon(q, &lot.polygon) && parts::distance_to_boundary(q, &lot.polygon) > 1.2 {
                    let d = frame.to_local(q);
                    let back = parts::bounds(&frame.local_poly(&lot)).1.y;
                    // Only past the back edge, over the water.
                    assert!(d.y > back - 1.5, "geometry off the lot away from the water at {d:?}");
                    far = far.max(d.y - back);
                }
            }
        }
        far
    };
    for style in [Style::Boathouses, Style::NetSheds, Style::FishMarketHall, Style::PastelVilla, Style::PastelTerrace, Style::BoardwalkCafe] {
        let lu = if style == Style::FishMarketHall || style == Style::NetSheds { LandUse::Industrial } else { LandUse::Residential };
        let wet = Extras { shore: Some(shore), ..harbour(true) };
        let g = generate_styled_extras(&lot, lu, 2, Tier::Mid, 0.9, 77, style, &rules, &pal, &wet);
        let out = beyond(&g);
        assert!(out > 5.0, "{style:?}: a pier {out:.1} m out over the water");
        assert!(g.iter().any(|(_, m)| m.positions.iter().any(|p| p[1] < -1.6)), "{style:?}: piles down into the water");
        let dry = generate_styled_extras(&lot, lu, 2, Tier::Mid, 0.9, 77, style, &rules, &pal, &harbour(true));
        assert!(beyond(&dry) < 1.3, "{style:?}: inland, nothing off the lot");
    }
    let market = Extras { shore: Some(shore), business: Some(nd_sim::BusinessKind::FishMarket), ..harbour(true) };
    let g = generate_styled_extras(&lot, LandUse::Industrial, 2, Tier::Mid, 0.9, 77, Style::FishMarketHall, &rules, &pal, &market);
    assert!(beyond(&g) < 1.3, "the fish market's own jetty, not ours");
}
