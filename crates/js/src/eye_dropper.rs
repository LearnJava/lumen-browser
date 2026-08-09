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
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_eye_dropper(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        install_eye_dropper_bindings_v8(&rt).unwrap();
        f(&rt);
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
}
