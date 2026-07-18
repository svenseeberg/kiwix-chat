pub mod client;
pub mod parse;

pub use client::KiwixClient;

/// A single full-text search hit returned by `/search?format=xml`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SearchResult {
    pub title: String,
    /// ZIM name (the identifier used in `/content` and `/raw` URLs).
    pub zim_name: String,
    /// Path of the entry within the ZIM file.
    pub path: String,
    /// Plain-text snippet of the matching portion, if any.
    pub snippet: String,
}

/// A page of an article's plain text, produced by `read_article`.
///
/// Long articles are returned in chunks of at most `max_chars`; `next_offset` is
/// `Some(_)` when more text remains beyond this page.
#[derive(Debug, Clone, PartialEq)]
pub struct ArticlePage {
    /// The plain text for this page (at most `max_chars` characters).
    pub text: String,
    /// Total number of characters in the full plain-text article.
    pub total_chars: usize,
    /// Character offset at which this page begins.
    pub offset: usize,
    /// Offset to resume from on the next call, or `None` if this is the last page.
    pub next_offset: Option<usize>,
}

/// A book (ZIM file) entry discovered via the OPDS catalog.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Book {
    pub title: String,
    pub name: String,
    pub language: String,
    pub description: String,
    pub article_count: Option<u64>,
}
