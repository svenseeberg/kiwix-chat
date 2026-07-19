use std::time::Duration;

use anyhow::{Context, Result};

use super::parse::{parse_catalog_xml, parse_search_xml};
use super::{ArticlePage, Book, GrepArticleResult, GrepBlock, SearchResult};

/// HTTP client for a local `kiwix-serve` instance.
#[derive(Clone)]
pub struct KiwixClient {
    http: reqwest::Client,
    base: String,
}

impl KiwixClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("kiwix-chat")
            .build()
            .context("building kiwix HTTP client")?;
        Ok(Self {
            http,
            base: base_url.into().trim_end_matches('/').to_string(),
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Lightweight reachability check against the OPDS catalog root.
    pub async fn is_reachable(&self) -> bool {
        let url = format!("{}/catalog/v2/root.xml", self.base);
        matches!(self.http.get(&url).send().await, Ok(r) if r.status().is_success())
    }

    /// Full-text search scoped to a language, returning parsed hits.
    pub async fn search(
        &self,
        pattern: &str,
        lang: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let limit = limit.clamp(1, 140);
        let url = format!("{}/search", self.base);
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("pattern", pattern),
                ("books.filter.lang", lang),
                ("pageLength", &limit.to_string()),
                ("format", "xml"),
            ])
            .send()
            .await
            .context("sending search request")?
            .error_for_status()
            .context("kiwix search returned an error status")?;
        let body = resp.text().await.context("reading search response")?;
        parse_search_xml(&body)
    }

    /// Fetch a single article via the public `/raw` endpoint and convert it to plain text.
    ///
    /// Returns the page of text starting at `offset` and spanning at most `max_chars`
    /// characters. Long articles are paginated to stay within the model's context
    /// budget; `ArticlePage::next_offset` indicates whether more text remains.
    pub async fn read_article(
        &self,
        zim_name: &str,
        path: &str,
        offset: usize,
        max_chars: usize,
    ) -> Result<ArticlePage> {
        let html = self.fetch_article_html(zim_name, path).await?;

        let text = html2text::from_read(html.as_bytes(), 100);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");

        let total_chars = text.chars().count();
        let offset = offset.min(total_chars);
        let page: String = text.chars().skip(offset).take(max_chars).collect();
        let end = offset + page.chars().count();
        let next_offset = if end < total_chars { Some(end) } else { None };

        Ok(ArticlePage {
            text: page,
            total_chars,
            offset,
            next_offset,
        })
    }

    /// Search within a single article for lines containing `query`, returning each
    /// matching line together with `context` lines before and after it.
    ///
    /// This is cheaper than `read_article` for locating a specific fact in a long
    /// article: only the relevant excerpts are returned instead of a full page of
    /// text. Matching is case-insensitive substring matching.
    pub async fn grep_article(
        &self,
        zim_name: &str,
        path: &str,
        query: &str,
        context: usize,
    ) -> Result<GrepArticleResult> {
        let html = self.fetch_article_html(zim_name, path).await?;
        // Keep line breaks (unlike read_article) so lines can be matched individually.
        let text = html2text::from_read(html.as_bytes(), 100);
        Ok(grep_lines(&text, query, context))
    }

    /// Fetch a single article's raw HTML via the public `/raw` endpoint.
    async fn fetch_article_html(&self, zim_name: &str, path: &str) -> Result<String> {
        let path = path.trim_start_matches('/');
        let url = format!("{}/raw/{}/content/{}", self.base, zim_name, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("sending article request")?
            .error_for_status()
            .with_context(|| format!("fetching article {zim_name}/{path}"))?;
        resp.text().await.context("reading article response")
    }

    /// List available books (ZIM files) from the OPDS catalog.
    pub async fn list_books(&self) -> Result<Vec<Book>> {
        let url = format!("{}/catalog/v2/entries", self.base);
        let resp = self
            .http
            .get(&url)
            .query(&[("count", "-1")])
            .send()
            .await
            .context("sending catalog request")?
            .error_for_status()
            .context("kiwix catalog returned an error status")?;
        let body = resp.text().await.context("reading catalog response")?;
        parse_catalog_xml(&body)
    }
}

/// Find lines of `text` containing `query` (case-insensitive substring) and
/// return them with `context` lines of surrounding context.
///
/// Overlapping or directly adjacent context windows are merged into a single
/// block so nearby matches don't duplicate lines.
fn grep_lines(text: &str, query: &str, context: usize) -> GrepArticleResult {
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();
    let needle = query.to_lowercase();

    // Empty query would match every line; treat it as no matches.
    let match_indices: Vec<usize> = if needle.is_empty() {
        Vec::new()
    } else {
        lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    };

    let mut blocks: Vec<GrepBlock> = Vec::new();
    for &i in &match_indices {
        let start = i.saturating_sub(context);
        let end = (i + context).min(total_lines.saturating_sub(1)); // inclusive

        // Merge into the previous block when the windows touch or overlap.
        if let Some(last) = blocks.last_mut() {
            let last_end = last.start_line - 1 + last.lines.len() - 1; // inclusive, 0-based
            if start <= last_end + 1 {
                for idx in (last_end + 1)..=end {
                    last.lines.push(lines[idx].to_string());
                }
                continue;
            }
        }

        blocks.push(GrepBlock {
            start_line: start + 1,
            lines: lines[start..=end].iter().map(|l| l.to_string()).collect(),
        });
    }

    GrepArticleResult {
        total_lines,
        match_count: match_indices.len(),
        blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
line one
line two has a Cat
line three
line four
line five
line six mentions a cat again
line seven";

    #[test]
    fn matches_with_context() {
        let r = grep_lines(SAMPLE, "cat", 1);
        assert_eq!(r.total_lines, 7);
        assert_eq!(r.match_count, 2);
        assert_eq!(r.blocks.len(), 2);
        assert_eq!(r.blocks[0].start_line, 1);
        assert_eq!(
            r.blocks[0].lines,
            vec!["line one", "line two has a Cat", "line three"]
        );
        assert_eq!(r.blocks[1].start_line, 5);
        assert_eq!(
            r.blocks[1].lines,
            vec!["line five", "line six mentions a cat again", "line seven"]
        );
    }

    #[test]
    fn case_insensitive() {
        let r = grep_lines(SAMPLE, "CAT", 0);
        assert_eq!(r.match_count, 2);
        assert_eq!(r.blocks[0].lines, vec!["line two has a Cat"]);
    }

    #[test]
    fn merges_adjacent_windows() {
        let text = "a\nMATCH one\nb\nc\nMATCH two\nd";
        let r = grep_lines(text, "MATCH", 2);
        assert_eq!(r.match_count, 2);
        // Windows [0..=3] and [2..=5] overlap -> single merged block of all 6 lines.
        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].start_line, 1);
        assert_eq!(r.blocks[0].lines.len(), 6);
    }

    #[test]
    fn clamps_at_edges() {
        let r = grep_lines(SAMPLE, "line one", 5);
        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].start_line, 1);
        // Context runs off the start (clamped) and 5 lines forward.
        assert_eq!(r.blocks[0].lines.len(), 6);
    }

    #[test]
    fn no_matches() {
        let r = grep_lines(SAMPLE, "elephant", 3);
        assert_eq!(r.match_count, 0);
        assert!(r.blocks.is_empty());
    }

    #[test]
    fn empty_query_matches_nothing() {
        let r = grep_lines(SAMPLE, "", 3);
        assert_eq!(r.match_count, 0);
        assert!(r.blocks.is_empty());
    }
}
