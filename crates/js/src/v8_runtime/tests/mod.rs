//! Тесты `v8_runtime.rs`, вынесенные из inline-модуля `mod tests` (дорожка
//! SPLIT, батч JS-4).
//!
//! Здесь осталась первая половина модуля — фикстура `rt()`, которую видят все
//! потомки, и темы в исходном порядке: перепись PERF-9, отчёт об исключениях
//! (`eval_and_report`, BUG-591), модули (`eval_module_*`), compat-слой S2,
//! `install_dom` (S3), обратная доставка ресурсных событий (BUG-480 срез 10) и
//! трекер мутаций DOM `take_dom_touched` (BUG-341 S7). Остальное — в
//! `dom_suspend_focus.rs` рядом.

use super::*;
use lumen_core::JsRuntime;

fn rt() -> V8JsRuntime {
    V8JsRuntime::new().unwrap()
}

/// PERF-9 census: how much of a navigation's cost is the shim, and does the
/// shim survive evaluation with **no** `_lumen_*` natives registered.
///
/// Both questions decide whether a V8 startup snapshot is worth building:
/// the snapshot can only capture pure-JS state, so a shim that calls a
/// native at top level and bakes the result cannot be snapshotted as-is.
/// Prints rather than asserts — this is a measurement, not a gate. Run with
/// `cargo test -p lumen-js --features v8-backend perf9 -- --nocapture`.
#[test]
fn perf9_census_shim_eval_without_natives() {
    let shim = crate::dom::web_api_shim();
    // A fresh runtime is a fresh isolate + context with the global template,
    // i.e. exactly the per-navigation starting point — but no natives.
    let rt = rt();
    let t0 = std::time::Instant::now();
    let outcome = rt.eval(&format!("{shim}\nundefined;\n"));
    let elapsed = t0.elapsed();
    eprintln!("[PERF-9] shim size: {} bytes", shim.len());
    eprintln!("[PERF-9] eval (no natives): {:?}", elapsed);
    match &outcome {
        Ok(_) => eprintln!("[PERF-9] shim evaluates WITHOUT natives — snapshot is viable as-is"),
        Err(e) => eprintln!("[PERF-9] shim NEEDS natives at eval time: {e:?}"),
    }
}

/// PERF-9 census, second half: the real per-navigation JS cost, split into
/// isolate+context creation and `install_dom` (native registration + shim
/// eval). A snapshot can only ever remove part of the second number, so
/// this is the ceiling on what PERF-9 can win. Printed, not asserted.
#[test]
fn perf9_census_install_dom_cost() {
    // Three rounds: the first pays one-time V8 platform init, the later two
    // are the steady state a real navigation sees.
    for round in 1..=3 {
        let t0 = std::time::Instant::now();
        let rt = V8JsRuntime::new().unwrap();
        let t_new = t0.elapsed();
        let t1 = std::time::Instant::now();
        rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false)
            .unwrap();
        let t_install = t1.elapsed();
        eprintln!(
            "[PERF-9] round {round}: V8JsRuntime::new {t_new:?} + install_dom {t_install:?} = {:?}",
            t_new + t_install
        );
    }
}

/// PERF-9: the module-shim bytecode cache (`CODE_CACHE`) must not change
/// what a script *returns* — only how fast it compiles. Evals the same
/// constant script above `CODE_CACHE_MIN_LEN` in three fresh isolates,
/// asserts identical results, and checks the cache entry under this
/// test's own key is populated by the miss and left untouched by the
/// two hits that follow.
#[test]
fn perf9_code_cache_hit_preserves_semantics() {
    let big_script = format!(
        "(function() {{ var acc = 0; for (var i = 0; i < 3; i++) {{ acc += i; }} return acc; }})();\n{}",
        "// padding to clear CODE_CACHE_MIN_LEN\n".repeat(200)
    );
    assert!(big_script.len() >= CODE_CACHE_MIN_LEN, "test script must exceed the caching threshold");
    let hash = code_cache_hash(&big_script);

    CODE_CACHE.lock().unwrap_or_else(|e| e.into_inner()).remove(&hash);

    let first = rt().eval(&big_script).unwrap();
    assert_eq!(first, JsValue::Number(3.0));
    // `CODE_CACHE` is process-wide and this test runs alongside every
    // other test thread, so its total length is not a safe thing to
    // assert on (BUG-class: another thread's unrelated `eval()` can grow
    // it between two reads). Its byte content under *this test's own*
    // key is race-free — nothing else in the suite evals this exact
    // padded script — so pin that instead of the map's size.
    let bytes_after_miss = CODE_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&hash)
        .cloned();
    assert!(
        bytes_after_miss.is_some(),
        "a miss above the threshold must populate the cache"
    );

    for _ in 0..2 {
        let again = rt().eval(&big_script).unwrap();
        assert_eq!(again, JsValue::Number(3.0));
    }
    let bytes_after_hits = CODE_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&hash).cloned();
    assert_eq!(
        bytes_after_hits, bytes_after_miss,
        "a cache hit must not rewrite or drop this test's own entry"
    );
}

/// PERF-9: scripts below `CODE_CACHE_MIN_LEN` (the vast majority of
/// `eval()` calls — one-off dynamic snippets like `_lumen_focus_update`)
/// must never be cached, so the cache cannot grow unbounded on them.
#[test]
fn perf9_code_cache_skips_small_scripts() {
    let small_script = "1 + 1";
    assert!(small_script.len() < CODE_CACHE_MIN_LEN);
    let hash = code_cache_hash(small_script);
    assert_eq!(rt().eval(small_script).unwrap(), JsValue::Number(2.0));
    assert!(
        !CODE_CACHE.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&hash),
        "a small script must not be added to the code cache"
    );
}

