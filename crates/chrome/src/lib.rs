//! Compile-time bridge from the CC design asset (`assets/chrome/chrome.html`)
//! to typed Rust — see `docs/tasks/p1-css-chrome.md` (CC-3).
//!
//! `build.rs` parses the asset with `lumen_html_parser`/`lumen_css_parser`,
//! fails the build if it contains a CSS property or selector `lumen-layout`
//! does not implement, and generates (into `OUT_DIR/chrome_gen.rs`, included
//! below):
//! * `ids` — a string constant per `id`-carrying element.
//! * [`ChromeIds`] — a typed resolver from those constants to [`lumen_dom::NodeId`].
//! * `ChromeAction` — an enum of the distinct `data-action` attribute values.
//! * `templates::IDS` — ids of `<template>` elements (empty until CC-6).
//!
//! [`parse_document`] is the runtime counterpart used by the chrome host
//! introduced in CC-4 (`crates/shell/src/main.rs`): it re-parses the same
//! asset bytes `build.rs` already validated, so [`ChromeIds::resolve`] against
//! its result cannot fail in practice.

#[cfg(test)]
mod gate;
mod model;
pub use model::{
    bind_model, bind_model_tracked, ChromeArchiveEntryModel, ChromeBookmarkCardModel, ChromeBookmarkFolderModel,
    ChromeBookmarksModel, ChromeCertModel, ChromeContentView, ChromeDownloadModel, ChromeDropdownModel,
    ChromeFindModel, ChromeMutations,
    ChromeHistoryModel, ChromeHistoryRow, ChromeModel, ChromePaletteModel,
    ChromePaletteResultModel, ChromePermState, ChromePrintModel, ChromeRightSidebarModel, ChromeSettingsModel,
    ChromeSidebarTab, ChromeSuggestionModel, ChromeTabGroup, ChromeTabModel, ChromeWorkspaceModel, OmniboxModel,
    SelectorTouch, NO_DOMAIN_LABEL,
};

/// Error returned by [`ChromeIds::resolve`] when the chrome [`lumen_dom::Document`]
/// it was given is missing an element id present in `assets/chrome/chrome.html`.
///
/// Should not occur in practice: `build.rs` parses the exact same asset bytes
/// this crate is generated from, so every id in `ids` is guaranteed to exist
/// in a `Document` built by parsing that same asset. Surfaced as a `Result`
/// (not a panic) because that guarantee is a build-time invariant the type
/// system does not itself enforce — production code in this crate does not
/// `unwrap`/`panic!`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeIdError {
    /// The missing element id (one of the `ids::*` constants).
    pub id: &'static str,
}

impl std::fmt::Display for ChromeIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chrome document is missing element id {:?}", self.id)
    }
}

impl std::error::Error for ChromeIdError {}

fn find(doc: &lumen_dom::Document, id: &'static str) -> Result<lumen_dom::NodeId, ChromeIdError> {
    doc.find_by_id(id).ok_or(ChromeIdError { id })
}

/// UA-default rule for the chrome document (CC-CSS-6): real browsers make
/// their own chrome UI text non-selectable by default, and the frozen design
/// reference (`docs/design/lumen-v3_3.html`) carries no `user-select`
/// declaration of its own since the property has no visual effect there —
/// nothing to strip/translate the way CC-CSS-1 did for `::-webkit-scrollbar`.
/// Textually first in the collected style text so any later author rule of
/// equal specificity still overrides it (e.g. a future `.omni-field`
/// exception for editable text). `user-select` is an inherited property, so
/// setting it on `html` covers every chrome element by default.
///
/// No `font-family` default is needed here even though BUG-128 ч.2 changed the
/// document default from an empty list to `serif`: the asset's own
/// `body{font-family:var(--font-ui)}` wins over anything this rule could say,
/// and every chrome element inherits from `<body>`. Verified by
/// `ua_defaults_need_no_font_family_because_the_asset_sets_its_own`.
const UA_DEFAULTS: &str = "html{user-select:none}";

/// Parses `html` (the chrome asset's contents — a runtime host passes
/// [`crate`]-external `chrome_preview::HTML`, the same bytes `build.rs`
/// already validated) into a [`lumen_dom::Document`] plus the
/// [`lumen_css_parser::Stylesheet`] collected from its `<style>` elements
/// (prefixed with [`UA_DEFAULTS`]).
///
/// Mirrors `build.rs`'s own parse step for the `Document` exactly (same
/// parser, same `<style>`-text collection walk) so a [`ChromeIds::resolve`]
/// against the returned document resolves every `ids::*` constant; the
/// `Stylesheet` additionally carries the UA defaults build.rs's own CSS gate
/// does not need to see (codegen only inspects HTML structure).
pub fn parse_document(html: &str) -> (lumen_dom::Document, lumen_css_parser::Stylesheet) {
    let doc = lumen_html_parser::parse(html);
    let mut style_text = String::from(UA_DEFAULTS);
    style_text.push_str(&collect_style_text(&doc));
    let sheet = lumen_css_parser::parse(&style_text);
    (doc, sheet)
}

