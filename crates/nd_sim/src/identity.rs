//! [identity] Which city a save belongs to: the name the player gave it and
//! a game id made once when the game starts. Saved with the world, so every
//! save of one game (named, quick, autosave and its history) carries both.
//!
//! The sim never makes an id itself (it stays deterministic): the app makes
//! one for a new game and for a save that has none. Saves from before
//! identities load with both empty, which reads as [`UNNAMED`].

use serde::{Deserialize, Serialize};

/// What a city without a name is called.
pub const UNNAMED: &str = "Unnamed city";
/// The longest name kept, in characters.
pub const NAME_MAX: usize = 40;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CityIdentity {
    /// As the player typed it (tidied by [`clean_name`]); "" for none.
    pub name: String,
    /// Unique per game; "" for a save from before ids.
    pub game_id: String,
}

impl CityIdentity {
    /// The name to show: the city's own, or [`UNNAMED`].
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            UNNAMED
        } else {
            self.name.as_str()
        }
    }

    pub fn has_id(&self) -> bool {
        !self.game_id.is_empty()
    }
}

/// A name as kept: control characters dropped, runs of whitespace folded to
/// one space, the ends trimmed, at most [`NAME_MAX`] characters.
pub fn clean_name(raw: &str) -> String {
    let mut out = String::new();
    for word in raw.split_whitespace() {
        let word: String = word.chars().filter(|c| !c.is_control()).collect();
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
    }
    out.chars().take(NAME_MAX).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_tidied_and_unnamed_reads_as_such() {
        assert_eq!(clean_name("  Neon \t Bay\n"), "Neon Bay");
        assert_eq!(clean_name("a\u{7}b"), "ab");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), NAME_MAX);
        assert_eq!(clean_name("Ünïcode Städt"), "Ünïcode Städt");
        let id = CityIdentity::default();
        assert_eq!(id.display_name(), UNNAMED);
        assert!(!id.has_id());
        let named = CityIdentity { name: "Neon Bay".into(), game_id: "g1".into() };
        assert_eq!(named.display_name(), "Neon Bay");
    }
}
