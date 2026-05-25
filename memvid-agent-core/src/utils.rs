use anyhow::Result;
use std::io::Read;
use std::path::Path;
use std::fs;

pub fn atomic_write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, contents.as_ref())?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

pub fn sha256_digest(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
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
        assert!(!path.with_extension("tmp").exists());
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
}
