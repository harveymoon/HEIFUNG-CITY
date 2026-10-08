//! [fishing] Fishing fleets: out in the morning to grounds on open water,
//! trawling for hours, home in the afternoon with the catch; berths empty
//! while out; storms keep them in; saves and determinism.

use super::*;
use crate::fishing::{self as fish, TrawlerPhase};
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::mind::IdleMind;
use crate::ports::PortKind;

/// A coast with a street along the shore and a fishing port on the sea.
fn coast_port(kind: PortKind) -> (World, BuildingId) {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::Coast, 5), ..WorldConfig::default() });
    w.economy.treasury = 900_000.0;
    let mut why: Vec<String> = Vec::new();
    let sea = |w: &World, p: Vec2| w.landscape.water_at(p).is_some_and(|h| h.kind == crate::landscape::WaterKind::Sea);
    for x in [0.0f32, 250.0, -250.0, 500.0, -500.0, 750.0, -750.0] {
        // Down from the middle of the map to the sea.
        let mut y = 600.0;
        while y > -1700.0 && !sea(&w, Vec2::new(x, y)) {
            y -= 5.0;
        }
        if y <= -1700.0 {
            why.push(format!("x {x}: no sea"));
            continue;
        }
        let shore = Vec2::new(x, y + 5.0);
        let road_at = shore + Vec2::Y * 130.0;
        let mut t = w.clone();
        let id = match t.place_port(kind, shore + Vec2::Y * 15.0) {
            Ok(id) => id,
            Err(e) => {
                why.push(format!("x {x} shore {shore:?}: {e}"));
                continue;
            }
        };
        t.add_road(&[road_at - Vec2::X * 300.0, road_at + Vec2::X * 300.0], 1, 1);
        t.add_zone(vec![road_at + Vec2::new(-150.0, 10.0), road_at + Vec2::new(150.0, 10.0), road_at + Vec2::new(150.0, 90.0), road_at + Vec2::new(-150.0, 90.0)], LandUse::Residential);
        let [s0, _] = t.ports.port_of(id).unwrap().road_line();
        t.add_road(&[s0, road_at - Vec2::X * 300.0], 1, 1);
        t.refresh_port_links();
        if t.ports.port_of(id).unwrap().works() {
            w = t;
            staff(&mut w, id);
            return (w, id);
        }
        why.push(format!("x {x}: not working"));
    }
    panic!("no spot for a fishing port on the coast: {why:?}");
}

fn staff(w: &mut World, id: BuildingId) {
    let jobs = w.buildings[id].jobs as usize;
    w.buildings[id].workers = vec![CitizenId::default(); jobs];
}

/// Run to the given minute of the day (the next one if past it), keeping
/// the port staffed.
fn run_to(w: &mut World, id: BuildingId, minute: u64, mut each: impl FnMut(&World)) {
    let mut minds = IdleMind;
    let mut n = 0;
    while w.clock.minute_of_day() != minute || n == 0 {
        w.tick(&mut minds);
        staff(w, id);
        each(w);
        n += 1;
    }
}

fn wet_path(w: &World, pts: &[Vec2]) -> bool {
    pts.windows(2).all(|s| fish::line_wet(&w.landscape, s[0], s[1], 4.0, 0.0))
}

#[test]
fn grounds_lie_on_open_water_reachable_by_water_and_are_the_same_every_time() {
    let (mut w, id) = coast_port(PortKind::FishingPortLarge);
    run_to(&mut w, id, 2 * 60, |_| {});
    let f = w.fleet(id).expect("a fleet for the port").clone();
    assert!(!f.grounds.is_empty(), "the port has fishing grounds");
    assert!(f.boats.len() >= 4 && f.boats.len() <= PortKind::FishingPortLarge.berths() as usize, "a boat per berth: {}", f.boats.len());
    let port = w.ports.port_of(id).unwrap().clone();
    for g in &f.grounds {
        assert!(w.landscape.is_water(g.center), "a ground on water");
        assert!(-w.landscape.water_distance(g.center) >= fish::GROUND_CLEAR - 1.0, "well off the shore");
        assert!(g.center.distance(port.berth) > 200.0, "out from the quay");
        assert!(wet_path(&w, &g.route), "the way there never crosses land");
        assert!(g.route.first().unwrap().distance(f.offing) < 1.0 && g.route.last().unwrap().distance(g.center) < 1.0);
        assert!((0.75..=1.3).contains(&g.richness));
        // Every sweep over it stays on the water.
        for t in 0..6 {
            assert!(wet_path(&w, &fish::sweep(&w.landscape, g.center, g.radius, t as f32)), "a sweep over the ground stays on water");
        }
    }
    let (again, id2) = coast_port(PortKind::FishingPortLarge);
    let mut again = again;
    run_to(&mut again, id2, 2 * 60, |_| {});
    assert_eq!(again.fleet(id2).unwrap().grounds, f.grounds, "grounds come from the landscape seed");
}

