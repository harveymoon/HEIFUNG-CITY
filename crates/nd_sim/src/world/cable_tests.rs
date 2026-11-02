//! [cable] Stations, lines, riders and saves.

use super::*;
use crate::cable::{self as cable, CableKind};
use crate::citizens::TripMode;
use crate::mind::IdleMind;

/// Someone standing at `pos` with no home or job.
fn walker(w: &mut World, pos: Vec2) -> CitizenId {
    let n = w.citizens.len();
    w.citizens.insert(Citizen {
        name: format!("Rider {n}"),
        seed: 900 + n as u64,
        home: None,
        work: None,
        pos,
        heading: 0.0,
        prev_pos: pos,
        prev_heading: 0.0,
        mode: TravelMode::Walk,
        car: None,
        commute: Default::default(),
        look: None,
        to_car: None,
        drunk: Default::default(),
        owns_car: false,
        owns_bike: false,
        vehicle: None,
        parked_at: None,
        transit: None,
        activity: Activity::Idle,
        memories: Vec::new(),
        places: Default::default(),
        thought: String::new(),
        spotlight: false,
        next_decision_tick: u64::MAX / 2,
        arrived_tick: 0,
        inside: None,
        mood: 0.6,
        income: 120.0,
        savings: 400.0,
        rent_stress_days: 0,
        satiety: 0.9,
        groceries: 0.0,
        hungry_days: 0,
        trail: Vec::new(),
        trail_stops: Default::default(),
        pending_footfall: Vec::new(),
        household: None,
        age: 35,
        wakefulness: 0.9,
        asleep: false,
        packed_meal: 1.0,
    })
}

/// A long street with lots along both sides.
fn town() -> World {
    let landscape = crate::landscape::LandscapeParams { preset: crate::landscape::LandscapePreset::Flat, ..Default::default() };
    let mut w = World::new(WorldConfig { emergencies: false, society: false, social: false, landscape, ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-1200.0, 0.0), Vec2::new(1200.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-1150.0, 4.0), Vec2::new(1150.0, 4.0), Vec2::new(1150.0, 60.0), Vec2::new(-1150.0, 60.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(-1150.0, -4.0), Vec2::new(1150.0, -4.0), Vec2::new(1150.0, -60.0), Vec2::new(-1150.0, -60.0)], LandUse::Commercial);
    w.implement();
    w.economy.treasury = 2_000_000.0;
    w
}

fn lot_near(w: &World, p: Vec2) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable()).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).unwrap()
}

fn lot_centre(w: &World, p: Vec2) -> Vec2 {
    w.lots[lot_near(w, p)].center
}

/// A gondola from one end of town to the other, on two lots.
fn gondola(w: &mut World, from: Vec2, to: Vec2) -> CableLineId {
    let (a, b) = (lot_centre(w, from), lot_centre(w, to));
    w.add_cable_line(CableKind::Gondola, &[CableStop::At(a, SiteWant::Auto), CableStop::At(b, SiteWant::Auto)]).expect("gondola")
}

/// A tall building on the lot nearest `p`.
fn tower(w: &mut World, p: Vec2, floors: u8) -> BuildingId {
    let lot = lot_near(w, p);
    let id = w.place_special(lot, "Test Tower".into(), 0, 0, |b| b.floors = floors);
    w.buildings[id].construction = None;
    w.buildings[id].demolition = None;
    id
}

