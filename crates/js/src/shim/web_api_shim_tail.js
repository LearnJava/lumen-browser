
// ── PerformanceObserver (Performance Timeline L2 §4–5) ───────────────────────
// The spec's model, not a convenience filter (BUG-648):
// * each observer has an *observer type* ('undefined' → 'single'/'multiple'),
//   an *options list* and an *observer buffer* of entries not yet delivered;
// * `_perf_observer_notify` is §5.1 «queue a PerformanceEntry»: it only
//   appends to the buffers of interested observers and queues ONE
//   PerformanceObserver task (§5.3) per global — the callback never runs inside
//   `mark()`/`measure()`/`observe()`, so `disconnect()` right after `mark()`
//   still cancels the delivery and a page may assign the function its callback
//   calls after `observe({buffered: true})` (the web-vitals shape);
// * `takeRecords()` drains the observer buffer, which the task drains too, so
//   an entry reaches a page exactly once.
var _perf_observers = [];
// §2 «performance observer task queued flag».
var _perf_po_task_queued = false;

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

function _perf_po_warn(msg) {
    if (typeof console !== 'undefined' && console.warn) console.warn('PerformanceObserver: ' + msg);
}

// Function *expressions* under internal names, published below as
// non-enumerable globals (WebIDL §3.7.1) — a top-level declaration would land
// on the global object enumerable and non-configurable (idlharness), the same
// reason as `IdleDeadline` further down.
var _perf_po_iface = function PerformanceObserver(callback) {
    if (!new.target) throw new TypeError("Failed to construct 'PerformanceObserver': Please use the 'new' operator");
    if (typeof callback !== 'function') throw new TypeError('PerformanceObserver: callback must be a function');
    this._cb = callback;
    // §4 observer type / options list / observer buffer.
    this._observerType = 'undefined';
    this._options = [];
    this._buffer = [];
    // §4 «requires dropped entries»: raised by every observe() call, lowered
    // by the first callback that reports the count.
    this._requiresDropped = false;
    // A buffered observe() whose only news is a dropped count (see observe()).
    this._forceDeliver = false;
};
Object.defineProperty(globalThis, 'PerformanceObserver', {
    value: _perf_po_iface, writable: true, enumerable: false, configurable: true,
});
// Performance Timeline L2 §4.5: `[SameObject] static readonly attribute
// FrozenArray<DOMString> supportedEntryTypes` — one frozen array, the same
// object on every read (`supportedEntryTypes.any.js` «caches result»).
var _PERF_SUPPORTED_ENTRY_TYPES_FROZEN = Object.freeze(_PERF_SUPPORTED_ENTRY_TYPES.slice());
var _perf_po_supported_get = function() { return _PERF_SUPPORTED_ENTRY_TYPES_FROZEN; };
Object.defineProperty(_perf_po_supported_get, 'name', { value: 'get supportedEntryTypes' });
Object.defineProperty(PerformanceObserver, 'supportedEntryTypes', {
    get: _perf_po_supported_get, enumerable: true, configurable: true,
});

// The entry types an observer's options list subscribes it to.
function _perf_po_types(obs) {
    var out = [];
    for (var i = 0; i < obs._options.length; i++) {
        var o = obs._options[i];
        var list = o.entryTypes ? o.entryTypes : [o.type];
        for (var j = 0; j < list.length; j++) {
            if (out.indexOf(list[j]) === -1) out.push(list[j]);
        }
    }
    return out;
}

// WebIDL conversion of the `PerformanceObserverInit` dictionary: members in
// lexicographic order, `entryTypes` as `sequence<DOMString>` (a string is not
// an object and so not a sequence — `observe({entryTypes: 'mark'})` is a
// TypeError, per `po-observe.any.js`).
function _perf_po_convert_init(opts) {
    if (opts === undefined || opts === null) return {};
    if (typeof opts !== 'object' && typeof opts !== 'function') {
        throw new TypeError("Failed to execute 'observe' on 'PerformanceObserver': The provided value is not of type 'PerformanceObserverInit'.");
    }
    var out = {};
    var b = opts.buffered;
    if (b !== undefined) out.buffered = !!b;
    var et = opts.entryTypes;
    if (et !== undefined) {
        if ((typeof et !== 'object' && typeof et !== 'function') || et === null
            || typeof et[Symbol.iterator] !== 'function') {
            throw new TypeError("Failed to execute 'observe' on 'PerformanceObserver': The provided value cannot be converted to a sequence.");
        }
        var seq = [];
        for (var it = et[Symbol.iterator](), step = it.next(); !step.done; step = it.next()) {
            seq.push(String(step.value));
        }
        out.entryTypes = seq;
    }
    var t = opts.type;
    if (t !== undefined) out.type = String(t);
    return out;
}

