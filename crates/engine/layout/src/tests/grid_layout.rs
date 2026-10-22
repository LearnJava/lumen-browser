use super::*;

// --- CSS Grid Layout tests ---

/// Parse `grid-template-columns: 100px 200px 300px`.
#[test]
fn grid_parse_fixed_columns() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: 100px 200px 300px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 3);
    assert_eq!(div.style.grid_template_columns[0], GridTrackSize::Length(Length::Px(100.0)));
    assert_eq!(div.style.grid_template_columns[1], GridTrackSize::Length(Length::Px(200.0)));
    assert_eq!(div.style.grid_template_columns[2], GridTrackSize::Length(Length::Px(300.0)));
}

/// Parse fr units.
#[test]
fn grid_parse_fr_columns() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: 1fr 2fr; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 2);
    assert_eq!(div.style.grid_template_columns[0], GridTrackSize::Fr(1.0));
    assert_eq!(div.style.grid_template_columns[1], GridTrackSize::Fr(2.0));
}

/// Parse `repeat(3, 100px)` — expands to 3 tracks.
#[test]
fn grid_parse_repeat() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: repeat(3, 100px); }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 3);
    for ts in &div.style.grid_template_columns {
        assert_eq!(*ts, GridTrackSize::Length(Length::Px(100.0)));
    }
}

/// Parse `grid-column: 2 / 4`.
#[test]
fn grid_parse_column_shorthand() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-column: 2 / 4; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_column_start, GridLine::Line(2));
    assert_eq!(div.style.grid_column_end, GridLine::Line(4));
}

/// Parse `grid-row: 1 / span 2`.
#[test]
fn grid_parse_row_span() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-row: 1 / span 2; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_row_start, GridLine::Line(1));
    assert_eq!(div.style.grid_row_end, GridLine::Span(2));
}

/// Two equal fr columns should each get half the container width.
#[test]
fn grid_two_fr_columns_equal_width() {
    let root = lay(
        "<body><div><span></span><span></span></div></body>",
        "div { display: grid; grid-template-columns: 1fr 1fr; width: 400px; } \
         span { height: 50px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2, "должно быть 2 grid-item");
    assert!((items[0].rect.width - 200.0).abs() < 1.0, "первый item = 200px, получили {}", items[0].rect.width);
    assert!((items[1].rect.width - 200.0).abs() < 1.0, "второй item = 200px, получили {}", items[1].rect.width);
    // Second item starts at x=200.
    assert!((items[1].rect.x - items[0].rect.x - 200.0).abs() < 1.0);
}

/// Fixed 3-column grid: items placed in row order.
#[test]
fn grid_three_column_auto_placement() {
    let root = lay(
        "<body><div><a></a><a></a><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px 100px; width: 300px; } \
         a { height: 30px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 4);
    // First 3 items on row 1, 4th on row 2.
    assert!((items[0].rect.y - items[1].rect.y).abs() < 1.0, "items 0,1 одна строка");
    assert!((items[1].rect.y - items[2].rect.y).abs() < 1.0, "items 1,2 одна строка");
    assert!(items[3].rect.y > items[0].rect.y + 1.0, "item 4 на второй строке");
    // Column positions.
    assert!(items[0].rect.x < items[1].rect.x, "col 0 < col 1");
    assert!(items[1].rect.x < items[2].rect.x, "col 1 < col 2");
}

/// Explicit grid-column / grid-row placement.
#[test]
fn grid_explicit_placement() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px 100px; \
               grid-template-rows: 50px 50px; width: 300px; } \
         a { grid-column: 3; grid-row: 2; height: 40px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let item = div.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
    // item at column 3, row 2 → x ≈ 200, y ≈ 50.
    assert!((item.rect.x - 200.0).abs() < 1.0, "x≈200, got {}", item.rect.x);
    assert!((item.rect.y - 50.0).abs() < 1.0, "y≈50, got {}", item.rect.y);
}

/// Grid with `gap` between cells.
#[test]
fn grid_gap_applied() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; \
               column-gap: 20px; width: 220px; } \
         a { height: 30px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2);
    // Second item starts at x ≈ 120 (100px col + 20px gap).
    assert!((items[1].rect.x - items[0].rect.x - 120.0).abs() < 1.0,
        "gap: x diff should be 120, got {}", items[1].rect.x - items[0].rect.x);
}

// ──── grid content distribution: align-content / justify-content / place-content
//      (CSS Box Alignment L3 §5, CSS Grid L1 §12.3) ────

/// Grid items of the container, in source order, excluding `Skip` boxes.
fn grid_items(div: &LayoutBox) -> Vec<&LayoutBox> {
    div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect()
}

/// `justify-content: center` centres the fixed column tracks in the inline axis.
#[test]
fn grid_justify_content_center_offsets_columns() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: center; } \
         a { height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert_eq!(items.len(), 2);
    // Free space 400 - 200 = 200 → first track pushed by 100.
    assert!((items[0].rect.x - div.rect.x - 100.0).abs() < 1.0,
        "first column offset by half the free space, got {}", items[0].rect.x - div.rect.x);
    assert!((items[1].rect.x - items[0].rect.x - 100.0).abs() < 1.0,
        "tracks stay adjacent, got {}", items[1].rect.x - items[0].rect.x);
}

/// `justify-content: end` flushes the column tracks to the inline end edge.
#[test]
fn grid_justify_content_end_flushes_columns() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: end; } \
         a { height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert!((items[0].rect.x - div.rect.x - 200.0).abs() < 1.0,
        "first column offset by the whole free space, got {}", items[0].rect.x - div.rect.x);
}

/// `justify-content: space-between` widens the gap between the column tracks.
#[test]
fn grid_justify_content_space_between_widens_gap() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: space-between; } \
         a { height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert!((items[0].rect.x - div.rect.x).abs() < 1.0,
        "first column at the start edge, got {}", items[0].rect.x - div.rect.x);
    // 200px of free space becomes the single in-between gap.
    assert!((items[1].rect.x - items[0].rect.x - 300.0).abs() < 1.0,
        "second column pushed by track + free space, got {}", items[1].rect.x - items[0].rect.x);
}

/// A column-spanning item's cell absorbs the spacing `justify-content` injected.
#[test]
fn grid_justify_content_space_between_widens_spanning_cell() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: space-between; } \
         a { grid-column: 1 / 3; height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // Cell spans track 1 (100) + injected gap (200) + track 2 (100) = 400.
    assert!((items[0].rect.width - 400.0).abs() < 1.0,
        "spanning cell covers the widened gap, got {}", items[0].rect.width);
}

