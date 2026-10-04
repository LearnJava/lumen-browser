use super::*;
use super::layout_generation_misc::first_inline_run_frag;

// ── Half-leading (CSS 2.1 §10.8.1) ──────────────────────────────────────

#[test]
fn half_leading_baseline_centred_in_line_box() {
    // line-height: 2.0 → half_leading = (32 - 16) / 2 = 8px for 16px font.
    // Baseline фрагмента должен быть смещён на 8px вниз от верха строки.
    let root = lay_measured(
        "<p>Hello</p>",
        "p { line-height: 2.0; font-size: 16px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    let expected_half_leading = 8.0_f32; // (32 - 16) / 2
    assert!(
        (frag.y_offset - expected_half_leading).abs() < 0.1,
        "half_leading with line-height:2: expected y_offset={}, got {}",
        expected_half_leading,
        frag.y_offset
    );
}

#[test]
fn half_leading_zero_when_line_height_equals_font_size() {
    // line-height: 1.0 → нет leading, y_offset = 0.
    let root = lay_measured(
        "<p>Hello</p>",
        "p { line-height: 1.0; font-size: 16px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    assert!(
        frag.y_offset.abs() < 0.001,
        "line-height:1.0 → no half-leading, expected y_offset=0, got {}",
        frag.y_offset
    );
}

#[test]
fn half_leading_line_box_height_correct() {
    // line-height: 1.5, font-size: 20px → line_h = 30px.
    // Высота InlineRun должна быть 30px.
    let root = lay_measured(
        "<p>Hello</p>",
        "p { line-height: 1.5; font-size: 20px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let run = p.children.iter().find(|c| matches!(c.kind, crate::box_tree::BoxKind::InlineRun { .. })).expect("InlineRun not found");
    assert!(
        (run.rect.height - 30.0).abs() < 0.5,
        "line-height:1.5 font-size:20px → height=30px, got {}",
        run.rect.height
    );
}

// ── Multi-column layout ──────────────────────────────────────────────────

#[test]
fn multicol_column_count_divides_width() {
    // column-count: 3 + column-gap: 10px → each column = (300 - 20) / 3 = 93.33px.
    // Three equal 30px boxes (total 90px) balance into 3 columns of 30px each,
    // so each box maps cleanly to one column fragment.
    let root = lay_measured(
        "<div id='c'><div></div><div></div><div></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 10px; } #c div { height: 30px; }",
        800.0,
    );
    let container = first_element_child(&root);
    assert_eq!(container.children.len(), 3);
    let col_w = container.children[0].rect.width;
    assert!((col_w - 93.33).abs() < 0.1, "col_w={col_w}");
    // All three children should be in different columns (x differs).
    let x0 = container.children[0].rect.x;
    let x1 = container.children[1].rect.x;
    let x2 = container.children[2].rect.x;
    assert!(x1 > x0, "child1.x={x1} should be right of child0.x={x0}");
    assert!(x2 > x1, "child2.x={x2} should be right of child1.x={x1}");
}

#[test]
fn multicol_no_repeat_width_when_no_column_props() {
    // Without column-count / column-width, block flow is unchanged.
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div></div>",
        "#c { width: 300px; } #a { height: 20px; } #b { height: 20px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let ch0 = &container.children[0];
    let ch1 = &container.children[1];
    assert_eq!(ch0.rect.x, ch1.rect.x, "children should share same x in normal flow");
    assert!(ch1.rect.y > ch0.rect.y, "b should be below a");
}

#[test]
fn multicol_column_span_all_spans_full_width() {
    // A child with column-span:all should be laid out at the full container width,
    // not squeezed into a single column.
    // Layout: 2 column children → span-all → 2 more column children.
    let root = lay_measured(
        r#"<div id='c'>
          <div id='a'></div>
          <div id='s'></div>
          <div id='b'></div>
        </div>"#,
        r#"#c { width: 300px; column-count: 2; column-gap: 10px; }
           #a { height: 20px; }
           #b { height: 20px; }
           #s { column-span: all; height: 10px; }"#,
        800.0,
    );
    let container = first_element_child(&root);
    // Find the span-all child by its full container width (300px) — column
    // fragments of #a/#b are col_w wide, only the spanner spans the full width.
    let span_child = container.children.iter()
        .find(|c| (c.rect.width - 300.0).abs() < 1.0)
        .expect("span-all child not found");
    // Span-all element must cover the full container width (300px).
    assert!(
        (span_child.rect.width - 300.0).abs() < 1.0,
        "span-all child width={} should be 300px",
        span_child.rect.width
    );
    // Span-all element must start at container's content_x.
    assert!(
        span_child.rect.x < 10.0,
        "span-all child x={} should be near container left edge",
        span_child.rect.x
    );
}

#[test]
fn multicol_column_span_all_children_below_span() {
    // Children after a column-span:all element must be positioned below it.
    let root = lay_measured(
        r#"<div id='c'>
          <div id='s'></div>
          <div id='b'></div>
        </div>"#,
        r#"#c { width: 300px; column-count: 2; column-gap: 10px; }
           #s { column-span: all; height: 15px; }
           #b { height: 20px; }"#,
        800.0,
    );
    let container = first_element_child(&root);
    // Spanner is the only full-width (300px) child; #b becomes column fragments.
    let span_child = container.children.iter()
        .find(|c| (c.rect.width - 300.0).abs() < 1.0)
        .expect("span-all child not found");
    let span_bottom = span_child.rect.y + span_child.rect.height;
    // Every column fragment of #b (the non-span children) must be below the spanner.
    let after_children: Vec<&LayoutBox> = container.children.iter()
        .filter(|c| (c.rect.width - 300.0).abs() >= 1.0 && c.rect.height > 0.0)
        .collect();
    assert!(!after_children.is_empty(), "expected #b column fragments below span");
    for after_child in after_children {
        assert!(
            after_child.rect.y >= span_bottom,
            "after_child.y={} must be >= span bottom={}",
            after_child.rect.y,
            span_bottom
        );
    }
}

#[test]
fn multicol_column_fill_auto_sequential() {
    // column-fill: auto — each column is filled up to the container height before
    // spilling to the next column, rather than distributing content evenly.
    // 3 children of 15px each (total 45px) in a 40px-tall container: col0 fills to
    // 40px (the first two boxes + the top 10px of the third), and the third box's
    // remaining 5px spills into col1 (CSS Multicol §3.4 fragmentation).
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div><div id='d'></div></div>",
        "#c { width: 300px; column-count: 2; column-gap: 0px; height: 40px; column-fill: auto; } \
         #a { height: 15px; } #b { height: 15px; } #d { height: 15px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let frags: Vec<&LayoutBox> = container.children.iter()
        .filter(|c| c.rect.height > 0.0)
        .collect();
    // col_w = 300 / 2 = 150. col0 at content_x, col1 at content_x + 150.
    let col0_x = frags.iter().map(|c| c.rect.x).fold(f32::INFINITY, f32::min);
    // col0 must be filled all the way to the container height before col1 is used.
    let col0_bottom = frags.iter()
        .filter(|c| (c.rect.x - col0_x).abs() < 1.0)
        .map(|c| c.rect.y + c.rect.height)
        .fold(0.0f32, f32::max);
    assert!(
        (col0_bottom - 40.0).abs() < 1.0,
        "col0 must fill to container height 40 before spilling (col0_bottom={col0_bottom})"
    );
    // The spillover fragment must exist in col1 (x = content_x + 150).
    assert!(
        frags.iter().any(|c| c.rect.x > col0_x + 100.0),
        "expected a spillover fragment in col1 (col0_x={col0_x})"
    );
}

#[test]
fn multicol_balance_fragments_boxes_across_columns() {
    // Regression (BUG-186, TEST-33 case 5): two 36px background boxes in a
    // 3-column balance container fragment into three 24px column slices
    // (total 72 / 3 = 24), matching Edge — not one atomic box per column with
    // an empty third column. The container height collapses to 24px.
    let root = lay_measured(
        "<div id='c'><div></div><div></div></div>",
        "#c { width: 660px; column-count: 3; column-gap: 12px; } #c div { height: 36px; }",
        800.0,
    );
    let container = first_element_child(&root);
    // col_w = (660 - 24) / 3 = 212.
    let frags: Vec<&LayoutBox> = container.children.iter()
        .filter(|c| c.rect.height > 0.0)
        .collect();
    // Every fragment is at most one column tall (24px), never a whole 36px box.
    for f in &frags {
        assert!(f.rect.height <= 24.0 + 0.5, "fragment too tall: {}", f.rect.height);
        assert!((f.rect.width - 212.0).abs() < 0.5, "fragment width={}", f.rect.width);
    }
    // All three columns receive content (distinct x positions).
    let mut xs: Vec<f32> = frags.iter().map(|f| f.rect.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    assert_eq!(xs.len(), 3, "all 3 columns should hold a fragment, got xs={xs:?}");
    // Container content height = balanced column height = 24px.
    assert!(
        (container.rect.height - 24.0).abs() < 1.0,
        "container height={} should be 24px",
        container.rect.height
    );
}

#[test]
fn multicol_column_fill_balance_vs_auto_target() {
    // Verify that column-fill:balance uses total/n_cols as target, not container height.
    // With height:20px and 2 children of 15px each and 2 columns:
    //   balance: target = ceil(30/2) = 15 → ch0 fills col0 (15px), ch1 overflows to col1
    //   auto:    target = 20 → ch0(15)+ch1(15)=30>20 with count_cap=1, so still col0+col1
    // Both end up with same layout here; test that column_fill_balance is parsed.
    let root = lay("<p>x</p>", "p { column-fill: balance; }");
    assert!(first_p_style(&root).column_fill_balance, "balance should set column_fill_balance=true");
    let root2 = lay("<p>x</p>", "p { column-fill: auto; }");
    assert!(!first_p_style(&root2).column_fill_balance, "auto should set column_fill_balance=false");
}

#[test]
fn multicol_balance_does_not_skip_first_column() {
    // Regression (BUG-117): with column-count:3 and items each taller than the
    // balanced target height, the greedy assigner advanced past the EMPTY first
    // column (height_overflow fires on column 0 because item height > target),
    // placing items in columns 1 and 2 and leaving column 0 blank. Items must
    // fill column 0 first (CSS Multicol §3.4 — columns filled in order).
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; } \
         #a { height: 40px; } #b { height: 40px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let a = &container.children[0];
    let b = &container.children[1];
    // col_w = 300/3 = 100. col0 at content_x, col1 at content_x + 100.
    assert!(
        (a.rect.x - container.rect.x).abs() < 1.0,
        "first item must be in column 0 (a.x={}, container.x={})",
        a.rect.x, container.rect.x
    );
    assert!(
        (b.rect.x - a.rect.x - 100.0).abs() < 1.0,
        "second item must be in column 1, not column 2 (b.x={}, a.x={})",
        b.rect.x, a.rect.x
    );
}

#[test]
fn multicol_fill_auto_ignores_count_cap() {
    // Regression (BUG-117): column-fill:auto must fill a column purely by height.
    // The per-column count cap (a balance-mode anti-starvation guard) wrongly forced
    // one item per column even in auto mode. With 3 short items and a tall container,
    // all three must stack in column 0.
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div><div id='d'></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; height: 100px; column-fill: auto; } \
         #a { height: 10px; } #b { height: 10px; } #d { height: 10px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let a = &container.children[0];
    let b = &container.children[1];
    let d = &container.children[2];
    // All three fit in column 0 (30px < 100px) → identical x.
    assert!(
        (a.rect.x - b.rect.x).abs() < 1.0 && (a.rect.x - d.rect.x).abs() < 1.0,
        "auto must stack all items in col0 (xs: {} {} {})",
        a.rect.x, b.rect.x, d.rect.x
    );
    // And they stack vertically within the column.
    assert!(
        b.rect.y > a.rect.y && d.rect.y > b.rect.y,
        "items must stack vertically in col0 (ys: {} {} {})",
        a.rect.y, b.rect.y, d.rect.y
    );
}

// ── ::marker box (BUG-011) ───────────────────────────────────────────

#[test]
fn list_item_generates_marker_box() {
    let root = lay("<ul><li>item</li></ul>", "");
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
    assert!(marker.is_some(), "list-item must have a ::marker child");
    if let BoxKind::Marker { text, position, list_style_type, .. } = &marker.unwrap().kind {
        // Disc renders geometrically — marker_text returns "" for bullet types.
        assert!(text.is_empty(), "disc marker text must be empty (geometric rendering)");
        assert_eq!(*list_style_type, ListStyleType::Disc, "default list-style-type is disc");
        assert_eq!(*position, ListStylePosition::Outside);
    }
}

// ── list-style-position: inside (CSS Lists L3 §2.4) ─────────────────

fn lsp_li(html: &str, css: &str) -> LayoutBox {
    let root = lay_measured(html, css, 400.0);
    let ul = first_element_child(&root);
    ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap().clone()
}

fn lsp_run(li: &LayoutBox) -> &LayoutBox {
    li.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap()
}

#[test]
fn list_style_position_inside_indents_only_first_line() {
    // 8px glyphs, 120px content: the inside marker (24px) shares line 0, so
    // line 0 starts 24px in while wrapped lines return to the content edge.
    let li = lsp_li(
        "<ul><li>aaaa bbbb cccc dddd eeee ffff</li></ul>",
        "ul { padding: 0; width: 120px; list-style-position: inside; }",
    );
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let run = lsp_run(&li);
    let BoxKind::InlineRun { lines, .. } = &run.kind else { unreachable!() };
    assert!(lines.len() >= 2, "text must wrap, got {} line(s)", lines.len());
    assert_eq!(marker.rect.x, li.rect.x, "inside marker sits at the content edge");
    assert_eq!(run.rect.x, li.rect.x, "run keeps the full content box");
    assert_eq!(run.rect.width, li.rect.width, "run keeps the full content width");
    assert_eq!(lines[0][0].x, marker.rect.width, "line 0 inset by the marker width");
    assert_eq!(lines[1][0].x, 0.0, "wrapped line returns to the content edge");
}

#[test]
fn list_style_position_outside_has_no_first_line_inset() {
    let li = lsp_li(
        "<ul><li>aaaa bbbb cccc dddd eeee ffff</li></ul>",
        "ul { padding: 0 0 0 40px; width: 160px; }",
    );
    let run = lsp_run(&li);
    let BoxKind::InlineRun { lines, first_line_inset, .. } = &run.kind else { unreachable!() };
    assert_eq!(*first_line_inset, 0.0);
    assert_eq!(lines[0][0].x, 0.0);
}

#[test]
fn list_style_position_inside_block_child_drops_below_marker_line() {
    // A block-level first child cannot share the marker's line: the marker
    // gets its own line box and the block starts one marker line lower, at
    // the full content width.
    let li = lsp_li(
        "<ul><li><p>text</p></li></ul>",
        "ul { padding: 0; width: 120px; list-style-position: inside; } p { margin: 0; }",
    );
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let p = li.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.rect.x, li.rect.x, "block child is not shifted right");
    assert_eq!(p.rect.width, li.rect.width, "block child keeps the full width");
    assert!(
        (p.rect.y - (li.rect.y + marker.rect.height)).abs() < 0.01,
        "block child starts below the marker line: p.y={} li.y={} marker.h={}",
        p.rect.y, li.rect.y, marker.rect.height
    );
}

