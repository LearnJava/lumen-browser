//! Тесты `v8_perf_observers`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`]: same fixture document, same
/// `install_dom` argument list, same `_LUMEN_EXTENSION_ACTIVE` pre-eval.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
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
/// off the instance and onto the prototype buys.
#[test]
fn performance_to_json_carries_attributes_only() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("Object.keys(performance.toJSON()).join(',')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("timeOrigin".to_string()));
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
                po.disconnect();\
                performance.mark('after');\
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
                got.length === 1 && got[0].name === 'first-paint' && got[0].startTime === 42.0\
            ").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn performance_observer_buffered_delivers_existing() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("\
                _lumen_deliver_paint_entry('first-paint', 10.0);\
                var got = [];\
                var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
                po.observe({entryTypes:['paint'], buffered: true});\
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
                got.length === 2
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
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

// ── MutationObserver tests ────────────────────────────────────────────────

#[test]
fn mutation_observer_exists_as_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof MutationObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn mutation_observer_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.MutationObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn mutation_observer_fires_on_attribute_change() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_fired = false;
                var _mo_rec = null;
                var obs = new MutationObserver(function(records) {
                    _mo_fired = true;
                    _mo_rec = records[0];
                });
                var el = document.getElementById('main');
                obs.observe(el, { attributes: true });
                el.setAttribute('data-x', '42');
            "#).unwrap();
    // Flush synchronously; queueMicrotask delivery drains on next eval but
    // using the flush function is more explicit and reliable in tests.
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    let fired = rt.eval("_mo_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
    let attr = rt.eval("_mo_rec && _mo_rec.type").unwrap();
    assert_eq!(attr, lumen_core::JsValue::String("attributes".into()));
}

#[test]
fn mutation_record_is_interface_global() {
    // BUG-317: MutationRecord resolves as a global interface (bare identifier
    // and window property) and is not constructible (DOM §4.3.3).
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval("typeof MutationRecord === 'function'").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    assert_eq!(
        rt.eval("typeof window.MutationRecord === 'function'").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    assert_eq!(
        rt.eval("try { new MutationRecord(); false } catch (e) { e instanceof TypeError }")
            .unwrap(),
        lumen_core::JsValue::Bool(true)
    );
}

#[test]
fn mutation_observer_records_are_mutation_record_instances() {
    // BUG-317: records delivered to the callback are `instanceof MutationRecord`
    // (WPT dom/nodes/MutationObserver-callback-arguments.html).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_is_rec = false;
                var obsR = new MutationObserver(function(records) {
                    _mo_is_rec = records[0] instanceof MutationRecord;
                });
                var el = document.getElementById('main');
                obsR.observe(el, { attributes: true });
                el.setAttribute('data-y', '7');
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    assert_eq!(
        rt.eval("_mo_is_rec").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
}

#[test]
fn attribute_ns_methods_are_name_based() {
    // BUG-309: the namespaced attribute accessors mirror the name-only model —
    // setAttributeNS stores under the qualified name, so hasAttribute finds it
    // irrespective of namespace (WPT dom/nodes/Element-hasAttribute.html §1).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _el = document.createElement('p'); _el.setAttributeNS('foo', 'x', 'first');")
        .unwrap();
    assert_eq!(
        rt.eval("_el.hasAttribute('x')").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    assert_eq!(
        rt.eval("_el.getAttributeNS('foo', 'x')").unwrap(),
        lumen_core::JsValue::String("first".into())
    );
    assert_eq!(
        rt.eval("_el.hasAttributeNS('foo', 'x')").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    rt.eval("_el.removeAttributeNS('foo', 'x')").unwrap();
    assert_eq!(
        rt.eval("_el.hasAttribute('x')").unwrap(),
        lumen_core::JsValue::Bool(false)
    );
    assert_eq!(
        rt.eval("_el.getAttributeNS('foo', 'x')").unwrap(),
        lumen_core::JsValue::Null
    );
}

#[test]
fn has_attributes_reflects_attribute_presence() {
    // BUG-312: Element.hasAttributes() (DOM §4.9.2) — false with no attributes,
    // true once any attribute is present (WPT dom/nodes/Element-hasAttributes.html).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _el = document.createElement('p');").unwrap();
    assert_eq!(
        rt.eval("_el.hasAttributes()").unwrap(),
        lumen_core::JsValue::Bool(false)
    );
    rt.eval("_el.setAttribute('id', 'x');").unwrap();
    assert_eq!(
        rt.eval("_el.hasAttributes()").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    rt.eval("_el.removeAttribute('id');").unwrap();
    assert_eq!(
        rt.eval("_el.hasAttributes()").unwrap(),
        lumen_core::JsValue::Bool(false)
    );
}

#[test]
fn mutation_observer_fires_on_child_list_change() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_cl_fired = false;
                var obs2 = new MutationObserver(function(records) {
                    _mo_cl_fired = records.some(function(r){ return r.type === 'childList'; });
                });
                var body = document.body;
                obs2.observe(body, { childList: true });
                var d = document.createElement('div');
                body.appendChild(d);
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    let fired = rt.eval("_mo_cl_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
}

#[test]
fn mutation_observer_disconnect_stops_delivery() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_cnt = 0;
                var obs3 = new MutationObserver(function() { _mo_cnt++; });
                var el3 = document.getElementById('main');
                obs3.observe(el3, { attributes: true });
                obs3.disconnect();
                el3.setAttribute('data-y', '1');
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    let cnt = rt.eval("_mo_cnt").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
}

#[test]
fn mutation_observer_take_records_clears_queue() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var obs4 = new MutationObserver(function() {});
                var el4 = document.getElementById('main');
                obs4.observe(el4, { attributes: true });
                el4.setAttribute('data-z', '1');
                var recs = obs4.takeRecords();
            "#).unwrap();
    let len = rt.eval("recs.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(1.0));
    // Internal queue must be cleared
    let inner_len = rt.eval("obs4.takeRecords().length").unwrap();
    assert_eq!(inner_len, lumen_core::JsValue::Number(0.0));
}

#[test]
fn mutation_observer_take_records_full_sequence() {
    // BUG-318: mirrors WPT dom/nodes/MutationObserver-takeRecords.html — the
    // full record shape must match. In particular: element.textContent yields a
    // childList record (not characterData), a live text node's `.data` write
    // yields a characterData record with the correct target/oldValue, and
    // addedNodes carries the actual (interned) node wrapper.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var p = document.createElement('p');
                p.setAttribute('id', 'n00');
                document.body.appendChild(p);
                var obs = new MutationObserver(function(){});
                obs.observe(p, {subtree:true, childList:true, attributes:true,
                                characterData:true, attributeOldValue:true,
                                characterDataOldValue:true});
                p.id = "foo";
                p.id = "bar";
                p.className = "bar";
                p.textContent = "old data";
                p.firstChild.data = "new data";
                var recs = obs.takeRecords();
                globalThis._summary = [
                    recs.length,
                    recs[0].type, recs[0].attributeName, recs[0].oldValue,
                    recs[1].type, recs[1].oldValue,
                    recs[2].type, recs[2].attributeName, recs[2].oldValue,
                    recs[3].type, recs[3].addedNodes.length, (recs[3].addedNodes[0] === p.firstChild),
                    recs[4].type, recs[4].oldValue, (recs[4].target === p.firstChild),
                    obs.takeRecords().length
                ].join('|');
            "#).unwrap();
    assert_eq!(
        rt.eval("_summary").unwrap(),
        lumen_core::JsValue::String(
            "5|attributes|id|n00|attributes|foo|attributes|class||childList|1|true|characterData|old data|true|0".into()
        )
    );
}

#[test]
fn mutation_observer_reobserve_after_disconnect_delivers() {
    // BUG-318: mirrors WPT dom/nodes/MutationObserver-disconnect.html — a fresh
    // observe() after disconnect() must re-activate delivery (the observer was
    // spliced out of the active list by disconnect and only re-added by observe).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                globalThis._cnt = 0;
                globalThis._info = '';
                var el = document.getElementById('main');
                var observer = new MutationObserver(function(seq){
                    _cnt++;
                    _info = seq.length + '/' + seq[0].type + '/' + seq[0].attributeName + '/' + seq[0].oldValue;
                });
                observer.observe(el, {attributes:true});
                el.id = "foo";
                el.id = "bar";
                observer.disconnect();
                observer.observe(el, {attributes:true, attributeOldValue:true});
                el.id = "latest";
                observer.disconnect();
                observer.observe(el, {attributes:true, attributeOldValue:true});
                el.id = "n0000";
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    assert_eq!(rt.eval("_cnt").unwrap(), lumen_core::JsValue::Number(1.0));
    assert_eq!(
        rt.eval("_info").unwrap(),
        lumen_core::JsValue::String("1/attributes/id/latest".into())
    );
}

#[test]
fn mutation_observer_subtree_scoped_to_target() {
    // BUG-318: a subtree observer records mutations inside its own subtree only,
    // not everywhere in the document. The record's target is the mutated node.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var a = document.createElement('div');
                var b = document.createElement('div');
                document.body.appendChild(a);
                document.body.appendChild(b);
                var child = document.createElement('span');
                a.appendChild(child);
                var obs = new MutationObserver(function(){});
                obs.observe(a, {subtree:true, attributes:true});
                child.setAttribute('x', '1');
                b.setAttribute('y', '2');
                var recs = obs.takeRecords();
                globalThis._sub = recs.length + '|' + (recs.length === 1 && recs[0].target === child);
            "#).unwrap();
    assert_eq!(
        rt.eval("_sub").unwrap(),
        lumen_core::JsValue::String("1|true".into())
    );
}

// ── ResizeObserver tests ──────────────────────────────────────────────────

#[test]
fn resize_observer_exists_as_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof ResizeObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resize_observer_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.ResizeObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn resize_observer_fires_when_rect_changes() {
    let rt = v8_runtime_with_dom(make_doc());
    // Inject a fake bounding rect for the node
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 100.0])].into_iter().collect());
    rt.eval(r#"
                var _ro_fired = false;
                var _ro_entry = null;
                var ro = new ResizeObserver(function(entries) {
                    _ro_fired = true;
                    _ro_entry = entries[0];
                });
                var body = document.body;
                ro.observe(body);
                _lumen_deliver_resize_observers();
            "#).unwrap();
    let fired = rt.eval("_ro_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
    let w = rt.eval("_ro_entry && _ro_entry.contentRect.width").unwrap();
    assert_eq!(w, lumen_core::JsValue::Number(200.0));
}

#[test]
fn resize_observer_no_delivery_when_size_unchanged() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
    rt.eval("var _ro_cnt2 = 0; var ro2 = new ResizeObserver(function(){ _ro_cnt2++; }); ro2.observe(document.body);").unwrap();
    // First delivery
    rt.eval("_lumen_deliver_resize_observers()").unwrap();
    // Second delivery with same rect → no callback
    rt.eval("_lumen_deliver_resize_observers()").unwrap();
    let cnt = rt.eval("_ro_cnt2").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(1.0));
}

#[test]
fn resize_observer_disconnect_stops_delivery() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 300.0, 200.0])].into_iter().collect());
    rt.eval(r#"
                var _ro_cnt3 = 0;
                var ro3 = new ResizeObserver(function(){ _ro_cnt3++; });
                ro3.observe(document.body);
                ro3.disconnect();
                _lumen_deliver_resize_observers();
            "#).unwrap();
    let cnt = rt.eval("_ro_cnt3").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
}

