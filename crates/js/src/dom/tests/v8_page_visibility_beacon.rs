//! Тесты `v8_page_visibility_beacon`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

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

// Mock fetch provider that records calls to fetch_with_body_sync.
// V8 twin of [`super::CaptureFetch`].
type FetchCall = (String, String, String, Vec<u8>);
struct CaptureFetch {
    calls: std::sync::Mutex<Vec<FetchCall>>,
}
impl CaptureFetch {
    fn new() -> Arc<Self> {
        Arc::new(Self { calls: std::sync::Mutex::new(vec![]) })
    }
}
impl lumen_core::ext::JsFetchProvider for CaptureFetch {
    fn fetch_sync(&self, url: &str, method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.calls.lock().unwrap().push((url.into(), method.into(), String::new(), vec![]));
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
    fn fetch_with_body_sync(&self, url: &str, method: &str, content_type: &str, body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.calls.lock().unwrap().push((url.into(), method.into(), content_type.into(), body.to_vec()));
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
}

/// V8 twin of [`super::runtime_with_fetch`].
fn v8_runtime_with_fetch(provider: Arc<CaptureFetch>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = provider;
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    rt
}

// ─── Page Visibility API tests ───────────────────────────────────────────

#[test]
fn page_visibility_initial_visible() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.visibilityState === 'visible' && document.hidden === false").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn apply_visibility_hidden() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fired = false; \
                 document.addEventListener('visibilitychange', function() { fired = true; }); \
                 _lumen_apply_visibility(true); \
                 document.visibilityState === 'hidden' && document.hidden === true && fired"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn apply_visibility_noop_when_same() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var count = 0; \
                 document.addEventListener('visibilitychange', function() { count++; }); \
                 _lumen_apply_visibility(false); \
                 count"  // already visible → no event
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

// ─── PH1-15: T1 pause/unpause via set_document_visibility ───────────────

#[test]
fn set_document_visibility_hidden_sets_document_hidden() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.set_document_visibility(true);
    let r = rt.eval("document.hidden").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_document_visibility_visible_clears_document_hidden() {
    let rt = v8_runtime_with_dom(make_doc());
    // Start hidden, then unpause.
    rt.set_document_visibility(true);
    rt.set_document_visibility(false);
    let r = rt.eval("document.visibilityState === 'visible' && document.hidden === false").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_document_visibility_fires_visibilitychange_on_hide() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fired = false; \
                 document.addEventListener('visibilitychange', function() { fired = true; }); \
                 true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    rt.set_document_visibility(true);
    let fired = rt.eval("fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_document_visibility_fires_visibilitychange_on_show() {
    let rt = v8_runtime_with_dom(make_doc());
    // Hide first.
    rt.set_document_visibility(true);
    // Register listener after hide.
    rt.eval(
        "var showFired = false; \
                 document.addEventListener('visibilitychange', function() { showFired = true; });"
    ).unwrap();
    rt.set_document_visibility(false);
    let r = rt.eval("showFired").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn set_document_visibility_noop_on_same_state() {
    let rt = v8_runtime_with_dom(make_doc());
    // Already visible — hiding fires event.
    rt.eval(
        "var count = 0; \
                 document.addEventListener('visibilitychange', function() { count++; });"
    ).unwrap();
    // Calling visible→visible: no event expected.
    rt.set_document_visibility(false); // already visible
    let r = rt.eval("count").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn set_document_visibility_heap_survives_pause_unpause() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("globalThis.__t1_val__ = 99;").unwrap();
    // Simulate T0 → T1 → T0.
    rt.set_document_visibility(true);
    rt.set_document_visibility(false);
    let v = rt.eval("globalThis.__t1_val__").unwrap();
    assert_eq!(v, lumen_core::JsValue::Number(99.0));
}

// ─── document.readyState + lifecycle tests ───────────────────────────────

#[test]
fn ready_state_initial_loading() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.readyState").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("loading".into()));
}

#[test]
fn ready_state_interactive_fires_dcl() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var dcl = false; var rsc = false; \
                 document.addEventListener('readystatechange', function() { rsc = true; }); \
                 document.addEventListener('DOMContentLoaded', function() { dcl = true; }); \
                 _lumen_apply_ready_state('interactive'); \
                 document.readyState === 'interactive' && rsc && dcl"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn ready_state_complete_fires_load() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var loaded = false; \
                 window.addEventListener('load', function() { loaded = true; }); \
                 _lumen_apply_ready_state('interactive'); \
                 _lumen_apply_ready_state('complete'); \
                 document.readyState === 'complete' && loaded"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn ready_state_onload_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var called = false; \
                 window.onload = function() { called = true; }; \
                 _lumen_apply_ready_state('interactive'); \
                 _lumen_apply_ready_state('complete'); \
                 called"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn ready_state_forward_only() {
    let rt = v8_runtime_with_dom(make_doc());
    // Cannot go backward from 'complete' to 'interactive'
    let r = rt.eval(
        "_lumen_apply_ready_state('interactive'); \
                 _lumen_apply_ready_state('complete'); \
                 _lumen_apply_ready_state('interactive'); \
                 document.readyState"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("complete".into()));
}

// document.write()/writeln() — HTML LS 8.4.4, scoped fix (see dom.rs write/
// writeln comment): insert at end of body while still 'loading', no-op once
// 'complete', never the spec's page-erasing implicit document.open().
#[test]
fn document_write_inserts_while_loading() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "document.write('<span id=\"w\">hi</span>'); \
                 document.getElementById('w') !== null && \
                 document.getElementById('w').textContent === 'hi'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_writeln_appends_newline() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "document.writeln('a'); document.writeln('b'); \
                 document.body.innerHTML"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("<div id=\"main\"><span class=\"highlight\">Hello</span></div>a\nb\n".into()));
}

