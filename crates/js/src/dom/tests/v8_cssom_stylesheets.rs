//! CSSOM-1 срез 3: `document.styleSheets`, `<style>`/`<link>.sheet`,
//! `CSSStyleSheet.cssRules`, `CSSStyleRule.selectorText`/`style.cssText`,
//! `CSSMediaRule.media.mediaText` — read-only JS bindings over
//! `V8JsRuntime::update_stylesheet_nodes`. See
//! `docs/tasks/p1-cssom-1-stylesheets.md`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_css_parser::StylesheetNodeEntry;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// [`make_doc`] plus a `<style id=s1>` in `<head>` — needed to test
/// `<style>.sheet`, not just `document.styleSheets`.
fn make_doc_with_style() -> (Arc<Mutex<Document>>, u32) {
    let doc_arc = make_doc();
    let style_nid = {
        let mut doc = doc_arc.lock().unwrap();
        let head = super::super::find_element_by_tag(&doc, "head").unwrap();
        let style = doc.create_element(QualName::html("style"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(style).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"),
                value: "s1".into(),
            });
        }
        doc.append_child(head, style);
        style.index() as u32
    };
    (doc_arc, style_nid)
}

fn one_sheet_entry(node: u32, css: &str) -> Vec<StylesheetNodeEntry> {
    vec![StylesheetNodeEntry {
        node,
        sheet: Arc::new(lumen_css_parser::parse(css)),
        disabled: false,
    }]
}

#[test]
fn style_sheets_empty_without_registry() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.styleSheets.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn style_sheets_length_after_update() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt.eval("document.styleSheets.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn style_rule_selector_text_and_style_css_text() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "p.foo { color: red; font-weight: bold; }",
    ));
    let r = rt.eval("document.styleSheets[0].cssRules[0].selectorText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("p.foo".to_string()));
    let r = rt.eval("document.styleSheets[0].cssRules[0].style.cssText").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("color: red; font-weight: bold;".to_string())
    );
}

#[test]
fn media_rule_media_text_and_nested_rule() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "@media screen and (min-width: 600px) { div { color: blue; } }",
    ));
    let r = rt.eval("document.styleSheets[0].cssRules.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
    let r = rt.eval("document.styleSheets[0].cssRules[0].media.mediaText").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("screen and (min-width: 600px)".to_string())
    );
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].cssRules[0].selectorText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("div".to_string()));
}

#[test]
fn style_element_sheet_getter() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt
        .eval("document.getElementById('s1').sheet.cssRules.length")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn style_element_sheet_null_without_registry_entry() {
    let (doc, _style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval("document.getElementById('s1').sheet").unwrap();
    assert_eq!(r, lumen_core::JsValue::Null);
}

#[test]
fn instanceof_css_style_sheet_and_css_style_rule() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt.eval("document.styleSheets[0] instanceof CSSStyleSheet").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt
        .eval("document.styleSheets[0].cssRules[0] instanceof CSSStyleRule")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt
        .eval("document.styleSheets[0].cssRules[0] instanceof CSSRule")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── CSSOM-8 (BUG-518 срез 9): `.style`'s write half on an own sheet's rule ──

#[test]
fn top_level_rule_style_setter_updates_css_text() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    rt.eval("document.styleSheets[0].cssRules[0].style.color = 'blue'").unwrap();
    let r = rt.eval("document.styleSheets[0].cssRules[0].style.cssText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color: blue;".to_string()));
}

#[test]
fn top_level_rule_style_setter_leaves_sibling_rule_untouched() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "p { color: red; } span { color: green; }",
    ));
    rt.eval("document.styleSheets[0].cssRules[0].style.setProperty('color', 'blue')")
        .unwrap();
    let r = rt.eval("document.styleSheets[0].cssRules[1].style.cssText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color: green;".to_string()));
}

#[test]
fn top_level_rule_style_css_text_setter_replaces_whole_declaration_list() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    rt.eval(
        "document.styleSheets[0].cssRules[0].style.cssText = 'color: blue; font-weight: bold'",
    )
    .unwrap();
    let r = rt.eval("document.styleSheets[0].cssRules[0].style.cssText").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("color: blue; font-weight: bold;".to_string())
    );
}

#[test]
fn media_child_rule_style_setter_updates_css_text() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "@media screen { div { color: red; } }",
    ));
    rt.eval("document.styleSheets[0].cssRules[0].cssRules[0].style.color = 'blue'")
        .unwrap();
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].cssRules[0].style.cssText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color: blue;".to_string()));
}

// ── CSSOM-8 вариант C: the page cascade a same-tick `getComputedStyle` reads
// is an INDEPENDENT parse of the concatenated `<style>`/`<link>` text
// (`page_pipeline.rs::build_page_cascade`), not the per-node `Stylesheet`
// these CSSOM natives mutate above. `FlushHandles::maybe_flush` replays the
// recorded ops onto a throwaway clone of the pushed cascade sheet
// (`style_flush.rs::cssom_patched_sheet`), so both registries below carry the
// SAME source text — that identity is what `Stylesheet::locate_embedded_source`
// needs to find the node's contribution inside the cascade's concatenation.

/// [`v8_runtime_with_dom`] plus the cascade/viewport push
/// `FlushHandles::maybe_flush` needs to do anything, mirroring
/// `v8_bug493_sync_flush.rs::v8_runtime_with_flush`.
fn v8_runtime_with_flush_and_style_node(css: &str) -> (V8JsRuntime, u32) {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, css));
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(css)));
    rt.update_viewport_size(800.0, 600.0);
    (rt, style_nid)
}