#[test]
fn resize_observer_fires_again_on_size_change() {
    // After a size change, observer should fire a second time.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
    rt.eval(r#"
                var _ro_sz_cnt = 0;
                var ro_sz = new ResizeObserver(function() { _ro_sz_cnt++; });
                ro_sz.observe(document.body);
                _lumen_deliver_resize_observers();
            "#).unwrap();
    // Change size
    rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 80.0])].into_iter().collect());
    rt.eval("_lumen_deliver_resize_observers()").unwrap();
    let cnt = rt.eval("_ro_sz_cnt").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(2.0));
}

#[test]
fn resize_observer_border_box_size_fields() {
    // Entry must expose borderBoxSize and contentBoxSize arrays.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 150.0, 75.0])].into_iter().collect());
    rt.eval(r#"
                var _ro_bb_entry = null;
                var ro_bb = new ResizeObserver(function(entries) { _ro_bb_entry = entries[0]; });
                ro_bb.observe(document.body);
                _lumen_deliver_resize_observers();
            "#).unwrap();
    let is = rt.eval("_ro_bb_entry && _ro_bb_entry.borderBoxSize[0].inlineSize").unwrap();
    assert_eq!(is, lumen_core::JsValue::Number(150.0));
    let bs = rt.eval("_ro_bb_entry && _ro_bb_entry.contentBoxSize[0].blockSize").unwrap();
    assert_eq!(bs, lumen_core::JsValue::Number(75.0));
}

#[test]
fn resize_observer_unobserve_stops_delivery() {
    // Save element reference — document.body may create a new proxy each access.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
    rt.eval(r#"
                var _ro_un_cnt = 0;
                var _ro_un_target = document.body;
                var ro_un = new ResizeObserver(function() { _ro_un_cnt++; });
                ro_un.observe(_ro_un_target);
                ro_un.unobserve(_ro_un_target);
                _lumen_deliver_resize_observers();
            "#).unwrap();
    let cnt = rt.eval("_ro_un_cnt").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
}

/// BUG-661 §2: `observe()` on anything that is not an `Element` is a
/// `TypeError` (Resize Observer §3.1), not a silent no-op.
#[test]
fn resize_observer_observe_throws_on_non_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"(function() {
                        var ro = new ResizeObserver(function(){});
                        var thrown = [];
                        var probes = [undefined, null, {}, 'x', document, document.createTextNode('t')];
                        for (var i = 0; i < probes.length; i++) {
                            try { ro.observe(probes[i]); thrown.push('no-throw'); }
                            catch (e) { thrown.push(e instanceof TypeError ? 'TypeError' : String(e)); }
                        }
                        return thrown.join(',');
                    })()"#,
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "TypeError,TypeError,TypeError,TypeError,TypeError,TypeError".into()
        )
    );
}

