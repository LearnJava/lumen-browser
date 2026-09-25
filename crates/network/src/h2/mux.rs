//! Shared HTTP/2 connection with concurrent streams — RFC 9113 §5 (PERF-13).
//!
//! One [`H2Mux`] owns one connected [`H2Conn`] on a dedicated owner thread.
//! Any number of fetch threads submit requests to it concurrently; the owner
//! writes each request as a new stream, reads frames off the socket and hands
//! every completed response back to the thread that asked for it, matched by
//! stream id. This is what lets a page load keep a single TCP+TLS connection
//! per origin instead of paying a handshake for every parallel subresource
//! (BUG-1115: 98 connections for one github.com load before this).
//!
//! ## Why a thread and not async I/O
//!
//! The network stack is blocking (`std::net` + `rustls::StreamOwned`), and a
//! TLS stream cannot be split into independent read and write halves. The
//! owner thread therefore does both: it blocks on the command channel while
//! the connection is idle, and alternates "drain new requests → read one
//! frame with a short socket timeout" while streams are in flight. The poll
//! interval bounds the extra latency a request submitted mid-flight can see;
//! it is far below one TLS handshake, which is what the design saves.
//!
//! ## Lifetime
//!
//! The owner exits when the connection breaks, after the peer's GOAWAY once
//! its surviving streams finish, after [`IDLE_TIMEOUT`] without streams, or
//! when every [`H2Mux`] handle is gone. [`H2Mux::is_usable`] turns false
//! first, so the pool stops handing the connection out.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use lumen_core::error::Error;
use lumen_core::ext::AbortToken;

use super::conn::{H2Conn, H2Response, StreamEvent, H2_BODY_EXCEEDS_SEND_WINDOW};
use crate::{tls::CertInfo, RawStream};

/// Socket read timeout while streams are in flight — how long a request
/// submitted mid-flight may wait before the owner notices it. Windows rounds
/// `SO_RCVTIMEO` up to the system timer tick (measured 15.6 ms), which is the
/// real worst case there — still far below the handshake it saves.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Close a connection that carried no stream for this long. Shorter than the
/// common server-side HTTP/2 idle limits, so we normally close first instead
/// of discovering a dead socket on the next request.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// A stream that received no frame for this long fails with a timeout —
/// the multiplexed counterpart of the per-socket `FETCH_READ_TIMEOUT` of a
/// dedicated connection.
const STREAM_TIMEOUT: Duration = Duration::from_secs(60);

/// Concurrent-stream cap when the peer states no SETTINGS_MAX_CONCURRENT_STREAMS
/// (RFC 9113 §5.1.2 recommends peers allow at least 100).
const DEFAULT_MAX_STREAMS: usize = 100;

/// How often a waiting requester checks its abort token.
const ABORT_POLL: Duration = Duration::from_millis(50);

/// One request as handed to the owner thread — owned, because it crosses
/// threads.
pub(crate) struct MuxRequest {
    /// Request method (`GET`, `POST`, …).
    pub method: String,
    /// `:scheme` pseudo-header.
    pub scheme: String,
    /// `:authority` pseudo-header.
    pub authority: String,
    /// `:path` pseudo-header.
    pub path: String,
    /// Regular header fields, lowercase names, in send order.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Request body; empty for bodiless methods.
    pub body: Vec<u8>,
}

/// Why a multiplexed request produced no response.
#[derive(Debug)]
pub(crate) struct MuxError {
    /// The underlying error.
    pub error: Error,
    /// The peer certainly did not process the request (never sent, refused,
    /// beyond a GOAWAY's last stream id, or an idempotent request lost with
    /// the connection before any response header) — safe to resend it on a
    /// fresh connection.
    pub retryable: bool,
}

impl MuxError {
    fn retryable(msg: &str) -> Self {
        Self { error: Error::Network(format!("H2 connection unusable: {msg}")), retryable: true }
    }
}

type Reply = mpsc::SyncSender<Result<H2Response, MuxError>>;

enum Cmd {
    Request { id: u64, req: MuxRequest, reply: Reply },
    Cancel { id: u64 },
}

