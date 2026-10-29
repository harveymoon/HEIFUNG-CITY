use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset, WaterKind};
use crate::mind::IdleMind;

fn valley(seed: u64) -> World {
    World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, seed), ..WorldConfig::default() })
}

/// A point on the main river inside the basin, its flow direction, the
/// unit vector from it towards the middle of the map, and its width.
fn river_spot(w: &World) -> (Vec2, Vec2, Vec2, f32) {
    let r = &w.landscape.rivers[0];
    let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
    let (seg, _, _) = r.nearest(p);
    let flow = r.flow(seg);
    let mut n = Vec2::new(-flow.y, flow.x);
    if n.dot(-p) < 0.0 {
        n = -n;
    }
    (p, flow, n, r.width)
}

#[test]
fn worlds_from_the_same_landscape_are_the_same() {
    let (a, b) = (valley(9), valley(9));
    assert_eq!(a.landscape.rivers.len(), b.landscape.rivers.len());
    assert_eq!(a.landscape.rivers[0].points, b.landscape.rivers[0].points);
    assert_eq!(a.road_z, b.road_z);
    for k in 0..20 {
        let p = Vec2::new(k as f32 * 90.0 - 900.0, (k * 7 % 11) as f32 * 150.0 - 800.0);
        assert_eq!(a.ground_height(p), b.ground_height(p));
    }
    assert_ne!(valley(10).landscape.rivers[0].points, a.landscape.rivers[0].points);
}

#[test]
fn no_lots_on_water_and_bridges_where_roads_cross_it() {
    let mut w = valley(3);
    let (p, flow, n, width) = river_spot(&w);
    let treasury = w.economy.treasury;
    // A road across the river, and a zone straddling it.
    let a = p + n * 160.0;
    let b = p - n * 160.0;
    w.add_road(&[a, b], 1, 1);
    let bridges: Vec<SegmentId> = w.roads.alive_segments().filter(|(_, s)| s.bridge && s.lanes() == 2).map(|(id, _)| id).collect();
    assert_eq!(bridges.len(), 1, "one bridge span over the river");
    let (b0, b1) = w.roads.segment_points(bridges[0]);
    assert!(b0.distance(b1) >= width && b0.distance(b1) < width + 30.0, "the span is the river plus abutments: {}", b0.distance(b1));
    assert!(w.road_height(bridges[0], 0.5) >= WATER_LEVEL + BRIDGE_CLEARANCE - 0.01, "the deck clears the water");
    assert!(w.surface_height(p) > 5.0, "a car on the bridge is on the deck");
    assert!(w.economy.treasury < treasury - width * BRIDGE_COST_PER_M * 0.9, "the bridge was paid for");
    let (m, cost) = w.bridge_cost(&[a, b]);
    assert!(m >= width && cost > 0.0);
    // Gentle grades: no street segment steeper than its limit.
    for (id, s) in w.roads.alive_segments().filter(|(_, s)| s.is_street()) {
        let (pa, pb) = w.roads.segment_points(id);
        let dz = (w.road_z[s.a.0 as usize] - w.road_z[s.b.0 as usize]).abs();
        // [r4-hillside] Each within its own grade (a local street on the
        // land climbs to `LOCAL_STREET_GRADE`, a deck's approach 7%).
        assert!(dz <= pa.distance(pb) * super::segment_grade(&w.roads, s) + 0.05, "grade on {id:?}");
    }
    let zone = vec![p + n * 150.0 - flow * 120.0, p + n * 150.0 + flow * 120.0, p - n * 150.0 + flow * 120.0, p - n * 150.0 - flow * 120.0];
    w.add_zone(zone, LandUse::Residential);
    assert!(w.lots.values().filter(|l| l.usable()).count() >= 6, "lots on both banks");
    for l in w.lots.values() {
        assert!(!w.landscape.is_water(l.center), "lot centre in the river at {:?}", l.center);
        let wet = l.polygon.iter().filter(|q| w.landscape.water_at(**q).is_some_and(|h| h.kind == WaterKind::River) && w.landscape.rivers[0].nearest(**q).2 < width * 0.5 - 2.0).count();
        assert_eq!(wet, 0, "lot outline reaches into the river");
    }
    // Lots sit on level pads at their street's height.
    let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
    let pad = w.lot_pad(lot);
    assert!((w.ground_height(w.lots[lot].center) - pad).abs() < 0.3, "the ground under a lot is its pad");
}

