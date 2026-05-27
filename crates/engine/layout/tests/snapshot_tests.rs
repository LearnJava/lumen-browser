//! Snapshot-тесты для layout-дерева.
//!
//! Каждый тест прогоняет HTML+CSS через `lumen-layout::layout`, сериализует
//! результат и сравнивает с golden-файлом в `tests/snapshots/`. При первом
//! запуске или после намеренного изменения формата нужно задать
//! `UPDATE_SNAPSHOTS=1` чтобы пересоздать файлы:
//!
//!   UPDATE_SNAPSHOTS=1 cargo test -p lumen-layout --test snapshot_tests
//!
//! Без флага — проверка на побайтовое совпадение.

use lumen_core::geom::Size;

fn assert_snapshot(name: &str, actual: &str) {
    let snap_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots");
    let snap_path = snap_dir.join(format!("{name}.snap"));

    let update = std::env::var("UPDATE_SNAPSHOTS")
        .map(|v| v == "1")
        .unwrap_or(false);

    if update {
        std::fs::create_dir_all(&snap_dir).expect("create snapshots dir");
        std::fs::write(&snap_path, actual).expect("write snapshot");
        return;
    }

    if !snap_path.exists() {
        panic!(
            "Snapshot '{name}' not found at {}.\nRun with UPDATE_SNAPSHOTS=1 to create it.",
            snap_path.display()
        );
    }

    let expected = std::fs::read_to_string(&snap_path).expect("read snapshot");
    if actual != expected.as_str() {
        panic!(
            "Snapshot '{name}' mismatch.\n\
             --- expected ---\n{expected}\
             --- actual ---\n{actual}"
        );
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Extracts the `<body>` layout box from the full layout tree.
/// The HTML5 parser wraps content in `document → html → head + body`,
/// so `layout()` returns a tree rooted at the document block, not the body.
/// Tests work at body level, so we strip the wrappers here.
fn body_layout_box(mut root: lumen_layout::LayoutBox) -> lumen_layout::LayoutBox {
    use lumen_layout::BoxKind;
    if let Some(html_idx) = root.children.iter()
        .position(|c| matches!(c.kind, BoxKind::Block)) {
        let mut html_box = root.children.remove(html_idx);
        if let Some(body_idx) = html_box.children.iter()
            .position(|c| matches!(c.kind, BoxKind::Block)) {
            return html_box.children.remove(body_idx);
        }
        return html_box;
    }
    root
}

fn build(html: &str, css: &str, width: f32) -> String {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = lumen_layout::layout(&doc, &sheet, Size::new(width, 600.0));
    lumen_layout::serialize_layout_tree(&body_layout_box(tree))
}

struct Fixed8;
impl lumen_layout::TextMeasurer for Fixed8 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        8.0
    }
}

fn build_measured(html: &str, css: &str, width: f32) -> String {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8);
    lumen_layout::serialize_layout_tree(&body_layout_box(tree))
}

// ── тесты ───────────────────────────────────────────────────────────────────

#[test]
fn empty_page() {
    let actual = build("", "", 800.0);
    assert_snapshot("empty_page", &actual);
}

#[test]
fn single_paragraph() {
    let actual = build("<p>hello</p>", "", 800.0);
    assert_snapshot("single_paragraph", &actual);
}

#[test]
fn paragraph_with_styles() {
    let actual = build(
        "<p>x</p>",
        "p { color: red; background: blue; padding: 10px; margin: 5px; font-size: 20px; }",
        800.0,
    );
    assert_snapshot("paragraph_with_styles", &actual);
}

#[test]
fn nested_blocks() {
    let actual = build(
        "<div><p>nested</p></div>",
        "div { background: red; } p { background: blue; }",
        800.0,
    );
    assert_snapshot("nested_blocks", &actual);
}

#[test]
fn inline_flow_with_styled_link() {
    // Текст + <a> с собственным цветом — в один InlineRun, два сегмента.
    let actual = build(
        "<p>before <a>link</a> after</p>",
        "a { color: blue; }",
        800.0,
    );
    assert_snapshot("inline_flow_with_styled_link", &actual);
}

