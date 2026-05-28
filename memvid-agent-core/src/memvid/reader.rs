use anyhow::{Context, Result};
use memvid_core::TimelineQuery;
use std::num::NonZeroU64;
use std::path::Path;

pub struct FrameInfo {
    pub id: u64,
    pub timestamp: i64,
    pub preview: String,
    pub uri: Option<String>,
    pub tags: Vec<String>,
}

pub struct Reader {
    mv: memvid_core::Memvid,
}

impl Reader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mv = memvid_core::Memvid::open_read_only(path.as_ref())
            .with_context(|| format!("Failed to open .mv2 file: {}", path.as_ref().display()))?;
        Ok(Self { mv })
    }

    pub fn frame_count(&self) -> usize {
        self.mv.frame_count()
    }

    pub fn enumerate(&mut self) -> Result<Vec<FrameInfo>> {
        let count = self.mv.frame_count();
        if count == 0 {
            return Ok(Vec::new());
        }

        let query = TimelineQuery {
            limit: NonZeroU64::new(count as u64),
            since: None,
            until: None,
            reverse: false,
        };

        let entries = self.mv.timeline(query)?;

        let ids: Vec<u64> = entries.iter().map(|e| e.frame_id).collect();

        let mut result = Vec::with_capacity(entries.len());
        for (entry, &id) in entries.iter().zip(ids.iter()) {
            let frame = self.mv.frame_by_id(id)?;
            result.push(FrameInfo {
                id: entry.frame_id,
                timestamp: entry.timestamp,
                preview: entry.preview.clone(),
                uri: entry.uri.clone(),
                tags: frame.tags.clone(),
            });
        }

        Ok(result)
    }

    pub fn read_text(&mut self, frame_id: u64) -> Result<String> {
        let payload = self
            .mv
            .frame_canonical_payload(frame_id)
            .with_context(|| format!("Failed to read payload from frame {}", frame_id))?;
        String::from_utf8(payload)
            .with_context(|| format!("Frame {} content is not valid UTF-8", frame_id))
    }

    pub fn read_raw(&mut self, frame_id: u64) -> Result<Vec<u8>> {
        self.mv
            .frame_canonical_payload(frame_id)
            .with_context(|| format!("Failed to read payload from frame {}", frame_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memvid_core::Memvid;
    use memvid_core::PutOptions;

    fn create_test_mv2(dir: &tempfile::TempDir, frames: &[&str]) -> std::path::PathBuf {
        let path = dir.path().join("test.mv2");
        let mut mv = Memvid::create(&path).expect("create .mv2");
        for &content in frames {
            mv.put_bytes_with_options(
                content.as_bytes(),
                PutOptions {
                    tags: vec!["type=test".into()],
                    ..Default::default()
                },
            )
            .expect("put frame");
        }
        mv.commit().expect("commit");
        path
    }

    #[test]
    fn open_nonexistent_file() {
        let result = Reader::open("/nonexistent/path.mv2");
        assert!(result.is_err());
    }

    #[test]
    fn open_empty_mv2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.mv2");
        {
            let mv = Memvid::create(&path).expect("create empty");
            // drop without writing frames — Memvid drops call commit in Drop
        }
        let mut reader = Reader::open(&path).expect("open empty");
        assert_eq!(reader.frame_count(), 0);
        let frames = reader.enumerate().expect("enumerate empty");
        assert!(frames.is_empty());
    }

    #[test]
    fn enumerate_single_frame() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_test_mv2(&dir, &["hello world"]);
        let mut reader = Reader::open(&path).expect("open");
        assert_eq!(reader.frame_count(), 1);
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn enumerate_multiple_frames() {
        let dir = tempfile::tempdir().unwrap();
        let data = &["alpha", "beta", "gamma"];
        let path = create_test_mv2(&dir, data);
        let mut reader = Reader::open(&path).expect("open");
        assert_eq!(reader.frame_count(), 3);
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn read_text_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_test_mv2(&dir, &["Hello, World!"]);
        let mut reader = Reader::open(&path).expect("open");
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 1);
        let text = reader.read_text(frames[0].id).expect("read text");
        assert_eq!(text, "Hello, World!");
    }

    #[test]
    fn read_raw_payload() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"binary payload \x00\x01\x02";
        let path = dir.path().join("raw.mv2");
        {
            let mut mv = Memvid::create(&path).expect("create");
            mv.put_bytes(payload).expect("put");
            mv.commit().expect("commit");
        }
        let mut reader = Reader::open(&path).expect("open");
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 1);
        let raw = reader.read_raw(frames[0].id).expect("read raw");
        assert_eq!(raw, payload);
    }

    #[test]
    fn tags_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tags.mv2");
        {
            let mut mv = Memvid::create(&path).expect("create");
            mv.put_bytes_with_options(
                b"tagged content",
                PutOptions {
                    tags: vec!["type=knowledge".into(), "source=python".into()],
                    ..Default::default()
                },
            )
            .expect("put with tags");
            mv.commit().expect("commit");
        }
        let mut reader = Reader::open(&path).expect("open");
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].tags.contains(&"type=knowledge".to_string()));
        assert!(frames[0].tags.contains(&"source=python".to_string()));
    }

    #[test]
    fn roundtrip_write_then_read_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversation.mv2");

        let conversation_data = serde_json::json!({
            "id": "conv-1",
            "model_used": "smollm2-360m",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "world"}
            ],
            "tokens_used": 42
        });

        {
            let mut mv = Memvid::create(&path).expect("create");
            let bytes = serde_json::to_vec(&conversation_data).expect("serialize");
            mv.put_bytes_with_options(
                &bytes,
                PutOptions {
                    tags: vec![
                        "type=conversation".into(),
                        "model=smollm2-360m".into(),
                        "tokens=42".into(),
                    ],
                    ..Default::default()
                },
            )
            .expect("put conversation");
            mv.commit().expect("commit");
        }

        let mut reader = Reader::open(&path).expect("open");
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].tags.contains(&"type=conversation".to_string()));

        let text = reader.read_text(frames[0].id).expect("read text");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("parse json");
        assert_eq!(parsed["id"], "conv-1");
        assert_eq!(parsed["model_used"], "smollm2-360m");
        assert_eq!(parsed["messages"][0]["content"], "hello");
        assert_eq!(parsed["tokens_used"], 42);
    }

    #[test]
    fn roundtrip_write_then_read_knowledge() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.mv2");

        let knowledge_entry = serde_json::json!({
            "id": "know-1",
            "source": "python",
            "content": "Python is a programming language",
            "checksum": "abc123"
        });

        {
            let mut mv = Memvid::create(&path).expect("create");
            let bytes = serde_json::to_vec(&knowledge_entry).expect("serialize");
            mv.put_bytes_with_options(
                &bytes,
                PutOptions {
                    tags: vec!["type=knowledge".into(), "source=python".into()],
                    ..Default::default()
                },
            )
            .expect("put knowledge");
            mv.commit().expect("commit");
        }

        let mut reader = Reader::open(&path).expect("open");
        let frames = reader.enumerate().expect("enumerate");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].tags.contains(&"type=knowledge".to_string()));

        let text = reader.read_text(frames[0].id).expect("read text");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("parse json");
        assert_eq!(parsed["id"], "know-1");
        assert_eq!(parsed["source"], "python");
        assert_eq!(parsed["content"], "Python is a programming language");
    }
}
