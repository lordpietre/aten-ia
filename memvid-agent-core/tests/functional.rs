use std::path::Path;

// ---------------------------------------------------------------------------
// Functional tests for aten-ia.
// All tests are isolated via tempfile::tempdir and do NOT require a GGUF model.
// Only public APIs are used (no private functions).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Config lifecycle
// ---------------------------------------------------------------------------

#[test]
fn config_save_and_reload_preserves_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let cfg = test_config();
    cfg.save_to_path(&config_path).unwrap();

    let loaded = memvid_agent_core::config::Config::load_or_create_with_path(&config_path).unwrap();
    assert_eq!(loaded.model.path, cfg.model.path);
    assert_eq!(loaded.model.n_ctx, cfg.model.n_ctx);
    assert_eq!(loaded.model.chat_template, cfg.model.chat_template);
    assert_eq!(loaded.generation.top_k, cfg.generation.top_k);
    assert_eq!(loaded.generation.temp, cfg.generation.temp);
    assert_eq!(loaded.generation.max_tokens, cfg.generation.max_tokens);
    assert_eq!(loaded.api.enabled, cfg.api.enabled);
    assert_eq!(loaded.api.port, cfg.api.port);
    assert_eq!(loaded.ingestion.timeout_seconds, 30);
    assert_eq!(loaded.ingestion.max_size_bytes, 5 * 1024 * 1024);
}

#[test]
fn config_creates_default_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    assert!(!config_path.exists());

    let cfg = memvid_agent_core::config::Config::load_or_create_with_path(&config_path).unwrap();
    assert!(config_path.exists());
    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.model.path, "models/default-model.gguf");
    assert!(!cfg.api.enabled);
    assert!(cfg.languages.installed.is_empty());
}

#[test]
fn config_validate_rejects_invalid() {
    let mut cfg = test_config();
    cfg.model.n_ctx = 0;
    assert!(cfg.validate().is_err());

    cfg.model.n_ctx = 4096;
    cfg.generation.max_tokens = 0;
    assert!(cfg.validate().is_err());

    cfg.generation.temp = 0.0;
    cfg.api.port = 0;
    assert!(cfg.validate().is_err());
}

#[test]
fn config_env_overrides_model_path() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    unsafe {
        std::env::set_var("MODEL_PATH", "/env/custom.gguf");
    }

    let cfg = memvid_agent_core::config::Config::load_or_create_with_path(&config_path).unwrap();
    assert_eq!(cfg.model.path, "/env/custom.gguf");

    unsafe {
        std::env::remove_var("MODEL_PATH");
    }
}

#[test]
fn config_languages_mark_installed() {
    use memvid_agent_core::config::LanguagesConfig;

    let mut lang = LanguagesConfig {
        installed: Vec::new(),
    };
    lang.mark_installed("rust");
    assert_eq!(lang.installed, vec!["rust"]);
    lang.mark_installed("rust");
    assert_eq!(lang.installed.len(), 1);
    lang.mark_installed("python");
    assert_eq!(lang.installed.len(), 2);
    lang.mark_installed("");
    assert_eq!(lang.installed.len(), 3);
}

// ---------------------------------------------------------------------------
// Knowledge pipeline: store -> search -> persist -> reload
// ---------------------------------------------------------------------------

#[test]
fn knowledge_index_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    assert!(index.is_empty());

    index
        .add_entry(knowledge_entry(
            "src1",
            "Rust is a systems programming language",
        ))
        .unwrap();
    index
        .add_entry(knowledge_entry("src2", "Python is great for data science"))
        .unwrap();
    index
        .add_entry(knowledge_entry("src3", "JavaScript runs in the browser"))
        .unwrap();
    assert_eq!(index.len(), 3);

    let results = index.search("python", 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source, "src2");

    drop(index);

    let reloaded = KnowledgeIndex::load(dir.path()).unwrap();
    assert_eq!(reloaded.len(), 3);
    let results = reloaded.search("rust", 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source, "src1");
}

#[test]
fn knowledge_index_search_scored_by_relevance() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    index
        .add_entry(knowledge_entry("a", "python python python python"))
        .unwrap();
    index.add_entry(knowledge_entry("b", "python")).unwrap();

    let results = index.search("python", 5);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].source, "a");
}

#[test]
fn knowledge_index_search_empty_and_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    index
        .add_entry(knowledge_entry("test", "hello world"))
        .unwrap();

    assert!(index.search("", 5).is_empty());
    assert!(index.search("   ", 5).is_empty());
}

#[test]
fn knowledge_index_search_unicode() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    index
        .add_entry(knowledge_entry("es", "cafe y nino y nihongo"))
        .unwrap();

    assert_eq!(index.search("cafe", 5).len(), 1);
    assert_eq!(index.search("nino", 5).len(), 1);
}

#[test]
fn knowledge_index_rebuild_from_jsonl_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let entries;
    {
        let mut index = KnowledgeIndex::load(dir.path()).unwrap();
        index.add_entry(knowledge_entry("a", "alpha")).unwrap();
        index.add_entry(knowledge_entry("b", "beta")).unwrap();
        entries = index.len();
    }
    let rebuilt = KnowledgeIndex::rebuild_from_jsonl(dir.path()).unwrap();
    assert_eq!(rebuilt.len(), entries);
}

#[test]
fn knowledge_index_add_entries_batch() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    let entries = vec![
        knowledge_entry("a", "alpha"),
        knowledge_entry("b", "beta"),
        knowledge_entry("c", "gamma"),
    ];
    index.add_entries(&entries).unwrap();
    assert_eq!(index.len(), 3);
    assert_eq!(index.add_entries(&[]).unwrap(), ());
}

#[test]
fn knowledge_index_search_special_chars() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    index
        .add_entry(knowledge_entry("test", "hello (world) [test] {foo} &bar$"))
        .unwrap();

    assert_eq!(index.search("world", 5).len(), 1);
}

#[test]
fn knowledge_index_load_skips_malformed_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge_index.jsonl");
    std::fs::write(&path, "{valid}\nnot json\n{\"also\": \"bad\"").unwrap();

    use memvid_agent_core::retrieval::KnowledgeIndex;
    let index = KnowledgeIndex::load(dir.path()).unwrap();
    assert_eq!(index.len(), 0);
}

