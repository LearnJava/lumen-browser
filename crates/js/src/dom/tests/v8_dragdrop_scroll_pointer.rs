//! Тесты `v8_dragdrop_scroll_pointer`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-2).

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

// ── HTML5 Drag and Drop API (HTML LS §9.10) ───────────────────────────────

#[test]
fn data_transfer_set_get_data() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var dt = new DataTransfer();
                dt.setData('text/plain', 'hello drag');
                dt.getData('text/plain')
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::String("hello drag".into()));
}

#[test]
fn data_transfer_normalises_text_format() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var dt = new DataTransfer();
                dt.setData('text', 'world');
                dt.getData('text/plain')
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::String("world".into()));
}

#[test]
fn data_transfer_types_reflect_set_data() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var dt = new DataTransfer();
                dt.setData('text/plain', 'a');
                dt.setData('text/html', '<b>a</b>');
                dt.types.length === 2 && dt.types.indexOf('text/plain') >= 0
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn data_transfer_clear_data_single_format() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var dt = new DataTransfer();
                dt.setData('text/plain', 'a');
                dt.setData('text/html', '<b>a</b>');
                dt.clearData('text/plain');
                dt.types.length === 1 && dt.types[0] === 'text/html'
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn data_transfer_item_list_add_and_iterate() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var dt = new DataTransfer();
                dt.items.add('foo', 'text/plain');
                dt.items.length === 1 && dt.items[0].kind === 'string'
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn data_transfer_item_get_as_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var dt = new DataTransfer();
                dt.setData('text/plain', 'payload');
                var got = null;
                dt.items[0].getAsString(function(s) { got = s; });
                got
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::String("payload".into()));
}

#[test]
fn drag_event_has_fresh_data_transfer() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var e = new DragEvent('dragstart', { bubbles: true });
                e.dataTransfer instanceof DataTransfer
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn draggable_attribute_getter_setter() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var el = document.createElement('div');
                document.body.appendChild(el);
                el.draggable = true;
                el.draggable === true && el.getAttribute('draggable') === 'true'
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn data_transfer_classes_exported_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                typeof window.DataTransfer === 'function' &&
                typeof window.DataTransferItem === 'function' &&
                typeof window.DataTransferItemList === 'function'
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn lumen_dispatch_drag_event_fires_on_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var el = document.getElementById('main');
                var fired = false;
                el.addEventListener('dragstart', function(e) { fired = true; });
                var nid = el.__nid__;
                _lumen_dispatch_drag_event(nid, 'dragstart', 10, 20, '{}');
                fired
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn lumen_dispatch_drag_event_passes_coordinates() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var el = document.getElementById('main');
                var cx = -1, cy = -1;
                el.addEventListener('drag', function(e) { cx = e.clientX; cy = e.clientY; });
                _lumen_dispatch_drag_event(el.__nid__, 'drag', 55, 77, '{}');
                cx === 55 && cy === 77
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn lumen_dispatch_drag_event_populates_data_transfer() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var el = document.getElementById('main');
                var payload = '';
                el.addEventListener('drop', function(e) {
                    payload = e.dataTransfer.getData('text/plain');
                });
                _lumen_dispatch_drag_event(el.__nid__, 'drop', 0, 0, '{"text/plain":"transferred"}');
                payload
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::String("transferred".into()));
}

#[test]
fn lumen_dispatch_drag_event_bubbles_to_parent() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var parent = document.getElementById('main');
                var child = document.createElement('div');
                parent.appendChild(child);
                var bubbled = false;
                parent.addEventListener('dragover', function() { bubbled = true; });
                _lumen_dispatch_drag_event(child.__nid__, 'dragover', 0, 0, '{}');
                bubbled
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn drag_event_default_not_prevented_without_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var el = document.getElementById('main');
                // returns true when default is not prevented
                _lumen_dispatch_drag_event(el.__nid__, 'dragstart', 0, 0, '{}')
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

// ── window scroll API (CSSOM View Module §4) ─────────────────────────────

#[test]
fn window_scroll_y_initially_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("window.scrollY").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(0.0));
}

#[test]
fn window_page_y_offset_alias() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.set_page_scroll_y(150.0);
    let v = rt.eval("window.pageYOffset").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(150.0));
}

