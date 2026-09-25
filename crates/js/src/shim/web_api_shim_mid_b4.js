
// ── Web Storage (localStorage / sessionStorage) ───────────────────────────────
// Spec: https://html.spec.whatwg.org/multipage/webstorage.html §8
// Both objects share the same factory; backing native functions differ per type.
//
// BUG-773: `Storage` is a WebIDL *legacy platform object*. Its named-property
// getter/setter/deleter make `storage.foo`, `storage['foo'] = x`,
// `delete storage.foo`, `'foo' in storage` and `Object.keys(storage)` exact
// synonyms of `getItem`/`setItem`/`removeItem`/enumerating the real keys — one
// operation reachable through two syntaxes. This used to be a plain object with
// five own methods, so a property-style write created an ordinary JS property
// on the wrapper: invisible to `getItem`/`length`/`key()`, absent from the
// persistent backend and therefore silently lost on the next page load — two
// unconnected planes of data on one object. The interceptor is a `Proxy`; the
// five operations and `length` live on a real, shared `Storage.prototype`,
// which is also what makes them *shadow* a same-named storage key.

function Storage() { throw new TypeError('Illegal constructor'); }

// proxy → its native accessor set. A WeakMap and not a field on the object
// itself: any own property would be page-visible and — worse — would shadow the
// storage key of the same name (see the visibility rule in the factory below).
var _lumen_storage_impl = new WeakMap();

function _lumen_storage_of(o) {
    var impl = _lumen_storage_impl.get(o);
    if (impl === undefined) throw new TypeError('Illegal invocation');
    return impl;
}

// WebIDL arity check: `localStorage.getItem()` must throw a TypeError rather
// than read the key spelled `undefined` (`missing_arguments.window.js`).
function _lumen_storage_arity(have, want, op) {
    if (have < want) {
        throw new TypeError('Storage.' + op + ': ' + want + ' argument' +
                            (want === 1 ? '' : 's') + ' required, but only ' +
                            have + ' present.');
    }
}

// Operations and `length` are writable + enumerable + configurable on the
// interface prototype, exactly as WebIDL prescribes — plain assignment already
// gives that shape.
Storage.prototype.key = function(n) {
    _lumen_storage_arity(arguments.length, 1, 'key');
    return _lumen_u2n(_lumen_storage_of(this).key(n >>> 0));
};
Storage.prototype.getItem = function(key) {
    _lumen_storage_arity(arguments.length, 1, 'getItem');
    return _lumen_u2n(_lumen_storage_of(this).get(String(key)));
};
Storage.prototype.setItem = function(key, value) {
    _lumen_storage_arity(arguments.length, 2, 'setItem');
    _lumen_storage_of(this).set(String(key), String(value));
};
Storage.prototype.removeItem = function(key) {
    _lumen_storage_arity(arguments.length, 1, 'removeItem');
    _lumen_storage_of(this).remove(String(key));
};
Storage.prototype.clear = function() { _lumen_storage_of(this).clear(); };
Object.defineProperty(Storage.prototype, 'length', {
    get: function() { return _lumen_storage_of(this).len(); },
    enumerable: true,
    configurable: true
});
if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
    Object.defineProperty(Storage.prototype, Symbol.toStringTag, {
        value: 'Storage', writable: false, enumerable: false, configurable: true
    });
}

function _lumen_make_storage(getLen, getKey, getItem, setItem, removeItem, clear) {
    // The object the Proxy wraps carries nothing but the prototype link and any
    // symbol-keyed property a page defines on it — WebIDL routes only *string*
    // names through the named-property hooks.
    var target = Object.create(Storage.prototype);
    var proxy;

    // WebIDL «named property visibility»: `Storage` carries no
    // [LegacyOverrideBuiltIns], so a name already answered by the object or
    // anywhere on its prototype chain hides the storage key of the same name.
    // That is what keeps `storage.length` and `storage.clear` meaning the
    // interface members after `setItem('length', …)`
    // (`storage_functions_not_overwritten.window.js`).
    function visible(prop) {
        return typeof prop === 'string'
            && !Reflect.has(target, prop)
            && getItem(prop) !== undefined;
    }

    proxy = new Proxy(target, {
        get: function(t, prop, receiver) {
            if (visible(prop)) return getItem(prop);
            return Reflect.get(t, prop, receiver);
        },
        set: function(t, prop, value, receiver) {
            // The named property *setter* runs for every string name, shadowed
            // or not — only reads are shadowed. `set.window.js` asserts a
            // same-named setter on the prototype is never invoked.
            if (typeof prop === 'string' && receiver === proxy) {
                setItem(prop, String(value));
                return true;
            }
            return Reflect.set(t, prop, value, receiver);
        },
        has: function(t, prop) {
            if (typeof prop === 'string' && getItem(prop) !== undefined) return true;
            return Reflect.has(t, prop);
        },
        deleteProperty: function(t, prop) {
            if (visible(prop)) { removeItem(prop); return true; }
            return Reflect.deleteProperty(t, prop);
        },
        getOwnPropertyDescriptor: function(t, prop) {
            if (visible(prop)) {
                return { value: getItem(prop), writable: true,
                         enumerable: true, configurable: true };
            }
            return Reflect.getOwnPropertyDescriptor(t, prop);
        },
        defineProperty: function(t, prop, desc) {
            if (typeof prop === 'string') {
                // WebIDL: a named setter accepts a data descriptor only, and
                // routes it into `setItem`. A `configurable: false` request
                // cannot be honoured through a Proxy (the invariant check
                // rejects a non-configurable descriptor for a key that is not a
                // real property of the target) — no spec text or WPT case asks
                // for that combination on `Storage`.
                if ('get' in desc || 'set' in desc) return false;
                if (!('value' in desc) && !('writable' in desc)) return false;
                setItem(prop, String(desc.value));
                return true;
            }
            return Reflect.defineProperty(t, prop, desc);
        },
        ownKeys: function(t) {
            var out = [], n = getLen();
            for (var i = 0; i < n; i++) {
                var k = getKey(i);
                if (k !== undefined) out.push(k);
            }
            // Symbol-keyed own properties must stay in the list or the Proxy
            // invariant check throws for any of them that is non-configurable.
            var own = Reflect.ownKeys(t);
            for (var j = 0; j < own.length; j++) {
                if (out.indexOf(own[j]) === -1) out.push(own[j]);
            }
            return out;
        },
        // WebIDL: a legacy platform object stays extensible and its prototype is
        // immutable.
        preventExtensions: function() { return false; },
        setPrototypeOf: function(t, proto) { return proto === Storage.prototype; }
    });

    _lumen_storage_impl.set(proxy, {
        len: getLen, key: getKey, get: getItem,
        set: setItem, remove: removeItem, clear: clear
    });
    return proxy;
}

var localStorage = _lumen_make_storage(
    _lumen_ls_length, _lumen_ls_key,
    _lumen_ls_get, _lumen_ls_set, _lumen_ls_remove, _lumen_ls_clear
);

var sessionStorage = _lumen_make_storage(
    _lumen_ss_length, _lumen_ss_key,
    _lumen_ss_get, _lumen_ss_set, _lumen_ss_remove, _lumen_ss_clear
);

// ── MutationObserver (WHATWG DOM §4.3.2) ─────────────────────────────────────
// Intercept existing mutation primitives to capture DOM change events.
// Wrapping happens here before the Element API (which calls these primitives)
// is built, so all subsequent setAttribute / innerHTML / appendChild calls
// automatically trigger observer delivery via queueMicrotask.

var _mo_observers = [];
var _mo_delivery_queued = false;

// True if `nid` is `ancestorNid` or a descendant of it (walks the parent chain
// via `_lumen_get_parent`). Scopes `subtree:true` observers to their own subtree
// (DOM §4.3.1) so a mutation elsewhere in the document — e.g. testharness.js's own
// results-table writes — is not misattributed to them (BUG-318).
function _lumen_mo_in_subtree(ancestorNid, nid) {
    var cur = nid;
    while (cur !== undefined && cur !== null) {
        if (cur === ancestorNid) return true;
        cur = _lumen_get_parent(cur);
    }
    return false;
}

// Siblings of `nid` within `parentNid`'s current child list (DOM §4.3.3
// `previousSibling`/`nextSibling`). Call it at the point in the mutation
// primitive where the tree reflects the moment the spec means — after
// insertion (the node's new neighbors) but before removal (the node's old
// ones), since `_lumen_get_children` only sees whatever the tree holds now.
function _lumen_mo_siblings(parentNid, nid) {
    var sibs = _lumen_get_children(parentNid);
    var idx = sibs.indexOf(nid);
    if (idx < 0) return [null, null];
    return [idx > 0 ? sibs[idx - 1] : null, idx + 1 < sibs.length ? sibs[idx + 1] : null];
}

function _mo_notify(nid, type, attrName, oldVal, addedNodeIds, removedNodeIds, prevSibNid, nextSibNid) {
    var hasObs = false;
    for (var oi = 0; oi < _mo_observers.length; oi++) {
        var obs = _mo_observers[oi];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var entry = obs._observations[ei];
            var tnid = entry.target && entry.target.__nid__;
            if (tnid === undefined) continue;
            var opts = entry.opts;
            // DOM §4.3.1: queue a record only if the mutated node is the observed
            // target, or — with subtree:true — a descendant of it. Without the
            // ancestry test, subtree observers captured every document mutation.
            if (opts.subtree) {
                if (!_lumen_mo_in_subtree(tnid, nid)) continue;
            } else if (tnid !== nid) {
                continue;
            }
            if (type === 'attributes' && !opts.attributes) continue;
            if (type === 'childList' && !opts.childList) continue;
            if (type === 'characterData' && !opts.characterData) continue;
            if (type === 'attributes' && opts.attributeFilter &&
                    opts.attributeFilter.indexOf(attrName) < 0) continue;
            var rec = {
                type: type,
                // DOM §4.3.3: target is the mutated node itself — for a subtree
                // observer this is the descendant, not the observation root.
                target: _lumen_make_element(nid),
                attributeName: attrName || null,
                attributeNamespace: null,
                oldValue: (type === 'attributes' && opts.attributeOldValue) ? oldVal :
                          (type === 'characterData' && opts.characterDataOldValue) ? oldVal : null,
                // addedNodes/removedNodes are node ids from the mutation primitives;
                // deliver them as (interned) node wrappers so `record.addedNodes[i]`
                // is `===` the same object scripts see via `firstChild` etc.
                addedNodes: (addedNodeIds || []).map(_lumen_make_element),
                removedNodes: (removedNodeIds || []).map(_lumen_make_element),
                nextSibling: (nextSibNid !== undefined && nextSibNid !== null) ? _lumen_make_element(nextSibNid) : null,
                previousSibling: (prevSibNid !== undefined && prevSibNid !== null) ? _lumen_make_element(prevSibNid) : null,
            };
            // BUG-317: records are MutationRecord instances (DOM §4.3.3).
            Object.setPrototypeOf(rec, MutationRecord.prototype);
            obs._records.push(rec);
            hasObs = true;
        }
    }
    if (hasObs && !_mo_delivery_queued) {
        _mo_delivery_queued = true;
        queueMicrotask(_lumen_flush_mutation_observers);
    }
}

// Synchronous delivery of all pending MutationObserver records.
// Called automatically via queueMicrotask after mutations.
// Can also be called directly by the shell after event dispatch (e.g. after
// _lumen_dispatch) to ensure observer callbacks run before the next paint.
function _lumen_flush_mutation_observers() {
    _mo_delivery_queued = false;
    for (var i = 0; i < _mo_observers.length; i++) {
        var o = _mo_observers[i];
        if (o._records.length === 0) continue;
        var recs = o._records;
        o._records = [];
        try { o._cb(recs, o); } catch(e) { _lumen_report_exception(e); }
    }
}

// BUG-827: nodes the PARSER wrote must queue childList records too. DOM §4.3
// hangs `queue a mutation record` off the insertion step itself, not off the
// API that triggered it, so a node the parser put in the tree owes an observer
// exactly the record `appendChild` owes it. The shell parses the whole document
// before the first script runs, so it replays the insertions it would have made
// here: `pairs` is a flat [parent, child, parent, child, …] list in tree order —
// the order a streaming parser would have inserted them — covering everything
// up to and including the `<script>` that is about to execute.
//
// Called from `crates/shell/src/main.rs` (`flush_parser_inserts`), which skips
// the call entirely while `_lumen_mo_observing()` is false: a record queued
// before anyone called `observe()` is dropped by the spec anyway, and building
// the argument for a whole document is not free.
function _lumen_mo_parser_inserted(pairs) {
    if (_mo_observers.length === 0) return;
    for (var i = 0; i + 1 < pairs.length; i += 2) {
        _mo_notify(pairs[i], 'childList', null, null, [pairs[i + 1]], []);
    }
}

