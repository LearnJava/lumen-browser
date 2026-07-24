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
//! Nothing in this crate runs the chrome document yet — that is the runtime
//! host introduced in CC-4.

#[cfg(test)]
mod gate;

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
}
