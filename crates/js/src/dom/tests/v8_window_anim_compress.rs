//! Тесты `v8_window_anim_compress`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

// V8 twin of the (removed) QuickJS `runtime_with_dom` helper.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// V8 twin of the (removed) QuickJS `runtime_deterministic` helper.
fn v8_runtime_deterministic(doc: Arc<Mutex<Document>>, url: &str) -> V8JsRuntime {
    v8_runtime_deterministic_cfg(doc, url, None, false)
}

// DEVX-16: like `v8_runtime_deterministic`, but exposes the `--rng-seed`
// override / `--monotonic-clock` knobs instead of hardcoding them off.
fn v8_runtime_deterministic_cfg(
    doc: Arc<Mutex<Document>>,
    url: &str,
    rng_seed: Option<u64>,
    monotonic_clock: bool,
) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.set_deterministic_mode(true, rng_seed, monotonic_clock);
    rt.install_dom(doc, url, None, None, None, None, None, None, None, None, false).unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

// ── document.caretPositionFromPoint tests ──────────────────────────────────

#[test]
fn caret_position_from_point_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.caretPositionFromPoint === 'function'"));
}

#[test]
fn caret_position_from_point_returns_object() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   var cp = document.caretPositionFromPoint(10, 20); \
                   return cp !== null && cp.offsetNode !== undefined && typeof cp.offset === 'number'; \
                 })()"));
}

#[test]
fn caret_position_from_point_has_get_client_rects() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   var cp = document.caretPositionFromPoint(0, 0); \
                   return cp !== null && typeof cp.getClientRects === 'function'; \
                 })()"));
}


// ── _lumen_gc_collect tests ────────────────────────────────────────────────

#[test]
fn gc_collect_removes_listener_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    // Register two listeners on nid=42 and one on nid=99.
    rt.eval("_lumen_add_listener(42,'click',function(){}); \
                     _lumen_add_listener(42,'mouseover',function(){}); \
                     _lumen_add_listener(99,'click',function(){});")
        .unwrap();
    // Verify target listeners are present before collect.
    let has42click = rt.eval("'42:click' in _lumen_listeners").unwrap();
    assert_eq!(has42click, lumen_core::JsValue::Bool(true));
    let has42over = rt.eval("'42:mouseover' in _lumen_listeners").unwrap();
    assert_eq!(has42over, lumen_core::JsValue::Bool(true));

    // Collect nid=42 → its entries should be deleted; nid=99 must survive.
    rt.eval("_lumen_gc_collect([42]);").unwrap();

    let gone42click = rt.eval("'42:click' in _lumen_listeners").unwrap();
    assert_eq!(gone42click, lumen_core::JsValue::Bool(false));
    let gone42over = rt.eval("'42:mouseover' in _lumen_listeners").unwrap();
    assert_eq!(gone42over, lumen_core::JsValue::Bool(false));
    // nid=99 must survive.
    let has99 = rt.eval("'99:click' in _lumen_listeners").unwrap();
    assert_eq!(has99, lumen_core::JsValue::Bool(true));
}

#[test]
fn gc_collect_removes_input_value_entry() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_input_values[7] = 'hello'; _input_values[8] = 'world';")
        .unwrap();
    rt.eval("_lumen_gc_collect([7]);").unwrap();

    // Deleted property → undefined → JsValue::Null (from_rq maps both).
    let v7 = rt.eval("_input_values[7]").unwrap();
    assert_eq!(v7, lumen_core::JsValue::Null);

    let v8 = rt.eval("_input_values[8]").unwrap();
    assert_eq!(v8, lumen_core::JsValue::String("world".into()));
}

#[test]
fn gc_collect_empty_array_is_noop() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_add_listener(5,'click',function(){});").unwrap();
    rt.eval("_lumen_gc_collect([]);").unwrap();
    // nid=5 listener must still be there.
    let has5 = rt.eval("'5:click' in _lumen_listeners").unwrap();
    assert_eq!(has5, lumen_core::JsValue::Bool(true));
}

#[test]
fn gc_collect_unknown_nid_is_noop() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_add_listener(3,'focus',function(){});").unwrap();
    rt.eval("_lumen_gc_collect([9999]);").unwrap();
    // nid=3 listener must still be there.
    let has3 = rt.eval("'3:focus' in _lumen_listeners").unwrap();
    assert_eq!(has3, lumen_core::JsValue::Bool(true));
}

// ── deterministic render mode (8F) tests ─────────────────────────────────

