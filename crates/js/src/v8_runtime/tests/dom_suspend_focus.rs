//! Тесты `v8_runtime.rs`, вынесенные из inline-модуля `mod tests` (дорожка
//! SPLIT, батч JS-4) — вторая половина модуля.
//!
//! Четыре темы подряд, в исходном порядке: DOM- и window-нативы поверх живого
//! документа (BUG-327 `childNodes`/`hasChildNodes`, `querySelector`, именованное
//! свойство окна, `structuredClone`, таймеры, `location`, activation behavior
//! у `dispatchEvent`), затем suspend/resume (S11, частичный 10C.2), focus API
//! (BUG-381, HTML LS §6.6) и именованный доступ к окну (BUG-384, §7.3.3)
//! вместе с его гонкой за замок документа (BUG-794).

use super::*;
// Ограниченный захват документа (BUG-794) переехал в `v8_runtime::named_access`
// батчем SPLIT-JS7; глоб родителя эти два имени не протягивает — они нужны
// только тестам, и в самом `v8_runtime.rs` такой `use` погас бы как unused.
use crate::v8_runtime::named_access::{NAMED_ACCESS_LOCK_BUDGET, lock_document_bounded};

// BUG-327: `Node.prototype.hasChildNodes()` was missing entirely, and the
// ordinary live element/text/comment wrapper (`_lumen_build_element`) had no
// `.childNodes` at all (only `document`/`DocumentFragment`/detached
// `CharacterData` did) — any WPT test walking a live subtree via
// `.childNodes` or calling `.hasChildNodes()` threw or saw an empty tree.
// Mirrors WPT `dom/nodes/Document-createTextNode.html` /
// `Document-createComment-createTextNode.js` (`c.hasChildNodes is not a
// function`) and `dom/nodes/Node-childNodes.html`.
#[test]
fn node_child_nodes_and_has_child_nodes() {
    let rt = runtime_with_dom(make_doc(), "");
    let r = rt
        .eval(
            "var div = document.createElement('div'); \
                 var a = document.createElement('span'), b = document.createElement('span'); \
                 div.appendChild(a); div.appendChild(b); \
                 var t = document.createTextNode('x'); \
                 var c = document.createComment('y'); \
                 div.hasChildNodes() === true && div.childNodes.length === 2 && \
                 div.childNodes[0] === a && div.childNodes[1] === b && \
                 a.hasChildNodes() === false && a.childNodes.length === 0 && \
                 t.hasChildNodes() === false && t.childNodes.length === 0 && \
                 c.hasChildNodes() === false && c.childNodes.length === 0 && \
                 document.hasChildNodes() === true",
        )
        .unwrap();
    assert_eq!(r, JsValue::Bool(true));
}

#[test]
fn query_selector_by_class_reads_text_content() {
    let rt = runtime_with_dom(make_doc(), "");
    let text = rt
        .eval("document.querySelector('.highlight').textContent")
        .unwrap();
    assert_eq!(text, JsValue::String("Hello".into()));
}

