// ── BUG-196: ::before / ::after on a flex container generate flex items ──

/// Recursively counts boxes whose background equals `rgba`.
fn count_bg(b: &super::super::LayoutBox, rgba: [u8; 4], out: &mut usize) {
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && [bg.r, bg.g, bg.b, bg.a] == rgba
    {
        *out += 1;
    }
    for c in &b.children {
        count_bg(c, rgba, out);
    }
}

#[test]
fn flex_container_before_pseudo_generates_item() {
    // CSS Flexbox §4 — `content: attr()` on `.swatch::before` (display:flex)
    // must generate a blockified flex item carrying the attr text and its own
    // background, placed before the in-flow child. Regression for BUG-196:
    // pseudo-elements were dropped entirely for flex/grid containers.
    let root = super::super::layout(
        &lumen_html_parser::parse(
            "<div class=swatch data-label=Hello><div class=bar></div></div>",
        ),
        &lumen_css_parser::parse(
            ".swatch{display:flex} \
                 .swatch::before{content:attr(data-label);display:flex;background:#2c3e50} \
                 .bar{background:#3498db}",
        ),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    // The generated ::before box must exist with its dark background.
    let mut dark = 0;
    count_bg(&root, [0x2c, 0x3e, 0x50, 0xff], &mut dark);
    assert_eq!(dark, 1, "::before flex item with background must be generated");
    // Its attr() content must be resolved into a text segment.
    let mut text = String::new();
    super::collect_seg_text(&root, &mut text);
    assert!(text.contains("Hello"), "attr() content missing: {text:?}");
}

#[test]
fn flex_container_without_before_has_no_extra_item() {
    // No ::before rule → no phantom flex item injected.
    let root = super::super::layout(
        &lumen_html_parser::parse("<div class=row><div class=a></div></div>"),
        &lumen_css_parser::parse(".row{display:flex} .a{background:#3498db}"),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut blue = 0;
    count_bg(&root, [0x34, 0x98, 0xdb, 0xff], &mut blue);
    assert_eq!(blue, 1, "only the single in-flow child must exist");
}

#[test]
fn marker_default_inherits_parent_color() {
    // No ::marker rule → marker inherits color from li parent.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse("ul { color: #ff0000; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "expected at least one marker");
    // Marker should have inherited red color from parent ul.
    assert!(
        markers[0].style.color.r > 200,
        "marker should inherit red color from ul, got r={}", markers[0].style.color.r,
    );
}

#[test]
fn wide_marker_box_grows_and_right_aligns_at_content_edge() {
    // BUG-185: a marker string wider than the default `em*1.5` box (e.g. a long
    // `@counter-style` prefix/suffix like "#1: ") must grow the marker box leftward
    // so its right edge meets the content edge — otherwise the string overflows
    // into the first content word ("#1:Item" instead of "#1: Item").
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    fn find_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }
    let root = super::super::layout_measured(
        &lumen_html_parser::parse("<ul><li>Item</li></ul>"),
        &lumen_css_parser::parse("li::marker { content: \"#1: \"; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
        &Fixed8,
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    let m = markers.first().expect("expected a marker");
    // "#1: " = 4 chars × 8px (Fixed8) = 32px > default 24px → box widened.
    assert!(m.rect.width >= 31.0,
        "wide marker box must grow to fit the text, got width={}", m.rect.width);
    // Right edge of the marker aligns with the content (InlineRun) left edge.
    let run = find_run(&root).expect("content InlineRun");
    let marker_right = m.rect.x + m.rect.width;
    assert!((marker_right - run.rect.x).abs() <= 1.0,
        "marker right edge {marker_right} must meet content edge {}", run.rect.x);
}

#[test]
fn marker_css_rule_overrides_color() {
    // ::marker { color: #0000ff } → marker gets blue, not parent color.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse("ul { color: #ff0000; } li::marker { color: #0000ff; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "expected at least one marker");
    // Marker must use blue (::marker rule) not parent red.
    assert!(
        markers[0].style.color.b > 200,
        "marker should be blue from ::marker rule, got b={}", markers[0].style.color.b,
    );
    assert!(
        markers[0].style.color.r < 50,
        "marker should NOT be red (parent color), got r={}", markers[0].style.color.r,
    );
}

#[test]
fn marker_content_none_suppresses_marker() {
    // li::marker { content: none } → no BoxKind::Marker in tree.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse("li::marker { content: none; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(markers.is_empty(), "content:none should suppress marker box, found {} markers", markers.len());
}

#[test]
fn marker_content_string_overrides_text() {
    // li::marker { content: "★ " } → marker text becomes "★ " not "• ".
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse(r#"li::marker { content: "★ "; }"#),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "expected marker with custom content");
    if let super::super::BoxKind::Marker { ref text, .. } = markers[0].kind {
        assert_eq!(text, "★ ", "custom content string should override default marker text");
    } else {
        panic!("expected BoxKind::Marker");
    }
}

#[test]
fn marker_default_without_css_rule_still_renders() {
    // No ::marker CSS rule at all → marker renders with default disc bullet.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse(""),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "default disc list item must produce a marker box");
}

#[test]
fn marker_font_size_css_rule_applied() {
    // li::marker { font-size: 24px } → marker uses 24px, not the inherited 16px.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse("li { font-size: 16px; } li::marker { font-size: 24px; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "expected marker");
    assert!(
        (markers[0].style.font_size - 24.0).abs() < 0.5,
        "marker should have font-size 24px from CSS rule, got {}", markers[0].style.font_size,
    );
}

#[test]
fn marker_inherits_font_size_from_parent_without_rule() {
    // No ::marker rule → marker inherits font-size from li parent.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse("li { font-size: 20px; }"),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "expected marker");
    assert!(
        (markers[0].style.font_size - 20.0).abs() < 0.5,
        "marker should inherit 20px font-size from li, got {}", markers[0].style.font_size,
    );
}

#[test]
fn marker_ignores_non_applicable_properties() {
    // CSS Pseudo-Elements L4 §5.5 — only font/color/text-flow/content/animation
    // properties apply to ::marker. A `letter-spacing` declaration must be
    // dropped (marker keeps the inherited default of 0), while `color` — which
    // is in the allowed set — must still take effect.
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>item</li></ul>"),
        &lumen_css_parser::parse(
            "li::marker { letter-spacing: 15px; color: #00ff00; }",
        ),
        lumen_core::geom::Size::new(800.0, 600.0),
    );
    let mut markers = Vec::new();
    super::find_markers(&root, &mut markers);
    assert!(!markers.is_empty(), "expected marker");
    // color (allowed) applied → green.
    assert!(
        markers[0].style.color.g > 200 && markers[0].style.color.r < 50,
        "::marker color should apply, got {:?}", markers[0].style.color,
    );
    // letter-spacing (not allowed) ignored → still the inherited default 0,
    // not the 15px the ::marker rule tried to set.
    assert!(
        markers[0].style.letter_spacing.abs() < 0.5,
        "::marker letter-spacing must be ignored, got {}",
        markers[0].style.letter_spacing,
    );
}

// ── BUG-136 — float / clear / margin interaction (TEST-105) ───────────────

/// Collect every box whose background color matches `hex` (0xRRGGBB).
fn boxes_with_bg(b: &super::super::LayoutBox, hex: u32, out: &mut Vec<super::super::Rect>) {
    let (r, g, bl) = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    if let Some(col) = b.style.background_color.and_then(|c| c.to_color_opt())
        && col.r == r && col.g == g && col.b == bl
    {
        out.push(b.rect);
    }
    for ch in &b.children {
        boxes_with_bg(ch, hex, out);
    }
}

fn find_one_bg(root: &super::super::LayoutBox, hex: u32) -> super::super::Rect {
    let mut v = Vec::new();
    boxes_with_bg(root, hex, &mut v);
    assert_eq!(v.len(), 1, "expected exactly one box with bg #{hex:06x}, found {}", v.len());
    v[0]
}

#[test]
fn float_inflow_block_keeps_full_width_between_floats() {
    // c1: left float + right float + an empty in-flow block with margin:0 100px.
    // CSS 2.1 §9.5 — the block keeps the full containing-block width (300) minus
    // its margins (200) → width 100, positioned at content_left + margin_left.
    // The naive float-narrowing collapsed it to width 0 (squeezed in the gap).
    let root = super::super::layout(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=fl></div>\
                   <div class=fr></div>\
                   <div class=mid></div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            ".cell{position:relative;width:300px;height:300px;overflow:hidden}\
                 .fl{float:left;width:90px;height:120px;background:#e53e3e}\
                 .fr{float:right;width:90px;height:120px;background:#9f7aea}\
                 .mid{height:60px;margin:0 100px;background:#f6e05e}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
    );
    let mid = find_one_bg(&root, 0xf6e05e);
    assert!((mid.width - 100.0).abs() < 1.0, "mid width should be 100, got {}", mid.width);
}

#[test]
fn float_clear_absorbs_margin_top() {
    // c2: float left, then a `clear:both; margin-top:30px` block. CSS 2.1 §9.5.2 —
    // clearance places the border edge at max(natural, float bottom); the margin is
    // absorbed by clearance, NOT stacked on top (float_bottom 120 + margin 30 = 150
    // was the bug; correct is 120).
    let root = super::super::layout(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=fl></div>\
                   <div class=cl></div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            "body{margin:0}\
                 .cell{position:relative;width:300px;height:300px;overflow:hidden}\
                 .fl{float:left;width:120px;height:120px;background:#ed8936}\
                 .cl{clear:both;margin-top:30px;height:80px;background:#4fd1c5}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
    );
    let cl = find_one_bg(&root, 0x4fd1c5);
    // Cell content top is 0 (no padding); float is 120 tall → cleared block at y=120.
    assert!((cl.y - 120.0).abs() < 1.0, "cleared block should sit at float bottom y=120, got {}", cl.y);
}

#[test]
fn floats_wrap_to_next_line_when_they_overflow() {
    // c3: three 130px floats with 8px margins in a 300px container — the third
    // does not fit (146*2 = 292 ≤ 300 < 146*3) and must drop to a new line
    // (CSS 2.1 §9.5.1 rule 8) instead of overflowing past the right edge.
    let root = super::super::layout(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=f></div>\
                   <div class=f></div>\
                   <div class=g></div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            "body{margin:0}\
                 .cell{position:relative;width:300px;height:300px;overflow:hidden}\
                 .f{float:left;width:130px;height:90px;margin:8px;background:#4299e1}\
                 .g{float:left;width:130px;height:90px;margin:8px;background:#fc8181}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
    );
    let third = find_one_bg(&root, 0xfc8181);
    // First two floats occupy line 1 (top ~8); the third wraps below them.
    assert!(third.y > 100.0, "third float should wrap to a new line (y>100), got {}", third.y);
    assert!((third.x - 8.0).abs() < 1.0, "wrapped float should reset to left (x=8), got {}", third.x);
}

#[test]
fn shrink_to_fit_float_wrapper_sums_inner_floats_side_by_side() {
    // BUG-178: an auto-width float wrapper containing two float:left children
    // must shrink-to-fit to the SUM of their margin-box widths (they sit side
    // by side, CSS 2.1 §9.5.1), not the max. The old code took the max → the
    // wrapper was only as wide as one child, so the second float wrapped to a
    // new line. Container is wide enough that both floats fit on one line.
    let root = super::super::layout(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=wrap>\
                     <div class=a></div>\
                     <div class=b></div>\
                   </div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            ".cell{width:600px;height:300px;overflow:hidden}\
                 .wrap{float:left}\
                 .a{float:left;width:120px;height:80px;margin-right:20px;background:#1133cc}\
                 .b{float:left;width:120px;height:80px;background:#22bb44}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
    );
    let a = find_one_bg(&root, 0x1133cc);
    let b = find_one_bg(&root, 0x22bb44);
    // Both floats share the same line: b starts after a (x = a.x + 120 + 20),
    // and does NOT drop below it.
    assert!(
        (b.y - a.y).abs() < 1.0,
        "second inner float must stay on the same line (b.y {} vs a.y {})",
        b.y, a.y
    );
    assert!(
        (b.x - (a.x + 140.0)).abs() < 1.0,
        "second inner float must sit to the right of the first (b.x {}, expected {})",
        b.x, a.x + 140.0
    );
}

/// Every glyph is 10px wide — gives line wrapping deterministic widths.
struct Fixed10Float;
impl super::super::TextMeasurer for Fixed10Float {
    fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
}

/// Absolute `rect.x` of the first `InlineRun` box found (depth-first). Line
/// fragment positions are run-relative, so the run's own origin is what moves
/// when its line boxes are narrowed by a float.
fn first_run_x(b: &super::super::LayoutBox) -> Option<f32> {
    if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) {
        return Some(b.rect.x);
    }
    b.children.iter().find_map(first_run_x)
}

#[test]
fn float_narrows_line_boxes_in_nested_block() {
    // RP-4 (4a): a float in the cell + a nested non-BFC <p> with text. CSS 2.1
    // §9.5 — the <p> keeps the full containing-block width (300); only its line
    // boxes recede past the float. The legacy approximation narrowed the <p>
    // box itself (width 200, x=100); the fix keeps it full-width and shortens
    // its line boxes via the inherited float context.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=fl></div>\
                   <p class=par>aaa aaa aaa</p>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            "body{margin:0}\
                 .cell{width:300px;height:300px}\
                 .fl{float:left;width:100px;height:80px;background:#e53e3e}\
                 .par{height:60px;background:#f6e05e}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
        &Fixed10Float,
    );
    let par = find_one_bg(&root, 0xf6e05e);
    assert!(
        (par.width - 300.0).abs() < 1.0,
        "nested block must keep full width 300 (not narrowed to the float band), got {}",
        par.width,
    );
    let run_x = first_run_x(&root).expect("paragraph inline run missing");
    assert!(
        run_x >= 100.0 - 0.01,
        "first line box must start after the float right edge (100), got x={run_x}",
    );
}

#[test]
fn bfc_block_does_not_overlap_float() {
    // RP-4 (4b): a block that establishes its own BFC (overflow:hidden) beside a
    // float must NOT overlap the float — its border box shifts past the float's
    // right edge (CSS 2.1 §9.5) instead of sliding under it.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=fl></div>\
                   <div class=bfc></div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            "body{margin:0}\
                 .cell{width:300px;height:300px}\
                 .fl{float:left;width:100px;height:80px;background:#e53e3e}\
                 .bfc{overflow:hidden;height:40px;background:#4299e1}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
        &Fixed10Float,
    );
    let bfc = find_one_bg(&root, 0x4299e1);
    assert!(
        bfc.x >= 100.0 - 0.01,
        "BFC block must shift past the float right edge (100), got x={}",
        bfc.x,
    );
}