#[test]
fn knowledge_index_chunk_text_sizes() {
    use memvid_agent_core::retrieval::KnowledgeIndex;

    assert_eq!(KnowledgeIndex::chunk_text("").len(), 1);
    assert_eq!(
        KnowledgeIndex::chunk_text("a".repeat(100).as_str()).len(),
        1
    );

    let chunks = KnowledgeIndex::chunk_text("word ".repeat(5000).as_str());
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(chunk.len() <= 4000);
    }
}

#[test]
fn knowledge_index_entries_iterator() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    index.add_entry(knowledge_entry("a", "alpha")).unwrap();
    index.add_entry(knowledge_entry("b", "beta")).unwrap();

    let sources: Vec<&str> = index.entries().iter().map(|e| e.source.as_str()).collect();
    assert_eq!(sources, vec!["a", "b"]);
}

#[test]
fn knowledge_index_no_match_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let mut index = KnowledgeIndex::load(dir.path()).unwrap();
    index
        .add_entry(knowledge_entry("python", "Python is fun"))
        .unwrap();
    assert!(index.search("rust", 5).is_empty());
}

// ---------------------------------------------------------------------------
// Agent pipeline: store_knowledge -> search -> memory_summary
// ---------------------------------------------------------------------------

#[test]
fn agent_store_and_search_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    agent
        .store_knowledge_direct("test-source", "unique searchable content")
        .unwrap();
    assert_eq!(agent.knowledge_count(), 1);

    let results = agent.search_knowledge("unique", 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source, "test-source");
}

#[test]
fn agent_store_knowledge_chunked() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};
    let opts = ChunkOptions {
        max_size: 20,
        overlap: 5,
        strategy: ChunkStrategy::Fixed,
    };
    let ids = agent
        .store_knowledge_chunked("src", &"word ".repeat(100), &opts)
        .unwrap();
    assert!(ids.len() > 1);
    assert!(agent.knowledge_count() > 1);
}

#[test]
fn agent_store_knowledge_chunked_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());
    let opts = memvid_agent_core::types::ChunkOptions::default();
    let ids = agent.store_knowledge_chunked("src", "", &opts).unwrap();
    assert!(ids.is_empty());
    assert_eq!(agent.knowledge_count(), 0);
}

#[test]
fn agent_memory_summary_tracks_counts() {
    let dir = tempfile::tempdir().unwrap();
    let agent = test_agent(dir.path());
    let summary = agent.memory_summary().unwrap();
    assert!(summary.contains("0 conversations"));
    assert!(summary.contains("0 knowledge entries"));

    let mut agent = agent;
    agent.store_knowledge_direct("src", "content").unwrap();
    let summary = agent.memory_summary().unwrap();
    assert!(summary.contains("1 indexed"));
}

#[test]
fn agent_ingest_raw_appends_system_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    agent.ingest_raw("doc.txt", "some content").unwrap();
    agent.ingest_raw("doc2.txt", "more content").unwrap();
    assert_eq!(agent.interaction_count(), 0);
}

#[test]
fn agent_ingest_knowledge_increments_count() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    agent.ingest_knowledge("doc.txt", "important info").unwrap();
    assert!(agent.knowledge_count() > 0);
}

#[test]
fn agent_interaction_count_starts_zero() {
    let dir = tempfile::tempdir().unwrap();
    let agent = test_agent(dir.path());
    assert_eq!(agent.interaction_count(), 0);
}

#[test]
fn agent_search_knowledge_honors_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());
    for i in 0..10 {
        agent
            .store_knowledge_direct("src", &format!("content number {}", i))
            .unwrap();
    }
    let results = agent.search_knowledge("content", 3);
    assert_eq!(results.len(), 3);
}

#[test]
fn agent_search_knowledge_special_chars() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());
    agent
        .store_knowledge_direct("test", "hello (world) [test]")
        .unwrap();
    let res = agent.search_knowledge("(world)", 10);
    assert_eq!(res.len(), 1);
}

#[test]
fn agent_search_knowledge_empty_query() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());
    agent.store_knowledge_direct("src", "content").unwrap();
    assert!(agent.search_knowledge("", 10).is_empty());
}

#[test]
fn agent_search_knowledge_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());
    agent
        .store_knowledge_direct("src", "unique content")
        .unwrap();
    let results = agent.search_knowledge("nonexistent", 10);
    assert!(results.is_empty());
}

#[test]
fn agent_batch_result_construction() {
    use memvid_agent_core::agent::BatchResult;
    let result = BatchResult {
        success: vec!["a".into(), "b".into()],
        failures: vec![("c".into(), "error".into())],
        total_chunks: 10,
    };
    assert_eq!(result.success.len(), 2);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.total_chunks, 10);
}

// ---------------------------------------------------------------------------
// Session behavior
// ---------------------------------------------------------------------------

