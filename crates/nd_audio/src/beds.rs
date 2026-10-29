//! The ambience: three beds crossfaded by camera height (high: the distant
//! city's wash and the wind; mid: the traffic roar; street: tyres and
//! pass-bys, the crowd's murmur and footsteps, air-conditioning hum, neon
//! buzz), with weather, nature and water over them. All noise and a few
//! oscillators, shaped by filters; nothing is a recording.
//!
//! Parameters are eased once a block ([`BLOCK`] frames) and every gain is
//! ramped across the block, so nothing zips or clicks as the camera moves.

use crate::dsp::*;
use crate::scene::BedParams;

/// Frames between parameter updates.
pub const BLOCK: usize = 32;

/// How fast the beds follow new parameters, seconds: the crossfade by
/// height quickly (zooming should be heard), the rest slowly.
const TAU_HEIGHT: f32 = 0.35;
const TAU_LEVELS: f32 = 1.6;

/// The share of each bed's level in the readout: high, mid, street, and
/// the weather, nature and water over them.
pub const GROUPS: [&str; 4] = ["high", "mid", "street", "weather_nature"];

/// A gain that ramps linearly across a block.
#[derive(Clone, Copy, Debug, Default)]
struct Ramp {
    from: f32,
    to: f32,
}

impl Ramp {
    fn set(&mut self, to: f32) {
        self.from = self.to;
        self.to = to;
    }
    #[inline]
    fn at(&self, t: f32) -> f32 {
        self.from + (self.to - self.from) * t
    }
    fn live(&self) -> bool {
        self.from > 1e-5 || self.to > 1e-5
    }
}

/// A car going by in the street: a swell of tyre hiss that pans across
/// and dulls as it leaves.
#[derive(Clone, Debug, Default)]
struct Passby {
    on: bool,
    t: f32,
    dur: f32,
    amp: f32,
    pan0: f32,
    pan1: f32,
    hz: f32,
    f: Svf,
    body: Svf,
}

/// One voice of the crowd: noise through two drifting formants, opened
/// and closed at a syllable's pace.
#[derive(Clone, Debug)]
struct Talker {
    pink: Pink,
    f1: Svf,
    f2: Svf,
    syll: Drift,
    vowel: Drift,
    pan: f32,
}

/// A short filtered noise burst: a footstep, a raindrop, a splash.
#[derive(Clone, Debug, Default)]
struct Tick {
    on: bool,
    env: f32,
    decay: f32,
    amp: f32,
    pan: f32,
    f: Svf,
}

/// A bird's phrase: one to five glided sine chirps.
#[derive(Clone, Debug, Default)]
struct Chirp {
    on: bool,
    t: f32,
    dur: f32,
    gap: f32,
    reps: u8,
    f0: f32,
    f1: f32,
    amp: f32,
    pan: f32,
    ph: Phase,
}

/// A cricket: a high carrier gated into chirps of a few pulses.
#[derive(Clone, Debug, Default)]
struct Cricket {
    t: f32,
    period: f32,
    pulses: u8,
    hz: f32,
    pan: f32,
    amp: f32,
    ph: Phase,
}

