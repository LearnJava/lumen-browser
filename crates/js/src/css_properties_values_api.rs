//! CSS Properties & Values API (Houdini) — custom property registration
//! Implements CSS.registerProperty(), @property at-rule parsing, and initial-value fallback in compute_style.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Global registry of custom property definitions (keyed by property name).
/// This is shared across all stylesheets and reachable via CSS.registerProperty().
static REGISTERED_PROPERTIES: OnceLock<Mutex<RegisteredPropertiesMap>> = OnceLock::new();

/// Maps property name (e.g. "--my-color") to its definition.
#[derive(Clone, Debug, Default)]
pub struct RegisteredPropertiesMap {
    properties: HashMap<String, RegisteredProperty>,
}

impl RegisteredPropertiesMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a custom property definition.
    pub fn register(&mut self, name: String, def: RegisteredProperty) {
        self.properties.insert(name, def);
    }

    /// Look up a registered property by name.
    pub fn get(&self, name: &str) -> Option<RegisteredProperty> {
        self.properties.get(name).cloned()
    }

    /// Get all registered properties.
    pub fn all(&self) -> Vec<(String, RegisteredProperty)> {
        self.properties.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Clear all registrations (for tests).
    pub fn clear(&mut self) {
        self.properties.clear();
    }
}

/// Get the global registered properties registry, initializing it if necessary.
pub fn get_registered_properties() -> &'static Mutex<RegisteredPropertiesMap> {
    REGISTERED_PROPERTIES.get_or_init(|| Mutex::new(RegisteredPropertiesMap::new()))
}

/// Definition of a custom CSS property.
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredProperty {
    /// Property name (including "--" prefix).
    pub name: String,
    /// Syntax descriptor (e.g. "<color>", "<length>", "*" for any).
    pub syntax: String,
    /// Whether the property inherits (default: true).
    pub inherits: bool,
    /// Initial value (used as fallback).
    pub initial_value: String,
}

/// V8 port of the former rquickjs `install_css_properties_values_api` (Ph3 V8
/// migration S5-S7, rquickjs side removed in S12b-B4): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
/// Uses a pure JS shim that stores properties in a global Map.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_css_properties_values_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn3;
    use lumen_core::ext::JsRuntime as _;
    // BUG-531: `CSS.registerProperty()` used to store `syntax`/`initialValue`
    // verbatim with no grammar check at all — no `SyntaxError` was ever
    // thrown for a malformed `syntax` descriptor or a mismatched
    // `initialValue`. This native backs the shim's validation step;
    // `has_initial_value` distinguishes "not given" (fine only for the
    // universal `'*'` syntax) from an empty string (a real, checked value).
    rt.register_native(
        "_lumen_validate_registered_property",
        into_v8_fn3(|syntax: String, has_initial_value: bool, initial_value: String| -> Option<String> {
            let initial = has_initial_value.then_some(initial_value.as_str());
            lumen_layout::style::validate_registered_property(&syntax, initial).err()
        }),
    )?;
    // CSSOM-10: the same check decides whether an `@property` rule is valid enough
    // to appear in `cssRules` (the CSSOM tree lives below layout and cannot call it).
    lumen_css_parser::cssom_tree::set_property_rule_validator(|syntax, initial| {
        lumen_layout::style::validate_registered_property(syntax, initial).is_ok()
    });
    rt.eval(CSS_PROPERTIES_VALUES_SHIM)?;
    Ok(())
}

