//! Тесты WebSocket, bfcache-фильтров и EventSource, вынесенные из `v8_ws_sse`
//! (SPLIT-JS8). Моки провайдеров (`JsWebSocketProvider`, `JsSseProvider`)
//! внедряются через тот же `install_dom`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use super::v8_ws_sse::v8_runtime_with_dom;

// ── WebSocket API ─────────────────────────────────────────────────────────

#[test]
fn window_has_websocket_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.WebSocket === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_constants_defined() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("WebSocket.CONNECTING === 0 && WebSocket.OPEN === 1 && WebSocket.CLOSING === 2 && WebSocket.CLOSED === 3")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// GAP-WSASYNC срез 1: `_lumen_ws_connect` now resolves the handshake on a
/// background thread instead of the calling thread, so a single
/// `_lumen_pump_websockets()` right after the constructor can race it even
/// against a mock provider with no real I/O. Pumps in a loop (bounded, 1 s)
/// until `cond_js` evaluates to `true`, returning whether it did.
fn pump_until(rt: &V8JsRuntime, cond_js: &str) -> bool {
    for _ in 0..200 {
        rt.eval("_lumen_pump_websockets();").unwrap();
        if rt.eval(cond_js).unwrap() == lumen_core::JsValue::Bool(true) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

// Mock WS provider: connect always fails (no server).
struct FailWsProvider;
impl lumen_core::ext::JsWebSocketProvider for FailWsProvider {
    fn connect(&self, _url: &str, _protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        Err(lumen_core::error::Error::Network("test: no server".into()))
    }
}

fn v8_runtime_with_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(FailWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, None, false, None).unwrap();
    rt
}

#[test]
fn websocket_connect_fail_sets_closed_state() {
    let rt = v8_runtime_with_ws(make_doc());
    // GAP-WSASYNC срез 1: connect fails off-thread now — pump until the
    // `error`+synthesized-`close` pair lands and readyState reaches CLOSED.
    rt.eval("var ws = new WebSocket('ws://127.0.0.1:1');").unwrap();
    assert!(pump_until(&rt, "ws.readyState === 3"));
}

#[test]
fn websocket_connect_fail_no_handle() {
    let rt = v8_runtime_with_ws(make_doc());
    // The handle is non-zero immediately (still-connecting registry entry);
    // it only resets to 0 once the terminal close/error event is delivered.
    rt.eval("var ws = new WebSocket('ws://127.0.0.1:1');").unwrap();
    assert!(pump_until(&rt, "ws.readyState === 3"));
    let r = rt.eval("ws._handle === 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_connect_fail_fires_onerror() {
    let rt = v8_runtime_with_ws(make_doc());
    rt.eval(
        "var fired = false;
                 var ws = new WebSocket('ws://127.0.0.1:1');
                 ws.onerror = function() { fired = true; };",
    )
    .unwrap();
    assert!(pump_until(&rt, "ws.readyState === 3"));
    let r = rt.eval("fired").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// GAP-CSPENF срез 11: mock provider that always refuses with
/// `Error::CspConnectSrcBlocked`, the way `HttpClient::connect` (срез 11)
/// does when the document's `connect-src` blocks the handshake — proves the
/// native `_lumen_ws_connect` bridge surfaces the block through the
/// `_lumen_ws_last_csp_block` side channel (mirroring `_lumen_fetch_last_csp_block`,
/// срез 10) rather than swallowing it as a generic connect failure.
struct CspBlockedWsProvider;
impl lumen_core::ext::JsWebSocketProvider for CspBlockedWsProvider {
    fn connect(&self, _url: &str, _protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        Err(lumen_core::error::Error::CspConnectSrcBlocked {
            blocked_uri: "wss://blocked.example/x".into(),
            original_policy: "connect-src 'none'".into(),
        })
    }
}

fn v8_runtime_with_csp_blocked_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(CspBlockedWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, None, false, None).unwrap();
    rt
}

#[test]
fn websocket_connect_src_block_reaches_native_side_channel() {
    let rt = v8_runtime_with_csp_blocked_ws(make_doc());
    // GAP-WSASYNC срез 1: the block is detected on the background connect
    // thread now, so the side channel populates asynchronously — poll it
    // (read-and-clear, so the first non-empty read is the final one).
    rt.eval("_lumen_ws_connect('wss://blocked.example/x', '');").unwrap();
    let mut result = lumen_core::JsValue::Undefined;
    for _ in 0..200 {
        result = rt.eval("_lumen_ws_last_csp_block()").unwrap();
        if matches!(&result, lumen_core::JsValue::Array(arr) if !arr.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    match result {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], lumen_core::JsValue::String("wss://blocked.example/x".into()));
            assert_eq!(arr[1], lumen_core::JsValue::String("connect-src 'none'".into()));
        }
        other => panic!("expected [uri, policy], got {other:?}"),
    }
}

#[test]
fn websocket_connect_src_block_fires_security_policy_violation_event() {
    let rt = v8_runtime_with_csp_blocked_ws(make_doc());
    // GAP-WSASYNC срез 1: the block now surfaces through the normal
    // `error` poll event (which fires the violation itself), not the old
    // synchronous `!h` constructor branch a `_lumen_tick_timers()` used to
    // be enough to drain — pump until the listener has run.
    rt.eval(
        "var seen = null; \
         document.addEventListener('securitypolicyviolation', function(e) { \
             seen = [e.violatedDirective, e.blockedURI, e.originalPolicy].join('|'); \
         }); \
         var ws = new WebSocket('wss://blocked.example/x');",
    )
    .unwrap();
    assert!(pump_until(&rt, "seen !== null"));
    assert_eq!(
        rt.eval("seen").unwrap(),
        lumen_core::JsValue::String(
            "connect-src|wss://blocked.example/x|connect-src 'none'".into()
        )
    );
}

// ── _lumen_bfcache_blocked: bfcache eligibility filters (Ph3 bfcache L1) ──

#[test]
fn bfcache_blocked_false_by_default() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt.eval("_lumen_bfcache_blocked()").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn bfcache_blocked_true_when_websocket_open() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("_ws_instances.push({ readyState: 1 }); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_false_when_websocket_closed() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    // readyState 3 (CLOSED) must not block — only OPEN (1) does.
    let r = rt
        .eval("_ws_instances.push({ readyState: 3 }); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn bfcache_blocked_true_when_eventsource_open() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("_sse_instances.push({ readyState: 1 }); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_true_when_beforeunload_listener_registered() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("window.addEventListener('beforeunload', function() {}); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_true_when_unload_listener_registered() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("window.addEventListener('unload', function() {}); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_true_when_onbeforeunload_property_set() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("window.onbeforeunload = function() {}; _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-988: a live `Worker` runs on its own OS thread that nothing pumps and
// nothing tells "the page went away" — `park_current_page` used to keep the
// whole runtime (Worker included) alive indefinitely instead of dropping it
// like any other ineligible page, so the thread kept ticking its own
// timers/messages long after the page that created it was gone.
#[test]
fn bfcache_blocked_true_when_worker_active() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("new Worker('data:text/javascript,'); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bfcache_blocked_false_after_worker_terminated() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("var w = new Worker('data:text/javascript,'); w.terminate(); _lumen_bfcache_blocked()")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

// BUG-564: `document.fonts.ready` used to be `undefined`, so any script
// awaiting font loading (`document.fonts.ready.then(...)`) threw
// synchronously instead of getting a Promise.
#[test]
fn document_fonts_ready_is_a_thenable_promise() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    let r = rt
        .eval("document.fonts.ready instanceof Promise && typeof document.fonts.ready.then === 'function'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// Mock WS provider: immediately queues Open + one Text message.
struct MockWsProvider;
struct MockWsSession {
    queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsWsEvent>>,
    /// Sub-protocol echoed back to the client (first requested, "" if none).
    protocol: String,
}
impl lumen_core::ext::JsWebSocketSession for MockWsSession {
    fn send_text(&self, _text: &str) -> lumen_core::error::Result<()> { Ok(()) }
    fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> { Ok(()) }
    fn poll(&self) -> Option<lumen_core::ext::JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&self, _code: u16, _reason: &str) -> lumen_core::error::Result<()> { Ok(()) }
    fn protocol(&self) -> String { self.protocol.clone() }
}
impl lumen_core::ext::JsWebSocketProvider for MockWsProvider {
    fn connect(&self, _url: &str, protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        use lumen_core::ext::JsWsEvent;
        let mut q = std::collections::VecDeque::new();
        q.push_back(JsWsEvent::Open);
        q.push_back(JsWsEvent::Message { data: b"hello".to_vec(), is_binary: false });
        // Echo the client's first requested sub-protocol, mirroring a real server.
        let protocol = protocols.first().cloned().unwrap_or_default();
        Ok(Box::new(MockWsSession { queue: std::sync::Mutex::new(q), protocol }))
    }
}

fn v8_runtime_with_mock_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(MockWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, None, false, None).unwrap();
    rt
}

#[test]
fn websocket_mock_connect_open_state() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    // Phase 0: pump (in a loop — GAP-WSASYNC срез 1 resolves the handshake
    // off-thread, so a single pump right after the constructor can race it)
    // to deliver the Open event → readyState = 1.
    rt.eval("var ws = new WebSocket('ws://mock');").unwrap();
    assert!(pump_until(&rt, "ws.readyState === 1"));
}

#[test]
fn websocket_mock_open_fires_onopen() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    rt.eval(
        "var opened = false;
                 var ws = new WebSocket('ws://mock');
                 ws.onopen = function() { opened = true; };",
    )
    .unwrap();
    assert!(pump_until(&rt, "opened === true"));
}

/// `new WebSocket(url, protocols)` forwards the requested sub-protocol; on open,
/// the server-selected protocol is surfaced as `ws.protocol`. The mock echoes the
/// first requested protocol.
#[test]
fn websocket_subprotocol_surfaced_on_open() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    rt.eval("var ws = new WebSocket('ws://mock', ['chat', 'superchat']);").unwrap();
    assert!(pump_until(&rt, "ws.readyState === 1"));
    let r = rt.eval("ws.protocol").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("chat".into()));
}

/// A string `protocols` argument is accepted and surfaced as `ws.protocol`.
#[test]
fn websocket_subprotocol_string_arg() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    rt.eval("var ws = new WebSocket('ws://mock', 'json');").unwrap();
    assert!(pump_until(&rt, "ws.readyState === 1"));
    let r = rt.eval("ws.protocol").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("json".into()));
}