#[test]
fn session_push_and_take_batch() {
    use chrono::Utc;
    use memvid_agent_core::session::Session;
    use memvid_agent_core::types::{Message, MessageRole};

    let mut session = Session::new();
    session.push_message(Message {
        role: MessageRole::User,
        content: "hello".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    session.push_message(Message {
        role: MessageRole::Assistant,
        content: "world".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    assert_eq!(session.messages().len(), 2);

    let taken = session.take_batch();
    assert_eq!(taken.len(), 2);
    assert!(session.messages().is_empty());
}

#[test]
fn session_take_batch_empty() {
    use memvid_agent_core::session::Session;
    let mut session = Session::new();
    assert!(session.take_batch().is_empty());
}

#[test]
fn session_increment_interactions() {
    use memvid_agent_core::session::Session;
    let mut session = Session::new();
    assert_eq!(session.interaction_count(), 0);
    session.increment_interactions();
    assert_eq!(session.interaction_count(), 1);
    for _ in 0..99 {
        session.increment_interactions();
    }
    assert_eq!(session.interaction_count(), 100);
}

#[test]
fn session_estimate_tokens_accuracy() {
    use memvid_agent_core::session::estimate_tokens;
    assert_eq!(estimate_tokens("hello"), 1);
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("a".repeat(40).as_str()), 10);
    assert_eq!(estimate_tokens("   "), 0);
}

#[test]
fn session_push_message_special_chars() {
    use chrono::Utc;
    use memvid_agent_core::session::Session;
    use memvid_agent_core::types::{Message, MessageRole};

    let mut session = Session::new();
    session.push_message(Message {
        role: MessageRole::User,
        content: "hello\nworld\t\r\n".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    session.push_message(Message {
        role: MessageRole::Assistant,
        content: "cafe nino nihongo".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    session.push_message(Message {
        role: MessageRole::System,
        content: "".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    assert_eq!(session.messages().len(), 3);
    assert_eq!(session.messages()[0].content, "hello\nworld\t\r\n");
    assert_eq!(session.messages()[2].content, "");
}

#[test]
fn session_push_very_long_message() {
    use chrono::Utc;
    use memvid_agent_core::session::Session;
    use memvid_agent_core::types::{Message, MessageRole};

    let mut session = Session::new();
    let long = "a".repeat(100_000);
    session.push_message(Message {
        role: MessageRole::User,
        content: long.clone(),
        timestamp: Utc::now(),
        tokens: None,
    });
    assert_eq!(session.messages().len(), 1);
    assert_eq!(session.messages()[0].content.len(), 100_000);
}

#[test]
fn session_take_batch_all_roles() {
    use chrono::Utc;
    use memvid_agent_core::session::Session;
    use memvid_agent_core::types::{Message, MessageRole};

    let mut session = Session::new();
    session.push_message(Message {
        role: MessageRole::User,
        content: "a".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    session.push_message(Message {
        role: MessageRole::Assistant,
        content: "b".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    session.push_message(Message {
        role: MessageRole::System,
        content: "c".into(),
        timestamp: Utc::now(),
        tokens: None,
    });
    let taken = session.take_batch();
    assert_eq!(taken.len(), 3);
    assert!(session.messages().is_empty());
}

// ---------------------------------------------------------------------------
// Context policy trimming
// ---------------------------------------------------------------------------

#[test]
fn context_policy_budget_calculation() {
    use memvid_agent_core::context_policy::ContextPolicy;
    let policy = ContextPolicy::new(4096, 2048);
    assert_eq!(policy.prompt_budget(), 2048);
    assert_eq!(policy.n_ctx(), 4096);
    assert_eq!(policy.max_tokens(), 2048);
}

#[test]
fn context_policy_trim_messages_empty() {
    use memvid_agent_core::context_policy::ContextPolicy;
    let policy = ContextPolicy::new(4096, 2048);
    let result = policy.trim_messages("sys", &[], "input", |s| s.len() / 4);
    assert_eq!(result.len(), 0);
}

#[test]
fn context_policy_trim_messages_only_system() {
    use chrono::Utc;
    use memvid_agent_core::context_policy::ContextPolicy;
    use memvid_agent_core::types::{Message, MessageRole};

    let policy = ContextPolicy::new(256, 128);
    let msgs = vec![
        Message {
            role: MessageRole::System,
            content: "sys1".into(),
            timestamp: Utc::now(),
            tokens: None,
        },
        Message {
            role: MessageRole::System,
            content: "sys2".into(),
            timestamp: Utc::now(),
            tokens: None,
        },
    ];
    let result = policy.trim_messages("dev", &msgs, "input", |s| s.len() / 4);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].content, "sys1");
    assert_eq!(result[1].content, "sys2");
}

#[test]
fn context_policy_trim_messages_preserves_system_order() {
    use chrono::Utc;
    use memvid_agent_core::context_policy::ContextPolicy;
    use memvid_agent_core::types::{Message, MessageRole};

    let policy = ContextPolicy::new(4096, 2048);
    let msgs = vec![
        Message {
            role: MessageRole::System,
            content: "first".into(),
            timestamp: Utc::now(),
            tokens: None,
        },
        Message {
            role: MessageRole::User,
            content: "user1".into(),
            timestamp: Utc::now(),
            tokens: None,
        },
        Message {
            role: MessageRole::System,
            content: "second".into(),
            timestamp: Utc::now(),
            tokens: None,
        },
        Message {
            role: MessageRole::User,
            content: "user2".into(),
            timestamp: Utc::now(),
            tokens: None,
        },
    ];
    let result = policy.trim_messages("dev", &msgs, "input", |s| s.len() / 4);
    assert_eq!(result[0].role, MessageRole::System);
    assert_eq!(result[0].content, "first");
    assert_eq!(result[1].role, MessageRole::System);
    assert_eq!(result[1].content, "second");
}

#[test]
fn context_policy_zero_budget_returns_empty() {
    use memvid_agent_core::context_policy::ContextPolicy;
    let policy = ContextPolicy::new(64, 64);
    let msgs = vec![msg("user", "hi")];
    let result = policy.trim_messages("sys", &msgs, "input", |s| s.len() / 4);
    assert_eq!(result.len(), 0);
}

#[test]
fn context_policy_prompt_budget_low_n_ctx() {
    use memvid_agent_core::context_policy::ContextPolicy;
    let policy = ContextPolicy::new(64, 128);
    assert_eq!(policy.prompt_budget(), 0);
}

// ---------------------------------------------------------------------------
// Prompt building - all templates
// ---------------------------------------------------------------------------

#[test]
fn prompt_chatml_includes_system_and_history() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::ChatML);
    let msgs = vec![msg("user", "hi"), msg("assistant", "hello there")];
    let result = builder.build(&msgs, "how are you?", &[]);
    assert!(result.contains("<|im_start|>system"));
    assert!(result.contains("expert software engineer"));
    assert!(result.contains("<|im_start|>user\nhi\n<|im_end|>"));
    assert!(result.contains("<|im_start|>assistant\nhello there\n<|im_end|>"));
    assert!(result.contains("<|im_start|>user\nhow are you?\n<|im_end|>"));
    assert!(result.contains("<|im_start|>assistant\n"));
}

#[test]
fn prompt_chatml_with_rag_context() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::ChatML);
    let result = builder.build(&[], "question", &["relevant docs here".to_string()]);
    assert!(result.contains("## Relevant context"));
    assert!(result.contains("relevant docs here"));
}

#[test]
fn prompt_chatml_with_system_in_history() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::ChatML);
    let msgs = vec![msg("system", "user loaded file x.py")];
    let result = builder.build(&msgs, "explain it", &[]);
    assert!(result.contains("<|im_start|>system\nuser loaded file x.py"));
}

#[test]
fn prompt_llama3_template() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::Llama3);
    let result = builder.build(&[], "hello", &[]);
    assert!(result.starts_with("<|begin_of_text|>"));
    assert!(result.contains("<|start_header_id|>system<|end_header_id|>"));
    assert!(result.contains("<|start_header_id|>user<|end_header_id|>\n\nhello"));
    assert!(result.contains("<|start_header_id|>assistant<|end_header_id|>"));
}

#[test]
fn prompt_mistral_template() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::Mistral);
    let result = builder.build(&[], "hello", &[]);
    assert!(result.starts_with("[INST]"));
    assert!(result.contains("expert software engineer"));
    assert!(result.ends_with("[INST] hello [/INST]\n"));
}

#[test]
fn prompt_raw_template_returns_input_only() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::Raw);
    let msgs = vec![msg("user", "ignored")];
    let result = builder.build(&msgs, "hello world", &["rag context".to_string()]);
    assert_eq!(result, "hello world");
}