/// Handle to a shared multiplexed HTTP/2 connection. Cheap to share via
/// `Arc`; every method takes `&self`.
pub struct H2Mux {
    tx: mpsc::Sender<Cmd>,
    accepting: Arc<AtomicBool>,
    next_id: AtomicU64,
    cert_info: Option<CertInfo>,
}

impl std::fmt::Debug for H2Mux {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H2Mux").field("usable", &self.is_usable()).finish()
    }
}

impl H2Mux {
    /// Take over an established connection (preface and SETTINGS exchange
    /// done) and start its owner thread. `cert_info` is the certificate of
    /// the TLS handshake that opened it, reported on every response it serves.
    pub(crate) fn spawn(conn: H2Conn<RawStream>, cert_info: Option<CertInfo>) -> Result<Self, Error> {
        conn.transport()
            .set_read_timeout(Some(POLL_INTERVAL))
            .map_err(|e| Error::Network(format!("H2 set poll timeout: {e}")))?;
        let (tx, rx) = mpsc::channel();
        let accepting = Arc::new(AtomicBool::new(true));
        let owner = Owner {
            conn,
            rx,
            accepting: Arc::clone(&accepting),
            queued: VecDeque::new(),
            active: HashMap::new(),
        };
        std::thread::Builder::new()
            .name("lumen-h2-mux".to_owned())
            .spawn(move || owner.run())
            .map_err(|e| Error::Network(format!("H2 spawn owner thread: {e}")))?;
        Ok(Self { tx, accepting, next_id: AtomicU64::new(0), cert_info })
    }

    /// Whether new requests may still be submitted (the connection is alive
    /// and the peer has not sent GOAWAY).
    pub(crate) fn is_usable(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }

    /// Certificate of the TLS handshake that opened this connection.
    pub(crate) fn cert_info(&self) -> Option<CertInfo> {
        self.cert_info.clone()
    }

