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

/// JavaScript shim: Navigation singleton with history entries and event handling.
const NAVIGATION_API_SHIM: &str = r#"(function() {
  'use strict';

  /// NavigationHistoryEntry class: represents a single entry in the navigation history.
  class NavigationHistoryEntry {
    constructor(url, key, id, index) {
      this._url = url;
      this._key = key;
      this._id = id;
      this._index = index;
      this._state = null;
    }

    get url() {
      return this._url;
    }

    get key() {
      return this._key;
    }

    get id() {
      return this._id;
    }

    get index() {
      return this._index;
    }

    getState() {
      return this._state;
    }

    _setState(state) {
      this._state = state;
    }
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
    }

    _isIntercepted() {
      return this._intercepted;
    }

    _getHandledPromise() {
      return this._handledPromise;
    }
  }

  /// Navigation singleton class.
  class Navigation extends EventTarget {
    constructor() {
      super();
      this._currentIndex = 0;
      this._entries = [];
      this._nextKeyId = 1;
      this._nextEntryId = 1;

      // Initialize with single blank entry
      const initialEntry = new NavigationHistoryEntry(
        window.location.href,
        'key-0',
        'id-0',
        0
      );
      this._entries.push(initialEntry);
    }

    get currentEntry() {
      return this._entries[this._currentIndex] || null;
    }

    entries() {
      return [...this._entries];
    }

    navigate(url, options = {}) {
      const {state, replace} = options;

      return new Promise((resolve, reject) => {
        setTimeout(() => {
          const navigateEvent = new NavigateEvent({
            navigationType: replace ? 'replace' : 'push',
            userInitiated: true,
            destination: {
              url: url,
              key: '',
              id: ''
            }
          });

          // Dispatch navigate event
          const dispatchResult = this.dispatchEvent(navigateEvent);

          if (!dispatchResult) {
            // Event was cancelled
            const errorEvent = new Event('navigateerror');
            errorEvent.error = new Error('Navigation cancelled');
            this.dispatchEvent(errorEvent);
            reject(new Error('Navigation cancelled'));
            return;
          }

          // Wait for intercept handler if present
          navigateEvent._getHandledPromise()
            .then(() => {
              // Perform navigation
              const newEntry = new NavigationHistoryEntry(
                url,
                'key-' + (this._nextKeyId++),
                'id-' + (this._nextEntryId++),
                replace ? this._currentIndex : this._currentIndex + 1
              );
              newEntry._setState(state);

              if (replace) {
                this._entries[this._currentIndex] = newEntry;
              } else {
                // Remove forward history if present
                this._entries = this._entries.slice(0, this._currentIndex + 1);
                this._entries.push(newEntry);
                this._currentIndex++;
              }

              // Dispatch navigatesuccess event
              this.dispatchEvent(new Event('navigatesuccess'));
              this.dispatchEvent(new Event('currententrychange'));

              resolve({
                committed: Promise.resolve(this.currentEntry),
                finished: Promise.resolve(this.currentEntry)
              });
            })
            .catch((error) => {
              // Dispatch navigateerror event
              const errorEvent = new Event('navigateerror');
              errorEvent.error = error;
              this.dispatchEvent(errorEvent);
              reject(error);
            });
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
      const entry = this._entries.find((e) => e.key === key);
      if (!entry) {
        return Promise.reject(new Error('No entry with key: ' + key));
      }

      const delta = this._entries.indexOf(entry) - this._currentIndex;
      return this._traverseBy(delta, options);
    }

    _traverseBy(delta, options = {}) {
      return new Promise((resolve, reject) => {
        setTimeout(() => {
          const newIndex = this._currentIndex + delta;

          if (newIndex < 0 || newIndex >= this._entries.length) {
            reject(new Error('Cannot traverse beyond history bounds'));
            return;
          }

          const oldEntry = this.currentEntry;
          const newEntry = this._entries[newIndex];

          const navigateEvent = new NavigateEvent({
            navigationType: delta > 0 ? 'forward' : 'back',
            userInitiated: true,
            destination: {
              url: newEntry.url,
              key: newEntry.key,
              id: newEntry.id
            }
          });

          const dispatchResult = this.dispatchEvent(navigateEvent);

          if (!dispatchResult) {
            const errorEvent = new Event('navigateerror');
            errorEvent.error = new Error('Traversal cancelled');
            this.dispatchEvent(errorEvent);
            reject(new Error('Traversal cancelled'));
            return;
          }

          navigateEvent._getHandledPromise()
            .then(() => {
              this._currentIndex = newIndex;
              this.dispatchEvent(new Event('navigatesuccess'));
              this.dispatchEvent(new Event('currententrychange'));

              resolve({
                committed: Promise.resolve(newEntry),
                finished: Promise.resolve(newEntry)
              });
            })
            .catch((error) => {
              const errorEvent = new Event('navigateerror');
              errorEvent.error = error;
              this.dispatchEvent(errorEvent);
              reject(error);
            });
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

  // Export classes
  globalThis.NavigationHistoryEntry = NavigationHistoryEntry;
  globalThis.NavigateEvent = NavigateEvent;
  globalThis.Navigation = Navigation;
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::JsRuntime as _;
    use lumen_dom::Document;
    use crate::QuickJsRuntime;
    use std::sync::{Arc, Mutex};

    fn make_rt() -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None)
            .unwrap();
        rt
    }

    fn bool_eval(rt: &QuickJsRuntime, script: &str) -> bool {
        match rt.eval(script) {
            Ok(lumen_core::JsValue::Bool(b)) => b,
            Ok(other) => panic!("expected bool from `{script}`, got {other:?}"),
            Err(e) => panic!("eval error in `{script}`: {e}"),
        }
    }

}
