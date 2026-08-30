//! Секции `install_dom`: ядро DOM — узлы, дерево, Shadow DOM, выделение, редактирование.
//!
//! Вырезано из `V8JsRuntime::install_dom` батчем SPLIT-JS6 без правки тел:
//! секции жили внутри замыкания `self.run(…)` на отступе 4, то есть ровно на
//! отступе тела функции, а единственной правкой стала приставка контекста у
//! площадок `reg!` — см. [`super::reg`].

use super::reg;
#[allow(unused_imports)]
use super::super::*;

/// `document.documentElement`/`body`/`head` and other document-level reads.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_document_meta(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── document meta ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_document_root", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.root().index() as u32
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_body", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "body").map(|n| n.index() as u32)
        });
        // BUG-703: `document.head` — the sibling of `_lumen_get_body` that the
        // live `document` never had. Same tree scan (first `<head>` in document
        // order, which in a parsed HTML document is the `<head>` child of
        // `<html>`).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_head", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "head").map(|n| n.index() as u32)
        });
        // BUG-281: `document.documentElement` — the `<html>` element, distinct from
        // `_lumen_get_document_root` (the `Document` node itself, `nodeType === 9`).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_html_element", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            doc.document_element().map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_document_title", move || -> String {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "title")
                .map(|nid| collect_text_content(&doc, nid))
                .unwrap_or_default()
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_set_document_title", move |text: String| {
            let mut doc = d.lock().unwrap();
            if let Some(title_id) = find_element_by_tag(&doc, "title") {
                set_text_content(&mut doc, title_id, &text);
            }
        });
        // BUG-358: document-metadata IDL attributes the live `document` never
        // exposed — `characterSet`/`charset`/`inputEncoding` share one native
        // (all three are spec-defined aliases of the same encoding name),
        // `compatMode` reads the tree builder's quirks-mode flag directly off
        // `Document` (no new plumbing — `set_mode` is already called from
        // `lumen-html-parser`'s DOCTYPE handling), `contentType` reads the MIME
        // type the shell stamped on `Document` in `parse_and_layout`.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_document_character_set", move || -> String {
            d.lock().unwrap().character_set().to_string()
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_document_content_type", move || -> String {
            d.lock().unwrap().content_type().to_string()
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_document_compat_mode", move || -> String {
            match d.lock().unwrap().mode() {
                DocumentMode::Quirks => "BackCompat",
                DocumentMode::NoQuirks | DocumentMode::LimitedQuirks => "CSS1Compat",
            }
            .to_string()
        });
    }
    Ok(())
}