#[test]
fn prompt_custom_developer_prompt() {
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};

    let builder = PromptBuilder::new(ChatTemplate::ChatML).with_developer_prompt("You are a poet.");
    let result = builder.build(&[], "write a poem", &[]);
    assert!(result.contains("You are a poet."));
    assert!(!result.contains("expert software engineer"));
}

#[test]
fn prompt_template_from_str() {
    use memvid_agent_core::prompt::ChatTemplate;
    assert_eq!(ChatTemplate::from_str("chatml"), ChatTemplate::ChatML);
    assert_eq!(ChatTemplate::from_str("CHATML"), ChatTemplate::ChatML);
    assert_eq!(ChatTemplate::from_str("llama3"), ChatTemplate::Llama3);
    assert_eq!(ChatTemplate::from_str("mistral"), ChatTemplate::Mistral);
    assert_eq!(ChatTemplate::from_str("unknown"), ChatTemplate::Raw);
    assert_eq!(ChatTemplate::from_str(""), ChatTemplate::Raw);
}

// ---------------------------------------------------------------------------
// Format detection (types)
// ---------------------------------------------------------------------------

#[test]
fn format_from_extension_detection() {
    use memvid_agent_core::types::Format;
    assert_eq!(Format::from_extension(Path::new("doc.pdf")), Format::Pdf);
    assert_eq!(Format::from_extension(Path::new("book.epub")), Format::Epub);
    assert_eq!(
        Format::from_extension(Path::new("readme.md")),
        Format::Markdown
    );
    assert_eq!(
        Format::from_extension(Path::new("readme.markdown")),
        Format::Markdown
    );
    assert_eq!(
        Format::from_extension(Path::new("index.html")),
        Format::Html
    );
    assert_eq!(Format::from_extension(Path::new("page.htm")), Format::Html);
    assert_eq!(Format::from_extension(Path::new("file.txt")), Format::Text);
    assert_eq!(Format::from_extension(Path::new("Makefile")), Format::Text);
    assert_eq!(Format::from_extension(Path::new("doc.PDF")), Format::Pdf);
    assert_eq!(
        Format::from_extension(Path::new("file.backup.pdf")),
        Format::Pdf
    );
    assert_eq!(Format::from_extension(Path::new("doc.Pdf")), Format::Pdf);
}

#[test]
fn format_display() {
    use memvid_agent_core::types::Format;
    assert_eq!(format!("{}", Format::Text), "text");
    assert_eq!(format!("{}", Format::Markdown), "markdown");
    assert_eq!(format!("{}", Format::Html), "html");
    assert_eq!(format!("{}", Format::Pdf), "pdf");
    assert_eq!(format!("{}", Format::Epub), "epub");
}

// ---------------------------------------------------------------------------
// Chunking strategies
// ---------------------------------------------------------------------------

#[test]
fn chunking_all_strategies_produce_correct_chunks() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let text = "# Intro\nhello\n# Details\nmore info here\n# Conclusion\nbye";

    let heading_chunks = chunker::chunk_text(
        text,
        &ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        },
        "doc",
    );
    assert_eq!(heading_chunks.len(), 3);
    assert_eq!(heading_chunks[0].heading, Some("# Intro".into()));
    assert_eq!(heading_chunks[1].heading, Some("# Details".into()));

    let para_chunks = chunker::chunk_text(
        "word\n".repeat(100).as_str(),
        &ChunkOptions {
            max_size: 50,
            overlap: 10,
            strategy: ChunkStrategy::Paragraph,
        },
        "src",
    );
    assert!(para_chunks.len() >= 2);

    let fixed_chunks = chunker::chunk_text(
        "ABCDEFGHIJ",
        &ChunkOptions {
            max_size: 5,
            overlap: 2,
            strategy: ChunkStrategy::Fixed,
        },
        "fixed",
    );
    assert_eq!(fixed_chunks.len(), 3);
    assert_eq!(fixed_chunks[0].content, "ABCDE");
    assert_eq!(fixed_chunks[1].content, "DEFGH");
    assert_eq!(fixed_chunks[2].content, "GHIJ");
}

#[test]
fn chunking_empty_and_whitespace() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let opts = ChunkOptions {
        max_size: 50,
        overlap: 5,
        strategy: ChunkStrategy::Paragraph,
    };
    assert!(chunker::chunk_text("", &opts, "empty").is_empty());

    let chunks = chunker::chunk_text("   \n  \n  ", &opts, "ws");
    assert!(chunks.is_empty() || chunks.iter().all(|c| c.content.trim().is_empty()));
}

#[test]
fn chunking_fixed_various_overlaps() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let text = "ABCDEFGHIJ";

    assert_eq!(
        chunker::chunk_text(
            text,
            &ChunkOptions {
                max_size: 5,
                overlap: 0,
                strategy: ChunkStrategy::Fixed,
            },
            "fixed"
        )
        .len(),
        2
    );

    assert_eq!(
        chunker::chunk_text(
            text,
            &ChunkOptions {
                max_size: 5,
                overlap: 5,
                strategy: ChunkStrategy::Fixed,
            },
            "fixed"
        )
        .len(),
        6
    );

    assert_eq!(
        chunker::chunk_text(
            text,
            &ChunkOptions {
                max_size: 3,
                overlap: 0,
                strategy: ChunkStrategy::Fixed,
            },
            "fixed"
        )
        .len(),
        4
    );
}

