//! [fix/roads-tools] Probe: on a save, lay a new road across a built block
//! and compare what the plan shows in red (`road_takes`) with what
//! implementing changes (buildings lost, lots reshaped). Not part of the
//! game. `g2_recut_probe <save.ron> [tries]`
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, Vec2, World};
use std::collections::BTreeSet;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let base = World::from_ron(&std::fs::read_to_string(&a[1]).unwrap()).expect("load");
    let tries: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    // Candidate roads: from a street node, 140 m across, to another street node.
    let nodes: Vec<(usize, Vec2)> = base.roads.nodes.iter().enumerate().filter(|(_, n)| n.segments.iter().filter_map(|&s| base.roads.try_segment(s)).any(|s| s.is_street() && !s.fronts_nothing())).map(|(i, n)| (i, n.pos)).collect();
    let mut done = 0;
    let mut k = 0usize;
    while done < tries && k < nodes.len() * 4 {
        k += 1;
        let (_, p) = nodes[(k * 7919) % nodes.len()];
        let Some(&(_, q)) = nodes.iter().filter(|(_, q)| (q.distance(p) - 140.0).abs() < 25.0).min_by(|x, y| ((x.1.distance(p) - 140.0).abs()).total_cmp(&(y.1.distance(p) - 140.0).abs())) else { continue };
        // The line must not follow a street already.
        if base.roads.nearest_where((p + q) * 0.5, |s| s.is_street()).is_some_and(|(_, _, c)| c.distance((p + q) * 0.5) < 15.0) {
            continue;
        }
        let pts = vec![p, q];
        let takes: BTreeSet<_> = base.road_takes(&pts, &[0.0, 0.0], 1, 1, RoadKind::Street).into_iter().filter_map(|l| base.lots[l].building).collect();
        if takes.is_empty() {
            continue;
        }
        let mut w = base.clone();
        w.project = None;
        w.add_gesture(Gesture::Road { points: pts.clone(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        let check = w.active_project_check();
        if check.refusal.is_some() {
            continue;
        }
        let before_b: BTreeSet<_> = w.buildings.iter().filter(|(_, b)| b.demolition.is_none()).map(|(id, _)| id).collect();
        let before_lots: Vec<(nd_sim::LotId, Vec<Vec2>, Option<nd_sim::BuildingId>)> = w.lots.iter().map(|(id, l)| (id, l.polygon.clone(), l.building)).collect();
        let t = std::time::Instant::now();
        w.try_implement().unwrap();
        let dt = t.elapsed();
        let red_all: BTreeSet<_> = takes.iter().copied().chain(check.lost.iter().copied()).collect();
        let after_b: BTreeSet<_> = w.buildings.iter().filter(|(_, b)| b.demolition.is_none()).map(|(id, _)| id).collect();
        let lost: Vec<_> = before_b.difference(&after_b).copied().collect();
        let beyond: Vec<_> = lost.iter().filter(|b| !takes.contains(b)).collect();
        let unshown = lost.iter().filter(|b| !red_all.contains(b)).count();
        println!("   shown in red with the trial's: {} ; lost but not shown: {unshown}; shown but kept: {}", red_all.len(), red_all.iter().filter(|b| !lost.contains(b)).count());
        let mut reshaped = 0;
        let mut far = 0.0f32;
        let mut farlist = Vec::new();
        for (id, poly, b) in &before_lots {
            let same = w.lots.get(*id).is_some_and(|l| l.polygon == *poly) || b.and_then(|b| w.buildings.get(b)).and_then(|x| w.lots.get(x.lot)).is_some_and(|l| l.polygon == *poly);
            if !same && b.is_some() {
                reshaped += 1;
                let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
                let d = nd_sim::geom::distance_to_polyline(c, &pts);
                far = far.max(d);
                if d > 60.0 {
                    let now = w.lots.get(*id);
                    let moved = b.and_then(|b| w.buildings.get(b)).map(|x| x.lot);
                    farlist.push(format!("{d:.0} m: lot {id:?} at ({:.0},{:.0}) exists {} npts {}->{} building {:?} now on {:?} city {}", c.x, c.y, now.is_some(), poly.len(), now.map_or(0, |l| l.polygon.len()), b, moved, b.is_some_and(|b| w.is_city_building(b))));
                }
            }
        }
        for f in farlist.iter().take(6) { println!("   far: {f}"); }
        println!("road ({:.0},{:.0})-({:.0},{:.0}): red {} lost {} beyond-red {} built lots reshaped {} (farthest {:.0} m) implement {:?}", p.x, p.y, q.x, q.y, takes.len(), lost.len(), beyond.len(), reshaped, far, dt);
        for b in beyond.iter().take(8) {
            let lot = before_lots.iter().find(|(_, _, x)| *x == Some(**b));
            if let Some((_, poly, _)) = lot {
                let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
                println!("   beyond: {:?} at ({:.0},{:.0}) {:.0} m from the road; new lot there: {}", b, c.x, c.y, nd_sim::geom::distance_to_polyline(c, &pts), w.lots.values().any(|l| l.contains(c)));
            }
        }
        done += 1;
    }
}
