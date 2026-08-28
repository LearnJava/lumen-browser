//! Тесты `v8_idle_message_clipboard`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// Runtime installed against a concrete page URL — the input
/// `window.isSecureContext` is computed from (BUG-399).
fn v8_runtime_with_url(url: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), url, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

// ── requestIdleCallback / cancelIdleCallback tests ─────────────────────────

#[test]
fn request_idle_callback_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof requestIdleCallback === 'function' && typeof window.requestIdleCallback === 'function'"));
}

#[test]
fn cancel_idle_callback_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof cancelIdleCallback === 'function'"));
}

#[test]
fn request_idle_callback_returns_numeric_id() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof requestIdleCallback(function(){}) === 'number'"));
}

#[test]
fn cancel_idle_callback_does_not_throw() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("cancelIdleCallback(999)").unwrap();
}

#[test]
fn request_idle_callback_bad_arg_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var threw = false; \
                 try { requestIdleCallback('notafn'); } catch(e) { threw = e instanceof TypeError; } \
                 threw"));
}

// ── MessageChannel / MessagePort tests ────────────────────────────────────

#[test]
fn message_channel_creates_two_ports() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var ch = new MessageChannel(); \
                 ch.port1 instanceof MessagePort && ch.port2 instanceof MessagePort"));
}

#[test]
fn message_channel_ports_are_distinct() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var ch = new MessageChannel(); ch.port1 !== ch.port2"));
}

#[test]
fn message_port_post_delivers_via_onmessage() {
    let rt = v8_runtime_with_dom(make_doc());
    // onmessage auto-starts port1; postMessage on port2 delivers to port1.
    // Delivery is a real task (HTML §9.2.3, BUG-702/BUG-704), not a
    // microtask, so it only runs after an explicit _lumen_tick_timers().
    rt.eval(
        "var ch = new MessageChannel(); \
                 var received = null; \
                 ch.port1.onmessage = function(e) { received = e.data; }; \
                 ch.port2.postMessage('hello');",
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_tick_timers(); received === 'hello'"));
}

#[test]
fn message_port_post_delivers_object() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ch = new MessageChannel(); \
                 var got = null; \
                 ch.port1.onmessage = function(e) { got = e.data; }; \
                 ch.port2.postMessage({ x: 42 });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_tick_timers(); got !== null && got.x === 42"));
}

#[test]
fn message_port_structured_clone_is_deep_copy() {
    let rt = v8_runtime_with_dom(make_doc());
    // Mutations to the original after postMessage should not affect received copy.
    rt.eval(
        "var ch = new MessageChannel(); \
                 var got = null; \
                 ch.port1.onmessage = function(e) { got = e.data; }; \
                 var orig = { v: 1 }; \
                 ch.port2.postMessage(orig); \
                 orig.v = 99;",
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_tick_timers(); got !== null && got.v === 1"));
}

#[test]
fn message_port_start_drains_queue() {
    let rt = v8_runtime_with_dom(make_doc());
    // Post before onmessage is set → message queued; start() drains it.
    rt.eval(
        "var ch = new MessageChannel(); \
                 var got = null; \
                 ch.port2.postMessage('queued'); \
                 ch.port1.onmessage = function(e) { got = e.data; };",
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_tick_timers(); got === 'queued'"));
}

#[test]
fn message_port_add_event_listener_delivers() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ch = new MessageChannel(); \
                 var got = null; \
                 ch.port1.addEventListener('message', function(e) { got = e.data; }); \
                 ch.port2.postMessage('evt');",
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_tick_timers(); got === 'evt'"));
}

#[test]
fn message_port_close_stops_delivery() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var ch = new MessageChannel(); \
                 var count = 0; \
                 ch.port1.onmessage = function() { count++; }; \
                 ch.port2.postMessage('a'); \
                 ch.port1.close(); \
                 ch.port2.postMessage('b'); \
                 _lumen_tick_timers(); \
                 count === 0"));
}