    /// Run one request on the shared connection and wait for its response.
    ///
    /// With an `abort` token, the wait is interrupted when it fires: the
    /// stream is cancelled (RST_STREAM) and `Error::Aborted` returned — the
    /// connection itself stays up for the other streams.
    pub(crate) fn request(
        &self,
        req: MuxRequest,
        abort: Option<&AbortToken>,
    ) -> Result<H2Response, MuxError> {
        if !self.is_usable() {
            return Err(MuxError::retryable("not accepting new streams"));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Cmd::Request { id, req, reply })
            .map_err(|_| MuxError::retryable("owner thread gone"))?;
        // The owner drops `reply` without answering only when it exits with
        // the request still unsent — so a disconnect is always retryable.
        let closed = || MuxError::retryable("closed before the request was sent");
        let Some(token) = abort else {
            return rx.recv().map_err(|_| closed())?;
        };
        loop {
            match rx.recv_timeout(ABORT_POLL) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(closed()),
                Err(mpsc::RecvTimeoutError::Timeout) if token.is_aborted() => {
                    let _ = self.tx.send(Cmd::Cancel { id });
                    return Err(MuxError {
                        error: Error::Aborted("fetch aborted".to_owned()),
                        retryable: false,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

/// A request whose stream is open on the wire.
struct Active {
    id: u64,
    reply: Reply,
    /// GET/HEAD/OPTIONS — may be resent if the connection dies before any
    /// response header (RFC 9110 §9.2.2).
    idempotent: bool,
    /// Last time a frame for this stream arrived (or it was opened).
    last_frame: Instant,
}

struct Owner {
    conn: H2Conn<RawStream>,
    rx: mpsc::Receiver<Cmd>,
    accepting: Arc<AtomicBool>,
    queued: VecDeque<(u64, MuxRequest, Reply)>,
    active: HashMap<u32, Active>,
}

impl Owner {
    fn run(mut self) {
        let fatal = self.drive();
        self.accepting.store(false, Ordering::Release);
        let msg = match &fatal {
            Some(e) => e.to_string(),
            None => "connection closed".to_owned(),
        };
        for (sid, a) in std::mem::take(&mut self.active) {
            let retryable = a.idempotent && !self.conn.response_started(sid);
            let _ = a.reply.send(Err(MuxError {
                error: Error::Network(format!("H2 connection lost: {msg}")),
                retryable,
            }));
        }
        self.fail_queued();
        if fatal.is_none() {
            self.conn.send_goaway();
        }
        // Commands still in the channel drop their `reply` here, which the
        // requester reads as a retryable disconnect.
    }

    /// The owner loop. Returns the connection error that ended it, or `None`
    /// for an orderly end (idle, GOAWAY drained, all handles dropped).
    fn drive(&mut self) -> Option<Error> {
        loop {
            if self.active.is_empty() && self.queued.is_empty() {
                if !self.accepting.load(Ordering::Acquire) {
                    return None;
                }
                // Idle: nothing to read for, block on the next command.
                match self.rx.recv_timeout(IDLE_TIMEOUT) {
                    Ok(cmd) => self.accept(cmd),
                    Err(_) => return None,
                }
            }
            while let Ok(cmd) = self.rx.try_recv() {
                self.accept(cmd);
            }
            if let Err(e) = self.start_queued() {
                return Some(e);
            }
            if self.active.is_empty() {
                continue;
            }
            match self.conn.poll_frame() {
                Ok(Some(frame)) => {
                    if let Some(a) = self.active.get_mut(&frame.stream_id()) {
                        a.last_frame = Instant::now();
                    }
                    match self.conn.handle_mux_frame(frame) {
                        Ok(Some(event)) => self.on_event(event),
                        Ok(None) => {}
                        Err(e) => return Some(e),
                    }
                }
                Ok(None) => self.expire_streams(),
                Err(e) => return Some(e),
            }
        }
    }

    fn accept(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Request { id, req, reply } => {
                if self.accepting.load(Ordering::Acquire) {
                    self.queued.push_back((id, req, reply));
                } else {
                    let _ = reply.send(Err(MuxError::retryable("peer sent GOAWAY")));
                }
            }
            Cmd::Cancel { id } => {
                self.queued.retain(|(qid, _, _)| *qid != id);
                if let Some(&sid) = self.active.iter().find(|(_, a)| a.id == id).map(|(s, _)| s) {
                    self.active.remove(&sid);
                    // A failed RST write surfaces on the next read as a
                    // connection error; nothing to do about it here.
                    let _ = self.conn.cancel_stream(sid);
                }
            }
        }
    }

    /// Open streams for queued requests while the peer's concurrency limit
    /// allows. `Err` means the connection broke while writing.
    fn start_queued(&mut self) -> Result<(), Error> {
        let limit = self
            .conn
            .max_concurrent_streams()
            .map_or(DEFAULT_MAX_STREAMS, |n| (n as usize).max(1));
        while self.active.len() < limit {
            if !self.conn.can_open_stream() {
                // Stream ids exhausted: this connection is done for new work.
                self.accepting.store(false, Ordering::Release);
                self.fail_queued();
                return Ok(());
            }
            let Some((id, req, reply)) = self.queued.pop_front() else {
                return Ok(());
            };
            let headers: Vec<(&[u8], &[u8])> =
                req.headers.iter().map(|(k, v)| (k.as_slice(), v.as_slice())).collect();
            match self.conn.open_stream(
                &req.method,
                &req.scheme,
                &req.authority,
                &req.path,
                &headers,
                &req.body,
            ) {
                Ok(sid) => {
                    let idempotent = matches!(req.method.as_str(), "GET" | "HEAD" | "OPTIONS");
                    self.active
                        .insert(sid, Active { id, reply, idempotent, last_frame: Instant::now() });
                }
                // A routing signal, not a broken connection: the caller
                // resends the request over HTTP/1.1.
                Err(e) if e.to_string().contains(H2_BODY_EXCEEDS_SEND_WINDOW) => {
                    let _ = reply.send(Err(MuxError { error: e, retryable: false }));
                }
                Err(e) => {
                    // The request's END_STREAM never reached the peer, so it
                    // cannot have been processed.
                    let _ = reply.send(Err(MuxError { error: e, retryable: true }));
                    return Err(Error::Network("H2 write failed".to_owned()));
                }
            }
        }
        Ok(())
    }

    fn on_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Complete(sid, resp) => {
                if let Some(a) = self.active.remove(&sid) {
                    let _ = a.reply.send(Ok(resp));
                }
            }
            StreamEvent::Failed { sid, error, retryable } => {
                if let Some(a) = self.active.remove(&sid) {
                    let _ = a.reply.send(Err(MuxError { error, retryable }));
                }
            }
            StreamEvent::GoAway { last_stream_id } => {
                self.accepting.store(false, Ordering::Release);
                let refused: Vec<u32> =
                    self.active.keys().copied().filter(|&sid| sid > last_stream_id).collect();
                for sid in refused {
                    if let Some(a) = self.active.remove(&sid) {
                        let _ = a.reply.send(Err(MuxError::retryable("peer sent GOAWAY")));
                    }
                }
                self.fail_queued();
            }
        }
    }

    /// Fail streams that went [`STREAM_TIMEOUT`] without a frame.
    fn expire_streams(&mut self) {
        let now = Instant::now();
        let expired: Vec<u32> = self
            .active
            .iter()
            .filter(|(_, a)| now.duration_since(a.last_frame) > STREAM_TIMEOUT)
            .map(|(&sid, _)| sid)
            .collect();
        for sid in expired {
            if let Some(a) = self.active.remove(&sid) {
                let _ = self.conn.cancel_stream(sid);
                let _ = a.reply.send(Err(MuxError {
                    error: Error::Network(format!(
                        "H2 stream {sid} timed out after {}s without data",
                        STREAM_TIMEOUT.as_secs()
                    )),
                    retryable: false,
                }));
            }
        }
    }

    fn fail_queued(&mut self) {
        for (_, _, reply) in self.queued.drain(..) {
            let _ = reply.send(Err(MuxError::retryable("connection closing")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h2::frame::{Frame, SETTING_MAX_CONCURRENT_STREAMS};
    use crate::h2::hpack::{Decoder, Encoder};
    use crate::h2::pool::{Acquire, H2Pool};
    use crate::pool::PoolKey;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::AtomicUsize;

    /// Minimal h2c server: answers every request with `200` and the request
    /// path as the body. Requests are collected in batches of `batch` and
    /// answered in REVERSE order, so a client that matched responses by
    /// arrival order instead of stream id would get the wrong bodies.
    fn serve(stream: TcpStream, batch: usize, max_streams: Option<u32>) {
        let mut s = stream;
        let mut magic = [0u8; 24];
        if s.read_exact(&mut magic).is_err() {
            return;
        }
        let mut out = Vec::new();
        let params = max_streams.map(|n| vec![(SETTING_MAX_CONCURRENT_STREAMS, n)]).unwrap_or_default();
        let _ = Frame::Settings { ack: false, params }.encode(&mut out);
        let _ = Frame::Settings { ack: true, params: vec![] }.encode(&mut out);
        if s.write_all(&out).is_err() {
            return;
        }
        let mut dec = Decoder::new();
        let mut enc = Encoder::new();
        let mut buf = Vec::new();
        let mut pending: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match s.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            while let Ok(Some((frame, used))) = Frame::parse(&buf, 1 << 20) {
                buf.drain(..used);
                if let Frame::Headers { stream_id, block_fragment, .. } = frame {
                    let fields = dec.decode(&block_fragment).unwrap_or_default();
                    let path = fields
                        .iter()
                        .find(|f| f.name == b":path")
                        .map(|f| f.value.clone())
                        .unwrap_or_default();
                    pending.push((stream_id, path));
                }
            }
            if pending.len() >= batch {
                let mut out = Vec::new();
                for (sid, path) in pending.drain(..).rev() {
                    let block = enc.encode(&[(b":status".as_slice(), b"200".as_slice())]);
                    let _ = Frame::Headers {
                        stream_id: sid,
                        end_stream: false,
                        end_headers: true,
                        priority: None,
                        block_fragment: block,
                    }
                    .encode(&mut out);
                    let _ = Frame::Data { stream_id: sid, end_stream: true, data: path }.encode(&mut out);
                }
                if s.write_all(&out).is_err() {
                    return;
                }
            }
        }
    }

    /// Start a server; returns its address and a counter of accepted
    /// connections.
    fn start_server(batch: usize, max_streams: Option<u32>) -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let accepted = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepted);
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || serve(s, batch, max_streams));
            }
        });
        (addr, accepted)
    }

