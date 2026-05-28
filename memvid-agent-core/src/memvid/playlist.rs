use crate::types::{Manifest, SegmentEntry, WriterConfig};
use crate::memvid::manifest;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct Playlist {
    pub manifest: Manifest,
    pub config: WriterConfig,
    pub manifest_path: PathBuf,
}

impl Playlist {
    pub fn init(config: WriterConfig) -> Result<Self> {
        let data_dir = &config.data_dir;
        std::fs::create_dir_all(data_dir.join("conversations"))?;
        std::fs::create_dir_all(data_dir.join("knowledge"))?;
        std::fs::create_dir_all(data_dir.join("archive"))?;

        let manifest_path = data_dir.join("manifest.json");
        let core_path = data_dir.join("core.mv2");

        if !core_path.exists() {
            let mut mv = memvid_core::Memvid::create(&core_path)
                .context("Failed to create core.mv2")?;
            let identity = serde_json::json!({
                "agent": "memvid-agent-core",
                "version": env!("CARGO_PKG_VERSION"),
                "created_at": chrono::Utc::now().to_rfc3339(),
            });
            mv.put_bytes_with_options(
                &serde_json::to_vec(&identity)
                    .context("Failed to serialize identity")?,
                memvid_core::PutOptions {
                    tags: vec!["type=core".into(), "section=identity".into()],
                    ..Default::default()
                },
            )
            .context("Failed to write identity payload")?;
            mv.commit().context("Failed to commit core.mv2")?;
        }

        let manifest = if manifest_path.exists() {
            manifest::load_manifest(&manifest_path)?
        } else {
            let m = manifest::create_initial_manifest("core.mv2");
            manifest::save_manifest(&m, &manifest_path)?;
            m
        };

        Ok(Self {
            manifest,
            config,
            manifest_path,
        })
    }

    pub fn add_segment(&mut self, entry: SegmentEntry) -> Result<()> {
        // Backup current manifest before modifying
        let backup_path = self.manifest_path.with_extension("json.bak");
        std::fs::copy(&self.manifest_path, &backup_path).ok();

        manifest::append_conversation_to_manifest(&mut self.manifest, entry);
        manifest::save_manifest(&self.manifest, &self.manifest_path)?;
        Ok(())
    }

    pub fn add_knowledge_segment(&mut self, entry: SegmentEntry) -> Result<()> {
        let backup_path = self.manifest_path.with_extension("json.bak");
        std::fs::copy(&self.manifest_path, &backup_path).ok();

        manifest::append_knowledge_to_manifest(&mut self.manifest, entry);
        manifest::save_manifest(&self.manifest, &self.manifest_path)?;
        Ok(())
    }

    pub fn next_segment_path(&self) -> PathBuf {
        let now = chrono::Utc::now();
        let date = now.format("%Y%m%d");
        let count = self.manifest.conversation_segments.len() + 1;
        self.config
            .data_dir
            .join("conversations")
            .join(format!("conv_{}_{:03}.mv2", date, count))
    }

    pub fn next_knowledge_path(&self) -> PathBuf {
        let now = chrono::Utc::now();
        let date = now.format("%Y%m%d");
        let count = self.manifest.knowledge_segments.len() + 1;
        self.config
            .data_dir
            .join("knowledge")
            .join(format!("know_{}_{:03}.mv2", date, count))
    }

    pub fn should_roll_segment(&self, current_size: u64) -> bool {
        current_size >= self.config.segment_max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let playlist = Playlist::init(config).unwrap();
        let data_dir = playlist.manifest_path.parent().unwrap();
        assert!(data_dir.join("conversations").exists());
        assert!(data_dir.join("knowledge").exists());
        assert!(data_dir.join("archive").exists());
    }

