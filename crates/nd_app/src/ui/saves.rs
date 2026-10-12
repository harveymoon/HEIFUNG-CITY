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
//! when it is next saved ([`ensure_game_id`]). An old named save takes its
//! save name as the city's on load ([`legacy_city_name`]); other old saves
//! are "Unnamed city" until named.
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
    /// The city the save is of: its sidecar's; for a named save from
    /// before city names the name it was saved under (as it loads, see
    /// [`legacy_city_name`]); otherwise "Unnamed city".
    pub fn city(&self) -> &str {
        match &self.meta {
            Some(m) if !m.city.trim().is_empty() => &m.city,
            _ if self.kind == SlotKind::Named && !self.label.trim().is_empty() => &self.label,
            _ => nd_sim::identity::UNNAMED,
        }
    }

    /// "Day 12 · 340 people · $ 52000" from the sidecar, or "" for an old save.
    pub fn figures(&self) -> String {
        self.meta.as_ref().map_or_else(String::new, |m| format!("Day {} · {} people · $ {:.0}", m.day, m.population, m.treasury))
    }
}

/// [identity] A game's history entries, newest first, each with its file time.
pub type HistoryList = std::sync::Arc<Vec<(super::history::Entry, Option<SystemTime>)>>;

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
    /// [saves] The quick save being written on a worker thread: the
    /// message to show when it is done.
    quick: Option<std::thread::JoinHandle<Result<String, String>>>,
    /// Pictures to render (PNG paths); `render/thumbs.rs` takes them.
    shots: Vec<PathBuf>,
    /// [saves] Pictures that are copies of another being rendered (the
    /// autosave's is its history entry's): (rendered, copy), copied when the
    /// rendered one is written, so the city is filmed once, not twice.
    picture_copies: Vec<(PathBuf, PathBuf)>,
    /// Pictures loaded as egui textures, by path, with the file time they
    /// were read at.
    pictures: HashMap<PathBuf, (Option<SystemTime>, egui::TextureHandle)>,
    /// When each picture's file was last looked at, as a [`disk_generation`],
    /// and whether it had one: the lists ask every frame, the disk is asked
    /// again only after the game wrote to the folder.
    pictures_checked: HashMap<PathBuf, (u64, bool)>,
    /// Pictures asked for and not yet seen on disk newer than the asking
    /// (the renderer writes them a few frames later): looked at every frame
    /// until they land.
    pictures_due: HashMap<PathBuf, SystemTime>,
    /// [identity] The game history listing (`history::list`) with each
    /// entry's file time, for the game id and [`disk_generation`] it was
    /// read at; the Game history window shows it every frame.
    history: Option<(u64, String, HistoryList)>,
    /// [title] Every game with its saves and history ([`games`]), as of
    /// the [`disk_generation`] it was read at.
    games: Option<(u64, std::sync::Arc<Vec<Game>>)>,
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
        Self { slots: Vec::new(), dirty: true, pending_load: None, autosave: None, quick: None, shots: Vec::new(), picture_copies: Vec::new(), pictures: HashMap::new(), pictures_checked: HashMap::new(), pictures_due: HashMap::new(), history: None, games: None, name: String::new(), title, history_requested: None, new_city_name: String::new() }
    }
}

