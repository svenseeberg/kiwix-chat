use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

use super::types::{ChatMessage, Role, Tool, ToolCall};

/// Client for an OpenAI-compatible chat-completions backend (llama.cpp, Ollama, …).
#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    /// Base URL including the `/v1` suffix, without a trailing slash.
    base: String,
    model: String,
}

impl LlmClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("kiwix-chat")
            .build()
            .context("building LLM HTTP client")?;
        Ok(Self {
            http,
            base: normalize_base(&base_url.into()),
            model: model.into(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Probe a candidate backend: return the list of model ids from `/v1/models`.
    ///
    /// Uses a short connect/read timeout so autodetection of a down server is fast.
    pub async fn probe_models(base_url: &str, timeout_secs: u64) -> Result<Vec<String>> {
        let base = normalize_base(base_url);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()?;
        let resp = http
            .get(format!("{base}/models"))
            .send()
            .await?
            .error_for_status()?;

        #[derive(Deserialize)]
        struct Models {
            data: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }
        let models: Models = resp.json().await.context("parsing /v1/models response")?;
        Ok(models.data.into_iter().map(|m| m.id).collect())
    }

    /// Stream a chat completion. Assistant answer fragments are delivered via
    /// `on_token`; reasoning/thinking fragments (from `reasoning_content`,
    /// `reasoning`, `thinking` delta fields or inline `<think>…</think>` tags) are
    /// delivered via `on_reasoning`. Reasoning is display-only and is not included
    /// in the assembled message returned to the caller.
    ///
    /// Returns the fully assembled assistant message (answer text and/or tool calls).
    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[Tool],
        mut on_token: impl FnMut(&str),
        mut on_reasoning: impl FnMut(&str),
    ) -> Result<ChatMessage> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
            body["tool_choice"] = json!("auto");
        }

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base))
            .json(&body)
            .send()
            .await
            .context("sending chat completion request")?
            .error_for_status()
            .context("chat completion returned an error status")?;

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut content = String::new();
        let mut acc = ToolCallAccumulator::default();
        let mut think = ThinkSplitter::default();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("reading stream chunk")?;
            buf.push_str(&String::from_utf8_lossy(&chunk));

            // SSE events are separated by newlines; process complete lines only.
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim().to_string();
                buf.drain(..=nl);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    think.flush(&mut on_token, &mut on_reasoning, &mut content);
                    return Ok(assemble(content, acc.finish()));
                }
                let delta: StreamChunk = match serde_json::from_str(data) {
                    Ok(d) => d,
                    Err(_) => continue, // ignore keep-alives / non-JSON lines
                };
                if let Some(choice) = delta.choices.into_iter().next() {
                    // Dedicated reasoning fields, if the backend exposes them.
                    for field in [
                        choice.delta.reasoning_content,
                        choice.delta.reasoning,
                        choice.delta.thinking,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if !field.is_empty() {
                            on_reasoning(&field);
                        }
                    }
                    // Answer text may embed inline <think>…</think> reasoning.
                    if let Some(text) = choice.delta.content {
                        think.feed(&text, &mut on_token, &mut on_reasoning, &mut content);
                    }
                    for tc in choice.delta.tool_calls {
                        acc.push(tc);
                    }
                }
            }
        }
        think.flush(&mut on_token, &mut on_reasoning, &mut content);
        Ok(assemble(content, acc.finish()))
    }
}

/// Splits streamed `content` into answer text and inline `<think>…</think>`
/// reasoning. State persists across chunks, and partial tags spanning a chunk
/// boundary are held back until they can be resolved.
#[derive(Default)]
struct ThinkSplitter {
    buf: String,
    in_think: bool,
}