// BUG-280 (mirrors `dom::tests::dynamic_window_property_is_bare_reachable`):
// `window` must literally BE the engine's real global object, so a property
// assigned via `window.foo = ...` — including names not known in advance,
// e.g. testharness.js's dynamic `expose(fn, name)` — resolves as a bare,
// unqualified identifier. V8 is the default engine (ADR-018), so this is
// the authoritative test surface for the fix in `WEB_API_SHIM`.
#[test]
fn dynamic_window_property_is_bare_reachable() {
    let rt = runtime_with_dom(make_doc(), "");
    let ok = rt
        .eval(
            "window.__bug280_probe = function() { return 42; }; \
                 typeof __bug280_probe === 'function' && __bug280_probe() === 42",
        )
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

// BUG-280: same for a property assigned via bare `self`, matching the
// real-browser invariant `self === window === globalThis` (same object).
#[test]
fn dynamic_self_property_is_bare_reachable() {
    let rt = runtime_with_dom(make_doc(), "");
    let ok = rt
        .eval(
            "self.__bug280_probe2 = 'hi'; \
                 typeof __bug280_probe2 !== 'undefined' && __bug280_probe2 === 'hi' \
                 && window === globalThis",
        )
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

// P3-structclone (mirrors the rquickjs `dom::tests::structured_clone_*`
// suite): the shared `WEB_API_SHIM` structuredClone must preserve cycles and
// shared references, deep-copy ArrayBuffers/typed arrays, and throw a
// DataCloneError DOMException for non-serializable values. V8 is the default
// engine (ADR-018), so this is the authoritative validation surface.
#[test]
fn structured_clone_cycles_typed_arrays_and_dataclone_error() {
    let rt = runtime_with_dom(make_doc(), "");
    let ok = rt
        .eval(
            "var o = { name: 'a' }; o.self = o; \
                 var c = structuredClone(o); \
                 var cyclesOk = c.self === c && c !== o && c.name === 'a'; \
                 var shared = { v: 1 }; \
                 var sc = structuredClone({ a: shared, b: shared }); \
                 var sharedOk = sc.a === sc.b && sc.a !== shared && sc.a.v === 1; \
                 var ta = new Uint16Array([10, 20, 30]); \
                 var tc = structuredClone(ta); tc[1] = 999; \
                 var taOk = tc instanceof Uint16Array && tc.length === 3 \
                     && tc[0] === 10 && ta[1] === 20 && tc.buffer !== ta.buffer; \
                 var threw = false, ename = ''; \
                 try { structuredClone(function(){}); } \
                 catch (e) { threw = true; ename = e.name; } \
                 var errOk = threw && ename === 'DataCloneError'; \
                 cyclesOk && sharedOk && taOk && errOk",
        )
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

#[test]
fn timeout_is_deferred_until_tick() {
    let rt = runtime_with_dom(make_doc(), "");
    // Timer must NOT fire synchronously — deferred to _lumen_tick_timers().
    let result = rt
        .eval("var x = 0; setTimeout(function() { x = 1; }, 0); x")
        .unwrap();
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn timeout_fires_after_tick() {
    let rt = runtime_with_dom(make_doc(), "");
    rt.eval("var x = 0; setTimeout(function() { x = 1; }, 0);")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); x").unwrap();
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn location_href_reads_page_url() {
    let rt = runtime_with_dom(make_doc(), "https://example.com/page");
    let href = rt.eval("window.location.href").unwrap();
    assert_eq!(href, JsValue::String("https://example.com/page".into()));
}

#[test]
fn location_href_assignment_queues_navigate_request() {
    let rt = runtime_with_dom(make_doc(), "https://example.com/page");
    rt.eval("window.location.href = 'https://example.com/next'")
        .unwrap();
    match rt.take_navigate_request() {
        Some(crate::dom::NavigateRequest::Push(url)) => {
            assert_eq!(url, "https://example.com/next");
        }
        other => panic!("expected NavigateRequest::Push, got {other:?}"),
    }
    // Consumed — a second read returns None.
    assert!(rt.take_navigate_request().is_none());
}

/// BUG-439: a JS-synthesized `click` (`el.dispatchEvent(new MouseEvent(...))`,
/// as opposed to the native `HTMLElement.prototype.click()`) must still run
/// the target's activation behavior — here, submitting the owning form —
/// as long as the event was not cancelled and is not already trusted.
#[test]
fn dispatch_event_click_runs_activation_behavior_for_submit_button() {
    let rt = runtime_with_dom(make_doc(), "https://example.com/page");
    rt.eval(
        "var form = document.createElement('form');
             var btn = document.createElement('button');
             btn.type = 'submit';
             form.appendChild(btn);
             document.body.appendChild(form);
             btn.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));",
    )
    .unwrap();
    match rt.take_navigate_request() {
        Some(crate::dom::NavigateRequest::SubmitForm { .. }) => {}
        other => panic!("expected NavigateRequest::SubmitForm, got {other:?}"),
    }
}

/// Same as above, but the `click` handler calls `preventDefault()` — the
/// activation behavior (form submit) must not run.
#[test]
fn dispatch_event_click_cancelled_skips_activation_behavior() {
    let rt = runtime_with_dom(make_doc(), "https://example.com/page");
    rt.eval(
        "var form = document.createElement('form');
             var btn = document.createElement('button');
             btn.type = 'submit';
             btn.addEventListener('click', function(e) { e.preventDefault(); });
             form.appendChild(btn);
             document.body.appendChild(form);
             btn.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));",
    )
    .unwrap();
    assert!(rt.take_navigate_request().is_none());
}

// ── S11: suspend/resume (partial 10C.2) ───────────────────────────────────

#[test]
fn suspend_resume_round_trips_data_global() {
    let mut rt = rt();
    rt.eval("globalThis.__test = 42").unwrap();
    let heap = rt.suspend().unwrap();
    let resumed = V8JsRuntime::resume(heap).unwrap();
    assert_eq!(resumed.get_global("__test").unwrap(), JsValue::Number(42.0));
}

