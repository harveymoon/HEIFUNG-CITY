//! [r5-trash] Where the trash goes: the trucks tip at the sites, the sites
//! fill, burn, sort and ship, the depots' yards go out by lorry, the books.

use super::*;
use crate::economy::Item;
use crate::mind::IdleMind;
use crate::services::{Placeable, ServiceKind};
use crate::waste::*;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// The society tests' town: an avenue with civic land to the north and
/// housing to the south, on flat ground.
fn town(seed: u64) -> World {
    let mut w = World::new(WorldConfig { seed, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
    for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
        w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
    }
    w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
    w.add_zone(square(-690.0, -60.0, 1380.0, 52.0), LandUse::Residential);
    w
}

fn populate(w: &mut World, n: usize) {
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    for l in lots {
        w.build_on(l);
        if let Some(b) = w.lots[l].building {
            w.finish_construction(b);
        }
    }
    for _ in 0..200 {
        if w.citizens.len() >= n {
            break;
        }
        w.immigrate();
    }
}

fn adults(w: &World) -> Vec<CitizenId> {
    w.citizens.iter().filter(|(_, c)| c.can_work()).map(|(id, _)| id).collect()
}

fn staff(w: &mut World, b: BuildingId, who: &[CitizenId]) {
    for &c in who {
        if let Some(old) = w.citizens[c].work {
            w.buildings[old].workers.retain(|x| *x != c);
        }
        w.citizens[c].work = Some(b);
        w.buildings[b].workers.push(c);
    }
}

/// Put a city service down near `x` on the civic strip, finished.
fn place(w: &mut World, kind: ServiceKind, tier: Tier, x: f32) -> BuildingId {
    let what = Placeable::Service(kind, tier);
    let lot = w
        .lots
        .iter()
        .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access())
        .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
        .map(|(id, _)| id)
        .expect("a civic lot");
    let at = w.lots[lot].center;
    let id = w.place_building(what, at).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
    if w.buildings[id].demolition.is_some() {
        w.finish_demolition(id);
    }
    if w.buildings[id].construction.is_some() {
        w.finish_construction(id);
    }
    id
}

fn at_hour(w: &mut World, hour: u64) {
    let day = w.clock.day() + 1;
    w.clock.tick = day * crate::clock::MINUTES_PER_DAY + hour * 60;
}

/// A town with a staffed middle depot in the middle and a week of trash
/// in every bin.
fn town_with_depot(seed: u64) -> (World, BuildingId, Vec<CitizenId>) {
    let mut w = town(seed);
    populate(&mut w, 140);
    let crew = adults(&w);
    let depot = place(&mut w, ServiceKind::TrashDepot, Tier::Mid, 0.0);
    staff(&mut w, depot, &crew[..8]);
    w.hourly_trash();
    for b in w.society.trash.bins.values_mut() {
        b.stored = b.daily.max(10.0) * 7.0;
    }
    (w, depot, crew[8..].to_vec())
}

#[test]
fn the_trucks_tip_at_a_landfill_which_fills_and_books_its_cost() {
    let (mut w, _depot, crew) = town_with_depot(31);
    let tip = place(&mut w, ServiceKind::Landfill, Tier::Low, 450.0);
    staff(&mut w, tip, &crew[..3]);
    at_hour(&mut w, 7);
    let mut minds = IdleMind;
    let mut tipping = false;
    // The shift and the drive home after it.
    for _ in 0..12 * 60 + 30 {
        w.tick(&mut minds);
        tipping |= w.society.trash.trucks.values().any(|t| t.dump == Some(tip));
    }
    assert!(tipping, "a truck drove to the landfill");
    let d = &w.society.trash.disposal;
    let filled = d.sites.get(&tip).map_or(0.0, |s| s.filled_t);
    assert!(filled > 0.5, "the landfill took {filled:.2} t");
    assert!(d.t_today[Dest::Landfill.index()] > 0.5);
    assert!(w.landfill_fill(tip).unwrap() > 0.0);
    // Booked at midnight under waste disposal, itemised by the site.
    for _ in 0..6 * 60 {
        w.tick(&mut minds);
    }
    let paid = w.economy.ledger.yesterday_of(Flow::WasteDisposal);
    assert!(paid < 0.0, "the tipping was paid for: {paid}");
    assert!(w.economy.ledger.items_yesterday.iter().any(|e| e.flow == Flow::WasteDisposal && e.item == Item::Building(tip)));
    // The tip is a dirty neighbour now.
    w.refresh_dirty();
    assert!(w.dirty.iter().any(|s| s.building == tip));
}

