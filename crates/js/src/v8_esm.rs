//! ES module loader for the V8 backend — `<script type=module>` (HTML LS §8.1.3).
//!
//! V8 port of the rquickjs ESM stack in [`crate::esm`] (Ph3 V8 migration S12b-23,
//! closes BUG-350). The two loaders differ in *plumbing*, not in policy:
//!
//! * rquickjs takes a `Resolver`/`Loader` pair by value (`Runtime::set_loader`),
//!   so `QuickJsRuntime` shares its registries with them through `Arc<Mutex<…>>`.
//! * V8's `ResolveModuleCallback` is a captureless `extern "C" fn` — it can only
//!   reach embedder state through a `thread_local!`. Since the isolate lives on
//!   one dedicated thread ([`crate::v8_runtime`]), a thread-local registry is
//!   exactly as scoped as the QuickJS one: per runtime, per thread.
//!
//! Specifier resolution is *shared* code — both loaders call
//! [`crate::esm::resolve_specifier_with`], so import maps, relative-URL joining
//! and the virtual `lumen://inline-N` base behave identically on both engines.
//!
//! What V8 does natively that the QuickJS path emulates:
//!
//! * **Import attributes** (`import … with { type: 'json' }`, TC39 Stage 3) are
//!   parsed by V8 and handed to the resolve callback as a `FixedArray`, so the
//!   Phase 0 source-stripping preprocessor ([`crate::import_attributes`]) is not
//!   used here — the attribute is read straight off the import request.
//! * **Dynamic `import()`** is wired through the isolate's
//!   `HostImportModuleDynamicallyCallback` instead of being driven by the
//!   loader trait.
//!
//! `import.meta` keeps using the shared source-level preprocessor
//! ([`crate::import_meta`]): its `.url` / `.resolve()` / `.env` shape is Lumen
//! policy rather than an engine capability, and reusing the transformer keeps
//! the two engines byte-identical on that surface.

use crate::esm::{resolve_specifier_with, ImportMap};
use crate::import_meta::transform_import_meta;
use std::cell::RefCell;
use std::collections::HashMap;

// ── Thread-local registry ─────────────────────────────────────────────────────

/// Module state owned by the JS thread that runs the isolate.
///
/// Mirrors the `module_registry` / `module_page_url` / `module_import_map`
/// fields of [`crate::QuickJsRuntime`], minus the `Arc<Mutex<…>>` wrappers: the
/// V8 resolve callback is captureless and can only read a `thread_local!`.
#[derive(Default)]
struct EsmState {
    /// Resolved specifier → module source, populated by [`register_source`].
    sources: HashMap<String, String>,
    /// Resolved specifier → compiled module, so a diamond import graph
    /// instantiates each module exactly once (ESM module-map semantics).
    modules: HashMap<String, v8::Global<v8::Module>>,
    /// `Module::get_identity_hash()` → resolved specifier, so the resolve
    /// callback can use the *referrer's* specifier as the base URL for
    /// relative imports.
    specifier_by_hash: HashMap<i32, String>,
    /// Page URL — fallback base for relative imports from inline modules.
    page_url: String,
    /// Import map (HTML LS §8.1.6.2) for bare specifiers.
    import_map: ImportMap,
    /// Counter behind the virtual `lumen://inline-N` specifiers.
    inline_seq: usize,
}

thread_local! {
    /// Per-JS-thread module registry. Dropping `v8::Global` handles after the
    /// isolate is disposed is a no-op (`Global::drop` checks isolate liveness),
    /// so thread-local teardown order is safe.
    static ESM: RefCell<EsmState> = RefCell::new(EsmState::default());
}

/// Run `f` against the thread-local module state.
///
/// The borrow must never span a call back into V8 (a resolve callback re-enters
/// these helpers), so every caller keeps it to a single map operation.
fn with_state<R>(f: impl FnOnce(&mut EsmState) -> R) -> R {
    ESM.with(|s| f(&mut s.borrow_mut()))
}