/// `align-content: center` centres the row tracks inside a definite height.
#[test]
fn grid_align_content_center_offsets_rows() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 50px 50px; height: 300px; \
               align-content: center; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert_eq!(items.len(), 2);
    // Free space 300 - 100 = 200 → first row pushed by 100.
    assert!((items[0].rect.y - div.rect.y - 100.0).abs() < 1.0,
        "first row offset by half the free space, got {}", items[0].rect.y - div.rect.y);
    assert!((items[1].rect.y - items[0].rect.y - 50.0).abs() < 1.0,
        "rows stay adjacent, got {}", items[1].rect.y - items[0].rect.y);
}

/// `align-content: space-evenly` spaces the row tracks including both edges.
#[test]
fn grid_align_content_space_evenly_spaces_rows() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 50px 50px; height: 350px; \
               align-content: space-evenly; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // Free 350 - 100 = 250, three equal gaps of 250/3 ≈ 83.33.
    let per = 250.0 / 3.0;
    assert!((items[0].rect.y - div.rect.y - per).abs() < 1.0,
        "first row offset by one share, got {}", items[0].rect.y - div.rect.y);
    assert!((items[1].rect.y - items[0].rect.y - (50.0 + per)).abs() < 1.0,
        "rows separated by track + one share, got {}", items[1].rect.y - items[0].rect.y);
}

/// `align-content: center` on an overflowing grid resolves *unsafely* (CSS Box
/// Alignment L3 §5.3): the tracks shift back past the start edge, like Edge.
#[test]
fn grid_align_content_center_overflow_shifts_past_start_edge() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 100px 100px; height: 50px; \
               align-content: center; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // Free space 50 - 200 = -150 → the first row starts 75px above the edge.
    assert!((items[0].rect.y - div.rect.y + 75.0).abs() < 1.0,
        "overflowing tracks centre unsafely, got {}", items[0].rect.y - div.rect.y);
}

/// `align-content: space-between` on an overflowing grid falls back to `start`
/// (CSS Box Alignment L3 §5.3) instead of producing negative spacing.
#[test]
fn grid_align_content_space_between_overflow_falls_back_to_start() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 100px 100px; height: 50px; \
               align-content: space-between; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert!((items[0].rect.y - div.rect.y).abs() < 1.0,
        "first row at the start edge, got {}", items[0].rect.y - div.rect.y);
    assert!((items[1].rect.y - items[0].rect.y - 100.0).abs() < 1.0,
        "rows stay adjacent, no negative spacing, got {}", items[1].rect.y - items[0].rect.y);
}

/// `place-content: <align-content> <justify-content>` reaches both grid axes.
#[test]
fn grid_place_content_shorthand_applies_to_both_axes() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 100px; width: 400px; height: 300px; \
               place-content: end center; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // align-content: end → y offset by the whole 200px of block free space.
    assert!((items[0].rect.y - div.rect.y - 200.0).abs() < 1.0,
        "align-content: end flushes the row, got {}", items[0].rect.y - div.rect.y);
    // justify-content: center → x offset by half the 300px of inline free space.
    assert!((items[0].rect.x - div.rect.x - 150.0).abs() < 1.0,
        "justify-content: center centres the column, got {}", items[0].rect.x - div.rect.x);
}

/// Default `align-content: normal` behaves as `stretch`: auto rows share the
/// leftover block space of a definitely-sized container.
#[test]
fn grid_align_content_normal_stretches_auto_rows() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: auto auto; height: 200px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert_eq!(items.len(), 2);
    // Both auto rows grow to 100px each → the second row starts halfway down.
    assert!((items[1].rect.y - items[0].rect.y - 100.0).abs() < 1.0,
        "auto rows share the free space, got {}", items[1].rect.y - items[0].rect.y);
}

/// `grid-auto-flow: column` places items vertically first.
#[test]
fn grid_auto_flow_column() {
    let root = lay(
        "<body><div><a></a><a></a><a></a></div></body>",
        "div { display: grid; grid-template-rows: 50px 50px; \
               grid-auto-flow: column; width: 300px; } \
         a { width: 80px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 3);
    // items 0,1 same column (different y); item 2 in next column.
    assert!((items[0].rect.x - items[1].rect.x).abs() < 1.0, "items 0,1 same column");
    assert!(items[2].rect.x > items[0].rect.x + 1.0, "item 2 next column");
}

/// `minmax(50px, 1fr)` — explicit minmax() track.
#[test]
fn grid_parse_minmax() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: minmax(50px, 1fr); }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 1);
    assert!(matches!(div.style.grid_template_columns[0], GridTrackSize::Minmax(_, _)));
}

/// `grid-area` shorthand parses `row-start / col-start / row-end / col-end`.
#[test]
fn grid_parse_area_shorthand() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-area: 2 / 1 / 4 / 3; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_row_start, GridLine::Line(2));
    assert_eq!(div.style.grid_column_start, GridLine::Line(1));
    assert_eq!(div.style.grid_row_end, GridLine::Line(4));
    assert_eq!(div.style.grid_column_end, GridLine::Line(3));
}

/// `display: grid` container has no height when empty.
#[test]
fn grid_empty_container_zero_height() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.rect.height, 0.0, "empty grid should have 0 height");
}

/// Auto rows sized by content.
#[test]
fn grid_auto_row_height_from_content() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; width: 100px; } \
         a { height: 80px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    // Container height should accommodate the 80px item.
    assert!(div.rect.height >= 80.0, "grid height should be ≥80px, got {}", div.rect.height);
}

// ── CSS Grid named areas ──────────────────────────────────────────────────

/// `parse_grid_template_areas` — 2×2 grid with named areas.
#[test]
fn grid_template_areas_parse_2x2() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas(r#""header header" "sidebar main""#);
    assert_eq!(areas.len(), 2, "should have 2 rows");
    assert_eq!(areas[0], vec!["header", "header"]);
    assert_eq!(areas[1], vec!["sidebar", "main"]);
}

/// `parse_grid_template_areas` — single row.
#[test]
fn grid_template_areas_parse_single_row() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas(r#""a b c""#);
    assert_eq!(areas, vec![vec!["a", "b", "c"]]);
}

/// `parse_grid_template_areas` — `none` returns empty.
#[test]
fn grid_template_areas_none() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas("none");
    assert!(areas.is_empty());
}