/// The ambience generator.
pub struct Beds {
    sr: f32,
    rng: Rng,
    target: BedParams,
    now: BedParams,
    // High: wash and wind.
    wash_n: [Brown; 2],
    wash_f: [Svf; 2],
    wash_drift: Drift,
    wind_f: [Svf; 2],
    whistle_f: [Svf; 2],
    gust: [Drift; 2],
    // Mid: traffic roar and its rumble.
    traffic_n: [Pink; 2],
    traffic_f: [Svf; 2],
    rumble_n: [Brown; 2],
    rumble_f: [Svf; 2],
    traffic_drift: [Drift; 2],
    // Street.
    tyre_n: [Pink; 2],
    tyre_f: [Svf; 2],
    passbys: [Passby; 6],
    talkers: [Talker; 3],
    steps: [Tick; 6],
    hum_ph: [Phase; 3],
    fan_n: [Brown; 2],
    fan_f: [Svf; 2],
    fan_ph: Phase,
    neon_ph: [Phase; 2],
    neon_f: [Svf; 2],
    flicker: f32,
    flicker_left: f32,
    // Nature, weather, water.
    birds: [Chirp; 3],
    crickets: [Cricket; 2],
    rain_hp: [Svf; 2],
    rain_lp: [Svf; 2],
    roof_n: [Pink; 2],
    roof_f: [Svf; 2],
    drops: [Tick; 6],
    water_n: [Brown; 2],
    water_f: [Svf; 2],
    fizz_f: [Svf; 2],
    wave_ph: [Phase; 2],
    wave_drift: [Drift; 2],
    // The whole bed: muffle and level.
    muffle: [Svf; 2],
    // Gains ramped across the block.
    g: Gains,
    /// Sum of squares per group since the last [`Beds::take_meters`], and
    /// the frames counted.
    meters: [f32; 4],
    metered: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Gains {
    wash: Ramp,
    wind: Ramp,
    whistle: Ramp,
    traffic: Ramp,
    rumble: Ramp,
    tyres: Ramp,
    passby: Ramp,
    murmur: Ramp,
    steps: Ramp,
    hum: Ramp,
    fan: Ramp,
    neon: Ramp,
    birds: Ramp,
    crickets: Ramp,
    rain: Ramp,
    roof: Ramp,
    drops: Ramp,
    water: Ramp,
    level: Ramp,
}

/// The overall trim of each layer, set by ear from the offline renders'
/// levels (see `examples/render_wavs.rs`): the beds sit around -24 dBFS
/// RMS on their own at full slider, well under the UI cues.
mod trim {
    pub const WASH: f32 = 0.11;
    pub const WIND: f32 = 0.55;
    pub const WHISTLE: f32 = 0.08;
    pub const TRAFFIC: f32 = 0.30;
    pub const RUMBLE: f32 = 0.10;
    pub const TYRES: f32 = 0.10;
    pub const PASSBY: f32 = 0.55;
    pub const MURMUR: f32 = 0.55;
    pub const STEPS: f32 = 0.22;
    pub const HUM: f32 = 0.018;
    pub const FAN: f32 = 0.035;
    pub const NEON: f32 = 0.022;
    pub const BIRDS: f32 = 0.07;
    pub const CRICKETS: f32 = 0.1;
    pub const RAIN: f32 = 0.16;
    pub const ROOF: f32 = 0.22;
    pub const DROPS: f32 = 0.14;
    pub const WATER: f32 = 0.14;
}

fn lerp_params(now: &mut BedParams, to: &BedParams, a_h: f32, a: f32) {
    let s = |x: &mut f32, y: f32, c: f32| *x += c * (y - *x);
    s(&mut now.high, to.high, a_h);
    s(&mut now.mid, to.mid, a_h);
    s(&mut now.street, to.street, a_h);
    s(&mut now.wash, to.wash, a);
    s(&mut now.wash_tone, to.wash_tone, a);
    s(&mut now.wind, to.wind, a);
    s(&mut now.gust, to.gust, a);
    s(&mut now.traffic, to.traffic, a);
    s(&mut now.traffic_tone, to.traffic_tone, a);
    s(&mut now.tyres, to.tyres, a);
    s(&mut now.passby_rate, to.passby_rate, a);
    s(&mut now.murmur, to.murmur, a);
    s(&mut now.steps_rate, to.steps_rate, a);
    s(&mut now.hum, to.hum, a);
    s(&mut now.neon, to.neon, a);
    s(&mut now.birds, to.birds, a);
    s(&mut now.crickets, to.crickets, a);
    s(&mut now.rain, to.rain, a);
    s(&mut now.water, to.water, a);
    s(&mut now.muffle, to.muffle, a_h.min(a * 2.0));
    s(&mut now.level, to.level, a);
}

impl Beds {
    pub fn new(sr: f32, seed: u32) -> Self {
        let mut rng = Rng::new(seed ^ 0x5eed_0bed);
        let talker = |rng: &mut Rng, pan: f32| Talker { pink: Pink::default(), f1: Svf::default(), f2: Svf::default(), syll: Drift::new(rng.unit()), vowel: Drift::new(rng.unit()), pan };
        let talkers = [talker(&mut rng, -0.55), talker(&mut rng, 0.05), talker(&mut rng, 0.6)];
        let crickets = [
            Cricket { t: 0.0, period: 0.72, pulses: 3, hz: 4_500.0, pan: -0.5, amp: 1.0, ph: Phase::default() },
            Cricket { t: 0.31, period: 0.58, pulses: 4, hz: 4_820.0, pan: 0.55, amp: 0.7, ph: Phase::default() },
        ];
        let start = BedParams { level: 1.0, ..Default::default() };
        Self {
            sr,
            target: start,
            now: start,
            wash_n: Default::default(),
            wash_f: Default::default(),
            wash_drift: Drift::new(0.5),
            wind_f: Default::default(),
            whistle_f: Default::default(),
            gust: [Drift::new(0.4), Drift::new(0.6)],
            traffic_n: Default::default(),
            traffic_f: Default::default(),
            rumble_n: Default::default(),
            rumble_f: Default::default(),
            traffic_drift: [Drift::new(0.5), Drift::new(0.5)],
            tyre_n: Default::default(),
            tyre_f: Default::default(),
            passbys: Default::default(),
            talkers,
            steps: Default::default(),
            hum_ph: Default::default(),
            fan_n: Default::default(),
            fan_f: Default::default(),
            fan_ph: Phase::default(),
            neon_ph: [Phase::default(), Phase { p: 0.37 }],
            neon_f: Default::default(),
            flicker: 1.0,
            flicker_left: 0.0,
            birds: Default::default(),
            crickets,
            rain_hp: Default::default(),
            rain_lp: Default::default(),
            roof_n: Default::default(),
            roof_f: Default::default(),
            drops: Default::default(),
            water_n: Default::default(),
            water_f: Default::default(),
            fizz_f: Default::default(),
            wave_ph: [Phase { p: 0.0 }, Phase { p: 0.43 }],
            wave_drift: [Drift::new(0.5), Drift::new(0.5)],
            muffle: Default::default(),
            g: Gains::default(),
            meters: [0.0; 4],
            metered: 0,
            rng,
        }
    }

