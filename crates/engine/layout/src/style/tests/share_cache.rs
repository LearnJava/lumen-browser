//! THREAD-4 срез 2 — тесты intra-pass structural memo (`style::share_cache`).
//!
//! Срез 1 нашёл ~4.5× повтор одного и того же структурного ключа
//! `(tag, classes)` среди SVG-иконок github.com (GitHub Octicons). Первый
//! тест здесь — тот самый случай в миниатюре: N идентичных `<svg class=
//! "octicon">` под одним родителем должны дать сокращение числа вызовов
//! `compute_style` и при этом байт-в-байт совпадающий `ComputedStyle`.
//! Второй — негативный: как только в листе появляется правило с
//! комбинатором, способное дать РАЗНЫЙ результат для одинаково выглядящих
//! узлов в разных поддеревьях, memo обязан САМООТКЛЮЧИТЬСЯ (не просто не
//! посчитать это правило — перестать шарить вообще), иначе один узел получил
//! бы стиль другого.

use super::*;
use lumen_dom::{NodeId, ShadowRootMode};
use std::collections::HashMap;

const VP: Size = Size { width: 800.0, height: 600.0 };

fn octicon_group(n: usize) -> lumen_dom::Document {
    let icon = r#"<svg class="octicon" aria-hidden="true"><path d="M1 1 2 2"></path></svg>"#;
    lumen_html_parser::parse(&format!("<div class=\"toolbar\">{}</div>", icon.repeat(n)))
}

#[test]
fn repeated_svg_icons_share_the_cascade_without_changing_the_result() {
    // `SHADOW_SHEETS` is a thread-local shared with every other test on this
    // thread (`cargo test` reuses threads) — an unrelated shadow-DOM test
    // leaving it populated would disable sharing here for a reason that has
    // nothing to do with this test's own document.
    clear_shadow_sheets();
    let doc = octicon_group(6);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(".octicon { fill: rgb(1, 2, 3); } path { stroke: rgb(9, 9, 9); }");
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    assert_eq!(svgs.len(), 6);

    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    // Correctness first: every icon's own style, and its <path> child's style,
    // must be the one the rules actually specify — a wrong "hit" that reused
    // an unrelated node's style would show up here as a wrong color.
    for &svg in &svgs {
        let style = map.style_for(svg).expect("style computed for every element");
        assert_eq!(style.svg_fill, SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }));
        let path = doc.get(svg).children[0];
        let path_style = map.style_for(path).expect("style computed for every element");
        assert_eq!(path_style.svg_stroke, SvgPaint::Color(Color { r: 9, g: 9, b: 9, a: 255 }));
    }

    // The win: every icon (after the first) shares the *same* `ComputedStyle`
    // allocation as the one before it — a refcount bump, not a recomputed
    // cascade (BUG-341 S9's reasoning). `take_compute_style_calls` would show
    // this too, but it is a process-wide atomic (deliberately, for rayon
    // fan-out — see its doc comment) shared with every other test running
    // concurrently in this binary, so asserting on it here would be flaky.
    let first = map.style_arc(svgs[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(std::sync::Arc::ptr_eq(&first, &arc), "repeated icons must share one allocation");
    }
    let first_path = map.style_arc(doc.get(svgs[0]).children[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(doc.get(svg).children[0]).expect("arc");
        assert!(std::sync::Arc::ptr_eq(&first_path, &arc), "repeated <path> children must share one allocation");
    }
}

#[test]
fn a_descendant_selector_still_shares_when_the_key_proves_ancestor_identity() {
    // BUG-1112: `.octicon path` has a `Descendant` combinator, which THREAD-4
    // срез 2 banned outright. It is safe here because both `<path>`s are
    // literal children of a repeated `<svg class="octicon">` — their key's
    // `inherited_ptr` already collides (same toolbar parent, so both `<svg>`s
    // receive the identical `inherited` allocation and, once the first `<svg>`
    // itself gets cached, so do their `<path>` children), which by
    // `selector_is_share_safe`'s doc comment proves the ancestor chain (here:
    // the `.octicon` parent) is identical for both, so reusing one's cascade
    // result for the other is exactly as correct as recomputing it.
    clear_shadow_sheets();
    let doc = octicon_group(6);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(".octicon path { stroke: rgb(9, 9, 9); }");
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let first_path = map.style_arc(doc.get(svgs[0]).children[0]).expect("arc");
    for &svg in &svgs[1..] {
        let path = doc.get(svg).children[0];
        let arc = map.style_arc(path).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&first_path, &arc),
            "a descendant-combinator rule must not block sharing once the key already proves ancestor identity"
        );
        assert_eq!(
            map.style_for(path).expect("style").svg_stroke,
            SvgPaint::Color(Color { r: 9, g: 9, b: 9, a: 255 })
        );
    }
}

