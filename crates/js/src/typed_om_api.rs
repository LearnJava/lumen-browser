//! CSS Typed Object Model L1 API (CSS Typed OM L1).
//!
//! Provides `element.attributeStyleMap` (StylePropertyMap) and `element.computedStyleMap()`
//! (StylePropertyMapReadOnly) access to CSS values via `CSSStyleValue` objects.
//!
//! Classes:
//! - `CSSStyleValue` — base class for all CSS values; `.parse`/`.parseAll` fall back to the
//!   same generic dimension/identifier/opaque split used for reading the cascade (no
//!   per-property grammar table)
//! - `CSSUnitValue` — numeric value with unit (e.g. 10px, 2.5em); `CSS.px(1)` and friends on
//!   the `CSS` namespace construct one per unit name
//! - `CSSKeywordValue` — keyword value (e.g. auto, inherit)
//! - `CSSNumericValue` — base class for numeric values; `add`/`sub`/`mul`/`div`/`min`/`max`
//!   build a `CSSMathValue` tree (construction + `calc()` serialisation only — no resolution
//!   context, so `to()`/`equals()` on a math value are not implemented)
//! - `CSSMathValue` family — `CSSMathSum`/`CSSMathProduct`/`CSSMathNegate`/`CSSMathInvert`/
//!   `CSSMathMin`/`CSSMathMax`
//! - `CSSUnparsedValue`/`CSSVariableReferenceValue` — `var()` reference values
//!
//! Not implemented: `CSSTransformValue`/`CSSColorValue` families (GAP-TYPEDOM remainder).
//!
//! Maps:
//! - `StylePropertyMapReadOnly` — `element.computedStyleMap()`, reads the resolved cascade
//! - `StylePropertyMap` — `element.attributeStyleMap`, reflects the inline `style=""` attribute
//!
//! The inheritance direction matters and is the spec's (§6): the mutable map
//! **extends** the read-only one. Lumen had it inverted until BUG-387 — the
//! computed map extended the inline one and inherited its reader, so
//! `computedStyleMap().get(prop)` answered `undefined` for every property that
//! came from a stylesheet rule rather than from `style=""`.

/// V8 port of the former rquickjs `install_typed_om_api` (Ph3 V8 migration S5-S7): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_typed_om_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(TYPED_OM_SHIM)?;
    Ok(())
}