#[test]
fn without_a_site_the_depot_hauls_out_of_town_at_the_road_price() {
    let (mut w, depot, _) = town_with_depot(32);
    at_hour(&mut w, 7);
    let mut minds = IdleMind;
    let mut contractor = false;
    // The shift, the trucks home, and the contractor's next hour.
    for _ in 0..13 * 60 {
        w.tick(&mut minds);
        contractor |= w.society.trash.trucks.values().any(|t| t.hauler == crate::trash::Hauler::Contractor(depot));
    }
    let d = &w.society.trash.disposal;
    assert!(d.started && d.starter_on(w.clock.day()), "the starter contract runs");
    assert!(d.t_today[Dest::Road.index()] > 0.5, "out by lorry: {:.2} t", d.t_today[Dest::Road.index()]);
    assert!(contractor, "the contractor's lorry left for the edge of town");
    // The starter price is half the road price; after it, the full price.
    assert_eq!(w.road_price(), COST_ROAD * STARTER_DISCOUNT);
    let per_t = d.cost_today[Dest::Road.index()] / d.t_today[Dest::Road.index()];
    assert!((per_t - COST_ROAD * STARTER_DISCOUNT).abs() < 0.01, "{per_t}");
    w.society.trash.disposal.starter_until = 0;
    assert_eq!(w.road_price(), COST_ROAD);
    assert!(COST_ROAD > COST_LANDFILL * 4.0 && COST_ROAD > COST_SHIP * 2.0, "lorries out of town are the dearest");
}

#[test]
fn a_full_yard_turns_the_trucks_away() {
    let (mut w, depot, _) = town_with_depot(33);
    w.hourly_trash();
    let d = &mut w.society.trash.disposal;
    d.starter_until = 0;
    d.yards.insert(depot, YARD_T[Tier::Mid.index()]);
    assert_eq!(w.tip_at_depot(depot, 4_000.0), 0.0, "no room in the yard");
    assert_eq!(w.society.trash.disposal.blocked_today, 1);
    // A broke city's contractor takes half.
    w.economy.treasury = -10.0;
    at_hour(&mut w, 10);
    w.hourly_waste();
    let left = w.society.trash.disposal.yards[&depot];
    let hour = HAUL_T_DAY[Tier::Mid.index()] / (HAUL_HOURS.1 - HAUL_HOURS.0) as f32;
    assert!((YARD_T[Tier::Mid.index()] - left - hour * 0.5).abs() < 0.01, "hauled {}", YARD_T[Tier::Mid.index()] - left);
}