#[test]
fn suspend_resume_round_trips_string_and_array() {
    let mut rt = rt();
    rt.eval(r#"globalThis.__name = "lumen"; globalThis.__items = [1, 2, 3];"#)
        .unwrap();
    let heap = rt.suspend().unwrap();
    let resumed = V8JsRuntime::resume(heap).unwrap();
    assert_eq!(
        resumed.get_global("__name").unwrap(),
        JsValue::String("lumen".into())
    );
    assert_eq!(
        resumed.eval("__items[0] + __items[1] + __items[2]").unwrap(),
        JsValue::Number(6.0)
    );
}

#[test]
fn suspend_resume_round_trips_plain_object() {
    let mut rt = rt();
    rt.eval("globalThis.__state = { count: 7, label: 'ok' }")
        .unwrap();
    let heap = rt.suspend().unwrap();
    let resumed = V8JsRuntime::resume(heap).unwrap();
    assert_eq!(
        resumed.eval("__state.count").unwrap(),
        JsValue::Number(7.0)
    );
    assert_eq!(
        resumed.eval("__state.label").unwrap(),
        JsValue::String("ok".into())
    );
}

#[test]
fn suspend_drops_closures_but_keeps_sibling_data() {
    // F1: closures are not structured-cloneable. A function-valued global
    // must not abort the whole capture — sibling data globals still
    // round-trip, and the function is simply absent afterwards.
    let mut rt = rt();
    rt.eval("globalThis.__fn = function() { return 1; }; globalThis.__ok = 'kept';")
        .unwrap();
    let heap = rt.suspend().unwrap();
    let resumed = V8JsRuntime::resume(heap).unwrap();
    assert_eq!(
        resumed.get_global("__ok").unwrap(),
        JsValue::String("kept".into())
    );
    // The restored runtime never had `__fn` installed — reading it back
    // yields `undefined`, not the original function.
    assert_eq!(resumed.eval("typeof __fn").unwrap(), JsValue::String("undefined".into()));
}

#[test]
fn suspend_without_page_globals_restores_nothing() {
    // No page script ran — only baseline (built-in) globals exist, so
    // there is nothing new to capture. `compress_heap` always frames a
    // non-empty zlib stream (magic + header), even over empty input, so
    // the round-trip behavior is asserted instead of raw byte length.
    let mut rt = rt();
    let heap = rt.suspend().unwrap();
    let resumed = V8JsRuntime::resume(heap).unwrap();
    assert_eq!(
        resumed.eval("typeof __anything").unwrap(),
        JsValue::String("undefined".into())
    );
}

#[test]
fn resume_of_empty_snapshot_yields_fresh_runtime() {
    let resumed = V8JsRuntime::resume(SuspendedHeap::default()).unwrap();
    assert_eq!(
        resumed.eval("typeof __anything").unwrap(),
        JsValue::String("undefined".into())
    );
}

// BUG-291 end-to-end: reproduces `testharness.js`'s `Output.show_results`
// pattern verbatim — build a `section > table > tbody` tree entirely
// detached from the document via a template-driven `render()` (the same
// `substitute`/`make_dom` helpers `tests/wpt/resources/testharness.js`
// uses), fetch `tbody` via `section.querySelector("tbody")` while still
// detached, append several rows, then do the exact crash-site call
// (`tbody.lastChild.lastChild.appendChild(...)`) for each row before
// finally attaching the whole tree to the document. Before the fix this
// threw `Cannot read properties of null (reading 'appendChild')` on the
// very first row, because `Element.querySelector` ignored `this` and
// searched from `document.root()` — which never reaches a detached tree.
#[test]
fn bug291_testharness_results_table_pattern_does_not_throw() {
    let rt = runtime_with_dom(make_doc(), "");
    let r = rt.eval(r#"
            function is_single_node(template) { return typeof template[0] === "string"; }
            function filter(array, callable) {
                var rv = [];
                for (var i = 0; i < array.length; i++) { if (callable(array[i])) rv.push(array[i]); }
                return rv;
            }
            function map(array, callable) {
                var rv = [];
                for (var i = 0; i < array.length; i++) { rv.push(callable(array[i])); }
                return rv;
            }
            function extend(array, items) { Array.prototype.push.apply(array, items); }

            function substitute(template, substitutions) {
                if (typeof template === "function") {
                    var replacement = template(substitutions);
                    if (!replacement) return null;
                    return substitute(replacement, substitutions);
                }
                if (is_single_node(template)) return substitute_single(template, substitutions);
                return filter(map(template, function(x) { return substitute(x, substitutions); }),
                               function(x) { return x !== null; });
            }
            function substitute_single(template, substitutions) {
                var substitution_re = /\$\{([^ }]*)\}/g;
                function do_substitution(input) {
                    var components = input.split(substitution_re);
                    var rv = [];
                    if (components.length === 1) { rv = components; }
                    else if (substitutions) {
                        for (var i = 0; i < components.length; i += 2) {
                            if (components[i]) rv.push(components[i]);
                            if (substitutions[components[i + 1]]) rv.push(String(substitutions[components[i + 1]]));
                        }
                    }
                    return rv;
                }
                function substitute_attrs(attrs, rv) {
                    rv[1] = {};
                    for (var name in template[1]) {
                        if (attrs.hasOwnProperty(name)) {
                            rv[1][do_substitution(name).join("")] = do_substitution(attrs[name]).join("");
                        }
                    }
                }
                function substitute_children(children, rv) {
                    for (var i = 0; i < children.length; i++) {
                        if (children[i] instanceof Object) {
                            var replacement = substitute(children[i], substitutions);
                            if (replacement !== null) {
                                if (is_single_node(replacement)) rv.push(replacement);
                                else extend(rv, replacement);
                            }
                        } else {
                            extend(rv, do_substitution(String(children[i])));
                        }
                    }
                    return rv;
                }
                var rv = [];
                rv.push(do_substitution(String(template[0])).join(""));
                if (template[0] === "{text}") { substitute_children(template.slice(1), rv); }
                else { substitute_attrs(template[1], rv); substitute_children(template.slice(2), rv); }
                return rv;
            }
            function make_dom_single(template, doc) {
                var output_document = doc || document;
                var element;
                if (template[0] === "{text}") {
                    element = output_document.createTextNode("");
                    for (var i = 1; i < template.length; i++) { element.data += template[i]; }
                } else {
                    element = output_document.createElementNS('http://www.w3.org/1999/xhtml', template[0]);
                    for (var name in template[1]) {
                        if (template[1].hasOwnProperty(name)) element.setAttribute(name, template[1][name]);
                    }
                    for (var i = 2; i < template.length; i++) {
                        if (template[i] instanceof Object) {
                            element.appendChild(make_dom(template[i]));
                        } else {
                            element.appendChild(output_document.createTextNode(template[i]));
                        }
                    }
                }
                return element;
            }
            function make_dom(template, substitutions, output_document) {
                if (is_single_node(template)) return make_dom_single(template, output_document);
                return map(template, function(x) { return make_dom_single(x, output_document); });
            }
            function render(template, substitutions, output_document) {
                return make_dom(substitute(template, substitutions), output_document);
            }

            var log = document.createElement('div');
            document.body.appendChild(log);

            // Built entirely detached — not yet reachable from document.root().
            var section = render(
                ["section", {},
                    ["h2", {}, "Details"],
                    ["table", {"id":"results", "class":""},
                        ["thead", {},
                            ["tr", {},
                                ["th", {}, "Result"],
                                ["th", {}, "Test Name"],
                                ["th", {}, "Message" ]]],
                        ["tbody", {}]]]);
            var tbody = section.querySelector("tbody");
            if (tbody === null) throw new Error("querySelector on detached subtree returned null");

            var tests = [
                { name: 'foo', message: undefined, stack: undefined },
                { name: 'bar', message: 'assert_equals: expected 1 but got 2', stack: 'at foo (file.js:1:1)' },
                { name: 'baz', message: undefined, stack: undefined },
            ];
            for (var ti = 0; ti < tests.length; ti++) {
                var test = tests[ti];
                tbody.appendChild(render(
                    ["tr", {"class":"overall-pass"},
                        ["td", {"class":"pass"}, "PASS"],
                        ["td", {}, test.name],
                        ["td", {},
                            test.message ?? "",
                            ["pre", {}, test.stack ?? ""]]]));
                // The exact BUG-291 crash site.
                tbody.lastChild.lastChild.appendChild(
                    document.createElementNS('http://www.w3.org/1999/xhtml', 'details'));
            }
            log.appendChild(section);
            document.querySelectorAll('tbody tr').length;
        "#).unwrap();
    assert_eq!(r, JsValue::Number(3.0));
}

// ── BUG-381: focus API (HTML LS §6.6) ──────────────────────────────────────

/// `html > body > [ a#link[href], input#field, input#off[disabled],
/// div#plain, div#tabbed[tabindex=3] ]` — one representative of every branch
/// `_lumen_is_focusable` distinguishes.
fn make_focus_doc() -> Arc<Mutex<lumen_dom::Document>> {
    fn attr(doc: &mut lumen_dom::Document, nid: lumen_dom::NodeId, name: &str, value: &str) {
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html(name),
                value: value.into(),
            });
        }
    }
    let mut doc = lumen_dom::Document::new();
    let html = doc.create_element(lumen_dom::QualName::html("html"));
    let body = doc.create_element(lumen_dom::QualName::html("body"));
    let link = doc.create_element(lumen_dom::QualName::html("a"));
    attr(&mut doc, link, "id", "link");
    attr(&mut doc, link, "href", "#x");
    let field = doc.create_element(lumen_dom::QualName::html("input"));
    attr(&mut doc, field, "id", "field");
    let off = doc.create_element(lumen_dom::QualName::html("input"));
    attr(&mut doc, off, "id", "off");
    attr(&mut doc, off, "disabled", "");
    let plain = doc.create_element(lumen_dom::QualName::html("div"));
    attr(&mut doc, plain, "id", "plain");
    let tabbed = doc.create_element(lumen_dom::QualName::html("div"));
    attr(&mut doc, tabbed, "id", "tabbed");
    attr(&mut doc, tabbed, "tabindex", "3");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    for child in [link, field, off, plain, tabbed] {
        doc.append_child(body, child);
    }
    Arc::new(Mutex::new(doc))
}

