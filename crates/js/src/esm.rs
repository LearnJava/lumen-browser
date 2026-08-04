//! ES Module URL-resolution infrastructure for `<script type=module>` support
//! (HTML LS §8.1.3), shared by the V8 module loader ([`crate::v8_esm`]).
//!
//! Module specifier resolution follows URL Standard §5.1:
//! - Absolute URLs passed through unchanged.
//! - Relative specifiers (`./foo.js`, `../bar.js`) resolved against `base_url`.
//! - Bare specifiers (`lodash`) kept as-is (caller must pre-register them by canonical name).

use std::collections::HashMap;

/// Import map: specifier mappings for bare specifiers and scoped paths.
///
/// Parsed from `<script type="importmap">` JSON per WHATWG Import Maps spec.
/// Supports `imports` (global mappings) and `scopes` (context-specific mappings).
#[derive(Debug, Clone, Default)]
pub struct ImportMap {
    /// Global import mappings: specifier → resolved URL.
    pub imports: HashMap<String, String>,
    /// Scoped mappings: scope URL → { specifier → resolved URL }.
    pub scopes: HashMap<String, HashMap<String, String>>,
}

impl ImportMap {
    /// Parse an import map from a JSON string.
    ///
    /// Returns `None` if the JSON is invalid or missing required fields.
    /// Silently ignores unknown keys and invalid entries.
    pub fn parse(json_str: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
        let mut map = ImportMap::default();

        // Parse "imports" object
        if let Some(imports_obj) = value.get("imports").and_then(|v| v.as_object()) {
            for (key, val) in imports_obj {
                if let Some(url) = val.as_str() {
                    map.imports.insert(key.clone(), url.to_string());
                }
            }
        }

        // Parse "scopes" object
        if let Some(scopes_obj) = value.get("scopes").and_then(|v| v.as_object()) {
            for (scope_key, scope_val) in scopes_obj {
                if let Some(scope_map) = scope_val.as_object() {
                    let mut scope_imports = HashMap::new();
                    for (key, val) in scope_map {
                        if let Some(url) = val.as_str() {
                            scope_imports.insert(key.clone(), url.to_string());
                        }
                    }
                    if !scope_imports.is_empty() {
                        map.scopes.insert(scope_key.clone(), scope_imports);
                    }
                }
            }
        }

        Some(map)
    }

    /// Resolve a specifier using this import map.
    ///
    /// Returns the resolved URL if found, or `None` if the specifier is not in the map.
    pub fn resolve(&self, specifier: &str, _scope_url: Option<&str>) -> Option<String> {
        // Try exact match in imports
        if let Some(url) = self.imports.get(specifier) {
            return Some(url.clone());
        }

        // Try longest prefix match in imports for packages like "lodash" → "lodash/index.js"
        // when specifier is "lodash/foo.js"
        let mut best_prefix = "";
        let mut best_url = None;
        for (prefix, url) in &self.imports {
            if specifier.starts_with(prefix) && prefix.len() > best_prefix.len() {
                // Ensure we match on package boundary: "lodash" matches "lodash/foo.js"
                // but not "lodashing"
                let rest = &specifier[prefix.len()..];
                if rest.is_empty() || rest.starts_with('/') {
                    best_prefix = prefix;
                    best_url = Some((prefix, url));
                }
            }
        }

        if let Some((prefix, url)) = best_url {
            let rest = &specifier[prefix.len()..];
            return Some(format!("{}{}", url, rest));
        }

        None
    }
}

/// Resolve `name` relative to `base` using simplified URL resolution rules.
///
/// Rules (in priority order):
/// 1. `data:` and `blob:` prefixes — return unchanged.
/// 2. Absolute HTTP/HTTPS URL (starts with `https://` or `http://`) — unchanged.
/// 3. `./` or `../` prefix — resolve relative to `base`.
///    If `base` is empty or a virtual `lumen://` specifier, fall back to `page_url`.
/// 4. Bare specifier — try import map, fall back to returning unchanged.
///
/// `page_url` is the fallback base for relative specifiers whose importer has
/// no meaningful directory (inline `lumen://inline-N` module scripts). Used by
/// the V8 module loader ([`crate::v8_esm`]), whose resolve callback is a
/// captureless `fn` reading thread-local state.
pub fn resolve_specifier_with(
    page_url: &str,
    import_map: &ImportMap,
    base: &str,
    name: &str,
) -> String {
    // (1) data: and blob: — pass through
    if name.starts_with("data:") || name.starts_with("blob:") {
        return name.to_owned();
    }
    // (2) Absolute URL — pass through
    if name.starts_with("https://") || name.starts_with("http://") || name.starts_with("file://") {
        return name.to_owned();
    }
    // (3) Relative specifier — resolve against base
    if name.starts_with("./") || name.starts_with("../") {
        // `lumen://inline-N` is a virtual specifier assigned to inline module scripts.
        // Relative imports from them should resolve against the page URL, not the
        // virtual specifier (which has no meaningful directory path).
        let effective_base = if base.is_empty() || base.starts_with("lumen://") {
            page_url
        } else {
            base
        };
        return resolve_relative(effective_base, name);
    }
    // (4) Bare specifier — try import map
    if let Some(resolved) = import_map.resolve(name, Some(base)) {
        return resolved;
    }
    // Fall back to returning as-is
    name.to_owned()
}

// ── URL utilities ─────────────────────────────────────────────────────────────

