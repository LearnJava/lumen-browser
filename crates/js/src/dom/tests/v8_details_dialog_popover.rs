//! Тесты `v8_details_dialog_popover`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn toggle_attribute_add() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.toggleAttribute('hidden') === true && el.hasAttribute('hidden')"));
}

#[test]
fn toggle_attribute_remove() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('hidden', ''); \
                 el.toggleAttribute('hidden') === false && !el.hasAttribute('hidden')"));
}

#[test]
fn toggle_attribute_force_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.toggleAttribute('hidden', true) === true && el.hasAttribute('hidden')"));
}

#[test]
fn toggle_attribute_force_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('hidden', ''); \
                 el.toggleAttribute('hidden', false) === false && !el.hasAttribute('hidden')"));
}

// ── BUG-594: `hidden` tristate reflection ────────────────────────────────

#[test]
fn hidden_getter_until_found_case_insensitive() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('hidden', 'UNTIL-FOUND'); \
                 el.hidden === 'until-found'"));
}

#[test]
fn hidden_getter_other_value_is_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('hidden', 'foo'); \
                 el.hidden === true"));
}

#[test]
fn hidden_getter_absent_is_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.hidden === false"));
}

#[test]
fn hidden_setter_string_until_found() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.hidden = 'until-found'; \
                 el.getAttribute('hidden') === 'until-found' && el.hidden === 'until-found'"));
}

#[test]
fn hidden_setter_nonempty_string_is_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.hidden = 'foo'; \
                 el.getAttribute('hidden') === '' && el.hidden === true"));
}

#[test]
fn hidden_setter_false_removes_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('hidden', 'until-found'); \
                 el.hidden = false; \
                 !el.hasAttribute('hidden') && el.hidden === false"));
}

#[test]
fn hidden_setter_number_follows_tobool() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.hidden = 1; \
                 var a = el.getAttribute('hidden') === '' && el.hidden === true; \
                 el.hidden = 0; \
                 var b = !el.hasAttribute('hidden') && el.hidden === false; \
                 a && b"));
}

// ── BUG-595: `autocorrect`/`writingSuggestions` global attributes ────────

#[test]
fn autocorrect_absent_is_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 'autocorrect' in el && el.autocorrect === true"));
}

#[test]
fn autocorrect_off_is_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('autocorrect', 'OFF'); \
                 el.autocorrect === false"));
}

#[test]
fn autocorrect_invalid_value_is_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('autocorrect', 'invalid_value'); \
                 el.autocorrect === true"));
}

#[test]
fn autocorrect_setter_writes_on_off_keyword() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.autocorrect = 'hello'; \
                 var a = el.getAttribute('autocorrect') === 'on' && el.autocorrect === true; \
                 el.autocorrect = false; \
                 var b = el.getAttribute('autocorrect') === 'off' && el.autocorrect === false; \
                 a && b"));
}

#[test]
fn writing_suggestions_available_on_every_element() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "'writingSuggestions' in document.createElement('div')"));
}

#[test]
fn writing_suggestions_default_is_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.writingSuggestions === 'true'"));
}

#[test]
fn writing_suggestions_own_false_overrides_default() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('writingsuggestions', 'false'); \
                 el.writingSuggestions === 'false' && \
                 el.getAttribute('writingsuggestions') === 'false'"));
}

#[test]
fn writing_suggestions_invalid_own_value_falls_back_to_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('writingsuggestions', 'foo'); \
                 el.writingSuggestions === 'true'"));
}

#[test]
fn writing_suggestions_inherits_from_ancestor() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var parent = document.getElementById('main'); \
                 var child = parent.querySelector('.highlight'); \
                 parent.setAttribute('writingsuggestions', 'false'); \
                 child.writingSuggestions === 'false' && \
                 child.getAttribute('writingsuggestions') === null"));
}

#[test]
fn writing_suggestions_own_value_overrides_inherited() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var parent = document.getElementById('main'); \
                 var child = parent.querySelector('.highlight'); \
                 parent.setAttribute('writingsuggestions', 'false'); \
                 child.setAttribute('writingsuggestions', 'true'); \
                 child.writingSuggestions === 'true' && parent.writingSuggestions === 'false'"));
}

// ── <details>/<summary> + <dialog> tests ─────────────────────────────────

/// Build a doc with <details id="d"><summary id="s">Sum</summary><p>Body</p></details>
/// and <dialog id="dlg">Hello</dialog>.
fn make_details_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html    = doc.create_element(QualName::html("html"));
    let body    = doc.create_element(QualName::html("body"));
    let details = doc.create_element(QualName::html("details"));
    let summary = doc.create_element(QualName::html("summary"));
    let p       = doc.create_element(QualName::html("p"));
    let dialog  = doc.create_element(QualName::html("dialog"));
    fn set_id(doc: &mut Document, nid: lumen_dom::NodeId, id: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: id.into() });
        }
    }
    set_id(&mut doc, details, "d");
    set_id(&mut doc, summary, "s");
    set_id(&mut doc, dialog,  "dlg");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, details);
    doc.append_child(details, summary);
    doc.append_child(details, p);
    doc.append_child(body, dialog);
    Arc::new(Mutex::new(doc))
}