/// BUG-661 §1: a newly observed target is reported on the next event-loop
/// turn even though nothing schedules a relayout — the delivery pass puts
/// itself on the timer queue instead of waiting for the shell.
#[test]
fn resize_observer_initial_delivery_without_relayout() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let (html_nid, body_nid) = {
        let doc = doc_arc.lock().unwrap();
        (
            super::find_element_by_tag(&doc, "html").unwrap().index() as u32,
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32,
        )
    };
    rt.update_layout_rects(
        [
            (html_nid, [0.0, 0.0, 1024.0, 720.0]),
            (body_nid, [0.0, 0.0, 200.0, 100.0]),
        ]
        .into_iter()
        .collect(),
    );
    rt.eval(
        r#"
                var _ro_init_w = -1;
                var _ro_init = new ResizeObserver(function(entries) { _ro_init_w = entries[0].contentRect.width; });
                _ro_init.observe(document.body);
            "#,
    )
    .unwrap();
    // No relayout, no explicit delivery call — only the event loop turns.
    assert_eq!(
        rt.eval("_ro_init_w").unwrap(),
        lumen_core::JsValue::Number(-1.0),
        "observe() must not deliver synchronously"
    );
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(rt.eval("_ro_init_w").unwrap(), lumen_core::JsValue::Number(200.0));
}

