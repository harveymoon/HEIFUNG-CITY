//! [laptop-nav] Navigation input that works without a mouse and over a
//! remote desktop: what the camera (`camera::control_camera`) reads instead
//! of the raw input resources.
//!
//! Why: Bevy's `AccumulatedMouseMotion` is the raw device motion (Windows
//! raw input). Over Remote Desktop, Chrome Remote Desktop, VNC, a VM or with
//! a pen the pointer is an *absolute* device, and winit 0.30 hands its
//! absolute coordinates (0..65535) on as if they were deltas (its
//! `has_flag(flags, MOUSE_MOVE_RELATIVE)` is always true: that flag is 0).
//! So a right-drag over RDP spun the camera to its limits or not at all.
//! Here drags follow the window's cursor position (`CursorMoved`), which
//! every remote tool moves correctly; raw motion is only a fallback (the
//! cursor stopped at a screen edge) and only while it looks sane.
//!
//! Touchpads on Windows send two-finger scrolls as many small, fractional
//! wheel steps and a pinch as Ctrl + those steps; macOS sends pixel deltas
//! and real pinch and rotate gestures. Each wheel event is sorted
//! ([`classify_scroll`]) into a mouse wheel notch (zooms), a touchpad
//! scroll (pans) or a Ctrl/pinch zoom ([`scroll_effect`]); the Settings
//! window can make every scroll zoom or every scroll pan instead.
//!
//! Also here: the left-button drag/tap tracking (a left-drag on the map pans
//! in the Inspect tool, a tap without a drag still picks), the keyboard
//! mappings as pure functions, the on-screen navigation pad's requests
//! ([`NavPad`]) and the remote API's synthetic input ([`NavRemote`]).

use std::collections::VecDeque;

use bevy::input::gestures::{PinchGesture, RotationGesture};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::{CursorLeft, CursorMoved, PrimaryWindow};
use serde::{Deserialize, Serialize};

use crate::ui::UiFlags;

/// Raw motion bigger than this in one frame is not a mouse moving: it is an
/// absolute pointer's coordinates passed on as a delta.
pub const RAW_SANE: f32 = 1500.0;
/// A left press that moves further than this (logical px) is a drag, not a click.
pub const DRAG_PX: f32 = 6.0;
/// A touchpad's wheel "line" as a drag of this many pixels.
pub const LINE_PX: f32 = 60.0;
/// Pixel scroll as wheel lines when zooming (as before).
pub const PIXEL_PER_LINE: f32 = 60.0;
/// A pinch's magnification step as wheel lines of zoom.
pub const PINCH_LINES: f32 = 8.0;
/// Wheel events closer together than this belong to one gesture.
const STREAK_GAP: f64 = 0.3;
/// How long one tap of a pad button (or a nav command step) moves, seconds.
pub const TAP_SECS: f32 = 0.25;

/// What a two-finger scroll does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScrollMode {
    /// A mouse wheel zooms; a touchpad's two-finger scroll pans (pinch or
    /// Ctrl+scroll zooms).
    #[default]
    Auto,
    /// Every scroll zooms (the old behaviour).
    Zoom,
    /// Every scroll pans; pinch or Ctrl+scroll zooms.
    Pan,
}

impl ScrollMode {
    pub const ALL: [ScrollMode; 3] = [ScrollMode::Auto, ScrollMode::Zoom, ScrollMode::Pan];
    pub fn label(self) -> &'static str {
        match self {
            ScrollMode::Auto => "Auto",
            ScrollMode::Zoom => "Zoom",
            ScrollMode::Pan => "Pan",
        }
    }
}

/// When the on-screen navigation pad shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PadMode {
    /// Until a mouse wheel is used this run (a laptop or a remote session
    /// keeps it).
    #[default]
    Auto,
    On,
    Off,
}