#[test]
fn chunking_heading_consecutive_and_trailing() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let opts = ChunkOptions {
        max_size: 200,
        overlap: 10,
        strategy: ChunkStrategy::Heading,
    };

    let chunks = chunker::chunk_text("# H1\n# H2\ncontent", &opts, "doc");
    assert_eq!(chunks.len(), 2);

    let chunks = chunker::chunk_text("content\n# Heading", &opts, "doc");
    assert_eq!(chunks.len(), 1);
}

#[test]
fn chunking_heading_mixed_levels() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let text = "# H1\ncontent\n## H2\nmore\n### H3\ndetails";
    let chunks = chunker::chunk_text(
        text,
        &ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        },
        "doc",
    );
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].heading, Some("# H1".into()));
    assert_eq!(chunks[1].heading, Some("## H2".into()));
    assert_eq!(chunks[2].heading, Some("### H3".into()));
}

#[test]
fn chunking_deduplicate() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let chunks = chunker::chunk_and_deduplicate(
        "hello\nhello\nworld",
        &ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Paragraph,
        },
        "dedup",
    );
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].content, "hello");
    assert_eq!(chunks[1].content, "world");
}

#[test]
fn chunking_deduplicate_empty() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    assert!(
        chunker::chunk_and_deduplicate(
            "",
            &ChunkOptions {
                max_size: 5,
                overlap: 0,
                strategy: ChunkStrategy::Paragraph,
            },
            "dedup"
        )
        .is_empty()
    );
}

#[test]
fn chunking_deduplicate_all_unique() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let chunks = chunker::chunk_and_deduplicate(
        "hello\nworld\nfoo",
        &ChunkOptions {
            max_size: 5,
            overlap: 0,
            strategy: ChunkStrategy::Paragraph,
        },
        "dedup",
    );
    assert_eq!(chunks.len(), 3);
}

#[test]
fn chunking_heading_no_headings() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let chunks = chunker::chunk_text(
        "plain text\nwithout any\nheadings",
        &ChunkOptions {
            max_size: 200,
            overlap: 10,
            strategy: ChunkStrategy::Heading,
        },
        "plain",
    );
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].heading.is_none());
}

#[test]
fn chunking_fixed_exact_size() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    assert_eq!(
        chunker::chunk_text(
            "ABCDE",
            &ChunkOptions {
                max_size: 5,
                overlap: 0,
                strategy: ChunkStrategy::Fixed,
            },
            "fixed"
        )
        .len(),
        1
    );

    assert_eq!(
        chunker::chunk_text(
            "ABCDEF",
            &ChunkOptions {
                max_size: 5,
                overlap: 0,
                strategy: ChunkStrategy::Fixed,
            },
            "fixed"
        )
        .len(),
        2
    );
}

#[test]
fn chunking_unicode_text() {
    use memvid_agent_core::chunker;
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};

    let chunks = chunker::chunk_text(
        "nino y cafe",
        &ChunkOptions {
            max_size: 50,
            overlap: 5,
            strategy: ChunkStrategy::Fixed,
        },
        "unicode",
    );
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.contains("nino"));
}

// ---------------------------------------------------------------------------
// HTML extraction
// ---------------------------------------------------------------------------

#[test]
fn html_to_text_strips_all_tags() {
    use memvid_agent_core::extractor::html_to_text;
    assert_eq!(html_to_text("<p>Hello <b>world</b></p>"), "Hello world");
    assert_eq!(
        html_to_text("<div><p>first</p><p>second</p></div>"),
        "first second"
    );
}

#[test]
fn html_to_text_removes_scripts_and_styles() {
    use memvid_agent_core::extractor::html_to_text;
    let html =
        "<p>Hello</p><script>alert('xss')</script><style>.cls{color:red}</style><p>World</p>";
    assert_eq!(html_to_text(html), "Hello World");
}

#[test]
fn html_to_text_handles_entities() {
    use memvid_agent_core::extractor::html_to_text;
    assert_eq!(html_to_text("<p>AT&amp;T &lt;foo&gt;</p>"), "AT&T <foo>");
    assert_eq!(html_to_text("<p>&#65;&#x42;&#x43;</p>"), "ABC");
}

#[test]
fn html_to_text_empty_and_no_html() {
    use memvid_agent_core::extractor::html_to_text;
    assert_eq!(html_to_text(""), "");
    assert_eq!(html_to_text("plain text"), "plain text");
    assert_eq!(html_to_text("<div><span></span></div>"), "");
}

#[test]
fn html_to_text_malformed_handling() {
    use memvid_agent_core::extractor::html_to_text;
    let text = html_to_text("<p>hello</p><div>world</p></div>");
    assert!(text.contains("hello"));
    assert!(text.contains("world"));
}

// ---------------------------------------------------------------------------
// HTML -> Markdown
// ---------------------------------------------------------------------------

#[test]
fn html_to_markdown_basic_constructs() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<h1>Title</h1><p>Hello <strong>world</strong></p>");
    assert!(md.contains("# Title"));
    assert!(md.contains("**world**"));
}

#[test]
fn html_to_markdown_links_and_images() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown(r#"<a href="https://example.com">click here</a>"#);
    assert!(md.contains("[click here](https://example.com)"));

    let md = html_to_markdown(r#"<img src="pic.png" alt="photo">"#);
    assert!(md.contains("![photo](pic.png)"));
}

#[test]
fn html_to_markdown_lists_and_hr() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<ul><li>item 1</li><li>item 2</li></ul>");
    assert!(md.contains("- item 1"));
    assert!(md.contains("- item 2"));

    let md = html_to_markdown("<hr>");
    assert!(md.contains("---"));
}

#[test]
fn html_to_markdown_removes_scripts() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<p>text</p><script>bad</script>");
    assert!(!md.contains("bad"));
    assert!(md.contains("text"));
}

#[test]
fn html_to_markdown_nested_formatting() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<p><strong><em>bold italic</em></strong></p>");
    assert!(md.contains("***"));
    assert!(md.contains("bold italic"));
}

#[test]
fn html_to_markdown_code_and_blockquotes() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<p>use <code>fn()</code></p>");
    assert!(md.contains("`fn()`"));

    let md = html_to_markdown("<blockquote>cite</blockquote>");
    assert!(md.contains("> cite"));
}

#[test]
fn html_to_markdown_pre_tags() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<pre>code block</pre>");
    assert!(md.contains("```"));
    assert!(md.contains("code block"));
}