#[test]
fn list_style_image_marker_carries_url() {
    // CSS Lists L3 §2.3 — `list-style-image` populates the Marker box's
    // `image` field and the URL is collected for fetching.
    let root = lay(
        "<ul><li>item</li></ul>",
        "li { list-style-image: url(\"bullet.png\"); }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    if let BoxKind::Marker { image, .. } = &marker.kind {
        assert_eq!(image.as_deref(), Some("bullet.png"));
    } else {
        panic!("expected Marker box");
    }
    let urls = collect_background_image_requests(&root, 1.0);
    assert!(urls.iter().any(|u| u == "bullet.png"), "marker image must be fetched");
}

#[test]
fn list_style_image_marker_shown_with_type_none() {
    // CSS Lists L3 §2.3 — an explicit image still produces a marker even when
    // `list-style-type: none`.
    let root = lay(
        "<ul><li>item</li></ul>",
        "li { list-style-type: none; list-style-image: url(\"b.png\"); }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
    assert!(marker.is_some(), "list-style-image must generate a marker despite type:none");
}

#[test]
fn list_item_none_no_marker() {
    let root = lay("<ul><li>item</li></ul>", "li { list-style-type: none; }");
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
    assert!(marker.is_none(), "list-style-type:none must not generate marker");
}

#[test]
fn ordered_list_decimal_marker() {
    let root = lay(
        "<ol><li>a</li><li>b</li></ol>",
        "ol { list-style-type: decimal; }",
    );
    let ol = first_element_child(&root);
    let lis: Vec<_> = ol.children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    assert_eq!(lis.len(), 2);
    let m0 = lis[0].children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let m1 = lis[1].children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    if let (BoxKind::Marker { text: t0, .. }, BoxKind::Marker { text: t1, .. }) = (&m0.kind, &m1.kind) {
        assert_eq!(t0, "1. ", "first item");
        assert_eq!(t1, "2. ", "second item");
    }
}

#[test]
fn marker_outside_not_in_flow() {
    // For outside markers: child_y must not advance past the marker.
    let root = lay(
        "<ul><li>item</li></ul>",
        "ul { margin: 0; padding: 0; } li { font-size: 16px; line-height: 1; }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let content = li.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineRun { .. })).unwrap();
    // Marker y should equal content y (both at top of list item).
    assert_eq!(marker.rect.y, content.rect.y, "marker and content must share the same top");
    // Marker x must be to the left of content x.
    assert!(marker.rect.x < content.rect.x, "marker must be left of content");
}

/// BUG-038: list-style-position: inside — marker must share the first line with content,
/// not occupy a separate block line. li height must equal one line-height.
#[test]
fn marker_inside_shares_line_with_content() {
    let root = lay_measured(
        "<ul><li>item</li></ul>",
        "ul { padding-left: 0; } \
         li { list-style-position: inside; font-size: 16px; line-height: 1; }",
        800.0,
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let content = li.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineRun { .. })).unwrap();
    // Marker and content must be on the same line.
    assert_eq!(marker.rect.y, content.rect.y, "inside marker and content must share the same y");
    // CSS Lists L3 §2.4: the run keeps the full content box; only its first
    // line is inset by the marker width, so the text starts right of the marker.
    let BoxKind::InlineRun { lines, .. } = &content.kind else { unreachable!() };
    assert_eq!(content.rect.x, marker.rect.x, "run keeps the content edge");
    assert!(
        content.rect.x + lines[0][0].x >= marker.rect.x + marker.rect.width,
        "first-line text must start right of the inside marker"
    );
    // li height must be one line-height (16 * 1.0 = 16px), not two.
    assert!((li.rect.height - 16.0).abs() < 1.0,
        "li height should be one line (16px), got {}", li.rect.height);
}

// ─── CSS 2.1 §9.5 — float + clear ────────────────────────────────────────

/// `float: left` с явной шириной — элемент помещается у левого края контейнера.
#[test]
fn float_left_positioned_at_left_edge() {
    let root = lay(
        "<div class='c'><div class='f'>x</div></div>",
        ".c { width: 400px; }
         .f { float: left; width: 100px; height: 50px; }",
    );
    let c = first_element_child(&root);
    let f = first_element_child(c);
    assert_eq!(f.rect.x, 0.0, "float left: x at container left");
    assert_eq!(f.rect.y, 0.0, "float left: y at top");
    assert_eq!(f.rect.width,  100.0, "float left: explicit width");
    assert_eq!(f.rect.height,  50.0, "float left: explicit height");
}

/// `float: right` с явной шириной — элемент у правого края контейнера.
#[test]
fn float_right_positioned_at_right_edge() {
    let root = lay(
        "<div class='c'><div class='f'>x</div></div>",
        ".c { width: 400px; }
         .f { float: right; width: 100px; height: 50px; }",
    );
    let c = first_element_child(&root);
    let f = first_element_child(c);
    // right edge of container = 400px; float width = 100px → x = 300
    assert_eq!(f.rect.x, 300.0, "float right: x at container_right - width");
    assert_eq!(f.rect.y,   0.0, "float right: y at top");
}

/// Float left: последующий in-flow block-брат сохраняет полную ширину
/// containing-block, но его line-box сдвигается за float (CSS 2.1 §9.5).
/// RP-4: раньше клипали сам бокс (x=100, width=300) — это была аппроксимация.
#[test]
fn float_left_narrows_sibling_width() {
    let root = lay(
        "<div class='c'><div class='f'>x</div><div class='s'>y</div></div>",
        ".c { width: 400px; }
         .f { float: left; width: 100px; height: 50px; }
         .s { height: 30px; }",
    );
    let c = first_element_child(&root);
    let sibling = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.float_side == FloatSide::None)
        .expect("sibling block");
    // CSS 2.1 §9.5: the block keeps the full containing-block width, not narrowed.
    assert_eq!(sibling.rect.x,     0.0,   "sibling keeps full-width origin");
    assert_eq!(sibling.rect.width, 400.0, "sibling keeps full containing-block width");
    // Only its line box recedes past the float (the inner inline run starts at 100).
    let run = sibling.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::InlineRun { .. }))
        .expect("inline run");
    assert_eq!(run.rect.x, 100.0, "line box starts after the left float");
}