/// Resolve a relative URL `name` against `base`.
///
/// Strips the last path component from `base`, appends `name`, then normalises
/// `./` and `../` segments. Preserves scheme + authority prefix from `base`.
fn resolve_relative(base: &str, name: &str) -> String {
    // Extract scheme+authority prefix from base (e.g. "https://example.com")
    let prefix_end = base.find("://")
        .map(|i| {
            let after_scheme = i + 3;
            base[after_scheme..].find('/').map(|j| after_scheme + j).unwrap_or(base.len())
        })
        .unwrap_or(0);

    // Base directory: strip everything after the last `/`
    let base_dir = if let Some(slash) = base.rfind('/') {
        if slash >= prefix_end {
            &base[..slash + 1]
        } else {
            base
        }
    } else {
        base
    };

    // Join base_dir + name and normalise segments
    let joined = format!("{base_dir}{name}");
    normalize_path(&joined)
}

/// Collapse `./` and `../` path segments in `url`.
fn normalize_path(url: &str) -> String {
    // Split into scheme+authority and path parts
    let (prefix, path) = if let Some(idx) = url.find("://") {
        let after = idx + 3;
        let path_start = url[after..].find('/').map(|i| after + i).unwrap_or(url.len());
        (&url[..path_start], &url[path_start..])
    } else {
        ("", url)
    };

    let mut segments: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "." | "" if !segments.is_empty() => {}
            ".." => { segments.pop(); }
            s => segments.push(s),
        }
    }
    format!("{prefix}{}", segments.join("/"))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_map_parse_basic() {
        let json = r#"{ "imports": { "react": "/vendor/react.js" } }"#;
        let map = ImportMap::parse(json).unwrap();
        assert_eq!(map.imports.get("react"), Some(&"/vendor/react.js".to_string()));
    }

    #[test]
    fn import_map_parse_multiple() {
        let json = r#"{
            "imports": {
                "react": "/vendor/react.js",
                "lodash": "/vendor/lodash/index.js"
            }
        }"#;
        let map = ImportMap::parse(json).unwrap();
        assert_eq!(map.imports.len(), 2);
        assert_eq!(map.imports.get("react"), Some(&"/vendor/react.js".to_string()));
        assert_eq!(map.imports.get("lodash"), Some(&"/vendor/lodash/index.js".to_string()));
    }

    #[test]
    fn import_map_parse_with_scopes() {
        let json = r#"{
            "imports": { "react": "/vendor/react.js" },
            "scopes": {
                "/app/": { "utils": "/app/utils.js" }
            }
        }"#;
        let map = ImportMap::parse(json).unwrap();
        assert_eq!(map.imports.get("react"), Some(&"/vendor/react.js".to_string()));
        assert!(map.scopes.contains_key("/app/"));
    }

    #[test]
    fn import_map_parse_invalid_json() {
        let json = "{ invalid }";
        assert!(ImportMap::parse(json).is_none());
    }

    #[test]
    fn import_map_resolve_exact() {
        let json = r#"{ "imports": { "react": "/vendor/react.js" } }"#;
        let map = ImportMap::parse(json).unwrap();
        assert_eq!(map.resolve("react", None), Some("/vendor/react.js".to_string()));
        assert_eq!(map.resolve("missing", None), None);
    }

    #[test]
    fn import_map_resolve_package_path() {
        let json = r#"{ "imports": { "lodash": "/vendor/lodash/index.js" } }"#;
        let map = ImportMap::parse(json).unwrap();
        assert_eq!(
            map.resolve("lodash/map", None),
            Some("/vendor/lodash/index.js/map".to_string())
        );
    }

    #[test]
    fn import_map_resolve_package_boundary() {
        let json = r#"{ "imports": { "lodash": "/vendor/lodash/index.js" } }"#;
        let map = ImportMap::parse(json).unwrap();
        // "lodashing" should NOT match "lodash" — must be package boundary
        assert_eq!(map.resolve("lodashing", None), None);
    }

    #[test]
    fn absolute_url_unchanged() {
        let map = ImportMap::default();
        assert_eq!(
            resolve_specifier_with(
                "https://example.com/app.html",
                &map,
                "https://example.com/app.html",
                "https://cdn.example.com/lib.js",
            ),
            "https://cdn.example.com/lib.js"
        );
    }

    #[test]
    fn data_url_unchanged() {
        let map = ImportMap::default();
        let data = "data:text/javascript,export const x=1;";
        assert_eq!(
            resolve_specifier_with("https://example.com/", &map, "", data),
            data
        );
    }

    #[test]
    fn relative_same_dir() {
        let map = ImportMap::default();
        assert_eq!(
            resolve_specifier_with(
                "https://example.com/app.html",
                &map,
                "https://example.com/app.html",
                "./utils.js",
            ),
            "https://example.com/utils.js"
        );
    }

    #[test]
    fn relative_parent_dir() {
        let map = ImportMap::default();
        assert_eq!(
            resolve_specifier_with(
                "https://example.com/app/main.js",
                &map,
                "https://example.com/app/main.js",
                "../lib/util.js",
            ),
            "https://example.com/lib/util.js"
        );
    }

    #[test]
    fn bare_specifier_unchanged() {
        let map = ImportMap::default();
        assert_eq!(
            resolve_specifier_with("https://example.com/", &map, "https://example.com/", "lodash"),
            "lodash"
        );
    }

    #[test]
    fn relative_uses_page_url_when_base_empty() {
        let map = ImportMap::default();
        assert_eq!(
            resolve_specifier_with("https://example.com/page.html", &map, "", "./helper.js"),
            "https://example.com/helper.js"
        );
    }

    #[test]
    fn relative_uses_page_url_for_virtual_lumen_base() {
        // Inline module scripts get a virtual lumen://inline-N specifier.
        // Relative imports from them should resolve against the page URL.
        let map = ImportMap::default();
        assert_eq!(
            resolve_specifier_with(
                "https://example.com/page.html",
                &map,
                "lumen://inline-0",
                "./helper.js",
            ),
            "https://example.com/helper.js"
        );
    }
}
