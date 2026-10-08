
// ── performance (HR Timer — W3C HR Time L2 + User Timing L3) ─────────────────
// Time origin is the instant this block ran: the native DOM install for the
// page, the worker global-scope install for a worker. HR Time L3 §4.2 makes it
// a property of the global scope, so a worker started later legitimately gets
// a later origin than the page that spawned it.
var _perf_origin_ms = typeof _lumen_now_ms === 'function' ? _lumen_now_ms() : 0;
// Internal entry store: array of {entryType, name, startTime, duration}.
var _perf_entries = [];
// The most recent 'navigation' entry delivered by `_lumen_deliver_perf_entry`
// (web_api_shim_tail.js) — `performance.timing`/`performance.navigation`
// (BUG-767) derive from this same snapshot rather than a second channel.
var _perf_last_navigation_entry = null;

// ── Resource Timing L2 §4.4: the resource timing buffer ──────────────────────
// The `resource` entry type is the only one with a bounded buffer, so these
// live next to `_perf_entries` rather than inside it: every other type is
// appended without a limit. `_perf_rt_size` counts only the `resource` entries
// *currently in* `_perf_entries` — clearResourceTimings() resets it to 0, which
// is what makes room for the secondary buffer to drain.
var _perf_rt_limit = 250;           // resource timing buffer size limit
var _perf_rt_size = 0;              // resource timing buffer current size
var _perf_rt_secondary = [];        // resource timing secondary buffer
var _perf_rt_full_pending = false;  // resource timing buffer full event pending flag
// Performance Timeline L2 §6.2.1 `droppedEntriesCount` for entryType 'resource':
// entries the page never made room for, counted for the lifetime of the
// document. Not reset by clearResourceTimings() — the drop already happened.
var _perf_rt_dropped = 0;

// §4.4 «can add resource timing entry».
function _perf_rt_can_add() { return _perf_rt_size < _perf_rt_limit; }

// Queue an engine task. Written straight into `_lumen_timers` with `nesting: 0`
// where that queue exists (the page), for the reason `_ro_schedule_initial` and
// `_lumen_fire_hashchange` give: the §8.6 4 ms clamp is about timer *nesting*
// and must not apply to a task the engine queues on the page's behalf. This
// block is also spliced into a WorkerGlobalScope, which has neither that queue
// nor — at the instant this shim is evaluated — a `setTimeout`, hence both
// fallbacks.
function _perf_queue_task(fn) {
    if (typeof _lumen_timers !== 'undefined' && _lumen_timers
        && typeof _lumen_timer_seq === 'number') {
        var deadline = (typeof _lumen_now_ms === 'function') ? _lumen_now_ms() : 0;
        _lumen_timers.push({ id: _lumen_timer_seq++, fn: fn, deadline: deadline, interval: null, nesting: 0 });
        if (typeof _lumen_request_wakeup === 'function') _lumen_request_wakeup(deadline);
        return;
    }
    if (typeof setTimeout === 'function') { setTimeout(fn, 0); return; }
    fn();
}

// §4.4 «add a PerformanceResourceTiming entry». Answers whether the entry
// landed in the buffer; observers are notified either way, because the
// performance entry buffer and the observer stream are two separate sinks
// (Performance Timeline L2 §6.2.1) — a page with a zero-sized buffer still
// gets its PerformanceObserver callback.
function _perf_rt_add(entry) {
    if (_perf_rt_can_add() && !_perf_rt_full_pending) {
        _perf_entries.push(entry);
        _perf_rt_size++;
        return true;
    }
    if (!_perf_rt_full_pending) {
        _perf_rt_full_pending = true;
        _perf_queue_task(_perf_rt_fire_buffer_full);
    }
    _perf_rt_secondary.push(entry);
    return false;
}

// §4.4 «copy secondary buffer».
function _perf_rt_copy_secondary() {
    while (_perf_rt_secondary.length > 0 && _perf_rt_can_add()) {
        _perf_entries.push(_perf_rt_secondary.shift());
        _perf_rt_size++;
    }
}

