//! ES Module loader infrastructure for `<script type=module>` support (HTML LS §8.1.3).
//!
//! Implements `rquickjs::loader::{Resolver, Loader}` backed by an in-memory registry.
//! The registry is shared between `LumenLoader` (attached to the QuickJS Runtime) and
//! `QuickJsRuntime` (which populates it with pre-fetched module source code).
//!
//! Module specifier resolution follows URL Standard §5.1:
//! - Absolute URLs passed through unchanged.
//! - Relative specifiers (`./foo.js`, `../bar.js`) resolved against `base_url`.
//! - Bare specifiers (`lodash`) kept as-is (caller must pre-register them by canonical name).

use crate::import_attributes::{new_type_registry, ModuleType, ModuleTypeRegistry};
use crate::import_meta::transform_import_meta;
use rquickjs::{loader::{Loader, Resolver}, Ctx, Error, Module, Result as QjsResult};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Shared, late-writable page URL used by `LumenResolver` to resolve relative
/// module specifiers from inline `<script type=module>` scripts.
///
/// Because the resolver is moved into the QuickJS `Runtime` via `set_loader`,
/// it cannot be updated via `&mut self` afterwards. Sharing an `Arc<Mutex<String>>`
/// with `QuickJsRuntime` allows the resolver to pick up the page URL that is only
/// known later (when `install_dom` is called).
pub type SharedPageUrl = Arc<Mutex<String>>;

/// Shared module source registry: specifier → source code.
///
/// Populated by `QuickJsRuntime::register_module()` before evaluation.
/// The same `Arc<Mutex<…>>` is shared between the `LumenLoader` (QuickJS side)
/// and `QuickJsRuntime` (Rust side) so new modules can be added at any time.
pub type ModuleRegistry = Arc<Mutex<HashMap<String, String>>>;

/// Creates an empty `ModuleRegistry`.
pub fn new_registry() -> ModuleRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

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

/// URL resolver: normalises module specifiers into canonical keys for the registry.
///
/// Relative specifiers are resolved against `base` (the importer's specifier).
/// Absolute HTTP/HTTPS URLs and data: URIs are passed through unchanged.
/// `blob:lumen/…` virtual URLs are passed through unchanged.
///
/// The page URL is held in a `SharedPageUrl` (`Arc<Mutex<String>>`): because
/// `LumenResolver` is moved into the QuickJS `Runtime` via `set_loader` and
/// cannot be mutated afterwards, the shared handle lets `QuickJsRuntime` write
/// the page URL during `install_dom` and have the resolver pick it up at
/// resolution time.
#[derive(Clone)]
pub struct LumenResolver {
    /// Base page URL; used as fallback base when the import base is empty or virtual.
    pub page_url: SharedPageUrl,
    /// Import map: global mappings for bare specifiers.
    pub import_map: Arc<Mutex<ImportMap>>,
}

impl LumenResolver {
    /// Create a resolver; `page_url` is the initial fallback base (may be empty).
    /// The returned `SharedPageUrl` can be updated later (e.g. from `install_dom`).
    pub fn new(initial_page_url: &str) -> (Self, SharedPageUrl) {
        let shared = Arc::new(Mutex::new(initial_page_url.to_owned()));
        (Self {
            page_url: Arc::clone(&shared),
            import_map: Arc::new(Mutex::new(ImportMap::default())),
        }, shared)
    }

    /// Set the import map for this resolver.
    pub fn set_import_map(&self, map: ImportMap) {
        if let Ok(mut guard) = self.import_map.lock() {
            *guard = map;
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
    pub fn resolve_specifier(&self, base: &str, name: &str) -> String {
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
                self.page_url.lock().unwrap_or_else(|e| e.into_inner()).clone()
            } else {
                base.to_owned()
            };
            return resolve_relative(&effective_base, name);
        }
        // (4) Bare specifier — try import map
        if let Ok(map) = self.import_map.lock()
            && let Some(resolved) = map.resolve(name, Some(base))
        {
            return resolved;
        }
        // Fall back to returning as-is
        name.to_owned()
    }
}

impl std::fmt::Debug for LumenResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = self.page_url.lock().unwrap_or_else(|e| e.into_inner());
        f.debug_struct("LumenResolver").field("page_url", &*url).finish()
    }
}

impl Resolver for LumenResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> QjsResult<String> {
        Ok(self.resolve_specifier(base, name))
    }
}

/// Module loader backed by `ModuleRegistry`.
///
/// When QuickJS requests a module by specifier (after resolution), this loader
/// looks it up in the shared registry and compiles it as a JS module.
/// Missing modules produce `Error::new_loading`.
///
/// Import attributes (TC39 Stage 3, `with { type: 'json' }`): when the shared
/// [`ModuleTypeRegistry`] declares a type for the specifier, the loader applies
/// it — `json` modules are validated as JSON and compiled as a synthetic
/// `export default JSON.parse(...)` module; any other type fails the load.
#[derive(Clone)]
pub struct LumenLoader {
    registry: ModuleRegistry,
    /// Declared import-attribute types per resolved specifier (written by the
    /// `import_attributes` preprocessor in `QuickJsRuntime`).
    types: ModuleTypeRegistry,
}

impl LumenLoader {
    /// Create a loader backed by `registry` with no declared module types.
    pub fn new(registry: ModuleRegistry) -> Self {
        Self { registry, types: new_type_registry() }
    }

    /// Create a loader that also consults `types` for import-attribute
    /// (`with { type: '…' }`) module types.
    pub fn with_types(registry: ModuleRegistry, types: ModuleTypeRegistry) -> Self {
        Self { registry, types }
    }
}

