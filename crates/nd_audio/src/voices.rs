//! [sound] The spot sounds' voices: each kind synthesised from noise, a
//! few oscillators and resonant filters (nothing recorded), then dulled by
//! distance and muffle, eased to its gain and placed in the stereo. Held
//! voices follow their [`SpotUpdate`]s and fade out when the updates stop;
//! one-shots play once. A short two-tap echo off the buildings sits over
//! the lot, deeper in a dense district.
//!
//! Parameters move once a block ([`BLOCK`] frames); gains ramp inside it.

use std::f32::consts::{SQRT_2, TAU};

use crate::dsp::*;
use crate::engine::{VoicePool, SPOT_VOICES};
use crate::spots::{SpotKind, SpotUpdate};

/// Frames between parameter updates.
pub const BLOCK: usize = 32;
/// Held voices fade out this long after their last update, seconds.
pub const STALE: f32 = 0.8;
/// How fast a held voice follows its gain and pan, seconds.
const TAU_GAIN: f32 = 0.3;
const TAU_PAN: f32 = 0.25;
/// The spot bus's overall trim, set from the offline renders: a train
/// going by close peaks about -10 dBFS at full sliders, over beds at -24.
pub const SPOT_TRIM: f32 = 0.5;

/// A decaying envelope: `trigger` sets it, each `next` multiplies.
#[derive(Clone, Copy, Debug, Default)]
struct Decay {
    v: f32,
    k: f32,
}

impl Decay {
    fn trigger(&mut self, amp: f32, tau: f32, sr: f32) {
        self.v = amp;
        self.k = (-1.0 / (tau.max(1e-4) * sr)).exp();
    }
    #[inline]
    fn next(&mut self) -> f32 {
        let v = self.v;
        self.v *= self.k;
        v
    }
    fn live(&self) -> bool {
        self.v > 1e-5
    }
}

/// Semitones to a ratio.
fn semi(n: f32) -> f32 {
    (n / 12.0).exp2()
}

/// I V vi IV: chord roots and triads, semitones over the key.
const CHORDS: [[f32; 3]; 4] = [[0.0, 4.0, 7.0], [7.0, 11.0, 14.0], [9.0, 12.0, 16.0], [5.0, 9.0, 12.0]];

/// A train, surface or metro.
#[derive(Clone, Debug, Default)]
struct Train {
    metro: bool,
    speed: f32,
    rumble_n: Brown,
    rumble_f: Svf,
    body_n: Pink,
    body_f: Svf,
    clack: Decay,
    clack_f: [Svf; 2],
    thump: Decay,
    thump_ph: Phase,
    /// Metres to the next wheel over a rail joint, and where in the pattern.
    to_clack: f32,
    pat: usize,
    whine_ph: Phase,
    whine_hz: f32,
    squeal_ph: Phase,
    squeal: f32,
    squeal_left: f32,
    squeal_hz: f32,
    hum_ph: Phase,
}

/// The wheels' spacing over a joint, metres: a bogie's two axles, the
/// car's length to the next bogie, the coupling.
const AXLES: [f32; 4] = [2.5, 14.5, 2.5, 5.5];

impl Train {
    fn block(&mut self, speed: f32, rng: &mut Rng, sr: f32, dt: f32) {
        self.speed += one_pole(0.6, 1.0 / dt) * (speed - self.speed);
        let v = self.speed;
        let (rum, body, c1, c2) = if self.metro { (70.0 + 4.0 * v, 260.0 + 6.0 * v, 380.0, 950.0) } else { (90.0 + 7.0 * v, 420.0 + 14.0 * v, 950.0, 2_300.0) };
        self.rumble_f.set(rum, 0.8, sr);
        self.body_f.set(body, 0.9, sr);
        self.clack_f[0].set(c1, 5.0, sr);
        self.clack_f[1].set(c2, 7.0, sr);
        self.whine_hz = 35.0 + 24.0 * v;
        // A flange squeal now and then on the slow curves into a station.
        if !self.metro && self.squeal_left <= 0.0 && (3.0..10.0).contains(&v) && rng.chance(dt * 0.12) {
            self.squeal_left = rng.range(0.8, 2.5);
            self.squeal_hz = rng.range(2_700.0, 3_600.0);
        }
        self.squeal_left -= dt;
        let want = if self.squeal_left > 0.0 { 1.0 } else { 0.0 };
        self.squeal += one_pole(0.15, 1.0 / dt) * (want - self.squeal);
    }