// §4.4 «fire a buffer full event». The loop is the spec's: the page may react
// to the event by clearing the buffer or raising the limit, and then the
// entries it was about to lose are copied in after all. One pass that makes no
// progress means the page did not make room, so the remainder is dropped —
// counted, because that count is what `droppedEntriesCount` reports.
function _perf_rt_fire_buffer_full() {
    while (_perf_rt_secondary.length > 0) {
        var before = _perf_rt_secondary.length;
        if (!_perf_rt_can_add()) _perf_rt_dispatch_buffer_full();
        _perf_rt_copy_secondary();
        var after = _perf_rt_secondary.length;
        if (before <= after) {
            _perf_rt_dropped += after;
            _perf_rt_secondary = [];
            break;
        }
    }
    _perf_rt_full_pending = false;
}

// `resourcetimingbufferfull` at the Performance object. `Event` belongs to the
// page shim, so a worker (which evaluates this block without it) gets a plain
// object with the same shape — EventTarget.dispatchEvent reads only `type`.
function _perf_rt_dispatch_buffer_full() {
    var ev = null;
    if (typeof Event === 'function') {
        try { ev = new Event('resourcetimingbufferfull'); } catch (e) { ev = null; }
    }
    if (!ev) {
        ev = { type: 'resourcetimingbufferfull', target: null, currentTarget: null,
               defaultPrevented: false, isTrusted: true };
    }
    performance.dispatchEvent(ev);
}

// HR Time L3 §4 declares `interface Performance : EventTarget`, so this is a
// real interface — constructor plus a prototype chained to EventTarget —
// rather than the flat object literal it used to be (BUG-400). A singleton is
// still an *instance*: page code legitimately calls
// `performance.addEventListener('resourcetimingbufferfull', ...)` (Resource
// Timing L2 §4.4) and checks `performance instanceof Performance`, and neither
// works when the methods are own properties of a literal. Putting the
// operations on the prototype also leaves the instance with no own enumerable
// properties, which is what makes the WebIDL default `toJSON()` below the only
// thing `JSON.stringify(performance)` can report — same as in browsers.
// Not constructible from script: the IDL declares no constructor.
function Performance() { throw new TypeError('Illegal constructor'); }
Performance.prototype = Object.create(EventTarget.prototype);
Performance.prototype.constructor = Performance;

// `readonly attribute DOMHighResTimeStamp timeOrigin` — a readonly WebIDL
// attribute is a getter-only accessor on the prototype, not a writable data
// property (class of BUG-366): page script must not be able to answer for the
// engine by plain assignment.
Object.defineProperty(Performance.prototype, 'timeOrigin', {
    get: function() { return _perf_origin_ms; },
    enumerable: true, configurable: true,
});

