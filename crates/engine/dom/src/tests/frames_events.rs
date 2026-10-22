//! Тесты collect_iframes, check_popup_gate, DOM-снимка (T3 hibernation),
//! IME composition events и Event.isTrusted.

use super::*;

// ── collect_iframes ───────────────────────────────────────────────────────

fn make_iframe(sandbox: Option<&str>, src: Option<&str>) -> Document {
    let mut doc = Document::new();
    let iframe = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe).data {
        if let Some(s) = sandbox {
            attrs.push(Attribute { name: QualName::html("sandbox"), value: s.to_string() });
        }
        if let Some(s) = src {
            attrs.push(Attribute { name: QualName::html("src"), value: s.to_string() });
        }
    }
    doc.append_child(doc.root(), iframe);
    doc
}

fn append_with_attrs(doc: &mut Document, tag: &str, pairs: &[(&str, &str)]) -> NodeId {
    let el = doc.create_element(QualName::html(tag));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(el).data {
        for (k, v) in pairs {
            attrs.push(Attribute { name: QualName::html(*k), value: (*v).to_string() });
        }
    }
    doc.append_child(doc.root(), el);
    el
}

/// OBJECT-1 срез 2: `<object data>`/`<embed src>` — кандидаты во вложенный
/// документ; `type="image/…"`, пустой адрес и вердикт «не документ» —
/// нет. `type="image/svg+xml"` — XML-тип, кандидат (срез 5).
#[test]
fn collect_iframes_offers_object_and_embed_as_embedded_hosts() {
    let mut doc = Document::new();
    let obj = append_with_attrs(&mut doc, "object", &[("data", "a.html"), ("name", "o")]);
    let embed = append_with_attrs(&mut doc, "embed", &[("src", "b.txt")]);
    let svg = append_with_attrs(&mut doc, "object", &[("data", "i.svg"), ("type", "image/svg+xml")]);
    append_with_attrs(&mut doc, "object", &[("data", "i.png"), ("type", "image/png")]);
    append_with_attrs(&mut doc, "object", &[("data", "  ")]);
    append_with_attrs(&mut doc, "embed", &[("data", "wrong-attr.html")]);
    let frames = collect_iframes(&doc);
    let got: Vec<(NodeId, Option<&str>, bool)> =
        frames.iter().map(|f| (f.node, f.src.as_deref(), f.embedded)).collect();
    assert_eq!(
        got,
        vec![(obj, Some("a.html"), true), (embed, Some("b.txt"), true), (svg, Some("i.svg"), true)]
    );
    assert_eq!(frames[0].name.as_deref(), Some("o"));

    assert!(doc.set_embedded_document(embed, "b.txt", false));
    assert!(!doc.set_embedded_document(embed, "b.txt", false), "тот же вердикт — не изменение");
    doc.set_embedded_document(svg, "i.svg", false);
    let nodes: Vec<NodeId> = collect_iframes(&doc).iter().map(|f| f.node).collect();
    assert_eq!(nodes, vec![obj], "«не документ» больше не предлагается");
}

/// Вердикт привязан к адресу: смена `data` делает его устаревшим.
#[test]
fn embedded_document_verdict_is_stale_for_another_url() {
    let mut doc = Document::new();
    let obj = append_with_attrs(&mut doc, "object", &[("data", "a.html")]);
    assert_eq!(doc.embedded_document(obj, "a.html"), None);
    doc.set_embedded_document(obj, "a.html", true);
    assert_eq!(doc.embedded_document(obj, "a.html"), Some(true));
    assert_eq!(doc.embedded_document(obj, "b.html"), None);
    doc.reclaim_dead_nodes(&[obj]);
    assert_eq!(doc.embedded_document(obj, "a.html"), None);
}

#[test]
fn collect_iframes_empty_document() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), div);
    assert!(collect_iframes(&doc).is_empty());
}

#[test]
fn collect_iframes_finds_iframe_without_sandbox() {
    let doc = make_iframe(None, Some("https://example.com"));
    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].src.as_deref(), Some("https://example.com"));
    assert!(!frames[0].is_sandboxed);
    assert!(frames[0].sandbox.is_empty());
}

