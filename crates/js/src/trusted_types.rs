//! Trusted Types API (W3C Trusted Types L2, AA-5 Phase 0).
//!
//! Implements the policy machinery per spec: `trustedTypes.createPolicy(name, rules)`
//! invokes the policy's own rule callbacks (`createHTML`/`createScript`/`createScriptURL`),
//! throws `TypeError` when a rule is missing, registers `"default"` as `defaultPolicy`
//! exactly once (DefaultPolicy guard), and exposes `emptyHTML`/`emptyScript` plus the
//! `getAttributeType`/`getPropertyType` sink tables. Trusted value objects
//! (`TrustedHTML`/`TrustedScript`/`TrustedScriptURL`) carry an internal brand (WeakMap)
//! and are not constructible from page script ("Illegal constructor").
//!
//! Phase 0: no sink enforcement — DOM sinks (innerHTML etc.) keep accepting plain
//! strings; trusted values stringify transparently when assigned.

use rquickjs::Ctx;

/// Installs `window.trustedTypes`, the three trusted value classes and
/// `TrustedTypePolicy` into the global scope of the given QuickJS context.
pub fn install_trusted_types_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(TRUSTED_TYPES_SHIM)?;
    Ok(())
}

const TRUSTED_TYPES_SHIM: &str = r#"
// Trusted Types API (W3C TT L2, Phase 0: no sink enforcement).
(function () {
  'use strict';
  // Construction token: only code inside this closure can mint trusted values.
  var SECRET = Symbol('trusted-types-secret');
  // Brand + payload storage; a faked prototype chain without a VALUES entry
  // is rejected by isHTML/isScript/isScriptURL.
  var VALUES = new WeakMap();

  function makeTrustedClass(className) {
    function T(token, value) {
      if (token !== SECRET) throw new TypeError('Illegal constructor');
      VALUES.set(this, String(value));
    }
    T.prototype.toString = function () { return VALUES.get(this); };
    T.prototype.toJSON = function () { return VALUES.get(this); };
    Object.defineProperty(T, 'name', { value: className, configurable: true });
    return T;
  }

  var TrustedHTML = makeTrustedClass('TrustedHTML');
  var TrustedScript = makeTrustedClass('TrustedScript');
  var TrustedScriptURL = makeTrustedClass('TrustedScriptURL');

  var POLICY_RULES = new WeakMap();

  // TT §3.2: invoke the policy's rule callback; missing rule => TypeError.
  function runRule(policy, ruleName, Ctor, input, args) {
    var rules = POLICY_RULES.get(policy);
    if (!rules || typeof rules[ruleName] !== 'function') {
      throw new TypeError(
        "Policy " + policy.name + "'s TrustedTypePolicyOptions did not specify a '" +
        ruleName + "' member");
    }
    var result = rules[ruleName].apply(undefined, [String(input)].concat(args));
    return new Ctor(SECRET, result);
  }

  function TrustedTypePolicy(token, name, rules) {
    if (token !== SECRET) throw new TypeError('Illegal constructor');
    Object.defineProperty(this, 'name', { value: String(name), enumerable: true });
    // Snapshot the three callbacks (spec: options are read once at creation).
    POLICY_RULES.set(this, {
      createHTML: rules && rules.createHTML,
      createScript: rules && rules.createScript,
      createScriptURL: rules && rules.createScriptURL
    });
  }
  TrustedTypePolicy.prototype.createHTML = function (input) {
    return runRule(this, 'createHTML', TrustedHTML, input, Array.prototype.slice.call(arguments, 1));
  };
  TrustedTypePolicy.prototype.createScript = function (input) {
    return runRule(this, 'createScript', TrustedScript, input, Array.prototype.slice.call(arguments, 1));
  };
  TrustedTypePolicy.prototype.createScriptURL = function (input) {
    return runRule(this, 'createScriptURL', TrustedScriptURL, input, Array.prototype.slice.call(arguments, 1));
  };

  var defaultPolicy = null;
  var EMPTY_HTML = new TrustedHTML(SECRET, '');
  var EMPTY_SCRIPT = new TrustedScript(SECRET, '');

  // TrustedTypePolicyFactory (the window.trustedTypes singleton).
  var factory = {
    createPolicy: function (name, rules) {
      name = String(name);
      // DefaultPolicy guard: "default" is registered once; a second
      // registration throws (TT §4.3). Duplicate non-default names are
      // allowed without a CSP trusted-types directive (Phase 0: no CSP).
      if (name === 'default') {
        if (defaultPolicy) throw new TypeError('Policy with name "default" already exists');
        defaultPolicy = new TrustedTypePolicy(SECRET, name, rules);
        return defaultPolicy;
      }
      return new TrustedTypePolicy(SECRET, name, rules);
    },
    get defaultPolicy() { return defaultPolicy; },
    // Brand checks: instanceof alone is forgeable via Object.create.
    isHTML: function (v) { return v instanceof TrustedHTML && VALUES.has(v); },
    isScript: function (v) { return v instanceof TrustedScript && VALUES.has(v); },
    isScriptURL: function (v) { return v instanceof TrustedScriptURL && VALUES.has(v); },
    get emptyHTML() { return EMPTY_HTML; },
    get emptyScript() { return EMPTY_SCRIPT; },
    // TT §4.4 sink tables (minimal Phase 0 subset).
    getAttributeType: function (tagName, attribute) {
      tagName = String(tagName).toLowerCase();
      attribute = String(attribute).toLowerCase();
      if (attribute.length > 2 && attribute.indexOf('on') === 0) return 'TrustedScript';
      if (tagName === 'iframe' && attribute === 'srcdoc') return 'TrustedHTML';
      if (tagName === 'script' && attribute === 'src') return 'TrustedScriptURL';
      return null;
    },
    getPropertyType: function (tagName, property) {
      tagName = String(tagName).toLowerCase();
      property = String(property);
      if (property === 'innerHTML' || property === 'outerHTML') return 'TrustedHTML';
      if (tagName === 'script') {
        if (property === 'src') return 'TrustedScriptURL';
        if (property === 'text' || property === 'textContent' || property === 'innerText') {
          return 'TrustedScript';
        }
      }
      return null;
    }
  };

  globalThis.TrustedHTML = TrustedHTML;
  globalThis.TrustedScript = TrustedScript;
  globalThis.TrustedScriptURL = TrustedScriptURL;
  globalThis.TrustedTypePolicy = TrustedTypePolicy;
  globalThis.trustedTypes = factory;
  if (typeof window !== 'undefined') {
    window.TrustedHTML = TrustedHTML;
    window.TrustedScript = TrustedScript;
    window.TrustedScriptURL = TrustedScriptURL;
    window.TrustedTypePolicy = TrustedTypePolicy;
    window.trustedTypes = factory;
  }
})();
"#;