/// `document.fonts` (CSS Font Loading `FontFaceSet`).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_document_fonts(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── document.fonts (FontFaceSet) ──────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_fonts_size", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.fonts().size() as u32
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_fonts_get", move |idx: u32| -> Option<String> {
            let doc = d.lock().unwrap();
            doc.fonts().all().get(idx as usize).map(|face| {
                // Serialize FontFace to JSON manually
                let family_esc = face.family.replace('\\', "\\\\").replace('"', "\\\"");
                let style_esc = face.style.replace('\\', "\\\\").replace('"', "\\\"");
                let weight_esc = face.weight.replace('\\', "\\\\").replace('"', "\\\"");
                let stretch_esc = face.stretch.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let unicode_range_esc = face.unicode_range.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let src_esc = face.src.replace('\\', "\\\\").replace('"', "\\\"");
                let status_str = match face.status {
                    lumen_dom::FontFaceStatus::Unloaded => "unloaded",
                    lumen_dom::FontFaceStatus::Loading => "loading",
                    lumen_dom::FontFaceStatus::Loaded => "loaded",
                    lumen_dom::FontFaceStatus::Error => "error",
                };
                format!(
                    r#"{{"family":"{family_esc}","style":"{style_esc}","weight":"{weight_esc}","stretch":{stretch_json},"unicodeRange":{unicode_json},"src":"{src_esc}","status":"{status_str}"}}"#,
                    stretch_json = if face.stretch.is_some() { format!(r#""{}""#, stretch_esc) } else { "null".to_string() },
                    unicode_json = if face.unicode_range.is_some() { format!(r#""{}""#, unicode_range_esc) } else { "null".to_string() }
                )
            })
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_fonts_get_by_family", move |family: String| -> Vec<String> {
            let doc = d.lock().unwrap();
            doc.fonts().get_by_family(&family).iter().map(|face| {
                let family_esc = face.family.replace('\\', "\\\\").replace('"', "\\\"");
                let style_esc = face.style.replace('\\', "\\\\").replace('"', "\\\"");
                let weight_esc = face.weight.replace('\\', "\\\\").replace('"', "\\\"");
                let stretch_esc = face.stretch.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let unicode_range_esc = face.unicode_range.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let src_esc = face.src.replace('\\', "\\\\").replace('"', "\\\"");
                let status_str = match face.status {
                    lumen_dom::FontFaceStatus::Unloaded => "unloaded",
                    lumen_dom::FontFaceStatus::Loading => "loading",
                    lumen_dom::FontFaceStatus::Loaded => "loaded",
                    lumen_dom::FontFaceStatus::Error => "error",
                };
                format!(
                    r#"{{"family":"{family_esc}","style":"{style_esc}","weight":"{weight_esc}","stretch":{stretch_json},"unicodeRange":{unicode_json},"src":"{src_esc}","status":"{status_str}"}}"#,
                    stretch_json = if face.stretch.is_some() { format!(r#""{}""#, stretch_esc) } else { "null".to_string() },
                    unicode_json = if face.unicode_range.is_some() { format!(r#""{}""#, unicode_range_esc) } else { "null".to_string() }
                )
            }).collect()
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_fonts_has_family", move |family: String| -> bool {
            let doc = d.lock().unwrap();
            doc.fonts().has_family(&family)
        });
    }
    Ok(())
}

/// `getElementById`/`querySelector` and the other node-lookup entry points.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_node_lookup(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── node lookup ──────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_element_by_id", move |id: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            find_first_matching(&doc, doc.root(), &|node| {
                matches!(&node.data, NodeData::Element { .. })
                    && node.get_attr("id") == Some(id.as_str())
            })
            .map(|n| n.index() as u32)
        });
        // BUG-391: DOM LS требует `SyntaxError` DOMException на невалидный или
        // неподдерживаемый селектор в `querySelector(All)`/`matches`/`closest`.
        // Селекторные нативы ниже намеренно остаются «прощающими» (их же
        // использует каскад и внутренние помощники шима вроде
        // `getElementsByTagName`, которым бросать нельзя), поэтому проверка
        // вынесена отдельным предикатом — шим зовёт его на публичных входах.
        reg!(scope, ctx, store, "_lumen_selector_is_valid", move |sel: String| -> bool {
            lumen_css_parser::is_valid_selector_list(&sel)
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_query_selector", move |sel: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            query_all(&doc, &sel).into_iter().next().map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_query_selector_all",
            move |sel: String| -> Vec<u32> {
                let doc = d.lock().unwrap();
                query_all(&doc, &sel)
                    .into_iter()
                    .map(|n| n.index() as u32)
                    .collect()
            }
        );
        // BUG-291: Element/DocumentFragment/ShadowRoot.querySelector(All) must be
        // scoped to descendants of the calling node, not the whole document —
        // the unscoped `_lumen_query_selector(_all)` above silently found nothing
        // for subtrees not yet attached to the document (`testharness.js` builds
        // its results table off-document before appending it).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_query_selector_scoped",
            move |node_id: u32, sel: String| -> Option<u32> {
                let doc = d.lock().unwrap();
                let scope = NodeId::from_index(node_id as usize);
                query_all_scoped(&doc, scope, &sel).into_iter().next().map(|n| n.index() as u32)
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_query_selector_all_scoped",
            move |node_id: u32, sel: String| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let scope = NodeId::from_index(node_id as usize);
                query_all_scoped(&doc, scope, &sel)
                    .into_iter()
                    .map(|n| n.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_node_matches_selector",
            move |node_id: u32, sel: String| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches_selector(&doc, nid, &sel)
            }
        );
        // DOM LS §4.2.6: `Element`/`DocumentFragment`/`ShadowRoot` querySelector(All)
        // must search only the calling node's descendants, not the whole document
        // (found while diagnosing P2-wpt S4 — `document.querySelectorAll` above was
        // wrongly reused for these scoped call sites, so a query against a detached
        // subtree — e.g. `testharness.js`'s `render()` template builder — always
        // returned nothing).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_query_selector_scoped",
            move |node_id: u32, sel: String| -> Option<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                query_all_within(&doc, nid, &sel).into_iter().next().map(|n| n.index() as u32)
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_query_selector_all_scoped",
            move |node_id: u32, sel: String| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                query_all_within(&doc, nid, &sel)
                    .into_iter()
                    .map(|n| n.index() as u32)
                    .collect()
            }
        );
    }
    Ok(())
}