#[test]
fn details_open_property_getter() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('d').open === false"));
}

#[test]
fn details_open_property_setter() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); \
                 d.open = true; \
                 d.hasAttribute('open') && d.open === true"));
}

#[test]
fn details_summary_click_opens() {
    let rt = v8_runtime_with_dom(make_details_doc());
    rt.eval("document.getElementById('s').click()").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('d').hasAttribute('open')"));
}

#[test]
fn details_summary_click_closes() {
    let rt = v8_runtime_with_dom(make_details_doc());
    rt.eval("document.getElementById('d').setAttribute('open', '')").unwrap();
    rt.eval("document.getElementById('s').click()").unwrap();
    assert!(bool_eval(&rt,
        "!document.getElementById('d').hasAttribute('open')"));
}

/// BUG-851: the state a `toggle` handler sees has to survive the statement
/// after the click. The flip used to happen twice — once in a `click`
/// listener on `document` (which also dispatched the event) and once in
/// the activation behaviour — so the handler saw `open` and the caller,
/// one statement later, saw the attribute gone again.
#[test]
fn details_summary_click_survives_dispatch() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); var seen = null; \
                 d.addEventListener('toggle', function() { seen = d.open; }); \
                 document.getElementById('s').click(); \
                 _lumen_tick_timers(); \
                 seen === true && d.open === true"));
}

/// HTML LS §4.11.2: only the FIRST `<summary>` child is the disclosure
/// control, so activating a second one is not an activation at all.
#[test]
fn details_second_summary_does_not_toggle() {
    let doc = make_details_doc();
    {
        let mut d = doc.lock().unwrap();
        let details = d.find_by_id("d").expect("details");
        let extra = d.create_element(QualName::html("summary"));
        if let NodeData::Element { attrs, .. } = &mut d.get_mut(extra).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"), value: "s2".into(),
            });
        }
        d.append_child(details, extra);
    }
    let rt = v8_runtime_with_dom(doc);
    rt.eval("document.getElementById('s2').click()").unwrap();
    assert!(bool_eval(&rt, "!document.getElementById('d').open"));
}

#[test]
fn details_toggle_event_fired() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var got = null; \
                 document.getElementById('d').addEventListener('toggle', function(e) { got = e; }); \
                 document.getElementById('s').click(); \
                 _lumen_tick_timers(); \
                 got !== null && got.oldState === 'closed' && got.newState === 'open' \
                 && got.isTrusted === true && got.bubbles === false && got.cancelable === false \
                 && got.target === document.getElementById('d') \
                 && Object.getPrototypeOf(got) === ToggleEvent.prototype"));
}

/// HTML LS §4.6.1: an `<a>` with no `href` is a placeholder, not a
/// hyperlink — it has no activation behaviour, so a click inside it must
/// reach the `<summary>` above it (`anchor-without-link.html`).
#[test]
fn details_click_on_hrefless_anchor_in_summary_opens() {
    let doc = make_details_doc();
    {
        let mut d = doc.lock().unwrap();
        let summary = d.find_by_id("s").expect("summary");
        let anchor = d.create_element(QualName::html("a"));
        if let NodeData::Element { attrs, .. } = &mut d.get_mut(anchor).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"), value: "a1".into(),
            });
        }
        d.append_child(summary, anchor);
    }
    let rt = v8_runtime_with_dom(doc);
    rt.eval("document.getElementById('a1').click()").unwrap();
    assert!(bool_eval(&rt, "document.getElementById('d').open === true"));
}

/// A script write to `open` — property, `setAttribute` or
/// `removeAttribute` — is a state change and owes the same event. Before
/// BUG-851 none of the three notified anyone: the shim's only dispatch
/// site was the `click` listener this fix deleted.
#[test]
fn details_script_open_write_fires_toggle() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); var log = []; \
                 d.addEventListener('toggle', function(e) { log.push(e.oldState + '>' + e.newState); }); \
                 d.open = true; _lumen_tick_timers(); \
                 d.removeAttribute('open'); _lumen_tick_timers(); \
                 d.setAttribute('open', ''); _lumen_tick_timers(); \
                 log.join(',') === 'closed>open,open>closed,closed>open'"));
}

