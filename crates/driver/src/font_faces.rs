//! FONTLOAD-3: populates `Document::fonts` with CSS-connected `FontFace`
//! entries from `@font-face` rules parsed out of the initial page markup.
//!
//! `crates/shell` has done this since FONTLOAD-1/2
//! (`crates/shell/src/page_pipeline.rs`, `rule_to_font_face`); `InProcessSession`
//! (`crates/driver/src/session.rs`) never called `Document::fonts_mut()` at
//! all, so `document.fonts.size` read `0` for anything driven through it —
//! `crates/driver`'s own Rust-level test harness (`crates/driver/tests/`) and
//! headless embedders (`lumen --mcp`, `crates/shell/src/automation_server.rs`).
//! Note this is narrower than `bugs/BUG-467-OPEN.md`'s FONTLOAD-2 "gap 0"
//! framing, which named this file/module as the WPT `run_report.py` path —
//! that path actually spawns the real `lumen.exe` (`crates/shell`) and drives
//! it over WebDriver BiDi (`tests/wpt/README.md`), so it goes through
//! `page_pipeline.rs`, not `InProcessSession`; this slice doesn't move that
//! pass rate. Worth doing anyway: `InProcessSession` is a real, documented
//! embedding/testing surface (`lib.rs` "Уровни 2–3 тестирования") with the
//! same gap. This module is a driver-local port, not a shared one: `lumen-dom`
//! and `lumen-css-parser` are sibling leaf crates (neither depends on the
//! other), so a conversion between their types has no natural shared home
//! short of a new crate — out of proportion for one ~15-line function.
//! `session.rs` is already past the 2000-line file-size cap and must not gain
//! it either.
//!
//! Deliberately narrower than shell's version: every entry starts
//! [`lumen_dom::FontFaceStatus::Unloaded`] (the constructor default), even
//! for `local()` sources. Shell immediately marks a resolvable `local()`
//! source `Loaded` because it already runs a `FontRegistry`/
//! `SystemFontIndex` lookup for real rendering; `InProcessSession` (`session.rs`)
//! has no such registry, so reproducing that check would mean pulling font
//! matching into driver for no reader of `document.fonts.size`/`.has()` — the
//! WPT tests this unblocks only need the CSS-connected set to be non-empty
//! and iterable, not to know which entries are locally resolvable. Unloaded
//! is also the spec's own initial state for a CSS-connected face that
//! hasn't been forced to load yet (CSS Font Loading §11.1).
//!
//! Static-only, same as shell: rules are read once, from the stylesheet
//! parsed before `run_pipeline` returns. A later CSSOM mutation
//! (`insertRule`, `<style>` inserted/removed by script) is not reflected —
//! that reactivity gap is the larger, separately-scoped "gap 1" in
//! `bugs/BUG-467-OPEN.md`.

use lumen_css_parser::{FontFaceRule, FontFaceSourceKind};
use lumen_dom::{Document, FontFace, FontFaceExtendedDescriptors};

/// Converts a parsed `@font-face` rule into a DOM `FontFace`, mirroring
/// `crates/shell/src/subresources.rs::rule_to_font_face`.
fn rule_to_font_face(rule: &FontFaceRule) -> FontFace {
    let src_str = rule
        .sources
        .iter()
        .map(|src| {
            let kind_str = match src.kind {
                FontFaceSourceKind::Url => "url",
                FontFaceSourceKind::Local => "local",
            };
            format!("{kind_str}(\"{}\")", src.value)
        })
        .collect::<Vec<_>>()
        .join(", ");

    FontFace::new(
        rule.family.clone(),
        rule.style.as_deref().unwrap_or("normal").to_string(),
        rule.weight.as_deref().unwrap_or("400").to_string(),
        rule.stretch.clone(),
        rule.unicode_range.clone(),
        src_str,
    )
    .with_extended_descriptors(FontFaceExtendedDescriptors {
        feature_settings: rule.feature_settings.clone(),
        variation_settings: rule.variation_settings.clone(),
        display: rule.display.clone(),
        ascent_override: rule.ascent_override.clone(),
        descent_override: rule.descent_override.clone(),
        line_gap_override: rule.line_gap_override.clone(),
        size_adjust: rule.size_adjust.clone(),
    })
}

/// Adds one `FontFace` entry per `@font-face` rule in `font_faces` to
/// `doc.fonts_mut()`. Called once per navigation, from a stylesheet parse
/// done BEFORE the page's own scripts run (`session.rs::run_pipeline`) — a
/// synchronous top-level script read of `document.fonts` freezes the JS-side
/// snapshot on first touch, so population has to land ahead of that touch,
/// not after it (see the call site's comment for the full reasoning).
pub(crate) fn populate_document_fonts(doc: &mut Document, font_faces: &[FontFaceRule]) {
    for rule in font_faces {
        doc.fonts_mut().add(rule_to_font_face(rule));
    }
}
