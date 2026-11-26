//! Runtime self-test used by `--self-test` / `--verify` and CI smoke tests.
//!
//! It exercises every subsystem that does not require a GGUF model or network
//! access: config, persistence, RAG, chunking, extraction, feeds, queue,
//! prompts, model catalog, fine-tune estimation and utilities.
//!
//! This is intentionally dependency-free (no `tempfile`): it uses a unique
//! directory under the system temp dir and removes it at the end.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::path::Path;
use uuid::Uuid;

use crate::books_catalog::{BookResource, LanguageBooks, prepare_knowledge_from_books};
use crate::chunker;
use crate::config::Config;
use crate::extractor;
use crate::feeds;
use crate::finetune;
use crate::memvid::reader::Reader;
use crate::memvid::writer::MemvidWriter;
use crate::models_catalog::ModelsCatalog;
use crate::prompt::{ChatTemplate, PromptBuilder};
use crate::queue::FeedQueue;
use crate::retrieval::KnowledgeIndex;
use crate::types::{
    ChunkOptions, ChunkStrategy, ConversationBatch, Format, KnowledgeEntry, Message, MessageRole,
    WriterConfig,
};
use crate::utils;

macro_rules! run {
    ($failed:ident, $name:expr, $f:expr) => {{
        if !check($name, $f) {
            $failed += 1;
        }
    }};
}

fn check<F>(name: &str, f: F) -> bool
where
    F: FnOnce() -> Result<()>,
{
    match f() {
        Ok(()) => {
            println!("  ok    {name}");
            true
        }
        Err(e) => {
            println!("  FAIL  {name}: {e:#}");
            false
        }
    }
}

