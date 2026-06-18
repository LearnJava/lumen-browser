//! IPC channel between Lumen processes.
//!
//! Protocol: length-prefixed bincode over TCP loopback (127.0.0.1).
//! The network service acts as TCP server; the shell connects as client.
//!
//! Wire format per message:
//! ```text
//! [4 bytes: u32 LE body_len][body_len bytes: bincode payload]
//! ```
//!
//! Phase 1: single synchronous connection, one in-flight request at a time.
//!
//! Two roles share this framing layer:
//! - **Network service** (PH1-4): the network process is the TCP server, the
//!   shell is the client; messages are `Fetch`/`Ping`/`Shutdown`.
//! - **Tab control channel** (TAB-4/TAB-5): the shell, started with
//!   `--ipc-server`, is the TCP server; an external controller (e.g.
//!   `graphic_tests/run.py`) is the client and drives tabs via `CreateTab` /
//!   `NavigateTab` / `Screenshot` / `CloseTab`. This lets the controller open
//!   the browser once and pull PNGs over IPC instead of gdigrab/ffmpeg.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use serde::{Deserialize, Serialize};

use lumen_core::error::{Error, Result};

// ── Message types ────────────────────────────────────────────────────────────

/// Identifier for a tab in the shell's `--ipc-server` control channel (TAB-4).
///
/// Assigned by the shell when it replies to `CreateTab`; the controller echoes
/// it back on every subsequent per-tab command. Monotonic, never reused within
/// a server lifetime.
pub type TabId = u32;

/// A request sent over an IPC channel.
///
/// `Fetch`/`Ping`/`Shutdown` are used by the network-service channel; the
/// `*Tab`/`Screenshot` variants are used by the shell's `--ipc-server` tab
/// control channel (TAB-4).
#[derive(Debug, Serialize, Deserialize)]
pub enum IpcRequest {
    /// Fetch a URL and return the response body.
    Fetch(FetchRequest),
    /// Health-check — service replies with `IpcResponse::Pong`.
    Ping,
    /// Orderly shutdown — service exits after sending `IpcResponse::Shutdown`.
    Shutdown,
    /// TAB-4: allocate a new headless tab. Reply: `TabCreated { tab_id }`.
    CreateTab,
    /// TAB-4: close the tab `tab_id`. Reply: `TabClosed { tab_id }` (or
    /// `TabError` if the id is unknown).
    CloseTab {
        /// Tab to close.
        tab_id: TabId,
    },
    /// TAB-4: navigate `tab_id` to `url` (load + parse + layout). Reply:
    /// `Navigated { tab_id }` once the page is ready to be screenshotted.
    NavigateTab {
        /// Tab to navigate.
        tab_id: TabId,
        /// Absolute URL or local file path to load.
        url: String,
    },
    /// TAB-5: render `tab_id` offscreen (CPU path) and return a PNG. Reply:
    /// `Screenshot { tab_id, png }`.
    Screenshot {
        /// Tab to render.
        tab_id: TabId,
    },
}

/// A response sent back over an IPC channel.
#[derive(Debug, Serialize, Deserialize)]
pub enum IpcResponse {
    /// Successful fetch — body bytes of the HTTP response.
    FetchOk(FetchOk),
    /// Failed fetch — human-readable error message.
    FetchErr(FetchErr),
    /// Reply to `IpcRequest::Ping`.
    Pong,
    /// Acknowledgement of `IpcRequest::Shutdown` before the service exits.
    Shutdown,
    /// TAB-4: reply to `CreateTab` — the freshly allocated tab id.
    TabCreated {
        /// Id of the new tab.
        tab_id: TabId,
    },
    /// TAB-4: reply to `CloseTab`.
    TabClosed {
        /// Id of the closed tab.
        tab_id: TabId,
    },
    /// TAB-4: reply to `NavigateTab` once the page has been loaded + laid out.
    Navigated {
        /// Id of the navigated tab.
        tab_id: TabId,
    },
    /// TAB-5: reply to `Screenshot` — PNG-encoded RGBA8 render of the tab.
    Screenshot {
        /// Id of the rendered tab.
        tab_id: TabId,
        /// PNG bytes (RGBA8).
        png: Vec<u8>,
    },
    /// TAB-4: a per-tab command failed (unknown id, load/render error, …).
    TabError {
        /// Tab the failing command referenced.
        tab_id: TabId,
        /// Human-readable failure reason.
        message: String,
    },
}

