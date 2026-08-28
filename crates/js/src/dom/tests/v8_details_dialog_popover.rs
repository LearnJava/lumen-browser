//! Тесты `v8_details_dialog_popover`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

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
