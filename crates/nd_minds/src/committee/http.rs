//! [committee] The model connectors: a local model behind an
//! OpenAI-compatible HTTP API (Ollama, LM Studio, llama.cpp's server, vLLM)
//! and the Claude Messages API. Each runs on a worker thread of its own and
//! answers one turn at a time, so the game never waits on the network; a
//! failure comes back as an error the courier answers with the stand-in.
//! The API key is only ever put in a request header: never in an error,
//! a log line or `Debug` output.

use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::settings::{BackendChoice, CommitteeSettings};
use super::{system_prompt, user_prompt, CommitteeBackend, MockCommittee, Task, TurnReply, TurnRequest};

/// Anthropic API version header.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Beta for `fallbacks: "default"` (a declined turn re-runs on a fallback model).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Clone)]
pub enum Flavor {
    OpenAi { base_url: String, model: String, max_tokens: u32 },
    Claude { url: String, model: String, key: String, effort: String, fallbacks: bool, max_tokens: u32 },
}

impl std::fmt::Debug for Flavor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Flavor::OpenAi { base_url, model, .. } => write!(f, "OpenAi({base_url}, {model})"),
            Flavor::Claude { url, model, .. } => write!(f, "Claude({url}, {model}, key: <set>)"),
        }
    }
}

impl Flavor {
    pub fn label(&self) -> String {
        match self {
            Flavor::OpenAi { base_url, model, .. } => format!("local model {model} at {base_url}"),
            Flavor::Claude { model, .. } => format!("Claude API, {model}"),
        }
    }
}

pub struct HttpBackend {
    tx: Sender<(u64, TurnRequest)>,
    /// Behind a mutex only so the backend is `Sync` (a Bevy resource
    /// holds it); one thread ever polls.
    rx: Mutex<Receiver<(u64, Result<TurnReply, String>)>>,
    name: String,
    /// [fix/review-app] Turns the courier gave up on: the worker answers
    /// one at a time, so a queue behind a slow call would otherwise still
    /// send (and pay for) turns whose answers are thrown away.
    cancelled: Arc<Mutex<HashSet<u64>>>,
}