/// Pure-JS CSS Typed OM L1 shim.
/// Defines the CSSStyleValue hierarchy and the StylePropertyMapReadOnly / StylePropertyMap classes.
#[cfg(feature = "v8-backend")]
const TYPED_OM_SHIM: &str = r#"(function(global) {
  'use strict';

  // ── CSSStyleValue — base class for all CSS values ────────────────────────────
  function CSSStyleValue(cssText) {
    this.cssText = String(cssText == null ? '' : cssText);
  }
  CSSStyleValue.prototype.toString = function() {
    return this.cssText;
  };

  // CSS Typed OM L1 §4.2 spells the unitless and percentage units 'number' and
  // 'percent', but they serialise as '' and '%'.
  var UNIT_SUFFIX = { number: '', percent: '%' };
  function unitSuffix(unit) {
    return Object.prototype.hasOwnProperty.call(UNIT_SUFFIX, unit) ? UNIT_SUFFIX[unit] : unit;
  }
  function normaliseUnit(unit) {
    if (unit === undefined || unit === null || unit === '') return 'number';
    var u = String(unit);
    if (u === '%') return 'percent';
    return u.toLowerCase();
  }

  // Absolute unit groups, each expressed in its group's canonical unit. Only
  // conversions inside one group are defined without a resolution context —
  // 'em'/'vh'/'percent' and friends deliberately appear nowhere here, so
  // `to()` reports them as unconvertible instead of inventing a factor.
  var UNIT_GROUPS = [
    { px:   1, cm: 96 / 2.54, mm: 96 / 25.4, q: 96 / 101.6, in: 96, pt: 96 / 72, pc: 16 },
    { deg:  1, grad: 0.9, rad: 180 / Math.PI, turn: 360 },
    { ms:   1, s: 1000 },
    { hz:   1, khz: 1000 },
    { dppx: 1, dpi: 1 / 96, dpcm: 2.54 / 96 }
  ];
  function conversionFactor(from, to) {
    if (from === to) return 1;
    for (var i = 0; i < UNIT_GROUPS.length; i++) {
      var g = UNIT_GROUPS[i];
      if (Object.prototype.hasOwnProperty.call(g, from) &&
          Object.prototype.hasOwnProperty.call(g, to)) {
        return g[from] / g[to];
      }
    }
    return null;
  }

  // ── CSSNumericValue — base for numeric operations (§7) ────────────────────────
  // Spec order is CSSNumericValue before CSSUnitValue/CSSMathValue: both extend
  // it, and `add()`/`sub()`/... below dispatch on `instanceof CSSNumericValue`.
  function CSSNumericValue() {
    CSSStyleValue.call(this);
  }
  CSSNumericValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSNumericValue.prototype.constructor = CSSNumericValue;

  // ── CSSUnitValue — numeric value with unit ────────────────────────────────────
  function CSSUnitValue(value, unit) {
    CSSNumericValue.call(this);
    var v = Number(value) || 0;
    var u = normaliseUnit(unit === undefined ? 'px' : unit);
    this.cssText = String(v) + unitSuffix(u);
    this.value = v;
    this.unit = u;
  }
  CSSUnitValue.prototype = Object.create(CSSNumericValue.prototype);
  CSSUnitValue.prototype.constructor = CSSUnitValue;
  CSSUnitValue.prototype.to = function(newUnit) {
    var target = normaliseUnit(newUnit);
    var factor = conversionFactor(this.unit, target);
    // §4.5.1 `to()` throws when the conversion is not defined. Returning the
    // number unchanged under the new unit label — what this did before
    // BUG-387 — is a silently wrong value, which is worse than no answer.
    if (factor === null) {
      throw new TypeError("CSSUnitValue.to: cannot convert '" + this.unit + "' to '" + target + "'");
    }
    return new CSSUnitValue(this.value * factor, target);
  };

  // ── CSSKeywordValue — keyword value ────────────────────────────────────────────
  function CSSKeywordValue(value) {
    CSSStyleValue.call(this, String(value));
    this.value = String(value);
  }
  CSSKeywordValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSKeywordValue.prototype.constructor = CSSKeywordValue;

  // ── CSSMathValue hierarchy — arithmetic on numeric values (§8) ────────────────
  // Construction and serialisation only: resolving a mixed-unit tree to a single
  // numeric value (`to()`/`equals()` on a CSSMathValue, CSSMathSum.values as a
  // CSSNumericArray with unit-typed elements) needs a resolution context this
  // slice does not add. `add()`/`sub()`/`mul()`/`div()`/`min()`/`max()` on
  // CSSNumericValue build a correctly `calc()`-serialising tree, which is what
  // `style.set('width', a.add(b))` actually consumes downstream.
  function toNumericValue(v) {
    if (v instanceof CSSNumericValue) return v;
    if (typeof v === 'number') return new CSSUnitValue(v, 'number');
    throw new TypeError('CSSNumericValue: operand is not a CSSNumericValue or number');
  }

  function CSSMathValue(operator) {
    CSSNumericValue.call(this);
    this.operator = operator;
  }
  CSSMathValue.prototype = Object.create(CSSNumericValue.prototype);
  CSSMathValue.prototype.constructor = CSSMathValue;

  function CSSMathSum(values) {
    CSSMathValue.call(this, 'sum');
    this.values = values;
    this.cssText = 'calc(' + values.map(function(v) { return v.toString(); }).join(' + ') + ')';
  }
  CSSMathSum.prototype = Object.create(CSSMathValue.prototype);
  CSSMathSum.prototype.constructor = CSSMathSum;

  function CSSMathProduct(values) {
    CSSMathValue.call(this, 'product');
    this.values = values;
    this.cssText = 'calc(' + values.map(function(v) { return v.toString(); }).join(' * ') + ')';
  }
  CSSMathProduct.prototype = Object.create(CSSMathValue.prototype);
  CSSMathProduct.prototype.constructor = CSSMathProduct;

  function CSSMathNegate(value) {
    CSSMathValue.call(this, 'negate');
    this.value = value;
    this.cssText = 'calc(-1 * ' + value.toString() + ')';
  }
  CSSMathNegate.prototype = Object.create(CSSMathValue.prototype);
  CSSMathNegate.prototype.constructor = CSSMathNegate;

  function CSSMathInvert(value) {
    CSSMathValue.call(this, 'invert');
    this.value = value;
    this.cssText = 'calc(1 / ' + value.toString() + ')';
  }
  CSSMathInvert.prototype = Object.create(CSSMathValue.prototype);
  CSSMathInvert.prototype.constructor = CSSMathInvert;

  function CSSMathMin(values) {
    CSSMathValue.call(this, 'min');
    this.values = values;
    this.cssText = 'min(' + values.map(function(v) { return v.toString(); }).join(', ') + ')';
  }
  CSSMathMin.prototype = Object.create(CSSMathValue.prototype);
  CSSMathMin.prototype.constructor = CSSMathMin;

  function CSSMathMax(values) {
    CSSMathValue.call(this, 'max');
    this.values = values;
    this.cssText = 'max(' + values.map(function(v) { return v.toString(); }).join(', ') + ')';
  }
  CSSMathMax.prototype = Object.create(CSSMathValue.prototype);
  CSSMathMax.prototype.constructor = CSSMathMax;

  // §7.4: flatten same-operator sums/products into one, so `a.add(b).add(c)`
  // serialises as `calc(a + b + c)` rather than nesting `calc(calc(a + b) + c)`.
  function flattenSameOperator(values, ctor) {
    var out = [];
    values.forEach(function(v) {
      if (v instanceof ctor) {
        out = out.concat(v.values);
      } else {
        out.push(v);
      }
    });
    return out;
  }

  CSSNumericValue.prototype.add = function() {
    var operands = [this].concat(Array.prototype.slice.call(arguments).map(toNumericValue));
    return new CSSMathSum(flattenSameOperator(operands, CSSMathSum));
  };
  CSSNumericValue.prototype.sub = function() {
    var operands = [this].concat(Array.prototype.slice.call(arguments).map(function(v) {
      return new CSSMathNegate(toNumericValue(v));
    }));
    return new CSSMathSum(flattenSameOperator(operands, CSSMathSum));
  };
  CSSNumericValue.prototype.mul = function() {
    var operands = [this].concat(Array.prototype.slice.call(arguments).map(toNumericValue));
    return new CSSMathProduct(flattenSameOperator(operands, CSSMathProduct));
  };
  CSSNumericValue.prototype.div = function() {
    var operands = [this].concat(Array.prototype.slice.call(arguments).map(function(v) {
      return new CSSMathInvert(toNumericValue(v));
    }));
    return new CSSMathProduct(flattenSameOperator(operands, CSSMathProduct));
  };
  CSSNumericValue.prototype.min = function() {
    var operands = [this].concat(Array.prototype.slice.call(arguments).map(toNumericValue));
    return new CSSMathMin(operands);
  };
  CSSNumericValue.prototype.max = function() {
    var operands = [this].concat(Array.prototype.slice.call(arguments).map(toNumericValue));
    return new CSSMathMax(operands);
  };
  CSSNumericValue.prototype.negate = function() {
    return new CSSMathNegate(this);
  };
  CSSNumericValue.prototype.invert = function() {
    return new CSSMathInvert(this);
  };

  // ── CSSUnparsedValue / CSSVariableReferenceValue (§9) — var() references ──────
  function CSSVariableReferenceValue(variable, fallback) {
    var name = String(variable);
    if (name.slice(0, 2) !== '--') {
      throw new TypeError('CSSVariableReferenceValue: "' + name + '" is not a custom property name');
    }
    this.variable = name;
    this.fallback = fallback === undefined ? null : fallback;
  }
  CSSVariableReferenceValue.prototype.toString = function() {
    return 'var(' + this.variable + (this.fallback ? ', ' + this.fallback.toString() : '') + ')';
  };

  function CSSUnparsedValue(members) {
    var list = Array.prototype.slice.call(members || []);
    for (var i = 0; i < list.length; i++) {
      if (typeof list[i] !== 'string' && !(list[i] instanceof CSSVariableReferenceValue)) {
        throw new TypeError('CSSUnparsedValue: member ' + i + ' is not a string or CSSVariableReferenceValue');
      }
      this[i] = list[i];
    }
    this.length = list.length;
  }
  CSSUnparsedValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSUnparsedValue.prototype.constructor = CSSUnparsedValue;
  Object.defineProperty(CSSUnparsedValue.prototype, 'cssText', {
    get: function() {
      var out = '';
      for (var i = 0; i < this.length; i++) {
        out += this[i].toString();
      }
      return out;
    },
    configurable: true
  });
  if (typeof Symbol !== 'undefined' && Symbol.iterator) {
    CSSUnparsedValue.prototype[Symbol.iterator] = function() {
      var self = this, i = 0;
      return { next: function() {
        return i < self.length ? { value: self[i++], done: false } : { value: undefined, done: true };
      } };
    };
  }

  var NUMBER_WITH_UNIT = /^([+-]?(?:\d+(?:\.\d+)?|\.\d+))(%|[a-zA-Z]+)?$/;
  var CSS_IDENTIFIER   = /^-?[A-Za-z_][\w-]*$/;

  // Wraps a resolved CSS string in the most specific CSSStyleValue subclass that
  // fits it. Anything that is neither a dimension nor a bare identifier —
  // 'rgb(0, 128, 0)', '10px 20px', '"Inter", sans-serif' — becomes a plain
  // CSSStyleValue: calling it a CSSKeywordValue (what this did before BUG-387)
  // claims it is a single CSS identifier, which it is not.
  function cssValueFromString(css) {
    var m = NUMBER_WITH_UNIT.exec(css);
    if (m) return new CSSUnitValue(Number(m[1]), m[2] === undefined ? 'number' : m[2]);
    if (CSS_IDENTIFIER.test(css)) return new CSSKeywordValue(css);
    return new CSSStyleValue(css);
  }

  // A custom property (`--`-prefixed) is case-sensitive and never spelled
  // camelCase — it must reach the engine verbatim.
  function camelToKebab(name) {
    if (name.slice(0, 2) === '--') return name;
    return name.replace(/[A-Z]/g, function(c) { return '-' + c.toLowerCase(); });
  }

  // Splits on commas that are not inside a nested `(...)` — the same rule
  // `parseAll` needs to break e.g. `1px, calc(1px, 2px)` (not a real property
  // value, but the split rule is unit-agnostic) into top-level items only.
  function splitTopLevelCommas(text) {
    var parts = [];
    var depth = 0;
    var start = 0;
    for (var i = 0; i < text.length; i++) {
      var c = text.charAt(i);
      if (c === '(') depth++;
      else if (c === ')') depth--;
      else if (c === ',' && depth === 0) {
        parts.push(text.slice(start, i));
        start = i + 1;
      }
    }
    parts.push(text.slice(start));
    return parts.map(function(s) { return s.trim(); }).filter(function(s) { return s.length > 0; });
  }

  // ── CSSStyleValue.parse / .parseAll (§4.3) ─────────────────────────────────────
  // `property` is accepted but not consulted for a property-specific grammar —
  // Lumen has no per-property Typed OM parser table yet, so both fall back to
  // the same generic dimension/identifier/opaque split `cssValueFromString`
  // uses for reading the cascade. Good enough for round-tripping a value this
  // API itself produced; a value needing real property-aware parsing (e.g.
  // rejecting `10px` for `color`) is not caught here.
  CSSStyleValue.parse = function(property, cssText) {
    var text = String(cssText).trim();
    if (text === '') {
      throw new TypeError('CSSStyleValue.parse: empty value for "' + String(property) + '"');
    }
    return cssValueFromString(text);
  };
  CSSStyleValue.parseAll = function(property, cssText) {
    var parts = splitTopLevelCommas(String(cssText));
    if (parts.length === 0) {
      throw new TypeError('CSSStyleValue.parseAll: empty value for "' + String(property) + '"');
    }
    return parts.map(cssValueFromString);
  };

  // ── StylePropertyMapReadOnly (§6.1) — element.computedStyleMap() ──────────────
  // The read half of both maps. Which declarations it reads is fixed by the
  // subclass prototype's `__computed__` flag, not by the caller: this class
  // reads the resolved cascade (the very snapshot `getComputedStyle` answers
  // from), `StylePropertyMap` below overrides the flag and reads the inline
  // `style=""` attribute instead.
  function StylePropertyMapReadOnly(nid) {
    this.__nid__ = nid;
  }
  StylePropertyMapReadOnly.prototype.__computed__ = true;

  // Resolved value of one property, or '' when this map has none.
  StylePropertyMapReadOnly.prototype.__lookup__ = function(prop) {
    var name = camelToKebab(String(prop));
    if (!this.__computed__) return _lumen_get_style_property(this.__nid__, name) || '';
    // Custom properties live in their own inherited snapshot (BUG-732), so the
    // computed map has to ask the same two bindings `getComputedStyle` does.
    if (name.slice(0, 2) === '--') return _lumen_get_custom_property(this.__nid__, name) || '';
    return _lumen_get_computed_style(this.__nid__, name) || '';
  };

  // All declarations of this map as [property, value] pairs, property-sorted.
  // A malformed payload means a broken native bridge, not an empty map — let
  // the JSON error surface rather than report "no declarations".
  StylePropertyMapReadOnly.prototype.__entries__ = function() {
    return JSON.parse(this.__computed__
      ? _lumen_get_computed_style_entries(this.__nid__)
      : _lumen_get_style_entries(this.__nid__));
  };

  StylePropertyMapReadOnly.prototype.get = function(prop) {
    var val = this.__lookup__(prop);
    return val === '' ? undefined : cssValueFromString(val);
  };
  StylePropertyMapReadOnly.prototype.getAll = function(prop) {
    var val = this.__lookup__(prop);
    return val === '' ? [] : [cssValueFromString(val)];
  };
  StylePropertyMapReadOnly.prototype.has = function(prop) {
    return this.__lookup__(prop) !== '';
  };
  Object.defineProperty(StylePropertyMapReadOnly.prototype, 'size', {
    get: function() { return this.__entries__().length; },
    configurable: true
  });
  // §6.1 is `iterable<USVString, sequence<CSSStyleValue>>`: every value is a
  // sequence, even for the single-valued properties Lumen stores.
  StylePropertyMapReadOnly.prototype.entries = function() {
    return this.__entries__().map(function(e) {
      return [e[0], [cssValueFromString(e[1])]];
    }).values();
  };
  StylePropertyMapReadOnly.prototype.keys = function() {
    return this.__entries__().map(function(e) { return e[0]; }).values();
  };
  StylePropertyMapReadOnly.prototype.values = function() {
    return this.__entries__().map(function(e) { return [cssValueFromString(e[1])]; }).values();
  };
  StylePropertyMapReadOnly.prototype.forEach = function(callback, thisArg) {
    if (typeof callback !== 'function') {
      throw new TypeError('StylePropertyMapReadOnly.forEach: callback is not a function');
    }
    var self = this;
    this.__entries__().forEach(function(e) {
      callback.call(thisArg, [cssValueFromString(e[1])], e[0], self);
    });
  };
  if (typeof Symbol !== 'undefined' && Symbol.iterator) {
    StylePropertyMapReadOnly.prototype[Symbol.iterator] = StylePropertyMapReadOnly.prototype.entries;
  }

  // ── StylePropertyMap (§6.2) — element.attributeStyleMap (mutable) ─────────────
  function StylePropertyMap(nid) {
    StylePropertyMapReadOnly.call(this, nid);
  }
  StylePropertyMap.prototype = Object.create(StylePropertyMapReadOnly.prototype);
  StylePropertyMap.prototype.constructor = StylePropertyMap;
  StylePropertyMap.prototype.__computed__ = false;
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

  // ── Export classes to global ──────────────────────────────────────────────────
  if (typeof global.CSS !== 'object') global.CSS = {};
  global.CSS.CSSStyleValue = CSSStyleValue;
  global.CSS.CSSUnitValue = CSSUnitValue;
  global.CSS.CSSKeywordValue = CSSKeywordValue;
  global.CSS.CSSNumericValue = CSSNumericValue;
  global.CSS.CSSMathValue = CSSMathValue;
  global.CSS.CSSMathSum = CSSMathSum;
  global.CSS.CSSMathProduct = CSSMathProduct;
  global.CSS.CSSMathNegate = CSSMathNegate;
  global.CSS.CSSMathInvert = CSSMathInvert;
  global.CSS.CSSMathMin = CSSMathMin;
  global.CSS.CSSMathMax = CSSMathMax;
  global.CSS.CSSUnparsedValue = CSSUnparsedValue;
  global.CSS.CSSVariableReferenceValue = CSSVariableReferenceValue;
  global.CSS.StylePropertyMap = StylePropertyMap;
  global.CSS.StylePropertyMapReadOnly = StylePropertyMapReadOnly;

  // §4.2's `CSS.px(1)` etc. factories — one CSSUnitValue constructor per unit
  // name, hung directly off the `CSS` namespace object above.
  var UNIT_FACTORY_NAMES = [
    'number', 'percent', 'em', 'ex', 'ch', 'ic', 'rem', 'lh', 'rlh',
    'vw', 'vh', 'vi', 'vb', 'vmin', 'vmax',
    'cm', 'mm', 'q', 'in', 'pt', 'pc', 'px', 'fr',
    'deg', 'grad', 'rad', 'turn', 's', 'ms', 'hz', 'khz',
    'dpi', 'dpcm', 'dppx', 'x'
  ];
  UNIT_FACTORY_NAMES.forEach(function(name) {
    global.CSS[name] = function(value) { return new CSSUnitValue(value, name); };
  });

  // ── Window/globalThis reference ───────────────────────────────────────────────
  if (typeof window === 'object' && window) {
    window.CSSStyleValue = CSSStyleValue;
    window.CSSUnitValue = CSSUnitValue;
    window.CSSKeywordValue = CSSKeywordValue;
    window.CSSNumericValue = CSSNumericValue;
    window.CSSMathValue = CSSMathValue;
    window.CSSMathSum = CSSMathSum;
    window.CSSMathProduct = CSSMathProduct;
    window.CSSMathNegate = CSSMathNegate;
    window.CSSMathInvert = CSSMathInvert;
    window.CSSMathMin = CSSMathMin;
    window.CSSMathMax = CSSMathMax;
    window.CSSUnparsedValue = CSSUnparsedValue;
    window.CSSVariableReferenceValue = CSSVariableReferenceValue;
    window.StylePropertyMap = StylePropertyMap;
    window.StylePropertyMapReadOnly = StylePropertyMapReadOnly;
  }
})(typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : typeof window !== 'undefined' ? window : this);
"#;
