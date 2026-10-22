//! Тесты FontFace/FontFaceSet, Performance Timeline, интеграции с GC,
//! лимита числа узлов, is_element_draggable и locate_text_offset_range.

use super::*;

// ──────── FontFace and FontFaceSet ────────

#[test]
fn font_face_creation() {
    let face = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"roboto.ttf\")".to_string(),
    );

    assert_eq!(face.family, "Roboto");
    assert_eq!(face.style, "normal");
    assert_eq!(face.weight, "400");
    assert_eq!(face.stretch, None);
    assert_eq!(face.unicode_range, None);
    assert_eq!(face.src, "url(\"roboto.ttf\")");
    assert_eq!(face.status, FontFaceStatus::Unloaded);
}

#[test]
fn font_face_with_properties() {
    let face = FontFace::new(
        "Inter".to_string(),
        "italic".to_string(),
        "700".to_string(),
        Some("condensed".to_string()),
        Some("U+0000-FFFF".to_string()),
        "url(\"inter-700i.woff2\") format(\"woff2\")".to_string(),
    );

    assert_eq!(face.family, "Inter");
    assert_eq!(face.style, "italic");
    assert_eq!(face.weight, "700");
    assert_eq!(face.stretch, Some("condensed".to_string()));
    assert_eq!(face.unicode_range, Some("U+0000-FFFF".to_string()));
}

#[test]
fn font_face_set_add_and_size() {
    let mut set = FontFaceSet::new();
    assert_eq!(set.size(), 0);

    let face1 = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"roboto.ttf\")".to_string(),
    );
    set.add(face1);
    assert_eq!(set.size(), 1);

    let face2 = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "700".to_string(),
        None,
        None,
        "url(\"roboto-bold.ttf\")".to_string(),
    );
    set.add(face2);
    assert_eq!(set.size(), 2);
}

#[test]
fn font_face_set_has_family() {
    let mut set = FontFaceSet::new();
    assert!(!set.has_family("Roboto"));

    let face = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"roboto.ttf\")".to_string(),
    );
    set.add(face);
    assert!(set.has_family("Roboto"));
    assert!(!set.has_family("Inter"));
}

#[test]
fn font_face_set_get_by_family() {
    let mut set = FontFaceSet::new();

    let face1 = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"roboto-400.ttf\")".to_string(),
    );
    set.add(face1);

    let face2 = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "700".to_string(),
        None,
        None,
        "url(\"roboto-700.ttf\")".to_string(),
    );
    set.add(face2);

    let face3 = FontFace::new(
        "Inter".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"inter-400.ttf\")".to_string(),
    );
    set.add(face3);

    let roboto_faces = set.get_by_family("Roboto");
    assert_eq!(roboto_faces.len(), 2);
    assert_eq!(roboto_faces[0].weight, "400");
    assert_eq!(roboto_faces[1].weight, "700");

    let inter_faces = set.get_by_family("Inter");
    assert_eq!(inter_faces.len(), 1);
    assert_eq!(inter_faces[0].weight, "400");

    let missing = set.get_by_family("Helvetica");
    assert_eq!(missing.len(), 0);
}

#[test]
fn font_face_set_clear() {
    let mut set = FontFaceSet::new();

    let face = FontFace::new(
        "Roboto".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"roboto.ttf\")".to_string(),
    );
    set.add(face);
    assert_eq!(set.size(), 1);

    set.clear();
    assert_eq!(set.size(), 0);
    assert!(!set.has_family("Roboto"));
}

