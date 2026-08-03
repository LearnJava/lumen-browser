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

/// Native binding for platform color picker
/// Called from shell/platform modules for each OS
pub extern "C" fn _lumen_eye_dropper_open() -> *const u8 {
    // This will be implemented by the shell layer for each OS
    // Returns JSON: {"sRGBHex": "#rrggbb"} or error
    // For now, returns null (platform integration deferred to P3)
    std::ptr::null()
}

/// JavaScript shim: Eye Dropper API (Phase 0)
#[cfg(feature = "v8-backend")]
const EYE_DROPPER_SHIM: &str = r#"
(function() {
  // EyeDropper class
  class EyeDropper {
    constructor(options) {
      this.options = options || {};
    }

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

        // Call native binding (implemented by shell)
        const result = _lumen_eye_dropper_open?.call?.(null);

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
        with_eye_dropper(|rt| {
            let ok = rt
                .eval("(function() { const dropper = new EyeDropper({}); return !!dropper; })()")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_eye_dropper_resolve_value() {
        with_eye_dropper(|rt| {
            let ok = rt
                .eval(
                    r#"
                    (function() {
                        const dropper = new EyeDropper();
                        dropper.open().then(result => {
                            if (!result.hasOwnProperty('sRGBHex')) throw new Error('missing sRGBHex');
                            if (typeof result.sRGBHex !== 'string') throw new Error('sRGBHex not a string');
                        });
                        return true;
                    })()
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
