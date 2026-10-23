// CSS View Transitions L1/L2 — the script-visible API (VT-API): `ViewTransition`,
// `ViewTransitionTypeSet`, `Document.prototype.startViewTransition`,
// `Document.prototype.activeViewTransition`, `PageRevealEvent`, `PageSwapEvent`.
//
// The pixels are the shell's: this file only drives the state machine of
// css-view-transitions-1 §6.3 and tells the shell three things through
// `_lumen_vt_begin` (snapshot the frame on screen), `_lumen_vt_end` (the new state is
// ready, start the cross-fade) and `_lumen_vt_cancel` (drop the snapshot). Timing the
// spec ties to rendering opportunities is mapped onto tasks (`setTimeout(…, 0)`).
//
// Phases of one transition:  pending-capture → update-callback-called → animating → done
// (`skip` jumps to done from any of them).
(function () {
'use strict';
if (typeof document === 'undefined') { return; }

var G = (typeof globalThis !== 'undefined') ? globalThis : (0, eval)('this');

function domErr(msg, name) {
  if (typeof DOMException === 'function') { return new DOMException(msg, name); }
  var e = new Error(msg);
  e.name = name;
  return e;
}

// One task later (spec: "queue a global task"); a microtask where the runtime has no timers.
function later(fn) {
  if (typeof setTimeout === 'function') { setTimeout(fn, 0); } else { Promise.resolve().then(fn); }
}

function noop() {}

// Implementation-defined bound on the update callback's promise (css-view-transitions-1
// §6.3 step "update callback timeout"; Chromium uses 4 s).
var UPDATE_CALLBACK_TIMEOUT_MS = 4000;

function deferred() {
  var d = { settled: false };
  d.promise = new Promise(function (res, rej) {
    d.resolve = function (v) { if (!d.settled) { d.settled = true; res(v); } };
    d.reject = function (e) { if (!d.settled) { d.settled = true; rej(e); } };
  });
  return d;
}

// ── interface plumbing ──────────────────────────────────────────────────────
// WebIDL members are enumerable and configurable; `class` methods are not enumerable.
function defineInterface(ctor, tag) {
  Object.defineProperty(G, ctor.name, { value: ctor, writable: true, enumerable: false, configurable: true });
  Object.defineProperty(ctor.prototype, Symbol.toStringTag, { value: tag || ctor.name, configurable: true });
}
function defineGetter(proto, name, fn) {
  var holder = { get [name]() { return fn(this); } };
  var d = Object.getOwnPropertyDescriptor(holder, name);
  Object.defineProperty(proto, name, { get: d.get, enumerable: true, configurable: true });
}
function defineMethod(proto, name, fn) {
  Object.defineProperty(proto, name, { value: fn, writable: true, enumerable: true, configurable: true });
}

var STATE = new WeakMap();      // ViewTransition → internal state
var TYPES = new WeakMap();      // ViewTransitionTypeSet → Set<string>

function stateOf(vt) {
  var s = STATE.get(vt);
  if (!s) { throw new TypeError('Illegal invocation'); }
  return s;
}
// A Promise-typed attribute reports a wrong receiver as a rejected promise, not a throw.
function promiseAttribute(pick) {
  return function (self) {
    var s = STATE.get(self);
    if (!s) { return Promise.reject(new TypeError('Illegal invocation')); }
    return pick(s).promise;
  };
}

class ViewTransition {
  constructor() { throw new TypeError('Illegal constructor'); }
}
class ViewTransitionTypeSet {
  constructor() { throw new TypeError('Illegal constructor'); }
}

// ── ViewTransitionTypeSet: setlike<DOMString> ───────────────────────────────
function setOf(obj) {
  var s = TYPES.get(obj);
  if (!s) { throw new TypeError('Illegal invocation'); }
  return s;
}
function requireArgs(args, n, what) {
  if (args.length < n) {
    throw new TypeError("Failed to execute '" + what + "': " + n + ' argument' + (n > 1 ? 's' : '') +
      ' required, but only ' + args.length + ' present.');
  }
}
var tsp = ViewTransitionTypeSet.prototype;
defineGetter(tsp, 'size', function (self) { return setOf(self).size; });
defineMethod(tsp, 'add', ({ add(value) {
  requireArgs(arguments, 1, 'add');
  setOf(this).add(String(value));
  return this;
} }).add);
defineMethod(tsp, 'delete', ({ delete(value) {
  requireArgs(arguments, 1, 'delete');
  return setOf(this).delete(String(value));
} }).delete);
defineMethod(tsp, 'clear', ({ clear() { setOf(this).clear(); } }).clear);
defineMethod(tsp, 'has', ({ has(value) {
  requireArgs(arguments, 1, 'has');
  return setOf(this).has(String(value));
} }).has);
defineMethod(tsp, 'entries', ({ entries() { return setOf(this).entries(); } }).entries);
defineMethod(tsp, 'values', ({ values() { return setOf(this).values(); } }).values);
defineMethod(tsp, 'keys', ({ keys() { return setOf(this).keys(); } }).keys);
defineMethod(tsp, 'forEach', ({ forEach(callback) {
  requireArgs(arguments, 1, 'forEach');
  if (typeof callback !== 'function') {
    throw new TypeError("Failed to execute 'forEach': parameter 1 is not of type 'Function'.");
  }
  var thisArg = arguments[1];
  var self = this;
  setOf(this).forEach(function (v) { callback.call(thisArg, v, v, self); });
} }).forEach);
Object.defineProperty(tsp, Symbol.iterator, { value: tsp.values, writable: true, configurable: true });
defineInterface(ViewTransitionTypeSet);

function makeTypeSet(types) {
  var set = Object.create(tsp);
  TYPES.set(set, new Set(types));
  return set;
}

// ── the document's transition ───────────────────────────────────────────────
var active = null;  // the internal state of document.activeViewTransition

// Natives are bare globals of the runtime's script scope, not properties of `globalThis`.
function native(name) {
  if (name === '_lumen_vt_begin' && typeof _lumen_vt_begin === 'function') { _lumen_vt_begin(); }
  else if (name === '_lumen_vt_end' && typeof _lumen_vt_end === 'function') { _lumen_vt_end(); }
  else if (name === '_lumen_vt_cancel' && typeof _lumen_vt_cancel === 'function') { _lumen_vt_cancel(); }
}

// Every rendered element carrying a `view-transition-name` must carry a unique one
// (§6.3 "capture the old state" / "capture the new state"). `auto` and `match-element`
// generate a name per element, so they never collide.
function hasDuplicateNames() {
  if (typeof _lumen_vt_named_elements !== 'function') { return false; }
  var list;
  try { list = JSON.parse(_lumen_vt_named_elements()); } catch (e) { return false; }
  var seen = Object.create(null);
  for (var i = 0; i < list.length; i++) {
    var name = list[i][1];
    if (name === 'auto' || name === 'match-element') { continue; }
    if (seen[name]) { return true; }
    seen[name] = true;
  }
  return false;
}

// ── the pseudo-element tree (§6.4, UA style sheet) ──────────────────────────
// `getComputedStyle(html, '::view-transition…')` answers while a transition is between "capture
// the old state" and its end. The tree has a group per captured name (`root` always), and the
// group takes `COPIED_PROPS` from the captured element once the transition is activated.
var COPIED_PROPS = ['writing-mode', 'direction', 'text-orientation', 'mix-blend-mode', 'backdrop-filter', 'color-scheme'];

// `{ name: [[property, value], …] }` of the elements carrying a `view-transition-name` now.
function captureNames() {
  var out = Object.create(null);
  var list = [];
  if (typeof _lumen_vt_named_elements === 'function') {
    try { list = JSON.parse(_lumen_vt_named_elements()); } catch (e) { list = []; }
  }
  var rootNid = document.documentElement ? document.documentElement.__nid__ : null;
  if (rootNid != null) { list.push([rootNid, 'root']); }
  for (var i = 0; i < list.length; i++) {
    var name = list[i][1];
    if (name === 'auto' || name === 'match-element' || name in out) { continue; }
    var props = [];
    if (typeof _lumen_get_computed_style === 'function') {
      for (var j = 0; j < COPIED_PROPS.length; j++) {
        var v = _lumen_get_computed_style(list[i][0], COPIED_PROPS[j]);
        if (v) { props.push([COPIED_PROPS[j], v]); }
      }
    }
    out[name] = props;
  }
  return out;
}

// The style map of the pseudo-element `kind` (`view-transition`, `view-transition-old(a)`, …) of
// the active transition, or null when the page has no such pseudo-element right now.
Object.defineProperty(G, '_lumen_vt_pseudo_style', {
  value: function (kind) {
    var s = active;
    if (!s || (s.phase !== 'update-callback-called' && s.phase !== 'animating')) { return null; }
    var m = /^(view-transition(?:-group|-image-pair|-old|-new)?)(?:\(([^()]+)\))?$/.exec(kind);
    if (!m || typeof _lumen_vt_tree_style !== 'function') { return null; }
    var copied = [];
    if (m[2] !== undefined) {
      // The old/new parts exist for the names of that state only; group and image pair for either.
      var inOld = s.oldNames && m[2] in s.oldNames;
      var inNew = s.newNames && m[2] in s.newNames;
      var exists = m[1] === 'view-transition-old' ? inOld : m[1] === 'view-transition-new' ? inNew : inOld || inNew;
      if (!exists) { return null; }
      if (m[1] === 'view-transition-group' && s.newNames) { copied = (inNew ? s.newNames : s.oldNames)[m[2]]; }
    } else if (m[1] !== 'view-transition') {
      return null;
    }
    try { return JSON.parse(_lumen_vt_tree_style(kind, JSON.stringify(copied))); } catch (e) { return null; }
  },
  writable: true, enumerable: false, configurable: true,
});

// §6.3 "skip the view transition": ready rejects with `reason`, finished follows the
// update callback, which still runs when it has not yet.
function skip(s, reason) {
  if (s.phase === 'done') { return; }
  var needsCallback = !s.callbackStarted;
  s.phase = 'done';
  if (active === s) { active = null; }
  if (s.began) { s.began = false; native('_lumen_vt_cancel'); }
  s.ready.reject(reason);
  if (needsCallback) { later(function () { invokeCallback(s); }); }
  // `finished` follows the update callback. Deliberately not `updateDone.promise.then(…)`:
  // a reaction there would mark a rejected `updateCallbackDone` as handled, and the page
  // is owed its unhandledrejection event for it.
  s.followsCallback = true;
  if (s.outcome) { settleFinished(s); }
}

function settleFinished(s) {
  if (s.outcome.ok) { s.finished.resolve(undefined); } else { s.finished.reject(s.outcome.error); }
}

// §6.3 "activate the view transition": the update callback is done and the new state
// is on the page.
function activate(s) {
  if (s.phase !== 'update-callback-called') { return; }
  if (hasDuplicateNames()) {
    skip(s, domErr('Transition was aborted because of invalid state: duplicate view-transition-name', 'InvalidStateError'));
    return;
  }
  s.phase = 'animating';
  s.newNames = captureNames();
  native('_lumen_vt_end');
  s.ready.resolve(undefined);
  // The cross-fade itself runs in the shell; `finished` settles one task after `ready`
  // (and after every `waitUntil` promise), so the page can observe the active state.
  later(function () { finish(s); });
}

function finish(s) {
  if (s.phase !== 'animating') { return; }
  // `waitUntil` promises only delay `finished`; whether they fulfil or reject is irrelevant.
  // One added while waiting extends the wait.
  var waitAll = function () {
    var seen = s.waits.length;
    var settled = s.waits.map(function (p) { return p.then(noop, noop); });
    return Promise.all(settled).then(function () { return s.waits.length > seen ? waitAll() : undefined; });
  };
  waitAll().then(function () {
    if (s.phase !== 'animating') { return; }
    s.phase = 'done';
    if (active === s) { active = null; }
    s.began = false;
    s.finished.resolve(undefined);
  });
}

function invokeCallback(s) {
  s.callbackStarted = true;
  var p;
  try {
    p = Promise.resolve(s.callback ? s.callback() : undefined);
  } catch (e) {
    p = Promise.reject(e);
  }
  var timer = null;
  if (s.phase === 'update-callback-called' && typeof setTimeout === 'function') {
    timer = setTimeout(function () {
      timer = null;
      skip(s, domErr('Transition was aborted because of timeout in DOM update', 'TimeoutError'));
    }, UPDATE_CALLBACK_TIMEOUT_MS);
  }
  var settled = function () {
    if (timer !== null && typeof clearTimeout === 'function') { clearTimeout(timer); timer = null; }
  };
  p.then(
    function () {
      settled();
      s.outcome = { ok: true };
      s.updateDone.resolve(undefined);
      if (s.followsCallback) { settleFinished(s); }
      activate(s);
    },
    function (e) {
      settled();
      s.outcome = { ok: false, error: e };
      s.updateDone.reject(e);
      if (s.followsCallback) { settleFinished(s); }
      if (s.phase === 'update-callback-called') { skip(s, e); }
    });
}

// §6.3 "capture the old state", then the update callback.
function capture(s) {
  if (s.phase !== 'pending-capture') { return; }
  if (hasDuplicateNames()) {
    skip(s, domErr('Transition was aborted because of invalid state: duplicate view-transition-name', 'InvalidStateError'));
    return;
  }
  s.phase = 'update-callback-called';
  s.oldNames = captureNames();
  s.began = true;
  native('_lumen_vt_begin');
  invokeCallback(s);
}

// ── ViewTransition ──────────────────────────────────────────────────────────
var vtp = ViewTransition.prototype;
defineGetter(vtp, 'updateCallbackDone', promiseAttribute(function (s) { return s.updateDone; }));
defineGetter(vtp, 'ready', promiseAttribute(function (s) { return s.ready; }));
defineGetter(vtp, 'finished', promiseAttribute(function (s) { return s.finished; }));
defineMethod(vtp, 'skipTransition', ({ skipTransition() {
  skip(stateOf(this), domErr('Skipped view transition due to skipTransition() call', 'AbortError'));
} }).skipTransition);
defineGetter(vtp, 'types', function (self) { return stateOf(self).types; });
defineGetter(vtp, 'transitionRoot', function (self) { stateOf(self); return document.documentElement; });
defineMethod(vtp, 'waitUntil', ({ waitUntil(promise) {
  requireArgs(arguments, 1, 'waitUntil');
  var s = stateOf(this);
  if (s.phase === 'done') { return; }
  var p = Promise.resolve(promise);
  s.waits.push(p);
} }).waitUntil);
defineInterface(ViewTransition);

function isDocument(obj) {
  return typeof Document === 'function' ? obj instanceof Document : obj === document;
}

// `optional` parameter: the operation's `length` is 0, so it reads `arguments`.
function startViewTransition() {
  if (!isDocument(this)) { throw new TypeError('Illegal invocation'); }
  var callbackOptions = arguments[0];
  var callback = null;
  var types = [];
  if (typeof callbackOptions === 'function') {
    callback = callbackOptions;
  } else if (callbackOptions !== undefined && callbackOptions !== null) {
    if (typeof callbackOptions !== 'object') {
      throw new TypeError("Failed to execute 'startViewTransition' on 'Document': " +
        'parameter 1 is not of type \'StartViewTransitionOptions\'.');
    }
    var update = callbackOptions.update;
    if (update !== undefined && update !== null) {
      if (typeof update !== 'function') {
        throw new TypeError("Failed to execute 'startViewTransition' on 'Document': " +
          "The provided callback is not a function.");
      }
      callback = update;
    }
    var ts = callbackOptions.types;
    if (ts !== undefined && ts !== null) {
      if (typeof ts[Symbol.iterator] !== 'function') {
        throw new TypeError("Failed to execute 'startViewTransition' on 'Document': " +
          "The provided value cannot be converted to a sequence.");
      }
      types = Array.from(ts, String);
    }
  }

  var vt = Object.create(vtp);
  var s = {
    vt: vt, callback: callback, phase: 'pending-capture', began: false, callbackStarted: false,
    outcome: null, followsCallback: false,
    ready: deferred(), updateDone: deferred(), finished: deferred(), waits: [],
    types: makeTypeSet(types),
  };
  STATE.set(vt, s);

  if (active) {
    skip(active, domErr('Transition was aborted because of invocation of startViewTransition()', 'AbortError'));
  }
  active = s;
  later(function () { capture(s); });
  return vt;
}

var docProto = (typeof Document === 'function' && Document.prototype) ? Document.prototype : document;
defineMethod(docProto, 'startViewTransition', startViewTransition);
Object.defineProperty(docProto, 'activeViewTransition', {
  get: Object.getOwnPropertyDescriptor({ get activeViewTransition() {
    if (!isDocument(this)) { throw new TypeError('Illegal invocation'); }
    return active ? active.vt : null;
  } },
    'activeViewTransition').get,
  enumerable: true,
  configurable: true,
});

// ── pagereveal / pageswap (HTML LS §7.11.9) ─────────────────────────────────
// Only the interfaces and the event-handler attributes: firing them belongs to the
// cross-document navigation (docs/tasks/ph3-view-transitions-mpa.md).
if (typeof Event === 'function') {
  var EVENT_DATA = new WeakMap();
  var viewTransitionOrNull = function (v, ctor) {
    if (v === undefined || v === null) { return null; }
    if (!(v instanceof ViewTransition)) {
      throw new TypeError("Failed to construct '" + ctor + "': Failed to read the 'viewTransition' property " +
        "from '" + ctor + "Init': Failed to convert value to 'ViewTransition'.");
    }
    return v;
  };
  var eventData = function (e) {
    var d = EVENT_DATA.get(e);
    if (!d) { throw new TypeError('Illegal invocation'); }
    return d;
  };
  class PageRevealEvent extends Event {
    constructor(type, init) {
      requireArgs(arguments, 1, "construct 'PageRevealEvent'");
      super(type, init);
      EVENT_DATA.set(this, { viewTransition: viewTransitionOrNull(init && init.viewTransition, 'PageRevealEvent') });
    }
  }
  defineGetter(PageRevealEvent.prototype, 'viewTransition', function (self) { return eventData(self).viewTransition; });
  defineInterface(PageRevealEvent);

  class PageSwapEvent extends Event {
    constructor(type, init) {
      requireArgs(arguments, 1, "construct 'PageSwapEvent'");
      super(type, init);
      var activation = init && init.activation !== undefined ? init.activation : null;
      if (activation !== null && (typeof activation !== 'object' && typeof activation !== 'function')) {
        throw new TypeError("Failed to construct 'PageSwapEvent': Failed to read the 'activation' property " +
          "from 'PageSwapEventInit': Failed to convert value to 'NavigationActivation'.");
      }
      EVENT_DATA.set(this, {
        activation: activation,
        viewTransition: viewTransitionOrNull(init && init.viewTransition, 'PageSwapEvent'),
      });
    }
  }
  defineGetter(PageSwapEvent.prototype, 'activation', function (self) { return eventData(self).activation; });
  defineGetter(PageSwapEvent.prototype, 'viewTransition', function (self) { return eventData(self).viewTransition; });
  defineInterface(PageSwapEvent);
}
if (typeof window === 'object' && window !== null) {
  if (!('onpagereveal' in window)) { window.onpagereveal = null; }
  if (!('onpageswap' in window)) { window.onpageswap = null; }
}
})();