/// A same-tick `getComputedStyle` after `CSSStyleSheet.insertRule` must see
/// the inserted rule — pre-slice, `insertRule` only reached the per-node
/// `Stylesheet` CSSOM hands out, never the independent cascade parse layout
/// reads.
#[test]
fn insert_rule_is_visible_to_same_tick_get_computed_style() {
    let (rt, _style_nid) = v8_runtime_with_flush_and_style_node("#main { width: 50px; }");
    let r = rt
        .eval(
            "(function() {
                document.styleSheets[0].insertRule('#main { width: 123px; }', 1);
                return getComputedStyle(document.getElementById('main')).width;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("123px".to_string()));
}

/// Sibling of the above for `deleteRule` — removing the overriding
/// later rule must un-hide the earlier one in the very same read.
#[test]
fn delete_rule_is_visible_to_same_tick_get_computed_style() {
    let (rt, _style_nid) =
        v8_runtime_with_flush_and_style_node("#main { width: 50px; } #main { width: 123px; }");
    let r = rt
        .eval(
            "(function() {
                document.styleSheets[0].deleteRule(1);
                return getComputedStyle(document.getElementById('main')).width;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("50px".to_string()));
}

/// Sibling of the above for the `.style` setter (BUG-518 срез 9) — that
/// slice only ever proved the own-sheet `cssText` echoed the write, not that
/// the cascade layout reads from picked it up.
#[test]
fn rule_style_setter_is_visible_to_same_tick_get_computed_style() {
    let (rt, _style_nid) = v8_runtime_with_flush_and_style_node("#main { width: 50px; }");
    let r = rt
        .eval(
            "(function() {
                document.styleSheets[0].cssRules[0].style.width = '123px';
                return getComputedStyle(document.getElementById('main')).width;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("123px".to_string()));
}

/// A page that never calls any CSSOM mutator must not pay the
/// `cssom_patched_sheet` clone at all — same-tick reads stay on the plain
/// pushed cascade sheet.
#[test]
fn get_computed_style_without_cssom_mutation_uses_pristine_cascade() {
    let (rt, _style_nid) = v8_runtime_with_flush_and_style_node("#main { width: 50px; }");
    let r = rt
        .eval("getComputedStyle(document.getElementById('main')).width")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("50px".to_string()));
}

// ── CSSOM-8, вложенные правила: nested-rule addressing into a top-level
// `@mixin`'s `@result` tree (`mixin-invalidation.tentative.html`) ──────────

#[test]
fn mixin_rule_css_rules_is_empty_without_a_result_block() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "@mixin --m() { color: red; }"));
    let r = rt.eval("document.styleSheets[0].cssRules[0].cssRules.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn mixin_rule_css_rules_has_one_result_child_with_a_result_block() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "@mixin --m() { @result { color: red; } }",
    ));
    let r = rt.eval("document.styleSheets[0].cssRules[0].cssRules.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
    // `@result`'s own child is the sole declarations-only "decls" node.
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].cssRules[0].cssRules[0].style.cssText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color: red;".to_string()));
}

#[test]
fn mixin_result_nested_rule_exposes_its_own_style() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "@mixin --m() { @result { &.a { color: blue; } } }",
    ));
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].cssRules[0].cssRules[0].style.cssText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color: blue;".to_string()));
}

/// CSSOM-8 срез 12: `mixin-invalidation.tentative.html`'s "invalidation of
/// @mixin from same stylesheet" — a `.style` write on a real `& {...}`
/// nested rule inside `@result` must reach the element the enclosing
/// `@apply` call site applies it to, not just the mixin-result tree's own
/// CSSOM view (see `mixin_result_node_style_setter_updates_css_text` right
/// below, which only checks the latter). Before this slice `replay_cssom_ops`
/// mutated `mixin_rules[..].result` and left `collect_mixin_nested_rules`'s
/// baked, already-selector-combined copy in `Stylesheet::rules` stale.
#[test]
fn set_style_on_a_mixin_nested_rule_is_visible_to_same_tick_get_computed_style_of_the_applying_element() {
    let (rt, _style_nid) = v8_runtime_with_flush_and_style_node(
        "@mixin --m() { @result { &#main { width: 50px; } } } #main { @apply --m; }",
    );
    let r = rt
        .eval(
            "(function() {
                document.styleSheets[0].cssRules[0].cssRules[0].cssRules[0].style.width = '123px';
                return getComputedStyle(document.getElementById('main')).width;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("123px".to_string()));
}

#[test]
fn mixin_result_node_style_setter_updates_css_text() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "@mixin --m() { @result { color: red; } }",
    ));
    rt.eval("document.styleSheets[0].cssRules[0].cssRules[0].cssRules[0].style.color = 'blue'")
        .unwrap();
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].cssRules[0].cssRules[0].style.cssText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color: blue;".to_string()));
}

/// `mixin-invalidation.tentative.html`'s "invalidation on adding @apply
/// rule" subtest shape — `CSSGroupingRule.insertRule` on a top-level style
/// rule's own body, restricted to an `@apply` statement.
#[test]
fn top_level_rule_insert_rule_accepts_an_apply_statement() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].insertRule('@apply --centered();', 0)")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn top_level_rule_insert_rule_rejects_a_non_apply_rule_text() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt.eval(
        "(function() {
            try {
                document.styleSheets[0].cssRules[0].insertRule('b { color: red; }', 0);
                return 'no-throw';
            } catch (e) {
                return e.name;
            }
        })()",
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("SyntaxError".to_string()));
}
