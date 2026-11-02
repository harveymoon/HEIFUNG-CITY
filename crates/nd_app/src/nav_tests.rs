//! [laptop-nav] The input mapping as pure functions, then the whole path
//! from synthetic window events (as a touchpad, a remote desktop or a
//! normal mouse sends them) through `gather` and `control_camera` to the
//! camera, in a headless app.

use std::time::Duration;

use bevy::input::mouse::MouseScrollUnit;
use bevy::input::InputPlugin;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use super::*;
use crate::camera::{control_camera, CameraFly, CameraFollow, CameraMode, OrbitCamera, WalkState};
use crate::sim::Sim;
use crate::tools::{Tool, ToolState};
use crate::ui::UiFlags;

const LINE: MouseScrollUnit = MouseScrollUnit::Line;
const PIXEL: MouseScrollUnit = MouseScrollUnit::Pixel;

#[test]
fn a_wheel_notch_is_a_wheel_and_fractions_are_a_touchpad() {
    assert_eq!(classify_scroll(LINE, Vec2::new(0.0, 1.0), false, None), ScrollKind::Wheel);
    assert_eq!(classify_scroll(LINE, Vec2::new(0.0, -2.0), false, None), ScrollKind::Wheel);
    assert_eq!(classify_scroll(LINE, Vec2::new(0.0, 0.0667), false, None), ScrollKind::Touchpad);
    assert_eq!(classify_scroll(LINE, Vec2::new(-0.3, 0.0), false, None), ScrollKind::Touchpad);
    assert_eq!(classify_scroll(PIXEL, Vec2::new(0.0, 12.0), false, None), ScrollKind::Touchpad);
    // A whole step inside a touchpad gesture stays a touchpad step.
    assert_eq!(classify_scroll(LINE, Vec2::new(0.0, 1.0), false, Some(ScrollKind::Touchpad)), ScrollKind::Touchpad);
    // Ctrl (a Windows touchpad's pinch) zooms whatever it is.
    assert_eq!(classify_scroll(LINE, Vec2::new(0.0, 0.1), true, Some(ScrollKind::Touchpad)), ScrollKind::CtrlZoom);
    assert_eq!(classify_scroll(PIXEL, Vec2::new(0.0, 5.0), true, None), ScrollKind::CtrlZoom);
}

#[test]
fn scroll_zooms_or_pans_as_the_setting_says() {
    let auto = NavSettings::default();
    // Auto: the wheel zooms, a touchpad pans (both ways), Ctrl zooms.
    assert_eq!(scroll_effect(ScrollKind::Wheel, LINE, Vec2::new(0.0, 1.0), &auto), (1.0, Vec2::ZERO));
    let (z, p) = scroll_effect(ScrollKind::Touchpad, LINE, Vec2::new(0.1, -0.2), &auto);
    assert_eq!(z, 0.0);
    assert!((p - Vec2::new(0.1, -0.2) * LINE_PX).length() < 1e-4);
    assert_eq!(scroll_effect(ScrollKind::Touchpad, PIXEL, Vec2::new(3.0, 4.0), &auto), (0.0, Vec2::new(3.0, 4.0)));
    assert_eq!(scroll_effect(ScrollKind::CtrlZoom, PIXEL, Vec2::new(0.0, 30.0), &auto), (0.5, Vec2::ZERO));
    // Zoom: a touchpad zooms with its vertical, pans with its sideways steps.
    let zoom = NavSettings { scroll: ScrollMode::Zoom, ..auto };
    let (z, p) = scroll_effect(ScrollKind::Touchpad, LINE, Vec2::new(0.5, 0.25), &zoom);
    assert_eq!(z, 0.25);
    assert_eq!(p, Vec2::new(0.5 * LINE_PX, 0.0));
    // Pan: even the wheel pans; Ctrl still zooms.
    let pan = NavSettings { scroll: ScrollMode::Pan, ..auto };
    assert_eq!(scroll_effect(ScrollKind::Wheel, LINE, Vec2::new(0.0, 1.0), &pan), (0.0, Vec2::new(0.0, LINE_PX)));
    assert_eq!(scroll_effect(ScrollKind::CtrlZoom, LINE, Vec2::new(0.0, 1.0), &pan).0, 1.0);
    // Speeds and inverts.
    let fast = NavSettings { zoom_speed: 2.0, invert_zoom: true, pan_speed: 0.5, invert_scroll_pan: true, ..auto };
    assert_eq!(scroll_effect(ScrollKind::Wheel, LINE, Vec2::new(0.0, 1.0), &fast).0, -2.0);
    assert_eq!(scroll_effect(ScrollKind::Touchpad, PIXEL, Vec2::new(10.0, 0.0), &fast).1, Vec2::new(-5.0, 0.0));
}

