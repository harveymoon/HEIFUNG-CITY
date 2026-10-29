//! The mixer: the buses, the player's sliders turned into gains, and the
//! master limiter that keeps the sum sensible.

use crate::dsp::{one_pole, soft_clip_to};

/// The buses everything is mixed into before the master.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    /// The three beds, weather, nature and water.
    Ambience,
    /// Spot sounds from the city (phase 2: trains, sirens, horns).
    Effects,
    /// Clicks, tools, windows, alerts.
    Ui,
    /// Generative music (phase 3); silent for now.
    Music,
}

impl Bus {
    pub const ALL: [Bus; 4] = [Bus::Ambience, Bus::Effects, Bus::Ui, Bus::Music];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            Bus::Ambience => "ambience",
            Bus::Effects => "effects",
            Bus::Ui => "ui",
            Bus::Music => "music",
        }
    }
}

/// What the player set: each slider `0..1`, mute, and how far the sound is
/// ducked now (1 = not at all; 0 = silent, the window out of focus).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixLevels {
    pub master: f32,
    pub ambience: f32,
    pub effects: f32,
    pub ui: f32,
    pub music: f32,
    pub muted: bool,
    pub duck: f32,
}

impl Default for MixLevels {
    fn default() -> Self {
        Self { master: 0.8, ambience: 0.7, effects: 0.8, ui: 0.6, music: 0.5, muted: false, duck: 1.0 }
    }
}

/// A slider position to a gain: squared, so half way is -12 dB and the
/// bottom of the travel fades smoothly to silence (a linear slider spends
/// most of its travel sounding loud).
pub fn slider_gain(v: f32) -> f32 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    v * v
}

impl MixLevels {
    fn bus_slider(&self, bus: Bus) -> f32 {
        match bus {
            Bus::Ambience => self.ambience,
            Bus::Effects => self.effects,
            Bus::Ui => self.ui,
            Bus::Music => self.music,
        }
    }

    /// The gain each bus is mixed at, master, mute and ducking included.
    pub fn bus_gains(&self) -> [f32; 4] {
        if self.muted {
            return [0.0; 4];
        }
        let master = slider_gain(self.master) * self.duck.clamp(0.0, 1.0);
        Bus::ALL.map(|b| master * slider_gain(self.bus_slider(b)))
    }

    /// Nothing can be heard: the engine may stop rendering once faded.
    pub fn silent(&self) -> bool {
        self.bus_gains().iter().all(|&g| g <= 0.0)
    }
}

/// The master limiter: a peak follower that turns the sum down fast when
/// it nears full scale and lets go slowly, then a soft clip so nothing ever
/// passes 0.98 (-0.2 dBFS). Mostly idle: the beds and cues are mixed well under it.
#[derive(Clone, Debug)]
pub struct Limiter {
    env: f32,
    attack: f32,
    release: f32,
    /// Where it starts turning down (linear peak).
    pub threshold: f32,
    /// The gain it applied last (for the readout).
    pub gain: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        Self { env: 0.0, attack: one_pole(0.0015, sr), release: one_pole(0.25, sr), threshold: 0.7, gain: 1.0 }
    }

    /// One stereo frame through the limiter.
    #[inline]
    pub fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        let c = if peak > self.env { self.attack } else { self.release };
        self.env += c * (peak - self.env);
        let g = if self.env > self.threshold { self.threshold / self.env } else { 1.0 };
        self.gain = g;
        (soft_clip_to(l * g, 0.85, 0.98), soft_clip_to(r * g, 0.85, 0.98))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliders_map_to_gains_that_climb_from_silence_to_unity() {
        assert_eq!(slider_gain(0.0), 0.0);
        assert_eq!(slider_gain(1.0), 1.0);
        assert!((slider_gain(0.5) - 0.25).abs() < 1e-6);
        assert_eq!(slider_gain(-1.0), 0.0);
        assert_eq!(slider_gain(2.0), 1.0);
        assert_eq!(slider_gain(f32::NAN), 0.0);
        let mut last = -1.0;
        for i in 0..=100 {
            let g = slider_gain(i as f32 / 100.0);
            assert!(g > last);
            last = g;
        }
    }

    #[test]
    fn bus_gains_multiply_master_bus_and_duck_and_mute_silences() {
        let m = MixLevels { master: 1.0, ambience: 0.5, effects: 1.0, ui: 0.0, music: 1.0, muted: false, duck: 1.0 };
        let g = m.bus_gains();
        assert!((g[Bus::Ambience.index()] - 0.25).abs() < 1e-6);
        assert_eq!(g[Bus::Effects.index()], 1.0);
        assert_eq!(g[Bus::Ui.index()], 0.0);
        let half = MixLevels { master: 0.5, ..m }.bus_gains();
        assert!((half[Bus::Effects.index()] - 0.25).abs() < 1e-6);
        let ducked = MixLevels { duck: 0.2, ..m }.bus_gains();
        assert!((ducked[Bus::Effects.index()] - 0.2).abs() < 1e-6);
        assert_eq!(MixLevels { muted: true, ..m }.bus_gains(), [0.0; 4]);
        assert!(MixLevels { muted: true, ..m }.silent());
        assert!(MixLevels { master: 0.0, ..m }.silent());
        assert!(!m.silent());
    }

    #[test]
    fn the_limiter_holds_a_loud_sum_under_full_scale() {
        let sr = 48_000.0;
        let mut lim = Limiter::new(sr);
        let mut peak: f32 = 0.0;
        for i in 0..48_000 {
            let x = 3.0 * (i as f32 * 0.05).sin();
            let (l, r) = lim.run(x, -x);
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 1.0, "{peak}");
        // A quiet signal passes untouched once the envelope has let go.
        let mut lim = Limiter::new(sr);
        let (l, _) = lim.run(0.1, 0.1);
        assert_eq!(l, 0.1);
    }
}
