//! [w1-roads] Where the Perfect City's port and stations stand against
//! the streets. Not part of the game.
use nd_procgen::citygen;
use nd_sim::Vec2;
fn main() {
    let preset = citygen::CityPreset::PerfectCity;
    let (w, _, laid) = citygen::generate_world(preset, preset.default_seed());
    for l in laid.furnished.iter().filter(|l| l.contains("port") || l.contains("station") || l.contains("yard") || l.contains("ferry")) {
        println!("{l}");
    }
    for p in w.ports.ports.values() {
        println!("port {:?} center {:?} stub {:?} road {} corners {:?}", p.kind, p.center, p.stub, p.road_connected, p.corners());
        let c = p.center;
        for (sid, s) in w.roads.alive_segments() {
            let (a, b) = w.roads.segment_points(sid);
            if nd_sim::geom::project_on_segment(c, a, b).1.distance(c) < 90.0 {
                println!("   seg {sid:?} {a:?}-{b:?} kind {:?} lanes {}/{} motorway {} ramp {} deg {}/{}", s.kind, s.lanes_fwd, s.lanes_bwd, s.is_motorway(), s.ramp, w.roads.node(s.a).segments.len(), w.roads.node(s.b).segments.len());
            }
        }
    }
    for (sid, s) in w.roads.alive_segments() {
        let (a, b) = w.roads.segment_points(sid);
        let c = Vec2::new(0.0, -410.0);
        if s.is_street() && (a.distance(c) < 160.0 || b.distance(c) < 160.0) && (a.y > -440.0 || b.y > -440.0) && (a.y < -380.0 || b.y < -380.0) {
            println!("near-central {sid:?} {a:?}-{b:?} lanes {}/{} deg {}/{} motorway {}", s.lanes_fwd, s.lanes_bwd, w.roads.street_degree(s.a), w.roads.street_degree(s.b), s.is_motorway());
        }
    }
    let y = nd_sim::rail::TRUNK_Y;
    for kind in [] as [nd_sim::rail::RailFacility; 0] {
        for x in (-400..=600).step_by(50) {
            for dy in [3.0f32, -3.0] {
                let (f, rf, sf) = nd_sim::rail::Facility::plan(kind, Vec2::new(x as f32, y + dy), &w.roads);
                let why = w.rail_piece_problem(&f);
                println!("{kind:?} x {x} dy {dy}: rail {rf} road {sf} center {:?} -> {:?}", f.center, why);
            }
        }
    }
    for (sid, s) in w.roads.alive_segments() {
        let (a, b) = w.roads.segment_points(sid);
        if s.is_street() && (a.y - y).abs() < 60.0 && (b.y - y).abs() < 60.0 && (a.x - b.x).abs() > 20.0 {
            println!("along-track street {a:?}-{b:?} lanes {}/{}", s.lanes_fwd, s.lanes_bwd);
        }
    }
}