#[test]
fn a_combinator_rule_disables_sharing_for_the_nodes_it_could_reach() {
    // Two icons, identical tag/attrs, under differently-classed ancestors —
    // `.blue`/`.plain` are plain `<div>`s, never eligible for the share key
    // (`is_svg_presentational_element` is false for `div`), so their own
    // style is always a fresh, per-instance allocation and the two icons'
    // `inherited_ptr` never collides — `.blue .octicon` never gets a chance
    // to reuse anything, matching CSS whether or not the combinator itself
    // is allowed (BUG-1112). If sharing fired here anyway, one icon would
    // silently render the other's fill.
    let doc = lumen_html_parser::parse(concat!(
        r#"<div class="plain"><svg class="octicon" aria-hidden="true"><path d="M1 1"></path></svg></div>"#,
        r#"<div class="blue"><svg class="octicon" aria-hidden="true"><path d="M1 1"></path></svg></div>"#,
    ));
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        ".octicon { fill: rgb(1, 2, 3); } .blue .octicon { fill: rgb(0, 0, 255); }",
    );
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let containers = &doc.get(doc.body().unwrap()).children;
    let plain_svg = doc.get(containers[0]).children[0];
    let blue_svg = doc.get(containers[1]).children[0];

    let plain_fill = map.style_for(plain_svg).expect("style").svg_fill.clone();
    let blue_fill = map.style_for(blue_svg).expect("style").svg_fill.clone();
    assert_eq!(plain_fill, SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }));
    assert_eq!(blue_fill, SvgPaint::Color(Color { r: 0, g: 0, b: 255, a: 255 }), "ancestor-dependent rule must still apply per node");
}

#[test]
fn an_unrelated_shadow_tree_elsewhere_does_not_disable_sharing() {
    // THREAD-4 срез 4: срез 3 нашёл, что github.com's один web-component
    // (`SHADOW_SHEETS` непусто) гасил кэш ДОКУМЕНТО-ШИРОКО — ни одна из
    // ~1775 иконок ни разу не попадала в кэш, хотя ни одна не имеет
    // отношения к тому shadow-дереву. Этот тест воспроизводит ровно это:
    // repeated icons live entirely outside the shadow host (not the host,
    // not slotted into it, not inside its tree) and must still share.
    clear_shadow_sheets();
    let icon = r#"<svg class="octicon" aria-hidden="true"><path d="M1 1 2 2"></path></svg>"#;
    let doc = lumen_html_parser::parse(&format!(
        r#"<div id="host"></div><div class="toolbar">{}</div>"#,
        icon.repeat(6),
    ));
    let body = doc.body().expect("body");
    let host = doc.get(body).children[0];
    let toolbar = doc.get(body).children[1];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    assert_eq!(svgs.len(), 6);

    // `host` has no children, so nothing is slotted into it — the shadow
    // tree touches only `host` itself, never the toolbar's icons.
    let mut sheets: HashMap<NodeId, Stylesheet> = HashMap::new();
    sheets.insert(host, lumen_css_parser::parse(":host { color: red; }"));
    set_shadow_sheets(sheets);

    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(".octicon { fill: rgb(1, 2, 3); }");
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);
    clear_shadow_sheets();

    let first = map.style_arc(svgs[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&first, &arc),
            "icons unrelated to the shadow host must still share despite SHADOW_SHEETS being non-empty"
        );
        assert_eq!(map.style_for(svg).expect("style").svg_fill, SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }));
    }
}