#[test]
fn websocket_mock_message_via_pump() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    // Set handler before pump so onmessage fires when the message is dispatched.
    rt.eval(
        "var received = null;
                 var ws = new WebSocket('ws://mock');
                 ws.onmessage = function(e) { received = e.data; };",
    )
    .unwrap();
    assert!(pump_until(&rt, "received !== null"));
    let r = rt.eval("received").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello".into()));
}

/// `send()` while CONNECTING must throw `InvalidStateError` (WHATWG WebSocket).
#[test]
fn websocket_send_in_connecting_throws() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    // No pump → stays CONNECTING (readyState 0).
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     try { ws.send('x'); 'nothrow'; } catch (e) { e.name; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("InvalidStateError".into()));
}

/// `close()` with an out-of-range code throws `InvalidAccessError`; a valid
/// custom code (3000–4999) transitions the socket to CLOSING (2).
#[test]
fn websocket_close_code_validation() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let bad = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     try { ws.close(1234); 'nothrow'; } catch (e) { e.name; }",
        )
        .unwrap();
    assert_eq!(bad, lumen_core::JsValue::String("InvalidAccessError".into()));
    let ok = rt
        .eval(
            "var ws2 = new WebSocket('ws://mock');
                     ws2.close(3001); ws2.readyState",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Number(2.0));
}

