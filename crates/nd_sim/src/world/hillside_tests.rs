//! [r4-hillside] Building on mountains like Hong Kong or San Francisco:
//! paths and streets lie on the hillside in a narrow bench held by walls,
//! town streets climb steeper than avenues and highways (and the tool says
//! when the land is steeper still, offering a zig-zag), cars slow on the
//! climb, terraces on a steep hillside are walled rather than graded, and
//! the Zone tool says how steep the ground is and what that allows.

use super::*;
use crate::landscape::{earthwork_hold, held_band, Hold, LandscapeParams, LandscapePreset, BANK_HEIGHT};
use crate::roads::RoadKind;
use super::water::{land_grade, AVENUE_GRADE, LOCAL_STREET_GRADE, PATH_GRADE, STREET_GRADE};

fn hilly_world() -> World {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::Plains, 7), ..WorldConfig::test_town() });
    w.economy.treasury = 1e9;
    w
}

/// A dry spot well clear of the water, 200..700 m out.
fn dry_spot(w: &World) -> Vec2 {
    let mut y = -700.0;
    while y <= 700.0 {
        let mut x = -700.0;
        while x <= 700.0 {
            let p = Vec2::new(x, y);
            if x.abs().max(y.abs()) >= 200.0 && w.landscape.water_distance(p) > 150.0 {
                return p;
            }
            x += 40.0;
        }
        y += 40.0;
    }
    panic!("no dry spot");
}

/// The land round `at` replaced by a plane rising `slope` along +y.
fn plane(w: &mut World, at: Vec2, slope: f32) {
    w.landscape.reshape_for_test(move |p, h| {
        let d = p - at;
        if d.x.abs() > 400.0 || d.y.abs() > 400.0 {
            return h;
        }
        60.0 + slope * d.y.clamp(-350.0, 350.0)
    });
}

/// The pieces laid along the line `a`-`b` (the map's highway and trunk
/// line left out).
fn laid(w: &World, a: Vec2, b: Vec2) -> Vec<SegmentId> {
    w.roads.alive_segments().filter(|(id, _)| {
        let (p, q) = w.roads.segment_points(*id);
        let m = p.lerp(q, 0.5);
        crate::geom::project_on_segment(m, a, b).1.distance(m) < 1.0
    }).map(|(id, _)| id).collect()
}

fn natural(w: &World, p: Vec2) -> f32 {
    w.landscape.height(p).max(BANK_HEIGHT)
}

/// Harvey's flag (claude_flags line 135): a path across a mountainside cut
/// the hill up round it. Graded with 1-in-2.5 slopes, a bench on ground
/// steeper than that never met the land again. Now it is held: the path's
/// band at its height, the land just beyond it untouched.
#[test]
fn a_path_across_a_steep_hillside_lies_in_a_narrow_bench() {
    for (kind, lanes, slope) in [(RoadKind::Path, 0u8, 0.6f32), (RoadKind::Path, 0, 0.3), (RoadKind::Street, 1, 0.45)] {
        let mut w = hilly_world();
        let at = dry_spot(&w);
        plane(&mut w, at, slope);
        let pts = [at - Vec2::new(100.0, 0.0), at + Vec2::new(100.0, 0.0)];
        if kind == RoadKind::Path {
            w.add_path(&pts);
        } else {
            w.add_road(&pts, lanes, lanes);
        }
        let mut checked = 0;
        for sid in laid(&w, pts[0], pts[1]) {
            let seg = w.roads.segment(sid);
            assert_eq!(earthwork_hold(&w.landscape, &w.roads, &w.road_z, &w.road_inner, sid), Some(Hold::Bench), "{kind:?} on {slope}: held in a bench");
            let (a, b) = w.roads.segment_points(sid);
            let mid = a.lerp(b, 0.5);
            let z = w.road_height(sid, 0.5);
            assert!((z - natural(&w, mid)).abs() < 0.5, "{kind:?} on {slope}: on the land ({z} vs {})", natural(&w, mid));
            let (_, back) = held_band(seg, Hold::Bench);
            for s in [-1.0f32, 1.0] {
                let beyond = mid + Vec2::Y * s * (back + 6.0);
                let moved = (w.ground_height(beyond) - natural(&w, beyond)).abs();
                assert!(moved < 0.6, "{kind:?} on {slope}: the hill moved {moved:.1} m {} m off the way", back + 6.0);
                checked += 1;
            }
        }
        assert!(checked > 0);
    }
}