/// Parameters for a fetch request (Phase 1: GET-only, no custom headers/body).
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchRequest {
    /// Monotonically increasing identifier assigned by the shell. Used to correlate
    /// responses when multiplexing is added in Phase 2.
    pub id: u64,
    /// Absolute URL string (scheme + host + path + query).
    pub url: String,
    /// HTTP method, e.g. `"GET"` or `"POST"`.
    pub method: String,
    /// Additional request headers beyond the defaults in HttpClient.
    pub headers: Vec<(String, String)>,
    /// Request body for POST/PUT; `None` for GET/HEAD.
    pub body: Option<Vec<u8>>,
}

/// Successful HTTP response payload returned by the network service.
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchOk {
    /// Echoed from the corresponding `FetchRequest::id`.
    pub id: u64,
    /// HTTP status code (e.g. 200, 301, 404).
    pub status: u16,
    /// Response headers as (name, value) pairs.
    pub headers: Vec<(String, String)>,
    /// Decoded response body bytes.
    pub body: Vec<u8>,
}

/// Error returned when a fetch fails.
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchErr {
    /// Echoed from the corresponding `FetchRequest::id`.
    pub id: u64,
    /// Human-readable error from `lumen_core::error::Error`.
    pub error: String,
}

// ── IpcChannel ───────────────────────────────────────────────────────────────

/// Bidirectional framing layer over any `Read + Write` stream.
///
/// Sends and receives length-prefixed bincode messages. Thread-unsafe — wrap in a
/// `Mutex` for concurrent access.
pub struct IpcChannel<S> {
    stream: S,
}

impl<S: Read + Write> IpcChannel<S> {
    /// Wrap an existing stream.
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    /// Serialize `msg` via bincode and write it with a 4-byte LE length prefix.
    pub fn send<T: Serialize>(&mut self, msg: &T) -> Result<()> {
        let bytes =
            bincode::serialize(msg).map_err(|e| Error::Io(format!("ipc serialize: {e}")))?;
        let len = u32::try_from(bytes.len())
            .map_err(|_| Error::Io("ipc message too large".to_owned()))?;
        self.stream
            .write_all(&len.to_le_bytes())
            .map_err(|e| Error::Io(format!("ipc write len: {e}")))?;
        self.stream
            .write_all(&bytes)
            .map_err(|e| Error::Io(format!("ipc write body: {e}")))?;
        self.stream
            .flush()
            .map_err(|e| Error::Io(format!("ipc flush: {e}")))?;
        Ok(())
    }

    /// Read one length-prefixed message and deserialize it.
    pub fn recv<T: for<'de> Deserialize<'de>>(&mut self) -> Result<T> {
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .map_err(|e| Error::Io(format!("ipc read len: {e}")))?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        self.stream
            .read_exact(&mut buf)
            .map_err(|e| Error::Io(format!("ipc read body: {e}")))?;
        bincode::deserialize(&buf).map_err(|e| Error::Io(format!("ipc deserialize: {e}")))
    }
}

// ── IpcServer ────────────────────────────────────────────────────────────────

/// TCP server that the network service uses to accept connections from the shell.
pub struct IpcServer {
    listener: TcpListener,
}