#[test]
fn stations_take_a_lot_a_roof_or_open_ground_and_say_why_not() {
    let mut w = town();
    // A lot: bought out for a ground station.
    let lot = lot_centre(&w, Vec2::new(-800.0, 30.0));
    let site = w.cable_station_site(lot, SiteWant::Auto).expect("a lot");
    assert!(!site.roof && site.lot.is_some(), "{}", site.what);
    // A tall building: its roof.
    let t = tower(&mut w, Vec2::new(300.0, 30.0), 10);
    let at = w.lots[w.buildings[t].lot].center;
    let roof = w.cable_station_site(at, SiteWant::Auto).expect("a roof");
    assert!(roof.roof && roof.host == Some(t) && roof.floor > w.building_height(t), "{} (tower {:.0} m, operating {})", roof.what, w.building_height(t), w.buildings[t].operating());
    // A road is no place for a station.
    assert!(w.cable_station_site(Vec2::new(0.0, 0.0), SiteWant::Auto).unwrap_err().contains("road"));
    // Open ground far from any way to walk in.
    assert!(w.cable_station_site(Vec2::new(0.0, 400.0), SiteWant::Auto).unwrap_err().contains("walk in"));
    // A roof wanted where there is no building.
    assert!(w.cable_station_site(lot, SiteWant::Roof).is_err());
    // Too close to a station already there.
    let line = gondola(&mut w, Vec2::new(-800.0, 30.0), Vec2::new(0.0, -30.0));
    let first = w.cable.lines[line].stations[0];
    let near = w.cable.stations[first].pos + Vec2::new(35.0, 0.0);
    assert!(w.cable_station_site(near, SiteWant::Auto).unwrap_err().contains("too close"));
    let b = w.cable.stations[first].building.expect("the station's lot building");
    assert!(w.buildings[b].cable && w.buildings[b].is_city_structure());
}

#[test]
fn a_line_is_laid_with_pylons_off_the_road_and_paid_for() {
    let mut w = town();
    let before = w.economy.treasury;
    // Across the street (not along it: a line down the middle of a road
    // has nowhere for its pylons).
    let a = lot_centre(&w, Vec2::new(-600.0, 45.0));
    let b = lot_centre(&w, Vec2::new(500.0, -45.0));
    let stops = [CableStop::At(a, SiteWant::Auto), CableStop::At(b, SiteWant::Auto)];
    let pv = w.cable_line_preview(CableKind::Gondola, &stops);
    assert!(pv.refusal.is_none(), "{:?}", pv.refusal);
    assert_eq!(pv.new_stations, 2);
    assert!(pv.pylons >= 3, "1.1 km needs pylons: {}", pv.pylons);
    assert!(pv.capacity_per_hour > 300.0 && pv.build_cost > 2.0 * cable::GONDOLA.station_cost);
    let id = w.add_cable_line(CableKind::Gondola, &stops).expect("line");
    assert!((before - w.economy.treasury - pv.build_cost).abs() < 1.0, "paid {} for {}", before - w.economy.treasury, pv.build_cost);
    let l = &w.cable.lines[id];
    assert_eq!(l.stations.len(), 2);
    for p in l.pylons() {
        let (sid, _, q) = w.roads.nearest_where(p.pos, |_| true).unwrap();
        assert!(q.distance(p.pos) > w.roads.segment(sid).half_width(), "a pylon on the road at {:?}", p.pos);
        assert!(p.height() >= cable::GONDOLA.min_pylon - 0.01);
    }
    // Lots under the rope are held below the cabins.
    let mid = l.point_at(l.length() * 0.3, 0.0).0;
    let under = w.lots.iter().find(|(_, lt)| point_in_polygon(mid, &lt.polygon)).map(|(id, _)| id);
    if let Some(lot) = under {
        let lim = w.height_limit_at(lot).expect("a limit under the line");
        assert!(lim < l.rope_z(l.length() * 0.3) - cable::GONDOLA.hanger + 0.1, "{lim}");
    }
    // A tramway calls at two stations only.
    let c = lot_centre(&w, Vec2::new(950.0, 30.0));
    let three = [CableStop::At(lot_centre(&w, Vec2::new(-300.0, 30.0)), SiteWant::Auto), CableStop::At(lot_centre(&w, Vec2::new(300.0, -30.0)), SiteWant::Auto), CableStop::At(c, SiteWant::Auto)];
    assert!(w.cable_line_preview(CableKind::Tramway, &three).refusal.unwrap().contains("two stations"));
    // Too poor.
    w.economy.treasury = 100.0;
    let d = lot_centre(&w, Vec2::new(-200.0, -30.0));
    assert!(w.add_cable_line(CableKind::Gondola, &[CableStop::At(c, SiteWant::Auto), CableStop::At(d, SiteWant::Auto)]).unwrap_err().contains("treasury"));
}

