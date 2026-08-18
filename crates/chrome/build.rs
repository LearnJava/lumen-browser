//! Compile-time parse-gate + codegen for the chrome asset (CC-3,
//! docs/tasks/p1-css-chrome.md). Runs `lumen_html_parser`/`lumen_css_parser`
//! — the exact same parsers the runtime chrome host (CC-4) will use — over
//! `assets/chrome/chrome.html` and fails the build if it contains a CSS
//! property or selector the layout engine does not implement. On success it
//! emits `OUT_DIR/chrome_gen.rs`: string id constants, a typed `ChromeIds`
//! resolver, the `ChromeAction` enum (from `data-action` attribute values),
//! and the `<template>` id registry.
//!
//! `assets/chrome/chrome.html` is itself generated from the frozen design
//! reference by `scripts/gen_chrome_assets.py` — a rendering mismatch is an
//! engine bug, and an unmapped interaction is a generator gap; this build
//! script only validates/reads the already-generated asset, it never edits it.
//!
//! The CSS gate + codegen-identifier helpers live in `src/gate.rs`, included
//! verbatim below so the same logic is unit-testable from `mod gate;` in
//! `lib.rs` without duplication.

// Постоянное исключение из `clippy::panic` (docs/lint-policy.md §10): паника —
// это и есть штатный способ провалить сборку из build-скрипта. Возвращать
// `Result` некуда, а «мягкий» отказ дал бы собранный бинарь с невалидным
// ассетом хрома — ровно то, что этот скрипт обязан не допустить.
#![allow(clippy::panic)]

use std::collections::{BTreeSet, HashSet};
use std::{env, fs, path::Path, path::PathBuf};

use lumen_dom::{Document, NodeData};

include!("src/gate.rs");

fn main() {
    let asset_path = asset_path();
    println!("cargo:rerun-if-changed={}", asset_path.display());

    let html = fs::read_to_string(&asset_path).unwrap_or_else(|e| {
        panic!(
            "lumen-chrome build.rs: failed to read {}: {e} (run `python scripts/gen_chrome_assets.py` first)",
            asset_path.display()
        )
    });

    let doc = lumen_html_parser::parse(&html);
    let style_text = collect_style_text(&doc);
    let sheet = lumen_css_parser::parse(&style_text);

    let violations = validate_css(&sheet);
    if !violations.is_empty() {
        panic!(
            "lumen-chrome build.rs: {} unsupported CSS construct(s) in assets/chrome/chrome.html:\n  {}\n\
             Either the reference uses a CSS feature lumen-layout does not implement yet (fix the engine, \
             CC-CSS-*), or a new `-webkit-*` construct needs an explicit allowlist entry in build.rs/gate.rs.",
            violations.len(),
            violations.join("\n  ")
        );
    }

    let collected = collect(&doc);
    write_codegen(&collected);
}

fn asset_path() -> PathBuf {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|e| panic!("lumen-chrome build.rs: CARGO_MANIFEST_DIR not set: {e}"));
    Path::new(&manifest_dir).join("../../assets/chrome/chrome.html")
}

