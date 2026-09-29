//! Sealing pass for the engine's internal global names (BUG-378).
//!
//! Two mechanisms put engine internals on the page's global object:
//!
//! * natives registered from Rust — [`crate::v8_runtime::V8JsRuntime::register_native`]
//!   and the `reg!` macros in `v8_runtime.rs`, all of which bottom out in
//!   [`crate::v8_compat::register_v8_native`];
//! * top-level `var`/`function _lumen_…` declarations inside
//!   [`crate::dom::WEB_API_SHIM`] and the ~120 per-API module shims, which by
//!   ECMAScript semantics become properties of the global object.
//!
//! Both used to produce properties with the default attributes
//! (`writable`/`enumerable`/`configurable` = `true`), so a plain local page
//! measured 604 internal names among the global object's 1308 own properties,
//! 592 of them reachable by an ordinary `for (k in window)`. Worse, the shim
//! resolves natives *late* — `_lumen_get_attr(nid, name)` is an ordinary scope
//! lookup that ends at the global object — so assigning
//! `window._lumen_get_attr = () => 'HIJACKED'` re-pointed the bottom layer of
//! the DOM for every other script on the page and for automation evaluating in
//! the same context.
//!
//! [`seal_internal_globals_v8`] closes both halves at one point, at the end of
//! `install_dom` once every native and every shim has been installed:
//!
//! * every internal name becomes non-enumerable, so `for (k in window)` and
//!   `Object.keys(window)` no longer see engine internals;
//! * function-valued internals additionally become non-writable and
//!   non-configurable, which is what kills the hijack: the natives and the
//!   shim's own wrappers around them (`_lumen_set_attr`, `_lumen_append_child`,
//!   … — installed during the shim eval, i.e. before this pass) can no longer
//!   be replaced by page script.
//!
//! Non-function internals (engine *state*: `_lumen_timers`,
//! `_lumen_loc_parts`, `_lumen_last_focused_nid`, `_LUMEN_PAGE_URL`, …) keep
//! their writability on purpose — the shim assigns to them at runtime, long
//! after this pass, and freezing them would make those writes fail silently in
//! the shim's sloppy-mode code. They are only hidden from enumeration.
//!
//! **BUG-753 срез 2 — the container.** Most of what the paragraph above describes
//! is now moot: internal names no longer live on the global at all. Every context
//! owns a null-prototype *container* object ([`install_container`]) hung off the
//! global under a V8 private symbol. Natives registered from Rust
//! (`register_v8_native`) go there, the page shim ([`wrap_page_shim`]) exports its
//! `_`-prefixed / `__` / `_lumen…` bindings there (early-bound locals plus live
//! accessors), and every *internal* `eval` runs as `with (container) { … }`
//! ([`wrap_for_container`]) so module shims, Rust-side snippets and tests resolve
//! bare internal names against it. The page-script boundary
//! (`eval_and_report*`, module entry points) is not wrapped, so page script sees
//! neither `window._lumen_x` nor a bare `_lumen_x`. This sealing pass stays for
//! what still lands on the global: an unmigrated module shim's own top-level
//! declaration, and `_lumen_import_meta_resolve` (module code reaches it through
//! the `import.meta` preamble; the census test pins that it is the only one).
//! Срез 3 removes those two, plus the `with` cost on shims that never needed it.
//!
//! Precedent: [`crate::file_input::seal_file_natives_v8`] (BUG-371) does the
//! stronger thing — outright `delete` — for the file-API natives, which is
//! possible only because those two shims copy the natives into closure
//! variables at install time. The rest of the engine resolves them late, hence
//! the weaker but universally applicable treatment here.