/// `parse_grid_template_areas` — dot (.) cells are stored as-is.
#[test]
fn grid_template_areas_dot_cells() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas(r#""a . b""#);
    assert_eq!(areas[0], vec!["a", ".", "b"]);
}

/// `GridLine::parse` recognises named area idents.
#[test]
fn grid_line_parse_named_ident() {
    use crate::GridLine;
    assert_eq!(GridLine::parse("main"), Some(GridLine::Named("main".into())));
    assert_eq!(GridLine::parse("header-area"), Some(GridLine::Named("header-area".into())));
    assert_eq!(GridLine::parse("auto"), Some(GridLine::Auto));
    assert_eq!(GridLine::parse("2"), Some(GridLine::Line(2)));
    // digit-only or empty → not an ident
    assert_eq!(GridLine::parse("3abc"), None);
}

/// `grid-area: <name>` shorthand sets all four placement properties to Named.
#[test]
fn grid_area_named_sets_all_four() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-area: main; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_row_start,    GridLine::Named("main".into()));
    assert_eq!(div.style.grid_row_end,      GridLine::Named("main".into()));
    assert_eq!(div.style.grid_column_start, GridLine::Named("main".into()));
    assert_eq!(div.style.grid_column_end,   GridLine::Named("main".into()));
}

/// `grid-template-areas` stored on container after cascade.
#[test]
fn grid_template_areas_stored_on_container() {
    let root = lay(
        "<body><div></div></body>",
        r#"div { display: grid; grid-template-areas: "header header" "sidebar main"; }"#,
    );
    let body = &root;
    let div = first_element_child(body);
    let areas = &div.style.grid_template_areas;
    assert_eq!(areas.len(), 2, "should have 2 rows");
    assert_eq!(areas[0], vec!["header", "header"]);
    assert_eq!(areas[1], vec!["sidebar", "main"]);
}

/// Named area layout: a 2×2 grid where items reference areas by name.
///
/// ```css
/// .grid {
///   display: grid;
///   grid-template-columns: 100px 100px;
///   grid-template-rows: 50px 50px;
///   grid-template-areas: "a b" "a c";
///   width: 200px;
/// }
/// .item-a { grid-area: a; }  /* row 1–3, col 1–2 */
/// .item-b { grid-area: b; }  /* row 1–2, col 2–3 */
/// .item-c { grid-area: c; }  /* row 2–3, col 2–3 */
/// ```
#[test]
fn grid_named_areas_layout_placement() {
    let root = lay(
        "<body><div><span id='a'></span><span id='b'></span><span id='c'></span></div></body>",
        r#"
        div {
            display: grid;
            grid-template-columns: 100px 100px;
            grid-template-rows: 50px 50px;
            grid-template-areas: "a b" "a c";
            width: 200px;
        }
        #a { grid-area: a; }
        #b { grid-area: b; }
        #c { grid-area: c; }
        "#,
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 3, "3 named-area items");
    let item_a = &items[0];
    let item_b = &items[1];
    let item_c = &items[2];
    // item-a occupies rows 1-2 (height=100) at column 1 (x=0, width=100)
    assert!((item_a.rect.x - 0.0).abs() < 1.0,  "a.x should be 0, got {}", item_a.rect.x);
    assert!((item_a.rect.width - 100.0).abs() < 1.0, "a.w should be 100, got {}", item_a.rect.width);
    assert!((item_a.rect.height - 100.0).abs() < 1.0, "a.h should be 100 (2 rows), got {}", item_a.rect.height);
    // item-b occupies row 1 at column 2 (x=100, width=100, height=50)
    assert!((item_b.rect.x - 100.0).abs() < 1.0, "b.x should be 100, got {}", item_b.rect.x);
    assert!((item_b.rect.y - 0.0).abs() < 1.0,   "b.y should be 0, got {}", item_b.rect.y);
    assert!((item_b.rect.width - 100.0).abs() < 1.0, "b.w should be 100, got {}", item_b.rect.width);
    assert!((item_b.rect.height - 50.0).abs() < 1.0, "b.h should be 50, got {}", item_b.rect.height);
    // item-c occupies row 2 at column 2 (y=50, width=100, height=50)
    assert!((item_c.rect.x - 100.0).abs() < 1.0, "c.x should be 100, got {}", item_c.rect.x);
    assert!((item_c.rect.y - 50.0).abs() < 1.0,  "c.y should be 50, got {}", item_c.rect.y);
    assert!((item_c.rect.width - 100.0).abs() < 1.0, "c.w should be 100, got {}", item_c.rect.width);
    assert!((item_c.rect.height - 50.0).abs() < 1.0, "c.h should be 50, got {}", item_c.rect.height);
}

