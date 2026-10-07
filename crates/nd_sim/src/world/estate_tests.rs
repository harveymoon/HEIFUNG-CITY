//! [land] Tests of the land market: pressure and density, buyouts, merged
//! plots, ownership, billionaires and their convoys, determinism and old
//! saves.

use super::*;
use crate::estate::*;
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

fn lots_of(w: &World, lu: LandUse, in_x: std::ops::Range<f32>) -> Vec<LotId> {
    w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.car_access() && in_x.contains(&l.center.x)).map(|(id, _)| id).collect()
}

/// Pressure of `value` in every cell within 100 m of each point, nothing
/// elsewhere, as if sampled for a couple of days.
fn press(w: &mut World, busy: &[Vec2], value: f32) {
    let p = &mut w.estate.pressure;
    let mut present = vec![0.0f32; p.value.len()];
    for (i, v) in present.iter_mut().enumerate() {
        let c = p.cell_center(i);
        if busy.iter().any(|b| b.distance(c) < 100.0) {
            *v = value;
        }
    }
    let zeros = vec![0.0f32; present.len()];
    p.samples = 0;
    p.sample(&present, &zeros);
    p.samples = 48;
}

/// Low homes the length of a road: a busy block at the west end, a quiet
/// one at the east end, every home days old.
fn two_blocks() -> (World, Vec<BuildingId>, Vec<BuildingId>) {
    let mut w = town();
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(rect(-600.0, 8.0, 160.0, 60.0), LandUse::Residential);
    w.add_zone(rect(440.0, 8.0, 160.0, 60.0), LandUse::Residential);
    let mut busy = Vec::new();
    let mut quiet = Vec::new();
    for lot in lots_of(&w, LandUse::Residential, -700.0..0.0) {
        busy.extend(built(&mut w, lot));
    }
    for lot in lots_of(&w, LandUse::Residential, 0.0..700.0) {
        quiet.extend(built(&mut w, lot));
    }
    for &b in busy.iter().chain(&quiet) {
        let bld = &mut w.buildings[b];
        bld.floors = 2;
        bld.tier = Tier::Low;
        bld.built_tick = 0;
        bld.recompute_capacity();
    }
    w.clock = crate::clock::SimClock::starting_at(5, 1, 0);
    w.economy.demand[LandUse::Residential.index()] = 0.8;
    (w, busy, quiet)
}

#[test]
fn pressure_densifies_a_busy_block_and_leaves_a_quiet_one_low() {
    let (mut w, busy, quiet) = two_blocks();
    assert!(busy.len() >= 4 && quiet.len() >= 4, "two blocks of homes: {} and {}", busy.len(), quiet.len());
    let busy_at = w.building_lot(busy[0]).unwrap().center;
    press(&mut w, &[busy_at, busy_at + Vec2::new(80.0, 0.0)], 60.0);
    let (bl, ql) = (w.buildings[busy[0]].lot, w.buildings[quiet[0]].lot);
    assert!(w.pressure_floors(bl) >= 10, "a busy block justifies height: {}", w.pressure_floors(bl));
    assert_eq!(w.pressure_floors(ql), 2, "a quiet one does not");
    assert_eq!(w.height_cap(ql), 2, "however much is built round it");
    assert!(w.height_cap(bl) > 4, "the busy block may rise over its neighbours: {}", w.height_cap(bl));
    // Mobility counts: the same pressure with the city moving people badly
    // justifies less.
    let i = w.estate.pressure.index(busy_at).unwrap();
    let before = w.estate.pressure.floors_at(busy_at);
    w.estate.pressure.mobility[i] = 0.5;
    assert!(w.estate.pressure.floors_at(busy_at) < before, "height follows how well the city moves people");
    w.estate.pressure.mobility[i] = 1.0;
    // Developers buy the busy block out over a few days, never the quiet one.
    for _ in 0..12 {
        w.densify();
        w.clock.tick += 1440;
    }
    let bought: Vec<BuildingId> = busy.iter().copied().filter(|b| w.buildings.get(*b).is_some_and(|x| x.plot.owner.is_some())).collect();
    assert!(!bought.is_empty(), "the busy block was bought out");
    for b in &bought {
        let x = &w.buildings[*b];
        assert!(x.demolition.is_some() && x.construction.is_some(), "bought, coming down, to be rebuilt");
        assert!(x.floors > 2, "rebuilt bigger: {}", x.floors);
        assert!(matches!(w.owner_of(*b).map(|o| &o.1.kind), Some(OwnerKind::Developer)));
    }
    for q in &quiet {
        let x = &w.buildings[*q];
        assert!(x.plot.owner.is_none() && x.demolition.is_none() && x.floors == 2, "the quiet block stays as it is");
    }
    assert_eq!(w.estate.stats.buyouts_total as usize, bought.len());
}