/// Wrap the assembled page shim into a function scope (BUG-753, срез 1).
///
/// The shim's ~1000 top-level `var`/`function` names stop being global-object
/// properties: they become locals of one IIFE, so the shim's own calls bind
/// early (to the local) instead of walking the scope chain to the global. The
/// result is a function *expression*, called with the context's internal
/// container as `__lumen_C` (срез 2) — the prologue/epilogue publish the names:
///
/// * public names (`document`, `Element`, `fetch`, …) — a plain
///   `writable`/`enumerable`/`configurable` data property on the global, the
///   attributes the old indirect-eval `var` binding had;
/// * internal and `_`-prefixed names ([`is_accessor_export`]) — a non-enumerable
///   accessor **on the container** over the local binding, so late writes by
///   module shims / Rust (`_lumen_x = …`, run under `with (container)`) still
///   reach the variable the shim itself reads;
/// * natives already on the container are bound to locals up front, and
///   internal names the body only *references* get a local plus an accessor, so
///   a native a later `install_*` registers reaches the local through the setter.
///
/// The names are found by scanning column-0 `function NAME` / `var NAME[, …]`
/// lines — the shim's formatting invariant, guarded by
/// `shim_exports_every_old_global`.
#[cfg(feature = "v8-backend")]
pub(crate) fn wrap_page_shim(body: &str, natives: &[String]) -> String {
    let names = top_level_names(body);
    let declared: std::collections::HashSet<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
    let mut out = String::with_capacity(body.len() + names.len() * 160 + natives.len() * 40 + 1024);
    out.push_str("(function(__lumen_C) {\n");
    // Prologue: the global properties exist *before* the body runs, as they did
    // for eval-`var`s — the body itself calls `Object.defineProperty(globalThis,
    // 'X', { enumerable: false })` on its own names and reads `window.X` while
    // loading. Function declarations are hoisted, so their values are already
    // final here; `var`s start as `undefined` and are filled in by the epilogue.
    out.push_str(
        "var __lumen_x = function(n, g, s) { try { Object.defineProperty(__lumen_C, n, \
         { get: g, set: s, enumerable: false, configurable: true }); } catch (e) {} };\n\
         var __lumen_p = function(n, v) { try { Object.defineProperty(globalThis, n, \
         { value: v, writable: true, enumerable: true, configurable: true }); } catch (e) {} };\n\
         var __lumen_v = function(n) { try { if (!Object.getOwnPropertyDescriptor(globalThis, n)) \
         Object.defineProperty(globalThis, n, { value: void 0, writable: true, enumerable: true, \
         configurable: true }); } catch (e) {} };\n\
         var __lumen_f = function(n, v) { try { var d = Object.getOwnPropertyDescriptor(globalThis, n); \
         if (d && v !== undefined && 'value' in d) Object.defineProperty(globalThis, n, { value: v }); } \
         catch (e) {} };\n",
    );
    // Early binding of the natives already on the container (срез 2): the shim
    // reads them from a local instead of walking the scope chain.
    let mut bound: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for n in natives {
        if declared.contains(n.as_str()) || n == "__lumen_C" || !is_identifier(n) {
            continue;
        }
        if bound.insert(n.as_str()) {
            out.push_str(&format!("var {n} = __lumen_C.{n};\n"));
        }
    }
    // Names the shim references but neither declares nor finds on the container
    // yet — natives that a later `install_*` registers. A local plus a live
    // accessor on the container: the later `container.name = fn` reaches the
    // local through the setter.
    for n in referenced_internal_names(body) {
        if declared.contains(n.as_str()) || bound.contains(n.as_str()) || n == "__lumen_C" {
            continue;
        }
        out.push_str(&format!("var {n};\n"));
        out.push_str(&format!(
            "__lumen_x(\"{n}\", function() {{ return {n}; }}, function(v) {{ {n} = v; }});\n"
        ));
    }
    for (n, is_fn) in &names {
        if is_accessor_export(n) {
            out.push_str(&format!(
                "__lumen_x(\"{n}\", function() {{ return {n}; }}, function(v) {{ {n} = v; }});\n"
            ));
        } else if *is_fn {
            out.push_str(&format!("__lumen_p(\"{n}\", {n});\n"));
        } else {
            out.push_str(&format!("__lumen_v(\"{n}\");\n"));
        }
    }
    out.push_str(body);
    // Epilogue: publish the final values of public `var`s.
    out.push_str("\n;\n");
    for (n, is_fn) in &names {
        if !is_accessor_export(n) && !*is_fn {
            out.push_str(&format!("__lumen_f(\"{n}\", typeof {n} === 'undefined' ? void 0 : {n});\n"));
        }
    }
    out.push_str("})\n");
    out
}

