use super::*;
use nd_sim::roads::RoadNetwork;
use nd_sim::zoning::{generate_lots, Zone, ZoneId};
use slotmap::SlotMap;

fn lots(poly: Vec<Vec2>, lu: LandUse) -> Vec<Lot> {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
    let zone = Zone { polygon: poly, land_use: lu };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    lots.values().cloned().collect()
}

fn check_mesh(g: &Geometry, what: &str) {
    for (s, m) in g.iter() {
        assert_eq!(m.positions.len(), m.uvs.len(), "{what} {s:?} uv count");
        assert_eq!(m.positions.len(), m.tangents.len(), "{what} {s:?} tangent count");
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{what} {s:?} has NaN");
        assert!(m.normals.iter().all(|p| p.iter().all(|v| v.is_finite())), "{what} {s:?} has NaN normals");
        assert!(m.uvs.iter().all(|p| p.iter().all(|v| v.is_finite())), "{what} {s:?} has NaN uvs");
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{what} {s:?} bad index");
    }
}

#[test]
fn every_style_builds_on_odd_parcels_without_nans() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let zone = vec![Vec2::new(-100.0, -20.0), Vec2::new(100.0, -20.0), Vec2::new(100.0, 120.0), Vec2::new(-100.0, 120.0)];
    for lu in LandUse::ALL {
        let lots = lots(zone.clone(), lu);
        assert!(!lots.is_empty());
        let odd = lots.iter().find(|l| l.polygon.len() != 4).unwrap_or(&lots[0]);
        for style in Style::ALL {
            for (lot, floors) in [(&lots[0], 6u8), (odd, 2u8)] {
                for tier in [Tier::Low, Tier::Mid, Tier::High] {
                    let g = generate_styled(lot, lu, floors, tier, 0.7, 42, style, &rules, &pal);
                    assert!(g.triangle_count() > 0, "{style:?} produced nothing");
                    check_mesh(&g, &format!("{style:?}"));
                }
            }
        }
    }
}

/// Two touching solar parcels drawn as one field: the tracker grid runs
/// across the seam, no tracker is placed twice or too close, and the
/// pair holds at least as many as the two lone plants did; the
/// generator draws a pedestal under every tracker the renderer turns.
/// Two touching wind farms share one turbine grid that keeps its
/// spacing across the seam.
#[test]
fn touching_plants_are_drawn_as_one_field() {
    use nd_sim::geom::shared_boundary;
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let zone = vec![Vec2::new(-100.0, -20.0), Vec2::new(100.0, -20.0), Vec2::new(100.0, 120.0), Vec2::new(-100.0, 120.0)];
    let all = lots(zone, LandUse::Industrial);
    let (a, b) = all
        .iter()
        .enumerate()
        .find_map(|(i, a)| all.iter().skip(i + 1).find(|b| shared_boundary(&a.polygon, &b.polygon) >= 10.0 && a.area() > 600.0 && b.area() > 600.0).map(|b| (a, b)))
        .expect("two touching parcels");
    let pair = [a, b];
    let fields: Vec<PlantField> = (0..2).map(|i| PlantField::new(&pair, i, 7).unwrap()).collect();
    assert!(PlantField::new(&pair[..1], 0, 7).is_none(), "a field of one is a lone plant");
    let draw = |lot: &Lot, kind: PowerPlantKind, field: Option<&PlantField>| {
        let extras = Extras { plant: Some(kind), animated_rotors: true, field, ..Extras::default() };
        generate_building(lot, LandUse::Industrial, 1, Tier::Mid, 1.0, 11, &rules, &pal, &extras)
    };
    let lone: usize = pair.iter().map(|l| solar_trackers(l, None).len()).sum();
    let merged: Vec<Vec3> = (0..2).flat_map(|i| {
        let g = draw(pair[i], PowerPlantKind::Solar, Some(&fields[i]));
        check_mesh(&g, "solar field");
        // Pedestals only: the panels are the renderer's.
        assert!(g.get(Surface::Glow).positions.len() < 400, "animated panels and their rim lights are left out");
        solar_trackers(pair[i], Some(&fields[i])).into_iter().map(|t| Vec3::new(t.pos.x, 0.0, t.pos.y)).collect::<Vec<_>>()
    }).collect();
    assert!(merged.len() >= lone, "a field holds {} trackers, the two plants alone {lone}", merged.len());
    for (i, p) in merged.iter().enumerate() {
        assert!(merged[i + 1..].iter().all(|q| q.distance(*p) > power::SOLAR_PITCH * 0.95), "trackers at {p} too close or placed twice");
    }
    // Every tracker stands on one of the pair and its panel stays on it.
    for p in &merged {
        let q = Vec2::new(p.x, p.z);
        assert!(pair.iter().any(|l| nd_sim::zoning::point_in_polygon(q, &l.polygon)));
    }
    // Some panel row crosses the seam: panels on both sides within a
    // pitch of it, on the same row line.
    let seam_mid = {
        let n = a.polygon.len();
        (0..n).map(|k| (a.polygon[k], a.polygon[(k + 1) % n])).find(|(p, q)| fields[0].is_seam(*p, *q)).map(|(p, q)| (p + q) * 0.5).expect("a seam")
    };
    assert!(merged.iter().filter(|p| Vec2::new(p.x, p.z).distance(seam_mid) < 40.0).count() >= 4, "trackers run up to the seam");

    let sites: Vec<Turbine> = (0..2).flat_map(|i| wind_turbines(pair[i], 5, Some(&fields[i]))).collect();
    let lone_sites: usize = pair.iter().map(|l| wind_turbines(l, 5, None).len()).sum();
    assert!(sites.len() >= lone_sites, "field {} turbines, alone {lone_sites}", sites.len());
    for (i, t) in sites.iter().enumerate() {
        assert!(pair.iter().any(|l| nd_sim::zoning::point_in_polygon(t.pos, &l.polygon)));
        for u in &sites[i + 1..] {
            assert!(t.pos.distance(u.pos) >= power::TURBINE_SPACING * 0.95, "turbines {} apart", t.pos.distance(u.pos));
        }
    }
    // The generator draws a mast for every turbine the renderer spins.
    for i in 0..2 {
        let g = draw(pair[i], PowerPlantKind::Wind, Some(&fields[i]));
        check_mesh(&g, "wind field");
    }
}