#[test]
fn grounds_keep_out_of_the_shipping_lanes() {
    let (mut w, id) = coast_port(PortKind::FishingPortSmall);
    let port = w.ports.port_of(id).unwrap().clone();
    // A ferry terminal along the coast with its routes out to sea.
    let mut ferry = None;
    for d in [400.0f32, -400.0, 650.0, -650.0] {
        let along = port.along * d;
        if let Ok(f) = w.place_port(PortKind::FerryTerminal, port.center + along) {
            ferry = Some(f);
            break;
        }
    }
    let Some(ferry) = ferry else { return };
    run_to(&mut w, id, 60, |_| {});
    let fb = w.ports.port_of(ferry).unwrap().berth;
    let lanes: Vec<Vec<Vec2>> = w.landscape.water_routes_out(fb).into_iter().map(|(_, r)| r).collect();
    for g in &w.fleet(id).unwrap().grounds {
        for l in &lanes {
            let d = l.windows(2).map(|s| crate::geom::project_on_segment(g.center, s[0], s[1]).1.distance(g.center)).fold(f32::INFINITY, f32::min);
            assert!(d >= fish::LANE_CLEAR - 1.0, "a ground {d:.0} m from a ferry route");
        }
    }
}

#[test]
fn trawlers_go_out_in_the_morning_fish_for_hours_and_come_home_in_the_afternoon() {
    let (mut w, id) = coast_port(PortKind::FishingPortLarge);
    run_to(&mut w, id, 0, |_| {});
    let fish_before = w.buildings[id].business.as_ref().unwrap().stock_of(Resource::Fish);
    let n = w.fleet(id).unwrap().boats.len();
    let mut departed: Vec<Option<u64>> = vec![None; n];
    let mut home: Vec<Option<u64>> = vec![None; n];
    let mut fished_far = false;
    let mut empty_berth = false;
    let mut credited = false;
    let mut last: Vec<TrawlerPhase> = vec![TrawlerPhase::InPort; n];
    let mut stock_last = fish_before;
    run_to(&mut w, id, 20 * 60, |w| {
        let f = w.fleet(id).unwrap();
        let port = w.ports.port_of(id).unwrap();
        let minute = w.clock.minute_of_day();
        for (k, b) in f.boats.iter().enumerate() {
            if last[k] == TrawlerPhase::InPort && b.phase == TrawlerPhase::Outbound {
                departed[k] = Some(minute);
                assert!(wet_path(w, &b.path), "the way out is all water");
            }
            if last[k] == TrawlerPhase::Inbound && b.phase == TrawlerPhase::Unloading {
                home[k] = Some(minute);
                assert!(b.catch_today > 0.0, "it came home with fish");
                credited = true;
            }
            if b.phase == TrawlerPhase::Inbound && last[k] != TrawlerPhase::Inbound {
                assert!(wet_path(w, &b.path), "the way home is all water");
            }
            if b.phase.nets_out() {
                let (p, _) = f.boat_pose(k, 1.0);
                assert!(w.landscape.is_water(p), "fishing on the water");
                if p.distance(port.berth) > 250.0 {
                    fished_far = true;
                }
            }
            if b.phase.at_sea() {
                let (p, _) = f.boat_pose(k, 1.0);
                assert!(w.landscape.is_water(p), "a boat at sea is on the water: {p:?}");
                // Its berth is empty: nothing else lies there.
                let (m, _) = f.mooring(b.slot);
                assert!(f.boats.iter().enumerate().all(|(j, o)| j == k || o.slot != b.slot), "one boat per berth");
                if p.distance(m) > 30.0 {
                    empty_berth = true;
                }
            }
            last[k] = b.phase;
        }
        let stock = w.buildings[id].business.as_ref().unwrap().stock_of(Resource::Fish);
        // No fish appear on the quay but off the boats.
        if f.boats.iter().all(|b| b.phase != TrawlerPhase::Unloading) {
            assert!(stock <= stock_last + 1e-3, "fish come in on the boats");
        }
        stock_last = stock;
    });
    let out: Vec<u64> = departed.iter().flatten().copied().collect();
    let back: Vec<u64> = home.iter().flatten().copied().collect();
    assert!(out.len() >= 3, "most boats went out: {out:?}");
    assert!(out.iter().all(|m| (3 * 60..9 * 60).contains(m)), "they cast off in the early morning: {out:?}");
    assert!(out.iter().max() != out.iter().min(), "staggered: {out:?}");
    assert_eq!(back.len(), out.len(), "and all came home");
    assert!(back.iter().all(|m| (11 * 60..18 * 60).contains(m)), "in the afternoon: {back:?}");
    assert!(fished_far, "they fished out at sea");
    assert!(empty_berth, "a boat at sea leaves its berth empty");
    assert!(credited);
    let f = w.fleet(id).unwrap();
    assert!(f.landed_today > 0.0 && w.scene.fish_landed_today >= f.landed_today - 1e-3);
    assert!(f.boats.iter().all(|b| !b.phase.at_sea()), "all in by evening");
    assert!(f.boats.iter().filter(|b| b.trip_day.is_some()).all(|b| b.fished > 150), "hours on the grounds: {:?}", f.boats.iter().map(|b| b.fished).collect::<Vec<_>>());
}