#[test]
fn document_fonts_collection() {
    let mut doc = Document::new();
    assert_eq!(doc.fonts().size(), 0);

    let face = FontFace::new(
        "CustomFont".to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(\"custom.ttf\")".to_string(),
    );
    doc.fonts_mut().add(face);

    assert_eq!(doc.fonts().size(), 1);
    assert!(doc.fonts().has_family("CustomFont"));
}

// ── Performance Timeline tests (W3C Performance Timeline §3) ──────────────

#[test]
fn performance_entry_creation() {
    let entry = PerformanceEntry::new(
        PerformanceEntryType::Mark,
        "myMark".to_string(),
        10.5,
        0.0,
    );
    assert_eq!(entry.entry_type, PerformanceEntryType::Mark);
    assert_eq!(entry.name, "myMark");
    assert_eq!(entry.start_time, 10.5);
    assert_eq!(entry.duration, 0.0);
    assert_eq!(entry.end_time(), 10.5);
}

#[test]
fn performance_entry_end_time() {
    let entry = PerformanceEntry::new(
        PerformanceEntryType::Measure,
        "myMeasure".to_string(),
        100.0,
        50.0,
    );
    assert_eq!(entry.end_time(), 150.0);
}

#[test]
fn performance_entries_add_and_retrieve() {
    let mut entries = PerformanceEntries::new();
    assert!(entries.is_empty());

    let mark = PerformanceEntry::new(
        PerformanceEntryType::Mark,
        "mark1".to_string(),
        10.0,
        0.0,
    );
    entries.add_entry(mark);
    assert_eq!(entries.len(), 1);
    assert!(!entries.is_empty());

    let all = entries.all();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "mark1");
}

#[test]
fn performance_entries_filter_by_type() {
    let mut entries = PerformanceEntries::new();

    let mark = PerformanceEntry::new(
        PerformanceEntryType::Mark,
        "mark1".to_string(),
        10.0,
        0.0,
    );
    entries.add_entry(mark);

    let measure = PerformanceEntry::new(
        PerformanceEntryType::Measure,
        "measure1".to_string(),
        10.0,
        25.0,
    );
    entries.add_entry(measure);

    let marks = entries.get_by_type(PerformanceEntryType::Mark);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].name, "mark1");

    let measures = entries.get_by_type(PerformanceEntryType::Measure);
    assert_eq!(measures.len(), 1);
    assert_eq!(measures[0].name, "measure1");

    let resources = entries.get_by_type(PerformanceEntryType::Resource);
    assert_eq!(resources.len(), 0);
}

#[test]
fn performance_entries_filter_by_name() {
    let mut entries = PerformanceEntries::new();

    for i in 0..3 {
        let entry = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "mark1".to_string(),
            (i * 10) as f64,
            0.0,
        );
        entries.add_entry(entry);
    }

    let mark2 = PerformanceEntry::new(
        PerformanceEntryType::Mark,
        "mark2".to_string(),
        100.0,
        0.0,
    );
    entries.add_entry(mark2);

    let mark1s = entries.get_by_name("mark1");
    assert_eq!(mark1s.len(), 3);

    let mark2s = entries.get_by_name("mark2");
    assert_eq!(mark2s.len(), 1);

    let mark3s = entries.get_by_name("mark3");
    assert_eq!(mark3s.len(), 0);
}

#[test]
fn performance_entries_first_by_name() {
    let mut entries = PerformanceEntries::new();

    let mark1 = PerformanceEntry::new(
        PerformanceEntryType::Mark,
        "myMark".to_string(),
        50.0,
        0.0,
    );
    entries.add_entry(mark1);

    let first = entries.get_first_by_name("myMark");
    assert!(first.is_some());
    assert_eq!(first.unwrap().start_time, 50.0);

    let missing = entries.get_first_by_name("missing");
    assert!(missing.is_none());
}

#[test]
fn performance_entries_clear() {
    let mut entries = PerformanceEntries::new();

    let entry = PerformanceEntry::new(
        PerformanceEntryType::Mark,
        "mark1".to_string(),
        10.0,
        0.0,
    );
    entries.add_entry(entry);
    assert_eq!(entries.len(), 1);

    entries.clear();
    assert_eq!(entries.len(), 0);
    assert!(entries.is_empty());
}

#[test]
fn document_mark() {
    let mut doc = Document::new();
    assert_eq!(doc.performance_entries().len(), 0);

    doc.mark("myMark".to_string(), Some(25.0));
    assert_eq!(doc.performance_entries().len(), 1);

    let entries = doc.performance_entries_by_type(PerformanceEntryType::Mark);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "myMark");
    assert_eq!(entries[0].start_time, 25.0);
    assert_eq!(entries[0].duration, 0.0);
}

#[test]
fn document_measure_between_marks() {
    let mut doc = Document::new();

    // Create two marks
    doc.mark("start".to_string(), Some(100.0));
    doc.mark("end".to_string(), Some(150.0));

    // Measure between them
    let duration = doc.measure("myMeasure".to_string(), "start", "end");
    assert_eq!(duration, Some(50.0));

    let measures = doc.performance_entries_by_type(PerformanceEntryType::Measure);
    assert_eq!(measures.len(), 1);
    assert_eq!(measures[0].name, "myMeasure");
    assert_eq!(measures[0].duration, 50.0);
}

