//! Load a save and report what the city has of civic offices, parks and
//! venues against its demand for them. Not part of the game.
//! `cargo run --example civic_probe -- <save.ron> [days]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{LandUse, World};

fn report(w: &World, label: &str) {
    let mut civic = (0u32, 0u32, 0.0f32, 0u32); // buildings, sites, floors, jobs
    let mut civic_lot_area = 0.0f32;
    let mut civic_floor_area = 0.0f32;
    let mut park = (0u32, 0.0f32);
    let mut venues = 0u32;
    let mut venue_jobs = 0u32;
    for b in w.buildings.values() {
        let lot_area: f32 = std::iter::once(b.lot).chain(b.plot.extra_lots.iter().copied()).filter_map(|l| w.lots.get(l)).map(|l| l.area()).sum();
        match b.land_use {
            LandUse::Civic if !b.is_city_structure() => {
                if b.construction.is_some() {
                    civic.1 += 1;
                } else {
                    civic.0 += 1;
                    civic.2 += b.floors as f32;
                    civic.3 += b.jobs as u32;
                    civic_lot_area += lot_area;
                    civic_floor_area += lot_area * 0.5 * b.floors as f32;
                }
            }
            LandUse::Park if b.operating() => {
                park.0 += 1;
                park.1 += lot_area;
            }
            LandUse::Entertainment if b.operating() => {
                venues += 1;
                venue_jobs += b.jobs as u32;
            }
            _ => {}
        }
    }
    let mut zoned = [(0u32, 0.0f32); 6];
    for l in w.lots.values() {
        if l.building.is_none() && l.usable() {
            let z = &mut zoned[l.land_use.index()];
            z.0 += 1;
            z.1 += l.area();
        }
    }
    let e = &w.economy;
    let pop = e.population;
    println!("{label}: day {} pop {pop}", w.clock.day() + 1);
    println!("  demand {:?}", LandUse::ALL.iter().map(|lu| format!("{} {:.0}%", lu.label(), e.demand_for(*lu) * 100.0)).collect::<Vec<_>>());
    println!(
        "  civic: {} buildings ({} sites), avg floors {:.1}, {} jobs, lot area {:.0} m2, floor area {:.0} m2 ({:.1} m2/person); {} vacant civic lots, {:.0} m2",
        civic.0,
        civic.1,
        civic.2 / civic.0.max(1) as f32,
        civic.3,
        civic_lot_area,
        civic_floor_area,
        civic_floor_area / pop.max(1) as f32,
        zoned[LandUse::Civic.index()].0,
        zoned[LandUse::Civic.index()].1
    );
    println!("  parks: {} open, {:.0} m2 ({:.1} m2/person); {} vacant park lots, {:.0} m2", park.0, park.1, park.1 / pop.max(1) as f32, zoned[LandUse::Park.index()].0, zoned[LandUse::Park.index()].1);
    println!("  venues: {venues} ({:.0} people each), {venue_jobs} jobs ({:.1} per 100 people), cover {:?}; jobs {} working age {}", pop as f32 / venues.max(1) as f32, venue_jobs as f32 * 100.0 / pop.max(1) as f32, e.cover_of(nd_sim::market::BusinessKind::Venue), e.jobs_permanent, e.working_age);
    for lu in [LandUse::Civic, LandUse::Park, LandUse::Entertainment] {
        if let Some(r) = e.demand_reason(lu) {
            println!("  why {}: {r}", lu.label());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    report(&w, "loaded");
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    for d in 0..days {
        for _ in 0..1440 {
            w.tick(&mut minds);
        }
        report(&w, &format!("after {} day(s)", d + 1));
    }
}
