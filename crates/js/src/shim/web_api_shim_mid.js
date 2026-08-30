
// ── UIEvent / MouseEvent / KeyboardEvent / InputEvent / FocusEvent ────────────
// ── WheelEvent / PointerEvent / AnimationEvent / TransitionEvent / … ─────────
// WHATWG UI Events spec — provides typed event classes for instanceof checks
// and named properties (clientX, key, deltaY, …) that web apps depend on.

function UIEvent(type, init) {
    Event.call(this, type, init);
    this.detail = (init && init.detail != null) ? (init.detail | 0) : 0;
    this.view   = (init && init.view   != null) ? init.view   : null;
}
UIEvent.prototype = Object.create(Event.prototype);
UIEvent.prototype.constructor = UIEvent;

function MouseEvent(type, init) {
    UIEvent.call(this, type, init);
    this.screenX       = (init && init.screenX       != null) ? +init.screenX       : 0;
    this.screenY       = (init && init.screenY       != null) ? +init.screenY       : 0;
    this.clientX       = (init && init.clientX       != null) ? +init.clientX       : 0;
    this.clientY       = (init && init.clientY       != null) ? +init.clientY       : 0;
    this.pageX         = (init && init.pageX         != null) ? +init.pageX         : this.clientX;
    this.pageY         = (init && init.pageY         != null) ? +init.pageY         : this.clientY;
    this.offsetX       = (init && init.offsetX       != null) ? +init.offsetX       : 0;
    this.offsetY       = (init && init.offsetY       != null) ? +init.offsetY       : 0;
    this.movementX     = (init && init.movementX     != null) ? +init.movementX     : 0;
    this.movementY     = (init && init.movementY     != null) ? +init.movementY     : 0;
    this.button        = (init && init.button        != null) ? (init.button  | 0)  : 0;
    this.buttons       = (init && init.buttons       != null) ? (init.buttons | 0)  : 0;
    this.ctrlKey       = !!(init && init.ctrlKey);
    this.shiftKey      = !!(init && init.shiftKey);
    this.altKey        = !!(init && init.altKey);
    this.metaKey       = !!(init && init.metaKey);
    this.relatedTarget = (init && init.relatedTarget != null) ? init.relatedTarget : null;
}
MouseEvent.prototype = Object.create(UIEvent.prototype);
MouseEvent.prototype.constructor = MouseEvent;
MouseEvent.prototype.getModifierState = function(key) {
    if (key === 'Control') return this.ctrlKey;
    if (key === 'Shift')   return this.shiftKey;
    if (key === 'Alt')     return this.altKey;
    if (key === 'Meta')    return this.metaKey;
    return false;
};

function KeyboardEvent(type, init) {
    UIEvent.call(this, type, init);
    this.key         = (init && init.key         != null) ? String(init.key)         : '';
    this.code        = (init && init.code        != null) ? String(init.code)        : '';
    this.keyCode     = (init && init.keyCode     != null) ? (init.keyCode  | 0)      : 0;
    this.charCode    = (init && init.charCode    != null) ? (init.charCode | 0)      : 0;
    this.which       = (init && init.which       != null) ? (init.which    | 0)      : this.keyCode;
    this.location    = (init && init.location    != null) ? (init.location | 0)      : 0;
    this.repeat      = !!(init && init.repeat);
    this.isComposing = !!(init && init.isComposing);
    this.ctrlKey     = !!(init && init.ctrlKey);
    this.shiftKey    = !!(init && init.shiftKey);
    this.altKey      = !!(init && init.altKey);
    this.metaKey     = !!(init && init.metaKey);
}
KeyboardEvent.prototype = Object.create(UIEvent.prototype);
KeyboardEvent.prototype.constructor = KeyboardEvent;
KeyboardEvent.prototype.getModifierState = function(key) {
    if (key === 'Control') return this.ctrlKey;
    if (key === 'Shift')   return this.shiftKey;
    if (key === 'Alt')     return this.altKey;
    if (key === 'Meta')    return this.metaKey;
    return false;
};
KeyboardEvent.DOM_KEY_LOCATION_STANDARD = 0;
KeyboardEvent.DOM_KEY_LOCATION_LEFT     = 1;
KeyboardEvent.DOM_KEY_LOCATION_RIGHT    = 2;
KeyboardEvent.DOM_KEY_LOCATION_NUMPAD   = 3;

function InputEvent(type, init) {
    UIEvent.call(this, type, init);
    this.data         = (init && init.data      != null) ? init.data      : null;
    this.inputType    = (init && init.inputType != null) ? String(init.inputType) : '';
    this.isComposing  = !!(init && init.isComposing);
    this.dataTransfer = (init && init.dataTransfer != null) ? init.dataTransfer : null;
}
InputEvent.prototype = Object.create(UIEvent.prototype);
InputEvent.prototype.constructor = InputEvent;
InputEvent.prototype.getTargetRanges = function() { return []; };

function FocusEvent(type, init) {
    UIEvent.call(this, type, init);
    this.relatedTarget = (init && init.relatedTarget != null) ? init.relatedTarget : null;
}
FocusEvent.prototype = Object.create(UIEvent.prototype);
FocusEvent.prototype.constructor = FocusEvent;

function WheelEvent(type, init) {
    MouseEvent.call(this, type, init);
    this.deltaX    = (init && init.deltaX    != null) ? +init.deltaX    : 0;
    this.deltaY    = (init && init.deltaY    != null) ? +init.deltaY    : 0;
    this.deltaZ    = (init && init.deltaZ    != null) ? +init.deltaZ    : 0;
    this.deltaMode = (init && init.deltaMode != null) ? (init.deltaMode | 0) : 0;
}
WheelEvent.prototype = Object.create(MouseEvent.prototype);
WheelEvent.prototype.constructor = WheelEvent;
WheelEvent.DOM_DELTA_PIXEL = 0;
WheelEvent.DOM_DELTA_LINE  = 1;
WheelEvent.DOM_DELTA_PAGE  = 2;

// Pointer Events Level 2 — pointerId=1 / pointerType='mouse' for mouse input
function PointerEvent(type, init) {
    MouseEvent.call(this, type, init);
    this.pointerId          = (init && init.pointerId        != null) ? (init.pointerId | 0)      : 1;
    this.pointerType        = (init && init.pointerType      != null) ? String(init.pointerType)  : 'mouse';
    this.isPrimary          = (init && init.isPrimary        != null) ? !!init.isPrimary          : true;
    this.width              = (init && init.width            != null) ? +init.width               : 1;
    this.height             = (init && init.height           != null) ? +init.height              : 1;
    this.pressure           = (init && init.pressure         != null) ? +init.pressure            : 0;
    this.tangentialPressure = (init && init.tangentialPressure != null) ? +init.tangentialPressure : 0;
    this.tiltX              = (init && init.tiltX            != null) ? (init.tiltX  | 0)         : 0;
    this.tiltY              = (init && init.tiltY            != null) ? (init.tiltY  | 0)         : 0;
    this.twist              = (init && init.twist            != null) ? (init.twist  | 0)         : 0;
    this.altitudeAngle      = (init && init.altitudeAngle    != null) ? +init.altitudeAngle       : Math.PI / 2;
    this.azimuthAngle       = (init && init.azimuthAngle     != null) ? +init.azimuthAngle        : 0;
}
PointerEvent.prototype = Object.create(MouseEvent.prototype);
PointerEvent.prototype.constructor = PointerEvent;
PointerEvent.prototype.getCoalescedEvents = function() { return []; };
PointerEvent.prototype.getPredictedEvents = function() { return []; };

// AnimationEvent — animationstart / animationend / animationiteration / animationcancel
function AnimationEvent(type, init) {
    Event.call(this, type, init);
    this.animationName = (init && init.animationName != null) ? String(init.animationName) : '';
    this.elapsedTime   = (init && init.elapsedTime   != null) ? +init.elapsedTime   : 0;
    this.pseudoElement = (init && init.pseudoElement != null) ? String(init.pseudoElement) : '';
}
AnimationEvent.prototype = Object.create(Event.prototype);
AnimationEvent.prototype.constructor = AnimationEvent;

// TransitionEvent — transitionstart / transitionend / transitionrun / transitioncancel
function TransitionEvent(type, init) {
    Event.call(this, type, init);
    this.propertyName  = (init && init.propertyName  != null) ? String(init.propertyName)  : '';
    this.elapsedTime   = (init && init.elapsedTime   != null) ? +init.elapsedTime   : 0;
    this.pseudoElement = (init && init.pseudoElement != null) ? String(init.pseudoElement) : '';
}
TransitionEvent.prototype = Object.create(Event.prototype);
TransitionEvent.prototype.constructor = TransitionEvent;

// StorageEvent — fires on localStorage/sessionStorage change in another context
//
// WebIDL coercion matters here and is easy to get wrong in three separate ways
// (BUG-774), so both the constructor and initStorageEvent go through the two
// helpers below instead of assigning arguments as they arrive:
//   * `DOMString? x = null` — an ABSENT or explicitly `undefined` value takes
//     the declared default (`null`), an explicit `null` stays `null`, anything
//     else goes through ToString. So `{key: undefined}` is `null`, not
//     the string "undefined".
//   * `USVString url = ""` — NOT nullable: `undefined` takes the default `""`,
//     but `null` becomes the string "null".
//   * `DOMString type` on both entry points is a REQUIRED argument with no
//     default, so it never defaults — `undefined` becomes "undefined" and
//     `null` becomes "null", and omitting it altogether is a TypeError.
// `_lumen_se_nullable_str`/`_lumen_se_default_str` take the raw `arguments` slot so that
// "absent" and "explicitly undefined" can share one branch, which is exactly
// what WebIDL's default-value rule asks for.
function _lumen_se_nullable_str(v) { return (v === undefined || v === null) ? null : String(v); }
function _lumen_se_default_str(v)  { return (v === undefined) ? '' : String(v); }
// `type` is declared alone so that `StorageEvent.length === 1` (WebIDL counts
// required arguments only); the init dictionary is read out of `arguments`.
function StorageEvent(type) {
    if (!(this instanceof StorageEvent)) {
        throw new TypeError("Failed to construct 'StorageEvent': Please use the 'new' operator.");
    }
    if (arguments.length < 1) {
        throw new TypeError("Failed to construct 'StorageEvent': 1 argument required, but only 0 present.");
    }
    var init = arguments[1];
    Event.call(this, type, init);
    // Event's own base coercion is `String(type || '')`, which collapses both
    // `null` and `undefined` to the empty string — re-do it per WebIDL here
    // rather than changing the base class every other event shares.
    this.type        = String(type);
    this.key         = _lumen_se_nullable_str(init ? init.key      : undefined);
    this.oldValue    = _lumen_se_nullable_str(init ? init.oldValue : undefined);
    this.newValue    = _lumen_se_nullable_str(init ? init.newValue : undefined);
    this.url         = _lumen_se_default_str (init ? init.url      : undefined);
    this.storageArea = (init && init.storageArea !== undefined && init.storageArea !== null)
        ? init.storageArea : null;
}
StorageEvent.prototype = Object.create(Event.prototype);
StorageEvent.prototype.constructor = StorageEvent;
// Same one-declared-argument trick: `initStorageEvent.length` must be 1.
StorageEvent.prototype.initStorageEvent = function(type) {
    if (arguments.length < 1) {
        throw new TypeError("Failed to execute 'initStorageEvent' on 'StorageEvent': 1 argument required, but only 0 present.");
    }
    var a = arguments;
    this.type        = String(type);
    this.bubbles     = !!a[1];
    this.cancelable  = !!a[2];
    this.key         = _lumen_se_nullable_str(a[3]);
    this.oldValue    = _lumen_se_nullable_str(a[4]);
    this.newValue    = _lumen_se_nullable_str(a[5]);
    this.url         = _lumen_se_default_str (a[6]);
    this.storageArea = (a[7] === undefined || a[7] === null) ? null : a[7];
    // DOM LS §2.9 «initialize an event» — the legacy init methods also clear the
    // flags a previous dispatch may have left on the object.
    this.isTrusted        = false;
    this.target           = null;
    this.defaultPrevented = false;
    this.cancelBubble     = false;
    this._stopImmediate   = false;
};

// PopStateEvent — history.pushState / back / forward
function PopStateEvent(type, init) {
    Event.call(this, type, init);
    this.state = (init && init.state !== undefined) ? init.state : null;
}
PopStateEvent.prototype = Object.create(Event.prototype);
PopStateEvent.prototype.constructor = PopStateEvent;

// HashChangeEvent — URL hash (#fragment) changes
function HashChangeEvent(type, init) {
    Event.call(this, type, init);
    this.oldURL = (init && init.oldURL != null) ? String(init.oldURL) : '';
    this.newURL = (init && init.newURL != null) ? String(init.newURL) : '';
}
HashChangeEvent.prototype = Object.create(Event.prototype);
HashChangeEvent.prototype.constructor = HashChangeEvent;

// ToggleEvent — <details> and popover state changes (HTML LS §4.11.1, Popover
// API §3.5). Both used to fire a plain `Event` with `oldState`/`newState` bolted
// on as own properties and no such global existed at all (BUG-578), so
// `Object.getPrototypeOf(evt) === ToggleEvent.prototype` — the assertion
// `toggleEvent.html`'s own `testEvent()` makes of every event it receives —
// could not hold however correct the states were.
// `oldState`/`newState` are WebIDL `DOMString` members with a `''` default and
// readonly attributes, so: a member explicitly set to `undefined` counts as
// absent (default), anything else is stringified — `null` becomes the string
// `'null'`, not `''` — and the result is exposed through a getter with no
// setter, which is what `assert_readonly` looks for. `source` is the Popover
// API Level 2 member (`Element?`, default null); `relatedTarget` must stay
// absent, so nothing here invents one.
function ToggleEvent(type, init) {
    Event.call(this, type, init);
    var oldState = (init == null || init.oldState === undefined) ? '' : String(init.oldState);
    var newState = (init == null || init.newState === undefined) ? '' : String(init.newState);
    var source   = (init == null || init.source   === undefined) ? null : init.source;
    Object.defineProperty(this, 'oldState', { get: function() { return oldState; }, enumerable: true, configurable: true });
    Object.defineProperty(this, 'newState', { get: function() { return newState; }, enumerable: true, configurable: true });
    Object.defineProperty(this, 'source',   { get: function() { return source;   }, enumerable: true, configurable: true });
}
ToggleEvent.prototype = Object.create(Event.prototype);
ToggleEvent.prototype.constructor = ToggleEvent;

// ContentVisibilityAutoStateChangeEvent — CSS Contain L2 §4.1 (BUG-852).
// `skipped` is a readonly WebIDL boolean with a `false` default, so a member
// left out (or set to `undefined`) counts as absent, and anything else goes
// through the ordinary boolean conversion.
function ContentVisibilityAutoStateChangeEvent(type, init) {
    if (!new.target) throw new TypeError("Constructor ContentVisibilityAutoStateChangeEvent requires 'new'");
    Event.call(this, type, init);
    var skipped = !!(init != null && init.skipped);
    Object.defineProperty(this, 'skipped', {
        get: function() { return skipped; }, enumerable: true, configurable: true
    });
}
ContentVisibilityAutoStateChangeEvent.prototype = Object.create(Event.prototype);
ContentVisibilityAutoStateChangeEvent.prototype.constructor = ContentVisibilityAutoStateChangeEvent;

// ErrorEvent — uncaught script errors
function ErrorEvent(type, init) {
    // WebIDL: an interface object is not callable (BUG-813,
    // `workers/Worker_dispatchEvent_ErrorEvent.htm`).
    if (!new.target) throw new TypeError("Constructor ErrorEvent requires 'new'");
    Event.call(this, type, init);
    this.message  = (init && init.message  != null) ? String(init.message)  : '';
    this.filename = (init && init.filename != null) ? String(init.filename) : '';
    this.lineno   = (init && init.lineno   != null) ? (init.lineno  | 0) : 0;
    this.colno    = (init && init.colno    != null) ? (init.colno   | 0) : 0;
    this.error    = (init && init.error    !== undefined) ? init.error : null;
}
ErrorEvent.prototype = Object.create(Event.prototype);
ErrorEvent.prototype.constructor = ErrorEvent;
// `assert_class_string(e, 'ErrorEvent')` reads `Object.prototype.toString`,
// which answers `[object Object]` for a plain constructor without this
// (BUG-813, `workers/Worker_ErrorEvent_type.htm`).
Object.defineProperty(ErrorEvent.prototype, Symbol.toStringTag, {
    value: 'ErrorEvent', configurable: true,
});

// ── Uncaught exception reporting (HTML LS §8.1.3.6 "report the exception") ───
// BUG-591: dispatches a window 'error' ErrorEvent for a genuinely uncaught
// exception from any callback the engine itself invokes -- a top-level classic
// script, a timer, requestAnimationFrame/requestIdleCallback, queueMicrotask,
// a DOM/window event listener, an observer callback (Mutation/Resize/
// Intersection/Performance), a MessagePort or WebSocket handler, and the
// window lifecycle handlers -- the callback-boundary catch(e){} sites this
// replaces used to swallow the error outright. `window.onerror`
// uses the special 5-argument OnErrorEventHandler calling convention (WebIDL)
// rather than receiving the Event object; that distinction is implemented in
// the 'error' branch of window.dispatchEvent (below), which every caller of
// this function funnels through, so page code that already does
// `window.dispatchEvent(new ErrorEvent(...))` gets the same 5-arg behaviour.
//
// `filename`/`lineno`/`colno` are omitted by every JS-side caller (V8's
// `Error` has no structured location API from script -- only `.stack`, a
// free-form string) and best-effort-parsed from the first `at file:line:col`
// frame instead. The one caller with a reliable structured location is the
// Rust host itself: `V8JsRuntime::eval_and_report` (`v8_runtime.rs`) reads
// `v8::Message` (populated by V8 for both compile and runtime errors) and
// passes all three explicitly, which is why they are accepted as optional
// trailing arguments rather than always re-derived here.
function _lumen_parse_error_location(err) {
    if (!err || typeof err.stack !== 'string') return null;
    var lines = err.stack.split('\n');
    for (var i = 0; i < lines.length; i++) {
        var m = /at (?:.*\()?([^\s()]+):(\d+):(\d+)\)?\s*$/.exec(lines[i]);
        if (m) return { filename: m[1], lineno: +m[2], colno: +m[3] };
    }
    return null;
}
function _lumen_report_exception(err, filename, lineno, colno) {
    var message = (err instanceof Error) ? String(err.message) : String(err);
    if (filename === undefined || filename === null) {
        var loc = _lumen_parse_error_location(err);
        filename = loc ? loc.filename : (typeof location !== 'undefined' ? location.href : '');
        lineno = loc ? loc.lineno : 0;
        colno = loc ? loc.colno : 0;
    }
    var ev = new ErrorEvent('error', {
        message: message, filename: String(filename || ''),
        lineno: lineno | 0, colno: colno | 0,
        error: err, bubbles: false, cancelable: true,
    });
    var notCancelled = window.dispatchEvent(ev);
    // Diagnostic value proven during BUG-703/BUG-716: a page whose async
    // bootstrap swallows everything is otherwise silent on stderr right up to
    // the point it hangs.
    if (notCancelled) {
        _lumen_console_error('Uncaught ' + ((err && err.stack) ? err.stack : message));
    }
}

// PromiseRejectionEvent — HTML LS §8.1.7.5, carried by `unhandledrejection` /
// `rejectionhandled`.
//
// BUG-702 added the interface for construction and feature detection; BUG-716
// wired the actual dispatch via V8's isolate-level promise-reject callback
// (`v8_runtime.rs`) calling `_lumen_dispatch_unhandled_rejection` below. Defining
// the interface is not merely cosmetic on its own, though: core-js's
// `promise-constructor-detection` treats a browser without
// `PromiseRejectionEvent` as one whose native Promise cannot be trusted, and
// replaces `globalThis.Promise` with its own polyfill on every site that ships
// core-js. On `tbank.ru/auth/login/` that swap ended in an endless storm of
// polyfill notification microtasks — the page never finished loading. With the
// constructor present, core-js keeps V8's Promise and the same page loads.
function PromiseRejectionEvent(type, init) {
    Event.call(this, type, init);
    this.promise = (init && init.promise !== undefined) ? init.promise : undefined;
    this.reason  = (init && init.reason  !== undefined) ? init.reason  : undefined;
}
PromiseRejectionEvent.prototype = Object.create(Event.prototype);
PromiseRejectionEvent.prototype.constructor = PromiseRejectionEvent;

// SubmitEvent — form submission; carries reference to the submitter button
function SubmitEvent(type, init) {
    Event.call(this, type, init);
    this.submitter = (init && init.submitter != null) ? init.submitter : null;
}
SubmitEvent.prototype = Object.create(Event.prototype);
SubmitEvent.prototype.constructor = SubmitEvent;

// PageTransitionEvent — pageshow / pagehide (bfcache)
function PageTransitionEvent(type, init) {
    Event.call(this, type, init);
    this.persisted = !!(init && init.persisted);
}
PageTransitionEvent.prototype = Object.create(Event.prototype);
PageTransitionEvent.prototype.constructor = PageTransitionEvent;

// BeforeUnloadEvent — fires before navigation away; returnValue triggers dialog
function BeforeUnloadEvent(type, init) {
    Event.call(this, type, init);
    this.returnValue = '';
}
BeforeUnloadEvent.prototype = Object.create(Event.prototype);
BeforeUnloadEvent.prototype.constructor = BeforeUnloadEvent;

// ── HTML5 Drag and Drop API (HTML LS §9.10) ───────────────────────────────────
// DataTransferItem — single item in the drag data store.
function DataTransferItem(kind, type, data) {
    this.kind = kind;   // 'string' or 'file'
    this.type = String(type || '').toLowerCase();
    this._data = data;  // string value or null for file kind
}
DataTransferItem.prototype.getAsString = function(callback) {
    if (this.kind !== 'string' || typeof callback !== 'function') return;
    var d = this._data;
    try { callback(d != null ? String(d) : ''); } catch(e) { _lumen_report_exception(e); }
};
DataTransferItem.prototype.getAsFile = function() {
    return null; // Phase 0: no native file access
};

// DataTransferItemList — ordered list of DataTransferItems.
function DataTransferItemList(owner) {
    this._items = [];
    this._owner = owner; // back-ref to DataTransfer for type sync
}
DataTransferItemList.prototype.add = function(dataOrFile, type) {
    if (typeof dataOrFile === 'string') {
        var t = String(type || 'text/plain').toLowerCase();
        // Spec: only one item per unique type (string kind)
        for (var i = 0; i < this._items.length; i++) {
            if (this._items[i].kind === 'string' && this._items[i].type === t) return null;
        }
        var item = new DataTransferItem('string', t, dataOrFile);
        this._items.push(item);
        this._owner._sync_from_items();
        return item;
    }
    // file kind (Phase 0: no actual File support)
    return null;
};
DataTransferItemList.prototype.remove = function(index) {
    if (index >= 0 && index < this._items.length) {
        this._items.splice(index, 1);
        this._owner._sync_from_items();
    }
};
DataTransferItemList.prototype.clear = function() {
    this._items = [];
    this._owner._sync_from_items();
};
Object.defineProperty(DataTransferItemList.prototype, 'length', {
    get: function() { return this._items.length; }
});
// Indexed access via Proxy-like approach using numeric properties
DataTransferItemList.prototype._rebuild_indices = function() {
    // Clear old numeric properties beyond new length
    var old_n = typeof this._prev_len === 'number' ? this._prev_len : 0;
    var n = this._items.length;
    for (var i = n; i < old_n; i++) delete this[i];
    for (var j = 0; j < n; j++) this[j] = this._items[j];
    this._prev_len = n;
};
DataTransferItemList.prototype[Symbol.iterator] = function() {
    var items = this._items.slice();
    var idx = 0;
    return {
        next: function() {
            if (idx < items.length) return { value: items[idx++], done: false };
            return { value: undefined, done: true };
        }
    };
};

// DataTransfer — the drag data store (HTML LS §9.10.1).
function DataTransfer() {
    this._data = {};         // format → string
    this._types = [];        // read-only types list
    this.effectAllowed = 'uninitialized';
    this.dropEffect = 'none';
    this.items = new DataTransferItemList(this);
    this.files = Object.freeze([]); // FileList stub
}
DataTransfer.prototype._sync_from_items = function() {
    // Rebuild _data and _types from items list; also refresh indexed access on the list
    this._data = {};
    this._types = [];
    var list = this.items._items;
    for (var i = 0; i < list.length; i++) {
        if (list[i].kind === 'string') {
            this._data[list[i].type] = list[i]._data;
            this._types.push(list[i].type);
        }
    }
    this.items._rebuild_indices();
};
Object.defineProperty(DataTransfer.prototype, 'types', {
    get: function() { return Object.freeze(this._types.slice()); }
});
DataTransfer.prototype.setData = function(format, data) {
    var fmt = String(format || '').toLowerCase();
    // Normalise 'text' → 'text/plain', 'url' → 'text/uri-list' per spec
    if (fmt === 'text') fmt = 'text/plain';
    if (fmt === 'url') fmt = 'text/uri-list';
    // Remove existing item with same type, then add new one
    var list = this.items._items;
    for (var i = list.length - 1; i >= 0; i--) {
        if (list[i].kind === 'string' && list[i].type === fmt) list.splice(i, 1);
    }
    list.push(new DataTransferItem('string', fmt, String(data != null ? data : '')));
    this._sync_from_items();
};
DataTransfer.prototype.getData = function(format) {
    var fmt = String(format || '').toLowerCase();
    if (fmt === 'text') fmt = 'text/plain';
    if (fmt === 'url') fmt = 'text/uri-list';
    return Object.prototype.hasOwnProperty.call(this._data, fmt) ? this._data[fmt] : '';
};
DataTransfer.prototype.clearData = function(format) {
    if (arguments.length === 0 || format === undefined || format === null) {
        // Remove all string-kind items
        var list = this.items._items;
        for (var i = list.length - 1; i >= 0; i--) {
            if (list[i].kind === 'string') list.splice(i, 1);
        }
    } else {
        var fmt = String(format).toLowerCase();
        if (fmt === 'text') fmt = 'text/plain';
        if (fmt === 'url') fmt = 'text/uri-list';
        var list2 = this.items._items;
        for (var i = list2.length - 1; i >= 0; i--) {
            if (list2[i].kind === 'string' && list2[i].type === fmt) list2.splice(i, 1);
        }
    }
    this._sync_from_items();
};
DataTransfer.prototype.setDragImage = function(_image, _x, _y) {
    // Phase 0: no-op (custom drag image not supported)
};

// DragEvent — drag-and-drop events (HTML LS §9.10.5)
function DragEvent(type, init) {
    MouseEvent.call(this, type, init);
    // If no DataTransfer provided, create a fresh one for new drag operations
    this.dataTransfer = (init && init.dataTransfer != null)
        ? init.dataTransfer
        : new DataTransfer();
}
DragEvent.prototype = Object.create(MouseEvent.prototype);
DragEvent.prototype.constructor = DragEvent;

// _lumen_dispatch_drag_event — called by Rust shell (Phase 1) to fire a drag event
// on a specific element. data_json is a JSON string of { format: value } pairs.
function _lumen_dispatch_drag_event(nid, type, x, y, data_json) {
    var dt = new DataTransfer();
    if (data_json) {
        try {
            var d = JSON.parse(data_json);
            var keys = Object.keys(d);
            for (var i = 0; i < keys.length; i++) dt.setData(keys[i], d[keys[i]]);
        } catch(e) {}
    }
    var evt = new DragEvent(type, {
        bubbles: true, cancelable: true, isTrusted: true,
        clientX: x || 0, clientY: y || 0,
        dataTransfer: dt
    });
    _lumen_dispatch_rich(nid, evt);
    return !evt.defaultPrevented;
}

// ClipboardEvent — copy / cut / paste
function ClipboardEvent(type, init) {
    Event.call(this, type, init);
    this.clipboardData = (init && init.clipboardData != null) ? init.clipboardData : null;
}
ClipboardEvent.prototype = Object.create(Event.prototype);
ClipboardEvent.prototype.constructor = ClipboardEvent;

// CompositionEvent — IME compositionstart / compositionupdate / compositionend
function CompositionEvent(type, init) {
    UIEvent.call(this, type, init);
    this.data = (init && init.data != null) ? String(init.data) : '';
}
CompositionEvent.prototype = Object.create(UIEvent.prototype);
CompositionEvent.prototype.constructor = CompositionEvent;

// ── Per-element event listener store ─────────────────────────────────────────
// Key: String(nid) + ':' + type  →  Array of handler functions.

var _lumen_listeners = {};

// ── on<type> event handler IDL attributes (BUG-360) ──────────────────────────
// Key: String(nid) + ':' + type (no 'on' prefix) → the current handler
// function, or absent. Backed by a table (keyed by nid) rather than a plain
// expando on the element wrapper so the bubbling dispatch loop can check for
// a handler at each ancestor by nid alone — no need to force
// `_lumen_make_element` on every hop just to ask 'does it have an onclick'.
// Cleared together with `_lumen_listeners` for a nid in `_lumen_gc_collect`,
// same lifetime as the rest of that node's per-nid JS-side state.
var _lumen_on_handlers = {};

// HTML LS §8.1.7.2.1 'the event handler content attribute' algorithm,
// simplified: compile the attribute's text as a function body. An unparsable
// body yields no handler rather than throwing.
function _lumen_compile_inline_handler(body) {
    try { return new Function('event', String(body)); } catch (e) { return null; }
}

// HTML LS §8.1.7.3: on <body>/<frameset>, a handful of event handler IDL
// attributes forward to the Window object instead of storing locally. Only
// `onload` is wired here — it is the one actually evidenced in this bug
// (`<body onload="…">` driving check-layout-th.js and similar WPT harnesses)
// and it is safe to forward unconditionally because the `load` event never
// dispatches through node bubbling (see `_lumen_fire_page_lifecycle`), so
// there is no double-fire risk. The rest of the spec's 'Window-reflecting
// body element event handler set' (onblur/onerror/onfocus/onresize/onscroll)
// is left as an ordinary per-element handler — a known, narrower deviation.
var _LUMEN_BODY_FORWARDED_TO_WINDOW = { onload: 1 };

// Store (or, for a non-function value, clear) the handler backing the
// `on<type>` IDL attribute named `attrName` (e.g. 'onclick') on element `nid`.
function _lumen_set_on_handler(nid, attrName, fn) {
    if (typeof fn !== 'function') fn = null;
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    if ((tag === 'BODY' || tag === 'FRAMESET') && _LUMEN_BODY_FORWARDED_TO_WINDOW[attrName]) {
        window[attrName] = fn;
        return;
    }
    var key = String(nid) + ':' + attrName.slice(2);
    if (fn) _lumen_on_handlers[key] = fn;
    else delete _lumen_on_handlers[key];
}

function _lumen_get_on_handler(nid, attrName) {
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    if ((tag === 'BODY' || tag === 'FRAMESET') && _LUMEN_BODY_FORWARDED_TO_WINDOW[attrName]) {
        return window[attrName] || null;
    }
    var key = String(nid) + ':' + attrName.slice(2);
    return _lumen_on_handlers[key] || null;
}

// Compile an event-handler *content attribute*'s text and install it.
function _lumen_compile_and_set_on_handler(nid, attrName, body) {
    _lumen_set_on_handler(nid, attrName, _lumen_compile_inline_handler(body));
}

// True for any HTML attribute name in the `on*` content-attribute shape
// (`onclick`, `onencrypted`, …) — deliberately generic (not limited to the
// curated `_LUMEN_EVENT_HANDLER_ATTRS` list below) so dispatch honours a
// handler compiled from an attribute name the engine has no dedicated IDL
// accessor for, matching how a content attribute always wins over 'there is
// no such IDL property'.
function _lumen_is_on_attr_name(name) {
    return name.length > 2 && name.charCodeAt(0) === 111 /* 'o' */ && name.charCodeAt(1) === 110 /* 'n' */;
}

// HTML LS §8.1.7.2 — event handler content attributes exposed as `on<type>`
// IDL attributes on every HTML element (the GlobalEventHandlers mixin), plus
// the couple of media-specific ones (onencrypted/onwaitingforkey) the spec's
// table also lists as generic 'on all HTML elements' entries. Defines
// `el.onclick`-style accessors; the underlying dispatch/compile machinery
// above works for *any* on-prefixed attribute name regardless of this list.
var _LUMEN_EVENT_HANDLER_ATTRS = [
    'onabort', 'onauxclick', 'onbeforeinput', 'onbeforematch', 'onbeforetoggle',
    'onblur', 'oncancel', 'oncanplay', 'oncanplaythrough', 'onchange', 'onclick',
    'onclose', 'oncontextlost', 'oncontextmenu', 'oncontextrestored', 'oncopy',
    'oncuechange', 'oncut', 'ondblclick', 'ondrag', 'ondragend', 'ondragenter',
    'ondragleave', 'ondragover', 'ondragstart', 'ondrop', 'ondurationchange',
    // CSS Contain L2 §4.1 — the `content-visibility: auto` state change. The
    // content-attribute half already worked (`_lumen_is_on_attr_name` accepts
    // any `on*`), so only the IDL accessor was missing — which is what
    // `'oncontentvisibilityautostatechange' in el` asks about (BUG-852).
    'oncontentvisibilityautostatechange',
    'onemptied', 'onencrypted', 'onended', 'onerror', 'onfocus', 'onformdata',
    // Fullscreen §4.2 — declared as plain `null` properties on the element
    // wrapper before BUG-390, i.e. an assignment landed on a throwaway wrapper
    // and no dispatch path could ever find it back.
    'onfullscreenchange', 'onfullscreenerror',
    'oninput', 'oninvalid', 'onkeydown', 'onkeypress', 'onkeyup', 'onload',
    'onloadeddata', 'onloadedmetadata', 'onloadstart', 'onmousedown',
    'onmouseenter', 'onmouseleave', 'onmousemove', 'onmouseout', 'onmouseover',
    'onmouseup', 'onmousewheel', 'onpaste', 'onpause', 'onplay', 'onplaying',
    'onprogress', 'onratechange', 'onreset', 'onresize', 'onscroll',
    'onscrollend', 'onsecuritypolicyviolation', 'onseeked', 'onseeking',
    'onselect', 'onslotchange', 'onstalled', 'onsubmit', 'onsuspend',
    'ontimeupdate', 'ontoggle', 'ontouchcancel', 'ontouchend', 'ontouchmove',
    'ontouchstart', 'ontransitioncancel', 'ontransitionend', 'ontransitionrun',
    'ontransitionstart', 'onvolumechange', 'onwaiting', 'onwaitingforkey',
    'onwheel'
];

// Define the `el.on<type>` accessor pair for one curated handler name. Since
// BUG-849 `obj` is the shared `_LUMEN_WRAPPER_ON_MEMBERS` bundle rather than a
// freshly built wrapper, so the pair is created once per NAME instead of once
// per name per node, and the node is read off `this` at call time.
function _lumen_define_on_handler_prop(obj, attrName) {
    Object.defineProperty(obj, attrName, {
        get: function() { return _lumen_get_on_handler(this.__nid__, attrName); },
        set: function(v) { _lumen_set_on_handler(this.__nid__, attrName, v); },
        enumerable: true,
        configurable: true,
    });
}

function _lumen_add_listener(nid, type, fn) {
    if (typeof fn !== 'function') return;
    var key = String(nid) + ':' + String(type);
    if (!_lumen_listeners[key]) _lumen_listeners[key] = [];
    _lumen_listeners[key].push(fn);
}
function _lumen_rm_listener(nid, type, fn) {
    var key = String(nid) + ':' + String(type);
    var arr = _lumen_listeners[key];
    if (!arr) return;
    var idx = arr.indexOf(fn);
    if (idx >= 0) arr.splice(idx, 1);
}
function _lumen_dispatch(nid, event) {
    var key = String(nid) + ':' + event.type;
    var arr = _lumen_listeners[key];
    if (arr && arr.length > 0) {
        var copy = arr.slice(); // snapshot in case a handler mutates the list
        for (var i = 0; i < copy.length; i++) {
            try { copy[i].call(null, event); } catch(e) { _lumen_report_exception(e); }
            if (event._stopImmediate) break;
        }
    }
    // BUG-360: on<type> IDL attribute (el.onclick = fn / onclick="…") fires
    // after explicit listeners, same ordering as EventTarget.prototype.dispatchEvent.
    if (!event._stopImmediate) {
        var onFn = _lumen_get_on_handler(nid, 'on' + event.type);
        if (onFn) { try { onFn.call(_lumen_make_element(nid), event); } catch(e) { _lumen_report_exception(e); } }
    }
    return !event.defaultPrevented;
}

// Sentinel NID used by document.addEventListener to store document-level listeners.
var _LUMEN_DOC_LISTENER_NID = -1;

// Dispatch an event starting at `start_nid` and bubbling up to the document.
// Called from Rust on user input (click, keydown, etc.).
// These events are marked as isTrusted=true because they come through the shell's native event loop.
function _lumen_dispatch_bubble(start_nid, type) {
    var evt = new Event(type, { bubbles: true, cancelable: true, isTrusted: true });
    evt.target = _lumen_make_element(start_nid);
    var cur = start_nid;
    while (cur !== null && cur !== undefined) {
        var key = String(cur) + ':' + String(type);
        var arr = _lumen_listeners[key];
        var onFn = _lumen_get_on_handler(cur, 'on' + type);
        if (arr || onFn) {
            var el = _lumen_make_element(cur);
            if (arr) {
                var copy = arr.slice();
                for (var i = 0; i < copy.length; i++) {
                    if (evt.cancelBubble) break;
                    try { copy[i].call(el, evt); } catch(e) { _lumen_report_exception(e); }
                    if (evt._stopImmediate) break;
                }
            }
            // BUG-360: on<type> fires after explicit listeners at this target.
            if (onFn && !evt.cancelBubble && !evt._stopImmediate) {
                try { onFn.call(el, evt); } catch(e) { _lumen_report_exception(e); }
            }
        }
        if (evt.cancelBubble) break;
        var pid = _lumen_u2n(_lumen_get_parent(cur));
        cur = (pid !== null && pid !== undefined) ? pid : null;
    }
    if (!evt.cancelBubble) {
        var dkey = String(_LUMEN_DOC_LISTENER_NID) + ':' + String(type);
        var darr = _lumen_listeners[dkey];
        if (darr) {
            var dcopy = darr.slice();
            for (var i = 0; i < dcopy.length; i++) {
                if (evt.cancelBubble) break;
                try { dcopy[i].call(document, evt); } catch(e) { _lumen_report_exception(e); }
                if (evt._stopImmediate) break;
            }
        }
    }
    return !evt.defaultPrevented;
}

// Bubble a pre-constructed event object (with target already set) through the DOM.
// Used by _lumen_dispatch_mouse_event and _lumen_dispatch_key_event so they can
// pass rich typed events instead of plain Event instances.
function _lumen_dispatch_rich(start_nid, event) {
    event.target = _lumen_make_element(start_nid);
    var cur = start_nid;
    while (cur !== null && cur !== undefined) {
        var key = String(cur) + ':' + event.type;
        var arr = _lumen_listeners[key];
        var onFn = _lumen_get_on_handler(cur, 'on' + event.type);
        if (arr || onFn) {
            var el = _lumen_make_element(cur);
            if (arr) {
                var copy = arr.slice();
                for (var i = 0; i < copy.length; i++) {
                    if (event.cancelBubble) break;
                    try { copy[i].call(el, event); } catch(e) { _lumen_report_exception(e); }
                    if (event._stopImmediate) break;
                }
            }
            // BUG-360: on<type> fires after explicit listeners at this target.
            if (onFn && !event.cancelBubble && !event._stopImmediate) {
                try { onFn.call(el, event); } catch(e) { _lumen_report_exception(e); }
            }
        }
        if (event.cancelBubble || !event.bubbles) break;
        var pid = _lumen_u2n(_lumen_get_parent(cur));
        cur = (pid !== null && pid !== undefined) ? pid : null;
    }
    if (!event.cancelBubble) {
        var dkey = String(_LUMEN_DOC_LISTENER_NID) + ':' + event.type;
        var darr = _lumen_listeners[dkey];
        if (darr) {
            var dcopy = darr.slice();
            for (var i = 0; i < dcopy.length; i++) {
                if (event.cancelBubble) break;
                try { dcopy[i].call(document, event); } catch(e) { _lumen_report_exception(e); }
                if (event._stopImmediate) break;
            }
        }
    }
    return !event.defaultPrevented;
}

// Called from shell with actual viewport coordinates and modifier state.
// Creates a trusted MouseEvent and dispatches it through the DOM.
// mod: bit-mask — bit0=ctrl, bit1=shift, bit2=alt, bit3=meta
function _lumen_dispatch_mouse_event(start_nid, type, clientX, clientY, button, buttons, mod) {
    var ev = new MouseEvent(type, {
        bubbles: true, cancelable: true, isTrusted: true,
        clientX: clientX, clientY: clientY,
        screenX: clientX, screenY: clientY,
        pageX:   clientX, pageY:   clientY,
        button: button, buttons: buttons,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8)
    });
    return _lumen_dispatch_rich(start_nid, ev);
}

// Called from the shell right before it runs the form submission algorithm for
// an activated submit button (HTML LS 4.10.21.4 step 11: fire a cancelable
// event named 'submit' at the form). Returns false when a page handler called
// preventDefault(), in which case the shell must not navigate — that is how an
// SPA takes submission over (BUG-437: the shell used to submit natively without
// ever telling JS, so the page's own submit handler never ran).
// submitter_nid < 0 means 'no submitter' (form.requestSubmit() with no button).
function _lumen_dispatch_submit_event(form_nid, submitter_nid) {
    var submitter = (submitter_nid === null || submitter_nid === undefined || submitter_nid < 0)
        ? null : _lumen_make_element(submitter_nid);
    var ev = new SubmitEvent('submit', {
        bubbles: true, cancelable: true, isTrusted: true, submitter: submitter
    });
    return _lumen_dispatch_rich(form_nid, ev);
}

// Called from shell when pointer is locked and DeviceEvent::MouseMotion fires.
// Dispatches mousemove + pointermove with movementX/Y reflecting raw OS delta.
// (W3C Pointer Lock L2 §6.3 — clientX/Y reflect last position; movement deltas are raw.)
function _lumen_dispatch_locked_mousemove(nid, clientX, clientY, dx, dy, mod) {
    var mev = new MouseEvent('mousemove', {
        bubbles: true, cancelable: true, isTrusted: true,
        clientX: clientX, clientY: clientY,
        screenX: clientX, screenY: clientY,
        pageX:   clientX, pageY:   clientY,
        movementX: dx, movementY: dy,
        button: 0, buttons: 0,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8)
    });
    _lumen_dispatch_rich(nid, mev);
    var pev = new PointerEvent('pointermove', {
        bubbles: true, cancelable: true, isTrusted: true,
        clientX: clientX, clientY: clientY,
        screenX: clientX, screenY: clientY,
        pageX:   clientX, pageY:   clientY,
        movementX: dx, movementY: dy,
        button: 0, buttons: 0,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8),
        pointerId: 1, pointerType: 'mouse', isPrimary: true,
        pressure: 0.0, width: 1, height: 1,
        altitudeAngle: Math.PI / 2, azimuthAngle: 0,
        tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
    });
    pev.getCoalescedEvents = function() { return [pev]; };
    pev.getPredictedEvents  = function() { return []; };
    _lumen_dispatch_rich(nid, pev);
}

// Build a non-dispatched PointerEvent representing one buffered intermediate
// sample for _lumen_dispatch_pointer_event's getCoalescedEvents()/
// getPredictedEvents() arrays (Pointer Events L3 §4.1). Mirrors the main
// event's fields except position.
function _lumen_make_coalesced_pointer_event(type, cx, cy, button, buttons, mod, bubbles) {
    var cev = new PointerEvent(type, {
        bubbles: bubbles, cancelable: bubbles, isTrusted: true,
        clientX: cx, clientY: cy,
        screenX: cx, screenY: cy,
        pageX:   cx, pageY:   cy,
        button: button, buttons: buttons,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8),
        pointerId: 1, pointerType: 'mouse', isPrimary: true,
        pressure: buttons ? 0.5 : 0.0,
        altitudeAngle: Math.PI / 2, azimuthAngle: 0,
        width: 1, height: 1,
        tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
    });
    cev.getCoalescedEvents = function() { return [cev]; };
    cev.getPredictedEvents = function() { return []; };
    return cev;
}

// Pointer Events L3 §4.1: linearly extrapolate up to 2 future positions from
// the last two entries of `coalesced` (oldest..newest, main event last).
// Returns [] when fewer than 2 points are available (no velocity to derive).
// Not a spec-mandated algorithm — linear extrapolation is an accepted default.
function _lumen_predict_pointer_events(coalesced) {
    var n = coalesced.length;
    if (n < 2) return [];
    var last = coalesced[n - 1];
    var prev = coalesced[n - 2];
    var dx = last.clientX - prev.clientX;
    var dy = last.clientY - prev.clientY;
    var mod = (last.ctrlKey ? 1 : 0) | (last.shiftKey ? 2 : 0) |
              (last.altKey  ? 4 : 0) | (last.metaKey  ? 8 : 0);
    var out = [];
    for (var i = 1; i <= 2; i++) {
        out.push(_lumen_make_coalesced_pointer_event(
            last.type, last.clientX + dx * i, last.clientY + dy * i,
            last.button, last.buttons, mod, last.bubbles
        ));
    }
    return out;
}

// Called from shell for pointer events (W3C Pointer Events Level 2/3).
// Mirrors _lumen_dispatch_mouse_event but creates a PointerEvent (extends MouseEvent).
// Non-bubbling types (pointerenter / pointerleave) set bubbles:false per spec.
// mod: bit-mask — bit0=ctrl, bit1=shift, bit2=alt, bit3=meta
// coalesced: optional array of [x,y] CSS-pixel positions buffered since the
// last dispatch (Level 3 §4.1), oldest first, NOT including this event's own
// (clientX, clientY). Omitted/empty for non-move event types.
function _lumen_dispatch_pointer_event(start_nid, type, clientX, clientY, button, buttons, mod, coalesced) {
    var bubbles = (type !== 'pointerenter' && type !== 'pointerleave');
    var ev = new PointerEvent(type, {
        bubbles: bubbles, cancelable: bubbles, isTrusted: true,
        clientX: clientX, clientY: clientY,
        screenX: clientX, screenY: clientY,
        pageX:   clientX, pageY:   clientY,
        button: button, buttons: buttons,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8),
        pointerId: 1, pointerType: 'mouse', isPrimary: true,
        pressure: buttons ? 0.5 : 0.0,
        // Pointer Events Level 3 §4.1 — mouse always perpendicular to surface
        altitudeAngle: Math.PI / 2, azimuthAngle: 0,
        width: 1, height: 1,
        tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
    });
    // Level 3 §4.1: intermediate samples buffered since the last dispatch,
    // then this event appended last (spec order: oldest..newest, main event
    // last). Without `coalesced` (non-move event types, or callers using the
    // dedicated `_lumen_dispatch_pointer_move_coalesced` instead) this is
    // just [ev], with no predicted events — same as the non-coalescing case.
    var coalescedEvents = [];
    if (Array.isArray(coalesced)) {
        for (var i = 0; i < coalesced.length; i++) {
            coalescedEvents.push(_lumen_make_coalesced_pointer_event(
                type, coalesced[i][0], coalesced[i][1], button, buttons, mod, bubbles
            ));
        }
    }
    coalescedEvents.push(ev);
    ev.getCoalescedEvents = function() { return coalescedEvents; };
    ev.getPredictedEvents = function() { return _lumen_predict_pointer_events(coalescedEvents); };
    return _lumen_dispatch_rich(start_nid, ev);
}

// Ph3 pointer-events-l3, Срез 3-4 — called from shell with every raw
// `CursorMoved` sample buffered since the last flush (Pointer Events Level 3
// §4.1 coalesced events). `points_json` is a JSON array of `[x, y]`
// CSS-pixel pairs in chronological order; the last pair is the main event
// actually dispatched to `nid`. Builds one `PointerEvent` per point (shared
// pointerId/pointerType/button state, own clientX/Y) and exposes the full
// list via `getCoalescedEvents()` on the main event — main event last, per
// spec. `getPredictedEvents()` linearly extrapolates two future points from
// the last two samples' velocity (the spec does not mandate a specific
// prediction algorithm); fewer than 2 samples → no prediction.
// mod: bit-mask — bit0=ctrl, bit1=shift, bit2=alt, bit3=meta
function _lumen_dispatch_pointer_move_coalesced(nid, points_json, button, buttons, mod) {
    var points = JSON.parse(points_json);
    if (points.length === 0) return;
    function makeMoveEvent(x, y) {
        return new PointerEvent('pointermove', {
            bubbles: true, cancelable: true, isTrusted: true,
            clientX: x, clientY: y,
            screenX: x, screenY: y,
            pageX:   x, pageY:   y,
            button: button, buttons: buttons,
            ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
            altKey:   !!(mod & 4), metaKey:  !!(mod & 8),
            pointerId: 1, pointerType: 'mouse', isPrimary: true,
            pressure: buttons ? 0.5 : 0.0,
            altitudeAngle: Math.PI / 2, azimuthAngle: 0,
            width: 1, height: 1,
            tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
        });
    }
    var coalesced = [];
    for (var i = 0; i < points.length - 1; i++) {
        coalesced.push(makeMoveEvent(points[i][0], points[i][1]));
    }
    var last = points[points.length - 1];
    var main = makeMoveEvent(last[0], last[1]);
    coalesced.push(main); // main event is last, per Pointer Events L3 §4.1
    main.getCoalescedEvents = function() { return coalesced; };
    var predicted = [];
    if (points.length >= 2) {
        var a = points[points.length - 2];
        var dx = last[0] - a[0];
        var dy = last[1] - a[1];
        predicted.push(makeMoveEvent(last[0] + dx, last[1] + dy));
        predicted.push(makeMoveEvent(last[0] + dx * 2, last[1] + dy * 2));
    }
    main.getPredictedEvents = function() { return predicted; };
    return _lumen_dispatch_rich(nid, main);
}

// _lumen_dispatch_capture_event — fire gotpointercapture / lostpointercapture on a node.
// W3C Pointer Events L3 §4.1: these events do NOT bubble.
function _lumen_dispatch_capture_event(nid, type) {
    var ev = new PointerEvent(type, {
        bubbles: false, cancelable: false, isTrusted: true,
        pointerId: 1, pointerType: 'mouse', isPrimary: true,
        altitudeAngle: Math.PI / 2, azimuthAngle: 0,
        width: 1, height: 1,
        tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
    });
    // gotpointercapture/lostpointercapture never coalesce (not a move event);
    // an empty sequence is the spec-correct answer, not a placeholder.
    ev.getCoalescedEvents = function() { return []; };
    ev.getPredictedEvents = function() { return []; };
    _lumen_dispatch_rich(nid, ev);
}

// Called from shell for keydown / keyup / keypress events.
// mod: same bit-mask as _lumen_dispatch_mouse_event
// Engine → shim: publish a form control's new value after the engine performed
// its own native text-editing default action (BUG-436).
//
// Since BUG-441 the current value of an `<input>`/`<textarea>` lives in the
// document itself, so this writes there — the same slot `el.value = …` writes
// and both layout and form submission read. The shell calls it right after its
// own edit and before it dispatches `input`, so a listener reading `this.value`
// sees exactly what the field now renders. `_input_values` is kept in step for
// the remaining control kinds that still shadow their value in JS.
function _lumen_set_field_value(nid, value) {
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    if (tag === 'INPUT' || tag === 'TEXTAREA') {
        _lumen_set_dirty_value(nid, String(value));
        return;
    }
    _input_values[nid] = String(value);
}

function _lumen_dispatch_key_event(start_nid, type, key, code, keyCode, location, mod, repeat, isComposing) {
    var ev = new KeyboardEvent(type, {
        bubbles: true, cancelable: true, isTrusted: true,
        key: key, code: code, keyCode: keyCode, charCode: keyCode,
        which: keyCode, location: location,
        repeat: !!repeat, isComposing: !!isComposing,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8)
    });
    return _lumen_dispatch_rich(start_nid, ev);
}

// ── DOMTokenList (classList) ──────────────────────────────────────────────────

// DOM §7.1: one DOMTokenList over an arbitrary space-separated attribute.
// `classList` is the `class` case; `relList` (BUG-826) is the same list over
// `rel` plus a `supports()` of its own.
function _lumen_make_attr_token_list(nid, attrName) {
    function getArr() {
        var c = _lumen_get_attr(nid, attrName);
        return (c && c.length > 0)
            ? c.split(/\s+/).filter(function(t) { return t.length > 0; })
            : [];
    }
    function setArr(arr) { _lumen_set_attr(nid, attrName, arr.join(' ')); }
    var cl = {
        contains: function(cls) { return getArr().indexOf(String(cls)) >= 0; },
        add: function() {
            var arr = getArr();
            for (var i = 0; i < arguments.length; i++) {
                var cls = String(arguments[i]);
                if (arr.indexOf(cls) < 0) arr.push(cls);
            }
            setArr(arr);
        },
        remove: function() {
            var arr = getArr();
            for (var i = 0; i < arguments.length; i++) {
                var cls = String(arguments[i]);
                var idx = arr.indexOf(cls);
                if (idx >= 0) arr.splice(idx, 1);
            }
            setArr(arr);
        },
        toggle: function(cls, force) {
            cls = String(cls);
            var arr = getArr();
            var idx = arr.indexOf(cls);
            if (force !== undefined) {
                if (force && idx < 0)   { arr.push(cls); setArr(arr); return true; }
                if (!force && idx >= 0) { arr.splice(idx, 1); setArr(arr); return false; }
                return !!force;
            }
            if (idx >= 0) { arr.splice(idx, 1); setArr(arr); return false; }
            arr.push(cls); setArr(arr); return true;
        },
        replace: function(oldCls, newCls) {
            var arr = getArr();
            var idx = arr.indexOf(String(oldCls));
            if (idx < 0) return false;
            arr[idx] = String(newCls); setArr(arr); return true;
        },
        item: function(i) { var arr = getArr(); return arr[i] !== undefined ? arr[i] : null; },
        forEach: function(fn, thisArg) { getArr().forEach(fn, thisArg); },
        toString: function() { return getArr().join(' '); },
    };
    Object.defineProperty(cl, 'length', {
        get: function() { return getArr().length; },
        enumerable: true, configurable: true,
    });
    return cl;
}

function _lumen_make_class_list(nid) {
    return _lumen_make_attr_token_list(nid, 'class');
}

// ── CSSStyleDeclaration (inline style) ───────────────────────────────────────

function _lumen_parse_style(s) {
    var obj = {};
    if (!s) return obj;
    s.split(';').forEach(function(decl) {
        var idx = decl.indexOf(':');
        if (idx < 0) return;
        var prop = decl.slice(0, idx).trim();
        var val  = decl.slice(idx + 1).trim();
        if (prop) obj[prop] = val;
    });
    return obj;
}
function _lumen_serialize_style(obj) {
    return Object.keys(obj).map(function(k) { return k + ': ' + obj[k]; }).join('; ');
}
function _lumen_camel_to_kebab(prop) {
    return prop.replace(/([A-Z])/g, function(m) { return '-' + m.toLowerCase(); });
}

function _lumen_make_style(nid) {
    function getParsed() {
        var s = _lumen_get_attr(nid, 'style');
        return _lumen_parse_style(s !== undefined ? s : '');
    }
    function setParsed(obj) { _lumen_set_attr(nid, 'style', _lumen_serialize_style(obj)); }
    var handler = {
        getPropertyValue: function(prop) {
            return getParsed()[_lumen_camel_to_kebab(String(prop))] || '';
        },
        setProperty: function(prop, val) {
            var obj = getParsed();
            obj[_lumen_camel_to_kebab(String(prop))] = String(val);
            setParsed(obj);
        },
        removeProperty: function(prop) {
            var obj = getParsed();
            var key = _lumen_camel_to_kebab(String(prop));
            var old = obj[key] || '';
            delete obj[key]; setParsed(obj); return old;
        },
    };
    Object.defineProperty(handler, 'cssText', {
        get: function() { var s = _lumen_get_attr(nid, 'style'); return s !== undefined ? s : ''; },
        set: function(v) { _lumen_set_attr(nid, 'style', String(v)); },
        enumerable: true, configurable: true,
    });
    return new Proxy(handler, {
        get: function(target, prop) {
            if (prop in target) return target[prop];
            return target.getPropertyValue(_lumen_camel_to_kebab(String(prop)));
        },
        set: function(target, prop, value) {
            if (prop in target) { target[prop] = value; return true; }
            target.setProperty(_lumen_camel_to_kebab(String(prop)), value);
            return true;
        },
    });
}

// ── ShadowRoot wrapper ────────────────────────────────────────────────────────
// Wraps a shadow-root NodeId as a DocumentFragment-like ShadowRoot object.
// `mode`     : 'open' | 'closed' (stored for the `.mode` property)
// `host_nid` : NodeId of the shadow host element

function _lumen_make_shadow_root(nid, mode, host_nid) {
    var _style = _lumen_make_style(nid);
    var sr = {
        __nid__:          nid,
        __isShadowRoot__: true,
        mode:             mode,
        get host()        { return _lumen_make_element(host_nid); },
        // DOM §4.4 Node.baseURI (BUG-377) — own copy for the same reason as the
        // DocumentFragment wrapper above: a plain literal, no prototype chain.
        get baseURI()     { return _lumen_document_base_url(); },
        get innerHTML()   { return _lumen_get_inner_html(nid); },
        set innerHTML(v)  { _lumen_set_inner_html(nid, String(v)); },
        get textContent() { return _lumen_get_text_content(nid); },
        set textContent(v){ _lumen_set_text_content(nid, String(v)); },
        get style()       { return _style; },
        // Scoped to this shadow tree's descendants — see BUG-291.
        querySelector:    function(sel) {
            var n = _lumen_u2n(_lumen_query_selector_scoped(nid, _lumen_sel(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll: function(sel) {
            return _lumen_query_selector_all_scoped(nid, _lumen_sel(sel)).map(_lumen_make_element);
        },
        getElementById:   function(id) {
            var n = _lumen_u2n(_lumen_get_element_by_id(String(id)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        appendChild:      function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_append_child(nid, c.__nid__);
                _lumen_ce_maybe_connected(c);
            }
            return c;
        },
        removeChild:      function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_remove_child(nid, c.__nid__);
                _lumen_ce_maybe_disconnected(c);
            }
            return c;
        },
        addEventListener:    function(type, fn) { _lumen_add_listener(nid, type, fn); },
        removeEventListener: function(type, fn) { _lumen_rm_listener(nid, type, fn); },
        dispatchEvent:       function(evt) {
            if (!evt) return true;
            evt.target = this; evt.currentTarget = this;
            return _lumen_dispatch(nid, evt);
        },
    };
    Object.defineProperty(sr, 'children', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    return sr;
}

// ── DocumentFragment wrapper ──────────────────────────────────────────────────
// Wraps a DocumentFragment NodeId. Unlike ShadowRoot, a DocumentFragment is
// consumed when appended: all children are moved to the target parent (DOM LS
// §4.2.4). `cloneNode(true)` on a fragment deep-clones without consuming it.

// DOM §4.2.6 «converting nodes into a node»: аргумент ParentNode-методов —
// либо узел, либо строка, которая становится текстовым узлом. Возвращает nid
// или `null`, если аргумент не то и не другое.
function _lumen_node_or_text_nid(arg) {
    if (typeof arg === 'string') return _lumen_create_text_node(arg);
    if (arg && arg.__nid__ !== undefined) return arg.__nid__;
    return _lumen_create_text_node(String(arg));
}

function _lumen_make_document_fragment(nid) {
    var frag = {
        __nid__:              nid,
        __isDocumentFragment__: true,
        get nodeType()        { return 11; }, // Node.DOCUMENT_FRAGMENT_NODE
        get nodeName()        { return '#document-fragment'; },
        // BUG-314: `new DocumentFragment()` is owned by the current document;
        // `firstChild` returns the first inserted child (cached wrapper, so it
        // compares === with the node handed to appendChild).
        get ownerDocument()   { return document; },
        // DOM §4.4 Node.baseURI (BUG-377) — own copy: this wrapper is a plain
        // literal with no [[Prototype]], so the shared `Node.prototype`
        // accessor never reaches it. Its node document is the live one above.
        get baseURI()         { return _lumen_document_base_url(); },
        get firstChild()      {
            var ch = _lumen_get_children(nid);
            return ch.length ? _lumen_make_element(ch[0]) : null;
        },
        get textContent()     { return _lumen_get_text_content(nid); },
        set textContent(v)    { _lumen_set_text_content(nid, String(v)); },
        get innerHTML()       { return _lumen_get_inner_html(nid); },
        set innerHTML(v)      { _lumen_set_inner_html(nid, String(v)); },
        // Scoped to this fragment's descendants — see BUG-291.
        querySelector:        function(sel) {
            var n = _lumen_u2n(_lumen_query_selector_scoped(nid, _lumen_sel(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll:     function(sel) {
            return _lumen_query_selector_all_scoped(nid, _lumen_sel(sel)).map(_lumen_make_element);
        },
        appendChild:          function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_append_child(nid, c.__nid__);
            }
            return c;
        },
        removeChild:          function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_remove_child(nid, c.__nid__);
            }
            return c;
        },
        // cloneNode: returns a new fragment with deep-cloned children (always deep for fragments).
        cloneNode:            function(deep) {
            var clone_nid = _lumen_clone_subtree(nid, deep ? 1 : 0);
            return _lumen_make_document_fragment(clone_nid);
        },
        // Ниже — узловые операции, которых у фрагмента не было вовсе, хотя
        // именно во фрагмент (`template.content`) реактивные библиотеки
        // собирают разметку перед вставкой в документ.
        get lastChild()       {
            var ch = _lumen_get_children(nid);
            return ch.length ? _lumen_make_element(ch[ch.length - 1]) : null;
        },
        // У фрагмента родителя не бывает по определению (DOM §4.7).
        get parentNode()      { return null; },
        get parentElement()   { return null; },
        get nextSibling()     { return null; },
        get previousSibling() { return null; },
        insertBefore:         function(newNode, refNode) {
            if (!newNode || newNode.__nid__ === undefined) {
                throw new TypeError('insertBefore: newNode must be a node');
            }
            if (refNode === null || refNode === undefined) {
                _lumen_append_child(nid, newNode.__nid__);
            } else {
                _lumen_insert_before(nid, newNode.__nid__, refNode.__nid__);
            }
            return newNode;
        },
        replaceChild:         function(newChild, oldChild) {
            if (!newChild || !oldChild || newChild.__nid__ === undefined || oldChild.__nid__ === undefined) {
                throw new TypeError('replaceChild: both arguments must be nodes');
            }
            _lumen_insert_before(nid, newChild.__nid__, oldChild.__nid__);
            _lumen_remove_child(nid, oldChild.__nid__);
            return oldChild;
        },
        hasChildNodes:        function() { return _lumen_get_children(nid).length > 0; },
        contains:             function(other) { return _lumen_node_contains(this, other); },
        compareDocumentPosition: function(other) { return _lumen_node_compare_position(this, other); },
        isSameNode:           function(other) { return !!other && _lumen_tree_nid(other) === nid; },
        getRootNode:          function() { return this; },
        // DOM §4.2.6 ParentNode.append/prepend/replaceChildren — принимают узлы
        // и строки (строка становится текстовым узлом).
        append:               function() {
            for (var i = 0; i < arguments.length; i++) {
                _lumen_append_child(nid, _lumen_node_or_text_nid(arguments[i]));
            }
        },
        prepend:              function() {
            var first = _lumen_get_children(nid)[0];
            for (var i = 0; i < arguments.length; i++) {
                var cnid = _lumen_node_or_text_nid(arguments[i]);
                if (first === undefined) {
                    _lumen_append_child(nid, cnid);
                } else {
                    _lumen_insert_before(nid, cnid, first);
                }
            }
        },
        replaceChildren:      function() {
            var kids = _lumen_get_children(nid);
            for (var i = 0; i < kids.length; i++) {
                _lumen_remove_child(nid, kids[i]);
            }
            for (var j = 0; j < arguments.length; j++) {
                _lumen_append_child(nid, _lumen_node_or_text_nid(arguments[j]));
            }
        },
    };
    // DOM §4.2.6 ParentNode.children — element-only live HTMLCollection (BUG-310).
    Object.defineProperty(frag, 'children', {
        get: function() { return _lumen_make_html_collection(nid); },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(frag, 'childNodes', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    return frag;
}

// XML 1.0 Name production (https://www.w3.org/TR/xml/#NT-Name), used by
// `document.createProcessingInstruction` to validate the `target` (DOM §4.5).
// BMP-only: the astral NameStartChar range #x10000-#xEFFFF is omitted (no WPT
// subtest exercises it and surrogate handling would add noise). Combining/
// punctuation ranges are split so that e.g. U+00D7 (×) and U+00B7 (·, middle
// dot) are excluded from NameStartChar but the latter is a valid NameChar.
var _LUMEN_XML_NAME_START =
    '\u003A\u0041-\u005A\u005F\u0061-\u007A' +
    '\u00C0-\u00D6\u00D8-\u00F6\u00F8-\u02FF\u0370-\u037D' +
    '\u037F-\u1FFF\u200C-\u200D\u2070-\u218F\u2C00-\u2FEF' +
    '\u3001-\uD7FF\uF900-\uFDCF\uFDF0-\uFFFD';
var _LUMEN_XML_NAME_CHAR = _LUMEN_XML_NAME_START +
    '\u002D\u002E\u0030-\u0039\u00B7\u0300-\u036F\u203F-\u2040';
var _LUMEN_XML_NAME_RE = new RegExp(
    '^[' + _LUMEN_XML_NAME_START + '][' + _LUMEN_XML_NAME_CHAR + ']*$');

// True if `s` matches the XML Name production. Empty string is not a Name.
function _lumen_is_xml_name(s) {
    return _LUMEN_XML_NAME_RE.test(s);
}

// DOM §4.2.3 (pre-insert validity): Node-insertion methods must throw
// HierarchyRequestError when called on a CharacterData receiver — Text,
// Comment and ProcessingInstruction can never have children. Shared by the
// generic `appendChild` (Text/Comment, wrapped via `_lumen_make_element`) and
// the ProcessingInstruction object below (BUG-325).
function _lumen_character_data_insertion_error() {
    return new DOMException(
        'Node insertion methods are not supported on CharacterData nodes',
        'HierarchyRequestError');
}

// DOM §4.5 ProcessingInstruction — a detached, JS-only CharacterData node
// (no arena backing; PIs are never laid out). Enough surface for scripts to
// read/write `target`/`data` and inspect `nodeType`/`ownerDocument`. The
// `ProcessingInstruction`/`CharacterData`/`Node` interface globals (for
// `instanceof`) are defined below (BUG-314) and this object's prototype is set
// to `ProcessingInstruction.prototype` before it is returned.
function _lumen_make_processing_instruction(target, data) {
    var _data = String(data);
    var pi = {
        __isProcessingInstruction__: true,
        get nodeType()      { return 7; }, // Node.PROCESSING_INSTRUCTION_NODE
        get nodeName()      { return target; },
        get target()        { return target; },
        get data()          { return _data; },
        set data(v)         { _data = String(v); },
        get nodeValue()     { return _data; },
        set nodeValue(v)    { _data = String(v); },
        get textContent()   { return _data; },
        set textContent(v)  { _data = String(v); },
        get length()        { return _data.length; },
        get ownerDocument() { return document; },
        get parentNode()    { return null; },
        get childNodes()    { return []; },
        // BUG-325: CharacterData never has children — throw, not `TypeError:
        // ... is not a function`, for the whole Node-insertion surface.
        appendChild:  function()  { throw _lumen_character_data_insertion_error(); },
        insertBefore: function()  { throw _lumen_character_data_insertion_error(); },
        replaceChild: function()  { throw _lumen_character_data_insertion_error(); },
        removeChild:  function()  { throw _lumen_character_data_insertion_error(); },
    };
    // BUG-314: give the PI object the ProcessingInstruction → CharacterData →
    // Node prototype chain so `pi instanceof ProcessingInstruction` holds. The
    // literal's own accessors above take precedence over anything on the chain.
    Object.setPrototypeOf(pi, ProcessingInstruction.prototype);
    return pi;
}

// ── DOM interface constructors (DOM Standard §4, HTML §4) ────────────────────
// BUG-314: node-family interfaces exposed as global constructors. Two roles:
//   1. Reference / `instanceof` resolution — before this a bare
//      `x instanceof Node` or `window['Comment']` threw `... is not defined`,
//      taking whole scripts (and testharness feature-detection) down. Every
//      interface below at least resolves now.
//   2. Real construction — `new Comment(data)`, `new Text(data)` and
//      `new DocumentFragment()` build actual nodes.
// The CharacterData family (Comment/Text/ProcessingInstruction) are detached
// JS-only objects (no arena backing — same design as the pre-existing PI node),
// so they get a REAL prototype chain and working `instanceof`. Native-backed
// element/text wrappers built by `_lumen_build_element` get their [[Prototype]]
// wired up too (BUG-322, see `_lumen_element_prototype_for` below), so ordinary
// `document.createElement('div') instanceof HTMLDivElement/HTMLElement/Element/Node`
// hold for live nodes as well — not just the detached constructor forms above. A
// constructible `new Document()` and a live `document.doctype` node are deferred
// (BUG-321, since fixed).

// Abstract bases — not constructible from script (DOM §4.4/§4.9, HTML §3.2.2).
function Node() { throw new TypeError('Illegal constructor'); }
// DOM §4.4 Node.hasChildNodes() — shared by every node kind (element, live
// text/comment, Document, DocumentFragment, DocumentType) via the Node.prototype
// chain wired below and by BUG-322, so it only needs `this.childNodes` to exist
// on the receiver. BUG-327: was missing entirely (`c.hasChildNodes is not a
// function`), alongside `.childNodes` itself being absent on the ordinary live
// element/text/comment wrapper — see the `childNodes` getter added to `_obj` in
// `_lumen_build_element` below.
Node.prototype.hasChildNodes = function() { return this.childNodes.length > 0; };

// DOM §4.4 Node.baseURI (BUG-377) — was missing everywhere, not even as a
// broken getter: `'baseURI' in document` answered `false`, so the very common
// `document.baseURI.substring(...)` opener of a test/helper file died with
// `Cannot read properties of undefined`. It is an attribute of `Node`, not of
// `Document`, so one accessor on the shared prototype serves elements, text,
// comments, doctypes, attributes and detached CharacterData alike — everything
// `_lumen_build_element`/`_lumen_make_character_data`/`_lumen_make_doctype`
// hands out chains through here (BUG-322/314). The four node-ish shapes that
// are plain literals with no [[Prototype]] at all (`document`, the
// detached-document builder, the DocumentFragment and ShadowRoot wrappers)
// carry an own copy instead; see each of them below.
//
// Value is the node document's base URL, i.e. the same HTML LS §4.2.3 answer
// the shim's own URL-reflection machinery already computes — hence the reuse of
// `_lumen_document_base_url()` (function declaration, hoisted from further down
// the shim) rather than a second `<base>` walk that could drift from it.
//
// Readonly per WebIDL: a getter and NO setter — deliberately not the
// empty-setter stub that BUG-375 was about, which swallows an assignment
// silently instead of ignoring it the way a real accessor property does.
Object.defineProperty(Node.prototype, 'baseURI', {
    get: function() { return _lumen_document_base_url(); },
    enumerable: true,
    configurable: true,
});

// ── DOM §4.4 Node.contains() / compareDocumentPosition() (BUG-732) ───────────
// Both were missing outright, so a call landed as `TypeError: ... is not a
// function` in the middle of third-party code (confirmed on a live site) and
// took the rest of that script down with it — the ordinary shape
// `if (container.contains(target))` has no fallback path to take.
//
// Both work on arena node ids rather than on JS wrapper identity: the live
// `document` singleton is an object literal, not an `__nid__` carrier, and
// `documentElement.parentNode` answers a *wrapper for the document root node*,
// not that literal — so an identity-based parent walk would report
// `document.contains(el) === false` for every element on the page.

// Arena id of `n` for tree-order work: the node id for native-backed
// wrappers, the document root's id for the `document` singleton,
// `null` for detached JS-only nodes (`new Comment()` and friends, which have no
// arena backing at all).
function _lumen_tree_nid(n) {
    if (n === null || n === undefined || (typeof n !== 'object' && typeof n !== 'function')) return null;
    if (n === document) return _lumen_root_nid;
    var v = n.__nid__;
    return typeof v === 'number' ? v : null;
}

// [node, parent, ..., root] in arena ids.
function _lumen_ancestor_nids(nid) {
    var chain = [];
    var cur = nid;
    while (cur !== null && cur !== undefined) {
        chain.push(cur);
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    return chain;
}

// Total order used only for the DISCONNECTED case, where the spec asks for a
// consistent result (`a.compareDocumentPosition(b)` must be the mirror of
// `b.compareDocumentPosition(a)`) but not for any particular one. Arena nodes
// order by node id; a JS-only node gets a lazily assigned ordinal and sorts
// after every arena node.
var _LUMEN_DP_JS_BASE = 4294967296;
var _lumen_dp_next_ordinal = 1;
function _lumen_dp_rank(n) {
    var nid = _lumen_tree_nid(n);
    if (nid !== null) return nid;
    if (!Object.prototype.hasOwnProperty.call(n, '__lumen_dp_ord__')) {
        try {
            Object.defineProperty(n, '__lumen_dp_ord__', {
                value: _lumen_dp_next_ordinal++, enumerable: false, configurable: false, writable: false,
            });
        } catch (e) {
            return _LUMEN_DP_JS_BASE;
        }
    }
    return _LUMEN_DP_JS_BASE + n.__lumen_dp_ord__;
}

function _lumen_node_contains(self, other) {
    if (other === null || other === undefined) return false;
    if (self === other) return true;
    var a = _lumen_tree_nid(self);
    var b = _lumen_tree_nid(other);
    if (a !== null && b !== null) {
        var cur = b;
        while (cur !== null) {
            if (cur === a) return true;
            cur = _lumen_u2n(_lumen_get_parent(cur));
        }
        return false;
    }
    // At least one side is a detached JS-only node: no arena chain to walk, so
    // fall back to wrapper identity up the `parentNode` links those nodes do keep.
    var n = other;
    while (n) {
        if (n === self) return true;
        n = n.parentNode;
    }
    return false;
}

function _lumen_node_compare_position(self, other) {
    if (other === null || other === undefined || typeof other !== 'object') {
        throw new TypeError('compareDocumentPosition: argument is not a Node');
    }
    if (self === other) return 0;
    var a = _lumen_tree_nid(self);
    var b = _lumen_tree_nid(other);
    if (a !== null && b !== null) {
        if (a === b) return 0;
        var ca = _lumen_ancestor_nids(a);
        var cb = _lumen_ancestor_nids(b);
        if (ca[ca.length - 1] === cb[cb.length - 1]) {
            // DOM §4.4: an ancestor precedes and contains; a descendant follows
            // and is contained by.
            if (cb.indexOf(a) >= 0) return 20;  // CONTAINED_BY | FOLLOWING
            if (ca.indexOf(b) >= 0) return 10;  // CONTAINS | PRECEDING
            // Same tree, neither contains the other: compare the two distinct
            // children of their nearest common ancestor in child order.
            var i = ca.length - 1;
            var j = cb.length - 1;
            while (i >= 0 && j >= 0 && ca[i] === cb[j]) { i--; j--; }
            var kids = _lumen_get_children(ca[i + 1]);
            return kids.indexOf(cb[j]) > kids.indexOf(ca[i]) ? 4 : 2;  // FOLLOWING : PRECEDING
        }
    }
    // Different trees, or a node with no arena backing: DISCONNECTED |
    // IMPLEMENTATION_SPECIFIC plus a stable direction.
    return 33 | (_lumen_dp_rank(other) > _lumen_dp_rank(self) ? 4 : 2);
}

Node.prototype.contains = function(other) { return _lumen_node_contains(this, other); };
Node.prototype.compareDocumentPosition = function(other) {
    return _lumen_node_compare_position(this, other);
};
// The DOCUMENT_POSITION_* bit names (DOM §4.4) — a caller writes
// `pos & Node.DOCUMENT_POSITION_CONTAINED_BY`, and an undefined constant there
// turns every answer into a silent `0`, so the method is only usable together
// with them. Exposed on the interface object and on the prototype (WebIDL
// constants live on both).
[['DOCUMENT_POSITION_DISCONNECTED', 1],
 ['DOCUMENT_POSITION_PRECEDING', 2],
 ['DOCUMENT_POSITION_FOLLOWING', 4],
 ['DOCUMENT_POSITION_CONTAINS', 8],
 ['DOCUMENT_POSITION_CONTAINED_BY', 16],
 ['DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC', 32]].forEach(function(_c) {
    Object.defineProperty(Node, _c[0], { value: _c[1], writable: false, enumerable: true, configurable: false });
    Object.defineProperty(Node.prototype, _c[0], { value: _c[1], writable: false, enumerable: true, configurable: false });
});

function Element() { throw new TypeError('Illegal constructor'); }
Element.prototype = Object.create(Node.prototype);
Element.prototype.constructor = Element;
function CharacterData() { throw new TypeError('Illegal constructor'); }
CharacterData.prototype = Object.create(Node.prototype);
CharacterData.prototype.constructor = CharacterData;
// DOM §4.10 CharacterData — length/substringData/appendData/insertData/
// deleteData/replaceData. Defined once on the shared prototype so Text,
// Comment and ProcessingInstruction all get them: every concrete `data`
// accessor (native live-tree nodes in `_lumen_build_element`, the detached
// `_lumen_make_character_data` nodes behind `new Comment()`/`new Text()`, and
// the `_lumen_make_processing_instruction` object) is an instance-own
// accessor, so `this.data`/`this.data = ...` below resolve to the right
// backing store regardless of which of the three shapes `this` is — no
// separate native offset/count plumbing needed, since JS strings are already
// UTF-16-code-unit indexed like the DOM spec expects.
function _lumen_character_data_check_offset(offset, length) {
    // WebIDL `unsigned long` (no [EnforceRange]) coerces via ToUint32 — matches
    // `>>> 0` (negative/NaN/out-of-range values wrap, then get range-checked below).
    offset = offset >>> 0;
    if (offset > length) {
        throw new DOMException(
            'Index or size is negative or greater than the allowed amount',
            'IndexSizeError');
    }
    return offset;
}
Object.defineProperty(CharacterData.prototype, 'length', {
    get: function() { return this.data.length; },
    enumerable: true, configurable: true,
});
CharacterData.prototype.substringData = function(offset, count) {
    var data = this.data;
    offset = _lumen_character_data_check_offset(offset, data.length);
    count = count >>> 0;
    if (offset + count > data.length) count = data.length - offset;
    return data.substring(offset, offset + count);
};
CharacterData.prototype.appendData = function(data) {
    this.data = this.data + String(data);
};
CharacterData.prototype.insertData = function(offset, data) {
    this.replaceData(offset, 0, data);
};
CharacterData.prototype.deleteData = function(offset, count) {
    this.replaceData(offset, count, '');
};
CharacterData.prototype.replaceData = function(offset, count, data) {
    var oldData = this.data;
    offset = _lumen_character_data_check_offset(offset, oldData.length);
    count = count >>> 0;
    if (offset + count > oldData.length) count = oldData.length - offset;
    this.data = oldData.substring(0, offset) + String(data) + oldData.substring(offset + count);
};
function Attr() { throw new TypeError('Illegal constructor'); }
Attr.prototype = Object.create(Node.prototype);
Attr.prototype.constructor = Attr;
// DOM §4.5 Document() — a detached document with no browsing context (BUG-321).
// The live page is backed by the single arena `document` object literal below;
// a script-created document cannot own arena nodes, so it tracks its children
// in a JS array (`_lumen_build_detached_document`, defined below once
// `DocumentType`/`DOMImplementation` exist). `new Document()` is the
// `application/xml`-typed constructor form; `DOMImplementation.createDocument`/
// `createHTMLDocument` (BUG-324) build the same shape with a different
// prototype/contentType.
function Document() {
    if (!(this instanceof Document)) { throw new TypeError('Illegal constructor'); }
    return _lumen_build_detached_document(Document.prototype, 'application/xml');
}
Document.prototype = Object.create(Node.prototype);
Document.prototype.constructor = Document;
// DOM §4.5 XMLDocument — the interface `DOMImplementation.createDocument`
// returns (not constructible from script directly).
function XMLDocument() { throw new TypeError('Illegal constructor'); }
XMLDocument.prototype = Object.create(Document.prototype);
XMLDocument.prototype.constructor = XMLDocument;
function DocumentType() { throw new TypeError('Illegal constructor'); }
DocumentType.prototype = Object.create(Node.prototype);
DocumentType.prototype.constructor = DocumentType;
function ProcessingInstruction() { throw new TypeError('Illegal constructor'); }
ProcessingInstruction.prototype = Object.create(CharacterData.prototype);
ProcessingInstruction.prototype.constructor = ProcessingInstruction;
function HTMLElement() { throw new TypeError('Illegal constructor'); }
HTMLElement.prototype = Object.create(Element.prototype);
HTMLElement.prototype.constructor = HTMLElement;
// DOM §4.5 DOMImplementation — not constructible from script; instances are
// only minted by `_lumen_make_dom_implementation` (BUG-324).
function DOMImplementation() { throw new TypeError('Illegal constructor'); }
DOMImplementation.prototype.constructor = DOMImplementation;

// Common concrete HTML element interfaces, generated so `instanceof
// HTMLDivElement` (and feature-detection like `'HTMLDialogElement' in window`)
// resolves. Each is a bare, non-constructible interface whose prototype chains
// through HTMLElement; the `in globalThis` guard preserves already-defined
// interfaces (e.g. the richer `HTMLImageElement`/`Image` pair below).
['HTMLDivElement','HTMLSpanElement','HTMLParagraphElement','HTMLHeadingElement',
 'HTMLAnchorElement','HTMLInputElement','HTMLButtonElement','HTMLSelectElement',
 'HTMLOptionElement','HTMLTextAreaElement','HTMLLabelElement','HTMLFormElement',
 'HTMLUListElement','HTMLOListElement','HTMLLIElement','HTMLTableElement',
 'HTMLTableRowElement','HTMLTableCellElement','HTMLTableSectionElement',
 'HTMLScriptElement','HTMLStyleElement','HTMLLinkElement','HTMLMetaElement',
 'HTMLHtmlElement','HTMLHeadElement','HTMLBodyElement','HTMLTitleElement',
 'HTMLCanvasElement','HTMLVideoElement','HTMLAudioElement','HTMLIFrameElement',
 'HTMLTemplateElement','HTMLPreElement','HTMLBRElement','HTMLHRElement',
 'HTMLDialogElement','HTMLFrameSetElement','HTMLUnknownElement'
].forEach(function(_name) {
    if (_name in globalThis) return;
    var _ctor = function() { throw new TypeError('Illegal constructor'); };
    Object.defineProperty(_ctor, 'name', { value: _name, configurable: true });
    _ctor.prototype = Object.create(HTMLElement.prototype);
    _ctor.prototype.constructor = _ctor;
    globalThis[_name] = _ctor;
});

// BUG-322: tag name (as returned by `_lumen_get_tag_name`, always upper-cased) →
// concrete HTML*Element interface global. Tags without a dedicated entry fall back
// to `HTMLElement.prototype` in `_lumen_element_prototype_for` below, matching HTML
// LS §3.1.3 (most elements use the plain `HTMLElement` interface; `HTMLUnknownElement`
// is reserved for genuinely unrecognized tag names, which this simplification does
// not attempt to distinguish). `HTMLImageElement` (defined further down as the richer
// `Image`/`HTMLImageElement` pair) is referenced here as a hoisted function
// declaration — safe regardless of textual order, since this table itself is only
// read lazily from `_lumen_build_element`, long after the whole shim has loaded.
var _lumen_html_tag_prototypes = {
    'DIV': HTMLDivElement, 'SPAN': HTMLSpanElement, 'P': HTMLParagraphElement,
    'H1': HTMLHeadingElement, 'H2': HTMLHeadingElement, 'H3': HTMLHeadingElement,
    'H4': HTMLHeadingElement, 'H5': HTMLHeadingElement, 'H6': HTMLHeadingElement,
    'A': HTMLAnchorElement, 'INPUT': HTMLInputElement, 'BUTTON': HTMLButtonElement,
    'SELECT': HTMLSelectElement, 'OPTION': HTMLOptionElement, 'TEXTAREA': HTMLTextAreaElement,
    'LABEL': HTMLLabelElement, 'FORM': HTMLFormElement, 'UL': HTMLUListElement,
    'OL': HTMLOListElement, 'LI': HTMLLIElement, 'TABLE': HTMLTableElement,
    'TR': HTMLTableRowElement, 'TD': HTMLTableCellElement, 'TH': HTMLTableCellElement,
    'THEAD': HTMLTableSectionElement, 'TBODY': HTMLTableSectionElement, 'TFOOT': HTMLTableSectionElement,
    'SCRIPT': HTMLScriptElement, 'STYLE': HTMLStyleElement, 'LINK': HTMLLinkElement,
    'META': HTMLMetaElement, 'HTML': HTMLHtmlElement, 'HEAD': HTMLHeadElement,
    'BODY': HTMLBodyElement, 'TITLE': HTMLTitleElement, 'CANVAS': HTMLCanvasElement,
    'VIDEO': HTMLVideoElement, 'AUDIO': HTMLAudioElement, 'IFRAME': HTMLIFrameElement,
    'TEMPLATE': HTMLTemplateElement, 'PRE': HTMLPreElement, 'BR': HTMLBRElement,
    'HR': HTMLHRElement, 'DIALOG': HTMLDialogElement, 'IMG': HTMLImageElement,
    'FRAMESET': HTMLFrameSetElement,
};
// BUG-367: HTML LS §3.1.3 — a tag name the HTML specification does not define
// gets `HTMLUnknownElement`, not `HTMLElement`. Membership of this set is what
// separates the two: everything listed here is a real HTML element (either with
// a dedicated interface in `_lumen_html_tag_prototypes` above or, for the rest,
// plain `HTMLElement`), everything else is unknown. Obsolete-but-parsed names
// that the spec still gives an interface to (`center`, `font`, `marquee`, …)
// belong here as well; the eight names the spec explicitly maps to
// `HTMLUnknownElement` (`applet`, `bgsound`, `blink`, `isindex`, `keygen`,
// `multicol`, `nextid`, `spacer`) are deliberately absent. Keys are upper-cased
// to match `_lumen_get_tag_name`. Valid custom element names (anything with a
// hyphen) are handled separately below — the spec gives those `HTMLElement`
// whether or not `customElements.define` has run.
var _LUMEN_KNOWN_HTML_TAGS = {};
('A ABBR ADDRESS AREA ARTICLE ASIDE AUDIO B BASE BDI BDO BLOCKQUOTE BODY BR ' +
 'BUTTON CANVAS CAPTION CITE CODE COL COLGROUP DATA DATALIST DD DEL DETAILS ' +
 'DFN DIALOG DIV DL DT EM EMBED FIELDSET FIGCAPTION FIGURE FOOTER FORM H1 H2 ' +
 'H3 H4 H5 H6 HEAD HEADER HGROUP HR HTML I IFRAME IMG INPUT INS KBD LABEL ' +
 'LEGEND LI LINK MAIN MAP MARK MENU META METER NAV NOSCRIPT OBJECT OL OPTGROUP ' +
 'OPTION OUTPUT P PICTURE PRE PROGRESS Q RP RT RUBY S SAMP SCRIPT SEARCH ' +
 'SECTION SELECT SLOT SMALL SOURCE SPAN STRONG STYLE SUB SUMMARY SUP TABLE ' +
 'TBODY TD TEMPLATE TEXTAREA TFOOT TH THEAD TIME TITLE TR TRACK U UL VAR VIDEO ' +
 'WBR ' +
 // Obsolete, still parsed, still interface-bearing (HTML LS §16).
 'ACRONYM BASEFONT BIG CENTER DIR FONT FRAME FRAMESET LISTING MARQUEE NOBR ' +
 'NOEMBED NOFRAMES PARAM PLAINTEXT RB RTC STRIKE TT XMP'
).split(' ').forEach(function(_t) { _LUMEN_KNOWN_HTML_TAGS[_t] = true; });

// BUG-322: resolves the [[Prototype]] a native element wrapper (`_lumen_build_element`)
// should get. Non-HTML-namespace elements (SVG/MathML/unknown) get the generic
// `Element.prototype` here — the SVG shim (`svg.rs`) re-points `createElementNS`
// results at typed `SVG*Element` prototypes afterward, and those already chain
// through `Element.prototype` (`class SVGElement extends Element`), so this is a
// safe, non-conflicting default for anything the SVG shim doesn't touch (e.g. SVG
// markup parsed via `innerHTML` rather than `createElementNS`).
function _lumen_element_prototype_for(nid) {
    var ns = _lumen_u2n(_lumen_get_namespace_uri(nid));
    if (ns !== 'http://www.w3.org/1999/xhtml') return Element.prototype;
    var tag  = _lumen_get_tag_name(nid);
    var ctor = _lumen_html_tag_prototypes[tag];
    if (ctor) return ctor.prototype;
    // BUG-367: unknown tag → HTMLUnknownElement (whose chain still runs through
    // HTMLElement.prototype, so `instanceof HTMLElement` keeps holding).
    return (_LUMEN_KNOWN_HTML_TAGS[tag] || tag.indexOf('-') >= 0)
        ? HTMLElement.prototype : HTMLUnknownElement.prototype;
}

// BUG-367: DOM LS §4.9 — an element's web-visible `tagName`/`nodeName` is its
// qualified name, ASCII-upper-cased ONLY when the element is in the HTML
// namespace. The native `_lumen_get_tag_name` upper-cases unconditionally
// because its result keys the interface table above, so reading it directly
// reported `RECT` for an SVG `<rect>`; this helper re-derives the name from the
// untouched local name instead and only falls back to the native string for
// non-elements (`#text`/`#comment`/…), which have no local name at all.
function _lumen_qualified_tag_name(nid) {
    var local = _lumen_u2n(_lumen_get_local_name(nid));
    if (local === null) return _lumen_get_tag_name(nid);
    return _lumen_u2n(_lumen_get_namespace_uri(nid)) === 'http://www.w3.org/1999/xhtml'
        ? local.toUpperCase() : local;
}

// Builds a detached CharacterData node (Comment/Text) with `proto` as its
// [[Prototype]] so both the DOM prototype chain (proto → CharacterData.prototype
// → Node.prototype) and `instanceof` resolve. `data` is stringified per DOM §4.5
// (undefined → '', null → 'null'); only the first constructor argument is read.
function _lumen_make_character_data(nodeType, nodeName, data, proto) {
    var _data = (data === undefined) ? '' : String(data);
    var obj = Object.create(proto);
    // data / nodeValue / textContent are the same mutable CharacterData string.
    ['data', 'nodeValue', 'textContent'].forEach(function(_prop) {
        Object.defineProperty(obj, _prop, {
            get: function() { return _data; },
            set: function(v) { _data = String(v); },
            enumerable: true, configurable: true,
        });
    });
    Object.defineProperty(obj, 'length',        { get: function() { return _data.length; }, enumerable: true, configurable: true });
    Object.defineProperty(obj, 'nodeType',      { get: function() { return nodeType; },     enumerable: true, configurable: true });
    Object.defineProperty(obj, 'nodeName',      { get: function() { return nodeName; },     enumerable: true, configurable: true });
    Object.defineProperty(obj, 'ownerDocument', { get: function() { return document; },     enumerable: true, configurable: true });
    Object.defineProperty(obj, 'parentNode',    { get: function() { return null; },         enumerable: true, configurable: true });
    Object.defineProperty(obj, 'childNodes',    { get: function() { return []; },           enumerable: true, configurable: true });
    return obj;
}

// DOM §4.5 Comment(data) / Text(data) — a returned object wins over `this`, so
// `new Comment()`/`new Text()` yield the detached CharacterData node above.
function Comment(data) { return _lumen_make_character_data(8, '#comment', data, Comment.prototype); }
Comment.prototype = Object.create(CharacterData.prototype);
Comment.prototype.constructor = Comment;
function Text(data) { return _lumen_make_character_data(3, '#text', data, Text.prototype); }
Text.prototype = Object.create(CharacterData.prototype);
Text.prototype.constructor = Text;

// DOM §4.7 DocumentFragment() — a native (arena-backed) empty fragment, so it
// can hold real inserted children. The wrapper is a plain native-backed object,
// so it is NOT a `DocumentFragment` instanceof; the interface global still
// resolves for reference checks.
function DocumentFragment() { return _lumen_make_document_fragment(_lumen_create_fragment()); }
DocumentFragment.prototype = Object.create(Node.prototype);
DocumentFragment.prototype.constructor = DocumentFragment;

// BUG-321: a DocumentType wrapper (`nodeType` 10) whose [[Prototype]] is
// DocumentType.prototype, so `document.doctype instanceof DocumentType` holds.
// Interned in the shared `_lumen_element_wrappers` cache (keyed by nid) so that
// `document.doctype` and `document.childNodes[1]` yield the SAME object
// (`===` node identity) and `_lumen_gc_collect` purges it like any other
// node wrapper. `name`/`publicId`/`systemId` read the native fields on demand.
function _lumen_make_doctype(nid) {
    if (nid === null || nid === undefined) return null;
    var cached = _lumen_element_wrappers[nid];
    if (cached !== undefined) return cached;
    var _field = function(which) {
        var v = _lumen_u2n(_lumen_get_doctype_field(nid, which));
        return v !== null ? v : '';
    };
    var obj = Object.create(DocumentType.prototype);
    Object.defineProperty(obj, '__nid__',       { value: nid, enumerable: false });
    Object.defineProperty(obj, 'nodeType',      { get: function() { return 10; },            enumerable: true });
    Object.defineProperty(obj, 'name',          { get: function() { return _field('name'); },   enumerable: true });
    Object.defineProperty(obj, 'nodeName',      { get: function() { return _field('name'); },   enumerable: true });
    Object.defineProperty(obj, 'publicId',      { get: function() { return _field('public'); }, enumerable: true });
    Object.defineProperty(obj, 'systemId',      { get: function() { return _field('system'); }, enumerable: true });
    Object.defineProperty(obj, 'ownerDocument', { get: function() { return document; },       enumerable: true });
    Object.defineProperty(obj, 'parentNode',    { get: function() { return document; },       enumerable: true });
    // DOM §4.4: DocumentType never has children — BUG-327's Node.prototype.hasChildNodes()
    // needs `.childNodes` to exist on every node kind, not just element/text/comment.
    Object.defineProperty(obj, 'childNodes',    { get: function() { return []; },             enumerable: true });
    _lumen_element_wrappers[nid] = obj;
    return obj;
}

// BUG-321: wrap a child nid by node kind so `document.childNodes` returns a
// DocumentType for the doctype child rather than a bogus element wrapper.
// Text / element / comment nodes fall through to `_lumen_make_element` (whose
// `nodeType` getter already distinguishes text via `_lumen_is_text_node`).
function _lumen_make_node(nid) {
    if (nid === null || nid === undefined) return null;
    if (_lumen_is_doctype(nid)) { return _lumen_make_doctype(nid); }
    return _lumen_make_element(nid);
}

// BUG-324: a DocumentType minted by `DOMImplementation.createDocumentType` —
// detached (no arena backing, unlike the page's own `<!doctype>` wrapped by
// `_lumen_make_doctype` above). DOM §4.5 sets its node document to the
// document whose implementation created it, even before any `appendChild`;
// `__lumen_setOwner` lets `createDocument`/`appendChild` re-home it on
// adoption into a (possibly different) document.
function _lumen_make_detached_doctype(name, publicId, systemId, ownerDoc) {
    var _owner = ownerDoc;
    var obj = Object.create(DocumentType.prototype);
    Object.defineProperty(obj, 'nodeType',      { get: function() { return 10; },   enumerable: true });
    Object.defineProperty(obj, 'nodeName',      { get: function() { return name; }, enumerable: true });
    Object.defineProperty(obj, 'name',          { get: function() { return name; }, enumerable: true });
    Object.defineProperty(obj, 'publicId',      { get: function() { return publicId; }, enumerable: true });
    Object.defineProperty(obj, 'systemId',      { get: function() { return systemId; }, enumerable: true });
    Object.defineProperty(obj, 'nodeValue',     { get: function() { return null; }, enumerable: true });
    Object.defineProperty(obj, 'parentNode',    { get: function() { return null; }, enumerable: true });
    Object.defineProperty(obj, 'childNodes',    { get: function() { return []; },   enumerable: true });
    Object.defineProperty(obj, 'ownerDocument', { get: function() { return _owner; }, enumerable: true });
    Object.defineProperty(obj, '__lumen_setOwner', { value: function(doc) { _owner = doc; }, enumerable: false });
    return obj;
}

// BUG-324: shared builder for a detached document (no browsing context) — used
// by `new Document()` and by `DOMImplementation.createDocument`/
// `createHTMLDocument`. `proto` fixes which interface's prototype chain the
// result exposes (`Document.prototype` vs `XMLDocument.prototype`);
// `contentType` is fixed at construction (DOM §4.5 / §7 — a document with no
// browsing context always resolves to UTF-8 / about:blank / CSS1Compat).
// `createElement`/`createElementNS`/`createTextNode` build real arena nodes
// (never inserted into the live tree's root, so never rendered) so the
// resulting subtree behaves like any other detached DOM subtree; their
// `ownerDocument` still reads back as the single live `document` (the arena
// has no per-node document tag) — a known simplification, not spec-accurate
// for these detached documents.
function _lumen_build_detached_document(proto, contentType) {
    var doc = Object.create(proto);
    var _children = [];
    var _impl = null;
    Object.defineProperty(doc, 'nodeType',      { get: function() { return 9; },            enumerable: true });
    Object.defineProperty(doc, 'nodeName',      { get: function() { return '#document'; },  enumerable: true });
    Object.defineProperty(doc, 'nodeValue',     { get: function() { return null; },         enumerable: true });
    Object.defineProperty(doc, 'DOCUMENT_NODE', { get: function() { return 9; },            enumerable: true });
    Object.defineProperty(doc, 'ownerDocument', { get: function() { return null; },         enumerable: true });
    Object.defineProperty(doc, 'childNodes',    { get: function() { return _children.slice(); }, enumerable: true });
    Object.defineProperty(doc, 'doctype', {
        get: function() {
            for (var i = 0; i < _children.length; i++) {
                if (_children[i] && _children[i].nodeType === 10) { return _children[i]; }
            }
            return null;
        },
        enumerable: true,
    });
    Object.defineProperty(doc, 'documentElement', {
        get: function() {
            for (var i = 0; i < _children.length; i++) {
                if (_children[i] && _children[i].nodeType === 1) { return _children[i]; }
            }
            return null;
        },
        enumerable: true,
    });
    // HTML LS 3.1.4 (BUG-703): head/body of a document with no browsing
    // context — the first `<head>` / `<body>`-or-`<frameset>` child of the
    // document element. The live `document` reads these off the arena
    // (`_lumen_get_head`/`_lumen_get_body`); here the tree is the detached
    // subtree hanging off `documentElement`, so walk its element children.
    // BUG-415: both accessors are rooted at the *html element*, which HTML LS
    // 3.1.4 defines as the document element only when it is an `html` element
    // in the HTML namespace — anything else (a `<body>` promoted to root) has
    // no head and no body. Before this guard the walk started at whatever the
    // document element happened to be, so `doc.appendChild(createElement('body'))`
    // + `body.appendChild(frameset)` reported the frameset as `doc.body`.
    function _detached_html_element() {
        var root = doc.documentElement;
        if (!root || root.__nid__ === undefined) { return null; }
        return (_lumen_is_html_element_nid(root.__nid__)
            && _lumen_u2n(_lumen_get_local_name(root.__nid__)) === 'html') ? root : null;
    }
    function _detached_doc_child(tags) {
        var root = _detached_html_element();
        if (!root) { return null; }
        var kids = root.children;
        for (var i = 0; i < kids.length; i++) {
            if (tags.indexOf(kids[i].tagName) >= 0) { return kids[i]; }
        }
        return null;
    }
    Object.defineProperty(doc, 'head', {
        get: function() { return _detached_doc_child(['HEAD']); },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(doc, 'body', {
        get: function() { return _detached_doc_child(['BODY', 'FRAMESET']); },
        // HTML LS 3.1.4 — the body setter: only a body/frameset element is
        // accepted (anything else is a HierarchyRequestError, a non-node a
        // WebIDL TypeError); it replaces the current body element in place, or
        // is appended to the document element when there is none. With no
        // document element at all there is nowhere to put it.
        set: function(value) {
            if (value === null || value === undefined || value.__nid__ === undefined) {
                throw new TypeError('Document.body: the value is not an HTMLElement');
            }
            var tag = value.tagName;
            if (tag !== 'BODY' && tag !== 'FRAMESET') {
                throw new DOMException(
                    'Document.body: the new value is neither a body nor a frameset element',
                    'HierarchyRequestError');
            }
            var current = _detached_doc_child(['BODY', 'FRAMESET']);
            if (current !== null && current.__nid__ === value.__nid__) { return; }
            if (current !== null) {
                var parent = _lumen_u2n(_lumen_get_parent(current.__nid__));
                if (parent !== null) {
                    _lumen_insert_before(parent, value.__nid__, current.__nid__);
                    _lumen_remove_child(parent, current.__nid__);
                    return;
                }
            }
            // No body element yet: append to the *document element*, not to
            // `_detached_html_element()` — the spec appends even when the root
            // is not an `html` element (the getter then keeps reporting null).
            var root = doc.documentElement;
            if (!root || root.__nid__ === undefined) {
                throw new DOMException(
                    'Document.body: the document has no document element',
                    'HierarchyRequestError');
            }
            _lumen_append_child(root.__nid__, value.__nid__);
        },
        enumerable: true, configurable: true,
    });
    // HTML LS §3.1.5 (BUG-486): a document with no browsing context never runs
    // scripts of its own, so its `currentScript` is always `null` — but the
    // property must exist, or feature detection reads `undefined` on it.
    Object.defineProperty(doc, 'currentScript', {
        get: function() { return null; },
        enumerable: true,
    });
    Object.defineProperty(doc, 'implementation', {
        get: function() {
            if (_impl === null) { _impl = _lumen_make_dom_implementation(doc); }
            return _impl;
        },
        enumerable: true,
    });
    Object.defineProperty(doc, 'URL',           { get: function() { return 'about:blank'; }, enumerable: true });
    Object.defineProperty(doc, 'documentURI',   { get: function() { return 'about:blank'; }, enumerable: true });
    // BUG-377: a document with no browsing context has `about:blank` for its
    // URL, so that is also its base URL — an own property overriding the
    // `Node.prototype` accessor, which would otherwise report the *live* page's
    // base URL for a document that has nothing to do with it.
    Object.defineProperty(doc, 'baseURI',       { get: function() { return 'about:blank'; }, enumerable: true });
    Object.defineProperty(doc, 'compatMode',    { get: function() { return 'CSS1Compat'; },  enumerable: true });
    Object.defineProperty(doc, 'characterSet',  { get: function() { return 'UTF-8'; },       enumerable: true });
    Object.defineProperty(doc, 'charset',       { get: function() { return 'UTF-8'; },       enumerable: true });
    Object.defineProperty(doc, 'inputEncoding', { get: function() { return 'UTF-8'; },       enumerable: true });
    Object.defineProperty(doc, 'contentType',   { get: function() { return contentType; },   enumerable: true });
    Object.defineProperty(doc, 'location',      { get: function() { return null; },          enumerable: true });
    doc.createElement = function(tag) {
        var nid = _lumen_create_element(String(tag).toLowerCase());
        if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
        return _lumen_make_element(nid);
    };
    doc.createElementNS = function(ns, qualifiedName) {
        var local = String(qualifiedName || '').replace(/^[^:]+:/, '');
        var nid = _lumen_create_element_ns(ns === null || ns === undefined ? '' : String(ns), local);
        if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
        return _lumen_make_element(nid);
    };
    doc.createTextNode = function(t) {
        var nid = _lumen_create_text_node(String(t));
        if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
        return _lumen_make_element(nid);
    };
    doc.createComment = function(t) {
        var nid = _lumen_create_comment(t === undefined ? '' : String(t));
        if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
        return _lumen_make_element(nid);
    };
    doc.createDocumentFragment = function() { return _lumen_make_document_fragment(_lumen_create_fragment()); };
    // -- BUG-415: Node / ParentNode over the document's own child list ------
    // Everything *below* the document element is an ordinary arena subtree and
    // already mutates through the element wrappers; what has no arena backing
    // is the document->child edge itself, which lives in `_children`. Until now
    // only `appendChild` existed here, so `doc.removeChild(doc.documentElement)`
    // -- the first line of most WPT document tests -- threw
    // `doc.removeChild is not a function` and took the rest of the file with it.
    function _detached_child_index(node) {
        if (node === null || node === undefined) { return -1; }
        for (var i = 0; i < _children.length; i++) {
            if (_children[i] === node) { return i; }
        }
        // An arena-backed wrapper is minted afresh on every access, so wrapper
        // identity alone is not enough - fall back to the node id.
        var nid = _lumen_tree_nid(node);
        if (nid === null) { return -1; }
        for (var j = 0; j < _children.length; j++) {
            if (_lumen_tree_nid(_children[j]) === nid) { return j; }
        }
        return -1;
    }
    // DOM 4.2.3 pre-insert: a node is removed from wherever it currently hangs
    // before being inserted, be that an arena parent or this list.
    function _detached_adopt(node) {
        if (node === null || node === undefined) {
            throw new TypeError('the argument is not a Node');
        }
        if (typeof node.__lumen_setOwner === 'function') { node.__lumen_setOwner(doc); }
        var nid = _lumen_tree_nid(node);
        if (nid !== null) {
            var parent = _lumen_u2n(_lumen_get_parent(nid));
            if (parent !== null) { _lumen_remove_child(parent, nid); }
        }
        var at = _detached_child_index(node);
        if (at >= 0) { _children.splice(at, 1); }
    }
    doc.appendChild = function(node) {
        _detached_adopt(node);
        _children.push(node);
        return node;
    };
    doc.insertBefore = function(node, ref) {
        if (ref === null || ref === undefined) { return doc.appendChild(node); }
        if (_detached_child_index(ref) < 0) {
            throw new DOMException(
                'insertBefore: the reference node is not a child of this document', 'NotFoundError');
        }
        _detached_adopt(node);
        // The index is re-read after the adopt: removing `node` from this same
        // list may have shifted the reference node down by one.
        _children.splice(_detached_child_index(ref), 0, node);
        return node;
    };
    doc.removeChild = function(node) {
        var at = _detached_child_index(node);
        if (at < 0) {
            throw new DOMException(
                'removeChild: the node is not a child of this document', 'NotFoundError');
        }
        _children.splice(at, 1);
        return node;
    };
    doc.replaceChild = function(newChild, oldChild) {
        if (_detached_child_index(oldChild) < 0) {
            throw new DOMException(
                'replaceChild: the node to replace is not a child of this document', 'NotFoundError');
        }
        _detached_adopt(newChild);
        _children.splice(_detached_child_index(oldChild), 1, newChild);
        return oldChild;
    };
    doc.hasChildNodes = function() { return _children.length > 0; };
    Object.defineProperty(doc, 'firstChild', {
        get: function() { return _children.length > 0 ? _children[0] : null; },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(doc, 'lastChild', {
        get: function() { return _children.length > 0 ? _children[_children.length - 1] : null; },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(doc, 'children', {
        get: function() {
            var out = [];
            for (var i = 0; i < _children.length; i++) {
                if (_children[i] && _children[i].nodeType === 1) { out.push(_children[i]); }
            }
            return out;
        },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(doc, 'childElementCount', {
        get: function() { return doc.children.length; },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(doc, 'firstElementChild', {
        get: function() { return doc.documentElement; },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(doc, 'lastElementChild', {
        get: function() {
            var kids = doc.children;
            return kids.length > 0 ? kids[kids.length - 1] : null;
        },
        enumerable: true, configurable: true,
    });
    // DOM 4.4 Node.contains - the inherited `Node.prototype.contains` walks
    // `parentNode` links, and an arena child of this document has none (the
    // edge is in `_children`), so it would answer false for the document's own
    // subtree. Bridge the one missing hop explicitly.
    doc.contains = function(other) {
        if (other === doc) { return true; }
        for (var i = 0; i < _children.length; i++) {
            if (_lumen_node_contains(_children[i], other)) { return true; }
        }
        return false;
    };
    // DOM 4.4 Node.cloneNode - a document clones into another document of the
    // same interface and content type; `deep` also clones the children.
    doc.cloneNode = function(deep) {
        var copy = _lumen_build_detached_document(proto, contentType);
        if (deep) {
            for (var i = 0; i < _children.length; i++) {
                var child = _children[i];
                if (child && typeof child.cloneNode === 'function') {
                    copy.appendChild(child.cloneNode(true));
                }
            }
        }
        return copy;
    };
    // HTML LS 3.1.5: a document with no browsing context never loads, so its
    // ready state is `complete` from the moment it is created.
    Object.defineProperty(doc, 'readyState', {
        get: function() { return 'complete'; },
        enumerable: true, configurable: true,
    });
    // -- BUG-415: tree accessors, scoped to the document element's subtree ---
    // The scoped natives walk descendants only, so the document element itself
    // is tested separately wherever it can legitimately match.
    function _detached_root_nid() {
        var root = doc.documentElement;
        return (root && root.__nid__ !== undefined) ? root.__nid__ : null;
    }
    function _detached_walk(root, visit) {
        var kids = _lumen_get_children(root);
        for (var i = 0; i < kids.length; i++) {
            if (visit(kids[i])) { return kids[i]; }
            var hit = _detached_walk(kids[i], visit);
            if (hit !== null) { return hit; }
        }
        return null;
    }
    doc.querySelector = function(sel) {
        var root = _detached_root_nid();
        if (root === null) { return null; }
        var s = _lumen_sel(sel);
        if (_lumen_node_matches_selector(root, s)) { return _lumen_make_element(root); }
        var n = _lumen_u2n(_lumen_query_selector_scoped(root, s));
        return n !== null ? _lumen_make_element(n) : null;
    };
    doc.querySelectorAll = function(sel) {
        var root = _detached_root_nid();
        if (root === null) { return []; }
        var s = _lumen_sel(sel);
        var hits = _lumen_node_matches_selector(root, s) ? [root] : [];
        return hits.concat(_lumen_query_selector_all_scoped(root, s)).map(_lumen_make_element);
    };
    // Walked rather than routed through the selector engine: an `id` or a tag
    // name is arbitrary text, not a selector, and escaping it into one would
    // turn a lookup miss into a parse error.
    doc.getElementById = function(id) {
        var root = _detached_root_nid();
        if (root === null) { return null; }
        var want = String(id);
        if (_lumen_u2n(_lumen_get_attr(root, 'id')) === want) { return _lumen_make_element(root); }
        var hit = _detached_walk(root, function(n) {
            return _lumen_u2n(_lumen_get_attr(n, 'id')) === want;
        });
        return hit !== null ? _lumen_make_element(hit) : null;
    };
    // DOM 4.5 getElementsByTagName / getElementsByTagNameNS. Walked rather than
    // queried for the same reason `getElementById` above is, and matched by the
    // shared predicates (BUG-416) — which also keeps a `'*'` ask off the text
    // and comment children `_detached_walk` visits.
    function _detached_by_predicate(pred) {
        var root = _detached_root_nid();
        if (root === null) { return []; }
        var out = pred(root) ? [root] : [];
        _detached_walk(root, function(n) {
            if (pred(n)) { out.push(n); }
            return false;
        });
        return out.map(_lumen_make_element);
    }
    doc.getElementsByTagName = function(qualifiedName) {
        return _detached_by_predicate(_lumen_tag_name_predicate(qualifiedName));
    };
    doc.getElementsByTagNameNS = function(namespace, localName) {
        return _detached_by_predicate(_lumen_tag_ns_predicate(namespace, localName));
    };
    doc.getElementsByClassName = function(names) {
        var root = _detached_root_nid();
        if (root === null) { return []; }
        var sel = _lumen_class_selector(names);
        if (sel === null) { return []; }
        var hits = _lumen_node_matches_selector(root, sel) ? [root] : [];
        return hits.concat(_lumen_query_selector_all_scoped(root, sel)).map(_lumen_make_element);
    };
    // HTML LS 3.1.5 document.title. Getter - the child text content of the
    // title element, stripped and collapsed. Setter - retarget the existing
    // title element, or create one in the head; with no head there is nowhere
    // to put it and the assignment is a no-op, as the spec requires.
    function _detached_title_element() {
        var root = _detached_root_nid();
        if (root === null) { return null; }
        var hit = _detached_walk(root, function(n) {
            return _lumen_u2n(_lumen_get_local_name(n)) === 'title';
        });
        return hit !== null ? _lumen_make_element(hit) : null;
    }
    Object.defineProperty(doc, 'title', {
        get: function() {
            var el = _detached_title_element();
            if (el === null) { return ''; }
            // Child text content (DOM 4.9) - Text children only, not the whole
            // descendant text, which is what `textContent` would give.
            var text = '';
            var kids = _lumen_get_children(el.__nid__);
            for (var i = 0; i < kids.length; i++) {
                if (_lumen_is_text_node(kids[i])) { text += _lumen_get_text_content(kids[i]); }
            }
            return text.replace(/[ \t\n\f\r]+/g, ' ').replace(/^ /, '').replace(/ $/, '');
        },
        set: function(value) {
            var el = _detached_title_element();
            if (el === null) {
                var head = doc.head;
                if (head === null) { return; }
                el = doc.createElement('title');
                head.appendChild(el);
            }
            el.textContent = String(value);
        },
        enumerable: true, configurable: true,
    });
    return doc;
}

// BUG-324: `document.implementation` (DOM §4.5 DOMImplementation) — one
// instance per document, cached by the caller (`document`'s own getter below,
// or `_lumen_build_detached_document`'s `_impl` closure) so repeated access
// yields the SAME object (`document.implementation === document.implementation`,
// WPT `Document-implementation.html`).
function _lumen_make_dom_implementation(ownerDoc) {
    var impl = {
        // DOM §4.5 'validate': observed browser behavior (WPT
        // `DOMImplementation-createDocumentType.html`) is far looser than the
        // XML Name production `_lumen_is_xml_name` enforces elsewhere
        // (`createProcessingInstruction`) — leading digits, symbols, empty
        // strings, and stray/duplicate/leading/trailing colons are all
        // accepted; only whitespace and '>' (which would corrupt a
        // `<!DOCTYPE name>` serialization) throw.
        createDocumentType: function(qualifiedName, publicId, systemId) {
            var qn = String(qualifiedName);
            if (/[\s>]/.test(qn)) {
                throw new DOMException(
                    'createDocumentType: qualifiedName contains whitespace or the character >: ' + qn,
                    'InvalidCharacterError');
            }
            return _lumen_make_detached_doctype(qn, String(publicId), String(systemId), ownerDoc);
        },
        // DOM §4.5: namespace/qualifiedName are both required (missing either
        // throws TypeError, per WebIDL argument-count checking); qualifiedName
        // '' or null/undefined omits the document element.
        createDocument: function(namespace, qualifiedName, doctype) {
            if (arguments.length < 2) {
                throw new TypeError('createDocument requires at least 2 arguments');
            }
            var ns = (namespace === undefined || namespace === null) ? null : String(namespace);
            var qn = (qualifiedName === undefined || qualifiedName === null) ? '' : String(qualifiedName);
            var contentType = ns === 'http://www.w3.org/1999/xhtml' ? 'application/xhtml+xml'
                : ns === 'http://www.w3.org/2000/svg' ? 'image/svg+xml'
                : 'application/xml';
            var doc = _lumen_build_detached_document(XMLDocument.prototype, contentType);
            if (doctype !== null && doctype !== undefined) {
                doc.appendChild(doctype);
            }
            if (qn !== '') {
                var local = qn.replace(/^[^:]+:/, '');
                var nid = _lumen_create_element_ns(ns === null ? '' : ns, local);
                if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
                doc.appendChild(_lumen_make_element(nid));
            }
            return doc;
        },
        // DOM §4.5: builds the standard html>head,body skeleton as a REAL
        // (arena-backed, but unattached to the live tree's root) subtree, so
        // `documentElement.firstChild`/`lastChild` etc. traverse it normally.
        // `title` is a WebIDL trailing optional argument with no default — an
        // explicit `undefined` is treated the same as omitted (no <title>
        // element at all), only other values (including `null`) create one.
        createHTMLDocument: function(title) {
            var doc = _lumen_build_detached_document(Document.prototype, 'text/html');
            doc.appendChild(_lumen_make_detached_doctype('html', '', '', doc));
            var htmlNid = _lumen_create_element('html');
            var headNid = _lumen_create_element('head');
            _lumen_append_child(htmlNid, headNid);
            if (arguments.length > 0 && title !== undefined) {
                var titleNid = _lumen_create_element('title');
                _lumen_append_child(headNid, titleNid);
                _lumen_append_child(titleNid, _lumen_create_text_node(String(title)));
            }
            var bodyNid = _lumen_create_element('body');
            _lumen_append_child(htmlNid, bodyNid);
            doc.appendChild(_lumen_make_element(htmlNid));
            return doc;
        },
        // DOM §4.5: legacy no-op, always true.
        hasFeature: function() { return true; },
    };
    Object.setPrototypeOf(impl, DOMImplementation.prototype);
    return impl;
}

// Dispatch slotchange on all <slot> elements inside the shadow root of `host_nid`.
// Called when host's light DOM changes (appendChild / removeChild).
function _lumen_fire_slotchange(host_nid) {
    var sr_nid = _lumen_u2n(_lumen_get_shadow_root(host_nid));
    if (sr_nid === null) return;
    var slots = _lumen_query_selector_all('slot');
    for (var i = 0; i < slots.length; i++) {
        var slot_nid = slots[i];
        var ev = new Event('slotchange', { bubbles: true, cancelable: false });
        _lumen_dispatch(slot_nid, ev);
    }
}

// ── Form Constraint Validation API (HTML LS §4.10.21) ────────────────────────
// Per-nid storage, keyed independently of the (now cached, see BUG-291)
// `_lumen_make_element` wrapper object — kept as separate maps rather than
// folded onto the wrapper to avoid coupling this state to wrapper lifetime.

// nid → custom validity message set via setCustomValidity() ('' → no custom error)
var _validity_msg = {};
// nid → current input value (undefined → fall back to value attribute)
var _input_values = {};
// nid → cached CanvasRenderingContext2D object (persists across _lumen_make_element).
var _canvas2d_ctxs = {};
// nid → cached GPUCanvasContext object (getContext('webgpu'), persists across _lumen_make_element).
var _canvas_webgpu_ctxs = {};
// nid → cached ImageBitmapRenderingContext object (getContext('bitmaprenderer')).
var _canvas_bitmaprenderer_ctxs = {};

// ValidityState — readonly snapshot of one form control's validity.
function ValidityState(flags) {
    this.valueMissing    = !!flags.valueMissing;
    this.typeMismatch    = !!flags.typeMismatch;
    this.patternMismatch = !!flags.patternMismatch;
    this.tooLong         = !!flags.tooLong;
    this.tooShort        = !!flags.tooShort;
    this.rangeUnderflow  = !!flags.rangeUnderflow;
    this.rangeOverflow   = !!flags.rangeOverflow;
    this.stepMismatch    = !!flags.stepMismatch;
    this.badInput        = !!flags.badInput;
    this.customError     = !!flags.customError;
    this.valid = !this.valueMissing   && !this.typeMismatch  && !this.patternMismatch
              && !this.tooLong        && !this.tooShort
              && !this.rangeUnderflow && !this.rangeOverflow && !this.stepMismatch
              && !this.badInput       && !this.customError;
}

// Computes ValidityState for a form control element (HTML LS §4.10.21.1).
function _compute_validity(el) {
    var flags = {};
    var type  = (el.type || 'text').toLowerCase();
    var val   = (el.value != null) ? String(el.value) : '';
    var enid  = el.__nid__;
    var customMsg = (enid !== undefined && _validity_msg[enid]) ? _validity_msg[enid] : '';

    // §4.10.21.1 #1: valueMissing — required + empty
    if (el.hasAttribute && el.hasAttribute('required') && val.trim() === '') {
        flags.valueMissing = true;
    }

    // §4.10.21.1 #3: typeMismatch — email/url/number format
    if (!flags.valueMissing && val !== '') {
        if (type === 'email') {
            // Simplified email check: user@domain.tld
            if (!/^[^\s@,;]+@[^\s@,;]+\.[^\s@,;]+$/.test(val)) flags.typeMismatch = true;
        } else if (type === 'url') {
            try { new URL(val); } catch(e) { flags.typeMismatch = true; }
        } else if (type === 'number') {
            if (isNaN(Number(val))) flags.typeMismatch = true;
        }
    }

    // §4.10.21.1 #4: patternMismatch — pattern attribute
    if (!flags.typeMismatch && val !== '' && el.hasAttribute && el.hasAttribute('pattern')) {
        var pat = el.getAttribute('pattern');
        if (pat) {
            try {
                if (!(new RegExp('^(?:' + pat + ')$')).test(val)) flags.patternMismatch = true;
            } catch(e) {}
        }
    }

    // §4.10.21.1 #6/#7: tooLong / tooShort
    if (el.hasAttribute && el.hasAttribute('maxlength')) {
        var maxL = parseInt(el.getAttribute('maxlength'), 10);
        if (!isNaN(maxL) && val.length > maxL) flags.tooLong = true;
    }
    if (val !== '' && el.hasAttribute && el.hasAttribute('minlength')) {
        var minL = parseInt(el.getAttribute('minlength'), 10);
        if (!isNaN(minL) && val.length < minL) flags.tooShort = true;
    }

    // §4.10.21.1 #5: rangeUnderflow / rangeOverflow / stepMismatch (number + range)
    if (type === 'number' || type === 'range') {
        var num = Number(val);
        if (!isNaN(num) && val !== '') {
            if (el.hasAttribute && el.hasAttribute('min')) {
                var mn = Number(el.getAttribute('min'));
                if (!isNaN(mn) && num < mn) flags.rangeUnderflow = true;
            }
            if (el.hasAttribute && el.hasAttribute('max')) {
                var mx = Number(el.getAttribute('max'));
                if (!isNaN(mx) && num > mx) flags.rangeOverflow = true;
            }
            if (el.hasAttribute && el.hasAttribute('step')) {
                var stepA = el.getAttribute('step');
                if (stepA && stepA !== 'any') {
                    var st = Number(stepA);
                    var base = el.hasAttribute('min') ? Number(el.getAttribute('min')) : 0;
                    if (!isNaN(st) && st > 0 && Math.abs((num - base) % st) > 1e-9) {
                        flags.stepMismatch = true;
                    }
                }
            }
        }
    }

    // §4.10.21.1 #10: customError
    if (customMsg) flags.customError = true;

    return new ValidityState(flags);
}

// ── Path2D class (HTML LS §4.12.5.1.5) ─────────────────────────────────────────
// Reusable path object; coordinates stored in user space; CTM applied at use-time.
function Path2D(arg) {
    // Allocate a native path object and record its ID on this instance.
    var svg = (typeof arg === 'string') ? arg : '';
    if (arg instanceof Path2D) {
        // Copy constructor: create empty then addPath.
        this.__pid__ = _lumen_canvas2d_path2d_new('');
        _lumen_canvas2d_path2d_add_path(this.__pid__, arg.__pid__, '');
    } else {
        this.__pid__ = _lumen_canvas2d_path2d_new(svg);
    }
}
Path2D.prototype.moveTo = function(x, y) {
    _lumen_canvas2d_path2d_move_to(this.__pid__, +x, +y);
};
Path2D.prototype.lineTo = function(x, y) {
    _lumen_canvas2d_path2d_line_to(this.__pid__, +x, +y);
};
Path2D.prototype.closePath = function() {
    _lumen_canvas2d_path2d_close(this.__pid__);
};
Path2D.prototype.bezierCurveTo = function(cp1x, cp1y, cp2x, cp2y, x, y) {
    _lumen_canvas2d_path2d_bezier(this.__pid__, +cp1x, +cp1y, +cp2x, +cp2y, +x, +y);
};
Path2D.prototype.quadraticCurveTo = function(cpx, cpy, x, y) {
    _lumen_canvas2d_path2d_quadratic(this.__pid__, +cpx, +cpy, +x, +y);
};
Path2D.prototype.arc = function(x, y, r, startAngle, endAngle, anticlockwise) {
    _lumen_canvas2d_path2d_arc(this.__pid__, +x, +y, +r, +startAngle, +endAngle, !!anticlockwise);
};
Path2D.prototype.arcTo = function(x1, y1, x2, y2, r) {
    _lumen_canvas2d_path2d_arc_to(this.__pid__, +x1, +y1, +x2, +y2, +r);
};
// ellipse: native binding limited to 7 args, so implemented via arc with save/scale.
Path2D.prototype.ellipse = function(cx, cy, rx, ry, rot, startAngle, endAngle, anticlockwise) {
    // Approximate via arc in scaled user space — correct for all standard use cases.
    // Creates a throwaway arc path and merges segments into this path via arc+addPath.
    var tmp = new Path2D();
    _lumen_canvas2d_path2d_arc(tmp.__pid__, 0, 0, 1, +startAngle, +endAngle, !!anticlockwise);
    // Build transform: scale(rx,ry) then rotate(rot) then translate(cx,cy)
    // [a,b,c,d,e,f] = [rx*cos(r), rx*sin(r), -ry*sin(r), ry*cos(r), cx, cy]
    var cos_r = Math.cos(+rot), sin_r = Math.sin(+rot);
    var rx_ = +rx, ry_ = +ry;
    var a = rx_ * cos_r, b = rx_ * sin_r, c = -ry_ * sin_r, d = ry_ * cos_r;
    _lumen_canvas2d_path2d_add_path(this.__pid__, tmp.__pid__, '' + a + ',' + b + ',' + c + ',' + d + ',' + (+cx) + ',' + (+cy));
};
Path2D.prototype.rect = function(x, y, w, h) {
    _lumen_canvas2d_path2d_rect(this.__pid__, +x, +y, +w, +h);
};
// `Object.prototype.toString.call(new Path2D())` must name the interface; the
// tag itself is defined next to the other canvas classes (`_lumen_idl_tag`).
Path2D.prototype.addPath = function(path, transform) {
    if (!(path instanceof Path2D)) return;
    if (transform && typeof transform === 'object' && transform.a !== undefined) {
        var t = transform;
        _lumen_canvas2d_path2d_add_path(this.__pid__, path.__pid__,
            '' + t.a + ',' + t.b + ',' + t.c + ',' + t.d + ',' + t.e + ',' + t.f);
    } else {
        _lumen_canvas2d_path2d_add_path(this.__pid__, path.__pid__, '');
    }
};

// ── Canvas 2D interfaces (HTML LS §4.12.5) ───────────────────────────────────
// BUG-449: the context, ImageData, TextMetrics, the gradient and the pattern
// used to be object literals minted per call, with every member an OWN property
// and `Object.prototype` for a prototype. So `ctx instanceof
// CanvasRenderingContext2D` was a ReferenceError rather than `false`, `new
// ImageData(w, h)` did not exist at all, and neither a polyfill nor a WPT test
// could patch `X.prototype`. The members live on real prototypes now, and the
// per-instance state sits in ONE non-enumerable slot — which is also what the
// brand checks read: a method invoked on a foreign `this` has to throw a
// TypeError rather than paint onto whichever nid it finds
// (`2d.imageData.create1.this`, `2d.imageData.put.wrongtype`).

// WebIDL: an interface member is enumerable+configurable on the prototype, an
// internal slot is invisible to the page. `_lumen_slot` writes the latter.
function _lumen_slot(obj, name, value) {
    Object.defineProperty(obj, name, {
        value: value, writable: true, enumerable: false, configurable: true,
    });
}

// `Object.prototype.toString.call(x)` must name the interface (WPT's
// `assert_class_string`); the tag has to be per class, an inherited one would
// make every subclass claim the base's name (the BUG-912 shape).
function _lumen_idl_tag(ctor, name) {
    Object.defineProperty(ctor.prototype, Symbol.toStringTag, {
        value: name, writable: false, enumerable: false, configurable: true,
    });
}

// WebIDL `[EnforceRange] long` (canvas §4.12.5.1 declares every ImageData
// coordinate that way): ToNumber, reject anything not finite, truncate toward
// zero, reject outside the 32-bit signed range. Anything looser silently
// answers a question the page did not ask — `getImageData(NaN, 0, 1, 1)` must
// throw, not read the origin (BUG-448).
function _lumen_canvas_long(value, method, argName) {
    var n = Number(value);
    if (!isFinite(n)) {
        throw new TypeError(method + ": " + argName + " is not a finite number");
    }
    n = n < 0 ? Math.ceil(n) : Math.floor(n);
    if (n < -2147483648 || n > 2147483647) {
        throw new TypeError(method + ": " + argName + " is out of range for a long");
    }
    return n;
}

// WebIDL plain `unsigned long` (no [EnforceRange]) — the ImageData constructor
// declares its dimensions that way, so a non-number is NOT a TypeError there:
// ToUint32('width') is 0, and 0 is what makes the constructor throw
// IndexSizeError (`2d.imageData.object.ctor.basics`).
function _lumen_canvas_ulong(value) {
    var n = Number(value);
    if (!isFinite(n)) { return 0; }
    return n >>> 0;
}

// Normalizes a rectangle whose width/height may be negative, the way the
// canvas spec's "get/put image data" steps do: a negative extent flips the
// rectangle about its origin rather than describing an empty area.
function _lumen_canvas_normalize_rect(x, y, w, h) {
    if (w < 0) { x += w; w = -w; }
    if (h < 0) { y += h; h = -h; }
    // Flipping can push the origin past the 32-bit range the binding reads it
    // as; clamping keeps a wrap-around from turning a wholly off-canvas rect
    // into a read of real pixels. Any rect that far out is empty either way,
    // the canvas being at most a few thousand pixels wide.
    if (x < -2147483648) { x = -2147483648; }
    if (y < -2147483648) { y = -2147483648; }
    return { x: x, y: y, w: w, h: h };
}

// ── ImageData (HTML LS §4.12.5.1.14) ─────────────────────────────────────────

// The largest pixel count whose RGBA8 buffer still fits in a 2^30-byte
// allocation. The spec answers an unallocatable size with IndexSizeError, and
// `new ImageData(1 << 31, 1 << 31)` is exactly that case.
var _LUMEN_IMAGE_DATA_MAX_PIXELS = 268435455;

function _lumen_image_data_settings(v, ctorName) {
    var out = { colorSpace: 'srgb', pixelFormat: 'rgba-unorm8' };
    if (v === undefined || v === null) { return out; }
    if (typeof v !== 'object' && typeof v !== 'function') {
        // A dictionary argument that is neither nullish nor an object is a
        // TypeError before any of the constructor's own steps run — which is
        // why `new ImageData(self, 4, 4)` throws TypeError and not
        // IndexSizeError for its zero width.
        throw new TypeError(ctorName + ": settings is not an object");
    }
    if (v.colorSpace !== undefined) {
        var cs = String(v.colorSpace);
        if (cs !== 'srgb' && cs !== 'display-p3') {
            throw new TypeError(ctorName + ": '" + cs + "' is not a valid PredefinedColorSpace");
        }
        out.colorSpace = cs;
    }
    if (v.pixelFormat !== undefined) {
        var pf = String(v.pixelFormat);
        if (pf !== 'rgba-unorm8' && pf !== 'rgba-float16') {
            throw new TypeError(ctorName + ": '" + pf + "' is not a valid ImageDataPixelFormat");
        }
        if (pf === 'rgba-float16') {
            // Refusing loudly beats reporting a format whose buffer would still
            // be 8-bit: the engine has no Float16Array to back it.
            throw new TypeError(ctorName + ": the 'rgba-float16' pixel format is not supported");
        }
        out.pixelFormat = pf;
    }
    return out;
}

function _lumen_is_image_data_buffer(v) {
    return typeof Uint8ClampedArray === 'function' && v instanceof Uint8ClampedArray;
}

function ImageData(arg0, arg1, arg2, arg3) {
    var C = "Failed to construct 'ImageData'";
    if (!(this instanceof ImageData)) {
        throw new TypeError(C + ": please use the 'new' operator");
    }
    if (arguments.length < 2) {
        throw new TypeError(C + ": 2 arguments required, but only " +
            arguments.length + " present");
    }
    // WebIDL overload resolution keys on argument 0: a Uint8ClampedArray picks
    // the (data, sw, sh) form, ANYTHING else falls through to (sw, sh) — which
    // is why `new ImageData(new Uint8Array(100), 25)` is an IndexSizeError for
    // width 0 rather than a TypeError for the wrong buffer type.
    var data = null, w, h = null, settings;
    if (_lumen_is_image_data_buffer(arg0)) {
        data = arg0;
        w = _lumen_canvas_ulong(arg1);
        if (arg2 !== undefined) { h = _lumen_canvas_ulong(arg2); }
        settings = arg3;
    } else {
        w = _lumen_canvas_ulong(arg0);
        h = _lumen_canvas_ulong(arg1);
        settings = arg2;
    }
    var opts = _lumen_image_data_settings(settings, C);
    if (data !== null) {
        if (data.length % 4 !== 0) {
            throw new DOMException(
                C + ": the source data length is not a multiple of 4", 'InvalidStateError');
        }
        var rows = data.length / 4;
        if (w === 0 || rows % w !== 0) {
            throw new DOMException(
                C + ": the source data length is not a multiple of (4 * width)", 'IndexSizeError');
        }
        var derived = rows / w;
        if (h === null) {
            h = derived;
        } else if (h === 0 || h !== derived) {
            throw new DOMException(
                C + ": the source data length does not match the given dimensions", 'IndexSizeError');
        }
    } else {
        if (w === 0 || h === 0) {
            throw new DOMException(
                C + ": the source width and height must be non-zero", 'IndexSizeError');
        }
        if (w * h > _LUMEN_IMAGE_DATA_MAX_PIXELS) {
            throw new DOMException(
                C + ": the requested image data is too large", 'IndexSizeError');
        }
        data = new Uint8ClampedArray(w * h * 4);
    }
    _lumen_slot(this, '__image_data__', {
        width: w, height: h, data: data,
        colorSpace: opts.colorSpace, pixelFormat: opts.pixelFormat,
    });
}

function _lumen_image_data_slot(v, member) {
    if (!v || v.__image_data__ === undefined) {
        throw new TypeError("Failed to read the '" + member +
            "' property from 'ImageData': receiver is not an ImageData");
    }
    return v.__image_data__;
}

// `width`/`height`/`data` are readonly (`2d.imageData.object.readonly`), so
// they are getters with no setter — a page assigning to them is a silent no-op
// in sloppy mode, exactly as in a spec browser.
Object.defineProperty(ImageData.prototype, 'width', {
    get: function() { return _lumen_image_data_slot(this, 'width').width; },
    enumerable: true, configurable: true,
});
Object.defineProperty(ImageData.prototype, 'height', {
    get: function() { return _lumen_image_data_slot(this, 'height').height; },
    enumerable: true, configurable: true,
});
Object.defineProperty(ImageData.prototype, 'data', {
    get: function() { return _lumen_image_data_slot(this, 'data').data; },
    enumerable: true, configurable: true,
});
Object.defineProperty(ImageData.prototype, 'colorSpace', {
    get: function() { return _lumen_image_data_slot(this, 'colorSpace').colorSpace; },
    enumerable: true, configurable: true,
});
Object.defineProperty(ImageData.prototype, 'pixelFormat', {
    get: function() { return _lumen_image_data_slot(this, 'pixelFormat').pixelFormat; },
    enumerable: true, configurable: true,
});
_lumen_idl_tag(ImageData, 'ImageData');

// Builds the ImageData returned by getImageData/createImageData. `bytes` is
// the native RGBA8 payload (a plain array) or null for a blank one; a payload
// of the wrong length means the native could not serve the request, and the
// spec's answer there is transparent black, not a short buffer.
function _lumen_make_image_data(w, h, bytes) {
    var img = new ImageData(w, h);
    if (bytes && bytes.length === img.data.length) { img.data.set(bytes); }
    return img;
}

// ── CanvasGradient / CanvasPattern (HTML LS §4.12.5.1.4) ─────────────────────

function CanvasGradient() {
    throw new TypeError("Illegal constructor");
}
CanvasGradient.prototype.addColorStop = function(offset, color) {
    if (!this || this.__gid__ === undefined) {
        throw new TypeError("Failed to execute 'addColorStop' on 'CanvasGradient': " +
            "receiver is not a CanvasGradient");
    }
    var o = Number(offset);
    if (!isFinite(o)) {
        throw new TypeError("addColorStop: offset is not a finite number");
    }
    if (o < 0 || o > 1) {
        throw new DOMException("addColorStop: offset is outside [0, 1]", 'IndexSizeError');
    }
    _lumen_canvas2d_gradient_add_color_stop(this.__gid__, o, String(color));
};
_lumen_idl_tag(CanvasGradient, 'CanvasGradient');
_lumen_idl_tag(Path2D, 'Path2D');

function _lumen_make_canvas_gradient(gid) {
    var g = Object.create(CanvasGradient.prototype);
    _lumen_slot(g, '__gid__', gid);
    return g;
}

function CanvasPattern() {
    throw new TypeError("Illegal constructor");
}
// The transform is stored and reported but not yet applied by the rasterizer —
// the native pattern has no matrix slot. Kept because a page feature-detects
// the method, and because dropping the argument silently is what a stub does.
CanvasPattern.prototype.setTransform = function(transform) {
    if (!this || this.__patid__ === undefined) {
        throw new TypeError("Failed to execute 'setTransform' on 'CanvasPattern': " +
            "receiver is not a CanvasPattern");
    }
    var t = transform;
    if (t === undefined || t === null) {
        this.__pattern_transform__ = [1, 0, 0, 1, 0, 0];
        return;
    }
    if (typeof t !== 'object') {
        throw new TypeError("setTransform: argument is not a DOMMatrix2DInit");
    }
    this.__pattern_transform__ = [
        t.a === undefined ? (t.m11 === undefined ? 1 : +t.m11) : +t.a,
        t.b === undefined ? (t.m12 === undefined ? 0 : +t.m12) : +t.b,
        t.c === undefined ? (t.m21 === undefined ? 0 : +t.m21) : +t.c,
        t.d === undefined ? (t.m22 === undefined ? 1 : +t.m22) : +t.d,
        t.e === undefined ? (t.m41 === undefined ? 0 : +t.m41) : +t.e,
        t.f === undefined ? (t.m42 === undefined ? 0 : +t.m42) : +t.f,
    ];
};
_lumen_idl_tag(CanvasPattern, 'CanvasPattern');

function _lumen_make_canvas_pattern(patid) {
    var p = Object.create(CanvasPattern.prototype);
    _lumen_slot(p, '__patid__', patid);
    _lumen_slot(p, '__pattern_transform__', [1, 0, 0, 1, 0, 0]);
    return p;
}

// ── TextMetrics (HTML LS §4.12.5.1.13) ───────────────────────────────────────

function TextMetrics() {
    throw new TypeError("Illegal constructor");
}
_lumen_idl_tag(TextMetrics, 'TextMetrics');

// One accessor per IDL attribute, all reading the same measurement record, so
// a page that walks the prototype sees the interface rather than a bag of own
// data properties.
(function() {
    var names = ['width', 'actualBoundingBoxLeft', 'actualBoundingBoxRight',
        'actualBoundingBoxAscent', 'actualBoundingBoxDescent',
        'fontBoundingBoxAscent', 'fontBoundingBoxDescent',
        'emHeightAscent', 'emHeightDescent',
        'hangingBaseline', 'alphabeticBaseline', 'ideographicBaseline'];
    for (var i = 0; i < names.length; i++) {
        (function(name) {
            Object.defineProperty(TextMetrics.prototype, name, {
                get: function() {
                    if (!this || this.__text_metrics__ === undefined) {
                        throw new TypeError("Failed to read the '" + name +
                            "' property from 'TextMetrics': receiver is not a TextMetrics");
                    }
                    return this.__text_metrics__[name];
                },
                enumerable: true, configurable: true,
            });
        })(names[i]);
    }
})();

function _lumen_make_text_metrics(values) {
    var tm = Object.create(TextMetrics.prototype);
    _lumen_slot(tm, '__text_metrics__', values);
    return tm;
}

// ── CanvasRenderingContext2D (HTML LS §4.12.5.1) ─────────────────────────────

function CanvasRenderingContext2D() {
    throw new TypeError("Illegal constructor");
}
_lumen_idl_tag(CanvasRenderingContext2D, 'CanvasRenderingContext2D');

// Every member below starts here: the state slot doubles as the brand check.
function _lumen_c2d(v, member) {
    if (!v || v.__canvas2d__ === undefined) {
        throw new TypeError("Failed to execute '" + member +
            "' on 'CanvasRenderingContext2D': receiver is not a CanvasRenderingContext2D");
    }
    return v.__canvas2d__;
}

function _lumen_c2d_method(name, fn) {
    Object.defineProperty(CanvasRenderingContext2D.prototype, name, {
        value: fn, writable: true, enumerable: true, configurable: true,
    });
}

// A plain state property: getter reads the record, setter validates and pushes
// the value into the native context.
function _lumen_c2d_prop(name, apply, coerce) {
    Object.defineProperty(CanvasRenderingContext2D.prototype, name, {
        get: function() { return _lumen_c2d(this, name)[name]; },
        set: function(v) {
            var st = _lumen_c2d(this, name);
            var next = coerce(v, st[name]);
            if (next === undefined) { return; }
            st[name] = next;
            if (apply) { apply(st.nid, next); }
        },
        enumerable: true, configurable: true,
    });
}

function _lumen_c2d_paint_style(name, setColor, setGradient, setPattern) {
    Object.defineProperty(CanvasRenderingContext2D.prototype, name, {
        get: function() { return _lumen_c2d(this, name)[name]; },
        set: function(v) {
            var st = _lumen_c2d(this, name);
            if (v && typeof v === 'object' && v.__gid__ !== undefined) {
                st[name] = v; setGradient(st.nid, v.__gid__);
            } else if (v && typeof v === 'object' && v.__patid__ !== undefined) {
                st[name] = v; setPattern(st.nid, v.__patid__);
            } else {
                // HTML LS §4.12.5.1.3: строка разбирается как CSS <color>;
                // невалидная ИГНОРИРУЕТСЯ (атрибут сохраняет прежнее
                // значение), валидная хранится в канонической сериализации —
                // её и возвращает натив (BUG-451). Раньше здесь оседала сырая
                // строка, поэтому геттер отдавал '#0F0' и даже 'not-a-color'.
                var ser = setColor(st.nid, String(v));
                if (ser === null || ser === undefined) { return; }
                st[name] = ser;
            }
        },
        enumerable: true, configurable: true,
    });
}

// `shadowColor` — тот же контракт «разобрать / игнорировать невалидное /
// хранить канонически», но без градиентов и паттернов: по §4.12.5.1.3 это
// только <color>.
function _lumen_c2d_color_prop(name, setColor) {
    Object.defineProperty(CanvasRenderingContext2D.prototype, name, {
        get: function() { return _lumen_c2d(this, name)[name]; },
        set: function(v) {
            var st = _lumen_c2d(this, name);
            var ser = setColor(st.nid, String(v));
            if (ser === null || ser === undefined) { return; }
            st[name] = ser;
        },
        enumerable: true, configurable: true,
    });
}

Object.defineProperty(CanvasRenderingContext2D.prototype, 'canvas', {
    get: function() { return _lumen_c2d(this, 'canvas').canvas; },
    enumerable: true, configurable: true,
});

_lumen_c2d_paint_style('fillStyle',
    function(nid, v) { return _lumen_canvas2d_set_fill_style(nid, v); },
    function(nid, g) { _lumen_canvas2d_set_fill_style_gradient(nid, g); },
    function(nid, p) { _lumen_canvas2d_set_fill_style_pattern(nid, p); });
_lumen_c2d_paint_style('strokeStyle',
    function(nid, v) { return _lumen_canvas2d_set_stroke_style(nid, v); },
    function(nid, g) { _lumen_canvas2d_set_stroke_style_gradient(nid, g); },
    function(nid, p) { _lumen_canvas2d_set_stroke_style_pattern(nid, p); });

_lumen_c2d_prop('lineWidth', function(nid, v) { _lumen_canvas2d_set_line_width(nid, v); },
    function(v) { var n = Number(v); return (isFinite(n) && n > 0) ? n : undefined; });
_lumen_c2d_prop('globalAlpha', function(nid, v) { _lumen_canvas2d_set_global_alpha(nid, v); },
    function(v) { var n = Number(v); return (isFinite(n) && n >= 0 && n <= 1) ? n : undefined; });
_lumen_c2d_prop('globalCompositeOperation',
    function(nid, v) { _lumen_canvas2d_set_global_composite_operation(nid, v); },
    function(v) { return String(v); });
_lumen_c2d_prop('lineCap', function(nid, v) { _lumen_canvas2d_set_line_cap(nid, v); },
    function(v) { return String(v); });
_lumen_c2d_prop('lineJoin', function(nid, v) { _lumen_canvas2d_set_line_join(nid, v); },
    function(v) { return String(v); });
_lumen_c2d_prop('miterLimit', function(nid, v) { _lumen_canvas2d_set_miter_limit(nid, v); },
    function(v) { var n = Number(v); return (isFinite(n) && n > 0) ? n : undefined; });
_lumen_c2d_color_prop('shadowColor',
    function(nid, v) { return _lumen_canvas2d_set_shadow_color(nid, v); });
_lumen_c2d_prop('shadowBlur', function(nid, v) { _lumen_canvas2d_set_shadow_blur(nid, v); },
    function(v) { var n = Number(v); return (isFinite(n) && n >= 0) ? n : undefined; });
_lumen_c2d_prop('shadowOffsetX', function(nid, v) { _lumen_canvas2d_set_shadow_offset_x(nid, v); },
    function(v) { var n = Number(v); return isFinite(n) ? n : undefined; });
_lumen_c2d_prop('shadowOffsetY', function(nid, v) { _lumen_canvas2d_set_shadow_offset_y(nid, v); },
    function(v) { var n = Number(v); return isFinite(n) ? n : undefined; });
_lumen_c2d_prop('font', function(nid, v) { _lumen_canvas2d_set_font(nid, v); },
    function(v) { return String(v); });
_lumen_c2d_prop('textAlign', function(nid, v) { _lumen_canvas2d_set_text_align(nid, v); },
    function(v) { return String(v); });
_lumen_c2d_prop('textBaseline', function(nid, v) { _lumen_canvas2d_set_text_baseline(nid, v); },
    function(v) { return String(v); });
// Accepted, stored and reported, but not yet consulted by the rasterizer.
_lumen_c2d_prop('direction', null, function(v) { return String(v); });
_lumen_c2d_prop('lineDashOffset', null,
    function(v) { var n = Number(v); return isFinite(n) ? n : undefined; });
_lumen_c2d_prop('imageSmoothingEnabled', null, function(v) { return !!v; });
_lumen_c2d_prop('filter', null, function(v) { return String(v); });

// Rect operations
_lumen_c2d_method('fillRect', function(x, y, w, h) {
    _lumen_canvas2d_fill_rect(_lumen_c2d(this, 'fillRect').nid, +x, +y, +w, +h);
});
_lumen_c2d_method('clearRect', function(x, y, w, h) {
    _lumen_canvas2d_clear_rect(_lumen_c2d(this, 'clearRect').nid, +x, +y, +w, +h);
});
_lumen_c2d_method('strokeRect', function(x, y, w, h) {
    _lumen_canvas2d_stroke_rect(_lumen_c2d(this, 'strokeRect').nid, +x, +y, +w, +h);
});
// Path operations
_lumen_c2d_method('beginPath', function() {
    _lumen_canvas2d_begin_path(_lumen_c2d(this, 'beginPath').nid);
});
_lumen_c2d_method('moveTo', function(x, y) {
    _lumen_canvas2d_move_to(_lumen_c2d(this, 'moveTo').nid, +x, +y);
});
_lumen_c2d_method('lineTo', function(x, y) {
    _lumen_canvas2d_line_to(_lumen_c2d(this, 'lineTo').nid, +x, +y);
});
_lumen_c2d_method('closePath', function() {
    _lumen_canvas2d_close_path(_lumen_c2d(this, 'closePath').nid);
});
_lumen_c2d_method('arc', function(cx, cy, r, sa, ea, ccw) {
    _lumen_canvas2d_arc(_lumen_c2d(this, 'arc').nid, +cx, +cy, +r, +sa, +ea, !!ccw);
});
_lumen_c2d_method('ellipse', function(cx, cy, rx, ry, rot, sa, ea, ccw) {
    // Implemented via transforms: save → translate(cx,cy) → rotate(rot) →
    // scale(rx,ry) → arc(0,0,1,sa,ea,ccw) → restore.
    var nid = _lumen_c2d(this, 'ellipse').nid;
    _lumen_canvas2d_save(nid);
    _lumen_canvas2d_translate(nid, +cx, +cy);
    if (+rot !== 0) { _lumen_canvas2d_rotate(nid, +rot); }
    _lumen_canvas2d_scale(nid, +rx, +ry);
    _lumen_canvas2d_arc(nid, 0, 0, 1, +sa, +ea, !!ccw);
    _lumen_canvas2d_restore(nid);
});
_lumen_c2d_method('arcTo', function(x1, y1, x2, y2, r) {
    _lumen_canvas2d_arc_to(_lumen_c2d(this, 'arcTo').nid, +x1, +y1, +x2, +y2, +r);
});
_lumen_c2d_method('rect', function(x, y, w, h) {
    _lumen_canvas2d_rect(_lumen_c2d(this, 'rect').nid, +x, +y, +w, +h);
});
_lumen_c2d_method('bezierCurveTo', function(cp1x, cp1y, cp2x, cp2y, x, y) {
    _lumen_canvas2d_bezier_curve_to(_lumen_c2d(this, 'bezierCurveTo').nid,
        +cp1x, +cp1y, +cp2x, +cp2y, +x, +y);
});
_lumen_c2d_method('quadraticCurveTo', function(cpx, cpy, x, y) {
    _lumen_canvas2d_quadratic_curve_to(_lumen_c2d(this, 'quadraticCurveTo').nid,
        +cpx, +cpy, +x, +y);
});
_lumen_c2d_method('fill', function(ruleOrPath) {
    var nid = _lumen_c2d(this, 'fill').nid;
    if (ruleOrPath instanceof Path2D) {
        _lumen_canvas2d_fill_path(nid, ruleOrPath.__pid__);
    } else {
        _lumen_canvas2d_fill(nid);
    }
});
_lumen_c2d_method('stroke', function(path) {
    var nid = _lumen_c2d(this, 'stroke').nid;
    if (path instanceof Path2D) {
        _lumen_canvas2d_stroke_path(nid, path.__pid__);
    } else {
        _lumen_canvas2d_stroke(nid);
    }
});
_lumen_c2d_method('clip', function(path) {
    var nid = _lumen_c2d(this, 'clip').nid;
    if (path instanceof Path2D) {
        _lumen_canvas2d_clip_path(nid, path.__pid__);
    } else {
        _lumen_canvas2d_clip(nid);
    }
});
// State stack
_lumen_c2d_method('save', function() {
    _lumen_canvas2d_save(_lumen_c2d(this, 'save').nid);
});
_lumen_c2d_method('restore', function() {
    _lumen_canvas2d_restore(_lumen_c2d(this, 'restore').nid);
});
// Transforms
_lumen_c2d_method('translate', function(tx, ty) {
    _lumen_canvas2d_translate(_lumen_c2d(this, 'translate').nid, +tx, +ty);
});
_lumen_c2d_method('rotate', function(angle) {
    _lumen_canvas2d_rotate(_lumen_c2d(this, 'rotate').nid, +angle);
});
_lumen_c2d_method('scale', function(sx, sy) {
    _lumen_canvas2d_scale(_lumen_c2d(this, 'scale').nid, +sx, +sy);
});
_lumen_c2d_method('transform', function(a, b, c, d, e, f) {
    _lumen_canvas2d_transform(_lumen_c2d(this, 'transform').nid, +a, +b, +c, +d, +e, +f);
});
_lumen_c2d_method('setTransform', function(a, b, c, d, e, f) {
    // `setTransform()` with no arguments resets to the identity matrix
    // (§4.12.5.1.6); passing the six undefineds through as `+undefined` sent
    // six NaNs into the native and lost the transform for good.
    var nid = _lumen_c2d(this, 'setTransform').nid;
    if (arguments.length === 0) {
        _lumen_canvas2d_set_transform(nid, 1, 0, 0, 1, 0, 0);
        return;
    }
    if (arguments.length === 1) {
        var m = a;
        if (m === null || typeof m !== 'object') {
            throw new TypeError("setTransform: argument is not a DOMMatrix2DInit");
        }
        _lumen_canvas2d_set_transform(nid,
            m.a === undefined ? 1 : +m.a, m.b === undefined ? 0 : +m.b,
            m.c === undefined ? 0 : +m.c, m.d === undefined ? 1 : +m.d,
            m.e === undefined ? 0 : +m.e, m.f === undefined ? 0 : +m.f);
        return;
    }
    _lumen_canvas2d_set_transform(nid, +a, +b, +c, +d, +e, +f);
});
_lumen_c2d_method('resetTransform', function() {
    _lumen_canvas2d_reset_transform(_lumen_c2d(this, 'resetTransform').nid);
});
// Pixel manipulation
_lumen_c2d_method('getImageData', function(sx, sy, sw, sh) {
    var nid = _lumen_c2d(this, 'getImageData').nid;
    if (arguments.length < 4) {
        throw new TypeError("getImageData: 4 arguments required, but only " +
            arguments.length + " present");
    }
    var x = _lumen_canvas_long(sx, 'getImageData', 'sx');
    var y = _lumen_canvas_long(sy, 'getImageData', 'sy');
    var w = _lumen_canvas_long(sw, 'getImageData', 'sw');
    var h = _lumen_canvas_long(sh, 'getImageData', 'sh');
    if (w === 0 || h === 0) {
        throw new DOMException(
            'The source width and height must be non-zero', 'IndexSizeError');
    }
    var r = _lumen_canvas_normalize_rect(x, y, w, h);
    return _lumen_make_image_data(r.w, r.h,
        _lumen_canvas2d_get_image_data(nid, r.x, r.y, r.w, r.h));
});
_lumen_c2d_method('putImageData', function(imageData, dx, dy, dirtyX, dirtyY, dirtyWidth, dirtyHeight) {
    var nid = _lumen_c2d(this, 'putImageData').nid;
    if (!imageData || imageData.__image_data__ === undefined) {
        throw new TypeError("putImageData: argument 1 is not an ImageData");
    }
    var sw = imageData.width | 0, sh = imageData.height | 0;
    var x = _lumen_canvas_long(dx, 'putImageData', 'dx');
    var y = _lumen_canvas_long(dy, 'putImageData', 'dy');
    // The whole source is the default dirty rectangle; the 7-argument
    // form narrows it (canvas §4.12.5.1.10) and used to be dropped on
    // the floor here (BUG-448).
    var dr = { x: 0, y: 0, w: sw, h: sh };
    if (arguments.length > 3) {
        dr = _lumen_canvas_normalize_rect(
            _lumen_canvas_long(dirtyX, 'putImageData', 'dirtyX'),
            _lumen_canvas_long(dirtyY, 'putImageData', 'dirtyY'),
            _lumen_canvas_long(dirtyWidth, 'putImageData', 'dirtyWidth'),
            _lumen_canvas_long(dirtyHeight, 'putImageData', 'dirtyHeight'));
    }
    // Clip the dirty rectangle to the source; only that part crosses
    // the binding, so a small dirty rect costs a small payload.
    if (dr.x < 0) { dr.w += dr.x; dr.x = 0; }
    if (dr.y < 0) { dr.h += dr.y; dr.y = 0; }
    if (dr.x + dr.w > sw) { dr.w = sw - dr.x; }
    if (dr.y + dr.h > sh) { dr.h = sh - dr.y; }
    if (dr.w <= 0 || dr.h <= 0) { return; }
    var d = imageData.data;
    var H = '0123456789abcdef', hex = '';
    for (var row = 0; row < dr.h; row++) {
        var base = ((dr.y + row) * sw + dr.x) * 4;
        for (var i = 0; i < dr.w * 4; i++) {
            var b = d[base + i] & 255;
            hex += H[b >> 4] + H[b & 15];
        }
    }
    _lumen_canvas2d_put_image_data(nid, hex, dr.w, dr.h, x + dr.x, y + dr.y);
});
_lumen_c2d_method('createImageData', function(w, h) {
    // Two overloads: (sw, sh) and the copy form (imageData), which
    // takes the argument's dimensions and returns transparent black.
    // The copy form used to fall through to `0|0` and hand back a 0×0
    // buffer (BUG-448).
    _lumen_c2d(this, 'createImageData');
    if (arguments.length === 1) {
        if (!w || w.__image_data__ === undefined) {
            throw new TypeError("createImageData: argument 1 is not an ImageData");
        }
        return _lumen_make_image_data(w.width | 0, w.height | 0, null);
    }
    var sw = _lumen_canvas_long(w, 'createImageData', 'sw');
    var sh = _lumen_canvas_long(h, 'createImageData', 'sh');
    if (sw === 0 || sh === 0) {
        throw new DOMException(
            'The source width and height must be non-zero', 'IndexSizeError');
    }
    return _lumen_make_image_data(Math.abs(sw), Math.abs(sh), null);
});
// drawImage forms: (src,dx,dy) | (src,dx,dy,dw,dh) | (src,sx,sy,sw,sh,dx,dy,dw,dh).
// Source may be a <canvas>/OffscreenCanvas (canvas bitmap store, via __nid__)
// or a decoded <img> element (img_bitmap_store, via __nid__ + tag=img).
_lumen_c2d_method('drawImage', function(image, a, b, c, d, e, f, g, h) {
    var nid = _lumen_c2d(this, 'drawImage').nid;
    if (!image || image.__nid__ === undefined) { return; }
    var src = image.__nid__;
    var isImg = (_lumen_get_tag_name(src) === 'IMG');
    var iw = +image.width || 0, ih = +image.height || 0;
    if (arguments.length >= 9) {
        var sx = +a, sy = +b, sw = +c, sh = +d;
        var dx9 = +e, dy9 = +f, dw9 = +g, dh9 = +h;
        if (!(sw > 0) || !(sh > 0) || !(dw9 > 0) || !(dh9 > 0)) { return; }
        var coords = sx + ',' + sy + ',' + sw + ',' + sh + ',' + dx9 + ',' + dy9 + ',' + dw9 + ',' + dh9;
        if (isImg) { _lumen_canvas2d_draw_image_crop_from_img(nid, src, coords); }
        else { _lumen_canvas2d_draw_image_crop(nid, src, coords); }
        return;
    }
    var dx, dy, dw, dh;
    if (arguments.length >= 5) {
        dx = +a; dy = +b; dw = +c; dh = +d;
        if (!(dw > 0) || !(dh > 0)) { return; }
        if (isImg) { _lumen_canvas2d_draw_image_from_img(nid, src, dx, dy, dw, dh); }
        else { _lumen_canvas2d_draw_image(nid, src, dx, dy, dw, dh); }
    } else {
        dx = +a; dy = +b;
        if (isImg) {
            // 3-arg form: pass dw/dh=0 so the native uses the image's natural size.
            _lumen_canvas2d_draw_image_from_img(nid, src, dx, dy, 0, 0);
        } else {
            if (!(iw > 0) || !(ih > 0)) { return; }
            _lumen_canvas2d_draw_image(nid, src, dx, dy, iw, ih);
        }
    }
});
// Text
_lumen_c2d_method('fillText', function(t, x, y) {
    _lumen_canvas2d_fill_text(_lumen_c2d(this, 'fillText').nid,
        String(t == null ? '' : t), +x, +y);
});
_lumen_c2d_method('strokeText', function(t, x, y) {
    _lumen_canvas2d_stroke_text(_lumen_c2d(this, 'strokeText').nid,
        String(t == null ? '' : t), +x, +y);
});
_lumen_c2d_method('measureText', function(t) {
    var st = _lumen_c2d(this, 'measureText');
    var s = String(t == null ? '' : t);
    // The twelve numbers come from the font itself (glyph boxes for the ink
    // extents, `hhea` for the vertical ones), in the IDL order the native
    // documents; the fallback keeps the three the shim used to report at all.
    var m = _lumen_canvas2d_text_metrics(st.nid, s);
    if (!m || m.length !== 12) {
        var fs = _lumen_canvas_font_px(st.font);
        var w = _lumen_canvas2d_measure_text(st.nid, s);
        m = [w, 0, w, fs * 0.8, fs * 0.2, fs * 0.8, fs * 0.2,
             fs * 0.8, fs * 0.2, fs * 0.8, 0, -fs * 0.2];
    }
    return _lumen_make_text_metrics({
        width: m[0],
        actualBoundingBoxLeft: m[1],
        actualBoundingBoxRight: m[2],
        actualBoundingBoxAscent: m[3],
        actualBoundingBoxDescent: m[4],
        fontBoundingBoxAscent: m[5],
        fontBoundingBoxDescent: m[6],
        emHeightAscent: m[7],
        emHeightDescent: m[8],
        hangingBaseline: m[9],
        alphabeticBaseline: m[10],
        ideographicBaseline: m[11],
    });
});
// Line dash
_lumen_c2d_method('setLineDash', function() { _lumen_c2d(this, 'setLineDash'); });
_lumen_c2d_method('getLineDash', function() { _lumen_c2d(this, 'getLineDash'); return []; });
// Hit testing
_lumen_c2d_method('isPointInPath', function(pathOrX, xOrY, y) {
    var nid = _lumen_c2d(this, 'isPointInPath').nid;
    if (pathOrX instanceof Path2D) {
        return _lumen_canvas2d_is_point_in_path(nid, pathOrX.__pid__, +xOrY, +y);
    }
    return false;
});
_lumen_c2d_method('isPointInStroke', function() {
    _lumen_c2d(this, 'isPointInStroke');
    return false;
});
// Gradients and patterns
_lumen_c2d_method('createLinearGradient', function(x0, y0, x1, y1) {
    return _lumen_make_canvas_gradient(_lumen_canvas2d_create_linear_gradient(
        _lumen_c2d(this, 'createLinearGradient').nid, +x0, +y0, +x1, +y1));
});
_lumen_c2d_method('createRadialGradient', function(x0, y0, r0, x1, y1, r1) {
    return _lumen_make_canvas_gradient(_lumen_canvas2d_create_radial_gradient(
        _lumen_c2d(this, 'createRadialGradient').nid, +x0, +y0, +r0, +x1, +y1, +r1));
});
_lumen_c2d_method('createConicGradient', function(angle, cx, cy) {
    return _lumen_make_canvas_gradient(_lumen_canvas2d_create_conic_gradient(
        _lumen_c2d(this, 'createConicGradient').nid, +angle, +cx, +cy));
});
_lumen_c2d_method('createPattern', function(image, repetition) {
    _lumen_c2d(this, 'createPattern');
    if (!image || image.__nid__ === undefined) { return null; }
    var rep = (repetition == null || repetition === '') ? 'repeat' : String(repetition);
    var pid = _lumen_canvas2d_create_pattern(image.__nid__, rep);
    if (!pid) { return null; }
    return _lumen_make_canvas_pattern(pid);
});

// Enumerated text-shaping state (§4.12.5.1.12). Stored and reported; the
// rasterizer does not consult them yet, so an invalid value is ignored exactly
// as the spec asks rather than rejected.
function _lumen_c2d_enum_prop(name, allowed) {
    _lumen_c2d_prop(name, null, function(v) {
        var s = String(v);
        return allowed.indexOf(s) === -1 ? undefined : s;
    });
}
_lumen_c2d_enum_prop('imageSmoothingQuality', ['low', 'medium', 'high']);
_lumen_c2d_enum_prop('fontKerning', ['auto', 'normal', 'none']);
_lumen_c2d_enum_prop('fontStretch', ['ultra-condensed', 'extra-condensed', 'condensed',
    'semi-condensed', 'normal', 'semi-expanded', 'expanded', 'extra-expanded', 'ultra-expanded']);
_lumen_c2d_enum_prop('fontVariantCaps', ['normal', 'small-caps', 'all-small-caps', 'petite-caps',
    'all-petite-caps', 'unicase', 'titling-caps']);
_lumen_c2d_enum_prop('textRendering', ['auto', 'optimizeSpeed', 'optimizeLegibility',
    'geometricPrecision']);
// letterSpacing/wordSpacing take a CSS <length>; anything else is ignored.
function _lumen_c2d_length_prop(name) {
    _lumen_c2d_prop(name, null, function(v) {
        var s = String(v);
        return /^[+-]?(\d+\.?\d*|\.\d+)(px|em|rem|ex|ch|vw|vh|vmin|vmax|cm|mm|in|pt|pc|q)$/i.test(s)
            ? s : undefined;
    });
}
_lumen_c2d_length_prop('letterSpacing');
_lumen_c2d_length_prop('wordSpacing');

// The context is never lost and never opaque: there is one software bitmap per
// canvas and nothing can take it away (§4.12.5.1.1).
_lumen_c2d_method('isContextLost', function() {
    _lumen_c2d(this, 'isContextLost');
    return false;
});
_lumen_c2d_method('getContextAttributes', function() {
    _lumen_c2d(this, 'getContextAttributes');
    return { alpha: true, colorSpace: 'srgb', desynchronized: false, willReadFrequently: false };
});

// §4.12.5.1.2 reset: the bitmap goes transparent black, the state stack is
// emptied and every attribute returns to its initial value.
_lumen_c2d_method('reset', function() {
    var st = _lumen_c2d(this, 'reset');
    var d = _lumen_canvas_dims(st.nid);
    _lumen_canvas2d_reset_transform(st.nid);
    _lumen_canvas2d_clear_rect(st.nid, 0, 0, d[0], d[1]);
    _lumen_canvas2d_begin_path(st.nid);
    var defaults = _lumen_canvas2d_default_state(st.canvas, st.nid);
    for (var k in defaults) {
        if (k === 'nid' || k === 'canvas') { continue; }
        st[k] = defaults[k];
    }
    _lumen_canvas2d_set_fill_style(st.nid, st.fillStyle);
    _lumen_canvas2d_set_stroke_style(st.nid, st.strokeStyle);
    _lumen_canvas2d_set_line_width(st.nid, st.lineWidth);
    _lumen_canvas2d_set_global_alpha(st.nid, st.globalAlpha);
    _lumen_canvas2d_set_global_composite_operation(st.nid, st.globalCompositeOperation);
    _lumen_canvas2d_set_line_cap(st.nid, st.lineCap);
    _lumen_canvas2d_set_line_join(st.nid, st.lineJoin);
    _lumen_canvas2d_set_miter_limit(st.nid, st.miterLimit);
    _lumen_canvas2d_set_shadow_color(st.nid, st.shadowColor);
    _lumen_canvas2d_set_shadow_blur(st.nid, st.shadowBlur);
    _lumen_canvas2d_set_shadow_offset_x(st.nid, st.shadowOffsetX);
    _lumen_canvas2d_set_shadow_offset_y(st.nid, st.shadowOffsetY);
    _lumen_canvas2d_set_font(st.nid, st.font);
    _lumen_canvas2d_set_text_align(st.nid, st.textAlign);
    _lumen_canvas2d_set_text_baseline(st.nid, st.textBaseline);
});

// §4.12.5.1.7 roundRect. `radii` is one radius or a list of up to four, each a
// number or a `{x, y}` (DOMPointInit); the corners are scaled down together
// when they would overlap, so no arc can cross the rectangle's own edge.
function _lumen_corner_radius(v, method) {
    if (v !== null && typeof v === 'object') {
        var rx = v.x === undefined ? 0 : Number(v.x);
        var ry = v.y === undefined ? 0 : Number(v.y);
        if (!isFinite(rx) || !isFinite(ry)) {
            throw new TypeError(method + ": a radius is not a finite number");
        }
        if (rx < 0 || ry < 0) {
            throw new DOMException(method + ": a radius is negative", 'IndexSizeError');
        }
        return [rx, ry];
    }
    var r = Number(v);
    if (!isFinite(r)) {
        throw new TypeError(method + ": a radius is not a finite number");
    }
    if (r < 0) {
        throw new DOMException(method + ": a radius is negative", 'IndexSizeError');
    }
    return [r, r];
}
_lumen_c2d_method('roundRect', function(x, y, w, h, radii) {
    var st = _lumen_c2d(this, 'roundRect');
    var nid = st.nid;
    var X = +x, Y = +y, W = +w, H = +h;
    if (!isFinite(X) || !isFinite(Y) || !isFinite(W) || !isFinite(H)) { return; }
    if (radii === undefined) { radii = 0; }
    var list = (radii !== null && typeof radii === 'object' && typeof radii.length === 'number')
        ? Array.prototype.slice.call(radii) : [radii];
    if (list.length < 1 || list.length > 4) {
        throw new RangeError("roundRect: radii must hold between one and four radii");
    }
    var r = [];
    for (var i = 0; i < list.length; i++) { r.push(_lumen_corner_radius(list[i], 'roundRect')); }
    // upperLeft, upperRight, lowerRight, lowerLeft — the CSS corner shorthand.
    var ul, ur, lr, ll;
    if (r.length === 1) { ul = ur = lr = ll = r[0]; }
    else if (r.length === 2) { ul = lr = r[0]; ur = ll = r[1]; }
    else if (r.length === 3) { ul = r[0]; ur = ll = r[1]; lr = r[2]; }
    else { ul = r[0]; ur = r[1]; lr = r[2]; ll = r[3]; }
    // A negative extent mirrors the rectangle, and the corners swap with it.
    if (W < 0) { X += W; W = -W; var sw1 = ul; ul = ur; ur = sw1; var sw2 = ll; ll = lr; lr = sw2; }
    if (H < 0) { Y += H; H = -H; var sh1 = ul; ul = ll; ll = sh1; var sh2 = ur; ur = lr; lr = sh2; }
    var scale = Math.min(
        H / (ul[1] + ll[1]), W / (ul[0] + ur[0]),
        H / (ur[1] + lr[1]), W / (ll[0] + lr[0]));
    if (isFinite(scale) && scale < 1) {
        ul = [ul[0] * scale, ul[1] * scale]; ur = [ur[0] * scale, ur[1] * scale];
        lr = [lr[0] * scale, lr[1] * scale]; ll = [ll[0] * scale, ll[1] * scale];
    }
    // Each corner is a quarter ellipse; drawn as an arc in a scaled user space,
    // the way `ellipse` already is, so the path stays one connected subpath.
    function corner(cx, cy, rx, ry, start, end) {
        if (rx <= 0 || ry <= 0) { _lumen_canvas2d_line_to(nid, cx, cy); return; }
        _lumen_canvas2d_save(nid);
        _lumen_canvas2d_translate(nid, cx, cy);
        _lumen_canvas2d_scale(nid, rx, ry);
        _lumen_canvas2d_arc(nid, 0, 0, 1, start, end, false);
        _lumen_canvas2d_restore(nid);
    }
    var HALF = Math.PI / 2;
    _lumen_canvas2d_move_to(nid, X + ul[0], Y);
    _lumen_canvas2d_line_to(nid, X + W - ur[0], Y);
    corner(X + W - ur[0], Y + ur[1], ur[0], ur[1], -HALF, 0);
    _lumen_canvas2d_line_to(nid, X + W, Y + H - lr[1]);
    corner(X + W - lr[0], Y + H - lr[1], lr[0], lr[1], 0, HALF);
    _lumen_canvas2d_line_to(nid, X + ll[0], Y + H);
    corner(X + ll[0], Y + H - ll[1], ll[0], ll[1], HALF, Math.PI);
    _lumen_canvas2d_line_to(nid, X, Y + ul[1]);
    corner(X + ul[0], Y + ul[1], ul[0], ul[1], Math.PI, Math.PI + HALF);
    _lumen_canvas2d_close_path(nid);
});

// Parses the px size out of a canvas `font` string, for the TextMetrics
// ascent/descent approximation.
function _lumen_canvas_font_px(f) {
    var parts = String(f).split(' ');
    for (var i = 0; i < parts.length; i++) {
        if (parts[i].indexOf('px') !== -1) {
            var n = parseFloat(parts[i]);
            if (n > 0) return n;
        }
    }
    return 10;
}

// Builds a CanvasRenderingContext2D backed by the native _lumen_canvas2d_*
// bindings (lumen_canvas::Context2D), keyed by the canvas element's node index
// `nid`. Drawing methods forward to the native rasterizer; the shell uploads
// the pixel buffer to the renderer under `canvas:{nid}` each frame.
function _lumen_make_canvas2d_ctx(canvasEl, nid) {
    var ctx = Object.create(CanvasRenderingContext2D.prototype);
    _lumen_slot(ctx, '__canvas2d__', _lumen_canvas2d_default_state(canvasEl, nid));
    return ctx;
}

// The initial value of every attribute (§4.12.5.1.1), in one place because
// `reset()` has to restore exactly this set.
function _lumen_canvas2d_default_state(canvasEl, nid) {
    return {
        nid: nid,
        canvas: canvasEl,
        fillStyle: '#000000',
        strokeStyle: '#000000',
        lineWidth: 1.0,
        globalAlpha: 1.0,
        globalCompositeOperation: 'source-over',
        lineCap: 'butt',
        lineJoin: 'miter',
        miterLimit: 10,
        shadowColor: 'rgba(0, 0, 0, 0)',
        shadowBlur: 0,
        shadowOffsetX: 0,
        shadowOffsetY: 0,
        font: '10px sans-serif',
        textAlign: 'start',
        textBaseline: 'alphabetic',
        direction: 'inherit',
        lineDashOffset: 0,
        imageSmoothingEnabled: true,
        imageSmoothingQuality: 'low',
        filter: 'none',
        letterSpacing: '0px',
        wordSpacing: '0px',
        fontKerning: 'auto',
        fontStretch: 'normal',
        fontVariantCaps: 'normal',
        textRendering: 'auto',
    };
}

// Resolve a canvas element's bitmap width/height (HTML LS §4.12.4 defaults
// 300×150). The attributes reflect as `unsigned long`, so this is the §2.6.2
// getter: parse non-negative integer, and answer the default for an error or for
// anything outside 0…2147483647.
//
// BUG-452: this used to be `parseInt(aw, 10) || default`, where `||` swallowed
// the perfectly valid **0** (`<canvas width=0>` measured 300×150, and
// `canvas.width = 0; canvas.width` answered 300 while the attribute honestly
// held '0'). Three more values came out wrong through the same two lines:
// `'0x100'` parses to 0 and was likewise swallowed, `'-100'` sailed through `||`
// as a truthy −100 and the `< 1` clamps below then reported **1** rather than the
// default, and an out-of-range `'4294967291'` was returned verbatim.
//
// The `< 1` clamps are gone: a zero-size canvas is legal and simply has no
// pixels. The native backing store keeps its own `clamp(1, MAX_CANVAS_DIM)`
// (`canvas2d.rs`) so the rasterizer never sees a zero extent — that clamp is an
// allocation detail and must not be observable here, which is precisely the
// confusion these two lines encoded.
function _lumen_canvas_dims(nid) {
    return [
        _lumen_canvas_dim_attr(nid, 'width', 300),
        _lumen_canvas_dim_attr(nid, 'height', 150),
    ];
}

function _lumen_canvas_dim_attr(nid, attr, def) {
    var p = _lumen_parse_integer(_lumen_u2n(_lumen_get_attr(nid, attr)));
    if (p === null || p < 0 || p > 2147483647) return def;
    return p;
}

// ── Element factory ───────────────────────────────────────────────────────────

// BUG-291: node wrappers must be stable under `===` for repeated access to the
// same node (`tbody.lastChild === tr` etc.) — real-world JS (testharness.js's
// results renderer among it) relies on reference identity. `_lumen_build_element`
// used to run fresh on every `_lumen_make_element` call, minting a brand-new JS
// object each time; this cache interns wrappers by nid so the same node always
// yields the same object. Purged per-nid by `_lumen_gc_collect` (idle shell tick)
// so detached, zero-JS-ref nodes don't retain memory here, same lifecycle as
// `_input_values`/`_canvas2d_ctxs` below. Caching by nid for the life of the JS
// context is safe even though this covers element *and* text-node wrappers: the
// DOM node arena is append-only for the lifetime of a document
// (`crates/engine/dom/src/lib.rs` `alloc()`; no free-list reuse until a future
// Phase-3 compaction), and this whole shim is re-evaluated from scratch on
// every navigation/bfcache thaw (fresh V8 isolate), so a cached wrapper can
// never alias onto an unrelated later node.
var _lumen_element_wrappers = {};

// ── ParentNode / ElementTraversal helpers (DOM Standard §4.2.6/§4.2.7) ────────
// BUG-310: element-only tree navigation. `_lumen_get_children` returns EVERY
// child node (text/comment included), so `children`/`childElementCount`/
// `firstElementChild`/… must filter to element nodes to match the spec.

// True when `id` refers to an element node. Text nodes report via
// `_lumen_is_text_node`; every other non-element (comment/document/fragment/
// shadow-root) carries a `#`-prefixed node name, which a real element never does.
function _lumen_is_element_nid(id) {
    if (_lumen_is_text_node(id)) return false;
    var t = _lumen_get_tag_name(id);
    return typeof t === 'string' && t.length > 0 && t.charAt(0) !== '#';
}

// Element children of `nid`, in tree order, as raw node ids.
function _lumen_element_child_nids(nid) {
    var all = _lumen_get_children(nid);
    var out = [];
    for (var i = 0; i < all.length; i++) {
        if (_lumen_is_element_nid(all[i])) out.push(all[i]);
    }
    return out;
}

// HTMLCollection marker prototype (DOM Standard §4.2.10.2) so `children` can
// satisfy `x instanceof HTMLCollection`. Not constructible from script.
function HTMLCollection() { throw new TypeError('Illegal constructor'); }

// NodeList marker prototype (DOM Standard §4.2.10.1). Separate interface from
// HTMLCollection above because HTML LS §3.1.5 requires `getElementsByName` to
// hand back a NodeList specifically — a collection answering `instanceof
// HTMLCollection` fails that half of the interface (BUG-412). Only the surface
// the collection Proxy below can back is provided: `length`, indices, `item`,
// `forEach` and iteration; `entries`/`keys`/`values` are not implemented.
function NodeList() { throw new TypeError('Illegal constructor'); }
NodeList.prototype.forEach = function(cb, thisArg) {
    if (typeof cb !== 'function') throw new TypeError('callback is not a function');
    for (var i = 0; i < this.length; i++) cb.call(thisArg, this[i], i, this);
};

// BUG-328: the `name`-attribute half of `namedItem`/`ownKeys` (below) only
// applies to elements in the HTML namespace (DOM §4.2.10.2) — an element
// created with createElementNS(null-or-empty-string, ...) (`Namespace::None`,
// no namespace at all) must not have its `name` attribute exposed as a
// collection property, even though its `id` attribute still is (the id-pass
// in both functions below is intentionally namespace-blind).
function _lumen_is_html_namespace(nid) {
    return _lumen_u2n(_lumen_get_namespace_uri(nid)) === 'http://www.w3.org/1999/xhtml';
}

// HTML LS §3.1.5 — the member set behind `document.getElementsByName(name)`:
// every element of the document that is in the HTML namespace (so a foreign
// `<svg name=x>`/`<math name=x>` does NOT match) and whose `name` content
// attribute is exactly `name`. Comparison is case-sensitive and an `id` of the
// same value never matches — `name` is not one of the attributes selectors
// fold case for, so the native `[name]` query plus this JS comparison agree.
// The fixed, always-valid `[name]` selector is queried and the value compared
// here rather than a `[name="..."]` selector being built, so an argument
// carrying quotes, backslashes or newlines needs no CSS string escaping to
// stay correct.
function _lumen_elements_named_nids(name) {
    var all = _lumen_query_selector_all('[name]');
    var out = [];
    for (var i = 0; i < all.length; i++) {
        if (!_lumen_is_html_namespace(all[i])) continue;
        if (_lumen_u2n(_lumen_get_attr(all[i], 'name')) === name) out.push(all[i]);
    }
    return out;
}

// `namedItem` semantics (DOM §4.2.10.2): the first element in `ids` (tree
// order) for which `id === name`, or (failing that on the SAME element) which
// is in the HTML namespace and has `name attribute === name`; `null` if none
// match. BUG-328: this must be a single tree-order scan checking both
// conditions per element, not an id-pass over every element followed by a
// separate name-pass — a two-pass structure would return an id-matching
// element even when a different, earlier element already satisfies the
// name condition for the same key.
function _lumen_html_collection_named(ids, name) {
    for (var i = 0; i < ids.length; i++) {
        if (_lumen_u2n(_lumen_get_attr(ids[i], 'id')) === name) return _lumen_make_element(ids[i]);
        if (_lumen_is_html_namespace(ids[i]) && _lumen_u2n(_lumen_get_attr(ids[i], 'name')) === name) {
            return _lumen_make_element(ids[i]);
        }
    }
    return null;
}

// BUG-323: supported property names for `for-in`/`Object.getOwnPropertyNames`/
// `hasOwnProperty` enumeration (DOM §4.2.10.2). A single tree-order pass over
// `ids`, per element appending its `id` (if new), then — only for elements in
// the HTML namespace (BUG-328) — its `name` attribute (if new): the same
// per-element id-then-name order `_lumen_html_collection_named` above uses,
// NOT an id-pass over every element followed by a separate name-pass (BUG-328:
// that ordered a later element's `id` before an earlier element's `name`,
// e.g. tree order `[name=bar, id=baz]` produced `['baz','bar']` instead of
// the spec-required `['bar','baz']`), so the `ownKeys`/`getOwnPropertyDescriptor`
// traps stay consistent with what `get`/`has`/`namedItem` already expose.
function _lumen_html_collection_own_names(ids) {
    var names = [];
    var seen = {};
    function add(v) {
        if (v !== null && v !== '' && !Object.prototype.hasOwnProperty.call(seen, v)) {
            seen[v] = true;
            names.push(v);
        }
    }
    for (var i = 0; i < ids.length; i++) {
        add(_lumen_u2n(_lumen_get_attr(ids[i], 'id')));
        if (_lumen_is_html_namespace(ids[i])) add(_lumen_u2n(_lumen_get_attr(ids[i], 'name')));
    }
    return names;
}

// Live HTMLCollection over `owner_nid`'s element children (DOM §4.2.10.2).
// Backed by a Proxy so `length`, indices and named lookups all re-query the
// live tree on every access — the collection stays correct across
// append/remove without being rebuilt. Consumers may index it (`coll[0]`),
// call `.item(i)`/`.namedItem(name)` or read `.length`.
function _lumen_make_html_collection(owner_nid) {
    return _lumen_make_nid_collection(
        function() { return _lumen_element_child_nids(owner_nid); },
        HTMLCollection.prototype);
}

// The Proxy machinery behind every collection Lumen hands out: `idsFn` returns
// the current member node ids, `protoObj` decides which interface the result
// claims to implement. Split out of `_lumen_make_html_collection` so
// `HTMLFormControlsCollection` (BUG-383) shares it instead of re-implementing
// indexed and named access. `noNamed` drops the named half (`namedItem`,
// `list['someName']`, named own-keys): that is HTMLCollection behaviour
// (DOM §4.2.10.2), while a NodeList exposes indices alone (BUG-412).
function _lumen_make_nid_collection(idsFn, protoObj, noNamed) {
    var proto = Object.create(protoObj);
    function ids() { return idsFn(); }
    return new Proxy(proto, {
        get: function(target, prop) {
            if (prop === 'length') return ids().length;
            if (prop === 'item') {
                return function(i) {
                    var list = ids();
                    i = i >>> 0;
                    return i < list.length ? _lumen_make_element(list[i]) : null;
                };
            }
            if (prop === 'namedItem' && !noNamed) {
                return function(name) { return _lumen_html_collection_named(ids(), String(name)); };
            }
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
                var list = ids();
                var idx = parseInt(prop, 10);
                return idx < list.length ? _lumen_make_element(list[idx]) : undefined;
            }
            if (!noNamed && typeof prop === 'string' && prop !== 'constructor') {
                var named = _lumen_html_collection_named(ids(), prop);
                if (named !== null) return named;
            }
            return target[prop];
        },
        has: function(target, prop) {
            if (prop === 'length' || prop === 'item') return true;
            if (prop === 'namedItem') return !noNamed;
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
                return parseInt(prop, 10) < ids().length;
            }
            if (!noNamed && typeof prop === 'string'
                && _lumen_html_collection_named(ids(), prop) !== null) return true;
            return prop in target;
        },
        // BUG-323: `ownKeys` + `getOwnPropertyDescriptor` so `for-in`,
        // `Object.getOwnPropertyNames`/`keys` and `hasOwnProperty` see the
        // collection's indices and named keys instead of the empty plain
        // `proto` target. Indexed keys are enumerable (WebIDL legacy platform
        // object indexed-property semantics); named keys are own but
        // non-enumerable, matching real HTMLCollection behaviour where
        // `for (var p in list)` yields only indices while
        // `Object.getOwnPropertyNames`/`hasOwnProperty` also see named keys.
        ownKeys: function(target) {
            var list = ids();
            var keys = [];
            for (var i = 0; i < list.length; i++) keys.push(String(i));
            if (noNamed) return keys;
            var names = _lumen_html_collection_own_names(list);
            for (var k = 0; k < names.length; k++) keys.push(names[k]);
            return keys;
        },
        getOwnPropertyDescriptor: function(target, prop) {
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
                var list = ids();
                var idx = parseInt(prop, 10);
                if (idx < list.length) {
                    return { value: _lumen_make_element(list[idx]), writable: false, enumerable: true, configurable: true };
                }
                return undefined;
            }
            if (!noNamed && typeof prop === 'string') {
                var named = _lumen_html_collection_named(ids(), prop);
                if (named !== null) {
                    return { value: named, writable: false, enumerable: false, configurable: true };
                }
            }
            return undefined;
        },
    });
}

function _lumen_make_element(nid) {
    if (nid === null || nid === undefined) return null;
    // BUG-291: return the interned wrapper if this nid was already wrapped,
    // so repeated access to the same underlying node (`.firstChild`,
    // `.parentElement`, `getElementById` twice, ...) yields the same JS
    // object — required for `===` node identity and for expando properties
    // set on a node to survive later re-access. Split from the actual
    // builder (`_lumen_build_element`) so the cache write below is the one
    // and only place a wrapper gets interned, matching what `_lumen_gc_collect`
    // purges.
    var cached = _lumen_element_wrappers[nid];
    if (cached !== undefined) return cached;
    var built = _lumen_build_element(nid);
    _lumen_element_wrappers[nid] = built;
    return built;
}

// HTML LS 3.2.6.6 — DOMStringMap (`element.dataset`), BUG-703.
// data-foo-bar  <->  dataset.fooBar. Built on a Proxy so get/set/delete/`in`/
// enumeration all stay live against the element's attributes instead of
// snapshotting them.
function _lumen_dataset_attr_name(prop) {
    // Spec: a key containing an ASCII upper alpha right after a `-` is invalid.
    if (/-[a-z]/.test(prop)) { return null; }
    return 'data-' + prop.replace(/[A-Z]/g, function(c) { return '-' + c.toLowerCase(); });
}
function _lumen_dataset_prop_name(attr) {
    return attr.slice(5).replace(/-([a-z])/g, function(m, c) { return c.toUpperCase(); });
}
function _lumen_dataset_keys(nid) {
    var out = [];
    var names = _lumen_get_attr_names(nid);
    for (var i = 0; i < names.length; i++) {
        if (names[i].indexOf('data-') === 0) { out.push(_lumen_dataset_prop_name(names[i])); }
    }
    return out;
}
// WebIDL interface object — `dataset` must satisfy `instanceof DOMStringMap`
// (BUG-414: the WPT dataset tests assert exactly that, which is why the old
// SVG-only `get dataset() { return {}; }` stub failed them even where it
// answered). Not constructible from script, per WebIDL.
function DOMStringMap() { throw new TypeError('Illegal constructor'); }
globalThis.DOMStringMap = DOMStringMap;

function _lumen_make_dataset(nid) {
    return new Proxy(Object.create(DOMStringMap.prototype), {
        get: function(_t, prop) {
            if (typeof prop !== 'string') { return undefined; }
            var attr = _lumen_dataset_attr_name(prop);
            if (attr === null) { return undefined; }
            var v = _lumen_u2n(_lumen_get_attr(nid, attr));
            return v !== null ? v : undefined;
        },
        set: function(_t, prop, value) {
            var attr = _lumen_dataset_attr_name(String(prop));
            if (attr === null) {
                throw new DOMException('Invalid dataset name: ' + prop, 'SyntaxError');
            }
            _lumen_set_attr(nid, attr, String(value));
            return true;
        },
        has: function(_t, prop) {
            if (typeof prop !== 'string') { return false; }
            var attr = _lumen_dataset_attr_name(prop);
            return attr !== null && _lumen_u2n(_lumen_get_attr(nid, attr)) !== null;
        },
        deleteProperty: function(_t, prop) {
            var attr = _lumen_dataset_attr_name(String(prop));
            if (attr !== null) { _lumen_remove_attr(nid, attr); }
            return true;
        },
        ownKeys: function() { return _lumen_dataset_keys(nid); },
        getOwnPropertyDescriptor: function(_t, prop) {
            if (typeof prop !== 'string') { return undefined; }
            var attr = _lumen_dataset_attr_name(prop);
            if (attr === null) { return undefined; }
            var v = _lumen_u2n(_lumen_get_attr(nid, attr));
            if (v === null) { return undefined; }
            return { value: v, writable: true, enumerable: true, configurable: true };
        },
    });
}

// ── DOM §4.9.1 NamedNodeMap / §4.9.2 Attr — `element.attributes` (BUG-732) ───
// `element.attributes` was `undefined` even though every piece of data behind
// it (`_lumen_get_attr_names` + `_lumen_get_attr`) was already there: code that
// walks an element's attributes generically — serializers, sanitizers,
// framework hydration diffing — got a `TypeError` on the first access.
// Not constructible from script, per WebIDL.
function NamedNodeMap() { throw new TypeError('Illegal constructor'); }
globalThis.NamedNodeMap = NamedNodeMap;

// A live `Attr` node over `nid`'s `name` attribute: reads and writes go
// straight through to the element, so the object never holds a stale value.
// Lumen's attribute model is name-only (see `getAttributeNS` on the element
// wrapper), hence `namespaceURI === null` and a prefix split done on the
// qualified name alone.
function _lumen_make_attr(nid, name) {
    var colon = name.indexOf(':');
    var attr = Object.create(Attr.prototype);
    function value() {
        var v = _lumen_u2n(_lumen_get_attr(nid, name));
        return v !== null ? v : '';
    }
    function setValue(v) { _lumen_set_attr(nid, name, String(v)); }
    Object.defineProperties(attr, {
        name:         { get: function() { return name; }, enumerable: true, configurable: true },
        nodeName:     { get: function() { return name; }, enumerable: true, configurable: true },
        localName:    { get: function() { return colon >= 0 ? name.slice(colon + 1) : name; }, enumerable: true, configurable: true },
        prefix:       { get: function() { return colon >= 0 ? name.slice(0, colon) : null; }, enumerable: true, configurable: true },
        namespaceURI: { get: function() { return null; }, enumerable: true, configurable: true },
        nodeType:     { get: function() { return 2; }, enumerable: true, configurable: true },
        // DOM §4.9.2: `specified` is a legacy getter that is always true.
        specified:    { get: function() { return true; }, enumerable: true, configurable: true },
        value:        { get: value, set: setValue, enumerable: true, configurable: true },
        nodeValue:    { get: value, set: setValue, enumerable: true, configurable: true },
        textContent:  { get: value, set: setValue, enumerable: true, configurable: true },
        ownerElement: { get: function() { return _lumen_make_element(nid); }, enumerable: true, configurable: true },
        ownerDocument: { get: function() { return document; }, enumerable: true, configurable: true },
    });
    return attr;
}

// Live `NamedNodeMap` over `nid`'s attributes: indices, `length`, `item()`,
// `getNamedItem()`/`setNamedItem()`/`removeNamedItem()` and named access all
// re-read `_lumen_get_attr_names` on every access, so the map tracks
// `setAttribute`/`removeAttribute` without being rebuilt — the same Proxy
// design `_lumen_make_nid_collection` uses for HTMLCollection.
function _lumen_make_named_node_map(nid) {
    var proto = Object.create(NamedNodeMap.prototype);
    function names() { return _lumen_get_attr_names(nid); }
    function at(list, i) { return i < list.length ? _lumen_make_attr(nid, list[i]) : null; }
    var methods = {
        item: function(i) { return at(names(), i >>> 0); },
        getNamedItem: function(n) {
            var name = String(n);
            return _lumen_get_attr(nid, name) !== undefined ? _lumen_make_attr(nid, name) : null;
        },
        // Namespaces are not modelled (see `_lumen_make_attr`), so the NS forms
        // ignore the namespace and look the qualified name up.
        getNamedItemNS: function(ns, n) { return methods.getNamedItem(n); },
        setNamedItem: function(attr) {
            if (!attr || typeof attr.name !== 'string') {
                throw new TypeError('setNamedItem: argument is not an Attr');
            }
            var prev = methods.getNamedItem(attr.name);
            _lumen_set_attr(nid, attr.name, String(attr.value));
            return prev;
        },
        setNamedItemNS: function(attr) { return methods.setNamedItem(attr); },
        removeNamedItem: function(n) {
            var name = String(n);
            if (_lumen_get_attr(nid, name) === undefined) {
                throw new DOMException('No attribute named ' + name, 'NotFoundError');
            }
            var prev = _lumen_make_attr(nid, name);
            _lumen_remove_attr(nid, name);
            return prev;
        },
        removeNamedItemNS: function(ns, n) { return methods.removeNamedItem(n); },
    };
    return new Proxy(proto, {
        get: function(target, prop) {
            if (prop === 'length') return names().length;
            if (typeof prop === 'string' && Object.prototype.hasOwnProperty.call(methods, prop)) {
                return methods[prop];
            }
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
                var byIndex = at(names(), parseInt(prop, 10));
                return byIndex !== null ? byIndex : undefined;
            }
            if (typeof prop === 'string' && prop !== 'constructor'
                && _lumen_get_attr(nid, prop) !== undefined) {
                return _lumen_make_attr(nid, prop);
            }
            return target[prop];
        },
        has: function(target, prop) {
            if (prop === 'length') return true;
            if (typeof prop === 'string' && Object.prototype.hasOwnProperty.call(methods, prop)) return true;
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
                return parseInt(prop, 10) < names().length;
            }
            if (typeof prop === 'string' && _lumen_get_attr(nid, prop) !== undefined) return true;
            return prop in target;
        },
        // Indexed keys enumerable, named keys own-but-not-enumerable — the same
        // split `_lumen_make_nid_collection` applies (BUG-323), so `for-in`
        // yields indices while `Object.getOwnPropertyNames` also sees names.
        ownKeys: function() {
            var list = names();
            var keys = [];
            for (var i = 0; i < list.length; i++) keys.push(String(i));
            for (var k = 0; k < list.length; k++) {
                if (!/^[0-9]+$/.test(list[k])) keys.push(list[k]);
            }
            return keys;
        },
        getOwnPropertyDescriptor: function(target, prop) {
            if (typeof prop !== 'string') return undefined;
            var list = names();
            if (/^[0-9]+$/.test(prop)) {
                var byIndex = at(list, parseInt(prop, 10));
                return byIndex !== null
                    ? { value: byIndex, writable: false, enumerable: true, configurable: true }
                    : undefined;
            }
            if (_lumen_get_attr(nid, prop) !== undefined) {
                return { value: _lumen_make_attr(nid, prop), writable: false, enumerable: false, configurable: true };
            }
            return undefined;
        },
    });
}

// ── HTML LS §3.2.7 `innerText` / `outerText` setters (BUG-413) ───────────────
// Neither property existed at all — not on the wrapper, not on a prototype — so
// `el.innerText = s` quietly minted an expando and every test in the WPT
// directory `html/dom/elements/the-innertext-and-outertext-properties/` died on
// the *next* statement. Only the two setters live here; the `innerText` getter
// is «rendered text» and needs the layout boxes, which is a separate slice.

// `innerText`/`outerText` are `HTMLElement` members, so an SVG or MathML element
// — or a Text/Comment node, which shares this wrapper factory — has to behave as
// if the property were simply absent. WPT asserts exactly that with
// `testHTML('<svg>', 'abc', …)`.
var _LUMEN_HTML_NS = 'http://www.w3.org/1999/xhtml';
function _lumen_is_html_element_nid(n) {
    return _lumen_u2n(_lumen_get_namespace_uri(n)) === _LUMEN_HTML_NS;
}

// ── getElementsByTagName(NS) matching (DOM LS §4.5) ──────────────────────────
// BUG-416. The name-matching half, shared by the document, the element and the
// detached-document accessors below. Deliberately NOT routed through the
// selector engine the way `getElementsByClassName` is: a tag name is arbitrary
// text rather than a CSS type selector, and a type selector here is matched by
// exact string equality against the local name (`style.rs::matches_simple`), so
// delegating got BOTH halves of the case rule wrong — `getElementsByTagName('DIV')`
// found nothing instead of every HTML `<div>`, and a name that is not a valid
// identifier (`'a b'`, `'1'`) parsed as some other selector or as none at all.
//
// Returns a predicate over a raw node id. A null local name is what the native
// side answers for every non-element node, so it doubles as the element check.
function _lumen_tag_name_predicate(qualifiedName) {
    var want  = String(qualifiedName);
    var lower = want.toLowerCase();
    var all   = want === '*';
    return function(n) {
        var local = _lumen_u2n(_lumen_get_local_name(n));
        if (local === null) return false;
        if (all) return true;
        // An HTML-namespace element in an HTML document matches the ASCII
        // lower-cased ask; everything else — SVG, MathML, no namespace — has to
        // match its qualified name exactly. That distinction is the whole point
        // of `Element.getElementsByTagName-foreign-0*.html`: an SVG
        // `<linearGradient>` must be missed by `getElementsByTagName('lineargradient')`
        // and hit by the exactly-spelled one, while an HTML `<div>` answers to
        // any casing.
        return _lumen_u2n(_lumen_get_namespace_uri(n)) === _LUMEN_HTML_NS
            ? local.toLowerCase() === lower
            : local === want;
    };
}

// DOM LS §4.5 «getElementsByTagNameNS». `namespace` is matched against the
// element's namespace URI — null and '' both mean «no namespace» per «validate
// and extract», so `createElementNS(null, 'x')` is found by a null ask — and
// `localName` against the local name, case-sensitively in both positions; '*'
// matches anything. Note BUG-830: a namespace URI outside the six the DOM enum
// knows collapses into the HTML one at creation time, so such an element can
// only be found under the namespace it collapsed to, not the one it was asked
// for. That is a limitation of the enum, not of this matcher.
function _lumen_tag_ns_predicate(namespace, localName) {
    var wantNs = (namespace === null || namespace === undefined || namespace === '')
        ? null : String(namespace);
    var wantLocal = String(localName);
    var anyNs     = wantNs === '*';
    var anyLocal  = wantLocal === '*';
    return function(n) {
        var local = _lumen_u2n(_lumen_get_local_name(n));
        if (local === null) return false;
        if (!anyNs && _lumen_u2n(_lumen_get_namespace_uri(n)) !== wantNs) return false;
        return anyLocal || local === wantLocal;
    };
}

// Filters a tree-ordered list of raw node ids by `pred` and wraps the survivors.
// Static array, not a live HTMLCollection — the same simplification
// `querySelectorAll`/`getElementsByClassName` already make.
function _lumen_collect_matching(nids, pred) {
    var out = [];
    for (var i = 0; i < nids.length; i++) {
        if (pred(nids[i])) out.push(_lumen_make_element(nids[i]));
    }
    return out;
}

// Mimics assigning to an object that carries no such accessor: the own accessor
// installed by the wrapper literal is shadowed by a plain data property, so the
// write neither reaches the DOM nor silently vanishes.
function _lumen_assign_as_expando(obj, prop, value) {
    Object.defineProperty(obj, prop, {
        value: value, writable: true, enumerable: true, configurable: true,
    });
}

// Same overflow sentinel `createElement`/`createTextNode` check — the native
// side answers -1 once the node budget is exhausted (BUG-457).
function _lumen_new_node_or_throw(n) {
    if (n < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
    return n;
}

// HTML LS §3.2.7 «rendered text fragment»: splits `input` on line breaks into the
// Text nodes and `<br>` elements the two setters insert, returning their NodeIds
// in tree order. A CRLF pair counts as ONE break, `\n\n` and `\r\r` as two, and
// a leading or trailing break yields a `<br>` with no Text node beside it.
function _lumen_rendered_text_nids(input) {
    var out = [];
    var i = 0;
    while (i < input.length) {
        var start = i;
        while (i < input.length && input[i] !== '\n' && input[i] !== '\r') { i++; }
        if (i > start) {
            out.push(_lumen_new_node_or_throw(_lumen_create_text_node(input.slice(start, i))));
        }
        while (i < input.length && (input[i] === '\n' || input[i] === '\r')) {
            if (input[i] === '\r' && input[i + 1] === '\n') { i++; }
            i++;
            out.push(_lumen_new_node_or_throw(_lumen_create_element('br')));
        }
    }
    return out;
}

// HTML LS §3.2.7 «merge with the next text node»: when `nodeNid` and its next
// sibling are both Text, fold the sibling's data into it and drop the sibling.
// Deliberately narrower than `normalize()` — the `outerText` setter merges only
// the two nodes that used to touch the replaced element, nothing else.
function _lumen_merge_with_next_text(pid, nodeNid) {
    if (nodeNid === null || !_lumen_is_text_node(nodeNid)) { return; }
    var sibs = _lumen_get_children(pid);
    var i = sibs.indexOf(nodeNid);
    if (i < 0 || i + 1 >= sibs.length) { return; }
    var next = sibs[i + 1];
    if (!_lumen_is_text_node(next)) { return; }
    _lumen_set_text_content(nodeNid, _lumen_get_text_content(nodeNid) + _lumen_get_text_content(next));
    _lumen_remove_child(pid, next);
}

// ── HTML LS §3.2.7 `innerText` / `outerText` getters (BUG-413, slice 2) ──────
// «Rendered text», not `textContent`: a `display:none` subtree and a
// `visibility:hidden` box drop out, a collapsible whitespace run folds to one
// space, `text-transform` applies, and a block boundary or `<br>` becomes a
// line feed.
//
// The layout bridge read here is the computed-style snapshot that already backs
// `getComputedStyle`, republished by the shell after every relayout. Two of its
// properties decide everything:
//
//   * an entry EXISTS ⟺ the engine laid the node out. `display: none` produces
//     no box and no segment, so it produces no entry either — that is the whole
//     `innerText`-vs-`textContent` difference, read straight off the engine.
//   * an INLINE element has no entry, because it owns no box: its content is
//     flattened into the enclosing block's inline run. The style that governs
//     that content is published on the *text node* instead
//     (`lumen_layout::INLINE_SEGMENT_PROPERTIES`), which is why every style
//     lookup below is made against the text node and never against its parent.
//
// So an entry-less *element* is not «hidden» — it is a transparent inline
// wrapper, and the code recurses through it while contributing nothing of its
// own. Whether its text survives is decided one level down, by whether the text
// node itself has an entry.
//
// What the bridge does NOT carry is the per-line-box text, so step 4 of the
// collection steps («for each CSS text box produced by node …») is reproduced
// from the computed `white-space`/`text-transform` values instead of read off
// the boxes. The difference is confined to soft wraps: a line the engine broke
// mid-paragraph contributes no line feed here. A wrap inserts no character in
// `textContent` either, and the spec's own modified rules already collapse an
// end-of-line space, so the two agree except on a trailing space a soft wrap
// keeps alive.
//
// Three more deviations worth naming. `<select>`/`<optgroup>`/`<option>` are
// not given the synthetic boxes step 3's exception list prescribes. A node the
// engine has not laid out yet — a fresh document, a subtree appended since the
// last relayout, anything read from a parser-time script ([BUG-443]) — reads as
// «not being rendered», so the getter answers `textContent` for it. And a
// `<br>`, which owns no box either way, contributes its line feed even when it
// is itself `display: none` or `visibility: hidden`; nothing distinguishes those
// from an ordinary one at this layer.

// A node is «being rendered» when the engine published a computed style for it.
function _lumen_rt_is_rendered(n) {
    return _lumen_get_computed_style(n, 'visibility') !== '';
}

// Step 8 of the collection steps: a box that starts and ends a line. `table-row`
// and the row groups are not block-level in CSS-DISPLAY terms, but they do break
// the line in every engine — and a table whose rows ran together would make the
// tab of step 6 meaningless. `table-cell` is deliberately absent: it gets the
// tab, not a line feed.
var _LUMEN_RT_BLOCK_LEVEL = {
    'block': 1, 'flow-root': 1, 'list-item': 1, 'table': 1, 'table-caption': 1,
    'table-row': 1, 'table-row-group': 1, 'table-header-group': 1,
    'table-footer-group': 1, 'flex': 1, 'grid': 1,
};

// `text-transform` over a whole text-node string. `capitalize` is approximated
// on ASCII word starts: the real rule is per typographic unit and spans text
// nodes, which this getter's per-node view cannot see.
function _lumen_rt_transform(s, tt) {
    if (tt === 'uppercase') { return s.toUpperCase(); }
    if (tt === 'lowercase') { return s.toLowerCase(); }
    if (tt === 'capitalize') {
        return s.replace(/(^|[^A-Za-z0-9])([a-z])/g, function(_m, sep, ch) {
            return sep + ch.toUpperCase();
        });
    }
    return s;
}

// Step 4: the text of the boxes `n` produces, after `white-space` collapsing and
// `text-transform`. Returns an item for `_lumen_rt_concat`; `pre` marks a string
// whose spaces survive contact with its neighbours.
function _lumen_rt_text_item(n) {
    var raw = _lumen_get_text_content(n).replace(/\r\n?/g, '\n');
    var ws  = _lumen_get_computed_style(n, 'white-space');
    var preserveSpaces = (ws === 'pre' || ws === 'pre-wrap' || ws === 'break-spaces');
    var s;
    if (preserveSpaces) {
        s = raw;
    } else if (ws === 'pre-line') {
        // Spaces and tabs collapse, segment breaks survive — and a space that
        // ends up touching one of those breaks is removed, as at a line end.
        s = raw.replace(/[ \t\f]+/g, ' ').replace(/ *\n */g, '\n');
    } else {
        s = raw.replace(/[ \t\n\f]+/g, ' ');
    }
    return {
        s: _lumen_rt_transform(s, _lumen_get_computed_style(n, 'text-transform')),
        pre: preserveSpaces,
    };
}

// HTML LS §3.2.7 «rendered text collection steps». Returns a flat list whose
// items are numbers (a required line break count) or `{s, pre}` objects. The
// line feed a `<br>` contributes is one of those objects, not a break count: the
// spec appends it as a plain string, so it neither merges with an adjacent count
// nor gets stripped at either end of the result.
function _lumen_rt_collect(n) {
    var items = [];
    var isText = _lumen_is_text_node(n);
    // Step 1 runs before the node itself is judged, so a box-less
    // `display: contents` element still passes its children through.
    if (!isText) {
        var kids = _lumen_get_children(n);
        for (var _rci = 0; _rci < kids.length; _rci++) {
            var sub = _lumen_rt_collect(kids[_rci]);
            for (var _rcj = 0; _rcj < sub.length; _rcj++) { items.push(sub[_rcj]); }
        }
    }
    var vis = _lumen_get_computed_style(n, 'visibility');
    if (isText) {
        // Steps 2 and 3 for a text node: no entry means the engine produced no
        // segment for it — `display: none` somewhere above — and a non-visible
        // one means the segment was laid out but not painted. Either way the
        // text contributes nothing, and a text node has no children to pass on.
        return vis === 'visible' ? [_lumen_rt_text_item(n)] : items;
    }
    // Steps 5 and 7 key off an HTML element name; step 6 and step 8 key off the
    // used `display` and so apply to foreign content too.
    var isHtml = _lumen_is_html_element_nid(n);
    var tag = isHtml ? _lumen_get_tag_name(n) : '';
    // Step 5 goes first because a `<br>` is empty: it produces no box and no
    // segment, so it has no entry, and judging it by the entry-less rule below
    // would silently drop every line feed the spec's most literal case makes.
    if (tag === 'BR') { items.push({ s: '\n', pre: true }); return items; }

    // Step 2 — note the spec returns the CHILDREN's items here rather than
    // nothing, because a descendant may set `visibility: visible` again.
    if (vis !== '' && vis !== 'visible') { return items; }
    // An entry-less element owns no box: an inline wrapper whose content is
    // already accounted for by the text nodes below it. Its children's items
    // pass through — which is also step 3's behaviour for `display: contents` —
    // but it contributes no line break of its own, so a `display: none` block
    // adds nothing at all.
    if (vis === '') { return items; }

    var display = _lumen_get_computed_style(n, 'display');
    if (display === 'table-cell') {                                        // step 6
        var pid = _lumen_u2n(_lumen_get_parent(n));
        if (pid !== null) {
            var sibs = _lumen_get_children(pid);
            var seen = false;
            var last = true;
            for (var _rck = 0; _rck < sibs.length; _rck++) {
                if (sibs[_rck] === n) { seen = true; continue; }
                if (seen && _lumen_get_computed_style(sibs[_rck], 'display') === 'table-cell') {
                    last = false;
                    break;
                }
            }
            if (!last) { items.push({ s: '\t', pre: true }); }
        }
    }

    if (tag === 'P') { items.unshift(2); items.push(2); }                  // step 7
    else if (_LUMEN_RT_BLOCK_LEVEL[display] === 1) {                       // step 8
        items.unshift(1);
        items.push(1);
    }
    return items;
}

// Steps 3-7 of the getter: drop empty strings, strip the leading and trailing
// runs of required line break counts, turn every remaining run into as many line
// feeds as its largest member, and concatenate.
//
// The concatenation is where the cross-node part of whitespace collapsing
// happens — the part a per-text-node `replace` cannot do. A collapsible space is
// held back as `pending` and only committed once a character follows it, so two
// inline siblings that both touch a space contribute one, and a space that ends
// up against a line break or against either end of the result disappears.
function _lumen_rt_concat(items) {
    var kept = [];
    for (var _rti = 0; _rti < items.length; _rti++) {
        var it = items[_rti];
        if (typeof it === 'number') { kept.push(it); continue; }
        if (it.s !== '') { kept.push(it); }
    }
    var from = 0;
    var to = kept.length;
    while (from < to && typeof kept[from] === 'number') { from++; }
    while (to > from && typeof kept[to - 1] === 'number') { to--; }

    var out = '';
    var pending = false;
    var run = 0;
    for (var _rtj = from; _rtj < to; _rtj++) {
        var item = kept[_rtj];
        if (typeof item === 'number') {
            if (item > run) { run = item; }
            continue;
        }
        if (run > 0) {
            pending = false;
            for (var _rtk = 0; _rtk < run; _rtk++) { out += '\n'; }
            run = 0;
        }
        if (item.pre) {
            if (pending) { out += ' '; pending = false; }
            out += item.s;
            continue;
        }
        for (var _rtl = 0; _rtl < item.s.length; _rtl++) {
            var ch = item.s.charAt(_rtl);
            if (ch === ' ') {
                // A collapsible space at the very start of the result, or right
                // after a line break, is at a line edge and never renders.
                if (out !== '' && out.charAt(out.length - 1) !== '\n') { pending = true; }
                continue;
            }
            if (ch === '\n') { pending = false; out += '\n'; continue; }
            if (pending) { out += ' '; pending = false; }
            out += ch;
        }
    }
    return out;
}

// Shared by both getters — HTML LS gives `outerText` the same getter steps as
// `innerText`; only their setters differ.
function _lumen_rendered_text(nid) {
    var text = _lumen_rt_concat(_lumen_rt_collect(nid));
    if (_lumen_rt_is_rendered(nid) || text !== '') { return text; }
    // Step 1: `this` is not being rendered → `textContent`. Reached when the
    // element owns no box AND nothing below it was laid out, which is what
    // separates a `display: none` subtree (or a detached one, or a document with
    // no layout yet) from an ordinary inline wrapper — the wrapper's own text
    // nodes carry entries and have just produced a non-empty result above.
    return _lumen_get_text_content(nid);
}

// BUG-849: the wrapper's whole interface used to be built PER NODE — the object
// literal below (~130 accessors and methods), the `Object.defineProperty` block
// after it, and one `on<type>` accessor pair for every name in
// `_LUMEN_EVENT_HANDLER_ATTRS` — roughly 250 closures per element. That cost
// ~142 us per `document.createElement` and ~35 KB of heap per node, so 40 000
// script-built nodes reached 1.4 GB and a fatal V8 out-of-memory. The members
// are built ONCE here instead and installed on a per-interface shared prototype
// (`_lumen_wrapper_proto_for`), the same «onto the interface prototype, not onto
// every instance» move BUG-383 already made for reflected IDL attributes; an
// instance now owns nothing but `__nid__`.
//
// Consequence for anything added here: a member reads its node through
// `this.__nid__` (the `var nid = this.__nid__;` prologue every one of them
// opens with), so it may NOT be called with a foreign `this` — and a nested
// callback inside a member must not repeat the prologue, since `this` is the
// global object there.
var _LUMEN_WRAPPER_MEMBERS = {
        // BUG-367: `__nid__` is re-declared as a non-enumerable, non-writable
        // own property right below the literal — see the `Object.defineProperty`
        // call at the end of this function for why. It has to be seeded here so
        // that the accessors defined in this literal (which capture `nid`) and
        // the shim's own `child.__nid__` readers agree from the first moment.
        get tagName()        { var nid = this.__nid__; return _lumen_qualified_tag_name(nid); },
        get nodeName()       { var nid = this.__nid__; return _lumen_qualified_tag_name(nid); },
        // DOM LS §4.9: `localName` is the qualified name with no case folding at
        // all (`rect`, `linearGradient`), and Lumen never parses a prefix out of
        // a tag name, so `prefix` is always `null` — present-and-null, which is
        // what `'prefix' in el` feature checks look for, not absent (BUG-367).
        get localName()      { var nid = this.__nid__; return _lumen_u2n(_lumen_get_local_name(nid)); },
        get prefix()         { var nid = this.__nid__; return null; },
        get nodeType()       { var nid = this.__nid__; return _lumen_is_text_node(nid) ? 3 : (_lumen_is_comment_node(nid) ? 8 : 1); },
        // DOM LS §4.9.1: XHTML namespace for HTML elements, `null` for non-element nodes
        // (text/comment). react-dom's root-listening bootstrap (BUG-281) reads this.
        get namespaceURI()   { var nid = this.__nid__; return _lumen_u2n(_lumen_get_namespace_uri(nid)); },
        get id()             { var nid = this.__nid__; var v = _lumen_u2n(_lumen_get_attr(nid, 'id'));    return v !== null ? v : ''; },
        set id(v)            { var nid = this.__nid__; _lumen_set_attr(nid, 'id', String(v)); },
        get className()      { var nid = this.__nid__; var v = _lumen_u2n(_lumen_get_attr(nid, 'class')); return v !== null ? v : ''; },
        set className(v)     { var nid = this.__nid__; _lumen_set_attr(nid, 'class', String(v)); },
        get classList()      { return _lumen_wrapper_slot(this, '__classList__', _lumen_make_class_list); },
        get style()          { return _lumen_wrapper_slot(this, '__style__', _lumen_make_style); },
        // HTML LS 3.2.6.6 (BUG-703): lazily built, then cached for the wrapper's
        // lifetime so `el.dataset === el.dataset` holds as it does in a browser.
        get dataset()        { var nid = this.__nid__;
            return _lumen_wrapper_slot(this, '__dataset__', _lumen_make_dataset);
        },
        // DOM §4.9 Element.attributes (BUG-732) — the map itself is live, and
        // cached for the wrapper's lifetime so `el.attributes === el.attributes`
        // holds as it does in a browser (same treatment as `dataset` above).
        get attributes()     { var nid = this.__nid__;
            return _lumen_wrapper_slot(this, '__attributes__', _lumen_make_named_node_map);
        },
        // DOM §4.9: the Attr-node accessors that pair with `attributes`.
        getAttributeNode:   function(n)      { var nid = this.__nid__; return this.attributes.getNamedItem(n); },
        getAttributeNodeNS: function(ns, n)  { var nid = this.__nid__; return this.attributes.getNamedItem(n); },
        setAttributeNode:   function(attr)   { var nid = this.__nid__; return this.attributes.setNamedItem(attr); },
        setAttributeNodeNS: function(attr)   { var nid = this.__nid__; return this.attributes.setNamedItem(attr); },
        removeAttributeNode: function(attr)  { var nid = this.__nid__;
            if (!attr || typeof attr.name !== 'string') {
                throw new TypeError('removeAttributeNode: argument is not an Attr');
            }
            return this.attributes.removeNamedItem(attr.name);
        },
        get attributeStyleMap() { var nid = this.__nid__;
            // CSS Typed OM L1 — StylePropertyMap for element.style (mutable)
            if (typeof CSS === 'undefined' || !CSS.StylePropertyMap) return null;
            return new CSS.StylePropertyMap(nid);
        },
        computedStyleMap: function() { var nid = this.__nid__;
            // CSS Typed OM L1 §6.1 — read-only map over the RESOLVED CASCADE, not
            // over the inline style attribute (BUG-387). The former
            // `ComputedStylePropertyMap` class this used to build is gone: it was
            // not a spec name, and it subclassed the inline map.
            if (typeof CSS === 'undefined' || !CSS.StylePropertyMapReadOnly) return null;
            return new CSS.StylePropertyMapReadOnly(nid);
        },
        get textContent()    { var nid = this.__nid__; return _lumen_get_text_content(nid); },
        set textContent(v)   { var nid = this.__nid__; _lumen_set_text_content(nid, String(v)); },
        get innerHTML()      { var nid = this.__nid__; return _lumen_get_inner_html(nid); },
        set innerHTML(v)     { var nid = this.__nid__; _lumen_set_inner_html(nid, String(v)); },
        // DOM Parsing §2.6 'Extensions to the Element interface' — outerHTML
        // (BUG-351). Getter serializes this element itself; setter parses the
        // assigned markup and replaces `this` with the result in its parent,
        // mirroring `replaceWith`. Per spec, no-op if detached, throws if the
        // parent is the Document node itself (i.e. `this` is the root element).
        get outerHTML()      { var nid = this.__nid__; return _lumen_get_outer_html(nid); },
        set outerHTML(v) { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            if (pid === _lumen_get_document_root()) {
                throw new DOMException(
                    'Failed to set the outerHTML property: This element has no parent node.',
                    'NoModificationAllowedError');
            }
            var newIds = _lumen_parse_html_fragment(String(v));
            var wrapped = [];
            for (var _ohi = 0; _ohi < newIds.length; _ohi++) { wrapped.push(_lumen_make_element(newIds[_ohi])); }
            this.replaceWith.apply(this, wrapped);
        },
        // HTML LS §3.2.7 (BUG-413). Both getters run the same steps — the spec
        // gives `outerText` no getter of its own — over the layout snapshots;
        // see `_lumen_rendered_text` above for what that reads and where it
        // approximates. `undefined` outside the HTML namespace, because both are
        // `HTMLElement` members and this factory also wraps SVG/MathML elements
        // and Text/Comment nodes.
        get innerText() { var nid = this.__nid__;
            return _lumen_is_html_element_nid(nid) ? _lumen_rendered_text(nid) : undefined;
        },
        get outerText() { var nid = this.__nid__;
            return _lumen_is_html_element_nid(nid) ? _lumen_rendered_text(nid) : undefined;
        },
        // `[LegacyNullToEmptyString]` is why `null` becomes '' here while
        // `undefined` stringifies to 'undefined'.
        set innerText(v) { var nid = this.__nid__;
            if (!_lumen_is_html_element_nid(nid)) {
                _lumen_assign_as_expando(this, 'innerText', v);
                return;
            }
            var kids = _lumen_rendered_text_nids(v === null ? '' : String(v));
            var old  = _lumen_get_children(nid).slice();
            for (var _iti = 0; _iti < old.length; _iti++) { _lumen_remove_child(nid, old[_iti]); }
            for (var _itj = 0; _itj < kids.length; _itj++) { _lumen_append_child(nid, kids[_itj]); }
        },
        // Same fragment, but it replaces the element itself and then re-joins the
        // text nodes that used to sit either side of it. Assigning '' therefore
        // removes the element and leaves a single merged neighbour behind.
        set outerText(v) { var nid = this.__nid__;
            if (!_lumen_is_html_element_nid(nid)) {
                _lumen_assign_as_expando(this, 'outerText', v);
                return;
            }
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) {
                throw new DOMException(
                    'Failed to set the outerText property: This element has no parent node.',
                    'NoModificationAllowedError');
            }
            var sibs = _lumen_get_children(pid);
            var idx  = sibs.indexOf(nid);
            var prevNid = idx > 0 ? sibs[idx - 1] : null;
            var nextNid = (idx >= 0 && idx + 1 < sibs.length) ? sibs[idx + 1] : null;
            var kids = _lumen_rendered_text_nids(v === null ? '' : String(v));
            // An all-empty assignment still has to leave a Text node behind, so
            // that the merge below has something to fold the neighbours into.
            if (kids.length === 0) { kids.push(_lumen_new_node_or_throw(_lumen_create_text_node('')));  }
            for (var _oti = 0; _oti < kids.length; _oti++) { _lumen_insert_before(pid, kids[_oti], nid); }
            _lumen_remove_child(pid, nid);
            if (nextNid !== null) {
                var after = _lumen_get_children(pid);
                var ni    = after.indexOf(nextNid);
                if (ni > 0) { _lumen_merge_with_next_text(pid, after[ni - 1]); }
            }
            _lumen_merge_with_next_text(pid, prevNid);
        },
        getAttribute:    function(n)    { var nid = this.__nid__; return _lumen_u2n(_lumen_get_attr(nid, String(n))); },
        setAttribute:    function(n, v) { var nid = this.__nid__;
            var attrName = String(n);
            var oldVal   = _lumen_u2n(_lumen_get_attr(nid, attrName));
            var newVal   = String(v);
            _lumen_set_attr(nid, attrName, newVal);
            // BUG-360: (re)compile `on<type>` content attributes into a handler
            // as soon as they are set programmatically, not just at parse time.
            if (_lumen_is_on_attr_name(attrName)) {
                _lumen_compile_and_set_on_handler(nid, attrName, newVal);
            }
            _lumen_ce_maybe_attr_changed(nid, attrName, oldVal, newVal);
        },
        removeAttribute: function(n)    { var nid = this.__nid__;
            var attrName = String(n);
            _lumen_remove_attr(nid, attrName);
            if (_lumen_is_on_attr_name(attrName)) {
                _lumen_set_on_handler(nid, attrName, null);
            }
        },
        hasAttribute:    function(n)    { var nid = this.__nid__; return _lumen_get_attr(nid, String(n)) !== undefined; },
        // DOM §4.9.2: hasAttributes() — true iff the element carries any attribute.
        hasAttributes:   function()     { var nid = this.__nid__; return _lumen_get_attr_names(nid).length > 0; },
        // DOM §4.9.2: namespaced attribute accessors. Lumen's attribute model is
        // name-only, so the namespace argument is accepted but ignored — the
        // attribute is stored and looked up under its qualified name, matching the
        // name-based getAttribute/hasAttribute lookup (BUG-309).
        getAttributeNS:    function(ns, n)    { var nid = this.__nid__; return _lumen_u2n(_lumen_get_attr(nid, String(n))); },
        setAttributeNS:    function(ns, n, v) { var nid = this.__nid__;
            var attrName = String(n);
            var oldVal   = _lumen_u2n(_lumen_get_attr(nid, attrName));
            _lumen_set_attr(nid, attrName, String(v));
            _lumen_ce_maybe_attr_changed(nid, attrName, oldVal, String(v));
        },
        removeAttributeNS: function(ns, n)    { var nid = this.__nid__; _lumen_remove_attr(nid, String(n)); },
        hasAttributeNS:    function(ns, n)    { var nid = this.__nid__; return _lumen_get_attr(nid, String(n)) !== undefined; },
        // DOM LS §4.9.3: toggleAttribute(qualifiedName, force?)
        toggleAttribute: function(n, force) { var nid = this.__nid__;
            var attrName = String(n);
            var has = _lumen_get_attr(nid, attrName) !== undefined;
            if (force === undefined) {
                if (has) { _lumen_remove_attr(nid, attrName); return false; }
                _lumen_set_attr(nid, attrName, ''); return true;
            }
            if (force) {
                if (!has) _lumen_set_attr(nid, attrName, '');
                return true;
            }
            if (has) _lumen_remove_attr(nid, attrName);
            return false;
        },
        // Reflected `open` boolean attribute — shared by <details> (HTML5 §4.11.1)
        // and <dialog> (HTML5 §4.11.7).
        get open() { var nid = this.__nid__; return _lumen_get_attr(nid, 'open') !== undefined; },
        set open(v) { var nid = this.__nid__;
            if (v) { _lumen_set_attr(nid, 'open', ''); }
            else { _lumen_remove_attr(nid, 'open'); }
        },
        // HTMLDialogElement API (HTML5 §4.11.7)
        get returnValue() { var v = this.__returnValue__; return v === undefined ? '' : v; },
        set returnValue(v) { _lumen_wrapper_set_slot(this, '__returnValue__', String(v)); },
        show: function() { var nid = this.__nid__;
            _lumen_set_attr(nid, 'open', '');
        },
        showModal: function() { var nid = this.__nid__;
            _lumen_set_attr(nid, 'open', '');
            _lumen_set_attr(nid, 'data-lumen-modal', '');
            if (_lumen_modal_dialog_nids.indexOf(nid) < 0) {
                _lumen_modal_dialog_nids.push(nid);
            }
            // HTML LS §6.6.3: save the currently focused element so close() can restore it.
            _lumen_dialog_prev_focus[nid] = _lumen_last_focused_nid;
            // Focus the first [autofocus] descendant, or the dialog itself if none.
            var target = _lumen_find_autofocus_in(nid);
            _lumen_request_focus(target !== -1 ? target : nid);
        },
        close: function(rv) { var nid = this.__nid__;
            if (_lumen_get_attr(nid, 'open') === undefined) return;
            if (rv !== undefined) { _lumen_wrapper_set_slot(this, '__returnValue__', String(rv)); }
            _lumen_remove_attr(nid, 'open');
            _lumen_remove_attr(nid, 'data-lumen-modal');
            var idx = _lumen_modal_dialog_nids.indexOf(nid);
            if (idx >= 0) _lumen_modal_dialog_nids.splice(idx, 1);
            // HTML LS §6.6.3: restore focus to the element that was focused before open.
            var prev = _lumen_dialog_prev_focus[nid];
            delete _lumen_dialog_prev_focus[nid];
            if (prev !== undefined && prev !== -1) {
                _lumen_request_focus(prev);
            } else {
                _lumen_request_blur();
            }
            var closeEvt = new Event('close', { bubbles: false, cancelable: false });
            _lumen_dispatch(nid, closeEvt);
        },
        // HTML Popover API (WHATWG HTML §6.12)
        get popover() { var nid = this.__nid__;
            var v = _lumen_get_attr(nid, 'popover');
            if (v === undefined) return null;
            var norm = (v || '').toLowerCase();
            if (norm === 'manual') return 'manual';
            if (norm === 'hint') return 'hint'; // Popover API Level 2
            return 'auto';
        },
        set popover(v) { var nid = this.__nid__;
            if (v === null || v === undefined || v === false) {
                _lumen_remove_attr(nid, 'popover');
            } else {
                _lumen_set_attr(nid, 'popover', v === '' ? '' : String(v).toLowerCase());
            }
        },
        showPopover:   function()      { var nid = this.__nid__; _lumen_popover_show(nid); },
        hidePopover:   function()      { var nid = this.__nid__; _lumen_popover_hide(nid); },
        togglePopover: function(force) { var nid = this.__nid__; _lumen_popover_toggle(nid, force); },
        // Fullscreen API (WHATWG Fullscreen §4.3)
        requestFullscreen: function(options) { var nid = this.__nid__;
            var self = this;
            return new Promise(function(resolve, reject) {
                // WHATWG Fullscreen §4.3 error preconditions (BUG-390). Before
                // this list the only gate was `document.fullscreenEnabled`,
                // hardcoded true — so the reject branch was dead and every
                // refusal case (detached element, showing popover, no user
                // activation) silently entered fullscreen instead.
                var why = _lumen_fs_request_error(nid, self);
                if (why !== null) {
                    _lumen_fire_fullscreen_error(nid);
                    reject(new TypeError('requestFullscreen(): ' + why));
                    return;
                }
                // Exit previous fullscreen element if it is a different node.
                if (_fs_nid !== -1 && _fs_nid !== nid) {
                    _lumen_remove_attr(_fs_nid, _FS_ATTR);
                    var prev = _lumen_make_element(_fs_nid);
                    if (prev) { prev.dispatchEvent(new Event('fullscreenchange', { bubbles: true })); }
                }
                _fs_nid = nid;
                _lumen_set_attr(nid, _FS_ATTR, '');
                // Notify shell to enter OS fullscreen.
                if (typeof _lumen_fs_enter === 'function') { _lumen_fs_enter(nid); }
                self.dispatchEvent(new Event('fullscreenchange', { bubbles: true }));
                document.dispatchEvent(new Event('fullscreenchange'));
                resolve();
            });
        },
        requestPointerLock: function() { var nid = this.__nid__;
            var self = this;
            return new Promise(function(resolve, reject) {
                // Phase 1: set JS-side mirror of locked element for pointerLockElement getter.
                _ptr_lock_el = self;
                if (typeof _lumen_ptr_lock_request === 'function') {
                    _lumen_ptr_lock_request(nid);
                }
                self.dispatchEvent(new Event('pointerlockchange', { bubbles: true }));
                document.dispatchEvent(new Event('pointerlockchange'));
                resolve();
            });
        },
        // onfullscreenchange / onfullscreenerror live in
        // _LUMEN_EVENT_HANDLER_ATTRS instead (BUG-390) — a plain null property
        // here would be re-created empty by every _lumen_make_element call.
        onpointerlockchange: null,
        onpointerlockerror: null,
        // HTML LS §9.10 — drag-and-drop IDL attributes
        get draggable() { var nid = this.__nid__;
            var v = _lumen_get_attr(nid, 'draggable');
            if (v === undefined || v === null) return false;
            return String(v).toLowerCase() !== 'false';
        },
        set draggable(v) { var nid = this.__nid__;
            _lumen_set_attr(nid, 'draggable', v ? 'true' : 'false');
        },
        ondragstart:  null,
        ondrag:       null,
        ondragend:    null,
        ondragenter:  null,
        ondragover:   null,
        ondragleave:  null,
        ondrop:       null,
        // Pointer Events Level 3 §4.1 — pointer capture
        ongotpointercapture:  null,
        onlostpointercapture: null,
        setPointerCapture: function(pointerId) { var nid = this.__nid__;
            // Spec: InvalidStateError if element is not connected — skip check for Phase 0
            if (typeof _lumen_set_capture_state === 'function') {
                _lumen_set_capture_state(nid);
            }
            _lumen_dispatch_capture_event(nid, 'gotpointercapture');
        },
        releasePointerCapture: function(pointerId) { var nid = this.__nid__;
            if (typeof _lumen_release_capture_state === 'function') {
                _lumen_release_capture_state();
            }
            _lumen_dispatch_capture_event(nid, 'lostpointercapture');
        },
        hasPointerCapture: function(pointerId) { var nid = this.__nid__;
            if (typeof _lumen_get_capture_nid === 'function') {
                return _lumen_get_capture_nid() === nid;
            }
            return false;
        },
        appendChild:     function(c) { var nid = this.__nid__;
            // BUG-325: DOM §4.2.3 pre-insert validity — Text/Comment (both
            // wrapped here via `_lumen_make_element`, sharing this literal)
            // are CharacterData and can never have children.
            if (_lumen_is_text_node(nid) || _lumen_is_comment_node(nid)) {
                throw _lumen_character_data_insertion_error();
            }
            if (!c || c.__nid__ === undefined) return c;
            if (c.__isDocumentFragment__) {
                // DOM LS §4.2.4: fragment append moves all children, not the fragment itself.
                var kids = _lumen_get_children(c.__nid__).slice();
                for (var _fi = 0; _fi < kids.length; _fi++) {
                    _lumen_append_child(nid, kids[_fi]);
                    _lumen_ce_maybe_connected(_lumen_make_element(kids[_fi]));
                }
            } else {
                _lumen_append_child(nid, c.__nid__);
                _lumen_ce_maybe_connected(c);
            }
            _lumen_fire_slotchange(nid);
            return c;
        },
        removeChild:     function(c) { var nid = this.__nid__;
            if (c && c.__nid__ !== undefined) {
                _lumen_remove_child(nid, c.__nid__);
                _lumen_ce_maybe_disconnected(c);
                _lumen_fire_slotchange(nid);
            }
            return c;
        },
        // ── ChildNode mixin (DOM LS §4.2.6) ─────────────────────────────────────
        // Removes this element from its parent — except on a <select>, where
        // `remove(index)` is HTMLSelectElement's option remover (HTML LS §4.10.7)
        // and only the argument-less call is `ChildNode.remove()`. The two have to
        // be distinguished here rather than on `HTMLSelectElement.prototype`,
        // because this own property would shadow any prototype method (BUG-383).
        remove: function(index) { var nid = this.__nid__;
            if (index !== undefined
                && (_lumen_get_tag_name(nid) || '').toUpperCase() === 'SELECT') {
                var opts = _lumen_select_options(nid);
                var oi = Math.trunc(Number(index));
                if (!isFinite(oi) || oi < 0 || oi >= opts.length) return;
                var op_parent = _lumen_u2n(_lumen_get_parent(opts[oi]));
                if (op_parent !== null) _lumen_remove_child(op_parent, opts[oi]);
                return;
            }
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid !== null) {
                _lumen_remove_child(pid, nid);
                _lumen_ce_maybe_disconnected(this);
            }
        },
        // Inserts nodes immediately before this element.
        before: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            for (var _bi = 0; _bi < arguments.length; _bi++) {
                var _bn = arguments[_bi];
                if (typeof _bn === 'string') {
                    var _btn = _lumen_create_text_node(_bn);
                    _lumen_insert_before(pid, _btn, nid);
                } else if (_bn && _bn.__nid__ !== undefined) {
                    _lumen_insert_before(pid, _bn.__nid__, nid);
                }
            }
        },
        // Inserts nodes immediately after this element.
        after: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            var ch = _lumen_get_children(pid);
            var idx = ch.indexOf(nid);
            var nextSib = (idx >= 0 && idx + 1 < ch.length) ? ch[idx + 1] : null;
            for (var _ai = 0; _ai < arguments.length; _ai++) {
                var _an = arguments[_ai];
                if (typeof _an === 'string') {
                    var _atn = _lumen_create_text_node(_an);
                    if (nextSib !== null) { _lumen_insert_before(pid, _atn, nextSib); }
                    else { _lumen_append_child(pid, _atn); }
                } else if (_an && _an.__nid__ !== undefined) {
                    if (nextSib !== null) { _lumen_insert_before(pid, _an.__nid__, nextSib); }
                    else { _lumen_append_child(pid, _an.__nid__); }
                }
            }
        },
        // Replaces this element with the given nodes/strings.
        replaceWith: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            var ch = _lumen_get_children(pid);
            var idx = ch.indexOf(nid);
            var nextSib = (idx >= 0 && idx + 1 < ch.length) ? ch[idx + 1] : null;
            _lumen_remove_child(pid, nid);
            _lumen_ce_maybe_disconnected(this);
            for (var _ri = 0; _ri < arguments.length; _ri++) {
                var _rn = arguments[_ri];
                if (typeof _rn === 'string') {
                    var _rtn = _lumen_create_text_node(_rn);
                    if (nextSib !== null) { _lumen_insert_before(pid, _rtn, nextSib); }
                    else { _lumen_append_child(pid, _rtn); }
                } else if (_rn && _rn.__nid__ !== undefined) {
                    if (nextSib !== null) { _lumen_insert_before(pid, _rn.__nid__, nextSib); }
                    else { _lumen_append_child(pid, _rn.__nid__); }
                }
            }
        },
        // ── ParentNode extensions (DOM LS §4.2.5) ───────────────────────────────
        // Inserts nodes before the first child of this element.
        prepend: function() { var nid = this.__nid__;
            var ch = _lumen_get_children(nid);
            var firstChild = ch.length > 0 ? ch[0] : null;
            for (var _pi = 0; _pi < arguments.length; _pi++) {
                var _pn = arguments[_pi];
                if (typeof _pn === 'string') {
                    var _ptn = _lumen_create_text_node(_pn);
                    if (firstChild !== null) { _lumen_insert_before(nid, _ptn, firstChild); }
                    else { _lumen_append_child(nid, _ptn); }
                } else if (_pn && _pn.__nid__ !== undefined) {
                    if (firstChild !== null) { _lumen_insert_before(nid, _pn.__nid__, firstChild); }
                    else { _lumen_append_child(nid, _pn.__nid__); }
                }
            }
        },
        // ParentNode.append (DOM LS §4.2.5): appends nodes/strings as the last children.
        append: function() { var nid = this.__nid__;
            for (var _ai = 0; _ai < arguments.length; _ai++) {
                var _an = arguments[_ai];
                if (typeof _an === 'string') {
                    _lumen_append_child(nid, _lumen_create_text_node(_an));
                } else if (_an && _an.__nid__ !== undefined) {
                    _lumen_append_child(nid, _an.__nid__);
                }
            }
        },
        // HTML LS 4.9.2 (old-fashioned but conforming markup) — insertAdjacent{Text,Element}.
        // Delegates to the before/after/prepend/append methods above (same silent
        // no-op-if-no-parent behavior as before/after for beforebegin/afterend).
        // Found missing (BUG-299) while diagnosing testharness.js's results
        // renderer / P2-wpt S4 (`Output.show_results` -> `get_asserts_output`)
        // calling insertAdjacentText unconditionally for every test with no
        // recorded asserts.
        insertAdjacentText: function(where, data) { var nid = this.__nid__;
            var text = String(data);
            switch (String(where).toLowerCase()) {
                case 'beforebegin': this.before(text); break;
                case 'afterbegin':  this.prepend(text); break;
                case 'beforeend':   this.append(text); break;
                case 'afterend':    this.after(text); break;
                default:
                    throw new DOMException(
                        'The value provided (' + where + ') is not one of beforebegin, ' +
                        'afterbegin, beforeend, or afterend.', 'SyntaxError');
            }
        },
        insertAdjacentElement: function(where, element) { var nid = this.__nid__;
            if (!element || element.__nid__ === undefined) return null;
            switch (String(where).toLowerCase()) {
                case 'beforebegin': this.before(element); return element;
                case 'afterbegin':  this.prepend(element); return element;
                case 'beforeend':   this.append(element); return element;
                case 'afterend':    this.after(element); return element;
                default:
                    throw new DOMException(
                        'The value provided (' + where + ') is not one of beforebegin, ' +
                        'afterbegin, beforeend, or afterend.', 'SyntaxError');
            }
        },
        // DOM Parsing §2.6 — insertAdjacentHTML (BUG-351). Parses `html` and
        // inserts the result at the given position, same delegation pattern as
        // insertAdjacentText/insertAdjacentElement above.
        insertAdjacentHTML: function(where, html) { var nid = this.__nid__;
            var newIds = _lumen_parse_html_fragment(String(html));
            var wrapped = [];
            for (var _iahi = 0; _iahi < newIds.length; _iahi++) { wrapped.push(_lumen_make_element(newIds[_iahi])); }
            switch (String(where).toLowerCase()) {
                case 'beforebegin': this.before.apply(this, wrapped); break;
                case 'afterbegin':  this.prepend.apply(this, wrapped); break;
                case 'beforeend':   this.append.apply(this, wrapped); break;
                case 'afterend':    this.after.apply(this, wrapped); break;
                default:
                    throw new DOMException(
                        'The value provided (' + where + ') is not one of beforebegin, ' +
                        'afterbegin, beforeend, or afterend.', 'SyntaxError');
            }
        },
        // Replaces all children of this element.
        replaceChildren: function() { var nid = this.__nid__;
            var old = _lumen_get_children(nid).slice();
            for (var _rci = 0; _rci < old.length; _rci++) {
                _lumen_remove_child(nid, old[_rci]);
            }
            for (var _rni = 0; _rni < arguments.length; _rni++) {
                var _rcn = arguments[_rni];
                if (typeof _rcn === 'string') {
                    _lumen_append_child(nid, _lumen_create_text_node(_rcn));
                } else if (_rcn && _rcn.__nid__ !== undefined) {
                    _lumen_append_child(nid, _rcn.__nid__);
                }
            }
        },
        // DOM LS §4.4: cloneNode(deep) — shallow or deep copy of this element.
        cloneNode:       function(deep) { var nid = this.__nid__;
            var clone_nid = _lumen_clone_subtree(nid, deep ? 1 : 0);
            return _lumen_make_element(clone_nid);
        },
        // BUG-796: `content` used to live here, as a template-only getter answering
        // `undefined` on every other element — and this table shadows the interface
        // prototypes, so it also swallowed `HTMLMetaElement.content`. It is an IDL
        // attribute of `HTMLTemplateElement` alone and now sits on that prototype
        // (search `HTMLTemplateElement.prototype, 'content'`); nothing tag-specific
        // belongs in this shared table.
        // Scoped to this element's descendants (DOM Parentnode §4.2.5) — works on
        // detached subtrees too, unlike the document-global `_lumen_query_selector`.
        querySelector:    function(sel) { var nid = this.__nid__;
            var n = _lumen_u2n(_lumen_query_selector_scoped(nid, _lumen_sel(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll: function(sel) { var nid = this.__nid__;
            return _lumen_query_selector_all_scoped(nid, _lumen_sel(sel)).map(_lumen_make_element);
        },
        // DOM LS §4.9: getElementsByClassName(names), scoped to this element's
        // descendants (BUG-302). Static array, not a live HTMLCollection.
        getElementsByClassName: function(names) { var nid = this.__nid__;
            var sel = _lumen_class_selector(names);
            if (sel === null) return [];
            return _lumen_query_selector_all_scoped(nid, sel).map(_lumen_make_element);
        },
        // DOM LS §4.5: getElementsByTagName(qualifiedName) /
        // getElementsByTagNameNS(namespace, localName), scoped to this element's
        // descendants (BUG-416 — both were missing from the element wrapper
        // entirely, so `el.getElementsByTagName is not a function`). The
        // universal selector is used only to enumerate the subtree in tree order
        // through the native walker; the name matching itself is spec-shaped and
        // lives in the predicates above.
        getElementsByTagName: function(qualifiedName) { var nid = this.__nid__;
            return _lumen_collect_matching(
                _lumen_query_selector_all_scoped(nid, '*'),
                _lumen_tag_name_predicate(qualifiedName));
        },
        getElementsByTagNameNS: function(namespace, localName) { var nid = this.__nid__;
            return _lumen_collect_matching(
                _lumen_query_selector_all_scoped(nid, '*'),
                _lumen_tag_ns_predicate(namespace, localName));
        },
        matches: function(sel) { var nid = this.__nid__;
            return _lumen_node_matches_selector(nid, _lumen_sel(sel));
        },
        addEventListener:    function(type, fn) { var nid = this.__nid__; _lumen_add_listener(nid, type, fn); },
        removeEventListener: function(type, fn) { var nid = this.__nid__; _lumen_rm_listener(nid, type, fn); },
        // HTML LS §6.10 activation behavior: a non-cancelled, script-dispatched
        // `click` runs the same activation the native `click()` method runs
        // (form submit, link navigation, checkbox toggle, …). Native clicks
        // reach it through HTMLElement.prototype.click(); this is the
        // dispatchEvent-side counterpart for `el.dispatchEvent(new
        // MouseEvent('click', ...))` (BUG-439).
        dispatchEvent:       function(evt) { var nid = this.__nid__;
            if (!evt) return true;
            evt.target = this; evt.currentTarget = this;
            var notCancelled = _lumen_dispatch(nid, evt);
            if (notCancelled && evt.isTrusted === false && evt.type === 'click') {
                // Same activation-target walk as `click()` (BUG-837): the
                // behaviour belongs to the nearest activatable ancestor.
                var at = _lumen_activation_target(nid);
                if (at !== -1) {
                    _lumen_run_activation_behavior(at, (at === nid) ? this : _lumen_make_element(at));
                }
            }
            return notCancelled;
        },
        closest: function(sel) { var nid = this.__nid__;
            var s = _lumen_sel(sel);
            var cur = nid;
            while (cur !== undefined && cur !== null) {
                if (_lumen_node_matches_selector(cur, s)) return _lumen_make_element(cur);
                var pid = _lumen_u2n(_lumen_get_parent(cur));
                cur = pid !== null ? pid : null;
            }
            return null;
        },
        attachShadow: function(init) { var nid = this.__nid__;
            var m = (init && init.mode === 'closed') ? 'closed' : 'open';
            var sr_nid = _lumen_attach_shadow(nid, m);
            return _lumen_make_shadow_root(sr_nid, m, nid);
        },
        getBoundingClientRect: function() { var nid = this.__nid__;
            var r = _lumen_get_bounding_rect(nid);
            if (!r) { return { x:0, y:0, width:0, height:0, top:0, right:0, bottom:0, left:0 }; }
            return { x: r[0], y: r[1], width: r[2], height: r[3],
                     top: r[1], left: r[0], right: r[0]+r[2], bottom: r[1]+r[3] };
        },
        // `src` used to live here as an own property on EVERY element (BUG-305).
        // It is now one row of the reflection table (BUG-383) installed on the
        // interfaces that actually have the attribute — `<img>`/`<script>`/
        // `<iframe>`/`<source>`/`<track>`/`<video>`/`<input type=image>` — so
        // `document.createElement('div').src` is `undefined` again, as it is in
        // every browser. `<audio>` keeps its own `src` accessor (`audio_element.rs`),
        // which shadows the prototype one and drives the media loader.
        get offsetWidth()  { var nid = this.__nid__; var r = _lumen_get_bounding_rect(nid); return r ? r[2] : 0; },
        get offsetHeight() { var nid = this.__nid__; var r = _lumen_get_bounding_rect(nid); return r ? r[3] : 0; },
        get offsetLeft()   { var nid = this.__nid__; var r = _lumen_get_bounding_rect(nid); return r ? r[0] : 0; },
        get offsetTop()    { var nid = this.__nid__; var r = _lumen_get_bounding_rect(nid); return r ? r[1] : 0; },
        get clientWidth()  { var nid = this.__nid__; var r = _lumen_get_bounding_rect(nid); return r ? r[2] : 0; },
        get clientHeight() { var nid = this.__nid__; var r = _lumen_get_bounding_rect(nid); return r ? r[3] : 0; },
        get scrollLeft() { var nid = this.__nid__;
            var s = _lumen_get_scroll_state(nid); return s ? s[0] : 0;
        },
        set scrollLeft(v) { var nid = this.__nid__; _lumen_request_scroll(nid, +v, _lumen_get_scroll_state(nid) ? _lumen_get_scroll_state(nid)[1] : 0); },
        get scrollTop() { var nid = this.__nid__;
            var s = _lumen_get_scroll_state(nid); return s ? s[1] : 0;
        },
        set scrollTop(v) { var nid = this.__nid__; _lumen_request_scroll(nid, _lumen_get_scroll_state(nid) ? _lumen_get_scroll_state(nid)[0] : 0, +v); },
        get scrollWidth()  { var nid = this.__nid__; var s = _lumen_get_scroll_state(nid); return s ? s[2] : 0; },
        get scrollHeight() { var nid = this.__nid__; var s = _lumen_get_scroll_state(nid); return s ? s[3] : 0; },
        scrollTo: function(x, y) { var nid = this.__nid__;
            if (typeof x === 'object' && x !== null) { y = x.top || 0; x = x.left || 0; }
            _lumen_request_scroll(nid, +x, +y);
        },
        scrollBy: function(x, y) { var nid = this.__nid__;
            if (typeof x === 'object' && x !== null) { y = x.top || 0; x = x.left || 0; }
            var s = _lumen_get_scroll_state(nid);
            _lumen_request_scroll(nid, (s ? s[0] : 0) + (+x), (s ? s[1] : 0) + (+y));
        },
        scrollIntoView: function() { var nid = this.__nid__;
            // Scroll the nearest ancestor scroll container to make this element visible.
            var r = _lumen_get_bounding_rect(nid);
            if (!r) return;
            var parent = _lumen_u2n(_lumen_get_parent(nid));
            while (parent !== null && parent !== undefined) {
                var ps = _lumen_get_scroll_state(parent);
                if (ps) {
                    var pr = _lumen_get_bounding_rect(parent);
                    if (pr) { _lumen_request_scroll(parent, r[0] - pr[0], r[1] - pr[1]); }
                    return;
                }
                parent = _lumen_u2n(_lumen_get_parent(parent));
            }
            // CSSOM-View §14: the viewport is the last scrolling box of the
            // chain, so an element with no scrollable ancestor scrolls the PAGE
            // (BUG-821, second facet — this used to fall off the loop and do
            // nothing at all, which is most elements on most pages). Layout
            // rects are document coordinates, so `r[1]` is already the page
            // offset that puts the element's top edge at the top of the
            // viewport — the `block: 'start'` default. The arguments stay
            // ignored here as they are for the container branch (BUG-479).
            _lumen_request_page_scroll(r[1], 0);
        },
        // ── Focus-related IDL reflection (HTML LS §6.6, BUG-381) ─────────────
        // `tabIndex` reflects the `tabindex` content attribute; with the
        // attribute absent or unparseable the default is 0 for elements that
        // are focusable anyway and −1 for everything else. `<body>`/`<html>`
        // report −1 even though a script may focus them, matching browsers.
        // BUG-452: `tabIndex` is a hand-written accessor rather than a row of
        // the `_lumen_define_reflection` table, so it did not inherit that
        // table's `long` range guard — `tabindex="2147483648"` read back
        // verbatim instead of falling back to the default.
        get tabIndex() { var nid = this.__nid__;
            var parsed = _lumen_parse_integer(_lumen_u2n(_lumen_get_attr(nid, 'tabindex')));
            if (parsed !== null && parsed >= _LUMEN_LONG_MIN && parsed <= _LUMEN_LONG_MAX) return parsed;
            var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
            if (tag === 'BODY' || tag === 'HTML') return -1;
            return _lumen_is_focusable(nid) ? 0 : -1;
        },
        // The SETTER takes a WebIDL `long`, so its argument is converted with
        // ToNumber + truncation, NOT parsed with the content-attribute rules:
        // `el.tabIndex = '3px'` is `NaN` → 0, where the string `tabindex="3px"`
        // is 3. Using the parser here made the two agree on a value the spec
        // gives different answers for, and disagreed with the `long` setter of
        // the reflection table next door on the very same conversion.
        set tabIndex(v) { var nid = this.__nid__;
            var n = Number(v);
            n = isFinite(n) ? Math.trunc(n) : 0;
            if (n < _LUMEN_LONG_MIN || n > _LUMEN_LONG_MAX) n = 0;
            _lumen_set_attr(nid, 'tabindex', String(n));
        },
        // Boolean `autofocus` content attribute (HTML LS §6.6.6).
        get autofocus() { var nid = this.__nid__; return _lumen_has_attr(nid, 'autofocus'); },
        set autofocus(v) { var nid = this.__nid__;
            if (v) { _lumen_set_attr(nid, 'autofocus', ''); }
            else { _lumen_remove_attr(nid, 'autofocus'); }
        },
        // ── HTMLInputElement / HTMLTextAreaElement / HTMLSelectElement properties ──
        // `type` and `name` used to be own properties here, reflected for every
        // element alike; they are now table rows per interface (BUG-383), which
        // is what gives `<button>` its `submit` default and `<textarea>`/`<select>`
        // their fixed `type` strings.
        //
        // `value` and `checked`, by contrast, are NOT plain reflection: they are
        // the *current* value/checkedness, which the content attribute only seeds
        // (HTML LS §4.10.5.4 «dirty value flag»). They stay here as accessors,
        // keyed by nid so they survive wrapper re-creation. The content
        // attribute behind each one is reachable as `defaultValue`/`defaultChecked`
        // from the reflection table.
        //
        // Where the current value is *stored* differs: for `<input>`/`<textarea>`
        // it is document-side (`Document::dirty_values`, BUG-441) because layout
        // and form submission read it from there; the rest still use the JS-side
        // `_input_values` map. `checked` is document-side too
        // (`Document::dirty_checkedness`, BUG-444), for the same reason —
        // checkbox painting, `:checked`/`:indeterminate` matching and form
        // submission all read it from Rust.
        get value() { var nid = this.__nid__;
            var tag0 = (_lumen_get_tag_name(nid) || '').toUpperCase();
            // BUG-441: for the two text-entry controls the current value lives
            // in the document (`Document::dirty_values`), the one place layout
            // and form submission read — not in a JS-side shadow they cannot
            // see. Everything else keeps the `_input_values` map.
            if (tag0 === 'INPUT' || tag0 === 'TEXTAREA') {
                var dv = _lumen_u2n(_lumen_get_dirty_value(nid));
                if (dv !== null) return String(dv);
                if (tag0 === 'TEXTAREA') return _lumen_get_text_content(nid);
                var dav = _lumen_u2n(_lumen_get_attr(nid, 'value'));
                return dav !== null ? dav : '';
            }
            if (_input_values[nid] !== undefined) return _input_values[nid];
            var tag = tag0;
            // HTML LS §4.10.10: an <option> with no `value` attribute takes its
            // value from its text; §4.10.7: a <select>'s value is that of its
            // first selected option. Both are needed for `select.value` and
            // `select.options[i].value` to agree.
            if (tag === 'OPTION') {
                var ov = _lumen_u2n(_lumen_get_attr(nid, 'value'));
                if (ov !== null) return String(ov);
                return _lumen_option_text(nid);
            }
            if (tag === 'SELECT') {
                var _si = _lumen_select_selected_index(nid);
                if (_si === -1) return '';
                return _lumen_make_element(_lumen_select_options(nid)[_si]).value;
            }
            if (tag === 'TEXTAREA') return _lumen_get_text_content(nid);
            var av = _lumen_u2n(_lumen_get_attr(nid, 'value'));
            return av !== null ? av : '';
        },
        set value(v) { var nid = this.__nid__;
            var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
            if (tag === 'SELECT') { _lumen_select_set_value(nid, String(v)); return; }
            // BUG-441: raise the control's dirty value flag in the document, so
            // the field re-renders with the new text and submits it. The
            // `value` content attribute is left alone on purpose — it is the
            // default value `defaultValue`/`form.reset()` restore.
            if (tag === 'INPUT' || tag === 'TEXTAREA') {
                _lumen_set_dirty_value(nid, String(v));
                return;
            }
            _input_values[nid] = String(v);
        },
        // BUG-444: the control's current checkedness lives document-side
        // (`Document::dirty_checkedness`) — the `checked` content attribute
        // is only its default, reflected separately as `defaultChecked`.
        get checked() { var nid = this.__nid__;
            var dc = _lumen_u2n(_lumen_get_dirty_checked(nid));
            return dc !== null ? dc : _lumen_has_attr(nid, 'checked');
        },
        set checked(v) { var nid = this.__nid__;
            _lumen_set_dirty_checked(nid, !!v);
        },
        // ── Constraint Validation API (HTML LS §4.10.21) ─────────────────────────
        get validity() { var nid = this.__nid__; return _compute_validity(this); },
        get validationMessage() { var nid = this.__nid__;
            var cm = _validity_msg[nid] || '';
            if (cm) return cm;
            var vs = _compute_validity(this);
            if (vs.valueMissing)    return 'Please fill out this field.';
            if (vs.typeMismatch)    return 'Please enter a valid ' + (this.type || 'value') + '.';
            if (vs.patternMismatch) return 'Please match the requested format.';
            if (vs.tooLong)         return 'Please shorten this text.';
            if (vs.tooShort)        return 'Please lengthen this text.';
            if (vs.rangeUnderflow)  return 'Value must be >= ' + this.getAttribute('min') + '.';
            if (vs.rangeOverflow)   return 'Value must be <= ' + this.getAttribute('max') + '.';
            if (vs.stepMismatch)    return 'Please enter a valid value.';
            return '';
        },
        // true when the element participates in constraint validation
        get willValidate() { var nid = this.__nid__;
            var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
            if (tag !== 'INPUT' && tag !== 'TEXTAREA' && tag !== 'SELECT') return false;
            var t = (this.type || '').toLowerCase();
            if (t === 'hidden' || t === 'button' || t === 'submit' || t === 'reset' || t === 'image') return false;
            if (this.hasAttribute('disabled')) return false;
            return true;
        },
        // Fires 'invalid' event and returns false if the element fails constraint validation.
        checkValidity: function() { var nid = this.__nid__;
            if (!this.willValidate) return true;
            var vs = this.validity;
            if (!vs.valid) {
                var ev = new Event('invalid', { bubbles: false, cancelable: true });
                this.dispatchEvent(ev);
                return false;
            }
            return true;
        },
        // Like checkValidity(); may show the browser's default validation UI (Phase 0: same as checkValidity).
        reportValidity: function() { var nid = this.__nid__; return this.checkValidity(); },
        // Overrides validity with a custom message; empty string clears the override (HTML LS §4.10.21.2).
        setCustomValidity: function(msg) { var nid = this.__nid__;
            var m = String(msg);
            if (m) _validity_msg[nid] = m;
            else delete _validity_msg[nid];
        },
        // HTML LS §4.10.5.1.14: showPicker() — programmatically opens the
        // UA-provided picker for applicable input types.
        // Phase 0: fires a synthetic 'click' event so shell integrations can hook it;
        // throws NotSupportedError for types that have no picker.
        showPicker: function() { var nid = this.__nid__;
            var t = (this.type || 'text').toLowerCase();
            var pickerTypes = ['color', 'date', 'datetime-local', 'month', 'time', 'week', 'file'];
            var supported = false;
            for (var _pi = 0; _pi < pickerTypes.length; _pi++) {
                if (pickerTypes[_pi] === t) { supported = true; break; }
            }
            if (!supported) {
                var err = new Error('showPicker() is not supported for type ' + t);
                err.name = 'NotSupportedError';
                throw err;
            }
            if (this.disabled) {
                var err2 = new Error('showPicker() called on a disabled element');
                err2.name = 'InvalidStateError';
                throw err2;
            }
            // Fire a click event; shell / test code can listen to open a native picker.
            this.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }));
        },
        // `elements` (was a plain `Array` on every element) and `noValidate` moved
        // to `HTMLFormElement.prototype`/`HTMLFieldSetElement.prototype` — the
        // former is now a real `HTMLFormControlsCollection`, the latter a row of
        // the reflection table (BUG-383).
        // DOM LS §4.2.4: insertBefore(newNode, refNode) — inserts before refNode (or appends if null).
        insertBefore: function(newNode, refNode) { var nid = this.__nid__;
            if (!newNode || newNode.__nid__ === undefined) return newNode;
            if (!refNode || refNode.__nid__ === undefined) {
                return this.appendChild(newNode);
            }
            if (newNode.__isDocumentFragment__) {
                var kids = _lumen_get_children(newNode.__nid__).slice();
                for (var _ib = 0; _ib < kids.length; _ib++) {
                    _lumen_insert_before(nid, kids[_ib], refNode.__nid__);
                    _lumen_ce_maybe_connected(_lumen_make_element(kids[_ib]));
                }
            } else {
                _lumen_insert_before(nid, newNode.__nid__, refNode.__nid__);
                _lumen_ce_maybe_connected(newNode);
            }
            return newNode;
        },
        // HTMLSlotElement (DOM LS §4.2.2.2): applicable only on <slot> elements.
        // assignedNodes({flatten}) — returns the assigned light-DOM nodes for this slot.
        // Phase 0: returns the host's direct children that match this slot's `name` attribute.
        assignedNodes: function(opts) { var nid = this.__nid__;
            if ((_lumen_get_tag_name(nid) || '').toUpperCase() !== 'SLOT') return [];
            var slot_name = _lumen_u2n(_lumen_get_attr(nid, 'name')) || '';
            var host_nid  = _lumen_u2n(_lumen_get_shadow_root_host(nid));
            if (host_nid === null) return [];
            var host_kids = _lumen_get_children(host_nid);
            var out = [];
            for (var _sn = 0; _sn < host_kids.length; _sn++) {
                var k = host_kids[_sn];
                var k_slot = _lumen_u2n(_lumen_get_attr(k, 'slot')) || '';
                if (k_slot === slot_name) out.push(_lumen_make_element(k));
            }
            return out;
        },
        assignedElements: function(opts) { var nid = this.__nid__;
            return this.assignedNodes(opts).filter(function(n) { return n.nodeType === 1; });
        },
        // Reflected `slot` content attribute (which shadow slot to assign this element to).
        get slot() { var nid = this.__nid__; var v = _lumen_u2n(_lumen_get_attr(nid, 'slot')); return v !== null ? v : ''; },
        set slot(v) { var nid = this.__nid__; _lumen_set_attr(nid, 'slot', String(v)); },
        // assignedSlot — the <slot> element this node is slotted into, or null.
        // Phase 0 stub: full implementation requires composed tree traversal.
        get assignedSlot() { var nid = this.__nid__; return null; },
        // ── checkVisibility (W3C Viewport API §4.1) ──────────────────────────────
        // Returns false if this element or any ancestor has display:none, is
        // disconnected, or (if options say so) has opacity:0 / visibility:hidden.
        checkVisibility: function(opts) { var nid = this.__nid__;
            var options = opts || {};
            var checkOpacity     = !!options.checkOpacity;
            var checkVisibilityCss = !!options.checkVisibilityCSS;
            var checkContentVisibility = !!options.checkContentVisibility;
            var cur = nid;
            while (cur !== null && cur !== undefined) {
                var disp = _lumen_get_computed_style(cur, 'display');
                if (disp === '' || disp === 'none') return false;
                if (checkOpacity) {
                    var op = _lumen_get_computed_style(cur, 'opacity');
                    if (op !== null && op !== '' && parseFloat(op) === 0) return false;
                }
                if (checkVisibilityCss) {
                    var vis = _lumen_get_computed_style(cur, 'visibility');
                    if (vis === 'hidden' || vis === 'collapse') return false;
                }
                if (checkContentVisibility) {
                    var cv = _lumen_get_computed_style(cur, 'content-visibility');
                    if (cv === 'hidden') return false;
                }
                cur = _lumen_u2n(_lumen_get_parent(cur));
            }
            return true;
        },
        // ── setHTMLUnsafe (WHATWG HTML LS §14.5) ─────────────────────────────────
        // Parses html as a markup fragment and replaces element children.
        // Unsafe: no sanitization (unlike Sanitizer API).
        setHTMLUnsafe: function(html) { var nid = this.__nid__;
            _lumen_set_inner_html(nid, String(html));
        },
        // ── getHTML (WHATWG HTML LS §14.5) ───────────────────────────────────────
        // Serialises element's subtree as an HTML string.
        // Phase 0: serializableShadowRoots option deferred (Shadow DOM Phase 2).
        getHTML: function(opts) { var nid = this.__nid__;
            return _lumen_get_inner_html(nid);
        },
        // ── moveBefore (DOM LS, Chrome 133+) ─────────────────────────────────────
        // Moves `node` to be the previous sibling of `child` within this element,
        // preserving the node's CSS transition / animation state.
        // Phase 0: state preservation is a no-op (animations reset on DOM move).
        moveBefore: function(node, child) { var nid = this.__nid__;
            if (!node || !node.__nid__) throw new TypeError('moveBefore: node required');
            var nodeNid = node.__nid__;
            var oldParent = _lumen_u2n(_lumen_get_parent(nodeNid));
            if (oldParent !== null) {
                _lumen_remove_child(oldParent, nodeNid);
            }
            if (child !== null && child !== undefined) {
                _lumen_insert_before(nid, nodeNid, child.__nid__);
            } else {
                _lumen_append_child(nid, nodeNid);
            }
        },
};

// ── HTMLCanvasElement (HTML LS §4.12.5) ──────────────────────────────────────
// BUG-450: `getContext`/`toDataURL`/`toBlob`/`transferControlToOffscreen`/
// `width`/`height` used to live in `_LUMEN_WRAPPER_MEMBERS`, i.e. on the shared
// per-interface prototype EVERY element wrapper chains through — so
// `'getContext' in document.createElement('div')` answered true, `div.toDataURL()`
// returned a PNG, and `div.width = 42` wrote a `width` attribute HTML LS does not
// give `<div>`. Scripts feature-detect canvas with exactly that `in` test. Six
// members of one interface belong on that interface's prototype, which is the
// move BUG-796 made for `content` and BUG-383 for `src`; `width`/`height` are
// handed to the interfaces that really own them by the reflection table in
// `web_api_shim_tail_b.js`.
//
// A wrapper's [[Prototype]] sits one link BELOW the interface prototype
// (`_lumen_wrapper_proto_for`), so a `<canvas>` still reaches these while nothing
// else does — and, unlike the old placement, a page can now patch
// `HTMLCanvasElement.prototype` and be seen by parser-built canvases.

// WebIDL brand check. Without it `HTMLCanvasElement.prototype.toDataURL.call(div)`
// would still operate on whatever node id it found on the receiver — the same
// hole BUG-449 closed for the Canvas 2D interfaces, whose state slot doubles as
// their brand.
function _lumen_canvas_nid(self) {
    var nid = (self === null || self === undefined) ? undefined : self.__nid__;
    if (nid === null || nid === undefined
        || (_lumen_get_tag_name(nid) || '').toLowerCase() !== 'canvas') {
        throw new TypeError('Illegal invocation: receiver is not an HTMLCanvasElement');
    }
    return nid;
}

// `getContext(contextId, options)` — `contextId` is a required DOMString, and
// §4.12.5 matches it by EXACT value against the context-id table. Lower-casing it
// (as this did) made `'2D'` and `'WebGL'` hand out contexts the spec refuses, and
// `contextType || ''` turned `getContext(0)` into `getContext('')` instead of the
// WebIDL string conversion `'0'`. '2d' returns a cached CanvasRenderingContext2D;
// 'webgl'/'webgl2' fall through to null here (the functional WebGL path is the
// separate `webgl_canvas` shim, which wraps this method per element). Returns
// null once control has been transferred via transferControlToOffscreen.
HTMLCanvasElement.prototype.getContext = function(contextType) {
    var nid = _lumen_canvas_nid(this);
    if (arguments.length === 0) {
        throw new TypeError("Failed to execute 'getContext' on 'HTMLCanvasElement': "
            + '1 argument required, but only 0 present.');
    }
    var t = String(contextType);
    if (typeof _lumen_canvas_is_transferred === 'function' && _lumen_canvas_is_transferred(nid)) return null;
    if (t === '2d') {
        if (_canvas2d_ctxs[nid]) return _canvas2d_ctxs[nid];
        var d = _lumen_canvas_dims(nid);
        _lumen_canvas2d_create(nid, d[0], d[1]);
        var c2d = _lumen_make_canvas2d_ctx(this, nid);
        _canvas2d_ctxs[nid] = c2d;
        return c2d;
    }
    // 'bitmaprenderer' returns an ImageBitmapRenderingContext (HTML LS §4.12.5.1):
    // transferFromImageBitmap(bitmap) replaces this canvas's displayed bitmap wholesale
    // (no drawing operations of its own, unlike '2d').
    if (t === 'bitmaprenderer') {
        if (_canvas_bitmaprenderer_ctxs[nid]) return _canvas_bitmaprenderer_ctxs[nid];
        var bd = _lumen_canvas_dims(nid);
        _lumen_canvas2d_create(nid, bd[0], bd[1]);
        var brctx = {
            canvas: this,
            transferFromImageBitmap: function(bitmap) {
                if (bitmap === null) {
                    _lumen_canvas2d_clear_rect(nid, 0, 0, _lumen_canvas_dims(nid)[0], _lumen_canvas_dims(nid)[1]);
                    return;
                }
                if (!bitmap || typeof bitmap.__canvas_id__ !== 'number') {
                    throw new TypeError('transferFromImageBitmap: argument is not an ImageBitmap');
                }
                var ok = _lumen_bitmaprenderer_transfer_from_image_bitmap(nid, bitmap.__canvas_id__);
                if (!ok) {
                    throw new DOMException('transferFromImageBitmap: the ImageBitmap has been detached', 'InvalidStateError');
                }
            }
        };
        _canvas_bitmaprenderer_ctxs[nid] = brctx;
        return brctx;
    }
    // 'webgpu' returns a GPUCanvasContext bound to this canvas. configure() allocates a
    // render-target texture; rendered frames present into the canvas:{nid} 2D buffer the
    // shell composites. Returns null without the WebGPU shim (Phase 0 builds).
    if (t === 'webgpu') {
        if (_canvas_webgpu_ctxs[nid]) return _canvas_webgpu_ctxs[nid];
        if (typeof GPUCanvasContext !== 'function') return null;
        var wd = _lumen_canvas_dims(nid);
        _lumen_canvas2d_create(nid, wd[0], wd[1]);
        var gctx = new GPUCanvasContext(this);
        _canvas_webgpu_ctxs[nid] = gctx;
        return gctx;
    }
    return null;
};

// HTMLCanvasElement.transferControlToOffscreen (HTML LS §4.12.14).
// Transfers the canvas bitmap to a new OffscreenCanvas and prevents future
// getContext() calls. The returned OffscreenCanvas can be sent to a Worker
// via postMessage with a transfer list.
HTMLCanvasElement.prototype.transferControlToOffscreen = function() {
    var nid = _lumen_canvas_nid(this);
    if (typeof _lumen_canvas_is_transferred === 'function' && _lumen_canvas_is_transferred(nid)) {
        throw new DOMException('Canvas control already transferred', 'InvalidStateError');
    }
    if (_canvas2d_ctxs[nid]) {
        throw new DOMException('Canvas already has an active 2D context', 'InvalidStateError');
    }
    var d = _lumen_canvas_dims(nid);
    _lumen_canvas2d_create(nid, d[0], d[1]);
    var jsonStr = _lumen_canvas_transfer_control_to_offscreen(nid);
    var obj = JSON.parse(jsonStr);
    // Create an OffscreenCanvas JS object wrapping the pre-created native canvas.
    // We set __canvas_id__ directly instead of calling the constructor so the
    // native side does not allocate a second backing buffer.
    var oc = Object.create(OffscreenCanvas.prototype);
    oc.__canvas_id__ = obj.__canvas_id__;
    oc.width = obj.width;
    oc.height = obj.height;
    oc._2d_context = null;
    return oc;
};

// Privacy: blank data URL defeats canvas pixel-hash fingerprinting (ADR-007).
HTMLCanvasElement.prototype.toDataURL = function() {
    _lumen_canvas_nid(this);
    return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
};
HTMLCanvasElement.prototype.toBlob = function(cb) {
    _lumen_canvas_nid(this);
    if (typeof cb === 'function') cb(null);
};

// `width`/`height` reflect the content attributes as `unsigned long` with the
// interface defaults 300 and 150 (§4.12.5). Setting resizes the backing bitmap,
// which clears it. The setter follows HTML LS §2.6.2 reflection rather than the
// old `parseInt`: a value outside [0, 2147483647] — which is what a negative or
// non-numeric argument becomes — writes the DEFAULT to the content attribute,
// where before `canvas.width = -1` wrote `width="0"` while the getter answered
// 300, i.e. the attribute and the IDL attribute disagreed.
function _lumen_canvas_define_dim(name, index, def) {
    Object.defineProperty(HTMLCanvasElement.prototype, name, {
        get: function() { return _lumen_canvas_dims(_lumen_canvas_nid(this))[index]; },
        set: function(v) {
            var nid = _lumen_canvas_nid(this);
            var n = Number(v);
            n = isFinite(n) ? Math.trunc(n) : 0;
            if (n < 0 || n > 2147483647) n = def;
            _lumen_set_attr(nid, name, String(n));
            if (_canvas2d_ctxs[nid]) {
                var d = _lumen_canvas_dims(nid);
                _lumen_canvas2d_resize(nid, d[0], d[1]);
            }
        },
        enumerable: true, configurable: true,
    });
}
_lumen_canvas_define_dim('width', 0, 300);
_lumen_canvas_define_dim('height', 1, 150);

    // ── contentEditable / isContentEditable (HTML LS §6.9.3) ────────────────
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'contentEditable', {
        get: function() { var nid = this.__nid__;
            var v = _lumen_u2n(_lumen_get_attr(nid, 'contenteditable'));
            if (v === null) return 'inherit';
            if (v === '' || v.toLowerCase() === 'true') return 'true';
            if (v.toLowerCase() === 'plaintext-only') return 'plaintext-only';
            if (v.toLowerCase() === 'false') return 'false';
            return 'inherit';
        },
        set: function(v) { var nid = this.__nid__;
            var s = String(v).toLowerCase();
            if (s === 'true') _lumen_set_attr(nid, 'contenteditable', 'true');
            else if (s === 'false') _lumen_set_attr(nid, 'contenteditable', 'false');
            else if (s === 'plaintext-only') _lumen_set_attr(nid, 'contenteditable', 'plaintext-only');
            else _lumen_remove_attr(nid, 'contenteditable');
        },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'isContentEditable', {
        get: function() { var nid = this.__nid__; return _lumen_is_contenteditable(nid); },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'shadowRoot', {
        get: function() { var nid = this.__nid__;
            var sr_nid = _lumen_u2n(_lumen_get_shadow_root(nid));
            return sr_nid !== null ? _lumen_make_shadow_root(sr_nid, 'open', nid) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'parentElement', {
        get: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            return pid !== null ? _lumen_make_element(pid) : null;
        },
        enumerable: false, configurable: true,
    });
    // DOM §4.4 Node.parentNode (BUG-310): the parent node, or null at the tree
    // root. Mirrors `parentElement` — the shim wraps every parent (element or
    // container) through `_lumen_make_element`, so `.parentNode.children` works.
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'parentNode', {
        get: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            return pid !== null ? _lumen_make_element(pid) : null;
        },
        enumerable: false, configurable: true,
    });
    // DOM §4.4 Node.isConnected (BUG-311): true when the node's shadow-inclusive
    // root is the document. In the flat shim tree that means `documentElement`
    // (<html>) is on the node's ancestor chain (or is the node itself) — a
    // detached subtree never reaches it, so its topmost ancestor is some
    // orphan node and `isConnected` is false.
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'isConnected', {
        get: function() { var nid = this.__nid__;
            var htmlId = _lumen_u2n(_lumen_get_html_element());
            if (htmlId === null) return false;
            var cur = nid;
            while (cur !== null) {
                if (cur === htmlId) return true;
                cur = _lumen_u2n(_lumen_get_parent(cur));
            }
            return false;
        },
        enumerable: false, configurable: true,
    });
    // DOM §4.2.6 ParentNode.children — element-only live HTMLCollection (BUG-310).
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'children', {
        get: function() { var nid = this.__nid__; return _lumen_make_html_collection(nid); },
        enumerable: false, configurable: true,
    });
    // BUG-327: `.childNodes` was entirely absent on the ordinary live
    // element/text/comment wrapper (only `document`/`DocumentFragment`/detached
    // `CharacterData` had it) — `Node-childNodes.html`, `Node.hasChildNodes()`
    // (added above) and everything that walks a live subtree via `.childNodes`
    // threw or silently reported an empty tree.
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'childNodes', {
        get: function() { var nid = this.__nid__; return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'firstChild', {
        get: function() { var nid = this.__nid__;
            var ch = _lumen_get_children(nid);
            return ch.length > 0 ? _lumen_make_element(ch[0]) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'lastChild', {
        get: function() { var nid = this.__nid__;
            var ch = _lumen_get_children(nid);
            return ch.length > 0 ? _lumen_make_element(ch[ch.length - 1]) : null;
        },
        enumerable: false, configurable: true,
    });
    // DOM §4.4 Node.replaceChild(new, old) — не было вовсе: реактивные
    // библиотеки заменяют узел на месте именно им (`e.replaceChild is not a
    // function` — форма входа id.tbank.ru, 2026-08-17). Возвращает СТАРЫЙ узел,
    // как требует спека.
    _LUMEN_WRAPPER_MEMBERS.replaceChild = function(newChild, oldChild) { var nid = this.__nid__;
        if (!newChild || !oldChild || newChild.__nid__ === undefined || oldChild.__nid__ === undefined) {
            throw new TypeError('replaceChild: both arguments must be nodes');
        }
        _lumen_insert_before(nid, newChild.__nid__, oldChild.__nid__);
        _lumen_remove_child(nid, oldChild.__nid__);
        return oldChild;
    };
    // DOM §4.4 Node.isSameNode / isEqualNode. `isSameNode` нельзя свести к
    // `===`: обёртка узла создаётся заново на каждое обращение, поэтому
    // сравниваются идентификаторы узлов дерева.
    _LUMEN_WRAPPER_MEMBERS.isSameNode = function(other) { var nid = this.__nid__;
        if (!other) return false;
        return _lumen_tree_nid(other) === nid;
    };
    _LUMEN_WRAPPER_MEMBERS.isEqualNode = function(other) { var nid = this.__nid__;
        if (!other) return false;
        var onid = _lumen_tree_nid(other);
        if (onid === null) return false;
        if (onid === nid) return true;
        // Структурное равенство (DOM §4.4): тип, имя и сериализация поддерева.
        if (this.nodeType !== other.nodeType) return false;
        if (this.nodeName !== other.nodeName) return false;
        return _lumen_get_outer_html(nid) === _lumen_get_outer_html(onid);
    };
    // DOM §4.4 Node.getRootNode() — корень дерева (документ или корень
    // отсоединённого поддерева).
    _LUMEN_WRAPPER_MEMBERS.getRootNode = function() { var nid = this.__nid__;
        var cur = nid;
        while (true) {
            var pid = _lumen_u2n(_lumen_get_parent(cur));
            if (pid === null) break;
            cur = pid;
        }
        if (cur === _lumen_root_nid) return document;
        return _lumen_make_element(cur);
    };
    // DOM §4.4 Node.normalize() — склеить соседние текстовые узлы и выбросить
    // пустые, рекурсивно по поддереву.
    _LUMEN_WRAPPER_MEMBERS.normalize = function() { var nid = this.__nid__;
        var walk = function(id) {
            var kids = _lumen_get_children(id);
            var prevText = null;
            for (var i = 0; i < kids.length; i++) {
                var k = kids[i];
                if (_lumen_is_text_node(k)) {
                    var data = _lumen_get_text_content(k);
                    if (data === '') { _lumen_remove_child(id, k); continue; }
                    if (prevText !== null) {
                        _lumen_set_text_content(prevText, _lumen_get_text_content(prevText) + data);
                        _lumen_remove_child(id, k);
                        continue;
                    }
                    prevText = k;
                } else {
                    prevText = null;
                    walk(k);
                }
            }
        };
        walk(nid);
    };
    // DOM §4.4 Node.nextSibling / Node.previousSibling — соседи ЛЮБОГО типа,
    // включая текстовые узлы и комментарии. Их не было вовсе: обёртка знала
    // только `nextElementSibling`/`previousElementSibling`, и обход
    // `firstChild` → `nextSibling` (компилированные шаблоны Solid/lit, обход
    // смешанного содержимого) упирался в `undefined`.
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'nextSibling', {
        get: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return null;
            var sibs = _lumen_get_children(pid);
            var idx = sibs.indexOf(nid);
            return (idx >= 0 && idx + 1 < sibs.length) ? _lumen_make_element(sibs[idx + 1]) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'previousSibling', {
        get: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return null;
            var sibs = _lumen_get_children(pid);
            var idx = sibs.indexOf(nid);
            return (idx > 0) ? _lumen_make_element(sibs[idx - 1]) : null;
        },
        enumerable: false, configurable: true,
    });
    // DOM §4.2.6/§4.2.7 ParentNode/NonDocumentTypeChildNode element traversal (BUG-310).
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'childElementCount', {
        get: function() { var nid = this.__nid__; return _lumen_element_child_nids(nid).length; },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'firstElementChild', {
        get: function() { var nid = this.__nid__;
            var ch = _lumen_element_child_nids(nid);
            return ch.length > 0 ? _lumen_make_element(ch[0]) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'lastElementChild', {
        get: function() { var nid = this.__nid__;
            var ch = _lumen_element_child_nids(nid);
            return ch.length > 0 ? _lumen_make_element(ch[ch.length - 1]) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'nextElementSibling', {
        get: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return null;
            var sibs = _lumen_element_child_nids(pid);
            var idx = sibs.indexOf(nid);
            return (idx >= 0 && idx + 1 < sibs.length) ? _lumen_make_element(sibs[idx + 1]) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'previousElementSibling', {
        get: function() { var nid = this.__nid__;
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return null;
            var sibs = _lumen_element_child_nids(pid);
            var idx = sibs.indexOf(nid);
            return (idx > 0) ? _lumen_make_element(sibs[idx - 1]) : null;
        },
        enumerable: false, configurable: true,
    });
    // Web Animations API (WAAPI Level 1) — element.animate() and getAnimations().
    _LUMEN_WRAPPER_MEMBERS.animate = function(keyframes, options) { var nid = this.__nid__;
        return _wa_element_animate(this, keyframes, options);
    };
    _LUMEN_WRAPPER_MEMBERS.getAnimations = function() { var nid = this.__nid__;
        return _wa_get_animations_for(this);
    };
    // DOM LS §4.4: `ownerDocument` must be the same `document` object by reference
    // for every node — react-dom's container-identity check (BUG-281) compares
    // `element.ownerDocument === document`. Defined non-enumerable (matching real
    // engines, where it lives on the prototype rather than as an own property):
    // `document` itself is reachable from any element via `documentElement`/`body`,
    // so an *enumerable* back-reference here would make every node object cyclic and
    // blow up code that walks own-enumerable properties (e.g. `eval()`'s return-value
    // serialization in lib.rs's `from_rq`).
    Object.defineProperty(_LUMEN_WRAPPER_MEMBERS, 'ownerDocument', {
        get: function() { var nid = this.__nid__; return document; },
        enumerable: false,
        configurable: true,
    });

// Character-data-only members. DOM §4.10 CharacterData.data / Node.nodeValue for
// live text AND comment nodes; element wrappers keep `data` free for expandos and
// `nodeValue` absent, matching the pre-existing shape. Writing routes through
// `_lumen_set_text_content`, whose MutationObserver wrap emits a `characterData`
// record (BUG-318, WPT MutationObserver-takeRecords.html).
// `CharacterData.prototype.length`/`substringData`/`appendData`/`insertData`/
// `deleteData`/`replaceData` are all built on top of this `data` accessor, so both
// Text and Comment get the full interface here.
var _LUMEN_WRAPPER_CD_MEMBERS = {
    get data()        { return _lumen_get_text_content(this.__nid__); },
    set data(v)       { _lumen_set_text_content(this.__nid__, String(v)); },
    get nodeValue()   { return _lumen_get_text_content(this.__nid__); },
    set nodeValue(v)  { _lumen_set_text_content(this.__nid__, String(v)); },
};

// BUG-360: `el.on<type>` IDL attributes (GlobalEventHandlers, elements only —
// Text/Comment nodes do not implement it). One curated accessor pair per NAME,
// shared by every element, instead of one pair per node.
var _LUMEN_WRAPPER_ON_MEMBERS = {};
for (var _ehi = 0; _ehi < _LUMEN_EVENT_HANDLER_ATTRS.length; _ehi++) {
    _lumen_define_on_handler_prop(_LUMEN_WRAPPER_ON_MEMBERS, _LUMEN_EVENT_HANDLER_ATTRS[_ehi]);
}

var _LUMEN_WRAPPER_DESCRIPTORS    = Object.getOwnPropertyDescriptors(_LUMEN_WRAPPER_MEMBERS);
var _LUMEN_WRAPPER_CD_DESCRIPTORS = Object.getOwnPropertyDescriptors(_LUMEN_WRAPPER_CD_MEMBERS);
var _LUMEN_WRAPPER_ON_DESCRIPTORS = Object.getOwnPropertyDescriptors(_LUMEN_WRAPPER_ON_MEMBERS);
var _lumen_wrapper_protos = new Map();

// A wrapper's [[Prototype]]: an interface-specific object carrying every shared
// member, whose own prototype is the interface prototype (BUG-322's chain, so
// `instanceof Element`/`HTMLDivElement`/`Text`/`CharacterData` still resolve).
// Sitting one link BELOW the interface prototype it also keeps the shadowing the
// old own-property layout gave for free — e.g. `remove` here still wins over
// `HTMLSelectElement.prototype`'s (BUG-383) — while costing one object per
// interface instead of one property set per node.
function _lumen_wrapper_proto_for(iface, isCharacterData) {
    var proto = _lumen_wrapper_protos.get(iface);
    if (proto !== undefined) return proto;
    proto = Object.create(iface);
    Object.defineProperties(proto, _LUMEN_WRAPPER_DESCRIPTORS);
    Object.defineProperties(proto, isCharacterData ? _LUMEN_WRAPPER_CD_DESCRIPTORS
                                                   : _LUMEN_WRAPPER_ON_DESCRIPTORS);
    _lumen_wrapper_protos.set(iface, proto);
    return proto;
}

// Re-points a live wrapper at another interface. `svg.rs` does this to give a
// `createElementNS` result its typed `SVG*Element` chain; a plain
// `Object.setPrototypeOf(el, Ctor.prototype)` would drop every shared member
// now that BUG-849 has moved them off the instance.
function _lumen_retarget_wrapper(el, iface) {
    if (!el || !iface) { return el; }
    Object.setPrototypeOf(el, _lumen_wrapper_proto_for(iface, false));
    return el;
}

// Per-node state that used to live in a closure variable of the builder
// (`classList`, `style`, `dataset`, `attributes`): created on first read and
// then frozen onto the instance, so `el.style === el.style` holds as it does in
// a browser — and a node no script ever asks pays nothing.
function _lumen_wrapper_slot(obj, key, factory) {
    var v = obj[key];
    if (v === undefined) {
        v = factory(obj.__nid__);
        Object.defineProperty(obj, key, { value: v, enumerable: false, configurable: true });
    }
    return v;
}
function _lumen_wrapper_set_slot(obj, key, value) {
    Object.defineProperty(obj, key,
        { value: value, enumerable: false, configurable: true, writable: true });
}

function _lumen_build_element(nid) {
    var isText    = _lumen_is_text_node(nid);
    var isComment = isText ? false : _lumen_is_comment_node(nid);
    var iface     = isText ? Text.prototype
                  : (isComment ? Comment.prototype : _lumen_element_prototype_for(nid));
    var _obj = Object.create(_lumen_wrapper_proto_for(iface, isText || isComment));
    // BUG-367: `__nid__` is the wrapper's internal arena handle, not a DOM
    // member — non-enumerable (an enumerable one was the first key of
    // `Object.keys`/`for…in`/spread/`JSON.stringify` on every node, a Lumen
    // fingerprint) and non-writable, because the whole shim resolves tree
    // mutations through `child.__nid__` and one assignment from page script
    // (`a.__nid__ = b.__nid__`) re-pointed `appendChild(a)` at node `b`.
    Object.defineProperty(_obj, '__nid__',
        { value: nid, enumerable: false, writable: false, configurable: false });
    if (!isText && !isComment) {
        // Seed the handler table from whatever `on*` content attributes this node
        // already carries (HTML-parsed markup, or a `setAttribute` call that landed
        // before this wrapper was first built) — that keeps `<div onclick="…">` and
        // `el.setAttribute('onclick', …)` behaving identically regardless of which
        // one races the wrapper into existence first (BUG-360).
        var _attrNames = _lumen_get_attr_names(nid);
        for (var _ani = 0; _ani < _attrNames.length; _ani++) {
            var _an = _attrNames[_ani];
            if (_lumen_is_on_attr_name(_an)) {
                _lumen_compile_and_set_on_handler(nid, _an, _lumen_u2n(_lumen_get_attr(nid, _an)) || '');
            }
        }
    }

    // ── HTMLSelectListElement API (Open UI Customizable Select §3) ────────────
    // Phase 0: <selectlist> renders as a native <select> widget.
    // Options may be direct children or inside a <listbox> child element.
    // CSS: appearance: base-select  (P4 wires ::picker(select) styling)
    if ((_lumen_get_tag_name(nid) || '').toUpperCase() === 'SELECTLIST') {
        _obj.multiple = false;
        _obj.size = 1;
        Object.defineProperty(_obj, 'options', {
            get: function() { return _lumen_selectlist_options(nid); },
            enumerable: true, configurable: true,
        });
        Object.defineProperty(_obj, 'length', {
            get: function() { return _lumen_selectlist_options(nid).length; },
            enumerable: true, configurable: true,
        });
        Object.defineProperty(_obj, 'selectedIndex', {
            get: function() {
                var opts = _lumen_selectlist_options(nid);
                for (var i = 0; i < opts.length; i++) {
                    if (opts[i].hasAttribute('selected')) return i;
                }
                return opts.length > 0 ? 0 : -1;
            },
            set: function(idx) {
                var opts = _lumen_selectlist_options(nid);
                for (var i = 0; i < opts.length; i++) {
                    if (i === idx) _lumen_set_attr(opts[i].__nid__, 'selected', '');
                    else _lumen_remove_attr(opts[i].__nid__, 'selected');
                }
            },
            enumerable: true, configurable: true,
        });
        Object.defineProperty(_obj, 'value', {
            get: function() {
                var opts = _lumen_selectlist_options(nid);
                var sel = this.selectedIndex;
                if (sel < 0 || sel >= opts.length) return '';
                var v = _lumen_u2n(_lumen_get_attr(opts[sel].__nid__, 'value'));
                return v !== null ? v : (opts[sel].textContent || '');
            },
            set: function(v) {
                var sv = String(v);
                var opts = _lumen_selectlist_options(nid);
                for (var i = 0; i < opts.length; i++) {
                    var ov = _lumen_u2n(_lumen_get_attr(opts[i].__nid__, 'value'));
                    if (ov === null) ov = opts[i].textContent || '';
                    if (ov === sv) {
                        _lumen_set_attr(opts[i].__nid__, 'selected', '');
                    } else {
                        _lumen_remove_attr(opts[i].__nid__, 'selected');
                    }
                }
            },
            enumerable: true, configurable: true,
        });
        _obj.item = function(idx) {
            var opts = _lumen_selectlist_options(nid);
            return (idx >= 0 && idx < opts.length) ? opts[idx] : null;
        };
        _obj.namedItem = function(name) {
            var opts = _lumen_selectlist_options(nid);
            for (var i = 0; i < opts.length; i++) {
                var id_ = _lumen_u2n(_lumen_get_attr(opts[i].__nid__, 'id'));
                var nm  = _lumen_u2n(_lumen_get_attr(opts[i].__nid__, 'name'));
                if (id_ === name || nm === name) return opts[i];
            }
            return null;
        };
        _obj.add = function(el, before) {
            if (!el || el.__nid__ === undefined) return;
            var listbox = _lumen_selectlist_listbox(nid);
            var container = listbox !== null ? listbox : nid;
            if (before === undefined || before === null) {
                _lumen_append_child(container, el.__nid__);
            } else if (typeof before === 'number') {
                var opts = _lumen_selectlist_options(nid);
                if (before >= 0 && before < opts.length) {
                    _lumen_insert_before(container, el.__nid__, opts[before].__nid__);
                } else {
                    _lumen_append_child(container, el.__nid__);
                }
            } else if (before && before.__nid__ !== undefined) {
                _lumen_insert_before(container, el.__nid__, before.__nid__);
            }
        };
        _obj.remove = function(idx) {
            var opts = _lumen_selectlist_options(nid);
            if (idx >= 0 && idx < opts.length) {
                _lumen_remove_child(_lumen_u2n(_lumen_get_parent(opts[idx].__nid__)), opts[idx].__nid__);
            }
        };
    }
    return _obj;
}

var _lumen_root_nid = _lumen_get_document_root();

var console = {
    log:   function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
    warn:  function() { _lumen_console_warn( Array.prototype.join.call(arguments, ' ')); },
    error: function() { _lumen_console_error(Array.prototype.join.call(arguments, ' ')); },
    info:  function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
    debug: function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
};

// ── FontFace and FontFaceSet (CSS Fonts Module Level 4 §11) ─────────────────

function _lumen_parse_font_face_json(jsonStr) {
    try {
        return JSON.parse(jsonStr);
    } catch(e) {
        return null;
    }
}

function _lumen_get_fonts() {
    var size = _lumen_fonts_size();
    var faces = [];
    for (var i = 0; i < size; i++) {
        var jsonStr = _lumen_fonts_get(i);
        if (jsonStr) {
            var obj = _lumen_parse_font_face_json(jsonStr);
            if (obj) {
                faces.push(obj);
            }
        }
    }
    var fontSet = {
        _faces: faces,
        ready: Promise.resolve(),
        get length() { return this._faces.length; },
        item: function(index) {
            return this._faces[index] || null;
        },
        // Iterate over FontFace objects
        entries: function() {
            var self = this;
            var idx = 0;
            return {
                next: function() {
                    if (idx < self._faces.length) {
                        return { value: [idx, self._faces[idx]], done: false };
                    }
                    return { done: true };
                }
            };
        },
        forEach: function(callback, thisArg) {
            for (var i = 0; i < this._faces.length; i++) {
                callback.call(thisArg, this._faces[i], i, this);
            }
        },
        [Symbol.iterator]: function() {
            var idx = 0;
            var faces = this._faces;
            return {
                next: function() {
                    if (idx < faces.length) {
                        return { value: faces[idx++], done: false };
                    }
                    return { done: true };
                }
            };
        },
    };
    // Symbol.iterator might not be available in all JS engines
    if (typeof Symbol !== 'undefined' && typeof Symbol.iterator !== 'undefined') {
        fontSet[Symbol.iterator] = function() {
            var idx = 0;
            var faces = this._faces;
            return {
                next: function() {
                    if (idx < faces.length) {
                        return { value: faces[idx++], done: false };
                    }
                    return { done: true };
                }
            };
        };
    }
    return fontSet;
}

// ── Range (WHATWG DOM §4.5) ────────────────────────────────────────────────
// Creates a Range object whose endpoints are identified by [nid, offset] pairs.
// nid 0 with offset 0 is the collapsed-at-document-start default.

function _lumen_make_range(sNid, sOff, eNid, eOff) {
    var r = {
        __start_nid__: sNid, __start_off__: sOff,
        __end_nid__:   eNid, __end_off__:   eOff,
        get startContainer() { return _lumen_make_element(this.__start_nid__); },
        get startOffset()    { return this.__start_off__; },
        get endContainer()   { return _lumen_make_element(this.__end_nid__); },
        get endOffset()      { return this.__end_off__; },
        get collapsed()      { return this.__start_nid__ === this.__end_nid__ && this.__start_off__ === this.__end_off__; },
        get commonAncestorContainer() {
            if (this.__start_nid__ === this.__end_nid__) return _lumen_make_element(this.__start_nid__);
            var p = _lumen_u2n(_lumen_get_parent(this.__start_nid__));
            return p !== null ? _lumen_make_element(p) : _lumen_make_element(this.__start_nid__);
        },
        setStart: function(node, offset) {
            if (!node || node.__nid__ === undefined) return;
            this.__start_nid__ = node.__nid__; this.__start_off__ = offset >>> 0;
        },
        setEnd: function(node, offset) {
            if (!node || node.__nid__ === undefined) return;
            this.__end_nid__ = node.__nid__; this.__end_off__ = offset >>> 0;
        },
        setStartBefore: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__start_nid__ = p; this.__start_off__ = Math.max(0, idx);
        },
        setStartAfter: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__start_nid__ = p; this.__start_off__ = idx + 1;
        },
        setEndBefore: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__end_nid__ = p; this.__end_off__ = Math.max(0, idx);
        },
        setEndAfter: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__end_nid__ = p; this.__end_off__ = idx + 1;
        },
        collapse: function(toStart) {
            if (toStart === false) {
                this.__start_nid__ = this.__end_nid__; this.__start_off__ = this.__end_off__;
            } else {
                this.__end_nid__ = this.__start_nid__; this.__end_off__ = this.__start_off__;
            }
        },
        selectNode: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var ch = _lumen_get_children(p), idx = ch.indexOf(node.__nid__);
            this.__start_nid__ = p; this.__start_off__ = Math.max(0, idx);
            this.__end_nid__   = p; this.__end_off__   = idx + 1;
        },
        selectNodeContents: function(node) {
            if (!node || node.__nid__ === undefined) return;
            this.__start_nid__ = node.__nid__; this.__start_off__ = 0;
            this.__end_nid__   = node.__nid__; this.__end_off__   = _lumen_node_length(node.__nid__);
        },
        cloneRange: function() {
            return _lumen_make_range(this.__start_nid__, this.__start_off__, this.__end_nid__, this.__end_off__);
        },
        toString: function() {
            return _lumen_get_range_text(this.__start_nid__, this.__start_off__, this.__end_nid__, this.__end_off__);
        },
        deleteContents: function() {
            var pos = _lumen_range_delete_contents(this.__start_nid__, this.__start_off__, this.__end_nid__, this.__end_off__);
            this.__start_nid__ = pos[0]; this.__start_off__ = pos[1];
            this.__end_nid__   = pos[0]; this.__end_off__   = pos[1];
        },
        extractContents: function() { this.deleteContents(); return null; },
        cloneContents:   function() { return null; },
        insertNode: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(this.__start_nid__));
            if (p !== null) _lumen_append_child(p, node.__nid__);
        },
        surroundContents:     function() {},
        compareBoundaryPoints: function(how, other) {
            how = (how >>> 0) & 3;
            var pairs = [[this.__start_nid__, this.__start_off__, other.__start_nid__, other.__start_off__],
                         [this.__start_nid__, this.__start_off__, other.__end_nid__,   other.__end_off__  ],
                         [this.__end_nid__,   this.__end_off__,   other.__start_nid__, other.__start_off__],
                         [this.__end_nid__,   this.__end_off__,   other.__end_nid__,   other.__end_off__  ]];
            var p = pairs[how];
            if (p[0] !== p[2]) return p[0] < p[2] ? -1 : 1;
            if (p[1] !== p[3]) return p[1] < p[3] ? -1 : 1;
            return 0;
        },
        getBoundingClientRect: function() {
            var el = _lumen_make_element(this.__start_nid__);
            return (el && el.getBoundingClientRect) ? el.getBoundingClientRect()
                : { top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0 };
        },
        getClientRects:   function() { return [this.getBoundingClientRect()]; },
        detach:           function() {},
        isPointInRange:   function() { return false; },
        comparePoint:     function() { return 0; },
        intersectsNode:   function() { return false; },
    };
    r.START_TO_START = 0; r.START_TO_END = 1; r.END_TO_START = 2; r.END_TO_END = 3;
    return r;
}

// Range constructor (allows `new Range()`)
function Range() { return _lumen_make_range(0, 0, 0, 0); }
Range.prototype.START_TO_START = 0; Range.prototype.START_TO_END = 1;
Range.prototype.END_TO_START  = 2; Range.prototype.END_TO_END  = 3;

// ── Selection singleton (WHATWG Selection API §3) ─────────────────────────
// All access to the selection state goes through the Rust bindings.

var _lumen_selection = (function() {
    function _raw() { return _lumen_get_selection(); } // null | [aNid,aOff,fNid,fOff]
    return {
        get anchorNode()   { var s = _raw(); return s ? _lumen_make_element(s[0]) : null; },
        get anchorOffset() { var s = _raw(); return s ? s[1] : 0; },
        get focusNode()    { var s = _raw(); return s ? _lumen_make_element(s[2]) : null; },
        get focusOffset()  { var s = _raw(); return s ? s[3] : 0; },
        get isCollapsed()  { var s = _raw(); return !s || (s[0] === s[2] && s[1] === s[3]); },
        get rangeCount()   { return _raw() ? 1 : 0; },
        get type() {
            var s = _raw();
            if (!s) return 'None';
            return (s[0] === s[2] && s[1] === s[3]) ? 'Caret' : 'Range';
        },
        getRangeAt: function(n) {
            if (n !== 0) throw new RangeError('Selection.getRangeAt: index out of bounds');
            var s = _raw();
            if (!s) throw new RangeError('Selection.getRangeAt: no range');
            return _lumen_make_range(s[0], s[1], s[2], s[3]);
        },
        addRange: function(range) {
            if (!range || range.__start_nid__ === undefined) return;
            _lumen_set_selection(range.__start_nid__, range.__start_off__, range.__end_nid__, range.__end_off__);
        },
        removeRange:    function() { _lumen_clear_selection(); },
        removeAllRanges: function() { _lumen_clear_selection(); },
        empty:          function() { _lumen_clear_selection(); },
        collapse: function(node, offset) {
            if (!node || node.__nid__ === undefined) { _lumen_clear_selection(); return; }
            var off = (offset === undefined || offset === null) ? 0 : (offset >>> 0);
            _lumen_set_selection(node.__nid__, off, node.__nid__, off);
        },
        collapseToStart: function() {
            var s = _raw(); if (!s) return;
            _lumen_set_selection(s[0], s[1], s[0], s[1]);
        },
        collapseToEnd: function() {
            var s = _raw(); if (!s) return;
            _lumen_set_selection(s[2], s[3], s[2], s[3]);
        },
        extend: function(node, offset) {
            if (!node || node.__nid__ === undefined) return;
            var s = _raw();
            var aNid = s ? s[0] : node.__nid__, aOff = s ? s[1] : 0;
            _lumen_set_selection(aNid, aOff, node.__nid__, offset >>> 0);
        },
        selectAllChildren: function(node) {
            if (!node || node.__nid__ === undefined) return;
            _lumen_set_selection(node.__nid__, 0, node.__nid__, _lumen_node_length(node.__nid__));
        },
        deleteFromDocument: function() {
            var s = _raw(); if (!s) return;
            _lumen_range_delete_contents(s[0], s[1], s[2], s[3]);
            _lumen_clear_selection();
        },
        setBaseAndExtent: function(aN, aO, fN, fO) {
            if (!aN || aN.__nid__ === undefined || !fN || fN.__nid__ === undefined) return;
            _lumen_set_selection(aN.__nid__, aO >>> 0, fN.__nid__, fO >>> 0);
        },
        containsNode:    function() { return false; },
        getComposedRanges: function() { return []; },
        modify:          function() {},
        toString: function() { return _lumen_get_selection_text(); },
    };
}());

// ── contenteditable key dispatch (Input Events Level 2 §4.1) ─────────────────
// Called by the shell when a key is pressed while a contenteditable element has
// focus. Fires beforeinput → DOM mutation → input following the spec sequence.
//
// `inputType`  — Input Events Level 2 inputType string (e.g. insertText)
// `data`       — inserted text for insertText; null/undefined for deletions
// `targetNid`  — nid of the contenteditable host element
//
// Returns true if the event was not cancelled and the mutation was applied.
function _lumen_handle_contenteditable_key(inputType, data, targetNid) {
    var target = (targetNid !== undefined && targetNid !== null)
        ? _lumen_make_element(targetNid)
        : null;
    if (!target) return false;

    // Fire beforeinput (cancelable).
    var before = new InputEvent('beforeinput', {
        bubbles: true, cancelable: true,
        inputType: inputType,
        data: (data !== undefined && data !== null) ? String(data) : null,
    });
    var notCancelled = target.dispatchEvent(before);
    if (!notCancelled) return false;

    // Apply the DOM mutation.
    var applied = false;
    if (inputType === 'insertText') {
        applied = _lumen_contenteditable_insert_text(String(data || ''));
    } else if (inputType === 'deleteContentBackward' || inputType === 'deleteWordBackward') {
        applied = _lumen_contenteditable_delete_backward();
    } else if (inputType === 'deleteContentForward' || inputType === 'deleteWordForward') {
        applied = _lumen_contenteditable_delete_forward();
    } else if (inputType === 'insertParagraph') {
        applied = _lumen_contenteditable_insert_paragraph();
    } else if (inputType === 'insertLineBreak') {
        applied = _lumen_contenteditable_insert_text('\n');
    }

    if (!applied) return false;

    // Fire input (not cancelable).
    var inp = new InputEvent('input', {
        bubbles: true, cancelable: false,
        inputType: inputType,
        data: (data !== undefined && data !== null) ? String(data) : null,
    });
    target.dispatchEvent(inp);
    return true;
}

// ── Pointer Lock state (W3C Pointer Lock L2 §6) ──────────────────────────────
// Current locked element — null when unlocked.  Mirrored in Rust pointer_lock
// thread-local for cross-thread movement accumulation.
var _ptr_lock_el = null;

// ── Fullscreen API (WHATWG Fullscreen §4) ────────────────────────────────────
// Current fullscreen element NID (-1 = none).
var _fs_nid = -1;
// Sentinel attribute written by requestFullscreen() and read by the CSS cascade.
// CSS: :fullscreen — P4 wires PseudoClass::Fullscreen to check this attr.
var _FS_ATTR = 'data-lumen-fullscreen';

// ── Page Visibility API + document.readyState state vars ─────────────────────
// Declared before `document` because getters below capture these by name.
var _doc_hidden = false;
var _doc_visibility_state = 'visible';
var _doc_ready_state = 'loading';
var __dom_node_warned = false;
// BUG-324: cache for the live page's `document.implementation`, so repeated
// access returns the same object (`document.implementation === document.implementation`).
var _lumen_document_implementation = null;

// HTML LS §4.8.3: the `HTMLImageElement` interface and its legacy factory
// function `Image(width?, height?)`. BUG-305: both were entirely absent, so
// `new Image()` — one of the most common legacy patterns (image preloading,
// tracking pixels, canvas sources) — threw `Image is not defined` and took the
// whole script down. `Image(w, h)` is defined to be equivalent to
// `document.createElement('img')` with the width/height content attributes set
// from the constructor arguments. Returning the element from the constructor
// makes `new Image()` yield the native `<img>` wrapper (a returned object wins
// over `this`), so it participates in layout/paint like any parsed `<img>`.
// `HTMLImageElement` is exposed as an interface global with a real
// HTMLElement/Element/Node prototype chain (BUG-322), same as the other
// concrete HTML*Element interfaces generated further up — `_lumen_html_tag_prototypes`
// points the `<img>` tag at this constructor's `.prototype`, so every `<img>`
// wrapper (including ones built by `Image()` below) resolves `instanceof
// HTMLImageElement/HTMLElement/Element/Node`.
function HTMLImageElement() { throw new TypeError('Illegal constructor'); }
HTMLImageElement.prototype = Object.create(HTMLElement.prototype);
HTMLImageElement.prototype.constructor = HTMLImageElement;
function Image(width, height) {
    var img = document.createElement('img');
    if (width !== undefined && width !== null)  { img.width  = width; }
    if (height !== undefined && height !== null) { img.height = height; }
    return img;
}

var document = {
    // DOM LS §4.5: `Document.nodeType` is always `Node.DOCUMENT_NODE` (9). react-dom's
    // root-creation path (BUG-281) checks this before mounting.
    get nodeType()   { return 9; },
    get nodeName()   { return '#document'; },
    get ownerDocument() { return null; },
    // DOM §4.9: the document's child nodes (top-level comments, the doctype and
    // the root element) in tree order, wrapped kind-aware so the doctype child
    // is a DocumentType node (BUG-321). Static array (same simplification as
    // querySelectorAll). For a standard page `childNodes[1]` is the doctype.
    get childNodes() {
        return _lumen_get_children(_lumen_root_nid).map(_lumen_make_node);
    },
    // BUG-327: DOM §4.4 Node.hasChildNodes() — the `document` singleton isn't
    // wired to `Document.prototype` (so it doesn't inherit the one just added
    // there), hence an own copy here.
    hasChildNodes: function() { return this.childNodes.length > 0; },
    // Same reason as `hasChildNodes` above: `Node.prototype.contains` /
    // `.compareDocumentPosition` (BUG-732) don't reach this literal, and
    // `document.contains(node)` is the single most common form of the call.
    contains: function(other) { return _lumen_node_contains(this, other); },
    compareDocumentPosition: function(other) { return _lumen_node_compare_position(this, other); },
    // DOM §4.5: the document's DocumentType child (`<!doctype …>`), or null.
    get doctype() {
        var dnid = _lumen_u2n(_lumen_get_document_doctype());
        return dnid !== null ? _lumen_make_doctype(dnid) : null;
    },
    // DOM §4.5: DOMImplementation, cached (BUG-324 — was absent entirely,
    // cascading `Cannot read properties of undefined (reading '...')` into
    // every WPT fixture that builds an XML/HTML document through it).
    get implementation() {
        if (_lumen_document_implementation === null) {
            _lumen_document_implementation = _lumen_make_dom_implementation(document);
        }
        return _lumen_document_implementation;
    },
    get title()  { return _lumen_get_document_title(); },
    set title(v) { _lumen_set_document_title(String(v)); },
    get cookie()  { return _lumen_cookie_get(); },
    set cookie(v) { _lumen_cookie_set(String(v)); },
    // DOM §7.3 / DOM §4.5 (BUG-358): document-metadata IDL attributes — the live
    // `document` never defined these at all (mirrors `_lumen_build_detached_document`'s
    // hardcoded block above, but reads real per-load state). `charset`/`inputEncoding`
    // are legacy aliases of `characterSet` (HTML LS), hence the shared native.
    get characterSet()  { return _lumen_get_document_character_set(); },
    get charset()       { return _lumen_get_document_character_set(); },
    get inputEncoding() { return _lumen_get_document_character_set(); },
    get compatMode()    { return _lumen_get_document_compat_mode(); },
    get contentType()   { return _lumen_get_document_content_type(); },
    get URL()           { return _lumen_loc_href; },
    get documentURI()   { return _lumen_loc_href; },
    // DOM §4.4 Node.baseURI (BUG-377). Unlike `URL`/`documentURI` this is NOT
    // the document URL: a `<base href>` in the page overrides it (HTML LS
    // §4.2.3). Own property because the live `document` is an object literal
    // that never reaches `Node.prototype` — same reason `hasChildNodes` and
    // `contains` are duplicated above.
    get baseURI()       { return _lumen_document_base_url(); },
    // `Document.location` is `[PutForwards=href]` like `window.location`, so an
    // assignment navigates rather than being silently swallowed.
    get location()      { return _lumen_location; },
    set location(v)     { _lumen_location.href = v; },
    get body()   {
        var bid = _lumen_u2n(_lumen_get_body());
        return bid !== null ? _lumen_make_element(bid) : null;
    },
    // HTML LS 3.1.4 (BUG-703): the head element. Missing entirely until now —
    // webpack's chunk loader ends in `document.head.appendChild(script)`, so on
    // every bundled site each lazily-loaded chunk threw
    // `Cannot read properties of undefined`; inside an async bootstrap that
    // became an unreported rejection and the app silently never rendered.
    get head()   {
        var hid = _lumen_u2n(_lumen_get_head());
        return hid !== null ? _lumen_make_element(hid) : null;
    },
    // HTML LS §3.1.5 (BUG-486, blocking BUG-703): the `<script>` element whose
    // body is executing right now, `null` outside classic script execution.
    // Self-locating bundles key themselves off it — the tbank.ru micro-block
    // bundles read `document.currentScript.dataset.mmid`, so with it missing all
    // 44 of them registered under the key `undefined`, overwriting each other,
    // and every one of the page's 81 blocks rendered as an empty frame.
    get currentScript() {
        var s = _lumen_current_script_stack;
        return s.length !== 0 ? s[s.length - 1] : null;
    },
    // BUG-281: must return the `<html>` element, not the `Document` node itself
    // (react-dom's container-identity checks fail if `tagName` reads `#document`).
    get documentElement() {
        var hid = _lumen_u2n(_lumen_get_html_element());
        return hid !== null ? _lumen_make_element(hid) : null;
    },
    // HTML LS §6.6.3 (BUG-381): the element that currently holds focus. Falls
    // back to `<body>` — not `null` — while nothing is focused, per spec; the
    // underlying state is `_lumen_last_focused_nid`, kept in step with the
    // shell's `focused_node` by `_lumen_focus_update`.
    get activeElement() {
        var n = _lumen_last_focused_nid;
        if (n === null || n === undefined || n === -1) {
            var bid = _lumen_u2n(_lumen_get_body());
            return bid !== null ? _lumen_make_element(bid) : null;
        }
        return _lumen_make_element(n);
    },
    // HTML LS §6.6.3: true while this document's window is the active one.
    // Reuses the shell signal that already drives `document.visibilityState`.
    hasFocus: function() { return !_doc_hidden; },
    // HTML LS §6.6.3 (BUG-353): 'on'/'off', reflecting a design-mode flag kept
    // on the native `Document` — when 'on' the whole document becomes an
    // editing host (see `find_editing_host`'s design-mode fallback), without
    // requiring `contenteditable` on any element.
    get designMode() { return _lumen_get_design_mode() ? 'on' : 'off'; },
    set designMode(v) {
        var s = String(v).toLowerCase();
        // Spec: a value that is neither 'on' nor 'off' (case-insensitive) leaves
        // the current mode untouched rather than falling back to 'off'.
        if (s === 'on') _lumen_set_design_mode(true);
        else if (s === 'off') _lumen_set_design_mode(false);
    },
    getElementById:    function(id)  {
        var n = _lumen_u2n(_lumen_get_element_by_id(String(id)));
        return n !== null ? _lumen_make_element(n) : null;
    },
    querySelector:     function(sel) {
        var n = _lumen_u2n(_lumen_query_selector(_lumen_sel(sel)));
        return n !== null ? _lumen_make_element(n) : null;
    },
    querySelectorAll:  function(sel) {
        return _lumen_query_selector_all(_lumen_sel(sel)).map(_lumen_make_element);
    },
    // DOM LS §4.5: getElementsByTagName(qualifiedName) — a static array, not a
    // live HTMLCollection (same simplification `querySelectorAll` above makes).
    // Found missing (broke `testharness.js`'s own `test_timeout()`/
    // `get_script_url()`, which call it unconditionally) while implementing
    // P2-wpt S4; BUG-416 then replaced the original «hand the tag to the
    // selector engine as a type selector» body, which matched the local name by
    // exact string equality and so answered nothing at all for
    // `getElementsByTagName('DIV')` and mis-parsed a non-identifier name.
    getElementsByTagName: function(qualifiedName) {
        return _lumen_collect_matching(
            _lumen_query_selector_all('*'),
            _lumen_tag_name_predicate(qualifiedName));
    },
    // DOM LS §4.5: getElementsByTagNameNS(namespace, localName) — the
    // namespace-aware sibling, missing from the document as well as the element
    // until BUG-416.
    getElementsByTagNameNS: function(namespace, localName) {
        return _lumen_collect_matching(
            _lumen_query_selector_all('*'),
            _lumen_tag_ns_predicate(namespace, localName));
    },
    // DOM LS §4.5: getElementsByClassName(names) — document-global variant.
    // Static array, not a live HTMLCollection (same simplification as above).
    getElementsByClassName: function(names) {
        var sel = _lumen_class_selector(names);
        if (sel === null) return [];
        return _lumen_query_selector_all(sel).map(_lumen_make_element);
    },
    // HTML LS §3.1.5: getElementsByName(elementName) — the HTML-namespace
    // elements whose `name` attribute equals the argument, in tree order
    // (BUG-412: the method was missing from the shim entirely, so every call
    // threw `is not a function`). Unlike the two accessors above this returns a
    // LIVE NodeList rather than a static array: the spec's own liveness is what
    // `document.images` already gets from the same Proxy, and named-access is
    // switched off because a NodeList has no `namedItem`. The DOMString
    // conversion of the argument is what makes `getElementsByName(null)` look
    // for the literal name `null`.
    getElementsByName: function(elementName) {
        var name = String(elementName);
        return _lumen_make_nid_collection(
            function() { return _lumen_elements_named_nids(name); },
            NodeList.prototype, true);
    },
    // HTML LS §3.1.5 — `document.images`: a live HTMLCollection of the `img`
    // elements in the document, in tree order (BUG-732: was `undefined`, so
    // `document.images.length` threw). A real live collection rather than the
    // static array `getElementsByTagName` above settles for: unlike a one-off
    // query, this one is read repeatedly by long-lived code (image
    // preloaders/lazy-loaders) that expects later-inserted images to show up.
    get images() {
        return _lumen_make_nid_collection(
            function() { return _lumen_query_selector_all('img'); },
            HTMLCollection.prototype);
    },
    createElement:     function(tag) {
        var nid = _lumen_create_element(String(tag).toLowerCase());
        // QuickJS truncates the Rust u32::MAX sentinel to -1 (signed FFI
        // narrowing); the V8 native returns -1 explicitly as i32 (BUG-457) —
        // either way `nid < 0` catches the overflow on both engines.
        if (nid < 0) {
            throw new DOMException('DOM node limit exceeded', 'QuotaExceededError');
        }
        var cnt = _lumen_dom_node_count();
        if (!__dom_node_warned && cnt >= 40000) {
            __dom_node_warned = true;
            console.warn('DOM tree exceeds 40000 nodes');
        }
        // BUG-571: a script element minted here is allowed to run once it is
        // inserted into the document — see `_lumen_resource_track`.
        _lumen_resource_track(nid, tag);
        return _lumen_make_element(nid);
    },
    // DOM LS §4.5: createElementNS(namespace, qualifiedName) creates a native
    // arena node (with __nid__) so layout/paint see it. SVG tag case is preserved
    // (native binding does not lowercase) — `linearGradient`/`clipPath` stay intact.
    createElementNS:   function(ns, qualifiedName) {
        var local = String(qualifiedName || '').replace(/^[^:]+:/, '');
        // BUG-328: DOM §4.5 validate-and-extract normalizes a null/undefined
        // namespace to no namespace — String(null) would otherwise send the
        // literal 4-char string null down to _lumen_create_element_ns, which
        // is neither the SVG URL nor empty and so falls into the HTML
        // fallback (wrong namespace, silently).
        var nid = _lumen_create_element_ns(ns === null || ns === undefined ? '' : String(ns), local);
        // See createElement above re: engine-specific overflow encoding.
        if (nid < 0) {
            throw new DOMException('DOM node limit exceeded', 'QuotaExceededError');
        }
        // BUG-571: SVG's <script> runs on insertion exactly like the HTML one.
        _lumen_resource_track(nid, local);
        return _lumen_make_element(nid);
    },
    createTextNode:         function(t) {
        var nid = _lumen_create_text_node(String(t));
        if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
        return _lumen_make_element(nid);
    },
    // DOM LS §4.5: createComment(data) — previously ignored `data` entirely and
    // always built an empty *Text* node (both the missing argument and the
    // wrong node kind are fixed here; see `_lumen_create_comment`/BUG-322-family
    // nodeType/nodeName/prototype fixes below for why a real Comment node now
    // reports nodeType 8, nodeName '#comment' and `Comment.prototype`).
    createComment:          function(t) {
        var nid = _lumen_create_comment(t === undefined ? '' : String(t));
        if (nid < 0) { throw new DOMException('DOM node limit exceeded', 'QuotaExceededError'); }
        return _lumen_make_element(nid);
    },
    // DOM LS §4.5: createDocumentFragment() returns an empty DocumentFragment.
    createDocumentFragment: function()    { return _lumen_make_document_fragment(_lumen_create_fragment()); },
    // DOM LS §4.5: createProcessingInstruction(target, data). Throws
    // InvalidCharacterError if `target` is not a valid XML Name or `data`
    // contains the PI-closing sequence ?> . Returns a ProcessingInstruction
    // node (BUG-313).
    createProcessingInstruction: function(target, data) {
        var t = String(target);
        var d = String(data);
        if (!_lumen_is_xml_name(t)) {
            throw new DOMException(
                'createProcessingInstruction: the target is not a valid XML name: ' + t,
                'InvalidCharacterError');
        }
        if (d.indexOf('?>') !== -1) {
            throw new DOMException(
                'createProcessingInstruction: the data must not contain the sequence ?>',
                'InvalidCharacterError');
        }
        return _lumen_make_processing_instruction(t, d);
    },
    appendChild:       function(c)   {
        if (c && c.__nid__ !== undefined) _lumen_append_child(_lumen_root_nid, c.__nid__);
        return c;
    },
    // Page Visibility API (HTML LS §15.1) — state vars declared after navigator
    get hidden()          { return _doc_hidden; },
    get visibilityState() { return _doc_visibility_state; },
    // Document lifecycle (HTML LS §8.5) — readyState driven by _lumen_apply_ready_state()
    get readyState()      { return _doc_ready_state; },
    // HTML LS §8.4.4 document.write()/writeln() — was missing entirely, so any
    // page calling it (legacy ad/analytics snippets are the common case) threw
    // `document.write is not a function` and aborted the rest of that script.
    // Spec-accurate behaviour needs an active-parser insertion point we do not
    // track; instead this covers the two cases that matter without the
    // destructive implicit document.open() the spec calls for on a closed
    // document (which would wipe an already-hydrating SPA root out from under
    // it): while still parsing, the text lands at the end of body, same as a
    // real browser's insertion point would for a synchronous inline-script
    // call; once the document has finished loading it is a no-op, matching
    // real browsers' document.write() intervention for scripts that call it
    // after load instead of erasing the page.
    write: function() {
        if (_doc_ready_state !== 'loading') return;
        var body = document.body;
        if (!body) return;
        var text = '';
        for (var i = 0; i < arguments.length; i++) text += String(arguments[i]);
        body.insertAdjacentHTML('beforeend', text);
    },
    writeln: function() {
        var args = Array.prototype.slice.call(arguments);
        args.push('\n');
        document.write.apply(document, args);
    },
    // addEventListener intercepts DOMContentLoaded to fire immediately when already ready
    addEventListener: function(type, fn, opts) {
        if (type === 'DOMContentLoaded' && _doc_ready_state !== 'loading') {
            queueMicrotask(function() {
                try { fn(new Event('DOMContentLoaded', { bubbles: true })); } catch(e) { _lumen_report_exception(e); }
            });
            return;
        }
        _lumen_add_listener(_LUMEN_DOC_LISTENER_NID, type, fn);
    },
    removeEventListener: function(type, fn) { _lumen_rm_listener(_LUMEN_DOC_LISTENER_NID, type, fn); },
    // dispatchEvent: fire all document-level listeners for the given event
    dispatchEvent: function(evt) {
        if (!evt || !evt.type) return false;
        var key = String(_LUMEN_DOC_LISTENER_NID) + ':' + String(evt.type);
        var arr = _lumen_listeners[key];
        if (arr) {
            var copy = arr.slice();
            for (var i = 0; i < copy.length; i++) {
                try { copy[i].call(document, evt); } catch(e) { _lumen_report_exception(e); }
            }
        }
        return !evt.defaultPrevented;
    },
    get fonts() {
        return _lumen_get_fonts();
    },
    // ── Selection API ─────────────────────────────────────────────────────
    getSelection:  function() { return _lumen_selection; },
    createRange:   function() { return _lumen_make_range(0, 0, 0, 0); },
    // execCommand (HTML §9.2.1 — executes a legacy editing command)
    execCommand: function(cmd, showUI, value) {
        return _lumen_exec_command(String(cmd), value !== undefined && value !== null ? String(value) : '');
    },
    queryCommandEnabled:   function(cmd) { return true; },
    queryCommandState:     function(cmd) { return false; },
    queryCommandValue:     function(cmd) { return ''; },
    queryCommandSupported: function(cmd) { return true; },
    queryCommandIndeterm:  function(cmd) { return false; },
    // Web Animations API (WAAPI Level 1) — document.timeline and document.getAnimations().
    get timeline() { return _wa_doc_timeline; },
    getAnimations: function() { return _wa_doc_get_animations(); },
    // Fullscreen API (WHATWG Fullscreen §4) — document-level surface.
    get fullscreenElement() {
        return _fs_nid !== -1 ? _lumen_make_element(_fs_nid) : null;
    },
    get fullscreenEnabled() { return true; },
    exitFullscreen: function() {
        return new Promise(function(resolve, reject) {
            // Fullscreen §4.4: with no fullscreen element the promise rejects
            // with a TypeError instead of resolving on a no-op (BUG-390).
            if (_fs_nid === -1) {
                reject(new TypeError('exitFullscreen(): the document is not in fullscreen mode'));
                return;
            }
            var old = _fs_nid;
            _lumen_remove_attr(_fs_nid, _FS_ATTR);
            _fs_nid = -1;
            // Notify shell to exit OS fullscreen.
            if (typeof _lumen_fs_exit === 'function') { _lumen_fs_exit(); }
            var prev = _lumen_make_element(old);
            if (prev) { prev.dispatchEvent(new Event('fullscreenchange', { bubbles: true })); }
            document.dispatchEvent(new Event('fullscreenchange'));
            resolve();
        });
    },
    onfullscreenchange: null,
    onfullscreenerror:  null,
    // Pointer Lock API (W3C Pointer Lock L2 §2-4) — Phase 1: JS mirror via _ptr_lock_el
    get pointerLockElement() {
        return _ptr_lock_el;
    },
    exitPointerLock: function() {
        _ptr_lock_el = null;
        if (typeof _lumen_exit_ptr_lock === 'function') { _lumen_exit_ptr_lock(); }
        document.dispatchEvent(new Event('pointerlockchange'));
    },
    onpointerlockchange: null,
    onpointerlockerror: null,
    // Storage Access API (W3C Storage Access API §5) — Phase 0: always granted
    requestStorageAccess: function() {
        return Promise.resolve();
    },
    hasStorageAccess: function() {
        return Promise.resolve(true);
    },
    requestStorageAccessFor: function(origin) {
        return Promise.resolve();
    },
    hasUnpartitionedCookieAccess: function() {
        return Promise.resolve(true);
    },
    // DOM LS §4.6: adoptNode — moves node into this document (Phase 0: no-op, returns node).
    adoptNode: function(node) { return node; },
    // DOM LS §4.7: importNode — returns a clone of node for use in this document.
    importNode: function(node, deep) {
        if (!node) return null;
        if (node.__nid__ !== undefined) {
            var clone_nid = _lumen_clone_subtree(node.__nid__, deep ? 1 : 0);
            return _lumen_make_element(clone_nid);
        }
        return null;
    },
    // DOM LS §4.5: createTreeWalker(root, whatToShow, filter) — returns a TreeWalker.
    createTreeWalker: function(root, whatToShow, filter) {
        return new _TreeWalker(root, whatToShow !== undefined ? whatToShow : 0xFFFFFFFF, filter || null);
    },
    // DOM LS §4.4: createNodeIterator(root, whatToShow, filter) — returns a NodeIterator.
    createNodeIterator: function(root, whatToShow, filter) {
        return new _NodeIterator(root, whatToShow !== undefined ? whatToShow : 0xFFFFFFFF, filter || null);
    },
    // CSSOM View §5.1: caretPositionFromPoint(x, y) — returns a CaretPosition or null.
    // Phase 0: no layout hit-testing yet; returns body at offset 0 when body exists.
    caretPositionFromPoint: function(x, y) {
        var bodyNid = _lumen_u2n(_lumen_get_body());
        if (bodyNid === null) return null;
        return new _CaretPosition(_lumen_make_element(bodyNid), 0);
    },
};

var alert    = function(m) { _lumen_console_log('[alert] ' + String(m)); };
var confirm  = function()  { return false; };
var prompt   = function()  { return null; };
var print    = function()  { _lumen_print_dialog(); };

// ── HTML LS §4.12.1 'prepare the script element' (BUG-571) ───────────────────
// Script execution used to be a one-shot walk of the already-parsed tree, run
// exactly once per navigation by the shell (`main.rs::run_scripts_with_dom`),
// so a <script> built with `document.createElement` and appended into the live
// document stayed inert forever — no exception, no network request, nothing.
// That is the single most common dynamic-loading pattern on the web (webpack
// chunk loaders, lazy analytics/ad snippets, polyfill loaders) and 218 subtests
// of the WPT `the-script-element` category. Everything below is the live half
// of the algorithm: elements minted through the DOM API are tracked, and the
// first time one of them becomes connected it is prepared and executed.
//
// Deliberately NOT covered, matching the spec's 'already started' flag: scripts
// produced by the fragment parser (innerHTML / insertAdjacentHTML /
// document.write) and scripts that came from the document parser. Neither ever
// enters `_lumen_resource_pending`, so neither can be re-run from here.
//
// BUG-703: <link rel=stylesheet> rides the same machinery. The shell fetches a
// dynamically inserted sheet and puts it in the cascade, but nothing told the
// page — so `link.onload` never fired, and the common «await the sheet, then
// render» loader waited forever.

// nid → 'script' | 'link' | 'track' for an element built by createElement/
// createElementNS whose insertion has to start a resource load and that has not
// been prepared yet. The entry is deleted on preparation, so the map doubles as
// the spec's per-element 'already started' flag: moving an executed script (or a
// loaded link, or a loaded track) around the tree can never fetch it a second
// time.
var _lumen_resource_pending = {};
var _lumen_resource_pending_count = 0;

// Remember a freshly created element whose insertion starts a load. Called from
// document.createElement / createElementNS — the only two ways page script can
// mint such an element that the spec allows to act on insertion.
function _lumen_resource_track(nid, local) {
    var tag = String(local).toLowerCase();
    if (tag !== 'script' && tag !== 'link' && tag !== 'track' && tag !== 'source'
        && tag !== 'style') return;
    _lumen_resource_pending[nid] = tag;
    _lumen_resource_pending_count++;
}

// Same shadow-inclusive test as Node.isConnected, by nid alone — no element
// wrapper is allocated, because this runs on the DOM insertion hot path.
function _lumen_resource_is_connected(nid) {
    var htmlId = _lumen_u2n(_lumen_get_html_element());
    if (htmlId === null) return false;
    var cur = nid;
    while (cur !== null && cur !== undefined) {
        if (cur === htmlId) return true;
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    return false;
}

// HTML LS §2.1.5 JavaScript MIME types — the JS-side mirror of
// `main.rs::is_classic_script_type` (parser path); keep the two in step.
var _LUMEN_CLASSIC_SCRIPT_TYPES = {
    'text/javascript': 1, 'application/javascript': 1, 'application/ecmascript': 1,
    'application/x-ecmascript': 1, 'application/x-javascript': 1, 'text/ecmascript': 1,
    'text/javascript1.0': 1, 'text/javascript1.1': 1, 'text/javascript1.2': 1,
    'text/javascript1.3': 1, 'text/javascript1.4': 1, 'text/javascript1.5': 1,
    'text/jscript': 1, 'text/livescript': 1, 'text/x-ecmascript': 1,
    'text/x-javascript': 1
};
function _lumen_is_classic_script_type(t) {
    if (t === null || t === undefined) return true;
    var s = String(t).trim();
    if (s === '') return true;
    return _LUMEN_CLASSIC_SCRIPT_TYPES[s.toLowerCase()] === 1;
}

// `load`/`error` on a script or link element neither bubble nor cancel, so a
// plain at-target dispatch is the whole story. `_lumen_dispatch` runs
// addEventListener listeners and the `on<type>` IDL attribute alike (BUG-360).
// The event is `isTrusted` and carries the element as its `target`: it is
// generated by the engine, and `_lumen_dispatch` fills in neither (it has no
// general target assignment at all — BUG-873). `fetch-src/empty.html` asserts
// both on the very event BUG-838 is about, and a handler that cannot read
// `ev.target` cannot tell two pending scripts apart.
function _lumen_resource_fire(nid, type) {
    try {
        var ev = new Event(type, { bubbles: false, cancelable: false, isTrusted: true });
        ev.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, ev);
    }
    catch (e) {}
    // BUG-480 срез 10: зеркало ресурсного события в родительский изолят —
    // обработчики фасада (`s.onload`, назначенный родителем на элемент
    // под-документа) живут не здесь, и без обратного конверта они навсегда
    // no-op. Гейт «есть родитель» внутри натива; в минимальных изолятах без
    // бриджа функции нет, топ-страница получает пустой вызов натива.
    if (typeof _lumen_frame_mirror_resource === 'function') {
        try { _lumen_frame_mirror_resource(nid, type); } catch (e2) {}
    }
}

// HTML LS §3.1.5 «current script» (BUG-486, blocking BUG-703). A stack, not a
// single slot: a classic script may synchronously insert and run another one,
// and the outer script must see itself again once the inner one returns.
// Pushed only around classic script bodies — module scripts, event handlers and
// any asynchronous callback read `null`, which is exactly what the spec asks
// for (the stack is empty by the time a task or microtask runs).
var _lumen_current_script_stack = [];
function _lumen_push_current_script(nid) {
    var n = _lumen_u2n(nid);
    _lumen_current_script_stack.push(n === null || n < 0 ? null : _lumen_make_element(n));
}
function _lumen_pop_current_script() {
    _lumen_current_script_stack.pop();
}

// A classic script body runs in global scope — indirect eval is exactly that.
// An uncaught exception must not escape into the DOM call that inserted the
// element (the spec reports it to the page instead), hence the catch.
// `nid` is the `<script>` element being executed; it backs
// `document.currentScript` for the duration of the body (omitted → `null`).
function _lumen_script_execute_classic(text, nid) {
    _lumen_push_current_script(nid);
    // BUG-591: report the exception (window 'error'/onerror) instead of only
    // logging it -- this is the classic-script runtime-error reporting step
    // of HTML LS §8.1.3.6, reached by every script inserted through the DOM
    // (createElement('script') + appendChild, the parser's own insertion
    // path, …), as opposed to the initial page-load loop in
    // `crates/shell/src/main.rs`, which goes through the Rust-side
    // `V8JsRuntime::eval_and_report` for the same reporting step instead.
    try { (0, eval)(text); }
    catch (e) { _lumen_report_exception(e); }
    finally { _lumen_pop_current_script(); }
}

// `import()` is compiled lazily through `new Function` rather than written
// inline: the shim is compiled as one classic script, so a host that refuses
// dynamic import in this position would take the whole shim down with it.
// Cached as `false` after a failed compile — module scripts then report `error`
// instead of hanging, and classic scripts are unaffected either way.
var _lumen_dynamic_import = null;
function _lumen_get_dynamic_import() {
    if (_lumen_dynamic_import === null) {
        try { _lumen_dynamic_import = new Function('s', 'return import(s);'); }
        catch (e) { _lumen_dynamic_import = false; }
    }
    return _lumen_dynamic_import;
}

// Evaluate a module script body. `url` is the absolute URL of an external
// module (empty for an inline one); registering the source under that specifier
// and then importing it reuses the existing ES module map, so a second <script>
// with the same src evaluates the module only once. Static imports *inside* the
// module still resolve only against pre-registered sources — the network module
// graph is BUG-446, unchanged by this.
function _lumen_script_run_module(url, text) {
    var dyn = _lumen_get_dynamic_import();
    if (!dyn) return Promise.reject(new Error('dynamic import is unavailable'));
    var spec;
    if (url) { spec = url; _lumen_esm_register(spec, text); }
    else { spec = _lumen_esm_register_inline(text); }
    return Promise.resolve(dyn(spec));
}

// External `<script src>`: HTML LS §4.12.1 sets the 'force async' flag on any
// script element inserted by script, so the fetch and the execution both belong
// to a later task — never to the appendChild call itself. That matters twice
// over here: Lumen's `fetch` is synchronous underneath (an inline fetch would
// stall the insertion), and the near-universal `el.onload = …` assignment that
// follows appendChild would otherwise be installed after the event fired.
function _lumen_script_load_external(nid, src, isModule) {
    setTimeout(function() {
        var url = _url_resolve(String(src), _lumen_document_base_url());
        fetch(url, { _lumenInitiatorType: 'script' }).then(function(resp) {
            if (!resp.ok) throw new Error('HTTP ' + resp.status);
            return resp.text();
        }).then(function(text) {
            if (isModule) return _lumen_script_run_module(url, text);
            _lumen_script_execute_classic(text, nid);
        }).then(function() {
            _lumen_resource_fire(nid, 'load');
        }).catch(function(e) {
            _lumen_console_error('script load failed: ' + url + ': ' + e);
            _lumen_resource_fire(nid, 'error');
        });
    }, 0);
}

// ── `<script src="">`: an empty src obtains nothing and reports `error` ──────
//
// HTML LS §4.12.1 separates «el has no src attribute» (fall through to the
// inline body) from «el has a src attribute whose value is the empty string»
// (obtain no resource, queue an element task that fires `error`). Collapsing
// both into '' was BUG-838: the element went down the inline branch, found no
// body and returned in silence — no event, no request, no log line, so a page
// waiting for the `error` the spec promises waited forever.
//
// nid → 1 once the task has been queued. The two entry points overlap: a
// `<script src="">` a head script creates goes through the insertion hook
// below, and the parser pass re-visits every script still in the tree once
// parsing ends, so without the flag such an element would report twice.
var _lumen_script_empty_src_done = {};

function _lumen_script_fire_empty_src_error(nid) {
    if (_lumen_script_empty_src_done[nid] === 1) return;
    _lumen_script_empty_src_done[nid] = 1;
    // A task, not an inline dispatch: `script.onerror = …` is assigned before
    // the appendChild that triggers this, but `empty.html` asserts the event is
    // *not* synchronous («event should not be dispatched synchronously»). Same
    // hop the external-src path above takes, for the same reason.
    setTimeout(function() { _lumen_resource_fire(nid, 'error'); }, 0);
}

// Does this element carry a src attribute that names no resource? Answers only
// for the script types that can have one — a data block (`importmap`,
// `application/json`, a template language) is never a script, and the spec
// returns on its type before ever reading src.
function _lumen_script_has_empty_src(nid, isModule, type) {
    if (!isModule && !_lumen_is_classic_script_type(type)) return false;
    var src = _lumen_u2n(_lumen_get_attr(nid, 'src'));
    // Whitespace-only is not literally the empty string, so a spec-literal
    // reading would URL-parse it — and the URL parser strips exactly that
    // whitespace, resolving it to the document itself. Fetching the page's own
    // HTML only to fail parsing it as JS ends at this same `error`, so the
    // request is skipped and the event queued directly.
    return src !== null && String(src).trim() === '';
}

// The parser's half. A `<script>` written by the HTML parser never passes
// through the insertion hook, and the shell's own collector
// (`main.rs::collect_scripts_ordered`) drops an empty src just as silently, so
// the markup's empty-src scripts are picked up in one pass when parsing ends —
// the same shape `_lumen_link_hints_scan` uses for `<link>` hints.
function _lumen_script_empty_src_scan() {
    var scripts;
    try { scripts = document.getElementsByTagName('script'); } catch (e) { return; }
    if (!scripts) return;
    for (var i = 0; i < scripts.length; i++) {
        var el = scripts[i];
        if (!el || el.__nid__ === undefined) continue;
        var type = _lumen_u2n(_lumen_get_attr(el.__nid__, 'type'));
        var isModule = type !== null && String(type).trim().toLowerCase() === 'module';
        if (!_lumen_script_has_empty_src(el.__nid__, isModule, type)) continue;
        _lumen_script_fire_empty_src_error(el.__nid__);
    }
}

// HTML LS §4.12.1 steps 11-32, minus the parser-only branches.
function _lumen_script_prepare(nid) {
    var type = _lumen_u2n(_lumen_get_attr(nid, 'type'));
    var isModule = type !== null && String(type).trim().toLowerCase() === 'module';
    // A non-JS type (importmap, application/json, speculationrules, a template
    // language, …) is a data block: it is never a script and never runs.
    if (!isModule && !_lumen_is_classic_script_type(type)) return;
    if (_lumen_script_has_empty_src(nid, isModule, type)) {
        _lumen_script_fire_empty_src_error(nid);
        return;
    }
    var src = _lumen_u2n(_lumen_get_attr(nid, 'src'));
    src = (src === null) ? '' : String(src).trim();
    if (src !== '') { _lumen_script_load_external(nid, src, isModule); return; }
    // `src` wins over the inline body; with no `src` the body is the source.
    var body = _lumen_u2n(_lumen_get_text_content(nid));
    if (body === null || String(body).trim() === '') return;
    body = String(body);
    if (!isModule) {
        // An inline classic script executes synchronously, inside the insertion
        // — `assert_equals(window.ran, true)` on the line after appendChild is
        // the canonical WPT shape and must already see the side effect.
        _lumen_script_execute_classic(body, nid);
        return;
    }
    // Module scripts are deferred by spec; a task hop is the approximation.
    setTimeout(function() {
        _lumen_script_run_module('', body).then(function() {
            _lumen_resource_fire(nid, 'load');
        }).catch(function(e) {
            _lumen_console_error('module script failed: ' + e);
            _lumen_resource_fire(nid, 'error');
        });
    }, 0);
}

// HTML LS §4.6.7 «process the linked resource»: a <link> whose `rel` makes it
// fetch a resource fires `load` on success and `error` on failure. Only the
// stylesheet case is wired here — it is the one pages await (a lazy-block
// loader that awaits `link.onload` before rendering hangs forever otherwise,
// BUG-703) and the one the shell already fetches for the cascade.
//
// The fetch below is a second, cache-warm request for the same URL rather than
// a report from the cascade loader: the shell re-collects link hrefs from the
// whole tree on restyle (`main.rs::collect_link_hrefs`) and has no per-node
// completion signal to forward. So `load` here means «the bytes arrived», not
// «the sheet is in the cascade» — the same approximation the <script> path
// above makes, and enough for the await that pages actually write.
// nid → 1 once this element's stylesheet outcome has been reported, whichever
// of the two paths got there first (BUG-804).
//
// The paths cannot be merged and must not both fire. A link the page builds
// with `createElement` is fetched here, from the insertion hook, while the
// document is still running its scripts; the shell's own cascade pass runs
// *after* those scripts and therefore sees that same element in the tree, so
// without the flag it would report a second `load` for it.
var _lumen_link_sheet_done = {};

// The shell's half: `[nid, ok, nid, ok, …]` for every `<link rel=stylesheet>`
// the cascade pass collected — the parser-written ones included, which is the
// whole point (they never pass through the insertion hook, so nothing here
// could otherwise know they exist, let alone how they ended).
//
// Dispatched inside this call rather than queued, even though §4.6.7 says
// «queue an element task». The task hop this path would defer to has nowhere
// earlier to land: the shell calls in once the cascade pass is done, no page
// code is on the stack, and a `setTimeout` from here waits for the next timer
// pump — which can fall *after* `window.onload`, where a stylesheet's `load`
// may not (in a real browser the sheet blocks the window load outright). The
// createElement path below keeps its task hop, because there the page's own
// `link.onload = …` really does follow the insertion.
function _lumen_deliver_parser_link_events(pairs) {
    if (!pairs || pairs.length === undefined) return;
    for (var i = 0; i + 1 < pairs.length; i += 2) {
        var nid = pairs[i];
        if (_lumen_link_sheet_done[nid] === 1) continue;
        _lumen_link_sheet_done[nid] = 1;
        _lumen_resource_fire(nid, pairs[i + 1] ? 'load' : 'error');
    }
}

function _lumen_link_prepare(nid) {
    var rel = _lumen_u2n(_lumen_get_attr(nid, 'rel'));
    if (rel === null) return;
    var toks = String(rel).toLowerCase().split(/\s+/);
    var isSheet = false;
    for (var i = 0; i < toks.length; i++) {
        if (toks[i] === 'stylesheet') isSheet = true;
    }
    // BUG-826: `preload`/`modulepreload`/`prefetch` ride the same insertion
    // hook. Independent of the stylesheet branch — `rel='preload stylesheet'`
    // is two link types on one element and the spec processes both.
    _lumen_link_hint_prepare(nid, toks);
    if (!isSheet) return;
    var href = _lumen_u2n(_lumen_get_attr(nid, 'href'));
    href = (href === null) ? '' : String(href).trim();
    // No href → no resource is obtained, so neither event fires.
    if (href === '') return;
    // Claim the element before the fetch starts: the shell's cascade pass runs
    // once every script of the document has finished, so it would otherwise
    // report this same link a second time (BUG-804).
    if (_lumen_link_sheet_done[nid] === 1) return;
    _lumen_link_sheet_done[nid] = 1;
    // Task hop for the same reason as the external <script> path: the
    // `link.onload = …` assignment almost always follows the appendChild.
    setTimeout(function() {
        var url = _url_resolve(href, _lumen_document_base_url());
        fetch(url, { _lumenInitiatorType: 'link' }).then(function(resp) {
            if (!resp.ok) throw new Error('HTTP ' + resp.status);
            // Drain the body: an unread response holds its fetch slot (BUG-721).
            return resp.text();
        }).then(function() {
            _lumen_resource_fire(nid, 'load');
        }).catch(function(e) {
            _lumen_console_error('stylesheet load failed: ' + url + ': ' + e);
            _lumen_resource_fire(nid, 'error');
        });
    }, 0);
}

// ── <link> resource hints: preload / modulepreload / prefetch / icon ─────────
// ── (BUG-826, `icon` added by BUG-848) ────────────────────────────────────────
//
// HTML LS §4.6.7 «link type preload» and «link type modulepreload» (`icon` is
// its own, separate §4.6.7 link type, folded in here for the fetch shape it
// shares with `prefetch`). Before this, a hint reached
// `Event::SubresourceHintFound` in the shell, which only printed it to
// stderr — so the log claimed a preload/icon fetch had happened while no
// request was ever made and the element reported nothing to the page.
//
// The fetch lives here rather than in the shell for the same reason the
// stylesheet path above does: `load`/`error` belong to the element, and the
// shell has no per-node completion signal to forward. It costs the early start
// the preload scanner exists for — the request now begins once the DOM is
// parsed, not while the HTML is still streaming — which is the residual left on
// the bug.

// The `as` attribute is enumerated over the Fetch destinations. A value outside
// this table leaves it in *no* state, which for `rel=preload` means exactly
// what an absent `as` means: no resource is obtained, and the element fires
// neither `load` nor `error` (WPT `preload/onload-event.html` asserts both).
var _LUMEN_LINK_AS_DESTINATIONS = {
    'audio': 1, 'audioworklet': 1, 'document': 1, 'embed': 1, 'fetch': 1,
    'font': 1, 'frame': 1, 'iframe': 1, 'image': 1, 'json': 1, 'manifest': 1,
    'object': 1, 'paintworklet': 1, 'report': 1, 'script': 1, 'serviceworker': 1,
    'sharedworker': 1, 'style': 1, 'track': 1, 'video': 1, 'webidentity': 1,
    'worker': 1, 'xslt': 1
};

// Destinations a `rel=modulepreload` can serve: the script-like set plus the
// module types a module map can hold. A *valid* keyword outside it (`as=image`,
// `as=font`, …) is a destination the module fetch cannot produce, so the
// element reports `error`; an absent or unrecognized keyword is in no state and
// falls back to the default destination «script», which succeeds.
var _LUMEN_MODULEPRELOAD_DESTINATIONS = {
    'audioworklet': 1, 'json': 1, 'paintworklet': 1, 'script': 1,
    'serviceworker': 1, 'sharedworker': 1, 'style': 1, 'worker': 1
};

// nid → 1 once the element's hint has been acted on. Keyed per node, not per
// URL: the same element must not fetch twice when it is moved around the tree,
// and the parser-document pass below must not re-run a hint the insertion hook
// already ran (the two paths overlap for a link a head script appends).
var _lumen_link_hint_done = {};

// Which of the four hint types this `rel` carries, '' for none. First token
// wins — an element is one hint at a time, and `rel='preload prefetch'` is not
// a shape the spec gives a combined meaning to.
//
// `icon` (BUG-848) is not a "resource hint" in HTML LS §4.6.7's own taxonomy —
// it is §4.6.7's separate "link type icon" — but it fetches and reports
// load/error the same way `prefetch` does (no `as`/`type` gating, either),
// so it rides this dispatcher instead of a fourth copy of the fetch shape.
// `rel="shortcut icon"` needs no special case: the token split below already
// hands `_lumen_link_hint_kind` the plain `icon` token from it.
function _lumen_link_hint_kind(toks) {
    for (var i = 0; i < toks.length; i++) {
        var t = toks[i];
        if (t === 'preload' || t === 'modulepreload' || t === 'prefetch' || t === 'icon') return t;
    }
    return '';
}

// §4.6.7: a `media` that does not match the environment means the resource is
// not obtained — and therefore no event either way. A media query the engine
// cannot evaluate is treated as matching, which errs towards fetching.
function _lumen_link_hint_media_matches(nid) {
    var media = _lumen_u2n(_lumen_get_attr(nid, 'media'));
    if (media === null || String(media).trim() === '') return true;
    try { return !!matchMedia(String(media)).matches; } catch (e) { return true; }
}

// §4.6.7: a `type` the destination cannot consume also means «no resource is
// obtained», silently. Only the two destinations whose MIME set the engine
// actually knows are checked; for every other destination a `type` is accepted,
// which is the lenient half of the same rule.
function _lumen_link_hint_type_supported(dest, type) {
    if (type === null || type === undefined) return true;
    var t = String(type).trim().toLowerCase();
    if (t === '') return true;
    var base = t.split(';')[0].trim();
    if (dest === 'style') return base === 'text/css';
    if (dest === 'script') return _lumen_is_classic_script_type(base);
    return true;
}

// The shared fetch. `onBody(url, resp, text)` — optional hook run before the
// `load` event; anything it throws turns the hint into an `error`, which is the
// right shape for modulepreload (a body that cannot enter the module map is a
// failed preload).
function _lumen_link_hint_fetch(nid, href, onBody) {
    // Task hop for the same reason as the <script>/stylesheet paths above: the
    // `link.onload = …` assignment almost always follows the appendChild.
    setTimeout(function() {
        var url = _url_resolve(String(href), _lumen_document_base_url());
        fetch(url, { _lumenInitiatorType: 'link' }).then(function(resp) {
            if (!resp.ok) throw new Error('HTTP ' + resp.status);
            // Drain the body even when nothing reads it: an unread response
            // holds its fetch slot (BUG-721).
            return resp.text().then(function(text) {
                if (onBody) onBody(url, resp, text);
            });
        }).then(function() {
            _lumen_resource_fire(nid, 'load');
        }).catch(function(e) {
            _lumen_console_error('link hint fetch failed: ' + url + ': ' + e);
            _lumen_resource_fire(nid, 'error');
        });
    }, 0);
}

// `rel=preload`, §4.6.7 «process the linked resource».
function _lumen_link_preload(nid, href) {
    var asAttr = _lumen_u2n(_lumen_get_attr(nid, 'as'));
    var dest = (asAttr === null) ? '' : String(asAttr).trim().toLowerCase();
    // No state (absent or unrecognized) → nothing is fetched and nothing is
    // reported. Deliberately silent: a page must be able to write a preload for
    // a destination this engine has never heard of without seeing a spurious
    // failure.
    if (_LUMEN_LINK_AS_DESTINATIONS[dest] !== 1) return;
    if (!_lumen_link_hint_media_matches(nid)) return;
    if (!_lumen_link_hint_type_supported(dest, _lumen_u2n(_lumen_get_attr(nid, 'type')))) return;
    _lumen_link_hint_fetch(nid, href, null);
}

// `rel=modulepreload`, §4.6.7 «fetch a modulepreload module script graph».
// Only the entry point is fetched — the graph's static imports are not walked,
// which matches how far the engine's module loading goes anyway (BUG-446).
function _lumen_link_modulepreload(nid, href) {
    var asAttr = _lumen_u2n(_lumen_get_attr(nid, 'as'));
    var dest = (asAttr === null) ? '' : String(asAttr).trim().toLowerCase();
    if (dest !== ''
        && _LUMEN_LINK_AS_DESTINATIONS[dest] === 1
        && _LUMEN_MODULEPRELOAD_DESTINATIONS[dest] !== 1) {
        // A destination the module fetch cannot serve fails loudly, unlike the
        // silent preload case above — the spec fires `error` here.
        setTimeout(function() { _lumen_resource_fire(nid, 'error'); }, 0);
        return;
    }
    _lumen_link_hint_fetch(nid, href, function(url, resp, text) {
        // Seed the module map so a later `import` of the same URL reuses these
        // bytes instead of fetching them again — the whole point of the hint.
        // Only for a JavaScript response: registering a CSS or JSON body under
        // the URL would turn a later import's type rejection into a syntax
        // error, which is a worse answer than the extra request (BUG-896).
        var ct = null;
        try { ct = resp.headers.get('content-type'); } catch (e) {}
        if (ct === null || ct === undefined) return;
        var base = String(ct).split(';')[0].trim().toLowerCase();
        if (!_lumen_is_classic_script_type(base)) return;
        try { _lumen_esm_register(url, text); } catch (e) {}
    });
}

// Act on one element's hint, once. `toks` — the already lower-cased `rel`
// tokens.
function _lumen_link_hint_prepare(nid, toks) {
    if (_lumen_link_hint_done[nid] === 1) return;
    var kind = _lumen_link_hint_kind(toks);
    if (kind === '') return;
    var href = _lumen_u2n(_lumen_get_attr(nid, 'href'));
    href = (href === null) ? '' : String(href).trim();
    // No href → no resource is obtained, so neither event fires.
    if (href === '') return;
    _lumen_link_hint_done[nid] = 1;
    if (kind === 'preload') { _lumen_link_preload(nid, href); return; }
    if (kind === 'modulepreload') { _lumen_link_modulepreload(nid, href); return; }
    // `prefetch` and `icon` (BUG-848) are both destination-agnostic: prefetch
    // warms the cache for a future navigation, icon has no `as` at all — so
    // neither is gated by `as`/`type` (WPT `preload/prefetch-events`).
    _lumen_link_hint_fetch(nid, href, null);
}

// The parser's half. A `<link>` written by the HTML parser never goes through
// the insertion hook above (that one covers elements minted by createElement),
// so the hints in the markup are picked up in one pass when parsing is done.
// Walks the whole document rather than just `<head>` — a `<link>` in the body
// is conforming markup and WPT uses it.
function _lumen_link_hints_scan() {
    var links;
    try { links = document.getElementsByTagName('link'); } catch (e) { return; }
    if (!links) return;
    for (var i = 0; i < links.length; i++) {
        var el = links[i];
        if (!el || el.__nid__ === undefined) continue;
        var rel = _lumen_u2n(_lumen_get_attr(el.__nid__, 'rel'));
        if (rel === null) continue;
        _lumen_link_hint_prepare(el.__nid__, String(rel).toLowerCase().split(/\s+/));
    }
}

// ── <style>: HTML LS §4.14 «update a style block» (BUG-804) ──────────────────
//
// `<style>` never reported anything to the page, on ANY insertion path: it is
// not in `_lumen_resource_track`'s tag list, and nothing else in the shim knew
// the element existed. That is a wider hole than the parser one this bug is
// mostly about — `style.onload` could not fire even for an element the page had
// just built itself.
//
// The whole model lives here rather than in the shell, and for a stronger
// reason than the <script>/<link> halves: a style block has no network load to
// report at all, so there is nothing for the shell to know. It is «the sheet is
// now built» — an event that belongs to the element and to the DOM, and the DOM
// is here. The one thing that CAN fail is an `@import` inside the block, which
// §4.14 makes the element report as `error`.

// nid → 1 once the element has had its first update run. Only the *first* one
// is guarded: §4.14 re-runs on every child-list change, and
// `style_load_event.html` asserts exactly that (two `load`s for one parse plus
// one `textContent` write). The flag exists so the parser pass below does not
// re-do an update the insertion hook already performed.
var _lumen_style_updated = {};

// `type` decides whether the element has an associated sheet at all. Absent,
// empty, or an ASCII case-insensitive `text/css` — it does; anything else and
// §4.14 returns before building one, so no event is due either.
function _lumen_style_type_is_css(nid) {
    var type = _lumen_u2n(_lumen_get_attr(nid, 'type'));
    if (type === null) return true;
    var s = String(type).trim();
    return s === '' || s.toLowerCase() === 'text/css';
}

// The `@import` URLs of a style block, in source order.
//
// Deliberately a scan of the text rather than a CSS parse: the shell owns the
// real cascade, and all this needs to answer is «which subresources does this
// block claim to need», so that a block whose import cannot be obtained reports
// `error` (`style_events.html`, `style-error-01.html`). A malformed `@import`
// the real parser would drop simply does not match here.
var _LUMEN_STYLE_IMPORT_RE =
    /@import\s+(?:url\(\s*(?:"([^"]*)"|'([^']*)'|([^)"'\s]*))\s*\)|"([^"]*)"|'([^']*)')/g;

function _lumen_style_import_urls(text) {
    var out = [];
    if (!text) return out;
    _LUMEN_STYLE_IMPORT_RE.lastIndex = 0;
    var m;
    while ((m = _LUMEN_STYLE_IMPORT_RE.exec(text)) !== null) {
        var url = m[1] || m[2] || m[3] || m[4] || m[5] || '';
        if (url !== '') out.push(url);
        // A zero-length match would spin forever; `@import` is 7 chars, so the
        // regex cannot match empty, but the guard is free.
        if (m.index === _LUMEN_STYLE_IMPORT_RE.lastIndex) _LUMEN_STYLE_IMPORT_RE.lastIndex++;
    }
    return out;
}

// §4.14 «update a style block» for one element, plus the event it owes.
//
// `now` says whether the element may report inside this turn. It is false for
// every DOM-driven update, because `style.onload = …` is normally assigned on
// the line *after* the insertion that triggers this, and because
// `style_load_async.html` asserts the handler does not run synchronously.
//
// It is true for the parser pass, and that is not an optimization: the spec
// fires a parser-written block's `load` during parsing, well before `load` on
// the window. The pass already runs at the latest defensible moment
// (`readyState` → `interactive`), so a further task hop pushes the event past
// `window.onload` — where `style_load_event.html` reads `loadCount` and found
// 0. There is nothing left to defer to.
function _lumen_style_update_block(nid, now) {
    _lumen_style_updated[nid] = 1;
    if (!_lumen_style_type_is_css(nid)) return;
    var text = _lumen_u2n(_lumen_get_text_content(nid));
    var imports = _lumen_style_import_urls(text === null ? '' : String(text));
    if (imports.length === 0) {
        if (now) { _lumen_resource_fire(nid, 'load'); return; }
        setTimeout(function() { _lumen_resource_fire(nid, 'load'); }, 0);
        return;
    }
    // An `@import` has to be obtained before the block can report, so this
    // branch is asynchronous whatever `now` says.
    setTimeout(function() { _lumen_style_load_imports(nid, imports); }, 0);
}

// Obtain every `@import` of the block; the element reports `load` only if all
// of them arrived as CSS.
//
// The content type is checked, not just the status: `style-error-01.html`
// imports a `text/plain` file that a server answers with 200, and §4.14 wants
// `error` for it — a sheet that cannot be parsed as CSS was not obtained.
function _lumen_style_load_imports(nid, imports) {
    var base = _lumen_document_base_url();
    var pending = imports.length;
    var failed = false;
    var settle = function() {
        if (--pending > 0) return;
        _lumen_resource_fire(nid, failed ? 'error' : 'load');
    };
    for (var i = 0; i < imports.length; i++) {
        (function(href) {
            var url;
            try { url = _url_resolve(href, base); }
            catch (e) { failed = true; settle(); return; }
            fetch(url, { _lumenInitiatorType: 'css' }).then(function(resp) {
                if (!resp.ok) throw new Error('HTTP ' + resp.status);
                var ctype = '';
                try { ctype = String(resp.headers.get('content-type') || ''); } catch (e) {}
                // An absent Content-Type is not evidence of the wrong one —
                // only a type that is present and is not CSS refuses the sheet.
                if (ctype !== '' && ctype.split(';')[0].trim().toLowerCase() !== 'text/css') {
                    throw new Error('not CSS: ' + ctype);
                }
                // Drain the body: an unread response holds its fetch slot
                // (BUG-721).
                return resp.text();
            }).then(function() { settle(); }).catch(function(e) {
                _lumen_console_error('@import failed: ' + url + ': ' + e);
                failed = true;
                settle();
            });
        })(imports[i]);
    }
}

// Is this node a `<style>` element? By nid alone — this runs on the insertion
// and text-mutation hot paths, so no wrapper is allocated.
function _lumen_is_style_element(nid) {
    if (nid === null || nid === undefined) return false;
    var local;
    try { local = _lumen_u2n(_lumen_get_local_name(nid)); } catch (e) { return false; }
    return local !== null && String(local).toLowerCase() === 'style';
}

// A child list changed under `parent`: if that parent is a connected `<style>`,
// its block is rebuilt and the element reports again. This is the half
// `style_load_event.html` and `style-load-after-mutate.html` are about — the
// first appends text to an already-inserted element, the second writes
// `textContent` long after load.
function _lumen_style_children_changed(parent) {
    if (!_lumen_is_style_element(parent)) return;
    if (!_lumen_resource_is_connected(parent)) return;
    _lumen_style_update_block(parent, false);
}

// The parser's half, run once the document is parsed — same shape and same
// reason as `_lumen_link_hints_scan`: a `<style>` written by the parser never
// passes through the insertion hook. Elements the hook already handled carry
// the flag and are skipped, so a page whose head script builds a `<style>`
// still reports exactly one `load` for it.
function _lumen_style_blocks_scan() {
    var styles;
    try { styles = document.getElementsByTagName('style'); } catch (e) { return; }
    if (!styles) return;
    for (var i = 0; i < styles.length; i++) {
        var el = styles[i];
        if (!el || el.__nid__ === undefined) continue;
        if (_lumen_style_updated[el.__nid__] === 1) continue;
        _lumen_style_update_block(el.__nid__, true);
    }
}

// Run the pending check for one tracked element, if it is connected by now.
function _lumen_resource_try_prepare(nid) {
    var kind = _lumen_resource_pending[nid];
    // BUG-775: <track> is the odd one out — HTML LS §4.8.11.1 starts the track
    // processing model as soon as the element's *parent* is a media element, and
    // says nothing about being in a document (half of WPT's WebVTT tests never
    // append the <video> anywhere). The media shim owns the whole model and
    // answers false while the element is not yet parented to a <video>/<audio>,
    // in which case the element stays tracked for a later insertion.
    if (kind === 'track') {
        if (typeof _lumen_track_start_load !== 'function') return;
        if (!_lumen_track_start_load(nid)) return;
        delete _lumen_resource_pending[nid];
        _lumen_resource_pending_count--;
        return;
    }
    // BUG-825: <source> is the same shape as <track> — HTML LS §4.8.11.5 keys
    // «invoke the media load algorithm» on the parent being a media element and
    // says nothing about being in a document, and the media shim answers false
    // while the parent is not one, so the element stays tracked for a later
    // insertion.
    if (kind === 'source') {
        if (typeof _lumen_media_source_inserted !== 'function') return;
        if (!_lumen_media_source_inserted(nid)) return;
        delete _lumen_resource_pending[nid];
        _lumen_resource_pending_count--;
        return;
    }
    if (kind !== 'script' && kind !== 'link' && kind !== 'style') return;
    if (!_lumen_resource_is_connected(nid)) return;
    delete _lumen_resource_pending[nid];
    _lumen_resource_pending_count--;
    // BUG-804: `<style>` has no resource to fetch — becoming connected IS the
    // trigger, because §4.14 runs «update a style block» when the element
    // becomes browsing-context connected and it has children. Later child
    // changes are picked up by `_lumen_style_children_changed`, not here: the
    // element leaves the pending map on its first update, exactly like the
    // spec's own «already started» bookkeeping for the other kinds.
    if (kind === 'style') { _lumen_style_update_block(nid, false); return; }
    if (kind === 'link') { _lumen_link_prepare(nid); return; }
    _lumen_script_prepare(nid);
}

// Insertion hook. Fast path when no dynamically created script is outstanding
// (one property read), which is every page that never calls
// createElement('script') and every insertion after the last one has run.
function _lumen_resource_after_insert(childNid) {
    if (_lumen_resource_pending_count === 0) return;
    if (_lumen_resource_pending[childNid] !== undefined) {
        _lumen_resource_try_prepare(childNid);
        return;
    }
    // The inserted node may be an *ancestor* of a tracked element that was built
    // detached (`div.appendChild(script)` before `body.appendChild(div)`) and
    // only becomes connected now. Deleting the current key inside for-in is
    // well-defined, and the map holds only not-yet-prepared elements.
    for (var k in _lumen_resource_pending) _lumen_resource_try_prepare(+k);
}

// Route every tree insertion through the hook by wrapping the two natives that
// all of them bottom out in. One place instead of the ~30 shim call sites
// (appendChild, insertBefore, replaceChild, append/prepend/before/after/
// replaceWith, <select>.add, insertAdjacentElement, …), so a future insertion
// path cannot silently miss it.
//
// BUG-804 adds the mirror question to the same two natives: not «what became
// connected» but «whose children changed», which is what re-runs a `<style>`
// block. Both are asked here so a page that builds `<style>` + text in either
// order reports once for each update.
var _lumen_native_append_child  = _lumen_append_child;
var _lumen_native_insert_before = _lumen_insert_before;
_lumen_append_child = function(parent, child) {
    _lumen_native_append_child(parent, child);
    _lumen_resource_after_insert(child);
    _lumen_style_children_changed(parent);
};
_lumen_insert_before = function(parent, child, reference) {
    _lumen_native_insert_before(parent, child, reference);
    _lumen_resource_after_insert(child);
    _lumen_style_children_changed(parent);
};

// `textContent =` / `innerHTML =` replace a style block's children wholesale,
// which §4.14 treats exactly like an append — and it is the form
// `style_load_event.html` writes. Wrapped here, below the natives and above the
// MutationObserver wrappers further down, so both wrappers see every write:
// the MO one captures whatever `_lumen_set_text_content` names at ITS line,
// i.e. these.
var _lumen_native_set_text_content = _lumen_set_text_content;
var _lumen_native_set_inner_html   = _lumen_set_inner_html;
_lumen_set_text_content = function(nid, text) {
    _lumen_native_set_text_content(nid, text);
    _lumen_style_children_changed(nid);
};
_lumen_set_inner_html = function(nid, html) {
    _lumen_native_set_inner_html(nid, html);
    _lumen_style_children_changed(nid);
};

// ── Custom Elements registry ──────────────────────────────────────────────────
// Maps lower-case tag name → { ctor, observedAttributes: string[] }
var _lumen_ce_registry = {};
// Maps tag name → array of resolve callbacks for whenDefined().
var _lumen_ce_pending  = {};

// Calls connectedCallback on `el` if its tag is in the registry.
function _lumen_ce_maybe_connected(el) {
    if (!el || el.__nid__ === undefined) return;
    var tag   = _lumen_get_tag_name(el.__nid__).toLowerCase();
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    if (!el.__ceUpgraded__) {
        el.__ceUpgraded__ = true;
    }
    if (typeof entry.ctor.prototype.connectedCallback === 'function') {
        try { entry.ctor.prototype.connectedCallback.call(el); } catch(e) {
            _lumen_console_error('CE connectedCallback: ' + e);
        }
    }
}

// Calls disconnectedCallback on `el` if its tag is in the registry.
function _lumen_ce_maybe_disconnected(el) {
    if (!el || el.__nid__ === undefined) return;
    var tag   = _lumen_get_tag_name(el.__nid__).toLowerCase();
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    if (typeof entry.ctor.prototype.disconnectedCallback === 'function') {
        try { entry.ctor.prototype.disconnectedCallback.call(el); } catch(e) {
            _lumen_console_error('CE disconnectedCallback: ' + e);
        }
    }
}

// Calls attributeChangedCallback on the element at `nid` if applicable.
function _lumen_ce_maybe_attr_changed(nid, attrName, oldVal, newVal) {
    var tag   = _lumen_get_tag_name(nid).toLowerCase();
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    if (entry.observedAttributes.indexOf(attrName) < 0) return;
    if (typeof entry.ctor.prototype.attributeChangedCallback === 'function') {
        try {
            entry.ctor.prototype.attributeChangedCallback.call(
                _lumen_make_element(nid), attrName, oldVal, newVal
            );
        } catch(e) {
            _lumen_console_error('CE attributeChangedCallback: ' + e);
        }
    }
}

// Upgrades a single element wrapper: marks upgraded and calls connectedCallback.
function _lumen_ce_upgrade_element(el, entry) {
    if (!el || el.__ceUpgraded__) return;
    el.__ceUpgraded__ = true;
    if (typeof entry.ctor.prototype.connectedCallback === 'function') {
        try { entry.ctor.prototype.connectedCallback.call(el); } catch(e) {
            _lumen_console_error('CE connectedCallback (upgrade): ' + e);
        }
    }
}

// Upgrades all DOM elements matching `tag` that haven't been upgraded yet.
function _lumen_ce_upgrade_all(tag) {
    var nids = _lumen_query_selector_all(tag);
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    for (var i = 0; i < nids.length; i++) {
        _lumen_ce_upgrade_element(_lumen_make_element(nids[i]), entry);
    }
}

var customElements = {
    define: function(name, ctor, options) {
        name = String(name).toLowerCase();
        if (_lumen_ce_registry[name]) return;
        var observed = (ctor.observedAttributes && ctor.observedAttributes.length)
            ? ctor.observedAttributes.slice()
            : [];
        _lumen_ce_registry[name] = { ctor: ctor, observedAttributes: observed };
        _lumen_ce_upgrade_all(name);
        var pending = _lumen_ce_pending[name];
        if (pending) {
            for (var i = 0; i < pending.length; i++) {
                try { pending[i](ctor); } catch(e) {}
            }
            delete _lumen_ce_pending[name];
        }
    },
    get: function(name) {
        var entry = _lumen_ce_registry[String(name).toLowerCase()];
        return entry ? entry.ctor : undefined;
    },
    whenDefined: function(name) {
        name = String(name).toLowerCase();
        var entry = _lumen_ce_registry[name];
        if (entry) return Promise.resolve(entry.ctor);
        return new Promise(function(resolve) {
            if (!_lumen_ce_pending[name]) _lumen_ce_pending[name] = [];
            _lumen_ce_pending[name].push(resolve);
        });
    },
    upgrade: function(element) {
        if (!element || element.__nid__ === undefined) return;
        var tag   = _lumen_get_tag_name(element.__nid__).toLowerCase();
        var entry = _lumen_ce_registry[tag];
        if (entry) _lumen_ce_upgrade_element(element, entry);
    },
};

// ── location (HTML LS §7.7 + WHATWG URL §8) ──────────────────────────────────
