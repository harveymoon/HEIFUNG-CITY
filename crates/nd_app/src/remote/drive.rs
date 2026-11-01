//! [r5-drive] `drive`: Drive mode from the remote API. Get into a car
//! (`start`, optionally `at` a point), drive it for `ticks` sim ticks with
//! fixed controls right now (the car's physics and the traffic stepped
//! together, as the game would at 1×), hold controls for `seconds` of real
//! play, read where it is, and get out (`stop`).

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use crate::camera::CameraMode;
use crate::drive::Controls;

/// The physics step the remote drive takes (s).
const STEP: f32 = 1.0 / 30.0;

fn controls(cmd: &Value) -> Result<Controls, String> {
    let unit = |name: &str, lo: f32| -> Result<f32, String> {
        match opt_f32(cmd, name)? {
            Some(x) if (lo..=1.0).contains(&x) => Ok(x),
            Some(_) => Err(format!("{name}: {lo}..1")),
            None => Ok(0.0),
        }
    };
    Ok(Controls { throttle: unit("throttle", 0.0)?, brake: unit("brake", 0.0)?, steer: unit("steer", -1.0)?, handbrake: opt_bool(cmd, "handbrake")?.unwrap_or(false), horn: opt_bool(cmd, "horn")?.unwrap_or(false) })
}

fn state(ctx: &Ctx, held_max: usize) -> Value {
    let d = &*ctx.drive;
    let Some(c) = d.car else {
        return json!({ "driving": false, "mode": format!("{:?}", *ctx.mode).to_ascii_lowercase() });
    };
    json!({
        "driving": *ctx.mode == CameraMode::Drive,
        "pos": [round(c.pos.x), round(c.pos.y)], "floor": round(c.floor), "heading": round(c.heading),
        "bearing": round(crate::drive::bearing(c.heading)), "speed_kmh": round(c.v * 3.6),
        "street": d.street, "limit_kmh": d.limit.map(|l| round(l * 3.6)), "on_road": c.on_road, "in_tunnel": d.in_tunnel,
        "bumps": c.bumps, "last_bump": c.last_bump.map(|b| b.label()), "held_behind": d.held, "held_max": held_max,
        "red_runs": d.red_runs, "assist": d.assist, "view": format!("{:?}", d.view).to_ascii_lowercase(),
        "traffic_sees_player": ctx.sim.world.traffic.player.is_some(), "game_speed": crate::sim::SPEEDS[ctx.sim.speed_index].0,
        "tick": ctx.sim.world.clock.tick,
    })
}

pub(super) fn drive(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if opt_bool(cmd, "stop")? == Some(true) {
        // Out at the next frame (the camera's switch puts the orbit there).
        *ctx.mode = CameraMode::Orbit;
        return Ok(json!({ "stopping": true, "at": ctx.drive.car.map(|c| v2(c.pos)) }));
    }
    if opt_bool(cmd, "start")? == Some(true) || (field(cmd, "at").is_some() && !ctx.drive.active()) {
        ctx.follow.stop();
        ctx.fly.flight = None;
        let near = match opt_vec2(cmd, "at")? {
            Some(p) => p,
            None => ctx.cams.iter().next().map(|c| nd_sim::Vec2::new(c.target.x, c.target.z)).unwrap_or_default(),
        };
        let mut d = std::mem::take(&mut *ctx.drive);
        let car = d.begin(&mut ctx.sim, near);
        *ctx.drive = d;
        if car.is_none() {
            return Err("no street to drive on near there".into());
        }
        if let Some(h) = opt_f32(cmd, "heading")? {
            if let Some(c) = ctx.drive.car.as_mut() {
                c.heading = h;
            }
        }
        *ctx.mode = CameraMode::Drive;
    }
    if let Some(a) = opt_bool(cmd, "assist")? {
        ctx.drive.assist = a;
    }
    if let Some(v) = opt_str(cmd, "view")? {
        ctx.drive.view = match v {
            "chase" => crate::drive::DriveView::Chase,
            "bonnet" => crate::drive::DriveView::Bonnet,
            _ => return Err("view: chase|bonnet".into()),
        };
    }
    let c = controls(cmd)?;
    let mut held_max = ctx.drive.held;
    if let Some(ticks) = opt_u64(cmd, "ticks")? {
        if !(1..=900).contains(&ticks) {
            return Err("ticks: 1..900".into());
        }
        if !ctx.drive.active() {
            return Err("not driving: start first".into());
        }
        let per_tick = ctx.sim.world.config.traffic_seconds_per_tick.max(0.1);
        let steps = (per_tick / STEP).round().max(1.0) as usize;
        for _ in 0..ticks {
            // As in play: the traffic is told where the car is when the
            // tick runs (and predicts it on from there), then the car
            // drives through the seconds that tick shows.
            ctx.drive.tell_traffic(&mut ctx.sim);
            ctx.sim.step_ticks(1);
            ctx.drive.tell_traffic(&mut ctx.sim);
            held_max = held_max.max(ctx.sim.world.traffic.player_held.len());
            for _ in 0..steps {
                ctx.drive.advance(&ctx.sim, c, STEP);
            }
            let mut d = std::mem::take(&mut *ctx.drive);
            d.read_road(&ctx.sim, &mut ctx.flags);
            *ctx.drive = d;
        }
    } else if let Some(secs) = opt_f32(cmd, "seconds")? {
        if !(0.0..=60.0).contains(&secs) {
            return Err("seconds: 0..60".into());
        }
        ctx.drive.remote = Some((c, secs));
    }
    Ok(state(ctx, held_max))
}
