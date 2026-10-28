//! [r4-decks] Probe the raised ways near a point on a save: their segments
//! and levels, the columns and gaps, and the buildings standing under the
//! decks (floors, when built, what fits). Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example deck_probe -- <save.ron> x y r [rebuild]
use glam::Vec2;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let f: Vec<f32> = args[2..5].iter().map(|s| s.parse().unwrap()).collect();
    let (p, r) = (Vec2::new(f[0], f[1]), f[2]);
    if args.get(5).is_some_and(|a| a == "rebuild") {
        w.rebuild_from_plan();
        println!("-- after rebuild_from_plan");
    }
    if args.get(5).is_some_and(|a| a == "trial") {
        let g: Vec<f32> = args[6..10].iter().map(|s| s.parse().unwrap()).collect();
        let pts = vec![Vec2::new(g[0], g[1]), Vec2::new(g[2], g[3])];
        w.project = None;
        w.add_gesture(nd_sim::plan::Gesture::Road { points: pts, lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: vec![], car_only: None });
        let check = w.active_project_check();
        println!("-- trial: refusal {:?}; gaps {:?}; lost {}", check.refusal, check.pillar_gaps, check.lost.len());
        let n = w.buildings.len();
        println!("implement: {:?}; buildings {} -> {}", w.try_implement(), n, w.buildings.len());
    }
    let all: Vec<_> = w.lots.keys().collect();
    for pl in w.pillars.iter().filter(|q| q.at.distance(p) < r) {
        println!("pillar ({:.1},{:.1}) site {:?}", pl.at.x, pl.at.y, w.pillar_site(pl.at, pl.level, &all, &[]));
    }
    println!("tick {} pillars {} gaps {} buildings {}", w.clock.tick, w.pillars.len(), w.pillar_gaps.len(), w.buildings.len());
    for g in &w.pillar_gaps {
        println!("gap ({:.0},{:.0})->({:.0},{:.0}) end {} over_road {} level {:.1}", g.a.x, g.a.y, g.b.x, g.b.y, g.end, g.over_road, g.level);
    }
    for sid in w.roads.segments_in_box(p - Vec2::splat(r), p + Vec2::splat(r)) {
        let s = w.roads.segment(sid);
        let (la, lb) = (w.roads.node(s.a).level, w.roads.node(s.b).level);
        if la.max(lb) < 1.0 {
            continue;
        }
        let (a, b) = w.roads.segment_points(sid);
        println!("seg {:?} {:?} ({:.0},{:.0})->({:.0},{:.0}) lv {:.1}->{:.1} via {} spans {}", sid, s.kind, a.x, a.y, b.x, b.y, la, lb, s.viaduct, w.roads.spans_lots(sid));
    }
    for pl in w.pillars.iter().filter(|q| q.at.distance(p) < r) {
        println!("pillar ({:.1},{:.1}) lv {:.1} hw {:.1} way {:x}", pl.at.x, pl.at.y, pl.level, pl.half_width, pl.way);
    }
    let day = |t: u64| t as f32 / 1440.0;
    for (lid, l) in w.lots.iter().filter(|(_, l)| l.center.distance(p) < r) {
        let cap = w.deck_cap(lid);
        let col = w.pillars_on(&l.polygon);
        let Some(bid) = l.building else {
            if cap.is_some() || col {
                println!("lot {:?} ({:.0},{:.0}) empty {:?} cap {:?} column {}", lid, l.center.x, l.center.y, l.land_use, cap, col);
            }
            continue;
        };
        let b = &w.buildings[bid];
        // Distance from the lot to the nearest raised segment.
        let mut near = f32::MAX;
        let mut lv = 0.0;
        for sid in w.roads.segments_in_box(l.center - Vec2::splat(60.0), l.center + Vec2::splat(60.0)) {
            let s = w.roads.segment(sid);
            let (la, lb) = (w.roads.node(s.a).level, w.roads.node(s.b).level);
            if la.min(lb) < 1.0 {
                continue;
            }
            let (a, bb) = w.roads.segment_points(sid);
            let d = l.polygon.iter().map(|&v| nd_sim::geom::project_on_segment(v, a, bb).1.distance(v)).fold(f32::MAX, f32::min);
            let d = d.min(nd_sim::geom::distance_to_outline(nd_sim::geom::project_on_segment(l.center, a, bb).1, &l.polygon));
            let inside = nd_sim::geom::point_in_polygon(nd_sim::geom::project_on_segment(l.center, a, bb).1, &l.polygon);
            let d = if inside { 0.0 } else { d };
            if d < near {
                near = d;
                lv = la.min(lb);
            }
        }
        if near < 8.0 || cap.is_some() || col {
            println!(
                "bld {:?} lot {:?} ({:.0},{:.0}) {:?} floors {} built day {:.1} cap {:?} column {} near-deck {:.1} m lv {:.1} name {}",
                bid, lid, l.center.x, l.center.y, b.land_use, b.floors, day(b.built_tick), cap, col, near, lv, b.name
            );
        }
    }
}