Performance.prototype.now = function() {
    return (typeof _lumen_now_ms === 'function' ? _lumen_now_ms() : 0) - _perf_origin_ms;
};
// User Timing L3 §4.2/§4.3 `[Default] object toJSON()` — shared by
// PerformanceMark and PerformanceMeasure, the only two entry types with a
// `detail` attribute.
function _perf_user_timing_to_json() {
    return { name: this.name, entryType: this.entryType, startTime: this.startTime,
              duration: this.duration, detail: this.detail };
}
// User Timing L3 §4.2/§4.3: `detail` is a structured clone of what the page
// passed, not the object itself — a later mutation of the page's object must
// not show through the entry, and an unserialisable value (a function) is a
// DataCloneError from mark()/measure(). `structuredClone` is a page-shim
// global (web_api_shim_tail_b.js); a scope without it keeps the raw value.
function _perf_clone_detail(detail) {
    if (detail === undefined || detail === null) return null;
    return typeof structuredClone === 'function' ? structuredClone(detail) : detail;
}
// `readonly attribute any detail` is a getter on each interface prototype
// (WebIDL), not an own data property: the value sits in a non-enumerable slot.
function _perf_set_detail(entry, detail) {
    Object.defineProperty(entry, '_perf_detail', {
        value: detail, writable: false, enumerable: false, configurable: true,
    });
}
// Taken off an object-literal accessor so the function's `name` is the WebIDL
// `get detail`.
function _perf_detail_getter(ctor) {
    return Object.getOwnPropertyDescriptor({
        get detail() {
            if (!(this instanceof ctor)) throw new TypeError('Illegal invocation');
            return this._perf_detail;
        },
    }, 'detail').get;
}
// The read-only PerformanceTiming attribute names a Window-scope mark may not
// take (User Timing L3 §4.2 constructor step 1): the spec's `measure()`
// resolves a string against these before marks, so such a mark would be
// unreachable by name.
var _PERF_TIMING_ATTR_NAMES = [
    'navigationStart', 'unloadEventStart', 'unloadEventEnd', 'redirectStart',
    'redirectEnd', 'fetchStart', 'domainLookupStart', 'domainLookupEnd',
    'connectStart', 'connectEnd', 'secureConnectionStart', 'requestStart',
    'responseStart', 'responseEnd', 'domLoading', 'domInteractive',
    'domContentLoadedEventStart', 'domContentLoadedEventEnd', 'domComplete',
    'loadEventStart', 'loadEventEnd',
];
// Entry fields are set with [[DefineOwnProperty]], not assignment: the
// PerformanceEntry accessors below have no setter, so `entry.name = x` on an
// object that inherits from it would silently not create the field (BUG-1189).
function _perf_put(o, k, v) {
    Object.defineProperty(o, k, { value: v, writable: true, enumerable: true, configurable: true });
}
// Performance Timeline L2 §3 `interface PerformanceEntry` (BUG-1189) — the
// common base every entry interface below and in web_api_shim_tail.js chains
// its prototype to. No IDL constructor. Entries keep `name`/`entryType`/
// `startTime`/`duration` as own data properties (they shadow the accessors
// here); the accessors answer only for an entry that lacks the own field
// (`id`/`navigationId`, which no entry sets) and throw on anything that is not
// a PerformanceEntry, the prototype object included. A function *expression*
// published as a non-enumerable global, as `_perf_mark_iface` explains.
var _perf_entry_iface = function PerformanceEntry() { throw new TypeError('Illegal constructor'); };
function _perf_entry_getter(attr, dflt) {
    var g = {};
    g[attr] = function() {
        if (this === _perf_entry_iface.prototype || !(this instanceof _perf_entry_iface)) {
            throw new TypeError("Failed to read the '" + attr + "' property from 'PerformanceEntry': Illegal invocation");
        }
        return dflt;
    };
    Object.defineProperty(g[attr], 'name', { value: 'get ' + attr, configurable: true });
    return g[attr];
}
['id', 'name', 'entryType', 'startTime', 'duration', 'navigationId'].forEach(function(attr) {
    Object.defineProperty(_perf_entry_iface.prototype, attr, {
        get: _perf_entry_getter(attr, attr === 'name' || attr === 'entryType' ? '' : 0),
        enumerable: true, configurable: true });
});
_perf_entry_iface.prototype.toJSON = function toJSON() {
    if (this === _perf_entry_iface.prototype || !(this instanceof _perf_entry_iface)) {
        throw new TypeError("Failed to execute 'toJSON' on 'PerformanceEntry': Illegal invocation");
    }
    return { id: this.id, name: this.name, entryType: this.entryType, startTime: this.startTime,
             duration: this.duration, navigationId: this.navigationId };
};
Object.defineProperty(_perf_entry_iface, 'prototype', { writable: false });
Object.defineProperty(_perf_entry_iface.prototype, Symbol.toStringTag,
    { value: 'PerformanceEntry', configurable: true });
Object.defineProperty(globalThis, 'PerformanceEntry',
    { value: _perf_entry_iface, writable: true, enumerable: false, configurable: true });
