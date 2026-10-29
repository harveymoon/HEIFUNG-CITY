//! [r4-hillside] How much ground the streets and paths move on a hillside.
//! Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example hillside_probe -- <mode> [args]`
//! - `save <save.ron> [x y r]`: every at-grade street and path of a save
//!   (or those near a point), measured.
//! - `fresh [seed]`: a new Islands map at relief 1; streets, avenues and
//!   paths laid at test sites on its hillsides (down the fall line, across
//!   it, and on the diagonal), then measured.
//! - `ways <save.ron> x y r`: the ways near a point, with their heights.
//! - `grid <save.ron> x y r [step]`: the land's heights round a point.
//! - `addpath|addstreet <save.ron> x y x y ...`: lay a path (street) on a
//!   copy of a save and measure it.
//!
//! "Moved" is the built ground against the natural land, sampled every 2 m
//! out to 40 m from each way (each sample counted for the nearest way only,
//! samples on lots left out; for `addpath`, against the ground before it
//! was laid): `vol/m` is m³ of cut and fill per metre of
//! way, `band` the mean width (m) of ground moved more than 0.25 m beyond
//! the way's own corridor, `max` the deepest cut or highest fill (m).
use nd_sim::roads::{RoadKind, SegmentId};
use nd_sim::{Vec2, World};

#[derive(Default, Clone)]
struct Tally {
    ways: usize,
    length: f32,
    volume: f32,
    band_area: f32,
    worst: f32,
}

fn class(w: &World, sid: SegmentId) -> &'static str {
    let s = w.roads.segment(sid);
    match s.kind {
        RoadKind::Path => "path",
        RoadKind::Rail => "rail",
        RoadKind::Street if s.is_motorway() => "motorway",
        RoadKind::Street if s.lanes() >= 4 => "avenue",
        RoadKind::Street => "street",
    }
}

fn measured(w: &World, sid: SegmentId) -> bool {
    let s = w.roads.segment(sid);
    !(s.bridge || s.viaduct || s.tunnel || s.is_rail() || s.is_motorway()) && w.roads.node(s.a).level == 0.0 && w.roads.node(s.b).level == 0.0
}

/// Measure the ways `sids` (by class, and each one alone).
fn measure(w: &World, base: Option<&World>, sids: &[SegmentId], per_way: bool) -> std::collections::BTreeMap<&'static str, Tally> {
    const STEP: f32 = 2.0;
    const REACH: f32 = 40.0;
    let mut by: std::collections::BTreeMap<&'static str, Tally> = Default::default();
    let lots: Vec<(Vec2, Vec2, &Vec<Vec2>)> = w.lots.iter().map(|(_, l)| {
        let (lo, hi) = nd_sim::geom::bounds(&l.polygon);
        (lo, hi, &l.polygon)
    }).collect();
    let on_lot = |p: Vec2| lots.iter().any(|(lo, hi, poly)| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y && nd_sim::geom::point_in_polygon(p, poly));
    for &sid in sids {
        let s = w.roads.segment(sid);
        let (a, b) = w.roads.segment_points(sid);
        let len = a.distance(b);
        if len < 1.0 {
            continue;
        }
        let core = s.corridor_half_width();
        let (lo, hi) = (a.min(b) - Vec2::splat(REACH), a.max(b) + Vec2::splat(REACH));
        let mut t = Tally { ways: 1, length: len, ..Default::default() };
        let mut x = (lo.x / STEP).floor() * STEP;
        while x <= hi.x {
            let mut y = (lo.y / STEP).floor() * STEP;
            while y <= hi.y {
                let p = Vec2::new(x, y);
                y += STEP;
                let (tt, q) = nd_sim::geom::project_on_segment(p, a, b);
                let d = q.distance(p);
                if d > REACH || tt <= 0.0 || tt >= 1.0 {
                    continue;
                }
                // Nearest way only.
                let nearer = w.roads.segments_near(p, d).into_iter().any(|o| {
                    if o == sid {
                        return false;
                    }
                    let (oa, ob) = w.roads.segment_points(o);
                    nd_sim::geom::project_on_segment(p, oa, ob).1.distance(p) < d - 1e-3
                });
                if nearer || on_lot(p) {
                    continue;
                }
                let natural = w.landscape.height(p).max(nd_sim::landscape::BANK_HEIGHT);
                if natural <= nd_sim::landscape::BANK_HEIGHT + 0.01 {
                    continue;
                }
                // Against the ground before the way was laid, if given.
                let dz = w.ground_height(p) - base.map_or(natural, |b| b.ground_height(p));
                t.volume += dz.abs() * STEP * STEP;
                if d > core + 0.5 && dz.abs() > 0.25 {
                    t.band_area += STEP * STEP;
                }
                if dz.abs() > t.worst.abs() {
                    t.worst = dz;
                }
            }
            x += STEP;
        }
        if per_way {
            let (za, zb) = (w.road_height(sid, 0.0), w.road_height(sid, 1.0));
            let (ga, gb) = (w.landscape.height(a), w.landscape.height(b));
            println!(
                "  way {:>5} {:<7} ({:>6.0},{:>6.0})->({:>6.0},{:>6.0}) {:>4.0} m  land {:>5.1}%  road {:>5.1}%  vol/m {:>6.1}  band {:>5.1} m  max {:>+6.1} m",
                sid.0,
                class(w, sid),
                a.x,
                a.y,
                b.x,
                b.y,
                len,
                (gb - ga).abs() / len * 100.0,
                (zb - za).abs() / len * 100.0,
                t.volume / len,
                t.band_area / len,
                t.worst
            );
        }
        let e = by.entry(class(w, sid)).or_default();
        e.ways += 1;
        e.length += t.length;
        e.volume += t.volume;
        e.band_area += t.band_area;
        if t.worst.abs() > e.worst.abs() {
            e.worst = t.worst;
        }
    }
    by
}

