//! [sound] Render every spot sound (alone, as the game would place it) and
//! a few mixed city scenes to WAV files with the game's own engine, and
//! print their levels: `cargo run -p nd_audio --example render_spots -- <out dir>`.

use nd_audio::scene::{bed_params, BedParams, Surroundings};
use nd_audio::spots::{pick, Candidate, HornClock, Listener, SpotKind, Strike, ThunderClock, ALL_ON};
use nd_audio::wav::{analyse, render_scene, write_wav, Analysis, SR};
use nd_audio::{Command, MixLevels};

/// A source that moves along x at `v` m/s, `off` metres out in z, from `x0`.
fn mover(id: u64, kind: SpotKind, x0: f32, v: f32, off: f32, y: f32, t: f32) -> Candidate {
    Candidate { vel: [v, 0.0, 0.0], ..Candidate::at(id, kind, x0 + v * t, y, off) }
}

fn street() -> Listener {
    // The camera 25 m back and 12 m up, looking along +z; right is -x.
    Listener { eye: [0.0, 12.0, -25.0], focus: [0.0, 0.0, 0.0], right: [-1.0, 0.0, 0.0], height: 12.0, underground: false }
}

fn at_height(h: f32) -> Listener {
    Listener { eye: [0.0, h, -h * 0.8], height: h, ..street() }
}

struct Scene {
    name: &'static str,
    secs: f32,
    beds: BedParams,
    mix: MixLevels,
    listener: Listener,
    /// The held candidates at `t`.
    cands: Box<dyn Fn(f32) -> Vec<Candidate>>,
    /// Jams (horns) and the storm.
    jams: Vec<Candidate>,
    storm: f32,
    /// Strikes at fixed times: (t, distance, pan).
    strikes: Vec<(f32, f32, f32)>,
    room: f32,
}

