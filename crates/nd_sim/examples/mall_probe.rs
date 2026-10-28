//! [land-assembly] [malls] Probe: load a save (or the demo district), run
//! on for some days and print once a game day the strip malls, malls and
//! mega-malls (their shop units let and to let, tenants), the joined sites
//! (strip mines, oil fields, farms, car parks and other industry spread
//! over the plots behind their road plot, the backland among them) and the
//! landlocked zoned lots, with unemployment and the ledger's sum. Used to
//! compare the work before and after on a copy of a real save. Not part of
//! the game.
//!
//! cargo run --profile fast -p nd_sim --example mall_probe -- <save.ron|demo> [days] [--force-mega] [--save out.ron]
//!
//! `--force-mega`: at the end, the best-let mall (one is made of the
//! biggest shop site if there is none) grows into a mega-mall, and the
//! world runs on until it opens (for screenshots). `--save`: writes the
//! world to `out.ron` (gzipped RON, as the game saves).

use nd_minds::RuleBasedMind;
use nd_sim::estate::Form;
use nd_sim::World;
use std::collections::BTreeMap;

fn report(w: &World, label: &str, secs: f64) {
    let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1);
    let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
    println!("{label}: day {} pop {} unemployment {:.1}% ledger_sum {} secs {:.1}", w.clock.day(), w.citizens.len(), idle as f32 / adults as f32 * 100.0, w.ledger_sum(), secs);
    let s = w.mall_stats();
    println!("   malls: strip {} mall {} mega {} (building {}) shop units {} let {} to let {} tenants {}", s.strip_malls, s.malls, s.mega_malls, s.building, s.units, s.let_units, s.vacant, s.tenants);
    let st = &w.estate.stats;
    println!("   estate: site plots bought {} (backland {}) malls rebuilt {} mega-malls {} merges {}", st.site_plots_total, st.backland_plots_total, st.malls_total, st.mega_malls_total, st.merges_total);
    // Joined sites by kind: (sites, plots, backland plots, biggest site m²).
    let mut sites: BTreeMap<String, (usize, usize, usize, f32)> = BTreeMap::new();
    for (id, b) in w.buildings.iter() {
        if b.plot.extra_lots.is_empty() || b.is_tenant() {
            continue;
        }
        let what = b.business.as_ref().map_or_else(|| format!("{:?}", b.land_use), |z| z.kind.label().to_string());
        let back = b.plot.extra_lots.iter().filter(|l| w.lots.get(**l).is_some_and(|x| x.landlocked)).count();
        let area = w.footprint(id).map_or(0.0, |l| l.area());
        let e = sites.entry(what).or_default();
        e.0 += 1;
        e.1 += 1 + b.plot.extra_lots.len();
        e.2 += back;
        e.3 = e.3.max(area);
    }
    println!("   joined sites (sites/plots/backland/biggest m²): {}", sites.iter().map(|(k, (n, p, bl, a))| format!("{k} {n}/{p}/{bl}/{a:.0}")).collect::<Vec<_>>().join(", "));
    let mines: Vec<String> = w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::StripMine)).map(|(id, b)| format!("{} {} plots {:.0} m² jobs {}", b.name, 1 + b.plot.extra_lots.len(), w.footprint(id).map_or(0.0, |l| l.area()), b.jobs)).collect();
    println!("   strip mines ({}): {}", mines.len(), mines.join("; "));
    let parks: Vec<String> = w.buildings.iter().filter(|(_, b)| b.is_parking() && !b.plot.extra_lots.is_empty()).map(|(_, b)| format!("{} {} plots {} bays", b.name, 1 + b.plot.extra_lots.len(), b.parking_capacity)).collect();
    println!("   grown car parks ({}): {}", parks.len(), parks.join("; "));
    let fresh = w.buildings.values().filter(|b| b.land_use == nd_sim::LandUse::Commercial && !b.is_tenant() && w.clock.tick.saturating_sub(b.built_tick) < 1440).count();
    println!("   commercial buildings begun or opened today {fresh}");
    let (all, wide, vacant, big, cands) = w.mall_site_diag();
    println!("   commercial road plots {all}: strip-mall size {wide} (vacant {vacant}), mall size {big}; old buildings that could be rebuilt as one {cands}");
    if std::env::var("SITE_DIAG").is_ok() {
        for l in w.site_growth_diag() {
            println!("   site: {l}");
        }
    }
    let locked = w.lots.values().filter(|l| l.landlocked && l.building.is_none() && l.land_use != nd_sim::LandUse::Park).count();
    println!("   landlocked empty lots {locked}");
    let mut malls: Vec<String> = Vec::new();
    for (id, b) in w.buildings.iter() {
        let Some(f) = b.plot.form.filter(|f| f.is_mall()) else { continue };
        if f == Form::StripMall && malls.len() > 40 {
            continue;
        }
        let shops = b.units.iter().filter(|u| u.shopfront).count();
        let vacant = b.units.iter().filter(|u| u.shopfront && u.vacant()).count();
        malls.push(format!("{} [{}{}] {} plots, {}/{} let, {} tenants{}", b.name, f.label(), if b.operating() { "" } else { ", building" }, 1 + b.plot.extra_lots.len(), shops - vacant, shops, w.tenants_of(id).len(), w.lots.get(b.lot).map_or(String::new(), |l| format!(" at ({:.0},{:.0})", l.center.x, l.center.y))));
    }
    if !malls.is_empty() {
        println!("   {}", malls.join("\n   "));
    }
}

