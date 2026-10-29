//! The UI sounds: what each cue is made of (a small patch of oscillators
//! and noise through one filter), the voice that plays it, and the rate
//! limiter that keeps drags and repeats from machine-gunning.
//!
//! They are meant quiet and short, a little analogue: soft attacks, a
//! touch of drive, a filter on everything, a random detune of a percent or
//! so per trigger so repeats are not identical.

use crate::dsp::*;

/// A UI sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// A button, tab or checkbox clicked.
    Click,
    /// Another tool picked.
    ToolChange,
    /// A point of a road (or path, rail, line) placed.
    RoadPoint,
    /// A road finished (the gesture went into the project).
    RoadDone,
    /// The project implemented: roads and zones built.
    Implement,
    /// A building put down.
    BuildingPlaced,
    /// Something bulldozed.
    Bulldoze,
    /// Zones painted.
    ZonePaint,
    WindowOpen,
    WindowClose,
    /// A click refused (cannot build here, implement refused).
    Error,
    /// A toast popped up.
    Alert,
    SpeedUp,
    SpeedDown,
    Pause,
    Resume,
}

impl Cue {
    pub const ALL: [Cue; 16] = [
        Cue::Click,
        Cue::ToolChange,
        Cue::RoadPoint,
        Cue::RoadDone,
        Cue::Implement,
        Cue::BuildingPlaced,
        Cue::Bulldoze,
        Cue::ZonePaint,
        Cue::WindowOpen,
        Cue::WindowClose,
        Cue::Error,
        Cue::Alert,
        Cue::SpeedUp,
        Cue::SpeedDown,
        Cue::Pause,
        Cue::Resume,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Cue::Click => "click",
            Cue::ToolChange => "tool_change",
            Cue::RoadPoint => "road_point",
            Cue::RoadDone => "road_done",
            Cue::Implement => "implement",
            Cue::BuildingPlaced => "building_placed",
            Cue::Bulldoze => "bulldoze",
            Cue::ZonePaint => "zone_paint",
            Cue::WindowOpen => "window_open",
            Cue::WindowClose => "window_close",
            Cue::Error => "error",
            Cue::Alert => "alert",
            Cue::SpeedUp => "speed_up",
            Cue::SpeedDown => "speed_down",
            Cue::Pause => "pause",
            Cue::Resume => "resume",
        }
    }

    pub fn parse(s: &str) -> Option<Cue> {
        Cue::ALL.into_iter().find(|c| c.key().eq_ignore_ascii_case(s.trim()))
    }

    fn index(self) -> usize {
        self as usize
    }

    /// The shortest gap between two of this cue, seconds.
    pub fn min_gap(self) -> f64 {
        match self {
            Cue::Click => 0.06,
            Cue::ToolChange => 0.08,
            Cue::RoadPoint => 0.07,
            Cue::RoadDone => 0.12,
            Cue::Implement => 0.35,
            Cue::BuildingPlaced => 0.09,
            Cue::Bulldoze => 0.14,
            Cue::ZonePaint => 0.13,
            Cue::WindowOpen | Cue::WindowClose => 0.07,
            Cue::Error => 0.25,
            Cue::Alert => 0.6,
            Cue::SpeedUp | Cue::SpeedDown => 0.06,
            Cue::Pause | Cue::Resume => 0.15,
        }
    }

    /// The patch it plays.
    pub fn patch(self) -> &'static Patch {
        match self {
            Cue::Click => &CLICK,
            Cue::ToolChange => &TOOL,
            Cue::RoadPoint => &ROAD_POINT,
            Cue::RoadDone => &ROAD_DONE,
            Cue::Implement => &IMPLEMENT,
            Cue::BuildingPlaced => &BUILDING,
            Cue::Bulldoze => &BULLDOZE,
            Cue::ZonePaint => &ZONE,
            Cue::WindowOpen => &WINDOW_OPEN,
            Cue::WindowClose => &WINDOW_CLOSE,
            Cue::Error => &ERROR,
            Cue::Alert => &ALERT,
            Cue::SpeedUp => &SPEED_UP,
            Cue::SpeedDown => &SPEED_DOWN,
            Cue::Pause => &PAUSE,
            Cue::Resume => &RESUME,
        }
    }
}