// User Timing L3 §4.2 `interface PerformanceMark : PerformanceEntry` with
// `constructor(DOMString markName, optional PerformanceMarkOptions markOptions = {})`
// (BUG-687) — unlike the other entry interfaces in this shim it IS
// constructible, and `performance.mark()` is defined as running this very
// constructor and then queueing the result. Fields stay own properties, as on
// every other entry type here. `[Exposed=(Window,Worker)]`: this block is
// shared with the worker scope. A function *expression* under an internal
// name, published below as a non-enumerable global (WebIDL §3.7.1) — a
// top-level declaration would land on the global enumerable and
// non-configurable (idlharness), as `_perf_po_iface` explains.
// `markOptions` comes off `arguments`: an optional argument does not count
// towards the WebIDL `length`, which is 1.
var _perf_mark_iface = function PerformanceMark(markName) {
    var markOptions = arguments[1];
    if (new.target === undefined) {
        throw new TypeError("Failed to construct 'PerformanceMark': Please use the 'new' operator.");
    }
    if (arguments.length < 1) {
        throw new TypeError("Failed to construct 'PerformanceMark': 1 argument required, but only 0 present.");
    }
    var name = String(markName);
    if (typeof document === 'object' && document !== null
        && _PERF_TIMING_ATTR_NAMES.indexOf(name) !== -1) {
        throw new DOMException("Failed to construct 'PerformanceMark': '" + name
            + "' is part of the PerformanceTiming interface, and cannot be used as a mark name.", 'SyntaxError');
    }
    // WebIDL dictionary conversion: only undefined/null (→ `{}`) or an object.
    if (markOptions !== undefined && markOptions !== null
        && typeof markOptions !== 'object' && typeof markOptions !== 'function') {
        throw new TypeError("Failed to construct 'PerformanceMark': The provided value is not of type 'PerformanceMarkOptions'.");
    }
    var opts = markOptions === undefined || markOptions === null ? {} : markOptions;
    var start;
    if (opts.startTime !== undefined) {
        start = Number(opts.startTime);
        if (!isFinite(start)) {
            throw new TypeError("Failed to construct 'PerformanceMark': The provided double value is non-finite.");
        }
        if (start < 0) {
            throw new TypeError("Failed to construct 'PerformanceMark': '" + name
                + "' cannot have a negative start time.");
        }
    } else {
        start = performance.now();
    }
    _perf_put(this, 'entryType', 'mark');
    _perf_put(this, 'name', name);
    _perf_put(this, 'startTime', start);
    _perf_put(this, 'duration', 0);
    _perf_set_detail(this, _perf_clone_detail(opts.detail));
};
_perf_mark_iface.prototype.toJSON = _perf_user_timing_to_json;
Object.defineProperty(_perf_mark_iface.prototype, 'detail',
    { get: _perf_detail_getter(_perf_mark_iface), enumerable: true, configurable: true });
// User Timing L3 §4.3 `interface PerformanceMeasure : PerformanceEntry` — no
// IDL constructor; entries are built off the prototype by `measure()` only.
var _perf_measure_iface = function PerformanceMeasure() { throw new TypeError('Illegal constructor'); };
_perf_measure_iface.prototype.toJSON = _perf_user_timing_to_json;
Object.defineProperty(_perf_measure_iface.prototype, 'detail',
    { get: _perf_detail_getter(_perf_measure_iface), enumerable: true, configurable: true });
Object.setPrototypeOf(_perf_mark_iface.prototype, _perf_entry_iface.prototype);
Object.setPrototypeOf(_perf_measure_iface.prototype, _perf_entry_iface.prototype);
// An interface object's `prototype` is non-writable (WebIDL §3.7.1).
Object.defineProperty(_perf_mark_iface, 'prototype', { writable: false });
Object.defineProperty(_perf_measure_iface, 'prototype', { writable: false });
// `Object.prototype.toString` must name the interface — what the registry
// WPT's `[object PerformanceMark]` check reads; an entry built off a plain
// function prototype would otherwise stringify as `[object Object]`.
Object.defineProperty(_perf_mark_iface.prototype, Symbol.toStringTag,
    { value: 'PerformanceMark', configurable: true });
Object.defineProperty(_perf_measure_iface.prototype, Symbol.toStringTag,
    { value: 'PerformanceMeasure', configurable: true });
Object.defineProperty(globalThis, 'PerformanceMark',
    { value: _perf_mark_iface, writable: true, enumerable: false, configurable: true });