/// [land-assembly] For screenshots: on the unzoned backland behind a road
/// plot of `lu` (the most of it found), zone it `lu`, open `kind` on the
/// road plot and let it buy and spread until it stops. Prints where.
fn scene_site(w: &mut World, lu: nd_sim::LandUse, kind: nd_sim::BusinessKind, money: f32) -> Option<nd_sim::BuildingId> {
    let mut best: Option<(usize, nd_sim::LotId, Vec<usize>)> = None;
    let mut lots: Vec<(nd_sim::LotId, glam::Vec2, glam::Vec2, bool)> = w.lots.iter().filter(|(_, l)| l.land_use == lu && l.car_access() && !l.landlocked).map(|(id, l)| (id, l.center, l.front_dir, l.building.is_some_and(|b| w.buildings.get(b).is_some_and(|x| x.is_special() || x.floors > 2)))).collect();
    lots.sort_by(|a, b| a.0.cmp(&b.0));
    for (lid, c, front, busy) in lots {
        if busy {
            continue;
        }
        let at = c - front * 45.0;
        let ids: Vec<usize> = w.parcels.in_circle(at, 40.0).into_iter().filter(|&i| {
            let p = &w.parcels.parcels[i];
            p.land_use.is_none() && !p.rough && !p.path_frontage
        }).collect();
        if best.as_ref().is_none_or(|b| ids.len() > b.0) {
            best = Some((ids.len(), lid, ids));
        }
    }
    let (n, lot, ids) = best?;
    println!("scene: {n} backland parcels behind a {lu:?} road plot");
    w.project = None;
    w.paint_parcels(&ids, Some(lu), None);
    if let Err(e) = w.try_implement() {
        println!("scene: zoning refused: {e}");
        return None;
    }
    // The plot itself first, then its neighbours, until one spreads.
    let at = w.lots[lot].center;
    let mut tries: Vec<(f32, nd_sim::LotId)> = w.lots.iter().filter(|(id, l)| l.land_use == lu && l.car_access() && !l.landlocked && (*id == lot || l.center.distance(at) < 90.0) && l.building.is_none_or(|b| w.buildings.get(b).is_some_and(|x| !x.is_special() && x.floors <= 2))).map(|(id, l)| (if id == lot { -1.0 } else { l.center.distance(at) }, id)).collect();
    tries.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut found = None;
    for (_, l) in tries.into_iter().take(8) {
        let Some(bid) = w.force_open_site(l, kind) else { continue };
        w.transfer(nd_sim::bank::Account::Outside, nd_sim::bank::Account::Business(bid), money, nd_sim::bank::Kind::Investment);
        for _ in 0..12 {
            if !w.grow_site_in_place(bid) {
                // Built plots bought come down first; clear them and go on.
                let pending = w.buildings[bid].plot.pending_lots.clone();
                if pending.is_empty() {
                    break;
                }
                for p in pending {
                    if let Some(o) = w.lots.get(p).and_then(|x| x.building) {
                        w.finish_demolition(o);
                    }
                }
            }
        }
        found = Some(bid);
        if !w.buildings[bid].plot.extra_lots.is_empty() {
            break;
        }
    }
    let bid = found?;
    let b = &w.buildings[bid];
    let at = w.lots[b.lot].center;
    println!("scene: {} [{}] {} plots, {:.0} m², at ({:.0},{:.0}) facing ({:.2},{:.2})", b.name, kind.label(), 1 + b.plot.extra_lots.len(), w.footprint(bid).map_or(0.0, |l| l.area()), at.x, at.y, w.lots[b.lot].front_dir.x, w.lots[b.lot].front_dir.y);
    Some(bid)
}

