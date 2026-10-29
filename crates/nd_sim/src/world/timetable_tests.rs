//! [r4-timetable] Timetables in saves: an old save's lines keep the hours
//! and frequency every line had before (buses 05:30–23:30, the metro
//! 05:00–24:00, trains 05:00–23:00, ferries 06:00–22:00), and a timetable
//! and timeline set in a game come back from its save. The trains' and
//! ferries' timetables, and what their extra runs cost.

use super::*;
use crate::timetable::{Kind, LineRef, Part, Preset, Timetable};

/// `text` without every field called `name` (at any depth), as a save
/// from before the field was written.
fn without_fields(text: &str, name: &str) -> String {
    let key = format!("{name}: ");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(&key) {
        // Only a field at the start of its line (after the indent).
        let line_start = rest[..i].rfind('\n').map_or(0, |n| n + 1);
        if !rest[line_start..i].chars().all(|c| c == ' ') {
            out.push_str(&rest[..i + key.len()]);
            rest = &rest[i + key.len()..];
            continue;
        }
        out.push_str(&rest[..line_start]);
        // The value runs to the first comma outside brackets.
        let after = &rest[i + key.len()..];
        let mut depth = 0i32;
        let mut end = after.len();
        for (k, ch) in after.char_indices() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    end = k + 1;
                    break;
                }
                _ => {}
            }
        }
        let tail = &after[end..];
        rest = tail.strip_prefix('\n').unwrap_or(tail);
    }
    out.push_str(rest);
    out
}

/// A town with a bus line, a metro line and a station for the trains.
fn lined_town() -> (World, RouteId, crate::subway::MetroLineId) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-1200.0, 0.0), Vec2::new(1200.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-1150.0, 4.0), Vec2::new(1150.0, 4.0), Vec2::new(1150.0, 60.0), Vec2::new(-1150.0, 60.0)], LandUse::Residential);
    w.implement();
    let route = w.add_bus_route();
    w.add_bus_stop(route, Vec2::new(-600.0, 4.0)).unwrap();
    w.add_bus_stop(route, Vec2::new(600.0, 4.0)).unwrap();
    w.set_route_buses(route, 3);
    let lot = |w: &World, x: f32| w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).min_by(|a, b| a.1.center.distance(Vec2::new(x, 30.0)).total_cmp(&b.1.center.distance(Vec2::new(x, 30.0)))).map(|(id, _)| id).unwrap();
    let a = w.build_metro_station(lot(&w, -900.0)).unwrap();
    let b = w.build_metro_station(lot(&w, 900.0)).unwrap();
    let line = w.add_metro_line(&[a, b]).unwrap();
    w.set_metro_headway(line, 30).unwrap();
    (w, route, line)
}

#[test]
fn an_old_save_keeps_todays_hours_and_frequencies() {
    let (w, route, line) = lined_town();
    let text = w.to_ron().unwrap();
    let mut old = text.clone();
    for f in ["timetable", "timeline", "default_preset"] {
        old = without_fields(&old, f);
    }
    assert!(!old.contains("timetable: ") && !old.contains("timeline: "), "the old save has no timetables");
    let back = World::from_ron(&old).expect("an old save loads");
    let r = &back.transit.routes[route];
    assert!(r.timetable.is_none() && r.timeline.today.is_empty());
    assert_eq!(r.schedule(), Timetable::flat(Kind::Bus, 3));
    assert_eq!((r.schedule().start, r.schedule().end), (5 * 60 + 30, 23 * 60 + 30), "buses 05:30 to 23:30");
    assert_eq!((r.buses_at(5 * 60 + 29), r.buses_at(5 * 60 + 30), r.buses_at(23 * 60 + 29), r.buses_at(23 * 60 + 30)), (0, 3, 3, 0));
    let l = &back.subway.lines[line];
    assert_eq!((l.headway_at(4 * 60 + 59), l.headway_at(5 * 60), l.headway_at(23 * 60 + 59)), (None, Some(30), Some(30)), "the metro 05:00 to midnight at its headway");
    assert_eq!((back.rail.headway_at(5 * 60), back.rail.headway_at(23 * 60)), (Some(crate::rail::PASSENGER_HEADWAY), None), "trains 05:00 to 23:00");
    let s = back.rail.schedule();
    assert_eq!((s.start, s.end, s.night), (5 * 60, 23 * 60, false));
    let f = Timetable::flat(Kind::Ferry, crate::ports::FERRY_HEADWAY as u32);
    assert_eq!((f.start, f.end), (6 * 60, 22 * 60), "ferries 06:00 to 22:00");
    assert!(back.transit.default_preset.is_none());
    // And it saves as the new save does once loaded: nothing else moved.
    assert_eq!(back.to_ron().unwrap(), World::from_ron(&text).unwrap().to_ron().unwrap());
}