impl ThinkSplitter {
    fn feed(
        &mut self,
        text: &str,
        on_token: &mut impl FnMut(&str),
        on_reasoning: &mut impl FnMut(&str),
        content: &mut String,
    ) {
        const OPEN: &str = "<think>";
        const CLOSE: &str = "</think>";
        self.buf.push_str(text);
        loop {
            if !self.in_think {
                if let Some(pos) = self.buf.find(OPEN) {
                    if pos > 0 {
                        let before = self.buf[..pos].to_string();
                        content.push_str(&before);
                        on_token(&before);
                    }
                    self.buf.drain(..pos + OPEN.len());
                    self.in_think = true;
                    continue;
                }
                // No open tag: emit everything except a possible partial tag tail.
                let safe = self.buf.len() - hold_back(&self.buf, OPEN);
                if safe > 0 {
                    let emit = self.buf[..safe].to_string();
                    content.push_str(&emit);
                    on_token(&emit);
                    self.buf.drain(..safe);
                }
                break;
            } else if let Some(pos) = self.buf.find(CLOSE) {
                if pos > 0 {
                    on_reasoning(&self.buf[..pos].to_string());
                }
                self.buf.drain(..pos + CLOSE.len());
                self.in_think = false;
                continue;
            } else {
                let safe = self.buf.len() - hold_back(&self.buf, CLOSE);
                if safe > 0 {
                    on_reasoning(&self.buf[..safe].to_string());
                    self.buf.drain(..safe);
                }
                break;
            }
        }
    }

    /// Emit any buffered remainder at end of stream (nothing more can arrive).
    fn flush(
        &mut self,
        on_token: &mut impl FnMut(&str),
        on_reasoning: &mut impl FnMut(&str),
        content: &mut String,
    ) {
        if self.buf.is_empty() {
            return;
        }
        let rest = std::mem::take(&mut self.buf);
        if self.in_think {
            on_reasoning(&rest);
        } else {
            content.push_str(&rest);
            on_token(&rest);
        }
    }
}

/// Length of the longest suffix of `buf` that is a (proper) prefix of `tag`,
/// i.e. how many trailing bytes must be held back in case a tag is still forming.
fn hold_back(buf: &str, tag: &str) -> usize {
    let b = buf.as_bytes();
    let t = tag.as_bytes();
    let max = (t.len() - 1).min(b.len());
    for k in (1..=max).rev() {
        if b[b.len() - k..] == t[..k] {
            return k;
        }
    }
    0
}

/// Ensure the base URL ends with `/v1` and has no trailing slash.
fn normalize_base(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn assemble(content: String, tool_calls: Vec<ToolCall>) -> ChatMessage {
    ChatMessage {
        role: Role::Assistant,
        content: (!content.is_empty()).then_some(content),
        tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
        tool_call_id: None,
    }
}

/// Reassembles streamed tool-call fragments keyed by their `index`.
#[derive(Default)]
struct ToolCallAccumulator {
    calls: Vec<ToolCall>,
}

impl ToolCallAccumulator {
    fn push(&mut self, delta: ToolCallDelta) {
        let idx = delta.index.unwrap_or(0);
        while self.calls.len() <= idx {
            self.calls.push(ToolCall::default());
        }
        let call = &mut self.calls[idx];
        if let Some(id) = delta.id {
            call.id = id;
        }
        if let Some(kind) = delta.kind {
            call.kind = kind;
        }
        if let Some(f) = delta.function {
            if let Some(name) = f.name {
                call.function.name.push_str(&name);
            }
            if let Some(args) = f.arguments {
                call.function.arguments.push_str(&args);
            }
        }
    }

    fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_iter()
            .filter(|c| !c.function.name.is_empty())
            .map(|mut c| {
                if c.kind.is_empty() {
                    c.kind = "function".to_string();
                }
                c
            })
            .collect()
    }
}

// --- Streaming response wire types (delta form) ---

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning trace (llama.cpp `--jinja`, DeepSeek-style).
    #[serde(default)]
    reasoning_content: Option<String>,
    /// Reasoning trace (some OpenAI-compatible servers).
    #[serde(default)]
    reasoning: Option<String>,
    /// Reasoning trace (Ollama).
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
