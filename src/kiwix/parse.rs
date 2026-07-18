use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;

use super::{Book, SearchResult};

/// Parse the RSS/OpenSearch XML returned by `/search?format=xml` into search results.
///
/// The document looks like:
/// ```xml
/// <rss><channel>
///   <item>
///     <title>Article Title</title>
///     <link>http://host/content/zimname/A/Article_Path</link>
///     <description>snippet with <b>highlight</b></description>
///     <book name="zimname" .../>
///   </item>
/// </channel></rss>
/// ```
pub fn parse_search_xml(xml: &str) -> Result<Vec<SearchResult>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut in_item = false;
    // The field currently being captured, and the nesting depth below its element
    // (so inline markup like <b> inside <description> doesn't stop the capture).
    let mut field: Option<Field> = None;
    let mut depth: i32 = 0;
    let mut cur = ItemBuilder::default();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = local_name(e.name().as_ref());
                if name == "item" {
                    in_item = true;
                    cur = ItemBuilder::default();
                } else if in_item {
                    match field {
                        Some(_) => depth += 1, // nested element inside a captured field
                        None => {
                            field = Field::from_name(&name);
                            depth = 0;
                            if name == "book" {
                                if let Some(v) = attr(&e, b"name") {
                                    cur.zim_name = v;
                                }
                            }
                        }
                    }
                }
            }
            // <book .../> is usually self-closing.
            Event::Empty(e) => {
                if in_item && local_name(e.name().as_ref()) == "book" {
                    if let Some(v) = attr(&e, b"name") {
                        cur.zim_name = v;
                    }
                }
            }
            Event::Text(t) if field.is_some() => {
                let text = t.unescape().unwrap_or_default();
                match field {
                    Some(Field::Title) => cur.title.push_str(&text),
                    Some(Field::Link) => cur.link.push_str(&text),
                    Some(Field::Description) => cur.description.push_str(&text),
                    _ => {}
                }
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref());
                if name == "item" {
                    in_item = false;
                    results.push(cur.build());
                    cur = ItemBuilder::default();
                    field = None;
                } else if field.is_some() {
                    if depth == 0 {
                        field = None;
                    } else {
                        depth -= 1;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(results)
}

/// Which `<item>` child element text is currently being captured.
#[derive(Clone, Copy)]
enum Field {
    Title,
    Link,
    Description,
}

impl Field {
    fn from_name(name: &str) -> Option<Field> {
        match name {
            "title" => Some(Field::Title),
            "link" => Some(Field::Link),
            "description" => Some(Field::Description),
            _ => None,
        }
    }
}

#[derive(Default)]
struct ItemBuilder {
    title: String,
    link: String,
    description: String,
    zim_name: String,
}

impl ItemBuilder {
    fn build(self) -> SearchResult {
        // Derive zim_name + path from the link when a <book> element was absent.
        let (zim_from_link, path) = split_content_url(&self.link);
        let zim_name = if self.zim_name.is_empty() {
            zim_from_link
        } else {
            self.zim_name
        };
        SearchResult {
            title: self.title,
            zim_name,
            path,
            snippet: strip_html(&self.description),
        }
    }
}

/// Split a `.../content/<zim>/<path>` URL into (zim_name, path).
fn split_content_url(link: &str) -> (String, String) {
    if let Some(idx) = link.find("/content/") {
        let rest = &link[idx + "/content/".len()..];
        if let Some(slash) = rest.find('/') {
            return (rest[..slash].to_string(), rest[slash + 1..].to_string());
        }
        return (rest.to_string(), String::new());
    }
    (String::new(), String::new())
}

/// Parse the OPDS (Atom) feed from `/catalog/v2/entries` into a list of books.
pub fn parse_catalog_xml(xml: &str) -> Result<Vec<Book>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut books = Vec::new();
    let mut in_entry = false;
    // Current captured field and nesting depth below it. `author`/`publisher` map to
    // `Skip` so their nested <name> element is not mistaken for the book name.
    let mut field: Option<CField> = None;
    let mut depth: i32 = 0;
    let mut cur = BookBuilder::default();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = local_name(e.name().as_ref());
                if name == "entry" {
                    in_entry = true;
                    cur = BookBuilder::default();
                } else if in_entry {
                    match field {
                        Some(_) => depth += 1,
                        None => {
                            field = CField::from_name(&name);
                            depth = 0;
                        }
                    }
                }
            }
            Event::Text(t) if field.is_some() => {
                let text = t.unescape().unwrap_or_default();
                match field {
                    Some(CField::Title) => cur.title.push_str(&text),
                    Some(CField::Name) => cur.name.push_str(&text),
                    Some(CField::Language) => cur.language.push_str(&text),
                    Some(CField::Description) => cur.description.push_str(&text),
                    Some(CField::ArticleCount) => {
                        cur.article_count = text.trim().parse().ok();
                    }
                    Some(CField::Skip) | None => {}
                }
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref());
                if name == "entry" {
                    in_entry = false;
                    books.push(cur.build());
                    cur = BookBuilder::default();
                    field = None;
                } else if field.is_some() {
                    if depth == 0 {
                        field = None;
                    } else {
                        depth -= 1;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(books)
}

/// Which `<entry>` child element text is currently being captured.
#[derive(Clone, Copy)]
enum CField {
    Title,
    Name,
    Language,
    Description,
    ArticleCount,
    /// A scope whose contents (e.g. `<author><name>…`) must be ignored.
    Skip,
}

impl CField {
    fn from_name(name: &str) -> Option<CField> {
        match name {
            "title" => Some(CField::Title),
            "name" => Some(CField::Name),
            "language" => Some(CField::Language),
            "summary" | "content" => Some(CField::Description),
            "articleCount" => Some(CField::ArticleCount),
            "author" | "publisher" => Some(CField::Skip),
            _ => None,
        }
    }
}

#[derive(Default)]
struct BookBuilder {
    title: String,
    name: String,
    language: String,
    description: String,
    article_count: Option<u64>,
}

impl BookBuilder {
    fn build(self) -> Book {
        Book {
            title: self.title,
            name: self.name,
            language: self.language,
            description: strip_html(&self.description),
            article_count: self.article_count,
        }
    }
}

/// Local (namespace-stripped) element name as an owned String.
fn local_name(qname: &[u8]) -> String {
    let s = qname;
    let local = match s.iter().position(|&b| b == b':') {
        Some(i) => &s[i + 1..],
        None => s,
    };
    String::from_utf8_lossy(local).into_owned()
}

/// Read an attribute value by (local) key from a start/empty tag.
fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        let k = a.key.as_ref();
        let local = match k.iter().position(|&b| b == b':') {
            Some(i) => &k[i + 1..],
            None => k,
        };
        if local == key {
            Some(String::from_utf8_lossy(&a.value).into_owned())
        } else {
            None
        }
    })
}

