
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
    var list = this._listeners[type];
    if (list) {
        var snapshot = list.slice();
        for (var i = 0; i < snapshot.length; i++) {
            var entry = snapshot[i];
            try {
                if (typeof entry.callback === 'function') entry.callback.call(this, event);
                else if (entry.callback && typeof entry.callback.handleEvent === 'function') entry.callback.handleEvent(event);
            } catch (e) { _lumen_et_report(e); }
            if (entry.once) this.removeEventListener(type, entry.callback, entry.capture);
            if (event._stopImmediate) break;
        }
    }
    var onprop = 'on' + type;
    if (typeof this[onprop] === 'function') {
        try { this[onprop].call(this, event); } catch (e) { _lumen_et_report(e); }
    }
    event.currentTarget = null;
    return !event.defaultPrevented;
};