#[test]
fn document_measure_missing_marks() {
    let mut doc = Document::new();

    doc.mark("start".to_string(), Some(100.0));

    // Try to measure with missing mark
    let duration = doc.measure("myMeasure".to_string(), "start", "missing");
    assert_eq!(duration, None);

    // No measure should be created
    let measures = doc.performance_entries_by_type(PerformanceEntryType::Measure);
    assert_eq!(measures.len(), 0);
}

#[test]
fn document_performance_entries_by_name() {
    let mut doc = Document::new();

    doc.mark("mark1".to_string(), Some(10.0));
    doc.mark("mark1".to_string(), Some(20.0));
    doc.mark("mark2".to_string(), Some(30.0));

    let mark1s = doc.performance_entries_by_name("mark1");
    assert_eq!(mark1s.len(), 2);

    let mark2s = doc.performance_entries_by_name("mark2");
    assert_eq!(mark2s.len(), 1);

    let mark3s = doc.performance_entries_by_name("mark3");
    assert_eq!(mark3s.len(), 0);
}

#[test]
fn document_clear_performance_entries() {
    let mut doc = Document::new();

    doc.mark("mark1".to_string(), Some(10.0));
    doc.mark("mark2".to_string(), Some(20.0));
    assert_eq!(doc.performance_entries().len(), 2);

    doc.clear_performance_entries();
    assert_eq!(doc.performance_entries().len(), 0);
}

#[test]
fn performance_observer_creation() {
    let observer = PerformanceObserver::new();
    assert!(observer.handle().is_none());
    assert_eq!(observer.observed_types().len(), 0);
}

#[test]
fn performance_observer_observe() {
    let mut observer = PerformanceObserver::new();
    observer.observe(vec![PerformanceEntryType::Mark, PerformanceEntryType::Measure]);

    assert_eq!(observer.observed_types().len(), 2);
    assert!(observer.is_observing(PerformanceEntryType::Mark));
    assert!(observer.is_observing(PerformanceEntryType::Measure));
    assert!(!observer.is_observing(PerformanceEntryType::Resource));
}

#[test]
fn performance_observer_disconnect() {
    let mut observer = PerformanceObserver::new();
    observer.observe(vec![PerformanceEntryType::Mark]);
    observer.set_handle(123);

    assert!(observer.handle().is_some());
    assert_eq!(observer.handle(), Some(123));

    observer.disconnect();
    assert!(observer.handle().is_none());
    assert_eq!(observer.observed_types().len(), 0);
}

#[test]
fn performance_entry_type_display() {
    assert_eq!(PerformanceEntryType::Mark.to_string(), "mark");
    assert_eq!(PerformanceEntryType::Measure.to_string(), "measure");
    assert_eq!(PerformanceEntryType::Navigation.to_string(), "navigation");
    assert_eq!(PerformanceEntryType::Resource.to_string(), "resource");
    assert_eq!(PerformanceEntryType::Paint.to_string(), "paint");
    assert_eq!(PerformanceEntryType::Layout.to_string(), "layout");
}

#[test]
fn input_mode_parse_basic() {
    assert_eq!(InputMode::parse("text"), InputMode::Text);
    assert_eq!(InputMode::parse("none"), InputMode::None);
    assert_eq!(InputMode::parse("decimal"), InputMode::Decimal);
    assert_eq!(InputMode::parse("numeric"), InputMode::Numeric);
    assert_eq!(InputMode::parse("tel"), InputMode::Tel);
    assert_eq!(InputMode::parse("search"), InputMode::Search);
    assert_eq!(InputMode::parse("email"), InputMode::Email);
    assert_eq!(InputMode::parse("url"), InputMode::Url);
}

#[test]
fn input_mode_parse_case_insensitive() {
    assert_eq!(InputMode::parse("TEXT"), InputMode::Text);
    assert_eq!(InputMode::parse("NONE"), InputMode::None);
    assert_eq!(InputMode::parse("DeCiMaL"), InputMode::Decimal);
    assert_eq!(InputMode::parse("NumErIc"), InputMode::Numeric);
}

