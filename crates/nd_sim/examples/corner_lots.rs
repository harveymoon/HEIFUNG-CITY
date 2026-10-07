//! [review] Probe: lots whose pad (levelled to the front street) stands
//! above another street they also touch (a corner lot's side street).
//!
//! cargo run --profile fast -p nd_sim --example corner_lots -- saves/city.ron

fn main() {
    let path = std::env::args().nth(1).expect("a save path");
    let w = nd_sim::World::from_ron(&std::fs::read_to_string(&path).expect("read")).expect("load");
    println!("relief {} lots {} retaining walls {}", w.relief, w.lots.len(), w.ground.retaining.len());
    let mut diffs: Vec<(f32, glam::Vec2, f32, f32, nd_sim::roads::SegmentId, nd_sim::LotId)> = Vec::new();
    for (id, lot) in w.lots.iter() {
        let pad = w.lot_pad(id);
        let mut worst: Option<(f32, f32, nd_sim::roads::SegmentId)> = None;
        for (sid, s) in w.roads.alive_segments() {
            if sid == lot.front_segment || !s.is_street() || s.bridge || s.tunnel {
                continue;
            }
            let (a, b) = w.roads.segment_points(sid);
            let reach = s.half_width() + nd_sim::zoning::SIDEWALK + 1.0;
            for k in 0..lot.polygon.len() {
                let (p, q) = (lot.polygon[k], lot.polygon[(k + 1) % lot.polygon.len()]);
                for f in [0.0, 0.25, 0.5, 0.75] {
                    let x = p.lerp(q, f);
                    let (t, on) = nd_sim::roads::project_on_segment(x, a, b);
                    if on.distance(x) <= reach {
                        let z = w.road_height(sid, t);
                        let d = pad - z;
                        if worst.is_none_or(|(wd, _, _)| d > wd) {
                            worst = Some((d, z, sid));
                        }
                    }
                }
            }
        }
        if let Some((d, z, sid)) = worst {
            diffs.push((d, lot.center, pad, z, sid, id));
        }
    }
    diffs.sort_by(|a, b| b.0.total_cmp(&a.0));
    for th in [0.5f32, 1.0, 2.0, 3.0] {
        println!("pad above a side street by > {th} m: {}", diffs.iter().filter(|d| d.0 > th).count());
    }
    let walled = |id: nd_sim::LotId| w.ground.retaining.iter().any(|r| r.lot == id);
    println!("of those over 1 m, with a wall: {} of {}", diffs.iter().filter(|d| d.0 > 1.0 && walled(d.5)).count(), diffs.iter().filter(|d| d.0 > 1.0).count());
    for d in diffs.iter().filter(|d| d.0 > 1.0 && !walled(d.5)).take(5) {
        println!("  no wall: {:.2} m at {:?}", d.0, d.1);
    }
    for d in diffs.iter().take(8) {
        let lot = &w.lots[d.5];
        println!("  {:.2} m at {:?} (pad {:.2}, street {:.2}) side {:?} front {:?} poly {:?}", d.0, d.1, d.2, d.3, w.roads.segment_points(d.4), w.roads.segment_points(lot.front_segment), lot.polygon);
    }
}