#[test]
fn a_tall_building_in_the_way_raises_the_pylons_and_a_roof_station_keeps_its_building() {
    let mut w = town();
    let base = gondola(&mut w, Vec2::new(-600.0, 30.0), Vec2::new(600.0, 30.0));
    let low = w.cable.lines[base].pylons().map(|p| p.height()).fold(0.0, f32::max);
    w.remove_cable_line(base);
    w.step_cable();
    assert!(w.cable.lines.is_empty() && w.cable.stations.is_empty(), "the stations come down with their only line");
    // The same line with a 15-storey block in its way.
    let t = tower(&mut w, Vec2::new(0.0, 30.0), 15);
    let line = gondola(&mut w, Vec2::new(-600.0, 30.0), Vec2::new(600.0, 30.0));
    let high = w.cable.lines[line].pylons().map(|p| p.height()).fold(0.0, f32::max);
    assert!(high > low + 10.0, "pylons {high:.0} m over the block against {low:.0} m");
    assert!(w.buildings.contains_key(t), "the block stays");
    // A station on its roof: the building stays and keeps its use.
    let roof_at = w.lots[w.buildings[t].lot].center;
    let other = lot_centre(&w, Vec2::new(-900.0, -30.0));
    let id = w.add_cable_line(CableKind::Gondola, &[CableStop::At(other, SiteWant::Auto), CableStop::At(roof_at, SiteWant::Roof)]).expect("to the roof");
    let top = *w.cable.lines[id].stations.last().unwrap();
    assert!(w.cable.stations[top].roof && w.cable.stations[top].building == Some(t));
    assert!(w.cable.stations[top].floor > w.building_height(t));
    // Pull the building down and the roof station (and its line) go.
    w.demolish(t);
    w.step_cable();
    assert!(!w.cable.lines.contains_key(id) && !w.cable.stations.contains_key(top));
}

/// A rider waiting at the first station of `line` for its last.
fn waiting_rider(w: &mut World, line: CableLineId) -> CitizenId {
    let l = &w.cable.lines[line];
    let (a, b) = (l.stations[0], *l.stations.last().unwrap());
    let target = w.cable.stations[b].building.unwrap();
    let at = w.cable.stations[a].pos;
    let r = walker(w, at);
    w.citizens[r].transit = Some(TransitTrip { leg: TransitLeg::Cable { line, board: a, alight: b }, target, purpose: Purpose::Wander, since: w.clock.tick.max(1), bike: false });
    w.citizens[r].activity = Activity::Waiting { at };
    r
}

#[test]
fn gondola_riders_board_passing_cabins_ride_and_get_off_at_the_far_end() {
    let mut w = town();
    let mut mind = IdleMind;
    w.clock = SimClock::starting_at(0, 9, 0);
    let line = gondola(&mut w, Vec2::new(-900.0, 30.0), Vec2::new(700.0, 30.0));
    let r = waiting_rider(&mut w, line);
    assert!(w.citizens[r].in_cable_station() && !w.citizens[r].is_visible(), "waiting inside the station");
    let fares = w.economy.ledger.today[Flow::BusFares.index()];
    let far = w.cable.stations[*w.cable.lines[line].stations.last().unwrap()].pos;
    let mut rode = false;
    let mut high = 0.0f32;
    for _ in 0..140 {
        w.tick(&mut mind);
        if let Activity::OnCable { cabin, .. } = w.citizens[r].activity {
            rode = true;
            let p = w.cable.lines[line].cabin_pose(cabin, w.config.traffic_seconds_per_tick);
            assert!(w.citizens[r].pos.distance(p.pos) < 30.0, "riding along with the cabin");
            high = high.max(p.z);
        }
    }
    assert!(rode, "boarded a cabin: {}", w.citizens[r].activity.label());
    assert!(high > 10.0, "up in the air: {high}");
    assert!(!matches!(w.citizens[r].activity, Activity::OnCable { .. } | Activity::Waiting { .. }), "off again: {}", w.citizens[r].activity.label());
    assert!(w.citizens[r].pos.distance(far) < 300.0, "near the far station: {:?}", w.citizens[r].pos);
    assert!(w.economy.ledger.today[Flow::BusFares.index()] >= fares + cable::GONDOLA.fare - 0.01, "the fare");
    let l = &w.cable.lines[line];
    assert_eq!(l.riders_today, 1);
    assert_eq!(w.cable.stations[l.stations[0]].boardings_today, 1);
    assert!(l.seats_today > 0 && l.capacity_used() > 0.0);
}

