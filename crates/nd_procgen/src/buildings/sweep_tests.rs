//! Property sweep (code review, October 2026, action 13): every style on
//! odd synthetic lots, at every height from a shed to a megatall, at every
//! tier and condition, with the extras that change the look (dynamic
//! lights, district looks, brand HQs, plant scales, building sites), with
//! the rules at the limits `validate` lets through and with emptied
//! palette lists. Each mesh must come out without a panic, finite, well
//! indexed, with equal attribute lengths, without degenerate triangles,
//! with normals that agree with the winding, identical on a second run and
//! on its lot.

use super::*;
use nd_sim::zoning::polygon_centroid;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// A lot from an outline in its own frame (frontage along +x at y = 0,
/// inward +y, counter-clockwise), turned by `yaw` and moved to `at`, so
/// nothing in the generators can lean on world axes.
pub(crate) fn lot_of(local: &[Vec2], yaw: f32, at: Vec2) -> Lot {
    let (s, c) = yaw.sin_cos();
    let w = |p: Vec2| at + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
    let (lo, hi) = parts::bounds(local);
    let corners = [w(Vec2::new(lo.x, lo.y)), w(Vec2::new(hi.x, lo.y)), w(Vec2::new(hi.x, hi.y)), w(Vec2::new(lo.x, hi.y))];
    let polygon: Vec<Vec2> = local.iter().map(|p| w(*p)).collect();
    let center = polygon_centroid(&polygon);
    let along = Vec2::new(c, s);
    Lot {
        zone: Default::default(),
        land_use: LandUse::Commercial,
        polygon,
        corners,
        front_edge: [corners[0], corners[1]],
        center,
        front_dir: Vec2::new(along.y, -along.x),
        front_segment: nd_sim::SegmentId(0),
        front_t: 0.5,
        access_node: nd_sim::NodeId(0),
        building: None,
        landlocked: false,
        footfall_today: 0,
        footfall_hour: 0,
        car_segment: None,
        car_point: corners[0].lerp(corners[1], 0.5),
        park: None,
        shore: Default::default(),
    }
}

pub(crate) fn rect(w: f32, d: f32) -> Vec<Vec2> {
    vec![Vec2::new(-w * 0.5, 0.0), Vec2::new(w * 0.5, 0.0), Vec2::new(w * 0.5, d), Vec2::new(-w * 0.5, d)]
}

/// The odd parcels: narrow and deep, a small square, long and shallow
/// (the coal crash), short and very deep, a triangle, a sliver and an L.
pub(crate) fn odd_lots() -> Vec<(&'static str, Lot)> {
    let yaw = 0.37;
    let at = Vec2::new(310.0, -170.0);
    vec![
        ("6x30", lot_of(&rect(6.0, 30.0), yaw, at)),
        ("9x9", lot_of(&rect(9.0, 9.0), yaw, at)),
        ("150x24", lot_of(&rect(150.0, 24.0), yaw, at)),
        ("24x150", lot_of(&rect(24.0, 150.0), yaw, at)),
        ("triangle", lot_of(&[Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(2.0, 28.0)], yaw, at)),
        ("sliver", lot_of(&rect(40.0, 3.0), yaw, at)),
        ("L", lot_of(&[Vec2::new(-15.0, 0.0), Vec2::new(15.0, 0.0), Vec2::new(15.0, 12.0), Vec2::new(0.0, 12.0), Vec2::new(0.0, 30.0), Vec2::new(-15.0, 30.0)], yaw, at)),
    ]
}