/// Float right: последующий in-flow block-брат сохраняет полную ширину;
/// его line-box укорачивается справа на ширину float (CSS 2.1 §9.5).
#[test]
fn float_right_narrows_sibling_width() {
    let root = lay(
        "<div class='c'><div class='f'>x</div><div class='s'>y</div></div>",
        ".c { width: 400px; }
         .f { float: right; width: 100px; height: 50px; }
         .s { height: 30px; }",
    );
    let c = first_element_child(&root);
    let sibling = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.float_side == FloatSide::None)
        .expect("sibling block");
    // CSS 2.1 §9.5: the block keeps the full containing-block width.
    assert_eq!(sibling.rect.x,     0.0,   "sibling starts at left edge");
    assert_eq!(sibling.rect.width, 400.0, "sibling keeps full containing-block width");
    // Its line box starts at the left edge but is shortened by the right float.
    let run = sibling.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::InlineRun { .. }))
        .expect("inline run");
    assert_eq!(run.rect.x, 0.0, "line box starts at left edge");
    assert!(run.rect.width <= 300.0 + 0.01, "line box shortened by right float, got {}", run.rect.width);
}

/// Два `float: left` выстраиваются горизонтально.
#[test]
fn two_left_floats_stack_horizontally() {
    let root = lay(
        "<div class='c'><div class='f1'>a</div><div class='f2'>b</div></div>",
        ".c  { width: 400px; }
         .f1 { float: left; width: 100px; height: 50px; }
         .f2 { float: left; width: 80px;  height: 40px; }",
    );
    let c = first_element_child(&root);
    let floats: Vec<_> = c.children.iter()
        .filter(|ch| ch.style.float_side == FloatSide::Left)
        .collect();
    assert_eq!(floats.len(), 2, "expected two left floats");
    assert_eq!(floats[0].rect.x, 0.0,   "first float at left edge");
    assert_eq!(floats[1].rect.x, 100.0, "second float after first");
}