#[test]
fn document_write_is_noop_after_complete() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "_lumen_apply_ready_state('interactive'); \
                 _lumen_apply_ready_state('complete'); \
                 document.write('<span id=\"late\">late</span>'); \
                 document.getElementById('late') === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-571: a `<script>` built through the DOM API and inserted into the
// live document runs when it becomes connected (HTML LS §4.12.1
// "prepare the script element"). Before this it stayed inert forever —
// no exception, no request, nothing.
#[test]
fn dynamic_inline_script_runs_on_append() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElement('script');
                       s.setAttribute('type', 'text/javascript');
                       s.textContent = 'globalThis.__b571_ran = true;';
                       document.body.appendChild(s);
                       globalThis.__b571_ran === true"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The 'already started' flag is per element, not per insertion: moving
/// an executed script back into the tree must not run it again.
#[test]
fn dynamic_script_runs_exactly_once() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b571_n = 0;
                       var s = document.createElement('script');
                       s.textContent = 'globalThis.__b571_n++;';
                       document.body.appendChild(s);
                       s.remove();
                       document.body.appendChild(s);
                       globalThis.__b571_n === 1"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-486 (`document.currentScript`, blocking BUG-703): a running
// classic script must be able to find its own element — self-locating
// bundles read their id/base URL off it (`currentScript.dataset.*`).
/// Inside an inline classic script `currentScript` is that very element.
#[test]
fn current_script_points_at_running_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElement('script');
                       s.setAttribute('data-mmid', 'block-42');
                       s.textContent =
                         'globalThis.__b486_self = document.currentScript;' +
                         'globalThis.__b486_mmid = document.currentScript.dataset.mmid;';
                       document.body.appendChild(s);
                       globalThis.__b486_self === s && globalThis.__b486_mmid === 'block-42'"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Outside script execution it is `null` — and the property exists, so
/// feature detection sees `null`, never `undefined`.
#[test]
fn current_script_is_null_outside_execution() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var before = document.currentScript;
                       var s = document.createElement('script');
                       s.textContent = 'globalThis.__b486_x = 1;';
                       document.body.appendChild(s);
                       before === null && document.currentScript === null &&
                       'currentScript' in document"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Nesting restores the outer script: an inner script inserted and run
