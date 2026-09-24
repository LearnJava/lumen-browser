// ── Event / CustomEvent constructors ─────────────────────────────────────────
// `[Exposed=*]` (DOM §2.2/§2.4): spliced into every WorkerGlobalScope too
// (`worker_exposed_shim`, WORKER-1 срез 3) — a worker had no `Event` at all, so
// the `WebSocket` slice's `new Event('open')` threw there. Nothing at the top
// level here touches page-only state; `composedPath` reaches the DOM only
// through a `typeof`-guarded native.

function Event(type, init) {
    this.type             = String(type || '');
    this.bubbles          = !!(init && init.bubbles);
    this.cancelable       = !!(init && init.cancelable);
    // DOM LS §2.2 EventInit.composed — read out of the init dictionary like the
    // other three flags. Lumen has no composed-tree retargeting yet, so nothing
    // dispatches differently on it, but events that the spec requires to be
    // composed (`fullscreenerror`, …) must still report it (BUG-390).
    this.composed         = !!(init && init.composed);
    this.isTrusted        = !!(init && init.isTrusted);
    this.defaultPrevented = false;
    this.cancelBubble     = false;
    this.target           = null;
    this.currentTarget    = null;
    this.timeStamp        = Date.now ? Date.now() : 0;
    this._stopImmediate   = false;
    // DOM §2.2 — NONE while the event is not being dispatched; set to
    // CAPTURING_PHASE/AT_TARGET/BUBBLING_PHASE by `_lumen_propagate` (BUG-873),
    // which is also what fills `_path` for the duration of one dispatch.
    this.eventPhase       = 0;
    this._path            = null;
}
Event.prototype.preventDefault = function() {
    if (this.cancelable) this.defaultPrevented = true;
};
Event.prototype.stopPropagation = function() { this.cancelBubble = true; };
Event.prototype.stopImmediatePropagation = function() { this._stopImmediate = true; this.cancelBubble = true; };
// DOM §2.2 `composedPath()` — the objects the event is travelling through, in
// target-first order (BUG-577). Empty outside a dispatch, which is what the
// spec says for an event that is not in flight. Shadow-tree retargeting is not
// modelled yet, so a path crossing a shadow boundary lists the real nodes.
Event.prototype.composedPath = function() {
    var p = this._path;
    if (!p || typeof _lumen_path_target !== 'function') return [];
    var out = [];
    for (var i = 0; i < p.length; i++) {
        var o = _lumen_path_target(p[i]);
        if (o) out.push(o);
    }
    return out;
};
// DOM §2.2 the four `eventPhase` constants, on both the interface object and
// its instances — `Event.AT_TARGET` and `e.AT_TARGET` are both live idioms.
Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;
Event.prototype.NONE = 0; Event.prototype.CAPTURING_PHASE = 1;
Event.prototype.AT_TARGET = 2; Event.prototype.BUBBLING_PHASE = 3;
// DOM §2.2 legacy "initialize an event" — used by events minted through
// `document.createEvent()`, which start out with an empty type and must be
// filled in before dispatch. Reinitializes only the four legacy-settable
// fields and, per spec, forces `isTrusted` back to false (this is always a
// script-authored event past this point, however it was constructed).
Event.prototype.initEvent = function(type, bubbles, cancelable) {
    this.type = String(type || '');
    this.bubbles = !!bubbles;
    this.cancelable = !!cancelable;
    this.isTrusted = false;
};

function CustomEvent(type, init) {
    Event.call(this, type, init);
    this.detail = (init && init.detail !== undefined) ? init.detail : null;
}
CustomEvent.prototype = Object.create(Event.prototype);
CustomEvent.prototype.constructor = CustomEvent;
// DOM §2.2 legacy "initialize a CustomEvent" — same vintage as `initEvent`/
// `initUIEvent`/`initMouseEvent` above, still called by real sites (BUG-791:
// dzen.ru's SSO-check bundle uses it, throwing `TypeError: n.initCustomEvent
// is not a function` when it is missing).
CustomEvent.prototype.initCustomEvent = function(type, bubbles, cancelable, detail) {
    this.initEvent(type, bubbles, cancelable);
    this.detail = detail !== undefined ? detail : null;
};