#[test]
fn a_root_scoped_custom_property_rule_does_not_disable_sharing() {
    // BUG-1112 срез 3: live instrumentation on github.com found `:root { … }`
    // (the common way to declare CSS custom properties) as a *candidate* for
    // every single node in the document (`RuleIndex`'s `universal` bucket —
    // `:root`'s subject has no type/class/id to index on), which zeroed
    // `share_insert` on the whole page even though `:root` can never match a
    // `<path>`/`<svg>`/… subject (only `<html>` is ever the document root).
    clear_shadow_sheets();
    let doc = octicon_group(6);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(":root { --accent: red; } .octicon { fill: rgb(1, 2, 3); }");
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();

    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let first = map.style_arc(svgs[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&first, &arc),
            "an unrelated :root rule in the sheet must not block sharing for non-root nodes"
        );
    }
}

#[test]
fn a_subject_attribute_selector_does_not_disable_sharing() {
    // BUG-1112 срез 3: second live blocker after the `:root` fix above —
    // github.com's dark-mode custom properties
    // (`[data-color-mode=light][data-light-theme*=light] { … }`) disqualified
    // every SVG-presentational node the same way. An attribute selector on
    // the *subject* is a pure function of `node`'s own attributes, which
    // `ShareKey.attrs` already pins byte-for-byte — sound to allow, unlike
    // `Class`/`Id`-only which was the pre-срез-3 restriction.
    clear_shadow_sheets();
    let doc = octicon_group(6);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(r#"[aria-hidden="true"] { fill: rgb(1, 2, 3); }"#);
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();

    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let first = map.style_arc(svgs[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&first, &arc),
            "an attribute selector matched against the subject's own (key-pinned) attributes must not block sharing"
        );
        assert_eq!(
            map.style_for(svg).expect("style").svg_fill,
            SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 })
        );
    }
}

#[test]
fn an_ancestor_position_attribute_selector_still_disables_sharing() {
    // Regression guard for a bug срез 3 introduced and caught before landing:
    // an early version of the subject/ancestor split assigned `is_subject`
    // to `ComplexSelector::head`, but `head` is the LEFTMOST (topmost
    // ancestor) compound — `matching.rs::matches_complex` matches the
    // *last* `tail` entry (or `head` when `tail` is empty) against `node`.
    // For a 2+ compound selector that made an ancestor's attribute selector
    // look "subject-safe", which is unsound: `ShareKey` only pins the
    // *subject's* attributes, not an ancestor's — two `.octicon`s under
    // differently-attributed ancestors must NOT share here.
    let doc = lumen_html_parser::parse(concat!(
        r#"<div data-theme="a"><svg class="octicon" aria-hidden="true"><path d="M1 1"></path></svg></div>"#,
        r#"<div data-theme="b"><svg class="octicon" aria-hidden="true"><path d="M1 1"></path></svg></div>"#,
    ));
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        r#".octicon { fill: rgb(1, 2, 3); } [data-theme="b"] .octicon { fill: rgb(0, 0, 255); }"#,
    );
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let containers = &doc.get(doc.body().unwrap()).children;
    let a_svg = doc.get(containers[0]).children[0];
    let b_svg = doc.get(containers[1]).children[0];

    assert_eq!(map.style_for(a_svg).expect("style").svg_fill, SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }));
    assert_eq!(
        map.style_for(b_svg).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 0, g: 0, b: 255, a: 255 }),
        "ancestor-attribute-dependent rule must still apply per node, not get shared away"
    );
    // Still holds after BUG-1112 срез 5 (ancestor `Attribute` allowed): this
    // scenario was never actually exercising that induction in the first
    // place — `a_svg`/`b_svg` sit under two DIFFERENT `<div>` instances, so
    // their `inherited_ptr` never collided regardless of `selector_is_share_
    // safe`'s verdict on `[data-theme="b"] .octicon` (a plain `<div>` is
    // never `is_svg_presentational_element`, so its own style is always a
    // fresh per-instance allocation — same reasoning as
    // `a_combinator_rule_disables_sharing_for_the_nodes_it_could_reach`
    // above). See
    // `an_ancestor_position_attribute_selector_now_shares_under_a_literal_
    // common_parent` below for the positive case срез 5 actually unlocks.
}

