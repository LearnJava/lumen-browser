//! Тесты `v8_perf_typedom_node`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-2).

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

// ── Resource Timing L2 tests (E-2) ─────────────────────────────────────────

#[test]
fn resource_timing_record_exists_in_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_record_resource_timing('https://example.com/app.js', 'script', 1000, 50);
                var entries = performance.getEntriesByType('resource');
                entries.length === 1 && entries[0].name === 'https://example.com/app.js'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_entry_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_record_resource_timing('https://cdn.example.com/style.css', 'link', 500, 80);
                var e = performance.getEntriesByType('resource')[0];
                e.entryType === 'resource' &&
                e.initiatorType === 'link' &&
                e.fetchStart === 500 &&
                e.responseEnd === 580 &&
                e.duration === 80
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_phase0_sub_timings_equal_fetch_start() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_record_resource_timing('https://example.com/img.png', 'img', 200, 30);
                var e = performance.getEntriesByType('resource')[0];
                e.domainLookupStart === 200 &&
                e.domainLookupEnd === 200 &&
                e.connectStart === 200 &&
                e.requestStart === 200 &&
                e.responseStart === 200
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_clear_resource_timings() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 100, 10);
                _lumen_record_resource_timing('https://example.com/b.js', 'script', 200, 20);
                performance.clearResourceTimings();
                performance.getEntriesByType('resource').length === 0
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_observer_notified() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
                po.observe({entryTypes: ['resource']});
                _lumen_record_resource_timing('https://example.com/fetch.json', 'fetch', 300, 15);
                got.length === 1 && got[0].initiatorType === 'fetch' && got[0].duration === 15
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_multiple_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_record_resource_timing('https://example.com/1.js', 'script', 100, 10);
                _lumen_record_resource_timing('https://example.com/2.js', 'script', 200, 20);
                _lumen_record_resource_timing('https://example.com/3.css', 'link', 300, 5);
                var all = performance.getEntriesByType('resource');
                all.length === 3 && all[2].name === 'https://example.com/3.css' && all[2].initiatorType === 'link'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── Resource Timing buffer + droppedEntriesCount (BUG-839) ────────────────