/// A tall job's tower crane: its head goes to the renderer when asked
/// (and is missing from the site's mesh), paints vary from site to
/// site, some carry a lit billboard, and the slew is a pure function of
/// working time.
#[test]
fn tower_cranes_hand_their_heads_over_and_vary_by_site() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let zone = vec![Vec2::new(-100.0, -20.0), Vec2::new(100.0, -20.0), Vec2::new(100.0, 120.0), Vec2::new(-100.0, 120.0)];
    let all = lots(zone, LandUse::Commercial);
    let lot = all.iter().max_by(|a, b| a.area().total_cmp(&b.area())).unwrap();
    let mut paints = Vec::new();
    let (mut with_ad, mut without_ad) = (0, 0);
    for seed in 0..40u64 {
        let site = |animated_cranes| generate_building(lot, LandUse::Commercial, 12, Tier::High, 1.0, seed, &rules, &pal, &Extras { construction: Some(0.5), animated_cranes, ..Extras::default() });
        let whole = site(false);
        let mast_only = site(true);
        assert!(whole.cranes.is_empty());
        let rigs = &mast_only.cranes;
        assert_eq!(rigs.len(), 1, "one crane on a twelve-storey job");
        let rig = &rigs[0];
        check_mesh(&rig.head, "crane head");
        // A slewing crane carries a full counter-jib (a still one keeps
        // it inside the fence), so the head is at least what was there.
        assert!(mast_only.triangle_count() + rig.head.triangle_count() >= whole.triangle_count(), "the head is moved out, nothing lost");
        assert!(rig.head.iter().flat_map(|(_, m)| m.positions.iter()).all(|p| Vec2::new(p[0], p[2]).length() < 60.0), "head drawn about the mast: {:?}", rig.head.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| Vec2::new(p[0], p[2]).length()).fold(0.0f32, f32::max));
        assert!(nd_sim::zoning::point_in_polygon(rig.pos, &lot.polygon), "the mast stands on the site");
        let p = crane_paint(seed);
        if !paints.contains(&p) {
            paints.push(p);
        }
        // The billboard is the only glow besides the two aviation lights.
        if rig.head.get(Surface::Glow).positions.len() > 2 * 24 {
            with_ad += 1;
        } else {
            without_ad += 1;
        }
    }
    assert!(paints.len() >= 3, "crane paints {paints:?}");
    assert!(with_ad > 0 && without_ad > 0, "{with_ad} cranes with a billboard, {without_ad} without");
    // The slew: deterministic, bounded, one heading per lift.
    for seed in [1u64, 2, 99] {
        let mut headings = Vec::new();
        for m in 0..400u64 {
            let a = crane_slew(seed, m);
            assert_eq!(a, crane_slew(seed, m));
            assert!(a.abs() <= CRANE_SLEW_RANGE);
            assert_eq!(a, crane_slew(seed, m - m % CRANE_LIFT_MINUTES), "steady through a lift");
            if !headings.contains(&a) {
                headings.push(a);
            }
        }
        assert!(headings.len() > 20, "a new heading for most lifts");
    }
}

/// Lots from a zone cut by straight, diagonal and curved roads.
fn many_lots(lu: LandUse) -> Vec<Lot> {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
    let curve: Vec<Vec2> = (0..=20).map(|i| {
        let t = i as f32 / 20.0;
        Vec2::new(-300.0 + 600.0 * t, 80.0 + 60.0 * (t * std::f32::consts::TAU).sin())
    }).collect();
    roads.add_polyline(&curve, 1, 1);
    let zone = Zone { polygon: vec![Vec2::new(-300.0, -20.0), Vec2::new(300.0, -20.0), Vec2::new(300.0, 180.0), Vec2::new(-300.0, 180.0)], land_use: lu };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    lots.values().filter(|l| l.usable()).cloned().collect()
}

fn frame_poly(f: &Frame, lot: &Lot) -> Vec<Vec2> {
    lot.polygon.iter().map(|p| f.to_local(*p)).collect()
}

/// How far a world point lies outside the lot's outline (zero inside).
/// The outline, not its bounding box: a triangular corner lot has a lot
/// of box that is somebody else's land.
fn outside_by(q: Vec2, lot: &Lot) -> f32 {
    if point_in_polygon(q, &lot.polygon) { 0.0 } else { parts::distance_to_boundary(q, &lot.polygon) }
}

/// Distance from a point to the track corridor a station or yard was
/// given: the platform segment widened by `half`.
fn outside_corridor(q: Vec2, link: &RailLink, half: f32) -> f32 {
    let (_, p) = nd_sim::roads::project_on_segment(q, link.platform[0], link.platform[1]);
    (q.distance(p) - half).max(0.0)
}

/// Worst overshoot, lowest and highest vertex of a building, with a
/// description of the worst offender.
fn extent(g: &Geometry, lot: &Lot, link: Option<&RailLink>) -> (f32, f32, f32, String) {
    let frame = Frame::from_lot(lot);
    let (mut over, mut top, mut low) = (0.0f32, 0.0f32, 0.0f32);
    let mut culprit = String::new();
    for (surface, m) in g.iter() {
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]);
            let mut d = outside_by(q, lot);
            if let Some(l) = link {
                // Platforms, sidings, apron and the gantry reach out to
                // the track by design; they must stay in its corridor.
                let half = if l.kind == RailFacility::FreightYard { 19.0 } else { 10.0 };
                d = d.min(outside_corridor(q, l, half));
            }
            if d > over {
                over = d;
                let l = frame.to_local(q);
                culprit = format!("{surface:?} at local ({:.1}, {:.1}) y {:.1}", l.x, l.y, p[1]);
            }
            if p[1] < low {
                low = p[1];
                if over <= 0.0 {
                    let l = frame.to_local(q);
                    culprit = format!("{surface:?} at local ({:.1}, {:.1}) y {:.1}", l.x, l.y, p[1]);
                }
            }
            top = top.max(p[1]);
        }
    }
    (over, top, low, culprit)
}