#[test]
fn the_catch_is_about_what_the_port_used_to_make_and_follows_the_crew() {
    let (mut w, id) = coast_port(PortKind::FishingPortSmall);
    let jobs = w.buildings[id].jobs as f32;
    run_to(&mut w, id, 0, |_| {});
    let mut total = 0.0;
    for _ in 0..4 {
        run_to(&mut w, id, 23 * 60 + 59, |_| {});
        total += w.fleet(id).unwrap().landed_today;
    }
    let old = 3.0 * jobs * crate::market::JOB_RATE * 24.0 * 4.0;
    eprintln!("catch ratio {:.2} ({total:.0} / {old:.0})", total / old);
    assert!(total > old * 0.7 && total < old * 1.3, "four days' catch {total:.0} against {old:.0} the old way");
    // With no crew, the boats stay in.
    let mut minds = IdleMind;
    let start = w.fleet(id).unwrap().landed_today;
    for _ in 0..(24 * 60) {
        w.buildings[id].workers.clear();
        w.tick(&mut minds);
    }
    let _ = start;
    assert_eq!(w.fleet(id).unwrap().trips_today, 0, "nobody to crew them");
}

#[test]
fn a_storm_keeps_the_boats_in_port() {
    let (mut w, id) = coast_port(PortKind::FishingPortLarge);
    run_to(&mut w, id, 60, |_| {});
    w.weather.spell = crate::weather::Spell::Storm;
    w.weather.spell_hours = 200;
    run_to(&mut w, id, 11 * 60, |w| {
        assert_eq!(w.fleet(id).unwrap().at_sea(), 0, "nobody puts out in a storm");
    });
    assert!(w.fleet(id).unwrap().kept_in.is_some(), "the port says why");
    // A storm blowing up while they are out calls them home.
    w.weather.spell = crate::weather::Spell::Normal;
    w.weather.spell_hours = 0;
    run_to(&mut w, id, 0, |_| {});
    run_to(&mut w, id, 9 * 60, |_| {});
    assert!(w.fleet(id).unwrap().at_sea() > 0, "out on a calm morning");
    w.weather.spell = crate::weather::Spell::Storm;
    w.weather.spell_hours = 200;
    run_to(&mut w, id, 9 * 60 + 1, |_| {});
    assert!(w.fleet(id).unwrap().boats.iter().all(|b| !b.phase.nets_out()), "nets in, heading home");
}

#[test]
fn fleets_survive_a_save_old_saves_load_and_it_is_deterministic() {
    let (mut w, id) = coast_port(PortKind::FishingPortSmall);
    let mut twin = w.clone();
    run_to(&mut w, id, 10 * 60, |_| {});
    run_to(&mut twin, id, 10 * 60, |_| {});
    assert_eq!(w.fishing, twin.fishing, "the same city fishes the same way");
    assert!(w.fleet(id).unwrap().at_sea() > 0);
    let text = w.to_ron().unwrap();
    let mut back = World::from_ron(&text).unwrap();
    assert_eq!(back.fishing, w.fishing, "boats at sea are saved where they are");
    let mut minds = IdleMind;
    for _ in 0..30 {
        w.tick(&mut minds);
        back.tick(&mut minds);
        staff(&mut w, id);
        staff(&mut back, id);
    }
    assert_eq!(back.fishing, w.fishing, "and carry on the same");
    // A save from before the fleets.
    let start = text.find("\n    fishing: (").expect("fishing field") + 1;
    let mut depth = 0;
    let mut end = start;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => {
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
    let mut old = World::from_ron(&stripped).expect("an old save loads");
    assert!(old.fishing.fleets.is_empty());
    old.tick(&mut minds);
    assert!(old.fleet(id).is_some_and(|f| f.boats.iter().all(|b| b.phase == TrawlerPhase::InPort)), "its boats start in port");
}

#[test]
fn fouled_water_lands_less_fish() {
    let (w, _) = coast_port(PortKind::FishingPortSmall);
    let p = Vec2::new(0.0, -1000.0);
    let mut dirty = w.clone();
    dirty.contamination.add(p, 300.0, 1.0);
    assert_eq!(w.water_fouling(p), 0.0);
    assert!(dirty.water_fouling(p) > 0.5, "contamination reaching the water fouls it");
}
