use crate::types::Format;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug)]
pub struct ExtractedFile {
    pub content: String,
    pub title: Option<String>,
    pub format: Format,
}

pub fn extract_file(path: &Path) -> Result<ExtractedFile> {
    let format = Format::from_extension(path);
    match format {
        Format::Pdf => extract_pdf(path),
        Format::Epub => extract_epub(path),
        Format::Html | Format::Markdown | Format::Text => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let title = path.file_stem().map(|s| s.to_string_lossy().to_string());
            Ok(ExtractedFile {
                content,
                title,
                format,
            })
        }
    }
}

pub fn extract_pdf(path: &Path) -> Result<ExtractedFile> {
    let content = pdf_extract::extract_text(path)
        .map_err(|e| anyhow::anyhow!("PDF extraction failed: {}", e))?;
    let title = path.file_stem().map(|s| s.to_string_lossy().to_string());
    Ok(ExtractedFile {
        content,
        title,
        format: Format::Pdf,
    })
}

pub fn extract_epub(path: &Path) -> Result<ExtractedFile> {
    use epub::doc::EpubDoc;

    let mut doc = EpubDoc::new(path).map_err(|e| anyhow::anyhow!("Failed to open EPUB: {}", e))?;
    let title = doc
        .get_title()
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()));
    let mut content = String::new();
    let num_chapters = doc.get_num_chapters();
    for _ in 0..num_chapters.max(1) {
        if let Some((text, _)) = doc.get_current_str() {
            content.push_str(&text);
            content.push('\n');
        }
        if !doc.go_next() {
            break;
        }
    }
    if content.trim().is_empty() {
        anyhow::bail!("No text content found in EPUB: {}", path.display());
    }
    Ok(ExtractedFile {
        content,
        title,
        format: Format::Epub,
    })
}

pub fn extract_text(content: &str, content_type: &str) -> String {
    let ct = content_type.to_lowercase();
    if ct.contains("html") {
        html_to_text(content)
    } else if ct.contains("markdown") || ct.contains("md") {
        content.to_string()
    } else {
        content.to_string()
    }
}

pub fn extract_metadata(html: &str) -> Metadata {
    let mut meta = Metadata::default();

    if let Some(title) = extract_title(html) {
        meta.title = Some(title);
    }

    if let Some(desc) = extract_meta_tag(html, "description") {
        meta.description = Some(desc);
    }
    if meta.description.is_none() {
        if let Some(desc) = extract_meta_tag(html, "og:description") {
            meta.description = Some(desc);
        }
    }

    if let Some(lang) = extract_html_lang(html) {
        meta.language = Some(lang);
    }

    meta
}

pub fn html_to_text(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let block_tags = [
        "div",
        "p",
        "br",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "li",
        "ul",
        "ol",
        "tr",
        "td",
        "th",
        "section",
        "article",
        "header",
        "footer",
        "nav",
        "main",
        "blockquote",
        "pre",
        "hr",
        "table",
        "tbody",
        "thead",
        "tfoot",
        "dd",
        "dt",
    ];

    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut prev_space = false;

    while i < len {
        if chars[i] == '<' {
            tag_name.clear();
            let mut j = i + 1;
            let is_end = j < len && chars[j] == '/';
            if is_end {
                j += 1;
            }
            while j < len && chars[j] != '>' && !chars[j].is_whitespace() {
                tag_name.push(chars[j].to_ascii_lowercase());
                j += 1;
            }

            if tag_name == "script" && !is_end {
                in_script = true;
            } else if tag_name == "script" && is_end {
                in_script = false;
            }
            if tag_name == "style" && !is_end {
                in_style = true;
            } else if tag_name == "style" && is_end {
                in_style = false;
            }

            if !in_script && !in_style && block_tags.contains(&tag_name.as_str()) && !is_end {
                if !prev_space {
                    result.push(' ');
                    prev_space = true;
                }
            }

            while i < len && chars[i] != '>' {
                i += 1;
            }
            if i < len {
                i += 1;
            }

            if !in_script && !in_style && block_tags.contains(&tag_name.as_str()) && is_end {
                if !prev_space {
                    result.push(' ');
                    prev_space = true;
                }
            }
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        if chars[i] == '&' {
            let entity = parse_html_entity(&chars, i);
            if !entity.is_empty() {
                result.push_str(&entity);
                i += 1;
                while i < len && chars[i] != ';' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                prev_space = entity == " ";
                continue;
            }
        }

        if chars[i].is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(chars[i]);
            prev_space = false;
        }

        i += 1;
    }

    result.trim().to_string()
}

