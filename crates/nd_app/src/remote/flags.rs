//! `flags` and `flag`: the player's "Flag for Claude" bookmarks; `favourite`.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::files::absolute;
use super::inspect::selected_detail;

/// `flags {since?, clear?}`: the player's "Flag for Claude" bookmarks, each
/// with its snapshot and the selection's live detail now.
pub(super) fn flags(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let since = opt_u64(cmd, "since")?.unwrap_or(0).min(u32::MAX as u64) as u32;
    let rows: Vec<Value> = ctx
        .claude
        .list
        .iter()
        .filter(|f| f.id > since)
        .map(|f| {
            let live = selected_detail(&ctx.sim, f.selection, None, &ctx.assets, &ctx.tools.favorites);
            let now = (!live["detail"].is_null()).then_some(live["detail"].clone());
            crate::flags::flag_json(f, now)
        })
        .collect();
    let total = ctx.claude.list.len();
    if opt_bool(cmd, "clear")?.unwrap_or(false) {
        ctx.claude.list.clear();
    }
    Ok(json!({ "flags": rows, "total": total, "file": absolute(crate::flags::FLAG_FILE) }))
}

/// `flag {note?}`: flag the current selection from the API, as F8 does in game.
pub(super) fn flag(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let note = opt_str(cmd, "note")?.unwrap_or_default().to_string();
    let w = &ctx.sim.world;
    let sel = *ctx.selection;
    let look = ctx.cams.iter().next().map_or([0.0, 0.0], |c| [c.target.x, c.target.z]);
    let snapshot = selected_detail(&ctx.sim, sel, ctx.tools.route_sel, &ctx.assets, &ctx.tools.favorites);
    let brief = selection(w, sel);
    let f = ctx.claude.add(w.clock.tick, sel, brief, note, look, snapshot).clone();
    Ok(json!({ "flag": crate::flags::flag_json(&f, None) }))
}

/// `favourite {citizen, on?}`: star a citizen, as the inspector's star does.
pub(super) fn favourite(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let id = key_from(field(cmd, "citizen").ok_or("missing citizen id")?)?;
    if !ctx.sim.world.citizens.contains_key(id) {
        return Err("no such citizen".into());
    }
    let on = opt_bool(cmd, "on")?.unwrap_or(!ctx.tools.favorites.contains(&id));
    ctx.tools.favorites.retain(|f| *f != id);
    if on {
        ctx.tools.favorites.push(id);
    }
    Ok(json!({ "citizen": key_str(id), "favourite": on, "favourites": ctx.tools.favorites.iter().map(|&f| key_str(f)).collect::<Vec<_>>() }))
}