/// Run all self-checks. Returns an error if any check fails.
pub fn run() -> Result<()> {
    println!("aten-ia · autodiagnóstico {}", env!("CARGO_PKG_VERSION"));

    let root = std::env::temp_dir().join(format!("aten-ia-selfcheck-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root)
        .with_context(|| format!("no se pudo crear {}", root.display()))?;

    let outcome = run_checks(&root);
    let _ = std::fs::remove_dir_all(&root);
    outcome
}

fn run_checks(root: &Path) -> Result<()> {
    let mut failed = 0usize;

    run!(
        failed,
        "configuración: valores por defecto, validación y guardado",
        || {
            let mut cfg = Config::default();
            cfg.data_dir = root.join("data");
            cfg.validate()?;
            cfg.save_to_path(root.join("config.json"))?;
            let loaded = Config::load_or_create_with_path(root.join("config.json"))?;
            anyhow::ensure!(loaded.data_dir == cfg.data_dir, "data_dir no coincide");
            anyhow::ensure!(loaded.model.n_ctx > 0, "n_ctx no cargado");
            Ok(())
        }
    );

    run!(failed, "utilidades: escritura atómica y SHA-256", || {
        let path = root.join("atomic.txt");
        utils::atomic_write(&path, b"selfcheck")?;
        let file_sha = utils::compute_file_checksum(&path)?;
        let direct_sha = utils::sha256_digest(b"selfcheck");
        anyhow::ensure!(file_sha == direct_sha, "checksum no coincide");
        Ok(())
    });

    run!(
        failed,
        "fragmentación: fija, por párrafos y por encabezados",
        || {
            let text = "# Title\n\nword word word word word word\n\nsecond paragraph\n";
            for strategy in [
                ChunkStrategy::Fixed,
                ChunkStrategy::Paragraph,
                ChunkStrategy::Heading,
            ] {
                let opts = ChunkOptions {
                    max_size: 24,
                    overlap: 4,
                    strategy,
                };
                let chunks = chunker::chunk_text(text, &opts, "selfcheck");
                anyhow::ensure!(!chunks.is_empty(), "fragmentos vacíos para {strategy:?}");
            }
            Ok(())
        }
    );

    run!(failed, "extracción: detección de formato y HTML", || {
        anyhow::ensure!(
            Format::from_extension(Path::new("doc.pdf")) == Format::Pdf,
            "detección de PDF fallida"
        );
        let text = extractor::extract_text("<html><body><h1>Hola</h1></body></html>", "text/html");
        anyhow::ensure!(text.contains("Hola"), "falta el texto HTML");
        let md = extractor::html_to_markdown("<h1>Hola</h1><p>mundo</p>");
        anyhow::ensure!(
            md.contains("# Hola") && md.contains("mundo"),
            "fallo de Markdown"
        );
        Ok(())
    });

    run!(failed, "RAG: alta, búsqueda y recarga", || {
        let dir = root.join("index");
        let mut index = KnowledgeIndex::load(&dir)?;
        let content = "selfcheck knowledge entry";
        let entry = KnowledgeEntry {
            id: Uuid::new_v4().to_string(),
            source: "selfcheck://doc".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            checksum: utils::sha256_digest(content.as_bytes()),
        };
        index.add_entry(entry)?;
        anyhow::ensure!(
            !index.search("selfcheck", 5).is_empty(),
            "la búsqueda no devolvió nada"
        );
        let reloaded = KnowledgeIndex::load(&dir)?;
        anyhow::ensure!(reloaded.len() == 1, "la recarga perdió entradas");
        Ok(())
    });

    run!(failed, "memoria .mv2: conversación y conocimiento", || {
        let dir = root.join("mv2");
        let mut writer = MemvidWriter::init(WriterConfig {
            data_dir: dir.clone(),
            ..Default::default()
        })?;

        writer.append_conversation(ConversationBatch {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            messages: vec![Message {
                role: MessageRole::User,
                content: "selfcheck conversation".to_string(),
                timestamp: Utc::now(),
                tokens: Some(3),
            }],
            model_used: "selfcheck".to_string(),
            tokens_used: 3,
        })?;
        writer.flush()?;

        let conv_name = writer
            .playlist
            .manifest
            .conversation_segments
            .last()
            .map(|s| s.filename.clone())
            .ok_or_else(|| anyhow::anyhow!("falta el segmento de conversación"))?;
        let conv_path = dir.join("conversations").join(conv_name);
        let mut reader = Reader::open(&conv_path)?;
        let frames = reader.enumerate()?;
        anyhow::ensure!(!frames.is_empty(), "la conversación no tiene frames");
        let text = reader.read_text(frames[0].id)?;
        anyhow::ensure!(
            text.contains("selfcheck conversation"),
            "falta el contenido de la conversación"
        );

        let content = "selfcheck knowledge";
        writer.append_knowledge(KnowledgeEntry {
            id: Uuid::new_v4().to_string(),
            source: "selfcheck://mv2".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            checksum: utils::sha256_digest(content.as_bytes()),
        })?;
        writer.flush_knowledge()?;

        let know_name = writer
            .playlist
            .manifest
            .knowledge_segments
            .last()
            .map(|s| s.filename.clone())
            .ok_or_else(|| anyhow::anyhow!("falta el segmento de conocimiento"))?;
        let know_path = dir.join("knowledge").join(know_name);
        let mut reader = Reader::open(&know_path)?;
        let frames = reader.enumerate()?;
        anyhow::ensure!(!frames.is_empty(), "el conocimiento no tiene frames");
        Ok(())
    });

    run!(failed, "cola: alta, pendiente y completado", || {
        let mut queue = FeedQueue::new(root);
        queue.add("https://example.com/selfcheck")?;
        anyhow::ensure!(
            queue.pending().len() == 1,
            "recuento de pendientes incorrecto"
        );
        let id = queue.pending()[0].id.clone();
        queue.mark_done(&id)?;
        anyhow::ensure!(
            queue.stats().done == 1,
            "recuento de completados incorrecto"
        );
        let reloaded = FeedQueue::new(root);
        anyhow::ensure!(reloaded.stats().done == 1, "la cola no se persistió");
        Ok(())
    });

    run!(failed, "feeds: lectura de RSS", || {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
<title>SelfCheck</title><link>https://example.com</link><description>d</description>
<item><title>Entry</title><link>https://example.com/entry</link>
<description>body</description></item>
</channel></rss>"#;
        let entries = feeds::parse_feed_xml(xml, "https://example.com/feed")?;
        anyhow::ensure!(entries.len() == 1, "entradas de feed incorrectas");
        anyhow::ensure!(entries[0].url.contains("entry"), "URL de feed incorrecta");
        Ok(())
    });

    run!(failed, "libros: preparación del catálogo", || {
        let books = LanguageBooks {
            language: "SelfCheck".to_string(),
            resources: vec![BookResource {
                title: "Book".to_string(),
                url: "https://example.com/book".to_string(),
                format: "HTML".to_string(),
            }],
        };
        let text = prepare_knowledge_from_books(&books, 10);
        anyhow::ensure!(
            text.contains("Book") && text.contains("example.com"),
            "fallo en libros"
        );
        Ok(())
    });

    run!(failed, "prompts: ChatML y contexto RAG", || {
        let prompt = PromptBuilder::new(ChatTemplate::ChatML).build(
            &[Message {
                role: MessageRole::User,
                content: "previous".to_string(),
                timestamp: Utc::now(),
                tokens: None,
            }],
            "hola",
            &["[doc] contexto".to_string()],
        );
        anyhow::ensure!(prompt.contains("hola"), "falta la entrada del usuario");
        anyhow::ensure!(prompt.contains("contexto"), "falta el contexto RAG");
        Ok(())
    });

    run!(failed, "modelos: carga del catálogo", || {
        let catalog = ModelsCatalog::load();
        anyhow::ensure!(catalog.list().len() >= 20, "catálogo demasiado pequeño");
        anyhow::ensure!(
            catalog.find("qwen2.5-0.5b").is_some(),
            "falta el modelo por defecto"
        );
        Ok(())
    });

    run!(failed, "ajuste fino: estimación y comando", || {
        let est = finetune::estimate(1_000, 500_000_000, 3, 4);
        anyhow::ensure!(est.ram_gb() > 0.0, "estimación de RAM inválida");
        let cmd = finetune::render_command(
            "llama-finetune",
            &["-m".to_string(), "model path".to_string()],
        );
        anyhow::ensure!(cmd.contains("\"model path\""), "comando sin comillas");
        Ok(())
    });

    if failed > 0 {
        bail!("{failed} self-check(s) failed");
    }
    println!("autodiagnóstico: todo correcto");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn selfcheck_passes() {
        super::run().expect("self-test must pass");
    }
}
