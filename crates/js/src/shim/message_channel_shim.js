
// ── MessageChannel / MessagePort (WHATWG HTML §8.3.4-§8.3.5) ─────────────────
// MessageChannel() creates two entangled MessagePort objects (port1 / port2).
// Messages posted on one port are delivered asynchronously to the other.
// Setting port.onmessage auto-starts the port (spec §8.3.5 step 4).
//
// Delivery MUST run as a task (HTML §9.2.3, port message queue task source),
// not a microtask: a microtask queue (queueMicrotask/Promise.resolve().then)
// is drained to exhaustion by V8 before control ever returns to Rust's event
// loop (kAuto policy, no manual drain hook here). Callers that keep
// rescheduling work from inside their own onmessage handler — e.g. React's
// Scheduler package, whose whole point in choosing MessageChannel over
// Promise is to get a real macrotask boundary between reschedules — never
// see that boundary and spin forever in one synchronous V8 burst instead
// (BUG-702). setTimeout(fn, 0) below feeds the same _lumen_timers/
// _lumen_tick_timers task queue window.postMessage already uses correctly.
//
// BUG-591: a message handler's exception must be reported to the global error
// handler, not swallowed. This shim is the one part of the page shim the
// *service-worker* scope evaluates too (`sw_worker.rs`), and that scope has
// neither `_lumen_report_exception` (page-only) nor `_lumen_et_report` (its
// EVENT_TARGET_SHIM wrapper) — hence the local typeof-guarded forwarder rather
// than a direct call.
function _lumen_mc_report(e) {
    if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
}

function MessagePort() {
    this._other          = null;
    this._started        = false;
    this._closed         = false;
    this._queue          = [];
    this._listeners      = [];
    this._onmessage      = null;
    this.onmessageerror  = null;
}

// start() — activate queued message delivery (HTML §8.3.5 «start» algorithm).
MessagePort.prototype.start = function() {
    if (this._started || this._closed) return;
    this._started = true;
    var self = this;
    setTimeout(function() { self._drain(); }, 0);
};

// close() — detach the port; further delivery and sends are no-ops.
MessagePort.prototype.close = function() {
    this._closed  = true;
    this._other   = null;
    this._queue   = [];
};

// postMessage(data) — clone data and enqueue delivery to the entangled port.
MessagePort.prototype.postMessage = function(message) {
    if (this._closed || !this._other || this._other._closed) return;
    var other = this._other;
    var clone = structuredClone(message);
    setTimeout(function() {
        if (other._closed) return;
        var evt = { type: 'message', data: clone, target: other,
                    currentTarget: other, bubbles: false, cancelable: false };
        if (other._started) {
            other._deliver(evt);
        } else {
            other._queue.push(evt);
        }
    }, 0);
};

// Internal: deliver evt to onmessage + 'message' addEventListener listeners.
MessagePort.prototype._deliver = function(evt) {
    if (typeof this._onmessage === 'function') {
        try { this._onmessage.call(this, evt); } catch(e) { _lumen_mc_report(e); }
    }
    for (var i = 0; i < this._listeners.length; i++) {
        try { this._listeners[i].call(this, evt); } catch(e) { _lumen_mc_report(e); }
    }
};

// Internal: drain queued messages after start().
MessagePort.prototype._drain = function() {
    var q = this._queue.splice(0);
    for (var i = 0; i < q.length; i++) this._deliver(q[i]);
};

// addEventListener — supports 'message' and 'messageerror'; auto-starts on 'message'.
MessagePort.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (type !== 'message' && type !== 'messageerror') return;
    if (this._listeners.indexOf(fn) < 0) this._listeners.push(fn);
    if (type === 'message') this.start();
};

// removeEventListener — removes a previously registered listener.
MessagePort.prototype.removeEventListener = function(type, fn) {
    var idx = this._listeners.indexOf(fn);
    if (idx >= 0) this._listeners.splice(idx, 1);
};

// dispatchEvent stub — required by some frameworks.
MessagePort.prototype.dispatchEvent = function(evt) {
    this._deliver(evt);
    return true;
};

// onmessage getter/setter — setting to a Function auto-starts delivery.
Object.defineProperty(MessagePort.prototype, 'onmessage', {
    get: function() { return this._onmessage || null; },
    set: function(fn) {
        this._onmessage = (typeof fn === 'function') ? fn : null;
        if (this._onmessage !== null) this.start();
    },
    configurable: true,
    enumerable:   true,
});

// MessageChannel — creates two entangled ports.
function MessageChannel() {
    var p1 = new MessagePort();
    var p2 = new MessagePort();
    p1._other = p2;
    p2._other = p1;
    this.port1 = p1;
    this.port2 = p2;
}

globalThis.MessageChannel = MessageChannel;
globalThis.MessagePort    = MessagePort;
