//! [sound] Spot sounds: what makes a noise near the camera (trains, the
//! metro, sirens, building works, the night's venues, horns in a jam,
//! thunder), which few of them are heard, how loud and where in the
//! stereo. Pure functions and two small clocks: the game hands over plain
//! [`Candidate`]s a few times a second and sends the [`SpotUpdate`]s
//! [`pick`] returns to the engine, which plays them ([`crate::voices`]).
//!
//! The city stays a mix: each kind has its own cap and range, the loudest
//! win, and the sum of what is kept is held to one loud source's worth
//! ([`BUDGET`]), so a busy district is livelier, not louder.

use crate::dsp::{smoothstep, Rng};
use crate::engine::{distance_gain, SPOT_VOICES};

/// A kind of spot sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpotKind {
    /// A surface train: rumble, wheel clacks, the motors' whine.
    Train,
    /// A metro train below: a deep rumble through the ground (clear in the
    /// Underground view).
    Metro,
    /// A police car, ambulance or fire engine on a call.
    Siren,
    /// Building works: hammering, a breaker, a digger's engine, a reversing beeper.
    Works,
    /// A club at night: the kick and bass through the walls.
    Club,
    /// A bar: talk, glasses, a jukebox.
    Bar,
    /// Karaoke: a backing track and someone singing, a little off key.
    Karaoke,
    /// A music hall: a live band.
    MusicHall,
    /// A car horn in a jam (one-shot).
    Horn,
    /// Thunder (one-shot).
    Thunder,
}

/// The groups the Settings window has a slider for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Trains and the metro.
    Transit,
    Sirens,
    /// Building works.
    Works,
    /// Clubs, bars, karaoke, music halls.
    Nightlife,
    /// Horns in jams.
    Horns,
    /// Thunder.
    Weather,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Transit, Group::Sirens, Group::Works, Group::Nightlife, Group::Horns, Group::Weather];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            Group::Transit => "transit",
            Group::Sirens => "sirens",
            Group::Works => "works",
            Group::Nightlife => "nightlife",
            Group::Horns => "horns",
            Group::Weather => "weather",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::Transit => "Trains and metro",
            Group::Sirens => "Sirens",
            Group::Works => "Building works",
            Group::Nightlife => "Nightlife",
            Group::Horns => "Horns",
            Group::Weather => "Thunder",
        }
    }

    pub fn parse(s: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.key().eq_ignore_ascii_case(s.trim()))
    }
}

/// How a kind is heard: full inside `near` metres, silent past `far`, at
/// `level` (1 = a train going by close), at most `max` at once.
#[derive(Clone, Copy, Debug)]
pub struct KindSpec {
    pub group: Group,
    pub near: f32,
    pub far: f32,
    pub level: f32,
    pub max: usize,
}

impl SpotKind {
    pub const ALL: [SpotKind; 10] =
        [SpotKind::Train, SpotKind::Metro, SpotKind::Siren, SpotKind::Works, SpotKind::Club, SpotKind::Bar, SpotKind::Karaoke, SpotKind::MusicHall, SpotKind::Horn, SpotKind::Thunder];

    pub fn key(self) -> &'static str {
        match self {
            SpotKind::Train => "train",
            SpotKind::Metro => "metro",
            SpotKind::Siren => "siren",
            SpotKind::Works => "works",
            SpotKind::Club => "club",
            SpotKind::Bar => "bar",
            SpotKind::Karaoke => "karaoke",
            SpotKind::MusicHall => "music_hall",
            SpotKind::Horn => "horn",
            SpotKind::Thunder => "thunder",
        }
    }

    pub fn parse(s: &str) -> Option<SpotKind> {
        SpotKind::ALL.into_iter().find(|k| k.key().eq_ignore_ascii_case(s.trim()))
    }

    /// One-shots play once and end; the rest are held by updates.
    pub fn one_shot(self) -> bool {
        matches!(self, SpotKind::Horn | SpotKind::Thunder)
    }

    pub fn spec(self) -> KindSpec {
        let s = |group, near, far, level, max| KindSpec { group, near, far, level, max };
        match self {
            SpotKind::Train => s(Group::Transit, 40.0, 900.0, 1.0, 3),
            SpotKind::Metro => s(Group::Transit, 30.0, 450.0, 0.8, 2),
            SpotKind::Siren => s(Group::Sirens, 35.0, 1_600.0, 0.62, 3),
            SpotKind::Works => s(Group::Works, 25.0, 420.0, 0.6, 3),
            SpotKind::Club => s(Group::Nightlife, 20.0, 380.0, 0.75, 2),
            SpotKind::Bar => s(Group::Nightlife, 12.0, 200.0, 0.55, 3),
            SpotKind::Karaoke => s(Group::Nightlife, 15.0, 240.0, 0.6, 2),
            SpotKind::MusicHall => s(Group::Nightlife, 25.0, 450.0, 0.7, 1),
            SpotKind::Horn => s(Group::Horns, 15.0, 500.0, 0.42, 4),
            SpotKind::Thunder => s(Group::Weather, 1.0, f32::INFINITY, 1.6, 2),
        }
    }
}

