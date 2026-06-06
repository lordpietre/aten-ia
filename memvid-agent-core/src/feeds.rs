use crate::types::FeedEntry;
use crate::web_fetcher::WebFetcher;
use anyhow::{Context, Result};
use feed_rs::parser;

pub fn fetch_feed(url: &str, fetcher: &mut WebFetcher) -> Result<Vec<FeedEntry>> {
    fetcher.throttle();

    let response = fetcher
        .agent
        .get(url)
        .call()
        .with_context(|| format!("Failed to fetch feed {}", url))?;

    let body = response
        .into_body()
        .read_to_string()
        .with_context(|| format!("Failed to read feed body from {}", url))?;

    parse_feed_xml(&body, url)
}

pub fn parse_feed_xml(xml: &str, source_url: &str) -> Result<Vec<FeedEntry>> {
    let feed = parser::parse(xml.as_bytes())
        .with_context(|| format!("Failed to parse feed XML from {}", source_url))?;

    let entries: Vec<FeedEntry> = feed
        .entries
        .into_iter()
        .filter_map(|entry| {
            let url = entry
                .links
                .iter()
                .find(|l| l.rel.as_deref() == Some("alternate"))
                .or_else(|| entry.links.first())
                .and_then(|l| {
                    let href = l.href.trim().to_string();
                    if href.is_empty() { None } else { Some(href) }
                })?;

            let title = entry
                .title
                .and_then(|t| t.content.trim().to_string().into())
                .unwrap_or_else(|| "Untitled".to_string());

            let published = entry.published.or(entry.updated);

            Some(FeedEntry {
                title,
                url,
                description: entry.summary.map(|s| s.content.trim().to_string()),
                published,
                source_feed: source_url.to_string(),
            })
        })
        .collect();

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rss_simple() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Test Feed</title>
    <item>
      <title>First Post</title>
      <link>https://example.com/1</link>
      <description>Description of first post</description>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
    </item>
    <item>
      <title>Second Post</title>
      <link>https://example.com/2</link>
    </item>
  </channel>
</rss>"#;
        let entries = parse_feed_xml(xml, "https://example.com/feed").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First Post");
        assert_eq!(entries[0].url, "https://example.com/1");
        assert_eq!(
            entries[0].description.as_deref(),
            Some("Description of first post")
        );
        assert!(entries[0].published.is_some());
        assert_eq!(entries[1].title, "Second Post");
        assert_eq!(entries[1].url, "https://example.com/2");
        assert_eq!(entries[1].source_feed, "https://example.com/feed");
    }

    #[test]
    fn parse_atom_simple() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Test Atom Feed</title>
  <entry>
    <title>Atom Entry</title>
    <link href="https://example.com/atom1" rel="alternate"/>
    <summary>Atom summary text</summary>
    <published>2024-06-15T10:00:00Z</published>
  </entry>
</feed>"#;
        let entries = parse_feed_xml(xml, "https://example.com/atom").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Atom Entry");
        assert_eq!(entries[0].url, "https://example.com/atom1");
        assert_eq!(entries[0].description.as_deref(), Some("Atom summary text"));
    }

    #[test]
    fn parse_invalid_xml() {
        let result = parse_feed_xml("not xml at all", "https://example.com/bad");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_feed() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel><title>Empty</title></channel>
</rss>"#;
        let entries = parse_feed_xml(xml, "https://example.com/empty").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_entry_no_link_skipped() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <item><title>No Link</title></item>
    <item><title>Has Link</title><link>https://example.com/ok</link></item>
  </channel>
</rss>"#;
        let entries = parse_feed_xml(xml, "https://example.com/f").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://example.com/ok");
    }
}