// True once any MutationObserver exists (constructed, not necessarily observing).
// The shell's cheap gate for the call above — see `_lumen_mo_parser_inserted`.
function _lumen_mo_observing() {
    return _mo_observers.length > 0;
}

// Wrap _lumen_set_attr to intercept attribute mutations
var _orig_set_attr = _lumen_set_attr;
_lumen_set_attr = function(nid, name, value) {
    var old = (_mo_observers.length > 0) ? _lumen_get_attr(nid, name) : undefined;
    _orig_set_attr(nid, name, value);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'attributes', String(name), old !== undefined ? old : null, null, null);
    }
    // GAP-SLOT (DOM LS §4.2.2.4): changing `slot` on a light-DOM child moves it
    // between named slots of its host's shadow tree — re-signal the host so
    // `_lumen_fire_slotchange` re-fires for the (possibly two) affected slots.
    if (String(name) === 'slot') {
        var _slot_host = _lumen_u2n(_lumen_get_parent(nid));
        if (_slot_host !== null) { _lumen_fire_slotchange(_slot_host); }
    }
};

// BUG-855: `removeAttribute`/`removeAttributeNS` (`web_api_shim_mid.js`) both
// call `_lumen_remove_attr` directly, past the attribute wrapper above — the
// only intercepted attribute path was *setting* one, so a scripted removal
// queued no `attributes` record at all.
var _orig_remove_attr = _lumen_remove_attr;
_lumen_remove_attr = function(nid, name) {
    var old = (_mo_observers.length > 0) ? _lumen_get_attr(nid, name) : undefined;
    _orig_remove_attr(nid, name);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'attributes', String(name), old !== undefined ? old : null, null, null);
    }
};

// Wrap _lumen_set_inner_html to intercept childList mutations. BUG-368 fixed
// the setter to actually parse+replace children (was a no-op text stub before),
// so this wrapper now reports the real before/after child lists, mirroring
// _lumen_set_text_content's wrapper below.
var _orig_set_inner_html = _lumen_set_inner_html;
_lumen_set_inner_html = function(nid, html) {
    if (_mo_observers.length === 0) { _orig_set_inner_html(nid, html); return; }
    var before = _lumen_get_children(nid);
    _orig_set_inner_html(nid, html);
    var after = _lumen_get_children(nid);
    _mo_notify(nid, 'childList', null, null, after, before);
};

// Wrap _lumen_append_child to intercept childList mutations
var _orig_append_child = _lumen_append_child;
_lumen_append_child = function(parent, child) {
    _orig_append_child(parent, child);
    if (_mo_observers.length > 0) {
        var sib = _lumen_mo_siblings(parent, child);
        _mo_notify(parent, 'childList', null, null, [child], [], sib[0], sib[1]);
    }
};

// Wrap _lumen_remove_child to intercept childList mutations
var _orig_remove_child = _lumen_remove_child;
_lumen_remove_child = function(parent, child) {
    // Neighbors must be read BEFORE removal — once the native call runs,
    // `child` is gone from `parent`'s child list and `_lumen_mo_siblings`
    // can no longer find it.
    var sib = (_mo_observers.length > 0) ? _lumen_mo_siblings(parent, child) : null;
    _orig_remove_child(parent, child);
    if (_mo_observers.length > 0) {
        _mo_notify(parent, 'childList', null, null, [], [child], sib[0], sib[1]);
    }
};

// BUG-855: `insertBefore` never queued a record at all — only `appendChild`
// and `removeChild` were intercepted, so every reference-relative insertion
// (the common form: `parent.insertBefore(node, someChild)`) was silent.
var _orig_insert_before = _lumen_insert_before;
_lumen_insert_before = function(parent, child, reference) {
    _orig_insert_before(parent, child, reference);
    if (_mo_observers.length > 0) {
        var sib = _lumen_mo_siblings(parent, child);
        _mo_notify(parent, 'childList', null, null, [child], [], sib[0], sib[1]);
    }
};

// BUG-855: `Node.replaceChild` (`web_api_shim_mid.js`) is implemented as
// insert-then-remove over the two natives above, so once both queue their
// own record a single `replaceChild` call fired two — insertion queued as
// the child's OWN addition, not as the replacement DOM §4.2.4 "replace"
// describes (one record, both `addedNodes` and `removedNodes`). Re-wrapped
// here, past the two natives, so it builds that one record directly instead.
//
// `_LUMEN_WRAPPER_MEMBERS` is only the *source* object: `web_api_shim_mid.js`
// has already installed its members on the interface prototypes (BUG-1122,
// `_lumen_install_node_members`) before this file runs. Reassigning
// `_LUMEN_WRAPPER_MEMBERS.replaceChild` alone edits a dictionary nothing reads
// again — `Node.prototype.replaceChild` has to be re-installed, or every node
// keeps calling the two-record original.
function _lumen_mo_replace_child(newChild, oldChild) { var nid = this.__nid__;
    if (!newChild || !oldChild || newChild.__nid__ === undefined || oldChild.__nid__ === undefined) {
        throw new TypeError('replaceChild: both arguments must be nodes');
    }
    var sib = (_mo_observers.length > 0) ? _lumen_mo_siblings(nid, oldChild.__nid__) : null;
    _orig_insert_before(nid, newChild.__nid__, oldChild.__nid__);
    _orig_remove_child(nid, oldChild.__nid__);
    _lumen_fire_slotchange(nid);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'childList', null, null, [newChild.__nid__], [oldChild.__nid__], sib[0], sib[1]);
    }
    return oldChild;
}
_LUMEN_WRAPPER_MEMBERS.replaceChild = _lumen_mo_replace_child;
_lumen_install_node_members(Node.prototype, {
    replaceChild: { value: _lumen_mo_replace_child, writable: true, enumerable: true, configurable: true },
});

// Wrap _lumen_set_text_content to intercept mutations. DOM §4.9.1: setting
// textContent on an ELEMENT replaces all its children with (at most) one text
// node — a childList mutation (removedNodes = old children, addedNodes = new
// text node). On a text/CharacterData node it replaces the node's data — a
// characterData mutation (BUG-318).
var _orig_set_text_content = _lumen_set_text_content;
_lumen_set_text_content = function(nid, text) {
    if (_mo_observers.length === 0) { _orig_set_text_content(nid, text); return; }
    if (_lumen_is_text_node(nid) || _lumen_is_comment_node(nid) || _lumen_is_processing_instruction_node(nid)) {
        var old = _lumen_get_text_content(nid);
        _orig_set_text_content(nid, text);
        _mo_notify(nid, 'characterData', null, old, null, null);
    } else {
        var before = _lumen_get_children(nid);
        _orig_set_text_content(nid, text);
        var after = _lumen_get_children(nid);
        _mo_notify(nid, 'childList', null, null, after, before);
    }
};

function MutationObserver(callback) {
    // DOM §4.3.1: the constructor's sole argument is a mandatory callback.
    if (typeof callback !== 'function') {
        throw new TypeError('Failed to construct \'MutationObserver\': parameter 1 is not of type \'Function\'.');
    }
    this._cb = callback;
    this._observations = [];
    this._records = [];
    _mo_observers.push(this);
}
MutationObserver.prototype.observe = function(target, options) {
    var opts = options || {};
    var config = {
        target: target,
        opts: {
            childList:               !!opts.childList,
            attributes:              !!(opts.attributes || opts.attributeFilter || opts.attributeOldValue),
            // characterDataOldValue implies characterData, same as attributeOldValue implies attributes above.
            characterData:           !!(opts.characterData || opts.characterDataOldValue),
            subtree:                 !!opts.subtree,
            attributeOldValue:       !!opts.attributeOldValue,
            characterDataOldValue:   !!opts.characterDataOldValue,
            attributeFilter:         opts.attributeFilter ? opts.attributeFilter.slice() : null,
        },
    };
    // DOM §4.3.1 step 3: at least one of childList/attributes/characterData
    // (after the OldValue/Filter implications just above) must be requested,
    // or observe() is asking to watch nothing.
    if (!config.opts.childList && !config.opts.attributes && !config.opts.characterData) {
        throw new TypeError('The options object must set at least one of \'childList\', \'attributes\', or \'characterData\' to true.');
    }
    if (!target || target.__nid__ === undefined) return;
    // DOM §4.3.1: observe() re-activates the observer. `disconnect()` removes it
    // from `_mo_observers`, so re-observing after a disconnect must re-register it
    // (only the constructor pushed before — BUG-318, WPT MutationObserver-disconnect).
    if (_mo_observers.indexOf(this) < 0) _mo_observers.push(this);
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) {
            this._observations[i] = config;
            return;
        }
    }
    this._observations.push(config);
};
MutationObserver.prototype.disconnect = function() {
    var idx = _mo_observers.indexOf(this);
    if (idx >= 0) _mo_observers.splice(idx, 1);
    this._observations = [];
    this._records = [];
};
MutationObserver.prototype.takeRecords = function() {
    var r = this._records;
    this._records = [];
    return r;
};

// DOM §4.3.3 MutationRecord — interface global so records delivered to a
// MutationObserver callback resolve `record instanceof MutationRecord`
// (BUG-317, same family as BUG-314). Not constructible from script; every
// record built in `_mo_notify` gets `MutationRecord.prototype` as its
// [[Prototype]]. The record literal's own data properties take precedence.
function MutationRecord() { throw new TypeError('Illegal constructor'); }

// ── ResizeObserver (W3C Resize Observer §5) ───────────────────────────────────
// Delivers size-change entries after layout; the shell calls
// _lumen_deliver_resize_observers() after each relayout.
//
// BUG-661 §1: the relayout path is not the only trigger. Resize Observer §3.2
// runs the observation loop as part of the update-the-rendering steps, so an
// observation that has never been reported must reach its callback on the next
// turn even when nothing in the document changed — the shell only relayouts on
// a dirty DOM/style, so a page that calls observe() and then sits still used to
// get no callback at all. _ro_schedule_initial() puts the pass on the event
// loop itself (a task in _lumen_timers, the BUG-842 pattern) so «guaranteed
// first delivery» no longer depends on someone else scheduling a reflow.

var _ro_observers = [];

// True while a first-delivery task is queued (the pass is idempotent, so one
// queued task covers any number of observe() calls made before it runs).
var _ro_initial_scheduled = false;
// Turns spent waiting for the first layout snapshot; see _ro_initial_pass.
var _ro_initial_attempts = 0;
var _RO_INITIAL_MAX_ATTEMPTS = 120;

function ResizeObserver(callback) {
    if (typeof callback !== 'function') {
        throw new TypeError('Failed to construct ResizeObserver: parameter 1 is not of type Function.');
    }
    this._cb = callback;
    this._observations = [];
    _ro_observers.push(this);
}
ResizeObserver.prototype.observe = function(target, options) {
    // Resize Observer §3.1: observe() takes an Element; anything else is a
    // TypeError (BUG-661 §2 — this used to return silently, so the WPT
    // «throw exception when observing non-element» assertion saw no throw).
    if (!target || typeof target !== 'object' || target.__nid__ === undefined || target.nodeType !== 1) {
        throw new TypeError('Failed to execute observe on ResizeObserver: parameter 1 is not of type Element.');
    }
    var box = (options && options.box) ? String(options.box) : 'content-box';
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) {
            // §3.1 step 2: re-observing removes the existing observation and
            // adds a fresh one, so the target is reported again.
            this._observations[i].box = box;
            this._observations[i].lastW = -1;
            this._observations[i].lastH = -1;
            _ro_initial_attempts = 0;
            _ro_schedule_initial();
            return;
        }
    }
    this._observations.push({ target: target, box: box, lastW: -1, lastH: -1 });
    _ro_initial_attempts = 0;
    _ro_schedule_initial();
};
ResizeObserver.prototype.unobserve = function(target) {
    this._observations = this._observations.filter(function(o) { return o.target !== target; });
};
ResizeObserver.prototype.disconnect = function() {
    var idx = _ro_observers.indexOf(this);
    if (idx >= 0) _ro_observers.splice(idx, 1);
    this._observations = [];
};

// Queue the first-delivery pass as an event-loop task. Written straight into
// _lumen_timers with nesting 0 rather than through setTimeout so the §8.6 4 ms
// clamp cannot delay it, and _lumen_request_wakeup makes the parked shell loop
// wake for it immediately.
function _ro_schedule_initial() {
    if (_ro_initial_scheduled) return;
    _ro_initial_scheduled = true;
    var deadline = _lumen_now_ms();
    _lumen_timers.push({ id: _lumen_timer_seq++, fn: _ro_initial_pass, deadline: deadline, interval: null, nesting: 0 });
    _lumen_request_wakeup(deadline);
}

