use super::*;

fn square(w: f32, d: f32) -> Vec<Vec2> {
    vec![Vec2::new(-w * 0.5, -d * 0.5), Vec2::new(w * 0.5, -d * 0.5), Vec2::new(w * 0.5, d * 0.5), Vec2::new(-w * 0.5, d * 0.5)]
}

#[test]
fn turbine_count_scales_with_lot_area_and_all_fit() {
    let mut last = 0;
    for side in [24.0f32, 40.0, 60.0, 90.0, 130.0] {
        let poly = square(side, side);
        let sites = wind_sites(&poly, 9);
        assert!(!sites.is_empty(), "a {side} m lot holds a turbine");
        assert!(sites.len() >= last, "{side} m: {} turbines, fewer than a smaller lot's {last}", sites.len());
        last = sites.len();
        for (p, dh, _) in &sites {
            assert!(nd_sim::zoning::point_in_polygon(*p, &poly));
            assert!(distance_to_boundary(*p, &poly) >= TURBINE_RADIUS + TURBINE_CLEARANCE - 1e-3, "{side} m: rotor reaches past the lot");
            assert!(dh.abs() <= 0.3, "turbines are one size");
        }
        for (i, (a, _, _)) in sites.iter().enumerate() {
            for (b, _, _) in &sites[i + 1..] {
                assert!(a.distance(*b) >= TURBINE_SPACING * 0.95 - 1e-3, "{side} m: turbines too close");
            }
        }
    }
    assert!(last >= 20, "a 130 m farm holds a grid of turbines, got {last}");
    assert!(wind_sites(&square(8.0, 8.0), 1).is_empty(), "no turbine on a lot too small for its blades");
}

/// Trackers pack densely (about one per 16 m² of open field), keep
/// their pitch, keep every panel tip inside the lot even fully tilted,
/// and leave the front strip free; triangle lots work too.
#[test]
fn solar_trackers_pack_a_honeycomb_inside_the_lot() {
    for (w, d) in [(30.0f32, 30.0f32), (60.0, 45.0), (120.0, 80.0)] {
        let poly = square(w, d);
        let sites = solar_sites(&poly);
        assert!(!sites.is_empty(), "{w}x{d} holds trackers");
        let open = (w - 2.0 * (SOLAR_PANEL_R + SOLAR_EDGE)) * (d - SOLAR_FRONT_CLEAR - 2.0 * SOLAR_PANEL_R - SOLAR_EDGE);
        assert!(sites.len() as f32 >= open / 20.0, "{w}x{d}: {} trackers for {open:.0} m²", sites.len());
        for (i, p) in sites.iter().enumerate() {
            assert!(distance_to_boundary(*p, &poly) >= SOLAR_PANEL_R + SOLAR_EDGE - 1e-3);
            assert!(p.y >= -d * 0.5 + SOLAR_FRONT_CLEAR);
            for q in &sites[i + 1..] {
                assert!(p.distance(*q) >= SOLAR_PITCH - 1e-3);
            }
        }
    }
    let tri = vec![Vec2::new(-50.0, -30.0), Vec2::new(60.0, -30.0), Vec2::new(-10.0, 50.0)];
    for p in solar_sites(&tri) {
        assert!(nd_sim::zoning::point_in_polygon(p, &tri) && distance_to_boundary(p, &tri) >= SOLAR_PANEL_R);
    }
    // The panel turned as far as it goes stays above the ground and
    // within its circle.
    assert!(SOLAR_HUB - SOLAR_PANEL_R * SOLAR_MAX_TILT.sin() > 0.4);
    let head = solar_head();
    let reach = head.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt()).fold(0.0f32, f32::max);
    assert!(reach <= SOLAR_PANEL_R + 1e-3, "panel reaches {reach}");
    assert!(head.triangle_count() < 120, "{} triangles a tracker", head.triangle_count());
    assert!(!head.get(Glow).positions.is_empty(), "a rim light");
}

#[test]
fn battery_lights_count_fifths_and_colour_by_flow() {
    assert_eq!(battery_lights(0.0, 0.0).0, 0);
    assert_eq!(battery_lights(0.01, 0.0).0, 1, "a sliver of charge shows");
    assert_eq!(battery_lights(0.2, 0.0).0, 1);
    assert_eq!(battery_lights(0.21, 0.0).0, 2);
    assert_eq!(battery_lights(1.0, 0.0).0, BATTERY_LIGHTS);
    let (_, charging) = battery_lights(0.5, -5.0);
    let (_, giving) = battery_lights(0.5, 5.0);
    let (_, idle) = battery_lights(0.5, 0.0);
    assert!(charging != giving && giving != idle && idle != charging);
}