#[test]
fn html_to_markdown_anchor_without_href() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<a>no link</a>");
    assert!(md.contains("no link"));
    assert!(!md.contains("]("));
}

#[test]
fn html_to_markdown_empty_input() {
    use memvid_agent_core::extractor::html_to_markdown;
    assert_eq!(html_to_markdown(""), "");
}

#[test]
fn html_to_markdown_headings_h4_h5_h6() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<h4>h4</h4><h5>h5</h5><h6>h6</h6>");
    assert!(md.contains("#### h4"));
    assert!(md.contains("##### h5"));
    assert!(md.contains("###### h6"));
}

#[test]
fn html_to_markdown_image_without_src() {
    use memvid_agent_core::extractor::html_to_markdown;
    let md = html_to_markdown("<img alt=\"photo\">");
    assert!(!md.contains("!["));
}

// ---------------------------------------------------------------------------
// Metadata extraction
// ---------------------------------------------------------------------------

#[test]
fn extract_metadata_basic() {
    use memvid_agent_core::extractor::extract_metadata;
    let html = r#"<html lang="es"><head><title>Mi Pagina</title><meta name="description" content="Descripcion"></head></html>"#;
    let meta = extract_metadata(html);
    assert_eq!(meta.title, Some("Mi Pagina".to_string()));
    assert_eq!(meta.description, Some("Descripcion".to_string()));
    assert_eq!(meta.language, Some("es".to_string()));
}

#[test]
fn extract_metadata_missing_fields() {
    use memvid_agent_core::extractor::extract_metadata;
    let html = "<html><head><title>Title</title></head></html>";
    let meta = extract_metadata(html);
    assert_eq!(meta.title, Some("Title".into()));
    assert!(meta.description.is_none());
    assert!(meta.language.is_none());
}

#[test]
fn extract_metadata_og_fallback() {
    use memvid_agent_core::extractor::extract_metadata;
    let html = r#"<meta property="og:description" content="OG desc">"#;
    let meta = extract_metadata(html);
    assert_eq!(meta.description, Some("OG desc".into()));
}

#[test]
fn extract_metadata_language_with_region() {
    use memvid_agent_core::extractor::extract_metadata;
    let html = r#"<html lang="en-US"><head><title>T</title></head></html>"#;
    let meta = extract_metadata(html);
    assert_eq!(meta.language, Some("en".into()));
}

#[test]
fn extract_metadata_missing_title() {
    use memvid_agent_core::extractor::extract_metadata;
    let html = r#"<html><meta name="description" content="desc"></html>"#;
    let meta = extract_metadata(html);
    assert!(meta.title.is_none());
    assert_eq!(meta.description, Some("desc".into()));
}

// ---------------------------------------------------------------------------
// Text extraction via extract_text
// ---------------------------------------------------------------------------

#[test]
fn extract_text_delegates_correctly() {
    use memvid_agent_core::extractor::extract_text;
    assert_eq!(extract_text("<p>hello</p>", "text/html"), "hello");
    assert_eq!(extract_text("hello world", "text/plain"), "hello world");
    assert_eq!(
        extract_text("# Hello\nWorld", "text/markdown"),
        "# Hello\nWorld"
    );
    assert_eq!(extract_text("**bold**", "text/md"), "**bold**");
    assert_eq!(extract_text("plain", "application/octet-stream"), "plain");
    assert_eq!(extract_text("", "text/html"), "");
    assert_eq!(extract_text("", "text/plain"), "");
}

// ---------------------------------------------------------------------------
// File extraction
// ---------------------------------------------------------------------------

#[test]
fn extract_file_txt_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hello.txt");
    std::fs::write(&path, "Hello, world!").unwrap();

    use memvid_agent_core::extractor::extract_file;
    let result = extract_file(&path).unwrap();
    assert_eq!(result.content, "Hello, world!");
    assert_eq!(result.title, Some("hello".into()));
    assert_eq!(result.format, memvid_agent_core::types::Format::Text);
}

#[test]
fn extract_file_md_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readme.md");
    std::fs::write(&path, "# Title\n\nContent").unwrap();

    use memvid_agent_core::extractor::extract_file;
    let result = extract_file(&path).unwrap();
    assert_eq!(result.content, "# Title\n\nContent");
    assert_eq!(result.format, memvid_agent_core::types::Format::Markdown);
}

#[test]
fn extract_file_html_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.html");
    std::fs::write(&path, "<p>Hello</p>").unwrap();

    use memvid_agent_core::extractor::extract_file;
    let result = extract_file(&path).unwrap();
    assert_eq!(result.content, "<p>Hello</p>");
    assert_eq!(result.format, memvid_agent_core::types::Format::Html);
}

#[test]
fn extract_file_nonexistent_returns_error() {
    use memvid_agent_core::extractor::extract_file;
    assert!(extract_file(Path::new("/nonexistent/test.txt")).is_err());
    assert!(extract_file(Path::new("/nonexistent/test.pdf")).is_err());
    assert!(extract_file(Path::new("/nonexistent/test.epub")).is_err());
}

#[test]
fn extract_file_no_extension_defaults_to_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Makefile");
    std::fs::write(&path, "all:\n\techo hi").unwrap();
    use memvid_agent_core::extractor::extract_file;
    let result = extract_file(&path).unwrap();
    assert_eq!(result.format, memvid_agent_core::types::Format::Text);
}

#[test]
fn extract_file_empty_txt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.txt");
    std::fs::write(&path, "").unwrap();
    use memvid_agent_core::extractor::extract_file;
    let result = extract_file(&path).unwrap();
    assert_eq!(result.content, "");
}

// ---------------------------------------------------------------------------
// Utils: atomic_write, FileLock, SHA-256
// ---------------------------------------------------------------------------

#[test]
fn atomic_write_roundtrip() {
    use memvid_agent_core::utils::atomic_write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");

    atomic_write(&path, b"hello world").unwrap();
    assert!(path.exists());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");

    atomic_write(&path, b"updated").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "updated");
}

#[test]
fn atomic_write_empty_and_large() {
    use memvid_agent_core::utils::atomic_write;
    let dir = tempfile::tempdir().unwrap();

    let path = dir.path().join("empty.txt");
    atomic_write(&path, b"").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");

    let path = dir.path().join("large.bin");
    let large = vec![0xABu8; 100_000];
    atomic_write(&path, &large).unwrap();
    assert_eq!(std::fs::read(&path).unwrap().len(), 100_000);
}

