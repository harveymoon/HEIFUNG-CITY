//! [r6-perf] Frame times and the sim's share of them, kept for the remote
//! `frame_stats` command: how smooth the game runs (mean, percentiles and
//! the spikes), not just the smoothed FPS the status reports.
//!
//! [r7-sim-thread] A frame's time is the main loop's period, First to First,
//! on the wall clock. Under pipelined rendering the main thread hands each
//! frame to the render thread and waits for the one before, so that is the
//! rate frames reach the screen. `Time<Real>` mixes the render thread's clock
//! (the instant it sends after presenting) with the main thread's when the
//! render has not finished yet, and then reads a steady 15 ms frame as 0 ms
//! followed by 150 ms; r6's numbers were taken while the sim filled the
//! frame (the render always finished first), so they were not affected.

use bevy::prelude::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::sim::Sim;

/// Frames kept: half a minute at 60 FPS.
const KEPT: usize = 2048;

/// A slow frame, for the list `frame_stats` gives with the summary.
#[derive(Clone)]
struct Slow {
    ms: f32,
    /// The main thread's schedules, First to Last.
    main_ms: f32,
    ticks: u32,
    events: usize,
    /// `sim_thread::PHASE` bits seen during the frame.
    phases: u32,
    /// The render thread's slowest prepare and draw meanwhile.
    prep_ms: f32,
    draw_ms: f32,
    /// From the end of the last frame's schedules to the extract (waiting
    /// for the render thread to hand its world back), and to this frame.
    wait_ms: f32,
    gap_ms: f32,
    /// The named systems over 2 ms.
    spans: Vec<(&'static str, f32)>,
}

/// The last [`KEPT`] frames: (frame ms, sim ms, ticks run).
#[derive(Resource, Default)]
pub struct FrameStats {
    frames: std::collections::VecDeque<(f32, f32, u32)>,
    /// [r7-sim-thread] The slow frames (over 50 ms), newest last.
    slow: std::collections::VecDeque<Slow>,
    began: Option<std::time::Instant>,
    /// The frame that just ran, filed when the next begins.
    last: Option<(Slow, f32)>,
}

impl FrameStats {
    pub fn reset(&mut self) {
        self.frames.clear();
        self.slow.clear();
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
            "slow": self.slow.iter().map(|l| {
                let phases: Vec<&str> = [(1, "ticking"), (2, "copying"), (4, "freeing")].into_iter().filter(|(b, _)| l.phases & b != 0).map(|(_, n)| n).collect();
                json!({ "ms": round(l.ms), "main_ms": round(l.main_ms), "ticks": l.ticks, "events": l.events, "sim_thread": phases, "render_prep_ms": round(l.prep_ms), "render_draw_ms": round(l.draw_ms), "wait_ms": round(l.wait_ms), "gap_ms": round(l.gap_ms), "systems": l.spans.iter().map(|(n, ms)| format!("{n} {ms:.0}")).collect::<Vec<_>>() })
            }).collect::<Vec<_>>(),
        })
    }
}

