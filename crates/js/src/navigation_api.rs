//! Navigation API (HTML LS §7.8).
//!
//! Provides `window.navigation` singleton with `currentEntry`, `entries()`,
//! `navigate()`, `back()`, `forward()`, `traverseTo()` methods and events
//! `navigate`, `navigatesuccess`, `navigateerror`, `currententrychange`.

/// V8 port of the former rquickjs `install_navigation_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B5): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_navigation_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(NAVIGATION_API_SHIM)?;
    Ok(())
}

/// JavaScript shim: Navigation singleton with history entries and event handling.
///
/// BUG-639: the shim used to be a Phase-0 sketch — a bare `URL` as
/// `NavigateEvent.destination`, a plain `Event` for `currententrychange`, no
/// `updateCurrentEntry()`, and a `NavigationHistoryEntry` that was a fresh
/// non-`EventTarget` object on every read, so `dispose` could never arrive and
/// `navigation.currentEntry === navigation.currentEntry` was `false`. Entries
/// are now cached per shell key (identity is stable across reads), resynced on
/// every `_lumen_navigation_set_state`, and an entry the shell dropped from
/// its stacks gets `index === -1` and a trusted `dispose` event.
///
/// Navigation API state (`getState()`) is kept here, per entry key, as a
/// structured clone — separate from `history.state` (HTML LS §7.2.6: they are
/// different pieces of the session history entry). It lives as long as this
/// document's realm; a cross-document traversal back to an entry does not
/// restore it yet.
#[cfg(feature = "v8-backend")]
const NAVIGATION_API_SHIM: &str = r#"(function() {
  'use strict';

  // Guards the non-constructible interfaces: only the shim holds the token.
  const TOKEN = {};
  function illegal() { throw new TypeError('Illegal constructor'); }

  // A structured clone is the spec's serialize/deserialize pair collapsed into
  // one step — `getState()` must hand out a fresh copy on every call.
  function cloneState(v) {
    return v === undefined ? undefined : structuredClone(v);
  }

  // `on<event>` IDL attributes: a prototype accessor backed by a per-instance
  // slot, so `'ondispose' in entry` holds and `EventTarget.dispatchEvent`'s
  // `this['on' + type]` lookup finds the handler.
  function defineHandlerAttr(proto, name) {
    const slot = '_h_' + name;
    Object.defineProperty(proto, 'on' + name, {
      get() { return this[slot] || null; },
      set(v) {
        Object.defineProperty(this, slot, {
          value: (typeof v === 'function' || (v && typeof v === 'object')) ? v : null,
          writable: true, configurable: true, enumerable: false
        });
      },
      enumerable: true, configurable: true
    });
  }

  function trusted(ev) { ev.isTrusted = true; return ev; }

  /// NavigationHistoryEntry (HTML LS §7.2.9.3): one entry of the shell's
  /// `nav_back`/current/`nav_fwd` stacks. Not constructible from script.
  class NavigationHistoryEntry extends EventTarget {
    constructor(token, key, id, url) {
      if (token !== TOKEN) illegal();
      super();
      this._key = key;
      this._id = id;
      this._url = url;
      this._index = -1;
    }
    get url()      { return this._url; }
    get key()      { return this._key; }
    get id()       { return this._id; }
    get index()    { return this._index; }
    get sameDocument() { return !this._disposed && navigation._docKeys.has(this._key); }
    getState()     { return cloneState(navigation._states.get(this._key)); }
  }
  defineHandlerAttr(NavigationHistoryEntry.prototype, 'dispose');
  Object.defineProperty(NavigationHistoryEntry.prototype, Symbol.toStringTag,
    { value: 'NavigationHistoryEntry', configurable: true });

  /// NavigationDestination (HTML LS §7.2.9.6): where a `navigate` event is
  /// heading. For a traversal it describes the target entry; for a push or
  /// replace there is no entry yet, so `key`/`id` are `''` and `index` is -1.
  class NavigationDestination {
    constructor(token, init) {
      if (token !== TOKEN) illegal();
      this._url = init.url;
      this._key = init.key || '';
      this._id = init.id || '';
      this._index = init.index === undefined ? -1 : init.index;
      this._sameDocument = !!init.sameDocument;
      this._state = init.state;
    }
    get url()          { return this._url; }
    get key()          { return this._key; }
    get id()           { return this._id; }
    get index()        { return this._index; }
    get sameDocument() { return this._sameDocument; }
    getState()         { return cloneState(this._state); }
  }
  Object.defineProperty(NavigationDestination.prototype, Symbol.toStringTag,
    { value: 'NavigationDestination', configurable: true });

  /// NavigationTransition (HTML LS §7.2.9.1): the navigation an `intercept()`
  /// turned into an ongoing same-document transition. `navigation.transition`
  /// holds it from the intercepting `navigate` event until the outcome
  /// (`navigatesuccess`/`navigateerror`) has been dispatched.
  class NavigationTransition {
    constructor(token, init) {
      if (token !== TOKEN) illegal();
      this._navigationType = init.navigationType;
      this._from = init.from;
      this._to = init.to;
      this._committed = new Promise((res, rej) => { this._resC = res; this._rejC = rej; });
      this._finished = new Promise((res, rej) => { this._resF = res; this._rejF = rej; });
      // The spec marks both as handled: a page that ignores them gets no
      // unhandled rejection.
      this._committed.catch(() => {});
      this._finished.catch(() => {});
    }
    get navigationType() { return this._navigationType; }
    get from()           { return this._from; }
    get to()             { return this._to; }
    get committed()      { return this._committed; }
    get finished()       { return this._finished; }
  }
  Object.defineProperty(NavigationTransition.prototype, Symbol.toStringTag,
    { value: 'NavigationTransition', configurable: true });

  /// NavigateEvent: fired before navigation.
  class NavigateEvent extends Event {
    constructor(init = {}) {
      // HTML LS §7.8.1: `canIntercept` navigations are the only ones whose
      // `navigate` event is cancelable — a browser/automation-initiated
      // navigation (typed URL, WebDriver `browsingContext.navigate`) fires a
      // non-cancelable event for observability only (BUG-1031).
      super('navigate', { cancelable: !!init.canIntercept });
      this._navigationType = init.navigationType || 'push';
      this._userInitiated = init.userInitiated || false;
      this._hashChange = init.hashChange || false;
      this._signal = init.signal || new AbortSignal();
      this._destination = init.destination || null;
      this._canIntercept = !!init.canIntercept;
      this._intercepted = false;
      this._handledPromise = Promise.resolve();
    }

    get navigationType() {
      return this._navigationType;
    }

    get userInitiated() {
      return this._userInitiated;
    }

    get hashChange() {
      return this._hashChange;
    }

    get signal() {
      return this._signal;
    }

    get destination() {
      return this._destination;
    }

    get canIntercept() {
      return this._canIntercept;
    }

    intercept(options = {}) {
      // BUG-1031: without this guard, a page's `navigate` listener that
      // unconditionally calls `intercept()` permanently wedges the browsing
      // context the next time a browser/automation-initiated navigation
      // (WebDriver `browsingContext.navigate`, typed URL, ...) fires this
      // event on it — the real cross-document load never runs, yet the
      // caller (e.g. BiDi) sees no error to react to.
      if (!this._canIntercept) {
        throw new DOMException(
          "Failed to execute 'intercept' on 'NavigateEvent': intercept() may " +
          'only be called on a cancelable navigate event.',
          'InvalidStateError'
        );
      }
      this._intercepted = true;
      const handler = options.handler || (() => {});
      this._handledPromise = Promise.resolve().then(handler);
      window._lumen_pending_intercept_handler = handler;
    }

    _isIntercepted() {
      return this._intercepted;
    }

    _getHandledPromise() {
      return this._handledPromise;
    }
  }

  /// NavigationCurrentEntryChangeEvent (HTML LS §7.2.9.7). `from` is a
  /// required dictionary member, so both a missing dictionary and one without
  /// `from` throw — WebIDL §3.2.18.
  class NavigationCurrentEntryChangeEvent extends Event {
    constructor(type, init) {
      if (arguments.length < 2 || init === undefined || init === null) {
        throw new TypeError(
          "Failed to construct 'NavigationCurrentEntryChangeEvent': " +
          "required member 'from' is undefined.");
      }
      if (typeof init !== 'object' && typeof init !== 'function') {
        throw new TypeError(
          "Failed to construct 'NavigationCurrentEntryChangeEvent': " +
          'parameter 2 is not of type NavigationCurrentEntryChangeEventInit.');
      }
      const from = init.from;
      if (from === undefined) {
        throw new TypeError(
          "Failed to construct 'NavigationCurrentEntryChangeEvent': " +
          "required member 'from' is undefined.");
      }
      if (!(from instanceof NavigationHistoryEntry)) {
        throw new TypeError(
          "Failed to construct 'NavigationCurrentEntryChangeEvent': " +
          "member 'from' is not of type NavigationHistoryEntry.");
      }
      const nt = init.navigationType;
      if (nt !== undefined && nt !== null &&
          ['push', 'replace', 'reload', 'traverse'].indexOf(String(nt)) < 0) {
        throw new TypeError(
          "Failed to construct 'NavigationCurrentEntryChangeEvent': " +
          "'" + String(nt) + "' is not a valid NavigationType.");
      }
      super(type, init);
      this._navigationType = (nt === undefined || nt === null) ? null : String(nt);
      this._from = from;
    }
    get navigationType() { return this._navigationType; }
    get from()           { return this._from; }
  }

  /// Navigation singleton class.
  /// State is read from the shell; mutations are sent to the shell (single authority).
  class Navigation extends EventTarget {
    constructor() {
      super();
      this._keyCounter = 0;
      this._nextEntryId = 1;
      this._synced = false;
      // key → NavigationHistoryEntry, so the same entry is the same object
      // across `currentEntry`/`entries()` reads.
      this._cache = new Map();
      this._list = [];
      this._current = null;
      this._rawSeen = null;
      // key → Navigation API state (already a structured clone).
      this._states = new Map();
      // Keys that were current at some point in this document's lifetime —
      // a cross-document navigation discards the realm, so these are exactly
      // the entries of this document (`NavigationHistoryEntry.sameDocument`).
      this._docKeys = new Set();
      // `navigate(url, {state})` in flight: attached to the entry the shell
      // commits next.
      this._pendingState = undefined;
      this._hasPendingState = false;
      // What the next shell-fired `currententrychange` reports.
      this._changeFrom = null;
      this._changeType = null;
    }

    /** Build a fresh key string. */
    _mkKey() { return String(this._keyCounter++); }
    _mkId()  { return 'id-' + String(this._nextEntryId++); }

    // ── shell-backed state accessors ──────────────────────────────────────────

    get _shellEntries() {
      try {
        const raw = _lumen_navigation_entries_json();
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) return parsed;
        return Array.isArray(parsed.entries) ? parsed.entries : [];
      } catch { return []; }
    }

    get _currentIndex() {
      try { return _lumen_navigation_current_index(); } catch { return 0; }
    }

    /// Reconcile the cached entry objects with the shell's current stacks.
    /// Cheap when nothing changed (the raw JSON is compared first).
    _sync() {
      let raw = '';
      try { raw = _lumen_navigation_entries_json(); } catch { raw = ''; }
      if (raw === this._rawSeen) return;
      this._rawSeen = raw;
      const shell = this._shellEntries;
      // No state published yet: the shell seeds the stacks only once the load
      // has finished (`apply_loaded_page` → `commit_nav_state`), yet parse-time
      // scripts and `onload` already read `currentEntry`, which the spec never
      // makes `null` for a fully active document. Stand in with one entry for
      // this document; the first real publish adopts it (below), so the page
      // keeps the same object.
      if (!shell.length) {
        this._rawSeen = null;
        if (!this._current) {
          let href = '';
          try { href = String(window.location.href); } catch { href = ''; }
          const ent = new NavigationHistoryEntry(TOKEN, 'initial', 'initial', href);
          ent._index = 0;
          ent._provisional = true;
          this._cache.set(ent._key, ent);
          this._list = [ent];
          this._current = ent;
          this._docKeys.add(ent._key);
        }
        return;
      }
      const idx = this._currentIndex;
      this._adoptProvisional(shell, idx);
      const oldCurrent = this._current;
      const oldKeys = new Set(this._list.map(e => e._key));
      const oldIndex = oldCurrent ? oldCurrent._index : -1;
      const list = [];
      const live = new Set();
      for (let i = 0; i < shell.length; i++) {
        const e = shell[i] || {};
        const key = String(e.key || '');
        const id = String(e.id || '');
        let ent = this._cache.get(key);
        if (ent && ent._id !== id) {
          // Same key, new id: a replaced entry is a new entry (§7.2.9.3).
          this._dropEntry(ent);
          ent = undefined;
        }
        if (!ent) {
          ent = new NavigationHistoryEntry(TOKEN, key, id, String(e.url || ''));
          this._cache.set(key, ent);
        }
        ent._url = String(e.url || '');
        ent._index = i;
        live.add(key);
        list.push(ent);
      }
      const disposed = [];
      for (const [key, ent] of this._cache) {
        if (!live.has(key)) disposed.push(ent);
      }
      for (const ent of disposed) this._dropEntry(ent);
      this._list = list;
      this._current = (idx >= 0 && idx < list.length) ? list[idx] : null;
      const cur = this._current;
      if (cur) {
        this._docKeys.add(cur._key);
        if (cur !== oldCurrent) {
          if (this._hasPendingState && !oldKeys.has(cur._key)) {
            this._states.set(cur._key, this._pendingState);
          }
          this._hasPendingState = false;
          this._pendingState = undefined;
          if (oldCurrent) {
            this._changeFrom = oldCurrent;
            if (oldKeys.has(cur._key)) this._changeType = 'traverse';
            else if (oldCurrent._index === -1 && cur._index === oldIndex) this._changeType = 'replace';
            else this._changeType = 'push';
          }
        }
      }
      for (const ent of disposed) {
        ent.dispatchEvent(trusted(new Event('dispose')));
      }
    }

    /// Re-key the stand-in initial entry onto the shell's entry for this
    /// document: the one with the same URL, nearest to the current index
    /// going back (a `pushState` made before the first publish sits after it).
    _adoptProvisional(shell, idx) {
      const prov = this._cache.get('initial');
      if (!prov || !prov._provisional) return;
      let hit = -1;
      for (let i = Math.min(idx, shell.length - 1); i >= 0; i--) {
        if (shell[i] && String(shell[i].url || '') === prov._url) { hit = i; break; }
      }
      if (hit < 0) hit = Math.min(Math.max(idx, 0), shell.length - 1);
      const e = shell[hit] || {};
      const key = String(e.key || '');
      this._cache.delete('initial');
      this._docKeys.delete('initial');
      prov._key = key;
      prov._id = String(e.id || '');
      prov._provisional = false;
      if (this._states.has('initial')) {
        this._states.set(key, this._states.get('initial'));
        this._states.delete('initial');
      }
      this._cache.set(key, prov);
      this._docKeys.add(key);
    }

    _dropEntry(ent) {
      this._cache.delete(ent._key);
      this._states.delete(ent._key);
      ent._index = -1;
      ent._disposed = true;
      // The disposed entry must not be reported as the from-entry of a later
      // change either; it stays reachable only through the page's own refs.
    }

    /// HTML LS §7.2.9.1: the ongoing intercepted transition, else `null`
    /// (never `undefined` — pages read `transition.from` after a `=== null`
    /// check).
    get transition() { return this._transition || null; }

    /// HTML LS §7.2.9.1 `NavigationActivation`; not modelled — `null`.
    get activation() { return null; }

    get currentEntry() {
      this._sync();
      return this._current;
    }

    entries() {
      this._sync();
      return this._list.slice();
    }

    canGoBack()    { try { return _lumen_navigation_can_go_back(); }    catch { return false; } }
    canGoForward() { try { return _lumen_navigation_can_go_forward(); } catch { return false; } }

    /// HTML LS §7.2.9.4 `updateCurrentEntry(options)`: replaces the current
    /// entry's Navigation API state in place — no navigation, no `navigate`
    /// event — and fires `currententrychange` with `navigationType: null`.
    updateCurrentEntry(options) {
      if (options !== undefined && options !== null &&
          typeof options !== 'object' && typeof options !== 'function') {
        throw new TypeError(
          "Failed to execute 'updateCurrentEntry' on 'Navigation': " +
          'parameter 1 is not of type NavigationUpdateCurrentEntryOptions.');
      }
      const state = (options === undefined || options === null) ? undefined : options.state;
      if (state === undefined) {
        throw new TypeError(
          "Failed to execute 'updateCurrentEntry' on 'Navigation': " +
          "required member 'state' is undefined.");
      }
      const current = this.currentEntry;
      if (!current) {
        throw new DOMException(
          "Failed to execute 'updateCurrentEntry' on 'Navigation': " +
          'the current entry is not available.', 'InvalidStateError');
      }
      // Throws DataCloneError for an unserializable state, before any change.
      const serialized = structuredClone(state);
      this._states.set(current._key, serialized);
      this.dispatchEvent(trusted(new NavigationCurrentEntryChangeEvent(
        'currententrychange', { navigationType: null, from: current })));
    }

    // ── navigation methods (fire navigate event, shell commits) ─────────────

    navigate(url, options = {}) {
      const opts = options || {};
      const state = opts.state;
      // `history: 'replace'` is the spec option (§7.2.9.4); `replace: true`
      // is the pre-standard spelling this shim used to read — kept.
      const replace = opts.history === 'replace' || opts.replace === true;
      // Serialize first: an unserializable state throws DataCloneError
      // synchronously, before anything is queued.
      const serialized = state !== undefined ? structuredClone(state) : undefined;
      this._pendingState = serialized;
      this._hasPendingState = state !== undefined;
      const key = this._mkKey();
      this._mkId();
      const stateJson = JSON.stringify(state !== undefined ? state : null);
      return this._request(replace ? 1 : 0, url, key, stateJson, null);
    }

    back(options = {}) {
      if (!this.canGoBack()) return this._rejected('Cannot go back');
      return this._request(2, '', '', '', null);
    }

    forward(options = {}) {
      if (!this.canGoForward()) return this._rejected('Cannot go forward');
      return this._request(3, '', '', '', null);
    }

    traverseTo(key, options = {}) {
      key = String(key);
      this._sync();
      if (!this._cache.has(key)) return this._rejected('Invalid key');
      return this._request(4, '', key, '', key);
    }

    /// HTML LS §7.2.9.4: the navigation methods return a
    /// `NavigationResult` dictionary synchronously — `{committed, finished}` —
    /// not a promise of one, so `navigation.navigate(u).committed` works.
    /// The shell commits on its next turn; both promises settle from a task
    /// after that with the entry that is current then.
    _request(action, url, key, data, expectKey) {
      let resolveC, rejectC, resolveF, rejectF;
      const committed = new Promise((res, rej) => { resolveC = res; rejectC = rej; });
      const finished = new Promise((res, rej) => { resolveF = res; rejectF = rej; });
      // A rejected `finished` is reported through `committed` too; mark it
      // handled so a page that only awaits `committed` gets no unhandled
      // rejection (the spec marks both promises as handled).
      finished.catch(() => {});
      committed.catch(() => {});
      try {
        _lumen_navigation_request(action, url, key, data);
      } catch (e) {
        const err = new DOMException('Navigation queue full', 'InvalidStateError');
        rejectC(err); rejectF(err);
        return { committed, finished };
      }
      // Settle from the shell's outcome events (`navigatesuccess` /
      // `navigateerror` / `currententrychange`, see the wire-up below). A
      // newer request supersedes this one: HTML LS §7.2.9.4 aborts the
      // ongoing navigation with an AbortError.
      if (this._ongoing) this._ongoing.fail();
      const before = this._current ? this._current._key : null;
      const self = this;
      const ongoing = {
        done: false,
        awaitEvents: false,
        ok(entry) {
          if (this.done) return;
          this.done = true;
          if (self._ongoing === this) self._ongoing = null;
          if (expectKey !== null && entry && entry.key !== expectKey) {
            const err = new DOMException('The navigation was aborted', 'AbortError');
            rejectC(err); rejectF(err);
            return;
          }
          resolveC(entry); resolveF(entry);
        },
        fail(err) {
          if (this.done) return;
          this.done = true;
          if (self._ongoing === this) self._ongoing = null;
          err = err || new DOMException('The navigation was aborted', 'AbortError');
          rejectC(err); rejectF(err);
        }
      };
      this._ongoing = ongoing;
      // Fallback for commits that fire no outcome event (a plain push or
      // traversal the shell applies without intercept): once the shell had
      // two turns, settle with the entry current then. A navigation whose
      // `navigate` event was intercepted or canceled (`awaitEvents`) is left
      // to `navigatesuccess`/`navigateerror` instead.
      setTimeout(() => {
        setTimeout(() => {
          if (ongoing.done || ongoing.awaitEvents) return;
          const entry = this.currentEntry;
          if (entry) ongoing.ok(entry); else ongoing.fail();
        }, 0);
      }, 0);
      return { committed, finished };
    }

    _rejected(message) {
      const err = new DOMException(message, 'InvalidStateError');
      const committed = Promise.reject(err);
      const finished = Promise.reject(err);
      committed.catch(() => {});
      finished.catch(() => {});
      return { committed, finished };
    }
  }
  for (const t of ['navigate', 'navigatesuccess', 'navigateerror', 'currententrychange']) {
    defineHandlerAttr(Navigation.prototype, t);
  }

  // Create global singleton
  const navigation = new Navigation();

  // Install on window and globalThis
  if (typeof window !== 'undefined') {
    Object.defineProperty(window, 'navigation', {
      value: navigation,
      writable: false,
      enumerable: true,
      configurable: false
    });
  }

  if (typeof globalThis !== 'undefined' && globalThis.navigation !== navigation) {
    Object.defineProperty(globalThis, 'navigation', {
      value: navigation,
      writable: false,
      enumerable: true,
      configurable: false
    });
  }

  // ── Navigation API shell wire-up ──────────────────────────────────────────
  window._lumen_pending_intercept_handler = null;

  // Every shell publish resyncs the entry objects right away, so `dispose`
  // fires when the entry leaves the stacks rather than on the next read.
  if (typeof _lumen_navigation_set_state === 'function') {
    const nativeSetState = _lumen_navigation_set_state;
    globalThis._lumen_navigation_set_state = function(json) {
      nativeSetState(json);
      navigation._sync();
    };
  }

  // `destKey` (optional) names the target entry of a traversal; for
  // push/replace/fragment the destination is a URL with no entry yet.
  window._lumen_dispatch_navigate = function(type, url, canIntercept, hashChange, destKey) {
    var destination = null;
    var target = destKey ? (navigation._sync(), navigation._cache.get(String(destKey))) : null;
    if (target) {
      destination = new NavigationDestination(TOKEN, {
        url: target._url, key: target._key, id: target._id, index: target._index,
        sameDocument: navigation._docKeys.has(target._key),
        state: navigation._states.get(target._key)
      });
    } else if (url) {
      var href = null;
      try { href = new URL(url, window.location.href).href; } catch (e) {}
      if (href !== null) {
        destination = new NavigationDestination(TOKEN, {
          url: href,
          sameDocument: !!hashChange,
          state: navigation._hasPendingState ? navigation._pendingState : undefined
        });
      }
    }
    var event = new NavigateEvent({
      navigationType: type,
      destination: destination,
      hashChange: hashChange,
      canIntercept: canIntercept,
      signal: new AbortSignal()
    });
    window.navigation.dispatchEvent(event);
    // BUG-1031: `canIntercept === false` navigations are dispatched purely
    // for observability (listeners may still read `destination`/log it) —
    // `intercept()` already throws above, and `preventDefault()` is a no-op
    // because the event isn't cancelable, but read the outcome defensively
    // too rather than trusting every call site got there first.
    if (!canIntercept) return false;
    if (event._isIntercepted()) {
      if (navigation._ongoing) navigation._ongoing.awaitEvents = true;
      if (navigation._transition) {
        const err = new DOMException('The navigation was aborted', 'AbortError');
        navigation._transition._rejC(err); navigation._transition._rejF(err);
      }
      navigation._transition = new NavigationTransition(TOKEN, {
        navigationType: type, from: navigation.currentEntry, to: destination });
      window._lumen_navigation_report_intercept(true, false);
      return true;
    }
    if (event.defaultPrevented) {
      if (navigation._ongoing) navigation._ongoing.awaitEvents = true;
      // HTML LS §7.2.9.8 "inner navigate event firing": a canceled event
      // aborts its signal (before `navigateerror`, which the shell fires).
      if (typeof _lumen_abort_signal_fire === 'function' && !event.signal.aborted) {
        _lumen_abort_signal_fire(event.signal,
          new DOMException('The navigation was aborted', 'AbortError'));
      }
      window._lumen_navigation_report_intercept(false, true);
      return true;
    }
    return false;
  };

  window._lumen_run_navigate_handler = function() {
    if (!window._lumen_pending_intercept_handler) return Promise.resolve();
    var handler = window._lumen_pending_intercept_handler;
    window._lumen_pending_intercept_handler = null;
    return Promise.resolve().then(handler).then(function(result) {
      var data = result || {};
      _lumen_navigation_request(
        6,
        data.url || '',
        '',
        JSON.stringify({ state: data.state || null, title: data.title || '' })
      );
    }).catch(function() {
      _lumen_navigation_request(7, '', '', '');
    });
  };

  window._lumen_fire_navigate_success = function() {
    window.navigation.dispatchEvent(trusted(new Event('navigatesuccess')));
    // §7.2.9.9: finished settles after the event, then the transition ends.
    const tr = navigation._transition;
    navigation._transition = null;
    if (tr) { tr._resC(); tr._resF(); }
    if (navigation._ongoing) navigation._ongoing.ok(navigation.currentEntry);
  };

  window._lumen_fire_navigate_error = function() {
    navigation._hasPendingState = false;
    navigation._pendingState = undefined;
    window.navigation.dispatchEvent(trusted(new Event('navigateerror')));
    const tr = navigation._transition;
    navigation._transition = null;
    const err = new DOMException('The navigation was aborted', 'AbortError');
    if (tr) { tr._rejC(err); tr._rejF(err); }
    if (navigation._ongoing) navigation._ongoing.fail(err);
  };

  // The shell publishes the new stacks (`_lumen_navigation_set_state`) before
  // firing this, so `navigation.currentEntry` is already the new entry and
  // `_sync` has recorded the one it replaced.
  window._lumen_fire_currententrychange = function() {
    navigation._sync();
    var from = navigation._changeFrom || navigation._current;
    var type = navigation._changeFrom ? navigation._changeType : null;
    navigation._changeFrom = null;
    navigation._changeType = null;
    if (!from) return;
    window.navigation.dispatchEvent(trusted(new NavigationCurrentEntryChangeEvent(
      'currententrychange', { navigationType: type, from: from })));
    if (navigation._ongoing && !window._lumen_pending_intercept_handler
        && navigation.currentEntry !== from) {
      navigation._ongoing.ok(navigation.currentEntry);
    }
  };

  // Export classes. Interface objects are non-enumerable (WebIDL §3.7).
  function exportIface(name, value) {
    Object.defineProperty(globalThis, name,
      { value: value, writable: true, enumerable: false, configurable: true });
  }
  // The shim-internal token check makes script construction throw; the
  // exported names are the classes themselves so `instanceof` and
  // `e.constructor === X` hold.
  exportIface('NavigationHistoryEntry', NavigationHistoryEntry);
  exportIface('NavigationDestination', NavigationDestination);
  exportIface('NavigationTransition', NavigationTransition);
  exportIface('NavigateEvent', NavigateEvent);
  exportIface('NavigationCurrentEntryChangeEvent', NavigationCurrentEntryChangeEvent);
  exportIface('Navigation', Navigation);
})();
"#;