/// An oscillator shape.
#[derive(Clone, Copy, Debug)]
pub enum Wave {
    Sine,
    Tri,
    Saw,
    /// A pulse of this duty.
    Pulse(f32),
    Noise,
}

/// One oscillator (or noise) of a patch with its own pitch glide and
/// envelope: `delay` before it starts, a linear `attack`, then an
/// exponential decay of time constant `decay`, cut (with a short fade) at
/// `len` seconds after it starts.
#[derive(Clone, Copy, Debug)]
pub struct Partial {
    pub wave: Wave,
    pub f0: f32,
    pub f1: f32,
    pub glide: f32,
    pub amp: f32,
    pub delay: f32,
    pub attack: f32,
    pub decay: f32,
    pub len: f32,
}

const fn p(wave: Wave, f0: f32, f1: f32, glide: f32, amp: f32, delay: f32, attack: f32, decay: f32, len: f32) -> Partial {
    Partial { wave, f0, f1, glide, amp, delay, attack, decay, len }
}

#[derive(Clone, Copy, Debug)]
pub enum Filter {
    Lp,
    Bp,
    Hp,
}

/// A UI sound: partials summed, through one filter whose cutoff glides
/// from `cut0` to `cut1`, a little drive, a gain and a place in the stereo.
#[derive(Debug)]
pub struct Patch {
    pub partials: &'static [Partial],
    pub filter: Filter,
    pub cut0: f32,
    pub cut1: f32,
    pub cut_glide: f32,
    pub q: f32,
    pub drive: f32,
    pub gain: f32,
    pub pan: f32,
}

impl Patch {
    /// How long it sounds, seconds.
    pub fn length(&self) -> f32 {
        self.partials.iter().map(|p| p.delay + p.len).fold(0.0, f32::max)
    }
}

use Wave::*;

static CLICK: Patch = Patch {
    partials: &[p(Noise, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0006, 0.004, 0.03), p(Sine, 1_850.0, 1_700.0, 0.02, 0.28, 0.0, 0.0008, 0.012, 0.045)],
    filter: Filter::Bp,
    cut0: 3_000.0,
    cut1: 2_400.0,
    cut_glide: 0.03,
    q: 0.9,
    drive: 0.3,
    gain: 0.28,
    pan: 0.0,
};

static TOOL: Patch = Patch {
    partials: &[p(Tri, 520.0, 540.0, 0.04, 0.5, 0.0, 0.003, 0.05, 0.12), p(Sine, 780.0, 790.0, 0.04, 0.42, 0.045, 0.003, 0.06, 0.14)],
    filter: Filter::Lp,
    cut0: 2_800.0,
    cut1: 1_800.0,
    cut_glide: 0.15,
    q: 0.7,
    drive: 0.2,
    gain: 0.22,
    pan: 0.0,
};

static ROAD_POINT: Patch = Patch {
    partials: &[p(Sine, 920.0, 610.0, 0.03, 0.62, 0.0, 0.001, 0.035, 0.1), p(Noise, 0.0, 0.0, 0.0, 0.22, 0.0, 0.0005, 0.006, 0.03)],
    filter: Filter::Lp,
    cut0: 3_500.0,
    cut1: 2_200.0,
    cut_glide: 0.06,
    q: 0.7,
    drive: 0.3,
    gain: 0.24,
    pan: 0.0,
};

static ROAD_DONE: Patch = Patch {
    partials: &[p(Sine, 610.0, 700.0, 0.03, 0.55, 0.0, 0.001, 0.04, 0.1), p(Sine, 820.0, 915.0, 0.03, 0.55, 0.07, 0.001, 0.06, 0.17), p(Tri, 1_640.0, 1_830.0, 0.03, 0.08, 0.07, 0.001, 0.04, 0.12)],
    filter: Filter::Lp,
    cut0: 3_200.0,
    cut1: 2_400.0,
    cut_glide: 0.15,
    q: 0.7,
    drive: 0.3,
    gain: 0.24,
    pan: 0.0,
};

