use crate::books_catalog::{BooksCatalog, prepare_knowledge_from_books};
use crate::chunker;
use crate::config::Config;
use crate::context_policy::ContextPolicy;
use crate::generation;
use crate::llama::context::LlamaContext;
use crate::memvid::writer::MemvidWriter;
use crate::prompt::{ChatTemplate, DEFAULT_DEVELOPER_PROMPT, PromptBuilder};
use crate::retrieval::KnowledgeIndex;
use crate::session::Session;
use crate::types::{
    ChunkOptions, ChunkStrategy, FeedResult, FetchedContent, IngestionConfig,
    KnowledgeEntry, Message, MessageRole, WriterConfig,
};
use crate::web_fetcher::WebFetcher;
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct BatchResult {
    pub success: Vec<String>,
    pub failures: Vec<(String, String)>,
    pub total_chunks: u32,
}

pub struct Agent {
    llm: LlamaContext,
    memory: MemvidWriter,
    knowledge_index: KnowledgeIndex,
    model_name: String,
    session: Session,
    prompt_builder: PromptBuilder,
    context_policy: ContextPolicy,
}

impl Agent {
    pub fn init(config: &Config) -> Result<Self> {
        let llm = LlamaContext::init(
            &config.model.path,
            config.model.n_ctx,
            config.model.n_gpu_layers,
            &config.model.kv_type_k,
            &config.model.kv_type_v,
            config.generation.top_k,
            config.generation.top_p,
            config.generation.temp,
        )?;
        let writer_config = WriterConfig {
            data_dir: config.data_dir.clone(),
            ..Default::default()
        };
        let memory = MemvidWriter::init(writer_config)?;
        let knowledge_index = KnowledgeIndex::load(&config.data_dir)?;

        let template = ChatTemplate::from_str(&config.model.chat_template);
        let mut prompt_builder = PromptBuilder::new(template);
        if config.developer_mode {
            let dev_prompt = config
                .developer_prompt
                .clone()
                .unwrap_or_else(|| DEFAULT_DEVELOPER_PROMPT.to_string());
            prompt_builder = prompt_builder.with_developer_prompt(dev_prompt);
        } else {
            prompt_builder = prompt_builder.with_developer_prompt("");
        }

        let context_policy = ContextPolicy::new(config.model.n_ctx, config.generation.max_tokens);

        Ok(Self {
            llm,
            memory,
            knowledge_index,
            model_name: config.model.name.clone(),
            session: Session::new(),
            prompt_builder,
            context_policy,
        })
    }

