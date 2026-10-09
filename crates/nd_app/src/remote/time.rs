//! Time: `pause`, `speed` and `step`.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::status::clock;
use super::view::*;
use crate::sim::SPEEDS;

/// Upper bound for `step`, so a typo cannot freeze the game for minutes.
const MAX_STEP: u64 = 100_000;

pub(super) fn pause(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let paused = opt_bool(cmd, "paused")?.unwrap_or(!ctx.sim.paused);
    ctx.sim.paused = paused;
    Ok(json!({ "paused": paused }))
}

pub(super) fn speed(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let sim = &mut ctx.sim;
    if let Some(i) = opt_u64(cmd, "index")? {
        if i as usize >= SPEEDS.len() {
            return Err(format!("speed index must be 0..{}", SPEEDS.len() - 1));
        }
        sim.speed_index = i as usize;
    } else if let Some(v) = opt_f32(cmd, "value")? {
        // The nearest step on a log scale: 10 picks 16×, 1.5 picks 1×.
        let v = v.max(0.01).ln();
        sim.speed_index = SPEEDS.iter().enumerate().min_by(|a, b| ((a.1).1.ln() - v).abs().total_cmp(&((b.1).1.ln() - v).abs())).map_or(1, |(i, _)| i);
    } else {
        return Err("speed needs {index: 0..5} or {value: multiple of real time}".into());
    }
    Ok(json!({ "speed_index": sim.speed_index, "speed": SPEEDS[sim.speed_index].1, "ticks_per_second": sim.speed(), "label": SPEEDS[sim.speed_index].0 }))
}

pub(super) fn step(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let n = opt_u64(cmd, "ticks")?.unwrap_or(1);
    if n > MAX_STEP {
        return Err(format!("ticks must be at most {MAX_STEP}"));
    }
    let start = std::time::Instant::now();
    // Hour by hour, each chunk ending at minute 59, so history sees
    // every hour and reads the day's counters before midnight
    // resets them, however many days one request covers.
    // [fix/review-app] The committee's round runs for every new day
    // (not once per request), and a meeting it calls ends the step
    // there: the city waits for the meeting, as in game.
    let mut left = n;
    let mut meeting = false;
    while left > 0 {
        let r = ctx.sim.world.clock.tick % 60;
        let chunk = left.min(if r < 59 { 59 - r } else { 60 });
        ctx.sim.step_ticks(chunk as u32);
        ctx.history.observe(&ctx.sim.world);
        left -= chunk;
        if crate::committee::daily(&mut ctx.sim, &mut ctx.committee, &mut ctx.flags) {
            meeting = true;
            break;
        }
    }
    let mut v = json!({ "ticks": n - left, "clock": clock(&ctx.sim), "took_ms": start.elapsed().as_millis() as u64 });
    if meeting {
        v["stopped"] = json!("the committee meets: the city is paused for the meeting (committee_meeting to run it)");
    }
    Ok(v)
}