/// BUG-854: `<frame>` — такой же хост вложенного browsing context.
/// Строится не через `make_iframe`, чтобы тег был виден в самом тесте.
fn make_frame(attrs_in: &[(&str, &str)]) -> Document {
    let mut doc = Document::new();
    let frame = doc.create_element(QualName::html("frame"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(frame).data {
        for (k, v) in attrs_in {
            attrs.push(Attribute { name: QualName::html(*k), value: (*v).to_string() });
        }
    }
    doc.append_child(doc.root(), frame);
    doc
}

#[test]
fn collect_iframes_finds_frame_element() {
    let doc = make_frame(&[("src", "child.html"), ("name", "f1")]);
    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 1, "<frame> is a nested browsing context host too");
    assert_eq!(frames[0].src.as_deref(), Some("child.html"));
    assert_eq!(frames[0].name.as_deref(), Some("f1"));
    assert!(!frames[0].is_sandboxed);
}

#[test]
fn collect_iframes_frame_srcdoc_is_not_a_source() {
    // `srcdoc` объявлен только у `<iframe>` — на `<frame>` это обычный
    // неизвестный атрибут, и брать его как источник нельзя.
    let doc = make_frame(&[("srcdoc", "<p>inline"), ("src", "child.html")]);
    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].srcdoc, None);
    assert_eq!(frames[0].src.as_deref(), Some("child.html"));
}

#[test]
fn collect_iframes_sandboxed_empty_attr_all_restrictions() {
    let doc = make_iframe(Some(""), Some("page.html"));
    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 1);
    assert!(frames[0].is_sandboxed);
    assert_eq!(frames[0].sandbox, SandboxFlags::all_restrictions());
    assert!(frames[0].sandbox.contains(SandboxFlags::SCRIPTS));
    assert!(frames[0].sandbox.contains(SandboxFlags::FORMS));
    assert!(frames[0].sandbox.contains(SandboxFlags::AUXILIARY_NAVIGATION));
}

#[test]
fn collect_iframes_allow_scripts_lifts_scripts_flag() {
    let doc = make_iframe(Some("allow-scripts"), Some("a.html"));
    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 1);
    assert!(frames[0].is_sandboxed);
    assert!(!frames[0].sandbox.contains(SandboxFlags::SCRIPTS));
    assert!(frames[0].sandbox.contains(SandboxFlags::FORMS));
}

#[test]
fn collect_iframes_loading_and_fetchpriority() {
    let mut doc = Document::new();
    // iframe1: loading="lazy", fetchpriority="high"
    let iframe1 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe1).data {
        attrs.push(Attribute { name: QualName::html("src"), value: "a.html".to_string() });
        attrs.push(Attribute { name: QualName::html("loading"), value: "LAZY".to_string() });
        attrs.push(Attribute { name: QualName::html("fetchpriority"), value: "HIGH".to_string() });
    }
    // iframe2: loading="eager", fetchpriority="low"
    let iframe2 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe2).data {
        attrs.push(Attribute { name: QualName::html("src"), value: "b.html".to_string() });
        attrs.push(Attribute { name: QualName::html("loading"), value: "eager".to_string() });
        attrs.push(Attribute { name: QualName::html("fetchpriority"), value: "low".to_string() });
    }
    // iframe3: fetchpriority="auto" → None, без loading
    let iframe3 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe3).data {
        attrs.push(Attribute { name: QualName::html("src"), value: "c.html".to_string() });
        attrs.push(Attribute { name: QualName::html("fetchpriority"), value: "auto".to_string() });
    }
    doc.append_child(doc.root(), iframe1);
    doc.append_child(doc.root(), iframe2);
    doc.append_child(doc.root(), iframe3);
    let iframes = collect_iframes(&doc);
    assert_eq!(iframes.len(), 3);
    assert!(iframes[0].loading_lazy, "loading=LAZY (case-insensitive) must set loading_lazy");
    assert_eq!(iframes[0].fetch_priority, Some("high".to_string()));
    assert!(!iframes[1].loading_lazy, "loading=eager must not set loading_lazy");
    assert_eq!(iframes[1].fetch_priority, Some("low".to_string()));
    assert!(!iframes[2].loading_lazy, "absent loading must not set loading_lazy");
    assert_eq!(iframes[2].fetch_priority, None, "fetchpriority=auto must map to None");
}