impl PadMode {
    pub const ALL: [PadMode; 3] = [PadMode::Auto, PadMode::On, PadMode::Off];
    pub fn label(self) -> &'static str {
        match self {
            PadMode::Auto => "Auto",
            PadMode::On => "Always",
            PadMode::Off => "Off",
        }
    }
}

/// The player's navigation settings (Settings window, kept in `settings.ron`).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavSettings {
    pub scroll: ScrollMode,
    /// Two-finger scroll pan speed.
    pub pan_speed: f32,
    /// Wheel, pinch and Ctrl+scroll zoom speed.
    pub zoom_speed: f32,
    /// Drag rotate and tilt speed (right-drag, Alt+drag, Walk look).
    pub turn_speed: f32,
    pub invert_zoom: bool,
    pub invert_scroll_pan: bool,
    pub invert_tilt: bool,
    /// A left-drag on the map pans in the Inspect tool.
    pub left_drag_pan: bool,
    pub pad: PadMode,
    /// Walk mode: the pointer stays free and a drag turns the head (no
    /// mouselook). On by itself when the raw mouse motion is unusable.
    pub drag_look: bool,
}

impl Default for NavSettings {
    fn default() -> Self {
        Self {
            scroll: ScrollMode::Auto,
            pan_speed: 1.0,
            zoom_speed: 1.0,
            turn_speed: 1.0,
            invert_zoom: false,
            invert_scroll_pan: false,
            invert_tilt: false,
            left_drag_pan: true,
            pad: PadMode::Auto,
            drag_look: false,
        }
    }
}

/// What one wheel event came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollKind {
    /// A mouse wheel's whole notches.
    Wheel,
    /// A touchpad's two-finger scroll: fractional steps, sideways steps or pixels.
    Touchpad,
    /// Ctrl held: a Windows touchpad pinch, or Ctrl+wheel.
    CtrlZoom,
}

impl ScrollKind {
    pub fn label(self) -> &'static str {
        match self {
            ScrollKind::Wheel => "mouse wheel",
            ScrollKind::Touchpad => "touchpad scroll",
            ScrollKind::CtrlZoom => "pinch / Ctrl+scroll",
        }
    }
}

/// Sort one wheel event. `streak` is the kind of the gesture it continues
/// (events less than `STREAK_GAP` apart): a touchpad scroll stays one even
/// when a step happens to be whole.
pub fn classify_scroll(unit: MouseScrollUnit, d: Vec2, ctrl: bool, streak: Option<ScrollKind>) -> ScrollKind {
    if ctrl {
        return ScrollKind::CtrlZoom;
    }
    if unit == MouseScrollUnit::Pixel || streak == Some(ScrollKind::Touchpad) {
        return ScrollKind::Touchpad;
    }
    let fractional = |v: f32| (v - v.round()).abs() > 0.01;
    if fractional(d.x) || fractional(d.y) {
        ScrollKind::Touchpad
    } else {
        ScrollKind::Wheel
    }
}

/// What one wheel event does: (zoom in wheel lines, + is in; pan as a drag
/// of that many pixels, the map moving with it).
pub fn scroll_effect(kind: ScrollKind, unit: MouseScrollUnit, d: Vec2, s: &NavSettings) -> (f32, Vec2) {
    let (lines, px) = match unit {
        MouseScrollUnit::Line => (d, d * LINE_PX),
        MouseScrollUnit::Pixel => (d / PIXEL_PER_LINE, d),
    };
    let z = if s.invert_zoom { -s.zoom_speed } else { s.zoom_speed };
    let p = if s.invert_scroll_pan { -s.pan_speed } else { s.pan_speed };
    let zoom_mode = match kind {
        ScrollKind::CtrlZoom => return (lines.y * z, Vec2::ZERO),
        ScrollKind::Wheel => s.scroll != ScrollMode::Pan,
        ScrollKind::Touchpad => s.scroll == ScrollMode::Zoom,
    };
    if zoom_mode {
        // Sideways steps (a tilting wheel, a touchpad) still pan sideways.
        (lines.y * z, Vec2::new(px.x, 0.0) * p)
    } else {
        (0.0, px * p)
    }
}