/// Set the page URL used as the base for relative imports from inline modules.
///
/// Called from `install_dom` on the JS thread, mirroring the write to
/// `QuickJsRuntime::module_page_url`.
pub(crate) fn set_page_url(url: &str) {
    with_state(|s| s.page_url = url.to_owned());
}

/// Install the import map (HTML LS §8.1.6.2) used for bare specifiers.
pub(crate) fn set_import_map(map: ImportMap) {
    with_state(|s| s.import_map = map);
}

/// Pre-register a module `source` under its resolved `specifier`.
///
/// Call before [`evaluate_entry_module`] so intra-page imports resolve.
pub(crate) fn register_source(specifier: &str, source: &str) {
    with_state(|s| {
        s.sources.insert(specifier.to_owned(), source.to_owned());
    });
}

/// Drop every registered module, compiled module and specifier mapping.
///
/// Used by the tests to isolate runs that share one JS thread; also the right
/// hook for a future document teardown (a new page must not inherit the
/// previous page's module map).
pub(crate) fn reset() {
    with_state(|s| *s = EsmState::default());
}

/// Allocate the next virtual `lumen://inline-N` specifier and register `source`
/// under it. Inline `<script type=module>` bodies have no URL of their own; the
/// virtual specifier gives the module map a stable key while
/// [`crate::esm::resolve_specifier_with`] redirects relative imports from it to
/// the page URL.
///
/// `pub(crate)` since BUG-571: a `<script type=module>` inserted by page script
/// is prepared in the JS shim, which registers its body through the
/// `_lumen_esm_register_inline` native and then `import()`s the returned
/// specifier.
pub(crate) fn register_inline(source: &str) -> String {
    with_state(|s| {
        let key = format!("lumen://inline-{}", s.inline_seq);
        s.inline_seq += 1;
        s.sources.insert(key.clone(), source.to_owned());
        key
    })
}

/// Resolve `name` as imported from `base`, applying the import map and the page
/// URL exactly like the rquickjs `LumenResolver`.
fn resolve(base: &str, name: &str) -> String {
    with_state(|s| resolve_specifier_with(&s.page_url, &s.import_map, base, name))
}

// ── Module compilation ────────────────────────────────────────────────────────

/// Declared module type of an import request, read from V8's import attributes.
enum DeclaredType {
    /// No `type` attribute — a plain JavaScript module.
    Js,
    /// `with { type: 'json' }` — source must be valid JSON, default-exported.
    Json,
    /// Any other declared type; per spec, importing it is an error.
    Unsupported(String),
}

/// Extract the `type` import attribute from V8's flat attribute array.
///
/// V8 packs the attributes as `[key, value, …]` repeated; the array handed to
/// `ResolveModuleCallback` carries a third per-attribute slot (the source
/// offset), the one handed to the dynamic-import callback does not — hence
/// `stride`.
fn declared_type(
    scope: &v8::PinScope<'_, '_>,
    attributes: v8::Local<'_, v8::FixedArray>,
    stride: usize,
) -> DeclaredType {
    let len = attributes.length();
    let mut i = 0;
    while i + 1 < len {
        let key = attributes
            .get(scope, i)
            .and_then(|d| v8::Local::<v8::Value>::try_from(d).ok())
            .map(|v| v.to_rust_string_lossy(scope));
        if key.as_deref() == Some("type") {
            let value = attributes
                .get(scope, i + 1)
                .and_then(|d| v8::Local::<v8::Value>::try_from(d).ok())
                .map(|v| v.to_rust_string_lossy(scope))
                .unwrap_or_default();
            return if value == "json" {
                DeclaredType::Json
            } else {
                DeclaredType::Unsupported(value)
            };
        }
        i += stride;
    }
    DeclaredType::Js
}