/// The event is a queued task, not an inline dispatch — nothing has been
/// delivered by the statement after the write.
#[test]
fn details_toggle_is_queued_not_synchronous() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); var n = 0; \
                 d.addEventListener('toggle', function() { n++; }); \
                 d.open = true; \
                 var duringTurn = n; \
                 _lumen_tick_timers(); \
                 duringTurn === 0 && n === 1"));
}

/// HTML LS «queue a details toggle event task»: a second change before the
/// task runs replaces it instead of queueing another, so the page gets ONE
/// event spanning both — `toggleEvent.html` t2/t6/t8.
#[test]
fn details_two_writes_in_one_turn_fire_one_event() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); var log = []; \
                 d.addEventListener('toggle', function(e) { log.push(e.oldState + '>' + e.newState); }); \
                 d.open = true; d.open = false; \
                 _lumen_tick_timers(); \
                 log.length === 1 && log[0] === 'closed>closed'"));
}

/// Writing the state the element is already in is not a change and owes no
/// event (`toggleEvent.html` t9/t10) — including a rewrite of the content
/// attribute's *value*, which never changes its presence.
#[test]
fn details_no_op_write_fires_nothing() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); var n = 0; \
                 d.addEventListener('toggle', function() { n++; }); \
                 d.open = false; _lumen_tick_timers(); \
                 d.open = true; _lumen_tick_timers(); \
                 d.setAttribute('open', 'open'); d.open = true; _lumen_tick_timers(); \
                 n === 1"));
}

/// The shell flips `open` itself on a native mouse click and then tells the
/// shim what changed: exactly one flip, exactly one event. It used to
/// dispatch a bare `Event('toggle')` while the deleted document listener
/// flipped the attribute back, so a real click opened nothing.
#[test]
fn details_native_toggle_notifies_once() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); var log = []; \
                 d.addEventListener('toggle', function(e) { log.push(e.newState); }); \
                 _lumen_set_attr(d.__nid__, 'open', ''); \
                 _lumen_details_native_toggled(d.__nid__, false); \
                 _lumen_tick_timers(); \
                 log.length === 1 && log[0] === 'open' && d.open === true"));
}

/// HTML LS §4.11.1.1 exclusive accordion — and the sibling it closes gets
/// an `open` → `closed` event of its own, through the same steps.
#[test]
fn details_name_exclusivity_closes_other() {
    let doc = make_details_doc();
    {
        let mut d = doc.lock().unwrap();
        let first = d.find_by_id("d").expect("details");
        let body = d.get(first).parent.expect("body");
        let other = d.create_element(QualName::html("details"));
        for (n, v) in [("id", "d2"), ("name", "grp")] {
            if let NodeData::Element { attrs, .. } = &mut d.get_mut(other).data {
                attrs.push(lumen_dom::Attribute {
                    name: QualName::html(n), value: v.into(),
                });
            }
        }
        d.append_child(body, other);
        if let NodeData::Element { attrs, .. } = &mut d.get_mut(first).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("name"), value: "grp".into(),
            });
        }
    }
    let rt = v8_runtime_with_dom(doc);
    assert!(bool_eval(&rt,
        "var a = document.getElementById('d'), b = document.getElementById('d2'); \
                 var closed = null; \
                 b.open = true; _lumen_tick_timers(); \
                 b.addEventListener('toggle', function(e) { closed = e.oldState + '>' + e.newState; }); \
                 a.open = true; _lumen_tick_timers(); \
                 a.open === true && b.open === false && closed === 'open>closed'"));
}

/// A `<details open>` the parser wrote owes an event nobody queued: the
/// markup never passes through the attribute-write hook. The end-of-parse
/// scan pays it exactly once (`toggleEvent.html` details9).
#[test]
fn details_parser_open_scan_fires_once() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var d = document.getElementById('d'); \
                 d.open = true; _lumen_tick_timers(); \
                 _details_known_open = {}; \
                 var log = []; \
                 d.addEventListener('toggle', function(e) { log.push(e.oldState + '>' + e.newState); }); \
                 _lumen_details_open_scan(); _lumen_tick_timers(); \
                 _lumen_details_open_scan(); _lumen_tick_timers(); \
                 log.length === 1 && log[0] === 'closed>open'"));
}

/// BUG-919: exclusivity (HTML LS §4.11.1.1) is a *connection*-time question,
/// not just a write-time one. Two `<details name=x open>` built detached and
/// only connected to each other via `appendChild(fragment)` never had a
/// chance to conflict at write time — neither was in the document yet — so
/// the first-write-time check found nothing (`details-name-exclusivity-
/// fragment-insertion.html` in WPT).
#[test]
fn details_fragment_insertion_closes_detached_open_sibling() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var container = document.createElement('div'); document.body.appendChild(container); \
                 var frag = document.createDocumentFragment(); \
                 var a = document.createElement('details'); \
                 a.setAttribute('name', 'grp2'); a.setAttribute('open', ''); \
                 var b = document.createElement('details'); \
                 b.setAttribute('name', 'grp2'); b.setAttribute('open', ''); \
                 frag.appendChild(a); frag.appendChild(b); \
                 container.appendChild(frag); \
                 a.hasAttribute('open') === true && b.hasAttribute('open') === false"));
}

