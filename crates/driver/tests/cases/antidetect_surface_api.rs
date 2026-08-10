//! ADR-007 Layer 1 — antidetect surface: no automation marker may be
//! *observable* from a page.
//!
//! Anti-bot systems (Cloudflare, DataDome, Akamai) detect automation by
//! querying globals that headless drivers inject: `navigator.webdriver`
//! (Selenium), `chrome.runtime` (CDP), `cdc_*` (ChromeDriver), `__playwright`
//! / `__pwInitScripts` (Playwright), `__selenium_*` / `__webdriver_*`
//! (Selenium), `callPhantom` / `_phantom` (PhantomJS), `domAutomation*`.
//!
//! This is a **negative test**: it asserts absence, not presence.
//!
//! BUG-379 — why these assertions look the way they do. This file used to scan
//! `crates/js/src/dom.rs` as *text* for marker names, which was wrong twice
//! over: the marker list lived in a different module
//! (`crates/js/src/surface_api.rs`), and "the name does not appear in this
//! source file" is not the property a detector measures anyway. Meanwhile the
//! surface-API shim was defining all fifteen names as non-configurable
//! `undefined`-returning getters, so `Object.getOwnPropertyNames(window)`,
//! `'__webdriver_evaluate' in window` and `hasOwnProperty` all answered *true*
//! on Lumen and *false* on Chrome/Firefox — the anti-fingerprint layer was
//! itself a 15-marker fingerprint, and the green source-scan test never saw
//! it. The assertions below therefore run in a real page on the default (V8)
//! engine and check exactly what a page can observe.
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

/// The fifteen names the shim used to reserve (BUG-379), plus the ChromeDriver
/// and other well-known markers this file has always covered.
const MARKERS: &[&str] = &[
    "__playwright",
    "__pwInitScripts",
    "__pwExecPath",
    "__selenium_unwrapped",
    "__selenium_evaluate",
    "__webdriver_evaluate",
    "__webdriver_script_fn",
    "__webdriver_script_func",
    "__lastWatirAlert",
    "__lastWatirConfirm",
    "__lastWatirPrompt",
    "_phantom",
    "callPhantom",
    "domAutomation",
    "domAutomationController",
    "cdc_adoQpoasnfa76pfcZLmcfl_Array",
    "__webdriverio",
    "__cypress",
    "__nightmare",
    "_selenium",
];

const PAGE: &str = "<html><body><div id=\"d\">probe</div></body></html>";

fn session() -> InProcessSession {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    s
}

/// `eval` a boolean expression in the page and return its value.
fn ev_bool(s: &mut InProcessSession, expr: &str) -> bool {
    let raw = s.eval(expr).unwrap_or_else(|e| panic!("eval {expr}: {e:?}"));
    match raw.trim().trim_matches('"') {
        "true" => true,
        "false" => false,
        other => panic!("expected a boolean from `{expr}`, got `{other}`"),
    }
}

#[test]
fn markers_are_not_own_properties_of_window() {
    let mut s = session();
    for name in MARKERS {
        assert!(
            ev_bool(
                &mut s,
                &format!("Object.getOwnPropertyNames(window).indexOf('{name}') === -1")
            ),
            "'{name}' must not appear in Object.getOwnPropertyNames(window)"
        );
        assert!(
            ev_bool(
                &mut s,
                &format!("!Object.prototype.hasOwnProperty.call(window, '{name}')")
            ),
            "window.hasOwnProperty('{name}') must be false"
        );
    }
}

#[test]
fn markers_fail_the_in_operator() {
    let mut s = session();
    for name in MARKERS {
        assert!(
            ev_bool(&mut s, &format!("!('{name}' in window)")),
            "'{name}' in window must be false — a property that merely reads as \
             undefined is still a detection signal"
        );
    }
}

#[test]
fn markers_read_as_undefined() {
    let mut s = session();
    for name in MARKERS {
        assert!(
            ev_bool(&mut s, &format!("typeof window['{name}'] === 'undefined'")),
            "window['{name}'] must read as undefined"
        );
    }
}

/// The one-line detector from the BUG-379 report: `false` in Chrome and
/// Firefox, so it must be `false` here.
#[test]
fn one_line_prefix_detector_finds_nothing() {
    let mut s = session();
    assert!(
        !ev_bool(
            &mut s,
            "Object.getOwnPropertyNames(window).some(function(n) { \
               return /^(__webdriver|__selenium|__playwright|__pw|__lastWatir|_phantom|callPhantom|domAutomation|cdc_|_selenium)/.test(n); \
             })"
        ),
        "no automation-marker name may be an own property of window"
    );
}

#[test]
fn navigator_webdriver_is_absent() {
    let mut s = session();
    assert!(
        ev_bool(&mut s, "!('webdriver' in navigator)"),
        "'webdriver' must not be a property of navigator"
    );
}

#[test]
fn chrome_runtime_is_absent() {
    let mut s = session();
    assert!(
        ev_bool(
            &mut s,
            "typeof window.chrome === 'undefined' || typeof window.chrome.runtime === 'undefined'"
        ),
        "window.chrome.runtime must be absent (CDP detection marker)"
    );
}

/// The compatibility half of the same layer: properties every real browser
/// exposes, whose *absence* is equally telling.
#[test]
fn standard_navigator_properties_are_present() {
    let mut s = session();
    for expr in [
        "navigator.appName === 'Netscape'",
        "navigator.vendor === 'Google Inc.'",
        "navigator.product === 'Gecko'",
        "navigator.cookieEnabled === true",
        "typeof navigator.plugins === 'object' && navigator.plugins !== null",
        "typeof navigator.mimeTypes === 'object' && navigator.mimeTypes !== null",
    ] {
        assert!(ev_bool(&mut s, expr), "{expr}");
    }
}
