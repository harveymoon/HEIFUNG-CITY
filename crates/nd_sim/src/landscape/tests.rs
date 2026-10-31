use super::*;

#[test]
fn landscapes_are_deterministic_and_presets_differ() {
    for preset in LandscapePreset::ALL {
        let p = LandscapeParams::preset(preset, 42);
        let a = Landscape::new(&p);
        let b = Landscape::new(&p);
        assert_eq!(format!("{:?}", a.rivers.iter().map(|r| r.points.clone()).collect::<Vec<_>>()), format!("{:?}", b.rivers.iter().map(|r| r.points.clone()).collect::<Vec<_>>()), "{preset:?}");
        assert_eq!(a.lakes.len(), b.lakes.len());
        for k in 0..40 {
            let q = Vec2::new(k as f32 * 83.0 - 1600.0, (k * 37 % 29) as f32 * 110.0 - 1500.0);
            assert_eq!(a.height(q), b.height(q), "{preset:?} at {q:?}");
            assert_eq!(a.is_water(q), b.is_water(q));
        }
        // The middle of the map is always dry land.
        for k in 0..36 {
            let q = Vec2::from_angle(k as f32 * 0.1745) * (CLEAR_RADIUS - 10.0);
            assert!(!a.is_water(q), "{preset:?}: water at {q:?} in the clear zone");
            assert!(!a.is_water(q * 0.5));
        }
    }
    let valley = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 1));
    assert!(!valley.rivers.is_empty() && valley.sea.is_none());
    let other = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 2));
    assert_ne!(valley.rivers[0].points[10], other.rivers[0].points[10], "the seed moves the river");
    let isl = Landscape::new(&LandscapeParams::preset(LandscapePreset::Islands, 1));
    assert!(isl.sea.as_ref().is_some_and(|s| s.islands.len() >= 2), "a big island and offshore ones");
    assert!(isl.is_water(Vec2::new(1500.0, 1500.0)));
    let delta = Landscape::new(&LandscapeParams::preset(LandscapePreset::Delta, 3));
    assert!(delta.rivers.len() >= 3, "channels fan out");
    let flat = Landscape::new(&LandscapeParams::flat());
    assert!(flat.is_flat() && flat.height(Vec2::new(100.0, 50.0)) == 0.0);
}

/// A hash (FNV-1a, stable across toolchains) of everything a landscape
/// generates, to pin the output.
fn fingerprint(l: &Landscape) -> u64 {
    let text = format!(
        "{:?}|{:?}|{:?}|{:?}",
        l.rivers.iter().map(|r| (r.points.clone(), r.width, r.current, r.stream)).collect::<Vec<_>>(),
        l.lakes.iter().map(|k| k.outline.clone()).collect::<Vec<_>>(),
        l.sea.as_ref().map(|s| (s.outline.clone(), s.islands.clone())),
        l.hot_spots.iter().map(|s| (s.pos, s.radius, s.heat)).collect::<Vec<_>>()
    );
    crate::hash::fnv_bytes(text.bytes())
}

#[test]
fn desert_output_is_pinned() {
    let mut all = 0u64;
    for preset in LandscapePreset::ALL {
        for seed in [1u64, 7, 42] {
            let p = LandscapeParams { branching: 0.6, ..LandscapeParams::preset(preset, seed) };
            all = all.rotate_left(7) ^ fingerprint(&Landscape::new(&p));
            all = all.rotate_left(7) ^ fingerprint(&Landscape::new(&LandscapeParams::preset(preset, seed)));
        }
    }
    assert_eq!(all, PINNED, "the Desert (default) landscape changed: old saves and tests depend on it");
}
const PINNED: u64 = 0x56b7_45ee_eb64_3b97;