#[test]
fn input_mode_parse_whitespace_trim() {
    assert_eq!(InputMode::parse("  text  "), InputMode::Text);
    assert_eq!(InputMode::parse("\n  email\t"), InputMode::Email);
}

#[test]
fn input_mode_parse_unknown_default_to_text() {
    assert_eq!(InputMode::parse("unknown"), InputMode::Text);
    assert_eq!(InputMode::parse("random"), InputMode::Text);
    assert_eq!(InputMode::parse(""), InputMode::Text);
}

#[test]
fn input_mode_as_str() {
    assert_eq!(InputMode::Text.as_str(), "text");
    assert_eq!(InputMode::None.as_str(), "none");
    assert_eq!(InputMode::Decimal.as_str(), "decimal");
    assert_eq!(InputMode::Numeric.as_str(), "numeric");
    assert_eq!(InputMode::Tel.as_str(), "tel");
    assert_eq!(InputMode::Search.as_str(), "search");
    assert_eq!(InputMode::Email.as_str(), "email");
    assert_eq!(InputMode::Url.as_str(), "url");
}

#[test]
fn node_input_mode_for_input_element() {
    let mut doc = Document::new();

    // <input inputmode="email">
    let input_email = doc.create_element(QualName::html("input"));
    doc.get_mut(input_email).data = NodeData::Element {
        name: QualName::html("input"),
        attrs: vec![Attribute {
            name: QualName::html("inputmode"),
            value: "email".to_string(),
        }],
    };

    assert_eq!(doc.get(input_email).input_mode(), Some(InputMode::Email));

    // <input inputmode="numeric">
    let input_numeric = doc.create_element(QualName::html("input"));
    doc.get_mut(input_numeric).data = NodeData::Element {
        name: QualName::html("input"),
        attrs: vec![Attribute {
            name: QualName::html("inputmode"),
            value: "numeric".to_string(),
        }],
    };

    assert_eq!(
        doc.get(input_numeric).input_mode(),
        Some(InputMode::Numeric)
    );
}

#[test]
fn node_input_mode_for_textarea_element() {
    let mut doc = Document::new();

    // <textarea inputmode="url">
    let textarea = doc.create_element(QualName::html("textarea"));
    doc.get_mut(textarea).data = NodeData::Element {
        name: QualName::html("textarea"),
        attrs: vec![Attribute {
            name: QualName::html("inputmode"),
            value: "url".to_string(),
        }],
    };

    assert_eq!(doc.get(textarea).input_mode(), Some(InputMode::Url));
}

#[test]
fn node_input_mode_default_to_text_when_absent() {
    let mut doc = Document::new();

    // <input> without inputmode attribute
    let input = doc.create_element(QualName::html("input"));
    doc.get_mut(input).data = NodeData::Element {
        name: QualName::html("input"),
        attrs: vec![],
    };

    assert_eq!(doc.get(input).input_mode(), Some(InputMode::Text));
}

#[test]
fn node_input_mode_none_for_other_elements() {
    let mut doc = Document::new();

    // <div inputmode="email"> should return None
    let div = doc.create_element(QualName::html("div"));
    doc.get_mut(div).data = NodeData::Element {
        name: QualName::html("div"),
        attrs: vec![Attribute {
            name: QualName::html("inputmode"),
            value: "email".to_string(),
        }],
    };

    assert_eq!(doc.get(div).input_mode(), None);
}

// ── GC integration tests ───────────────────────────────────────────────────

#[test]
fn gc_acquire_increments_ref_count() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));

    assert_eq!(doc.js_ref_count(div), 0);
    assert_eq!(doc.acquire_js_ref(div), 1);
    assert_eq!(doc.js_ref_count(div), 1);
    assert_eq!(doc.acquire_js_ref(div), 2);
    assert_eq!(doc.js_ref_count(div), 2);
}

#[test]
fn gc_release_decrements_ref_count() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));

    doc.acquire_js_ref(div);
    doc.acquire_js_ref(div);
    assert_eq!(doc.release_js_ref(div), 1);
    assert_eq!(doc.release_js_ref(div), 0);
    assert_eq!(doc.js_ref_count(div), 0);
}

#[test]
fn gc_release_on_zero_ref_is_noop() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));

    // release without prior acquire must not panic
    assert_eq!(doc.release_js_ref(div), 0);
    assert_eq!(doc.js_ref_count(div), 0);
}