#[test]
fn file_lock_acquire_release() {
    use memvid_agent_core::utils::FileLock;
    let dir = tempfile::tempdir().unwrap();
    {
        let _lock = FileLock::acquire(dir.path()).unwrap();
        assert!(dir.path().join(".lock").exists());
    }
    assert!(!dir.path().join(".lock").exists());
}

#[test]
fn file_lock_prevents_second_instance() {
    use memvid_agent_core::utils::FileLock;
    let dir = tempfile::tempdir().unwrap();
    let _lock = FileLock::acquire(dir.path()).unwrap();
    assert!(FileLock::acquire(dir.path()).is_err());
}

#[test]
fn file_lock_contains_pid() {
    use memvid_agent_core::utils::FileLock;
    let dir = tempfile::tempdir().unwrap();
    {
        let _lock = FileLock::acquire(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join(".lock")).unwrap();
        let pid: u32 = content.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());
    }
}

#[test]
fn sha256_digest_known_values() {
    use memvid_agent_core::utils::sha256_digest;
    assert_eq!(
        sha256_digest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_digest(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_digest_binary_data() {
    use memvid_agent_core::utils::sha256_digest;
    let result = sha256_digest(&[0x00, 0xFF, 0xAB, 0xCD]);
    assert_eq!(result.len(), 64);
    assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn file_checksum_matches_direct_digest() {
    use memvid_agent_core::utils::{compute_file_checksum, sha256_digest};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.bin");
    std::fs::write(&path, b"checksum me").unwrap();
    assert_eq!(
        compute_file_checksum(&path).unwrap(),
        sha256_digest(b"checksum me")
    );
}

#[test]
fn file_checksum_nonexistent_returns_error() {
    use memvid_agent_core::utils::compute_file_checksum;
    assert!(compute_file_checksum("/nonexistent/file.bin").is_err());
}

// ---------------------------------------------------------------------------
// WebFetcher configuration
// ---------------------------------------------------------------------------

#[test]
fn web_fetcher_config_defaults() {
    use memvid_agent_core::web_fetcher::WebFetcher;
    let config = memvid_agent_core::types::IngestionConfig::default();
    let fetcher = WebFetcher::new(&config);
    assert_eq!(fetcher.config.timeout_seconds, 30);
    assert_eq!(fetcher.config.max_size_bytes, 5 * 1024 * 1024);
    assert_eq!(fetcher.config.rate_limit_per_second, 2);
    assert_eq!(fetcher.config.max_retries, 3);
}

#[test]
fn web_fetcher_zero_rate_limit() {
    use memvid_agent_core::web_fetcher::WebFetcher;
    let mut config = memvid_agent_core::types::IngestionConfig::default();
    config.rate_limit_per_second = 0;
    let fetcher = WebFetcher::new(&config);
    assert_eq!(fetcher.min_interval.as_millis(), 0);
}

#[test]
fn web_fetcher_high_rate_limit() {
    use memvid_agent_core::web_fetcher::WebFetcher;
    let mut config = memvid_agent_core::types::IngestionConfig::default();
    config.rate_limit_per_second = 10000;
    let fetcher = WebFetcher::new(&config);
    assert_eq!(fetcher.min_interval.as_millis(), 0);
}

#[test]
fn global_throttle_does_not_panic() {
    memvid_agent_core::web_fetcher::global_throttle(1000);
    memvid_agent_core::web_fetcher::global_throttle(0);
}

// ---------------------------------------------------------------------------
// Generation pipeline
// ---------------------------------------------------------------------------

#[test]
fn generation_result_construction() {
    use memvid_agent_core::generation::GenerationResult;
    let result = GenerationResult {
        response: "hello".to_string(),
        tokens_estimated: 5,
    };
    assert_eq!(result.response, "hello");
    assert_eq!(result.tokens_estimated, 5);
}

#[test]
fn generation_prompt_assembles_correct_template() {
    use memvid_agent_core::context_policy::ContextPolicy;
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};
    use memvid_agent_core::retrieval::KnowledgeIndex;

    let dir = tempfile::tempdir().unwrap();
    let index = KnowledgeIndex::load(dir.path()).unwrap();
    let builder = PromptBuilder::new(ChatTemplate::ChatML);
    let policy = ContextPolicy::new(4096, 2048);
    let batch = vec![msg("user", "hi")];

    let trimmed =
        policy.trim_messages(builder.developer_prompt(), &batch, "hello", |t| t.len() / 4);
    let prompt = builder.build(&trimmed, "hello", &[]);
    assert!(prompt.contains("<|im_start|>system"));
    assert!(prompt.contains("expert software engineer"));
    assert!(prompt.contains("<|im_start|>user\nhello"));
}

// ---------------------------------------------------------------------------
// Books catalog (public API only)
// ---------------------------------------------------------------------------

#[test]
fn books_catalog_prepare_knowledge() {
    use memvid_agent_core::books_catalog::{
        BookResource, LanguageBooks, prepare_knowledge_from_books,
    };

    let books = LanguageBooks {
        language: "Rust".to_string(),
        resources: vec![
            BookResource {
                title: "The Book".into(),
                url: "https://doc.rust-lang.org/book".into(),
                format: "HTML".into(),
            },
            BookResource {
                title: "Rust by Example".into(),
                url: "https://doc.rust-lang.org/stable/rust-by-example".into(),
                format: "HTML".into(),
            },
        ],
    };
    let knowledge = prepare_knowledge_from_books(&books, 5);
    assert!(knowledge.contains("Rust"));
    assert!(knowledge.contains("The Book"));
    assert!(knowledge.contains("Rust by Example"));
    assert!(knowledge.contains("https://doc.rust-lang.org/book"));
}

// ---------------------------------------------------------------------------
// Types: serialization roundtrips
// ---------------------------------------------------------------------------

#[test]
fn types_conversation_batch_roundtrip() {
    use chrono::Utc;
    use memvid_agent_core::types::{ConversationBatch, Message, MessageRole};

    let batch = ConversationBatch {
        id: "test-id".into(),
        timestamp: Utc::now(),
        messages: vec![
            Message {
                role: MessageRole::User,
                content: "hello".into(),
                timestamp: Utc::now(),
                tokens: None,
            },
            Message {
                role: MessageRole::Assistant,
                content: "world".into(),
                timestamp: Utc::now(),
                tokens: Some(5),
            },
        ],
        model_used: "test-model".into(),
        tokens_used: 42,
    };
    let json = serde_json::to_string(&batch).unwrap();
    let deserialized: ConversationBatch = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, batch.id);
    assert_eq!(deserialized.messages.len(), 2);
    assert_eq!(deserialized.model_used, "test-model");
    assert_eq!(deserialized.tokens_used, 42);
}

#[test]
fn types_manifest_roundtrip() {
    use chrono::Utc;
    use memvid_agent_core::types::Manifest;

    let manifest = Manifest {
        version: "1.0.0".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        core_segment: "core.mv2".into(),
        conversation_segments: vec![],
        knowledge_segments: vec![],
        archived_segments: vec![],
    };
    let json = serde_json::to_string(&manifest).unwrap();
    let deserialized: Manifest = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.version, "1.0.0");
    assert_eq!(deserialized.core_segment, "core.mv2");
}

#[test]
fn types_knowledge_entry_roundtrip() {
    use chrono::Utc;
    use memvid_agent_core::types::KnowledgeEntry;

    let entry = KnowledgeEntry {
        id: "know-1".into(),
        source: "test-source".into(),
        content: "test content".into(),
        timestamp: Utc::now(),
        checksum: "def456".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let deserialized: KnowledgeEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "know-1");
    assert_eq!(deserialized.source, "test-source");
    assert_eq!(deserialized.content, "test content");
}

#[test]
fn types_fetched_content_optional_fields() {
    use memvid_agent_core::types::FetchedContent;

    let with_all = FetchedContent {
        url: "https://example.com".into(),
        title: Some("Example".into()),
        description: Some("Desc".into()),
        content: "<p>hello</p>".into(),
        content_type: "text/html".into(),
        size_bytes: 14,
    };
    assert_eq!(with_all.title, Some("Example".into()));
    assert_eq!(with_all.description, Some("Desc".into()));

    let without = FetchedContent {
        url: "https://example.com".into(),
        title: None,
        description: None,
        content: "body".into(),
        content_type: "text/plain".into(),
        size_bytes: 4,
    };
    assert!(without.title.is_none());
    assert!(without.description.is_none());
}

#[test]
fn types_segment_entry_roundtrip() {
    use chrono::Utc;
    use memvid_agent_core::types::SegmentEntry;

    let entry = SegmentEntry {
        id: "seg-1".into(),
        filename: "conv_20250101_001.mv2".into(),
        created_at: Utc::now(),
        size_bytes: 1024,
        message_count: 5,
        model_used: "test-model".into(),
        tokens_used: 100,
        checksum: "abc123".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let deserialized: SegmentEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, entry.id);
    assert_eq!(deserialized.filename, entry.filename);
    assert_eq!(deserialized.size_bytes, 1024);
}

// ---------------------------------------------------------------------------
// ChunkOptions and IngestionConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn chunk_options_defaults_correct() {
    use memvid_agent_core::types::{ChunkOptions, ChunkStrategy};
    let opts = ChunkOptions::default();
    assert_eq!(opts.max_size, 1024);
    assert_eq!(opts.overlap, 200);
    assert_eq!(opts.strategy, ChunkStrategy::Paragraph);
}