/// `clear: both` сдвигает элемент ниже обоих float-ов.
#[test]
fn clear_both_advances_past_floats() {
    let root = lay(
        "<div class='c'><div class='fl'>a</div><div class='fr'>b</div><div class='clr'>c</div></div>",
        ".c   { width: 400px; }
         .fl  { float: left;  width: 80px; height: 60px; }
         .fr  { float: right; width: 80px; height: 40px; }
         .clr { clear: both; height: 20px; }",
    );
    let c = first_element_child(&root);
    let clr = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.clear == ClearSide::Both)
        .expect("clear:both block");
    // clear:both → must start at y >= max(60, 40) = 60
    assert!(clr.rect.y >= 60.0 - 0.01,
        "clear:both block must start below tallest float (got {})", clr.rect.y);
}

/// Контейнер height охватывает float (float clearing родителя).
/// CSS 2.1 §9.5: контейнер должен расти, чтобы содержать свои float-ы.
#[test]
fn container_height_encloses_float() {
    let root = lay(
        "<div class='c'><div class='f'>x</div></div>",
        ".c { width: 400px; }
         .f { float: left; width: 100px; height: 80px; }",
    );
    let c = first_element_child(&root);
    // Container has no non-float children, so height = float height = 80.
    assert!(c.rect.height >= 80.0 - 0.01,
        "container must enclose float (height={}, expected >=80)", c.rect.height);
}