/// Turn a registered source into the text V8 actually compiles for `specifier`.
///
/// * `Json` wraps the (validated) JSON payload in a synthetic
///   `export default JSON.parse(<literal>)` module — the same shape the
///   rquickjs `LumenLoader` synthesises.
/// * `Js` runs the shared `import.meta` transformer.
/// * `Unsupported` is an error, reported to the caller as a message to throw.
fn module_text(specifier: &str, source: &str, ty: &DeclaredType) -> Result<String, String> {
    match ty {
        DeclaredType::Json => {
            if serde_json::from_str::<serde_json::Value>(source).is_err() {
                return Err(format!(
                    "module '{specifier}' is not valid JSON (imported with {{ type: 'json' }})"
                ));
            }
            // serde's string escaping produces a valid JS string literal.
            let literal = serde_json::to_string(source)
                .map_err(|e| format!("module '{specifier}': cannot embed JSON ({e})"))?;
            Ok(format!("export default JSON.parse({literal});"))
        }
        DeclaredType::Unsupported(t) => Err(format!(
            "module '{specifier}': unsupported import attribute type '{t}'"
        )),
        DeclaredType::Js => {
            Ok(transform_import_meta(source, specifier).unwrap_or_else(|| source.to_owned()))
        }
    }
}

/// Module-map key for `specifier` imported as `ty`.
///
/// The ECMAScript module map is keyed by *(specifier, type)*, not by specifier
/// alone: the same URL imported plainly and `with { type: 'json' }` yields two
/// different modules (a JS module and a synthetic JSON one). Keeping them apart
/// costs one suffix; sharing one entry would hand the second importer whichever
/// form happened to compile first.
fn cache_key(specifier: &str, ty: &DeclaredType) -> String {
    match ty {
        // NUL can't appear in a URL or a bare specifier, so the suffix cannot
        // collide with a real specifier.
        DeclaredType::Json => format!("{specifier}\u{0}json"),
        _ => specifier.to_owned(),
    }
}

/// Compile `text` as an ES module named `specifier` and remember it in the
/// module map under `key`, so a second import of the same specifier+type
/// reuses the instance.
///
/// `specifier` (not `key`) is what gets recorded for the module's identity
/// hash: it is the base URL relative imports from this module resolve against.
///
/// Returns `None` with a pending exception on the scope when compilation fails.
fn compile<'s>(
    scope: &v8::PinScope<'s, '_>,
    key: &str,
    specifier: &str,
    text: &str,
) -> Option<v8::Local<'s, v8::Module>> {
    let name = v8::String::new(scope, specifier)?;
    let src = v8::String::new(scope, text)?;
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
        0,     // line offset
        0,     // column offset
        false, // is_shared_cross_origin
        -1,    // script id: let V8 assign
        None,  // source map url
        false, // is_opaque
        false, // is_wasm
        true,  // is_module
        None,  // host defined options
    );
    let mut source = v8::script_compiler::Source::new(src, Some(&origin));
    let module = v8::script_compiler::compile_module(scope, &mut source)?;
    let hash = module.get_identity_hash().get();
    let global = v8::Global::new(scope, module);
    with_state(|s| {
        s.modules.insert(key.to_owned(), global);
        s.specifier_by_hash.insert(hash, specifier.to_owned());
    });
    Some(module)
}

/// Fetch `specifier` from the module map, compiling it from the registered
/// source on first use. `None` means an exception was thrown on `scope`.
fn module_for<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    specifier: &str,
    ty: &DeclaredType,
) -> Option<v8::Local<'s, v8::Module>> {
    let key = cache_key(specifier, ty);
    if let Some(existing) = with_state(|s| s.modules.get(&key).cloned()) {
        return Some(v8::Local::new(scope, existing));
    }
    let Some(source) = with_state(|s| s.sources.get(specifier).cloned()) else {
        throw(scope, &format!("module '{specifier}' not found"));
        return None;
    };
    match module_text(specifier, &source, ty) {
        Ok(text) => compile(scope, &key, specifier, &text),
        Err(msg) => {
            throw(scope, &msg);
            None
        }
    }
}