/// What the engine is told about one spot sound: its gain (distance,
/// slider and loudness budget included), where it sits in the stereo, how
/// muffled and how far (far is duller), and kind-specific `speed`
/// (m/s, trains) and `intensity` (`0..1`: a jam's anger, a thunderclap's
/// nearness). `pitch` is the Doppler shift; `delay` holds a one-shot back
/// (seconds). `id` ties the updates of one source together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotUpdate {
    pub id: u64,
    pub kind: SpotKind,
    pub gain: f32,
    pub pan: f32,
    pub dist: f32,
    pub muffle: f32,
    pub speed: f32,
    pub intensity: f32,
    pub pitch: f32,
    pub delay: f32,
}

impl SpotUpdate {
    pub fn new(id: u64, kind: SpotKind, gain: f32) -> Self {
        Self { id, kind, gain, pan: 0.0, dist: 30.0, muffle: 0.0, speed: 0.0, intensity: 1.0, pitch: 1.0, delay: 0.0 }
    }
}

/// Something in the city that could be heard: its kind, a stable id,
/// where it is (x, height, z in metres, the game's axes: y up), its
/// velocity (for the Doppler shift) and how strongly it sounds (`0..1`:
/// a venue's crowd, a works' activity, a jam's anger).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub id: u64,
    pub kind: SpotKind,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub intensity: f32,
}

impl Candidate {
    pub fn at(id: u64, kind: SpotKind, x: f32, y: f32, z: f32) -> Self {
        Self { id, kind, pos: [x, y, z], vel: [0.0; 3], intensity: 1.0 }
    }
}

/// Where the ears are: the camera `eye`, the spot it looks at (`focus`,
/// on the ground), the camera's right (for the stereo), its height over
/// the ground, and whether the Underground view is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub eye: [f32; 3],
    pub focus: [f32; 3],
    pub right: [f32; 3],
    pub height: f32,
    pub underground: bool,
}

impl Default for Listener {
    fn default() -> Self {
        Self { eye: [0.0, 2.0, 0.0], focus: [0.0, 0.0, 0.0], right: [1.0, 0.0, 0.0], height: 2.0, underground: false }
    }
}

/// The group sliders as gains (from [`crate::mix::slider_gain`]).
pub type GroupGains = [f32; 6];

pub const ALL_ON: GroupGains = [1.0; 6];

/// The kept spot sounds' gains are scaled down together when their summed
/// power passes this (one close train is 1): many sources, one mix.
pub const BUDGET: f32 = 1.0;

/// At most this many held (not one-shot) spots, leaving voices for horns
/// and thunder.
pub const HELD_MAX: usize = SPOT_VOICES - 4;