    #[inline]
    fn sample(&mut self, rng: &mut Rng, sr: f32) -> f32 {
        let v = self.speed;
        let moving = smoothstep(0.3, 3.0, v);
        let sf = (0.25 + v / 18.0).min(1.3);
        let mut x = 0.55 * sf * self.rumble_f.lp(self.rumble_n.next(rng.white()));
        x += 0.3 * sf * moving * self.body_f.bp(self.body_n.next(rng.white()));
        // The wheels over the joints.
        if v > 0.5 {
            self.to_clack -= v / sr;
            if self.to_clack <= 0.0 {
                self.to_clack += AXLES[self.pat];
                self.pat = (self.pat + 1) % AXLES.len();
                let a = (v / 16.0).clamp(0.25, 1.0) * rng.range(0.75, 1.0);
                self.clack.trigger(a, 0.003, sr);
                self.thump.trigger(a, 0.035, sr);
            }
        }
        if self.clack.live() || self.thump.live() {
            let e = self.clack.next() * rng.white();
            let hi = if self.metro { 0.5 } else { 1.0 };
            x += 0.7 * self.clack_f[0].run(e).bp + 0.45 * hi * self.clack_f[1].run(e).bp;
            x += 0.3 * self.thump.next() * sine(self.thump_ph.tick(if self.metro { 55.0 } else { 75.0 }, sr));
        }
        // The motors' whine, up with speed; louder in the metro.
        let w = if self.metro { 0.07 } else { 0.035 } * moving * (1.0 - 0.5 * smoothstep(14.0, 26.0, v));
        if w > 0.0 {
            let p = self.whine_ph.tick(self.whine_hz, sr);
            x += w * (sine(p) + 0.4 * sine(3.0 * p));
        }
        // Standing: the compressor and the air conditioning.
        let idle = 1.0 - moving;
        if idle > 0.0 {
            x += 0.05 * idle * tri(self.hum_ph.tick(100.0, sr));
        }
        if self.squeal > 1e-3 {
            let wob = 1.0 + 0.004 * sine(self.squeal_ph.p * 7.0);
            x += 0.05 * self.squeal * sine(self.squeal_ph.tick(self.squeal_hz * wob, sr));
        }
        x
    }
}

/// A siren: police (wail and yelp), ambulance (hi-lo), fire engine (low
/// wail and its air horn).
#[derive(Clone, Debug, Default)]
struct Siren {
    variant: u8,
    t: f32,
    hz: f32,
    ph: Phase,
    f: Svf,
    horn: f32,
    horn_ph: [Phase; 2],
    next_horn: f32,
}

impl Siren {
    fn block(&mut self, pitch: f32, rng: &mut Rng, sr: f32, dt: f32) {
        self.t += dt;
        let t = self.t;
        let base = match self.variant {
            0 => {
                // Police: wail for 8 s, yelp for 4.
                let cycle = t % 12.0;
                if cycle < 8.0 {
                    let u = 0.5 - 0.5 * (TAU * cycle / 4.0).cos();
                    650.0 + 700.0 * u
                } else {
                    let u = 0.5 - 0.5 * (TAU * cycle / 0.32).cos();
                    700.0 + 650.0 * u
                }
            }
            1 => {
                if (t / 0.55) as i64 % 2 == 0 {
                    960.0
                } else {
                    770.0
                }
            }
            _ => {
                let u = 0.5 - 0.5 * (TAU * t / 6.0).cos();
                420.0 + 520.0 * u
            }
        };
        self.hz = base * pitch;
        self.f.set(self.hz * 1.6, 0.8, sr);
        if self.variant == 2 {
            self.next_horn -= dt;
            if self.next_horn <= 0.0 {
                self.next_horn = rng.range(4.0, 9.0);
                self.horn = rng.range(0.6, 1.3);
            }
            self.horn -= dt;
        }
    }

    #[inline]
    fn sample(&mut self, sr: f32) -> f32 {
        let dt = self.hz / sr;
        let p = self.ph.tick(self.hz, sr);
        let raw = 0.6 * pulse(p, dt, 0.5) + 0.4 * saw(p, dt);
        let mut x = warm(self.f.bp(raw), 1.2) * 1.6;
        if self.horn > 0.0 {
            let a = (self.horn / 0.05).min(1.0);
            let (h1, h2) = (233.0, 277.0);
            x += 0.6 * a * (pulse(self.horn_ph[0].tick(h1, sr), h1 / sr, 0.4) + pulse(self.horn_ph[1].tick(h2, sr), h2 / sr, 0.4));
        }
        x
    }
}

/// Building works: a hammer in bursts, a breaker now and then, a diesel
/// engine idling and revving, a reversing beeper.
#[derive(Clone, Debug, Default)]
struct Works {
    hit: Decay,
    hit_f: [Svf; 3],
    hits_left: u8,
    next_hit: f32,
    thud: Decay,
    thud_ph: Phase,
    drill_left: f32,
    next_drill: f32,
    drill_ph: Phase,
    drill_f: Svf,
    engine_ph: Phase,
    engine_f: Svf,
    rev: f32,
    rev_to: f32,
    beeps_left: u8,
    beep_t: f32,
    next_beeps: f32,
    beep_ph: Phase,
}

impl Works {
    fn new(rng: &mut Rng, sr: f32) -> Self {
        let mut w = Works { next_hit: rng.range(0.1, 1.0), next_drill: rng.range(2.0, 12.0), next_beeps: rng.range(6.0, 20.0), ..Default::default() };
        let tones = [rng.range(1_200.0, 1_600.0), rng.range(2_600.0, 3_200.0), rng.range(3_900.0, 4_600.0)];
        for (f, hz) in w.hit_f.iter_mut().zip(tones) {
            f.set(hz, 14.0, sr);
        }
        w.drill_f.set(1_800.0, 1.2, sr);
        w.engine_f.set(260.0, 0.9, sr);
        w
    }