/// Everything wrong with one mesh, as short kind keys with a detail.
fn faults(g: &Geometry, lot: &Lot, style: Style) -> Vec<(&'static str, String)> {
    let sliver = lot.depth().min(lot.width()) < 4.0;
    let mut out = Vec::new();
    // Track-side styles reach out to the stand-in track behind the lot.
    let link = style.rail_kind().map(|k| back_edge_link(lot, k));
    let off = |q: Vec2| {
        let mut d = if point_in_polygon(q, &lot.polygon) { 0.0 } else { parts::distance_to_boundary(q, &lot.polygon) };
        if let Some(l) = &link {
            let half = if l.kind == RailFacility::FreightYard { 19.0 } else { 10.0 };
            // The yard's tracks run the lot's whole back edge, past the
            // stand-in platform's ends.
            let back = -lot.front_dir * 4.0;
            let (_, p) = nd_sim::roads::project_on_segment(q, lot.corners[3] + back, lot.corners[2] + back);
            d = d.min((q.distance(p) - half).max(0.0));
        }
        d
    };
    for (s, m) in g.iter() {
        let n = m.positions.len();
        if m.normals.len() != n || m.colors.len() != n || m.uvs.len() != n || m.tangents.len() != n {
            out.push(("attribute lengths", format!("{s:?}: {n} positions, {} normals, {} colours, {} uvs, {} tangents", m.normals.len(), m.colors.len(), m.uvs.len(), m.tangents.len())));
            continue;
        }
        let finite = m.positions.iter().chain(&m.normals).all(|p| p.iter().all(|v| v.is_finite()))
            && m.colors.iter().chain(&m.tangents).all(|p| p.iter().all(|v| v.is_finite()))
            && m.uvs.iter().all(|p| p.iter().all(|v| v.is_finite()));
        if !finite {
            out.push(("not finite", format!("{s:?}")));
            continue;
        }
        if m.indices.len() % 3 != 0 || m.indices.iter().any(|&i| i as usize >= n) {
            out.push(("bad index", format!("{s:?}")));
            continue;
        }
        let (mut degenerate, mut flipped) = (0, 0);
        let mut flip_at = Vec3::ZERO;
        for t in m.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
            let cross = (b - a).cross(c - a);
            if cross.length() < 1e-6 {
                degenerate += 1;
                continue;
            }
            let nv: Vec3 = [t[0], t[1], t[2]].iter().map(|&i| Vec3::from(m.normals[i as usize])).sum();
            if nv.length() > 0.5 && cross.normalize().dot(nv.normalize()) < -0.2 {
                flipped += 1;
                flip_at = a;
            }
        }
        if degenerate > 0 {
            out.push(("degenerate triangles", format!("{s:?}: {degenerate}")));
        }
        if flipped > 0 {
            out.push(("normals against winding", format!("{s:?}: {flipped}, one at {flip_at}")));
        }
        let over = m.positions.iter().map(|p| off(Vec2::new(p[0], p[2]))).fold(0.0f32, f32::max);
        // Eaves, awnings and aviation lights may lean a little over the
        // lot line (the pavement is beyond it); a sliver is never built on
        // (`nd_sim::zoning`), it is only swept for panics and bad data.
        if over > 1.2 && !sliver {
            out.push(("off the lot", format!("{s:?}: {over:.1} m")));
        }
        if m.positions.iter().any(|p| p[1] < -0.3) {
            out.push(("below ground", format!("{s:?}")));
        }
    }
    out
}

#[derive(Default)]
struct Report {
    cases: usize,
    by_kind: BTreeMap<&'static str, Vec<String>>,
}