#[test]
fn message_port_remove_event_listener_stops_delivery() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var ch = new MessageChannel(); \
                 var count = 0; \
                 var fn = function() { count++; }; \
                 ch.port1.addEventListener('message', fn); \
                 ch.port1.removeEventListener('message', fn); \
                 ch.port2.postMessage('x'); \
                 _lumen_tick_timers(); \
                 count === 0"));
}

#[test]
fn message_channel_window_export() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.MessageChannel === MessageChannel"));
}

// ── navigator.clipboard tests ──────────────────────────────────────────────

#[test]
fn navigator_clipboard_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.clipboard === 'object' && navigator.clipboard !== null"));
}

#[test]
fn navigator_clipboard_read_text_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof navigator.clipboard.readText === 'function' && \
                 typeof navigator.clipboard.readText().then === 'function'"));
}

#[test]
fn navigator_clipboard_write_text_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof navigator.clipboard.writeText === 'function' && \
                 typeof navigator.clipboard.writeText('hi').then === 'function'"));
}

#[test]
fn navigator_clipboard_stub_read_resolves_string() {
    let rt = v8_runtime_with_dom(make_doc());
    // Without native binding, readText resolves to empty string. Two-step
    // per the S12b-2 lesson (see message_port_post_delivers_via_onmessage).
    rt.eval(
        "var ok = false; \
                 navigator.clipboard.readText().then(function(v) { ok = typeof v === 'string'; });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "ok"));
}

// ── navigator.permissions tests ───────────────────────────────────────────

#[test]
fn navigator_permissions_query_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof navigator.permissions === 'object' && \
                 typeof navigator.permissions.query === 'function'"));
}

#[test]
fn navigator_permissions_clipboard_granted() {
    let rt = v8_runtime_with_dom(make_doc());
    // Two-step per the S12b-2 lesson (see message_port_post_delivers_via_onmessage).
    rt.eval(
        "var state = null; \
                 navigator.permissions.query({ name: 'clipboard-read' }).then(function(ps) { state = ps.state; });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "state === 'granted'"));
}

#[test]
fn navigator_permissions_camera_denied() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var state = null; \
                 navigator.permissions.query({ name: 'camera' }).then(function(ps) { state = ps.state; });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "state === 'denied'"));
}

#[test]
fn navigator_permissions_bad_descriptor_rejects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var rejected = false; \
                 navigator.permissions.query(null).catch(function(e) { rejected = true; });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "rejected"));
}

// The four below run against the real install (`crates/js/src/permissions.rs`
// over `WEB_API_SHIM`'s own `EventTarget`), which the module's own unit
// tests cannot reach — they stub `EventTarget` because plain V8 has none.

/// BUG-386: an unrecognised name used to come back `granted`, which is
/// what broke feature detection — a page cannot tell "supported" from
/// "never heard of it" if the answer is always yes.
#[test]
fn navigator_permissions_unknown_name_rejects_with_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = 'never settled'; \
                 navigator.permissions.query({ name: 'totally-made-up-permission-xyz' }).then( \
                   function(ps) { out = 'resolved:' + ps.state; }, \
                   function(e) { out = 'rejected:' + (e instanceof TypeError); });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "out === 'rejected:true'"));
}

/// `local-fonts` is what holds `queryLocalFonts()`'s gate shut until OS
/// font enumeration can actually ask the user (BUG-385).
#[test]
fn navigator_permissions_local_fonts_denied() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var state = null; \
                 navigator.permissions.query({ name: 'local-fonts' }).then(function(ps) { state = ps.state; });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "state === 'denied'"));
}