static IMPLEMENT: Patch = Patch {
    partials: &[
        p(Saw, 220.0, 220.0, 0.0, 0.3, 0.0, 0.02, 0.35, 0.75),
        p(Saw, 277.6, 277.2, 0.0, 0.24, 0.0, 0.025, 0.35, 0.75),
        p(Saw, 329.2, 329.6, 0.0, 0.24, 0.0, 0.03, 0.35, 0.75),
        p(Saw, 440.9, 440.9, 0.0, 0.15, 0.05, 0.03, 0.3, 0.7),
        p(Sine, 880.0, 880.0, 0.0, 0.2, 0.09, 0.01, 0.3, 0.66),
    ],
    filter: Filter::Lp,
    cut0: 380.0,
    cut1: 3_000.0,
    cut_glide: 0.26,
    q: 1.2,
    drive: 0.6,
    gain: 0.2,
    pan: 0.0,
};

static BUILDING: Patch = Patch {
    partials: &[
        p(Sine, 140.0, 55.0, 0.09, 0.9, 0.0, 0.001, 0.09, 0.3),
        p(Noise, 0.0, 0.0, 0.0, 0.3, 0.0, 0.0005, 0.01, 0.05),
        p(Tri, 330.0, 320.0, 0.05, 0.22, 0.02, 0.002, 0.05, 0.13),
    ],
    filter: Filter::Lp,
    cut0: 1_900.0,
    cut1: 800.0,
    cut_glide: 0.15,
    q: 0.7,
    drive: 0.4,
    gain: 0.2,
    pan: 0.0,
};

static BULLDOZE: Patch = Patch {
    partials: &[p(Noise, 0.0, 0.0, 0.0, 1.0, 0.0, 0.005, 0.12, 0.38), p(Saw, 92.0, 46.0, 0.25, 0.35, 0.0, 0.004, 0.15, 0.38), p(Noise, 0.0, 0.0, 0.0, 0.4, 0.06, 0.002, 0.03, 0.12)],
    filter: Filter::Lp,
    cut0: 2_600.0,
    cut1: 340.0,
    cut_glide: 0.3,
    q: 1.5,
    drive: 1.2,
    gain: 0.25,
    pan: 0.0,
};

static ZONE: Patch = Patch {
    partials: &[p(Noise, 0.0, 0.0, 0.0, 1.0, 0.0, 0.025, 0.05, 0.17)],
    filter: Filter::Bp,
    cut0: 850.0,
    cut1: 2_500.0,
    cut_glide: 0.13,
    q: 1.8,
    drive: 0.0,
    gain: 0.2,
    pan: 0.0,
};

static WINDOW_OPEN: Patch = Patch {
    partials: &[p(Sine, 420.0, 680.0, 0.07, 0.5, 0.0, 0.004, 0.06, 0.15), p(Tri, 840.0, 1_360.0, 0.07, 0.14, 0.0, 0.004, 0.05, 0.13)],
    filter: Filter::Lp,
    cut0: 2_200.0,
    cut1: 3_400.0,
    cut_glide: 0.08,
    q: 0.7,
    drive: 0.2,
    gain: 0.18,
    pan: 0.0,
};

static WINDOW_CLOSE: Patch = Patch {
    partials: &[p(Sine, 680.0, 420.0, 0.07, 0.5, 0.0, 0.004, 0.05, 0.13), p(Tri, 1_360.0, 840.0, 0.07, 0.12, 0.0, 0.004, 0.04, 0.11)],
    filter: Filter::Lp,
    cut0: 3_000.0,
    cut1: 1_800.0,
    cut_glide: 0.08,
    q: 0.7,
    drive: 0.2,
    gain: 0.16,
    pan: 0.0,
};

