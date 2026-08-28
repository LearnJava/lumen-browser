
// ── performance (HR Timer — W3C HR Time L2 + User Timing L3) ─────────────────
// Time origin is the instant this block ran: the native DOM install for the
// page, the worker global-scope install for a worker. HR Time L3 §4.2 makes it
// a property of the global scope, so a worker started later legitimately gets
// a later origin than the page that spawned it.
var _perf_origin_ms = typeof _lumen_now_ms === 'function' ? _lumen_now_ms() : 0;
// Internal entry store: array of {entryType, name, startTime, duration}.
var _perf_entries = [];

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
// User Timing L3 §4.2 — performance.mark(name, options?)
Performance.prototype.mark = function(name, opts) {
    var start = (opts && typeof opts.startTime === 'number') ? opts.startTime : this.now();
    var entry = { entryType: 'mark', name: String(name), startTime: start, duration: 0 };
    _perf_entries.push(entry);
    // Guarded: PerformanceObserver is part of the page shim only, so in a
    // worker scope this function does not exist (see PERFORMANCE_SHIM docs).
    if (typeof _perf_observer_notify === 'function') _perf_observer_notify([entry]);
    return entry;
};
// User Timing L3 §4.3 — performance.measure(name, start?, end?)
Performance.prototype.measure = function(name, startMark, endMark) {
    var start = 0, end = this.now();
    if (typeof startMark === 'string') {
        var sm = _perf_entries_by_name(startMark, 'mark');
        if (sm.length > 0) start = sm[sm.length - 1].startTime;
    } else if (typeof startMark === 'number') {
        start = startMark;
    }
    if (typeof endMark === 'string') {
        var em = _perf_entries_by_name(endMark, 'mark');
        if (em.length > 0) end = em[em.length - 1].startTime;
    } else if (typeof endMark === 'number') {
        end = endMark;
    }
    var entry = { entryType: 'measure', name: String(name), startTime: start, duration: end - start };
    _perf_entries.push(entry);
    if (typeof _perf_observer_notify === 'function') _perf_observer_notify([entry]);
    return entry;
};
Performance.prototype.getEntriesByName = function(name, type) {
    return _perf_entries_by_name(String(name), type);
};
Performance.prototype.getEntriesByType = function(type) {
    var t = String(type);
    return _perf_entries.filter(function(e) { return e.entryType === t; });
};
Performance.prototype.getEntries = function() { return _perf_entries.slice(); };
Performance.prototype.clearMarks = function(name) {
    if (typeof name === 'string') {
        _perf_entries = _perf_entries.filter(function(e) { return !(e.entryType === 'mark' && e.name === name); });
    } else {
        _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'mark'; });
    }
};
Performance.prototype.clearMeasures = function(name) {
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
// HR Time L3 §4 `[Default] object toJSON()`. The default toJSON operation
// serialises the interface's *attributes*, not its operations, and Performance
// declares exactly one attribute Lumen implements — timeOrigin. The legacy
// Navigation Timing L2 partial adds `timing`/`navigation`, which browsers also
// emit here; Lumen has neither interface (no per-milestone timing data exists
// in the engine at all — the shell delivers a navigation entry as url +
// total duration only), so they are absent rather than faked — BUG-767.
Performance.prototype.toJSON = function() {
    return { timeOrigin: this.timeOrigin };
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
