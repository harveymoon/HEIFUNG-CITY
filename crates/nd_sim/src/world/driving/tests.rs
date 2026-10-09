use super::*;
use crate::buildings::Tier;
use crate::mind::IdleMind;
use crate::services::{IncidentKind, ServiceKind, UnitKind, UnitState};

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// Two avenues joined by cross streets, civic land between them and
/// homes to the south; a side street makes (0, 0) a four-way junction.
fn town(seed: u64) -> World {
    let mut w = World::new(WorldConfig { seed, emergencies: false, ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
    for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
        w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
    }
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 0.0)], 1, 1);
    w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
    w.add_zone(square(-690.0, -60.0, 680.0, 52.0), LandUse::Residential);
    w.add_zone(square(10.0, -60.0, 680.0, 52.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).take(10).collect();
    for l in lots {
        w.build_on(l);
        if let Some(b) = w.lots[l].building {
            w.finish_construction(b);
        }
    }
    for _ in 0..40 {
        if w.citizens.len() >= 16 {
            break;
        }
        w.immigrate();
    }
    w
}

fn civic_lot(w: &World, x: f32) -> LotId {
    w.lots
        .iter()
        .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0 && l.car_point.y.abs() < 12.0)
        .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
        .map(|(id, _)| id)
        .expect("a civic lot")
}

/// A crewed station of `kind` near `x`, staffed by `who`.
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

fn tick(w: &mut World, n: usize, mut stop: impl FnMut(&World) -> bool) -> bool {
    let mut minds = IdleMind;
    for _ in 0..n {
        if stop(w) {
            return true;
        }
        w.tick(&mut minds);
    }
    stop(w)
}

/// A drive long overdue: the driver's anger at `extra` times the
/// expected time, and their profile then.
fn overdue_drive(seed: u64) -> Option<(CarId, f32, f32, DriverProfile, DriverProfile, u32)> {
    let mut w = town(seed);
    let found = tick(&mut w, 3000, |w| w.traffic.cars.values().any(|c| c.drive.calm.is_some() && w.citizens.contains_key(c.citizen)));
    if !found {
        return None;
    }
    let id = w.traffic.cars.iter().find(|(_, c)| c.drive.calm.is_some() && w.citizens.contains_key(c.citizen)).map(|(id, _)| id)?;
    let expected = w.traffic.cars[id].drive.expected_min;
    assert!(expected >= 1.0, "timed from its route: {expected}");
    // Pretend it has been on the road five times as long as it should.
    let back = (expected * 5.0 + 20.0) as u64;
    w.traffic.cars[id].drive.trip_start = w.clock.tick.saturating_sub(back);
    let mut minds = IdleMind;
    for _ in 0..12 {
        if !w.traffic.cars.contains_key(id) {
            return None;
        }
        w.tick(&mut minds);
    }
    let c = w.traffic.cars.get(id)?;
    Some((id, c.drive.anger, c.drive.anger_peak, c.drive.calm.unwrap(), c.drive.profile, w.driving.road_rage_today))
}

/// Two citizen cars on the road at once (a few seeds tried).
fn two_on_the_road() -> (World, CarId, CarId) {
    for seed in [3u64, 5, 9, 11, 13] {
        let mut w = town(seed);
        let mine = |w: &World| -> Vec<CarId> { w.traffic.cars.iter().filter(|(_, c)| w.citizens.contains_key(c.citizen) && c.bus.is_none() && c.cargo.is_none() && !c.siren && !c.in_stunt() && c.drive.crash.is_none()).map(|(id, _)| id).collect() };
        if tick(&mut w, 4000, |w| mine(w).len() >= 2) {
            let v = mine(&w);
            return (w, v[0], v[1]);
        }
    }
    panic!("no two citizen cars on the road");
}

fn bad_crash(w: &mut World, cars: &[CarId], cause: CrashCause) -> u32 {
    let id = 9_000;
    for &c in cars {
        w.traffic.cars[c].drive.crash = Some(crate::traffic::CrashState { id, hold: 0.0, wrecked: true });
    }
    let pos = w.traffic.cars[cars[0]].pos;
    w.handle_crash(CrashReport { id, cars: cars.to_vec(), pos, speed: 15.0, cause, severity: 0.95 });
    id
}