#[test]
fn drags_follow_the_cursor_and_ignore_absolute_raw_motion() {
    // The cursor moved: its delta, whatever the raw motion says.
    assert_eq!(pick_drag_delta(Some(Vec2::new(4.0, -2.0)), Vec2::new(9.0, 9.0), true), Vec2::new(4.0, -2.0));
    // It did not (stuck at a screen edge): the raw motion, when trusted.
    assert_eq!(pick_drag_delta(None, Vec2::new(9.0, 1.0), true), Vec2::new(9.0, 1.0));
    assert_eq!(pick_drag_delta(None, Vec2::new(9.0, 1.0), false), Vec2::ZERO);
    // An absolute pointer's coordinates passed as a delta (RDP): never.
    assert!(!plausible_raw(Vec2::new(32768.0, 20000.0)));
    assert_eq!(pick_drag_delta(None, Vec2::new(32768.0, 20000.0), true), Vec2::ZERO);
    assert!(plausible_raw(Vec2::new(-300.0, 120.0)));
}

#[test]
fn the_left_button_pans_only_where_it_should() {
    use LeftDrag::*;
    // Inspect: a drag past the threshold pans; a press that has not moved does nothing yet.
    assert_eq!(left_drag_role(true, false, false, true, true, true), Pan);
    assert_eq!(left_drag_role(true, false, false, true, true, false), None);
    // Turned off in Settings, or another tool: the tool keeps the left button.
    assert_eq!(left_drag_role(true, false, false, true, false, true), None);
    assert_eq!(left_drag_role(true, false, false, false, true, true), None);
    // Alt turns, Shift pans, in every tool; never for a press on a panel.
    assert_eq!(left_drag_role(true, true, false, false, true, false), Orbit);
    assert_eq!(left_drag_role(true, false, true, false, true, false), Pan);
    assert_eq!(left_drag_role(false, true, true, true, true, true), None);
}

#[test]
fn keys_cover_every_move() {
    let held = |keys: &'static [KeyCode]| move |k: KeyCode| keys.contains(&k);
    let none = KeyCaptures::default();
    assert_eq!(orbit_keys(held(&[KeyCode::KeyW]), false, none).pan, Vec2::Y);
    assert_eq!(orbit_keys(held(&[KeyCode::ArrowLeft]), false, none).pan, -Vec2::X);
    // Shift turns W/S into tilt.
    let m = orbit_keys(held(&[KeyCode::KeyW]), true, none);
    assert_eq!((m.pan, m.tilt), (Vec2::ZERO, 1.0));
    assert_eq!(orbit_keys(held(&[KeyCode::End]), false, none).tilt, -1.0);
    assert_eq!(orbit_keys(held(&[KeyCode::KeyQ]), false, none).yaw, 1.0);
    for k in [KeyCode::BracketLeft, KeyCode::PageUp, KeyCode::Period] {
        assert_eq!(orbit_keys(move |x| x == k, false, none).zoom, 1.0, "{k:?} zooms in");
    }
    for k in [KeyCode::BracketRight, KeyCode::PageDown, KeyCode::Comma] {
        assert_eq!(orbit_keys(move |x| x == k, false, none).zoom, -1.0, "{k:?} zooms out");
    }
    // The Road tool keeps [ ] and Page Up/Down; . and , still zoom.
    let road = KeyCaptures { brackets: true, pages: true };
    assert_eq!(orbit_keys(held(&[KeyCode::BracketLeft, KeyCode::PageUp]), false, road).zoom, 0.0);
    assert_eq!(orbit_keys(held(&[KeyCode::Period]), false, road).zoom, 1.0);
    // Walk: move, turn, look.
    assert_eq!(walk_keys(held(&[KeyCode::KeyW, KeyCode::KeyD])), (Vec2::new(1.0, 1.0), 0.0, 0.0));
    assert_eq!(walk_keys(held(&[KeyCode::KeyQ, KeyCode::KeyR])), (Vec2::ZERO, 1.0, 1.0));
    assert_eq!(walk_keys(held(&[KeyCode::KeyE, KeyCode::End])), (Vec2::ZERO, -1.0, -1.0));
}