/// GAP-REFERRER срез 6: `referrerpolicy` — сырое значение атрибута,
/// парсится/применяется вызывающей стороной (`lumen-shell`), которая
/// одна зависит от `lumen-network`.
#[test]
fn collect_iframes_reads_referrerpolicy_attribute() {
    let mut doc = Document::new();
    let iframe1 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe1).data {
        attrs.push(Attribute { name: QualName::html("src"), value: "a.html".to_string() });
        attrs.push(Attribute {
            name: QualName::html("referrerpolicy"),
            value: "no-referrer".to_string(),
        });
    }
    let iframe2 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe2).data {
        attrs.push(Attribute { name: QualName::html("src"), value: "b.html".to_string() });
    }
    doc.append_child(doc.root(), iframe1);
    doc.append_child(doc.root(), iframe2);
    let iframes = collect_iframes(&doc);
    assert_eq!(iframes[0].referrer_policy, Some("no-referrer".to_string()));
    assert_eq!(iframes[1].referrer_policy, None, "absent attribute must be None");
}

#[test]
fn collect_iframes_multiple_iframes() {
    let mut doc = Document::new();
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(doc.root(), body);

    // iframe 1: no sandbox
    let f1 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f1).data {
        attrs.push(Attribute { name: QualName::html("src"), value: "a.html".to_string() });
    }
    doc.append_child(body, f1);

    // iframe 2: allow-scripts allow-forms
    let f2 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f2).data {
        attrs.push(Attribute {
            name: QualName::html("sandbox"),
            value: "allow-scripts allow-forms".to_string(),
        });
        attrs.push(Attribute { name: QualName::html("src"), value: "b.html".to_string() });
    }
    doc.append_child(body, f2);

    // iframe 3: sandbox="" (all restrictions)
    let f3 = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f3).data {
        attrs.push(Attribute { name: QualName::html("sandbox"), value: String::new() });
        attrs.push(Attribute { name: QualName::html("src"), value: "c.html".to_string() });
    }
    doc.append_child(body, f3);

    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 3);
    assert!(!frames[0].is_sandboxed);
    assert!(frames[1].is_sandboxed);
    assert!(!frames[1].sandbox.contains(SandboxFlags::SCRIPTS));
    assert!(!frames[1].sandbox.contains(SandboxFlags::FORMS));
    assert!(frames[2].is_sandboxed);
    assert_eq!(frames[2].sandbox, SandboxFlags::all_restrictions());
}

#[test]
fn collect_iframes_srcdoc_attribute() {
    let mut doc = Document::new();
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(doc.root(), body);

    let f = doc.create_element(QualName::html("iframe"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f).data {
        attrs.push(Attribute {
            name: QualName::html("srcdoc"),
            value: "<p>hello</p>".to_string(),
        });
    }
    doc.append_child(body, f);

    let frames = collect_iframes(&doc);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].srcdoc.as_deref(), Some("<p>hello</p>"));
    assert!(frames[0].src.is_none());
}

// ── check_popup_gate ──────────────────────────────────────────────────────

#[test]
fn popup_gate_blocked_when_auxiliary_navigation_set() {
    assert!(check_popup_gate(SandboxFlags::AUXILIARY_NAVIGATION));
}

#[test]
fn popup_gate_allowed_when_flag_not_set() {
    assert!(!check_popup_gate(SandboxFlags::empty()));
}

#[test]
fn popup_gate_blocked_when_all_restrictions() {
    assert!(check_popup_gate(SandboxFlags::all_restrictions()));
}

#[test]
fn popup_gate_allowed_after_allow_popups() {
    let flags = lumen_core::parse_sandbox_value(Some("allow-popups"));
    assert!(!check_popup_gate(flags));
}

// ── DOM snapshot (T3 hibernation) ─────────────────────────────────────────

#[test]
fn snapshot_empty_document_roundtrip() {
    let doc = Document::new();
    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");
    assert_eq!(restored.mode(), doc.mode());
    assert_eq!(restored.root(), doc.root());
}

