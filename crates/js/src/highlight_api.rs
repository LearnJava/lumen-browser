//! CSS Custom Highlight API (`css-highlight-api-1`) — `Highlight`/`HighlightRegistry`.
//! The live `CSS.highlights` registry and `Highlight` Setlike state live entirely
//! in the JS shim's closure (`HIGHLIGHT_API_SHIM`); the Rust structs below are a
//! standalone, unwired sketch exercised only by this file's own unit tests.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static HIGHLIGHTS_REGISTRY: OnceLock<Mutex<HighlightRegistry>> = OnceLock::new();

#[derive(Clone, Debug, Default)]
pub struct HighlightRegistry {
    highlights: HashMap<String, Highlight>,
}

impl HighlightRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: String, highlight: Highlight) {
        self.highlights.insert(name, highlight);
    }

    pub fn get(&self, name: &str) -> Option<Highlight> {
        self.highlights.get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.highlights.contains_key(name)
    }

    pub fn delete(&mut self, name: &str) -> bool {
        self.highlights.remove(name).is_some()
    }

    pub fn clear(&mut self) {
        self.highlights.clear();
    }

    pub fn all(&self) -> Vec<(String, Highlight)> {
        self.highlights
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

pub fn get_highlights_registry() -> &'static Mutex<HighlightRegistry> {
    HIGHLIGHTS_REGISTRY.get_or_init(|| Mutex::new(HighlightRegistry::new()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub priority: i32,
    pub range_ids: Vec<String>,
}

impl Highlight {
    pub fn new(priority: i32, range_ids: Vec<String>) -> Self {
        Self {
            priority,
            range_ids,
        }
    }
}

/// Install CSS Highlight API JS bindings: `CSS.highlights` registry + `Highlight` class.
///
/// Evaluates the JS shim via [`lumen_core::ext::JsRuntime::eval`] on the default (V8) engine.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_highlight_api_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(HIGHLIGHT_API_SHIM)?;
    Ok(())
}

// BUG-534: rewritten from the Phase-0 ad-hoc shim (raw `.ranges` array,
// hand-rolled `CSS.highlights` object) into the spec's actual interfaces —
// `Highlight` is Setlike<(Range or StaticRange)>, `CSS.highlights` is an
// instance of a real `HighlightRegistry` (Maplike<DOMString, Highlight>).
// Both keep their membership list in a private closure (`WeakMap`), never
// delegating to a native `Set`/`Map` instance, because
// `Highlight-setlike-tampered-Set-prototype.html`/
// `HighlightRegistry-maplike-tampered-Map-prototype.html` freeze/replace
// `Set.prototype`/`Map.prototype` members with non-callable junk and still
// expect every method to work — the same reason `Headers` (`web_api_shim_mid_b.js`)
// keeps its own array-backed list instead of a real `Map`.
//
// `highlightsFromPoint()` (spec §5) is defined with full argument validation
// but always returns an empty array — real hit-testing against painted
// highlight ranges needs paint to consume this registry at all, which it
// doesn't yet (`highlight_name` on `DisplayCommand::DrawText` is a Phase-0
// stub, never fed from here). That remains open scope.
#[cfg(feature = "v8-backend")]
const HIGHLIGHT_API_SHIM: &str = r#"(function(global) {
  'use strict';

  function hdef(obj, key, value) {
    Object.defineProperty(obj, key, { value: value, writable: true, enumerable: false, configurable: true });
  }

  // Both Highlight (Setlike) and HighlightRegistry (Maplike) back their
  // membership by a singly-linked chain of plain entry objects rather than a
  // native Set/Map — two independent reasons converge on the same shape:
  //   1. `*-tampered-*-prototype.html` freeze `Set.prototype`/`Map.prototype`
  //      with non-callable junk and still expect every method to work, so
  //      nothing here may call through a native Set/Map instance's prototype.
  //   2. `*-iteration-with-modifications.html` requires *live* iteration —
  //      Map/Set semantics say an iterator must see insertions that happen
  //      after it was created (as long as they land after its current
  //      position) and skip not-yet-visited deletions, which a `.slice()`
  //      snapshot cannot do. `delete`/`clear` only flag an entry `removed`
  //      and never sever its `.next` pointer, so an iterator sitting on (or
  //      behind) a removed entry can still walk forward through the rest of
  //      the live chain — exactly how V8's own Map/Set iterators work
  //      internally.
  // `size` is a maintained counter, not a chain walk, since delete/clear are
  // O(1) flag flips rather than array splices.
  function makeChain() { return { head: null, tail: null, size: 0 }; }
  function chainAppend(ch, key, value) {
    var e = { key: key, value: value, next: null, removed: false };
    if (ch.tail) ch.tail.next = e; else ch.head = e;
    ch.tail = e;
    ch.size++;
    return e;
  }
  // Linear scan skipping removed entries — chains here stay small (one
  // Highlight's ranges, one page's named highlights), so this is not the
  // O(log n) lookup a spec implementation would want.
  function chainFind(ch, key) {
    for (var e = ch.head; e; e = e.next) if (!e.removed && e.key === key) return e;
    return null;
  }
  function chainDelete(ch, key) {
    var e = chainFind(ch, key);
    if (!e) return false;
    e.removed = true;
    ch.size--;
    return true;
  }
  function chainClear(ch) {
    for (var e = ch.head; e; e = e.next) e.removed = true;
    ch.head = null; ch.tail = null; ch.size = 0;
  }
  function chainForEach(ch, cb) {
    for (var e = ch.head; e; e = e.next) if (!e.removed) cb(e.key, e.value);
  }
  // Lazy-start iterator: the chain position is only read on the first
  // `.next()` call, so an iterator created while the chain is still empty
  // observes entries appended before that first call (BUG-534's
  // `*-iteration-with-modifications.html`).
  function makeChainIterator(protoObj, ch, project) {
    var started = false, cur = null;
    var it = Object.create(protoObj);
    Object.defineProperty(it, 'next', {
      value: function() {
        if (!started) { cur = ch.head; started = true; }
        while (cur && cur.removed) cur = cur.next;
        if (!cur) return { value: undefined, done: true };
        var e = cur;
        cur = cur.next;
        return { value: project(e.key, e.value), done: false };
      },
      writable: true, configurable: true,
    });
    return it;
  }

  // ── Highlight — Setlike<(Range or StaticRange)> ───────────────────────────
  var HIGHLIGHT_STATE = new WeakMap();
  function highlightStateOf(h) {
    var st = HIGHLIGHT_STATE.get(h);
    if (!st) throw new TypeError('Illegal invocation: receiver is not a Highlight object');
    return st;
  }
  var HIGHLIGHT_TYPES = ['highlight', 'spelling-error', 'grammar-error'];

  function Highlight() {
    if (new.target === undefined) {
      throw new TypeError('Failed to construct Highlight: please use the new operator');
    }
    var st = { chain: makeChain(), priority: 0, type: 'highlight' };
    HIGHLIGHT_STATE.set(this, st);
    for (var i = 0; i < arguments.length; i++) {
      var range = arguments[i];
      if (!chainFind(st.chain, range)) chainAppend(st.chain, range, range);
    }
  }

  Object.defineProperty(Highlight.prototype, 'priority', {
    get: function() { return highlightStateOf(this).priority; },
    set: function(v) { highlightStateOf(this).priority = Number(v) | 0; },
    enumerable: false, configurable: true,
  });
  // WebIDL enum setter: an out-of-enum value leaves the attribute unchanged
  // rather than throwing.
  Object.defineProperty(Highlight.prototype, 'type', {
    get: function() { return highlightStateOf(this).type; },
    set: function(v) {
      var s = String(v);
      if (HIGHLIGHT_TYPES.indexOf(s) >= 0) highlightStateOf(this).type = s;
    },
    enumerable: false, configurable: true,
  });
  Object.defineProperty(Highlight.prototype, 'size', {
    get: function() { return highlightStateOf(this).chain.size; },
    enumerable: false, configurable: true,
  });

  hdef(Highlight.prototype, 'has', function(range) {
    return chainFind(highlightStateOf(this).chain, range) !== null;
  });
  hdef(Highlight.prototype, 'add', function(range) {
    var ch = highlightStateOf(this).chain;
    if (!chainFind(ch, range)) chainAppend(ch, range, range);
    return this;
  });
  hdef(Highlight.prototype, 'delete', function(range) {
    return chainDelete(highlightStateOf(this).chain, range);
  });
  hdef(Highlight.prototype, 'clear', function() { chainClear(highlightStateOf(this).chain); });
  hdef(Highlight.prototype, 'forEach', function(cb, thisArg) {
    if (typeof cb !== 'function') throw new TypeError('Highlight.forEach requires a function callback');
    var self = this;
    chainForEach(highlightStateOf(this).chain, function(range) { cb.call(thisArg, range, range, self); });
  });

  var SET_ITER_PROTO = {};
  Object.defineProperty(SET_ITER_PROTO, Symbol.toStringTag, { value: 'Highlight Iterator', configurable: true });
  Object.defineProperty(SET_ITER_PROTO, Symbol.iterator, {
    value: function() { return this; }, writable: true, configurable: true,
  });
  hdef(Highlight.prototype, 'keys', function() {
    return makeChainIterator(SET_ITER_PROTO, highlightStateOf(this).chain, function(k) { return k; });
  });
  hdef(Highlight.prototype, 'values', function() {
    return makeChainIterator(SET_ITER_PROTO, highlightStateOf(this).chain, function(k) { return k; });
  });
  hdef(Highlight.prototype, 'entries', function() {
    return makeChainIterator(SET_ITER_PROTO, highlightStateOf(this).chain, function(k) { return [k, k]; });
  });
  hdef(Highlight.prototype, Symbol.iterator, Highlight.prototype.values);
  Object.defineProperty(Highlight.prototype, Symbol.toStringTag, { value: 'Highlight', configurable: true });

  global.Highlight = Highlight;

  // ── HighlightRegistry — Maplike<DOMString, Highlight> ─────────────────────
  // No spec constructor operation: `new HighlightRegistry()` must throw, so
  // the single `CSS.highlights` instance is built by hand via `Object.create`
  // + direct `WeakMap` state, bypassing the constructor body entirely.
  var REGISTRY_STATE = new WeakMap();
  function registryStateOf(r) {
    var st = REGISTRY_STATE.get(r);
    if (!st) throw new TypeError('Illegal invocation: receiver is not a HighlightRegistry object');
    return st;
  }
  function HighlightRegistry() {
    throw new TypeError('Illegal constructor');
  }

  hdef(HighlightRegistry.prototype, 'set', function(name, highlight) {
    var ch = registryStateOf(this).chain, n = String(name);
    var e = chainFind(ch, n);
    if (e) { e.value = highlight; } else { chainAppend(ch, n, highlight); }
    return this;
  });
  hdef(HighlightRegistry.prototype, 'get', function(name) {
    var e = chainFind(registryStateOf(this).chain, String(name));
    return e ? e.value : undefined;
  });
  hdef(HighlightRegistry.prototype, 'has', function(name) {
    return chainFind(registryStateOf(this).chain, String(name)) !== null;
  });
  hdef(HighlightRegistry.prototype, 'delete', function(name) {
    return chainDelete(registryStateOf(this).chain, String(name));
  });
  hdef(HighlightRegistry.prototype, 'clear', function() { chainClear(registryStateOf(this).chain); });
  hdef(HighlightRegistry.prototype, 'forEach', function(cb, thisArg) {
    if (typeof cb !== 'function') throw new TypeError('HighlightRegistry.forEach requires a function callback');
    var self = this;
    chainForEach(registryStateOf(this).chain, function(name, highlight) { cb.call(thisArg, highlight, name, self); });
  });
  var MAP_ITER_PROTO = {};
  Object.defineProperty(MAP_ITER_PROTO, Symbol.toStringTag, { value: 'HighlightRegistry Iterator', configurable: true });
  Object.defineProperty(MAP_ITER_PROTO, Symbol.iterator, {
    value: function() { return this; }, writable: true, configurable: true,
  });
  hdef(HighlightRegistry.prototype, 'keys', function() {
    return makeChainIterator(MAP_ITER_PROTO, registryStateOf(this).chain, function(k) { return k; });
  });
  hdef(HighlightRegistry.prototype, 'values', function() {
    return makeChainIterator(MAP_ITER_PROTO, registryStateOf(this).chain, function(k, v) { return v; });
  });
  hdef(HighlightRegistry.prototype, 'entries', function() {
    return makeChainIterator(MAP_ITER_PROTO, registryStateOf(this).chain, function(k, v) { return [k, v]; });
  });
  hdef(HighlightRegistry.prototype, Symbol.iterator, HighlightRegistry.prototype.entries);
  Object.defineProperty(HighlightRegistry.prototype, 'size', {
    get: function() { return registryStateOf(this).chain.size; },
    enumerable: false, configurable: true,
  });
  Object.defineProperty(HighlightRegistry.prototype, Symbol.toStringTag, { value: 'HighlightRegistry', configurable: true });

  // §5 highlightsFromPoint(x, y, options?): argument validation per spec IDL
  // overload resolution; real hit-testing is not implemented (see file doc
  // comment above), so a validated call always resolves to no hits.
  hdef(HighlightRegistry.prototype, 'highlightsFromPoint', function(x, y, options) {
    registryStateOf(this);
    var nx = Number(x), ny = Number(y);
    if (Number.isNaN(nx) || Number.isNaN(ny)) {
      throw new TypeError('highlightsFromPoint requires numeric x and y coordinates');
    }
    if (options !== undefined && (options === null || typeof options !== 'object')) {
      throw new TypeError('highlightsFromPoint options must be an object');
    }
    if (options && options.shadowRoots !== undefined) {
      var roots = options.shadowRoots;
      if (roots === null || typeof roots !== 'object' || typeof roots[Symbol.iterator] !== 'function') {
        throw new TypeError('shadowRoots must be iterable');
      }
      var arr = Array.from(roots);
      for (var i = 0; i < arr.length; i++) {
        var r = arr[i];
        if (!r || r.nodeType !== 11 || typeof r.host === 'undefined') {
          throw new TypeError('shadowRoots must contain ShadowRoot objects');
        }
      }
    }
    return [];
  });

  global.HighlightRegistry = HighlightRegistry;

  if (!global.CSS) global.CSS = {};
  var registry = Object.create(HighlightRegistry.prototype);
  REGISTRY_STATE.set(registry, { chain: makeChain() });
  global.CSS.highlights = registry;
})(globalThis);"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_registry_new() {
        let reg = HighlightRegistry::new();
        assert!(reg.all().is_empty());
    }

    #[test]
    fn highlight_registry_set_get() {
        let mut reg = HighlightRegistry::new();
        let hl = Highlight::new(10, vec!["range-1".to_string()]);
        reg.set("search".to_string(), hl.clone());

        let retrieved = reg.get("search").unwrap();
        assert_eq!(retrieved.priority, 10);
        assert_eq!(retrieved.range_ids, vec!["range-1"]);
    }

    #[test]
    fn highlight_registry_has() {
        let mut reg = HighlightRegistry::new();
        let hl = Highlight::new(0, Vec::new());
        reg.set("spelling-error".to_string(), hl);

        assert!(reg.has("spelling-error"));
        assert!(!reg.has("nonexistent"));
    }

    #[test]
    fn highlight_registry_delete() {
        let mut reg = HighlightRegistry::new();
        let hl = Highlight::new(0, Vec::new());
        reg.set("highlight-1".to_string(), hl);

        assert!(reg.delete("highlight-1"));
        assert!(!reg.has("highlight-1"));
    }

    #[test]
    fn highlight_registry_clear() {
        let mut reg = HighlightRegistry::new();
        reg.set("h1".to_string(), Highlight::new(1, Vec::new()));
        reg.set("h2".to_string(), Highlight::new(2, Vec::new()));

        assert_eq!(reg.all().len(), 2);
        reg.clear();
        assert!(reg.all().is_empty());
    }

    #[test]
    fn highlight_priority_ordering() {
        let h1 = Highlight::new(10, vec!["range-1".to_string()]);
        let h2 = Highlight::new(5, vec!["range-2".to_string()]);

        assert!(h1.priority > h2.priority);
    }

    #[test]
    fn highlight_default_priority() {
        let hl = Highlight::new(0, Vec::new());
        assert_eq!(hl.priority, 0);
    }

    #[test]
    fn highlight_registry_overwrites() {
        let mut reg = HighlightRegistry::new();
        let hl1 = Highlight::new(5, Vec::new());
        let hl2 = Highlight::new(10, Vec::new());

        reg.set("name".to_string(), hl1);
        reg.set("name".to_string(), hl2);

        let retrieved = reg.get("name").unwrap();
        assert_eq!(retrieved.priority, 10);
    }

    #[test]
    fn highlight_registry_all() {
        let mut reg = HighlightRegistry::new();
        reg.set("h1".to_string(), Highlight::new(1, Vec::new()));
        reg.set("h2".to_string(), Highlight::new(2, Vec::new()));

        let all = reg.all();
        assert_eq!(all.len(), 2);
    }
}