#[test]
fn a_buyout_pulls_the_building_down_and_builds_bigger() {
    let (mut w, busy, _) = two_blocks();
    let at = w.building_lot(busy[0]).unwrap().center;
    press(&mut w, &[at, at + Vec2::new(80.0, 0.0)], 60.0);
    // People live there.
    run(&mut w, 600);
    let bid = *busy.iter().max_by_key(|b| w.buildings[**b].households.len()).unwrap();
    let residents = w.buildings[bid].residents.clone();
    let (old_floors, old_homes) = (w.buildings[bid].floors, w.buildings[bid].homes);
    let price = w.sale_price(bid);
    assert!(price > 0.0, "every owner has a price");
    let mut rng = crate::rng_for(1, 2);
    assert!(w.buy_out_and_rebuild(bid, &mut rng), "bought");
    let b = &w.buildings[bid];
    assert!(b.residents.is_empty() && b.households.is_empty(), "the old residents move out at once");
    assert!(b.demolition.is_some(), "the existing demolition sequence");
    for c in &residents {
        if let Some(c) = w.citizens.get(*c) {
            assert!(c.home != Some(bid));
            assert!(c.memories.iter().any(|m| m.text.contains("was sold to")), "they remember why");
        }
    }
    // The wrecking crew works through the days; then the site is built.
    run(&mut w, 3 * 1440);
    assert!(w.buildings[bid].demolition.is_none(), "the old building is down");
    w.finish_construction(bid);
    let b = &w.buildings[bid];
    assert!(b.construction.is_none() && b.floors > old_floors && b.homes > old_homes, "something bigger: {} floors, {} homes (was {old_floors}, {old_homes})", b.floors, b.homes);
}

/// An industrial firm on a plot in a row of works plots.
fn works() -> (World, BuildingId, Vec<LotId>) {
    let mut w = town();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(rect(-150.0, 8.0, 300.0, 70.0), LandUse::Industrial);
    let mut lots = lots_of(&w, LandUse::Industrial, -150.0..150.0);
    lots.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
    assert!(lots.len() >= 3, "a row of works plots");
    let firm = built(&mut w, lots[1]).expect("the works");
    let jobs = w.buildings[firm].jobs;
    w.buildings[firm].business = Some(Business::new(BusinessKind::Fab, jobs));
    (w, firm, lots)
}

#[test]
fn a_firm_buys_the_plot_next_door_and_rebuilds_across_both() {
    let (mut w, firm, lots) = works();
    let mine = w.buildings[firm].lot;
    let neighbours: Vec<LotId> = lots.iter().copied().filter(|l| *l != mine && lots_touch(&w.lots[mine], &w.lots[*l])).collect();
    assert!(!neighbours.is_empty(), "plots touch");
    let jobs_before = w.buildings[firm].jobs;
    let area: f32 = w.lots[mine].area();
    assert!(w.expand_into_neighbours(firm), "it buys");
    let b = &w.buildings[firm];
    assert_eq!(b.plot.form, Some(Form::Merged));
    let all = b.plot.lots(b.lot);
    assert!(all.len() >= 2, "the plot next door is part of it now");
    for l in &all {
        assert_eq!(w.lots[*l].building, Some(firm), "plots stay plots for zoning; the building spans them");
        assert_eq!(w.lots[*l].land_use, LandUse::Industrial);
    }
    let fp = w.footprint(firm).unwrap();
    assert!(fp.area() > area * 1.6, "one footprint over both plots: {} vs {}", fp.area(), area);
    assert!(b.demolition.is_some() && b.construction.is_some(), "rebuilt through the usual sequence");
    w.finish_construction(firm);
    let b = &w.buildings[firm];
    assert!(b.jobs + 1 >= jobs_before * 2, "a bigger works: {} jobs (was {jobs_before})", b.jobs);
    assert_eq!(b.business.as_ref().map(|z| z.kind), Some(BusinessKind::Fab), "the same firm");
    assert!(w.owner_of(firm).is_some_and(|(_, o)| matches!(o.kind, OwnerKind::Firm { .. })));
    // Nothing else can be built on the merged plots.
    for l in &all {
        w.build_on(*l);
        assert_eq!(w.lots[*l].building, Some(firm));
    }
    // Pulled down, every plot is free again.
    w.demolish(firm);
    w.finish_demolition(firm);
    for l in &all {
        assert!(w.lots[*l].building.is_none(), "the plots are free again");
    }
}