/// [review] The blame went to the first *citizen* driver, while the
/// cause came from the car at fault: a stranger running a red light
/// got the citizen they hit arrested.
#[test]
fn a_strangers_red_light_does_not_get_their_victim_arrested() {
    let (mut w, stranger, victim) = two_on_the_road();
    let who = w.traffic.cars[victim].citizen;
    // Someone from out of town at the wheel.
    w.traffic.cars[stranger].citizen = CitizenId::default();
    bad_crash(&mut w, &[stranger, victim], CrashCause::RedLight);
    let scene = w.services.active().find(|i| i.kind == IncidentKind::Crash).expect("a crash scene");
    assert_ne!(scene.suspect, Some(who), "the victim is no suspect");
    assert_eq!(scene.suspect, None, "the stranger is not ours to arrest");
    // [review] The crash scene is in the day's calls (with the police).
    let medical = w.services.active().filter(|i| i.kind == IncidentKind::Medical).count() as u32;
    assert_eq!(w.services.stats.incidents_today[1], 1, "the crash counted with the police");
    assert_eq!(w.services.stats.incidents_today[2], medical);
    // And a citizen who ran the light is the suspect.
    let (mut w, culprit, other) = two_on_the_road();
    let who = w.traffic.cars[culprit].citizen;
    bad_crash(&mut w, &[culprit, other], CrashCause::RedLight);
    let scene = w.services.active().find(|i| i.kind == IncidentKind::Crash).expect("a crash scene");
    assert_eq!(scene.suspect, Some(who));
}

/// [review] An injured driver whose car was not written off kept
/// driving while a medical incident waited for them at the scene.
#[test]
fn a_hurt_driver_does_not_drive_on() {
    let (mut w, a, b) = two_on_the_road();
    let drivers = [w.traffic.cars[a].citizen, w.traffic.cars[b].citizen];
    let id = 9_000;
    for c in [a, b] {
        // A bad crash, neither car written off by the traffic model.
        w.traffic.cars[c].drive.crash = Some(crate::traffic::CrashState { id, hold: 30.0, wrecked: false });
    }
    // One wreck makes it a bad crash; the other car is only dented.
    w.traffic.cars[a].drive.crash.as_mut().unwrap().wrecked = true;
    let pos = w.traffic.cars[a].pos;
    w.handle_crash(CrashReport { id, cars: vec![a, b], pos, speed: 20.0, cause: CrashCause::RearEnd, severity: 0.95 });
    let hurt = w.driving.recent.last().unwrap().injured.clone();
    assert!(!hurt.is_empty(), "someone was hurt at this severity");
    for d in hurt {
        assert!(drivers.contains(&d));
        assert!(!w.citizens[d].activity.is_traveling(), "{} is hurt and out of the car", w.citizens[d].name);
    }
}

#[test]
fn a_driver_stuck_far_too_long_loses_their_temper_the_same_way_every_run() {
    let mut checked = 0;
    for seed in [3u64, 5, 9, 11] {
        let Some(a) = overdue_drive(seed) else { continue };
        let b = overdue_drive(seed).expect("same seed, same run");
        assert_eq!((a.0, a.1, a.2, a.4), (b.0, b.1, b.2, b.4), "deterministic");
        let (_, anger, peak, calm, now, raged) = a;
        assert!(anger > 0.1 && peak >= anger, "angry after five times the expected time: {anger}");
        assert!(now.headway < calm.headway && now.aggression > calm.aggression, "drives worse: {now:?} vs {calm:?}");
        if anger >= 0.5 {
            assert!(raged >= 1);
        }
        checked += 1;
    }
    assert!(checked >= 1, "no private drive found in any town");
}

