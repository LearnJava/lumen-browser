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
//! What this pass deliberately does **not** do: it cannot stop a script that
//! already *knows* a name from reading it (`typeof window._lumen_get_attr`),
//! because the ~120 module shims still call the page shim's names through the
//! global object. [BUG-753](../../../bugs/BUG-753-OPEN.md) removes the names in
//! slices: срез 1 ([`wrap_page_shim`]) put `WEB_API_SHIM` in an IIFE, so its own
//! calls bind to locals and only a re-export remains on the global; the
//! module shims move onto an internal container in срез 2, and срез 3 drops
//! the re-export.
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
/// epilogue then re-exports each name to the global so the ~120 module shims
/// and Rust-side `eval` snippets that still call them by bare name keep
/// working (срез 2 moves those consumers onto an internal container; срез 3
/// drops the re-export of internal names altogether):
///
/// * public names (`document`, `Element`, `fetch`, …) — a plain
///   `writable`/`enumerable`/`configurable` data property, the attributes the
///   old indirect-eval `var` binding had;
/// * internal and `_`-prefixed names ([`is_accessor_export`]) — a non-enumerable accessor over the
///   local binding, so late writes by module shims / Rust (`_lumen_x = …`)
///   still reach the variable the shim itself reads. Sealing (freezing the
///   function-valued ones) is done by the pass in this module.
///
/// The names are found by scanning column-0 `function NAME` / `var NAME[, …]`
/// lines — the shim's formatting invariant, guarded by
/// `shim_exports_every_old_global`.
#[cfg(feature = "v8-backend")]
pub(crate) fn wrap_page_shim(body: &str) -> String {
    let names = top_level_names(body);
    let mut out = String::with_capacity(body.len() + names.len() * 160 + 1024);
    out.push_str("(function() {\n");
    // Prologue: the global properties exist *before* the body runs, as they did
    // for eval-`var`s — the body itself calls `Object.defineProperty(globalThis,
    // 'X', { enumerable: false })` on its own names and reads `window.X` while
    // loading. Function declarations are hoisted, so their values are already
    // final here; `var`s start as `undefined` and are filled in by the epilogue.
    out.push_str(
        "var __lumen_x = function(n, g, s) { try { Object.defineProperty(globalThis, n, \
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
    out.push_str("})();\n");
    out
}

/// Names re-exported as live accessors: the engine-internal ones plus every
/// other single-underscore name (`_wa_current_time`, `_details_known_open`) —
/// shim state that tests and module shims write to from outside the IIFE.
#[cfg(feature = "v8-backend")]
fn is_accessor_export(n: &str) -> bool {
    n.starts_with('_') || is_internal_name(n)
}

/// Same predicate as the `INTERNAL` regexp of [`SEAL_INTERNAL_GLOBALS`]:
/// `^__` or `^_+lumen` (case-insensitive).
#[cfg(feature = "v8-backend")]
fn is_internal_name(n: &str) -> bool {
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

    /// The registration choke point itself hides natives, without waiting for
    /// the sealing pass: a runtime that only registered natives (no
    /// `install_dom`, so no seal) must already keep them out of `for..in`,
    /// while leaving them writable for the re-registrations and shim wrappers
    /// that happen during install.
    #[test]
    fn registered_native_is_hidden_but_still_patchable() {
        let rt = V8JsRuntime::new().unwrap();
        rt.install_console_natives(Arc::new(Mutex::new(Vec::new()))).unwrap();
        assert!(truthy(
            &rt,
            "(function() { var d = Object.getOwnPropertyDescriptor(globalThis, '_lumen_console_log'); \
              return d.enumerable === false && d.writable === true && d.configurable === true; })()"
        ));
        assert_eq!(
            num(
                &rt,
                "var n = 0; for (var k in globalThis) { if (k === '_lumen_console_log') n++; } n"
            ),
            0.0
        );
    }

    /// Sealing must not make the names unreachable *for the engine* — the shim
    /// resolves every native late, through the global object.
    #[test]
    fn natives_stay_callable_after_sealing() {
        let rt = runtime();
        assert!(truthy(&rt, "typeof _lumen_get_attr === 'function'"));
        assert!(truthy(&rt, "typeof _lumen_set_attr === 'function'"));
        assert_eq!(text(&rt, "document.getElementById('main').getAttribute('data-x')"), "orig");
    }

    /// The hijack from the bug report: `window._lumen_get_attr = …` must no
    /// longer re-point `Element.getAttribute`.
    #[test]
    fn native_cannot_be_hijacked_by_assignment() {
        let rt = runtime();
        rt.eval("try { window._lumen_get_attr = function() { return 'HIJACKED'; }; } catch (e) {}")
            .unwrap();
        assert_eq!(
            text(&rt, "document.getElementById('main').getAttribute('data-x')"),
            "orig",
            "page script replaced the DOM's bottom layer"
        );
        // Strict-mode code gets a loud TypeError rather than a silent no-op.
        assert!(truthy(
            &rt,
            "(function() { 'use strict'; \
              try { window._lumen_get_attr = 1; return false; } \
              catch (e) { return e instanceof TypeError; } })()"
        ));
    }

    /// A frozen native is also not removable — `delete` used to report `true`
    /// and leave the shim calling into `undefined`.
    #[test]
    fn native_cannot_be_deleted() {
        let rt = runtime();
        assert!(truthy(
            &rt,
            "(function() { try { return delete window._lumen_get_attr === false; } \
              catch (e) { return true; } })()"
        ));
        assert!(truthy(&rt, "typeof _lumen_get_attr === 'function'"));
    }

    /// The shim's own wrappers around natives (installed during the shim eval,
    /// i.e. before the pass) are frozen too, not just the raw natives — those
    /// wrappers carry the MutationObserver/resource hooks.
    #[test]
    fn shim_wrapper_over_native_is_frozen() {
        let rt = runtime();
        assert!(truthy(
            &rt,
            "Object.getOwnPropertyDescriptor(window, '_lumen_set_attr').writable === false"
        ));
        rt.eval("try { window._lumen_set_attr = function() {}; } catch (e) {}").unwrap();
        rt.eval("document.getElementById('main').setAttribute('data-y', 'v')").unwrap();
        assert_eq!(text(&rt, "document.getElementById('main').getAttribute('data-y')"), "v");
    }

    /// Engine *state* stays writable: the shim assigns to these long after the
    /// pass has run, and a frozen slot would drop those writes silently.
    #[test]
    fn engine_state_stays_writable() {
        let rt = runtime();
        for name in ["_lumen_timers", "_lumen_loc_parts", "_lumen_last_focused_nid"] {
            let d = format!(
                "(function() {{ var d = Object.getOwnPropertyDescriptor(window, '{name}'); \
                  return d !== undefined && d.enumerable === false \n                  && (d.writable === true || typeof d.set === 'function'); }})()"
            );
            assert!(truthy(&rt, &d), "{name} must be hidden but still writable");
        }
        // End-to-end: `_lumen_tick_timers` rewrites `_lumen_timers` on every
        // pump, so a frozen slot would stop timer delivery outright.
        rt.eval("var fired = 0; setTimeout(function() { fired++; }, 0);").unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        assert_eq!(num(&rt, "fired"), 1.0);
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