    /// New parameters to ease towards.
    pub fn set(&mut self, p: BedParams) {
        self.target = p;
    }

    /// Jump straight to the target (offline renders start settled).
    pub fn settle(&mut self) {
        self.now = self.target;
        self.update_block(1.0, 1.0);
        self.update_block(1.0, 1.0);
    }

    /// The parameters as eased now.
    pub fn current(&self) -> &BedParams {
        &self.now
    }

    /// The RMS level of each group since the last call (high, mid, street,
    /// weather/nature), and resets the meters.
    pub fn take_meters(&mut self) -> [f32; 4] {
        let n = self.metered.max(1) as f32;
        let out = self.meters.map(|s| (s / n).sqrt());
        self.meters = [0.0; 4];
        self.metered = 0;
        out
    }

    /// Ease the parameters one block and set the filters and gain ramps.
    fn update_block(&mut self, a_h: f32, a: f32) {
        let target = self.target;
        lerp_params(&mut self.now, &target, a_h, a);
        let p = self.now;
        let sr = self.sr;
        let dt = BLOCK as f32 / sr;
        let rng = &mut self.rng;
        // Wash: a slow swell, brighter over a dense or neon-lit city.
        let swell = 0.75 + 0.5 * self.wash_drift.step(rng, dt, 6.0);
        for f in &mut self.wash_f {
            f.set(160.0 + 520.0 * p.wash_tone * p.wash_tone, 0.6, sr);
        }
        // Wind: gusts sweep the band and the level.
        let gusts: [f32; 2] = [self.gust[0].step(rng, dt, 3.2), self.gust[1].step(rng, dt, 3.9)];
        for (i, f) in self.wind_f.iter_mut().enumerate() {
            f.set(240.0 + 700.0 * gusts[i] * (0.5 + 0.5 * p.gust), 1.3, sr);
        }
        for (i, f) in self.whistle_f.iter_mut().enumerate() {
            f.set(900.0 + 900.0 * gusts[i], 7.0, sr);
        }
        let gust_amp = 0.45 + 0.75 * p.gust * (gusts[0] + gusts[1]) * 0.5;
        // Traffic: brighter as it speeds up; clumps of cars swell it.
        let clump: [f32; 2] = [self.traffic_drift[0].step(rng, dt, 2.4), self.traffic_drift[1].step(rng, dt, 2.9)];
        for f in &mut self.traffic_f {
            f.set(220.0 + 900.0 * p.traffic_tone, 0.55, sr);
        }
        for f in &mut self.rumble_f {
            f.set(95.0, 0.7, sr);
        }
        for f in &mut self.tyre_f {
            f.set(700.0 + 600.0 * p.traffic_tone, 0.5, sr);
        }
        // Passing cars.
        let street = p.street;
        if p.tyres * street > 1e-3 {
            let chance = p.passby_rate * dt;
            if rng.chance(chance) {
                if let Some(v) = self.passbys.iter_mut().find(|v| !v.on) {
                    let left = rng.chance(0.5);
                    *v = Passby {
                        on: true,
                        t: 0.0,
                        dur: rng.range(1.6, 4.2) / (0.6 + 0.6 * p.traffic_tone),
                        amp: rng.range(0.45, 1.0),
                        pan0: if left { -0.85 } else { 0.85 },
                        pan1: if left { 0.85 } else { -0.85 },
                        hz: rng.range(750.0, 1_500.0),
                        f: v.f,
                        body: v.body,
                    };
                }
            }
        }
        for v in self.passbys.iter_mut().filter(|v| v.on) {
            let u = (v.t / v.dur).clamp(0.0, 1.0);
            v.f.set(v.hz * (1.25 - 0.55 * u), 0.9, sr);
            v.body.set(160.0 + 120.0 * (1.0 - u), 0.8, sr);
        }
        // The crowd.
        for t in &mut self.talkers {
            t.syll.step(rng, dt, 0.16);
            let vowel = t.vowel.step(rng, dt, 0.35);
            t.f1.set(380.0 + 380.0 * vowel, 2.2, sr);
            t.f2.set(1_050.0 + 900.0 * (1.0 - vowel), 3.0, sr);
        }
        if p.murmur * street > 1e-3 && rng.chance(p.steps_rate * dt) {
            if let Some(s) = self.steps.iter_mut().find(|s| !s.on) {
                s.on = true;
                s.env = 1.0;
                s.decay = (-1.0 / (rng.range(0.008, 0.016) * sr)).exp();
                s.amp = rng.range(0.4, 1.0);
                s.pan = rng.range(-0.8, 0.8);
                s.f.set(rng.range(900.0, 2_400.0), 1.1, sr);
            }
        }
        // Hum and fans: steady, a blade's flutter.
        for f in &mut self.fan_f {
            f.set(260.0, 0.6, sr);
        }
        // Neon: a thin buzz, now and then a stutter.
        for f in &mut self.neon_f {
            f.set(1_600.0, 0.7, sr);
        }
        if self.flicker_left > 0.0 {
            self.flicker_left -= dt;
            if rng.chance(0.35) {
                self.flicker = rng.range(0.15, 1.0);
            }
        } else {
            self.flicker += 0.2 * (1.0 - self.flicker);
            if p.neon > 1e-3 && rng.chance(0.25 * dt) {
                self.flicker_left = rng.range(0.08, 0.45);
            }
        }
        // Birds: phrases now and then.
        if p.birds > 1e-3 && rng.chance(p.birds * 1.1 * dt) {
            if let Some(c) = self.birds.iter_mut().find(|c| !c.on) {
                let f0 = rng.range(2_300.0, 4_600.0);
                *c = Chirp {
                    on: true,
                    t: 0.0,
                    dur: rng.range(0.045, 0.12),
                    gap: rng.range(0.03, 0.09),
                    reps: 1 + (rng.unit() * 4.0) as u8,
                    f0,
                    f1: f0 + rng.range(-1_100.0, 1_300.0),
                    amp: rng.range(0.4, 1.0),
                    pan: rng.range(-0.9, 0.9),
                    ph: c.ph,
                };
            }
        }
        // Rain and drops.
        for f in &mut self.rain_hp {
            f.set(1_700.0, 0.6, sr);
        }
        for f in &mut self.rain_lp {
            f.set(8_500.0 - 3_000.0 * p.high, 0.6, sr);
        }
        for f in &mut self.roof_f {
            f.set(900.0, 0.5, sr);
        }
        if p.rain * street > 1e-3 && rng.chance((24.0 * p.rain * dt).min(0.9)) {
            if let Some(d) = self.drops.iter_mut().find(|d| !d.on) {
                d.on = true;
                d.env = 1.0;
                d.decay = (-1.0 / (rng.range(0.003, 0.007) * sr)).exp();
                d.amp = rng.range(0.3, 1.0);
                d.pan = rng.range(-0.9, 0.9);
                d.f.set(rng.range(2_800.0, 6_500.0), 2.5, sr);
            }
        }
        // Water: slow waves lapping.
        for f in &mut self.water_f {
            f.set(420.0, 0.6, sr);
        }
        for f in &mut self.fizz_f {
            f.set(2_600.0, 0.7, sr);
        }
        self.wave_drift[0].step(rng, dt, 3.0);
        self.wave_drift[1].step(rng, dt, 3.5);
        // The muffle: open at 16 kHz, closed near 380 Hz.
        let cut = (16_000f32.ln() + (380f32.ln() - 16_000f32.ln()) * p.muffle.clamp(0.0, 1.0)).exp();
        for f in &mut self.muffle {
            f.set(cut, 0.6, sr);
        }
        // The gains, ramped across the next block.
        let (high, mid) = (p.high, p.mid);
        let g = &mut self.g;
        g.wash.set(trim::WASH * p.wash * (high + 0.35 * mid) * swell);
        g.wind.set(trim::WIND * p.wind * gust_amp);
        g.whistle.set(trim::WHISTLE * p.wind * p.gust * p.gust * gust_amp);
        g.traffic.set(trim::TRAFFIC * p.traffic * (mid + 0.45 * high + 0.4 * street) * (0.8 + 0.4 * (clump[0] + clump[1]) * 0.5));
        g.rumble.set(trim::RUMBLE * p.traffic * (mid + 0.3 * high + 0.6 * street));
        g.tyres.set(trim::TYRES * p.tyres * (street + 0.3 * mid));
        g.passby.set(trim::PASSBY * p.tyres.max(0.25) * street);
        g.murmur.set(trim::MURMUR * p.murmur * street);
        g.steps.set(trim::STEPS * street.max(0.0) * p.murmur.max(0.2).min(1.0));
        g.hum.set(trim::HUM * p.hum * (street + 0.3 * mid));
        g.fan.set(trim::FAN * p.hum * (street + 0.2 * mid));
        g.neon.set(trim::NEON * p.neon * street * self.flicker);
        g.birds.set(trim::BIRDS * p.birds * (street + 0.5 * mid));
        g.crickets.set(trim::CRICKETS * p.crickets * (street + 0.6 * mid + 0.2 * high));
        g.rain.set(trim::RAIN * p.rain * (0.6 + 0.4 * street));
        g.roof.set(trim::ROOF * p.rain * (0.4 + 0.6 * street + 0.4 * mid));
        g.drops.set(trim::DROPS * street);
        g.water.set(trim::WATER * p.water * (street + 0.5 * mid + 0.15 * high));
        g.level.set(p.level.clamp(0.0, 1.0));
    }