#[test]
fn a_bad_crash_is_called_in_and_the_hurt_go_to_hospital() {
    let mut w = town(7);
    let folk: Vec<CitizenId> = w.citizens.keys().collect();
    assert!(folk.len() >= 12);
    station(&mut w, ServiceKind::Police, -450.0, &folk[0..4]);
    let hospital = station(&mut w, ServiceKind::Health, 450.0, &folk[4..8]);
    let rider = folk[10];
    // A motorcyclist heading east on the avenue, on their own bike.
    let near = w.citizens[rider].home.and_then(|h| w.building_entrance(h)).unwrap();
    let vid = w.give_car_parked(rider, near);
    w.vehicles.cars[vid].model = VehicleModel::new(VehicleClass::Mid, BodyType::Motorcycle);
    let to = w.roads.nearest_street(Vec2::new(250.0, 0.0)).unwrap().0;
    let car = w.drive_from_spot(vid, to, Vec2::new(250.0, 1.75), rider, crate::vehicle::Errand::Trip).expect("a route");
    w.leave_building(rider);
    w.citizens[rider].activity = Activity::Driving { car, to: None, purpose: Purpose::Wander };
    for _ in 0..30 {
        tick(&mut w, 1, |_| false);
        if w.traffic.cars[car].v > 8.0 {
            break;
        }
    }
    tick(&mut w, 3, |_| false);
    assert!(w.traffic.cars[car].drive.profile.assigned, "the rider got a profile");
    w.traffic.crash(vec![car], CrashCause::OffRoad, 30.0);
    tick(&mut w, 1, |_| false);
    let rec = w.driving.recent.last().expect("crash recorded").clone();
    assert_eq!(rec.injured, vec![rider], "a bad spill at speed hurts the rider");
    assert_eq!(w.driving.crashes_today, 1);
    assert_eq!(w.driving.injuries_today, 1);
    let crash = w.services.incident(rec.incident.unwrap()).unwrap();
    assert_eq!(crash.kind, IncidentKind::Crash);
    assert!(crash.on_road && crash.pos.distance(rec.pos) < 1.0);
    assert!(!w.citizens[rider].owns_car && w.citizens[rider].car.is_none() && !w.vehicles.cars.contains_key(vid), "the bike is written off (scrapped)");
    let medical = w.services.incidents.iter().find(|i| i.kind == IncidentKind::Medical && i.citizen == Some(rider)).expect("a medical call").id;
    // [clarity] Lying in the road until the paramedics take them.
    assert!(w.citizens[rider].is_visible(), "the rider is out on the street");
    assert_eq!(w.pose_of(rider), crate::world::Pose::Lying, "the hurt lie on the ground");
    // An ambulance goes, the rider ends up in a hospital bed.
    let sent = tick(&mut w, 200, |w| w.services.units.values().any(|u| u.kind == UnitKind::Ambulance && u.incident == Some(medical) && u.state != UnitState::Idle));
    assert!(sent, "an ambulance was dispatched");
    // [clarity] Its crew are the hospital's own staff, and it says where it goes.
    if let Some((uid, amb)) = w.services.units.iter().find(|(_, u)| u.incident == Some(medical)).and_then(|(id, u)| Some((id, u.car?))) {
        let j = w.car_journey(amb).expect("a journey");
        assert!(j.purpose.starts_with("answering"), "{}", j.purpose);
        let crew = w.unit_crew(uid);
        assert_eq!(crew.len(), 2);
        assert!(crew.iter().all(|c| w.citizens[*c].work == Some(hospital)), "medics are hospital staff");
        assert_eq!(j.driver, crate::world::Driver::Crew(crew));
        assert!(j.to.is_some_and(|p| p.pos.is_some_and(|p| p.distance(rec.pos) < 30.0)), "going to the crash");
    }
    let admitted = tick(&mut w, 400, |w| matches!(w.citizens[rider].activity, Activity::Visiting(b) if b == hospital));
    assert!(admitted, "taken to hospital: {:?}", w.services.incident(medical).map(|i| i.stage));
    // The police clear the scene and the wreck is towed, not deleted.
    let towed = tick(&mut w, 400, |w| !w.traffic.cars.contains_key(car) && (w.driving.towed_total > 0 || !w.driving.tows.is_empty()));
    assert!(towed, "wreck towed");
    assert!(!w.services.incident(rec.incident.unwrap()).unwrap().active());
}