function _ro_has_pending_initial() {
    for (var i = 0; i < _ro_observers.length; i++) {
        var obs = _ro_observers[i];
        for (var j = 0; j < obs._observations.length; j++) {
            if (obs._observations[j].lastW < 0) return true;
        }
    }
    return false;
}

// True once the shell has published a layout snapshot for this document. An
// observe() from a parse-time script runs before the first push, when every
// element reads back «no box» — reporting 0×0 then would be a wrong first
// entry rather than a missing one, so the pass waits instead. Shared with the
// IntersectionObserver first-delivery pass (BUG-807), which waits on the same
// condition for the same reason.
function _lumen_layout_published() {
    try {
        var root = document.documentElement;
        return !!(root && _lumen_get_bounding_rect(root.__nid__));
    } catch (e) {
        return false;
    }
}

function _ro_initial_pass() {
    _ro_initial_scheduled = false;
    if (!_ro_has_pending_initial()) return;
    if (!_lumen_layout_published() && _ro_initial_attempts < _RO_INITIAL_MAX_ATTEMPTS) {
        _ro_initial_attempts++;
        _ro_schedule_initial();
        return;
    }
    _lumen_deliver_resize_observers();
}

// BUG-661 §4: detaching an observed element destroys its box, which is an
// observable size change even when the element is put back at the same size on
// the very same turn (the classic remove() + appendChild() pair no delivery
// pass ever sees in between). Called from the _lumen_remove_child wrapper
// installed below, while the child is still attached, so an observed
// descendant can be found by walking parents.
function _ro_invalidate_detached(childNid) {
    if (_ro_observers.length === 0) return;
    var touched = false;
    for (var i = 0; i < _ro_observers.length; i++) {
        var obs = _ro_observers[i];
        for (var j = 0; j < obs._observations.length; j++) {
            var o = obs._observations[j];
            if (o.lastW < 0) continue;
            var cur = o.target.__nid__;
            while (cur !== null && cur !== undefined) {
                if (cur === childNid) {
                    o.lastW = -1; o.lastH = -1;
                    touched = true;
                    break;
                }
                cur = _lumen_u2n(_lumen_get_parent(cur));
            }
        }
    }
    if (touched) {
        _ro_initial_attempts = 0;
        _ro_schedule_initial();
    }
}

// Wrap the native once, by assignment rather than by a hoisted function
// declaration (which would overwrite the native before the alias is taken and
// recurse). Every removal in the shim — including the implicit one inside a
// reparenting appendChild/insertBefore — goes through this single binding.
var _lumen_remove_child_native = (typeof _lumen_remove_child === 'function') ? _lumen_remove_child : null;
if (_lumen_remove_child_native) {
    _lumen_remove_child = function(parentNid, childNid) {
        _ro_invalidate_detached(childNid);
        return _lumen_remove_child_native(parentNid, childNid);
    };
}

// BUG-661 §3: one length of a computed-style string in CSS px. Border widths
// are always published in px; a padding keeps its specified unit, so px/em/rem
// are resolved here and anything else (%, calc(), viewport units) falls back to
// 0 — the pre-BUG-661 behaviour of not subtracting it at all.
function _ro_len(value, fontPx, rootFontPx) {
    if (!value) return 0;
    var n = parseFloat(value);
    if (!isFinite(n)) return 0;
    if (value.slice(-3) === 'rem') return n * rootFontPx;
    if (value.slice(-2) === 'em') return n * fontPx;
    if (value.slice(-2) === 'px' || String(n) === value) return n;
    return 0;
}

// Content-box geometry of a border box: {w, h} of the content area plus the
// {x, y} offset of its top-left corner inside the border box, which is what
// Resize Observer §5.1 calls the entry's contentRect.
function _ro_content_geometry(nid, borderW, borderH) {
    var fontPx = parseFloat(_lumen_get_computed_style(nid, 'font-size')) || 16;
    var rootFontPx = 16;
    try {
        var root = document.documentElement;
        if (root) rootFontPx = parseFloat(_lumen_get_computed_style(root.__nid__, 'font-size')) || 16;
    } catch (e) { rootFontPx = 16; }
    var bl = _ro_len(_lumen_get_computed_style(nid, 'border-left-width'), fontPx, rootFontPx);
    var br = _ro_len(_lumen_get_computed_style(nid, 'border-right-width'), fontPx, rootFontPx);
    var bt = _ro_len(_lumen_get_computed_style(nid, 'border-top-width'), fontPx, rootFontPx);
    var bb = _ro_len(_lumen_get_computed_style(nid, 'border-bottom-width'), fontPx, rootFontPx);
    var pl = _ro_len(_lumen_get_computed_style(nid, 'padding-left'), fontPx, rootFontPx);
    var pr = _ro_len(_lumen_get_computed_style(nid, 'padding-right'), fontPx, rootFontPx);
    var pt = _ro_len(_lumen_get_computed_style(nid, 'padding-top'), fontPx, rootFontPx);
    var pb = _ro_len(_lumen_get_computed_style(nid, 'padding-bottom'), fontPx, rootFontPx);
    var w = borderW - bl - br - pl - pr;
    var h = borderH - bt - bb - pt - pb;
    return { w: w > 0 ? w : 0, h: h > 0 ? h : 0, x: pl, y: pt };
}

// CSS Contain L2 §4.1 (BUG-852) — deliver the shell's batch of
// `content-visibility: auto` state changes. `changes` is an array of
// `[node_index, skipped]` pairs in tree order, computed inside the shell's
// «update the rendering» step, so this call already *is* the queued task: the
// page's own script cannot be on the stack here.
//
// `_lumen_dispatch` sets no target of its own (BUG-873), and a page watching
// several elements through one listener has nothing else to tell them apart —
// so the target is filled in here, the way `_lumen_details_fire_toggle` does.
function _lumen_deliver_cv_state_changes(changes) {
    if (!changes || changes.length === 0) return;
    for (var i = 0; i < changes.length; i++) {
        var nid = changes[i][0];
        var evt = new ContentVisibilityAutoStateChangeEvent('contentvisibilityautostatechange', {
            bubbles: false, cancelable: false, isTrusted: true, skipped: !!changes[i][1]
        });
        evt.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, evt);
    }
}

// GAP-CSSANIM срез 6 — `getAnimations()` registration for CSS-triggered
// transitions/animations. Срезы 1/2 below already dispatch real lifecycle
// events; срез 5 found that the Web Animations machinery itself (`Animation`/
// `KeyframeEffect`/`_wa_animations`, all in `web_api_shim_tail_b.js`) was
// already complete — the only gap is that `TransitionScheduler`/
// `AnimationScheduler` never register an entry there, so `getAnimations()`
// on an element with a live CSS transition/animation returns `[]`.
//
// Each registered entry is a real `Animation` wrapping an empty
// `KeyframeEffect(target, [], {})` — just enough for `.effect.target`/
// `.playState`/`.id` to answer without throwing. It is never `play()`ed and
// never ticks its own RAF: the visual value is driven natively by the Rust
// scheduler, and letting this shadow object's `_tick` run would overwrite
// `target.style` with its own (empty) keyframe computation on top of that.
// Keyed by `(kind prefix, node index, property/animation name)` so a second
// property transitioning on the same element gets its own entry, matching
// one `CSSTransition`/`CSSAnimation` per (target, property) per spec.
var _lumen_css_anim_registry = {};

function _lumen_css_anim_key(prefix, nid, name) { return prefix + nid + ':' + name; }

// CSS Transitions L1 §3 "creation" happens at the same time as `transitionrun`;
// CSS Animations L1 has no dedicated creation event, so `animationstart` (the
// earliest event this scheduler emits) is used as the approximation.
function _lumen_css_anim_register(prefix, nid, name) {
    var key = _lumen_css_anim_key(prefix, nid, name);
    var anim = _lumen_css_anim_registry[key];
    if (anim) return anim;
    var eff = new KeyframeEffect(_lumen_make_element(nid), [], {});
    anim = new Animation(eff, _wa_doc_timeline);
    anim.id = name;
    anim._state = 'running';
    _lumen_css_anim_registry[key] = anim;
    _wa_animations.push(anim);
    return anim;
}

// `finalState === 'idle'` drops the entry from `_wa_animations` entirely
// (CSS Transitions L1 §3: a completed/canceled transition is discarded);
// any other value keeps it there with that `playState` (CSS Animations L1
// §4.5.1: a finished CSS animation stays in `getAnimations()` until its
// `animation-name` is removed or it is replaced/canceled) and drops only the
// registry key, so a later restart under the same name creates a fresh entry.
function _lumen_css_anim_unregister(prefix, nid, name, finalState) {
    var key = _lumen_css_anim_key(prefix, nid, name);
    var anim = _lumen_css_anim_registry[key];
    if (!anim) return;
    delete _lumen_css_anim_registry[key];
    if (finalState === 'idle') {
        var idx = _wa_animations.indexOf(anim);
        if (idx >= 0) _wa_animations.splice(idx, 1);
    } else {
        anim._state = finalState;
    }
}

// CSS Transitions L1 §3 (GAP-CSSANIM срез 1) — deliver the shell's batch of
// transition lifecycle events. `events` is an array of `[node_index, kind,
// property_name, elapsed_time]` tuples, `kind` one of "run"/"start"/"end"/
// "cancel", computed by `TransitionScheduler::sync`/`tick` inside the shell's
// «update the rendering» step (Step 2, before rAF per spec §8.1.5.1) — same
// queued-task shape as `_lumen_deliver_cv_state_changes` above.
//
// `transitionrun`/`transitionstart`/`transitioncancel` are not cancelable;
// `transitionend` is (CSS Transitions L1 §3, "Firing Transition Events").
var _LUMEN_TRANSITION_EVENT_TYPES = {
    run: 'transitionrun', start: 'transitionstart',
    end: 'transitionend', cancel: 'transitioncancel'
};
function _lumen_deliver_transition_events(events) {
    if (!events || events.length === 0) return;
    for (var i = 0; i < events.length; i++) {
        var nid = events[i][0];
        var kind = events[i][1];
        var type = _LUMEN_TRANSITION_EVENT_TYPES[kind];
        if (!type) continue;
        var propertyName = events[i][2];
        if (kind === 'run') _lumen_css_anim_register('t:', nid, propertyName);
        var evt = new TransitionEvent(type, {
            bubbles: true, cancelable: type === 'transitionend', isTrusted: true,
            propertyName: propertyName, elapsedTime: events[i][3]
        });
        evt.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, evt);
        if (kind === 'end' || kind === 'cancel') _lumen_css_anim_unregister('t:', nid, propertyName, 'idle');
    }
}

// CSS Animations L1 §4.5.1 (GAP-CSSANIM срез 2) — deliver the shell's batch
// of CSS Animations lifecycle events. `events` is an array of `[node_index,
// kind, animation_name, elapsed_time]` tuples, `kind` one of
// "start"/"iteration"/"end"/"cancel", computed by
// `animation_scheduler::AnimationScheduler::tick` — same queued-task shape
// as `_lumen_deliver_transition_events` above.
//
// None of the four `AnimationEvent`s are cancelable (CSS Animations L1
// §4.5.1, "Event dispatch").
var _LUMEN_ANIMATION_EVENT_TYPES = {
    start: 'animationstart', iteration: 'animationiteration',
    end: 'animationend', cancel: 'animationcancel'
};
function _lumen_deliver_animation_events(events) {
    if (!events || events.length === 0) return;
    for (var i = 0; i < events.length; i++) {
        var nid = events[i][0];
        var kind = events[i][1];
        var type = _LUMEN_ANIMATION_EVENT_TYPES[kind];
        if (!type) continue;
        var animationName = events[i][2];
        if (kind === 'start') _lumen_css_anim_register('a:', nid, animationName);
        var evt = new AnimationEvent(type, {
            bubbles: true, cancelable: false, isTrusted: true,
            animationName: animationName, elapsedTime: events[i][3]
        });
        evt.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, evt);
        if (kind === 'end') _lumen_css_anim_unregister('a:', nid, animationName, 'finished');
        else if (kind === 'cancel') _lumen_css_anim_unregister('a:', nid, animationName, 'idle');
    }
}

