//! Headless jam probe: why every standing vehicle stands, and the heads
//! of the queues (`Traffic::stop_reason`, `Traffic::jam_chain`). Runs a
//! save (`jam_probe <save.ron> [ticks] [every]`) or the demo district
//! (`jam_probe demo [ticks] [every]`). Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{Gesture, LandUse, Vec2 as SVec2, World, WorldConfig};
use std::collections::{BTreeMap, HashMap};

fn seed(world: &mut World) {
    world.drain_events();
    world.start_project();
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![SVec2::new(-280.0, c), SVec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() });
        world.add_gesture(Gesture::Road { points: vec![SVec2::new(c, -280.0), SVec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() });
    }
    let cell = |cx: f32, cy: f32| vec![SVec2::new(cx - 65.0, cy - 65.0), SVec2::new(cx + 65.0, cy - 65.0), SVec2::new(cx + 65.0, cy + 65.0), SVec2::new(cx - 65.0, cy + 65.0)];
    let plan = [
        (-210.0, -210.0, LandUse::Industrial), (-70.0, -210.0, LandUse::Industrial), (70.0, -210.0, LandUse::Residential), (210.0, -210.0, LandUse::Residential),
        (-210.0, -70.0, LandUse::Commercial), (-70.0, -70.0, LandUse::Entertainment), (70.0, -70.0, LandUse::Commercial), (210.0, -70.0, LandUse::Residential),
        (-210.0, 70.0, LandUse::Residential), (-70.0, 70.0, LandUse::Park), (70.0, 70.0, LandUse::Civic), (210.0, 70.0, LandUse::Commercial),
        (-210.0, 210.0, LandUse::Residential), (-70.0, 210.0, LandUse::Residential), (70.0, 210.0, LandUse::Entertainment), (210.0, 210.0, LandUse::Residential),
    ];
    for (x, y, lu) in plan {
        world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
    }
    world.add_gesture(Gesture::Road { points: vec![SVec2::new(-420.0, nd_sim::rail::TRUNK_Y), SVec2::new(-330.0, -410.0), SVec2::new(-280.0, -348.0), SVec2::new(-110.0, -348.0), SVec2::new(-70.0, -324.0), SVec2::new(150.0, -324.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: nd_sim::roads::RoadKind::Rail, elevation: Vec::new() });
    world.implement();
    world.economy.treasury = world.economy.treasury.max(150_000.0);
    // The district's way in: a street north from the grid onto the highway.
    world.link_to_highway(SVec2::new(0.0, 280.0));
    use nd_sim::RailFacility;
    let _ = world.place_rail_facility(RailFacility::FreightYard, SVec2::new(-200.0, -330.0));
    let _ = world.place_rail_facility(RailFacility::Station, SVec2::new(80.0, -310.0));
    let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_power(lot, nd_sim::PowerPlantKind::Coal);
    }
    let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.car_access() && l.area() >= nd_sim::transit::DEPOT_MIN_AREA).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_depot(lot);
    }
    let line = world.add_bus_route();
    for p in [(-70.0, -136.0), (70.0, -136.0), (136.0, -70.0), (136.0, 70.0), (70.0, 136.0), (-70.0, 136.0), (-136.0, 70.0), (-136.0, -70.0)] {
        let _ = world.add_bus_stop(line, SVec2::new(p.0, p.1));
    }
    world.set_route_buses(line, 2);
}