#[test]
fn full_cabins_leave_a_queue_that_the_next_cabins_take() {
    let mut w = town();
    let mut mind = IdleMind;
    w.clock = SimClock::starting_at(0, 9, 0);
    let line = gondola(&mut w, Vec2::new(-900.0, 30.0), Vec2::new(700.0, 30.0));
    let riders: Vec<CitizenId> = (0..36).map(|_| waiting_rider(&mut w, line)).collect();
    let seats = cable::GONDOLA.seats as usize;
    let mut most_in_a_cabin = 0;
    let mut queue_seen = false;
    for _ in 0..60 {
        w.tick(&mut mind);
        let l = &w.cable.lines[line];
        for k in 0..l.cabins {
            most_in_a_cabin = most_in_a_cabin.max(l.in_cabin(k));
        }
        let waiting = riders.iter().filter(|r| matches!(w.citizens[**r].activity, Activity::Waiting { .. })).count();
        queue_seen |= waiting > 0 && l.aboard.len() >= seats;
        if waiting == 0 {
            break;
        }
    }
    assert!(most_in_a_cabin <= seats, "a cabin holds {seats}: {most_in_a_cabin}");
    assert_eq!(most_in_a_cabin, seats, "the first cabins leave full");
    assert!(queue_seen, "the rest queue for the next cabins");
    assert!(w.cable.lines[line].left_behind_today > 0);
    assert!(riders.iter().all(|r| !matches!(w.citizens[*r].activity, Activity::Waiting { .. })), "and every one of them gets a cabin soon");
}

#[test]
fn an_aerial_tramway_shuttles_two_cabins_and_riders_wait_for_theirs() {
    let mut w = town();
    let mut mind = IdleMind;
    w.clock = SimClock::starting_at(0, 9, 0);
    let a = lot_centre(&w, Vec2::new(-900.0, 30.0));
    let b = lot_centre(&w, Vec2::new(500.0, 30.0));
    let line = w.add_cable_line(CableKind::Tramway, &[CableStop::At(a, SiteWant::Auto), CableStop::At(b, SiteWant::Auto)]).expect("tramway");
    assert_eq!(w.cable.lines[line].cabins, 2);
    assert!(w.set_cable_cabins(line, 10).is_err());
    let r = waiting_rider(&mut w, line);
    // The way back: from the far end to the first.
    let (sa, sb) = (w.cable.lines[line].stations[0], w.cable.lines[line].stations[1]);
    let far = w.cable.stations[sb].pos;
    let back = walker(&mut w, far);
    let home = w.cable.stations[sa].building.unwrap();
    w.citizens[back].transit = Some(TransitTrip { leg: TransitLeg::Cable { line, board: sb, alight: sa }, target: home, purpose: Purpose::Wander, since: 1, bike: false });
    w.citizens[back].activity = Activity::Waiting { at: far };
    let period = w.cable.lines[line].shuttle_ticks(w.config.traffic_seconds_per_tick).2;
    let (mut r_rode, mut back_rode) = (None, None);
    for _ in 0..(period as usize + 20) {
        w.tick(&mut mind);
        if let Activity::OnCable { cabin, .. } = w.citizens[r].activity {
            r_rode = Some(cabin);
        }
        if let Activity::OnCable { cabin, .. } = w.citizens[back].activity {
            back_rode = Some(cabin);
        }
    }
    assert!(r_rode.is_some() && back_rode.is_some(), "both rode: {:?} {:?}", r_rode, back_rode);
    assert_ne!(r_rode, back_rode, "one cabin each way at a time");
    assert!(!matches!(w.citizens[r].activity, Activity::OnCable { .. } | Activity::Waiting { .. }));
    assert_eq!(w.cable.lines[line].riders_today, 2);
}