#[test]
fn climates_are_deterministic_and_add_their_features() {
    for climate in Climate::ALL {
        for preset in LandscapePreset::ALL {
            let p = LandscapeParams { climate, ..LandscapeParams::preset(preset, 11) };
            let (a, b) = (Landscape::new(&p), Landscape::new(&p));
            assert_eq!(fingerprint(&a), fingerprint(&b), "{climate:?} {preset:?}");
            // The middle stays dry whatever the climate adds.
            for k in 0..36 {
                let q = Vec2::from_angle(k as f32 * 0.1745) * (CLEAR_RADIUS - 10.0);
                assert!(!a.is_water(q), "{climate:?} {preset:?}: water at {q:?}");
            }
            for h in &a.hot_spots {
                assert!(!a.is_water(h.pos), "{climate:?} {preset:?}: hot spot in the water");
            }
        }
    }
    for seed in [1u64, 7, 42, 99] {
        for preset in [LandscapePreset::RiverValley, LandscapePreset::Coast, LandscapePreset::Plains, LandscapePreset::Islands] {
            let base = LandscapeParams::preset(preset, seed);
            let desert = Landscape::new(&base);
            let volcanic = Landscape::new(&LandscapeParams { climate: Climate::Volcanic, ..base.clone() });
            assert!(volcanic.hot_spots.len() >= desert.hot_spots.len() + 3, "{preset:?} {seed}: {} vs {}", volcanic.hot_spots.len(), desert.hot_spots.len());
            // The climate adds to the map; the generator's own features stay put.
            assert_eq!(format!("{:?}", &volcanic.hot_spots[..desert.hot_spots.len()].iter().map(|h| h.pos).collect::<Vec<_>>()), format!("{:?}", desert.hot_spots.iter().map(|h| h.pos).collect::<Vec<_>>()));
            let wet = Landscape::new(&LandscapeParams { climate: Climate::Tropical, ..base.clone() });
            assert!(wet.rivers.len() + wet.lakes.len() >= desert.rivers.len() + desert.lakes.len(), "{preset:?} {seed}");
        }
    }
    let isl = LandscapeParams::preset(LandscapePreset::Islands, 3);
    let tropical = Landscape::new(&LandscapeParams { climate: Climate::Tropical, ..isl.clone() });
    assert!(tropical.sea.as_ref().unwrap().islands.len() > Landscape::new(&isl).sea.as_ref().unwrap().islands.len(), "a tropical sea has more islands");
}

#[test]
fn a_new_world_takes_its_weather_from_the_landscape_climate() {
    for climate in Climate::ALL {
        let w = crate::World::new(crate::WorldConfig { landscape: LandscapeParams { climate, ..LandscapeParams::flat() }, emergencies: false, society: false, ..crate::WorldConfig::default() });
        assert_eq!(w.landscape.params.climate, climate);
        assert_eq!(w.weather.climate, climate, "the grid-weather climate matches the look");
    }
}

#[test]
fn old_saves_without_a_climate_are_desert() {
    let p: LandscapeParams = ron::from_str("(preset: Coast, seed: 5, water: 0.5, relief: 0.3, branching: 0.25, islands: 1, highway: true)").unwrap();
    assert_eq!(p.climate, Climate::Desert);
    assert_eq!(p.preset, LandscapePreset::Coast);
    let p2: LandscapeParams = ron::from_str(&ron::to_string(&LandscapeParams { climate: Climate::Arctic, ..p.clone() }).unwrap()).unwrap();
    assert_eq!(p2.climate, Climate::Arctic);
    assert_eq!(Climate::from_label("Iceland"), Some(Climate::Volcanic));
    assert_eq!(Climate::from_label("tropical"), Some(Climate::Tropical));
}

#[test]
fn water_routes_always_end() {
    // Junctions, delta channels and duplicate points make zero-length
    // edges; every route must still come back, and be short.
    for preset in [LandscapePreset::Delta, LandscapePreset::RiverValley, LandscapePreset::Coast, LandscapePreset::Islands, LandscapePreset::Plains] {
        for seed in 1..8 {
            let l = Landscape::new(&LandscapeParams { branching: 0.9, ..LandscapeParams::preset(preset, seed) });
            let probes: Vec<Vec2> = l.rivers.iter().flat_map(|r| r.points.iter().step_by(9).copied()).collect();
            for (k, p) in probes.iter().enumerate() {
                if let Some(r) = l.water_route(*p, None) {
                    assert!(r.len() < 2000, "{preset:?}/{seed}: {} points", r.len());
                }
                if let Some(q) = probes.get(k * 7 % probes.len()) {
                    if let Some(r) = l.water_route(*p, Some(*q)) {
                        assert!(r.len() < 2000);
                    }
                }
            }
        }
    }
}

