use crate::types::{QueueEntry, QueueStatus};
use anyhow::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct FeedQueue {
    path: PathBuf,
    entries: Vec<QueueEntry>,
}

impl FeedQueue {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join("feed_queue.jsonl");
        let entries = Self::load_entries(&path);
        Self { path, entries }
    }

    fn load_entries(path: &Path) -> Vec<QueueEntry> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                serde_json::from_str(line).ok()
            })
            .collect()
    }

    fn persist(&self) -> Result<()> {
        let mut content = String::new();
        for entry in &self.entries {
            content.push_str(&serde_json::to_string(entry)?);
            content.push('\n');
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn add(&mut self, url: &str) -> Result<()> {
        let entry = QueueEntry {
            id: Uuid::new_v4().to_string(),
            url: url.to_string(),
            status: QueueStatus::Pending,
            retries: 0,
            error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.entries.push(entry);
        self.persist()
    }

    pub fn list(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn pending(&self) -> Vec<&QueueEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == QueueStatus::Pending)
            .collect()
    }

    pub fn mark_processing(&mut self, id: &str) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.status = QueueStatus::Processing;
            entry.updated_at = Utc::now();
            self.persist()?;
        }
        Ok(())
    }

    pub fn mark_done(&mut self, id: &str) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.status = QueueStatus::Done;
            entry.updated_at = Utc::now();
            self.persist()?;
        }
        Ok(())
    }

    pub fn mark_failed(&mut self, id: &str, error: &str) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.status = QueueStatus::Failed;
            entry.retries += 1;
            entry.error = Some(error.to_string());
            entry.updated_at = Utc::now();
            self.persist()?;
        }
        Ok(())
    }

    pub fn stats(&self) -> QueueStats {
        let mut stats = QueueStats::default();
        for entry in &self.entries {
            match entry.status {
                QueueStatus::Pending => stats.pending += 1,
                QueueStatus::Processing => stats.processing += 1,
                QueueStatus::Done => stats.done += 1,
                QueueStatus::Failed => stats.failed += 1,
            }
        }
        stats
    }

    pub fn remove_done(&mut self) -> Result<()> {
        self.entries.retain(|e| e.status != QueueStatus::Done);
        self.persist()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct QueueStats {
    pub pending: usize,
    pub processing: usize,
    pub done: usize,
    pub failed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn new_queue_empty() {
        let dir = tempdir().unwrap();
        let q = FeedQueue::new(dir.path());
        assert_eq!(q.len(), 0);
        assert!(q.list().is_empty());
    }

    #[test]
    fn add_and_list() {
        let dir = tempdir().unwrap();
        let mut q = FeedQueue::new(dir.path());
        q.add("https://example.com/feed.xml").unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q.list()[0].url, "https://example.com/feed.xml");
        assert_eq!(q.list()[0].status, QueueStatus::Pending);
    }

    #[test]
    fn persist_and_reload() {
        let dir = tempdir().unwrap();
        {
            let mut q = FeedQueue::new(dir.path());
            q.add("https://a.com/feed").unwrap();
            q.add("https://b.com/feed").unwrap();
        }
        let q = FeedQueue::new(dir.path());
        assert_eq!(q.len(), 2);
        assert_eq!(q.list()[0].url, "https://a.com/feed");
        assert_eq!(q.list()[1].url, "https://b.com/feed");
    }

    #[test]
    fn mark_states() {
        let dir = tempdir().unwrap();
        let mut q = FeedQueue::new(dir.path());
        q.add("https://example.com/feed").unwrap();
        let id = q.list()[0].id.clone();

        q.mark_processing(&id).unwrap();
        assert_eq!(q.list()[0].status, QueueStatus::Processing);

        q.mark_done(&id).unwrap();
        assert_eq!(q.list()[0].status, QueueStatus::Done);
    }

    #[test]
    fn mark_failed_increments_retries() {
        let dir = tempdir().unwrap();
        let mut q = FeedQueue::new(dir.path());
        q.add("https://example.com/feed").unwrap();
        let id = q.list()[0].id.clone();

        q.mark_failed(&id, "timeout").unwrap();
        assert_eq!(q.list()[0].status, QueueStatus::Failed);
        assert_eq!(q.list()[0].retries, 1);
        assert_eq!(q.list()[0].error.as_deref(), Some("timeout"));
    }

    #[test]
    fn pending_filter() {
        let dir = tempdir().unwrap();
        let mut q = FeedQueue::new(dir.path());
        q.add("https://a.com").unwrap();
        q.add("https://b.com").unwrap();
        let id = q.list()[0].id.clone();
        q.mark_done(&id).unwrap();

        let p = q.pending();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].url, "https://b.com");
    }

    #[test]
    fn stats() {
        let dir = tempdir().unwrap();
        let mut q = FeedQueue::new(dir.path());
        q.add("https://a.com").unwrap();
        q.add("https://b.com").unwrap();
        q.add("https://c.com").unwrap();
        let id_a = q.list()[0].id.clone();
        let id_b = q.list()[1].id.clone();
        q.mark_done(&id_a).unwrap();
        q.mark_failed(&id_b, "err").unwrap();

        let s = q.stats();
        assert_eq!(s.pending, 1);
        assert_eq!(s.done, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.processing, 0);
    }

    #[test]
    fn remove_done() {
        let dir = tempdir().unwrap();
        let mut q = FeedQueue::new(dir.path());
        q.add("https://a.com").unwrap();
        q.add("https://b.com").unwrap();
        let id = q.list()[0].id.clone();
        q.mark_done(&id).unwrap();
        assert_eq!(q.len(), 2);

        q.remove_done().unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q.list()[0].url, "https://b.com");
    }

    #[test]
    fn reload_after_persist_preserves_states() {
        let dir = tempdir().unwrap();
        let id;
        {
            let mut q = FeedQueue::new(dir.path());
            q.add("https://example.com/feed").unwrap();
            id = q.list()[0].id.clone();
            q.mark_done(&id).unwrap();
        }
        let q = FeedQueue::new(dir.path());
        assert_eq!(q.list()[0].status, QueueStatus::Done);
    }
}