/// PERF-9 census, third half: **which** natives the shim needs while it is
/// being evaluated. Every one of these is a per-document Rust closure, so
/// each is a site that must become lazy before a snapshot can be taken.
/// Feeds stub definitions in one at a time, following the interpreter's own
/// "X is not defined" errors until the shim either completes or fails for a
/// reason other than a missing native. Printed, not asserted.
#[test]
fn perf9_census_top_level_native_deps() {
    let shim = crate::dom::web_api_shim();
    let mut stubs: Vec<String> = Vec::new();
    for step in 0..60 {
        let prelude = stubs
            .iter()
            .map(|n| format!("globalThis.{n} = function() {{ return undefined; }};\n"))
            .collect::<String>();
        // Fresh isolate each round: a failed shim leaves half-built globals.
        let rt = V8JsRuntime::new().unwrap();
        match rt.eval(&format!("{prelude}{shim}\nundefined;\n")) {
            Ok(_) => {
                eprintln!("[PERF-9] shim completed after stubbing {} natives", stubs.len());
                break;
            }
            Err(e) => {
                let msg = format!("{e:?}");
                // "_lumen_foo is not defined" → stub it and go round again.
                // The name is embedded in a Debug-formatted string, so scan
                // for the marker rather than splitting on whitespace.
                let missing = msg.find("_lumen_").filter(|_| msg.contains("is not defined")).map(|at| {
                    msg[at..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect::<String>()
                });
                match missing {
                    Some(name) => {
                        eprintln!("[PERF-9] step {step}: needs {name}");
                        stubs.push(name);
                    }
                    None => {
                        eprintln!(
                            "[PERF-9] stopped after {} stubs, non-missing-native failure: {msg}",
                            stubs.len()
                        );
                        break;
                    }
                }
            }
        }
    }
    eprintln!("[PERF-9] top-level native deps ({}): {:?}", stubs.len(), stubs);
}

#[test]
fn eval_number() {
    assert_eq!(rt().eval("1 + 2").unwrap(), JsValue::Number(3.0));
}

#[test]
fn eval_string() {
    assert_eq!(
        rt().eval(r#""hello" + " world""#).unwrap(),
        JsValue::String("hello world".into())
    );
}

#[test]
fn eval_bool() {
    assert_eq!(rt().eval("true").unwrap(), JsValue::Bool(true));
    assert_eq!(rt().eval("false").unwrap(), JsValue::Bool(false));
}

#[test]
fn eval_null() {
    assert_eq!(rt().eval("null").unwrap(), JsValue::Null);
}

#[test]
fn debug_heap_stats_reports_nonzero_isolate_heap() {
    let (total, used) = rt().debug_heap_stats();
    assert!(total > 0, "total_heap_size should be positive, got {total}");
    assert!(used > 0, "used_heap_size should be positive, got {used}");
    assert!(used <= total, "used ({used}) should not exceed total ({total})");
}

#[test]
fn set_get_global() {
    let rt = rt();
    rt.set_global("x", JsValue::Number(42.0)).unwrap();
    assert_eq!(rt.get_global("x").unwrap(), JsValue::Number(42.0));
}

#[test]
fn set_global_string() {
    let rt = rt();
    rt.set_global("greeting", JsValue::String("hi".into()))
        .unwrap();
    assert_eq!(
        rt.get_global("greeting").unwrap(),
        JsValue::String("hi".into())
    );
}

#[test]
fn call_function_add() {
    let rt = rt();
    rt.eval("function add(a, b) { return a + b; }").unwrap();
    assert_eq!(
        rt.call_function("add", &[JsValue::Number(3.0), JsValue::Number(4.0)])
            .unwrap(),
        JsValue::Number(7.0)
    );
}

#[test]
fn call_function_no_args() {
    let rt = rt();
    rt.eval("function forty_two() { return 42; }").unwrap();
    assert_eq!(
        rt.call_function("forty_two", &[]).unwrap(),
        JsValue::Number(42.0)
    );
}

#[test]
fn eval_array() {
    assert_eq!(
        rt().eval("[1, 2, 3]").unwrap(),
        JsValue::Array(vec![
            JsValue::Number(1.0),
            JsValue::Number(2.0),
            JsValue::Number(3.0),
        ])
    );
}

#[test]
fn eval_object() {
    let val = rt().eval(r#"({ a: 1, b: "x" })"#).unwrap();
    assert_eq!(
        val,
        JsValue::object([
            ("a".to_string(), JsValue::Number(1.0)),
            ("b".to_string(), JsValue::String("x".into())),
        ])
    );
}

#[test]
fn eval_circular_object_does_not_crash() {
    // BUG-633: a self-referential object (the shape of WPT's
    // `testharness.js` `Test`/`EventExpectationsManager` pair) must not
    // send `from_v8` into unbounded recursion — it should come back as a
    // `[Circular]` marker instead of hanging or crashing the process.
    let val = rt().eval("var o = {}; o.self = o; o").unwrap();
    match val {
        JsValue::Object(entries) => {
            let self_val = entries.into_iter().find(|(k, _)| k == "self").map(|(_, v)| v);
            assert_eq!(self_val, Some(JsValue::String("[Circular]".into())));
        }
        other => panic!("expected object, got {other:?}"),
    }
}

#[test]
fn eval_deeply_nested_array_truncates() {
    // BUG-633: a pathologically deep (but non-cyclic) structure must
    // also be bounded, not just cycles — otherwise a long linked chain
    // hits the same unbounded-recursion crash.
    let script = format!("{}{}{}", "[".repeat(200), "1", "]".repeat(200));
    let val = rt().eval(&script).unwrap();
    let mut current = val;
    let mut depth = 0usize;
    loop {
        match current {
            JsValue::Array(mut items) if items.len() == 1 => {
                current = items.pop().unwrap();
                depth += 1;
            }
            JsValue::String(s) => {
                assert_eq!(s, "[Max Depth Exceeded]");
                break;
            }
            other => panic!("unexpected {other:?} at depth {depth}"),
        }
    }
    assert!(depth <= FROM_V8_MAX_DEPTH, "depth {depth} exceeded cap");
}

#[test]
fn eval_runtime_error() {
    assert!(matches!(
        rt().eval("throw new Error('boom')"),
        Err(JsError::Runtime(_))
    ));
}

#[test]
fn eval_syntax_error() {
    assert!(matches!(rt().eval("function ("), Err(JsError::Runtime(_))));
}

// ── eval_and_report (BUG-591) ────────────────────────────────────────────

/// No `_lumen_report_exception` exists on a bare runtime (no DOM shim
/// installed) — the lookup fails and is silently skipped (see the method's
/// own doc comment), so behaviour must otherwise be identical to `eval()`.
/// The end-to-end path (with the shim installed, actually reaching
/// `window`'s 'error'/onerror pipeline) is covered by the
/// `dom::tests::v8_core::bug591_*` tests, which also exercise the
/// `filename`/`lineno`/`colno` extracted here via `v8::Message`.
#[test]
fn eval_and_report_matches_eval_on_success() {
    assert_eq!(
        rt().eval_and_report("1 + 2").unwrap(),
        JsValue::Number(3.0)
    );
}

#[test]
fn eval_and_report_runtime_error() {
    assert!(matches!(
        rt().eval_and_report("throw new Error('boom')"),
        Err(JsError::Runtime(_))
    ));
}

#[test]
fn eval_and_report_syntax_error() {
    assert!(matches!(
        rt().eval_and_report("function ("),
        Err(JsError::Runtime(_))
    ));
}

// ── eval_module_and_report / eval_module_at_and_report (BUG-591, module
// scripts) ────────────────────────────────────────────────────────────

/// A module's own top-level runtime error (evaluation starts and the
/// module reaches `ModuleStatus::Errored`) must reach
/// `window.onerror`/'error' the same way a classic script's does -- the
/// gap `eval_module_and_report` closes.
#[test]
fn eval_module_and_report_runtime_error_fires_window_error() {
    let rt = runtime_with_dom(make_doc(), "");
    rt.eval(
        "var caught = null; \
             window.addEventListener('error', function(e) { caught = e.message; });",
    )
    .unwrap();
    let outcome = rt.eval_module_and_report("throw new Error('module-boom');");
    assert!(matches!(outcome, Err(JsError::Runtime(_))));
    let caught = rt.eval("caught").unwrap();
    assert_eq!(caught, JsValue::String("module-boom".to_string()));
}

/// A module **load** failure (parse/link error -- the module body never
/// starts evaluating) must NOT reach `window.onerror`: per HTML LS that
/// belongs to the script element's own `error` event instead, and
/// reporting it here would misfire `window.onerror` on an ordinary
/// syntax error or missing import.
#[test]
fn eval_module_and_report_load_error_does_not_fire_window_error() {
    let rt = runtime_with_dom(make_doc(), "");
    rt.eval(
        "var caught = null; \
             window.addEventListener('error', function(e) { caught = e.message; });",
    )
    .unwrap();
    let outcome = rt.eval_module_and_report("function (");
    assert!(matches!(outcome, Err(JsError::Runtime(_))));
    let caught = rt.eval("caught").unwrap();
    assert_eq!(caught, JsValue::Null);
}

/// External-module counterpart (`<script type=module src=URL>`), same
/// runtime-error-only reporting rule.
#[test]
fn eval_module_at_and_report_runtime_error_fires_window_error() {
    let rt = runtime_with_dom(make_doc(), "");
    rt.eval(
        "var caught = null; \
             window.addEventListener('error', function(e) { caught = e.message; });",
    )
    .unwrap();
    let outcome = rt.eval_module_at_and_report(
        "https://example.test/mod.js",
        "throw new Error('url-module-boom');",
    );
    assert!(matches!(outcome, Err(JsError::Runtime(_))));
    let caught = rt.eval("caught").unwrap();
    assert_eq!(caught, JsValue::String("url-module-boom".to_string()));
}

#[test]
fn round_trip_bool() {
    let rt = rt();
    rt.set_global("flag", JsValue::Bool(true)).unwrap();
    assert_eq!(rt.eval("flag").unwrap(), JsValue::Bool(true));
}

#[test]
fn round_trip_array() {
    let rt = rt();
    rt.set_global(
        "arr",
        JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0)]),
    )
    .unwrap();
    assert_eq!(rt.eval("arr[0] + arr[1]").unwrap(), JsValue::Number(3.0));
}

