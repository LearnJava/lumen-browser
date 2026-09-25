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
//! Phase 0: most DOM sinks (innerHTML etc.) still accept plain strings and
//! trusted values stringify transparently when assigned. TRUSTEDTYPES-1 срез 1
//! added the first enforced sink — `setTimeout`/`setInterval` string handlers
//! under `require-trusted-types-for 'script'` — via
//! `_lumen_tt_get_compliant_script`. Срез 2 added the HTML sink group
//! (`_lumen_tt_get_compliant_html`, TT L2 §4.1.1's "Get Trusted Type
//! compliant string" run with `expectedType` = `TrustedHTML`): `Element`/
//! `ShadowRoot` `innerHTML`/`outerHTML` setters, `insertAdjacentHTML`,
//! `setHTMLUnsafe`, `Document.write`/`writeln`. Срез 3 adds the remaining
//! table-driven sinks: `Element.setAttribute`/`setAttributeNS` (gated per
//! attribute through `_lumen_tt_get_compliant_attribute_value`, which reuses
//! the §4.4 `getAttributeType` sink table — `on*` → TrustedScript, `iframe
//! srcdoc` → TrustedHTML, `script src` → TrustedScriptURL, everything else
//! untouched), the matching IDL property pair (`HTMLScriptElement.src` via
//! the new `_lumen_tt_get_compliant_script_url`, `HTMLIFrameElement.srcdoc`
//! via `_lumen_tt_get_compliant_html`), and `Range.createContextualFragment`
//! (an HTML sink). Still unenforced: `eval`/`new Function` compile-time
//! interception (needs a V8 `ModifyCodeGenerationFromStrings`-style host
//! hook, not a JS-shim change), and the `<script>` `textContent`/`innerText`/
//! `.text` "internal slot" sinks (HTML LS's script-specific alternative
//! source-text mechanism, distinct from the plain attribute/property sinks
//! above) — see TRUSTEDTYPES-1 in ROADMAP.md for the remaining scope.

