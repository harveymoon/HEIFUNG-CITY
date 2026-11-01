//! [laptop-nav] `nav`: the navigation settings and state, and the pad's
//! moves (`do`); `input`: synthetic pointer, wheel, gesture and key events
//! played into the game's input a frame at a time, as a window would send
//! them (nothing reaches the operating system).

use bevy::input::mouse::MouseScrollUnit;
use bevy::prelude::*;
use serde_json::{json, Value};

use super::ctx::Ctx;
use crate::nav::{key_from_name, NavMove, NavSettings, PadMode, ScrollMode, Synth};

fn settings_json(s: &NavSettings) -> Value {
    json!({
        "scroll": s.scroll.label().to_ascii_lowercase(), "pan_speed": s.pan_speed, "zoom_speed": s.zoom_speed, "turn_speed": s.turn_speed,
        "invert_zoom": s.invert_zoom, "invert_scroll_pan": s.invert_scroll_pan, "invert_tilt": s.invert_tilt,
        "left_drag_pan": s.left_drag_pan, "pad": match s.pad { PadMode::Auto => "auto", PadMode::On => "on", PadMode::Off => "off" }, "drag_look": s.drag_look,
    })
}

/// The pad's move for an action name (rates, as the keys give).
pub(crate) fn action_move(name: &str) -> Option<NavMove> {
    let m = |pan: Vec2, zoom: f32, yaw: f32, tilt: f32| Some(NavMove { pan, zoom, yaw, tilt });
    match name {
        "pan_up" | "up" | "north" => m(Vec2::Y, 0.0, 0.0, 0.0),
        "pan_down" | "down" | "south" => m(-Vec2::Y, 0.0, 0.0, 0.0),
        "pan_left" | "left" | "west" => m(-Vec2::X, 0.0, 0.0, 0.0),
        "pan_right" | "right" | "east" => m(Vec2::X, 0.0, 0.0, 0.0),
        "zoom_in" | "in" => m(Vec2::ZERO, 1.0, 0.0, 0.0),
        "zoom_out" | "out" => m(Vec2::ZERO, -1.0, 0.0, 0.0),
        "rotate_left" | "turn_left" => m(Vec2::ZERO, 0.0, 1.0, 0.0),
        "rotate_right" | "turn_right" => m(Vec2::ZERO, 0.0, -1.0, 0.0),
        "tilt_up" | "steeper" => m(Vec2::ZERO, 0.0, 0.0, 1.0),
        "tilt_down" | "flatter" => m(Vec2::ZERO, 0.0, 0.0, -1.0),
        _ => None,
    }
}

pub(super) fn nav(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if let Some(set) = cmd.get("set") {
        let mut s = *ctx.nav_settings;
        let obj = set.as_object().ok_or("set: an object")?;
        for (k, v) in obj {
            let num = || v.as_f64().map(|x| x as f32).filter(|x| (0.1..=5.0).contains(x)).ok_or(format!("{k}: a number 0.1..5"));
            let flag = || v.as_bool().ok_or(format!("{k}: true or false"));
            match k.as_str() {
                "scroll" => {
                    let t = v.as_str().unwrap_or("").to_ascii_lowercase();
                    s.scroll = ScrollMode::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(&t)).ok_or("scroll: auto|zoom|pan")?;
                }
                "pad" => {
                    s.pad = match v.as_str().unwrap_or("").to_ascii_lowercase().as_str() {
                        "auto" => PadMode::Auto,
                        "on" | "always" => PadMode::On,
                        "off" => PadMode::Off,
                        _ => return Err("pad: auto|on|off".into()),
                    }
                }
                "pan_speed" => s.pan_speed = num()?,
                "zoom_speed" => s.zoom_speed = num()?,
                "turn_speed" => s.turn_speed = num()?,
                "invert_zoom" => s.invert_zoom = flag()?,
                "invert_scroll_pan" => s.invert_scroll_pan = flag()?,
                "invert_tilt" => s.invert_tilt = flag()?,
                "left_drag_pan" => s.left_drag_pan = flag()?,
                "drag_look" => s.drag_look = flag()?,
                _ => return Err(format!("set: unknown setting {k:?}")),
            }
        }
        *ctx.nav_settings = s;
        crate::ui::theme::save_nav(&s);
    }
    if let Some(action) = cmd.get("do").and_then(Value::as_str) {
        let seconds = cmd.get("seconds").and_then(Value::as_f64).unwrap_or(crate::nav::TAP_SECS as f64) as f32;
        if !(0.0..=10.0).contains(&seconds) {
            return Err("seconds: 0..10".into());
        }
        match action {
            "north" | "face_north" => ctx.nav_pad.north = true,
            "reset" | "level" => {
                ctx.nav_pad.north = true;
                ctx.nav_pad.level = true;
            }
            a => {
                let m = action_move(a).ok_or_else(|| format!("unknown action {a:?}; pan_up|pan_down|pan_left|pan_right|zoom_in|zoom_out|rotate_left|rotate_right|tilt_up|tilt_down|north|reset"))?;
                ctx.nav_remote.holds.push((m, seconds));
            }
        }
    }
    let n = &*ctx.nav_input;
    Ok(json!({
        "settings": settings_json(&ctx.nav_settings),
        "raw_reliable": n.raw_reliable, "wheel_seen": n.wheel_seen, "touchpad_seen": n.touchpad_seen,
        "last_scroll": n.last_scroll.map(|k| k.label()), "pad_shown": crate::ui::nav_pad::shown(&ctx.nav_settings, n),
        "queued_frames": ctx.nav_remote.frames.len(), "holds": ctx.nav_remote.holds.len(),
        "camera": super::camera::camera_pose(ctx),
    }))
}