function _lumen_deliver_resize_observers() {
    if (_ro_observers.length === 0) return;
    var dpr = (typeof devicePixelRatio === 'number' && devicePixelRatio > 0) ? devicePixelRatio : 1;
    for (var oi = 0; oi < _ro_observers.length; oi++) {
        var obs = _ro_observers[oi];
        var entries = [];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var o = obs._observations[ei];
            var nid = o.target.__nid__;
            var rect = _lumen_get_bounding_rect(nid);
            // An element with no box (display:none, detached) has a zero-sized
            // box per §5.1 «calculate box size» — reported once, then it stops
            // differing from lastW/lastH.
            var bw = rect ? rect[2] : 0, bh = rect ? rect[3] : 0;
            // The content geometry costs nine computed-style reads, so a
            // border-box observation only pays for it once it has an entry.
            var cg = o.box === 'border-box' ? null : _ro_content_geometry(nid, bw, bh);
            var w = cg ? cg.w : bw;
            var h = cg ? cg.h : bh;
            if (o.lastW >= 0 && Math.abs(w - o.lastW) < 0.5 && Math.abs(h - o.lastH) < 0.5) continue;
            if (!cg) cg = _ro_content_geometry(nid, bw, bh);
            o.lastW = w; o.lastH = h;
            entries.push({
                target: o.target,
                contentRect: { x: cg.x, y: cg.y, width: cg.w, height: cg.h,
                               top: cg.y, left: cg.x, bottom: cg.y + cg.h, right: cg.x + cg.w },
                borderBoxSize:  [{ inlineSize: bw,   blockSize: bh }],
                contentBoxSize: [{ inlineSize: cg.w, blockSize: cg.h }],
                devicePixelContentBoxSize: [{ inlineSize: Math.round(cg.w * dpr), blockSize: Math.round(cg.h * dpr) }],
            });
        }
        if (entries.length > 0) {
            try { obs._cb(entries, obs); } catch(e) { _lumen_report_exception(e); }
        }
    }
}

// ── Canvas CSS resize tracking ────────────────────────────────────────────────
// When a canvas element's CSS layout dimensions change (detected after each
// relayout), the backing bitmap is scaled to the new size and a `resize` event
// is fired on the element (HTML LS §4.12.4 / Resize Observer integration).
//
// The shell calls _lumen_deliver_canvas_css_resize() after update_layout_rects,
// alongside _lumen_deliver_resize_observers and _lumen_deliver_intersection_observers.

// last CSS dimensions per canvas nid (as a string key), set on first observation.
var _canvas_css_dims = {};

function _lumen_deliver_canvas_css_resize() {
    for (var nid_str in _canvas2d_ctxs) {
        var nid = +nid_str;
        var rect = _lumen_get_bounding_rect(nid);
        if (!rect) continue;
        var w = (rect[2] + 0.5) | 0;  // round to integer CSS px
        var h = (rect[3] + 0.5) | 0;
        if (w < 1) w = 1;
        if (h < 1) h = 1;
        var prev = _canvas_css_dims[nid_str];
        if (!prev) {
            // first observation — record dims without firing event
            _canvas_css_dims[nid_str] = [w, h];
            continue;
        }
        if (prev[0] === w && prev[1] === h) continue;
        // CSS dimensions changed: scale pixel buffer and fire event
        _canvas_css_dims[nid_str] = [w, h];
        _lumen_canvas2d_scale_resize(nid, w, h);
        _lumen_dispatch(nid, new Event('resize'));
    }
}

// ── IntersectionObserver (WICG Intersection Observer §4) ─────────────────────
// Delivers intersection entries after layout; the shell calls
// _lumen_deliver_intersection_observers() after each relayout.
//
// BUG-807: the relayout path is not the only trigger. Intersection Observer
// §3.2 requires observe() itself to queue an initial notification, so the
// callback must arrive on its own shortly after the call, with nothing in the
// document changing. The shell only relayouts on a dirty DOM/style, so a page
// that observed a target and then sat still used to get no callback at all —
// any unrelated mutation elsewhere on the page delivered it instead, which is
// what made the «observe and wait» form hang rather than fail.
// _io_schedule_initial() puts the pass on the event loop itself, the same way
// ResizeObserver does since BUG-661.

var _io_observers = [];

// True while a first-delivery task is queued (the pass is idempotent, so one
// queued task covers any number of observe() calls made before it runs).
var _io_initial_scheduled = false;
// Turns spent waiting for the first layout snapshot; see _io_initial_pass.
var _io_initial_attempts = 0;
var _IO_INITIAL_MAX_ATTEMPTS = 120;