/// synchronously must not leave `currentScript` pointing at itself.
#[test]
fn current_script_restores_outer_after_nested_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var outer = document.createElement('script');
                       outer.textContent =
                         'var o = document.currentScript;' +
                         'var inner = document.createElement("script");' +
                         'inner.textContent = "globalThis.__b486_inner = document.currentScript;";' +
                         'document.body.appendChild(inner);' +
                         'globalThis.__b486_outer_before = o;' +
                         'globalThis.__b486_outer_after = document.currentScript;' +
                         'globalThis.__b486_inner_el = inner;';
                       document.body.appendChild(outer);
                       globalThis.__b486_outer_before === outer &&
                       globalThis.__b486_outer_after === outer &&
                       globalThis.__b486_inner === globalThis.__b486_inner_el"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A script that throws must still restore the previous value — a stale
/// `currentScript` would mislead every script that runs after it.
#[test]
fn current_script_cleared_after_throwing_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElement('script');
                       s.textContent = 'throw new Error("boom");';
                       document.body.appendChild(s);
                       document.currentScript === null"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// An absent `type` means classic JavaScript, as do the legacy MIME
/// spellings — the same whitelist the parser path applies.
#[test]
fn dynamic_script_legacy_and_absent_type_run() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b571_c = 0;
                       ['', 'text/javascript1.5', 'TEXT/JScript', 'application/ecmascript']
                         .forEach(function(t, i) {
                             var s = document.createElement('script');
                             if (t !== '') s.type = t;
                             s.textContent = 'globalThis.__b571_c++;';
                             document.body.appendChild(s);
                         });
                       globalThis.__b571_c === 4"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A non-JavaScript `type` is a data block, never code.
#[test]
fn dynamic_script_with_data_type_is_inert() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"['application/json', 'importmap', 'text/template'].forEach(function(t) {
                           var s = document.createElement('script');
                           s.type = t;
                           s.textContent = 'globalThis.__b571_data = true;';
                           document.body.appendChild(s);
                       });
                       globalThis.__b571_data === undefined"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Insertion into a detached subtree must not run the script; it runs
/// later, when an ancestor carries it into the document.
#[test]
fn dynamic_script_waits_until_ancestor_is_connected() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var d = document.createElement('div');
                       var s = document.createElement('script');
                       s.textContent = 'globalThis.__b571_deep = true;';
                       d.appendChild(s);
                       var beforeInsert = globalThis.__b571_deep === undefined;
                       document.body.appendChild(d);
                       beforeInsert && globalThis.__b571_deep === true"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `insertBefore` is a second insertion path and goes through the same
/// hook — the wrapper sits on the native, not on one DOM method.
#[test]
fn dynamic_script_runs_via_insert_before() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElement('script');
                       s.textContent = 'globalThis.__b571_ib = true;';
                       document.body.insertBefore(s, document.body.firstChild);
                       globalThis.__b571_ib === true"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// SVG's `<script>` (createElementNS) is prepared exactly like the HTML one.
#[test]
fn dynamic_svg_script_runs() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElementNS('http://www.w3.org/2000/svg', 'script');
                       s.textContent = 'globalThis.__b571_svg = true;';
                       document.body.appendChild(s);
                       globalThis.__b571_svg === true"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The fragment parser sets the 'already started' flag, so markup
/// injected through innerHTML stays inert — the fix must not change that.
#[test]
fn innerhtml_script_stays_inert() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"document.body.innerHTML =
                           '<script>globalThis.__b571_ih = true;</scr' + 'ipt>';
                       globalThis.__b571_ih === undefined"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// An external `<script src>` inserted by script is force-async: the
/// insertion returns immediately and nothing is fetched inside it.
#[test]
fn dynamic_external_script_does_not_execute_synchronously() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElement('script');
                       s.src = 'missing.js';
                       s.textContent = 'globalThis.__b571_ext = true;';
                       document.body.appendChild(s);
                       globalThis.__b571_ext === undefined"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// An inline `<script type=module>` is deferred one task, then routed
