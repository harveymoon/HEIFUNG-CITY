//! [bridge-riders] Follow one citizen's drives in a save: every `every`
//! ticks their activity, their car's errand, position, trip timing, anger
//! and race / takeover role, and the journey line the inspector shows.
//! Not part of the game.
//! `driver_probe <save> <citizen idx:version> [ticks] [every]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{Activity, World};
use slotmap::{Key, KeyData};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let (i, v) = args[2].split_once(':').expect("idx:version");
    let ffi = (u64::from(v.parse::<u32>().unwrap()) << 32) | u64::from(i.parse::<u32>().unwrap());
    let who: nd_sim::CitizenId = KeyData::from_ffi(ffi).into();
    let ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let every: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    println!("start day {} {:02}:{:02} tick {} citizen {:?} {:?}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.clock.tick, who.data(), w.citizens.get(who).map(|c| c.name.clone()));
    let mut last = String::new();
    for k in 0..ticks {
        w.tick(&mut minds);
        if k % every != 0 {
            continue;
        }
        let Some(c) = w.citizens.get(who) else {
            println!("gone");
            break;
        };
        let car = match &c.activity {
            Activity::Driving { car, .. } => Some(*car),
            _ => None,
        };
        let line = match car.and_then(|id| w.traffic.cars.get(id).map(|x| (id, x))) {
            Some((id, x)) => {
                let j = w.car_journey(id).map(|j| j.purpose).unwrap_or_default();
                let (el, ex) = w.car_trip_minutes(id).unwrap_or((0.0, 0.0));
                let end = x.route.last().map(|&l| w.traffic.net.lanes[l as usize].point_at(x.s_end).0).unwrap_or_default();
                let since = c.commute.search_since;
                format!("end ({:.0},{:.0}) lanes left {} replans {} search_since {} | {:?} car {:?} errand {:?} pos ({:.0},{:.0}) v {:.1} trip {el:.0}/{ex:.0} anger {:.2} calm {} gang {:?} | {j} | {}", end.x, end.y, x.route.len() - x.idx, x.replans, since, c.activity, id.data(), x.errand, x.pos.x, x.pos.y, x.v, x.drive.anger, x.drive.calm.is_some(), x.drive.gang, w.stop_reason_text(id))
            }
            None => format!("{:?} at ({:.0},{:.0})", c.activity, c.pos.x, c.pos.y),
        };
        if line != last {
            println!("day {} {:02}:{:02} {line}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60);
            last = line;
        }
    }
}
