//! [concerns] World-level tests: complaints filed where they happen,
//! happiness sampled, both saved.

use super::*;
use crate::clock::MINUTES_PER_DAY;
use crate::concerns::{Concern, MOOD_HOURS};
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

/// A street, one finished block of homes and its first residents.
fn town() -> (World, BuildingId) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Residential).min_by(|a, b| a.1.center.distance(Vec2::new(100.0, 20.0)).total_cmp(&b.1.center.distance(Vec2::new(100.0, 20.0)))).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let home = w.lots[lot].building.unwrap();
    w.finish_construction(home);
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    assert!(!w.citizens.is_empty(), "someone moved in");
    (w, home)
}

#[test]
fn hunger_is_filed_under_food_once_a_day() {
    let (mut w, _) = town();
    let c = w.citizens.keys().next().unwrap();
    w.citizens[c].satiety = 0.0;
    w.citizens[c].savings = 1000.0;
    let today = w.clock.day();
    w.daily_citizen(c);
    w.daily_citizen(c);
    assert_eq!(w.concerns.count(Concern::Food, today, today), 1, "one person, one count a day");
    let q = w.concerns.quotes(Concern::Food).next().unwrap();
    assert_eq!(q.citizen, c);
    assert!(q.text.to_lowercase().contains("hungry"), "{}", q.text);
    assert_eq!(w.concerns.ranked(today)[0].concern, Concern::Food);
}

#[test]
fn a_memory_is_quoted_and_placed_where_it_was_said() {
    let (mut w, home) = town();
    let c = w.citizens.keys().next().unwrap();
    let tick = w.clock.tick;
    w.citizens[c].remember(tick, "Shelves were empty at Corner Shop.");
    let at = w.building_lot(home).map(|l| (home, l.center));
    assert!(w.concerns.heard_at(c, &w.citizens[c], Concern::Food, 0.4, at));
    let q = w.concerns.quotes(Concern::Food).next().unwrap();
    assert_eq!(q.text, "Shelves were empty at Corner Shop.");
    assert_eq!(q.building, Some(home));
    let (spot, n) = w.concerns.hotspot(Concern::Food, w.clock.day(), 7).unwrap();
    assert_eq!(n, 1);
    assert_eq!(w.building_nearest(spot), Some(home));
}

#[test]
fn happiness_is_sampled_daily_and_hourly_and_survives_a_load() {
    let (mut w, _) = town();
    let ids: Vec<CitizenId> = w.citizens.keys().collect();
    for (i, id) in ids.iter().enumerate() {
        w.citizens[*id].mood = if i % 2 == 0 { 0.2 } else { 0.9 };
    }
    let mut minds = IdleMind;
    let to = (w.clock.day() + 2) * MINUTES_PER_DAY + 61;
    let first_midnight = w.clock.day() + 1;
    while w.clock.tick < to {
        w.tick(&mut minds);
    }
    assert_eq!(w.concerns.mood_days.len(), 2, "one sample each midnight");
    assert_eq!(w.concerns.mood_days[0].day(), first_midnight);
    let hours = w.concerns.mood_hours.len();
    assert!(hours >= 40 && hours <= MOOD_HOURS, "{hours} hourly samples");
    let s = w.concerns.mood_days[1];
    assert!(s.people > 0 && s.mean > 0.0 && s.mean < 1.0);
    assert!(s.unhappy + s.happy <= 1.0 + 1e-6);
    assert_eq!(s.class_people.iter().sum::<u32>(), s.people);
    let a = ids[0];
    if w.citizens.contains_key(a) {
        w.complain(a, Concern::Housing, 0.5, "Rent is too high.");
    }
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.concerns.mood_days, w.concerns.mood_days);
    assert_eq!(back.concerns.mood_hours, w.concerns.mood_hours);
    assert_eq!(back.concerns.days.len(), w.concerns.days.len());
    let today = back.clock.day();
    assert_eq!(back.concerns.count(Concern::Housing, today, today), w.concerns.count(Concern::Housing, today, today));
    assert_eq!(back.concerns.ranked(today), w.concerns.ranked(today));
}

#[test]
fn a_save_from_before_concerns_loads_with_none() {
    let (w, _) = town();
    let text = w.to_ron().unwrap();
    let start = text.find("concerns:").expect("concerns are saved");
    let mut depth = 0i32;
    let mut end = start;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let old = format!("{}{}", &text[..start], text[end..].trim_start_matches([',', '\n', '\r', ' ']));
    assert!(!old.contains("concerns:"));
    let back = World::from_ron(&old).unwrap();
    assert!(back.concerns.days.is_empty() && back.concerns.mood_days.is_empty());
}

// ── [clarity] dangerous streets and far hospitals ───────────────────────────

#[test]
fn crashes_piling_up_on_a_street_bring_dangerous_street_complaints() {
    let (mut w, home) = town();
    let at = w.building_lot(home).unwrap().car_point;
    let seg = w.building_lot(home).unwrap().car_segment.unwrap();
    for i in 0..crate::world::journeys::DANGEROUS_STREET {
        w.journeys.crash_sites.push(crate::world::journeys::CrashSite { pos: at + Vec2::new(i as f32 * 3.0, 0.0), tick: 0 });
    }
    let per = w.crashes_per_segment();
    assert_eq!(per.first().map(|p| p.0), Some(seg), "counted on the street out front: {per:?}");
    assert_eq!(per[0].1, crate::world::journeys::DANGEROUS_STREET);
    let today = w.clock.day();
    w.daily_dangerous_streets();
    assert!(w.concerns.count(Concern::DangerousStreets, today, today) >= 1, "the residents complain");
    let q = w.concerns.quotes(Concern::DangerousStreets).next().unwrap();
    assert!(q.text.contains("crashes"), "{}", q.text);
}

#[test]
fn a_town_without_a_hospital_complains_of_healthcare() {
    let (mut w, _) = town();
    // A town big enough to expect one.
    let first = w.citizens.values().next().unwrap().clone();
    while w.citizens.len() < crate::world::journeys::HEALTHCARE_TOWN {
        w.citizens.insert(first.clone());
    }
    let today = w.clock.day();
    w.daily_healthcare();
    assert!(w.concerns.count(Concern::Healthcare, today, today) >= 1);
    assert!(w.concerns.quotes(Concern::Healthcare).next().unwrap().text.contains("hospital"));
}