#[test]
fn rivers_carry_a_current_and_route_off_the_map() {
    let l = Landscape::new(&LandscapeParams { branching: 0.8, ..LandscapeParams::preset(LandscapePreset::RiverValley, 5) });
    let main = &l.rivers[0];
    let mid = main.points[main.points.len() / 2];
    let hit = l.water_at(mid).expect("water on the centreline");
    assert_eq!(hit.kind, WaterKind::River);
    assert!(hit.current > 0.3 && hit.flow.length() > 0.9);
    assert!(l.height(mid) < WATER_LEVEL, "the bed is below the water");
    let bank = mid + Vec2::new(main.width, 0.0).rotate(hit.flow.perp());
    assert!(l.height(bank) >= BANK_HEIGHT - 0.5);
    // Streams run faster than the river.
    if let Some(s) = l.rivers.iter().find(|r| r.stream) {
        assert!(s.current > main.current * 0.9);
        assert!(s.width < main.width);
    }
    let route = l.water_route(mid, None).expect("a way off the map");
    let end = *route.last().unwrap();
    assert!(end.x.abs() > LAND_HALF - 60.0 || end.y.abs() > LAND_HALF - 60.0, "ends at the edge: {end:?}");
    // Shore finding from a point on land beside it.
    let shore = l.nearest_shore(bank + hit.flow.perp() * 30.0, 120.0).or_else(|| l.nearest_shore(bank - hit.flow.perp() * 30.0, 120.0));
    assert!(shore.is_some());
}

/// The starter avenue crosses the highway, which stands on a high
/// embankment on the default map: it passes under in a cut, the highway
/// keeps its height on a deck over it, and the embankment either side
/// slopes no steeper than about one in two.
#[test]
fn streets_pass_under_the_highway_and_embankments_slope() {
    let mut w = crate::world::World::new(crate::world::WorldConfig::default());
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 2, 2);
    w.add_road(&[Vec2::new(0.0, -700.0), Vec2::new(0.0, 700.0)], 2, 2);
    let cuts: Vec<&Underpass> = w.ground.underpasses.iter().filter(|u| u.at.x.abs() < 1.0 && (u.at.y - HIGHWAY_Y).abs() < 20.0).collect();
    assert_eq!(cuts.len(), 2, "the avenue passes under both carriageways");
    for u in cuts {
        assert!(u.over_z - u.under_z >= UNDERPASS_CLEARANCE, "{u:?}");
        // In the cut the ground is the avenue's, under the deck too.
        for along in [-20.0f32, -5.0, 0.0, 5.0, 20.0] {
            let p = u.at + u.under_dir * along;
            assert!((w.ground_height(p) - u.under_z_at(along)).abs() < 0.3, "ground at {p} is {}, the avenue {}", w.ground_height(p), u.under_z_at(along));
        }
        // What drives along the highway stays up on its deck.
        assert!((w.surface_height(u.at + u.over_dir * 6.0) - u.over_z_at(6.0)).abs() < 0.3, "{} vs {} {u:?} decks {:?}", w.surface_height(u.at + u.over_dir * 6.0), u.over_z_at(6.0), w.ground.decks_at(u.at + u.over_dir * 6.0));
    }
    // Away from the avenue, across the embankment: no cliff (2.5 across
    // per metre up is 0.4; the raster and the blend allow a little more).
    for x in [-300.0f32, 200.0] {
        let mut worst = 0.0f32;
        let mut prev = w.ground_height(Vec2::new(x, HIGHWAY_Y - 140.0));
        for k in 1..=70 {
            let y = HIGHWAY_Y - 140.0 + k as f32 * 4.0;
            let h = w.ground_height(Vec2::new(x, y));
            // The two carriageways may stand at different heights: the
            // median between them is not an embankment side.
            if (y - HIGHWAY_Y).abs() > HIGHWAY_HALF_GAP + 10.0 {
                worst = worst.max((h - prev).abs() / 4.0);
            }
            prev = h;
        }
        assert!(worst < 0.55, "embankment at x = {x} as steep as {worst:.2}");
    }
}