/// A frame's raw motion is believable (not an absolute pointer's coordinates).
pub fn plausible_raw(d: Vec2) -> bool {
    d.x.abs() < RAW_SANE && d.y.abs() < RAW_SANE
}

/// The pointer's movement for a drag this frame: the cursor's when it moved
/// (works everywhere, remote sessions too), else the raw motion when it is
/// trusted (the cursor stuck at a screen edge while the mouse goes on).
pub fn pick_drag_delta(cursor: Option<Vec2>, raw: Vec2, raw_reliable: bool) -> Vec2 {
    match cursor {
        Some(d) => d,
        None if raw_reliable && plausible_raw(raw) => raw,
        None => Vec2::ZERO,
    }
}

/// What a held left button does to the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftDrag {
    /// Nothing: the tool has it.
    None,
    Pan,
    /// Turn and tilt, as a right-drag.
    Orbit,
}

/// The left button's camera role: Alt+drag turns and tilts and Shift+drag
/// pans in every tool; in the Inspect tool a plain drag (past `DRAG_PX`)
/// pans. Only for a press that began on the map.
pub fn left_drag_role(on_map: bool, alt: bool, shift: bool, inspect: bool, left_pan: bool, dragging: bool) -> LeftDrag {
    if !on_map {
        LeftDrag::None
    } else if alt {
        LeftDrag::Orbit
    } else if shift || (inspect && left_pan && dragging) {
        LeftDrag::Pan
    } else {
        LeftDrag::None
    }
}

/// A camera move asked for by keys or the pad, as rates (per second):
/// `pan` x right, y up the screen; `zoom` + in; `yaw` + turns as Q does;
/// `tilt` + looks more steeply down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavMove {
    pub pan: Vec2,
    pub zoom: f32,
    pub yaw: f32,
    pub tilt: f32,
}

impl NavMove {
    pub fn is_zero(&self) -> bool {
        *self == NavMove::default()
    }

    pub fn add(&mut self, o: NavMove) {
        self.pan += o.pan;
        self.zoom += o.zoom;
        self.yaw += o.yaw;
        self.tilt += o.tilt;
    }

    pub fn scaled(self, k: f32) -> NavMove {
        NavMove { pan: self.pan * k, zoom: self.zoom * k, yaw: self.yaw * k, tilt: self.tilt * k }
    }
}

/// Which keys the active tool keeps for itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyCaptures {
    /// [ and ] (the Road tool's levels).
    pub brackets: bool,
    /// Page Up / Page Down (the Road tool's levels).
    pub pages: bool,
}

/// The orbit camera's keys: WASD / arrows pan (with Shift, W/S and up/down
/// tilt), Q/E turn, [ ] or Page Up/Down or . , zoom, Home/End tilt.
pub fn orbit_keys(held: impl Fn(KeyCode) -> bool, shift: bool, cap: KeyCaptures) -> NavMove {
    let mut m = NavMove::default();
    let up = held(KeyCode::KeyW) || held(KeyCode::ArrowUp);
    let down = held(KeyCode::KeyS) || held(KeyCode::ArrowDown);
    if shift {
        m.tilt += up as i32 as f32 - down as i32 as f32;
    } else {
        m.pan.y += up as i32 as f32 - down as i32 as f32;
    }
    if held(KeyCode::KeyA) || held(KeyCode::ArrowLeft) {
        m.pan.x -= 1.0;
    }
    if held(KeyCode::KeyD) || held(KeyCode::ArrowRight) {
        m.pan.x += 1.0;
    }
    if held(KeyCode::KeyQ) {
        m.yaw += 1.0;
    }
    if held(KeyCode::KeyE) {
        m.yaw -= 1.0;
    }
    let zin = (held(KeyCode::BracketLeft) && !cap.brackets) || (held(KeyCode::PageUp) && !cap.pages) || held(KeyCode::Period);
    let zout = (held(KeyCode::BracketRight) && !cap.brackets) || (held(KeyCode::PageDown) && !cap.pages) || held(KeyCode::Comma);
    m.zoom += zin as i32 as f32 - zout as i32 as f32;
    if held(KeyCode::Home) {
        m.tilt += 1.0;
    }
    if held(KeyCode::End) {
        m.tilt -= 1.0;
    }
    m
}