#[test]
fn deterministic_date_now_returns_zero() {
    let rt = v8_runtime_deterministic(make_doc(), "http://x.com/#test");
    let v = rt.eval("Date.now()").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(0.0), "Date.now() must be 0 in deterministic mode");
}

#[test]
fn deterministic_performance_now_returns_zero() {
    let rt = v8_runtime_deterministic(make_doc(), "http://x.com/");
    let v = rt.eval("performance.now()").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(0.0), "performance.now() must be 0 in deterministic mode");
}

#[test]
fn deterministic_math_random_reproducible() {
    // Two runtimes with same URL fragment must produce identical random sequences.
    let rt_a = v8_runtime_deterministic(make_doc(), "http://x.com/#seed42");
    let rt_b = v8_runtime_deterministic(make_doc(), "http://y.org/other#seed42");
    let seq_a: Vec<_> = (0..5).map(|_| rt_a.eval("Math.random()").unwrap()).collect();
    let seq_b: Vec<_> = (0..5).map(|_| rt_b.eval("Math.random()").unwrap()).collect();
    assert_eq!(seq_a, seq_b, "same fragment → same random sequence");
}

#[test]
fn deterministic_math_random_different_seeds() {
    // Different fragments must produce different sequences.
    let rt_a = v8_runtime_deterministic(make_doc(), "http://x.com/#foo");
    let rt_b = v8_runtime_deterministic(make_doc(), "http://x.com/#bar");
    let r_a = rt_a.eval("Math.random()").unwrap();
    let r_b = rt_b.eval("Math.random()").unwrap();
    assert_ne!(r_a, r_b, "different fragments → different random values");
}

#[test]
fn deterministic_math_random_in_range() {
    let rt = v8_runtime_deterministic(make_doc(), "http://x.com/#test");
    for _ in 0..20 {
        if let lumen_core::JsValue::Number(v) = rt.eval("Math.random()").unwrap() {
            assert!((0.0..1.0).contains(&v), "Math.random() must be in [0, 1): got {v}");
        } else {
            panic!("Math.random() must return a number");
        }
    }
}

#[test]
fn normal_mode_date_now_nonzero() {
    // In non-deterministic mode Date.now() must return a positive value (wall clock).
    let rt = v8_runtime_with_dom(make_doc());
    if let lumen_core::JsValue::Number(v) = rt.eval("Date.now()").unwrap() {
        assert!(v > 0.0, "Date.now() must be positive in normal mode");
    } else {
        panic!("Date.now() must return a number");
    }
}

// ── DEVX-16: --rng-seed / --monotonic-clock reach the JS runtime ─────────

#[test]
fn rng_seed_override_beats_url_hash() {
    // Different URLs, but the same explicit `--rng-seed` override, must
    // produce identical Math.random sequences (the override takes
    // precedence over URL-hash derivation).
    let rt_a = v8_runtime_deterministic_cfg(make_doc(), "http://x.com/#foo", Some(7), false);
    let rt_b = v8_runtime_deterministic_cfg(make_doc(), "http://y.org/other#bar", Some(7), false);
    let seq_a: Vec<_> = (0..5).map(|_| rt_a.eval("Math.random()").unwrap()).collect();
    let seq_b: Vec<_> = (0..5).map(|_| rt_b.eval("Math.random()").unwrap()).collect();
    assert_eq!(seq_a, seq_b, "same --rng-seed override → same random sequence regardless of URL");
}

#[test]
fn rng_seed_override_differs_from_default_url_derivation() {
    // Same URL fragment, but one runtime gets an explicit override —
    // the override must NOT collapse to the URL-hash-derived sequence.
    let rt_default = v8_runtime_deterministic(make_doc(), "http://x.com/#test");
    let rt_override = v8_runtime_deterministic_cfg(make_doc(), "http://x.com/#test", Some(999), false);
    let r_default = rt_default.eval("Math.random()").unwrap();
    let r_override = rt_override.eval("Math.random()").unwrap();
    assert_ne!(r_default, r_override, "--rng-seed override must change the sequence vs URL-hash default");
}

// Extracts the f64 payload of a `Number` JsValue, panicking otherwise —
// the monotonic-clock tests below compare deltas rather than absolute
// values, since `performance` shim install already consumes one tick of
// the shared counter (`_perf_origin_ms = _lumen_now_ms()`).
fn expect_number(v: lumen_core::JsValue) -> f64 {
    match v {
        lumen_core::JsValue::Number(n) => n,
        other => panic!("expected Number, got {other:?}"),
    }
}

