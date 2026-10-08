//! Тесты `v8_perf_observers`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`]: same fixture document, same
/// `install_dom` argument list, same `_LUMEN_EXTENSION_ACTIVE` pre-eval.
pub(super) fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

// ── performance tests ─────────────────────────────────────────────────────

#[test]
fn performance_now_returns_non_negative() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.now() >= 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_now_monotonic() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var t1 = performance.now(); var t2 = performance.now(); t2 >= t1").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_time_origin_positive() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.timeOrigin > 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.performance.now === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-400: Performance is a real interface, not an object literal ──────

/// WPT `hr-time/basic.any.js`, subtest «Performance interface extends
/// EventTarget»: listener registration + dispatch must actually work.
#[test]
fn performance_extends_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var didHandle = false;\
                     performance.addEventListener('testEvent', function() { didHandle = true; }, { once: true });\
                     performance.dispatchEvent(new Event('testEvent'));\
                     didHandle",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The prototype chain, not just the presence of the three methods:
/// a literal carrying copies of `addEventListener` would pass the WPT
/// subtest above while still failing every `instanceof` check.
#[test]
fn performance_prototype_chain_reaches_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "performance instanceof Performance\
                     && performance instanceof EventTarget\
                     && window.Performance === Performance\
                     && Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype\
                     && performance.addEventListener === EventTarget.prototype.addEventListener",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// HR Time L3 §4 declares no constructor, so the exposed interface
/// object must not be callable as one.
#[test]
fn performance_constructor_is_illegal() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("try { new Performance(); false } catch (e) { e instanceof TypeError }")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// WPT `hr-time/performance-tojson.html`, the part Lumen can satisfy:
/// `toJSON()` exists, returns an object and reports `timeOrigin`.
#[test]
fn performance_to_json_reports_time_origin() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var json = performance.toJSON();\
                     typeof performance.toJSON === 'function'\
                     && typeof json === 'object'\
                     && json.timeOrigin === performance.timeOrigin\
                     && JSON.parse(JSON.stringify(performance)).timeOrigin === performance.timeOrigin",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The WebIDL default `toJSON()` serialises attributes only — the
/// operations must not leak into it, which is exactly what moving them
/// off the instance and onto the prototype buys. `timing`/`navigation`
/// (BUG-767, the legacy Navigation Timing L1 partial) are attributes too,
/// so they belong in this set alongside `timeOrigin`.
#[test]
fn performance_to_json_carries_attributes_only() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("Object.keys(performance.toJSON()).join(',')")
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("timeOrigin,timing,navigation".to_string())
    );
}

/// `readonly attribute DOMHighResTimeStamp timeOrigin` — plain
/// assignment from page script must not move the engine's value.
#[test]
fn performance_time_origin_is_readonly() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var before = performance.timeOrigin; performance.timeOrigin = 0; performance.timeOrigin === before")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_mark_stores_entry() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.mark('t1'); performance.getEntriesByType('mark').length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn performance_mark_returns_entry_name() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.mark('mymark').name").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("mymark".into()));
}