    fn block(&mut self, activity: f32, rng: &mut Rng, sr: f32, dt: f32) {
        self.next_hit -= dt;
        if self.next_hit <= 0.0 {
            if self.hits_left == 0 {
                // A pause, then a new burst of strokes.
                self.hits_left = rng.range(3.0, 9.0) as u8;
                self.next_hit = rng.range(1.5, 6.0) / activity.max(0.3);
            } else {
                self.hits_left -= 1;
                let a = rng.range(0.6, 1.0);
                self.hit.trigger(a, 0.004, sr);
                self.thud.trigger(a, 0.03, sr);
                self.next_hit = rng.range(0.38, 0.62);
            }
        }
        self.next_drill -= dt;
        if self.next_drill <= 0.0 {
            self.drill_left = rng.range(1.2, 3.5);
            self.next_drill = rng.range(10.0, 28.0) / activity.max(0.3);
        }
        self.drill_left -= dt;
        if rng.chance(dt * 0.3) {
            self.rev_to = if rng.chance(0.35) { rng.range(0.5, 1.0) } else { 0.0 };
        }
        self.rev += one_pole(0.5, 1.0 / dt) * (self.rev_to - self.rev);
        self.engine_f.set(220.0 + 300.0 * self.rev, 0.9, sr);
        self.next_beeps -= dt;
        if self.next_beeps <= 0.0 {
            self.beeps_left = rng.range(4.0, 9.0) as u8;
            self.beep_t = 0.0;
            self.next_beeps = rng.range(15.0, 40.0);
        }
    }

    #[inline]
    fn sample(&mut self, rng: &mut Rng, sr: f32) -> f32 {
        let mut x = 0.0;
        if self.hit.live() || self.thud.live() {
            let e = self.hit.next() * rng.white();
            x += 1.0 * (self.hit_f[0].run(e).bp + 0.7 * self.hit_f[1].run(e).bp + 0.5 * self.hit_f[2].run(e).bp);
            x += 0.45 * self.thud.next() * sine(self.thud_ph.tick(110.0, sr));
        }
        if self.drill_left > 0.0 {
            let gate = if self.drill_ph.tick(23.0, sr) < 0.35 { 1.0 } else { 0.15 };
            x += 0.5 * gate * (self.drill_left / 0.05).min(1.0) * self.drill_f.bp(rng.white());
        }
        let hz = 30.0 + 18.0 * self.rev;
        x += (0.2 + 0.12 * self.rev) * self.engine_f.lp(pulse(self.engine_ph.tick(hz, sr), hz / sr, 0.3));
        if self.beeps_left > 0 {
            self.beep_t += 1.0 / sr;
            if self.beep_t < 0.45 {
                let edge = (self.beep_t / 0.01).min(1.0) * ((0.45 - self.beep_t) / 0.01).min(1.0);
                x += 0.08 * edge * sine(self.beep_ph.tick(1_050.0, sr));
            } else if self.beep_t >= 0.9 {
                self.beep_t = 0.0;
                self.beeps_left -= 1;
            }
        }
        x
    }
}

/// A venue's music and crowd, heard through its walls, the door opening
/// now and then.
#[derive(Clone, Debug, Default)]
struct Venue {
    kind: u8,
    bpm: f32,
    key: f32,
    beats: f32,
    step: i32,
    kick: Decay,
    kick_ph: Phase,
    snare: Decay,
    snare_f: Svf,
    snare_ph: Phase,
    hat: Decay,
    hat_f: Svf,
    bass: Decay,
    bass_ph: Phase,
    bass_hz: f32,
    bass_f: Svf,
    pad_ph: [Phase; 3],
    pad_hz: [f32; 3],
    pad: Decay,
    pad_f: Svf,
    // The singer (karaoke).
    sing_ph: Phase,
    sing_hz: f32,
    sing_to: f32,
    sing_amp: f32,
    sing_on: f32,
    vib_ph: Phase,
    formant: [Svf; 2],
    // The talk and the glasses (bars, between songs).
    talk_n: Pink,
    talk_f: [Svf; 2],
    syll: Drift,
    vowel: Drift,
    clink: Decay,
    clink_ph: [Phase; 3],
    clink_hz: [f32; 3],
    next_clink: f32,
    // The walls, and the door.
    wall: Svf,
    door: f32,
    door_left: f32,
    next_door: f32,
}

const VOWELS: [(f32, f32); 4] = [(800.0, 1_200.0), (500.0, 900.0), (450.0, 1_900.0), (320.0, 2_300.0)];

impl Venue {
    /// `kind`: 0 club, 1 bar, 2 karaoke, 3 music hall.
    fn new(kind: u8, rng: &mut Rng, sr: f32) -> Self {
        let bpm = match kind {
            0 => rng.range(122.0, 128.0),
            1 => rng.range(88.0, 96.0),
            2 => rng.range(90.0, 100.0),
            _ => rng.range(100.0, 112.0),
        };
        let key = 55.0 * semi((rng.next_u32() % 7) as f32);
        let mut v = Venue { kind, bpm, key, step: -1, next_clink: rng.range(0.5, 3.0), next_door: rng.range(4.0, 15.0), syll: Drift::new(0.5), vowel: Drift::new(0.5), ..Default::default() };
        v.snare_f.set(1_900.0, 0.9, sr);
        v.hat_f.set(8_000.0, 0.7, sr);
        v.bass_f.set(260.0, 1.2, sr);
        v.pad_f.set(900.0, 0.7, sr);
        v.beats = rng.range(0.0, 16.0);
        v
    }

    fn wall_cut(&self) -> f32 {
        match self.kind {
            0 => 230.0,
            1 => 1_400.0,
            2 => 1_500.0,
            _ => 520.0,
        }
    }