impl HttpBackend {
    pub fn new(flavor: Flavor, timeout: Duration) -> Self {
        let (tx, jobs) = channel::<(u64, TurnRequest)>();
        let (done, rx) = channel();
        let name = flavor.label();
        let cancelled: Arc<Mutex<HashSet<u64>>> = Arc::default();
        let skip = cancelled.clone();
        std::thread::Builder::new()
            .name("committee-http".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new().timeout(timeout).build();
                while let Ok((id, req)) = jobs.recv() {
                    if skip.lock().map(|mut c| c.remove(&id)).unwrap_or(false) {
                        continue;
                    }
                    let r = call(&agent, &flavor, &req);
                    if done.send((id, r)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn the committee's HTTP worker");
        HttpBackend { tx, rx: Mutex::new(rx), name, cancelled }
    }
}

impl CommitteeBackend for HttpBackend {
    fn submit(&mut self, id: u64, req: TurnRequest) {
        let _ = self.tx.send((id, req));
    }

    fn poll(&mut self) -> Vec<(u64, Result<TurnReply, String>)> {
        let out: Vec<(u64, Result<TurnReply, String>)> = self.rx.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        // Answered after all (cancelled while its call was in flight):
        // nothing left to skip.
        if !out.is_empty() {
            if let Ok(mut c) = self.cancelled.lock() {
                for (id, _) in &out {
                    c.remove(id);
                }
            }
        }
        out
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn cancel(&mut self, id: u64) {
        if let Ok(mut c) = self.cancelled.lock() {
            c.insert(id);
        }
    }
}

/// The flavour the settings ask for (None: the stand-in). Err when the
/// Claude API is chosen with no key.
pub fn flavor_of(s: &CommitteeSettings) -> Result<Option<Flavor>, String> {
    Ok(match s.backend {
        BackendChoice::Mock => None,
        BackendChoice::Local => {
            if s.local_url.trim().is_empty() || s.local_model.trim().is_empty() {
                return Err("the local model needs a URL and a model name".into());
            }
            Some(Flavor::OpenAi { base_url: s.local_url.trim().trim_end_matches('/').to_string(), model: s.local_model.trim().to_string(), max_tokens: s.max_tokens.max(64) })
        }
        BackendChoice::Claude => {
            let key = s.claude_key().ok_or("no Claude API key: set ANTHROPIC_API_KEY or paste a key in the settings")?;
            Some(Flavor::Claude {
                url: s.claude_url.trim().to_string(),
                model: s.claude_model.trim().to_string(),
                key,
                effort: s.claude_effort.trim().to_string(),
                fallbacks: s.claude_fallbacks,
                // Thinking counts against the cap; leave it room.
                max_tokens: (s.max_tokens * 4).max(1600),
            })
        }
    })
}

/// The backend the settings ask for. Err (with the stand-in to use
/// meanwhile) when they cannot be honoured.
pub fn backend_from(s: &CommitteeSettings) -> Result<Box<dyn CommitteeBackend>, String> {
    match flavor_of(s)? {
        None => Ok(Box::new(MockCommittee::default())),
        Some(f) => Ok(Box::new(HttpBackend::new(f, Duration::from_secs(s.timeout_s.max(5))))),
    }
}

/// "Test connection": one tiny request on a thread of its own; the answer
/// (or the error) arrives on the receiver.
pub fn spawn_test(s: &CommitteeSettings) -> Receiver<Result<String, String>> {
    let (tx, rx) = channel();
    let flavor = flavor_of(s);
    let timeout = Duration::from_secs(s.timeout_s.clamp(5, 60));
    std::thread::spawn(move || {
        let r = match flavor {
            Err(e) => Err(e),
            Ok(None) => Ok("the stand-in needs no connection".to_string()),
            Ok(Some(f)) => {
                let agent = ureq::AgentBuilder::new().timeout(timeout).build();
                let req = TurnRequest { speaker: None, task: Task::Ping, city: "Neon District".into(), day: 0, report: String::new(), seed: 0, max_tokens: 16 };
                let start = std::time::Instant::now();
                call(&agent, &f, &req).map(|r| format!("{} answered in {:.1} s: \"{}\"", f.label(), start.elapsed().as_secs_f32(), r.text.chars().take(60).collect::<String>()))
            }
        };
        let _ = tx.send(r);
    });
    rx
}

/// One request, blocking (on the worker thread).
pub fn call(agent: &ureq::Agent, flavor: &Flavor, req: &TurnRequest) -> Result<TurnReply, String> {
    let system = system_prompt(req);
    let user = user_prompt(req);
    let wants_json = req.task.wants_json();
    match flavor {
        Flavor::OpenAi { base_url, model, max_tokens } => {
            let body = json!({
                "model": model,
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
                "max_tokens": (*max_tokens).max(req.max_tokens),
                "temperature": 0.8,
                "stream": false,
            });
            let url = format!("{base_url}/chat/completions");
            let v = post(agent.post(&url).set("Content-Type", "application/json"), &body, "local model")?;
            let text = v.pointer("/choices/0/message/content").and_then(Value::as_str).ok_or_else(|| format!("local model: unexpected reply {}", clip(&v.to_string())))?;
            Ok(TurnReply::from_raw(&strip_think(text), wants_json))
        }
        Flavor::Claude { url, model, key, effort, fallbacks, max_tokens } => {
            let mut body = json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            });
            if !effort.is_empty() {
                body["output_config"] = json!({ "effort": effort });
            }
            let mut r = agent.post(url).set("Content-Type", "application/json").set("x-api-key", key).set("anthropic-version", ANTHROPIC_VERSION);
            if *fallbacks {
                body["fallbacks"] = json!("default");
                r = r.set("anthropic-beta", FALLBACK_BETA);
            }
            let v = post(r, &body, "Claude API")?;
            if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
                return Err("Claude API: the model declined this turn (refusal)".into());
            }
            let text: String = v
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("text")).filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
                .unwrap_or_default();
            if text.trim().is_empty() {
                let why = v.get("stop_reason").and_then(Value::as_str).unwrap_or("?");
                return Err(format!("Claude API: no text in the reply (stop_reason {why})"));
            }
            Ok(TurnReply::from_raw(&text, wants_json))
        }
    }
}

fn post(r: ureq::Request, body: &Value, who: &str) -> Result<Value, String> {
    match r.send_string(&body.to_string()) {
        Ok(resp) => {
            let text = resp.into_string().map_err(|e| format!("{who}: reading the reply failed: {e}"))?;
            serde_json::from_str(&text).map_err(|_| format!("{who}: the reply is not JSON: {}", clip(&text)))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string).or_else(|| v.get("error").and_then(Value::as_str).map(str::to_string))).unwrap_or_else(|| clip(&text));
            Err(format!("{who}: HTTP {code}: {msg}"))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("{who}: could not connect ({})", t.kind())),
    }
}

/// Reasoning models served locally wrap their thoughts in <think> tags.
fn strip_think(s: &str) -> String {
    match (s.find("<think>"), s.find("</think>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &s[..a], &s[b + "</think>".len()..]),
        _ => s.to_string(),
    }
}

fn clip(s: &str) -> String {
    let t: String = s.chars().take(160).collect();
    t
}