#[test]
fn performance_measure_duration() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.mark('s'); performance.mark('e', {startTime: performance.now()+10}); var m = performance.measure('d','s','e'); m.duration >= 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_get_entries_by_name() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.mark('x'); performance.mark('x'); performance.getEntriesByName('x','mark').length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn performance_clear_marks() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("performance.mark('a'); performance.clearMarks(); performance.getEntriesByType('mark').length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn performance_observer_constructor_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof PerformanceObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.PerformanceObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_receives_mark_entry() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                var got = [];\
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
                po.observe({entryTypes:['mark']});\
                performance.mark('obs_test');\
                _lumen_tick_timers();\
                got.length === 1 && got[0].name === 'obs_test'\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_disconnect_stops_delivery() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                var count = 0;\
                var po = new PerformanceObserver(function() { count++; });\
                po.observe({entryTypes:['mark']});\
                performance.mark('before');\
                _lumen_tick_timers();\
                po.disconnect();\
                performance.mark('after');\
                _lumen_tick_timers();\
                count === 1\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_paint_entry_via_lumen_deliver() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                var got = [];\
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
                po.observe({entryTypes:['paint']});\
                _lumen_deliver_paint_entry('first-paint', 42.0);\
                _lumen_tick_timers();\
                got.length === 1 && got[0].name === 'first-paint' && got[0].startTime === 42.0\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-645: every paint-timing WPT opens with
// `assert_implements(window.PerformancePaintTiming)`; the interface object must
// exist, refuse `new`, and be the prototype of the entries the shell delivers.
#[test]
fn performance_paint_timing_interface_backs_paint_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                var threw = false;\
                try { new PerformancePaintTiming(); } catch (e) { threw = e instanceof TypeError; }\
                _lumen_deliver_paint_entry('first-contentful-paint', 7.5);\
                var e = performance.getEntriesByType('paint')[0];\
                var j = e.toJSON();\
                typeof window.PerformancePaintTiming === 'function' && threw\
                    && e instanceof PerformancePaintTiming\
                    && j.name === 'first-contentful-paint' && j.entryType === 'paint'\
                    && j.startTime === 7.5 && j.duration === 0\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-1189: `PerformanceEntry` is the shared base of every entry interface; the
// accessors are setter-less, so entries must still carry their own fields.
#[test]
fn performance_entry_is_shared_base_of_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                _lumen_deliver_paint_entry('first-paint', 10.0);
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 5);
                performance.mark('m'); performance.measure('x', 'm');
                var all = performance.getEntries();
                var d = Object.getOwnPropertyDescriptor(PerformanceEntry.prototype, 'name');
                var threw = false;
                try { new PerformanceEntry(); } catch (e) { threw = e instanceof TypeError; }
                var protoThrows = false;
                try { PerformanceEntry.prototype.name; } catch (e) { protoThrows = e instanceof TypeError; }
                threw && protoThrows && !!d.get && d.set === undefined
                    && all.length >= 4
                    && all.every(function(e) { return e instanceof PerformanceEntry && e.name !== '' && e.entryType !== ''; })
                    && performance.getEntriesByType('paint')[0].startTime === 10.0
                    && performance.getEntriesByType('measure')[0].name === 'x'
                    && performance.getEntriesByType('mark')[0].id === 0
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-678: `assert_implements(window.LargestContentfulPaint)` opens every LCP
// and soft-navigation WPT; the interface object must exist, refuse `new`, back
// the entries the shell delivers and serialise the IDL attributes.
#[test]
fn largest_contentful_paint_interface_backs_lcp_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    // NodeId 6 = <div id="main"> in make_doc().
    let r = rt.eval("\
                var threw = false;\
                try { new LargestContentfulPaint(); } catch (e) { threw = e instanceof TypeError; }\
                _lumen_deliver_lcp_entry(6, 1024, 200.5, 210.5);\
                var e = performance.getEntriesByType('largest-contentful-paint')[0];\
                var j = e.toJSON();\
                typeof window.LargestContentfulPaint === 'function' && threw\
                    && e instanceof LargestContentfulPaint\
                    && e.renderTime === 210.5 && e.loadTime === 200.5\
                    && j.entryType === 'largest-contentful-paint' && j.size === 1024\
                    && j.renderTime === 210.5 && j.element === e.element && !('toJSON' in j)\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-678: the soft-navigation hook writes into the page shim's performance
// timeline — it used to push into `performance._perf_entries`, which does not
// exist, so `getEntriesByType('soft-navigation')` stayed empty even when called.
#[test]
fn soft_nav_hook_feeds_the_performance_timeline() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                _lumen_deliver_soft_nav('https://example.test/about', 12.5, 0);\
                var list = performance.getEntriesByType('soft-navigation');\
                list.length === 1 && list[0] instanceof PerformanceSoftNavigationEntry\
                    && list[0].name === 'https://example.test/about' && list[0].startTime === 12.5\
                    && performance.getEntries().indexOf(list[0]) !== -1\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-673: resource/navigation entries are instances of
// PerformanceResourceTiming / PerformanceNavigationTiming (the latter
// inheriting from the former); neither interface is constructible, and the
// `[Default] toJSON` serialises the entry's fields without carrying itself.
#[test]
fn performance_resource_and_navigation_timing_interfaces_back_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var threw = 0;
                try { new PerformanceResourceTiming(); } catch (e) { if (e instanceof TypeError) threw++; }
                try { new PerformanceNavigationTiming(); } catch (e) { if (e instanceof TypeError) threw++; }
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 5);
                _lumen_deliver_perf_entry('navigation', 'https://example.com/', 0.0, 300.0,
                    '{"type":"navigate","domComplete":250}');
                var r = performance.getEntriesByType('resource')[0];
                var n = performance.getEntriesByType('navigation')[0];
                var rj = r.toJSON(), nj = JSON.parse(JSON.stringify(n));
                typeof window.PerformanceResourceTiming === 'function'
                    && typeof window.PerformanceNavigationTiming === 'function' && threw === 2
                    && r instanceof PerformanceResourceTiming && !(r instanceof PerformanceNavigationTiming)
                    && n instanceof PerformanceNavigationTiming && n instanceof PerformanceResourceTiming
                    && Object.getPrototypeOf(PerformanceNavigationTiming.prototype) === PerformanceResourceTiming.prototype
                    && rj.name === 'https://example.com/a.js' && rj.initiatorType === 'script'
                    && rj.responseEnd === 15 && !('toJSON' in rj)
                    && nj.type === 'navigate' && nj.domComplete === 250 && nj.duration === 300
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-687: mark/measure entries are PerformanceMark / PerformanceMeasure
// instances whose class string names the interface (the registry WPT's
// `[object PerformanceMark]` check). PerformanceMark is constructible per User
// Timing L3 §4.2 (validating startTime and PerformanceTiming names, cloning
// `detail`); PerformanceMeasure is not.
#[test]
fn performance_mark_and_measure_interfaces_back_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var d = { a: 1 };
                var m = performance.mark('m1', { startTime: 5, detail: d });
                d.a = 2;
                var s = performance.measure('s1', 'm1');
                var built = new PerformanceMark('free', { startTime: 7 });
                var threw = 0;
                try { new PerformanceMeasure(); } catch (e) { if (e instanceof TypeError) threw++; }
                try { PerformanceMark('x'); } catch (e) { if (e instanceof TypeError) threw++; }
                try { new PerformanceMark('x', { startTime: -1 }); } catch (e) { if (e instanceof TypeError) threw++; }
                try { performance.mark('navigationStart'); } catch (e) { if (e.name === 'SyntaxError') threw++; }
                try { performance.mark('x', 123); } catch (e) { if (e instanceof TypeError) threw++; }
                try { new PerformanceMark('x', { startTime: NaN }); } catch (e) { if (e instanceof TypeError) threw++; }
                typeof window.PerformanceMark === 'function' && typeof window.PerformanceMeasure === 'function'
                    && threw === 6
                    && m instanceof PerformanceMark && s instanceof PerformanceMeasure
                    && Object.prototype.toString.call(m) === '[object PerformanceMark]'
                    && Object.prototype.toString.call(s) === '[object PerformanceMeasure]'
                    && m.startTime === 5 && m.detail.a === 1 && m.detail !== d
                    && s.startTime === 5 && s.detail === null
                    && built.entryType === 'mark' && built.startTime === 7 && built.detail === null
                    && performance.getEntriesByName('free').length === 0
                    && !m.hasOwnProperty('detail') && PerformanceMark.length === 1
                    && performance.mark.length === 1 && performance.measure.length === 1
                    && !Object.getOwnPropertyDescriptor(window, 'PerformanceMark').enumerable
                    && Object.getOwnPropertyDescriptor(PerformanceMark.prototype, 'detail').get.name === 'get detail'
                    && (function() { try { performance.mark.call(null, 'x'); } catch (e) { return e instanceof TypeError; } return false; })()
                    && JSON.stringify(m.toJSON()) === '{"name":"m1","entryType":"mark","startTime":5,"duration":0,"detail":{"a":1}}'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-696: measure() validation (User Timing L3 §4.3) — missing marks are a
