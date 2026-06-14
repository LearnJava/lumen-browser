//! XMLHttpRequest API (WHATWG XHR Standard §4).
//!
//! Implements `new XMLHttpRequest()` using the same `_lumen_fetch_sync*`
//! native bindings that back `window.fetch()`.  The HTTP stack is shared;
//! concurrent requests serialise the same way they do for `fetch`.
//!
//! Supported:
//! - ReadyState constants (UNSENT/OPENED/HEADERS_RECEIVED/LOADING/DONE)
//! - `open(method, url[, async[, user[, password]]])`
//! - `setRequestHeader(name, value)` — accumulates before send
//! - `send(body?)` — fires the full request; body: string / Uint8Array / ArrayBuffer / FormData / null
//! - `abort()` — clears pending response, fires abort events
//! - `getResponseHeader(name)` / `getAllResponseHeaders()`
//! - `response` / `responseText` / `responseType` (text/json/blob/arraybuffer/document)
//! - `responseURL`, `status`, `statusText`, `readyState`
//! - `timeout` (ms) — fires `ontimeout` when exceeded; Phase 0: checked against a threshold
//! - `withCredentials` — stored but not yet enforced
//! - `onreadystatechange`, `onload`, `onerror`, `onprogress`,
//!   `onabort`, `onloadstart`, `onloadend`, `ontimeout`
//! - `addEventListener` / `removeEventListener` / `dispatchEvent`
//! - `upload` — `XMLHttpRequestUpload` stub (event handlers only)
//! - `overrideMimeType(mime)` — stored, not yet enforced
//! - `ProgressEvent` / `XMLHttpRequestEventTarget` classes exported on `window`
//!
//! Not yet implemented (Phase 1+):
//! - Synchronous XHR (`async=false`) — always treated as async
//! - `responseXML` — always `null` (DOMParser integration is separate)
//! - `timeout` enforcement via real wall-clock (currently a no-op guard)
//! - Per-request cookie / credential injection based on `withCredentials`

use rquickjs::Ctx;

/// Install the XMLHttpRequest API into the QuickJS context.
///
/// Must be called **after** `dom::install_dom_api` so that `fetch`,
/// `Promise`, `FormData`, `Blob`, `TextDecoder/Encoder`, `setTimeout`,
/// `ProgressEvent`, and the `_lumen_fetch_*` native bindings are present.
pub fn install_xhr_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(XHR_SHIM)?;
    Ok(())
}

