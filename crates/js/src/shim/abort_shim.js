// ── AbortController / AbortSignal (DOM §3.1–3.2) ──────────────────────────────
// abort() records state and fires listeners; fetch() checks signal.aborted
// before issuing the (synchronous) request. `[Exposed=*]` — this file is also
// spliced into every WorkerGlobalScope (`worker_exposed_shim`, WORKER-1 срез 4),
// which has no page-only `_lumen_report_exception`, so a throwing listener is
// routed through EventTarget's guarded `_lumen_et_report` instead.
function AbortSignal() {
    this.aborted = false;
    this.reason = undefined;
    this.onabort = null;
    this._listeners = [];
}
AbortSignal.prototype.addEventListener = function(type, fn) {
    if (type === 'abort') this._listeners.push(fn);
};
AbortSignal.prototype.removeEventListener = function(type, fn) {
    if (type !== 'abort') return;
    var i = this._listeners.indexOf(fn);
    if (i >= 0) this._listeners.splice(i, 1);
};
AbortSignal.prototype.throwIfAborted = function() {
    if (this.aborted) throw this.reason || new DOMException('signal is aborted without reason', 'AbortError');
};
// Shared signal-abort steps (DOM §3.2): set state, fire onabort + listeners.
function _lumen_abort_signal_fire(sig, reason) {
    if (sig.aborted) return;
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    var evt = { type: 'abort', target: sig };
    if (typeof sig.onabort === 'function') { try { sig.onabort(evt); } catch(e) { _lumen_et_report(e); } }
    var listeners = sig._listeners.slice();
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) { _lumen_et_report(e); }
    }
}

function AbortController() {
    this.signal = new AbortSignal();
}
AbortController.prototype.abort = function(reason) {
    _lumen_abort_signal_fire(this.signal, reason);
};
// AbortSignal.abort(reason) — DOM §3.2.2: returns an already-aborted signal.
AbortSignal.abort = function(reason) {
    var sig = new AbortSignal();
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    return sig;
};
// AbortSignal.timeout(ms) — DOM §3.2.2: aborts with TimeoutError after the
// shell timer queue (setTimeout shim) fires.
AbortSignal.timeout = function(ms) {
    var sig = new AbortSignal();
    // Recorded so fetch() can enforce the deadline natively: the JS thread is
    // parked inside the synchronous native fetch, so this setTimeout can never
    // fire mid-request — the native deadline thread does the in-flight abort.
    sig._timeoutMs = (typeof ms === 'number' && ms > 0) ? ms : 0;
    setTimeout(function() {
        _lumen_abort_signal_fire(sig, new DOMException('signal timed out', 'TimeoutError'));
    }, ms);
    return sig;
};
// AbortSignal.any(signals) — DOM §3.2.2: races the sources; the result aborts
// with the reason of the first source that aborts.
AbortSignal.any = function(signals) {
    var sig = new AbortSignal();
    var sources = [];
    function onAbort(evt) {
        if (sig.aborted) return;
        // Detach from remaining sources — the race is decided.
        for (var j = 0; j < sources.length; j++) {
            sources[j].removeEventListener('abort', onAbort);
        }
        _lumen_abort_signal_fire(sig, evt && evt.target ? evt.target.reason : undefined);
    }
    if (signals) {
        for (var i = 0; i < signals.length; i++) {
            if (!signals[i]) continue;
            if (signals[i].aborted) {
                sig.aborted = true;
                sig.reason = signals[i].reason;
                return sig;
            }
            sources.push(signals[i]);
            signals[i].addEventListener('abort', onAbort);
        }
    }
    return sig;
};

