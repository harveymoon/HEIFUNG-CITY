//! The review commands: `report`, `map`, `history` and `list` (`export`,
//! which writes them all to a file, is in `files`).

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::status::clock;
use super::view::*;
use super::{list as table, map as heat, report as review};

pub(super) fn report(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let sections = opt_strings(cmd, "sections")?.unwrap_or_default();
    let mut v = review::report(&ctx.sim.world, &sections)?;
    // The sim's own history rings: asked for by name only, as they are long.
    if sections.iter().any(|s| s == "history") {
        let days = opt_u64(cmd, "history_days")?.unwrap_or(30).clamp(1, 365) as usize;
        v["history"] = review::sim_history(&ctx.sim.world, days);
    }
    v["clock"] = clock(&ctx.sim);
    v["took_ms"] = json!(ms(start));
    Ok(v)
}

pub(super) fn map(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let layer = opt_str(cmd, "layer")?.unwrap_or("summary");
    let mut v = heat::map(&ctx.sim.world, opt_f32(cmd, "cell_m")?, layer)?;
    v["took_ms"] = json!(ms(start));
    Ok(v)
}

pub(super) fn history(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let keys = opt_strings(cmd, "keys")?;
    let days = opt_u64(cmd, "days")?.map(|d| d as usize);
    ctx.history.to_json(keys.as_deref(), days)
}

pub(super) fn list(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let args = table::ListArgs {
        what: req_str(cmd, "what")?,
        sort: opt_str(cmd, "sort")?,
        desc: opt_bool(cmd, "desc")?.unwrap_or(false),
        filter: field(cmd, "filter"),
        fields: opt_strings(cmd, "fields")?,
        offset: opt_u64(cmd, "offset")?.unwrap_or(0) as usize,
        limit: (opt_u64(cmd, "limit")?.unwrap_or(table::DEFAULT_LIMIT as u64) as usize).min(table::MAX_LIMIT),
    };
    let mut v = table::list(&ctx.sim.world, &args)?;
    v["took_ms"] = json!(ms(start));
    Ok(v)
}
