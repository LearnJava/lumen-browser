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
// DOM §3.2 «signal abort»: set the state of the signal and of every dependent
// that is not aborted yet *before* any `abort` event fires, then run the abort
// steps (onabort + listeners) for the signal followed by its dependents. A
// dependent is never itself a source (`_lumen_abort_signal_make_dependent`
// flattens), so one level is the whole graph.
function _lumen_abort_signal_fire(sig, reason) {
    if (sig.aborted) return;
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    var toAbort = [];
    var deps = sig._dependents || [];
    for (var d = 0; d < deps.length; d++) {
        if (deps[d].aborted) continue;
        deps[d].aborted = true;
        deps[d].reason = sig.reason;
        toAbort.push(deps[d]);
    }
    _lumen_abort_signal_run_steps(sig);
    for (var k = 0; k < toAbort.length; k++) _lumen_abort_signal_run_steps(toAbort[k]);
}
// DOM §3.2 «run the abort steps»: fire `abort` at the signal.
function _lumen_abort_signal_run_steps(sig) {
    var evt = { type: 'abort', target: sig };
    if (typeof sig.onabort === 'function') { try { sig.onabort(evt); } catch(e) { _lumen_et_report(e); } }
    var listeners = sig._listeners.slice();
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) { _lumen_et_report(e); }
    }
}
// DOM §3.2 «create a dependent abort signal» — shared by `AbortSignal.any` and
// `TaskSignal.any` (scheduler.rs, BUG-665). An already-aborted source decides
// the result at once (the first one in list order); otherwise the result
// follows the *non-dependent* sources: a dependent source contributes its own
// sources instead, which is what makes events fire in creation order across
// a chain of `any()` calls.
function _lumen_abort_signal_make_dependent(result, signals) {
    var i;
    for (i = 0; i < signals.length; i++) {
        if (signals[i] && signals[i].aborted) {
            result.aborted = true;
            result.reason = signals[i].reason;
            return result;
        }
    }
    result._abortDependent = true;
    result._sources = [];
    function link(src) {
        if (result._sources.indexOf(src) >= 0) return;
        result._sources.push(src);
        (src._dependents || (src._dependents = [])).push(result);
    }
    for (i = 0; i < signals.length; i++) {
        var s = signals[i];
        if (!s) continue;
        if (!s._abortDependent) { link(s); continue; }
        for (var j = 0; j < s._sources.length; j++) link(s._sources[j]);
    }
    return result;
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
// AbortSignal.any(signals) — DOM §3.2.2: the result aborts with the reason of
// the first source that aborts.
AbortSignal.any = function(signals) {
    return _lumen_abort_signal_make_dependent(new AbortSignal(), signals || []);
};
