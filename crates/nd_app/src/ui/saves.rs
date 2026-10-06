//! Save slots, the load confirmation, and the daily autosave.
//!
//! `saves/city.ron` is always the latest manual save (the path older builds
//! and scripts know); every manual save also lands in a dated slot,
//! `saves/city-<unix seconds>-day<N>.ron`, of which the newest few are kept.
//! The sim autosaves to `saves/autosave.ron` at the start of each day.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::prelude::*;
use bevy_egui::egui;
use nd_sim::SimEvent;

use super::{hint, UiFlags, WARN};
use crate::sim::Sim;
use crate::tools::{EffectivePlan, Selection, ToolState};

const DIR: &str = "saves";
pub const LATEST: &str = "saves/city.ron";
pub const AUTOSAVE: &str = "saves/autosave.ron";
/// Dated slots kept before the oldest is deleted on the next save.
const SLOTS_KEPT: usize = 8;

#[derive(Resource)]
pub struct Saves {
    slots: Vec<Slot>,
    /// The directory listing is stale: rescan before showing it.
    dirty: bool,
    /// A load the player clicked that still needs confirming.
    pending_load: Option<PathBuf>,
    /// The autosave being written on a worker thread: the day and how long
    /// the write took, or why it failed.
    autosave: Option<std::thread::JoinHandle<Result<(u64, f64), String>>>,
}

impl Default for Saves {
    fn default() -> Self {
        Self { slots: Vec::new(), dirty: true, pending_load: None, autosave: None }
    }
}

struct Slot {
    path: PathBuf,
    label: String,
    modified: Option<SystemTime>,
}

impl Saves {
    /// The directory changed behind the panel's back (the remote API saved).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn rescan(&mut self) {
        self.dirty = false;
        self.slots.clear();
        let Ok(dir) = std::fs::read_dir(DIR) else { return };
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "ron") {
                continue;
            }
            let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
            let label = slot_label(&path);
            self.slots.push(Slot { path, label, modified });
        }
        // Newest first; the two fixed names sort by time like the rest.
        self.slots.sort_by(|a, b| b.modified.cmp(&a.modified));
    }

    /// Delete the oldest dated slots beyond `SLOTS_KEPT`. The zero-padded
    /// timestamp makes the file names sort chronologically.
    fn prune(&self) {
        let Ok(dir) = std::fs::read_dir(DIR) else { return };
        let mut dated: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("city-") && n.ends_with(".ron"))).collect();
        dated.sort();
        let excess = dated.len().saturating_sub(SLOTS_KEPT);
        for p in dated.into_iter().take(excess) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// "latest", "autosave", or "Day N" from a dated slot's file name.
fn slot_label(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    match stem {
        "city" => "latest".into(),
        "autosave" => "autosave".into(),
        _ => match stem.rsplit_once("-day") {
            Some((_, day)) => format!("Day {day}"),
            None => stem.to_string(),
        },
    }
}

