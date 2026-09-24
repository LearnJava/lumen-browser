// ── WebSocket API (RFC 6455 §§3–7) ─────────────────────────────────────────
// Handshake and recv run on background threads; JS polls via
// _lumen_pump_websockets() — the page from its tick, a worker from its task
// loop (`run_worker_tasks`). `[Exposed=(Window,Worker)]`: this slice is
// spliced verbatim into `worker_exposed_shim` (WORKER-1 срез 3, BUG-1071), so
// it may reference only what a worker scope has — `Event`, `DOMException`,
// `TextEncoder`, `setTimeout`, the `_lumen_ws_*` natives; page-only helpers
// are reached through a `typeof` guard or `_lumen_et_report`.

var _ws_instances = [];

function CloseEvent(code, reason, wasClean, init) {
    Event.call(this, 'close', init);
    this.code = code || 1000;
    this.reason = reason || '';
    this.wasClean = !!wasClean;
}
CloseEvent.prototype = Object.create(Event.prototype);
CloseEvent.prototype.constructor = CloseEvent;

function _lumen_ws_fire(ws, ev) {
    ev.target = ws;
    var prop = 'on' + ev.type;
    if (typeof ws[prop] === 'function') { try { ws[prop](ev); } catch(e) { _lumen_et_report(e); } }
    var arr = ws._listeners[ev.type];
    if (arr) { for (var i = 0; i < arr.length; i++) { try { arr[i](ev); } catch(e) { _lumen_et_report(e); } } }
}

function _lumen_ws_pump_one(ws) {
    if (!ws._handle) return;
    var raw;
    while ((raw = _lumen_ws_poll(ws._handle)) !== null && raw !== undefined) {
        try {
            var ev = JSON.parse(raw);
            if (ev.t === 'open') {
                ws.readyState = 1;
                ws.protocol = ev.protocol || '';
                _lumen_ws_fire(ws, new Event('open', { isTrusted: true }));
            } else if (ev.t === 'msg') {
                if (ws.readyState !== 1) { continue; }
                var msgData;
                if (ev.bin) {
                    // Rust encodes binary payload as hex; decode to typed buffer.
                    var hex = ev.data;
                    var len = hex.length >>> 1;
                    var u8 = new Uint8Array(len);
                    for (var bi = 0; bi < len; bi++) {
                        u8[bi] = parseInt(hex.substr(bi * 2, 2), 16);
                    }
                    msgData = ws.binaryType === 'arraybuffer' ? u8.buffer : u8;
                } else {
                    msgData = ev.data;
                }
                _lumen_ws_fire(ws, new MessageEvent(msgData, { isTrusted: true }));
            } else if (ev.t === 'close') {
                ws.readyState = 3;
                // A received Close frame means the closing handshake completed → wasClean.
                _lumen_ws_fire(ws, new CloseEvent(ev.code, ev.reason, true, { isTrusted: true }));
                ws._handle = 0;
                break;
            } else if (ev.t === 'flushed') {
                // GAP-WSASYNC срез 2 (BUG-869): a queued send() finished
                // writing to the socket — take its bytes back out of
                // bufferedAmount, mirroring the increment in send() below.
                ws.bufferedAmount -= ev.bytes;
                if (ws.bufferedAmount < 0) { ws.bufferedAmount = 0; }
            } else if (ev.t === 'error') {
                // GAP-WSASYNC срез 1: connect() now resolves off-thread, so a
                // `connect-src` refusal or an ordinary handshake failure both
                // surface here instead of the old synchronous `!h` branch in
                // the constructor below — same violation-report side channel,
                // same synthesized `close(1006, '', wasClean=false)` (no real
                // close handshake ever happened, since no connection opened).
                var wsCspAsync = (typeof _lumen_ws_last_csp_block === 'function') ? _lumen_ws_last_csp_block() : null;
                if (typeof _lumen_fire_connect_src_violation === 'function') { _lumen_fire_connect_src_violation(wsCspAsync); }
                var err = new Event('error', { isTrusted: true }); err.message = ev.msg;
                _lumen_ws_fire(ws, err);
                ws.readyState = 3; ws._handle = 0;
                _lumen_ws_fire(ws, new CloseEvent(1006, '', false, { isTrusted: true }));
                break;
            }
        } catch(ignore) {}
    }
}

function _lumen_pump_websockets() {
    for (var i = _ws_instances.length - 1; i >= 0; i--) {
        _lumen_ws_pump_one(_ws_instances[i]);
        if (_ws_instances[i].readyState === 3) { _ws_instances.splice(i, 1); }
    }
}