#[test]
fn window_scroll_to_instant_queues_page_request() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.scrollTo(0, 500)").unwrap();
    let reqs = rt.take_page_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert!((reqs[0].0 - 500.0).abs() < 0.1, "target_y should be 500");
    assert!(!reqs[0].1, "smooth should be false for instant scroll");
}

#[test]
fn window_scroll_to_smooth_sets_smooth_flag() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.scrollTo({ top: 300, behavior: 'smooth' })").unwrap();
    let reqs = rt.take_page_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert!((reqs[0].0 - 300.0).abs() < 0.1, "target_y should be 300");
    assert!(reqs[0].1, "smooth should be true");
}

#[test]
fn window_scroll_by_adds_to_current_page_scroll() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.set_page_scroll_y(200.0);
    rt.eval("window.scrollBy(0, 100)").unwrap();
    let reqs = rt.take_page_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert!((reqs[0].0 - 300.0).abs() < 0.1, "target_y should be 300 (200+100)");
}

#[test]
fn window_scroll_alias_works() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.scroll(0, 400)").unwrap();
    let reqs = rt.take_page_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert!((reqs[0].0 - 400.0).abs() < 0.1);
}

#[test]
fn window_scroll_x_is_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("window.scrollX").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(0.0));
}

// ── BUG-821: the page 'scroll' event follows the position, not the wheel ──

/// The shell calls `set_page_scroll_y` once per rendering update, so the
/// «did it move» decision has to live here — otherwise every frame of a
/// still page would queue a `scroll` event.
#[test]
fn set_page_scroll_y_reports_only_real_movement() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(rt.set_page_scroll_y(300.0), "first move must report a change");
    assert!(!rt.set_page_scroll_y(300.0), "the same position is not a change");
    assert!(rt.set_page_scroll_y(0.0), "scrolling back to the top is a change");
}

/// The other end of that chain: what the shell fires when the position
/// did move has to reach an ordinary `window` listener.
#[test]
fn fire_window_scroll_reaches_a_window_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__scrolls = 0;
                 window.addEventListener('scroll', function() { globalThis.__scrolls++; });",
    )
    .unwrap();
    rt.fire_window_scroll();
    let v = rt.eval("__scrolls").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(1.0));
}

/// `scrollIntoView` on an element whose ancestors are all unscrollable
/// must scroll the viewport — it used to walk off the ancestor loop and
/// do nothing at all.
#[test]
fn scroll_into_view_without_scrollable_ancestor_scrolls_the_page() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = match rt.eval("document.getElementById('main').__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected a numeric nid, got {other:?}"),
    };
    // Layout rects are document coordinates: the element sits 900 CSS px
    // down the page, so `block: start` means scrolling the page to 900.
    let mut rects = std::collections::HashMap::new();
    rects.insert(nid, [0.0_f32, 900.0, 100.0, 20.0]);
    rt.update_layout_rects(rects);
    rt.eval("document.getElementById('main').scrollIntoView()").unwrap();
    let reqs = rt.take_page_scroll_requests();
    assert_eq!(reqs.len(), 1, "one page-scroll request expected");
    assert!((reqs[0].0 - 900.0).abs() < 0.1, "target_y should be 900, got {}", reqs[0].0);
    assert!(!reqs[0].1, "scrollIntoView is instant until BUG-479 honours its options");
}

// ── BUG-822: `scrollend` closes the sequence `scroll` opened ──

/// An instant page scroll is one complete sequence, so the frame that
/// moves the page also owes the `scrollend`; a frame that moves nothing
/// owes nothing.
#[test]
fn page_scrollend_is_due_on_an_instant_scroll() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(rt.page_scrollend_due(true, true), "an instant move ends its own sequence");
    assert!(!rt.page_scrollend_due(false, true), "a still page owes nothing");
}

/// While an animation/momentum is still driving the position the debt
/// accumulates instead: exactly one `scrollend` at the end, not one per
/// frame.
#[test]
fn page_scrollend_waits_for_the_animation_to_settle() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(!rt.page_scrollend_due(true, false), "mid-animation frame owes nothing yet");
    assert!(!rt.page_scrollend_due(true, false), "still mid-animation");
    assert!(rt.page_scrollend_due(true, true), "the frame that settles pays the debt");
    assert!(!rt.page_scrollend_due(false, true), "and the debt is paid only once");
}