/// BUG-919 remainder: `innerHTML =` parses with the native HTML parser
/// directly (never the JS attribute-write hook, exactly like the page's own
/// initial parse), so a `<details open>` it writes owes the same end-of-parse
/// `toggle` the ready-state scan already pays for markup the shell parsed.
#[test]
fn details_inner_html_open_fires_toggle() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var container = document.createElement('div'); document.body.appendChild(container); \
                 container.innerHTML = '<details id=\"fresh\" open></details>'; \
                 var el = document.getElementById('fresh'); \
                 var log = []; \
                 el.addEventListener('toggle', function(e) { log.push(e.oldState + '>' + e.newState); }); \
                 _lumen_tick_timers(); \
                 log.length === 1 && log[0] === 'closed>open' && el.open === true"));
}

/// BUG-919: the virtual document `DOMParser.parseFromString` builds has no
/// native node behind it at all — its own tokenizer's `setAttribute` is a
/// plain JS object write, never `_lumen_set_attr` — so it needs its own copy
/// of the "queue a details toggle event task" step (`dom_parser.rs`'s
/// `_vDetailsOpenScan`). Queued, not synchronous, matching HTML LS and
/// `toggleEvent.html`'s own parser subtest, which attaches `ontoggle` right
/// after `parseFromString` returns.
#[test]
fn dom_parser_details_open_fires_toggle_task_not_sync() {
    let rt = v8_runtime_with_dom(make_details_doc());
    let during = rt
        .eval(
            "var log = null; \
             var doc = new DOMParser().parseFromString('<details open></details>', 'text/html'); \
             var el = doc.querySelector('details'); \
             el.ontoggle = function(e) { log = e.target.open; }; \
             log",
        )
        .unwrap();
    assert_eq!(during, lumen_core::JsValue::Null);
    let after = rt.eval("log").unwrap();
    assert_eq!(after, lumen_core::JsValue::Bool(true));
}

#[test]
fn dialog_show_sets_open() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.show(); \
                 dlg.hasAttribute('open') && dlg.open === true"));
}

#[test]
fn dialog_show_modal_sets_open() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.showModal(); \
                 dlg.hasAttribute('open') && dlg.open === true"));
}

#[test]
fn dialog_close_removes_open() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.show(); \
                 dlg.close(); \
                 !dlg.hasAttribute('open')"));
}

#[test]
fn dialog_close_fires_close_event() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 var got = false; \
                 dlg.addEventListener('close', function() { got = true; }); \
                 dlg.show(); \
                 dlg.close(); \
                 got"));
}

#[test]
fn dialog_return_value() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.show(); \
                 dlg.close('ok'); \
                 dlg.returnValue === 'ok'"));
}

#[test]
fn dialog_escape_key_closes_modal() {
    let rt = v8_runtime_with_dom(make_details_doc());
    rt.eval("document.getElementById('dlg').showModal()").unwrap();
    let root_nid = rt.eval("_lumen_root_nid").unwrap();
    let nid = match root_nid { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
    rt.eval(&format!(
        "_lumen_dispatch_key_event({}, 'keydown', 'Escape', 'Escape', 27, 0, 0, false, false)",
        nid
    )).unwrap();
    assert!(bool_eval(&rt,
        "!document.getElementById('dlg').hasAttribute('open')"));
}

#[test]
fn dialog_escape_cancel_preventable() {
    let rt = v8_runtime_with_dom(make_details_doc());
    rt.eval(
        "document.getElementById('dlg').showModal(); \
                 document.getElementById('dlg').addEventListener('cancel', function(e) { \
                     e.preventDefault(); \
                 });"
    ).unwrap();
    let root_nid = rt.eval("_lumen_root_nid").unwrap();
    let nid = match root_nid { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
    rt.eval(&format!(
        "_lumen_dispatch_key_event({}, 'keydown', 'Escape', 'Escape', 27, 0, 0, false, false)",
        nid
    )).unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('dlg').hasAttribute('open')"));
}

#[test]
fn dialog_request_close_removes_open() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.show(); \
                 dlg.requestClose(); \
                 !dlg.hasAttribute('open')"));
}

#[test]
fn dialog_request_close_fires_cancel_then_close() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 var log = []; \
                 dlg.addEventListener('cancel', function() { log.push('cancel'); }); \
                 dlg.addEventListener('close', function() { log.push('close'); }); \
                 dlg.show(); \
                 dlg.requestClose(); \
                 log.length === 2 && log[0] === 'cancel' && log[1] === 'close'"));
}

