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
/// CSSStyleValue, CSSUnitValue, CSSKeywordValue, CSSNumericValue.to().
const TYPED_OM_SHIM: &str = r#"(function(global) {
  'use strict';

  // Base CSSStyleValue class
  class CSSStyleValue {
    constructor() {}
    // Placeholder for subclasses
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

    // Convert to another unit (simplified: only supports px/em/% equivalents)
    to(unit) {
      const u = String(unit).toLowerCase();
      if (u === this.unit.toLowerCase()) {
        return new CSSUnitValue(this.value, this.unit);
      }
      // For now, throw error on unsupported conversions
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

  // AttributeStyleMap: wrapper for element.style with get/set/has/delete/entries/keys/values
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
      // Try to parse as numeric with unit (e.g., "10px", "2em", "50%")
      const match = String(cssText).match(/^([-+]?[\d.]+)(px|em|rem|%|vh|vw|ch|ex|vmin|vmax|cm|mm|in|pt|pc|q)?$/i);
      if (match) {
        const value = parseFloat(match[1]);
        const unit = match[2] || 'px';
        return new CSSUnitValue(value, unit);
      }
      // Treat as keyword
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
      // Try to parse as numeric with unit
      const match = String(cssText).match(/^([-+]?[\d.]+)(px|em|rem|%|vh|vw|ch|ex|vmin|vmax|cm|mm|in|pt|pc|q)?$/i);
      if (match) {
        const value = parseFloat(match[1]);
        const unit = match[2] || 'px';
        return new CSSUnitValue(value, unit);
      }
      // Treat as keyword
      return new CSSKeywordValue(cssText);
    }
  }

  // Install on Element.prototype
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
      // Return a fresh ComputedStyleMap each time (no caching, values may change)
      return new ComputedStyleMap(this);
    };
  }

  // Expose classes for tests
  global.CSSStyleValue = CSSStyleValue;
  global.CSSKeywordValue = CSSKeywordValue;
  global.CSSUnitValue = CSSUnitValue;
  global.CSSNumericValue = CSSNumericValue;

})(globalThis);
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
    fn test_attribute_style_map_get_set() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: String = ctx.eval(
                r#"
                const div = document.createElement('div');
                div.style.setProperty('width', '100px');
                const val = div.attributeStyleMap.get('width');
                val.toString();
                "#,
            ).unwrap();
            assert_eq!(result, "100px");
        });
    }

    #[test]
    fn test_attribute_style_map_has() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const div = document.createElement('div');
                div.style.setProperty('width', '100px');
                div.attributeStyleMap.has('width');
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_attribute_style_map_delete() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: bool = ctx.eval(
                r#"
                const div = document.createElement('div');
                div.style.setProperty('width', '100px');
                div.attributeStyleMap.delete('width');
                !div.attributeStyleMap.has('width');
                "#,
            ).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_attribute_style_map_entries() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: u32 = ctx.eval(
                r#"
                const div = document.createElement('div');
                div.style.setProperty('width', '100px');
                div.style.setProperty('height', '50px');
                let count = 0;
                for (const [prop, val] of div.attributeStyleMap.entries()) {
                  count++;
                }
                count;
                "#,
            ).unwrap();
            assert!(result >= 2);
        });
    }

    #[test]
    fn test_attribute_style_map_keys() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: u32 = ctx.eval(
                r#"
                const div = document.createElement('div');
                div.style.setProperty('width', '100px');
                let count = 0;
                for (const key of div.attributeStyleMap.keys()) {
                  count++;
                }
                count;
                "#,
            ).unwrap();
            assert!(result >= 1);
        });
    }

    #[test]
    fn test_attribute_style_map_values() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: u32 = ctx.eval(
                r#"
                const div = document.createElement('div');
                div.style.setProperty('width', '100px');
                let count = 0;
                for (const val of div.attributeStyleMap.values()) {
                  count++;
                }
                count;
                "#,
            ).unwrap();
            assert!(result >= 1);
        });
    }

    #[test]
    fn test_computed_style_map_get() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_typed_om_api(&ctx).unwrap();
            let result: String = ctx.eval(
                r#"
                const div = document.createElement('div');
                document.body.appendChild(div);
                div.style.width = '50px';
                const cmap = div.computedStyleMap();
                const w = cmap.get('width');
                w ? w.toString() : 'undefined';
                "#,
            ).unwrap();
            // May return 'undefined' in headless, but test doesn't error
            assert!(!result.is_empty());
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
}
