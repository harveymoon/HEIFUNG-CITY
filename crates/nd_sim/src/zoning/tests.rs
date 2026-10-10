use super::*;

/// A piece too deep in the recursion to cut further is kept only when
/// it has no hole: its outer ring alone would cover the lake.
#[test]
fn a_piece_with_a_hole_is_never_a_parcel_whole() {
    use rand::SeedableRng;
    let ring = |c: f64, r: f64| LineString::from(vec![(c - r, c - r), (c + r, c - r), (c + r, c + r), (c - r, c + r), (c - r, c - r)]);
    let holed = Polygon::new(ring(0.0, 60.0), vec![ring(0.0, 10.0)]);
    let solid = Polygon::new(ring(0.0, 60.0), vec![]);
    let roads = RoadNetwork::default();
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1);
    let mut out = Vec::new();
    subdivide(holed, LandUse::Residential, &roads, &mut rng, 61, &mut out);
    assert!(out.is_empty(), "the holed piece was kept whole");
    subdivide(solid, LandUse::Residential, &roads, &mut rng, 61, &mut out);
    assert_eq!(out.len(), 1);
}

/// [tools] The outline Harvey's geothermal plant's parcels joined into
/// on a recut: a slit out to a tip and straight back where two shared
/// edges did not quite match. It read as a sliver, the recut lot was
/// landlocked, and the plant was lost to a road 100 m away.
#[test]
fn a_joined_outline_has_no_slit() {
    let v = |x: f32, y: f32| Vec2::new(x, y);
    let slit = vec![v(-516.75287, -221.87045), v(-507.64688, -247.44885), v(-467.52332, -231.34691), v(-470.71185, -222.39044), v(-503.92023, -234.21283), v(-470.71185, -222.39044), v(-474.94632, -210.49606)];
    let compact = |p: &[Vec2]| polygon_area(p).abs() / perimeter(p).powi(2);
    assert!(compact(&slit) < MIN_COMPACTNESS, "the slit reads as a sliver");
    let clean = simplify_collinear(&slit);
    assert_eq!(clean.len(), 4, "{clean:?}");
    assert!(compact(&clean) > MIN_COMPACTNESS && reflex_vertex(&clean, 0.6).is_none(), "{clean:?}");
    assert!((polygon_area(&clean).abs() - polygon_area(&slit).abs()).abs() < 1.0, "no ground lost");
}

fn roads_cross() -> RoadNetwork {
    let mut r = RoadNetwork::default();
    r.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    r.add_polyline(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    r
}

fn usable(lots: &SlotMap<LotId, Lot>) -> Vec<&Lot> {
    lots.values().filter(|l| l.usable()).collect()
}

#[test]
fn zone_straddling_roads_splits_into_blocks_and_parcels() {
    let roads = roads_cross();
    let zone = Zone {
        polygon: vec![Vec2::new(-100.0, -80.0), Vec2::new(100.0, -80.0), Vec2::new(100.0, 80.0), Vec2::new(-100.0, 80.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let good = usable(&lots);
    assert!(good.len() >= 8, "got {} usable parcels", good.len());
    for lot in lots.values() {
        assert!(lot.center.x.abs() > 5.0 && lot.center.y.abs() > 5.0, "parcel on road at {:?}", lot.center);
        assert!(polygon_area(&lot.polygon) > 0.0, "parcels are counter-clockwise");
        assert!(reflex_vertex(&lot.polygon, 0.6).is_none(), "parcels have no sharp inward corners");
    }
    for lot in &good {
        let mid = lot.entrance();
        let (_, _, d) = nearest_road(&roads, mid, false).unwrap();
        assert!((d - 5.75 - SIDEWALK).abs() < 2.6, "usable parcels front a road, got {d}");
    }
    let total: f32 = lots.values().map(|l| l.area()).sum();
    assert!(total > 200.0 * 160.0 * 0.6, "coverage {total}");
}

#[test]
fn diagonal_road_makes_non_rectangular_parcels() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0)], 1, 1);
    let zone = Zone {
        polygon: vec![Vec2::new(-100.0, -100.0), Vec2::new(100.0, -100.0), Vec2::new(100.0, 100.0), Vec2::new(-100.0, 100.0)],
        land_use: LandUse::Commercial,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    dump_lots(&roads, &lots, "ND_DUMP2");
    assert!(!usable(&lots).is_empty());
    let non_rect = lots.values().filter(|l| l.polygon.len() != 4).count();
    assert!(non_rect > 0, "expected triangles or pentagons where the diagonal road meets the zone edge");
}

#[test]
fn covering_zone_takes_precedence() {
    let roads = roads_cross();
    let zone = Zone {
        polygon: vec![Vec2::new(10.0, 10.0), Vec2::new(150.0, 10.0), Vec2::new(150.0, 150.0), Vec2::new(10.0, 150.0)],
        land_use: LandUse::Residential,
    };
    let cover = Zone { polygon: zone.polygon.clone(), land_use: LandUse::Park };
    let mut lots = SlotMap::with_key();
    let ids = generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[cover]);
    assert!(ids.is_empty());
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]
}

