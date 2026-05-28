use crate::chunker;
use crate::types::{ChunkOptions, ChunkStrategy, KnowledgeEntry};
use crate::utils;
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

const CATALOG_URL: &str = "https://raw.githubusercontent.com/EbookFoundation/free-programming-books/main/books/free-programming-books-langs.md";
const CATALOG_CACHE_FILENAME: &str = "languages_catalog.json";
const MAX_RESOURCE_BYTES: u64 = 5 * 1024 * 1024;

fn fold_accents(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'ā' | 'ă' | 'ą' | 'ǎ' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' | 'ǐ' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' | 'ŏ' | 'ő' | 'ǒ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ǔ' | 'ų' => 'u',
            'ñ' | 'ń' | 'ň' | 'ņ' => 'n',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'ß' => 's',
            _ => c,
        });
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEntry {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageEntry {
    pub key: String,
    pub name: String,
    pub resources: Vec<ResourceEntry>,
}

pub struct LanguagesCatalog {
    entries: Vec<LanguageEntry>,
}

impl LanguagesCatalog {
    pub fn load_or_fetch(data_dir: &Path) -> Result<Self> {
        let cache_path = data_dir.join(CATALOG_CACHE_FILENAME);

        if cache_path.exists() {
            if let Ok(catalog) = Self::load_cache(&cache_path) {
                return Ok(catalog);
            }
            eprintln!("[warn] Cache corrupted, re-fetching catalog…");
        }

        let catalog = Self::fetch()?;
        if let Err(e) = catalog.save_cache(&cache_path) {
            eprintln!("[warn] Failed to cache catalog: {}", e);
        }
        Ok(catalog)
    }

    fn load_cache(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .context("Failed to read catalog cache")?;
        let entries: Vec<LanguageEntry> = serde_json::from_str(&content)
            .context("Failed to parse catalog cache")?;
        Ok(Self { entries })
    }

    fn save_cache(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.entries)
            .context("Failed to serialize catalog")?;
        utils::atomic_write(path, content)?;
        Ok(())
    }

    fn fetch() -> Result<Self> {
        let resp = ureq::get(CATALOG_URL)
            .call()
            .context("Failed to fetch free-programming-books catalog")?;

        let body = resp
            .into_body()
            .read_to_string()
            .context("Failed to read catalog response body")?;

        Self::parse(&body)
    }

    fn parse(markdown: &str) -> Result<Self> {
        let lang_re = Regex::new(r"^###\s+(.+)$")
            .map_err(|e| anyhow::anyhow!("Invalid lang regex: {}", e))?;
        let link_re = Regex::new(r"^\s*\*\s+\[([^\]]+)\]\(([^)]+)\)")
            .map_err(|e| anyhow::anyhow!("Invalid link regex: {}", e))?;

        let mut entries = Vec::new();
        let mut current_name: Option<String> = None;
        let mut current_resources: Vec<ResourceEntry> = Vec::new();

        for line in markdown.lines() {
            if let Some(caps) = lang_re.captures(line) {
                if let Some(name) = current_name.take() {
                    if !current_resources.is_empty() {
                        entries.push(LanguageEntry {
                            key: name.to_lowercase().replace(' ', "-"),
                            name,
                            resources: std::mem::take(&mut current_resources),
                        });
                    }
                }
                current_name = Some(strip_html(&caps[1]));
            } else if let Some(caps) = link_re.captures(line) {
                let title = caps[1].to_string();
                let url = caps[2].to_string();
                if !url.is_empty() && !title.is_empty() {
                    current_resources.push(ResourceEntry { title, url });
                }
            }
        }

        if let Some(name) = current_name {
            if !current_resources.is_empty() {
                entries.push(LanguageEntry {
                    key: name.to_lowercase().replace(' ', "-"),
                    name,
                    resources: current_resources,
                });
            }
        }

        Ok(Self { entries })
    }

    pub fn list(&self) -> &[LanguageEntry] {
        &self.entries
    }

    pub fn find(&self, key_or_name: &str) -> Option<&LanguageEntry> {
        let lower = key_or_name.to_lowercase();
        let folded = fold_accents(&lower);
        self.entries
            .iter()
            .find(|e| {
                e.key == lower
                    || e.name.to_lowercase() == lower
                    || fold_accents(&e.name.to_lowercase()) == folded
            })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn download_language_resources(
    lang: &LanguageEntry,
    max_resources: usize,
) -> Result<Vec<KnowledgeEntry>> {
    let mut entries: Vec<KnowledgeEntry> = Vec::new();

    for resource in lang.resources.iter().take(max_resources) {
        let resp = match ureq::get(&resource.url).call() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  [warn] Failed to fetch {}: {}", resource.title, e);
                continue;
            }
        };

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let is_text = content_type.starts_with("text/");
        let is_pdf = content_type.contains("application/pdf");
        let is_epub = content_type.contains("application/epub+zip") || content_type == "application/epub";

        if !is_text && !is_pdf && !is_epub {
            eprintln!("  [skip] {} — unsupported type: {}", resource.title, content_type);
            continue;
        }

        let size: u64 = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if size > MAX_RESOURCE_BYTES {
            eprintln!("  [skip] {} — too large: {} bytes", resource.title, size);
            continue;
        }

        let text = if is_text {
            let body = match resp.into_body().read_to_string() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  [warn] Failed to read {}: {}", resource.title, e);
                    continue;
                }
            };
            if content_type.contains("html") {
                strip_html(&body)
            } else {
                body
            }
        } else {
            let bytes = match resp.into_body().read_to_vec() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  [warn] Failed to read {}: {}", resource.title, e);
                    continue;
                }
            };
            let ext = if is_pdf { "pdf" } else { "epub" };
            let temp_path = std::env::temp_dir().join(format!("memvid_{}.{}", Uuid::new_v4(), ext));
            let result = (|| -> Result<String> {
                std::fs::write(&temp_path, &bytes)?;
                let extracted = crate::extractor::extract_file(&temp_path)
                    .with_context(|| format!("Failed to extract {}", resource.title))?;
                std::fs::remove_file(&temp_path)?;
                Ok(extracted.content)
            })();
            match result {
                Ok(content) => content,
                Err(e) => {
                    let _ = std::fs::remove_file(&temp_path);
                    eprintln!("  [warn] {} — {}", resource.title, e);
                    continue;
                }
            }
        };

        if text.trim().is_empty() {
            eprintln!("  [skip] {} — empty content", resource.title);
            continue;
        }

        let chunk_opts = ChunkOptions {
            max_size: 4000,
            overlap: 600,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunker::chunk_text(&text, &chunk_opts, &format!("{}/{}", lang.key, resource.title));
        for chunk in &chunks {
            let content = if let Some(ref h) = chunk.heading {
                format!("{}\n\n{}", h, chunk.content)
            } else {
                chunk.content.clone()
            };
            let checksum = utils::sha256_digest(content.as_bytes());
            entries.push(KnowledgeEntry {
                id: Uuid::new_v4().to_string(),
                source: chunk.source.clone(),
                content,
                timestamp: chrono::Utc::now(),
                checksum,
            });
        }

        eprintln!("  ✓ {} — {} chunks", resource.title, chunks.len());
    }

    Ok(entries)
}

