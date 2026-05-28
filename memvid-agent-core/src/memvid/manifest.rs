use crate::types::{Manifest, SegmentEntry};
use anyhow::{Context, Result};
use std::path::Path;

pub fn load_manifest<P: AsRef<Path>>(path: P) -> Result<Manifest> {
    let content = std::fs::read_to_string(path.as_ref())
        .context("Failed to read manifest.json")?;
    let manifest: Manifest = serde_json::from_str(&content)
        .context("Failed to parse manifest.json")?;
    Ok(manifest)
}

pub fn save_manifest<P: AsRef<Path>>(manifest: &Manifest, path: P) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or(Path::new("."));
    let uuid = uuid::Uuid::new_v4();
    let temp_path = parent.join(format!(".tmp_{}", uuid));
    let content = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&temp_path, &content)?;

    // fsync temp file before rename
    let file = std::fs::File::open(&temp_path)?;
    file.sync_all()?;
    drop(file);

    std::fs::rename(&temp_path, path)?;

    // fsync parent directory
    if let Ok(dir) = std::fs::File::open(parent) {
        dir.sync_all().ok();
    }

    Ok(())
}

pub fn create_initial_manifest(core_segment: &str) -> Manifest {
    use chrono::Utc;
    Manifest {
        version: "1.0.0".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        core_segment: core_segment.to_string(),
        conversation_segments: Vec::new(),
        knowledge_segments: Vec::new(),
        archived_segments: Vec::new(),
    }
}

pub fn append_conversation_to_manifest(
    manifest: &mut Manifest,
    entry: SegmentEntry,
) {
    manifest.conversation_segments.push(entry);
    manifest.updated_at = chrono::Utc::now();
}

pub fn append_knowledge_to_manifest(
    manifest: &mut Manifest,
    entry: SegmentEntry,
) {
    manifest.knowledge_segments.push(entry);
    manifest.updated_at = chrono::Utc::now();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SegmentEntry;
    use chrono::Utc;

    #[test]
    fn create_initial_manifest_has_version() {
        let m = create_initial_manifest("core.mv2");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.core_segment, "core.mv2");
        assert!(m.conversation_segments.is_empty());
        assert!(m.knowledge_segments.is_empty());
        assert!(m.archived_segments.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        let m = create_initial_manifest("core.mv2");
        save_manifest(&m, &path).unwrap();
        let loaded = load_manifest(&path).unwrap();
        assert_eq!(loaded.version, m.version);
        assert_eq!(loaded.core_segment, m.core_segment);
    }

    #[test]
    fn append_conversation_adds_entry() {
        let mut m = create_initial_manifest("core.mv2");
        let entry = SegmentEntry {
            id: "entry-1".into(),
            filename: "conv_20250101_001.mv2".into(),
            created_at: Utc::now(),
            size_bytes: 1024,
            message_count: 5,
            model_used: "test".into(),
            tokens_used: 100,
            checksum: "abc".into(),
        };
        append_conversation_to_manifest(&mut m, entry);
        assert_eq!(m.conversation_segments.len(), 1);
        assert_eq!(m.conversation_segments[0].id, "entry-1");
    }

    #[test]
    fn save_uses_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        let m = create_initial_manifest("core.mv2");
        save_manifest(&m, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn append_knowledge_adds_entry() {
        let mut m = create_initial_manifest("core.mv2");
        let entry = SegmentEntry {
            id: "know-1".into(),
            filename: "know_20250101_001.mv2".into(),
            created_at: Utc::now(),
            size_bytes: 2048,
            message_count: 0,
            model_used: "test".into(),
            tokens_used: 0,
            checksum: "xyz".into(),
        };
        append_knowledge_to_manifest(&mut m, entry);
        assert_eq!(m.knowledge_segments.len(), 1);
        assert_eq!(m.knowledge_segments[0].id, "know-1");
    }

    #[test]
    fn append_knowledge_multiple_entries() {
        let mut m = create_initial_manifest("core.mv2");
        for i in 0..3 {
            let entry = SegmentEntry {
                id: format!("know-{}", i),
                filename: format!("know_20250101_00{}.mv2", i),
                created_at: Utc::now(),
                size_bytes: 100,
                message_count: 0,
                model_used: "test".into(),
                tokens_used: 0,
                checksum: format!("chk{}", i),
            };
            append_knowledge_to_manifest(&mut m, entry);
        }
        assert_eq!(m.knowledge_segments.len(), 3);
    }

    #[test]
    fn append_conversation_appends_not_replaces() {
        let mut m = create_initial_manifest("core.mv2");
        for i in 0..3 {
            let entry = SegmentEntry {
                id: format!("conv-{}", i),
                filename: format!("conv_20250101_00{}.mv2", i),
                created_at: Utc::now(),
                size_bytes: 100,
                message_count: 1,
                model_used: "test".into(),
                tokens_used: 10,
                checksum: format!("chk{}", i),
            };
            append_conversation_to_manifest(&mut m, entry);
        }
        assert_eq!(m.conversation_segments.len(), 3);
    }

    #[test]
    fn load_manifest_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let result = load_manifest(&path);
        assert!(result.is_err());
    }

    #[test]
    fn create_initial_manifest_no_segments() {
        let m = create_initial_manifest("core.mv2");
        assert_eq!(m.version, "1.0.0");
        assert!(m.conversation_segments.is_empty());
        assert!(m.knowledge_segments.is_empty());
        assert!(m.archived_segments.is_empty());
        assert_eq!(m.core_segment, "core.mv2");
    }
}
