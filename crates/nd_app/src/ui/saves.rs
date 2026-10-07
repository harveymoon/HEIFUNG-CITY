//! Saved games: named saves with a map picture, the quick save slots, the
//! load confirmation, and the daily autosave.
//!
//! - **Named games**: "Save as" writes `saves/<name>.ron` (the name made
//!   file-safe by [`file_stem`]), a sidecar `saves/<name>.meta.json` with the
//!   name as typed, the day, the population and the treasury, and a top-down
//!   picture of the city, `saves/<name>.png`, rendered offscreen a moment
//!   later (`render/thumbs.rs`).
//! - **Quick save**: `saves/city.ron` is always the latest (the path older
//!   builds and scripts know); every quick save also lands in a dated slot,
//!   `saves/city-<unix seconds>-day<N>.ron`, of which the newest few are
//!   kept. It gets a picture and a sidecar too.
//! - **Autosave**: `saves/autosave.ron` at the start of each day, and the
//!   same into the game's history (`ui/history.rs`).
//!
//! [identity] Every world carries its city's name and a game id
//! (`nd_sim::CityIdentity`), and every sidecar repeats them. A new game
//! gets an id at once ([`new_game_id`]); a save from before ids gets one
//! when it is next saved ([`ensure_game_id`]) and is "Unnamed city" until
//! named.
//!
//! The title screen (`ui/title.rs`) and the Saves window list them newest
//! first, grouped by city name ([`group_by_city`]). Saves from before
//! sidecars show their file name and no figures.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::prelude::*;
use bevy_egui::egui;
use nd_sim::SimEvent;
use serde::{Deserialize, Serialize};

use super::{hint, section, UiFlags, GOLD, IVORY, WARN};
use crate::sim::Sim;
use crate::tools::{EffectivePlan, Selection, ToolState};

pub const DIR: &str = "saves";
pub const LATEST: &str = "saves/city.ron";
pub const AUTOSAVE: &str = "saves/autosave.ron";
/// Dated slots kept before the oldest is deleted on the next save.
const SLOTS_KEPT: usize = 8;

/// What is known about a save without reading the city: written beside it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SaveMeta {
    /// The name as the player typed it ("" for quick saves and autosaves).
    pub name: String,
    /// Day of the city (1-based, as the top bar shows it).
    pub day: u64,
    pub population: usize,
    pub treasury: f32,
    #[serde(default)]
    pub buildings: usize,
    #[serde(default)]
    pub landscape: String,
    pub saved_unix: u64,
    /// [identity] The city's name ("" for none: "Unnamed city").
    #[serde(default)]
    pub city: String,
    /// [identity] The game the save belongs to ("" before ids).
    #[serde(default)]
    pub game_id: String,
    /// [identity] A history entry's name for the moment, if marked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
}

impl SaveMeta {
    /// The city's name to show and group by.
    pub fn city_name(&self) -> &str {
        if self.city.trim().is_empty() {
            nd_sim::identity::UNNAMED
        } else {
            &self.city
        }
    }
}

/// What kind of save a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Named,
    Latest,
    Autosave,
    /// A dated backup of a quick save.
    Dated,
}

impl SlotKind {
    pub fn key(self) -> &'static str {
        match self {
            SlotKind::Named => "named",
            SlotKind::Latest => "latest",
            SlotKind::Autosave => "autosave",
            SlotKind::Dated => "dated",
        }
    }
}

pub struct Slot {
    pub path: PathBuf,
    pub label: String,
    pub kind: SlotKind,
    pub modified: Option<SystemTime>,
    pub meta: Option<SaveMeta>,
    /// `saves/<stem>.png` (it may not have been written, or ever be: saves
    /// from before pictures).
    pub picture: Option<PathBuf>,
}

impl Slot {
    /// The city the save is of: its sidecar's, or "Unnamed city".
    pub fn city(&self) -> &str {
        self.meta.as_ref().map_or(nd_sim::identity::UNNAMED, |m| m.city_name())
    }

