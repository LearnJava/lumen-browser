//! Eye Dropper API (W3C Color WG)
//!
//! Phase 0 stub: EyeDropper with native platform color picker integration
//! (PowerShell ColorDialog on Windows, zenity on Linux, osascript on macOS)

/// V8 port of the former rquickjs `install_eye_dropper_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B2): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_eye_dropper_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(EYE_DROPPER_SHIM)?;
    Ok(())
}

/// JavaScript shim: Eye Dropper API (Phase 0)
#[cfg(feature = "v8-backend")]
const EYE_DROPPER_SHIM: &str = r#"
(function() {
  // EyeDropper class. Spec (WICG Eye Dropper API): constructor takes no
  // arguments — `options` belongs to `open()`, not the constructor.
  class EyeDropper {
    constructor() {}

    async open(options) {
      // WICG Eye Dropper API §3 step 3 — without transient activation the
      // call must reject with NotAllowedError before doing anything else
      // (BUG-698). Mirrors the same gate in window_management.rs/
      // local_font_access.rs; `activation` undefined (no `navigator` stub,
      // e.g. this module's own unit tests) stays permissive like those do.
      const activation = (typeof navigator !== 'undefined') ? navigator.userActivation : undefined;
      if (activation && activation.isActive === false) {
        throw new DOMException(
          'EyeDropper.open() requires transient activation.', 'NotAllowedError');
      }
      // WICG Eye Dropper API §3 — consume user activation once the check passes.
      if (typeof _lumen_consume_user_activation === 'function') {
        _lumen_consume_user_activation();
      }

      const signal = options?.signal;

      // Check if abort signal is already aborted
      if (signal?.aborted) {
        throw signal.reason || new DOMException('AbortError', 'AbortError');
      }

      // Phase 0: Call native binding to open platform color picker
      return new Promise((resolve, reject) => {
        const onAbort = () => {
          reject(new DOMException('AbortError', 'AbortError'));
          if (signal) signal.removeEventListener('abort', onAbort);
        };

        if (signal) signal.addEventListener('abort', onAbort);

        // Call native binding (implemented by shell). No platform picker is
        // wired up yet (BUG-365), so this is never a function today — the
        // `typeof` guard (not `?.call?.()`, which does not protect against an
        // undeclared identifier and threw ReferenceError) keeps that honest
        // and routes straight to the documented white-color fallback below.
        const nativeOpen = globalThis._lumen_eye_dropper_open;
        const result = typeof nativeOpen === 'function' ? nativeOpen() : null;

        if (signal?.aborted) {
          if (signal) signal.removeEventListener('abort', onAbort);
          reject(new DOMException('AbortError', 'AbortError'));
          return;
        }

        if (!result) {
          // Fallback: return white color if native binding not available
          if (signal) signal.removeEventListener('abort', onAbort);
          resolve({ sRGBHex: '#ffffff' });
          return;
        }

        // Parse JSON result from native binding
        try {
          const parsed = JSON.parse(result);
          if (signal) signal.removeEventListener('abort', onAbort);
          resolve(parsed);
        } catch (e) {
          if (signal) signal.removeEventListener('abort', onAbort);
          reject(e);
        }
      });
    }
  }

  Object.defineProperty(EyeDropper.prototype, Symbol.toStringTag, {
    value: 'EyeDropper',
    writable: false,
    enumerable: false,
    configurable: true,
  });

  // Export to global scope
  if (typeof window !== 'undefined') {
    window.EyeDropper = EyeDropper;
  }
  if (typeof globalThis !== 'undefined') {
    globalThis.EyeDropper = EyeDropper;
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_eye_dropper(f: impl FnOnce(&V8JsRuntime)) {
        with_eye_dropper_setup("", f);
    }

    /// Same harness with `extra` evaluated after the default `navigator`
    /// stub (transient activation granted, the happy-path default), so a
    /// test can override `userActivation` before install.
    fn with_eye_dropper_setup(extra: &str, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var navigator = { userActivation: { isActive: true } }; \
             function DOMException(msg, name) { this.message = msg; this.name = name; } \
             DOMException.prototype = Object.create(Error.prototype); \
             globalThis.DOMException = DOMException;",
        )
        .unwrap();
        if !extra.is_empty() {
            rt.eval(extra).unwrap();
        }
        install_eye_dropper_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    /// Resolves `expr` (a promise) and reports `"resolved"` or `"rejected|<name>|<message>"`.
    fn settle(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            r#"
            var __out = 'never settled';
            ({expr}).then(
              function() {{ __out = 'resolved'; }},
              function(e) {{ __out = 'rejected|' + e.name + '|' + e.message; }});
            "#
        ))
        .unwrap();
        match rt.eval("String(__out)").unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn test_eye_dropper_constructor() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("(function() { const dropper = new EyeDropper(); return !!dropper; })()")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_open_returns_promise() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("new EyeDropper().open() instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_open_accepts_options() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("new EyeDropper().open({}) instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_global_export() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval(
                    r#"
                    (typeof window === 'undefined' || !!window.EyeDropper) &&
                    (typeof globalThis === 'undefined' || !!globalThis.EyeDropper)
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_options_constructor() {
        // The constructor ignores extra arguments (spec: it takes none) —
        // passing one must not throw.
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("(function() { const dropper = new EyeDropper({}); return !!dropper; })()")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_constructor_length_is_zero() {
        with_eye_dropper(|rt| {
            let ok = rt.eval("EyeDropper.length === 0").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_no_stray_options_property() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("!new EyeDropper().hasOwnProperty('options')")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_to_string_tag() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("Object.prototype.toString.call(new EyeDropper()) === '[object EyeDropper]'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// BUG-365 regression: with no native platform binding installed,
    /// `open()` must resolve the documented `#ffffff` fallback instead of
    /// rejecting with `ReferenceError: _lumen_eye_dropper_open is not
    /// defined`. The two-`eval` split relies on V8's default
    /// `MicrotasksPolicy::kAuto` draining the promise job between them (same
    /// pattern as `shared_storage.rs`'s `promise_result` helper).
    #[test]
    fn test_eye_dropper_resolve_value() {
        with_eye_dropper(|rt| {
            rt.eval(
                r#"
                globalThis.__ok = null;
                globalThis.__err = null;
                new EyeDropper().open().then(
                    result => { globalThis.__ok = result; },
                    err => { globalThis.__err = err && err.message ? err.message : String(err); }
                );
                "#,
            )
            .unwrap();

            let err = rt.eval("globalThis.__err").unwrap();
            assert_eq!(err, JsValue::Null, "open() rejected: {err:?}");

            let hex = rt.eval("globalThis.__ok && globalThis.__ok.sRGBHex").unwrap();
            assert_eq!(hex, JsValue::String("#ffffff".to_string()));
        });
    }

    /// BUG-698 — WICG Eye Dropper API §3 step 3: without transient
    /// activation, `open()` must reject with `NotAllowedError` instead of
    /// running its fallback logic.
    #[test]
    fn test_eye_dropper_open_requires_transient_activation() {
        with_eye_dropper_setup("navigator.userActivation.isActive = false;", |rt| {
            let out = settle(rt, "new EyeDropper().open()");
            assert_eq!(
                out,
                "rejected|NotAllowedError|EyeDropper.open() requires transient activation."
            );
        });
    }

    /// BUG-698 companion: with transient activation granted, `open()` must
    /// still resolve the documented fallback (no regression on the happy path).
    #[test]
    fn test_eye_dropper_open_succeeds_with_transient_activation() {
        with_eye_dropper(|rt| {
            let out = settle(rt, "new EyeDropper().open()");
            assert_eq!(out, "resolved");
        });
    }
}
