use crate::chunker;
use crate::types::{ChunkOptions, ChunkStrategy, KnowledgeEntry};
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const JSONL_FILENAME: &str = "knowledge_index.jsonl";

pub struct KnowledgeIndex {
    entries: Vec<KnowledgeEntry>,
    jsonl_path: PathBuf,
}

impl KnowledgeIndex {
    pub fn load(data_dir: &Path) -> Result<Self> {
        let jsonl_path = data_dir.join(JSONL_FILENAME);
        let entries = if jsonl_path.exists() {
            Self::load_jsonl(&jsonl_path)?
        } else {
            Vec::new()
        };
        Ok(Self {
            entries,
            jsonl_path,
        })
    }

    pub fn rebuild_from_jsonl(data_dir: &Path) -> Result<Self> {
        Self::load(data_dir)
    }

    pub fn add_entry(&mut self, entry: KnowledgeEntry) -> Result<()> {
        self.entries.push(entry.clone());
        self.append_entry_to_jsonl(&entry)
    }

    pub fn add_entries(&mut self, entries: &[KnowledgeEntry]) -> Result<()> {
        for entry in entries {
            self.entries.push(entry.clone());
        }
        self.flush_jsonl()?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<&KnowledgeEntry> {
        if query.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        if query_words.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(usize, &KnowledgeEntry)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let content_lower = entry.content.to_lowercase();
                let source_lower = entry.source.to_lowercase();
                let id_lower = entry.id.to_lowercase();

                let matches: usize = query_words
                    .iter()
                    .map(|w| {
                        content_lower.matches(w).count()
                            + source_lower.matches(w).count()
                            + id_lower.matches(w).count()
                    })
                    .sum();

                (matches * 10000 + i, entry)
            })
            .filter(|(_, e)| {
                let content_lower = e.content.to_lowercase();
                query_words.iter().any(|w| content_lower.contains(w))
            })
            .collect();

        scored.sort_by_key(|k| std::cmp::Reverse(k.0));
        scored.truncate(limit);
        scored.into_iter().map(|(_, e)| e).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[KnowledgeEntry] {
        &self.entries
    }

    /// Remove all entries whose source starts with the given prefix.
    /// Rewrites the full JSONL after removal.
    pub fn remove_by_source_prefix(&mut self, prefix: &str) -> Result<usize> {
        let before = self.entries.len();
        self.entries.retain(|e| !e.source.starts_with(prefix));
        let removed = before - self.entries.len();
        if removed > 0 {
            self.flush_jsonl()?;
        }
        Ok(removed)
    }

    pub fn chunk_text(text: &str) -> Vec<String> {
        let opts = ChunkOptions {
            max_size: 4000,
            overlap: 600,
            strategy: ChunkStrategy::Fixed,
        };
        chunker::chunk_text(text, &opts, "internal")
            .into_iter()
            .map(|c| c.content)
            .collect()
    }

    fn flush_jsonl(&self) -> Result<()> {
        let parent = self.jsonl_path.parent().unwrap_or(Path::new("."));
        let uuid = uuid::Uuid::new_v4();
        let temp_path = parent.join(format!(".tmp_{}", uuid));

        let mut out = std::fs::File::create(&temp_path)
            .context("Failed to create temp file for knowledge_index.jsonl")?;
        for entry in &self.entries {
            let line =
                serde_json::to_string(entry).context("Failed to serialize knowledge entry")?;
            writeln!(out, "{}", line).context("Failed to write to knowledge_index.jsonl")?;
        }
        out.sync_all()
            .context("Failed to fsync knowledge_index.jsonl")?;
        drop(out);

        std::fs::rename(&temp_path, &self.jsonl_path)
            .context("Failed to rename knowledge_index.jsonl")?;

        if let Ok(dir) = std::fs::File::open(parent) {
            dir.sync_all().ok();
        }

        Ok(())
    }