/// through the ES module map (`_lumen_esm_register_inline` + `import()`),
/// and fires `load` when it has evaluated.
#[test]
fn dynamic_inline_module_script_runs_and_fires_load() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b571_mod_load = false;
                       var s = document.createElement('script');
                       s.type = 'module';
                       s.onload = function() { globalThis.__b571_mod_load = true; };
                       s.textContent = 'globalThis.__b571_mod = true;';
                       document.body.appendChild(s);
                       var deferred = globalThis.__b571_mod === undefined;
                       _lumen_tick_timers();
                       deferred && globalThis.__b571_mod === true"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let loaded = rt.eval("globalThis.__b571_mod_load === true").unwrap();
    assert_eq!(loaded, lumen_core::JsValue::Bool(true));
}

/// A throwing inline script is reported, not propagated into the DOM
/// call that inserted it.
#[test]
fn dynamic_script_error_does_not_escape_append() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var s = document.createElement('script');
                       s.textContent = 'throw new Error(3);';
                       document.body.appendChild(s);
                       true"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-838: `src=""` is not «no src». It obtains no resource, so the
/// element must still report `error` — and as a task, not inline: the
/// handler is assigned before the `appendChild` that triggers it, but
/// WPT `fetch-src/empty.html` asserts the event is asynchronous.
#[test]
fn bug838_empty_src_script_fires_error_as_a_task() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b838 = [];
                       var s = document.createElement('script');
                       s.onerror = function(ev) { globalThis.__b838.push(ev); };
                       s.setAttribute('src', '');
                       document.body.appendChild(s);
                       var sync = globalThis.__b838.length === 0;
                       _lumen_tick_timers();
                       var ev = globalThis.__b838[0];
                       sync && globalThis.__b838.length === 1 && ev.type === 'error'
                           && ev.bubbles === false && ev.cancelable === false
                           && ev.isTrusted === true && ev.target === s"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The `error` is queued once, not once per entry point: the element an
/// `appendChild` already reported must be skipped by the parser scan
/// that runs when `readyState` reaches `interactive`.
#[test]
fn bug838_empty_src_script_reports_once_across_both_paths() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b838n = 0;
                       var s = document.createElement('script');
                       s.onerror = function() { globalThis.__b838n++; };
                       s.setAttribute('src', '');
                       document.body.appendChild(s);
                       _lumen_apply_ready_state('interactive');
                       _lumen_tick_timers();
                       _lumen_tick_timers();
                       globalThis.__b838n === 1"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `<script src="">` the *parser* wrote never passes through the