    /// "Day 12 · 340 people · $ 52000" from the sidecar, or "" for an old save.
    pub fn figures(&self) -> String {
        self.meta.as_ref().map_or_else(String::new, |m| format!("Day {} · {} people · $ {:.0}", m.day, m.population, m.treasury))
    }
}

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
    /// Pictures to render (PNG paths); `render/thumbs.rs` takes them.
    shots: Vec<PathBuf>,
    /// Pictures loaded as egui textures, by path, with the file time they
    /// were read at.
    pictures: HashMap<PathBuf, (Option<SystemTime>, egui::TextureHandle)>,
    /// The name typed into "Save as".
    pub name: String,
    /// The title screen is up (at startup unless `ND_TITLE=0`).
    pub title: bool,
    /// [identity] Open (true) or close the Game history window on the next
    /// UI pass (the remote API's `window {name: "history"}`).
    pub history_requested: Option<bool>,
    /// [identity] The name typed on the title screen for "Play this map".
    pub new_city_name: String,
}

impl Default for Saves {
    fn default() -> Self {
        let title = !std::env::var("ND_TITLE").is_ok_and(|v| v == "0");
        Self { slots: Vec::new(), dirty: true, pending_load: None, autosave: None, shots: Vec::new(), pictures: HashMap::new(), name: String::new(), title, history_requested: None, new_city_name: String::new() }
    }
}

impl Saves {
    /// The directory changed behind the panel's back (the remote API saved).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// The pictures asked for since the last call.
    pub fn take_shots(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.shots)
    }

    /// Every save, newest first (rescanning if stale).
    pub fn slots(&mut self) -> &[Slot] {
        if self.dirty {
            self.dirty = false;
            self.slots = scan();
        }
        &self.slots
    }

    /// The picture of a save as an egui texture, loaded once per file time.
    pub fn picture(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let mtime = Some(mtime);
        if let Some((t, tex)) = self.pictures.get(path) {
            if *t == mtime {
                return Some(tex.clone());
            }
        }
        let image = load_png(path)?;
        let tex = ctx.load_texture(format!("save_picture:{}", path.display()), image, egui::TextureOptions::LINEAR);
        self.pictures.insert(path.to_path_buf(), (mtime, tex.clone()));
        Some(tex)
    }

    /// Delete the oldest dated slots beyond `SLOTS_KEPT`. The zero-padded
    /// timestamp makes the file names sort chronologically.
    fn prune(&self) {
        let Ok(dir) = std::fs::read_dir(DIR) else { return };
        let mut dated: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("city-") && n.ends_with(".ron"))).collect();
        dated.sort();
        let excess = dated.len().saturating_sub(SLOTS_KEPT);
        for p in dated.into_iter().take(excess) {
            let _ = std::fs::remove_file(&p);
            let _ = std::fs::remove_file(meta_path(&p));
        }
    }
}

/// Every save in `saves/`, newest first.
pub fn scan() -> Vec<Slot> {
    let mut slots = Vec::new();
    let Ok(dir) = std::fs::read_dir(DIR) else { return slots };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "ron") {
            continue;
        }
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        let meta = read_meta(&path);
        let kind = slot_kind(&path);
        let label = match (&meta, kind) {
            (Some(m), SlotKind::Named) if !m.name.is_empty() => m.name.clone(),
            _ => slot_label(&path),
        };
        // Kept even before the file exists: the picture lands a moment
        // after the save, and `Saves::picture` picks it up when it does.
        let picture = Some(path.with_extension("png"));
        slots.push(Slot { path, label, kind, modified, meta, picture });
    }
    // Newest first; the fixed names sort by time like the rest.
    slots.sort_by(|a, b| b.modified.cmp(&a.modified));
    slots
}

fn slot_kind(path: &Path) -> SlotKind {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    match stem {
        "city" => SlotKind::Latest,
        "autosave" => SlotKind::Autosave,
        _ if stem.starts_with("city-") && stem.contains("-day") => SlotKind::Dated,
        _ => SlotKind::Named,
    }
}

/// "Latest quick save", "Autosave", "Quick save, day N" or the stem.
fn slot_label(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    match slot_kind(path) {
        SlotKind::Latest => "Latest quick save".into(),
        SlotKind::Autosave => "Autosave".into(),
        SlotKind::Dated => match stem.rsplit_once("-day") {
            Some((_, day)) => format!("Quick save, day {day}"),
            None => stem.to_string(),
        },
        SlotKind::Named => stem.replace('_', " "),
    }
}