#[test]
fn line_wrap_two_lines() {
    // "hello world" = 88px при Fixed8, viewport 60px → две строки.
    let actual = build_measured("<p>hello world</p>", "", 60.0);
    assert_snapshot("line_wrap_two_lines", &actual);
}

#[test]
fn cyrillic_paragraph() {
    let actual = build("<p>Привет, мир</p>", "p { color: green; }", 800.0);
    assert_snapshot("cyrillic_paragraph", &actual);
}

#[test]
fn multiple_paragraphs_stack() {
    let actual = build("<p>a</p><p>b</p><p>c</p>", "", 800.0);
    assert_snapshot("multiple_paragraphs_stack", &actual);
}

#[test]
fn display_none_skipped() {
    let actual = build(
        r#"<p>visible</p><p class="hidden">x</p>"#,
        ".hidden { display: none; }",
        800.0,
    );
    assert_snapshot("display_none_skipped", &actual);
}

#[test]
fn nth_child_odd_applies() {
    let actual = build(
        "<p>a</p><p>b</p><p>c</p>",
        "p:nth-child(odd) { background: red; }",
        800.0,
    );
    assert_snapshot("nth_child_odd_applies", &actual);
}

#[test]
fn not_class_excludes() {
    let actual = build(
        r#"<p>a</p><p class="hl">b</p><p>c</p>"#,
        "p:not(.hl) { color: red; }",
        800.0,
    );
    assert_snapshot("not_class_excludes", &actual);
}

#[test]
fn descendant_combinator() {
    let actual = build(
        "<div><p>x</p></div><p>y</p>",
        "div p { background: yellow; }",
        800.0,
    );
    assert_snapshot("descendant_combinator", &actual);
}

#[test]
fn text_decoration_underline_on_link() {
    // Underline на `<a>` — фрагмент "link" должен получить decoration=underline,
    // соседний текст в `<p>` — без декорации (не наследуется через дочерний).
    let actual = build(
        "<p>before <a>link</a> after</p>",
        "a { text-decoration: underline; }",
        800.0,
    );
    assert_snapshot("text_decoration_underline_on_link", &actual);
}

#[test]
fn border_solid_all_sides() {
    let actual = build(
        "<p>x</p>",
        "p { border: 4px solid blue; }",
        800.0,
    );
    assert_snapshot("border_solid_all_sides", &actual);
}

#[test]
fn border_top_only() {
    let actual = build(
        "<p>x</p>",
        "p { border-top: 2px solid red; }",
        800.0,
    );
    assert_snapshot("border_top_only", &actual);
}

#[test]
fn box_sizing_border_box_with_padding_border() {
    // border-box: width=100 включает padding 10 и border 2 — rect.width = 100,
    // дочерний контент сжимается до 100 - 10*2 - 2*2 = 76px.
    let actual = build(
        "<p>x</p>",
        "p { width: 100px; padding: 10px; border: 2px solid black; box-sizing: border-box; }",
        800.0,
    );
    assert_snapshot("box_sizing_border_box_with_padding_border", &actual);
}

#[test]
fn css_var_substitution_in_inherited_property() {
    // CSS Variables L1: --c определён на <body>, inherited custom property
    // виден у <p>; var(--c) разворачивается в red и применяется к color.
    // Custom property declaration сама в snapshot не печатается — формат
    // serialize_layout_tree её игнорирует (она в .custom_props, а не в
    // resolved style fields).
    let actual = build(
        "<body><p>x</p></body>",
        "body { --c: red; } p { color: var(--c); }",
        800.0,
    );
    assert_snapshot("css_var_substitution_in_inherited_property", &actual);
}

#[test]
fn img_replaced_element() {
    // <img> создаёт BoxKind::Image с src/alt; width/height из HTML-атрибутов
    // применяются как presentational hints, CSS перекрывает их.
    let actual = build(
        r#"<p>before</p><img src="logo.png" alt="Lumen logo" width="100" height="40"><p>after</p>"#,
        "",
        800.0,
    );
    assert_snapshot("img_replaced_element", &actual);
}

// ── Flex layout (4B.3) ───────────────────────────────────────────────────────