/// `clear: left` сдвигает элемент мимо левого float.
#[test]
fn clear_left_only_clears_left_floats() {
    let root = lay(
        "<div class='c'><div class='fl'>a</div><div class='clr'>c</div></div>",
        ".c   { width: 400px; }
         .fl  { float: left; width: 80px; height: 50px; }
         .clr { clear: left; height: 20px; }",
    );
    let c = first_element_child(&root);
    let clr = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.clear == ClearSide::Left)
        .expect("clear:left block");
    assert!(clr.rect.y >= 50.0 - 0.01,
        "clear:left must start below left float (got {})", clr.rect.y);
}

/// CSS `float` парсится в FloatSide.
#[test]
fn float_side_parsed_correctly() {
    let root = lay("<div class='l'>x</div><div class='r'>x</div><div class='n'>x</div>",
        ".l { float: left } .r { float: right } .n { float: none }");
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let l = iter.next().unwrap();
    let r = iter.next().unwrap();
    let n = iter.next().unwrap();
    assert_eq!(l.style.float_side, FloatSide::Left,  "float: left");
    assert_eq!(r.style.float_side, FloatSide::Right, "float: right");
    assert_eq!(n.style.float_side, FloatSide::None,  "float: none");
}

/// CSS `clear` парсится в ClearSide.
#[test]
fn clear_parsed_correctly() {
    let root = lay("<div class='b'>x</div><div class='l'>x</div><div class='r'>x</div>",
        ".b { clear: both } .l { clear: left } .r { clear: right }");
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let b = iter.next().unwrap();
    let l = iter.next().unwrap();
    let r = iter.next().unwrap();
    assert_eq!(b.style.clear, ClearSide::Both,  "clear: both");
    assert_eq!(l.style.clear, ClearSide::Left,  "clear: left");
    assert_eq!(r.style.clear, ClearSide::Right, "clear: right");
}

