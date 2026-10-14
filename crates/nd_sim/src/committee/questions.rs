//! [ask-committee] "Ask the committee": the mayor's questions between
//! meetings and what each member chose to say (or not). Saved with the
//! committee; the next meeting reads a short digest of the threads it has
//! not yet discussed, and marks them discussed. The answers themselves come
//! from a text backend (`nd_minds::committee::questions`).

use super::*;

/// Threads kept (the oldest discussed ones go first).
pub const QUESTIONS_KEPT: usize = 40;
/// A question's length cap, characters.
pub const QUESTION_MAX_CHARS: usize = 400;
/// An answer's length cap as stored, characters.
pub const ANSWER_MAX_CHARS: usize = 900;
/// Threads in a meeting's digest at most (the newest).
pub const DIGEST_THREADS: usize = 5;
/// Each answer in the digest, characters.
pub const DIGEST_ANSWER_CHARS: usize = 160;
/// The whole digest, characters.
pub const DIGEST_MAX_CHARS: usize = 1800;

/// One member's answer to a question: they spoke, or they chose not to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestionReply {
    pub member: u32,
    /// Their name when they answered (the pool keeps them, but a save
    /// should read on its own).
    pub name: String,
    /// Their scope's label then ("Housing").
    pub scope: String,
    /// False: they chose not to answer ("no comment").
    pub respond: bool,
    pub text: String,
    /// The stand-in answered for a model that failed or timed out.
    pub stand_in: bool,
}

/// A question the mayor put to the committee, and the replies so far.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Question {
    pub id: u32,
    pub day: u32,
    pub text: String,
    /// The members asked (seated when it was asked), in the order they
    /// answer.
    pub asked: Vec<u32>,
    /// In the order they came.
    pub replies: Vec<QuestionReply>,
    /// The meeting that read it (None: not discussed yet).
    pub discussed: Option<u32>,
}

impl Question {
    /// Members asked who have not answered (or declined) yet, in order.
    pub fn waiting(&self) -> Vec<u32> {
        self.asked.iter().copied().filter(|id| !self.replies.iter().any(|r| r.member == *id)).collect()
    }

    pub fn is_complete(&self) -> bool {
        self.waiting().is_empty()
    }

    pub fn answers(&self) -> impl Iterator<Item = &QuestionReply> {
        self.replies.iter().filter(|r| r.respond)
    }
}

/// `s` cut to `n` characters at a word, with "..." when cut.
pub fn clip_words(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        Some(i) if i > n / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}...", cut.trim_end_matches(|c: char| c.is_whitespace() || c == ',' || c == '.'))
}

impl Committee {
    /// Put a question to the seated members (in an order that rotates with
    /// the question, so the same member does not always speak first).
    /// Returns its id.
    pub fn ask(&mut self, text: &str, today: u32) -> Result<u32, String> {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err("the question is empty".into());
        }
        let text = clip_words(&text, QUESTION_MAX_CHARS);
        let mut asked = self.seated_ids();
        if asked.is_empty() {
            return Err("no members are seated: appoint some first".into());
        }
        let id = self.fresh_id();
        let n = asked.len();
        asked.rotate_left((mix(id as u64, 0x9a5e) % n as u64) as usize);
        self.questions.push(Question { id, day: today, text, asked, replies: Vec::new(), discussed: None });
        // Keep the log bounded: drop the oldest discussed threads first,
        // then the oldest of any.
        while self.questions.len() > QUESTIONS_KEPT {
            let i = self.questions.iter().position(|q| q.discussed.is_some()).unwrap_or(0);
            self.questions.remove(i);
        }
        Ok(id)
    }

    pub fn question(&self, id: u32) -> Option<&Question> {
        self.questions.iter().find(|q| q.id == id)
    }

    pub fn question_mut(&mut self, id: u32) -> Option<&mut Question> {
        self.questions.iter_mut().find(|q| q.id == id)
    }

    /// File a member's reply (once per member per question; a second is
    /// ignored). Returns whether it was filed.
    pub fn answer_question(&mut self, question: u32, member: u32, respond: bool, text: &str, stand_in: bool) -> bool {
        let (name, scope) = self.member(member).map_or_else(|| (format!("member {member}"), String::new()), |p| (p.name.clone(), p.scope.label().to_string()));
        let Some(q) = self.question_mut(question) else { return false };
        if !q.asked.contains(&member) || q.replies.iter().any(|r| r.member == member) {
            return false;
        }
        let text = clip_words(text, ANSWER_MAX_CHARS);
        let respond = respond && !text.is_empty();
        q.replies.push(QuestionReply { member, name, scope, respond, text, stand_in });
        true
    }

    /// Answered threads no meeting has read yet.
    pub fn undiscussed(&self) -> Vec<&Question> {
        self.questions.iter().filter(|q| q.discussed.is_none() && q.is_complete()).collect()
    }

    /// The digest the next meeting reads: the newest undiscussed answered
    /// threads (at most `DIGEST_THREADS`), each answer clipped, the whole
    /// bounded. Empty when there is none.
    pub fn questions_digest(&self) -> String {
        let all = self.undiscussed();
        if all.is_empty() {
            return String::new();
        }
        let shown = &all[all.len().saturating_sub(DIGEST_THREADS)..];
        let mut s = String::from("Questions the mayor put to the committee since the last meeting, and what members said:\n");
        if all.len() > shown.len() {
            s.push_str(&format!("({} older question(s) not listed.)\n", all.len() - shown.len()));
        }
        for q in shown {
            let mut line = format!("- Day {}, \"{}\": ", q.day + 1, clip_words(&q.text, 160));
            let said: Vec<String> = q.answers().map(|r| format!("{} ({}): \"{}\"", r.name, r.scope, clip_words(&r.text, DIGEST_ANSWER_CHARS))).collect();
            let quiet: Vec<&str> = q.replies.iter().filter(|r| !r.respond).map(|r| r.name.as_str()).collect();
            if said.is_empty() {
                line.push_str("nobody answered");
            } else {
                line.push_str(&said.join("; "));
            }
            if !quiet.is_empty() && !said.is_empty() {
                line.push_str(&format!("; no comment from {}", quiet.join(", ")));
            }
            line.push('\n');
            if s.chars().count() + line.chars().count() > DIGEST_MAX_CHARS {
                s.push_str("(more not listed.)\n");
                break;
            }
            s.push_str(&line);
        }
        s
    }

    /// Mark every answered undiscussed thread as read by `meeting`; returns
    /// their ids. A thread still being answered waits for the next one.
    pub fn mark_questions_discussed(&mut self, meeting: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for q in self.questions.iter_mut() {
            if q.discussed.is_none() && q.is_complete() {
                q.discussed = Some(meeting);
                out.push(q.id);
            }
        }
        out
    }
}