#[test]
fn the_highway_is_there_from_the_start_with_through_traffic_and_ramps_join_it() {
    let mut w = valley(4);
    let hw: Vec<&crate::roads::Segment> = w.roads.alive_segments().map(|(_, s)| s).filter(|s| s.is_highway()).collect();
    assert!(!hw.is_empty(), "highway carriageways exist");
    assert!(hw.iter().any(|s| s.bridge), "the highway bridges the river");
    let (entries, exits) = w.highway_ends();
    assert_eq!((entries.len(), exits.len()), (2, 2), "two carriageways, each in from and out to elsewhere");
    let mut minds = IdleMind;
    let mut seen = 0;
    for _ in 0..60 {
        w.tick(&mut minds);
        seen = seen.max(w.through_cars());
    }
    assert!(seen >= 4, "through traffic flows: {seen}");
    assert!(w.traffic.cars.values().any(|c| World::is_through_car(c) && c.v > 15.0), "at highway speed");
    // A street crossing the highway passes over it: no junction.
    w.add_road(&[Vec2::new(60.0, 300.0), Vec2::new(60.0, 540.0)], 1, 1);
    let crossing = w.roads.nodes.iter().any(|n| n.pos.distance(Vec2::new(60.0, HIGHWAY_Y + HIGHWAY_HALF_GAP)) < 2.0 && n.segments.len() >= 3);
    assert!(!crossing, "streets fly over the highway");
    // An on-ramp: a one-way lane from the street dropped onto the eastbound carriageway.
    w.add_road(&[Vec2::new(60.0, 470.0), Vec2::new(150.0, HIGHWAY_Y + HIGHWAY_HALF_GAP + 4.0)], 1, 0);
    let merge = w.roads.node_near(Vec2::new(150.0, HIGHWAY_Y + HIGHWAY_HALF_GAP), 6.0).expect("the ramp end joined the carriageway");
    assert_eq!(w.roads.node(merge).segments.iter().filter(|&&s| w.roads.segment(s).alive).count(), 3);
    let inter = w.traffic.net.intersections.iter().find(|i| i.node == merge).expect("a merge");
    assert!(!inter.signalized, "ramps merge without lights");
    let from = w.roads.node_near(Vec2::new(60.0, 300.0), 2.0).unwrap();
    let (_, exits) = w.highway_ends();
    assert!(exits.iter().any(|&(e, _)| w.roads.path(from, e).is_some()), "the city reaches elsewhere by the ramp");
}

