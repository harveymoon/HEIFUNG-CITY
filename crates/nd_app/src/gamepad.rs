//! [r15-polish] Gamepads through gilrs, with its panics contained.
//!
//! Bevy's own `GilrsPlugin` is switched off: gilrs 0.11.2 can panic in
//! `Gilrs::gamepad` (`gamepad.rs:474`, index out of bounds with no pads
//! known) when Windows reports a pad it has not listed yet, and a panic in
//! a system took the whole game down (twice for r14-foodtrucks, once at
//! frame 0, with no gamepad attached). This is the same plugin with every
//! gilrs call run through `crash::contain`: a panic there logs one line
//! and gamepads stop for the session; the game goes on.
//!
//! `ND_GAMEPAD=0` leaves gilrs out altogether (no gamepads).

use bevy::input::gamepad::{GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::InputSystems;
use bevy::platform::cell::SyncCell;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use gilrs::{ev::filter::axis_dpad_to_button, EventType, Filter};

/// The gilrs context; `None` once it has panicked (or failed to start).
#[derive(Resource)]
struct Pads {
    gilrs: Option<SyncCell<gilrs::Gilrs>>,
    to_entity: HashMap<gilrs::GamepadId, Entity>,
}

/// Whether gamepads are wanted this run (`ND_GAMEPAD=0`: no).
fn wanted() -> bool {
    !std::env::var("ND_GAMEPAD").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
}

pub struct SafeGamepadPlugin;

impl Plugin for SafeGamepadPlugin {
    fn build(&self, app: &mut App) {
        if !wanted() {
            info!("gamepads off (ND_GAMEPAD=0)");
            return;
        }
        let gilrs = match crate::crash::contain(|| gilrs::GilrsBuilder::new().with_default_filters(false).set_update_state(false).build()) {
            Ok(Ok(g)) => Some(SyncCell::new(g)),
            Ok(Err(e)) => {
                warn!("gamepads off: gilrs did not start ({e})");
                None
            }
            Err(e) => {
                warn!("gamepads off: gilrs panicked starting ({e})");
                None
            }
        };
        app.insert_resource(Pads { gilrs, to_entity: HashMap::default() }).add_systems(PreUpdate, pad_events.before(InputSystems));
    }
}

/// What one gilrs event becomes, gathered inside `contain` and written
/// after it.
enum Out {
    Connected(gilrs::GamepadId, String, Option<u16>, Option<u16>),
    Disconnected(gilrs::GamepadId),
    Button(gilrs::GamepadId, GamepadButton, f32),
    Axis(gilrs::GamepadId, GamepadAxis, f32),
}

/// Bevy's `gilrs_event_system` (and its startup pass on the first run),
/// with the gilrs calls contained.
fn pad_events(
    mut commands: Commands,
    mut pads: ResMut<Pads>,
    mut started: Local<bool>,
    mut events: MessageWriter<RawGamepadEvent>,
    mut connection_events: MessageWriter<GamepadConnectionEvent>,
    mut button_events: MessageWriter<RawGamepadButtonChangedEvent>,
    mut axis_events: MessageWriter<RawGamepadAxisChangedEvent>,
) {
    let Some(cell) = pads.gilrs.as_mut() else { return };
    let first = !*started;
    *started = true;
    let got = crate::crash::contain(|| {
        let gilrs = cell.get();
        let mut out = Vec::new();
        if first {
            for (id, pad) in gilrs.gamepads() {
                out.push(Out::Connected(id, pad.name().to_string(), pad.vendor_id(), pad.product_id()));
            }
        }
        while let Some(ev) = gilrs.next_event().filter_ev(&axis_dpad_to_button, gilrs) {
            gilrs.update(&ev);
            match ev.event {
                // `connected_gamepad`, not `gamepad`: the latter is what
                // panicked on a pad gilrs had no data for.
                EventType::Connected => {
                    if let Some(pad) = gilrs.connected_gamepad(ev.id) {
                        out.push(Out::Connected(ev.id, pad.name().to_string(), pad.vendor_id(), pad.product_id()));
                    }
                }
                EventType::Disconnected => out.push(Out::Disconnected(ev.id)),
                EventType::ButtonChanged(b, v, _) => {
                    if let Some(b) = convert_button(b) {
                        out.push(Out::Button(ev.id, b, v));
                    }
                }
                EventType::AxisChanged(a, v, _) => {
                    if let Some(a) = convert_axis(a) {
                        out.push(Out::Axis(ev.id, a, v));
                    }
                }
                _ => {}
            }
        }
        gilrs.inc();
        out
    });
    let out = match got {
        Ok(out) => out,
        Err(e) => {
            warn!("gamepads off for this session: gilrs panicked ({e})");
            pads.gilrs = None;
            return;
        }
    };
    for o in out {
        match o {
            Out::Connected(id, name, vendor_id, product_id) => {
                let entity = *pads.to_entity.entry(id).or_insert_with(|| commands.spawn_empty().id());
                let event = GamepadConnectionEvent::new(entity, GamepadConnection::Connected { name, vendor_id, product_id });
                events.write(event.clone().into());
                connection_events.write(event);
            }
            // Events for a pad never announced are dropped (Bevy's own
            // system panicked on those too).
            Out::Disconnected(id) => {
                if let Some(&entity) = pads.to_entity.get(&id) {
                    let event = GamepadConnectionEvent::new(entity, GamepadConnection::Disconnected);
                    events.write(event.clone().into());
                    connection_events.write(event);
                }
            }
            Out::Button(id, b, v) => {
                if let Some(&entity) = pads.to_entity.get(&id) {
                    events.write(RawGamepadButtonChangedEvent::new(entity, b, v).into());
                    button_events.write(RawGamepadButtonChangedEvent::new(entity, b, v));
                }
            }
            Out::Axis(id, a, v) => {
                if let Some(&entity) = pads.to_entity.get(&id) {
                    events.write(RawGamepadAxisChangedEvent::new(entity, a, v).into());
                    axis_events.write(RawGamepadAxisChangedEvent::new(entity, a, v));
                }
            }
        }
    }
}

/// bevy_gilrs' converter (private there).
fn convert_button(button: gilrs::Button) -> Option<GamepadButton> {
    use gilrs::Button as B;
    Some(match button {
        B::South => GamepadButton::South,
        B::East => GamepadButton::East,
        B::North => GamepadButton::North,
        B::West => GamepadButton::West,
        B::C => GamepadButton::C,
        B::Z => GamepadButton::Z,
        B::LeftTrigger => GamepadButton::LeftTrigger,
        B::LeftTrigger2 => GamepadButton::LeftTrigger2,
        B::RightTrigger => GamepadButton::RightTrigger,
        B::RightTrigger2 => GamepadButton::RightTrigger2,
        B::Select => GamepadButton::Select,
        B::Start => GamepadButton::Start,
        B::Mode => GamepadButton::Mode,
        B::LeftThumb => GamepadButton::LeftThumb,
        B::RightThumb => GamepadButton::RightThumb,
        B::DPadUp => GamepadButton::DPadUp,
        B::DPadDown => GamepadButton::DPadDown,
        B::DPadLeft => GamepadButton::DPadLeft,
        B::DPadRight => GamepadButton::DPadRight,
        B::Unknown => return None,
    })
}

fn convert_axis(axis: gilrs::Axis) -> Option<GamepadAxis> {
    use gilrs::Axis as A;
    Some(match axis {
        A::LeftStickX => GamepadAxis::LeftStickX,
        A::LeftStickY => GamepadAxis::LeftStickY,
        A::LeftZ => GamepadAxis::LeftZ,
        A::RightStickX => GamepadAxis::RightStickX,
        A::RightStickY => GamepadAxis::RightStickY,
        A::RightZ => GamepadAxis::RightZ,
        A::Unknown | A::DPadX | A::DPadY => return None,
    })
}
