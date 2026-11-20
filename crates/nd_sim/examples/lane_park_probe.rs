//! [r11-parking] Cars parked on the lanes: load a save (raw, then as the
//! game loads it), count street-parked cars standing in a traffic lane, on
//! a ring, ramp, highway, tunnel or bridge, or on top of another parked
//! car, and dump the cars near `x y` (optional). Then run `days` (default 0)
//! and count again.
//!
//! cargo run --profile fast --example lane_park_probe -p nd_sim -- <save> [days] [x y]...

use glam::Vec2;
use nd_minds::RuleBasedMind;
use nd_sim::{CarPlace, World};

/// Why a street-parked car stands where it should not (None: fine): its
/// centre in a way it may not stand on, or within 1.9 m of the centre line
/// of a lane (a moving car then drives through it). Ways more than 2.5 m
/// above or below the car's own street (a viaduct overhead) do not count.
fn bad_place(w: &World, by_seg: &std::collections::HashMap<u32, Vec<usize>>, p: Vec2, own: Option<nd_sim::roads::SegmentId>) -> Option<String> {
    let height = |sid, p: Vec2| {
        let (a, b) = w.roads.segment_points(sid);
        let len = a.distance(b).max(0.01);
        w.road_height(sid, ((p - a).dot((b - a) / len) / len).clamp(0.0, 1.0))
    };
    let own_h = own.filter(|s| w.roads.try_segment(*s).is_some()).map(|s| height(s, p));
    for sid in w.roads.segments_near(p, 30.0) {
        let s = w.roads.segment(sid);
        if !s.is_street() || s.lanes() == 0 {
            continue;
        }
        if own_h.is_some_and(|h| (height(sid, p) - h).abs() > 2.5) {
            continue;
        }
        let (a, b) = w.roads.segment_points(sid);
        let off = p.distance(nd_sim::geom::project_on_segment(p, a, b).1);
        if off < s.half_width() && (s.ring || s.ramp || s.is_highway() || s.tunnel || s.bridge) {
            return Some(format!("on a {} (seg {} off {:.1})", if s.ring { "ring" } else if s.ramp { "ramp" } else if s.is_highway() { "highway" } else if s.tunnel { "tunnel" } else { "bridge" }, sid.0, off));
        }
        for lane in by_seg.get(&sid.0).into_iter().flatten().map(|&l| &w.traffic.net.lanes[l]) {
            let gap = lane.points.windows(2).map(|q| p.distance(nd_sim::geom::project_on_segment(p, q[0], q[1]).1)).fold(f32::MAX, f32::min);
            if gap < 1.9 {
                return Some(format!("in a lane (seg {} {}+{} {:.1} m from a lane centre)", sid.0, s.lanes_fwd, s.lanes_bwd, gap));
            }
        }
    }
    None
}

fn census(w: &World, label: &str) {
    let spots: Vec<_> = w.vehicles.cars.iter().filter_map(|(id, c)| match c.place {
        CarPlace::Parked(s) if s.kind.on_street() => Some((id, c, s)),
        _ => None,
    }).collect();
    let mut by_seg = std::collections::HashMap::<u32, Vec<usize>>::new();
    for (i, l) in w.traffic.net.lanes.iter().enumerate() {
        if let nd_sim::traffic::LaneKind::Road { segment, .. } = l.kind {
            by_seg.entry(segment.0).or_default().push(i);
        }
    }
    let mut bad = std::collections::BTreeMap::<String, u32>::new();
    let mut eg = Vec::new();
    for (id, c, s) in &spots {
        if let Some(why) = bad_place(w, &by_seg, s.pos, s.segment) {
            *bad.entry(why.split(' ').take(3).collect::<Vec<_>>().join(" ")).or_default() += 1;
            if eg.len() < 12 {
                eg.push(format!("  {:?} {:?} {:?} at ({:.1},{:.1}) seg {:?} side {} slot {} since {} booted {}: {}", id, c.model.body, s.kind, s.pos.x, s.pos.y, s.segment.map(|x| x.0), s.side, s.slot, s.since, s.booted, why));
            }
        }
    }
    let mut stacked = 0;
    for i in 0..spots.len() {
        for j in i + 1..spots.len() {
            if spots[i].2.pos.distance(spots[j].2.pos) < 2.0 {
                stacked += 1;
            }
        }
    }
    let total: u32 = bad.values().sum();
    println!("{label}: street-parked {} on lanes {} {:?} stacked pairs {}", spots.len(), total, bad, stacked);
    for e in eg {
        println!("{e}");
    }
}

fn dump(w: &World, at: Vec2) {
    for (id, c) in w.vehicles.cars.iter() {
        let CarPlace::Parked(s) = c.place else { continue };
        if s.pos.distance(at) < 12.0 {
            println!("  near: {:?} {:?} owner {:?} {:?} at ({:.2},{:.2}) seg {:?} side {} slot {} since {} booted {}", id, c.model.body, c.owner, s.kind, s.pos.x, s.pos.y, s.segment.map(|x| x.0), s.side, s.slot, s.since, s.booted);
        }
    }
    for sid in w.roads.segments_near(at, 15.0) {
        let s = w.roads.segment(sid);
        let (a, b) = w.roads.segment_points(sid);
        println!("  seg {}: ({:.1},{:.1})-({:.1},{:.1}) {:?} {}+{} ring {} ramp {} hw {} bridge {} viaduct {} tunnel {} ring_of {:?} legal slots {} h {:.1}..{:.1}", sid.0, a.x, a.y, b.x, b.y, s.kind, s.lanes_fwd, s.lanes_bwd, s.ring, s.ramp, s.is_highway(), s.bridge, s.viaduct, s.tunnel, w.roads.ring_of_segment(sid).map(|r| (r, w.roads.rings[r].level)), w.kerb_slot_count(sid), w.road_height(sid, 0.0), w.road_height(sid, 1.0));
    }
    for c in w.traffic.cars.values() {
        if c.pos.distance(at) < 8.0 {
            println!("  moving: {:?} at ({:.1},{:.1}) v {:.1}", c.errand, c.pos.x, c.pos.y, c.v);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read save");
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let ats: Vec<Vec2> = args[3.min(args.len())..].chunks(2).filter(|c| c.len() == 2).map(|c| Vec2::new(c[0].parse().unwrap(), c[1].parse().unwrap())).collect();
    let text = nd_sim::world::save_text(&bytes).expect("text");
    let raw: World = ron::from_str(&text).expect("parse");
    census(&raw, "raw save");
    drop(raw);
    let mut w = World::from_ron(&text).expect("load");
    census(&w, "after load");
    for &at in &ats {
        println!("at ({:.1},{:.1}):", at.x, at.y);
        dump(&w, at);
    }
    let mut minds = RuleBasedMind::default();
    for t in 0..days * 1440 {
        w.tick(&mut minds);
        if t % 720 == 719 {
            census(&w, &format!("day {} {:02}:00", w.clock.day() + 1, w.clock.hour()));
        }
    }
}