#[test]
fn clear_in_nested_block_clears_parent_floats() {
    // RP-4 (4c): `clear:left` on a block nested inside a non-BFC wrapper must
    // clear the *enclosing* context's float (CSS 2.1 §9.5.2) — it drops below
    // the float bottom (80) even though the float is the wrapper's sibling, not
    // the wrapper's own child.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=fl></div>\
                   <div class=outer>\
                     <div class=cl></div>\
                   </div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            "body{margin:0}\
                 .cell{width:300px;height:300px}\
                 .fl{float:left;width:100px;height:80px;background:#e53e3e}\
                 .cl{clear:left;height:30px;background:#9f7aea}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
        &Fixed10Float,
    );
    let cl = find_one_bg(&root, 0x9f7aea);
    assert!(
        cl.y >= 80.0 - 0.01,
        "nested cleared block must drop below the parent float bottom (80), got y={}",
        cl.y,
    );
}

#[test]
fn nested_floats_stack() {
    // RP-4 (4c): a float declared inside a non-BFC block placed beside an outer
    // float must stack to the *right* of the outer float (its inline position is
    // measured against the inherited context), not reset to the wrapper's left
    // content edge. The wrapper has text so it counts as in-flow content and
    // receives the propagated float context.
    let root = super::super::layout_measured(
        &lumen_html_parser::parse(
            "<div class=cell>\
                   <div class=fl></div>\
                   <div class=outer>\
                     <div class=inner></div>\
                     aaa\
                   </div>\
                 </div>",
        ),
        &lumen_css_parser::parse(
            "body{margin:0}\
                 .cell{width:300px;height:300px}\
                 .fl{float:left;width:100px;height:80px;background:#e53e3e}\
                 .inner{float:left;width:50px;height:40px;background:#22bb44}",
        ),
        lumen_core::geom::Size::new(1024.0, 720.0),
        &Fixed10Float,
    );
    let inner = find_one_bg(&root, 0x22bb44);
    assert!(
        (inner.x - 100.0).abs() < 1.0,
        "nested float must stack beside the outer float (x≈100), got x={}",
        inner.x,
    );
}

