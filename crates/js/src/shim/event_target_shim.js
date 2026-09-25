
// ── EventTarget base class ────────────────────────────────────────────────────
// WHATWG DOM §2.7 — minimal EventTarget so the many Web API shims that do
// `class X extends EventTarget` (Document PiP, WebHID, WebUSB, Bluetooth,
// WebSerial, WebXR, Navigation API, form-associated custom elements, …) have a
// global base to inherit from. DOM nodes (document, window, elements) keep their
// own native `addEventListener` wired to `_lumen_add_listener`; this class is
// only the constructible base for pure-JS event sources that dispatch to
// themselves. Listeners are stored per type; `dispatchEvent` also invokes the
// matching `on<type>` property handler, mirroring browser behaviour.
function EventTarget() {
    Object.defineProperty(this, '_listeners', { value: Object.create(null), writable: true });
}
// This shim is also spliced into a WorkerGlobalScope (`worker_exposed_shim`),
// which does not carry `_lumen_report_exception` (defined in the page-only
// `WEB_API_SHIM_MID`) — `typeof` on a name absent from scope is safe, a direct
// reference would throw ReferenceError instead.
function _lumen_et_report(e) {
    if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
}

// LONGTASK-1 срез 3: per-callback timing buffer feeding
// `PerformanceLongAnimationFrameTiming.scripts[]` (Long Animation Frames API
// §4, `long_animation_frames.rs`). Every user callback the shim invokes
// (event listener here, timer/rAF callback in `web_api_shim_mid.js`) pushes
// one entry; `V8PersistentJs::deliver_long_animation_frame`
// (`crates/shell/src/persistent_js.rs`) reads and clears this array once per
// rendering opportunity (`relayout()`), independent of whether that frame
// turns out to be long — so a frame's `scripts[]` covers everything that ran
// since the previous frame, matching the spec's "update the rendering"
// framing. Declared here (not in the page-only `web_api_shim_mid.js`)
// because this file is shared with `worker_exposed_shim` (BUG-401) and
// dispatchEvent below is the one invocation site common to both; the buffer
// itself is harmless dead weight in a worker, which has no `relayout()` to
// drain it.
var _lumen_frame_scripts = [];

// Record one user-callback invocation's timing into the current frame's
// script-attribution buffer. `startTime` must come from `performance.now()`
// taken immediately before the call; safe to call from a scope without
// `performance` (falls back to a no-op) so this shim doesn't gain a hard
// dependency on `PERFORMANCE_SHIM`'s install order.
//
// LONGTASK-1 срез 4: optional 4th argument `fn` — the actual callback that
// was invoked — feeds `_lumen_capture_call_site` (native, V8 stack/function
// introspection, `script_attribution.rs`) to fill in `sourceURL`/
// `sourceFunctionName`/`sourceLine`/`sourceColumn` (срез 5 added
// `sourceCharPosition` to the same native call — see that module's docs for
// the `-1`→`0` "unavailable" convention). Best-effort: absent `fn`,
// a non-function `fn` (e.g. an EventListener object's `handleEvent`, or a
// scope with the native missing — the classic-script/module-script parse-time
// path has no single callback function at all, see `scripts.rs`) or an
// exception inside the native all fall back to the class defaults instead of
// throwing.
function _lumen_record_script_timing(startTime, invoker, invokerType, fn) {
    if (typeof performance === 'undefined' || typeof performance.now !== 'function') return;
    var duration = performance.now() - startTime;
    var entry = {
        startTime: startTime,
        duration: duration,
        invoker: String(invoker),
        invokerType: invokerType,
        executionStart: startTime
    };
    if (typeof fn === 'function' && typeof _lumen_capture_call_site === 'function') {
        try {
            var site = _lumen_capture_call_site(fn);
            if (site) {
                entry.sourceURL = site.sourceURL;
                entry.sourceFunctionName = site.sourceFunctionName;
                entry.sourceLine = site.sourceLine;
                entry.sourceColumn = site.sourceColumn;
                entry.sourceCharPosition = site.sourceCharPosition;
            }
        } catch (_) { /* best-effort attribution — never let it break dispatch */ }
    }
    _lumen_frame_scripts.push(entry);
}
EventTarget.prototype.addEventListener = function(type, callback, options) {
    if (!callback) return;
    type = String(type);
    var capture = !!(options === true || (options && options.capture));
    var list = this._listeners[type] || (this._listeners[type] = []);
    for (var i = 0; i < list.length; i++) {
        if (list[i].callback === callback && list[i].capture === capture) return;
    }
    list.push({ callback: callback, capture: capture, once: !!(options && options.once) });
};
EventTarget.prototype.removeEventListener = function(type, callback, options) {
    type = String(type);
    var list = this._listeners[type];
    if (!list) return;
    var capture = !!(options === true || (options && options.capture));
    for (var i = 0; i < list.length; i++) {
        if (list[i].callback === callback && list[i].capture === capture) { list.splice(i, 1); return; }
    }
};
EventTarget.prototype.dispatchEvent = function(event) {
    if (!event || event.type == null) return true;
    var type = String(event.type);
    event.target = event.target || this;
    event.currentTarget = this;
    // LONGTASK-1 срез 3: descriptor for `PerformanceScriptTiming.invoker`,
    // best-effort ("Ctor.type", e.g. "XMLHttpRequest.load") since this base
    // class has no element/tag-name info of its own — DOM-node dispatch goes
    // through the native path instead (`_lumen_propagate` in
    // `web_api_shim_mid.js`), not this one.
    var ctorName = (this && this.constructor && this.constructor.name) || 'EventTarget';
    var invoker = ctorName + '.' + type;
    var list = this._listeners[type];
    if (list) {
        var snapshot = list.slice();
        for (var i = 0; i < snapshot.length; i++) {
            var entry = snapshot[i];
            var _t0 = (typeof performance !== 'undefined' && performance.now) ? performance.now() : 0;
            try {
                if (typeof entry.callback === 'function') entry.callback.call(this, event);
                else if (entry.callback && typeof entry.callback.handleEvent === 'function') entry.callback.handleEvent(event);
            } catch (e) { _lumen_et_report(e); }
            _lumen_record_script_timing(_t0, invoker, 'event-listener', entry.callback);
            if (entry.once) this.removeEventListener(type, entry.callback, entry.capture);
            if (event._stopImmediate) break;
        }
    }
    var onprop = 'on' + type;
    if (typeof this[onprop] === 'function') {
        var _t1 = (typeof performance !== 'undefined' && performance.now) ? performance.now() : 0;
        try { this[onprop].call(this, event); } catch (e) { _lumen_et_report(e); }
        _lumen_record_script_timing(_t1, invoker, 'event-listener', this[onprop]);
    }
    event.currentTarget = null;
    return !event.defaultPrevented;
};