impl Saves {
    /// The directory changed (a save, a picture, the remote API): rescan
    /// before the lists show it again.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        disk_changed();
    }

    /// Ask for a save's picture: rendered offscreen and written a few frames
    /// later (`render/thumbs.rs`).
    fn want_picture(&mut self, png: PathBuf) {
        self.pictures_due.insert(png.clone(), SystemTime::now());
        self.shots.push(png);
    }

    /// [identity] Every entry of a game's history, newest first, with the
    /// entry's file time: read from disk once per [`disk_generation`] (and
    /// after [`Self::forget_history`]), not every frame.
    pub fn history(&mut self, game_id: &str) -> HistoryList {
        let gen = disk_generation();
        if !self.history.as_ref().is_some_and(|(g, id, _)| *g == gen && id == game_id) {
            let entries = super::history::list(Path::new(DIR), game_id).into_iter().map(|e| {
                let modified = std::fs::metadata(&e.path).and_then(|m| m.modified()).ok();
                (e, modified)
            });
            self.history = Some((gen, game_id.to_string(), std::sync::Arc::new(entries.collect())));
        }
        self.history.as_ref().map_or_else(Default::default, |(_, _, v)| v.clone())
    }

    /// [title] Every game, newest first, with its save states (the saves
    /// folder and every game's history): read once per [`disk_generation`].
    pub fn games(&mut self) -> std::sync::Arc<Vec<Game>> {
        let gen = disk_generation();
        if self.dirty || !self.games.as_ref().is_some_and(|(g, _)| *g == gen) {
            let history = scan_history(Path::new(DIR));
            let games = games(self.slots(), &history);
            self.games = Some((gen, std::sync::Arc::new(games)));
        }
        self.games.as_ref().map_or_else(Default::default, |(_, v)| v.clone())
    }

    /// [identity] Drop the cached history listing: the next look reads the
    /// folder again (the window does this while closed, so it opens fresh).
    pub fn forget_history(&mut self) {
        self.history = None;
    }

    /// [saves] Ask for `copy` to be `png`'s picture once that is rendered.
    fn want_picture_copy(&mut self, png: PathBuf, copy: PathBuf) {
        self.pictures_due.insert(copy.clone(), SystemTime::now());
        self.picture_copies.retain(|(_, c)| *c != copy);
        self.picture_copies.push((png, copy));
    }

    /// [saves] The copies waiting for `rendered` (written now): taken.
    pub fn take_picture_copies(&mut self, rendered: &Path) -> Vec<PathBuf> {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.picture_copies).into_iter().partition(|(r, _)| r == rendered);
        self.picture_copies = rest;
        mine.into_iter().map(|(_, c)| c).collect()
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
    /// The file is looked at again only after the game wrote to the saves
    /// folder ([`disk_generation`]), or every frame while a picture asked
    /// for has not landed yet.
    pub fn picture(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        let gen = disk_generation();
        if !self.pictures_due.contains_key(path) {
            if let Some(&(checked, found)) = self.pictures_checked.get(path) {
                if checked == gen {
                    return if found { self.pictures.get(path).map(|(_, tex)| tex.clone()) } else { None };
                }
            }
        }
        let tex = self.load_picture(ctx, path);
        self.pictures_checked.insert(path.to_path_buf(), (gen, tex.is_some()));
        tex
    }

    /// [`Self::picture`] from disk: the file's time, and the texture again
    /// when it changed.
    fn load_picture(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        if self.pictures_due.get(path).is_some_and(|asked| mtime >= *asked) {
            self.pictures_due.remove(path);
        }
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

    /// Delete the oldest dated slots beyond `SLOTS_KEPT`.
    fn prune(&self) {
        prune_slots();
    }
}

/// Delete the oldest dated slots beyond `SLOTS_KEPT`. The zero-padded
/// timestamp makes the file names sort chronologically.
fn prune_slots() {
    let Ok(dir) = std::fs::read_dir(DIR) else { return };
    let mut dated: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("city-") && n.ends_with(".ron"))).collect();
    dated.sort();
    let excess = dated.len().saturating_sub(SLOTS_KEPT);
    for p in dated.into_iter().take(excess) {
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(meta_path(&p));
    }
    if excess > 0 {
        disk_changed();
    }
}

