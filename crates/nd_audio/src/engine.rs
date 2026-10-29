//! The engine: the beds, the UI voices and (later) the spot voices mixed
//! into the buses, then a DC blocker and the master limiter. It runs in the
//! sound card's callback, fed by [`Command`]s; the same code renders WAVs
//! offline ([`Engine::render`] on a buffer).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::beds::Beds;
use crate::cues::{Cue, Voice};
use crate::dsp::{dc_pole, one_pole, DcBlock, Rng, Smooth};
use crate::mix::{Bus, Limiter, MixLevels};
use crate::scene::BedParams;

/// What the game tells the engine.
#[derive(Clone, Copy, Debug)]
pub enum Command {
    /// New slider positions, mute or ducking.
    Mix(MixLevels),
    /// New ambience parameters.
    Beds(BedParams),
    /// Play a UI cue at this gain (from the rate limiter).
    Cue(Cue, f32),
}

/// At most this many UI cues sound at once; a new one takes the place of
/// the quietest.
pub const UI_VOICES: usize = 8;
/// [sound] Phase 2's spot sounds (trains, sirens, horns) will share a cap
/// like this one, loudest kept, each at [`distance_gain`] for its
/// distance from the listener.
pub const SPOT_VOICES: usize = 20;

/// A spot sound's gain at `d` metres: full inside `near`, then falling off
/// as `near / d` (the inverse distance law, -6 dB a doubling), silent past
/// `far` (eased out over its last quarter so it never pops off).
pub fn distance_gain(d: f32, near: f32, far: f32) -> f32 {
    let d = d.max(0.0);
    if d >= far {
        return 0.0;
    }
    let law = if d <= near { 1.0 } else { near / d };
    let edge = far * 0.75;
    let fade = if d > edge { 1.0 - (d - edge) / (far - edge) } else { 1.0 };
    law * fade
}

/// A pool of voices with a cap: when full, the quietest gives way.
#[derive(Debug)]
pub struct VoicePool<V> {
    voices: Vec<V>,
    cap: usize,
}

impl<V> VoicePool<V> {
    pub fn new(cap: usize) -> Self {
        Self { voices: Vec::with_capacity(cap), cap }
    }