/// Evaluate `script` and assert it produced exactly `true`.
fn assert_js_true(rt: &V8JsRuntime, script: &str) {
    assert_eq!(rt.eval(script).unwrap(), JsValue::Bool(true), "script: {script}");
}

#[test]
fn focus_api_surface_is_defined() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(&rt, "typeof HTMLElement.prototype.focus === 'function'");
    assert_js_true(&rt, "typeof HTMLElement.prototype.blur === 'function'");
    assert_js_true(&rt, "typeof document.hasFocus === 'function'");
    assert_js_true(&rt, "typeof window.focus === 'function'");
    assert_js_true(&rt, "typeof window.blur === 'function'");
    assert_js_true(&rt, "typeof document.getElementById('field').focus === 'function'");
}

#[test]
fn active_element_defaults_to_body() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(&rt, "document.activeElement.tagName === 'BODY'");
}

#[test]
fn element_focus_updates_active_element_synchronously() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "document.getElementById('field').focus(); document.activeElement.id === 'field'",
    );
}

#[test]
fn element_focus_queues_a_shell_request() {
    let doc = make_focus_doc();
    let field = doc.lock().unwrap().find_by_id("field").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval("document.getElementById('field').focus()").unwrap();
    assert_eq!(rt.take_focus_requests(), vec![Some(field.index() as u32)]);
}

