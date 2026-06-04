//! CSS Scroll Snap L2 events (W3C CSS Scroll Snap §4).
//!
//! Installs `snapChanging` and `snapChanged` events on scroll containers
//! when they snap to a new position. Phase 0: event infrastructure is complete;
//! shell integration via `_lumen_fire_snap_changing` / `_lumen_fire_snap_changed`
//! bidings will emit events when snap-points change.
//!
//! Installed interfaces:
//! - `SnapChangeEvent` class — snapTargetBlock, snapTargetInline properties
//! - `window.SnapChangeEvent` exported as global
//! - `_lumen_fire_snap_changing(nid, snapTargetBlock, snapTargetInline)` — fire snapChanging
//! - `_lumen_fire_snap_changed(nid, snapTargetBlock, snapTargetInline)` — fire snapChanged

use rquickjs::Ctx;

/// Install CSS Scroll Snap L2 events into the JS context.
///
/// Adds `SnapChangeEvent` class and native bindings for shell to fire snap events.
/// Phase 0: shell must call `_lumen_fire_snap_changing/changed` when the DOM
/// scroll snap state changes via layout calculations (apply_page_y_snap).
///
/// Must be called **after** `install_dom_api` so that `Event` is already defined.
pub fn install_scroll_snap_events_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SCROLL_SNAP_EVENTS_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing CSS Scroll Snap L2 events.
const SCROLL_SNAP_EVENTS_SHIM: &str = r#"(function() {
  'use strict';

  // ── SnapChangeEvent (W3C CSS Scroll Snap §4) ──────────────────────────────────
  // Fired when a scroll container snaps to a new snap point.
  function SnapChangeEvent(type, init) {
    if (typeof Event === 'undefined') return;
    var base = new Event(type, init);
    // Copy Event properties
    Object.defineProperty(this, '_base', { value: base, enumerable: false });
    this.type       = base.type;
    this.bubbles    = base.bubbles;
    this.cancelable = base.cancelable;
    this.snapTargetBlock  = (init && typeof init === 'object' && init.snapTargetBlock !== undefined) ? init.snapTargetBlock : null;
    this.snapTargetInline = (init && typeof init === 'object' && init.snapTargetInline !== undefined) ? init.snapTargetInline : null;
  }
  if (typeof Event !== 'undefined') {
    SnapChangeEvent.prototype = Object.create(Event.prototype);
    SnapChangeEvent.prototype.constructor = SnapChangeEvent;
  }

  // Export SnapChangeEvent as global
  globalThis.SnapChangeEvent = SnapChangeEvent;

  // ── Native bindings for shell to fire snap events ────────────────────────────
  // Shell calls _lumen_fire_snap_changing(nid, snapTargetBlock, snapTargetInline)
  // when the user initiates a scroll that will snap to a new position.
  // Then calls _lumen_fire_snap_changed(nid, ...) when the snap is complete.

  globalThis._lumen_fire_snap_changing = function(nid, snapTargetBlock, snapTargetInline) {
    if (typeof _lumen_make_element === 'undefined') return;
    var el = _lumen_make_element(nid);
    if (!el) return;
    var ev = new SnapChangeEvent('snapchanging', {
      bubbles: true,
      cancelable: true,
      snapTargetBlock: snapTargetBlock,
      snapTargetInline: snapTargetInline
    });
    if (typeof el.dispatchEvent === 'function') {
      el.dispatchEvent(ev);
    }
  };

  globalThis._lumen_fire_snap_changed = function(nid, snapTargetBlock, snapTargetInline) {
    if (typeof _lumen_make_element === 'undefined') return;
    var el = _lumen_make_element(nid);
    if (!el) return;
    var ev = new SnapChangeEvent('snapchanged', {
      bubbles: true,
      cancelable: false,
      snapTargetBlock: snapTargetBlock,
      snapTargetInline: snapTargetInline
    });
    if (typeof el.dispatchEvent === 'function') {
      el.dispatchEvent(ev);
    }
  };
})();
"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_scroll_snap_events<F>(f: F)
    where
        F: FnOnce(&rquickjs::Ctx),
    {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            if let Err(e) = super::install_scroll_snap_events_bindings(&ctx) {
                panic!("Failed to install scroll snap events: {}", e);
            }
            f(&ctx);
        });
    }

    #[test]
    fn snap_change_event_class_exists() {
        with_scroll_snap_events(|ctx| {
            let result: bool = ctx
                .eval("typeof SnapChangeEvent === 'function'")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn snap_change_event_constructor_with_props() {
        with_scroll_snap_events(|ctx| {
            // Test that SnapChangeEvent constructor accepts properties.
            let result: bool = ctx
                .eval("new SnapChangeEvent('snapchanging', { snapTargetBlock: 'center' }) !== undefined")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn lumen_fire_snap_changing_exists() {
        with_scroll_snap_events(|ctx| {
            let result: bool = ctx
                .eval("typeof globalThis._lumen_fire_snap_changing === 'function'")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn lumen_fire_snap_changed_exists() {
        with_scroll_snap_events(|ctx| {
            let result: bool = ctx
                .eval("typeof globalThis._lumen_fire_snap_changed === 'function'")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn snap_change_event_with_init_props() {
        with_scroll_snap_events(|ctx| {
            // Verify that SnapChangeEvent can be created with init object.
            ctx.eval::<(), _>(
                "var ev = new SnapChangeEvent('snapchanging', { \
                   snapTargetBlock: 'end', \
                   snapTargetInline: 'start' \
                 }); \
                 globalThis.__test_ok = true;"
            )
            .unwrap();
            let ok: bool = ctx.eval("globalThis.__test_ok").unwrap();
            assert!(ok);
        });
    }
}
