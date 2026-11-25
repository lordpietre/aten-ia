//! End-to-end ingestion tests against a local HTTP server.
//!
//! No external network or GGUF model is required: the server runs on
//! 127.0.0.1 and the agent uses `LlamaContext::null()`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use memvid_agent_core::agent::Agent;
use memvid_agent_core::context_policy::ContextPolicy;
use memvid_agent_core::llama::context::LlamaContext;
use memvid_agent_core::memvid::writer::MemvidWriter;
use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};
use memvid_agent_core::retrieval::KnowledgeIndex;
use memvid_agent_core::session::Session;
use memvid_agent_core::types::{IngestionConfig, WriterConfig};

const HTML: &str = "<html><head><title>Test Page</title></head><body><h1>Hello</h1><p>selfcheck content about rust ownership</p></body></html>";
fn response_for(path: &str, port: u16) -> (&'static str, String) {
    if path.starts_with("/feed") {
        let rss = format!(
            r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>Feed</title><link>http://127.0.0.1:{port}</link>
<description>d</description>
<item><title>Entry</title><link>http://127.0.0.1:{port}/entry</link>
<description>feed entry body about borrowing</description></item>
</channel></rss>"#
        );
        ("application/rss+xml", rss)
    } else if path.starts_with("/entry") {
        ("text/html", HTML.replace("ownership", "borrowing"))
    } else {
        ("text/html", HTML.to_string())
    }
}

fn spawn_server() -> (u16, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    let handle = thread::spawn(move || {
        while !stop_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (content_type, body) = response_for(path, port);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    (port, stop, handle)
}

fn test_agent(data_dir: &Path) -> Agent {
    let memory = MemvidWriter::init(WriterConfig {
        data_dir: data_dir.to_path_buf(),
        ..Default::default()
    })
    .unwrap();
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

fn ingestion() -> IngestionConfig {
    IngestionConfig {
        timeout_seconds: 5,
        max_size_bytes: 1024 * 1024,
        rate_limit_per_second: 1000,
        chunk_max_size: 256,
        chunk_overlap: 32,
        max_retries: 0,
        retry_backoff_seconds: 0,
        ..Default::default()
    }
}

#[test]
fn fetch_ingests_html_from_local_server() {
    let (port, stop, handle) = spawn_server();
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    let url = format!("http://127.0.0.1:{port}/page");
    let (fetched, indexed) = agent.fetch_and_ingest(&url, &ingestion()).unwrap();
    assert!(
        fetched.content.contains("ownership"),
        "content: {}",
        fetched.content
    );
    assert!(indexed > 0, "expected indexed chunks");
    assert!(agent.knowledge_count() > 0);

    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
}

#[test]
fn feed_ingests_entries_from_local_server() {
    let (port, stop, handle) = spawn_server();
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    let url = format!("http://127.0.0.1:{port}/feed");
    let result = agent.fetch_and_ingest_feed(&url, &ingestion(), 5).unwrap();
    assert_eq!(result.entries_found, 1);
    assert!(result.entries_indexed > 0);
    assert!(agent.knowledge_count() > 0);

    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
}

#[test]
fn batch_processes_multiple_urls() {
    let (port, stop, handle) = spawn_server();
    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());

    let urls = vec![
        format!("http://127.0.0.1:{port}/page1"),
        format!("http://127.0.0.1:{port}/page2"),
    ];
    let result = agent.process_url_batch(&urls, &ingestion()).unwrap();
    assert_eq!(result.success.len(), 2);
    assert!(result.failures.is_empty());
    assert!(result.total_chunks > 0);

    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
}

#[test]
fn fetch_rejects_oversized_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let body = "x".repeat(4096);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let mut agent = test_agent(dir.path());
    let mut small = ingestion();
    small.max_size_bytes = 128;
    let url = format!("http://127.0.0.1:{port}/large");
    let err = agent
        .fetch_and_ingest(&url, &small)
        .unwrap_err()
        .to_string();
    assert!(err.to_lowercase().contains("too large"), "{err}");
    handle.join().unwrap();
}