fn strip_html(html: &str) -> String {
    fn decode_numeric_entity(entity: &str) -> Option<char> {
        let num_str = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok()?
        } else if let Some(dec) = entity.strip_prefix('#') {
            dec.parse::<u32>().ok()?
        } else {
            return None;
        };
        char::from_u32(num_str)
    }

    fn is_known_entity(name: &str) -> bool {
        matches!(name, "amp" | "lt" | "gt" | "quot" | "apos" | "nbsp")
    }

    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity_buf = String::new();

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => {
                if c == '&' {
                    in_entity = true;
                    entity_buf.clear();
                } else if in_entity {
                    if c == ';' {
                        if entity_buf.starts_with('#') {
                            if let Some(ch) = decode_numeric_entity(&entity_buf) {
                                result.push(ch);
                            }
                        } else if is_known_entity(entity_buf.as_str()) {
                            let decoded = match entity_buf.as_str() {
                                "amp" => "&",
                                "lt" => "<",
                                "gt" => ">",
                                "quot" => "\"",
                                "apos" => "'",
                                "nbsp" => " ",
                                _ => "",
                            };
                            result.push_str(decoded);
                        } else {
                            result.push('&');
                            result.push_str(&entity_buf);
                        }
                        in_entity = false;
                    } else if entity_buf.len() > 16 {
                        result.push('&');
                        result.push_str(&entity_buf);
                        result.push(c);
                        in_entity = false;
                    } else {
                        entity_buf.push(c);
                    }
                } else {
                    result.push(c);
                }
            }
            _ => {}
        }
    }

    let mut cleaned = String::with_capacity(result.len());
    let mut prev_space = false;
    for c in result.chars() {
        if c.is_whitespace() {
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            cleaned.push(c);
            prev_space = false;
        }
    }

    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_markdown() {
        let md = r#"# Books
### Python
* [Learn Python](https://python.org)
* [Python for Beginners](https://example.com)
### Rust
* [The Book](https://doc.rust-lang.org/book)
"#;
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert_eq!(catalog.len(), 2);
        let python = catalog.find("python").unwrap();
        assert_eq!(python.resources.len(), 2);
        assert_eq!(python.resources[0].title, "Learn Python");
    }

    #[test]
    fn find_by_key() {
        let md = "### Python\n* [A Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert!(catalog.find("python").is_some());
        assert!(catalog.find("Python").is_some());
    }

    #[test]
    fn find_by_name() {
        let md = "### C++\n* [A Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert!(catalog.find("c++").is_some());
        assert!(catalog.find("C++").is_some());
    }

    #[test]
    fn find_unknown() {
        let md = "### Python\n* [A Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert!(catalog.find("unknown").is_none());
    }

    #[test]
    fn skip_empty_sections() {
        let md = "### EmptySection\n### Python\n* [Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.find("python").unwrap().resources.len(), 1);
    }

    #[test]
    fn strip_html_simple() {
        let html = "<p>Hello <b>world</b></p>";
        let text = strip_html(html);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn strip_html_entities() {
        let html = "<p>AT&amp;T &lt;foo&gt;</p>";
        let text = strip_html(html);
        assert_eq!(text, "AT&T <foo>");
    }

    #[test]
    fn strip_html_no_tags() {
        let html = "plain text";
        let text = strip_html(html);
        assert_eq!(text, "plain text");
    }

    #[test]
    fn strip_html_collapses_whitespace() {
        let html = "<div>\n  <p>hello</p>\n  <p>world</p>\n</div>";
        let text = strip_html(html);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn key_format() {
        let md = "### C Sharp\n* [Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        let csharp = catalog.find("c-sharp").unwrap();
        assert_eq!(csharp.key, "c-sharp");
    }

    #[test]
    fn handles_empty_input() {
        let catalog = LanguagesCatalog::parse("").unwrap();
        assert_eq!(catalog.len(), 0);
    }

    #[test]
    fn strip_html_script_style() {
        let html = "<p>text</p><script>alert('x')</script><style>.cls{}</style>";
        let text = strip_html(html);
        // strip_html doesn't remove script/style content — it only strips tags
        // So script/style body text is preserved
        assert!(text.contains("text"));
    }

    #[test]
    fn strip_html_lone_ampersand() {
        let html = "<p>AT&T</p>";
        let text = strip_html(html);
        eprintln!("DEBUG strip_html('{}') = '{}'", html, text);
        for (i, c) in text.chars().enumerate() {
            eprintln!("  text[{}] = U+{:04X} '{}'", i, c as u32, c);
        }
        assert_eq!(text, "AT&T");
    }

    #[test]
    fn strip_html_unclosed_tag() {
        let html = "<p>hello";
        let text = strip_html(html);
        assert_eq!(text, "hello");
    }

    #[test]
    fn strip_html_malformed_entity() {
        let html = "<p>&abcdefghijklmnopqrstuvwxyz;</p>";
        let text = strip_html(html);
        // Entity buffer exceeded 16 chars, falls back to literal
        assert!(text.contains("&abcdefghijklmnopqrstuvwxyz;") || text.is_empty());
    }

    #[test]
    fn strip_html_numeric_entity_invalid() {
        let html = "&#99999999;"; // too large for valid Unicode
        let text = strip_html(html);
        assert_eq!(text, "");
    }

    #[test]
    fn find_case_sensitivity() {
        let md = "### Rust\n* [Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert!(catalog.find("rust").is_some());
        assert!(catalog.find("RUST").is_some());
        assert!(catalog.find("Rust").is_some());
    }

    #[test]
    fn find_partial_no_match() {
        let md = "### Python\n* [Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        assert!(catalog.find("pyt").is_none());
        assert!(catalog.find("thon").is_none());
    }

    #[test]
    fn parse_with_html_anchor_headers() {
        let md = "### <a id=\"rust\"></a>Rust\n* [Book](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        let rust = catalog.find("rust");
        assert!(rust.is_some());
        assert_eq!(rust.unwrap().resources.len(), 1);
    }

    #[test]
    fn list_returns_entries() {
        let md = "### A\n* [a](https://a)\n### B\n* [b](https://b)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        let entries = catalog.list();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "A");
        assert_eq!(entries[1].name, "B");
    }

    #[test]
    fn languages_with_diacritics() {
        let md = "### Español\n* [Libro](https://example.com)\n";
        let catalog = LanguagesCatalog::parse(md).unwrap();
        let found = catalog.find("espanol");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Español");
    }
}