fn collect_style_text(doc: &Document) -> String {
    let mut style_text = String::new();
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if let NodeData::Element { name, .. } = &node.data
            && name.local.eq_ignore_ascii_case("style")
        {
            for &child in &node.children {
                if let NodeData::Text(text) = &doc.get(child).data {
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

// ── Codegen ──────────────────────────────────────────────────────────────

struct Collected {
    ids: Vec<String>,
    actions: BTreeSet<String>,
    templates: Vec<String>,
}

fn collect(doc: &Document) -> Collected {
    let mut ids = Vec::new();
    let mut actions = BTreeSet::new();
    let mut templates = Vec::new();

    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if let NodeData::Element { name, .. } = &node.data {
            if let Some(v) = node.get_attr("id") {
                ids.push(v.to_string());
                if name.local.eq_ignore_ascii_case("template") {
                    templates.push(v.to_string());
                }
            }
            if let Some(v) = node.get_attr("data-action") {
                actions.insert(v.to_string());
            }
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    ids.sort();
    templates.sort();
    Collected { ids, actions, templates }
}

fn write_codegen(collected: &Collected) {
    let out_dir = env::var("OUT_DIR")
        .unwrap_or_else(|e| panic!("lumen-chrome build.rs: OUT_DIR not set: {e}"));
    let dest = Path::new(&out_dir).join("chrome_gen.rs");

    let mut seen_const = HashSet::new();
    let mut id_entries = Vec::new();
    for id in &collected.ids {
        let field = to_snake_case(id);
        let const_name = field.to_uppercase();
        if is_reserved_identifier(&field) {
            panic!(
                "lumen-chrome build.rs: element id {id:?} converts to Rust keyword `{field}` — \
                 rename the id in docs/design/lumen-v3_3.html or special-case it in build.rs"
            );
        }
        if !seen_const.insert(const_name.clone()) {
            panic!(
                "lumen-chrome build.rs: two element ids collide after codegen name conversion: \
                 `{const_name}` (from {id:?}) — rename one in docs/design/lumen-v3_3.html"
            );
        }
        id_entries.push((const_name, field, id.clone()));
    }

    let action_entries: Vec<(String, &String)> = collected
        .actions
        .iter()
        .map(|action| (to_pascal_case(action), action))
        .collect();

    let mut out = String::new();
    out.push_str(
        "// @generated by crates/chrome/build.rs from assets/chrome/chrome.html. Do not edit.\n\n",
    );

    out.push_str("/// String ids of every element carrying an `id` attribute in `assets/chrome/chrome.html`.\npub mod ids {\n");
    for (const_name, _field, id) in &id_entries {
        out.push_str(&format!("    /// `id=\"{id}\"`.\n    pub const {const_name}: &str = {id:?};\n"));
    }
    out.push_str("}\n\n");

    out.push_str(
        "/// Typed resolver for every id-carrying element of the chrome document.\n\
         ///\n\
         /// Built once at startup by [`ChromeIds::resolve`] against the live\n\
         /// (runtime-parsed) chrome `Document` — see docs/tasks/p1-css-chrome.md CC-4.\n\
         #[derive(Debug, Clone, Copy)]\npub struct ChromeIds {\n",
    );
    for (_const_name, field, id) in &id_entries {
        out.push_str(&format!("    /// `id=\"{id}\"`.\n    pub {field}: lumen_dom::NodeId,\n"));
    }
    out.push_str("}\n\n");

    out.push_str(
        "impl ChromeIds {\n\
         \x20   /// Resolves every id constant against `doc`. Fails only if `doc` was not\n\
         \x20   /// parsed from the exact `assets/chrome/chrome.html` this crate was built\n\
         \x20   /// against — build.rs already validated every id here exists in that asset.\n\
         \x20   pub fn resolve(doc: &lumen_dom::Document) -> Result<Self, ChromeIdError> {\n\
         \x20       Ok(Self {\n",
    );
    for (const_name, field, _id) in &id_entries {
        out.push_str(&format!("            {field}: find(doc, ids::{const_name})?,\n"));
    }
    out.push_str("        })\n    }\n}\n\n");

    out.push_str(
        "/// Semantic action carried by a chrome element's `data-action` attribute.\n\
         ///\n\
         /// Generated from the distinct `data-action=\"…\"` values in\n\
         /// `assets/chrome/chrome.html` — see `scripts/gen_chrome_assets.py`. Consumed by\n\
         /// the hit-test dispatcher introduced in CC-5.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\npub enum ChromeAction {\n",
    );
    for (variant, value) in &action_entries {
        out.push_str(&format!("    /// `data-action=\"{value}\"`.\n    {variant},\n"));
    }
    out.push_str("}\n\n");

    out.push_str(
        "impl ChromeAction {\n\
         \x20   /// Parses a `data-action` attribute value. `None` for unrecognised strings\n\
         \x20   /// (should not occur for the build-gated asset; kept `Option` because callers\n\
         \x20   /// may read attributes off a live, possibly-mutated DOM).\n\
         \x20   pub fn from_attr_value(value: &str) -> Option<Self> {\n        match value {\n",
    );
    for (variant, value) in &action_entries {
        out.push_str(&format!("            {value:?} => Some(Self::{variant}),\n"));
    }
    out.push_str(
        "            _ => None,\n        }\n    }\n\n\
         \x20   /// Serialises back to the `data-action` attribute string.\n\
         \x20   pub const fn attr_value(self) -> &'static str {\n        match self {\n",
    );
    for (variant, value) in &action_entries {
        out.push_str(&format!("            Self::{variant} => {value:?},\n"));
    }
    out.push_str("        }\n    }\n}\n\n");

    out.push_str(
        "/// `id`s of `<template>` elements in the chrome asset — CC-6 clones these for\n\
         /// lists (tab rows, omnibox suggestions, download cards, history entries, …).\n\
         ///\n\
         /// Empty for now: the current asset renders its example lists as static markup;\n\
         /// CC-6 introduces `<template>` wrapping once the ChromeModel→DOM diff needs\n\
         /// real cloning (docs/tasks/p1-css-chrome.md CC-3 step 3 / CC-6).\n\
         pub mod templates {\n    pub const IDS: &[&str] = &[\n",
    );
    for t in &collected.templates {
        out.push_str(&format!("        {t:?},\n"));
    }
    out.push_str("    ];\n}\n");

    fs::write(&dest, out)
        .unwrap_or_else(|e| panic!("lumen-chrome build.rs: failed to write {}: {e}", dest.display()));
}
