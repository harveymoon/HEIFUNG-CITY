//! [pt-growth] Tests of the playtest round's growth work: the land value
//! scale, the land density (foot traffic), lot assembly, re-densification,
//! foreclosure, construction in working hours and the demolition reasons.

use super::*;
use crate::buildings::DemolitionReason;
use crate::estate::*;
use crate::land_value::{compress, Channel, COMPRESS_KNEE};
use crate::mind::IdleMind;

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn town() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn built(w: &mut World, lot: LotId) -> Option<BuildingId> {
    w.build_on(lot);
    let b = w.lots[lot].building?;
    w.finish_construction(b);
    Some(b)
}

/// Pressure of `value` everywhere within 200 m of `at`, as if sampled for days.
fn press(w: &mut World, at: Vec2, value: f32) {
    let p = &mut w.estate.pressure;
    let present: Vec<f32> = (0..p.value.len()).map(|i| if p.cell_center(i).distance(at) < 200.0 { value } else { 0.0 }).collect();
    let zeros = vec![0.0f32; present.len()];
    p.samples = 0;
    p.sample(&present, &zeros);
    p.samples = 48;
    p.always = true;
}

// ── 1. the land value scale ──────────────────────────────────────────────

#[test]
fn the_land_value_scale_saturates_and_one_is_out_of_reach() {
    // Below the knee a score is the value; above, a curve towards 1.
    assert_eq!(compress(0.3), 0.3);
    assert_eq!(compress(-0.4), 0.0);
    assert_eq!(compress(COMPRESS_KNEE), COMPRESS_KNEE);
    let mut last = 0.0;
    for i in 0..400 {
        let v = compress(i as f32 * 0.025);
        assert!(v >= last && v < 1.0, "monotone and under 1: {v}");
        last = v;
    }
    // The old clamp gave 1.0 to anything past 0.75: now a sum of 1 is
    // upper-middle land, 2 a fine downtown, and only a score of about 2.9
    // reaches 0.95.
    assert!(compress(1.0) > 0.66 && compress(1.0) < 0.75, "{}", compress(1.0));
    assert!(compress(2.0) < 0.95 && compress(3.5) > 0.95, "{} {}", compress(2.0), compress(3.5));
}

#[test]
fn like_adds_to_like_with_diminishing_returns() {
    // Twenty parks round a block are one great park nearby, not twenty
    // times the lift: the channel saturates at its cap.
    let lv0 = LandValue::new(400.0, 32.0);
    let mut lv = lv0.clone();
    let mut b = LandValueBuilder::new(&lv);
    b.channel(Channel::Parks);
    for i in 0..20 {
        b.splat(Vec2::new(-200.0 + (i % 5) as f32 * 10.0, (i / 5) as f32 * 10.0), 180.0, 0.35);
    }
    b.channel(Channel::Other);
    b.splat(Vec2::new(200.0, 0.0), 180.0, 0.35);
    b.finish(0.3, &mut lv);
    let (cap, _) = Channel::Parks.cap().unwrap();
    let many = lv.raw_at(Vec2::new(-180.0, 15.0)) - 0.3;
    assert!(many > 0.0 && many <= cap + 1e-4, "twenty parks: {many} (cap {cap})");
    let one_plain = lv.raw_at(Vec2::new(200.0, 0.0)) - 0.3;
    assert!(one_plain > 0.1, "an uncapped term is the plain sum: {one_plain}");
    // A dense, amenity-rich block (every channel near its cap) is dear but
    // short of 1.
    let mut lv = lv0.clone();
    let mut b = LandValueBuilder::new(&lv);
    for ch in [Channel::Roads, Channel::Parks, Channel::Homes, Channel::Civic, Channel::Commerce, Channel::Services, Channel::Transit, Channel::Shore, Channel::Landmarks] {
        b.channel(ch);
        for _ in 0..30 {
            b.splat(Vec2::ZERO, 300.0, 0.5);
        }
    }
    b.finish(crate::land_value::BASE, &mut lv);
    let top = lv.at(Vec2::ZERO);
    assert!(top > 0.9 && top < 0.99, "the best block in town: {top}");
    // A plain field: low-tier land.
    assert!(lv0.at(Vec2::ZERO) < 0.38);
}