// ── Margin collapsing CSS 2.1 §8.3.1 ─────────────────────────────────────

/// Соседние блоки: побеждает бо́льший margin-top (top wins).
#[test]
fn sibling_blocks_margin_collapse_top_wins() {
    // mb=10, mt=30 → gap = max(10,30) = 30, а не 40
    let root = lay(
        "<div class='a'>x</div><div class='b'>y</div>",
        ".a { height: 10px; margin-bottom: 10px; } .b { height: 10px; margin-top: 30px; }",
    );
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    assert!((a.rect.y - 0.0).abs() < 0.1, "a.y={}", a.rect.y);
    // bottom of .a = 10. gap = max(10,30)=30. .b top = 40.
    assert!((b.rect.y - 40.0).abs() < 0.1, "b.y={}", b.rect.y);
}

/// Соседние блоки: побеждает бо́льший margin-bottom (bottom wins).
#[test]
fn sibling_blocks_margin_collapse_bottom_wins() {
    // mb=30, mt=10 → gap = max(30,10) = 30, а не 40
    let root = lay(
        "<div class='a'>x</div><div class='b'>y</div>",
        ".a { height: 10px; margin-bottom: 30px; } .b { height: 10px; margin-top: 10px; }",
    );
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    assert!((a.rect.y - 0.0).abs() < 0.1, "a.y={}", a.rect.y);
    // bottom of .a = 10. gap = max(30,10)=30. .b top = 40.
    assert!((b.rect.y - 40.0).abs() < 0.1, "b.y={}", b.rect.y);
}

/// Цепочка из трёх блоков: два соседних схлопывания независимы.
#[test]
fn three_sibling_blocks_margin_collapse_chain() {
    // .a mb=20, .b mt=15 mb=25, .c mt=10
    // gap(a–b) = max(20,15)=20,  gap(b–c) = max(25,10)=25
    let root = lay(
        "<div class='a'>x</div><div class='b'>y</div><div class='c'>z</div>",
        ".a { height: 5px; margin-bottom: 20px; }
         .b { height: 5px; margin-top: 15px; margin-bottom: 25px; }
         .c { height: 5px; margin-top: 10px; }",
    );
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    let c = iter.next().unwrap();
    assert!((a.rect.y -  0.0).abs() < 0.1, "a.y={}", a.rect.y);
    assert!((b.rect.y - 25.0).abs() < 0.1, "b.y={}", b.rect.y);
    assert!((c.rect.y - 55.0).abs() < 0.1, "c.y={}", c.rect.y);
}

/// BUG-193: a `display: table` wrapper box is block-level, so its margins
/// collapse with adjacent sibling margins (CSS 2.1 §8.3.1) — even though the
/// table establishes a BFC for its own rows/cells. The gap between the table
/// and the following block must be `max(30, 10) = 30`, not the summed `40`.
#[test]
fn table_bottom_margin_collapses_with_next_sibling() {
    let root = lay(
        "<table class='t'><tr><td>x</td></tr></table><div class='b'>y</div>",
        ".t { margin-bottom: 30px; } .b { height: 10px; margin-top: 10px; }",
    );
    let table = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Table))
        .expect("table box");
    let b = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .expect("following block");
    let gap = b.rect.y - (table.rect.y + table.rect.height);
    assert!(
        (gap - 30.0).abs() < 0.1,
        "table↔block gap={gap} (expected collapsed 30, not summed 40)",
    );
}

// ── CSS Intrinsic Sizing L3 — min-content / max-content / fit-content ────

/// `width: fit-content` на block-элементе с явной шириной потомка: бокс
/// сжимается до ширины потомка, не растягиваясь на весь контейнер.
#[test]
fn fit_content_shrinks_to_child_explicit_width() {
    let root = lay(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: fit-content; }
         .inner { width: 120px; height: 10px; }",
    );
    let outer = first_element_child(&root);
    // outer's border-box should equal inner's 120px (no padding/border on outer).
    assert!(
        (outer.rect.width - 120.0).abs() < 1.0,
        "outer.width={} expected≈120",
        outer.rect.width
    );
}

/// `width: fit-content` не выходит за пределы доступного пространства.
#[test]
fn fit_content_capped_at_available_width() {
    // Container 200px wide; inner has explicit width 300px (wider than container).
    let root = lay_viewport(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: fit-content; }
         .inner { width: 300px; height: 10px; }",
        Size { width: 200.0, height: 600.0 },
    );
    let outer = first_element_child(&root);
    // fit-content = min(available=200, max-content=300) → 200.
    assert!(
        outer.rect.width <= 200.0 + 0.5,
        "outer.width={} should be ≤ 200",
        outer.rect.width
    );
}

/// `width: max-content` expands past the container to fit content.
#[test]
fn max_content_expands_to_child_explicit_width() {
    let root = lay_viewport(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: max-content; }
         .inner { width: 500px; height: 10px; }",
        Size { width: 200.0, height: 600.0 },
    );
    let outer = first_element_child(&root);
    // max-content ignores available width — should be 500px.
    assert!(
        (outer.rect.width - 500.0).abs() < 1.0,
        "outer.width={} expected≈500",
        outer.rect.width
    );
}

