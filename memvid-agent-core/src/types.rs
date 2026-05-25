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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn conversation_batch_roundtrip() {
        let batch = ConversationBatch {
            id: "test-id".into(),
            timestamp: Utc::now(),
            messages: vec![
                Message { role: MessageRole::User, content: "hello".into(), timestamp: Utc::now(), tokens: None },
                Message { role: MessageRole::Assistant, content: "world".into(), timestamp: Utc::now(), tokens: Some(5) },
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
        let roles = vec![MessageRole::User, MessageRole::Assistant, MessageRole::System, MessageRole::Tool];
        for role in roles {
            let json = serde_json::to_string(&role).unwrap();
            let deserialized: MessageRole = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, role);
        }
    }
}
