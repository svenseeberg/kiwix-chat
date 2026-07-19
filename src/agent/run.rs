use tokio::sync::mpsc::UnboundedSender;

use super::tools::{dispatch, tool_defs};
use crate::kiwix::KiwixClient;
use crate::llm::{ChatMessage, LlmClient};

/// Updates emitted by the agent loop for the UI to render.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A fragment of the assistant's answer text.
    Token(String),
    /// A fragment of the assistant's reasoning/thinking trace.
    Reasoning(String),
    /// A tool call has finished (dim status line).
    ToolFinished { summary: String },
    /// Token usage reported by the backend for the latest request.
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    /// The turn completed successfully.
    Done,
    /// The turn failed; carries a user-facing message.
    Error(String),
}

/// Run a single user turn to completion: stream the model, execute any tool calls,
/// and loop up to `max_rounds` times until the model produces a final answer.
///
/// `messages` is the full conversation (including the just-added user message); this
/// function appends assistant and tool messages to it so history persists across turns.
pub async fn run_turn(
    llm: &LlmClient,
    kiwix: &KiwixClient,
    lang: &str,
    max_rounds: usize,
    messages: &mut Vec<ChatMessage>,
    tx: &UnboundedSender<AgentEvent>,
) {
    let tools = tool_defs(lang);

    for round in 0..max_rounds {
        let assistant = match llm
            .stream_chat(
                messages,
                &tools,
                |t| {
                    let _ = tx.send(AgentEvent::Token(t.to_string()));
                },
                |r| {
                    let _ = tx.send(AgentEvent::Reasoning(r.to_string()));
                },
                |prompt_tokens, completion_tokens| {
                    let _ = tx.send(AgentEvent::Usage {
                        prompt_tokens,
                        completion_tokens,
                    });
                },
            )
            .await
        {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(AgentEvent::Error(format!("LLM error: {e:#}")));
                return;
            }
        };

        let tool_calls = assistant.tool_calls.clone();
        messages.push(assistant);

        let Some(calls) = tool_calls.filter(|c| !c.is_empty()) else {
            // No tool calls: the streamed text is the final answer.
            let _ = tx.send(AgentEvent::Done);
            return;
        };

        // On the final permitted round, don't execute more tools — force a wrap-up.
        if round + 1 == max_rounds {
            break;
        }

        for call in calls {
            let (content, summary) =
                match dispatch(kiwix, lang, &call.function.name, &call.function.arguments).await {
                    Ok(o) => (o.content, o.summary),
                    Err(e) => (
                        format!("Error running {}: {e:#}", call.function.name),
                        format!("{} failed: {e}", call.function.name),
                    ),
                };

            let _ = tx.send(AgentEvent::ToolFinished { summary });
            messages.push(ChatMessage::tool_result(call.id, content));
        }
    }

    // Reached the round cap with tool calls still pending: ask for a final answer
    // without tools so the user still gets a response.
    let _ = tx.send(AgentEvent::ToolFinished {
        summary: "Reached tool-call limit; composing an answer from gathered context".to_string(),
    });
    messages.push(ChatMessage::user(
        "Please provide your best final answer now using the information gathered above, \
         and cite the article titles you used.",
    ));
    match llm
        .stream_chat(
            messages,
            &[],
            |t| {
                let _ = tx.send(AgentEvent::Token(t.to_string()));
            },
            |r| {
                let _ = tx.send(AgentEvent::Reasoning(r.to_string()));
            },
            |prompt_tokens, completion_tokens| {
                let _ = tx.send(AgentEvent::Usage {
                    prompt_tokens,
                    completion_tokens,
                });
            },
        )
        .await
    {
        Ok(m) => {
            messages.push(m);
            let _ = tx.send(AgentEvent::Done);
        }
        Err(e) => {
            let _ = tx.send(AgentEvent::Error(format!("LLM error: {e:#}")));
        }
    }
}