/// `width: min-content` with single-word text: box shrinks to word width.
#[test]
fn min_content_shrinks_to_word_width() {
    // Fixed8 measurer: each char = 8px. "Hello" = 5 chars = 40px.
    // Container is 800px wide. min-content should give 40px.
    let root = lay_measured(
        "<p class='p'>Hello</p>",
        ".p { width: min-content; }",
        800.0,
    );
    let p = first_element_child(&root);
    // With Fixed8 measurer: "Hello" = 5 × 8 = 40px.
    assert!(
        (p.rect.width - 40.0).abs() < 1.0,
        "p.width={} expected≈40 (5 chars × 8px)",
        p.rect.width
    );
}

/// `width: fit-content` on block with text: shrinks to text width.
#[test]
fn fit_content_text_shrinks_within_container() {
    // "Hi" = 2 chars × 8px = 16px; container = 800px.
    let root = lay(
        "<p class='p'>Hi</p>",
        ".p { width: fit-content; }",
    );
    let p = first_element_child(&root);
    assert!(
        p.rect.width <= 800.0,
        "p.width={} should be ≤ container",
        p.rect.width
    );
    // Text content width = 16px. Box should shrink to ~16px (+ any padding).
    assert!(
        p.rect.width < 100.0,
        "p.width={} should be much less than 800px (container)",
        p.rect.width
    );
}

/// `width: fit-content` with text: element shrinks to text content width.
#[test]
fn fit_content_text_node_shrinks_to_content() {
    // "Hi" = 2 chars × 8px = 16px with Fixed8 measurer.
    let root = lay_measured(
        "<div class='d'>Hi</div>",
        ".d { width: fit-content; }",
        800.0,
    );
    let div = first_element_child(&root);
    // Should shrink to text content width ≈ 16px, not fill the 800px container.
    assert!(
        div.rect.width < 100.0,
        "div.width={} should shrink to ~16px",
        div.rect.width
    );
    assert!(
        div.rect.width >= 16.0,
        "div.width={} should be at least text width 16px",
        div.rect.width
    );
}

/// `width: max-content` parsing: keyword stored correctly.
#[test]
fn max_content_keyword_parsed() {
    let sheet = lumen_css_parser::parse(".x { width: max-content; }");
    let doc = lumen_html_parser::parse("<div class='x'>a</div>");
    let vp = Size { width: 800.0, height: 600.0 };
    use crate::style::Length;
    let children = doc.get(doc.body().unwrap()).children.clone();
    let div_id = children.into_iter().find(|&id| {
        matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
    }).unwrap();
    let div_style = compute_style(&doc, div_id, &sheet, &ComputedStyle::root(), vp, false);
    assert!(
        matches!(div_style.width, Some(Length::MaxContent)),
        "expected MaxContent, got {:?}", div_style.width
    );
}

/// `width: min-content` and `width: fit-content` parsing round-trip.
#[test]
fn min_fit_content_keywords_parsed() {
    let sheet = lumen_css_parser::parse(".a { width: min-content; } .b { width: fit-content; }");
    let doc = lumen_html_parser::parse("<div class='a'></div><div class='b'></div>");
    let root_style = ComputedStyle::root();
    let vp = Size { width: 800.0, height: 600.0 };
    use crate::style::Length;
    let children = doc.get(doc.body().unwrap()).children.clone();
    let mut it = children.into_iter().filter(|&id| matches!(&doc.get(id).data, lumen_dom::NodeData::Element { .. }));
    let a_id = it.next().unwrap();
    let b_id = it.next().unwrap();
    let a_style = compute_style(&doc, a_id, &sheet, &root_style, vp, false);
    let b_style = compute_style(&doc, b_id, &sheet, &root_style, vp, false);
    assert!(matches!(a_style.width, Some(Length::MinContent)), "got {:?}", a_style.width);
    assert!(matches!(b_style.width, Some(Length::FitContent(None))), "got {:?}", b_style.width);
}

/// CSS Sizing L4 §4.1: `stretch` and its legacy aliases parse to the dedicated
/// `Length::Stretch`, not to `fit-content`.
#[test]
fn stretch_keywords_parse_to_stretch() {
    use crate::style::Length;
    let vp = Size { width: 800.0, height: 600.0 };
    for kw in ["stretch", "-webkit-fill-available", "-moz-available"] {
        let sheet = lumen_css_parser::parse(&format!(".x {{ width: {kw}; height: {kw}; }}"));
        let doc = lumen_html_parser::parse("<div class='x'>a</div>");
        let children = doc.get(doc.body().unwrap()).children.clone();
        let id = children.into_iter().find(|&id| {
            matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
        }).unwrap();
        let st = compute_style(&doc, id, &sheet, &ComputedStyle::root(), vp, false);
        assert!(matches!(st.width, Some(Length::Stretch)), "{kw}: width={:?}", st.width);
        assert!(matches!(st.height, Some(Length::Stretch)), "{kw}: height={:?}", st.height);
    }
}

/// `width: stretch` fills the containing block minus margins, also for an
/// inline-block that `auto` would shrink-wrap; `fit-content` still shrinks.
#[test]
fn stretch_width_fills_containing_block_minus_margins() {
    let root = lay(
        "<div class='wrap'><div class='s'>x</div></div>",
        ".wrap { width: 300px; } .s { display: inline-block; width: stretch; margin: 0 10px; }",
    );
    let s = first_element_child(first_element_child(&root));
    assert!((s.rect.width - 280.0).abs() < 0.5, "width={}", s.rect.width);
    let root = lay(
        "<div class='wrap'><div class='s'>x</div></div>",
        ".wrap { width: 300px; } .s { display: inline-block; width: fit-content; margin: 0 10px; }",
    );
    let s = first_element_child(first_element_child(&root));
    assert!(s.rect.width < 100.0, "fit-content must still shrink, width={}", s.rect.width);
}