/// The sidecar of a save: `<stem>.meta.json` beside it.
pub fn meta_path(save: &Path) -> PathBuf {
    save.with_extension("meta.json")
}

fn read_meta(save: &Path) -> Option<SaveMeta> {
    let text = std::fs::read_to_string(meta_path(save)).ok()?;
    serde_json::from_str(&text).ok()
}

/// A file-safe stem for a typed name: ASCII letters, digits and `-` kept,
/// anything else made `_`, runs of `_` folded, ends trimmed, at most 48
/// characters; names Windows reserves get a `_` after them. Empty input
/// gives "city".
pub fn file_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' };
        if c == '_' && (out.ends_with('_') || out.is_empty()) {
            continue;
        }
        out.push(c);
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_matches(|c| c == '_' || c == '-').to_string();
    if out.is_empty() {
        return "city".into();
    }
    let reserved = ["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];
    if reserved.contains(&out.to_ascii_lowercase().as_str()) {
        return format!("{out}_");
    }
    out
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// [identity] A new game id: the time in milliseconds (hex) and 64 bits
/// from a randomly keyed hasher over the time in nanoseconds, the process
/// and a counter. File-safe (hex and one `-`), unique across games and
/// processes without a random crate: `RandomState` is keyed from the OS.
pub fn new_game_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(nanos);
    h.write_u32(std::process::id());
    h.write_u64(SEQ.fetch_add(1, Ordering::Relaxed));
    format!("{:x}-{:016x}", nanos / 1_000_000, h.finish())
}

/// [identity] Give the world a game id if it has none (a save from before
/// ids, loaded): done when it is next saved, so its saves from then on
/// group under one game. True if one was made.
pub fn ensure_game_id(world: &mut nd_sim::World) -> bool {
    if world.identity.has_id() {
        return false;
    }
    world.identity.game_id = new_game_id();
    true
}

/// [identity] Name the city (tidied; "" leaves it unnamed). Returns the
/// name kept.
pub fn rename_city(world: &mut nd_sim::World, name: &str) -> String {
    world.identity.name = nd_sim::identity::clean_name(name);
    world.identity.name.clone()
}

/// The sidecar for the city as it stands.
fn meta_of(sim: &Sim, name: &str) -> SaveMeta {
    let w = &sim.world;
    SaveMeta {
        name: name.to_string(),
        day: w.clock.day() + 1,
        population: w.citizens.len(),
        treasury: w.economy.treasury,
        buildings: w.buildings.len(),
        landscape: w.landscape.params.preset.label().to_string(),
        saved_unix: now_unix(),
        city: w.identity.name.clone(),
        game_id: w.identity.game_id.clone(),
        bookmark: None,
    }
}

/// Write `path` (a .ron save), its sidecar, and ask for its picture.
fn write_save(sim: &mut Sim, saves: &mut Saves, path: &Path, name: &str) -> Result<(), String> {
    ensure_game_id(&mut sim.world);
    let shown = path.to_string_lossy().replace('\\', "/");
    sim.save(&shown)?;
    if let Ok(text) = serde_json::to_string_pretty(&meta_of(sim, name)) {
        let _ = std::fs::write(meta_path(path), text);
    }
    saves.shots.push(path.with_extension("png"));
    saves.dirty = true;
    Ok(())
}

/// Save the city under a name: `saves/<file_stem(name)>.ron`, its sidecar
/// and picture. Returns the save's path.
pub fn save_as(sim: &mut Sim, saves: &mut Saves, flags: &mut UiFlags, name: &str) -> Result<PathBuf, String> {
    let name = if name.trim().is_empty() { "City".to_string() } else { name.trim().to_string() };
    let path = PathBuf::from(DIR).join(format!("{}.ron", file_stem(&name)));
    match write_save(sim, saves, &path, &name) {
        Ok(()) => {
            flags.message = format!("saved \"{name}\" ({})", path.display());
            Ok(path)
        }
        Err(e) => {
            flags.message = format!("save failed: {e}");
            Err(e)
        }
    }
}

