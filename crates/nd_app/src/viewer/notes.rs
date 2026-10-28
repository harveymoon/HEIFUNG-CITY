//! [viewer] Notes on buildings and styles: the player's (or an agent's)
//! remarks from the building viewer, kept as work items.
//!
//! A note is on a building standing in a city (by the game's id, the
//! building's id, and its name and place, so it still reads when the
//! building is gone) or on a style of the catalogue (by the style's key).
//! Notes live in the game's home folder in `building_notes.ron`, not in a
//! save: they are about the game's look, across every city. Each change
//! rewrites the file. The remote API's `building_notes` lists, reads,
//! writes, answers and resolves them, so Claude and other agents can pick
//! them up as work.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Where the notes are kept, under the home folder.
pub const NOTES_FILE: &str = "building_notes.ron";

/// What a note is about.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// A building in a city.
    Building {
        /// The game's id (`World::identity.game_id`; "" for an old save).
        game: String,
        /// The city's name when the note was made.
        #[serde(default)]
        city: String,
        /// The building's id in that game (`remote` key form, "3v1").
        building: String,
        /// Its name and lot centre when the note was made.
        name: String,
        pos: [f32; 2],
        /// The style it was drawn in, for reading the note without the city.
        #[serde(default)]
        style: String,
    },
    /// A style of the catalogue, by key (`Tenement`).
    Style { style: String },
}

impl Target {
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Building { .. } => "building",
            Target::Style { .. } => "style",
        }
    }

    /// One line for lists: the building's name or the style.
    pub fn title(&self) -> String {
        match self {
            Target::Building { name, style, .. } if !style.is_empty() => format!("{name} ({style})"),
            Target::Building { name, .. } => name.clone(),
            Target::Style { style } => format!("style {style}"),
        }
    }

    pub fn json(&self) -> Value {
        match self {
            Target::Building { game, city, building, name, pos, style } => json!({ "kind": "building", "game": game, "city": city, "building": building, "name": name, "pos": pos, "style": style }),
            Target::Style { style } => json!({ "kind": "style", "style": style }),
        }
    }

    /// The same building or style (a building by game and id).
    pub fn same(&self, other: &Target) -> bool {
        match (self, other) {
            (Target::Building { game: g1, building: b1, .. }, Target::Building { game: g2, building: b2, .. }) => g1 == g2 && b1 == b2,
            (Target::Style { style: a }, Target::Style { style: b }) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

/// An answer under a note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub unix: u64,
    /// "player", "claude" or whoever the API caller says.
    pub by: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u32,
    pub target: Target,
    pub text: String,
    pub by: String,
    pub created: u64,
    pub updated: u64,
    /// When it was marked done, and by whom; `None` while it is open.
    #[serde(default)]
    pub resolved: Option<(u64, String)>,
    #[serde(default)]
    pub replies: Vec<Reply>,
}

impl Note {
    pub fn is_open(&self) -> bool {
        self.resolved.is_none()
    }

    pub fn json(&self) -> Value {
        json!({
            "id": self.id, "target": self.target.json(), "text": self.text, "by": self.by,
            "created": self.created, "updated": self.updated, "status": if self.is_open() { "open" } else { "resolved" },
            "resolved": self.resolved.as_ref().map(|(t, by)| json!({ "unix": t, "by": by })),
            "replies": self.replies.iter().map(|r| json!({ "unix": r.unix, "by": r.by, "text": r.text })).collect::<Vec<_>>(),
        })
    }
}

/// The file's contents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoteFile {
    pub next: u32,
    pub notes: Vec<Note>,
}

