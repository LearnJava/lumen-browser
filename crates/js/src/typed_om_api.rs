//! CSS Typed Object Model L1 API (CSSOM L1 §5, CSS Typed OM L1)
//!
//! Provides `element.attributeStyleMap` (StylePropertyMap) and `element.computedStyleMap()`
//! (ComputedStylePropertyMap) access to CSS values via `CSSStyleValue` objects.
//!
//! Classes:
//! - `CSSStyleValue` — base class for all CSS values
//! - `CSSUnitValue` — numeric value with unit (e.g. 10px, 2.5em)
//! - `CSSKeywordValue` — keyword value (e.g. auto, inherit)
//! - `CSSNumericValue` — base class for numeric values (not fully implemented in Phase 0)
//!
//! Maps:
//! - `StylePropertyMap` — element.attributeStyleMap (mutable)
//! - `ComputedStylePropertyMap` — element.computedStyleMap() (read-only)

use rquickjs::Ctx;

/// Install CSS Typed OM API bindings.
/// Installs JS class definitions and integrates into Element prototype.
pub fn install_typed_om_api(ctx: &Ctx) -> rquickjs::Result<()> {
    // Install CSSStyleValue class hierarchy and property maps
    ctx.eval::<(), _>(TYPED_OM_SHIM)?;
    Ok(())
}

/// Pure-JS CSS Typed OM L1 shim.
/// Defines CSSStyleValue hierarchy and StylePropertyMap / ComputedStylePropertyMap classes.
const TYPED_OM_SHIM: &str = r#"(function(global) {
  'use strict';

  // ── CSSStyleValue — base class for all CSS values ────────────────────────────
  function CSSStyleValue(cssText) {
    this.cssText = String(cssText || '');
  }
  CSSStyleValue.prototype.toString = function() {
    return this.cssText;
  };

  // ── CSSUnitValue — numeric value with unit ────────────────────────────────────
  function CSSUnitValue(value, unit) {
    CSSStyleValue.call(this, (Number(value) || 0) + String(unit || 'px'));
    this.value = Number(value) || 0;
    this.unit = String(unit || 'px');
  }
  CSSUnitValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSUnitValue.prototype.constructor = CSSUnitValue;
  CSSUnitValue.prototype.to = function(newUnit) {
    // Phase 0: simple string conversion (no actual unit conversion)
    return new CSSUnitValue(this.value, newUnit);
  };

  // ── CSSKeywordValue — keyword value ────────────────────────────────────────────
  function CSSKeywordValue(value) {
    CSSStyleValue.call(this, String(value));
    this.value = String(value);
  }
  CSSKeywordValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSKeywordValue.prototype.constructor = CSSKeywordValue;

  // ── CSSNumericValue — base for numeric operations ────────────────────────────
  function CSSNumericValue() {
    CSSStyleValue.call(this);
  }
  CSSNumericValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSNumericValue.prototype.constructor = CSSNumericValue;

  // ── StylePropertyMap — mutable map of CSS properties (element.attributeStyleMap) ─
  function StylePropertyMap(nid) {
    this.__nid__ = nid;
  }
  StylePropertyMap.prototype.get = function(prop) {
    var val = _lumen_get_style_property(this.__nid__, String(prop));
    if (!val) return undefined;
    // Phase 0: return CSSUnitValue if numeric, else CSSKeywordValue
    if (/^\d+(\.\d+)?(px|em|rem|vh|vw|deg|rad|s|ms|%)$/.test(val)) {
      var match = val.match(/^([\d.]+)(.*?)$/);
      return new CSSUnitValue(Number(match[1]), match[2]);
    }
    return new CSSKeywordValue(val);
  };
  StylePropertyMap.prototype.set = function(prop, value) {
    var val;
    if (value instanceof CSSStyleValue) {
      val = value.cssText;
    } else if (value && typeof value === 'object' && value.cssText !== undefined) {
      val = value.cssText;
    } else {
      val = String(value);
    }
    _lumen_set_style_property(this.__nid__, String(prop), val);
  };
  StylePropertyMap.prototype.delete = function(prop) {
    _lumen_delete_style_property(this.__nid__, String(prop));
  };
  StylePropertyMap.prototype.has = function(prop) {
    return _lumen_has_style_property(this.__nid__, String(prop));
  };
  StylePropertyMap.prototype.entries = function() {
    return _lumen_get_style_entries(this.__nid__).entries();
  };
  StylePropertyMap.prototype.keys = function() {
    return _lumen_get_style_entries(this.__nid__).keys();
  };
  StylePropertyMap.prototype.values = function() {
    return _lumen_get_style_entries(this.__nid__).values();
  };

  // ── ComputedStylePropertyMap — read-only computed styles ─────────────────────
  function ComputedStylePropertyMap(nid) {
    this.__nid__ = nid;
    this.__readOnly__ = true;
  }
  ComputedStylePropertyMap.prototype = Object.create(StylePropertyMap.prototype);
  ComputedStylePropertyMap.prototype.constructor = ComputedStylePropertyMap;
  ComputedStylePropertyMap.prototype.set = function() {
    throw new TypeError('ComputedStylePropertyMap is read-only');
  };
  ComputedStylePropertyMap.prototype.delete = function() {
    throw new TypeError('ComputedStylePropertyMap is read-only');
  };

  // ── Export classes to global ──────────────────────────────────────────────────
  if (typeof global.CSS !== 'object') global.CSS = {};
  global.CSS.CSSStyleValue = CSSStyleValue;
  global.CSS.CSSUnitValue = CSSUnitValue;
  global.CSS.CSSKeywordValue = CSSKeywordValue;
  global.CSS.CSSNumericValue = CSSNumericValue;
  global.CSS.StylePropertyMap = StylePropertyMap;
  global.CSS.ComputedStylePropertyMap = ComputedStylePropertyMap;

  // ── Window/globalThis reference ───────────────────────────────────────────────
  if (typeof window === 'object' && window) {
    window.CSSStyleValue = CSSStyleValue;
    window.CSSUnitValue = CSSUnitValue;
    window.CSSKeywordValue = CSSKeywordValue;
    window.CSSNumericValue = CSSNumericValue;
  }
})(typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : typeof window !== 'undefined' ? window : this);
"#;
