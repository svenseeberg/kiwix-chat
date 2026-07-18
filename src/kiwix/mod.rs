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

/// A book (ZIM file) entry discovered via the OPDS catalog.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Book {
    pub title: String,
    pub name: String,
    pub language: String,
    pub description: String,
    pub article_count: Option<u64>,
}