#[cfg(feature = "v8-backend")]
fn is_identifier(n: &str) -> bool {
    let mut it = n.chars();
    it.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Identifier tokens of `body` that are engine-internal names by
/// [`is_internal_name`] and not member accesses (`x._lumen_name`).
#[cfg(feature = "v8-backend")]
fn referenced_names(body: &str) -> impl Iterator<Item = &str> {
    let b = body.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                    i += 1;
                }
                let prev = start.checked_sub(1).map(|k| b[k]);
                if prev != Some(b'.') {
                    return Some(&body[start..i]);
                }
            } else {
                i += 1;
            }
        }
        None
    })
}

#[cfg(feature = "v8-backend")]
fn referenced_internal_names(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                i += 1;
            }
            let tok = &body[start..i];
            let prev = start.checked_sub(1).map(|k| b[k]);
            if prev != Some(b'.') && is_internal_name(tok) {
                seen.insert(tok.to_string());
            }
        } else {
            i += 1;
        }
    }
    seen.into_iter().collect()
}

/// Names re-exported as live accessors: the engine-internal ones plus every
/// other single-underscore name (`_wa_current_time`, `_details_known_open`) —
/// shim state that tests and module shims write to from outside the IIFE.
#[cfg(feature = "v8-backend")]
pub(crate) fn is_accessor_export(n: &str) -> bool {
    n.starts_with('_') || is_internal_name(n)
}

/// Same predicate as the `INTERNAL` regexp of [`SEAL_INTERNAL_GLOBALS`]:
/// `^__` or `^_+lumen` (case-insensitive).
#[cfg(feature = "v8-backend")]
pub(crate) fn is_internal_name(n: &str) -> bool {
    n.starts_with("__") || n.trim_start_matches('_').get(..5).is_some_and(|p| p.eq_ignore_ascii_case("lumen")) && n.starts_with('_')
}

/// Column-0 `function NAME(` and `var NAME[ = …][, NAME2 …];` declarations,
/// as `(name, is_function_declaration)`.
#[cfg(feature = "v8-backend")]
fn top_level_names(body: &str) -> Vec<(String, bool)> {
    fn ident(s: &str) -> Option<&str> {
        let end = s
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
            .unwrap_or(s.len());
        (end > 0 && !s.as_bytes()[0].is_ascii_digit()).then(|| &s[..end])
    }
    /// Declarator heads of one `var` line: split at depth-0 commas, ignoring
    /// anything inside brackets or quotes.
    fn declarators(rest: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let (mut depth, mut quote, mut start, mut end) = (0i32, None::<char>, 0usize, rest.len());
        let mut prev = ' ';
        for (i, c) in rest.char_indices() {
            match quote {
                Some(q) => {
                    if c == q && prev != '\\' {
                        quote = None;
                    }
                }
                None => match c {
                    '/' if rest[i..].starts_with("//") => {
                        end = i;
                        break;
                    }
                    '\'' | '"' | '`' => quote = Some(c),
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    ',' if depth == 0 => {
                        out.push(&rest[start..i]);
                        start = i + 1;
                    }
                    _ => {}
                },
            }
            prev = c;
        }
        out.push(&rest[start..end]);
        out
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut names = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("function ") {
            if let Some(n) = ident(rest.trim_start())
                && seen.insert(n.to_string())
            {
                names.push((n.to_string(), true));
            }
        } else if let Some(rest) = line.strip_prefix("var ") {
            for d in declarators(rest) {
                if let Some(n) = ident(d.trim())
                    && seen.insert(n.to_string())
                {
                    names.push((n.to_string(), false));
                }
            }
        }
    }
    names
}

/// The per-context internal container (BUG-753 срез 2): a null-prototype object
/// that holds the engine's internal names — natives registered from Rust and the
/// page shim's `_`-prefixed / `__` / `_lumen…` bindings — instead of the global
/// object. It hangs off the global under a V8 *private* symbol, so no script can
/// reach it by name; engine code reaches it through the scope-extension
/// [`wrap_for_container`] puts around every internal `eval` (`with (container)`),
/// and the page shim through its `__lumen_C` parameter.
#[cfg(feature = "v8-backend")]
fn container_private<'s>(scope: &v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "lumen#internal-container")?;
    Some(v8::Private::for_api(scope, Some(name)))
}