/// Named area with a span > 1 row: area "sidebar" spans both rows.
#[test]
fn grid_named_area_spanning_rows() {
    let root = lay(
        "<body><div><span id='h'></span><span id='s'></span></div></body>",
        r#"
        div {
            display: grid;
            grid-template-columns: 200px 600px;
            grid-template-rows: 80px 80px;
            grid-template-areas: "header header" "sidebar content";
            width: 800px;
        }
        #h { grid-area: header; }
        #s { grid-area: sidebar; }
        "#,
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    // header spans both columns: x=0, w=800, y=0, h=80
    let header = &items[0];
    assert!((header.rect.x - 0.0).abs() < 1.0,    "h.x={}", header.rect.x);
    assert!((header.rect.width - 800.0).abs() < 1.0, "h.w={}", header.rect.width);
    assert!((header.rect.y - 0.0).abs() < 1.0,    "h.y={}", header.rect.y);
    assert!((header.rect.height - 80.0).abs() < 1.0, "h.h={}", header.rect.height);
    // sidebar: x=0, w=200, y=80, h=80
    let sidebar = &items[1];
    assert!((sidebar.rect.x - 0.0).abs() < 1.0,   "s.x={}", sidebar.rect.x);
    assert!((sidebar.rect.width - 200.0).abs() < 1.0, "s.w={}", sidebar.rect.width);
    assert!((sidebar.rect.y - 80.0).abs() < 1.0,  "s.y={}", sidebar.rect.y);
}

// ── grid-auto-flow: dense ────────────────────────────────────────────────

/// Dense row packing fills the gap left by a wide item.
///
///  3 cols, A and B each span 2 cols; C and D are 1×1.
///
///  Sparse (row):             Dense (row dense):
///  +---+---+---+             +---+---+---+
///  | A   A |   |             | A   A | C |  ← C fills gap in row 1
///  +---+---+---+             +---+---+---+
///  | B   B | C |             | B   B | D |  ← D fills gap in row 2
///  +---+---+---+             +---+---+---+
///  | D |       |
///  +---+---+---+
#[test]
fn grid_dense_row_fills_gap() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span>\
           <span id='b'></span>\
           <span id='c'></span>\
           <span id='d'></span>\
         </div></body>",
        r#"
        #g {
            display: grid;
            grid-template-columns: 100px 100px 100px;
            grid-auto-rows: 50px;
            grid-auto-flow: row dense;
            width: 300px;
        }
        #a { grid-column: span 2; }
        #b { grid-column: span 2; }
        /* c, d: auto 1×1 */
        "#,
    );
    let body = &root;
    let grid = first_element_child(body);
    let items: Vec<_> = grid.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 4, "expected 4 items");

    let a = &items[0];
    let b = &items[1];
    let c = &items[2];
    let d = &items[3];

    // A: cols 1-2, row 1 → x=0, w=200, y=0
    assert!((a.rect.x - 0.0).abs() < 1.0,     "a.x={}", a.rect.x);
    assert!((a.rect.width - 200.0).abs() < 1.0, "a.w={}", a.rect.width);
    assert!((a.rect.y - 0.0).abs() < 1.0,     "a.y={}", a.rect.y);

    // B: cols 1-2, row 2 → x=0, w=200, y=50
    assert!((b.rect.x - 0.0).abs() < 1.0,     "b.x={}", b.rect.x);
    assert!((b.rect.width - 200.0).abs() < 1.0, "b.w={}", b.rect.width);
    assert!((b.rect.y - 50.0).abs() < 1.0,    "b.y={}", b.rect.y);

    // Dense: C fills the gap at col 3, row 1 → x=200, y=0
    assert!((c.rect.x - 200.0).abs() < 1.0, "c.x={}: dense must fill row-1 gap", c.rect.x);
    assert!((c.rect.y - 0.0).abs() < 1.0,   "c.y={}: dense must fill row-1 gap", c.rect.y);

    // Dense: D fills the gap at col 3, row 2 → x=200, y=50
    assert!((d.rect.x - 200.0).abs() < 1.0, "d.x={}: dense must fill row-2 gap", d.rect.x);
    assert!((d.rect.y - 50.0).abs() < 1.0,  "d.y={}: dense must fill row-2 gap", d.rect.y);
}

/// Sparse layout must NOT back-fill: C stays in row 2 (after B), D in row 3.
///
///  Same grid: A(span2), B(span2), C(1×1), D(1×1) with `grid-auto-flow: row`.
///  Col-3 gap in row 1 is skipped by the forward-only cursor.
#[test]
fn grid_sparse_row_no_backfill() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span>\
           <span id='b'></span>\
           <span id='c'></span>\
           <span id='d'></span>\
         </div></body>",
        r#"
        #g {
            display: grid;
            grid-template-columns: 100px 100px 100px;
            grid-auto-rows: 50px;
            grid-auto-flow: row;
            width: 300px;
        }
        #a { grid-column: span 2; }
        #b { grid-column: span 2; }
        "#,
    );
    let body = &root;
    let grid = first_element_child(body);
    let items: Vec<_> = grid.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 4, "expected 4 items");

    let c = &items[2];
    let d = &items[3];

    // Sparse: C ends up at col 3, row 2 (not row 1 — cursor didn't go back).
    assert!((c.rect.x - 200.0).abs() < 1.0, "c.x={}: sparse must not back-fill col3 row1", c.rect.x);
    assert!((c.rect.y - 50.0).abs() < 1.0,  "c.y={}: sparse must not back-fill col3 row1", c.rect.y);

    // D ends up at col 1, row 3 (cursor advanced past row 2).
    assert!((d.rect.y - 100.0).abs() < 1.0, "d.y={}: sparse must not back-fill", d.rect.y);
}

// ── BUG-801: auto-placement must terminate when an item needs a column
//    past the last explicit line ─────────────────────────────────────────
//
// Three shapes that used to hang `lay_out_grid`'s Pass-2 scan forever: an
// explicit column start beyond the explicit grid, a span wider than the
// explicit grid, and both combined. Each must finish and grow the implicit
// grid to fit the item (CSS Grid L1 §7.1) rather than refusing placement.
// A regression here reintroduces an infinite loop, not a wrong pixel — if
// this test never returns, that is the failure, not a panic.

/// `grid-column: 3` on a 2-column grid, row auto: the fixed column start
/// is beyond the explicit grid, so the scan must not search columns at
/// all for this item — only occupancy decides the row.
#[test]
fn grid_column_start_beyond_explicit_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span><span id='b'></span></div></body>",
        "#g { display: grid; grid-template-columns: 100px 100px; column-gap: 4px; width: 400px; } \
         #a { grid-column: 3; height: 20px; } \
         #b { height: 20px; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 2);
    let a = &items[0];
    let b = &items[1];
    // A lands in the implicit 3rd column (past the 2 explicit + gap), row 1.
    assert!(a.rect.x > 200.0, "a.x={}: must be in the implicit 3rd column", a.rect.x);
    assert!((a.rect.y - 0.0).abs() < 1.0, "a.y={}", a.rect.y);
    // B auto-places on row 2, column 1 — placing A's fixed column also
    // advances the sparse cursor forward past row 1, same as it would for
    // any other placed item.
    assert!((b.rect.x - 0.0).abs() < 1.0, "b.x={}", b.rect.x);
    assert!(b.rect.y > a.rect.y, "b.y={} must be below a.y={}", b.rect.y, a.rect.y);
}

/// `grid-column: span 3` on a 2-column grid, both axes otherwise auto: the
/// item's own span exceeds the explicit column count, so the column bound
/// used by the scan must grow to the span or no starting column ever fits.
#[test]
fn grid_span_wider_than_explicit_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span><span id='b'></span></div></body>",
        "#g { display: grid; grid-template-columns: 100px 100px; width: 400px; } \
         #a { grid-column: span 3; height: 20px; } \
         #b { height: 20px; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 2);
    let a = &items[0];
    let b = &items[1];
    // A must start at column 1 (the only column a span-3 item can ever fit)
    // and span the grown implicit grid.
    assert!((a.rect.x - 0.0).abs() < 1.0, "a.x={}", a.rect.x);
    assert!((a.rect.width - 400.0).abs() < 1.0, "a.w={}: spans the whole grown grid", a.rect.width);
    // B auto-places on the next row — row 1 is fully occupied by A's span.
    assert!(b.rect.y > a.rect.y, "b.y={} must be below a.y={}", b.rect.y, a.rect.y);
}

