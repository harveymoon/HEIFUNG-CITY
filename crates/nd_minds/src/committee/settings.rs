//! [committee] Which backend the committee talks through, saved in
//! `committee_settings.ron` in the game's working directory. The Claude API
//! key comes from the `ANTHROPIC_API_KEY` environment variable or, failing
//! that, the settings file; it is never shown, logged or put in an error.

use serde::{Deserialize, Serialize};

pub const SETTINGS_FILE: &str = "committee_settings.ron";
pub const DEFAULT_CLAUDE_MODEL: &str = "claude-sonnet-5-5";
pub const DEFAULT_CLAUDE_URL: &str = "https://api.anthropic.com/v1/messages";
/// Ollama's OpenAI-compatible endpoint; LM Studio's is http://127.0.0.1:1234/v1.
pub const DEFAULT_LOCAL_URL: &str = "http://127.0.0.1:11434/v1";
pub const DEFAULT_LOCAL_MODEL: &str = "qwen2.5:14b";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendChoice {
    /// Templates and rules, offline.
    #[default]
    Mock,
    /// A local model behind an OpenAI-compatible HTTP API.
    Local,
    /// The Claude Messages API.
    Claude,
}

impl BackendChoice {
    pub const ALL: [BackendChoice; 3] = [BackendChoice::Mock, BackendChoice::Local, BackendChoice::Claude];

    pub fn key(self) -> &'static str {
        match self {
            BackendChoice::Mock => "mock",
            BackendChoice::Local => "local",
            BackendChoice::Claude => "claude",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BackendChoice::Mock => "Stand-in (offline)",
            BackendChoice::Local => "Local model",
            BackendChoice::Claude => "Claude API",
        }
    }

    pub fn parse(s: &str) -> Option<BackendChoice> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mock" | "stand-in" | "standin" | "offline" | "templates" => Some(BackendChoice::Mock),
            "local" | "ollama" | "lmstudio" | "lm studio" | "openai" => Some(BackendChoice::Local),
            "claude" | "anthropic" => Some(BackendChoice::Claude),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitteeSettings {
    pub backend: BackendChoice,
    /// Base URL of an OpenAI-compatible server (`.../v1`).
    pub local_url: String,
    pub local_model: String,
    pub claude_url: String,
    pub claude_model: String,
    /// Used only when `ANTHROPIC_API_KEY` is not set.
    pub claude_api_key: Option<String>,
    /// Effort for Claude (`low` keeps the committee cheap and quick).
    pub claude_effort: String,
    /// Ask the API to re-run a declined turn on a fallback model
    /// (`fallbacks: "default"`); turn off if a model rejects it.
    pub claude_fallbacks: bool,
    /// Reply cap for a local model; Claude gets more room, as its thinking
    /// counts against it.
    pub max_tokens: u32,
    /// Seconds before a turn is given to the stand-in.
    pub timeout_s: u64,
    /// [talk] Reply cap of one chat line with a citizen (Claude's thinking
    /// counts against it).
    pub talk_max_tokens: u32,
    /// [talk] Chat lines a day (UTC) sent to a model; past it the
    /// stand-in answers. 0: chats never use the model.
    pub talk_daily_budget: u32,
    /// [talk] A chat leaves a memory in the citizen ("Talked to the mayor
    /// about ..."), one a game day.
    pub talk_memories: bool,
}

impl Default for CommitteeSettings {
    fn default() -> Self {
        Self {
            backend: BackendChoice::Mock,
            local_url: DEFAULT_LOCAL_URL.into(),
            local_model: DEFAULT_LOCAL_MODEL.into(),
            claude_url: DEFAULT_CLAUDE_URL.into(),
            claude_model: DEFAULT_CLAUDE_MODEL.into(),
            claude_api_key: None,
            claude_effort: "low".into(),
            claude_fallbacks: true,
            max_tokens: 400,
            timeout_s: 90,
            talk_max_tokens: 600,
            talk_daily_budget: 60,
            talk_memories: true,
        }
    }
}

/// Never print the key.
impl std::fmt::Debug for CommitteeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitteeSettings")
            .field("backend", &self.backend)
            .field("local_url", &self.local_url)
            .field("local_model", &self.local_model)
            .field("claude_url", &self.claude_url)
            .field("claude_model", &self.claude_model)
            .field("claude_api_key", &self.claude_api_key.as_ref().map(|_| "<set>"))
            .field("claude_effort", &self.claude_effort)
            .field("claude_fallbacks", &self.claude_fallbacks)
            .field("max_tokens", &self.max_tokens)
            .field("timeout_s", &self.timeout_s)
            .field("talk_max_tokens", &self.talk_max_tokens)
            .field("talk_daily_budget", &self.talk_daily_budget)
            .field("talk_memories", &self.talk_memories)
            .finish()
    }
}

/// Where the key comes from, for the window (never the key itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Env,
    File,
    Missing,
}

impl KeySource {
    pub fn label(self) -> &'static str {
        match self {
            KeySource::Env => "from ANTHROPIC_API_KEY",
            KeySource::File => "from committee_settings.ron",
            KeySource::Missing => "missing: set ANTHROPIC_API_KEY or paste one below",
        }
    }
}

impl CommitteeSettings {
    pub fn load_from(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| ron::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn load() -> Self {
        Self::load_from(std::path::Path::new(SETTINGS_FILE))
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, Default::default()).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(std::path::Path::new(SETTINGS_FILE))
    }

    /// The Claude key: the environment first, then the file.
    pub fn claude_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| self.claude_api_key.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()))
    }

    pub fn key_source(&self) -> KeySource {
        if std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.trim().is_empty()) {
            KeySource::Env
        } else if self.claude_api_key.as_ref().is_some_and(|k| !k.trim().is_empty()) {
            KeySource::File
        } else {
            KeySource::Missing
        }
    }
}