function WebSocket(url, protocols) {
    this.url = String(url || '');
    this.readyState = 0;
    this.protocol = '';
    this.extensions = '';
    this.binaryType = 'blob';
    this.bufferedAmount = 0;
    this.onopen = null; this.onmessage = null;
    this.onclose = null; this.onerror = null;
    this._handle = 0;
    this._listeners = {};
    var self = this;
    var protoCsv = '';
    if (protocols != null) {
        if (Array.isArray(protocols)) {
            protoCsv = protocols.filter(function(p) { return typeof p === 'string' && p.length > 0; }).join(',');
        } else if (typeof protocols === 'string') {
            protoCsv = protocols;
        }
    }
    var h = _lumen_ws_connect(this.url, protoCsv);
    if (!h) {
        // GAP-WSASYNC срез 1: real connect failures (refused, timed out,
        // `connect-src` blocked, ...) no longer land here — `_lumen_ws_connect`
        // always hands back a handle and resolves the outcome asynchronously
        // through the `error`/`close` poll events above. `!h` now means only
        // "no WebSocket provider installed" (embedder didn't wire one up).
        this.readyState = 3;
        setTimeout(function() {
            var e = new Event('error', { isTrusted: true }); e.message = 'WebSocket connection failed';
            _lumen_ws_fire(self, e);
            _lumen_ws_fire(self, new CloseEvent(1006, '', false, { isTrusted: true }));
        }, 0);
        return;
    }
    this._handle = h;
    _ws_instances.push(this);
    // Phase 0: no persistent event loop — caller must invoke _lumen_pump_websockets()
    // after setting onopen/onmessage to receive queued events.
}
// (BufferSource or Blob) branch of the WebIDL union accepted by send() — the
// only two member types that carry raw bytes; everything else in the union
// (USVString, plus null/undefined/number/plain-object/function reaching JS
// through the untyped shim) coerces via ToString (BUG-862).
function _lumen_ws_is_buffer_source(data) {
    return data instanceof ArrayBuffer || (typeof ArrayBuffer.isView === 'function' && ArrayBuffer.isView(data));
}

// Application-data byte length used for bufferedAmount accounting (WHATWG WebSocket).
function _lumen_ws_bytelen(data) {
    if (typeof data === 'string') {
        return new TextEncoder().encode(data).length;
    }
    if (_lumen_ws_is_buffer_source(data)) {
        return data.byteLength;
    }
    if (typeof Blob !== 'undefined' && data instanceof Blob) {
        return data.size;
    }
    // WebIDL union (BufferSource or Blob or USVString) send() argument:
    // anything that isn't buffer-like or a Blob coerces via ToString to
    // USVString instead of crashing on a missing `byteLength` (BUG-862).
    return new TextEncoder().encode(String(data)).length;
}

WebSocket.prototype.send = function(data) {
    if (this.readyState === 0) {
        throw new DOMException("Failed to execute 'send' on 'WebSocket': Still in CONNECTING state.", 'InvalidStateError');
    }
    var n = _lumen_ws_bytelen(data);
    if (this.readyState === 1) {
        // GAP-WSASYNC срез 2 (BUG-869): the native call only queues the
        // frame now — count it as buffered immediately, the 'flushed' poll
        // event above subtracts it back out once the writer thread actually
        // puts it on the wire.
        this.bufferedAmount += n;
        if (typeof data === 'string') {
            _lumen_ws_send(this._handle, data);
        } else if (_lumen_ws_is_buffer_source(data) || (typeof Blob !== 'undefined' && data instanceof Blob)) {
            _lumen_ws_send_bin(this._handle, data instanceof Uint8Array ? data : new Uint8Array(data));
        } else {
            _lumen_ws_send(this._handle, String(data));
        }
    } else if (this.readyState === 2 || this.readyState === 3) {
        // CLOSING/CLOSED: data is discarded but counted (WHATWG §the-websocket-interface send()).
        this.bufferedAmount += n;
    }
};
WebSocket.prototype.close = function(code, reason) {
    if (code !== undefined && code !== null) {
        if (code !== 1000 && (code < 3000 || code > 4999)) {
            throw new DOMException("Failed to execute 'close' on 'WebSocket': The code must be either 1000, or between 3000 and 4999.", 'InvalidAccessError');
        }
    }
    if (typeof reason === 'string' && new TextEncoder().encode(reason).length > 123) {
        throw new DOMException("Failed to execute 'close' on 'WebSocket': The close reason must not be greater than 123 UTF-8 bytes.", 'SyntaxError');
    }
    if (this.readyState === 2 || this.readyState === 3) {
        return;
    }
    this.readyState = 2;
    _lumen_ws_close(this._handle, typeof code === 'number' ? code : 1000, typeof reason === 'string' ? reason : '');
};
WebSocket.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
WebSocket.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    var idx = this._listeners[type].indexOf(fn);
    if (idx >= 0) this._listeners[type].splice(idx, 1);
};
WebSocket.CONNECTING = 0; WebSocket.OPEN = 1;
WebSocket.CLOSING = 2;    WebSocket.CLOSED = 3;
WebSocket.prototype.CONNECTING = 0; WebSocket.prototype.OPEN = 1;
WebSocket.prototype.CLOSING = 2;    WebSocket.prototype.CLOSED = 3;
