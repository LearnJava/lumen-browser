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
fn a_combinator_rule_disables_sharing_for_the_nodes_it_could_reach() {
    // Two icons, identical tag/attrs, under differently-classed ancestors —
    // `.blue .octicon` can only be decided by walking each one's own DOM
    // ancestors, which the structural key does not model. If sharing fired
    // here anyway, one icon would silently render the other's fill.
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
