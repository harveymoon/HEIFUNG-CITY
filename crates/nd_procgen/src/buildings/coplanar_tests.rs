//! [render] Z-fighting guard: no two differently coloured faces of a
//! building may lie in the same plane facing the same way and overlap.
//! Two such faces have identical depths, so which one wins flickers with
//! the camera (window glass in its frame, a light on its panel). Every
//! decal must stand at its own depth: see `parts::window_light`.

use super::*;
use std::collections::HashMap;

type V3 = [f32; 3];

/// Overlaps smaller than this (square metres) are slivers along shared edges, or the
/// hidden ends of parts sunk into another (a trunk in its canopy).
const MIN_CLASH: f32 = 0.05;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Area of the overlap of two convex polygons in 2D (Sutherland-Hodgman).
fn overlap_area(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    let mut out: Vec<[f32; 2]> = a.to_vec();
    let orient = |p: &[[f32; 2]]| {
        let mut s = 0.0;
        for i in 0..p.len() {
            let (q, r) = (p[i], p[(i + 1) % p.len()]);
            s += q[0] * r[1] - r[0] * q[1];
        }
        s
    };
    let sb = orient(b).signum();
    for i in 0..b.len() {
        let (e0, e1) = (b[i], b[(i + 1) % b.len()]);
        let side = |p: [f32; 2]| sb * ((e1[0] - e0[0]) * (p[1] - e0[1]) - (e1[1] - e0[1]) * (p[0] - e0[0]));
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        if out.len() < 3 {
            return 0.0;
        }
    }
    orient(&out).abs() * 0.5
}

/// Pairs of triangles that would z-fight: coplanar (within 0.5 mm),
/// facing the same way, of a different surface or colour, overlapping by
/// more than a few square centimetres. Returns a count and an example.
pub(super) fn coplanar_clashes(g: &Geometry) -> (usize, Option<String>) {
    let all = clash_list(g);
    (all.len(), all.into_iter().next().map(|c| c.what))
}

/// Two faces that z-fight.
pub(super) struct Clash {
    pub surfaces: [Surface; 2],
    pub normal: V3,
    pub area: f32,
    pub what: String,
}

/// Every clash (see `coplanar_clashes`).
pub(super) fn clash_list(g: &Geometry) -> Vec<Clash> {
    struct Tri {
        n: V3,
        p: [V3; 3],
        key: (u8, [i32; 3]),
        s: Surface,
        what: String,
    }
    let mut tris: Vec<Tri> = Vec::new();
    for (s, m) in g.iter() {
        for t in m.indices.chunks_exact(3) {
            let p = [m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]];
            let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            let len = dot(c, c).sqrt();
            if len < 1e-4 {
                continue;
            }
            let n = [c[0] / len, c[1] / len, c[2] / len];
            let col = m.colors[t[0] as usize];
            let q = |v: f32| (v * 50.0).round() as i32;
            tris.push(Tri { n, p, key: (s as u8, [q(col[0]), q(col[1]), q(col[2])]), s, what: format!("{s:?} {:.2},{:.2},{:.2}", col[0], col[1], col[2]) });
        }
    }
    // Bucket by plane: the normal to a few degrees and the offset to 2 mm,
    // checking the neighbouring offset bucket too.
    let mut buckets: HashMap<([i32; 3], i64), Vec<usize>> = HashMap::new();
    let nkey = |n: V3| [(n[0] * 200.0).round() as i32, (n[1] * 200.0).round() as i32, (n[2] * 200.0).round() as i32];
    for (i, t) in tris.iter().enumerate() {
        let d = dot(t.n, t.p[0]);
        buckets.entry((nkey(t.n), (d / 0.002).floor() as i64)).or_default().push(i);
    }
    let mut found: Vec<Clash> = Vec::new();
    for ((nk, dk), list) in &buckets {
        for dk2 in [*dk, dk + 1] {
            let Some(other) = buckets.get(&(*nk, dk2)) else { continue };
            for &i in list {
                for &j in other {
                    if dk2 == *dk && j <= i {
                        continue;
                    }
                    let (a, b) = (&tris[i], &tris[j]);
                    if a.key == b.key || dot(a.n, b.n) < 0.999 {
                        continue;
                    }
                    if (dot(a.n, a.p[0]) - dot(a.n, b.p[0])).abs() > 0.0005 {
                        continue;
                    }
                    // Into the plane's 2D basis.
                    let n = a.n;
                    let u = if n[1].abs() < 0.9 { cross([0.0, 1.0, 0.0], n) } else { cross([1.0, 0.0, 0.0], n) };
                    let ul = dot(u, u).sqrt();
                    let u = [u[0] / ul, u[1] / ul, u[2] / ul];
                    let v = cross(n, u);
                    let flat = |t: &Tri| t.p.map(|p| [dot(p, u), dot(p, v)]);
                    let (fa, fb) = (flat(a), flat(b));
                    let bb = |f: &[[f32; 2]; 3]| (f.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]), f.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]));
                    let ((alo, ahi), (blo, bhi)) = (bb(&fa), bb(&fb));
                    if alo[0] >= bhi[0] || blo[0] >= ahi[0] || alo[1] >= bhi[1] || blo[1] >= ahi[1] {
                        continue;
                    }
                    let area = overlap_area(&fa, &fb);
                    // Faces turned to the ground are never seen from above.
                    if area > MIN_CLASH && a.n[1] > -0.5 {
                        let what = format!("{} vs {} at {:?} n {:?} ({area:.3} m2)", a.what, b.what, a.p[0], a.n.map(|x| (x * 100.0).round() / 100.0));
                        found.push(Clash { surfaces: [a.s, b.s], normal: a.n, area, what });
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.what.cmp(&b.what));
    found
}