/// Throw a JS `Error` carrying `message` on `scope`.
fn throw(scope: &mut v8::PinScope<'_, '_>, message: &str) {
    if let Some(msg) = v8::String::new(scope, message) {
        let exc = v8::Exception::error(scope, msg);
        scope.throw_exception(exc);
    }
}

// ── Callbacks ─────────────────────────────────────────────────────────────────

/// V8 `ResolveModuleCallback`: map one static `import` request to a module.
///
/// Captureless by ABI, so all state comes from the thread-local registry.
fn resolve_module_callback<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    import_attributes: v8::Local<'s, v8::FixedArray>,
    referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    v8::callback_scope!(unsafe scope, context);

    let raw = specifier.to_rust_string_lossy(scope);
    let base = with_state(|s| {
        s.specifier_by_hash
            .get(&referrer.get_identity_hash().get())
            .cloned()
            .unwrap_or_default()
    });
    let resolved = resolve(&base, &raw);
    // `ResolveModuleCallback` receives [key, value, source-offset] per attribute.
    let ty = declared_type(scope, import_attributes, 3);
    module_for(scope, &resolved, &ty)
}

/// V8 `HostImportModuleDynamicallyCallback`: implement `import(specifier)`.
///
/// The embedder owns the whole load here — compile, instantiate, evaluate, then
/// settle the returned promise with the module namespace (or the error).
fn dynamic_import_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _host_defined_options: v8::Local<'s, v8::Data>,
    resource_name: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let resolver = v8::PromiseResolver::new(scope)?;
    let promise = resolver.get_promise(scope);

    let raw = specifier.to_rust_string_lossy(scope);
    // The importer's own specifier is V8's "resource name" — the script origin
    // we set in `compile()`, i.e. the resolved specifier of the referrer.
    let base = if resource_name.is_null_or_undefined() {
        String::new()
    } else {
        resource_name.to_rust_string_lossy(scope)
    };
    let resolved = resolve(&base, &raw);
    // The dynamic-import array is [key, value] per attribute (no source offset).
    let ty = declared_type(scope, import_attributes, 2);

    // Errors are reported by rejecting the promise, never by leaving an
    // exception pending: V8 requires a settled/settle-able promise back.
    v8::tc_scope!(let tc, scope);
    let outcome = load_and_evaluate(tc, &resolved, &ty);
    match outcome {
        Ok(namespace) => {
            resolver.resolve(tc, namespace);
        }
        Err(()) => {
            let exc = tc
                .exception()
                .unwrap_or_else(|| v8::undefined(tc).into());
            resolver.reject(tc, exc);
        }
    }
    Some(promise)
}

/// Compile → instantiate → evaluate `specifier`, returning its namespace object.
///
/// `Err(())` means an exception is pending on `scope`.
fn load_and_evaluate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    specifier: &str,
    ty: &DeclaredType,
) -> Result<v8::Local<'s, v8::Value>, ()> {
    let module = module_for(scope, specifier, ty).ok_or(())?;
    if module.get_status() == v8::ModuleStatus::Uninstantiated
        && module.instantiate_module(scope, resolve_module_callback).is_none()
    {
        return Err(());
    }
    if module.get_status() == v8::ModuleStatus::Instantiated
        && module.evaluate(scope).is_none()
    {
        return Err(());
    }
    if module.get_status() == v8::ModuleStatus::Errored {
        let exc = module.get_exception();
        scope.throw_exception(exc);
        return Err(());
    }
    Ok(module.get_module_namespace())
}

// ── Entry points used by `V8JsRuntime` ────────────────────────────────────────

