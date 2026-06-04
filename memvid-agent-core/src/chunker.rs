use crate::types::{Chunk, ChunkOptions, ChunkStrategy};

pub fn chunk_text(text: &str, options: &ChunkOptions, source: &str) -> Vec<Chunk> {
    match options.strategy {
        ChunkStrategy::Heading => chunk_by_headings(text, options, source),
        ChunkStrategy::Paragraph => chunk_by_paragraphs(text, options, source),
        ChunkStrategy::Fixed => chunk_fixed(text, options, source),
    }
}

fn chunk_by_headings(text: &str, options: &ChunkOptions, source: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = text.lines().collect();
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current_section = String::new();
    let mut current_heading: Option<String> = None;
    let mut chunk_idx = 0u32;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let has_content = !current_section.trim().is_empty();
            if has_content || current_heading.is_some() {
                chunks.push(Chunk {
                    content: current_section.trim().to_string(),
                    index: chunk_idx,
                    heading: current_heading.clone(),
                    source: source.to_string(),
                });
                chunk_idx += 1;
            }
            current_section.clear();
            current_heading = Some(trimmed.to_string());
            continue;
        }

        current_section.push_str(line);
        current_section.push('\n');

        if current_section.len() >= options.max_size {
            let content = current_section.trim().to_string();
            if !content.is_empty() {
                chunks.push(Chunk {
                    content,
                    index: chunk_idx,
                    heading: current_heading.clone(),
                    source: source.to_string(),
                });
                chunk_idx += 1;
            }

            let overlap_chars = options.overlap.min(current_section.len());
            let overlap_start = current_section.len().saturating_sub(overlap_chars);
            let overlap_start = current_section.floor_char_boundary(overlap_start);
            let overlap_text = &current_section[overlap_start..];
            current_section = if let Some(pos) = overlap_text.rfind('\n') {
                overlap_text[pos..].to_string()
            } else {
                overlap_text.to_string()
            };
        }
    }

    let remaining = current_section.trim().to_string();
    if !remaining.is_empty() {
        chunks.push(Chunk {
            content: remaining,
            index: chunk_idx,
            heading: current_heading.clone(),
            source: source.to_string(),
        });
    } else if current_heading.is_some() && chunks.iter().any(|c| c.heading.is_some()) {
        chunks.push(Chunk {
            content: String::new(),
            index: chunk_idx,
            heading: current_heading,
            source: source.to_string(),
        });
    }

    chunks
}

fn chunk_by_paragraphs(text: &str, options: &ChunkOptions, source: &str) -> Vec<Chunk> {
    let paragraphs: Vec<&str> = text
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current = String::new();
    let mut chunk_idx = 0u32;
    let mut first_heading: Option<String> = None;

    for para in &paragraphs {
        if para.starts_with('#') && first_heading.is_none() {
            first_heading = Some(para.to_string());
        }

        if current.len() + para.len() > options.max_size && !current.is_empty() {
            chunks.push(Chunk {
                content: current.trim().to_string(),
                index: chunk_idx,
                heading: first_heading.clone(),
                source: source.to_string(),
            });
            chunk_idx += 1;

            let words: Vec<&str> = current.split_whitespace().collect();
            let overlap_words = words.len().saturating_sub(options.overlap.max(50));
            if overlap_words > 0 && overlap_words < words.len() {
                current = words[overlap_words..].join(" ");
                current.push('\n');
            } else {
                current.clear();
            }
        }

        current.push_str(para);
        current.push('\n');
    }

    let remaining = current.trim().to_string();
    if !remaining.is_empty() {
        chunks.push(Chunk {
            content: remaining,
            index: chunk_idx,
            heading: first_heading,
            source: source.to_string(),
        });
    }

    chunks
}