/// `grid-column: 9 / span 2` on a 2-column grid: both defects from the bug
/// report at once — explicit start past the grid, spanning further still.
#[test]
fn grid_explicit_start_and_span_beyond_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span></div></body>",
        "#g { display: grid; grid-template-columns: 100px 100px; width: 400px; } \
         #a { grid-column: 9 / span 2; height: 20px; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 1);
    // Must land past the 2 explicit columns (200px), in implicit columns
    // 9-10 — those, and the 6 unused implicit columns before them, share
    // the leftover width, so the item does not reach the container edge.
    assert!(items[0].rect.x > 200.0, "x={}: must be past the explicit grid", items[0].rect.x);
}

/// Dense column-flow mirror of the row-flow fix: `grid-row: span 3` on a
/// 2-row grid must grow the implicit row count rather than hang.
#[test]
fn grid_row_span_wider_than_explicit_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span></div></body>",
        "#g { display: grid; grid-template-rows: 50px 50px; grid-auto-flow: column; \
              grid-auto-columns: 100px; height: 100px; } \
         #a { grid-row: span 3; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 1);
    // Reaching this assertion at all is the point — the scan terminated.
    assert!((items[0].rect.y - 0.0).abs() < 1.0, "y={}", items[0].rect.y);
}

/// Dense column flow: small items back-fill gaps left by tall items in earlier columns.
///
///  2 cols, 3 explicit rows (50px).
///  A spans 2 rows (col 1, rows 1-2); B spans 3 rows (col 2, rows 1-3).
///  Dense: C fills the remaining slot in col 1, row 3.
///  Sparse: C would continue forward to col 3 (outside the explicit grid).
#[test]
fn grid_dense_column_fills_gap() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span>\
           <span id='b'></span>\
           <span id='c'></span>\
         </div></body>",
        r#"
        #g {
            display: grid;
            grid-template-columns: 100px 100px;
            grid-template-rows: 50px 50px 50px;
            grid-auto-flow: column dense;
            width: 200px;
        }
        #a { grid-row: span 2; }
        #b { grid-row: span 3; }
        /* c: auto 1×1 */
        "#,
    );
    let body = &root;
    let grid = first_element_child(body);
    let items: Vec<_> = grid.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 3, "expected 3 items");

    let a = &items[0];
    let b = &items[1];
    let c = &items[2];

    // A: col 1, rows 1-2 → x=0, y=0, h=100
    assert!((a.rect.x - 0.0).abs() < 1.0,      "a.x={}", a.rect.x);
    assert!((a.rect.y - 0.0).abs() < 1.0,      "a.y={}", a.rect.y);
    assert!((a.rect.height - 100.0).abs() < 1.0, "a.h={}", a.rect.height);

    // B: col 2, rows 1-3 → x=100, y=0, h=150
    assert!((b.rect.x - 100.0).abs() < 1.0,    "b.x={}", b.rect.x);
    assert!((b.rect.y - 0.0).abs() < 1.0,      "b.y={}", b.rect.y);
    assert!((b.rect.height - 150.0).abs() < 1.0, "b.h={}", b.rect.height);

    // Dense: C fills col 1 row 3 → x=0, y=100
    assert!((c.rect.x - 0.0).abs() < 1.0,   "c.x={}: dense col must back-fill col1 row3", c.rect.x);
    assert!((c.rect.y - 100.0).abs() < 1.0, "c.y={}: dense col must back-fill col1 row3", c.rect.y);
}


// ──────────────── `grid-template` / `grid` shorthand (CSS Grid L1 §7.4, §8.2) ────────────────

/// Стиль контейнера `<div>` после каскада с заданным CSS.
fn grid_div_style(css: &str) -> std::sync::Arc<crate::style::ComputedStyle> {
    let root = lay("<body><div></div></body>", css);
    first_element_child(&root).style.clone()
}

/// `grid-template: <rows> / <columns>` — обе оси, areas пусты.
#[test]
fn grid_template_rows_slash_columns() {
    let s = grid_div_style("div { display: grid; grid-template: 50px auto / 1fr 2fr; }");
    assert_eq!(s.grid_template_rows, vec![GridTrackSize::Length(Length::Px(50.0)), GridTrackSize::Auto]);
    assert_eq!(s.grid_template_columns, vec![GridTrackSize::Fr(1.0), GridTrackSize::Fr(2.0)]);
    assert!(s.grid_template_areas.is_empty());
}

/// `grid-template` со строками: areas + размер ряда + колонки после `/`.
#[test]
fn grid_template_areas_rows_and_columns() {
    let s = grid_div_style(
        r#"div { display: grid; grid-template: "a a" 40px "b c" 1fr / 100px 200px; }"#,
    );
    assert_eq!(s.grid_template_areas, vec![vec!["a", "a"], vec!["b", "c"]]);
    assert_eq!(s.grid_template_rows, vec![GridTrackSize::Length(Length::Px(40.0)), GridTrackSize::Fr(1.0)]);
    assert_eq!(
        s.grid_template_columns,
        vec![GridTrackSize::Length(Length::Px(100.0)), GridTrackSize::Length(Length::Px(200.0))]
    );
}

/// Ряды без размера — `auto`; колонки без `/` остаются initial.
#[test]
fn grid_template_areas_only_rows_default_auto() {
    let s = grid_div_style(r#"div { display: grid; grid-template: "a b" "c d"; }"#);
    assert_eq!(s.grid_template_areas.len(), 2);
    assert_eq!(s.grid_template_rows, vec![GridTrackSize::Auto, GridTrackSize::Auto]);
    assert!(s.grid_template_columns.is_empty());
}

/// Шортхенд сбрасывает ранее заданные longhand-ы (в т.ч. areas).
#[test]
fn grid_template_resets_previous_longhands() {
    let s = grid_div_style(
        r#"div { display: grid; grid-template-areas: "x y"; grid-template-columns: 9px;
                 grid-template: none; }"#,
    );
    assert!(s.grid_template_areas.is_empty());
    assert!(s.grid_template_columns.is_empty());
    assert!(s.grid_template_rows.is_empty());
}

