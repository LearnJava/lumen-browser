//! W3C Long Animation Frames API (LoAF) — JS shim.
//!
//! Installs `PerformanceLongAnimationFrameTiming` and `PerformanceScriptTiming` classes
//! plus the `_lumen_deliver_long_animation_frame(...)` delivery binding.
//!
//! Phase 0 scope:
//! * `PerformanceLongAnimationFrameTiming` — all spec fields, entryType `"long-animation-frame"`.
//! * `PerformanceScriptTiming` — all spec fields, entryType `"script"`.
//! * `_lumen_deliver_long_animation_frame(start_ms, duration_ms, render_start,
//!   style_layout_start, first_ui_event_ts, blocking_duration_ms, scripts_json)`
//!   delivery binding for shell to report slow frames.
//! * PerformanceObserver subscribers to `"long-animation-frame"` receive entries.
//!
//! Phase 1: automatic detection of frames > 50 ms from the shell rendering loop.
//!
//! Spec: <https://w3c.github.io/long-animation-frames/>

use rquickjs::Ctx;

/// Install Long Animation Frames API into the QuickJS context.
///
/// Must be called after `install_dom_api` so that `PerformanceObserver`,
/// `_perf_entries`, and `_perf_observer_notify` are already in scope.
pub fn install_long_animation_frames_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(LOAF_SHIM)?;
    Ok(())
}

