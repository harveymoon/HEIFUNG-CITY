//! Files: `save`, `load`, `list_saves`, `screenshot` and `export`, and the
//! folders they may use.

use std::path::PathBuf;
use std::sync::Mutex;

use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::{guard, identity, list, map, report};
use super::status::clock;
use super::dispatch::Replaces;
use crate::ui::{load_now, save_now};

pub(super) fn save(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if let Some(name) = opt_str(cmd, "name")? {
        // A named game: saves/<file-safe name>.ron, its sidecar and picture.
        let Ctx { sim, saves, flags, .. } = ctx;
        let path = crate::ui::save_as(sim, saves, flags, name)?;
        let p = slash(&path);
        return Ok(json!({ "path": absolute(&p), "name": name.trim(), "thumbnail": absolute(&p.replace(".ron", ".png")), "meta": absolute(&p.replace(".ron", ".meta.json")), "note": "the thumbnail is written about half a second later (rendered offscreen)", "message": flags.message, "identity": identity::identity_json(&sim.world) }));
    }
    match opt_str(cmd, "path")? {
        Some(raw) => {
            let path = allowed_path(raw, "ron", true)?;
            let shown = path.to_string_lossy().replace('\\', "/");
            crate::ui::saves::ensure_game_id(&mut ctx.sim.world);
            ctx.sim.save(&path)?;
            ctx.saves.mark_dirty();
            ctx.flags.message = format!("saved {shown}");
            Ok(json!({ "path": shown }))
        }
        None => {
            let Ctx { sim, saves, flags, .. } = ctx;
            save_now(sim, saves, flags);
            if flags.message.starts_with("save failed") {
                return Err(flags.message.clone());
            }
            Ok(json!({ "path": absolute(crate::ui::LATEST), "thumbnail": absolute("saves/city.png"), "message": flags.message }))
        }
    }
}

/// A path with forward slashes.
pub(super) fn slash(p: &std::path::Path) -> String {
    p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

/// Every save, newest first, with what its sidecar knows and its picture.
pub(super) fn list_saves() -> Value {
    let slots = crate::ui::saves::scan();
    let rows: Vec<Value> = slots
        .iter()
        .map(|s| {
            let modified = s.modified.and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            let bytes = std::fs::metadata(&s.path).map_or(0, |m| m.len());
            json!({
                "path": slash(&s.path),
                "name": s.label,
                "kind": s.kind.key(),
                "modified_unix": modified,
                "bytes": bytes,
                "day": s.meta.as_ref().map(|m| m.day),
                "population": s.meta.as_ref().map(|m| m.population),
                "treasury": s.meta.as_ref().map(|m| round(m.treasury)),
                "buildings": s.meta.as_ref().map(|m| m.buildings),
                "landscape": s.meta.as_ref().map(|m| m.landscape.clone()),
                "thumbnail": s.picture.as_ref().filter(|p| p.exists()).map(|p| absolute(&slash(p))),
                "city": s.city(),
                "game_id": s.meta.as_ref().map(|m| m.game_id.clone()).filter(|g| !g.is_empty()),
            })
        })
        .collect();
    // [identity] The same saves grouped by city name, as the title screen
    // and the Saves window show them.
    let groups: Vec<Value> = crate::ui::saves::group_by_city(&slots).into_iter().map(|(city, members)| json!({ "city": city, "saves": members.iter().map(|&i| slash(&slots[i].path)).collect::<Vec<_>>() })).collect();
    json!({ "saves": rows, "by_city": groups, "latest": crate::ui::LATEST })
}

/// A path a file command may use (see `guard::confine`): inside saves/,
/// screenshots/ or exports/ under the working directory, or the temp
/// folder, ending in `.ext`. `write` makes its folder.
pub(super) fn allowed_path(raw: &str, ext: &str, write: bool) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let path = guard::confine(raw, ext, &cwd, &std::env::temp_dir())?;
    if write {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
    }
    Ok(path)
}

pub(super) fn absolute(path: &str) -> String {
    let p = PathBuf::from(path);
    let abs = if p.is_absolute() { p } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_default() };
    abs.to_string_lossy().replace('\\', "/")
}

/// [fix/review-app] A default file name stamp: unix milliseconds, and a
/// counter after the first in the same millisecond (two shots in one
/// second used to overwrite each other).
pub(super) fn unique_stamp() -> String {
    static LAST: Mutex<(u128, u32)> = Mutex::new((0, 0));
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let Ok(mut last) = LAST.lock() else { return ms.to_string() };
    if last.0 == ms {
        last.1 += 1;
        format!("{ms}-{}", last.1)
    } else {
        *last = (ms, 0);
        ms.to_string()
    }
}

pub(super) fn screenshot(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let default = format!("screenshots/shot-{}.png", unique_stamp());
    // PNG only: Bevy's `jpeg` image feature is not enabled in this build,
    // and the failure would surface only as a log line, not as a reply.
    let path = allowed_path(opt_str(cmd, "path")?.unwrap_or(&default), "png", true)?;
    // The capture runs in the render graph and the file is written by the
    // observer when the GPU copy lands, a frame or two from now; the reply
    // only promises the path.
    ctx.commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    Ok(json!({ "path": path.to_string_lossy().replace('\\', "/"), "note": "written a frame or two later; poll for the file" }))
}

/// `export {path?}`: everything the review commands know, in one JSON file
/// for offline analysis: the full report, the summary map, history, and
/// every building, business, citizen, route, plant and rail facility as
/// `list` rows. Default `exports/city-<unix ms>.json` under the working
/// directory (the repo root under `cargo run`; `/exports` is git-ignored).
pub(super) fn export(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let default = format!("exports/city-{}.json", unique_stamp());
    let path = allowed_path(opt_str(cmd, "path")?.unwrap_or(&default), "json", true)?;
    let w = &ctx.sim.world;
    let mut tables = serde_json::Map::new();
    for what in list::WHATS {
        tables.insert(what.to_string(), json!(list::rows(w, what)?));
    }
    let doc = json!({
        "format": "heifung-city city export v1",
        "exported_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "clock": clock(&ctx.sim),
        "report": report::report(w, &[])?,
        "map_summary": map::map(w, None, "summary").unwrap_or(Value::Null),
        "history": ctx.history.to_json(None, None)?,
        "tables": tables,
    });
    let text = serde_json::to_string(&doc).map_err(|e| e.to_string())?;
    crate::sim::write_atomic(&path, text.as_bytes()).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy().replace('\\', "/"), "bytes": text.len(), "took_ms": ms(start) }))
}

/// What `load` checks before it asks for `confirm`.
pub(super) fn load_replaces(cmd: &Value, _: &Ctx) -> Result<Replaces, String> {
    allowed_path(req_str(cmd, "path")?, "ron", false)?;
    Ok(Replaces::IfUnsaved)
}

/// `load {path, confirm?}`.
pub(super) fn load(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let path = allowed_path(req_str(cmd, "path")?, "ron", false)?;
    let message = load_now(&path, &mut ctx.sim, &mut ctx.flags)?;
    ctx.saves.mark_dirty();
    ctx.saves.title = false;
    ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "city loaded");
    Ok(json!({ "message": message, "clock": clock(&ctx.sim), "paused": ctx.sim.paused }))
}
