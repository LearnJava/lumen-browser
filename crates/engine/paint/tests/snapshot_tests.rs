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

#[test]
fn img_replaced_element() {
    // DrawImage с src/alt и размерами 100×40 из HTML атрибутов.
    let actual = build(
        r#"<img src="logo.png" alt="Lumen logo" width="100" height="40">"#,
        "",
        800.0,
    );
    assert_snapshot("img_replaced_element", &actual);
}

#[test]
fn img_with_background_and_border() {
    // Painter's order: FillRect (background) → DrawBorder → DrawImage.
    // Размер коробки 50+2×2=54 в обе стороны из-за border-2px.
    let actual = build(
        r#"<img src="x.png" width="50" height="50">"#,
        "img { background: navy; border: 2px solid red; }",
        800.0,
    );
    assert_snapshot("img_with_background_and_border", &actual);
}

#[test]
fn img_inside_span() {
    // <img> inside <span> must produce DrawImage in inline flow (BUG-005).
    let actual = build(
        r#"<span><img src="inline.png" alt="icon" width="32" height="32"></span>"#,
        "",
        800.0,
    );
    assert_snapshot("img_inside_span", &actual);
}

// ── DrawLayerSnapshot serialization ─────────────────────────────────────────

#[test]
fn layer_snapshot_serializes() {
    use lumen_core::geom::Rect;
    use lumen_paint::DisplayCommand;
    // Проверяем, что DrawLayerSnapshot сериализуется в ожидаемый формат.
    let dl: Vec<DisplayCommand> = vec![DisplayCommand::DrawLayerSnapshot {
        id: 42,
        rect: Rect::new(10.0, 20.0, 100.0, 80.0),
        alpha: 0.75,
    }];
    let s = lumen_paint::serialize_display_list(&dl);
    assert_snapshot("layer_snapshot_serializes", &s);
}

#[test]
fn layer_snapshot_zero_alpha_is_transparent() {
    use lumen_core::geom::Rect;
    use lumen_paint::DisplayCommand;
    // alpha=0.0 — полностью прозрачный снимок.
    let dl = vec![DisplayCommand::DrawLayerSnapshot {
        id: 0,
        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
        alpha: 0.0,
    }];
    let s = lumen_paint::serialize_display_list(&dl);
    assert!(s.contains("alpha=0.000"), "alpha=0 must serialize correctly");
}

#[test]
fn layer_snapshot_full_alpha_is_opaque() {
    use lumen_core::geom::Rect;
    use lumen_paint::DisplayCommand;
    // alpha=1.0 — непрозрачный снимок.
    let dl = vec![DisplayCommand::DrawLayerSnapshot {
        id: 999,
        rect: Rect::new(5.0, 15.0, 200.0, 150.0),
        alpha: 1.0,
    }];
    let s = lumen_paint::serialize_display_list(&dl);
    assert!(s.contains("id=999"), "must contain snapshot id");
    assert!(s.contains("alpha=1.000"), "must contain alpha");
    assert!(s.contains("DrawLayerSnapshot"), "must contain command name");
}

// ── text-overflow: ellipsis ──────────────────────────────────────────────────

/// "HelloWorld" = 10 chars * 8px = 80px > 60px container.
/// overflow:hidden + text-overflow:ellipsis: layout truncates to "HelloW…",
/// block walk emits PushClipRect around the InlineRun child.
#[test]
fn text_overflow_ellipsis_clips_overflowing_line() {
    let actual = build_measured(
        "<div>HelloWorld</div>",
        "div { width: 60px; overflow: hidden; text-overflow: ellipsis; }",
        800.0,
    );
    assert!(actual.contains('\u{2026}'), "ellipsis char must be emitted");
    assert!(actual.contains("PushClipRect"), "overflow:hidden must emit PushClipRect");
    let push_pos = actual.find("PushClipRect").expect("PushClipRect must appear");
    let ell_pos = actual.find('\u{2026}').expect("ellipsis char must appear");
    let pop_pos = actual.find("PopClip").expect("PopClip must appear");
    assert!(push_pos < ell_pos, "PushClipRect must precede ellipsis");
    assert!(ell_pos < pop_pos, "PopClip must follow ellipsis");
}

/// "Hi" = 2 * 8px = 16px < 60px — no overflow, no ellipsis, no extra clip.
#[test]
fn text_overflow_ellipsis_no_clip_when_text_fits() {
    let actual = build_measured(
        "<div>Hi</div>",
        "div { width: 60px; overflow: hidden; text-overflow: ellipsis; }",
        800.0,
    );
    assert!(!actual.contains('\u{2026}'), "no ellipsis when text fits in container");
}