#[test]
fn snapshot_document_with_elements_roundtrip() {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    doc.append_child(doc.root(), html);
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(html, body);
    let text = doc.create_text("hello world");
    doc.append_child(body, text);
    doc.set_mode(DocumentMode::NoQuirks);

    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");

    assert_eq!(restored.mode(), DocumentMode::NoQuirks);
    let root_children = restored.get(restored.root()).children.clone();
    assert_eq!(root_children.len(), 1);
    let html_id = root_children[0];
    let html_children = restored.get(html_id).children.clone();
    assert_eq!(html_children.len(), 1);
    let body_id = html_children[0];
    let body_children = restored.get(body_id).children.clone();
    assert_eq!(body_children.len(), 1);
    let text_id = body_children[0];
    assert!(matches!(&restored.get(text_id).data, NodeData::Text(s) if s == "hello world"));
}

#[test]
fn snapshot_document_with_attributes_roundtrip() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    // Manually push attributes via NodeData::Element.
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
        attrs.push(Attribute { name: QualName::html("class"), value: "container".into() });
        attrs.push(Attribute { name: QualName::html("id"), value: "main".into() });
    }
    doc.append_child(doc.root(), div);

    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");

    let div_id = restored.get(restored.root()).children[0];
    assert_eq!(restored.get(div_id).get_attr("class"), Some("container"));
    assert_eq!(restored.get(div_id).get_attr("id"), Some("main"));
}

#[test]
fn snapshot_document_with_shadow_root_roundtrip() {
    let mut doc = Document::new();
    let host = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), host);
    let shadow = doc.attach_shadow(host, ShadowRootMode::Open);
    let span = doc.create_element(QualName::html("span"));
    doc.append_child(shadow, span);

    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");

    let host_id = restored.get(restored.root()).children[0];
    let shadow_id = restored.shadow_root_of(host_id).expect("shadow root");
    let shadow_children = &restored.get(shadow_id).children;
    assert_eq!(shadow_children.len(), 1);
}

#[test]
fn snapshot_quirks_mode_preserved() {
    let mut doc = Document::new();
    doc.set_mode(DocumentMode::Quirks);
    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");
    assert_eq!(restored.mode(), DocumentMode::Quirks);
}

#[test]
fn snapshot_selection_preserved() {
    let mut doc = Document::new();
    let text = doc.create_text("abcdef");
    doc.append_child(doc.root(), text);
    let sel = Selection {
        anchor: Some(DomPosition { container: text, offset: 0 }),
        focus: Some(DomPosition { container: text, offset: 3 }),
    };
    doc.set_selection(sel.clone());

    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");
    assert_eq!(restored.get_selection(), &sel);
}

#[test]
fn snapshot_blob_is_compact() {
    // Ensure the snapshot is a reasonable size (not accidentally inflated).
    let mut doc = Document::new();
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(doc.root(), body);
    let text = doc.create_text("hello");
    doc.append_child(body, text);
    let bytes = doc.to_bytes().expect("encode");
    // A 3-node tree should serialize well under 1 KB.
    assert!(bytes.len() < 1024, "snapshot too large: {} bytes", bytes.len());
}

// ──────────────────────────────────────────────────────────────────────────
// IME Composition Events tests
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn composition_event_type_as_str() {
    assert_eq!(CompositionEventType::Start.as_str(), "compositionstart");
    assert_eq!(CompositionEventType::Update.as_str(), "compositionupdate");
    assert_eq!(CompositionEventType::End.as_str(), "compositionend");
}

#[test]
fn composition_event_constructors() {
    let start = CompositionEvent::start("あ".to_string(), Some("ja".to_string()));
    assert_eq!(start.event_type, CompositionEventType::Start);
    assert_eq!(start.data.data, "あ");
    assert_eq!(start.data.locale, Some("ja".to_string()));
    assert_eq!(start.data.range, None);

    let update = CompositionEvent::update("あい".to_string(), Some((0, 2)));
    assert_eq!(update.event_type, CompositionEventType::Update);
    assert_eq!(update.data.data, "あい");
    assert_eq!(update.data.locale, None);
    assert_eq!(update.data.range, Some((0, 2)));

    let end = CompositionEvent::end("あいう".to_string());
    assert_eq!(end.event_type, CompositionEventType::End);
    assert_eq!(end.data.data, "あいう");
    assert_eq!(end.data.locale, None);
    assert_eq!(end.data.range, None);
}