/// The speed of sound, m/s.
const SOUND: f32 = 343.0;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where a source sits for the ears: its distance (the "heard" distance:
/// horizontal from the spot looked at, with half the camera's height on
/// top, so zooming out turns the street down without silencing it), its
/// pan, the extra muffle of being behind the camera, and the Doppler shift.
pub fn place(c: &Candidate, l: &Listener) -> (f32, f32, f32, f32) {
    let d = sub(c.pos, l.focus);
    let dh2 = d[0] * d[0] + d[2] * d[2];
    // Height counts half: the ears lean in towards what is looked at.
    let h = 0.5 * l.height.max(0.0) + 1.5;
    let dy = c.pos[1] - l.focus[1];
    let dist = (dh2 + h * h + 0.25 * dy * dy).sqrt();
    let right = l.right;
    let rl = (right[0] * right[0] + right[2] * right[2]).sqrt().max(1e-6);
    let x = (d[0] * right[0] + d[2] * right[2]) / rl;
    let pan = (x / dist.max(1.0)).clamp(-1.0, 1.0) * 0.9;
    // Behind the camera: a little duller (the only front/back cue stereo has).
    let fwd = sub(l.focus, l.eye);
    let fl = (fwd[0] * fwd[0] + fwd[2] * fwd[2]).sqrt();
    let behind = if fl > 1e-3 && dh2 > 1.0 {
        let cos = (d[0] * fwd[0] + d[2] * fwd[2]) / (fl * dh2.sqrt());
        0.25 * smoothstep(0.0, -0.8, cos)
    } else {
        0.0
    };
    // Doppler: from the ears to the source; moving away lowers the pitch.
    let to = sub(c.pos, l.eye);
    let tl = dot(to, to).sqrt().max(1.0);
    let v_r = dot(c.vel, to) / tl;
    let pitch = (SOUND / (SOUND + v_r)).clamp(0.88, 1.12);
    (dist, pan, behind, pitch)
}

/// The spot sounds to hold now, loudest first: each candidate at its
/// distance, slider and intensity, the quiet and far dropped, each kind to
/// its cap, at most [`HELD_MAX`], and the lot held to [`BUDGET`].
/// One-shot kinds among the candidates are ignored (see [`HornClock`],
/// [`ThunderClock`]).
pub fn pick(cands: &[Candidate], l: &Listener, groups: &GroupGains) -> Vec<SpotUpdate> {
    let mut out: Vec<SpotUpdate> = Vec::new();
    for c in cands {
        if c.kind.one_shot() {
            continue;
        }
        let spec = c.kind.spec();
        let slider = groups[spec.group.index()].clamp(0.0, 1.0);
        if slider <= 0.0 {
            continue;
        }
        let (dist, pan, behind, pitch) = place(c, l);
        let g = distance_gain(dist, spec.near, spec.far) * spec.level * c.intensity.clamp(0.0, 1.0) * slider;
        // Under -60 dB of a close train: not worth a voice.
        if g < 1e-3 {
            continue;
        }
        // The metro is felt through the ground from the street; heard
        // plainly in the Underground view, where the surface is muffled.
        let muffle = match (c.kind, l.underground) {
            (SpotKind::Metro, false) => 0.75,
            (SpotKind::Metro, true) => 0.0,
            (_, true) => 0.7,
            _ => 0.0,
        };
        // Through the ground it is half as loud as in its tunnel.
        let g = if c.kind == SpotKind::Metro && !l.underground { g * 0.5 } else { g };
        if g < 1e-3 {
            continue;
        }
        let speed = (c.vel[0] * c.vel[0] + c.vel[2] * c.vel[2]).sqrt();
        let pitch = if matches!(c.kind, SpotKind::Train | SpotKind::Metro | SpotKind::Siren) { pitch } else { 1.0 };
        out.push(SpotUpdate { id: c.id, kind: c.kind, gain: g, pan, dist, muffle: (muffle + behind).min(0.95), speed, intensity: c.intensity.clamp(0.0, 1.0), pitch, delay: 0.0 });
    }
    out.sort_by(|a, b| b.gain.total_cmp(&a.gain).then(a.id.cmp(&b.id)));
    let mut count = [0usize; SpotKind::ALL.len()];
    out.retain(|u| {
        let i = u.kind as usize;
        count[i] += 1;
        count[i] <= u.kind.spec().max
    });
    out.truncate(HELD_MAX);
    let power: f32 = out.iter().map(|u| u.gain * u.gain).sum();
    if power.sqrt() > BUDGET {
        let k = BUDGET / power.sqrt();
        for u in &mut out {
            u.gain *= k;
        }
    }
    out
}

/// Decides when the cars stuck in a jam lean on their horns: each jam near
/// the camera honks now and then, more the angrier it is, never more than
/// [`HornClock::MAX_PER_SECOND`] in all. Its own dice (render-side only).
#[derive(Clone, Debug)]
pub struct HornClock {
    rng: Rng,
    next_id: u64,
}

impl Default for HornClock {
    fn default() -> Self {
        Self { rng: Rng::new(0x40c4_0c4a), next_id: 1 << 48 }
    }
}

impl HornClock {
    pub const MAX_PER_SECOND: f32 = 1.6;

    /// Honks per second for one jam of this anger.
    pub fn rate(anger: f32) -> f32 {
        let a = anger.clamp(0.0, 1.0);
        0.04 + 0.5 * a * a
    }

    /// The horns over the next `dt` seconds from `jams` (candidates of
    /// kind [`SpotKind::Horn`], intensity = anger), each with a delay
    /// inside `dt` so they do not land on the sampling beat.
    pub fn tick(&mut self, jams: &[Candidate], l: &Listener, groups: &GroupGains, dt: f32) -> Vec<SpotUpdate> {
        let spec = SpotKind::Horn.spec();
        let slider = groups[spec.group.index()].clamp(0.0, 1.0);
        let mut out = Vec::new();
        if slider <= 0.0 || dt <= 0.0 {
            return out;
        }
        let heard: Vec<(&Candidate, f32, f32, f32)> = jams
            .iter()
            .filter(|c| c.kind == SpotKind::Horn)
            .filter_map(|c| {
                let (dist, pan, behind, _) = place(c, l);
                let g = distance_gain(dist, spec.near, spec.far);
                (g > 0.01).then_some((c, dist, pan, behind + if l.underground { 0.7 } else { 0.0 }))
            })
            .collect();
        let total: f32 = heard.iter().map(|(c, ..)| Self::rate(c.intensity)).sum();
        let scale = if total > Self::MAX_PER_SECOND { Self::MAX_PER_SECOND / total } else { 1.0 };
        for (c, dist, pan, muffle) in heard {
            let p = 1.0 - (-Self::rate(c.intensity) * scale * dt).exp();
            if self.rng.chance(p) {
                self.next_id += 1;
                let a = c.intensity.clamp(0.0, 1.0);
                let gain = distance_gain(dist, spec.near, spec.far) * spec.level * slider * (0.6 + 0.4 * a) * self.rng.range(0.7, 1.0);
                out.push(SpotUpdate { id: self.next_id, kind: SpotKind::Horn, gain, pan, dist, muffle: muffle.min(0.95), speed: 0.0, intensity: a, pitch: 1.0, delay: self.rng.range(0.0, dt) });
            }
        }
        out
    }
}

/// Thunder in a storm: strikes now and then at random distances, the near
/// ones rare, each heard after the light (distance / the speed of sound).
/// `flash` gives the strikes it decides on to the renderer if it wants them.
#[derive(Clone, Debug)]
pub struct ThunderClock {
    rng: Rng,
    next_id: u64,
}

impl Default for ThunderClock {
    fn default() -> Self {
        Self { rng: Rng::new(0x7b0d_e125), next_id: 1 << 56 }
    }
}

/// A lightning strike: how far (metres) and in which direction (pan).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub dist: f32,
    pub pan: f32,
}