#[test]
fn nuisance_saturates_too_and_homes_still_feel_it_most() {
    let mut lv = LandValue::new(400.0, 32.0);
    let mut b = LandValueBuilder::new(&lv);
    // A whole works district: twenty smelters.
    for i in 0..20 {
        b.splat_nuisance(Vec2::new(-150.0 + i as f32 * 15.0, 0.0), 220.0, 0.3);
    }
    b.finish(0.6, &mut lv);
    let p = Vec2::ZERO;
    let (cap, _) = Channel::Nuisance.cap().unwrap();
    assert!(lv.nuisance_at(p) <= cap + 1e-4, "{}", lv.nuisance_at(p));
    assert!(lv.at(p) >= 0.6 - cap - 1e-3, "a works district costs at most its cap: {}", lv.at(p));
    let (home, works) = (lv.for_use(p, LandUse::Residential), lv.for_use(p, LandUse::Industrial));
    assert!(home < lv.at(p) && lv.at(p) < works, "{home} {} {works}", lv.at(p));
}

#[test]
fn a_real_town_no_longer_reads_one_everywhere() {
    // The demo district a few days in: before the rescale most built lots
    // read 0.95-1.0; now the spread is low to upper-middle, never 1.
    let mut w = World::new(WorldConfig::default());
    w.drain_events();
    crate::scenarios::demo_district(&mut w);
    let mut minds = IdleMind;
    for _ in 0..(5 * 1440) {
        w.tick(&mut minds);
    }
    w.update_land_value();
    let mut v: Vec<f32> = w.buildings.values().filter(|b| b.land_use != LandUse::Park).filter_map(|b| w.lots.get(b.lot)).map(|l| w.land_value.at(l.center)).collect();
    assert!(v.len() > 50, "a town: {}", v.len());
    v.sort_by(|a, b| a.total_cmp(b));
    let top = v.iter().filter(|x| **x >= 0.95).count();
    assert_eq!(top, 0, "no lot at the top of the scale in a small town");
    let median = v[v.len() / 2];
    assert!(median > 0.1 && median < 0.66, "median {median}");
}

// ── 2. land density and the growth model ─────────────────────────────────

#[test]
fn foot_traffic_is_smoothed_over_days_and_raises_the_land_density() {
    let mut p = Pressure::new(400.0);
    let quiet = vec![0.0f32; p.value.len()];
    p.sample(&quiet, &quiet);
    let at = Vec2::new(10.0, 10.0);
    let i = p.index(at).unwrap();
    let mut busy = quiet.clone();
    busy[i] = 40.0;
    // A day of a busy street: the score climbs, a day and a half to rise.
    for _ in 0..24 {
        p.sample_foot(&busy);
    }
    let after_day = p.foot[i];
    assert!(after_day > 10.0 && after_day < 40.0, "{after_day}");
    // A quiet day takes less off than the busy day put on.
    for _ in 0..24 {
        p.sample_foot(&quiet);
    }
    assert!(p.foot[i] > after_day * 0.6, "falls slowly: {} from {after_day}", p.foot[i]);
    // The land density is the people present plus the street life.
    p.samples = 30;
    assert!(p.density_at(at) > p.at(at) + 10.0, "{} vs {}", p.density_at(at), p.at(at));
    assert!(p.floors_at(at) > 2, "a busy street justifies floors: {}", p.floors_at(at));
}

