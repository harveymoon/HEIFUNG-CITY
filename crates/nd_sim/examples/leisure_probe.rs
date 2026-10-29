//! [r4-leisure] Probe: load a save (a copy of a real one), report how its
//! old Entertainment land reads as commercial with the leisure sub-zone,
//! the clubs on it and the demand by sub-zone, then run on for some days
//! and report again (clubs open, people out at night, what grew on
//! commercial land by sub-zone). Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example leisure_probe -- <save.ron> [days]

use nd_minds::RuleBasedMind;
use nd_sim::commercial::CommercialSub;
use nd_sim::{Activity, BusinessKind, LandUse, Purpose, World};

fn report(w: &World, label: &str) {
    let lots = |lu: LandUse| w.lots.values().filter(|l| l.land_use == lu).count();
    let leisure_lots = w.lots.iter().filter(|(id, _)| w.commercial_sub_set(*id) == Some(CommercialSub::Leisure)).count();
    let zoned_commercial = w.lots.values().filter(|l| l.land_use.zoned_as() == LandUse::Commercial).count();
    let mut by_sub = [0usize; 4];
    let mut sites_by_sub = [0usize; 4];
    for (id, b) in w.buildings.iter() {
        if let Some(s) = w.commercial_sub_of_building(id) {
            if b.construction.is_some() {
                sites_by_sub[s.index()] += 1;
            } else {
                by_sub[s.index()] += 1;
            }
        }
    }
    let ent_buildings = w.buildings.values().filter(|b| b.land_use == LandUse::Entertainment).count();
    let venues: Vec<_> = w.buildings.values().filter_map(|b| b.business.as_ref().map(|z| (b, z))).filter(|(_, z)| z.kind == BusinessKind::Venue).collect();
    let venues_open = venues.iter().filter(|(b, z)| !z.closed && b.operating()).count();
    let venue_staff: usize = venues.iter().map(|(b, _)| b.workers.len()).sum();
    let out = w.citizens.values().filter(|c| matches!(c.activity, Activity::Traveling { purpose: Purpose::Visit(LandUse::Entertainment), .. })).count()
        + w.citizens.values().filter(|c| c.inside.and_then(|b| w.buildings.get(b)).is_some_and(|b| b.land_use == LandUse::Entertainment)).count();
    let d = w.commercial_demand();
    let nightlife_tax: f32 = nd_sim::tax::TaxCategory::group(LandUse::Entertainment).iter().map(|c| w.economy.taxes.yesterday_of(*c)).sum();
    println!(
        "{label}: day {} hour {} pop {} | lots: commercial {} leisure(entertainment) {} -> zoned commercial {} of which leisure sub-zone {} | leisure buildings {} venues {} open {} staff {} | people out or at leisure now {} | commercial buildings by sub-zone neighbourhood/retail/offices/leisure {:?} sites {:?} | demand shops {:.2} leisure {:.2} sub {:?} overall {:.2} | nightlife tax yesterday {:.0}",
        w.clock.day(),
        w.clock.hour(),
        w.citizens.len(),
        lots(LandUse::Commercial),
        lots(LandUse::Entertainment),
        zoned_commercial,
        leisure_lots,
        ent_buildings,
        venues.len(),
        venues_open,
        venue_staff,
        out,
        by_sub,
        sites_by_sub,
        w.economy.demand_for(LandUse::Commercial),
        w.economy.demand_for(LandUse::Entertainment),
        d.map(|x| (x * 100.0).round() / 100.0),
        w.commercial_demand_overall(),
        nightlife_tax,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).expect("a save");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut w = World::read_save(src).expect("load");
    let mut minds = RuleBasedMind::default();
    report(&w, "loaded");
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        let mut evening = false;
        loop {
            w.tick(&mut minds);
            w.drain_events();
            if w.clock.hour() == 22 && !evening {
                evening = true;
                report(&w, "  22:00");
            }
            if w.clock.minute_of_day() == 0 {
                break;
            }
        }
        report(&w, &format!("day end ({:.0} s)", t0.elapsed().as_secs_f64()));
    }
}
