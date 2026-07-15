//! Generation backend abstraction + Ollama HTTP implementation (ADR-019,
//! `docs/tasks/ph3-ai-module.md` Step 4).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::http::http_response_body;

/// Default Ollama HTTP API port (`127.0.0.1:11434`).
const DEFAULT_OLLAMA_PORT: u16 = 11434;

/// How long to wait for a response from the generation backend before giving up.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Produces free-form text from a prompt plus optional retrieved context
/// (§12.5, §12.8 — summarisation and RAG-augmented answers).
///
/// Implementors talk to whatever runtime backs the generation model (local
/// process, in-process runtime, …). Callers treat an `Err` the same way as
/// `AiBackend::query`/`summarise`'s empty-string case: "no answer available".
pub trait GenerationBackend: Send + Sync {
    /// Generate a response to `prompt`, optionally grounded in `context`
    /// (e.g. retrieved knowledge-store chunks for RAG). Pass an empty
    /// `context` for a bare prompt.
    fn generate(&self, prompt: &str, context: &str) -> Result<String, GenerationError>;
}

/// Failure to generate text via a [`GenerationBackend`].
#[derive(Debug)]
pub enum GenerationError {
    /// Could not connect to or communicate with the backend process.
    Io(std::io::Error),
    /// The backend responded but the body was not the expected JSON shape.
    InvalidResponse(String),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationError::Io(e) => write!(f, "generation backend I/O error: {e}"),
            GenerationError::InvalidResponse(msg) => {
                write!(f, "generation backend returned an invalid response: {msg}")
            }
        }
    }
}

impl std::error::Error for GenerationError {}

/// Generation backend that talks to a local Ollama daemon over plain HTTP
/// (`POST /api/generate`), per ADR-019.
///
/// Connects only to `127.0.0.1` by construction — no user-supplied URL, no
/// TLS, no `reqwest`/`hyper` dependency: the request is framed by hand over
/// `std::net::TcpStream` and the response parsed with `serde_json`.
pub struct OllamaGenerationBackend {
    port: u16,
    model: String,
}

impl OllamaGenerationBackend {
    /// New backend targeting the default Ollama port (11434) with `model`
    /// (e.g. `"phi3:mini"`).
    pub fn new(model: impl Into<String>) -> Self {
        Self { port: DEFAULT_OLLAMA_PORT, model: model.into() }
    }

    fn send_request(&self, prompt: &str) -> Result<String, GenerationError> {
        let payload =
            serde_json::json!({ "model": self.model, "prompt": prompt, "stream": false })
                .to_string();
        let request = format!(
            "POST /api/generate HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {len}\r\n\
             Connection: close\r\n\
             \r\n\
             {payload}",
            port = self.port,
            len = payload.len(),
        );

        let mut stream =
            TcpStream::connect(("127.0.0.1", self.port)).map_err(GenerationError::Io)?;
        stream.set_read_timeout(Some(REQUEST_TIMEOUT)).map_err(GenerationError::Io)?;
        stream.set_write_timeout(Some(REQUEST_TIMEOUT)).map_err(GenerationError::Io)?;
        stream.write_all(request.as_bytes()).map_err(GenerationError::Io)?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(GenerationError::Io)?;

        let body = http_response_body(&response).ok_or_else(|| {
            GenerationError::InvalidResponse("malformed HTTP response (no header/body split)".to_owned())
        })?;
        parse_generate_body(body)
    }
}

impl GenerationBackend for OllamaGenerationBackend {
    fn generate(&self, prompt: &str, context: &str) -> Result<String, GenerationError> {
        let full_prompt = if context.is_empty() {
            prompt.to_owned()
        } else {
            format!("Context:\n{context}\n\nQuestion: {prompt}")
        };
        self.send_request(&full_prompt)
    }
}

impl lumen_core::ext::AiBackend for OllamaGenerationBackend {
    fn query(&self, prompt: &str) -> String {
        GenerationBackend::generate(self, prompt, "").unwrap_or_default()
    }

    fn summarise(&self, text: &str) -> String {
        let prompt = format!(
            "summarise: Summarise the following text in 1-2 short sentences:\n\n{text}"
        );
        self.send_request(&prompt).unwrap_or_default()
    }
}

/// Parse an Ollama `/api/generate` response body: `{"response": "...", ...}`.
fn parse_generate_body(body: &[u8]) -> Result<String, GenerationError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| GenerationError::InvalidResponse(e.to_string()))?;
    value
        .get("response")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| GenerationError::InvalidResponse("missing `response` string".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// Start a one-shot mock Ollama server on a random port: reads and
    /// discards the request headers, replies with `response_body` as the
    /// JSON body of a 200 OK, then closes the connection.
    fn spawn_mock_server(response_body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock listener");
        let port = listener.local_addr().expect("local addr").port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept mock connection");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                if line.trim().is_empty() {
                    break;
                }
                line.clear();
            }
            let mut stream = stream;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body,
            );
            let _ = stream.write_all(response.as_bytes());
        });
        port
    }

    #[test]
    fn generate_parses_ollama_response_text() {
        let port = spawn_mock_server(r#"{"response":"a short summary","done":true}"#);
        let backend = OllamaGenerationBackend { port, model: "phi3:mini".to_owned() };

        let text =
            GenerationBackend::generate(&backend, "summarise this", "").expect("generate should succeed");

        assert_eq!(text, "a short summary");
    }

    #[test]
    fn generate_rejects_malformed_json() {
        let port = spawn_mock_server("not json");
        let backend = OllamaGenerationBackend { port, model: "phi3:mini".to_owned() };

        let result = GenerationBackend::generate(&backend, "prompt", "");

        assert!(matches!(result, Err(GenerationError::InvalidResponse(_))));
    }

    #[test]
    fn generate_rejects_response_missing_response_field() {
        let port = spawn_mock_server(r#"{"unexpected":"shape"}"#);
        let backend = OllamaGenerationBackend { port, model: "phi3:mini".to_owned() };

        let result = GenerationBackend::generate(&backend, "prompt", "");

        assert!(matches!(result, Err(GenerationError::InvalidResponse(_))));
    }

    #[test]
    fn ai_backend_summarise_delegates_to_generation_backend() {
        use lumen_core::ext::AiBackend;

        let port = spawn_mock_server(r#"{"response":"summary text"}"#);
        let backend = OllamaGenerationBackend { port, model: "phi3:mini".to_owned() };

        assert_eq!(backend.summarise("long article text"), "summary text");
    }

    #[test]
    fn ai_backend_summarise_defaults_to_empty_string_on_error() {
        use lumen_core::ext::AiBackend;

        let port = spawn_mock_server("not json");
        let backend = OllamaGenerationBackend { port, model: "phi3:mini".to_owned() };

        assert_eq!(backend.summarise("long article text"), "");
    }

    #[test]
    fn ai_backend_query_delegates_to_generation_backend() {
        use lumen_core::ext::AiBackend;

        let port = spawn_mock_server(r#"{"response":"an answer"}"#);
        let backend = OllamaGenerationBackend { port, model: "phi3:mini".to_owned() };

        assert_eq!(backend.query("a question"), "an answer");
    }
}