/// Walk mode's keys: (move: x right, y forward; turn + left; look + up).
/// WASD / arrows walk, Q/E turn, R/F or Home/End look up and down.
pub fn walk_keys(held: impl Fn(KeyCode) -> bool) -> (Vec2, f32, f32) {
    let k = |a: KeyCode, b: KeyCode| (held(a) || held(b)) as i32 as f32;
    let mv = Vec2::new(k(KeyCode::KeyD, KeyCode::ArrowRight) - k(KeyCode::KeyA, KeyCode::ArrowLeft), k(KeyCode::KeyW, KeyCode::ArrowUp) - k(KeyCode::KeyS, KeyCode::ArrowDown));
    let turn = held(KeyCode::KeyQ) as i32 as f32 - held(KeyCode::KeyE) as i32 as f32;
    let look = k(KeyCode::KeyR, KeyCode::Home) - k(KeyCode::KeyF, KeyCode::End);
    (mv, turn, look)
}

/// The angle nearest `yaw` that faces north (a whole number of turns).
pub fn north_of(yaw: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (yaw / tau).round() * tau
}

/// This frame's navigation input, gathered once (`gather`) for the camera
/// and the tools.
#[derive(Resource, Debug)]
pub struct NavInput {
    /// The cursor's latest position in the window (logical px), from its
    /// events; kept while a drag carries it outside the window.
    pub cursor: Option<Vec2>,
    /// How far the cursor moved this frame, when it did.
    pub cursor_delta: Option<Vec2>,
    /// The raw motion, zero when not believable.
    pub raw: Vec2,
    /// Raw motion has looked like a mouse's all run (false after an
    /// absolute pointer was seen, or a Walk mouselook that got none).
    pub raw_reliable: bool,
    raw_bad_frames: u32,
    /// Walk frames where the cursor moved and no raw motion came.
    no_raw_frames: u32,
    /// Zoom from the wheel, pinch and Ctrl+scroll, in wheel lines (+ in).
    pub zoom: f32,
    /// Pan from touchpad scrolls, as a drag of so many pixels.
    pub pan: Vec2,
    /// Turn from a rotate gesture, radians (+ counterclockwise).
    pub rotate: f32,
    pub last_scroll: Option<ScrollKind>,
    streak: Option<(ScrollKind, f64)>,
    /// A mouse wheel notch was seen this run (the pad's Auto hides).
    pub wheel_seen: bool,
    pub touchpad_seen: bool,
    left_down_at: Option<Vec2>,
    /// The left button went down on the map (not on a panel).
    pub left_on_map: bool,
    /// The held left button has moved past `DRAG_PX`.
    pub left_dragging: bool,
    /// The left button came up this frame without having dragged, after a
    /// press on the map: a click for the Inspect tool.
    pub left_tap: bool,
}

impl Default for NavInput {
    fn default() -> Self {
        Self {
            cursor: None,
            cursor_delta: None,
            raw: Vec2::ZERO,
            raw_reliable: true,
            raw_bad_frames: 0,
            no_raw_frames: 0,
            zoom: 0.0,
            pan: Vec2::ZERO,
            rotate: 0.0,
            last_scroll: None,
            streak: None,
            wheel_seen: false,
            touchpad_seen: false,
            left_down_at: None,
            left_on_map: false,
            left_dragging: false,
            left_tap: false,
        }
    }
}

