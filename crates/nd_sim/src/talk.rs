//! [talk] The mayor's chats with citizens ("Talk" in the person pane),
//! saved with the city so a conversation picks up where it left off.
//!
//! Display only: no system reads it, so it never changes what the sim
//! does. (A chat may leave a memory in the citizen, deliberately, through
//! `Citizen::remember`; that is the only way a chat touches the sim.)
//! Bounded on every side: lines per person, people kept, characters per
//! line.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::CitizenId;

/// Lines kept per person (oldest dropped).
pub const LINES_PER_PERSON: usize = 16;
/// People whose chats are kept (the longest-quiet dropped).
pub const PEOPLE_KEPT: usize = 40;
/// Characters kept of one line.
pub const LINE_MAX_CHARS: usize = 500;

/// One line of a chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TalkLine {
    pub tick: u64,
    /// The mayor said it (false: the citizen).
    pub mayor: bool,
    pub text: String,
    /// The offline stand-in wrote the citizen's line (no model).
    #[serde(default)]
    pub stand_in: bool,
}

/// Every chat, by citizen.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TalkLog {
    pub chats: BTreeMap<CitizenId, Vec<TalkLine>>,
}

impl TalkLog {
    pub fn lines(&self, id: CitizenId) -> &[TalkLine] {
        self.chats.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Add a line, keeping every cap.
    pub fn push(&mut self, id: CitizenId, line: TalkLine) {
        let mut line = line;
        line.text = clip_chars(line.text.trim(), LINE_MAX_CHARS);
        let v = self.chats.entry(id).or_default();
        v.push(line);
        if v.len() > LINES_PER_PERSON {
            let extra = v.len() - LINES_PER_PERSON;
            v.drain(..extra);
        }
        while self.chats.len() > PEOPLE_KEPT {
            let quiet = self.chats.iter().filter(|(k, _)| **k != id).min_by_key(|(_, l)| l.last().map_or(0, |x| x.tick)).map(|(k, _)| *k);
            match quiet {
                Some(k) => {
                    self.chats.remove(&k);
                }
                None => break,
            }
        }
    }

    pub fn clear(&mut self, id: CitizenId) {
        self.chats.remove(&id);
    }

    pub fn total_lines(&self) -> usize {
        self.chats.values().map(Vec::len).sum()
    }
}

/// At most `n` characters (on a character boundary), with "..." when cut.
pub fn clip_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let t: String = s.chars().take(n.saturating_sub(3)).collect();
    format!("{}...", t.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn ids(n: usize) -> Vec<CitizenId> {
        let mut m: SlotMap<CitizenId, ()> = SlotMap::with_key();
        (0..n).map(|_| m.insert(())).collect()
    }

    fn line(tick: u64, text: &str) -> TalkLine {
        TalkLine { tick, mayor: tick % 2 == 0, text: text.into(), stand_in: false }
    }

    #[test]
    fn a_chat_keeps_its_newest_lines_only() {
        let id = ids(1)[0];
        let mut log = TalkLog::default();
        for i in 0..(LINES_PER_PERSON as u64 + 5) {
            log.push(id, line(i, &format!("line {i}")));
        }
        let l = log.lines(id);
        assert_eq!(l.len(), LINES_PER_PERSON);
        assert_eq!(l.last().unwrap().text, format!("line {}", LINES_PER_PERSON + 4));
        assert_eq!(l[0].text, "line 5", "the oldest went first");
    }

    #[test]
    fn the_longest_quiet_chat_goes_when_too_many_are_kept() {
        let people = ids(PEOPLE_KEPT + 3);
        let mut log = TalkLog::default();
        for (i, id) in people.iter().enumerate() {
            log.push(*id, line(i as u64 * 10, "hello"));
        }
        assert_eq!(log.chats.len(), PEOPLE_KEPT);
        assert!(log.lines(people[0]).is_empty() && log.lines(people[2]).is_empty(), "the quietest went");
        assert_eq!(log.lines(*people.last().unwrap()).len(), 1, "the newest stays");
        assert!(log.total_lines() <= PEOPLE_KEPT * LINES_PER_PERSON);
    }

    #[test]
    fn a_long_line_is_clipped() {
        let id = ids(1)[0];
        let mut log = TalkLog::default();
        log.push(id, line(1, &"é".repeat(LINE_MAX_CHARS * 2)));
        let t = &log.lines(id)[0].text;
        assert!(t.chars().count() <= LINE_MAX_CHARS && t.ends_with("..."));
    }

    #[test]
    fn an_old_save_without_chats_loads() {
        let log: TalkLog = ron::from_str("()").unwrap();
        assert!(log.chats.is_empty());
    }
}
