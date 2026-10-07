//! [mapgen] Shared checks for the generated example cities
//! (`nd_procgen::citygen`), one test file per preset so each stays well
//! under a couple of minutes: the generator is deterministic, the street
//! network it lays is one connected network with a way in from the
//! highway, has no degenerate pieces or junctions, zones every use onto
//! lots with few of them landlocked, and the town then runs headless with
//! the rule-based mind for a few days: people move in, businesses open,
//! and traffic never locks up for good.

#![allow(dead_code)]

use nd_minds::RuleBasedMind;
use nd_procgen::citygen::{self, CityPreset};
use nd_sim::roads::{NodeId, SegmentId};
use nd_sim::{LandUse, Vec2, World};

pub const SEEDS: [u64; 2] = [1, 2];
/// Days each city runs.
const DAYS: u32 = 4;
/// Shortest street between junctions (the generator aims for 22 m; the
/// game's snapping may move a junction a few metres).
const MIN_CHAIN: f32 = 14.0;
/// Sharpest angle between two streets at a junction.
const MIN_ANGLE_DEG: f32 = 24.0;

/// The generator is deterministic, different seeds make different
/// cities, and each has streets of every class.
pub fn check_deterministic(preset: CityPreset) {
    let mut plans = Vec::new();
    for seed in SEEDS {
        let land = nd_sim::Landscape::new(&preset.landscape(seed));
        let a = citygen::generate(preset, seed, &land);
        let b = citygen::generate(preset, seed, &nd_sim::Landscape::new(&preset.landscape(seed)));
        assert!(a == b, "{preset:?} seed {seed}: two runs differ");
        assert!(a.roads.len() > 30 && a.zones.len() > 100, "{preset:?} seed {seed}: {} roads, {} zones", a.roads.len(), a.zones.len());
        for class in [citygen::RoadClass::Arterial, citygen::RoadClass::Collector, citygen::RoadClass::Local] {
            assert!(a.road_length(class) > 1500.0, "{preset:?} seed {seed}: only {:.0} m of {class:?}", a.road_length(class));
        }
        plans.push(a);
    }
    assert!(plans[0] != plans[1], "{preset:?}: the seeds make the same city");
}

fn is_city_street(w: &World, s: SegmentId) -> bool {
    let seg = w.roads.segment(s);
    seg.alive && seg.is_street() && !seg.is_motorway()
}

/// Street segments (city streets and ramps) not connected to the highway,
/// ignoring direction. (The two carriageways are not joined to each other:
/// the interchanges serve the westbound one.)
fn unreached_streets(w: &World) -> Vec<Vec2> {
    let nodes = &w.roads.nodes;
    let mut seen = vec![false; nodes.len()];
    let mut stack: Vec<usize> = w.roads.alive_segments().filter(|(_, s)| s.is_highway()).flat_map(|(_, s)| [s.a.0 as usize, s.b.0 as usize]).collect();
    for &n in &stack {
        seen[n] = true;
    }
    while let Some(n) = stack.pop() {
        for &sid in &nodes[n].segments {
            let s = w.roads.segment(sid);
            if !s.alive || !s.is_street() {
                continue;
            }
            for o in [s.a.0 as usize, s.b.0 as usize] {
                if !seen[o] {
                    seen[o] = true;
                    stack.push(o);
                }
            }
        }
    }
    w.roads.alive_segments().filter(|(_, s)| s.is_street() && !seen[s.a.0 as usize]).map(|(_, s)| w.roads.node(s.a).pos).collect()
}

/// City street segments at a node.
fn streets_at(w: &World, n: NodeId) -> Vec<SegmentId> {
    w.roads.node(n).segments.iter().copied().filter(|&s| is_city_street(w, s)).collect()
}

/// (shortest street between junctions, sharpest junction angle in
/// degrees, shortest single piece) over the city's streets, each with
/// where it is.
fn geometry(w: &World) -> ((f32, Vec2), (f32, Vec2), (f32, Vec2)) {
    let other = |s: SegmentId, n: NodeId| {
        let seg = w.roads.segment(s);
        if seg.a == n { seg.b } else { seg.a }
    };
    let junction = |n: NodeId| w.roads.node(n).segments.iter().filter(|&&s| w.roads.segment(s).alive && w.roads.segment(s).is_street()).count() != 2;
    let mut shortest_chain = (f32::INFINITY, Vec2::ZERO);
    let mut sharpest = (180.0f32, Vec2::ZERO);
    let mut shortest_piece = (f32::INFINITY, Vec2::ZERO);
    let low = |best: &mut (f32, Vec2), v: f32, at: Vec2| {
        if v < best.0 {
            *best = (v, at);
        }
    };
    for (i, node) in w.roads.nodes.iter().enumerate() {
        let n = NodeId(i as u32);
        let segs = streets_at(w, n);
        if segs.is_empty() {
            continue;
        }
        for &s in &segs {
            low(&mut shortest_piece, w.roads.node(other(s, n)).pos.distance(node.pos), node.pos);
        }
        if !junction(n) {
            continue;
        }
        // Walk each street from this junction to the next.
        for &s in &segs {
            let (mut cur, mut at, mut len) = (s, n, 0.0);
            loop {
                let next = other(cur, at);
                len += w.roads.node(next).pos.distance(w.roads.node(at).pos);
                if junction(next) || next == n {
                    break;
                }
                let Some(&on) = streets_at(w, next).iter().find(|&&x| x != cur) else { break };
                cur = on;
                at = next;
            }
            low(&mut shortest_chain, len, node.pos);
        }
        if segs.len() >= 2 {
            let dirs: Vec<Vec2> = segs.iter().map(|&s| (w.roads.node(other(s, n)).pos - node.pos).normalize_or_zero()).collect();
            for a in 0..dirs.len() {
                for b in a + 1..dirs.len() {
                    low(&mut sharpest, dirs[a].dot(dirs[b]).clamp(-1.0, 1.0).acos().to_degrees(), node.pos);
                }
            }
        }
    }
    (shortest_chain, sharpest, shortest_piece)
}

