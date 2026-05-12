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
