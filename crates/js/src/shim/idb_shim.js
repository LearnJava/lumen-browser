
// ── IndexedDB (W3C Indexed Database API 3.0) ─────────────────────────────────
// In-memory implementation: databases live in this runtime's JS heap and do not
// persist across reloads (Rust-backed persistence is a separate follow-up task).
// Request 'success'/'error' events and transaction 'complete'/'abort' fire
// asynchronously. The production scheduler queues the stepwise _idb_flush_step
// as a microtask: one unit per step (an open turn, a request dispatch, a
// settlement), so a handler's promise reactions run before the next dispatch
// and the transaction stays active across them (§3.1.7). The full synchronous
// drain _lumen_idb_flush() remains for the unit tests and the service-worker
// scope.
// This mirrors the raf / MutationObserver delivery pattern already used in this
// shim.

var _idb_databases = {};          // name -> { name, version, stores }
var _idb_active_txns = [];        // transactions with pending request dispatches
var _idb_pending_opens = [];      // IDBOpenDBRequest dispatch entries
// Connection queues (Indexed DB §3.3.1). An upgrade or a delete needs exclusive
// access to the database, so it may not run while another connection is open:
// every live IDBDatabase is registered here by name, the request broadcasts
// `versionchange` to them, fires `blocked` at itself while any of them is still
// open, and is parked until close() empties the list. Parking (rather than
// re-queueing) is what keeps a blocked request from spinning the event loop the
// way BUG-842's unbounded drain did: nothing reschedules a flush for it, and
// close() is the only thing that can wake it.
var _idb_connections = {};        // name -> [IDBDatabase] (open or close-pending)
var _idb_parked_opens = [];       // entries waiting for connections to close, in queue order
var _idb_parked_names = {};       // name -> true while that name's queue is blocked
var _idb_flush_scheduled = false; // a flush is pending (microtask or task)
var _idb_flushing = false;        // a flush is running right now
var _idb_dirty = false;           // set by any mutation; drives persistence at flush end
var _idb_dirty_dbs = {};          // db name -> true; which databases' schema needs re-mirroring
// Requests one flush dispatches before handing the remainder to the event loop.
// Bounded work — a cursor walk, a batch of puts — still finishes inside the same
// flush, which is what every caller of _lumen_idb_flush() expects; a request that
// re-arms itself from its own onsuccess (WPT's keep_alive idiom) spends the
// budget instead and yields, so the page's timers, rendering and remaining
// scripts keep running (BUG-842: the drain was a plain while loop inside one
// microtask, and 16.3 M spins in 6 s starved the document completely).
var _IDB_FLUSH_BUDGET = 1024;

// --- persistence (Rust-backed via _lumen_idb_load / _lumen_idb_persist) -------
// The whole per-origin database set is one opaque JSON snapshot. Date keys/values
// are tagged ({__idb_date__: ms}) since JSON has no Date type; everything else is
// plain structured data (numbers, strings, arrays, objects). Persistence is
// best-effort: when no backend is installed the shim stays in-heap-only.

function _idb_serialize() {
    return JSON.stringify(_idb_databases, function(k, v) {
        // `this[k]` is the original (pre-toJSON) value, so Dates are detectable
        // even though `v` is already their ISO string.
        if (this[k] instanceof Date) return { __idb_date__: this[k].getTime() };
        return v;
    });
}

function _idb_deserialize(json) {
    return JSON.parse(json, function(k, v) {
        if (v && typeof v === 'object' && typeof v.__idb_date__ === 'number') return new Date(v.__idb_date__);
        return v;
    });
}

// Writes the current snapshot to the backend if a mutation occurred since the
// last persist. Called at the end of every flush.
function _idb_persist_if_dirty() {
    if (!_idb_dirty) return;
    _idb_dirty = false;
    if (typeof _lumen_idb_persist === 'function') {
        try { _lumen_idb_persist(_idb_serialize()); }
        catch (e) { _lumen_console_error('IDB persist: ' + e); }
    }
    // Phase 3: mirror schema (db version / object stores / indexes) into the
    // structured per-origin SQLite tables so db_version()/list_databases() are
    // queryable independently of the opaque snapshot. Records stay in the snapshot
    // blob, which remains the authoritative lossless restore path; populating
    // idb_records row-by-row is a future incremental optimisation.
    _idb_persist_schema();
}

// Marks one database's schema as needing re-mirroring at the next persist —
// see _idb_dirty_dbs (BUG-917: without this every dirty flush walked every
// origin database's stores and indexes, not just the one that changed).
function _idb_mark_db_dirty(name) {
    if (name) _idb_dirty_dbs[name] = true;
}

// Encode a store keyPath (null | string | array) for IdbSchemaOp::CreateStore.key_path
// (Option<String> on the Rust side): null stays null, arrays are JSON-stringified.
function _idb_keypath_store(kp) {
    if (kp === null || kp === undefined) return null;
    if (Array.isArray(kp)) return JSON.stringify(kp);
    return String(kp);
}

// Encode an index keyPath (string | array) for IdbSchemaOp::CreateIndex.key_path
// (required String on the Rust side): arrays are JSON-stringified.
function _idb_keypath_index(kp) {
    if (Array.isArray(kp)) return JSON.stringify(kp);
    return String(kp);
}

// Write-through the current in-heap schema (versions, stores, indexes) into the
// structured backend. Idempotent (Rust side uses INSERT OR REPLACE). No-op when no
// structured backend is installed.
function _idb_persist_schema() {
    if (typeof _lumen_idb_schema_op !== 'function') return;
    var dirtyDbs = _idb_dirty_dbs;
    _idb_dirty_dbs = {};
    try {
        for (var dbName in dirtyDbs) {
            if (!dirtyDbs.hasOwnProperty(dbName)) continue;
            var db = _idb_databases[dbName];
            // A database whose only dirtying event was its own deletion has
            // nothing left to mirror — DeleteDatabase isn't a schema op (the
            // opaque snapshot stays authoritative for that), so skip it.
            if (!db) continue;
            _lumen_idb_schema_op(JSON.stringify({kind:'SetVersion',db_name:db.name,version:db.version ? Number(db.version) : 1}));
            for (var storeName in db.stores) {
                if (!db.stores.hasOwnProperty(storeName)) continue;
                var store = db.stores[storeName];
                _lumen_idb_schema_op(JSON.stringify({kind:'CreateStore',db_name:db.name,store_name:store.name,key_path:_idb_keypath_store(store.keyPath),auto_increment:!!store.autoIncrement}));
                for (var indexName in store.indexes) {
                    if (!store.indexes.hasOwnProperty(indexName)) continue;
                    var index = store.indexes[indexName];
                    _lumen_idb_schema_op(JSON.stringify({kind:'CreateIndex',db_name:db.name,store_name:store.name,index_name:index.name,key_path:_idb_keypath_index(index.keyPath),unique:!!index.unique,multi_entry:!!index.multiEntry}));
                }
            }
        }
    } catch (e) {
        _lumen_console_error('IDB schema mirror: ' + e);
    }
}

// --- key validation / comparison / extraction (Indexed DB §3.1) --------------

function _idb_is_valid_key(k) {
    var t = typeof k;
    if (t === 'number') return !isNaN(k);
    if (t === 'string') return true;
    if (k instanceof Date) return !isNaN(k.getTime());
    if (Array.isArray(k)) {
        for (var i = 0; i < k.length; i++) if (!_idb_is_valid_key(k[i])) return false;
        return true;
    }
    return false;
}

// Type precedence per spec: number < date < string < array.
function _idb_key_rank(k) {
    if (typeof k === 'number') return 1;
    if (k instanceof Date) return 2;
    if (typeof k === 'string') return 3;
    if (Array.isArray(k)) return 4;
    return 0;
}

// Returns -1, 0 or 1 comparing two valid keys per the IndexedDB key ordering.
function _idb_cmp(a, b) {
    var ra = _idb_key_rank(a), rb = _idb_key_rank(b);
    if (ra !== rb) return ra < rb ? -1 : 1;
    if (ra === 1 || ra === 3) return a < b ? -1 : (a > b ? 1 : 0);
    if (ra === 2) {
        var ta = a.getTime(), tb = b.getTime();
        return ta < tb ? -1 : (ta > tb ? 1 : 0);
    }
    if (ra === 4) {
        var n = Math.min(a.length, b.length);
        for (var i = 0; i < n; i++) {
            var c = _idb_cmp(a[i], b[i]);
            if (c !== 0) return c;
        }
        return a.length < b.length ? -1 : (a.length > b.length ? 1 : 0);
    }
    return 0;
}

// Extracts the key at keyPath from value; returns undefined if any segment is
// missing. keyPath may be a string (dotted), an array (yields an array key), or
// '' (the value itself).
function _idb_extract_key(value, keyPath) {
    if (Array.isArray(keyPath)) {
        var arr = [];
        for (var i = 0; i < keyPath.length; i++) {
            var v = _idb_extract_key(value, keyPath[i]);
            if (v === undefined) return undefined;
            arr.push(v);
        }
        return arr;
    }
    if (keyPath === '') return value;
    var parts = String(keyPath).split('.');
    var cur = value;
    for (var j = 0; j < parts.length; j++) {
        if (cur === null || typeof cur !== 'object') return undefined;
        cur = cur[parts[j]];
        if (cur === undefined) return undefined;
    }
    return cur;
}