#[test]
fn an_incinerator_burns_for_power_and_its_ash_fills_the_landfill() {
    let (mut w, _, crew) = town_with_depot(34);
    let tip = place(&mut w, ServiceKind::Landfill, Tier::Low, -450.0);
    let burner = place(&mut w, ServiceKind::Incinerator, Tier::Mid, 450.0);
    staff(&mut w, tip, &crew[..3]);
    let jobs = w.buildings[burner].jobs as usize;
    staff(&mut w, burner, &crew[3..3 + jobs]);
    at_hour(&mut w, 3);
    w.hourly_waste();
    assert!(w.tip_at_site(burner, 30_000.0) > 29_000.0, "the furnace takes a load");
    let before = w.contamination.within(w.lots[w.buildings[burner].lot].center, 600.0);
    w.hourly_waste();
    let s = &w.society.trash.disposal.sites[&burner];
    let per_hour = BURN_T_DAY[Tier::Mid.index()] / 24.0;
    assert!((s.done_today_t - per_hour).abs() < 0.01, "burnt {}", s.done_today_t);
    assert!((w.waste_mw() - per_hour * MWH_PER_T).abs() < 0.01, "on the grid: {} MW", w.waste_mw());
    let ash = w.society.trash.disposal.sites[&tip].filled_t;
    assert!((ash - per_hour * ASH_SHARE).abs() < 0.01, "ash to the tip: {ash}");
    assert!(w.contamination.within(w.lots[w.buildings[burner].lot].center, 600.0) > before, "fallout downwind");
    // Its output is counted with the plants'.
    w.refresh_power();
    assert!(w.economy.power.supply_mw >= w.waste_mw() - 0.001);
}

#[test]
fn recycling_sells_what_it_recovers_and_sends_the_rest_on() {
    let (mut w, _, crew) = town_with_depot(35);
    let plant = place(&mut w, ServiceKind::RecyclingPlant, Tier::Low, 450.0);
    let jobs = w.buildings[plant].jobs as usize;
    staff(&mut w, plant, &crew[..jobs]);
    at_hour(&mut w, 10);
    w.hourly_waste();
    assert!(w.tip_at_site(plant, 8_000.0) > 7_900.0);
    let till = w.economy.treasury;
    w.hourly_waste();
    let d = &w.society.trash.disposal;
    let s = &d.sites[&plant];
    assert!(s.done_today_t > 0.0 && s.recovered_today_t > 0.0);
    let share = s.recovered_today_t / s.done_today_t;
    assert!((share - (METAL_SHARE + PLASTIC_SHARE + PAPER_GLASS_SHARE)).abs() < 0.01, "{share}");
    assert!(d.sales_today > 0.0 && w.economy.ledger.today_of(Flow::RecyclingSales) > 0.0);
    assert!(w.economy.treasury > till, "sales come in at once");
    // No landfill or incinerator: the residue goes out by lorry.
    let residue = s.done_today_t - s.recovered_today_t;
    assert!((d.t_today[Dest::Road.index()] - residue).abs() < 0.01);
}

/// [r6-perfect-city] What a recycling plant cannot recover is burnt before
/// it is tipped: with an incinerator and a landfill in town the residue goes
/// to the furnace, and only its ash to the tip.
#[test]
fn recycling_residue_is_burnt_before_it_is_tipped() {
    let (mut w, _, crew) = town_with_depot(36);
    let tip = place(&mut w, ServiceKind::Landfill, Tier::Low, -450.0);
    let burner = place(&mut w, ServiceKind::Incinerator, Tier::Mid, 450.0);
    let plant = place(&mut w, ServiceKind::RecyclingPlant, Tier::Low, -150.0);
    for (k, b) in [plant, burner, tip].into_iter().enumerate() {
        let jobs = (w.buildings[b].jobs as usize).min(6);
        staff(&mut w, b, &crew[k * 6..k * 6 + jobs]);
    }
    at_hour(&mut w, 10);
    w.hourly_waste();
    assert!(w.tip_at_site(plant, 8_000.0) > 7_900.0);
    w.hourly_waste();
    let d = &w.society.trash.disposal;
    let s = &d.sites[&plant];
    let residue = s.done_today_t - s.recovered_today_t;
    assert!(residue > 0.0);
    assert!(d.sites.get(&burner).is_some_and(|b| b.in_today_t >= residue - 0.01), "the residue went to the incinerator: {:?}", d.sites.get(&burner).map(|b| b.in_today_t));
    assert!(d.sites.get(&tip).map_or(0.0, |t| t.in_today_t) < residue * 0.5, "not to the tip");
    assert!(d.t_today[Dest::Road.index()] < 0.01, "nothing on the lorries");
}