fn print(by: &std::collections::BTreeMap<&'static str, Tally>) {
    for (k, t) in by {
        println!(
            "{k:<8} ways {:>4}  length {:>6.0} m  moved {:>8.0} m³  vol/m {:>6.1}  band {:>5.1} m  worst {:>+6.1} m",
            t.ways,
            t.length,
            t.volume,
            t.volume / t.length.max(1.0),
            t.band_area / t.length.max(1.0),
            t.worst
        );
    }
}

/// Test sites on the map's hillsides: dry ground at least 8 m up whose
/// slope over 60 m is between 15% and 60%, 160 m or more apart.
fn sites(w: &World) -> Vec<(Vec2, Vec2, f32)> {
    let land = &w.landscape;
    let mut out: Vec<(Vec2, Vec2, f32)> = Vec::new();
    let r = 900.0;
    let mut y = -r;
    while y <= r {
        let mut x = -r;
        while x <= r {
            let p = Vec2::new(x, y);
            x += 40.0;
            let h = land.height(p);
            if h < 8.0 || land.water_distance(p) < 30.0 {
                continue;
            }
            let e = 30.0;
            let gx = (land.height(p + Vec2::X * e) - land.height(p - Vec2::X * e)) / (2.0 * e);
            let gy = (land.height(p + Vec2::Y * e) - land.height(p - Vec2::Y * e)) / (2.0 * e);
            let g = Vec2::new(gx, gy);
            let slope = g.length();
            if !(0.15..=0.6).contains(&slope) {
                continue;
            }
            if out.iter().any(|(q, _, _)| q.distance(p) < 160.0) {
                continue;
            }
            out.push((p, g / slope, slope));
        }
        y += 40.0;
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("fresh");
    match mode {
        "save" | "ways" => {
            let w = World::read_save(&args[2]).expect("load");
            let filt = (args.len() >= 6).then(|| (Vec2::new(args[3].parse().unwrap(), args[4].parse().unwrap()), args[5].parse::<f32>().unwrap()));
            let sids: Vec<SegmentId> = w
                .roads
                .alive_segments()
                .filter(|(id, _)| measured(&w, *id))
                .filter(|(id, _)| {
                    filt.is_none_or(|(p, r)| {
                        let (a, b) = w.roads.segment_points(*id);
                        nd_sim::geom::project_on_segment(p, a, b).1.distance(p) <= r
                    })
                })
                .map(|(id, _)| id)
                .collect();
            if mode == "ways" {
                for &sid in &sids {
                    let s = w.roads.segment(sid);
                    let (a, b) = w.roads.segment_points(sid);
                    println!(
                        "seg {} {:?} {}+{} ({:.1},{:.1})->({:.1},{:.1}) z {:.1}/{:.1} land {:.1}/{:.1} inner {:?}",
                        sid.0,
                        s.kind,
                        s.lanes_fwd,
                        s.lanes_bwd,
                        a.x,
                        a.y,
                        b.x,
                        b.y,
                        w.road_height(sid, 0.0),
                        w.road_height(sid, 1.0),
                        w.landscape.height(a),
                        w.landscape.height(b),
                        w.road_inner.get(sid.0 as usize).map(|v| v.iter().map(|(t, z)| format!("{t:.2}:{z:.1}/{:.1}", w.landscape.height(a.lerp(b, *t)))).collect::<Vec<_>>())
                    );
                }
            }
            print(&measure(&w, None, &sids, filt.is_some()));
        }
        "grid" => {
            // The land round a point: heights every `step` m.
            let w = World::read_save(&args[2]).expect("load");
            let (c, r, step) = (Vec2::new(args[3].parse().unwrap(), args[4].parse().unwrap()), args[5].parse::<f32>().unwrap(), args.get(6).and_then(|s| s.parse::<f32>().ok()).unwrap_or(20.0));
            let mut y = c.y + r;
            while y >= c.y - r {
                let mut row = format!("{y:>6.0} ");
                let mut x = c.x - r;
                while x <= c.x + r {
                    row.push_str(&format!("{:>6.1}", w.landscape.height(Vec2::new(x, y))));
                    x += step;
                }
                println!("{row}");
                y -= step;
            }
        }
        "addpath" | "addstreet" => {
            // Lay a way along the points given on a copy of a save, and
            // measure it (the ground it moves, against before).
            let mut w = World::read_save(&args[2]).expect("load");
            w.economy.treasury = 1e9;
            let nums: Vec<f32> = args[3..].iter().map(|s| s.parse().unwrap()).collect();
            let pts: Vec<Vec2> = nums.chunks(2).map(|c| Vec2::new(c[0], c[1])).collect();
            let kind = if mode == "addpath" { RoadKind::Path } else { RoadKind::Street };
            let lanes = if kind == RoadKind::Path { 0 } else { 1 };
            if let Some(why) = w.road_refusal(&pts, &[], kind, lanes, lanes) {
                println!("refused: {why}");
                return;
            }
            for m in w.road_grade_warnings(&pts, &[], kind, lanes, lanes) {
                println!("warning: {m}");
            }
            let n0 = w.roads.alive_segments().count();
            let base = w.clone();
            if kind == RoadKind::Path {
                w.add_path(&pts);
            } else {
                w.add_road(&pts, lanes, lanes);
            }
            // The new pieces: on the drawn line.
            let sids: Vec<SegmentId> = w
                .roads
                .alive_segments()
                .filter(|(id, s)| {
                    let (a, b) = w.roads.segment_points(*id);
                    s.kind == kind && pts.windows(2).any(|l| nd_sim::geom::project_on_segment(a.lerp(b, 0.5), l[0], l[1]).1.distance(a.lerp(b, 0.5)) < 1.0)
                })
                .map(|(id, _)| id)
                .collect();
            println!("{} pieces laid ({} segments before)", sids.len(), n0);
            if std::env::var("HILLSIDE_DEBUG").is_ok() {
                for &sid in &sids {
                    let (a, b) = w.roads.segment_points(sid);
                    println!(
                        "seg {} hold {:?} z {:.1}/{:.1} land {:.1}/{:.1} inner {:?}",
                        sid.0,
                        nd_sim::landscape::earthwork_hold(&w.landscape, &w.roads, &w.road_z, &w.road_inner, sid),
                        w.road_height(sid, 0.0),
                        w.road_height(sid, 1.0),
                        w.landscape.height(a),
                        w.landscape.height(b),
                        w.road_inner.get(sid.0 as usize).map(|v| v.iter().map(|(t, z)| format!("{t:.2}:{z:.1}/{:.1}", w.landscape.height(a.lerp(b, *t)))).collect::<Vec<_>>())
                    );
                }
            }
            print(&measure(&w, Some(&base), &sids, true));
        }
        _ => {
            let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(7);
            let mut land = nd_sim::LandscapeParams::preset(nd_sim::landscape::LandscapePreset::Islands, seed);
            land.relief = 1.0;
            let mut w = World::new(nd_sim::WorldConfig { landscape: land, ..Default::default() });
            w.economy.treasury = 1e9;
            let sites = sites(&w);
            println!("{} hillside sites", sites.len());
            // Kind, lanes, angle from the fall line (degrees).
            let plans: [(&str, RoadKind, u8, f32); 8] = [
                ("path up", RoadKind::Path, 0, 0.0),
                ("path diag", RoadKind::Path, 0, 45.0),
                ("street up", RoadKind::Street, 1, 0.0),
                ("street diag", RoadKind::Street, 1, 45.0),
                ("street across", RoadKind::Street, 1, 90.0),
                ("avenue diag", RoadKind::Street, 2, 60.0),
                ("path across", RoadKind::Path, 0, 90.0),
                ("street steep", RoadKind::Street, 1, 25.0),
            ];
            let mut refused: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            for (k, (p, fall, slope)) in sites.iter().enumerate() {
                let (label, kind, lanes, angle) = plans[k % plans.len()];
                let dir = Vec2::from_angle(angle.to_radians()).rotate(*fall);
                let half = if kind == RoadKind::Path { 50.0 } else { 70.0 };
                let pts = vec![*p - dir * half, *p + dir * half];
                let e = refused.entry(label).or_default();
                e.1 += 1;
                if let Some(why) = w.road_refusal(&pts, &[], kind, lanes, lanes) {
                    e.0 += 1;
                    println!("site {k} {label} slope {:.0}%: refused: {why}", slope * 100.0);
                    continue;
                }
                let warn = w.road_grade_warnings(&pts, &[], kind, lanes, lanes);
                if kind == RoadKind::Path {
                    w.add_path(&pts);
                } else {
                    w.add_road(&pts, lanes, lanes);
                }
                println!("site {k} {label} slope {:.0}% at ({:.0},{:.0}){}", slope * 100.0, p.x, p.y, if warn.is_empty() { String::new() } else { format!(" warnings: {}", warn.join("; ")) });
            }
            let sids: Vec<SegmentId> = w.roads.alive_segments().filter(|(id, _)| measured(&w, *id)).map(|(id, _)| id).collect();
            print(&measure(&w, None, &sids, true));
            for (k, (n, of)) in &refused {
                println!("{k:<14} refused {n} of {of}");
            }
        }
    }
}
