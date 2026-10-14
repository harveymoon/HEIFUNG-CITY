//! [ask-committee] "Ask the committee": the mayor types a question between
//! meetings and each seated member decides whether to answer, in
//! character, from the same persona card and city report their meeting
//! turns read. The thread (`nd_sim::committee::Question`) is saved with the
//! city; the next meeting reads a digest of it.
//!
//! One request per member per question, one at a time: the model backends
//! answer on a single worker thread anyway, so asking in turn costs no
//! time and lets each member read what the others said before them (the
//! last `EARLIER_KEPT` replies, clipped). Requests go through the courier
//! like every turn, so the settings' timeout and token cap hold and a
//! failed call is answered by the stand-in. Nothing here blocks a frame.

use std::collections::HashMap;

use nd_sim::committee::questions::clip_words;
use nd_sim::committee::{mix, CityReport, Question};
use nd_sim::World;

use super::{Courier, Delivered, Heard, PersonaCard, Task, TurnReply, TurnRequest};

/// Question requests in flight at once.
pub const IN_FLIGHT: usize = 1;
/// Earlier replies a member reads, at most (the newest).
pub const EARLIER_KEPT: usize = 4;
/// Each earlier reply as read, characters.
pub const EARLIER_CHARS: usize = 280;
/// A question turn's reply cap.
pub const MAX_TOKENS: u32 = 250;

/// What a reply says: did they answer, and with what. A reply with no
/// usable JSON is read leniently: `"respond": false` anywhere is a decline;
/// a `"text"` field (even cut off) is the answer; plain prose is the
/// answer; anything else (a bare `{`, an empty object) is a decline.
pub fn parse_answer(reply: &TurnReply) -> (bool, String) {
    if let Some(j) = &reply.json {
        let respond = match j.get("respond") {
            Some(serde_json::Value::Bool(b)) => Some(*b),
            Some(serde_json::Value::String(s)) => Some(matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "y")),
            _ => None,
        };
        let text = j.get("text").or_else(|| j.get("answer")).and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
        return match respond {
            Some(false) => (false, String::new()),
            _ if text.is_empty() => (false, String::new()),
            _ => (true, text),
        };
    }
    let raw = reply.text.trim();
    let squashed: String = raw.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    if squashed.contains("\"respond\":false") {
        return (false, String::new());
    }
    if let Some(i) = raw.find("\"text\"") {
        let rest = raw[i + 6..].trim_start().trim_start_matches(':').trim_start();
        if let Some(body) = rest.strip_prefix('"') {
            let mut out = String::new();
            let mut esc = false;
            for c in body.chars() {
                match c {
                    _ if esc => {
                        out.push(match c {
                            'n' => ' ',
                            c => c,
                        });
                        esc = false;
                    }
                    '\\' => esc = true,
                    '"' => break,
                    c => out.push(c),
                }
            }
            let out = out.trim().to_string();
            return if out.is_empty() { (false, String::new()) } else { (true, out) };
        }
    }
    let looks_json = raw.starts_with('{') || raw.starts_with("```");
    if raw.is_empty() || looks_json {
        return (false, String::new());
    }
    (true, raw.trim_matches('"').trim().to_string())
}

/// The request for `member`'s turn on question `q`.
pub fn request(w: &World, report: &CityReport, q: &Question, member: u32) -> Option<TurnRequest> {
    let p = w.committee.member(member)?;
    let facts = report.scope_facts(p.scope, w.committee.baseline.as_ref());
    let replies: Vec<_> = q.answers().collect();
    let earlier = replies.iter().skip(replies.len().saturating_sub(EARLIER_KEPT)).map(|r| Heard { name: r.name.clone(), scope: r.scope.clone(), text: clip_words(&r.text, EARLIER_CHARS) }).collect::<Vec<_>>();
    let last = q.waiting().len() == 1 && replies.is_empty();
    Some(TurnRequest {
        speaker: Some(PersonaCard::of(p)),
        task: Task::Question { question: q.text.clone(), facts, earlier, last },
        city: report.city.clone(),
        day: w.committee_today(),
        report: report.to_text(),
        seed: mix(mix(w.config.seed, q.id as u64), member as u64),
        max_tokens: MAX_TOKENS,
    })
}

/// The questions in flight: request id to (question, member).
#[derive(Default)]
pub struct Asker {
    asks: HashMap<u64, (u32, u32)>,
    /// Replies filed (the window's and the API's counter).
    pub answered: u32,
}

impl Asker {
    /// Forget what is in flight (a new city or a load): threads still
    /// waiting are asked again by the next `pump`.
    pub fn reset(&mut self) {
        self.asks.clear();
    }

    /// (question, member) pairs being answered now.
    pub fn typing(&self) -> Vec<(u32, u32)> {
        let mut v: Vec<(u32, u32)> = self.asks.values().copied().collect();
        v.sort();
        v
    }

    pub fn busy(&self) -> bool {
        !self.asks.is_empty()
    }

    /// Ask the next member waiting, oldest thread first, if there is room.
    pub fn pump(&mut self, w: &mut World, courier: &mut Courier) {
        if self.asks.len() >= IN_FLIGHT {
            return;
        }
        let mut report: Option<CityReport> = None;
        loop {
            if self.asks.len() >= IN_FLIGHT {
                return;
            }
            let next = w.committee.questions.iter().find_map(|q| q.waiting().into_iter().find(|m| !self.asks.values().any(|a| *a == (q.id, *m))).map(|m| (q.id, m)));
            let Some((qid, member)) = next else { return };
            let report = report.get_or_insert_with(|| CityReport::build(w));
            let req = w.committee.question(qid).and_then(|q| request(w, report, q, member));
            match req {
                Some(req) => {
                    let id = courier.submit(req);
                    self.asks.insert(id, (qid, member));
                }
                // Not in the pool any more: nothing to ask; close their turn.
                None => {
                    w.committee.answer_question(qid, member, false, "", false);
                }
            }
        }
    }

    /// Hand a reply in; true if it was ours.
    pub fn take(&mut self, w: &mut World, d: &Delivered) -> bool {
        let Some((qid, member)) = self.asks.remove(&d.id) else { return false };
        let (respond, text) = parse_answer(&d.reply);
        if w.committee.answer_question(qid, member, respond, &text, d.fell_back.is_some()) {
            self.answered += 1;
        }
        true
    }

    /// Ask and collect until nothing waits (the stand-in answers at once;
    /// the remote API's `ask` with it). Returns deliveries that were not
    /// ours, for the caller to hand on.
    #[must_use = "deliveries that are not the asker's must be handed on"]
    pub fn run(&mut self, w: &mut World, courier: &mut Courier, max_rounds: usize) -> Vec<Delivered> {
        let mut others = Vec::new();
        for _ in 0..max_rounds {
            self.pump(w, courier);
            if !self.busy() {
                break;
            }
            for d in courier.poll() {
                if !self.take(w, &d) {
                    others.push(d);
                }
            }
        }
        others
    }
}
