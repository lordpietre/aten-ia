use crate::types::{ConversationBatch, Manifest, SegmentEntry, WriterConfig};
use crate::memvid::playlist::Playlist;
use crate::utils;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct MemvidWriter {
    playlist: Playlist,
    current_segment_path: PathBuf,
    current_segment_size: u64,
    pending_batches: Vec<ConversationBatch>,
}

impl MemvidWriter {
    pub fn init(config: WriterConfig) -> Result<Self> {
        let playlist = Playlist::init(config.clone())?;
        let current_segment_path = playlist.next_segment_path();

        Ok(Self {
            playlist,
            current_segment_path,
            current_segment_size: 0,
            pending_batches: Vec::new(),
        })
    }

    pub fn append_conversation(&mut self, batch: ConversationBatch) -> Result<()> {
        self.pending_batches.push(batch);

        if self.pending_batches.len() >= self.playlist.config.batch_size {
            self.flush()?;
        }

        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.pending_batches.is_empty() {
            return Ok(());
        }

        let batches = std::mem::take(&mut self.pending_batches);
        let temp_path = self.current_segment_path.with_extension("tmp");

        // Write combined batches to .mv2 via memvid-core
        {
            let mut mv = memvid_core::Memvid::create(&temp_path)
                .context("Failed to create .mv2 segment")?;

            for batch in &batches {
                let bytes = serde_json::to_vec(batch)
                    .context("Failed to serialize batch")?;

                let tags: Vec<String> = vec![
                    format!("type=conversation"),
                    format!("model={}", batch.model_used),
                    format!("tokens={}", batch.tokens_used),
                ];

                mv.put_bytes_with_options(&bytes, memvid_core::PutOptions {
                    tags,
                    ..Default::default()
                }).context("Failed to write batch to .mv2")?;
            }

            mv.commit().context("Failed to commit .mv2 segment")?;
        }

        let temp_size = std::fs::metadata(&temp_path)
            .context("Failed to read temp file size")?
            .len();

        // Atomic rename
        std::fs::rename(&temp_path, &self.current_segment_path)
            .context("Failed to atomically rename .mv2 segment")?;

        // Update manifest
        let first_batch = &batches[0];
        let total_messages: u32 = batches.iter().map(|b| b.messages.len() as u32).sum();
        let total_tokens: u32 = batches.iter().map(|b| b.tokens_used).sum();
        let checksum = utils::compute_file_checksum(&self.current_segment_path)?;

        let entry = SegmentEntry {
            id: first_batch.id.clone(),
            filename: self.current_segment_path
                .file_name()
                .expect("segment path has no file name")
                .to_string_lossy()
                .to_string(),
            created_at: first_batch.timestamp,
            size_bytes: temp_size,
            message_count: total_messages,
            model_used: first_batch.model_used.clone(),
            tokens_used: total_tokens,
            checksum,
        };

        self.playlist.add_segment(entry)?;
        self.current_segment_size += temp_size;

        // Roll segment if needed
        if self.playlist.should_roll_segment(self.current_segment_size) {
            self.current_segment_path = self.playlist.next_segment_path();
            self.current_segment_size = 0;
        }

        Ok(())
    }
}

impl Drop for MemvidWriter {
    fn drop(&mut self) {
        if !self.pending_batches.is_empty() {
            let _ = self.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConversationBatch, Message, MessageRole};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_batch(model: &str, msg_count: usize) -> ConversationBatch {
        let messages: Vec<Message> = (0..msg_count)
            .map(|i| Message {
                role: if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant },
                content: format!("message {}", i),
                timestamp: Utc::now(),
                tokens: Some(10),
            })
            .collect();
        ConversationBatch {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            messages,
            model_used: model.to_string(),
            tokens_used: 42,
        }
    }

    #[test]
    fn init_creates_writer() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let writer = MemvidWriter::init(config).unwrap();
        assert!(writer.pending_batches.is_empty());
    }

    #[test]
    fn append_accumulates_batches() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            batch_size: 5,
            ..Default::default()
        };
        let mut writer = MemvidWriter::init(config).unwrap();
        writer.append_conversation(make_batch("test", 2)).unwrap();
        assert_eq!(writer.pending_batches.len(), 1);
    }

    #[test]
    fn flush_writes_mv2() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            batch_size: 5,
            ..Default::default()
        };
        let mut writer = MemvidWriter::init(config).unwrap();
        writer.append_conversation(make_batch("test", 2)).unwrap();
        writer.flush().unwrap();
        assert!(writer.pending_batches.is_empty());
        let conv_dir = dir.path().join("conversations");
        let entries: Vec<_> = std::fs::read_dir(&conv_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!entries.is_empty());
        let mv2_path = entries[0].path();
        assert_eq!(mv2_path.extension().unwrap(), "mv2");
        assert!(memvid_core::Memvid::verify(&mv2_path, false).is_ok());
    }

    #[test]
    fn flush_updates_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            batch_size: 2,
            ..Default::default()
        };
        let mut writer = MemvidWriter::init(config).unwrap();
        writer.append_conversation(make_batch("test", 2)).unwrap();
        writer.append_conversation(make_batch("test", 1)).unwrap();
        writer.flush().unwrap();
        let manifest_path = dir.path().join("manifest.json");
        let manifest: Manifest =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        assert_eq!(manifest.conversation_segments.len(), 1);
    }

    #[test]
    fn auto_flush_at_batch_size() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            batch_size: 3,
            ..Default::default()
        };
        let mut writer = MemvidWriter::init(config).unwrap();
        writer.append_conversation(make_batch("test", 1)).unwrap();
        writer.append_conversation(make_batch("test", 1)).unwrap();
        assert_eq!(writer.pending_batches.len(), 2);
        writer.append_conversation(make_batch("test", 1)).unwrap();
        assert!(writer.pending_batches.is_empty());
    }

    #[test]
    fn segment_rolls_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            batch_size: 1,
            segment_max_bytes: 1,
            ..Default::default()
        };
        let mut writer = MemvidWriter::init(config).unwrap();
        let first_path = writer.current_segment_path.clone();
        writer.append_conversation(make_batch("test", 1)).unwrap();
        assert_ne!(writer.current_segment_path, first_path);
    }
}
