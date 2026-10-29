//! [sound] Render every UI cue and a set of bed states to WAV files with
//! the game's own engine, and print their levels and spectra:
//! `cargo run -p nd_audio --example render_wavs -- <out dir>`.

use nd_audio::cues::Cue;
use nd_audio::scene::{bed_params, Surroundings};
use nd_audio::wav::{analyse, render_beds, render_cue, write_wav, SR};
use nd_audio::MixLevels;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "wav".into()));
    std::fs::create_dir_all(&dir).expect("out dir");
    // The default sliders, as a new player hears them.
    let mix = MixLevels::default();
    println!("UI cues (default sliders: master {}, ui {}):", mix.master, mix.ui);
    for cue in Cue::ALL {
        let s = render_cue(cue, mix, 1.2);
        let path = dir.join(format!("ui_{}.wav", cue.key()));
        write_wav(&path, &s, SR).expect("write");
        println!("  {:<16} {}", cue.key(), analyse(&s, SR).line());
    }
    // A drag: zone paint asked every frame (60 fps) for 2 s, through the
    // rate limiter, as the game does.
    {
        let mut lim = nd_audio::RateLimiter::default();
        let mut e = nd_audio::Engine::new(SR as f32, Default::default());
        e.apply(nd_audio::Command::Mix(MixLevels { ambience: 0.0, ..mix }));
        e.settle();
        let frame = SR as usize / 60;
        let mut out = vec![0.0; SR as usize * 3 * 2];
        let mut played = 0;
        for (i, chunk) in out.chunks_mut(frame * 2).enumerate() {
            let now = i as f64 / 60.0;
            if now < 2.0 {
                if let Some(g) = lim.allow(Cue::ZonePaint, now) {
                    e.apply(nd_audio::Command::Cue(Cue::ZonePaint, g));
                    played += 1;
                }
            }
            e.render(chunk);
        }
        write_wav(&dir.join("ui_zone_paint_drag.wav"), &out, SR).expect("write");
        println!("  {:<16} {} ({played} of 120 frames played)", "zone_drag", analyse(&out, SR).line());
    }
    let street = Surroundings { height: 2.0, radius: 60.0, ..Default::default() };
    let states: Vec<(&str, Surroundings)> = vec![
        ("high_city_day", Surroundings { height: 1_200.0, radius: 1_400.0, cars_near: 900.0, car_speed: 9.0, density: 0.6, daylight: 1.0, wind: 0.45, ..Default::default() }),
        ("high_gale_night", Surroundings { height: 1_200.0, radius: 1_400.0, cars_near: 300.0, car_speed: 9.0, density: 0.6, daylight: 0.0, wind: 1.0, neon: 0.5, ..Default::default() }),
        ("mid_traffic_slow", Surroundings { height: 140.0, radius: 200.0, cars_near: 60.0, car_speed: 4.0, density: 0.6, ..Default::default() }),
        ("mid_traffic_fast", Surroundings { height: 140.0, radius: 200.0, cars_near: 60.0, car_speed: 20.0, density: 0.6, ..Default::default() }),
        ("mid_quiet_suburb", Surroundings { height: 140.0, radius: 200.0, cars_near: 3.0, car_speed: 8.0, density: 0.15, green: 0.7, ..Default::default() }),
        ("street_neon_night_market", Surroundings { cars_close: 10.0, cars_near: 10.0, car_speed: 6.0, walkers_close: 45.0, walkers_near: 45.0, density: 0.9, neon: 1.0, daylight: 0.0, ..street }),
        ("street_downtown_day", Surroundings { cars_close: 18.0, cars_near: 18.0, car_speed: 11.0, walkers_close: 20.0, walkers_near: 20.0, density: 0.8, neon: 0.2, daylight: 1.0, ..street }),
        ("street_suburb_day_birds", Surroundings { cars_close: 1.0, cars_near: 1.0, car_speed: 8.0, walkers_close: 2.0, density: 0.15, green: 0.8, daylight: 1.0, ..street }),
        ("street_park_night_crickets", Surroundings { cars_close: 0.0, walkers_close: 1.0, density: 0.05, green: 1.0, daylight: 0.0, wind: 0.3, ..street }),
        ("street_rain", Surroundings { cars_close: 8.0, cars_near: 8.0, car_speed: 9.0, walkers_close: 5.0, density: 0.7, daylight: 0.6, rain: 0.9, ..street }),
        ("street_snow", Surroundings { cars_close: 6.0, cars_near: 6.0, car_speed: 6.0, walkers_close: 8.0, density: 0.7, daylight: 0.6, snow: 0.9, ..street }),
        ("street_waterfront", Surroundings { cars_close: 2.0, cars_near: 2.0, car_speed: 6.0, walkers_close: 6.0, density: 0.3, water: 0.5, daylight: 0.8, green: 0.3, ..street }),
        ("underground", Surroundings { cars_close: 10.0, cars_near: 10.0, car_speed: 9.0, walkers_close: 20.0, density: 0.8, underground: true, ..street }),
    ];
    println!("Beds (8 s each, ambience {}):", mix.ambience);
    for (name, s) in states {
        let p = bed_params(&s);
        let out = render_beds(p, mix, 8.0);
        write_wav(&dir.join(format!("bed_{name}.wav")), &out, SR).expect("write");
        println!("  {:<28} {}", name, analyse(&out, SR).line());
    }
    // A zoom from the street to 1.5 km and back down over 12 s, to hear
    // the crossfade (no clicks or zips).
    {
        let mut e = nd_audio::Engine::new(SR as f32, Default::default());
        e.apply(nd_audio::Command::Mix(mix));
        let base = Surroundings { cars_close: 14.0, cars_near: 14.0, car_speed: 10.0, walkers_close: 25.0, density: 0.8, neon: 0.7, daylight: 0.0, wind: 0.6, ..street };
        e.apply(nd_audio::Command::Beds(bed_params(&base)));
        e.settle();
        let mut out = vec![0.0; SR as usize * 12 * 2];
        let step = SR as usize / 4;
        for (i, chunk) in out.chunks_mut(step * 2).enumerate() {
            let t = i as f32 / 4.0;
            let u = if t < 6.0 { t / 6.0 } else { (12.0 - t) / 6.0 };
            let h = 2.0 * (750f32).powf(u.clamp(0.0, 1.0));
            let r = (h * 1.2).clamp(60.0, 1_500.0);
            let area = (r / 60.0).powi(2);
            e.apply(nd_audio::Command::Beds(bed_params(&Surroundings { height: h, radius: r, cars_near: 14.0 * area * 0.5, walkers_near: 25.0 * area * 0.3, ..base })));
            e.render(chunk);
        }
        write_wav(&dir.join("bed_zoom_street_to_high_and_back.wav"), &out, SR).expect("write");
        println!("  {:<28} {}", "zoom_street_high_street", analyse(&out, SR).line());
    }
    println!("written to {}", dir.display());
}