Object.defineProperty(globalThis, 'PerformanceMeasure',
    { value: _perf_measure_iface, writable: true, enumerable: false, configurable: true });
// User Timing L3 §4.3 "convert a mark to a timestamp". A number is used as-is
// (negative or non-finite is a TypeError); anything else is a mark name. In a
// Window a PerformanceTiming attribute name resolves first, through
// `performance.timing` relative to `navigationStart`, and a zero attribute
// (an event that never happened) is an InvalidAccessError; otherwise the most
// recent same-named mark, and none at all is a SyntaxError.
function _perf_mark_to_timestamp(value) {
    if (typeof value === 'number') {
        if (!isFinite(value)) throw new TypeError("Failed to execute 'measure' on 'Performance': The provided double value is non-finite.");
        if (value < 0) throw new TypeError("Failed to execute 'measure' on 'Performance': Timestamps cannot be negative.");
        return value;
    }
    var name = String(value);
    if (typeof document === 'object' && document !== null
        && _PERF_TIMING_ATTR_NAMES.indexOf(name) !== -1 && typeof performance.timing === 'object') {
        var v = performance.timing[name];
        if (name === 'navigationStart') return 0;
        if (!v) {
            throw new DOMException("Failed to execute 'measure' on 'Performance': The PerformanceTiming attribute '"
                + name + "' is 0.", 'InvalidAccessError');
        }
        return v - performance.timing.navigationStart;
    }
    var m = _perf_entries_by_name(name, 'mark');
    if (m.length === 0) {
        throw new DOMException("Failed to execute 'measure' on 'Performance': The mark '" + name
            + "' does not exist.", 'SyntaxError');
    }
    return m[m.length - 1].startTime;
}
// User Timing L3 §4.2 — performance.mark(name, options?): the PerformanceMark
// constructor, then queue + buffer the entry it made.
// WebIDL operations: named, `length` counts only the required argument, and
// a missing one is a TypeError — the operation's, not the constructor's.
Performance.prototype.mark = function mark(markName) {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    if (arguments.length < 1) throw new TypeError("Failed to execute 'mark' on 'Performance': 1 argument required, but only 0 present.");
    var entry = new _perf_mark_iface(markName, arguments[1]);
    _perf_entries.push(entry);
    // Guarded: PerformanceObserver is part of the page shim only, so in a
    // worker scope this function does not exist (see PERFORMANCE_SHIM docs).
    if (typeof _perf_observer_notify === 'function') _perf_observer_notify([entry]);
    return entry;
};
// User Timing L3 §4.3 — performance.measure(name, startOrMeasureOptions?, endMark?).
// `startOrMeasureOptions` is either a mark name/timestamp (named-args form) or
// a `PerformanceMeasureOptions` dictionary (`{start, end, duration, detail}`,
// dictionary form) — the two forms are mutually exclusive, so the dictionary
// case takes `endMark` off the table entirely rather than merging with it.
Performance.prototype.measure = function measure(measureName) {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    if (arguments.length < 1) throw new TypeError("Failed to execute 'measure' on 'Performance': 1 argument required, but only 0 present.");
    var name = measureName, startOrMeasureOptions = arguments[1], endMark = arguments[2];
    var start, end, detail = null;
    var opts = null;
    if (startOrMeasureOptions !== null && startOrMeasureOptions !== undefined
        && (typeof startOrMeasureOptions === 'object' || typeof startOrMeasureOptions === 'function')) {
        opts = startOrMeasureOptions;
    }
    var hasStart = false, hasEnd = false, hasDuration = false;
    if (opts !== null) {
        hasStart = opts.start !== undefined;
        hasEnd = opts.end !== undefined;
        hasDuration = opts.duration !== undefined;
        var hasDetail = opts.detail !== undefined;
        if (hasStart || hasEnd || hasDuration || hasDetail) {
            if (endMark !== undefined) {
                throw new TypeError("Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, |end_mark| must not be passed.");
            }
            if (!hasStart && !hasEnd) {
                throw new TypeError("Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, it must have at least one of 'start' or 'end'.");
            }
            if (hasStart && hasEnd && hasDuration) {
                throw new TypeError("Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, it must not have all of 'start', 'duration', and 'end'.");
            }
        }
        if (opts.detail !== undefined) detail = opts.detail;
    }
    // WebIDL: a union member that is neither an object nor a number becomes a
    // DOMString, so a number is a mark *name* here ('51.15'), not a timestamp.
    function toTimeArg(v) { return typeof v === 'number' ? v : String(v); }
    if (endMark !== undefined) {
        end = _perf_mark_to_timestamp(String(endMark));
    } else if (hasEnd) {
        end = _perf_mark_to_timestamp(toTimeArg(opts.end));
    } else if (hasStart && hasDuration) {
        end = _perf_mark_to_timestamp(toTimeArg(opts.start)) + Number(opts.duration);
    } else {
        end = this.now();
    }
    if (opts === null && startOrMeasureOptions !== undefined) {
        start = _perf_mark_to_timestamp(String(startOrMeasureOptions));
    } else if (hasStart) {
        start = _perf_mark_to_timestamp(toTimeArg(opts.start));
    } else if (hasDuration && hasEnd) {
        start = end - Number(opts.duration);
    } else {
        start = 0;
    }
    var entry = Object.create(_perf_measure_iface.prototype);
    _perf_put(entry, 'entryType', 'measure');
    _perf_put(entry, 'name', String(name));
    _perf_put(entry, 'startTime', start);
    _perf_put(entry, 'duration', end - start);
    _perf_set_detail(entry, _perf_clone_detail(detail));
    _perf_entries.push(entry);
    if (typeof _perf_observer_notify === 'function') _perf_observer_notify([entry]);
    return entry;
};
// WebIDL operations: named functions with the IDL `length` (optional
// arguments do not count) and a TypeError for a missing required argument —
// what `performance-timeline/idlharness.any.js` checks (BUG-648), including
// the brand check: `this` that is not a Performance is a TypeError.
Performance.prototype.getEntriesByName = function getEntriesByName(name) {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    if (arguments.length < 1) throw new TypeError("Failed to execute 'getEntriesByName' on 'Performance': 1 argument required, but only 0 present.");
    return _perf_entries_by_name(String(name), arguments[1]);
};
Performance.prototype.getEntriesByType = function getEntriesByType(type) {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    if (arguments.length < 1) throw new TypeError("Failed to execute 'getEntriesByType' on 'Performance': 1 argument required, but only 0 present.");
    var t = String(type);
    return _perf_entries.filter(function(e) { return e.entryType === t; });
};
Performance.prototype.getEntries = function getEntries() {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    return _perf_entries.slice();
};
Performance.prototype.clearMarks = function clearMarks() {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    var name = arguments[0];
    if (typeof name === 'string') {
        _perf_entries = _perf_entries.filter(function(e) { return !(e.entryType === 'mark' && e.name === name); });
    } else {
        _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'mark'; });
    }
};
Performance.prototype.clearMeasures = function clearMeasures() {
    if (!(this instanceof Performance)) throw new TypeError('Illegal invocation');
    var name = arguments[0];
    if (typeof name === 'string') {
        _perf_entries = _perf_entries.filter(function(e) { return !(e.entryType === 'measure' && e.name === name); });
    } else {
        _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'measure'; });
    }
};
// W3C Resource Timing L2 §4.4 — clears all 'resource' entries from the buffer.
// Resetting the current size is half the operation, not bookkeeping: it is the
// only way a page can make room for the secondary buffer while the buffer-full
// event is being handled.
Performance.prototype.clearResourceTimings = function() {
    _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'resource'; });
    _perf_rt_size = 0;
};
// W3C Resource Timing L2 §4.4 — sets the buffer size limit. WebIDL
// `unsigned long`, so the argument wraps modulo 2^32 rather than being clamped:
// `setResourceTimingBufferSize(-1)` is 4294967295, i.e. effectively unbounded.
Performance.prototype.setResourceTimingBufferSize = function(maxSize) {
    var n = Number(maxSize);
    if (!isFinite(n)) n = 0;
    _perf_rt_limit = (n < 0 ? Math.ceil(n) : Math.floor(n)) >>> 0;
};
// Resource Timing L2 §4.4 `attribute EventHandler onresourcetimingbufferfull`.
// An IDL event handler is an accessor on the interface prototype, so
// `'onresourcetimingbufferfull' in performance` answers true even before a
// handler is assigned — a plain expando (what this used to be) answers false
// and every feature detection reads the API as absent.
Object.defineProperty(Performance.prototype, 'onresourcetimingbufferfull', {
    get: function() {
        return this._onresourcetimingbufferfull !== undefined ? this._onresourcetimingbufferfull : null;
    },
    set: function(v) {
        this._onresourcetimingbufferfull = (typeof v === 'function') ? v : null;
    },
    enumerable: true, configurable: true,
});
// ── PerformanceTiming / PerformanceNavigation (legacy Navigation Timing L1) ──
// BUG-767. Both interfaces are derived from the very snapshot the L2
// `PerformanceNavigationTiming` entry already carries (`nav_timing.rs::detail_json`,
// BUG-640, stashed into `_perf_last_navigation_entry` by `_lumen_deliver_perf_entry`
// in web_api_shim_tail.js) — one conversion function, not a second collection
// path, per ADR-026's warning about two independent sources for the same fact.