/// Pure-JS CSS Properties & Values API shim.
/// Defines CSS.registerProperty() and provides interface to Rust-backed registry.
#[cfg(feature = "v8-backend")]
const CSS_PROPERTIES_VALUES_SHIM: &str = r#"(function(global) {
  'use strict';

  // Store registered properties in a global map accessible from Rust bindings.
  if (!__lumen_C._lumen_registered_properties) {
    __lumen_C._lumen_registered_properties = new Map();
  }

  // Create or extend CSS global object.
  if (!global.CSS) {
    global.CSS = {};
  }

  // CSS.registerProperty(definition)
  // definition: { name, syntax?, inherits?, initialValue? }
  global.CSS.registerProperty = function(definition) {
    if (!definition || typeof definition !== 'object') {
      throw new TypeError('registerProperty requires an object argument');
    }

    // WebIDL dictionary conversion: `name` and `inherits` are `required`, and a `DOMString`
    // member is stringified rather than type-checked (`['--name', 3]` is `'--name,3'`).
    if (definition.name === undefined) {
      throw new TypeError("registerProperty: required member 'name' is undefined");
    }
    if (definition.inherits === undefined) {
      throw new TypeError("registerProperty: required member 'inherits' is undefined");
    }
    const name = String(definition.name);
    const inherits = Boolean(definition.inherits);

    if (!name.startsWith('--') || name === '--') {
      throw new DOMException(`CustomPropertyName: '${name}' must start with '--'`, 'SyntaxError');
    }

    // Extract optional fields with defaults. `syntax`/`initialValue` are
    // DOMString descriptors: only an actually-*missing* member falls back to
    // its default — an explicit `null`/array/etc. is stringified instead
    // (`String(null)` === 'null', matching WebIDL's DOMString conversion),
    // so `registerProperty({syntax: null, ...})` registers the literal
    // one-character syntax "null", not the universal one.
    const hasSyntax = definition.syntax !== undefined;
    const syntax = hasSyntax ? String(definition.syntax) : '*';
    const hasInitialValue = definition.initialValue !== undefined;
    const initialValue = hasInitialValue ? String(definition.initialValue) : '';

    // CSS Properties and Values API §3.3.2 step 2: a name registered by a script
    // before is an error, whatever the new descriptors are.
    if (__lumen_C._lumen_registered_properties.has(name)) {
      throw new DOMException(`The property '${name}' has already been registered.`, 'InvalidModificationError');
    }

    // BUG-531: validate `syntax`/`initialValue` against the CSS Properties
    // and Values API grammar before ever storing them — previously nothing
    // here could throw, so a malformed `syntax` or a mismatched
    // `initialValue` was silently accepted.
    if (typeof _lumen_validate_registered_property === 'function') {
      const err = _lumen_validate_registered_property(syntax, hasInitialValue, initialValue);
      if (err !== null && err !== undefined) {
        throw new DOMException(err, 'SyntaxError');
      }
    }

    // BUG-1505: the cascade reads the registration from the document, not from this Map.
    if (typeof _lumen_register_css_property === 'function') {
      _lumen_register_css_property(name, syntax, inherits, initialValue, hasInitialValue);
    }

    // Store definition.
    __lumen_C._lumen_registered_properties.set(name, {
      name,
      syntax,
      inherits,
      initialValue
    });
  };

  // CSS.unregisterProperty(name): removes a registration made by registerProperty().
  global.CSS.unregisterProperty = function(name) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'unregisterProperty' on 'CSS': 1 argument required, but only 0 present.");
    }
    name = String(name);
    if (!__lumen_C._lumen_registered_properties.has(name)) {
      throw new DOMException(`The property '${name}' has not been registered.`, 'NotFoundError');
    }
    __lumen_C._lumen_registered_properties.delete(name);
    if (typeof _lumen_unregister_css_property === 'function') {
      _lumen_unregister_css_property(name);
    }
  };

  // Convenience method to retrieve all registered properties.
  // (Used for testing and StyleSheet.registered_properties access.)
  global.CSS._getRegisteredProperties = function() {
    const result = {};
    __lumen_C._lumen_registered_properties.forEach((def, name) => {
      result[name] = def;
    });
    return result;
  };

  // Export CSS object to window if not already present.
  if (typeof window !== 'undefined' && window) {
    window.CSS = global.CSS;
  }
})(typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : typeof window !== 'undefined' ? window : this);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_property_creates_entry() {
        let mut map = RegisteredPropertiesMap::new();
        let prop = RegisteredProperty {
            name: "--my-color".to_string(),
            syntax: "<color>".to_string(),
            inherits: true,
            initial_value: "blue".to_string(),
        };
        map.register("--my-color".to_string(), prop.clone());

        let retrieved = map.get("--my-color").expect("property should exist");
        assert_eq!(retrieved.name, "--my-color");
        assert_eq!(retrieved.syntax, "<color>");
        assert!(retrieved.inherits);
        assert_eq!(retrieved.initial_value, "blue");
    }

    #[test]
    fn test_register_property_get_missing() {
        let map = RegisteredPropertiesMap::new();
        assert_eq!(map.get("--nonexistent"), None);
    }

    #[test]
    fn test_register_property_all_empty() {
        let map = RegisteredPropertiesMap::new();
        assert_eq!(map.all().len(), 0);
    }

    #[test]
    fn test_register_property_all_multiple() {
        let mut map = RegisteredPropertiesMap::new();
        map.register(
            "--prop1".to_string(),
            RegisteredProperty {
                name: "--prop1".to_string(),
                syntax: "*".to_string(),
                inherits: false,
                initial_value: "0".to_string(),
            },
        );
        map.register(
            "--prop2".to_string(),
            RegisteredProperty {
                name: "--prop2".to_string(),
                syntax: "<length>".to_string(),
                inherits: true,
                initial_value: "10px".to_string(),
            },
        );

        let all = map.all();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|(name, _)| name == "--prop1"));
        assert!(all.iter().any(|(name, _)| name == "--prop2"));
    }

    #[test]
    fn test_register_property_clear() {
        let mut map = RegisteredPropertiesMap::new();
        map.register(
            "--test".to_string(),
            RegisteredProperty {
                name: "--test".to_string(),
                syntax: "*".to_string(),
                inherits: true,
                initial_value: "".to_string(),
            },
        );
        assert_eq!(map.all().len(), 1);

        map.clear();
        assert_eq!(map.all().len(), 0);
    }

    #[test]
    fn test_registered_property_clone() {
        let prop1 = RegisteredProperty {
            name: "--test".to_string(),
            syntax: "<color>".to_string(),
            inherits: false,
            initial_value: "red".to_string(),
        };
        let prop2 = prop1.clone();

        assert_eq!(prop1.name, prop2.name);
        assert_eq!(prop1.syntax, prop2.syntax);
        assert_eq!(prop1.inherits, prop2.inherits);
        assert_eq!(prop1.initial_value, prop2.initial_value);
    }

    #[test]
    fn test_registered_property_defaults() {
        let prop = RegisteredProperty {
            name: "--custom".to_string(),
            syntax: "*".to_string(),
            inherits: true,
            initial_value: "fallback".to_string(),
        };

        assert!(prop.inherits);
        assert_eq!(prop.syntax, "*");
    }

}