#[test]
fn flex_row_equal_children() {
    // Flex-grow:1 на трёх детях → каждый получает треть ширины 900px.
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div > div { flex-grow: 1; height: 50px; background: red; } \
         div { display: flex; width: 900px; height: 100px; }",
        1000.0,
    );
    assert_snapshot("flex_row_equal_children", &actual);
}

#[test]
fn flex_row_explicit_basis() {
    // flex-basis: 100px для двух детей в контейнере 400px → free space = 200px.
    let actual = build(
        "<div><div></div><div></div></div>",
        "div { display: flex; width: 400px; height: 60px; } \
         div > div { flex-basis: 100px; flex-grow: 1; height: 40px; }",
        500.0,
    );
    assert_snapshot("flex_row_explicit_basis", &actual);
}

#[test]
fn flex_column_children() {
    // flex-direction: column → дети стэкаются вертикально.
    let actual = build(
        "<div><div></div><div></div></div>",
        "div { display: flex; flex-direction: column; width: 200px; } \
         div > div { height: 30px; background: blue; }",
        400.0,
    );
    assert_snapshot("flex_column_children", &actual);
}

#[test]
fn flex_justify_content_center() {
    // justify-content: center → items сдвинуты к середине.
    let actual = build(
        "<div><div></div></div>",
        "div { display: flex; justify-content: center; width: 400px; } \
         div > div { width: 100px; height: 50px; }",
        500.0,
    );
    assert_snapshot("flex_justify_content_center", &actual);
}

#[test]
fn flex_justify_content_space_between() {
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; justify-content: space-between; width: 600px; height: 50px; } \
         div > div { width: 100px; height: 50px; }",
        700.0,
    );
    assert_snapshot("flex_justify_content_space_between", &actual);
}

#[test]
fn flex_row_gap() {
    // 3 × 100px items in 500px container with column-gap: 50px → items at x=0,150,300
    // free_space = 500 - 300 (items) - 100 (2×50 gap) = 100, no flex-grow → items use initial sizes
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; column-gap: 50px; width: 500px; height: 50px; } \
         div > div { width: 100px; height: 50px; }",
        600.0,
    );
    assert_snapshot("flex_row_gap", &actual);
}

#[test]
fn flex_column_gap() {
    // 3 items stacked in column with row-gap: 20px
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; flex-direction: column; row-gap: 20px; width: 100px; } \
         div > div { width: 100px; height: 30px; }",
        200.0,
    );
    assert_snapshot("flex_column_gap", &actual);
}

#[test]
fn flex_gap_with_grow() {
    // 2 items with gap: 20px and flex-grow: 1 each in 200px container
    // available for grow = 200 - 0 (items have 0 initial width via flex shorthand) - 20 (gap) = 180
    // each item gets 90px
    let actual = build(
        "<div><div></div><div></div></div>",
        "div { display: flex; gap: 20px; width: 200px; height: 50px; } \
         div > div { flex: 1; height: 50px; }",
        300.0,
    );
    assert_snapshot("flex_gap_with_grow", &actual);
}

// ── Flex wrap (4B.5) ─────────────────────────────────────────────────────────

#[test]
fn flex_wrap_two_lines() {
    // 3 × 200px items in 500px container with flex-wrap: wrap
    // Line 1: items 1+2 (400px total < 500px, item 3 doesn't fit: 400+200=600 > 500)
    // Line 2: item 3
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; flex-wrap: wrap; width: 500px; } \
         div > div { width: 200px; height: 40px; }",
        600.0,
    );
    assert_snapshot("flex_wrap_two_lines", &actual);
}

#[test]
fn flex_wrap_reverse() {
    // Same as above but wrap-reverse: line 2 appears at top, line 1 at bottom
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; flex-wrap: wrap-reverse; width: 500px; } \
         div > div { width: 200px; height: 40px; }",
        600.0,
    );
    assert_snapshot("flex_wrap_reverse", &actual);
}

#[test]
fn flex_wrap_with_row_gap() {
    // 3 items wrapping onto 2 lines with row-gap: 10px between lines
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; flex-wrap: wrap; width: 500px; row-gap: 10px; } \
         div > div { width: 200px; height: 40px; }",
        600.0,
    );
    assert_snapshot("flex_wrap_with_row_gap", &actual);
}