#[test]
fn a_built_neighbour_comes_down_first_then_joins() {
    let (mut w, firm, lots) = works();
    let mine = w.buildings[firm].lot;
    let neighbours: Vec<LotId> = lots.iter().copied().filter(|l| *l != mine && lots_touch(&w.lots[mine], &w.lots[*l])).collect();
    let mut theirs = Vec::new();
    for l in &neighbours {
        if let Some(b) = built(&mut w, *l) {
            w.buildings[b].floors = 1;
            w.buildings[b].recompute_capacity();
            theirs.push(b);
        }
    }
    assert!(!theirs.is_empty());
    // Empty backland (even landlocked) would go first, at no cost: promise
    // it elsewhere so the built neighbour is the one bought.
    let empty: Vec<LotId> = w.lots.iter().filter(|(id, l)| *id != mine && l.building.is_none() && lots_touch(&w.lots[mine], l)).map(|(id, _)| id).collect();
    for l in empty {
        w.estate.reserved.insert(l, BuildingId::default());
    }
    assert!(w.expand_into_neighbours(firm));
    let pending = w.buildings[firm].plot.pending_lots.clone();
    assert_eq!(pending.len(), 1, "one plot bought, its building coming down");
    let other = w.lots[pending[0]].building.unwrap();
    assert!(w.buildings[other].being_demolished(), "the neighbour is a demolition site");
    w.finish_demolition(other);
    let b = &w.buildings[firm];
    assert!(b.plot.pending_lots.is_empty() && b.plot.extra_lots.contains(&pending[0]), "cleared, it joins");
    assert_eq!(w.lots[pending[0]].building, Some(firm));
}

#[test]
fn branch_firms_scatter_across_the_city() {
    let mut w = town();
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 1, 1);
    w.add_zone(rect(-850.0, 8.0, 120.0, 60.0), LandUse::Commercial);
    w.add_zone(rect(-60.0, 8.0, 120.0, 60.0), LandUse::Commercial);
    w.add_zone(rect(730.0, 8.0, 120.0, 60.0), LandUse::Commercial);
    let first = lots_of(&w, LandUse::Commercial, -900.0..-700.0)[0];
    let shop = built(&mut w, first).unwrap();
    let jobs = w.buildings[shop].jobs;
    w.buildings[shop].business = Some(Business::new(BusinessKind::Bakery, jobs));
    let b1 = w.open_branch(shop).expect("a branch");
    let p0 = w.building_lot(shop).unwrap().center;
    let p1 = w.building_lot(b1).unwrap().center;
    assert!(p0.distance(p1) >= 450.0, "branches scatter: {}", p0.distance(p1));
    assert_eq!(w.buildings[b1].business.as_ref().map(|z| z.kind), Some(BusinessKind::Bakery));
    let owned = w.owned_with(shop);
    assert!(owned.contains(&b1) && owned.contains(&shop), "one owner holds both");
    assert_eq!(w.owned_with(b1).len(), owned.len(), "selecting either highlights both");
}

