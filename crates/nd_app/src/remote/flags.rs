//! `flags`: the player's "Flag for Claude" bookmarks.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::files::absolute;
use super::inspect::selected_detail;

/// `flags {since?, clear?}`: the player's "Flag for Claude" bookmarks, each
/// with its snapshot and the selection's live detail now.
pub(super) fn flags_cmd(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
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
