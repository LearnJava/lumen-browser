
function _lumen_u2n(v) { return v !== undefined ? v : null; }

// BUG-391: DOM LS §4.2.6 / §4.9 — querySelector(All)/matches/closest must throw
// a SyntaxError DOMException for a selector that is invalid or that the engine
// does not recognise, instead of silently reporting «nothing matched». The
// distinction is invisible to the natives (they return an empty result either
// way), so every public entry point stringifies its argument through this
// helper first and gets the validated string back. Internal callers that must
// never throw (getElementsByTagName/ClassName, the slot walker) keep calling
// the natives directly.
function _lumen_sel(sel) {
    var s = String(sel);
    if (!_lumen_selector_is_valid(s)) {
        throw new DOMException(s + ' is not a valid selector', 'SyntaxError');
    }
    return s;
}

// Engine-agnostic «is this content attribute present?». A missing attribute
// comes back as `undefined` from the QuickJS bindings but as `null` from the V8
// ones (BUG-442), so the bare `_lumen_get_attr(...) !== undefined` test used
// elsewhere in this shim reports «present» for every name on the default engine.
// Normalise through `_lumen_u2n` and compare against `null` instead.
function _lumen_has_attr(nid, name) {
    return _lumen_u2n(_lumen_get_attr(nid, name)) !== null;
}

// DOM LS §4.5/§4.9: getElementsByClassName(names) matches elements carrying
// EVERY whitespace-separated class token. Build a compound CSS class selector
// ('.a.b') and reuse the native query the CSS engine already runs — same
// static-array simplification `getElementsByTagName` makes (BUG-302). Returns
// null for an empty token list so callers can short-circuit to an empty array
// (a '' selector would otherwise throw in the query engine).
function _lumen_class_selector(names) {
    var parts = String(names).split(/\s+/).filter(function (s) { return s.length > 0; });
    if (parts.length === 0) return null;
    return parts.map(function (c) { return '.' + c; }).join('');
}

// ── Event / CustomEvent constructors ─────────────────────────────────────────

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
}
Event.prototype.preventDefault = function() {
    if (this.cancelable) this.defaultPrevented = true;
};
Event.prototype.stopPropagation = function() { this.cancelBubble = true; };
Event.prototype.stopImmediatePropagation = function() { this._stopImmediate = true; this.cancelBubble = true; };

function CustomEvent(type, init) {
    Event.call(this, type, init);
    this.detail = (init && init.detail !== undefined) ? init.detail : null;
}
CustomEvent.prototype = Object.create(Event.prototype);
CustomEvent.prototype.constructor = CustomEvent;