    fn open_mux(addr: std::net::SocketAddr) -> H2Mux {
        let tcp = TcpStream::connect(addr).expect("connect");
        let conn = H2Conn::connect(RawStream::Plain(tcp)).expect("h2 preface");
        H2Mux::spawn(conn, None).expect("spawn")
    }

    fn get(path: &str) -> MuxRequest {
        MuxRequest {
            method: "GET".to_owned(),
            scheme: "http".to_owned(),
            authority: "test".to_owned(),
            path: path.to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn concurrent_requests_share_one_connection_and_match_by_stream_id() {
        const N: usize = 8;
        let (addr, accepted) = start_server(N, None);
        let mux = Arc::new(open_mux(addr));
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let mux = Arc::clone(&mux);
                std::thread::spawn(move || mux.request(get(&format!("/r{i}")), None).map(|r| (i, r)))
            })
            .collect();
        for h in handles {
            let (i, (status, _, body)) = h.join().expect("join").expect("response");
            assert_eq!(status, 200);
            assert_eq!(body, format!("/r{i}").into_bytes(), "response routed to the wrong request");
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 1, "all streams on one connection");
    }

    #[test]
    fn peer_stream_limit_queues_instead_of_failing() {
        // The server answers only once it holds 2 requests and allows 2
        // concurrent streams: 6 requests must go through in 3 rounds.
        let (addr, accepted) = start_server(2, Some(2));
        let mux = Arc::new(open_mux(addr));
        let handles: Vec<_> = (0..6)
            .map(|i| {
                let mux = Arc::clone(&mux);
                std::thread::spawn(move || mux.request(get(&format!("/q{i}")), None))
            })
            .collect();
        for h in handles {
            assert!(h.join().expect("join").is_ok());
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pool_coalesces_parallel_first_requests_into_one_handshake() {
        const N: usize = 6;
        let (addr, accepted) = start_server(N, None);
        let pool = Arc::new(H2Pool::new());
        let key = PoolKey { host: "127.0.0.1".to_owned(), port: addr.port(), is_tls: false };
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let pool = Arc::clone(&pool);
                let key = key.clone();
                std::thread::spawn(move || {
                    let mux = match pool.acquire(&key) {
                        Acquire::Mux(m) => m,
                        Acquire::Connect(r) => {
                            // Slow handshake: every other thread arrives
                            // while it is still pending.
                            std::thread::sleep(Duration::from_millis(100));
                            r.fulfill(open_mux(addr))
                        }
                        Acquire::Direct => panic!("no thread should bypass the pool"),
                    };
                    mux.request(get(&format!("/p{i}")), None)
                })
            })
            .collect();
        for h in handles {
            assert!(h.join().expect("join").is_ok());
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 1, "one handshake for all first requests");
        assert_eq!(pool.live_connections(), 1);
    }

    #[test]
    fn dead_connection_reports_retryable_and_stops_being_usable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut magic = [0u8; 24];
                let _ = s.read_exact(&mut magic);
                let mut out = Vec::new();
                let _ = Frame::Settings { ack: false, params: vec![] }.encode(&mut out);
                let _ = s.write_all(&out);
                // Read the client's frames, then hang up without answering.
                let mut chunk = [0u8; 1024];
                let _ = s.read(&mut chunk);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let mux = open_mux(addr);
        let err = mux.request(get("/x"), None).expect_err("connection dropped");
        assert!(err.retryable, "idempotent request lost before any response is retryable");
        std::thread::sleep(Duration::from_millis(50));
        assert!(!mux.is_usable());
    }

    #[test]
    fn abort_cancels_only_the_waiting_stream() {
        // Batch of 2: a lone request gets no answer, so it is still waiting
        // when its token fires.
        let (addr, _) = start_server(2, None);
        let mux = open_mux(addr);
        let token = AbortToken::new();
        let t = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            t.abort();
        });
        let err = mux.request(get("/slow"), Some(&token)).expect_err("aborted");
        assert!(matches!(err.error, Error::Aborted(_)));
        assert!(mux.is_usable(), "cancelling one stream keeps the connection");
    }
}
