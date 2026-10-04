//! Тесты MutationObserver / ResizeObserver / IntersectionObserver, вынесенные из
//! `v8_perf_observers` (SPLIT-JS8).

use super::*;
use super::v8_perf_observers::v8_runtime_with_dom;

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
fn attribute_ns_methods_fall_back_to_name_based_for_an_unknown_namespace() {
    // BUG-309, updated by GAP-XMLDOC срез 10 (BUG-685): Lumen only tracks a
    // real `Namespace` for the eleven §13.2.6.5 "adjust foreign attributes"
    // names (xlink:*/xml:*/xmlns*) — an arbitrary caller-supplied namespace
    // URI like `'foo'` has no representation (BUG-830), so it keeps the old
    // name-only behavior rather than becoming unfindable.
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
fn attribute_ns_methods_are_namespace_aware_for_xlink() {
    // GAP-XMLDOC срез 10, BUG-685, BUG-309: `xlink:href` is one of the
    // eleven names §13.2.6.5 gives a real namespace, so the NS-aware
    // accessors must match it by (namespace, local name), not by the
    // qualified `n` argument alone — `getAttributeNS(XLINK_NS, 'href')`
    // finds an attribute stored as `xlink:href`, and a plain `getAttributeNS`
    // call with the wrong namespace does not.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.createElement('a'); \
         _el.setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', '#target');",
    )
    .unwrap();
    assert_eq!(
        rt.eval("_el.getAttributeNS('http://www.w3.org/1999/xlink', 'href')")
            .unwrap(),
        lumen_core::JsValue::String("#target".into())
    );
    assert_eq!(
        rt.eval("_el.hasAttributeNS('http://www.w3.org/1999/xlink', 'href')")
            .unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    // Wrong namespace for the same local name: no match.
    assert_eq!(
        rt.eval("_el.getAttributeNS('http://www.w3.org/2000/svg', 'href')")
            .unwrap(),
        lumen_core::JsValue::Null
    );
    // `Attr.namespaceURI` reflects the real namespace too.
    assert_eq!(
        rt.eval("_el.getAttributeNode('xlink:href').namespaceURI").unwrap(),
        lumen_core::JsValue::String("http://www.w3.org/1999/xlink".into())
    );
    rt.eval("_el.removeAttributeNS('http://www.w3.org/1999/xlink', 'href')")
        .unwrap();
    assert_eq!(
        rt.eval("_el.hasAttribute('xlink:href')").unwrap(),
        lumen_core::JsValue::Bool(false)
    );
}

#[test]
fn parser_built_xlink_href_reports_its_real_namespace_uri() {
    // GAP-XMLDOC срез 10, BUG-685: unlike a script-created attribute, a
    // parser-built `xlink:href` (inside SVG foreign content) gets its
    // namespace from `foreign_content::adjust_foreign_attribute` at parse
    // time — `Attr.namespaceURI` and `getAttributeNS` must see it without
    // any script ever calling `setAttributeNS`.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.innerHTML = '<svg><use xlink:href=\"#a\"></use></svg>'; \
         var _use = document.querySelector('use');",
    )
    .unwrap();
    assert_eq!(
        rt.eval("_use.getAttributeNS('http://www.w3.org/1999/xlink', 'href')")
            .unwrap(),
        lumen_core::JsValue::String("#a".into())
    );
    assert_eq!(
        rt.eval("_use.getAttributeNode('xlink:href').namespaceURI").unwrap(),
        lumen_core::JsValue::String("http://www.w3.org/1999/xlink".into())
    );
}

