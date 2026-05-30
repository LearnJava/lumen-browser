//! Battery Status API disable stub (ADR-007 Layer 4, 9D.4).
//!
//! The Battery Status API (`navigator.getBattery()`) is a high-entropy
//! fingerprinting source: battery level, charging state, and charge/discharge
//! time together form a near-unique device signature. This module disables the
//! API by replacing `navigator.getBattery` with a function that returns a
//! rejected `Promise`, matching Chrome's behavior when the API is removed via
//! Permissions Policy.
//!
//! Must be called **after** `dom::install_dom_api` (requires `navigator` to exist).

use rquickjs::Ctx;

/// Install Battery Status API disable shim into the JS context.
///
/// Replaces `navigator.getBattery` with a function that returns a rejected
/// `Promise`. Sites that call `navigator.getBattery()` receive a rejection
/// rather than a real battery descriptor, preventing fingerprinting.
///
/// Silently no-ops if `navigator` is not yet defined (should not happen if
/// called after `install_dom_api`).
pub fn install_battery_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(BATTERY_SHIM)?;
    Ok(())
}

/// JavaScript shim: override `navigator.getBattery` to return a rejected Promise.
const BATTERY_SHIM: &str = r#"(function() {
  if (typeof navigator === 'undefined') return;
  navigator.getBattery = function() {
    return Promise.reject(new Error('Battery Status API is not supported'));
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

    /// Set up a minimal `navigator` stub so tests don't need the full DOM shim.
    fn install_nav(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>("var navigator = {};").unwrap();
    }

    #[test]
    fn install_succeeds_without_navigator() {
        // Should not panic even if navigator is undefined.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_battery_bindings(&ctx).expect("install should succeed");
        });
    }

    #[test]
    fn install_succeeds_with_navigator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_battery_bindings(&ctx).expect("install should succeed");
        });
    }

    #[test]
    fn get_battery_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_battery_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof navigator.getBattery").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn get_battery_returns_thenable() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_battery_bindings(&ctx).unwrap();
            let is_thenable: bool = ctx
                .eval(
                    "(function() { \
                       var p = navigator.getBattery(); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(is_thenable, "getBattery() must return a thenable Promise");
        });
    }

    #[test]
    fn get_battery_has_catch_method() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_battery_bindings(&ctx).unwrap();
            // The returned Promise must expose a `catch` handler for rejection handling.
            let has_catch: bool = ctx
                .eval(
                    "(function() { \
                       var p = navigator.getBattery(); \
                       return typeof p.catch === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(has_catch, "getBattery() must return a Promise with .catch");
        });
    }
}