static ERROR: Patch = Patch {
    partials: &[p(Pulse(0.4), 233.0, 226.0, 0.08, 0.38, 0.0, 0.004, 0.07, 0.11), p(Pulse(0.4), 196.0, 190.0, 0.1, 0.38, 0.12, 0.004, 0.09, 0.17)],
    filter: Filter::Lp,
    cut0: 950.0,
    cut1: 650.0,
    cut_glide: 0.25,
    q: 0.9,
    drive: 0.5,
    gain: 0.3,
    pan: 0.0,
};

static ALERT: Patch = Patch {
    partials: &[
        p(Sine, 1_046.5, 1_046.5, 0.0, 0.45, 0.0, 0.002, 0.33, 0.9),
        p(Sine, 2_888.0, 2_888.0, 0.0, 0.1, 0.0, 0.001, 0.1, 0.4),
        p(Sine, 1_568.0, 1_568.0, 0.0, 0.3, 0.09, 0.002, 0.3, 0.8),
        p(Sine, 4_327.0, 4_327.0, 0.0, 0.05, 0.09, 0.001, 0.08, 0.3),
    ],
    filter: Filter::Lp,
    cut0: 5_000.0,
    cut1: 3_600.0,
    cut_glide: 0.5,
    q: 0.7,
    drive: 0.1,
    gain: 0.17,
    pan: 0.0,
};

static SPEED_UP: Patch = Patch {
    partials: &[p(Tri, 700.0, 930.0, 0.04, 0.5, 0.0, 0.002, 0.04, 0.09)],
    filter: Filter::Lp,
    cut0: 3_000.0,
    cut1: 3_000.0,
    cut_glide: 0.0,
    q: 0.7,
    drive: 0.2,
    gain: 0.2,
    pan: 0.0,
};

static SPEED_DOWN: Patch = Patch {
    partials: &[p(Tri, 930.0, 700.0, 0.04, 0.5, 0.0, 0.002, 0.04, 0.09)],
    filter: Filter::Lp,
    cut0: 3_000.0,
    cut1: 3_000.0,
    cut_glide: 0.0,
    q: 0.7,
    drive: 0.2,
    gain: 0.2,
    pan: 0.0,
};

static PAUSE: Patch = Patch {
    partials: &[p(Saw, 330.0, 105.0, 0.22, 0.32, 0.0, 0.003, 0.2, 0.3), p(Saw, 331.5, 105.6, 0.22, 0.2, 0.0, 0.003, 0.2, 0.3)],
    filter: Filter::Lp,
    cut0: 2_000.0,
    cut1: 300.0,
    cut_glide: 0.25,
    q: 0.9,
    drive: 0.4,
    gain: 0.17,
    pan: 0.0,
};

static RESUME: Patch = Patch {
    partials: &[p(Saw, 110.0, 330.0, 0.18, 0.32, 0.0, 0.01, 0.2, 0.28), p(Saw, 110.6, 331.5, 0.18, 0.2, 0.0, 0.01, 0.2, 0.28)],
    filter: Filter::Lp,
    cut0: 300.0,
    cut1: 2_200.0,
    cut_glide: 0.2,
    q: 0.9,
    drive: 0.4,
    gain: 0.17,
    pan: 0.0,
};

/// The most partials a patch has.
const MAX_PARTIALS: usize = 6;
/// Frames between filter cutoff updates in a voice.
const VOICE_BLOCK: u32 = 16;
/// The cues' common trim, set from the offline renders: at the default
/// sliders the cues peak around -20 dBFS, over beds at -32..-45 dB RMS.
const UI_TRIM: f32 = 3.5;
/// The fade at the end of each partial, seconds (no click on the cut).
const TAIL_FADE: f32 = 0.006;

/// A cue being played.
#[derive(Clone, Debug)]
pub struct Voice {
    patch: &'static Patch,
    t: f32,
    len: f32,
    gain: f32,
    detune: f32,
    phases: [Phase; MAX_PARTIALS],
    /// Each partial's pitch, updated with the filter every [`VOICE_BLOCK`] frames.
    hz: [f32; MAX_PARTIALS],
    filter: [Svf; 2],
    frame: u32,
    noise: Rng,
}