// L2 attributes are `DOMHighResTimeStamp`s relative to `timeOrigin` (a
// navigation entry's own `startTime` is always 0, so its milestone fields are
// already elapsed-ms-since-`timeOrigin`); L1 attributes are Unix-epoch
// milliseconds — add the origin back.
function _perf_l2_to_l1(relMs) {
    return Math.round(_perf_origin_ms + relMs);
}

// The full L1 attribute list, in spec order — shared between instance
// construction and `toJSON()` so the two can never drift apart (a `for...in`
// over the instance would also pick up `toJSON` itself, since a plain
// prototype-method assignment is enumerable).
var _perf_timing_field_names = [
    'navigationStart', 'unloadEventStart', 'unloadEventEnd', 'redirectStart',
    'redirectEnd', 'fetchStart', 'domainLookupStart', 'domainLookupEnd',
    'connectStart', 'connectEnd', 'secureConnectionStart', 'requestStart',
    'responseStart', 'responseEnd', 'domLoading', 'domInteractive',
    'domContentLoadedEventStart', 'domContentLoadedEventEnd', 'domComplete',
    'loadEventStart', 'loadEventEnd',
];

// Navigation Timing L1 §4.4 `interface PerformanceTiming`.
function PerformanceTiming() { throw new TypeError('Illegal constructor'); }
// §4.4 `[Default] object toJSON()`.
PerformanceTiming.prototype.toJSON = function() {
    var out = {};
    for (var i = 0; i < _perf_timing_field_names.length; i++) {
        var k = _perf_timing_field_names[i];
        out[k] = this[k];
    }
    return out;
};

