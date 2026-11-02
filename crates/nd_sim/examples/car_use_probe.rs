//! [r7-citizen-story] Probe: load a save and run it on, day by day, with
//! how the town's car owners use their cars (trips by mode, owners who
//! never drove, cars left at a short-stay kerb by home) and what the
//! parking rules did (tickets, clamps, tows, releases), plus jams; and,
//! optionally, one citizen's day hour by hour (car, activity, memories,
//! travel history).
//!
//! cargo run --profile fast -p nd_sim --example car_use_probe -- <save.ron> <days> ["Name" | -] [hours]
//! Not part of the game.

use nd_minds::{HybridMind, MockBackend};
use nd_sim::citizens::CitizenId;
use nd_sim::vehicle::{CarPlace, ParkKind};
use nd_sim::World;
use std::collections::HashMap;

fn hhmm(t: u64) -> String {
    format!("d{} {:02}:{:02}", t / 1440 + 1, (t % 1440) / 60, t % 60)
}

fn owners(w: &World, drove: &HashMap<CitizenId, u32>) -> String {
    let (mut n, mut never, mut kerb_home, mut limited, mut pound) = (0, 0, 0, 0, 0);
    for (id, c) in w.citizens.iter() {
        let Some(vid) = c.car else { continue };
        let Some(car) = w.vehicles.cars.get(vid) else { continue };
        n += 1;
        if drove.get(&id).copied().unwrap_or(0) == 0 {
            never += 1;
        }
        if let CarPlace::Parked(s) = car.place {
            if s.kind == ParkKind::Pound || s.kind == ParkKind::Hooked {
                pound += 1;
            }
            let home = c.home.and_then(|h| w.building_entrance(h));
            if s.kind.on_street() && home.is_some_and(|h| h.distance(s.pos) < 400.0) {
                kerb_home += 1;
                if w.kerb_limit(vid).is_some() {
                    limited += 1;
                }
            }
        }
    }
    format!("owners {n} never-drove-today {never} kerb-near-home {kerb_home} (time-limited {limited}) pound/hooked {pound}")
}

fn dump(w: &World, id: CitizenId) {
    let c = &w.citizens[id];
    let car = c.car.and_then(|v| w.vehicles.cars.get(v).map(|x| (v, x)));
    let place = match car {
        Some((v, x)) => match x.place {
            CarPlace::Parked(s) => {
                let home = c.home.and_then(|h| w.building_entrance(h));
                format!("{:?} parked {:?} since {} booted {} ticketed {} limit {:?} {:.0} m from home", v, s.kind, hhmm(s.since), s.booted, s.ticketed, w.kerb_limit(v), home.map_or(-1.0, |h| h.distance(s.pos)))
            }
            CarPlace::Driving(t) => format!("{v:?} driving {t:?}"),
        },
        None => "no car".into(),
    };
    println!("  [{}] {} | {} | owns_car {} bike {} | savings {:.0} | {}", hhmm(w.clock.tick), c.name, w.activity_line(id), c.owns_car, c.owns_bike, c.savings, place);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save");
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let who = args.get(3).cloned().filter(|s| s != "-");
    let every: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut w = World::read_save(&src).expect("load");
    let id = who.as_ref().and_then(|n| w.citizens.iter().find(|(_, c)| &c.name == n).map(|(id, _)| id));
    println!("loaded {} at {} pop {}", src, hhmm(w.clock.tick), w.citizens.len());
    if let Some(id) = id {
        dump(&w, id);
        for m in &w.citizens[id].memories {
            println!("    mem {} {}", hhmm(m.tick), m.text);
        }
    }
    let mut drove: HashMap<CitizenId, u32> = HashMap::new();
    let mut jam_sum = 0usize;
    let mut jam_n = 0usize;
    let mut peak = 0usize;
    let mut stalled_peak = 0usize;
    let start = w.clock.tick;
    let end = start + days * 1440;
    while w.clock.tick < end {
        w.tick(&mut minds);
        let t = w.clock.tick;
        peak = peak.max(w.traffic.cars.len());
        if t % 10 == 0 {
            for (cid, c) in w.citizens.iter() {
                if matches!(c.activity, nd_sim::citizens::Activity::Driving { .. }) {
                    *drove.entry(cid).or_default() += 1;
                }
            }
        }
        if t % 30 == 0 {
            let stalled = w.traffic.cars.values().filter(|c| c.stalled > 120.0).count();
            jam_sum += stalled;
            jam_n += 1;
            stalled_peak = stalled_peak.max(stalled);
        }
        if let Some(id) = id.filter(|_| t % every == 0) {
            if w.citizens.contains_key(id) {
                dump(&w, id);
            }
        }
        if t % 1440 == 1439 {
            let v = &w.vehicles.stats;
            let e = &w.enforcement.stats;
            println!(
                "DAY {} pop {} trips walk {} bike {} bus {} train {} car {} metro {} taxi {} | tickets {} clamps {} (paid {}) tows {} released {} auctioned {} | cars peak {} stalled>2min mean {:.0} peak {} | {}",
                t / 1440 + 1,
                w.citizens.len(),
                v.trips_today[0],
                v.trips_today[1],
                v.trips_today[2],
                v.trips_today[3],
                v.trips_today[4],
                v.trips_today[5],
                v.trips_today[6],
                e.tickets_today,
                e.clamps_today,
                e.clamps_paid_today,
                e.tows_today,
                e.released_today,
                e.auctioned_today,
                peak,
                jam_sum as f32 / jam_n.max(1) as f32,
                stalled_peak,
                owners(&w, &drove)
            );
            drove.clear();
            peak = 0;
            jam_sum = 0;
            jam_n = 0;
            stalled_peak = 0;
        }
    }
    // The save's cost of the travel history: its string, and the whole
    // save gzipped as the game writes it.
    {
        use std::io::Write;
        let travel = ron::to_string(&w.travel).unwrap_or_default().len();
        let trips: usize = w.travel.logs.values().map(|l| l.len()).sum();
        let text = w.to_ron().unwrap_or_default();
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = z.write_all(text.as_bytes());
        let gz = z.finish().map(|v| v.len()).unwrap_or(0);
        println!("SAVE travel string {travel} bytes for {trips} trips of {} people ({:.1} bytes/trip); whole save {} bytes text, {gz} bytes gzipped ({:.2}% travel)", w.travel.logs.len(), travel as f32 / trips.max(1) as f32, text.len(), travel as f32 * 100.0 / gz.max(1) as f32);
    }
    if let Some(id) = id.filter(|id| w.citizens.contains_key(*id)) {
        for m in &w.citizens[id].memories {
            println!("    mem {} {}", hhmm(m.tick), m.text);
        }
        println!("    car: {:?}", w.car_whereabouts(id));
        for line in w.travel_lines(id, 2) {
            println!("    trip {line}");
        }
    }
}
