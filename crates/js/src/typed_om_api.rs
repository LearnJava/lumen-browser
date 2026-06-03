//! CSS Typed OM (CSS Typed Object Model L1)
//! Implements element.attributeStyleMap, element.computedStyleMap(),
//! CSSStyleValue, CSSUnitValue, CSSKeywordValue, CSSNumericValue.to().

use rquickjs::Ctx;

/// Install CSS Typed OM bindings: attributeStyleMap, computedStyleMap, CSSStyleValue classes.
pub fn install_typed_om_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(TYPED_OM_SHIM)?;
    Ok(())
}

/// Pure-JS CSS Typed OM shim.
/// Defines Element.prototype.attributeStyleMap, Element.prototype.computedStyleMap(),
/// CSSStyleValue, CSSKeywordValue, CSSUnitValue, CSSNumericValue.to().
const TYPED_OM_SHIM: &str = r#"
'use strict';

// Base CSSStyleValue class — exposed in global scope
class CSSStyleValue {
  constructor() {}
}

// CSSKeywordValue: represents keyword values (e.g., 'auto', 'bold')
class CSSKeywordValue extends CSSStyleValue {
  constructor(value) {
    super();
    this.value = String(value);
  }
  toString() {
    return this.value;
  }
}

// CSSNumericValue: base for numeric values (px, em, %, etc.)
class CSSNumericValue extends CSSStyleValue {
  constructor(value, unit) {
    super();
    this.value = Number(value);
    this.unit = String(unit);
  }

  // Convert to another unit (simplified: only supports same unit)
  to(unit) {
    const u = String(unit).toLowerCase();
    if (u === this.unit.toLowerCase()) {
      return new CSSUnitValue(this.value, this.unit);
    }
    // Throw error on unsupported conversions
    throw new TypeError(`Cannot convert ${this.unit} to ${unit}`);
  }

  toString() {
    return this.value + this.unit;
  }
}

// CSSUnitValue: numeric value with unit (px, em, %, etc.)
class CSSUnitValue extends CSSNumericValue {
  constructor(value, unit) {
    super(value, unit);
  }
}

// AttributeStyleMap: wrapper for element.style
class AttributeStyleMap {
  constructor(element) {
    this._element = element;
  }

  get(property) {
    const val = this._element.style.getPropertyValue(property);
    if (!val) return undefined;
    return this._parseStyleValue(val);
  }

  set(property, value) {
    const cssText = this._toCssText(value);
    this._element.style.setProperty(property, cssText);
  }

  has(property) {
    return this._element.style.getPropertyValue(property) !== '';
  }

  delete(property) {
    this._element.style.removeProperty(property);
  }

  entries() {
    const result = [];
    const style = this._element.style;
    for (let i = 0; i < style.length; i++) {
      const prop = style[i];
      const val = style.getPropertyValue(prop);
      result.push([prop, this._parseStyleValue(val)]);
    }
    return result[Symbol.iterator]();
  }

  keys() {
    const result = [];
    const style = this._element.style;
    for (let i = 0; i < style.length; i++) {
      result.push(style[i]);
    }
    return result[Symbol.iterator]();
  }

  values() {
    const result = [];
    const style = this._element.style;
    for (let i = 0; i < style.length; i++) {
      const val = style.getPropertyValue(style[i]);
      result.push(this._parseStyleValue(val));
    }
    return result[Symbol.iterator]();
  }

  _parseStyleValue(cssText) {
    const match = String(cssText).match(/^([-+]?[\d.]+)(px|em|rem|%|vh|vw|ch|ex|vmin|vmax|cm|mm|in|pt|pc|q)?$/i);
    if (match) {
      const value = parseFloat(match[1]);
      const unit = match[2] || 'px';
      return new CSSUnitValue(value, unit);
    }
    return new CSSKeywordValue(cssText);
  }

  _toCssText(value) {
    if (typeof value === 'string') return value;
    if (value instanceof CSSUnitValue) {
      return value.value + value.unit;
    }
    if (value instanceof CSSKeywordValue) {
      return value.value;
    }
    return String(value);
  }
}

// ComputedStyleMap: read-only wrapper for getComputedStyle
class ComputedStyleMap {
  constructor(element) {
    this._element = element;
    this._computed = null;
  }

  get(property) {
    if (!this._computed) {
      this._computed = getComputedStyle(this._element);
    }
    const val = this._computed.getPropertyValue(property);
    if (!val) return undefined;
    return this._parseComputedValue(val);
  }