#[test]
fn north_is_the_nearest_whole_turn() {
    use std::f32::consts::TAU;
    assert_eq!(north_of(0.3), 0.0);
    assert_eq!(north_of(TAU - 0.2), TAU);
    assert_eq!(north_of(-TAU - 0.1), -TAU);
}

#[test]
fn key_names_parse() {
    assert_eq!(key_from_name("w"), Some(KeyCode::KeyW));
    assert_eq!(key_from_name("PageUp"), Some(KeyCode::PageUp));
    assert_eq!(key_from_name("["), Some(KeyCode::BracketLeft));
    assert_eq!(key_from_name("3"), Some(KeyCode::Digit3));
    assert_eq!(key_from_name("Hyper"), None);
}

// ---- The whole path: synthetic window events to the camera. ----

const START: (Vec3, f32, f32, f32) = (Vec3::ZERO, 0.7, 0.85, 400.0);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, InputPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)))
        .insert_resource(Sim::new())
        .init_resource::<UiFlags>()
        .init_resource::<CameraMode>()
        .init_resource::<WalkState>()
        .init_resource::<CameraFollow>()
        .init_resource::<CameraFly>()
        .init_resource::<ToolState>()
        .init_resource::<crate::drive::Drive>()
        .add_plugins(NavPlugin)
        .insert_resource(NavSettings::default())
        .add_systems(Update, control_camera);
    let (target, yaw, pitch, distance) = START;
    app.world_mut().spawn((OrbitCamera { target, yaw, pitch, distance }, Transform::default(), Projection::default(), DistanceFog::default()));
    app.update();
    app
}

fn cam(app: &mut App) -> (Vec3, f32, f32, f32) {
    let mut q = app.world_mut().query::<&OrbitCamera>();
    let c = q.single(app.world()).unwrap();
    (c.target, c.yaw, c.pitch, c.distance)
}

/// Play frames of synthetic events (as the remote `input` command does)
/// and a couple of quiet frames after.
fn play(app: &mut App, frames: Vec<Vec<Synth>>) {
    let n = frames.len();
    app.world_mut().resource_mut::<NavRemote>().frames.extend(frames);
    for _ in 0..n + 2 {
        app.update();
    }
}

fn drag(button: MouseButton, from: Vec2, to: Vec2, steps: usize, raw: Option<Vec2>) -> Vec<Vec<Synth>> {
    let mut f = vec![vec![Synth::Cursor(from)], vec![Synth::Button(button, true)]];
    for i in 1..=steps {
        let mut ev = vec![Synth::Cursor(from.lerp(to, i as f32 / steps as f32))];
        if let Some(r) = raw {
            ev.push(Synth::Raw(r));
        }
        f.push(ev);
    }
    f.push(vec![Synth::Button(button, false)]);
    f
}

fn wheel(unit: MouseScrollUnit, d: Vec2, n: usize) -> Vec<Vec<Synth>> {
    (0..n).map(|_| vec![Synth::Wheel(unit, d)]).collect()
}

#[test]
fn a_normal_mouse_still_orbits_pans_and_zooms() {
    let mut app = app();
    // Right-drag 100 px right with the raw motion a local mouse also sends.
    play(&mut app, drag(MouseButton::Right, Vec2::new(400.0, 300.0), Vec2::new(500.0, 300.0), 10, Some(Vec2::new(10.0, 0.0))));
    let (_, yaw, _, _) = cam(&mut app);
    assert!((yaw - (START.1 - 100.0 * 0.005)).abs() < 1e-3, "right-drag turns by the pointer's travel, not twice: {yaw}");
    // Middle-drag pans.
    play(&mut app, drag(MouseButton::Middle, Vec2::new(400.0, 300.0), Vec2::new(400.0, 400.0), 10, Some(Vec2::new(0.0, 10.0))));
    let (target, _, _, d0) = cam(&mut app);
    assert!(target.xz().length() > 10.0, "middle-drag pans: {target}");
    // A wheel notch zooms 12%.
    play(&mut app, wheel(LINE, Vec2::new(0.0, 1.0), 1));
    let (t1, _, _, d1) = cam(&mut app);
    assert!((d1 - d0 * 0.88).abs() < 0.5, "{d0} -> {d1}");
    assert!((t1 - target).xz().length() < 1.0, "the wheel does not pan");
    assert!(app.world().resource::<NavInput>().wheel_seen);
}

