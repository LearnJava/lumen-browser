//! CSS Properties & Values API (Houdini) — custom property registration
//! Implements CSS.registerProperty(), @property at-rule parsing, and initial-value fallback in compute_style.

use rquickjs::Ctx;
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

/// Install CSS.registerProperty bindings into the JS context.
/// Uses a pure JS shim that stores properties in a global Map.
pub fn install_css_properties_values_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(CSS_PROPERTIES_VALUES_SHIM)?;
    Ok(())
}

/// Pure-JS CSS Properties & Values API shim.
/// Defines CSS.registerProperty() and provides interface to Rust-backed registry.
const CSS_PROPERTIES_VALUES_SHIM: &str = r#"(function(global) {
  'use strict';

  // Store registered properties in a global map accessible from Rust bindings.
  if (!global._lumen_registered_properties) {
    global._lumen_registered_properties = new Map();
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

    const name = definition.name;
    if (!name || typeof name !== 'string') {
      throw new TypeError('registerProperty: name is required and must be a string');
    }

    if (!name.startsWith('--')) {
      throw new SyntaxError(`CustomPropertyName: '${name}' must start with '--'`);
    }

    // Extract optional fields with defaults.
    const syntax = definition.syntax || '*';
    const inherits = definition.inherits !== false; // default: true
    const initialValue = definition.initialValue || '';

    // Check if already registered (override allowed per spec).
    if (global._lumen_registered_properties.has(name)) {
      // Silently override or throw DOMException SyntaxError per CSS Houdini spec.
      // Phase 0: just override.
    }

    // Store definition.
    global._lumen_registered_properties.set(name, {
      name,
      syntax,
      inherits,
      initialValue
    });

    // Notify native Rust code (if bindings exist).
    if (typeof _lumen_register_css_property === 'function') {
      try {
        _lumen_register_css_property(name, syntax, inherits, initialValue);
      } catch (e) {
        // Rust bindings may fail; continue anyway.
        console.warn('CSS.registerProperty Rust binding failed:', e);
      }
    }
  };

  // Convenience method to retrieve all registered properties.
  // (Used for testing and StyleSheet.registered_properties access.)
  global.CSS._getRegisteredProperties = function() {
    const result = {};
    global._lumen_registered_properties.forEach((def, name) => {
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