fn chunk_fixed(text: &str, options: &ChunkOptions, source: &str) -> Vec<Chunk> {
    let max_size = options.max_size;
    let overlap = options.overlap;

    if text.len() <= max_size {
        return vec![Chunk {
            content: text.to_string(),
            index: 0,
            heading: None,
            source: source.to_string(),
        }];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let mut idx = 0u32;

    while start < text.len() {
        let mut end = (start + max_size).min(text.len());
        end = text.floor_char_boundary(end);

        if end <= start {
            break;
        }

        let chunk_text = &text[start..end];

        chunks.push(Chunk {
            content: chunk_text.to_string(),
            index: idx,
            heading: None,
            source: source.to_string(),
        });
        idx += 1;

        if end >= text.len() {
            break;
        }

        let advance = max_size.saturating_sub(overlap).max(1);
        let next_start = start + advance;
        let candidate = text.floor_char_boundary(next_start);
        // candidate must be strictly past start to make progress.
        // If floor_char_boundary rounds back to start (multi-byte edge case),
        // fall back to end which is always a valid boundary.
        start = if candidate > start { candidate } else { end };
    }

    chunks
}

pub fn chunk_and_deduplicate(text: &str, options: &ChunkOptions, source: &str) -> Vec<Chunk> {
    let chunks = chunk_text(text, options, source);
    let mut seen = Vec::new();
    let mut unique = Vec::new();

    for chunk in chunks {
        let checksum = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(chunk.content.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        if !seen.contains(&checksum) {
            seen.push(checksum);
            unique.push(chunk);
        }
    }

    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_opts() -> ChunkOptions {
        ChunkOptions {
            max_size: 50,
            overlap: 10,
            strategy: ChunkStrategy::Paragraph,
        }
    }

    #[test]
    fn chunk_small_text() {
        let chunks = chunk_text("hello world", &default_opts(), "test");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "hello world");
    }

    #[test]
    fn chunk_by_headings_splits_sections() {
        let text = "# Intro\nhello\n# Details\nmore info here\n# Conclusion\nbye";
        let opts = ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunk_text(text, &opts, "doc");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].heading, Some("# Intro".to_string()));
        assert_eq!(chunks[1].heading, Some("# Details".to_string()));
        assert_eq!(chunks[2].heading, Some("# Conclusion".to_string()));
    }

    #[test]
    fn chunk_paragraphs_splits_at_boundary() {
        let text = "word\n".repeat(100);
        let opts = ChunkOptions {
            max_size: 50,
            overlap: 10,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_text(&text, &opts, "src");
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_fixed_splits_evenly() {
        let text = "A".repeat(200);
        let opts = ChunkOptions {
            max_size: 60,
            overlap: 10,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(&text, &opts, "fixed");
        assert_eq!(chunks.len(), 4);
        for chunk in &chunks {
            assert!(chunk.content.len() <= 60);
        }
    }

    #[test]
    fn chunk_empty_text() {
        let chunks = chunk_text("", &default_opts(), "empty");
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_heading_preserves_order() {
        let text = "# A\n1\n# B\n2\n# C\n3";
        let opts = ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunk_text(text, &opts, "doc");
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i as u32);
        }
    }

    #[test]
    fn chunk_and_deduplicate_removes_duplicates() {
        let text = "hello\nhello\nworld";
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_and_deduplicate(text, &opts, "dedup");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].content, "hello");
        assert_eq!(chunks[1].content, "world");
    }

    #[test]
    fn chunk_paragraph_overlap_works() {
        let text = "word\n".repeat(30);
        let opts = ChunkOptions {
            max_size: 60,
            overlap: 20,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_text(&text, &opts, "overlap");
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_fixed_overlap_advances_correctly() {
        let text = "ABCDEFGHIJ";
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 2,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(text, &opts, "fixed");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].content, "ABCDE");
        assert_eq!(chunks[1].content, "DEFGH");
        assert_eq!(chunks[2].content, "GHIJ");
    }

    #[test]
    fn chunk_heading_no_headings() {
        let text = "plain text\nwithout any\nheadings";
        let opts = ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunk_text(text, &opts, "plain");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading.is_none());
    }

    #[test]
    fn chunk_heading_consecutive_headings() {
        let text = "# H1\ncontent1\n# H2\ncontent2\n# H3";
        let opts = ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunk_text(text, &opts, "doc");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].content, "content1");
        assert_eq!(chunks[1].content, "content2");
        assert!(chunks[2].content.is_empty());
    }

    #[test]
    fn chunk_heading_at_end() {
        let text = "content\n# Heading";
        let opts = ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunk_text(text, &opts, "doc");
        // Only the content before the heading is captured;
        // trailing heading with no content after it produces no chunk
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, None);
        assert!(chunks[0].content.contains("content"));
    }

    #[test]
    fn chunk_fixed_exact_size() {
        let text = "ABCDE";
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(text, &opts, "fixed");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "ABCDE");
    }

    #[test]
    fn chunk_fixed_exact_size_plus_one() {
        let text = "ABCDEF";
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(text, &opts, "fixed");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].content, "ABCDE");
        assert_eq!(chunks[1].content, "F");
    }

    #[test]
    fn chunk_fixed_overlap_equals_max_size() {
        let text = "ABCDEFGHIJ";
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 5,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(text, &opts, "fixed");
        assert_eq!(chunks.len(), 6);
    }

    #[test]
    fn chunk_fixed_overlap_zero() {
        let text = "ABCDEFGHIJ";
        let opts = ChunkOptions {
            max_size: 3,
            overlap: 0,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(text, &opts, "fixed");
        assert_eq!(chunks.len(), 4); // "ABC", "DEF", "GHI", "J"
    }

    #[test]
    fn chunk_paragraph_multiple_paragraphs_exceed_max() {
        let text = "A\n".repeat(10) + &"B\n".repeat(10);
        let opts = ChunkOptions {
            max_size: 20,
            overlap: 5,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_text(&text, &opts, "src");
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_paragraph_overlap_large() {
        let text = "word\n".repeat(20);
        let opts = ChunkOptions {
            max_size: 30,
            overlap: 25,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_text(&text, &opts, "src");
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn chunk_and_deduplicate_all_unique() {
        let text = "hello\nworld\nfoo";
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_and_deduplicate(text, &opts, "dedup");
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn chunk_and_deduplicate_empty_input() {
        let opts = ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_and_deduplicate("", &opts, "dedup");
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_unicode_text() {
        let text = "ñoño y café";
        let opts = ChunkOptions {
            max_size: 50,
            overlap: 5,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(text, &opts, "unicode");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("ñoño"));
    }

    #[test]
    fn chunk_whitespace_only() {
        let opts = ChunkOptions {
            max_size: 50,
            overlap: 5,
            strategy: ChunkStrategy::Paragraph,
        };
        let chunks = chunk_text("   \n  \n  ", &opts, "ws");
        assert!(chunks.is_empty() || chunks.iter().all(|c| c.content.trim().is_empty()));
    }

#[test]
fn chunk_heading_mixed_levels() {
        let text = "# H1\ncontent\n## H2\nmore\n### H3\ndetails";
        let opts = ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        };
        let chunks = chunk_text(text, &opts, "doc");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].heading, Some("# H1".into()));
        assert_eq!(chunks[1].heading, Some("## H2".into()));
        assert_eq!(chunks[2].heading, Some("### H3".into()));
    }

    #[test]
    fn chunk_fixed_multibyte_safe() {
        let text = "ññññññññññ".to_string();
        let byte_len = text.len();
        assert!(byte_len > text.chars().count());
        let max_size = (byte_len / 2).max(3);
        let opts = ChunkOptions {
            max_size,
            overlap: 2,
            strategy: ChunkStrategy::Fixed,
        };
        let chunks = chunk_text(&text, &opts, "multi");
        for chunk in &chunks {
            assert!(chunk.content.is_char_boundary(0));
            assert!(chunk.content.is_char_boundary(chunk.content.len()));
            let _ = chunk.content.chars().count();
        }
        assert!(!chunks.is_empty());
    }
}