#[test]
fn element_focus_fires_focus_then_focusin() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var seen = [];\
             var el = document.getElementById('field');\
             el.addEventListener('focus', function() { seen.push('focus'); });\
             el.addEventListener('focusin', function() { seen.push('focusin'); });\
             el.focus();\
             seen.join(',') === 'focus,focusin'",
    );
}

#[test]
fn focusin_bubbles_but_focus_does_not() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var seen = [];\
             document.body.addEventListener('focus', function() { seen.push('focus'); });\
             document.body.addEventListener('focusin', function() { seen.push('focusin'); });\
             document.getElementById('field').focus();\
             seen.join(',') === 'focusin'",
    );
}

#[test]
fn on_focus_property_handler_runs() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var hit = false;\
             var el = document.getElementById('field');\
             el.onfocus = function() { hit = true; };\
             el.focus();\
             hit",
    );
}

#[test]
fn moving_focus_fires_blur_focusout_focus_focusin_in_order() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var seen = [];\
             var a = document.getElementById('link'), b = document.getElementById('field');\
             ['focus','blur','focusin','focusout'].forEach(function(t) {\
                 a.addEventListener(t, function() { seen.push('a:' + t); });\
                 b.addEventListener(t, function() { seen.push('b:' + t); });\
             });\
             a.focus(); seen = []; b.focus();\
             seen.join(',') === 'a:blur,a:focusout,b:focus,b:focusin'",
    );
}

#[test]
fn focus_events_carry_related_target() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var related = 'unset';\
             var a = document.getElementById('link'), b = document.getElementById('field');\
             a.focus();\
             b.addEventListener('focus', function(e) { related = e.relatedTarget ? e.relatedTarget.id : null; });\
             b.focus();\
             related === 'link'",
    );
}

#[test]
fn refocusing_the_same_element_fires_nothing() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var n = 0;\
             var el = document.getElementById('field');\
             el.addEventListener('focus', function() { n++; });\
             el.focus(); el.focus(); el.focus();\
             n === 1",
    );
}

#[test]
fn blur_resets_active_element_to_body_and_fires_blur() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "var seen = [];\
             var el = document.getElementById('field');\
             el.addEventListener('blur', function() { seen.push('blur'); });\
             el.addEventListener('focusout', function() { seen.push('focusout'); });\
             el.focus(); el.blur();\
             seen.join(',') === 'blur,focusout' && document.activeElement.tagName === 'BODY'",
    );
}

#[test]
fn blur_on_an_unfocused_element_is_a_noop() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "document.getElementById('field').focus();\
             document.getElementById('link').blur();\
             document.activeElement.id === 'field'",
    );
}