// §4.2 observe().
// Brand check shared by the three operations (WebIDL «this is not a
// platform object implementing the interface» → TypeError).
function _perf_po_check(obj) {
    if (!(obj instanceof _perf_po_iface)) throw new TypeError('Illegal invocation');
}
PerformanceObserver.prototype.observe = function observe() {
    _perf_po_check(this);
    var opts = _perf_po_convert_init(arguments[0]);
    var hasEntryTypes = opts.entryTypes !== undefined;
    var hasType = opts.type !== undefined;
    if (!hasEntryTypes && !hasType) {
        throw new TypeError("Failed to execute 'observe' on 'PerformanceObserver': An observe() call must include either entryTypes or type arguments.");
    }
    // The spec says «entryTypes and any other member» — but `buffered` next to
    // `entryTypes` is ignored with a warning in every engine, and WPT
    // (`buffered-flag-with-entryTypes-observer.tentative.any.js`) asserts
    // exactly that; only `type` together with `entryTypes` throws.
    if (hasEntryTypes && hasType) {
        throw new TypeError("Failed to execute 'observe' on 'PerformanceObserver': An observe() call must not include both entryTypes and type arguments.");
    }
    if (this._observerType === 'undefined') {
        this._observerType = hasEntryTypes ? 'multiple' : 'single';
    }
    if (this._observerType === 'single' && hasEntryTypes) {
        throw new DOMException("Failed to execute 'observe' on 'PerformanceObserver': This observer has performed observe({type:...}, therefore it cannot perform observe({entryTypes:...})", 'InvalidModificationError');
    }
    if (this._observerType === 'multiple' && hasType) {
        throw new DOMException("Failed to execute 'observe' on 'PerformanceObserver': This PerformanceObserver has performed observe({entryTypes:...}, therefore it cannot perform observe({type:...})", 'InvalidModificationError');
    }
    // §4.2 «set this's requires dropped entries to true» — every observe()
    // call, not only a buffered one: `droppedentriescount.any.js` re-arms an
    // already-delivered observer with a second observe() and asserts the count
    // is reported again.
    this._requiresDropped = true;
    var registered = _perf_observers.indexOf(this) !== -1;
    if (this._observerType === 'multiple') {
        var types = [];
        for (var r = 0; r < opts.entryTypes.length; r++) {
            var et = opts.entryTypes[r];
            if (_PERF_SUPPORTED_ENTRY_TYPES.indexOf(et) === -1) {
                _perf_po_warn('unsupported entryType ' + et);
                continue;
            }
            if (types.indexOf(et) === -1) types.push(et);
        }
        if (opts.buffered) _perf_po_warn('the buffered flag is ignored together with entryTypes');
        if (types.length === 0) {
            _perf_po_warn('no supported entryTypes, observe() aborted');
            return;
        }
        // Repeated calls replace, never stack (§4.2 note).
        this._options = [{ entryTypes: types }];
        if (!registered) _perf_observers.push(this);
        return;
    }
    // Single-type form: unsupported type aborts the call.
    if (_PERF_SUPPORTED_ENTRY_TYPES.indexOf(opts.type) === -1) {
        _perf_po_warn('unsupported entryType ' + opts.type);
        return;
    }
    var item = { type: opts.type, buffered: !!opts.buffered };
    var replaced = false;
    for (var i = 0; i < this._options.length; i++) {
        if (this._options[i].type === item.type) { this._options[i] = item; replaced = true; break; }
    }
    if (!replaced) this._options.push(item);
    if (!registered) _perf_observers.push(this);
    if (item.buffered) {
        for (var k = 0; k < _perf_entries.length; k++) {
            if (_perf_entries[k].entryType === item.type) this._buffer.push(_perf_entries[k]);
        }
        // An observer armed on `resource` after the buffer overflowed learns
        // the count even with nothing buffered — the «Dropped entries counted
        // even if observer was not registered at the time» case of
        // `droppedentriescount.any.js`.
        if (_perf_dropped_count_for(this) > 0) this._forceDeliver = true;
        _perf_queue_observer_task();
    }
};
// §4.4 disconnect(). The observer type survives: re-observing in the other
// form is still an InvalidModificationError.
PerformanceObserver.prototype.disconnect = function disconnect() {
    _perf_po_check(this);
    var idx = _perf_observers.indexOf(this);
    if (idx !== -1) _perf_observers.splice(idx, 1);
    this._buffer = [];
    this._options = [];
    this._forceDeliver = false;
};
// §4.3 takeRecords(): a copy of the observer buffer, which is then emptied.
PerformanceObserver.prototype.takeRecords = function takeRecords() {
    _perf_po_check(this);
    var entries = this._buffer;
    this._buffer = [];
    return entries;
};

// Performance Timeline L2 §4 «dropped entries count» summed over the types
// this observer subscribes to. `resource` is the only bounded buffer in the
// engine, so it is the only type that can contribute — a type with no limit
// never drops anything, and reporting a non-zero count for it would be a lie
// the page cannot check.
function _perf_dropped_count_for(obs) {
    var n = 0;
    if (_perf_po_types(obs).indexOf('resource') !== -1) n += _perf_rt_dropped;
    return n;
}

// §5.5 «filter buffer by name and type»: the result is sorted by startTime.
// `Array.prototype.sort` is stable, so equal timestamps keep arrival order.
function _perf_po_filter(entries, name, type) {
    var out = [];
    for (var i = 0; i < entries.length; i++) {
        var e = entries[i];
        if (type !== null && e.entryType !== type) continue;
        if (name !== null && e.name !== name) continue;
        out.push(e);
    }
    out.sort(function(a, b) { return a.startTime - b.startTime; });
    return out;
}