#[test]
fn gc_node_in_tree_is_not_detached() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), div);

    assert!(!doc.is_detached(div));
}

#[test]
fn gc_orphan_node_is_detached() {
    let mut doc = Document::new();
    // Created but never appended
    let div = doc.create_element(QualName::html("div"));
    assert!(doc.is_detached(div));
}

#[test]
fn gc_detached_node_with_zero_refs_is_dead() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    // Never appended, zero JS refs → dead
    let dead = doc.dead_node_ids();
    assert!(dead.contains(&div));
}

#[test]
fn gc_detached_node_with_live_ref_is_not_dead() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    doc.acquire_js_ref(div);

    let dead = doc.dead_node_ids();
    assert!(!dead.contains(&div));
}

#[test]
fn gc_node_in_tree_is_not_dead_even_with_zero_refs() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), div);

    let dead = doc.dead_node_ids();
    assert!(!dead.contains(&div));
}

#[test]
fn gc_remove_from_tree_then_release_ref_makes_dead() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), div);
    doc.acquire_js_ref(div);

    // Remove from tree but JS still holds a ref
    doc.detach(div);
    assert!(!doc.dead_node_ids().contains(&div), "still has a JS ref");

    // JS finalizer fires
    doc.release_js_ref(div);
    assert!(doc.dead_node_ids().contains(&div), "now collectable");
}

#[test]
fn gc_referenced_grandchild_pins_whole_detached_subtree() {
    // GAP-P3GCJSDOM срез 4: a live JS ref anywhere inside a detached
    // subtree must block collection of the whole subtree, not just the
    // referenced node itself — the old per-root-only check would have
    // reported `container` dead while `child` was still reachable from JS.
    let mut doc = Document::new();
    let container = doc.create_element(QualName::html("div"));
    let child = doc.create_element(QualName::html("span"));
    doc.append_child(container, child);
    doc.append_child(doc.root(), container);

    // JS holds only the grandchild, then the whole subtree is detached.
    doc.acquire_js_ref(child);
    doc.detach(container);

    let dead = doc.dead_node_ids();
    assert!(!dead.contains(&container), "container pinned by child's live ref");
    assert!(!dead.contains(&child), "child itself still referenced");

    doc.release_js_ref(child);
    let dead = doc.dead_node_ids();
    assert!(dead.contains(&container));
    assert!(dead.contains(&child), "whole unreferenced subtree becomes collectable together");
}

#[test]
fn gc_fully_unreferenced_detached_subtree_is_dead_together() {
    let mut doc = Document::new();
    let container = doc.create_element(QualName::html("div"));
    let child = doc.create_element(QualName::html("span"));
    doc.append_child(container, child);
    doc.append_child(doc.root(), container);
    doc.detach(container);

    let dead = doc.dead_node_ids();
    assert!(dead.contains(&container));
    assert!(dead.contains(&child));
}

#[test]
fn gc_reclaim_frees_text_and_attrs_in_place() {
    let mut doc = Document::new();
    let div = doc.create_element(QualName::html("div"));
    doc.get_mut(div).data = NodeData::Element {
        name: QualName::html("div"),
        attrs: vec![Attribute {
            name: QualName::html("data-x"),
            value: "a".repeat(64),
        }],
    };
    let text = doc.create_text("b".repeat(64));
    doc.append_child(div, text);

    let dead = doc.dead_node_ids();
    assert!(dead.contains(&div));
    assert!(dead.contains(&text));
    doc.reclaim_dead_nodes(&dead);

    match &doc.get(div).data {
        NodeData::Element { attrs, .. } => assert!(attrs.is_empty(), "attrs must be freed"),
        other => panic!("unexpected variant: {other:?}"),
    }
    match &doc.get(text).data {
        NodeData::Text(s) => assert!(s.is_empty(), "text content must be freed"),
        other => panic!("unexpected variant: {other:?}"),
    }
    assert!(doc.get(div).children.is_empty(), "children list must be freed");
}

