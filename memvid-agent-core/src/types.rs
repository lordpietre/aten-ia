use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationBatch {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub messages: Vec<Message>,
    pub model_used: String,
    pub tokens_used: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentEntry {
    pub id: String,
    pub filename: String,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub message_count: u32,
    pub model_used: String,
    pub tokens_used: u32,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub core_segment: String,
    pub conversation_segments: Vec<SegmentEntry>,
    pub knowledge_segments: Vec<SegmentEntry>,
    pub archived_segments: Vec<SegmentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub source: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub checksum: String,
}

#[derive(Debug, Clone)]
pub struct WriterConfig {
    pub batch_size: usize,
    pub segment_max_bytes: u64,
    pub data_dir: std::path::PathBuf,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            segment_max_bytes: 50 * 1024 * 1024,
            data_dir: std::path::PathBuf::from("memvid_data"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchedContent {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedEntry {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub published: Option<DateTime<Utc>>,
    pub source_feed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: String,
    pub url: String,
    pub status: QueueStatus,
    pub retries: u32,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QueueStatus {
    Pending,
    Processing,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
pub struct FeedResult {
    pub feed_title: Option<String>,
    pub entries_found: usize,
    pub entries_indexed: usize,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub index: u32,
    pub heading: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ChunkOptions {
    pub max_size: usize,
    pub overlap: usize,
    pub strategy: ChunkStrategy,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self {
            max_size: 1024,
            overlap: 200,
            strategy: ChunkStrategy::Paragraph,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Text,
    Markdown,
    Html,
    Pdf,
    Epub,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::Text => write!(f, "text"),
            Format::Markdown => write!(f, "markdown"),
            Format::Html => write!(f, "html"),
            Format::Pdf => write!(f, "pdf"),
            Format::Epub => write!(f, "epub"),
        }
    }
}

impl Format {
    pub fn from_extension(path: &std::path::Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "pdf" => Format::Pdf,
            "epub" => Format::Epub,
            "md" | "markdown" => Format::Markdown,
            "html" | "htm" | "xhtml" => Format::Html,
            _ => Format::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChunkStrategy {
    Paragraph,
    Heading,
    Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionConfig {
    pub user_agent: String,
    pub timeout_seconds: u64,
    pub max_size_bytes: u64,
    pub rate_limit_per_second: u32,
    pub chunk_max_size: usize,
    pub chunk_overlap: usize,
    pub max_retries: u32,
    pub retry_backoff_seconds: u64,
}

impl Default for IngestionConfig {
    fn default() -> Self {
        Self {
            user_agent: "aten-ia/0.1.0".to_string(),
            timeout_seconds: 30,
            max_size_bytes: 5 * 1024 * 1024,
            rate_limit_per_second: 2,
            chunk_max_size: 1024,
            chunk_overlap: 200,
            max_retries: 3,
            retry_backoff_seconds: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::Path;

    #[test]
    fn format_from_extension_pdf() {
        assert_eq!(Format::from_extension(Path::new("doc.pdf")), Format::Pdf);
    }

    #[test]
    fn format_from_extension_epub() {
        assert_eq!(Format::from_extension(Path::new("book.epub")), Format::Epub);
    }

    #[test]
    fn format_from_extension_markdown() {
        assert_eq!(
            Format::from_extension(Path::new("readme.md")),
            Format::Markdown
        );
        assert_eq!(
            Format::from_extension(Path::new("readme.markdown")),
            Format::Markdown
        );
    }

    #[test]
    fn format_from_extension_html() {
        assert_eq!(
            Format::from_extension(Path::new("index.html")),
            Format::Html
        );
        assert_eq!(Format::from_extension(Path::new("page.htm")), Format::Html);
        assert_eq!(Format::from_extension(Path::new("doc.xhtml")), Format::Html);
    }

    #[test]
    fn format_from_extension_text_default() {
        assert_eq!(Format::from_extension(Path::new("file.txt")), Format::Text);
        assert_eq!(
            Format::from_extension(Path::new("file.unknown")),
            Format::Text
        );
        assert_eq!(Format::from_extension(Path::new("Makefile")), Format::Text);
        assert_eq!(Format::from_extension(Path::new("")), Format::Text);
    }

    #[test]
    fn format_display() {
        assert_eq!(format!("{}", Format::Text), "text");
        assert_eq!(format!("{}", Format::Markdown), "markdown");
        assert_eq!(format!("{}", Format::Html), "html");
        assert_eq!(format!("{}", Format::Pdf), "pdf");
        assert_eq!(format!("{}", Format::Epub), "epub");
    }

    #[test]
    fn conversation_batch_roundtrip() {
        let batch = ConversationBatch {
            id: "test-id".into(),
            timestamp: Utc::now(),
            messages: vec![
                Message {
                    role: MessageRole::User,
                    content: "hello".into(),
                    timestamp: Utc::now(),
                    tokens: None,
                },
                Message {
                    role: MessageRole::Assistant,
                    content: "world".into(),
                    timestamp: Utc::now(),
                    tokens: Some(5),
                },
            ],
            model_used: "test-model".into(),
            tokens_used: 42,
        };
        let json = serde_json::to_string(&batch).unwrap();
        let deserialized: ConversationBatch = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, batch.id);
        assert_eq!(deserialized.messages.len(), 2);
        assert_eq!(deserialized.model_used, "test-model");
    }

    #[test]
    fn manifest_roundtrip() {
        let manifest = Manifest {
            version: "1.0.0".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            core_segment: "core.mv2".into(),
            conversation_segments: vec![],
            knowledge_segments: vec![],
            archived_segments: vec![],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.version, "1.0.0");
    }

    #[test]
    fn writer_config_defaults() {
        let config = WriterConfig::default();
        assert_eq!(config.batch_size, 10);
        assert_eq!(config.segment_max_bytes, 50 * 1024 * 1024);
        assert_eq!(config.data_dir, std::path::PathBuf::from("memvid_data"));
    }

    #[test]
    fn message_role_serde() {
        let roles = vec![
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::System,
            MessageRole::Tool,
        ];
        for role in roles {
            let json = serde_json::to_string(&role).unwrap();
            let deserialized: MessageRole = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, role);
        }
    }

    #[test]
    fn format_from_extension_uppercase() {
        assert_eq!(Format::from_extension(Path::new("doc.PDF")), Format::Pdf);
        assert_eq!(Format::from_extension(Path::new("book.EPUB")), Format::Epub);
        assert_eq!(
            Format::from_extension(Path::new("readme.MD")),
            Format::Markdown
        );
        assert_eq!(Format::from_extension(Path::new("page.HTML")), Format::Html);
    }

    #[test]
    fn format_from_extension_mixed_case() {
        assert_eq!(Format::from_extension(Path::new("doc.Pdf")), Format::Pdf);
        assert_eq!(Format::from_extension(Path::new("doc.ePuB")), Format::Epub);
    }

    #[test]
    fn format_from_extension_multi_dot() {
        assert_eq!(
            Format::from_extension(Path::new("archive.tar.gz")),
            Format::Text
        );
        assert_eq!(
            Format::from_extension(Path::new("file.backup.pdf")),
            Format::Pdf
        );
        assert_eq!(
            Format::from_extension(Path::new("file.backup.epub")),
            Format::Epub
        );
    }

    #[test]
    fn format_from_extension_no_extension() {
        assert_eq!(Format::from_extension(Path::new("Makefile")), Format::Text);
        assert_eq!(Format::from_extension(Path::new("README")), Format::Text);
        assert_eq!(Format::from_extension(Path::new(".")), Format::Text);
    }

    #[test]
    fn format_from_extension_trailing_dot() {
        assert_eq!(Format::from_extension(Path::new("file.")), Format::Text);
        assert_eq!(Format::from_extension(Path::new("file.pdf.")), Format::Text);
    }

    #[test]
    fn ingestion_config_defaults() {
        let config = IngestionConfig::default();
        assert_eq!(config.user_agent, "aten-ia/0.1.0");
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.max_size_bytes, 5 * 1024 * 1024);
        assert_eq!(config.rate_limit_per_second, 2);
        assert_eq!(config.chunk_max_size, 1024);
        assert_eq!(config.chunk_overlap, 200);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_backoff_seconds, 5);
    }

    #[test]
    fn chunk_options_defaults() {
        let opts = ChunkOptions::default();
        assert_eq!(opts.max_size, 1024);
        assert_eq!(opts.overlap, 200);
        assert_eq!(opts.strategy, ChunkStrategy::Paragraph);
    }

    #[test]
    fn fetched_content_construction() {
        let content = FetchedContent {
            url: "https://example.com".into(),
            title: Some("Example".into()),
            description: Some("An example site".into()),
            content: "<p>hello</p>".into(),
            content_type: "text/html".into(),
            size_bytes: 14,
        };
        assert_eq!(content.url, "https://example.com");
        assert_eq!(content.title, Some("Example".into()));
        assert_eq!(content.content_type, "text/html");
    }

    #[test]
    fn chunk_construction() {
        let chunk = Chunk {
            content: "some text".into(),
            index: 0,
            heading: Some("Introduction".into()),
            source: "test.md".into(),
        };
        assert_eq!(chunk.content, "some text");
        assert_eq!(chunk.index, 0);
        assert_eq!(chunk.heading, Some("Introduction".into()));
    }

    #[test]
    fn chunk_without_heading() {
        let chunk = Chunk {
            content: "no heading".into(),
            index: 1,
            heading: None,
            source: "doc.txt".into(),
        };
        assert_eq!(chunk.content, "no heading");
        assert!(chunk.heading.is_none());
    }

    #[test]
    fn segment_entry_serde() {
        let entry = SegmentEntry {
            id: "seg-1".into(),
            filename: "conv_20250101_001.mv2".into(),
            created_at: Utc::now(),
            size_bytes: 1024,
            message_count: 5,
            model_used: "test-model".into(),
            tokens_used: 100,
            checksum: "abc123".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SegmentEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, entry.id);
        assert_eq!(deserialized.filename, entry.filename);
        assert_eq!(deserialized.size_bytes, 1024);
        assert_eq!(deserialized.message_count, 5);
        assert_eq!(deserialized.model_used, "test-model");
        assert_eq!(deserialized.tokens_used, 100);
        assert_eq!(deserialized.checksum, "abc123");
    }

    #[test]
    fn knowledge_entry_serde() {
        let entry = KnowledgeEntry {
            id: "know-1".into(),
            source: "test-source".into(),
            content: "test content".into(),
            timestamp: Utc::now(),
            checksum: "def456".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: KnowledgeEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, entry.id);
        assert_eq!(deserialized.source, entry.source);
        assert_eq!(deserialized.content, entry.content);
        assert_eq!(deserialized.checksum, entry.checksum);
    }

    #[test]
    fn message_with_zero_tokens() {
        let msg = Message {
            role: MessageRole::User,
            content: "hello".into(),
            timestamp: Utc::now(),
            tokens: Some(0),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tokens, Some(0));
    }

    #[test]
    fn fetched_content_without_title_description() {
        let content = FetchedContent {
            url: "https://example.com".into(),
            title: None,
            description: None,
            content: "body".into(),
            content_type: "text/plain".into(),
            size_bytes: 4,
        };
        assert!(content.title.is_none());
        assert!(content.description.is_none());
    }

    #[test]
    fn format_from_extension_special_chars() {
        assert_eq!(
            Format::from_extension(Path::new("file[name].pdf")),
            Format::Pdf
        );
        assert_eq!(
            Format::from_extension(Path::new("file with spaces.epub")),
            Format::Epub
        );
    }

    #[test]
    fn format_debug_and_clone() {
        let f = Format::Pdf;
        let cloned = f.clone();
        assert_eq!(format!("{:?}", f), "Pdf");
        assert_eq!(cloned, Format::Pdf);
    }
}