#[test]
fn partially_overlapping_zones_do_not_share_ground() {
    let roads = roads_cross();
    // An older residential zone in the corner of the cross, and a newer
    // commercial one drawn over its eastern part; both front the road
    // along y = 0.
    let older = Zone { polygon: rect(10.0, 10.0, 150.0, 150.0), land_use: LandUse::Residential };
    let newer = Zone { polygon: rect(90.0, 10.0, 230.0, 170.0), land_use: LandUse::Commercial };
    let mut lots = SlotMap::with_key();
    let new_ids = generate_lots(ZoneId::default(), &newer, &roads, &mut lots, &[]);
    let old_ids = generate_lots(ZoneId::default(), &older, &roads, &mut lots, &[newer.clone()]);
    assert!(new_ids.iter().any(|&id| lots[id].usable()) && old_ids.iter().any(|&id| lots[id].usable()));
    // The older zone's parcels stop at the newer zone's edge instead of
    // running under it.
    for &id in &old_ids {
        for p in &lots[id].polygon {
            assert!(p.x <= 90.0 + 0.05, "old-zone parcel vertex {p:?} lies inside the newer zone");
        }
    }
    // No two usable parcels overlap by more than a sliver.
    let good = usable(&lots);
    for i in 0..good.len() {
        for j in (i + 1)..good.len() {
            let inter = to_geo(&good[i].polygon).intersection(&to_geo(&good[j].polygon)).unsigned_area() as f32;
            let smaller = good[i].area().min(good[j].area());
            assert!(inter < 1.0 && inter < smaller * 0.01, "parcels overlap by {inter:.1} m2 of {smaller:.0}: {:?} / {:?}", good[i].center, good[j].center);
        }
    }
    // Both zones still get their row along each frontage: about 280 m
    // of frontage on the older zone and 140 m on the newer, a lot deep.
    let total: f32 = good.iter().map(|l| l.area()).sum();
    assert!(total > 420.0 * 24.0 * 0.6, "usable {total}");
}

#[test]
fn a_self_crossing_zone_yields_nothing_instead_of_garbage() {
    let roads = roads_cross();
    let bow = Zone { polygon: vec![Vec2::new(10.0, 10.0), Vec2::new(150.0, 150.0), Vec2::new(150.0, 10.0), Vec2::new(10.0, 150.0)], land_use: LandUse::Residential };
    let mut lots = SlotMap::with_key();
    assert!(generate_lots(ZoneId::default(), &bow, &roads, &mut lots, &[]).is_empty());
    // As a covering zone it is ignored for the ground subtraction and
    // only its centre test applies: the zone under it still parcels.
    let zone = Zone { polygon: rect(10.0, 10.0, 150.0, 150.0), land_use: LandUse::Commercial };
    let ids = generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[bow]);
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|&id| polygon_area(&lots[id].polygon) > 0.0 && lots[id].polygon.iter().all(|p| p.is_finite())));
}