// ──────────────────────────────────────────────────────────────────────────
// Event.isTrusted (DOM §2.10) tests
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn composition_event_helper_constructors_are_trusted() {
    // start/update/end are used by the native IME pipeline; they MUST default
    // to trusted so Cloudflare/DataDome-style isTrusted gates pass for real
    // user keystrokes routed through winit Ime::Preedit/Ime::Commit.
    assert!(CompositionEvent::start("a".into(), None).is_trusted);
    assert!(CompositionEvent::update("ab".into(), Some((0, 2))).is_trusted);
    assert!(CompositionEvent::end("abc".into()).is_trusted);
}

#[test]
fn composition_event_new_is_trusted() {
    // `CompositionEvent::new` is the native-pipeline constructor; it MUST
    // default to trusted. Script-synthesized events go through `untrusted`.
    let data = CompositionData {
        data: "x".to_string(),
        locale: None,
        range: None,
    };
    let evt = CompositionEvent::new(CompositionEventType::Update, data);
    assert!(evt.is_trusted, "CompositionEvent::new must default to trusted");
}

#[test]
fn composition_event_untrusted_constructor() {
    // Page-script-synthesized events (`new CompositionEvent(...)` in JS)
    // are never trusted per DOM §2.10.
    let data = CompositionData {
        data: "x".to_string(),
        locale: None,
        range: None,
    };
    let evt = CompositionEvent::untrusted(CompositionEventType::Start, data);
    assert!(!evt.is_trusted);
    assert_eq!(evt.event_type, CompositionEventType::Start);
}

#[test]
fn input_event_trusted_constructor() {
    let evt = InputEvent::trusted(EditInputType::InsertText, Some("a".into()), false);
    assert!(evt.is_trusted);
    assert_eq!(evt.input_type, EditInputType::InsertText);
    assert_eq!(evt.data.as_deref(), Some("a"));
    assert!(!evt.is_composing);
}

#[test]
fn input_event_untrusted_constructor() {
    // Script-synthesized `new InputEvent('input', { ... })` events must
    // never be trusted, regardless of the `inputType`.
    let evt = InputEvent::untrusted(EditInputType::DeleteContentBackward, None, false);
    assert!(!evt.is_trusted);
    assert_eq!(evt.input_type, EditInputType::DeleteContentBackward);
}

#[test]
fn input_event_is_trusted_independent_of_is_composing() {
    // is_trusted (provenance) is orthogonal to is_composing (IME state):
    // a real user IME keystroke is both trusted AND composing.
    let real_ime = InputEvent::trusted(EditInputType::InsertText, Some("あ".into()), true);
    assert!(real_ime.is_trusted);
    assert!(real_ime.is_composing);

    // A script-dispatched InputEvent can also carry is_composing=true; the
    // flags must not be conflated.
    let script_ime = InputEvent::untrusted(EditInputType::InsertText, Some("あ".into()), true);
    assert!(!script_ime.is_trusted);
    assert!(script_ime.is_composing);
}

#[test]
fn input_event_clone_preserves_is_trusted() {
    // Cloning must preserve trustedness — the bit is part of event identity
    // per DOM §2.10 (Event.isTrusted is set at construction, never mutated).
    let trusted = InputEvent::trusted(EditInputType::InsertText, Some("x".into()), false);
    let copy = trusted.clone();
    assert!(copy.is_trusted);

    let untrusted = InputEvent::untrusted(EditInputType::InsertText, Some("x".into()), false);
    let copy = untrusted.clone();
    assert!(!copy.is_trusted);
}

#[test]
fn composition_event_clone_preserves_is_trusted() {
    let trusted = CompositionEvent::start("a".into(), None);
    assert!(trusted.clone().is_trusted);

    let data = CompositionData {
        data: "a".to_string(),
        locale: None,
        range: None,
    };
    let untrusted = CompositionEvent::untrusted(CompositionEventType::Start, data);
    assert!(!untrusted.clone().is_trusted);
}