/// Vehicles standing still for over four minutes of motion.
fn stalled_cars(w: &World) -> Vec<Vec2> {
    // On the map: through traffic queueing to come on far out of sight is not a lock-up in town.
    let edge = nd_sim::landscape::MAP_EDGE;
    w.traffic.cars.values().filter(|c| c.v < 0.5 && c.stalled > 240.0 && !c.entering && c.pos.x.abs() < edge && c.pos.y.abs() < edge).map(|c| c.pos).collect()
}

fn stalled(w: &World) -> usize {
    stalled_cars(w).len()
}

pub fn check_city(preset: CityPreset, seed: u64) {
    let t0 = std::time::Instant::now();
    let (mut world, plan, laid) = citygen::generate_world(preset, seed);
    let tag = format!("{preset:?} seed {seed}");
    println!("{tag}: {} roads, {} zones -> {laid:?} in {:.1}s", plan.roads.len(), plan.zones.len(), t0.elapsed().as_secs_f32());

    // The network.
    assert!(world.implement_refused.is_none(), "{tag}: {:?}", world.implement_refused);
    assert!(laid.links >= 1, "{tag}: no interchange");
    assert!(world.has_way_in(), "{tag}: no way in");
    let cut_off = unreached_streets(&world);
    assert!(cut_off.is_empty(), "{tag}: {} street pieces cut off from the highway, at {:?}", cut_off.len(), &cut_off[..cut_off.len().min(6)]);
    let (chain, angle, piece) = geometry(&world);
    println!("{tag}: shortest street between junctions {:.1} m at {}, sharpest junction {:.1} deg at {}, shortest piece {:.2} m at {}", chain.0, chain.1, angle.0, angle.1, piece.0, piece.1);
    assert!(chain.0 >= MIN_CHAIN, "{tag}: a street of {:.1} m between junctions at {}", chain.0, chain.1);
    assert!(angle.0 >= MIN_ANGLE_DEG, "{tag}: streets meet at {:.1} deg at {}", angle.0, angle.1);
    assert!(piece.0 >= 0.5, "{tag}: a road piece {:.2} m long at {}", piece.0, piece.1);

    // The zoning.
    let lots = world.lots.len();
    let landlocked = world.lots.values().filter(|l| l.landlocked).count();
    let count = |lu: LandUse| world.lots.values().filter(|l| l.land_use == lu).count();
    println!("{tag}: {lots} lots ({landlocked} landlocked): {:?}", LandUse::ALL.map(|lu| (lu.label(), count(lu))));
    assert!(lots > 800, "{tag}: only {lots} lots");
    assert!((landlocked as f32) < lots as f32 * 0.12, "{tag}: {landlocked} of {lots} lots landlocked");
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial, LandUse::Park, LandUse::Entertainment] {
        assert!(count(lu) >= 10, "{tag}: {} {lu:?} lots", count(lu));
    }
    assert!(count(LandUse::Residential) * 4 > lots, "{tag}: homes are a quarter of the town at least");

    // A few days of life.
    let mut minds = RuleBasedMind::default();
    let mut peak_stalled = 0;
    let mut last_day_stalled = 0;
    // ND_CITYGEN_DAYS runs longer, to look for trouble.
    let days = std::env::var("ND_CITYGEN_DAYS").ok().and_then(|d| d.parse().ok()).unwrap_or(DAYS);
    for day in 1..=days {
        for t in 0..1440 {
            world.tick(&mut minds);
            if t % 30 == 0 {
                let s = stalled(&world);
                peak_stalled = peak_stalled.max(s);
                if day == days {
                    last_day_stalled = last_day_stalled.max(s);
                }
            }
        }
        let open = world.buildings.values().filter(|b| b.construction.is_none()).filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).count();
        println!(
            "{tag} day {day}: pop {} households {} | businesses open {open} | buildings {} | cars {} stalled {} (peak {peak_stalled}) | {:.1}s",
            world.citizens.len(),
            world.economy.households,
            world.buildings.len(),
            world.traffic.cars.len(),
            stalled(&world),
            t0.elapsed().as_secs_f32()
        );
        let st = stalled_cars(&world);
        if st.len() >= 10 {
            println!("{tag} day {day}: stalled at {:?}", st.iter().map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>());
        }
    }
    let pop = world.citizens.len();
    let open = world.buildings.values().filter(|b| b.construction.is_none()).filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).count();
    assert!(pop > 300, "{tag}: only {pop} people moved in");
    assert!(open > 20, "{tag}: only {open} businesses open");
    // Queues come and go; a lock-up only grows.
    let cars = world.traffic.cars.len();
    assert!(peak_stalled <= 30 + cars / 5, "{tag}: {peak_stalled} vehicles stalled at the worst ({cars} on the road)");
    assert!(last_day_stalled <= 20 + cars / 8, "{tag}: {last_day_stalled} vehicles stalled on the last day ({cars} on the road), at {:?}", stalled_cars(&world));
}