fn run(dir: &std::path::Path, s: Scene) -> (Analysis, usize) {
    let mut horns = HornClock::default();
    let mut thunder = ThunderClock::default();
    let mut max_voices = 0usize;
    let mut strikes = s.strikes.clone();
    let l = s.listener;
    let out = render_scene(s.beds, s.mix, s.secs, |t| {
        let mut cmds = vec![Command::Room(s.room)];
        let held = pick(&(s.cands)(t), &l, &ALL_ON);
        let mut n = held.len();
        cmds.extend(held.into_iter().map(Command::Spot));
        let shots = horns.tick(&s.jams, &l, &ALL_ON, 0.25);
        n += shots.len();
        cmds.extend(shots.into_iter().map(Command::Spot));
        for (_, u) in thunder.tick(s.storm, &ALL_ON, l.underground, 0.25) {
            cmds.push(Command::Spot(u));
        }
        strikes.retain(|&(at, dist, pan)| {
            if at <= t {
                cmds.push(Command::Spot(thunder.thunder(Strike { dist, pan }, 1.0, 1.0, l.underground, 0.0)));
                false
            } else {
                true
            }
        });
        max_voices = max_voices.max(n);
        cmds
    });
    write_wav(&dir.join(format!("{}.wav", s.name)), &out, SR).expect("write");
    (analyse(&out, SR), max_voices)
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "wav".into()));
    std::fs::create_dir_all(&dir).expect("out dir");
    let mix = MixLevels::default();
    let solo = MixLevels { ambience: 0.0, ..mix };
    let quiet = BedParams::default();
    let none = |_t: f32| -> Vec<Candidate> { vec![] };
    let mut scenes: Vec<Scene> = Vec::new();
    let base = |name: &'static str, secs: f32, cands: Box<dyn Fn(f32) -> Vec<Candidate>>| Scene {
        name,
        secs,
        beds: quiet,
        mix: solo,
        listener: street(),
        cands,
        jams: vec![],
        storm: 0.0,
        strikes: vec![],
        room: 0.3,
    };
    // Each alone, at the default sliders, placed as the game places them.
    scenes.push(base("spot_train_passby", 16.0, Box::new(|t| vec![mover(1, SpotKind::Train, -200.0, 25.0, 30.0, 0.3, t)])));
    scenes.push(base("spot_train_into_station", 14.0, Box::new(|t| {
        // Braking from 15 m/s to a stand at x = 0 over 10 s, then standing.
        let tt = t.min(10.0);
        let x = -75.0 + 15.0 * tt - 0.75 * tt * tt;
        let v = (15.0 - 1.5 * tt).max(0.0);
        vec![Candidate { vel: [v, 0.0, 0.0], ..Candidate::at(1, SpotKind::Train, x, 0.3, 25.0) }]
    })));
    scenes.push(base("spot_metro_from_street", 12.0, Box::new(|t| vec![mover(2, SpotKind::Metro, -150.0, 22.0, 10.0, -16.0, t)])));
    scenes.push(Scene { listener: Listener { underground: true, ..street() }, ..base("spot_metro_underground_view", 12.0, Box::new(|t| vec![mover(2, SpotKind::Metro, -150.0, 22.0, 10.0, -16.0, t)])) });
    for (i, name) in ["spot_siren_police", "spot_siren_ambulance", "spot_siren_fire"].into_iter().enumerate() {
        // The siren's id picks its kind (id % 3).
        let id = 30 + i as u64;
        scenes.push(base(name, 16.0, Box::new(move |t| vec![mover(id, SpotKind::Siren, -250.0, 16.0 + 3.0 * i as f32, 20.0, 0.5, t)])));
    }
    scenes.push(base("spot_works", 30.0, Box::new(|_| vec![Candidate::at(4, SpotKind::Works, 15.0, 0.0, 30.0)])));
    scenes.push(base("spot_club", 16.0, Box::new(|_| vec![Candidate { intensity: 0.9, ..Candidate::at(5, SpotKind::Club, -12.0, 0.0, 20.0) }])));
    scenes.push(base("spot_bar", 16.0, Box::new(|_| vec![Candidate { intensity: 0.8, ..Candidate::at(6, SpotKind::Bar, 8.0, 0.0, 10.0) }])));
    scenes.push(base("spot_karaoke", 16.0, Box::new(|_| vec![Candidate { intensity: 0.8, ..Candidate::at(7, SpotKind::Karaoke, -8.0, 0.0, 12.0) }])));
    scenes.push(base("spot_music_hall", 16.0, Box::new(|_| vec![Candidate { intensity: 0.9, ..Candidate::at(8, SpotKind::MusicHall, 10.0, 0.0, 25.0) }])));
    let jam: Vec<Candidate> = (0..6).map(|i| Candidate { intensity: 0.85, ..Candidate::at(100 + i, SpotKind::Horn, -30.0 + 12.0 * i as f32, 0.0, 15.0) }).collect();
    scenes.push(Scene { jams: jam.clone(), ..base("spot_horns_angry_jam", 14.0, Box::new(none)) });
    scenes.push(Scene { strikes: vec![(0.0, 350.0, -0.3), (6.0, 1_500.0, 0.5), (12.0, 5_000.0, -0.6)], ..base("spot_thunder_near_mid_far", 22.0, Box::new(none)) });

    // Mixed city scenes over their beds, at the default sliders.
    let street_s = Surroundings { height: 12.0, radius: 60.0, ..Default::default() };
    let night = Surroundings { cars_close: 14.0, cars_near: 30.0, car_speed: 6.0, walkers_close: 40.0, walkers_near: 60.0, density: 0.9, neon: 1.0, daylight: 0.0, ..street_s };
    scenes.push(Scene {
        beds: bed_params(&night),
        mix,
        jams: jam[..3].to_vec(),
        room: 0.8,
        ..base("city_street_neon_night", 24.0, Box::new(|t| {
            let mut v = vec![
                Candidate { intensity: 0.9, ..Candidate::at(5, SpotKind::Club, -20.0, 0.0, 25.0) },
                Candidate { intensity: 0.7, ..Candidate::at(9, SpotKind::Club, 70.0, 0.0, 90.0) },
                Candidate { intensity: 0.8, ..Candidate::at(6, SpotKind::Bar, 9.0, 0.0, 8.0) },
                Candidate { intensity: 0.6, ..Candidate::at(10, SpotKind::Bar, -14.0, 0.0, -6.0) },
                Candidate { intensity: 0.7, ..Candidate::at(7, SpotKind::Karaoke, 25.0, 0.0, 30.0) },
                Candidate { intensity: 0.8, ..Candidate::at(8, SpotKind::MusicHall, -80.0, 0.0, 60.0) },
                mover(1, SpotKind::Train, -400.0, 22.0, 180.0, 0.3, t),
                mover(2, SpotKind::Metro, 200.0, -20.0, 5.0, -16.0, t),
            ];
            if t > 8.0 {
                v.push(mover(30, SpotKind::Siren, -300.0, 18.0, 35.0, 0.5, t - 8.0));
            }
            v
        }))
    });
    let mid = Surroundings { height: 140.0, radius: 200.0, cars_near: 70.0, car_speed: 9.0, walkers_near: 40.0, density: 0.65, daylight: 1.0, ..Default::default() };
    scenes.push(Scene {
        beds: bed_params(&mid),
        mix,
        listener: at_height(140.0),
        jams: jam.clone(),
        room: 0.3,
        ..base("city_mid_day", 24.0, Box::new(|t| {
            vec![
                Candidate::at(4, SpotKind::Works, 60.0, 0.0, 40.0),
                Candidate::at(11, SpotKind::Works, -120.0, 0.0, 90.0),
                Candidate { intensity: 0.6, ..Candidate::at(12, SpotKind::Works, 30.0, 0.0, -150.0) },
                mover(1, SpotKind::Train, -500.0, 28.0, 60.0, 0.3, t),
                mover(2, SpotKind::Metro, 300.0, -22.0, -40.0, -16.0, t),
                mover(31, SpotKind::Siren, -400.0, 17.0, 120.0, 0.5, t),
                Candidate { intensity: 0.4, ..Candidate::at(6, SpotKind::Bar, 20.0, 0.0, 20.0) },
            ]
        }))
    });
    let high = Surroundings { height: 900.0, radius: 1_080.0, cars_near: 700.0, car_speed: 9.0, density: 0.6, daylight: 0.3, wind: 0.95, rain: 0.9, ..Default::default() };
    scenes.push(Scene {
        beds: bed_params(&high),
        mix,
        listener: at_height(900.0),
        storm: 1.0,
        strikes: vec![(1.0, 800.0, 0.4), (9.0, 2_500.0, -0.5), (16.0, 4_000.0, 0.2)],
        room: 0.0,
        ..base("city_high_storm", 26.0, Box::new(|t| {
            vec![mover(30, SpotKind::Siren, -600.0, 18.0, 200.0, 0.5, t), mover(1, SpotKind::Train, -900.0, 30.0, -300.0, 0.3, t)]
        }))
    });
    let under = Surroundings { cars_close: 10.0, cars_near: 10.0, car_speed: 9.0, walkers_close: 20.0, density: 0.8, underground: true, ..street_s };
    scenes.push(Scene {
        beds: bed_params(&under),
        mix,
        listener: Listener { underground: true, ..street() },
        jams: jam[..2].to_vec(),
        ..base("city_underground_view", 16.0, Box::new(|t| {
            vec![mover(2, SpotKind::Metro, -200.0, 20.0, 8.0, -16.0, t), Candidate::at(5, SpotKind::Club, -20.0, 0.0, 25.0), mover(30, SpotKind::Siren, -300.0, 18.0, 35.0, 0.5, t)]
        }))
    });
    // The worst case: every slider at full, everything at once and close.
    let loud = MixLevels { master: 1.0, ambience: 1.0, effects: 1.0, ui: 1.0, music: 1.0, muted: false, duck: 1.0 };
    let wild = Surroundings { rain: 1.0, wind: 1.0, ..night };
    scenes.push(Scene {
        beds: bed_params(&wild),
        mix: loud,
        jams: (0..30).map(|i| Candidate { intensity: 1.0, ..Candidate::at(200 + i, SpotKind::Horn, -20.0 + i as f32, 0.0, 5.0) }).collect(),
        storm: 1.0,
        strikes: vec![(0.5, 260.0, 0.0), (3.0, 300.0, 0.6), (6.0, 280.0, -0.6)],
        room: 1.0,
        ..base("city_worst_case_all_sliders_full", 16.0, Box::new(|t| {
            let mut v = Vec::new();
            for (i, k) in SpotKind::ALL.into_iter().filter(|k| !k.one_shot()).enumerate() {
                for j in 0..4u64 {
                    let id = i as u64 * 10 + j;
                    let a = id as f32;
                    v.push(Candidate { vel: [12.0, 0.0, 0.0], ..Candidate::at(id, k, 8.0 * a.cos() + 12.0 * t.sin(), 0.0, 8.0 * a.sin()) });
                }
            }
            v
        }))
    });
    println!("Spot sounds and city scenes (default sliders unless named):");
    for s in scenes {
        let (name, secs) = (s.name, s.secs);
        let start = std::time::Instant::now();
        let (a, n) = run(&dir, s);
        // The render's cost as a share of real time (beds, spots, picking, the WAV write).
        let cpu = start.elapsed().as_secs_f32() / secs * 100.0;
        println!("  {name:<34} voices {n:>2}  cpu {cpu:>4.1}%  {}", a.line());
    }
    println!("written to {}", dir.display());
}
