use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;

use super::subagent_system_prompt;
use super::tools::{dispatch, parse_args, tool_defs, ResearchArgs};
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
    /// A `research` sub-agent finished; carries the question and its full answer
    /// for display as a collapsible block (kept out of the streamed transcript).
    SubagentAnswer { question: String, answer: String },
    /// Token usage reported by the backend for the latest request.
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    /// The turn completed successfully.
    Done,
    /// The turn was cancelled by the user before it finished.
    Interrupted,
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
    mut cancel: watch::Receiver<bool>,
) {
    let tools = tool_defs(lang, true);

    for round in 0..max_rounds {
        // Cancelled between rounds: history is coherent (any tool calls are already
        // answered), so just stop here.
        if *cancel.borrow() {
            let _ = tx.send(AgentEvent::Interrupted);
            return;
        }

        let assistant = match llm
            .stream_chat(
                messages,
                &tools,
                &mut cancel,
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

        // Cancelled mid-stream: commit only the partial answer text (never the
        // possibly-incomplete tool calls) so the conversation stays valid.
        if *cancel.borrow() {
            let partial = assistant.content.filter(|c| !c.trim().is_empty());
            messages.push(ChatMessage::assistant(
                partial.unwrap_or_else(|| "(interrupted)".to_string()),
            ));
            let _ = tx.send(AgentEvent::Interrupted);
            return;
        }

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

        // Execute every requested tool before re-checking cancellation: all calls
        // in this round must be answered to keep the conversation valid. A pending
        // cancel is picked up at the top of the next round.
        for call in calls {
            // `research` runs a nested, isolated agent so bulky article text stays
            // out of this conversation; every other tool goes through `dispatch`.
            let (content, summary) = if call.function.name == "research" {
                run_research(
                    llm,
                    kiwix,
                    lang,
                    max_rounds,
                    &call.function.arguments,
                    tx,
                    &cancel,
                )
                .await
            } else {
                match dispatch(kiwix, lang, &call.function.name, &call.function.arguments).await {
                    Ok(o) => (o.content, o.summary),
                    Err(e) => (
                        format!("Error running {}: {e:#}", call.function.name),
                        format!("{} failed: {e}", call.function.name),
                    ),
                }
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
            &mut cancel,
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
            let _ = tx.send(if *cancel.borrow() {
                AgentEvent::Interrupted
            } else {
                AgentEvent::Done
            });
        }
        Err(e) => {
            let _ = tx.send(AgentEvent::Error(format!("LLM error: {e:#}")));
        }
    }
}

/// Handle a `research` tool call: parse its arguments, run the sub-agent, emit the
/// answer for display, and return the `(tool_result content, status summary)` the
/// parent loop stores back into the main conversation.
async fn run_research(
    llm: &LlmClient,
    kiwix: &KiwixClient,
    lang: &str,
    max_rounds: usize,
    arguments: &str,
    tx: &UnboundedSender<AgentEvent>,
    cancel: &watch::Receiver<bool>,
) -> (String, String) {
    let args: ResearchArgs = match parse_args(arguments) {
        Ok(a) => a,
        Err(e) => {
            return (
                format!("Error running research: invalid arguments: {e}"),
                format!("research failed: {e}"),
            );
        }
    };
    let sub_lang = args.lang.as_deref().unwrap_or(lang);

    let _ = tx.send(AgentEvent::ToolFinished {
        summary: format!("↳ researching: \"{}\"", args.question),
    });

    let answer = run_subagent(llm, kiwix, sub_lang, max_rounds, &args.question, tx, cancel).await;

    // Surface the full answer as a collapsible block; the tool_result carries the
    // same text back to the parent agent.
    let _ = tx.send(AgentEvent::SubagentAnswer {
        question: args.question.clone(),
        answer: answer.clone(),
    });

    (answer, format!("↳ researched: \"{}\"", args.question))
}

/// Run an isolated sub-agent to answer a single question. Its searches and article
/// reads live in a throwaway message history, so they never enter the parent
/// conversation; only tool-activity summaries (prefixed with `↳`) are surfaced,
/// and the final answer text is returned to the caller.
async fn run_subagent(
    llm: &LlmClient,
    kiwix: &KiwixClient,
    lang: &str,
    max_rounds: usize,
    question: &str,
    tx: &UnboundedSender<AgentEvent>,
    cancel: &watch::Receiver<bool>,
) -> String {
    let mut cancel = cancel.clone();
    // No `research` tool here: a sub-agent must not spawn further sub-agents.
    let tools = tool_defs(lang, false);
    let mut messages = vec![
        ChatMessage::system(subagent_system_prompt(kiwix.base())),
        ChatMessage::user(question),
    ];

    let mut last_answer = String::new();

    for round in 0..max_rounds {
        if *cancel.borrow() {
            return interrupted_answer(last_answer);
        }

        // Reasoning, tokens and usage are display-only for the parent; the sub-agent
        // captures its answer from the assembled message instead.
        let assistant = match llm
            .stream_chat(&messages, &tools, &mut cancel, |_| {}, |_| {}, |_, _| {})
            .await
        {
            Ok(m) => m,
            Err(e) => return format!("Research failed: LLM error: {e:#}"),
        };

        if let Some(text) = assistant.content.as_ref().filter(|c| !c.trim().is_empty()) {
            last_answer = text.clone();
        }

        if *cancel.borrow() {
            return interrupted_answer(last_answer);
        }

        let tool_calls = assistant.tool_calls.clone();
        messages.push(assistant);

        let Some(calls) = tool_calls.filter(|c| !c.is_empty()) else {
            // No tool calls: the streamed text is the sub-agent's final answer.
            return last_answer;
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
            let _ = tx.send(AgentEvent::ToolFinished {
                summary: format!("↳ {summary}"),
            });
            messages.push(ChatMessage::tool_result(call.id, content));
        }
    }

    // Reached the round cap with tools still pending: ask for a tool-free answer.
    messages.push(ChatMessage::user(
        "Please provide your best final answer now using the information gathered above, \
         and end with the Sources section as instructed.",
    ));
    match llm
        .stream_chat(&messages, &[], &mut cancel, |_| {}, |_| {}, |_, _| {})
        .await
    {
        Ok(m) => m
            .content
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(last_answer),
        Err(e) => format!("Research failed: LLM error: {e:#}"),
    }
}

/// The best available answer when a sub-agent is interrupted mid-flight.
fn interrupted_answer(partial: String) -> String {
    if partial.trim().is_empty() {
        "(research interrupted before an answer was produced)".to_string()
    } else {
        partial
    }
}