#[test]
fn monotonic_clock_advances_date_now() {
    let rt = v8_runtime_deterministic_cfg(make_doc(), "http://x.com/#test", None, true);
    let a = expect_number(rt.eval("Date.now()").unwrap());
    let b = expect_number(rt.eval("Date.now()").unwrap());
    let c = expect_number(rt.eval("Date.now()").unwrap());
    assert_eq!(b - a, 1.0, "each Date.now() call must advance by exactly 1 ms");
    assert_eq!(c - b, 1.0, "each Date.now() call must advance by exactly 1 ms");
}

#[test]
fn monotonic_clock_advances_performance_now() {
    let rt = v8_runtime_deterministic_cfg(make_doc(), "http://x.com/#test", None, true);
    let a = expect_number(rt.eval("performance.now()").unwrap());
    let b = expect_number(rt.eval("performance.now()").unwrap());
    assert_eq!(b - a, 1.0, "each performance.now() call must advance by exactly 1 ms");
}

#[test]
fn monotonic_clock_shares_one_counter_across_date_and_performance() {
    // Date.now() and performance.now() both read `_lumen_now_ms()`, so an
    // interleaved performance.now() call must consume a tick of the SAME
    // counter Date.now() reads — two consecutive Date.now() calls with one
    // performance.now() call between them must be 2 ms apart, not 1.
    let rt = v8_runtime_deterministic_cfg(make_doc(), "http://x.com/#test", None, true);
    let a = expect_number(rt.eval("Date.now()").unwrap());
    let _ = rt.eval("performance.now()").unwrap();
    let c = expect_number(rt.eval("Date.now()").unwrap());
    assert_eq!(c - a, 2.0, "an interleaved performance.now() call must advance the shared counter too");
}

#[test]
fn without_monotonic_clock_date_now_stays_frozen() {
    // Default deterministic mode (monotonic_clock = false) keeps the
    // pre-DEVX-16 frozen-at-0 behaviour on repeated calls.
    let rt = v8_runtime_deterministic(make_doc(), "http://x.com/#test");
    let a = rt.eval("Date.now()").unwrap();
    let b = rt.eval("Date.now()").unwrap();
    assert_eq!(a, lumen_core::JsValue::Number(0.0));
    assert_eq!(b, lumen_core::JsValue::Number(0.0));
}

// ─── window.open() / window.opener tests ─────────────────────────────────

#[test]
fn window_open_function_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.open === 'function'"));
}

#[test]
fn window_opener_is_null() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.opener === null"));
}

#[test]
fn window_open_queues_popup_request() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.open('https://example.com', '_blank', 'width=800,height=600')")
        .unwrap();
    let reqs = rt.take_window_open_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "https://example.com");
    assert_eq!(reqs[0].target, "_blank");
    assert_eq!(reqs[0].width, 800);
    assert_eq!(reqs[0].height, 600);
}

#[test]
fn window_open_empty_url_defaults_to_empty_string() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.open()").unwrap();
    let reqs = rt.take_window_open_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "");
}

#[test]
fn window_open_returns_stub_object() {
    let rt = v8_runtime_with_dom(make_doc());
    // Should return an object (not null/undefined) with a close() method.
    assert!(bool_eval(
        &rt,
        "var w = window.open('about:blank'); typeof w === 'object' && w !== null && typeof w.close === 'function'"
    ));
}

#[test]
fn window_open_stub_location_href() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var w = window.open('https://lumen.example/'); w.location.href === 'https://lumen.example/'"
    ));
}

#[test]
fn window_open_multiple_calls_queue_all() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.open('https://a.com'); window.open('https://b.com', '_self')").unwrap();
    let reqs = rt.take_window_open_requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].url, "https://a.com");
    assert_eq!(reqs[1].url, "https://b.com");
}

#[test]
fn window_open_feature_parsing_partial() {
    // Only width specified — height should default to 600.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.open('https://x.com', '', 'width=1024')").unwrap();
    let reqs = rt.take_window_open_requests();
    assert_eq!(reqs[0].width, 1024);
    assert_eq!(reqs[0].height, 600);
}

#[test]
fn window_open_take_clears_queue() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.open('https://a.com')").unwrap();
    let first = rt.take_window_open_requests();
    assert_eq!(first.len(), 1);
    // Second drain must be empty.
    let second = rt.take_window_open_requests();
    assert_eq!(second.len(), 0);
}