#[test]
fn curved_road_cuts_only_its_own_strip_and_leaves_no_slivers() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let curve = crate::plan::sample_curve(&[Vec2::new(-120.0, 0.0), Vec2::new(-20.0, 120.0), Vec2::new(120.0, 0.0)]);
    roads.add_polyline(&curve, 1, 1);
    let zone = Zone {
        polygon: vec![Vec2::new(-200.0, 10.0), Vec2::new(200.0, 10.0), Vec2::new(200.0, 200.0), Vec2::new(-200.0, 200.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let good = usable(&lots);
    assert!(good.len() >= 12, "got {} usable parcels", good.len());
    for lot in &good {
        let compact = lot.area() / perimeter(&lot.polygon).powi(2);
        assert!(compact >= MIN_COMPACTNESS, "sliver parcel with compactness {compact}");
        assert!(lot.front_edge[0].distance(lot.front_edge[1]) >= 5.0);
    }
    // Parcels well inside the arch, away from any road, are landlocked.
    assert!(lots.values().any(|l| l.landlocked), "deep land without frontage is marked unusable");
}

#[test]
fn long_frontages_parcel_all_the_way_along() {
    // A single-sided frontage of 400 m and 1200 m used to parcel into
    // nothing: no cut passed the compactness test of its remainder.
    for &len in &[400.0f32, 1200.0] {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-len, 0.0), Vec2::new(len, 0.0)], 1, 1);
        for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment, LandUse::Civic, LandUse::Park] {
            let half = len * 0.5;
            let zone = Zone {
                polygon: vec![Vec2::new(-half, 10.0), Vec2::new(half, 10.0), Vec2::new(half, 70.0), Vec2::new(-half, 70.0)],
                land_use: lu,
            };
            let mut lots = SlotMap::with_key();
            generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
            let n = usable(&lots).len();
            let expect = (len / lu.lot_width() * 0.5) as usize;
            assert!(n >= expect, "{lu:?} on {len} m: {n} usable lots, expected at least {expect}");
            let spread = usable(&lots).iter().map(|l| l.center.x).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            assert!(spread.1 - spread.0 > len * 0.8, "{lu:?} on {len} m: lots cover only {:.0}..{:.0}", spread.0, spread.1);
        }
    }
}

#[test]
fn land_without_road_access_is_unusable() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    // A deep zone with a road only on its south side.
    let zone = Zone {
        polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 130.0), Vec2::new(-80.0, 130.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let front = usable(&lots);
    assert!(!front.is_empty());
    assert!(front.iter().all(|l| l.center.y < 60.0), "only the first row fronts the road");
    assert!(lots.values().any(|l| l.landlocked && l.center.y > 60.0), "back land is landlocked");
}

#[test]
fn homes_line_paths_but_shops_do_not() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    roads.add_polyline_kind(&[Vec2::new(-300.0, 80.0), Vec2::new(300.0, 80.0)], 0, 0, crate::roads::RoadKind::Path);
    // Homes: lots front the path too, without car access, and know the nearest street.
    let homes = Zone {
        polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 150.0), Vec2::new(-80.0, 150.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &homes, &roads, &mut lots, &[]);
    let on_path: Vec<&Lot> = lots.values().filter(|l| l.usable() && !l.car_access()).collect();
    assert!(!on_path.is_empty(), "homes line the path");
    assert!(on_path.iter().all(|l| l.car_segment.is_some() && (l.car_point.y).abs() < 1.0), "their car point is on the street");
    assert!(lots.values().filter(|l| l.usable()).any(|l| l.center.y > 90.0), "land beyond the path is used");
    // Shops: the path is not frontage.
    let zone = Zone {
        polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 150.0), Vec2::new(-80.0, 150.0)],
        land_use: LandUse::Commercial,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    assert!(lots.values().all(|l| (l.center.y - 80.0).abs() > 2.0), "no lot sits on the path");
    assert!(lots.values().filter(|l| l.usable()).all(|l| l.center.y < 60.0), "land beyond the path is not usable from it");
    assert!(lots.values().any(|l| l.center.y > 90.0 && l.landlocked));
}