impl Voice {
    pub fn new(cue: Cue, gain: f32, rng: &mut Rng) -> Self {
        let patch = cue.patch();
        Self {
            patch,
            t: 0.0,
            len: patch.length() + 0.01,
            gain,
            detune: 1.0 + rng.range(-0.012, 0.012),
            phases: Default::default(),
            hz: [0.0; MAX_PARTIALS],
            filter: Default::default(),
            frame: 0,
            noise: Rng::new(rng.next_u32()),
        }
    }

    pub fn done(&self) -> bool {
        self.t >= self.len
    }

    /// How loud it still is (for stealing the quietest), roughly.
    pub fn loudness(&self) -> f32 {
        self.gain * (1.0 - self.t / self.len).max(0.0)
    }

    /// Mix it into `out` (interleaved stereo) at `bus` gain.
    pub fn render(&mut self, out: &mut [f32], bus: f32, sr: f32) {
        let patch = self.patch;
        let dt = 1.0 / sr;
        for frame in out.chunks_exact_mut(2) {
            if self.t >= self.len {
                break;
            }
            if self.frame % VOICE_BLOCK == 0 {
                let u = if patch.cut_glide > 0.0 { (self.t / patch.cut_glide).min(1.0) } else { 1.0 };
                let cut = patch.cut0 * (patch.cut1 / patch.cut0).powf(u) * self.detune;
                for f in &mut self.filter {
                    f.set(cut, patch.q, sr);
                }
                for (i, part) in patch.partials.iter().enumerate().take(MAX_PARTIALS) {
                    let tt = (self.t - part.delay).max(0.0);
                    let g = if part.glide > 0.0 { (tt / part.glide).min(1.0) } else { 1.0 };
                    self.hz[i] = part.f0 * (part.f1 / part.f0.max(1e-3)).powf(g) * self.detune;
                }
            }
            self.frame += 1;
            let mut x = 0.0;
            for (i, part) in patch.partials.iter().enumerate() {
                let tt = self.t - part.delay;
                if tt < 0.0 || tt >= part.len {
                    continue;
                }
                let env = if tt < part.attack { tt / part.attack } else { (-(tt - part.attack) / part.decay).exp() } * ((part.len - tt) / TAIL_FADE).min(1.0);
                let s = match part.wave {
                    Noise => self.noise.white(),
                    w => {
                        let hz = self.hz[i];
                        let ph = self.phases[i].tick(hz, sr);
                        match w {
                            Sine => sine(ph),
                            Tri => tri(ph),
                            Saw => saw(ph, hz / sr),
                            Pulse(d) => pulse(ph, hz / sr, d),
                            Noise => 0.0,
                        }
                    }
                };
                x += s * env * part.amp;
            }
            let y = match patch.filter {
                Filter::Lp => self.filter[0].lp(x),
                Filter::Bp => self.filter[0].bp(x),
                Filter::Hp => self.filter[0].hp(x),
            };
            let y = warm(y, patch.drive) * patch.gain * UI_TRIM * self.gain * bus;
            let (l, r) = pan(y, patch.pan);
            // Equal-power pan puts -3 dB in each side at the centre; give it back.
            frame[0] += l * std::f32::consts::SQRT_2;
            frame[1] += r * std::f32::consts::SQRT_2;
            self.t += dt;
        }
    }
}

/// Keeps cues from piling up: a shortest gap per cue, repeats of the same
/// cue played softer, at most [`RateLimiter::MAX_PER_SECOND`] cues a
/// second in all, a click swallowed when a cue that means more came with
/// it, and a toast's chime held back right after an error.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    last: [f64; Cue::ALL.len()],
    streak: [u32; Cue::ALL.len()],
    recent: Vec<f64>,
    last_meaningful: f64,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self { last: [f64::NEG_INFINITY; Cue::ALL.len()], streak: [0; Cue::ALL.len()], recent: Vec::new(), last_meaningful: f64::NEG_INFINITY }
    }
}

