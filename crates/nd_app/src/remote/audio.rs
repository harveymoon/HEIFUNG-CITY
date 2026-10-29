//! [sound] `audio`: the mixer's settings and the engine's readout; set the
//! sliders, mute, play a cue; `audio_capture` renders what the camera hears
//! now to a WAV with the same engine, offline.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use crate::audio::{settings, update_settings, Unfocused};
use nd_audio::engine::load;
use nd_audio::{Cue, Status};

/// `audio {get?}` reads; `audio {set: {master, ambience, effects, ui,
/// music, muted, unfocused}}` changes (and saves) them; `audio {play: "<cue>"}`
/// plays a UI cue through the rate limiter.
pub(super) fn audio(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if let Some(set) = cmd.get("set") {
        let obj = set.as_object().ok_or("set must be an object: {master, ambience, effects, ui, music (0..1), muted (bool), unfocused (mute|duck|keep)}")?;
        let mut changed = settings();
        for (k, v) in obj {
            let level = || v.as_f64().map(|x| x as f32).filter(|x| x.is_finite() && (0.0..=1.0).contains(x)).ok_or(format!("{k} must be a number 0..1"));
            match k.as_str() {
                "master" => changed.master = level()?,
                "ambience" => changed.ambience = level()?,
                "effects" => changed.effects = level()?,
                "ui" => changed.ui = level()?,
                "music" => changed.music = level()?,
                "muted" => changed.muted = v.as_bool().ok_or("muted must be true or false")?,
                "unfocused" => changed.unfocused = v.as_str().and_then(Unfocused::parse).ok_or("unfocused must be mute, duck or keep")?,
                other => return Err(format!("unknown audio setting {other:?} (master, ambience, effects, ui, music, muted, unfocused)")),
            }
        }
        update_settings(|s| *s = changed);
    }
    if let Some(name) = opt_str(cmd, "play")? {
        let cue = Cue::parse(name).ok_or_else(|| format!("unknown cue {name:?}; one of {}", Cue::ALL.map(|c| c.key()).join(", ")))?;
        crate::audio::cue(cue);
    }
    Ok(readout(ctx))
}

fn readout(ctx: &Ctx) -> Value {
    let s = settings();
    let sound = &ctx.sound;
    let t = &sound.handle.telemetry;
    let status = match &sound.handle.status {
        Status::Playing { device, sample_rate, channels } => json!({ "playing": true, "device": device, "sample_rate": sample_rate, "channels": channels }),
        Status::Silent(why) => json!({ "playing": false, "why": why }),
    };
    let db = |x: f32| (nd_audio::dsp::gain_to_db(x) * 10.0).round() / 10.0;
    let r3 = |x: f32| (x * 1000.0).round() / 1000.0;
    let b = &sound.beds;
    let h = &sound.heard;
    json!({
        "settings": { "master": s.master, "ambience": s.ambience, "effects": s.effects, "ui": s.ui, "music": s.music, "muted": s.muted, "unfocused": s.unfocused.key() },
        "status": status,
        "focused": sound.focused,
        "bus_gains": nd_audio::Bus::ALL.iter().map(|b| (b.key(), r3(sound.mix.bus_gains()[b.index()]))).collect::<std::collections::BTreeMap<_, _>>(),
        "beds": {
            "weights": { "high": r3(load(&t.weights[0])), "mid": r3(load(&t.weights[1])), "street": r3(load(&t.weights[2])) },
            "level_db": { "high": db(load(&t.beds[0])), "mid": db(load(&t.beds[1])), "street": db(load(&t.beds[2])), "weather_nature": db(load(&t.beds[3])) },
            "target": {
                "wash": r3(b.wash), "wind": r3(b.wind), "traffic": r3(b.traffic), "traffic_tone": r3(b.traffic_tone), "tyres": r3(b.tyres), "passby_rate": r3(b.passby_rate),
                "murmur": r3(b.murmur), "steps_rate": r3(b.steps_rate), "hum": r3(b.hum), "neon": r3(b.neon), "birds": r3(b.birds), "crickets": r3(b.crickets),
                "rain": r3(b.rain), "water": r3(b.water), "muffle": r3(b.muffle), "level": r3(b.level),
            },
        },
        "heard": {
            "height_m": r3(h.height), "radius_m": r3(h.radius), "cars_close": h.cars_close, "cars_near": h.cars_near, "car_speed_ms": r3(h.car_speed),
            "walkers_close": h.walkers_close, "walkers_near": h.walkers_near, "density": r3(h.density), "neon": r3(h.neon), "green": r3(h.green),
            "industry": r3(h.industry), "water": r3(h.water), "daylight": r3(h.daylight), "rain": r3(h.rain), "snow": r3(h.snow), "wind": r3(h.wind), "underground": h.underground,
        },
        "voices": { "ui": t.ui_voices.load(std::sync::atomic::Ordering::Relaxed), "spot": t.spot_voices.load(std::sync::atomic::Ordering::Relaxed), "ui_cap": nd_audio::engine::UI_VOICES, "spot_cap": nd_audio::engine::SPOT_VOICES },
        "cues_played": sound.played,
        "output_peak_db": db(load(&t.peak)),
        "limiter_gain": r3(load(&t.limiter)),
        "audio_thread_cpu_pct": (load(&t.cpu) * 1000.0).round() / 10.0,
        "frames_rendered": t.frames.load(std::sync::atomic::Ordering::Relaxed),
    })
}

/// `audio_capture {path?, seconds?}`: what the camera hears now (the beds as
/// last sampled, at the default sliders or with `as_set` the player's,
/// without focus ducking) rendered
/// offline to a 48 kHz WAV, with its levels.
pub(super) fn capture(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let secs = opt_f32(cmd, "seconds")?.unwrap_or(6.0);
    if !(0.5..=60.0).contains(&secs) {
        return Err("seconds must be 0.5..60".into());
    }
    let default = format!("exports/wav/capture-{}.wav", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()));
    let path = super::files::allowed_path(opt_str(cmd, "path")?.unwrap_or(&default), "wav", true)?;
    // The default sliders unless asked for the player's, so captures compare.
    let mix = if opt_bool(cmd, "as_set")?.unwrap_or(false) { settings().mix(true) } else { nd_audio::MixLevels::default() };
    let out = nd_audio::wav::render_beds(ctx.sound.beds, nd_audio::MixLevels { muted: false, ..mix }, secs);
    nd_audio::wav::write_wav(&path, &out, nd_audio::wav::SR).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let a = nd_audio::wav::analyse(&out, nd_audio::wav::SR);
    Ok(json!({
        "path": path.to_string_lossy().replace('\\', "/"),
        "seconds": secs,
        "peak_db": a.peak_db, "rms_db": a.rms_db, "dc": a.dc, "clipped": a.clipped, "correlation": a.correlation,
        "bands_db": nd_audio::wav::BANDS.iter().zip(a.bands).map(|(hz, v)| json!({ "hz": hz, "db": (v * 10.0).round() / 10.0 })).collect::<Vec<_>>(),
    }))
}