/// `close()` with a reason longer than 123 UTF-8 bytes throws `SyntaxError`.
#[test]
fn websocket_close_reason_too_long_throws() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     var long = 'a'.repeat(124);
                     try { ws.close(1000, long); 'nothrow'; } catch (e) { e.name; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("SyntaxError".into()));
}

/// `send()` in CLOSING/CLOSED discards data but counts it in `bufferedAmount`.
#[test]
fn websocket_buffered_amount_in_closing() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     ws.close();           // CONNECTING → CLOSING
                     ws.send('hello');     // 5 bytes, discarded but counted
                     ws.bufferedAmount",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(5.0));
}

/// Ready-state constants are exposed on instances, not only the constructor.
#[test]
fn websocket_instance_constants() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     ws.CONNECTING === 0 && ws.OPEN === 1 && ws.CLOSING === 2 && ws.CLOSED === 3",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A second `close()` is a no-op (idempotent), readyState stays CLOSING.
#[test]
fn websocket_close_idempotent() {
    let rt = v8_runtime_with_mock_ws(make_doc());
    let r = rt
        .eval(
            "var ws = new WebSocket('ws://mock');
                     ws.close(); ws.close(); ws.readyState",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn websocket_no_provider_connect_returns_zero() {
    // Without ws_provider, _lumen_ws_connect always returns 0.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_lumen_ws_connect('ws://test', '')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

// ── EventSource / Server-Sent Events (HTML Living Standard §9.2) ──────────

/// Mock SSE session feeding a preset event sequence via `poll()`.
struct MockSseSession {
    queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsSseEvent>>,
}
impl lumen_core::ext::JsSseSession for MockSseSession {
    fn poll(&self) -> Option<lumen_core::ext::JsSseEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&mut self) {}
}

/// Mock SSE provider that queues a fixed event sequence on connect.
struct MockSseProvider {
    events: Vec<lumen_core::ext::JsSseEvent>,
}
impl lumen_core::ext::JsSseProvider for MockSseProvider {
    fn connect_sse(
        &self,
        _url: &str,
    ) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsSseSession>> {
        let q: std::collections::VecDeque<_> = self.events.iter().cloned().collect();
        Ok(Box::new(MockSseSession {
            queue: std::sync::Mutex::new(q),
        }))
    }
}

fn v8_runtime_with_mock_sse(
    doc: Arc<Mutex<Document>>,
    events: Vec<lumen_core::ext::JsSseEvent>,
) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsSseProvider> =
        Arc::new(MockSseProvider { events });
    rt.install_dom(doc, "", None, None, Some(provider), None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

#[test]
fn eventsource_constructor_no_provider_stays_connecting_then_closes_async() {
    // Without an sse_provider, _lumen_sse_connect returns 0. Per spec
    // (HTML LS §9.2.2) readyState stays CONNECTING synchronously
    // (BUG-363 pt.7); the queued failure task transitions it to CLOSED
    // and fires 'error'.
    let rt = v8_runtime_with_dom(make_doc());
    let sync = rt
        .eval("var es = new EventSource('https://x/sse'); es.readyState")
        .unwrap();
    assert_eq!(sync, lumen_core::JsValue::Number(0.0));
    let after = rt.eval("_lumen_tick_timers(); es.readyState").unwrap();
    assert_eq!(after, lumen_core::JsValue::Number(2.0));
}

#[test]
fn eventsource_no_provider_connect_returns_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_lumen_sse_connect('https://x/sse')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

/// GAP-CSPENF срез 11: mock provider that always refuses with
/// `Error::CspConnectSrcBlocked`, the way `HttpClient::connect_sse` (срез 11)
/// does when the document's `connect-src` blocks the handshake — same shape
/// as `CspBlockedWsProvider` above, proving `_lumen_sse_connect` surfaces the
/// block through `_lumen_sse_last_csp_block` rather than a generic failure.
struct CspBlockedSseProvider;
impl lumen_core::ext::JsSseProvider for CspBlockedSseProvider {
    fn connect_sse(&self, _url: &str) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsSseSession>> {
        Err(lumen_core::error::Error::CspConnectSrcBlocked {
            blocked_uri: "https://blocked.example/sse".into(),
            original_policy: "connect-src 'none'".into(),
        })
    }
}

fn v8_runtime_with_csp_blocked_sse(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsSseProvider> = Arc::new(CspBlockedSseProvider);
    rt.install_dom(doc, "", None, None, Some(provider), None, None, None, None, None, None, false, None).unwrap();
    rt
}

#[test]
fn eventsource_connect_src_block_reaches_native_side_channel() {
    let rt = v8_runtime_with_csp_blocked_sse(make_doc());
    let r = rt
        .eval("_lumen_sse_connect('https://blocked.example/sse'); _lumen_sse_last_csp_block()")
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], lumen_core::JsValue::String("https://blocked.example/sse".into()));
            assert_eq!(arr[1], lumen_core::JsValue::String("connect-src 'none'".into()));
        }
        other => panic!("expected [uri, policy], got {other:?}"),
    }
}