/// The demo district's roads, zoning, rail branch, highway link and rail
/// facilities (the layout `nd_app`'s `seed_demo_district` lays out) on
/// the default map.
fn demo_ground_world() -> crate::world::World {
    use crate::roads::RoadKind;
    use crate::{Gesture, LandUse};
    let mut w = crate::world::World::new(crate::world::WorldConfig::default());
    w.start_project();
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        w.add_gesture(Gesture::Road { points: vec![Vec2::new(-280.0, c), Vec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        w.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -280.0), Vec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    }
    let cell = |cx: f32, cy: f32| vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)];
    let uses = [LandUse::Industrial, LandUse::Residential, LandUse::Commercial, LandUse::Park];
    for (k, (x, y)) in [-210.0f32, -70.0, 70.0, 210.0].iter().flat_map(|&x| [-210.0f32, -70.0, 70.0, 210.0].map(move |y| (x, y))).enumerate() {
        w.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: uses[k % uses.len()] });
    }
    w.add_gesture(Gesture::Road {
        points: vec![Vec2::new(-420.0, crate::rail::TRUNK_Y), Vec2::new(-330.0, -410.0), Vec2::new(-280.0, -348.0), Vec2::new(-110.0, -348.0), Vec2::new(-70.0, -324.0), Vec2::new(150.0, -324.0)],
        lanes_fwd: 0,
        lanes_bwd: 0,
        curved: false,
        kind: RoadKind::Rail,
        elevation: Vec::new(), car_only: None });
    w.implement();
    w.economy.treasury = w.economy.treasury.max(150_000.0);
    assert!(w.link_to_highway(Vec2::new(0.0, 280.0)), "the demo's ramps join the highway");
    let _ = w.place_rail_facility(crate::RailFacility::FreightYard, Vec2::new(-200.0, -330.0));
    let _ = w.place_rail_facility(crate::RailFacility::Station, Vec2::new(80.0, -310.0));
    w
}

/// [review] A corner lot levelled to its front street that stands over
/// a lower side street meets it with a retaining wall: inside the lot
/// the ground is the pad right up to the edge (it used to ramp down
/// across a ground cell to the side street's pavement), and the wall
/// runs from the street's height up to the pad. Every such edge has one.
#[test]
fn corner_lots_over_a_lower_side_street_get_a_retaining_wall() {
    let w = demo_ground_world();
    let g = &w.ground;
    // Independently: lot edges with a street they do not front beside
    // them that lies well below the pad.
    let mut expected = 0;
    for (id, lot) in w.lots.iter() {
        let pad = w.lot_pad(id);
        let m = lot.polygon.len();
        for k in 0..m {
            let (a, b) = (lot.polygon[k], lot.polygon[(k + 1) % m]);
            if a.distance(b) < 4.0 {
                continue;
            }
            let mid = a.lerp(b, 0.5);
            let low = w.roads.alive_segments().any(|(sid, s)| {
                if sid == lot.front_segment || !s.is_street() || s.bridge || s.tunnel || s.viaduct {
                    return false;
                }
                let (p, q) = w.roads.segment_points(sid);
                let (t, on) = project_on_segment(mid, p, q);
                let ends = [a, b].iter().all(|&e| project_on_segment(e, p, q).1.distance(e) <= s.corridor_half_width() + 0.5);
                on.distance(mid) <= s.corridor_half_width() + 0.5 && ends && pad - w.road_height(sid, t) > RETAIN_MIN + 0.2
            });
            if low {
                expected += 1;
                assert!(g.retaining.iter().any(|r| r.lot == id && r.a.lerp(r.b, 0.5).distance(mid) < 0.01), "the edge at {mid} of a lot {:.2} m over its side street has a wall", pad);
            }
        }
    }
    println!("{} retaining walls, {expected} edges well over a side street", g.retaining.len());
    assert!(expected >= 1, "the demo's slopes make some corner lots stand over a side street");
    for r in &g.retaining {
        assert!(r.top - r.foot_a.max(r.foot_b) >= RETAIN_MIN - 1e-3, "{r:?}");
        assert_eq!(r.top, w.lot_pad(r.lot));
        let mid = r.a.lerp(r.b, 0.5);
        // On the pad right up to the wall, and on the wall's line too.
        for x in [mid + r.inward * 0.3, mid + r.inward * 2.0, mid] {
            assert!((w.ground_height(x) - r.top).abs() < 1e-3, "inside the lot at {x}: ground {} pad {}", w.ground_height(x), r.top);
        }
        // The wall faces the street: just outside, the ground is down
        // at the street, not the pad.
        let foot = (r.foot_a + r.foot_b) * 0.5;
        let out = mid - r.inward * 0.5;
        assert!(w.ground_height(out) < r.top - 0.2, "outside the wall at {out}: ground {} street {foot} pad {}", w.ground_height(out), r.top);
    }
}