  has(property) {
    if (!this._computed) {
      this._computed = getComputedStyle(this._element);
    }
    return this._computed.getPropertyValue(property) !== '';
  }

  entries() {
    if (!this._computed) {
      this._computed = getComputedStyle(this._element);
    }
    const result = [];
    for (let i = 0; i < this._computed.length; i++) {
      const prop = this._computed[i];
      const val = this._computed.getPropertyValue(prop);
      result.push([prop, this._parseComputedValue(val)]);
    }
    return result[Symbol.iterator]();
  }

  keys() {
    if (!this._computed) {
      this._computed = getComputedStyle(this._element);
    }
    const result = [];
    for (let i = 0; i < this._computed.length; i++) {
      result.push(this._computed[i]);
    }
    return result[Symbol.iterator]();
  }

  values() {
    if (!this._computed) {
      this._computed = getComputedStyle(this._element);
    }
    const result = [];
    for (let i = 0; i < this._computed.length; i++) {
      const val = this._computed.getPropertyValue(this._computed[i]);
      result.push(this._parseComputedValue(val));
    }
    return result[Symbol.iterator]();
  }

  _parseComputedValue(cssText) {
    const match = String(cssText).match(/^([-+]?[\d.]+)(px|em|rem|%|vh|vw|ch|ex|vmin|vmax|cm|mm|in|pt|pc|q)?$/i);
    if (match) {
      const value = parseFloat(match[1]);
      const unit = match[2] || 'px';
      return new CSSUnitValue(value, unit);
    }
    return new CSSKeywordValue(cssText);
  }
}

// Install on Element.prototype (only if Element is available — not in headless contexts)
if (typeof Element !== 'undefined') {
  if (!Element.prototype.attributeStyleMap) {
    Object.defineProperty(Element.prototype, 'attributeStyleMap', {
      get: function() {
        if (!this._typedOmAttributeStyleMap) {
          this._typedOmAttributeStyleMap = new AttributeStyleMap(this);
        }
        return this._typedOmAttributeStyleMap;
      },
      configurable: true
    });
  }

  if (!Element.prototype.computedStyleMap) {
    Element.prototype.computedStyleMap = function() {
      return new ComputedStyleMap(this);
    };
  }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    #[test]
    fn test_css_unit_value_creation() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: String = ctx.eval(
                r#"
                const val = new CSSUnitValue(10, 'px');
                val.toString();
                "#,
            ).unwrap();
            assert_eq!(result, "10px");
        });
    }

    #[test]
    fn test_css_keyword_value_creation() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: String = ctx.eval(
                r#"
                const val = new CSSKeywordValue('auto');
                val.toString();
                "#,
            ).unwrap();
            assert_eq!(result, "auto");
        });
    }

    #[test]
    fn test_css_unit_value_value_and_unit() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: f64 = ctx.eval("new CSSUnitValue(10, 'px').value").unwrap();
            assert_eq!(result, 10.0);
            let unit: String = ctx.eval("new CSSUnitValue(10, 'px').unit").unwrap();
            assert_eq!(unit, "px");
        });
    }

    #[test]
    fn test_css_keyword_value_value() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: String = ctx.eval("new CSSKeywordValue('auto').value").unwrap();
            assert_eq!(result, "auto");
        });
    }

    #[test]
    fn test_css_numeric_value_to_same_unit() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: String = ctx.eval(
                r#"
                const val = new CSSUnitValue(10, 'px');
                const val2 = val.to('px');
                val2.toString();
                "#,
            ).unwrap();
            assert_eq!(result, "10px");
        });
    }

    #[test]
    fn test_css_numeric_value_to_different_unit_throws() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                try {
                  const val = new CSSUnitValue(10, 'px');
                  val.to('em');
                  false;
                } catch (e) {
                  true;
                }
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_css_style_value_inheritance() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const uval = new CSSUnitValue(10, 'px');
                const kval = new CSSKeywordValue('auto');
                uval instanceof CSSStyleValue && kval instanceof CSSStyleValue;
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_css_unit_value_parsing_px() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const val = new CSSUnitValue(10, 'px');
                val.value === 10 && val.unit === 'px';
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_css_unit_value_parsing_em() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const val = new CSSUnitValue(2.5, 'em');
                val.value === 2.5 && val.unit === 'em';
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_css_unit_value_parsing_percent() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const val = new CSSUnitValue(50, '%');
                val.value === 50 && val.unit === '%';
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_css_unit_value_parsing_negative() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const val = new CSSUnitValue(-10, 'px');
                val.value === -10 && val.unit === 'px';
                "#,
            ).unwrap();
            assert!(result);
        });
    }
}