/// [r4-hillside] Town streets follow the land: a local street to 25%, an
/// avenue to 11%; highways and rail as before; a ramp off a deck keeps 7%.
#[test]
fn local_streets_climb_steeper_than_avenues_and_highways() {
    assert_eq!(land_grade(RoadKind::Street, 1, 1), LOCAL_STREET_GRADE);
    assert_eq!(land_grade(RoadKind::Street, 2, 2), AVENUE_GRADE);
    assert_eq!(land_grade(RoadKind::Street, 3, 0), STREET_GRADE);
    assert_eq!(land_grade(RoadKind::Path, 0, 0), PATH_GRADE);
    for (lanes, follows) in [(1u8, true), (2, false)] {
        let mut w = hilly_world();
        let at = dry_spot(&w);
        plane(&mut w, at, 0.2);
        // Straight up the fall line, 300 m: 60 m of climb.
        let (a0, b0) = (at - Vec2::new(0.0, 150.0), at + Vec2::new(0.0, 150.0));
        w.add_road(&[a0, b0], lanes, lanes);
        let mut worst = 0.0f32;
        let mut steepest = 0.0f32;
        let segs = laid(&w, a0, b0);
        assert!(!segs.is_empty());
        for sid in segs {
            let (a, b) = w.roads.segment_points(sid);
            let len = a.distance(b);
            let mut prev = w.road_height(sid, 0.0);
            for k in 1..=30 {
                let t = k as f32 / 30.0;
                let z = w.road_height(sid, t);
                steepest = steepest.max((z - prev).abs() / (len / 30.0));
                prev = z;
                worst = worst.max((z - natural(&w, a.lerp(b, t))).abs());
            }
        }
        let limit = land_grade(RoadKind::Street, lanes, lanes);
        assert!(steepest <= limit + 0.01, "{lanes}+{lanes}: climbs {steepest} past its {limit}");
        if follows {
            assert!(worst < 0.5, "a local street lies on a 20% hill ({worst:.1} m off it)");
        } else {
            assert!(worst > 2.0, "an avenue is eased onto cuttings and fills on a 20% hill ({worst:.1} m)");
        }
    }
}

/// [r4-hillside] The Road tool says when the land is steeper than the
/// kind takes, and offers a zig-zag whose legs each climb within it.
#[test]
fn the_tool_warns_past_each_kinds_limit_and_offers_a_switchback() {
    let mut w = hilly_world();
    let at = dry_spot(&w);
    plane(&mut w, at, 0.35);
    let (a, b) = (at - Vec2::new(0.0, 60.0), at + Vec2::new(0.0, 60.0));
    let warn = w.road_grade_warnings(&[a, b], &[], RoadKind::Street, 1, 1);
    assert!(warn.iter().any(|m| m.contains("steeper than a local street takes (25% at most)") && m.contains("switchback")), "{warn:?}");
    let avenue = w.road_grade_warnings(&[a, b], &[], RoadKind::Street, 2, 2);
    assert!(avenue.iter().any(|m| m.contains("steeper than an avenue takes (11% at most)")), "{avenue:?}");
    // A path climbs it in flights of steps: no warning.
    let path = w.road_grade_warnings(&[a, b], &[], RoadKind::Path, 0, 0);
    assert!(!path.iter().any(|m| m.contains("steeper than")), "{path:?}");
    // Along the contour a street is fine.
    let along = w.road_grade_warnings(&[at - Vec2::new(60.0, 0.0), at + Vec2::new(60.0, 0.0)], &[], RoadKind::Street, 1, 1);
    assert!(!along.iter().any(|m| m.contains("steeper than")), "{along:?}");
    let z = w.switchback(a, b, RoadKind::Street, 1, 1).expect("a zig-zag is offered");
    assert!(z.len() >= 3 && z[0] == a && z[z.len() - 1] == b, "{z:?}");
    for leg in z.windows(2) {
        let g = (natural(&w, leg[1]) - natural(&w, leg[0])).abs() / leg[0].distance(leg[1]);
        assert!(g <= LOCAL_STREET_GRADE + 0.01, "a leg climbs {g}");
    }
    assert!(w.switchback(at - Vec2::new(60.0, 0.0), at + Vec2::new(60.0, 0.0), RoadKind::Street, 1, 1).is_none(), "none needed along the contour");
    assert!(w.switchback(a, b, RoadKind::Path, 0, 0).is_none(), "a path takes 35% as it is");
}

