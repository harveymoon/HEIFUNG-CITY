//! [ferry-islands] Islands reached only by ferry: a town on one bank of a
//! river, homes on the other with no bridge, a ferry terminal on each.

use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::mind::IdleMind;
use crate::ports::PortKind;
use crate::transit::TransitLeg;
use crate::citizens::TripMode;

/// A point on the main river near the middle of the map, its flow, the
/// unit vector from it towards the middle, and its width.
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

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

/// Tick on to minute `m` of the day.
fn until(w: &mut World, m: u32) {
    let mut minds = IdleMind;
    while w.clock.minute_of_day() as u32 != m {
        w.tick(&mut minds);
    }
}

struct Isle {
    w: World,
    /// Terminals: on the town's bank and on the island's.
    town: BuildingId,
    isle: BuildingId,
    /// A home on the island and a shop in town, both open.
    home: BuildingId,
    shop: BuildingId,
    /// The island's side (unit vector away from the town).
    out: Vec2,
    at: Vec2,
    flow: Vec2,
}

fn lot_near(w: &World, p: Vec2, lu: LandUse) -> Option<LotId> {
    w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.building.is_none()).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id)
}

/// A street on the bank along the river with zones on its far side from
/// the water, and a terminal on the shore in front, joined to it.
fn bank(w: &mut World, p: Vec2, flow: Vec2, side: Vec2, width: f32, kind: PortKind, lu: [LandUse; 2]) -> BuildingId {
    let road_at = p + side * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
    w.add_zone(vec![road_at + side * 10.0 - flow * 280.0, road_at + side * 10.0 - flow * 20.0, road_at + side * 90.0 - flow * 20.0, road_at + side * 90.0 - flow * 280.0], lu[0]);
    w.add_zone(vec![road_at + side * 10.0 + flow * 20.0, road_at + side * 10.0 + flow * 280.0, road_at + side * 90.0 + flow * 280.0, road_at + side * 90.0 + flow * 20.0], lu[1]);
    let click = p + side * (width * 0.5 + 20.0 + if kind == PortKind::FerryTerminalLarge { 16.0 } else { 0.0 });
    let id = w.place_port(kind, click).expect("a terminal on the river bank");
    let [s0, s1] = w.ports.port_of(id).unwrap().road_line();
    // The access road's free end straight on to the street.
    let end = road_at + flow * (s1 - road_at).dot(flow).clamp(-280.0, 280.0);
    w.add_road(&[s1, end], 1, 1);
    let _ = s0;
    id
}

fn isle_with(kind: PortKind) -> Isle {
    let mut w = World::new(WorldConfig { emergencies: false, landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 8), ..WorldConfig::default() });
    w.economy.treasury = 1_000_000.0;
    let (p, flow, n, width) = river_spot(&w);
    let town = bank(&mut w, p, flow, n, width, kind, [LandUse::Residential, LandUse::Commercial]);
    let isle = bank(&mut w, p, flow, -n, width, kind, [LandUse::Residential, LandUse::Residential]);
    w.refresh_port_links();
    // Homes and a shop in town, a home on the island: open.
    let town_at = p + n * (width * 0.5 + 160.0);
    let isle_at = p - n * (width * 0.5 + 160.0);
    let mut homes = Vec::new();
    for k in 0..3 {
        let l = lot_near(&w, town_at - flow * (60.0 + 70.0 * k as f32), LandUse::Residential).expect("a home lot in town");
        w.build_on(l);
        homes.push(w.lots[l].building.unwrap());
    }
    let sl = lot_near(&w, town_at + flow * 80.0, LandUse::Commercial).expect("a shop lot in town");
    w.build_on(sl);
    let shop = w.lots[sl].building.unwrap();
    let hl = lot_near(&w, isle_at, LandUse::Residential).expect("a home lot on the island");
    w.build_on(hl);
    let home = w.lots[hl].building.unwrap();
    for b in homes.into_iter().chain([shop, home]) {
        w.finish_construction(b);
    }
    w.refresh_port_links();
    w.refresh_connectivity();
    Isle { w, town, isle, home, shop, out: -n, at: p, flow }
}

fn isle() -> Isle {
    isle_with(PortKind::FerryTerminal)
}

/// Someone at home on the island, on foot (moved in by hand: the test town
/// has no road in from elsewhere).
fn islander(s: &mut Isle) -> CitizenId {
    let w = &mut s.w;
    let door = w.building_entrance(s.home).unwrap();
    let tick = w.clock.tick;
    let mut c = Citizen::newcomer("Ida Strand".into(), 0x15_1A4D, 34, Some(s.home), None, door, tick);
    c.inside = Some(s.home);
    c.activity = Activity::AtHome;
    c.savings = 80.0;
    c.income = 100.0;
    c.next_decision_tick = u64::MAX;
    let id = w.citizens.insert(c);
    w.buildings[s.home].residents.push(id);
    id
}

/// Keep `id` from deciding anything of their own while a test drives them.
fn hold(w: &mut World, id: CitizenId) {
    if let Some(c) = w.citizens.get_mut(id) {
        c.next_decision_tick = u64::MAX;
        c.asleep = false;
        c.wakefulness = 1.0;
    }
}

