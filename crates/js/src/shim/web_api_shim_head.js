
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
function _lumen_scroll_settle_promise(target, sample, key) {
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
                // SMOOTH-SCROLL: a stepped animation pays its own `scrollend`
                // when it ends — until then the position may not have moved yet
                // (the first frame of an ease-out is ~0).
                if (key !== undefined && _lumen_smooth_anims[key] !== undefined) return;
                var now = sample();
                if (now[0] === before[0] && now[1] === before[1]) { finish(); }
            });
        });
    });
}

// SMOOTH-SCROLL: CSSOM View «smooth scrolling». The engine's scroll requests
// are instant (the shell applies them in one step and dispatches `scroll` +
// `scrollend` for each), so a smooth scroll is stepped from here: one instant
// request per animation frame along an ease-out-cubic curve, with the native
// `scrollend` of every intermediate step held back (`_lumen_fire_*scrollend*`
// in `web_api_shim_tail_b.js` look at `_lumen_smooth_anims`) and a single one
// paid when the sequence ends. A scroller is addressed by a key — `'p'` for
// the viewport, the node id for an element. A newer request for the same
// scroller replaces the running animation (same target: left alone, so a
// caller that re-issues `scrollTo` every tick does not restart the curve),
// an instant request cancels it, and a position nobody asked for (wheel,
// keys, scrollbar drag, the container's edge) ends it where it stands.
var _lumen_smooth_anims = {};
var _lumen_smooth_raf = false;
var _lumen_scrollend_bypass = false;
var _LUMEN_SMOOTH_MS = 200;      // = `scroll_anim::DURATION_MS` of the shell's wheel/key scroll
var _LUMEN_SMOOTH_SETTLE = 2;    // frames between the last step and `scrollend`: the shell has applied it

function _lumen_scroll_in_frame() {
    try { return window.parent !== window; } catch (e) { return true; }
}

// `behavior` of `ScrollOptions` → does this call animate? `auto` defers to the
// scrolling box's own `scroll-behavior` (`cssNid`: the element, or the root
// element for the viewport). A sub-document has no frame loop of its own
// (`rAF` of frames does not run), so it always scrolls instantly.
function _lumen_use_smooth(behavior, cssNid) {
    if (behavior === 'instant') return false;
    if (typeof requestAnimationFrame !== 'function' || _lumen_scroll_in_frame()) return false;
    if (behavior === 'smooth') return true;
    if (cssNid === null || cssNid === undefined) return false;
    return _lumen_get_computed_style(cssNid, 'scroll-behavior') === 'smooth';
}

function _lumen_scroller_get(key) {
    if (key === 'p') return [_lumen_get_page_scroll_x(), _lumen_get_page_scroll_y()];
    var s = _lumen_get_scroll_state(key);
    return s ? [s[0], s[1]] : [0, 0];
}

function _lumen_scroller_set(key, x, y) {
    if (key === 'p') {
        _lumen_request_page_scroll_x(x);
        _lumen_request_page_scroll(y, 0);
    } else {
        _lumen_request_scroll(key, x, y);
    }
}

// The viewport's scroll range, from the root's scrollable overflow area
// (`_lumen_get_scroll_state` has an entry for it, BUG-960) and the viewport.
function _lumen_page_scroll_max() {
    var hid = _lumen_u2n(_lumen_get_html_element());
    var vp = _lumen_get_viewport_size();
    var sw = vp[0], sh = vp[1];
    if (hid !== null) {
        var st = _lumen_get_scroll_state(hid);
        if (st) { sw = Math.max(sw, st[2]); sh = Math.max(sh, st[3]); }
        else {
            var r = _lumen_get_bounding_rect(hid);
            if (r) { sw = Math.max(sw, r[2]); sh = Math.max(sh, r[3]); }
        }
    }
    return [Math.max(0, sw - vp[0]), Math.max(0, sh - vp[1])];
}

function _lumen_smooth_kick() {
    if (_lumen_smooth_raf) return;
    _lumen_smooth_raf = true;
    requestAnimationFrame(_lumen_smooth_tick);
}

// Start (or retarget) the smooth scroll of `key` towards (tx, ty).
function _lumen_smooth_scroll(key, tx, ty) {
    var cur = _lumen_scroller_get(key);
    if (key === 'p') {
        var max = _lumen_page_scroll_max();
        tx = Math.min(Math.max(tx, 0), max[0]);
        ty = Math.min(Math.max(ty, 0), max[1]);
    }
    var a = _lumen_smooth_anims[key];
    // The same target as the running (or just landed, `scrollend` still owed)
    // animation: nothing to do — a caller re-issuing the call every timer tick
    // must neither restart the curve nor cancel the pending `scrollend`.
    if (a !== undefined && (a.fin === 0 || a.landed) && Math.abs(a.tx - tx) < 1 && Math.abs(a.ty - ty) < 1) return;
    if (a === undefined && Math.abs(cur[0] - tx) < 0.01 && Math.abs(cur[1] - ty) < 0.01) return;
    _lumen_smooth_anims[key] = {
        sx: cur[0], sy: cur[1], tx: tx, ty: ty, t0: performance.now(),
        hist: [cur[0], cur[1]], fin: 0, landed: false
    };
    _lumen_smooth_kick();
}

// An instant request: whatever was animating stops where it is.
function _lumen_scroll_instant(key, x, y) {
    delete _lumen_smooth_anims[key];
    _lumen_scroller_set(key, x, y);
}