#[test]
fn dialog_request_close_sets_return_value() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.show(); \
                 dlg.requestClose('ok'); \
                 dlg.returnValue === 'ok'"));
}

#[test]
fn dialog_request_close_preventable() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 dlg.addEventListener('cancel', function(e) { e.preventDefault(); }); \
                 dlg.show(); \
                 dlg.requestClose(); \
                 dlg.hasAttribute('open')"));
}

#[test]
fn dialog_request_close_noop_when_not_open() {
    let rt = v8_runtime_with_dom(make_details_doc());
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('dlg'); \
                 var got = false; \
                 dlg.addEventListener('cancel', function() { got = true; }); \
                 dlg.requestClose(); \
                 !got"));
}

// ── <dialog> focus management tests (HTML LS §6.6.3) ─────────────────────

fn make_dialog_focus_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let btn   = doc.create_element(QualName::html("button"));
    let dlg   = doc.create_element(QualName::html("dialog"));
    let ok    = doc.create_element(QualName::html("button"));
    let dlg2  = doc.create_element(QualName::html("dialog"));
    let ok2   = doc.create_element(QualName::html("button"));
    set_attribute(&mut doc, btn,  "id", "btn");
    set_attribute(&mut doc, dlg,  "id", "dlg");
    set_attribute(&mut doc, ok,   "id", "ok");
    set_attribute(&mut doc, ok,   "autofocus", "");
    set_attribute(&mut doc, dlg2, "id", "dlg2");
    set_attribute(&mut doc, ok2,  "id", "ok2");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, btn);
    doc.append_child(body, dlg);
    doc.append_child(dlg, ok);
    doc.append_child(body, dlg2);
    doc.append_child(dlg2, ok2);
    Arc::new(Mutex::new(doc))
}

#[test]
fn dialog_show_modal_requests_focus_on_autofocus() {
    let rt = v8_runtime_with_dom(make_dialog_focus_doc());
    rt.eval("document.getElementById('dlg').showModal();").unwrap();
    let reqs = rt.take_focus_requests();
    assert!(!reqs.is_empty(), "showModal should push a focus request");
    assert!(reqs.iter().any(|r| r.is_some()), "focus request should be Some(nid)");
}

#[test]
fn dialog_show_modal_requests_focus_on_dialog_when_no_autofocus() {
    let rt = v8_runtime_with_dom(make_dialog_focus_doc());
    rt.eval("document.getElementById('dlg2').showModal();").unwrap();
    let reqs = rt.take_focus_requests();
    assert!(!reqs.is_empty(), "showModal without autofocus should push a focus request");
    assert!(reqs.iter().any(|r| r.is_some()), "focus request should be Some(dialog_nid)");
}

#[test]
fn dialog_close_requests_blur_when_no_previous_focus() {
    let rt = v8_runtime_with_dom(make_dialog_focus_doc());
    rt.eval("document.getElementById('dlg').showModal();").unwrap();
    let _ = rt.take_focus_requests();
    rt.eval("document.getElementById('dlg').close();").unwrap();
    let reqs = rt.take_focus_requests();
    assert!(!reqs.is_empty(), "close should push a focus request");
    assert!(reqs.iter().any(|r| r.is_none()), "close with no prev focus should push None (blur)");
}

#[test]
fn dialog_close_restores_previous_focus() {
    let rt = v8_runtime_with_dom(make_dialog_focus_doc());
    let btn_nid: i32 = match rt.eval("document.getElementById('btn').__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as i32,
        _ => panic!("btn nid not a number"),
    };
    rt.eval(&format!("_lumen_last_focused_nid = {};", btn_nid)).unwrap();
    rt.eval("document.getElementById('dlg').showModal();").unwrap();
    let _ = rt.take_focus_requests();
    rt.eval("document.getElementById('dlg').close();").unwrap();
    let reqs = rt.take_focus_requests();
    assert!(
        reqs.iter().any(|r| r == &Some(btn_nid as u32)),
        "close should restore focus to the previously focused element"
    );
}

#[test]
fn dialog_last_focused_nid_global_exists() {
    let rt = v8_runtime_with_dom(make_dialog_focus_doc());
    assert!(bool_eval(&rt, "_lumen_last_focused_nid === -1"));
}

// ── <selectlist> tests (Open UI Customizable Select §3, Phase 0) ─────────

