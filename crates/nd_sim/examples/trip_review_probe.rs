//! [trip-review] Probe: how cyclists (and walkers) move through junctions.
//!
//! `paths <save> <ticks>`: runs the city and, for every bike ride (and walk)
//! that sets off, measures its planned path through each junction it
//! passes (a road node of three or more arms): the net turn, the sum of
//! every turn on the way through and the "excess" (the sum less the net:
//! 0 for a clean turn, large for a zig-zag), sideways jogs (a step across
//! the way of more than 1.5 m between two vertices less than 3 m apart
//! along it) and turns that go back on themselves. The worst crossings are
//! dumped with their arms.
//!
//! `trace <save> <ticks> <out.csv> [id ...]`: logs every tick the position,
//! heading, path index and activity of the given citizens (by their slot
//! key's `{:?}` text), or of a handful picked at the start (cyclists,
//! walkers, a driver) when none are given.
//!
//! cargo run --profile fast -p nd_sim --example trip_review_probe -- paths saves/autosave.ron 240

use nd_sim::citizens::{Activity, CitizenId, TravelMode};
use nd_sim::roads::{NodeId, RoadKind};
use nd_sim::{Vec2, World};
use std::collections::{HashMap, HashSet};
use std::io::Write;

/// Radius round a junction's centre inside which the path is measured.
const R: f32 = 22.0;

fn angle_between(a: Vec2, b: Vec2) -> f32 {
    a.perp_dot(b).atan2(a.dot(b)).to_degrees()
}

/// The polyline clipped to the circle round `c` of radius `r`: the first
/// stretch inside it (with entry and exit points).
fn clip(path: &[Vec2], c: Vec2, r: f32) -> Option<Vec<Vec2>> {
    let mut out: Vec<Vec2> = Vec::new();
    let mut inside = false;
    for i in 1..path.len() {
        let (p, q) = (path[i - 1], path[i]);
        let samples = (p.distance(q) / 0.5).ceil().max(1.0) as usize;
        for k in 0..=samples {
            let x = p.lerp(q, k as f32 / samples as f32);
            let ins = x.distance(c) <= r;
            if ins && !inside {
                inside = true;
                out.push(x);
            } else if ins && k == samples {
                out.push(x);
            } else if !ins && inside {
                out.push(x);
                return Some(out);
            }
        }
    }
    (out.len() >= 2).then_some(out)
}

#[derive(Default)]
struct Junction {
    net: f32,
    sum: f32,
    reversals: u32,
    jog: f32,
}

/// Turns of the clipped polyline (vertices closer than 0.3 m merged).
fn measure(pts: &[Vec2]) -> Junction {
    let mut v: Vec<Vec2> = Vec::new();
    for &p in pts {
        if v.last().is_none_or(|l: &Vec2| l.distance(p) > 0.3) {
            v.push(p);
        }
    }
    let mut j = Junction::default();
    if v.len() < 2 {
        return j;
    }
    let d0 = (v[1] - v[0]).normalize_or_zero();
    let dn = (v[v.len() - 1] - v[v.len() - 2]).normalize_or_zero();
    j.net = angle_between(d0, dn);
    let mut last_sign = 0.0f32;
    for i in 1..v.len() - 1 {
        let a = (v[i] - v[i - 1]).normalize_or_zero();
        let b = (v[i + 1] - v[i]).normalize_or_zero();
        let t = angle_between(a, b);
        j.sum += t.abs();
        if t.abs() > 8.0 {
            if last_sign != 0.0 && t.signum() != last_sign {
                j.reversals += 1;
            }
            last_sign = t.signum();
        }
        // A short leg that steps sideways off the way in.
        let leg = v[i + 1] - v[i];
        if leg.length() < 3.0 {
            j.jog = j.jog.max(leg.perp_dot(d0).abs().min(leg.length()));
        }
    }
    j
}

