//! [r6-perf] Frame times and the sim's share of them, kept for the remote
//! `frame_stats` command: how smooth the game runs (mean, percentiles and
//! the spikes), not just the smoothed FPS the status reports.

use bevy::prelude::*;
use serde_json::{json, Value};

use crate::sim::Sim;

/// Frames kept: half a minute at 60 FPS.
const KEPT: usize = 2048;

/// The last [`KEPT`] frames: (frame ms, sim ms, ticks run).
#[derive(Resource, Default)]
pub struct FrameStats {
    frames: std::collections::VecDeque<(f32, f32, u32)>,
}

impl FrameStats {
    pub fn reset(&mut self) {
        self.frames.clear();
    }

    /// Mean, percentiles and spike counts of the frames kept.
    pub fn json(&self) -> Value {
        let n = self.frames.len();
        if n == 0 {
            return json!({ "frames": 0 });
        }
        let stats = |mut v: Vec<f32>| {
            v.sort_by(f32::total_cmp);
            let pct = |q: f32| v[((v.len() - 1) as f32 * q) as usize];
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            json!({ "mean": round(mean), "p50": round(pct(0.5)), "p95": round(pct(0.95)), "p99": round(pct(0.99)), "max": round(pct(1.0)) })
        };
        let frame: Vec<f32> = self.frames.iter().map(|f| f.0).collect();
        let sim: Vec<f32> = self.frames.iter().map(|f| f.1).collect();
        let over = |ms: f32| frame.iter().filter(|&&f| f > ms).count();
        let total_s: f32 = frame.iter().sum::<f32>() / 1000.0;
        let ticks: u32 = self.frames.iter().map(|f| f.2).sum();
        json!({
            "frames": n,
            "seconds": round(total_s),
            "fps": round(n as f32 / total_s.max(1e-3)),
            "frame_ms": stats(frame.clone()),
            "sim_ms": stats(sim),
            "over_33ms": over(33.3), "over_50ms": over(50.0), "over_100ms": over(100.0),
            "ticks": ticks, "ticks_per_second": round(ticks as f32 / total_s.max(1e-3)),
        })
    }
}

fn round(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

fn record(time: Res<Time<Real>>, sim: Res<Sim>, mut stats: ResMut<FrameStats>) {
    let dt = time.delta_secs() * 1000.0;
    if dt <= 0.0 {
        return;
    }
    if stats.frames.len() >= KEPT {
        stats.frames.pop_front();
    }
    stats.frames.push_back((dt, sim.tick_ms, sim.ticks_last_frame));
}

pub struct FrameStatsPlugin;

impl Plugin for FrameStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameStats>().add_systems(Last, record);
    }
}