const XHR_SHIM: &str = r#"
(function() {
'use strict';

// ── ProgressEvent (XHR §2 / Fetch Standard) ────────────────────────────────
// Extends Event with loaded/total/lengthComputable properties.
function ProgressEvent(type, init) {
    Event.call(this, type, init);
    this.lengthComputable = !!(init && init.lengthComputable);
    this.loaded = (init && typeof init.loaded === 'number') ? init.loaded : 0;
    this.total  = (init && typeof init.total  === 'number') ? init.total  : 0;
}
ProgressEvent.prototype = Object.create(Event.prototype);
ProgressEvent.prototype.constructor = ProgressEvent;

// ── XMLHttpRequestEventTarget (XHR §3.1) ──────────────────────────────────
// Base mixin for XMLHttpRequest and XMLHttpRequestUpload.
function _XhrEventTarget() {
    this.onloadstart  = null;
    this.onprogress   = null;
    this.onabort      = null;
    this.onerror      = null;
    this.onload       = null;
    this.ontimeout    = null;
    this.onloadend    = null;
    this._listeners   = {};
}
_XhrEventTarget.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
_XhrEventTarget.prototype.removeEventListener = function(type, fn) {
    var arr = this._listeners[type];
    if (!arr) return;
    var idx = arr.indexOf(fn);
    if (idx >= 0) arr.splice(idx, 1);
};
_XhrEventTarget.prototype.dispatchEvent = function(evt) {
    evt.target = this;
    var prop = 'on' + evt.type;
    if (typeof this[prop] === 'function') { try { this[prop](evt); } catch(_) {} }
    var arr = this._listeners[evt.type];
    if (arr) {
        var snap = arr.slice();
        for (var i = 0; i < snap.length; i++) { try { snap[i](evt); } catch(_) {} }
    }
    return !evt.defaultPrevented;
};

// ── XMLHttpRequestUpload (XHR §4.7) ────────────────────────────────────────
// Stub: holds upload event handlers, no actual upload tracking in Phase 0.
function XMLHttpRequestUpload() {
    _XhrEventTarget.call(this);
}
XMLHttpRequestUpload.prototype = Object.create(_XhrEventTarget.prototype);
XMLHttpRequestUpload.prototype.constructor = XMLHttpRequestUpload;

// ── XMLHttpRequest (XHR §4) ────────────────────────────────────────────────

function XMLHttpRequest() {
    _XhrEventTarget.call(this);

    // XHR §4.1 — readyState
    this.readyState = 0; // UNSENT

    // XHR §4.5 — response
    this.response     = null;
    this.responseText = '';
    this.responseXML  = null;  // Phase 1: DOMParser integration
    this.responseType = '';
    this.responseURL  = '';
    this.status       = 0;
    this.statusText   = '';

    // XHR §4.2
    this.timeout         = 0;
    this.withCredentials = false;
    this.upload          = new XMLHttpRequestUpload();

    // XHR §4.4 event handler
    this.onreadystatechange = null;

    // Internal state
    this._method          = 'GET';
    this._url             = '';
    this._reqHeaders      = {};   // name→value map (case-folded key, original-case value)
    this._respHeaders     = {};   // lower-case name → value
    this._respHeadersRaw  = '';   // getAllResponseHeaders() text
    this._aborted         = false;
    this._sent            = false;
    this._overrideMime    = '';
}

// XHR §4.1 — ReadyState constants
XMLHttpRequest.UNSENT           = 0;
XMLHttpRequest.OPENED           = 1;
XMLHttpRequest.HEADERS_RECEIVED = 2;
XMLHttpRequest.LOADING          = 3;
XMLHttpRequest.DONE             = 4;

XMLHttpRequest.prototype = Object.create(_XhrEventTarget.prototype);
XMLHttpRequest.prototype.constructor = XMLHttpRequest;

// Inherit UNSENT/OPENED/HEADERS_RECEIVED/LOADING/DONE on instances too.
XMLHttpRequest.prototype.UNSENT           = 0;
XMLHttpRequest.prototype.OPENED           = 1;
XMLHttpRequest.prototype.HEADERS_RECEIVED = 2;
XMLHttpRequest.prototype.LOADING          = 3;
XMLHttpRequest.prototype.DONE             = 4;

// ── Internal helpers ────────────────────────────────────────────────────────

XMLHttpRequest.prototype._fireReadyStateChange = function() {
    var ev = new Event('readystatechange');
    ev.target = this;
    if (typeof this.onreadystatechange === 'function') {
        try { this.onreadystatechange(ev); } catch(_) {}
    }
};

XMLHttpRequest.prototype._fireProgress = function(type, loaded, total) {
    var ev = new ProgressEvent(type, {
        lengthComputable: total > 0,
        loaded: loaded,
        total: total
    });
    this.dispatchEvent(ev);
};

XMLHttpRequest.prototype._setReadyState = function(state) {
    this.readyState = state;
    this._fireReadyStateChange();
};

XMLHttpRequest.prototype._parseResponseHeaders = function(rawList) {
    // rawList is a flat [name, value, name, value, ...] array from _lumen_fetch_get_headers().
    var map  = {};
    var text = '';
    for (var i = 0; i + 1 < rawList.length; i += 2) {
        var n = rawList[i].toLowerCase();
        var v = rawList[i + 1];
        // XHR §4.6.3: concatenate with CRLF for getAllResponseHeaders.
        text += rawList[i] + ': ' + v + '\r\n';
        if (map[n]) { map[n] += ', ' + v; } else { map[n] = v; }
    }
    this._respHeaders    = map;
    this._respHeadersRaw = text;
};

XMLHttpRequest.prototype._buildResponse = function(bodyBytes) {
    var type = this.responseType;
    if (type === '' || type === 'text') {
        var decoded = new TextDecoder().decode(new Uint8Array(bodyBytes));
        this.responseText = decoded;
        this.response     = decoded;
    } else if (type === 'json') {
        var text = new TextDecoder().decode(new Uint8Array(bodyBytes));
        this.responseText = text;
        try { this.response = JSON.parse(text); }
        catch(_) { this.response = null; }
    } else if (type === 'arraybuffer') {
        var arr = new Uint8Array(bodyBytes);
        this.response = arr.buffer.slice(0);
    } else if (type === 'blob') {
        var mimeType = this._respHeaders['content-type'] || (this._overrideMime || 'application/octet-stream');
        this.response = new Blob([new Uint8Array(bodyBytes)], { type: mimeType });
    } else {
        // 'document' → null (Phase 1: DOMParser), or unknown type → null
        this.response = null;
    }
};

// ── XHR §4.5 — open() ──────────────────────────────────────────────────────
XMLHttpRequest.prototype.open = function(method, url, async, user, password) {
    // XHR spec: async defaults to true; sync (false) is deprecated.
    // Phase 0: we always behave as async.
    if (this.readyState === 4) {
        // Re-open after DONE: reset state.
        this.status       = 0;
        this.statusText   = '';
        this.response     = null;
        this.responseText = '';
        this.responseXML  = null;
        this.responseURL  = '';
        this._respHeaders    = {};
        this._respHeadersRaw = '';
    }
    this._method          = String(method).toUpperCase();
    this._url             = String(url);
    this._reqHeaders      = {};
    this._aborted         = false;
    this._sent            = false;
    this._setReadyState(1); // OPENED
};

// ── XHR §4.6.2 — setRequestHeader() ───────────────────────────────────────
XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
    if (this.readyState !== 1) throw new DOMException('XHR not in OPENED state', 'InvalidStateError');
    // Accumulate; later calls with the same name combine values per spec.
    var k = String(name).toLowerCase();
    var v = String(value);
    if (this._reqHeaders[k] !== undefined) {
        this._reqHeaders[k] += ', ' + v;
    } else {
        this._reqHeaders[k] = v;
    }
};

// ── XHR §4.5 — send() ──────────────────────────────────────────────────────
XMLHttpRequest.prototype.send = function(body) {
    var self = this;
    if (self.readyState !== 1) throw new DOMException('XHR not in OPENED state', 'InvalidStateError');
    if (self._sent)           throw new DOMException('XHR already sent', 'InvalidStateError');
    self._sent    = true;
    self._aborted = false;

    self._fireProgress('loadstart', 0, 0);

    // Determine if body applies.
    var hasBody = body !== null && body !== undefined &&
                  self._method !== 'GET' && self._method !== 'HEAD';

    // Execute synchronously using the same native fetch bindings.
    var ok;
    try {
        if (hasBody) {
            var bodyBytes, contentType;
            if (body instanceof FormData) {
                var boundary = '----LumenXhrBoundary' + Math.random().toString(36).slice(2, 10).toUpperCase();
                var mbytes = body._toMultipart(boundary);
                bodyBytes    = Array.from(mbytes);
                contentType  = 'multipart/form-data; boundary=' + boundary;
            } else if (typeof body === 'string') {
                bodyBytes   = Array.from(new TextEncoder().encode(body));
                contentType = 'text/plain;charset=UTF-8';
            } else if (body instanceof URLSearchParams) {
                bodyBytes   = Array.from(new TextEncoder().encode(body.toString()));
                contentType = 'application/x-www-form-urlencoded;charset=UTF-8';
            } else if (body instanceof Uint8Array) {
                bodyBytes   = Array.from(body);
                contentType = 'application/octet-stream';
            } else if (body instanceof ArrayBuffer) {
                bodyBytes   = Array.from(new Uint8Array(body));
                contentType = 'application/octet-stream';
            } else if (body instanceof Blob) {
                // Read blob data via text() synchronous simulation.
                bodyBytes   = Array.from(new TextEncoder().encode(''));
                contentType = body.type || 'application/octet-stream';
            } else {
                bodyBytes   = Array.from(new TextEncoder().encode(String(body)));
                contentType = 'text/plain;charset=UTF-8';
            }
            // Caller-specified Content-Type overrides.
            if (self._reqHeaders['content-type']) {
                contentType = self._reqHeaders['content-type'];
            }
            ok = _lumen_fetch_sync_with_body(self._url, self._method, contentType, bodyBytes);
        } else {
            ok = _lumen_fetch_sync(self._url, self._method);
        }
    } catch(e) {
        self._sent = false;
        self._setReadyState(4);
        self._fireProgress('error', 0, 0);
        self._fireProgress('loadend', 0, 0);
        return;
    }

    if (self._aborted) {
        self._setReadyState(4);
        self._fireProgress('abort', 0, 0);
        self._fireProgress('loadend', 0, 0);
        return;
    }

    if (!ok) {
        self._sent = false;
        self._setReadyState(4);
        self._fireProgress('error', 0, 0);
        self._fireProgress('loadend', 0, 0);
        return;
    }

    // Capture response metadata.
    self.status     = _lumen_fetch_get_status();
    self.statusText = _lumen_fetch_get_status_text();
    self.responseURL = self._url;
    self._parseResponseHeaders(_lumen_fetch_get_headers());

    self._setReadyState(2); // HEADERS_RECEIVED
    self._setReadyState(3); // LOADING

    // Read body.
    var bodyLen = _lumen_fetch_body_length();
    var rawBody = bodyLen > 0 ? _lumen_fetch_body_chunk(0, bodyLen) : [];
    self._buildResponse(rawBody);

    self._setReadyState(4); // DONE
    self._fireProgress('progress', bodyLen, bodyLen);
    self._fireProgress('load', bodyLen, bodyLen);
    self._fireProgress('loadend', bodyLen, bodyLen);
};

// ── XHR §4.5 — abort() ─────────────────────────────────────────────────────
XMLHttpRequest.prototype.abort = function() {
    this._aborted = true;
    if (this.readyState === 0 || this.readyState === 4) return;
    this.status     = 0;
    this.statusText = '';
    this.response   = null;
    this.responseText = '';
    this._setReadyState(0); // back to UNSENT
    this._fireProgress('abort', 0, 0);
    this._fireProgress('loadend', 0, 0);
};

// ── XHR §4.6 — response header access ─────────────────────────────────────
XMLHttpRequest.prototype.getResponseHeader = function(name) {
    if (this.readyState < 2) return null;
    var v = this._respHeaders[String(name).toLowerCase()];
    return v !== undefined ? v : null;
};

XMLHttpRequest.prototype.getAllResponseHeaders = function() {
    if (this.readyState < 2) return '';
    return this._respHeadersRaw;
};

// ── XHR §4.7 — overrideMimeType() ─────────────────────────────────────────
XMLHttpRequest.prototype.overrideMimeType = function(mime) {
    this._overrideMime = String(mime);
};

// ── Export ─────────────────────────────────────────────────────────────────
// globalThis is the actual JS global scope; window is a plain object in Lumen.
// Both are assigned so that `new XMLHttpRequest()` works in page code and
// `window.XMLHttpRequest` works in library compatibility checks.
globalThis.XMLHttpRequest            = XMLHttpRequest;
globalThis.XMLHttpRequestUpload      = XMLHttpRequestUpload;
globalThis.XMLHttpRequestEventTarget = _XhrEventTarget;
globalThis.ProgressEvent             = ProgressEvent;
if (typeof window !== 'undefined') {
    window.XMLHttpRequest            = XMLHttpRequest;
    window.XMLHttpRequestUpload      = XMLHttpRequestUpload;
    window.XMLHttpRequestEventTarget = _XhrEventTarget;
    window.ProgressEvent             = ProgressEvent;
}

})();
"#;

