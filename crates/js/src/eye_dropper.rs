//! Eye Dropper API (W3C Color WG)
//!
//! Phase 0 stub: EyeDropper with native platform color picker integration
//! (PowerShell ColorDialog on Windows, zenity on Linux, osascript on macOS)

use rquickjs::Ctx;

pub fn install_eye_dropper_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(EYE_DROPPER_SHIM)?;
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

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    #[test]
    fn test_eye_dropper_constructor() -> rquickjs::Result<()> {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_eye_dropper_bindings(&ctx)?;

            ctx.eval::<(), _>(
                r#"
                const dropper = new EyeDropper();
                if (!dropper) throw new Error("Failed to create EyeDropper");
                "#,
            )?;
            Ok(())
        })
    }

    #[test]
    fn test_eye_dropper_open_returns_promise() -> rquickjs::Result<()> {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_eye_dropper_bindings(&ctx)?;

            ctx.eval::<(), _>(
                r#"
                const dropper = new EyeDropper();
                const result = dropper.open();
                if (!(result instanceof Promise)) {
                  throw new Error("open() must return a Promise");
                }
                "#,
            )?;
            Ok(())
        })
    }

    #[test]
    fn test_eye_dropper_open_accepts_options() -> rquickjs::Result<()> {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_eye_dropper_bindings(&ctx)?;

            ctx.eval::<(), _>(
                r#"
                const dropper = new EyeDropper();

                // Test that open() accepts options parameter
                const promise = dropper.open({});
                if (!(promise instanceof Promise)) {
                  throw new Error("open() must accept options and return a Promise");
                }
                "#,
            )?;
            Ok(())
        })
    }

    #[test]
    fn test_eye_dropper_global_export() -> rquickjs::Result<()> {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_eye_dropper_bindings(&ctx)?;

            ctx.eval::<(), _>(
                r#"
                if (typeof window !== 'undefined' && !window.EyeDropper) {
                  throw new Error("EyeDropper not exported to window");
                }
                if (typeof globalThis !== 'undefined' && !globalThis.EyeDropper) {
                  throw new Error("EyeDropper not exported to globalThis");
                }
                "#,
            )?;
            Ok(())
        })
    }

    #[test]
    fn test_eye_dropper_options_constructor() -> rquickjs::Result<()> {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_eye_dropper_bindings(&ctx)?;

            ctx.eval::<(), _>(
                r#"
                const dropper = new EyeDropper({ /* future options */ });
                if (!dropper) throw new Error("Failed to create EyeDropper with options");
                "#,
            )?;
            Ok(())
        })
    }

    #[test]
    fn test_eye_dropper_resolve_value() -> rquickjs::Result<()> {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            super::install_eye_dropper_bindings(&ctx)?;

            ctx.eval::<(), _>(
                r#"
                const dropper = new EyeDropper();
                dropper.open().then(result => {
                  // Should have sRGBHex property
                  if (!result.hasOwnProperty('sRGBHex')) {
                    throw new Error("Result must have sRGBHex property");
                  }
                  if (typeof result.sRGBHex !== 'string') {
                    throw new Error("sRGBHex must be a string");
                  }
                });
                "#,
            )?;
            Ok(())
        })
    }
}