impl IpcServer {
    /// Bind on an OS-assigned loopback port. Returns `(server, bound_port)`.
    ///
    /// The network service prints the port to stdout so the shell can connect.
    pub fn bind() -> Result<(Self, u16)> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| Error::Io(format!("ipc bind: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::Io(format!("ipc local_addr: {e}")))?
            .port();
        Ok((Self { listener }, port))
    }

    /// Block until the shell connects and return the framing channel.
    pub fn accept(&self) -> Result<IpcChannel<TcpStream>> {
        let (stream, _addr) = self
            .listener
            .accept()
            .map_err(|e| Error::Io(format!("ipc accept: {e}")))?;
        // Disable Nagle — our messages are already framed; latency > throughput.
        stream
            .set_nodelay(true)
            .map_err(|e| Error::Io(format!("ipc set_nodelay: {e}")))?;
        Ok(IpcChannel::new(stream))
    }
}

// ── IpcClient ────────────────────────────────────────────────────────────────

/// Client used by the shell to communicate with the network service.
///
/// Phase 1: single blocking connection, one in-flight request at a time.
/// Wrap in `Mutex<IpcClient>` for multi-threaded use.
pub struct IpcClient {
    channel: IpcChannel<TcpStream>,
}

impl IpcClient {
    /// Connect to the network service listening on `127.0.0.1:port`.
    pub fn connect(port: u16) -> Result<Self> {
        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|e| Error::Io(format!("ipc connect port {port}: {e}")))?;
        stream
            .set_nodelay(true)
            .map_err(|e| Error::Io(format!("ipc set_nodelay: {e}")))?;
        Ok(Self { channel: IpcChannel::new(stream) })
    }

    /// Send a request and block until the matching response arrives.
    pub fn request(&mut self, req: &IpcRequest) -> Result<IpcResponse> {
        self.channel.send(req)?;
        self.channel.recv()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a message through a real TCP loopback connection.
    #[test]
    fn test_round_trip_ping() {
        let (server, port) = IpcServer::bind().unwrap();

        // Server thread.
        let server_thread = std::thread::spawn(move || {
            let mut ch = server.accept().unwrap();
            let req: IpcRequest = ch.recv().unwrap();
            assert!(matches!(req, IpcRequest::Ping));
            ch.send(&IpcResponse::Pong).unwrap();
        });

        let mut client = IpcClient::connect(port).unwrap();
        let resp = client.request(&IpcRequest::Ping).unwrap();
        assert!(matches!(resp, IpcResponse::Pong));

        server_thread.join().unwrap();
    }

    /// Ensure FetchRequest/FetchOk round-trip through the framing layer.
    #[test]
    fn test_round_trip_fetch() {
        let (server, port) = IpcServer::bind().unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut ch = server.accept().unwrap();
            let req: IpcRequest = ch.recv().unwrap();
            if let IpcRequest::Fetch(fr) = req {
                assert_eq!(fr.id, 42);
                assert_eq!(fr.url, "https://example.com/");
                ch.send(&IpcResponse::FetchOk(FetchOk {
                    id: fr.id,
                    status: 200,
                    headers: vec![("content-type".into(), "text/html".into())],
                    body: b"<html/>".to_vec(),
                }))
                .unwrap();
            } else {
                panic!("expected Fetch request");
            }
        });

        let mut client = IpcClient::connect(port).unwrap();
        let resp = client
            .request(&IpcRequest::Fetch(FetchRequest {
                id: 42,
                url: "https://example.com/".into(),
                method: "GET".into(),
                headers: vec![],
                body: None,
            }))
            .unwrap();

        if let IpcResponse::FetchOk(ok) = resp {
            assert_eq!(ok.status, 200);
            assert_eq!(ok.body, b"<html/>");
        } else {
            panic!("expected FetchOk response");
        }

