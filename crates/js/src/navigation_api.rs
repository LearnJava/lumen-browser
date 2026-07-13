/// Navigation API (HTML LS §7.8).
///
/// Provides `window.navigation` singleton with `currentEntry`, `entries()`,
/// `navigate()`, `back()`, `forward()`, `traverseTo()` methods and events
/// `navigate`, `navigatesuccess`, `navigateerror`, `currententrychange`.
use rquickjs::Ctx;

/// Install Navigation API into the JS context.
///
/// Defines `globalThis.window.navigation` (or `globalThis.navigation`) singleton.
pub fn install_navigation_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(NAVIGATION_API_SHIM)?;
    Ok(())
}

/// V8 port of [`install_navigation_api`] (Ph3 V8 migration S5-S7): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_navigation_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(NAVIGATION_API_SHIM)?;
    Ok(())
}

/// JavaScript shim: Navigation singleton with history entries and event handling.
const NAVIGATION_API_SHIM: &str = r#"(function() {
  'use strict';

/// NavigationHistoryEntry class: represents a single entry in the navigation history.
/// Backed by the shell's `nav_back`/`nav_fwd` stacks — **not** an in-JS mirror.
class NavigationHistoryEntry {
  constructor(url, key, id, index) {
    this._url = url;
    this._key = key;
    this._id = id;
    this._index = index;
    this._state = null;
  }

  get url()      { return this._url; }
  get key()      { return this._key; }
  get id()       { return this._id; }
  get index()    { return this._index; }

  getState()     { return this._state; }
  _setState(s)   { this._state = s; }
}

  /// NavigateEvent: fired before navigation.
  class NavigateEvent extends Event {
    constructor(init = {}) {
      super('navigate');
      this._navigationType = init.navigationType || 'push';
      this._userInitiated = init.userInitiated || false;
      this._hashChange = init.hashChange || false;
      this._signal = init.signal || new AbortSignal();
      this._destination = init.destination || null;
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

    intercept(options = {}) {
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

  /// Navigation singleton class.
  /// State is read from the shell; mutations are sent to the shell (single authority).
  class Navigation extends EventTarget {
    constructor() {
      super();
      this._keyCounter = 0;
      this._nextEntryId = 1;
      this._synced = false;
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

    get currentEntry() {
      const entries = this._shellEntries;
      const i = this._currentIndex;
      if (!entries.length || i < 0 || i >= entries.length) return null;
      const e = entries[i];
      const entry = new NavigationHistoryEntry(e.url, e.key, e.id, i);
      entry._setState(e.state);
      return entry;
    }

    entries() {
      const entries = this._shellEntries;
      const i = this._currentIndex;
      return entries.map((e, idx) => {
        const entry = new NavigationHistoryEntry(e.url, e.key, e.id, idx);
        entry._setState(e.state);
        return entry;
      });
    }

    canGoBack()    { try { return _lumen_navigation_can_go_back(); }    catch { return false; } }
    canGoForward() { try { return _lumen_navigation_can_go_forward(); } catch { return false; } }

    // ── navigation methods (fire navigate event, shell commits) ─────────────

    navigate(url, options = {}) {
      const {state, replace} = options;
      return new Promise((resolve, reject) => {
        // Enqueue the request for the shell to process.
        const key = this._mkKey();
        const id  = this._mkId();
        const stateJson = JSON.stringify(state !== undefined ? state : null);
        const action = replace ? 1 : 0;
        try {
          _lumen_navigation_request(action, url, key, stateJson);
        } catch { reject(new Error('Navigation queue full')); return; }

        // Allow the shell to process and fire events.
        setTimeout(() => {
          try {
            const newEntries = this._shellEntries;
            const newIdx = this._currentIndex;
            const entry = newEntries[newIdx];
            if (!entry || entry.key !== key) {
              // Shell either recycled the queue or navigated elsewhere; resolve anyway.
              resolve({ committed: Promise.resolve(this.currentEntry), finished: Promise.resolve(this.currentEntry) });
              return;
            }
            const entryObj = new NavigationHistoryEntry(entry.url, entry.key, entry.id, newIdx);
            entryObj._setState(entry.state);
            resolve({ committed: Promise.resolve(entryObj), finished: Promise.resolve(entryObj) });
          } catch { resolve({ committed: Promise.resolve(this.currentEntry), finished: Promise.resolve(this.currentEntry) }); }
        }, 0);
      });
    }

    back(options = {}) {
      return this._traverseBy(-1, options);
    }

    forward(options = {}) {
      return this._traverseBy(1, options);
    }

    traverseTo(key, options = {}) {
      return new Promise((resolve, reject) => {
        try {
          _lumen_navigation_request(4, '', key, '');
        } catch { reject(new Error('Navigation queue full')); return; }
        setTimeout(() => {
          const entries = this._shellEntries;
          const i = this._currentIndex;
          const entry = entries[i];
          if (!entry || entry.key !== key) {
            reject(new Error('Traversal target no longer valid'));
            return;
          }
          const obj = new NavigationHistoryEntry(entry.url, entry.key, entry.id, i);
          obj._setState(entry.state);
          resolve({ committed: Promise.resolve(obj), finished: Promise.resolve(obj) });
        }, 0);
      });
    }

    _traverseBy(delta, options = {}) {
      return new Promise((resolve, reject) => {
        setTimeout(() => {
          // navigation.back()/forward() only ever use ±1.
          try {
            _lumen_navigation_request(delta < 0 ? 2 : 3, '', '', '');
          } catch { reject(new Error('Navigation queue full')); return; }
          setTimeout(() => {
            const entries = this._shellEntries;
            const i = this._currentIndex;
            const e = entries[i];
            if (!e) { reject(new Error('No current entry after traversal')); return; }
            const obj = new NavigationHistoryEntry(e.url, e.key, e.id, i);
            obj._setState(e.state);
            resolve({ committed: Promise.resolve(obj), finished: Promise.resolve(obj) });
          }, 0);
        }, 0);
      });
    }
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

  if (typeof globalThis !== 'undefined') {
    Object.defineProperty(globalThis, 'navigation', {
      value: navigation,
      writable: false,
      enumerable: true,
      configurable: false
    });
  }

  // ── Navigation API shell wire-up ──────────────────────────────────────────
  window._lumen_pending_intercept_handler = null;

  window._lumen_dispatch_navigate = function(type, url, canIntercept, hashChange) {
    var destination = null;
    if (url) {
      try { destination = new URL(url, window.location.href); } catch (e) {}
    }
    var event = new NavigateEvent({
      navigationType: type,
      destination: destination,
      hashChange: hashChange,
      signal: new AbortSignal()
    });
    window.navigation.dispatchEvent(event);
    if (event._isIntercepted()) {
      window._lumen_navigation_report_intercept(true, false);
      return true;
    }
    if (event.defaultPrevented) {
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
    window.navigation.dispatchEvent(new Event('navigatesuccess'));
  };

  window._lumen_fire_navigate_error = function() {
    window.navigation.dispatchEvent(new Event('navigateerror'));
  };

  window._lumen_fire_currententrychange = function() {
    window.navigation.dispatchEvent(new Event('currententrychange'));
  };

  // Export classes
  globalThis.NavigationHistoryEntry = NavigationHistoryEntry;
  globalThis.NavigateEvent = NavigateEvent;
  globalThis.Navigation = Navigation;
})();
"#;