// Writes a generated key back into value at a string keyPath (autoIncrement).
function _idb_inject_key(value, keyPath, key) {
    var parts = String(keyPath).split('.');
    var cur = value;
    for (var i = 0; i < parts.length - 1; i++) {
        if (cur[parts[i]] === undefined || cur[parts[i]] === null) cur[parts[i]] = {};
        cur = cur[parts[i]];
    }
    cur[parts[parts.length - 1]] = key;
}

function _idb_error(name, message) {
    return new DOMException(message || name, name);
}

// Indexed DB §3.2 exposes `objectStoreNames`/`indexNames` as a `DOMStringList`
// (length + item() + contains()), not an Array — sites call `.contains()`,
// which a plain Array literal doesn't have (BUG-990). `names` is a plain
// sorted array snapshot; the object built here is a fresh live-value wrapper
// around it, not a shared reference back into caller state.
function _idb_string_list(names) {
    var list = { length: names.length };
    for (var i = 0; i < names.length; i++) list[i] = names[i];
    list.item = function(index) { return (index >= 0 && index < this.length) ? this[index] : null; };
    list.contains = function(str) { return names.indexOf(String(str)) >= 0; };
    return list;
}

// --- IDBKeyRange (Indexed DB §3.1.5) -----------------------------------------

function IDBKeyRange(lower, upper, lowerOpen, upperOpen) {
    this.lower = lower;
    this.upper = upper;
    this.lowerOpen = !!lowerOpen;
    this.upperOpen = !!upperOpen;
}
IDBKeyRange.prototype.includes = function(key) {
    if (!_idb_is_valid_key(key)) throw _idb_error('DataError', 'invalid key');
    if (this.lower !== undefined) {
        var c = _idb_cmp(key, this.lower);
        if (c < 0 || (c === 0 && this.lowerOpen)) return false;
    }
    if (this.upper !== undefined) {
        var c2 = _idb_cmp(key, this.upper);
        if (c2 > 0 || (c2 === 0 && this.upperOpen)) return false;
    }
    return true;
};
IDBKeyRange.only = function(value) {
    if (!_idb_is_valid_key(value)) throw _idb_error('DataError', 'invalid key');
    return new IDBKeyRange(value, value, false, false);
};
IDBKeyRange.lowerBound = function(lower, open) {
    if (!_idb_is_valid_key(lower)) throw _idb_error('DataError', 'invalid key');
    return new IDBKeyRange(lower, undefined, !!open, false);
};
IDBKeyRange.upperBound = function(upper, open) {
    if (!_idb_is_valid_key(upper)) throw _idb_error('DataError', 'invalid key');
    return new IDBKeyRange(undefined, upper, false, !!open);
};
IDBKeyRange.bound = function(lower, upper, lowerOpen, upperOpen) {
    if (!_idb_is_valid_key(lower) || !_idb_is_valid_key(upper)) throw _idb_error('DataError', 'invalid key');
    if (_idb_cmp(lower, upper) > 0) throw _idb_error('DataError', 'lower bound greater than upper bound');
    return new IDBKeyRange(lower, upper, !!lowerOpen, !!upperOpen);
};

// Coerces a query argument (key | IDBKeyRange | null) into an IDBKeyRange or null.
function _idb_to_range(q) {
    if (q === undefined || q === null) return null;
    if (q instanceof IDBKeyRange) return q;
    if (!_idb_is_valid_key(q)) throw _idb_error('DataError', 'invalid key or range');
    return IDBKeyRange.only(q);
}

// --- IDBRequest / IDBOpenDBRequest (Indexed DB §3.5) -------------------------

// Request, transaction and connection are EventTargets (BUG-1706/BUG-914
// rewrite): one listener store per object, the DOM add/remove semantics
// (capture flag, once, handleEvent) come from the shared EventTarget shim,
// which loads ahead of this one in every scope that runs it.
function IDBRequest(source, txn) {
    EventTarget.call(this);
    this.result = undefined;
    this.error = null;
    this.source = source || null;
    this.transaction = txn || null;
    this.readyState = 'pending';
    this.onsuccess = null;
    this.onerror = null;
    this._action = null;
}
IDBRequest.prototype = Object.create(EventTarget.prototype);
IDBRequest.prototype.constructor = IDBRequest;

function IDBOpenDBRequest() {
    IDBRequest.call(this, null, null);
    this.onupgradeneeded = null;
    this.onblocked = null;
}
IDBOpenDBRequest.prototype = Object.create(IDBRequest.prototype);
IDBOpenDBRequest.prototype.constructor = IDBOpenDBRequest;

// Fires `blocked` at an open/delete request whose database still has another
// connection open (Indexed DB §3.3.1 step «fire blocked»). Delete carries
// newVersion = null.
function _idb_fire_blocked(req, oldVersion, newVersion) {
    _idb_dispatch(req,
        _idb_make_event('blocked', req, { oldVersion: oldVersion, newVersion: newVersion }),
        [req]);
}

// Real `Event`s: only `error` bubbles and is cancelable (§3.5.6); `abort`
// bubbles without being cancelable; every other type here neither bubbles nor
// cancels. `isTrusted` is forced — this shim is the trusted source of its own
// events. Extra fields (`oldVersion`, `newVersion`) stay plain properties
// until IDB-EVENTS-S4 mints IDBVersionChangeEvent.
function _idb_make_event(type, target, extra) {
    var ev = new Event(type, {
        bubbles: type === 'error' || type === 'abort',
        cancelable: type === 'error'
    });
    ev.isTrusted = true;
    if (extra) for (var k in extra) ev[k] = extra[k];
    return ev;
}

// Propagation path of a request event, nearest target first: the request, its
// transaction, that transaction's connection (Indexed DB §3.5.6). An open
// request carries no transaction, so its path is just itself.
function _idb_request_path(req) {
    return req.transaction ? [req, req.transaction, req.transaction.db] : [req];
}

// Runs one node's listeners for `event` at `phase`. Capturing runs capture
// listeners only, bubbling non-capture ones only, and at the target both run
// in registration order. The `on<type>` property stays ahead of the listeners
// at its own node (bubble/target phases only, never capture) — the order this
// shim has always used. The live list is snapshotted before the on<type>
// handler and the walk (BUG-1706): a listener added during the dispatch — by
// the onX handler included — never sees this event, and one removed before
// its turn is skipped.
function _idb_invoke(node, event, phase) {
    event.currentTarget = node;
    event.eventPhase = phase;
    var type = event.type;
    var store = node._listeners;
    var list = store ? store[type] : null;
    var snapshot = list ? list.slice() : null;
    if (phase !== 1) {
        var handler = node['on' + type];
        if (typeof handler === 'function') {
            try { handler.call(node, event); } catch (e) { _idb_listener_exception(event, e); }
        }
    }
    if (!snapshot) return;
    for (var i = 0; i < snapshot.length; i++) {
        var entry = snapshot[i];
        if (list.indexOf(entry) < 0) continue;
        if (phase === 1 ? !entry.capture : (phase === 3 && entry.capture)) continue;
        try {
            var cb = entry.callback;
            if (typeof cb === 'function') cb.call(node, event);
            else {
                // DOM «get the current value of handleEvent»: the member lookup
                // itself may throw, a non-callable value throws TypeError.
                var fn = cb.handleEvent;
                if (typeof fn === 'function') fn.call(cb, event);
                else throw new TypeError('handleEvent is not a function');
            }
        } catch (e) { _idb_listener_exception(event, e); }
        if (entry.once) node.removeEventListener(type, entry.callback, entry.capture);
        if (event._stopImmediate) break;
    }
}

// A listener's exception never stops the dispatch — the remaining listeners
// still run (Indexed DB §3.5.6) — but it is reported like a timer's (the old
// console-only swallow hid errors from the page) and recorded on the event:
// the fire-success/error/upgradeneeded procedures abort the owning transaction
// with AbortError when they see the flag.
function _idb_listener_exception(event, e) {
    event._threw = true;
    if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
    else _lumen_console_error('IDB ' + event.type + ' listener: ' + e);
}

// Dispatches `event` along `path` (targets nearest-first, Indexed DB §3.5.6):
// capture far-to-near over the ancestors, the target, then bubble near-to-far
// while the event bubbles (BUG-914). `stopPropagation`/`stopImmediatePropagation`
// (`cancelBubble`/`_stopImmediate`) cut the rest of the path; currentTarget and
// eventPhase reset once the dispatch is over. A default action a caller derives
// from `defaultPrevented` runs only after this whole traversal.
function _idb_dispatch(target, event, path) {
    event.target = target;
    var i;
    for (i = path.length - 1; i > 0; i--) {
        if (event.cancelBubble) return;
        _idb_invoke(path[i], event, 1);
    }
    if (!event.cancelBubble) _idb_invoke(target, event, 2);
    if (event.bubbles) {
        for (i = 1; i < path.length; i++) {
            if (event.cancelBubble) return;
            _idb_invoke(path[i], event, 3);
        }
    }
    event.currentTarget = null;
    event.eventPhase = 0;
}