/// BUG-661 §1: the pass waits for the first layout snapshot instead of
/// reporting a bogus 0×0 entry for a document that has not been laid out.
#[test]
fn resize_observer_initial_delivery_waits_for_layout() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        r#"
                var _ro_wait_cnt = 0;
                var _ro_wait = new ResizeObserver(function() { _ro_wait_cnt++; });
                _ro_wait.observe(document.body);
            "#,
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("_ro_wait_cnt").unwrap(),
        lumen_core::JsValue::Number(0.0),
        "no layout snapshot yet → nothing to report"
    );
}

/// BUG-661 §3: `contentBoxSize` and `contentRect` are the content box —
/// border box minus border widths and padding — not a copy of the border box.
#[test]
fn resize_observer_content_box_excludes_padding_and_border() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [5.0, 7.0, 200.0, 100.0])].into_iter().collect());
    let style: std::collections::HashMap<String, String> = [
        ("border-left-width", "2px"),
        ("border-right-width", "3px"),
        ("border-top-width", "4px"),
        ("border-bottom-width", "5px"),
        ("padding-left", "10px"),
        ("padding-right", "20px"),
        ("padding-top", "6px"),
        ("padding-bottom", "8px"),
        ("font-size", "16px"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    rt.update_computed_styles([(nid, style)].into_iter().collect());
    rt.eval(
        r#"
                var _ro_cb_entry = null;
                var _ro_cb = new ResizeObserver(function(entries) { _ro_cb_entry = entries[0]; });
                _ro_cb.observe(document.body);
                _lumen_deliver_resize_observers();
            "#,
    )
    .unwrap();
    // 200 - 2 - 3 - 10 - 20 = 165; 100 - 4 - 5 - 6 - 8 = 77.
    assert_eq!(
        rt.eval("_ro_cb_entry.contentBoxSize[0].inlineSize").unwrap(),
        lumen_core::JsValue::Number(165.0)
    );
    assert_eq!(
        rt.eval("_ro_cb_entry.contentBoxSize[0].blockSize").unwrap(),
        lumen_core::JsValue::Number(77.0)
    );
    // borderBoxSize keeps the full border box.
    assert_eq!(
        rt.eval("_ro_cb_entry.borderBoxSize[0].inlineSize").unwrap(),
        lumen_core::JsValue::Number(200.0)
    );
    // contentRect's origin is the padding offset inside the border box,
    // not the element's viewport position.
    assert_eq!(
        rt.eval("_ro_cb_entry.contentRect.x").unwrap(),
        lumen_core::JsValue::Number(10.0)
    );
    assert_eq!(
        rt.eval("_ro_cb_entry.contentRect.y").unwrap(),
        lumen_core::JsValue::Number(6.0)
    );
}

/// BUG-661 §3: `box: 'border-box'` observes the border box, so padding
/// changes alone do not move the reported size.
#[test]
fn resize_observer_border_box_option() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 100.0])].into_iter().collect());
    let style: std::collections::HashMap<String, String> =
        [("padding-left", "10px"), ("padding-right", "20px")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    rt.update_computed_styles([(nid, style)].into_iter().collect());
    rt.eval(
        r#"
                var _ro_bbo = null;
                var _ro_bbo_obs = new ResizeObserver(function(entries) { _ro_bbo = entries[0]; });
                _ro_bbo_obs.observe(document.body, { box: 'border-box' });
                _lumen_deliver_resize_observers();
            "#,
    )
    .unwrap();
    assert_eq!(
        rt.eval("_ro_bbo.borderBoxSize[0].inlineSize").unwrap(),
        lumen_core::JsValue::Number(200.0)
    );
    // The entry still carries the true content box alongside it.
    assert_eq!(
        rt.eval("_ro_bbo.contentBoxSize[0].inlineSize").unwrap(),
        lumen_core::JsValue::Number(170.0)
    );
}