/// The container of `ctx`, if [`install_container`] has run for it.
#[cfg(feature = "v8-backend")]
pub(crate) fn container<'s>(
    scope: &v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Object>> {
    let key = container_private(scope)?;
    let value = ctx.global(scope).get_private(scope, key)?;
    v8::Local::<v8::Object>::try_from(value).ok()
}

/// Create the container of `ctx` (idempotent). It carries a reference to itself
/// as `__lumen_C`, so shim code running under `with (container)` can create
/// *new* internal names (`__lumen_C._x = …`) without leaking them onto the global.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_container<'s>(
    scope: &v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Object>> {
    if let Some(c) = container(scope, ctx) {
        return Some(c);
    }
    let key = container_private(scope)?;
    let null = v8::null(scope).into();
    let c = v8::Object::with_prototype_and_properties(scope, null, &[], &[]);
    let self_key = v8::String::new(scope, "__lumen_C")?;
    c.set(scope, self_key.into(), c.into());
    ctx.global(scope).set_private(scope, key, c.into());
    Some(c)
}

/// Object that owns `name` for reads/writes coming from Rust: the container for
/// an internal name (when the context has one), the global object otherwise.
#[cfg(feature = "v8-backend")]
pub(crate) fn holder_for<'s>(
    scope: &v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Object> {
    let global = ctx.global(scope);
    if is_accessor_export(name)
        && let Some(c) = container(scope, ctx)
    {
        // Names a shim still parks on the global (`globalThis._x = …`, an
        // unmigrated module shim's top-level declaration) stay reachable there
        // until срез 3 moves them; everything else is the container's.
        let has = |o: v8::Local<'s, v8::Object>, own: bool| {
            v8::String::new(scope, name)
                .and_then(|k| if own { o.has_own_property(scope, k.into()) } else { o.has(scope, k.into()) })
                .unwrap_or(false)
        };
        if has(c, false) || (is_internal_name(name) && !has(global, true)) {
            return c;
        }
    }
    global
}

/// Text prefix / suffix of an internal `eval`: `with (container) { … }`. The
/// container is handed over through a one-shot global that the prefix reads and
/// deletes before the script body runs, so the script never observes it.
#[cfg(feature = "v8-backend")]
pub(crate) const WITH_PREFIX: &str = "with ((function(){var c=globalThis.__lumen_c_tmp;delete globalThis.__lumen_c_tmp;return c})()) {";

/// How [`wrap_for_container`] must treat a script. `None` — leave it alone:
/// it opens with a `'use strict'` directive (the wrapper would silently drop it
/// — `with` is illegal in strict code) or declares a top-level `let`/`const`
/// (block-scoped inside the wrapper instead of living in the global lexical
/// scope, so a later `eval` would no longer see it). `Some(classes)` — wrap it;
/// the listed top-level `class` names are re-published on the global after the
/// block (`class X {}` is block-scoped there too, and other scripts expect the
/// name to stay reachable).
#[cfg(feature = "v8-backend")]
enum WrapPlan {
    /// `with (container) { … }` around the source; top-level classes re-published.
    Block(Vec<String>),
    /// `with (container) { (function() { 'use strict'; … }).call(globalThis) }`.
    StrictFn,
}

