//! [r16-crash] Damaged and hand-edited saves: a save cut short is refused
//! with an error (never a panic), and a save missing things the rest of it
//! names (a building, people, a household, a bus line, a road piece, a
//! lot, cars) loads and runs a day.
//!
//! cargo test --profile fast -p nd_sim --test bad_saves -- --nocapture

mod common;

use nd_minds::RuleBasedMind;
use nd_sim::World;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The busy demo city two days further on (people at work, cars out).
fn city() -> World {
    let (mut w, mut m) = common::busy_city();
    for _ in 0..1440 * 2 {
        w.tick(&mut m);
    }
    w
}

/// `edit` the city, save it, load it and run it a day; the panic's
/// message if any step panicked.
fn survives(name: &str, base: &World, edit: impl FnOnce(&mut World)) -> Result<(), String> {
    let mut w = base.clone();
    let r = catch_unwind(AssertUnwindSafe(move || {
        edit(&mut w);
        let text = w.to_save_ron().expect("serialises");
        let mut back = World::from_ron(&text).expect("an edited save still parses");
        let mut m = RuleBasedMind::default();
        for _ in 0..1440 {
            back.tick(&mut m);
        }
        back.to_save_ron().expect("and saves again");
    }));
    r.map_err(|p| {
        let m = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        format!("{name}: {m}")
    })
}

#[test]
fn a_save_missing_what_it_names_loads_and_runs() {
    let base = city();
    eprintln!("{} buildings, {} people, {} cars, {} bus lines", base.buildings.len(), base.citizens.len(), base.traffic.cars.len(), base.transit.routes.len());
    assert!(base.buildings.len() > 10 && base.citizens.len() > 50 && !base.traffic.cars.is_empty());
    let mut failed = Vec::new();
    let mut check = |name: &str, edit: &dyn Fn(&mut World)| {
        if let Err(e) = survives(name, &base, |w| edit(w)) {
            failed.push(e);
        }
    };
    // The busiest building (workers, residents, maybe a business).
    check("a building gone", &|w| {
        let id = w.buildings.iter().max_by_key(|(_, b)| b.workers.len() + b.residents.len()).map(|(id, _)| id).expect("a building");
        w.buildings.remove(id);
    });
    check("every third building gone", &|w| {
        let ids: Vec<_> = w.buildings.keys().step_by(3).collect();
        for id in ids {
            w.buildings.remove(id);
        }
    });
    check("people gone", &|w| {
        let ids: Vec<_> = w.citizens.keys().step_by(4).collect();
        for id in ids {
            w.citizens.remove(id);
        }
    });
    check("households gone", &|w| {
        let ids: Vec<_> = w.households.keys().step_by(3).collect();
        for id in ids {
            w.households.remove(id);
        }
    });
    check("lots gone", &|w| {
        let ids: Vec<_> = w.lots.keys().step_by(5).collect();
        for id in ids {
            w.lots.remove(id);
        }
    });
    check("cars gone", &|w| {
        let ids: Vec<_> = w.traffic.cars.keys().step_by(2).collect();
        for id in ids {
            w.traffic.cars.remove(id);
        }
    });
    check("bus lines gone", &|w| {
        let ids: Vec<_> = w.transit.routes.keys().collect();
        for id in ids {
            w.transit.routes.remove(id);
        }
    });
    check("the last road piece gone", &|w| {
        w.roads.segments.pop();
    });
    check("service units gone", &|w| {
        let ids: Vec<_> = w.services.units.keys().collect();
        for id in ids {
            w.services.units.remove(id);
        }
    });
    assert!(failed.is_empty(), "{} of the edited saves panicked:\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn a_save_cut_short_is_refused_not_a_panic() {
    let w = common::demo_world();
    let text = w.to_save_ron().expect("serialises");
    let mut gz = Vec::new();
    w.write_ron_gz(&mut gz).expect("compresses");
    let mut panicked = Vec::new();
    for k in 1..40 {
        let cut = text.len() * k / 40;
        match catch_unwind(|| World::from_save_bytes(&text.as_bytes()[..cut]).is_err()) {
            Ok(true) => {}
            Ok(false) => panicked.push(format!("plain cut at {cut} loaded")),
            Err(_) => panicked.push(format!("plain cut at {cut} panicked")),
        }
        let cut = gz.len() * k / 40;
        match catch_unwind(|| World::from_save_bytes(&gz[..cut]).is_err()) {
            Ok(true) => {}
            Ok(false) => panicked.push(format!("packed cut at {cut} loaded")),
            Err(_) => panicked.push(format!("packed cut at {cut} panicked")),
        }
    }
    // Junk, and a save with its numbers changed to nonsense.
    assert!(World::from_save_bytes(b"").is_err());
    assert!(World::from_save_bytes(b"\x1f\x8b\x08garbage").is_err());
    assert!(World::from_save_bytes(b"(not: a world)").is_err());
    assert!(panicked.is_empty(), "{}", panicked.join("\n"));
}

/// A real save (`ND_R16_SAVE=<path>`, a copy of a player's): it loads,
/// runs, and loads and runs again with the same edits as above.
/// cargo test --profile fast -p nd_sim --test bad_saves -- --ignored --nocapture
#[test]
#[ignore]
fn a_real_save_edited_loads_and_runs() {
    let Some(path) = std::env::var_os("ND_R16_SAVE") else { return };
    let t = std::time::Instant::now();
    let base = World::read_save(&path).expect("the save loads");
    eprintln!("loaded {} people, {} buildings in {:.1} s", base.citizens.len(), base.buildings.len(), t.elapsed().as_secs_f32());
    let ticks = 120;
    let run = |name: &str, edit: &dyn Fn(&mut World)| -> Result<(), String> {
        let mut w = base.clone();
        catch_unwind(AssertUnwindSafe(move || {
            edit(&mut w);
            let mut back = World::from_ron(&w.to_save_ron().expect("serialises")).expect("parses");
            let mut m = RuleBasedMind::default();
            for _ in 0..ticks {
                back.tick(&mut m);
            }
        }))
        .map_err(|p| format!("{name}: {}", p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()))
    };
    let mut failed = Vec::new();
    let cases: Vec<(&str, Box<dyn Fn(&mut World)>)> = vec![
        ("as saved", Box::new(|_| {})),
        ("every seventh building gone", Box::new(|w| {
            let ids: Vec<_> = w.buildings.keys().step_by(7).collect();
            for id in ids {
                w.buildings.remove(id);
            }
        })),
        ("every fifth person gone", Box::new(|w| {
            let ids: Vec<_> = w.citizens.keys().step_by(5).collect();
            for id in ids {
                w.citizens.remove(id);
            }
        })),
        ("every third car gone", Box::new(|w| {
            let ids: Vec<_> = w.traffic.cars.keys().step_by(3).collect();
            for id in ids {
                w.traffic.cars.remove(id);
            }
        })),
        ("the last road piece gone", Box::new(|w| {
            w.roads.segments.pop();
        })),
    ];
    for (name, edit) in &cases {
        let t = std::time::Instant::now();
        let r = run(name, edit.as_ref());
        eprintln!("{name}: {} in {:.1} s", if r.is_ok() { "ran" } else { "PANICKED" }, t.elapsed().as_secs_f32());
        if let Err(e) = r {
            failed.push(e);
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