#[test]
fn gc_reclaim_prunes_shadow_root_anchor_then_frees_it_next_pass() {
    // A dead host's shadow_roots entry must not keep anchoring its shadow
    // subtree forever (Document::is_detached treats any shadow_roots
    // value as permanently alive) — reclaim_dead_nodes must prune the
    // entry so the next dead_node_ids() pass can see the orphaned
    // shadow-root subtree as an ordinary detached candidate.
    let mut doc = Document::new();
    let host = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), host);
    let shadow = doc.attach_shadow(host, ShadowRootMode::Open);

    assert!(!doc.is_detached(shadow), "anchored by shadow_roots[host] while host is alive");
    doc.detach(host);
    assert!(!doc.is_detached(shadow), "still anchored while shadow_roots[host] exists");

    let dead = doc.dead_node_ids();
    assert!(dead.contains(&host));
    assert!(!dead.contains(&shadow), "still anchored on the pass that kills its host");
    doc.reclaim_dead_nodes(&dead);

    assert!(doc.shadow_root_of(host).is_none(), "shadow_roots entry pruned with its dead host");
    let dead_next = doc.dead_node_ids();
    assert!(dead_next.contains(&shadow), "orphaned shadow subtree collectable on the next pass");
}

#[test]
fn gc_root_is_never_dead() {
    let doc = Document::new();
    let dead = doc.dead_node_ids();
    assert!(!dead.contains(&doc.root()));
}

#[test]
fn gc_shadow_root_is_not_dead() {
    let mut doc = Document::new();
    let host = doc.create_element(QualName::html("div"));
    doc.append_child(doc.root(), host);
    let shadow_root = doc.attach_shadow(host, ShadowRootMode::Open);

    // Even with zero JS refs, shadow root must not be collected
    let dead = doc.dead_node_ids();
    assert!(!dead.contains(&shadow_root));
}

// ── DOM node count / limit tests ──────────────────────────────────────────

#[test]
fn node_count_returns_arena_length() {
    let mut doc = Document::new();
    let before = doc.node_count();
    doc.create_element(QualName::html("div"));
    assert_eq!(doc.node_count(), before + 1);
}

#[test]
fn try_create_element_ok_below_limit() {
    let mut doc = Document::new();
    let result = doc.try_create_element(QualName::html("span"));
    assert!(result.is_ok());
}

#[test]
fn try_create_element_err_at_limit() {
    let mut doc = Document::new();
    // Fill arena to exactly MAX_DOM_NODES nodes.
    while doc.node_count() < MAX_DOM_NODES {
        doc.create_element(QualName::html("div"));
    }
    assert_eq!(doc.node_count(), MAX_DOM_NODES);
    let result = doc.try_create_element(QualName::html("p"));
    assert_eq!(result, Err(NodeLimitExceeded));
}

#[test]
fn try_create_element_ok_after_reclaim_frees_slots_at_limit() {
    // BUG-1160: the cap counts live slots, not arena length — a reclaimed
    // slot is free capacity again.
    let mut doc = Document::new();
    while doc.node_count() < MAX_DOM_NODES {
        doc.create_element(QualName::html("div"));
    }
    assert_eq!(doc.try_create_element(QualName::html("p")), Err(NodeLimitExceeded));
    let dead = doc.dead_node_ids();
    doc.reclaim_dead_nodes(&dead);
    assert_eq!(doc.live_node_count(), MAX_DOM_NODES - dead.len());
    assert!(doc.try_create_element(QualName::html("p")).is_ok());
    assert_eq!(doc.node_count(), MAX_DOM_NODES, "freed slot reused, arena did not grow");
}

#[test]
fn try_create_text_ok_below_limit() {
    let mut doc = Document::new();
    assert!(doc.try_create_text("hi").is_ok());
}

#[test]
fn try_create_text_err_at_limit() {
    let mut doc = Document::new();
    while doc.node_count() < MAX_DOM_NODES {
        doc.create_element(QualName::html("div"));
    }
    assert_eq!(doc.try_create_text("hi"), Err(NodeLimitExceeded));
}

#[test]
fn try_create_comment_ok_below_limit() {
    let mut doc = Document::new();
    assert!(doc.try_create_comment("hi").is_ok());
}

#[test]
fn try_create_comment_err_at_limit() {
    let mut doc = Document::new();
    while doc.node_count() < MAX_DOM_NODES {
        doc.create_element(QualName::html("div"));
    }
    assert_eq!(doc.try_create_comment("hi"), Err(NodeLimitExceeded));
}

#[test]
fn node_limit_exceeded_display() {
    let msg = NodeLimitExceeded.to_string();
    assert!(msg.contains("50000"), "display should mention MAX_DOM_NODES");
}