/// Every note, as kept in [`NOTES_FILE`].
#[derive(Resource, Default)]
pub struct BuildingNotes {
    pub book: NoteFile,
    /// Where they are written; `None` keeps them in memory (tests).
    path: Option<std::path::PathBuf>,
    /// The last write's error, shown in the viewer.
    pub error: Option<String>,
    /// Bumped on every change, for lists that cache.
    pub version: u64,
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl BuildingNotes {
    /// Read `path` (a missing file is no notes; an unreadable one is kept
    /// aside as `.bad` so a write does not lose it).
    pub fn load(path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let mut error = None;
        let book = match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<NoteFile>(&text) {
                Ok(b) => b,
                Err(e) => {
                    let _ = std::fs::copy(&path, path.with_extension("ron.bad"));
                    error = Some(format!("{}: {e} (kept as .ron.bad)", path.display()));
                    NoteFile::default()
                }
            },
            Err(_) => NoteFile::default(),
        };
        let mut n = Self { book, path: Some(path), error, version: 0 };
        n.book.next = n.book.next.max(n.book.notes.iter().map(|x| x.id).max().unwrap_or(0));
        n
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    fn save(&mut self) {
        self.version += 1;
        let Some(path) = &self.path else { return };
        let text = match ron::ser::to_string_pretty(&self.book, ron::ser::PrettyConfig::default()) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        // Written aside and renamed, so a crash mid-write keeps the old file.
        let tmp = path.with_extension("ron.tmp");
        self.error = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path)).err().map(|e| format!("{}: {e}", path.display()));
    }

    pub fn get(&self, id: u32) -> Option<&Note> {
        self.book.notes.iter().find(|n| n.id == id)
    }

    /// The notes on a building or style, oldest first.
    pub fn on<'a>(&'a self, target: &'a Target) -> impl Iterator<Item = &'a Note> + 'a {
        self.book.notes.iter().filter(move |n| n.target.same(target))
    }

    pub fn open_count(&self, target: &Target) -> usize {
        self.on(target).filter(|n| n.is_open()).count()
    }

    /// A new note; returns its id.
    pub fn add(&mut self, target: Target, text: &str, by: &str) -> Result<u32, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("a note needs text".into());
        }
        self.book.next += 1;
        let id = self.book.next;
        let t = now_unix();
        self.book.notes.push(Note { id, target, text: text.to_string(), by: by.to_string(), created: t, updated: t, resolved: None, replies: Vec::new() });
        self.save();
        Ok(id)
    }

    /// Change a note's text.
    pub fn edit(&mut self, id: u32, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("a note needs text (delete it instead)".into());
        }
        let n = self.book.notes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("no note {id}"))?;
        n.text = text.to_string();
        n.updated = now_unix();
        self.save();
        Ok(())
    }

    pub fn reply(&mut self, id: u32, text: &str, by: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("a reply needs text".into());
        }
        let n = self.book.notes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("no note {id}"))?;
        let t = now_unix();
        n.replies.push(Reply { unix: t, by: by.to_string(), text: text.to_string() });
        n.updated = t;
        self.save();
        Ok(())
    }

    /// Mark done (`done`) or open again.
    pub fn resolve(&mut self, id: u32, done: bool, by: &str) -> Result<(), String> {
        let n = self.book.notes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("no note {id}"))?;
        let t = now_unix();
        n.resolved = done.then(|| (t, by.to_string()));
        n.updated = t;
        self.save();
        Ok(())
    }

    pub fn delete(&mut self, id: u32) -> Result<(), String> {
        let before = self.book.notes.len();
        self.book.notes.retain(|n| n.id != id);
        if self.book.notes.len() == before {
            return Err(format!("no note {id}"));
        }
        self.save();
        Ok(())
    }
}

pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn build(&self, app: &mut App) {
        // The home folder is the working directory by now (`home::enter`).
        app.insert_resource(BuildingNotes::load(NOTES_FILE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nd_notes_{}_{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join(NOTES_FILE);
        let _ = std::fs::remove_file(&p);
        p
    }

    fn house() -> Target {
        Target::Building { game: "g1".into(), city: "Testville".into(), building: "4v1".into(), name: "Kestrel House".into(), pos: [10.0, -4.0], style: "walk-up tenement".into() }
    }

    /// [viewer] Notes survive a restart: written on every change, read
    /// back whole with their replies, resolutions and the next id.
    #[test]
    fn notes_save_and_load() {
        let path = temp("roundtrip");
        let mut n = BuildingNotes::load(&path);
        assert!(n.book.notes.is_empty(), "no file is no notes");
        let a = n.add(house(), "  the roof garden floats  ", "player").unwrap();
        let b = n.add(Target::Style { style: "PencilTower".into() }, "too plain at night", "player").unwrap();
        assert!(n.add(Target::Style { style: "X".into() }, "   ", "player").is_err(), "an empty note is refused");
        n.reply(a, "fixed the pad height", "claude").unwrap();
        n.resolve(a, true, "claude").unwrap();
        n.edit(b, "too plain at night: more signs").unwrap();
        assert!(n.error.is_none(), "{:?}", n.error);
        let back = BuildingNotes::load(&path);
        assert_eq!(back.book, n.book);
        let note = back.get(a).unwrap();
        assert_eq!(note.text, "the roof garden floats");
        assert!(!note.is_open() && note.replies.len() == 1);
        assert_eq!(back.open_count(&Target::Style { style: "pencilTOWER".into() }), 1, "style keys match in any case");
        // Ids keep counting after a delete and a reload.
        let mut back = back;
        back.delete(b).unwrap();
        let c = back.add(house(), "again", "player").unwrap();
        assert!(c > b);
        assert_eq!(BuildingNotes::load(&path).on(&house()).count(), 2);
    }

    /// A file from an older build with fields missing still loads; a broken
    /// one is kept aside, not overwritten.
    #[test]
    fn old_and_broken_files() {
        let path = temp("old");
        std::fs::write(&path, r#"(notes: [(id: 3, target: Style(style: "Stacks"), text: "more laundry", by: "player", created: 1, updated: 1)])"#).unwrap();
        let mut n = BuildingNotes::load(&path);
        assert!(n.error.is_none(), "{:?}", n.error);
        assert_eq!(n.book.notes.len(), 1);
        assert!(n.book.notes[0].is_open() && n.book.notes[0].replies.is_empty());
        assert_eq!(n.add(Target::Style { style: "Stacks".into() }, "and cables", "player").unwrap(), 4, "next id follows the file's");
        let bad = temp("bad");
        std::fs::write(&bad, "not ron at all (").unwrap();
        let n = BuildingNotes::load(&bad);
        assert!(n.error.is_some());
        assert!(bad.with_extension("ron.bad").exists());
    }
}