/// Triangles a style may spend on a lot: a base, a share per floor for
/// facades, and a share per square metre for yards and parks.
fn budget(floors: u8, lot: &Lot) -> usize {
    4000 + 300 * floors as usize + 2 * polygon_area(&lot.polygon).abs() as usize
}

/// Building sites at every stage stay on their lot and are well formed.
#[test]
fn construction_sites_stay_on_their_lot() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut worst: Vec<String> = Vec::new();
    for lu in LandUse::ALL {
        let lots = many_lots(lu);
        for (li, lot) in lots.iter().enumerate() {
            for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                for progress in [0.0f32, 0.05, 0.3, 0.55, 0.8, 0.96, 1.0] {
                    let extras = Extras { construction: Some(progress), ..Extras::default() };
                    let g = generate_building(lot, lu, floors, tier, 1.0, seed + li as u64, &rules, &pal, &extras);
                    assert!(g.triangle_count() > 0, "site at {progress} on {lu:?} lot {li} drew nothing");
                    check_mesh(&g, &format!("site at {progress} on {lu:?} lot {li}"));
                    let (over, top, low, culprit) = extent(&g, lot, None);
                    // Scaffolding and hoarding are dense, and a site is temporary: it
                    // gets twice the finished building's budget.
                    let site_budget = budget(floors, lot) * 2;
                    if over > 1.0 || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -0.3 || g.triangle_count() > site_budget {
                        let frame = Frame::from_lot(lot);
                        let (plo, phi) = parts::bounds(&frame_poly(&frame, lot));
                        worst.push(format!("{lu:?} lot {li} floors {floors} at {progress}: overshoots {over:.1} m, top {top:.0} m, low {low:.1}, {} tris ({} verts, local {:.0}..{:.0} x {:.0}..{:.0}): {culprit}", g.triangle_count(), lot.polygon.len(), plo.x, phi.x, plo.y, phi.y));
                    }
                }
            }
        }
    }
    assert!(worst.is_empty(), "{} sites leave their lot or budget:\n{}", worst.len(), worst.join("\n"));
}

/// The site's small ground-level triangles (wholly under `y_max`, no
/// edge over 7 m: the yard's clutter, not the hoarding runs or the
/// sheeting down the scaffold) as millimetre-rounded vertices, sorted.
fn low_tris(g: &Geometry, y_max: f32) -> Vec<(String, [[i64; 3]; 3])> {
    let mut out = Vec::new();
    for (s, m) in g.iter() {
        for t in m.indices.chunks_exact(3) {
            let v = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
            let small = (0..3).all(|k| Vec3::from(v[k]).distance(Vec3::from(v[(k + 1) % 3])) < 7.0);
            if small && v.iter().all(|p| p[1] < y_max) {
                out.push((format!("{s:?}"), v.map(|p| p.map(|x| (x * 1000.0).round() as i64))));
            }
        }
    }
    out.sort();
    out
}

/// `small` is contained in `big`, counting repeats.
fn is_sub_multiset<T: Ord>(small: &[T], big: &[T]) -> bool {
    let mut j = 0;
    for x in small {
        while j < big.len() && big[j] < *x {
            j += 1;
        }
        if j == big.len() || big[j] != *x {
            return false;
        }
        j += 1;
    }
    true
}

/// A progress step keeps the yard where it was: the cabin, the truck,
/// the heaps, the stacks still there and the crew on the ground do not
/// jump when the structure gains a floor (the stacks only thin out).
#[test]
fn construction_clutter_is_stable_across_progress() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut checked = 0;
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial] {
        for (li, lot) in many_lots(lu).iter().enumerate().take(12) {
            for (floors, a, b) in [(9u8, 0.30f32, 0.36f32), (20, 0.2, 0.45), (3, 0.45, 0.6)] {
                let site = |p: f32| generate_building(lot, lu, floors, Tier::Mid, 1.0, 5 + li as u64, &rules, &pal, &Extras { construction: Some(p), ..Extras::default() });
                let (early, late) = (low_tris(&site(a), 1.6), low_tris(&site(b), 1.6));
                if !is_sub_multiset(&late, &early) {
                    let extra: Vec<_> = late.iter().filter(|t| !early.contains(t)).take(6).collect();
                    panic!("{lu:?} lot {li} floors {floors}: the yard at {b} is not the yard at {a} thinned out ({} vs {} ground triangles); new: {extra:?}", late.len(), early.len());
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 20);
}

/// Demolition sites at every stage stay on their lot, are well formed,
/// and come down: by the end nothing taller than the rubble is left.
#[test]
fn demolition_sites_stay_on_their_lot() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut worst: Vec<String> = Vec::new();
    for lu in LandUse::ALL {
        let lots = many_lots(lu);
        for (li, lot) in lots.iter().enumerate() {
            for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                for progress in [0.0f32, 0.05, 0.2, 0.45, 0.7, 0.85, 0.99, 1.0] {
                    let extras = Extras { demolition: Some(progress), ..Extras::default() };
                    let g = generate_building(lot, lu, floors, tier, 1.0, seed + li as u64, &rules, &pal, &extras);
                    let what = format!("demolition at {progress} on {lu:?} lot {li} floors {floors}");
                    assert!(g.triangle_count() > 0, "{what} drew nothing");
                    check_mesh(&g, &what);
                    let (over, top, low, culprit) = extent(&g, lot, None);
                    if over > 1.0 || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -0.3 || g.triangle_count() > budget(floors, lot) * 2 {
                        worst.push(format!("{what}: overshoots {over:.1} m, top {top:.0} m, low {low:.1}, {} tris: {culprit}", g.triangle_count()));
                    }
                    // Machines and cranes come and go, but once the
                    // structure is down only the site is left.
                    if progress >= 0.85 && top > 8.0 {
                        worst.push(format!("{what}: still {top:.1} m tall after the structure came down"));
                    }
                }
            }
        }
    }
    assert!(worst.is_empty(), "{} demolition sites misbehave:
{}", worst.len(), worst.join("
"));
}

/// A building cut part way down keeps no triangle above the cut, and
/// what is below is untouched.
#[test]
fn clipping_drops_everything_above_the_cut() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lot = &many_lots(LandUse::Commercial)[0];
    let mut g = generate_building(lot, LandUse::Commercial, 9, Tier::Mid, 1.0, 7, &rules, &pal, &Extras::default());
    let below = |g: &Geometry, y: f32| g.iter().flat_map(|(_, m)| m.indices.chunks_exact(3).filter(move |t| t.iter().all(|&i| m.positions[i as usize][1] <= y)).map(|_| ()).collect::<Vec<_>>()).count();
    let low_before = below(&g, 5.0);
    let points = construction::clip_above(&mut g, 12.0);
    assert!(!points.is_empty(), "walls were cut");
    let top = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
    assert!(top <= 12.0 + 1e-3, "nothing above the cut: {top}");
    assert_eq!(below(&g, 5.0), low_before, "the lower floors are untouched");
    check_mesh(&g, "clipped building");
}