/// A town of `n` grown-ups with the given incomes (cloned from whoever
/// moved in first), quiet residential land in the east, works in the west.
fn rich_poor(n: usize, income: impl Fn(usize) -> f32) -> (World, BuildingId) {
    let mut w = town();
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 1, 1);
    w.add_zone(rect(-850.0, 8.0, 140.0, 70.0), LandUse::Industrial);
    w.add_zone(rect(-500.0, 8.0, 200.0, 70.0), LandUse::Residential);
    w.add_zone(rect(500.0, 8.0, 300.0, 70.0), LandUse::Residential);
    let site = lots_of(&w, LandUse::Industrial, -900.0..-600.0)[0];
    let works = built(&mut w, site).unwrap();
    for lot in lots_of(&w, LandUse::Residential, -600.0..0.0) {
        built(&mut w, lot);
    }
    run(&mut w, 900);
    let template = w.citizens.values().find(|c| c.can_work()).cloned().expect("someone moved in");
    let mut k = 0;
    while w.citizens.len() < n {
        let mut c = template.clone();
        c.seed = 10_000 + k as u64;
        c.household = None;
        c.home = None;
        c.work = None;
        w.citizens.insert(c);
        k += 1;
    }
    let ids: Vec<CitizenId> = w.citizens.keys().collect();
    for (i, id) in ids.into_iter().enumerate() {
        w.citizens[id].income = income(i);
    }
    w.update_economy();
    (w, works)
}

#[test]
fn billionaires_need_extreme_disparity() {
    let (mut w, _) = rich_poor(1000, |_| 90.0);
    assert!(!w.elite_gate().3, "an equal town has none");
    for _ in 0..80 {
        w.daily_elites();
        w.clock.tick += 1440;
    }
    assert!(w.estate.elites.is_empty(), "never, however long");
    // Mild inequality is not enough either.
    let ids: Vec<CitizenId> = w.citizens.keys().collect();
    for (i, id) in ids.iter().enumerate() {
        w.citizens[*id].income = if i % 3 == 0 { 140.0 } else { 70.0 };
    }
    w.update_economy();
    assert!(!w.elite_gate().3, "Gini {:.2}", w.economy.gini);
}

#[test]
fn a_billionaire_buys_quiet_land_and_stays_within_limits() {
    let (mut w, works) = rich_poor(1000, |i| if i % 20 == 0 { 2500.0 } else if i % 6 == 0 { 160.0 } else { 45.0 });
    let (_, gini, ratio, open) = w.elite_gate();
    assert!(open, "extreme disparity opens the gate: Gini {gini:.2}, top/median {ratio:.1}");
    // Rare: not every day.
    let mut first_day = None;
    for d in 0..200 {
        w.daily_elites();
        w.clock.tick += 1440;
        if first_day.is_none() && !w.estate.elites.is_empty() {
            first_day = Some(d);
        }
    }
    let first_day = first_day.expect("in time one comes");
    assert!(first_day > 0, "not on the first day the gate opens");
    assert_eq!(w.elite_cap(), 1);
    assert_eq!(w.estate.elites.len(), 1, "never more than the town carries");
    let e = w.estate.elites[0].clone();
    assert!((2..=5).contains(&e.escorts));
    assert!(w.citizens.contains_key(e.citizen), "a real (fictional) person");
    assert_eq!(w.class_of(e.citizen), Class::Elite);
    let mansion = e.mansion.expect("a mansion site");
    let b = &w.buildings[mansion];
    assert_eq!(b.plot.form, Some(Form::Mansion));
    assert!(b.plot.lots(b.lot).len() + b.plot.pending_lots.len() >= 2, "connected plots");
    let at = w.footprint(mansion).unwrap().center;
    let loud = w.building_lot(works).unwrap().center;
    assert!(at.distance(loud) >= ELITE_NUISANCE_CLEARANCE, "away from the works");
    assert!(at.x > 300.0, "on the quiet land");
    assert!(w.estate.owners[e.owner].properties.len() >= 2, "several properties");
    // Built and moved in: the NIMBY pull starts, within strict limits.
    for l in w.buildings[mansion].plot.pending_lots.clone() {
        if let Some(o) = w.lots[l].building {
            w.finish_demolition(o);
        }
    }
    w.finish_construction(mansion);
    assert_eq!(w.citizens[e.citizen].home, Some(mansion), "the billionaire moved in");
    assert_eq!(w.buildings[mansion].rent, 0.0, "bought outright");
    assert_eq!(w.estate.nimby.len(), 1);
    let n = w.estate.nimby[0].clone();
    assert!(n.radius <= NIMBY_MAX_RADIUS);
    // Cheap homes round it: some inside the radius are priced out, none
    // outside, and never more than the cap.
    let mut near = Vec::new();
    let mut far = Vec::new();
    for lot in lots_of(&w, LandUse::Residential, 300.0..900.0) {
        if w.lots[lot].building.is_some() {
            continue;
        }
        if let Some(b) = built(&mut w, lot) {
            w.buildings[b].tier = Tier::Low;
            w.buildings[b].floors = 2;
            w.buildings[b].recompute_capacity();
            if w.lots[lot].center.distance(n.center) < n.radius {
                near.push(b)
            } else {
                far.push(b)
            }
        }
    }
    // Within the radius nothing rises tall.
    if let Some(&b) = near.first() {
        assert!(w.pressure_floors(w.buildings[b].lot) <= NIMBY_HEIGHT_CAP);
    }
    for _ in 0..90 {
        w.daily_nimby();
        w.clock.tick += 1440;
    }
    let priced = near.iter().filter(|b| w.buildings.get(**b).is_some_and(|x| x.tier == Tier::Mid)).count();
    assert!(priced as u8 <= NIMBY_PRICE_OUT_CAP, "capped: {priced}");
    assert!(near.is_empty() || priced > 0, "some cheap homes nearby were priced out");
    assert!(far.iter().all(|b| w.buildings[*b].tier == Tier::Low), "nothing outside the radius");
    assert!(w.estate.nimby.is_empty(), "and the pull fades away");
}

