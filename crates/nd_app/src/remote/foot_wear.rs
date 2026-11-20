//! [r10-worn-paths] The walking-heat field (`nd_sim::foot_wear`) for the
//! remote API: `foot_wear` reports its totals, the heat and wear at a
//! point, and the most trodden spots.

use nd_sim::foot_wear::{self, CELL, CHUNK_M, HALF_LIFE_HOURS, WEAR_FULL, WEAR_START};
use nd_sim::World;
use serde_json::{json, Value};

use super::view::{opt_u64, opt_vec2, round, v2};

fn spot(w: &World, p: nd_sim::Vec2) -> Value {
    let h = w.foot_wear.at(p);
    json!({
        "pos": v2(p), "heat": round(h), "passes_per_day": round(h / (HALF_LIFE_HOURS / std::f32::consts::LN_2 / 24.0)),
        "grass_wear": round(foot_wear::wear(h)), "paving_wear": round(crate::render::foot_pave_wear(h)),
    })
}

pub fn command(cmd: &Value, w: &World) -> Result<Value, String> {
    let s = w.foot_wear.stats();
    let top = opt_u64(cmd, "top")?.unwrap_or(10).min(200) as usize;
    let at = opt_vec2(cmd, "at")?;
    let mut out = json!({
        "cell_m": CELL, "chunk_m": CHUNK_M, "chunks": s.chunks, "cells": s.cells, "worn_cells": s.worn, "bare_cells": s.bare,
        "max_heat": round(s.max), "total_heat": s.total.round(), "version": w.foot_wear.version,
        "half_life_days": round(HALF_LIFE_HOURS / 24.0),
        "thresholds": { "grass_wear_start": WEAR_START, "grass_bare": WEAR_FULL, "paving_start": crate::render::FOOT_PAVE_START, "paving_full": crate::render::FOOT_PAVE_FULL },
        "hottest": w.foot_wear.hottest(top).into_iter().map(|(_, p)| spot(w, p)).collect::<Vec<_>>(),
        "note": "heat: steps through a 2 m cell, fading by half every 3 days unused (about 4.3 x the passes a day kept up); grass wears from 40 and is bare earth at 600; paving darkens from 200 to 8,000",
    });
    if let Some(p) = at {
        out["at"] = spot(w, p);
    }
    Ok(out)
}
