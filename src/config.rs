use clap::Parser;

/// Terminal chat UI backed by a local LLM agent and a local Kiwix (Wikipedia) server.
#[derive(Debug, Clone, Parser)]
#[command(name = "kiwix-chat", version, about)]
pub struct Cli {
    /// Base URL of the kiwix-serve instance (host + port, optional root prefix).
    #[arg(long, env = "KIWIX_URL", default_value = "http://localhost:8080")]
    pub kiwix_url: String,

    /// OpenAI-compatible LLM base URL. If unset, auto-detect llama.cpp (:8080) then Ollama (:11434).
    #[arg(long, env = "KIWIX_CHAT_LLM_URL")]
    pub llm_url: Option<String>,

    /// Model id to use. Defaults to the first model reported by /v1/models.
    #[arg(long, env = "KIWIX_CHAT_MODEL")]
    pub model: Option<String>,

    /// 3-letter language code used to scope Wikipedia searches.
    #[arg(long, env = "KIWIX_CHAT_LANG", default_value = "eng")]
    pub lang: String,

    /// Maximum number of agent tool-call rounds per user turn.
    #[arg(long, env = "KIWIX_CHAT_MAX_ROUNDS", default_value_t = 6)]
    pub max_rounds: usize,

    /// Write a debug log to kiwix-chat.log instead of stdout (which the TUI owns).
    #[arg(short, long)]
    pub verbose: bool,
}

/// Candidate OpenAI-compatible backends probed at startup, in priority order.
pub const LLM_AUTODETECT_URLS: &[&str] = &[
    "http://localhost:8080/v1",  // llama.cpp
    "http://localhost:11434/v1", // Ollama
];

/// Connection timeout for the startup LLM probe.
pub const LLM_PROBE_TIMEOUT_SECS: u64 = 3;

impl Cli {
    /// Normalized kiwix base URL without a trailing slash.
    pub fn kiwix_base(&self) -> String {
        self.kiwix_url.trim_end_matches('/').to_string()
    }
}