/// Nothing a building generates may reach beyond its own lot outline
/// (a short way for canopies and kerbside clutter), dig into the
/// ground, or blow its triangle budget.
#[test]
fn geometry_stays_near_its_lot() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut worst: Vec<(f32, String)> = Vec::new();
    for lu in LandUse::ALL {
        let lots = many_lots(lu);
        assert!(lots.len() > 10, "{lu:?}: {} lots", lots.len());
        for (li, lot) in lots.iter().enumerate() {
            let span = { let (lo, hi) = parts::bounds(&lot.polygon); (hi - lo).max_element() };
            for style in Style::ALL {
                for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                    let g = generate_styled(lot, lu, floors, tier, 0.6, seed + li as u64, style, &rules, &pal);
                    let link = style.rail_kind().map(|k| back_edge_link(lot, k));
                    let (over, top, low, culprit) = extent(&g, lot, link.as_ref());
                    let tris = g.triangle_count();
                    let allow = 1.0;
                    if over > allow || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -0.3 || tris > budget(floors, lot) {
                        let frame = Frame::from_lot(lot);
                        let (plo, phi) = parts::bounds(&frame_poly(&frame, lot));
                        worst.push((over.max(-low).max((tris as f32 - budget(floors, lot) as f32) / 1000.0), format!("{style:?} {lu:?} lot {li} ({} verts, span {span:.0}, local {:.0}..{:.0} x {:.0}..{:.0}) overshoots {over:.1} m, top {top:.0} m, low {low:.1}, {tris} tris of {}, floors {floors}: {culprit}", lot.polygon.len(), plo.x, phi.x, plo.y, phi.y, budget(floors, lot))));
                    }
                }
            }
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    // One line per style: its worst case and how many lots it fails on.
    let mut seen: Vec<String> = Vec::new();
    for (_, w) in &worst {
        let style = w.split(' ').next().unwrap().to_string();
        if !seen.contains(&style) {
            let n = worst.iter().filter(|x| x.1.starts_with(&format!("{style} "))).count();
            eprintln!("{n:4} x {w}");
            seen.push(style);
        }
    }
    assert!(worst.is_empty(), "{} style/lot combinations reach beyond their lot or budget", worst.len());
}

/// A tower the sim gives a rooftop pad draws a helideck on its highest
/// roof, clear of everything on it, without leaving its lot or budget;
/// an unflagged tower draws none, and a flagged one always picks a
/// style that can carry a deck.
#[test]
fn rooftop_pads_sit_on_tower_roofs_and_stay_on_the_lot() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let (mut tried, mut drawn) = (0usize, 0usize);
    let mut misses: Vec<String> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for lu in [LandUse::Residential, LandUse::Commercial] {
        let lots = many_lots(lu);
        for (li, lot) in lots.iter().enumerate().take(36) {
            for style in Style::ALL.into_iter().filter(|s| s.pad_capable()) {
                let floors = 14u8;
                let seed = 3 + li as u64;
                let plain = generate_styled(lot, lu, floors, Tier::High, 1.0, seed, style, &rules, &pal);
                // Some towers carry a helipad of their own (corporate slabs).
                let built_in = plain.pads.len();
                let padded = generate_styled_extras(lot, lu, floors, Tier::High, 1.0, seed, style, &rules, &pal, &Extras { rooftop_pad: true, ..Extras::default() });
                check_mesh(&padded, "rooftop pad");
                tried += 1;
                let (over, top, low, culprit) = extent(&padded, lot, None);
                if over > 1.0 || low < -0.3 || top > (60.0 + floors as f32 * 5.0).max(90.0) || padded.triangle_count() > budget(floors, lot) {
                    bad.push(format!("{style:?} lot {li}: over {over:.1} top {top:.0} low {low:.1} tris {}: {culprit}", padded.triangle_count()));
                }
                assert!(padded.pads.len() <= built_in + 1, "{style:?}: one deck at most");
                let Some(p) = padded.pads.last().copied() else {
                    misses.push(format!("{style:?} lot {li}"));
                    continue;
                };
                drawn += 1;
                assert!(p.y > 8.0, "{style:?} lot {li}: the deck is up on a roof ({:.1} m)", p.y);
                let through = plain.iter().flat_map(|(_, m)| m.positions.iter()).filter(|q| Vec2::new(q[0], q[2]).distance(Vec2::new(p.x, p.z)) < 2.5 && q[1] > p.y + 0.3).count();
                assert_eq!(through, 0, "{style:?} lot {li}: the tower rises through its deck at {:.1} m", p.y);
            }
        }
    }
    assert!(bad.is_empty(), "{} rooftop pads leave their lot or budget:
{}", bad.len(), bad.join("
"));
    eprintln!("rooftop decks: {drawn} of {tried}; no room on {}", misses.len());
    assert!(drawn * 10 >= tried * 8, "{drawn} of {tried} towers drew a deck; missed {misses:?}");
    // The pick: a flagged tower always gets a deck-carrying style.
    let lots = many_lots(LandUse::Residential);
    for (li, lot) in lots.iter().enumerate().take(30) {
        for lu in [LandUse::Residential, LandUse::Commercial] {
            let s = choose_style(lot, lu, 14, Tier::High, 50 + li as u64, &rules, &Extras { rooftop_pad: true, ..Extras::default() });
            assert!(s.pad_capable(), "{lu:?} lot {li} picked {s:?}");
        }
    }
    // Vertiports draw a pad per slot where the lot has room, at the deck height.
    for tier in Tier::ALL {
        let lot = &lots[0];
        let g = generate_building(lot, LandUse::Civic, 1, tier, 1.0, 5, &rules, &pal, &Extras { service: Some(nd_sim::ServiceKind::Vertiport), ..Extras::default() });
        assert!(!g.pads.is_empty(), "{tier:?} vertiport has pads");
        assert!(g.pads.len() <= nd_sim::air::vertiport_slots(tier) as usize);
        let deck = nd_sim::air::vertiport_deck(tier);
        assert!(g.pads.iter().all(|p| (p.y - deck).abs() < 0.6), "{tier:?} pads at the deck height {deck}: {:?}", g.pads);
    }
}