#[test]
fn turbine_layout_is_deterministic_and_triangle_lots_stay_contained() {
    let tri = vec![Vec2::new(-50.0, -30.0), Vec2::new(60.0, -30.0), Vec2::new(-10.0, 50.0)];
    assert_eq!(wind_sites(&tri, 4), wind_sites(&tri, 4));
    for (p, _, _) in wind_sites(&tri, 4) {
        assert!(distance_to_boundary(p, &tri) >= TURBINE_RADIUS + TURBINE_CLEARANCE - 1e-3);
    }
    let rotor = wind_rotor(0.3);
    let reach = rotor.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt()).fold(0.0f32, f32::max);
    assert!(reach <= TURBINE_RADIUS + 0.7, "blades sweep {reach} m, past the clearance");
}
/// [plant-scale] A street along y = 0 and an industrial lot `w` wide and
/// `d` deep fronting it.
fn plant_lot(w: f32, d: f32) -> nd_sim::Lot {
    let mut roads = nd_sim::RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let poly = vec![Vec2::new(-w * 0.5, 6.0), Vec2::new(w * 0.5, 6.0), Vec2::new(w * 0.5, 6.0 + d), Vec2::new(-w * 0.5, 6.0 + d)];
    nd_sim::zoning::make_lot(nd_sim::zoning::ZoneId::default(), nd_sim::LandUse::Industrial, poly, Vec2::new(0.0, 6.0 + d * 0.5), &roads).expect("a lot")
}

fn coal_mesh(lot: &nd_sim::Lot, seed: u64, scale: f32) -> Geometry {
    let extras = super::super::Extras { plant: Some(nd_sim::PowerPlantKind::Coal), plant_scale: scale, ..Default::default() };
    super::super::generate_building(lot, nd_sim::LandUse::Industrial, 1, nd_sim::Tier::Mid, 1.0, seed, &super::super::BuildingRules::default(), &crate::palette::Palette::default(), &extras)
}

fn fingerprint(g: &Geometry) -> (usize, f64) {
    let mut sum = 0.0f64;
    for (_, m) in g.iter() {
        for p in &m.positions {
            sum += (p[0] as f64) * 1.3 + (p[1] as f64) * 0.7 + (p[2] as f64) * 1.9;
        }
    }
    (g.triangle_count(), sum)
}