impl Report {
    fn add(&mut self, kind: &'static str, what: String) {
        self.by_kind.entry(kind).or_default().push(what);
    }
    fn merge(&mut self, o: Report) {
        self.cases += o.cases;
        for (k, v) in o.by_kind {
            self.by_kind.entry(k).or_default().extend(v);
        }
    }
    fn summary(&self) -> String {
        let mut s = format!("{} cases\n", self.cases);
        for (k, v) in &self.by_kind {
            s += &format!("  {k}: {} cases, e.g.\n", v.len());
            for e in v.iter().take(if std::env::var_os("SWEEP_ALL").is_some() { usize::MAX } else { 8 }) {
                s += &format!("    [{k}] {e}
");
            }
        }
        s
    }
}

fn panic_text(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// Draw one case twice (or once, `twice` false), checking everything.
#[allow(clippy::too_many_arguments)]
fn run(r: &mut Report, what: &str, lot: &Lot, lu: LandUse, floors: u8, tier: Tier, cond: f32, seed: u64, style: Style, rules: &BuildingRules, pal: &Palette, extras: &Extras, twice: bool) {
    r.cases += 1;
    let draw = || catch_unwind(AssertUnwindSafe(|| generate_styled_extras(lot, lu, floors, tier, cond, seed, style, rules, pal, extras)));
    let g = match draw() {
        Ok(g) => g,
        Err(e) => return r.add("panic", format!("{what}: {}", panic_text(e))),
    };
    for (kind, detail) in faults(&g, lot, style) {
        r.add(kind, format!("{what}: {detail}"));
    }
    if twice {
        match draw() {
            Ok(h) if h == g => {}
            Ok(_) => r.add("not deterministic", what.to_string()),
            Err(e) => r.add("panic", format!("{what} (second run): {}", panic_text(e))),
        }
    }
}

/// A league tower only goes up on a lot with room for its base; on a
/// smaller one the style draws its plain slab.
pub(crate) fn tower_on(lot: &Lot, floors: u8, k: usize) -> Option<nd_sim::towers::Tower> {
    (lot.width().min(lot.depth()) >= 20.0).then(|| tower_for(floors, k))
}

fn tower_for(floors: u8, k: usize) -> nd_sim::towers::Tower {
    nd_sim::towers::Tower { height: (floors as f32 * 4.0).clamp(120.0, 900.0), design: nd_sim::towers::TowerDesign::ALL[k % 4], seed: 77 + k as u64, started_day: 0, topped_out_day: Some(1), record_bid: false }
}

pub(crate) fn hq() -> nd_sim::corp::BrandMark {
    nd_sim::corp::BrandMark { colour: [0.9, 0.2, 0.6], shape: Default::default(), monogram: 'N', role: nd_sim::corp::BrandRole::Headquarters, colour_index: 1 }
}

/// Run `body` for every style on a few threads and merge the reports.
fn per_style(body: impl Fn(&mut Report, usize, Style) + Sync) -> Report {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
    let styles: Vec<(usize, Style)> = Style::ALL.into_iter().enumerate().collect();
    let mut total = Report::default();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let (styles, body) = (&styles, &body);
                s.spawn(move || {
                    let mut r = Report::default();
                    for &(i, style) in styles.iter().skip(t).step_by(threads) {
                        body(&mut r, i, style);
                    }
                    r
                })
            })
            .collect();
        for h in handles {
            total.merge(h.join().expect("sweep thread"));
        }
    });
    total
}

fn check(r: &Report, name: &str) {
    eprintln!("{name}: {}", r.summary());
    assert!(r.by_kind.is_empty(), "{name} sweep found faults:\n{}", r.summary());
}

/// Every style × lot × height, the tier, condition, lights, look and brand
/// cycled through so every combination of neighbours turns up; every case
/// drawn twice.
#[test]
fn sweep_every_style_on_odd_lots_at_every_height() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = odd_lots();
    let r = per_style(|r, si, style| {
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 6, 12, 40, 250].into_iter().enumerate() {
                let k = si * 31 + li * 7 + fi * 3;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = LandUse::ALL[k % LandUse::ALL.len()];
                let look = if k % 4 == 0 { None } else { Some(nd_sim::districts::Look::ALL[k % 8]) };
                let brand = (k % 5 == 0).then(hq);
                let tower = (style == Style::Supertall).then(|| tower_on(lot, floors, k)).flatten();
                let extras = Extras { dynamic_lights: k % 2 == 0, look, brand, tower, ..Extras::default() };
                let what = format!("{style:?} on {name} {floors}f {tier:?} cond {cond} {lu:?} look {look:?} hq {} dyn {}", brand.is_some(), extras.dynamic_lights);
                run(r, &what, lot, lu, floors, tier, cond, 1000 + k as u64, style, &rules, &pal, &extras, true);
            }
        }
    });
    check(&r, "styles");
}

/// Every style at every tier × condition × lights setting, on a plain lot
/// and the L, at a middling height.
#[test]
fn sweep_every_style_at_every_tier_and_condition() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = odd_lots();
    let r = per_style(|r, si, style| {
        for (name, lot) in [&lots[3], &lots[6]] {
            for tier in Tier::ALL {
                for cond in [0.0f32, 0.5, 1.0] {
                    for dynamic_lights in [false, true] {
                        let lu = LandUse::ALL[si % LandUse::ALL.len()];
                        let extras = Extras { dynamic_lights, tower: (style == Style::Supertall).then(|| tower_on(lot, 80, si)).flatten(), ..Extras::default() };
                        let what = format!("{style:?} on {name} {tier:?} cond {cond} dyn {dynamic_lights}");
                        run(r, &what, lot, lu, 8, tier, cond, 7 + si as u64, style, &rules, &pal, &extras, false);
                    }
                }
            }
        }
    });
    check(&r, "tiers");
}