/// The variety styles change form with height: a tall site draws a
/// taller building than a low one, and the glass castle ends in
/// crystals, not a needle (its top is broad).
#[test]
fn styles_change_form_with_height() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Residential);
    let lot = lots.iter().max_by(|a, b| polygon_area(&a.polygon).abs().total_cmp(&polygon_area(&b.polygon).abs())).unwrap();
    let top = |g: &Geometry| g.iter().filter(|(s, _)| *s != Surface::Glow && *s != Surface::Matte).flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
    for style in [Style::WalledCity, Style::PencilTower, Style::TwinSlab, Style::Megastructure, Style::GlassTerraces, Style::CapsuleHotel, Style::HillsideSteps, Style::NeonTower, Style::Fortress] {
        let low = generate_styled(lot, LandUse::Residential, 3, Tier::Mid, 0.8, 9, style, &rules, &pal);
        let high = generate_styled(lot, LandUse::Residential, 16, Tier::Mid, 0.8, 9, style, &rules, &pal);
        assert!(top(&high) > top(&low) * 1.6, "{style:?}: 16 floors ({:.0} m) should rise well above 3 ({:.0} m)", top(&high), top(&low));
    }
    // No spire: within the top 15% of the glass castle's height there is
    // still a crystal at least a few metres across.
    let g = generate_styled(lot, LandUse::Residential, 16, Tier::High, 1.0, 4, Style::Fortress, &rules, &pal);
    let h = top(&g);
    let crown: Vec<Vec2> = g.get(Surface::Glass).positions.iter().filter(|p| p[1] > h * 0.85).map(|p| Vec2::new(p[0], p[2])).collect();
    let (lo, hi) = parts::bounds(&crown);
    assert!((hi - lo).min_element() > 3.0, "the glass castle should not taper to a point: crown {:?}", hi - lo);
}

/// Cheap buildings use the weathered materials, smart ones do not.
#[test]
fn low_value_buildings_are_weathered() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Residential);
    let worn = |g: &Geometry| g.iter().filter(|(s, _)| s.is_worn()).count();
    let low = generate_styled(&lots[0], LandUse::Residential, 4, Tier::Low, 0.5, 3, Style::Tenement, &rules, &pal);
    let high = generate_styled(&lots[0], LandUse::Residential, 4, Tier::High, 1.0, 3, Style::Tenement, &rules, &pal);
    assert!(worn(&low) > 0 && low.get(Surface::Brick).is_empty(), "a low-tier tenement should be weathered brick");
    assert_eq!(worn(&high), 0, "a well-kept high-tier building stays clean");
}

#[test]
fn style_weights_respect_land_use_and_tier() {
    let rules = BuildingRules::default();
    let mut rng = rng_for(1, 2);
    for _ in 0..50 {
        let s = rules.pick_style(LandUse::Industrial, Tier::Low, &mut rng);
        assert!(matches!(s, Style::Junkyard | Style::RustyWarehouse | Style::Workshops));
        let s = rules.pick_style(LandUse::Industrial, Tier::High, &mut rng);
        assert!(matches!(s, Style::Datacenter | Style::Depot | Style::FabPlant));
    }
}

/// The same inputs draw the same building, byte for byte: the renderer
/// keys caches on (lot, seed) and a save must look the same on reload.
#[test]
fn generation_is_deterministic() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Commercial);
    for (li, lot) in lots.iter().take(6).enumerate() {
        for style in [Style::Shophouse, Style::Stacks, Style::Junkyard, Style::Plaza, Style::Crystal] {
            let a = generate_styled(lot, LandUse::Commercial, 5, Tier::Mid, 0.6, 99 + li as u64, style, &rules, &pal);
            let b = generate_styled(lot, LandUse::Commercial, 5, Tier::Mid, 0.6, 99 + li as u64, style, &rules, &pal);
            assert!(a == b, "{style:?} on lot {li} differs between two runs");
        }
        let extras = Extras { powered: 0.4, ..Extras::default() };
        let a = generate_building(lot, LandUse::Residential, 7, Tier::Low, 0.5, 3 + li as u64, &rules, &pal, &extras);
        let b = generate_building(lot, LandUse::Residential, 7, Tier::Low, 0.5, 3 + li as u64, &rules, &pal, &extras);
        assert!(a == b, "generate_building on lot {li} differs between two runs");
    }
}

