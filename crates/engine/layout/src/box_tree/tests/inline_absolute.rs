use lumen_core::geom::Size;

/// BUG-928 — an inline-level box (`display: inline-block`) whose element is
/// `position: absolute` must be taken out of the inline flow and placed at
/// its declared `left`/`top`, not left sitting at its static in-line
/// position. Regression for the first of the bug's three forms.
#[test]
fn absolute_inline_block_escapes_the_inline_row() {
    let html = r#"<body>A<span id="s">S</span></body>"#;
    let css = "body { margin: 0; } \
               #s { position: absolute; left: 500px; top: 400px; \
                    display: inline-block; width: 30px; height: 30px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let s = super::find_by_id_all(&root, &doc, "s").expect("#s not found");
    assert_eq!(s.rect.x, 500.0, "absolute inline-block must land at left:500px, got {}", s.rect.x);
    assert_eq!(s.rect.y, 400.0, "absolute inline-block must land at top:400px, got {}", s.rect.y);
    assert_eq!(s.rect.width, 30.0);
    assert_eq!(s.rect.height, 30.0);
}

/// Second form of BUG-928: a form control (implicit `display: inline-block`
/// UA default) with `position: absolute` must escape the inline row the same
/// way an authored `inline-block` does.
#[test]
fn absolute_form_control_escapes_the_inline_row() {
    let html = r#"<body>A<input id="cb" type="checkbox"></body>"#;
    let css = "body { margin: 0; } \
               #cb { position: absolute; left: 200px; top: 300px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let cb = super::find_by_id_all(&root, &doc, "cb").expect("#cb not found");
    assert!(
        matches!(cb.kind, super::super::BoxKind::FormControl { .. }),
        "absolute form control must keep its FormControl box kind, got {:?}", cb.kind
    );
    assert_eq!(cb.rect.x, 200.0, "absolute form control must land at left:200px, got {}", cb.rect.x);
    assert_eq!(cb.rect.y, 300.0, "absolute form control must land at top:300px, got {}", cb.rect.y);
}

/// Third, smaller-related form of BUG-928: CSS 2.1 §9.7 blockifies a
/// `display: inline` element with `position: absolute` — it must get its own
/// box (placed at left/top) instead of flattening into the surrounding
/// `InlineRun` text fragment.
#[test]
fn absolute_bare_inline_gets_its_own_box() {
    let html = r#"<body>A<span id="t">bare-inline</span></body>"#;
    let css = "body { margin: 0; } \
               #t { position: absolute; left: 100px; top: 50px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t not found");
    assert_eq!(t.rect.x, 100.0, "absolute bare-inline must land at left:100px, got {}", t.rect.x);
    assert_eq!(t.rect.y, 50.0, "absolute bare-inline must land at top:50px, got {}", t.rect.y);
}