#[test]
fn an_ancestor_position_attribute_selector_now_shares_under_a_literal_common_parent() {
    // BUG-1112 срез 5: an ancestor `Attribute` selector is now share-safe —
    // this is the case it makes reachable. All six icons are literal
    // children of the SAME live `<div data-theme="b">`, so their
    // `inherited_ptr` collides (`case a` of the induction, the simplest
    // one: not even a chained cache-hit ancestor is needed) and
    // `[data-theme="b"] .octicon` reads the SAME `data-theme` value for
    // every one of them — before срез 5 this pattern zeroed `shareable` for
    // every icon under such a wrapper, even though the wrapper is
    // per-instance shared, not per-icon.
    clear_shadow_sheets();
    let icon = r#"<svg class="octicon" aria-hidden="true"><path d="M1 1 2 2"></path></svg>"#;
    let doc = lumen_html_parser::parse(&format!(r#"<div data-theme="b">{}</div>"#, icon.repeat(6)));
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        r#".octicon { fill: rgb(1, 2, 3); } [data-theme="b"] .octicon { fill: rgb(0, 0, 255); }"#,
    );
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    assert_eq!(svgs.len(), 6);

    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let first = map.style_arc(svgs[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&first, &arc),
            "an ancestor attribute selector must not block sharing once the key already proves ancestor identity"
        );
        assert_eq!(
            map.style_for(svg).expect("style").svg_fill,
            SvgPaint::Color(Color { r: 0, g: 0, b: 255, a: 255 })
        );
    }
}

#[test]
fn a_subject_dynamic_pseudo_class_still_disables_sharing_behind_a_combinator() {
    // Companion regression guard: with the head/tail fix, the *last* tail
    // compound is the subject — confirm a genuinely position-dependent
    // pseudo-class there (`:first-child`, not in the key at any level) still
    // disables sharing, same as it must at zero-tail depth.
    let doc = lumen_html_parser::parse(concat!(
        r#"<div class="toolbar">"#,
        r#"<svg class="octicon" aria-hidden="true"></svg>"#,
        r#"<svg class="octicon" aria-hidden="true"></svg>"#,
        r#"</div>"#,
    ));
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        ".toolbar > .octicon:first-child { fill: rgb(255, 0, 0); } .octicon { fill: rgb(1, 2, 3); }",
    );
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    assert_eq!(
        map.style_for(svgs[0]).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 255, g: 0, b: 0, a: 255 }),
        ":first-child must still win on the actual first child"
    );
    assert_eq!(
        map.style_for(svgs[1]).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }),
        "and must not leak onto its sibling via a wrongly-shared cache entry"
    );
}

#[test]
fn a_subject_first_child_pseudo_class_does_not_disable_sharing_when_irrelevant() {
    // BUG-1112 срез 4: live instrumentation (срез 3) found
    // `.pagination > :first-child`/`.btn .octicon:only-child`-shaped rules
    // disqualifying every SVG-presentational node in the document, because
    // `:first-child`'s subject has no type/class/id — `RuleIndex` buckets it
    // as `universal`, a "candidate" for every node regardless of whether it
    // is ever a descendant of `.pagination`/`.btn`. None of the icons here
    // are descendants of `.pagination` at all, so the rule never actually
    // applies to them — but срез 3's fix (`Attribute`/`Root` safe on subject)
    // did not yet cover this shape; this test is the case срез 4 lands.
    //
    // Once any rule in the sheet has a `:first-child`/`:last-child`/
    // `:only-child` subject anywhere, `ShareKey` starts tracking real sibling
    // position for every node (`sheet_has_position_dependent_subject`'s
    // sheet-wide, not per-selector, gate — see its doc comment for why: a
    // per-node reachability check would cost as much as just running
    // `matches_complex`). So among 6 siblings, only the true first and true
    // last get their own singleton key; the middle 4 still collide and
    // share — that partial win (4/6, not 6/6) is what this test checks, not
    // uniform sharing across every position.
    clear_shadow_sheets();
    let doc = octicon_group(6);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        ".pagination > :first-child { color: red; } .octicon { fill: rgb(1, 2, 3); }",
    );
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();

    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    // Middle siblings (neither first nor last) share one allocation despite
    // the position-dependent rule in the sheet — it never reaches them, and
    // their `is_first_child`/`is_last_child` key fields are both `false`.
    let middle = map.style_arc(svgs[1]).expect("arc");
    for &svg in &svgs[2..5] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&middle, &arc),
            "middle siblings, never reachable by the :first-child rule, must still share"
        );
    }
    for &svg in &svgs {
        assert_eq!(
            map.style_for(svg).expect("style").svg_fill,
            SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }),
            "every icon's own style must stay correct regardless of sharing"
        );
    }
}