#[test]
fn a_timetable_and_its_timeline_come_back_from_a_save() {
    let (mut w, route, line) = lined_town();
    w.set_line_preset(LineRef::Bus(route), Preset::Commuter).unwrap();
    w.set_line_preset(LineRef::Metro(line), Preset::NightOwl).unwrap();
    let mut rail = Preset::Hours24.timetable(Kind::Rail, 90);
    rail.am = (7 * 60, 8 * 60);
    w.set_line_timetable(LineRef::Rail, Some(rail)).unwrap();
    w.transit.routes[route].timeline.board(8, 5, 40);
    w.transit.default_preset = Some(Preset::Commuter);
    let mut back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.transit.routes[route].timetable, w.transit.routes[route].timetable);
    assert_eq!(back.transit.routes[route].timeline.today[8].boarded, 5);
    assert_eq!(back.subway.lines[line].schedule().value(Part::Night), 60, "night owl: every hour at night on a 30-minute line");
    assert_eq!(back.rail.timetable, Some(rail.clamped(Kind::Rail)));
    assert_eq!(back.transit.default_preset, Some(Preset::Commuter));
    // A new line starts on the city's default.
    let r = back.add_bus_route();
    assert_eq!(back.transit.routes[r].schedule().preset(Kind::Bus), Some(Preset::Commuter));
}

#[test]
fn trains_and_ferries_follow_their_timetables_and_pay_for_extra_runs() {
    let (mut w, ..) = lined_town();
    // Rail: the standard is every two hours, 05:00 to 23:00.
    let mut t = Timetable::flat(Kind::Rail, 120);
    t.values[Part::MorningRush.index()] = 30;
    w.set_line_timetable(LineRef::Rail, Some(t)).unwrap();
    assert_eq!((w.rail.headway_at(7 * 60), w.rail.headway_at(12 * 60)), (Some(30), Some(120)));
    // Twelve runs on the timeline against the standard nine: three extra
    // trains paid for at midnight...
    w.rail.timeline.run(7, 12);
    let before = w.economy.treasury;
    w.daily_timetables();
    assert!((before - w.economy.treasury - 3.0 * super::timetables::RAIL_RUN_COST).abs() < 1e-2, "paid {}", before - w.economy.treasury);
    assert!(w.rail.timeline.today.is_empty() && w.rail.timeline.yesterday[7].runs == 12, "rolled");
    // ... and on the standard timetable nothing extra is charged.
    w.set_line_timetable(LineRef::Rail, None).unwrap();
    w.rail.timeline.run(7, 12);
    let before = w.economy.treasury;
    w.daily_timetables();
    assert_eq!(w.economy.treasury, before);
    // A ferry terminal's sailing hours.
    let mut p = crate::ports::Port {
        kind: crate::ports::PortKind::FerryTerminal,
        center: Vec2::ZERO,
        along: Vec2::X,
        seaward: Vec2::Y,
        pier: [Vec2::ZERO, Vec2::Y * 20.0],
        berth: Vec2::Y * 20.0,
        water: crate::landscape::WaterKind::Sea,
        building: None,
        lot: None,
        road_connected: true,
        walk_connected: true,
        reaches_elsewhere: true,
        last_sailing: 0,
        remote: Vec::new(),
        sailings: 0,
        stub: None,
        moored: None,
        timetable: None,
        timeline: Default::default(),
    };
    assert_eq!((p.ferry_headway_at(5 * 60 + 59), p.ferry_headway_at(6 * 60), p.ferry_headway_at(22 * 60)), (None, Some(crate::ports::FERRY_HEADWAY), None));
    p.timetable = Some(Preset::Hours24.timetable(Kind::Ferry, 60));
    assert_eq!(p.ferry_headway_at(3 * 60), Some(60));
}
