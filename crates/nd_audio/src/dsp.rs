//! The building blocks: noise, oscillators, filters, smoothers, the soft
//! clip. Everything runs per sample on `f32` and allocates nothing.

use std::f32::consts::{PI, TAU};

/// The engine's own noise and dice (xorshift32): render-side only, never
/// the sim's RNG, so sound can never move the simulation.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `0..1`.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// White noise in `-1..1`.
    #[inline]
    pub fn white(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Uniform in `lo..hi`.
    #[inline]
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// True with probability `p`.
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

/// Pink noise (Paul Kellet's economy filter on white): -3 dB an octave,
/// the colour of distant traffic and crowds.
#[derive(Clone, Debug, Default)]
pub struct Pink {
    b: [f32; 3],
}

impl Pink {
    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.b[0] = 0.99765 * self.b[0] + white * 0.0990460;
        self.b[1] = 0.96300 * self.b[1] + white * 0.2965164;
        self.b[2] = 0.57000 * self.b[2] + white * 1.0526913;
        (self.b[0] + self.b[1] + self.b[2] + white * 0.1848) * 0.25
    }
}

/// Brown (red) noise: white integrated with a leak, -6 dB an octave.
#[derive(Clone, Debug, Default)]
pub struct Brown {
    y: f32,
}

impl Brown {
    #[inline]
    pub fn next(&mut self, white: f32) -> f32 {
        self.y = 0.996 * self.y + 0.06 * white;
        self.y * 1.6
    }
}

/// One-pole smoother towards a target. `coef` from [`one_pole`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Smooth {
    pub v: f32,
}

impl Smooth {
    pub fn new(v: f32) -> Self {
        Self { v }
    }

    #[inline]
    pub fn step(&mut self, target: f32, coef: f32) -> f32 {
        self.v += coef * (target - self.v);
        self.v
    }
}

/// The one-pole coefficient that reaches ~63% of a step in `tau` seconds
/// when stepped `rate` times a second.
pub fn one_pole(tau: f32, rate: f32) -> f32 {
    if tau <= 0.0 {
        1.0
    } else {
        1.0 - (-1.0 / (tau * rate)).exp()
    }
}

/// A cheap one-pole low-pass on a signal (`a` from [`lp_coef`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    y: f32,
}

impl OnePole {
    #[inline]
    pub fn lp(&mut self, x: f32, a: f32) -> f32 {
        self.y += a * (x - self.y);
        self.y
    }
}

/// The one-pole low-pass coefficient for a cutoff at `hz`.
pub fn lp_coef(hz: f32, sr: f32) -> f32 {
    1.0 - (-TAU * hz.max(1.0) / sr).exp()
}

/// The state-variable filter (Simper's trapezoidal SVF): low, band and
/// high outputs from one structure, stable under fast cutoff changes.
/// Coefficients are set with [`Svf::set`] (a `tan`, so callers set them
/// once a block, not once a sample).
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

/// One sample of an SVF: low, band, high.
#[derive(Clone, Copy, Debug)]
pub struct SvfOut {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
}

impl Svf {
    pub fn new(hz: f32, q: f32, sr: f32) -> Self {
        let mut f = Self::default();
        f.set(hz, q, sr);
        f
    }

    /// Cutoff (clamped under Nyquist) and resonance `q` (0.5 broad .. 10 sharp).
    pub fn set(&mut self, hz: f32, q: f32, sr: f32) {
        let hz = hz.clamp(10.0, sr * 0.45);
        let g = (PI * hz / sr).tan();
        self.k = 1.0 / q.max(0.3);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut { lp: v2, bp: v1, hp: x - self.k * v1 - v2 }
    }

    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.run(x).lp
    }

    /// Band-pass normalised to unity gain at the centre.
    #[inline]
    pub fn bp(&mut self, x: f32) -> f32 {
        let k = self.k;
        self.run(x).bp * k
    }

    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        self.run(x).hp
    }
}

/// A phase accumulator in `0..1`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Phase {
    pub p: f32,
}

impl Phase {
    #[inline]
    pub fn tick(&mut self, hz: f32, sr: f32) -> f32 {
        let p = self.p;
        self.p += hz / sr;
        if self.p >= 1.0 {
            self.p -= self.p.floor();
        }
        p
    }
}

#[inline]
pub fn sine(p: f32) -> f32 {
    (p * TAU).sin()
}

#[inline]
pub fn tri(p: f32) -> f32 {
    4.0 * (p - (p + 0.5).floor()).abs() - 1.0
}

/// PolyBLEP correction for a step at phase 0, `dt` the phase step.
#[inline]
fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

