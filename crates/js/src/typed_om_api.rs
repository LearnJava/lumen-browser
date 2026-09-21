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
//! - `CSSTransformValue` and the `CSSTransformComponent` family — `CSSTranslate`/
//!   `CSSRotate`/`CSSScale`/`CSSSkew`/`CSSSkewX`/`CSSSkewY`/`CSSPerspective`/
//!   `CSSMatrixComponent`; `toMatrix()` builds a `DOMMatrix` (from
//!   `geometry_shim.js`), resolving lengths/angles via `CSSUnitValue.to()` —
//!   no layout context, so a percentage or relative unit throws instead of
//!   guessing a used value
//!
//! Not implemented: `CSSColorValue` family (GAP-TYPEDOM remainder).
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

  // ── CSSTransformComponent hierarchy (§11) — individual transform functions ────
  // Spec keeps this family separate from CSSStyleValue (it is `toMatrix()` +
  // `is2D` + a stringifier, not a value that reads/writes a property), so
  // unlike everything above it these do not extend CSSStyleValue. `toMatrix()`
  // needs the DOMMatrix from geometry_shim.js, which is evaluated earlier in
  // the same combined shim string (dom.rs's DOM_SHIM), so it is already a
  // global by the time this file runs.
  function requireNumericValue(v, what) {
    if (!(v instanceof CSSNumericValue)) {
      throw new TypeError('CSSTransformComponent: ' + what + ' must be a CSSNumericValue');
    }
    return v;
  }
  // Length/angle resolution has no layout context here (no element, no
  // viewport) — only unit-to-unit conversions `CSSUnitValue.to()` already
  // knows (the UNIT_GROUPS table above) are honoured; a percentage or an
  // `em` throws, same as `to()` does, instead of inventing a used value.
  function numericToPx(nv) {
    if (!(nv instanceof CSSUnitValue)) {
      throw new TypeError('CSSTransformComponent: cannot resolve to a length without a used value');
    }
    if (nv.unit === 'number' && nv.value === 0) return 0;
    return nv.to('px').value;
  }
  function numericToDeg(nv) {
    if (!(nv instanceof CSSUnitValue)) {
      throw new TypeError('CSSTransformComponent: cannot resolve to an angle without a used value');
    }
    return nv.to('deg').value;
  }
  function numericToFactor(v) {
    var nv = toNumericValue(v);
    if (!(nv instanceof CSSUnitValue) || nv.unit !== 'number') {
      throw new TypeError('CSSTransformComponent: cannot resolve to a number without a used value');
    }
    return nv.value;
  }

  function CSSTransformComponent() {}
  CSSTransformComponent.prototype.toString = function() { return this.cssText; };
  Object.defineProperty(CSSTransformComponent.prototype, 'is2D', {
    get: function() { return this._is2D; },
    configurable: true
  });

  function CSSTranslate(x, y, z) {
    CSSTransformComponent.call(this);
    this.x = requireNumericValue(x, 'x');
    this.y = requireNumericValue(y, 'y');
    if (z === undefined) {
      this.z = new CSSUnitValue(0, 'px');
      this._is2D = true;
    } else {
      this.z = requireNumericValue(z, 'z');
      this._is2D = false;
    }
  }
  CSSTranslate.prototype = Object.create(CSSTransformComponent.prototype);
  CSSTranslate.prototype.constructor = CSSTranslate;
  Object.defineProperty(CSSTranslate.prototype, 'cssText', {
    get: function() {
      return this._is2D
        ? 'translate(' + this.x + ', ' + this.y + ')'
        : 'translate3d(' + this.x + ', ' + this.y + ', ' + this.z + ')';
    },
    configurable: true
  });
  CSSTranslate.prototype.toMatrix = function() {
    return new DOMMatrix().translate(numericToPx(this.x), numericToPx(this.y), numericToPx(this.z));
  };

  function CSSRotate() {
    CSSTransformComponent.call(this);
    if (arguments.length === 1) {
      this.x = 0; this.y = 0; this.z = 1;
      this.angle = requireNumericValue(arguments[0], 'angle');
      this._is2D = true;
    } else if (arguments.length === 4) {
      this.x = Number(arguments[0]);
      this.y = Number(arguments[1]);
      this.z = Number(arguments[2]);
      this.angle = requireNumericValue(arguments[3], 'angle');
      this._is2D = false;
    } else {
      throw new TypeError('CSSRotate: expected 1 or 4 arguments');
    }
  }
  CSSRotate.prototype = Object.create(CSSTransformComponent.prototype);
  CSSRotate.prototype.constructor = CSSRotate;
  Object.defineProperty(CSSRotate.prototype, 'cssText', {
    get: function() {
      return this._is2D
        ? 'rotate(' + this.angle + ')'
        : 'rotate3d(' + this.x + ', ' + this.y + ', ' + this.z + ', ' + this.angle + ')';
    },
    configurable: true
  });
  CSSRotate.prototype.toMatrix = function() {
    var deg = numericToDeg(this.angle);
    return this._is2D
      ? new DOMMatrix().rotate(deg)
      : new DOMMatrix().rotateAxisAngle(this.x, this.y, this.z, deg);
  };

  function CSSScale(x, y, z) {
    CSSTransformComponent.call(this);
    this.x = toNumericValue(x);
    this.y = toNumericValue(y);
    if (z === undefined) {
      this.z = new CSSUnitValue(1, 'number');
      this._is2D = true;
    } else {
      this.z = toNumericValue(z);
      this._is2D = false;
    }
  }
  CSSScale.prototype = Object.create(CSSTransformComponent.prototype);
  CSSScale.prototype.constructor = CSSScale;
  Object.defineProperty(CSSScale.prototype, 'cssText', {
    get: function() {
      return this._is2D
        ? 'scale(' + this.x + ', ' + this.y + ')'
        : 'scale3d(' + this.x + ', ' + this.y + ', ' + this.z + ')';
    },
    configurable: true
  });
  CSSScale.prototype.toMatrix = function() {
    return new DOMMatrix().scale(numericToFactor(this.x), numericToFactor(this.y), numericToFactor(this.z));
  };

  function CSSSkew(ax, ay) {
    CSSTransformComponent.call(this);
    this.ax = requireNumericValue(ax, 'ax');
    this.ay = requireNumericValue(ay, 'ay');
    this._is2D = true;
  }
  CSSSkew.prototype = Object.create(CSSTransformComponent.prototype);
  CSSSkew.prototype.constructor = CSSSkew;
  Object.defineProperty(CSSSkew.prototype, 'cssText', {
    get: function() { return 'skew(' + this.ax + ', ' + this.ay + ')'; },
    configurable: true
  });
  // Matches the `skew(ax, ay)` case of `_dm_func_to_matrix` in geometry_shim.js
  // exactly (skewX's matrix, then skewY's, in that multiplication order) —
  // chaining the public instance methods from identity reaches the same result.
  CSSSkew.prototype.toMatrix = function() {
    return new DOMMatrix().skewX(numericToDeg(this.ax)).skewY(numericToDeg(this.ay));
  };

  function CSSSkewX(ax) {
    CSSTransformComponent.call(this);
    this.ax = requireNumericValue(ax, 'ax');
    this._is2D = true;
  }
  CSSSkewX.prototype = Object.create(CSSTransformComponent.prototype);
  CSSSkewX.prototype.constructor = CSSSkewX;
  Object.defineProperty(CSSSkewX.prototype, 'cssText', {
    get: function() { return 'skewX(' + this.ax + ')'; },
    configurable: true
  });
  CSSSkewX.prototype.toMatrix = function() { return new DOMMatrix().skewX(numericToDeg(this.ax)); };

  function CSSSkewY(ay) {
    CSSTransformComponent.call(this);
    this.ay = requireNumericValue(ay, 'ay');
    this._is2D = true;
  }
  CSSSkewY.prototype = Object.create(CSSTransformComponent.prototype);
  CSSSkewY.prototype.constructor = CSSSkewY;
  Object.defineProperty(CSSSkewY.prototype, 'cssText', {
    get: function() { return 'skewY(' + this.ay + ')'; },
    configurable: true
  });
  CSSSkewY.prototype.toMatrix = function() { return new DOMMatrix().skewY(numericToDeg(this.ay)); };

  function CSSPerspective(length) {
    CSSTransformComponent.call(this);
    var isNoneKeyword = length instanceof CSSKeywordValue && length.value === 'none';
    if (!(length instanceof CSSNumericValue) && !isNoneKeyword) {
      throw new TypeError('CSSPerspective: length must be a CSSNumericValue or the keyword "none"');
    }
    this.length = length;
    this._is2D = false;
  }
  CSSPerspective.prototype = Object.create(CSSTransformComponent.prototype);
  CSSPerspective.prototype.constructor = CSSPerspective;
  Object.defineProperty(CSSPerspective.prototype, 'cssText', {
    get: function() { return 'perspective(' + this.length + ')'; },
    configurable: true
  });
  // Same `m[11] = -1 / d` construction as the private `_dm_perspective()` in
  // geometry_shim.js (not exported, so rebuilt here from the public 16-value
  // DOMMatrix constructor form instead of calling it directly).
  CSSPerspective.prototype.toMatrix = function() {
    var m = [1, 0, 0, 0,  0, 1, 0, 0,  0, 0, 1, 0,  0, 0, 0, 1];
    if (this.length instanceof CSSNumericValue) {
      var d = numericToPx(this.length);
      if (d !== 0) { m[11] = -1 / d; }
    }
    return new DOMMatrix(m);
  };

  function CSSMatrixComponent(matrix, options) {
    CSSTransformComponent.call(this);
    var m = (matrix instanceof DOMMatrixReadOnly) ? matrix : new DOMMatrix(matrix);
    this.matrix = new DOMMatrix(m);
    this._is2D = (options && options.is2D !== undefined) ? !!options.is2D : m.is2D;
  }
  CSSMatrixComponent.prototype = Object.create(CSSTransformComponent.prototype);
  CSSMatrixComponent.prototype.constructor = CSSMatrixComponent;
  Object.defineProperty(CSSMatrixComponent.prototype, 'cssText', {
    get: function() { return this.matrix.toString(); },
    configurable: true
  });
  CSSMatrixComponent.prototype.toMatrix = function() { return new DOMMatrix(this.matrix); };

  // ── CSSTransformValue (§11.1) — element.style's transform as a component list ─
  function CSSTransformValue(transforms) {
    // Not `CSSStyleValue.call(this)` — that assigns `this.cssText` as an own
    // field, which throws in strict mode against the getter-only `cssText`
    // accessor this prototype defines below (same pattern as
    // CSSUnparsedValue above).
    var list = Array.prototype.slice.call(transforms || []);
    if (list.length === 0) {
      throw new TypeError('CSSTransformValue: transforms must not be empty');
    }
    for (var i = 0; i < list.length; i++) {
      if (!(list[i] instanceof CSSTransformComponent)) {
        throw new TypeError('CSSTransformValue: item ' + i + ' is not a CSSTransformComponent');
      }
      this[i] = list[i];
    }
    this.length = list.length;
  }
  CSSTransformValue.prototype = Object.create(CSSStyleValue.prototype);
  CSSTransformValue.prototype.constructor = CSSTransformValue;
  Object.defineProperty(CSSTransformValue.prototype, 'is2D', {
    get: function() {
      for (var i = 0; i < this.length; i++) {
        if (!this[i].is2D) return false;
      }
      return true;
    },
    configurable: true
  });
  Object.defineProperty(CSSTransformValue.prototype, 'cssText', {
    get: function() {
      var parts = [];
      for (var i = 0; i < this.length; i++) { parts.push(this[i].toString()); }
      return parts.join(' ');
    },
    configurable: true
  });
  // Same reduction `_dm_parse_transform_string` uses for a `<transform-list>`
  // string (`acc = component × acc`, accumulated left to right) — so a
  // CSSTransformValue built from the same functions as a transform string
  // reaches the identical DOMMatrix.
  CSSTransformValue.prototype.toMatrix = function() {
    var acc = new DOMMatrix();
    for (var i = 0; i < this.length; i++) {
      acc = this[i].toMatrix().multiply(acc);
    }
    return acc;
  };
  if (typeof Symbol !== 'undefined' && Symbol.iterator) {
    CSSTransformValue.prototype[Symbol.iterator] = function() {
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

  // §6.2's set()/append() are variadic — `styleMap.set('background-position',
  // CSS.px(1), CSS.px(2))` — but without a per-property grammar table Lumen
  // cannot know each property's separator. This hardcodes the properties WPT
  // and real pages actually exercise as comma-separated layer lists; every
  // other property falls back to space-joining, which matches shorthands like
  // `margin`/`background-position` and is a no-op for the single-value case.
  var COMMA_LIST_PROPERTIES = {
    'background': 1, 'background-image': 1, 'background-position': 1, 'background-size': 1,
    'background-repeat': 1, 'mask': 1, 'mask-image': 1,
    'transition': 1, 'transition-property': 1, 'transition-duration': 1,
    'transition-timing-function': 1, 'transition-delay': 1,
    'animation': 1, 'animation-name': 1, 'animation-duration': 1, 'animation-timing-function': 1,
    'animation-delay': 1, 'animation-iteration-count': 1, 'animation-direction': 1,
    'animation-fill-mode': 1, 'animation-play-state': 1,
    'will-change': 1, 'font-family': 1, 'grid-template-columns': 1, 'grid-template-rows': 1
  };

  function serialiseOneValue(value) {
    if (value instanceof CSSStyleValue) return value.cssText;
    if (value && typeof value === 'object' && value.cssText !== undefined) return value.cssText;
    return String(value);
  }

  StylePropertyMap.prototype.set = function(prop, value) {
    var name = camelToKebab(String(prop));
    var rest = Array.prototype.slice.call(arguments, 1).map(serialiseOneValue);
    if (rest.length === 0) {
      throw new TypeError('StylePropertyMap.set: at least one value is required');
    }
    var sep = Object.prototype.hasOwnProperty.call(COMMA_LIST_PROPERTIES, name) ? ', ' : ' ';
    _lumen_set_style_property(this.__nid__, name, rest.join(sep));
  };
  // §6.2 `append()` — adds a layer to a property whose grammar is a
  // comma-separated list (background-image, transition, ...) instead of
  // replacing the whole value the way `set()` does. Spec throws
  // `NotSupportedError` for a property that has no such list grammar; there is
  // no DOMException binding in this shim, so a TypeError carries the same
  // "you can't do that" signal.
  StylePropertyMap.prototype.append = function(prop) {
    var name = camelToKebab(String(prop));
    if (!Object.prototype.hasOwnProperty.call(COMMA_LIST_PROPERTIES, name)) {
      throw new TypeError('StylePropertyMap.append: "' + name + '" does not support multiple values');
    }
    var added = Array.prototype.slice.call(arguments, 1).map(serialiseOneValue);
    if (added.length === 0) {
      throw new TypeError('StylePropertyMap.append: at least one value is required');
    }
    var existing = this.__lookup__(name);
    var layers = existing === '' ? [] : splitTopLevelCommas(existing);
    _lumen_set_style_property(this.__nid__, name, layers.concat(added).join(', '));
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
  global.CSS.CSSTransformComponent = CSSTransformComponent;
  global.CSS.CSSTransformValue = CSSTransformValue;
  global.CSS.CSSTranslate = CSSTranslate;
  global.CSS.CSSRotate = CSSRotate;
  global.CSS.CSSScale = CSSScale;
  global.CSS.CSSSkew = CSSSkew;
  global.CSS.CSSSkewX = CSSSkewX;
  global.CSS.CSSSkewY = CSSSkewY;
  global.CSS.CSSPerspective = CSSPerspective;
  global.CSS.CSSMatrixComponent = CSSMatrixComponent;
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
    window.CSSTransformComponent = CSSTransformComponent;
    window.CSSTransformValue = CSSTransformValue;
    window.CSSTranslate = CSSTranslate;
    window.CSSRotate = CSSRotate;
    window.CSSScale = CSSScale;
    window.CSSSkew = CSSSkew;
    window.CSSSkewX = CSSSkewX;
    window.CSSSkewY = CSSSkewY;
    window.CSSPerspective = CSSPerspective;
    window.CSSMatrixComponent = CSSMatrixComponent;
    window.StylePropertyMap = StylePropertyMap;
    window.StylePropertyMapReadOnly = StylePropertyMapReadOnly;
  }
})(typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : typeof window !== 'undefined' ? window : this);
"#;