#[test]
fn resource_timing_buffer_size_limit_keeps_entry_out_of_buffer() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                performance.setResourceTimingBufferSize(0);
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 1);
                _lumen_tick_timers();
                performance.getEntriesByType('resource').length === 0
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_observer_sees_entry_the_buffer_refused() {
    // The buffer and the observer stream are separate sinks: a
    // zero-sized buffer must not silence PerformanceObserver.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                performance.setResourceTimingBufferSize(0);
                var seen = 0;
                new PerformanceObserver(function(l) { seen += l.getEntries().length; })
                    .observe({type: 'resource'});
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 1);
                _lumen_tick_timers();
                seen === 1 && performance.getEntriesByType('resource').length === 0
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_buffer_full_event_is_a_queued_task() {
    // Queued, not inline: the page assigns the handler after the load
    // that overflows the buffer just as often as before it.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                performance.setResourceTimingBufferSize(1);
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 1);
                _lumen_record_resource_timing('https://example.com/b.js', 'script', 20, 1);
                var fired = 0;
                performance.onresourcetimingbufferfull = function() { fired++; };
                var beforeTick = fired;
                _lumen_tick_timers();
                beforeTick === 0 && fired === 1
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_buffer_full_listener_can_make_room() {
    // The spec's loop: a handler that clears the buffer gets the
    // entries that were about to be dropped copied in instead.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                performance.setResourceTimingBufferSize(1);
                performance.addEventListener('resourcetimingbufferfull', function() {
                    performance.clearResourceTimings();
                });
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 1);
                _lumen_record_resource_timing('https://example.com/b.js', 'script', 20, 1);
                _lumen_tick_timers();
                var names = performance.getEntriesByType('resource').map(function(e) { return e.name; });
                names.length === 1 && names[0] === 'https://example.com/b.js'
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_onbufferfull_is_an_idl_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                ('onresourcetimingbufferfull' in performance) &&
                performance.onresourcetimingbufferfull === null
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_callback_takes_three_arguments() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                var argc = -1, opts = null;
                new PerformanceObserver(function(list, obs, options) {
                    argc = arguments.length; opts = options;
                }).observe({type: 'mark'});
                performance.mark('m');
                argc === 3 && opts !== null && opts.droppedEntriesCount === 0
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dropped_entries_count_reported_once_per_observe() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                performance.setResourceTimingBufferSize(0);
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 1);
                _lumen_tick_timers();
                var counts = [];
                var po = new PerformanceObserver(function(list, obs, options) {
                    counts.push(options.droppedEntriesCount);
                });
                po.observe({type: 'resource'});
                _lumen_record_resource_timing('https://example.com/b.js', 'script', 20, 1);
                _lumen_record_resource_timing('https://example.com/c.js', 'script', 30, 1);
                _lumen_tick_timers();
                counts.length === 2 && counts[0] === 1 && counts[1] === undefined
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dropped_entries_count_zero_for_an_unbounded_type() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                performance.setResourceTimingBufferSize(0);
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 1);
                _lumen_tick_timers();
                var got = -1;
                new PerformanceObserver(function(list, obs, options) {
                    got = options.droppedEntriesCount;
                }).observe({type: 'mark'});
                performance.mark('m');
                got === 0
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resource_timing_entry_detail_and_tojson() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 5,
                    { status: 200, decodedBodySize: 700, encodedBodySize: 300,
                      contentType: 'text/javascript', nextHopProtocol: 'http/1.1' });
                var e = performance.getEntriesByType('resource')[0];
                var j = JSON.parse(JSON.stringify(e));
                e.responseStatus === 200 && e.decodedBodySize === 700 &&
                e.encodedBodySize === 300 && e.transferSize === 600 &&
                e.nextHopProtocol === 'http/1.1' &&
                j.initiatorType === 'script' && j.responseEnd === 15
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn shell_delivered_rows_become_resource_entries() {
    // The shell hands unix-epoch milliseconds; a load that finished
    // before this runtime existed clamps to 0 rather than going
    // negative.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"
                var origin = performance.timeOrigin;
                _lumen_deliver_resource_timings(JSON.stringify([
                    { url: 'https://example.com/i.png', initiatorType: 'img',
                      startMs: origin + 40, durationMs: 12, status: 200,
                      decodedBodySize: 64, encodedBodySize: 64 },
                    { url: 'https://example.com/early.css', initiatorType: 'css',
                      startMs: origin - 500, durationMs: 3, status: 200 }
                ]));
                var all = performance.getEntriesByType('resource');
                all.length === 2 && all[0].startTime === 40 && all[0].initiatorType === 'img' &&
                all[1].startTime === 0 && all[1].initiatorType === 'css'
                "#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── _lumen_deliver_perf_entry generic binding tests (O-2) ──────────────────

#[test]
fn deliver_perf_entry_basic_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('longtask', 'self', 500.0, 75.0, null);
                var e = performance.getEntriesByType('longtask')[0];
                e.entryType === 'longtask' && e.name === 'self' && e.startTime === 500 && e.duration === 75
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn deliver_perf_entry_detail_json_merged() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('element', 'img', 200.0, 0.0, '{"renderTime":210,"loadTime":205,"identifier":"hero"}');
                var e = performance.getEntriesByType('element')[0];
                e.renderTime === 210 && e.loadTime === 205 && e.identifier === 'hero'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn deliver_perf_entry_notifies_observer() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
                po.observe({entryTypes: ['navigation']});
                _lumen_deliver_perf_entry('navigation', 'self', 100.0, 60.0, null);
                got.length === 1 && got[0].entryType === 'navigation'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn deliver_perf_entry_invalid_json_still_delivers() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('event', 'click', 300.0, 5.0, '{not valid json}');
                var e = performance.getEntriesByType('event')[0];
                e !== undefined && e.entryType === 'event' && e.startTime === 300
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn deliver_perf_entry_empty_detail_json_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('navigation', 'https://example.com/', 0.0, 800.0, '{}');
                var e = performance.getEntriesByType('navigation')[0];
                e.entryType === 'navigation' && e.duration === 800
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── Navigation Timing L2 tests (II-1) ─────────────────────────────────────

#[test]
fn nav_timing_observer_receives_navigation_entry() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
                po.observe({entryTypes: ['navigation']});
                _lumen_deliver_perf_entry('navigation', 'https://example.com/', 0.0, 350.0, null);
                got.length === 1 && got[0].entryType === 'navigation' && got[0].duration === 350
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn nav_timing_start_time_is_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('navigation', 'https://lumen.test/', 0.0, 120.0, null);
                var e = performance.getEntriesByType('navigation')[0];
                e.startTime === 0
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn nav_timing_name_is_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('navigation', 'https://lumen.test/page', 0.0, 200.0, null);
                performance.getEntriesByType('navigation')[0].name === 'https://lumen.test/page'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn nav_timing_buffered_replay() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                _lumen_deliver_perf_entry('navigation', 'https://buffered.test/', 0.0, 500.0, null);
                var got = [];
                var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
                po.observe({entryTypes: ['navigation'], buffered: true});
                got.length === 1 && got[0].name === 'https://buffered.test/'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── CSS Typed OM L1 tests (A-3 feature) ────────────────────────────────────
#[test]
fn css_typed_om_css_style_value_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof CSS.CSSStyleValue === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_css_unit_value_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof CSS.CSSUnitValue === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_css_keyword_value_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof CSS.CSSKeywordValue === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_element_attribute_style_map_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.documentElement.attributeStyleMap !== null && typeof document.documentElement.attributeStyleMap === 'object'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_element_computed_style_map_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof document.documentElement.computedStyleMap === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_set_get_property() {
    let rt = v8_runtime_with_dom(make_doc());
    // First, check that documentElement exists
    let r = rt.eval("document.documentElement !== null").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));

    // Check basic style property
    let r2 = rt.eval("typeof document.documentElement.style === 'object'").unwrap();
    assert_eq!(r2, lumen_core::JsValue::Bool(true));
}

