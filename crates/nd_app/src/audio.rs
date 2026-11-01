//! [sound] The game's end of the sound engine (`nd_audio`): the settings
//! and the Settings window's sliders, what the camera hears (sampled a few
//! times a second from the sim and the view, read only), and the UI cues
//! picked up from what the player did. Render-side only: nothing here
//! writes to the sim or draws on its RNG.
//!
//! `ND_AUDIO=0` runs without opening the sound card (test instances).

mod listen;
mod spots;
mod watch;

pub use spots::SpotReport;

use std::sync::Mutex;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use nd_audio::{Command, Cue, Handle, MixLevels, RateLimiter, Status};
use serde::{Deserialize, Serialize};

use crate::sim::SimSet;

/// What the window does to the sound when it loses focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unfocused {
    /// Silent until it is back (the default).
    #[default]
    Mute,
    /// Turned well down.
    Duck,
    /// Plays on as if focused.
    Keep,
}

impl Unfocused {
    pub const ALL: [Unfocused; 3] = [Unfocused::Mute, Unfocused::Duck, Unfocused::Keep];

    pub fn label(self) -> &'static str {
        match self {
            Unfocused::Mute => "Mute",
            Unfocused::Duck => "Turn down",
            Unfocused::Keep => "Keep playing",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Unfocused::Mute => "mute",
            Unfocused::Duck => "duck",
            Unfocused::Keep => "keep",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.key().eq_ignore_ascii_case(s.trim()) || u.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// The ducked level when the window is out of focus and set to turn down
/// (a gain on the master: -12 dB).
pub const DUCK: f32 = 0.25;

/// [sound] The spot sounds' group sliders (under Effects): each `0..1`,
/// all full by default.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpotLevels {
    pub transit: f32,
    pub sirens: f32,
    pub works: f32,
    pub nightlife: f32,
    pub horns: f32,
    pub weather: f32,
}

impl Default for SpotLevels {
    fn default() -> Self {
        Self { transit: 1.0, sirens: 1.0, works: 1.0, nightlife: 1.0, horns: 1.0, weather: 1.0 }
    }
}

impl SpotLevels {
    /// The slider for a group.
    pub fn get_mut(&mut self, g: nd_audio::Group) -> &mut f32 {
        use nd_audio::Group as G;
        match g {
            G::Transit => &mut self.transit,
            G::Sirens => &mut self.sirens,
            G::Works => &mut self.works,
            G::Nightlife => &mut self.nightlife,
            G::Horns => &mut self.horns,
            G::Weather => &mut self.weather,
        }
    }

    pub fn get(&self, g: nd_audio::Group) -> f32 {
        let mut c = *self;
        *c.get_mut(g)
    }
}

/// [sound] The player's sound settings, saved in `settings.ron` beside the
/// other settings. Every field has a default, so a file from before sound
/// (or with only some of these) loads.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub master: f32,
    pub ambience: f32,
    pub effects: f32,
    pub ui: f32,
    /// Music is silent for now (phase 3); the slider is kept.
    pub music: f32,
    pub muted: bool,
    pub unfocused: Unfocused,
    /// [sound] The spot sounds' groups (trains, sirens, works, nightlife,
    /// horns, thunder), each under the Effects slider.
    pub spots: SpotLevels,
}

impl Default for AudioSettings {
    fn default() -> Self {
        let m = MixLevels::default();
        Self { master: m.master, ambience: m.ambience, effects: m.effects, ui: m.ui, music: m.music, muted: false, unfocused: Unfocused::Mute, spots: SpotLevels::default() }
    }
}

impl AudioSettings {
    /// Sliders kept in `0..1` (a hand-edited file may say anything).
    pub fn clamped(mut self) -> Self {
        for v in [&mut self.master, &mut self.ambience, &mut self.effects, &mut self.ui, &mut self.music] {
            *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 };
        }
        for g in nd_audio::Group::ALL {
            let v = self.spots.get_mut(g);
            *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
        }
        self
    }

    /// The spot groups' sliders as gains (squared, like the buses).
    pub fn group_gains(&self) -> nd_audio::spots::GroupGains {
        nd_audio::Group::ALL.map(|g| nd_audio::mix::slider_gain(self.spots.get(g)))
    }

    /// The mixer's levels with the window focused or not.
    pub fn mix(&self, focused: bool) -> MixLevels {
        let duck = match (focused, self.unfocused) {
            (true, _) | (false, Unfocused::Keep) => 1.0,
            (false, Unfocused::Duck) => DUCK,
            (false, Unfocused::Mute) => 0.0,
        };
        MixLevels { master: self.master, ambience: self.ambience, effects: self.effects, ui: self.ui, music: self.music, muted: self.muted, duck }
    }
}

/// The settings now, shared by the Settings window, the remote API and the
/// systems here (a lock a frame; nothing heavy is done holding it).
static SETTINGS: Mutex<Option<(AudioSettings, u64)>> = Mutex::new(None);

/// The sound settings (read from `settings.ron` the first time).
pub fn settings() -> AudioSettings {
    settings_versioned().0
}