/// Very small HTML→text reduction for snippets/descriptions: drop tags, collapse whitespace.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Decode a few common entities and collapse whitespace.
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results() {
        let xml = r#"<?xml version="1.0"?>
        <rss version="2.0" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">
          <channel>
            <title>Search</title>
            <item>
              <title>Ada Lovelace</title>
              <link>http://localhost:8080/content/wikipedia_en_all/A/Ada_Lovelace</link>
              <description>An &lt;b&gt;English&lt;/b&gt; mathematician &amp; writer</description>
              <book title="Wikipedia" id="abc" name="wikipedia_en_all"/>
            </item>
            <item>
              <title>Charles Babbage</title>
              <link>http://localhost:8080/content/wikipedia_en_all/A/Charles_Babbage</link>
              <description>Inventor</description>
            </item>
          </channel>
        </rss>"#;
        let r = parse_search_xml(xml).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Ada Lovelace");
        assert_eq!(r[0].zim_name, "wikipedia_en_all");
        assert_eq!(r[0].path, "A/Ada_Lovelace");
        assert_eq!(r[0].snippet, "An English mathematician & writer");
        // Second item has no <book>, so zim/path come from the link.
        assert_eq!(r[1].zim_name, "wikipedia_en_all");
        assert_eq!(r[1].path, "A/Charles_Babbage");
    }

    #[test]
    fn parses_catalog_entries() {
        let xml = r#"<?xml version="1.0"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <entry>
            <title>Wikipedia (English)</title>
            <summary>The free encyclopedia</summary>
            <language>eng</language>
            <name>wikipedia_en_all_maxi</name>
            <articleCount>6543210</articleCount>
            <author><name>Wikipedia</name></author>
            <publisher><name>Kiwix</name></publisher>
          </entry>
        </feed>"#;
        let b = parse_catalog_xml(xml).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].title, "Wikipedia (English)");
        assert_eq!(b[0].name, "wikipedia_en_all_maxi");
        assert_eq!(b[0].language, "eng");
        assert_eq!(b[0].description, "The free encyclopedia");
        assert_eq!(b[0].article_count, Some(6543210));
    }

    #[test]
    fn strips_tags_and_entities() {
        assert_eq!(strip_html("<p>a &amp; b</p>"), "a & b");
    }
}
