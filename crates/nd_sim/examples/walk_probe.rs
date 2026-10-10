//! [pt-walk] Load a save and report the walk graph's components, stranded
//! areas, failed trips, park access at given points and where wanderers
//! are headed. Not part of the game.
//! `walk_probe <save.ron> [ticks] [x,y ...]`; env COMPS=1 (stranded
//! components), ISLE=<node>,_ (bridges/tunnels on the drive to town),
//! SEG=<id>,<id> (paths' nodes and heights).
use glam::Vec2;
use nd_minds::{HybridMind, MockBackend};
use nd_sim::citizens::{Activity, Purpose};
use nd_sim::World;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let mut w = World::from_ron(&text).expect("load");
    let ticks: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let points: Vec<Vec2> = args.iter().skip(3).filter_map(|a| {
        let (x, y) = a.split_once(',')?;
        Some(Vec2::new(x.parse().ok()?, y.parse().ok()?))
    }).collect();
    w.refresh_connectivity();
    w.update_land_value();
    let mut sizes: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, c) in w.walk.comp.iter().enumerate() {
        if !w.walk.adj[i].is_empty() {
            *sizes.entry(*c).or_default() += 1;
        }
    }
    let mut v: Vec<_> = sizes.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    let isolated = w.walk.adj.iter().filter(|a| a.is_empty()).count();
    println!("walk nodes {} comps {} isolated {} main {:?} top {:?}", w.walk.nodes.len(), v.len(), isolated, w.connectivity.walk_main, &v[..v.len().min(8)]);
    for a in w.stranded_areas() {
        println!("  AREA {:?} comp {} at ({:.0},{:.0}) {}", a.net, a.comp, a.center.x, a.center.y, a.describe(&w));
    }
    // Small stranded walk components with a building: what they are.
    if std::env::var("COMPS").is_ok() {
        for a in w.stranded_areas().iter().filter(|a| a.net == nd_sim::world::connectivity::Net::Walk) {
            let nodes: Vec<usize> = (0..w.walk.nodes.len()).filter(|&i| w.walk.comp[i] == a.comp && !w.walk.adj[i].is_empty()).collect();
            let main = w.connectivity.walk_main.unwrap();
            let mut best = (f32::INFINITY, 0usize, 0usize);
            for &i in &nodes {
                let p = w.walk.nodes[i].pos;
                for j in w.walk.nodes_in_box(p - Vec2::splat(60.0), p + Vec2::splat(60.0)) {
                    if w.walk.comp[j as usize] == main && !w.walk.adj[j as usize].is_empty() {
                        let d = w.walk.nodes[j as usize].pos.distance(p);
                        if d < best.0 { best = (d, i, j as usize); }
                    }
                }
            }
            let lots: Vec<String> = a.lots.iter().map(|l| { let lot = &w.lots[*l]; format!("{l:?} {:?} landlocked {} front_seg {} access_node {:?}", lot.land_use, lot.landlocked, lot.front_segment.0, lot.access_node) }).collect();
            println!("COMP {} nodes {} nearest main node {:.1} m ({:?} -> {:?}) lots {:?}", a.comp, nodes.len(), best.0, w.walk.nodes[best.1].pos, w.walk.nodes[best.2].pos, lots);
            let segs: std::collections::BTreeSet<u32> = a.lots.iter().map(|l| w.lots[*l].front_segment.0).collect();
            for s in segs {
                let sg = w.roads.segment(nd_sim::roads::SegmentId(s));
                println!("   seg {s} kind {:?} bridge {} viaduct {} tunnel {} ramp {} highway {} fronts_nothing {} alive {}", sg.kind, sg.bridge, sg.viaduct, sg.tunnel, sg.ramp, sg.is_highway(), sg.fronts_nothing(), sg.alive);
            }
        }
    }
    if let Ok(v) = std::env::var("ISLE") {
        let (a, _) = v.split_once(',').unwrap();
        let near = |p: Vec2| nd_sim::roads::NodeId(w.roads.nodes.iter().enumerate().filter(|(_, n)| !n.segments.is_empty()).min_by(|x, y| x.1.pos.distance(p).total_cmp(&y.1.pos.distance(p))).unwrap().0 as u32);
        // From road node `a` to the node nearest the old town centre.
        let (a, b) = (nd_sim::roads::NodeId(a.parse().unwrap()), near(Vec2::new(-175.0, 655.0)));
        if let Some(path) = w.roads.path(a, b) {
            for p in path.windows(2) {
                if let Some((_, sid)) = w.roads.neighbors(p[0]).find(|(n, _)| *n == p[1]) {
                    let sg = w.roads.segment(sid);
                    if sg.fronts_nothing() || sg.bridge || sg.tunnel {
                        println!("ISLE seg {} at {:?} bridge {} viaduct {} tunnel {} ramp {} highway {}", sid.0, w.roads.node(p[0]).pos, sg.bridge, sg.viaduct, sg.tunnel, sg.ramp, sg.is_highway());
                    }
                }
            }
        }
    }
    if let Ok(v) = std::env::var("SEG") {
        for id in v.split(',').filter_map(|x| x.parse::<u32>().ok()) {
            let sid = nd_sim::roads::SegmentId(id);
            let Some(sg) = w.roads.try_segment(sid) else { continue };
            println!("SEG {id} kind {:?} alive {} bridge {} viaduct {}", sg.kind, sg.alive, sg.bridge, sg.viaduct);
            for n in [sg.a, sg.b] {
                let node = w.roads.node(n);
                let others: Vec<String> = node.segments.iter().filter(|&&s| s != sid && w.roads.segment(s).alive).map(|&s| { let o = w.roads.segment(s); format!("{}:{:?}", s.0, o.kind) }).collect();
                println!("   node {} at ({:.1},{:.1}) level {:.2} z {:.2} ground {:.2} others {:?}", n.0, node.pos.x, node.pos.y, node.level, w.road_z.get(n.0 as usize).copied().unwrap_or(0.0), w.ground_height(node.pos), others);
            }
        }
    }
    for p in &points {
        let k = w.walk.nearest_within(*p, 48.0);
        let acc = w.land_value.access_at(*p).map(|a| a.describe());
        println!("  POINT ({:.0},{:.0}) node {:?} edges {:?} comp {:?} access {:?}", p.x, p.y, k, k.map(|k| w.walk.adj[k as usize].len()), k.and_then(|k| w.walk.component(k)), acc);
        let lot = w.lots.iter().find(|(_, l)| nd_sim::geom::point_in_polygon(*p, &l.polygon));
        if let Some((lid, l)) = lot {
            let b = l.building.and_then(|b| w.buildings.get(b));
            println!("     lot {lid:?} {:?} center ({:.0},{:.0}) access@center {:?} building {:?} operating {:?}", l.land_use, l.center.x, l.center.y, w.land_value.access_at(l.center).map(|a| a.describe()), b.map(|b| &b.name), b.map(|b| b.operating()));
        }
        // Nearest parks.
        let mut parks: Vec<(f32, String, bool)> = w.buildings.values().filter(|b| b.land_use == nd_sim::LandUse::Park).filter_map(|b| w.lots.get(b.lot).map(|l| (l.center.distance(*p), b.name.clone(), b.operating()))).collect();
        parks.sort_by(|a, b| a.0.total_cmp(&b.0));
        println!("     parks near {:?}", &parks[..parks.len().min(3)]);
    }
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
    if ticks > 0 {
        println!("after {ticks} ticks: failed {} alerts {}", w.connectivity.failed.len(), w.connectivity.alerts.len());
        for a in &w.connectivity.alerts {
            println!("  ALERT {}", a.text);
        }
        for f in w.connectivity.failed.iter().rev().take(10) {
            println!("  FAIL {:?} ({:.0},{:.0}) -> {:?}: {}", f.citizen, f.from.x, f.from.y, w.buildings.get(f.target).map(|b| &b.name), f.why);
        }
    }
    // Wanderers: where they are going, by land use of the lot nearest the end.
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    let mut dist = Vec::new();
    for c in w.citizens.values() {
        if let Activity::Traveling { purpose: Purpose::Wander, path, .. } = &c.activity {
            let Some(end) = path.last() else { continue };
            dist.push(c.pos.distance(*end));
            let lu = w.lots.values().min_by(|a, b| a.center.distance(*end).total_cmp(&b.center.distance(*end))).map(|l| format!("{:?}", l.land_use)).unwrap_or_default();
            *by.entry(lu).or_default() += 1;
        }
    }
    dist.sort_by(|a, b| a.total_cmp(b));
    println!("wanderers {} heading to {:?} median dist {:?}", dist.len(), by, dist.get(dist.len() / 2));
}