#[test]
fn an_island_with_a_ferry_is_not_stranded() {
    let s = isle();
    let w = &s.w;
    assert_eq!(w.ports.links.len(), 1, "one crossing between the two terminals: {:?}", w.ports.links);
    let pi = w.ports.port_of(s.isle).unwrap();
    let pt = w.ports.port_of(s.town).unwrap();
    assert!(pi.works(), "the island terminal carries people: island road {} walk {} out {}; town road {} walk {}", pi.road_connected, pi.walk_connected, pi.reaches_elsewhere, pt.road_connected, pt.walk_connected);
    let islands = &w.connectivity.islands;
    assert_eq!(islands.len(), 1, "the far bank is an island: {islands:?}");
    assert_eq!(islands[0].terminals, vec![s.isle]);
    assert_eq!(islands[0].mainland, vec![s.town]);
    assert!(islands[0].buildings.contains(&s.home));
    let home_lot = w.buildings[s.home].lot;
    assert!(!w.lot_cut_off(home_lot, connectivity::Net::Walk), "not stranded");
    assert!(!w.stranded_areas().iter().any(|a| a.buildings.contains(&s.home)), "no stranded area for the island: {:?}", w.stranded_areas().iter().map(|a| a.describe(w)).collect::<Vec<_>>());
    let lines = w.lot_link_warnings(home_lot);
    assert!(lines.iter().any(|l| l.contains("on an island") && l.contains("passenger ferry")), "the inspector says how it is reached: {lines:?}");
    let info = w.islands();
    assert_eq!(info.len(), 1);
    assert!(info[0].sail_minutes > 0.0 && !info[0].fire, "{:?}", info[0]);
}

#[test]
fn an_islander_goes_to_town_by_ferry_and_back() {
    let mut s = isle();
    let id = islander(&mut s);
    until(&mut s.w, 8 * 60);
    hold(&mut s.w, id);
    let w = &mut s.w;
    let fares = w.ports.stats.ferry_fares_today;
    w.travel_to(id, Some(s.shop), Purpose::Shop(Resource::Goods));
    assert!(matches!(w.citizens[id].transit.map(|t| t.leg), Some(TransitLeg::Ferry { board, alight }) if board == s.isle && alight == s.town), "plans the crossing: {:?} ({})", w.citizens[id].transit, w.citizens[id].thought);
    assert!(matches!(w.citizens[id].activity, Activity::Traveling { purpose: Purpose::ToStop, .. }), "walks to the pier first");
    let (mut waited, mut sailed, mut there) = (false, false, false);
    let mut minds = IdleMind;
    for _ in 0..600 {
        w.tick(&mut minds);
        hold(w, id);
        match w.citizens[id].activity {
            Activity::Waiting { .. } => waited = true,
            Activity::OnFerry { from, to } => {
                assert_eq!((from, to), (s.isle, s.town));
                sailed = true;
            }
            _ => {}
        }
        if w.citizens[id].inside == Some(s.shop) {
            there = true;
            break;
        }
    }
    assert!(waited && sailed && there, "waited {waited}, sailed {sailed}, at the shop {there}: {} ({})", w.citizens[id].activity.label(), w.citizens[id].thought);
    assert!(w.ports.stats.own_trips_today >= 1 && w.ports.stats.ferry_fares_today > fares, "the crossing counted and paid");
    assert!(w.citizens[id].commute.trips[TripMode::Ferry.index()] > 0.0, "remembered as a ferry trip");
    // And home again the same way.
    w.travel_to(id, Some(s.home), Purpose::Home);
    assert!(matches!(w.citizens[id].transit.map(|t| t.leg), Some(TransitLeg::Ferry { board, alight }) if board == s.town && alight == s.isle), "home by ferry: {:?}", w.citizens[id].transit);
    let mut home = false;
    for _ in 0..600 {
        w.tick(&mut minds);
        hold(w, id);
        if w.citizens[id].inside == Some(s.home) {
            home = true;
            break;
        }
    }
    assert!(home, "home on the island: {} ({})", w.citizens[id].activity.label(), w.citizens[id].thought);
}

#[test]
fn after_the_last_boat_a_rider_waits_on_the_pier_for_the_first() {
    let mut s = isle();
    let id = islander(&mut s);
    // In town late: the boats stopped at ten.
    until(&mut s.w, 22 * 60 + 40);
    hold(&mut s.w, id);
    let w = &mut s.w;
    let door = w.building_entrance(s.shop).unwrap();
    {
        let c = &mut w.citizens[id];
        c.inside = Some(s.shop);
        c.activity = Activity::Visiting(s.shop);
        c.pos = door;
        c.prev_pos = door;
    }
    let wait = w.ferry_wait(s.town, s.isle, w.clock.minute_of_day() as u32).expect("a boat in the morning");
    assert!(wait > 6.0 * 60.0, "the first boat is hours off: {wait}");
    w.travel_to(id, Some(s.home), Purpose::Home);
    assert!(matches!(w.citizens[id].transit.map(|t| t.leg), Some(TransitLeg::Ferry { .. })), "the ferry is the only way home: {}", w.citizens[id].thought);
    let mut minds = IdleMind;
    let mut told = false;
    let mut boarded_at = None;
    for _ in 0..(10 * 60) {
        w.tick(&mut minds);
        hold(w, id);
        if w.citizens[id].thought.contains("last boat") {
            told = true;
        }
        if matches!(w.citizens[id].activity, Activity::OnFerry { .. }) {
            boarded_at = Some(w.clock.minute_of_day() as u32);
            break;
        }
    }
    assert!(told, "said the last boat had gone");
    let m = boarded_at.expect("took the first boat of the morning");
    assert!((6 * 60..8 * 60).contains(&m), "on the first boats: {m}");
}