// §2.2 "parse a margin": whitespace-separated, 1–4 tokens, each an absolute
// px length or a percentage, expanded to four sides like the `margin`
// shorthand. Returns the four token strings, or null for a value the
// constructor must reject with a SyntaxError (_io_margin_or_throw).
var _IO_MARGIN_TOKEN = /^([+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)(px|%)$/i;
function _io_parse_margin(str) {
    var s = str === undefined ? '0px' : String(str);
    var parts = s.split(/[ \t\n\f\r]+/).filter(function(p) { return p.length > 0; });
    if (parts.length === 0) parts = ['0px'];
    if (parts.length > 4) return null;
    var toks = [];
    for (var i = 0; i < parts.length; i++) {
        var m = _IO_MARGIN_TOKEN.exec(parts[i]);
        if (!m) return null;
        toks.push(String(Number(m[1])) + (m[2] === '%' ? '%' : 'px'));
    }
    if (toks.length === 1) toks.push(toks[0]);
    if (toks.length === 2) toks.push(toks[0]);
    if (toks.length === 3) toks.push(toks[1]);
    return toks;
}

// §2.2 steps 3–4 (BUG-626): an unparsable margin is a SyntaxError
// DOMException, not a silent fallback to the default.
function _io_margin_or_throw(value, name) {
    var toks = _io_parse_margin(value);
    if (!toks) {
        throw new DOMException("Failed to construct 'IntersectionObserver': "
            + name + " must be specified in pixels or percent.", 'SyntaxError');
    }
    return toks.join(' ');
}

// WebIDL conversion of `threshold`, whose IDL type is
// `(double or sequence<double>)` (BUG-626): an iterable becomes a list, any
// other value one double, and a non-finite entry («foo» → NaN) is a
// TypeError. Runs with the dictionary conversion, i.e. before the
// constructor steps that can throw SyntaxError for a margin.
function _io_convert_thresholds(t) {
    if (t === undefined) return [0];
    var list = (t !== null && typeof t === 'object' && typeof t[Symbol.iterator] === 'function')
        ? Array.from(t) : [t];
    return list.map(function(v) {
        var n = Number(v);
        if (!isFinite(n)) {
            throw new TypeError("Failed to construct 'IntersectionObserver': "
                + 'The provided double value is non-finite.');
        }
        return n;
    });
}

// §2.2 constructor step 5 for `threshold`: a value outside [0, 1] is a
// RangeError (BUG-626); the list is sorted ascending and an empty one
// becomes [0].
function _io_parse_thresholds(list) {
    for (var i = 0; i < list.length; i++) {
        if (list[i] < 0 || list[i] > 1) {
            throw new RangeError("Failed to construct 'IntersectionObserver': "
                + 'Threshold values must be numbers between 0 and 1');
        }
    }
    list = list.slice();
    list.sort(function(a, b) { return a - b; });
    if (list.length === 0) list.push(0);
    return Object.freeze(list);
}

function IntersectionObserver(callback, options) {
    // WebIDL conversions come before any constructor step: the callback must
    // be callable, the dictionary an object (or undefined/null), and `root`
    // an Element, a Document or null.
    if (typeof callback !== 'function') {
        throw new TypeError("Failed to construct 'IntersectionObserver': "
            + "The callback provided as parameter 1 is not a function.");
    }
    if (options !== undefined && options !== null && typeof options !== 'object' && typeof options !== 'function') {
        throw new TypeError("Failed to construct 'IntersectionObserver': "
            + "The provided value is not of type 'IntersectionObserverInit'.");
    }
    this._cb = callback;
    this._options = options || {};
    // A sub-frame's `contentDocument` is a frame_bridge.rs facade that carries
    // `__bid__` but no `nodeType`, so it is accepted by that marker.
    var root = this._options.root;
    if (root != null && !(typeof root === 'object'
            && (root.nodeType === 1 || root.nodeType === 9 || root.__bid__ !== undefined))) {
        throw new TypeError("Failed to construct 'IntersectionObserver': "
            + "The provided value is not of type '(Document or Element)'.");
    }
    var thresholds = _io_convert_thresholds(this._options.threshold);
    this._observations = [];
    // [[QueuedEntries]] (§2.2): filled by the observation update and drained
    // either by the notification step or by takeRecords().
    this._queuedEntries = [];
    this._root = this._options.root == null ? null : this._options.root;
    this._rootMargin = _io_margin_or_throw(this._options.rootMargin, 'rootMargin');
    this._scrollMargin = _io_margin_or_throw(this._options.scrollMargin, 'scrollMargin');
    this._thresholds = _io_parse_thresholds(thresholds);
    _io_observers.push(this);
}
// §2.2 readonly IDL attributes, exposed as prototype accessors like every
// other interface attribute; each returns the value fixed at construction
// (thresholds is a FrozenArray, so the same frozen object every time).
Object.defineProperty(IntersectionObserver.prototype, 'root', {
    get: function() { return this._root; }, enumerable: true, configurable: true });
Object.defineProperty(IntersectionObserver.prototype, 'rootMargin', {
    get: function() { return this._rootMargin; }, enumerable: true, configurable: true });
Object.defineProperty(IntersectionObserver.prototype, 'scrollMargin', {
    get: function() { return this._scrollMargin; }, enumerable: true, configurable: true });
Object.defineProperty(IntersectionObserver.prototype, 'thresholds', {
    get: function() { return this._thresholds; }, enumerable: true, configurable: true });
// §2.2 takeRecords(): return the queued entries and empty the queue, so the
// notification step that follows has nothing left to deliver for them.
IntersectionObserver.prototype.takeRecords = function() {
    var q = this._queuedEntries;
    this._queuedEntries = [];
    return q;
};
IntersectionObserver.prototype.observe = function(target) {
    // §2.2: observe() takes an Element; anything else is a TypeError from the
    // WebIDL conversion (BUG-626 — this used to return silently). The lazy-image
    // observer (_lumen_init_lazy_images) passes a bare {__nid__} proxy with no
    // nodeType, so only a value carrying a non-element nodeType or no __nid__
    // at all is rejected (a frame_bridge.rs element facade has both).
    if (!target || typeof target !== 'object' || target.__nid__ === undefined
            || (target.nodeType !== undefined && target.nodeType !== 1)) {
        throw new TypeError("Failed to execute 'observe' on 'IntersectionObserver': "
            + "parameter 1 is not of type 'Element'.");
    }
    for (var i = 0; i < this._observations.length; i++) {
        // §3.2 step 1: observing an already-observed target is a no-op, so it
        // queues nothing either.
        if (this._observations[i].target === target) return;
    }
    // §3.2.2 IntersectionObserverRegistration: previousThresholdIndex = -1
    // means «never delivered», so the first update always queues an entry.
    // lastRatio mirrors it (-1 until then) for _io_has_pending_initial.
    this._observations.push({ target: target, lastRatio: -1,
                              lastIndex: -1, lastIntersecting: false });
    // disconnect() unregisters the observer; observing again re-arms it.
    if (_io_observers.indexOf(this) < 0) _io_observers.push(this);
    _io_initial_attempts = 0;
    _io_schedule_initial();
};
IntersectionObserver.prototype.unobserve = function(target) {
    this._observations = this._observations.filter(function(o) { return o.target !== target; });
};
IntersectionObserver.prototype.disconnect = function() {
    var idx = _io_observers.indexOf(this);
    if (idx >= 0) _io_observers.splice(idx, 1);
    this._observations = [];
    this._queuedEntries = [];
};

// Queue the first-delivery pass as an event-loop task. Written straight into
// _lumen_timers with nesting 0 rather than through setTimeout so the §8.6 4 ms
// clamp cannot delay it, and _lumen_request_wakeup makes the parked shell loop
// wake for it immediately (the BUG-661/BUG-842 pattern).
function _io_schedule_initial() {
    if (_io_initial_scheduled) return;
    _io_initial_scheduled = true;
    var deadline = _lumen_now_ms();
    _lumen_timers.push({ id: _lumen_timer_seq++, fn: _io_initial_pass, deadline: deadline, interval: null, nesting: 0 });
    _lumen_request_wakeup(deadline);
}

function _io_has_pending_initial() {
    for (var i = 0; i < _io_observers.length; i++) {
        var obs = _io_observers[i];
        for (var j = 0; j < obs._observations.length; j++) {
            if (obs._observations[j].lastRatio < 0) return true;
        }
    }
    return false;
}

function _io_initial_pass() {
    _io_initial_scheduled = false;
    if (!_io_has_pending_initial()) return;
    // Before the first layout snapshot every target reads back «no box», which
    // would deliver a wrong not-intersecting first entry instead of a missing
    // one; the pass waits for the snapshot, bounded by _IO_INITIAL_MAX_ATTEMPTS
    // so a document that never gets one (dump modes) does not re-arm forever.
    if (!_lumen_layout_published() && _io_initial_attempts < _IO_INITIAL_MAX_ATTEMPTS) {
        _io_initial_attempts++;
        _io_schedule_initial();
        return;
    }
    _lumen_deliver_intersection_observers();
}

// Parse CSS margin shorthand into [top, right, bottom, left] px values.
// Only px units are supported; other units resolve to 0.
function _parse_root_margin(str) {
    if (!str) return [0, 0, 0, 0];
    var parts = str.trim().split(/\s+/);
    var vals = parts.map(function(p) {
        return p.indexOf('px') >= 0 ? parseFloat(p) : 0;
    });
    if (vals.length === 1) return [vals[0], vals[0], vals[0], vals[0]];
    if (vals.length === 2) return [vals[0], vals[1], vals[0], vals[1]];
    if (vals.length === 3) return [vals[0], vals[1], vals[2], vals[1]];
    return [vals[0], vals[1], vals[2], vals[3]];
}

// Resolve a serialized rootMargin ("T R B L", each px or %) to px against a
// root of the given size.
function _io_resolve_margin(str, w, h) {
    var parts = str.split(' ');
    var out = [];
    for (var i = 0; i < 4; i++) {
        var p = parts[i];
        var n = parseFloat(p);
        if (p.charAt(p.length - 1) === '%') n = n * ((i === 0 || i === 2) ? h : w) / 100;
        out.push(n);
    }
    return out;
}

// ── Intersection geometry (§2.2 root intersection rectangle, §3.2.7) ────────
// Rects below are [left, top, right, bottom] in the coordinate space
// `_lumen_get_bounding_rect` answers in (BUG-627).

// Expand a rect outward by a serialized margin ("T R B L", px or %); a
// negative component shrinks it. Percentages resolve against the undilated
// rect: height for top/bottom, width for left/right — what the WPT
// `root-margin-root-element.html` expectations encode.
function _io_expand(r, marginStr) {
    var m = _io_resolve_margin(marginStr, r[2] - r[0], r[3] - r[1]);
    return [r[0] - m[3], r[1] - m[0], r[2] + m[1], r[3] + m[2]];
}

// Edge-inclusive intersection (§3.2.10 step «isIntersecting»: touching rects
// still intersect, with zero area). null when the rects are disjoint.
function _io_intersect(a, b) {
    var l = Math.max(a[0], b[0]), t = Math.max(a[1], b[1]);
    var r = Math.min(a[2], b[2]), btm = Math.min(a[3], b[3]);
    if (r < l || btm < t) return null;
    return [l, t, r, btm];
}

function _io_dom_rect(r) {
    var x = r ? r[0] : 0, y = r ? r[1] : 0;
    var w = r ? r[2] - r[0] : 0, h = r ? r[3] - r[1] : 0;
    return { x: x, y: y, width: w, height: h,
             top: y, left: x, bottom: y + h, right: x + w };
}

// §2.2 «content clip»: overflow clips the element's content to its padding
// edge. Every such element (overflow scroll/auto/hidden/clip) is exactly the
// set `_lumen_get_scroll_state` has an entry for (BUG-504 part 7), so this
// needs no computed-style read. <html>/<body> are left out: their overflow
// propagates to the viewport, and the viewport is already the implicit root.
function _io_has_content_clip(nid) {
    if (nid === _io_html_nid || nid === _io_body_nid) return false;
    return !!_lumen_get_scroll_state(nid);
}

function _io_px(nid, prop) {
    var v = parseFloat(_lumen_get_computed_style(nid, prop));
    return isFinite(v) ? v : 0;
}

// Padding box of an element with a content clip: its border box minus the
// border widths. null when the element has no box.
function _io_padding_box(nid) {
    var r = _lumen_get_bounding_rect(nid);
    if (!r) return null;
    return [r[0] + _io_px(nid, 'border-left-width'), r[1] + _io_px(nid, 'border-top-width'),
            r[0] + r[2] - _io_px(nid, 'border-right-width'),
            r[1] + r[3] - _io_px(nid, 'border-bottom-width')];
}

// The clip a content-clipping ancestor applies, grown by scrollMargin. An
// axis whose overflow stays `visible` (only possible opposite `clip`, CSS
// Overflow 3 §3.1) does not clip, so it is left unbounded.
function _io_clip_rect(nid, scrollMargin) {
    var pb = _io_padding_box(nid);
    if (!pb) return null;
    var r = _io_expand(pb, scrollMargin);
    if (_lumen_get_computed_style(nid, 'overflow-x') === 'visible') {
        r[0] = -Infinity; r[2] = Infinity;
    }
    if (_lumen_get_computed_style(nid, 'overflow-y') === 'visible') {
        r[1] = -Infinity; r[3] = Infinity;
    }
    return r;
}

function _io_position(nid) {
    var p = _lumen_get_computed_style(nid, 'position');
    return p || 'static';
}

// The target's containing-block chain (CSS 2 §10.1), nearest first, as a
// list of element nids — the path §3.2.7 walks from target up to the root.
// An absolutely positioned box skips to its nearest positioned ancestor, a
// fixed one leaves the document entirely (its containing block is the
// viewport). Transforms/containment establishing a containing block for
// positioned descendants are not modelled.
//
// Reading `position` flags the computed-style cache as needed for the rest
// of the page's life (BUG-935 S44), so an implicit-root observer whose target
// sits under no clipping ancestor — the lazy-load/analytics common case —
// takes the plain DOM parent chain and never touches computed styles.
function _io_cb_chain(nid, needPositions) {
    var parents = [];
    var p = _lumen_u2n(_lumen_get_parent(nid));
    while (p !== null && p !== undefined) {
        parents.push(p);
        p = _lumen_u2n(_lumen_get_parent(p));
    }
    if (!needPositions) {
        var anyClip = false;
        for (var i = 0; i < parents.length; i++) {
            if (_io_has_content_clip(parents[i])) { anyClip = true; break; }
        }
        if (!anyClip) return parents;
    }
    var chain = [];
    var mode = _io_position(nid);
    for (var j = 0; j < parents.length; j++) {
        if (mode === 'fixed') break;
        var a = parents[j];
        var pos = _io_position(a);
        if (mode === 'absolute' && pos === 'static') continue;
        chain.push(a);
        mode = pos;
    }
    return chain;
}

var _io_html_nid = null, _io_body_nid = null;

// Per-observer part of one update pass: the intersection root and its root
// intersection rectangle (§2.2). `rect` is null when the root has no box.
function _io_root_info(obs, vpW, vpH) {
    var root = obs._root;
    if (root == null || root === document) {
        // Implicit root, or the top-level document: the viewport.
        return { kind: 'document', bid: undefined, nid: null,
                 rect: _io_expand([0, 0, vpW, vpH], obs._rootMargin) };
    }
    if (root.nodeType !== 1) {
        // Another Document — a detached one (createHTMLDocument) or a
        // sub-frame's contentDocument facade. Neither has geometry here.
        return { kind: 'other-document', bid: root.__bid__, nid: null, rect: null };
    }
    if (root.__bid__ !== undefined) {
        // An element of a sub-frame: frame_bridge.rs reports no geometry.
        return { kind: 'element', bid: root.__bid__, nid: root.__nid__, rect: null };
    }
    var nid = root.__nid__;
    var clip = _io_has_content_clip(nid);
    var r = clip ? _io_padding_box(nid) : null;
    if (!clip) {
        var br = _lumen_get_bounding_rect(nid);
        r = br ? [br[0], br[1], br[0] + br[2], br[1] + br[3]] : null;
    }
    // Both rootMargin and scrollMargin apply to a scrollable root.
    if (r) r = _io_expand(r, obs._rootMargin);
    if (r && clip) r = _io_expand(r, obs._scrollMargin);
    return { kind: 'element', bid: undefined, nid: nid, rect: r };
}

// §3.2.10 steps 5–9 for one target: its bounding box, the intersection
// rectangle after every clip on the way to the root (§3.2.7), and whether the
// two intersect at all.
function _io_compute(obs, info, target) {
    var res = { target: null, inter: null, rootBounds: info.rect, hit: false };
    // Step 6: an explicit root in another document never intersects. A
    // sub-frame target has no geometry in this realm either way.
    var sameDoc = info.kind === 'document'
        ? target.__bid__ === undefined
        : info.bid === target.__bid__ && info.kind === 'element';
    if (!sameDoc) {
        res.rootBounds = null;
        return res;
    }
    if (target.__bid__ !== undefined) return res;
    var nid = target.__nid__;
    var br = _lumen_get_bounding_rect(nid);
    if (!br) return res;
    res.target = [br[0], br[1], br[0] + br[2], br[1] + br[3]];
    if (!info.rect) return res;
    var chain = _io_cb_chain(nid, info.kind === 'element');
    var ir = res.target;
    var reached = info.kind === 'document';
    for (var i = 0; i < chain.length && ir; i++) {
        var a = chain[i];
        if (a === info.nid) { reached = true; break; }
        if (!_io_has_content_clip(a)) continue;
        // A scroll container on the way clips the target by its padding box,
        // grown by [[scrollMargin]] («apply scroll margin to a scrollport»).
        var clipRect = _io_clip_rect(a, obs._scrollMargin);
        ir = clipRect ? _io_intersect(ir, clipRect) : null;
    }
    // Step 7: an Element root must be on the target's containing-block chain.
    if (!reached) return res;
    if (ir) ir = _io_intersect(ir, info.rect);
    res.inter = ir;
    res.hit = !!ir;
    return res;
}

function _lumen_deliver_intersection_observers() {
    if (_io_observers.length === 0) return;
    var vp = _lumen_get_viewport_size();
    var vpW = vp[0], vpH = vp[1];
    var de = document.documentElement, body = document.body;
    _io_html_nid = de ? de.__nid__ : null;
    _io_body_nid = body ? body.__nid__ : null;
    for (var oi = 0; oi < _io_observers.length; oi++) {
        var obs = _io_observers[oi];
        var info = _io_root_info(obs, vpW, vpH);
        var thresholds = obs._thresholds;
        var entries = obs._queuedEntries;
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var o = obs._observations[ei];
            // A target with no box (display:none, detached) is reported as an
            // empty box with isIntersecting false — for its first notification
            // (BUG-807) and, per §3.2.10, when a box it had goes away.
            var c = _io_compute(obs, info, o.target);
            var t = c.target, it = c.inter;
            var area = t ? (t[2] - t[0]) * (t[3] - t[1]) : 0;
            var iarea = it ? (it[2] - it[0]) * (it[3] - it[1]) : 0;
            // Step 10: a zero-area target that touches the root counts as
            // fully visible.
            var ratio = area > 0 ? iarea / area : (c.hit ? 1 : 0);
            // Step 11: index of the first threshold above the ratio.
            var idx = 0;
            while (idx < thresholds.length && thresholds[idx] <= ratio) idx++;
            var changed = idx !== o.lastIndex || c.hit !== o.lastIntersecting;
            o.lastIndex = idx;
            o.lastIntersecting = c.hit;
            o.lastRatio = ratio;
            if (!changed) continue;
            entries.push({
                target: o.target,
                isIntersecting: c.hit,
                intersectionRatio: ratio,
                boundingClientRect: _io_dom_rect(t),
                intersectionRect: _io_dom_rect(it),
                rootBounds: _io_dom_rect(c.rootBounds),
                time: typeof performance !== 'undefined' ? performance.now() : 0,
            });
        }
    }
    // §3.2.4 notify: every observer's queue is filled above before any
    // callback runs, and each queue is taken just before its own callback, so
    // a callback that calls another observer's takeRecords() drains entries
    // that observer then never sees delivered a second time.
    var notify = _io_observers.slice();
    for (var ni = 0; ni < notify.length; ni++) {
        var nobs = notify[ni];
        var queue = nobs._queuedEntries;
        nobs._queuedEntries = [];
        if (queue.length === 0) continue;
        try { nobs._cb(queue, nobs); } catch(e) { _lumen_report_exception(e); }
    }
}

