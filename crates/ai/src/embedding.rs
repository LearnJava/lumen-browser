//! Embedding backend abstraction + Ollama HTTP implementation (ADR-019,
//! `docs/tasks/ph3-ai-module.md` Step 2).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::http::http_response_body;

/// Default Ollama HTTP API port (`127.0.0.1:11434`).
const DEFAULT_OLLAMA_PORT: u16 = 11434;

/// How long to wait for a response from the embedding backend before giving up.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Produces a dense embedding vector for a piece of text (§12.5, §12.8).
///
/// Implementors talk to whatever runtime backs the embedding model (local
/// process, in-process runtime, …). Callers treat an `Err` the same way as
/// `AiBackend::embed`'s empty-vector case: "no embedding available".
pub trait EmbeddingBackend: Send + Sync {
    /// Embed `text` into a dense vector, or `Err` if the backend is
    /// unreachable or its response cannot be parsed.
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
}

/// Failure to embed text via an [`EmbeddingBackend`].
#[derive(Debug)]
pub enum EmbeddingError {
    /// Could not connect to or communicate with the backend process.
    Io(std::io::Error),
    /// The backend responded but the body was not the expected JSON shape.
    InvalidResponse(String),
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddingError::Io(e) => write!(f, "embedding backend I/O error: {e}"),
            EmbeddingError::InvalidResponse(msg) => {
                write!(f, "embedding backend returned an invalid response: {msg}")
            }
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// Embedding backend that talks to a local Ollama daemon over plain HTTP
/// (`POST /api/embeddings`), per ADR-019.
///
/// Connects only to `127.0.0.1` by construction — no user-supplied URL, no
/// TLS, no `reqwest`/`hyper` dependency: the request is framed by hand over
/// `std::net::TcpStream` and the response parsed with `serde_json`.
pub struct OllamaEmbeddingBackend {
    port: u16,
    model: String,
}

impl OllamaEmbeddingBackend {
    /// New backend targeting the default Ollama port (11434) with `model`
    /// (e.g. `"nomic-embed-text"`).
    pub fn new(model: impl Into<String>) -> Self {
        Self { port: DEFAULT_OLLAMA_PORT, model: model.into() }
    }

    fn send_request(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let payload = serde_json::json!({ "model": self.model, "prompt": text }).to_string();
        let request = format!(
            "POST /api/embeddings HTTP/1.1\r\n\
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
            TcpStream::connect(("127.0.0.1", self.port)).map_err(EmbeddingError::Io)?;
        stream.set_read_timeout(Some(REQUEST_TIMEOUT)).map_err(EmbeddingError::Io)?;
        stream.set_write_timeout(Some(REQUEST_TIMEOUT)).map_err(EmbeddingError::Io)?;
        stream.write_all(request.as_bytes()).map_err(EmbeddingError::Io)?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(EmbeddingError::Io)?;

        let body = http_response_body(&response).ok_or_else(|| {
            EmbeddingError::InvalidResponse("malformed HTTP response (no header/body split)".to_owned())
        })?;
        parse_embedding_body(body)
    }
}

impl EmbeddingBackend for OllamaEmbeddingBackend {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.send_request(text)
    }
}

impl lumen_core::ext::AiBackend for OllamaEmbeddingBackend {
    fn query(&self, _prompt: &str) -> String {
        // Chat/generation lands in Step 4 (GenerationBackend, RagEngine);
        // this backend only implements embeddings so far.
        String::new()
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        EmbeddingBackend::embed(self, text).unwrap_or_default()
    }
}

/// Parse an Ollama `/api/embeddings` response body: `{"embedding": [f32, ...]}`.
fn parse_embedding_body(body: &[u8]) -> Result<Vec<f32>, EmbeddingError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| EmbeddingError::InvalidResponse(e.to_string()))?;
    let array = value
        .get("embedding")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| EmbeddingError::InvalidResponse("missing `embedding` array".to_owned()))?;
    array
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| EmbeddingError::InvalidResponse("non-numeric embedding element".to_owned()))
        })
        .collect()
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
    fn embed_parses_ollama_response_vector() {
        let port = spawn_mock_server(r#"{"embedding":[0.1,0.2,0.3,0.4]}"#);
        let backend = OllamaEmbeddingBackend { port, model: "nomic-embed-text".to_owned() };

        let vector = EmbeddingBackend::embed(&backend, "hello world").expect("embed should succeed");

        assert_eq!(vector.len(), 4);
        assert!((vector[0] - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn embed_rejects_malformed_json() {
        let port = spawn_mock_server("not json");
        let backend = OllamaEmbeddingBackend { port, model: "nomic-embed-text".to_owned() };

        let result = EmbeddingBackend::embed(&backend, "hello world");

        assert!(matches!(result, Err(EmbeddingError::InvalidResponse(_))));
    }

    #[test]
    fn embed_rejects_response_missing_embedding_field() {
        let port = spawn_mock_server(r#"{"unexpected":"shape"}"#);
        let backend = OllamaEmbeddingBackend { port, model: "nomic-embed-text".to_owned() };

        let result = EmbeddingBackend::embed(&backend, "hello world");

        assert!(matches!(result, Err(EmbeddingError::InvalidResponse(_))));
    }

    #[test]
    fn ai_backend_embed_delegates_to_embedding_backend() {
        use lumen_core::ext::AiBackend;

        let port = spawn_mock_server(r#"{"embedding":[1.0,2.0]}"#);
        let backend = OllamaEmbeddingBackend { port, model: "nomic-embed-text".to_owned() };

        assert_eq!(AiBackend::embed(&backend, "hello world"), vec![1.0, 2.0]);
        assert_eq!(backend.query("anything"), "");
    }

    #[test]
    fn ai_backend_embed_defaults_to_empty_vector_on_error() {
        use lumen_core::ext::AiBackend;

        let port = spawn_mock_server("not json");
        let backend = OllamaEmbeddingBackend { port, model: "nomic-embed-text".to_owned() };

        assert_eq!(AiBackend::embed(&backend, "hello world"), Vec::<f32>::new());
    }
}