#[test]
fn a_remote_desktop_pointer_drags_without_raw_motion() {
    let mut app = app();
    // Over RDP: the cursor moves, the raw motion is absolute nonsense.
    play(&mut app, drag(MouseButton::Right, Vec2::new(400.0, 300.0), Vec2::new(340.0, 340.0), 6, Some(Vec2::new(31000.0, 18000.0))));
    let (_, yaw, pitch, _) = cam(&mut app);
    assert!((yaw - (START.1 + 60.0 * 0.005)).abs() < 1e-3, "{yaw}");
    assert!((pitch - (START.2 + 40.0 * 0.005)).abs() < 1e-3, "{pitch}");
    assert!(!app.world().resource::<NavInput>().raw_reliable, "absolute raw motion is not trusted again");
    // And with no raw motion at all.
    let mut app = self::app();
    play(&mut app, drag(MouseButton::Right, Vec2::new(400.0, 300.0), Vec2::new(460.0, 300.0), 6, None));
    assert!((cam(&mut app).1 - (START.1 - 0.3)).abs() < 1e-3);
}

#[test]
fn inspect_left_drag_pans_and_a_click_does_not() {
    let mut app = app();
    // A click (no travel) does not move the camera, and is a tap.
    play(&mut app, vec![vec![Synth::Cursor(Vec2::new(300.0, 300.0))], vec![Synth::Button(MouseButton::Left, true)], vec![Synth::Button(MouseButton::Left, false)]]);
    assert_eq!(cam(&mut app).0.xz(), Vec2::ZERO);
    // A drag pans.
    play(&mut app, drag(MouseButton::Left, Vec2::new(300.0, 300.0), Vec2::new(300.0, 200.0), 10, None));
    let (t, _, _, _) = cam(&mut app);
    assert!(t.xz().length() > 20.0, "left-drag pans in Inspect: {t}");
    // In the Road tool the left button is the tool's; Alt+drag turns.
    let mut app = self::app();
    app.world_mut().resource_mut::<ToolState>().tool = Tool::Road;
    play(&mut app, drag(MouseButton::Left, Vec2::new(300.0, 300.0), Vec2::new(300.0, 200.0), 10, None));
    assert_eq!(cam(&mut app).0.xz(), Vec2::ZERO);
    let mut f = drag(MouseButton::Left, Vec2::new(300.0, 300.0), Vec2::new(380.0, 300.0), 8, None);
    f[0].push(Synth::Key(KeyCode::AltLeft, true));
    f.last_mut().unwrap().push(Synth::Key(KeyCode::AltLeft, false));
    play(&mut app, f);
    assert!((cam(&mut app).1 - (START.1 - 0.4)).abs() < 1e-3, "Alt+left-drag turns");
}

#[test]
fn the_left_tap_is_seen_on_release_only_without_a_drag() {
    let mut app = app();
    app.world_mut().resource_mut::<NavRemote>().frames.extend([vec![Synth::Cursor(Vec2::new(300.0, 300.0))], vec![Synth::Button(MouseButton::Left, true)]]);
    app.update();
    app.update();
    assert!(!app.world().resource::<NavInput>().left_tap);
    app.world_mut().resource_mut::<NavRemote>().frames.push_back(vec![Synth::Button(MouseButton::Left, false)]);
    app.update();
    assert!(app.world().resource::<NavInput>().left_tap);
    // Dragged: no tap.
    app.world_mut().resource_mut::<NavRemote>().frames.extend([vec![Synth::Button(MouseButton::Left, true)], vec![Synth::Cursor(Vec2::new(340.0, 300.0))], vec![Synth::Button(MouseButton::Left, false)]]);
    let mut taps = 0;
    for _ in 0..4 {
        app.update();
        taps += app.world().resource::<NavInput>().left_tap as u32;
    }
    assert_eq!(taps, 0);
}