#[cfg(feature = "v8-backend")]
fn wrap_plan(script: &str) -> Option<WrapPlan> {
    let t = script.trim_start();
    if t.starts_with("'use strict'") || t.starts_with("\"use strict\"") {
        // A strict script cannot sit inside `with`; run its body in a strict
        // function under the `with` instead — unless it declares top-level names
        // that must stay global, which the function would swallow.
        let declares = script.lines().any(|l| {
            ["function ", "async function ", "var ", "class ", "let ", "const "]
                .iter()
                .any(|k| l.starts_with(k))
        });
        return (!declares).then_some(WrapPlan::StrictFn);
    }
    let b = script.as_bytes();
    let mut classes = Vec::new();
    let (mut depth, mut i, mut stmt_start) = (0i32, 0usize, true);
    while i < b.len() {
        let c = b[i];
        match c {
            b'\'' | b'"' | b'`' => {
                i += 1;
                while i < b.len() && b[i] != c {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                stmt_start = false;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'{' | b'(' | b'[' => {
                depth += 1;
                stmt_start = false;
            }
            b'}' | b')' | b']' => {
                depth -= 1;
                stmt_start = c == b'}' && depth == 0;
            }
            b';' => stmt_start = depth == 0,
            b'\n' => stmt_start = stmt_start || depth == 0,
            c if c.is_ascii_whitespace() => {}
            c if depth == 0 && stmt_start && c.is_ascii_alphabetic() => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                    i += 1;
                }
                let word = &script[start..i];
                if matches!(word, "let" | "const") && b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
                    return None;
                }
                if word == "class" && b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
                    let rest = script[i..].trim_start();
                    let end = rest
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                        .unwrap_or(rest.len());
                    if end > 0 {
                        classes.push(rest[..end].to_string());
                    }
                }
                stmt_start = false;
                continue;
            }
            _ => stmt_start = false,
        }
        i += 1;
    }
    Some(WrapPlan::Block(classes))
}

/// Wrap `script` so its bare identifiers resolve against the container first.
/// `None` when the context has no container (the script then runs as-is).
#[cfg(feature = "v8-backend")]
pub(crate) fn wrap_for_container<'s>(
    scope: &v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Context>,
    script: &str,
) -> Option<String> {
    // Nothing in the script can resolve to the container: run it exactly as
    // written (keeps `let`/`const`/strict semantics, columns and code-cache keys).
    if !referenced_names(script).any(is_accessor_export) {
        return None;
    }
    let plan = wrap_plan(script)?;
    let c = container(scope, ctx)?;
    let key = v8::String::new(scope, "__lumen_c_tmp")?;
    ctx.global(scope).set(scope, key.into(), c.into());
    Some(with_plan_text(script, &plan))
}

/// The wrapped source text [`wrap_for_container`] compiles.
#[cfg(feature = "v8-backend")]
fn with_plan_text(script: &str, plan: &WrapPlan) -> String {
    match plan {
        WrapPlan::Block(classes) => with_container_text(script, classes),
        WrapPlan::StrictFn => {
            let mut out = String::with_capacity(script.len() + WITH_PREFIX.len() + 40);
            out.push_str(WITH_PREFIX);
            out.push_str("(function() {");
            out.push_str(script);
            out.push_str("
}).call(globalThis);}");
            out
        }
    }
}

/// [`WrapPlan::Block`] text.
#[cfg(feature = "v8-backend")]
pub(crate) fn with_container_text(script: &str, classes: &[String]) -> String {
    let mut out = String::with_capacity(script.len() + WITH_PREFIX.len() + 4);
    out.push_str(WITH_PREFIX);
    // Re-publication of top-level classes goes *before* the body — a trailing
    // statement would replace the script's completion value. A live accessor
    // over the block binding: the global name resolves to the class once its
    // declaration has run, exactly when a global lexical binding would.
    for n in classes {
        out.push_str(&format!(
            "Object.defineProperty(globalThis,\"{n}\",{{get:function(){{return {n}}},             set:function(v){{Object.defineProperty(globalThis,\"{n}\",{{value:v,writable:true,             configurable:true}})}},enumerable:false,configurable:true}});"
        ));
    }
    out.push_str(script);
    out.push_str("
}");
    out
}

#[cfg(feature = "v8-backend")]
use lumen_core::JsResult;

/// Hide every internal global name from enumeration and freeze the
/// function-valued ones (BUG-378).
///
/// Called at the tail of [`crate::v8_runtime::V8JsRuntime::install_dom`], after
/// every native registration and every shim eval — see the module docs for why
/// this has to be the last install step and why non-function internals stay
/// writable.
#[cfg(feature = "v8-backend")]
pub(crate) fn seal_internal_globals_v8(rt: &crate::v8_runtime::V8JsRuntime) -> JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SEAL_INTERNAL_GLOBALS)?;
    Ok(())
}