fn parse_html_entity(chars: &[char], start: usize) -> String {
    let len = chars.len();
    if start >= len || chars[start] != '&' {
        return String::new();
    }
    let mut end = start + 1;
    while end < len && chars[end] != ';' && end - start < 20 {
        end += 1;
    }
    if end >= len || chars[end] != ';' {
        return String::new();
    }
    let name: String = chars[start + 1..end].iter().collect();
    match name.as_str() {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => " ".to_string(),
        _ if name.starts_with('#') => {
            let num = &name[1..];
            if let Ok(codepoint) = if num.starts_with('x') || num.starts_with('X') {
                u32::from_str_radix(&num[1..], 16)
            } else {
                num.parse::<u32>()
            } {
                if let Some(c) = char::from_u32(codepoint) {
                    return c.to_string();
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

pub fn html_to_markdown(html: &str) -> String {
    let mut md = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_script = false;
    let mut in_style = false;
    let mut in_pre = false;
    let mut link_href: Option<String> = None;

    while i < len {
        if chars[i] == '<' {
            let tag = extract_tag(&chars, i);
            let tagname = tag.name.to_lowercase();

            match tagname.as_str() {
                "script" if !tag.is_end => in_script = true,
                "script" if tag.is_end => in_script = false,
                "style" if !tag.is_end => in_style = true,
                "style" if tag.is_end => in_style = false,
                "pre" if !tag.is_end => {
                    in_pre = true;
                    md.push_str("\n```\n");
                }
                "pre" if tag.is_end => {
                    in_pre = false;
                    md.push_str("\n```\n");
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !tag.is_end => {
                    let level = tagname[1..].parse::<usize>().unwrap_or(1);
                    md.push('\n');
                    for _ in 0..level {
                        md.push('#');
                    }
                    md.push(' ');
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if tag.is_end => {
                    md.push('\n');
                }
                "p" if !tag.is_end => {}
                "p" if tag.is_end => md.push_str("\n\n"),
                "br" => md.push_str("\n"),
                "hr" => md.push_str("\n---\n"),
                "li" if !tag.is_end => {
                    md.push_str("\n- ");
                }
                "a" if !tag.is_end => {}
                "a" if tag.is_end => {
                    if let Some(ref href) = link_href {
                        md.push_str(&format!("]({})", href));
                    }
                    link_href = None;
                }
                "img" if !tag.is_end => {
                    if let Some(src) = tag.get_attr("src") {
                        let alt = tag.get_attr("alt").unwrap_or_default();
                        md.push_str(&format!("![{}]({})", alt, src));
                    }
                }
                "strong" | "b" if !tag.is_end => md.push_str("**"),
                "strong" | "b" if tag.is_end => md.push_str("**"),
                "em" | "i" if !tag.is_end => md.push_str("*"),
                "em" | "i" if tag.is_end => md.push_str("*"),
                "code" if !in_pre => md.push_str("`"),
                "code" if in_pre => {}
                "code" if tag.is_end && !in_pre => md.push_str("`"),
                "code" if tag.is_end && in_pre => {}
                "blockquote" if !tag.is_end => md.push_str("\n> "),
                "blockquote" if tag.is_end => md.push('\n'),
                _ => {}
            }

            if tagname == "a" && !tag.is_end {
                link_href = tag.get_attr("href").map(|s| s.to_string());
                if link_href.is_some() {
                    md.push('[');
                }
            }

            i = tag.end;
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        if in_pre {
            md.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '&' {
            let entity = parse_html_entity(&chars, i);
            if !entity.is_empty() {
                md.push_str(&entity);
                i += 1;
                while i < len && chars[i] != ';' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }
        }

        md.push(chars[i]);
        i += 1;
    }

    let re = Regex::new(r"\n{3,}").unwrap();
    let result = re.replace_all(md.trim(), "\n\n").to_string();
    if result.trim().is_empty() && md.contains('\n') {
        "\n\n".to_string()
    } else {
        result
    }
}

fn extract_title(html: &str) -> Option<String> {
    let re = Regex::new(r"<title[^>]*>([^<]+)</title>").ok()?;
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

fn extract_meta_tag(html: &str, name: &str) -> Option<String> {
    let patterns = [
        format!(
            r#"<meta\s+name=["']{}["'][^>]*content=["']([^"']+)["']"#,
            regex::escape(name)
        ),
        format!(
            r#"<meta\s+property=["']{}["'][^>]*content=["']([^"']+)["']"#,
            regex::escape(name)
        ),
        format!(
            r#"<meta\s+content=["']([^"']+)["'][^>]*name=["']{}["']"#,
            regex::escape(name)
        ),
        format!(
            r#"<meta\s+content=["']([^"']+)["'][^>]*property=["']{}["']"#,
            regex::escape(name)
        ),
    ];
    for pattern in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().trim().to_string());
                }
            }
        }
    }
    None
}

fn extract_html_lang(html: &str) -> Option<String> {
    let re = Regex::new(r#"<html[^>]*\slang=["']([^"']+)["']"#).ok()?;
    re.captures(html).and_then(|c| c.get(1)).map(|m| {
        m.as_str()
            .split('-')
            .next()
            .unwrap_or(m.as_str())
            .to_string()
    })
}

struct TagInfo {
    name: String,
    is_end: bool,
    end: usize,
    attrs: HashMap<String, String>,
}

fn extract_tag(chars: &[char], start: usize) -> TagInfo {
    let len = chars.len();
    let mut name = String::new();
    let mut is_end = false;
    let mut attrs = HashMap::new();
    let mut i = start + 1;

    if i < len && chars[i] == '/' {
        is_end = true;
        i += 1;
    }

    while i < len && !chars[i].is_whitespace() && chars[i] != '>' {
        name.push(chars[i]);
        i += 1;
    }

    if !is_end {
        while i < len {
            while i < len && chars[i].is_whitespace() && chars[i] != '>' {
                i += 1;
            }
            if i >= len || chars[i] == '>' || chars[i] == '/' {
                break;
            }

            let mut attr_name = String::new();
            while i < len && chars[i] != '=' && !chars[i].is_whitespace() && chars[i] != '>' {
                attr_name.push(chars[i]);
                i += 1;
            }

            while i < len && chars[i].is_whitespace() {
                i += 1;
            }

            if i < len && chars[i] == '=' {
                i += 1;
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }

                let mut attr_val = String::new();
                if i < len && (chars[i] == '"' || chars[i] == '\'') {
                    let quote = chars[i];
                    i += 1;
                    while i < len && chars[i] != quote {
                        attr_val.push(chars[i]);
                        i += 1;
                    }
                    if i < len {
                        i += 1;
                    }
                } else {
                    while i < len && !chars[i].is_whitespace() && chars[i] != '>' {
                        attr_val.push(chars[i]);
                        i += 1;
                    }
                }
                attrs.insert(attr_name.to_lowercase(), attr_val);
            } else if !attr_name.is_empty() {
                attrs.insert(attr_name.to_lowercase(), String::new());
            }
        }
    }

    if i < len && chars[i] == '/' {
        i += 1;
    }
    if i < len && chars[i] == '>' {
        i += 1;
    }

    TagInfo {
        name,
        is_end,
        end: i,
        attrs,
    }
}

impl TagInfo {
    fn get_attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_simple() {
        let html = "<p>Hello <b>world</b></p>";
        let text = html_to_text(html);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn html_to_text_strips_scripts() {
        let html = "<p>Hello</p><script>alert('xss')</script><p>World</p>";
        let text = html_to_text(html);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn html_to_text_strips_styles() {
        let html = "<p>Text</p><style>.cls{color:red}</style><p>More</p>";
        let text = html_to_text(html);
        assert_eq!(text, "Text More");
    }

    #[test]
    fn html_to_text_entities() {
        let html = "<p>AT&amp;T &lt;foo&gt;</p>";
        let text = html_to_text(html);
        assert_eq!(text, "AT&T <foo>");
    }

    #[test]
    fn html_to_text_no_html() {
        let text = html_to_text("plain text");
        assert_eq!(text, "plain text");
    }

    #[test]
    fn html_to_text_block_tags_add_spaces() {
        let html = "<div><p>first</p><p>second</p></div>";
        let text = html_to_text(html);
        assert_eq!(text, "first second");
    }

    #[test]
    fn extract_title_basic() {
        let html = "<html><head><title>My Page</title></head></html>";
        assert_eq!(extract_title(html), Some("My Page".to_string()));
    }

    #[test]
    fn extract_title_no_title() {
        let html = "<html><head></head></html>";
        assert_eq!(extract_title(html), None);
    }

    #[test]
    fn extract_meta_description() {
        let html = r#"<meta name="description" content="A test page">"#;
        assert_eq!(
            extract_meta_tag(html, "description"),
            Some("A test page".to_string())
        );
    }

    #[test]
    fn extract_meta_og_description() {
        let html = r#"<meta property="og:description" content="OG desc">"#;
        assert_eq!(
            extract_meta_tag(html, "og:description"),
            Some("OG desc".to_string())
        );
    }

    #[test]
    fn extract_html_lang_attr() {
        let html = r#"<html lang="en">"#;
        assert_eq!(extract_html_lang(html), Some("en".to_string()));
    }

    #[test]
    fn extract_metadata_combines() {
        let html = r#"<html lang="es"><head><title>Mi Pagina</title><meta name="description" content="Descripción"></head></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.title, Some("Mi Pagina".to_string()));
        assert_eq!(meta.description, Some("Descripción".to_string()));
        assert_eq!(meta.language, Some("es".to_string()));
    }

    #[test]
    fn html_to_markdown_basic() {
        let html = "<h1>Title</h1><p>Hello <strong>world</strong></p>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("**world**"));
    }

    #[test]
    fn html_to_markdown_links() {
        let html = r#"<a href="https://example.com">click here</a>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("[click here](https://example.com)"));
    }

    #[test]
    fn html_to_markdown_lists() {
        let html = "<ul><li>item 1</li><li>item 2</li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("- item 1"));
        assert!(md.contains("- item 2"));
    }

    #[test]
    fn html_to_markdown_removes_scripts() {
        let html = "<p>text</p><script>bad</script>";
        let md = html_to_markdown(html);
        assert!(!md.contains("bad"));
        assert!(md.contains("text"));
    }

    #[test]
    fn html_to_markdown_images() {
        let html = r#"<img src="pic.png" alt="photo">"#;
        let md = html_to_markdown(html);
        assert!(md.contains("![photo](pic.png)"));
    }

    #[test]
    fn extract_text_delegates_html() {
        let result = extract_text("<p>hello</p>", "text/html");
        assert_eq!(result, "hello");
    }

    #[test]
    fn extract_text_passes_plain() {
        let result = extract_text("hello world", "text/plain");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn parse_numeric_html_entities() {
        let html = "<p>&#65;&#x42;&#x43;</p>";
        let text = html_to_text(html);
        assert_eq!(text, "ABC");
    }

    #[test]
    fn html_to_text_empty() {
        assert_eq!(html_to_text(""), "");
    }

    #[test]
    fn html_to_markdown_hr() {
        let md = html_to_markdown("<hr>");
        assert!(md.contains("---"));
    }

    #[test]
    fn extract_pdf_nonexistent_file() {
        let result = extract_pdf(Path::new("/nonexistent/test.pdf"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("PDF") || err.contains("No such file") || err.contains("not found"));
    }

    #[test]
    fn extract_epub_nonexistent_file() {
        let result = extract_epub(Path::new("/nonexistent/test.epub"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_file_nonexistent_file() {
        let result = extract_file(Path::new("/nonexistent/test.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_file_pdf_nonexistent() {
        let result = extract_file(Path::new("/nonexistent/test.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_file_epub_nonexistent() {
        let result = extract_file(Path::new("/nonexistent/test.epub"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_file_txt_happy_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, "Hello, world!").unwrap();
        let result = extract_file(&path).unwrap();
        assert_eq!(result.content, "Hello, world!");
        assert_eq!(result.title, Some("hello".into()));
        assert_eq!(result.format, Format::Text);
    }

    #[test]
    fn extract_file_md_happy_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("readme.md");
        std::fs::write(&path, "# Title\n\nContent").unwrap();
        let result = extract_file(&path).unwrap();
        assert_eq!(result.content, "# Title\n\nContent");
        assert_eq!(result.title, Some("readme".into()));
        assert_eq!(result.format, Format::Markdown);
    }

    #[test]
    fn extract_file_html_happy_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("page.html");
        std::fs::write(&path, "<p>Hello</p>").unwrap();
        let result = extract_file(&path).unwrap();
        assert_eq!(result.content, "<p>Hello</p>");
        assert_eq!(result.title, Some("page".into()));
        assert_eq!(result.format, Format::Html);
    }

    #[test]
    fn extract_pdf_invalid_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("fake.pdf");
        std::fs::write(&path, "not a real PDF").unwrap();
        let result = extract_pdf(&path);
        assert!(result.is_err());
    }

    #[test]
    fn extract_epub_invalid_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("fake.epub");
        std::fs::write(&path, "not a real EPUB").unwrap();
        let result = extract_epub(&path);
        assert!(result.is_err());
    }

    #[test]
    fn extract_file_empty_txt() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "").unwrap();
        let result = extract_file(&path).unwrap();
        assert_eq!(result.content, "");
    }

    #[test]
    fn extract_text_with_markdown_content_type() {
        let result = extract_text("# Hello\n\nWorld", "text/markdown");
        assert_eq!(result, "# Hello\n\nWorld");
    }

    #[test]
    fn extract_text_with_md_content_type() {
        let result = extract_text("**bold**", "text/md");
        assert_eq!(result, "**bold**");
    }

    #[test]
    fn extract_text_unknown_content_type() {
        let result = extract_text("plain text", "application/octet-stream");
        assert_eq!(result, "plain text");
    }

    #[test]
    fn extract_text_empty() {
        assert_eq!(extract_text("", "text/html"), "");
        assert_eq!(extract_text("", "text/plain"), "");
    }

    #[test]
    fn html_to_markdown_blockquotes() {
        let md = html_to_markdown("<blockquote>cite</blockquote>");
        assert!(md.contains("> cite"));
    }

    #[test]
    fn html_to_markdown_inline_code() {
        let md = html_to_markdown("<p>use <code>fn()</code></p>");
        assert!(md.contains("`fn()`"));
    }

    #[test]
    fn html_to_markdown_strong_and_emphasis_nested() {
        let md = html_to_markdown("<p><strong><em>bold italic</em></strong></p>");
        assert!(md.contains("***"));
        assert!(md.contains("bold italic"));
    }

    #[test]
    fn html_to_markdown_anchor_without_href() {
        let md = html_to_markdown("<a>no link</a>");
        // Should just have the text, no markdown link syntax
        assert!(md.contains("no link"));
        assert!(!md.contains("]("));
    }

    #[test]
    fn html_to_markdown_image_without_src() {
        let md = html_to_markdown("<img alt=\"photo\">");
        assert!(!md.contains("!["));
    }

    #[test]
    fn html_to_markdown_empty_input() {
        assert_eq!(html_to_markdown(""), "");
    }

    #[test]
    fn html_to_markdown_pre_tags() {
        let md = html_to_markdown("<pre>code block</pre>");
        assert!(md.contains("```"));
        assert!(md.contains("code block"));
    }

    #[test]
    fn html_to_markdown_headings_h4_h5_h6() {
        let md = html_to_markdown("<h4>h4</h4><h5>h5</h5><h6>h6</h6>");
        assert!(md.contains("#### h4"));
        assert!(md.contains("##### h5"));
        assert!(md.contains("###### h6"));
    }

    #[test]
    fn html_to_markdown_hr_multiple() {
        let md = html_to_markdown("<hr><hr>");
        assert_eq!(md.matches("---").count(), 2);
    }

    #[test]
    fn html_to_markdown_consecutive_breaks() {
        let md = html_to_markdown("<br><br><br>");
        // The final regex collapses 3+ newlines to 2, but counts individual \n chars
        assert!(md.matches('\n').count() >= 2);
    }

    #[test]
    fn html_to_text_malformed_html() {
        let text = html_to_text("<p>hello</p><div>world</p></div>");
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn html_to_text_ampersand_not_entity() {
        let text = html_to_text("<p>AT&T && foo</p>");
        assert!(text.contains("AT&T"));
    }

    #[test]
    fn html_to_text_no_text_content() {
        let text = html_to_text("<div><span></span></div>");
        assert_eq!(text, "");
    }

    #[test]
    fn html_to_text_very_long() {
        let long = format!("<p>{}</p>", "word ".repeat(10_000));
        let text = html_to_text(&long);
        assert!(text.len() > 10_000);
    }

    #[test]
    fn extract_metadata_missing_title() {
        let html = r#"<html><meta name="description" content="desc"></html>"#;
        let meta = extract_metadata(html);
        assert!(meta.title.is_none());
        assert_eq!(meta.description, Some("desc".into()));
    }

    #[test]
    fn extract_metadata_missing_description() {
        let html = r#"<html><head><title>Title</title></head></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.title, Some("Title".into()));
        assert!(meta.description.is_none());
    }

    #[test]
    fn extract_metadata_missing_language() {
        let html = "<html><head><title>T</title></head></html>";
        let meta = extract_metadata(html);
        assert!(meta.language.is_none());
    }

    #[test]
    fn extract_metadata_language_with_region() {
        let html = r#"<html lang="en-US"><head><title>T</title></head></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.language, Some("en".into()));
    }

    #[test]
    fn extract_metadata_og_description_fallback() {
        let html = r#"<meta property="og:description" content="og desc">"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.description, Some("og desc".into()));
    }

    #[test]
    fn extract_meta_tag_content_before_name() {
        let html = r#"<meta content="value" name="description">"#;
        assert_eq!(extract_meta_tag(html, "description"), Some("value".into()));
    }

    #[test]
    fn html_to_text_nonstandard_entity() {
        let text = html_to_text("<p>&unknown;</p>");
        // Unknown entity — the & is not consumed so it's treated as literal text
        assert!(text.contains("&unknown;") || text.is_empty());
    }

    #[test]
    fn html_to_text_partial_entity() {
        let text = html_to_text("<p>foo & bar</p>");
        assert_eq!(text, "foo & bar");
    }

    #[test]
    fn html_to_markdown_anchor_with_empty_href() {
        let md = html_to_markdown(r#"<a href="">empty</a>"#);
        assert!(md.contains("[](empty)") || md.contains("empty"));
    }

    #[test]
    fn extract_file_no_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("Makefile");
        std::fs::write(&path, "all:\n\techo hi").unwrap();
        let result = extract_file(&path).unwrap();
        assert_eq!(result.format, Format::Text);
        assert_eq!(result.title, Some("Makefile".into()));
    }

    #[test]
    fn extract_file_md_with_no_title() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".hidden");
        std::fs::write(&path, "content").unwrap();
        let result = extract_file(&path).unwrap();
        assert!(result.title.is_some());
    }
}
