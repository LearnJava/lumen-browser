//! Тесты `v8_lazy_image_io`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── Lazy image loading ────────────────────────────────────────────────────
// Delivery goes through IntersectionObserver (_lazy_io) created inside
// _lumen_init_lazy_images; _lumen_deliver_intersection_observers() is the
// trigger (called by deliver_layout_observers in shell), not deliver_lazy_images.

#[test]
fn lazy_images_queued_when_in_viewport() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    // Node 5 — place its bounding rect fully within the viewport.
    rt.update_layout_rects([(5, [10.0, 50.0, 200.0, 150.0])].into_iter().collect());
    // Register node 5 as a lazy image.
    rt.eval("_lumen_init_lazy_images([[5, 'photo.jpg']]);").unwrap();
    // Deliver via IntersectionObserver (matches shell's deliver_layout_observers path).
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let reqs = rt.take_lazy_image_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].0, 5);
    assert_eq!(reqs[0].1, "photo.jpg");
}

#[test]
fn lazy_images_not_queued_when_far_below_fold() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    // Node 6 is 3 viewport-heights below the fold (y=1900, margin=600 → root bottom=1200).
    rt.update_layout_rects([(6, [0.0, 1900.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_init_lazy_images([[6, 'far.png']]);").unwrap();
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let reqs = rt.take_lazy_image_requests();
    assert!(reqs.is_empty(), "image 3 viewports below fold must not be loaded yet");
}

#[test]
fn lazy_images_removed_from_map_after_queue() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    rt.update_layout_rects([(7, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_init_lazy_images([[7, 'once.png']]);").unwrap();
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let first = rt.take_lazy_image_requests();
    assert_eq!(first.len(), 1);
    // Second delivery must NOT queue again (image was unobserved after first load).
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let second = rt.take_lazy_image_requests();
    assert!(second.is_empty(), "already-loaded image must not be queued twice");
}

#[test]
fn lazy_images_init_idempotent() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    // Register same image twice in one call — only the first URL is stored.
    rt.eval("_lumen_init_lazy_images([[8, 'dup.png'],[8, 'other.png']]);").unwrap();
    // Place rect out of range (y=5000, root bottom = 600+600 = 1200).
    rt.update_layout_rects([(8, [0.0, 5000.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let reqs = rt.take_lazy_image_requests();
    // Far below the lazy-load margin: not queued yet.
    assert!(reqs.is_empty());
    // Second init with different URL — must be ignored (first registration wins).
    rt.eval("_lumen_init_lazy_images([[8, 'new.png']]);").unwrap();
    // Move into viewport.
    rt.update_layout_rects([(8, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let reqs2 = rt.take_lazy_image_requests();
    assert_eq!(reqs2.len(), 1);
    assert_eq!(reqs2[0].1, "dup.png", "first registration URL must win");
}

#[test]
fn lazy_deliver_lazy_images_is_noop() {
    // _lumen_deliver_lazy_images() must be a no-op; delivery is via IO.
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    rt.update_layout_rects([(9, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_init_lazy_images([[9, 'noop.png']]);").unwrap();
    // Old shell path: this must no longer queue images on its own.
    rt.eval("_lumen_deliver_lazy_images();").unwrap();
    let reqs = rt.take_lazy_image_requests();
    assert!(reqs.is_empty(), "_lumen_deliver_lazy_images must be a no-op");
    // But IO path must still work.
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let reqs2 = rt.take_lazy_image_requests();
    assert_eq!(reqs2.len(), 1);
}

#[test]
fn lazy_images_within_margin_but_below_fold() {
    // Image just below viewport but within 1-viewport-height margin must be loaded.
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    // y=650: just below fold (600), within margin (600+600=1200).
    rt.update_layout_rects([(10, [0.0, 650.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_init_lazy_images([[10, 'near.png']]);").unwrap();
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    let reqs = rt.take_lazy_image_requests();
    assert_eq!(reqs.len(), 1, "image just below fold within margin must be loaded");
}

// ── rootMargin support ────────────────────────────────────────────────────

#[test]
fn root_margin_single_value_expands_all_sides() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_parse_root_margin('10px')").unwrap();
    match r {
        lumen_core::JsValue::Array(a) => {
            assert_eq!(a[0], lumen_core::JsValue::Number(10.0)); // top
            assert_eq!(a[1], lumen_core::JsValue::Number(10.0)); // right
            assert_eq!(a[2], lumen_core::JsValue::Number(10.0)); // bottom
            assert_eq!(a[3], lumen_core::JsValue::Number(10.0)); // left
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn root_margin_two_values_parsed_correctly() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_parse_root_margin('5px 20px')").unwrap();
    match r {
        lumen_core::JsValue::Array(a) => {
            assert_eq!(a[0], lumen_core::JsValue::Number(5.0));  // top
            assert_eq!(a[1], lumen_core::JsValue::Number(20.0)); // right
            assert_eq!(a[2], lumen_core::JsValue::Number(5.0));  // bottom
            assert_eq!(a[3], lumen_core::JsValue::Number(20.0)); // left
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn root_margin_four_values_parsed_correctly() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_parse_root_margin('1px 2px 3px 4px')").unwrap();
    match r {
        lumen_core::JsValue::Array(a) => {
            assert_eq!(a[0], lumen_core::JsValue::Number(1.0)); // top
            assert_eq!(a[1], lumen_core::JsValue::Number(2.0)); // right
            assert_eq!(a[2], lumen_core::JsValue::Number(3.0)); // bottom
            assert_eq!(a[3], lumen_core::JsValue::Number(4.0)); // left
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn root_margin_expands_root_for_element_below_viewport() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Element at y=800, below viewport height of 720.
    rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 100.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    // rootMargin "0px 0px 200px 0px" expands root bottom to 720+200=920.
    // Element top=800 < 920 → intersecting.
    rt.eval(r#"
                var _rm_fired = false;
                var ioRm = new IntersectionObserver(function(entries) {
                    if (entries[0].isIntersecting) _rm_fired = true;
                }, { rootMargin: '0px 0px 200px 0px' });
                ioRm.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let fired = rt.eval("_rm_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true),
        "rootMargin should expand root to detect element below viewport");
}

#[test]
fn root_margin_zero_does_not_see_element_below_viewport() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Element at y=800, below viewport height of 720, no rootMargin.
    rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 100.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _rm_fired2 = false;
                var ioRm2 = new IntersectionObserver(function(entries) {
                    if (entries[0].isIntersecting) _rm_fired2 = true;
                });
                ioRm2.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let fired = rt.eval("_rm_fired2").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(false),
        "without rootMargin, element below viewport must not intersect");
}