#[test]
fn eventsource_connect_src_block_fires_security_policy_violation_event() {
    let rt = v8_runtime_with_csp_blocked_sse(make_doc());
    rt.eval(
        "var seen = null; \
         document.addEventListener('securitypolicyviolation', function(e) { \
             seen = [e.violatedDirective, e.blockedURI, e.originalPolicy].join('|'); \
         }); \
         var es = new EventSource('https://blocked.example/sse'); \
         _lumen_tick_timers();",
    )
    .unwrap();
    assert_eq!(
        rt.eval("seen").unwrap(),
        lumen_core::JsValue::String(
            "connect-src|https://blocked.example/sse|connect-src 'none'".into()
        )
    );
}

#[test]
fn eventsource_opens_on_sse_connect() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open]);
    let r = rt
        .eval(
            "var opened = false;
                     var es = new EventSource('https://x/sse');
                     es.onopen = function() { opened = true; };
                     _lumen_pump_sse();
                     [es.readyState, opened]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            // readyState OPEN (1) and onopen fired.
            assert_eq!(arr[0], lumen_core::JsValue::Number(1.0));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_delivers_message() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "message".into(),
                data: "hello world".into(),
                id: Some("42".into()),
            },
        ],
    );
    let r = rt
        .eval(
            "var data = null; var lid = null;
                     var es = new EventSource('https://x/sse');
                     es.onmessage = function(e) { data = e.data; lid = e.lastEventId; };
                     _lumen_pump_sse();
                     [data, lid]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::String("hello world".into()));
            assert_eq!(arr[1], lumen_core::JsValue::String("42".into()));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_delivers_typed_event() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "ping".into(),
                data: "p".into(),
                id: None,
            },
        ],
    );
    // A named event must reach addEventListener('ping', ...), not onmessage.
    let r = rt
        .eval(
            "var got = null; var onmsg = false;
                     var es = new EventSource('https://x/sse');
                     es.onmessage = function() { onmsg = true; };
                     es.addEventListener('ping', function(e) { got = e.data; });
                     _lumen_pump_sse();
                     [got, onmsg]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::String("p".into()));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(false));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_close_sets_closed() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open]);
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     _lumen_pump_sse();
                     es.close();
                     es.readyState",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn eventsource_stream_end_fires_error_with_connecting() {
    use lumen_core::ext::JsSseEvent;
    // The stream ended and the native session is reconnecting: readyState
    // becomes CONNECTING (0) and `error` fires (HTML LS §9.2.5 step 1).
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![JsSseEvent::Open, JsSseEvent::Reconnecting],
    );
    let r = rt
        .eval(
            "var errored = false;
                     var es = new EventSource('https://x/sse');
                     es.onerror = function() { errored = true; };
                     _lumen_pump_sse();
                     [es.readyState, errored]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(0.0)); // CONNECTING
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));  // error fired
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_error_event_fires_onerror() {
    use lumen_core::ext::JsSseEvent;
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![JsSseEvent::Open, JsSseEvent::Error("boom".into())],
    );
    let r = rt
        .eval(
            "var errored = false; var msg = null;
                     var es = new EventSource('https://x/sse');
                     es.onerror = function(e) { errored = true; msg = e.message; };
                     _lumen_pump_sse();
                     [errored, msg, es.readyState]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[1], lumen_core::JsValue::String("boom".into()));
            assert_eq!(arr[2], lumen_core::JsValue::Number(2.0));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_poll_json_escapes_message() {
    use lumen_core::ext::JsSseEvent;
    // Data containing quotes/newlines must round-trip through JSON intact.
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "message".into(),
                data: "line1\nline2 \"quoted\"".into(),
                id: None,
            },
        ],
    );
    let r = rt
        .eval(
            "var data = null;
                     var es = new EventSource('https://x/sse');
                     es.onmessage = function(e) { data = e.data; };
                     _lumen_pump_sse();
                     data",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("line1\nline2 \"quoted\"".into())
    );
}