#[test]
fn hydro_only_beside_a_river_with_a_current() {
    let mut w = valley(6);
    w.economy.treasury = 500_000.0;
    let (p, flow, n, width) = river_spot(&w);
    w.landscape.rivers[0].current = 1.5;
    let road_at = p + n * (width * 0.5 + 45.0);
    w.add_road(&[road_at - flow * 200.0, road_at + flow * 200.0], 1, 1);
    let zone = vec![p + n * (width * 0.5 + 2.0) - flow * 150.0, p + n * (width * 0.5 + 2.0) + flow * 150.0, p + n * (width * 0.5 + 90.0) + flow * 150.0, p + n * (width * 0.5 + 90.0) - flow * 150.0];
    w.add_zone(zone, LandUse::Industrial);
    // A lot far from any water.
    w.add_road(&[Vec2::new(-100.0, -100.0), Vec2::new(100.0, -100.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-90.0, -90.0), Vec2::new(90.0, -90.0), Vec2::new(90.0, -20.0), Vec2::new(-90.0, -20.0)], LandUse::Industrial);
    let dry = w.lots.iter().filter(|(_, l)| l.usable() && l.center.length() < 150.0 && l.area() >= 300.0).map(|(id, _)| id).next().expect("a dry lot");
    assert!(w.power_site(dry, PowerPlantKind::Hydro).is_err());
    assert!(w.build_power(dry, PowerPlantKind::Hydro).is_err(), "no hydro away from water");
    assert!(w.build_power(dry, PowerPlantKind::Tidal).is_err(), "no tide in a valley");
    let near_river = |w: &World| {
        w.lots
            .iter()
            .filter(|(_, l)| l.usable() && l.area() >= 300.0 && l.land_use == LandUse::Industrial && l.center.distance(p) < 200.0)
            .min_by(|a, b| w.landscape.rivers[0].nearest(a.1.center).2.total_cmp(&w.landscape.rivers[0].nearest(b.1.center).2))
            .map(|(id, _)| id)
    };
    let by_river = near_river(&w).expect("a riverside lot after the rebuild");
    assert!(w.power_site(by_river, PowerPlantKind::Hydro).is_ok(), "{:?}", w.power_site(by_river, PowerPlantKind::Hydro));
    let plant = w.build_power(by_river, PowerPlantKind::Hydro).expect("hydro by the river");
    assert_eq!(w.buildings[plant].plant, Some(PowerPlantKind::Hydro));
    w.allocate_power();
    assert!(w.economy.power.supply_mw > 50.0, "the river turns the turbines: {}", w.economy.power.supply_mw);
    // Slow water will not do.
    w.landscape.rivers[0].current = 0.3;
    let other = w.lots.iter().filter(|(id, l)| *id != by_river && l.usable() && l.building.is_none() && l.area() >= 300.0 && w.landscape.river_beside(&l.polygon, 30.0).is_some()).map(|(id, _)| id).next();
    if let Some(other) = other {
        assert!(w.build_power(other, PowerPlantKind::Hydro).unwrap_err().contains("too slow"));
    }
    // Geothermal only over a hot spot.
    assert!(w.power_site(dry, PowerPlantKind::Geothermal).is_err() || w.landscape.hot_spot_at(w.lots[dry].center).is_some());
}

#[test]
fn docks_go_on_the_shore_build_a_pier_and_ships_call() {
    let mut w = valley(8);
    w.economy.treasury = 500_000.0;
    let (p, flow, n, width) = river_spot(&w);
    // Away from the water: refused.
    assert!(w.place_port(PortKind::ShippingPort, Vec2::new(0.0, 0.0)).is_err());
    // A city street and some homes nearby so the dock connects.
    let road_at = p + n * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
    w.add_zone(vec![road_at + n * 10.0 - flow * 100.0, road_at + n * 10.0 + flow * 100.0, road_at + n * 80.0 + flow * 100.0, road_at + n * 80.0 - flow * 100.0], LandUse::Residential);
    let click = p + n * (width * 0.5 + 20.0);
    let id = w.place_port(PortKind::ShippingPort, click).expect("a port on the river bank");
    let dock = w.ports.port_of(id).unwrap().clone();
    assert!(w.landscape.is_water(dock.pier[1]), "the pier reaches the water");
    assert!(!w.landscape.is_water(dock.center), "the yard is on land");
    // The stub joins the city street.
    let [s0, _] = dock.road_line();
    w.add_road(&[s0, road_at - flow * 300.0], 1, 1);
    assert!(w.ports.port_of(id).unwrap().reaches_elsewhere, "the river runs off the map");
    assert!(w.ports.port_of(id).unwrap().road_connected, "the dock's street reaches the city");
    // A ferry terminal on the same bank a little way along.
    let ferry = w.place_port(PortKind::FerryTerminal, click + flow * 170.0);
    // Wherever it lands, a second dock never overlaps the first.
    if let Ok(f) = ferry {
        let fp = w.ports.port_of(f).unwrap().footprint();
        assert!(!polygons_overlap(&fp, &dock.footprint()));
    }
    // A lake is no place for a shipping port.
    if let Some(lake) = w.landscape.lakes.first() {
        let edge = lake.outline[0] + (lake.outline[0] - lake.center).normalize() * 20.0;
        if let Err(e) = w.plan_port(PortKind::ShippingPort, edge) {
            assert!(e.contains("lake") || e.contains("edge") || e.contains("ragged") || e.contains("steep"), "{e}");
        }
    }
    // A freighter calls.
    let mut minds = IdleMind;
    let mut sailed = false;
    for _ in 0..600 {
        w.tick(&mut minds);
        if w.ports.ships.values().any(|s| s.kind == ShipKind::Freighter) {
            sailed = true;
            break;
        }
    }
    assert!(sailed, "a freighter sails for the port");
    let ship = w.ports.ships.values().find(|s| s.kind == ShipKind::Freighter).unwrap();
    let (start, _) = ship.pose_at(0.0);
    assert!(start.x.abs() > crate::landscape::LAND_HALF - 100.0 || start.y.abs() > crate::landscape::LAND_HALF - 100.0, "it comes in from off the map: {start:?}");
    assert!(ship.pose_at(ship.berth_s).0.distance(dock.berth) < 1.0, "it ties up at the berth");
}