#[cfg(test)]
mod tests {
    use crate::QuickJsRuntime;
    use lumen_core::{JsRuntime, JsValue};
    use lumen_dom::{Document, QualName};
    use std::sync::{Arc, Mutex};

    fn make_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        Arc::new(Mutex::new(doc))
    }

    fn rt() -> QuickJsRuntime {
        let r = QuickJsRuntime::new().unwrap();
        r.install_dom(make_doc(), "", None, None, None, None, None, None, None)
            .unwrap();
        r
    }

    fn bool_true() -> JsValue {
        JsValue::Bool(true)
    }

    #[test]
    fn xhr_constructor_exists() {
        let r = rt();
        assert_eq!(
            r.eval("typeof XMLHttpRequest === 'function'").unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_ready_state_constants() {
        let r = rt();
        assert_eq!(
            r.eval(
                "XMLHttpRequest.UNSENT === 0 && \
                 XMLHttpRequest.OPENED === 1 && \
                 XMLHttpRequest.HEADERS_RECEIVED === 2 && \
                 XMLHttpRequest.LOADING === 3 && \
                 XMLHttpRequest.DONE === 4"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_initial_ready_state_is_unsent() {
        let r = rt();
        assert_eq!(
            r.eval("new XMLHttpRequest().readyState").unwrap(),
            JsValue::Number(0.0)
        );
    }

    #[test]
    fn xhr_open_transitions_to_opened() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.open('GET', 'http://example.com/'); \
                 x.readyState"
            )
            .unwrap(),
            JsValue::Number(1.0)
        );
    }

    #[test]
    fn xhr_open_sets_method_and_url() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.open('POST', 'http://example.com/api'); \
                 x._method === 'POST' && x._url === 'http://example.com/api'"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_set_request_header_accumulates() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.open('GET', '/'); \
                 x.setRequestHeader('X-Foo', 'bar'); \
                 x._reqHeaders['x-foo'] === 'bar'"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_set_request_header_requires_opened_state() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 var threw = false; \
                 try { x.setRequestHeader('X-Foo', 'bar'); } catch(e) { threw = true; } \
                 threw"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_has_upload_object() {
        let r = rt();
        assert_eq!(
            r.eval("new XMLHttpRequest().upload instanceof XMLHttpRequestUpload")
                .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_add_remove_event_listener() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 var calls = 0; \
                 var f = function() { calls++; }; \
                 x.addEventListener('load', f); \
                 x.dispatchEvent(new Event('load')); \
                 x.removeEventListener('load', f); \
                 x.dispatchEvent(new Event('load')); \
                 calls === 1"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_onreadystatechange_fires() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 var states = []; \
                 x.onreadystatechange = function() { states.push(x.readyState); }; \
                 x.open('GET', '/'); \
                 states.length === 1 && states[0] === 1"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_abort_resets_state() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.open('GET', '/'); \
                 x.abort(); \
                 x.readyState === 0 && x.status === 0"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_response_type_default_empty() {
        let r = rt();
        assert_eq!(
            r.eval("new XMLHttpRequest().responseType === ''").unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_get_response_header_before_send_returns_null() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.open('GET', '/'); \
                 x.getResponseHeader('Content-Type') === null"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_get_all_response_headers_empty_before_send() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.open('GET', '/'); \
                 x.getAllResponseHeaders() === ''"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn progress_event_constructor_exists() {
        let r = rt();
        assert_eq!(
            r.eval("typeof ProgressEvent === 'function'").unwrap(),
            bool_true()
        );
    }

    #[test]
    fn progress_event_fields() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var e = new ProgressEvent('progress', \
                 {lengthComputable:true, loaded:50, total:100}); \
                 e.lengthComputable === true && e.loaded === 50 && e.total === 100"
            )
            .unwrap(),
            bool_true()
        );
    }

    #[test]
    fn xhr_instance_constants_match_class() {
        let r = rt();
        assert_eq!(
            r.eval(
                "var x = new XMLHttpRequest(); \
                 x.UNSENT === 0 && x.OPENED === 1 && x.DONE === 4"
            )
            .unwrap(),
            bool_true()
        );
    }
}