/// Невалидное значение (нет `/` и нет строк) — декларация игнорируется.
#[test]
fn grid_template_invalid_is_ignored() {
    let s = grid_div_style(
        "div { display: grid; grid-template-columns: 10px 20px; grid-template: 1fr 1fr; }",
    );
    assert_eq!(s.grid_template_columns.len(), 2);
    assert_eq!(s.grid_template_columns[0], GridTrackSize::Length(Length::Px(10.0)));
}

/// Непрямоугольные areas — декларация целиком невалидна.
#[test]
fn grid_template_ragged_areas_ignored() {
    let s = grid_div_style(r#"div { display: grid; grid-template: "a b" "c"; }"#);
    assert!(s.grid_template_areas.is_empty());
    assert!(s.grid_template_rows.is_empty());
}

/// Имена линий `[a]` пропускаются и не ломают разбор.
#[test]
fn grid_template_line_names_skipped() {
    let s = grid_div_style(
        r#"div { display: grid; grid-template: [r1] "a b" 30px [r2] / [c1] 1fr [c2] 2fr; }"#,
    );
    assert_eq!(s.grid_template_rows, vec![GridTrackSize::Length(Length::Px(30.0))]);
    assert_eq!(s.grid_template_columns, vec![GridTrackSize::Fr(1.0), GridTrackSize::Fr(2.0)]);
}

/// `grid: <rows> / auto-flow dense <auto-cols>` — колоночный поток.
#[test]
fn grid_shorthand_auto_flow_columns_form() {
    let s = grid_div_style("div { display: grid; grid: 100px 200px / auto-flow dense 40px; }");
    assert_eq!(s.grid_template_rows.len(), 2);
    assert!(s.grid_template_columns.is_empty());
    assert_eq!(s.grid_auto_flow, GridAutoFlow::ColumnDense);
    assert_eq!(s.grid_auto_columns, vec![GridTrackSize::Length(Length::Px(40.0))]);
    assert_eq!(s.grid_auto_rows, Vec::<GridTrackSize>::new());
}

/// `grid: auto-flow <auto-rows> / <columns>` — рядный поток.
#[test]
fn grid_shorthand_auto_flow_rows_form() {
    let s = grid_div_style("div { display: grid; grid: auto-flow 60px / 1fr 1fr 1fr; }");
    assert_eq!(s.grid_auto_flow, GridAutoFlow::Row);
    assert_eq!(s.grid_auto_rows, vec![GridTrackSize::Length(Length::Px(60.0))]);
    assert_eq!(s.grid_template_columns.len(), 3);
    assert!(s.grid_template_rows.is_empty());
}

/// `grid: auto-flow` без размера — `grid-auto-rows: auto`; `dense` в любом порядке.
#[test]
fn grid_shorthand_auto_flow_dense_any_order() {
    let s = grid_div_style("div { display: grid; grid: dense auto-flow / 1fr; }");
    assert_eq!(s.grid_auto_flow, GridAutoFlow::RowDense);
    assert_eq!(s.grid_auto_rows, Vec::<GridTrackSize>::new());
}

/// `grid: <template>` форма сбрасывает `grid-auto-*`.
#[test]
fn grid_shorthand_template_form_resets_auto() {
    let s = grid_div_style(
        r#"div { display: grid; grid-auto-flow: column dense; grid-auto-rows: 7px;
                 grid-auto-columns: 8px; grid: "a b" 20px / 1fr 1fr; }"#,
    );
    assert_eq!(s.grid_auto_flow, GridAutoFlow::Row);
    assert_eq!(s.grid_auto_rows, Vec::<GridTrackSize>::new());
    assert_eq!(s.grid_auto_columns, Vec::<GridTrackSize>::new());
    assert_eq!(s.grid_template_areas, vec![vec!["a", "b"]]);
    assert_eq!(s.grid_template_columns.len(), 2);
}

/// `auto-flow` с обеих сторон `/` — невалидно, ничего не меняется.
#[test]
fn grid_shorthand_auto_flow_both_sides_ignored() {
    let s = grid_div_style(
        "div { display: grid; grid-auto-rows: 7px; grid: auto-flow / auto-flow; }",
    );
    assert_eq!(s.grid_auto_rows, vec![GridTrackSize::Length(Length::Px(7.0))]);
}

/// `grid: none` сбрасывает всё.
#[test]
fn grid_shorthand_none_resets_all() {
    let s = grid_div_style(
        "div { display: grid; grid-template-columns: 5px; grid-auto-flow: column; grid: none; }",
    );
    assert!(s.grid_template_columns.is_empty());
    assert_eq!(s.grid_auto_flow, GridAutoFlow::Row);
}

/// Сквозной layout: `grid: "a b" 50px / 100px 100px` + `grid-area` размещает элементы.
#[test]
fn grid_shorthand_areas_layout_placement() {
    let root = lay(
        "<body><div><span id='a'></span><span id='b'></span></div></body>",
        r#"div { display: grid; grid: "a b" 50px / 100px 100px; width: 200px; }
           #a { grid-area: a; } #b { grid-area: b; }"#,
    );
    let div = first_element_child(&root);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2);
    assert!((items[0].rect.x - 0.0).abs() < 1.0, "a.x={}", items[0].rect.x);
    assert!((items[1].rect.x - 100.0).abs() < 1.0, "b.x={}", items[1].rect.x);
    assert!((items[0].rect.height - 50.0).abs() < 1.0, "a.h={}", items[0].rect.height);
}

// ── <grid-line>: отрицательные номера, именованные линии, span <name> ─────

/// Раскладка одного элемента `#a` в сетке 300px; возвращает его rect.
fn place_in_3x3(template: &str, item_css: &str) -> (f32, f32, f32, f32) {
    let root = lay(
        "<body><div id='g'><span id='a'></span></div></body>",
        &format!(
            "#g {{ display: grid; width: 300px; {template} }} #a {{ {item_css} }}"
        ),
    );
    let items = grid_items(first_element_child(&root));
    let r = &items[0].rect;
    (r.x, r.y, r.width, r.height)
}

const COLS_3: &str = "grid-template-columns: 100px 100px 100px; \
                      grid-template-rows: 40px 40px 40px;";