    fn append_entry_to_jsonl(&self, entry: &KnowledgeEntry) -> Result<()> {
        let parent = self.jsonl_path.parent().unwrap_or(Path::new("."));
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .context("Failed to create parent directory for knowledge_index.jsonl")?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.jsonl_path)
            .context("Failed to open knowledge_index.jsonl for append")?;
        let line = serde_json::to_string(entry).context("Failed to serialize knowledge entry")?;
        use std::io::Write;
        writeln!(file, "{}", line).context("Failed to write to knowledge_index.jsonl")?;
        file.sync_all()
            .context("Failed to fsync knowledge_index.jsonl")?;
        Ok(())
    }

    fn load_jsonl(path: &Path) -> Result<Vec<KnowledgeEntry>> {
        let file = std::fs::File::open(path).context("Failed to open knowledge_index.jsonl")?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line.context("Failed to read line from knowledge_index.jsonl")?;
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<KnowledgeEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    eprintln!("[warn] Skipping malformed knowledge entry: {}", e);
                }
            }
        }

        Ok(entries)
    }

    /// Rebuild knowledge_index.jsonl by reading all knowledge .mv2 segments.
    /// Returns a fresh KnowledgeIndex with all entries loaded from the .mv2 files.
    pub fn rebuild_from_mv2(data_dir: &Path) -> Result<Self> {
        use crate::memvid::reader::Reader;
        use crate::types::Manifest;

        let manifest_path = data_dir.join("manifest.json");
        let jsonl_path = data_dir.join(JSONL_FILENAME);

        let manifest: Manifest = if manifest_path.exists() {
            let content =
                std::fs::read_to_string(&manifest_path).context("Failed to read manifest.json")?;
            serde_json::from_str(&content).context("Failed to parse manifest.json")?
        } else {
            return Self::load(data_dir);
        };

        let knowledge_dir = data_dir.join("knowledge");
        let mut entries: Vec<KnowledgeEntry> = Vec::new();

        for seg in &manifest.knowledge_segments {
            let seg_path = knowledge_dir.join(&seg.filename);
            if !seg_path.exists() {
                eprintln!("[warn] Knowledge segment not found: {}", seg_path.display());
                continue;
            }

            let mut reader = match Reader::open(&seg_path) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[warn] Failed to open {}: {}", seg_path.display(), e);
                    continue;
                }
            };

            let frames = match reader.enumerate() {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("[warn] Failed to enumerate {}: {}", seg_path.display(), e);
                    continue;
                }
            };

            for frame in &frames {
                let text = match reader.read_text(frame.id) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if let Ok(entry) = serde_json::from_str::<KnowledgeEntry>(&text) {
                    entries.push(entry);
                }
            }
        }

        let index = Self {
            entries,
            jsonl_path,
        };
        index.flush_jsonl()?;
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_entry(source: &str, content: &str) -> KnowledgeEntry {
        KnowledgeEntry {
            id: Uuid::new_v4().to_string(),
            source: source.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            checksum: crate::utils::sha256_digest(content.as_bytes()),
        }
    }

    #[test]
    fn empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let index = KnowledgeIndex::load(dir.path()).unwrap();
        assert_eq!(index.len(), 0);
        assert!(index.is_empty());
    }

    #[test]
    fn add_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();

        index
            .add_entry(make_entry(
                "python",
                "Python is a programming language for general purpose",
            ))
            .unwrap();
        index
            .add_entry(make_entry("rust", "Rust is a systems programming language"))
            .unwrap();

        assert_eq!(index.len(), 2);

        let results = index.search("python", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "python");
    }

    #[test]
    fn search_returns_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();

        index
            .add_entry(make_entry("python", "Python is great for data science"))
            .unwrap();
        index
            .add_entry(make_entry(
                "python_async",
                "Python async programming with asyncio",
            ))
            .unwrap();
        index
            .add_entry(make_entry("rust", "Rust is great for systems"))
            .unwrap();

        let results = index.search("python", 5);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.source.contains("python")));
    }

    #[test]
    fn search_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();

        for i in 0..10 {
            index
                .add_entry(make_entry(
                    &format!("src_{}", i),
                    &format!("content about item {}", i),
                ))
                .unwrap();
        }

        let results = index.search("content", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_empty_query() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entry(make_entry("test", "some content")).unwrap();
        let results = index.search("", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn no_match_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index
            .add_entry(make_entry("python", "Python is fun"))
            .unwrap();
        let results = index.search("rust", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn persist_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut index = KnowledgeIndex::load(dir.path()).unwrap();
            index
                .add_entry(make_entry("persist", "test persistence"))
                .unwrap();
        }
        {
            let index = KnowledgeIndex::load(dir.path()).unwrap();
            assert_eq!(index.len(), 1);
            assert_eq!(index.entries()[0].source, "persist");
        }
    }

    #[test]
    fn chunk_text_small() {
        let chunks = KnowledgeIndex::chunk_text("hello world");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("hello world"));
    }

    #[test]
    fn chunk_text_large() {
        let text = "word ".repeat(2000);
        let chunks = KnowledgeIndex::chunk_text(&text);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 4000);
        }
    }

    #[test]
    fn chunk_text_overlap() {
        let text = "word ".repeat(5000);
        let chunks = KnowledgeIndex::chunk_text(&text);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn rebuild_from_jsonl_recovers_data() {
        let dir = tempfile::tempdir().unwrap();
        let entries_count;
        {
            let mut index = KnowledgeIndex::load(dir.path()).unwrap();
            index.add_entry(make_entry("src1", "content1")).unwrap();
            index.add_entry(make_entry("src2", "content2")).unwrap();
            entries_count = index.len();
            drop(index);
        }
        let rebuilt = KnowledgeIndex::rebuild_from_jsonl(dir.path()).unwrap();
        assert_eq!(rebuilt.len(), entries_count);
    }

    #[test]
    fn add_entries_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entries(&[]).unwrap();
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn add_entries_single() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entries(&[make_entry("src", "content")]).unwrap();
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn add_entries_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index
            .add_entries(&[
                make_entry("a", "alpha"),
                make_entry("b", "beta"),
                make_entry("c", "gamma"),
            ])
            .unwrap();
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn search_special_chars() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index
            .add_entry(make_entry("test", "hello (world) [test] {foo} &bar$"))
            .unwrap();
        let results = index.search("world", 5);
        assert_eq!(results.len(), 1);
        let results = index.search("(world)", 5);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_whitespace_query() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entry(make_entry("test", "hello world")).unwrap();
        let results = index.search("   ", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn search_unicode() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entry(make_entry("es", "ñoño y café")).unwrap();
        let results = index.search("ñoño", 5);
        assert_eq!(results.len(), 1);
        let results = index.search("café", 5);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_scoring_by_match_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index
            .add_entry(make_entry("a", "python python python"))
            .unwrap();
        index.add_entry(make_entry("b", "python is fun")).unwrap();
        let results = index.search("python", 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].source, "a");
    }

    #[test]
    fn chunk_text_empty() {
        let chunks = KnowledgeIndex::chunk_text("");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn chunk_text_single_char() {
        let chunks = KnowledgeIndex::chunk_text("a");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "a");
    }

    #[test]
    fn load_jsonl_with_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge_index.jsonl");
        std::fs::write(&path, "").unwrap();
        let index = KnowledgeIndex::load(dir.path()).unwrap();
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn load_jsonl_with_malformed_lines_skips_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge_index.jsonl");
        std::fs::write(&path, "{valid}\nnot json\n{\"also\": \"invalid\"\n").unwrap();
        let index = KnowledgeIndex::load(dir.path()).unwrap();
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn entries_iterator() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entry(make_entry("a", "alpha")).unwrap();
        index.add_entry(make_entry("b", "beta")).unwrap();
        let entries: Vec<_> = index.entries().iter().map(|e| e.source.as_str()).collect();
        assert_eq!(entries, vec!["a", "b"]);
    }

    #[test]
    fn rebuild_from_jsonl_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let index = KnowledgeIndex::rebuild_from_jsonl(dir.path()).unwrap();
        assert_eq!(index.len(), 0);
    }
}
