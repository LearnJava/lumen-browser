
function _lumen_u2n(v) { return v !== undefined ? v : null; }

// BUG-479: shared plumbing for the Promise-returning scroll methods
// (`Element`/`window` `scrollTo`/`scrollBy`/`scrollIntoView`, CSSOM View
// Module's "Scrolling with a promise"). The engine only dispatches
// `scroll`/`scrollend` when a queued scroll request actually moves the
// position (element containers: `set_scroll_position` finding the node still
// counts as "moved" even at a clamped no-op edge, so the listener alone is
// enough there; the page path is stricter and drops the events entirely for
// a no-op `window.scrollTo`), so a promise that only waited for `scrollend`
// could hang forever on a request that settles without ever moving anything.
// `sample()` returns this call's `[x, y]` scroll position; two chained
// `requestAnimationFrame`s put the fallback check after the rendering update
// that drains the just-queued request (and dispatches its `scroll`/`scrollend`
// pair, if any) has run — see `about_to_wait.rs`'s scroll-request drain and
// `on_redraw_requested`'s step-1 comment for why one round trip is enough.
// If the position moved at all by then, a real sequence is underway (an
// instant scroll's `scrollend` already resolved the promise via the listener
// by this point; a smooth animation keeps moving and is left to its own
// `scrollend`) — the fallback only ever fires for the genuine no-op case.
function _lumen_scroll_settle_promise(target, sample) {
    return new Promise(function(resolve) {
        var done = false;
        var before = sample();
        function finish() {
            if (done) return;
            done = true;
            target.removeEventListener('scrollend', onEnd);
            resolve();
        }
        function onEnd() { finish(); }
        target.addEventListener('scrollend', onEnd);
        if (typeof requestAnimationFrame !== 'function') { finish(); return; }
        requestAnimationFrame(function() {
            requestAnimationFrame(function() {
                if (done) return;
                var now = sample();
                if (now[0] === before[0] && now[1] === before[1]) { finish(); }
            });
        });
    });
}

// BUG-479: shared alignment maths for `scrollIntoView({block|inline})` —
// CSSOM View Module's "scroll an element into view" §6 step 3, one axis at a
// time. `contentPos`/`targetSize` are the target's position/extent in the
// container's UNSCROLLED content space (i.e. what its scroll offset would
// have to equal for the target's start edge to sit at the container's start
// edge); `clientSize` is the container's own (scroll-independent) box size;
// `curScroll` is only read for `'nearest'`, to test whether the target is
// already visible at the CURRENT offset.
function _lumen_align_scroll(contentPos, targetSize, clientSize, curScroll, align) {
    switch (align) {
        case 'end': return contentPos + targetSize - clientSize;
        case 'center': return contentPos + targetSize / 2 - clientSize / 2;
        case 'nearest': {
            var visStart = contentPos - curScroll;
            var visEnd = visStart + targetSize;
            if (visStart >= 0 && visEnd <= clientSize) return curScroll;
            return visStart < 0 ? contentPos : contentPos + targetSize - clientSize;
        }
        case 'start':
        default: return contentPos;
    }
}

// BUG-479: normalises `scrollIntoView`'s argument — legacy boolean
// (`alignToTop`), a `ScrollIntoViewOptions` dict, or omitted — into
// `{block, inline, behavior}` with the spec's defaults, validating enum
// members the way WebIDL enum coercion would (an unrecognised value is a
// TypeError, not a silent fallback to the default).
function _lumen_parse_scroll_into_view_opts(arg) {
    if (arg === false) return { block: 'end', inline: 'nearest', behavior: 'auto' };
    if (arg === undefined || arg === true || typeof arg !== 'object' || arg === null) {
        return { block: 'start', inline: 'nearest', behavior: 'auto' };
    }
    var positions = ['start', 'center', 'end', 'nearest'];
    var behaviors = ['auto', 'instant', 'smooth'];
    var block = arg.block === undefined ? 'start' : String(arg.block);
    var inline = arg.inline === undefined ? 'nearest' : String(arg.inline);
    var behavior = arg.behavior === undefined ? 'auto' : String(arg.behavior);
    if (positions.indexOf(block) === -1) {
        throw new TypeError("Failed to execute 'scrollIntoView': The provided value '" + block + "' is not a valid enum value of type ScrollLogicalPosition.");
    }
    if (positions.indexOf(inline) === -1) {
        throw new TypeError("Failed to execute 'scrollIntoView': The provided value '" + inline + "' is not a valid enum value of type ScrollLogicalPosition.");
    }
    if (behaviors.indexOf(behavior) === -1) {
        throw new TypeError("Failed to execute 'scrollIntoView': The provided value '" + behavior + "' is not a valid enum value of type ScrollBehavior.");
    }
    return { block: block, inline: inline, behavior: behavior };
}

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
