//! [events] Tests: street races (and their bust), raves only where there is
//! a scene for them, road wear (it builds, cuts hit the poor streets first,
//! worn roads are slower) and a save round trip.

use super::*;
use crate::citizens::{driver_traits, rebelliousness};
use crate::mind::IdleMind;
use crate::road_wear;
use crate::services::ServiceKind;
use crate::street_events::{EventKind, RaceStage};
use crate::vehicle::{BodyType, VehicleClass, VehicleModel};

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// Two avenues joined by cross streets: empty civic land between them
/// (where races meet and finish), homes to the south, and with
/// `warehouses` a strip of cheap industry to the north.
fn town(seed: u64, warehouses: bool) -> World {
    let mut w = World::new(WorldConfig { seed, emergencies: false, ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
    for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
        w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
    }
    w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
    w.add_zone(square(-690.0, -60.0, 680.0, 52.0), LandUse::Residential);
    w.add_zone(square(10.0, -60.0, 680.0, 52.0), LandUse::Residential);
    if warehouses {
        w.add_zone(square(-300.0, 128.0, 300.0, 50.0), LandUse::Industrial);
    }
    let homes: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).take(12).collect();
    let sheds: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Industrial && l.usable()).map(|(id, _)| id).take(4).collect();
    for l in homes.into_iter().chain(sheds) {
        w.build_on(l);
        if let Some(b) = w.lots[l].building {
            w.finish_construction(b);
        }
    }
    if warehouses {
        for b in w.buildings.values_mut().filter(|b| b.land_use == LandUse::Industrial) {
            b.tier = Tier::Low;
        }
    }
    for _ in 0..60 {
        if w.citizens.len() >= 24 {
            break;
        }
        w.immigrate();
    }
    w
}

fn run(w: &mut World, n: usize, mut stop: impl FnMut(&World) -> bool) -> bool {
    let mut minds = IdleMind;
    for _ in 0..n {
        if stop(w) {
            return true;
        }
        w.tick(&mut minds);
    }
    stop(w)
}

fn civic_lot(w: &World, x: f32) -> LotId {
    w.lots
        .iter()
        .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0 && l.car_point.y.abs() < 12.0)
        .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
        .map(|(id, _)| id)
        .expect("a civic lot")
}

fn station(w: &mut World, kind: ServiceKind, x: f32, who: &[CitizenId]) -> BuildingId {
    let b = w.build_service(civic_lot(w, x), kind, Tier::Mid).unwrap();
    for &c in who {
        if let Some(old) = w.citizens[c].work {
            w.buildings[old].workers.retain(|x| *x != c);
        }
        w.citizens[c].work = Some(b);
        w.buildings[b].workers.push(c);
    }
    b
}

fn seed_where(from: u64, f: impl Fn(u64) -> bool) -> u64 {
    (from..from + 50_000).find(|&s| f(s)).expect("a seed")
}

/// Young locals with tuned coupés, at home and free.
fn racers(w: &mut World, who: &[CitizenId]) {
    let mut s = 1;
    for &c in who {
        s = seed_where(s + 1, |s| {
            let (c, h) = driver_traits(s);
            rebelliousness(c, h) > 0.7
        });
        let p = &mut w.citizens[c];
        p.seed = s;
        p.age = 24;
        p.next_decision_tick = u64::MAX / 2;
        if let Some(h) = p.home {
            p.inside = Some(h);
            p.activity = Activity::AtHome;
        }
        let near = w.citizens[c].home.and_then(|h| w.building_entrance(h)).unwrap();
        let vid = w.give_car_parked(c, near);
        w.vehicles.cars[vid].model = VehicleModel { tuned: true, ..VehicleModel::new(VehicleClass::Expensive, BodyType::Coupe) };
    }
}