/// insertion hook, so it is reported by the scan that runs when
/// `readyState` reaches `interactive`. The element is built in the
/// arena rather than through `innerHTML`, which drops a `<script>`
/// outright (`innerhtml_script_stays_inert` above only asserts it does
/// not run — it does not survive either).
#[test]
fn bug838_empty_src_script_from_markup_fires_error() {
    let doc = make_doc();
    {
        #[allow(clippy::unwrap_used)]
        let mut d = doc.lock().unwrap();
        let script = d.create_element(QualName::html("script"));
        if let NodeData::Element { attrs, .. } = &mut d.get_mut(script).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("src"),
                value: "".into(),
            });
        }
        let root = d.root();
        d.append_child(root, script);
    }
    let rt = v8_runtime_with_dom(doc);
    let r = rt
        .eval(
            r#"globalThis.__b838m = 0;
                       var s = document.getElementsByTagName('script')[0];
                       s.addEventListener('error', function() { globalThis.__b838m++; });
                       var before = globalThis.__b838m === 0;
                       _lumen_apply_ready_state('interactive');
                       _lumen_tick_timers();
                       before && globalThis.__b838m === 1"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The control the fix turns on: an element with *no* `src` attribute
/// and an empty body is still silent — that branch is «no script to
/// run», not «a resource that failed».
#[test]
fn bug838_absent_src_and_empty_body_stays_silent() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b838q = 0;
                       var s = document.createElement('script');
                       s.onerror = function() { globalThis.__b838q++; };
                       document.body.appendChild(s);
                       _lumen_apply_ready_state('interactive');
                       _lumen_tick_timers();
                       globalThis.__b838q === 0"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-804: `<style>` reported nothing on any path — it was not in
/// `_lumen_resource_track`'s tag list at all. A block a script builds
/// now fires `load` once it is connected, and as a task: the
/// `st.onload = …` assignment normally follows the `appendChild`, and
/// `style_load_async.html` asserts the handler is not run inline.
#[test]
fn bug804_created_style_fires_load_as_a_task() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b804 = [];
                       var st = document.createElement('style');
                       st.textContent = '#a { color: red; }';
                       st.onload = function(ev) { globalThis.__b804.push(ev); };
                       document.head.appendChild(st);
                       var sync = globalThis.__b804.length === 0;
                       _lumen_tick_timers();
                       var ev = globalThis.__b804[0];
                       sync && globalThis.__b804.length === 1 && ev.type === 'load'
                           && ev.isTrusted === true && ev.target === st"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// HTML LS §4.14 re-runs «update a style block» on every child-list
/// change, so a later `textContent` write owes a SECOND `load` —
/// exactly what `style_load_event.html` counts.
#[test]
fn bug804_style_text_mutation_fires_load_again() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b804n = 0;
                       var st = document.createElement('style');
                       st.onload = function() { globalThis.__b804n++; };
                       document.head.appendChild(st);
                       st.textContent = '.box { color: red; }';
                       _lumen_tick_timers();
                       var first = globalThis.__b804n;
                       st.textContent = '.box { color: green; }';
                       _lumen_tick_timers();
                       first === 2 && globalThis.__b804n === 3"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `<style>` the parser wrote never passes through the insertion
/// hook, so the `interactive` scan reports it — and reports it
/// *directly*, not through a task: the scan already runs at the latest
/// defensible moment, and a further hop puts the event after
/// `window.onload`, where `style_load_event.html` reads its counter.
#[test]
fn bug804_parser_style_fires_load_without_a_timer_pump() {
    let doc = make_doc();
    {
        #[allow(clippy::unwrap_used)]
        let mut d = doc.lock().unwrap();
        let style = d.create_element(QualName::html("style"));
        let text = d.create_text("#a { color: red; }");
        d.append_child(style, text);
        let root = d.root();
        d.append_child(root, style);
    }
    let rt = v8_runtime_with_dom(doc);
    let r = rt
        .eval(
            r#"globalThis.__b804p = 0;
                       var st = document.getElementsByTagName('style')[0];
                       st.addEventListener('load', function() { globalThis.__b804p++; });
                       var before = globalThis.__b804p === 0;
                       _lumen_apply_ready_state('interactive');
                       before && globalThis.__b804p === 1"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The `interactive` scan must skip a `<style>` the insertion hook has
/// already updated, or a head script's element reports its first
/// update twice.
#[test]
fn bug804_style_reports_once_across_both_paths() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b804d = 0;
                       var st = document.createElement('style');
                       st.onload = function() { globalThis.__b804d++; };
                       st.textContent = '#a { color: red; }';
                       document.head.appendChild(st);
                       _lumen_apply_ready_state('interactive');
                       _lumen_tick_timers();
                       _lumen_tick_timers();
                       globalThis.__b804d === 1"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `type` that is neither absent, empty nor `text/css` means the
/// element has no associated sheet at all, so §4.14 returns before
/// building one and no event is due.
#[test]
fn bug804_style_with_foreign_type_stays_silent() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"globalThis.__b804t = 0;
                       var st = document.createElement('style');
                       st.setAttribute('type', 'text/plain');
                       st.textContent = '#a { color: red; }';
                       st.onload = function() { globalThis.__b804t++; };
                       document.head.appendChild(st);
                       _lumen_apply_ready_state('interactive');
                       _lumen_tick_timers();
                       globalThis.__b804t === 0"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The `@import` scan decides whether the block has subresources to
/// obtain before it may report, so it has to read all three spellings
/// the CSS syntax allows — and only those.
#[test]
fn bug804_style_import_urls_reads_every_spelling() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            r#"var got = _lumen_style_import_urls(
                           '@import url(a.css);\n' +
                           '@import url("b.css");\n' +
                           "@import url('c.css');\n" +
                           '@import "d.css";\n' +
                           "@import 'e.css';\n" +
                           '.x { background: url(not-an-import.png); }');
                       got.join(',') === 'a.css,b.css,c.css,d.css,e.css'"#,
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn window_dcl_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var got = false; \
                 window.addEventListener('DOMContentLoaded', function() { got = true; }); \
                 _lumen_apply_ready_state('interactive'); \
                 got"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ─── navigator.sendBeacon tests ──────────────────────────────────────────

#[test]
fn send_beacon_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof navigator.sendBeacon === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn send_beacon_no_provider_returns_false() {
    let rt = v8_runtime_with_dom(make_doc());
    // No fetch provider registered → _lumen_send_beacon returns false
    let r = rt.eval("navigator.sendBeacon('https://example.com/beacon', 'data')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn send_beacon_urlsearchparams_body() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "typeof navigator.sendBeacon === 'function' && \
                 navigator.sendBeacon('https://example.com/', new URLSearchParams('k=v')) === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn send_beacon_blob_body() {
    let rt = v8_runtime_with_dom(make_doc());
    // Blob body: content_type taken from blob.type; no provider → false.
    let r = rt.eval(
        "var b = new Blob(['ping'], { type: 'application/octet-stream' }); \
                 navigator.sendBeacon('https://example.com/b', b) === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn send_beacon_with_provider_returns_true() {
    // W3C Beacon §4: sendBeacon returns true when request is queued (not when complete).
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    let r = rt.eval("navigator.sendBeacon('https://example.com/ping', 'hit')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ─── fetch keepalive + priority tests (FF-5) ─────────────────────────────

#[test]
fn fetch_keepalive_with_provider_fires_request() {
    // keepalive=true in Phase 0 behaves like a normal fetch (synchronous path),
    // so the provider is called and the response is resolved.
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    // Keepalive POST with body — should fire the request synchronously.
    let r = rt.eval(
        "var p = fetch('https://example.com/analytics', \
                   { method: 'POST', body: 'ping', keepalive: true }); \
                 p instanceof Promise"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "keepalive fetch must fire the network request");
    assert_eq!(calls[0].0, "https://example.com/analytics");
    assert_eq!(calls[0].1, "POST");
}

#[test]
fn fetch_keepalive_no_provider_returns_promise() {
    // Without a provider, keepalive fetch behaves like a normal fetch:
    // still returns a Promise (rejected), does not throw synchronously.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "fetch('https://example.com/ping', { keepalive: true }) instanceof Promise"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_priority_high_and_low_accepted() {
    // Fetch Priority Hints §2.2.6: 'high' and 'low' are valid values.
    // Both should be accepted without error; Phase 0 ignores them for scheduling.
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval(
        "fetch('https://example.com/h', { priority: 'high' }); \
                 fetch('https://example.com/l', { priority: 'low' })"
    ).unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 2, "both priority fetch calls must fire");
}

#[test]
fn fetch_priority_invalid_normalizes_to_auto() {
    // Any value outside 'high'|'low' normalises to 'auto' — no error thrown,
    // request still fires normally.
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    // 'urgent' is not a valid priority value; silently treated as 'auto'.
    rt.eval("fetch('https://example.com/', { priority: 'urgent' })").unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "invalid priority must not prevent request from firing");
}

// ─── URL.createObjectURL tests ────────────────────────────────────────────

#[test]
fn url_create_object_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var b = new Blob(['data']); \
                 var url = URL.createObjectURL(b); \
                 url.startsWith('blob:lumen/')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn url_revoke_object_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var b = new Blob(['x']); \
                 var u = URL.createObjectURL(b); \
                 URL.revokeObjectURL(u); \
                 u.startsWith('blob:lumen/')"  // revoke just removes from store, url string stays
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