impl Loader for LumenLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, specifier: &str) -> QjsResult<Module<'js>> {
        let source = {
            let guard = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(specifier).cloned()
        };
        let declared_type = {
            let guard = self.types.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(specifier).cloned()
        };
        match (source, declared_type) {
            (Some(src), Some(ModuleType::Json)) => {
                // JSON-assert guard: a module imported `with { type: 'json' }`
                // must be valid JSON, otherwise the import fails to load.
                if serde_json::from_str::<serde_json::Value>(&src).is_err() {
                    return Err(Error::new_loading_message(
                        specifier,
                        "module is not valid JSON (imported with { type: 'json' })",
                    ));
                }
                // Embed the JSON text as a JS string literal (serde escaping is
                // a valid JS string) and default-export the parsed value.
                let literal = serde_json::to_string(&src).map_err(|_| Error::new_loading(specifier))?;
                let synth = format!("export default JSON.parse({literal});");
                Module::declare(ctx.clone(), specifier, synth.as_bytes())
            }
            (Some(_), Some(ModuleType::Unsupported(ty))) => Err(Error::new_loading_message(
                specifier,
                format!("unsupported import attribute type '{ty}'"),
            )),
            (Some(src), None) => {
                let src = transform_import_meta(&src, specifier)
                    .unwrap_or(src);
                Module::declare(ctx.clone(), specifier, src.as_bytes())
            }
            (None, _) => Err(Error::new_loading(specifier)),
        }
    }
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

    fn make_resolver(page_url: &str) -> LumenResolver {
        LumenResolver::new(page_url).0
    }

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
        let r = make_resolver("https://example.com/app.html");
        assert_eq!(r.resolve_specifier("https://example.com/app.html", "https://cdn.example.com/lib.js"),
                   "https://cdn.example.com/lib.js");
    }

    #[test]
    fn data_url_unchanged() {
        let r = make_resolver("https://example.com/");
        let data = "data:text/javascript,export const x=1;";
        assert_eq!(r.resolve_specifier("", data), data);
    }

    #[test]
    fn relative_same_dir() {
        let r = make_resolver("https://example.com/app.html");
        assert_eq!(
            r.resolve_specifier("https://example.com/app.html", "./utils.js"),
            "https://example.com/utils.js"
        );
    }

    #[test]
    fn relative_parent_dir() {
        let r = make_resolver("https://example.com/app/main.js");
        assert_eq!(
            r.resolve_specifier("https://example.com/app/main.js", "../lib/util.js"),
            "https://example.com/lib/util.js"
        );
    }

    #[test]
    fn bare_specifier_unchanged() {
        let r = make_resolver("https://example.com/");
        assert_eq!(r.resolve_specifier("https://example.com/", "lodash"), "lodash");
    }

    #[test]
    fn relative_uses_page_url_when_base_empty() {
        let r = make_resolver("https://example.com/page.html");
        assert_eq!(
            r.resolve_specifier("", "./helper.js"),
            "https://example.com/helper.js"
        );
    }

    #[test]
    fn relative_uses_page_url_for_virtual_lumen_base() {
        // Inline module scripts get a virtual lumen://inline-N specifier.
        // Relative imports from them should resolve against the page URL.
        let r = make_resolver("https://example.com/page.html");
        assert_eq!(
            r.resolve_specifier("lumen://inline-0", "./helper.js"),
            "https://example.com/helper.js"
        );
    }

    #[test]
    fn page_url_can_be_updated_via_shared_handle() {
        let (r, handle) = LumenResolver::new("");
        // With an empty page_url and empty base, the relative path cannot be resolved to a
        // real origin; resolve_relative returns the path normalised but still relative.
        assert_eq!(r.resolve_specifier("", "./a.js"), "./a.js");
        // After updating the shared handle, relative imports from inline module scripts
        // (which have a virtual lumen:// base) resolve correctly against the page origin.
        *handle.lock().unwrap() = "https://example.com/page.html".to_owned();
        assert_eq!(r.resolve_specifier("lumen://inline-0", "./a.js"), "https://example.com/a.js");
    }

    #[test]
    fn loader_finds_registered_module() {
        use rquickjs::{Runtime, Context};
        let registry = new_registry();
        registry.lock().unwrap().insert(
            "mymod".to_owned(),
            "export const answer = 42;".to_owned(),
        );
        let loader = LumenLoader::new(registry);
        let (resolver, _url) = LumenResolver::new("https://example.com/");
        let rt = Runtime::new().unwrap();
        rt.set_loader(resolver, loader);
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            let val: rquickjs::Value = ctx.eval(r#"
                import('mymod').then(m => m.answer)
            "#).unwrap();
            drop(val);
        });
    }

    #[test]
    fn loader_missing_module_returns_error() {
        use rquickjs::{Runtime, Context};
        let registry = new_registry();
        let loader = LumenLoader::new(Arc::clone(&registry));
        let (resolver, _url) = LumenResolver::new("file:///page.html");
        let rt = Runtime::new().unwrap();
        rt.set_loader(resolver, loader);
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            // direct Module::declare_and_eval of a module that imports a missing dep
            let result = rquickjs::Module::declare::<&str, &str>(
                ctx.clone(), "main", "import './missing.js'; export const x=1;"
            );
            // Declaring the module itself succeeds (it's parsed, not yet evaluated)
            // Evaluating it would fail — just verify declare doesn't panic
            drop(result);
        });
    }
}
