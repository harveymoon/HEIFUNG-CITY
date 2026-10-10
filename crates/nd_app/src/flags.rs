//! "Flag for Claude": the player bookmarks whatever they have selected,
//! with a note, so an assistant driving the remote API can see exactly
//! what they mean ("this car, stuck here"), not guess from a description.
//!
//! F8 (or the button in the inspector and the person pane) opens a small
//! note box; saving it records the selection, the note, the time and a
//! snapshot of the selection's full detail as it was at that moment (the
//! same JSON as the remote `selected` command), because a stuck car may
//! have driven off by the time anyone reads the flag. The list lives in the
//! running app (`flags` reads it, with each flag's live detail as well) and
//! is appended to `flags/claude_flags.jsonl` under the working directory so
//! it survives a restart for whoever reads the file.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use serde_json::{json, Value};

use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::{Selection, ToolState};
use crate::ui::UiFlags;

/// One bookmark.
#[derive(Clone)]
pub struct Flag {
    pub id: u32,
    pub tick: u64,
    pub unix: u64,
    pub selection: Selection,
    /// Kind, id and label of the selection when flagged.
    pub brief: Value,
    pub note: String,
    /// Where the camera looked when it was made (for a flag with nothing
    /// selected: "look here").
    pub look_at: [f32; 2],
    /// The selection's full detail when flagged.
    pub snapshot: Value,
}

#[derive(Resource, Default)]
pub struct ClaudeFlags {
    pub list: Vec<Flag>,
    next: u32,
    /// The note being typed; `Some` while the note box is open.
    pub draft: Option<String>,
    /// [fix/review-app] Where the camera looked when the note box opened:
    /// the view the player flagged, not wherever it drifted while typing.
    draft_look: Option<[f32; 2]>,
}

/// Where flags are appended, one JSON object per line.
pub const FLAG_FILE: &str = "flags/claude_flags.jsonl";

impl ClaudeFlags {
    /// Record a flag and append it to `FLAG_FILE` (best effort: a read-only
    /// folder only loses the file copy).
    pub fn add(&mut self, tick: u64, selection: Selection, brief: Value, note: String, look_at: [f32; 2], snapshot: Value) -> &Flag {
        self.next += 1;
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let flag = Flag { id: self.next, tick, unix, selection, brief, note, look_at, snapshot };
        let line = flag_json(&flag, None).to_string();
        let path = std::path::Path::new(FLAG_FILE);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
        self.list.push(flag);
        self.list.last().unwrap()
    }
}

/// A flag as JSON; `now` is the selection's live detail, if it still exists.
pub fn flag_json(f: &Flag, now: Option<Value>) -> Value {
    json!({
        "id": f.id, "tick": f.tick, "time": crate::remote::tick_time_json(f.tick), "unix": f.unix, "note": f.note,
        "selection": f.brief, "look_at": f.look_at,
        "snapshot": f.snapshot, "now": now,
    })
}

pub struct FlagsPlugin;

impl Plugin for FlagsPlugin {
    fn build(&self, app: &mut App) {
        // [fix/review-app] After the side panel, which sets `want_flag` and
        // drops keyboard focus.
        app.init_resource::<ClaudeFlags>().add_systems(EguiPrimaryContextPass, flag_box.after(crate::ui::side_panel));
    }
}

/// F8 or a "Flag for Claude" button (`UiFlags::want_flag`) opens the note
/// box; Enter or Save records the flag, Escape or Cancel drops it.
#[allow(clippy::too_many_arguments)]
fn flag_box(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    selection: Res<Selection>,
    tools: Res<ToolState>,
    assets: Res<GameAssets>,
    cams: Query<&crate::camera::OrbitCamera>,
    mut flags: ResMut<ClaudeFlags>,
    mut ui: ResMut<UiFlags>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Plain F8 only: Shift+F8 is the camera's middle-tier jump.
    let shifted = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if (keys.just_pressed(KeyCode::F8) && !shifted && !ui.keyboard_captured) || std::mem::take(&mut ui.want_flag) {
        if flags.draft.is_none() {
            flags.draft = Some(String::new());
            flags.draft_look = cams.iter().next().map(|c| [c.target.x, c.target.z]);
        }
    }
    let Some(mut note) = flags.draft.take() else { return Ok(()) };
    let brief = crate::remote::selection_json_brief(&sim.world, *selection);
    let what = match &brief {
        Value::Null => "the view (nothing selected)".to_string(),
        v => format!("{} {}", v["kind"].as_str().unwrap_or(""), v["label"].as_str().or(v["name"].as_str()).unwrap_or("")),
    };
    let (mut save, mut cancel) = (false, false);
    egui::Window::new(egui::RichText::new("Flag for Claude").color(crate::ui::GOLD).strong())
        .id(egui::Id::new("flag_for_claude"))
        // [pt-ux] Above the City Data view too (the panes sit below it).
        .order(egui::Order::Tooltip)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -80.0))
        .show(ctx, |ui| {
            ui.label(format!("Flagging {what}"));
            ui.label(egui::RichText::new("What should Claude look at? (Enter saves, Esc cancels)").small().weak());
            let r = ui.add(egui::TextEdit::singleline(&mut note).desired_width(320.0).hint_text("e.g. this car has been stuck for ages"));
            r.request_focus();
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                save = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            ui.horizontal(|ui| {
                save |= ui.button("Save flag").clicked();
                cancel |= ui.button("Cancel").clicked();
            });
        });
    if save {
        let look = flags.draft_look.take().or_else(|| cams.iter().next().map(|c| [c.target.x, c.target.z])).unwrap_or([0.0, 0.0]);
        let snapshot = crate::remote::selected_detail(&sim, *selection, tools.route_sel, &assets, &tools.favorites);
        let id = flags.add(sim.world.clock.tick, *selection, brief, note.trim().to_string(), look, snapshot).id;
        ui.message = format!("Flagged for Claude (#{id}): the remote API's `flags` command lists it");
    } else if !cancel {
        flags.draft = Some(note);
    } else {
        flags.draft_look = None;
    }
    Ok(())
}