/// BUG-661 §4: detaching an observed element invalidates its last reported
/// size, so putting it back at the same size still notifies.
#[test]
fn resize_observer_reparent_redelivers_at_same_size() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 100.0])].into_iter().collect());
    rt.eval(
        r#"
                var _ro_rp_cnt = 0;
                var _ro_rp_target = document.body;
                var _ro_rp = new ResizeObserver(function() { _ro_rp_cnt++; });
                _ro_rp.observe(_ro_rp_target);
                _lumen_deliver_resize_observers();
            "#,
    )
    .unwrap();
    assert_eq!(rt.eval("_ro_rp_cnt").unwrap(), lumen_core::JsValue::Number(1.0));
    // Same size, no detach → no second delivery.
    rt.eval("_lumen_deliver_resize_observers()").unwrap();
    assert_eq!(rt.eval("_ro_rp_cnt").unwrap(), lumen_core::JsValue::Number(1.0));
    // remove() + appendChild() at the same size → one more delivery.
    rt.eval(
        r#"
                var _ro_rp_parent = _ro_rp_target.parentNode;
                _ro_rp_parent.removeChild(_ro_rp_target);
                _ro_rp_parent.appendChild(_ro_rp_target);
                _lumen_deliver_resize_observers();
            "#,
    )
    .unwrap();
    assert_eq!(rt.eval("_ro_rp_cnt").unwrap(), lumen_core::JsValue::Number(2.0));
}

// ── IntersectionObserver tests ────────────────────────────────────────────