#[test]
fn engine_name() {
    assert_eq!(rt().engine_name(), "v8");
}

#[test]
fn is_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<V8JsRuntime>();
}

#[test]
fn resume_produces_functional_runtime() {
    let fresh = V8JsRuntime::resume(SuspendedHeap::default()).unwrap();
    assert_eq!(fresh.eval("6 * 7").unwrap(), JsValue::Number(42.0));
}

// ── S2: compat-layer tests ────────────────────────────────────────────────

#[test]
fn console_log_callable_from_js() {
    use std::sync::{Arc, Mutex};
    let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let rt = rt();
    rt.install_console_natives(Arc::clone(&msgs)).unwrap();
    rt.eval("_lumen_console_log('hello')").unwrap();
    let captured = msgs.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0], (0, "hello".to_string()));
}

#[test]
fn console_warn_and_error_callable_from_js() {
    use std::sync::{Arc, Mutex};
    let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let rt = rt();
    rt.install_console_natives(Arc::clone(&msgs)).unwrap();
    rt.eval("_lumen_console_warn('w'); _lumen_console_error('e')")
        .unwrap();
    let captured = msgs.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], (1, "w".to_string()));
    assert_eq!(captured[1], (2, "e".to_string()));
}

#[test]
fn console_log_numeric_arg_coerced_to_string() {
    use std::sync::{Arc, Mutex};
    let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let rt = rt();
    rt.install_console_natives(Arc::clone(&msgs)).unwrap();
    // JS passes 42 (a Number) to a native expecting String — coerced to "42".
    rt.eval("_lumen_console_log(42)").unwrap();
    let captured = msgs.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].1, "42");
}

#[test]
fn native_registered_after_eval_is_accessible() {
    use std::sync::{Arc, Mutex};
    let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let rt = rt();
    rt.install_console_natives(Arc::clone(&msgs)).unwrap();
    // Calling the native inside a JS function defined after registration.
    rt.eval("function f(x) { _lumen_console_log(x); } f('ok')")
        .unwrap();
    assert_eq!(msgs.lock().unwrap()[0].1, "ok");
}

// ── S3: install_dom (DOM-core natives + WEB_API_SHIM) ───────────────────────