/// The built ground is smooth: between free cells (not a road, a cut or
/// a pad) it is never steeper than 1 in 1.5, no free cell stands up or
/// sinks as a spike against all eight neighbours, the ground beside every
/// road and pad runs down (or up) to it no steeper than that either, and
/// there is no seam where the raster meets the ground beyond it.
#[test]
fn built_ground_has_no_cliffs_spikes_or_seams() {
    let w = demo_ground_world();
    let t0 = std::time::Instant::now();
    let again = Ground::build(&w.landscape, &w.roads, &w.road_z, &w.road_inner, &w.lots, w.ground_half());
    println!("ground built in {:?}", t0.elapsed());
    assert_eq!(again.h, w.ground.h);
    let g = &w.ground;
    let n = g.n;
    assert!(n > 2);
    let at = |i: usize, j: usize| Vec2::new(-g.half + i as f32 * GROUND_CELL, -g.half + j as f32 * GROUND_CELL);
    let limit = 1.0 / 1.5;
    let (mut worst, mut worst_at) = (0.0f32, Vec2::ZERO);
    let (mut worst_held, mut held_at) = (0.0f32, Vec2::ZERO);
    let mut spikes = Vec::new();
    // Walls stand at underpass cuts, sunken roads and tunnel portals, and
    // under a viaduct the ground is left as it was: none of that is
    // earthwork.
    let skip: Vec<bool> = (0..n * n).map(|k| matches!(g.fix[k], Fix::Cut | Fix::Wall) || !g.decks_at(at(k % n, k / n)).is_empty()).collect();
    for j in 1..n - 1 {
        for i in 1..n - 1 {
            let k = j * n + i;
            let mut walled = skip[k];
            let mut above = 0;
            let mut below = 0;
            for (di, dj) in [(-1isize, -1isize), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                let m = (j as isize + dj) as usize * n + (i as isize + di) as usize;
                let run = GROUND_CELL * if di != 0 && dj != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                let s = (g.h[k] - g.h[m]).abs() / run;
                if g.h[k] > g.h[m] + 1.0 {
                    above += 1;
                }
                if g.h[k] < g.h[m] - 1.0 {
                    below += 1;
                }
                walled |= skip[m];
                if g.fix[k] != Fix::Free || skip[k] || skip[m] {
                    continue;
                }
                // The hills are as steep as they were made; only the
                // works are held to the limit.
                let (pk, pm) = (at(i, j), at((i as isize + di) as usize, (j as isize + dj) as usize));
                let natural = (w.landscape.height(pk) - w.landscape.height(pm)).abs() / run;
                let s = if s <= natural + 0.02 { 0.0 } else { s };
                if g.fix[m] == Fix::Free {
                    if s > worst {
                        (worst, worst_at) = (s, at(i, j));
                    }
                } else if s > worst_held {
                    (worst_held, held_at) = (s, at(i, j));
                }
            }
            if g.fix[k] == Fix::Free && !walled && (above == 8 || below == 8) {
                spikes.push(at(i, j));
            }
        }
    }
    println!("steepest free ground 1 in {:.2} at {worst_at}; beside a road or pad 1 in {:.2} at {held_at}; {} spikes", 1.0 / worst, 1.0 / worst_held, spikes.len());
    if std::env::var("ND_GROUND_DEBUG").is_ok() {
        for c in [worst_at, held_at] {
            let ci = ((c.x + g.half) / GROUND_CELL).round() as isize;
            let cj = ((c.y + g.half) / GROUND_CELL).round() as isize;
            for dj in -3isize..=3 {
                let row: Vec<String> = (-3isize..=3).map(|di| {
                    let k = (cj + dj) as usize * n + (ci + di) as usize;
                    format!("{:7.2}{}|{:6.1}", g.h[k], match g.fix[k] { Fix::Free => ' ', Fix::Road => 'R', Fix::Cut => 'C', Fix::Wall => 'W', Fix::Pad => 'P' }, g.outer_height(&w.landscape, at((ci + di) as usize, (cj + dj) as usize)))
                }).collect();
                println!("y {:7.1}: {}", at(0, (cj + dj) as usize).y, row.join(" "));
            }
            println!();
            for o in &g.outer {
                if project_on_segment(c, o.0, o.1).1.distance(c) < 80.0 {
                    println!("outer {:?}", o);
                }
            }
        }
    }
    assert!(worst <= limit, "free ground as steep as 1 in {:.2} at {worst_at}", 1.0 / worst);
    assert!(worst_held <= limit, "ground beside a road or pad as steep as 1 in {:.2} at {held_at}", 1.0 / worst_held);
    assert!(spikes.is_empty(), "spikes at {spikes:?}");
    // The ways keep their heights through the grading: no embankment as
    // high as the hills at the map edge across the basin (the rail
    // branch behind the plant stood 50 m up against the lots).
    for p in [Vec2::new(-200.0, -348.0), Vec2::new(-200.0, crate::rail::TRUNK_Y), Vec2::new(-120.0, HIGHWAY_Y - HIGHWAY_HALF_GAP)] {
        let rise = w.ground_height(p) - w.landscape.height(p);
        assert!(rise.abs() < 12.0, "the way at {p} stands {rise:.1} m off the land");
    }
    // No seam where the raster meets the ground beyond it, nor at the
    // edge of the drawn map.
    for edge in [g.half, LAND_HALF] {
        for k in 0..60 {
            let along = -1700.0 + k as f32 * 57.0;
            for p in [Vec2::new(edge, along), Vec2::new(-edge, along), Vec2::new(along, edge), Vec2::new(along, -edge)] {
                let out = p + p.signum() * Vec2::new((p.x.abs() >= edge - 0.1) as u8 as f32, (p.y.abs() >= edge - 0.1) as u8 as f32) * 2.0;
                let inn = p - (out - p);
                let (a, b) = (w.ground_height(inn), w.ground_height(out));
                assert!((a - b).abs() < 4.0 * limit, "a seam at {p}: {a:.2} inside, {b:.2} outside");
            }
        }
    }
}

