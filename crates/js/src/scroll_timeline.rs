//! CSS Scroll-Driven Animations Level 1 — JS API shim.
//!
//! Installs `ScrollTimeline` and `ViewTimeline` classes and the
//! `_lumen_deliver_scroll_progress(progress_y, progress_x)` delivery binding.
//!
//! The shell calls `_lumen_deliver_scroll_progress` after every scroll update
//! (RedrawRequested step 1) to push the viewport fraction into all registered
//! `ScrollTimeline` instances.  Each timeline's `currentTime` property reflects
//! the progress as a CSS `<percentage>` value (0–100).
//!
//! Phase 0 scope:
//! * Root-viewport `ScrollTimeline` (source = document.scrollingElement).
//! * `ViewTimeline` constructor (currentTime not yet updated — P4 wires layout).
//! * `_lumen_deliver_scroll_progress(progress_y, progress_x)` binding.
//!
//! Phase 1 (P4): `animation-timeline: scroll()` parsed in ComputedStyle + wired
//! to `AnimationScheduler` to drive keyframe progress from `currentTime`.
//!
//! Spec: <https://www.w3.org/TR/scroll-animations-1/>

use rquickjs::Ctx;

/// Install CSS Scroll-Driven Animations L1 JS API into the QuickJS context.
///
/// Must be called **after** `install_dom_api` so that `document` and `Event`
/// are available.
pub fn install_scroll_timeline_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SCROLL_TIMELINE_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C CSS Scroll-Driven Animations Level 1 API.
const SCROLL_TIMELINE_SHIM: &str = r#"(function() {
  'use strict';

  // Global registry of all live ScrollTimeline instances.
  // WeakRef is not available in QuickJS — use plain array; items are removed
  // when a timeline is GC'd or when _lumen_deliver_scroll_progress cleans up
  // nulled slots (defensive, but GC timing is non-deterministic so we keep it simple).
  var _sda_timelines = [];

  // ── AnimationTimeline base ────────────────────────────────────────────────

  // Minimal AnimationTimeline base (W3C Web Animations §6.1).
  function AnimationTimeline() {
    this._currentTime = null;
  }
  AnimationTimeline.prototype = {
    constructor: AnimationTimeline,
    /// CSS <percentage> progress: 0–100, or null before scroll starts.
    get currentTime() { return this._currentTime; }
  };

  // ── ScrollTimeline ────────────────────────────────────────────────────────

  /// CSS Scroll-Driven Animations Level 1 §3 — scroll progress timeline.
  ///
  /// Tracks the scroll fraction of a scroll container (default: root viewport).
  /// `currentTime` is a CSS percentage value (0.0–100.0, null when inactive).
  function ScrollTimeline(options) {
    AnimationTimeline.call(this);
    var opts = options || {};
    this._source = opts.source !== undefined ? opts.source : null;
    this._axis   = opts.axis   !== undefined ? opts.axis   : 'block';
    // Register for progress delivery.
    _sda_timelines.push(this);
  }
  ScrollTimeline.prototype = Object.create(AnimationTimeline.prototype);
  ScrollTimeline.prototype.constructor = ScrollTimeline;
  /// Scroll container element, or null for the root viewport.
  Object.defineProperty(ScrollTimeline.prototype, 'source', {
    get: function() {
      return this._source !== null ? this._source
        : (typeof document !== 'undefined'
            ? (document.scrollingElement || document.documentElement)
            : null);
    }
  });
  /// Scroll axis: 'block' | 'inline' | 'x' | 'y' (default 'block').
  Object.defineProperty(ScrollTimeline.prototype, 'axis', {
    get: function() { return this._axis; }
  });

  globalThis.ScrollTimeline = ScrollTimeline;

  // ── ViewTimeline ──────────────────────────────────────────────────────────

  /// CSS Scroll-Driven Animations Level 1 §4 — view progress timeline.
  ///
  /// Tracks the visibility fraction of `subject` inside its scroll container.
  /// Phase 0: constructor only — currentTime is updated by P4 layout wiring.
  function ViewTimeline(options) {
    ScrollTimeline.call(this, options);
    var opts = options || {};
    this._subject = opts.subject !== undefined ? opts.subject : null;
  }
  ViewTimeline.prototype = Object.create(ScrollTimeline.prototype);
  ViewTimeline.prototype.constructor = ViewTimeline;
  /// Subject element whose visibility fraction drives `currentTime`.
  Object.defineProperty(ViewTimeline.prototype, 'subject', {
    get: function() { return this._subject; }
  });

  globalThis.ViewTimeline = ViewTimeline;

  // ── Delivery binding ──────────────────────────────────────────────────────

  /// Called by the shell (lumen-shell) after each scroll update.
  ///
  /// `progress_y` — block-axis progress [0.0, 1.0].
  /// `progress_x` — inline-axis progress [0.0, 1.0].
  ///
  /// Updates `currentTime` (as CSS %) on every root-viewport ScrollTimeline
  /// registered so far.  ViewTimeline.currentTime is left for P4 layout wiring.
  globalThis._lumen_deliver_scroll_progress = function(progress_y, progress_x) {
    var pct_y = +progress_y * 100;
    var pct_x = +progress_x * 100;
    for (var i = 0; i < _sda_timelines.length; i++) {
      var tl = _sda_timelines[i];
      if (!tl || !(tl instanceof ScrollTimeline)) continue;
      // Only update root-viewport timelines (source === null means root).
      if (tl._source !== null) continue;
      // Skip ViewTimeline — progress driven by layout, not by raw scroll.
      if (tl instanceof ViewTimeline) continue;
      var axis = tl._axis;
      if (axis === 'block' || axis === 'y') {
        tl._currentTime = pct_y;
      } else {
        tl._currentTime = pct_x;
      }
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

    fn with_scroll_timeline<F>(f: F)
    where
        F: FnOnce(&rquickjs::Ctx),
    {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_scroll_timeline_bindings(&ctx)
                .expect("scroll_timeline install failed");
            f(&ctx);
        });
    }

    #[test]
    fn scroll_timeline_class_exists() {
        with_scroll_timeline(|ctx| {
            let ok: bool = ctx.eval("typeof ScrollTimeline === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn view_timeline_class_exists() {
        with_scroll_timeline(|ctx| {
            let ok: bool = ctx.eval("typeof ViewTimeline === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deliver_binding_exists() {
        with_scroll_timeline(|ctx| {
            let ok: bool = ctx
                .eval("typeof _lumen_deliver_scroll_progress === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn scroll_timeline_default_axis() {
        with_scroll_timeline(|ctx| {
            let axis: String = ctx
                .eval("new ScrollTimeline().axis")
                .unwrap();
            assert_eq!(axis, "block");
        });
    }

    #[test]
    fn scroll_timeline_custom_axis() {
        with_scroll_timeline(|ctx| {
            let axis: String = ctx
                .eval("new ScrollTimeline({axis:'inline'}).axis")
                .unwrap();
            assert_eq!(axis, "inline");
        });
    }

    #[test]
    fn current_time_null_before_delivery() {
        with_scroll_timeline(|ctx| {
            let is_null: bool = ctx
                .eval("new ScrollTimeline().currentTime === null")
                .unwrap();
            assert!(is_null);
        });
    }

    #[test]
    fn deliver_updates_block_current_time() {
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>(
                "var tl = new ScrollTimeline({axis:'block'}); \
                 _lumen_deliver_scroll_progress(0.5, 0.25);",
            )
            .unwrap();
            let ct: f64 = ctx.eval("tl.currentTime").unwrap();
            assert!((ct - 50.0).abs() < 1e-6, "expected 50.0, got {ct}");
        });
    }

    #[test]
    fn deliver_updates_inline_current_time() {
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>(
                "var tl = new ScrollTimeline({axis:'inline'}); \
                 _lumen_deliver_scroll_progress(0.5, 0.25);",
            )
            .unwrap();
            let ct: f64 = ctx.eval("tl.currentTime").unwrap();
            assert!((ct - 25.0).abs() < 1e-6, "expected 25.0, got {ct}");
        });
    }

    #[test]
    fn deliver_updates_y_axis_current_time() {
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>(
                "var tl = new ScrollTimeline({axis:'y'}); \
                 _lumen_deliver_scroll_progress(1.0, 0.0);",
            )
            .unwrap();
            let ct: f64 = ctx.eval("tl.currentTime").unwrap();
            assert!((ct - 100.0).abs() < 1e-6, "expected 100.0, got {ct}");
        });
    }

    #[test]
    fn deliver_does_not_update_view_timeline() {
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>(
                "var vt = new ViewTimeline(); \
                 _lumen_deliver_scroll_progress(0.8, 0.3);",
            )
            .unwrap();
            let is_null: bool = ctx.eval("vt.currentTime === null").unwrap();
            assert!(is_null, "ViewTimeline.currentTime should stay null (P4 wires it)");
        });
    }

    #[test]
    fn deliver_does_not_update_element_scroll_timeline() {
        // source !== null → element-specific scroll container, not root viewport.
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>(
                "var tl = new ScrollTimeline({source: {}}); \
                 _lumen_deliver_scroll_progress(0.6, 0.2);",
            )
            .unwrap();
            let is_null: bool = ctx.eval("tl.currentTime === null").unwrap();
            assert!(is_null, "element-specific timeline should not be updated by root delivery");
        });
    }

    #[test]
    fn multiple_timelines_all_updated() {
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>(
                "var tl1 = new ScrollTimeline({axis:'block'}); \
                 var tl2 = new ScrollTimeline({axis:'block'}); \
                 _lumen_deliver_scroll_progress(0.75, 0.0);",
            )
            .unwrap();
            let ct1: f64 = ctx.eval("tl1.currentTime").unwrap();
            let ct2: f64 = ctx.eval("tl2.currentTime").unwrap();
            assert!((ct1 - 75.0).abs() < 1e-6);
            assert!((ct2 - 75.0).abs() < 1e-6);
        });
    }

    #[test]
    fn view_timeline_subject_property() {
        with_scroll_timeline(|ctx| {
            ctx.eval::<(), _>("var obj = {id:42}; var vt = new ViewTimeline({subject:obj});")
                .unwrap();
            let id: i32 = ctx.eval("vt.subject.id").unwrap();
            assert_eq!(id, 42);
        });
    }

    #[test]
    fn view_timeline_is_instance_of_scroll_timeline() {
        with_scroll_timeline(|ctx| {
            let ok: bool = ctx
                .eval("new ViewTimeline() instanceof ScrollTimeline")
                .unwrap();
            assert!(ok);
        });
    }
}
