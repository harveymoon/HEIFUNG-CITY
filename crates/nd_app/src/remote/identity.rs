//! [identity] `city_name` and `game_history`: the city's name and game id,
//! and the game's history of autosaves (list, bookmark, mark now, revert).
//! The storage is `ui/history.rs`; the save files are `ui/saves.rs`.

use std::path::Path;

use serde_json::{json, Value};

use super::view::*;
use super::ctx::Ctx;
use super::dispatch::Replaces;
use super::files::{absolute, allowed_path, slash};
use super::status::clock;
use crate::ui::history;
use crate::ui::saves::{self, DIR};

/// The city's name and id as every reply carries them.
pub fn identity_json(w: &nd_sim::World) -> Value {
    let id = &w.identity;
    json!({ "name": id.name, "display_name": id.display_name(), "game_id": if id.game_id.is_empty() { Value::Null } else { json!(id.game_id) } })
}

/// `city_name {name?}`: read, or rename the city.
pub fn city_name(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if let Some(name) = opt_str(cmd, "name")? {
        let kept = ctx.sim.set_city_name(name);
        ctx.saves.mark_dirty();
        ctx.flags.message = format!("the city is now called {}", ctx.sim.world.identity.display_name());
        let mut v = identity_json(&ctx.sim.world);
        v["renamed"] = json!(true);
        v["note"] = json!(if kept.is_empty() { "an empty name leaves the city unnamed" } else { "saves from now on carry the new name" });
        return Ok(v);
    }
    let mut v = identity_json(&ctx.sim.world);
    v["history_entries"] = json!(history::list(Path::new(DIR), &ctx.sim.world.identity.game_id).len());
    Ok(v)
}

fn entry_json(e: &history::Entry) -> Value {
    let p = slash(&e.path);
    let pic = e.picture();
    json!({
        "path": absolute(&p),
        "file": e.path.file_name().map(|n| n.to_string_lossy().into_owned()),
        "day": e.meta.day,
        "population": e.meta.population,
        "treasury": round(e.meta.treasury),
        "buildings": e.meta.buildings,
        "saved_unix": e.meta.saved_unix,
        "city": e.meta.city_name(),
        "bookmark": e.bookmark(),
        "thumbnail": pic.exists().then(|| absolute(&slash(&pic))),
    })
}

/// The entry `path` (or `file`, its file name) names in the running game's
/// history.
fn entry_of(cmd: &Value, ctx: &Ctx) -> Result<history::Entry, String> {
    let game = &ctx.sim.world.identity.game_id;
    let dir = Path::new(DIR);
    if let Some(file) = opt_str(cmd, "file")? {
        return history::list(dir, game).into_iter().find(|e| e.path.file_name().is_some_and(|n| n.to_string_lossy() == file.trim())).ok_or_else(|| format!("no history entry {file:?} in this game (game_history lists them)"));
    }
    let path = allowed_path(req_str(cmd, "path").map_err(|_| "give path or file (from game_history)".to_string())?, "ron", false)?;
    history::find(dir, game, &path).ok_or_else(|| format!("{} is not an entry of this game's history (game_history lists them)", path.display()))
}

/// What `game_history` checks before it asks for `confirm`: only
/// `revert` replaces the city, and it always asks.
pub(super) fn replaces(cmd: &Value, ctx: &Ctx) -> Result<Replaces, String> {
    if opt_str(cmd, "action")?.unwrap_or("list").trim().to_ascii_lowercase() != "revert" {
        return Ok(Replaces::No);
    }
    let e = entry_of(cmd, ctx)?;
    Ok(Replaces::Always(format!(
        "revert replaces the running city (day {}) with day {}{}; send confirm: true to do it",
        ctx.sim.world.clock.day() + 1,
        e.meta.day,
        e.bookmark().map_or(String::new(), |b| format!(" (\"{b}\")"))
    )))
}

/// `game_history {action?}`: list (default), bookmark, mark, revert.
pub fn game_history(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("list").trim().to_ascii_lowercase();
    let dir = Path::new(DIR);
    match action.as_str() {
        "list" => {
            let game = opt_str(cmd, "game_id")?.map(str::to_string).unwrap_or_else(|| ctx.sim.world.identity.game_id.clone());
            let marked_only = opt_bool(cmd, "marked_only")?.unwrap_or(false);
            let entries: Vec<Value> = history::list(dir, &game).iter().filter(|e| !marked_only || e.bookmark().is_some()).map(entry_json).collect();
            Ok(json!({ "game_id": game, "city": identity_json(&ctx.sim.world), "entries": entries, "kept_unmarked": history::KEPT, "kept_spaced": history::KEPT_SPACED, "spacing_days": history::SPACING, "note": "newest first; every daily autosave lands here, marked ones are never pruned" }))
        }
        "bookmark" | "mark_entry" | "unmark" => {
            let e = entry_of(cmd, ctx)?;
            let label = if action == "unmark" { None } else { opt_str(cmd, "label")? };
            let meta = history::set_bookmark(&e.path, label)?;
            Ok(json!({ "entry": entry_json(&history::Entry { path: e.path, meta }) }))
        }
        "mark" | "mark_now" => {
            let label = opt_str(cmd, "label")?.unwrap_or("").to_string();
            let label = if label.trim().is_empty() { format!("Day {}", ctx.sim.world.clock.day() + 1) } else { label };
            let path = saves::mark_now(&mut ctx.sim, &mut ctx.saves, &label)?;
            let e = history::find(dir, &ctx.sim.world.identity.game_id, &path).ok_or("the entry was not written")?;
            ctx.flags.message = format!("marked \"{label}\" in the game history");
            Ok(json!({ "entry": entry_json(&e), "note": "the thumbnail is written about half a second later" }))
        }
        "revert" => {
            // `confirm` was checked first (`replaces`).
            let e = entry_of(cmd, ctx)?;
            saves::revert(&e.path, &mut ctx.sim, &mut ctx.saves, &mut ctx.flags)?;
            ctx.saves.title = false;
            ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "reverted");
            Ok(json!({ "message": ctx.flags.message, "day": e.meta.day, "clock": clock(&ctx.sim), "paused": ctx.sim.paused, "city": identity_json(&ctx.sim.world) }))
        }
        other => Err(format!("unknown action {other:?}; one of list, bookmark, unmark, mark, revert")),
    }
}