#[test]
fn no_quiet_land_no_mansion() {
    let (mut w, _) = rich_poor(1000, |i| if i % 20 == 0 { 2500.0 } else { 45.0 });
    let everywhere: Vec<Vec2> = (0..40).flat_map(|x| (0..40).map(move |y| Vec2::new(-1000.0 + x as f32 * 50.0, -1000.0 + y as f32 * 50.0))).collect();
    press(&mut w, &everywhere, 40.0);
    assert!(w.elite_site().is_none(), "busy everywhere: nowhere quiet enough");
    let mut rng = crate::rng_for(3, 4);
    assert!(w.spawn_elite(&mut rng).is_err());
}

#[test]
fn escorts_follow_the_billionaire() {
    let mut w = town();
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(rect(400.0, 8.0, 200.0, 60.0), LandUse::Residential);
    w.add_zone(rect(-600.0, 8.0, 120.0, 60.0), LandUse::Commercial);
    for lot in lots_of(&w, LandUse::Residential, 0.0..700.0) {
        built(&mut w, lot);
    }
    let site = lots_of(&w, LandUse::Commercial, -700.0..0.0)[0];
    let shop = built(&mut w, site).unwrap();
    run(&mut w, 600);
    let cid = w.citizens.iter().find(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id).expect("someone lives here");
    if let Some(v) = w.citizens[cid].car {
        w.drop_car(v, crate::vehicle::CarEvent::Sold);
    }
    let home = w.citizens[cid].home.unwrap();
    let owner = w.new_owner("Test Fortune".into(), OwnerKind::Elite { citizen: cid });
    w.estate.elites.push(Elite { citizen: cid, owner, net_worth: 30e9, origin: EliteOrigin::Local, mansion: None, cars: vec![supercar(1)], escorts: 3, police: true, since_day: 0 });
    let at_home = w.building_entrance(home).unwrap();
    let vid = w.give_car_parked(cid, at_home);
    w.vehicles.cars[vid].model = supercar(1);
    let c = &mut w.citizens[cid];
    c.activity = Activity::AtHome;
    c.inside = Some(home);
    c.pos = at_home;
    c.next_decision_tick = u64::MAX;
    w.travel_to(cid, Some(shop), Purpose::Shop(crate::market::Resource::Goods));
    let mut minds = IdleMind;
    let mut boss = None;
    for _ in 0..120 {
        w.tick(&mut minds);
        if let Some(conv) = w.estate.convoys.first() {
            boss = Some(conv.boss);
            break;
        }
    }
    let boss = boss.expect("a convoy forms when the billionaire drives");
    let conv = w.estate.convoys[0].clone();
    assert_eq!(conv.escorts.len(), 3, "three black cars");
    assert!(conv.police.is_some(), "the richest get a police car");
    for e in &conv.escorts {
        assert_eq!(w.traffic.cars[*e].model, escort_model(), "black SUVs");
        assert_eq!(w.traffic.cars[*e].route.last(), w.traffic.cars[boss].route.last(), "bound where the boss is");
    }
    // They keep with it on the way.
    let mut close = 0;
    let mut samples = 0;
    let start: Vec<Vec2> = conv.escorts.iter().map(|e| w.traffic.cars[*e].pos).collect();
    for _ in 0..60 {
        w.tick(&mut minds);
        let Some(b) = w.traffic.cars.get(boss) else { break };
        for e in &conv.escorts {
            if let Some(x) = w.traffic.cars.get(*e) {
                samples += 1;
                // Three cars at a city pace and a safe headway string out
                // over a hundred metres or so.
                if x.pos.distance(b.pos) < 170.0 {
                    close += 1;
                }
            }
        }
    }
    assert!(samples > 0 && close * 10 >= samples * 8, "escorts stay with the boss: {close} of {samples} samples within 170 m (gaps now {:?})", conv.escorts.iter().filter_map(|e| Some(w.traffic.cars.get(*e)?.pos.distance(w.traffic.cars.get(boss)?.pos))).collect::<Vec<_>>());
    let moved = conv.escorts.iter().zip(&start).filter(|(e, s)| w.traffic.cars.get(**e).is_none_or(|x| x.pos.distance(**s) > 20.0)).count();
    assert!(moved >= 2, "they follow, not wait");
    // At the end of the run they leave the road.
    for _ in 0..1440 {
        w.tick(&mut minds);
        if w.estate.convoys.iter().all(|c| c.escorts.is_empty() && c.police.is_none()) {
            break;
        }
    }
    assert!(conv.escorts.iter().all(|e| !w.traffic.cars.contains_key(*e)), "escorts gone with the run");
}