#[test]
fn eventsource_retry_event_updates_reconnect_delay() {
    use lumen_core::ext::JsSseEvent;
    // A Retry event from the server updates the internal reconnect delay.
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![JsSseEvent::Open, JsSseEvent::Retry(500)],
    );
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     _lumen_pump_sse();
                     es._retryMs",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(500.0));
}

#[test]
fn eventsource_reconnect_fires_open_again() {
    use lumen_core::ext::JsSseEvent;
    // Every announced connection fires `open`, not just the first — that
    // is what the `retry:` WPT tests time (BUG-844).
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Reconnecting,
            JsSseEvent::Open,
        ],
    );
    let r = rt
        .eval(
            "var opens = 0, errors = 0;
                     var es = new EventSource('https://x/sse');
                     es.onopen = function() { opens++; };
                     es.onerror = function() { errors++; };
                     _lumen_pump_sse();
                     [opens, errors, es.readyState]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(2.0)); // two `open`
            assert_eq!(arr[1], lumen_core::JsValue::Number(1.0)); // one `error`
            assert_eq!(arr[2], lumen_core::JsValue::Number(1.0)); // OPEN again
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_close_event_is_terminal() {
    use lumen_core::ext::JsSseEvent;
    // `Close` means the native session stopped for good; it must not
    // schedule a reconnect of its own (the session owns reconnection).
    let rt = v8_runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open, JsSseEvent::Close]);
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     _lumen_pump_sse();
                     [es.readyState, es._handle]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(2.0)); // CLOSED
            assert_eq!(arr[1], lumen_core::JsValue::Number(0.0)); // handle released
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_remove_event_listener() {
    use lumen_core::ext::JsSseEvent;
    // removeEventListener must stop delivery to the removed handler.
    let rt = v8_runtime_with_mock_sse(
        make_doc(),
        vec![
            JsSseEvent::Open,
            JsSseEvent::Message {
                event_type: "ping".into(),
                data: "p".into(),
                id: None,
            },
        ],
    );
    let r = rt
        .eval(
            "var count = 0;
                     var fn1 = function() { count++; };
                     var es = new EventSource('https://x/sse');
                     es.addEventListener('ping', fn1);
                     es.removeEventListener('ping', fn1);
                     _lumen_pump_sse();
                     count",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn eventsource_constants_on_both_interface_and_prototype() {
    // BUG-363 pt.1: constants must be visible via the interface object
    // AND the prototype (so instances see them through the chain too).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     [EventSource.CONNECTING, EventSource.prototype.CONNECTING,
                      es.CONNECTING, es.OPEN, es.CLOSED]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(0.0));
            assert_eq!(arr[1], lumen_core::JsValue::Number(0.0));
            assert_eq!(arr[2], lumen_core::JsValue::Number(0.0));
            assert_eq!(arr[3], lumen_core::JsValue::Number(1.0));
            assert_eq!(arr[4], lumen_core::JsValue::Number(2.0));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_without_new_throws_typeerror() {
    // BUG-363 pt.2: calling the constructor as a plain function must throw.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("try { EventSource('https://x/sse'); 'no throw'; } catch (e) { e instanceof TypeError ? 'TypeError' : String(e); }").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("TypeError".into()));
}