// Runs a request's deferred action (data read/write), then fires its
// success or error event; on an unhandled error the owning transaction is
// aborted (Indexed DB §3.5.5). Operations run at dispatch time in FIFO order so
// that intra- and inter-transaction ordering matches the spec.
function _idb_dispatch_request(req) {
    if (req._action) {
        var action = req._action;
        req._action = null;
        try { req.result = action(); req.error = null; }
        catch (e) { req.result = undefined; req.error = (e && e.name) ? e : _idb_error('DataError', String(e)); }
    }
    req.readyState = 'done';
    if (req.error) {
        var ev = _idb_make_event('error', req);
        _idb_dispatch_active(req.transaction, function() {
            _idb_dispatch(req, ev, _idb_request_path(req));
        });
        // The error's default action — abort the owning transaction — is
        // decided only after the whole path has run, so a preventDefault at
        // any of its nodes suppresses the abort (§3.5.6). An exception from
        // any listener aborts regardless, with AbortError (§fire an error
        // event): the page's failure to handle the event is the more specific
        // reason, but the spec fixes the transaction's error.
        if (req.transaction && (ev._threw || !ev.defaultPrevented)) {
            req.transaction.error = ev._threw
                ? _idb_error('AbortError', 'an event listener threw an exception')
                : req.error;
            req.transaction._aborted = true;
        }
    } else {
        var sev = _idb_make_event('success', req);
        _idb_dispatch_active(req.transaction, function() {
            _idb_dispatch(req, sev, _idb_request_path(req));
        });
        // §fire a success event: an exception from a listener aborts the
        // transaction with AbortError — complete never fires.
        if (sev._threw && req.transaction) {
            req.transaction.error = _idb_error('AbortError', 'an event listener threw an exception');
            req.transaction._aborted = true;
        }
    }
}

// --- IDBTransaction (Indexed DB §3.4) ----------------------------------------

function IDBTransaction(db, storeNames, mode, durability) {
    EventTarget.call(this);
    this.db = db;
    this.mode = mode || 'readonly';
    this.durability = durability || 'default';
    this._storeNames = storeNames.slice().sort();
    this.error = null;
    this.oncomplete = null;
    this.onabort = null;
    this.onerror = null;
    this._queue = [];
    this._stores = {};
    this._aborted = false;
    // Indexed DB §3.1.7 state 'active': true from creation and during the
    // dispatch of the transaction's events plus the microtasks its handlers
    // queued; the task following a dispatch finds it inactive
    // (_idb_deactivate_later). Requests created while inactive are refused
    // with TransactionInactiveError.
    this._active = true;
    // Guards the single pending deactivation task (see _idb_deactivate_later).
    this._deactPending = false;
    // Indexed DB §3.4 state 'finished': set SYNCHRONOUSLY by abort() and by the
    // commit inside the flush, long before the terminal event is delivered — a
    // finished transaction accepts no objectStore(), no request and no second
    // abort()/commit().
    this._finished = false;
    // State 'committing': commit() was called explicitly. Requests already in
    // the queue still run; new ones are refused.
    this._committing = false;
    // The terminal event (complete/abort) has been fired. Separate from
    // _finished, which the same transaction reaches one turn earlier.
    this._settled = false;
    this._isUpgrade = false;
    this._snapshot = null;
}
IDBTransaction.prototype = Object.create(EventTarget.prototype);
IDBTransaction.prototype.constructor = IDBTransaction;
Object.defineProperty(IDBTransaction.prototype, 'objectStoreNames', {
    get: function() { return _idb_string_list(this._storeNames); }
});
IDBTransaction.prototype.objectStore = function(name) {
    if (this._finished) throw _idb_error('InvalidStateError', 'transaction has finished');
    if (this._storeNames.indexOf(name) < 0) throw _idb_error('NotFoundError', 'store not in transaction scope');
    if (!this._stores[name]) {
        var sd = this.db._data.stores[name];
        if (!sd) throw _idb_error('NotFoundError', 'no object store named ' + name);
        this._stores[name] = new IDBObjectStore(sd, this);
    }
    return this._stores[name];
};
IDBTransaction.prototype.abort = function() {
    if (this._finished) throw _idb_error('InvalidStateError', 'transaction has already finished');
    this._aborted = true;
    this._finished = true;
    _idb_schedule_txn(this);
};
IDBTransaction.prototype.commit = function() {
    if (this._finished || this._committing) throw _idb_error('InvalidStateError', 'transaction is no longer active');
    this._committing = true;
    _idb_schedule_txn(this);
};

// `abort` bubbles to the connection (BUG-914); `complete` stops at the
// transaction (Indexed DB §3.4).
function _idb_fire_txn(txn, type) {
    _idb_dispatch_active(txn, function() {
        _idb_dispatch(txn, _idb_make_event(type, txn),
            type === 'abort' ? [txn, txn.db] : [txn]);
    });
}

function _idb_schedule_txn(txn) {
    if (_idb_active_txns.indexOf(txn) < 0) _idb_active_txns.push(txn);
    _idb_schedule_flush();
}

function _idb_schedule_flush() {
    // While a flush is running, work queued by a handler must NOT chain another
    // microtask: a microtask queued from a microtask never returns to the event
    // loop, so the page would stay starved even with the per-flush budget below.
    // _lumen_idb_flush() hands whatever is left to a task itself.
    if (_idb_flush_scheduled || _idb_flushing) return;
    _idb_flush_scheduled = true;
    queueMicrotask(_idb_flush_step);
}

// True when this runtime owns the page's event-loop timer queue, i.e. when a
// deferred IndexedDB turn can actually be handed to a task. The service-worker
// scope, which evaluates this same shim, stubs setTimeout to run synchronously
// and has no timer queue at all — there the drain stays unbounded, exactly as
// before, since there is nothing to yield to.
function _idb_has_task_queue() {
    return typeof _lumen_timers !== 'undefined'
        && typeof _lumen_request_wakeup === 'function'
        && typeof _lumen_now_ms === 'function';
}

// False in the production step chain (_idb_flush_step), true while the full
// synchronous drain _lumen_idb_flush() runs — there the open/delete
// settlement stays inline, the way every unit test reads its results.
var _idb_sync_flush = false;

// Takes the transaction's «active» flag away once control returns to the event
// loop (§3.1.7): the deactivation is its own TASK, unshifted to the HEAD of
// _lumen_timers so it runs in front of timers the handler created — a
// setTimeout(fn, 0) from a success handler lands on an inactive transaction,
// while the handler's promise reactions (microtasks, which run before any
// task) still see it active. One pending deactivation per transaction: the
// next dispatch re-arms the flag and the still-pending task turns it off.
// Scopes without a task queue (service worker) fall back to a microtask.
function _idb_deactivate_later(txn) {
    if (txn._deactPending) return;
    txn._deactPending = true;
    var off = function() { txn._deactPending = false; txn._active = false; };
    if (!_idb_has_task_queue()) { queueMicrotask(off); return; }
    var deadline = _lumen_now_ms();
    _lumen_timers.unshift({ id: _lumen_timer_seq++, fn: off, deadline: deadline, interval: null, nesting: 0 });
    _lumen_request_wakeup(deadline);
}

// Wraps one event dispatch with the transaction's active flag: handlers see it
// active and may enqueue requests. After the dispatch the flag survives the
// handler's promise reactions — microtasks run before any task, and the
// deactivation is a task (_idb_deactivate_later) — in the step chain and in
// the synchronous drain alike: a drain that continues in a later task must not
// find the reactions of a handler it ran there facing a dead transaction.
function _idb_dispatch_active(txn, dispatch) {
    if (!txn) return dispatch();
    txn._active = true;
    try { return dispatch(); }
    finally { _idb_deactivate_later(txn); }
}

// Schedules the next IndexedDB turn as an event-loop TASK. The entry goes
// straight into _lumen_timers with nesting 0 instead of through setTimeout, so
// the HTML LS §8.6 4 ms clamp (BUG-271) cannot throttle a long cursor walk to
// 250 steps/s; _lumen_request_wakeup makes the shell wake for it immediately.
function _idb_defer_flush() {
    if (_idb_flush_scheduled || !_idb_has_task_queue()) return;
    _idb_flush_scheduled = true;
    var deadline = _lumen_now_ms();
    _lumen_timers.push({ id: _lumen_timer_seq++, fn: _lumen_idb_flush, deadline: deadline, interval: null, nesting: 0 });
    _lumen_request_wakeup(deadline);
}

// Dispatches up to `budget` of the transaction's requests and returns what is
// left of it. A transaction whose handlers queued more than the budget allows
// stays unfinished and resumes in the next turn — that is what keeps it active
// across turns the way keep_alive expects (Indexed DB §3.1.7 processes each
// request as its own task).
function _idb_flush_txn(txn, budget) {
    if (txn._settled) return budget;
    while (txn._queue.length > 0 && !txn._aborted && budget > 0) {
        budget--;
        _idb_dispatch_request(txn._queue.shift());
    }
    if (!txn._aborted && txn._queue.length > 0) {
        // Every enqueue path (_idb_make_request, cursor.continue/advance,
        // _idb_open_cursor) already re-schedules the transaction, but say it
        // here too so this function alone guarantees the resume.
        _idb_schedule_txn(txn);
        return 0;
    }
    _idb_settle_txn(txn);
    return budget;
}

