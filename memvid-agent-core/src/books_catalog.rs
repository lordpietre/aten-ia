use anyhow::{Context, Result};
use std::collections::HashMap;

const BOOKS_LANGS_URL: &str = "https://raw.githubusercontent.com/EbookFoundation/free-programming-books/main/books/free-programming-books-langs.md";

#[derive(Debug, Clone)]
pub struct BookResource {
    pub title: String,
    pub url: String,
    pub format: String, // HTML, PDF, EPUB, etc.
}

#[derive(Debug, Clone)]
pub struct LanguageBooks {
    pub language: String,
    pub resources: Vec<BookResource>,
}

pub struct BooksCatalog {
    languages: HashMap<String, Vec<BookResource>>,
}

impl BooksCatalog {
    /// Fetch and parse the languages books catalog from GitHub
    pub fn fetch() -> Result<Self> {
        eprintln!("📚 Fetching free programming books catalog...");
        let response = ureq::get(BOOKS_LANGS_URL)
            .call()
            .context("Failed to fetch books catalog")?;
        let content = response.into_body().read_to_string()?;
        let languages = Self::parse_markdown(&content)?;

        eprintln!("✓ Loaded {} programming languages", languages.len());

        Ok(Self { languages })
    }

    /// Parse the markdown content and extract languages and books
    fn parse_markdown(content: &str) -> Result<HashMap<String, Vec<BookResource>>> {
        let mut languages = HashMap::new();
        let mut current_language: Option<String> = None;
        let mut current_resources: Vec<BookResource> = Vec::new();

        for line in content.lines() {
            // Match section headers like "### Python" or "### C++"
            if line.starts_with("### ") {
                // Save previous language if exists
                if let Some(lang) = current_language.take() {
                    if !current_resources.is_empty() {
                        languages.insert(lang, current_resources.clone());
                        current_resources.clear();
                    }
                }

                let language = line
                    .trim_start_matches("### ")
                    .trim_start_matches("<a id=\"")
                    .split('"')
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !language.is_empty()
                    && !language.contains("Index")
                    && !language.contains("BY PROGRAMMING")
                {
                    current_language = Some(language);
                }
            }

            // Match resource links: * [Title](url)
            if current_language.is_some() {
                if line.trim_start_matches("* ").starts_with('[') {
                    if let Some(resource) = Self::parse_resource_line(line) {
                        current_resources.push(resource);
                    }
                }
            }
        }

        // Save last language
        if let Some(lang) = current_language {
            if !current_resources.is_empty() {
                languages.insert(lang, current_resources);
            }
        }

        Ok(languages)
    }

    /// Parse a single resource line: * [Title](url) - Description (Format)
    fn parse_resource_line(line: &str) -> Option<BookResource> {
        let line = line.trim_start_matches("* ").trim();

        // Extract title and URL from [Title](url)
        let title_end = line.find("](")? + 1;
        let title = line[1..title_end - 1].to_string();

        let url_start = title_end + 1;
        let url_end = line[url_start..].find(')')?;
        let url = line[url_start..url_start + url_end].to_string();

        // Extract format from parentheses at end: (PDF, HTML, EPUB, etc.)
        let url_close = url_start + url_end;
        let format = if let Some(last_paren) = line[url_close + 1..].rfind('(') {
            let paren_pos = url_close + 1 + last_paren;
            if let Some(end) = line[paren_pos..].find(')') {
                line[paren_pos + 1..paren_pos + end]
                    .split(',')
                    .next()
                    .unwrap_or("HTML")
                    .trim()
                    .to_string()
            } else {
                "HTML".to_string()
            }
        } else {
            "HTML".to_string()
        };

        Some(BookResource { title, url, format })
    }

    /// List all available languages
    pub fn list_languages(&self) -> Vec<&str> {
        let mut langs: Vec<&str> = self.languages.keys().map(|s| s.as_str()).collect();
        langs.sort();
        langs
    }

    /// Get books for a specific language
    pub fn get_language_books(&self, language: &str) -> Option<LanguageBooks> {
        self.languages.get(language).map(|resources| LanguageBooks {
            language: language.to_string(),
            resources: resources.clone(),
        })
    }

    /// Get books grouped by format for a language
    pub fn get_books_by_format(
        &self,
        language: &str,
    ) -> Result<HashMap<String, Vec<BookResource>>> {
        let books = self
            .get_language_books(language)
            .context(format!("Language '{}' not found", language))?;

        let mut by_format = HashMap::new();
        for resource in books.resources {
            by_format
                .entry(resource.format.clone())
                .or_insert_with(Vec::new)
                .push(resource);
        }

        Ok(by_format)
    }

