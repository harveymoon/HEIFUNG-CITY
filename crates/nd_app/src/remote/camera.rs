//! `camera`: pose, mode and follow; the pose as `status` reports it.

use bevy::prelude::*;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use crate::camera::CameraMode;
use crate::tools::Selection;

pub(super) fn camera_pose(ctx: &Ctx) -> Value {
    match *ctx.mode {
        CameraMode::Walk => {
            let w = &ctx.walk;
            json!({ "mode": "walk", "pos": [round(w.pos.x), round(w.pos.y), round(w.pos.z)], "yaw": round(w.yaw), "pitch": round(w.pitch) })
        }
        CameraMode::Orbit => match ctx.cams.iter().next() {
            Some(cam) => {
                // The eye as `control_camera` will place it from these
                // parameters; the transform itself lags a frame behind a
                // remote camera command.
                let (sy, cy) = cam.yaw.sin_cos();
                let (sp, cp) = cam.pitch.sin_cos();
                let eye = cam.target + Vec3::new(sy * cp, sp, cy * cp) * cam.distance;
                json!({
                    "mode": "orbit", "target": [round(cam.target.x), round(cam.target.z)], "distance": round(cam.distance),
                    "yaw": round(cam.yaw), "pitch": round(cam.pitch), "eye": [round(eye.x), round(eye.y), round(eye.z)],
                })
            }
            None => json!({ "mode": "orbit" }),
        },
    }
}

pub(super) fn camera(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    // Any explicit pose ends a follow and a flight; `follow` below may
    // start a new one.
    ctx.follow.stop();
    ctx.fly.flight = None;
    if let Some(mode) = opt_str(cmd, "mode")? {
        *ctx.mode = match mode.to_ascii_lowercase().as_str() {
            "orbit" => CameraMode::Orbit,
            "walk" => CameraMode::Walk,
            _ => return Err(format!("unknown camera mode {mode:?}; orbit or walk")),
        };
    }
    let w = &ctx.sim.world;
    let mut follow: Option<Selection> = None;
    if let Some(f) = field(cmd, "follow") {
        // A citizen id, a car id, or an object {citizen|car|building: id}.
        let (kind, id) = match f {
            Value::Object(m) => m.iter().next().map(|(k, v)| (k.as_str(), v)).ok_or("follow: empty object")?,
            v => ("citizen", v),
        };
        follow = Some(match kind {
            "citizen" => {
                // A bare id may name a citizen or a car; try both.
                let cid: nd_sim::CitizenId = key_from(id)?;
                let car: nd_sim::CarId = key_from(id)?;
                if w.citizens.contains_key(cid) {
                    Selection::Citizen(cid)
                } else if w.traffic.cars.contains_key(car) {
                    Selection::Car(car)
                } else {
                    return Err("follow: no such citizen or car".into());
                }
            }
            "car" => {
                let id = key_from(id)?;
                if !w.traffic.cars.contains_key(id) {
                    return Err("follow: no such car".into());
                }
                Selection::Car(id)
            }
            "building" => {
                let id = key_from(id)?;
                if !w.buildings.contains_key(id) {
                    return Err("follow: no such building".into());
                }
                Selection::Building(id)
            }
            "bot" => {
                let id = key_from(id)?;
                if !w.bots.bots.contains_key(id) {
                    return Err("follow: no such delivery bot".into());
                }
                Selection::Bot(id)
            }
            // [fire-scene]
            "unit" => {
                let id = key_from(id)?;
                if crate::fire_scene::unit_pose(w, id, ctx.sim.frac()).is_none() {
                    return Err("follow: no such unit out of its station".into());
                }
                Selection::Unit(id)
            }
            // [train-inspect]
            "train" => {
                let id = key_from(id)?;
                if !w.rail.trains.contains_key(id) {
                    return Err("follow: no such train".into());
                }
                Selection::Train(id)
            }
            "metro" => {
                let id = key_from(id)?;
                if !w.subway.trains.contains_key(id) {
                    return Err("follow: no such metro train".into());
                }
                Selection::Metro(id)
            }
            // [ferry-routes]
            "ship" => {
                let id = key_from(id)?;
                if !w.ports.ships.contains_key(id) {
                    return Err("follow: no such ship".into());
                }
                Selection::Ship(id)
            }
            _ => return Err("follow: one of citizen, car, bot, building, train, metro, ship".into()),
        });
    }
    let look_at = opt_vec2(cmd, "look_at")?;
    let x = opt_f32(cmd, "x")?;
    let z = opt_f32(cmd, "z")?.or(opt_f32(cmd, "y")?);
    let distance = opt_f32(cmd, "distance")?;
    let yaw = opt_f32(cmd, "yaw")?;
    let pitch = opt_f32(cmd, "pitch")?;
    // [pt-ux] Underground the orbit may go below the ground.
    let min_pitch = crate::camera::min_pitch(ctx.tools.depth);
    let mut applied = false;
    for mut cam in &mut ctx.cams {
        if let Some(p) = look_at {
            cam.target = Vec3::new(p.x, 0.0, p.y);
        }
        if let Some(x) = x {
            cam.target.x = x;
        }
        if let Some(z) = z {
            cam.target.z = z;
        }
        if let Some(d) = distance {
            cam.distance = d.clamp(12.0, 2500.0);
        }
        if let Some(y) = yaw {
            cam.yaw = y;
        }
        if let Some(p) = pitch {
            cam.pitch = p.clamp(min_pitch, 1.5);
        }
        if let Some(sel) = follow {
            // Start on the thing right away rather than one frame later.
            let p = crate::camera::thing_pose(&ctx.sim, sel).map(|t| t.pos);
            if let Some(p) = p {
                cam.target = Vec3::new(p.x, 0.0, p.y);
                if distance.is_none() {
                    cam.distance = cam.distance.min(90.0);
                }
            }
        }
        applied = true;
    }
    if !applied {
        return Err("no camera yet".into());
    }
    // Walk mode: `look_at`/x/z place the eye; yaw/pitch turn the head.
    if *ctx.mode == CameraMode::Walk {
        let walk = &mut ctx.walk;
        if let Some(p) = look_at {
            walk.pos.x = p.x;
            walk.pos.z = p.y;
        }
        if let Some(x) = x {
            walk.pos.x = x;
        }
        if let Some(z) = z {
            walk.pos.z = z;
        }
        if let Some(y) = yaw {
            walk.yaw = y;
        }
        if let Some(p) = pitch {
            walk.pitch = p.clamp(-1.35, 1.35);
        }
    }
    if let Some(sel) = follow {
        let generation = ctx.sim.generation;
        ctx.follow.start(sel, generation);
    }
    Ok(json!({ "camera": camera_pose(ctx), "following": follow.map(|s| selection(&ctx.sim.world, s)) }))
}