/// `GridLine::parse`: все формы §8.3.
#[test]
fn grid_line_parse_all_forms() {
    use crate::GridLine as L;
    assert_eq!(L::parse("-1"), Some(L::Line(-1)));
    assert_eq!(L::parse("0"), None);
    assert_eq!(L::parse("span 2"), Some(L::Span(2)));
    assert_eq!(L::parse("span 0"), None);
    assert_eq!(L::parse("span foo"), Some(L::SpanNamed("foo".into(), 1)));
    assert_eq!(L::parse("span 2 foo"), Some(L::SpanNamed("foo".into(), 2)));
    assert_eq!(L::parse("foo span 2"), None);
    assert_eq!(L::parse("span foo 3"), Some(L::SpanNamed("foo".into(), 3)));
    assert_eq!(L::parse("2 foo"), Some(L::NamedLine("foo".into(), 2)));
    assert_eq!(L::parse("foo 2"), Some(L::NamedLine("foo".into(), 2)));
    assert_eq!(L::parse("-1 foo"), Some(L::NamedLine("foo".into(), -1)));
    assert_eq!(L::parse("0 foo"), None);
    assert_eq!(L::parse("auto foo"), None);
    assert_eq!(L::parse("1 2"), None);
}

/// `grid-column: 1 / -1` — от первой до последней линии явной сетки.
#[test]
fn grid_negative_line_spans_whole_explicit_grid() {
    let (x, _, w, _) = place_in_3x3(COLS_3, "grid-column: 1 / -1;");
    assert!((x - 0.0).abs() < 1.0 && (w - 300.0).abs() < 1.0, "x={x} w={w}");
}

/// `grid-row: -2 / -1` — последняя строка.
#[test]
fn grid_negative_row_lines_pick_last_row() {
    let (_, y, _, h) = place_in_3x3(COLS_3, "grid-row: -2 / -1;");
    assert!((y - 80.0).abs() < 1.0 && (h - 40.0).abs() < 1.0, "y={y} h={h}");
}

/// `grid-column-start: -2` на сетке из 3 столбцов — линия 3, end авто → один трек.
#[test]
fn grid_negative_start_only() {
    let (x, _, w, _) = place_in_3x3(COLS_3, "grid-column-start: -2;");
    assert!((x - 200.0).abs() < 1.0 && (w - 100.0).abs() < 1.0, "x={x} w={w}");
}

/// `3 / 1` — линии меняются местами (§8.3.1).
#[test]
fn grid_reversed_lines_swap() {
    let (x, _, w, _) = place_in_3x3(COLS_3, "grid-column: 3 / 1;");
    assert!((x - 0.0).abs() < 1.0 && (w - 200.0).abs() < 1.0, "x={x} w={w}");
}

/// Именованные линии: `[a] 100px [b] 100px [c] 100px [d]`, `grid-column: b / d`.
#[test]
fn grid_named_lines_start_end() {
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: [a] 100px [b] 100px [c] 100px [d]; grid-template-rows: 40px;",
        "grid-column: b / d;",
    );
    assert!((x - 100.0).abs() < 1.0 && (w - 200.0).abs() < 1.0, "x={x} w={w}");
}

/// `grid-column: x` копирует имя на end; обе стороны — первая линия `x` → span 1.
#[test]
fn grid_single_named_line_shorthand_copies_to_end() {
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: 100px [x] 100px [x] 100px;",
        "grid-column: x;",
    );
    assert!((x - 100.0).abs() < 1.0 && (w - 100.0).abs() < 1.0, "x={x} w={w}");
}

/// `<integer> <name>` — N-я линия с именем.
#[test]
fn grid_nth_named_line() {
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: [x] 100px [x] 100px [x] 100px [x];",
        "grid-column: 2 x / 4 x;",
    );
    assert!((x - 100.0).abs() < 1.0 && (w - 200.0).abs() < 1.0, "x={x} w={w}");
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: [x] 100px [x] 100px [x] 100px [x];",
        "grid-column: -2 x / -1 x;",
    );
    assert!((x - 200.0).abs() < 1.0 && (w - 100.0).abs() < 1.0, "x={x} w={w}");
}

/// `span <name>` ищет следующую линию с именем от противоположной стороны.
#[test]
fn grid_span_to_named_line() {
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: [s] 100px [m] 100px [m] 100px [e];",
        "grid-column: s / span m;",
    );
    assert!((x - 0.0).abs() < 1.0 && (w - 100.0).abs() < 1.0, "x={x} w={w}");
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: [s] 100px [m] 100px [m] 100px [e];",
        "grid-column: s / span 2 m;",
    );
    assert!((x - 0.0).abs() < 1.0 && (w - 200.0).abs() < 1.0, "x={x} w={w}");
}

/// `span <name> / <line>` — назад от end.
#[test]
fn grid_span_named_backward_from_end() {
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: [m] 100px [m] 100px [m] 100px [e];",
        "grid-column: span m / e;",
    );
    assert!((x - 200.0).abs() < 1.0 && (w - 100.0).abs() < 1.0, "x={x} w={w}");
}

/// Неявные линии области: `grid-column: main-start / main-end`.
#[test]
fn grid_implicit_area_lines() {
    let (x, y, w, h) = place_in_3x3(
        "grid-template-columns: 100px 100px 100px; grid-template-rows: 40px 40px; \
         grid-template-areas: \". main main\" \". main main\";",
        "grid-column: main-start / main-end; grid-row: main-start / main-end;",
    );
    assert!((x - 100.0).abs() < 1.0 && (w - 200.0).abs() < 1.0, "x={x} w={w}");
    assert!((y - 0.0).abs() < 1.0 && (h - 80.0).abs() < 1.0, "y={y} h={h}");
}

/// Явная сетка без треков в шаблоне берётся из `grid-template-areas`.
#[test]
fn grid_negative_line_uses_areas_extent() {
    let root = lay(
        "<body><div id='g'><span id='a'></span></div></body>",
        "#g { display: grid; width: 200px; grid-auto-columns: 100px; \
         grid-template-areas: \"a b\"; } #a { grid-column: 1 / -1; }",
    );
    let r = &grid_items(first_element_child(&root))[0].rect;
    assert!((r.width - 200.0).abs() < 1.0, "w={}", r.width);
}

/// Область (`b-start`/`b-end`) приоритетнее линии `b` без суффикса.
#[test]
fn grid_area_name_beats_plain_line_name() {
    let (x, _, w, _) = place_in_3x3(
        "grid-template-columns: 100px 100px 100px; grid-template-rows: 40px; \
         grid-template-areas: \". b b\";",
        "grid-column: b;",
    );
    assert!((x - 100.0).abs() < 1.0 && (w - 200.0).abs() < 1.0, "x={x} w={w}");
}

