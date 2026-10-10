//! Load a save, lay a short street far from everything the player placed,
//! and report what the rebuild changed elsewhere: parcels, lots, grown
//! buildings, city buildings (lot outline, pad height, plant output).
//! Also times the first project check. Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example parcel_probe -- <save.ron> [x y dx dy]`
use nd_sim::plan::Gesture;
use nd_sim::roads::RoadKind;
use nd_sim::{Vec2, World};
use std::collections::BTreeMap;
use std::time::Instant;

fn round(p: &[Vec2]) -> Vec<(i32, i32)> {
    p.iter().map(|q| ((q.x * 100.0).round() as i32, (q.y * 100.0).round() as i32)).collect()
}

struct Snap {
    parcels: BTreeMap<(i32, i32), Vec<(i32, i32)>>,
    /// building -> (lot outline, floors, built tick, pad, output)
    buildings: BTreeMap<String, (Vec<(i32, i32)>, u8, u64, i32, i32, bool)>,
    at: BTreeMap<String, Vec2>,
}

fn snap(w: &World) -> Snap {
    let parcels = w.parcels.parcels.iter().map(|p| (p.key.center, round(&p.polygon))).collect();
    let mut buildings = BTreeMap::new();
    let mut at = BTreeMap::new();
    for (id, b) in w.buildings.iter() {
        let Some(l) = w.lots.get(b.lot) else { continue };
        let out = (w.plant_capacity_mw(b) * 100.0).round() as i32 + (w.power_site(b.lot, b.plant.unwrap_or(nd_sim::power::PowerPlantKind::Coal)).unwrap_or(0.0) * 1000.0).round() as i32 * 0;
        let pad = (w.lot_pad(b.lot) * 100.0).round() as i32;
        at.insert(format!("{:?} {}", id, b.name), l.center);
        buildings.insert(format!("{:?} {}", id, b.name), (round(&l.polygon), b.floors, b.built_tick, pad, out, w.is_city_building(id)));
    }
    Snap { parcels, buildings, at }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let t0 = Instant::now();
    let mut w = World::from_ron(&text).expect("load");
    println!("loaded in {:.2}s: {} parcels {} lots {} buildings", t0.elapsed().as_secs_f32(), w.parcels.len(), w.lots.len(), w.buildings.len());
    if args.get(2).map(String::as_str) == Some("--at") {
        let v: Vec<f32> = args[3..].iter().map(|s| s.parse().unwrap()).collect();
        for c in v.chunks(2) {
            let p = Vec2::new(c[0], c[1]);
            println!("at ({:.0}, {:.0}): {:?}", p.x, p.y, w.no_parcel_why(p));
            for i in w.parcels.in_circle(p, 25.0) {
                let q = &w.parcels.parcels[i];
                let (lo, hi) = nd_sim::geom::bounds(&q.polygon);
                println!("  parcel {i} c ({:.0},{:.0}) area {:.0} box {:.0}x{:.0} n{} path {} street {} rough {:?} water {:.0} lu {:?} lot {:?} refusals park {:?} com {:?} civic {:?}", q.center.x, q.center.y, q.area, hi.x-lo.x, hi.y-lo.y, q.polygon.len(), q.path_frontage, q.street_frontage, q.rough_why, q.water_frontage, q.land_use, q.lot, w.parcel_refusal(i, Some(nd_sim::LandUse::Park)), w.parcel_refusal(i, Some(nd_sim::LandUse::Commercial)), w.parcel_refusal(i, Some(nd_sim::LandUse::Civic)));
                if let Some(l) = q.lot.and_then(|l| w.lots.get(l)) {
                    println!("     lot {:?} area {:.0} landlocked {} car {:?} bld {:?}", l.land_use, l.area(), l.landlocked, l.car_segment, l.building.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()));
                }
                println!("     poly {:?}", q.polygon.iter().map(|v| (v.x.round(), v.y.round())).collect::<Vec<_>>());
            }
            for (sid, s) in w.roads.alive_segments() {
                let (a, b) = w.roads.segment_points(sid);
                let (_, on) = nd_sim::geom::project_on_segment(p, a, b);
                if on.distance(p) < 60.0 {
                    println!("  seg {:?} {:?} lanes {}/{} hw {:.1} bridge {} tunnel {} fronts_nothing {} ({:.0},{:.0})-({:.0},{:.0}) d {:.0}", sid, s.kind, s.lanes_fwd, s.lanes_bwd, s.corridor_half_width(), s.bridge, s.tunnel, s.fronts_nothing(), a.x, a.y, b.x, b.y, on.distance(p));
                }
            }
        }
        return;
    }
    for (id, b) in w.buildings.iter() {
        if !w.is_city_building(id) { continue; }
        let Some(l) = w.lots.get(b.lot) else { println!("city {} has no lot", b.name); continue };
        let at = nd_sim::zoning::corridor_over_at(&l.polygon, &w.roads);
        if let Some((sid, p)) = at { let sg = w.roads.segment(sid); println!("  over by {:?} {:?} lanes {}/{} hw {:.1} at ({:.1},{:.1}) inside {} d {:.2}", sid, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.corridor_half_width(), p.x, p.y, nd_sim::zoning::point_in_polygon(p, &l.polygon), nd_sim::zoning::distance_to_outline(p, &l.polygon)); }
        let over = at.is_some();
        let fronts = nd_sim::zoning::fronts_a_way(&l.polygon, &w.roads);
        if std::env::var_os("PROBE_LIST").is_some() { println!("citylist {} at ({:.0},{:.0}) area {:.0} mw {:.1}", b.name, l.center.x, l.center.y, l.area(), w.plant_capacity_mw(b)); }
        if over || !fronts { println!("city {} at ({:.0},{:.0}) over {over} fronts {fronts} poly {:?}", b.name, l.center.x, l.center.y, l.polygon.iter().map(|v| (v.x.round(), v.y.round())).collect::<Vec<_>>()); }
    }
    if std::env::var_os("PROBE_WARM").is_some() {
        let t = Instant::now();
        w.rebuild_from_plan();
        println!("warm rebuild (roads re-laid by today's rules) {:.3}s", t.elapsed().as_secs_f32());
        prof_table();
    }
    let before = snap(&w);
    // The road: given, else a stub off the dead end farthest from every city building.
    let (a, b) = if args.len() >= 6 {
        let v: Vec<f32> = args[2..6].iter().map(|s| s.parse().unwrap()).collect();
        (Vec2::new(v[0], v[1]), Vec2::new(v[0] + v[2], v[1] + v[3]))
    } else {
        let city: Vec<Vec2> = w.buildings.iter().filter(|(id, _)| w.is_city_building(*id)).filter_map(|(_, b)| w.lots.get(b.lot)).map(|l| l.center).collect();
        let mut best = None;
        for (i, n) in w.roads.nodes.iter().enumerate() {
            if n.segments.len() != 1 {
                continue;
            }
            let s = w.roads.segment(n.segments[0]);
            if !s.alive || !s.is_street() || s.fronts_nothing() || s.bridge {
                continue;
            }
            let far = city.iter().map(|c| c.distance(n.pos)).fold(f32::MAX, f32::min);
            if best.is_none_or(|(d, _, _)| far > d) {
                let other = if s.a.0 as usize == i { s.b } else { s.a };
                let dir = (n.pos - w.roads.node(other).pos).normalize_or_zero();
                best = Some((far, n.pos, dir));
            }
        }
        let (d, p, dir) = best.expect("a dead end");
        println!("stub off the dead end at ({:.0}, {:.0}), {:.0} m from the nearest city building", p.x, p.y, d);
        (p, p + dir * 50.0)
    };
    w.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
    prof_table();
    let t = Instant::now();
    let check = w.active_project_check();
    println!("first project_check {:.3}s refusal {:?}", t.elapsed().as_secs_f32(), check.refusal);
    prof_table();
    let t = Instant::now();
    let check2 = w.active_project_check();
    println!("second project_check {:.3}s refusal {:?}", t.elapsed().as_secs_f32(), check2.refusal);
    let t = Instant::now();
    let r = w.try_implement();
    println!("implement {:.3}s {:?}", t.elapsed().as_secs_f32(), r);
    let after = snap(&w);
    let mid = a.lerp(b, 0.5);
    let far = |c: (i32, i32)| Vec2::new(c.0 as f32, c.1 as f32) * 0.25;
    let gone: Vec<_> = before.parcels.keys().filter(|k| after.parcels.get(*k) != before.parcels.get(*k)).collect();
    let new: Vec<_> = after.parcels.keys().filter(|k| !before.parcels.contains_key(*k)).collect();
    let gone_far = gone.iter().filter(|k| far(***k).distance(mid) > 250.0).count();
    let maxd = gone.iter().map(|k| far(**k).distance(mid)).fold(0.0f32, f32::max);
    println!("parcels: {} -> {}; changed or gone {} (of which >250 m from the road {gone_far}, farthest {maxd:.0} m); new {}", before.parcels.len(), after.parcels.len(), gone.len(), new.len());
    for k in gone.iter().filter(|k| far(***k).distance(mid) > 250.0) { let c = far(**k); println!("  far parcel at ({:.0}, {:.0}) {:.0} m", c.x, c.y, c.distance(mid)); }
    let mut moved = 0;
    for (k, v) in &before.buildings {
        match after.buildings.get(k) {
            None => {
                println!("  LOST {k} {:.0} m away city={}", before.at[k].distance(mid), v.5);
                moved += 1;
            }
            Some(n) if n != v => {
                moved += 1;
                if v.5 || moved < 20 {
                    println!("  CHANGED {k} {:.0} m away city={} outline {} floors {}->{} built {}->{} pad {}->{} mw {}->{}", before.at[k].distance(mid), v.5, if n.0 == v.0 { "same" } else { "differs" }, v.1, n.1, v.2, n.2, v.3, n.3, v.4, n.4);
                }
            }
            _ => {}
        }
    }
    println!("buildings changed or lost: {moved} of {}", before.buildings.len());
}

#[cfg(feature = "profile")]
fn prof_table() {
    let mut rows: Vec<_> = nd_sim::prof::take().into_iter().collect();
    rows.sort_by(|a, b| b.1.total.total_cmp(&a.1.total));
    for (name, e) in rows {
        println!("  {:<20} {:>4} calls {:>8.1} ms", name, e.calls, e.total * 1000.0);
    }
}
#[cfg(not(feature = "profile"))]
fn prof_table() {}