// BUG-359: `window.open("relative.html")` must resolve against the
// opener's document URL before being queued — previously the raw
// string reached the shell/network layer and failed with
// `missing scheme`.
#[test]
fn window_open_resolves_relative_url() {
    let rt = v8_runtime_deterministic(make_doc(), "https://example.com/dir/page.html");
    rt.eval("window.open('support/x.html')").unwrap();
    let reqs = rt.take_window_open_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "https://example.com/dir/support/x.html");
}

#[test]
fn window_open_stub_location_href_resolves_relative_url() {
    let rt = v8_runtime_deterministic(make_doc(), "https://example.com/dir/page.html");
    assert!(bool_eval(
        &rt,
        "var w = window.open('support/x.html'); w.location.href === 'https://example.com/dir/support/x.html'"
    ));
}

// ── Web Animations API ─────────────────────────────────────────────────

#[test]
fn web_animations_classes_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.Animation === 'function' && typeof window.KeyframeEffect === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn keyframe_effect_stores_keyframes() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var kf = new KeyframeEffect(null, [{opacity:0},{opacity:1}], 300); \
                 kf.getKeyframes().length"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

#[test]
fn keyframe_effect_timing_duration() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var kf = new KeyframeEffect(null, [], {duration:500, delay:100}); \
                 kf.getTiming().duration"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(500.0));
}

#[test]
fn animation_initial_state_is_idle() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.playState"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("idle".into()));
}

#[test]
fn animation_play_changes_state() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.play(); \
                 a.playState"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("running".into()));
}

#[test]
fn animation_pause_changes_state() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.play(); a.pause(); \
                 a.playState"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("paused".into()));
}

#[test]
fn animation_cancel_removes_from_registry() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.play(); a.cancel();"
    ).unwrap();
    let r = rt.eval("document.getAnimations().length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn document_timeline_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.timeline instanceof DocumentTimeline").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_timeline_current_time_null_before_raf() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.timeline.currentTime === null").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_timeline_current_time_after_raf() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_run_raf_callbacks(100.0)").unwrap();
    let r = rt.eval("document.timeline.currentTime >= 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_animate_returns_animation() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); \
                 var a = el.animate([{opacity:0},{opacity:1}], 300); \
                 a instanceof Animation"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_animate_play_state_running() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); \
                 var a = el.animate([{opacity:0},{opacity:1}], 300); \
                 a.playState"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("running".into()));
}

#[test]
fn element_get_animations() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); \
                 el.animate([{opacity:0},{opacity:1}], 500); \
                 el.getAnimations().length"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn document_get_animations() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); \
                 el.animate([{opacity:0},{opacity:1}], 500); \
                 document.getAnimations().length"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

/// The `_lumen_tick_timers()` is required since BUG-808: the playback
/// events are queued as event-loop tasks (§4.4.3), not dispatched
/// inside `finish()`.
#[test]
fn animation_finish_fires_onfinish() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fired = false; \
                 var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.onfinish = function() { fired = true; }; \
                 a.finish(); _lumen_tick_timers(); \
                 fired"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn animation_finish_state() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.finish(); \
                 a.playState"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("finished".into()));
}

#[test]
fn keyframe_effect_property_indexed_form() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var kf = new KeyframeEffect(null, {opacity: [0, 0.5, 1]}, 400); \
                 kf.getKeyframes().length"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(3.0));
}

#[test]
fn animation_reverse_negates_playback_rate() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var a = new Animation(new KeyframeEffect(null, [], 300)); \
                 a.play(); \
                 var rate_before = a.playbackRate; \
                 a.reverse(); \
                 a.playbackRate === -rate_before"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_animate_applies_opacity_style() {
    let rt = v8_runtime_with_dom(make_doc());
    // Advance time then tick to let the animation apply its first frame.
    let r = rt.eval(
        "var el = document.createElement('div'); \
                 document.body.appendChild(el); \
                 _wa_current_time = 0; \
                 var a = el.animate([{opacity:0},{opacity:1}], {duration:1000}); \
                 // At t=0 the animation should set opacity to 0
                 a._applyAtP(0); \
                 el.style.opacity"
    ).unwrap();
    // opacity at progress=0 should be '0'
    assert_eq!(r, lumen_core::JsValue::String("0".into()));
}

// ── CompressionStream / DecompressionStream (WHATWG Compression Streams) ──
//
// V8 twin note: the originals interleaved write/close/read().then()/assert
// inside ONE eval() with `_lumen_drain_microtasks()` calls to force
// resolution ordering — a QuickJS-only mechanism (no-op on V8, S12b-2
// lesson). On V8 the microtask checkpoint runs after each top-level
// `eval()` returns, so every promise resolution boundary is split into
// its own `rt.eval()` call instead.

