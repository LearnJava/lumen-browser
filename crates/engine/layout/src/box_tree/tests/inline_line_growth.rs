//! BUG-1281 — CSS 2.1 §10.8.1: строка текстового рана доходит до верха и низа самого
//! крупного inline-бокса, а не берёт `line-height` блока.

use lumen_core::geom::Size;

/// Ширина 8 px на символ, ascent 0.8 / descent 0.2 / line-gap 0.2 → `normal` = 1.2 × кегль.
struct Fixed8;

impl super::super::super::TextMeasurer for Fixed8 {
    fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    fn line_gap_px(&self, font_size_px: f32) -> f32 { font_size_px * 0.2 }
}

fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) {
        return Some(b);
    }
    b.children.iter().find_map(find_run)
}

fn run_of(html: &str, css: &str) -> (super::super::LayoutBox, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    (root, doc)
}

#[test]
fn larger_inline_font_stretches_the_line() {
    let (root, doc) = run_of(
        r#"<body><div>a <span style="font-size:40px">T</span> b</div><div id="n">next</div></body>"#,
        "body { margin: 0; } div { margin: 0; }",
    );
    let run = find_run(&root).expect("InlineRun");
    // strut 16px: 19.2; span 40px: ascent 32 + 4 над baseline, 8 + 4 под ней → 48.
    assert!((run.rect.height - 48.0).abs() < 0.01, "height = {}", run.rect.height);
    let next = super::find_by_id_all(&root, &doc, "n").expect("#n");
    assert!((next.rect.y - 48.0).abs() < 0.01, "следующий блок на y = {}", next.rect.y);
}

#[test]
fn mixed_font_sizes_share_one_baseline() {
    let (root, _) = run_of(
        r#"<body><div>a <span style="font-size:40px">T</span> b</div></body>"#,
        "body { margin: 0; } div { margin: 0; }",
    );
    let run = find_run(&root).expect("InlineRun");
    let super::super::BoxKind::InlineRun { lines, .. } = &run.kind else { unreachable!() };
    let baseline = |f: &crate::InlineFrag| f.y_offset + f.style.font_size * 0.8;
    let frags = &lines[0];
    let big = frags.iter().find(|f| f.style.font_size == 40.0).expect("40px frag");
    let small = frags.iter().find(|f| f.style.font_size == 16.0).expect("16px frag");
    assert!((baseline(big) - baseline(small)).abs() < 0.01, "{} vs {}", baseline(big), baseline(small));
    assert!(big.y_offset >= 0.0, "крупный текст не выступает над строкой: {}", big.y_offset);
}

#[test]
fn same_font_inline_does_not_change_line_height() {
    let (root, _) = run_of(
        r#"<body><div>a <b>bold</b> b</div></body>"#,
        "body { margin: 0; } div { margin: 0; }",
    );
    let run = find_run(&root).expect("InlineRun");
    assert!((run.rect.height - 19.2).abs() < 0.01, "height = {}", run.rect.height);
}

#[test]
fn smaller_inline_font_does_not_shrink_the_line() {
    let (root, _) = run_of(
        r#"<body><div>a <small style="font-size:8px">s</small> b</div></body>"#,
        "body { margin: 0; } div { margin: 0; }",
    );
    let run = find_run(&root).expect("InlineRun");
    assert!((run.rect.height - 19.2).abs() < 0.01, "height = {}", run.rect.height);
}