#[test]
fn people_choose_the_gondola_where_it_beats_the_walk_and_not_at_night() {
    let mut w = town();
    w.clock = SimClock::starting_at(0, 8, 0);
    let line = gondola(&mut w, Vec2::new(-1000.0, 30.0), Vec2::new(900.0, 30.0));
    let (sa, sb) = (w.cable.lines[line].stations[0], w.cable.lines[line].stations[1]);
    let from = w.cable.stations[sa].entrance + Vec2::new(40.0, 0.0);
    let to = w.cable.stations[sb].entrance - Vec2::new(40.0, 0.0);
    let target = w.cable.stations[sb].building.unwrap();
    let c = walker(&mut w, from);
    let q = super::trips::TripQuery { from, to, target, purpose: Purpose::Wander, allow_transit: true, allow_car: false, allow_bike: false };
    let opts = w.mode_options(c, &q);
    let ride = opts.iter().find(|o| o.mode == TripMode::Cable).expect("the gondola is an option");
    let walk = opts.iter().find(|o| o.mode == TripMode::Walk).unwrap();
    assert!(ride.minutes < walk.minutes * 0.5 && ride.cost < walk.cost, "riding beats a 1.9 km walk: {} vs {}", ride.minutes, walk.minutes);
    assert!(matches!(ride.plan, super::trips::ModePlan::Cable { line: l, board, alight, .. } if l == line && board == sa && alight == sb));
    // A short hop is walked.
    let near = super::trips::TripQuery { to: from + Vec2::new(150.0, 0.0), ..q };
    assert!(!w.mode_options(c, &near).iter().any(|o| o.mode == TripMode::Cable));
    // Closed at night.
    w.clock = SimClock::starting_at(1, 2, 0);
    assert!(!w.mode_options(c, &q).iter().any(|o| o.mode == TripMode::Cable));
    // And by day a trip there goes by gondola, door to door.
    w.clock = SimClock::starting_at(2, 9, 0);
    let mut mind = IdleMind;
    w.travel_to(c, Some(target), Purpose::Wander);
    assert!(matches!(w.citizens[c].transit.map(|t| t.leg), Some(TransitLeg::Cable { .. })), "set off for the station: {}", w.citizens[c].activity.label());
    let mut rode = false;
    for _ in 0..200 {
        w.tick(&mut mind);
        rode |= matches!(w.citizens[c].activity, Activity::OnCable { .. });
    }
    assert!(rode, "rode the gondola");
    assert_eq!(w.vehicles.stats.trips_today[TripMode::Cable.index()], 1, "counted as a cable trip");
}

#[test]
fn waiting_riders_give_up_at_closing_and_a_closed_line_strands_nobody() {
    let mut w = town();
    let mut mind = IdleMind;
    let line = gondola(&mut w, Vec2::new(-900.0, 30.0), Vec2::new(700.0, 30.0));
    // Past closing: nobody boards, the rope stops once empty, the waiting give up.
    w.clock = SimClock::starting_at(0, 23, 30);
    let r = waiting_rider(&mut w, line);
    for _ in 0..20 {
        w.tick(&mut mind);
    }
    assert!(!matches!(w.citizens[r].activity, Activity::OnCable { .. } | Activity::Waiting { .. }), "{}", w.citizens[r].activity.label());
    assert!(!w.cable.lines[line].running, "the rope stops at night");
    assert_eq!(w.cable.lines[line].riders_today, 0);
    // By day, a closed line sends its waiting riders on their way.
    w.clock = SimClock::starting_at(1, 10, 0);
    let r2 = waiting_rider(&mut w, line);
    assert!(w.remove_cable_line(line));
    assert!(w.citizens[r2].transit.is_none_or(|t| !matches!(t.leg, TransitLeg::Cable { .. })));
    assert!(!w.citizens[r2].in_cable_station());
}