// Common tail of both flush modes: the transaction accepts no more requests.
// Revert on abort (the abort event bubbles to the connection, BUG-914) or
// commit (complete stops at the transaction, §3.4).
function _idb_settle_txn(txn) {
    txn._finished = true;
    txn._settled = true;
    if (txn._aborted) {
        _idb_revert_txn(txn);
        _idb_abort_txn_requests(txn);
        _idb_fire_txn(txn, 'abort');
    } else {
        // A committed write/versionchange transaction changed the stored data.
        if (txn.mode !== 'readonly') { _idb_dirty = true; _idb_mark_db_dirty(txn.db.name); }
        _idb_fire_txn(txn, 'complete');
    }
}

// The stepwise twin of _idb_flush_txn: ONE request dispatch, or the settlement
// when the queue is empty. After the dispatch the transaction goes back to the
// FRONT of _idb_active_txns — ahead of transactions its handler created, so
// transaction-lifetime-empty still commits tx1 before tx2 — and a transaction
// whose handler queued nothing settles only on the NEXT step: the step runs as
// a microtask, so the promise reactions the handler queued (§3.1.7 keeps the
// transaction active across them) get their requests in first.
function _idb_flush_txn_step(txn) {
    if (txn._settled) return;
    if (txn._queue.length > 0 && !txn._aborted) {
        _idb_dispatch_request(txn._queue.shift());
        _idb_active_txns.unshift(txn);
        return;
    }
    _idb_settle_txn(txn);
}

// Copies the mutable half of an object store so an abort can put it back
// (Indexed DB §3.4.5 «abort a transaction», step 1). The record wrappers are
// cloned, not shared: _write assigns into an existing wrapper's `value`, so a
// snapshot holding the same wrapper would be overwritten along with the store.
// The stored values themselves are shared — a page mutating a stored object in
// place is already outside what the structured clone would have preserved.
function _idb_clone_store(store) {
    var recs = new Array(store.records.length);
    for (var i = 0; i < store.records.length; i++) {
        recs[i] = { key: store.records[i].key, value: store.records[i].value };
    }
    var indexes = {};
    for (var n in store.indexes) {
        var ix = store.indexes[n];
        indexes[n] = { name: ix.name, keyPath: ix.keyPath, unique: ix.unique, multiEntry: ix.multiEntry };
    }
    return { records: recs, indexes: indexes, keyGenerator: store.keyGenerator,
             name: store.name, keyPath: store.keyPath, autoIncrement: store.autoIncrement };
}

// Puts a snapshot back INTO THE SAME store object rather than replacing it in
// the map: every IDBObjectStore/IDBIndex wrapper the page is holding keeps a
// direct reference to it, and a replacement would leave those wrappers writing
// into an object no longer reachable from the database.
function _idb_restore_store(store, snap) {
    store.records = snap.records;
    store.indexes = snap.indexes;
    store.keyGenerator = snap.keyGenerator;
    store.name = snap.name;
    store.keyPath = snap.keyPath;
    store.autoIncrement = snap.autoIncrement;
    // Any createIndex/deleteIndex still queued when the transaction aborted was
    // dropped, unrun, by _idb_abort_txn_requests — its "effective schema" view
    // (BUG-916) must not keep predicting a mutation that will now never land.
    store._pendingIndexOps = [];
}

// Takes the transaction's undo snapshot, once, before its first mutation.
// A versionchange transaction snapshots the whole database (the store map and
// the version change with it); an ordinary one only the stores in its scope.
function _idb_txn_snapshot(txn) {
    if (txn._snapshot || txn.mode === 'readonly') return;
    var data = txn.db._data;
    var stores = {};
    if (txn._isUpgrade) {
        for (var n in data.stores) stores[n] = { ref: data.stores[n], snap: _idb_clone_store(data.stores[n]) };
    } else {
        for (var i = 0; i < txn._storeNames.length; i++) {
            var name = txn._storeNames[i];
            var st = data.stores[name];
            if (st) stores[name] = { ref: st, snap: _idb_clone_store(st) };
        }
    }
    txn._snapshot = { version: data.version, stores: stores, whole: txn._isUpgrade };
}

// Reverts everything the transaction wrote (Indexed DB §3.4.5, step 1). Runs
// before the requests are settled and before the abort event, so a handler
// already reads the reverted database.
function _idb_revert_txn(txn) {
    var snap = txn._snapshot;
    if (!snap) return;
    txn._snapshot = null;
    var data = txn.db._data;
    if (snap.whole) {
        data.version = snap.version;
        txn.db.version = snap.version;
        // A store created by this transaction has to go; one it deleted comes
        // back by its original reference, so the page's wrapper still works.
        for (var name in data.stores) {
            if (!Object.prototype.hasOwnProperty.call(snap.stores, name)) delete data.stores[name];
        }
    }
    for (var n in snap.stores) {
        var entry = snap.stores[n];
        _idb_restore_store(entry.ref, entry.snap);
        data.stores[n] = entry.ref;
    }
    // The reverted state is what has to reach the backend, so a persist is owed
    // exactly as much as a commit would owe one.
    _idb_dirty = true;
    _idb_mark_db_dirty(txn.db.name);
}

// Settles every request still queued when the transaction aborted (Indexed DB
// §3.4.5 «abort a transaction», step 3). Without this a request left in the
// queue keeps readyState 'pending' and its error handler is never called, so a
// page waiting on it waits forever. Deliberately not routed through
// _idb_dispatch_request: the action must not run, and the error event must not
// re-abort a transaction that is already aborting.
function _idb_abort_txn_requests(txn) {
    var queued = txn._queue;
    txn._queue = [];
    for (var i = 0; i < queued.length; i++) {
        var req = queued[i];
        req._action = null;
        req.result = undefined;
        req.error = _idb_error('AbortError', 'transaction was aborted');
        req.readyState = 'done';
        // At the request only: the transaction is already aborting, and this
        // event must not ride up the path to re-abort or re-report it.
        _idb_dispatch(req, _idb_make_event('error', req), [req]);
    }
}

// Creates a request whose `fn` (data read/write) runs at dispatch time, in the
// transaction's request order. Synchronous validation (key range, mode) must be
// done by the caller before calling this, so it can throw to the caller.
function _idb_make_request(source, txn, fn) {
    if (txn._finished || txn._committing || !txn._active) throw _idb_error('TransactionInactiveError', 'transaction is not active');
    // Cheapest correct place for the undo snapshot: every data mutation
    // (add/put/delete/clear, cursor update/delete) is a request, and it is
    // taken once per transaction. A readonly one is skipped inside.
    _idb_txn_snapshot(txn);
    var req = new IDBRequest(source, txn);
    req._action = fn;
    txn._queue.push(req);
    _idb_schedule_txn(txn);
    return req;
}

// --- IDBDatabase (Indexed DB §3.3) -------------------------------------------

function IDBDatabase(data) {
    EventTarget.call(this);
    this._data = data;
    this.name = data.name;
    this.version = data.version;
    this._upgradeTxn = null;
    // «close pending» (Indexed DB §3.3.9): set synchronously by close(), which is
    // what refuses further transaction() calls. The connection itself stays
    // registered until its running transactions have finished — see
    // _idb_conn_open.
    this._closed = false;
    this._txns = [];
    this.onversionchange = null;
    this.onabort = null;
    this.onerror = null;
    this.onclose = null;
}
IDBDatabase.prototype = Object.create(EventTarget.prototype);
IDBDatabase.prototype.constructor = IDBDatabase;

function _idb_fire_db_event(db, type, extra) {
    _idb_dispatch(db, _idb_make_event(type, db, extra), [db]);
}

// A connection blocks an upgrade until close() has been called AND every
// transaction it started has finished (Indexed DB §3.3.9 «close a database
// connection»): running the upgrade under a transaction that is still writing
// would tear the database apart. Prunes settled transactions on the way past, so
// a long-lived connection's list stays the size of its concurrent work.
function _idb_conn_open(db) {
    var live = [];
    for (var i = 0; i < db._txns.length; i++) if (!db._txns[i]._settled) live.push(db._txns[i]);
    db._txns = live;
    return !db._closed || live.length > 0;
}

// Connections of `name` that are still open, excluding `exclude` (the connection
// the requesting open() is itself creating). Fully closed ones are dropped from
// the registry here — this is the only place it is read, so a lazy sweep is
// equivalent to reaping them the moment their last transaction settles.
function _idb_live_connections(name, exclude) {
    var arr = _idb_connections[name];
    if (!arr) return [];
    var kept = [], out = [];
    for (var i = 0; i < arr.length; i++) {
        if (!_idb_conn_open(arr[i])) continue;
        kept.push(arr[i]);
        if (arr[i] !== exclude) out.push(arr[i]);
    }
    if (kept.length > 0) _idb_connections[name] = kept; else delete _idb_connections[name];
    return out;
}

// Indexed DB §3.3.9 «close a database connection». Called by close() and by an
// open request whose version change transaction aborted — that connection is
// never handed to the page, so leaving it registered would block every queued
// upgrade and delete on that name for the lifetime of the document.
function _idb_close_connection(db) {
    db._closed = true;
    // The connection may still be holding a running transaction, in which case
    // _idb_unpark finds it open and the parked requests wait — the end-of-flush
    // sweep in _lumen_idb_flush releases them when it settles.
    _idb_unpark(db.name);
}

