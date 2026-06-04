//! CSS Highlight API (L1 3) — custom text highlight registration
//! Implements CSS.highlights registry via JS shim and Rust backing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static HIGHLIGHTS_REGISTRY: OnceLock<Mutex<HighlightRegistry>> = OnceLock::new();

#[derive(Clone, Debug, Default)]
pub struct HighlightRegistry {
    highlights: HashMap<String, Highlight>,
}

impl HighlightRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: String, highlight: Highlight) {
        self.highlights.insert(name, highlight);
    }

    pub fn get(&self, name: &str) -> Option<Highlight> {
        self.highlights.get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.highlights.contains_key(name)
    }

    pub fn delete(&mut self, name: &str) -> bool {
        self.highlights.remove(name).is_some()
    }

    pub fn clear(&mut self) {
        self.highlights.clear();
    }

    pub fn all(&self) -> Vec<(String, Highlight)> {
        self.highlights
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

pub fn get_highlights_registry() -> &'static Mutex<HighlightRegistry> {
    HIGHLIGHTS_REGISTRY.get_or_init(|| Mutex::new(HighlightRegistry::new()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub priority: i32,
    pub range_ids: Vec<String>,
}

impl Highlight {
    pub fn new(priority: i32, range_ids: Vec<String>) -> Self {
        Self {
            priority,
            range_ids,
        }
    }
}

pub fn install_highlight_api_bindings(ctx: &rquickjs::Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(HIGHLIGHT_API_SHIM)?;
    Ok(())
}

const HIGHLIGHT_API_SHIM: &str = r#"(function(global) {
  'use strict';

  // HighlightRegistry map stored in global scope
  const _highlightRegistry = {};

  // Highlight constructor: new Highlight(...ranges)
  global.Highlight = function Highlight(...ranges) {
    this.priority = 0;
    this.ranges = ranges;
  };

  // CSS.highlights object with set/get/has/delete/clear methods
  if (!global.CSS) global.CSS = {};

  global.CSS.highlights = {
    set: function(name, highlight) {
      _highlightRegistry[name] = highlight || { priority: 0, ranges: [] };
    },
    
    get: function(name) {
      return _highlightRegistry[name];
    },
    
    has: function(name) {
      return name in _highlightRegistry;
    },
    
    delete: function(name) {
      if (name in _highlightRegistry) {
        delete _highlightRegistry[name];
        return true;
      }
      return false;
    },
    
    clear: function() {
      for (const key in _highlightRegistry) {
        delete _highlightRegistry[key];
      }
    }
  };

  // Expose _highlightRegistry for Rust bindings if needed
  global._highlightRegistry = _highlightRegistry;
})(globalThis);"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_registry_new() {
        let reg = HighlightRegistry::new();
        assert!(reg.all().is_empty());
    }

    #[test]
    fn highlight_registry_set_get() {
        let mut reg = HighlightRegistry::new();
        let hl = Highlight::new(10, vec!["range-1".to_string()]);
        reg.set("search".to_string(), hl.clone());

        let retrieved = reg.get("search").unwrap();
        assert_eq!(retrieved.priority, 10);
        assert_eq!(retrieved.range_ids, vec!["range-1"]);
    }

    #[test]
    fn highlight_registry_has() {
        let mut reg = HighlightRegistry::new();
        let hl = Highlight::new(0, Vec::new());
        reg.set("spelling-error".to_string(), hl);

        assert!(reg.has("spelling-error"));
        assert!(!reg.has("nonexistent"));
    }

    #[test]
    fn highlight_registry_delete() {
        let mut reg = HighlightRegistry::new();
        let hl = Highlight::new(0, Vec::new());
        reg.set("highlight-1".to_string(), hl);

        assert!(reg.delete("highlight-1"));
        assert!(!reg.has("highlight-1"));
    }

    #[test]
    fn highlight_registry_clear() {
        let mut reg = HighlightRegistry::new();
        reg.set("h1".to_string(), Highlight::new(1, Vec::new()));
        reg.set("h2".to_string(), Highlight::new(2, Vec::new()));

        assert_eq!(reg.all().len(), 2);
        reg.clear();
        assert!(reg.all().is_empty());
    }

    #[test]
    fn highlight_priority_ordering() {
        let h1 = Highlight::new(10, vec!["range-1".to_string()]);
        let h2 = Highlight::new(5, vec!["range-2".to_string()]);

        assert!(h1.priority > h2.priority);
    }

    #[test]
    fn highlight_default_priority() {
        let hl = Highlight::new(0, Vec::new());
        assert_eq!(hl.priority, 0);
    }

    #[test]
    fn highlight_registry_overwrites() {
        let mut reg = HighlightRegistry::new();
        let hl1 = Highlight::new(5, Vec::new());
        let hl2 = Highlight::new(10, Vec::new());

        reg.set("name".to_string(), hl1);
        reg.set("name".to_string(), hl2);

        let retrieved = reg.get("name").unwrap();
        assert_eq!(retrieved.priority, 10);
    }

    #[test]
    fn highlight_registry_all() {
        let mut reg = HighlightRegistry::new();
        reg.set("h1".to_string(), Highlight::new(1, Vec::new()));
        reg.set("h2".to_string(), Highlight::new(2, Vec::new()));

        let all = reg.all();
        assert_eq!(all.len(), 2);
    }
}