#[test]
fn warn_threshold_less_than_max() {
    const { assert!(WARN_DOM_NODES < MAX_DOM_NODES) };
}

// ── is_element_draggable ──────────────────────────────────────────────────

/// Create a one-element document and return (doc, element_id).
fn make_elem(tag: &str, attrs: &[(&str, &str)]) -> (Document, NodeId) {
    let mut doc = Document::new();
    let elem = doc.create_element(QualName::html(tag));
    for (name, val) in attrs {
        let node = doc.get_mut(elem);
        if let NodeData::Element { attrs: a, .. } = &mut node.data {
            a.push(Attribute {
                name: QualName::html(*name),
                value: val.to_string(),
            });
        }
    }
    doc.append_child(doc.root(), elem);
    (doc, elem)
}

#[test]
fn draggable_true_makes_element_draggable() {
    let (doc, node) = make_elem("div", &[("draggable", "true")]);
    assert!(is_element_draggable(&doc, node));
}

#[test]
fn draggable_false_prevents_drag_on_img() {
    let (doc, node) = make_elem("img", &[("draggable", "false"), ("src", "x.png")]);
    assert!(!is_element_draggable(&doc, node));
}

#[test]
fn img_is_draggable_by_default() {
    let (doc, node) = make_elem("img", &[("src", "photo.jpg")]);
    assert!(is_element_draggable(&doc, node));
}

#[test]
fn anchor_with_href_is_draggable() {
    let (doc, node) = make_elem("a", &[("href", "https://example.com")]);
    assert!(is_element_draggable(&doc, node));
}

#[test]
fn anchor_without_href_is_not_draggable() {
    let (doc, node) = make_elem("a", &[]);
    assert!(!is_element_draggable(&doc, node));
}

#[test]
fn plain_div_is_not_draggable() {
    let (doc, node) = make_elem("div", &[]);
    assert!(!is_element_draggable(&doc, node));
}

#[test]
fn draggable_case_insensitive() {
    let (doc, node) = make_elem("div", &[("DRAGGABLE", "TRUE")]);
    assert!(is_element_draggable(&doc, node));
}

// ──────── locate_text_offset_range ────────

#[test]
fn locate_text_offset_range_single_child() {
    let mut doc = Document::new();
    let host = doc.create_element(QualName::html("div"));
    let text = doc.create_text("hello wrold");
    doc.append_child(doc.root(), host);
    doc.append_child(host, text);

    let found = locate_text_offset_range(&doc, host, 6, 11).unwrap();
    assert_eq!(found, (text, 6, 11));
}

#[test]
fn locate_text_offset_range_across_nested_elements() {
    // <div>hello <b>wrold</b> there</div> — textContent = "hello wrold there".
    let mut doc = Document::new();
    let host = doc.create_element(QualName::html("div"));
    let t1 = doc.create_text("hello ");
    let bold = doc.create_element(QualName::html("b"));
    let t2 = doc.create_text("wrold");
    let t3 = doc.create_text(" there");
    doc.append_child(doc.root(), host);
    doc.append_child(host, t1);
    doc.append_child(host, bold);
    doc.append_child(bold, t2);
    doc.append_child(host, t3);

    assert_eq!(node_text_content(&doc, host), "hello wrold there");

    // "wrold" sits at global offset 6..11, inside t2 (nested under <b>).
    let found = locate_text_offset_range(&doc, host, 6, 11).unwrap();
    assert_eq!(found, (t2, 0, 5));
}

#[test]
fn locate_text_offset_range_none_when_crossing_nodes() {
    let mut doc = Document::new();
    let host = doc.create_element(QualName::html("div"));
    let t1 = doc.create_text("ab");
    let t2 = doc.create_text("cd");
    doc.append_child(doc.root(), host);
    doc.append_child(host, t1);
    doc.append_child(host, t2);

    // Range 1..3 spans "b" (end of t1) + "c" (start of t2) — crosses a boundary.
    assert!(locate_text_offset_range(&doc, host, 1, 3).is_none());
}

#[test]
fn locate_text_offset_range_out_of_bounds() {
    let (doc, div, _text) = make_text_doc("abc");
    assert!(locate_text_offset_range(&doc, div, 10, 12).is_none());
}