#[test]
fn non_focusable_elements_are_not_focused() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    // A bare <div> and a disabled <input> both stay unfocusable; a <div> with
    // an explicit tabindex and an <a href> do not.
    assert_js_true(
        &rt,
        "document.getElementById('plain').focus();\
             document.activeElement.tagName === 'BODY'",
    );
    assert_js_true(
        &rt,
        "document.getElementById('off').focus();\
             document.activeElement.tagName === 'BODY'",
    );
    assert_js_true(
        &rt,
        "document.getElementById('tabbed').focus();\
             document.activeElement.id === 'tabbed'",
    );
    assert_js_true(
        &rt,
        "document.getElementById('link').focus();\
             document.activeElement.id === 'link'",
    );
}

#[test]
fn inert_subtree_has_no_focusable_areas() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(
        &rt,
        "document.body.setAttribute('inert', '');\
             document.getElementById('field').focus();\
             document.activeElement.tagName === 'BODY'",
    );
}

#[test]
fn tab_index_reflects_the_content_attribute() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    assert_js_true(&rt, "document.getElementById('tabbed').tabIndex === 3");
    // Focusable without the attribute → 0; everything else → −1.
    assert_js_true(&rt, "document.getElementById('field').tabIndex === 0");
    assert_js_true(&rt, "document.getElementById('plain').tabIndex === -1");
    assert_js_true(&rt, "document.body.tabIndex === -1");
    assert_js_true(
        &rt,
        "var d = document.getElementById('plain');\
             d.tabIndex = -1;\
             d.getAttribute('tabindex') === '-1' && d.tabIndex === -1",
    );
    // BUG-452: a trailing tail is IGNORED, not rejected — HTML LS §2.4.4.1 step
    // 8 collects ASCII digits and stops, so `'2px'` reflects as 2. This used to
    // assert −1, which was the shared parser's own too-strict behaviour written
    // down as if it were the rule; WPT's reference implementation of the same
    // steps (`tests/wpt/html/dom/reflection.js::ReflectionTests.parseInt`)
    // returns 5 for its `"5%"` case through exactly this branch.
    assert_js_true(
        &rt,
        "var d = document.getElementById('plain');\
             d.setAttribute('tabindex', '2px');\
             d.tabIndex === 2",
    );
    // Genuinely unparseable, and out of the reflected `long` range, both fall
    // back to the default.
    assert_js_true(
        &rt,
        "var d = document.getElementById('plain');\
             d.setAttribute('tabindex', 'px2');\
             d.tabIndex === -1",
    );
    assert_js_true(
        &rt,
        "var d = document.getElementById('plain');\
             d.setAttribute('tabindex', '\\u00A07');\
             d.tabIndex === -1",
    );
    // Out of the reflected `long` range → the DEFAULT, not the value read back
    // verbatim. Which default that is comes from focusability, not from this
    // rule: §2.4.4.1 parses `'2147483648'` successfully (the range cap lives in
    // §2.6.2 reflection, not in the parse), so the `tabindex` focus flag is set
    // and the element's default is 0 — hence the assertion is «not verbatim»
    // plus the focusable default, not a hardcoded −1.
    assert_js_true(
        &rt,
        "var d = document.getElementById('plain');\
             d.setAttribute('tabindex', '2147483648');\
             d.tabIndex === 0",
    );
}

#[test]
fn autofocus_reflects_the_content_attribute() {
    let rt = runtime_with_dom(make_focus_doc(), "");
    // Attribute presence is checked via `getAttribute`, not `hasAttribute`:
    // the latter answers `true` for every name on the V8 bindings (BUG-442).
    assert_js_true(
        &rt,
        "var el = document.getElementById('field');\
             var before = el.autofocus;\
             el.autofocus = true;\
             var mid = el.autofocus && el.getAttribute('autofocus') === '';\
             el.autofocus = false;\
             before === false && mid && el.autofocus === false\
                 && el.getAttribute('autofocus') === null",
    );
}

#[test]
fn shell_reported_focus_change_fires_events_and_moves_active_element() {
    let doc = make_focus_doc();
    let field = doc.lock().unwrap().find_by_id("field").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval(
        "globalThis.seen = [];\
             document.getElementById('field')\
                 .addEventListener('focus', function() { globalThis.seen.push('focus'); });",
    )
    .unwrap();
    // What the shell does after a click moves `focused_node`.
    rt.eval(&format!("_lumen_focus_update({})", field.index())).unwrap();
    assert_js_true(&rt, "document.activeElement.id === 'field'");
    assert_js_true(&rt, "globalThis.seen.join(',') === 'focus'");
}

#[test]
fn shell_echo_of_a_page_initiated_focus_does_not_double_dispatch() {
    let doc = make_focus_doc();
    let field = doc.lock().unwrap().find_by_id("field").unwrap();
    let rt = runtime_with_dom(doc, "");
    rt.eval(
        "globalThis.n = 0;\
             var el = document.getElementById('field');\
             el.addEventListener('focus', function() { globalThis.n++; });\
             el.focus();",
    )
    .unwrap();
    // The shell drains the queued request and echoes it back — must be a no-op.
    rt.eval(&format!("_lumen_focus_update({})", field.index())).unwrap();
    assert_js_true(&rt, "globalThis.n === 1");
}

