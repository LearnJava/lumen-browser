/// Badging API (W3C Badging API).
///
/// Phase 0: no-op stubs.
/// - `navigator.setAppBadge(count?)` → Promise\<undefined\>
/// - `navigator.clearAppBadge()` → Promise\<undefined\>
///
/// Native binding `_lumen_set_app_badge(count)` is a no-op hook for shell Phase 1
/// (OS badge integration via Win32 taskbar / Linux Unity counter / macOS dock).
use rquickjs::Ctx;

/// Install Badging API bindings into the JS context.
pub fn install_badging_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(BADGING_SHIM)?;
    Ok(())
}

const BADGING_SHIM: &str = r#"
(function() {
  // W3C Badging API §3 — native hook for shell Phase 1 integration.
  // Phase 0: no-op; shell installs a real handler in Phase 1.
  if (typeof _lumen_set_app_badge === 'undefined') {
    globalThis._lumen_set_app_badge = function(_count) {};
  }

  // W3C Badging API §4.1: navigator.setAppBadge(contents?)
  // contents is either undefined (flag badge) or a non-negative integer.
  navigator.setAppBadge = function(contents) {
    var count = (typeof contents === 'undefined') ? null : (contents >>> 0);
    _lumen_set_app_badge(count);
    return Promise.resolve(undefined);
  };

  // W3C Badging API §4.2: navigator.clearAppBadge()
  navigator.clearAppBadge = function() {
    _lumen_set_app_badge(null);
    return Promise.resolve(undefined);
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

    fn with_badging(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                "#,
            )
            .unwrap();
            install_badging_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn set_app_badge_exists() {
        with_badging(|ctx| {
            let ok: bool = ctx
                .eval("typeof navigator.setAppBadge === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn clear_app_badge_exists() {
        with_badging(|ctx| {
            let ok: bool = ctx
                .eval("typeof navigator.clearAppBadge === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_app_badge_returns_promise() {
        with_badging(|ctx| {
            let ok: bool = ctx
                .eval("navigator.setAppBadge(3) instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn clear_app_badge_returns_promise() {
        with_badging(|ctx| {
            let ok: bool = ctx
                .eval("navigator.clearAppBadge() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }
}