#[test]
fn plain_attribute_namespace_uri_is_null() {
    // DOM §4.9.2: an attribute the parser never namespaces (`id`, `class`,
    // a plain `href` even on an SVG element) reports `namespaceURI === null`
    // — `Namespace::Html` is not a "real" namespace for an `Attr`, unlike for
    // a `Node` (GAP-XMLDOC срез 10, BUG-685).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _el = document.createElement('div'); _el.setAttribute('id', 'x');")
        .unwrap();
    assert_eq!(
        rt.eval("_el.getAttributeNode('id').namespaceURI").unwrap(),
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
fn get_attribute_names_lists_attributes_in_order() {
    // BUG-1136: Element.prototype.getAttributeNames() (DOM §4.9) was missing,
    // so samsung.com threw `getAttributeNames is not a function`.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _el = document.createElement('div');\
         _el.setAttribute('id', 'd'); _el.setAttribute('class', 'a');\
         _el.setAttribute('data-x', '1'); _el.setAttribute('aria-label', 'z');",
    )
    .unwrap();
    assert_eq!(
        rt.eval("'getAttributeNames' in Element.prototype && Array.isArray(_el.getAttributeNames())")
            .unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    assert_eq!(
        rt.eval("_el.getAttributeNames().join(',')").unwrap(),
        lumen_core::JsValue::String("id,class,data-x,aria-label".into())
    );
    assert_eq!(
        rt.eval("_el.getAttributeNames() !== _el.getAttributeNames()").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    rt.eval("_el.removeAttribute('class');").unwrap();
    assert_eq!(
        rt.eval("_el.getAttributeNames().join(',')").unwrap(),
        lumen_core::JsValue::String("id,data-x,aria-label".into())
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
fn mutation_observer_fires_on_remove_attribute() {
    // BUG-855: `removeAttribute` called `_lumen_remove_attr` past the wrapper
    // that only intercepted attribute *sets*, so a removal queued no record.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_rm_seen = [];
                var obs = new MutationObserver(function(records) {
                    records.forEach(function(r) { _mo_rm_seen.push(r.attributeName); });
                });
                var el = document.getElementById('main');
                el.setAttribute('data-x', '1');
                obs.observe(el, { attributes: true });
                el.removeAttribute('data-x');
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    assert_eq!(rt.eval("_mo_rm_seen.length").unwrap(), lumen_core::JsValue::Number(1.0));
    assert_eq!(
        rt.eval("_mo_rm_seen[0]").unwrap(),
        lumen_core::JsValue::String("data-x".into())
    );
}

#[test]
fn mutation_observer_fires_on_insert_before() {
    // BUG-855: `insertBefore` was never wrapped for MO notification at all —
    // only `appendChild`/`removeChild` were, so a reference-relative
    // insertion (the common form) was silent.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_ib_recs = [];
                var obs = new MutationObserver(function(records) { _mo_ib_recs = records; });
                var p = document.createElement('div');
                var a = document.createElement('span'); a.id = 'a';
                var b = document.createElement('span'); b.id = 'b';
                p.appendChild(a);
                p.appendChild(b);
                document.body.appendChild(p);
                obs.observe(p, { childList: true });
                var c = document.createElement('span'); c.id = 'c';
                p.insertBefore(c, b);
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    assert_eq!(rt.eval("_mo_ib_recs.length").unwrap(), lumen_core::JsValue::Number(1.0));
    assert_eq!(
        rt.eval("_mo_ib_recs[0].addedNodes.length").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
    assert_eq!(
        rt.eval("_mo_ib_recs[0].addedNodes[0].id").unwrap(),
        lumen_core::JsValue::String("c".into())
    );
    // DOM §4.3.3: previousSibling/nextSibling of the inserted node, not the
    // hardcoded `null` the record literal used before this fix.
    assert_eq!(
        rt.eval("_mo_ib_recs[0].previousSibling && _mo_ib_recs[0].previousSibling.id").unwrap(),
        lumen_core::JsValue::String("a".into())
    );
    assert_eq!(
        rt.eval("_mo_ib_recs[0].nextSibling && _mo_ib_recs[0].nextSibling.id").unwrap(),
        lumen_core::JsValue::String("b".into())
    );
}

#[test]
fn mutation_observer_replace_child_fires_one_combined_record() {
    // BUG-855: `replaceChild` is implemented as insert-then-remove; once both
    // natives are wrapped for MO, a naive re-wrap fires TWO records instead
    // of the one combined `+added -removed` record DOM §4.2.4 "replace"
    // describes (WPT MutationObserver-childList.html, "replacement mutation").
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _mo_rc_recs = [];
                var obs = new MutationObserver(function(records) { _mo_rc_recs = records; });
                var p = document.createElement('div');
                var oldChild = document.createElement('span'); oldChild.id = 'old';
                p.appendChild(oldChild);
                document.body.appendChild(p);
                obs.observe(p, { childList: true });
                var newChild = document.createElement('i'); newChild.id = 'new';
                p.replaceChild(newChild, oldChild);
            "#).unwrap();
    rt.eval("_lumen_flush_mutation_observers()").unwrap();
    assert_eq!(rt.eval("_mo_rc_recs.length").unwrap(), lumen_core::JsValue::Number(1.0));
    assert_eq!(
        rt.eval("_mo_rc_recs[0].addedNodes.length").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
    assert_eq!(
        rt.eval("_mo_rc_recs[0].addedNodes[0].id").unwrap(),
        lumen_core::JsValue::String("new".into())
    );
    assert_eq!(
        rt.eval("_mo_rc_recs[0].removedNodes.length").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
    assert_eq!(
        rt.eval("_mo_rc_recs[0].removedNodes[0].id").unwrap(),
        lumen_core::JsValue::String("old".into())
    );
}

#[test]
fn mutation_observer_observe_throws_without_any_kind_requested() {
    // DOM §4.3.1 step 3: `observe()` with none of childList/attributes/
    // characterData (after the OldValue/Filter implications) is a TypeError.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "try { new MutationObserver(function(){}).observe(document.body, {}); false } \
             catch (e) { e instanceof TypeError }"
        ).unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    // attributeOldValue/characterDataOldValue imply their flag — no throw.
    assert_eq!(
        rt.eval(
            "try { new MutationObserver(function(){}).observe(document.body, \
             { characterDataOldValue: true }); true } catch (e) { false }"
        ).unwrap(),
        lumen_core::JsValue::Bool(true)
    );
}

#[test]
fn mutation_observer_constructor_throws_without_callback() {
    // DOM §4.3.1: the callback argument is mandatory.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval("try { new MutationObserver(); false } catch (e) { e instanceof TypeError }")
            .unwrap(),
        lumen_core::JsValue::Bool(true)
    );
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
    // BUG-1003: a second delivery inside the same frame is held back; the next
    // frame's rAF batch is what reports it.
    rt.eval("_lumen_run_raf_callbacks(0); _lumen_deliver_resize_observers()").unwrap();
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

/// BUG-1056: with an rAF pending, the first delivery is not taken by the
/// timer task ahead of it — the frame runs rAF callbacks, then the observer.
#[test]
fn resize_observer_first_delivery_follows_raf_in_same_frame() {
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
                var _ro_ord = [];
                new ResizeObserver(function() { _ro_ord.push('ro'); }).observe(document.body);
                requestAnimationFrame(function() { _ro_ord.push('raf'); });
            "#,
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("_ro_ord.join()").unwrap(),
        lumen_core::JsValue::String("".into()),
        "the timer task must leave the delivery to the frame"
    );
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    assert_eq!(rt.eval("_ro_ord.join()").unwrap(), lumen_core::JsValue::String("raf,ro".into()));
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
                _lumen_run_raf_callbacks(0);
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

/// BUG-1056: the shell's post-relayout delivery must not report ahead of the
/// rAF callbacks queued for the same frame.
#[test]
fn resize_observer_layout_delivery_follows_raf_in_same_frame() {
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
                var _ro_ord = [];
                new ResizeObserver(function() { _ro_ord.push('ro'); }).observe(document.body);
                requestAnimationFrame(function() { _ro_ord.push('raf'); });
                _lumen_deliver_resize_observers_layout();
            "#,
    )
    .unwrap();
    assert_eq!(rt.eval("_ro_ord.join()").unwrap(), lumen_core::JsValue::String("".into()));
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    assert_eq!(rt.eval("_ro_ord.join()").unwrap(), lumen_core::JsValue::String("raf,ro".into()));
}
