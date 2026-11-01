//! [r5-layered] Survey a save's raised ways: length by deck level, and what
//! stands on the ground under them. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example under_survey -- <save.ron>
use glam::Vec2;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let w = World::read_save(&args[1]).expect("load");
    println!("day {} pillars {} buildings {}", w.clock.day(), w.pillars.len(), w.buildings.len());
    let mut bands = [0.0f32; 6];
    for (sid, s) in w.roads.alive_segments() {
        if !s.viaduct || s.tunnel {
            continue;
        }
        let (la, lb) = (w.roads.node(s.a).level, w.roads.node(s.b).level);
        let (a, b) = w.roads.segment_points(sid);
        let lv = la.min(lb);
        let k = if lv < 3.0 { 0 } else if lv < 4.5 { 1 } else if lv < 6.0 { 2 } else if lv < 8.0 { 3 } else if lv < 12.0 { 4 } else { 5 };
        bands[k] += a.distance(b);
        if std::env::var("SEGS").is_ok() {
            println!("{:?} {:?} ({:.0},{:.0})-({:.0},{:.0}) {:.1}-{:.1}", sid, s.kind, a.x, a.y, b.x, b.y, la, lb);
        }
    }
    println!("raised length by min level: <3 {:.0}  3-4.5 {:.0}  4.5-6 {:.0}  6-8 {:.0}  8-12 {:.0}  12+ {:.0}", bands[0], bands[1], bands[2], bands[3], bands[4], bands[5]);
    // What stands under the decks (4.2 m and up), every 4 m along them.
    let mut what: std::collections::BTreeMap<String, u32> = Default::default();
    for (sid, s) in w.roads.alive_segments() {
        if !s.viaduct || s.tunnel {
            continue;
        }
        let (a, b) = w.roads.segment_points(sid);
        let (la, lb) = (w.roads.node(s.a).level, w.roads.node(s.b).level);
        let n = (a.distance(b) / 4.0) as usize;
        for k in 0..n {
            let t = (k as f32 + 0.5) / n as f32;
            if la + (lb - la) * t < std::env::var("MINLV").ok().and_then(|v| v.parse().ok()).unwrap_or(4.2) {
                continue;
            }
            let p = a + (b - a) * t;
            let key = match w.lots.values().find(|l| l.contains(p)) {
                Some(l) => match l.building.and_then(|b| w.buildings.get(b)) {
                    Some(b) => format!("lot {:?} built {}fl", l.land_use, b.floors),
                    None => format!("lot {:?} vacant", l.land_use),
                },
                None => {
                    let lv = la + (lb - la) * t;
                    let road = w.roads.segments_in_box(p - glam::Vec2::splat(30.0), p + glam::Vec2::splat(30.0)).any(|o| {
                        let os = w.roads.segment(o);
                        if o == sid || !os.alive || os.tunnel {
                            return false;
                        }
                        let (oa, ob) = w.roads.segment_points(o);
                        let (tt, q) = nd_sim::roads::project_on_segment(p, oa, ob);
                        let ol = w.roads.node(os.a).level + (w.roads.node(os.b).level - w.roads.node(os.a).level) * tt;
                        q.distance(p) < os.half_width() + 3.5 && ol < lv - 2.0
                    });
                    let water = !w.landscape.is_flat() && w.landscape.is_water(p);
                    if road { "road below".into() } else if water { "water".into() } else if w.parcels.parcels.iter().any(|q| q.contains(p)) { "parcel unzoned".into() } else { "open".into() }
                }
            };
            *what.entry(key).or_default() += 1;
        }
    }
    println!("under the decks: {what:?}");
    if std::env::var("PERSEG").is_ok() {
        let bays = w.find_bays();
        for (sid, s) in w.roads.alive_segments() {
            if !s.viaduct || s.tunnel { continue; }
            let (a, b) = w.roads.segment_points(sid);
            let (la, lb) = (w.roads.node(s.a).level, w.roads.node(s.b).level);
            if la.min(lb) < 6.0 { continue; }
            let hw = nd_sim::world::deck_half_width(s.kind, s.lanes_fwd, s.lanes_bwd);
            let len = a.distance(b);
            let dir = (b - a) / len;
            let cols = w.pillars.iter().filter(|p| { let d = p.at - a; let t = d.dot(dir); (-1.0..=len + 1.0).contains(&t) && d.perp_dot(dir).abs() < hw + 0.8 }).count();
            let nb = bays.iter().filter(|bay| nd_sim::roads::project_on_segment(bay.pos, a, b).1.distance(bay.pos) < hw).count();
            println!("seg {:?} {:?} ({:.0},{:.0})-({:.0},{:.0}) len {:.0} lv {:.1}-{:.1} hw {:.1} cols {} bays {}", sid, s.kind, a.x, a.y, b.x, b.y, len, la, lb, hw, cols, nb);
        }
    }
    let u = &w.undercroft;
    println!("bays {} by use {:?} (bare market hawkers workshop court camp)", u.bays.len(), u.counts());
    let mut by_clear = [0usize; 4];
    for b in &u.bays {
        by_clear[if b.clearance < 4.5 { 0 } else if b.clearance < 7.0 { 1 } else if b.clearance < 11.0 { 2 } else { 3 }] += 1;
    }
    println!("by headroom <4.5 {} 4.5-7 {} 7-11 {} 11+ {}", by_clear[0], by_clear[1], by_clear[2], by_clear[3]);
    if std::env::var("HEIGHTS").is_ok() {
        for b in w.undercroft.bays.iter().take(30) {
            println!("bay ({:.0},{:.0}) dir ({:.2},{:.2}) level {:.1} ground {:.2} surface {:.2} relief {}", b.pos.x, b.pos.y, b.dir.x, b.dir.y, b.level, w.ground_height(b.pos), w.surface_height(b.pos), w.relief);
        }
    }
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    if days > 0 {
        let mut w = w;
        let mut minds = nd_minds::RuleBasedMind::default();
        let end = w.clock.tick + days * 1440;
        while w.clock.tick < end {
            w.tick(&mut minds);
            if w.clock.tick % 1440 == 0 {
                let u = &w.undercroft;
                let v: u32 = u.bays.iter().map(|b| b.visits_yesterday).sum();
                let t: f32 = u.bays.iter().map(|b| b.takings_yesterday).sum();
                let camps = w.society.homeless.camps.iter().filter(|c| c.under).count();
                println!("day {} bays {} by use {:?} visits yesterday {} takings {:.0} under-camps {} homeless households {}", w.clock.day(), u.bays.len(), u.counts(), v, t, camps, w.society.homeless.households());
            }
        }
        for b in w.undercroft.bays.iter().filter(|b| b.use_ != nd_sim::undercroft::UnderUse::Bare).take(40) {
            println!("  {:?} ({:.0},{:.0}) {:.0}x{:.0} clear {:.1} under '{}' holders {} visits {} takings {:.0}", b.use_, b.pos.x, b.pos.y, b.len, b.width, b.clearance, b.under, b.holders.len(), b.visits_yesterday, b.takings_yesterday);
        }
        if let Some(out) = args.get(3) {
            std::fs::write(out, w.to_ron().unwrap()).unwrap();
        }
    }
    let _ = Vec2::ZERO;
}
