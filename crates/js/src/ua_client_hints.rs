/// User-Agent Client Hints (W3C UA-CH §4–6)
/// Phase 0: static Chrome 114 profile — all values are fixed.
/// `navigator.userAgentData` exposes low-entropy values directly.
/// `getHighEntropyValues(hints)` returns Promise<UADataValues> with static fields.
use rquickjs::Ctx;

/// Install User-Agent Client Hints bindings into the JS context.
///
/// Adds `navigator.userAgentData` (a `NavigatorUAData` instance) and exports
/// `NavigatorUAData` on `globalThis`. Phase 0: static Chrome 114 / Windows 10 profile.
pub fn install_ua_client_hints_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(UA_CLIENT_HINTS_SHIM)?;
    Ok(())
}

const UA_CLIENT_HINTS_SHIM: &str = r#"
(function() {
  // NavigatorUABrandVersion — one entry in the brands / fullVersionList arrays.
  function NavigatorUABrandVersion(brand, version) {
    this.brand = brand;
    this.version = version;
  }
  NavigatorUABrandVersion.prototype.toJSON = function() {
    return { brand: this.brand, version: this.version };
  };

  // Low-entropy brand list (reported without permission).
  var _brands = [
    new NavigatorUABrandVersion("Not A;Brand", "99"),
    new NavigatorUABrandVersion("Chromium", "114"),
    new NavigatorUABrandVersion("Google Chrome", "114")
  ];

  // High-entropy full-version list.
  var _fullVersionList = [
    new NavigatorUABrandVersion("Not A;Brand", "99.0.0.0"),
    new NavigatorUABrandVersion("Chromium", "114.0.5735.133"),
    new NavigatorUABrandVersion("Google Chrome", "114.0.5735.133")
  ];

  // Static high-entropy values (Phase 0: fixed Chrome 114 / Windows 10 x64).
  var _highEntropy = {
    platform:        "Windows",
    platformVersion: "10.0.0",
    architecture:    "x86",
    bitness:         "64",
    model:           "",
    uaFullVersion:   "114.0.5735.133",
    wow64:           false
  };

  // NavigatorUAData — the object exposed as navigator.userAgentData.
  function NavigatorUAData() {}

  // Low-entropy accessors.
  Object.defineProperty(NavigatorUAData.prototype, 'brands', {
    get: function() { return _brands.slice(); },
    enumerable: true, configurable: true
  });
  Object.defineProperty(NavigatorUAData.prototype, 'mobile', {
    get: function() { return false; },
    enumerable: true, configurable: true
  });
  Object.defineProperty(NavigatorUAData.prototype, 'platform', {
    get: function() { return "Windows"; },
    enumerable: true, configurable: true
  });

  // High-entropy accessor — returns Promise<UADataValues>.
  // Resolves immediately with the requested subset of static values.
  NavigatorUAData.prototype.getHighEntropyValues = function(hints) {
    if (!Array.isArray(hints)) {
      return Promise.reject(new TypeError('hints must be an array'));
    }
    var result = {};
    // Always include low-entropy fields in the resolved object.
    result.brands   = _brands.slice();
    result.mobile   = false;
    result.platform = "Windows";
    // Add each requested high-entropy hint.
    for (var i = 0; i < hints.length; i++) {
      var h = hints[i];
      if (h === 'fullVersionList') {
        result.fullVersionList = _fullVersionList.slice();
      } else if (h in _highEntropy) {
        result[h] = _highEntropy[h];
      }
    }
    return Promise.resolve(result);
  };

  // W3C §6.1 serialisation.
  NavigatorUAData.prototype.toJSON = function() {
    return {
      brands:   _brands.map(function(b) { return b.toJSON(); }),
      mobile:   false,
      platform: "Windows"
    };
  };

  // Expose class on globalThis (window alias when available).
  globalThis.NavigatorUAData = NavigatorUAData;

  // Install navigator.userAgentData.
  if (typeof navigator !== 'undefined') {
    try {
      Object.defineProperty(navigator, 'userAgentData', {
        value: new NavigatorUAData(),
        writable: false,
        configurable: true,
        enumerable: true
      });
    } catch(_) {}
  }
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

    fn with_ua_hints_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                globalThis.navigator = navigator;
                function DOMException(message, name) {
                  Error.call(this, message);
                  this.message = message;
                  this.name = name || 'Error';
                }
                DOMException.prototype = Object.create(Error.prototype);
                DOMException.prototype.constructor = DOMException;
                globalThis.DOMException = DOMException;
                "#,
            )
            .unwrap();
            install_ua_client_hints_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn navigator_ua_data_exists() {
        with_ua_hints_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof navigator.userAgentData === 'object' && navigator.userAgentData !== null",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn low_entropy_brands_mobile_platform() {
        with_ua_hints_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = navigator.userAgentData;
                    d.brands.length === 3 &&
                    d.mobile === false &&
                    d.platform === "Windows"
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn get_high_entropy_values_returns_promise() {
        with_ua_hints_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "navigator.userAgentData.getHighEntropyValues(['platformVersion']) instanceof Promise",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn get_high_entropy_values_resolves_with_platform_version() {
        let (rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                globalThis.navigator = navigator;
                "#,
            )
            .unwrap();
            install_ua_client_hints_bindings(&ctx).unwrap();
            // Schedule the .then() callback — it runs as a microtask.
            ctx.eval::<(), _>(
                r#"
                var _result = null;
                navigator.userAgentData
                  .getHighEntropyValues(['platformVersion', 'architecture', 'bitness'])
                  .then(function(v) { _result = v; });
                "#,
            )
            .unwrap();
        });
        // Drain QuickJS microtask queue so the .then() callback executes.
        while rt.execute_pending_job().unwrap_or(false) {}
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    _result !== null &&
                    _result.platformVersion === "10.0.0" &&
                    _result.architecture === "x86" &&
                    _result.bitness === "64"
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