#[test]
fn every_road_side_of_a_block_gets_parcels_including_corners() {
    let mut roads = RoadNetwork::default();
    for c in [-70.0f32, 70.0] {
        roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
    }
    let zone = Zone {
        polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let good: Vec<&Lot> = lots.values().filter(|l| l.usable()).collect();
    // Parcels front all four roads.
    for (axis, sign) in [(0, -1.0f32), (0, 1.0), (1, -1.0), (1, 1.0)] {
        let n = good.iter().filter(|l| {
            let e = l.entrance();
            let v = if axis == 0 { e.x } else { e.y };
            (v * sign) > 50.0
        }).count();
        assert!(n >= 3, "side {axis}/{sign} has only {n} usable parcels");
    }
    // Corners are claimed by a lot whose polygon reaches both roads.
    for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = Vec2::new(sx * 58.0, sy * 58.0);
        assert!(good.iter().any(|l| l.contains(corner)), "corner {corner:?} is not in a usable lot");
    }
    // Usable land covers most of the block.
    let usable_area: f32 = good.iter().map(|l| l.area()).sum();
    assert!(usable_area > 122.0 * 122.0 * 0.5, "usable {usable_area}");
}

#[test]
fn s_curve_through_a_block_keeps_most_land_usable() {
    let mut roads = RoadNetwork::default();
    for c in [-70.0f32, 70.0] {
        roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
    }
    let curve = crate::plan::sample_curve(&[Vec2::new(-20.0, -70.0), Vec2::new(-35.0, -20.0), Vec2::new(25.0, 20.0), Vec2::new(10.0, 70.0)]);
    roads.add_polyline(&curve, 1, 1);
    let zone = Zone {
        polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let good: Vec<&Lot> = lots.values().filter(|l| l.usable()).collect();
    let usable_area: f32 = good.iter().map(|l| l.area()).sum();
    let curve_len: f32 = curve.windows(2).map(|w| w[0].distance(w[1])).sum();
    let block_area = 122.0 * 122.0 - 140.0 * 2.0 * 8.95 - curve_len * 2.0 * 8.95;
    let dead: Vec<f32> = lots.values().filter(|l| l.landlocked).map(|l| l.area()).collect();
    assert!(good.len() >= 28, "only {} usable parcels around the curve; usable {usable_area:.0} of {block_area:.0}; dead {:?}", good.len(), dead);
    assert!(usable_area > block_area * 0.6, "usable {usable_area} of {block_area}");
    // Lots along the curve are not too deep or too shallow.
    for l in &good {
        let (w, d) = (l.width(), l.depth());
        assert!(d > 7.0 && d < 24.0 * 3.2, "odd lot depth {d}");
        assert!(w < 14.0 * 3.6, "lot too wide: {w} (a wedge spanning the block)");
    }
}

#[test]
fn dump_s_curve_parcels_for_inspection() {
    let mut roads = RoadNetwork::default();
    for c in [-70.0f32, 70.0] {
        roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
    }
    let curve = crate::plan::sample_curve(&[Vec2::new(-20.0, -70.0), Vec2::new(-35.0, -20.0), Vec2::new(25.0, 20.0), Vec2::new(10.0, 70.0)]);
    roads.add_polyline(&curve, 1, 1);
    let zone = Zone {
        polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
        land_use: LandUse::Residential,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    dump_lots(&roads, &lots, "ND_DUMP");
}

fn dump_lots(roads: &RoadNetwork, lots: &SlotMap<LotId, Lot>, var: &str) {
    let Ok(path) = std::env::var(var) else { return };
    let mut out = String::new();
    for (sid, _) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        out.push_str(&format!("road|{},{};{},{}
", a.x, a.y, b.x, b.y));
    }
    let reasons = DEAD_REASONS.with(|r| r.borrow().clone());
    for l in lots.values() {
        let pts: Vec<String> = l.polygon.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        let kind = if l.usable() {
            "lot".to_string()
        } else {
            let r = reasons.iter().find(|(c, _, _, _)| c.distance(l.center) < 0.01).map(|(_, r, a, k)| format!("dead:{r}:{a:.0}:{k:.3}")).unwrap_or_else(|| "dead".into());
            r
        };
        out.push_str(&format!("{}|{}
", kind, pts.join(";")));
    }
    let _ = std::fs::write(path, out);
}

/// [review] A memoised cut is the cut: the same inputs give parcels equal
/// to a fresh cut, and a changed road misses the memo.
#[test]
fn memoised_parcels_match_a_fresh_cut() {
    let mut roads = roads_cross();
    let water = WaterMask::default();
    let clip = Some((Vec2::splat(-400.0), Vec2::splat(400.0)));
    let fresh = format!("{:?}", cut_parcels(&roads, &water, clip, None));
    let first = format!("{:?}", generate_parcels_with(&roads, &water, clip, None));
    let again = format!("{:?}", generate_parcels_with(&roads, &water, clip, None));
    assert_eq!(fresh, first);
    assert_eq!(fresh, again);
    roads.add_polyline(&[Vec2::new(-150.0, 120.0), Vec2::new(150.0, 120.0)], 1, 1);
    let changed = format!("{:?}", generate_parcels_with(&roads, &water, clip, None));
    assert_eq!(changed, format!("{:?}", cut_parcels(&roads, &water, clip, None)));
    assert_ne!(changed, fresh, "a new road recuts");
}

/// [parcel-size] F27: a sliver along a street and the backland behind it,
/// neither usable alone ("no road access" on ground touching the road), are
/// mended into one sound parcel.
#[test]
fn a_sliver_and_the_backland_behind_it_are_mended_into_one_parcel() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    let hw = roads.segment(roads.alive_segments().next().unwrap().0).corridor_half_width();
    let sliver = rect(0.0, hw, 30.0, hw + 3.0);
    let back = rect(0.0, hw + 3.0, 30.0, hw + 33.0);
    assert!(!fronted(&back, PARCEL_USE, &roads), "the backland has no frontage");
    let sliver_lot = make_lot(ZoneId::default(), PARCEL_USE, ccw(&sliver), inner_center(&sliver), &roads).unwrap();
    assert!(sliver_lot.landlocked, "the sliver alone is no lot");
    let out = mend_parcels(vec![sliver, back], &roads, &WaterMask::default());
    assert_eq!(out.len(), 1, "one parcel");
    let lot = make_lot(ZoneId::default(), PARCEL_USE, ccw(&out[0]), inner_center(&out[0]), &roads).unwrap();
    assert!(!lot.landlocked, "with a frontage on the street");
    assert!((polygon_area(&out[0]).abs() - 30.0 * 33.0).abs() < 1.0);
}

/// [parcel-size] F37: no parcel is past the size cap, fronted or not.
#[test]
fn a_huge_piece_is_cut_down_to_the_size_cap() {
    let big = rect(0.0, 0.0, 160.0, 90.0);
    let mut out = Vec::new();
    cap_parcel(big, &WaterMask::default(), 0, &mut out);
    assert!(out.len() >= 6);
    let total: f32 = out.iter().map(|p| polygon_area(p).abs()).sum();
    assert!((total - 160.0 * 90.0).abs() < 1.0, "nothing lost");
    for p in &out {
        assert!(polygon_area(p).abs() <= MAX_PARCEL_AREA + 1.0, "{} m²", polygon_area(p).abs());
    }
}

/// [parcel-size] A real cut of a big block leaves nothing over the cap.
#[test]
fn a_fresh_cut_has_no_parcel_past_the_cap() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, -150.0), Vec2::new(300.0, -150.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-300.0, 150.0), Vec2::new(300.0, 150.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-300.0, -150.0), Vec2::new(-300.0, 150.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(300.0, -150.0), Vec2::new(300.0, 150.0)], 1, 1);
    let cut = recut_parcels(&[], &CutBasis::default(), &roads, &WaterMask::default(), Some((Vec2::splat(-500.0), Vec2::splat(500.0))), None, &[]);
    assert!(!cut.parcels.is_empty());
    for p in &cut.parcels {
        assert!(p.area <= MAX_PARCEL_AREA + 1.0, "parcel at {} is {:.0} m²", p.center, p.area);
    }
}