function _perf_make_timing() {
    var nav = _perf_last_navigation_entry;
    var origin = Math.round(_perf_origin_ms);
    // Spec fallback for a milestone that hasn't happened yet is 0, same as
    // every other not-yet-fired PerformanceTiming attribute in real browsers.
    function ms(key) { return nav ? _perf_l2_to_l1(nav[key] || 0) : 0; }
    var t = Object.create(PerformanceTiming.prototype);
    var fields = {
        navigationStart: origin,
        unloadEventStart: ms('unloadEventStart'),
        unloadEventEnd: ms('unloadEventEnd'),
        redirectStart: ms('redirectStart'),
        redirectEnd: ms('redirectEnd'),
        fetchStart: ms('fetchStart'),
        domainLookupStart: ms('domainLookupStart'),
        domainLookupEnd: ms('domainLookupEnd'),
        connectStart: ms('connectStart'),
        connectEnd: ms('connectEnd'),
        secureConnectionStart: ms('secureConnectionStart'),
        requestStart: ms('requestStart'),
        responseStart: ms('responseStart'),
        responseEnd: ms('responseEnd'),
        // `domLoading` was removed from Navigation Timing L2 (no L2 attribute
        // answers it, so no key exists on `nav` for it either) — kept here
        // only for L1 back-compat, honestly stubbed to `navigationStart`
        // rather than a fabricated sub-phase timestamp.
        domLoading: origin,
        domInteractive: ms('domInteractive'),
        domContentLoadedEventStart: ms('domContentLoadedEventStart'),
        domContentLoadedEventEnd: ms('domContentLoadedEventEnd'),
        domComplete: ms('domComplete'),
        loadEventStart: ms('loadEventStart'),
        loadEventEnd: ms('loadEventEnd'),
    };
    for (var i = 0; i < _perf_timing_field_names.length; i++) {
        var k = _perf_timing_field_names[i];
        Object.defineProperty(t, k, { value: fields[k], enumerable: true, configurable: true });
    }
    return t;
}

