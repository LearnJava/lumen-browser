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

use rquickjs::Ctx;

/// Install Soft Navigation Timing API stubs into the JS context.
///
/// Must run after the DOM shim so that `performance` and `PerformanceObserver`
/// are already defined.
pub fn install_soft_navigation_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SOFT_NAVIGATION_SHIM)?;
    Ok(())
}

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

    // Insert into performance entries (same bucket as PerformanceObserver reads).
    if (typeof performance !== 'undefined' && Array.isArray(performance._perf_entries)) {
      performance._perf_entries.push(entry);
    }

    // Notify PerformanceObserver subscribers for 'soft-navigation'.
    if (typeof performance !== 'undefined' && Array.isArray(performance._observers)) {
      var observers = performance._observers;
      for (var i = 0; i < observers.length; i++) {
        var obs = observers[i];
        if (!obs._types || obs._types.indexOf('soft-navigation') >= 0) {
          try { obs._callback([entry]); } catch(e) {}
        }
      }
    }
  };

})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_prereqs(ctx: &rquickjs::Ctx) {
        install_soft_navigation_api(ctx).unwrap();
    }

    #[test]
    fn soft_nav_entry_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval("typeof PerformanceSoftNavigationEntry === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn soft_nav_entry_constructor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
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
            assert!(ok);
        });
    }

    #[test]
    fn deliver_soft_nav_inserts_entry() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            // Run setup and assertion in a single eval so shared vars are visible
            let ok: bool = ctx
                .eval(
                    r#"
                    var _perf = { _perf_entries: [], _observers: [] };
                    var _origPerf = typeof performance !== 'undefined' ? performance : null;
                    // Temporarily override performance for this test
                    var savedPerf = globalThis.performance;
                    globalThis.performance = _perf;
                    _lumen_deliver_soft_nav('/home', 0, 0);
                    var _ok = _perf._perf_entries.length === 1
                      && _perf._perf_entries[0].entryType === 'soft-navigation'
                      && _perf._perf_entries[0].name === '/home';
                    globalThis.performance = savedPerf;
                    _ok
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deliver_soft_nav_notifies_observer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var _perf2 = { _perf_entries: [], _observers: [] };
                    var _notified = false;
                    _perf2._observers.push({
                      _types: ['soft-navigation'],
                      _callback: function(entries) { _notified = entries.length === 1; }
                    });
                    var savedPerf2 = globalThis.performance;
                    globalThis.performance = _perf2;
                    _lumen_deliver_soft_nav('/page', 10, 200);
                    globalThis.performance = savedPerf2;
                    _notified
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn soft_nav_entry_to_json() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var e = new PerformanceSoftNavigationEntry({name: '/x', startTime: 5, duration: 10});
                    var j = e.toJSON();
                    j.entryType === 'soft-navigation' && j.name === '/x' && j.startTime === 5
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
