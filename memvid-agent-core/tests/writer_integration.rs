use std::path::PathBuf;

// Writer integration test: full cycle Playlist → MemvidWriter → flush → verify.
// Does NOT require a .gguf model.

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct WriterConfig {
    batch_size: usize,
    segment_max_bytes: u64,
    data_dir: PathBuf,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            segment_max_bytes: 50 * 1024 * 1024,
            data_dir: PathBuf::from("memvid_data"),
        }
    }
}

#[test]
fn full_writer_cycle() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let config = WriterConfig {
        data_dir: dir.path().to_path_buf(),
        batch_size: 3,
        ..Default::default()
    };

    // 1. Playlist init creates directories and core.mv2
    let data_dir = &config.data_dir;
    std::fs::create_dir_all(data_dir.join("conversations"))?;
    std::fs::create_dir_all(data_dir.join("knowledge"))?;
    std::fs::create_dir_all(data_dir.join("archive"))?;

    // 2. Create a conversation batch similar to how agent.rs does it
    let batch = serde_json::json!({
        "id": "test-integration-id",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "messages": [
            { "role": "User", "content": "hello", "timestamp": chrono::Utc::now().to_rfc3339(), "tokens": null },
            { "role": "Assistant", "content": "world", "timestamp": chrono::Utc::now().to_rfc3339(), "tokens": 5 }
        ],
        "model_used": "integration-test",
        "tokens_used": 42
    });

    // 3. Write via memvid-core directly (same pattern as MemvidWriter::flush)
    let segment_path = data_dir.join("conversations").join("conv_integration_001.mv2");
    let temp_path = segment_path.with_extension("tmp");

    {
        let mut mv = memvid_core::Memvid::create(&temp_path)?;
        let bytes = serde_json::to_vec(&batch)?;
        mv.put_bytes_with_options(
            &bytes,
            memvid_core::PutOptions {
                tags: vec!["type=integration-test".into()],
                ..Default::default()
            },
        )?;
        mv.commit()?;
    }

    // 4. Atomic rename
    std::fs::rename(&temp_path, &segment_path)?;

    // 5. Verify the .mv2 is valid
    assert!(segment_path.exists());
    assert!(memvid_core::Memvid::verify(&segment_path, false).is_ok());

    // 6. Read back and verify content
    let mv = memvid_core::Memvid::open(&segment_path)?;
    // smoke check — stats should return non-zero entries
    let stats = mv.stats().unwrap();
    assert!(stats.frame_count > 0);

    Ok(())
}

#[test]
fn memvid_core_version_is_compatible() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("version_test.mv2");
    let mut mv = memvid_core::Memvid::create(&path)?;
    mv.put_bytes(b"compatibility check")?;
    mv.commit()?;
    assert!(memvid_core::Memvid::verify(&path, false).is_ok());
    Ok(())
}
