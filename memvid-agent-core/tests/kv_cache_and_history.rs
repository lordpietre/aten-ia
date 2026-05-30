// ---------------------------------------------------------------------------
// Integration tests for:
//   1. The configurable KV-cache type feature (TurboQuant wiring, Option 4).
//   2. The UTF-8 conversation-history truncation regression.
//
// All tests are isolated via tempfile::tempdir and do NOT require a GGUF model.
// They use a null LlamaContext for paths that never touch inference.
// ---------------------------------------------------------------------------

use chrono::Utc;
use memvid_agent_core::agent::Agent;
use memvid_agent_core::config::Config;
use memvid_agent_core::context_policy::ContextPolicy;
use memvid_agent_core::llama::context::LlamaContext;
use memvid_agent_core::memvid::writer::MemvidWriter;
use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};
use memvid_agent_core::retrieval::KnowledgeIndex;
use memvid_agent_core::session::Session;
use memvid_agent_core::types::{ConversationBatch, Message, MessageRole, WriterConfig};

// ---------------------------------------------------------------------------
// KV-cache config contract
// ---------------------------------------------------------------------------

/// A fresh config defaults both KV caches to f16 — i.e. enabling the feature
/// changes nothing until the user opts in. This is the safety contract.
#[test]
fn kv_cache_defaults_are_f16() {
    let cfg = Config::default();
    assert_eq!(cfg.model.kv_type_k, "f16");
    assert_eq!(cfg.model.kv_type_v, "f16");
}

/// A config.json written before this feature existed (no kv_type_* keys) must
/// still load, with both caches defaulting to f16. Backward compatibility.
#[test]
fn legacy_config_without_kv_fields_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let legacy = r#"{
        "version": 1,
        "data_dir": "memvid_data",
        "developer_mode": true,
        "developer_prompt": null,
        "model": {
            "path": "m.gguf", "name": "m", "n_ctx": 4096, "n_gpu_layers": 0,
            "chat_template": "chatml", "download_url": null, "sha256": null
        },
        "generation": { "top_k": 40, "top_p": 0.95, "temp": 0.8, "max_tokens": 2048 },
        "api": { "enabled": false, "host": "127.0.0.1", "port": 8787, "token": null },
        "languages": { "installed": [] }
    }"#;
    std::fs::write(&path, legacy).unwrap();

    let cfg = Config::load_or_create_with_path(&path).unwrap();
    assert_eq!(cfg.model.kv_type_k, "f16");
    assert_eq!(cfg.model.kv_type_v, "f16");
}

/// The recommended asymmetric config (K safe, V compressed) survives a
/// save→load round-trip on disk.
#[test]
fn asymmetric_kv_config_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");

    let mut cfg = Config::default();
    cfg.model.kv_type_k = "f16".to_string(); // K is everything → keep precise
    cfg.model.kv_type_v = "turbo3".to_string(); // V is free → compress hard
    cfg.save_to_path(&path).unwrap();

    let loaded = Config::load_or_create_with_path(&path).unwrap();
    assert_eq!(loaded.model.kv_type_k, "f16");
    assert_eq!(loaded.model.kv_type_v, "turbo3");
}

/// The resolver maps every documented name to a distinct ggml_type, and
/// unknown names fall back to f16 (never panic, never reject the config).
#[test]
fn kv_cache_type_resolver_covers_documented_names() {
    use memvid_agent_core::llama::context::kv_cache_ggml_type;

    let f16 = kv_cache_ggml_type("f16");
    // Plain types resolve to themselves and are stable.
    assert_eq!(kv_cache_ggml_type("F16"), f16);
    assert_eq!(kv_cache_ggml_type("nonsense"), f16); // unknown → f16
    assert_eq!(kv_cache_ggml_type(""), f16);

    // The three turbo codecs are all distinct from each other and from f16.
    let t2 = kv_cache_ggml_type("turbo2");
    let t3 = kv_cache_ggml_type("turbo3");
    let t4 = kv_cache_ggml_type("turbo4");
    assert_ne!(t2, f16);
    assert_ne!(t3, f16);
    assert_ne!(t4, f16);
    assert_ne!(t2, t3);
    assert_ne!(t3, t4);
}

// ---------------------------------------------------------------------------
// UTF-8 conversation-history truncation (regression)
// ---------------------------------------------------------------------------

fn null_agent(memory: MemvidWriter, data_dir: &std::path::Path) -> Agent {
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

/// Regression: a stored message longer than 500 bytes whose 500th byte lands
/// inside a multi-byte UTF-8 character must NOT panic when displayed in the
/// history. Before the fix, `&content[..500]` panicked here.
#[test]
fn read_conversation_history_does_not_panic_on_multibyte() {
    let dir = tempfile::tempdir().unwrap();
    let writer_config = WriterConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    };
    let mut memory = MemvidWriter::init(writer_config).unwrap();

    // 600 × 'é' (2 bytes each) → byte index 500 is mid-character.
    let nasty = "é".repeat(600);
    let batch = ConversationBatch {
        id: "conv-1".to_string(),
        timestamp: Utc::now(),
        messages: vec![Message {
            role: MessageRole::User,
            content: nasty,
            timestamp: Utc::now(),
            tokens: None,
        }],
        model_used: "test-model".to_string(),
        tokens_used: 0,
    };
    memory.append_conversation(batch).unwrap();
    memory.flush().unwrap(); // persist segment + manifest (no llm needed)

    let agent = null_agent(memory, dir.path());

    // The call itself is the assertion: it must return Ok and not panic.
    let lines = agent.read_conversation_history().unwrap();
    let joined = lines.join("\n");

    // The long message was truncated with the "..." marker.
    assert!(
        joined.contains("..."),
        "expected truncated history, got: {joined}"
    );
    // And it still contains real é content (truncation cut on a char boundary).
    assert!(joined.contains('é'));
}

/// A short multibyte message is shown in full (no truncation marker added).
#[test]
fn read_conversation_history_keeps_short_messages_whole() {
    let dir = tempfile::tempdir().unwrap();
    let writer_config = WriterConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    };
    let mut memory = MemvidWriter::init(writer_config).unwrap();

    let batch = ConversationBatch {
        id: "conv-1".to_string(),
        timestamp: Utc::now(),
        messages: vec![Message {
            role: MessageRole::Assistant,
            content: "café ñoño 你好 🌍".to_string(),
            timestamp: Utc::now(),
            tokens: None,
        }],
        model_used: "test-model".to_string(),
        tokens_used: 0,
    };
    memory.append_conversation(batch).unwrap();
    memory.flush().unwrap();

    let agent = null_agent(memory, dir.path());
    let joined = agent.read_conversation_history().unwrap().join("\n");
    assert!(joined.contains("café ñoño 你好 🌍"));
}
