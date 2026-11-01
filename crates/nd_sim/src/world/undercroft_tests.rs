//! [r5-layered] Life under the raised roads: the bays between the columns,
//! what they become, who runs and visits them, and the camps that shelter
//! there.

use super::*;
use crate::undercroft::*;

struct IdleMind;
impl MindProvider for IdleMind {
    fn name(&self) -> &'static str {
        "idle"
    }
    fn decide(&mut self, _: &MindContext) -> Option<Decision> {
        None
    }
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn way(points: &[Vec2], levels: &[f32], kind: RoadKind, lanes: u8) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: lanes, lanes_bwd: lanes, curved: false, kind, elevation: levels.to_vec(), car_only: None }
}

fn try_build(w: &mut World, g: Gesture) -> Result<(), String> {
    w.project = None;
    w.add_gesture(g);
    w.try_implement()
}

/// A street with homes north of it, a cross street running south under a
/// raised street `level` m up over open ground at y = -70.
fn town(level: f32, people: bool) -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    // The cross street runs under the deck (a deck too low to pass over
    // it stops short of it).
    let south = if level >= 4.5 { -200.0 } else { -40.0 };
    w.add_road(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, south)], 1, 1);
    let pts = [Vec2::new(-440.0, -70.0), Vec2::new(-330.0, -70.0), Vec2::new(330.0, -70.0), Vec2::new(440.0, -70.0)];
    try_build(&mut w, way(&pts, &[0.0, level, level, 0.0], RoadKind::Street, 1)).expect("the raised street is built");
    assert!(w.pillars.len() >= 20, "columns along it: {}", w.pillars.len());
    if people {
        w.add_zone(square(-300.0, 10.0, 290.0, 200.0), LandUse::Residential);
        w.add_zone(square(10.0, 10.0, 290.0, 200.0), LandUse::Residential);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
        for id in lots {
            w.build_on(id);
        }
        let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
        }
        let mut guard = 0;
        while w.citizens.len() < 60 && guard < 48 {
            run(&mut w, 60);
            guard += 1;
        }
        assert!(w.citizens.len() >= 60, "people moved in: {}", w.citizens.len());
    }
    w.refresh_bays();
    w
}

/// [r5-layered] Between the columns of a raised street over open ground
/// are bays of sheltered ground: none on a column, none over the street
/// that runs under the deck, all with headroom; found the same again and
/// after a save and load.
#[test]
fn a_raised_street_has_bays_between_its_columns() {
    let w = town(9.0, false);
    let bays = w.undercroft.bays.clone();
    assert!(bays.len() >= 12, "bays under the deck: {}", bays.len());
    for b in &bays {
        assert!(b.clearance >= MIN_CLEARANCE && b.clearance <= 9.0 - SOFFIT_DROP + 0.01, "headroom {}", b.clearance);
        assert!((b.pos.y + 70.0).abs() < 1.0, "under the deck: {:?}", b.pos);
        assert!(w.pillars.iter().all(|p| !b.contains(p.at, -0.5)), "a column in the bay at {:?}", b.pos);
        assert!(b.pos.x.abs() > 8.0 + b.len * 0.5 * 0.45, "a bay over the cross street at {:?} ({} m)", b.pos, b.len);
        assert!(b.len >= MIN_BAY && b.len <= MAX_BAY + 0.1);
        assert_eq!(b.use_, UnderUse::Bare);
    }
    let again = w.find_bays();
    assert_eq!(again.iter().map(|b| b.id).collect::<Vec<_>>(), bays.iter().map(|b| b.id).collect::<Vec<_>>(), "found the same again");
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.undercroft.bays.iter().map(|b| b.id).collect::<Vec<_>>(), bays.iter().map(|b| b.id).collect::<Vec<_>>(), "and after a load");
}

/// [r5-layered] A deck too low to stand under has no bays.
#[test]
fn a_deck_too_low_to_stand_under_has_no_bays() {
    let w = town(3.6, false);
    assert!(w.undercroft.bays.is_empty(), "{} bays under a 3.6 m deck", w.undercroft.bays.len());
}