#[test]
fn a_subject_first_child_pseudo_class_still_applies_correctly_when_relevant() {
    // Companion positive check: when the rule DOES reach real siblings (one
    // is genuinely first-child, the other is not), sharing must still give
    // each its own correct answer, not the other's — same shape as the
    // pre-existing `a_subject_dynamic_pseudo_class_still_disables_sharing_
    // behind_a_combinator` test, but now exercising the newly-allowed
    // sharing path instead of the disabled one: the two icons here have
    // different `is_first_child`/`is_last_child` key fields, so they were
    // never going to collide on the key in the first place.
    let doc = lumen_html_parser::parse(concat!(
        r#"<div class="toolbar">"#,
        r#"<svg class="octicon" aria-hidden="true"></svg>"#,
        r#"<svg class="octicon" aria-hidden="true"></svg>"#,
        r#"</div>"#,
    ));
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        ".toolbar > :first-child { fill: rgb(255, 0, 0); } .octicon { fill: rgb(1, 2, 3); }",
    );
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    assert_eq!(
        map.style_for(svgs[0]).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 255, g: 0, b: 0, a: 255 }),
        ":first-child must still win on the actual first child"
    );
    assert_eq!(
        map.style_for(svgs[1]).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }),
        "and must not leak onto its sibling now that :first-child is share-safe"
    );
}

#[test]
fn a_shadow_host_sibling_does_not_get_a_plain_siblings_cached_style() {
    // THREAD-4 срез 4, the false-share hazard `build_key`'s doc comment
    // describes: two `<svg class="octicon">` siblings under the same real
    // parent (so identical `tag`+`attrs`+`inherited_ptr` — the exact key
    // `ShareCache` looks up on), but the *second* one has its own shadow
    // root attached and a `:host` rule inside it. If the cache handed back
    // the first icon's plain-document style for the second, the `:host`
    // rule would silently never apply.
    clear_shadow_sheets();
    let mut doc = lumen_html_parser::parse(concat!(
        r#"<div class="toolbar">"#,
        r#"<svg class="octicon" aria-hidden="true"></svg>"#,
        r#"<svg class="octicon" aria-hidden="true"></svg>"#,
        r#"</div>"#,
    ));
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    assert_eq!(svgs.len(), 2);
    let shadow_host = svgs[1];
    doc.attach_shadow(shadow_host, ShadowRootMode::Open);

    let mut sheets: HashMap<NodeId, Stylesheet> = HashMap::new();
    sheets.insert(shadow_host, lumen_css_parser::parse(":host { fill: rgb(255, 0, 0); }"));
    set_shadow_sheets(sheets);

    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(".octicon { fill: rgb(1, 2, 3); }");
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);
    clear_shadow_sheets();

    let plain_fill = map.style_for(svgs[0]).expect("style").svg_fill.clone();
    let host_fill = map.style_for(svgs[1]).expect("style").svg_fill.clone();
    assert_eq!(plain_fill, SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }));
    assert_eq!(
        host_fill,
        SvgPaint::Color(Color { r: 255, g: 0, b: 0, a: 255 }),
        ":host rule must apply to the shadow-host sibling, not the plain sibling's cached style"
    );
}

