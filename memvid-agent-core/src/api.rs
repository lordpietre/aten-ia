use crate::agent::Agent;
use crate::types::{Message, MessageRole};
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct ApiServer {
    agent: Arc<Mutex<Agent>>,
    model_name: String,
    host: String,
    port: u16,
    token: Option<String>,
    start_time: Instant,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

impl ApiServer {
    pub fn new(
        agent: Arc<Mutex<Agent>>,
        model_name: String,
        host: String,
        port: u16,
        token: Option<String>,
    ) -> Self {
        Self {
            agent,
            model_name,
            host,
            port,
            token,
            start_time: Instant::now(),
        }
    }

    pub fn run(&self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        let listener = TcpListener::bind(&addr)?;
        eprintln!("[api] Listening on http://{}", addr);
        listener.set_nonblocking(false)?;

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(e) = self.handle_connection(&mut stream) {
                        eprintln!("[api] Error: {}", e);
                    }
                }
                Err(e) => eprintln!("[api] Connection error: {}", e),
            }
        }
        Ok(())
    }

    fn handle_connection(&self, stream: &mut TcpStream) -> Result<()> {
        let req = read_http_request(stream)?;
        let response = self.route(&req);
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        Ok(())
    }

    fn route(&self, req: &HttpRequest) -> String {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/health") => self.handle_health(),
            ("GET", "/v1/models") => {
                if !self.check_auth(req) {
                    return unauthorized();
                }
                self.handle_models()
            }
            ("POST", "/v1/chat/completions") => {
                if !self.check_auth(req) {
                    return unauthorized();
                }
                self.handle_chat(req)
            }
            ("POST", "/token") | ("GET", "/token") => {
                if !self.check_auth(req) {
                    return unauthorized();
                }
                self.handle_token(req)
            }
            _ => json_response(
                404,
                &json!({"error": {"message": "Not found", "type": "not_found"}}),
            ),
        }
    }

    fn check_auth(&self, req: &HttpRequest) -> bool {
        match &self.token {
            None => true,
            Some(token) => req
                .headers
                .get("authorization")
                .and_then(|h| h.strip_prefix("Bearer "))
                .map(|t| t == token)
                .unwrap_or(false),
        }
    }

    fn handle_health(&self) -> String {
        json_response(
            200,
            &json!({
                "status": "ok",
                "model": self.model_name,
                "uptime_seconds": self.start_time.elapsed().as_secs(),
            }),
        )
    }

    fn handle_models(&self) -> String {
        json_response(
            200,
            &json!({
                "object": "list",
                "data": [{
                    "id": self.model_name,
                    "object": "model",
                    "created": 0,
                    "owned_by": "memvid-agent-core"
                }]
            }),
        )
    }

    fn handle_chat(&self, req: &HttpRequest) -> String {
        let body: serde_json::Value = match serde_json::from_str(&req.body) {
            Ok(v) => v,
            Err(e) => {
                return json_response(
                    400,
                    &json!({"error": {"message": format!("Invalid JSON: {}", e), "type": "invalid_request_error"}}),
                );
            }
        };

        let messages = match body["messages"].as_array() {
            Some(m) => m,
            None => {
                return json_response(
                    400,
                    &json!({"error": {"message": "Missing 'messages' field", "type": "invalid_request_error"}}),
                );
            }
        };

        let mut api_messages: Vec<Message> = Vec::with_capacity(messages.len());
        for msg in messages {
            let role = match msg["role"].as_str() {
                Some("user") => MessageRole::User,
                Some("assistant") => MessageRole::Assistant,
                Some("system") => MessageRole::System,
                Some("tool") => MessageRole::Tool,
                _ => continue,
            };
            let content = msg["content"].as_str().unwrap_or("");
            api_messages.push(Message {
                role,
                content: content.to_string(),
                timestamp: Utc::now(),
                tokens: None,
            });
        }

        let mut agent = self.agent.lock().unwrap();
        let content = match agent.chat_with_messages(&api_messages) {
            Ok(r) => r,
            Err(e) => {
                return json_response(
                    500,
                    &json!({"error": {"message": format!("{}", e), "type": "internal_error"}}),
                );
            }
        };

        json_response(
            200,
            &json!({
                "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                "object": "chat.completion",
                "model": self.model_name,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": content,
                    },
                    "finish_reason": "stop",
                }],
                "usage": {
                    "prompt_tokens": 0,
                    "completion_tokens": 0,
                    "total_tokens": 0,
                }
            }),
        )
    }

    fn handle_token(&self, _req: &HttpRequest) -> String {
        let token = self.token.clone().unwrap_or_else(|| String::new());
        json_response(
            200,
            &json!({
                "token": token,
                "model": self.model_name,
            }),
        )
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buf = Vec::new();
    let mut temp = [0u8; 4096];

    loop {
        let n = stream.read(&mut temp)?;
        if n == 0 {
            anyhow::bail!("Connection closed");
        }
        buf.extend_from_slice(&temp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let request_str = String::from_utf8_lossy(&buf);
    let header_end = request_str.find("\r\n\r\n").unwrap();

    let mut method = String::new();
    let mut path = String::new();
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut content_length: usize = 0;

    for (i, line) in request_str[..header_end].lines().enumerate() {
        if i == 0 {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                method = parts[0].to_string();
                path = parts[1].to_string();
            }
        } else if !line.is_empty() {
            if let Some(pos) = line.find(':') {
                headers.insert(
                    line[..pos].trim().to_lowercase(),
                    line[pos + 1..].trim().to_string(),
                );
            }
        }
    }

    if let Some(cl) = headers.get("content-length") {
        content_length = cl.parse().unwrap_or(0);
    }

    let body_received = buf.len().saturating_sub(header_end + 4);
    if body_received < content_length {
        let needed = content_length - body_received;
        let mut remaining = vec![0u8; needed];
        stream.read_exact(&mut remaining)?;
        buf.extend_from_slice(&remaining);
    }

    let full_request = String::from_utf8_lossy(&buf);
    let body_start = full_request.find("\r\n\r\n").unwrap() + 4;
    let body = full_request[body_start..].to_string();

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn json_response(status: u16, body: &serde_json::Value) -> String {
    let body_str = serde_json::to_string(body).unwrap_or_default();
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };

    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
        status,
        status_text,
        body_str.len(),
        body_str
    )
}