    fn block(&mut self, rng: &mut Rng, sr: f32, dt: f32) {
        // The door: a few seconds open now and then, the music spilling out.
        self.next_door -= dt;
        if self.next_door <= 0.0 {
            self.door_left = rng.range(1.5, 4.0);
            self.next_door = rng.range(8.0, 25.0);
        }
        self.door_left -= dt;
        let want = if self.door_left > 0.0 { 1.0 } else { 0.0 };
        self.door += one_pole(0.12, 1.0 / dt) * (want - self.door);
        let closed = self.wall_cut();
        self.wall.set(closed * (4_500.0 / closed).powf(self.door), 0.7, sr);
        if self.kind == 1 {
            let s = self.syll.step(rng, dt, 0.18);
            let v = self.vowel.step(rng, dt, 0.5);
            self.talk_f[0].set(350.0 + 500.0 * v, 3.0, sr);
            self.talk_f[1].set(1_100.0 + 900.0 * (1.0 - v) + 300.0 * s, 4.0, sr);
            self.next_clink -= dt;
            if self.next_clink <= 0.0 {
                self.next_clink = rng.range(0.6, 3.5);
                let f = rng.range(2_600.0, 3_400.0);
                self.clink_hz = [f, f * 1.47, f * 2.09];
                self.clink.trigger(rng.range(0.5, 1.0), 0.18, sr);
            }
        }
        if self.kind == 2 {
            let (f1, f2) = self.formant_now();
            self.formant[0].set(f1, 6.0, sr);
            self.formant[1].set(f2, 8.0, sr);
            self.sing_hz += one_pole(0.03, 1.0 / dt) * (self.sing_to - self.sing_hz);
            let want = self.sing_on;
            self.sing_amp += one_pole(0.04, 1.0 / dt) * (want - self.sing_amp);
        }
    }

    fn formant_now(&self) -> (f32, f32) {
        VOWELS[(self.step.max(0) as usize / 2) % VOWELS.len()]
    }

    /// A new sixteenth: the patterns.
    fn on_step(&mut self, step: i32, rng: &mut Rng, sr: f32) {
        let s = step.rem_euclid(16);
        let bar = step.div_euclid(16);
        let chord = CHORDS[bar.rem_euclid(4) as usize];
        let root = self.key * semi(chord[0]);
        match self.kind {
            0 => {
                if s % 4 == 0 {
                    self.kick.trigger(1.0, 0.16, sr);
                    self.kick_ph = Phase::default();
                }
                if s % 8 == 4 {
                    self.snare.trigger(0.45, 0.07, sr);
                }
                if s % 4 == 2 {
                    self.hat.trigger(0.5, 0.03, sr);
                    self.bass.trigger(1.0, 0.12, sr);
                    self.bass_hz = root * if s == 14 { 1.5 } else { 1.0 };
                }
            }
            1 => {
                if s % 8 == 0 {
                    self.kick.trigger(0.4, 0.12, sr);
                    self.kick_ph = Phase::default();
                }
                if s == 0 || s == 8 {
                    self.set_pad(chord, 2.0);
                    self.pad.trigger(0.6, 1.2, sr);
                }
            }
            2 => {
                if s % 8 == 0 {
                    self.kick.trigger(0.7, 0.14, sr);
                    self.kick_ph = Phase::default();
                }
                if s % 8 == 4 {
                    self.snare.trigger(0.35, 0.08, sr);
                }
                if s % 4 == 0 {
                    self.bass.trigger(0.8, 0.25, sr);
                    self.bass_hz = root;
                }
                if s == 0 {
                    self.set_pad(chord, 2.0);
                    self.pad.trigger(0.7, 1.8, sr);
                }
                // The singer: a phrase of three bars, a bar's breath.
                let singing = bar.rem_euclid(4) != 3;
                if s % 2 == 0 {
                    if singing && rng.chance(0.75) {
                        let tones = [chord[0], chord[1], chord[2], chord[0] + 2.0, chord[2] + 2.0];
                        let n = tones[(rng.next_u32() % tones.len() as u32) as usize];
                        // A little off key: it is karaoke.
                        let off = rng.range(-0.35, 0.35);
                        self.sing_to = self.key * 4.0 * semi(n + off);
                        self.sing_on = 1.0;
                    } else if !singing {
                        self.sing_on = 0.0;
                    }
                }
            }
            _ => {
                if s % 8 == 0 {
                    self.kick.trigger(0.9, 0.15, sr);
                    self.kick_ph = Phase::default();
                }
                if s % 8 == 4 {
                    self.snare.trigger(0.6, 0.1, sr);
                }
                if s % 2 == 0 {
                    self.hat.trigger(0.3, 0.025, sr);
                }
                if s % 4 == 0 {
                    // A walking bass: root, third, fifth, sixth.
                    let walk = [0.0, 4.0, 7.0, 9.0][(s / 4) as usize];
                    self.bass.trigger(0.9, 0.3, sr);
                    self.bass_hz = root * semi(walk);
                }
                if s % 8 == 6 {
                    self.set_pad(chord, 2.0);
                    self.pad.trigger(0.8, 0.25, sr);
                }
            }
        }
    }

    fn set_pad(&mut self, chord: [f32; 3], octave: f32) {
        for (hz, n) in self.pad_hz.iter_mut().zip(chord) {
            *hz = self.key * octave * semi(n);
        }
    }