/// The sealing pass itself.
///
/// `INTERNAL` matches the two shapes engine internals actually use: a
/// double-underscore prefix (`__lumen_platform_cloners`, `__dom_node_warned`)
/// and any number of leading underscores followed by `lumen` in any case
/// (`_lumen_get_attr`, `__lumen_fs_internal`, `_LUMEN_PAGE_URL`). Names that
/// are already non-configurable are skipped rather than redefined: the
/// automation-marker properties installed by `surface_api.rs` are accessors
/// with `configurable: false`, and `Object.defineProperty` on those throws
/// (see BUG-379 — that surface is a separate defect and is left untouched).
#[cfg(feature = "v8-backend")]
const SEAL_INTERNAL_GLOBALS: &str = r#"
(function() {
  'use strict';
  var INTERNAL = /^__|^_+lumen/i;
  var names = Object.getOwnPropertyNames(globalThis);
  for (var i = 0; i < names.length; i++) {
    var k = names[i];
    if (!INTERNAL.test(k)) continue;
    var d;
    try { d = Object.getOwnPropertyDescriptor(globalThis, k); } catch (e) { continue; }
    if (!d) continue;
    if (!d.configurable) {
      // A module shim's top-level `var`/`function` — those still run as plain
      // Scripts (only WEB_API_SHIM goes through indirect eval), so the binding
      // is non-configurable and `enumerable` cannot be flipped at all.
      // `writable: true → false` is the one transition the spec still allows
      // here, so at least take the hijack away. Same for the automation
      // markers of `surface_api.rs`, which are non-configurable accessors and
      // have nothing to change (BUG-379 owns that surface).
      if ('value' in d && d.writable === true && typeof d.value === 'function') {
        try { Object.defineProperty(globalThis, k, { writable: false }); } catch (e) {}
      }
      continue;
    }
    try {
      if ('value' in d) {
        // Functions — natives and the shim's wrappers around them — are the
        // hijack surface: freeze them. State stays writable, because the shim
        // assigns to it long after this pass has run; it is still hidden and
        // made non-deletable (nothing deletes an internal global after the
        // install sequence — the shims that do, do it at install time).
        var lock = (typeof d.value === 'function');
        Object.defineProperty(globalThis, k, {
          value: d.value,
          writable: lock ? false : d.writable,
          enumerable: false,
          configurable: false
        });
      } else {
        // Accessor: the re-export of a shim-local binding (`wrap_page_shim`,
        // BUG-753). Hide it and make it permanent; a function-valued one also
        // loses its setter — a write from page script would re-point the
        // shim's own local binding, which is the hijack this pass exists to
        // prevent. State-valued ones keep the setter (module shims and Rust
        // assign to them at runtime).
        var setter = d.set;
        try {
          if (typeof d.get === 'function' && typeof d.get() === 'function') setter = undefined;
        } catch (e) {}
        Object.defineProperty(globalThis, k, {
          get: d.get,
          set: setter,
          enumerable: false,
          configurable: false
        });
      }
    } catch (e) { /* leave this one as it was rather than fail the pass */ }
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // `panic!` — штатный способ провалить тест; исключение из clippy.toml не
    // достаёт до хелперов модуля (docs/lint-policy.md §10).
    #![allow(clippy::panic, clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;
    use lumen_dom::{Document, NodeData, QualName};
    use std::sync::{Arc, Mutex};

    /// `<html><body><div id="main" data-x="orig"></div></body></html>`.
    fn make_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"),
                value: "main".into(),
            });
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("data-x"),
                value: "orig".into(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, div);
        Arc::new(Mutex::new(doc))
    }

    fn runtime() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
            .unwrap();
        rt
    }

    fn num(rt: &V8JsRuntime, script: &str) -> f64 {
        match rt.eval(script).unwrap() {
            JsValue::Number(n) => n,
            other => panic!("expected number from `{script}`, got {other:?}"),
        }
    }

    fn text(rt: &V8JsRuntime, script: &str) -> String {
        match rt.eval(script).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected string from `{script}`, got {other:?}"),
        }
    }

    fn truthy(rt: &V8JsRuntime, script: &str) -> bool {
        rt.eval(script).unwrap() == JsValue::Bool(true)
    }

    /// The headline census of BUG-378: no internal name may be reachable by an
    /// ordinary `for (k in window)` / `Object.keys(window)`.
    #[test]
    fn no_internal_name_is_enumerable() {
        let rt = runtime();
        let script = "var re = /^__|^_+lumen/i, n = 0; \
                      for (var k in window) { if (re.test(k)) n++; } n";
        let leaked = num(&rt, script);
        // Report the actual names, not just the count — a regression here is
        // otherwise a bare number with no lead.
        if leaked > 0.0 {
            let names = text(
                &rt,
                "var re = /^__|^_+lumen/i, a = []; \
                 for (var k in window) { if (re.test(k)) a.push(k); } a.slice(0, 20).join(',')",
            );
            panic!("{leaked} internal names still enumerable, first 20: {names}");
        }
        assert_eq!(num(&rt, "Object.keys(window).filter(function(k) { \
                             return /^__|^_+lumen/i.test(k); }).length"), 0.0);
    }

    /// Page-script view: `eval_and_report` is the top-level `<script>` boundary and
    /// runs without the internal container in scope.
    fn page(rt: &V8JsRuntime, script: &str) -> JsValue {
        rt.eval_and_report(script).unwrap()
    }

    fn page_truthy(rt: &V8JsRuntime, script: &str) -> bool {
        page(rt, script) == JsValue::Bool(true)
    }

    /// Natives registered from Rust land on the internal container, not on the
    /// global: a runtime that only registered natives (no `install_dom`) already
    /// keeps them off the page-visible global, and the engine's own scope still
    /// resolves them.
    #[test]
    fn registered_native_is_not_on_the_global() {
        let rt = V8JsRuntime::new().unwrap();
        rt.install_console_natives(Arc::new(Mutex::new(Vec::new()))).unwrap();
        assert!(page_truthy(
            &rt,
            "Object.getOwnPropertyDescriptor(globalThis, '_lumen_console_log') === undefined              && typeof _lumen_console_log === 'undefined'"
        ));
        assert!(truthy(&rt, "typeof _lumen_console_log === 'function'"));
    }

    /// Sealing must not make the names unreachable *for the engine* — the shim
    /// resolves every native through the container.
    #[test]
    fn natives_stay_callable_after_sealing() {
        let rt = runtime();
        assert!(truthy(&rt, "typeof _lumen_get_attr === 'function'"));
        assert!(truthy(&rt, "typeof _lumen_set_attr === 'function'"));
        assert_eq!(text(&rt, "document.getElementById('main').getAttribute('data-x')"), "orig");
    }

    /// The headline of BUG-753: a page script that knows a name cannot read it —
    /// neither as a member of `window` nor as a bare identifier.
    #[test]
    fn page_script_cannot_read_internal_names() {
        let rt = runtime();
        for name in [
            "_lumen_get_attr",
            "_lumen_set_attr",
            "_lumen_query_selector_scoped",
            "_lumen_timers",
            "_lumen_loc_parts",
            "_lumen_tick_timers",
        ] {
            let probe = format!(
                "typeof window.{name} === 'undefined' && typeof {name} === 'undefined'                  && !('{name}' in window)                  && Object.getOwnPropertyNames(window).indexOf('{name}') < 0"
            );
            assert!(page_truthy(&rt, &probe), "{name} is visible to page script");
        }
    }

    /// The hijack from the bug report: `window._lumen_get_attr = …` must not
    /// re-point `Element.getAttribute` — the page's write lands on a global the
    /// shim never reads.
    #[test]
    fn native_cannot_be_hijacked_by_assignment() {
        let rt = runtime();
        page(&rt, "try { window._lumen_get_attr = function() { return 'HIJACKED'; }; } catch (e) {}");
        assert_eq!(
            text(&rt, "document.getElementById('main').getAttribute('data-x')"),
            "orig",
            "page script replaced the DOM's bottom layer"
        );
        // Not even a bare-name assignment or a delete reaches the container.
        page(&rt, "try { _lumen_set_attr = function() {}; delete window._lumen_get_attr; } catch (e) {}");
        rt.eval("document.getElementById('main').setAttribute('data-y', 'v')").unwrap();
        assert_eq!(text(&rt, "document.getElementById('main').getAttribute('data-y')"), "v");
        assert!(truthy(&rt, "typeof _lumen_get_attr === 'function'"));
    }

    /// Engine *state* is reachable and writable from the engine's scope: the shim
    /// assigns to it long after install, and `_lumen_tick_timers` rewrites
    /// `_lumen_timers` on every pump.
    #[test]
    fn engine_state_stays_writable() {
        let rt = runtime();
        for name in ["_lumen_timers", "_lumen_loc_parts", "_lumen_last_focused_nid"] {
            assert!(
                truthy(&rt, &format!("typeof {name} !== 'undefined'")),
                "{name} must be reachable from the engine's scope"
            );
        }
        rt.eval("var fired = 0; setTimeout(function() { fired++; }, 0);").unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        assert_eq!(num(&rt, "fired"), 1.0);
    }

    /// BUG-753 census: the only engine-internal own property of the page's global
    /// object is `_lumen_import_meta_resolve`, which module code reaches through
    /// the `import.meta` preamble (`crate::import_meta`) and so cannot move onto
    /// the container until `import.meta` is set up by a host callback (срез 3).
    /// Any other name showing up here is a regression.
    #[test]
    fn only_known_internal_names_remain_on_the_global() {
        let rt = runtime();
        let names = text(
            &rt,
            "Object.getOwnPropertyNames(globalThis).filter(function(n) {                return /^__|^_+lumen/i.test(n); }).sort().join(',')",
        );
        assert_eq!(names, "_lumen_import_meta_resolve");
    }

    /// `WEB_API_SHIM` is evaluated through indirect eval so its top-level
    /// declarations land on the global object as *configurable* properties (see
    /// the comment at its eval site in `v8_runtime.rs`). Lexical declarations
    /// behave differently under eval — they would go into a declarative
    /// environment that dies with the eval call and never reach the global
    /// object at all, so a top-level `let`/`const`/`class` added to the shim
    /// would silently disappear. There are none today; this guards that.
    #[test]
    fn shim_has_no_top_level_lexical_declarations() {
        // Assembled from its five parts (BUG-401) — a lexical declaration in
        // any of them, shared blocks included, is equally fatal here.
        let shim = crate::dom::web_api_shim();
        let offenders: Vec<&str> = shim
            .lines()
            .filter(|l| {
                l.starts_with("let ") || l.starts_with("const ") || l.starts_with("class ")
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "top-level lexical declarations in WEB_API_SHIM would not survive its \
             indirect-eval evaluation (BUG-378) — declare them with `var` instead: {offenders:?}"
        );
    }

    /// The name scan behind [`super::wrap_page_shim`]: column-0 `function` /
    /// `var` heads, comma lists, trailing `//` comments (a comma inside one
    /// once produced the bogus name `in`), nested and indented lines ignored.
    #[test]
    fn top_level_names_scan() {
        let src = "function a(x) { var inner = 1; }
var b = 1, c = [1, 2], d; // e, in queue
var s = 'x, y', f = function() {};
  var indented = 1;
let z = 1;
";
        let got: Vec<(String, bool)> = super::top_level_names(src);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d", "s", "f"]);
        assert!(got[0].1 && !got[1].1);
    }

    /// The IIFE keeps scratch `var`s of top-level loops off the global object,
    /// while the declared names stay reachable (BUG-753 срез 1).
    #[test]
    fn shim_scratch_vars_do_not_leak_to_global() {
        let rt = runtime();
        assert!(truthy(&rt, "typeof window._dohi === 'undefined' && typeof Element === 'function'"));
    }

    /// Ordinary web-visible globals must keep their normal, enumerable shape —
    /// the pass must not over-reach.
    #[test]
    fn web_visible_globals_untouched() {
        let rt = runtime();
        for name in ["document", "fetch", "setTimeout", "localStorage"] {
            assert!(
                truthy(&rt, &format!("typeof window.{name} !== 'undefined'")),
                "{name} disappeared"
            );
        }
        assert!(truthy(&rt, "Object.keys(window).length > 100"));
    }
}
