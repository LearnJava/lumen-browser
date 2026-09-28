//! Soft Navigation Timing API (W3C Soft Navigations Level 1).
//!
//! A "soft navigation" is a user-interaction-triggered client-side navigation that
//! changes the URL and renders new content without a full page reload (e.g., SPA
//! route changes via History API or Navigation API).
//!
//! Phase 0 exposes:
//! - `PerformanceSoftNavigationEntry` class (entryType = `'soft-navigation'`).
//! - `_lumen_deliver_soft_nav(url, startTime, durationMs)` — shell hook to record
//!   a soft navigation and notify `PerformanceObserver` subscribers.
//!
//! The entry is inserted into `performance._perf_entries` (same slot used by other
//! performance entries) so that `performance.getEntriesByType('soft-navigation')`
//! works correctly.

/// V8 port of the former rquickjs `install_soft_navigation_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B2): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_soft_navigation_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SOFT_NAVIGATION_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SOFT_NAVIGATION_SHIM: &str = r#"(function() {
  'use strict';

  // ── PerformanceSoftNavigationEntry ────────────────────────────────────────
  // W3C Soft Navigations §4.2

  if (typeof PerformanceSoftNavigationEntry === 'undefined') {
    function PerformanceSoftNavigationEntry(init) {
      init = init || {};
      this.entryType  = 'soft-navigation';
      this.name       = init.name || '';
      this.startTime  = init.startTime || 0;
      this.duration   = init.duration  || 0;
      // Soft navigation-specific fields
      this.navigationId = init.navigationId || '';
    }
    PerformanceSoftNavigationEntry.prototype.toJSON = function() {
      return {
        entryType:    this.entryType,
        name:         this.name,
        startTime:    this.startTime,
        duration:     this.duration,
        navigationId: this.navigationId
      };
    };
    globalThis.PerformanceSoftNavigationEntry = PerformanceSoftNavigationEntry;
  }

  // ── _lumen_deliver_soft_nav ───────────────────────────────────────────────
  // Shell hook: called after a history.pushState / Navigation API navigation
  // that qualifies as a soft navigation (URL change + user interaction).
  // Arguments:
  //   url        — new URL (used as entry.name)
  //   startTime  — navigation start timestamp (ms, same epoch as performance.now())
  //   durationMs — time until largest contentful paint or DOMContentLoaded (Phase 0: 0)

  globalThis._lumen_deliver_soft_nav = function(url, startTime, durationMs) {
    var entry = new PerformanceSoftNavigationEntry({
      name:         url || '',
      startTime:    startTime  || 0,
      duration:     durationMs || 0,
      navigationId: String(Date.now())
    });

    // The page shim's performance entry buffer and §5.1 «queue a
    // PerformanceEntry» (`web_api_shim_tail.js`), the same pair every other
    // entry type goes through (BUG-678). `performance._perf_entries` /
    // `performance._observers`, which this hook used to write, exist nowhere,
    // so even a wired caller would have produced an entry nobody could read.
    if (typeof _perf_entries !== 'undefined') _perf_entries.push(entry);
    if (typeof _perf_observer_notify === 'function') _perf_observer_notify([entry]);
  };

})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_soft_navigation(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        install_soft_navigation_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn soft_nav_entry_class_exists() {
        with_soft_navigation(|rt| {
            let ok = rt
                .eval("typeof PerformanceSoftNavigationEntry === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn soft_nav_entry_constructor() {
        with_soft_navigation(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var e = new PerformanceSoftNavigationEntry({name: '/about', startTime: 100, duration: 50});
                    e.entryType === 'soft-navigation'
                      && e.name      === '/about'
                      && e.startTime === 100
                      && e.duration  === 50
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    // Delivery into the real performance timeline is covered with the page
    // shim loaded — `dom/tests/v8_perf_observers.rs`
    // (`soft_nav_hook_feeds_the_performance_timeline`); without it the hook
    // must stay a harmless no-op.
    #[test]
    fn deliver_soft_nav_without_page_shim_is_noop() {
        with_soft_navigation(|rt| {
            let ok = rt
                .eval("_lumen_deliver_soft_nav('/home', 0, 0); true")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn soft_nav_entry_to_json() {
        with_soft_navigation(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var e = new PerformanceSoftNavigationEntry({name: '/x', startTime: 5, duration: 10});
                    var j = e.toJSON();
                    j.entryType === 'soft-navigation' && j.name === '/x' && j.startTime === 5
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