/// The final frame of touch momentum can clamp at the edge and move
/// nothing at all — the sequence still ended, so the debt taken on
/// earlier frames must still be paid.
#[test]
fn page_scrollend_is_paid_even_if_the_last_frame_does_not_move() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(!rt.page_scrollend_due(true, false));
    assert!(rt.page_scrollend_due(false, true), "settling with a debt still fires");
}

/// The dispatch end of the same chain, for both targets.
#[test]
fn fire_window_scrollend_reaches_a_window_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "globalThis.__ends = 0;
                 window.addEventListener('scrollend', function() { globalThis.__ends++; });",
    )
    .unwrap();
    rt.fire_window_scrollend();
    let v = rt.eval("__ends").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(1.0));
}

#[test]
fn fire_element_scrollend_reaches_an_element_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = match rt.eval("document.getElementById('main').__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected a numeric nid, got {other:?}"),
    };
    rt.eval(
        "globalThis.__elEnds = 0;
                 document.getElementById('main')
                     .addEventListener('scrollend', function() { globalThis.__elEnds++; });",
    )
    .unwrap();
    rt.fire_element_scrollend(nid);
    let v = rt.eval("__elEnds").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(1.0));
}

/// The feature test a page runs before it decides it may wait for the
/// end of a scroll: the name has to be *present*, not merely assignable.
#[test]
fn onscrollend_is_detectable_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval("('onscrollend' in window) && ('onscroll' in window) && window.onscrollend === null")
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

/// …and an assignment to it is what the dispatch actually calls.
#[test]
fn window_onscrollend_handler_is_invoked() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("globalThis.__viaProp = 0; window.onscrollend = function() { globalThis.__viaProp++; };")
        .unwrap();
    rt.fire_window_scrollend();
    let v = rt.eval("__viaProp").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(1.0));
}

#[test]
fn print_request_default_values() {
    let req = PrintRequest::default();
    assert_eq!(req.margin_top, 48.0);
    assert_eq!(req.margin_bottom, 48.0);
    assert_eq!(req.margin_left, 48.0);
    assert_eq!(req.margin_right, 48.0);
    assert_eq!(req.paper_width_in, 8.5);
    assert_eq!(req.paper_height_in, 11.0);
    assert_eq!(req.output_path, None);
}

#[test]
fn multiple_print_calls_accumulate() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.print()").unwrap();
    rt.eval("window.print()").unwrap();
    let reqs = rt.take_print_requests();
    assert_eq!(reqs.len(), 2);
}

// ── JJ Phase 5: Modern HTML5 APIs ────────────────────────────────────────

#[test]
fn set_html_unsafe_sets_content() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     document.body.appendChild(d);\
                     d.setHTMLUnsafe('<p>hello</p>');\
                     d.innerHTML === '<p>hello</p>'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_html_returns_inner_html() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     document.body.appendChild(d);\
                     d.innerHTML = '<span>world</span>';\
                     d.getHTML() === '<span>world</span>'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_html_with_options_phase0() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     document.body.appendChild(d);\
                     d.innerHTML = 'test';\
                     d.getHTML({serializableShadowRoots: true}) === 'test'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// ── BUG-368: innerHTML must parse/serialize real markup, not textContent ──