fn unauthorized() -> String {
    json_response(
        401,
        &json!({"error": {"message": "Unauthorized", "type": "auth_error"}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_response_format() {
        let resp = json_response(200, &json!({"status": "ok"}));
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("Content-Type: application/json"));
        assert!(resp.contains("\"status\":\"ok\""));
    }

    #[test]
    fn unauthorized_response() {
        let resp = unauthorized();
        assert!(resp.starts_with("HTTP/1.1 401 Unauthorized"));
        assert!(resp.contains("Unauthorized"));
    }

    #[test]
    fn parse_http_get_request() {
        let raw = "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = read_http_request_raw(raw);
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/health");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_http_post_with_body() {
        let raw = "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 27\r\n\r\n{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}";
        let req = read_http_request_raw(raw);
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/v1/chat/completions");
        assert!(req.body.contains("hi"));
    }

    #[test]
    fn parse_http_authorization_header() {
        let raw = "GET /v1/models HTTP/1.1\r\nAuthorization: Bearer abc123\r\n\r\n";
        let req = read_http_request_raw(raw);
        assert_eq!(req.headers.get("authorization").unwrap(), "Bearer abc123");
    }

    fn read_http_request_raw(raw: &str) -> HttpRequest {
        let mut method = String::new();
        let mut path = String::new();
        let mut headers = HashMap::new();

        let header_end = raw.find("\r\n\r\n").unwrap_or(raw.len());
        for (i, line) in raw[..header_end].lines().enumerate() {
            if i == 0 {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    method = parts[0].to_string();
                    path = parts[1].to_string();
                }
            } else if !line.is_empty() {
                if let Some(pos) = line.find(':') {
                    headers.insert(
                        line[..pos].trim().to_lowercase(),
                        line[pos + 1..].trim().to_string(),
                    );
                }
            }
        }

        let body_start = raw.find("\r\n\r\n").map(|i| i + 4).unwrap_or(raw.len());
        let body = raw[body_start..].to_string();

        HttpRequest {
            method,
            path,
            headers,
            body,
        }
    }

    fn test_server(token: Option<String>) -> (ApiServer, tempfile::TempDir) {
        use crate::agent::Agent;
        use crate::context_policy::ContextPolicy;
        use crate::llama::context::LlamaContext;
        use crate::memvid::writer::MemvidWriter;
        use crate::prompt::{ChatTemplate, PromptBuilder};
        use crate::retrieval::KnowledgeIndex;
        use crate::session::Session;
        use crate::types::WriterConfig;

        let dir = tempfile::tempdir().unwrap();
        let writer_config = WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let memory = MemvidWriter::init(writer_config).unwrap();
        let knowledge_index = KnowledgeIndex::load(dir.path()).unwrap();

        let agent = Agent::from_components(
            LlamaContext::null(),
            memory,
            knowledge_index,
            "test-model".to_string(),
            Session::new(),
            PromptBuilder::new(ChatTemplate::ChatML),
            ContextPolicy::new(4096, 2048),
        );

        let server = ApiServer::new(
            Arc::new(Mutex::new(agent)),
            "test-model".to_string(),
            "127.0.0.1".to_string(),
            8787,
            token,
        );
        (server, dir)
    }

    #[test]
    fn route_health_returns_200() {
        let (server, _dir) = test_server(None);
        let req = read_http_request_raw("GET /health HTTP/1.1\r\n\r\n");
        let resp = server.route(&req);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("test-model"));
    }

    #[test]
    fn route_unknown_returns_404() {
        let (server, _dir) = test_server(None);
        let req = read_http_request_raw("GET /unknown HTTP/1.1\r\n\r\n");
        let resp = server.route(&req);
        assert!(resp.starts_with("HTTP/1.1 404 Not Found"));
    }

    #[test]
    fn route_models_unauthorized() {
        let (server, _dir) = test_server(Some("secret".to_string()));
        let req = read_http_request_raw("GET /v1/models HTTP/1.1\r\n\r\n");
        let resp = server.route(&req);
        assert!(resp.starts_with("HTTP/1.1 401 Unauthorized"));
    }

    #[test]
    fn route_models_authorized() {
        let (server, _dir) = test_server(Some("secret".to_string()));
        let req = read_http_request_raw(
            "GET /v1/models HTTP/1.1\r\nAuthorization: Bearer secret\r\n\r\n",
        );
        let resp = server.route(&req);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("test-model"));
    }

    #[test]
    fn check_auth_no_token() {
        let (server, _dir) = test_server(None);
        let req = read_http_request_raw("GET /health HTTP/1.1\r\n\r\n");
        assert!(server.check_auth(&req));
    }

    #[test]
    fn check_auth_valid_token() {
        let (server, _dir) = test_server(Some("abc".to_string()));
        let req =
            read_http_request_raw("GET /v1/models HTTP/1.1\r\nAuthorization: Bearer abc\r\n\r\n");
        assert!(server.check_auth(&req));
    }

    #[test]
    fn check_auth_invalid_token() {
        let (server, _dir) = test_server(Some("abc".to_string()));
        let req =
            read_http_request_raw("GET /v1/models HTTP/1.1\r\nAuthorization: Bearer wrong\r\n\r\n");
        assert!(!server.check_auth(&req));
    }

    #[test]
    fn check_auth_malformed_header() {
        let (server, _dir) = test_server(Some("abc".to_string()));
        let req =
            read_http_request_raw("GET /v1/models HTTP/1.1\r\nAuthorization: Basic abc\r\n\r\n");
        assert!(!server.check_auth(&req));
    }

    #[test]
    fn route_token_returns_token_when_configured() {
        let (server, _dir) = test_server(Some("my-token".to_string()));
        let req =
            read_http_request_raw("GET /token HTTP/1.1\r\nAuthorization: Bearer my-token\r\n\r\n");
        let resp = server.route(&req);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("my-token"));
    }

    #[test]
    fn route_post_token_works() {
        let (server, _dir) = test_server(Some("my-token".to_string()));
        let req =
            read_http_request_raw("POST /token HTTP/1.1\r\nAuthorization: Bearer my-token\r\n\r\n");
        let resp = server.route(&req);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("my-token"));
    }
}