/// A band-limited sawtooth (polyBLEP), `dt` = hz / sr.
#[inline]
pub fn saw(p: f32, dt: f32) -> f32 {
    2.0 * p - 1.0 - blep(p, dt)
}

/// A band-limited square/pulse at duty `w`.
#[inline]
pub fn pulse(p: f32, dt: f32, w: f32) -> f32 {
    let a = saw(p, dt);
    let mut q = p + (1.0 - w);
    if q >= 1.0 {
        q -= 1.0;
    }
    (a - saw(q, dt)) * 0.5
}

/// A slow wandering value in `0..1`: a new random target every `period`
/// seconds (jittered), approached smoothly. Gusts, crowd swells, flicker.
#[derive(Clone, Debug, Default)]
pub struct Drift {
    pub value: Smooth,
    target: f32,
    left: f32,
}

impl Drift {
    pub fn new(start: f32) -> Self {
        Self { value: Smooth::new(start), target: start, left: 0.0 }
    }

    /// Advance by `dt` seconds (call once a block).
    pub fn step(&mut self, rng: &mut Rng, dt: f32, period: f32) -> f32 {
        self.left -= dt;
        if self.left <= 0.0 {
            self.target = rng.unit();
            self.left = period * rng.range(0.6, 1.4);
        }
        self.value.step(self.target, one_pole(period * 0.5, 1.0 / dt))
    }
}

/// Removes DC (a one-pole high-pass near 10 Hz).
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
}

impl DcBlock {
    #[inline]
    pub fn run(&mut self, x: f32, r: f32) -> f32 {
        let y = x - self.x1 + r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// The DC blocker's pole for `sr` (about 10 Hz).
pub fn dc_pole(sr: f32) -> f32 {
    1.0 - TAU * 10.0 / sr
}

/// A soft clip: linear below `knee`, bending smoothly to never pass 1.
#[inline]
pub fn soft_clip(x: f32, knee: f32) -> f32 {
    soft_clip_to(x, knee, 1.0)
}

/// [`soft_clip`] bending towards `ceil` instead of 1.
#[inline]
pub fn soft_clip_to(x: f32, knee: f32, ceil: f32) -> f32 {
    let a = x.abs();
    if a <= knee {
        x
    } else {
        let room = ceil - knee;
        (knee + room * ((a - knee) / room).tanh()).copysign(x)
    }
}

/// Gentle analogue warmth: a tanh drive normalised at small signals.
#[inline]
pub fn warm(x: f32, drive: f32) -> f32 {
    if drive <= 0.0 {
        x
    } else {
        (x * (1.0 + drive)).tanh() / (1.0 + drive)
    }
}

/// Equal-power pan of a mono sample: `pan` in `-1..1`.
#[inline]
pub fn pan(x: f32, pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (x * a.cos(), x * a.sin())
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clip_never_passes_one_and_is_linear_below_the_knee() {
        for i in -400..=400 {
            let x = i as f32 * 0.05;
            let y = soft_clip(x, 0.8);
            assert!(y.abs() < 1.0 + 1e-6, "{x} -> {y}");
            if x.abs() <= 0.8 {
                assert_eq!(y, x);
            }
        }
        // Monotonic.
        assert!(soft_clip(2.0, 0.8) > soft_clip(1.0, 0.8));
    }

    #[test]
    fn the_svf_low_pass_passes_lows_and_cuts_highs() {
        let sr = 48_000.0;
        let level = |hz: f32| {
            let mut f = Svf::new(500.0, 0.707, sr);
            let mut ph = Phase::default();
            let mut acc = 0.0;
            for i in 0..48_000 {
                let y = f.lp(sine(ph.tick(hz, sr)));
                if i > 4_800 {
                    acc += y * y;
                }
            }
            (acc / 43_200.0).sqrt()
        };
        assert!(level(100.0) > 0.65, "{}", level(100.0));
        assert!(level(8_000.0) < 0.01, "{}", level(8_000.0));
    }

    #[test]
    fn noise_has_no_dc_and_the_dc_blocker_removes_an_offset() {
        let mut rng = Rng::new(7);
        let mean: f32 = (0..100_000).map(|_| rng.white()).sum::<f32>() / 100_000.0;
        assert!(mean.abs() < 0.01, "{mean}");
        let mut dc = DcBlock::default();
        let r = dc_pole(48_000.0);
        let mut y = 0.0;
        for _ in 0..48_000 {
            y = dc.run(0.5, r);
        }
        assert!(y.abs() < 1e-3, "{y}");
    }
}
