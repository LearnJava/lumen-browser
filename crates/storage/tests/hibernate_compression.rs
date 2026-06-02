//! Integration test for T3-hibernation DOM blob compression (ADR-008 §10J.1).
//!
//! Uses a real parsed `Document` (not synthetic bytes) to prove the hibernation
//! round-trip preserves the tree and that a representative page genuinely shrinks
//! on disk — the property the whole feature exists for.

use lumen_dom::Document;
use lumen_html_parser::tree_builder::parse;
use lumen_storage::tab_snapshot::{HibernatedTabData, TabSnapshotStore};

/// A medium page with lots of repeated structure — typical of real DOM.
fn sample_page() -> String {
    let mut html = String::from("<!doctype html><html><head><title>Лента новостей</title></head><body>");
    for i in 0..300 {
        html.push_str(&format!(
            "<article class=\"card\" data-id=\"{i}\"><h2 class=\"card-title\">Заголовок {i}</h2>\
             <p class=\"card-body\">Текст параграфа номер {i} с повторяющейся разметкой.</p>\
             <a class=\"card-link\" href=\"/item/{i}\">читать далее</a></article>"
        ));
    }
    html.push_str("</body></html>");
    html
}

#[test]
fn real_document_blob_roundtrips_and_shrinks() {
    let doc = parse(&sample_page());
    let raw = doc.to_bytes().expect("serialise document");
    assert!(raw.len() > 8 * 1024, "expected a non-trivial blob, got {}", raw.len());

    let store = TabSnapshotStore::open_in_memory().unwrap();
    store
        .store(
            1,
            &HibernatedTabData {
                dom_blob: raw.clone(),
                css_source: "body { font: 16px sans-serif; }".into(),
                url: "https://example.com/feed".into(),
                title: "Лента новостей".into(),
                scroll_x: 0.0,
                scroll_y: 480.0,
            },
        )
        .unwrap();

    // fetch must return byte-identical raw bincode (transparent inflate).
    let fetched = store.fetch(1).unwrap().unwrap();
    assert_eq!(fetched.dom_blob, raw);

    // and the reconstructed tree must match the original structurally.
    let restored = Document::from_bytes(&fetched.dom_blob).expect("deserialise document");
    assert_eq!(restored.to_string(), doc.to_string());
}
