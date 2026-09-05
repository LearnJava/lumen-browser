//! FONTLOAD-3 — `InProcessSession::navigate`/`navigate_html` must populate
//! `document.fonts` from `@font-face` rules in the initial page markup
//! BEFORE the page's own scripts run, mirroring what `crates/shell` has done
//! since FONTLOAD-1/2 but for `InProcessSession` (`bugs/BUG-467-OPEN.md`
//! FONTLOAD-2 "gap 0" — narrower framing at `crate::font_faces`'s module
//! doc). Timing is the actual bug, not just presence: `document.fonts`'s JS
//! wrapper caches its `FontFaceSet` snapshot on first touch
//! (`_lumen_wrapper_slot`, `crates/js/src/shim/web_api_shim_mid.js`), so a
//! test that verifies only a POST-navigation `eval` would pass even if
//! population landed too late to matter — the first test below is the one
//! that actually exercises the ordering fix, by reading `document.fonts`
//! from a synchronous top-level `<script>`, the exact WPT pattern this
//! unblocks (`document.fonts.ready.then(...)` before any `test()`
//! registers).
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE_WITH_FONT_FACE: &str = r#"<html><head><style>
@font-face { font-family: "Probe"; src: local("Arial"), url("probe.woff2"); }
</style>
<script>window.__sync_size = document.fonts.size;</script>
</head><body></body></html>"#;

#[test]
fn synchronous_top_level_script_sees_css_connected_font_face() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE_WITH_FONT_FACE).expect("navigate_html");

    // The value the page's OWN top-level script captured, before this test's
    // `eval` ever runs — proves population happened ahead of script
    // execution, not merely ahead of this assertion.
    let sync_size = session.eval("window.__sync_size").expect("eval sync size");
    assert_eq!(sync_size, "1", "top-level script must see the CSS-connected face immediately");
}

#[test]
fn post_navigation_eval_reports_family_and_unloaded_status() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE_WITH_FONT_FACE).expect("navigate_html");

    let family = session
        .eval("document.fonts.values().next().value.family")
        .expect("eval family");
    assert_eq!(family, "\"Probe\"");

    // `InProcessSession` has no `FontRegistry`/`SystemFontIndex` to resolve
    // `local()` against, so — unlike shell — every entry starts `unloaded`,
    // the constructor default and also the spec's own initial state for a
    // CSS-connected face nothing has forced to load yet.
    let status = session
        .eval("document.fonts.values().next().value.status")
        .expect("eval status");
    assert_eq!(status, "\"unloaded\"");
}

#[test]
fn page_without_font_face_rules_reports_empty_set() {
    let mut session = InProcessSession::new();
    session.navigate_html("<html><body>no fonts here</body></html>").expect("navigate_html");

    let size = session.eval("document.fonts.size").expect("eval size");
    assert_eq!(size, "0");
}

// FONTLOAD-8 (`bugs/BUG-467-OPEN.md`): the CSS-side `@font-face` descriptor
// grammar — `FontFaceRule` now carries `font-feature-settings`/
// `font-variation-settings`/the four metrics-override descriptors, threaded
// through `crate::font_faces::rule_to_font_face` into the same
// `document.fonts` snapshot these other tests already exercise end-to-end
// (real CSS parser → `lumen_dom::FontFace` → native JSON → JS wrapper), not
// just the JS-shim-only unit coverage in `crates/js`.

const PAGE_WITH_EXTENDED_DESCRIPTORS: &str = r#"<html><head><style>
@font-face {
    font-family: "Extended";
    src: url("extended.woff2");
    font-feature-settings: "liga" 1;
    font-variation-settings: "wght" 700;
    ascent-override: 90%;
    descent-override: 10%;
    line-gap-override: normal;
    size-adjust: 105%;
}
</style></head><body></body></html>"#;

#[test]
fn css_connected_face_exposes_extended_descriptors() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE_WITH_EXTENDED_DESCRIPTORS).expect("navigate_html");

    let ok = session
        .eval(
            r#"
                (function() {
                    var f = document.fonts.values().next().value;
                    return f.featureSettings === '"liga" 1' &&
                        f.variationSettings === '"wght" 700' &&
                        f.ascentOverride === '90%' &&
                        f.descentOverride === '10%' &&
                        f.lineGapOverride === 'normal' &&
                        f.sizeAdjust === '105%';
                })()
            "#,
        )
        .expect("eval extended descriptors");
    assert_eq!(ok, "true");
}

#[test]
fn css_connected_face_defaults_extended_descriptors_when_absent() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE_WITH_FONT_FACE).expect("navigate_html");

    let ok = session
        .eval(
            r#"
                (function() {
                    var f = document.fonts.values().next().value;
                    return f.featureSettings === 'normal' &&
                        f.variationSettings === 'normal' &&
                        f.display === 'auto' &&
                        f.ascentOverride === 'normal' &&
                        f.descentOverride === 'normal' &&
                        f.lineGapOverride === 'normal' &&
                        f.sizeAdjust === '100%';
                })()
            "#,
        )
        .expect("eval defaults");
    assert_eq!(ok, "true");
}