    #[inline]
    fn sample(&mut self, rng: &mut Rng, sr: f32) -> f32 {
        self.beats += self.bpm / 60.0 / sr;
        let step = (self.beats * 4.0) as i32;
        if step != self.step {
            self.step = step;
            self.on_step(step, rng, sr);
        }
        let mut x = 0.0;
        if self.kick.live() {
            let e = self.kick.next();
            // The pitch drops as it decays: the "doof".
            let hz = 45.0 + 110.0 * e * e;
            x += 0.8 * e * sine(self.kick_ph.tick(hz, sr));
        }
        if self.snare.live() {
            let e = self.snare.next();
            x += e * (0.8 * self.snare_f.bp(rng.white()) + 0.4 * sine(self.snare_ph.tick(190.0, sr)));
        }
        if self.hat.live() {
            x += 0.5 * self.hat.next() * self.hat_f.hp(rng.white());
        }
        if self.bass.live() {
            let e = self.bass.next().min(0.8) / 0.8;
            let hz = self.bass_hz.max(20.0);
            let p = self.bass_ph.tick(hz, sr);
            let raw = if self.kind == 0 { saw(p, hz / sr) } else { tri(p) };
            x += 0.55 * e * self.bass_f.lp(raw);
        }
        if self.pad.live() {
            let e = self.pad.next();
            let mut s = 0.0;
            for (ph, &hz) in self.pad_ph.iter_mut().zip(self.pad_hz.iter()) {
                let p = ph.tick(hz, sr);
                s += saw(p, hz / sr);
            }
            x += 0.16 * e * self.pad_f.lp(s);
        }
        if self.kind == 2 && self.sing_amp > 1e-3 {
            let vib = 1.0 + 0.012 * sine(self.vib_ph.tick(5.5, sr));
            let hz = self.sing_hz.max(60.0) * vib;
            let src = saw(self.sing_ph.tick(hz, sr), hz / sr);
            x += 0.5 * self.sing_amp * (self.formant[0].bp(src) + 0.6 * self.formant[1].bp(src));
        }
        if self.kind == 1 {
            let n = self.talk_n.next(rng.white());
            let s = self.syll.value.v;
            x += 1.8 * (0.4 + 0.6 * s) * (self.talk_f[0].bp(n) + 0.7 * self.talk_f[1].bp(n));
            if self.clink.live() {
                let e = self.clink.next();
                let mut c = 0.0;
                for (ph, &hz) in self.clink_ph.iter_mut().zip(self.clink_hz.iter()) {
                    c += sine(ph.tick(hz, sr));
                }
                x += 0.12 * e * c;
            }
        }
        self.wall.lp(x)
    }
}

/// A car horn: two notes a third apart, a tap, a double beep or, angry, a
/// long lean on it.
#[derive(Clone, Debug, Default)]
struct Horn {
    t: f32,
    /// Sounding spans (start, end), seconds.
    spans: [(f32, f32); 2],
    len: f32,
    hz: f32,
    ph: [Phase; 2],
    f: Svf,
}

impl Horn {
    fn new(anger: f32, rng: &mut Rng, sr: f32) -> Self {
        let a = anger.clamp(0.0, 1.0);
        let first = rng.range(0.12, 0.3) + a * rng.range(0.0, 1.1);
        let spans = if rng.chance(0.35) && a < 0.8 {
            let gap = rng.range(0.08, 0.14);
            let second = rng.range(0.12, 0.25);
            [(0.0, first.min(0.3)), (first.min(0.3) + gap, first.min(0.3) + gap + second)]
        } else {
            [(0.0, first), (0.0, 0.0)]
        };
        let len = spans[0].1.max(spans[1].1) + 0.05;
        let mut f = Svf::default();
        f.set(rng.range(800.0, 1_200.0), 0.8, sr);
        Horn { t: 0.0, spans, len, hz: rng.range(380.0, 470.0), ph: Default::default(), f }
    }

    #[inline]
    fn sample(&mut self, sr: f32) -> f32 {
        let t = self.t;
        self.t += 1.0 / sr;
        let mut env = 0.0f32;
        for &(a, b) in &self.spans {
            if b > a && t >= a && t < b + 0.03 {
                let att = ((t - a) / 0.008).min(1.0);
                let rel = if t > b { 1.0 - (t - b) / 0.03 } else { 1.0 };
                env = env.max(att * rel);
            }
        }
        if env <= 0.0 {
            return 0.0;
        }
        let (h1, h2) = (self.hz, self.hz * 1.26);
        let s = pulse(self.ph[0].tick(h1, sr), h1 / sr, 0.4) + 0.8 * pulse(self.ph[1].tick(h2, sr), h2 / sr, 0.4);
        warm(self.f.bp(s), 0.8) * env * 1.1
    }

    fn done(&self) -> bool {
        self.t >= self.len
    }
}

/// Thunder: a near strike's crack and tearing, then the rumble rolling in
/// swells; a far one only the low roll. Wide (two noises).
#[derive(Clone, Debug, Default)]
struct Thunder {
    t: f32,
    len: f32,
    crack: Decay,
    crack_f: Svf,
    tear: f32,
    rumble_n: [Brown; 2],
    rumble_f: [Svf; 2],
    swells: [(f32, f32, f32); 6],
    env: f32,
}

impl Thunder {
    fn new(near: f32, rng: &mut Rng, sr: f32) -> Self {
        let near = near.clamp(0.0, 1.0);
        let mut th = Thunder::default();
        let mut start = if near > 0.5 { 0.05 } else { rng.range(0.1, 0.6) };
        for s in &mut th.swells {
            let tau = rng.range(0.6, 2.2) * (1.4 - 0.5 * near);
            *s = (start, rng.range(0.4, 1.0), tau);
            start += rng.range(0.3, 1.6);
        }
        th.swells[0].1 = 1.0;
        th.len = th.swells.iter().map(|s| s.0 + 5.0 * s.2).fold(0.0, f32::max).min(14.0);
        if near > 0.35 {
            th.crack.trigger(near, 0.06, sr);
            th.tear = 0.45 * near;
        }
        th.crack_f.set(2_500.0, 0.7, sr);
        let cut = 80.0 + 360.0 * near;
        for f in &mut th.rumble_f {
            f.set(cut, 0.7, sr);
        }
        th
    }