/// Шортхенд: невалидная часть — вся декларация игнорируется.
#[test]
fn grid_column_shorthand_invalid_part_ignored() {
    let s = grid_div_style("div { grid-column: 2 / 4; grid-column: 3 / 3abc; }");
    assert_eq!(s.grid_column_start, GridLine::Line(2));
    assert_eq!(s.grid_column_end, GridLine::Line(4));
}

/// `grid-column: 3` сбрасывает end в `auto` (а не оставляет прежний).
#[test]
fn grid_column_shorthand_single_resets_end() {
    let s = grid_div_style("div { grid-column: 2 / 4; grid-column: 3; }");
    assert_eq!(s.grid_column_start, GridLine::Line(3));
    assert_eq!(s.grid_column_end, GridLine::Auto);
    let s = grid_div_style("div { grid-column: foo; }");
    assert_eq!(s.grid_column_end, GridLine::Named("foo".into()));
}

/// `grid-area`: пропущенные значения копируются из противоположной стороны,
/// если это `<custom-ident>`, иначе `auto` (§8.4).
#[test]
fn grid_area_omitted_values() {
    let s = grid_div_style("div { grid-area: a / b; }");
    assert_eq!(s.grid_row_start, GridLine::Named("a".into()));
    assert_eq!(s.grid_column_start, GridLine::Named("b".into()));
    assert_eq!(s.grid_row_end, GridLine::Named("a".into()));
    assert_eq!(s.grid_column_end, GridLine::Named("b".into()));
    let s = grid_div_style("div { grid-area: 2 / 3; }");
    assert_eq!(s.grid_row_end, GridLine::Auto);
    assert_eq!(s.grid_column_end, GridLine::Auto);
    let s = grid_div_style("div { grid-area: 2 / 3 / 4; }");
    assert_eq!(s.grid_column_end, GridLine::Auto);
    assert_eq!(s.grid_row_end, GridLine::Line(4));
}

/// Имена линий сохраняются (развёрнутый `repeat`, объединение на стыках).
#[test]
fn grid_template_line_names_stored() {
    let s = grid_div_style(
        "div { display: grid; grid-template-columns: [a b] 10px [c] repeat(2, [r] 5px [s]) [z]; }",
    );
    let names = &s.grid_template_col_line_names;
    assert_eq!(names.len(), 4, "{names:?}");
    assert_eq!(names[0], vec!["a", "b"]);
    assert_eq!(names[1], vec!["c", "r"]);
    assert_eq!(names[2], vec!["s", "r"]);
    assert_eq!(names[3], vec!["s", "z"]);
}

/// `grid-template` тоже сохраняет имена линий.
#[test]
fn grid_template_shorthand_stores_line_names() {
    let s = grid_div_style("div { display: grid; grid-template: [r1] 10px [r2] / [c1] 20px [c2]; }");
    assert_eq!(s.grid_template_row_line_names, vec![vec!["r1"], vec!["r2"]]);
    assert_eq!(s.grid_template_col_line_names, vec![vec!["c1"], vec!["c2"]]);
}


/// CSS Grid L1 §7.6: `grid-auto-rows: <track-size>+` — the list repeats over the implicit rows
/// (20px 40px 20px 40px …).
#[test]
fn grid_auto_rows_list_repeats_over_implicit_rows() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span><span id='b'></span><span id='c'></span><span id='d'></span><span id='e'></span>\
         </div></body>",
        "#g { display: grid; grid-template-columns: 100px; grid-auto-rows: 20px 40px; width: 100px; }",
    );
    let grid = first_element_child(&root);
    let ys: Vec<f32> = grid.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).map(|c| c.rect.y).collect();
    assert_eq!(ys.len(), 5);
    for (y, want) in ys.iter().zip([0.0, 20.0, 60.0, 80.0, 120.0]) {
        assert!((y - want).abs() < 1.0, "ys={ys:?}");
    }
}

/// The pattern starts after the explicit tracks: one explicit row, then `10px 30px` repeating.
#[test]
fn grid_auto_rows_list_starts_after_explicit_rows() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span><span id='b'></span><span id='c'></span><span id='d'></span>\
         </div></body>",
        "#g { display: grid; grid-template-columns: 100px; grid-template-rows: 50px; grid-auto-rows: 10px 30px; width: 100px; }",
    );
    let grid = first_element_child(&root);
    let ys: Vec<f32> = grid.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).map(|c| c.rect.y).collect();
    for (y, want) in ys.iter().zip([0.0, 50.0, 60.0, 90.0]) {
        assert!((y - want).abs() < 1.0, "ys={ys:?}");
    }
}

/// `grid-auto-columns: 30px 60px` with `grid-auto-flow: column` repeats over the implicit columns.
#[test]
fn grid_auto_columns_list_repeats_over_implicit_columns() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span><span id='b'></span><span id='c'></span><span id='d'></span>\
         </div></body>",
        "#g { display: grid; grid-template-rows: 20px; grid-auto-flow: column; grid-auto-columns: 30px 60px; width: 300px; }",
    );
    let grid = first_element_child(&root);
    let xs: Vec<f32> = grid.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).map(|c| c.rect.x).collect();
    let base = xs[0];
    for (x, want) in xs.iter().zip([0.0, 30.0, 90.0, 120.0]) {
        assert!((x - base - want).abs() < 1.0, "xs={xs:?}");
    }
}

/// An invalid token (`repeat()`, a line name, `subgrid`) drops the whole declaration.
#[test]
fn grid_auto_list_rejects_non_track_sizes() {
    let s = grid_div_style("div { display: grid; grid-auto-rows: 20px repeat(2, 10px); }");
    assert!(s.grid_auto_rows.is_empty());
    let s = grid_div_style("div { display: grid; grid-auto-rows: [a] 20px; }");
    assert!(s.grid_auto_rows.is_empty());
    let s = grid_div_style("div { display: grid; grid-auto-rows: 20px 30px; }");
    assert_eq!(s.grid_auto_rows.len(), 2);
}

/// `grid: auto-flow <track-size>* / …` — the shorthand takes the whole `<track-size>+` list too.
#[test]
fn grid_shorthand_auto_flow_takes_track_size_list() {
    let s = grid_div_style("div { display: grid; grid: auto-flow 10px 20px / 100px; }");
    assert_eq!(s.grid_auto_rows.len(), 2);
}