/// The rules at the limits `validate` allows (floor height 2.2 and 12 m,
/// windows taller than a floor) and every palette list emptied.
#[test]
fn sweep_rules_at_their_limits_and_empty_palettes() {
    let pal = Palette::default();
    let lots = odd_lots();
    let mut empty = Palette::default();
    for list in [&mut empty.glow_colors, &mut empty.car_colors, &mut empty.sign_colors, &mut empty.container_colors, &mut empty.container_marks, &mut empty.tarp_colors, &mut empty.led_colors, &mut empty.concrete, &mut empty.glass, &mut empty.brick, &mut empty.plaster, &mut empty.panel, &mut empty.metal_paint, &mut empty.awning_colors] {
        list.clear();
    }
    let mut ruleset = Vec::new();
    for fh in [2.2f32, 12.0] {
        let mut rules = BuildingRules::default();
        for r in [&mut rules.residential, &mut rules.commercial, &mut rules.industrial, &mut rules.entertainment, &mut rules.civic, &mut rules.park] {
            r.floor_height = fh;
            r.window_size = [fh + 1.0, fh + 1.0];
            r.window_spacing = 0.8;
        }
        rules.validate();
        ruleset.push((format!("fh {fh}"), rules));
    }
    let r = per_style(|r, si, style| {
        for (li, (name, lot)) in lots.iter().enumerate() {
            let lu = LandUse::ALL[(si + li) % LandUse::ALL.len()];
            let tier = Tier::ALL[(si + li) % 3];
            for floors in [2u8, 12] {
                for (rn, rules) in &ruleset {
                    let extras = Extras { tower: (style == Style::Supertall).then(|| tower_on(lot, floors, si)).flatten(), ..Extras::default() };
                    run(r, &format!("{style:?} on {name} {floors}f {tier:?} {rn}"), lot, lu, floors, tier, 0.3, 3 + li as u64, style, rules, &pal, &extras, false);
                }
                let extras = Extras { tower: (style == Style::Supertall).then(|| tower_on(lot, floors, si)).flatten(), ..Extras::default() };
                run(r, &format!("{style:?} on {name} {floors}f {tier:?} empty palette"), lot, lu, floors, tier, 0.3, 3 + li as u64, style, &BuildingRules::default(), &empty, &extras, false);
            }
        }
    });
    check(&r, "limits");
}

/// Coal, nuclear and geothermal plants at the scales the sim gives them,
/// and past both ends, on every odd lot.
#[test]
fn sweep_plants_at_every_scale() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut r = Report::default();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for kind in [PowerPlantKind::Coal, PowerPlantKind::Nuclear, PowerPlantKind::Geothermal] {
            for scale in [0.3f32, 1.0, 2.4, 3.0] {
                let extras = Extras { plant: Some(kind), plant_scale: scale, ..Extras::default() };
                let style = choose_style(lot, LandUse::Industrial, 1, Tier::Mid, 9, &rules, &extras);
                run(&mut r, &format!("{kind:?} x{scale} on {name}"), lot, LandUse::Industrial, 1, Tier::Mid, 0.6, 9 + li as u64, style, &rules, &pal, &extras, true);
            }
            // The renderer's smoke reads the same layout.
            if kind == PowerPlantKind::Coal {
                for scale in [0.3f32, 1.0, 3.0] {
                    if let Err(e) = catch_unwind(|| coal_vents(lot, 9, scale)) {
                        r.add("panic", format!("coal_vents x{scale} on {name}: {}", panic_text(e)));
                    }
                }
            }
        }
    }
    check(&r, "plants");
}

/// Building and demolition sites at every stage on every odd lot.
#[test]
fn sweep_sites_at_every_stage() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut r = Report::default();
    for (li, (name, lot)) in odd_lots().iter().enumerate() {
        for (floors, tier) in [(2u8, Tier::Low), (12, Tier::Mid), (40, Tier::High)] {
            for k in 0..=10 {
                let p = k as f32 / 10.0;
                let lu = LandUse::ALL[(li + k) % LandUse::ALL.len()];
                let style = choose_style(lot, lu, floors, tier, 4, &rules, &Extras::default());
                for (site, extras) in [("site", Extras { construction: Some(p), ..Extras::default() }), ("demolition", Extras { demolition: Some(p), ..Extras::default() })] {
                    run(&mut r, &format!("{site} {p} {style:?} on {name} {floors}f"), lot, lu, floors, tier, 0.5, 4 + li as u64, style, &rules, &pal, &extras, k % 5 == 0);
                }
            }
        }
    }
    check(&r, "sites");
}