#[test]
fn inner_html_setter_parses_elements_not_text() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     document.body.appendChild(d);\
                     d.innerHTML = '<i id=\"x\">y</i>';\
                     d.children.length === 1 && \
                     d.firstElementChild.tagName === 'I' && \
                     d.firstElementChild.id === 'x' && \
                     d.firstElementChild.textContent === 'y'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn inner_html_round_trips_elements_attrs_comments_and_text() {
    let rt = v8_runtime_with_dom(make_doc());
    // Same repro tree as bugs/BUG-368-OPEN.md's probe.
    let ok = rt
        .eval(
            "var host = document.createElement('div');\
                     document.body.appendChild(host);\
                     host.innerHTML = '<span id=\"a\" class=\"c\">A</span><!--k--><b>B</b>tail';\
                     host.childNodes.length === 4 && \
                     host.children.length === 2 && \
                     host.firstElementChild.tagName === 'SPAN' && \
                     host.innerHTML === '<span id=\"a\" class=\"c\">A</span><!--k--><b>B</b>tail'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// ── BUG-351: outerHTML / insertAdjacentHTML ────────────────────────────

#[test]
fn outer_html_getter_serializes_element_itself() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     d.id = 'x';\
                     d.innerHTML = '<b>y</b>';\
                     d.outerHTML === '<div id=\"x\"><b>y</b></div>'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn outer_html_setter_replaces_element_in_parent() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var parent = document.createElement('div');\
                     document.body.appendChild(parent);\
                     parent.innerHTML = '<p id=\"old\">a</p>';\
                     parent.firstElementChild.outerHTML = '<span id=\"new\">b</span>';\
                     parent.children.length === 1 && \
                     parent.firstElementChild.tagName === 'SPAN' && \
                     parent.firstElementChild.id === 'new'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn outer_html_setter_throws_on_document_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var threw = false;\
                     try { document.documentElement.outerHTML = '<html></html>'; } \
                     catch (e) { threw = e.name === 'NoModificationAllowedError'; }\
                     threw",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn insert_adjacent_html_inserts_parsed_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var parent = document.createElement('div');\
                     document.body.appendChild(parent);\
                     var el = document.createElement('a');\
                     parent.appendChild(el);\
                     el.insertAdjacentHTML('afterend', '<span id=\"z\">Z</span>');\
                     parent.children.length === 2 && \
                     parent.lastElementChild.tagName === 'SPAN' && \
                     parent.lastElementChild.id === 'z'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn move_before_rearranges_children() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var parent = document.createElement('div');\
                     document.body.appendChild(parent);\
                     var a = document.createElement('span'); a.id = 'a';\
                     var b = document.createElement('span'); b.id = 'b';\
                     var c = document.createElement('span'); c.id = 'c';\
                     parent.appendChild(a); parent.appendChild(b); parent.appendChild(c);\
                     parent.moveBefore(c, b);\
                     var kids = parent.children;\
                     kids[0].id === 'a' && kids[1].id === 'c' && kids[2].id === 'b'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn move_before_null_appends_to_end() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var parent = document.createElement('div');\
                     document.body.appendChild(parent);\
                     var a = document.createElement('span'); a.id = 'a';\
                     var b = document.createElement('span'); b.id = 'b';\
                     parent.appendChild(a); parent.appendChild(b);\
                     parent.moveBefore(a, null);\
                     var kids = parent.children;\
                     kids[0].id === 'b' && kids[1].id === 'a'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn check_visibility_disconnected_returns_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval("var d = document.createElement('div'); d.checkVisibility() === false")
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn check_visibility_accepts_options_without_throw() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     typeof d.checkVisibility({checkOpacity: true, checkVisibilityCSS: true}) === 'boolean'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_html_unsafe_method_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     typeof d.setHTMLUnsafe === 'function'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_html_method_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     typeof d.getHTML === 'function'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn move_before_method_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     typeof d.moveBefore === 'function'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn check_visibility_method_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.createElement('div');\
                     typeof d.checkVisibility === 'function'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// ── Web Animations API — additional coverage ──────────────────────────────

#[test]
fn wa_document_timeline_current_time_is_number() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("typeof document.timeline.currentTime === 'number' || document.timeline.currentTime === null").unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn wa_document_timeline_class_exposed() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("typeof window.DocumentTimeline === 'function'").unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn wa_animation_playback_rate_default_one() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     a.playbackRate",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(1.0));
}

#[test]
fn wa_animation_ready_is_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     typeof a.ready === 'object' && a.ready !== null",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn wa_element_get_animations_returns_running() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     el.animate([{opacity:'0'},{opacity:'1'}], 500); \
                     el.getAnimations().length",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(1.0));
}

#[test]
fn wa_animation_playback_event_class_exposed() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("typeof window.AnimationPlaybackEvent === 'function'").unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

/// BUG-808: `Animation` must be an `EventTarget` (Web Animations §5.3).
/// Before the fix `addEventListener` was simply absent, so the call threw
/// and took the whole test file with it.
#[test]
fn wa_animation_is_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     a instanceof EventTarget && typeof a.addEventListener === 'function' \
                       && typeof a.removeEventListener === 'function' \
                       && typeof a.dispatchEvent === 'function'",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

/// The listener and the `on<type>` property must BOTH see the finish
/// event — the property alone was all that worked before.
#[test]
fn wa_animation_finish_reaches_listener_and_property() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     var log = []; \
                     a.addEventListener('finish', function() { log.push('L'); }); \
                     a.onfinish = function() { log.push('P'); }; \
                     a.finish(); _lumen_tick_timers(); \
                     log.join(',')",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::String("L,P".into()));
}