fn v2(v: &Value, what: &str) -> Result<Vec2, String> {
    let a = v.as_array().filter(|a| a.len() == 2).ok_or(format!("{what}: [x, y]"))?;
    let f = |i: usize| a[i].as_f64().map(|x| x as f32).ok_or(format!("{what}: numbers"));
    Ok(Vec2::new(f(0)?, f(1)?))
}

fn button(v: &Value) -> Result<MouseButton, String> {
    match v.as_str().unwrap_or("") {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        b => Err(format!("button {b:?}: left|right|middle")),
    }
}

/// One event object into synthetic events.
fn event(e: &Value) -> Result<Vec<Synth>, String> {
    let down = e.get("down").and_then(Value::as_bool).unwrap_or(true);
    if let Some(p) = e.get("cursor") {
        return Ok(vec![Synth::Cursor(v2(p, "cursor")?)]);
    }
    if e.get("leave").is_some() {
        return Ok(vec![Synth::CursorLeft]);
    }
    if let Some(b) = e.get("button") {
        return Ok(vec![Synth::Button(button(b)?, down)]);
    }
    if let Some(w) = e.get("wheel") {
        let unit = match e.get("unit").and_then(Value::as_str).unwrap_or("line") {
            "line" => MouseScrollUnit::Line,
            "pixel" => MouseScrollUnit::Pixel,
            u => return Err(format!("unit {u:?}: line|pixel")),
        };
        return Ok(vec![Synth::Wheel(unit, v2(w, "wheel")?)]);
    }
    if let Some(p) = e.get("pinch").and_then(Value::as_f64) {
        return Ok(vec![Synth::Pinch(p as f32)]);
    }
    if let Some(r) = e.get("rotate").and_then(Value::as_f64) {
        return Ok(vec![Synth::Rotate(r as f32)]);
    }
    if let Some(r) = e.get("raw") {
        return Ok(vec![Synth::Raw(v2(r, "raw")?)]);
    }
    if let Some(k) = e.get("key").and_then(Value::as_str) {
        return Ok(vec![Synth::Key(key_from_name(k).ok_or(format!("unknown key {k:?}"))?, down)]);
    }
    Err(format!("unknown event {e}; cursor|leave|button|wheel|pinch|rotate|raw|key"))
}

pub(super) fn input(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let mut frames: Vec<Vec<Synth>> = Vec::new();
    // `drag`: press at `from`, move to `to` over `frames` frames, release.
    if let Some(d) = cmd.get("drag") {
        let b = button(d.get("button").unwrap_or(&json!("left")))?;
        let from = v2(d.get("from").ok_or("drag: from")?, "from")?;
        let to = v2(d.get("to").ok_or("drag: to")?, "to")?;
        let n = d.get("frames").and_then(Value::as_u64).unwrap_or(20).clamp(1, 600) as usize;
        let mods: Vec<KeyCode> = d.get("hold").and_then(Value::as_array).map(|a| a.iter().filter_map(|k| k.as_str().and_then(key_from_name)).collect()).unwrap_or_default();
        let mut first: Vec<Synth> = mods.iter().map(|k| Synth::Key(*k, true)).collect();
        first.push(Synth::Cursor(from));
        frames.push(first);
        frames.push(vec![Synth::Button(b, true)]);
        for i in 1..=n {
            frames.push(vec![Synth::Cursor(from.lerp(to, i as f32 / n as f32))]);
        }
        let mut last = vec![Synth::Button(b, false)];
        last.extend(mods.iter().map(|k| Synth::Key(*k, false)));
        frames.push(last);
        frames.push(Vec::new());
    }
    if let Some(list) = cmd.get("frames") {
        let list = list.as_array().ok_or("frames: a list of frames, each a list of events")?;
        for f in list {
            if let Some(n) = f.get("wait").and_then(Value::as_u64) {
                frames.extend((0..n.min(600)).map(|_| Vec::new()));
                continue;
            }
            let events = match f {
                Value::Array(a) => a.iter().map(event).collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect(),
                e => event(e)?,
            };
            frames.push(events);
        }
    }
    if frames.is_empty() {
        return Err("input: give frames [[{cursor:[x,y]}, {button:'right'}], ...] or drag {button, from, to, frames}".into());
    }
    let n = frames.len();
    ctx.nav_remote.frames.extend(frames);
    Ok(json!({ "queued": n, "note": "played one frame at a time from the next frame; read `camera` or `nav` after" }))
}