#[test]
fn shell_reported_focus_on_a_text_node_resolves_to_its_element() {
    // The shell tracks focus by layout box, whose node can be a text node.
    let mut d = lumen_dom::Document::new();
    let html = d.create_element(lumen_dom::QualName::html("html"));
    let body = d.create_element(lumen_dom::QualName::html("body"));
    let link = d.create_element(lumen_dom::QualName::html("a"));
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut d.get_mut(link).data {
        attrs.push(lumen_dom::Attribute {
            name: lumen_dom::QualName::html("id"),
            value: "link".into(),
        });
    }
    let text = d.create_text("click me");
    d.append_child(d.root(), html);
    d.append_child(html, body);
    d.append_child(body, link);
    d.append_child(link, text);
    let text_idx = text.index();
    let rt = runtime_with_dom(Arc::new(Mutex::new(d)), "");
    rt.eval(&format!("_lumen_focus_update({text_idx})")).unwrap();
    assert_js_true(&rt, "document.activeElement.id === 'link'");
}

#[test]
fn autofocus_element_is_focused_when_parsing_completes() {
    let mut d = lumen_dom::Document::new();
    let html = d.create_element(lumen_dom::QualName::html("html"));
    let body = d.create_element(lumen_dom::QualName::html("body"));
    let field = d.create_element(lumen_dom::QualName::html("input"));
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut d.get_mut(field).data {
        attrs.push(lumen_dom::Attribute {
            name: lumen_dom::QualName::html("id"),
            value: "field".into(),
        });
        attrs.push(lumen_dom::Attribute {
            name: lumen_dom::QualName::html("autofocus"),
            value: "".into(),
        });
    }
    d.append_child(d.root(), html);
    d.append_child(html, body);
    d.append_child(body, field);
    let rt = runtime_with_dom(Arc::new(Mutex::new(d)), "");
    assert_js_true(&rt, "document.activeElement.tagName === 'BODY'");
    rt.eval("_lumen_apply_ready_state('interactive')").unwrap();
    assert_js_true(&rt, "document.activeElement.id === 'field'");
}

// ── BUG-384: named access on Window (HTML LS §7.3.3) ─────────────────────

/// `html > body > (div#probe, img[name=logo], div[name=plain], div#Object)`.
/// Deliberately mixes the three cases the interceptor must tell apart: an
/// `id` (always a named property), a `name` on one of the five eligible
/// elements, and a `name` on an element that is *not* eligible — plus one
/// `id` that collides with an ECMAScript built-in.
fn make_named_access_doc() -> Arc<Mutex<lumen_dom::Document>> {
    fn attr(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str, value: &str) {
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html(name),
                value: value.into(),
            });
        }
    }
    let mut d = lumen_dom::Document::new();
    let html = d.create_element(lumen_dom::QualName::html("html"));
    let body = d.create_element(lumen_dom::QualName::html("body"));
    let probe = d.create_element(lumen_dom::QualName::html("div"));
    attr(&mut d, probe, "id", "probe");
    let logo = d.create_element(lumen_dom::QualName::html("img"));
    attr(&mut d, logo, "name", "logo");
    let plain = d.create_element(lumen_dom::QualName::html("div"));
    attr(&mut d, plain, "name", "plain");
    let shadowing = d.create_element(lumen_dom::QualName::html("div"));
    attr(&mut d, shadowing, "id", "Object");
    d.append_child(d.root(), html);
    d.append_child(html, body);
    for child in [probe, logo, plain, shadowing] {
        d.append_child(body, child);
    }
    Arc::new(Mutex::new(d))
}

/// The bug's own regression check, verbatim from `BUG-384-FIXED.md`: a
/// `<div id="probe">` must be reachable as `window.probe`, as the bare
/// identifier `probe`, and must answer `true` to `'probe' in window`.
/// Before the fix all three were `undefined`/`false` — a bare reference
/// threw `ReferenceError` and took the rest of the script with it.
#[test]
fn element_id_is_a_named_property_of_window() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    assert_js_true(&rt, "'probe' in window");
    assert_js_true(&rt, "window.probe !== undefined && window.probe !== null");
    assert_js_true(&rt, "probe.tagName === 'DIV'");
    assert_js_true(&rt, "globalThis.probe === window.probe");
}

/// Named access must hand back the *same wrapper* `getElementById` does —
/// otherwise `window.probe === document.getElementById('probe')` is false
/// and identity-based code (event-listener bookkeeping, `Map` keys) breaks
/// in a way that is harder to spot than the missing property was.
#[test]
fn named_access_yields_the_same_wrapper_as_get_element_by_id() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    assert_js_true(&rt, "window.probe === document.getElementById('probe')");
}