/// Builds `html > head > title > "Test Page"`, `html > body > div#main > span.highlight > "Hello"`.
/// Mirrors `dom::tests::make_doc`.
fn make_doc() -> Arc<Mutex<lumen_dom::Document>> {
    let mut doc = lumen_dom::Document::new();
    let html = doc.create_element(lumen_dom::QualName::html("html"));
    let head = doc.create_element(lumen_dom::QualName::html("head"));
    let title = doc.create_element(lumen_dom::QualName::html("title"));
    let title_text = doc.create_text("Test Page");
    let body = doc.create_element(lumen_dom::QualName::html("body"));
    let div = doc.create_element(lumen_dom::QualName::html("div"));
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
        attrs.push(lumen_dom::Attribute {
            name: lumen_dom::QualName::html("id"),
            value: "main".into(),
        });
    }
    let span = doc.create_element(lumen_dom::QualName::html("span"));
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(span).data {
        attrs.push(lumen_dom::Attribute {
            name: lumen_dom::QualName::html("class"),
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

/// Mirrors `dom::tests::runtime_with_dom`: a V8 runtime with DOM-core natives
/// and `WEB_API_SHIM` installed against `doc`, page URL `page_url`.
fn runtime_with_dom(doc: Arc<Mutex<lumen_dom::Document>>, page_url: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, page_url, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// BUG-480 срез 4: `install_dom` проставляет реестру ключ собственного
/// документа и origin страницы; postMessage в биндинг фрейма доходит через
/// `_lumen_frame_pump_messages`, а хук из WEB_API_SHIM строит полноценный
/// MessageEvent для window.onmessage.
#[test]
fn frame_post_message_self_delivery_through_install_dom() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    // Биндинг «фрейма» с документом, совпадающим с собственным, — механика
    // постановки в ящик та же, что у пары родитель↔ребёнок, но внутри
    // одного изолята. Слот родителя с about:-URL даёт источнику события
    // унаследованный origin (self_origin страницы).
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.register_parent_document(1, doc, "about:srcdoc".to_owned(), true);
    rt.eval("window.__got = null; window.onmessage = function(e) { window.__got = e; };")
        .unwrap();
    rt.eval("_lumen_frame_content_window(1).postMessage({n: 5}, '*')").unwrap();
    // До пумпы доставки нет — postMessage асинхронен.
    assert_eq!(
        rt.eval("window.__got === null").unwrap(),
        JsValue::Bool(true)
    );
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    // Хук WEB_API_SHIM доставил MessageEvent; origin about:-отправителя
    // унаследован от self_origin страницы (install_dom), source — фасад
    // слота родителя.
    assert!(matches!(
        rt.eval(
            "window.__got !== null && window.__got.data.n === 5 \
                 && window.__got.origin === 'https://parent.example' \
                 && window.__got.source !== null"
        )
        .unwrap(),
        JsValue::Bool(true)
    ));
}

/// BUG-480 срез 7: focus()/blur() через фасад документа обновляют
/// `_lumen_last_focused_nid` ЭТОГО контекста (activeElement меняется сразу
/// после пумпы), но НЕ ставят фокус-запрос шеллу — очередь рантайма фрейма
/// шеллом пока не дренируется, поэтому хук доставки её не наполняет.
#[test]
fn frame_facade_focus_and_blur_update_active_element_without_shell_request() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    // tabindex делает div#main фокусируемым (HTML LS §6.6.1).
    rt.eval("document.getElementById('main').setAttribute('tabindex', '0');")
        .unwrap();
    rt.eval(
        "_lumen_frame_content_document(1).getElementById('main').focus({ preventScroll: true })",
    )
    .unwrap();
    // До пумпы — только постановка конверта, состояние не тронуто.
    assert_eq!(
        rt.eval("document.activeElement === document.body").unwrap(),
        JsValue::Bool(true)
    );
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(matches!(
        rt.eval(
            "document.activeElement !== null \
                 && document.activeElement.id === 'main'"
        )
        .unwrap(),
        JsValue::Bool(true)
    ));
    // Отклонение среза: уведомления шеллу нет — очередь пуста.
    assert!(rt.take_focus_requests().is_empty());
    // blur через фасад возвращает фокус назад.
    rt.eval("_lumen_frame_content_document(1).getElementById('main').blur()")
        .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        rt.eval("document.activeElement === document.body").unwrap(),
        JsValue::Bool(true)
    );
    assert!(rt.take_focus_requests().is_empty());
}

/// BUG-480 срез 7: dispatchEvent через фасад доставляет событие слушателям
/// ЭТОГО изолята с сохранённым detail (JSON-круготрип) — та же механика,
/// что у собственного el.dispatchEvent.
#[test]
fn frame_facade_dispatch_event_runs_local_listeners_with_detail() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.eval(
        "window.__got = null; \
             document.getElementById('main').addEventListener('hello', function(e) { \
                 window.__got = { type: e.type, bubbles: e.bubbles, detail: e.detail }; \
             });",
    )
    .unwrap();
    rt.eval(
        "_lumen_frame_content_document(1).getElementById('main') \
             .dispatchEvent({ type: 'hello', bubbles: true, cancelable: false, \
                              detail: { n: 42 } });",
    )
    .unwrap();
    assert_eq!(
        rt.eval("window.__got === null").unwrap(),
        JsValue::Bool(true),
        "до пумпы доставки нет"
    );
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(matches!(
        rt.eval(
            "window.__got !== null && window.__got.type === 'hello' \
                 && window.__got.bubbles === true \
                 && window.__got.detail.n === 42"
        )
        .unwrap(),
        JsValue::Bool(true)
    ));
}

/// BUG-480 срез 8: `<script>`, вставленный в под-документ через фасад
/// (createElement + textContent + appendChild), исполняется на тике пумпы
/// штатной `_lumen_script_prepare` — с честным document.currentScript.
#[test]
fn frame_facade_inserted_script_executes_on_pump_with_current_script() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.eval(
        "var d = _lumen_frame_content_document(1); \
             var s = d.createElement('script'); \
             s.setAttribute('id', 'probe'); \
             s.textContent = 'window.__ran = true; \
                              window.__cs = document.currentScript \
                                && document.currentScript.id;'; \
             d.body.appendChild(s);",
    )
    .unwrap();
    // До пумпы скрипт не исполнялся — доставка через границу асинхронная.
    assert_eq!(
        rt.eval("window.__ran === undefined").unwrap(),
        JsValue::Bool(true)
    );
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(matches!(
        rt.eval("window.__ran === true && window.__cs === 'probe'")
            .unwrap(),
        JsValue::Bool(true)
    ));
}

/// Срез 8: «already started» — per element. Повторная вставка исполненного
/// скрипта не перезапускает его; data-блок (не-JS type) не исполняется
/// вовсе.
#[test]
fn frame_inserted_script_runs_once_and_data_blocks_never_run() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.eval(
        "window.__count = 0; \
             var d = _lumen_frame_content_document(1); \
             var s = d.createElement('script'); \
             s.textContent = 'window.__count++;'; \
             d.body.appendChild(s); \
             var j = d.createElement('script'); \
             j.setAttribute('type', 'application/json'); \
             j.textContent = 'window.__count += 10;'; \
             d.body.appendChild(j);",
    )
    .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        rt.eval("window.__count").unwrap(),
        JsValue::Number(1.0),
        "классика исполнена один раз, JSON-блок — нет"
    );
    // remove + повторный append того же элемента: новый конверт доставлен,
    // но исполнение не повторяется («already started»).
    rt.eval(
        "var d = _lumen_frame_content_document(1); \
             d.body.removeChild(s); \
             d.body.appendChild(s);",
    )
    .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        rt.eval("window.__count").unwrap(),
        JsValue::Number(1.0),
        "повторная вставка не перезапускает исполненный скрипт"
    );
}