    /// Chat using externally provided messages (e.g. from API).
    /// Unlike `chat()`, this does NOT push messages to the internal session.
    pub fn chat_with_messages(&mut self, messages: &[Message]) -> Result<String> {
        let user_input = messages
            .iter()
            .rfind(|m| m.role == MessageRole::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");

        self.session.increment_interactions();
        let result = generation::generate_chat(
            &mut self.llm,
            &self.prompt_builder,
            &self.context_policy,
            &self.knowledge_index,
            messages,
            user_input,
        )?;

        Ok(result.response)
    }

    pub fn chat(&mut self, user_input: &str) -> Result<String> {
        self.session.increment_interactions();

        let result = generation::generate_chat(
            &mut self.llm,
            &self.prompt_builder,
            &self.context_policy,
            &self.knowledge_index,
            self.session.messages(),
            user_input,
        )?;

        self.session.push_message(Message {
            role: MessageRole::User,
            content: user_input.to_string(),
            timestamp: Utc::now(),
            tokens: None,
        });
        self.session.push_message(Message {
            role: MessageRole::Assistant,
            content: result.response.clone(),
            timestamp: Utc::now(),
            tokens: Some(result.tokens_estimated),
        });

        if self.session.interaction_count().is_multiple_of(5) {
            self.session
                .flush(&self.llm, &self.model_name, &mut self.memory)?;
        }

        Ok(result.response)
    }

    pub fn ingest_raw(&mut self, filename: &str, content: &str) -> Result<()> {
        self.session.push_message(Message {
            role: MessageRole::System,
            content: format!(
                "The user has loaded the file '{}' with the following content:\n\n{}",
                filename, content
            ),
            timestamp: Utc::now(),
            tokens: None,
        });
        Ok(())
    }

    pub fn ingest_knowledge(&mut self, filename: &str, content: &str) -> Result<()> {
        self.ingest_raw(filename, content)?;
        let chunk_opts = ChunkOptions {
            max_size: 4000,
            overlap: 600,
            strategy: ChunkStrategy::Heading,
        };
        self.store_knowledge_chunked(filename, content, &chunk_opts)?;
        Ok(())
    }

    pub fn ingest_file(
        &mut self,
        path: &std::path::Path,
    ) -> Result<crate::extractor::ExtractedFile> {
        let extracted = crate::extractor::extract_file(path)?;
        let source = path.file_name().unwrap_or_default().to_string_lossy();

        self.session.push_message(Message {
            role: MessageRole::System,
            content: format!(
                "The user has loaded the file '{}' ({} format, {} chars).",
                source,
                extracted.format,
                extracted.content.len(),
            ),
            timestamp: Utc::now(),
            tokens: None,
        });

        let chunk_opts = ChunkOptions {
            max_size: 4000,
            overlap: 600,
            strategy: ChunkStrategy::Heading,
        };
        self.store_knowledge_chunked(&source, &extracted.content, &chunk_opts)?;
        Ok(extracted)
    }

    pub fn store_knowledge_direct(&mut self, source: &str, content: &str) -> Result<()> {
        let checksum = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        let entry = KnowledgeEntry {
            id: Uuid::new_v4().to_string(),
            source: source.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            checksum,
        };

        self.memory.append_knowledge(entry.clone())?;
        self.knowledge_index.add_entry(entry)?;
        Ok(())
    }

    /// Like `store_knowledge_direct` but skips entries already present with the
    /// same `(source, checksum)`. Returns `true` if stored, `false` if skipped.
    pub fn store_knowledge_dedup(&mut self, source: &str, content: &str) -> Result<bool> {
        let entry = KnowledgeEntry {
            id: Uuid::new_v4().to_string(),
            source: source.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            checksum: crate::utils::sha256_digest(content.as_bytes()),
        };

        if self.knowledge_index.add_entry_dedup(entry.clone())? {
            self.memory.append_knowledge(entry)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Fetch a URL, chunk it, and index the chunks (deduplicated by checksum).
    /// Returns the fetched content and the number of chunks actually indexed
    /// (duplicates already present are not counted).
    pub fn fetch_and_ingest(
        &mut self,
        url: &str,
        ingestion: &IngestionConfig,
    ) -> Result<(FetchedContent, usize)> {
        let mut fetcher = WebFetcher::new(ingestion);
        let content = fetcher.fetch_and_retry(url)?;

        let chunk_opts = ChunkOptions {
            max_size: ingestion.chunk_max_size,
            overlap: ingestion.chunk_overlap,
            strategy: ChunkStrategy::Paragraph,
        };

        let chunks = chunker::chunk_text(&content.content, &chunk_opts, url);
        let mut indexed = 0usize;
        for chunk in &chunks {
            if self.store_knowledge_dedup(&chunk.source, &chunk.content)? {
                indexed += 1;
            }
        }

        let title_info = match &content.title {
            Some(t) => format!(" ({})", t),
            None => String::new(),
        };
        self.session.push_message(Message {
            role: MessageRole::System,
            content: format!(
                "The user fetched URL '{}'{} — {} chunks indexed as knowledge.",
                url, title_info, indexed
            ),
            timestamp: Utc::now(),
            tokens: None,
        });

        Ok((content, indexed))
    }

    pub fn fetch_and_ingest_feed(
        &mut self,
        url: &str,
        ingestion: &IngestionConfig,
        max_entries: usize,
    ) -> Result<FeedResult> {
        let mut fetcher = WebFetcher::new(ingestion);
        let entries = crate::feeds::fetch_feed(url, &mut fetcher)?;
        let total = entries.len();
        let feed_title = entries.first().map(|_| url.to_string());

        let mut indexed = 0usize;
        let mut failures = Vec::new();

        let chunk_opts = ChunkOptions {
            max_size: ingestion.chunk_max_size,
            overlap: ingestion.chunk_overlap,
            strategy: ChunkStrategy::Paragraph,
        };

        for entry in entries.iter().take(max_entries) {
            match fetcher.fetch_and_retry(&entry.url) {
                Ok(content) => {
                    let chunks = crate::chunker::chunk_text(&content.content, &chunk_opts, &entry.url);
                    for chunk in &chunks {
                        if self.store_knowledge_dedup(&chunk.source, &chunk.content)? {
                            indexed += 1;
                        }
                    }
                }
                Err(e) => {
                    failures.push(format!("{}: {:#}", entry.title, e));
                }
            }
        }

        self.session.push_message(Message {
            role: MessageRole::System,
            content: format!(
                "Feed '{}' fetched — {} entries found, {} indexed, {} failed.",
                url, total, indexed, failures.len()
            ),
            timestamp: Utc::now(),
            tokens: None,
        });

        Ok(FeedResult {
            feed_title,
            entries_found: total,
            entries_indexed: indexed,
            failures,
        })
    }

    pub fn process_url_batch(
        &mut self,
        urls: &[String],
        ingestion: &IngestionConfig,
    ) -> Result<BatchResult> {
        let mut results = BatchResult::default();
        for url in urls {
            match self.fetch_and_ingest(url, ingestion) {
                Ok((_content, indexed)) => {
                    results.success.push(url.clone());
                    // Real number of chunks indexed, not a size-based estimate.
                    results.total_chunks += indexed as u32;
                }
                Err(e) => {
                    results.failures.push((url.clone(), format!("{:#}", e)));
                }
            }
        }
        Ok(results)
    }

    pub fn store_knowledge_chunked(
        &mut self,
        source: &str,
        content: &str,
        chunk_opts: &ChunkOptions,
    ) -> Result<Vec<String>> {
        let chunks = chunker::chunk_text(content, chunk_opts, source);
        let mut ids = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let checksum = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(chunk.content.as_bytes());
                format!("{:x}", hasher.finalize())
            };

            let entry = KnowledgeEntry {
                id: Uuid::new_v4().to_string(),
                source: chunk.source.clone(),
                content: chunk.content.clone(),
                timestamp: Utc::now(),
                checksum,
            };
            let id = entry.id.clone();
            self.memory.append_knowledge(entry.clone())?;
            self.knowledge_index.add_entry(entry)?;
            ids.push(id);
        }
        Ok(ids)
    }

    pub fn search_knowledge(&self, query: &str, limit: usize) -> Vec<&KnowledgeEntry> {
        self.knowledge_index.search(query, limit)
    }

    pub fn search_knowledge_filtered(
        &self,
        query: &str,
        limit: usize,
        source_filter: Option<&str>,
    ) -> Vec<&KnowledgeEntry> {
        self.knowledge_index.search_with_filter(query, limit, source_filter)
    }

    pub fn knowledge_count(&self) -> usize {
        self.knowledge_index.len()
    }

    pub fn unlearn_language(&mut self, lang_key: &str) -> Result<usize> {
        let prefix = format!("{}/", lang_key);
        self.knowledge_index.remove_by_source_prefix(&prefix)
    }

    /// Download and ingest free programming books for a language from EbookFoundation index.
    /// Returns the number of resources included in the ingested knowledge snippet.
    pub fn download_and_ingest_books(&mut self, language: &str, limit: usize) -> Result<usize> {
        let catalog = BooksCatalog::fetch()?;
        let books = catalog.get_language_books(language).ok_or_else(|| {
            anyhow::anyhow!(format!("Language '{}' not found in catalog", language))
        })?;

        let knowledge = prepare_knowledge_from_books(&books, limit);
        let source = format!("free-programming-books:{}", language);
        self.store_knowledge_direct(&source, &knowledge)?;

        Ok(std::cmp::min(limit, books.resources.len()))
    }

    pub fn reindex_from_mv2(&mut self, data_dir: &std::path::Path) -> Result<()> {
        let index = KnowledgeIndex::rebuild_from_mv2(data_dir)?;
        self.knowledge_index = index;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn switch_model(
        &mut self,
        path: &str,
        n_ctx: u32,
        n_gpu_layers: u32,
        kv_type_k: &str,
        kv_type_v: &str,
        model_name: &str,
        chat_template: &str,
        top_k: i32,
        top_p: f32,
        temp: f32,
    ) -> Result<()> {
        let new_llm = LlamaContext::init(
            path,
            n_ctx,
            n_gpu_layers,
            kv_type_k,
            kv_type_v,
            top_k,
            top_p,
            temp,
        )?;
        self.llm = new_llm;
        self.model_name = model_name.to_string();
        self.context_policy = ContextPolicy::new(n_ctx, self.context_policy.max_tokens());
        // Preserve the developer prompt across the model switch instead of
        // resetting it to the default (previously a silent regression).
        self.prompt_builder = self
            .prompt_builder
            .with_template(ChatTemplate::from_str(chat_template));
        Ok(())
    }

    pub fn ingestion_config(&self) -> IngestionConfig {
        IngestionConfig::default()
    }

    pub fn from_components(
        llm: LlamaContext,
        memory: MemvidWriter,
        knowledge_index: KnowledgeIndex,
        model_name: String,
        session: Session,
        prompt_builder: PromptBuilder,
        context_policy: ContextPolicy,
    ) -> Self {
        Self {
            llm,
            memory,
            knowledge_index,
            model_name,
            session,
            prompt_builder,
            context_policy,
        }
    }

    pub fn interaction_count(&self) -> u64 {
        self.session.interaction_count()
    }

    pub fn memory_summary(&self) -> Result<String> {
        let m = &self.memory.playlist.manifest;
        let conv_count = m.conversation_segments.len();
        let know_count = m.knowledge_segments.len();
        let total_chats: u32 = m
            .conversation_segments
            .iter()
            .map(|s| s.message_count)
            .sum();
        Ok(format!(
            "{} conversations stored ({} segments), {} knowledge entries ({} indexed)",
            total_chats,
            conv_count,
            know_count,
            self.knowledge_index.len()
        ))
    }

    pub fn read_conversation_history(&self) -> Result<Vec<String>> {
        use crate::memvid::reader::Reader;
        let m = &self.memory.playlist.manifest;
        let data_dir = &self.memory.playlist.config.data_dir;
        let conversations_dir = data_dir.join("conversations");
        let mut results = Vec::new();

        if m.conversation_segments.is_empty() {
            results.push("  (no conversation history)".to_string());
            return Ok(results);
        }

        for seg in &m.conversation_segments {
            let seg_path = conversations_dir.join(&seg.filename);
            if !seg_path.exists() {
                continue;
            }
            let mut reader = match Reader::open(&seg_path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let frames = match reader.enumerate() {
                Ok(f) => f,
                Err(_) => continue,
            };
            for frame in &frames {
                if let Ok(text) = reader.read_text(frame.id)
                    && let Ok(batch) = serde_json::from_str::<serde_json::Value>(&text)
                    && let Some(messages) = batch["messages"].as_array()
                {
                    for msg in messages {
                        let role = msg["role"].as_str().unwrap_or("unknown");
                        let content = msg["content"].as_str().unwrap_or("");
                        let label = match role {
                            "user" => "You",
                            "assistant" => "Assistant",
                            "system" => "System",
                            "tool" => "Tool",
                            _ => role,
                        };
                        // UTF-8-safe truncation: byte slicing `&content[..500]`
                        // panics when byte 500 falls inside a multi-byte char.
                        let display = crate::utils::truncate_chars(content, 500, "...");
                        results.push(format!("  {}: {}", label, display));
                    }
                }
            }
        }

        Ok(results)
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if self.llm.is_valid()
            && let Err(e) = self
                .session
                .flush(&self.llm, &self.model_name, &mut self.memory)
        {
            tracing::warn!("Failed to flush session on drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_policy::ContextPolicy;
    use crate::prompt::ChatTemplate;
    use crate::types::WriterConfig;
    use std::path::Path;

    fn test_agent(data_dir: &Path) -> Agent {
        let writer_config = WriterConfig {
            data_dir: data_dir.to_path_buf(),
            ..Default::default()
        };
        let memory = MemvidWriter::init(writer_config).unwrap();
        let knowledge_index = KnowledgeIndex::load(data_dir).unwrap();

        Agent {
            llm: LlamaContext::null(),
            memory,
            knowledge_index,
            model_name: "test-model".to_string(),
            session: Session::new(),
            prompt_builder: PromptBuilder::new(ChatTemplate::ChatML),
            context_policy: ContextPolicy::new(4096, 2048),
        }
    }

    #[test]
    fn store_knowledge_direct_adds_to_both_stores() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        assert_eq!(agent.knowledge_count(), 0);

        agent
            .store_knowledge_direct("test-source", "test content")
            .unwrap();
        assert_eq!(agent.knowledge_count(), 1);

        let results = agent.search_knowledge("test", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "test-source");
        assert_eq!(results[0].content, "test content");
    }

    #[test]
    fn store_knowledge_dedup_skips_repeat_ingestion() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        assert!(agent.store_knowledge_dedup("doc.txt", "hello").unwrap());
        assert_eq!(agent.knowledge_count(), 1);

        // Re-ingesting identical (source, content) is a no-op.
        assert!(!agent.store_knowledge_dedup("doc.txt", "hello").unwrap());
        assert_eq!(agent.knowledge_count(), 1);

        // Different content under the same source is still stored.
        assert!(agent.store_knowledge_dedup("doc.txt", "world").unwrap());
        assert_eq!(agent.knowledge_count(), 2);
    }

    #[test]
    fn store_knowledge_direct_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        agent.store_knowledge_direct("src1", "hello world").unwrap();
        agent.store_knowledge_direct("src2", "foo bar").unwrap();
        assert_eq!(agent.knowledge_count(), 2);

        let results = agent.search_knowledge("hello", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "src1");
    }

    #[test]
    fn search_knowledge_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        for i in 0..5 {
            agent
                .store_knowledge_direct("src", &format!("content number {}", i))
                .unwrap();
        }

        let results = agent.search_knowledge("content", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_knowledge_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        agent
            .store_knowledge_direct("src", "unique content")
            .unwrap();
        let results = agent.search_knowledge("nonexistent", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn knowledge_count_initially_zero() {
        let dir = tempfile::tempdir().unwrap();
        let agent = test_agent(dir.path());
        assert_eq!(agent.knowledge_count(), 0);
    }

    #[test]
    fn memory_summary_empty() {
        let dir = tempfile::tempdir().unwrap();
        let agent = test_agent(dir.path());

        let summary = agent.memory_summary().unwrap();
        assert!(summary.contains("0 conversations"));
        assert!(summary.contains("0 knowledge entries"));
        assert!(summary.contains("0 indexed"));
    }

    #[test]
    fn memory_summary_after_knowledge_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        agent.store_knowledge_direct("src", "some content").unwrap();
        let summary = agent.memory_summary().unwrap();
        // Indexed entries visible immediately; segments appear after flush
        assert!(summary.contains("1 indexed"));
    }

    #[test]
    fn ingest_raw_appends_system_message() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        assert_eq!(agent.session.messages().len(), 0);
        agent.ingest_raw("test.txt", "some file content").unwrap();

        assert_eq!(agent.session.messages().len(), 1);
        assert_eq!(agent.session.messages()[0].role, MessageRole::System);
        assert!(agent.session.messages()[0].content.contains("test.txt"));
        assert!(
            agent.session.messages()[0]
                .content
                .contains("some file content")
        );
    }

    #[test]
    fn ingest_knowledge_adds_to_session_and_index() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());

        assert_eq!(agent.knowledge_count(), 0);
        assert_eq!(agent.session.messages().len(), 0);

        agent.ingest_knowledge("doc.txt", "important info").unwrap();

        assert_eq!(agent.session.messages().len(), 1);
        assert_eq!(agent.knowledge_count(), 1);
    }

    #[test]
    fn interaction_count_starts_at_zero() {
        let dir = tempfile::tempdir().unwrap();
        let agent = test_agent(dir.path());
        assert_eq!(agent.interaction_count(), 0);
    }

    #[test]
    fn search_knowledge_empty_after_store_delegates_to_index() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        agent.store_knowledge_direct("a", "alpha").unwrap();
        agent.store_knowledge_direct("b", "beta").unwrap();

        let res = agent.search_knowledge("alpha", 10);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].source, "a");
    }

    #[test]
    fn store_knowledge_chunked_returns_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        let opts = ChunkOptions::default();
        let ids = agent
            .store_knowledge_chunked("src", "some content to chunk", &opts)
            .unwrap();
        assert!(!ids.is_empty());
        assert_eq!(agent.knowledge_count(), ids.len());
    }

    #[test]
    fn store_knowledge_chunked_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        let opts = ChunkOptions::default();
        let ids = agent.store_knowledge_chunked("src", "", &opts).unwrap();
        assert!(ids.is_empty());
        assert_eq!(agent.knowledge_count(), 0);
    }

    #[test]
    fn store_knowledge_chunked_large_content() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        let opts = ChunkOptions {
            max_size: 20,
            overlap: 5,
            strategy: ChunkStrategy::Fixed,
        };
        let content = "word ".repeat(100);
        let ids = agent
            .store_knowledge_chunked("src", &content, &opts)
            .unwrap();
        assert!(ids.len() > 1);
    }

    #[test]
    fn ingest_file_txt() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();
        let result = agent.ingest_file(&file_path).unwrap();
        assert_eq!(result.content, "hello world");
        assert_eq!(result.format, crate::types::Format::Text);
        assert!(agent.knowledge_count() > 0);
        assert_eq!(agent.session.messages().len(), 1);
    }

    #[test]
    fn ingest_file_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        let result = agent.ingest_file(&dir.path().join("nonexistent.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn batch_result_construction() {
        let result = BatchResult {
            success: vec!["a".into(), "b".into()],
            failures: vec![("c".into(), "error".into())],
            total_chunks: 10,
        };
        assert_eq!(result.success.len(), 2);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.total_chunks, 10);
    }

    #[test]
    fn search_knowledge_special_chars() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        agent
            .store_knowledge_direct("test", "hello (world) [test]")
            .unwrap();
        let res = agent.search_knowledge("(world)", 10);
        assert_eq!(res.len(), 1);
    }

    #[test]
    fn search_knowledge_empty_query() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        agent.store_knowledge_direct("src", "content").unwrap();
        let res = agent.search_knowledge("", 10);
        assert!(res.is_empty());
    }

    #[test]
    fn search_knowledge_unicode() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        agent.store_knowledge_direct("es", "café y ñoño").unwrap();
        let res = agent.search_knowledge("café", 10);
        assert_eq!(res.len(), 1);
    }

    #[test]
    fn knowledge_count_after_chunked_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        assert_eq!(agent.knowledge_count(), 0);
        let opts = ChunkOptions {
            max_size: 20,
            overlap: 5,
            strategy: ChunkStrategy::Fixed,
        };
        agent
            .store_knowledge_chunked("src", &"word ".repeat(50), &opts)
            .unwrap();
        assert!(agent.knowledge_count() > 1);
    }

    #[test]
    fn memory_summary_after_chunked_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = test_agent(dir.path());
        let opts = ChunkOptions::default();
        agent
            .store_knowledge_chunked("src", "some content", &opts)
            .unwrap();
        let summary = agent.memory_summary().unwrap();
        assert!(summary.contains("1 indexed"));
    }
}