// BUG-281: document/element DOM-tree shape gaps broke react-dom's root-identity
// checks (`document.nodeType`, `documentElement.tagName`, `ownerDocument` identity,
// `namespaceURI`). Each assertion below mirrors one row of the bug's repro table.
#[test]
fn bug_281_document_node_type_is_document_node() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.nodeType === 9").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug_281_document_element_is_html_not_document() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.documentElement.tagName === 'HTML'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug_281_element_owner_document_identity() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("document.getElementById('main').ownerDocument === document")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug_281_element_namespace_uri_is_xhtml() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("document.getElementById('main').namespaceURI === 'http://www.w3.org/1999/xhtml'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug_281_text_node_namespace_uri_is_null() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("document.getElementsByTagName('title')[0].firstChild.namespaceURI === null")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_has_property() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var el = document.documentElement;
                typeof el.attributeStyleMap.has === 'function'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_delete_property() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var el = document.documentElement;
                typeof el.attributeStyleMap.delete === 'function'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_css_unit_value_value_and_unit() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var uv = new CSS.CSSUnitValue(42, 'em');
                typeof uv.value === 'number' && typeof uv.unit === 'string'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_css_unit_value_to_method() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                // 'em' needs a resolution context and now throws (BUG-387);
                // 'pt' is in the same absolute-length group as 'px'.
                var uv = new CSS.CSSUnitValue(10, 'px');
                typeof uv.to === 'function' && uv.to('pt') instanceof CSS.CSSUnitValue
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_style_property_map_keys_values() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var el = document.documentElement;
                typeof el.attributeStyleMap.keys === 'function' && typeof el.attributeStyleMap.values === 'function'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_typed_om_computed_style_property_map_is_read_only() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var el = document.documentElement;
                var computed = el.computedStyleMap();
                // §6.1 declares no write surface at all, so `set`/`delete` are
                // absent rather than present-and-throwing (BUG-387).
                computed !== null && typeof computed === 'object'
                    && typeof computed.set === 'undefined'
                    && typeof computed.delete === 'undefined'
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── DOM node count / limit bindings ───────────────────────────────────────