#[test]
fn intersection_observer_exists_as_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof IntersectionObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn intersection_observer_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.IntersectionObserver === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-807: `observe()` queues its own initial notification, so a target
/// on a page that never relayouts again is still reported — the callback
/// used to arrive only as a side effect of an unrelated mutation.
#[test]
fn intersection_observer_initial_delivery_without_relayout() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let (html_nid, body_nid) = {
        let doc = doc_arc.lock().unwrap();
        (
            super::find_element_by_tag(&doc, "html").unwrap().index() as u32,
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32,
        )
    };
    rt.update_layout_rects(
        [
            (html_nid, [0.0, 0.0, 1024.0, 720.0]),
            (body_nid, [0.0, 0.0, 100.0, 50.0]),
        ]
        .into_iter()
        .collect(),
    );
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(
        r#"
                var _io_init_ratio = -1;
                var _io_init = new IntersectionObserver(function(entries) {
                    _io_init_ratio = entries[0].intersectionRatio;
                });
                _io_init.observe(document.body);
            "#,
    )
    .unwrap();
    // No relayout, no explicit delivery call — only the event loop turns.
    assert_eq!(
        rt.eval("_io_init_ratio").unwrap(),
        lumen_core::JsValue::Number(-1.0),
        "observe() must not deliver synchronously"
    );
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("_io_init_ratio").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
}

/// BUG-807: the pass waits for the first layout snapshot rather than
/// reporting every target of a not-yet-laid-out document as invisible.
#[test]
fn intersection_observer_initial_delivery_waits_for_layout() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(
        r#"
                var _io_wait_cnt = 0;
                var _io_wait = new IntersectionObserver(function() { _io_wait_cnt++; });
                _io_wait.observe(document.body);
            "#,
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("_io_wait_cnt").unwrap(),
        lumen_core::JsValue::Number(0.0),
        "no layout snapshot yet → nothing to report"
    );
}

/// BUG-807: a target with no box owes an initial notification all the
/// same (§3.2.1 reports it as an empty box, not as no observation).
#[test]
fn intersection_observer_initial_delivery_for_boxless_target() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let html_nid = {
        let doc = doc_arc.lock().unwrap();
        super::find_element_by_tag(&doc, "html").unwrap().index() as u32
    };
    // Document laid out, but the observed target itself has no box.
    rt.update_layout_rects(
        [(html_nid, [0.0, 0.0, 1024.0, 720.0])].into_iter().collect(),
    );
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(
        r#"
                var _io_box_cnt = 0;
                var _io_box_entry = null;
                var _io_box = new IntersectionObserver(function(entries) {
                    _io_box_cnt++;
                    _io_box_entry = entries[0];
                });
                _io_box.observe(document.body);
                _lumen_tick_timers();
            "#,
    )
    .unwrap();
    assert_eq!(
        rt.eval("_io_box_cnt").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
    assert_eq!(
        rt.eval("_io_box_entry.isIntersecting").unwrap(),
        lumen_core::JsValue::Bool(false)
    );
    // The notification is owed once, not on every pass.
    rt.eval("_lumen_deliver_intersection_observers()").unwrap();
    assert_eq!(
        rt.eval("_io_box_cnt").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
}

#[test]
fn intersection_observer_fires_on_first_observe_visible() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _io_fired = false;
                var _io_entry = null;
                var io = new IntersectionObserver(function(entries) {
                    _io_fired = true;
                    _io_entry = entries[0];
                });
                io.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let fired = rt.eval("_io_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
    let ratio = rt.eval("_io_entry && _io_entry.intersectionRatio > 0").unwrap();
    assert_eq!(ratio, lumen_core::JsValue::Bool(true));
    let intersecting = rt.eval("_io_entry.isIntersecting").unwrap();
    assert_eq!(intersecting, lumen_core::JsValue::Bool(true));
}

#[test]
fn intersection_observer_not_intersecting_when_outside_viewport() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Element is below viewport
    rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 50.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _io2_entry = null;
                var io2 = new IntersectionObserver(function(entries) { _io2_entry = entries[0]; });
                io2.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let intersecting = rt.eval("_io2_entry && _io2_entry.isIntersecting").unwrap();
    assert_eq!(intersecting, lumen_core::JsValue::Bool(false));
}