/// JS shim for the W3C Long Animation Frames API.
const LOAF_SHIM: &str = r#"(function() {
  'use strict';

  // ── PerformanceScriptTiming ─────────────────────────────────────────────────

  // W3C Long Animation Frames API §4 — per-script timing entry.
  //
  // Each slow frame may contain one or more script invocations that contributed
  // to the jank. PerformanceScriptTiming exposes the source and duration of each.
  function PerformanceScriptTiming(init) {
    var o = init || {};
    /// W3C entryType for script timings within a long frame.
    this.entryType             = 'script';
    this.name                  = 'long-animation-frame';
    /// Milliseconds since navigation start when script execution began.
    this.startTime             = Number(o.startTime)  || 0;
    /// Total script duration in milliseconds.
    this.duration              = Number(o.duration)   || 0;
    /// String describing the invoker: e.g. 'BUTTON#btn.onclick', 'setTimeout'.
    this.invoker               = typeof o.invoker             !== 'undefined' ? String(o.invoker)             : '';
    /// How the script was invoked: 'classic-script' | 'module-script' |
    /// 'event-listener' | 'user-callback' | 'resolve-promise' | 'reject-promise'.
    this.invokerType           = typeof o.invokerType         !== 'undefined' ? String(o.invokerType)         : 'classic-script';
    /// Which frame the script ran in: 'self' | 'descendant-iframe' | 'same-origin-ancestor-frame' | etc.
    this.windowAttribution     = typeof o.windowAttribution   !== 'undefined' ? String(o.windowAttribution)   : 'self';
    /// Time (ms) when JS bytecode execution started (after compilation/parse).
    this.executionStart        = Number(o.executionStart)     || 0;
    /// Milliseconds spent in forced style and layout within this script.
    this.forcedStyleAndLayoutDuration = Number(o.forcedStyleAndLayoutDuration) || 0;
    /// Milliseconds spent paused in synchronous operations within this script.
    this.pauseDuration         = Number(o.pauseDuration)      || 0;
    /// URL of the script source file.
    this.sourceURL             = typeof o.sourceURL           !== 'undefined' ? String(o.sourceURL)           : '';
    /// Name of the function that was invoked, or empty string for top-level code.
    this.sourceFunctionName    = typeof o.sourceFunctionName  !== 'undefined' ? String(o.sourceFunctionName)  : '';
    /// Character offset in sourceURL of the function entry point.
    this.sourceCharPosition    = Number(o.sourceCharPosition) || 0;
  }
  PerformanceScriptTiming.prototype.toJSON = function() {
    return {
      entryType: this.entryType, name: this.name,
      startTime: this.startTime, duration: this.duration,
      invoker: this.invoker, invokerType: this.invokerType,
      windowAttribution: this.windowAttribution,
      executionStart: this.executionStart,
      forcedStyleAndLayoutDuration: this.forcedStyleAndLayoutDuration,
      pauseDuration: this.pauseDuration,
      sourceURL: this.sourceURL,
      sourceFunctionName: this.sourceFunctionName,
      sourceCharPosition: this.sourceCharPosition
    };
  };

  globalThis.PerformanceScriptTiming = PerformanceScriptTiming;

  // ── PerformanceLongAnimationFrameTiming ────────────────────────────────────

  // W3C Long Animation Frames API §3 — a single slow animation frame (> 50 ms).
  //
  // Delivered to PerformanceObserver listeners subscribed to 'long-animation-frame'.
  // Contains the full breakdown of time spent in the frame plus an array of
  // PerformanceScriptTiming entries for contributing scripts.
  function PerformanceLongAnimationFrameTiming(init) {
    var o = init || {};
    /// W3C entryType; always 'long-animation-frame'.
    this.entryType             = 'long-animation-frame';
    this.name                  = 'long-animation-frame';
    /// Milliseconds since navigation start when the frame began.
    this.startTime             = Number(o.startTime)             || 0;
    /// Total frame duration in milliseconds (> 50 to qualify as a long frame).
    this.duration              = Number(o.duration)              || 0;
    /// Time (ms) when the rendering phase of this frame started.
    this.renderStart           = Number(o.renderStart)           || 0;
    /// Time (ms) when style recalculation and layout began in this frame.
    this.styleAndLayoutStart   = Number(o.styleAndLayoutStart)   || 0;
    /// Timestamp (ms) of the first UI event that was queued during this frame, or 0.
    this.firstUIEventTimestamp = Number(o.firstUIEventTimestamp) || 0;
    /// Duration (ms) beyond the 50 ms budget: max(0, duration − 50).
    this.blockingDuration = typeof o.blockingDuration !== 'undefined' && Number(o.blockingDuration) >= 0
      ? Number(o.blockingDuration)
      : Math.max(0, this.duration - 50);
    /// Array of PerformanceScriptTiming for each script that ran in this frame.
    var rawScripts = Array.isArray(o.scripts) ? o.scripts : [];
    this.scripts = rawScripts.map(function(s) {
      return s instanceof PerformanceScriptTiming ? s : new PerformanceScriptTiming(s);
    });
  }
  PerformanceLongAnimationFrameTiming.prototype.toJSON = function() {
    return {
      entryType: this.entryType, name: this.name,
      startTime: this.startTime, duration: this.duration,
      renderStart: this.renderStart,
      styleAndLayoutStart: this.styleAndLayoutStart,
      firstUIEventTimestamp: this.firstUIEventTimestamp,
      blockingDuration: this.blockingDuration,
      scripts: this.scripts.map(function(s) { return s.toJSON(); })
    };
  };

  globalThis.PerformanceLongAnimationFrameTiming = PerformanceLongAnimationFrameTiming;

  // ── Delivery binding ──────────────────────────────────────────────────────

  // Called by the shell to report a long animation frame.
  //
  // Parameters:
  //   start_ms              — frame start (performance.now() equivalent)
  //   duration_ms           — total frame duration; should be > 50 to qualify
  //   render_start          — rendering phase start, or 0
  //   style_layout_start    — style+layout start, or 0
  //   first_ui_event_ts     — first UI event timestamp, or 0
  //   blocking_duration_ms  — pre-computed blocking portion, or -1 to auto-compute
  //   scripts_json          — JSON array of PerformanceScriptTiming initialisers, or null
  globalThis._lumen_deliver_long_animation_frame = function(
    start_ms, duration_ms, render_start, style_layout_start,
    first_ui_event_ts, blocking_duration_ms, scripts_json
  ) {
    var scripts = [];
    if (scripts_json) {
      try { scripts = JSON.parse(scripts_json); } catch (_) {}
    }
    var bd = Number(blocking_duration_ms);
    var entry = new PerformanceLongAnimationFrameTiming({
      startTime:             Number(start_ms),
      duration:              Number(duration_ms),
      renderStart:           Number(render_start),
      styleAndLayoutStart:   Number(style_layout_start),
      firstUIEventTimestamp: Number(first_ui_event_ts),
      blockingDuration:      bd >= 0 ? bd : undefined,
      scripts:               scripts
    });

    if (typeof _perf_entries !== 'undefined') {
      _perf_entries.push(entry);
    }
    if (typeof _perf_observer_notify === 'function') {
      _perf_observer_notify([entry]);
    }
  };

})();
"#;

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Minimal performance infrastructure required to test observer delivery.
    const PERF_STUB: &str = r#"
        var _perf_entries = [];
        var _perf_observers = [];
        function _perf_observer_notify(entries) {
            for (var i = 0; i < _perf_observers.length; i++) {
                var obs = _perf_observers[i];
                for (var j = 0; j < entries.length; j++) {
                    if (obs._types.indexOf(entries[j].entryType) !== -1) {
                        var captured = entries;
                        obs._cb({ getEntries: function() { return captured; } }, obs);
                    }
                }
            }
        }
        function PerformanceObserver(cb) { this._cb = cb; this._types = []; }
        PerformanceObserver.prototype.observe = function(opts) {
            this._types = (opts && opts.entryTypes) ? opts.entryTypes : [];
            _perf_observers.push(this);
        };
        PerformanceObserver.prototype.disconnect = function() {
            var i = _perf_observers.indexOf(this);
            if (i !== -1) _perf_observers.splice(i, 1);
        };
    "#;

    fn with_loaf<F>(f: F)
    where
        F: FnOnce(&rquickjs::Ctx),
    {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_long_animation_frames_bindings(&ctx).expect("LoAF install failed");
            f(&ctx);
        });
    }

    fn with_loaf_and_perf<F>(f: F)
    where
        F: FnOnce(&rquickjs::Ctx),
    {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(PERF_STUB).expect("perf stub install failed");
            super::install_long_animation_frames_bindings(&ctx).expect("LoAF install failed");
            f(&ctx);
        });
    }

    #[test]
    fn loaf_timing_class_exists() {
        with_loaf(|ctx| {
            let ok: bool = ctx
                .eval("typeof PerformanceLongAnimationFrameTiming === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn script_timing_class_exists() {
        with_loaf(|ctx| {
            let ok: bool = ctx
                .eval("typeof PerformanceScriptTiming === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deliver_binding_exists() {
        with_loaf(|ctx| {
            let ok: bool = ctx
                .eval("typeof _lumen_deliver_long_animation_frame === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn loaf_entry_fields() {
        with_loaf(|ctx| {
            ctx.eval::<(), _>(
                "var e = new PerformanceLongAnimationFrameTiming({\
                    startTime: 1000, duration: 80, renderStart: 1010,\
                    styleAndLayoutStart: 1020, firstUIEventTimestamp: 990,\
                    scripts: []\
                });",
            )
            .unwrap();
            let entry_type: String = ctx.eval("e.entryType").unwrap();
            assert_eq!(entry_type, "long-animation-frame");
            let name: String = ctx.eval("e.name").unwrap();
            assert_eq!(name, "long-animation-frame");
            let start: f64 = ctx.eval("e.startTime").unwrap();
            assert!((start - 1000.0).abs() < 1e-6);
            let dur: f64 = ctx.eval("e.duration").unwrap();
            assert!((dur - 80.0).abs() < 1e-6);
            let rs: f64 = ctx.eval("e.renderStart").unwrap();
            assert!((rs - 1010.0).abs() < 1e-6);
            let sal: f64 = ctx.eval("e.styleAndLayoutStart").unwrap();
            assert!((sal - 1020.0).abs() < 1e-6);
            let ui: f64 = ctx.eval("e.firstUIEventTimestamp").unwrap();
            assert!((ui - 990.0).abs() < 1e-6);
        });
    }

    #[test]
    fn blocking_duration_auto_computed() {
        with_loaf(|ctx| {
            // 80ms frame → blockingDuration = 80 - 50 = 30
            let bd: f64 = ctx
                .eval("new PerformanceLongAnimationFrameTiming({duration:80}).blockingDuration")
                .unwrap();
            assert!((bd - 30.0).abs() < 1e-6, "expected 30, got {bd}");
            // 40ms frame (below threshold) → blockingDuration = 0
            let bd2: f64 = ctx
                .eval("new PerformanceLongAnimationFrameTiming({duration:40}).blockingDuration")
                .unwrap();
            assert!((bd2 - 0.0).abs() < 1e-6, "expected 0, got {bd2}");
        });
    }

    #[test]
    fn script_timing_fields() {
        with_loaf(|ctx| {
            ctx.eval::<(), _>(
                "var s = new PerformanceScriptTiming({\
                    startTime: 500, duration: 30,\
                    invoker: 'BUTTON#btn.onclick',\
                    invokerType: 'event-listener',\
                    windowAttribution: 'self',\
                    executionStart: 502,\
                    forcedStyleAndLayoutDuration: 5,\
                    pauseDuration: 0,\
                    sourceURL: 'https://example.com/app.js',\
                    sourceFunctionName: 'handleClick',\
                    sourceCharPosition: 1234\
                });",
            )
            .unwrap();
            let et: String = ctx.eval("s.entryType").unwrap();
            assert_eq!(et, "script");
            let invoker: String = ctx.eval("s.invoker").unwrap();
            assert_eq!(invoker, "BUTTON#btn.onclick");
            let it: String = ctx.eval("s.invokerType").unwrap();
            assert_eq!(it, "event-listener");
            let url: String = ctx.eval("s.sourceURL").unwrap();
            assert_eq!(url, "https://example.com/app.js");
            let fn_name: String = ctx.eval("s.sourceFunctionName").unwrap();
            assert_eq!(fn_name, "handleClick");
            let pos: i32 = ctx.eval("s.sourceCharPosition").unwrap();
            assert_eq!(pos, 1234);
        });
    }

    #[test]
    fn scripts_array_populated_from_json() {
        with_loaf(|ctx| {
            ctx.eval::<(), _>(
                r#"var e = new PerformanceLongAnimationFrameTiming({
                    duration: 80,
                    scripts: [
                        {startTime:1000, duration:20, invoker:'setTimeout', invokerType:'user-callback'},
                        {startTime:1020, duration:15, invoker:'BUTTON.onclick', invokerType:'event-listener'}
                    ]
                });"#,
            )
            .unwrap();
            let len: i32 = ctx.eval("e.scripts.length").unwrap();
            assert_eq!(len, 2);
            let inv0: String = ctx.eval("e.scripts[0].invoker").unwrap();
            assert_eq!(inv0, "setTimeout");
            let inv1: String = ctx.eval("e.scripts[1].invoker").unwrap();
            assert_eq!(inv1, "BUTTON.onclick");
            let is_script_timing: bool = ctx
                .eval("e.scripts[0] instanceof PerformanceScriptTiming")
                .unwrap();
            assert!(is_script_timing);
        });
    }

    #[test]
    fn deliver_creates_entry_in_perf_buffer() {
        with_loaf_and_perf(|ctx| {
            ctx.eval::<(), _>(
                "_lumen_deliver_long_animation_frame(2000, 75, 2010, 2020, 0, -1, null);",
            )
            .unwrap();
            let len: i32 = ctx.eval("_perf_entries.length").unwrap();
            assert_eq!(len, 1);
            let et: String = ctx.eval("_perf_entries[0].entryType").unwrap();
            assert_eq!(et, "long-animation-frame");
            let dur: f64 = ctx.eval("_perf_entries[0].duration").unwrap();
            assert!((dur - 75.0).abs() < 1e-6);
        });
    }

    #[test]
    fn deliver_notifies_observer() {
        with_loaf_and_perf(|ctx| {
            ctx.eval::<(), _>(
                r#"var got = [];
                   var po = new PerformanceObserver(function(list) {
                       got = got.concat(list.getEntries());
                   });
                   po.observe({entryTypes: ['long-animation-frame']});
                   _lumen_deliver_long_animation_frame(3000, 60, 0, 0, 0, -1, null);"#,
            )
            .unwrap();
            let len: i32 = ctx.eval("got.length").unwrap();
            assert_eq!(len, 1, "observer should have received 1 entry");
            let et: String = ctx.eval("got[0].entryType").unwrap();
            assert_eq!(et, "long-animation-frame");
        });
    }

    #[test]
    fn deliver_with_scripts_json() {
        with_loaf_and_perf(|ctx| {
            ctx.eval::<(), _>(
                r#"_lumen_deliver_long_animation_frame(
                    4000, 90, 0, 0, 0, -1,
                    '[{"startTime":4005,"duration":40,"invoker":"fetch.then","invokerType":"resolve-promise"}]'
                );"#,
            )
            .unwrap();
            let scripts_len: i32 =
                ctx.eval("_perf_entries[0].scripts.length").unwrap();
            assert_eq!(scripts_len, 1);
            let inv: String = ctx.eval("_perf_entries[0].scripts[0].invoker").unwrap();
            assert_eq!(inv, "fetch.then");
        });
    }
}