fn age(modified: Option<SystemTime>) -> String {
    let Some(secs) = modified.and_then(|m| SystemTime::now().duration_since(m).ok()).map(|d| d.as_secs()) else { return String::new() };
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// Write the latest slot and a dated copy of it.
pub fn save_now(sim: &Sim, saves: &mut Saves, flags: &mut UiFlags) {
    saves.dirty = true;
    flags.message = match sim.save(LATEST) {
        Ok(()) => {
            let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let slot = format!("{DIR}/city-{secs:012}-day{}.ron", sim.world.clock.day() + 1);
            match std::fs::copy(LATEST, &slot) {
                Ok(_) => {
                    saves.prune();
                    format!("saved {LATEST} and slot {}", slot_label(Path::new(&slot)))
                }
                Err(e) => format!("saved {LATEST}; slot copy failed: {e}"),
            }
        }
        Err(e) => format!("save failed: {e}"),
    };
}

/// Replace the running city. The tools and the selection refer to ids in
/// the old world, so they go back to defaults; the plan cache forgets its
/// version key; and the city comes up paused so the player can look first.
pub fn load_now(path: &Path, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, plan: &mut EffectivePlan, flags: &mut UiFlags) {
    let shown = path.display().to_string();
    flags.message = match sim.load(&shown) {
        Ok(()) => {
            *tools = ToolState::default();
            *selection = Selection::None;
            plan.invalidate();
            sim.paused = true;
            format!("loaded {shown} (paused)")
        }
        Err(e) => format!("load failed: {e}"),
    };
}

/// The body of the Saves window: save, the slot list, the load confirmation.
pub fn panel(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, plan: &mut EffectivePlan, saves: &mut Saves, flags: &mut UiFlags) {
    if saves.dirty {
        saves.rescan();
    }
    ui.horizontal(|ui| {
        if ui.button("Save").on_hover_text(format!("Writes {LATEST} and a dated slot; the newest {SLOTS_KEPT} slots are kept")).clicked() {
            save_now(sim, saves, flags);
        }
        if ui.small_button("refresh list").clicked() {
            saves.dirty = true;
        }
        hint(ui, &format!("autosave at the start of each day → {AUTOSAVE}"));
    });

    if let Some(pending) = saves.pending_load.clone() {
        // Confirmation: a running city would be thrown away.
        ui.label(egui::RichText::new(format!(
            "Load {}? The running city (day {}, {}) is lost unless saved.",
            slot_label(&pending),
            sim.world.clock.day() + 1,
            sim.world.clock.format_hhmm()
        )).color(WARN));
        ui.horizontal(|ui| {
            if ui.button("Load it").clicked() {
                load_now(&pending, sim, tools, selection, plan, flags);
                saves.pending_load = None;
            }
            if ui.button("Cancel").clicked() {
                saves.pending_load = None;
            }
        });
        return;
    }

    if saves.slots.is_empty() {
        hint(ui, "No saves yet.");
    }
    let mut load: Option<PathBuf> = None;
    for slot in &saves.slots {
        ui.horizontal(|ui| {
            if ui.small_button("Load").clicked() {
                load = Some(slot.path.clone());
            }
            ui.label(egui::RichText::new(&slot.label).small());
            hint(ui, &age(slot.modified));
        });
    }
    if let Some(path) = load {
        // A fresh, untouched city has nothing to lose: load straight away.
        if sim.world.clock.tick > 0 {
            saves.pending_load = Some(path);
        } else {
            load_now(&path, sim, tools, selection, plan, flags);
        }
    }
}

/// Autosave when the sim rolls over to a new day. Runs in `SimSet::Sync`,
/// where `Sim::events` holds this frame's events for every reader.
///
/// The frame pays only for a copy of the world; serialising it to RON (the
/// slow part, hundreds of milliseconds for a big city) and writing it
/// happen on a worker thread, and the write is atomic. One autosave at a
/// time: a day that turns while the last one is still writing (1024x on a
/// slow disk) is skipped, and the next day catches up.
pub fn autosave(sim: Res<Sim>, mut saves: ResMut<Saves>, mut flags: ResMut<UiFlags>) {
    if saves.autosave.as_ref().is_some_and(|j| j.is_finished()) {
        let job = saves.autosave.take().expect("checked above");
        saves.dirty = true;
        flags.message = match job.join() {
            Ok(Ok((day, ms))) => {
                debug!("autosave of day {day} written in {ms:.0} ms off the main thread");
                format!("autosaved day {day} to {AUTOSAVE}")
            }
            Ok(Err(e)) => format!("autosave failed: {e}"),
            Err(_) => "autosave failed: the writer thread panicked".into(),
        };
    }
    if !sim.events.iter().any(|e| matches!(e, SimEvent::NewDay(_))) {
        return;
    }
    if saves.autosave.is_some() {
        warn!("autosave skipped: the previous one is still being written");
        return;
    }
    let start = std::time::Instant::now();
    let world = sim.world.clone();
    let hitch = start.elapsed().as_secs_f64() * 1000.0;
    debug!("autosave: world copied in {hitch:.1} ms on the main thread");
    let day = world.clock.day() + 1;
    let job = std::thread::Builder::new().name("nd-autosave".into()).spawn(move || {
        let t = std::time::Instant::now();
        crate::sim::save_world(&world, Path::new(AUTOSAVE)).map(|()| (day, t.elapsed().as_secs_f64() * 1000.0))
    });
    match job {
        Ok(j) => saves.autosave = Some(j),
        Err(e) => flags.message = format!("autosave failed: {e}"),
    }
}