/// Срез 8: конверт, чей скрипт успели отсоединить до доставки, теряется
/// без пометки «started» — повторная вставка исполняется, как у главного
/// документа, где preparation ждёт первой connected-вставки.
#[test]
fn detached_before_delivery_script_runs_on_reinsertion() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.eval(
        "var d = _lumen_frame_content_document(1); \
             var s = d.createElement('script'); \
             s.textContent = 'window.__ran = true;'; \
             d.body.appendChild(s); \
             d.body.removeChild(s);",
    )
    .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        rt.eval("window.__ran === undefined").unwrap(),
        JsValue::Bool(true),
        "отсоединённый до доставки конверт не исполняется"
    );
    rt.eval(
        "var d = _lumen_frame_content_document(1); \
             d.body.appendChild(s);",
    )
    .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(matches!(
        rt.eval("window.__ran === true").unwrap(),
        JsValue::Bool(true)
    ));
}

/// Срез 9: скрипт, вставленный ПУСТЫМ, не помечается «already started»
/// первой доставкой (по спеке флаг ставится только когда подготовка
/// началась), поэтому поздний `src = …` получает вторую доставку и
/// запускает штатную подготовку: fetch силами провайдеров контекста
/// (здесь их нет → спековый error на элементе).
#[test]
fn frame_facade_late_src_starts_preparation_after_silent_first_delivery() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.eval(
        "window.__err = false; \
             var d = _lumen_frame_content_document(1); \
             var s = d.createElement('script'); \
             d.body.appendChild(s); \
             window.__snid = s.__nid__; \
             _lumen_make_element(s.__nid__).addEventListener('error', function () { \
                 window.__err = true; \
             });",
    )
    .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        matches!(
            rt.eval(
                "_lumen_frame_scripts_started[__snid] === undefined && window.__err === false"
            )
            .unwrap(),
            JsValue::Bool(true)
        ),
        "пустой скрипт первой доставкой не начинается и не помечается"
    );
    // Каноничный порядок «appendChild, потом src»: сеттер рефлексии пишет
    // атрибут через натив записи, тот ставит второй конверт RunScript.
    rt.eval("s.src = 'late.js';").unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        matches!(
            rt.eval("_lumen_frame_scripts_started[__snid] === 1 && window.__err === false")
                .unwrap(),
            JsValue::Bool(true)
        ),
        "вторая доставка помечает элемент и запускает загрузку"
    );
    // Провайдера сети нет: fetch отклоняется, спековый error доезжает
    // асинхронно (task hop таймера внутри _lumen_script_load_external).
    rt.eval("_lumen_tick_timers();").unwrap();
    assert_eq!(
        rt.eval("window.__err").unwrap(),
        JsValue::Bool(true),
        "неудавшаяся загрузка отстрелила error на элементе"
    );
    // Уже начавшийся скрипт повторных доставок не начинает заново.
    rt.eval("s.src = 'other.js';").unwrap();
    rt.eval("_lumen_frame_pump_messages(); _lumen_tick_timers();").unwrap();
    assert_eq!(
        rt.eval("window.__err").unwrap(),
        JsValue::Bool(true),
        "повторный src после already started — no-op"
    );
}

/// Срез 9: дата-блок (не-JS type) не помечается «already started» вовсе —
/// он никогда не становится скриптом, каким бы атрибутам его ни учили.
#[test]
fn frame_data_block_stays_unmarked_after_delivery() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    rt.eval(
        "var d = _lumen_frame_content_document(1); \
             var j = d.createElement('script'); \
             j.setAttribute('type', 'application/json'); \
             j.textContent = '{\"x\":1}'; \
             d.body.appendChild(j); \
             window.__jnid = j.__nid__;",
    )
    .unwrap();
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        matches!(
            rt.eval("_lumen_frame_scripts_started[__jnid] === undefined")
                .unwrap(),
            JsValue::Bool(true)
        ),
        "дата-блок не начинается ни при какой доставке"
    );
}

// ── BUG-480 срез 10: обратная доставка ресурсных событий ────────────────

/// Пара изолятов «родитель ↔ ребёнок» на общих документах: у родителя
/// биндинг на документ ребёнка (`host_nid`), у ребёнка — слот родителя с
/// документом родителя. Точная топология прод-регистраций shell'а.
fn parent_child_pair(
) -> (V8JsRuntime, V8JsRuntime, Arc<Mutex<lumen_dom::Document>>, Arc<Mutex<lumen_dom::Document>>)
{
    let parent_doc = make_doc();
    let child_doc = make_doc();
    let parent =
        runtime_with_dom(Arc::clone(&parent_doc), "https://parent.example/index.html");
    let child = runtime_with_dom(Arc::clone(&child_doc), "about:srcdoc");
    parent.register_frame_document(
        1,
        Arc::clone(&child_doc),
        "about:srcdoc".to_owned(),
        None,
        true,
    );
    child.register_parent_document(
        1,
        Arc::clone(&parent_doc),
        "https://parent.example/index.html".to_owned(),
        true,
    );
    (parent, child, parent_doc, child_doc)
}

/// Срез 10: ресурсное событие (`load`) из изолята ребёнка доезжает до
/// обработчиков фасада родителя — сначала слушатель `addEventListener`,
/// затем свойство `on<type>`; `target`/`currentTarget` — интернированный
/// фасад, событие недоверенное по построению конструктора, но помечено
/// доверенным (движковое), bubbles/cancelable выключены, как у локального
/// `_lumen_resource_fire`.
#[test]
fn frame_resource_event_reaches_facade_handlers_in_parent() {
    let (parent, child, parent_doc, _child_doc) = parent_child_pair();
    parent
        .eval(
            "window.__order = []; \
                 var d = _lumen_frame_content_document(1); \
                 var s = d.getElementById('main'); \
                 window.__s = s; \
                 s.addEventListener('load', function () { window.__order.push('l1'); }); \
                 s.addEventListener('load', function () { \
                     window.__order.push('l2'); \
                 });",
        )
        .unwrap();
    child
        .eval(
            "_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'load')",
        )
        .unwrap();
    // Конверт держит спящий цикл РОДИТЕЛЯ живым (тот же предикат среза 8).
    assert!(crate::frame_bridge::frame_transport_has_for(Some(
        Arc::as_ptr(&parent_doc) as usize
    )));
    assert_eq!(
        parent.eval("window.__order.length").unwrap(),
        JsValue::Number(0.0),
        "до пумпы родителя доставок нет"
    );
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        matches!(
            parent
                .eval("window.__order.join(',') === 'l1,l2'")
                .unwrap(),
            JsValue::Bool(true)
        ),
        "оба слушателя вызываются в порядке регистрации"
    );
    // Теперь назначается свойство on<type>: тот же фасад, второй конверт.
    parent
        .eval(
            "window.__s.onerror = function (ev) { \
                     window.__order.push('prop:' + ev.type + ':' + (ev.target === __s) + ':' + \
                         (ev.currentTarget === ev.target) + ':' + ev.bubbles + ':' + ev.isTrusted); \
                 };",
        )
        .unwrap();
    child
        .eval(
            "_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'error')",
        )
        .unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        matches!(
            parent
                .eval("window.__order.join(',').split(',')[2] !== undefined && window.__order[2].indexOf('prop:error:') === 0")
                .unwrap(),
            JsValue::Bool(true)
        ),
        "свойство on<type> вызвано после слушателей"
    );
    assert!(
        matches!(
            parent
                .eval("window.__order[2] === 'prop:error:true:true:false:true'")
                .unwrap(),
            JsValue::Bool(true)
        ),
        "target/currentTarget — интернированный фасад, isTrusted у движкового события"
    );
}