impl ThunderClock {
    /// Strikes per second at full storm.
    pub const RATE: f32 = 0.11;

    /// Strikes over the next `dt` seconds at `storm` (`0..1`); with each,
    /// its thunder (delayed by distance, at most 6 s).
    pub fn tick(&mut self, storm: f32, groups: &GroupGains, underground: bool, dt: f32) -> Vec<(Strike, SpotUpdate)> {
        let storm = storm.clamp(0.0, 1.0);
        let slider = groups[Group::Weather.index()].clamp(0.0, 1.0);
        let mut out = Vec::new();
        if storm < 0.05 || dt <= 0.0 {
            return out;
        }
        let p = 1.0 - (-Self::RATE * storm * storm * dt).exp();
        if self.rng.chance(p) {
            // Mostly kilometres away; one in eight or so close by.
            let u = self.rng.unit();
            let dist = 250.0 + 7_000.0 * u * u;
            let strike = Strike { dist, pan: self.rng.range(-0.8, 0.8) };
            let flash = self.rng.range(0.0, dt);
            out.push((strike, self.thunder(strike, storm, slider, underground, flash)));
        }
        out
    }

    /// The thunder of one strike (also for a strike the renderer decided).
    pub fn thunder(&mut self, s: Strike, storm: f32, slider: f32, underground: bool, flash_delay: f32) -> SpotUpdate {
        self.next_id += 1;
        let near = (-s.dist / 1_500.0).exp();
        let spec = SpotKind::Thunder.spec();
        // Loud enough far off to be heard over the rain: thunder carries.
        let gain = spec.level * slider * (0.35 + 0.65 * near) * (0.6 + 0.4 * storm.clamp(0.0, 1.0));
        SpotUpdate {
            id: self.next_id,
            kind: SpotKind::Thunder,
            gain,
            pan: s.pan * (0.4 + 0.6 * near),
            dist: s.dist,
            muffle: if underground { 0.8 } else { 0.0 },
            speed: 0.0,
            intensity: near,
            pitch: 1.0,
            delay: flash_delay + (s.dist / SOUND).min(6.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street() -> Listener {
        Listener { eye: [0.0, 20.0, -30.0], focus: [0.0, 0.0, 0.0], right: [-1.0, 0.0, 0.0], height: 20.0, underground: false }
    }

    #[test]
    fn closer_is_louder_and_the_stereo_follows_the_camera() {
        let l = street();
        let near = Candidate::at(1, SpotKind::Train, 20.0, 0.0, 0.0);
        let far = Candidate::at(2, SpotKind::Train, 300.0, 0.0, 0.0);
        let picked = pick(&[near, far], &l, &ALL_ON);
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].id, 1);
        assert!(picked[0].gain > picked[1].gain * 3.0, "{picked:?}");
        // The camera looks along +z with its right towards -x: +x is on the left.
        assert!(picked[0].pan < -0.3, "{picked:?}");
        let right = pick(&[Candidate::at(3, SpotKind::Train, -20.0, 0.0, 0.0)], &l, &ALL_ON);
        assert!(right[0].pan > 0.3);
        // Straight ahead: centred.
        let ahead = pick(&[Candidate::at(4, SpotKind::Train, 0.0, 0.0, 50.0)], &l, &ALL_ON);
        assert!(ahead[0].pan.abs() < 0.05);
        // Behind the camera: duller.
        let behind = pick(&[Candidate::at(5, SpotKind::Train, 0.0, 0.0, -80.0)], &l, &ALL_ON);
        assert!(behind[0].muffle > 0.1 && ahead[0].muffle == 0.0);
    }

    #[test]
    fn zooming_out_turns_spots_down_without_cutting_them() {
        let c = [Candidate::at(1, SpotKind::Siren, 0.0, 0.0, 0.0)];
        let at = |h: f32| pick(&c, &Listener { eye: [0.0, h, -h], height: h, ..street() }, &ALL_ON).first().map_or(0.0, |u| u.gain);
        let (g3, g100, g600, g3000) = (at(3.0), at(100.0), at(600.0), at(4_000.0));
        assert!(g3 > g100 && g100 > g600 && g600 > g3000, "{g3} {g100} {g600} {g3000}");
        assert!(g600 > 0.05, "a siren is still heard from 600 m up: {g600}");
        assert_eq!(g3000, 0.0);
    }

    #[test]
    fn caps_per_kind_the_voice_limit_and_the_budget_keep_it_a_mix() {
        let l = street();
        let mut cands = Vec::new();
        for i in 0..40u64 {
            let a = i as f32 * 0.7;
            cands.push(Candidate::at(i, SpotKind::Club, 15.0 * a.cos(), 0.0, 15.0 * a.sin()));
            cands.push(Candidate::at(100 + i, SpotKind::Bar, 12.0 * a.cos(), 0.0, 12.0 * a.sin()));
            cands.push(Candidate::at(200 + i, SpotKind::Works, 20.0 * a.cos(), 0.0, 20.0 * a.sin()));
            cands.push(Candidate::at(300 + i, SpotKind::Train, 30.0 * a.cos(), 0.0, 30.0 * a.sin()));
        }
        let picked = pick(&cands, &l, &ALL_ON);
        assert!(picked.len() <= HELD_MAX);
        for k in SpotKind::ALL {
            assert!(picked.iter().filter(|u| u.kind == k).count() <= k.spec().max, "{k:?}");
        }
        let power: f32 = picked.iter().map(|u| u.gain * u.gain).sum();
        assert!(power.sqrt() <= BUDGET + 1e-4, "{power}");
        // One source alone is not turned down by the budget.
        let one = pick(&cands[..1], &l, &ALL_ON);
        assert!((one[0].gain - distance_gain(place(&cands[0], &l).0, 20.0, 380.0) * SpotKind::Club.spec().level).abs() < 1e-5);
    }

    #[test]
    fn sliders_and_intensity_scale_and_zero_drops() {
        let l = street();
        let c = Candidate { intensity: 0.5, ..Candidate::at(1, SpotKind::Bar, 5.0, 0.0, 5.0) };
        let full = pick(&[c], &l, &ALL_ON)[0].gain;
        let mut g = ALL_ON;
        g[Group::Nightlife.index()] = 0.25;
        assert!((pick(&[c], &l, &g)[0].gain - full * 0.25).abs() < 1e-6);
        g[Group::Nightlife.index()] = 0.0;
        assert!(pick(&[c], &l, &g).is_empty());
        let off = Candidate { intensity: 0.0, ..c };
        assert!(pick(&[off], &l, &ALL_ON).is_empty());
    }

    #[test]
    fn the_metro_is_muffled_from_the_street_and_clear_underground() {
        let c = [Candidate::at(1, SpotKind::Metro, 10.0, -12.0, 10.0), Candidate::at(2, SpotKind::Club, -10.0, 0.0, 10.0)];
        let up = pick(&c, &street(), &ALL_ON);
        let down = pick(&c, &Listener { underground: true, ..street() }, &ALL_ON);
        let m = |v: &[SpotUpdate], id| v.iter().find(|u| u.id == id).unwrap().muffle;
        assert!(m(&up, 1) > 0.5 && m(&down, 1) == 0.0);
        assert!(m(&up, 2) == 0.0 && m(&down, 2) > 0.5);
    }

    #[test]
    fn doppler_raises_an_approaching_siren_and_lowers_a_leaving_one() {
        let l = street();
        let coming = Candidate { vel: [-15.0, 0.0, 0.0], ..Candidate::at(1, SpotKind::Siren, 100.0, 0.0, 0.0) };
        let going = Candidate { vel: [15.0, 0.0, 0.0], ..coming };
        assert!(place(&coming, &l).3 > 1.03 && place(&going, &l).3 < 0.97);
        // Venues do not shift.
        let bar = Candidate { vel: [-30.0, 0.0, 0.0], ..Candidate::at(2, SpotKind::Bar, 10.0, 0.0, 0.0) };
        assert_eq!(pick(&[bar], &l, &ALL_ON)[0].pitch, 1.0);
    }

    #[test]
    fn horns_come_more_from_angry_jams_and_never_flood() {
        let l = street();
        let count = |anger: f32, n: u64| {
            let jams: Vec<Candidate> = (0..n).map(|i| Candidate { intensity: anger, ..Candidate::at(i, SpotKind::Horn, 10.0 + i as f32, 0.0, 20.0) }).collect();
            let mut clock = HornClock::default();
            (0..2_400).map(|_| clock.tick(&jams, &l, &ALL_ON, 0.25).len()).sum::<usize>() as f32 / 600.0
        };
        let calm = count(0.1, 1);
        let angry = count(1.0, 1);
        assert!(angry > calm * 4.0, "{calm} {angry}");
        let crowd = count(1.0, 200);
        assert!(crowd <= HornClock::MAX_PER_SECOND * 1.15, "{crowd} a second");
        let mut g = ALL_ON;
        g[Group::Horns.index()] = 0.0;
        let jams = [Candidate { intensity: 1.0, ..Candidate::at(1, SpotKind::Horn, 5.0, 0.0, 5.0) }];
        assert!((0..400).all(|_| HornClock::default().tick(&jams, &l, &g, 0.25).is_empty()));
    }

    #[test]
    fn thunder_follows_the_storm_and_comes_after_the_flash() {
        let mut clock = ThunderClock::default();
        let strikes = |clock: &mut ThunderClock, storm: f32| (0..4_000).map(|_| clock.tick(storm, &ALL_ON, false, 0.25).len()).sum::<usize>();
        assert_eq!(strikes(&mut clock, 0.0), 0);
        let full = strikes(&mut clock, 1.0);
        let light = strikes(&mut clock, 0.4);
        // ~0.11 a second over 1000 s.
        assert!((60..=170).contains(&full), "{full}");
        assert!(light < full / 2, "{light} vs {full}");
        let near = clock.thunder(Strike { dist: 300.0, pan: 0.5 }, 1.0, 1.0, false, 0.0);
        let far = clock.thunder(Strike { dist: 6_000.0, pan: 0.5 }, 1.0, 1.0, false, 0.0);
        assert!(near.gain > far.gain && near.intensity > far.intensity);
        assert!(near.delay < far.delay && far.delay <= 6.0 && near.delay > 0.5);
    }

    #[test]
    fn kinds_and_groups_parse_back() {
        for k in SpotKind::ALL {
            assert_eq!(SpotKind::parse(k.key()), Some(k));
            assert!(k.spec().near < k.spec().far);
        }
        for g in Group::ALL {
            assert_eq!(Group::parse(g.key()), Some(g));
        }
    }
}
