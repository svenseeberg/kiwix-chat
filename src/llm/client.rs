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

    /// Stream a chat completion. Assistant text fragments are delivered via `on_token`.
    /// Returns the fully assembled assistant message (text and/or tool calls).
    pub async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[Tool],
        mut on_token: impl FnMut(&str),
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
                    return Ok(assemble(content, acc.finish()));
                }
                let delta: StreamChunk = match serde_json::from_str(data) {
                    Ok(d) => d,
                    Err(_) => continue, // ignore keep-alives / non-JSON lines
                };
                if let Some(choice) = delta.choices.into_iter().next() {
                    if let Some(text) = choice.delta.content {
                        if !text.is_empty() {
                            on_token(&text);
                            content.push_str(&text);
                        }
                    }
                    for tc in choice.delta.tool_calls {
                        acc.push(tc);
                    }
                }
            }
        }
        Ok(assemble(content, acc.finish()))
    }
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