/// [r4-hillside] Cars slow on a climb, buses and lorries more, and brake
/// down a steep descent; the lanes of a street up a hill know their climb.
#[test]
fn vehicles_slow_on_steep_streets() {
    use crate::traffic::{climb_factor, LaneKind};
    assert_eq!(climb_factor(0.02, false), 1.0);
    assert!(climb_factor(0.25, false) < 0.65 && climb_factor(0.25, false) > 0.5);
    assert!(climb_factor(0.25, true) < climb_factor(0.25, false));
    assert!(climb_factor(-0.25, false) < 1.0 && climb_factor(-0.25, false) >= 0.65);
    let mut w = hilly_world();
    let at = dry_spot(&w);
    plane(&mut w, at, 0.2);
    let (a0, b0) = (at - Vec2::new(0.0, 150.0), at + Vec2::new(0.0, 150.0));
    w.add_road(&[a0, b0], 1, 1);
    let mine = laid(&w, a0, b0);
    let (mut up, mut down) = (0, 0);
    for lane in &w.traffic.net.lanes {
        let LaneKind::Road { segment, .. } = lane.kind else { continue };
        if !mine.contains(&segment) {
            continue;
        }
        let north = lane.points[lane.points.len() - 1].y > lane.points[0].y;
        if north {
            assert!(lane.climb > 0.15, "uphill lane climbs {}", lane.climb);
            up += 1;
        } else {
            assert!(lane.climb < -0.15, "downhill lane {}", lane.climb);
            down += 1;
        }
    }
    assert!(up > 0 && down > 0);
}

/// [r4-hillside] Lots on a steep hillside are walled terraces: their
/// ground held at the pad, walls where it parts from the land, and the
/// hillside beyond them left as it was (graded, a terrace buried the lots
/// below it); a terrace higher than a wall's cap over the drop stands
/// on a bank below its wall.
#[test]
fn terraces_on_a_steep_hillside_are_walled_not_graded() {
    let mut w = hilly_world();
    let at = dry_spot(&w);
    plane(&mut w, at, 0.5);
    w.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
    let ids: Vec<usize> = w.parcels.in_circle(at, 70.0).into_iter().filter(|&i| w.parcels.parcels[i].street_frontage && w.parcel_refusal(i, Some(LandUse::Residential)).is_none()).collect();
    assert!(ids.len() >= 4, "parcels to zone ({})", ids.len());
    w.paint_parcels(&ids, Some(LandUse::Residential), None);
    w.implement();
    let terraced: Vec<LotId> = w.lots.iter().filter(|(id, l)| crate::landscape::walled_terrace(&w.landscape, l, w.ground.pads[id])).map(|(id, _)| id).collect();
    assert!(terraced.len() >= 2, "walled terraces ({})", terraced.len());
    let walls: Vec<_> = w.ground.retaining.iter().filter(|r| terraced.contains(&r.lot)).collect();
    assert!(walls.iter().any(|r| !r.holds_back && r.top - r.foot_a.max(r.foot_b) > 1.0), "a podium wall over the drop");
    assert!(walls.iter().any(|r| r.holds_back), "a wall holding the hill back");
    for id in &terraced {
        let l = &w.lots[*id];
        let pad = w.ground.pads[id];
        assert!((w.ground_height(l.center) - pad).abs() < 0.3, "the lot's ground is its pad");
        // The hillside 10 m past the lot's back edge (away from the street)
        // is the land, unless another lot or way is there.
        let back = l.center + (l.center - w.roads.nearest_where(l.center, |s| s.is_street()).unwrap().2).normalize_or_zero() * 10.0;
        // Past the lot's held margin and one cell of the raster.
        let dir = (back - l.center).normalize_or_zero();
        let reach = l.polygon.iter().map(|c| (*c - l.center).dot(dir)).fold(0.0f32, f32::max);
        let p = l.center + dir * (reach + 2.0 + crate::landscape::GROUND_CELL * 1.5);
        if w.lots.values().any(|o| o.contains(p)) || w.roads.segments_near(p, 12.0).len() > 0 {
            continue;
        }
        // Left as the land, unless the hill there stands so far over the
        // pad that it is cut back above the wall's cap (a rock face).
        // (any terrace's within reach: each holds its walls to the cap).
        let rock = terraced
            .iter()
            .map(|o| (crate::geom::distance_to_outline(p, &w.lots[*o].polygon), w.ground.pads[o]))
            .filter(|(edge, _)| *edge <= 2.0 + crate::landscape::BENCH_CAP_REACH)
            .map(|(edge, pad)| pad + crate::landscape::BENCH_WALL_MAX + (edge - 2.0).max(0.0) * crate::landscape::BENCH_ROCK_SLOPE)
            .fold(f32::INFINITY, f32::min);
        // And the bank under a podium higher than the cap.
        let bank = terraced
            .iter()
            .map(|o| (crate::geom::distance_to_outline(p, &w.lots[*o].polygon), w.ground.pads[o]))
            .filter(|(edge, _)| *edge <= 2.0 + crate::landscape::BENCH_CAP_REACH)
            .map(|(edge, pad)| pad - crate::landscape::BENCH_WALL_MAX - (edge - 2.0).max(0.0) * crate::landscape::BENCH_FILL_SLOPE)
            .fold(f32::NEG_INFINITY, f32::max);
        let want = natural(&w, p).min(rock).max(bank);
        let moved = (w.ground_height(p) - want).abs();
        // (A metre and a half: the grading rounds the bank's toe.)
        assert!(moved < 1.5, "the hillside behind a terrace moved {moved:.1} m from the land (or its rock face): ground {:.1} natural {:.1} rock {rock:.1} pad {pad:.1} at {p} lot {}", w.ground_height(p), natural(&w, p), l.center);
    }
}