#[test]
fn building_sites_on_the_island_get_their_materials_by_ferry() {
    let mut s = isle_with(PortKind::FerryTerminalLarge);
    let w = &mut s.w;
    assert!(w.ports.links.first().is_some_and(|l| l.cars), "two large terminals: a car ferry crossing");
    let l = lot_near(w, s.at + s.out * 140.0 + s.flow * 120.0, LandUse::Residential).expect("another island lot");
    w.build_on(l);
    let site = w.lots[l].building.unwrap();
    w.refresh_connectivity();
    assert!(w.island_supplied(site), "the island is reached by ferry");
    until(w, 7 * 60);
    let mut minds = IdleMind;
    let mut lorry = false;
    let mut loads = 0;
    for _ in 0..(3 * 1440) {
        w.tick(&mut minds);
        lorry |= w.ports.stats.lorries_ferried_today > 0;
        loads = loads.max(w.ports.stats.loads_ferried_today);
        if w.buildings.get(site).is_none_or(|b| b.construction.is_none()) {
            break;
        }
    }
    let b = &w.buildings[site];
    assert!(b.construction.as_ref().is_none_or(|c| c.blocked.is_none()), "never held up: {:?}", b.construction.as_ref().and_then(|c| c.blocked));
    assert!(b.construction.as_ref().is_none_or(|c| c.deliveries > 0), "materials reached it");
    assert!(loads > 0 && lorry, "loads crossed ({loads}) and lorries drove off the car ferry on the island");
}

#[test]
fn a_rider_aboard_when_the_city_is_saved_steps_off_after_the_load() {
    let mut s = isle();
    let id = islander(&mut s);
    let w = &mut s.w;
    w.citizens[id].inside = None;
    w.citizens[id].activity = Activity::OnFerry { from: s.isle, to: s.town };
    w.citizens[id].transit = Some(crate::transit::TransitTrip { leg: TransitLeg::Ferry { board: s.isle, alight: s.town }, target: s.shop, purpose: Purpose::Shop(Resource::Goods), since: 1, bike: false });
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let c = &back.citizens[id];
    assert!(!matches!(c.activity, Activity::OnFerry { .. }), "set down: {}", c.activity.label());
    assert!(!back.ports.links.is_empty(), "the crossing is worked out again on load");
}

#[test]
fn the_ferry_sim_replays_the_same() {
    let run = || {
        let mut s = isle();
        let id = islander(&mut s);
        until(&mut s.w, 7 * 60 + 30);
        s.w.travel_to(id, Some(s.shop), Purpose::Shop(Resource::Goods));
        tick(&mut s.w, 400);
        let c = &s.w.citizens[id];
        (c.pos.x.to_bits(), c.pos.y.to_bits(), c.activity.label(), s.w.ports.stats.ferry_riders_today, s.w.ports.stats.own_trips_today, s.w.ports.ships.len())
    };
    assert_eq!(run(), run());
}

#[test]
fn an_island_home_suits_retirees_and_island_workers_more_than_long_commuters() {
    let s = isle();
    let w = &s.w;
    let retirees = [(70u8, 11u64, None, 300.0), (72, 12, None, 300.0)];
    let commuter = [(30u8, 11u64, Some(s.shop), 500.0)];
    let a = w.island_appeal(&retirees, s.home);
    let b = w.island_appeal(&commuter, s.home);
    assert!(a > b, "retirees {a} vs a commuter {b}");
    assert_eq!(w.island_appeal(&commuter, s.shop), 1.0, "a mainland home is no question");
}

#[test]
fn islanders_out_in_town_head_home_for_the_last_boat() {
    let mut s = isle();
    let id = islander(&mut s);
    until(&mut s.w, 19 * 60 + 30);
    let w = &mut s.w;
    let door = w.building_entrance(s.shop).unwrap();
    {
        let c = &mut w.citizens[id];
        c.inside = Some(s.shop);
        c.activity = Activity::Visiting(s.shop);
        c.pos = door;
        c.prev_pos = door;
        c.next_decision_tick = u64::MAX;
    }
    let mut minds = IdleMind;
    let mut home = false;
    for _ in 0..240 {
        w.tick(&mut minds);
        if w.citizens[id].inside == Some(s.home) {
            home = true;
            break;
        }
    }
    assert!(home, "home on the last boats: {} at {} ({})", w.citizens[id].activity.label(), w.clock.format_hhmm(), w.citizens[id].thought);
}
