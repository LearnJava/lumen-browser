//! `HTMLElement.inert` JS binding — HTML Living Standard §6.7.
//!
//! Installs the `inert` getter/setter on `HTMLElement.prototype` so that JS
//! code can read and toggle the inert state of elements:
//!
//! ```js
//! dialog.inert = true;   // sets the `inert` attribute
//! dialog.inert;          // → true
//! ```
//!
//! Phase 0: the setter stores a flag on the element JS object and calls the
//! native binding `_lumen_set_inert(nid, bool)` which the shell will wire in
//! Phase 1 to propagate the attribute change back to the DOM.
//!
//! Phase 1 (shell wiring): implement `_lumen_set_inert` native binding to call
//! `Document::set_attr(node, "inert", "")` / `Document::remove_attr(node, "inert")`,
//! then trigger a style recalc so that the UA `[inert] { pointer-events: none; }`
//! rule (wired by P4) takes effect.
use rquickjs::Ctx;

/// Install `HTMLElement.prototype.inert` getter/setter into the JS context.
pub fn install_inert_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(INERT_SHIM)?;
    Ok(())
}

const INERT_SHIM: &str = r#"
(function() {
  'use strict';

  // Guard: only patch if HTMLElement is defined (not available in worker contexts).
  if (typeof HTMLElement === 'undefined') return;

  // Phase 0: inert state stored as a JS property on each element instance.
  // Phase 1: replace _lumen_inert_storage with a native WeakMap backed by DOM attrs.
  Object.defineProperty(HTMLElement.prototype, 'inert', {
    configurable: true,
    enumerable: true,

    get: function get_inert() {
      return this._inert === true;
    },

    set: function set_inert(value) {
      var inert = Boolean(value);
      this._inert = inert;

      // Phase 1 hook: propagate to Rust DOM (set/remove `inert` attribute).
      // Shell implements _lumen_set_inert(nid: i32, inert: bool) → void.
      if (typeof globalThis._lumen_set_inert === 'function' && typeof this.__nid === 'number') {
        globalThis._lumen_set_inert(this.__nid, inert);
      }
    },
  });

  // Phase 1 hook stub — shell overrides this with a real native binding.
  // Storing a no-op now prevents "is not a function" errors in Phase 0.
  if (typeof globalThis._lumen_set_inert === 'undefined') {
    globalThis._lumen_set_inert = function _lumen_set_inert(_nid, _inert) {
      // Phase 0 no-op — Phase 1: set/remove DOM `inert` attribute via Rust binding.
    };
  }
})();
"#;

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Set up minimal HTMLElement stub + inert bindings.
    fn with_inert_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;

                function HTMLElement() {}
                HTMLElement.prototype = Object.create(null);
                window.HTMLElement = HTMLElement;

                window.makeEl = function() {
                  var el = Object.create(HTMLElement.prototype);
                  el.__nid = 42;
                  return el;
                };
                "#,
            )
            .unwrap();
            install_inert_api(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn inert_defaults_to_false() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("var el = makeEl(); el.inert === false")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn setting_inert_true_reflects_as_true() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("var el = makeEl(); el.inert = true; el.inert === true")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn setting_inert_false_reflects_as_false() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("var el = makeEl(); el.inert = true; el.inert = false; el.inert === false")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn inert_coerces_truthy_value() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("var el = makeEl(); el.inert = 1; el.inert === true")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn inert_coerces_falsy_value() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("var el = makeEl(); el.inert = 0; el.inert === false")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn lumen_set_inert_stub_exists() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof globalThis._lumen_set_inert === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn stub_does_not_throw() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval("globalThis._lumen_set_inert(42, true); true")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn inert_property_on_prototype_not_instance() {
        with_inert_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeEl();
                    el.inert = true;
                    // The descriptor should be on the prototype, not own property
                    // (but _inert storage is on instance — check prototype has 'inert')
                    typeof Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'inert') === 'object'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