// ── TreeWalker / NodeIterator / NodeFilter (DOM LS §4.4–4.5) ─────────────────
// NodeFilter constants (DOM LS §4.3).
var NodeFilter = {
    FILTER_ACCEPT:  1,
    FILTER_REJECT:  2,
    FILTER_SKIP:    3,
    SHOW_ALL:            0xFFFFFFFF,
    SHOW_ELEMENT:        0x1,
    SHOW_TEXT:           0x4,
    SHOW_CDATA_SECTION:  0x8,
    SHOW_PROCESSING_INSTRUCTION: 0x40,
    SHOW_COMMENT:        0x80,
    SHOW_DOCUMENT:       0x100,
    SHOW_DOCUMENT_TYPE:  0x200,
    SHOW_DOCUMENT_FRAGMENT: 0x400,
};

// Wraps an arena nid with the wrapper matching its real node kind — needed
// wherever a traversal result can land on `document` itself (nodeType 9), a
// doctype or a document fragment, not just a plain element/text/comment.
// `_lumen_make_element` alone would mint a bogus Element for all three.
function _lumen_make_node_by_nid(nid) {
    if (nid === _lumen_root_nid) return document;
    if (_lumen_is_doctype(nid)) return _lumen_make_doctype(nid);
    if (_lumen_is_document_fragment(nid)) return _lumen_make_document_fragment(nid);
    return _lumen_make_element(nid);
}

// Returns NodeFilter.FILTER_ACCEPT / SKIP / REJECT for a node nid given
// whatToShow bitmask and an optional filter callback or NodeFilter object.
function _nf_accepts(nid, whatToShow, filter) {
    // whatToShow bitmask check — DOM LS §4.3's full nodeType set, not just
    // element/text/comment/PI: `nid` can be `document` itself (root of a
    // document-rooted walker), a DocumentType or a DocumentFragment.
    var nt;
    if (nid === _lumen_root_nid) { nt = 9; }
    else if (_lumen_is_doctype(nid)) { nt = 10; }
    else if (_lumen_is_document_fragment(nid)) { nt = 11; }
    else if (_lumen_is_text_node(nid)) { nt = _lumen_is_cdata_section(nid) ? 4 : 3; }
    else if (_lumen_is_comment_node(nid)) { nt = 8; }
    else if (_lumen_is_processing_instruction_node(nid)) { nt = 7; }
    else { nt = 1; }
    var bit;
    switch (nt) {
        case 3:  bit = NodeFilter.SHOW_TEXT; break;
        case 4:  bit = NodeFilter.SHOW_CDATA_SECTION; break;
        case 7:  bit = NodeFilter.SHOW_PROCESSING_INSTRUCTION; break;
        case 8:  bit = NodeFilter.SHOW_COMMENT; break;
        case 9:  bit = NodeFilter.SHOW_DOCUMENT; break;
        case 10: bit = NodeFilter.SHOW_DOCUMENT_TYPE; break;
        case 11: bit = NodeFilter.SHOW_DOCUMENT_FRAGMENT; break;
        default: bit = NodeFilter.SHOW_ELEMENT;
    }
    if (!(whatToShow & bit)) return NodeFilter.FILTER_SKIP;
    if (!filter) return NodeFilter.FILTER_ACCEPT;
    var el = _lumen_make_node_by_nid(nid);
    var result;
    if (typeof filter === 'function') {
        try { result = filter(el); } catch(e) { result = NodeFilter.FILTER_REJECT; }
    } else if (filter && typeof filter.acceptNode === 'function') {
        try { result = filter.acceptNode(el); } catch(e) { result = NodeFilter.FILTER_REJECT; }
    } else {
        result = NodeFilter.FILTER_ACCEPT;
    }
    return result;
}

// Collects all nids in subtree of root in document order (pre-order, depth-first).
function _tw_subtree(root_nid) {
    var result = [];
    function visit(n) {
        result.push(n);
        var ch = _lumen_get_children(n);
        for (var i = 0; i < ch.length; i++) visit(ch[i]);
    }
    visit(root_nid);
    return result;
}

// ── TreeWalker (DOM LS §4.5) ─────────────────────────────────────────────────
function _TreeWalker(root, whatToShow, filter) {
    this.root        = root;
    this.whatToShow  = whatToShow;
    this.filter      = filter;
    this.currentNode = root;
}

_TreeWalker.prototype._root_nid = function() {
    return _lumen_tree_nid(this.root);
};

_TreeWalker.prototype._cur_nid = function() {
    return _lumen_tree_nid(this.currentNode);
};

// Returns the parent node within the root subtree, or null.
_TreeWalker.prototype.parentNode = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var p = _lumen_u2n(_lumen_get_parent(cur));
    while (p !== null) {
        if (p === root) { break; }
        var pp = _lumen_u2n(_lumen_get_parent(p));
        if (pp === null) { p = null; break; }
        p = pp;
    }
    if (p === null) return null;
    // Walk from root towards cur; find first ancestor that is accepted
    // Actually per spec: parentNode returns the nearest accepted ancestor in root subtree.
    var candidate = _lumen_u2n(_lumen_get_parent(cur));
    while (candidate !== null && candidate !== root) {
        var r = _nf_accepts(candidate, this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(candidate);
            return this.currentNode;
        }
        candidate = _lumen_u2n(_lumen_get_parent(candidate));
    }
    // Check root itself
    if (root !== null && cur !== root) {
        var rr = _nf_accepts(root, this.whatToShow, this.filter);
        if (rr === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = this.root;
            return this.currentNode;
        }
    }
    return null;
};