fn collect_style_text(doc: &lumen_dom::Document) -> String {
    let mut style_text = String::new();
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if let lumen_dom::NodeData::Element { name, .. } = &node.data
            && name.local.eq_ignore_ascii_case("style")
        {
            for &child in &node.children {
                if let lumen_dom::NodeData::Text(text) = &doc.get(child).data {
                    style_text.push_str(text);
                }
            }
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    style_text
}

include!(concat!(env!("OUT_DIR"), "/chrome_gen.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_asset() -> lumen_dom::Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        lumen_html_parser::parse(&html)
    }

    #[test]
    fn resolves_every_id_on_the_real_asset() {
        let doc = parse_asset();
        ChromeIds::resolve(&doc).expect("every ids::* constant must resolve against the real asset");
    }

    #[test]
    fn parse_document_produces_a_document_and_stylesheet_ids_resolve_against() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let (doc, sheet) = parse_document(&html);
        ChromeIds::resolve(&doc).expect("parse_document's Document must carry every ids::* id");
        assert!(!sheet.rules.is_empty(), "parse_document must collect <style> rules");
    }

    #[test]
    fn resolve_fails_cleanly_for_a_document_missing_ids() {
        let doc = lumen_html_parser::parse("<html><body></body></html>");
        let err = ChromeIds::resolve(&doc).unwrap_err();
        assert!(!err.id.is_empty());
        assert!(err.to_string().contains(err.id));
    }

    #[test]
    fn ids_constants_match_asset_id_values() {
        assert_eq!(ids::AVATAR_BTN, "avatarBtn");
        assert_eq!(ids::SIDEBAR, "sidebar");
        assert_eq!(ids::I_PLUS, "i-plus");
    }

    #[test]
    fn chrome_action_round_trips_through_attr_value() {
        assert_eq!(ChromeAction::ToggleFind.attr_value(), "toggle-find");
        assert_eq!(ChromeAction::from_attr_value("toggle-find"), Some(ChromeAction::ToggleFind));

        assert_eq!(ChromeAction::CloseModal.attr_value(), "close-modal");
        assert_eq!(ChromeAction::from_attr_value("close-modal"), Some(ChromeAction::CloseModal));

        assert_eq!(ChromeAction::from_attr_value("not-a-real-action"), None);
    }

    #[test]
    fn template_registry_is_empty_until_cc6() {
        assert!(templates::IDS.is_empty());
    }

    /// CC-CSS-6: `UA_DEFAULTS` must make chrome UI text non-selectable by
    /// default — checked against a real text-bearing element (the profile
    /// name label) rather than a synthetic fixture, since the point is that
    /// the frozen design reference itself carries no `user-select`
    /// declaration and still ends up `None` after `parse_document`.
    #[test]
    fn ua_defaults_make_chrome_text_non_selectable() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let (doc, sheet) = parse_document(&html);
        let chrome_ids = ChromeIds::resolve(&doc).expect("asset must resolve every id");
        let root = lumen_layout::layout(&doc, &sheet, lumen_core::geom::Size::new(1280.0, 800.0));
        let profile_name = lumen_layout::find_box_by_node(&root, chrome_ids.profile_name)
            .expect("#profileName must have a layout box");
        assert_eq!(
            profile_name.style.user_select,
            lumen_layout::UserSelect::None,
            "chrome UI text must be non-selectable by default (CC-CSS-6)"
        );
    }

    /// BUG-128 ч.2 guard: the chrome document is laid out by the ordinary
    /// engine, so it starts from `lumen_layout::ComputedStyle::root()` — whose
    /// default `font-family` became `serif`. Chrome is insulated from that only
    /// because the asset itself declares `body{font-family:var(--font-ui)}`;
    /// drop that declaration and every chrome label would silently switch to
    /// Times New Roman. Probed on `<body>` because what IT computes to is what
    /// every element inheriting a family gets.
    #[test]
    fn ua_defaults_need_no_font_family_because_the_asset_sets_its_own() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let (doc, sheet) = parse_document(&html);
        let body = doc.body().expect("chrome asset must have a <body>");
        let root = lumen_layout::layout(&doc, &sheet, lumen_core::geom::Size::new(1280.0, 800.0));
        let body_box =
            lumen_layout::find_box_by_node(&root, body).expect("<body> must have a layout box");
        assert_eq!(
            body_box.style.font_family.first().map(String::as_str),
            Some("Inter"),
            "chrome must resolve its own --font-ui stack, not the document default `serif`"
        );
    }

    /// CC-16 census: what does the startup parse of the chrome asset actually
    /// cost? The task line is explicitly conditional ("only if the startup
    /// profile justifies it"), so this measures the exact call the shell makes
    /// once during `Lumen` construction — [`parse_document`] on the real asset
    /// — split into its HTML and CSS halves.
    ///
    /// `#[ignore]` (a wall-clock measurement, not an assertion) — run with
    /// `cargo test -p lumen-chrome --release cc16_parse_cost_census -- --ignored --nocapture`.
    /// Reported as min over the samples per `docs/perf-method.md` (min is the
    /// least noise-contaminated estimator of the true cost).
    #[test]
    #[ignore = "wall-clock census, not an assertion — see CC-16"]
    fn cc16_parse_cost_census() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

        const WARMUP: usize = 3;
        const SAMPLES: usize = 20;

        let mut html_ns = Vec::with_capacity(SAMPLES);
        let mut css_ns = Vec::with_capacity(SAMPLES);
        let mut total_ns = Vec::with_capacity(SAMPLES);

        for i in 0..(WARMUP + SAMPLES) {
            let t0 = std::time::Instant::now();
            let doc = lumen_html_parser::parse(&html);
            let t1 = std::time::Instant::now();
            let mut style_text = String::from(UA_DEFAULTS);
            style_text.push_str(&collect_style_text(&doc));
            let sheet = lumen_css_parser::parse(&style_text);
            let t2 = std::time::Instant::now();
            // Keep both alive across the timers so nothing is optimized away.
            std::hint::black_box((&doc, &sheet));
            if i >= WARMUP {
                html_ns.push((t1 - t0).as_nanos());
                css_ns.push((t2 - t1).as_nanos());
                total_ns.push((t2 - t0).as_nanos());
            }
        }

        let min = |v: &[u128]| v.iter().copied().min().unwrap_or(0) as f64 / 1e6;
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse(&collect_style_text(&doc));
        println!(
            "CC16_CENSUS asset_bytes={} nodes={} css_rules={} html_min={:.2}ms css_min={:.2}ms total_min={:.2}ms",
            html.len(),
            doc.node_count(),
            sheet.rules.len(),
            min(&html_ns),
            min(&css_ns),
            min(&total_ns),
        );

        // The other half of the CC-16 ledger: a pre-parsed asset still has to be
        // *restored* at startup. `lumen_dom::Document` already derives serde and
        // `bincode` is what T3 hibernation uses, so this is the real cost of the
        // proposal's own restore step — not an estimate. `Document::clone` is the
        // floor any reconstruction (bincode, codegen'd constructors, anything)
        // must pay, since all of them allocate the same 2160 nodes and strings.
        let blob = bincode::serialize(&doc).expect("chrome Document must serialize");
        let mut de_ns = Vec::with_capacity(SAMPLES);
        let mut clone_ns = Vec::with_capacity(SAMPLES);
        for i in 0..(WARMUP + SAMPLES) {
            let t0 = std::time::Instant::now();
            let restored: lumen_dom::Document =
                bincode::deserialize(&blob).expect("chrome Document must round-trip");
            let t1 = std::time::Instant::now();
            let cloned = doc.clone();
            let t2 = std::time::Instant::now();
            std::hint::black_box((&restored, &cloned));
            if i >= WARMUP {
                de_ns.push((t1 - t0).as_nanos());
                clone_ns.push((t2 - t1).as_nanos());
            }
        }
        println!(
            "CC16_CENSUS_RESTORE blob_bytes={} bincode_de_min={:.2}ms doc_clone_min={:.2}ms",
            blob.len(),
            min(&de_ns),
            min(&clone_ns),
        );

        // Context for the verdict: parsing is only the first of the two things
        // startup does with this asset. The host immediately lays it out, and
        // that pass is *not* something a pre-parsed `Document` skips.
        let mut layout_ns = Vec::with_capacity(SAMPLES);
        for i in 0..(WARMUP + SAMPLES) {
            let t0 = std::time::Instant::now();
            let root = lumen_layout::layout(&doc, &sheet, lumen_core::geom::Size::new(1280.0, 800.0));
            let t1 = std::time::Instant::now();
            std::hint::black_box(&root);
            if i >= WARMUP {
                layout_ns.push((t1 - t0).as_nanos());
            }
        }
        println!("CC16_CENSUS_LAYOUT chrome_layout_min={:.2}ms", min(&layout_ns));
    }

    /// `UA_DEFAULTS` is textually first, so an author rule of equal
    /// specificity declared later in the collected style text still wins —
    /// verified with a synthetic override rather than the frozen asset
    /// (which declares no `user-select` of its own, CC-CSS-6).
    #[test]
    fn ua_defaults_can_be_overridden_by_a_later_author_rule() {
        let html = r#"<html><body><input id="omniInput"></body></html>"#;
        let (doc, mut sheet) = parse_document(html);
        sheet.merge_from(lumen_css_parser::parse("#omniInput{user-select:text}"));
        let root = lumen_layout::layout(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0));
        let omni_input = doc.find_by_id("omniInput").expect("test fixture has #omniInput");
        let b = lumen_layout::find_box_by_node(&root, omni_input).expect("#omniInput must have a layout box");
        assert_eq!(b.style.user_select, lumen_layout::UserSelect::Text);
    }
}