/// The subscription path has to be wired to the shim's real
/// `EventTarget`, not to an inert `onchange` field.
#[test]
fn navigator_permissions_status_is_a_real_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var status = null; \
                 navigator.permissions.query({ name: 'camera' }).then(function(ps) { status = ps; });",
    )
    .unwrap();
    assert!(bool_eval(&rt, "status instanceof EventTarget"));
    assert!(bool_eval(&rt, "status instanceof PermissionStatus"));
    assert!(bool_eval(&rt, "typeof status.addEventListener === 'function'"));
    assert!(bool_eval(&rt, "navigator.permissions instanceof Permissions"));
}

/// A third-party script must not be able to swap the container out and
/// answer for the engine (the BUG-366 class).
#[test]
fn navigator_permissions_is_not_overwritable() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("navigator.permissions = { query: function() { return 'forged'; } };")
        .unwrap();
    assert!(bool_eval(&rt, "navigator.permissions instanceof Permissions"));
}

// ── isSecureContext / crossOriginIsolated tests ────────────────────────────

/// BUG-399: the flag used to be the literal `true` for every page. Each
/// URL below is a distinct clause of Secure Contexts §3.1/§3.2.
#[test]
fn is_secure_context_is_true_on_trustworthy_url() {
    for url in [
        "https://example.com/page",
        "wss://example.com/socket",
        "file:///tmp/page.html",
        "about:blank",
        "about:srcdoc",
        "data:text/html,<p>hi</p>",
        "blob:https://example.com/2a1f-0b",
        "http://localhost:8000/t",
        "http://localhost./t",
        "http://app.localhost/t",
        "http://127.0.0.1:18300/t",
        "http://127.1.2.3/t",
        "http://[::1]:8000/t",
        "http://[0:0:0:0:0:0:0:1]/t",
    ] {
        let rt = v8_runtime_with_url(url);
        assert!(
            bool_eval(&rt, "window.isSecureContext === true"),
            "expected secure context for {url}"
        );
    }
}

#[test]
fn is_secure_context_is_false_on_insecure_origin() {
    for url in [
        "http://example.com/page",
        "http://127notanip.example/t",
        // Not the loopback prefix, and not a loopback IPv6 address.
        "http://128.0.0.1/t",
        "http://[::2]/t",
        "http://[::ffff:127.0.0.1]/t",
        // `localhost` only as a whole label, not as a substring.
        "http://notlocalhost/t",
        "http://localhost.example.com/t",
        "ws://example.com/socket",
        // A blob: URL is only as trustworthy as the origin it carries.
        "blob:http://example.com/2a1f-0b",
        // An `about:` URL other than blank/srcdoc has no creator to
        // inherit from.
        "about:settings",
    ] {
        let rt = v8_runtime_with_url(url);
        assert!(
            bool_eval(&rt, "window.isSecureContext === false"),
            "expected insecure context for {url}"
        );
    }
}

/// No page URL at all (the shape most unit-test runtimes install with)
/// is not a trustworthy origin — the safe direction to be wrong in.
#[test]
fn is_secure_context_is_false_without_page_url() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.isSecureContext === false"));
}

/// WebIDL declares a readonly attribute: page script must not be able
/// to answer for the engine by assigning to it (the BUG-366 class).
#[test]
fn is_secure_context_is_not_writable() {
    let rt = v8_runtime_with_url("http://example.com/page");
    rt.eval("window.isSecureContext = true;").unwrap();
    assert!(bool_eval(&rt, "window.isSecureContext === false"));
    assert!(bool_eval(&rt, "isSecureContext === false"));
}

/// A same-document URL change does not re-create the environment, so
/// it must not move the flag. Since BUG-829 the parsed location parts
/// hold a resolved same-origin URL rather than the raw relative string,
/// so the snapshot is no longer the only thing keeping this true.
#[test]
fn is_secure_context_survives_same_document_navigation() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("history.pushState({}, '', '/other')").unwrap();
    assert!(bool_eval(&rt, "location.href === 'https://example.com/other'"));
    assert!(bool_eval(&rt, "window.isSecureContext === true"));
}

#[test]
fn cross_origin_isolated_is_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.crossOriginIsolated === false"));
}
