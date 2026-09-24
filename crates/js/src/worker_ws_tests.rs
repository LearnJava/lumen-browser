//! WORKER-1 срез 3 (BUG-1071): `Event` and `WebSocket` in a worker scope.
//!
//! Kept out of `worker.rs`, which is over the file-size cap
//! (`docs/lint-policy.md` §5.1).

// Хелперы тестового модуля: исключение из clippy.toml покрывает
// только тело `#[test]` (docs/lint-policy.md §10).
#![allow(clippy::unwrap_used)]

use super::*;
use lumen_core::ext::{JsWebSocketProvider, JsWebSocketSession, JsWsEvent};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Opens straight away, pushes one text message, and answers `close()` with
/// the server's Close frame — enough for a full open → message → close cycle.
struct EchoCloseProvider;

struct EchoCloseSession {
    queue: Mutex<VecDeque<JsWsEvent>>,
    sent: Mutex<Vec<String>>,
}

impl JsWebSocketSession for EchoCloseSession {
    fn send_text(&self, text: &str) -> lumen_core::error::Result<()> {
        self.sent.lock().unwrap().push(text.to_string());
        self.queue.lock().unwrap().push_back(JsWsEvent::Message {
            data: format!("echo:{text}").into_bytes(),
            is_binary: false,
        });
        Ok(())
    }
    fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> {
        Ok(())
    }
    fn poll(&self) -> Option<JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&self, code: u16, reason: &str) -> lumen_core::error::Result<()> {
        self.queue
            .lock()
            .unwrap()
            .push_back(JsWsEvent::Close { code: Some(code), reason: reason.to_string() });
        Ok(())
    }
    fn protocol(&self) -> String {
        String::new()
    }
}

impl JsWebSocketProvider for EchoCloseProvider {
    fn connect(
        &self,
        _url: &str,
        _protocols: &[String],
    ) -> lumen_core::error::Result<Box<dyn JsWebSocketSession>> {
        let mut q = VecDeque::new();
        q.push_back(JsWsEvent::Open);
        q.push_back(JsWsEvent::Message { data: b"hello".to_vec(), is_binary: false });
        Ok(Box::new(EchoCloseSession { queue: Mutex::new(q), sent: Mutex::new(Vec::new()) }))
    }
}

/// Collects the worker's replies until one contains `needle` or 3 s pass.
fn wait_for_reply(queue: &WorkerMessageQueue, needle: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        seen.extend(drain_messages(queue).into_iter().map(|(_, json)| json));
        if seen.iter().any(|m| m.contains(needle)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    seen
}

/// A dedicated worker opens a socket through its page's provider and the
/// worker's own task loop delivers every event — nobody posts to the worker,
/// so only the loop's socket polling can wake it (`run_worker_tasks`).
#[test]
fn v8_dedicated_worker_websocket_round_trip() {
    let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
    let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
    let store: WorkerBlobStore = Arc::new(Mutex::new(HashMap::new()));
    let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
    let nid = Arc::new(Mutex::new(0u32));
    let provider: Arc<dyn JsWebSocketProvider> = Arc::new(EchoCloseProvider);
    let script = "var ws = new WebSocket('ws://mock/');\
         ws.onopen = function(e) { postMessage('open:' + (e instanceof Event) + ':' + ws.readyState); ws.send('ping'); };\
         ws.addEventListener('message', function(e) {\
           postMessage('msg:' + e.data);\
           if (e.data === 'echo:ping') ws.close(1000, 'bye');\
         });\
         ws.onclose = function(e) { postMessage('close:' + e.code + ':' + e.reason + ':' + e.wasClean); };"
        .to_string();
    let id = spawn_worker_v8(
        &reg, &queue, &errors, &nid, &store, script, String::new(), false, None,
        &Arc::new(Mutex::new(Vec::new())), &Arc::new(Mutex::new(0u32)), Some(provider), None,
    );
    let seen = wait_for_reply(&queue, "close:");
    terminate_worker(&reg, id);
    let order: Vec<&str> = ["open:true:1", "msg:hello", "msg:echo:ping", "close:1000:bye:true"]
        .into_iter()
        .filter(|want| !seen.iter().any(|m| m.contains(want)))
        .collect();
    assert!(order.is_empty(), "missing {order:?} in {seen:?} (errors: {:?})", drain_errors(&errors));
}

/// A socket opened from a timer callback, not the top-level script, still gets
/// its events: the loop asks whether a socket is live only after the turn's
/// tasks have run — asked before them, the thread slept in `recv()` for good.
#[test]
fn v8_dedicated_worker_websocket_opened_from_a_timer() {
    let queue: WorkerMessageQueue = Arc::new(Mutex::new(Vec::new()));
    let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
    let reg: WorkerRegistry = Arc::new(Mutex::new(HashMap::new()));
    let provider: Arc<dyn JsWebSocketProvider> = Arc::new(EchoCloseProvider);
    let script = "setTimeout(function() {\
           var ws = new WebSocket('ws://mock/');\
           ws.onmessage = function(e) { postMessage('msg:' + e.data); };\
         }, 0);"
        .to_string();
    let id = spawn_worker_v8(
        &reg, &queue, &errors, &Arc::new(Mutex::new(0u32)), &Arc::new(Mutex::new(HashMap::new())),
        script, String::new(), false, None, &Arc::new(Mutex::new(Vec::new())),
        &Arc::new(Mutex::new(0u32)), Some(provider), None,
    );
    let seen = wait_for_reply(&queue, "msg:hello");
    terminate_worker(&reg, id);
    assert!(seen.iter().any(|m| m.contains("msg:hello")), "no message in {seen:?}");
}

/// Without a provider (a service worker, or an embedder that wired none) the
/// class still exists and fails like the page's: `error`, then `close(1006)`.
#[test]
fn v8_worker_websocket_without_provider_fails_like_the_page() {
    let rt = V8JsRuntime::new().unwrap();
    let queue: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let errors: WorkerErrorQueue = Arc::new(Mutex::new(Vec::new()));
    install_worker_globals_v8(
        &rt, 0, queue, errors, Arc::new(Mutex::new(HashMap::new())), None, "", false,
        Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(0u32)), None,
    )
    .unwrap();
    rt.eval(
        "var log = []; var ws = new WebSocket('ws://nowhere/');\
         ws.onerror = function(e) { log.push(e.type); };\
         ws.onclose = function(e) { log.push('close:' + e.code); };",
    )
    .unwrap();
    let _ = run_worker_tasks(&rt);
    assert_eq!(
        rt.eval("typeof WebSocket + '|' + WebSocket.OPEN + '|' + log.join(',')").unwrap(),
        lumen_core::JsValue::String("function|1|error,close:1006".into())
    );
}

/// `Event`/`CustomEvent` are `[Exposed=*]` — a worker scope used to have
/// neither, which is what the `WebSocket` slice's `new Event('open')` needs.
#[test]
fn v8_worker_scope_has_event_and_custom_event() {
    let rt = V8JsRuntime::new().unwrap();
    install_worker_scope_globals_v8(&rt, None).unwrap();
    let r = rt
        .eval(
            "var t = new EventTarget(), got = null;\
             t.addEventListener('x', function(e) { got = e.detail; });\
             var ev = new CustomEvent('x', { detail: 7, cancelable: true });\
             t.dispatchEvent(ev);\
             [typeof Event, ev instanceof Event, got, new Event('y').type, typeof CloseEvent].join('|')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("function|true|7|y|function".into()));
}