/// Срез 10: `removeEventListener` снимает слушателя фасада; повторная
/// доставка вызывает только оставшихся.
#[test]
fn facade_remove_listener_stops_delivery() {
    let (parent, child, _parent_doc, _child_doc) = parent_child_pair();
    parent
        .eval(
            "window.__n = 0; \
                 var d = _lumen_frame_content_document(1); \
                 var s = d.getElementById('main'); \
                 var fn = function () { window.__n++; }; \
                 s.addEventListener('load', fn); \
                 window.__fn = fn;",
        )
        .unwrap();
    child
        .eval("_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'load')")
        .unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(parent.eval("window.__n").unwrap(), JsValue::Number(1.0));
    parent
        .eval(
            "var s2 = _lumen_frame_content_document(1).getElementById('main'); \
                 s2.removeEventListener('load', window.__fn);",
        )
        .unwrap();
    child
        .eval("_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'load')")
        .unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        parent.eval("window.__n").unwrap(),
        JsValue::Number(1.0),
        "после removeEventListener доставок нет"
    );
}

/// Срез 10: гейты постановки зеркала — контекст без слота родителя
/// (топ-страница), не-элемент (текстовый узел, nid за ареной) и минимальный
/// изолят без натива ничего не ставят.
#[test]
fn mirror_gates_top_level_non_element_and_missing_native() {
    let doc = make_doc();
    let rt = runtime_with_dom(doc, "https://parent.example/index.html");
    // Топ-страница: слота родителя нет.
    assert_eq!(
        rt.eval(
            "_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'load')"
        )
        .unwrap(),
        JsValue::Bool(false),
        "топ-страница не зеркалит"
    );
    rt.register_parent_document(
        1,
        make_doc(),
        "https://parent.example/index.html".to_owned(),
        true,
    );
    // Текстовый узел — не элемент.
    rt.eval("window.__tnid = document.createTextNode('x').__nid__;").unwrap();
    assert_eq!(
        rt.eval("_lumen_frame_mirror_resource(__tnid, 'load')").unwrap(),
        JsValue::Bool(false),
        "текстовый узел не зеркалится"
    );
    // nid за границей арены.
    assert_eq!(
        rt.eval("_lumen_frame_mirror_resource(9999999, 'load')").unwrap(),
        JsValue::Bool(false),
        "чужой nid отброшен"
    );
    // Минимальный изолят без натива: зеркало молча отсутствует.
    let bare = V8JsRuntime::new().unwrap();
    bare.eval("var window = globalThis;").unwrap();
    crate::frame_bridge::install_frame_bridge_v8(&bare, Default::default()).unwrap();
    assert_eq!(
        bare.eval("typeof _lumen_frame_mirror_resource").unwrap(),
        JsValue::String("function".into()),
        "зеркало установлено вместе с бриджем"
    );
    assert_eq!(
        bare.eval("_lumen_frame_mirror_resource(3, 'load')").unwrap(),
        JsValue::Bool(false),
        "без self_doc/слота родителя постановки нет"
    );
}

/// Срез 10: фильтр доступности при разборе ящика — биндинг отправителя,
/// которого у получателя нет или который `accessible: false`, отбрасывает
/// конверт БЕЗ доставки (у cross-origin детей фасадов элементов нет).
#[test]
fn resource_envelope_dropped_without_accessible_sender_binding() {
    let parent_doc = make_doc();
    let child_doc = make_doc();
    let parent =
        runtime_with_dom(Arc::clone(&parent_doc), "https://parent.example/index.html");
    let child = runtime_with_dom(Arc::clone(&child_doc), "about:srcdoc");
    // У родителя НЕТ биндинга на этого ребёнка.
    child.register_parent_document(
        1,
        Arc::clone(&parent_doc),
        "https://parent.example/index.html".to_owned(),
        true,
    );
    parent
        .eval("window.__got = false;")
        .unwrap();
    child
        .eval("_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'load')")
        .unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        parent.eval("window.__got").unwrap(),
        JsValue::Bool(false),
        "неизвестный отправитель — тихая потеря конверта"
    );
    // Ящик разобран (конверт снят и отброшен), второй пумпа чист.
    assert!(!crate::frame_bridge::frame_transport_has_for(Some(
        Arc::as_ptr(&parent_doc) as usize
    )));

    // Теперь биндинг есть, но cross-origin/opaque (accessible=false).
    parent.register_frame_document(
        1,
        Arc::clone(&child_doc),
        "about:srcdoc".to_owned(),
        None,
        false,
    );
    child
        .eval("_lumen_frame_mirror_resource(document.getElementById('main').__nid__, 'load')")
        .unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert_eq!(
        parent.eval("window.__got").unwrap(),
        JsValue::Bool(false),
        "cross-origin отправитель не доставляется"
    );
}

/// Срез 10 — заголовочный сценарий: внешний `<script src>` ребёнка,
/// вставленный родителем через фасад, при неудавшейся загрузке отстреливает
/// спековый `error` В РЕБЁНКЕ, зеркало возвращает его родителю, и
/// назначенный на фасад `onerror` наконец вызывается (хвост среза 9:
/// раньше обработчики фасада были no-op).
#[test]
fn external_script_failure_mirrors_error_to_facade_handler() {
    let (parent, child, _parent_doc, _child_doc) = parent_child_pair();
    parent
        .eval(
            "var d = _lumen_frame_content_document(1); \
                 var s = d.createElement('script'); \
                 s.setAttribute('id', 'probe'); \
                 window.__s = s; \
                 window.__err = null; \
                 s.onerror = function (ev) { window.__err = ev.type; }; \
                 s.src = 'missing.js'; \
                 d.body.appendChild(s);",
        )
        .unwrap();
    // Ребёнок разбирает конверты RunScript (подготовка ставит task hop),
    // затем таймер запускает fetch — провайдера нет, цепочка отклоняется.
    child.eval("_lumen_frame_pump_messages(); _lumen_tick_timers();").unwrap();
    // Родитель забирает ресурсное событие error.
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        matches!(
            parent
                .eval("window.__err === 'error' && window.__s.onerror !== null")
                .unwrap(),
            JsValue::Bool(true)
        ),
        "onerror фасада получил зеркальный error"
    );
}


