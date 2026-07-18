pub mod run;
pub mod tools;

pub use run::{run_turn, AgentEvent};

/// How much article text (in characters) a single `read_article` call may return.
pub const ARTICLE_MAX_CHARS: usize = 8000;

/// Default number of search hits requested per `search_wikipedia` call.
pub const DEFAULT_SEARCH_LIMIT: usize = 8;

/// System prompt instructing the agent to ground answers in the local Kiwix library.
pub const SYSTEM_PROMPT: &str = "\
You are a helpful research assistant with access to a local, offline Wikipedia (Kiwix) library \
through tools. Answer the user's questions using ONLY information you retrieve from that library.

Guidelines:
- Use `search_wikipedia` to find relevant articles, then `read_article` to read the most \
  promising ones before answering. Use `list_books` if you are unsure what corpora or languages \
  are available.
- Prefer reading at least one article rather than answering from the search snippets alone.
- Base your answer strictly on the retrieved content. If the library does not contain enough \
  information, say so plainly rather than inventing facts.
- Cite the article titles you relied on at the end of your answer.
- Be concise and factual. Do not mention the tool mechanics unless asked.";