/// A truck lot draws more lorries the fuller the sim says it is.
#[test]
fn a_truck_lot_shows_its_fleet() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Industrial);
    let lot = lots.iter().max_by(|a, b| a.area().total_cmp(&b.area())).unwrap();
    let tris = |fleet: f32| {
        let e = Extras { business: Some(nd_sim::BusinessKind::FleetLot), fleet, ..Extras::default() };
        generate_building(lot, LandUse::Industrial, 1, Tier::Low, 0.8, 9, &rules, &pal, &e).triangle_count()
    };
    assert!(tris(1.0) > tris(0.0) + 200, "{} vs {}", tris(1.0), tris(0.0));
}

#[test]
fn enum_tables_match_their_indices() {
    for (i, s) in Surface::ALL.iter().enumerate() {
        assert_eq!(s.index(), i, "{s:?}");
    }
    for (i, s) in Style::ALL.iter().enumerate() {
        assert_eq!(s.index(), i, "{s:?}");
    }
    assert_eq!(Geometry::default().parts.len(), Surface::ALL.len());
}

/// Rule files may hold chances above one; nothing downstream may panic.
#[test]
fn out_of_range_rules_are_clamped() {
    let mut rules = BuildingRules::default();
    rules.commercial.sign_chance = 7.0;
    rules.entertainment.vertical_sign_chance = f32::NAN;
    rules.tiers[0].lit_chance = 3.0;
    rules.tiers[1].window_colors.clear();
    rules.validate();
    assert_eq!(rules.commercial.sign_chance, 1.0);
    assert_eq!(rules.entertainment.vertical_sign_chance, 0.0);
    assert_eq!(rules.tiers[0].lit_chance, 1.0);
    assert!(!rules.tiers[1].window_colors.is_empty());
    // Even without validate() the generators clamp at the point of use.
    let mut raw = BuildingRules::default();
    raw.tiers[2].lit_chance = 4.0;
    raw.commercial.sign_chance = 2.0;
    let pal = Palette::default();
    let lots = many_lots(LandUse::Commercial);
    for style in [Style::Shophouse, Style::RetailBlock, Style::Tenement] {
        let g = generate_styled(&lots[0], LandUse::Commercial, 5, Tier::High, 0.3, 1, style, &raw, &pal);
        assert!(g.triangle_count() > 0);
    }
}

