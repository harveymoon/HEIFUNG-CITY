//! [r14-photofix] Probe: load a save, read the hunger demand straight after
//! the load, then run to 09:00 and count cyclists stacked on one another
//! (another rider within 0.3 m, heading the same way) and riders who have
//! not moved for an hour (a following gap that never lets go would show
//! there).
//!
//! cargo run --profile fast -p nd_sim --example rider_gap_probe -- saves/city.ron [hours]

use nd_sim::citizens::{Activity, CitizenId, TravelMode};
use nd_sim::{Vec2, World};
use std::collections::HashMap;

fn riders(w: &World) -> Vec<(CitizenId, Vec2, f32)> {
    w.citizens.iter().filter(|(_, c)| c.mode == TravelMode::Bike && matches!(c.activity, Activity::Traveling { .. })).map(|(id, c)| (id, c.pos, c.heading)).collect()
}

/// (riders, riders with another within 0.3 m heading the same way, the
/// biggest such group, pairs closer than 2 m on one line)
fn stacked(rs: &[(CitizenId, Vec2, f32)]) -> (usize, usize, usize, usize) {
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, r) in rs.iter().enumerate() {
        grid.entry(((r.1.x / 2.0).floor() as i32, (r.1.y / 2.0).floor() as i32)).or_default().push(i);
    }
    let (mut on_one, mut biggest, mut close) = (0, 1, 0);
    for (i, r) in rs.iter().enumerate() {
        let (cx, cy) = ((r.1.x / 2.0).floor() as i32, (r.1.y / 2.0).floor() as i32);
        let mut group = 1;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &j in grid.get(&(x, y)).map_or(&[][..], |v| &v[..]) {
                    if j == i || (r.2 - rs[j].2).cos() < 0.8 {
                        continue;
                    }
                    let d = r.1.distance(rs[j].1);
                    if d < 0.3 {
                        group += 1;
                    }
                    if j > i && d < 2.0 {
                        close += 1;
                    }
                }
            }
        }
        if group > 1 {
            on_one += 1;
        }
        biggest = biggest.max(group);
    }
    (rs.len(), on_one, biggest, close)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let hours: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(9);
    let mut w = World::read_save(&src).expect("load");
    let n = &w.food_need;
    println!("after load: day {} {:02}:{:02}, people {}, hunger demand: hungry_counted {} share {:.3} town_hungry {} cells {}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len(), n.hungry, n.share, n.town_hungry(), n.cells.len());
    let mut minds = nd_minds::RuleBasedMind::default();
    let mut still: HashMap<CitizenId, (Vec2, u64)> = HashMap::new();
    let start = w.clock.tick;
    let mut worst_stuck = 0;
    while w.clock.tick < start + hours * 60 {
        w.tick(&mut minds);
        w.drain_events();
        let rs = riders(&w);
        let tick = w.clock.tick;
        still.retain(|id, _| rs.iter().any(|r| r.0 == *id));
        for r in &rs {
            let e = still.entry(r.0).or_insert((r.1, tick));
            if e.0.distance(r.1) > 0.5 {
                *e = (r.1, tick);
            }
        }
        let stuck = still.values().filter(|(_, t)| tick - t >= 60).count();
        worst_stuck = worst_stuck.max(stuck);
        if (tick - start) % 30 == 0 {
            let (all, on_one, biggest, close) = stacked(&rs);
            println!("{:02}:{:02} riders {all:5} on one point {on_one:4} biggest group {biggest} pairs <2 m on one line {close:4} still an hour+ {stuck}", w.clock.hour(), w.clock.minute_of_day() % 60);
        }
    }
    println!("most riders still for an hour or more at once: {worst_stuck}");
}
