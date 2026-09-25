
// ── PerformanceObserver (Performance Timeline L2 §5–6) ───────────────────────
// observe({entryTypes}) or observe({type, buffered}) per §6.2.2.
// disconnect() → stops observing. Callback: fn(list, observer).
var _perf_observers = [];

// Single source of truth for supportedEntryTypes AND observe()'s admission
// check (BUG-354): only types an entry constructor actually produces belong
// here, so the two cannot drift apart again. 'element'/'event'/'first-input'/
// 'soft-navigation' are intentionally excluded — no PerformanceEntry of those
// types is ever produced on the live document (soft-navigation has a
// PerformanceSoftNavigationEntry class but nothing calls its delivery hook
// outside unit tests). 'taskattribution' (LONGTASK-1) is excluded on purpose
// too — spec-visible only via `PerformanceLongTaskTiming.attribution`, never
// independently observable (`longtask-timing/supported-longtask-types.window.js`).
// 'longtask'/'long-animation-frame' (LONGTASK-1): the shell times every
// `eval_js` dispatch and every relayout frame past the 50ms threshold —
// `crates/shell/src/persistent_js.rs`/`relayout.rs`.
var _PERF_SUPPORTED_ENTRY_TYPES = ['largest-contentful-paint', 'layout-shift',
    'long-animation-frame', 'longtask', 'mark', 'measure', 'navigation',
    'paint', 'resource'];

function PerformanceObserver(callback) {
    if (typeof callback !== 'function') throw new TypeError('PerformanceObserver: callback must be a function');
    this._cb      = callback;
    this._types   = [];
    this._buffered = false;
    // Performance Timeline L2 §6.2 «requires dropped entries»: raised by every
    // observe() call, lowered by the first callback that reports the count.
    this._requiresDropped = false;
}
// Performance Timeline L2 §6.2.2: supportedEntryTypes static accessor.
Object.defineProperty(PerformanceObserver, 'supportedEntryTypes', {
    get: function() {
        return _PERF_SUPPORTED_ENTRY_TYPES.slice();
    },
    configurable: true,
});
PerformanceObserver.prototype.observe = function(opts) {
    var types;
    var buffered;
    if (opts && typeof opts.type === 'string') {
        // §6.2.2 single-type form: observe({type, buffered})
        // Per spec step 6: an unsupported single type aborts observe() entirely.
        if (_PERF_SUPPORTED_ENTRY_TYPES.indexOf(opts.type) === -1) {
            if (typeof console !== 'undefined' && console.warn) {
                console.warn('PerformanceObserver: unsupported entryType ' + opts.type);
            }
            return;
        }
        types   = [opts.type];
        buffered = !!(opts.buffered);
    } else {
        // §6.2.2 multi-type form: observe({entryTypes[, buffered]})
        // Spec disallows buffered here, but we accept it for compatibility.
        // Unsupported types are dropped individually, not fatal to the call.
        var requested = (opts && Array.isArray(opts.entryTypes)) ? opts.entryTypes : [];
        types = [];
        for (var r = 0; r < requested.length; r++) {
            if (_PERF_SUPPORTED_ENTRY_TYPES.indexOf(requested[r]) === -1) {
                if (typeof console !== 'undefined' && console.warn) {
                    console.warn('PerformanceObserver: unsupported entryType ' + requested[r]);
                }
                continue;
            }
            types.push(requested[r]);
        }
        buffered = !!(opts && opts.buffered);
    }
    // Merge into existing subscribed types so repeated observe() calls accumulate.
    for (var i = 0; i < types.length; i++) {
        if (this._types.indexOf(types[i]) === -1) this._types.push(types[i]);
    }
    if (buffered) this._buffered = true;
    // §6.2 step «set this's requires dropped entries to true» — every observe()
    // call, not only a buffered one: `droppedentriescount.any.js` re-arms an
    // already-delivered observer with a second observe() and asserts the count
    // is reported again.
    this._requiresDropped = true;
    // De-duplicate in global list.
    var idx = _perf_observers.indexOf(this);
    if (idx === -1) _perf_observers.push(this);
    // If buffered: deliver already-existing matching entries immediately.
    // Delivered even when the buffer holds nothing, provided this observer has
    // a dropped count to report — an observer armed on `resource` after the
    // buffer overflowed learns the count and nothing else, which is exactly the
    // «Dropped entries counted even if observer was not registered at the time»
    // case of the WPT file above.
    if (buffered && types.length > 0) {
        var buf = _perf_entries.filter(function(e) {
            return types.indexOf(e.entryType) !== -1;
        });
        if (buf.length > 0 || _perf_dropped_count_for(this) > 0) {
            _perf_deliver_to_observer(this, buf);
        }
    }
};
PerformanceObserver.prototype.disconnect = function() {
    var idx = _perf_observers.indexOf(this);
    if (idx !== -1) _perf_observers.splice(idx, 1);
};
PerformanceObserver.prototype.takeRecords = function() {
    var entries = [];
    for (var i = 0; i < this._types.length; i++) {
        var type = this._types[i];
        var matching = _perf_entries.filter(function(e) { return e.entryType === type; });
        entries = entries.concat(matching);
    }
    return entries;
};