    fn block(&mut self, dt: f32) {
        let t = self.t;
        let mut e = 0.0f32;
        for &(s, a, tau) in &self.swells {
            if t > s {
                let u = t - s;
                e += a * (1.0 - (-u / 0.12).exp()) * (-u / tau).exp();
            }
        }
        // Fade the last second so it never cuts off.
        let tail = ((self.len - t) / 1.0).clamp(0.0, 1.0);
        self.env = e.min(1.0) * tail;
        self.t += dt;
    }

    #[inline]
    fn sample(&mut self, rng: &mut Rng) -> (f32, f32) {
        let mut mid = 0.0;
        if self.crack.live() {
            mid += 0.6 * self.crack.next() * self.crack_f.hp(rng.white());
        }
        if self.tear > 1e-3 && self.t < 0.6 {
            // Tearing: sparse crackle in the first half second.
            if rng.chance(0.004) {
                mid += self.tear * rng.white();
            }
        }
        let l = self.rumble_f[0].lp(self.rumble_n[0].next(rng.white()));
        let r = self.rumble_f[1].lp(self.rumble_n[1].next(rng.white()));
        let e = self.env * 0.5;
        (mid + e * 0.5 * (l + r), e * 0.5 * (l - r))
    }

    fn done(&self) -> bool {
        self.t >= self.len
    }
}

#[derive(Clone, Debug)]
enum Gen {
    Train(Box<Train>),
    Siren(Box<Siren>),
    Works(Box<Works>),
    Venue(Box<Venue>),
    Horn(Box<Horn>),
    Thunder(Box<Thunder>),
}

/// One spot sound playing.
#[derive(Clone, Debug)]
pub struct SpotVoice {
    pub id: u64,
    pub kind: SpotKind,
    target: SpotUpdate,
    gain: f32,
    gain_from: f32,
    pan: f32,
    pitch: f32,
    cut: f32,
    /// Seconds since the last update (held voices).
    since: f32,
    /// Seconds before a one-shot starts.
    delay: f32,
    lp: [Svf; 2],
    gen: Gen,
    rng: Rng,
}

impl SpotVoice {
    pub fn new(u: SpotUpdate, rng: &mut Rng, sr: f32) -> Self {
        let mut r = Rng::new(rng.next_u32() ^ (u.id as u32).wrapping_mul(0x9e37_79b9));
        let gen = match u.kind {
            SpotKind::Train | SpotKind::Metro => Gen::Train(Box::new(Train { metro: u.kind == SpotKind::Metro, speed: u.speed, to_clack: r.range(0.0, 5.0), ..Default::default() })),
            SpotKind::Siren => Gen::Siren(Box::new(Siren { variant: (u.id % 3) as u8, t: r.range(0.0, 6.0), next_horn: r.range(1.0, 4.0), ..Default::default() })),
            SpotKind::Works => Gen::Works(Box::new(Works::new(&mut r, sr))),
            SpotKind::Club => Gen::Venue(Box::new(Venue::new(0, &mut r, sr))),
            SpotKind::Bar => Gen::Venue(Box::new(Venue::new(1, &mut r, sr))),
            SpotKind::Karaoke => Gen::Venue(Box::new(Venue::new(2, &mut r, sr))),
            SpotKind::MusicHall => Gen::Venue(Box::new(Venue::new(3, &mut r, sr))),
            SpotKind::Horn => Gen::Horn(Box::new(Horn::new(u.intensity, &mut r, sr))),
            SpotKind::Thunder => Gen::Thunder(Box::new(Thunder::new(u.intensity, &mut r, sr))),
        };
        let one = u.kind.one_shot();
        Self {
            id: u.id,
            kind: u.kind,
            target: u,
            // Held sounds fade in; one-shots start at their level.
            gain: if one { u.gain } else { 0.0 },
            gain_from: if one { u.gain } else { 0.0 },
            pan: u.pan,
            pitch: u.pitch,
            cut: cutoff(&u),
            since: 0.0,
            delay: if one { u.delay.max(0.0) } else { 0.0 },
            lp: Default::default(),
            gen,
            rng: r,
        }
    }

    /// A new update for a held voice.
    pub fn update(&mut self, u: SpotUpdate) {
        self.target = u;
        self.since = 0.0;
    }

    /// Roughly how loud it is now (for stealing the quietest).
    pub fn loudness(&self) -> f32 {
        self.gain.max(if self.since < STALE { self.target.gain } else { 0.0 })
    }

    pub fn done(&self) -> bool {
        match &self.gen {
            Gen::Horn(h) => h.done(),
            Gen::Thunder(t) => t.done(),
            _ => self.since >= STALE && self.gain < 1e-4,
        }
    }