function _idb_register_connection(db) {
    var arr = _idb_connections[db.name];
    if (!arr) { arr = []; _idb_connections[db.name] = arr; }
    if (arr.indexOf(db) < 0) arr.push(db);
}

// Parks a blocked open/delete entry and blocks the whole rest of that name's
// queue: §3.3.1 processes a connection queue in order, so a request behind a
// blocked one must not overtake it even when it needs no exclusive access.
function _idb_park_open(name, entry) {
    _idb_parked_names[name] = true;
    _idb_parked_opens.push(entry);
}

// Wakes a name's parked requests once its last connection has closed, putting
// them back at the FRONT of the pending queue in their original order.
// Returns true when it actually released something, so the flush can drain them
// in the same turn.
function _idb_unpark(name) {
    if (!_idb_parked_names[name]) return false;
    if (_idb_live_connections(name, null).length > 0) return false;
    delete _idb_parked_names[name];
    var move = [], keep = [];
    for (var i = 0; i < _idb_parked_opens.length; i++) {
        if (_idb_parked_opens[i].name === name) move.push(_idb_parked_opens[i]);
        else keep.push(_idb_parked_opens[i]);
    }
    _idb_parked_opens = keep;
    if (move.length === 0) return false;
    _idb_pending_opens = move.concat(_idb_pending_opens);
    _idb_schedule_flush();
    return true;
}
Object.defineProperty(IDBDatabase.prototype, 'objectStoreNames', {
    get: function() { return _idb_string_list(Object.keys(this._data.stores).sort()); }
});
IDBDatabase.prototype.createObjectStore = function(name, options) {
    if (!this._upgradeTxn) throw _idb_error('InvalidStateError', 'createObjectStore allowed only during a versionchange transaction');
    // Schema mutations bypass the request queue, so they take the snapshot
    // themselves — an aborted upgrade has to lose its new stores and indexes
    // exactly as it loses its records.
    _idb_txn_snapshot(this._upgradeTxn);
    name = String(name);
    if (this._data.stores[name]) throw _idb_error('ConstraintError', 'object store already exists: ' + name);
    options = options || {};
    var keyPath = (options.keyPath === undefined || options.keyPath === null) ? null : options.keyPath;
    var store = {
        name: name,
        keyPath: keyPath,
        autoIncrement: !!options.autoIncrement,
        keyGenerator: 1,
        records: [],
        indexes: {}
    };
    this._data.stores[name] = store;
    if (this._upgradeTxn._storeNames.indexOf(name) < 0) this._upgradeTxn._storeNames.push(name);
    return new IDBObjectStore(store, this._upgradeTxn);
};
IDBDatabase.prototype.deleteObjectStore = function(name) {
    if (!this._upgradeTxn) throw _idb_error('InvalidStateError', 'deleteObjectStore allowed only during a versionchange transaction');
    if (!this._data.stores[name]) throw _idb_error('NotFoundError', 'no object store named ' + name);
    _idb_txn_snapshot(this._upgradeTxn);
    delete this._data.stores[name];
};
IDBDatabase.prototype.transaction = function(storeNames, mode, options) {
    // WebIDL converts the `mode` argument to the IDBTransactionMode enum before
    // any step of §3.3.4 runs, so an unknown string is a TypeError even for a
    // closed connection; 'versionchange' is a valid enum value and is refused
    // later, by step 5, i.e. AFTER the NotFoundError of an unknown store name.
    mode = (mode === undefined) ? 'readonly' : String(mode);
    if (mode !== 'readonly' && mode !== 'readwrite' && mode !== 'versionchange') {
        throw new TypeError(mode + ' is not a valid value for enumeration IDBTransactionMode');
    }
    var durability = (options && options.durability !== undefined) ? String(options.durability) : 'default';
    if (durability !== 'default' && durability !== 'strict' && durability !== 'relaxed') {
        throw new TypeError(durability + ' is not a valid value for enumeration IDBTransactionDurability');
    }
    if (this._closed) throw _idb_error('InvalidStateError', 'database connection is closed');
    if (typeof storeNames === 'string') storeNames = [storeNames];
    else storeNames = storeNames.slice();
    for (var i = 0; i < storeNames.length; i++) {
        if (!this._data.stores[storeNames[i]]) throw _idb_error('NotFoundError', 'no object store named ' + storeNames[i]);
    }
    if (storeNames.length === 0) throw _idb_error('InvalidAccessError', 'empty store scope');
    if (mode === 'versionchange') throw new TypeError('a versionchange transaction cannot be created with transaction()');
    var txn = new IDBTransaction(this, storeNames, mode, durability);
    // Keeps the connection open for §3.3.9 until this transaction finishes;
    // pruning here bounds the list to the connection's concurrent transactions.
    _idb_conn_open(this);
    this._txns.push(txn);
    // Indexed DB §3.1.7: a transaction is created active and commits as soon as
    // control returns to the event loop with no request of its own left — it
    // does NOT need a request to reach a terminal state. Queueing it here rather
    // than in _idb_make_request is what makes an empty transaction complete, and
    // it preserves creation order between transactions (the flush is a FIFO).
    _idb_schedule_txn(txn);
    return txn;
};
IDBDatabase.prototype.close = function() { _idb_close_connection(this); };

// --- IDBObjectStore (Indexed DB §3.2) ----------------------------------------

function IDBObjectStore(store, txn) {
    this._store = store;
    this.transaction = txn;
    this.name = store.name;
    this.keyPath = store.keyPath;
    this.autoIncrement = store.autoIncrement;
}
Object.defineProperty(IDBObjectStore.prototype, 'indexNames', {
    get: function() { return _idb_string_list(_idb_effective_index_names(this._store)); }
});

// Binary search over the store's key-sorted records array.
function _idb_find_record(records, key) {
    var lo = 0, hi = records.length;
    while (lo < hi) {
        var mid = (lo + hi) >> 1;
        var c = _idb_cmp(records[mid].key, key);
        if (c < 0) lo = mid + 1;
        else if (c > 0) hi = mid;
        else return { found: true, idx: mid };
    }
    return { found: false, idx: lo };
}

// Throws ConstraintError if writing (value, primaryKey) would duplicate a value
// in any unique index (excluding the record currently at primaryKey).
function _idb_check_unique(store, value, primaryKey) {
    for (var name in store.indexes) {
        var idx = store.indexes[name];
        if (!idx.unique) continue;
        var ik = _idb_extract_key(value, idx.keyPath);
        if (ik === undefined) continue;
        var keys = (idx.multiEntry && Array.isArray(ik)) ? ik : [ik];
        for (var ki = 0; ki < keys.length; ki++) {
            for (var r = 0; r < store.records.length; r++) {
                var rec = store.records[r];
                if (_idb_cmp(rec.key, primaryKey) === 0) continue;
                var rik = _idb_extract_key(rec.value, idx.keyPath);
                if (rik === undefined) continue;
                var rkeys = (idx.multiEntry && Array.isArray(rik)) ? rik : [rik];
                for (var rk = 0; rk < rkeys.length; rk++) {
                    if (_idb_is_valid_key(keys[ki]) && _idb_is_valid_key(rkeys[rk]) && _idb_cmp(keys[ki], rkeys[rk]) === 0) {
                        throw _idb_error('ConstraintError', 'unique index ' + name + ' violation');
                    }
                }
            }
        }
    }
}

IDBObjectStore.prototype._write = function(value, key, overwrite) {
    var store = this._store;
    var usedKey;
    if (store.keyPath !== null) {
        if (key !== undefined) throw _idb_error('DataError', 'in-line keys do not take an explicit key argument');
        var k = _idb_extract_key(value, store.keyPath);
        if (k === undefined) {
            if (store.autoIncrement && typeof store.keyPath === 'string') {
                k = store.keyGenerator++;
                _idb_inject_key(value, store.keyPath, k);
            } else {
                throw _idb_error('DataError', 'evaluating the key path yielded no key');
            }
        } else {
            if (!_idb_is_valid_key(k)) throw _idb_error('DataError', 'evaluated key is not a valid key');
            if (store.autoIncrement && typeof k === 'number' && k >= store.keyGenerator) store.keyGenerator = Math.floor(k) + 1;
        }
        usedKey = k;
    } else {
        if (key === undefined) {
            if (store.autoIncrement) { usedKey = store.keyGenerator++; }
            else throw _idb_error('DataError', 'a key is required for an out-of-line store without autoIncrement');
        } else {
            if (!_idb_is_valid_key(key)) throw _idb_error('DataError', 'the supplied key is not a valid key');
            usedKey = key;
            if (store.autoIncrement && typeof key === 'number' && key >= store.keyGenerator) store.keyGenerator = Math.floor(key) + 1;
        }
    }
    var pos = _idb_find_record(store.records, usedKey);
    if (pos.found && !overwrite) throw _idb_error('ConstraintError', 'a record already exists for this key');
    _idb_check_unique(store, value, usedKey);
    if (pos.found) store.records[pos.idx].value = value;
    else store.records.splice(pos.idx, 0, { key: usedKey, value: value });
    return usedKey;
};