/// [clarity] Every vehicle says where it is coming from and going, and
/// why; a police car on patrol says so and shows its officers.
#[test]
fn vehicles_say_where_they_come_from_and_go() {
    let mut w = town(7);
    let folk: Vec<CitizenId> = w.citizens.keys().collect();
    let police = station(&mut w, ServiceKind::Police, -450.0, &folk[0..4]);
    // A driver leaving a neighbour's for home.
    let c = folk[10];
    let home = w.citizens[c].home.unwrap();
    let other = w.buildings.iter().find(|(id, b)| b.land_use == LandUse::Residential && *id != home && w.building_entrance(*id).is_some()).map(|(id, _)| id).expect("another home");
    {
        let p = &mut w.citizens[c];
        p.inside = Some(other);
        p.activity = Activity::Visiting(other);
        p.next_decision_tick = u64::MAX / 2;
    }
    let vid = w.give_car_parked(c, w.building_entrance(other).unwrap());
    assert!(w.start_drive(c, vid, home, Purpose::Home), "set off");
    let Activity::Driving { car, .. } = w.citizens[c].activity else { panic!("driving") };
    tick(&mut w, 1, |_| false);
    let j = w.car_journey(car).expect("a journey");
    assert_eq!(j.purpose, "going home");
    assert_eq!(j.from.building, Some(other), "coming from where they were");
    assert_eq!(j.to.as_ref().and_then(|p| p.building), Some(home), "going home");
    assert_eq!(j.driver, crate::world::Driver::Citizen(c));
    let line = w.activity_line(c);
    assert!(line.starts_with(&format!("going home to {}", w.buildings[home].name)), "{line}");
    // A police car on patrol: no destination, its officers aboard.
    let patrol = tick(&mut w, 600, |w| w.services.units.values().any(|u| u.kind == UnitKind::PoliceCar && u.state == UnitState::Patrol && u.car.is_some()));
    assert!(patrol, "a police car went out on patrol");
    let (uid, pcar) = w.services.units.iter().find(|(_, u)| u.kind == UnitKind::PoliceCar && u.state == UnitState::Patrol && u.car.is_some()).map(|(id, u)| (id, u.car.unwrap())).unwrap();
    let j = w.car_journey(pcar).unwrap();
    assert_eq!(j.purpose, "patrolling");
    assert!(j.to.is_none());
    let crew = w.unit_crew(uid);
    assert!(!crew.is_empty() && crew.iter().all(|o| w.citizens[*o].work == Some(police)), "the officers are the station's staff");
    assert_eq!(j.driver, crate::world::Driver::Crew(crew));
    // Through traffic is a stranger from out of town.
    if let Some((id, _)) = w.traffic.cars.iter().find(|(_, c)| c.errand == crate::vehicle::Errand::Through) {
        let j = w.car_journey(id).unwrap();
        assert_eq!(j.driver, crate::world::Driver::Unknown(crate::world::journeys::STRANGER.into()));
    }
}

/// A seed whose personality makes a takeover regular.
fn rebel_seed(from: u64) -> u64 {
    (from..from + 5000)
        .find(|&s| {
            let (c, h) = driver_traits(s);
            rebelliousness(c, h) > 0.8
        })
        .unwrap()
}

fn node_at(w: &World, at: Vec2) -> usize {
    w.roads.nodes.iter().position(|n| n.pos.distance(at) < 0.5).unwrap()
}