// Performance Timeline L2 §6.2.1: the number of entries dropped for the types
// this observer subscribes to. `resource` is the only bounded buffer in the
// engine, so it is the only type that can contribute — a type with no limit
// never drops anything, and reporting a non-zero count for it would be a lie
// the page cannot check.
function _perf_dropped_count_for(obs) {
    var n = 0;
    if (obs._types.indexOf('resource') !== -1) n += _perf_rt_dropped;
    return n;
}

// Deliver a batch of entries to a single observer (wraps in EntryList).
//
// The callback takes THREE arguments (§6.2.1 `PerformanceObserverCallback`):
// the entry list, the observer, and a `PerformanceObserverCallbackOptions`
// whose `droppedEntriesCount` is present only while the observer's «requires
// dropped entries» flag is up. Delivering two arguments made every read of
// `options.droppedEntriesCount` throw a TypeError inside the callback — which
// the surrounding catch then swallowed (BUG-840).
function _perf_deliver_to_observer(obs, entries) {
    var list = {
        getEntries:        function() { return entries.slice(); },
        getEntriesByName:  function(n, t) { return entries.filter(function(e) { return e.name === n && (!t || e.entryType === t); }); },
        getEntriesByType:  function(t) { return entries.filter(function(e) { return e.entryType === t; }); },
    };
    var options = {};
    if (obs._requiresDropped) {
        options.droppedEntriesCount = _perf_dropped_count_for(obs);
        obs._requiresDropped = false;
    }
    try { obs._cb(list, obs, options); } catch(e) { _lumen_report_exception(e); }
}

// Called internally when new entries are created (mark/measure/paint).
function _perf_observer_notify(entries) {
    for (var i = 0; i < _perf_observers.length; i++) {
        var obs = _perf_observers[i];
        var matching = entries.filter(function(e) { return obs._types.indexOf(e.entryType) !== -1; });
        if (matching.length > 0) _perf_deliver_to_observer(obs, matching);
    }
}

// Paint Timing `interface PerformancePaintTiming : PerformanceEntry` —
// the interface object every paint-timing WPT feature-detects first
// (`assert_implements(window.PerformancePaintTiming)`, BUG-645). The IDL
// declares no constructor, so script-side `new` throws; the shell's entries
// are built off the prototype instead. Fields stay own properties, as on every
// other entry type in this shim; `toJSON` is the WebIDL `[Default]` one of
// PerformanceEntry. PerformanceEntry itself is not exposed: mark/measure/
// resource entries are still plain objects, and a global that `instanceof`
// answered false for would lie about them.
function PerformancePaintTiming() { throw new TypeError('Illegal constructor'); }
PerformancePaintTiming.prototype.toJSON = function() {
    return { name: this.name, entryType: this.entryType, startTime: this.startTime,
             duration: this.duration };
};