/// A race night with police in town. Returns the world after it, and the
/// most cars seen racing flat out at once.
fn race_night(seed: u64) -> (World, usize) {
    let mut w = town(seed, false);
    let folk: Vec<CitizenId> = w.citizens.keys().collect();
    station(&mut w, ServiceKind::Police, 450.0, &folk[0..4]);
    station(&mut w, ServiceKind::Dispatch, 0.0, &folk[4..6]);
    racers(&mut w, &folk[8..12]);
    assert!(w.race_crew().len() >= 4, "the four with fast cars are a crew: {}", w.race_crew().len());
    let id = w.start_race(Some(Vec2::new(-500.0, 30.0))).expect("a race starts");
    assert!(w.race().is_some_and(|r| r.id == id && r.cars.len() == 4 && r.stage == RaceStage::Gathering));
    // They park up in the meet lot.
    let gathered = run(&mut w, 300, |w| w.race().is_some_and(|r| r.stage != RaceStage::Gathering || r.cars.iter().filter(|&&c| w.traffic.cars[c].in_stunt()).count() >= 3));
    assert!(gathered, "cars gathered in the lot: {:?}", w.race().map(|r| r.stage));
    if let Some(r) = w.race().filter(|r| r.stage == RaceStage::Gathering) {
        for &c in &r.cars {
            if w.traffic.cars[c].in_stunt() {
                assert!(w.traffic.cars[c].pos.distance(r.meet.centre) < r.meet.width.max(r.meet.depth), "parked in the lot");
            }
        }
    }
    let racing = run(&mut w, 200, |w| w.race().is_some_and(|r| r.stage == RaceStage::Racing));
    assert!(racing, "the flag dropped");
    let mut most = 0;
    let over = run(&mut w, 900, |w| {
        if let Some(r) = w.race() {
            let n = r.cars.iter().filter(|&&c| w.traffic.cars.get(c).is_some_and(|c| c.drive.gang.is_some_and(|g| g.race && g.phase == crate::traffic::GangPhase::Racing) && c.v > 12.0)).count();
            most = most.max(n);
        }
        w.race().is_none()
    });
    assert!(over, "the night ended");
    (w, most)
}

#[test]
fn a_street_race_gathers_runs_and_the_police_bust_it() {
    let mut busted = None;
    for seed in [11u64, 12, 13, 14, 15] {
        let (w, most) = race_night(seed);
        assert!(most >= 2, "cars raced flat out along the streets: {most}");
        let rec = w.street_events.history.iter().rev().find(|r| r.kind == EventKind::Race).expect("recorded").clone();
        assert!(rec.finish.is_some() && rec.people >= 3, "{rec:?}");
        assert!(w.street_events.news.iter().any(|n| n.headline.contains("Street race!")), "in the news");
        assert!(w.traffic.cars.values().all(|c| !(c.in_stunt() && c.drive.gang.is_some_and(|g| g.race))), "nobody left parked in a lot");
        if rec.police {
            busted = Some((seed, rec, w));
            break;
        }
    }
    let (seed, rec, w) = busted.expect("the police busted one of the races");
    assert!(rec.arrests + u32::from(rec.chase) > 0 || rec.fines > 0.0, "arrests or fines: {rec:?}");
    let fines = w.economy.ledger.today_of(Flow::Fines) + w.economy.ledger.yesterday_of(Flow::Fines);
    assert!(fines >= rec.fines - 0.01, "fines booked: {fines} of {}", rec.fines);
    assert!(w.street_events.busts_total >= 1);
    // Deterministic.
    let (again, _) = race_night(seed);
    let rec2 = again.street_events.history.iter().rev().find(|r| r.kind == EventKind::Race).unwrap();
    assert_eq!((rec.ended, rec.arrests, rec.people, rec.police), (rec2.ended, rec2.arrests, rec2.people, rec2.police));
}

/// Make `n` adults artists (by their seed) who like a night out.
fn artists(w: &mut World, n: usize) {
    let who: Vec<CitizenId> = w.citizens.iter().filter(|(_, c)| c.stage() == crate::citizens::LifeStage::Adult).map(|(id, _)| id).take(n).collect();
    let mut s = 1;
    for c in who {
        s = seed_where(s + 1, |s| super::pressure::artist_seed(s) && driver_traits(s).1 > 0.5);
        w.citizens[c].seed = s;
        w.citizens[c].age = w.citizens[c].age.clamp(20, 45);
    }
}

/// No growth: the town stays as built, so its culture is what the test
/// made it.
fn frozen(mut w: World) -> World {
    w.config.max_builds_per_interval = 0;
    w.config.immigration_interval = u64::MAX / 4;
    w
}

fn raves_in(w: &mut World, nights: u64) -> u32 {
    let start = w.street_events.raves_total;
    run(w, (nights * 1440) as usize, |_| false);
    w.street_events.raves_total - start
}