/// `height: stretch` fills the containing block's definite height minus the
/// box's own vertical margins; against an `auto`-height parent it is `auto`.
#[test]
fn stretch_height_fills_definite_containing_block() {
    let root = lay(
        "<div class='wrap'><div class='s'>x</div></div>",
        ".wrap { width: 300px; height: 200px; } .s { height: stretch; margin: 10px 0 30px; }",
    );
    let s = first_element_child(first_element_child(&root));
    assert!((s.rect.height - 160.0).abs() < 0.5, "height={}", s.rect.height);
    let root = lay(
        "<div class='wrap'><div class='s'>x</div></div>",
        ".wrap { width: 300px; } .s { height: stretch; margin: 10px 0 30px; }",
    );
    let s = first_element_child(first_element_child(&root));
    assert!(s.rect.height < 100.0, "indefinite parent → auto, height={}", s.rect.height);
}

/// `min-width: stretch` / `max-width: stretch` resolve against the containing block.
#[test]
fn stretch_in_min_and_max_width() {
    let root = lay(
        "<div class='wrap'><div class='a'>x</div><div class='b'>x</div></div>",
        ".wrap { width: 300px; } .a { display: inline-block; min-width: stretch; }
         .b { width: 500px; max-width: stretch; }",
    );
    let wrap = first_element_child(&root);
    assert!((wrap.children[0].rect.width - 300.0).abs() < 0.5, "min: {}", wrap.children[0].rect.width);
    assert!((wrap.children[1].rect.width - 300.0).abs() < 0.5, "max: {}", wrap.children[1].rect.width);
}

/// `fit-content(<length>)` functional form: parsed with inner length.
#[test]
fn fit_content_functional_form_parsed() {
    let sheet = lumen_css_parser::parse(".x { width: fit-content(200px); }");
    let doc = lumen_html_parser::parse("<div class='x'>a</div>");
    let vp = Size { width: 800.0, height: 600.0 };
    use crate::style::Length;
    let children = doc.get(doc.body().unwrap()).children.clone();
    let div_id = children.into_iter().find(|&id| {
        matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
    }).unwrap();
    let style = compute_style(&doc, div_id, &sheet, &ComputedStyle::root(), vp, false);
    assert!(
        matches!(style.width, Some(Length::FitContent(Some(_)))),
        "expected FitContent(Some(200px)), got {:?}", style.width
    );
}

/// CSS Multicol L1 §7: lays out `n` 30px blocks in a 3-column container
/// (`width: 300px`, no gap) with the given extra container style and returns
/// the container box.
fn multicol_30px_items(extra: &str, n: usize) -> LayoutBox {
    let items = "<div></div>".repeat(n);
    let root = lay_measured(
        &format!("<div id='c'>{items}</div>"),
        &format!("#c {{ width: 300px; column-count: 3; column-gap: 0px; {extra} }} #c div {{ height: 30px; }}"),
        800.0,
    );
    first_element_child(&root).clone()
}

#[test]
fn multicol_column_fill_auto_without_height_stays_in_first_column() {
    // Edge: `column-fill: auto` with no height limit never opens a second
    // column — three 30px blocks stack to a 90px-tall container.
    let c = multicol_30px_items("column-fill: auto;", 3);
    assert!((c.rect.height - 90.0).abs() < 0.5, "height={}", c.rect.height);
    assert!(c.children.iter().all(|f| f.rect.x < 1.0), "all fragments in column 0");
}

#[test]
fn multicol_column_fill_auto_uses_max_height_as_limit() {
    // Edge: auto-height container, `max-height: 50px` — column 0 fills to 50px,
    // the rest flows on; the container is exactly 50px tall.
    let c = multicol_30px_items("column-fill: auto; max-height: 50px;", 6);
    assert!((c.rect.height - 50.0).abs() < 0.5, "height={}", c.rect.height);
    let col0: f32 = c.children.iter().filter(|f| f.rect.x < 1.0).map(|f| f.rect.height).sum();
    assert!((col0 - 50.0).abs() < 0.5, "column 0 filled to the limit, got {col0}");
}

#[test]
fn multicol_column_fill_balance_is_capped_by_max_height() {
    // Edge: balanced content would need 60px per column, `max-height: 40px`
    // caps the column height, extra content overflows into further columns.
    let c = multicol_30px_items("max-height: 40px;", 6);
    assert!((c.rect.height - 40.0).abs() < 0.5, "height={}", c.rect.height);
    let max_x = c.children.iter().map(|f| f.rect.x).fold(0.0f32, f32::max);
    assert!(max_x > 200.5, "overflow column expected beyond the 3rd, max_x={max_x}");
}

#[test]
fn multicol_column_fill_auto_atomic_items_fill_to_height() {
    // Atomic (bordered) items with `column-fill: auto; height: 100px`: the
    // first column takes as many whole items as fit (3 × 30px = 90px).
    let root = lay_measured(
        "<div id='c'><div></div><div></div><div></div><div></div><div></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; column-fill: auto; height: 100px; } \
         #c div { height: 30px; border: 1px solid #000; }",
        800.0,
    );
    let c = first_element_child(&root);
    let col0 = c.children.iter().filter(|f| f.rect.x < 1.0).count();
    assert_eq!(col0, 3, "column 0 holds 3 atomic items");
}