/// A takeover at (0, 0) with police in town. Returns its record, the
/// cars in the show, the donut cars and whether the junction was shut.
fn takeover_night(seed: u64) -> (takeover::Record, usize, usize, bool) {
    let mut w = town(seed);
    let folk: Vec<CitizenId> = w.citizens.keys().collect();
    station(&mut w, ServiceKind::Police, -450.0, &folk[0..4]);
    let mut s = 1;
    for &c in &folk[8..12] {
        s = rebel_seed(s + 1);
        let p = &mut w.citizens[c];
        p.seed = s;
        p.age = 23;
        p.next_decision_tick = u64::MAX / 2;
        if let Some(h) = p.home {
            p.inside = Some(h);
            p.activity = Activity::AtHome;
        }
        let near = w.citizens[c].home.and_then(|h| w.building_entrance(h)).unwrap();
        let vid = w.give_car_parked(c, near);
        w.vehicles.cars[vid].model = VehicleModel { tuned: true, ..VehicleModel::new(VehicleClass::Expensive, BodyType::Coupe) };
    }
    let id = w.start_takeover(Some(Vec2::ZERO)).expect("starts");
    assert!(w.takeover().is_some_and(|t| t.id == id && t.residents.len() == 4));
    let show = tick(&mut w, 300, |w| w.takeover().is_some_and(|t| t.stage == Stage::Show));
    assert!(show, "the crews gathered and the show began");
    let t = w.takeover().unwrap().clone();
    let inter = &w.traffic.net.intersections[w.traffic.net.node_intersection[t.node.0 as usize]];
    let blocked = inter.connectors.iter().all(|&(l, _)| w.traffic.net.lanes[l as usize].blocked);
    let stunts = t.cars.iter().filter(|&&c| w.traffic.cars[c].in_stunt()).count();
    let donuts = t.cars.iter().filter(|&&c| w.traffic.cars[c].drive.stunt.is_some_and(|s| s.is_donut())).count();
    assert!(stunts >= 3 && donuts >= 1, "{stunts} in the show, {donuts} spinning");
    let over = tick(&mut w, 600, |w| w.takeover().is_none());
    assert!(over, "it ended");
    let rec = w.driving.takeovers.history.last().unwrap().clone();
    let inter = &w.traffic.net.intersections[w.traffic.net.node_intersection[node_at(&w, rec.centre)]];
    assert!(inter.connectors.iter().all(|&(l, _)| !w.traffic.net.lanes[l as usize].blocked), "the junction reopened");
    assert!(w.traffic.cars.values().all(|c| !c.in_stunt()), "everyone back on the lanes");
    (rec, stunts, donuts, blocked)
}

#[test]
fn a_takeover_holds_a_junction_and_breaks_up_when_the_police_arrive() {
    let (rec, stunts, _, blocked) = takeover_night(11);
    assert!(blocked, "the junction was closed to traffic during the show");
    assert!(rec.police, "the police broke it up");
    assert!(rec.residents == 4 && rec.outsiders >= takeover::OUTSIDERS.0, "{} locals, {} outsiders", rec.residents, rec.outsiders);
    assert!(rec.participants >= stunts);
    // Deterministic.
    let (again, s2, _, _) = takeover_night(11);
    assert_eq!((rec.ended, rec.participants, rec.crowd, rec.arrests), (again.ended, again.participants, again.crowd, again.arrests));
    assert_eq!(stunts, s2);
}

/// REVIEW2 L9: when a takeover ends, a stunt car whose driver has no
/// home and no exit to drive to is towed off, not left spinning.
#[test]
fn a_stunt_car_with_nowhere_to_go_is_towed_not_left_spinning() {
    let mut w = town(11);
    let folk: Vec<CitizenId> = w.citizens.keys().collect();
    let mut s = 1;
    for &c in &folk[8..12] {
        s = rebel_seed(s + 1);
        let p = &mut w.citizens[c];
        p.seed = s;
        p.age = 23;
        p.next_decision_tick = u64::MAX / 2;
        if let Some(h) = p.home {
            p.inside = Some(h);
            p.activity = Activity::AtHome;
        }
        let near = w.citizens[c].home.and_then(|h| w.building_entrance(h)).unwrap();
        let vid = w.give_car_parked(c, near);
        w.vehicles.cars[vid].model = VehicleModel { tuned: true, ..VehicleModel::new(VehicleClass::Expensive, BodyType::Coupe) };
    }
    w.start_takeover(Some(Vec2::ZERO)).expect("starts");
    assert!(tick(&mut w, 300, |w| w.takeover().is_some_and(|t| t.stage == Stage::Show)), "the show began");
    let mut t = w.driving.takeovers.active.take().unwrap();
    let car = *t.cars.iter().find(|&&c| w.traffic.cars[c].in_stunt()).expect("a stunt car");
    // Its driver has nowhere to go: no home, and no exit off the map.
    let driver = w.traffic.cars[car].citizen;
    if let Some(p) = w.citizens.get_mut(driver) {
        p.home = None;
    }
    t.exit = None;
    w.end_takeover(t, false);
    tick(&mut w, 5, |_| false);
    assert!(!w.traffic.cars.get(car).is_some_and(|c| c.in_stunt()), "no longer doing donuts");
}