/// [berthing] G6: a freighter turns and lies broadside along the quay at
/// the berth, and the crane lands its imports and loads the exports while
/// it stands there, box by box, as many as it carries.
#[test]
fn freighters_berth_broadside_and_the_crane_works_their_cargo() {
    let mut w = valley(8);
    w.economy.treasury = 500_000.0;
    let (p, flow, n, width) = river_spot(&w);
    let road_at = p + n * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
    w.add_zone(vec![road_at + n * 10.0 - flow * 100.0, road_at + n * 10.0 + flow * 100.0, road_at + n * 80.0 + flow * 100.0, road_at + n * 80.0 - flow * 100.0], LandUse::Residential);
    let click = p + n * (width * 0.5 + 20.0);
    let id = w.place_port(PortKind::ShippingPort, click).expect("a port on the river bank");
    let [s0, _] = w.ports.port_of(id).unwrap().road_line();
    w.add_road(&[s0, road_at - flow * 300.0], 1, 1);
    // Exports waiting on the quay; the next ship takes three boxes.
    w.ports.outbound = 3.0 * crate::ports::CONTAINER_UNITS + 5.0;
    let dock = w.ports.port_of(id).unwrap().clone();
    let mut minds = IdleMind;
    let mut sid = None;
    for _ in 0..600 {
        w.tick(&mut minds);
        sid = w.ports.ships.iter().find(|(_, s)| s.kind == ShipKind::Freighter).map(|(k, _)| k);
        if sid.is_some() {
            break;
        }
    }
    let sid = sid.expect("a freighter sails");
    let ship = w.ports.ships[sid].clone();
    assert_eq!(ship.boxes_out, 3, "it loads the exports by the box");
    assert!(w.ports.outbound < crate::ports::CONTAINER_UNITS, "taken off the quay");
    // Broadside at the berth, along the shore; nosing in further out.
    let (at, dir) = ship.pose_at(ship.berth_s);
    assert!(at.distance(dock.berth) < 1.0);
    assert!(dir.dot(dock.along).abs() > 0.99, "lies along the quay: {dir:?} vs {:?}", dock.along);
    let x = ship.berth_s - crate::ports::BERTH_TURN - 5.0;
    assert_eq!(ship.pose_at(x), crate::geom::point_along(&ship.path, x), "on its course further out");
    let half = ship.pose_at(ship.berth_s - crate::ports::BERTH_TURN * 0.5);
    assert!(half.0.distance(at) > 1.0, "still coming in, turning");
    // At the berth: the crane's moves, first landing then loading.
    let mut ship = ship;
    ship.boxes_in = 2;
    ship.s = ship.berth_s;
    ship.docked = true;
    let mut seen = Vec::new();
    let mut carried = false;
    for left in (1..=crate::ports::FREIGHTER_DWELL).rev() {
        ship.dwell = left;
        for f in [0.0, 0.25, 0.5, 0.75] {
            if let Some(c) = crate::ports::crane_work(&dock, &ship, f) {
                assert_eq!(c.of, 5);
                carried |= c.carrying;
                if seen.last() != Some(&(c.n, c.landing)) {
                    seen.push((c.n, c.landing));
                }
            }
        }
    }
    assert_eq!(seen, vec![(0, true), (1, true), (2, false), (3, false), (4, false)], "two landed, then three loaded");
    assert!(carried);
    // Nothing to move, the crane stands.
    ship.boxes_in = 0;
    ship.boxes_out = 0;
    ship.dwell = 30;
    assert!(crate::ports::crane_work(&dock, &ship, 0.0).is_none());
}

