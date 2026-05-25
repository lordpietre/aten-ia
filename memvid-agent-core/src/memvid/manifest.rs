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
    let temp_path = path.with_extension("tmp");
    let content = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&temp_path, &content)?;
    std::fs::rename(&temp_path, path)?;
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
        assert!(!path.with_extension("tmp").exists());
        assert!(path.exists());
    }
}
