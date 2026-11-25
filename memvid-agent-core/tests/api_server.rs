//! End-to-end tests for the HTTP API over a real TCP socket.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use memvid_agent_core::agent::Agent;
use memvid_agent_core::api::ApiServer;
use memvid_agent_core::context_policy::ContextPolicy;
use memvid_agent_core::llama::context::LlamaContext;
use memvid_agent_core::memvid::writer::MemvidWriter;
use memvid_agent_core::prompt::{ChatTemplate, PromptBuilder};
use memvid_agent_core::retrieval::KnowledgeIndex;
use memvid_agent_core::session::Session;
use memvid_agent_core::shutdown;
use memvid_agent_core::types::WriterConfig;

const TOKEN: &str = "selfcheck-token";

struct ShutdownGuard;

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        shutdown::shutdown_flag().store(false, std::sync::atomic::Ordering::SeqCst);
    }
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
        "selfcheck-model".to_string(),
        Session::new(),
        PromptBuilder::new(ChatTemplate::ChatML),
        ContextPolicy::new(4096, 2048),
    )
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn start_server(port: u16) -> thread::JoinHandle<()> {
    let dir = tempfile::tempdir().unwrap();
    // Leak the temp dir for the server lifetime: the thread owns the agent.
    let path = dir.keep();
    let agent = Arc::new(Mutex::new(test_agent(&path)));
    let server = ApiServer::new(
        agent,
        "selfcheck-model".to_string(),
        "127.0.0.1".to_string(),
        port,
        Some(TOKEN.to_string()),
    );

    thread::spawn(move || {
        let _ = server.run();
        drop(server);
        let _ = std::fs::remove_dir_all(&path);
    })
}

fn wait_until_ready(port: u16) {
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("API server did not start on port {port}");
}

fn request(port: u16, raw: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn get(path: &str, auth: bool) -> String {
    let auth_header = if auth {
        format!("Authorization: Bearer {TOKEN}\r\n")
    } else {
        String::new()
    };
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth_header}\r\n")
}

#[test]
fn api_covers_health_models_auth_and_404() {
    let _guard = ShutdownGuard;
    shutdown::shutdown_flag().store(false, std::sync::atomic::Ordering::SeqCst);

    let port = free_port();
    let handle = start_server(port);
    wait_until_ready(port);

    let health = request(port, &get("/health", false));
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.contains("\"status\":\"ok\""), "{health}");
    assert!(health.contains("selfcheck-model"), "{health}");

    let unauthorized = request(port, &get("/v1/models", false));
    assert!(unauthorized.starts_with("HTTP/1.1 401"), "{unauthorized}");

    let models = request(port, &get("/v1/models", true));
    assert!(models.starts_with("HTTP/1.1 200"), "{models}");
    assert!(models.contains("selfcheck-model"), "{models}");

    let token = request(port, &get("/token", true));
    assert!(token.contains(TOKEN), "{token}");

    let missing = request(port, &get("/does-not-exist", true));
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");

    shutdown::request_shutdown();
    handle.join().unwrap();
}

#[test]
fn api_returns_400_on_invalid_json() {
    let _guard = ShutdownGuard;
    shutdown::shutdown_flag().store(false, std::sync::atomic::Ordering::SeqCst);

    let port = free_port();
    let handle = start_server(port);
    wait_until_ready(port);

    let body = "{not-json";
    let raw = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let response = request(port, &raw);
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(response.contains("Invalid JSON"), "{response}");

    shutdown::request_shutdown();
    handle.join().unwrap();
}