#[test]
fn landscapes_and_docks_survive_a_save_and_old_saves_load_flat() {
    let mut w = valley(12);
    w.economy.treasury = 500_000.0;
    let (p, _, n, width) = river_spot(&w);
    let _ = w.place_port(PortKind::FerryTerminal, p + n * (width * 0.5 + 18.0));
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.landscape.params, w.landscape.params);
    assert_eq!(back.landscape.rivers[0].points, w.landscape.rivers[0].points);
    assert_eq!(back.landscape.lakes.len(), w.landscape.lakes.len());
    assert_eq!(back.ports.ports.len(), w.ports.ports.len());
    assert_eq!(back.road_z.len(), back.roads.nodes.len(), "road heights rebuilt on load");
    assert_eq!(back.config.landscape, back.landscape.params);
    let q = p + n * 300.0;
    assert!((back.ground_height(q) - w.ground_height(q)).abs() < 1e-3);
    // A save from before landscapes: no landscape field at all.
    let mut old = World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::default() });
    old.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    let text = old.to_ron().unwrap();
    let start = text.find("\n    landscape: (").expect("landscape field") + 1;
    let mut depth = 0;
    let mut end = start;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let stripped = format!("{}{}", &text[..start], text[end..].trim_start_matches(','));
    let loaded = World::from_ron(&stripped).expect("an old save loads");
    assert!(loaded.landscape.is_flat());
    assert_eq!(loaded.ground_height(Vec2::new(20.0, 30.0)), 0.0);
    assert_eq!(loaded.surface_height(Vec2::new(20.0, 0.0)), 0.0);
}

/// [ferries] (D6) A large terminal runs car ferries with cars on deck, and
/// a terminal gives its estimate before it is built.
#[test]
fn a_large_ferry_terminal_runs_car_ferries_and_terminals_are_estimated() {
    let mut w = valley(8);
    w.economy.treasury = 500_000.0;
    let (p, flow, n, width) = river_spot(&w);
    let road_at = p + n * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
    w.add_zone(vec![road_at + n * 10.0 - flow * 100.0, road_at + n * 10.0 + flow * 100.0, road_at + n * 80.0 + flow * 100.0, road_at + n * 80.0 - flow * 100.0], LandUse::Residential);
    let click = p + n * (width * 0.5 + 24.0);
    let est = w.ferry_estimate(PortKind::FerryTerminalLarge, click).or_else(|| w.ferry_estimate(PortKind::FerryTerminal, click)).expect("an estimate where a terminal fits");
    assert!(est.sailings > 0 && est.riders > 0, "{est:?}");
    assert!(est.text().contains("riders"));
    assert!(w.ferry_estimate(PortKind::FerryTerminal, road_at + n * 60.0).is_none(), "away from the water: none");
    let Ok(id) = w.place_port(PortKind::FerryTerminalLarge, click) else { return };
    assert!(w.ports.port_of(id).unwrap().kind.car_deck() > 0);
    let [s0, _] = w.ports.port_of(id).unwrap().road_line();
    w.add_road(&[s0, road_at - flow * 300.0], 1, 1);
    let mut minds = IdleMind;
    let mut ferry = None;
    for _ in 0..1500 {
        w.tick(&mut minds);
        if let Some(s) = w.ports.ships.values().find(|s| s.port == id) {
            ferry = Some((s.kind, s.cars, s.passengers));
            break;
        }
    }
    let (kind, cars, _) = ferry.expect("a car ferry sails");
    assert_eq!(kind, ShipKind::CarFerry);
    assert!(cars <= PortKind::FerryTerminalLarge.car_deck());
}