#[test]
fn raves_happen_only_where_there_is_a_scene_for_them() {
    // Warehouses but nobody to dance: no scene, no rave.
    let mut dull = frozen(town(5, true));
    assert!(dull.buildings.values().any(|b| b.land_use == LandUse::Industrial && b.tier == Tier::Low), "there are old warehouses");
    let spots = dull.rave_spots();
    assert!(!spots.is_empty() && spots.iter().all(|s| !s.pull.has_scene()), "{:?}", spots.first().map(|s| s.pull));
    let n = raves_in(&mut dull, 6);
    assert_eq!(n, 0, "no scene, no raves: {:?}", dull.street_events.history);
    assert!(dull.street_events.rave_spots.iter().all(|s| s.pull.night_chance() == 0.0));
    // The same town with artists living by the warehouses: a scene.
    let mut arty = frozen(town(5, true));
    artists(&mut arty, 14);
    let best = arty.rave_spots().into_iter().next().unwrap();
    assert!(best.pull.has_scene() && best.pull.artists >= 6, "{:?}", best.pull);
    let held = raves_in(&mut arty, 10);
    assert!(held >= 1, "a scene by the warehouses throws a rave within ten nights");
    let rec = arty.street_events.history.iter().find(|r| r.kind == EventKind::Rave).expect("recorded");
    assert!(rec.people >= 3 && rec.culture >= 1.0, "guests came, where the scene is: {rec:?}");
    // No warehouses at all: nowhere to hold one either.
    let mut none = frozen(town(5, false));
    artists(&mut none, 14);
    assert!(none.rave_spots().is_empty());
    assert_eq!(raves_in(&mut none, 3), 0);
}

#[test]
fn a_rave_draws_a_queue_lets_people_in_and_ends() {
    let mut w = town(5, true);
    artists(&mut w, 14);
    // 23:00 tonight.
    let to = (23 * 60 + 1440 - w.clock.minute_of_day() as usize) % 1440;
    run(&mut w, to, |_| false);
    let b = w.rave_spots()[0].building;
    w.street_events.rave_rolled_day = w.clock.day();
    let id = w.start_rave(b).expect("a rave");
    let r = w.rave().unwrap().clone();
    assert_eq!(r.id, id);
    assert!(r.guests.len() >= 4, "guests invited: {}", r.guests.len());
    let mut queued = 0;
    let inside = run(&mut w, 240, |w| match w.rave() {
        Some(r) => {
            queued = queued.max(r.queue.iter().filter(|&&c| w.citizens[c].pos.distance(r.door) < 30.0).count());
            r.inside.len() >= 3
        }
        None => false,
    });
    assert!(inside, "people got in");
    assert!(queued >= 1, "a queue stood at the door");
    assert!(w.citizens.values().filter(|c| c.inside == Some(b)).count() >= 3, "they are in the warehouse");
    let over = run(&mut w, 600, |w| w.rave().is_none());
    assert!(over, "it ended by dawn");
    let left = run(&mut w, 30, |w| w.citizens.values().all(|c| !matches!(c.activity, Activity::Visiting(x) if x == b)));
    assert!(left, "everyone went home");
}

/// The world's street pieces that wear, with their midpoints.
fn pieces(w: &World) -> Vec<(usize, Vec2)> {
    (0..w.roads.segments.len()).filter(|&i| road_wear::wears(&w.roads, i)).map(|i| (i, w.road_wear.mids[i])).collect()
}

#[test]
fn roads_wear_with_age_and_traffic() {
    let mut w = town(3, false);
    w.set_road_funding(0.0);
    run(&mut w, 2 * 1440 + 5, |_| false);
    let ps = pieces(&w);
    assert!(!ps.is_empty());
    let wear: Vec<f32> = ps.iter().map(|&(i, _)| w.road_wear.wear_of(i)).collect();
    let min = wear.iter().copied().fold(f32::MAX, f32::min);
    let max = wear.iter().copied().fold(0.0, f32::max);
    assert!(min >= 2.0 * road_wear::AGE_WEAR * 0.99, "every piece aged: {min}");
    assert!(max > min + 1e-4, "traffic wore the busy pieces more: {min} .. {max}");
    assert!(w.road_wear.days.last().is_some_and(|d| d.spent == 0.0), "nothing spent with the line cut");
}

