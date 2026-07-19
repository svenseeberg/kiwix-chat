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

/// A contiguous block of article lines around one or more grep matches.
///
/// Overlapping or adjacent context windows are merged into a single block so
/// nearby matches don't repeat lines.
#[derive(Debug, Clone, PartialEq)]
pub struct GrepBlock {
    /// 1-based line number of the first line in this block.
    pub start_line: usize,
    /// The lines of this block, in order.
    pub lines: Vec<String>,
}

/// Result of grepping an article: the matching lines with surrounding context.
#[derive(Debug, Clone, PartialEq)]
pub struct GrepArticleResult {
    /// Total number of lines in the full plain-text article.
    pub total_lines: usize,
    /// Number of lines that matched the query.
    pub match_count: usize,
    /// The context blocks around the matches, in document order.
    pub blocks: Vec<GrepBlock>,
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