// Returns the first child of currentNode that passes the filter.
_TreeWalker.prototype.firstChild = function() {
    var cur = this._cur_nid();
    if (cur === null) return null;
    var children = _lumen_get_children(cur);
    for (var i = 0; i < children.length; i++) {
        var r = _nf_accepts(children[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(children[i]);
            return this.currentNode;
        }
        if (r !== NodeFilter.FILTER_REJECT) {
            // SKIP — recurse into its children (DOM spec §4.5.5)
            var saved = this.currentNode;
            this.currentNode = _lumen_make_node_by_nid(children[i]);
            var found = this.firstChild();
            if (found) return found;
            this.currentNode = saved;
        }
    }
    return null;
};

// Returns the last child of currentNode that passes the filter.
_TreeWalker.prototype.lastChild = function() {
    var cur = this._cur_nid();
    if (cur === null) return null;
    var children = _lumen_get_children(cur);
    for (var i = children.length - 1; i >= 0; i--) {
        var r = _nf_accepts(children[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(children[i]);
            return this.currentNode;
        }
        if (r !== NodeFilter.FILTER_REJECT) {
            var saved = this.currentNode;
            this.currentNode = _lumen_make_node_by_nid(children[i]);
            var found = this.lastChild();
            if (found) return found;
            this.currentNode = saved;
        }
    }
    return null;
};

// Returns the previous sibling (in root subtree) of currentNode.
_TreeWalker.prototype.previousSibling = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var pid = _lumen_u2n(_lumen_get_parent(cur));
    if (pid === null) return null;
    var sibs = _lumen_get_children(pid);
    var idx  = sibs.indexOf(cur);
    for (var i = idx - 1; i >= 0; i--) {
        var r = _nf_accepts(sibs[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(sibs[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the next sibling (in root subtree) of currentNode.
_TreeWalker.prototype.nextSibling = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var pid = _lumen_u2n(_lumen_get_parent(cur));
    if (pid === null) return null;
    var sibs = _lumen_get_children(pid);
    var idx  = sibs.indexOf(cur);
    for (var i = idx + 1; i < sibs.length; i++) {
        var r = _nf_accepts(sibs[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(sibs[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the previous node in document order (depth-first pre-order) that passes filter.
_TreeWalker.prototype.previousNode = function() {
    var root = this._root_nid();
    var cur  = this._cur_nid();
    if (cur === null || cur === root) return null;
    var all = _tw_subtree(root);
    var idx = all.indexOf(cur);
    for (var i = idx - 1; i >= 0; i--) {
        var r = _nf_accepts(all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(all[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the next node in document order (depth-first pre-order) that passes filter.
_TreeWalker.prototype.nextNode = function() {
    var root = this._root_nid();
    var cur  = this._cur_nid();
    if (root === null) return null;
    var all = _tw_subtree(root);
    var idx = cur !== null ? all.indexOf(cur) : -1;
    for (var i = idx + 1; i < all.length; i++) {
        var r = _nf_accepts(all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_node_by_nid(all[i]);
            return this.currentNode;
        }
    }
    return null;
};

// ── NodeIterator (DOM LS §4.4) ───────────────────────────────────────────────
// Simplified: maintains a reference position as an index into the flat subtree.
function _NodeIterator(root, whatToShow, filter) {
    this.root        = root;
    this.whatToShow  = whatToShow;
    this.filter      = filter;
    this._all        = null; // lazily built
    this._pos        = -1;   // -1 = before root
    this.referenceNode = root;
    this.pointerBeforeReferenceNode = true;
}

_NodeIterator.prototype._ensure = function() {
    if (this._all === null) {
        var root_nid = _lumen_tree_nid(this.root);
        this._all = root_nid !== null ? _tw_subtree(root_nid) : [];
    }
};

// Returns the next accepted node (forward traversal).
_NodeIterator.prototype.nextNode = function() {
    this._ensure();
    for (var i = this._pos + 1; i < this._all.length; i++) {
        var r = _nf_accepts(this._all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this._pos = i;
            var el = _lumen_make_node_by_nid(this._all[i]);
            this.referenceNode = el;
            this.pointerBeforeReferenceNode = false;
            return el;
        }
    }
    return null;
};

// Returns the previous accepted node (backward traversal).
_NodeIterator.prototype.previousNode = function() {
    this._ensure();
    for (var i = this._pos - 1; i >= 0; i--) {
        var r = _nf_accepts(this._all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this._pos = i;
            var el = _lumen_make_node_by_nid(this._all[i]);
            this.referenceNode = el;
            this.pointerBeforeReferenceNode = true;
            return el;
        }
    }
    return null;
};

// No-op per DOM LS §4.4.6.
_NodeIterator.prototype.detach = function() {};

// ── CaretPosition (CSSOM View §5.1) ──────────────────────────────────────────
// Returned by document.caretPositionFromPoint(). Phase 0: no layout hit-testing;
// always points to body at offset 0. getClientRects() returns an empty list.
function _CaretPosition(offsetNode, offset) {
    this.offsetNode = offsetNode;
    this.offset     = offset;
}
_CaretPosition.prototype.getClientRects = function() { return new DOMRectList([]); };

// ── window.matchMedia / MediaQueryList (CSS Media Queries L4 §4.2) ───────────
// Pure-JS shim on top of the native binding `_lumen_match_media` (parses + matches
// a media query against an ad-hoc MediaContext). The registry keeps strong refs
// while the user-side MQL is reachable; shell pumps changes via
// `_lumen_deliver_media_changes(w, h, dark, reducedMotion)` after each relayout
// or preference flip.
var _mqlRegistry = [];

function MediaQueryListEvent(type, init) {
    Event.call(this, type, init || {});
    this.media   = (init && init.media)   || '';
    this.matches = !!(init && init.matches);
}
MediaQueryListEvent.prototype = Object.create(Event.prototype);
MediaQueryListEvent.prototype.constructor = MediaQueryListEvent;

function MediaQueryList(media) {
    var vp = (typeof _lumen_get_viewport_size === 'function')
        ? _lumen_get_viewport_size() : [800, 600];
    var raw = String(media == null ? '' : media);
    // Media Queries L4 §Serializing a media query list — `.media` reports the
    // canonical serialization (whitespace collapsed, invalid clauses folded
    // into `not all`), not an echo of the constructor argument.
    this.media       = _lumen_serialize_media_query(raw);
    this.matches     = !!_lumen_match_media(raw, vp[0], vp[1], false, false);
    this.onchange    = null;
    this._listeners  = [];
}
MediaQueryList.prototype.addListener = function(fn) {
    if (typeof fn === 'function') this.addEventListener('change', fn);
};
MediaQueryList.prototype.removeListener = function(fn) {
    if (typeof fn === 'function') this.removeEventListener('change', fn);
};
MediaQueryList.prototype.addEventListener = function(type, fn) {
    if (type === 'change' && typeof fn === 'function') {
        // Spec: ignore duplicate registrations of the same callback.
        for (var i = 0; i < this._listeners.length; i++) {
            if (this._listeners[i] === fn) return;
        }
        this._listeners.push(fn);
    }
};
MediaQueryList.prototype.removeEventListener = function(type, fn) {
    if (type === 'change') {
        var idx = this._listeners.indexOf(fn);
        if (idx !== -1) this._listeners.splice(idx, 1);
    }
};
MediaQueryList.prototype.dispatchEvent = function(ev) {
    if (!ev || ev.type !== 'change') return true;
    for (var i = 0; i < this._listeners.length; i++) {
        try { this._listeners[i].call(this, ev); } catch(e) { _lumen_report_exception(e); }
    }
    if (typeof this.onchange === 'function') {
        try { this.onchange.call(this, ev); } catch(e) { _lumen_report_exception(e); }
    }
    return !ev.defaultPrevented;
};
MediaQueryList.prototype._fire = function(matches) {
    this.matches = matches;
    var ev = new MediaQueryListEvent('change', { media: this.media, matches: matches });
    ev.target = this;
    ev.currentTarget = this;
    this.dispatchEvent(ev);
};

// Shell entry point: re-evaluate every registered MediaQueryList against the
// new context. Fires `change` only when `matches` actually flipped (spec).
function _lumen_deliver_media_changes(w, h, dark, reducedMotion) {
    var darkB = !!dark;
    var rmB   = !!reducedMotion;
    for (var i = 0; i < _mqlRegistry.length; i++) {
        var mql = _mqlRegistry[i];
        if (!mql) continue;
        var newM = !!_lumen_match_media(mql.media, w, h, darkB, rmB);
        if (mql.matches !== newM) mql._fire(newM);
    }
}

// ── postMessage (HTML LS §7.7.4) ─────────────────────────────────────────────
var _message_listeners = [];

// ── Window load / DOMContentLoaded / visibilitychange / error listener arrays ──
var _load_listeners = [];
var _domcontentloaded_win_listeners = [];
var _visibilitychange_listeners = [];
var _error_listeners = [];
var _other_win_listeners = {};
// Window listeners registered with the capture flag. Kept apart from the
// buckets above because those are all bubble/at-target buckets and are read by
// `window.dispatchEvent`, whereas these run in the capture phase of a dispatch
// aimed at a node *below* the window — the first hop of `_lumen_event_path`
// walked backwards (BUG-873). Read from `_lumen_invoke_at_window`.
// Prototype-less: unlike `_lumen_listeners` this one is keyed by a bare event
// type, so a page listening for `constructor`/`toString` would otherwise read a
// `Object.prototype` member back as if it were a listener array.
var _win_capture_listeners = Object.create(null);
// The types `window.addEventListener` files in a dedicated bucket above rather
// than in `_other_win_listeners`. All of them are dispatched at the window
// itself, so a capture registration for one must stay in its own bucket.
var _LUMEN_WIN_TARGETED_EVENTS = {
    popstate: 1, pageshow: 1, pagehide: 1, message: 1,
    load: 1, DOMContentLoaded: 1, visibilitychange: 1, error: 1,
};

var window = {
    history: history,
    onpopstate: null,
    onhashchange: null,
    onmessage: null,
    onpageshow: null,
    onpagehide: null,
    // BUG-834: declared for the same reason as `onscroll` below — `'onunload' in
    // window` / `'onbeforeunload' in window` is the feature test a page runs
    // before deciding whether it may hook the unload sequence. Assignment
    // already worked (`_lumen_bfcache_blocked` and the two dispatch loops in
    // `_lumen_unload_document`/`_lumen_fire_beforeunload` read the property
    // directly), a bare `in` check did not.
    onunload: null,
    onbeforeunload: null,
    onload: null,
    // BUG-702: present so `'onunhandledrejection' in window` is true, which is the
    // other half of the feature test libraries run for promise-rejection support.
    // Dispatched via `_lumen_dispatch_unhandled_rejection` — see BUG-716.
    onunhandledrejection: null,
    onrejectionhandled: null,
    // BUG-822: declared so `'onscroll' in window` / `'onscrollend' in window`
    // answer true — the feature test a page runs before deciding whether it may
    // wait for the end of a scroll. Assignment already worked without them
    // (`dispatchEvent`'s generic branch reads `window['on' + type]` at dispatch
    // time), but a bare `in` check did not; declaring the property is all that
    // branch needs, no dispatch-side change.
    onscroll: null,
    onscrollend: null,
    // `location` is deliberately absent here: it is defined directly on
    // `globalThis` as an unforgeable accessor (see `Location` above), and
    // `window` becomes `globalThis` at the end of this shim, so `window.location`
    // resolves to that accessor. Listing it here would make the window→globalThis
    // copy loop below re-ASSIGN it (`globalThis[k] = d.value`, the plain-value
    // branch), which now runs the navigating setter and would fire a spurious
    // full navigation to the current URL on every page load.
    navigator: navigator,
    alert: alert,
    confirm: confirm,
    prompt: prompt,
    print: print,
    setTimeout: setTimeout,
    setInterval: setInterval,
    clearTimeout: clearTimeout,
    clearInterval: clearInterval,
    requestAnimationFrame: requestAnimationFrame,
    cancelAnimationFrame: cancelAnimationFrame,
    _lumen_run_raf_callbacks: _lumen_run_raf_callbacks,
    EventSource: EventSource,
    WebSocket: WebSocket,
    CloseEvent: CloseEvent,
    MessageEvent: MessageEvent,
    _lumen_pump_websockets: _lumen_pump_websockets,
    _lumen_pump_sse: _lumen_pump_sse,
    caches: caches,
    document: document,
    console: console,
    fetch: fetch,
    Request: Request,
    Response: Response,
    Headers: Headers,
    AbortController: AbortController,
    AbortSignal: AbortSignal,
    ReadableStream: ReadableStream,
    WritableStream: WritableStream,
    TransformStream: TransformStream,
    ReadableStreamDefaultReader: ReadableStreamDefaultReader,
    WritableStreamDefaultWriter: WritableStreamDefaultWriter,
    TextDecoderStream: TextDecoderStream,
    TextEncoderStream: TextEncoderStream,
    CompressionStream: CompressionStream,
    DecompressionStream: DecompressionStream,
    ByteLengthQueuingStrategy: ByteLengthQueuingStrategy,
    CountQueuingStrategy: CountQueuingStrategy,
    FormData: FormData,
    TextEncoder: TextEncoder,
    TextDecoder: TextDecoder,
    localStorage: localStorage,
    sessionStorage: sessionStorage,
    _lumen_dispatch_composition: _lumen_dispatch_composition,
    _lumen_dispatch_mouse_event:        _lumen_dispatch_mouse_event,
    _lumen_dispatch_locked_mousemove:   _lumen_dispatch_locked_mousemove,
    _lumen_dispatch_pointer_event:      _lumen_dispatch_pointer_event,
    _lumen_dispatch_pointer_move_coalesced: _lumen_dispatch_pointer_move_coalesced,
    _lumen_dispatch_capture_event:      _lumen_dispatch_capture_event,
    _lumen_dispatch_key_event:     _lumen_dispatch_key_event,
    _lumen_set_field_value:        _lumen_set_field_value,
    _lumen_dispatch_rich:          _lumen_dispatch_rich,
    _lumen_set_ime_target: _lumen_set_ime_target,
    _lumen_fire_page_lifecycle: _lumen_fire_page_lifecycle,
    addEventListener: function(type, fn, options) {
        if (typeof fn !== 'function') return;
        // A capture listener on the window sees an event on its way DOWN to a
        // node, which is a different bucket from everything below (BUG-873).
        // Only for the generic types: the specially-bucketed ones below are all
        // dispatched AT the window (`load`, `popstate`, …), and DOM §2.9 ignores
        // the capture flag at the target — so routing those away from their
        // bucket would silence them instead of re-ordering them.
        if (_lumen_capture_flag(options) && _LUMEN_WIN_TARGETED_EVENTS[type] !== 1) {
            if (!_win_capture_listeners[type]) _win_capture_listeners[type] = [];
            _win_capture_listeners[type].push(fn);
            return;
        }
        if (type === 'popstate') {
            _popstate_listeners.push(fn);
        } else if (type === 'pageshow') {
            _pageshow_listeners.push(fn);
        } else if (type === 'pagehide') {
            _pagehide_listeners.push(fn);
        } else if (type === 'message') {
            _message_listeners.push(fn);
        } else if (type === 'load') {
            if (_doc_ready_state === 'complete') {
                // already loaded — fire async per spec
                queueMicrotask(function() {
                    try { fn(new Event('load', { bubbles: false })); } catch(e) { _lumen_report_exception(e); }
                });
            } else {
                _load_listeners.push(fn);
            }
        } else if (type === 'DOMContentLoaded') {
            if (_doc_ready_state !== 'loading') {
                queueMicrotask(function() {
                    try { fn(new Event('DOMContentLoaded', { bubbles: true })); } catch(e) { _lumen_report_exception(e); }
                });
            } else {
                _domcontentloaded_win_listeners.push(fn);
            }
        } else if (type === 'visibilitychange') {
            _visibilitychange_listeners.push(fn);
        } else if (type === 'error') {
            _error_listeners.push(fn);
        } else {
            if (!_other_win_listeners[type]) _other_win_listeners[type] = [];
            _other_win_listeners[type].push(fn);
        }
    },
    removeEventListener: function(type, fn, options) {
        var arr;
        if (_lumen_capture_flag(options) && _LUMEN_WIN_TARGETED_EVENTS[type] !== 1) arr = _win_capture_listeners[type];
        else if (type === 'popstate') arr = _popstate_listeners;
        else if (type === 'pageshow') arr = _pageshow_listeners;
        else if (type === 'pagehide') arr = _pagehide_listeners;
        else if (type === 'message') arr = _message_listeners;
        else if (type === 'load') arr = _load_listeners;
        else if (type === 'DOMContentLoaded') arr = _domcontentloaded_win_listeners;
        else if (type === 'visibilitychange') arr = _visibilitychange_listeners;
        else if (type === 'error') arr = _error_listeners;
        else arr = _other_win_listeners[type];
        if (!arr) return;
        var idx = arr.indexOf(fn);
        if (idx >= 0) arr.splice(idx, 1);
    },
    dispatchEvent: function(evt) {
        if (!evt || !evt.type) return true;
        var arr;
        if (evt.type === 'load') {
            arr = _load_listeners.slice();
            for (var i = 0; i < arr.length; i++) {
                try { arr[i].call(window, evt); } catch(e) { _lumen_report_exception(e); }
            }
            if (typeof window.onload === 'function') {
                try { window.onload.call(window, evt); } catch(e) { _lumen_report_exception(e); }
            }
        } else if (evt.type === 'error') {
            // Deliberately NOT routed through `_lumen_report_exception` here: this
            // branch runs *inside* that function's own dispatch (`_lumen_report_exception`
            // -> `window.dispatchEvent(new ErrorEvent(...))` -> here), so reporting
            // an exception thrown by an 'error' listener itself would recurse.
            arr = _error_listeners.slice();
            for (var i = 0; i < arr.length; i++) { try { arr[i].call(window, evt); } catch(e) {} }
            if (typeof window.onerror === 'function') {
                // BUG-591: `onerror`'s IDL type is OnErrorEventHandler, not the
                // plain EventHandler every other on<type> attribute uses -- its
                // "internal raw handler" is called with 5 positional arguments
                // (message, source, lineno, colno, error) instead of the Event
                // object, but only when the event genuinely is an ErrorEvent;
                // `window.dispatchEvent(new Event('error'))` still passes the
                // Event itself (single argument) to the same handler.
                var isErrorEvt = (evt instanceof ErrorEvent);
                var rv;
                try {
                    rv = isErrorEvt
                        ? window.onerror.call(window, evt.message, evt.filename, evt.lineno, evt.colno, evt.error)
                        : window.onerror.call(window, evt);
                } catch (e) { rv = undefined; }
                // Returning a truthy value from the ErrorEvent-flavoured call
                // cancels the event's default action (HTML LS "the event
                // handler processing algorithm", error-event special case).
                if (isErrorEvt && rv) { evt.preventDefault(); }
            }
        } else {
            arr = _other_win_listeners[evt.type];
            if (arr) {
                arr = arr.slice();
                for (var i = 0; i < arr.length; i++) { try { arr[i].call(window, evt); } catch(e) { _lumen_report_exception(e); } }
            }
            // BUG-392: the `on<type>` IDL attribute fires after the explicit
            // listeners, same ordering as the 'load'/'error' branches above and
            // as `_lumen_dispatch` does for elements. Generic by design: every
            // Window event handler attribute declared as a plain nullable
            // property (`onpopstate`, `ongamepadconnected`, …) is reached this
            // way, so a new one needs no dispatch-side change. No double-fire:
            // `load`/`error` are handled by the branches above, and the engine's
            // own delivery of `hashchange`/`popstate`/`message` calls the
            // handler directly instead of going through `dispatchEvent`.
            var onFn = window['on' + evt.type];
            if (typeof onFn === 'function') { try { onFn.call(window, evt); } catch(e) { _lumen_report_exception(e); } }
        }
        return !evt.defaultPrevented;
    },
    /// postMessage (HTML LS §7.7.4): dispatch a MessageEvent to this window.
    /// Two call shapes: legacy `(message, targetOrigin, transfer)` and the
    /// current `(message, options)` where `options.targetOrigin` defaults to
    /// '/'. `targetOrigin` '*' → always deliver; '/' → same-origin only;
    /// any other string is parsed as an absolute URL and compared by origin
    /// (a parse failure throws `SyntaxError`, per spec — a mismatch just
    /// silently drops the message, it is not an error). `message` is
    /// structured-cloned, not passed by reference (BUG-717).
    postMessage: function(message, targetOrigin) {
        if (targetOrigin !== null && typeof targetOrigin === 'object') {
            targetOrigin = ('targetOrigin' in targetOrigin) ? targetOrigin.targetOrigin : '/';
        } else if (targetOrigin === undefined) {
            targetOrigin = '/';
        }
        var origin = location.origin;
        if (targetOrigin !== '*') {
            var target;
            if (targetOrigin === '/') {
                target = origin;
            } else {
                // `_lumen_parse_url` (url_parse_shim.js) splits an authority
                // on its first '/'/'?'/'#' without validating what is inside
                // it — no forbidden-host-code-point check like the URL
                // Standard's host parser has, so `new URL('http://foo bar')`
                // would build an origin instead of throwing. Reject those
                // code points here so the SyntaxError WPT expects still
                // fires; a real domain never contains them.
                var parsedTarget;
                try { parsedTarget = new URL(String(targetOrigin)); }
                catch (e) { parsedTarget = null; }
                if (parsedTarget === null || /[\x00-\x20#\/:<>?@\[\\\]^|]/.test(parsedTarget.hostname)) {
                    throw new DOMException(
                        "Failed to execute 'postMessage' on 'Window': Invalid target origin '" +
                        targetOrigin + "' in a call to 'postMessage'.", 'SyntaxError');
                }
                target = parsedTarget.origin;
            }
            if (target !== origin) return;
        }
        var ev = new MessageEvent(structuredClone(message));
        ev.origin = origin;
        ev.source = window;
        // Spec §7.7.4 step 5: dispatch as a task (asynchronously).
        setTimeout(function() {
            if (typeof window.onmessage === 'function') {
                try { window.onmessage(ev); } catch(e) { _lumen_report_exception(e); }
            }
            for (var i = 0; i < _message_listeners.length; i++) {
                try { _message_listeners[i](ev); } catch(e) { _lumen_report_exception(e); }
            }
        }, 0);
    },
};

// BUG-874 (second half): `Window includes GlobalEventHandlers` (HTML LS
// §8.1.7.1) — same gap as `document` above, on the same curated list. The
// generic branch of `window.dispatchEvent` (see the `onFn = window['on' +
// evt.type]` read a little above) already needs no dispatch-side change for a
// new entry here, per its own comment; only the bare `'onX' in window` idiom
// was missing a declared property to answer `true`. `hasOwnProperty` skips
// the handlers already declared on the literal above with bespoke dispatch
// (`onload`, `onscroll`, …) — same `null` value, so nothing behavioural
// changes for them.
for (var _wohi = 0; _wohi < _LUMEN_EVENT_HANDLER_ATTRS.length; _wohi++) {
    var _wohAttr = _LUMEN_EVENT_HANDLER_ATTRS[_wohi];
    if (!Object.prototype.hasOwnProperty.call(window, _wohAttr)) window[_wohAttr] = null;
}

// BUG-480 срез 4: доставка кросс-фреймового message в ЭТО окно из бриджа
// фреймов (frame_bridge::_lumen_frame_pump_messages). Данные уже разобраны,
// source — фасад окна отправителя или null. Тот же порядок, что у локального
// window.postMessage выше: сначала onmessage, затем addEventListener('message').
globalThis._lumen_deliver_frame_message = function(data, origin, source) {
    var ev = new MessageEvent(data);
    ev.origin = origin || '';
    if (source !== null && source !== undefined) ev.source = source;
    if (typeof window.onmessage === 'function') {
        try { window.onmessage(ev); } catch(e) {}
    }
    for (var i = 0; i < _message_listeners.length; i++) {
        try { _message_listeners[i](ev); } catch(e) {}
    }
};

// BUG-480 срез 6: синтетический click() из родительского фасада iframe
// (frame_bridge::_lumen_frame_pump_messages вызывает на тике ЭТОГО контекста).
// Исполняется в этом изоляте, поэтому событие достаётся слушателям этого
// документа; сама последовательность — та же бездоверительная семантика
// click(), что у HTMLElement.prototype.click (общая _lumen_perform_click,
// объявление поднимается хостингом в пределах одного скрипта шима).
globalThis._lumen_deliver_frame_click = function(nid) {
    if (typeof nid !== 'number' || nid < 0) return;
    _lumen_perform_click(nid);
};

// BUG-480 срез 7: focus() из чужого фасада iframe — семантика
// HTMLElement.prototype.focus, исполненная В ЭТОМ изоляте: focusability-гейт и
// _lumen_focus_update (blur/focusout на прежде сфокусированном, focus/focusin
// на новом). Два отклонения, оба задокументированы в BUG-480: (1) БЕЗ
// `_lumen_request_focus` — очередь фокус-запросов рантайма фрейма шеллом пока
// не дренируется (фреймы не рендерятся), запрос там только копился бы;
// `preventScroll` переносится конвертом, но игнорируется — layout у фреймов
// нулевой, скроллить нечего.
globalThis._lumen_deliver_frame_focus = function(nid, preventScroll) {
    if (typeof nid !== 'number' || nid < 0) return;
    if (!_lumen_is_focusable(nid)) return;
    _lumen_focus_update(nid);
};
// Парный blur(): no-op для не сфокусированного элемента, как у
// HTMLElement.prototype.blur; тоже без `_lumen_request_blur`.
globalThis._lumen_deliver_frame_blur = function(nid) {
    if (typeof nid !== 'number' || nid < 0) return;
    if (_lumen_last_focused_nid !== _lumen_nearest_element_nid(nid)) return;
    _lumen_focus_update(-1);
};
// Срез 7: произвольное событие из чужого фасада dispatchEvent(). Точная копия
// последовательности собственного el.dispatchEvent этого шима (см. фабрику
// живых элементов): снимок Event строится заново в этом изоляте, диспатчится
// через _lumen_dispatch (слушатели цели + on<type>), а недоверенный 'click'
// без preventDefault запускает активационное поведение (BUG-439).
globalThis._lumen_deliver_frame_dom_event = function(nid, env) {
    if (typeof nid !== 'number' || nid < 0 || !env) return;
    var type = typeof env.type === 'string' ? env.type : '';
    if (!type) return;
    var init = { bubbles: !!env.bubbles, cancelable: !!env.cancelable };
    var ev = new Event(type, init);
    if (env.detail !== null && env.detail !== undefined && typeof CustomEvent === 'function') {
        ev = new CustomEvent(type, { bubbles: !!env.bubbles, cancelable: !!env.cancelable, detail: env.detail });
    }
    ev.target = _lumen_make_element(nid);
    ev.currentTarget = ev.target;
    var notCancelled = _lumen_dispatch(nid, ev);
    if (notCancelled && ev.isTrusted === false && type === 'click') {
        var at = _lumen_activation_target(nid);
        if (at !== -1) {
            _lumen_run_activation_behavior(at, (at === nid)
                ? _lumen_make_element(nid) : _lumen_make_element(at));
        }
    }
};

// BUG-480 срез 8: `<script>`, вставленный в под-документ из чужого фасада
// (appendChild/insertBefore через contentDocument). Мост ставит конверт
// RunScript, этот хук на тике ЭТОГО контекста исполняет элемент штатной
// `_lumen_script_prepare` — тем же путём, что скрипт, созданный самим
// ребёнком: гейт типа (data-блок не исполняется), пустой src → error,
// внешний src → fetch, инлайн-классика синхронно с document.currentScript.
//
// «Already started» — per element (HTML LS §4.12.1): повторная вставка
// исполненного скрипта не перезапускает его. Отсоединённый до доставки
// конверт теряется БЕЗ пометки — как у главного документа, где preparation
// ждёт первого connected-вставки.
//
// Срез 9: флаг ставится только когда подготовка РЕАЛЬНО началась по спеке —
// шаг «set el's already started to true» стоит после гейтов «дата-блок» и
// «нет src, тело пусто», поэтому оба эти исхода оставляют элемент
// непомеченным. Иначе поздний setAttribute('src', …) на вставленном пустым
// скрипте (каноничное `s.src = url` после appendChild) навсегда глотался бы
// первой доставкой. Предикат зеркалит ранние выходы `_lumen_script_prepare`.
function _lumen_frame_script_will_start(nid) {
    var type = _lumen_u2n(_lumen_get_attr(nid, 'type'));
    var isModule = type !== null && String(type).trim().toLowerCase() === 'module';
    // Дата-блок никогда не становится скриптом.
    if (!isModule && !_lumen_is_classic_script_type(type)) return false;
    // ЛЮБОЙ src начинает элемент: непустой — загрузкой, пустой/пробельный —
    // error-таском (спека ставит already started до обеих веток).
    var src = _lumen_u2n(_lumen_get_attr(nid, 'src'));
    if (src !== null) return true;
    var body = _lumen_u2n(_lumen_get_text_content(nid));
    return body !== null && String(body).trim() !== '';
}
var _lumen_frame_scripts_started = {};
globalThis._lumen_deliver_frame_run_script = function(nid) {
    if (typeof nid !== 'number' || nid < 0) return;
    if (!_lumen_resource_is_connected(nid)) return;
    // Уже начавшийся — спековый ранний выход №1; не начинающийся вовсе
    // (дата-блок / пусто без src) — выход до пометки, чтобы поздний
    // setAttribute('src') получил свою доставку.
    if (_lumen_frame_scripts_started[nid] === 1) return;
    if (!_lumen_frame_script_will_start(nid)) return;
    _lumen_frame_scripts_started[nid] = 1;
    _lumen_script_prepare(nid);
};

// _lumen_dispatch_unhandled_rejection (BUG-716) — Rust→JS bridge for
// `v8::Isolate::set_promise_reject_callback` (`v8_runtime.rs`). Called
// directly with the *live* `promise`/`reason` values, never through
// `eval`/JSON — an `Error` reason must keep its class and `.stack`, and
// `PromiseRejectionEvent.promise` must be the actual settled promise per
// HTML LS §8.1.7.5. `type` is 'unhandledrejection' (cancelable — its default
// action is a console report, which the Rust side suppresses when this
// returns `true`) or 'rejectionhandled' (not cancelable, no default action).
function _lumen_dispatch_unhandled_rejection(type, promise, reason) {
    var evt = new PromiseRejectionEvent(type, {
        promise: promise,
        reason: reason,
        cancelable: type === 'unhandledrejection',
        bubbles: false,
    });
    window.dispatchEvent(evt);
    return !!evt.defaultPrevented;
}

// ── queueMicrotask (HTML LS §8.1.4.4) ────────────────────────────────────────
// Schedules `fn` as a microtask; implemented via a resolved Promise chain, which
// V8 drains between tasks (same semantics as spec §8.1.4.2 microtask queue).
//
// BUG-702: the resolve/then pair is captured HERE, at shim-install time, while
// `Promise` is still V8's own, and is never re-read from the global afterwards.
// A page is free to replace `window.Promise` with its own implementation — core-js
// does exactly that whenever its feature detection rejects the native one — and
// such a polyfill schedules its reaction jobs through the host `queueMicrotask`.
// Reading `Promise` from the global here would then close the loop: polyfill
// resolve -> queueMicrotask -> polyfill Promise.resolve().then() -> polyfill
// resolve -> ... an unbounded recursion that spins the engine at 100% CPU
// forever (the tbank.ru hang).
var queueMicrotask = (function() {
    var _nativeResolve = Promise.resolve.bind(Promise);
    var _nativeThen = Promise.prototype.then;
    return function queueMicrotask(fn) {
        if (typeof fn !== 'function') throw new TypeError('queueMicrotask: argument must be a function');
        // §8.1.4.4 step 3 reports an uncaught exception from `fn`, it does not
        // reject a promise -- BUG-591 (before this, an uncaught throw here
        // surfaced as an unhandledrejection on the untouched wrapper promise
        // below, the wrong event entirely: queue-microtask-exceptions.any.html
        // waits on 'error', never on 'unhandledrejection').
        _nativeThen.call(_nativeResolve(), function() {
            try { fn(); } catch (e) { _lumen_report_exception(e); }
        });
    };
})();

