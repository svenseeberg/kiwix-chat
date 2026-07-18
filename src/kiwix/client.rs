use std::time::Duration;

use anyhow::{Context, Result};

use super::parse::{parse_catalog_xml, parse_search_xml};
use super::{Book, SearchResult};

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
    /// The result is truncated to `max_chars` to keep it within the model's context budget.
    pub async fn read_article(
        &self,
        zim_name: &str,
        path: &str,
        max_chars: usize,
    ) -> Result<String> {
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
        let html = resp.text().await.context("reading article response")?;

        let text = html2text::from_read(html.as_bytes(), 100);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok(truncate_chars(&text, max_chars))
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

/// Truncate to at most `max_chars` characters on a char boundary, adding an ellipsis marker.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str(" […truncated]");
    out
}