    /// Mix one block (at most [`BLOCK`] frames) into `out`.
    fn render_block(&mut self, out: &mut [f32], sr: f32) {
        let n = out.len() / 2;
        let dt = n as f32 / sr;
        if self.delay > 0.0 {
            self.delay -= dt;
            return;
        }
        let held = !self.kind.one_shot();
        self.since += dt;
        let t = self.target;
        let goal = if held && self.since >= STALE { 0.0 } else { t.gain.max(0.0) };
        self.gain_from = self.gain;
        if held {
            self.gain += one_pole(TAU_GAIN, 1.0 / dt) * (goal - self.gain);
            self.pan += one_pole(TAU_PAN, 1.0 / dt) * (t.pan - self.pan);
            self.pitch += one_pole(0.4, 1.0 / dt) * (t.pitch - self.pitch);
        }
        self.cut += one_pole(0.3, 1.0 / dt) * (cutoff(&t) - self.cut);
        for f in &mut self.lp {
            f.set(self.cut, 0.6, sr);
        }
        let rng = &mut self.rng;
        match &mut self.gen {
            Gen::Train(g) => g.block(t.speed, rng, sr, dt),
            Gen::Siren(g) => g.block(self.pitch, rng, sr, dt),
            Gen::Works(g) => g.block(t.intensity, rng, sr, dt),
            Gen::Venue(g) => g.block(rng, sr, dt),
            Gen::Horn(_) => {}
            Gen::Thunder(g) => g.block(dt),
        }
        if self.gain < 1e-5 && self.gain_from < 1e-5 {
            // Silent: keep the clocks running, skip the samples.
            return;
        }
        let (pl, pr) = pan(1.0, self.pan);
        let (pl, pr) = (pl * SQRT_2, pr * SQRT_2);
        let inv = 1.0 / n as f32;
        for (k, frame) in out.chunks_exact_mut(2).enumerate() {
            let (mid, side) = match &mut self.gen {
                Gen::Train(g) => (g.sample(rng, sr), 0.0),
                Gen::Siren(g) => (g.sample(sr), 0.0),
                Gen::Works(g) => (g.sample(rng, sr), 0.0),
                Gen::Venue(g) => (g.sample(rng, sr), 0.0),
                Gen::Horn(g) => (g.sample(sr), 0.0),
                Gen::Thunder(g) => g.sample(rng),
            };
            let g = self.gain_from + (self.gain - self.gain_from) * (k as f32 + 1.0) * inv;
            let m = self.lp[0].lp(mid) * g;
            let s = if side != 0.0 { self.lp[1].lp(side) * g } else { 0.0 };
            frame[0] += m * pl + s;
            frame[1] += m * pr - s;
        }
    }
}

/// How dull a source is: air takes the highs off with distance (gently:
/// 16 kHz close, ~3 kHz at 600 m), muffle takes the rest.
pub fn cutoff(u: &SpotUpdate) -> f32 {
    let air = 16_000.0 / (1.0 + u.dist.max(0.0) / 120.0);
    let m = u.muffle.clamp(0.0, 1.0);
    (air * (180.0 / air).powf(m)).clamp(150.0, 16_000.0)
}

/// The echo off the buildings: two taps, each fed back a little, dulled.
#[derive(Clone, Debug)]
struct Echo {
    buf: [Vec<f32>; 2],
    pos: usize,
    taps: [usize; 2],
    damp: [OnePole; 2],
    amount: f32,
    target: f32,
}

impl Echo {
    fn new(sr: f32) -> Self {
        let taps = [(0.113 * sr) as usize, (0.157 * sr) as usize];
        let len = taps[1] + 1;
        Self { buf: [vec![0.0; len], vec![0.0; len]], pos: 0, taps, damp: Default::default(), amount: 0.0, target: 0.0 }
    }

    fn run(&mut self, block: &mut [f32], sr: f32) {
        let n = block.len() / 2;
        self.amount += one_pole(0.5, sr / n as f32) * (self.target - self.amount);
        if self.amount < 1e-4 && self.target < 1e-4 {
            return;
        }
        let a = lp_coef(2_400.0, sr);
        let len = self.buf[0].len();
        let wet = self.amount * 0.32;
        for f in block.chunks_exact_mut(2) {
            let l_tap = self.buf[0][(self.pos + len - self.taps[0]) % len];
            let r_tap = self.buf[1][(self.pos + len - self.taps[1]) % len];
            let l = self.damp[0].lp(l_tap, a);
            let r = self.damp[1].lp(r_tap, a);
            // Cross-fed (ping-pong), fed back at 0.3.
            self.buf[0][self.pos] = f[0] + 0.3 * r;
            self.buf[1][self.pos] = f[1] + 0.3 * l;
            f[0] += wet * l;
            f[1] += wet * r;
            self.pos = (self.pos + 1) % len;
        }
    }
}

/// The spot voices: the pool, the echo and a scratch block.
#[derive(Debug)]
pub struct Spots {
    sr: f32,
    pool: VoicePool<SpotVoice>,
    rng: Rng,
    echo: Echo,
    block: Vec<f32>,
}

impl Spots {
    pub fn new(sr: f32) -> Self {
        Self { sr, pool: VoicePool::new(SPOT_VOICES), rng: Rng::new(0x5907_5907), echo: Echo::new(sr), block: vec![0.0; BLOCK * 2] }
    }

    pub fn len(&self) -> usize {
        self.pool.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }

    /// A held spot's update (a new voice for an id not playing), or a one-shot.
    pub fn apply(&mut self, u: SpotUpdate) {
        if !u.gain.is_finite() {
            return;
        }
        // A new source at nothing is not worth a voice (an old one may fade to it).
        if u.gain <= 0.0 && !self.pool.iter_mut().any(|v| v.id == u.id) {
            return;
        }
        if !u.kind.one_shot() {
            if let Some(v) = self.pool.iter_mut().find(|v| v.id == u.id && v.kind == u.kind) {
                v.update(u);
                return;
            }
        }
        let v = SpotVoice::new(u, &mut self.rng, self.sr);
        self.pool.push(v, SpotVoice::loudness);
    }

    /// How much the buildings echo (`0..1`).
    pub fn set_room(&mut self, room: f32) {
        self.echo.target = if room.is_finite() { room.clamp(0.0, 1.0) } else { 0.0 };
    }

