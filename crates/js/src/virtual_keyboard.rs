/// Virtual Keyboard API (W3C Virtual Keyboard API).
///
/// Phase 0: geometry reporting stubs + event infrastructure.
/// - `navigator.virtualKeyboard.show()` — request VK visibility
/// - `navigator.virtualKeyboard.hide()` — request VK hide
/// - `navigator.virtualKeyboard.boundingRect` → DOMRect (0,0,0,0 in Phase 0)
/// - `navigator.virtualKeyboard.overlaysContent` — boolean getter/setter
/// - `geometrychange` event fires when keyboard geometry changes
///
/// Native bindings `_lumen_vk_show()` / `_lumen_vk_hide()` are no-op hooks
/// for shell Phase 1 (platform virtual keyboard integration).
use rquickjs::Ctx;

/// Install Virtual Keyboard API bindings into the JS context.
pub fn install_virtual_keyboard_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(VIRTUAL_KEYBOARD_SHIM)?;
    Ok(())
}

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
    // Phase 0: zero bounding rect (keyboard not visible / not integrated).
    this.boundingRect = new DOMRect(0, 0, 0, 0);
  }

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_vk(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
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
            install_virtual_keyboard_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn virtual_keyboard_exists() {
        with_vk(|ctx| {
            let ok: bool = ctx
                .eval("typeof navigator.virtualKeyboard === 'object' && navigator.virtualKeyboard !== null")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn show_and_hide_are_functions() {
        with_vk(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof navigator.virtualKeyboard.show === 'function' && \
                     typeof navigator.virtualKeyboard.hide === 'function'",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn overlays_content_defaults_false() {
        with_vk(|ctx| {
            let ok: bool = ctx
                .eval("navigator.virtualKeyboard.overlaysContent === false")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn overlays_content_setter() {
        with_vk(|ctx| {
            let ok: bool = ctx
                .eval(
                    "navigator.virtualKeyboard.overlaysContent = true; \
                     navigator.virtualKeyboard.overlaysContent === true",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn geometry_change_event_fires() {
        with_vk(|ctx| {
            let ok: bool = ctx
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
            assert!(ok);
        });
    }
}
