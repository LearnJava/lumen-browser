//! CSS Paint Worklet API stub (Houdini) — Phase 0
//! Implements CSS.paintWorklet.addModule() and paint() invocation registration.

use rquickjs::Ctx;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Global registry of registered paint classes (keyed by worklet name).
static PAINT_WORKLET_REGISTRY: OnceLock<Mutex<PaintWorkletRegistry>> = OnceLock::new();

/// Maps worklet name (e.g. "my-paint") to its definition.
#[derive(Clone, Debug, Default)]
pub struct PaintWorkletRegistry {
    worklets: HashMap<String, PaintWorkletDef>,
}

impl PaintWorkletRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a paint worklet definition.
    pub fn register(&mut self, name: String, def: PaintWorkletDef) {
        self.worklets.insert(name, def);
    }

    /// Look up a registered worklet by name.
    pub fn get(&self, name: &str) -> Option<PaintWorkletDef> {
        self.worklets.get(name).cloned()
    }

    /// Get all registered worklets.
    pub fn all(&self) -> Vec<(String, PaintWorkletDef)> {
        self.worklets.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Clear all registrations (for tests).
    pub fn clear(&mut self) {
        self.worklets.clear();
    }
}

/// Get the global paint worklet registry, initializing it if necessary.
pub fn get_paint_worklet_registry() -> &'static Mutex<PaintWorkletRegistry> {
    PAINT_WORKLET_REGISTRY.get_or_init(|| Mutex::new(PaintWorkletRegistry::new()))
}

/// Definition of a registered paint worklet.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintWorkletDef {
    /// Worklet name (e.g. "my-paint").
    pub name: String,
    /// Module URL from CSS.paintWorklet.addModule().
    pub module_url: String,
    /// Input properties used by the paint function.
    pub input_properties: Vec<String>,
}

/// Install CSS.paintWorklet bindings into the JS context.
/// Uses a pure JS shim that stores worklet definitions in a global registry.
pub fn install_paint_worklet_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(PAINT_WORKLET_SHIM)?;
    Ok(())
}

/// Pure-JS CSS Paint Worklet API shim.
/// Defines CSS.paintWorklet.addModule() and registerPaint().
const PAINT_WORKLET_SHIM: &str = r#"(function(global) {
  'use strict';

  // Store registered paint worklets in a global map accessible from Rust bindings.
  if (!global._lumen_paint_worklets) {
    global._lumen_paint_worklets = new Map();
  }

  // Create or extend CSS global object.
  if (!global.CSS) {
    global.CSS = {};
  }

  // CSS.paintWorklet stub - manages paint function registrations.
  global.CSS.paintWorklet = {
    /// Module URL being loaded (tracks context during addModule execution).
    _currentModule: null,

    /// Add a paint module, execute it to allow registerPaint calls.
    addModule: function(moduleUrl) {
      return Promise.resolve().then(() => {
        // Phase 0 stub: accept the URL but don't fetch/execute it.
        // In Phase 1, this would fetch the module, execute it in a worker context,
        // and collect registerPaint() calls via a proxy.
        this._currentModule = moduleUrl;
        return undefined;
      });
    }
  };

  // registerPaint() function - called within a paint module to register a class.
  // In Phase 0, stores the registration in the global map.
  // In Phase 1, would store input properties and execute paint() callbacks.
  if (!global.registerPaint) {
    global.registerPaint = function(name, paintClass) {
      if (typeof name !== 'string') {
        throw new TypeError('registerPaint: name must be a string');
      }
      if (typeof paintClass !== 'function' && typeof paintClass !== 'object') {
        throw new TypeError('registerPaint: paintClass must be a constructor or object');
      }

      // Store the registration in the global registry.
      const moduleUrl = global.CSS.paintWorklet._currentModule || '';
      const def = {
        name: name,
        moduleUrl: moduleUrl,
        inputProperties: paintClass.inputProperties || []
      };
      global._lumen_paint_worklets.set(name, def);
    };
  }
})(globalThis)"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paint_worklet_registry_register() {
        let mut registry = PaintWorkletRegistry::new();
        let def = PaintWorkletDef {
            name: "test-paint".to_string(),
            module_url: "https://example.com/paint.js".to_string(),
            input_properties: vec!["--color".to_string()],
        };
        registry.register("test-paint".to_string(), def.clone());
        assert_eq!(registry.get("test-paint"), Some(def));
    }

    #[test]
    fn test_paint_worklet_registry_clear() {
        let mut registry = PaintWorkletRegistry::new();
        let def = PaintWorkletDef {
            name: "test".to_string(),
            module_url: "test.js".to_string(),
            input_properties: vec![],
        };
        registry.register("test".to_string(), def);
        registry.clear();
        assert_eq!(registry.get("test"), None);
    }

    #[test]
    fn test_paint_worklet_def_clone() {
        let def = PaintWorkletDef {
            name: "clone-test".to_string(),
            module_url: "module.js".to_string(),
            input_properties: vec!["--size".to_string(), "--angle".to_string()],
        };
        let cloned = def.clone();
        assert_eq!(def, cloned);
    }
}
