//! Тесты `dom.rs`, вынесенные из inline-модуля `mod tests` (дорожка SPLIT,
//! батчи JS-1 и JS-2).
//!
//! Здесь осталась шапка модуля — фикстуры и помощники, которые видят все
//! потомки (`make_doc`, `find_element_by_tag`, `runtime_with_dom` и соседи),
//! плюс два теста самого шима; остальное разложено по файлам-модулям рядом.

#[cfg(feature = "v8-backend")]
use super::*;
#[cfg(feature = "v8-backend")]
use lumen_core::JsRuntime;
#[cfg(feature = "v8-backend")]
use lumen_dom::{Document, NodeData, QualName};

/// BUG-401 split the page shim into five consts so `worker.rs` can evaluate
/// the two `[Exposed=(Window,Worker)]` blocks in a worker scope without a
/// second copy of them. The page program must come out unchanged: the parts
/// have to be spliced back in source order, each exactly once — a wrong
/// order would put `Object.create(EventTarget.prototype)` before
/// `EventTarget` exists, and a duplicate would reset `_perf_entries`
/// halfway through the shim.
#[cfg(feature = "v8-backend")]
#[test]
fn web_api_shim_splices_its_parts_in_source_order() {
    let shim = web_api_shim();
    let mut prev = 0usize;
    for marker in [
        "function _lumen_u2n",
        "function EventTarget()",
        "function UIEvent(",
        "function Performance()",
        "function PerformanceObserver(",
        // IndexedDB — своя часть с 2026-08-17 (её же исполняет область
        // сервис-воркера): в собранном шиме она обязана стоять между
        // хвостом и его продолжением, ровно один раз.
        "function _idb_schedule_flush()",
        "globalThis.indexedDB",
    ] {
        assert_eq!(
            shim.matches(marker).count(),
            1,
            "{marker} must appear exactly once in the assembled shim"
        );
        let at = shim.find(marker).expect(marker);
        assert!(at > prev, "{marker} is out of source order");
        prev = at;
    }
}

/// The worker-facing subset is literally the same source the page gets —
/// not a paraphrase of it. Comparing the strings (rather than probing the
/// resulting objects) is what actually rules out the drift BUG-401 was
/// filed about.
#[cfg(feature = "v8-backend")]
#[test]
fn worker_exposed_shim_is_a_verbatim_slice_of_the_page_shim() {
    let page = web_api_shim();
    assert!(page.contains(EVENT_TARGET_SHIM));
    assert!(page.contains(PERFORMANCE_SHIM));
    let worker = worker_exposed_shim();
    assert!(worker.starts_with(EVENT_TARGET_SHIM));
    assert!(worker.contains(PERFORMANCE_SHIM));
    // Интерфейсы `[Exposed=Worker]` в странице появиться не должны: это
    // единственная часть воркерного шима, которой в странице нет (BUG-776).
    assert!(worker.contains(WORKER_LOCATION_NAVIGATOR_SHIM));
    assert!(!page.contains("globalThis.WorkerLocation"));
    assert!(!page.contains("globalThis.WorkerNavigator"));
}

#[cfg(feature = "v8-backend")]
fn make_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let head = doc.create_element(QualName::html("head"));
    let title = doc.create_element(QualName::html("title"));
    let title_text = doc.create_text("Test Page");
    let body = doc.create_element(QualName::html("body"));
    let div = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
        attrs.push(lumen_dom::Attribute {
            name: QualName::html("id"),
            value: "main".into(),
        });
    }
    let span = doc.create_element(QualName::html("span"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(span).data {
        attrs.push(lumen_dom::Attribute {
            name: QualName::html("class"),
            value: "highlight".into(),
        });
    }
    let text = doc.create_text("Hello");
    doc.append_child(doc.root(), html);
    doc.append_child(html, head);
    doc.append_child(head, title);
    doc.append_child(title, title_text);
    doc.append_child(html, body);
    doc.append_child(body, div);
    doc.append_child(div, span);
    doc.append_child(span, text);
    Arc::new(Mutex::new(doc))
}

#[cfg(feature = "v8-backend")]
mod v8_trusted_types;

#[cfg(feature = "v8-backend")]
mod v8_fullscreen_locks;

#[cfg(feature = "v8-backend")]
mod v8_generic_sensor;

#[cfg(feature = "v8-backend")]
mod v8_selector_syntax_error;

#[cfg(feature = "v8-backend")]
mod v8_css_storage_nav_misc;

#[cfg(feature = "v8-backend")]
mod v8_perf_typedom_node;

#[cfg(feature = "v8-backend")]
mod v8_dragdrop_scroll_pointer;

#[cfg(feature = "v8-backend")]
mod v8_pointer_lock;

#[cfg(feature = "v8-backend")]
mod v8_core;
#[cfg(feature = "v8-backend")]
mod v8_events_cache;

#[cfg(feature = "v8-backend")]
mod v8_inline_event_handlers;

#[cfg(feature = "v8-backend")]
mod v8_ws_sse;

#[cfg(feature = "v8-backend")]
mod v8_nav_url_storage;

#[cfg(feature = "v8-backend")]
mod v8_perf_observers;

#[cfg(feature = "v8-backend")]
mod v8_childnode_traversal;

#[cfg(feature = "v8-backend")]
mod v8_matchmedia;

#[cfg(feature = "v8-backend")]
mod v8_elem_geometry_scroll;

#[cfg(feature = "v8-backend")]
mod v8_point_hit_test;

#[cfg(feature = "v8-backend")]
mod v8_lazy_image_io;

#[cfg(feature = "v8-backend")]
mod v8_fontface_shadow_custom;

#[cfg(feature = "v8-backend")]
mod v8_window_anim_compress;

#[cfg(feature = "v8-backend")]
mod v8_idb;

#[cfg(feature = "v8-backend")]
mod v8_formdata;

#[cfg(feature = "v8-backend")]
mod v8_selection_range_editing;

#[cfg(feature = "v8-backend")]
mod v8_computedstyle;

#[cfg(feature = "v8-backend")]
mod v8_bug387_computed_style_map;

#[cfg(feature = "v8-backend")]
mod v8_bug732_node_and_collections;

#[cfg(feature = "v8-backend")]
mod v8_bug377_base_uri;

#[cfg(feature = "v8-backend")]
mod v8_webcrypto;

#[cfg(feature = "v8-backend")]
mod v8_url_abort_clone_blob;

#[cfg(feature = "v8-backend")]
mod v8_page_visibility_beacon;
#[cfg(feature = "v8-backend")]
mod v8_event_classes;
#[cfg(feature = "v8-backend")]
mod v8_whatwg_streams;

#[cfg(feature = "v8-backend")]
mod v8_details_dialog_popover;
#[cfg(feature = "v8-backend")]
mod v8_form_constraint_validation;
#[cfg(feature = "v8-backend")]
mod v8_idle_message_clipboard;

#[cfg(feature = "v8-backend")]
mod v8_webworker;