// SyntaxError, a zero PerformanceTiming attribute an InvalidAccessError, and
// conflicting PerformanceMeasureOptions members a TypeError; a number in the
// named form is a mark name, not a timestamp.
#[test]
fn performance_measure_validates_arguments() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                performance.mark('ok', { startTime: 10 });
                function name(f) { try { f(); } catch (e) { return e.name; } return 'none'; }
                var zero = ['redirectStart', 'unloadEventStart', 'domComplete'].filter(function(n) {
                    return !performance.timing[n];
                })[0];
                var okDict = performance.measure('d', { start: 'ok', duration: 5 });
                var okNav = performance.measure('n', 'navigationStart', 'ok');
                name(function() { performance.measure('a', 'Missing'); }) === 'SyntaxError'
                    && name(function() { performance.measure('a', 'ok', 'Missing'); }) === 'SyntaxError'
                    && name(function() { performance.measure('a', 51.15, 'ok'); }) === 'SyntaxError'
                    && name(function() { performance.measure('a', zero); }) === 'InvalidAccessError'
                    && name(function() { performance.measure('a', { detail: 'x' }); }) === 'TypeError'
                    && name(function() { performance.measure('a', { start: 1, duration: 2, end: 3 }); }) === 'TypeError'
                    && name(function() { performance.measure('a', { start: 1 }, 'ok'); }) === 'TypeError'
                    && name(function() { performance.measure('a', { start: -1 }); }) === 'TypeError'
                    && okDict.startTime === 10 && okDict.duration === 5
                    && okNav.startTime === 0 && okNav.duration === 10
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-687, same WPT: every entry type the registry lists must stringify as its
// interface — the navigation entry by its own tag, not the inherited resource one.
#[test]
fn performance_entry_interfaces_have_class_strings() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                _lumen_deliver_paint_entry('first-paint', 10.0);
                _lumen_record_resource_timing('https://example.com/a.js', 'script', 10, 5);
                _lumen_deliver_perf_entry('navigation', 'https://example.com/', 0.0, 300.0, '{}');
                _lumen_deliver_longtask_entry(1, 60);
                var tag = function(type) {
                    return Object.prototype.toString.call(performance.getEntriesByType(type)[0]);
                };
                tag('paint') === '[object PerformancePaintTiming]'
                    && tag('resource') === '[object PerformanceResourceTiming]'
                    && tag('navigation') === '[object PerformanceNavigationTiming]'
                    && tag('longtask') === '[object PerformanceLongTaskTiming]'
                    && Object.prototype.toString.call(new TaskAttributionTiming())
                        === '[object TaskAttributionTiming]'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_buffered_delivers_existing() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                _lumen_deliver_paint_entry('first-paint', 10.0);\
                var got = [];\
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
                po.observe({type:'paint', buffered: true});\
                _lumen_tick_timers();\
                got.length === 1\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── PerformanceObserver single-type form (Performance Timeline L2 §6.2.2) ──