// Scroll `key` to (x, y) honouring `behavior` (`auto` | `instant` | `smooth`).
function _lumen_scroll_to_pos(key, x, y, behavior) {
    // An element without a scrolling box (no layout box, `overflow: visible`) scrolls nowhere
    // (CSSOM View §Extensions to the Element interface: «return»).
    if (key !== 'p' && !(_lumen_get_scroll_state(key) && _lumen_is_scroll_container(key))) return;
    var cssNid = key === 'p' ? _lumen_u2n(_lumen_get_html_element()) : key;
    if (_lumen_use_smooth(behavior, cssNid)) _lumen_smooth_scroll(key, x, y);
    else _lumen_scroll_instant(key, x, y);
}

function _lumen_fire_scrollend_now(key) {
    _lumen_scrollend_bypass = true;
    try {
        if (key === 'p') _lumen_fire_window_scrollend_event();
        else _lumen_fire_scrollend_on_element(key);
    } finally { _lumen_scrollend_bypass = false; }
}

function _lumen_smooth_tick() {
    _lumen_smooth_raf = false;
    var now = performance.now();
    var more = false;
    var keys = Object.keys(_lumen_smooth_anims);
    for (var i = 0; i < keys.length; i++) {
        var k = keys[i];
        var a = _lumen_smooth_anims[k];
        if (a === undefined) continue;
        var key = k === 'p' ? 'p' : +k;
        if (a.fin > 0) {
            // The final step (or the moment it was cut short) is behind us.
            if (--a.fin === 0) {
                delete _lumen_smooth_anims[k];
                try { _lumen_fire_scrollend_now(key); } catch (e) {}
            } else { more = true; }
            continue;
        }
        var cur = _lumen_scroller_get(key);
        // Only an axis the animation moves is checked: the other one keeps the
        // value it started from, which the shell may have clamped (a raw 1000 on a
        // container that scrolls to 800) without anybody having touched it.
        var mine = true;
        for (var ax = 0; ax < 2 && mine; ax++) {
            if (Math.abs((ax === 0 ? a.tx - a.sx : a.ty - a.sy)) < 0.01) continue;
            var seen = false;
            for (var j = ax; j < a.hist.length; j += 2) {
                if (Math.abs(cur[ax] - a.hist[j]) < 0.5) { seen = true; break; }
            }
            mine = seen;
        }
        if (!mine) { a.fin = _LUMEN_SMOOTH_SETTLE; more = true; continue; }
        var t = (now - a.t0) / _LUMEN_SMOOTH_MS;
        var e = 1;
        if (t < 1) { var u = 1 - Math.max(t, 0); e = 1 - u * u * u; }
        var x = a.sx + (a.tx - a.sx) * e;
        var y = a.sy + (a.ty - a.sy) * e;
        _lumen_scroller_set(key, x, y);
        a.hist.push(x, y);
        if (a.hist.length > 6) a.hist.splice(0, 2);
        if (t >= 1) { a.fin = _LUMEN_SMOOTH_SETTLE; a.landed = true; }
        more = true;
    }
    if (more) _lumen_smooth_kick();
}

// `ScrollToOptions` / `scrollTo(x, y)` argument forms → `{x, y, behavior}`.
// `cur` is the scroller's position, kept for an axis the caller left out;
// `relative` makes the result a delta applied to `cur` (`scrollBy`).
function _lumen_parse_scroll_args(args, cur, relative, fn) {
    var left, top, behavior = 'auto';
    if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
        var o = args[0];
        left = o.left; top = o.top;
        if (o.behavior !== undefined) {
            behavior = String(o.behavior);
            if (behavior !== 'auto' && behavior !== 'instant' && behavior !== 'smooth') {
                throw new TypeError("Failed to execute '" + fn + "': The provided value '" + behavior + "' is not a valid enum value of type ScrollBehavior.");
            }
        }
    } else if (args.length === 0) {
        left = undefined; top = undefined;
    } else {
        left = args[0]; top = args[1] === undefined ? 0 : args[1];
    }
    function axis(v, c) {
        if (v === undefined) return c;
        v = +v;
        if (!isFinite(v)) v = 0;
        return relative ? c + v : v;
    }
    return { x: axis(left, cur[0]), y: axis(top, cur[1]), behavior: behavior };
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

// DOM §2.2's fixed legacy interface-name table for `document.createEvent()`
// (BUG-590), keyed lower-case since the lookup is case-insensitive. Looked up
// lazily (called only once a script actually invokes `createEvent`), so it
// can name constructors — `UIEvent`, `MouseEvent`, `BeforeUnloadEvent`, … —
// declared later in files concatenated after this one; by the time any
// script runs, the whole shim has already loaded into one scope.
function _lumen_legacy_event_ctor(name) {
    switch (name) {
        case 'event': case 'events': case 'htmlevents': return Event;
        case 'customevent': return CustomEvent;
        case 'uievent': case 'uievents': return UIEvent;
        case 'mouseevent': case 'mouseevents': return MouseEvent;
        case 'keyboardevent': return KeyboardEvent;
        case 'focusevent': return FocusEvent;
        case 'hashchangeevent': return HashChangeEvent;
        case 'storageevent': return StorageEvent;
        case 'messageevent': return MessageEvent;
        case 'dragevent': return DragEvent;
        case 'compositionevent': return CompositionEvent;
        case 'textevent': return TextEvent;
        case 'beforeunloadevent': return BeforeUnloadEvent;
        default: return null;
    }
}