/// Extras change the look without breaking the lot: brown-outs, car
/// park occupancy, a bus depot, a planned multi-lot park and a real
/// railway link on either side of the track.
#[test]
fn extras_paths_stay_on_their_lot() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Commercial);
    let lot = &lots[0];
    // Brown-out: fewer lit windows than full power, same footprint.
    let lit = |g: &Geometry| g.get(Surface::Glow).indices.len();
    let bright = generate_building(lot, LandUse::Residential, 8, Tier::Mid, 0.9, 5, &rules, &pal, &Extras::default());
    let dim = generate_building(lot, LandUse::Residential, 8, Tier::Mid, 0.9, 5, &rules, &pal, &Extras { powered: 0.1, ..Extras::default() });
    assert!(lit(&dim) < lit(&bright), "a brown-out should put windows out ({} vs {})", lit(&dim), lit(&bright));
    for g in [&bright, &dim] {
        check_mesh(g, "powered");
        assert!(extent(g, lot, None).0 <= 1.0);
    }
    // Occupancy lighting: nearly every window becomes a marked light over
    // glass, ranked so the renderer can switch them by share.
    let dynamic = generate_building(lot, LandUse::Residential, 8, Tier::Mid, 0.9, 5, &rules, &pal, &Extras { dynamic_lights: true, ..Extras::default() });
    check_mesh(&dynamic, "dynamic lights");
    let marks: Vec<f32> = dynamic.get(Surface::Glow).colors.iter().map(|c| c[3]).filter(|a| *a >= WINDOW_LIGHT_MARK).collect();
    assert!(marks.len() >= 4 * lit(&bright) / 6, "most windows are switchable lights: {} marked vertices", marks.len());
    assert!(marks.iter().all(|a| *a < WINDOW_LIGHT_MARK + 1.0), "ranks stay in 0..1");
    let ranks: std::collections::HashSet<u32> = marks.iter().map(|a| ((a - WINDOW_LIGHT_MARK) * 1000.0) as u32).collect();
    assert!(ranks.len() > 10, "windows get spread-out ranks, got {}", ranks.len());
    assert!(dynamic.get(Surface::Glass).positions.len() > bright.get(Surface::Glass).positions.len(), "glass behind every switchable light");
    assert!(dynamic.get(Surface::Glow).colors.iter().any(|c| c[3] == 1.0) || dynamic.get(Surface::Glow).colors.iter().all(|c| c[3] >= WINDOW_LIGHT_MARK), "other glow untouched");
    // Parking: a full lot has more car than an empty one.
    let empty = generate_building(lot, LandUse::Commercial, 1, Tier::Low, 0.8, 5, &rules, &pal, &Extras { parking: Some(0.0), ..Extras::default() });
    let full = generate_building(lot, LandUse::Commercial, 1, Tier::Low, 0.8, 5, &rules, &pal, &Extras { parking: Some(1.0), ..Extras::default() });
    assert!(full.triangle_count() > empty.triangle_count() + 100, "cars should show up in a full car park");
    let deck = generate_building(lot, LandUse::Commercial, 4, Tier::Mid, 0.8, 5, &rules, &pal, &Extras { parking: Some(0.7), ..Extras::default() });
    for g in [&empty, &full, &deck] {
        check_mesh(g, "parking");
        let (over, _, low, culprit) = extent(g, lot, None);
        assert!(over <= 1.0 && low >= -0.3, "parking leaves its lot: {over:.1} m / {low:.1}: {culprit}");
    }
    // Services and attractions, every tier, drawn through their extras.
    for kind in nd_sim::services::ServiceKind::ALL {
        for tier in Tier::ALL {
            let g = generate_building(lot, LandUse::Civic, 1, tier, 0.9, 9, &rules, &pal, &Extras { service: Some(kind), ..Extras::default() });
            check_mesh(&g, "service");
            let (over, _, low, culprit) = extent(&g, lot, None);
            assert!(g.triangle_count() > 50 && over <= 1.0 && low >= -0.3, "{kind:?} {tier:?}: {over:.1} m / {low:.1}: {culprit}");
        }
    }
    // Depot.
    let depot = generate_building(lot, LandUse::Civic, 2, Tier::Mid, 0.8, 5, &rules, &pal, &Extras { depot: true, ..Extras::default() });
    check_mesh(&depot, "depot");
    assert!(extent(&depot, lot, None).0 <= 1.0);

    // A real park plan over several adjoining lots, drawn lot by lot.
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
    let zone = Zone { polygon: vec![Vec2::new(-200.0, -20.0), Vec2::new(200.0, -20.0), Vec2::new(200.0, 180.0), Vec2::new(-200.0, 180.0)], land_use: LandUse::Park };
    let mut park_lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut park_lots, &[]);
    let mut lv = nd_sim::land_value::LandValue::new(400.0, 50.0);
    for v in &mut lv.values {
        *v = 0.8;
    }
    nd_sim::park::plan_parks(&mut park_lots, &lv);
    let planned: Vec<&Lot> = park_lots.values().filter(|l| l.usable() && l.park.is_some()).collect();
    assert!(planned.iter().any(|l| l.park.as_ref().unwrap().cluster_lots >= 2), "the park zone should plan at least one multi-lot park");
    assert!(planned.iter().any(|l| !l.park.as_ref().unwrap().shared_edges.is_empty()), "adjoining park lots should share an edge");
    let mut drawn = 0;
    for lot in planned.iter().copied() {
        let layout = lot.park.as_ref().unwrap();
        let g = generate_building(lot, LandUse::Park, 1, layout.tier, 1.0, 11, &rules, &pal, &Extras { park: Some(layout), ..Extras::default() });
        check_mesh(&g, "park");
        let (_, _, low, _) = extent(&g, lot, None);
        // A park of several lots is one park: a tree by a seam may lean
        // over the next lot of it, but nothing may leave the park.
        let (mut over, mut culprit) = (0.0f32, String::new());
        for (surface, m) in g.iter() {
            for p in &m.positions {
                let q = Vec2::new(p[0], p[2]);
                let d = planned.iter().map(|l| outside_by(q, l)).fold(f32::MAX, f32::min);
                if d > over {
                    over = d;
                    culprit = format!("{surface:?} at ({:.1}, {:.1}) y {:.1}", q.x, q.y, p[1]);
                }
            }
        }
        assert!(over <= 1.0 && low >= -0.3, "park lot ({} lots, tier {:?}) leaves its park: {over:.1} m / {low:.1}: {culprit}", layout.cluster_lots, layout.tier);
        drawn += 1;
    }
    assert!(drawn > 3);

    // A real railway link along the back of the lot, on both sides of
    // the track, for the station and the yard.
    for kind in [RailFacility::Station, RailFacility::FreightYard] {
        for side in [1.0f32, -1.0] {
            for (li, lot) in lots.iter().take(12).enumerate() {
                let a = lot.corners[3];
                let b = lot.corners[2];
                let d = (b - a).normalize_or_zero();
                // `out` must point from the track towards the lot, so pick the
                // platform direction that makes perp(along) * side do that.
                let along = if Vec2::new(-d.y, d.x).dot(lot.front_dir) * side > 0.0 { d } else { -d };
                let centre = (a + b) * 0.5 - lot.front_dir * 4.0;
                let half = (a.distance(b) * 0.45).min(40.0);
                let link = RailLink { kind, segment: nd_sim::SegmentId(0), platform: [centre - along * half, centre + along * half], side, rail_connected: true, road_connected: true, walk_connected: true };
                let g = generate_building(lot, LandUse::Industrial, 2, Tier::Mid, 0.8, 5 + li as u64, &rules, &pal, &Extras { rail: Some(&link), ..Extras::default() });
                check_mesh(&g, "rail");
                let (over, _, low, culprit) = extent(&g, lot, Some(&link));
                assert!(over <= 1.0 && low >= -0.3, "{kind:?} side {side} on lot {li} leaves its lot and track: {over:.1} m / {low:.1}: {culprit}");
                // The platform furniture must land on the lot's side of the track.
                let out = Vec2::new(-along.y, along.x) * side;
                let platform_side = g.get(Surface::Concrete).positions.iter().filter(|p| (Vec2::new(p[0], p[2]) - centre).dot(out) > 0.5).count();
                let far_side = g.get(Surface::Concrete).positions.iter().filter(|p| (Vec2::new(p[0], p[2]) - centre).dot(out) < -8.0).count();
                assert!(platform_side > far_side, "{kind:?} side {side} on lot {li} built across the track");
            }
        }
    }
}

/// A bare context on a lot, for driving one part generator directly.
fn ctx_on<'a>(lot: &Lot, rules: &'a BuildingRules, pal: &'a Palette, tier: Tier, wear: f32) -> Ctx<'a> {
    let finish = Finish { rules: rules.for_land_use(LandUse::Residential), style: rules.style(tier), palette: pal };
    Ctx::new(lot, Style::Stacks, LandUse::Residential, 3, tier, 1.0 - wear, 1, finish, &Extras::default())
}