#[test]
fn a_touchpad_scroll_pans_and_a_pinch_zooms() {
    let mut app = app();
    // A Windows precision touchpad: small fractional wheel steps.
    play(&mut app, wheel(LINE, Vec2::new(0.0, -0.08), 20));
    let (t, _, _, d) = cam(&mut app);
    assert_eq!(d, START.3, "a touchpad scroll does not zoom");
    assert!(t.xz().length() > 20.0, "it pans: {t}");
    assert!(!app.world().resource::<NavInput>().wheel_seen, "the pad stays up");
    // Its pinch: Ctrl + the same steps.
    let mut f = wheel(LINE, Vec2::new(0.0, 0.1), 10);
    f[0].insert(0, Synth::Key(KeyCode::ControlLeft, true));
    f.push(vec![Synth::Key(KeyCode::ControlLeft, false)]);
    play(&mut app, f);
    let (_, _, _, d2) = cam(&mut app);
    assert!(d2 < START.3 * 0.95, "Ctrl+scroll zooms in: {d2}");
    // A macOS touchpad: pixels, and real pinch and rotate gestures.
    let (t0, y0, _, _) = cam(&mut app);
    play(&mut app, wheel(PIXEL, Vec2::new(12.0, 0.0), 5));
    assert!((cam(&mut app).0 - t0).xz().length() > 10.0);
    play(&mut app, (0..5).map(|_| vec![Synth::Pinch(-0.05)]).collect());
    assert!(cam(&mut app).3 > d2, "pinching in zooms out");
    play(&mut app, vec![vec![Synth::Rotate(30.0)]]);
    assert!((cam(&mut app).1 - y0 - 30f32.to_radians()).abs() < 1e-3);
    // Set to Zoom, the touchpad zooms like a wheel.
    app.world_mut().resource_mut::<NavSettings>().scroll = ScrollMode::Zoom;
    let before = cam(&mut app).3;
    play(&mut app, wheel(LINE, Vec2::new(0.0, 0.25), 4));
    assert!(cam(&mut app).3 < before);
}

#[test]
fn keys_and_the_pad_move_the_orbit() {
    let mut app = app();
    let held = |k: KeyCode, frames: usize| {
        let mut f = vec![vec![Synth::Key(k, true)]];
        f.extend((0..frames).map(|_| Vec::new()));
        f.push(vec![Synth::Key(k, false)]);
        f
    };
    play(&mut app, held(KeyCode::PageUp, 20));
    let d = cam(&mut app).3;
    assert!(d < START.3 * 0.7, "Page Up zooms in: {d}");
    play(&mut app, held(KeyCode::Home, 10));
    assert!(cam(&mut app).2 > START.2, "Home tilts");
    // The Road tool keeps Page Up; . still zooms.
    app.world_mut().resource_mut::<ToolState>().tool = Tool::Road;
    play(&mut app, held(KeyCode::PageUp, 10));
    assert_eq!(cam(&mut app).3, d);
    play(&mut app, held(KeyCode::Period, 10));
    assert!(cam(&mut app).3 < d);
    // The pad: a tap of zoom out, a held pan, then north.
    let d = cam(&mut app).3;
    app.world_mut().resource_mut::<NavPad>().tap = NavMove { zoom: -1.0, ..default() };
    app.update();
    assert!(cam(&mut app).3 > d, "a tap of the pad's minus zooms out");
    app.world_mut().resource_mut::<NavRemote>().holds.push((NavMove { pan: Vec2::X, ..default() }, 0.2));
    let t = cam(&mut app).0;
    for _ in 0..20 {
        app.update();
    }
    assert!((cam(&mut app).0 - t).xz().length() > 5.0, "a held pad arrow pans");
    app.world_mut().resource_mut::<NavPad>().north = true;
    for _ in 0..120 {
        app.update();
    }
    assert_eq!(cam(&mut app).1, 0.0, "the compass faces north");
}

#[test]
fn walk_mode_works_without_mouselook() {
    let mut app = app();
    *app.world_mut().resource_mut::<CameraMode>() = CameraMode::Walk;
    app.update();
    let yaw0 = app.world().resource::<WalkState>().yaw;
    let mut f = vec![vec![Synth::Key(KeyCode::KeyQ, true)]];
    f.extend((0..10).map(|_| Vec::new()));
    f.push(vec![Synth::Key(KeyCode::KeyQ, false)]);
    play(&mut app, f);
    let yaw1 = app.world().resource::<WalkState>().yaw;
    assert!(yaw1 > yaw0 + 0.1, "Q turns left on foot: {yaw0} -> {yaw1}");
    // Drag to look: a left-drag turns the head with the pointer free.
    app.world_mut().resource_mut::<NavSettings>().drag_look = true;
    play(&mut app, drag(MouseButton::Left, Vec2::new(300.0, 300.0), Vec2::new(400.0, 300.0), 10, None));
    let yaw2 = app.world().resource::<WalkState>().yaw;
    assert!(yaw2 < yaw1 - 0.1, "dragging right looks right: {yaw1} -> {yaw2}");
    // Mouselook that gets no raw motion gives way to dragging by itself.
    let mut app = self::app();
    *app.world_mut().resource_mut::<CameraMode>() = CameraMode::Walk;
    app.update();
    play(&mut app, (0..40).map(|i| vec![Synth::Cursor(Vec2::new(300.0 + i as f32 * 3.0, 300.0))]).collect());
    assert!(!app.world().resource::<NavInput>().raw_reliable);
}