/// §4.4.3: the playback events are `AnimationPlaybackEvent`s, not bare
/// `Event`s, and `cancel` carries a null current time (§4.4.1).
#[test]
fn wa_animation_cancel_delivers_playback_event() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     var seen = ''; \
                     a.addEventListener('cancel', function(e) { \
                         seen = e.type + ':' + (e instanceof AnimationPlaybackEvent) + ':' + (e.currentTime === null); \
                     }); \
                     a.cancel(); _lumen_tick_timers(); \
                     seen",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::String("cancel:true:true".into()));
}

/// A removed listener must stop firing — proves the registry is real and
/// not a one-shot shim around the `on<type>` property.
#[test]
fn wa_animation_remove_event_listener_detaches() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     var n = 0; \
                     function h() { n++; } \
                     a.addEventListener('finish', h); \
                     a.removeEventListener('finish', h); \
                     a.finish(); _lumen_tick_timers(); \
                     n",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(0.0));
}

/// `web-animations/interfaces/Animation/style-change-events.html`
/// builds one subtest per `Object.keys(Animation.prototype)` entry, so
/// the three members this fix adds must not be enumerable — otherwise
/// the fix invents three failing subtests named after internals.
#[test]
fn wa_animation_new_prototype_members_are_not_enumerable() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "Object.keys(Animation.prototype) \
                       .filter(function(k) { return k === 'constructor' || k === '_fire' || k === '_onRemove'; }) \
                       .length",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(0.0));
}

/// The accessors declared after the constructor must survive the
/// prototype swap — a swap placed below them would silently drop
/// `currentTime`/`playState` and every method.
#[test]
fn wa_animation_accessors_survive_event_target_prototype() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt
        .eval(
            "var el = document.getElementById('main'); \
                     var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                     a.playState === 'running' && typeof a.pause === 'function' \
                       && a.constructor === Animation",
        )
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

// ── Pointer Events Level 3 §4.1 — pointer capture ────────────────────────