fn make_selectlist_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let sl   = doc.create_element(QualName::html("selectlist"));
    let o1   = doc.create_element(QualName::html("option"));
    let o2   = doc.create_element(QualName::html("option"));
    let o3   = doc.create_element(QualName::html("option"));
    fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, k: &str, v: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html(k), value: v.into() });
        }
    }
    fn set_text(doc: &mut Document, nid: lumen_dom::NodeId, text: &str) {
        let t = doc.create_text(text.to_owned());
        doc.append_child(nid, t);
    }
    set_attr(&mut doc, sl, "id", "sl");
    set_attr(&mut doc, o1, "value", "a");
    set_text(&mut doc, o1, "Apple");
    set_attr(&mut doc, o2, "value", "b");
    set_attr(&mut doc, o2, "selected", "");
    set_text(&mut doc, o2, "Banana");
    set_attr(&mut doc, o3, "value", "c");
    set_text(&mut doc, o3, "Cherry");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, sl);
    doc.append_child(sl, o1);
    doc.append_child(sl, o2);
    doc.append_child(sl, o3);
    Arc::new(Mutex::new(doc))
}

#[test]
fn selectlist_options_length() {
    let rt = v8_runtime_with_dom(make_selectlist_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('sl').options.length === 3 && \
                 document.getElementById('sl').length === 3"));
}

#[test]
fn selectlist_selected_index_from_attr() {
    let rt = v8_runtime_with_dom(make_selectlist_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('sl').selectedIndex === 1"));
}

#[test]
fn selectlist_value_from_selected_option() {
    let rt = v8_runtime_with_dom(make_selectlist_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('sl').value === 'b'"));
}

#[test]
fn selectlist_set_value_changes_selected() {
    let rt = v8_runtime_with_dom(make_selectlist_doc());
    assert!(bool_eval(&rt,
        "var sl = document.getElementById('sl'); \
                 sl.value = 'c'; \
                 sl.value === 'c' && sl.selectedIndex === 2"));
}

#[test]
fn selectlist_item_by_index() {
    let rt = v8_runtime_with_dom(make_selectlist_doc());
    assert!(bool_eval(&rt,
        "var sl = document.getElementById('sl'); \
                 sl.item(0) !== null && sl.item(0).getAttribute('value') === 'a' && \
                 sl.item(99) === null"));
}

// ── HTML Popover API tests (WHATWG HTML §6.12) ────────────────────────────

/// Build a document with two popover divs and a trigger button.
fn make_popover_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html  = doc.create_element(QualName::html("html"));
    let body  = doc.create_element(QualName::html("body"));
    let pop1  = doc.create_element(QualName::html("div"));
    let pop2  = doc.create_element(QualName::html("div"));
    let btn   = doc.create_element(QualName::html("button"));
    fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, k: &str, v: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html(k), value: v.into() });
        }
    }
    set_attr(&mut doc, pop1, "id",      "p1");
    set_attr(&mut doc, pop1, "popover", "auto");
    set_attr(&mut doc, pop2, "id",      "p2");
    set_attr(&mut doc, pop2, "popover", "manual");
    set_attr(&mut doc, btn,  "id",      "btn");
    set_attr(&mut doc, btn,  "popovertarget", "p1");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, pop1);
    doc.append_child(body, pop2);
    doc.append_child(body, btn);
    Arc::new(Mutex::new(doc))
}

#[test]
fn popover_property_getter_auto() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    assert!(bool_eval(&rt, "document.getElementById('p1').popover === 'auto'"));
}

#[test]
fn popover_property_getter_manual() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    assert!(bool_eval(&rt, "document.getElementById('p2').popover === 'manual'"));
}

#[test]
fn popover_property_getter_no_attr() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    assert!(bool_eval(&rt, "document.getElementById('btn').popover === null"));
}