#[test]
fn flex_wrap_grow_per_line() {
    // 3 items (basis=120px each) with flex-grow:1 in 300px container.
    // Line 1: items 1+2 = 240px < 300, item 3 doesn't fit (240+120=360>300).
    //   free_space = 300-240 = 60px; each of 2 items grows by 30 → 150px each.
    // Line 2: item 3 alone, grows to fill whole 300px.
    let actual = build(
        "<div><div></div><div></div><div></div></div>",
        "div { display: flex; flex-wrap: wrap; width: 300px; } \
         div > div { flex-grow: 1; width: 120px; height: 50px; }",
        400.0,
    );
    assert_snapshot("flex_wrap_grow_per_line", &actual);
}

// ── position: relative ───────────────────────────────────────────────────────

/// position:relative left:20px shifts the block 20px right from its normal-flow origin.
#[test]
fn position_relative_left_offset() {
    let actual = build(
        "<div></div>",
        "div { position: relative; left: 20px; width: 50px; height: 10px; }",
        800.0,
    );
    // After shift_tree: div rect.x = 20 (offset from normal-flow x=0).
    assert!(actual.contains("x=20.00") || actual.contains("(20.00,"), "left:20px must shift rect.x to 20");
}

/// position:relative top:15px shifts the block 15px down from its normal-flow origin.
#[test]
fn position_relative_top_offset() {
    let actual = build(
        "<div></div>",
        "div { position: relative; top: 15px; width: 50px; height: 10px; }",
        800.0,
    );
    assert!(actual.contains("y=15.00") || actual.contains(", 15.00,"), "top:15px must shift rect.y");
}

/// position:static (default) produces no offset — baseline check.
#[test]
fn position_static_no_offset() {
    let actual = build(
        "<div></div>",
        "div { width: 50px; height: 10px; }",
        800.0,
    );
    // Should NOT contain any x=20.00 or similar
    assert!(!actual.contains("x=20.00"), "static position must not shift rect");
}

// ── BUG-004 regression: height on inline / inline-block / block ─────────────

/// BUG-004: `display:inline-block; height:40px` must produce a 40px-tall box.
/// Earlier the project had no inline-block support, so authors couldn't apply
/// height to a span-like element at all. After inline-block was wired up,
/// `height` must take effect (CSS 2.1 §10.6.2 covers replaced/inline-block).
#[test]
fn bug_004_inline_block_height_applies() {
    let actual = build(
        r#"<span class="x">a</span>"#,
        ".x { display: inline-block; height: 40px; width: 80px; }",
        800.0,
    );
    assert!(
        actual.contains("(0.00, 0.00, 80.00, 40.00)") && actual.contains("display=inline-block"),
        "expected an 80x40 inline-block box, got:\n{actual}"
    );
}

/// BUG-004 spec-correctness: `display:inline; height:40px` must be ignored.
/// CSS 2.1 §10.6.1: "Properties 'height', 'min-height', and 'max-height' do
/// not apply to non-replaced inline elements." The inline run height must
/// follow line-height, not the explicit `height` declaration.
#[test]
fn bug_004_inline_height_ignored_per_spec() {
    let actual = build(
        r#"<p><span class="x">a</span></p>"#,
        ".x { display: inline; height: 40px; }",
        800.0,
    );
    // No inline box should adopt the 40px height — its row must be
    // line-height-driven (well under 40px for the default font-size).
    assert!(
        !actual.contains(", 40.00)") && !actual.contains("h=40px"),
        "height must not apply to display:inline, got:\n{actual}"
    );
}

/// BUG-004 baseline: `display:block; height:60px` on a `<div>` applies.
/// Ensures the regression suite covers all three display flavors.
#[test]
fn bug_004_block_height_applies() {
    let actual = build(
        "<div></div>",
        "div { width: 100px; height: 60px; }",
        800.0,
    );
    assert!(
        actual.contains("(0.00, 0.00, 100.00, 60.00)"),
        "expected a 100x60 block, got:\n{actual}"
    );
}
