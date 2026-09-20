
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

// postMessage(data) — clone data and enqueue delivery to the entangled port,
// or (BUG-868 GAP-WORKERSCOPE срез 2) hand it to the native bridge when this
// port has been transferred across the page↔worker boundary — see
// `_lumen_port_prepare_transfer` below.
MessagePort.prototype.postMessage = function(message) {
    if (this._closed || this._neutered) return;
    if (this._remoteBound) {
        var clone = structuredClone(message);
        var json;
        try { json = JSON.stringify(clone); } catch (e) { return; }
        if (this._remoteWorkerId !== null && this._remoteWorkerId !== undefined) {
            if (typeof _lumen_port_post_to_worker === 'function') {
                _lumen_port_post_to_worker(this._remoteWorkerId, this._lumenPortId, json);
            }
        } else if (typeof _lumen_port_post_reply === 'function') {
            _lumen_port_post_reply(this._lumenPortId, json);
        }
        return;
    }
    if (!this._other || this._other._closed) return;
    var other = this._other;
    var localClone = structuredClone(message);
    setTimeout(function() {
        if (other._closed) return;
        var evt = { type: 'message', data: localClone, target: other,
                    currentTarget: other, bubbles: false, cancelable: false };
        other._deliverOrQueue(evt);
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

// Internal: deliver now if started, else queue — shared by the local
// same-realm path above and the cross-boundary delivery natives below, so a
// transferred-but-not-yet-`start()`ed port still buffers per HTML §8.3.5.
MessagePort.prototype._deliverOrQueue = function(evt) {
    if (this._closed) return;
    if (this._started) {
        this._deliver(evt);
    } else {
        this._queue.push(evt);
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

// ── MessagePort transfer across the page↔worker boundary (BUG-868,
// GAP-WORKERSCOPE срез 2) ────────────────────────────────────────────────────
// Transferring a port to the other side of a `Worker.postMessage`/worker
// `postMessage` `transfer` list does not move the JS object — it detaches the
// port that was listed (`_neutered`) and rebinds ITS LOCAL PARTNER to talk to
// a freshly created proxy object on the other side instead. Both this
// object's `_lumenPortRegistry` entry and the proxy's are keyed by the same
// globally-allocated id (`_lumen_next_port_id`), so a lookup on either side
// resolves to "the local endpoint of this specific bridge".
var _lumenPortRegistry = {};

// Prepare `transfer` for departure: neuter each MessagePort being sent away
// and bind its local partner (if any) to the bridge id. `bindWorkerId` is the
// destination worker id when called from the page's `Worker.prototype.
// postMessage`; `null`/`undefined` from a worker's own `postMessage` (a
// worker has exactly one parent, so replies need no destination id — see
// `postMessage`'s `_remoteWorkerId` branch above). Returns the ids in
// transfer-list order, for the outgoing envelope's `ports` field.
function _lumen_port_prepare_transfer(transfer, bindWorkerId) {
    var ids = [];
    if (!transfer) return ids;
    for (var i = 0; i < transfer.length; i++) {
        var port = transfer[i];
        if (!(port instanceof MessagePort)) continue;
        var id = port._lumenPortId;
        if (id === undefined) {
            id = (typeof _lumen_next_port_id === 'function') ? _lumen_next_port_id() : 0;
        }
        port._lumenPortId = id;
        var partner = port._other;
        if (partner) {
            partner._lumenPortId   = id;
            partner._remoteBound   = true;
            partner._remoteWorkerId = (bindWorkerId === undefined) ? null : bindWorkerId;
            partner._other = null;
            _lumenPortRegistry[id] = partner;
        }
        port._neutered = true;
        port._other = null;
        ids.push(id);
    }
    return ids;
}

// Find (or, on first sight, create) the local endpoint for bridge id `id` on
// the RECEIVING side of a transfer. First message wins: the object created
// here is what both `ev.ports` and any sentinel embedded in `data` resolve
// to, preserving identity the way structured clone requires.
function _lumen_port_get_or_create(id, remoteWorkerId) {
    var p = _lumenPortRegistry[id];
    if (p) return p;
    p = new MessagePort();
    p._lumenPortId    = id;
    p._remoteBound    = true;
    p._remoteWorkerId = (remoteWorkerId === undefined) ? null : remoteWorkerId;
    _lumenPortRegistry[id] = p;
    return p;
}

// Build `ev.ports` (transfer-list order) for an incoming envelope's
// `ports: [id, ...]` field.
function _lumen_port_reify_list(ids, remoteWorkerId) {
    var out = [];
    for (var i = 0; i < (ids || []).length; i++) {
        out.push(_lumen_port_get_or_create(ids[i], remoteWorkerId));
    }
    return out;
}

// Replace `{__lumen_sentinel__:'__lumen_message_port__', portId}` markers
// found while walking already-parsed message data with the matching
// registered `MessagePort`, so a reference embedded in `data` shares
// identity with the corresponding entry in `ev.ports`.
function _lumen_port_walk_deserialize(obj, remoteWorkerId) {
    if (!obj || typeof obj !== 'object') return obj;
    if (obj.__lumen_sentinel__ === '__lumen_message_port__') {
        return _lumen_port_get_or_create(obj.portId, remoteWorkerId);
    }
    if (Array.isArray(obj)) {
        var arr = [];
        for (var i = 0; i < obj.length; i++) arr.push(_lumen_port_walk_deserialize(obj[i], remoteWorkerId));
        return arr;
    }
    var out = {};
    for (var k in obj) {
        if (Object.prototype.hasOwnProperty.call(obj, k)) {
            out[k] = _lumen_port_walk_deserialize(obj[k], remoteWorkerId);
        }
    }
    return out;
}

// Replace a MessagePort embedded in `data` — already listed in `transfer`,
// already assigned `_lumenPortId` by `_lumen_port_prepare_transfer` — with a
// JSON-serializable sentinel referencing its bridge id. A MessagePort found
// that is NOT in the transfer list has no id and serializes to `null` — this
// codebase's structured-clone layer does not throw `DataCloneError` for an
// untransferred port the way the spec does (a narrower miss than dropping the
// object silently, and out of this срез's scope).
function _lumen_port_walk_serialize(obj) {
    if (!obj || typeof obj !== 'object') return obj;
    if (obj instanceof MessagePort) {
        return (typeof obj._lumenPortId === 'number')
            ? { __lumen_sentinel__: '__lumen_message_port__', portId: obj._lumenPortId }
            : null;
    }
    if (Array.isArray(obj)) {
        var arr = [];
        for (var i = 0; i < obj.length; i++) arr.push(_lumen_port_walk_serialize(obj[i]));
        return arr;
    }
    var out = {};
    for (var k in obj) {
        if (Object.prototype.hasOwnProperty.call(obj, k)) out[k] = _lumen_port_walk_serialize(obj[k]);
    }
    return out;
}