/// [plant-scale] The coal station is the same for the same seed and
/// size, composed of more units (not stretched) on a bigger site: more
/// stacks of different heights, cooling towers on the biggest, every
/// stack on the lot, and the renderer's plumes start at the stacks.
#[test]
fn coal_station_is_deterministic_and_grows_by_units() {
    let small = plant_lot(24.0, 30.0);
    let mid = plant_lot(32.0, 34.0);
    let big = plant_lot(48.0, 52.0);
    let cases = [(&small, 0.72f32), (&mid, 1.09), (&big, 2.4)];
    let mut last_stacks = 0;
    for (lot, scale) in cases {
        let (_, poly) = crate::buildings::Frame::lot_local(lot);
        let a = coal_layout(&poly, 77, scale);
        assert_eq!(a, coal_layout(&poly, 77, scale), "same seed, same station");
        assert!(!a.boilers.is_empty() && !a.stacks.is_empty() && !a.heaps.is_empty(), "{scale}: {a:?}");
        assert!(a.stacks.len() >= last_stacks, "{scale}: {} stacks after {last_stacks}", a.stacks.len());
        last_stacks = a.stacks.len();
        for &(p, r, h, _) in &a.stacks {
            assert!(nd_sim::zoning::point_in_polygon(p, &poly) && distance_to_boundary(p, &poly) >= r, "{scale}: a stack off the lot");
            assert!(h > 20.0);
        }
        let heights: Vec<i32> = a.stacks.iter().map(|s| s.2.round() as i32).collect();
        if a.stacks.len() >= 2 {
            assert!(heights.iter().any(|h| *h != heights[0]), "{scale}: stacks of different heights {heights:?}");
        }
        let vents = coal_vents(lot, 77, scale);
        assert_eq!(vents.iter().filter(|v| !v.steam).count(), a.stacks.len(), "a plume per stack");
        assert_eq!(vents.iter().filter(|v| v.steam).count(), a.towers.len(), "steam per cooling tower");
        let g1 = coal_mesh(lot, 77, scale);
        let g2 = coal_mesh(lot, 77, scale);
        assert_eq!(fingerprint(&g1), fingerprint(&g2), "{scale}: same seed, same mesh");
        check_mesh_ok(&g1);
    }
    let (_, poly) = crate::buildings::Frame::lot_local(&big);
    let b = coal_layout(&poly, 77, 2.4);
    let (_, poly_s) = crate::buildings::Frame::lot_local(&small);
    let s = coal_layout(&poly_s, 77, 0.72);
    assert!(b.stacks.len() >= s.stacks.len() + 3, "big {} stacks, small {}", b.stacks.len(), s.stacks.len());
    assert!(b.boilers.len() > s.boilers.len());
    assert!(!b.towers.is_empty(), "cooling towers on the biggest");
    assert!(s.towers.is_empty());
    // Another seed lays it out differently.
    assert_ne!(coal_layout(&poly, 78, 2.4), b);
    // A huge lot does not stretch it: the station keeps its size and
    // the rest is coal yard.
    let huge = plant_lot(110.0, 110.0);
    let (_, poly_h) = crate::buildings::Frame::lot_local(&huge);
    let h = coal_layout(&poly_h, 77, 2.4);
    assert_eq!(h.boilers.len(), b.boilers.len());
    let size = |l: &CoalLayout| l.boilers.iter().map(|x| x.1.x * x.1.y).sum::<f32>();
    assert!((size(&h) - size(&b)).abs() < 1e-3 || h.boilers.iter().all(|x| x.1.x < 5.5), "boilers keep their size");
    assert!(h.heaps.len() > b.heaps.len(), "more coal yard on the spare ground");
    // Older saves' plants (scale 1) are the two-unit station.
    let (_, poly_m) = crate::buildings::Frame::lot_local(&mid);
    assert_eq!(coal_layout(&poly_m, 5, 1.0).boilers.len(), 2);
}

fn check_mesh_ok(g: &Geometry) {
    for (s, m) in g.iter() {
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{s:?} has NaN");
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{s:?} bad index");
    }
}
#[test]
#[ignore]
fn coal_layout_ascii() {
    for (w, d, scale) in [(24.0f32, 30.0f32, 0.72f32), (32.0, 34.0, 1.09), (40.0, 40.0, 1.6), (48.0, 52.0, 2.4), (110.0, 110.0, 2.4)] {
        let lot = plant_lot(w, d);
        let (_, poly) = crate::buildings::Frame::lot_local(&lot);
        let l = coal_layout(&poly, 77, scale);
        let (lo, hi) = bounds(&poly);
        let mut grid = vec![vec!['.'; ((hi.x - lo.x) / 1.0) as usize + 1]; ((hi.y - lo.y) / 2.0) as usize + 1];
        let mut mark = |c: Vec2, h: Vec2, ch: char| {
            for row in 0..grid.len() {
                for col in 0..grid[row].len() {
                    let p = Vec2::new(lo.x + col as f32 * 1.0, lo.y + row as f32 * 2.0);
                    if (p.x - c.x).abs() <= h.x && (p.y - c.y).abs() <= h.y {
                        grid[row][col] = ch;
                    }
                }
            }
        };
        for h in &l.heaps { mark(h.0, h.1, 'c'); }
        for b in &l.boilers { mark(b.0, b.1, 'B'); }
        for f in &l.filters { mark(f.0, f.1, 'f'); }
        if let Some(t) = l.turbine { mark(t.0, t.1, 'T'); }
        for t in &l.towers { mark(t.0, Vec2::splat(t.1), 'O'); }
        for s in &l.stacks { mark(s.0, Vec2::splat(s.1), 'S'); }
        if let Some(t) = l.transformer { mark(t, Vec2::new(5.0, 4.0), 'x'); }
        println!("{w}x{d} scale {scale}: {} boilers, {} stacks {:?}, {} towers, {} heaps, turbine {}", l.boilers.len(), l.stacks.len(), l.stacks.iter().map(|s| s.2.round()).collect::<Vec<_>>(), l.towers.len(), l.heaps.len(), l.turbine.is_some());
        for row in grid.iter().rev() {
            println!("{}", row.iter().collect::<String>());
        }
    }
}