/// [r4-hillside] The Zone tool says how steep a hillside is and what it
/// allows, with the numbers.
#[test]
fn zoning_says_how_steep_a_hillside_is_and_what_is_allowed() {
    let mut w = hilly_world();
    let at = dry_spot(&w);
    plane(&mut w, at, 1.1);
    w.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
    let ids: Vec<usize> = w.parcels.in_circle(at, 70.0).into_iter().filter(|&i| w.parcels.parcels[i].street_frontage).collect();
    assert!(!ids.is_empty());
    let mut said = 0;
    for &i in &ids {
        let Some(note) = w.parcel_ground_note(i) else { continue };
        assert!(note.contains('%'), "{note}");
        if w.parcels.parcels[i].hillside {
            assert!(note.contains("70%") && note.contains("180%"), "the limits: {note}");
        }
        said += 1;
    }
    assert!(said * 2 >= ids.len(), "most steep parcels say why ({said}/{})", ids.len());
    // Level ground says nothing.
    let mut flat = hilly_world();
    let at = dry_spot(&flat);
    plane(&mut flat, at, 0.0);
    flat.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
    for i in flat.parcels.in_circle(at, 70.0) {
        assert!(flat.parcel_ground_note(i).is_none());
    }
}

/// [r4-hillside] A bench's retaining walls stand at most `BENCH_WALL_MAX`
/// high: a street eased into a hillside much steeper than it may climb (a
/// 60% slope straight up) cuts deep, and past the cap the hill is cut back
/// as a rock face rather than held by a sheer wall 20 m and more high.
#[test]
fn bench_walls_are_never_taller_than_the_cap() {
    use crate::landscape::{BENCH_ROCK_SLOPE, BENCH_WALL_MAX};
    let mut w = hilly_world();
    let at = dry_spot(&w);
    plane(&mut w, at, 0.6);
    let (a, b) = (at - Vec2::new(0.0, 160.0), at + Vec2::new(0.0, 160.0));
    w.add_road(&[a, b], 1, 1);
    let mut deepest = 0.0f32;
    for sid in laid(&w, a, b) {
        let seg = w.roads.segment(sid);
        assert_eq!(earthwork_hold(&w.landscape, &w.roads, &w.road_z, &w.road_inner, sid), Some(Hold::Bench));
        let (p, q) = w.roads.segment_points(sid);
        let (_, back) = held_band(seg, Hold::Bench);
        for k in 1..10 {
            let t = k as f32 / 10.0;
            let c = p.lerp(q, t);
            let z = w.road_height(sid, t);
            deepest = deepest.max(natural(&w, c) - z);
            for s in [-1.0f32, 1.0] {
                for out in [1.0f32, 6.0, 12.0] {
                    let x = c + Vec2::X * s * (back + out);
                    let cap = z + BENCH_WALL_MAX + out * BENCH_ROCK_SLOPE + 1.0;
                    assert!(w.ground_height(x) <= cap, "ground {:.1} m over the way {out} m past its wall (cap {:.1})", w.ground_height(x) - z, cap - z);
                }
            }
        }
    }
    assert!(deepest > BENCH_WALL_MAX + 2.0, "a cut deeper than the cap to test ({deepest:.1} m)");
}

/// [r4-hillside] No terrace wall is taller than a bench's cap either.
#[test]
fn terrace_walls_are_never_taller_than_the_cap() {
    let mut w = hilly_world();
    let at = dry_spot(&w);
    plane(&mut w, at, 1.1);
    w.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
    let ids: Vec<usize> = w.parcels.in_circle(at, 70.0).into_iter().filter(|&i| w.parcels.parcels[i].street_frontage && w.parcel_refusal(i, Some(LandUse::Residential)).is_none()).collect();
    assert!(!ids.is_empty(), "terraceable parcels on a 110% hillside");
    w.paint_parcels(&ids, Some(LandUse::Residential), None);
    w.implement();
    assert!(!w.ground.retaining.is_empty());
    for r in &w.ground.retaining {
        for f in [r.foot_a, r.foot_b] {
            assert!((f - r.top).abs() <= crate::landscape::BENCH_WALL_MAX + 0.01, "a wall {:.1} m high", (f - r.top).abs());
        }
    }
}