impl NavInput {
    /// The pointer's movement for a drag this frame.
    pub fn drag_delta(&self) -> Vec2 {
        pick_drag_delta(self.cursor_delta, self.raw, self.raw_reliable)
    }

    /// Walk mode's mouselook got no raw motion while the cursor moved
    /// (frames in a row): raw input is not arriving, look by dragging.
    pub fn note_walk_look(&mut self) {
        let moved = self.cursor_delta.is_some_and(|d| d.length() > 0.5);
        if self.raw != Vec2::ZERO {
            self.no_raw_frames = 0;
        } else if moved {
            self.no_raw_frames += 1;
            if self.no_raw_frames >= 30 {
                self.raw_reliable = false;
            }
        }
    }

    /// Fold one frame's raw motion in.
    fn take_raw(&mut self, raw: Vec2) {
        if plausible_raw(raw) {
            self.raw = if self.raw_reliable { raw } else { Vec2::ZERO };
        } else {
            self.raw = Vec2::ZERO;
            self.raw_bad_frames += 1;
            if self.raw_bad_frames >= 2 {
                self.raw_reliable = false;
            }
        }
    }

    /// Fold one wheel event in.
    fn take_wheel(&mut self, unit: MouseScrollUnit, d: Vec2, ctrl: bool, now: f64, s: &NavSettings) {
        let streak = self.streak.filter(|(_, t)| now - t < STREAK_GAP).map(|(k, _)| k);
        let kind = classify_scroll(unit, d, ctrl, streak);
        self.streak = Some((kind, now));
        self.last_scroll = Some(kind);
        match kind {
            ScrollKind::Wheel => self.wheel_seen = true,
            ScrollKind::Touchpad => self.touchpad_seen = true,
            ScrollKind::CtrlZoom => {}
        }
        let (z, p) = scroll_effect(kind, unit, d, s);
        self.zoom += z;
        self.pan += p;
    }
}

/// [laptop-nav] What the on-screen pad (and the remote `nav` command) asks
/// of the camera; read and cleared by `control_camera`.
#[derive(Resource, Default, Debug)]
pub struct NavPad {
    /// Buttons held this frame (rates, as keys).
    pub hold: NavMove,
    /// Buttons just pressed: a step of `TAP_SECS` at once.
    pub tap: NavMove,
    /// Turn to face north (eases there).
    pub north: bool,
    /// Back to the default tilt as well.
    pub level: bool,
}

/// One synthetic input event for the remote `input` command.
#[derive(Clone, Debug, PartialEq)]
pub enum Synth {
    Cursor(Vec2),
    CursorLeft,
    Button(MouseButton, bool),
    Wheel(MouseScrollUnit, Vec2),
    Pinch(f32),
    /// Degrees, + counterclockwise (as winit sends).
    Rotate(f32),
    Raw(Vec2),
    Key(KeyCode, bool),
}

/// [laptop-nav] The remote API's levers: synthetic input, one list of
/// events per frame, played into Bevy's input messages as a window would
/// send them (nothing reaches the operating system: the real cursor is
/// never moved); and timed pad holds.
#[derive(Resource, Default, Debug)]
pub struct NavRemote {
    pub frames: VecDeque<Vec<Synth>>,
    /// (move as rates, seconds left).
    pub holds: Vec<(NavMove, f32)>,
}

pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        let settings = crate::ui::theme::saved_nav().unwrap_or_default();
        app.insert_resource(settings)
            .init_resource::<NavInput>()
            .init_resource::<NavPad>()
            .init_resource::<NavRemote>()
            .add_message::<CursorMoved>()
            .add_message::<CursorLeft>()
            .add_systems(PreUpdate, play_synthetic.before(InputSystems))
            .add_systems(Update, gather.before(crate::camera::control_camera));
    }
}

