
// ── PerformanceObserver (Performance Timeline L2 §5–6) ───────────────────────
// observe({entryTypes}) or observe({type, buffered}) per §6.2.2.
// disconnect() → stops observing. Callback: fn(list, observer).
var _perf_observers = [];

// Single source of truth for supportedEntryTypes AND observe()'s admission
// check (BUG-354): only types an entry constructor actually produces belong
// here, so the two cannot drift apart again. 'element'/'event'/'first-input'/
// 'longtask'/'soft-navigation' are intentionally excluded — no PerformanceEntry
// of those types is ever produced on the live document (soft-navigation has a
// PerformanceSoftNavigationEntry class but nothing calls its delivery hook
// outside unit tests).
var _PERF_SUPPORTED_ENTRY_TYPES = ['largest-contentful-paint', 'layout-shift',
    'mark', 'measure', 'navigation', 'paint', 'resource'];

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

// Called by the shell after first paint / first contentful paint.
// name = 'first-paint' | 'first-contentful-paint', start_ms = DOMHighResTimeStamp.
function _lumen_deliver_paint_entry(name, start_ms) {
    var entry = { entryType: 'paint', name: String(name), startTime: start_ms, duration: 0 };
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

// Called by the shell when layout shift detected (CLS).
// value = fractional shift distance (0.0..1.0+); session_id for grouping.
// had_input = whether user input occurred recently (affects grouping).
function _lumen_deliver_layout_shift(value, session_id, had_input) {
    var entry = {
        entryType: 'layout-shift',
        name: 'layout-shift',
        startTime: performance.now(),
        duration: 0,
        value: value,
        hadRecentInput: !!had_input,
        sources: [],
    };
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

// ── requestIdleCallback / cancelIdleCallback (HTML LS §8.6) ──────────────────
// Stub: fires via setTimeout(~50ms) with a synthetic IdleDeadline that always
// reports 50ms remaining — Lumen is single-process, so there is no real idle
// detection. The timeout option is honoured as the scheduling delay.
var _idle_cbs    = {};
var _idle_seq    = 1;

function requestIdleCallback(cb, opts) {
    if (typeof cb !== 'function') throw new TypeError('requestIdleCallback: argument must be a function');
    var delay = (opts && typeof opts.timeout === 'number' && opts.timeout > 0) ? Math.min(opts.timeout, 50) : 50;
    var id = _idle_seq++;
    _idle_cbs[id] = cb;
    setTimeout(function() {
        var fn = _idle_cbs[id];
        if (!fn) return;
        delete _idle_cbs[id];
        var deadline = { timeRemaining: function() { return 50; }, didTimeout: false };
        try { fn(deadline); } catch(e) { _lumen_report_exception(e); }
    }, delay);
    return id;
}

function cancelIdleCallback(id) {
    delete _idle_cbs[id | 0];
}