#[test]
fn a_save_keeps_lines_and_stations_and_sets_riders_down() {
    let mut w = town();
    let mut mind = IdleMind;
    w.clock = SimClock::starting_at(0, 9, 0);
    let line = gondola(&mut w, Vec2::new(-900.0, 30.0), Vec2::new(700.0, 30.0));
    w.set_cable_cabins(line, 30).unwrap();
    w.set_cable_hours(line, 7 * 60, 22 * 60).unwrap();
    w.set_cable_fare(line, 4.5).unwrap();
    let r = waiting_rider(&mut w, line);
    for _ in 0..30 {
        w.tick(&mut mind);
        if matches!(w.citizens[r].activity, Activity::OnCable { .. }) {
            break;
        }
    }
    assert!(matches!(w.citizens[r].activity, Activity::OnCable { .. }));
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.cable.lines.len(), 1);
    let (l, m) = (back.cable.lines.values().next().unwrap(), &w.cable.lines[line]);
    assert_eq!((l.cabins, l.open, l.close, l.fare), (30, 7 * 60, 22 * 60, 4.5));
    assert_eq!(l.supports, m.supports);
    assert_eq!(back.cable.stations.len(), 2);
    assert!(back.buildings.values().filter(|b| b.cable).count() == 2);
    assert!(!matches!(back.citizens[r].activity, Activity::OnCable { .. }), "nobody is left in a cabin that was not saved");
    // The cabins stand where they stood.
    assert_eq!(l.phase, m.phase);
    assert_eq!(l.cabin_pose(3, 4.0), m.cabin_pose(3, 4.0));
}

#[test]
fn per_mode_tallies_from_before_the_cable_cars_still_load() {
    // Eight slots, as saves wrote them before; nine now.
    let old = "(pain: (1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0), trips: (1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0))";
    let c: crate::citizens::Commute = ron::from_str(old).expect("an old commute");
    assert_eq!(c.pain[7], 8.0);
    assert_eq!(c.pain[TripMode::Cable.index()], 0.0);
    let stats = "(trips_today: (1, 2, 3, 4, 5, 6, 7, 8), trips_yesterday: (0, 0, 0, 0, 0, 0, 0, 0))";
    let s: crate::vehicle::CarStats = ron::from_str(stats).expect("old car stats");
    assert_eq!(s.trips_today[7], 8);
    // And today's round trip keeps all nine.
    let mut now = crate::citizens::Commute::default();
    now.pain[TripMode::Cable.index()] = 2.5;
    let back: crate::citizens::Commute = ron::from_str(&ron::to_string(&now).unwrap()).unwrap();
    assert_eq!(back.pain[TripMode::Cable.index()], 2.5);
}

#[test]
fn stations_lift_land_value_and_scenic_lines_draw_visitors() {
    let mut w = town();
    w.update_land_value();
    let probe = lot_centre(&w, Vec2::new(-900.0, 30.0)) + Vec2::new(60.0, 0.0);
    let before = w.land_value.at(probe);
    let line = gondola(&mut w, Vec2::new(-900.0, 30.0), Vec2::new(700.0, 30.0));
    w.update_land_value();
    assert!(w.land_value.at(probe) > before, "{} -> {}", before, w.land_value.at(probe));
    // A scenic line is a sight.
    w.cable.lines[line].scenic = 0.8;
    w.refresh_attractions();
    assert!(w.tourism.attractions.iter().any(|a| a.kind == crate::tourism::AttractionKind::CableCar));
}