/// [r5-drive] Drive mode with the keyboard alone: Shift+Tab gets into a
/// car on the nearest street, W drives it, A/D steer, Q turns the camera,
/// Space is the handbrake (not pause), Esc gets out to Orbit over the car,
/// and the traffic forgets it.
#[test]
fn drive_mode_with_the_keyboard_only() {
    let mut app = app();
    app.add_systems(Update, crate::drive::drive_input.after(crate::nav::gather).before(control_camera));
    let hold = |k: KeyCode, frames: usize| {
        let mut f = vec![vec![Synth::Key(k, true)]];
        f.extend((0..frames).map(|_| Vec::new()));
        f.push(vec![Synth::Key(k, false)]);
        f
    };
    play(&mut app, vec![vec![Synth::Key(KeyCode::ShiftLeft, true), Synth::Key(KeyCode::Tab, true)], vec![Synth::Key(KeyCode::Tab, false), Synth::Key(KeyCode::ShiftLeft, false)]]);
    assert_eq!(*app.world().resource::<CameraMode>(), CameraMode::Drive);
    let start = app.world().resource::<crate::drive::Drive>().car.expect("in a car on the highway");
    assert_eq!(app.world().resource::<Sim>().speed_index, 0, "the game at 1x while driving");
    play(&mut app, hold(KeyCode::KeyW, 150));
    let car = app.world().resource::<crate::drive::Drive>().car.unwrap();
    assert!(car.pos.distance(start.pos) > 10.0 && car.v > 3.0, "W drives: {:?} at {} m/s", car.pos, car.v);
    assert!(app.world().resource::<Sim>().player_car.is_some(), "the traffic is handed the car (r7-sim-thread: with every batch)");
    // Steering with D turns it (heading up, the sim's right).
    let h0 = car.heading;
    play(&mut app, vec![vec![Synth::Key(KeyCode::KeyW, true), Synth::Key(KeyCode::KeyD, true)]].into_iter().chain((0..40).map(|_| Vec::new())).chain([vec![Synth::Key(KeyCode::KeyW, false), Synth::Key(KeyCode::KeyD, false)]]).collect());
    let car = app.world().resource::<crate::drive::Drive>().car.unwrap();
    assert!(crate::chase::wrap(car.heading - h0) > 0.05 || car.bumps > 0, "D steers right: {h0} -> {}", car.heading);
    // Q swings the camera round the car.
    play(&mut app, hold(KeyCode::KeyQ, 20));
    assert!(app.world().resource::<crate::drive::Drive>().nudge.look.x.abs() > 0.1);
    // Space: the handbrake, not the pause.
    play(&mut app, hold(KeyCode::Space, 30));
    assert!(!app.world().resource::<Sim>().paused);
    // Escape: out, the orbit over the car, the traffic told.
    let at = app.world().resource::<crate::drive::Drive>().car.unwrap().pos;
    play(&mut app, vec![vec![Synth::Key(KeyCode::Escape, true)], vec![Synth::Key(KeyCode::Escape, false)]]);
    assert_eq!(*app.world().resource::<CameraMode>(), CameraMode::Orbit);
    assert!(app.world().resource::<crate::drive::Drive>().car.is_none());
    assert!(app.world().resource::<Sim>().player_car.is_none());
    let (target, ..) = cam(&mut app);
    assert!(Vec2::new(target.x, target.z).distance(at) < 15.0, "orbit over the car: {target:?} vs {at:?}");
    assert_eq!(app.world().resource::<Sim>().speed_index, crate::sim::DEFAULT_SPEED, "the game's speed back");
}