#[test]
fn dom_node_count_binding_returns_positive() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_lumen_dom_node_count() > 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dom_node_count_increments_after_create_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var before = _lumen_dom_node_count();
                document.createElement('span');
                _lumen_dom_node_count() === before + 1
                "#
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dom_node_count_at_max_after_prefill() {
    let doc = {
        use lumen_dom::{Document, QualName};
        let mut d = Document::new();
        while d.node_count() < lumen_dom::MAX_DOM_NODES {
            d.create_element(QualName::html("div"));
        }
        // Verify prefill worked
        assert_eq!(d.node_count(), lumen_dom::MAX_DOM_NODES);
        Arc::new(Mutex::new(d))
    };
    let rt = v8_runtime_with_dom(doc);
    // The binding should reflect the pre-filled count
    let r = rt.eval(&format!("_lumen_dom_node_count() >= {}", lumen_dom::MAX_DOM_NODES)).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dom_create_element_throws_quota_exceeded_when_full() {
    // Pre-fill the arena to MAX_DOM_NODES via the Rust API so the JS
    // binding returns the error sentinel without 50 000 JS evals.
    let doc = {
        use lumen_dom::{Document, QualName};
        let mut d = Document::new();
        while d.node_count() < lumen_dom::MAX_DOM_NODES {
            d.create_element(QualName::html("div"));
        }
        Arc::new(Mutex::new(d))
    };
    let rt = v8_runtime_with_dom(doc);
    // QuickJS converts Rust u32::MAX to -1 (signed overflow), so the shim
    // now checks `nid < 0` and throws QuotaExceededError.
    let r = rt.eval(
        r#"
                var caught = '';
                try { document.createElement('p'); }
                catch (e) { caught = e.name; }
                caught
                "#,
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("QuotaExceededError".into()));
}

#[test]
fn dom_create_text_node_throws_quota_exceeded_when_full() {
    // BUG-418: createTextNode/createComment were completely ungated —
    // unlike createElement they never checked MAX_DOM_NODES at all.
    let doc = {
        use lumen_dom::{Document, QualName};
        let mut d = Document::new();
        while d.node_count() < lumen_dom::MAX_DOM_NODES {
            d.create_element(QualName::html("div"));
        }
        Arc::new(Mutex::new(d))
    };
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        r#"
                var caught = '';
                try { document.createTextNode('x'); }
                catch (e) { caught = e.name; }
                caught
                "#,
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("QuotaExceededError".into()));
}

#[test]
fn dom_create_comment_throws_quota_exceeded_when_full() {
    let doc = {
        use lumen_dom::{Document, QualName};
        let mut d = Document::new();
        while d.node_count() < lumen_dom::MAX_DOM_NODES {
            d.create_element(QualName::html("div"));
        }
        Arc::new(Mutex::new(d))
    };
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        r#"
                var caught = '';
                try { document.createComment('x'); }
                catch (e) { caught = e.name; }
                caught
                "#,
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("QuotaExceededError".into()));
}

#[test]
fn native_binding_panic_does_not_abort_process() {
    // BUG-418: an invalid NodeId reaching `Document::get`/`get_mut` used to
    // panic inside V8's `extern "C"` callback boundary, which Rust refuses
    // to unwind through ("panic in a function that cannot unwind") and
    // aborts the whole process. `native_fn_trampoline` now wraps native
    // dispatch in `catch_unwind`, turning that into a catchable JS error —
    // if this test runs at all (rather than aborting the test binary), the
    // guard is in place; the assertions confirm the error surfaces to JS.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        r#"
                var caught = '';
                try { _lumen_append_child(0, 4294967295); }
                catch (e) { caught = e.name; }
                caught
                "#,
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("Error".into()));
}

// ── D-6: chrome.runtime stub tests ───────────────────────────────────────

#[test]
fn chrome_runtime_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("typeof chrome !== 'undefined' && typeof chrome.runtime !== 'undefined'").unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn chrome_runtime_send_message_is_function() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("typeof chrome.runtime.sendMessage").unwrap();
    assert_eq!(v, lumen_core::JsValue::String("function".into()));
}

#[test]
fn chrome_runtime_send_message_does_not_throw() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var ok = false;
                try { chrome.runtime.sendMessage({type: 'test'}); ok = true; } catch(e) {}
                ok
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn chrome_runtime_on_message_add_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval(r#"
                var called = false;
                chrome.runtime.onMessage.addListener(function(msg) { called = true; });
                chrome.runtime.onMessage._listeners.length === 1
            "#).unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn browser_runtime_alias_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("typeof browser !== 'undefined' && typeof browser.runtime !== 'undefined'").unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

#[test]
fn chrome_runtime_get_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let v = rt.eval("chrome.runtime.getURL('icons/icon.png')").unwrap();
    assert_eq!(v, lumen_core::JsValue::String(
        "chrome-extension://lumen-extension/icons/icon.png".into()
    ));
}