// §4.2.2 `interface PerformanceObserverEntryList` — the first callback
// argument. No IDL constructor; the list is built off the prototype.
var _perf_po_entry_list_iface = function PerformanceObserverEntryList() { throw new TypeError('Illegal constructor'); };
Object.defineProperty(globalThis, 'PerformanceObserverEntryList', {
    value: _perf_po_entry_list_iface, writable: true, enumerable: false, configurable: true,
});
function _perf_po_list_check(obj) {
    if (!(obj instanceof _perf_po_entry_list_iface)) throw new TypeError('Illegal invocation');
}
PerformanceObserverEntryList.prototype.getEntries = function getEntries() {
    _perf_po_list_check(this);
    return _perf_po_filter(this._entries, null, null);
};
PerformanceObserverEntryList.prototype.getEntriesByType = function getEntriesByType(type) {
    _perf_po_list_check(this);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'getEntriesByType' on 'PerformanceObserverEntryList': 1 argument required, but only 0 present.");
    return _perf_po_filter(this._entries, null, String(type));
};
PerformanceObserverEntryList.prototype.getEntriesByName = function getEntriesByName(name) {
    _perf_po_list_check(this);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'getEntriesByName' on 'PerformanceObserverEntryList': 1 argument required, but only 0 present.");
    var type = arguments[1];
    return _perf_po_filter(this._entries, String(name), type === undefined ? null : String(type));
};
function _perf_po_make_entry_list(entries) {
    var list = Object.create(PerformanceObserverEntryList.prototype);
    Object.defineProperty(list, '_entries', { value: entries, writable: false, enumerable: false, configurable: false });
    return list;
}

// WebIDL interface-object shape for both interfaces (§3.7): the prototype is
// non-writable, members are enumerable, the class string names the
// interface.
[[PerformanceObserver, 'PerformanceObserver'],
 [PerformanceObserverEntryList, 'PerformanceObserverEntryList']].forEach(function(pair) {
    var iface = pair[0];
    var proto = iface.prototype;
    Object.keys(proto).forEach(function(k) {
        Object.defineProperty(proto, k, { enumerable: true, writable: true, configurable: true });
    });
    Object.defineProperty(proto, Symbol.toStringTag, { value: pair[1], configurable: true });
    Object.defineProperty(iface, 'prototype', { writable: false, enumerable: false, configurable: false });
});

// Invoke one observer's callback with its drained entries. The callback takes
// THREE arguments (§4 `PerformanceObserverCallback`): the entry list, the
// observer, and a `PerformanceObserverCallbackOptions` whose
// `droppedEntriesCount` is present only while the observer's «requires
// dropped entries» flag is up (BUG-840). `this` is the observer (§5.3
// «invoke … with po as the callback this value»).
function _perf_deliver_to_observer(obs, entries) {
    var list = _perf_po_make_entry_list(entries);
    var options = {};
    if (obs._requiresDropped) {
        options.droppedEntriesCount = _perf_dropped_count_for(obs);
        obs._requiresDropped = false;
    }
    try { obs._cb.call(obs, list, obs, options); } catch(e) { _lumen_report_exception(e); }
}

// §5.3 «queue the PerformanceObserver task»: at most one pending per global.
function _perf_queue_observer_task() {
    if (_perf_po_task_queued) return;
    _perf_po_task_queued = true;
    _perf_queue_task(_perf_run_observer_task);
}
function _perf_run_observer_task() {
    _perf_po_task_queued = false;
    var notifyList = _perf_observers.slice();
    for (var i = 0; i < notifyList.length; i++) {
        var po = notifyList[i];
        var entries = po._buffer;
        // The spec text says «return» here; every engine continues with the
        // next observer, and an early return would starve every observer
        // registered after an idle one.
        if (entries.length === 0 && !po._forceDeliver) continue;
        po._buffer = [];
        po._forceDeliver = false;
        _perf_deliver_to_observer(po, entries);
    }
}

// §5.1 «queue a PerformanceEntry», observer half — called for every new entry
// (mark/measure/paint/LCP/layout-shift/resource/navigation/longtask/LoAF). The
// caller has already put the entry into the performance entry buffer.
function _perf_observer_notify(entries) {
    var any = false;
    for (var i = 0; i < _perf_observers.length; i++) {
        var obs = _perf_observers[i];
        var types = _perf_po_types(obs);
        for (var j = 0; j < entries.length; j++) {
            if (types.indexOf(entries[j].entryType) !== -1) {
                obs._buffer.push(entries[j]);
                any = true;
            }
        }
    }
    if (any) _perf_queue_observer_task();
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
    // BUG-665: run under a background scheduling state (scheduler.rs), so a
    // `scheduler.yield()` inside the callback continues at that priority.
    try {
        if (typeof _lumen_sched_idle_invoke === 'function') _lumen_sched_idle_invoke(fn, deadline);
        else fn(deadline);
    } catch (e) { _lumen_report_exception(e); }
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
