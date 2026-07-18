use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{ARTICLE_MAX_CHARS, DEFAULT_SEARCH_LIMIT};
use crate::kiwix::KiwixClient;
use crate::llm::Tool;

/// JSON-schema tool definitions advertised to the model on every request.
pub fn tool_defs(default_lang: &str) -> Vec<Tool> {
    vec![
        Tool::function(
            "search_wikipedia",
            "Full-text search the local Wikipedia/Kiwix library. Returns a list of matching \
             articles with their zim_name, path, and a short snippet. Use this to find articles \
             relevant to the user's question before reading them.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search terms."
                    },
                    "lang": {
                        "type": "string",
                        "description": format!(
                            "3-letter language code to scope the search (default: {default_lang})."
                        )
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (1-140).",
                        "minimum": 1,
                        "maximum": 140
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "read_article",
            "Fetch the full plain-text content of a specific article, identified by the zim_name \
             and path returned from search_wikipedia. Use this to read an article before citing it.",
            json!({
                "type": "object",
                "properties": {
                    "zim_name": {
                        "type": "string",
                        "description": "The ZIM name of the book containing the article."
                    },
                    "path": {
                        "type": "string",
                        "description": "The article path within the ZIM file (e.g. 'A/Ada_Lovelace')."
                    }
                },
                "required": ["zim_name", "path"]
            }),
        ),
        Tool::function(
            "list_books",
            "List the available books (ZIM files) in the local Kiwix library, including their \
             name, language, and article count. Use this to discover which corpora exist and which \
             zim_name or language to search.",
            json!({ "type": "object", "properties": {} }),
        ),
    ]
}

/// Outcome of a tool call: the string returned to the model plus a short human summary for the UI.
pub struct ToolOutcome {
    pub content: String,
    pub summary: String,
}

/// Execute a tool call by name against the Kiwix client.
pub async fn dispatch(
    kiwix: &KiwixClient,
    default_lang: &str,
    name: &str,
    arguments: &str,
) -> Result<ToolOutcome> {
    match name {
        "search_wikipedia" => search(kiwix, default_lang, arguments).await,
        "read_article" => read(kiwix, arguments).await,
        "list_books" => list_books(kiwix).await,
        other => Ok(ToolOutcome {
            content: format!("Error: unknown tool '{other}'."),
            summary: format!("Unknown tool '{other}'"),
        }),
    }
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    lang: Option<String>,
    limit: Option<usize>,
}

async fn search(kiwix: &KiwixClient, default_lang: &str, arguments: &str) -> Result<ToolOutcome> {
    let args: SearchArgs = parse_args(arguments)?;
    let lang = args.lang.unwrap_or_else(|| default_lang.to_string());
    let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);

    let results = kiwix.search(&args.query, &lang, limit).await?;
    let summary = format!(
        "Searched Wikipedia for \"{}\" ({} result{})",
        args.query,
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    if results.is_empty() {
        return Ok(ToolOutcome {
            content: format!(
                "No results found for \"{}\" in language '{lang}'.",
                args.query
            ),
            summary,
        });
    }

    let items: Vec<Value> = results
        .iter()
        .map(|r| {
            json!({
                "title": r.title,
                "zim_name": r.zim_name,
                "path": r.path,
                "snippet": r.snippet,
            })
        })
        .collect();
    Ok(ToolOutcome {
        content: serde_json::to_string_pretty(&json!({ "results": items }))?,
        summary,
    })
}

#[derive(Deserialize)]
struct ReadArgs {
    zim_name: String,
    path: String,
}

async fn read(kiwix: &KiwixClient, arguments: &str) -> Result<ToolOutcome> {
    let args: ReadArgs = parse_args(arguments)?;
    let text = kiwix
        .read_article(&args.zim_name, &args.path, ARTICLE_MAX_CHARS)
        .await?;
    let summary = format!("Read '{}' ({} chars)", args.path, text.chars().count());
    Ok(ToolOutcome {
        content: json!({
            "zim_name": args.zim_name,
            "path": args.path,
            "text": text,
        })
        .to_string(),
        summary,
    })
}

async fn list_books(kiwix: &KiwixClient) -> Result<ToolOutcome> {
    let books = kiwix.list_books().await?;
    let summary = format!("Listed {} book(s)", books.len());
    let items: Vec<Value> = books
        .iter()
        .map(|b| {
            json!({
                "name": b.name,
                "title": b.title,
                "language": b.language,
                "article_count": b.article_count,
                "description": b.description,
            })
        })
        .collect();
    Ok(ToolOutcome {
        content: serde_json::to_string_pretty(&json!({ "books": items }))?,
        summary,
    })
}

/// Parse tool-call JSON arguments, tolerating an empty string as `{}`.
fn parse_args<T: for<'de> Deserialize<'de>>(arguments: &str) -> Result<T> {
    let trimmed = arguments.trim();
    let src = if trimmed.is_empty() { "{}" } else { trimmed };
    Ok(serde_json::from_str(src)?)
}