    /// Drop everything (muted).
    pub fn clear(&mut self) {
        self.pool.retain(|_| false);
    }

    /// Mix `out` (interleaved stereo) at bus gain `bus`.
    pub fn render(&mut self, out: &mut [f32], bus: f32) {
        let frames = out.len() / 2;
        let mut i = 0;
        let sr = self.sr;
        while i < frames {
            let n = BLOCK.min(frames - i);
            let block = &mut self.block[..n * 2];
            block.fill(0.0);
            for v in self.pool.iter_mut() {
                v.render_block(block, sr);
            }
            self.echo.run(block, sr);
            let g = bus * SPOT_TRIM;
            for (o, s) in out[i * 2..(i + n) * 2].iter_mut().zip(block.iter()) {
                *o += s * g;
            }
            i += n;
        }
        self.pool.retain(|v| !v.done());
    }

    /// What is playing: kind, id, gain now.
    pub fn playing(&mut self) -> Vec<(SpotKind, u64, f32)> {
        self.pool.iter_mut().map(|v| (v.kind, v.id, v.gain)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn render(spots: &mut Spots, secs: f32, every: Option<&dyn Fn(f32) -> Vec<SpotUpdate>>) -> Vec<f32> {
        let mut out = vec![0.0; (SR * secs) as usize * 2];
        let step = (SR * 0.25) as usize * 2;
        for (i, chunk) in out.chunks_mut(step).enumerate() {
            if let Some(f) = every {
                for u in f(i as f32 * 0.25) {
                    spots.apply(u);
                }
            }
            spots.render(chunk, 1.0);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_kind_sounds_within_level_and_stops_when_its_updates_stop() {
        for kind in SpotKind::ALL {
            let mut s = Spots::new(SR);
            let u = SpotUpdate { speed: 15.0, intensity: 0.9, ..SpotUpdate::new(7, kind, 1.0) };
            let held = !kind.one_shot();
            let feed = move |t: f32| if t < 5.0 && (held || t == 0.0) { vec![u] } else { vec![] };
            let out = render(&mut s, 16.0, Some(&feed));
            let first = &out[..(SR * 5.0) as usize * 2];
            let p = peak(&out);
            assert!(p > 0.02 && p < 0.9, "{kind:?} peak {p}");
            assert!(rms(first) > 0.003, "{kind:?} rms {}", rms(first));
            assert!(out.iter().all(|x| x.is_finite()));
            assert!(s.is_empty(), "{kind:?} still playing");
            let tail = &out[out.len() - (SR as usize) * 2..];
            assert!(peak(tail) < 1e-3, "{kind:?} rings on {}", peak(tail));
        }
    }

    #[test]
    fn a_held_voice_fades_in_and_out_without_a_jump() {
        let mut s = Spots::new(SR);
        let u = SpotUpdate::new(1, SpotKind::Club, 1.0);
        s.apply(u);
        let mut out = vec![0.0; 2 * 4_800];
        s.render(&mut out, 1.0);
        // The first 100 ms: still easing in.
        assert!(peak(&out) < 0.2, "{}", peak(&out));
        let feed = move |t: f32| if t < 2.0 { vec![u] } else { vec![] };
        let out = render(&mut s, 5.0, Some(&feed));
        let jump = out.chunks_exact(2).collect::<Vec<_>>().windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < 0.2, "{jump}");
    }

    #[test]
    fn far_and_muffled_is_duller() {
        let near = SpotUpdate { dist: 10.0, ..SpotUpdate::new(1, SpotKind::Siren, 1.0) };
        let far = SpotUpdate { dist: 900.0, ..near };
        let muffled = SpotUpdate { muffle: 0.8, ..near };
        assert!(cutoff(&near) > 12_000.0);
        assert!(cutoff(&far) < cutoff(&near) * 0.3);
        assert!(cutoff(&muffled) < 1_000.0);
    }

    #[test]
    fn the_pool_holds_its_cap_and_updates_by_id() {
        let mut s = Spots::new(SR);
        for i in 0..50u64 {
            s.apply(SpotUpdate::new(i, SpotKind::Bar, 0.01 * (i as f32 + 1.0)));
        }
        assert_eq!(s.len(), SPOT_VOICES);
        s.apply(SpotUpdate::new(49, SpotKind::Bar, 0.3));
        assert_eq!(s.len(), SPOT_VOICES);
        // The quietest were stolen: the loudest are kept.
        let ids: Vec<u64> = s.playing().iter().map(|p| p.1).collect();
        assert!(ids.contains(&49) && !ids.contains(&0));
    }

    #[test]
    fn twenty_voices_render_cheaply() {
        let mut s = Spots::new(SR);
        let kinds = [SpotKind::Train, SpotKind::Metro, SpotKind::Siren, SpotKind::Works, SpotKind::Club, SpotKind::Bar, SpotKind::Karaoke, SpotKind::MusicHall];
        let ups: Vec<SpotUpdate> = (0..SPOT_VOICES as u64).map(|i| SpotUpdate { speed: 12.0, pan: (i as f32 * 0.37).sin(), ..SpotUpdate::new(i, kinds[i as usize % kinds.len()], 0.05) }).collect();
        s.set_room(0.6);
        let start = std::time::Instant::now();
        let feed = |_t: f32| ups.clone();
        let out = render(&mut s, 10.0, Some(&feed));
        let secs = start.elapsed().as_secs_f32();
        assert!(out.iter().all(|x| x.is_finite()));
        // Debug builds are slow; still well under real time.
        assert!(secs < 10.0, "{secs} s for 10 s of 20 voices");
    }
}
