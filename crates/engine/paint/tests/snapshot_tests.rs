/// Snapshot-тесты для display list.
///
/// Каждый тест сериализует display list в текст и сравнивает с golden-файлом
/// в `tests/snapshots/`. При первом запуске golden-файлов ещё нет — нужно
/// задать `UPDATE_SNAPSHOTS=1` чтобы сгенерировать их:
///
///   UPDATE_SNAPSHOTS=1 cargo test -p lumen-paint --test snapshot_tests
///
/// Последующие запуски без флага будут проверять на совпадение.
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

// ── helpers ──────────────────────────────────────────────────────────────────

fn build(html: &str, css: &str, width: f32) -> String {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = lumen_layout::layout(&doc, &sheet, Size::new(width, 600.0));
    let dl = lumen_paint::build_display_list(&tree);
    lumen_paint::serialize_display_list(&dl)
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
    let dl = lumen_paint::build_display_list(&tree);
    lumen_paint::serialize_display_list(&dl)
}

// ── тесты ────────────────────────────────────────────────────────────────────

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
fn paragraph_with_background() {
    let actual = build("<p>x</p>", "p { background: red; }", 800.0);
    assert_snapshot("paragraph_with_background", &actual);
}

#[test]
fn nested_blocks_paint_order() {
    let actual = build(
        "<div><p>x</p></div>",
        "div { background: red; } p { background: blue; }",
        800.0,
    );
    assert_snapshot("nested_blocks_paint_order", &actual);
}

#[test]
fn cyrillic_text() {
    let actual = build("<p>Привет</p>", "", 800.0);
    assert_snapshot("cyrillic_text", &actual);
}

/// line_wrap_two_lines использует Fixed8 (8px/символ) и viewport 60px,
/// так что "hello world" (88px) не влезает в одну строку.
#[test]
fn line_wrap_two_lines() {
    let actual = build_measured("<p>hello world</p>", "", 60.0);
    assert_snapshot("line_wrap_two_lines", &actual);
}