/// Triangles per shipping container and per Stacks building, printed
/// for the record (run with --nocapture). Every unit stays within its
/// budget, worn places spend more of it on rust and dents.
#[test]
fn container_triangle_counts() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Residential);
    let mut counts = Vec::new();
    let mut by_wear = Vec::new();
    for (i, (wear, tier)) in [(0.0f32, Tier::High), (0.3, Tier::Mid), (0.6, Tier::Low), (1.0, Tier::Low)].iter().enumerate() {
        let c = ctx_on(&lots[0], &rules, &pal, *tier, *wear);
        let mut rng = rng_for(5, i as u64);
        let mut sum = 0;
        for k in 0..60 {
            let mut g = Geometry::default();
            parts::container_unit(&mut g, &c, &mut rng, 0.0, 0.0, 0.0, if k % 3 == 0 { 12.1 } else { 6.05 }, 2.44, 2.6, (k % 3) as f32 * 2.6, pal.container(k));
            check_mesh(&g, "container");
            assert!(g.triangle_count() <= parts::CONTAINER_TRIS, "a container spent {} triangles", g.triangle_count());
            counts.push(g.triangle_count());
            sum += g.triangle_count();
        }
        by_wear.push(sum);
    }
    assert!(by_wear[3] > by_wear[0], "worn containers should carry more rust and dents: {by_wear:?}");
    let (lo, hi) = (*counts.iter().min().unwrap(), *counts.iter().max().unwrap());
    let avg = counts.iter().sum::<usize>() as f32 / counts.len() as f32;
    let stacks: Vec<usize> = lots.iter().take(40).enumerate().map(|(li, lot)| generate_styled(lot, LandUse::Residential, 3, Tier::Low, 0.5, 11 + li as u64, Style::Stacks, &rules, &pal).triangle_count()).collect();
    let savg = stacks.iter().sum::<usize>() as f32 / stacks.len() as f32;
    eprintln!("container tris: min {lo} avg {avg:.1} max {hi}; Stacks building avg {savg:.0} (max {})", stacks.iter().max().unwrap());
}

/// Units painted from the same palette entry come out different (paint
/// jitter, fading, livery, wear), and the same seed draws the same unit.
#[test]
fn containers_vary_per_unit_deterministically() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Residential);
    let c = ctx_on(&lots[0], &rules, &pal, Tier::Low, 0.5);
    let draw = |seed: u64| {
        let mut g = Geometry::default();
        let mut rng = rng_for(seed, 77);
        parts::container_unit(&mut g, &c, &mut rng, 0.0, 0.0, 0.0, 6.05, 2.44, 2.6, 0.0, pal.container(0));
        g
    };
    let mut paints: Vec<[u32; 3]> = Vec::new();
    let mut shapes: Vec<usize> = Vec::new();
    for seed in 0..40 {
        let g = draw(seed);
        assert!(g == draw(seed), "seed {seed} draws a different container twice");
        let sheet = g.get(Surface::Container);
        // The first sheet vertex carries the unit's body paint.
        let p = sheet.colors[0];
        let key = [(p[0] * 1000.0) as u32, (p[1] * 1000.0) as u32, (p[2] * 1000.0) as u32];
        if !paints.contains(&key) {
            paints.push(key);
        }
        if !shapes.contains(&g.triangle_count()) {
            shapes.push(g.triangle_count());
        }
        // Walls, roof and the decals painted on them all use the sheet.
        assert!(sheet.indices.len() / 3 >= 10, "the corrugated sheet covers walls, ends and roof");
    }
    assert!(paints.len() >= 35, "only {} distinct paints in 40 units", paints.len());
    assert!(shapes.len() >= 3, "livery and wear should vary the details: {shapes:?}");
}

/// A prop is an I-beam: two flanges and a web (two faces each, 12 rust
/// triangles), 0.2 m square in plan around its point, on a base plate
/// and a concrete pad on the ground, under a cap plate at the top.
#[test]
fn supports_are_ibeams_and_stay_put() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = many_lots(LandUse::Residential);
    let c = ctx_on(&lots[0], &rules, &pal, Tier::Low, 0.5);
    let mut rng = rng_for(3, 3);
    let p = Vec2::new(1.0, -2.0);
    for (base, on_ground) in [(0.0f32, true), (2.6, false)] {
        let mut g = Geometry::default();
        parts::ibeam_support(&mut g, &c, &mut rng, p, 0.4, base, base + 2.6, on_ground);
        check_mesh(&g, "prop");
        let steel = g.get(Surface::Rust);
        assert_eq!(steel.indices.len() / 3, 12, "two flanges and a web, two faces each");
        let w = c.frame.to_world(p.x, p.y);
        let half = parts::IBEAM_FLANGE * 0.5 * std::f32::consts::SQRT_2 + 1e-3;
        assert!(steel.positions.iter().all(|q| Vec2::new(q[0], q[2]).distance(w) <= half), "the beam stays within its section");
        // The web is thinner than the flanges: some rust vertices sit
        // close to the beam's axis plane, others out at the flange tips.
        let a = c.frame.yaw() + 0.4;
        let axis = Vec2::new(a.cos(), a.sin());
        let xs: Vec<f32> = steel.positions.iter().map(|q| (Vec2::new(q[0], q[2]) - w).dot(axis).abs()).collect();
        assert!(xs.iter().any(|x| *x < 0.02) && xs.iter().any(|x| *x > 0.09), "an I section: {xs:?}");
        assert_eq!(!g.get(Surface::Concrete).is_empty(), on_ground, "a pad only on the ground");
        let all = g.iter().flat_map(|(_, m)| m.positions.iter()).map(|q| q[1]);
        let (lo, hi) = all.fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        assert!(lo >= base - 1e-3 && hi <= base + 2.6 + 1e-3, "the prop fills the gap and no more: {lo}..{hi}");
        assert!(g.iter().flat_map(|(_, m)| m.positions.iter()).all(|q| Vec2::new(q[0], q[2]).distance(w) <= 0.45), "plates and pad stay round the prop");
    }
    // Stacks draw them, on their lots.
    let mut props = 0;
    for (li, lot) in lots.iter().take(30).enumerate() {
        let g = generate_styled(lot, LandUse::Residential, 4, Tier::Low, 0.4, 21 + li as u64, Style::Stacks, &rules, &pal);
        props += g.get(Surface::Rust).indices.len() / 3;
        for q in &g.get(Surface::Rust).positions {
            assert!(outside_by(Vec2::new(q[0], q[2]), lot) <= 1.0 && q[1] >= -0.01, "lot {li}: a prop outside its lot at {q:?}");
        }
    }
    assert!(props >= 20 * 5, "stacks should stand on I-beam props ({props} rust triangles)");
}
