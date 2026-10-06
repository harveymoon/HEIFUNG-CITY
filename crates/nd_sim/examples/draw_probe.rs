//! Load a save, run it on and check where cars are *drawn*: the pose the
//! renderer interpolates (`Car::pose_at`) sampled through each tick, and
//! the height it lifts them to (`World::car_height`). Reports draw jumps
//! (a sample further from the last than the car could drive) and height
//! jumps, with the lane and segment they happened on.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::LaneKind;
use nd_sim::World;
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let dt = w.config.traffic_seconds_per_tick;
    const N: usize = 32;
    let mut last: HashMap<nd_sim::CarId, (glam::Vec2, f32)> = HashMap::new();
    let mut shown = 0;
    for _ in 0..ticks {
        w.tick(&mut minds);
        let mut now = HashMap::new();
        for (id, c) in w.traffic.cars.iter() {
            let lane = c.route.get(c.idx).copied().unwrap_or(0);
            let seg = match w.traffic.net.lanes[lane as usize].kind { LaneKind::Road { segment, .. } => format!("seg {}", segment.0), LaneKind::Connector { node, .. } => format!("conn@node {}", node.0) };
            let vmax = c.v.max(c.prev_pos.distance(c.pos) / dt) + 3.0;
            let mut prev = last.get(&id).copied();
            for k in 0..=N {
                let f = k as f32 / N as f32;
                let (p, _) = c.pose_at(f);
                // The height the renderer lifts it to there.
                let h = w.car_height_at(c, p);
                if let Some((q, hq)) = prev {
                    let step = p.distance(q);
                    let allowed = vmax * dt / N as f32 * 2.5 + 1.0;
                    if (step > allowed || (h - hq).abs() > 1.0) && shown < 40 {
                        shown += 1;
                        println!("t{} {:?} f{:.2} drawn step {:.1} m (allowed {:.1}) h {:.1}->{:.1} on lane {} {} s {:.1} v {:.1} idx {}/{} pos ({:.0},{:.0})", w.clock.tick, id, f, step, allowed, hq, h, lane, seg, c.s, c.v, c.idx, c.route.len(), p.x, p.y);
                    }
                }
                prev = Some((p, h));
            }
            now.insert(id, prev.unwrap());
        }
        last = now;
    }
}