/// Bumped whenever the game writes, deletes or renames something in the
/// saves folder (a save, its sidecar or picture, a history entry, a
/// bookmark, a prune), so what is read from the folder can be kept until it
/// moves. A process-wide counter: the history functions and the remote API
/// write without a `Saves` at hand.
static DISK_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The saves folder changed: see [`disk_generation`].
pub fn disk_changed() {
    DISK_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// The saves folder's write counter: equal values mean nothing the game
/// wrote has changed there in between.
fn disk_generation() -> u64 {
    DISK_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
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
    saves.want_picture(path.with_extension("png"));
    saves.mark_dirty();
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
    saves.mark_dirty();
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

/// [saves] Quick save without stopping the game: the frame pays for a copy
/// of the world (about 90 ms for 17,000 people); the latest slot, its dated
/// copy and their sidecars are written on a worker thread, and the message
/// comes when they are on disk (`autosave` looks in every frame). The Saves
/// window's button; the remote API's `save` stays [`save_now`], done when
/// it answers. A second press while one is writing waits for it.
pub fn save_now_in_background(sim: &mut Sim, saves: &mut Saves, flags: &mut UiFlags) {
    if saves.quick.is_some() {
        flags.message = "still writing the last quick save".into();
        return;
    }
    ensure_game_id(&mut sim.world);
    let world = sim.world.clone();
    sim.saved_tick = Some(sim.world.clock.tick);
    let meta = serde_json::to_string_pretty(&meta_of(sim, "")).ok();
    let slot = format!("{DIR}/city-{:012}-day{}.ron", now_unix(), sim.world.clock.day() + 1);
    let job = std::thread::Builder::new().name("nd-quicksave".into()).spawn(move || {
        crate::sim::save_world(&world, Path::new(LATEST))?;
        drop(world);
        if let Some(text) = &meta {
            let _ = std::fs::write(meta_path(Path::new(LATEST)), text);
        }
        crate::sim::copy_atomic(Path::new(LATEST), Path::new(&slot)).map_err(|e| format!("saved {LATEST}; slot copy failed: {e}"))?;
        if let Some(text) = &meta {
            let _ = std::fs::write(meta_path(Path::new(&slot)), text);
        }
        prune_slots();
        Ok(format!("saved {LATEST} and slot {}", slot_label(Path::new(&slot))))
    });
    match job {
        Ok(j) => {
            saves.quick = Some(j);
            flags.message = "saving…".into();
            saves.want_picture(Path::new(LATEST).with_extension("png"));
        }
        Err(e) => flags.message = format!("save failed: {e}"),
    }
}

/// Replace the running city; it comes up paused so the player can look
/// first. The message (also put in `flags.message`) says how it went. The
/// tools, the selection and the plan cache refer to the old city's ids:
/// `city_reset` clears them for the new `Sim::generation`.
pub fn load_now(path: &Path, sim: &mut Sim, flags: &mut UiFlags) -> Result<String, String> {
    let shown = path.display().to_string();
    let result = match sim.load(&shown) {
        Ok(()) => {
            sim.paused = true;
            if let Some(example) = starter_of(&sim.world) {
                // [identity] An example city at its first day (the bundled
                // Perfect AI City): a new game of its own, under a new name,
                // so its saves never pass for the example's.
                sim.world.identity = nd_sim::CityIdentity { name: String::new(), game_id: new_game_id() };
                let name = rename_city(&mut sim.world, &super::newgame::suggest_name());
                Ok(format!("new game {name} started from the example {example} (paused); rename it in the City or Saves window"))
            } else {
                // [identity] A named save from before city names: the name it
                // was saved under becomes the city's.
                if sim.world.identity.name.trim().is_empty() {
                    if let Some(name) = legacy_city_name(path) {
                        rename_city(&mut sim.world, &name);
                    }
                }
                Ok(format!("loaded {shown} (paused)"))
            }
        }
        Err(e) => Err(format!("load failed: {e}")),
    };
    flags.message = match &result {
        Ok(m) | Err(m) => m.clone(),
    };
    result
}

/// [identity] If a loaded world is a starting city rather than a game in
/// play, the example's name to mention: nobody lives there yet, it is its
/// first day, and it has no game id of its own (examples are written
/// without one, `nd_procgen/examples/perfect_city.rs`) or carries a starting
/// town's name (`CityPreset::label`). A player's own fresh city saved on day
/// one has an id and its own name, and loads as itself.
pub fn starter_of(world: &nd_sim::World) -> Option<String> {
    if !world.citizens.is_empty() || world.clock.day() > 0 {
        return None;
    }
    let name = world.identity.name.trim();
    let preset_name = nd_procgen::citygen::CityPreset::ALL.iter().any(|c| c.label().eq_ignore_ascii_case(name));
    (!world.identity.has_id() || preset_name).then(|| if name.is_empty() { "city".to_string() } else { name.to_string() })
}

/// [identity] The city name an unnamed save takes on load: a named save's
/// name as typed (its sidecar), or else its file name with spaces. None for
/// the quick save slots, the autosave and history entries, which have no
/// name of their own.
fn legacy_city_name(path: &Path) -> Option<String> {
    if path.components().any(|c| c.as_os_str() == super::history::SUBDIR) || slot_kind(path) != SlotKind::Named {
        return None;
    }
    let typed = read_meta(path).map(|m| m.name).filter(|n| !n.trim().is_empty());
    let name = nd_sim::identity::clean_name(&typed.unwrap_or_else(|| slot_label(path)));
    (!name.is_empty()).then_some(name)
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
    saves.want_picture(path.with_extension("png"));
    saves.mark_dirty();
    Ok(path)
}

/// [identity] Go back to a moment of the game's history: load it as Load
/// does (the city comes up paused). The caller has asked first.
pub fn revert(path: &Path, sim: &mut Sim, saves: &mut Saves, flags: &mut UiFlags) -> Result<String, String> {
    let day = super::history::list(Path::new(DIR), &sim.world.identity.game_id).into_iter().find(|e| e.path == path).map(|e| e.meta.day);
    load_now(path, sim, flags)?;
    saves.mark_dirty();
    flags.message = match day {
        Some(d) => format!("reverted to day {d} (paused)"),
        None => format!("reverted to {} (paused)", path.display()),
    };
    Ok(flags.message.clone())
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

/// [title] What a save state of a game is, as the title screen lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateKind {
    Named,
    Latest,
    Autosave,
    /// A dated backup of a quick save.
    Dated,
    /// A daily autosave kept in the game's history.
    History,
    /// A history entry the player named.
    Marked,
}

impl StateKind {
    fn of(kind: SlotKind) -> Self {
        match kind {
            SlotKind::Named => StateKind::Named,
            SlotKind::Latest => StateKind::Latest,
            SlotKind::Autosave => StateKind::Autosave,
            SlotKind::Dated => StateKind::Dated,
        }
    }
}

/// [title] One saved moment of a game: a save in `saves/` or an entry of
/// its history.
#[derive(Clone, Debug)]
pub struct SaveState {
    pub path: PathBuf,
    pub label: String,
    pub kind: StateKind,
    pub modified: Option<SystemTime>,
    pub meta: Option<SaveMeta>,
    pub picture: PathBuf,
}

impl SaveState {
    /// "Day 12 · 340 people · $ 52000", or "" without a sidecar.
    pub fn figures(&self) -> String {
        self.meta.as_ref().map_or_else(String::new, |m| format!("Day {} · {} people · $ {:.0}", m.day, m.population, m.treasury))
    }
}

/// [title] One game (one city as played from its start): every save of it,
/// newest first.
#[derive(Clone, Debug)]
pub struct Game {
    /// The game id, or `city:<name>` for saves from before ids.
    pub key: String,
    /// The city's name as its newest save has it.
    pub city: String,
    pub states: Vec<SaveState>,
}

impl Game {
    pub fn newest(&self) -> Option<&SaveState> {
        self.states.first()
    }
}

/// [title] Saves and history entries grouped by game: by game id where the
/// sidecar has one (two games may share a city name), else by city name.
/// Games newest first, each game's states newest first. `history` is each
/// game id's entries with their file times (`history::list`).
pub fn games(slots: &[Slot], history: &[(String, Vec<(super::history::Entry, Option<SystemTime>)>)]) -> Vec<Game> {
    let mut out: Vec<Game> = Vec::new();
    let mut add = |key: String, city: &str, state: SaveState| match out.iter_mut().find(|g| g.key == key) {
        Some(g) => g.states.push(state),
        None => out.push(Game { key, city: city.to_string(), states: vec![state] }),
    };
    for s in slots {
        let key = s.meta.as_ref().map(|m| m.game_id.trim()).filter(|id| !id.is_empty()).map_or_else(|| format!("city:{}", s.city()), str::to_string);
        add(key, s.city(), SaveState { path: s.path.clone(), label: s.label.clone(), kind: StateKind::of(s.kind), modified: s.modified, meta: s.meta.clone(), picture: s.path.with_extension("png") });
    }
    for (id, entries) in history {
        for (e, modified) in entries {
            let (kind, label) = match e.bookmark() {
                Some(b) => (StateKind::Marked, format!("★ {b}")),
                None => (StateKind::History, format!("History, day {}", e.meta.day)),
            };
            add(id.clone(), e.meta.city_name(), SaveState { path: e.path.clone(), label, kind, modified: *modified, meta: Some(e.meta.clone()), picture: e.picture() });
        }
    }
    for g in &mut out {
        g.states.sort_by(|a, b| b.modified.cmp(&a.modified));
        // The name the city has now: its newest save's.
        if let Some(n) = g.states.first().and_then(|s| s.meta.as_ref()).map(|m| m.city_name().to_string()) {
            if !g.key.starts_with("city:") {
                g.city = n;
            }
        }
    }
    out.sort_by(|a, b| b.newest().and_then(|s| s.modified).cmp(&a.newest().and_then(|s| s.modified)));
    out
}

/// [title] Every game's history in a saves folder: (game id, entries with
/// their file times), for [`games`].
pub fn scan_history(dir: &Path) -> Vec<(String, Vec<(super::history::Entry, Option<SystemTime>)>)> {
    let Ok(rd) = std::fs::read_dir(dir.join(super::history::SUBDIR)) else { return Vec::new() };
    let mut out = Vec::new();
    for d in rd.flatten().filter(|d| d.path().is_dir()) {
        let Some(id) = d.file_name().to_str().map(str::to_string) else { continue };
        let entries: Vec<_> = super::history::list(dir, &id).into_iter().map(|e| {
            let modified = std::fs::metadata(&e.path).and_then(|m| m.modified()).ok();
            (e, modified)
        }).collect();
        if !entries.is_empty() {
            out.push((id, entries));
        }
    }
    out
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
            ui.label(egui::RichText::new(sim.world.identity.display_name()).color(GOLD).strong().font(crate::ui::theme::lead()));
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
    super::fmt::age_since(modified, SystemTime::now())
}

/// The body of the Saves window: save as a name, quick save, the list with
/// pictures, the load confirmation.
pub fn panel(ui: &mut egui::Ui, sim: &mut Sim, saves: &mut Saves, flags: &mut UiFlags) {
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
            save_now_in_background(sim, saves, flags);
        }
        if ui.small_button("refresh list").clicked() {
            saves.mark_dirty();
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
                let _ = load_now(&pending, sim, flags);
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
                    ui.painter().rect_filled(r, 3.0, super::theme::NO_PICTURE);
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
            let _ = load_now(&path, sim, flags);
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
    // [saves] A quick save written in the background, landed.
    if saves.quick.as_ref().is_some_and(|j| j.is_finished()) {
        let job = saves.quick.take().expect("checked above");
        saves.mark_dirty();
        flags.message = match job.join() {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => format!("save failed: {e}"),
            Err(_) => "save failed: the writer thread panicked".into(),
        };
    }
    if saves.autosave.as_ref().is_some_and(|j| j.is_finished()) {
        let job = saves.autosave.take().expect("checked above");
        saves.mark_dirty();
        flags.message = match job.join() {
            Ok(Ok((day, ms))) => {
                debug!("autosave of day {day} written in {ms:.0} ms off the main thread");
                // [identity] The game's history keeps its newest autosaves
                // and every marked one.
                let pruned = super::history::prune_thinned(Path::new(DIR), &sim.world.identity.game_id, super::history::KEPT, super::history::KEPT_SPACED, super::history::SPACING);
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
    // once, then copied.
    let entry = super::history::entry_path(Path::new(DIR), &world.identity.game_id, day, now_unix_ms());
    let entry_bg = entry.clone();
    let job = std::thread::Builder::new().name("nd-autosave".into()).spawn(move || {
        let t = std::time::Instant::now();
        // [memory] Streamed to the history entry, then copied: never the
        // whole save as one string (over 128 MiB for a big city).
        crate::sim::save_world(&world, &entry_bg)?;
        drop(world);
        crate::sim::copy_atomic(&entry_bg, Path::new(AUTOSAVE)).map_err(|e| e.to_string())?;
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
    saves.want_picture(entry.with_extension("png"));
    saves.want_picture_copy(entry.with_extension("png"), Path::new(AUTOSAVE).with_extension("png"));
    saves.mark_dirty();
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

    /// [memory] A save is streamed to its file (never built as one string)
    /// and the autosave's second copy is a file copy: both hold exactly the
    /// RON `to_save_ron` gives ([saves] compressed: unpacked first), and load back.
    #[test]
    fn streamed_saves_are_the_ron_byte_for_byte() {
        let mut sim = Sim::new();
        sim.seed_demo_district();
        for _ in 0..600 {
            sim.world.tick(&mut sim.minds);
        }
        let dir = std::env::temp_dir().join(format!("nd_stream_save_{}_{}", std::process::id(), new_game_id()));
        let a = dir.join("a.ron");
        let b = dir.join("sub").join("b.ron");
        crate::sim::save_world(&sim.world, &a).expect("save");
        crate::sim::copy_atomic(&a, &b).expect("copy");
        let want = sim.world.to_save_ron().unwrap();
        assert!(want.len() < sim.world.to_ron().unwrap().len(), "[saves] not indented");
        let unpack = |p: &Path| nd_sim::world::save_text(&std::fs::read(p).unwrap()).unwrap();
        assert!(nd_sim::world::is_compressed(&std::fs::read(&a).unwrap()), "saves are compressed");
        assert!(std::fs::metadata(&a).unwrap().len() * 4 < want.len() as u64, "and much smaller");
        assert_eq!(unpack(&a), want);
        assert_eq!(unpack(&b), want);
        let back = nd_sim::World::read_save(&b).expect("loads");
        // A plain RON save from before compression still loads.
        let plain = dir.join("plain.ron");
        std::fs::write(&plain, &want).unwrap();
        assert_eq!(nd_sim::World::read_save(&plain).expect("plain loads").clock.tick, sim.world.clock.tick);
        assert_eq!(back.citizens.len(), sim.world.citizens.len());
        assert_eq!(back.buildings.len(), sim.world.buildings.len());
        assert_eq!(back.clock.tick, sim.world.clock.tick);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "tmp")).collect();
        assert!(leftovers.is_empty(), "no temporary files left behind");
        let _ = std::fs::remove_dir_all(&dir);
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

    /// [identity] An old named save takes its save name as the city name;
    /// quick saves, the autosave and history entries do not.
    #[test]
    fn legacy_named_saves_take_their_name() {
        let dir = std::env::temp_dir().join(format!("nd_legacy_name_{}_{}", std::process::id(), new_game_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let typed = dir.join("Neon_Bay.ron");
        std::fs::write(meta_path(&typed), r#"{"name":"Neon Bay!","day":3,"population":10,"treasury":5.0,"saved_unix":1}"#).unwrap();
        assert_eq!(legacy_city_name(&typed).as_deref(), Some("Neon Bay!"));
        assert_eq!(legacy_city_name(&dir.join("Old_Town.ron")).as_deref(), Some("Old Town"), "no sidecar: the file name");
        assert_eq!(legacy_city_name(&dir.join("city.ron")), None);
        assert_eq!(legacy_city_name(&dir.join("autosave.ron")), None);
        assert_eq!(legacy_city_name(&dir.join("city-000001700000-day3.ron")), None);
        assert_eq!(legacy_city_name(&dir.join("history").join("g").join("000000000001000-day3.ron")), None);
        // Loaded: the city takes it. (A city played past its first day: an
        // empty day-one save without an id is a starting city, below.)
        let mut sim0 = Sim::new();
        sim0.world.clock.tick = 2 * 1440 + 30;
        let legacy = dir.join("Old_Town.ron");
        std::fs::write(&legacy, legacy_ron(&sim0.world)).unwrap();
        let mut sim = Sim::new();
        let mut flags = UiFlags::default();
        load_now(&legacy, &mut sim, &mut flags).unwrap();
        assert!(flags.message.starts_with("loaded"), "{}", flags.message);
        assert_eq!(sim.world.identity.name, "Old Town");
        assert!(!sim.world.identity.has_id(), "the id still comes with the next save");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [identity] An example city at its first day (no id, nobody living
    /// there: the bundled Perfect AI City) loads as a new game with its own
    /// id and a new name; a player's fresh day-one save loads as itself.
    #[test]
    fn example_cities_load_as_new_games() {
        let dir = std::env::temp_dir().join(format!("nd_starter_{}_{}", std::process::id(), new_game_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut example = Sim::new();
        example.world.identity = nd_sim::CityIdentity { name: "Perfect AI City".into(), game_id: String::new() };
        let path = dir.join("Perfect_AI_City.ron");
        example.save(&path).unwrap();
        let mut sim = Sim::new();
        let mut flags = UiFlags::default();
        let msg = load_now(&path, &mut sim, &mut flags).unwrap();
        assert!(msg.starts_with("new game"), "{msg}");
        assert!(sim.world.identity.has_id());
        assert!(!sim.world.identity.name.is_empty() && sim.world.identity.name != "Perfect AI City", "{}", sim.world.identity.name);
        let first = sim.world.identity.clone();
        load_now(&path, &mut sim, &mut flags).unwrap();
        assert_ne!(sim.world.identity.game_id, first.game_id, "each start is its own game");
        // The example written with an id, under a starting town's name: still a starter.
        example.world.identity.game_id = new_game_id();
        example.save(&path).unwrap();
        load_now(&path, &mut sim, &mut flags).unwrap();
        assert_ne!(sim.world.identity.name, "Perfect AI City");
        // A player's own day-one city loads as itself.
        let mut own = Sim::new();
        own.set_city_name("Cobalt Reach");
        let own_path = dir.join("Cobalt_Reach.ron");
        own.save(&own_path).unwrap();
        let msg = load_now(&own_path, &mut sim, &mut flags).unwrap();
        assert!(msg.starts_with("loaded"), "{msg}");
        assert_eq!(sim.world.identity, own.world.identity);
        // And a played example (people, later days) keeps its name.
        example.world.clock.tick = 3 * 1440;
        assert!(starter_of(&example.world).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [identity] Grouping by city name: newest group first, unnamed saves
    /// together.
    #[test]
    fn saves_group_by_city() {
        let slot = |city: Option<&str>, n: &str| Slot {
            path: PathBuf::from(format!("saves/{n}.ron")),
            label: n.into(),
            kind: if city.is_some_and(|c| !c.is_empty()) { SlotKind::Named } else { SlotKind::Autosave },
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
        // An old named save groups under the name it was saved under.
        let old = Slot { path: PathBuf::from("saves/Old_Town.ron"), label: "Old Town".into(), kind: SlotKind::Named, modified: None, meta: None, picture: None };
        assert_eq!(old.city(), "Old Town");
    }

    /// [title] Games group by id (two games of one name stay apart), by
    /// name without one; history entries join their game; newest first.
    #[test]
    fn saves_group_into_games() {
        let t = |s: u64| Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s));
        let slot = |n: &str, city: &str, id: &str, kind: SlotKind, at: u64| Slot {
            path: PathBuf::from(format!("saves/{n}.ron")),
            label: n.into(),
            kind,
            modified: t(at),
            meta: Some(SaveMeta { city: city.into(), game_id: id.into(), day: at, ..Default::default() }),
            picture: None,
        };
        let slots = vec![
            slot("city", "Neon Bay", "g2", SlotKind::Latest, 50),
            slot("Neon_Bay", "Neon Bay", "g1", SlotKind::Named, 40),
            slot("autosave", "Neon Bay", "g2", SlotKind::Autosave, 30),
            slot("Old", "Old Town", "", SlotKind::Named, 10),
        ];
        let entry = |id: &str, day: u64, mark: Option<&str>| super::super::history::Entry { path: PathBuf::from(format!("saves/history/{id}/{day}.ron")), meta: SaveMeta { day, city: "Neon Bay".into(), game_id: id.into(), bookmark: mark.map(String::from), ..Default::default() } };
        let history = vec![("g1".to_string(), vec![(entry("g1", 3, None), t(60)), (entry("g1", 2, Some("bridge")), t(20))])];
        let g = games(&slots, &history);
        let keys: Vec<&str> = g.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, vec!["g1", "g2", "city:Old Town"], "newest game first; same name, two games");
        assert_eq!(g[0].states.len(), 3);
        assert_eq!(g[0].states[0].kind, StateKind::History);
        assert_eq!(g[0].states[2].kind, StateKind::Marked);
        assert_eq!(g[0].states[2].label, "★ bridge");
        assert_eq!(g[1].states.iter().map(|s| s.kind).collect::<Vec<_>>(), vec![StateKind::Latest, StateKind::Autosave]);
        assert_eq!(g[2].city, "Old Town");
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
        let (mut saves, mut flags) = (Saves::default(), UiFlags::default());
        let generation = sim.generation;
        revert(&early, &mut sim, &mut saves, &mut flags).unwrap();
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