/// Write the latest slot and a dated copy of it.
pub fn save_now(sim: &mut Sim, saves: &mut Saves, flags: &mut UiFlags) {
    saves.dirty = true;
    flags.message = match write_save(sim, saves, Path::new(LATEST), "") {
        Ok(()) => {
            let slot = format!("{DIR}/city-{:012}-day{}.ron", now_unix(), sim.world.clock.day() + 1);
            match std::fs::copy(LATEST, &slot) {
                Ok(_) => {
                    let _ = std::fs::copy(meta_path(Path::new(LATEST)), meta_path(Path::new(&slot)));
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

/// [identity] Save the city now into its game history with a name for the
/// moment ("Mark this moment"). Returns the entry's path.
pub fn mark_now(sim: &mut Sim, saves: &mut Saves, label: &str) -> Result<PathBuf, String> {
    ensure_game_id(&mut sim.world);
    let dir = Path::new(DIR);
    let w = &sim.world;
    let path = super::history::entry_path(dir, &w.identity.game_id, w.clock.day() + 1, now_unix_ms());
    sim.save(&path)?;
    let mut meta = meta_of(sim, "");
    meta.bookmark = Some(nd_sim::identity::clean_name(label)).filter(|l| !l.is_empty());
    if let Ok(text) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(meta_path(&path), text);
    }
    saves.shots.push(path.with_extension("png"));
    saves.dirty = true;
    Ok(path)
}

/// [identity] Go back to a moment of the game's history: load it as Load
/// does (the city comes up paused). The caller has asked first.
pub fn revert(path: &Path, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, plan: &mut EffectivePlan, saves: &mut Saves, flags: &mut UiFlags) {
    let day = super::history::list(Path::new(DIR), &sim.world.identity.game_id).into_iter().find(|e| e.path == path).map(|e| e.meta.day);
    load_now(path, sim, tools, selection, plan, flags);
    saves.dirty = true;
    if !flags.message.starts_with("load failed") {
        flags.message = match day {
            Some(d) => format!("reverted to day {d} (paused)"),
            None => format!("reverted to {} (paused)", path.display()),
        };
    }
}

/// [identity] Slots grouped by city name: (city, indices into `slots`),
/// the group with the newest save first and each group newest first (the
/// order `slots` is in). "Unnamed city" collects saves without a name.
pub fn group_by_city(slots: &[Slot]) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        let city = s.city();
        match groups.iter_mut().find(|(c, _)| c == city) {
            Some((_, v)) => v.push(i),
            None => groups.push((city.to_string(), vec![i])),
        }
    }
    groups
}

/// [identity] The city's name with a Rename control: shown at the top of
/// the City and Saves windows.
pub fn city_name_row(ui: &mut egui::Ui, sim: &mut Sim, flags: &mut UiFlags) {
    let id = egui::Id::new("city_rename");
    let mut editing: Option<String> = ui.ctx().data(|d| d.get_temp(id));
    ui.horizontal(|ui| match editing.as_mut() {
        Some(text) => {
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(180.0).hint_text("name your city"));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.small_button("OK").clicked() || enter {
                let kept = rename_city(&mut sim.world, text);
                flags.message = format!("the city is now called {}", if kept.is_empty() { nd_sim::identity::UNNAMED } else { &kept });
                editing = None;
            } else if ui.small_button("Cancel").clicked() {
                editing = None;
            }
        }
        None => {
            ui.label(egui::RichText::new(sim.world.identity.display_name()).color(GOLD).strong().size(15.0));
            if ui.small_button("Rename").on_hover_text("Change the city's name; saves from now on carry it").clicked() {
                editing = Some(sim.world.identity.name.clone());
            }
        }
    });
    ui.ctx().data_mut(|d| match &editing {
        Some(t) => {
            d.insert_temp(id, t.clone());
        }
        None => {
            d.remove::<String>(id);
        }
    });
}

/// A PNG as an egui image.
fn load_png(path: &Path) -> Option<egui::ColorImage> {
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    let bytes = std::fs::read(path).ok()?;
    let img = Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::Default, bevy::asset::RenderAssetUsages::MAIN_WORLD).ok()?;
    let rgba = img.try_into_dynamic().ok()?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// How long ago, in words.
pub fn age(modified: Option<SystemTime>) -> String {
    let Some(secs) = modified.and_then(|m| SystemTime::now().duration_since(m).ok()).map(|d| d.as_secs()) else { return String::new() };
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// The body of the Saves window: save as a name, quick save, the list with
/// pictures, the load confirmation.
pub fn panel(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, plan: &mut EffectivePlan, saves: &mut Saves, flags: &mut UiFlags) {
    section(ui, "CITY");
    city_name_row(ui, sim, flags);
    if ui.button("Game history…").on_hover_text("This game's autosaves: mark key moments, revert to an earlier day").clicked() {
        saves.history_requested = Some(true);
    }
    section(ui, "SAVE AS");
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut saves.name).desired_width(170.0).hint_text("name your city"));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(egui::RichText::new("Save").strong()).on_hover_text("Writes saves/<name>.ron with a map picture; the title screen lists it").clicked() || enter) && !saves.name.trim().is_empty() {
            let name = saves.name.clone();
            let _ = save_as(sim, saves, flags, &name);
        }
    });
    if !saves.name.trim().is_empty() {
        let stem = file_stem(&saves.name);
        let exists = Path::new(DIR).join(format!("{stem}.ron")).exists();
        hint(ui, &format!("→ saves/{stem}.ron{}", if exists { " (replaces the save of that name)" } else { "" }));
    }
    ui.horizontal(|ui| {
        if ui.button("Quick save").on_hover_text(format!("Writes {LATEST} and a dated slot; the newest {SLOTS_KEPT} slots are kept")).clicked() {
            save_now(sim, saves, flags);
        }
        if ui.small_button("refresh list").clicked() {
            saves.dirty = true;
        }
        if ui.small_button("title screen").on_hover_text("Back to the list of saved games").clicked() {
            saves.title = true;
        }
    });
    hint(ui, &format!("autosave at the start of each day → {AUTOSAVE}"));

    if let Some(pending) = saves.pending_load.clone() {
        // Confirmation: a running city would be thrown away.
        let label = saves.slots().iter().find(|s| s.path == pending).map_or_else(|| slot_label(&pending), |s| s.label.clone());
        ui.label(egui::RichText::new(format!("Load {label}? The running city (day {}, {}) is lost unless saved.", sim.world.clock.day() + 1, sim.world.clock.format_hhmm())).color(WARN));
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

    section(ui, "SAVED GAMES");
    if saves.slots().is_empty() {
        hint(ui, "No saves yet.");
    }
    let mut load: Option<PathBuf> = None;
    let groups = group_by_city(saves.slots());
    let rows: Vec<(PathBuf, String, SlotKind, String, Option<SystemTime>, Option<PathBuf>)> = saves.slots().iter().map(|s| (s.path.clone(), s.label.clone(), s.kind, s.figures(), s.modified, s.picture.clone())).collect();
    let ordered: Vec<(Option<(String, usize)>, usize)> = groups.iter().flat_map(|(city, members)| members.iter().enumerate().map(move |(k, &i)| ((k == 0).then(|| (city.clone(), members.len())), i))).collect();
    for (header, i) in ordered {
        let (path, label, kind, figures, modified, picture) = rows[i].clone();
        if let Some((city, n)) = header {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("{city}  ({n})")).color(GOLD).small().strong());
        }
        ui.horizontal(|ui| {
            let pic = picture.as_deref().and_then(|p| saves.picture(ui.ctx(), p));
            match pic {
                Some(tex) => {
                    ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(44.0, 44.0)).corner_radius(3.0));
                }
                None => {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 3.0, egui::Color32::from_rgb(20, 22, 34));
                }
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("Load").clicked() {
                        load = Some(path.clone());
                    }
                    let text = egui::RichText::new(&label).color(if kind == SlotKind::Named { GOLD } else { IVORY });
                    ui.label(if kind == SlotKind::Named { text.strong() } else { text.small() });
                });
                hint(ui, &format!("{figures}{}{}", if figures.is_empty() { "" } else { " · " }, age(modified)));
            });
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
pub fn autosave(mut sim: ResMut<Sim>, mut saves: ResMut<Saves>, mut flags: ResMut<UiFlags>) {
    if saves.autosave.as_ref().is_some_and(|j| j.is_finished()) {
        let job = saves.autosave.take().expect("checked above");
        saves.dirty = true;
        flags.message = match job.join() {
            Ok(Ok((day, ms))) => {
                debug!("autosave of day {day} written in {ms:.0} ms off the main thread");
                // [identity] The game's history keeps its newest autosaves
                // and every marked one.
                let pruned = super::history::prune(Path::new(DIR), &sim.world.identity.game_id, super::history::KEPT);
                if pruned > 0 {
                    debug!("game history: {pruned} old autosave(s) pruned");
                }
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
    ensure_game_id(&mut sim.world);
    let start = std::time::Instant::now();
    let world = sim.world.clone();
    let hitch = start.elapsed().as_secs_f64() * 1000.0;
    debug!("autosave: world copied in {hitch:.1} ms on the main thread");
    let day = world.clock.day() + 1;
    // [identity] The same autosave into the game's history: serialised
    // once, written twice.
    let entry = super::history::entry_path(Path::new(DIR), &world.identity.game_id, day, now_unix_ms());
    let entry_bg = entry.clone();
    let job = std::thread::Builder::new().name("nd-autosave".into()).spawn(move || {
        let t = std::time::Instant::now();
        let text = world.to_ron().map_err(|e| e.to_string())?;
        crate::sim::write_atomic(&entry_bg, text.as_bytes()).map_err(|e| e.to_string())?;
        crate::sim::write_atomic(Path::new(AUTOSAVE), text.as_bytes()).map_err(|e| e.to_string())?;
        Ok((day, t.elapsed().as_secs_f64() * 1000.0))
    });
    match job {
        Ok(j) => saves.autosave = Some(j),
        Err(e) => flags.message = format!("autosave failed: {e}"),
    }
    // [saves] The sidecar (name, day, population) and the map picture,
    // as every save gets; the picture is rendered a frame or two later.
    if let Ok(text) = serde_json::to_string_pretty(&meta_of(&sim, "")) {
        let _ = std::fs::write(meta_path(Path::new(AUTOSAVE)), &text);
        if let Some(parent) = entry.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(meta_path(&entry), text);
    }
    saves.shots.push(Path::new(AUTOSAVE).with_extension("png"));
    saves.shots.push(entry.with_extension("png"));
    saves.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_safe_file_stems() {
        assert_eq!(file_stem("Neon Bay"), "Neon_Bay");
        assert_eq!(file_stem("  ../../etc/passwd "), "etc_passwd");
        assert_eq!(file_stem("a:b*c?d\"e<f>g|h"), "a_b_c_d_e_f_g_h");
        assert_eq!(file_stem(""), "city");
        assert_eq!(file_stem("   "), "city");
        assert_eq!(file_stem("CON"), "CON_");
        assert!(file_stem(&"x".repeat(200)).len() <= 48);
    }

    /// [identity] Ids are unique, file-safe and not all alike in shape.
    #[test]
    fn game_ids_are_unique_and_file_safe() {
        let ids: std::collections::HashSet<String> = (0..5000).map(|_| new_game_id()).collect();
        assert_eq!(ids.len(), 5000);
        for id in ids.iter().take(50) {
            assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{id}");
            assert_eq!(file_stem(id), *id, "an id is its own file stem");
            assert!(id.len() >= 20);
        }
    }

    /// The RON of a world as a build from before identities wrote it.
    fn legacy_ron(w: &nd_sim::World) -> String {
        let ron = w.to_ron().unwrap();
        let start = ron.rfind("identity: (").expect("identity is saved");
        // To the matching close paren (names and ids hold none), and its comma.
        let mut depth = 0;
        let mut end = start;
        for (k, c) in ron[start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + k + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if ron[end..].starts_with(',') {
            end += 1;
        }
        let legacy = format!("{}{}", &ron[..start], &ron[end..]);
        assert!(!legacy.contains("game_id"));
        legacy
    }

    /// [identity] A save from before identities loads as "Unnamed city"
    /// without an id, gets one when next saved, and keeps it (and a name
    /// given since) through a save and load.
    #[test]
    fn legacy_saves_load_unnamed_and_get_an_id_when_saved() {
        let sim = Sim::new();
        assert!(sim.world.identity.has_id(), "a new game has an id at once");
        let mut w = nd_sim::World::from_ron(&legacy_ron(&sim.world)).expect("a legacy save loads");
        assert_eq!(w.identity.display_name(), nd_sim::identity::UNNAMED);
        assert!(!w.identity.has_id());
        // Its sidecar from before identities reads as unnamed too.
        let old_meta: SaveMeta = serde_json::from_str(r#"{"name":"Old","day":3,"population":10,"treasury":5.0,"saved_unix":1}"#).unwrap();
        assert_eq!(old_meta.city_name(), nd_sim::identity::UNNAMED);
        assert!(old_meta.game_id.is_empty() && old_meta.bookmark.is_none());
        assert!(ensure_game_id(&mut w));
        let id = w.identity.game_id.clone();
        assert!(!ensure_game_id(&mut w), "an id is made once");
        assert_eq!(rename_city(&mut w, "  Cobalt   Reach "), "Cobalt Reach");
        let back = nd_sim::World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.identity.game_id, id);
        assert_eq!(back.identity.display_name(), "Cobalt Reach");
    }

    /// [identity] Grouping by city name: newest group first, unnamed saves
    /// together.
    #[test]
    fn saves_group_by_city() {
        let slot = |city: Option<&str>, n: &str| Slot {
            path: PathBuf::from(format!("saves/{n}.ron")),
            label: n.into(),
            kind: SlotKind::Named,
            modified: None,
            meta: city.map(|c| SaveMeta { city: c.into(), ..Default::default() }),
            picture: None,
        };
        let slots = vec![slot(Some("Neon Bay"), "a"), slot(None, "b"), slot(Some("Iron Vale"), "c"), slot(Some("Neon Bay"), "d"), slot(Some(""), "e")];
        let g = group_by_city(&slots);
        let names: Vec<&str> = g.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(names, vec!["Neon Bay", nd_sim::identity::UNNAMED, "Iron Vale"]);
        assert_eq!(g[0].1, vec![0, 3]);
        assert_eq!(g[1].1, vec![1, 4]);
    }

    /// [identity] Revert puts back the moment saved, with its game id, and
    /// leaves the history as it was (the newer entries stay).
    #[test]
    fn revert_restores_an_earlier_autosave() {
        let dir = std::env::temp_dir().join(format!("nd_revert_test_{}_{}", std::process::id(), new_game_id()));
        let mut sim = Sim::new();
        sim.set_city_name("Revert Town");
        let game = sim.world.identity.game_id.clone();
        sim.step_ticks(30);
        let early_tick = sim.world.clock.tick;
        let early = super::super::history::entry_path(&dir, &game, sim.world.clock.day() + 1, 1_000);
        sim.save(&early).unwrap();
        sim.step_ticks(90);
        let late = super::super::history::entry_path(&dir, &game, sim.world.clock.day() + 1, 2_000);
        sim.save(&late).unwrap();
        sim.set_city_name("Renamed later");
        assert_ne!(sim.world.clock.tick, early_tick);
        let (mut tools, mut selection, mut plan, mut saves, mut flags) = (ToolState::default(), Selection::None, EffectivePlan::default(), Saves::default(), UiFlags::default());
        let generation = sim.generation;
        revert(&early, &mut sim, &mut tools, &mut selection, &mut plan, &mut saves, &mut flags);
        assert!(flags.message.starts_with("reverted"), "{}", flags.message);
        assert_eq!(sim.world.clock.tick, early_tick);
        assert_eq!(sim.world.identity.game_id, game, "the same game");
        assert_eq!(sim.world.identity.name, "Revert Town", "as it was then");
        assert!(sim.paused);
        assert!(sim.generation > generation, "renderers rebuild");
        assert_eq!(super::super::history::list(&dir, &game).len(), 2, "the later entry stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kinds_by_file_name() {
        assert_eq!(slot_kind(Path::new("saves/city.ron")), SlotKind::Latest);
        assert_eq!(slot_kind(Path::new("saves/autosave.ron")), SlotKind::Autosave);
        assert_eq!(slot_kind(Path::new("saves/city-000001700000-day3.ron")), SlotKind::Dated);
        assert_eq!(slot_kind(Path::new("saves/Neon_Bay.ron")), SlotKind::Named);
        assert_eq!(meta_path(Path::new("saves/Neon_Bay.ron")), PathBuf::from("saves/Neon_Bay.meta.json"));
    }
}