#[test]
fn compression_stream_constructor_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "typeof CompressionStream === 'function' && \
                     typeof DecompressionStream === 'function'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compression_stream_invalid_format_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var threw = false; \
                     try { new CompressionStream('lz4'); } catch(e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn decompression_stream_invalid_format_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var threw = false; \
                     try { new DecompressionStream('lz4'); } catch(e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compression_stream_has_readable_writable() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var cs = new CompressionStream('gzip'); \
                     cs.readable instanceof ReadableStream && cs.writable instanceof WritableStream",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compression_stream_is_transform_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("new CompressionStream('deflate') instanceof TransformStream")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Drains a `ReadableStream` reader into `sink.parts` and concatenates.
///
/// Since BUG-846 a compression stream emits output as it goes rather
/// than one chunk at `close()`, so a test that reads once no longer
/// holds the whole payload — the gzip header alone can arrive as its
/// own chunk. Every test below therefore collects, exactly as WPT's
/// `concatenate-stream.js` does.
const COLLECT_JS: &str = "function _collect(rd, sink) { \
                 rd.read().then(function(r) { \
                     if (r.done) { sink.done = true; return; } \
                     sink.parts.push(r.value); _collect(rd, sink); \
                 }, function() { sink.error = true; }); } \
             function _joined(sink) { var n = 0, i; \
                 for (i = 0; i < sink.parts.length; i++) n += sink.parts[i].length; \
                 var o = new Uint8Array(n), k = 0; \
                 for (i = 0; i < sink.parts.length; i++) { o.set(sink.parts[i], k); k += sink.parts[i].length; } \
                 return o; } \
             function _sink() { return { parts: [], done: false, error: false }; } ";

