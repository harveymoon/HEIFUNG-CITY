//! [r8-parks] Headless look at the parks of a save: each park (as the
//! city counts it), its lots, buildings, names, features and car park, and
//! the park lots near a point. `park_probe <save.ron> [x y radius] [ticks]`.
//! Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{LandUse, World};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let x: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(-610.0);
    let y: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(-715.0);
    let r: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(160.0);
    let ticks: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
    w.refresh_parks_now();
    let at = glam::Vec2::new(x, y);
    println!("day {} {:02}:{:02}", w.clock.day(), w.clock.hour(), w.clock.minute());
    let mut near: Vec<_> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park && !l.shore.beach && l.center.distance(at) <= r).collect();
    near.sort_by(|a, b| a.1.center.x.total_cmp(&b.1.center.x));
    for (id, l) in &near {
        let b = l.building.and_then(|b| w.buildings.get(b).map(|x| (b, x)));
        let p = l.park.as_ref();
        println!(
            "lot {:?} c=({:.0},{:.0}) area {:.0} landlocked {} car {} | bld {} | cluster {} lots {:.0} m2 anchor {} | shared {:?} | features {:?} | spaces {}",
            id,
            l.center.x,
            l.center.y,
            l.area(),
            l.landlocked,
            l.car_access(),
            b.map_or("-".to_string(), |(bid, x)| format!("{:?} '{}' site={}", bid, x.name, x.construction.as_ref().map_or("no".to_string(), |c| format!("{:.2}", c.progress)))),
            p.map_or(0, |p| p.cluster_lots),
            p.map_or(0.0, |p| p.cluster_area),
            p.is_some_and(|p| p.anchor),
            p.map(|p| p.shared_edges.clone()).unwrap_or_default(),
            p.map(|p| p.features.iter().map(|f| f.kind.label()).collect::<Vec<_>>()).unwrap_or_default(),
            p.map_or(0, |p| p.car_park.as_ref().map_or(0, |c| c.spaces.len())),
        );
    }
    // ND_PARK_DUMP=<file>: the lots, roads, paths and features near the
    // point as JSON lines, for plotting.
    if let Ok(path) = std::env::var("ND_PARK_DUMP") {
        use std::io::Write;
        let mut f = std::fs::File::create(path).unwrap();
        for (id, l) in w.lots.iter().filter(|(_, l)| l.center.distance(at) <= r * 1.3) {
            let pts: Vec<[f32; 2]> = l.polygon.iter().map(|p| [p.x, p.y]).collect();
            let park = l.land_use == LandUse::Park;
            writeln!(f, "{{\"lot\":\"{:?}\",\"park\":{},\"landlocked\":{},\"poly\":{:?}}}", id, park, l.landlocked, pts).unwrap();
            if let Some(p) = &l.park {
                for path in &p.paths {
                    let pts: Vec<[f32; 2]> = path.iter().map(|p| [p.x, p.y]).collect();
                    writeln!(f, "{{\"path\":{:?}}}", pts).unwrap();
                }
                for ft in &p.features {
                    writeln!(f, "{{\"feature\":\"{}\",\"at\":[{},{}],\"r\":{}}}", ft.kind.label(), ft.pos.x, ft.pos.y, ft.radius).unwrap();
                }
                for (a, b) in &p.inner_edges {
                    writeln!(f, "{{\"inner\":[[{},{}],[{},{}]]}}", a.x, a.y, b.x, b.y).unwrap();
                }
                if let Some(c) = p.car_park.as_ref().filter(|c| c.own) {
                    let pts: Vec<[f32; 2]> = c.outline.iter().map(|p| [p.x, p.y]).collect();
                    writeln!(f, "{{\"carpark\":{:?}}}", pts).unwrap();
                }
            }
        }
        for (sid, s) in w.roads.alive_segments() {
            let (a, b) = w.roads.segment_points(sid);
            if a.distance(at) < r * 1.5 || b.distance(at) < r * 1.5 {
                writeln!(f, "{{\"road\":[[{},{}],[{},{}]],\"kind\":\"{:?}\"}}", a.x, a.y, b.x, b.y, s.kind).unwrap();
            }
        }
    }
    println!("--- parks (the city's count)");
    let mut parks: Vec<_> = w.parks.parks.iter().collect();
    parks.sort_by(|a, b| b.1.area.total_cmp(&a.1.area));
    for (a, s) in parks.iter().take(args.get(6).and_then(|s| s.parse().ok()).unwrap_or(12)) {
        let lots = w.park_lots(**a);
        let c = lots.iter().filter_map(|l| w.lots.get(*l)).map(|l| l.center).sum::<glam::Vec2>() / lots.len().max(1) as f32;
        let feats: Vec<&str> = lots.iter().filter_map(|l| w.lots.get(*l)).filter_map(|l| l.park.as_ref()).flat_map(|p| p.features.iter().map(|f| f.kind.label())).collect();
        let spaces: usize = lots.iter().filter_map(|l| w.lots.get(*l)).filter_map(|l| l.park.as_ref()).map(|p| p.car_park.as_ref().map_or(0, |c| c.spaces.len())).sum();
        println!("{:?} '{}' {} lots {:.0} m2 at ({:.0},{:.0}) members {} | {:?} | car park spaces {}", a, s.name, lots.len(), s.area, c.x, c.y, s.members.len(), feats, spaces);
    }
}
