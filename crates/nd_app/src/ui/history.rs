//! [identity] Game history: every game keeps its autosaves, so the player can
//! look back over the city, mark the key moments with a name and revert to
//! an earlier day.
//!
//! - **Where**: `saves/history/<game id>/<unix ms>-day<N>.ron`, each with the
//!   sidecar (`.meta.json`, as every save has: the day, population,
//!   treasury, the city's name and id, and the bookmark) and a map picture
//!   (`.png`). The millisecond stamp first makes the file names sort in the
//!   order they were written, which is not the order of the days once the
//!   player has reverted and played on.
//! - **What goes in**: the daily autosave (`saves::autosave` writes it here
//!   and to `saves/autosave.ron`), and "Mark this moment", which saves now
//!   with a bookmark.
//! - **Kept**: the newest [`KEPT`] unmarked entries of a game; bookmarked
//!   ones are never pruned.
//! - **Revert**: loads an entry, after a confirmation, as Load does. Later
//!   entries stay, so a revert can itself be undone from the list.
//!
//! The storage functions take the saves folder as a parameter so the tests
//! run in a temporary one; the game passes [`super::saves::DIR`].

use std::path::{Path, PathBuf};

use bevy_egui::egui;

use super::saves::{age, file_stem, meta_path, SaveMeta};
use super::{hint, section, UiCtx, GOLD, IVORY, WARN};

/// The history folder inside the saves folder.
pub const SUBDIR: &str = "history";
/// Unmarked entries kept per game; the oldest go first.
pub const KEPT: usize = 24;

/// One saved moment of a game.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub meta: SaveMeta,
}

impl Entry {
    pub fn picture(&self) -> PathBuf {
        self.path.with_extension("png")
    }

    pub fn bookmark(&self) -> Option<&str> {
        self.meta.bookmark.as_deref().filter(|b| !b.trim().is_empty())
    }

    /// "Day 12 · 340 people · $ 52000".
    pub fn figures(&self) -> String {
        format!("Day {} · {} people · $ {:.0}", self.meta.day, self.meta.population, self.meta.treasury)
    }
}

/// The folder of one game's history. The id is made file-safe: a save may
/// carry any text there.
pub fn game_dir(saves: &Path, game_id: &str) -> PathBuf {
    saves.join(SUBDIR).join(file_stem(game_id))
}

/// Where a new entry of `game_id` for `day`, written at `unix_ms`, goes. A
/// name already taken (two saves in one millisecond) gets the next stamp.
pub fn entry_path(saves: &Path, game_id: &str, day: u64, unix_ms: u64) -> PathBuf {
    let dir = game_dir(saves, game_id);
    let mut ms = unix_ms;
    loop {
        let p = dir.join(format!("{ms:015}-day{day}.ron"));
        if !p.exists() {
            return p;
        }
        ms += 1;
    }
}

/// Every entry of a game, newest first. Entries whose sidecar is missing
/// or unreadable (a crash between the two writes) still list, with the day
/// from the file name and nothing else.
pub fn list(saves: &Path, game_id: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    if game_id.is_empty() {
        return out;
    }
    let Ok(dir) = std::fs::read_dir(game_dir(saves, game_id)) else { return out };
    for e in dir.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "ron") {
            continue;
        }
        let meta = std::fs::read_to_string(meta_path(&path)).ok().and_then(|t| serde_json::from_str::<SaveMeta>(&t).ok()).unwrap_or_else(|| {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let day = stem.rsplit_once("-day").and_then(|(_, d)| d.parse().ok()).unwrap_or(0);
            SaveMeta { day, game_id: game_id.to_string(), ..Default::default() }
        });
        out.push(Entry { path, meta });
    }
    // The stamp leads the file name: reverse name order is newest first.
    out.sort_by(|a, b| b.path.file_name().cmp(&a.path.file_name()));
    out
}