#[test]
fn popover_show_sets_open_attr() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval("document.getElementById('p1').showPopover()").unwrap();
    assert!(bool_eval(&rt, "document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn popover_hide_removes_open_attr() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval("var p1 = document.getElementById('p1'); p1.showPopover(); p1.hidePopover()").unwrap();
    assert!(bool_eval(&rt, "!document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn popover_toggle_shows_when_closed() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval("document.getElementById('p1').togglePopover()").unwrap();
    assert!(bool_eval(&rt, "document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn popover_toggle_hides_when_open() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval("var p1 = document.getElementById('p1'); p1.showPopover(); p1.togglePopover()").unwrap();
    assert!(bool_eval(&rt, "!document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn popover_toggle_event_fired() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    assert!(bool_eval(&rt,
        "var evt = null; \
                 document.getElementById('p1').addEventListener('toggle', function(e) { evt = e; }); \
                 document.getElementById('p1').showPopover(); \
                 evt !== null && evt.oldState === 'closed' && evt.newState === 'open'"));
}

#[test]
fn popover_beforetoggle_event_fired() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    assert!(bool_eval(&rt,
        "var evt = null; \
                 document.getElementById('p1').addEventListener('beforetoggle', function(e) { evt = e; }); \
                 document.getElementById('p1').showPopover(); \
                 evt !== null && evt.oldState === 'closed' && evt.newState === 'open'"));
}

#[test]
fn popover_auto_closes_other_auto_on_show() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval(
        "var p1 = document.getElementById('p1'); \
                 p1.showPopover(); \
                 document.getElementById('p2').setAttribute('popover','auto'); \
                 document.getElementById('p2').showPopover();"
    ).unwrap();
    assert!(bool_eval(&rt,
        "!document.getElementById('p1').hasAttribute('data-lumen-popover-open') && \
                 document.getElementById('p2').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn popover_manual_does_not_close_auto() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval("document.getElementById('p1').showPopover(); document.getElementById('p2').showPopover()").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('p1').hasAttribute('data-lumen-popover-open') && \
                 document.getElementById('p2').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn popover_fixed_style_applied_on_show() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval("document.getElementById('p1').showPopover()").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('p1').style.getPropertyValue('position') === 'fixed'"));
}

#[test]
fn popover_style_restored_on_hide() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval(
        "var p = document.getElementById('p1'); \
                 p.style.color = 'red'; \
                 p.showPopover(); \
                 p.hidePopover();"
    ).unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('p1').style.getPropertyValue('position') !== 'fixed'"));
}

#[test]
fn popovertarget_button_shows_popover() {
    let rt = v8_runtime_with_dom(make_popover_doc());
    rt.eval(
        "var btn = document.getElementById('btn'); \
                 _lumen_dispatch_mouse_event(btn.__nid__, 'click', 0, 0, 0, 1, 0);"
    ).unwrap();
    assert!(bool_eval(&rt, "document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
}

// ── popover=hint tests (Popover API Level 2) ──────────────────────────────

fn make_hint_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html  = doc.create_element(QualName::html("html"));
    let body  = doc.create_element(QualName::html("body"));
    let auto_pop = doc.create_element(QualName::html("div"));
    let hint_pop = doc.create_element(QualName::html("div"));
    fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, k: &str, v: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html(k), value: v.into() });
        }
    }
    set_attr(&mut doc, auto_pop, "id",      "auto");
    set_attr(&mut doc, auto_pop, "popover", "auto");
    set_attr(&mut doc, hint_pop, "id",      "hint");
    set_attr(&mut doc, hint_pop, "popover", "hint");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, auto_pop);
    doc.append_child(body, hint_pop);
    Arc::new(Mutex::new(doc))
}

#[test]
fn hint_popover_property_getter() {
    let rt = v8_runtime_with_dom(make_hint_doc());
    assert!(bool_eval(&rt, "document.getElementById('hint').popover === 'hint'"));
}

#[test]
fn hint_show_does_not_close_auto() {
    let rt = v8_runtime_with_dom(make_hint_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   document.getElementById('auto').showPopover(); \
                   document.getElementById('hint').showPopover(); \
                   return document.getElementById('auto').hasAttribute('data-lumen-popover-open') \
                       && document.getElementById('hint').hasAttribute('data-lumen-popover-open'); \
                 })()"));
}

#[test]
fn auto_show_closes_hint() {
    let rt = v8_runtime_with_dom(make_hint_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   document.getElementById('hint').showPopover(); \
                   document.getElementById('auto').showPopover(); \
                   return !document.getElementById('hint').hasAttribute('data-lumen-popover-open') \
                       && document.getElementById('auto').hasAttribute('data-lumen-popover-open'); \
                 })()"));
}

// ── Invoker Commands API (HTML LS §4.10.9, BUG-582) ─────────────────────────

/// Wires up `<button id="btn" commandfor="TARGET" command="COMMAND">` plus a
/// `TARGET` element built by `target_html`, appended to `<body>`.
fn install_invoker(rt: &V8JsRuntime, target_html: &str, command: &str) {
    rt.eval(&format!(
        "document.body.insertAdjacentHTML('beforeend', {:?}); \
         var btn = document.createElement('button'); \
         btn.id = 'btn'; \
         btn.setAttribute('commandfor', 'target'); \
         btn.setAttribute('command', {:?}); \
         document.body.appendChild(btn);",
        target_html, command
    )).unwrap();
}

#[test]
fn command_reflects_known_keyword_case_insensitively() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.createElement('button'); \
                 el.command = 'sHoW-mOdAl'; \
                 el.command === 'show-modal'"));
}

#[test]
fn command_reflects_custom_command_case_preserved() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.command = '--cUsToM'; \
                 el.command === '--cUsToM'"));
}

#[test]
fn command_invalid_value_reflects_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.createElement('button'); \
                 el.command = 'foo-bar'; \
                 el.command === ''"));
}

#[test]
fn command_for_element_resolves_by_id() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<div id='target'></div>", "--x");
    assert!(bool_eval(&rt,
        "document.getElementById('btn').commandForElement === document.getElementById('target')"));
}