/// [pt-visual] F11/F31: a highway cut deep into a hill or carried high over
/// a valley is held by retaining walls (`earthwork_hold`): the ground is
/// held at the road out to the wall band and stays the natural land just
/// beyond it, instead of 1-in-2.5 slopes reshaping the hillside.
#[test]
fn deep_highway_cuttings_and_fills_are_held_by_walls() {
    use crate::roads::RoadKind;
    let mut found = 0;
    for preset in [LandscapePreset::RiverValley, LandscapePreset::Islands] {
        let mut w = crate::world::World::new(crate::world::WorldConfig { landscape: LandscapeParams::preset(preset, 4), emergencies: false, society: false, ..crate::world::WorldConfig::default() });
        w.economy.treasury = 1e9;
        w.start_project();
        for k in -3..=3 {
            let y = k as f32 * 260.0;
            w.add_gesture(crate::Gesture::Road { points: vec![Vec2::new(-1400.0, y), Vec2::new(1400.0, y + 90.0)], lanes_fwd: 3, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        w.implement();
        for (sid, seg) in w.roads.alive_segments() {
            let Some(hold) = earthwork_hold(&w.landscape, &w.roads, &w.road_z, &w.road_inner, sid) else { continue };
            if w.roads.node(seg.a).level != 0.0 || w.roads.node(seg.b).level != 0.0 {
                continue;
            }
            // Inside a held run: the pieces either side are held the same way
            // (at a run's ends the graded neighbour's slope still reaches in).
            let held_both = [seg.a, seg.b].iter().all(|&n| w.roads.node(n).segments.iter().any(|&o| o != sid && earthwork_hold(&w.landscape, &w.roads, &w.road_z, &w.road_inner, o) == Some(hold)));
            if !held_both {
                continue;
            }
            let (a, b) = w.roads.segment_points(sid);
            let mid = a.lerp(b, 0.5);
            let side = (b - a).normalize_or_zero().perp();
            let z = way_height(&w.roads, &w.road_z, &w.road_inner, sid, 0.5);
            let (_, back) = earthwork_band(seg);
            // Inside the graded raster (beyond it the ground is the outer
            // blend along the ways).
            let half = w.ground_half() - 60.0;
            if mid.x.abs() > half || mid.y.abs() > half {
                continue;
            }
            for s in [-1.0f32, 1.0] {
                let near = mid + side * s * (back - 2.0);
                let beyond = mid + side * s * (back + 10.0);
                if w.roads.segments_in_box(beyond - Vec2::splat(30.0), beyond + Vec2::splat(30.0)).any(|o| o != sid) {
                    continue;
                }
                assert!((w.ground_height(near) - z).abs() < 0.8, "{hold:?}: the band is not held at the road ({} vs {z})", w.ground_height(near));
                let natural = w.landscape.height(beyond).max(BANK_HEIGHT);
                assert!((w.ground_height(beyond) - natural).abs() < 2.0, "{hold:?}: graded {} m beyond the wall", (w.ground_height(beyond) - natural).abs());
                found += 1;
            }
        }
    }
    assert!(found > 0, "no held highway earthworks to check");
}

/// [r4-edge-bridges] Harvey's flag "weird straight borders on the water":
/// the water runs on across the map's edge on every preset (and an Arctic
/// coast), wet on one side means wet on the other and the ground steps no
/// more than a bank's height; and so across the seam of a bought tile and
/// the edge of the land bought.
#[test]
fn the_water_is_continuous_across_the_map_edge_and_the_tile_seams() {
    use super::far::FarLand;
    let cases = [
        (LandscapePreset::RiverValley, Climate::Temperate),
        (LandscapePreset::Islands, Climate::Tropical),
        (LandscapePreset::Coast, Climate::Temperate),
        (LandscapePreset::Delta, Climate::Temperate),
        (LandscapePreset::Plains, Climate::Temperate),
        (LandscapePreset::Coast, Climate::Arctic),
    ];
    // Wet within a metre on one side, dry by more than one on the other.
    let differ = |a: f32, b: f32| (a < -2.0 && b > 2.0) || (a > 2.0 && b < -2.0);
    for (preset, climate) in cases {
        for seed in [3, 7] {
            let land = Landscape::new(&LandscapeParams { climate, ..LandscapeParams::preset(preset, seed) });
            let far = FarLand::new(&land);
            let grown = land.with_tiles(&[(2, 0), (-2, 0), (0, -2), (0, 2)]);
            let grown_far = FarLand::new(&grown);
            let (mut wet, mut wrong, mut step) = (0, 0, 0.0f32);
            let mut t = -LAND_HALF;
            while t <= LAND_HALF {
                for (n, e) in [(Vec2::X, Vec2::Y), (Vec2::NEG_X, Vec2::Y), (Vec2::Y, Vec2::X), (Vec2::NEG_Y, Vec2::X)] {
                    let on = n * LAND_HALF + e * t;
                    let (inside, outside) = (on - n * 0.5, on + n * 0.5);
                    // The map's edge: its raster against the far land.
                    let (a, b) = (land.water_distance(inside), far.water_distance(outside));
                    wet += (a < 0.0) as usize;
                    wrong += differ(a, b) as usize;
                    step = step.max((land.height(inside) - far.natural(outside)).abs());
                    // The seam of a bought tile: both sides the grown raster.
                    wrong += differ(grown.water_distance(inside), grown.water_distance(outside)) as usize;
                    // The edge of the land bought: its raster against the far land.
                    let (gi, go) = (n * GROWN_HALF + e * t - n * 0.5, n * GROWN_HALF + e * t + n * 0.5);
                    wrong += differ(grown.water_distance(gi), grown_far.water_distance(go)) as usize;
                }
                t += 5.0;
            }
            assert_eq!(wrong, 0, "{preset:?} {climate:?} seed {seed}: wet on one side of an edge and dry on the other");
            assert!(step < 3.0, "{preset:?} {climate:?} seed {seed}: a {step:.1} m step in the ground at the map's edge");
            if preset != LandscapePreset::RiverValley && preset != LandscapePreset::Plains {
                assert!(wet > 50, "{preset:?}: water at the edge to compare ({wet})");
            }
        }
    }
}