fn round(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

static ENDED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
static WAIT: AtomicU32 = AtomicU32::new(0);
static GAP: AtomicU32 = AtomicU32::new(0);

fn frame_end() {
    if let Ok(mut m) = ENDED.lock() {
        *m = Some(std::time::Instant::now());
    }
}

fn extract_mark() {
    if let Some(t) = ENDED.lock().ok().and_then(|m| *m) {
        put_max(&WAIT, t.elapsed().as_secs_f32() * 1000.0);
    }
}

/// [r7-sim-thread] Time in a few named systems this frame (the costly
/// ones), for the slow-frame list: `let _t = span("name");` at the top.
static SPANS: std::sync::Mutex<Vec<(&'static str, f32)>> = std::sync::Mutex::new(Vec::new());

pub struct Span(&'static str, std::time::Instant);

pub fn span(name: &'static str) -> Span {
    Span(name, std::time::Instant::now())
}

impl Drop for Span {
    fn drop(&mut self) {
        let ms = self.1.elapsed().as_secs_f32() * 1000.0;
        if let Ok(mut v) = SPANS.lock() {
            match v.iter_mut().find(|(n, _)| *n == self.0) {
                Some(e) => e.1 += ms,
                None => v.push((self.0, ms)),
            }
        }
    }
}

/// A frame is filed when the next begins: its time runs First to First.
fn frame_start(mut stats: ResMut<FrameStats>) {
    let now = std::time::Instant::now();
    if let Some(t) = ENDED.lock().ok().and_then(|m| *m) {
        put_max(&GAP, (now - t).as_secs_f32() * 1000.0);
    }
    let dt = stats.began.map_or(0.0, |t| (now - t).as_secs_f32() * 1000.0);
    stats.began = Some(now);
    let Some((mut slow, sim_ms)) = stats.last.take() else { return };
    if dt <= 0.0 {
        return;
    }
    slow.ms = dt;
    slow.gap_ms = take_max(&GAP);
    if dt > 50.0 {
        if stats.slow.len() >= 40 {
            stats.slow.pop_front();
        }
        stats.slow.push_back(slow.clone());
    }
    if stats.frames.len() >= KEPT {
        stats.frames.pop_front();
    }
    stats.frames.push_back((dt, sim_ms, slow.ticks));
}

/// The frame's own figures, at its end.
fn record(sim: Res<Sim>, mut stats: ResMut<FrameStats>) {
    let phases = crate::sim_thread::phases_seen();
    let (prep_ms, draw_ms) = (take_max(&RENDER_PREP), take_max(&RENDER_DRAW));
    let wait_ms = take_max(&WAIT);
    let main_ms = stats.began.map_or(0.0, |t| t.elapsed().as_secs_f32() * 1000.0);
    let mut spans: Vec<(&'static str, f32)> = SPANS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
    spans.retain(|s| s.1 > 2.0);
    spans.sort_by(|a, b| b.1.total_cmp(&a.1));
    stats.last = Some((Slow { spans, ms: 0.0, main_ms, ticks: sim.ticks_last_frame, events: sim.events.len(), phases, prep_ms, draw_ms, wait_ms, gap_ms: 0.0 }, sim.tick_ms));
}

/// [r7-sim-thread] The render thread's slowest prepare (extract commands to
/// the draw) and draw (the `Render` set: encoding and submitting) since the
/// main thread last looked, in ms as f32 bits.
static RENDER_PREP: AtomicU32 = AtomicU32::new(0);
static RENDER_DRAW: AtomicU32 = AtomicU32::new(0);
static RENDER_MARK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn take_max(a: &AtomicU32) -> f32 {
    f32::from_bits(a.swap(0, Ordering::Relaxed))
}

fn put_max(a: &AtomicU32, ms: f32) {
    let _ = a.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| (ms > f32::from_bits(old)).then_some(ms.to_bits()));
}

fn render_begin() {
    if let Ok(mut m) = RENDER_MARK.lock() {
        *m = Some(std::time::Instant::now());
    }
}

fn render_drawing() {
    if let Ok(mut m) = RENDER_MARK.lock() {
        if let Some(t) = *m {
            put_max(&RENDER_PREP, t.elapsed().as_secs_f32() * 1000.0);
        }
        *m = Some(std::time::Instant::now());
    }
}

fn render_done() {
    if let Ok(m) = RENDER_MARK.lock() {
        if let Some(t) = *m {
            put_max(&RENDER_DRAW, t.elapsed().as_secs_f32() * 1000.0);
        }
    }
}

pub struct FrameStatsPlugin;

impl Plugin for FrameStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameStats>().add_systems(First, frame_start).add_systems(Last, (record, frame_end.after(record)));
        use bevy::render::{Render, RenderApp, RenderSystems};
        if let Some(r) = app.get_sub_app_mut(RenderApp) {
            r.add_systems(bevy::render::ExtractSchedule, extract_mark);
            r.add_systems(Render, (render_begin.in_set(RenderSystems::ExtractCommands), render_drawing.after(RenderSystems::PrepareBindGroups).before(RenderSystems::Render), render_done.in_set(RenderSystems::Cleanup)));
        }
    }
}