#[test]
fn command_for_element_property_overrides_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var btn = document.createElement('button'); \
                 var other = document.createElement('div'); \
                 document.body.appendChild(other); \
                 btn.commandForElement = other; \
                 btn.commandForElement === other && btn.getAttribute('commandfor') === ''"));
}

#[test]
fn command_for_element_rejects_non_element() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var btn = document.createElement('button'); \
                 var threw = false; \
                 try { btn.commandForElement = {}; } catch (e) { threw = e instanceof TypeError; } \
                 threw"));
}

#[test]
fn command_event_constructor_defaults() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var e = new CommandEvent('command'); \
                 e.command === '' && e.source === null && e.type === 'command'"));
}

#[test]
fn command_event_constructor_reads_init() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 var e = new CommandEvent('command', { command: 'close', source: el }); \
                 e.command === 'close' && e.source === el"));
}

#[test]
fn button_type_defaults_to_button_when_commandfor_present() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var btn = document.createElement('button'); \
                 btn.setAttribute('commandfor', 'x'); \
                 btn.type === 'button'"));
}

#[test]
fn button_type_stays_submit_without_commandfor() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "document.createElement('button').type === 'submit'"));
}

#[test]
fn click_fires_command_event_on_target() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<div id='target'></div>", "--custom");
    assert!(bool_eval(&rt,
        "var got = null; \
                 document.getElementById('target').addEventListener('command', function(e) { got = e; }); \
                 document.getElementById('btn').click(); \
                 got instanceof CommandEvent && got.command === '--custom' \
                 && got.source === document.getElementById('btn') \
                 && got.target === document.getElementById('target')"));
}

#[test]
fn click_toggle_popover_command_opens_popover() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<div id='target' popover></div>", "toggle-popover");
    assert!(bool_eval(&rt,
        "document.getElementById('btn').click(); \
                 document.getElementById('target').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn click_show_modal_command_opens_dialog_as_modal() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<dialog id='target'></dialog>", "show-modal");
    assert!(bool_eval(&rt,
        "document.getElementById('btn').click(); \
                 document.getElementById('target').hasAttribute('open')"));
}

#[test]
fn click_show_modal_is_noop_on_already_open_dialog() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<dialog id='target'></dialog>", "show-modal");
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('target'); \
                 dlg.show(); \
                 document.getElementById('btn').click(); \
                 dlg.hasAttribute('open') && !dlg.matches(':modal')"));
}

#[test]
fn click_close_command_closes_dialog_with_value() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<dialog id='target'></dialog>", "close");
    assert!(bool_eval(&rt,
        "var dlg = document.getElementById('target'); \
                 dlg.show(); \
                 document.getElementById('btn').setAttribute('value', 'ok'); \
                 document.getElementById('btn').click(); \
                 !dlg.hasAttribute('open') && dlg.returnValue === 'ok'"));
}

#[test]
fn click_dialog_command_on_non_dialog_target_does_not_fire() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<div id='target'></div>", "close");
    assert!(bool_eval(&rt,
        "var got = false; \
                 document.getElementById('target').addEventListener('command', function() { got = true; }); \
                 document.getElementById('btn').click(); \
                 !got"));
}

#[test]
fn click_command_event_preventable() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<div id='target' popover></div>", "toggle-popover");
    assert!(bool_eval(&rt,
        "document.getElementById('target').addEventListener('command', function(e) { e.preventDefault(); }); \
                 document.getElementById('btn').click(); \
                 !document.getElementById('target').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn click_does_not_fire_when_button_disabled() {
    let rt = v8_runtime_with_dom(make_doc());
    install_invoker(&rt, "<div id='target' popover></div>", "toggle-popover");
    assert!(bool_eval(&rt,
        "document.getElementById('btn').setAttribute('disabled', ''); \
                 document.getElementById('btn').click(); \
                 !document.getElementById('target').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn click_skips_command_for_submit_button_with_form_owner() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.insertAdjacentHTML('beforeend', \
                 '<form id=\"f\"></form><div id=\"target\" popover></div>'); \
                 var btn = document.createElement('button'); \
                 btn.id = 'btn'; \
                 btn.setAttribute('type', 'submit'); \
                 btn.setAttribute('form', 'f'); \
                 btn.setAttribute('commandfor', 'target'); \
                 btn.setAttribute('command', 'toggle-popover'); \
                 document.getElementById('f').appendChild(btn);"
    ).unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('btn').click(); \
                 !document.getElementById('target').hasAttribute('data-lumen-popover-open')"));
}

#[test]
fn oncommand_content_attribute_installs_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var el = document.getElementById('main'); \
                 el.setAttribute('oncommand', 'this.dataset.fired = \"1\"'); \
                 typeof el.oncommand === 'function'"));
}