#[test]
fn eventsource_unparsable_url_throws_syntaxerror_domexception() {
    // BUG-363 pt.6: a URL the parser rejects outright (no scheme at all,
    // no document base to resolve against) throws a SyntaxError DOMException.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "try { new EventSource('not a url at all'); 'no throw'; }
                     catch (e) { e instanceof DOMException && e.name === 'SyntaxError' ? 'SyntaxError' : String(e); }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("SyntaxError".into()));
}

#[test]
fn eventsource_extends_event_target_and_dispatches_generically() {
    // BUG-363 pt.3: EventSource must inherit EventTarget so dispatchEvent
    // and the shared listener registry work like any other EventTarget.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     var got = null;
                     es.addEventListener('ping', function(e) { got = e.type; });
                     var ok = (es instanceof EventTarget) && (typeof es.dispatchEvent === 'function');
                     es.dispatchEvent(new Event('ping'));
                     [ok, got]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[1], lumen_core::JsValue::String("ping".into()));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_url_readystate_withcredentials_are_readonly() {
    // BUG-363 pt.4: url/readyState/withCredentials are readonly attributes,
    // not writable own data properties.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     var beforeUrl = es.url, beforeState = es.readyState;
                     es.url = 'zzz'; es.readyState = 99; es.withCredentials = true;
                     [es.url === beforeUrl, es.readyState === beforeState, es.withCredentials === false,
                      es.hasOwnProperty('url'), es.hasOwnProperty('readyState')]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            // Assignment is silently ignored (getter-only, non-strict mode) …
            assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[2], lumen_core::JsValue::Bool(true));
            // … and the accessors live on the prototype, not the instance.
            assert_eq!(arr[3], lumen_core::JsValue::Bool(false));
            assert_eq!(arr[4], lumen_core::JsValue::Bool(false));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_onmessage_is_prototype_accessor_not_own_property() {
    // BUG-363 pt.5: onopen/onmessage/onerror are accessor properties on
    // the prototype, so a fresh instance has no matching own property.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var es = new EventSource('https://x/sse');
                     var ownBefore = es.hasOwnProperty('onmessage');
                     var fn = function() {};
                     es.onmessage = fn;
                     [ownBefore, es.onmessage === fn, es.hasOwnProperty('onmessage')]",
        )
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Bool(false));
            assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
            assert_eq!(arr[2], lumen_core::JsValue::Bool(false));
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_resolves_relative_url_and_url_getter_is_absolute() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, None, None, None, None, None, None, None, false, None,
    )
    .unwrap();
    let r = rt
        .eval("new EventSource('resources/message.py').url")
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "https://example.com/eventsource/resources/message.py".into()
        )
    );
}