#[test]
fn document_begin_composition() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    doc.append_child(doc.root(), input);

    // No composition initially
    assert!(doc.get_composition().is_none());

    // Begin composition
    doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));
    let comp = doc.get_composition();
    assert!(comp.is_some());
    let comp = comp.unwrap();
    assert_eq!(comp.node, input);
    assert_eq!(comp.text, "あ");
    assert_eq!(comp.locale, Some("ja".to_string()));
    assert_eq!(comp.selection, None);
}

#[test]
fn document_update_composition() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));

    // Update with new preedit and selection
    doc.update_composition("あい".to_string(), Some((0, 2)));
    let comp = doc.get_composition().unwrap();
    assert_eq!(comp.text, "あい");
    assert_eq!(comp.selection, Some((0, 2)));
    // Locale should remain unchanged
    assert_eq!(comp.locale, Some("ja".to_string()));
}

#[test]
fn document_update_composition_no_active() {
    let mut doc = Document::new();
    // Updating without active composition should be a no-op
    doc.update_composition("text".to_string(), Some((0, 4)));
    assert!(doc.get_composition().is_none());
}

#[test]
fn document_end_composition() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));

    // End composition returns the state
    let ended = doc.end_composition();
    assert!(ended.is_some());
    let ended = ended.unwrap();
    assert_eq!(ended.node, input);
    assert_eq!(ended.text, "あ");

    // Composition should now be None
    assert!(doc.get_composition().is_none());
}

#[test]
fn document_end_composition_no_active() {
    let mut doc = Document::new();
    // Ending without active composition should return None
    assert!(doc.end_composition().is_none());
}

#[test]
fn document_composition_sequence() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));

    // Simulates a full IME composition sequence (Japanese input).
    // User wants to type "こんにちは" (konnichiha).

    // 1. Start: User types first key
    doc.begin_composition(input, "こ".to_string(), Some("ja".to_string()));
    assert_eq!(doc.get_composition().unwrap().text, "こ");

    // 2. Update: User continues typing
    doc.update_composition("こん".to_string(), Some((0, 2)));
    assert_eq!(doc.get_composition().unwrap().text, "こん");

    doc.update_composition("こんに".to_string(), Some((0, 3)));
    assert_eq!(doc.get_composition().unwrap().text, "こんに");

    // 3. End: User commits the input
    let final_state = doc.end_composition();
    assert!(final_state.is_some());
    assert_eq!(final_state.unwrap().text, "こんに");

    // Composition is now cleared
    assert!(doc.get_composition().is_none());
}

#[test]
fn composition_state_snapshot_roundtrip() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    doc.append_child(doc.root(), input);
    doc.begin_composition(input, "test".to_string(), Some("en".to_string()));

    // Serialize and deserialize
    let bytes = doc.to_bytes().expect("encode");
    let restored = Document::from_bytes(&bytes).expect("decode");

    // Composition state should be preserved
    let restored_comp = restored.get_composition();
    assert!(restored_comp.is_some());
    let restored_comp = restored_comp.unwrap();
    assert_eq!(restored_comp.text, "test");
    assert_eq!(restored_comp.locale, Some("en".to_string()));
}

#[test]
fn composition_helper_is_composing() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));

    // Not composing initially
    assert!(!doc.is_composing());

    // Begin composition
    doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));
    assert!(doc.is_composing());

    // End composition
    doc.end_composition();
    assert!(!doc.is_composing());
}

#[test]
fn composition_helper_get_range() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));

    // No range initially
    assert!(doc.get_composition_range().is_none());

    // Begin composition without range
    doc.begin_composition(input, "a".to_string(), None);
    assert!(doc.get_composition_range().is_none());

    // Update with range
    doc.update_composition("ab".to_string(), Some((0, 2)));
    assert_eq!(doc.get_composition_range(), Some((0, 2)));

    // Update with different range
    doc.update_composition("abc".to_string(), Some((0, 3)));
    assert_eq!(doc.get_composition_range(), Some((0, 3)));

    // End composition clears range
    doc.end_composition();
    assert!(doc.get_composition_range().is_none());
}