#[test]
fn an_unreachable_ancestor_hover_pseudo_class_does_not_disable_sharing() {
    // BUG-1112 срез 6: `.btn:hover .octicon` used to zero `shareable` for
    // EVERY `.octicon` node in the document (the abstract selector has a
    // `:hover` in ancestor position, which the key cannot pin at any depth),
    // even here, where `.btn` does not exist anywhere in the document —
    // `RuleIndex` hands the rule back as a candidate purely because its
    // subject (`.octicon`) matches, regardless of whether the ancestor part
    // could ever apply. Since the rule can never contribute a declaration to
    // any of these nodes (real or shared), it cannot threaten sharing either.
    clear_shadow_sheets();
    let doc = octicon_group(6);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        ".octicon { fill: rgb(1, 2, 3); } .btn:hover .octicon { fill: rgb(0, 0, 255); }",
    );
    let toolbar = doc.get(doc.body().unwrap()).children[0];
    let svgs: Vec<NodeId> = doc.get(toolbar).children.clone();
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);

    let first = map.style_arc(svgs[0]).expect("arc");
    for &svg in &svgs[1..] {
        let arc = map.style_arc(svg).expect("arc");
        assert!(
            std::sync::Arc::ptr_eq(&first, &arc),
            "an ancestor :hover selector that can never reach these nodes must not block sharing"
        );
        assert_eq!(
            map.style_for(svg).expect("style").svg_fill,
            SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }),
            "the unreachable rule must never apply"
        );
    }
}

#[test]
fn a_reachable_ancestor_hover_pseudo_class_still_disables_sharing() {
    // Companion regression guard: unlike a plain HTML ancestor (`.btn`,
    // never `is_svg_presentational_element`, so two DIFFERENT `.btn`
    // instances can never collide on `inherited_ptr` in the first place —
    // see `a_combinator_rule_disables_sharing_for_the_nodes_it_could_reach`),
    // an `is_svg_presentational_element` ancestor like `<g>` CAN collide:
    // two structurally identical `<g class="wrap">` siblings under the same
    // live `<svg>` parent get the same `ShareKey` and so the same cached
    // `Arc`, which their own children then inherit as `inherited_ptr` too.
    // `.wrap:hover` sitting on that ancestor is still a genuinely dynamic,
    // per-instance fact the key does not pin — hovering only the FIRST `<g>`
    // must not leak its `:hover`-styled fill onto the second `<g>`'s icon via
    // a wrongly-shared cache entry. The rescue in `ancestor_prefix_could_
    // rescue` must find this ancestor reachable (real `<g class="wrap">`
    // elements exist) and leave the selector unsafe.
    clear_shadow_sheets();
    let doc = lumen_html_parser::parse(concat!(
        "<svg>",
        r#"<g class="wrap"><path class="octicon" d="M1 1"></path></g>"#,
        r#"<g class="wrap"><path class="octicon" d="M1 1"></path></g>"#,
        "</svg>",
    ));
    let flat = lumen_dom::build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse(
        ".octicon { fill: rgb(1, 2, 3); } .wrap:hover .octicon { fill: rgb(0, 0, 255); }",
    );
    let svg = doc.get(doc.body().unwrap()).children[0];
    let wraps: Vec<NodeId> = doc.get(svg).children.clone();
    assert_eq!(wraps.len(), 2);
    let icons: Vec<NodeId> = wraps.iter().map(|&w| doc.get(w).children[0]).collect();

    crate::set_interactive_state(Some(wraps[0]), None, None);
    let map = crate::counters::precompute_counters(&doc, &sheet, VP, &flat, false);
    crate::clear_interactive_state();

    assert_eq!(
        map.style_for(icons[0]).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 0, g: 0, b: 255, a: 255 }),
        "hovered wrapper's icon must get the :hover fill"
    );
    assert_eq!(
        map.style_for(icons[1]).expect("style").svg_fill,
        SvgPaint::Color(Color { r: 1, g: 2, b: 3, a: 255 }),
        "un-hovered wrapper's icon must NOT leak the other one's :hover fill via a shared cache entry"
    );
}