#[test]
fn eventsource_empty_url_resolves_to_document_url() {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, None, None, None, None, None, None, None, false, None,
    )
    .unwrap();
    let r = rt.eval("new EventSource('').url").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https://example.com/eventsource/page.html".into())
    );
}

#[test]
fn eventsource_stringifies_null_and_undefined_instead_of_empty() {
    // Side-fix noted in BUG-362: `String(url)` instead of `String(url || '')`.
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, None, None, None, None, None, None, None, false, None,
    )
    .unwrap();
    let r = rt
        .eval("[new EventSource(null).url, new EventSource(undefined).url]")
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(
                arr[0],
                lumen_core::JsValue::String("https://example.com/eventsource/null".into())
            );
            assert_eq!(
                arr[1],
                lumen_core::JsValue::String(
                    "https://example.com/eventsource/undefined".into()
                )
            );
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn eventsource_connects_using_resolved_absolute_url() {
    use lumen_core::ext::JsSseEvent;
    struct RecordingSseProvider {
        seen: Arc<Mutex<Option<String>>>,
    }
    impl lumen_core::ext::JsSseProvider for RecordingSseProvider {
        fn connect_sse(
            &self,
            url: &str,
        ) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsSseSession>> {
            *self.seen.lock().unwrap() = Some(url.to_string());
            Ok(Box::new(MockSseSession {
                queue: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
                    JsSseEvent::Open,
                ])),
            }))
        }
    }
    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsSseProvider> =
        Arc::new(RecordingSseProvider { seen: Arc::clone(&seen) });
    rt.install_dom(
        make_doc(),
        "https://example.com/eventsource/page.html",
        None, None, Some(provider), None, None, None, None, None, None, false, None)
    .unwrap();
    rt.eval("var es = new EventSource('resources/message.py');")
        .unwrap();
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("https://example.com/eventsource/resources/message.py")
    );
}

