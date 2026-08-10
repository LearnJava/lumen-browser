//! ADR-007 Layer 1 runtime audit (9A.1, 9A.2).
//!
//! Verifies at runtime — via `V8JsRuntime` with the full DOM shim installed —
//! that no automation-detection markers are present in the Lumen JS environment.
//!
//! These are **negative tests**: we assert *absence*, not presence.
//! Anti-bot systems (Cloudflare, DataDome, Akamai) query these properties to
//! distinguish real browsers from headless automation tools:
//!
//! | Marker                          | Tool                        |
//! |---------------------------------|-----------------------------|
//! | `navigator.webdriver === true`  | Selenium / WebDriver        |
//! | `window.chrome.runtime`         | Chrome DevTools Protocol    |
//! | `cdc_*` variables               | ChromeDriver                |
//! | `__playwright` / `__pwInitScripts` | Playwright               |
//! | `__selenium_*` / `__webdriver_*` | Selenium                   |
//! | `callPhantom` / `_phantom`      | PhantomJS                   |
//! | `domAutomation*`                | WebDriver DOM injector      |
//!
//! Ported from `QuickJsRuntime` to `V8JsRuntime` in S12b-B6: the rquickjs side of
//! `surface_api.rs` was removed, so `navigator.appName`/`.vendor`/`.plugins`/`.mimeTypes`
//! are only installed under V8.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    use lumen_core::JsRuntime;
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

/// Assert a marker name is **absent** from `window`, not merely readable as
/// `undefined`.
///
/// BUG-379: `typeof window.__x === 'undefined'` — what every test here used to
/// assert — is the one detector variant that a `{ get: () => undefined }`
/// property satisfies, so it stayed green while all fifteen markers really
/// were own properties of the global. Detectors use `getOwnPropertyNames` /
/// `in` / `hasOwnProperty`, which distinguish the two states; so does this.
fn assert_marker_absent(rt: &V8JsRuntime, name: &str) {
    assert!(
        bool_eval(rt, &format!("typeof window.{name} === 'undefined'")),
        "window.{name} must read as undefined"
    );
    assert!(
        bool_eval(rt, &format!("!('{name}' in window)")),
        "'{name}' in window must be false — an undefined-returning property is itself a marker"
    );
    assert!(
        bool_eval(
            rt,
            &format!("!Object.prototype.hasOwnProperty.call(window, '{name}')")
        ),
        "window.hasOwnProperty('{name}') must be false"
    );
    assert!(
        bool_eval(
            rt,
            &format!("Object.getOwnPropertyNames(window).indexOf('{name}') === -1")
        ),
        "'{name}' must not appear in Object.getOwnPropertyNames(window)"
    );
}

// ── navigator.webdriver ──────────────────────────────────────────────────────

#[test]
fn webdriver_is_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof navigator.webdriver === 'undefined'"),
        "navigator.webdriver must be absent (Selenium detection marker)"
    );
}

#[test]
fn webdriver_not_in_navigator() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "!('webdriver' in navigator)"),
        "'webdriver' must not be enumerable on navigator"
    );
}

// ── Chrome DevTools Protocol markers ────────────────────────────────────────

#[test]
fn chrome_runtime_absent() {
    let rt = make_rt();
    // window.chrome should either be absent entirely or lack .runtime.
    assert!(
        bool_eval(
            &rt,
            "typeof window.chrome === 'undefined' || typeof window.chrome.runtime === 'undefined'"
        ),
        "window.chrome.runtime must be absent (CDP detection marker)"
    );
}

#[test]
fn no_cdc_variables() {
    // Known ChromeDriver client marker — the full name is obfuscated per build,
    // but all variants start with "cdc_".  We verify the well-known form.
    let rt = make_rt();
    assert!(
        bool_eval(
            &rt,
            "typeof window.cdc_adoQpoasnfa76pfcZLmcfl_Array === 'undefined'"
        ),
        "cdc_* ChromeDriver variable must be absent"
    );
}

// ── Playwright markers ───────────────────────────────────────────────────────

