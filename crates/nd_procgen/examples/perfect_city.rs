//! Probe: generate a city and run it headless, a row of figures per day.
//!
//! cargo run --profile fast -p nd_procgen --example perfect_city -- [days] [preset] [seed]
//!
//! [perfect-city] With the defaults it lays out the Perfect AI City, writes
//! its day-1 save to `saves/perfect_ai_city.ron` (`ND_SAVE` names another
//! file; `ND_SAVE=` skips it) and runs it headless, a row of figures per
//! day. `ND_SAVE_AT=<day>:<path>` also saves the grown city after that day;
//! `ND_DUMP=<file>` writes the plan and lots as text for a quick map.

use nd_minds::RuleBasedMind;
use nd_procgen::citygen::{self, CityPreset};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let days: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let preset = CityPreset::parse(args.get(2).map_or("perfect_city", |s| s)).expect("preset");
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(preset.default_seed());
    let t0 = std::time::Instant::now();
    let (mut world, plan, laid) = citygen::generate_world(preset, seed);
    for line in &laid.furnished {
        println!("  {line}");
    }
    if preset == CityPreset::PerfectCity {
        world.identity.name = "Perfect AI City".into();
    }
    let save = std::env::var("ND_SAVE").unwrap_or_else(|_| if preset == CityPreset::PerfectCity { "saves/perfect_ai_city.ron".into() } else { String::new() });
    if !save.is_empty() {
        write_save(&world, &save);
    }
    let save_at: Option<(u64, String)> = std::env::var("ND_SAVE_AT").ok().and_then(|s| s.split_once(':').and_then(|(d, p)| Some((d.parse().ok()?, p.to_string()))));
    if let Some(out) = std::env::var_os("ND_DUMP") {
        dump(&world, &plan, &std::path::PathBuf::from(out));
    }
    println!("{preset:?} seed {seed}: roads {} zones {} lots {} way_in {} in {:?}; treasury {:.0}; cores {:?} links {:?}", laid.roads, laid.zones, laid.lots, laid.way_in, t0.elapsed(), world.economy.treasury, plan.cores, plan.links);
    let mut minds = RuleBasedMind::default();
    println!("day   pop  housed unhoused  homes   jobs  filled  bld sites | served_min dark_max | cars stuck grid | treas | cov p/f/h/s | crim homeless | ms/day");
    for _ in 0..days {
        let t = std::time::Instant::now();
        let (mut served_min, mut dark_max, mut stuck_max, mut grid_max, mut cars_max) = (1.0f32, 0usize, 0usize, 0usize, 0usize);
        let mut worst: Vec<nd_sim::Vec2> = Vec::new();
        for m in 0..1440 {
            world.tick(&mut minds);
            if m % 60 == 30 {
                let e = &world.economy;
                served_min = served_min.min(e.power.served);
                dark_max = dark_max.max(e.power.unpowered_buildings as usize);
                cars_max = cars_max.max(world.traffic.cars.len());
                let stuck: Vec<nd_sim::Vec2> = world.traffic.cars.values().filter(|c| c.stalled > 180.0 && c.hold <= 0.0 && !c.entering && c.wait != nd_sim::traffic::Wait::Signal).map(|c| c.pos).collect();
                if stuck.len() > stuck_max {
                    stuck_max = stuck.len();
                    worst = stuck;
                }
                grid_max = grid_max.max(world.traffic.gridlocks().len());
            }
        }
        let e = &world.economy;
        let housed: usize = world.citizens.values().filter(|c| c.home.is_some()).count();
        let sites = world.buildings.values().filter(|b| b.construction.is_some()).count();
        let cov = &world.services.stats.coverage;
        println!(
            "{:>3} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>4} | {:>5.2} {:>5} | {:>5} {:>4} {:>3} | {:>9.0} | {:.0}/{:.0}/{:.0}/{:.0} | {:>4} {:>4} | {}",
            world.clock.day(),
            world.citizens.len(),
            housed,
            world.citizens.len() - housed,
            e.homes_total,
            e.jobs_total,
            e.jobs_total - e.jobs_free,
            world.buildings.len(),
            sites,
            served_min,
            dark_max,
            cars_max,
            stuck_max,
            grid_max,
            e.treasury,
            cov[0] * 100.0,
            cov[1] * 100.0,
            cov[2] * 100.0,
            cov[3] * 100.0,
            world.society.crime.offenders.len(),
            world.homeless_people(),
            t.elapsed().as_millis()
        );
        if let Some((d, path)) = &save_at {
            if world.clock.day() == *d {
                write_save(&world, path);
            }
        }
        let st = &world.vehicles.stats;
        let total: u32 = st.trips_yesterday.iter().sum();
        let shares: Vec<String> = nd_sim::citizens::TripMode::ALL.iter().map(|m| format!("{} {:.0}%", m.key(), 100.0 * st.trips_yesterday[m.index()] as f32 / total.max(1) as f32)).collect();
        println!("      modes: {} | metro riders {}", shares.join(" "), world.subway.stats.riders_yesterday);
        if stuck_max >= 60 {
            let mut cells: std::collections::BTreeMap<(i32, i32), usize> = Default::default();
            for p in &worst {
                *cells.entry(((p.x / 100.0).floor() as i32, (p.y / 100.0).floor() as i32)).or_default() += 1;
            }
            let mut v: Vec<_> = cells.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            let top: Vec<String> = v.iter().take(10).map(|((x, y), n)| format!("({},{}):{n}", x * 100 + 50, y * 100 + 50)).collect();
            println!("      stuck at worst hour by 100 m cell: {}", top.join(" "));
        }
    }
}

fn poly(s: &mut String, tag: &str, pts: &[nd_sim::Vec2]) {
    use std::fmt::Write;
    let _ = write!(s, "{tag}");
    for p in pts {
        let _ = write!(s, " {:.1},{:.1}", p.x, p.y);
    }
    let _ = writeln!(s);
}

/// The plan and the laid city as text for `render.py`.
fn dump(world: &nd_sim::World, plan: &citygen::CityPlan, out: &std::path::Path) {
    let mut s = String::new();
    let land = &world.landscape;
    for (outline, holes) in land.water_polygons() {
        poly(&mut s, "W", &outline);
        for h in holes {
            poly(&mut s, "I", &h);
        }
    }
    for r in &land.rivers {
        poly(&mut s, &format!("V {:.0}", r.width), &r.points);
    }
    for z in &plan.zones {
        poly(&mut s, &format!("Z {:?}", z.land_use), &z.polygon);
    }
    for l in world.lots.values() {
        poly(&mut s, &format!("L {:?}", l.land_use), &l.polygon);
    }
    for (id, seg) in world.roads.alive_segments() {
        let (a, b) = world.roads.segment_points(id);
        poly(&mut s, if seg.is_highway() { "H" } else { "S" }, &[a, b]);
    }
    for (_, b) in world.buildings.iter() {
        if world.lots.get(b.lot).is_some_and(|l| world.is_city_ground(l)) || b.plant.is_some() {
            if let Some(l) = world.lots.get(b.lot) {
                poly(&mut s, &format!("B {}", b.name.replace(' ', "_")), &[l.center]);
            }
        }
    }
    for h in &land.hot_spots {
        poly(&mut s, "B HOT", &[h.pos]);
    }
    std::fs::write(out, s).unwrap();
}

fn write_save(world: &nd_sim::World, path: &str) {
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("save file"));
    world.write_ron(file).expect("write save");
    println!("saved {path} (day {}, population {})", world.clock.day(), world.citizens.len());
}