    /// Search books by keyword
    pub fn search(&self, language: &str, keyword: &str) -> Vec<BookResource> {
        if let Some(books) = self.get_language_books(language) {
            books
                .resources
                .into_iter()
                .filter(|b| {
                    b.title.to_lowercase().contains(&keyword.to_lowercase())
                        || b.url.to_lowercase().contains(&keyword.to_lowercase())
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

/// Download books metadata and prepare for ingestion
pub fn prepare_knowledge_from_books(books: &LanguageBooks, limit: usize) -> String {
    let mut knowledge = format!(
        "# {} Programming Language - Free Books & Resources\n\n",
        books.language
    );

    knowledge.push_str("## Available Resources\n\n");

    for (idx, resource) in books.resources.iter().take(limit).enumerate() {
        knowledge.push_str(&format!(
            "{}. **{}** ({})\n   - URL: {}\n\n",
            idx + 1,
            resource.title,
            resource.format,
            resource.url
        ));
    }

    if books.resources.len() > limit {
        knowledge.push_str(&format!(
            "\n... and {} more resources available\n",
            books.resources.len() - limit
        ));
    }

    knowledge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_resource_line_html() {
        let line = "* [Learn Python](https://example.com/python.html) - Introduction (HTML)";
        let resource = BooksCatalog::parse_resource_line(line);
        assert!(resource.is_some());
        let r = resource.unwrap();
        assert_eq!(r.title, "Learn Python");
        assert_eq!(r.url, "https://example.com/python.html");
        assert_eq!(r.format, "HTML");
    }

    #[test]
    fn test_parse_resource_line_pdf() {
        let line = "* [Advanced Rust](https://example.com/rust.pdf) - Advanced concepts (PDF)";
        let resource = BooksCatalog::parse_resource_line(line);
        assert!(resource.is_some());
        let r = resource.unwrap();
        assert_eq!(r.title, "Advanced Rust");
        assert_eq!(r.format, "PDF");
    }

    #[test]
    fn test_prepare_knowledge() {
        let books = LanguageBooks {
            language: "Python".to_string(),
            resources: vec![
                BookResource {
                    title: "Learn Python".to_string(),
                    url: "https://example.com/python".to_string(),
                    format: "HTML".to_string(),
                },
                BookResource {
                    title: "Advanced Python".to_string(),
                    url: "https://example.com/advanced".to_string(),
                    format: "PDF".to_string(),
                },
            ],
        };

        let knowledge = prepare_knowledge_from_books(&books, 10);
        assert!(knowledge.contains("Python"));
        assert!(knowledge.contains("Learn Python"));
        assert!(knowledge.contains("Advanced Python"));
    }

    #[test]
    fn test_parse_resource_line_no_format() {
        let line = "* [No Format](https://example.com) - Just a description";
        let resource = BooksCatalog::parse_resource_line(line);
        assert!(resource.is_some());
        assert_eq!(resource.unwrap().format, "HTML");
    }

    #[test]
    fn test_parse_resource_line_empty_title() {
        let line = "* [](https://example.com) (PDF)";
        let resource = BooksCatalog::parse_resource_line(line);
        // Empty title [] is parsed as empty string, not filtered out
        assert!(resource.is_some());
        assert_eq!(resource.unwrap().title, "");
    }

    #[test]
    fn test_parse_resource_line_no_format_fallback() {
        let line = "* [No Format](https://example.com) - Just a description";
        let resource = BooksCatalog::parse_resource_line(line);
        assert!(resource.is_some());
        let r = resource.unwrap();
        // No trailing (Format) parens — defaults to HTML
        assert_eq!(r.format, "HTML");
    }

    #[test]
    fn test_parse_resource_line_malformed() {
        let line = "not a resource line";
        let resource = BooksCatalog::parse_resource_line(line);
        assert!(resource.is_none());
    }

    #[test]
    fn test_parse_markdown_empty() {
        let result = BooksCatalog::parse_markdown("");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_markdown_no_headers() {
        let result = BooksCatalog::parse_markdown("just some text\nwithout headers");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_markdown_filtered_headers() {
        let content = "### Index\ncontent\n### BY PROGRAMMING LANGUAGE\ncontent\n### Real Lang\n* [Book](url) (PDF)";
        let result = BooksCatalog::parse_markdown(content).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("Real Lang"));
    }

    #[test]
    fn test_prepare_knowledge_with_limit() {
        let books = LanguageBooks {
            language: "Rust".to_string(),
            resources: (0..5)
                .map(|i| BookResource {
                    title: format!("Book {}", i),
                    url: format!("https://example.com/{}", i),
                    format: "PDF".to_string(),
                })
                .collect(),
        };

        let knowledge = prepare_knowledge_from_books(&books, 3);
        assert!(knowledge.contains("Rust"));
        assert!(knowledge.contains("Book 0"));
        assert!(knowledge.contains("Book 2"));
        assert!(!knowledge.contains("Book 4"));
        assert!(knowledge.contains("and 2 more resources"));
    }

    #[test]
    fn test_prepare_knowledge_empty() {
        let books = LanguageBooks {
            language: "Empty".to_string(),
            resources: vec![],
        };
        let knowledge = prepare_knowledge_from_books(&books, 10);
        assert!(knowledge.contains("Empty"));
        assert!(knowledge.contains("Available Resources"));
    }

    #[test]
    fn test_parse_resource_line_html_anchor() {
        let line = r#"* <a id="some-section"></a>[Title](url) (PDF)"#;
        let resource = BooksCatalog::parse_resource_line(line);
        assert!(resource.is_some());
    }
}