    pub fn len(&self) -> usize {
        self.voices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Add `v`, stealing the voice with the least `loudness` when full.
    pub fn push(&mut self, v: V, loudness: impl Fn(&V) -> f32) {
        if self.voices.len() < self.cap {
            self.voices.push(v);
        } else if let Some((i, _)) = self.voices.iter().enumerate().min_by(|a, b| loudness(a.1).total_cmp(&loudness(b.1))) {
            self.voices[i] = v;
        }
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, V> {
        self.voices.iter_mut()
    }

    pub fn retain(&mut self, keep: impl FnMut(&V) -> bool) {
        self.voices.retain(keep);
    }
}

/// What the engine reports back, readable from any thread.
#[derive(Debug, Default)]
pub struct Telemetry {
    /// RMS of each bed group (high, mid, street, weather/nature) before the
    /// bus gain, as f32 bits.
    pub beds: [AtomicU32; 4],
    /// The beds' crossfade weights now (high, mid, street), f32 bits.
    pub weights: [AtomicU32; 3],
    /// UI voices sounding.
    pub ui_voices: AtomicU32,
    /// Spot voices sounding (phase 2; 0 for now).
    pub spot_voices: AtomicU32,
    /// The callback's share of the time it is given (render time over the
    /// buffer's duration), smoothed, f32 bits.
    pub cpu: AtomicU32,
    /// The output peak over the last half second, f32 bits.
    pub peak: AtomicU32,
    /// The limiter's gain reduction now (1 = none), f32 bits.
    pub limiter: AtomicU32,
    /// Frames rendered.
    pub frames: AtomicU32,
    /// The sound card went away (unplugged): the stream needs a restart.
    pub lost: AtomicBool,
}

pub fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

/// The mixer and everything in it.
pub struct Engine {
    sr: f32,
    beds: Beds,
    ui: VoicePool<Voice>,
    mix: MixLevels,
    /// The buses' gains eased towards `mix` (no clicks when a slider moves
    /// or the window loses focus).
    gains: [Smooth; 4],
    gain_coef: f32,
    limiter: Limiter,
    dc: [DcBlock; 2],
    dc_r: f32,
    rng: Rng,
    scratch: Vec<f32>,
    telemetry: Arc<Telemetry>,
    peak: f32,
    peak_frames: u32,
    cpu: f32,
}

impl Engine {
    pub fn new(sr: f32, telemetry: Arc<Telemetry>) -> Self {
        Self {
            sr,
            beds: Beds::new(sr, 0x0c17_7a11),
            ui: VoicePool::new(UI_VOICES),
            mix: MixLevels::default(),
            gains: [Smooth::new(0.0); 4],
            gain_coef: one_pole(0.08, sr),
            limiter: Limiter::new(sr),
            dc: Default::default(),
            dc_r: dc_pole(sr),
            rng: Rng::new(0x51ab_c0de),
            scratch: Vec::new(),
            telemetry,
            peak: 0.0,
            peak_frames: 0,
            cpu: 0.0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Mix(m) => self.mix = m,
            Command::Beds(p) => self.beds.set(p),
            Command::Cue(cue, gain) => {
                if !self.mix.muted {
                    let v = Voice::new(cue, gain, &mut self.rng);
                    self.ui.push(v, Voice::loudness);
                }
            }
        }
    }

    /// Jump the beds and bus gains to their targets (offline renders).
    pub fn settle(&mut self) {
        self.beds.settle();
        for (g, t) in self.gains.iter_mut().zip(self.mix.bus_gains()) {
            g.v = t;
        }
    }

    /// The beds' parameters as eased now.
    pub fn beds_now(&self) -> &BedParams {
        self.beds.current()
    }

    /// The callback's share of its time budget, as last measured.
    pub fn note_cpu(&mut self, used: f32) {
        self.cpu += 0.05 * (used - self.cpu);
        store(&self.telemetry.cpu, self.cpu);
    }

    /// Render `out` (interleaved stereo frames), replacing what is there.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let targets = self.mix.bus_gains();
        // Ease the bus gains once per 32 frames (cheap and smooth enough
        // at 0.08 s): one value per chunk, ramped linearly inside it.
        let chunk = 32;
        let mut start = 0;
        let silent = targets.iter().all(|&t| t == 0.0) && self.gains.iter().all(|g| g.v < 1e-5);
        if silent {
            // Muted and faded: skip the work, keep the voices from piling up.
            self.ui.retain(|_| false);
            for g in &mut self.gains {
                g.v = 0.0;
            }
            self.publish(0.0, frames);
            return;
        }
        let coef = 1.0 - (1.0 - self.gain_coef).powi(chunk as i32);
        if self.scratch.len() < chunk * 2 {
            self.scratch.resize(chunk * 2, 0.0);
        }
        let mut peak = 0.0f32;
        while start < frames {
            let n = chunk.min(frames - start);
            let mut g = [0.0; 4];
            for (i, s) in self.gains.iter_mut().enumerate() {
                g[i] = s.step(targets[i], coef);
            }
            let block = &mut out[start * 2..(start + n) * 2];
            let amb = g[Bus::Ambience.index()];
            if amb > 1e-6 {
                self.beds.render(block, amb);
            }
            let ui = g[Bus::Ui.index()];
            let sr = self.sr;
            for v in self.ui.iter_mut() {
                v.render(block, ui, sr);
            }
            for f in block.chunks_exact_mut(2) {
                // DC out first: after the limiter it could push a peak past it.
                let l = self.dc[0].run(f[0], self.dc_r);
                let r = self.dc[1].run(f[1], self.dc_r);
                let (l, r) = self.limiter.run(l, r);
                f[0] = l;
                f[1] = r;
                peak = peak.max(l.abs()).max(r.abs());
            }
            start += n;
        }
        self.ui.retain(|v| !v.done());
        self.publish(peak, frames);
    }

    fn publish(&mut self, peak: f32, frames: usize) {
        let t = &self.telemetry;
        self.peak = self.peak.max(peak);
        self.peak_frames += frames as u32;
        if self.peak_frames as f32 > self.sr * 0.5 {
            store(&t.peak, self.peak);
            self.peak = 0.0;
            self.peak_frames = 0;
            let m = self.beds.take_meters();
            for (a, v) in t.beds.iter().zip(m) {
                store(a, v);
            }
            let p = self.beds.current();
            for (a, v) in t.weights.iter().zip([p.high, p.mid, p.street]) {
                store(a, v);
            }
        }
        store(&t.limiter, self.limiter.gain);
        t.ui_voices.store(self.ui.len() as u32, Ordering::Relaxed);
        t.frames.fetch_add(frames as u32, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{bed_params, Surroundings};

    #[test]
    fn distance_falls_off_and_ends_at_far() {
        assert_eq!(distance_gain(0.0, 10.0, 400.0), 1.0);
        assert_eq!(distance_gain(10.0, 10.0, 400.0), 1.0);
        assert!((distance_gain(20.0, 10.0, 400.0) - 0.5).abs() < 1e-6);
        assert!(distance_gain(350.0, 10.0, 400.0) < distance_gain(299.0, 10.0, 400.0));
        assert_eq!(distance_gain(400.0, 10.0, 400.0), 0.0);
        let mut last = 2.0;
        for d in 0..450 {
            let g = distance_gain(d as f32, 10.0, 400.0);
            assert!(g <= last);
            last = g;
        }
    }

    #[test]
    fn the_pool_keeps_its_cap_and_steals_the_quietest() {
        let mut pool = VoicePool::new(3);
        for x in [0.5f32, 0.2, 0.9] {
            pool.push(x, |v| *v);
        }
        pool.push(0.7, |v| *v);
        let mut v: Vec<f32> = pool.iter_mut().map(|x| *x).collect();
        v.sort_by(f32::total_cmp);
        assert_eq!(v, vec![0.5, 0.7, 0.9]);
    }

    #[test]
    fn muted_renders_silence_and_drops_cues() {
        let t = Arc::new(Telemetry::default());
        let mut e = Engine::new(48_000.0, t.clone());
        e.apply(Command::Beds(bed_params(&Surroundings { height: 3.0, cars_close: 10.0, ..Default::default() })));
        e.apply(Command::Mix(MixLevels { muted: true, ..Default::default() }));
        e.apply(Command::Cue(Cue::Alert, 1.0));
        let mut out = vec![1.0; 4_800];
        e.render(&mut out);
        assert!(out.iter().all(|x| *x == 0.0));
        assert_eq!(t.ui_voices.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_storm_of_cues_on_loud_beds_stays_under_full_scale_with_no_dc() {
        let t = Arc::new(Telemetry::default());
        let mut e = Engine::new(48_000.0, t.clone());
        e.apply(Command::Mix(MixLevels { master: 1.0, ambience: 1.0, ui: 1.0, ..Default::default() }));
        e.apply(Command::Beds(bed_params(&Surroundings { height: 2.0, cars_close: 40.0, cars_near: 200.0, car_speed: 20.0, walkers_close: 60.0, neon: 1.0, density: 1.0, daylight: 0.0, rain: 1.0, wind: 1.0, water: 1.0, ..Default::default() })));
        e.settle();
        let mut out = vec![0.0; 2 * 48_000 * 3];
        for chunk in out.chunks_mut(960) {
            for cue in Cue::ALL {
                e.apply(Command::Cue(cue, 1.0));
            }
            e.render(chunk);
        }
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let dc = out.iter().sum::<f32>() / out.len() as f32;
        assert!(peak < 1.0, "{peak}");
        assert!(dc.abs() < 1e-3, "{dc}");
        assert!(t.ui_voices.load(Ordering::Relaxed) as usize <= UI_VOICES);
    }
}