IDBObjectStore.prototype.add = function(value, key) {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var self = this;
    return _idb_make_request(this, this.transaction, function() { return self._write(value, key, false); });
};
IDBObjectStore.prototype.put = function(value, key) {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var self = this;
    return _idb_make_request(this, this.transaction, function() { return self._write(value, key, true); });
};
IDBObjectStore.prototype.get = function(query) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        for (var i = 0; i < store.records.length; i++) if (range.includes(store.records[i].key)) return store.records[i].value;
        return undefined;
    });
};
IDBObjectStore.prototype.getKey = function(query) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        for (var i = 0; i < store.records.length; i++) if (range.includes(store.records[i].key)) return store.records[i].key;
        return undefined;
    });
};
IDBObjectStore.prototype.getAll = function(query, count) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var out = [];
        for (var i = 0; i < store.records.length; i++) {
            if (range === null || range.includes(store.records[i].key)) {
                out.push(store.records[i].value);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBObjectStore.prototype.getAllKeys = function(query, count) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var out = [];
        for (var i = 0; i < store.records.length; i++) {
            if (range === null || range.includes(store.records[i].key)) {
                out.push(store.records[i].key);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBObjectStore.prototype.count = function(query) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return store.records.length;
        var n = 0;
        for (var i = 0; i < store.records.length; i++) if (range.includes(store.records[i].key)) n++;
        return n;
    });
};
IDBObjectStore.prototype.delete = function(query) {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var store = this._store, range = _idb_to_range(query);
    if (range === null) throw _idb_error('DataError', 'a key or key range is required');
    return _idb_make_request(this, this.transaction, function() {
        for (var i = store.records.length - 1; i >= 0; i--) if (range.includes(store.records[i].key)) store.records.splice(i, 1);
        return undefined;
    });
};
IDBObjectStore.prototype.clear = function() {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var store = this._store;
    return _idb_make_request(this, this.transaction, function() { store.records = []; return undefined; });
};
// What `name` resolves to on `store` if every createIndex/deleteIndex already
// queued on it but not yet delivered were applied, in call order — undefined
// for "does not exist". Indexed DB §3.2.9/§3.2.10 make createIndex/deleteIndex
// transaction operations: the mutation itself lands at the call's own position
// in the request queue (see createIndex below, BUG-916), but the synchronous
// existence checks those two methods and .index() perform must not have to
// wait for a flush to see a mutation made earlier in the very same script.
function _idb_effective_index(store, name) {
    var idx = store.indexes[name];
    var pending = store._pendingIndexOps;
    if (pending) {
        for (var i = 0; i < pending.length; i++) {
            if (pending[i].name === name) idx = pending[i].idx;
        }
    }
    return idx;
}
// Same overlay as _idb_effective_index, but for the whole name set — backs
// IDBObjectStore.indexNames, which Indexed DB §3.2 requires to reflect a
// createIndex/deleteIndex made earlier in the same script even though the
// underlying mutation is queued for delivery at flush time (BUG-916).
function _idb_effective_index_names(store) {
    var names = Object.keys(store.indexes);
    var pending = store._pendingIndexOps;
    if (pending) {
        for (var i = 0; i < pending.length; i++) {
            var op = pending[i];
            var at = names.indexOf(op.name);
            if (op.idx) {
                if (at < 0) names.push(op.name);
            } else if (at >= 0) {
                names.splice(at, 1);
            }
        }
    }
    return names.sort();
}
IDBObjectStore.prototype.createIndex = function(name, keyPath, options) {
    if (!this.transaction._isUpgrade) throw _idb_error('InvalidStateError', 'createIndex allowed only during a versionchange transaction');
    name = String(name);
    var store = this._store;
    if (_idb_effective_index(store, name)) throw _idb_error('ConstraintError', 'index already exists: ' + name);
    _idb_txn_snapshot(this.transaction);
    options = options || {};
    var idx = { name: name, keyPath: keyPath, unique: !!options.unique, multiEntry: !!options.multiEntry };
    if (!store._pendingIndexOps) store._pendingIndexOps = [];
    var pendingEntry = { name: name, idx: idx };
    store._pendingIndexOps.push(pendingEntry);
    // The index handle exists immediately, but the schema mutation — and with
    // it the point from which add()/put() start enforcing its uniqueness —
    // takes effect at this call's own position in the transaction's request
    // queue, not synchronously: a data request queued before it must still see
    // the old schema, one queued after must see the new one (BUG-916: a later
    // deleteIndex written after a request must not retroactively undo that
    // request's constraint check).
    _idb_make_request(this, this.transaction, function() {
        store.indexes[name] = idx;
        var i = store._pendingIndexOps.indexOf(pendingEntry);
        if (i >= 0) store._pendingIndexOps.splice(i, 1);
        return undefined;
    });
    return new IDBIndex(idx, this);
};
IDBObjectStore.prototype.deleteIndex = function(name) {
    if (!this.transaction._isUpgrade) throw _idb_error('InvalidStateError', 'deleteIndex allowed only during a versionchange transaction');
    var store = this._store;
    if (!_idb_effective_index(store, name)) throw _idb_error('NotFoundError', 'no index named ' + name);
    _idb_txn_snapshot(this.transaction);
    if (!store._pendingIndexOps) store._pendingIndexOps = [];
    var pendingEntry = { name: name, idx: undefined };
    store._pendingIndexOps.push(pendingEntry);
    _idb_make_request(this, this.transaction, function() {
        delete store.indexes[name];
        var i = store._pendingIndexOps.indexOf(pendingEntry);
        if (i >= 0) store._pendingIndexOps.splice(i, 1);
        return undefined;
    });
};
IDBObjectStore.prototype.index = function(name) {
    var idx = _idb_effective_index(this._store, name);
    if (!idx) throw _idb_error('NotFoundError', 'no index named ' + name);
    return new IDBIndex(idx, this);
};
IDBObjectStore.prototype.openCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_store(store, range, dir); }, true, dir);
};
IDBObjectStore.prototype.openKeyCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_store(store, range, dir); }, false, dir);
};

// --- IDBIndex (Indexed DB §3.2.8) --------------------------------------------

function IDBIndex(idx, objectStore) {
    this._index = idx;
    this.objectStore = objectStore;
    this._store = objectStore._store;
    this.transaction = objectStore.transaction;
    this.name = idx.name;
    this.keyPath = idx.keyPath;
    this.unique = idx.unique;
    this.multiEntry = idx.multiEntry;
}
// Materialises an index as a list of { key, primaryKey, value } sorted by
// (index key, primary key). multiEntry array keys are expanded to one entry per
// element. Recomputed per query — simple and correct for an in-memory store.
function _idb_index_entries(store, index) {
    var out = [];
    for (var i = 0; i < store.records.length; i++) {
        var rec = store.records[i];
        var ik = _idb_extract_key(rec.value, index.keyPath);
        if (ik === undefined) continue;
        if (index.multiEntry && Array.isArray(ik)) {
            for (var j = 0; j < ik.length; j++) {
                if (_idb_is_valid_key(ik[j])) out.push({ key: ik[j], primaryKey: rec.key, value: rec.value });
            }
        } else if (_idb_is_valid_key(ik)) {
            out.push({ key: ik, primaryKey: rec.key, value: rec.value });
        }
    }
    out.sort(function(a, b) {
        var c = _idb_cmp(a.key, b.key);
        return c !== 0 ? c : _idb_cmp(a.primaryKey, b.primaryKey);
    });
    return out;
}
IDBIndex.prototype.get = function(query) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        var entries = _idb_index_entries(store, index);
        for (var i = 0; i < entries.length; i++) if (range.includes(entries[i].key)) return entries[i].value;
        return undefined;
    });
};
IDBIndex.prototype.getKey = function(query) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        var entries = _idb_index_entries(store, index);
        for (var i = 0; i < entries.length; i++) if (range.includes(entries[i].key)) return entries[i].primaryKey;
        return undefined;
    });
};
IDBIndex.prototype.getAll = function(query, count) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var entries = _idb_index_entries(store, index);
        var out = [];
        for (var i = 0; i < entries.length; i++) {
            if (range === null || range.includes(entries[i].key)) {
                out.push(entries[i].value);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBIndex.prototype.getAllKeys = function(query, count) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var entries = _idb_index_entries(store, index);
        var out = [];
        for (var i = 0; i < entries.length; i++) {
            if (range === null || range.includes(entries[i].key)) {
                out.push(entries[i].primaryKey);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBIndex.prototype.count = function(query) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var entries = _idb_index_entries(store, index);
        if (range === null) return entries.length;
        var n = 0;
        for (var i = 0; i < entries.length; i++) if (range.includes(entries[i].key)) n++;
        return n;
    });
};
IDBIndex.prototype.openCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, index = this._index, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_index(store, index, range, dir); }, true, dir);
};
IDBIndex.prototype.openKeyCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, index = this._index, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_index(store, index, range, dir); }, false, dir);
};

// --- cursors (Indexed DB §3.2.6) ---------------------------------------------

