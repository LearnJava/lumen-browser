//! Virtual Keyboard API (W3C Virtual Keyboard API).
//!
//! Phase 0: geometry reporting stubs + event infrastructure.
//! - `navigator.virtualKeyboard.show()` — request VK visibility
//! - `navigator.virtualKeyboard.hide()` — request VK hide
//! - `navigator.virtualKeyboard.boundingRect` → DOMRect (0,0,0,0 in Phase 0)
//! - `navigator.virtualKeyboard.overlaysContent` — boolean getter/setter
//! - `geometrychange` event fires when keyboard geometry changes
//!
//! Native bindings `_lumen_vk_show()` / `_lumen_vk_hide()` are no-op hooks
//! for shell Phase 1 (platform virtual keyboard integration).

/// V8 port of the former rquickjs `install_virtual_keyboard_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B2): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_virtual_keyboard_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(VIRTUAL_KEYBOARD_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const VIRTUAL_KEYBOARD_SHIM: &str = r#"
(function() {
  // Phase 0 native hooks — no-op; shell installs real handlers in Phase 1.
  if (typeof _lumen_vk_show === 'undefined') {
    globalThis._lumen_vk_show = function() {};
  }
  if (typeof _lumen_vk_hide === 'undefined') {
    globalThis._lumen_vk_hide = function() {};
  }

  // W3C Virtual Keyboard API §4.1 — VirtualKeyboard interface.
  function VirtualKeyboard() {
    this._overlaysContent = false;
    this._listeners = {};
    // Phase 0: zero bounding rect; DOMRect may not be available yet — defer.
    this._boundingRect = null;
  }

  // Lazy boundingRect: create DOMRect on first access so DOMRect is guaranteed defined.
  Object.defineProperty(VirtualKeyboard.prototype, 'boundingRect', {
    get: function() {
      if (!this._boundingRect) {
        this._boundingRect = (typeof DOMRect !== 'undefined')
          ? new DOMRect(0, 0, 0, 0)
          : { x: 0, y: 0, width: 0, height: 0 };
      }
      return this._boundingRect;
    },
    set: function(v) { this._boundingRect = v; },
    enumerable: true,
    configurable: true,
  });

  // §4.1: overlaysContent getter/setter.
  Object.defineProperty(VirtualKeyboard.prototype, 'overlaysContent', {
    get: function() { return this._overlaysContent; },
    set: function(v) { this._overlaysContent = Boolean(v); },
    enumerable: true,
    configurable: true,
  });

  // §4.1: show() — request that the UA show the virtual keyboard.
  VirtualKeyboard.prototype.show = function() {
    _lumen_vk_show();
  };

  // §4.1: hide() — request that the UA hide the virtual keyboard.
  VirtualKeyboard.prototype.hide = function() {
    _lumen_vk_hide();
  };

  // EventTarget mixin: addEventListener / removeEventListener / dispatchEvent.
  VirtualKeyboard.prototype.addEventListener = function(type, listener) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(listener);
  };

  VirtualKeyboard.prototype.removeEventListener = function(type, listener) {
    if (!this._listeners[type]) return;
    var arr = this._listeners[type];
    var idx = arr.indexOf(listener);
    if (idx !== -1) arr.splice(idx, 1);
  };

  VirtualKeyboard.prototype.dispatchEvent = function(event) {
    var type = event.type;
    if (this['on' + type]) {
      try { this['on' + type](event); } catch (_) {}
    }
    var listeners = this._listeners[type] || [];
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i](event); } catch (_) {}
    }
    return !event.defaultPrevented;
  };

  // §4.1: ongeometrychange attribute handler.
  VirtualKeyboard.prototype.ongeometrychange = null;

  // Install singleton on navigator.
  if (typeof navigator !== 'undefined') {
    Object.defineProperty(navigator, 'virtualKeyboard', {
      value: new VirtualKeyboard(),
      writable: false,
      configurable: true,
      enumerable: true,
    });
  }

  // §4.2: _lumen_fire_vk_geometry_change(x, y, width, height) — called by shell
  // when the platform VK geometry changes (Phase 1). Fires 'geometrychange' event.
  globalThis._lumen_fire_vk_geometry_change = function(x, y, width, height) {
    var vk = navigator.virtualKeyboard;
    vk.boundingRect = new DOMRect(x, y, width, height);
    var event = new Event('geometrychange');
    vk.dispatchEvent(event);
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_vk(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            var window = globalThis;
            var navigator = {};
            // Minimal DOMRect stub for test environment.
            globalThis.DOMRect = function(x, y, w, h) {
                this.x = x || 0; this.y = y || 0;
                this.width = w || 0; this.height = h || 0;
            };
            globalThis.Event = function(type) { this.type = type; this.defaultPrevented = false; };
            "#,
        )
        .unwrap();
        install_virtual_keyboard_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn virtual_keyboard_exists() {
        with_vk(|rt| {
            let ok = rt
                .eval("typeof navigator.virtualKeyboard === 'object' && navigator.virtualKeyboard !== null")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn show_and_hide_are_functions() {
        with_vk(|rt| {
            let ok = rt
                .eval(
                    "typeof navigator.virtualKeyboard.show === 'function' && \
                     typeof navigator.virtualKeyboard.hide === 'function'",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn overlays_content_defaults_false() {
        with_vk(|rt| {
            let ok = rt
                .eval("navigator.virtualKeyboard.overlaysContent === false")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn overlays_content_setter() {
        with_vk(|rt| {
            let ok = rt
                .eval(
                    "navigator.virtualKeyboard.overlaysContent = true; \
                     navigator.virtualKeyboard.overlaysContent === true",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn geometry_change_event_fires() {
        with_vk(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var fired = false;
                    navigator.virtualKeyboard.addEventListener('geometrychange', function(e) {
                        fired = true;
                    });
                    _lumen_fire_vk_geometry_change(0, 400, 375, 320);
                    fired === true && navigator.virtualKeyboard.boundingRect.width === 375
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