#[test]
fn budget_cuts_leave_the_poor_streets_crumbling() {
    let mut w = town(3, false);
    run(&mut w, 30, |_| false);
    // Rich east, poor west.
    let (n, half, cell) = (w.land_value.n, w.land_value.half, w.land_value.cell);
    for (k, v) in w.land_value.values.iter_mut().enumerate() {
        let x = -half + ((k % n) as f32 + 0.5) * cell;
        *v = if x > 0.0 { 0.85 } else { 0.15 };
    }
    let ps = pieces(&w);
    for &(i, _) in &ps {
        w.road_wear.wear[i] = 0.6;
    }
    // Funded at a third of what is needed, for a month.
    w.set_road_funding(0.35);
    for _ in 0..30 {
        w.daily_roads();
    }
    let side = |w: &World, east: bool| {
        let v: Vec<f32> = pieces(w).into_iter().filter(|(_, m)| (m.x > 0.0) == east && m.x.abs() > 40.0).map(|(i, _)| w.road_wear.wear_of(i)).collect();
        v.iter().sum::<f32>() / v.len() as f32
    };
    let (rich, poor) = (side(&w, true), side(&w, false));
    assert!(rich < poor - 0.08, "the dear streets were fixed first: rich {rich:.2}, poor {poor:.2}");
    assert!(poor > 0.6, "the poor streets only got worse: {poor:.2}");
    let day = w.road_wear.days.last().unwrap();
    assert!(day.spent > 0.0 && day.repaired_land_value > 0.6, "the crews served the rich: {day:?}");
    assert!(w.economy.ledger.today_of(Flow::RoadMaintenance) < 0.0, "booked as road maintenance");
    let by = w.road_wear_by_land_value();
    assert!(by[0].0 > by[2].0, "{by:?}");
    // Once the dear side is smooth, the crews reach the poor streets.
    for (i, m) in pieces(&w) {
        if w.land_value.at(m) > 0.5 {
            w.road_wear.wear[i] = 0.0;
        }
    }
    let before = side(&w, false);
    w.daily_roads();
    let day = w.road_wear.days.last().unwrap();
    assert!(day.spent > 0.0 && day.repaired_land_value < 0.3 && side(&w, false) < before, "then the poor side: {day:?}");
}

/// Ticks a car takes from x = -600 to x = 600 along the avenue.
fn drive_time(wear: f32) -> u32 {
    let mut w = town(9, false);
    for (i, _) in pieces(&w) {
        w.road_wear.wear[i] = wear;
    }
    w.apply_road_wear();
    let who = w.citizens.keys().nth(3).unwrap();
    let vid = w.give_car_parked(who, Vec2::new(-650.0, -10.0));
    let to = w.roads.nearest_street(Vec2::new(650.0, 0.0)).unwrap().0;
    let car = w.drive_from_spot(vid, to, Vec2::new(650.0, -1.75), who, crate::vehicle::Errand::Trip).expect("a route");
    w.leave_building(who);
    w.citizens[who].activity = Activity::Driving { car, to: None, purpose: Purpose::Wander };
    w.citizens[who].next_decision_tick = u64::MAX / 2;
    let mut n = 0;
    run(&mut w, 400, |w| {
        n += 1;
        !w.traffic.cars.contains_key(car) || w.traffic.cars[car].pos.x > 600.0
    });
    n
}

#[test]
fn worn_roads_slow_the_traffic() {
    assert_eq!(road_wear::slow_factor(road_wear::roughness(0.1)), 1.0);
    assert!(road_wear::slow_factor(road_wear::roughness(0.95)) < 0.75);
    let (smooth, worn) = (drive_time(0.05), drive_time(0.95));
    assert!(worn as f32 > smooth as f32 * 1.12, "the same drive takes longer on crumbling roads: {smooth} vs {worn} ticks");
}

#[test]
fn wear_and_tonights_events_survive_a_save() {
    let mut w = town(11, true);
    let folk: Vec<CitizenId> = w.citizens.keys().collect();
    racers(&mut w, &folk[8..12]);
    let ps = pieces(&w);
    for (k, &(i, _)) in ps.iter().enumerate() {
        w.road_wear.wear[i] = (k % 10) as f32 / 10.0;
    }
    w.set_road_funding(0.4);
    w.apply_road_wear();
    w.start_race(Some(Vec2::new(-500.0, 30.0))).expect("a race");
    run(&mut w, 20, |_| false);
    let text = w.to_ron().expect("saves");
    let back = World::from_ron(&text).expect("loads");
    assert_eq!(back.road_wear.funding, 0.4);
    for &(i, _) in &ps {
        assert!((back.road_wear.wear_of(i) - w.road_wear.wear_of(i)).abs() < 1e-6);
    }
    let rough = |w: &World| w.traffic.net.lanes.iter().filter(|l| l.rough > 0.0).count();
    assert!(rough(&back) > 0 && rough(&back) == rough(&w), "lanes rough again after the load");
    let (a, b) = (w.race().unwrap(), back.race().unwrap());
    assert_eq!((a.id, a.cars.len(), a.stage), (b.id, b.cars.len(), b.stage));
    assert_eq!(back.street_events.news.len(), w.street_events.news.len());
}