    /// Mix `frames` stereo frames of ambience into `out` (interleaved L R),
    /// scaled by `gain` (the ambience bus).
    pub fn render(&mut self, out: &mut [f32], gain: f32) {
        let frames = out.len() / 2;
        let a_h = one_pole(TAU_HEIGHT, self.sr / BLOCK as f32);
        let a = one_pole(TAU_LEVELS, self.sr / BLOCK as f32);
        let mut i = 0;
        while i < frames {
            self.update_block(a_h, a);
            let n = BLOCK.min(frames - i);
            self.render_block(&mut out[i * 2..(i + n) * 2], gain);
            i += n;
        }
    }

    fn render_block(&mut self, out: &mut [f32], gain: f32) {
        let sr = self.sr;
        let n = out.len() / 2;
        let g = self.g;
        let rng = &mut self.rng;
        let inv_n = 1.0 / BLOCK as f32;
        let dt = 1.0 / sr;
        let (wash_on, wind_on, traffic_on) = (g.wash.live(), g.wind.live() || g.whistle.live(), g.traffic.live() || g.rumble.live());
        let (tyre_on, murmur_on, hum_on, neon_on) = (g.tyres.live(), g.murmur.live(), g.hum.live() || g.fan.live(), g.neon.live());
        let (bird_on, cricket_on, rain_on, water_on) = (g.birds.live(), g.crickets.live(), g.rain.live() || g.roof.live(), g.water.live());
        let passby_on = g.passby.live() && self.passbys.iter().any(|v| v.on);
        let steps_on = self.steps.iter().any(|s| s.on);
        let drops_on = self.drops.iter().any(|d| d.on);
        let birds_now = bird_on && self.birds.iter().any(|c| c.on);
        let mut m = [0.0f32; 4];
        for k in 0..n {
            let t = k as f32 * inv_n;
            let (mut hl, mut hr, mut ml, mut mr, mut sl, mut srr, mut xl, mut xr) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            if wash_on {
                let w = g.wash.at(t);
                hl += w * self.wash_f[0].lp(self.wash_n[0].next(rng.white()));
                hr += w * self.wash_f[1].lp(self.wash_n[1].next(rng.white()));
            }
            if wind_on {
                let (w, wh) = (g.wind.at(t), g.whistle.at(t));
                let (nl, nr) = (rng.white(), rng.white());
                hl += w * self.wind_f[0].bp(nl) + wh * self.whistle_f[0].bp(nl);
                hr += w * self.wind_f[1].bp(nr) + wh * self.whistle_f[1].bp(nr);
            }
            if traffic_on {
                let (w, r) = (g.traffic.at(t), g.rumble.at(t));
                ml += w * self.traffic_f[0].lp(self.traffic_n[0].next(rng.white())) + r * self.rumble_f[0].lp(self.rumble_n[0].next(rng.white()));
                mr += w * self.traffic_f[1].lp(self.traffic_n[1].next(rng.white())) + r * self.rumble_f[1].lp(self.rumble_n[1].next(rng.white()));
            }
            if tyre_on {
                let w = g.tyres.at(t);
                sl += w * self.tyre_f[0].bp(self.tyre_n[0].next(rng.white()));
                srr += w * self.tyre_f[1].bp(self.tyre_n[1].next(rng.white()));
            }
            if passby_on {
                let w = g.passby.at(t);
                for v in self.passbys.iter_mut().filter(|v| v.on) {
                    let u = v.t / v.dur;
                    let env = (std::f32::consts::PI * u).sin().powi(3) * v.amp;
                    let x = v.f.bp(rng.white()) + 0.6 * v.body.lp(rng.white());
                    let (l, r) = pan(x * env * w, v.pan0 + (v.pan1 - v.pan0) * u);
                    sl += l;
                    srr += r;
                    v.t += dt;
                    if v.t >= v.dur {
                        v.on = false;
                    }
                }
            }
            if murmur_on {
                let w = g.murmur.at(t);
                for tk in &mut self.talkers {
                    let x = tk.pink.next(rng.white());
                    let open = tk.syll.value.v;
                    let y = (tk.f1.bp(x) + 0.6 * tk.f2.bp(x)) * open * open;
                    let (l, r) = pan(y * w, tk.pan);
                    sl += l;
                    srr += r;
                }
            }
            if steps_on {
                let w = g.steps.at(t);
                for s in self.steps.iter_mut().filter(|s| s.on) {
                    let y = s.f.bp(rng.white()) * s.env * s.amp * w;
                    s.env *= s.decay;
                    let (l, r) = pan(y, s.pan);
                    sl += l;
                    srr += r;
                    if s.env < 1e-4 {
                        s.on = false;
                    }
                }
            }
            if hum_on {
                let (h, fan) = (g.hum.at(t), g.fan.at(t));
                let hum = sine(self.hum_ph[0].tick(100.0, sr)) + 0.5 * sine(self.hum_ph[1].tick(200.0, sr)) + 0.25 * sine(self.hum_ph[2].tick(300.0, sr));
                let flutter = 0.8 + 0.2 * sine(self.fan_ph.tick(7.3, sr));
                sl += h * hum + fan * flutter * self.fan_f[0].lp(self.fan_n[0].next(rng.white()));
                srr += h * hum + fan * flutter * self.fan_f[1].lp(self.fan_n[1].next(rng.white()));
            }
            if neon_on {
                let w = g.neon.at(t);
                let d0 = 100.0 / sr;
                let d1 = 100.35 / sr;
                let a = saw(self.neon_ph[0].tick(100.0, sr), d0);
                let b = saw(self.neon_ph[1].tick(100.35, sr), d1);
                sl += w * self.neon_f[0].hp(a);
                srr += w * self.neon_f[1].hp(b);
            }
            if birds_now {
                let w = g.birds.at(t);
                for c in self.birds.iter_mut().filter(|c| c.on) {
                    if c.t < c.dur {
                        let u = c.t / c.dur;
                        let hz = c.f0 + (c.f1 - c.f0) * u;
                        let env = (std::f32::consts::PI * u).sin().powi(2) * c.amp;
                        let y = sine(c.ph.tick(hz, sr)) * env * w;
                        let (l, r) = pan(y, c.pan);
                        xl += l;
                        xr += r;
                    }
                    c.t += dt;
                    if c.t >= c.dur + c.gap {
                        if c.reps > 1 {
                            c.reps -= 1;
                            c.t = 0.0;
                            c.f0 *= 1.04;
                            c.f1 *= 1.04;
                        } else {
                            c.on = false;
                        }
                    }
                }
            }
            if cricket_on {
                let w = g.crickets.at(t);
                for c in &mut self.crickets {
                    // Pulses of 18 ms every 33 ms at the start of each period.
                    let pulse_len = 0.018;
                    let pulse_step = 0.033;
                    let idx = (c.t / pulse_step) as u8;
                    let within = c.t - idx as f32 * pulse_step;
                    let gate = if idx < c.pulses && within < pulse_len { (std::f32::consts::PI * within / pulse_len).sin().powi(2) } else { 0.0 };
                    let y = sine(c.ph.tick(c.hz, sr)) * gate * c.amp * w;
                    let (l, r) = pan(y, c.pan);
                    xl += l;
                    xr += r;
                    c.t += dt;
                    if c.t >= c.period {
                        c.t -= c.period;
                    }
                }
            }
            if rain_on {
                let (w, roof) = (g.rain.at(t), g.roof.at(t));
                let (nl, nr) = (rng.white(), rng.white());
                xl += w * self.rain_lp[0].lp(self.rain_hp[0].hp(nl)) + roof * self.roof_f[0].lp(self.roof_n[0].next(nl));
                xr += w * self.rain_lp[1].lp(self.rain_hp[1].hp(nr)) + roof * self.roof_f[1].lp(self.roof_n[1].next(nr));
            }
            if drops_on {
                let w = g.drops.at(t);
                for d in self.drops.iter_mut().filter(|d| d.on) {
                    let y = d.f.bp(rng.white()) * d.env * d.amp * w;
                    d.env *= d.decay;
                    let (l, r) = pan(y, d.pan);
                    xl += l;
                    xr += r;
                    if d.env < 1e-4 {
                        d.on = false;
                    }
                }
            }
            if water_on {
                let w = g.water.at(t);
                for ch in 0..2 {
                    let rate = 0.21 + 0.06 * ch as f32 + 0.05 * self.wave_drift[ch].value.v;
                    let s = 0.5 + 0.5 * sine(self.wave_ph[ch].tick(rate, sr));
                    let env = s * s * s;
                    let x = rng.white();
                    let y = w * (env * self.water_f[ch].lp(self.water_n[ch].next(x)) * 2.2 + 0.25 * env * env * self.fizz_f[ch].bp(x));
                    if ch == 0 {
                        xl += y;
                    } else {
                        xr += y;
                    }
                }
            }
            m[0] += hl * hl + hr * hr;
            m[1] += ml * ml + mr * mr;
            m[2] += sl * sl + srr * srr;
            m[3] += xl * xl + xr * xr;
            let lvl = g.level.at(t) * gain;
            let l = self.muffle[0].lp(hl + ml + sl + xl) * lvl;
            let r = self.muffle[1].lp(hr + mr + srr + xr) * lvl;
            out[k * 2] += l;
            out[k * 2 + 1] += r;
        }
        for (acc, v) in self.meters.iter_mut().zip(m) {
            *acc += v * 0.5;
        }
        self.metered += n as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{bed_params, Surroundings};

    fn render_secs(beds: &mut Beds, secs: f32) -> Vec<f32> {
        let mut out = vec![0.0; (48_000.0 * secs) as usize * 2];
        beds.render(&mut out, 1.0);
        out
    }

    #[test]
    fn a_change_of_surroundings_ramps_without_a_jump() {
        let mut beds = Beds::new(48_000.0, 1);
        beds.set(bed_params(&Surroundings { height: 2.0, cars_close: 20.0, car_speed: 12.0, walkers_close: 30.0, neon: 1.0, daylight: 0.0, density: 1.0, ..Default::default() }));
        beds.settle();
        let _ = render_secs(&mut beds, 1.0);
        // Fly up to 1 km in one go: the sample-to-sample step stays small.
        beds.set(bed_params(&Surroundings { height: 1_000.0, wind: 0.9, ..Default::default() }));
        let out = render_secs(&mut beds, 2.0);
        let max_step = out.chunks(2).collect::<Vec<_>>().windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0f32, f32::max);
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak < 0.9, "{peak}");
        assert!(max_step < 0.25, "{max_step}");
    }
}