/// Node and element property reads and writes (attributes, text, classes).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_node_properties(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── node properties ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_tag_name", move |node_id: u32| -> String {
            let doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            match &doc.get(nid).data {
                NodeData::Element { name, .. } => name.local.to_ascii_uppercase(),
                NodeData::Text(_) => "#text".into(),
                NodeData::Document => "#document".into(),
                NodeData::Comment(_) => "#comment".into(),
                NodeData::Doctype { .. } => "html".into(),
                NodeData::ShadowRoot { .. } => "#shadow-root".into(),
                NodeData::DocumentFragment => "#document-fragment".into(),
            }
        });
        let d = Arc::clone(&doc);
        // BUG-367: DOM LS §4.9 `localName` — the name exactly as stored, i.e.
        // lower-case for HTML (both the parser and `createElement` normalize) and
        // original case for foreign content (`createElementNS('…/svg',
        // 'linearGradient')`). Only `_lumen_get_tag_name` above upper-cases, and
        // only because the shim's tag→interface table is keyed on that form; the
        // shim derives the web-visible `tagName` from this local name plus the
        // namespace. `None` for non-elements, which have no local name at all.
        reg!(scope, ctx, store, 
            "_lumen_get_local_name",
            move |node_id: u32| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                match &doc.get(nid).data {
                    NodeData::Element { name, .. } => Some(name.local.clone()),
                    _ => None,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_is_text_node",
            move |node_id: u32| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches!(doc.get(nid).data, NodeData::Text(_))
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_is_comment_node",
            move |node_id: u32| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches!(doc.get(nid).data, NodeData::Comment(_))
            }
        );
        // BUG-321: DocumentType support (mirrors the rquickjs registration in
        // dom.rs). See there for the rationale.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_is_doctype",
            move |node_id: u32| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches!(doc.get(nid).data, NodeData::Doctype { .. })
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_document_doctype", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            let root = doc.root();
            doc.get(root)
                .children
                .iter()
                .copied()
                .find(|&c| matches!(doc.get(c).data, NodeData::Doctype { .. }))
                .map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_doctype_field",
            move |node_id: u32, which: String| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                match &doc.get(nid).data {
                    NodeData::Doctype {
                        name,
                        public_id,
                        system_id,
                    } => Some(match which.as_str() {
                        "public" => public_id.clone(),
                        "system" => system_id.clone(),
                        _ => name.clone(),
                    }),
                    _ => None,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_namespace_uri",
            move |node_id: u32| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                match &doc.get(nid).data {
                    NodeData::Element { name, .. } => {
                        namespace_uri(name.namespace).map(|s| s.to_string())
                    }
                    _ => None,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_attr",
            move |node_id: u32, name: String| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).get_attr(&name).map(|s| s.to_string())
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_set_attr",
            move |node_id: u32, name: String, value: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                let old = doc.get(nid).get_attr(&name).map(|s| s.to_string());
                set_attribute(&mut doc, nid, &name, &value);
                // BUG-341 S7: only record when the value actually changed —
                // mirrors `lumen_chrome::model::set_attr`'s change-detection
                // (bind_model writes idempotently every cycle regardless of
                // whether the value changed).
                if old.as_deref() != Some(value.as_str()) {
                    record_dom_touch(&touched, nid);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_remove_attr", move |node_id: u32, name: String| {
            let mut doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            let had = doc.get(nid).get_attr(&name).is_some();
            remove_attribute(&mut doc, nid, &name);
            if had {
                record_dom_touch(&touched, nid);
            }
            dirty.store(true, Ordering::Relaxed);
        });
        // ── Form-control runtime value (BUG-441) ────────────────────────────
        // `el.value` is NOT the `value` content attribute: the attribute only
        // seeds the control's value and then stays put as its *default*
        // (HTML LS §4.10.5.5). The current value lives in the document, where
        // layout and form submission read it, so a script assignment reaches
        // the screen and the submitted data instead of dying in a JS shadow.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_dirty_value",
            move |node_id: u32| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.dirty_value(nid).map(|s| s.to_string())
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_set_dirty_value",
            move |node_id: u32, value: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                let changed = doc.dirty_value(nid) != Some(value.as_str());
                doc.set_control_value(nid, value);
                if changed {
                    // The value drives `:placeholder-shown` / `:in-range` and
                    // the painted text — same restyle trigger as an attribute.
                    record_dom_touch(&touched, nid);
                    dirty.store(true, Ordering::Relaxed);
                }
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_clear_dirty_value", move |node_id: u32| {
            let mut doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            if doc.dirty_value(nid).is_some() {
                doc.clear_control_value(nid);
                record_dom_touch(&touched, nid);
                dirty.store(true, Ordering::Relaxed);
            }
        });
        // ── Checkbox/radio runtime checkedness (BUG-444) ────────────────────
        // Same shape as the dirty-value trio above: `el.checked` is NOT the
        // `checked` content attribute — the attribute only seeds the default
        // that `defaultChecked`/`form.reset()` restore (HTML LS §4.10.5.5).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store,
            "_lumen_get_dirty_checked",
            move |node_id: u32| -> Option<bool> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.dirty_checked(nid)
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store,
            "_lumen_set_dirty_checked",
            move |node_id: u32, checked: bool| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                let changed = doc.dirty_checked(nid) != Some(checked);
                doc.set_control_checked(nid, checked);
                if changed {
                    // Drives `:checked`/`:indeterminate` and the painted mark
                    // — same restyle trigger as an attribute change.
                    record_dom_touch(&touched, nid);
                    dirty.store(true, Ordering::Relaxed);
                }
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_clear_dirty_checked", move |node_id: u32| {
            let mut doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            if doc.dirty_checked(nid).is_some() {
                doc.clear_control_checked(nid);
                record_dom_touch(&touched, nid);
                dirty.store(true, Ordering::Relaxed);
            }
        });
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store,
            "_lumen_get_attr_names",
            move |node_id: u32| -> Vec<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                match &doc.get(nid).data {
                    NodeData::Element { attrs, .. } => {
                        attrs.iter().map(|a| a.name.local.to_string()).collect()
                    }
                    _ => Vec::new(),
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_text_content",
            move |node_id: u32| -> String {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_set_text_content",
            move |node_id: u32, text: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &text);
                // BUG-341 S7: record `nid` itself (not just its parent) —
                // a text/childList change here can flip `:empty` for `nid`.
                record_dom_touch(&touched, nid);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_inner_html",
            move |node_id: u32| -> String {
                // BUG-368: real HTML fragment serialization of `nid`'s children
                // (was a Phase-0 stub that returned plain `textContent`).
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                let mut out = String::new();
                serialize_children(&doc, nid, &mut out);
                out
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_set_inner_html",
            move |node_id: u32, html: String| {
                // BUG-368: parse `html` as a fragment and replace `nid`'s children
                // with the result (was a Phase-0 stub that stored `html` verbatim
                // as a single text node — no element/comment structure at all).
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                // HTML LS §4.12.3 / DOM Parsing: у `<template>` разметка уходит
                // в его content-фрагмент, а не в сам элемент. На этом стоит
                // весь класс библиотек, собирающих DOM из шаблонов (Solid, lit,
                // Vue): `t.innerHTML = …; t.content.firstChild.cloneNode(true)`.
                // Пока разметка ложилась в сам элемент, `content` оставался
                // пустым и `firstChild` был null — форма входа id.tbank.ru
                // падала на `Cannot read properties of null (reading
                // 'cloneNode')` (2026-08-17).
                let target = if is_template_element(&doc, nid) {
                    match doc.template_content(nid) {
                        Some(frag) => frag,
                        None => {
                            let frag = doc.create_fragment();
                            doc.set_template_content(nid, frag);
                            frag
                        }
                    }
                } else {
                    nid
                };
                let old_children: Vec<NodeId> = doc.get(target).children.clone();
                for c in old_children {
                    doc.detach(c);
                }
                let new_children = parse_html_fragment(&mut doc, &html);
                for c in new_children {
                    doc.append_child(target, c);
                }
                record_dom_touch(&touched, nid);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_outer_html",
            move |node_id: u32| -> String {
                // BUG-351: serialize `nid` itself (open tag + attrs + children +
                // close tag for elements; escaped data for text/comment).
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                let mut out = String::new();
                serialize_node(&doc, nid, &mut out);
                out
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_parse_html_fragment",
            move |html: String| -> Vec<u32> {
                // BUG-351: parses `html` and imports the result into the live
                // document as new, still-**detached** top-level nodes — callers
                // (outerHTML setter, insertAdjacentHTML) attach them via the
                // existing before/prepend/append/after/replaceWith JS helpers,
                // which already handle dirty/touched bookkeeping.
                let mut doc = d.lock().unwrap();
                parse_html_fragment(&mut doc, &html)
                    .into_iter()
                    .map(|n| n.index() as u32)
                    .collect()
            }
        );
    }
    Ok(())
}

/// Parent/child/sibling traversal.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_tree_navigation(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── tree navigation ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_children",
            move |node_id: u32| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid)
                    .children
                    .iter()
                    .map(|c| c.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_parent",
            move |node_id: u32| -> Option<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).parent.map(|p| p.index() as u32)
            }
        );
    }
    Ok(())
}