#[test]
fn composition_helper_get_target() {
    let mut doc = Document::new();
    let input1 = doc.create_element(QualName::html("input"));
    let input2 = doc.create_element(QualName::html("textarea"));

    // No target initially
    assert!(doc.get_composition_target().is_none());

    // Begin composition on input1
    doc.begin_composition(input1, "text".to_string(), None);
    assert_eq!(doc.get_composition_target(), Some(input1));

    // End and start on input2
    doc.end_composition();
    doc.begin_composition(input2, "more".to_string(), None);
    assert_eq!(doc.get_composition_target(), Some(input2));

    // End composition clears target
    doc.end_composition();
    assert!(doc.get_composition_target().is_none());
}

#[test]
fn composition_helpers_with_ranges() {
    let mut doc = Document::new();
    let contenteditable = doc.create_element(QualName::html("div"));

    // Simulate IME input with range tracking (UI Events §5.2.5)
    doc.begin_composition(contenteditable, "c".to_string(), Some("ru".to_string()));
    assert!(doc.is_composing());
    assert_eq!(doc.get_composition_target(), Some(contenteditable));

    // User updates composition
    doc.update_composition("ч".to_string(), Some((0, 1)));
    assert_eq!(doc.get_composition_range(), Some((0, 1)));

    doc.update_composition("чт".to_string(), Some((0, 2)));
    assert_eq!(doc.get_composition_range(), Some((0, 2)));

    // Final commit
    let final_state = doc.end_composition();
    assert!(!doc.is_composing());
    assert!(final_state.is_some());
    assert_eq!(final_state.unwrap().text, "чт");
}

#[test]
fn composition_event_dispatching_ready() {
    // Test CompositionEvent readiness for P3 dispatch (UI Events §5.2.5)
    // P3 will serialize these events to JS runtime

    // compositionstart event
    let start_evt = CompositionEvent::start("初".to_string(), Some("zh".to_string()));
    assert_eq!(start_evt.event_type, CompositionEventType::Start);
    assert_eq!(start_evt.event_type.as_str(), "compositionstart");
    assert_eq!(start_evt.data.data, "初");
    assert_eq!(start_evt.data.locale, Some("zh".to_string()));

    // compositionupdate events track user edits and cursor position
    let update1 = CompositionEvent::update("初".to_string(), Some((0, 1)));
    assert_eq!(update1.event_type.as_str(), "compositionupdate");
    assert_eq!(update1.data.range, Some((0, 1))); // cursor at offset 0, length 1

    let update2 = CompositionEvent::update("初中".to_string(), Some((0, 2)));
    assert_eq!(update2.data.data, "初中");
    assert_eq!(update2.data.range, Some((0, 2))); // preedit text spans 2 characters

    // compositionend event with final committed text
    let end_evt = CompositionEvent::end("初中文".to_string());
    assert_eq!(end_evt.event_type.as_str(), "compositionend");
    assert_eq!(end_evt.data.data, "初中文");
    assert_eq!(end_evt.data.range, None); // no range on final commit
}

#[test]
fn composition_event_empty_data() {
    // Edge case: some IMEs send compositionstart with empty data
    let start_empty = CompositionEvent::start("".to_string(), Some("ja".to_string()));
    assert_eq!(start_empty.data.data, "");
    assert_eq!(start_empty.data.locale, Some("ja".to_string()));

    // compositionupdate may not have locale info
    let update = CompositionEvent::update("text".to_string(), Some((0, 4)));
    assert_eq!(update.data.locale, None);

    // compositionend may have empty data (commit cleared by IME)
    let end_empty = CompositionEvent::end("".to_string());
    assert_eq!(end_empty.data.data, "");
    assert_eq!(end_empty.data.range, None);
}

#[test]
fn composition_multi_codepoint_range() {
    // Test range handling with multi-byte UTF-16 characters
    // Some characters (emoji, etc.) are 2 UTF-16 code units

    // surrogate pair emoji: 👍 = 2 UTF-16 code units
    let emoji_composition = CompositionEvent::update("👍".to_string(), Some((0, 2)));
    assert_eq!(emoji_composition.data.range, Some((0, 2)));

    // More complex: multiple characters with mixed widths
    let mixed = CompositionEvent::update("😀text😀".to_string(), Some((0, 6)));
    // emoji(2) + t(1) + e(1) + x(1) + t(1) + emoji(2) = 8 UTF-16 units
    assert_eq!(mixed.data.range, Some((0, 6)));
}