fn arms(w: &World, n: NodeId) -> String {
    let at = w.roads.node(n).pos;
    let mut s = Vec::new();
    for (m, sid) in w.roads.neighbors(n) {
        let seg = w.roads.segment(sid);
        let d = w.roads.node(m).pos - at;
        s.push(format!("{:?}{}+{} hw{:.1} {:.0}deg {:.0}m", seg.kind, seg.lanes_fwd, seg.lanes_bwd, seg.half_width(), d.y.atan2(d.x).to_degrees(), d.length()));
    }
    s.join(" | ")
}

fn junctions(w: &World) -> Vec<(NodeId, Vec2)> {
    w.roads
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| !n.segments.is_empty() && w.roads.neighbors(NodeId(*i as u32)).filter(|(_, s)| w.roads.segment(*s).kind != RoadKind::Rail).count() >= 3)
        .map(|(i, n)| (NodeId(i as u32), n.pos))
        .collect()
}

/// Along the whole path, resampled every metre: wiggles (a turn of more
/// than 25 degrees one way within 3 m, then more than 25 degrees back within
/// the next 5 m) and back-steps (heading more than 100 degrees off the way
/// it went 4 m before, and back within 60 degrees of it 4 m after: forward,
/// back and on again). Stretches near raised walk nodes (stairs) are left
/// out. Returns (metres, wiggles, back-steps).
fn wiggles(w: &World, path: &[Vec2]) -> (f32, u32, u32) {
    let mut pts: Vec<Vec2> = Vec::new();
    for i in 1..path.len() {
        let (p, q) = (path[i - 1], path[i]);
        let n = p.distance(q).ceil().max(1.0) as usize;
        for k in 0..n {
            pts.push(p.lerp(q, k as f32 / n as f32));
        }
    }
    if let Some(l) = path.last() {
        pts.push(*l);
    }
    let raised: Vec<bool> = path.iter().map(|p| w.walk.level_at_node(*p) > 0.0).collect();
    let near_raised = |x: Vec2| path.iter().zip(&raised).any(|(p, r)| *r && p.distance(x) < 12.0);
    let dirs: Vec<Vec2> = pts.windows(2).map(|w| (w[1] - w[0]).normalize_or_zero()).collect();
    let metres = dirs.len() as f32;
    let (mut wig, mut back) = (0u32, 0u32);
    let mut i = 4;
    while i + 8 < dirs.len() {
        if dirs[i] == Vec2::ZERO || dirs[i - 4] == Vec2::ZERO || dirs[i + 3] == Vec2::ZERO || dirs[i + 8] == Vec2::ZERO {
            i += 1;
            continue;
        }
        let a = angle_between(dirs[i], dirs[i + 3]);
        let b = angle_between(dirs[i + 3], dirs[i + 8]);
        let bs = angle_between(dirs[i - 4], dirs[i]).abs() > 100.0 && angle_between(dirs[i - 4], dirs[(i + 4).min(dirs.len() - 1)]).abs() < 60.0;
        let wg = a.abs() > 25.0 && b.abs() > 25.0 && a.signum() != b.signum();
        if (bs || wg) && !near_raised(pts[i]) {
            if bs {
                back += 1;
            } else {
                wig += 1;
            }
            i += 8;
            continue;
        }
        i += 1;
    }
    (metres, wig, back)
}

#[derive(Default)]
struct Stats {
    metres: f32,
    wiggles: u32,
    backs: u32,
    crossings: u32,
    excess_over_20: u32,
    reversals: u32,
    jogs: u32,
    excess_total: f32,
    stairs: u32,
    worst: Vec<(f32, String)>,
}