/// DOM node count for diagnostics.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_node_count(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── DOM node count ───────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_dom_node_count",
            move || -> u32 {
                d.lock().unwrap().node_count() as u32
            }
        );
    }
    Ok(())
}

/// `appendChild`/`removeChild`/`insertBefore` and friends.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_tree_mutation(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── tree mutation ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_create_element",
            move |tag: String| -> i32 {
                let mut doc = d.lock().unwrap();
                // Returns -1 when MAX_DOM_NODES is reached; JS shim checks `nid < 0`.
                // Must be i32, not u32: `IntoJsReturn for u32` widens via `as f64`,
                // which turns a u32::MAX sentinel into the *positive* 4294967295
                // (unlike the rquickjs FFI, which happened to truncate u32 to a
                // signed 32-bit value) — the shim's `< 0` check would then miss the
                // sentinel and index the arena out of bounds.
                match doc.try_create_element(QualName::html(tag.to_ascii_lowercase())) {
                    Ok(nid) => nid.index() as i32,
                    Err(_) => -1,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_create_element_ns",
            move |ns: String, local: String| -> i32 {
                let mut doc = d.lock().unwrap();
                // Foreign-content namespace selection. SVG keeps the local name's
                // original case (case-sensitive tags like `linearGradient`); the
                // empty string means "no namespace" per DOM §4.5 "validate and
                // extract" (BUG-328, e.g. `createElementNS(null/"", name)` — the
                // JS shim normalizes `null`/`undefined` to `""` before this call),
                // distinct from HTML; any other namespace URI falls back to HTML.
                // Returns -1 on overflow (see `_lumen_create_element` above for
                // why this must be i32, not u32).
                let namespace = if ns == "http://www.w3.org/2000/svg" {
                    Namespace::Svg
                } else if ns.is_empty() {
                    Namespace::None
                } else {
                    Namespace::Html
                };
                match doc.try_create_element(QualName { namespace, local }) {
                    Ok(nid) => nid.index() as i32,
                    Err(_) => -1,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_create_text_node",
            move |text: String| -> i32 {
                let mut doc = d.lock().unwrap();
                // Returns -1 when MAX_DOM_NODES is reached; JS shim checks `nid < 0`
                // (BUG-418: was ungated, letting the arena grow past the limit).
                match doc.try_create_text(text) {
                    Ok(nid) => nid.index() as i32,
                    Err(_) => -1,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_create_comment",
            move |text: String| -> i32 {
                let mut doc = d.lock().unwrap();
                // Returns -1 when MAX_DOM_NODES is reached; JS shim checks `nid < 0`
                // (BUG-418: was ungated, letting the arena grow past the limit).
                match doc.try_create_comment(text) {
                    Ok(nid) => nid.index() as i32,
                    Err(_) => -1,
                }
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_append_child",
            move |parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let parent = NodeId::from_index(parent_id as usize);
                let child = NodeId::from_index(child_id as usize);
                doc.append_child(parent, child);
                // BUG-341 S7: record the container — covers `parent`'s own
                // `:empty`/nth-child-of-its-parent state plus the reconciled
                // children (all within `restyle_root_set_for_node_change`'s
                // parent-subtree invalidation).
                record_dom_touch(&touched, parent);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_remove_child",
            move |_parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                // Read the authoritative parent from the DOM (not the
                // JS-supplied `_parent_id`) before detaching.
                let parent = doc.get(child).parent;
                doc.detach(child);
                if let Some(parent) = parent {
                    record_dom_touch(&touched, parent);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    Ok(())
}

/// Shadow DOM attachment and shadow-tree queries.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_shadow_dom(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── Shadow DOM ───────────────────────────────────────────────────────────────
    // Attaches a new shadow root to `nid` and returns the shadow root NodeId.
    // `mode`: "open" | "closed".  Triggers layout dirty so the composed tree rebuilds.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_attach_shadow", move |nid: u32, mode: String| -> u32 {
            let mut doc = d.lock().unwrap();
            let host = NodeId::from_index(nid as usize);
            let m = if mode == "closed" {
                ShadowRootMode::Closed
            } else {
                ShadowRootMode::Open
            };
            let shadow = doc.attach_shadow(host, m);
            // BUG-341 S7: Shadow DOM attachment changes shadow-tree style
            // scoping in ways not attributable to a simple node set —
            // conservative fallback.
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            shadow.index() as u32
        });
    }
    // Returns the shadow root NodeId for `nid` if the root is Open, else None.
    // Closed roots are intentionally hidden from JS (encapsulation contract).
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_shadow_root", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let host = NodeId::from_index(nid as usize);
            doc.shadow_root_of(host).and_then(|sr| {
                if matches!(
                    doc.get(sr).data,
                    NodeData::ShadowRoot { mode: ShadowRootMode::Open }
                ) {
                    Some(sr.index() as u32)
                } else {
                    None
                }
            })
        });
    }
    // Returns true when `nid` is a shadow-root node (useful for JS wrapper dispatch).
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_is_shadow_root", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            matches!(doc.get(id).data, NodeData::ShadowRoot { .. })
        });
    }
    // Returns true when `nid` is a DocumentFragment node.
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_is_document_fragment", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            matches!(doc.get(id).data, NodeData::DocumentFragment)
        });
    }
    // Allocate a new empty DocumentFragment and return its NodeId.
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_create_fragment", move || -> u32 {
            let mut doc = d.lock().unwrap();
            doc.create_fragment().index() as u32
        });
    }
    // Return the content DocumentFragment NodeId for a <template> element, or None.
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_template_content", move |nid: u32| -> Option<u32> {
            let mut doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            if let Some(frag) = doc.template_content(id) {
                return Some(frag.index() as u32);
            }
            // `<template>`, созданный из JS (`document.createElement`), не
            // проходил через tree-builder и потому не имел content-фрагмента:
            // фрагмент заводится здесь, при первом обращении, и запоминается.
            // Иначе каждый доступ к `.content` отдавал бы новый пустой
            // фрагмент — `t.content !== t.content`, а запись в него терялась.
            if !is_template_element(&doc, id) {
                return None;
            }
            let frag = doc.create_fragment();
            doc.set_template_content(id, frag);
            Some(frag.index() as u32)
        });
    }
    // Deep-clone a subtree rooted at `nid`. Returns the new root NodeId.
    // `deep`: 1 = deep clone (including children), 0 = shallow (node only).
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_clone_subtree", move |nid: u32, deep: u32| -> u32 {
            let mut doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            doc.deep_clone(id, deep != 0).index() as u32
        });
    }
    // Insert `child` immediately before `reference` in `reference`'s parent.
    // Mirrors DOM `insertBefore(child, reference)`.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_insert_before",
            move |_parent_id: u32, child_id: u32, reference_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                let reference = NodeId::from_index(reference_id as usize);
                let parent = doc.get(reference).parent;
                doc.insert_before(child, reference);
                if let Some(parent) = parent {
                    record_dom_touch(&touched, parent);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    // Return the shadow host NodeId for a node inside a shadow tree, or None.
    // Walks ancestors until a ShadowRoot is found, then returns its host.
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_shadow_root_host", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let mut cur = NodeId::from_index(nid as usize);
            loop {
                let node = doc.get(cur);
                if matches!(node.data, NodeData::ShadowRoot { .. }) {
                    return node.parent.map(|h| h.index() as u32);
                }
                {
                    let p = node.parent?;
                    cur = p
                }
            }
        });
    }
    Ok(())
}