#[test]
fn ingestion_config_defaults_correct() {
    use memvid_agent_core::types::IngestionConfig;
    let cfg = IngestionConfig::default();
    assert_eq!(cfg.user_agent, "aten-ia/0.1.0");
    assert_eq!(cfg.timeout_seconds, 30);
    assert_eq!(cfg.max_size_bytes, 5 * 1024 * 1024);
    assert_eq!(cfg.rate_limit_per_second, 2);
    assert_eq!(cfg.chunk_max_size, 1024);
    assert_eq!(cfg.chunk_overlap, 200);
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.retry_backoff_seconds, 5);
}

// ---------------------------------------------------------------------------
// WriterConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn writer_config_defaults_correct() {
    use memvid_agent_core::types::WriterConfig;
    let cfg = WriterConfig::default();
    assert_eq!(cfg.batch_size, 10);
    assert_eq!(cfg.segment_max_bytes, 50 * 1024 * 1024);
    assert_eq!(cfg.data_dir, std::path::PathBuf::from("memvid_data"));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config() -> memvid_agent_core::config::Config {
    memvid_agent_core::config::Config::default()
}

fn test_agent(data_dir: &Path) -> memvid_agent_core::agent::Agent {
    use memvid_agent_core::agent::Agent;
    use memvid_agent_core::context_policy::ContextPolicy;
    use memvid_agent_core::llama::context::LlamaContext;
    use memvid_agent_core::memvid::writer::MemvidWriter;
    use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};
    use memvid_agent_core::retrieval::KnowledgeIndex;
    use memvid_agent_core::session::Session;
    use memvid_agent_core::types::WriterConfig;

    let writer_config = WriterConfig {
        data_dir: data_dir.to_path_buf(),
        ..Default::default()
    };
    let memory = MemvidWriter::init(writer_config).unwrap();
    let knowledge_index = KnowledgeIndex::load(data_dir).unwrap();

    Agent::from_components(
        LlamaContext::null(),
        memory,
        knowledge_index,
        "test-model".to_string(),
        Session::new(),
        PromptBuilder::new(ChatTemplate::ChatML),
        ContextPolicy::new(4096, 2048),
    )
}

fn knowledge_entry(source: &str, content: &str) -> memvid_agent_core::types::KnowledgeEntry {
    use chrono::Utc;
    use uuid::Uuid;
    let checksum = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    };
    memvid_agent_core::types::KnowledgeEntry {
        id: Uuid::new_v4().to_string(),
        source: source.to_string(),
        content: content.to_string(),
        timestamp: Utc::now(),
        checksum,
    }
}

fn msg(role: &str, content: &str) -> memvid_agent_core::types::Message {
    use chrono::Utc;
    use memvid_agent_core::types::{Message, MessageRole};
    let role = match role {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "tool" => MessageRole::Tool,
        _ => panic!("unknown role: {}", role),
    };
    Message {
        role,
        content: content.to_string(),
        timestamp: Utc::now(),
        tokens: None,
    }
}