/// Срез 8: предикат «есть конверт для меня» горит между постановкой и
/// пумпой и гаснет после разбора ящика — по нему шелл будит спящий цикл.
#[test]
fn frame_transport_pending_flips_around_pump() {
    let doc = make_doc();
    let rt = runtime_with_dom(Arc::clone(&doc), "https://parent.example/index.html");
    rt.register_frame_document(1, Arc::clone(&doc), "about:srcdoc".to_owned(), None, true);
    assert!(!rt.frame_transport_pending(), "ящик пуст до постановки");
    rt.eval("_lumen_frame_content_window(1).postMessage('wake', '*')")
        .unwrap();
    assert!(
        rt.frame_transport_pending(),
        "самоадресованный конверт делает контекст «ожидающим»"
    );
    rt.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(!rt.frame_transport_pending(), "ящик разобран пумпой");
}

/// Serializes the two `user_agent_override_*` tests below against each
/// other — both read/write the process-global `GLOBAL_UA_OVERRIDE`
/// (BUG-295), which every `install_dom` call in the crate consults, so an
/// unsynchronized run risks one test's `Some(..)` leaking into the
/// other's `install_dom` under cargo's default parallel test execution.
static UA_OVERRIDE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// BUG-295 (`emulation.setUserAgentOverride`): a `set_global_user_agent_
/// override` call before `install_dom` must make `navigator.userAgent`
/// reflect it — exercises the exact mechanism the shell's live-window
/// `AutomationCommand::SetUserAgent` handler relies on for the *next
/// navigation* half of the fix, without needing a live winit window.
#[test]
fn user_agent_override_applies_at_install_dom() {
    let _guard = UA_OVERRIDE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_global_user_agent_override(Some("LumenBug295TestUA/1.0".to_owned()));
    let rt = runtime_with_dom(make_doc(), "");
    let ua = rt.eval("navigator.userAgent").unwrap();
    set_global_user_agent_override(None);
    assert_eq!(ua, JsValue::String("LumenBug295TestUA/1.0".into()));
}

/// Without an override, `navigator.userAgent` keeps the WEB_API_SHIM
/// default — proves the override is opt-in, not always-on.
#[test]
fn user_agent_override_is_noop_when_unset() {
    let _guard = UA_OVERRIDE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_global_user_agent_override(None);
    let rt = runtime_with_dom(make_doc(), "");
    let ua = rt.eval("navigator.userAgent").unwrap();
    assert_ne!(ua, JsValue::String("LumenBug295TestUA/1.0".into()));
}

/// Serializes the `timezone_override_*` tests below against each other —
/// same rationale as [`UA_OVERRIDE_TEST_LOCK`] (shared process-global,
/// `GLOBAL_TIMEZONE_OVERRIDE`).
static TIMEZONE_OVERRIDE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// BUG-295 (`browser.setTimezoneOverride`): a `set_global_timezone_
/// override` call before `install_dom` must make
/// `Intl.DateTimeFormat().resolvedOptions().timeZone` reflect it —
/// exercises the exact mechanism the shell's live-window
/// `AutomationCommand::SetTimezone` handler relies on for the *next
/// navigation* half of the fix, without needing a live winit window.
#[test]
fn timezone_override_applies_at_install_dom() {
    let _guard = TIMEZONE_OVERRIDE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_global_timezone_override(Some("Pacific/Kiritimati".to_owned()));
    let rt = runtime_with_dom(make_doc(), "");
    let tz = rt.eval("Intl.DateTimeFormat().resolvedOptions().timeZone").unwrap();
    set_global_timezone_override(None);
    assert_eq!(tz, JsValue::String("Pacific/Kiritimati".into()));
}

/// Without an override, `resolvedOptions().timeZone` keeps whatever the
/// unwrapped `Intl.DateTimeFormat` would have reported (host timezone on
/// a native-ICU build, `'UTC'` on the pure-JS shim fallback) — proves the
/// override is opt-in, not always-on. Doesn't assert the exact value
/// (host-dependent under native ICU), only that our override string
/// isn't leaking in from a previous test run.
#[test]
fn timezone_override_is_noop_when_unset() {
    let _guard = TIMEZONE_OVERRIDE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_global_timezone_override(None);
    let rt = runtime_with_dom(make_doc(), "");
    let tz = rt.eval("Intl.DateTimeFormat().resolvedOptions().timeZone").unwrap();
    assert_ne!(tz, JsValue::String("Pacific/Kiritimati".into()));
}

/// An explicit `options.timeZone` must win over the BiDi override (spec
/// behaviour — a caller who names a zone explicitly should get exactly
/// that zone back, not a session-wide emulation override).
#[test]
fn timezone_override_does_not_win_over_explicit_option() {
    let _guard = TIMEZONE_OVERRIDE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_global_timezone_override(Some("Pacific/Kiritimati".to_owned()));
    let rt = runtime_with_dom(make_doc(), "");
    let tz = rt
        .eval("Intl.DateTimeFormat('en-US', {timeZone: 'UTC'}).resolvedOptions().timeZone")
        .unwrap();
    set_global_timezone_override(None);
    assert_eq!(tz, JsValue::String("UTC".into()));
}