/// Selection API (WHATWG Selection API + DOM §4.5).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_selection(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── Selection API (WHATWG Selection API + DOM §4.5) ─────────────────────
    // Exposes document selection state to JavaScript. The Selection object is a
    // singleton per document; Range objects are snapshots of endpoint pairs.
    {
        // Returns [anchor_nid, anchor_offset, focus_nid, focus_offset] or null.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_selection", move || -> Option<Vec<u32>> {
            let doc = d.lock().unwrap();
            let sel = doc.get_selection();
            match (sel.anchor, sel.focus) {
                (Some(a), Some(f)) => Some(vec![
                    a.container.index() as u32,
                    a.offset,
                    f.container.index() as u32,
                    f.offset,
                ]),
                _ => None,
            }
        });
    }
    {
        // Sets selection to [anchor_nid, anchor_offset, focus_nid, focus_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_set_selection",
            move |anchor_nid: u32, anchor_off: u32, focus_nid: u32, focus_off: u32| {
                let mut doc = d.lock().unwrap();
                doc.set_selection(Selection {
                    anchor: Some(DomPosition {
                        container: NodeId::from_index(anchor_nid as usize),
                        offset: anchor_off,
                    }),
                    focus: Some(DomPosition {
                        container: NodeId::from_index(focus_nid as usize),
                        offset: focus_off,
                    }),
                });
                // BUG-341 S7: conservative — no differential test yet proves
                // `::selection` styling is independent of live selection state
                // in this cascade, so a selection change forces a full cascade
                // rather than risk an under-approximated restyle root-set.
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    {
        // Clears the current selection.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_clear_selection", move || {
            let mut doc = d.lock().unwrap();
            doc.set_selection(Selection { anchor: None, focus: None });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
        });
    }
    {
        // Returns text of the current selection.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_selection_text", move || -> String {
            let doc = d.lock().unwrap();
            match doc.get_selection().get_range() {
                Some(r) => range_text(&doc, &r),
                None => String::new(),
            }
        });
    }
    {
        // Returns text covered by the given range endpoints.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_range_text",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> String {
                let doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_index(start_nid as usize),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_index(end_nid as usize),
                        offset: end_off,
                    },
                };
                range_text(&doc, &r)
            }
        );
    }
    {
        // Number of direct DOM children (element offset validation).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_node_child_count", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_child_count(&doc, NodeId::from_index(nid as usize)) as u32
        });
    }
    {
        // DOM-spec "length" of node: char count for text, child count for elements.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_node_length", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_length(&doc, NodeId::from_index(nid as usize)) as u32
        });
    }
    {
        // Text content of a node (node.textContent).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_node_text_content", move |nid: u32| -> String {
            let doc = d.lock().unwrap();
            node_text_content(&doc, NodeId::from_index(nid as usize))
        });
    }
    {
        // Deletes the contents of range; returns [new_pos_nid, new_pos_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_range_delete_contents",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> Vec<u32> {
                let mut doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_index(start_nid as usize),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_index(end_nid as usize),
                        offset: end_off,
                    },
                };
                let pos = lumen_dom::delete_range(&mut doc, &r);
                // BUG-341 S7: arbitrary-range content deletion can remove
                // whole elements — not attributable to a simple node set.
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                vec![pos.container.index() as u32, pos.offset]
            }
        );
    }
    Ok(())
}