fn settings_versioned() -> (AudioSettings, u64) {
    let mut s = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    *s.get_or_insert_with(|| (crate::ui::theme::saved_audio().unwrap_or_default().clamped(), 0))
}

/// Change the sound settings; they are saved shortly after.
pub fn update_settings(f: impl FnOnce(&mut AudioSettings)) {
    let (mut now, v) = settings_versioned();
    f(&mut now);
    let now = now.clamped();
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, v + 1));
}

/// The sound card's state in words, for the Settings window.
static STATUS: Mutex<String> = Mutex::new(String::new());

fn note_status(status: &Status) {
    let text = match status {
        Status::Playing { device, sample_rate, .. } => format!("Playing on {device} at {sample_rate} Hz."),
        Status::Silent(why) => format!("Silent: {why}."),
    };
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = text;
}

/// Cues asked for from anywhere (the remote API, a window), played next frame.
static ASKED: Mutex<Vec<Cue>> = Mutex::new(Vec::new());

/// Ask for a UI cue (rate limited like the rest).
pub fn cue(c: Cue) {
    ASKED.lock().unwrap_or_else(|e| e.into_inner()).push(c);
}

/// The engine's handle and what drives it.
#[derive(Resource)]
pub struct Sound {
    pub handle: Handle,
    /// Off by `ND_AUDIO=0`: no device is opened, nothing restarts.
    pub disabled: bool,
    limiter: RateLimiter,
    /// Cues found this frame, played (rate limited) at the end of it.
    queue: Vec<Cue>,
    /// A click waits a frame or two: a cue that means more (the tool it
    /// picked, the window it opened) takes its place.
    pending_click: Option<u32>,
    frame: u32,
    /// What the camera hears, as last sampled, and the beds it called for.
    pub heard: nd_audio::Surroundings,
    pub beds: nd_audio::BedParams,
    /// [sound] The spot sounds picked last.
    pub spots: SpotReport,
    pub mix: MixLevels,
    pub focused: bool,
    /// The settings version last applied and when to save them.
    applied: u64,
    save_at: Option<f64>,
    restart_at: f64,
    /// Cues played (for the readout).
    pub played: u64,
}

impl Sound {
    fn new() -> Self {
        let disabled = std::env::var("ND_AUDIO").is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"));
        Self::with(if disabled { Handle::silent("off (ND_AUDIO=0)") } else { Handle::start() }, disabled)
    }

    /// No sound card (tests).
    #[cfg(test)]
    pub fn silent() -> Self {
        Self::with(Handle::silent("tests"), true)
    }

    fn with(handle: Handle, disabled: bool) -> Self {
        match &handle.status {
            Status::Playing { device, sample_rate, channels } => info!("sound: {device}, {sample_rate} Hz, {channels} channels"),
            Status::Silent(why) => info!("sound: silent ({why})"),
        }
        note_status(&handle.status);
        Self {
            handle,
            disabled,
            limiter: RateLimiter::default(),
            queue: Vec::new(),
            pending_click: None,
            frame: 0,
            heard: Default::default(),
            beds: Default::default(),
            spots: SpotReport::default(),
            mix: MixLevels::default(),
            focused: true,
            applied: u64::MAX,
            save_at: None,
            restart_at: 0.0,
            played: 0,
        }
    }

    /// Queue a cue found this frame.
    pub fn push(&mut self, c: Cue) {
        self.queue.push(c);
    }
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Sound::new())
            .init_resource::<watch::Seen>()
            .add_systems(Update, watch::actions.in_set(SimSet::Input).after(crate::tools::tool_input))
            .add_systems(Update, (watch::snapshot, listen::listen, spots::spots.after(listen::listen)).in_set(SimSet::Sync))
            .add_systems(PostUpdate, watch::ui_events.after(bevy_egui::EguiPostUpdateSet::EndPass).before(bevy_egui::EguiPostUpdateSet::ProcessOutput))
            .add_systems(Last, (mixer, play_cues).chain());
    }
}

/// The sliders, mute and focus into the mixer; save changed settings a
/// second after the last change; reopen a sound card that went away.
fn mixer(mut sound: ResMut<Sound>, time: Res<Time<Real>>, windows: Query<&Window, With<PrimaryWindow>>) {
    let now = time.elapsed_secs_f64();
    let (s, version) = settings_versioned();
    let focused = windows.single().map(|w| w.focused).unwrap_or(true);
    let mix = s.mix(focused);
    if version != sound.applied || mix != sound.mix {
        if version != sound.applied && sound.applied != u64::MAX {
            sound.save_at = Some(now + 1.0);
        }
        sound.applied = version;
        sound.mix = mix;
        sound.focused = focused;
        sound.handle.send(Command::Mix(mix));
    }
    if sound.save_at.is_some_and(|t| now >= t) {
        sound.save_at = None;
        crate::ui::theme::save_audio(&s);
    }
    // [sound] Unplugged headphones: try the (new) default device every few seconds.
    if !sound.disabled && sound.handle.telemetry.lost.load(std::sync::atomic::Ordering::Relaxed) && now >= sound.restart_at {
        sound.restart_at = now + 5.0;
        sound.handle = Handle::start();
        note_status(&sound.handle.status);
        let (mix, beds) = (sound.mix, sound.beds);
        sound.handle.send(Command::Mix(mix));
        sound.handle.send(Command::Beds(beds));
    }
}

