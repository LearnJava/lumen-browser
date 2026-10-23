//! `@container scroll-state(…)` — разбор `container-type`, оценка условий, `scrollable`.

use crate::box_tree::LayoutBox;
use crate::selector_query::find_box_by_selector;
use crate::style::ContainerType;
use lumen_core::geom::Size;

fn layout_tree(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = crate::layout(&doc, &sheet, Size::new(1024.0, 600.0));
    (doc, tree)
}

fn custom(tree: &LayoutBox, doc: &lumen_dom::Document, sel: &str, name: &str) -> String {
    let b = find_box_by_selector(tree, doc, sel).expect("box");
    b.style.custom_props.get(name).cloned().unwrap_or_default()
}

fn container_type_of(css: &str) -> (ContainerType, bool) {
    let (doc, tree) = layout_tree(r#"<div id="c"></div>"#, css);
    let b = find_box_by_selector(&tree, &doc, "#c").expect("box");
    (b.style.container_type, b.style.container_scroll_state)
}

#[test]
fn container_type_scroll_state_grammar() {
    assert_eq!(container_type_of("#c{container-type:scroll-state}"), (ContainerType::Normal, true));
    assert_eq!(container_type_of("#c{container-type:size scroll-state}"), (ContainerType::Size, true));
    assert_eq!(container_type_of("#c{container-type:scroll-state inline-size}"), (ContainerType::InlineSize, true));
    assert_eq!(container_type_of("#c{container-type:inline-size}"), (ContainerType::InlineSize, false));
    // Недопустимое значение объявление не меняет.
    assert_eq!(container_type_of("#c{container-type:size;container-type:scroll-state scroll-state}"), (ContainerType::Size, false));
    assert_eq!(container_type_of("#c{container-type:normal scroll-state}"), (ContainerType::Normal, false));
    assert_eq!(container_type_of("#c{container-type:size inline-size}"), (ContainerType::Normal, false));
    assert_eq!(container_type_of("#c{container:n / scroll-state}"), (ContainerType::Normal, true));
}

const SCROLLER: &str = "#s { container-type: scroll-state; overflow: auto; width: 100px; height: 50px }
     #t { --top: no; --bottom: no; --none: no; --any: no }
     @container scroll-state(scrollable: top) { #t { --top: yes } }
     @container scroll-state(scrollable: bottom) { #t { --bottom: yes } }
     @container scroll-state(scrollable: none) { #t { --none: yes } }
     @container scroll-state(scrollable) { #t { --any: yes } }";

#[test]
fn scrollable_bottom_when_content_overflows() {
    let (doc, tree) = layout_tree(
        r#"<div id="s"><div id="t"></div><div style="height:300px"></div></div>"#,
        SCROLLER,
    );
    assert_eq!(custom(&tree, &doc, "#t", "--bottom"), "yes");
    assert_eq!(custom(&tree, &doc, "#t", "--top"), "no");
    assert_eq!(custom(&tree, &doc, "#t", "--none"), "no");
    assert_eq!(custom(&tree, &doc, "#t", "--any"), "yes");
}

#[test]
fn not_scrollable_without_overflow_or_user_scrolling() {
    let (doc, tree) = layout_tree(r#"<div id="s"><div id="t"></div></div>"#, SCROLLER);
    assert_eq!(custom(&tree, &doc, "#t", "--none"), "yes");
    assert_eq!(custom(&tree, &doc, "#t", "--any"), "no");
    // `overflow: clip` и `hidden` пользователь не прокручивает.
    for ov in ["clip", "hidden"] {
        let css = format!("{SCROLLER} #s {{ overflow: {ov} }}");
        let (doc, tree) = layout_tree(
            r#"<div id="s"><div id="t"></div><div style="height:300px"></div></div>"#,
            &css,
        );
        assert_eq!(custom(&tree, &doc, "#t", "--none"), "yes", "overflow: {ov}");
        assert_eq!(custom(&tree, &doc, "#t", "--bottom"), "no", "overflow: {ov}");
    }
}

#[test]
fn unknown_condition_and_its_negation_both_fail() {
    let css = "#s { container-type: scroll-state; width: 100px }
         #t { --m: no }
         @container (scrollable: left) or (not (scrollable: left)) { #t { --m: yes } }
         @container scroll-state(scrollable: auto) or (not (scroll-state(scrollable: auto))) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="s"><div id="t"></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "no");
}

#[test]
fn known_condition_or_its_negation_applies() {
    let css = "#s { container-type: scroll-state; width: 100px }
         #t { --m: no }
         @container (scroll-state(scrollable)) or (not (scroll-state(scrollable))) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="s"><div id="t"></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "yes");
}

#[test]
fn query_picks_container_by_kind_through_the_chain() {
    // Размерный запрос минует ближайший scroll-state контейнер и берёт внешний size.
    let css = "#o { container-type: inline-size; width: 400px }
         #s { container-type: scroll-state }
         #t { --w: no }
         @container (min-width: 300px) { #t { --w: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="s"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--w"), "yes");
}

#[test]
fn named_query_skips_unnamed_nearer_container() {
    let css = "#o { container: outer / inline-size; width: 400px }
         #s { container-type: inline-size; width: 100px }
         #t { --w: no }
         @container outer (min-width: 300px) { #t { --w: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="s"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--w"), "yes");
}

#[test]
fn comma_list_matches_any_entry_with_its_own_container_name() {
    let css = "#o { container: --outer / scroll-state; overflow: scroll; width: 100px; height: 100px }
         #i { container-type: scroll-state; width: 50px; height: 400px }
         #t { --m: no }
         @container scroll-state(scrollable: right), --outer scroll-state(scrollable: bottom) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="i"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "yes");
}

#[test]
fn one_container_must_answer_every_feature_kind_of_the_query() {
    // `#c` is size + scroll-state and scrollable; the nearer `#i` is scroll-state only.
    let css = "#c { container-type: inline-size scroll-state; overflow: scroll; width: 100px; height: 100px }
         #i { container-type: scroll-state; width: 50px; height: 50px }
         #t { --m: no }
         @container (inline-size) and scroll-state(scrollable: bottom) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(
        r#"<div id="c"><div id="i"><div id="t"></div></div><div style="height:400px"></div></div>"#,
        css,
    );
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "yes");
}

fn width_ctx(kinds: crate::ContainerKinds, height: Option<f32>) -> crate::ContainerContext {
    let mut ctx = crate::style_ctx(&[]);
    ctx.kinds = kinds;
    ctx.height = height;
    ctx.width = 200.0;
    ctx
}

#[test]
fn range_syntax_and_unsupported_axis() {
    let size = width_ctx(crate::ContainerKinds::SIZE, Some(100.0));
    let inline_only = width_ctx(crate::ContainerKinds::SIZE, None);
    let q = |c: &str, ctx: &crate::ContainerContext| crate::evaluate_container_condition(c, ctx);
    assert!(q("(width > 100px)", &size));
    assert!(q("(100px < width < 300px)", &size));
    assert!(!q("(300px <= width)", &size));
    assert!(q("(inline-size >= 200px)", &size));
    assert!(q("(inline-size)", &size));
    // Блоковой оси у inline-size контейнера нет: условие по ней «unknown», и его отрицание тоже.
    assert!(!q("(height > 0px)", &inline_only));
    assert!(!q("not (height < 0px)", &inline_only));
    assert!(!q("not (orientation)", &inline_only));
    assert!(q("not (height < 0px)", &size));
}

/// Custom properties `collect_custom_properties_for` publishes, with the box-less elements.
fn custom_with_counters(html: &str, css: &str, sel: &str, name: &str) -> String {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let viewport = Size::new(1024.0, 600.0);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, counters) = crate::layout_measured_hyp_with_counters(&doc, &sheet, viewport, &crate::Fixed8, &null_hp, false);
    let id = crate::selector_query::find_first_dom_node_by_selector(&doc, sel).expect("node");
    let maps = crate::collect_custom_properties_for(&tree, Some(&counters), viewport);
    maps.get(&(id.index() as u32)).and_then(|m| m.get(name)).cloned().unwrap_or_default()
}

#[test]
fn boxless_element_gets_container_rules_and_publishes_custom_properties() {
    let css = "#o { container-type: inline-size; width: 400px }
         span { --m: no }
         @container (min-width: 300px) { #t { --m: yes } }";
    let html = r#"<div id="o"><span id="t"></span><span id="u"></span></div>"#;
    assert_eq!(custom_with_counters(html, css, "#t", "--m"), "yes");
    assert_eq!(custom_with_counters(html, css, "#u", "--m"), "no");
}

#[test]
fn boxless_container_answers_no_query_below_it() {
    let css = "#o { container-type: inline-size; width: 400px }
         #n { display: none; container-type: inline-size }
         #t { --m: no }
         @container (min-width: 0px) { #t { --m: yes } }";
    let html = r#"<div id="o"><div id="n"><span id="t"></span></div></div>"#;
    assert_eq!(custom_with_counters(html, css, "#t", "--m"), "no");
}

#[test]
fn block_axis_query_skips_inline_size_container() {
    // `.d` не отдаёт высоту: запрос по `height` идёт к `.c` (container-type: size).
    let css = ".c { container-type: size; width: 200px; height: 100px }
         .d { container-type: inline-size }
         #t { --h: no }
         @container (min-height: 50px) { #t { --h: yes } }";
    let (doc, tree) = layout_tree(r#"<div class="c"><div class="d"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--h"), "yes");
}

#[test]
fn block_axis_query_without_size_container_is_false() {
    let css = ".c { container-type: inline-size; width: 200px; height: 100px }
         #t { --h: no }
         @container (min-height: 50px) { #t { --h: yes } }";
    let (doc, tree) = layout_tree(r#"<div class="c"><div id="t"></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--h"), "no");
}

#[test]
fn query_over_both_axes_needs_one_container_with_both() {
    let css = ".c { container-type: size; width: 200px; height: 100px }
         .d { container-type: inline-size }
         #t { --m: no }
         @container ((width > 0px) and (height > 0px)) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div class="c"><div class="d"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "yes");
}

#[test]
fn ineligible_nearest_container_makes_size_query_unknown() {
    // `#i` — display: table не берёт size containment: запрос остаётся за ним и ложен,
    // к внешнему `#o` не переходит.
    let css = "#o, #i { container-type: inline-size; width: 200px }
         #i { display: table }
         #t { --m: no }
         @container (min-width: 1px) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="i"><p id="t"></p></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "no");
}

#[test]
fn display_contents_container_in_the_middle_hides_outer_one() {
    let css = ".c { container-type: inline-size; width: 30px }
         #m { display: contents }
         #t { --m: no }
         @container (width: 30px) { #t { --m: yes } }";
    let (doc, tree) = layout_tree(
        r#"<div class="c"><div><div id="m" class="c"><div><div id="t"></div></div></div></div></div>"#,
        css,
    );
    assert_eq!(custom(&tree, &doc, "#t", "--m"), "no");
}

#[test]
fn nested_container_rule_needs_enclosing_query_too() {
    let css = "#o { container: outer / size; width: 100px; height: 100px }
         #i { container-type: size; width: 50px; height: 50px }
         #t { --a: no; --b: no }
         @container outer (width: 70px) { @container (height: 50px) { #t { --a: yes } } }
         @container outer (width: 100px) { @container (height: 50px) { #t { --b: yes } } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="i"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--a"), "no");
    assert_eq!(custom(&tree, &doc, "#t", "--b"), "yes");
}

#[test]
fn name_only_query_matches_a_container_name_without_type() {
    let css = "#o { container-name: --bar } #i { container-name: --foo }
         #t { --f: no; --b: no; --z: no }
         @container --foo { #t { --f: yes } }
         @container --bar { #t { --b: yes } }
         @container --baz { #t { --z: yes } }";
    let (doc, tree) = layout_tree(r#"<div id="o"><div id="i"><div id="t"></div></div></div>"#, css);
    assert_eq!(custom(&tree, &doc, "#t", "--f"), "yes");
    assert_eq!(custom(&tree, &doc, "#t", "--b"), "yes");
    assert_eq!(custom(&tree, &doc, "#t", "--z"), "no");
}