#[test]
fn query_selector_finds_element_by_id() {
    let rt = runtime_with_dom(make_doc(), "");
    let ok = rt
        .eval("document.querySelector('#main').tagName === 'DIV'")
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

// ── BUG-341 S7: `take_dom_touched` (page-side DOM-mutation tracker) ────────

#[test]
fn take_dom_touched_is_empty_before_any_mutation() {
    let rt = runtime_with_dom(make_doc(), "");
    let t = rt.take_dom_touched();
    assert!(t.nodes.is_empty());
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_set_attribute() {
    let doc = make_doc();
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').setAttribute('data-x', '1')")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&main));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_ignores_a_no_op_set_attribute() {
    let doc = make_doc();
    let rt = runtime_with_dom(doc, "");
    // 'id' is already "main" — writing the same value is a no-op change.
    rt.eval("document.getElementById('main').setAttribute('id', 'main')")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.is_empty());
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_class_list_add() {
    let doc = make_doc();
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').classList.add('active')")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&main));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_remove_attribute() {
    let doc = make_doc();
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').setAttribute('data-x', '1')")
        .unwrap();
    rt.take_dom_touched(); // drain the setAttribute above
    rt.eval("document.getElementById('main').removeAttribute('data-x')")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&main));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_ignores_a_no_op_remove_attribute() {
    let doc = make_doc();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').removeAttribute('data-never-set')")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.is_empty());
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_append_child_on_the_parent() {
    let doc = make_doc();
    let body = doc.lock().unwrap().find_by_id("main").unwrap();
    let body = doc.lock().unwrap().get(body).parent.unwrap(); // <body>
    let rt = runtime_with_dom(doc, "");
    rt.eval(
        "document.body.appendChild(document.createElement('p'))",
    )
    .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&body));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_remove_child_on_the_parent() {
    let doc = make_doc();
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').removeChild(document.getElementById('main').firstChild)")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&main));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_text_content_change() {
    let doc = make_doc();
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').textContent = 'replaced'")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&main));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_reports_inline_style_change() {
    let doc = make_doc();
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').style.color = 'red'")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.nodes.contains(&main));
    assert!(!t.unattributed);
}

#[test]
fn take_dom_touched_marks_exec_command_unattributed() {
    let doc = make_doc();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.execCommand('selectAll', false, null)")
        .unwrap();
    let t = rt.take_dom_touched();
    assert!(t.unattributed);
}

#[test]
fn take_dom_touched_clears_between_calls() {
    let doc = make_doc();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('main').setAttribute('data-x', '1')")
        .unwrap();
    let first = rt.take_dom_touched();
    assert!(!first.nodes.is_empty());
    let second = rt.take_dom_touched();
    assert!(second.nodes.is_empty());
    assert!(!second.unattributed);
}

/// BUG-341 S7 part 2: end-to-end differential test for the page-pipeline
/// wiring (`Lumen::try_relayout_raf_incremental`) — `take_dom_touched()`'s
/// node set, fed through `restyle_root_set_for_node_change` into a
/// `RestyleDelta`, must be a *sufficient* dirty-root-set for
/// `layout_mutation_incremental_restyle` to reproduce a full
/// `layout_measured_hyp_with_counters` recompute exactly — driven by a real
/// V8 `classList.add` mutation, not a synthetic dirty-root set like the
/// `lumen-layout`-crate differential tests use. Any divergence here would
/// mean the shell's actual wiring (not just the underlying primitives) is
/// unsound.
#[test]
fn dom_touched_drives_incremental_restyle_matching_full_cascade() {
    use lumen_core::ext::NullHyphenationProvider;
    use lumen_core::geom::{Rect, Size};
    use lumen_layout::box_tree::{layout_measured_hyp_with_counters, layout_mutation_incremental_restyle, LayoutBox};
    use lumen_layout::counters::{set_incremental_restyle, RestyleDelta};
    use lumen_layout::style::{restyle_node_index, restyle_root_set_for_node_change, NodeChange};

    struct FixedMeasurer;
    impl lumen_layout::TextMeasurer for FixedMeasurer {
        fn char_width(&self, _: char, size: f32) -> f32 {
            size * 0.5
        }
    }

    fn collect_rects(b: &LayoutBox, out: &mut Vec<(lumen_dom::NodeId, Rect)>) {
        out.push((b.node, b.rect));
        for c in &b.children {
            collect_rects(c, out);
        }
    }

    let doc = make_doc();
    let sheet = lumen_css_parser::parse("#main { color: black; } #main.active { padding: 20px; color: red; }");
    let vp = Size::new(800.0, 600.0);
    let hp = NullHyphenationProvider;

    // Baseline: full cascade over the pre-mutation document.
    let (prev, baseline_counters) = {
        let d = doc.lock().unwrap();
        layout_measured_hyp_with_counters(&d, &sheet, vp, &FixedMeasurer, &hp, false)
    };

    let rt = runtime_with_dom(Arc::clone(&doc), "");
    rt.eval("document.getElementById('main').classList.add('active')").unwrap();
    let touched = rt.take_dom_touched();
    assert!(!touched.unattributed, "classList.add must be attributed");
    assert!(!touched.nodes.is_empty(), "classList.add must report the touched node");

    let dirty_roots = {
        let d = doc.lock().unwrap();
        let node_index = restyle_node_index(&d, &sheet);
        restyle_root_set_for_node_change(
            &d,
            touched.nodes.iter().map(|&n| (n, NodeChange::Unattributed)),
            &node_index,
        )
    };
    let delta = RestyleDelta { prev_styles: baseline_counters.styles().clone(), dirty_roots, content_dirty: lumen_layout::counters::ContentDirty::Untracked };

    // BUG-341 S19: the incremental pass consumes `prev` (it moves the
    // reusable subtrees into the tree it returns), and the geometry
    // sanity check at the end of this test still needs the old tree.
    let prev_rects_source = prev.clone();
    set_incremental_restyle(true);
    let (incr, _incr_counters) = {
        let d = doc.lock().unwrap();
        layout_mutation_incremental_restyle(&d, &sheet, vp, &FixedMeasurer, &hp, false, prev, delta)
    };
    set_incremental_restyle(false);

    let full = {
        let d = doc.lock().unwrap();
        layout_measured_hyp_with_counters(&d, &sheet, vp, &FixedMeasurer, &hp, false).0
    };

    let mut ia = Vec::new();
    let mut fb = Vec::new();
    collect_rects(&incr, &mut ia);
    collect_rects(&full, &mut fb);
    assert_eq!(ia.len(), fb.len(), "box count must match full layout");
    for ((na, ra), (nb, rb)) in ia.iter().zip(fb.iter()) {
        assert_eq!(na, nb, "node order must match");
        assert!(
            (ra.x - rb.x).abs() < 0.5
                && (ra.y - rb.y).abs() < 0.5
                && (ra.width - rb.width).abs() < 0.5
                && (ra.height - rb.height).abs() < 0.5,
            "rect mismatch for {na:?}: incr {ra:?} vs full {rb:?}",
        );
    }

    // Sanity: the fixture must actually move geometry, or a broken/empty
    // delta would pass this test vacuously. `#main` is a block with
    // `width:auto` (stretches to the containing block regardless of
    // padding — content-box shrinks instead, so *width* is unaffected),
    // but `height:auto` grows by `padding-top + padding-bottom`.
    let main = doc.lock().unwrap().find_by_id("main").unwrap();
    let mut prev_rects = Vec::new();
    collect_rects(&prev_rects_source, &mut prev_rects);
    let prev_main = prev_rects.iter().find(|(n, _)| *n == main).unwrap().1;
    let full_main = fb.iter().find(|(n, _)| *n == main).unwrap().1;
    assert!(
        (full_main.height - prev_main.height).abs() > 1.0,
        "fixture must actually change geometry, or this test is vacuous",
    );
}

mod dom_suspend_focus;