#[test]
fn intersection_observer_threshold_fires_only_on_crossing() {
    // element partially in viewport (ratio≈0.7), then fully out — only 2 callbacks:
    // initial delivery + crossing back out below threshold 0.5.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Partially visible: y=650, h=100, viewport h=720 → ratio=70/100=0.7
    rt.update_layout_rects([(nid, [0.0, 650.0, 100.0, 100.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _thr_cnt = 0;
                var io_thr = new IntersectionObserver(function(entries) {
                    _thr_cnt++;
                }, { threshold: 0.5 });
                io_thr.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    // Second delivery same rect — no crossing → no fire
    rt.eval("_lumen_deliver_intersection_observers()").unwrap();
    let cnt1 = rt.eval("_thr_cnt").unwrap();
    assert_eq!(cnt1, lumen_core::JsValue::Number(1.0));
    // Move fully out of viewport — ratio=0 crosses 0.5 → fires again
    rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_deliver_intersection_observers()").unwrap();
    let cnt2 = rt.eval("_thr_cnt").unwrap();
    assert_eq!(cnt2, lumen_core::JsValue::Number(2.0));
}

#[test]
fn intersection_observer_rootmargin_expands_viewport() {
    // Element just below viewport; positive rootMargin makes it visible.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Element top at y=730 (10px below 720px viewport)
    rt.update_layout_rects([(nid, [0.0, 730.0, 100.0, 50.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _rm_entry = null;
                var io_rm = new IntersectionObserver(function(entries) {
                    _rm_entry = entries[0];
                }, { rootMargin: '0px 0px 50px 0px' });
                io_rm.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let intersecting = rt.eval("_rm_entry && _rm_entry.isIntersecting").unwrap();
    assert_eq!(intersecting, lumen_core::JsValue::Bool(true));
}

#[test]
fn intersection_observer_rootmargin_contracts_viewport() {
    // Element near bottom; negative rootMargin pushes root boundary up, element leaves root.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Element at y=700, h=50 → nominally intersects 720px viewport by 20px
    rt.update_layout_rects([(nid, [0.0, 700.0, 100.0, 50.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _rm2_entry = null;
                var io_rm2 = new IntersectionObserver(function(entries) {
                    _rm2_entry = entries[0];
                }, { rootMargin: '0px 0px -50px 0px' });
                io_rm2.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    // rootBottom = 720-50 = 670; element top=700 > 670 → no intersection
    let intersecting = rt.eval("_rm2_entry && _rm2_entry.isIntersecting").unwrap();
    assert_eq!(intersecting, lumen_core::JsValue::Bool(false));
}

#[test]
fn intersection_observer_unobserve_stops_delivery() {
    // document.body may return a new proxy object each call, so save the reference
    // and use the same object for both observe() and unobserve().
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _un_cnt = 0;
                var _un_target = document.body;
                var io_un = new IntersectionObserver(function() { _un_cnt++; });
                io_un.observe(_un_target);
                io_un.unobserve(_un_target);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let cnt = rt.eval("_un_cnt").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
}

#[test]
fn intersection_observer_two_observers_fire_independently() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 100.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _cnt_a = 0, _cnt_b = 0;
                var io_a = new IntersectionObserver(function() { _cnt_a++; });
                var io_b = new IntersectionObserver(function() { _cnt_b++; });
                io_a.observe(document.body);
                io_b.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let a = rt.eval("_cnt_a").unwrap();
    let b = rt.eval("_cnt_b").unwrap();
    assert_eq!(a, lumen_core::JsValue::Number(1.0));
    assert_eq!(b, lumen_core::JsValue::Number(1.0));
}

#[test]
fn intersection_observer_intersection_rect_height() {
    // intersectionRect.height must equal the visible slice of the element.
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    // Element at y=680, h=100; viewport h=720 → 40px visible
    rt.update_layout_rects([(nid, [0.0, 680.0, 100.0, 100.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
                var _ir_entry = null;
                var io_ir = new IntersectionObserver(function(entries) { _ir_entry = entries[0]; });
                io_ir.observe(document.body);
                _lumen_deliver_intersection_observers();
            "#).unwrap();
    let ih = rt.eval("_ir_entry && _ir_entry.intersectionRect.height").unwrap();
    assert_eq!(ih, lumen_core::JsValue::Number(40.0));
    let ratio_ok = rt.eval("_ir_entry && Math.abs(_ir_entry.intersectionRatio - 0.4) < 0.01").unwrap();
    assert_eq!(ratio_ok, lumen_core::JsValue::Bool(true));
}