#[test]
fn pointer_event_level3_altitude_azimuth_properties() {
    // L3 PointerEvent must expose altitudeAngle=π/2 and azimuthAngle=0 for mouse.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var got = null; \
                 el.addEventListener('pointerdown', function(e) { got = e; }); \
                 _lumen_dispatch_pointer_event(el.__nid__, 'pointerdown', 0, 0, 0, 1, 0); \
                 Math.abs(got.altitudeAngle - Math.PI / 2) < 0.001 && got.azimuthAngle === 0 && \
                 got.width === 1 && got.height === 1 && \
                 got.tangentialPressure === 0 && got.tiltX === 0 && got.tiltY === 0 && \
                 got.twist === 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointer_event_get_coalesced_events_returns_array() {
    // getCoalescedEvents() must return an array containing the event itself.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var got = null; \
                 el.addEventListener('pointermove', function(e) { got = e; }); \
                 _lumen_dispatch_pointer_event(el.__nid__, 'pointermove', 5, 5, 0, 0, 0); \
                 Array.isArray(got.getCoalescedEvents()) && got.getCoalescedEvents().length === 1 && \
                 Array.isArray(got.getPredictedEvents()) && got.getPredictedEvents().length === 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── Pointer Events Level 3 §4.1 — real coalesced/predicted pointermove ───

#[test]
fn pointer_move_coalesced_dispatch_single_point() {
    // A single-point batch behaves like the non-coalescing dispatcher:
    // getCoalescedEvents() === [the event itself], no predicted events.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var got = null; \
                 el.addEventListener('pointermove', function(e) { got = e; }); \
                 _lumen_dispatch_pointer_move_coalesced(el.__nid__, '[[5,5]]', 0, 0, 0); \
                 var ce = got.getCoalescedEvents(); \
                 ce.length === 1 && ce[0] === got && \
                 got.clientX === 5 && got.clientY === 5 && \
                 got.getPredictedEvents().length === 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointer_move_coalesced_dispatch_multi_point() {
    // A 3-point batch: getCoalescedEvents() has all 3 in order, main event
    // (dispatched, matches the last point) is last in the list by identity;
    // getPredictedEvents() linearly extrapolates 2 points from the last leg.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var got = null; \
                 el.addEventListener('pointermove', function(e) { got = e; }); \
                 _lumen_dispatch_pointer_move_coalesced(el.__nid__, '[[0,0],[10,0],[20,0]]', 0, 0, 0); \
                 var ce = got.getCoalescedEvents(); \
                 var okCoalesced = ce.length === 3 && \
                     ce[0].clientX === 0  && ce[1].clientX === 10 && ce[2].clientX === 20 && \
                     ce[2] === got; \
                 var pe = got.getPredictedEvents(); \
                 var okPredicted = pe.length === 2 && \
                     pe[0].clientX === 30 && pe[1].clientX === 40 && \
                     pe[0].clientY === 0  && pe[1].clientY === 0; \
                 var okMain = got.clientX === 20 && got.clientY === 0 && got.type === 'pointermove'; \
                 okCoalesced && okPredicted && okMain"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointer_move_coalesced_dispatch_empty_batch_is_noop() {
    // An empty batch must not throw and must not dispatch anything.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var fired = false; \
                 el.addEventListener('pointermove', function(e) { fired = true; }); \
                 _lumen_dispatch_pointer_move_coalesced(el.__nid__, '[]', 0, 0, 0); \
                 !fired"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointer_event_coalesced_events_real_batch_and_prediction() {
    // With buffered intermediate samples, getCoalescedEvents() must return
    // every one of them (oldest first) plus the main event last, and
    // getPredictedEvents() must linearly extrapolate from the last two.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var got = null; \
                 el.addEventListener('pointermove', function(e) { got = e; }); \
                 _lumen_dispatch_pointer_event(el.__nid__, 'pointermove', 30, 30, 0, 0, 0, [[10,10],[20,20]]); \
                 var c = got.getCoalescedEvents(); \
                 var p = got.getPredictedEvents(); \
                 Array.isArray(c) && c.length === 3 && \
                 c[0].clientX === 10 && c[0].clientY === 10 && \
                 c[1].clientX === 20 && c[1].clientY === 20 && \
                 c[2] === got && \
                 Array.isArray(p) && p.length === 2 && \
                 p[0].clientX === 40 && p[0].clientY === 40 && \
                 p[1].clientX === 50 && p[1].clientY === 50"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_has_set_pointer_capture_method() {
    // Element must expose setPointerCapture, releasePointerCapture, hasPointerCapture.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 typeof el.setPointerCapture === 'function' && \
                 typeof el.releasePointerCapture === 'function' && \
                 typeof el.hasPointerCapture === 'function'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_pointer_capture_fires_gotpointercapture() {
    // setPointerCapture(1) must fire 'gotpointercapture' on the element (non-bubbling).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var parent = document.createElement('div'); parent.appendChild(el); document.body.appendChild(parent); \
                 var got_on_el = false; var bubbled_to_parent = false; \
                 el.addEventListener('gotpointercapture', function(e) { got_on_el = true; }); \
                 parent.addEventListener('gotpointercapture', function(e) { bubbled_to_parent = true; }); \
                 el.setPointerCapture(1); \
                 got_on_el && !bubbled_to_parent"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn release_pointer_capture_fires_lostpointercapture() {
    // releasePointerCapture(1) must fire 'lostpointercapture' on the element (non-bubbling).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var lost = false; \
                 el.addEventListener('lostpointercapture', function() { lost = true; }); \
                 el.setPointerCapture(1); \
                 el.releasePointerCapture(1); \
                 lost"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_has_ongotpointercapture_handlers() {
    // Element must expose ongotpointercapture and onlostpointercapture as null by default.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('button'); document.body.appendChild(el); \
                 el.ongotpointercapture === null && el.onlostpointercapture === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_capture_event_no_bubble() {
    // _lumen_dispatch_capture_event must fire a non-bubbling PointerEvent.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var parent = document.createElement('div'); document.body.appendChild(parent); \
                 var child = document.createElement('span'); parent.appendChild(child); \
                 var fired = false; var bubbled = false; \
                 child.addEventListener('gotpointercapture', function(e) { fired = true; }); \
                 parent.addEventListener('gotpointercapture', function(e) { bubbled = true; }); \
                 _lumen_dispatch_capture_event(child.__nid__, 'gotpointercapture'); \
                 fired && !bubbled"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