#[test]
fn close_event_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var ce = new CloseEvent(1001, 'bye', true); ce.code === 1001 && ce.reason === 'bye' && ce.wasClean === true")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn message_event_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var me = new MessageEvent('payload'); me.data === 'payload' && me.type === 'message'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn message_event_user_activation_defaults_to_null() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var me = new MessageEvent('payload'); me.userActivation === null")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn message_event_user_activation_reflects_init() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var ua = { isActive: true }; var me = new MessageEvent('payload', { userActivation: ua }); me.userActivation === ua")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_has_buffered_amount() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.bufferedAmount === 0")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_has_extensions_field() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.extensions === ''")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_binary_type_default_blob() {
    let rt = v8_runtime_with_ws(make_doc());
    let r = rt
        .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.binaryType")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("blob".into()));
}

// Mock provider: queues Open + one binary message (bytes [0x01, 0x02, 0x03]).
struct MockBinaryWsProvider;
struct MockBinaryWsSession {
    queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsWsEvent>>,
}
impl lumen_core::ext::JsWebSocketSession for MockBinaryWsSession {
    fn send_text(&self, _text: &str) -> lumen_core::error::Result<()> { Ok(()) }
    fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> { Ok(()) }
    fn poll(&self) -> Option<lumen_core::ext::JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }
    fn close(&self, _code: u16, _reason: &str) -> lumen_core::error::Result<()> { Ok(()) }
}
impl lumen_core::ext::JsWebSocketProvider for MockBinaryWsProvider {
    fn connect(&self, _url: &str, _protocols: &[String]) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
        use lumen_core::ext::JsWsEvent;
        let mut q = std::collections::VecDeque::new();
        q.push_back(JsWsEvent::Open);
        q.push_back(JsWsEvent::Message { data: vec![0x01, 0x02, 0x03], is_binary: true });
        Ok(Box::new(MockBinaryWsSession { queue: std::sync::Mutex::new(q) }))
    }
}

fn v8_runtime_with_binary_ws(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(MockBinaryWsProvider);
    rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, None, None, false, None).unwrap();
    rt
}

#[test]
fn websocket_binary_blob_mode_delivers_uint8array() {
    let rt = v8_runtime_with_binary_ws(make_doc());
    // Default binaryType='blob' → Uint8Array (our Phase 0 representation).
    rt.eval(
        "var received = null;
                 var ws = new WebSocket('ws://mock');
                 ws.onmessage = function(e) { received = e.data; };",
    )
    .unwrap();
    assert!(pump_until(&rt, "received !== null"));
    let r = rt
        .eval("received instanceof Uint8Array && received[0] === 1 && received[1] === 2 && received[2] === 3")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_binary_arraybuffer_mode_delivers_arraybuffer() {
    let rt = v8_runtime_with_binary_ws(make_doc());
    // binaryType='arraybuffer' → ArrayBuffer.
    rt.eval(
        "var received = null;
                 var ws = new WebSocket('ws://mock');
                 ws.binaryType = 'arraybuffer';
                 ws.onmessage = function(e) { received = e.data; };",
    )
    .unwrap();
    assert!(pump_until(&rt, "received !== null"));
    let r = rt
        .eval("received instanceof ArrayBuffer && new Uint8Array(received)[0] === 1")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn websocket_binary_hex_length_matches_byte_count() {
    let rt = v8_runtime_with_binary_ws(make_doc());
    // 3 bytes → Uint8Array of length 3.
    rt.eval(
        "var len = 0;
                 var ws = new WebSocket('ws://mock');
                 ws.onmessage = function(e) { len = e.data.length; };",
    )
    .unwrap();
    assert!(pump_until(&rt, "len === 3"));
    let r = rt.eval("len === 3").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