fn run_day(w: &mut World, minds: &mut RuleBasedMind) {
    loop {
        w.tick(minds);
        w.drain_events();
        if w.clock.minute_of_day() == 0 {
            break;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).map(String::as_str).unwrap_or("demo");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let force = args.iter().any(|a| a == "--force-mega");
    let out = args.iter().position(|a| a == "--save").and_then(|i| args.get(i + 1)).cloned();
    let mut w = if src == "demo" {
        let mut w = World::new(nd_sim::WorldConfig { seed: 1, ..nd_sim::WorldConfig::default() });
        w.drain_events();
        nd_sim::scenarios::demo_district(&mut w);
        w
    } else {
        World::read_save(src).expect("load")
    };
    let mut minds = RuleBasedMind::default();
    report(&w, "start", 0.0);
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        run_day(&mut w, &mut minds);
        report(&w, "day", t0.elapsed().as_secs_f64());
    }
    if args.iter().any(|a| a == "--scene") {
        scene_site(&mut w, nd_sim::LandUse::Industrial, nd_sim::BusinessKind::StripMine, 2_000_000.0);
        scene_site(&mut w, nd_sim::LandUse::Commercial, nd_sim::BusinessKind::Parking, 2_000_000.0);
        if w.mall_stats().malls == 0 {
            if let Some(m) = w.force_mall(None, true) {
                let at = w.lots[w.buildings[m].lot].center;
                println!("scene: mall {} at ({:.0},{:.0})", w.buildings[m].name, at.x, at.y);
            }
        }
        run_day(&mut w, &mut minds);
        report(&w, "scene", 0.0);
    }
    if force {
        match w.force_mega_mall() {
            Some(bid) => {
                let at = w.lots[w.buildings[bid].lot].center;
                println!("forced: {} grows into a mega-mall at ({:.0},{:.0})", w.buildings[bid].name, at.x, at.y);
                for _ in 0..12 {
                    if w.buildings.get(bid).is_some_and(|b| b.operating()) {
                        break;
                    }
                    run_day(&mut w, &mut minds);
                }
                if w.mall_stats().malls == 0 {
                    if let Some(m) = w.force_mall(Some(bid), false) {
                        let at = w.lots[w.buildings[m].lot].center;
                        println!("forced: mall {} at ({:.0},{:.0})", w.buildings[m].name, at.x, at.y);
                        run_day(&mut w, &mut minds);
                    }
                }
                for _ in 0..3 {
                    run_day(&mut w, &mut minds);
                }
                report(&w, "after the mega-mall", 0.0);
            }
            None => println!("forced: no mall could grow"),
        }
    }
    if args.iter().any(|a| a == "--scene") {
        // Strip malls still going up open now, for the camera.
        let sites: Vec<nd_sim::BuildingId> = w.buildings.iter().filter(|(_, b)| b.plot.form == Some(Form::StripMall) && b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
            let at = w.lots[w.buildings[id].lot].center;
            println!("scene: strip mall {} at ({:.0},{:.0}) facing ({:.2},{:.2})", w.buildings[id].name, at.x, at.y, w.lots[w.buildings[id].lot].front_dir.x, w.lots[w.buildings[id].lot].front_dir.y);
        }
        run_day(&mut w, &mut minds);
        report(&w, "scene end", 0.0);
    }
    if let Some(path) = out {
        let f = std::fs::File::create(&path).expect("create");
        w.write_ron_gz(std::io::BufWriter::new(f)).expect("save");
        println!("saved {path}");
    }
}
