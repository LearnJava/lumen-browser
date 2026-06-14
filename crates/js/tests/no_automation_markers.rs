//! ADR-007 Layer 1 runtime audit (9A.1, 9A.2).
//!
//! Verifies at runtime — via `QuickJsRuntime` with the full DOM shim installed —
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

use std::sync::{Arc, Mutex};

use lumen_dom::Document;
use lumen_js::QuickJsRuntime;

fn make_rt() -> QuickJsRuntime {
    let rt = QuickJsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None)
        .unwrap();
    rt
}

fn bool_eval(rt: &QuickJsRuntime, script: &str) -> bool {
    use lumen_core::JsRuntime;
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
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
    assert!(
        bool_eval(&rt, "typeof window.__playwright === 'undefined'"),
        "__playwright must be absent (Playwright detection marker)"
    );
}

#[test]
fn playwright_init_scripts_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.__pwInitScripts === 'undefined'"),
        "__pwInitScripts must be absent"
    );
}

#[test]
fn playwright_exec_path_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.__pwExecPath === 'undefined'"),
        "__pwExecPath must be absent"
    );
}

// ── Selenium / WebDriver markers ─────────────────────────────────────────────

#[test]
fn selenium_unwrapped_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.__selenium_unwrapped === 'undefined'"),
        "__selenium_unwrapped must be absent"
    );
}

#[test]
fn webdriver_evaluate_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.__webdriver_evaluate === 'undefined'"),
        "__webdriver_evaluate must be absent"
    );
}

#[test]
fn webdriver_script_fn_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.__webdriver_script_fn === 'undefined'"),
        "__webdriver_script_fn must be absent"
    );
}

// ── PhantomJS markers ────────────────────────────────────────────────────────

#[test]
fn call_phantom_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.callPhantom === 'undefined'"),
        "callPhantom must be absent (PhantomJS detection marker)"
    );
}

#[test]
fn phantom_global_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window._phantom === 'undefined'"),
        "_phantom must be absent"
    );
}

// ── DOM Automation controller ────────────────────────────────────────────────

#[test]
fn dom_automation_absent() {
    let rt = make_rt();
    assert!(
        bool_eval(&rt, "typeof window.domAutomation === 'undefined'"),
        "domAutomation must be absent"
    );
    assert!(
        bool_eval(
            &rt,
            "typeof window.domAutomationController === 'undefined'"
        ),
        "domAutomationController must be absent"
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
