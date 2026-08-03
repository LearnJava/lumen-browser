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

/// V8 port of the former rquickjs `install_scroll_snap_events_bindings` (Ph3 V8
/// migration S5-S7): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`. Must run
/// **after** DOM install so that `Event` is already defined.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_scroll_snap_events_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SCROLL_SNAP_EVENTS_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing CSS Scroll Snap L2 events.
#[cfg(feature = "v8-backend")]
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