/// [r5-layered] With homes round them the bays take uses over a few days
/// (markets, hawkers, workshops by the street, a court), run by households
/// from the blocks round them; a stroller stops at an open one, pays the
/// stallholders and stays a while; the stallholders go to their stalls
/// when they open.
#[test]
fn bays_take_uses_and_people_come() {
    let mut w = town(9.0, true);
    let n = w.undercroft.bays.len();
    assert!(n >= 8, "bays: {n}");
    for _ in 0..10 {
        w.clock.tick += 1440;
        w.daily_undercroft();
    }
    let counts = w.undercroft.counts();
    let used = n - counts[UnderUse::Bare as usize];
    assert!(used * 3 >= n, "a good share of the bays took a use in ten days: {used} of {n} ({counts:?})");
    let trades = [UnderUse::Market, UnderUse::Hawkers, UnderUse::Workshop].iter().filter(|u| counts[**u as usize] > 0).count();
    assert!(trades >= 2, "more than one kind of trade: {counts:?}");
    for b in w.undercroft.bays.iter().filter(|b| b.use_.is_trade()) {
        assert!(!b.holders.is_empty(), "a trade with nobody to run it");
    }
    if let Some(b) = w.undercroft.bays.iter().find(|b| b.use_ == UnderUse::Workshop) {
        assert!(b.pos.x.abs() < 60.0, "workshops by the street a van comes in on: {:?}", b.pos);
    }
    let holders: Vec<HouseholdId> = w.undercroft.bays.iter().flat_map(|b| b.holders.iter().copied()).collect();
    let mut uniq = holders.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), holders.len(), "no household runs two bays");

    // Six in the evening: everyone home, awake.
    let day = w.clock.day();
    w.clock.tick = (day + 1) * 1440 + 18 * 60 + 1;
    for c in w.citizens.values_mut() {
        if let Some(h) = c.home {
            c.activity = Activity::AtHome;
            c.inside = Some(h);
            c.asleep = false;
            c.savings = c.savings.max(60.0);
            c.next_decision_tick = u64::MAX;
        }
    }
    let ids: Vec<CitizenId> = w.citizens.keys().collect();
    let mut went = Vec::new();
    for _ in 0..30 {
        for &id in &ids {
            if !w.undercroft.trips.contains_key(&id) && w.under_stroll(id) {
                went.push(id);
            }
        }
        w.clock.tick += 1;
    }
    assert!(went.len() >= 5, "strollers stop at the bays: {}", went.len());
    let money_before: f32 = holders.iter().map(|h| w.households[*h].money).sum();
    for _ in 0..90 {
        let mut minds = IdleMind;
        w.step_citizens(&mut minds);
        w.clock.tick += 1;
    }
    let visits: u32 = w.undercroft.bays.iter().map(|b| b.visits_today).sum();
    assert!(visits >= 3, "visits counted: {visits} of {} who went", went.len());
    let here = went.iter().filter(|&&id| matches!(w.citizens[id].activity, Activity::Idle) && w.bay_at(w.citizens[id].pos, 1.5).is_some()).count();
    assert!(here >= 1, "someone is at a bay");
    let takings: f32 = w.undercroft.bays.iter().map(|b| b.takings_today).sum();
    let money_after: f32 = holders.iter().map(|h| w.households[*h].money).sum();
    if takings > 0.0 {
        assert!(money_after > money_before, "the stallholders took the money: {money_before} -> {money_after}");
    }
    // Opening time: a stallholder walks to the stall.
    let open = w.undercroft.bays.iter().filter(|b| b.use_.is_trade()).map(|b| b.use_.hours().0).min().unwrap();
    w.clock.tick = (day + 2) * 1440 + open as u64 * 60 + 29;
    for c in w.citizens.values_mut() {
        if let Some(h) = c.home {
            c.activity = Activity::AtHome;
            c.inside = Some(h);
            c.asleep = false;
        }
    }
    w.undercroft.trips.clear();
    w.hourly_undercroft();
    let staff = w.undercroft.trips.keys().filter(|id| w.citizens[**id].household.is_some_and(|h| holders.contains(&h))).count();
    assert!(staff >= 1, "a stallholder sets off for the stall at opening");
}

/// [r5-layered] People with nowhere to live shelter under the deck: a camp
/// pitches in a bay (over a vacant lot further off) and the bay is a camp.
#[test]
fn the_homeless_shelter_under_the_deck() {
    let mut w = town(9.0, true);
    let Some((hid, _)) = w.households.iter().find(|(_, h)| h.home.is_some()).map(|(id, h)| (id, h.clone())) else { panic!("a household") };
    let members = w.households[hid].members.clone();
    for m in &members {
        w.citizens[*m].owns_car = false;
    }
    let home = w.households[hid].home.take().unwrap();
    w.buildings[home].households.retain(|&h| h != hid);
    // Cheap land everywhere (a test town's land is middling).
    w.society.homeless.origin.push((hid, Vec2::new(-100.0, -40.0)));
    w.settle_homeless(hid);
    let camp = w.society.homeless.camp_of(hid).cloned();
    let lv = w.land_value.at(Vec2::new(-100.0, -70.0));
    if lv < crate::homeless::UNDER_CAMP_LV {
        let camp = camp.expect("a camp");
        assert!(camp.under, "the camp is under the deck: {:?} (land value {lv})", camp.pos);
        w.hourly_undercroft();
        assert_eq!(w.bay_at(camp.pos, 1.0).map(|b| b.use_), Some(UnderUse::Shelter));
        // A road works hour: it stays while its bay does.
        w.move_camps_on(true);
        assert!(w.society.homeless.camp_of(hid).is_some_and(|c| c.under));
    }
}