#[test]
fn performance_observer_single_type_receives_entry() {
    // observe({type: 'mark'}) — single-type form should work like entryTypes:['mark']
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });
                po.observe({type: 'mark'});
                performance.mark('single_type_test');
                _lumen_tick_timers();
                got.length === 1 && got[0].name === 'single_type_test'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_single_type_with_buffered() {
    // observe({type: 'navigation', buffered: true}) — must replay existing entries
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                _lumen_deliver_perf_entry('navigation', 'https://buf.test/', 0.0, 300.0, null);
                var got = [];
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });
                po.observe({type: 'navigation', buffered: true});
                _lumen_tick_timers();
                got.length === 1 && got[0].name === 'https://buf.test/'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_repeated_observe_accumulates_types() {
    // Multiple observe() calls accumulate subscribed types.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });
                po.observe({type: 'mark'});
                po.observe({type: 'measure'});
                performance.mark('m1');
                performance.measure('ms1', 'm1');
                _lumen_tick_timers();
                got.length === 2
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-648: observe() validation + task-queued delivery (Performance Timeline L2 §4.2/§5.3) ──

fn perf_bool(script: &str) -> bool {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn bug648_observe_without_type_or_entry_types_throws_type_error() {
    assert!(perf_bool(r#"
        var po = new PerformanceObserver(function() {});
        var a = false, b = false, c = false, d = false;
        try { po.observe({}); } catch (e) { a = e instanceof TypeError; }
        try { po.observe({entryType: ['mark']}); } catch (e) { b = e instanceof TypeError; }
        try { po.observe({entryTypes: 'mark'}); } catch (e) { c = e instanceof TypeError; }
        try { po.observe({type: 'mark', entryTypes: ['measure']}); } catch (e) { d = e instanceof TypeError; }
        a && b && c && d
    "#));
}

#[test]
fn bug648_mixing_observe_forms_throws_invalid_modification_error() {
    assert!(perf_bool(r#"
        var p1 = new PerformanceObserver(function() {});
        p1.observe({entryTypes: ['mark']});
        var a = null;
        try { p1.observe({type: 'measure'}); } catch (e) { a = e.name; }
        var p2 = new PerformanceObserver(function() {});
        p2.observe({type: 'mark'});
        var b = null;
        try { p2.observe({entryTypes: ['measure']}); } catch (e) { b = e.name; }
        // Unknown values are not errors (observe() aborts with a warning)…
        new PerformanceObserver(function() {}).observe({type: 'marks'});
        new PerformanceObserver(function() {}).observe({entryTypes: []});
        // …but the aborted call has already fixed the observer type.
        var p4 = new PerformanceObserver(function() {});
        p4.observe({type: 'marks'});
        var c = null;
        try { p4.observe({entryTypes: ['mark']}); } catch (e) { c = e.name; }
        a === 'InvalidModificationError' && b === 'InvalidModificationError'
            && c === 'InvalidModificationError'
    "#));
}

#[test]
fn bug648_callback_runs_in_a_task_not_inside_mark() {
    assert!(perf_bool(r#"
        var calls = 0;
        var po = new PerformanceObserver(function() { calls++; });
        po.observe({entryTypes: ['mark']});
        performance.mark('m1');
        performance.mark('m2');
        var sync = calls;
        _lumen_tick_timers();
        sync === 0 && calls === 1
    "#));
}

#[test]
fn bug648_disconnect_after_mark_cancels_the_delivery() {
    assert!(perf_bool(r#"
        var calls = 0;
        var po = new PerformanceObserver(function() { calls++; });
        po.observe({entryTypes: ['mark']});
        performance.mark('mark1');
        po.disconnect();
        performance.mark('mark2');
        _lumen_tick_timers();
        calls === 0
    "#));
}

// The web-vitals shape that broke cnbc/imdb: the reporting function the
// callback calls is assigned after `observe({buffered: true})` returns.
#[test]
fn bug648_buffered_observe_does_not_invoke_synchronously() {
    assert!(perf_bool(r#"
        performance.mark('early');
        var log = [];
        var report;
        new PerformanceObserver(function(list) { log.push('cb:' + list.getEntries().length); report(); })
            .observe({type: 'mark', buffered: true});
        log.push('after-observe');
        report = function() { log.push('report'); };
        _lumen_tick_timers();
        log.join(',') === 'after-observe,cb:1,report'
    "#));
}

#[test]
fn bug648_take_records_drains_the_observer_buffer() {
    assert!(perf_bool(r#"
        var calls = 0;
        var po = new PerformanceObserver(function() { calls++; });
        var r0 = po.takeRecords().length;
        po.observe({entryTypes: ['mark']});
        performance.mark('a'); performance.mark('b');
        var r1 = po.takeRecords().map(function(e) { return e.name; }).join();
        performance.mark('c');
        var r2 = po.takeRecords().length;
        var r3 = po.takeRecords().length;
        _lumen_tick_timers();
        r0 === 0 && r1 === 'a,b' && r2 === 1 && r3 === 0 && calls === 0
    "#));
}

#[test]
fn bug648_entry_types_observe_replaces_type_observe_stacks() {
    assert!(perf_bool(r#"
        var got = [];
        var po = new PerformanceObserver(function(list) {
            got = got.concat(list.getEntries().map(function(e) { return e.entryType; }));
        });
        po.observe({entryTypes: ['mark']});
        po.observe({entryTypes: ['measure']});
        performance.mark('m');
        performance.measure('x');
        _lumen_tick_timers();
        var multi = got.join();
        po.disconnect();
        got = [];
        var p2 = new PerformanceObserver(function(list) {
            got = got.concat(list.getEntries().map(function(e) { return e.entryType; }));
        });
        p2.observe({type: 'mark'});
        p2.observe({type: 'measure'});
        performance.mark('m2');
        performance.measure('x2');
        _lumen_tick_timers();
        multi === 'measure' && got.sort().join() === 'mark,measure'
    "#));
}

#[test]
fn bug648_disconnect_forgets_observed_types() {
    assert!(perf_bool(r#"
        var got = [];
        var po = new PerformanceObserver(function(list) {
            got = got.concat(list.getEntries().map(function(e) { return e.name; }));
        });
        po.observe({type: 'mark'});
        po.disconnect();
        po.observe({type: 'measure'});
        performance.mark('a');
        performance.measure('b');
        _lumen_tick_timers();
        got.join() === 'b'
    "#));
}

#[test]
fn bug648_entry_list_is_an_interface_instance_sorted_by_start_time() {
    assert!(perf_bool(r#"
        var ok = false, thisOk = false;
        var po = new PerformanceObserver(function(list, obs) {
            var names = list.getEntries().map(function(e) { return e.name; }).join();
            ok = list instanceof PerformanceObserverEntryList && obs === po
                && names === 'early,late'
                && list.getEntriesByName('late', 'mark').length === 1
                && list.getEntriesByType('measure').length === 0;
            thisOk = this === po;
        });
        po.observe({entryTypes: ['mark']});
        performance.mark('late', {startTime: 20});
        performance.mark('early', {startTime: 10});
        _lumen_tick_timers();
        var threw = false;
        try { new PerformanceObserverEntryList(); } catch (e) { threw = e instanceof TypeError; }
        ok && thisOk && threw && typeof window.PerformanceObserverEntryList === 'function'
    "#));
}

#[test]
fn bug648_entry_types_ignore_buffered_flag() {
    assert!(perf_bool(r#"
        performance.mark('past');
        var calls = 0;
        new PerformanceObserver(function() { calls++; })
            .observe({entryTypes: ['mark'], buffered: true});
        _lumen_tick_timers();
        calls === 0
    "#));
}

// `performance-timeline/idlharness.any.js`: once the entry list reaches the
// callback, idlharness checks the interface shapes themselves.
#[test]
fn bug648_interface_objects_have_the_webidl_shape() {
    assert!(perf_bool(r#"
        var g1 = Object.getOwnPropertyDescriptor(globalThis, 'PerformanceObserver');
        var g2 = Object.getOwnPropertyDescriptor(globalThis, 'PerformanceObserverEntryList');
        var p1 = Object.getOwnPropertyDescriptor(PerformanceObserver, 'prototype');
        var st = Object.getOwnPropertyDescriptor(PerformanceObserver, 'supportedEntryTypes');
        var tooFew = 0;
        try { performance.getEntriesByType(); } catch (e) { if (e instanceof TypeError) tooFew++; }
        try { performance.getEntriesByName(); } catch (e) { if (e instanceof TypeError) tooFew++; }
        var list = null;
        var po = new PerformanceObserver(function(l) { list = l; });
        po.observe({type: 'mark'});
        performance.mark('m');
        _lumen_tick_timers();
        try { list.getEntriesByType(); } catch (e) { if (e instanceof TypeError) tooFew++; }
        try { list.getEntriesByName(); } catch (e) { if (e instanceof TypeError) tooFew++; }
        !g1.enumerable && g1.configurable && g1.writable && !g2.enumerable
            && !p1.writable && !p1.enumerable && !p1.configurable
            && PerformanceObserver.length === 1 && PerformanceObserver.prototype.observe.length === 0
            && PerformanceObserver.prototype.takeRecords.name === 'takeRecords'
            && PerformanceObserverEntryList.prototype.getEntriesByName.length === 1
            && performance.getEntriesByName.length === 1 && performance.getEntries.name === 'getEntries'
            && Object.prototype.toString.call(po) === '[object PerformanceObserver]'
            && Object.prototype.toString.call(list) === '[object PerformanceObserverEntryList]'
            && st.enumerable && PerformanceObserver.supportedEntryTypes === PerformanceObserver.supportedEntryTypes
            && Object.isFrozen(PerformanceObserver.supportedEntryTypes)
            && tooFew === 4
            && (function() {
                var n = 0;
                [function() { PerformanceObserver.prototype.disconnect.call(null); },
                 function() { PerformanceObserver.prototype.takeRecords.call({}); },
                 function() { PerformanceObserverEntryList.prototype.getEntries.call(null); },
                 function() { Performance.prototype.getEntries.call(null); },
                 function() { Performance.prototype.getEntriesByType.call({}, 'mark'); }]
                    .forEach(function(f) { try { f(); } catch (e) { if (e instanceof TypeError) n++; } });
                return n === 5;
            })()
    "#));
}

#[test]
fn performance_observer_supported_entry_types() {
    // PerformanceObserver.supportedEntryTypes is an array including 'navigation'.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var types = PerformanceObserver.supportedEntryTypes;
                Array.isArray(types) && types.indexOf('navigation') !== -1 && types.indexOf('mark') !== -1
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── queueMicrotask tests ──────────────────────────────────────────────────

#[test]
fn queue_microtask_exists_as_function() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof queueMicrotask === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn queue_microtask_throws_on_non_function() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var threw = false; try { queueMicrotask(42); } catch(e) { threw = true; } threw").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn queue_microtask_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.queueMicrotask === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// S12b-2 lesson: the three tests above only pin `queueMicrotask`'s *existence*,
// because a QuickJS `eval()` returned with pending jobs unexecuted and the
// callback was unobservable without `_lumen_drain_microtasks` (a no-op on V8).
// V8 runs a microtask checkpoint after each script, so the scheduling contract
// is testable here: the callback fires after the script's synchronous tail, and
// by the time the *next* `eval` starts it has already run.
#[test]
fn queue_microtask_callback_runs_after_sync_tail() {
    let rt = v8_runtime_with_dom(make_doc());
    let during = rt
        .eval(
            "var log = [];\
                     queueMicrotask(function() { log.push('micro'); });\
                     log.push('sync');\
                     log.join(',')",
        )
        .unwrap();
    assert_eq!(
        during,
        lumen_core::JsValue::String("sync".to_string()),
        "microtask must not run inline at the queueMicrotask() call site"
    );
    let after = rt.eval("log.join(',')").unwrap();
    assert_eq!(
        after,
        lumen_core::JsValue::String("sync,micro".to_string()),
        "microtask must have run by the end of the script that queued it"
    );
}

// BUG-702: a page may replace the global `Promise` with its own implementation
// — core-js does exactly that whenever its feature detection rejects the native
// one — and such a polyfill schedules its own reaction jobs through the host
// `queueMicrotask`. When `queueMicrotask` re-read `Promise` from the global it
// called straight back into the polyfill, which notified again: unbounded
// recursion that spun the engine at 100% CPU forever on `tbank.ru/auth/login/`.
// The pristine resolve/then pair is captured at shim-install time instead.
#[test]
fn queue_microtask_ignores_page_replaced_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let reentered = rt
        .eval(
            "var log = [];\
                     var reentered = false;\
                     var fake = function() { throw new Error('page Promise ctor used'); };\
                     fake.resolve = function() { reentered = true; return { then: function(f) { f(); } }; };\
                     globalThis.Promise = fake;\
                     queueMicrotask(function() { log.push('micro'); });\
                     log.push('sync');\
                     reentered",
        )
        .unwrap();
    // The sabotage must actually be visible in the scope the shim resolves
    // `Promise` from — otherwise this test would pass for the wrong reason.
    let visible = rt.eval("Promise === fake").unwrap();
    assert_eq!(
        visible,
        lumen_core::JsValue::Bool(true),
        "test setup broken: the replaced Promise is not visible in global scope"
    );
    assert_eq!(
        reentered,
        lumen_core::JsValue::Bool(false),
        "queueMicrotask must not route through the page's replaced Promise"
    );
    let after = rt.eval("log.join(',')").unwrap();
    assert_eq!(
        after,
        lumen_core::JsValue::String("sync,micro".to_string()),
        "the microtask must still run, on the pristine Promise captured at install"
    );
}

// ── requestAnimationFrame / cancelAnimationFrame ──────────────────────────

#[test]
fn raf_returns_numeric_id() {
    let rt = v8_runtime_with_dom(make_doc());
    let id = rt.eval("requestAnimationFrame(function(){})").unwrap();
    assert!(matches!(id, lumen_core::JsValue::Number(n) if n >= 1.0));
}

#[test]
fn raf_ids_are_sequential() {
    let rt = v8_runtime_with_dom(make_doc());
    let id1 = rt.eval("requestAnimationFrame(function(){})").unwrap();
    let id2 = rt.eval("requestAnimationFrame(function(){})").unwrap();
    if let (lumen_core::JsValue::Number(n1), lumen_core::JsValue::Number(n2)) = (id1, id2) {
        assert!(n2 > n1);
    } else {
        panic!("expected numeric IDs");
    }
}

#[test]
fn raf_non_function_returns_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let id = rt.eval("requestAnimationFrame(42)").unwrap();
    assert_eq!(id, lumen_core::JsValue::Number(0.0));
}

#[test]
fn raf_marks_raf_pending() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(!rt.take_raf_pending(), "clean at start");
    rt.eval("requestAnimationFrame(function(){})").unwrap();
    assert!(rt.take_raf_pending(), "set after rAF call");
    assert!(!rt.take_raf_pending(), "cleared after take");
}

#[test]
fn raf_pending_flag_clone_observes_and_clears() {
    // ADR-016 M2.3: the UI thread reads a cloned `Arc<AtomicBool>` of the
    // rAF-pending flag lock-free (no engine-thread round-trip). The clone
    // must reflect both the mark (requestAnimationFrame) and the clear
    // (take_raf_pending) since it aliases the same atomic.
    use std::sync::atomic::Ordering;
    let rt = v8_runtime_with_dom(make_doc());
    let flag = rt.raf_pending_flag();
    assert!(!flag.load(Ordering::Relaxed), "clean at start");
    rt.eval("requestAnimationFrame(function(){})").unwrap();
    assert!(flag.load(Ordering::Relaxed), "clone observes the mark");
    assert!(rt.take_raf_pending());
    assert!(!flag.load(Ordering::Relaxed), "clone observes the clear");
}

#[test]
fn dom_dirty_flag_clone_observes_and_clears() {
    // ADR-016 M2.3: companion lock-free clone of the DOM-dirty flag, used to
    // trigger an async relayout after an off-thread rAF turn mutated the DOM.
    use std::sync::atomic::Ordering;
    let rt = v8_runtime_with_dom(make_doc());
    let flag = rt.dom_dirty_flag();
    let _ = rt.take_dom_dirty(); // clear any load-time dirtiness
    assert!(!flag.load(Ordering::Relaxed), "clean after initial take");
    rt.eval("document.body.setAttribute('data-x', '1')").unwrap();
    assert!(flag.load(Ordering::Relaxed), "clone observes the DOM mutation");
    assert!(rt.take_dom_dirty());
    assert!(!flag.load(Ordering::Relaxed), "clone observes the clear");
}

#[test]
fn raf_run_calls_callback_with_timestamp() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_ts = -1; requestAnimationFrame(function(t){ _raf_ts = t; })").unwrap();
    rt.eval("_lumen_run_raf_callbacks(16.7)").unwrap();
    let ts = rt.eval("_raf_ts").unwrap();
    assert_eq!(ts, lumen_core::JsValue::Number(16.7));
}

#[test]
fn raf_run_snapshot_pattern() {
    // Callbacks registered during a frame run go into the NEXT frame.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_count = 0;").unwrap();
    rt.eval("requestAnimationFrame(function() { _raf_count++; requestAnimationFrame(function(){ _raf_count++; }); })").unwrap();
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    let count1 = rt.eval("_raf_count").unwrap();
    assert_eq!(count1, lumen_core::JsValue::Number(1.0), "only outer cb in frame 1");
    rt.eval("_lumen_run_raf_callbacks(16)").unwrap();
    let count2 = rt.eval("_raf_count").unwrap();
    assert_eq!(count2, lumen_core::JsValue::Number(2.0), "inner cb in frame 2");
}

#[test]
fn raf_recursive_marks_pending() {
    let rt = v8_runtime_with_dom(make_doc());
    // Callback registers another rAF → raf_pending must be set after run.
    rt.eval("requestAnimationFrame(function() { requestAnimationFrame(function(){}); })").unwrap();
    let _ = rt.take_raf_pending(); // clear initial flag
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    assert!(rt.take_raf_pending(), "inner rAF sets pending for next frame");
}

#[test]
fn cancel_raf_prevents_callback() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_ran = false;").unwrap();
    rt.eval("var id = requestAnimationFrame(function(){ _raf_ran = true; });").unwrap();
    rt.eval("cancelAnimationFrame(id)").unwrap();
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    let ran = rt.eval("_raf_ran").unwrap();
    assert_eq!(ran, lumen_core::JsValue::Bool(false));
}

#[test]
fn cancel_raf_unknown_id_is_noop() {
    let rt = v8_runtime_with_dom(make_doc());
    // Should not throw or panic.
    rt.eval("cancelAnimationFrame(9999)").unwrap();
}

#[test]
fn raf_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.requestAnimationFrame === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn cancel_raf_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.cancelAnimationFrame === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── EE-5: rAF vsync batch / DOMHighResTimeStamp tests ────────────────────

#[test]
fn raf_coalesce_multiple_registrations_fire_in_one_batch() {
    // EE-5: multiple requestAnimationFrame() calls in the same frame
    // are all executed in a single _lumen_run_raf_callbacks() invocation.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_log = []; \
                     requestAnimationFrame(function(){ _raf_log.push(1); }); \
                     requestAnimationFrame(function(){ _raf_log.push(2); }); \
                     requestAnimationFrame(function(){ _raf_log.push(3); });").unwrap();
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    let len = rt.eval("_raf_log.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(3.0), "all 3 callbacks fired in one batch");
    let order = rt.eval("_raf_log[0] === 1 && _raf_log[1] === 2 && _raf_log[2] === 3").unwrap();
    assert_eq!(order, lumen_core::JsValue::Bool(true), "callbacks fire in registration order");
}

#[test]
fn raf_batch_uniform_timestamp() {
    // EE-5: all callbacks in a batch receive the identical DOMHighResTimeStamp.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_ts1 = null; var _raf_ts2 = null; \
                     requestAnimationFrame(function(t){ _raf_ts1 = t; }); \
                     requestAnimationFrame(function(t){ _raf_ts2 = t; });").unwrap();
    rt.eval("_lumen_run_raf_callbacks(42.5)").unwrap();
    let eq = rt.eval("_raf_ts1 === _raf_ts2").unwrap();
    assert_eq!(eq, lumen_core::JsValue::Bool(true), "both callbacks get same timestamp");
    let val = rt.eval("_raf_ts1").unwrap();
    assert_eq!(val, lumen_core::JsValue::Number(42.5));
}

#[test]
fn raf_deterministic_zero_timestamp() {
    // EE-5: deterministic mode (timestamp_ms === 0) delivers 0 to all callbacks.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_det_ts = -99; requestAnimationFrame(function(t){ _raf_det_ts = t; })").unwrap();
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    let ts = rt.eval("_raf_det_ts").unwrap();
    assert_eq!(ts, lumen_core::JsValue::Number(0.0), "deterministic mode passes 0 to callback");
}

#[test]
fn raf_live_clock_timestamp_non_negative() {
    // EE-5: when timestamp_ms < 0, JS uses performance.now() — must be >= 0.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_live_ts = null; requestAnimationFrame(function(t){ _raf_live_ts = t; })").unwrap();
    rt.eval("_lumen_run_raf_callbacks(-1)").unwrap();
    let ts = rt.eval("typeof _raf_live_ts === 'number' && _raf_live_ts >= 0").unwrap();
    assert_eq!(ts, lumen_core::JsValue::Bool(true), "live clock timestamp is non-negative DOMHighResTimeStamp");
}

#[test]
fn raf_exception_in_one_callback_does_not_stop_batch() {
    // EE-5: if one callback throws, subsequent callbacks still run (try/catch).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _raf_after_throw = false; \
                     requestAnimationFrame(function(){ throw new Error('boom'); }); \
                     requestAnimationFrame(function(){ _raf_after_throw = true; });").unwrap();
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    let ran = rt.eval("_raf_after_throw").unwrap();
    assert_eq!(ran, lumen_core::JsValue::Bool(true), "second callback ran despite first throwing");
}