#[test]
fn overlap_area_of_unit_squares() {
    let a = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let b = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5], [0.5, 1.5]];
    assert!((overlap_area(&a, &b) - 0.25).abs() < 1e-5);
    let c = [[2.0, 0.0], [3.0, 0.0], [3.0, 1.0]];
    assert_eq!(overlap_area(&a, &c), 0.0);
}

pub(super) fn test_lots(lu: LandUse) -> Vec<Lot> {
    use nd_sim::roads::RoadNetwork;
    use nd_sim::zoning::{generate_lots, Zone, ZoneId};
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
    let zone = Zone { polygon: vec![Vec2::new(-100.0, -20.0), Vec2::new(100.0, -20.0), Vec2::new(100.0, 120.0), Vec2::new(-100.0, 120.0)], land_use: lu };
    let mut lots = slotmap::SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let mut v: Vec<Lot> = lots.values().filter(|l| l.usable()).cloned().collect();
    v.sort_by(|a, b| nd_sim::geom::polygon_area(&b.polygon).abs().total_cmp(&nd_sim::geom::polygon_area(&a.polygon).abs()));
    v
}

/// Not a check: lists the coplanar clashes of every style
/// (`cargo test -p nd_procgen survey_coplanar -- --ignored --nocapture`;
/// `CLASH_STYLE=HillsideSteps:Residential:5:High` lists one building's).
/// The windows are clean; what remains is mostly faces hidden inside other
/// parts (stacked slabs' shared tops, a trunk's end in its canopy) and a few
/// boxes sharing a wall plane, left for a later pass.
#[test]
#[ignore]
fn survey_coplanar() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut rows: Vec<(usize, String)> = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment, LandUse::Civic] {
        let lots = test_lots(lu);
        for style in Style::ALL {
            for floors in [2u8, 5, 12] {
                for tier in [Tier::Low, Tier::Mid, Tier::High] {
                    let g = generate_styled_extras(&lots[0], lu, floors, tier, 0.9, 42, style, &rules, &pal, &Extras { dynamic_lights: true, ..Extras::default() });
                    let (n, ex) = coplanar_clashes(&g);
                    if n > 0 {
                        rows.push((n, format!("{lu:?} {style:?} {floors} {tier:?}: {n} e.g. {}", ex.unwrap_or_default())));
                    }
                }
            }
        }
    }
    if let Ok(f) = std::env::var("CLASH_STYLE") {
        let v: Vec<&str> = f.split(':').collect();
        let lu = LandUse::ALL.iter().copied().find(|l| format!("{l:?}") == v[1]).unwrap();
        let lots = test_lots(lu);
        let style = Style::ALL.iter().copied().find(|s| format!("{s:?}") == v[0]).unwrap();
        let tier = [Tier::Low, Tier::Mid, Tier::High].into_iter().find(|t| format!("{t:?}") == v[3]).unwrap();
        let g = generate_styled_extras(&lots[0], lu, v[2].parse().unwrap(), tier, 0.9, 42, style, &rules, &pal, &Extras { dynamic_lights: true, ..Extras::default() });
        for c in clash_list(&g) {
            println!("  {}", c.what);
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, r) in &rows {
        println!("{r}");
    }
    println!("{} combos clash", rows.len());
}

fn dynamic() -> Extras<'static> {
    Extras { dynamic_lights: true, ..Extras::default() }
}

/// The player's "Aurora Block" (stepped terraces, high tier, five floors)
/// flickered at its windows: the dark glass under each switchable light
/// lay in the plane of the window frame. No face of the style may share a
/// plane with a differently coloured one, at any size, tier or use.
#[test]
fn stepped_terraces_have_no_coplanar_faces() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    for lu in [LandUse::Residential, LandUse::Commercial] {
        let lots = test_lots(lu);
        for lot in lots.iter().take(3) {
            for floors in [2u8, 5, 9, 14] {
                for tier in [Tier::Low, Tier::Mid, Tier::High] {
                    for extras in [Extras::default(), dynamic()] {
                        let g = generate_styled_extras(lot, lu, floors, tier, 0.9, 42 + floors as u64, Style::HillsideSteps, &rules, &pal, &extras);
                        let clashes = clash_list(&g);
                        assert!(clashes.is_empty(), "{lu:?} {floors} floors {tier:?}: {} clashes, e.g. {}", clashes.len(), clashes[0].what);
                    }
                }
            }
        }
    }
}

/// No style's window pane or light lies in the plane of its frame (or of
/// another matte part on the wall): the z-fighting the stepped terraces
/// showed came from `windows`, which every style with punched windows
/// uses.
#[test]
fn no_window_shares_the_plane_of_its_frame() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut bad: Vec<String> = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        let lots = test_lots(lu);
        for style in Style::ALL {
            for floors in [3u8, 8] {
                for tier in [Tier::Low, Tier::High] {
                    let g = generate_styled_extras(&lots[0], lu, floors, tier, 0.9, 42, style, &rules, &pal, &dynamic());
                    let window = |s: Surface| matches!(s, Surface::Glass | Surface::Glow);
                    let frame = |s: Surface| matches!(s, Surface::Matte | Surface::Glass);
                    for c in clash_list(&g) {
                        let [a, b] = c.surfaces;
                        // A window-sized overlap on a wall.
                        if c.normal[1].abs() < 0.3 && c.area > 0.3 && ((window(a) && frame(b)) || (window(b) && frame(a))) {
                            bad.push(format!("{lu:?} {style:?} {floors} {tier:?}: {}", c.what));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} windows in the plane of their frame, e.g. {:?}", bad.len(), &bad);
}