// Called by the shell after first paint / first contentful paint.
// name = 'first-paint' | 'first-contentful-paint', start_ms = DOMHighResTimeStamp.
function _lumen_deliver_paint_entry(name, start_ms) {
    var entry = Object.create(PerformancePaintTiming.prototype);
    entry.entryType = 'paint';
    entry.name = String(name);
    entry.startTime = start_ms;
    entry.duration = 0;
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Called by the shell after rendering a large content element (LCP).
// element_id = NID of the element; size = area in pixels (>500px²).
// start_ms = DOMHighResTimeStamp; render_time_ms = when rendering completed.
function _lumen_deliver_lcp_entry(element_id, size, start_ms, render_time_ms) {
    var entry = {
        entryType: 'largest-contentful-paint',
        name: 'largest-contentful-paint',
        startTime: start_ms,
        duration: render_time_ms - start_ms,
        size: size,
        element: element_id >= 0 ? _lumen_make_element(element_id) : null,
        url: '',
        id: '',
        activationStart: 0,
    };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Layout Instability L1 §4.1/§4.2: the record classes behind a `layout-shift`
// entry. Exposed on `window` so a page's feature-detect (`typeof
// window.LayoutShift`) sees a real constructor instead of the plain object a
// bare `{...}` literal would produce — `PerformanceEntry`-shaped fields are
// own properties for the same reason every other entry type here uses a
// plain object rather than prototype inheritance (BUG-354's "ctor exists but
// nothing produces it" trap runs the other way for `LayoutShift`: the ctor
// now exists AND something produces it).
function LayoutShiftAttribution(node, previousRect, currentRect) {
    this.node = node || null;
    this.previousRect = previousRect;
    this.currentRect = currentRect;
}
function LayoutShift(init) {
    this.entryType = 'layout-shift';
    this.name = 'layout-shift';
    this.startTime = init.startTime;
    this.duration = 0;
    this.value = init.value;
    this.hadRecentInput = init.hadRecentInput;
    this.lastInputTime = init.lastInputTime || 0;
    this.sources = init.sources;
}

// Called by the shell when layout shift detected (CLS).
// value = fractional shift distance (0.0..1.0+); sources = the shifted nodes
// behind the score, largest impact first (up to five, §4.2), each
// {nid, prev: [x,y,w,h], curr: [x,y,w,h]}; had_input = whether user input
// occurred recently (affects grouping).
function _lumen_deliver_layout_shift(value, sources, had_input) {
    var sources_out = (sources || []).map(function(s) {
        var prev = s.prev ? new DOMRectReadOnly(s.prev[0], s.prev[1], s.prev[2], s.prev[3]) : null;
        var curr = s.curr ? new DOMRectReadOnly(s.curr[0], s.curr[1], s.curr[2], s.curr[3]) : null;
        return new LayoutShiftAttribution(_lumen_make_element(s.nid), prev, curr);
    });
    var entry = new LayoutShift({
        startTime: performance.now(),
        value: value,
        hadRecentInput: !!had_input,
        sources: sources_out,
    });
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Called when a resource fetch completes — from the shim itself for everything
// the page starts (`fetch()`, XHR, `<script src>`, `<link>`), and from the
// shell through `_lumen_deliver_resource_timings` for the subresources the
// engine fetches on the document's behalf (images, stylesheets, fonts).
//
// W3C Resource Timing L2 §4. The engine has no per-phase network breakdown, so
// every connection milestone collapses onto fetchStart and only the two ends of
// the request are real; the sizes and the status are real when the caller knows
// them. `detail` is an optional object with `status`, `encodedBodySize`,
// `decodedBodySize`, `contentType`, `nextHopProtocol`, `deliveryType`.
// initiator = 'script'|'link'|'img'|'css'|'fetch'|'xmlhttprequest'|'other'.
function _lumen_record_resource_timing(url, initiator, start_ms, duration_ms, detail) {
    var s = Number(start_ms);
    var d = Number(duration_ms);
    if (!isFinite(s) || s < 0) s = 0;
    if (!isFinite(d) || d < 0) d = 0;
    var det = detail || {};
    var decoded = Number(det.decodedBodySize) || 0;
    var encoded = (det.encodedBodySize === undefined || det.encodedBodySize === null)
        ? decoded : (Number(det.encodedBodySize) || 0);
    // §4.3 `transferSize`: the encoded body plus the response's own overhead,
    // for which the spec names 300 bytes as the fixed approximation. A response
    // served from cache transferred nothing.
    var delivery = det.deliveryType ? String(det.deliveryType) : '';
    var transfer = (delivery === 'cache') ? 0 : encoded + 300;
    var entry = {
        entryType: 'resource',
        name: String(url),
        startTime: s,
        duration: d,
        initiatorType: String(initiator),
        deliveryType: delivery,
        nextHopProtocol: det.nextHopProtocol ? String(det.nextHopProtocol) : '',
        workerStart: 0,
        redirectStart: 0,
        redirectEnd: 0,
        fetchStart: s,
        domainLookupStart: s,
        domainLookupEnd: s,
        connectStart: s,
        connectEnd: s,
        secureConnectionStart: 0,
        requestStart: s,
        firstInterimResponseStart: 0,
        responseStart: s,
        responseEnd: s + d,
        transferSize: transfer,
        encodedBodySize: encoded,
        decodedBodySize: decoded,
        responseStatus: Number(det.status) || 0,
        renderBlockingStatus: 'non-blocking',
        contentType: det.contentType ? String(det.contentType) : '',
    };
    // §4.2 `[Default] object toJSON()` — the whole attribute set, which is what
    // `JSON.stringify(entry)` must produce; an own-property spread would also
    // carry toJSON itself.
    var _keys = Object.keys(entry);
    Object.defineProperty(entry, 'toJSON', {
        value: function() {
            var out = {};
            for (var i = 0; i < _keys.length; i++) { out[_keys[i]] = entry[_keys[i]]; }
            return out;
        },
        writable: true, configurable: true, enumerable: false,
    });
    // The buffer and the observer stream are separate sinks: an entry the
    // buffer refuses is still delivered to every interested observer.
    _perf_rt_add(entry);
    _perf_observer_notify([entry]);
}

// Called by the shell once per event-loop step with the subresource loads that
// completed since the last call (images, stylesheets, fonts, parser scripts) —
// those are fetched by the engine, on threads that have no JS context, so they
// cannot record themselves the way the shim-side fetches do.
//
// `rows` is a JSON array of
// {url, initiatorType, startMs, durationMs, status, encodedBodySize,
//  decodedBodySize, contentType, nextHopProtocol, deliveryType}, where the two
// timestamps are unix-epoch milliseconds — the same clock `_lumen_now_ms`
// reads, so they convert to DOMHighResTimeStamps by subtracting the time
// origin. A load that finished before this document's JS runtime existed lands
// before the origin; it is clamped to 0 rather than reported negative, since
// `startTime` is defined to be a non-negative offset from the origin.
function _lumen_deliver_resource_timings(rows_json) {
    var rows;
    try { rows = JSON.parse(String(rows_json)); } catch (e) { return; }
    if (!rows || !rows.length) return;
    for (var i = 0; i < rows.length; i++) {
        var r = rows[i];
        if (!r || !r.url) continue;
        var start = Number(r.startMs) - _perf_origin_ms;
        if (!isFinite(start) || start < 0) start = 0;
        _lumen_record_resource_timing(r.url, r.initiatorType || 'other', start,
            Number(r.durationMs) || 0, r);
    }
}

// Generic entry delivery — called by Rust shell for any PerformanceEntry type.
// entry_type: W3C entryType string (e.g. 'navigation', 'resource').
// detail_json: optional JSON string; its properties are merged into the entry.
// The entry always lands in performance's entry buffer regardless of entry_type
// (getEntriesByType() sees it), but PerformanceObserver.observe() only forwards
// types listed in _PERF_SUPPORTED_ENTRY_TYPES (BUG-354) — delivering a type
// outside that list populates the buffer silently without notifying observers.
function _lumen_deliver_perf_entry(entry_type, name, start_ms, duration_ms, detail_json) {
    var entry = {
        entryType: String(entry_type),
        name: String(name),
        startTime: Number(start_ms),
        duration: Number(duration_ms),
    };
    if (detail_json) {
        try {
            var extra = JSON.parse(String(detail_json));
            for (var k in extra) {
                if (Object.prototype.hasOwnProperty.call(extra, k)) entry[k] = extra[k];
            }
        } catch(e) {}
    }
    // `performance.timing`/`performance.navigation` (BUG-767) read this same
    // entry rather than a second channel — see performance_shim.js.
    if (entry.entryType === 'navigation') _perf_last_navigation_entry = entry;
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// ── scheduler (Prioritized Task Scheduling API — W3C §2) ─────────────────────
// scheduler.postTask(fn, {priority?, delay?}) → Promise
// Priorities: 'user-blocking' (microtask-like), 'user-visible' (default,
// setTimeout 0), 'background' (setTimeout 0). All three converge to async
// execution; priority differentiation is Phase 2 (requires Rust task sources).
var scheduler = {
    postTask: function(fn, opts) {
        if (typeof fn !== 'function') return Promise.reject(new TypeError('scheduler.postTask: argument must be a function'));
        var delay = (opts && typeof opts.delay === 'number' && opts.delay > 0) ? opts.delay : 0;
        return new Promise(function(resolve, reject) {
            setTimeout(function() {
                try { resolve(fn()); } catch(e) { reject(e); }
            }, delay);
        });
    },
    yield: function() {
        return new Promise(function(resolve) { setTimeout(resolve, 0); });
    },
};

// ── requestIdleCallback / cancelIdleCallback (W3C Cooperative Scheduling) ───
// BUG-660: this used to be a fixed `setTimeout(~50ms)` handing the callback a
// plain object literal — no `IdleDeadline` interface, `timeRemaining()` the
// constant 50, `didTimeout` always false. Now:
//
// * An idle period is started by `_lumen_tick_timers` (web_api_shim_mid_b.js)
//   through an «idle marker» entry in `_lumen_timers`, which it runs after the
//   tick's ordinary tasks. The period starts only when the event loop is idle:
//   no ordinary timer is due and no task longer than a frame ended within the
//   last frame (`_lumen_idle_busy_until`) — the «remain responsive» latitude
//   the spec leaves to the UA. Otherwise the marker is re-armed.
// * The deadline is live (§«deadline» is computed on every `timeRemaining()`
//   call): the earliest of period start + 50 ms, the earliest pending ordinary
//   timer, and — while rAF callbacks are queued — the next frame (start +
//   1000/60). A `setTimeout`/`requestAnimationFrame` made from inside the
//   callback therefore shortens it immediately.
// * `options.timeout` arms its own timer; if that fires first the callback runs
//   with `didTimeout === true` and a deadline of «now» (`timeRemaining()` 0).
var _LUMEN_IDLE_MAX_MS  = 50;
var _LUMEN_IDLE_FRAME_MS = 1000 / 60;
var _idle_list          = [];     // [{ id, fn, timeoutTimer }] in request order
var _idle_seq           = 1;
var _idle_marker_armed  = false;
var _lumen_idle_busy_until = -Infinity;
var _idle_deadline_state = new WeakMap();

// A function *expression* under an internal name, published below as a
// non-enumerable global (WebIDL §3.7.1) — a top-level declaration would land
// on the global object enumerable and non-configurable (idlharness).
var _lumen_idle_deadline_iface = function IdleDeadline() { throw new TypeError('Illegal constructor'); };
Object.defineProperty(_lumen_idle_deadline_iface, 'prototype', { writable: false });
Object.defineProperty(globalThis, 'IdleDeadline', {
    value: _lumen_idle_deadline_iface, writable: true, enumerable: false, configurable: true,
});
Object.defineProperty(IdleDeadline.prototype, 'timeRemaining', {
    value: function timeRemaining() {
        var st = _idle_deadline_state.get(this);
        if (!st) throw new TypeError('Illegal invocation');
        var left = st.getDeadline() - _lumen_now_ms();
        return left > 0 ? left : 0;
    },
    writable: true, enumerable: true, configurable: true,
});
var _lumen_idle_did_timeout_get = function() {
    var st = _idle_deadline_state.get(this);
    if (!st) throw new TypeError('Illegal invocation');
    return st.didTimeout;
};
Object.defineProperty(_lumen_idle_did_timeout_get, 'name', { value: 'get didTimeout' });
Object.defineProperty(IdleDeadline.prototype, 'didTimeout', {
    get: _lumen_idle_did_timeout_get, enumerable: true, configurable: true,
});
Object.defineProperty(IdleDeadline.prototype, Symbol.toStringTag, {
    value: 'IdleDeadline', configurable: true,
});

function _lumen_make_idle_deadline(getDeadline, didTimeout) {
    var d = Object.create(IdleDeadline.prototype);
    _idle_deadline_state.set(d, { getDeadline: getDeadline, didTimeout: didTimeout });
    return d;
}

// Earliest deadline among ordinary timers — the idle machinery's own entries
// (markers and rIC timeouts) are not work the period has to yield to.
function _lumen_idle_next_timer() {
    var next = Infinity;
    for (var i = 0; i < _lumen_timers.length; i++) {
        var t = _lumen_timers[i];
        if (t.idleMarker || t.idleTimeout) continue;
        if (t.deadline < next) next = t.deadline;
    }
    return next;
}

function _lumen_idle_arm(deadline) {
    if (_idle_marker_armed) return;
    _idle_marker_armed = true;
    _lumen_timers.push({ id: _lumen_timer_seq++, fn: _lumen_idle_period, deadline: deadline,
                         interval: null, nesting: 0, idleMarker: true });
    _lumen_request_wakeup(deadline);
}

function _lumen_idle_invoke(fn, deadline) {
    var t0 = _lumen_now_ms();
    try { fn(deadline); } catch (e) { _lumen_report_exception(e); }
    var t1 = _lumen_now_ms();
    if (t1 - t0 > _LUMEN_IDLE_FRAME_MS) _lumen_idle_busy_until = t1 + _LUMEN_IDLE_FRAME_MS;
}

// Run by `_lumen_tick_timers` after the tick's ordinary tasks.
function _lumen_idle_period() {
    _idle_marker_armed = false;
    if (_idle_list.length === 0) return;
    var start = _lumen_now_ms();
    if (start < _lumen_idle_busy_until) { _lumen_idle_arm(_lumen_idle_busy_until); return; }
    if (_lumen_idle_next_timer() <= start) { _lumen_idle_arm(start); return; }
    var getDeadline = function() {
        var d = start + _LUMEN_IDLE_MAX_MS;
        var t = _lumen_idle_next_timer();
        if (t < d) d = t;
        if (_lumen_raf_callbacks.length > 0 && start + _LUMEN_IDLE_FRAME_MS < d) d = start + _LUMEN_IDLE_FRAME_MS;
        return d;
    };
    // Only the callbacks requested before the period began run in it.
    var runnable = _idle_list.slice(0);
    for (var i = 0; i < runnable.length; i++) {
        if (getDeadline() <= _lumen_now_ms()) break;
        var rec = runnable[i];
        var at = _idle_list.indexOf(rec);
        if (at < 0) continue;              // cancelled by an earlier callback
        _idle_list.splice(at, 1);
        if (rec.timeoutTimer) clearTimeout(rec.timeoutTimer);
        _lumen_idle_invoke(rec.fn, _lumen_make_idle_deadline(getDeadline, false));
    }
    if (_idle_list.length > 0) _lumen_idle_arm(Math.max(_lumen_now_ms(), _lumen_idle_busy_until));
}

// WebIDL operations on `Window` reject a foreign `this` (a sloppy-mode
// function sees `globalThis` for a bare call, so only a real receiver passes).
function _lumen_idle_check_this(self, name) {
    if (self !== globalThis && self !== undefined) throw new TypeError(name + ': Illegal invocation');
}

// `opts` is read from `arguments`: WebIDL `.length` counts only the required
// callback argument.
function requestIdleCallback(cb) {
    _lumen_idle_check_this(this, 'requestIdleCallback');
    var opts = arguments[1];
    if (typeof cb !== 'function') throw new TypeError('requestIdleCallback: argument must be a function');
    var rec = { id: _idle_seq++, fn: cb, timeoutTimer: 0 };
    var timeout = (opts && opts.timeout !== undefined) ? (Number(opts.timeout) >>> 0) : 0;
    if (timeout > 0) {
        var deadline = _lumen_now_ms() + timeout;
        rec.timeoutTimer = _lumen_timer_seq++;
        _lumen_timers.push({ id: rec.timeoutTimer, deadline: deadline, interval: null, nesting: 0,
                             idleTimeout: true, fn: function() {
            var at = _idle_list.indexOf(rec);
            if (at < 0) return;
            _idle_list.splice(at, 1);
            _lumen_idle_invoke(rec.fn, _lumen_make_idle_deadline(_lumen_now_ms, true));
        } });
        _lumen_request_wakeup(deadline);
    }
    _idle_list.push(rec);
    _lumen_idle_arm(_lumen_now_ms());
    return rec.id;
}

function cancelIdleCallback(id) {
    _lumen_idle_check_this(this, 'cancelIdleCallback');
    if (arguments.length < 1) throw new TypeError('cancelIdleCallback: 1 argument required, but only 0 present');
    var handle = Number(id) >>> 0;
    for (var i = 0; i < _idle_list.length; i++) {
        if (_idle_list[i].id === handle) {
            if (_idle_list[i].timeoutTimer) clearTimeout(_idle_list[i].timeoutTimer);
            _idle_list.splice(i, 1);
            return;
        }
    }
}