/// This frame's cues through the rate limiter to the engine.
fn play_cues(mut sound: ResMut<Sound>, time: Res<Time<Real>>) {
    let now = time.elapsed_secs_f64();
    let asked: Vec<Cue> = std::mem::take(&mut *ASKED.lock().unwrap_or_else(|e| e.into_inner()));
    sound.frame = sound.frame.wrapping_add(1);
    let mut cues = std::mem::take(&mut sound.queue);
    cues.extend(asked);
    let meaningful = cues.iter().any(|c| *c != Cue::Click);
    if meaningful {
        sound.pending_click = None;
    } else if cues.contains(&Cue::Click) && sound.pending_click.is_none() {
        sound.pending_click = Some(sound.frame);
    }
    cues.retain(|c| *c != Cue::Click);
    // The click plays two frames on, if nothing took its place.
    if sound.pending_click.is_some_and(|f| sound.frame.wrapping_sub(f) >= 2) {
        sound.pending_click = None;
        cues.push(Cue::Click);
    }
    // One of each a frame.
    let mut seen = Vec::new();
    for c in cues {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        if let Some(gain) = sound.limiter.allow(c, now) {
            sound.handle.send(Command::Cue(c, gain));
            sound.played += 1;
        }
    }
}

/// [sound] The Settings window's SOUND section.
pub fn settings_ui(ui: &mut bevy_egui::egui::Ui) {
    use bevy_egui::egui;
    crate::ui::section(ui, "SOUND");
    let mut s = settings();
    let before = s;
    ui.checkbox(&mut s.muted, "Mute all sound");
    ui.add_enabled_ui(!s.muted, |ui| {
        for (label, v) in [("Master", &mut s.master), ("Ambience", &mut s.ambience), ("Effects", &mut s.effects), ("Interface", &mut s.ui), ("Music", &mut s.music)] {
            ui.add(egui::Slider::new(v, 0.0..=1.0).text(label).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
        }
    });
    ui.add_enabled_ui(!s.muted, |ui| {
        egui::CollapsingHeader::new(egui::RichText::new("Effects by kind").small()).id_salt("sound_spot_groups").show(ui, |ui| {
            for g in nd_audio::Group::ALL {
                ui.add(egui::Slider::new(s.spots.get_mut(g), 0.0..=1.0).text(g.label()).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Out of focus").small());
        for u in Unfocused::ALL {
            if ui.selectable_label(s.unfocused == u, u.label()).clicked() {
                s.unfocused = u;
            }
        }
    });
    if s != before {
        update_settings(|now| *now = s);
    }
    let status = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    crate::ui::hint(ui, &format!("{status} All synthesised: the city's hum changes with height, traffic, crowds, the district, the hour and the weather; near the camera, trains, the metro, sirens, building works, clubs, bars, karaoke, music halls, horns in jams and thunder. Music comes later."));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_mutes_ducks_or_keeps_as_set() {
        let s = AudioSettings::default();
        assert_eq!(s.mix(true).duck, 1.0);
        assert_eq!(s.mix(false).duck, 0.0);
        assert_eq!(AudioSettings { unfocused: Unfocused::Duck, ..s }.mix(false).duck, DUCK);
        assert_eq!(AudioSettings { unfocused: Unfocused::Keep, ..s }.mix(false).duck, 1.0);
        assert!(s.mix(false).silent());
        assert!(AudioSettings { muted: true, ..s }.mix(true).silent());
        for u in Unfocused::ALL {
            assert_eq!(Unfocused::parse(u.key()), Some(u));
        }
    }

    #[test]
    fn wild_sliders_are_clamped() {
        let s = AudioSettings { master: 7.0, ambience: -1.0, effects: f32::NAN, spots: SpotLevels { horns: 3.0, sirens: f32::NAN, ..Default::default() }, ..Default::default() }.clamped();
        assert_eq!((s.master, s.ambience, s.effects), (1.0, 0.0, 0.5));
        assert_eq!((s.spots.horns, s.spots.sirens), (1.0, 1.0));
    }

    #[test]
    fn old_settings_without_spot_groups_load_with_them_full() {
        let s: AudioSettings = ron::from_str("(master: 0.5, effects: 0.3)").unwrap();
        assert_eq!(s.spots, SpotLevels::default());
        assert_eq!(s.group_gains(), [1.0; 6]);
        let half = AudioSettings { spots: SpotLevels { nightlife: 0.5, ..Default::default() }, ..s };
        assert_eq!(half.group_gains()[nd_audio::Group::Nightlife.index()], 0.25);
        let back: AudioSettings = ron::from_str(&ron::to_string(&half).unwrap()).unwrap();
        assert_eq!(back, half);
    }
}