        server_thread.join().unwrap();
    }

    /// Ensure FetchErr is correctly transported.
    #[test]
    fn test_round_trip_fetch_err() {
        let (server, port) = IpcServer::bind().unwrap();

        let server_thread = std::thread::spawn(move || {
            let mut ch = server.accept().unwrap();
            let req: IpcRequest = ch.recv().unwrap();
            if let IpcRequest::Fetch(fr) = req {
                ch.send(&IpcResponse::FetchErr(FetchErr {
                    id: fr.id,
                    error: "connection refused".into(),
                }))
                .unwrap();
            }
        });

        let mut client = IpcClient::connect(port).unwrap();
        let resp = client
            .request(&IpcRequest::Fetch(FetchRequest {
                id: 7,
                url: "https://unreachable.example/".into(),
                method: "GET".into(),
                headers: vec![],
                body: None,
            }))
            .unwrap();

        if let IpcResponse::FetchErr(err) = resp {
            assert_eq!(err.id, 7);
            assert!(err.error.contains("connection refused"));
        } else {
            panic!("expected FetchErr response");
        }

        server_thread.join().unwrap();
    }

    /// Verify large payloads (>64 KB) survive the 4-byte length prefix.
    #[test]
    fn test_large_body() {
        let (server, port) = IpcServer::bind().unwrap();
        let big_body: Vec<u8> = (0u8..255).cycle().take(128 * 1024).collect();
        let expected = big_body.clone();

        let server_thread = std::thread::spawn(move || {
            let mut ch = server.accept().unwrap();
            let req: IpcRequest = ch.recv().unwrap();
            if let IpcRequest::Fetch(fr) = req {
                ch.send(&IpcResponse::FetchOk(FetchOk {
                    id: fr.id,
                    status: 200,
                    headers: vec![],
                    body: big_body,
                }))
                .unwrap();
            }
        });

        let mut client = IpcClient::connect(port).unwrap();
        let resp = client
            .request(&IpcRequest::Fetch(FetchRequest {
                id: 1,
                url: "https://example.com/large".into(),
                method: "GET".into(),
                headers: vec![],
                body: None,
            }))
            .unwrap();

        if let IpcResponse::FetchOk(ok) = resp {
            assert_eq!(ok.body, expected);
        } else {
            panic!("expected FetchOk");
        }

        server_thread.join().unwrap();
    }

    /// TAB-4: drive a full create → navigate → screenshot → close sequence
    /// through the framing layer with the shell acting as TCP server.
    #[test]
    fn test_tab_control_round_trip() {
        let (server, port) = IpcServer::bind().unwrap();
        let png_bytes = vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4];
        let expected_png = png_bytes.clone();

        let server_thread = std::thread::spawn(move || {
            let mut ch = server.accept().unwrap();
            // CreateTab → TabCreated { 7 }
            assert!(matches!(ch.recv::<IpcRequest>().unwrap(), IpcRequest::CreateTab));
            ch.send(&IpcResponse::TabCreated { tab_id: 7 }).unwrap();
            // NavigateTab → Navigated
            match ch.recv::<IpcRequest>().unwrap() {
                IpcRequest::NavigateTab { tab_id, url } => {
                    assert_eq!(tab_id, 7);
                    assert_eq!(url, "https://example.com/");
                    ch.send(&IpcResponse::Navigated { tab_id }).unwrap();
                }
                other => panic!("expected NavigateTab, got {other:?}"),
            }
            // Screenshot → Screenshot { png }
            match ch.recv::<IpcRequest>().unwrap() {
                IpcRequest::Screenshot { tab_id } => {
                    assert_eq!(tab_id, 7);
                    ch.send(&IpcResponse::Screenshot { tab_id, png: png_bytes })
                        .unwrap();
                }
                other => panic!("expected Screenshot, got {other:?}"),
            }
            // CloseTab → TabClosed
            match ch.recv::<IpcRequest>().unwrap() {
                IpcRequest::CloseTab { tab_id } => {
                    assert_eq!(tab_id, 7);
                    ch.send(&IpcResponse::TabClosed { tab_id }).unwrap();
                }
                other => panic!("expected CloseTab, got {other:?}"),
            }
        });

        let mut client = IpcClient::connect(port).unwrap();
        let tab_id = match client.request(&IpcRequest::CreateTab).unwrap() {
            IpcResponse::TabCreated { tab_id } => tab_id,
            other => panic!("expected TabCreated, got {other:?}"),
        };
        assert_eq!(tab_id, 7);
        assert!(matches!(
            client
                .request(&IpcRequest::NavigateTab {
                    tab_id,
                    url: "https://example.com/".into(),
                })
                .unwrap(),
            IpcResponse::Navigated { tab_id: 7 }
        ));
        match client.request(&IpcRequest::Screenshot { tab_id }).unwrap() {
            IpcResponse::Screenshot { tab_id: 7, png } => assert_eq!(png, expected_png),
            other => panic!("expected Screenshot, got {other:?}"),
        }
        assert!(matches!(
            client.request(&IpcRequest::CloseTab { tab_id }).unwrap(),
            IpcResponse::TabClosed { tab_id: 7 }
        ));

        server_thread.join().unwrap();
    }
}