#[test]
fn playwright_global_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "__playwright");
}

#[test]
fn playwright_init_scripts_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "__pwInitScripts");
}

#[test]
fn playwright_exec_path_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "__pwExecPath");
}

// ── Selenium / WebDriver markers ─────────────────────────────────────────────

#[test]
fn selenium_unwrapped_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "__selenium_unwrapped");
    assert_marker_absent(&rt, "__selenium_evaluate");
}

#[test]
fn webdriver_evaluate_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "__webdriver_evaluate");
}

#[test]
fn webdriver_script_fn_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "__webdriver_script_fn");
    assert_marker_absent(&rt, "__webdriver_script_func");
}

// ── Watir markers ────────────────────────────────────────────────────────────

#[test]
fn watir_dialog_hooks_absent() {
    let rt = make_rt();
    for name in ["__lastWatirAlert", "__lastWatirConfirm", "__lastWatirPrompt"] {
        assert_marker_absent(&rt, name);
    }
}

// ── PhantomJS markers ────────────────────────────────────────────────────────

#[test]
fn call_phantom_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "callPhantom");
}

#[test]
fn phantom_global_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "_phantom");
}

// ── DOM Automation controller ────────────────────────────────────────────────

#[test]
fn dom_automation_absent() {
    let rt = make_rt();
    assert_marker_absent(&rt, "domAutomation");
    assert_marker_absent(&rt, "domAutomationController");
}

// ── The whole surface at once ─────────────────────────────────────────────────

/// BUG-379's one-line detector, verbatim in spirit: enumerate the global's own
/// property names and look for any automation-tool prefix. `false` in Chrome
/// and Firefox — must be `false` here.
#[test]
fn one_line_prefix_detector_finds_nothing() {
    let rt = make_rt();
    assert!(
        !bool_eval(
            &rt,
            "Object.getOwnPropertyNames(window).some(function(n) { \
               return /^(__webdriver|__selenium|__playwright|__pw|__lastWatir|_phantom|callPhantom|domAutomation|cdc_)/.test(n); \
             })"
        ),
        "no automation-marker name may be an own property of window"
    );
}

// ── Standard browser properties present ──────────────────────────────────────
// A real browser exposes these; their absence is itself a detection signal.

#[test]
fn navigator_app_name_is_netscape() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "navigator.appName === 'Netscape'"),
        "navigator.appName must be 'Netscape'"
    );
}

#[test]
fn navigator_vendor_is_google() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "navigator.vendor === 'Google Inc.'"),
        "navigator.vendor must be 'Google Inc.'"
    );
}

#[test]
fn navigator_plugins_is_object() {
    let rt = make_rt();
    assert!(
        bool_eval(
            &rt,
            "typeof navigator.plugins === 'object' && navigator.plugins !== null"
        ),
        "navigator.plugins must be a non-null object"
    );
}

#[test]
fn navigator_mime_types_is_object() {
    let rt = make_rt();
    assert!(
        bool_eval(
            &rt,
            "typeof navigator.mimeTypes === 'object' && navigator.mimeTypes !== null"
        ),
        "navigator.mimeTypes must be a non-null object"
    );
}

// ── event.isTrusted for native dispatches ────────────────────────────────────
// WebDriver-dispatched events have isTrusted=false; shell-dispatched events
// must have isTrusted=true so sites cannot distinguish from real user input.

#[test]
fn synthetic_event_is_not_trusted_by_default() {
    let rt = make_rt();
    // Events created via `new Event(...)` are not trusted (spec §2.9).
    assert!(
        bool_eval(&rt, "new Event('click').isTrusted === false"),
        "synthetic events must have isTrusted=false"
    );
}

#[test]
fn event_init_dict_can_set_is_trusted() {
    let rt = make_rt();
    // Shell-side dispatchers pass { isTrusted: true } in the init dict.
    assert!(
        bool_eval(
            &rt,
            "new Event('click', { isTrusted: true }).isTrusted === true"
        ),
        "events with isTrusted:true in init must be trusted"
    );
}