/// Without overflow:hidden, text-overflow:ellipsis must not trigger.
#[test]
fn text_overflow_ellipsis_requires_overflow_hidden() {
    let actual = build_measured(
        "<div>HelloWorld</div>",
        "div { width: 60px; text-overflow: ellipsis; }",
        800.0,
    );
    assert!(!actual.contains('\u{2026}'), "ellipsis must not appear without overflow:hidden");
}

// ── text-decoration-thickness ────────────────────────────────────────────────

/// text-decoration-thickness: 5px → underline FillRect height = 5.00.
/// Verifies that resolve_decoration_thickness() applies the Length variant.
#[test]
fn text_decoration_thickness_custom_length() {
    let actual = build_measured(
        "<span>underline</span>",
        "span { text-decoration: underline; text-decoration-thickness: 5px; color: red; }",
        800.0,
    );
    // Underline is a FillRect; height field should be "5.00" for 5px thickness.
    assert!(actual.contains("FillRect"), "underline must produce a FillRect");
    assert!(actual.contains("5.00"), "5px thickness must appear in FillRect dimensions");
}

/// text-decoration-thickness: auto → default thickness ≈ 7% of font-size ≥ 1px.
/// At default 16px font-size: 16 * 0.07 = 1.12px → rounds to 1.12.
#[test]
fn text_decoration_thickness_auto_uses_em_fraction() {
    let actual = build_measured(
        "<span>x</span>",
        "span { text-decoration: underline; text-decoration-thickness: auto; }",
        800.0,
    );
    assert!(actual.contains("FillRect"), "underline must produce a FillRect");
    // 16px * 0.07 = 1.12; check thickness is not 5px (i.e., not using wrong branch).
    assert!(!actual.contains("5.00"), "auto thickness must not be 5px");
}

// ── overflow: hidden clip ─────────────────────────────────────────────────────

/// overflow: hidden on a block emits PushClipRect / child / PopClip.
/// Child's FillRect must appear between the two clip commands.
#[test]
fn overflow_hidden_clips_children() {
    // Container 160×100; child 220×140 — overflows in both axes.
    let actual = build(
        r#"<div style="width:160px;height:100px;overflow:hidden;background:#0000ff">
             <div style="width:220px;height:140px;background:#ff0000"></div>
           </div>"#,
        "",
        800.0,
    );
    let push_pos = actual.find("PushClipRect").expect("overflow:hidden → PushClipRect");
    let pop_pos = actual.find("PopClip").expect("overflow:hidden → PopClip");
    // Child red rect appears between push and pop.
    let red_pos = actual.find("ff0000").expect("child background must be in DL");
    assert!(push_pos < red_pos, "PushClipRect must precede child");
    assert!(red_pos < pop_pos, "PopClip must follow child");
    // Clip rect width = 160px (no border on container).
    assert!(actual.contains("160.00"), "clip rect width must equal container width");
}

/// overflow: visible (default) must NOT emit any PushClipRect.
#[test]
fn overflow_visible_no_clip() {
    let actual = build(
        r#"<div style="width:160px;height:100px;overflow:visible">
             <div style="width:220px;height:140px;background:red"></div>
           </div>"#,
        "",
        800.0,
    );
    // text-overflow ellipsis path also emits PushClipRect, but there's no text here.
    assert!(!actual.contains("PushClipRect"), "overflow:visible must not clip");
}

/// overflow-x: hidden, overflow-y: visible → clip rect width = container width,
/// height sentinel = 2 000 000 (very large, unconstrained axis).
#[test]
fn overflow_x_hidden_y_visible_x_only_clip() {
    let actual = build(
        r#"<div style="width:160px;height:100px;overflow-x:hidden;overflow-y:visible">
             <div style="width:220px;height:140px;background:blue"></div>
           </div>"#,
        "",
        800.0,
    );
    assert!(actual.contains("PushClipRect"), "overflow-x:hidden → PushClipRect");
    // The sentinel for the unconstrained axis is 2 000 000.
    assert!(actual.contains("2000000.00"), "unconstrained y uses large sentinel");
}

/// overflow-x: visible, overflow-y: hidden → clip rect height = container height,
/// width sentinel = 2 000 000.
#[test]
fn overflow_y_hidden_x_visible_y_only_clip() {
    let actual = build(
        r#"<div style="width:160px;height:100px;overflow-x:visible;overflow-y:hidden">
             <div style="width:220px;height:140px;background:green"></div>
           </div>"#,
        "",
        800.0,
    );
    assert!(actual.contains("PushClipRect"), "overflow-y:hidden → PushClipRect");
    assert!(actual.contains("2000000.00"), "unconstrained x uses large sentinel");
}