fn scenario() -> World {
    let (mut w, busy, _) = two_blocks();
    let at = w.building_lot(busy[0]).unwrap().center;
    press(&mut w, &[at], 60.0);
    run(&mut w, 2 * 1440);
    w
}

#[test]
fn the_land_market_is_deterministic_and_saved() {
    let a = scenario();
    let b = scenario();
    let key = |w: &World| -> Vec<(u8, Option<Form>, usize, bool)> { w.buildings.values().map(|x| (x.floors, x.plot.form, x.plot.extra_lots.len(), x.plot.owner.is_some())).collect() };
    assert_eq!(key(&a), key(&b), "same seed, same market");
    assert_eq!(a.estate.stats, b.estate.stats);
    assert_eq!(a.estate.pressure, b.estate.pressure);
    let back = World::from_ron(&a.to_ron().unwrap()).unwrap();
    assert_eq!(back.estate.pressure, a.estate.pressure, "the grid is saved");
    assert_eq!(back.estate.stats, a.estate.stats);
    assert_eq!(key(&back), key(&a));
    assert_eq!(back.estate.owners.len(), a.estate.owners.len());
}

/// A real save from before the land market (`saves/flags/flag_patch.ron`,
/// copied into the worktree; skipped when absent): it loads with an empty
/// market and a cold grid that warms in an hour, and runs through a day.
#[test]
fn an_old_save_loads_and_runs_with_the_land_market() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../saves/flags/flag_patch.ron");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no {}", path.display());
        return;
    };
    let mut w = World::from_ron(&text).expect("the old save loads");
    assert!(w.estate.pressure.ready());
    assert!(w.estate.owners.is_empty() && w.estate.elites.is_empty());
    assert!(w.buildings.values().all(|b| b.plot == crate::estate::Plot::default()));
    let mut minds = IdleMind;
    let start = w.clock.day();
    while w.clock.day() == start || w.clock.minute_of_day() < 30 {
        w.tick(&mut minds);
    }
    assert!(w.estate.pressure.samples >= 1, "the grid warmed");
    let text = w.to_ron().unwrap();
    let again = World::from_ron(&text).expect("and saves again");
    assert_eq!(again.estate.pressure.samples, w.estate.pressure.samples);
}