/// Name an entry (a key moment), or clear its name with `None` or "".
pub fn set_bookmark(entry: &Path, label: Option<&str>) -> Result<SaveMeta, String> {
    if !entry.exists() {
        return Err(format!("no history entry {}", entry.display()));
    }
    let mp = meta_path(entry);
    let mut meta: SaveMeta = std::fs::read_to_string(&mp).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let label = label.map(nd_sim::identity::clean_name).filter(|l| !l.is_empty());
    meta.bookmark = label;
    let text = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    crate::sim::write_atomic(&mp, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(meta)
}

/// Delete a game's oldest unmarked entries beyond `kept` (with their
/// sidecars and pictures). Returns how many went.
pub fn prune(saves: &Path, game_id: &str, kept: usize) -> usize {
    let entries = list(saves, game_id);
    let mut seen = 0;
    let mut gone = 0;
    for e in entries {
        if e.bookmark().is_some() {
            continue;
        }
        seen += 1;
        if seen > kept {
            let _ = std::fs::remove_file(&e.path);
            let _ = std::fs::remove_file(meta_path(&e.path));
            let _ = std::fs::remove_file(e.picture());
            gone += 1;
        }
    }
    gone
}

/// The entry of a game a path names, if it is one: the path must be in
/// that game's history folder. Guards revert and bookmark against paths
/// from elsewhere.
pub fn find(saves: &Path, game_id: &str, path: &Path) -> Option<Entry> {
    let want = std::fs::canonicalize(path).ok()?;
    list(saves, game_id).into_iter().find(|e| std::fs::canonicalize(&e.path).is_ok_and(|p| p == want))
}

fn open_id() -> egui::Id {
    egui::Id::new("game_history_open")
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// What the viewer is in the middle of: a revert to confirm, a bookmark
/// being typed.
#[derive(Clone, Default)]
struct ViewState {
    confirm: Option<PathBuf>,
    naming: Option<(PathBuf, String)>,
    mark_label: String,
    marked_only: bool,
}

/// The Game history window: the running game's autosaves newest first,
/// with their pictures and bookmarks; mark this moment; revert.
pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    if let Some(open) = p.saves.history_requested.take() {
        set_open(ctx, open);
    }
    if !is_open(ctx) {
        return;
    }
    let sid = egui::Id::new("game_history_state");
    let mut st: ViewState = ctx.data(|d| d.get_temp(sid)).unwrap_or_default();
    let mut open = true;
    egui::Window::new(egui::RichText::new(format!("Game history: {}", p.sim.world.identity.display_name())).color(GOLD))
        .id(egui::Id::new("game_history_window"))
        .open(&mut open)
        .constrain_to(content)
        .default_pos(content.center() - egui::vec2(230.0, 260.0))
        .default_size(egui::vec2(460.0, 520.0))
        .resizable(true)
        .show(ctx, |ui| body(ui, p, &mut st));
    if !open {
        set_open(ctx, false);
        st.confirm = None;
        st.naming = None;
    }
    ctx.data_mut(|d| d.insert_temp(sid, st));
}

fn body(ui: &mut egui::Ui, p: &mut UiCtx, st: &mut ViewState) {
    let saves_dir = Path::new(super::saves::DIR);
    let game_id = p.sim.world.identity.game_id.clone();
    hint(ui, &format!("Every day's autosave is kept here (the newest {KEPT}, and every marked one). Mark the moments that matter; revert to go back to one."));
    section(ui, "MARK THIS MOMENT");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut st.mark_label).desired_width(200.0).hint_text("e.g. first metro line"));
        if ui.button("Mark").on_hover_text("Saves the city now into its history, under this name").clicked() {
            let label = if st.mark_label.trim().is_empty() { format!("Day {}", p.sim.world.clock.day() + 1) } else { st.mark_label.clone() };
            match super::saves::mark_now(p.sim, p.saves, &label) {
                Ok(_) => {
                    p.flags.message = format!("marked \"{label}\" in the game history");
                    st.mark_label.clear();
                }
                Err(e) => p.flags.message = format!("mark failed: {e}"),
            }
        }
    });
    if let Some(target) = st.confirm.clone() {
        let entry = find(saves_dir, &game_id, &target);
        let what = entry.as_ref().map_or_else(|| "that autosave".to_string(), |e| e.bookmark().map_or_else(|| format!("day {}", e.meta.day), |b| format!("\"{b}\" (day {})", e.meta.day)));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(format!("Revert to {what}? The running city (day {}, {}) is replaced. Its newer autosaves stay in this list.", p.sim.world.clock.day() + 1, p.sim.world.clock.format_hhmm())).color(WARN));
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new("Revert").strong()).clicked() {
                super::saves::revert(&target, p.sim, p.tools, p.selection, p.plan, p.saves, p.flags);
                st.confirm = None;
            }
            if ui.button("Cancel").clicked() {
                st.confirm = None;
            }
        });
        return;
    }
    section(ui, "AUTOSAVES");
    ui.checkbox(&mut st.marked_only, "marked moments only");
    let entries = list(saves_dir, &game_id);
    if entries.is_empty() {
        hint(ui, "Nothing yet: the first autosave comes at the start of the next day.");
        return;
    }
    egui::ScrollArea::vertical().id_salt("game_history_list").auto_shrink([false, false]).show(ui, |ui| {
        for e in entries.iter().filter(|e| !st.marked_only || e.bookmark().is_some()) {
            ui.horizontal(|ui| {
                match p.saves.picture(ui.ctx(), &e.picture()) {
                    Some(tex) => {
                        ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(56.0, 56.0)).corner_radius(3.0));
                    }
                    None => {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 3.0, egui::Color32::from_rgb(20, 22, 34));
                    }
                }
                ui.vertical(|ui| {
                    match e.bookmark() {
                        Some(b) => ui.label(egui::RichText::new(format!("★ {b}")).color(GOLD).strong()),
                        None => ui.label(egui::RichText::new(format!("Day {}", e.meta.day)).color(IVORY)),
                    };
                    let modified = std::fs::metadata(&e.path).and_then(|m| m.modified()).ok();
                    hint(ui, &format!("{} · {}", e.figures(), age(modified)));
                    let naming_this = st.naming.as_ref().is_some_and(|(p, _)| *p == e.path);
                    ui.horizontal(|ui| {
                        if naming_this {
                            let mut done = false;
                            if let Some((_, text)) = st.naming.as_mut() {
                                let r = ui.add(egui::TextEdit::singleline(text).desired_width(150.0).hint_text("name this moment"));
                                done = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            }
                            if ui.small_button("OK").clicked() || done {
                                let label = st.naming.take().map(|(_, t)| t).unwrap_or_default();
                                if let Err(err) = set_bookmark(&e.path, Some(&label)) {
                                    p.flags.message = format!("bookmark failed: {err}");
                                }
                            }
                            if ui.small_button("Cancel").clicked() {
                                st.naming = None;
                            }
                        } else {
                            if ui.small_button(if e.bookmark().is_some() { "Rename" } else { "Mark" }).on_hover_text("Name this moment; marked moments are never pruned").clicked() {
                                st.naming = Some((e.path.clone(), e.bookmark().unwrap_or("").to_string()));
                            }
                            if e.bookmark().is_some() && ui.small_button("Unmark").clicked() {
                                if let Err(err) = set_bookmark(&e.path, None) {
                                    p.flags.message = format!("bookmark failed: {err}");
                                }
                            }
                            if ui.small_button("Revert…").on_hover_text("Go back to this moment (asks first)").clicked() {
                                st.confirm = Some(e.path.clone());
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nd_history_test_{tag}_{}_{}", std::process::id(), super::super::saves::new_game_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_entry(saves: &Path, game: &str, day: u64, ms: u64, bookmark: Option<&str>) -> PathBuf {
        let p = entry_path(saves, game, day, ms);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "()").unwrap();
        let meta = SaveMeta { day, game_id: game.into(), city: "Neon Bay".into(), bookmark: bookmark.map(String::from), ..Default::default() };
        std::fs::write(meta_path(&p), serde_json::to_string(&meta).unwrap()).unwrap();
        p
    }

    #[test]
    fn entries_list_newest_first_and_prune_keeps_bookmarks() {
        let saves = tmp("prune");
        let game = "abc-123";
        for day in 1..=10u64 {
            write_entry(&saves, game, day, 1_000 + day, if day == 2 { Some("first bus") } else { None });
        }
        // Another game's history is its own.
        write_entry(&saves, "other", 1, 5, None);
        let all = list(&saves, game);
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].meta.day, 10);
        assert_eq!(all[9].meta.day, 1);
        assert_eq!(prune(&saves, game, 3), 6);
        let left = list(&saves, game);
        let days: Vec<u64> = left.iter().map(|e| e.meta.day).collect();
        assert_eq!(days, vec![10, 9, 8, 2], "three newest unmarked and the bookmark");
        assert_eq!(left[3].bookmark(), Some("first bus"));
        assert_eq!(list(&saves, "other").len(), 1);
        // A clash in one millisecond takes the next stamp.
        let a = entry_path(&saves, game, 10, 1_010);
        assert_ne!(a, left[0].path);
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn bookmarks_set_rename_and_clear() {
        let saves = tmp("bookmark");
        let p = write_entry(&saves, "g", 4, 77, None);
        assert_eq!(set_bookmark(&p, Some("  bridge  opened ")).unwrap().bookmark.as_deref(), Some("bridge opened"));
        assert_eq!(list(&saves, "g")[0].bookmark(), Some("bridge opened"));
        assert_eq!(set_bookmark(&p, Some("")).unwrap().bookmark, None);
        assert_eq!(set_bookmark(&p, None).unwrap().bookmark, None);
        assert!(set_bookmark(&saves.join("nope.ron"), Some("x")).is_err());
        // `find` takes only this game's entries.
        assert!(find(&saves, "g", &p).is_some());
        assert!(find(&saves, "other", &p).is_none());
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn hostile_ids_stay_inside_the_history_folder() {
        let saves = Path::new("saves");
        let d = game_dir(saves, "../../windows/system32");
        assert!(d.starts_with("saves/history") || d.starts_with("saves\\history"));
        assert!(!d.to_string_lossy().contains(".."));
    }
}