/// Play the next frame of the remote API's synthetic input.
#[allow(clippy::too_many_arguments)]
fn play_synthetic(
    mut remote: ResMut<NavRemote>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut moved: MessageWriter<CursorMoved>,
    mut left: MessageWriter<CursorLeft>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut wheel: MessageWriter<MouseWheel>,
    mut pinch: MessageWriter<PinchGesture>,
    mut rotate: MessageWriter<RotationGesture>,
    mut raw: MessageWriter<MouseMotion>,
    mut keys: MessageWriter<KeyboardInput>,
    mut last: Local<Option<Vec2>>,
) {
    let Some(frame) = remote.frames.pop_front() else { return };
    let window = windows.iter().next().unwrap_or(Entity::PLACEHOLDER);
    let state = |down: bool| if down { ButtonState::Pressed } else { ButtonState::Released };
    for e in frame {
        match e {
            Synth::Cursor(p) => {
                moved.write(CursorMoved { window, position: p, delta: last.map(|l| p - l) });
                *last = Some(p);
            }
            Synth::CursorLeft => {
                left.write(CursorLeft { window });
                *last = None;
            }
            Synth::Button(button, down) => {
                buttons.write(MouseButtonInput { button, state: state(down), window });
            }
            Synth::Wheel(unit, d) => {
                wheel.write(MouseWheel { unit, x: d.x, y: d.y, window, phase: TouchPhase::Moved });
            }
            Synth::Pinch(d) => {
                pinch.write(PinchGesture(d));
            }
            Synth::Rotate(d) => {
                rotate.write(RotationGesture(d));
            }
            Synth::Raw(d) => {
                raw.write(MouseMotion { delta: d });
            }
            Synth::Key(key_code, down) => {
                keys.write(KeyboardInput { key_code, logical_key: Key::Unidentified(NativeKey::Unidentified), state: state(down), text: None, repeat: false, window });
            }
        }
    }
}

/// Gather this frame's navigation input.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gather(
    time: Res<Time<Real>>,
    settings: Res<NavSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    flags: Option<Res<UiFlags>>,
    (mut moved, mut left): (MessageReader<CursorMoved>, MessageReader<CursorLeft>),
    (mut wheel, mut pinch, mut rotate): (MessageReader<MouseWheel>, MessageReader<PinchGesture>, MessageReader<RotationGesture>),
    mut nav: ResMut<NavInput>,
) {
    let over_ui = flags.is_some_and(|f| f.pointer_over_ui);
    // The cursor: its own events, so a drag that leaves the window (the
    // window keeps the mouse while a button is held) goes on counting.
    let mut delta: Option<Vec2> = None;
    for e in moved.read() {
        if let Some(last) = nav.cursor {
            *delta.get_or_insert(Vec2::ZERO) += e.position - last;
        }
        nav.cursor = Some(e.position);
    }
    if left.read().count() > 0 && !buttons.get_pressed().any(|_| true) {
        nav.cursor = None;
    }
    nav.cursor_delta = delta;
    nav.take_raw(motion.delta);

    nav.zoom = 0.0;
    nav.pan = Vec2::ZERO;
    nav.rotate = 0.0;
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let now = time.elapsed_secs_f64();
    for e in wheel.read() {
        nav.take_wheel(e.unit, Vec2::new(e.x, e.y), ctrl, now, &settings);
    }
    let z = if settings.invert_zoom { -settings.zoom_speed } else { settings.zoom_speed };
    for p in pinch.read() {
        nav.zoom += p.0 * PINCH_LINES * z;
    }
    for r in rotate.read() {
        nav.rotate += r.0.to_radians();
    }

    // The left button: where it went down, whether it has dragged, a tap.
    nav.left_tap = false;
    if buttons.just_pressed(MouseButton::Left) {
        nav.left_down_at = nav.cursor;
        nav.left_on_map = !over_ui;
        nav.left_dragging = false;
    }
    if buttons.pressed(MouseButton::Left) {
        if let (Some(a), Some(c)) = (nav.left_down_at, nav.cursor) {
            if a.distance(c) > DRAG_PX {
                nav.left_dragging = true;
            }
        }
    }
    if buttons.just_released(MouseButton::Left) {
        nav.left_tap = nav.left_on_map && !nav.left_dragging;
        nav.left_down_at = None;
    }
    if !buttons.pressed(MouseButton::Left) && !buttons.just_released(MouseButton::Left) {
        nav.left_on_map = false;
        nav.left_dragging = false;
    }
}

