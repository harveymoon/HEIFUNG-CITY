//! [camera] `follow`, `zoom_to` and `gift`: the inspector's camera and
//! gift buttons from the API; a delivery robot's detail; the target
//! parsing they share with `camera {follow}`.

use nd_sim::bots::{bot_code, bot_name, fleet_colour};
use nd_sim::{BotId, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::view::{building_name, field, key_from, key_str, opt_bool, opt_f32, opt_str, opt_vec2, round, selection, v2};
use super::ctx::Ctx;
use crate::camera::{chase_subject, thing_pose, zoom_distance, CameraMode, FlyGoal, FollowView};
use crate::tools::Selection;

/// The thing a command names: `{citizen|car|bot|building|vehicle: id}`
/// (a bare `id` field may name a citizen or a car), else the player's
/// selection. Errors name what is missing.
pub(super) fn target_of(cmd: &Value, ctx: &Ctx, what: &str) -> Result<Selection, String> {
    let w = &ctx.sim.world;
    if let Some(v) = field(cmd, "citizen") {
        let id = key_from(v)?;
        return w.citizens.contains_key(id).then_some(Selection::Citizen(id)).ok_or_else(|| format!("{what}: no such citizen"));
    }
    if let Some(v) = field(cmd, "car") {
        let id = key_from(v)?;
        return w.traffic.cars.contains_key(id).then_some(Selection::Car(id)).ok_or_else(|| format!("{what}: no such car"));
    }
    if let Some(v) = field(cmd, "bot") {
        let id = key_from(v)?;
        return w.bots.bots.contains_key(id).then_some(Selection::Bot(id)).ok_or_else(|| format!("{what}: no such delivery bot"));
    }
    if let Some(v) = field(cmd, "building") {
        let id = key_from(v)?;
        return w.buildings.contains_key(id).then_some(Selection::Building(id)).ok_or_else(|| format!("{what}: no such building"));
    }
    // [train-inspect]
    if let Some(v) = field(cmd, "train") {
        let id = key_from(v)?;
        return w.rail.trains.contains_key(id).then_some(Selection::Train(id)).ok_or_else(|| format!("{what}: no such train"));
    }
    if let Some(v) = field(cmd, "metro") {
        let id = key_from(v)?;
        return w.subway.trains.contains_key(id).then_some(Selection::Metro(id)).ok_or_else(|| format!("{what}: no such metro train"));
    }
    if let Some(v) = field(cmd, "unit") {
        let id = key_from(v)?;
        return crate::fire_scene::unit_pose(w, id, ctx.sim.frac()).map(|_| Selection::Unit(id)).ok_or_else(|| format!("{what}: no such unit out of its station"));
    }
    if let Some(v) = field(cmd, "vehicle") {
        let id = key_from(v)?;
        return w.vehicles.cars.get(id).is_some_and(|c| c.spot().is_some()).then_some(Selection::Parked(id)).ok_or_else(|| format!("{what}: no such parked vehicle"));
    }
    if let Some(v) = field(cmd, "id") {
        let cid: nd_sim::CitizenId = key_from(v)?;
        let car: nd_sim::CarId = key_from(v)?;
        if w.citizens.contains_key(cid) {
            return Ok(Selection::Citizen(cid));
        }
        if w.traffic.cars.contains_key(car) {
            return Ok(Selection::Car(car));
        }
        return Err(format!("{what}: no such citizen or car"));
    }
    match *ctx.selection {
        Selection::None => Err(format!("{what}: name one of citizen, car, bot, building, vehicle (an id), or select something first")),
        s => Ok(s),
    }
}

/// `follow {target?, stop?, on?, pov?, fly?, view?, turn?, tilt?, zoom?}`:
/// lock the camera on a thing (the selection by default), as the
/// inspector's Follow button does. `stop: true` or `on: false` ends it.
/// `pov: true` looks through their eyes (Walk view), `pov: false` goes back
/// to the follow camera. The camera flies in smoothly first unless `fly:
/// false`. [chase-cam] `view`: `chase` (low behind it, the default) or
/// `high` (the orbit camera riding on it), as F toggles; `turn` / `tilt`
/// (radians) nudge the chase camera round it as a right-drag does (it
/// swings back after a moment), `zoom` sets its distance (times the
/// default, 0.55..3). Following the same thing again keeps the follow and
/// applies only the view and nudges.
pub(super) fn follow(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let stop = opt_bool(cmd, "stop")?.unwrap_or(false) || opt_bool(cmd, "on")? == Some(false);
    if stop {
        ctx.follow.stop();
        ctx.fly.flight = None;
        if *ctx.mode == CameraMode::Walk {
            *ctx.mode = CameraMode::Orbit;
        }
        return Ok(json!({ "following": Value::Null, "camera": super::camera::camera_pose(ctx) }));
    }
    let sel = target_of(cmd, ctx, "follow")?;
    let generation = ctx.sim.generation;
    let again = ctx.follow.is(sel) && ctx.follow.generation == generation;
    if !again {
        ctx.follow.start(sel, generation);
    }
    if let Some(v) = opt_str(cmd, "view")? {
        let view = match v.to_ascii_lowercase().as_str() {
            "chase" => FollowView::Chase,
            "high" | "orbit" => FollowView::High,
            _ => return Err(format!("follow: unknown view {v:?}; chase or high")),
        };
        ctx.follow.set_view(view);
    }
    let turn = opt_f32(cmd, "turn")?.unwrap_or(0.0);
    let tilt = opt_f32(cmd, "tilt")?.unwrap_or(0.0);
    ctx.follow.nudge.turn(SVec2::new(turn, tilt));
    if let Some(c) = opt_bool(cmd, "cutaway")? {
        ctx.follow.cutaway_off = !c;
    }
    if let Some(z) = opt_f32(cmd, "zoom")? {
        ctx.follow.nudge.zoom = z.clamp(crate::chase::ZOOM_MIN, crate::chase::ZOOM_MAX);
    }
    let pose = thing_pose(&ctx.sim, sel);
    if let Some(pov) = opt_bool(cmd, "pov")? {
        if pov && pose.is_none_or(|p| p.heading.is_none()) {
            return Err("pov: a building has no point of view; follow a person, car or bot".into());
        }
        *ctx.mode = if pov { CameraMode::Walk } else { CameraMode::Orbit };
    }
    if *ctx.mode == CameraMode::Orbit && !again && opt_bool(cmd, "fly")?.unwrap_or(true) {
        let d = ctx.cams.iter().next().map_or(90.0, |c| c.distance.min(zoom_distance(&ctx.sim, sel) * 2.0));
        ctx.fly.to(FlyGoal::Thing(sel), Some(d), None);
    }
    Ok(json!({
        "following": selection(&ctx.sim.world, sel), "pov": *ctx.mode == CameraMode::Walk,
        "view": ctx.follow.view.label(), "chase_capable": chase_subject(&ctx.sim, sel).is_some(),
        "nudge": { "turn": round(ctx.follow.nudge.look.x), "tilt": round(ctx.follow.nudge.look.y), "zoom": round(ctx.follow.nudge.zoom) },
        "at": pose.map(|p| v2(p.pos)), "flying": ctx.fly.flying(), "camera": super::camera::camera_pose(ctx),
    }))
}

/// `zoom_to {target? | at?, distance?, pitch?}`: the inspector's "zoom to":
/// a smooth camera move onto a thing (the selection by default; it is
/// tracked while the camera travels) or a point. Ends a follow on anything
/// else. The move takes 0.7 to 2.2 s; the reply comes at once.
pub(super) fn zoom_to(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let distance = opt_f32(cmd, "distance")?;
    let pitch = opt_f32(cmd, "pitch")?;
    let (goal, d, at, what) = if let Some(p) = opt_vec2(cmd, "at")? {
        (FlyGoal::Point(p), distance.unwrap_or(70.0), p, Value::Null)
    } else {
        let sel = target_of(cmd, ctx, "zoom_to")?;
        let pose = thing_pose(&ctx.sim, sel).ok_or("zoom_to: it is nowhere to be seen")?;
        (FlyGoal::Thing(sel), distance.unwrap_or_else(|| zoom_distance(&ctx.sim, sel)), pose.pos, selection(&ctx.sim.world, sel))
    };
    if !matches!(goal, FlyGoal::Thing(s) if ctx.follow.is(s)) {
        ctx.follow.stop();
    }
    if *ctx.mode == CameraMode::Walk {
        *ctx.mode = CameraMode::Orbit;
    }
    ctx.fly.to(goal, Some(d.clamp(12.0, 2500.0)), Some(pitch.unwrap_or(0.62)));
    let from = ctx.cams.iter().next().map(|c| SVec2::new(c.target.x, c.target.z));
    let trip = from.map_or(0.0, |f| f.distance(at));
    Ok(json!({ "to": what, "at": v2(at), "distance": round(d), "seconds": round((0.7 + trip / 1400.0).clamp(0.7, 2.2)) }))
}

/// `gift {citizen?, amount?}`: move money (default 1000) from the city
/// treasury to the citizen's household, as the "gift $1000" button does.
pub(super) fn gift(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let id = match target_of(cmd, ctx, "gift")? {
        Selection::Citizen(id) => id,
        _ => return Err("gift: name a citizen (or select one)".into()),
    };
    let amount = opt_f32(cmd, "amount")?.unwrap_or(nd_sim::GIFT_AMOUNT);
    let g = ctx.sim.world.gift(id, amount)?;
    let w = &ctx.sim.world;
    let name = w.citizens.get(id).map(|c| c.name.clone()).unwrap_or_default();
    ctx.flags.message = format!("Gave ${:.0} to {name}'s household.", g.amount);
    Ok(json!({
        "citizen": key_str(id), "name": name, "amount": round(g.amount), "household": g.household.map(key_str),
        "household_money": g.household.and_then(|h| w.households.get(h)).map(|h| round(h.money)),
        "savings": g.household.is_none().then(|| w.citizens.get(id).map(|c| round(c.savings))).flatten(),
        "treasury": round(g.treasury),
    }))
}

/// A delivery robot as the inspector shows it: name and code, owning
/// business, fleet colour and size, cargo, destination, route.
pub fn bot_detail(w: &World, id: BotId) -> Option<Value> {
    let b = w.bots.bots.get(id)?;
    let [r, g, bl] = fleet_colour(b.shop);
    let hex = format!("#{:02x}{:02x}{:02x}", (r * 255.0) as u8, (g * 255.0) as u8, (bl * 255.0) as u8);
    let shop_kind = w.buildings.get(b.shop).and_then(|s| s.business.as_ref()).map(|z| z.kind.label());
    let ahead = b.path_ahead();
    Some(json!({
        "id": key_str(id), "code": bot_code(id), "name": bot_name(w, id), "serial": b.serial,
        "owner": { "building": key_str(b.shop), "name": building_name(w, Some(b.shop)), "business_kind": shop_kind, "fleet_colour": hex, "fleet_size": w.bots.fleet_of(b.shop).len() },
        "state": if b.returning { "returning to the shop" } else { "delivering" },
        "cargo": b.cargo().map(|(res, n)| json!({ "resource": res.label(), "amount": round(n) })),
        "destination": { "building": key_str(b.destination()), "name": building_name(w, Some(b.destination())) },
        "customer": { "building": key_str(b.home), "name": building_name(w, Some(b.home)), "household": b.household.map(key_str) },
        "pos": v2(b.pos), "metres_left": round(b.metres_left()), "minutes_left": round(b.metres_left() / nd_sim::bots::BOT_MPS / 60.0),
        "minutes_out": b.age,
        "route": ahead.iter().step_by((ahead.len() / 40).max(1)).map(|p| v2(*p)).collect::<Vec<_>>(),
    }))
}

/// A row for `find {what: "bots"}`.
pub fn bot_row(w: &World, id: BotId) -> Value {
    let b = &w.bots.bots[id];
    json!({ "id": key_str(id), "name": bot_name(w, id), "shop": key_str(b.shop), "pos": v2(b.pos), "returning": b.returning })
}

/// [camera] A citizen's places for the pins: home, work and what they own.
pub fn holdings_json(w: &World, id: nd_sim::CitizenId) -> Value {
    json!(w.holdings_of(id).into_iter().map(|(b, h)| json!({ "building": key_str(b), "name": building_name(w, Some(b)), "kind": h.label() })).collect::<Vec<_>>())
}