function _idb_cursor_list_store(store, range, direction) {
    var arr = [];
    for (var i = 0; i < store.records.length; i++) {
        var rec = store.records[i];
        if (range === null || range.includes(rec.key)) arr.push({ key: rec.key, primaryKey: rec.key, value: rec.value });
    }
    if (direction === 'prev' || direction === 'prevunique') arr.reverse();
    return arr;
}
function _idb_cursor_list_index(store, index, range, direction) {
    var entries = _idb_index_entries(store, index);
    var filtered = [];
    for (var i = 0; i < entries.length; i++) if (range === null || range.includes(entries[i].key)) filtered.push(entries[i]);
    if (direction === 'nextunique' || direction === 'prevunique') {
        var dedup = [], lastKey;
        for (var j = 0; j < filtered.length; j++) {
            if (dedup.length === 0 || _idb_cmp(filtered[j].key, lastKey) !== 0) { dedup.push(filtered[j]); lastKey = filtered[j].key; }
        }
        filtered = dedup;
    }
    if (direction === 'prev' || direction === 'prevunique') filtered.reverse();
    return filtered;
}

function IDBCursor(req, source, txn, store, withValue, direction) {
    this._req = req;
    this.source = source;
    this._txn = txn;
    this._store = store;
    this._list = null;       // materialised at first dispatch (deferred)
    this._pos = -1;
    this._withValue = withValue;
    this.direction = direction;
    this.key = undefined;
    this.primaryKey = undefined;
    if (withValue) this.value = undefined;
}
IDBCursor.prototype._step = function() {
    this._pos++;
    if (this._pos >= this._list.length) {
        this.key = undefined; this.primaryKey = undefined;
        if (this._withValue) this.value = undefined;
        this._req.result = null;
        return false;
    }
    var item = this._list[this._pos];
    this.key = item.key;
    this.primaryKey = item.primaryKey;
    if (this._withValue) this.value = item.value;
    this._req.result = this;
    return true;
};
IDBCursor.prototype.continue = function(key) {
    if (key !== undefined && !_idb_is_valid_key(key)) throw _idb_error('DataError', 'invalid cursor key');
    var self = this;
    this._req._action = function() {
        if (key !== undefined) {
            var desc = (self.direction === 'prev' || self.direction === 'prevunique');
            while (self._step()) {
                var c = _idb_cmp(self.key, key);
                if ((!desc && c >= 0) || (desc && c <= 0)) break;
            }
        } else {
            self._step();
        }
        return self._req.result;
    };
    this._txn._queue.push(this._req);
    _idb_schedule_txn(this._txn);
};
IDBCursor.prototype.advance = function(count) {
    count = count >>> 0;
    if (count === 0) throw new TypeError('advance count must be > 0');
    var self = this;
    this._req._action = function() {
        for (var i = 0; i < count; i++) if (!self._step()) break;
        return self._req.result;
    };
    this._txn._queue.push(this._req);
    _idb_schedule_txn(this._txn);
};
IDBCursor.prototype.update = function(value) {
    if (this._txn.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    if (this._pos < 0 || this._pos >= this._list.length) throw _idb_error('InvalidStateError', 'cursor is not positioned on a record');
    var store = this._store, pk = this.primaryKey;
    return _idb_make_request(this.source, this._txn, function() {
        if (store.keyPath !== null) {
            var k = _idb_extract_key(value, store.keyPath);
            if (k === undefined || _idb_cmp(k, pk) !== 0) throw _idb_error('DataError', 'cursor.update must not change the primary key');
        }
        var pos = _idb_find_record(store.records, pk);
        if (!pos.found) throw _idb_error('DataError', 'record no longer exists');
        _idb_check_unique(store, value, pk);
        store.records[pos.idx].value = value;
        return pk;
    });
};
IDBCursor.prototype.delete = function() {
    if (this._txn.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    if (this._pos < 0 || this._pos >= this._list.length) throw _idb_error('InvalidStateError', 'cursor is not positioned on a record');
    var store = this._store, pk = this.primaryKey;
    return _idb_make_request(this.source, this._txn, function() {
        var pos = _idb_find_record(store.records, pk);
        if (pos.found) store.records.splice(pos.idx, 1);
        return undefined;
    });
};

function _idb_open_cursor(source, txn, store, buildList, withValue, direction) {
    if (txn._finished || txn._committing || !txn._active) throw _idb_error('TransactionInactiveError', 'transaction is not active');
    var req = new IDBRequest(source, txn);
    var cursor = new IDBCursor(req, source, txn, store, withValue, direction);
    req._action = function() {
        cursor._list = buildList();
        cursor._step();
        return req.result;
    };
    txn._queue.push(req);
    _idb_schedule_txn(txn);
    return req;
}

// --- open / delete / flush (Indexed DB §3.1) ---------------------------------

// Runs one turn of an open/delete entry against `budget` and returns what is
// left of it. The version change transaction is budgeted exactly like an
// ordinary one (a keep_alive inside onupgradeneeded is what
// upgrade-transaction-deactivation-timing does), except that the entry itself is
// put back at the head of the pending queue: `success` may not fire before the
// version change transaction has committed (Indexed DB §3.3.1).
function _idb_process_open(entry, budget) {
    var req = entry.req;
    var name = entry.name;
    // Everything below runs once, when the connection queue lets this request
    // through — NOT when open()/deleteDatabase() was called. A request that
    // waited behind a delete must see the database as the delete left it, so the
    // version comparison, the VersionError and the connection itself are all
    // resolved here (Indexed DB §3.3.1 «open a database» / «delete a database»).
    if (!entry._started) {
        var existing = _idb_databases[name];
        if (entry._delete) {
            entry.oldVersion = existing ? existing.version : 0;
            entry.newVersion = null;
        } else {
            entry.oldVersion = existing ? existing.version : 0;
            entry.newVersion = (entry.version === undefined)
                ? (existing ? existing.version : 1)
                : entry.version;
            if (existing && entry.newVersion < entry.oldVersion) {
                entry._started = true;
                req.error = _idb_error('VersionError', 'requested version is lower than the existing version');
                _idb_dispatch_request(req);
                return budget - 1;
            }
            entry.upgrade = entry.newVersion > entry.oldVersion;
        }
        // An upgrade and a delete both need exclusive access: tell every other
        // connection to get out of the way, then wait for it to actually do so.
        if (entry.upgrade || entry._delete) {
            if (!entry._vcSent) {
                entry._vcSent = true;
                var others = _idb_live_connections(name, null);
                for (var c = 0; c < others.length; c++) {
                    _idb_fire_db_event(others[c], 'versionchange', { oldVersion: entry.oldVersion, newVersion: entry.newVersion });
                }
            }
            // Re-read: a versionchange handler is allowed to close() inline, and
            // then this request is not blocked at all.
            if (_idb_live_connections(name, null).length > 0) {
                if (!_idb_sync_flush && !entry._blockedTurn) {
                    // `blocked` waits one step: the reactions a versionchange
                    // handler queued (a test's `await wait_for('versionchange')`
                    // continuation, which arms its own watcher for `blocked`)
                    // run in microtasks before the event fires (§3.3.1).
                    entry._blockedTurn = true;
                    _idb_pending_opens.unshift(entry);
                    return budget;
                }
                if (!entry._blockedFired) {
                    entry._blockedFired = true;
                    _idb_fire_blocked(req, entry.oldVersion, entry.newVersion);
                }
                _idb_park_open(name, entry);
                return budget - 1;
            }
        }
        entry._started = true;
        if (entry._delete) {
            delete _idb_databases[name];
        } else {
            var data = _idb_databases[name];
            if (!data) { data = { name: name, version: 0, stores: {} }; _idb_databases[name] = data; }
            entry.data = data;
            entry.db = new IDBDatabase(data);
            req.result = entry.db;
            // Registered before onupgradeneeded runs: a delete queued behind this
            // open must see the connection the upgrade is holding.
            _idb_register_connection(entry.db);
        }
    }
    // A version upgrade (store/index creation, version bump) or a database
    // deletion mutates the persisted snapshot.
    if (entry.upgrade || entry._delete) { _idb_dirty = true; _idb_mark_db_dirty(name); }
    if (entry.upgrade) {
        var txn = entry._txn;
        if (!txn) {
            var data = entry.data, db = entry.db;
            txn = new IDBTransaction(db, Object.keys(data.stores), 'versionchange');
            txn._isUpgrade = true;
            entry._txn = txn;
            db._upgradeTxn = txn;
            // A close() inside onupgradeneeded must not let a queued delete run
            // over an upgrade that is still applying its schema (§3.3.9).
            db._txns.push(txn);
            // Eagerly, unlike an ordinary transaction: the version is bumped on
            // the next line, before any handler can mutate anything, and an
            // aborted upgrade has to give the old version back too.
            _idb_txn_snapshot(txn);
            data.version = entry.newVersion;
            db.version = entry.newVersion;
            req.transaction = txn;
            req.readyState = 'done';
            // The upgrade transaction is the request's parent for this event
            // (Indexed DB §3.5.6), so capture listeners on it and on the
            // connection see `upgradeneeded` before onupgradeneeded runs.
            var uev = _idb_make_event('upgradeneeded', req, { oldVersion: entry.oldVersion, newVersion: entry.newVersion });
            _idb_dispatch_active(txn, function() {
                _idb_dispatch(req, uev, [req, txn, db]);
            });
            // §fire a version change event: an exception from a listener
            // aborts the version change transaction with AbortError; the
            // settlement below then fails the open request with the same
            // error through its regular aborted-transaction branch.
            if (uev._threw) {
                txn.error = _idb_error('AbortError', 'an event listener threw an exception');
                txn._aborted = true;
            }
        }
        if (!entry._settle) {
            while (txn._queue.length > 0 && !txn._aborted && budget > 0) {
                budget--;
                _idb_dispatch_request(txn._queue.shift());
            }
            if (!txn._aborted && txn._queue.length > 0) { _idb_pending_opens.unshift(entry); return 0; }
            if (!_idb_sync_flush) {
                // The commit waits for the NEXT step (§3.1.7: the transaction
                // commits when control returns to the event loop): the reactions
                // the upgradeneeded handler queued — a test's
                // `await wait_for('upgradeneeded')` continuation, which may still
                // create stores — run in microtasks before that step.
                entry._settle = true;
                _idb_pending_opens.unshift(entry);
                return budget;
            }
        }
        txn._finished = true;
        // The version change transaction settles here instead of in
        // _idb_flush_txn; abort() has put it into _idb_active_txns too, so
        // without _settled the flush would fire its terminal event a second time.
        txn._settled = true;
        entry.db._upgradeTxn = null;
        req.transaction = null;
        if (txn._aborted) {
            _idb_revert_txn(txn);
            _idb_abort_txn_requests(txn);
            _idb_fire_txn(txn, 'abort');
            // An aborted version change fails the open request (§3.3.1): without
            // this the request has no error and _idb_dispatch_request fires
            // `success`, handing the page a connection whose upgrade was just
            // rolled back. An error already recorded (a failed upgrade handler)
            // wins — it is the more specific reason.
            if (!req.error) { req.error = _idb_error('AbortError', 'version change transaction was aborted'); }
            req.result = undefined;
            _idb_close_connection(entry.db);
            _idb_dispatch_request(req);
            return budget;
        }
        _idb_fire_txn(txn, 'complete');
        // §3.3.1: a close() during the upgrade (typically from inside
        // onupgradeneeded) still commits the version change — the schema work
        // is done — but the open request fails with AbortError instead of
        // handing the page a connection it asked to go away.
        if (entry.db._closed) {
            req.result = undefined;
            req.error = _idb_error('AbortError', 'the connection was closed during the version change');
            _idb_dispatch_request(req);
            return budget;
        }
    }
    req.readyState = 'done';
    req.error = null;
    // deleteDatabase's success is an IDBVersionChangeEvent (§3.3.1): oldVersion
    // is what was deleted, newVersion is null.
    var ev2 = entry._delete
        ? _idb_make_event('success', req, { oldVersion: entry.oldVersion, newVersion: null })
        : _idb_make_event('success', req);
    _idb_dispatch(req, ev2, _idb_request_path(req));
    return budget;
}

// Delivers pending IndexedDB events, up to _IDB_FLUSH_BUDGET request dispatches
// per call; whatever is left over continues in the next event-loop turn.
// Idempotent: handlers may enqueue further requests (cursor.continue) or
// transactions, and a nested call while a flush is running is a no-op.
//
// This is the FULL SYNCHRONOUS drain: the production scheduler queues the
// stepwise _idb_flush_step instead. The unit tests read their results straight
// out of this call, so here the transaction's active flag drops right after
// each dispatch instead of surviving the handler's microtasks.
function _lumen_idb_flush() {
    _idb_flush_scheduled = false;
    if (_idb_flushing) return;
    _idb_flushing = true;
    _idb_sync_flush = true;
    // With no task queue to yield to there is nothing to gain from stopping
    // early, so the drain keeps its original backstop instead of a budget.
    var budget = _idb_has_task_queue() ? _IDB_FLUSH_BUDGET : 1000000;
    try {
        var woke = true;
        while (woke) {
            woke = false;
            while (budget > 0 && (_idb_pending_opens.length > 0 || _idb_active_txns.length > 0)) {
                if (_idb_pending_opens.length > 0) {
                    var entry = _idb_pending_opens.shift();
                    // A request whose name is already blocked may not overtake the
                    // one that is waiting there, whatever it needs (§3.3.1).
                    if (_idb_parked_names[entry.name]) { _idb_parked_opens.push(entry); continue; }
                    budget = _idb_process_open(entry, budget);
                    continue;
                }
                budget = _idb_flush_txn(_idb_active_txns.shift(), budget);
            }
            // A connection whose close() was deferred by a running transaction is
            // released when that transaction settles, which happens in the drain
            // above and nowhere else — so this is where such a wait ends, and the
            // requests it frees run in this same turn rather than the next one.
            if (budget > 0) {
                var parked = Object.keys(_idb_parked_names);
                for (var p = 0; p < parked.length; p++) if (_idb_unpark(parked[p])) woke = true;
            }
        }
        _idb_persist_if_dirty();
    } finally {
        _idb_flushing = false;
        _idb_sync_flush = false;
    }
    if (_idb_pending_opens.length > 0 || _idb_active_txns.length > 0) _idb_defer_flush();
}

// Units left in the current production chain; refilled when a chain starts,
// spent one per step. When they run out, the chain parks on a task
// (_idb_defer_flush) — the starvation guard of the old single-microtask flush.
var _idb_step_budget = 0;

// One unit of the production flush: an open/delete turn, one request dispatch,
// or one transaction settlement. The next unit continues as a MICROTASK, so
// the promise reactions a handler queued run before the next dispatch — the
// transaction stays active across them (§3.1.7) — and a transaction with no
// further requests settles only on the step after its last dispatch. When the
// budget runs out the rest moves to a task, as _lumen_idb_flush always did.
function _idb_flush_step() {
    _idb_flush_scheduled = false;
    if (_idb_flushing) return;
    _idb_flushing = true;
    try {
        if (_idb_step_budget <= 0) {
            _idb_step_budget = _idb_has_task_queue() ? _IDB_FLUSH_BUDGET : 1000000;
        }
        // A transaction settled by the previous step may release a parked open.
        var parked = Object.keys(_idb_parked_names);
        for (var p = 0; p < parked.length; p++) _idb_unpark(parked[p]);
        if (_idb_pending_opens.length > 0) {
            var entry = _idb_pending_opens.shift();
            // A request whose name is already blocked may not overtake the
            // one that is waiting there, whatever it needs (§3.3.1).
            if (_idb_parked_names[entry.name]) { _idb_parked_opens.push(entry); }
            else _idb_process_open(entry, 1);
            _idb_step_budget--;
        } else if (_idb_active_txns.length > 0) {
            _idb_flush_txn_step(_idb_active_txns.shift());
            _idb_step_budget--;
        }
    } finally {
        _idb_flushing = false;
    }
    if (_idb_pending_opens.length > 0 || _idb_active_txns.length > 0) {
        if (_idb_step_budget > 0) {
            _idb_flush_scheduled = true;
            queueMicrotask(_idb_flush_step);
        } else {
            // The chain parks on a task: persist what has settled so far.
            _idb_persist_if_dirty();
            _idb_defer_flush();
        }
    } else {
        _idb_persist_if_dirty();
    }
}

var indexedDB = {
    open: function(name, version) {
        name = String(name);
        if (version !== undefined) {
            version = Number(version);
            if (!isFinite(version) || version < 1) throw new TypeError('IndexedDB version must be >= 1');
            version = Math.floor(version);
        }
        var req = new IDBOpenDBRequest();
        // Nothing but the argument check happens now: the request joins this
        // name's connection queue and every decision it makes — the version
        // comparison, the connection, the upgrade — is taken when the queue
        // reaches it (§3.3.1), because a request ahead of it may still delete or
        // upgrade the database.
        _idb_pending_opens.push({ req: req, name: name, version: version });
        _idb_schedule_flush();
        return req;
    },
    deleteDatabase: function(name) {
        name = String(name);
        var req = new IDBOpenDBRequest();
        req.result = undefined;
        // Deferred for the same reason as open(), and additionally because the
        // deletion itself may not happen while another connection is open.
        _idb_pending_opens.push({ req: req, name: name, _delete: true });
        _idb_schedule_flush();
        return req;
    },
    databases: function() {
        var out = [];
        for (var name in _idb_databases) out.push({ name: name, version: _idb_databases[name].version });
        return Promise.resolve(out);
    },
    cmp: function(a, b) {
        if (!_idb_is_valid_key(a) || !_idb_is_valid_key(b)) throw _idb_error('DataError', 'invalid key');
        return _idb_cmp(a, b);
    }
};

globalThis.indexedDB        = indexedDB;
globalThis.IDBKeyRange      = IDBKeyRange;
globalThis.IDBRequest       = IDBRequest;
globalThis.IDBOpenDBRequest = IDBOpenDBRequest;
globalThis.IDBDatabase      = IDBDatabase;
globalThis.IDBTransaction   = IDBTransaction;
globalThis.IDBObjectStore   = IDBObjectStore;
globalThis.IDBIndex         = IDBIndex;
globalThis.IDBCursor        = IDBCursor;
globalThis.IDBCursorWithValue = IDBCursor;
__lumen_C._lumen_idb_flush = _lumen_idb_flush;