#[test]
fn compression_stream_gzip_produces_nonempty_output() {
    let rt = v8_runtime_with_dom(make_doc());
    // Write [72,101,108,108,111] = "Hello", close, collect the output.
    rt.eval(&format!(
        "{COLLECT_JS} \
                 var cs = new CompressionStream('gzip'); \
                 var writer = cs.writable.getWriter(); \
                 var reader = cs.readable.getReader(); \
                 writer.write(new Uint8Array([72,101,108,108,111])); \
                 writer.close(); \
                 var sink = _sink(); _collect(reader, sink);"
    ))
    .unwrap();
    let r = rt
        .eval("sink.done && !sink.error && _joined(sink).length > 0")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-846: the decoded bytes must reach the reader from the *write*,
/// with the writable side still open. The buffer-then-flush model this
/// replaced left this read pending forever, which is what made three
/// `compression/decompression-*` WPT files time out.
#[test]
fn decompression_stream_emits_without_closing_the_writer() {
    let rt = v8_runtime_with_dom(make_doc());
    // WPT's own vector: zlib for the string 'expected output'.
    rt.eval(
        "var ds = new DecompressionStream('deflate'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(new Uint8Array([120,156,75,173,40,72,77,46,73,77,81,200,47,45,41,40,45,1,0,48,173,6,36])); \
                 var got = null, rejected = false; \
                 dr.read().then(function(r) { got = r.value; }, function() { rejected = true; });",
    )
    .unwrap();
    let r = rt
        .eval(
            "!rejected && got instanceof Uint8Array && \
                     String.fromCharCode.apply(null, Array.from(got)) === 'expected output'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A chunk that is not a BufferSource is a WebIDL conversion failure,
/// which errors both sides (`decompression-bad-chunks`) — it used to be
/// silently read as an empty chunk.
#[test]
fn decompression_stream_rejects_non_buffer_source_chunk() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ds = new DecompressionStream('gzip'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 var writeRejected = false, readRejected = false; \
                 dw.write('not a buffer').then(function() {}, function() { writeRejected = true; }); \
                 dr.read().then(function() {}, function() { readRejected = true; });",
    )
    .unwrap();
    let r = rt.eval("writeRejected && readRejected").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Junk behind the end of a stream must not swallow the bytes decoded
/// before it: the value arrives, and only the *next* read rejects
/// (`decompression-extra-input`).
#[test]
fn decompression_stream_extra_input_delivers_then_errors() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ds = new DecompressionStream('deflate'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(new Uint8Array([120,156,75,173,40,72,77,46,73,77,81,200,47,45,41,40,45,1,0,48,173,6,36,0])).then(function(){}, function(){}); \
                 var first = null, secondRejected = false; \
                 dr.read().then(function(r) { first = r.value; \
                     dr.read().then(function() {}, function() { secondRejected = true; }); });",
    )
    .unwrap();
    let r = rt
        .eval("first instanceof Uint8Array && first.length === 15 && secondRejected")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A body cut one byte short of its own trailer is an error, not a
/// successful decode (`decompression-corrupt-input`).
#[test]
fn decompression_stream_truncated_input_errors_at_close() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ds = new DecompressionStream('deflate'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(new Uint8Array([120,156,75,173,40,72,77,46,73,77,81,200,47,45,41,40,45,1,0,48,173,6])).then(function(){}, function(){}); \
                 dw.close().then(function(){}, function(){}); \
                 var closedRejected = false; \
                 dr.closed.then(function() {}, function() { closedRejected = true; });",
    )
    .unwrap();
    let r = rt.eval("closedRejected").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compression_stream_gzip_round_trip() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{COLLECT_JS} \
                 var input = new Uint8Array([72,101,108,108,111]); \
                 var cs = new CompressionStream('gzip'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 var csink = _sink(); _collect(cr, csink);"
    ))
    .unwrap();
    rt.eval(
        "var ds = new DecompressionStream('gzip'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(_joined(csink)); dw.close(); \
                 var dsink = _sink(); _collect(dr, dsink);",
    )
    .unwrap();
    let r = rt
        .eval(
            "var result = _joined(dsink); \
                     result.length === 5 && result[0] === 72 && result[4] === 111",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compression_stream_deflate_round_trip() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{COLLECT_JS} \
                 var input = new Uint8Array([65,66,67]); \
                 var cs = new CompressionStream('deflate'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 var csink = _sink(); _collect(cr, csink);"
    ))
    .unwrap();
    rt.eval(
        "var ds = new DecompressionStream('deflate'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(_joined(csink)); dw.close(); \
                 var dsink = _sink(); _collect(dr, dsink);",
    )
    .unwrap();
    let r = rt
        .eval(
            "var result = _joined(dsink); \
                     result.length === 3 && result[0] === 65 && result[2] === 67",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compression_stream_deflate_raw_round_trip() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{COLLECT_JS} \
                 var input = new Uint8Array([1,2,3,4,5]); \
                 var cs = new CompressionStream('deflate-raw'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 var csink = _sink(); _collect(cr, csink);"
    ))
    .unwrap();
    rt.eval(
        "var ds = new DecompressionStream('deflate-raw'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(_joined(csink)); dw.close(); \
                 var dsink = _sink(); _collect(dr, dsink);",
    )
    .unwrap();
    let r = rt
        .eval(
            "var result = _joined(dsink); \
                     result.length === 5 && result[0] === 1 && result[4] === 5",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn decompression_stream_corrupt_input_errors_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    // Feeding non-gzip bytes to a gzip DecompressionStream must error the
    // readable side (https://compression.spec.whatwg.org/), so reader.read()
    // rejects rather than resolving with an empty/undefined chunk.
    rt.eval(
        "var ds = new DecompressionStream('gzip'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(new Uint8Array([1,2,3,4,5,6,7,8])); dw.close(); \
                 var errored = false, resolved = false; \
                 dr.read().then(function() { resolved = true; }, function() { errored = true; });",
    )
    .unwrap();
    let r = rt.eval("errored && !resolved").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn decompression_stream_multi_chunk_matches_single_chunk() {
    let rt = v8_runtime_with_dom(make_doc());
    // Splitting the compressed body across several writes must decode to
    // the same bytes as feeding it in one chunk — the codec has to keep
    // its state between chunks (`decompression-split-chunk`).
    rt.eval(&format!(
        "{COLLECT_JS} \
                 var input = new Uint8Array([9,8,7,6,5,4,3,2,1,0]); \
                 var cs = new CompressionStream('deflate'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 var csink = _sink(); _collect(cr, csink);"
    ))
    .unwrap();
    rt.eval(
        "var compressed = _joined(csink); \
                 var ds = new DecompressionStream('deflate'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 var mid = compressed.length >> 1; \
                 dw.write(compressed.slice(0, mid)); \
                 dw.write(compressed.slice(mid)); \
                 dw.close(); \
                 var dsink = _sink(); _collect(dr, dsink);",
    )
    .unwrap();
    let r = rt
        .eval(
            "var result = _joined(dsink); \
                     result.length === 10 && result[0] === 9 && result[9] === 0",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