#[cfg(feature = "v8-backend")]
pub(crate) const TRUSTED_TYPES_SHIM: &str = r#"
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

  // TRUSTEDTYPES-1 срез 1: `require-trusted-types-for 'script'` (CSP3/TT L2
  // §4.1), set by the shell from the document's `<meta>` CSP once per
  // navigation — see `crates/shell/src/scripts.rs`.
  var REQUIRE_TT_FOR_SCRIPT = false;
  globalThis._lumen_tt_set_require_script = function (v) { REQUIRE_TT_FOR_SCRIPT = !!v; };

  // TT L2 §4.1.1 "Get Trusted Type compliant string", script subset: a
  // `TrustedScript` unwraps as-is; otherwise, under `require-trusted-types-for
  // 'script'`, a plain value must pass through `defaultPolicy.createScript`
  // (args: value, type name, sink name — matches the policy callback contract
  // used by `createPolicy`) or the sink throws. Without the CSP directive the
  // value is used verbatim (TT Phase 0 behaviour, unchanged for pages that
  // never opt in).
  globalThis._lumen_tt_get_compliant_script = function (input, sink) {
    if (input instanceof TrustedScript && VALUES.has(input)) return VALUES.get(input);
    var stringified = String(input);
    if (!REQUIRE_TT_FOR_SCRIPT) return stringified;
    if (defaultPolicy) {
      return String(defaultPolicy.createScript(stringified, 'TrustedScript', sink));
    }
    throw new TypeError(sink + " requires a Trusted Script value, no default policy is set.");
  };

  // TRUSTEDTYPES-1 срез 2: the HTML subset of the same §4.1.1 algorithm.
  // The `require-trusted-types-for 'script'` gate is not script-specific —
  // per spec it names the single defined sink *group*, which covers every
  // Trusted Types sink (HTML/Script/ScriptURL alike); `REQUIRE_TT_FOR_SCRIPT`
  // is reused here unchanged, matching the WPT fixtures that set only that
  // one CSP directive and then assert both script and HTML sinks throw.
  // `nullToEmpty` mirrors the IDL: `innerHTML`/`outerHTML`/`setHTMLUnsafe`
  // carry `[LegacyNullToEmptyString]` (DOM Parsing InnerHTML mixin) so a bare
  // `null` becomes '', while `insertAdjacentHTML`'s plain-`DOMString`
  // parameter stringifies `null` to the literal text "null" like any other
  // non-HTML-sink DOM API — both behaviours are asserted by the WPT fixtures
  // in tests/wpt/trusted-types/ (e.g. `HTMLElement-generic.html` vs.
  // `block-string-assignment-to-Element-insertAdjacentHTML.html`).
  globalThis._lumen_tt_get_compliant_html = function (input, sink, nullToEmpty) {
    if (input instanceof TrustedHTML && VALUES.has(input)) return VALUES.get(input);
    var stringified = (input === null && nullToEmpty) ? '' : String(input);
    if (!REQUIRE_TT_FOR_SCRIPT) return stringified;
    if (defaultPolicy) {
      return String(defaultPolicy.createHTML(stringified, 'TrustedHTML', sink));
    }
    throw new TypeError(sink + " requires a Trusted HTML value, no default policy is set.");
  };

  // TRUSTEDTYPES-1 срез 3: the ScriptURL subset of the same §4.1.1 algorithm
  // (TT L2 §4.1.1 "Get Trusted Type compliant string", expectedType =
  // TrustedScriptURL) — needed by the attribute-sink group below (`script.src`
  // as an attribute/property) and by any future navigational sink.
  globalThis._lumen_tt_get_compliant_script_url = function (input, sink) {
    if (input instanceof TrustedScriptURL && VALUES.has(input)) return VALUES.get(input);
    var stringified = String(input);
    if (!REQUIRE_TT_FOR_SCRIPT) return stringified;
    if (defaultPolicy) {
      return String(defaultPolicy.createScriptURL(stringified, 'TrustedScriptURL', sink));
    }
    throw new TypeError(sink + " requires a Trusted Script URL value, no default policy is set.");
  };

  // TRUSTEDTYPES-1 срез 3: TT L2 §4.1.1's "attribute type get algorithm" +
  // "Get Trusted Types compliant attribute value" run together — the entry
  // point `Element.setAttribute`/`setAttributeNS` call for every attribute
  // write, HTML/Script/ScriptURL alike. `getAttributeType` (TT §4.4's sink
  // table above) says which trusted interface (if any) the attribute expects;
  // an attribute outside that table (the overwhelming majority — `id`,
  // `class`, `href` on a plain `<a>`, ...) is untouched, matching Phase 0
  // behaviour. Sink naming mirrors the two spec-table entries this shim
  // knows by tag+attribute (`HTMLIFrameElement srcdoc`, `HTMLScriptElement
  // src`); every other TrustedScript sink (the `on*` handler family, per
  // GlobalEventHandlers) reports as the generic `Element <attrName>`, which
  // is what the WPT fixtures (`GlobalEventHandlers-onclick.html`,
  // `get-trusted-types-compliant-attribute-value.html`) assert for `onclick`.
  globalThis._lumen_tt_get_compliant_attribute_value = function (tagLower, attrLower, value) {
    var type = factory.getAttributeType(tagLower, attrLower);
    if (type === null) return String(value);
    var sink;
    if (tagLower === 'iframe' && attrLower === 'srcdoc') sink = 'HTMLIFrameElement srcdoc';
    else if (tagLower === 'script' && attrLower === 'src') sink = 'HTMLScriptElement src';
    else sink = 'Element ' + attrLower;
    if (type === 'TrustedHTML') return _lumen_tt_get_compliant_html(value, sink, false);
    if (type === 'TrustedScript') return _lumen_tt_get_compliant_script(value, sink);
    return _lumen_tt_get_compliant_script_url(value, sink);
  };

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
