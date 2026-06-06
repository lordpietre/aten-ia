use anyhow::{Context, Result};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

pub fn atomic_write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or(Path::new("."));
    let uuid = Uuid::new_v4();
    let temp_path = parent.join(format!(".tmp_{}", uuid));

    fs::write(&temp_path, contents.as_ref())?;

    // fsync temp file before rename
    let file = std::fs::File::open(&temp_path)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temp_path, path)?;

    // fsync parent directory to ensure rename is durable
    if let Ok(dir) = std::fs::File::open(parent) {
        dir.sync_all().ok();
    }

    Ok(())
}

/// Acquires an exclusive lock on the data directory.
/// Only one instance can hold the lock at a time.
pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub fn acquire(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(".lock");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let parts: Vec<&str> = content.split_whitespace().collect();
                let pid_str = parts.last().unwrap_or(&"");
                if let Ok(pid) = pid_str.parse::<u32>() {
                    let proc_path = format!("/proc/{}", pid);
                    if std::path::Path::new(&proc_path).exists() {
                        anyhow::bail!("Another aten-ia instance is already running (PID {})", pid);
                    }
                }
            }
            std::fs::remove_file(&path).ok();
        }
        let file = std::fs::File::create_new(&path).with_context(|| {
            format!(
                "Another instance is already running in {}",
                data_dir.display()
            )
        })?;
        use std::io::Write;
        writeln!(&file, "aten-ia {}", std::process::id())?;
        file.sync_all()?;
        Ok(Self { path })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("Failed to remove lock file {}: {}", self.path.display(), e);
        }
    }
}

pub fn sha256_digest(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Truncate a string to at most `max_chars` Unicode scalar values, appending
/// `…` (via `suffix`) when truncation occurred.
///
/// This exists because naive byte slicing (`&s[..n]`) panics when `n` lands in
/// the middle of a multi-byte UTF-8 sequence (accents, CJK, emoji). Truncating
/// by `char` boundaries is always safe regardless of where the limit falls.
pub fn truncate_chars(s: &str, max_chars: usize, suffix: &str) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{}{}", truncated, suffix)
}

pub fn compute_file_checksum<P: AsRef<Path>>(path: P) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path.as_ref())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_digest_empty() {
        let result = sha256_digest(b"");
        assert_eq!(result.len(), 64);
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_digest_known() {
        let result = sha256_digest(b"hello");
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn atomic_write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        atomic_write(&path, b"hello world").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        atomic_write(&path, b"data").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "data");
    }

    #[test]
    fn truncate_chars_under_limit_unchanged() {
        assert_eq!(truncate_chars("hello", 10, "..."), "hello");
        assert_eq!(truncate_chars("hello", 5, "..."), "hello");
    }

    #[test]
    fn truncate_chars_over_limit_adds_suffix() {
        assert_eq!(truncate_chars("hello world", 5, "..."), "hello...");
    }

    #[test]
    fn truncate_chars_is_utf8_safe_at_multibyte_boundary() {
        // Regression test: naive `&s[..n]` would panic here because the byte
        // index lands inside a 2-byte 'é'. truncate_chars must never panic and
        // must cut on a char boundary.
        let s = "é".repeat(600); // each 'é' is 2 bytes → byte 500 is mid-char
        let out = truncate_chars(&s, 500, "...");
        assert_eq!(out.chars().count(), 503); // 500 chars + "..."
        assert!(out.starts_with('é'));
        // The whole thing is still valid UTF-8 (would have panicked otherwise).
        assert!(out.ends_with("..."));
    }

    #[test]
    fn truncate_chars_handles_emoji_and_cjk() {
        let s = "😀漢字テスト";
        assert_eq!(truncate_chars(s, 3, "…"), "😀漢字…");
        assert_eq!(truncate_chars(s, 100, "…"), s);
    }

    #[test]
    fn file_checksum_matches_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"checksum me").unwrap();
        let file_hash = compute_file_checksum(&path).unwrap();
        let direct_hash = sha256_digest(b"checksum me");
        assert_eq!(file_hash, direct_hash);
    }

    #[test]
    fn atomic_write_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        atomic_write(&path, b"").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn atomic_write_large_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        let large = vec![0xABu8; 100_000];
        atomic_write(&path, &large).unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap().len(), 100_000);
    }

    #[test]
    fn file_lock_acquire_and_release() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _lock = FileLock::acquire(dir.path()).unwrap();
            assert!(dir.path().join(".lock").exists());
        }
        assert!(!dir.path().join(".lock").exists());
    }

    #[test]
    fn file_lock_prevents_second_instance() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = FileLock::acquire(dir.path()).unwrap();
        let second = FileLock::acquire(dir.path());
        assert!(second.is_err());
    }

    #[test]
    fn file_lock_contains_pid() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _lock = FileLock::acquire(dir.path()).unwrap();
            let content = std::fs::read_to_string(dir.path().join(".lock")).unwrap();
            let parts: Vec<&str> = content.trim().split_whitespace().collect();
            assert_eq!(parts[0], "aten-ia");
            let pid: u32 = parts[1].parse().unwrap();
            assert_eq!(pid, std::process::id());
        }
    }

    #[test]
    fn file_checksum_nonexistent_file() {
        let result = compute_file_checksum("/nonexistent/file.bin");
        assert!(result.is_err());
    }

    #[test]
    fn file_checksum_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.bin");
        std::fs::write(&path, b"").unwrap();
        let hash = compute_file_checksum(&path).unwrap();
        assert_eq!(hash, sha256_digest(b""));
    }

    #[test]
    fn sha256_digest_binary() {
        let result = sha256_digest(&[0x00, 0xFF, 0xAB, 0xCD]);
        assert_eq!(result.len(), 64);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