/// Install the isolate-wide dynamic-`import()` hook.
///
/// Called once per isolate from the V8 thread bootstrap; static imports need no
/// isolate-level hook (the callback is passed to `instantiate_module`).
pub(crate) fn install_dynamic_import_hook(isolate: &mut v8::Isolate) {
    isolate.set_host_import_module_dynamically_callback(dynamic_import_callback);
}

/// Evaluate `source` as the entry ES module of an inline `<script type=module>`.
///
/// The page URL (stored by [`set_page_url`]) becomes `import.meta.url` when
/// known, matching `QuickJsRuntime::eval_module`.
///
/// `Err(())` means an exception is pending — the caller owns the `TryCatch` and
/// turns it into a [`JsError`], so the message matches every other V8 entry
/// point instead of being re-derived here.
pub(crate) fn evaluate_entry_module(
    scope: &mut v8::PinScope<'_, '_>,
    source: &str,
) -> Result<(), ()> {
    let specifier = register_inline(source);
    // `import.meta.url` is the page URL for inline modules, which have no URL
    // of their own; fall back to the virtual specifier on a blank page.
    let meta_url = with_state(|s| {
        if s.page_url.is_empty() {
            specifier.clone()
        } else {
            s.page_url.clone()
        }
    });
    let text = transform_import_meta(source, &meta_url).unwrap_or_else(|| source.to_owned());

    let module = compile(scope, &specifier, &specifier, &text).ok_or(())?;
    if module.instantiate_module(scope, resolve_module_callback).is_none() {
        return Err(());
    }
    if module.evaluate(scope).is_none() {
        return Err(());
    }
    if module.get_status() == v8::ModuleStatus::Errored {
        let exc = module.get_exception();
        scope.throw_exception(exc);
        return Err(());
    }
    // Drain Promise continuations from dynamic import() / top-level await,
    // mirroring the `execute_pending_job` loop on the QuickJS path.
    scope.perform_microtask_checkpoint();
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use crate::esm::ImportMap;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Fresh runtime per test: every `V8JsRuntime` owns its JS thread, so the
    /// thread-local module registry starts empty and cannot leak between tests.
    fn rt() -> V8JsRuntime {
        V8JsRuntime::new().unwrap()
    }

    // ── Core module semantics (BUG-350: none of this worked before S12b-23) ──

    #[test]
    fn v8_eval_module_simple_export() {
        // `export` at top level is a parse error for a classic script — this is
        // the exact shape that used to fail through the trait default.
        assert!(rt().eval_module("export const x = 42;").is_ok());
    }

    #[test]
    fn v8_eval_module_side_effects_visible() {
        let rt = rt();
        rt.eval_module("globalThis.__esm_side_effect__ = 'hello';").unwrap();
        let val = rt.eval("globalThis.__esm_side_effect__").unwrap();
        assert_eq!(val, JsValue::String("hello".into()));
    }

    #[test]
    fn v8_eval_module_imports_registered_module() {
        let rt = rt();
        rt.register_module_source("mylib", "export const answer = 42;");
        rt.eval_module("import { answer } from 'mylib'; globalThis.__answer__ = answer;")
            .unwrap();
        assert_eq!(rt.eval("globalThis.__answer__").unwrap(), JsValue::Number(42.0));
    }

    #[test]
    fn v8_eval_module_syntax_error_returns_error() {
        assert!(rt().eval_module("this is not valid JS @@@@").is_err());
    }

    #[test]
    fn v8_eval_module_missing_import_returns_error() {
        // Unregistered specifier: the resolve callback throws, instantiation
        // fails, and the error must reach the caller instead of being swallowed.
        let rt = rt();
        assert!(rt.eval_module("import x from 'nope'; globalThis.__x = x;").is_err());
    }

    #[test]
    fn v8_eval_module_runtime_error_returns_error() {
        // Module body throws: `evaluate` leaves the module Errored, and that
        // must not be reported as success.
        assert!(rt().eval_module("throw new Error('boom');").is_err());
    }

    #[test]
    fn v8_module_evaluated_once_for_diamond_graph() {
        // ESM module-map semantics: two importers share one instance, so the
        // side effect in `counted` runs exactly once.
        let rt = rt();
        rt.register_module_source(
            "counted",
            "globalThis.__count = (globalThis.__count || 0) + 1; export const n = 1;",
        );
        rt.register_module_source("a", "import { n } from 'counted'; export const a = n;");
        rt.register_module_source("b", "import { n } from 'counted'; export const b = n;");
        rt.eval_module("import { a } from 'a'; import { b } from 'b'; globalThis.__ab = a + b;")
            .unwrap();
        assert_eq!(rt.eval("globalThis.__count").unwrap(), JsValue::Number(1.0));
        assert_eq!(rt.eval("globalThis.__ab").unwrap(), JsValue::Number(2.0));
    }

    #[test]
    fn v8_top_level_await_drains_before_return() {
        // `eval_module` performs a microtask checkpoint, mirroring the
        // `execute_pending_job` loop on the QuickJS path.
        let rt = rt();
        rt.eval_module("const v = await Promise.resolve(7); globalThis.__tla = v;")
            .unwrap();
        assert_eq!(rt.eval("globalThis.__tla").unwrap(), JsValue::Number(7.0));
    }

    #[test]
    fn v8_dynamic_import_resolves() {
        let rt = rt();
        rt.register_module_source("dynmod", "export const v = 'dynamic';");
        rt.eval_module("import('dynmod').then(m => { globalThis.__dyn = m.v; });")
            .unwrap();
        assert_eq!(rt.eval("globalThis.__dyn").unwrap(), JsValue::String("dynamic".into()));
    }

    #[test]
    fn v8_dynamic_import_of_missing_module_rejects() {
        // The host callback must reject the promise, never leave an exception
        // pending — V8 requires a settle-able promise back from the hook.
        let rt = rt();
        rt.eval_module(
            "import('gone').then(() => { globalThis.__d = 'resolved'; },
                                 () => { globalThis.__d = 'rejected'; });",
        )
        .unwrap();
        assert_eq!(rt.eval("globalThis.__d").unwrap(), JsValue::String("rejected".into()));
    }

    // ── Specifier resolution ─────────────────────────────────────────────────

    #[test]
    fn v8_relative_import_resolves_against_importer() {
        let rt = rt();
        rt.register_module_source("https://example.com/app/util.js", "export const u = 'util';");
        rt.register_module_source(
            "https://example.com/app/main.js",
            "import { u } from './util.js'; export const m = u;",
        );
        rt.eval_module(
            "import { m } from 'https://example.com/app/main.js'; globalThis.__rel = m;",
        )
        .unwrap();
        assert_eq!(rt.eval("globalThis.__rel").unwrap(), JsValue::String("util".into()));
    }

    #[test]
    fn v8_import_map_maps_bare_specifier() {
        let rt = rt();
        let map = ImportMap::parse(r#"{ "imports": { "react": "/vendor/react.js" } }"#).unwrap();
        rt.set_import_map(map);
        rt.register_module_source("/vendor/react.js", "export const name = 'react';");
        rt.eval_module("import { name } from 'react'; globalThis.__lib = name;").unwrap();
        assert_eq!(rt.eval("globalThis.__lib").unwrap(), JsValue::String("react".into()));
    }

    // ── Import attributes (TC39 Stage 3) ─────────────────────────────────────

    #[test]
    fn v8_json_module_import() {
        let rt = rt();
        rt.register_module_source("cfgmod", r#"{ "answer": 42, "name": "lumen" }"#);
        rt.eval_module(
            "import cfg from 'cfgmod' with { type: 'json' };\n\
             globalThis.__aa_answer = cfg.answer;\n\
             globalThis.__aa_name = cfg.name;",
        )
        .unwrap();
        assert_eq!(rt.eval("globalThis.__aa_answer").unwrap(), JsValue::Number(42.0));
        assert_eq!(rt.eval("globalThis.__aa_name").unwrap(), JsValue::String("lumen".into()));
    }

    #[test]
    fn v8_invalid_json_module_fails_to_load() {
        let rt = rt();
        rt.register_module_source("badjson", "export const not_json = 1;");
        let result =
            rt.eval_module("import x from 'badjson' with { type: 'json' }; globalThis.__x = x;");
        assert!(result.is_err(), "invalid JSON must fail the import");
    }

    #[test]
    fn v8_unsupported_attribute_type_fails_to_load() {
        let rt = rt();
        rt.register_module_source("styles", "body { color: red; }");
        let result = rt.eval_module("import s from 'styles' with { type: 'css' };");
        assert!(result.is_err(), "unsupported attribute type must fail the import");
    }

    #[test]
    fn v8_same_specifier_as_json_and_js_are_distinct_modules() {
        // The module map is keyed by (specifier, type): the same source
        // imported plainly and `with { type: 'json' }` must not collapse into
        // whichever form compiled first.
        let rt = rt();
        rt.register_module_source("both", r#"{ "v": 3 }"#);
        // As JSON: a synthetic default export.
        rt.eval_module("import d from 'both' with { type: 'json' }; globalThis.__j = d.v;")
            .unwrap();
        assert_eq!(rt.eval("globalThis.__j").unwrap(), JsValue::Number(3.0));
        // As JS the very same text is a bare object literal — a valid module
        // body with no exports, so importing a binding from it must fail
        // rather than silently reuse the cached JSON module.
        assert!(
            rt.eval_module("import { v } from 'both'; globalThis.__k = v;").is_err(),
            "plain import must not reuse the JSON module's exports"
        );
    }

    #[test]
    fn v8_nested_json_attribute_in_registered_module() {
        // The attribute lives inside a *registered* module, not the inline
        // entry: V8 parses it there too, so no source preprocessing is needed.
        let rt = rt();
        rt.register_module_source("data", r#"{ "v": 7 }"#);
        rt.register_module_source(
            "wrapper",
            "import d from 'data' with { type: 'json' };\nexport const v = d.v;",
        );
        rt.eval_module("import { v } from 'wrapper'; globalThis.__aa_v = v;").unwrap();
        assert_eq!(rt.eval("globalThis.__aa_v").unwrap(), JsValue::Number(7.0));
    }

    // ── import.meta ──────────────────────────────────────────────────────────

    #[test]
    fn v8_import_meta_url_is_module_specifier() {
        let rt = rt();
        let specifier = "https://example.com/mymod.js";
        rt.register_module_source(specifier, "export const u = import.meta.url;");
        rt.eval_module(&format!("import {{ u }} from '{specifier}'; globalThis.__url = u;"))
            .unwrap();
        assert_eq!(rt.eval("globalThis.__url").unwrap(), JsValue::String(specifier.into()));
    }

    #[test]
    fn v8_import_meta_resolve_relative() {
        let rt = rt();
        let specifier = "https://example.com/app/main.js";
        rt.register_module_source(specifier, "export const r = import.meta.resolve('./utils.js');");
        rt.eval_module(&format!("import {{ r }} from '{specifier}'; globalThis.__res = r;"))
            .unwrap();
        assert_eq!(
            rt.eval("globalThis.__res").unwrap(),
            JsValue::String("https://example.com/app/./utils.js".into())
        );
    }

    #[test]
    fn v8_import_meta_env_mode_is_production() {
        let rt = rt();
        let specifier = "https://example.com/env.js";
        rt.register_module_source(specifier, "export const mode = import.meta.env.MODE;");
        rt.eval_module(&format!("import {{ mode }} from '{specifier}'; globalThis.__mode = mode;"))
            .unwrap();
        assert_eq!(
            rt.eval("globalThis.__mode").unwrap(),
            JsValue::String("production".into())
        );
    }
}