/// Parse a key name for the remote `input` command ("KeyW", "PageUp", "w", "[").
pub fn key_from_name(name: &str) -> Option<KeyCode> {
    let n = name.trim();
    if n.chars().count() == 1 {
        let c = n.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphabetic() {
            return key_from_name(&format!("Key{c}"));
        }
        if c.is_ascii_digit() {
            return key_from_name(&format!("Digit{c}"));
        }
    }
    Some(match n {
        "[" => KeyCode::BracketLeft,
        "]" => KeyCode::BracketRight,
        "," => KeyCode::Comma,
        "." => KeyCode::Period,
        "ShiftLeft" | "Shift" => KeyCode::ShiftLeft,
        "ShiftRight" => KeyCode::ShiftRight,
        "ControlLeft" | "Ctrl" | "Control" => KeyCode::ControlLeft,
        "ControlRight" => KeyCode::ControlRight,
        "AltLeft" | "Alt" => KeyCode::AltLeft,
        "AltRight" => KeyCode::AltRight,
        "ArrowUp" | "Up" => KeyCode::ArrowUp,
        "ArrowDown" | "Down" => KeyCode::ArrowDown,
        "ArrowLeft" | "Left" => KeyCode::ArrowLeft,
        "ArrowRight" | "Right" => KeyCode::ArrowRight,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "Comma" => KeyCode::Comma,
        "Period" => KeyCode::Period,
        "BracketLeft" => KeyCode::BracketLeft,
        "BracketRight" => KeyCode::BracketRight,
        "Tab" => KeyCode::Tab,
        "Escape" | "Esc" => KeyCode::Escape,
        "Space" => KeyCode::Space,
        "Enter" => KeyCode::Enter,
        "KeyA" => KeyCode::KeyA,
        "KeyB" => KeyCode::KeyB,
        "KeyC" => KeyCode::KeyC,
        "KeyD" => KeyCode::KeyD,
        "KeyE" => KeyCode::KeyE,
        "KeyF" => KeyCode::KeyF,
        "KeyG" => KeyCode::KeyG,
        "KeyH" => KeyCode::KeyH,
        "KeyI" => KeyCode::KeyI,
        "KeyJ" => KeyCode::KeyJ,
        "KeyK" => KeyCode::KeyK,
        "KeyL" => KeyCode::KeyL,
        "KeyM" => KeyCode::KeyM,
        "KeyN" => KeyCode::KeyN,
        "KeyO" => KeyCode::KeyO,
        "KeyP" => KeyCode::KeyP,
        "KeyQ" => KeyCode::KeyQ,
        "KeyR" => KeyCode::KeyR,
        "KeyS" => KeyCode::KeyS,
        "KeyT" => KeyCode::KeyT,
        "KeyU" => KeyCode::KeyU,
        "KeyV" => KeyCode::KeyV,
        "KeyW" => KeyCode::KeyW,
        "KeyX" => KeyCode::KeyX,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        "Digit1" => KeyCode::Digit1,
        "Digit2" => KeyCode::Digit2,
        "Digit3" => KeyCode::Digit3,
        "Digit4" => KeyCode::Digit4,
        "Digit5" => KeyCode::Digit5,
        "Digit6" => KeyCode::Digit6,
        "Digit7" => KeyCode::Digit7,
        "Digit8" => KeyCode::Digit8,
        "Digit9" => KeyCode::Digit9,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "nav_tests.rs"]
mod tests;