/// Only `img`/`form`/`iframe`/`embed`/`object` expose their `name`
/// attribute as a named property (HTML LS §7.3.3); a `name` on any other
/// element is not a supported property name.
#[test]
fn name_attribute_is_named_property_only_on_eligible_elements() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    assert_js_true(&rt, "window.logo !== undefined && window.logo.tagName === 'IMG'");
    assert_js_true(&rt, "window.plain === undefined");
    assert_js_true(&rt, "!('plain' in window)");
}

/// A name that matches nothing must stay unresolved — the interceptor
/// declines, so the pre-existing `undefined`/`ReferenceError` behaviour is
/// untouched for everything that is not a named property.
#[test]
fn unmatched_name_stays_undefined() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    assert_js_true(&rt, "window.nosuchelement === undefined");
    assert_js_true(&rt, "!('nosuchelement' in window)");
    assert_js_true(&rt, "typeof nosuchelement === 'undefined'");
}

/// Resolution order (the reason the interceptor is `NON_MASKING`): a real
/// property of the global wins over an element with the same `id`. The
/// document deliberately contains a `<div id="Object">`; if named access
/// masked built-ins, `Object.keys` would be gone.
#[test]
fn real_globals_win_over_named_elements() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    assert_js_true(&rt, "typeof Object === 'function' && typeof Object.keys === 'function'");
    assert_js_true(&rt, "typeof document === 'object'");
}

/// …and so does a page-script `var`, which is what HTML LS §7.3.3 requires
/// (the named-properties object sits *behind* the global's own properties).
#[test]
fn page_variable_wins_over_named_element() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    rt.eval("var probe = 42;").unwrap();
    assert_js_true(&rt, "probe === 42");
    assert_js_true(&rt, "window.probe === 42");
}

/// The lookup runs against the live document, not a snapshot taken at
/// install time: an element appended by script is immediately reachable by
/// name, and unreachable again once removed.
#[test]
fn named_access_follows_live_dom_mutations() {
    let rt = runtime_with_dom(make_named_access_doc(), "");
    assert_js_true(&rt, "window.later === undefined");
    rt.eval(
        "var el = document.createElement('div');\
             el.setAttribute('id', 'later');\
             document.body.appendChild(el);",
    )
    .unwrap();
    assert_js_true(&rt, "window.later !== undefined && window.later.tagName === 'DIV'");
    rt.eval("document.body.removeChild(el);").unwrap();
    assert_js_true(&rt, "window.later === undefined");
}

// ── BUG-794: the lookup must survive a lock held by another thread ────────

/// An uncontended lock is taken on the first `try_lock`, without paying the
/// poll interval.
#[test]
fn bounded_document_lock_takes_a_free_lock_at_once() {
    let doc = make_named_access_doc();
    let start = std::time::Instant::now();
    assert!(lock_document_bounded(&doc).is_some());
    assert!(start.elapsed() < NAMED_ACCESS_LOCK_BUDGET);
}

/// The bug itself: the window `load` event is dispatched on the engine
/// thread while the UI thread still holds the document lock (measured at
/// 3.9 ms), so a bare `try_lock` declines and every named element in a
/// `load` handler becomes a `ReferenceError`. Waiting out a hold of that
/// order must resolve the name instead.
#[test]
fn bounded_document_lock_waits_out_another_thread() {
    let doc = make_named_access_doc();
    let held = Arc::clone(&doc);
    let (tx, rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let guard = held.lock().unwrap();
        tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(4));
        drop(guard);
    });
    rx.recv().unwrap();
    assert!(doc.try_lock().is_err(), "the other thread must hold the lock");
    assert!(lock_document_bounded(&doc).is_some());
    holder.join().unwrap();
}

/// …and it is a *bounded* wait, not a blocking one: a lock this thread will
/// never see released — in production, one this very thread already holds —
/// must decline within the budget rather than deadlock the JS thread.
#[test]
fn bounded_document_lock_gives_up_on_a_lock_that_never_frees() {
    let doc = make_named_access_doc();
    let held = Arc::clone(&doc);
    let release = Arc::new(AtomicBool::new(false));
    let release_holder = Arc::clone(&release);
    let (tx, rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let guard = held.lock().unwrap();
        tx.send(()).unwrap();
        while !release_holder.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        drop(guard);
    });
    rx.recv().unwrap();
    let start = std::time::Instant::now();
    assert!(lock_document_bounded(&doc).is_none());
    assert!(start.elapsed() >= NAMED_ACCESS_LOCK_BUDGET);
    release.store(true, Ordering::SeqCst);
    holder.join().unwrap();
}
