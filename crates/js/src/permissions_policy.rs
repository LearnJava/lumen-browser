//! Permissions Policy (Feature Policy) JS bindings.
//! <https://www.w3.org/TR/permissions-policy/#the-policy-object>
//!
//! Installs `document.featurePolicy` with the W3C `FeaturePolicy` interface.
//!
//! Phase 0: `featurePolicy.allowsFeature(name)` returns `true` by default
//! (policy data is recorded but enforcement is a Phase 1 task).

/// Install Permissions Policy JS bindings: `document.featurePolicy` (plus the
/// `document.permissionsPolicy` alias) and the
/// `_lumen_set_permissions_policy(headerValue)` native dispatch helper.
///
/// Must run after the DOM shim so that `document` and `window` are already
/// defined. Evaluates the JS shim via [`lumen_core::ext::JsRuntime::eval`] on
/// the default (V8) engine.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_permissions_policy_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(PERMISSIONS_POLICY_SHIM)?;
    Ok(())
}

/// JavaScript shim: FeaturePolicy interface + document.featurePolicy accessor.
#[cfg(feature = "v8-backend")]
const PERMISSIONS_POLICY_SHIM: &str = r#"
(function() {
  // ── Internal policy store ────────────────────────────────────────────────
  // Maps feature name → allowlist: '*' | 'none' | string[]
  var _ppStore = {};

  // ── FeaturePolicy interface (W3C Permissions Policy §8) ─────────────────
  // Exposed as document.featurePolicy (and document.permissionsPolicy alias).
  function FeaturePolicy() {}

  // Returns true if the feature is allowed for the given origin (default: 'self').
  // Phase 0: feature not in policy → true; policy entry '()' → false; else → true.
  FeaturePolicy.prototype.allowsFeature = function(feature, origin) {
    var entry = _ppStore[feature];
    if (entry === undefined) { return true; }  // default-allow for unlisted
    if (entry === 'none') { return false; }
    if (entry === '*') { return true; }
    var target = origin || 'self';
    return entry.indexOf(target) !== -1 || entry.indexOf('*') !== -1;
  };

  // Returns all feature names present in the active policy.
  FeaturePolicy.prototype.features = function() {
    return Object.keys(_ppStore);
  };

  // Returns feature names the current origin ('self') is allowed to use.
  FeaturePolicy.prototype.allowedFeatures = function() {
    return Object.keys(_ppStore).filter(function(f) {
      return FeaturePolicy.prototype.allowsFeature.call(this, f, 'self');
    }, this);
  };

  // Returns the allowlist for a specific feature as an array of origins.
  FeaturePolicy.prototype.getAllowlistForFeature = function(feature) {
    var entry = _ppStore[feature];
    if (entry === undefined || entry === '*') { return ['*']; }
    if (entry === 'none') { return []; }
    return entry.slice();
  };

  // Singleton installed on document.
  var _featurePolicyInstance = new FeaturePolicy();

  if (typeof document !== 'undefined') {
    // Install as non-configurable getter so it cannot be overwritten.
    Object.defineProperty(document, 'featurePolicy', {
      get: function() { return _featurePolicyInstance; },
      configurable: true,
      enumerable: false,
    });
    // W3C also specifies document.permissionsPolicy as the canonical name.
    Object.defineProperty(document, 'permissionsPolicy', {
      get: function() { return _featurePolicyInstance; },
      configurable: true,
      enumerable: false,
    });
  }

  // ── Native binding hook ─────────────────────────────────────────────────
  // Called by the Rust shell after HTTP response headers are received.
  // headerValue — raw Permissions-Policy header value (or Feature-Policy).
  // Phase 0: parses the header into _ppStore; Phase 1 shell enforces it.
  window._lumen_set_permissions_policy = function(headerValue) {
    _ppStore = {};
    if (!headerValue) { return; }
    var parts = headerValue.split(',');
    for (var i = 0; i < parts.length; i++) {
      var item = parts[i].trim();
      var eq = item.indexOf('=');
      if (eq === -1) { continue; }
      var name = item.slice(0, eq).trim().toLowerCase();
      var val = item.slice(eq + 1).trim();
      if (val === '*') {
        _ppStore[name] = '*';
      } else if (val === '()' || val === '') {
        _ppStore[name] = 'none';
      } else {
        // Strip parens and parse origin list.
        if (val.charAt(0) === '(') { val = val.slice(1); }
        if (val.charAt(val.length - 1) === ')') { val = val.slice(0, -1); }
        var origins = val.trim().split(/\s+/).map(function(o) {
          return o.replace(/^"|"$/g, '');  // strip surrounding quotes
        });
        _ppStore[name] = origins;
      }
    }
  };

  window.FeaturePolicy = FeaturePolicy;
})();
"#;

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Set up a minimal `window`/`document` stub plus the Permissions Policy shim
    /// on a bare V8 runtime — the shim only touches `document` and `window`, so no
    /// full `install_dom` is required. Evals on one runtime share global state, so
    /// the internal `_ppStore` persists across `eval` calls.
    fn with_pp_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            globalThis.window = globalThis;
            globalThis.document = {};
            "#,
        )
        .unwrap();
        install_permissions_policy_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn feature_policy_object_exists() {
        with_pp_api(|rt| {
            let ok = rt
                .eval("typeof document.featurePolicy === 'object'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn allows_feature_returns_true_by_default() {
        with_pp_api(|rt| {
            let ok = rt
                .eval("document.featurePolicy.allowsFeature('camera')")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn set_policy_disables_feature() {
        with_pp_api(|rt| {
            let ok = rt
                .eval(
                    "_lumen_set_permissions_policy('camera=(), microphone=*'); \
                     document.featurePolicy.allowsFeature('camera') === false && \
                     document.featurePolicy.allowsFeature('microphone') === true",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn features_returns_policy_names() {
        with_pp_api(|rt| {
            let ok = rt
                .eval(
                    "_lumen_set_permissions_policy('geolocation=(), usb=(self)'); \
                     var f = document.featurePolicy.features(); \
                     f.indexOf('geolocation') !== -1 && f.indexOf('usb') !== -1",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn permissions_policy_alias_exists() {
        with_pp_api(|rt| {
            let ok = rt
                .eval("typeof document.permissionsPolicy === 'object'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_allowlist_for_disabled_feature() {
        with_pp_api(|rt| {
            let ok = rt
                .eval(
                    "_lumen_set_permissions_policy('camera=()'); \
                     document.featurePolicy.getAllowlistForFeature('camera').length === 0",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