#[test]
fn a_transfer_station_ships_by_road_without_a_freight_yard() {
    let (mut w, _, crew) = town_with_depot(36);
    let shed = place(&mut w, ServiceKind::TransferStation, Tier::Low, 450.0);
    let jobs = w.buildings[shed].jobs as usize;
    staff(&mut w, shed, &crew[..jobs]);
    at_hour(&mut w, 9);
    w.hourly_waste();
    w.tip_at_site(shed, 20_000.0);
    assert!(!w.waste_ships_by_rail());
    w.hourly_waste();
    let d = &w.society.trash.disposal;
    assert!(d.t_today[Dest::Road.index()] > 0.0 && d.t_today[Dest::Shipped.index()] == 0.0);
    let per_t = d.cost_today[Dest::Road.index()] / d.t_today[Dest::Road.index()];
    assert!((per_t - COST_TRANSFER_ROAD).abs() < 0.01);
}

#[test]
fn a_full_landfill_turns_trucks_away_to_the_next_best() {
    let (mut w, depot, crew) = town_with_depot(37);
    let tip = place(&mut w, ServiceKind::Landfill, Tier::Low, 450.0);
    staff(&mut w, tip, &crew[..3]);
    w.hourly_waste();
    let at = w.lots[w.buildings[depot].lot].center;
    assert_eq!(w.dump_for(depot, at, 5_000.0), Some(tip));
    w.society.trash.disposal.sites.get_mut(&tip).unwrap().filled_t = LANDFILL_T[0];
    assert_eq!(w.dump_for(depot, at, 5_000.0), None, "full: back to the yard");
    assert_eq!(w.tip_at_site(tip, 5_000.0), 0.0);
}

#[test]
fn an_old_save_starts_on_the_starter_contract_and_keeps_its_state() {
    let (mut w, depot, _) = town_with_depot(38);
    // An old save: nothing of the disposal in it.
    w.society.trash.disposal = Disposal::default();
    at_hour(&mut w, 8);
    let day = w.clock.day();
    w.hourly_trash();
    let d = &w.society.trash.disposal;
    assert!(d.started && d.starter_until == day + STARTER_DAYS);
    w.society.trash.disposal.yards.insert(depot, 12.5);
    w.society.trash.disposal.t_total[Dest::Road.index()] = 3.0;
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.society.trash.disposal, w.society.trash.disposal);
}

#[test]
fn overflowing_bins_litter_their_street_and_sweepers_clear_it() {
    let mut w = town(39);
    populate(&mut w, 80);
    w.hourly_trash();
    for b in w.society.trash.bins.values_mut() {
        b.stored = b.capacity * 2.5;
    }
    w.daily_litter();
    let dirty = w.society.trash.litter.values().copied().fold(0.0f32, f32::max);
    assert!(dirty > 0.2, "litter on the streets: {dirty}");
    for b in w.society.trash.bins.values_mut() {
        b.stored = 0.0;
    }
    let before: f32 = w.society.trash.litter.values().sum();
    w.daily_litter();
    let after: f32 = w.society.trash.litter.values().sum();
    assert!(after < before * (LITTER_KEPT + 0.01), "it fades: {before} to {after}");
}

#[test]
fn disposal_is_deterministic() {
    let run = || {
        let (mut w, _, crew) = town_with_depot(40);
        let tip = place(&mut w, ServiceKind::Landfill, Tier::Low, 450.0);
        staff(&mut w, tip, &crew[..3]);
        at_hour(&mut w, 7);
        let mut minds = IdleMind;
        for _ in 0..20 * 60 {
            w.tick(&mut minds);
        }
        let d = &w.society.trash.disposal;
        (d.t_total.map(|x| (x * 1000.0) as i64), d.held_t() as i64, d.sites.get(&tip).map(|s| (s.filled_t * 1000.0) as i64))
    };
    assert_eq!(run(), run());
}