// Navigation Timing L1 §4.3 `interface PerformanceNavigation`.
function PerformanceNavigation() { throw new TypeError('Illegal constructor'); }
function _perf_nav_define_type_constants(target) {
    Object.defineProperty(target, 'TYPE_NAVIGATE', { value: 0, enumerable: true });
    Object.defineProperty(target, 'TYPE_RELOAD', { value: 1, enumerable: true });
    Object.defineProperty(target, 'TYPE_BACK_FORWARD', { value: 2, enumerable: true });
    Object.defineProperty(target, 'TYPE_RESERVED', { value: 255, enumerable: true });
}
_perf_nav_define_type_constants(PerformanceNavigation);
_perf_nav_define_type_constants(PerformanceNavigation.prototype);
PerformanceNavigation.prototype.toJSON = function() {
    return { type: this.type, redirectCount: this.redirectCount };
};

// `nav_timing.rs`'s `type` is the L2 string (always `"navigate"` today — see
// that module's doc comment on why reload/back-forward aren't distinguished
// yet); map it to the legacy numeric constant rather than picking a value
// independently.
function _perf_nav_legacy_type(l2Type) {
    if (l2Type === 'reload') return PerformanceNavigation.TYPE_RELOAD;
    if (l2Type === 'back_forward') return PerformanceNavigation.TYPE_BACK_FORWARD;
    return PerformanceNavigation.TYPE_NAVIGATE;
}

function _perf_make_navigation() {
    var nav = _perf_last_navigation_entry;
    var n = Object.create(PerformanceNavigation.prototype);
    Object.defineProperty(n, 'type', {
        value: nav ? _perf_nav_legacy_type(nav.type) : PerformanceNavigation.TYPE_NAVIGATE,
        enumerable: true, configurable: true,
    });
    Object.defineProperty(n, 'redirectCount', {
        value: nav ? (nav.redirectCount || 0) : 0,
        enumerable: true, configurable: true,
    });
    return n;
}

// Legacy Navigation Timing L2 §5-6 partial attributes on `Performance`.
Object.defineProperty(Performance.prototype, 'timing', {
    get: function() { return _perf_make_timing(); },
    enumerable: true, configurable: true,
});
Object.defineProperty(Performance.prototype, 'navigation', {
    get: function() { return _perf_make_navigation(); },
    enumerable: true, configurable: true,
});

// HR Time L3 §4 `[Default] object toJSON()`. The default toJSON operation
// serialises the interface's *attributes*, and the legacy Navigation Timing
// L2 partial above adds `timing`/`navigation` to that set (BUG-767) —
// serialised through their own `toJSON()`, same as every nested WebIDL
// dictionary/interface member would be.
Performance.prototype.toJSON = function() {
    return { timeOrigin: this.timeOrigin, timing: this.timing.toJSON(), navigation: this.navigation.toJSON() };
};

// The one instance. Built with Object.create + an explicit EventTarget
// initialiser because the constructor above deliberately throws for script.
var performance = Object.create(Performance.prototype);
EventTarget.call(performance);

function _perf_entries_by_name(name, type) {
    return _perf_entries.filter(function(e) {
        return e.name === name && (type === undefined || e.entryType === type);
    });
}
