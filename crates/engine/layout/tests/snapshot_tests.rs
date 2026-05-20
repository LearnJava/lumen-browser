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

fn build(html: &str, css: &str, width: f32) -> String {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = lumen_layout::layout(&doc, &sheet, Size::new(width, 600.0));
    lumen_layout::serialize_layout_tree(&tree)
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
    lumen_layout::serialize_layout_tree(&tree)
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