#[test]
fn hourly_sampling_counts_people_out_on_foot() {
    let mut w = town();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_zone(rect(-200.0, 8.0, 400.0, 60.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.keys().collect();
    for l in lots {
        built(&mut w, l);
    }
    run(&mut w, 3 * 1440);
    assert!(w.estate.pressure.foot.len() == w.estate.pressure.value.len(), "the foot grid is sampled");
    // An old save (no foot grid) gets one on its first sample.
    w.estate.pressure.foot.clear();
    w.hourly_pressure();
    assert_eq!(w.estate.pressure.foot.len(), w.estate.pressure.value.len());
}

/// A street of small one-storey houses, days old, demand for homes.
fn bungalow_street() -> (World, Vec<BuildingId>) {
    let mut w = town();
    w.add_road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], 1, 1);
    w.add_zone(rect(-300.0, 8.0, 600.0, 36.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential && l.car_access()).map(|(id, _)| id).collect();
    let mut homes = Vec::new();
    for l in lots {
        homes.extend(built(&mut w, l));
    }
    for &b in &homes {
        let x = &mut w.buildings[b];
        x.floors = 1;
        x.tier = Tier::Low;
        x.built_tick = 0;
        x.recompute_capacity();
    }
    w.clock = crate::clock::SimClock::starting_at(6, 1, 0);
    w.economy.demand[LandUse::Residential.index()] = 0.8;
    (w, homes)
}

#[test]
fn developers_assemble_small_plots_into_one_bigger_building() {
    let (mut w, homes) = bungalow_street();
    assert!(homes.len() >= 6, "a street of houses: {}", homes.len());
    let small = homes.iter().filter(|b| w.building_lot(**b).unwrap().area() < ASSEMBLY_SMALL_LOT).count();
    assert!(small >= 5, "small plots: {small}");
    let at = w.building_lot(homes[homes.len() / 2]).unwrap().center;
    press(&mut w, at, 80.0);
    // A neighbourhood of floors round it, so the neighbour rule allows height.
    let bid = homes[homes.len() / 2];
    assert!(w.pressure_floors(w.buildings[bid].lot) >= ASSEMBLY_MIN_FLOORS);
    let mut rng = crate::rng_for(3, 4);
    let lots_before = w.buildings[bid].plot.lots(w.buildings[bid].lot).len();
    let homes_before: u16 = homes.iter().map(|b| w.buildings[*b].homes).sum();
    assert!(w.assemble_plots(bid, &mut rng), "the developer buys the row");
    let b = &w.buildings[bid];
    let plots = 1 + b.plot.extra_lots.len() + b.plot.pending_lots.len();
    assert!(plots >= ASSEMBLY_MIN_LOTS && plots <= ASSEMBLY_MAX_LOTS && plots > lots_before, "{plots} plots");
    assert_eq!(b.plot.form, Some(Form::Merged));
    assert!(b.floors >= ASSEMBLY_MIN_FLOORS, "{} floors", b.floors);
    assert_eq!(b.demolition.as_ref().map(|d| d.reason), Some(DemolitionReason::Assembly), "it says why it comes down");
    assert!(matches!(w.owner_of(bid).map(|o| &o.1.kind), Some(OwnerKind::Developer)));
    // The neighbours come down too, for the same reason, and join when clear.
    let pending = b.plot.pending_lots.clone();
    for l in &pending {
        let n = w.lots[*l].building.unwrap();
        assert_eq!(w.buildings[n].demolition.as_ref().map(|d| d.reason), Some(DemolitionReason::Assembly));
    }
    assert_eq!(w.estate.stats.assemblies_total, 1);
    run(&mut w, 4 * 1440);
    let b = &w.buildings[bid];
    assert!(b.plot.pending_lots.is_empty() && b.plot.extra_lots.len() + 1 == plots, "every plot joined");
    w.finish_construction(bid);
    let b = &w.buildings[bid];
    let replaced: u16 = (plots as u16) * 6;
    assert!(b.homes > replaced, "more homes than the {plots} bungalows held: {} (all the street had {homes_before})", b.homes);
    let fp = w.footprint(bid).unwrap();
    assert!(fp.area() > w.lots[b.lot].area() * 2.0, "drawn across the plots");
}

#[test]
fn a_quiet_street_is_not_assembled() {
    let (mut w, homes) = bungalow_street();
    let at = w.building_lot(homes[0]).unwrap().center;
    press(&mut w, at + Vec2::new(5000.0, 0.0), 80.0);
    let mut rng = crate::rng_for(3, 4);
    for &b in &homes {
        assert!(!w.assemble_plots(b, &mut rng), "no density, no assembly");
    }
}

#[test]
fn a_developers_block_is_rebuilt_taller_again_as_the_street_gets_busier() {
    let (mut w, homes) = bungalow_street();
    let bid = homes[homes.len() / 2];
    let at = w.building_lot(bid).unwrap().center;
    press(&mut w, at, 80.0);
    let mut rng = crate::rng_for(3, 4);
    assert!(w.assemble_plots(bid, &mut rng));
    run(&mut w, 4 * 1440);
    w.finish_construction(bid);
    let first = w.buildings[bid].floors;
    w.buildings[bid].built_tick = 0;
    // The street round it has grown up meanwhile (the neighbour rule).
    for &h in &homes {
        if h != bid && w.buildings.get(h).is_some_and(|x| x.construction.is_none()) {
            w.buildings[h].floors = 8;
        }
    }
    // Much busier now: the developer's own block is a candidate again.
    press(&mut w, at, 400.0);
    w.estate.pressure.always = true;
    w.economy.demand[LandUse::Residential.index()] = 0.9;
    let mut grew = false;
    for _ in 0..20 {
        w.densify();
        w.clock.tick += 1440;
        if w.buildings[bid].floors > first {
            grew = true;
            break;
        }
    }
    assert!(grew, "rebuilt taller than {first}: {}", w.buildings[bid].floors);
}

#[test]
fn a_half_empty_block_is_lost_to_a_developer_who_cuts_the_rent() {
    let (mut w, homes) = bungalow_street();
    let bid = homes[0];
    {
        let b = &mut w.buildings[bid];
        b.tier = Tier::High;
        b.floors = 4;
        b.recompute_capacity();
        b.update_rent(0.7);
    }
    let rent = w.buildings[bid].rent;
    assert!(w.buildings[bid].households.is_empty());
    w.economy.waiting_immigrants = 10;
    for _ in 0..FORECLOSE_DAYS {
        w.foreclose_empty();
    }
    let b = &w.buildings[bid];
    assert!(b.tier < Tier::High, "a step down: {:?}", b.tier);
    assert!(b.rent < rent, "the rent came down: {} from {rent}", b.rent);
    assert!(matches!(w.owner_of(bid).map(|o| &o.1.kind), Some(OwnerKind::Developer)), "the owner lost it");
    assert_eq!(w.estate.stats.foreclosures_total, 1);
}

// ── 3. construction in working hours; demolition says why ────────────────

#[test]
fn sites_only_work_the_working_day_and_step_at_their_own_minute() {
    let mut w = town();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(rect(-300.0, 8.0, 600.0, 40.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.keys().collect();
    let mut sites = Vec::new();
    for l in lots {
        w.build_on(l);
        if let Some(b) = w.lots[l].building {
            let c = w.buildings[b].construction.as_mut().unwrap();
            c.deliveries = Construction::DELIVERIES;
            sites.push(b);
        }
    }
    assert!(sites.len() >= 6);
    // Night: nothing moves.
    w.clock = crate::clock::SimClock::starting_at(2, 20, 0);
    let before: Vec<f32> = sites.iter().map(|b| w.buildings[*b].construction.as_ref().unwrap().progress).collect();
    run(&mut w, 10 * 60);
    let night: Vec<f32> = sites.iter().map(|b| w.buildings[*b].construction.as_ref().map_or(1.0, |c| c.progress)).collect();
    assert_eq!(before, night, "no building work from 20:00 to 06:00");
    // Day: every site advances, at different minutes of the hour.
    w.clock = crate::clock::SimClock::starting_at(3, 9, 0);
    let mut first_step: Vec<Option<u64>> = vec![None; sites.len()];
    for _ in 0..60 {
        run(&mut w, 1);
        for (i, b) in sites.iter().enumerate() {
            let p = w.buildings[*b].construction.as_ref().map_or(1.0, |c| c.progress);
            if first_step[i].is_none() && p > before[i] {
                first_step[i] = Some(w.clock.minute());
            }
        }
    }
    assert!(first_step.iter().all(|s| s.is_some()), "every site worked in the hour: {first_step:?}");
    let mut minutes: Vec<u64> = first_step.iter().flatten().copied().collect();
    minutes.sort();
    minutes.dedup();
    assert!(minutes.len() >= 2, "not one city-wide jump: {minutes:?}");
    // The calendar time stays about what it was: a day's work in the day.
    let c = Construction::new(2.0);
    assert!((c.work_hour_step() * crate::clock::WORK_HOURS_PER_DAY * 2.0 - 1.0).abs() < 1e-5);
}

#[test]
fn a_site_says_when_it_will_be_done() {
    let clock = crate::clock::SimClock::starting_at(4, 15, 0);
    // Two working hours from 15:00 is 17:00; three is 09:00 the next day.
    assert_eq!(clock.after_work_hours(2.0), crate::clock::SimClock::starting_at(4, 17, 0).tick);
    assert_eq!(clock.after_work_hours(3.0), crate::clock::SimClock::starting_at(5, 9, 0).tick);
    let night = crate::clock::SimClock::starting_at(4, 22, 0);
    assert_eq!(night.after_work_hours(1.0), crate::clock::SimClock::starting_at(5, 9, 0).tick);
}

#[test]
fn every_demolition_says_why() {
    let (mut w, homes) = bungalow_street();
    // A shop shut a week is taken over; a developer's rebuild; a city take-over.
    let at = w.building_lot(homes[0]).unwrap().center;
    press(&mut w, at, 80.0);
    let mut rng = crate::rng_for(9, 9);
    let a = homes[0];
    assert!(w.buy_out_and_rebuild(a, &mut rng));
    assert_eq!(w.buildings[a].demolition.as_ref().unwrap().reason, DemolitionReason::Redevelopment);
    let c = homes[homes.len() - 1];
    let lot = w.buildings[c].lot;
    w.take_over_lot(lot, "bus depot").unwrap();
    if let Some(d) = w.buildings.get(c).and_then(|b| b.demolition.as_ref()) {
        assert_eq!(d.reason, DemolitionReason::CityTakeover);
    }
    // Labels read as the player expects.
    assert_eq!(DemolitionReason::Redevelopment.label(), "new development planned");
    assert!(DemolitionReason::Abandoned.label().contains("new owner"));
    assert_eq!(DemolitionReason::RoadWorks.label(), "road works");
    // An old save's demolition has no reason and loads.
    let d: crate::buildings::Demolition = ron::from_str("(progress: 0.5, days: 1.0, floors: 2, tier: Low, land_use: Residential, seed: 1)").unwrap();
    assert_eq!(d.reason, DemolitionReason::Unknown);
}

// ── 5. what the inspector says matches what is drawn ──────────────────────

#[test]
fn civic_buildings_stay_within_what_their_styles_draw() {
    let mut w = town();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(rect(-300.0, 8.0, 600.0, 60.0), LandUse::Civic);
    w.land_value.fill(0.95);
    let lots: Vec<LotId> = w.lots.keys().collect();
    let mut n = 0;
    for l in lots {
        for _ in 0..3 {
            let f = w.pick_floors(l, LandUse::Civic, Tier::High);
            assert!((1..=5).contains(&f), "a civic hall of {f} floors");
            n += 1;
        }
    }
    assert!(n > 0);
}

#[test]
fn growth_state_survives_save_and_load_and_old_saves_load() {
    let (mut w, homes) = bungalow_street();
    let bid = homes[homes.len() / 2];
    let at = w.building_lot(bid).unwrap().center;
    press(&mut w, at, 80.0);
    w.estate.pressure.foot = vec![1.5; w.estate.pressure.value.len()];
    let mut rng = crate::rng_for(3, 4);
    assert!(w.assemble_plots(bid, &mut rng));
    w.update_land_value();
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.estate.pressure.foot, w.estate.pressure.foot);
    assert_eq!(back.estate.stats.assemblies_total, 1);
    assert_eq!(back.buildings[bid].demolition.as_ref().map(|d| d.reason), Some(DemolitionReason::Assembly));
    assert_eq!(back.land_value.raw.len(), w.land_value.raw.len());
    let p = w.building_lot(bid).unwrap().center;
    assert_eq!(back.land_value.for_use(p, LandUse::Residential), w.land_value.for_use(p, LandUse::Residential));
}
