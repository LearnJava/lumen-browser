//! Permissions Policy (Feature Policy) JS bindings.
//! <https://www.w3.org/TR/permissions-policy/#the-policy-object>
//!
//! Installs `document.featurePolicy` with the W3C `FeaturePolicy` interface.
//!
//! Phase 0: `featurePolicy.allowsFeature(name)` returns `true` by default
//! (policy data is recorded but enforcement is a Phase 1 task).

use rquickjs::Ctx;

/// Install Permissions Policy JS bindings: `document.featurePolicy` and the
/// `_lumen_set_permissions_policy(headerValue)` native dispatch helper.
pub fn install_permissions_policy_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(PERMISSIONS_POLICY_SHIM)?;
    Ok(())
}

/// JavaScript shim: FeaturePolicy interface + document.featurePolicy accessor.
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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn with_pp_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var document = {};
                "#,
            )
            .unwrap();
            install_permissions_policy_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn feature_policy_object_exists() {
        with_pp_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof document.featurePolicy === 'object'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn allows_feature_returns_true_by_default() {
        with_pp_api(|ctx| {
            let ok: bool = ctx
                .eval("document.featurePolicy.allowsFeature('camera')")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_policy_disables_feature() {
        with_pp_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "_lumen_set_permissions_policy('camera=(), microphone=*'); \
                     document.featurePolicy.allowsFeature('camera') === false && \
                     document.featurePolicy.allowsFeature('microphone') === true",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn features_returns_policy_names() {
        with_pp_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "_lumen_set_permissions_policy('geolocation=(), usb=(self)'); \
                     var f = document.featurePolicy.features(); \
                     f.indexOf('geolocation') !== -1 && f.indexOf('usb') !== -1",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn permissions_policy_alias_exists() {
        with_pp_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof document.permissionsPolicy === 'object'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn get_allowlist_for_disabled_feature() {
        with_pp_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "_lumen_set_permissions_policy('camera=()'); \
                     document.featurePolicy.getAllowlistForFeature('camera').length === 0",
                )
                .unwrap();
            assert!(ok);
        });
    }
}