#[test]
fn drink_rises_at_a_venue_and_wears_off() {
    let mut w = town(3);
    let c = w.citizens.keys().next().unwrap();
    // A hedonist with no time for rules.
    w.citizens[c].seed = (1..5000u64).find(|&s| { let (ca, h) = driver_traits(s); h > 0.85 && rebelliousness(ca, h) > 0.7 }).unwrap();
    w.citizens[c].age = 30;
    let venue = w.buildings.keys().next().unwrap();
    w.buildings[venue].land_use = LandUse::Entertainment;
    w.citizens[c].activity = Activity::Visiting(venue);
    w.step_drink(150);
    let drunk = w.citizens[c].drunk;
    assert!(drunk >= DRUNK, "an evening at the bar: {drunk}");
    w.citizens[c].owns_car = true;
    assert!(crate::citizens::wants_to_drive_drunk(&w.citizens[c]), "a rebel drives anyway");
    w.citizens[c].activity = Activity::Idle;
    w.step_drink(600);
    assert_eq!(w.citizens[c].drunk, 0.0, "sober by morning");
}

/// A stranger's car on the road from `from` to `to`.
fn drive(w: &mut World, from: Vec2, to: Vec2) -> CarId {
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, from, ts, to).expect("route");
    let c = w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    w.traffic.cars[c].drive.profile = DriverProfile::professional(0.3);
    c
}

/// Write `car` off where it is.
fn wreck(w: &mut World, car: CarId) {
    w.traffic.crash(vec![car], CrashCause::OffRoad, 30.0);
    tick(w, 1, |_| false);
    assert!(w.driving.wrecks.iter().any(|x| x.car == car), "written off");
}

