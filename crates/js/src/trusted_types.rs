//! Trusted Types API (W3C Trusted Types §3)
//!
//! Phase 0 stub: `trustedTypes.createPolicy(name, rules)` creates a policy,
//! `TrustedHTML`, `TrustedScript`, `TrustedScriptURL` wrap strings,
//! `trustedTypes.defaultPolicy` is available, no enforcement.

use rquickjs::Ctx;

pub fn install_trusted_types_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(TRUSTED_TYPES_SHIM)?;
    Ok(())
}

const TRUSTED_TYPES_SHIM: &str = r#"
// Trusted Types API (Phase 0 stub)
globalThis.TrustedHTML = class {
  constructor(value) { this.value = value; }
  toString() { return this.value; }
  valueOf() { return this.value; }
  toJSON() { return this.value; }
};

globalThis.TrustedScript = class {
  constructor(value) { this.value = value; }
  toString() { return this.value; }
  valueOf() { return this.value; }
  toJSON() { return this.value; }
};

globalThis.TrustedScriptURL = class {
  constructor(value) { this.value = value; }
  toString() { return this.value; }
  valueOf() { return this.value; }
  toJSON() { return this.value; }
};

globalThis.TrustedURL = class {
  constructor(value) { this.value = value; }
  toString() { return this.value; }
  valueOf() { return this.value; }
  toJSON() { return this.value; }
};

globalThis.TrustedTypePolicy = class {
  constructor(name, rules) {
    this.name = name;
    this.rules = rules || {};
  }
  createHTML(input) {
    const val = typeof input === 'string' ? input : String(input);
    return new globalThis.TrustedHTML(val);
  }
  createScript(input) {
    const val = typeof input === 'string' ? input : String(input);
    return new globalThis.TrustedScript(val);
  }
  createScriptURL(input) {
    const val = typeof input === 'string' ? input : String(input);
    return new globalThis.TrustedScriptURL(val);
  }
  createURL(input) {
    const val = typeof input === 'string' ? input : String(input);
    return new globalThis.TrustedURL(val);
  }
};

globalThis.trustedTypes = {
  __policies: {},
  __defaultPolicy: null,
  createPolicy(name, rules) {
    if (this.__policies[name]) throw new TypeError('Policy already exists');
    return this.__policies[name] = new globalThis.TrustedTypePolicy(name, rules);
  },
  getPolicy(name) {
    return this.__policies[name] || null;
  },
  getPolicyNames() {
    return Object.keys(this.__policies);
  },
  get defaultPolicy() {
    if (!this.__defaultPolicy) {
      this.__defaultPolicy = new globalThis.TrustedTypePolicy('default-policy', {});
    }
    return this.__defaultPolicy;
  },
  isHTML(obj) { return obj instanceof globalThis.TrustedHTML; },
  isScript(obj) { return obj instanceof globalThis.TrustedScript; },
  isScriptURL(obj) { return obj instanceof globalThis.TrustedScriptURL; },
  isURL(obj) { return obj instanceof globalThis.TrustedURL; }
};

if (typeof window !== 'undefined') {
  window.TrustedHTML = globalThis.TrustedHTML;
  window.TrustedScript = globalThis.TrustedScript;
  window.TrustedScriptURL = globalThis.TrustedScriptURL;
  window.TrustedURL = globalThis.TrustedURL;
  window.TrustedTypePolicy = globalThis.TrustedTypePolicy;
  window.trustedTypes = globalThis.trustedTypes;
}
"#;