fn paths(w: &mut World, ticks: u64) {
    let mut minds = nd_minds::RuleBasedMind::default();
    let js = junctions(w);
    // A coarse grid of junctions.
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (k, (_, p)) in js.iter().enumerate() {
        grid.entry(((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32)).or_default().push(k);
    }
    let mut seen: HashSet<(CitizenId, usize, i32, i32)> = HashSet::new();
    let mut stats: HashMap<&'static str, Stats> = HashMap::new();
    let end = w.clock.tick + ticks;
    while w.clock.tick < end {
        w.tick(&mut minds);
        w.drain_events();
        for (id, c) in w.citizens.iter() {
            let Activity::Traveling { path, idx, .. } = &c.activity else { continue };
            if *idx > 1 || path.len() < 3 {
                continue;
            }
            let last = path[path.len() - 1];
            if !seen.insert((id, path.len(), last.x as i32, last.y as i32)) {
                continue;
            }
            let kind = match c.mode {
                TravelMode::Bike => "bike",
                TravelMode::Walk => "walk",
            };
            let st = stats.entry(kind).or_default();
            let (m, wg, bk) = wiggles(w, path);
            st.metres += m;
            st.wiggles += wg;
            st.backs += bk;
            // Junctions near any vertex.
            let mut near: HashSet<usize> = HashSet::new();
            for p in path.iter() {
                let (gx, gy) = ((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        if let Some(v) = grid.get(&(gx + dx, gy + dy)) {
                            for &k in v {
                                if js[k].1.distance(*p) < 14.0 {
                                    near.insert(k);
                                }
                            }
                        }
                    }
                }
            }
            for k in near {
                let (n, at) = js[k];
                // Up and down a footbridge's stairs the bike is wheeled
                // (a spiral's turns, a switchback): not ridden, apart.
                if path.iter().any(|p| p.distance(at) < R && w.walk.level_at_node(*p) > 0.0) {
                    st.stairs += 1;
                    continue;
                }
                let Some(pts) = clip(path, at, R) else { continue };
                let j = measure(&pts);
                st.crossings += 1;
                let excess = j.sum - j.net.abs();
                st.excess_total += excess;
                if excess > 20.0 {
                    st.excess_over_20 += 1;
                }
                if j.reversals > 0 {
                    st.reversals += 1;
                }
                if j.jog > 1.5 {
                    st.jogs += 1;
                }
                if excess > 20.0 && st.worst.len() < 400 {
                    let v: Vec<String> = pts.iter().map(|p| format!("({:.1},{:.1})", p.x - at.x, p.y - at.y)).collect();
                    st.worst.push((excess, format!("{id:?} node {} at ({:.0},{:.0}) net {:.0} sum {:.0} rev {} jog {:.1}\n      arms: {}\n      path rel: {}", n.0, at.x, at.y, j.net, j.sum, j.reversals, j.jog, arms(w, n), v.join(" "))));
                }
            }
        }
    }
    for (kind, st) in &mut stats {
        println!(
            "{kind}: {} junction crossings ({} more by footbridge stairs, left out), excess turn > 20deg: {} ({:.1}%), turn reversals: {} ({:.1}%), side jogs > 1.5 m: {} ({:.1}%), mean excess {:.1} deg",
            st.crossings,
            st.stairs,
            st.excess_over_20,
            100.0 * st.excess_over_20 as f32 / st.crossings.max(1) as f32,
            st.reversals,
            100.0 * st.reversals as f32 / st.crossings.max(1) as f32,
            st.jogs,
            100.0 * st.jogs as f32 / st.crossings.max(1) as f32,
            st.excess_total / st.crossings.max(1) as f32
        );
        println!("  {kind}: {:.0} km of paths, {} wiggles ({:.2}/km), {} back-steps ({:.2}/km)", st.metres / 1000.0, st.wiggles, st.wiggles as f32 / (st.metres / 1000.0).max(0.001), st.backs, st.backs as f32 / (st.metres / 1000.0).max(0.001));
        st.worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        let show: usize = std::env::var("SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
        for (e, s) in st.worst.iter().step_by((st.worst.len() / show.max(1)).max(1)).take(show) {
            println!("  excess {e:.0}: {s}");
        }
    }
}

fn trace(w: &mut World, ticks: u64, out: &str, ids: &[String]) {
    let mut minds = nd_minds::RuleBasedMind::default();
    let mut chosen: Vec<CitizenId> = w.citizens.iter().filter(|(id, _)| ids.contains(&format!("{id:?}"))).map(|(id, _)| id).collect();
    // `driver` among the ids: someone at the wheel now, far from the rest.
    if ids.iter().any(|s| s == "driver") {
        if let Some((id, _)) = w.citizens.iter().find(|(_, c)| matches!(c.activity, Activity::Driving { .. }) && c.home.is_some() && c.work.is_some()) {
            chosen.push(id);
        }
    }
    if chosen.is_empty() {
        // Owners of bikes who live in different places, people without,
        // someone with a car.
        let mut bikes: Vec<CitizenId> = Vec::new();
        let mut walkers: Vec<CitizenId> = Vec::new();
        let mut drivers: Vec<CitizenId> = Vec::new();
        let mut homes: Vec<Vec2> = Vec::new();
        let far = |homes: &Vec<Vec2>, p: Vec2| homes.iter().all(|h| h.distance(p) > 500.0);
        for (id, c) in w.citizens.iter() {
            if c.home.is_none() || c.work.is_none() {
                continue;
            }
            if c.owns_car && !c.owns_bike && drivers.is_empty() && far(&homes, c.pos) {
                drivers.push(id);
                homes.push(c.pos);
            } else if c.owns_bike && !c.owns_car && bikes.len() < 4 && far(&homes, c.pos) {
                bikes.push(id);
                homes.push(c.pos);
            } else if !c.owns_bike && !c.owns_car && walkers.len() < 3 && far(&homes, c.pos) {
                walkers.push(id);
                homes.push(c.pos);
            }
        }
        chosen.extend(bikes);
        chosen.extend(walkers);
        chosen.extend(drivers);
    }
    // Each planned path once, when it is set: `<out>.paths.txt`.
    let mut pf = std::fs::File::create(format!("{out}.paths.txt")).expect("paths");
    let mut last_key: HashMap<CitizenId, (usize, i32, i32)> = HashMap::new();
    let mut f = std::fs::File::create(out).expect("csv");
    writeln!(f, "tick,id,x,y,heading,mode,act,idx,len,inside").unwrap();
    let end = w.clock.tick + ticks;
    while w.clock.tick < end {
        w.tick(&mut minds);
        w.drain_events();
        for &id in &chosen {
            let Some(c) = w.citizens.get(id) else { continue };
            if let Activity::Traveling { path, purpose, .. } = &c.activity {
                if let Some(l) = path.last() {
                    let key = (path.len(), l.x as i32, l.y as i32);
                    if last_key.get(&id) != Some(&key) {
                        last_key.insert(id, key);
                        let pts: Vec<String> = path.iter().map(|p| format!("{:.1},{:.1}", p.x, p.y)).collect();
                        writeln!(pf, "{} {id:?} {:?} {purpose:?} from {:.1},{:.1} path {}", w.clock.tick, c.mode, c.prev_pos.x, c.prev_pos.y, pts.join(" ")).unwrap();
                    }
                }
            }
            let (act, idx, len) = match &c.activity {
                Activity::Traveling { path, idx, purpose, .. } => (format!("travel-{purpose:?}"), *idx, path.len()),
                Activity::Driving { .. } => ("driving".to_string(), 0, 0),
                a => (format!("{a:?}").chars().take_while(|c| c.is_alphanumeric()).collect(), 0, 0),
            };
            writeln!(f, "{},{:?},{:.2},{:.2},{:.3},{:?},{},{},{},{}", w.clock.tick, id, c.pos.x, c.pos.y, c.heading, c.mode, act, idx, len, c.inside.is_some()).unwrap();
        }
    }
    for id in &chosen {
        println!("traced {id:?} {}", w.citizens.get(*id).map_or("", |c| c.name.as_str()));
    }
}

/// `spots <save> <ticks> <out> x,y ...`: every new bike path passing
/// within 25 m of one of the spots, written as the trace's paths are (for
/// plotting the same junctions before and after a change).
fn spots(w: &mut World, ticks: u64, out: &str, at: &[String]) {
    let mut minds = nd_minds::RuleBasedMind::default();
    let at: Vec<Vec2> = at.iter().filter_map(|s| s.split_once(',')).map(|(x, y)| Vec2::new(x.parse().unwrap(), y.parse().unwrap())).collect();
    let mut f = std::fs::File::create(out).expect("spots");
    let mut seen: HashSet<(CitizenId, usize)> = HashSet::new();
    let end = w.clock.tick + ticks;
    while w.clock.tick < end {
        w.tick(&mut minds);
        w.drain_events();
        for (id, c) in w.citizens.iter() {
            let Activity::Traveling { path, idx, purpose, .. } = &c.activity else { continue };
            if c.mode != TravelMode::Bike || *idx > 1 || !seen.insert((id, path.len())) {
                continue;
            }
            for (k, p) in at.iter().enumerate() {
                if path.iter().any(|v| v.distance(*p) < 25.0) {
                    let pts: Vec<String> = path.iter().map(|p| format!("{:.1},{:.1}", p.x, p.y)).collect();
                    writeln!(f, "{} spot{k} {:?} {purpose:?} from {:.1},{:.1} path {}", w.clock.tick, c.mode, c.prev_pos.x, c.prev_pos.y, pts.join(" ")).unwrap();
                }
            }
        }
    }
}

/// Road nodes and segments within 40 m of (x, y), relative to it.
fn near(w: &World, x: f32, y: f32) {
    let c = Vec2::new(x, y);
    for (i, n) in w.roads.nodes.iter().enumerate() {
        if n.pos.distance(c) > 40.0 || n.segments.is_empty() {
            continue;
        }
        let rel = n.pos - c;
        println!("node {i} ({:.1},{:.1}) level {:.1}: {}", rel.x, rel.y, n.level, arms(w, NodeId(i as u32)));
        for (m, sid) in w.roads.neighbors(NodeId(i as u32)) {
            let r = w.roads.node(m).pos - c;
            let s = w.roads.segment(sid);
            println!("    seg {} -> node {} ({:.1},{:.1}) a={} b={}", sid.0, m.0, r.x, r.y, s.a.0, s.b.0);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).cloned().unwrap_or_default();
    let src = args.get(2).cloned().expect("save path");
    let ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(240);
    let t0 = std::time::Instant::now();
    let mut w = World::read_save(&src).expect("load");
    println!("loaded in {:.1}s at tick {} ({} citizens)", t0.elapsed().as_secs_f32(), w.clock.tick, w.citizens.len());
    match cmd.as_str() {
        "paths" => paths(&mut w, ticks),
        "trace" => trace(&mut w, ticks, args.get(4).map(String::as_str).unwrap_or("trace.csv"), &args[5.min(args.len())..]),
        "roads" => {
            // Every live way as a line, the walk graph's crossings and footbridges.
            let mut f = std::fs::File::create(args.get(3).map(String::as_str).unwrap_or("roads.csv")).expect("csv");
            for s in w.roads.segments.iter().filter(|s| s.alive) {
                let (a, b) = (w.roads.node(s.a).pos, w.roads.node(s.b).pos);
                writeln!(f, "{:?},{},{},{:.1},{:.1},{:.1},{:.1},{:.2}", s.kind, s.lanes_fwd, s.lanes_bwd, a.x, a.y, b.x, b.y, s.half_width()).unwrap();
            }
            for c in &w.walk.crossings {
                let (a, b) = (w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos);
                writeln!(f, "Crossing,{},{},{:.1},{:.1},{:.1},{:.1},0", c.closed as u8, 0, a.x, a.y, b.x, b.y).unwrap();
            }
        }
        "spots" => spots(&mut w, ticks, args.get(4).map(String::as_str).unwrap_or("spots.txt"), &args[5.min(args.len())..]),
        "near" => near(&w, args[3].parse().unwrap(), args[4].parse().unwrap()),
        _ => eprintln!("paths|trace|near"),
    }
}