fn report(w: &World, label: &str) {
    let t = &w.traffic;
    let mut why: BTreeMap<&str, usize> = BTreeMap::new();
    let mut heads: HashMap<nd_sim::CarId, (usize, bool)> = HashMap::new();
    let standing: Vec<nd_sim::CarId> = t.cars.iter().filter(|(_, c)| c.v < 0.5 && c.stalled > std::env::var("STALL").ok().and_then(|s| s.parse().ok()).unwrap_or(60.0)).map(|(id, _)| id).collect();
    for &id in &standing {
        let r = t.stop_reason(id);
        *why.entry(r.key()).or_default() += 1;
        let chain = t.jam_chain(id);
        if let Some(h) = chain.head() {
            let e = heads.entry(h).or_default();
            e.0 = e.0.max(chain.cars.len());
            e.1 |= chain.cycle;
        }
    }
    println!("   angry now {} road rage today {} crashes today {} timed {} over2x {}", w.angry_drivers_now(), w.driving.road_rage_today, w.driving.crashes_today, t.cars.values().filter(|c| c.drive.expected_min > 0.0).count(), t.cars.values().filter(|c| c.drive.expected_min > 0.0 && (w.clock.tick - c.drive.trip_start) as f32 > 2.0 * c.drive.expected_min).count());
    println!("{label}: tick {} day {} {:02}:{:02} vehicles {} standing>60s {} {:?}", w.clock.tick, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, t.cars.len(), standing.len(), why);
    let mut hs: Vec<_> = heads.into_iter().filter(|(_, (n, _))| *n >= 3).collect();
    hs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (h, (n, cycle)) in hs.iter().take(12) {
        let c = &t.cars[*h];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        let name = |id: nd_sim::CarId| format!("{id:?}");
        println!(
            "   head {:?} queue {n}{} at ({:.0},{:.0}) v{:.1} stalled {:.0}s wait {:?} entering {} hold {:.0} lane {lane} conn {} s {:.1}/{:.1} idx {}/{} errand {:?} bus {} cargo {} -> {}",
            h, if *cycle { " (cycle)" } else { "" }, c.pos.x, c.pos.y, c.v, c.stalled, c.wait, c.entering, c.hold, l.is_connector(), c.s, l.length, c.idx, c.route.len(), c.errand, c.bus.is_some(), c.cargo.is_some(), t.stop_reason(*h).text(name)
        );
    }
    let mut lc: Vec<_> = standing.iter().copied().filter(|&id| matches!(t.stop_reason(id), nd_sim::traffic::StopReason::LaneChange { .. })).collect();
    lc.sort_by(|a, b| t.cars[*b].stalled.total_cmp(&t.cars[*a].stalled));
    for id in lc.into_iter().take(6) {
        let c = &t.cars[id];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        let tgt = c.route[c.idx + 1];
        let tl = &t.net.lanes[tgt as usize];
        let deg = w.roads.neighbors(l.end_node).count();
        let around: Vec<String> = tl.cars.iter().filter(|&&o| (t.cars[o].s - c.s).abs() < 30.0).map(|&o| { let x = &t.cars[o]; format!("{:?}@{:.1} v{:.1} {:?} st{:.0}", o, x.s, x.v, x.wait, x.stalled) }).collect();
        println!("   LANECHANGE {:?} at ({:.0},{:.0}) stalled {:.0} lane {lane} kind {:?} s {:.1}/{:.1} end node deg {} next {:?} -> target {tgt} ({} cars) next after {:?} replans {} cooldown {:.0} around {:?}", id, c.pos.x, c.pos.y, c.stalled, l.kind, c.s, l.length, deg, l.next, tl.cars.len(), c.route.get(c.idx + 2), c.replans, c.cooldown, around);
    }
    for &id in standing.iter().filter(|&&id| t.stop_reason(id) == nd_sim::traffic::StopReason::Unexplained).take(10) {
        let c = &t.cars[id];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        println!("   UNEXPLAINED {:?} at ({:.0},{:.0}) wait {:?} by {:?} stalled {:.0} lane {lane} conn {} s {:.1}/{:.1} idx {}/{} lane cars {} pos_in_lane {:?}", id, c.pos.x, c.pos.y, c.wait, c.blocked_by, c.stalled, l.is_connector(), c.s, l.length, c.idx, c.route.len(), l.cars.len(), l.cars.iter().position(|&o| o == id));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().unwrap_or_else(|| "demo".into());
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(120);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut w = if src == "demo" {
        let mut w = World::new(WorldConfig::default());
        seed(&mut w);
        w
    } else {
        World::from_ron(&std::fs::read_to_string(&src).unwrap()).expect("load")
    };
    report(&w, "start");
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % every == 0 {
            report(&w, &format!("+{}", i + 1));
        }
    }

}
