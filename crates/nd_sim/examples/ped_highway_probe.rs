//! Load a save and list the people standing on highway or ramp carriageways,
//! the cabs parked on them and the taxi requests there; run on and repeat.
//! Not part of the game. `ped_highway_probe <save.ron> [ticks] [x y r]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::taxi::{CabJob, CabPlace};
use nd_sim::World;
use glam::Vec2;
use std::collections::BTreeMap;

fn on_motorway(w: &World, p: Vec2) -> Option<(u32, bool)> {
    let (sid, _, q) = w.roads.nearest_on_network(p)?;
    let s = w.roads.segment(sid);
    (s.fronts_nothing() && q.distance(p) <= s.half_width() + 0.5).then_some((sid.0, s.ramp))
}

fn report(w: &World, label: &str, at: Option<(Vec2, f32)>) {
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0;
    let mut examples = Vec::new();
    for (id, c) in w.citizens.iter() {
        if c.inside.is_some() || matches!(c.activity, nd_sim::citizens::Activity::Riding { .. } | nd_sim::citizens::Activity::Driving { .. } | nd_sim::citizens::Activity::Passenger { .. } | nd_sim::citizens::Activity::OnTrain { .. }) {
            continue;
        }
        if let Some((sid, ramp)) = on_motorway(w, c.pos).filter(|_| w.stranded_on_motorway(c.pos)) {
            n += 1;
            *by.entry(c.activity.label().to_string()).or_default() += 1;
            if examples.len() < 8 {
                examples.push(format!("{id:?} {:?} seg {sid} ramp {ramp} at ({:.1},{:.1}) {}", c.activity.label(), c.pos.x, c.pos.y, c.thought));
            }
        }
    }
    println!("{label}: tick {} on motorway {n} {:?}", w.clock.tick, by);
    for e in examples {
        println!("   {e}");
    }
    let mut cabs = 0;
    for (id, cab) in w.taxis.cabs.iter() {
        let p = match cab.place {
            CabPlace::Kerb { pos, .. } | CabPlace::Rank { pos, .. } => pos,
            _ => continue,
        };
        if let Some((sid, ramp)) = on_motorway(w, p) {
            cabs += 1;
            println!("   cab {id:?} {:?} job {:?} parked on seg {sid} ramp {ramp} at ({:.1},{:.1}) since {}", cab.place, cab.job, p.x, p.y, cab.since);
        }
    }
    let reqs: Vec<_> = w.taxis.requests.iter().filter(|r| on_motorway(w, r.at).is_some() || w.roads.segment(r.segment).fronts_nothing()).collect();
    println!("   cabs parked on motorway {cabs}, requests on motorway {} of {}", reqs.len(), w.taxis.requests.len());
    for r in reqs.iter().take(5) {
        println!("     req rider {:?} seg {} at ({:.1},{:.1}) cab {:?} since {}", r.rider, r.segment.0, r.at.x, r.at.y, r.cab, r.since);
    }
    if let Some((p, r)) = at {
        for (id, c) in w.citizens.iter().filter(|(_, c)| c.inside.is_none() && c.pos.distance(p) < r).take(12) {
            let path = match &c.activity {
                nd_sim::citizens::Activity::Traveling { path, idx, .. } => format!("idx {idx} {:?}", path.iter().map(|q| (q.x.round(), q.y.round())).collect::<Vec<_>>()),
                _ => String::new(),
            };
            println!("   near {id:?} {:?} ({:.1},{:.1}) transit {:?} {path}", c.activity.label(), c.pos.x, c.pos.y, c.transit.map(|t| t.leg));
        }
        let _ = CabJob::Idle;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let at = if args.len() >= 6 { Some((Vec2::new(args[3].parse().unwrap(), args[4].parse().unwrap()), args[5].parse().unwrap())) } else { None };
    let mut w = World::from_ron(&text).expect("load");
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    report(&w, "loaded", at);
    let snap = |w: &World| -> std::collections::HashMap<nd_sim::citizens::CitizenId, (String, Vec2, bool, bool, String)> {
        w.citizens.iter().map(|(id, c)| {
            let outside = c.inside.is_none() && !matches!(c.activity, nd_sim::citizens::Activity::Riding { .. } | nd_sim::citizens::Activity::Driving { .. } | nd_sim::citizens::Activity::Passenger { .. } | nd_sim::citizens::Activity::OnTrain { .. });
            (id, (c.activity.label().to_string(), c.pos, outside && on_motorway(w, c.pos).is_some(), c.transit.is_some(), format!("{:?}", c.activity).chars().take(60).collect()))
        }).collect()
    };
    let mut prev = snap(&w);
    let mut shown = 0;
    for i in 0..ticks {
        w.tick(&mut minds);
        let now = snap(&w);
        for (id, n) in &now {
            if let Some(p) = prev.get(id) {
                if n.2 && !p.2 && shown < 40 {
                    shown += 1;
                    println!("NEW {id:?} tick {}: was {:?} ({:.1},{:.1}) transit {} [{}] -> now {:?} ({:.1},{:.1}) [{}]", w.clock.tick, p.0, p.1.x, p.1.y, p.3, p.4, n.0, n.1.x, n.1.y, n.4);
                }
            }
        }
        prev = now;
        // `WATCH=<slot index>`: the walk route of that citizen when it changes.
        if let Ok(k) = std::env::var("WATCH") {
            let k: u32 = k.parse().unwrap_or(0);
            if let Some((id, c)) = w.citizens.iter().find(|(id, _)| format!("{id:?}").starts_with(&format!("CitizenId({k}v"))) {
                if let nd_sim::citizens::Activity::Traveling { path, idx, .. } = &c.activity {
                    if *idx == 0 {
                        println!("WATCH {id:?} tick {} mode {:?} from ({:.1},{:.1}) path {:?}", w.clock.tick, c.mode, c.pos.x, c.pos.y, path.iter().map(|q| (q.x.round(), q.y.round())).collect::<Vec<_>>());
                    }
                }
            }
        }
        if (i + 1) % 120 == 0 {
            report(&w, &format!("+{}", i + 1), at);
        }
    }
}