/// [services] One straight two-way street off the map at both ends and
/// no tow company: a car written off heading east has the nearest road
/// end behind it, with no way to turn. The tow used to aim for that end
/// only and never found a route, so the wreck stood for a day; now a
/// truck comes in from the map edge with its ambers on, hooks it and
/// takes it off the map, and the traffic eases past meanwhile.
#[test]
fn a_wreck_is_fetched_by_a_recovery_truck_not_left_for_hours() {
    let mut w = World::new(WorldConfig { seed: 4, emergencies: false, ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    let y = 0.5 * crate::roads::LANE_WIDTH;
    let car = drive(&mut w, Vec2::new(-560.0, y), Vec2::new(650.0, y));
    tick(&mut w, 3, |_| false);
    wreck(&mut w, car);
    assert!(w.tow(car), "the swap fallback finds the exit ahead, not just the nearest end");
    assert_eq!(w.driving.tows.len(), 1);
    // Again, for the dispatch.
    let mut w = World::new(WorldConfig { seed: 4, emergencies: false, ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    let car = drive(&mut w, Vec2::new(-560.0, y), Vec2::new(650.0, y));
    tick(&mut w, 3, |_| false);
    wreck(&mut w, car);
    assert!(w.traffic.cars[car].lateral >= 1.5, "pushed onto the shoulder");
    // On its roof: the crew rights it before hooking it.
    w.driving.wrecks.iter_mut().find(|x| x.car == car).unwrap().flipped = 2;
    let mut righting: Option<u64> = None;
    let wx = w.traffic.cars[car].pos.x;
    // A queue builds behind it.
    let mut behind = Vec::new();
    for k in 0..6 {
        behind.push(drive(&mut w, Vec2::new(-690.0 + 15.0 * k as f32, y), Vec2::new(650.0, y)));
    }
    let mut amber = false;
    let mut minds = IdleMind;
    let mut gone = None;
    for k in 0..240u64 {
        w.tick(&mut minds);
        if let Some(t) = w.driving.wrecks.iter().find_map(|x| x.truck) {
            let tc = &w.traffic.cars[t];
            assert_eq!(tc.errand, Errand::Recover(car));
            amber |= tc.drive.amber;
        }
        if let Some(x) = w.driving.wrecks.iter().find(|x| x.car == car && x.righted_at > 0) {
            righting.get_or_insert(x.righted_at);
        }
        if !w.traffic.cars.contains_key(car) {
            gone = Some(k);
            break;
        }
    }
    let gone = gone.expect("the wreck was towed within four hours");
    assert!(gone < TOW_UNREPORTED + TOW_ARRIVE_MAX + RIGHTING_MIN + 10, "and soon: {gone} min");
    assert!(amber, "the truck came with its ambers on");
    let righted = righting.expect("righted before it was hooked");
    assert!(w.clock.tick >= righted, "hooked only once back on its wheels");
    assert_eq!(w.driving.tows.len(), 1, "on the hook, off the map");
    let past = behind.iter().filter(|&&c| w.traffic.cars.get(c).is_none_or(|c| c.pos.x > wx)).count();
    assert!(past >= 4, "the queue eased past the wreck meanwhile: {past} of 6");
}

/// [services] A recovery truck stuck behind a queue near the wreck it
/// is going to takes the off-law moves (here the opposing lane, clear)
/// but no siren: nobody pulls over for it.
#[test]
fn a_recovery_truck_close_to_its_wreck_drives_round_the_queue() {
    let mut w = World::new(WorldConfig { seed: 5, emergencies: false, ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    let y = 0.5 * crate::roads::LANE_WIDTH;
    // A wreck in the middle of the lane that cannot be pushed aside
    // (as if there were no shoulder), a queue behind it.
    let car = drive(&mut w, Vec2::new(100.0, y), Vec2::new(650.0, y));
    tick(&mut w, 2, |_| false);
    wreck(&mut w, car);
    w.traffic.cars[car].lateral = 0.0;
    let wl = w.traffic.cars[car].route[w.traffic.cars[car].idx];
    let ws = w.traffic.cars[car].s;
    let mut queue = Vec::new();
    for k in 0..5 {
        let c = drive(&mut w, Vec2::new(-200.0 - 9.0 * k as f32, y), Vec2::new(650.0, y));
        queue.push(c);
    }
    let truck = drive(&mut w, Vec2::new(-320.0, y), Vec2::new(650.0, y));
    let (route, _, _) = w.traffic.net.route_lanes(w.traffic.cars[truck].route[0], w.traffic.cars[truck].s, wl, ws - 7.0).unwrap();
    assert!(w.traffic.follow(truck, route, ws - 7.0));
    w.traffic.cars[truck].errand = Errand::Recover(car);
    w.traffic.cars[truck].model = super::super::enforcement::tow_model();
    let k = w.driving.wrecks.iter().position(|x| x.car == car).unwrap();
    w.driving.wrecks[k].truck = Some(truck);
    w.driving.wrecks[k].sent = w.clock.tick;
    // Keep the wreck where it is (in the lane) for this test.
    let mut minds = IdleMind;
    let (mut overtook, mut pulled) = (false, false);
    for _ in 0..40 {
        w.tick(&mut minds);
        if let Some(c) = w.traffic.cars.get_mut(car) {
            c.lateral = 0.0;
        }
        let Some(t) = w.traffic.cars.get(truck) else { break };
        overtook |= t.drive.offlaw.mode == crate::traffic::OffLawMove::Overtake && t.drive.recovering;
        pulled |= queue.iter().any(|&q| w.traffic.cars.get(q).is_some_and(|q| q.lateral >= 1.0));
        assert!(!t.siren, "no siren");
        if !w.traffic.cars.contains_key(car) {
            break;
        }
    }
    assert!(overtook, "round the queue on the opposing lane");
    assert!(!pulled, "nobody pulled over for ambers");
    assert!(!w.traffic.cars.contains_key(car), "and it hooked the wreck");
}