impl RateLimiter {
    pub const MAX_PER_SECOND: usize = 10;
    /// Within this of the last of the same cue, the next one is a repeat.
    const REPEAT_WINDOW: f64 = 0.45;

    /// Whether `cue` may play at `now` (seconds), and at what gain.
    pub fn allow(&mut self, cue: Cue, now: f64) -> Option<f32> {
        let i = cue.index();
        if now - self.last[i] < cue.min_gap() {
            return None;
        }
        if cue == Cue::Click && now - self.last_meaningful < 0.1 {
            return None;
        }
        if cue == Cue::Alert && now - self.last[Cue::Error.index()] < 0.5 {
            return None;
        }
        self.recent.retain(|&t| now - t < 1.0);
        if self.recent.len() >= Self::MAX_PER_SECOND {
            return None;
        }
        self.streak[i] = if now - self.last[i] < Self::REPEAT_WINDOW { self.streak[i] + 1 } else { 0 };
        self.last[i] = now;
        self.recent.push(now);
        if cue != Cue::Click {
            self.last_meaningful = now;
        }
        Some(0.78f32.powi(self.streak[i].min(4) as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_does_not_machine_gun() {
        let mut lim = RateLimiter::default();
        // Zone paint asked for every frame at 144 fps for two seconds.
        let played = (0..288).filter(|i| lim.allow(Cue::ZonePaint, *i as f64 / 144.0).is_some()).count();
        assert!((10..=16).contains(&played), "{played}");
        // A repeat plays softer, down to a floor.
        let mut lim = RateLimiter::default();
        let g: Vec<f32> = (0..8).filter_map(|i| lim.allow(Cue::RoadPoint, i as f64 * 0.2)).collect();
        assert_eq!(g[0], 1.0);
        assert!(g[1] < 1.0 && g[2] < g[1]);
        assert!(g.last().copied().unwrap() >= 0.3);
        // After a pause it is back to full.
        assert_eq!(lim.allow(Cue::RoadPoint, 10.0), Some(1.0));
    }

    #[test]
    fn at_most_ten_cues_a_second_and_clicks_give_way() {
        let mut lim = RateLimiter::default();
        let mut n = 0;
        for i in 0..100 {
            let cue = Cue::ALL[i % Cue::ALL.len()];
            if lim.allow(cue, i as f64 * 0.01).is_some() {
                n += 1;
            }
        }
        assert!(n <= RateLimiter::MAX_PER_SECOND, "{n}");
        let mut lim = RateLimiter::default();
        assert!(lim.allow(Cue::ToolChange, 5.0).is_some());
        assert!(lim.allow(Cue::Click, 5.0).is_none(), "the tool's own sound covers its button");
        assert!(lim.allow(Cue::Click, 5.5).is_some());
        assert!(lim.allow(Cue::Error, 7.0).is_some());
        assert!(lim.allow(Cue::Alert, 7.1).is_none(), "the refusal's toast has its error sound");
        assert!(lim.allow(Cue::Alert, 8.0).is_some());
    }

    #[test]
    fn every_cue_is_short_quiet_and_ends_in_silence() {
        let sr = 48_000.0;
        let mut rng = Rng::new(3);
        for cue in Cue::ALL {
            let len = cue.patch().length();
            assert!(len <= 1.0, "{cue:?} lasts {len}");
            let mut v = Voice::new(cue, 1.0, &mut rng);
            let mut out = vec![0.0f32; (sr as usize) * 2];
            v.render(&mut out, 1.0, sr);
            assert!(v.done(), "{cue:?}");
            let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.02 && peak < 0.5, "{cue:?} peak {peak}");
            let end = ((len + 0.011) * sr) as usize * 2;
            assert!(out[end..].iter().all(|x| *x == 0.0), "{cue:?} rings on");
            assert!(Cue::parse(cue.key()) == Some(cue));
        }
    }
}