/// `contenteditable` mutation bindings (Input Events L2 §4.1).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_contenteditable(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── contenteditable mutation bindings (Input Events Level 2 §4.1) ─────────
    // These are called by the JS shim's _lumen_handle_contenteditable_key()
    // which fires beforeinput → calls here → fires input.
    {
        // True if nid or any ancestor has contenteditable set to a truthy value.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_is_contenteditable", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            lumen_dom::find_editing_host(&doc, NodeId::from_index(nid as usize)).is_some()
        });
    }
    Ok(())
}

/// `document.designMode` and the editing command surface (HTML LS §6.6.3, BUG-353).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_design_mode(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── document.designMode (HTML LS §6.6.3, BUG-353) ──────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_design_mode", move || -> bool {
            d.lock().unwrap().design_mode()
        });
    }
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_set_design_mode", move |enabled: bool| {
            d.lock().unwrap().set_design_mode(enabled);
        });
    }
    {
        // Insert `text` at the current selection (or caret) inside contenteditable.
        // Replaces selected content if the selection is non-collapsed.
        // Returns true on success.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_insert_text", move |text: String| -> bool {
            if text.is_empty() { return false; }
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let Some(anchor) = sel.anchor else { return false; };
            let insert_pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else {
                anchor
            };
            let new_pos = lumen_dom::insert_text_at(&mut doc, insert_pos, &text);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            // BUG-341 S7: text insertion at an arbitrary caret position — not
            // attributable to a simple node set.
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster before the caret (Backspace key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_delete_backward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            // Non-collapsed selection: delete it.
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            if anchor.offset == 0 { return false; }
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            // Walk backward one UTF-8 character boundary.
            let off = anchor.offset as usize;
            let mut prev = off.saturating_sub(1);
            while prev > 0 && !text.is_char_boundary(prev) {
                prev -= 1;
            }
            let r = DomRange {
                start: DomPosition { container: anchor.container, offset: prev as u32 },
                end: anchor,
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster after the caret (Delete key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_delete_forward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            let off = anchor.offset as usize;
            if off >= text.len() { return false; }
            // Walk forward one UTF-8 character boundary.
            let mut next = off + 1;
            while next < text.len() && !text.is_char_boundary(next) {
                next += 1;
            }
            let r = DomRange {
                start: anchor,
                end: DomPosition { container: anchor.container, offset: next as u32 },
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Split the block at the caret position (Enter key in contenteditable).
        // Finds the editing host, then calls insert_paragraph_break.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_insert_paragraph", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else if let Some(p) = sel.anchor {
                p
            } else {
                return false;
            };
            let Some(host) = lumen_dom::find_editing_host(&doc, pos.container) else {
                return false;
            };
            let new_pos = lumen_dom::insert_paragraph_break(&mut doc, pos, host);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // execCommand: bold/italic/underline/insertText/delete/selectAll/copy/cut/paste
        // Returns true if the command was handled.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_exec_command",
            move |cmd: String, value: String| -> bool {
                let mut doc = d.lock().unwrap();
                let sel = doc.get_selection().clone();
                match cmd.as_str() {
                    "selectAll" => {
                        // Select entire document body text
                        if let Some(body) = find_element_by_tag(&doc, "body") {
                            let children = doc.get(body).children.clone();
                            if !children.is_empty() {
                                let first = *children.first().unwrap();
                                let last = *children.last().unwrap();
                                let last_len = node_length(&doc, last);
                                doc.set_selection(Selection {
                                    anchor: Some(DomPosition { container: first, offset: 0 }),
                                    focus: Some(DomPosition {
                                        container: last,
                                        offset: last_len as u32,
                                    }),
                                });
                                record_dom_touch_unattributed(&touched);
                                dirty.store(true, Ordering::Relaxed);
                            }
                        }
                        true
                    }
                    "insertText" => {
                        if let Some(pos) = sel.anchor {
                            // Delete selection first if non-collapsed
                            let pos = sel
                                .get_range()
                                .filter(|r| !r.is_collapsed())
                                .map(|r| lumen_dom::delete_range(&mut doc, &r))
                                .unwrap_or(pos);
                            let new_pos = lumen_dom::insert_text_at(&mut doc, pos, &value);
                            doc.set_selection(Selection {
                                anchor: Some(new_pos),
                                focus: Some(new_pos),
                            });
                            record_dom_touch_unattributed(&touched);
                            dirty.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    "delete" | "forwardDelete" => {
                        if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                            let pos = lumen_dom::delete_range(&mut doc, &r);
                            doc.set_selection(Selection {
                                anchor: Some(pos),
                                focus: Some(pos),
                            });
                            record_dom_touch_unattributed(&touched);
                            dirty.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    // bold/italic/underline: CSSOM inline style toggling (stub — returns true
                    // so editors know the command is accepted; real inline-style mutation
                    // requires Range wrapping which is Phase 3 contenteditable work).
                    "bold" | "italic" | "underline" | "strikeThrough"
                    | "justifyLeft" | "justifyCenter" | "justifyRight" | "justifyFull"
                    | "indent" | "outdent"
                    | "createLink" | "unlink"
                    | "insertOrderedList" | "insertUnorderedList"
                    | "fontName" | "fontSize" | "foreColor" | "backColor"
                    | "removeFormat" => true,
                    // copy/cut/paste: clipboard interaction is handled by the shell;
                    // returning false lets it fall through to native clipboard handling.
                    "copy" | "cut" | "paste" => false,
                    _ => false,
                }
            }
        );
    }
    Ok(())
}