    #[test]
    fn init_creates_core_mv2() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        Playlist::init(config).unwrap();
        let core_path = dir.path().join("core.mv2");
        assert!(core_path.exists());
        assert!(std::fs::metadata(&core_path).unwrap().len() > 0);
        assert!(memvid_core::Memvid::verify(&core_path, false).is_ok());
    }

    #[test]
    fn init_creates_manifest_json() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        Playlist::init(config).unwrap();
        assert!(dir.path().join("manifest.json").exists());
    }

    #[test]
    fn next_segment_path_format() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let playlist = Playlist::init(config).unwrap();
        let path = playlist.next_segment_path();
        let filename = path.file_name().unwrap().to_string_lossy();
        assert!(filename.starts_with("conv_"));
        assert!(filename.ends_with(".mv2"));
    }

    #[test]
    fn should_roll_segment_returns_true_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let playlist = Playlist::init(config).unwrap();
        assert!(!playlist.should_roll_segment(0));
        assert!(playlist.should_roll_segment(50 * 1024 * 1024));
        assert!(playlist.should_roll_segment(100 * 1024 * 1024));
    }

    #[test]
    fn add_segment_updates_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut playlist = Playlist::init(config).unwrap();
        let entry = SegmentEntry {
            id: "seg-1".into(),
            filename: "conv_20250101_001.mv2".into(),
            created_at: chrono::Utc::now(),
            size_bytes: 512,
            message_count: 3,
            model_used: "test".into(),
            tokens_used: 50,
            checksum: "def".into(),
        };
        playlist.add_segment(entry).unwrap();
        assert_eq!(playlist.manifest.conversation_segments.len(), 1);
        let loaded = crate::memvid::manifest::load_manifest(&playlist.manifest_path).unwrap();
        assert_eq!(loaded.conversation_segments.len(), 1);
    }

    #[test]
    fn init_loads_existing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        Playlist::init(config.clone()).unwrap();
        let playlist2 = Playlist::init(config).unwrap();
        assert_eq!(playlist2.manifest.version, "1.0.0");
    }

    #[test]
    fn add_knowledge_segment_adds_entry() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut playlist = Playlist::init(config).unwrap();
        let entry = SegmentEntry {
            id: "know-seg-1".into(),
            filename: "know_20250101_001.mv2".into(),
            created_at: chrono::Utc::now(),
            size_bytes: 1024,
            message_count: 0,
            model_used: "test".into(),
            tokens_used: 0,
            checksum: "chk".into(),
        };
        playlist.add_knowledge_segment(entry).unwrap();
        assert_eq!(playlist.manifest.knowledge_segments.len(), 1);
        let loaded = crate::memvid::manifest::load_manifest(&playlist.manifest_path).unwrap();
        assert_eq!(loaded.knowledge_segments.len(), 1);
    }

    #[test]
    fn add_knowledge_segment_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut playlist = Playlist::init(config).unwrap();
        let entry = SegmentEntry {
            id: "know-1".into(),
            filename: "know_20250101_001.mv2".into(),
            created_at: chrono::Utc::now(),
            size_bytes: 512,
            message_count: 0,
            model_used: "test".into(),
            tokens_used: 0,
            checksum: "abc".into(),
        };
        playlist.add_knowledge_segment(entry).unwrap();
        let manifest: crate::types::Manifest =
            serde_json::from_str(&std::fs::read_to_string(&playlist.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest.knowledge_segments.len(), 1);
        assert_eq!(manifest.knowledge_segments[0].id, "know-1");
    }

    #[test]
    fn next_knowledge_path_format() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let playlist = Playlist::init(config).unwrap();
        let path = playlist.next_knowledge_path();
        let filename = path.file_name().unwrap().to_string_lossy();
        assert!(filename.starts_with("know_"));
        assert!(filename.ends_with(".mv2"));
        assert!(path.parent().unwrap().ends_with("knowledge"));
    }

    #[test]
    fn next_knowledge_path_increments_count() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut playlist = Playlist::init(config).unwrap();
        let first = playlist.next_knowledge_path();
        let entry = SegmentEntry {
            id: "know-1".into(),
            filename: first.file_name().unwrap().to_string_lossy().to_string(),
            created_at: chrono::Utc::now(),
            size_bytes: 100,
            message_count: 0,
            model_used: "test".into(),
            tokens_used: 0,
            checksum: "x".into(),
        };
        playlist.add_knowledge_segment(entry).unwrap();
        let second = playlist.next_knowledge_path();
        assert_ne!(first, second);
    }

    #[test]
    fn should_roll_segment_false_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            segment_max_bytes: 1024,
            ..Default::default()
        };
        let playlist = Playlist::init(config).unwrap();
        assert!(!playlist.should_roll_segment(0));
        assert!(!playlist.should_roll_segment(512));
        assert!(!playlist.should_roll_segment(1023));
    }

    #[test]
    fn should_roll_segment_true_at_exact_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            segment_max_bytes: 1024,
            ..Default::default()
        };
        let playlist = Playlist::init(config).unwrap();
        assert!(playlist.should_roll_segment(1024));
        assert!(playlist.should_roll_segment(2048));
    }

    #[test]
    fn init_creates_backup_on_add_segment() {
        let dir = tempfile::tempdir().unwrap();
        let config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut playlist = Playlist::init(config.clone()).unwrap();
        let entry = SegmentEntry {
            id: "s1".into(),
            filename: "conv_20250101_001.mv2".into(),
            created_at: chrono::Utc::now(),
            size_bytes: 100,
            message_count: 1,
            model_used: "t".into(),
            tokens_used: 10,
            checksum: "c".into(),
        };
        playlist.add_segment(entry).unwrap();
        let bak_path = playlist.manifest_path.with_extension("json.bak");
        assert!(bak_path.exists());
    }
}
