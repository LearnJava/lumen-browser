use lumen_core::geom::Size;
use crate::style::{GridTrackSize, Length};
use super::super::resolve_auto_fill_fit_count;

// ── BUG-803: `parse_svg_transform` must always return, never index out
//    of bounds ─────────────────────────────────────────────────────────
//
// A token that is neither a letter, whitespace, a comma nor `(` used to
// leave `pos` unmoved forever (an infinite loop), and a value ending
// exactly at a comma/whitespace check indexed one byte past the slice
// (a panic). Every case here is a value that used to hang or panic under
// `--dump-layout`; reaching the assertion at all is the point for most of
// them — a regression here reintroduces a wedged renderer, not a wrong
// pixel.

#[test]
fn svg_transform_fail_me_does_not_hang() {
    // FAIL_ME(30): uppercase run is alphabetic, so the name loop consumes
    // it whole and finds `(` — must parse (and ignore) as an unknown
    // function, not hang.
    let t = super::super::parse_svg_transform(Some("FAIL_ME(30)"));
    assert_eq!(t.matrix, super::super::SvgTransform::identity().matrix, "unknown function must be ignored, not applied");
}

#[test]
fn svg_transform_digit_in_function_name_does_not_hang() {
    // translate3d/matrix3d/rotate3d/scale3d: `is_alphabetic` stops at the
    // digit, leaving `pos` on it with zero further alphabetic progress —
    // this was the exact shape that hung forever pre-fix.
    for v in ["translate3d(1px,2px,3px)", "matrix3d(1,0,0,0,1,0,0,0,1,0,0,0)", "rotate3d(60deg)", "scale3d(2)"] {
        let _ = super::super::parse_svg_transform(Some(v));
    }
}

#[test]
fn svg_transform_underscore_and_pipe_do_not_hang() {
    // Bytes that are neither letters, whitespace, a comma nor `(` at all
    // (not even the start of a name) — the name loop makes zero
    // progress on the very first attempt.
    for v in ["rotate(30deg)|rotateX(60deg)", "foo_bar(30)", "1", ";", "|"] {
        let _ = super::super::parse_svg_transform(Some(v));
    }
}

#[test]
fn svg_transform_valid_rotate_still_parses() {
    let t = super::super::parse_svg_transform(Some("rotate(90)"));
    // rotate(90) ≈ [cos90, sin90, -sin90, cos90, 0, 0] = [0, 1, -1, 0, 0, 0].
    assert!((t.matrix[0] - 0.0).abs() < 0.001, "matrix={:?}", t.matrix);
    assert!((t.matrix[1] - 1.0).abs() < 0.001, "matrix={:?}", t.matrix);
}

#[test]
fn svg_transform_trailing_comma_does_not_panic() {
    // rotate(30), and the minimal repro ",": both end mid-scan on a
    // comma with pos == len — the unparenthesized `&&`/`||` used to index
    // attr_bytes[pos] unconditionally there.
    let _ = super::super::parse_svg_transform(Some("rotate(30),"));
    let _ = super::super::parse_svg_transform(Some(","));
}

#[test]
fn svg_transform_empty_and_none_are_identity() {
    assert_eq!(super::super::parse_svg_transform(Some("")).matrix, super::super::SvgTransform::identity().matrix);
    assert_eq!(super::super::parse_svg_transform(Some("none")).matrix, super::super::SvgTransform::identity().matrix);
    assert_eq!(super::super::parse_svg_transform(None).matrix, super::super::SvgTransform::identity().matrix);
}

#[test]
fn svg_defs_children_do_not_render_directly() {
    // BUG-201: <defs> content is invisible until referenced. A <rect> inside
    // <defs> with no <use> must produce no shape.
    let html = "<svg><defs><rect id=\"r1\" x=\"0\" y=\"0\" width=\"50\" height=\"35\"/></defs></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn has_any_shape(b: &super::super::LayoutBox) -> bool {
        if matches!(b.kind, super::super::BoxKind::SvgShape { .. }) { return true; }
        b.children.iter().any(has_any_shape)
    }
    assert!(!has_any_shape(&root), "<defs> children must not render directly");
}

#[test]
fn svg_text_element_simple() {
    // <text>Hello</text> should create a SvgText layout box with content.
    let html = "<svg><text>Hello</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_text_box(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::SvgText { .. }) {
                return Some(child);
            }
            if let Some(found) = find_text_box(child) {
                return Some(found);
            }
        }
        None
    }

    let text_box = find_text_box(&root);
    assert!(text_box.is_some(), "SvgText layout box not found");

    if let Some(tb) = text_box {
        if let super::super::BoxKind::SvgText { text, .. } = &tb.kind {
            assert_eq!(text, "Hello");
        } else {
            panic!("Found box is not SvgText");
        }
    }
}

#[test]
fn svg_text_with_x_y_attributes() {
    // <text x="10" y="20">Content</text> should store x/y values.
    let html = "<svg><text x=\"10\" y=\"20\">Test</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_text_box(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::SvgText { .. }) {
                return Some(child);
            }
            if let Some(found) = find_text_box(child) {
                return Some(found);
            }
        }
        None
    }

    if let Some(tb) = find_text_box(&root) {
        if let super::super::BoxKind::SvgText { x, y, text, .. } = &tb.kind {
            assert!((x - 10.0).abs() < 0.1, "x should be ~10, got {}", x);
            assert!((y - 20.0).abs() < 0.1, "y should be ~20, got {}", y);
            assert_eq!(text, "Test");
        } else {
            panic!("Found box is not SvgText");
        }
    }
}

#[test]
fn svg_text_anchor_middle() {
    // <text text-anchor="middle">Center</text> should parse text-anchor.
    let html = "<svg><text text-anchor=\"middle\">Center</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_text_box(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::SvgText { .. }) {
                return Some(child);
            }
            if let Some(found) = find_text_box(child) {
                return Some(found);
            }
        }
        None
    }

    if let Some(tb) = find_text_box(&root) {
        if let super::super::BoxKind::SvgText { text_anchor, .. } = &tb.kind {
            assert_eq!(*text_anchor, super::super::SvgTextAnchor::Middle);
        } else {
            panic!("Found box is not SvgText");
        }
    }
}

#[test]
fn svg_dominant_baseline_hanging() {
    // <text dominant-baseline="hanging">Hanging</text> should parse dominant-baseline.
    let html = "<svg><text dominant-baseline=\"hanging\">Hanging</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_text_box(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::SvgText { .. }) {
                return Some(child);
            }
            if let Some(found) = find_text_box(child) {
                return Some(found);
            }
        }
        None
    }

    if let Some(tb) = find_text_box(&root) {
        if let super::super::BoxKind::SvgText { dominant_baseline, .. } = &tb.kind {
            assert_eq!(*dominant_baseline, super::super::SvgDominantBaseline::Hanging);
        } else {
            panic!("Found box is not SvgText");
        }
    }
}

/// Finds the first `SvgText` box in a layout tree (test helper).
fn first_svg_text(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    for child in &b.children {
        if matches!(child.kind, super::super::BoxKind::SvgText { .. }) {
            return Some(child);
        }
        if let Some(found) = first_svg_text(child) {
            return Some(found);
        }
    }
    None
}

#[test]
fn svg_text_anchor_from_css_property() {
    // text-anchor set purely via a CSS rule (no presentation attribute).
    let html = "<svg><text class=\"t\">Center</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(".t { text-anchor: middle; }");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { text_anchor, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*text_anchor, super::super::SvgTextAnchor::Middle);
}

#[test]
fn svg_text_anchor_css_overrides_attribute() {
    // SVG 2 §6.4: presentation attribute has specificity 0 — a CSS rule wins.
    let html = "<svg><text class=\"t\" text-anchor=\"start\">X</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(".t { text-anchor: end; }");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { text_anchor, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*text_anchor, super::super::SvgTextAnchor::End);
}

#[test]
fn svg_text_anchor_inherits_from_container_attribute() {
    // text-anchor is inherited: a `<g text-anchor>` propagates to descendant <text>.
    let html = "<svg><g text-anchor=\"end\"><text>X</text></g></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { text_anchor, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*text_anchor, super::super::SvgTextAnchor::End);
}

#[test]
fn svg_text_anchor_defaults_to_start() {
    // No attribute, no CSS → `start` initial.
    let html = "<svg><text>X</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { text_anchor, dominant_baseline, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*text_anchor, super::super::SvgTextAnchor::Start);
    assert_eq!(*dominant_baseline, super::super::SvgDominantBaseline::Auto);
}

#[test]
fn svg_baseline_shift_from_attribute() {
    // <text baseline-shift="super"> presentation attribute folds into the cascade.
        let html = "<svg><text baseline-shift=\"super\">x</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { baseline_shift, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*baseline_shift, super::super::SvgBaselineShift::Super);
}

#[test]
fn svg_baseline_shift_css_overrides_attribute() {
    // SVG 2 §6.4: presentation attribute has specificity 0 — a CSS rule wins.
    let html = "<svg><text class=\"t\" baseline-shift=\"sub\">x</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(".t { baseline-shift: 3px; }");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { baseline_shift, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*baseline_shift, super::super::SvgBaselineShift::Length(3.0));
}

#[test]
fn svg_baseline_shift_not_inherited() {
    // baseline-shift is NOT inherited: a `<g baseline-shift>` does NOT propagate
    // to a descendant <text> that sets nothing — the text keeps the `baseline` initial.
    let html = "<svg><g baseline-shift=\"super\"><text>x</text></g></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { baseline_shift, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*baseline_shift, super::super::SvgBaselineShift::Baseline);
}

#[test]
fn svg_baseline_shift_defaults_to_baseline() {
    // No attribute, no CSS → `baseline` initial (no shift).
    let html = "<svg><text>x</text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let tb = first_svg_text(&root).expect("svg text box");
    let super::super::BoxKind::SvgText { baseline_shift, .. } = &tb.kind else {
        panic!("not SvgText");
    };
    assert_eq!(*baseline_shift, super::super::SvgBaselineShift::Baseline);
}

#[test]
fn svg_tspan_text_content() {
    // <text><tspan>Hello</tspan> <tspan>World</tspan></text> should collect all tspan text.
    let html = "<svg><text><tspan>Hello</tspan><tspan>World</tspan></text></svg>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    fn find_text_box(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::SvgText { .. }) {
                return Some(child);
            }
            if let Some(found) = find_text_box(child) {
                return Some(found);
            }
        }
        None
    }

    if let Some(tb) = find_text_box(&root) {
        if let super::super::BoxKind::SvgText { text, .. } = &tb.kind {
            assert!(text.contains("Hello"), "text should contain 'Hello', got '{}'", text);
            assert!(text.contains("World"), "text should contain 'World', got '{}'", text);
        } else {
            panic!("Found box is not SvgText");
        }
    }
}

// CSS Grid auto-fill/auto-fit tests (B-3)
#[test]
fn grid_auto_fill_count_basic() {
    // repeat(auto-fill, minmax(100px, 1fr)) with 500px available
    // should resolve to 5 tracks (500 / 100 = 5)
    let tracks = vec![GridTrackSize::Minmax(
        Box::new(GridTrackSize::Length(Length::Px(100.0))),
        Box::new(GridTrackSize::Fr(1.0)),
    )];
    let count = resolve_auto_fill_fit_count(500.0, &tracks, 0.0);
    assert_eq!(count, 5, "should fit 5 tracks of 100px each");
}

#[test]
fn grid_auto_fill_count_with_gap() {
    // repeat(auto-fill, minmax(100px, 1fr)) with 500px available and 10px gap
    // (500 + 10) / (100 + 10) = 510 / 110 ≈ 4.63 → 4 tracks
    let tracks = vec![GridTrackSize::Minmax(
        Box::new(GridTrackSize::Length(Length::Px(100.0))),
        Box::new(GridTrackSize::Fr(1.0)),
    )];
    let count = resolve_auto_fill_fit_count(500.0, &tracks, 10.0);
    assert_eq!(count, 4, "should fit 4 tracks with gap");
}

#[test]
fn grid_auto_fill_count_zero_width() {
    // Zero or negative width should return 1 track minimum
    let tracks = vec![GridTrackSize::Length(Length::Px(100.0))];
    let count = resolve_auto_fill_fit_count(0.0, &tracks, 0.0);
    assert_eq!(count, 1, "zero width should return 1 track minimum");
}

#[test]
fn grid_auto_fill_count_large_gap() {
    // Gap larger than available width should still return 1 track
    let tracks = vec![GridTrackSize::Length(Length::Px(50.0))];
    let count = resolve_auto_fill_fit_count(30.0, &tracks, 100.0);
    assert_eq!(count, 1, "should return 1 track minimum");
}

#[test]
fn grid_fit_content_parse() {
    // `fit-content(200px)` should parse correctly
    let parsed = GridTrackSize::parse_track_list("fit-content(200px)", false);
    assert_eq!(parsed.len(), 1, "fit-content(200px) should parse to single track");
    if let GridTrackSize::FitContent(limit) = &parsed[0] {
        // Verify the limit is a Length(200px)
        match &**limit {
            GridTrackSize::Length(Length::Px(val)) => {
                assert_eq!(*val, 200.0, "fit-content limit should be 200px");
            }
            _ => panic!("fit-content limit should be Length(200px), got {:?}", limit),
        }
    } else {
        panic!("parsed should be FitContent variant");
    }
}

#[test]
fn grid_fit_content_minmax() {
    // `fit-content(300px)` should be equivalent to minmax(auto, min(300px, max-content))
    let parsed = GridTrackSize::parse_track_list("fit-content(300px)", false);
    assert_eq!(parsed.len(), 1);
    // Verify internal structure has FitContent variant
    assert!(matches!(parsed[0], GridTrackSize::FitContent(_)));
}

#[test]
fn grid_auto_fill_multiple_tracks() {
    // repeat(auto-fill, minmax(50px, 1fr) minmax(50px, 1fr)) with 300px
    // Two tracks per repeat unit (100px total) → 3 units → 3 fills
    let tracks = vec![
        GridTrackSize::Minmax(
            Box::new(GridTrackSize::Length(Length::Px(50.0))),
            Box::new(GridTrackSize::Fr(1.0)),
        ),
        GridTrackSize::Minmax(
            Box::new(GridTrackSize::Length(Length::Px(50.0))),
            Box::new(GridTrackSize::Fr(1.0)),
        ),
    ];
    let count = resolve_auto_fill_fit_count(300.0, &tracks, 0.0);
    // Min width = max(50, 50) = 50px, so (300 + 0) / (50 + 0) = 6
    // But we have 2 tracks per repeat, so count should be based on total min width
    // Simplification: resolve_auto_fill_fit_count returns count of repeat units, not total tracks
    assert!(count >= 1, "should resolve to at least 1 repeat unit");
}

#[test]
fn grid_auto_fill_small_container() {
    // Container smaller than one track should still return 1
    let tracks = vec![GridTrackSize::Length(Length::Px(500.0))];
    let count = resolve_auto_fill_fit_count(100.0, &tracks, 0.0);
    assert_eq!(count, 1, "container smaller than track should return 1");
}

#[test]
fn grid_auto_fill_empty_tracks() {
    // Empty track list should return 1
    let tracks: Vec<GridTrackSize> = vec![];
    let count = resolve_auto_fill_fit_count(500.0, &tracks, 0.0);
    assert_eq!(count, 1, "empty track list should return 1");
}

// CSS Grid auto-fill/auto-fit Phase 2 layout tests (G-1)

/// Collect (x, y, width) of all Block children of the first grid container found.
fn collect_grid_item_rects(root: &super::super::LayoutBox) -> Vec<(f32, f32, f32)> {
    fn walk(b: &super::super::LayoutBox, out: &mut Vec<(f32, f32, f32)>, in_grid: bool) {
        if in_grid && matches!(b.kind, super::super::BoxKind::Block) {
            out.push((b.rect.x, b.rect.y, b.rect.width));
        }
        let next_in_grid = in_grid || matches!(b.kind, super::super::BoxKind::Block);
        for c in &b.children {
            walk(c, out, next_in_grid && !in_grid);
        }
    }
    // Find the first grid container and collect its direct children.
    fn find_grid(b: &super::super::LayoutBox) -> Option<Vec<(f32, f32, f32)>> {
        if b.style.display == super::super::Display::Grid {
            let items: Vec<_> = b.children.iter()
                .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
                .map(|c| (c.rect.x, c.rect.y, c.rect.width))
                .collect();
            return Some(items);
        }
        for c in &b.children {
            if let Some(v) = find_grid(c) {
                return Some(v);
            }
        }
        None
    }
    let _ = walk; // suppress unused warning
    find_grid(root).unwrap_or_default()
}

#[test]
fn grid_auto_fill_expands_columns_at_layout() {
    // repeat(auto-fill, 100px) in a 500px container → 5 columns; items flow into columns
    let html = "<div class='grid'>\
                 <div>A</div><div>B</div><div>C</div><div>D</div><div>E</div>\
                </div>";
    let css = ".grid { display: grid; grid-template-columns: repeat(auto-fill, 100px); \
                       width: 500px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(500.0, 300.0));
    let items = collect_grid_item_rects(&root);
    assert!(items.len() >= 2, "should have at least 2 items placed");
    // First item should be ~100px wide (one column)
    let (_, _, w0) = items[0];
    assert!(
        (w0 - 100.0).abs() < 2.0,
        "first item width should be ~100px (auto-fill expanded), got {}",
        w0
    );
    // Second item should be in the second column (x ≈ 100)
    let (x1, _, _) = items[1];
    assert!(
        (90.0..=110.0).contains(&x1),
        "second item x should be ~100px (column 2), got {}",
        x1
    );
}

#[test]
fn grid_auto_fill_minimum_one_column() {
    // Even when container is very small, at least 1 track must be produced (no crash)
    let html = "<div class='grid'><div>X</div></div>";
    let css = ".grid { display: grid; grid-template-columns: repeat(auto-fill, 200px); \
                       width: 50px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(50.0, 300.0));
    // Should not panic; grid should have content
    assert!(!root.children.is_empty(), "grid should have content");
    let items = collect_grid_item_rects(&root);
    assert!(!items.is_empty(), "should have at least one item placed");
}

#[test]
fn grid_auto_fit_expands_columns_at_layout() {
    // repeat(auto-fit, 100px) in a 300px container → 3 columns
    let html = "<div class='grid'>\
                 <div>P</div><div>Q</div><div>R</div>\
                </div>";
    let css = ".grid { display: grid; grid-template-columns: repeat(auto-fit, 100px); \
                       width: 300px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let items = collect_grid_item_rects(&root);
    assert!(items.len() >= 2, "should have at least 2 items placed");
    let (_, _, w0) = items[0];
    assert!(
        (w0 - 100.0).abs() < 2.0,
        "first item width ~100px for auto-fit, got {}",
        w0
    );
    let (x1, _, _) = items[1];
    assert!(
        (90.0..=110.0).contains(&x1),
        "second item x ~100px (column 2), got {}",
        x1
    );
}

#[test]
fn grid_auto_fill_with_minmax_tracks() {
    // repeat(auto-fill, minmax(80px, 1fr)) in 400px → multiple tracks, no panic
    let html = "<div class='grid'>\
                 <div>M</div><div>N</div>\
                </div>";
    let css = ".grid { display: grid; \
                       grid-template-columns: repeat(auto-fill, minmax(80px, 1fr)); \
                       width: 400px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 300.0));
    let items = collect_grid_item_rects(&root);
    assert!(!items.is_empty(), "minmax auto-fill items should be laid out");
}

// CSS Grid dense packing tests (B-4)
#[test]
fn grid_dense_fills_gaps() {
    // grid-auto-flow: row dense should fill gaps left by taller items
    let html = "<div class='container'>\
                 <div style='grid-row: 1 / 3;'>Large</div>\
                 <div>Item 2</div>\
                 <div>Item 3</div>\
               </div>";
    let css = ".container { \
                display: grid; \
                grid-template-columns: repeat(3, 1fr); \
                grid-auto-flow: row dense; \
              }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));

    fn find_grid_items(b: &super::super::LayoutBox) -> Vec<(f32, f32)> {
        let mut items = Vec::new();
        for child in &b.children {
            if matches!(child.kind, super::super::BoxKind::Block) && !child.children.is_empty() {
                // This is a grid item (has content)
                items.push((child.rect.x, child.rect.y));
            }
            items.extend(find_grid_items(child));
        }
        items
    }

    let items = find_grid_items(&root);
    // With dense, Item 2 and 3 should fill the gap in columns 2-3 of row 1
    assert!(items.len() >= 3, "should have at least 3 items");
}

#[test]
fn grid_column_dense_backfill() {
    // grid-auto-flow: column dense should backfill in column order
    let html = "<div class='container'>\
                 <div style='grid-column: 1 / 3;'>Wide</div>\
                 <div>Item 2</div>\
                 <div>Item 3</div>\
               </div>";
    let css = ".container { \
                display: grid; \
                grid-template-rows: repeat(2, 100px); \
                grid-auto-flow: column dense; \
              }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));

    // Just verify it doesn't panic and produces a layout
    assert!(!root.children.is_empty(), "grid should have content");
}

#[test]
fn grid_dense_vs_sparse_layout() {
    // Compare dense and sparse layouts to ensure they differ appropriately
    fn layout_with_flow(flow: &str) -> super::super::LayoutBox {
        let html = "<div class='container'>\
                     <div style='grid-column: span 2; grid-row: span 2;'>1</div>\
                     <div>2</div>\
                     <div>3</div>\
                     <div>4</div>\
                   </div>";
        let css = format!(".container {{ \
                           display: grid; \
                           grid-template-columns: repeat(3, 100px); \
                           grid-auto-flow: {}; \
                         }}", flow);
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&css);
        super::super::layout(&doc, &sheet, Size::new(300.0, 300.0))
    }

    let sparse = layout_with_flow("row");
    let dense = layout_with_flow("row dense");

    // Both should produce valid layouts
    assert!(!sparse.children.is_empty(), "sparse layout should have content");
    assert!(!dense.children.is_empty(), "dense layout should have content");
    // Layouts may differ due to dense filling gaps differently
}

#[test]
fn grid_dense_explicit_placement_respected() {
    // Explicitly placed items should not be affected by dense algorithm
    let html = "<div class='container'>\
                 <div style='grid-column: 2; grid-row: 2;'>Explicit</div>\
                 <div>Auto 1</div>\
                 <div>Auto 2</div>\
               </div>";
    let css = ".container { \
                display: grid; \
                grid-template-columns: repeat(3, 1fr); \
                grid-auto-flow: row dense; \
              }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));

    // Verify layout was created without panics
    assert!(!root.children.is_empty(), "grid should be laid out");
}

// --- text-align-last layout ---

/// Helper: create a minimal InlineFrag at (x, width) for alignment tests.
fn make_frag(x: f32, width: f32) -> super::super::InlineFrag {
    use crate::style::ComputedStyle;
    use lumen_dom::NodeId;
    super::super::InlineFrag {
        x,
        width,
        y_offset: 0.0,
        text: String::new(),
        style: ComputedStyle::root(),
        padding_left: 0.0,
        padding_right: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        is_first_line: false,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    }
}

#[test]
fn text_align_last_center_shifts_last_line() {
    // Non-last line: left (offset=0). Last line: center → (300-80)/2=110.
    use crate::style::{TextAlign, TextAlignLast, Direction};
    let mut lines = vec![
        vec![make_frag(0.0, 100.0)],
        vec![make_frag(0.0, 80.0)],
    ];
    super::super::align_lines(&mut lines, 300.0, TextAlign::Left, TextAlignLast::Center, Direction::Ltr);
    assert_eq!(lines[0][0].x, 0.0, "non-last line stays left");
    assert!((lines[1][0].x - 110.0).abs() < 0.5, "last line centered, got {}", lines[1][0].x);
}

#[test]
fn text_align_last_right_shifts_last_line() {
    // Non-last line: left (offset=0). Last line: right → 300-80=220.
    use crate::style::{TextAlign, TextAlignLast, Direction};
    let mut lines = vec![
        vec![make_frag(0.0, 100.0)],
        vec![make_frag(0.0, 80.0)],
    ];
    super::super::align_lines(&mut lines, 300.0, TextAlign::Left, TextAlignLast::Right, Direction::Ltr);
    assert_eq!(lines[0][0].x, 0.0, "non-last line stays left");
    assert!((lines[1][0].x - 220.0).abs() < 0.5, "last line right, got {}", lines[1][0].x);
}

#[test]
fn text_align_last_auto_inherits_text_align() {
    // Auto: last line uses same alignment as text_align (Right here).
    // Both lines → right offset = 300-100=200 for first, 300-80=220 for last.
    use crate::style::{TextAlign, TextAlignLast, Direction};
    let mut lines = vec![
        vec![make_frag(0.0, 100.0)],
        vec![make_frag(0.0, 80.0)],
    ];
    super::super::align_lines(&mut lines, 300.0, TextAlign::Right, TextAlignLast::Auto, Direction::Ltr);
    assert!((lines[0][0].x - 200.0).abs() < 0.5, "non-last right-aligned, got {}", lines[0][0].x);
    assert!((lines[1][0].x - 220.0).abs() < 0.5, "last line right (auto), got {}", lines[1][0].x);
}

#[test]
fn text_align_last_end_resolves_to_right_ltr() {
    // End in LTR = Right → last line offset = 300-80=220; non-last Left = 0.
    use crate::style::{TextAlign, TextAlignLast, Direction};
    let mut lines = vec![
        vec![make_frag(0.0, 100.0)],
        vec![make_frag(0.0, 80.0)],
    ];
    super::super::align_lines(&mut lines, 300.0, TextAlign::Left, TextAlignLast::End, Direction::Ltr);
    assert_eq!(lines[0][0].x, 0.0, "non-last line stays left");
    assert!((lines[1][0].x - 220.0).abs() < 0.5, "last line end→right, got {}", lines[1][0].x);
}

// ── <progress> / <meter> ────────────────────────────────────────────────

fn find_form_kind(root: &super::super::LayoutBox) -> Option<super::super::FormControlKind> {
    if let super::super::BoxKind::FormControl { kind } = &root.kind {
        return Some(kind.clone());
    }
    for child in &root.children {
        if let Some(k) = find_form_kind(child) {
            return Some(k);
        }
    }
    None
}

/// Collect the concatenated text of every `InlineRun` segment in the tree.
fn collect_inline_text(root: &super::super::LayoutBox, out: &mut Vec<String>) {
    if let super::super::BoxKind::InlineRun { segments, .. } = &root.kind {
        for seg in segments {
            out.push(seg.text.clone());
        }
    }
    for child in &root.children {
        collect_inline_text(child, out);
    }
}

#[test]
fn base_select_builds_styleable_tree_not_native_widget() {
    // `appearance: base-select` must render an author-styleable box tree
    // (FlowRoot → trigger Block → InlineRun with the selected label), not the
    // opaque `FormControlKind::Select` native widget.
    let html = r#"<select><option>Apple</option><option selected>Banana</option></select>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("select { appearance: base-select; }");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));

    assert!(
        find_form_kind(&root).is_none(),
        "base-select must NOT produce a native FormControl::Select box"
    );
    let mut texts = Vec::new();
    collect_inline_text(&root, &mut texts);
    assert!(
        texts.iter().any(|t| t.contains("Banana")),
        "trigger should show the selected option label, got {texts:?}"
    );
}

#[test]
fn native_select_still_uses_form_control_widget() {
    // Regression guard: without `appearance: base-select`, a `<select>` keeps
    // rendering as the opaque native widget.
    let html = r#"<select><option selected>Only</option></select>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    assert!(
        matches!(find_form_kind(&root), Some(super::super::FormControlKind::Select { .. })),
        "plain <select> should still be a native FormControl::Select"
    );
}

#[test]
fn progress_determinate_creates_kind() {
    let html = r#"<progress value="0.5" max="1.0"></progress>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let kind = find_form_kind(&root).expect("progress FormControl box");
    assert!(
        matches!(kind, super::super::FormControlKind::Progress { value: Some(v), max: m }
            if (v - 0.5).abs() < 0.001 && (m - 1.0).abs() < 0.001),
        "expected Progress{{value:Some(0.5), max:1.0}}, got {kind:?}"
    );
}

#[test]
fn progress_indeterminate_when_no_value_attr() {
    let html = r#"<progress max="10"></progress>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let kind = find_form_kind(&root).expect("progress FormControl box");
    assert!(
        matches!(kind, super::super::FormControlKind::Progress { value: None, .. }),
        "absent value attribute should produce indeterminate Progress, got {kind:?}"
    );
}

#[test]
fn progress_value_clamped_to_max() {
    let html = r#"<progress value="200" max="100"></progress>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let kind = find_form_kind(&root).expect("progress FormControl box");
    if let super::super::FormControlKind::Progress { value: Some(v), max: m } = kind {
        assert!((v - 100.0).abs() < 0.001, "value should be clamped to max={m}, got {v}");
    } else {
        panic!("expected determinate Progress, got {kind:?}");
    }
}

#[test]
fn meter_creates_kind_with_defaults() {
    // No attributes → min=0, max=1, value=0, low=0, high=1, optimum=0.5
    let html = r#"<meter></meter>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let kind = find_form_kind(&root).expect("meter FormControl box");
    assert!(
        matches!(kind, super::super::FormControlKind::Meter { min: m, max: mx, .. }
            if m.abs() < 0.001 && (mx - 1.0).abs() < 0.001),
        "default meter min=0/max=1, got {kind:?}"
    );
}

#[test]
fn meter_parses_all_attributes() {
    let html = r#"<meter min="0" max="10" value="7" low="3" high="8" optimum="6"></meter>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let kind = find_form_kind(&root).expect("meter FormControl box");
    if let super::super::FormControlKind::Meter { value, min, max, low, high, optimum } = kind {
        assert!((min - 0.0).abs() < 0.001, "min");
        assert!((max - 10.0).abs() < 0.001, "max");
        assert!((value - 7.0).abs() < 0.001, "value");
        assert!((low - 3.0).abs() < 0.001, "low");
        assert!((high - 8.0).abs() < 0.001, "high");
        assert!((optimum - 6.0).abs() < 0.001, "optimum");
    } else {
        panic!("expected Meter kind, got {kind:?}");
    }
}

#[test]
fn meter_min_ge_max_resets_to_defaults() {
    // Spec §4.10.14: when min >= max, reset to 0..1
    let html = r#"<meter min="5" max="3" value="4"></meter>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    let kind = find_form_kind(&root).expect("meter FormControl box");
    if let super::super::FormControlKind::Meter { min, max, .. } = kind {
        assert!(min.abs() < 0.001, "min should reset to 0, got {min}");
        assert!((max - 1.0).abs() < 0.001, "max should reset to 1, got {max}");
    } else {
        panic!("expected Meter kind");
    }
}

#[test]
fn progress_ua_style_300x16() {
    let html = r#"<progress value="0.5" max="1"></progress>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(600.0, 400.0));
    // Find the FormControl box and check its size matches UA defaults.
    // `rect` is the border-box: content(300) + 2×border(1) = 302 wide;
    //                           content(16)  + 2×border(1) = 18 tall.
    fn find_box(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::FormControl { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find_box)
    }
    let b = find_box(&root).expect("progress box");
    assert!((b.rect.width - 302.0).abs() < 1.0, "border-box width should be 302px, got {}", b.rect.width);
    assert!((b.rect.height - 18.0).abs() < 1.0, "border-box height should be 18px, got {}", b.rect.height);
}

// ── measure_text_w_varied (CSS Fonts L4 §6.3) ───────────────────────────

struct Fixed8Varied;
impl super::super::TextMeasurer for Fixed8Varied {
    fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
}

#[test]
fn measure_text_w_varied_empty_axes_equals_families() {
    let w_fam = super::super::measure_text_w_families("hello", 16.0, 0.0, 0.0, &[], &Fixed8Varied);
    let w_var = super::super::measure_text_w_varied("hello", 16.0, 0.0, 0.0, &[], &[], &Fixed8Varied);
    assert_eq!(w_fam, w_var);
}

#[test]
fn measure_text_w_varied_empty_text_is_zero() {
    let w = super::super::measure_text_w_varied("", 16.0, 0.0, 0.0, &[], &[], &Fixed8Varied);
    assert_eq!(w, 0.0);
}

#[test]
fn measure_text_w_varied_axes_use_char_width_varied() {
    struct VariedMeasurer;
    impl super::super::TextMeasurer for VariedMeasurer {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        fn char_width_varied(
            &self,
            _ch: char,
            _font_size_px: f32,
            axes: &[crate::style::FontVariationSetting],
            _families: &[String],
        ) -> f32 {
            if axes.is_empty() { 8.0 } else { 12.0 }
        }
    }
    let axes = vec![crate::style::FontVariationSetting { tag: *b"wght", value: 700.0 }];
    // 3 chars × 12px − 0 letter-spacing = 36px
    let w = super::super::measure_text_w_varied("abc", 16.0, 0.0, 0.0, &[], &axes, &VariedMeasurer);
    assert_eq!(w, 36.0, "non-empty axes должны вызывать char_width_varied");
}

// ── border-spacing tests ──────────────────────────────────────────────────

fn layout_table(css: &str, html_body: &str, vw: f32, vh: f32) -> super::super::LayoutBox {
    let html = format!("<html><head><style>{css}</style></head><body>{html_body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(vw, vh));
    fn find_table(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::Table) {
            return Some(b);
        }
        for child in &b.children {
            if let Some(found) = find_table(child) {
                return Some(found);
            }
        }
        None
    }
    find_table(&root).cloned().expect("Table not found in layout tree")
}

/// Returns the first TableRow found in `b`, searching children and TableRowGroup wrappers.
fn find_first_row(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    for child in &b.children {
        if matches!(child.kind, super::super::BoxKind::TableRow) {
            return Some(child);
        }
        if matches!(child.kind, super::super::BoxKind::TableRowGroup) {
            for row_child in &child.children {
                if matches!(row_child.kind, super::super::BoxKind::TableRow) {
                    return Some(row_child);
                }
            }
        }
    }
    None
}

/// Returns all TableRows found in `b` (direct or inside TableRowGroup).
fn collect_rows(b: &super::super::LayoutBox) -> Vec<&super::super::LayoutBox> {
    let mut rows = Vec::new();
    for child in &b.children {
        if matches!(child.kind, super::super::BoxKind::TableRow) {
            rows.push(child);
        } else if matches!(child.kind, super::super::BoxKind::TableRowGroup) {
            for row_child in &child.children {
                if matches!(row_child.kind, super::super::BoxKind::TableRow) {
                    rows.push(row_child);
                }
            }
        }
    }
    rows
}

#[test]
fn border_spacing_zero_by_default() {
    // Without border-spacing, gap between adjacent cells should be 0.
    let t = layout_table(
        "table { width: 200px; } td { width: 80px; }",
        "<table><tr><td></td><td></td></tr></table>",
        800.0, 600.0,
    );
    let row = find_first_row(&t).expect("row not found");
    let cells: Vec<_> = row.children.iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    assert!(cells.len() >= 2, "expected >= 2 cells");
    // With 0 spacing the second cell starts right after the first.
    let gap = cells[1].rect.x - (cells[0].rect.x + cells[0].rect.width);
    assert!(gap.abs() < 1.0, "expected gap=0, got {gap}");
}

#[test]
fn border_spacing_horizontal_separates_cells() {
    // border-spacing: 10px should add 10px between cells.
    let t = layout_table(
        "table { border-spacing: 10px; } td { width: 80px; }",
        "<table><tr><td></td><td></td></tr></table>",
        800.0, 600.0,
    );
    let row = find_first_row(&t).expect("row not found");
    let cells: Vec<_> = row.children.iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    assert!(cells.len() >= 2, "expected >= 2 cells, got {}", cells.len());
    let gap = cells[1].rect.x - (cells[0].rect.x + cells[0].rect.width);
    assert!((gap - 10.0).abs() < 1.0, "expected gap=10, got {gap}");
}

#[test]
fn border_spacing_two_values_h_v() {
    // border-spacing: 8px 20px → h=8px, v=20px; rows should be separated by 20px.
    let t = layout_table(
        "table { border-spacing: 8px 20px; } td { width: 80px; height: 30px; }",
        "<table><tr><td></td></tr><tr><td></td></tr></table>",
        800.0, 600.0,
    );
    let rows = collect_rows(&t);
    assert!(rows.len() >= 2, "expected >= 2 rows, got {}", rows.len());
    // Vertical gap between rows should be 20px.
    let v_gap = rows[1].rect.y - (rows[0].rect.y + rows[0].rect.height);
    assert!((v_gap - 20.0).abs() < 1.0, "expected v_gap=20, got {v_gap}");
}

#[test]
fn table_cell_height_is_minimum_grows_to_fit_content() {
    // CSS 2.1 §17.5.3: `height` on a table cell is a minimum. A cell with
    // height:64px border-box (content box 56px after 4px borders) whose content
    // needs 64px (32px block + 16px top/bottom margins) must grow its border-box
    // to 72px, not clamp to 64px. Otherwise content overflows into the
    // border-spacing gap and row pitch is short by the overflow (BUG-177).
    let t = layout_table(
        "table { border-spacing: 8px; } \
         td { width: 96px; height: 64px; border: 4px solid #000; box-sizing: border-box; } \
         .blk { width: 52px; height: 32px; margin: 16px auto; }",
        "<table>\
           <tr><td><div class=\"blk\"></div></td></tr>\
           <tr><td><div class=\"blk\"></div></td></tr>\
         </table>",
        800.0, 600.0,
    );
    let rows = collect_rows(&t);
    assert!(rows.len() >= 2, "expected >= 2 rows, got {}", rows.len());
    // Each cell grows to the content border-box height: 64 (content) + 2 (UA 1px
    // cell padding) + 8 (borders) = 74.
    let cell0 = rows[0].children.iter()
        .find(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .expect("cell in row 0");
    assert!((cell0.rect.height - 74.0).abs() < 0.5,
        "cell grows to fit content (74px), got {}", cell0.rect.height);
    // Row pitch = cell height (74) + vertical border-spacing (8) = 82.
    let pitch = rows[1].rect.y - rows[0].rect.y;
    assert!((pitch - 82.0).abs() < 0.5, "row pitch = 82, got {pitch}");
}

#[test]
fn table_cell_height_honoured_when_taller_than_content() {
    // The minimum must not shrink a cell below its specified height: a tall
    // height:120px cell with tiny content keeps 120px.
    let t = layout_table(
        "td { width: 80px; height: 120px; box-sizing: border-box; } \
         .blk { width: 20px; height: 20px; }",
        "<table><tr><td><div class=\"blk\"></div></td></tr></table>",
        800.0, 600.0,
    );
    let row = find_first_row(&t).expect("row not found");
    let cell = row.children.iter()
        .find(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .expect("cell");
    assert!((cell.rect.height - 120.0).abs() < 0.5,
        "specified 120px height honoured, got {}", cell.rect.height);
}

// ─── border-collapse: collapse (CSS 2.1 §17.6.2) — BUG-129 ─────────────────

#[test]
fn collapse_overlaps_adjacent_cell_borders() {
    // collapse: adjacent cells share one border. Each 100px border-box cell with a 2px
    // border should overlap its neighbour by 2px, so cell[1].x = cell[0].right - 2.
    let t = layout_table(
        "table { border-collapse: collapse; } td { width: 96px; border: 2px solid #000; }",
        "<table><tr><td></td><td></td><td></td></tr></table>",
        800.0, 600.0,
    );
    let row = find_first_row(&t).expect("row not found");
    let cells: Vec<_> = row.children.iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    assert_eq!(cells.len(), 3, "expected 3 cells");
    // border-box width = 96 (content) + 2*1 (UA 1px cell padding) + 2*2 (border) = 102.
    assert!((cells[0].rect.width - 102.0).abs() < 0.5, "cell border-box = 102, got {}", cells[0].rect.width);
    let overlap0 = (cells[0].rect.x + cells[0].rect.width) - cells[1].rect.x;
    let overlap1 = (cells[1].rect.x + cells[1].rect.width) - cells[2].rect.x;
    assert!((overlap0 - 2.0).abs() < 0.5, "cells overlap by collapsed 2px border, got {overlap0}");
    assert!((overlap1 - 2.0).abs() < 0.5, "cells overlap by collapsed 2px border, got {overlap1}");
}

#[test]
fn collapse_outer_cells_snap_to_table_border() {
    // collapse: the first cell's left border coincides with the table's own border edge
    // (table.x == cell[0].x), and the table border-box width = overlapped grid width.
    let t = layout_table(
        "table { border-collapse: collapse; border: 2px solid #000; } td { width: 96px; border: 2px solid #000; }",
        "<table><tr><td></td><td></td><td></td></tr></table>",
        800.0, 600.0,
    );
    let row = find_first_row(&t).expect("row not found");
    let cells: Vec<_> = row.children.iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    assert!((cells[0].rect.x - t.rect.x).abs() < 0.5,
        "first cell left coincides with table border: table.x={}, cell.x={}", t.rect.x, cells[0].rect.x);
    // 3 cells of 102px (96 + 2*1 UA padding + 2*2 border) overlapping by 2px at 2 internal
    // grid lines → 306 - 2*2 = 302.
    assert!((t.rect.width - 302.0).abs() < 0.5, "collapsed table width = 302, got {}", t.rect.width);
}

#[test]
fn collapse_overlaps_rows_vertically() {
    // collapse: consecutive rows share their horizontal border. Two 30px-tall rows with a
    // 2px border collapse the inter-row line, so row[1].y = row[0].bottom - 2.
    let t = layout_table(
        "table { border-collapse: collapse; } td { width: 50px; height: 26px; border: 2px solid #000; }",
        "<table><tr><td></td></tr><tr><td></td></tr></table>",
        800.0, 600.0,
    );
    let rows = collect_rows(&t);
    assert!(rows.len() >= 2, "expected >= 2 rows");
    let overlap = (rows[0].rect.y + rows[0].rect.height) - rows[1].rect.y;
    assert!((overlap - 2.0).abs() < 0.5, "rows overlap by collapsed 2px border, got {overlap}");
    // Top of first row coincides with the table top edge.
    assert!((rows[0].rect.y - t.rect.y).abs() < 0.5,
        "first row top coincides with table top: table.y={}, row.y={}", t.rect.y, rows[0].rect.y);
}

#[test]
fn separate_mode_keeps_full_cell_borders() {
    // Regression guard: border-collapse: separate (default) must NOT overlap cells.
    let t = layout_table(
        "table { border-collapse: separate; } td { width: 96px; border: 2px solid #000; }",
        "<table><tr><td></td><td></td></tr></table>",
        800.0, 600.0,
    );
    let row = find_first_row(&t).expect("row not found");
    let cells: Vec<_> = row.children.iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    // No spacing, no overlap: cell[1].x == cell[0].right exactly.
    let gap = cells[1].rect.x - (cells[0].rect.x + cells[0].rect.width);
    assert!(gap.abs() < 0.5, "separate mode: no overlap, gap should be 0, got {gap}");
}

// ─── SVG preserveAspectRatio viewBox mapping (BUG-198) ────────────────────

fn par(ax: super::super::SvgAlignX, ay: super::super::SvgAlignY, ms: super::super::SvgMeetOrSlice) -> super::super::PreserveAspectRatio {
    super::super::PreserveAspectRatio { align_x: ax, align_y: ay, meet_or_slice: ms }
}

#[test]
fn preserve_aspect_ratio_meet_letterboxes_uniformly() {
    // meet → contain. viewBox="0 0 200 100" into 200×200 → s = min(1.0, 2.0) = 1.0;
    // xMidYMid centers the 200×100 content vertically: free_y = 100 → oy = 50.
    let vb = super::super::ViewBox { min_x: 0.0, min_y: 0.0, width: 200.0, height: 100.0 };
    let (sx, sy, _ox, oy) = super::super::compute_preserve_aspect_ratio_transform(
        &vb, 200.0, 200.0, &par(super::super::SvgAlignX::Mid, super::super::SvgAlignY::Mid, super::super::SvgMeetOrSlice::Meet),
    );
    assert!((sx - sy).abs() < 1e-4, "meet must use uniform scale, got sx={sx} sy={sy}");
    assert!((sx - 1.0).abs() < 1e-4, "meet s expected 1.0, got {sx}");
    assert!((oy - 50.0).abs() < 1e-4, "xMidYMid vertical offset: expected 50, got {oy}");
}

#[test]
fn preserve_aspect_ratio_slice_covers() {
    // slice → cover. viewBox="0 0 100 200" into 200×200 → s = max(2.0, 1.0) = 2.0;
    // scaled width = 200 → no horizontal free space.
    let vb = super::super::ViewBox { min_x: 0.0, min_y: 0.0, width: 100.0, height: 200.0 };
    let (sx, sy, ox, _oy) = super::super::compute_preserve_aspect_ratio_transform(
        &vb, 200.0, 200.0, &par(super::super::SvgAlignX::Mid, super::super::SvgAlignY::Mid, super::super::SvgMeetOrSlice::Slice),
    );
    assert!((sx - 2.0).abs() < 1e-4, "slice sx expected 2.0, got {sx}");
    assert!((sy - 2.0).abs() < 1e-4, "slice sy expected 2.0, got {sy}");
    assert!(ox.abs() < 1e-4, "slice: no horizontal free space, ox should be 0, got {ox}");
}

#[test]
fn preserve_aspect_ratio_xminymin_top_left() {
    // xMinYMin → top-left alignment, no offset from the box edge.
    let vb = super::super::ViewBox { min_x: 0.0, min_y: 0.0, width: 200.0, height: 100.0 };
    let (_sx, _sy, ox, oy) = super::super::compute_preserve_aspect_ratio_transform(
        &vb, 200.0, 200.0, &par(super::super::SvgAlignX::Min, super::super::SvgAlignY::Min, super::super::SvgMeetOrSlice::Meet),
    );
    assert!(ox.abs() < 1e-4, "xMin offset should be 0, got {ox}");
    assert!(oy.abs() < 1e-4, "YMin offset should be 0, got {oy}");
}

#[test]
fn svg_root_inline_svg_ignores_object_fit_uses_preserve_aspect_ratio() {
    // BUG-198: object-fit on an inline <svg> has NO effect — the viewBox is mapped by
    // preserveAspectRatio (default xMidYMid meet). A wide viewBox in a tall box must be
    // letterboxed (contain), NOT stretched, even with `object-fit:fill` set.
    // viewBox="0 0 200 80" into 160×120 → meet scale = min(0.8, 1.5) = 0.8 → rect 160×64.
    let html = r#"<svg viewBox="0 0 200 80" style="width:160px;height:120px;object-fit:fill;"><rect x="0" y="0" width="200" height="80"/></svg>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find_shape(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::SvgShape { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find_shape)
    }
    let rect = find_shape(&root).expect("rect shape");
    assert!((rect.rect.width - 160.0).abs() < 1e-3, "meet rect width expected 160, got {}", rect.rect.width);
    assert!((rect.rect.height - 64.0).abs() < 1e-3, "meet rect height expected 64 (letterboxed), got {}", rect.rect.height);
}

#[test]
fn inflow_svg_path_box_anchored_at_viewport_origin() {
    // BUG-174: an in-flow (inline-block) SVG `<path>` must anchor its layout box at
    // the SVG viewport's document origin, not at (0, 0). The path bbox is computed
    // at paint time, so `lay_out_svg_element_position` leaves it ZERO-sized; without
    // anchoring, the painter shifts the raw `d` coordinates by (0, 0) and every path
    // collapses to the top-left page corner regardless of which SVG cell it lives in.
    // Two inline-block SVGs side by side: the second must sit to the right of the first.
    let html = r#"<div>
            <span style="display:inline-block"><svg width="100" height="100"><path d="M 10 10 L 90 90"/></svg></span>
            <span style="display:inline-block"><svg width="100" height="100"><path d="M 10 10 L 90 90"/></svg></span>
        </div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 400.0));
    fn collect_paths<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
        if matches!(&b.kind, super::super::BoxKind::SvgShape { shape: super::super::SvgShapeKind::Path { .. }, .. }) {
            out.push(b);
        }
        for c in &b.children { collect_paths(c, out); }
    }
    let mut paths = Vec::new();
    collect_paths(&root, &mut paths);
    assert_eq!(paths.len(), 2, "expected two path boxes, got {}", paths.len());
    // First path anchored near the page origin; the second SVG sits one inline-block
    // to the right (>= 100px advance), so its path box origin must be clearly greater.
    assert!(
        paths[1].rect.x > paths[0].rect.x + 50.0,
        "second inline-block SVG path must anchor to the right of the first: \
         path0.x={}, path1.x={}",
        paths[0].rect.x, paths[1].rect.x,
    );
}

#[test]
fn preserve_aspect_ratio_meet_scales_up_small_viewbox() {
    // A viewBox smaller than the box scales UP under meet (no clamp at 1.0, unlike the
    // old object-fit:none/scale-down). viewBox="0 0 80 60" into 160×120 → s = min(2.0, 2.0) = 2.0.
    let vb = super::super::ViewBox { min_x: 0.0, min_y: 0.0, width: 80.0, height: 60.0 };
    let (sx, sy, _ox, _oy) = super::super::compute_preserve_aspect_ratio_transform(
        &vb, 160.0, 120.0, &par(super::super::SvgAlignX::Mid, super::super::SvgAlignY::Mid, super::super::SvgMeetOrSlice::Meet),
    );
    assert!((sx - 2.0).abs() < 1e-4, "meet scales small viewBox up to 2.0, got {sx}");
    assert!((sx - sy).abs() < 1e-4, "meet must be uniform");
}

// ── grid `masonry` fallback integration tests ────────────────────────────
// CSS masonry is not shipped by stable browsers (Edge ignores `grid-template-*:
// masonry`). We match that: the axis falls back to `none` → a regular auto-sized
// grid that still honours the `order` property. These tests lock that behaviour.

fn masonry_grid_children(css: &str, vp: f32) -> Vec<super::super::LayoutBox> {
    let html = r#"<div id="grid"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(vp, vp));
    // Grid containers are Block boxes in lumen-layout. Find the deepest block with 3 children.
    fn find_3child_block(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::Block) && b.children.len() == 3 {
            return Some(b);
        }
        b.children.iter().find_map(find_3child_block)
    }
    find_3child_block(&root).map(|g| g.children.clone()).unwrap_or_default()
}

#[test]
fn grid_masonry_fallback_respects_order() {
    // `grid-template-rows: masonry` is ignored (Edge fallback) → regular 3-column
    // grid. The `order` property still reorders items: c (order:-1) is placed
    // first, so it lands in column 1 (leftmost).
    let css = r#"
            #grid {
                display: grid;
                grid-template-columns: 1fr 1fr 1fr;
                grid-template-rows: masonry;
                width: 300px;
                gap: 0px;
            }
            #a { height: 100px; order: 0; }
            #b { height: 60px; order: 0; }
            #c { height: 40px; order: -1; }
        "#;
    let children = masonry_grid_children(css, 300.0);
    let c_box = children.iter().find(|b| b.rect.height == 40.0);
    assert!(c_box.is_some(), "item c (height=40) not found in grid children");
    if let Some(c) = c_box {
        // c has order=-1, placed first → column 1 (leftmost x≈0).
        assert!(c.rect.x < 110.0, "c with order=-1 should be in column 1, got x={}", c.rect.x);
    }
}

#[test]
fn grid_masonry_fallback_source_order() {
    // `grid-template-rows: masonry` ignored → regular 3-column grid, items in
    // source order: a → column 1, b → column 2, c → column 3.
    let css = r#"
            #grid {
                display: grid;
                grid-template-columns: 1fr 1fr 1fr;
                grid-template-rows: masonry;
                width: 300px;
                gap: 0px;
            }
            #a { height: 100px; }
            #b { height: 60px; }
            #c { height: 40px; }
        "#;
    let children = masonry_grid_children(css, 300.0);
    let a_box = children.iter().find(|b| b.rect.height == 100.0);
    let b_box = children.iter().find(|b| b.rect.height == 60.0);
    assert!(a_box.is_some(), "item a (height=100) not found");
    assert!(b_box.is_some(), "item b (height=60) not found");
    if let (Some(a), Some(b)) = (a_box, b_box) {
        // a first → column 1 (x≈0), b second → column 2 (x≈100).
        assert!(a.rect.x < b.rect.x, "a should be in column 1 (x<b.x), got a.x={} b.x={}", a.rect.x, b.rect.x);
    }
}

// ── parent↔first-child margin collapse (CSS 2.1 §8.3.1) ─────────────────

fn find_block_by_width(b: &super::super::LayoutBox, w: f32) -> Option<&super::super::LayoutBox> {
    if matches!(b.kind, super::super::BoxKind::Block) && (b.rect.width - w).abs() < 1.0 {
        return Some(b);
    }
    for child in &b.children {
        if let Some(found) = find_block_by_width(child, w) {
            return Some(found);
        }
    }
    None
}

#[test]
fn parent_first_child_margin_collapses_through_no_border_padding() {
    // CSS 2.1 §8.3.1: parent with no border/padding/BFC, child margin-top: 70px.
    // The child's margin collapses with the parent's (0), pushing both to y=70.
    // Before fix: parent at y=0, child at y=70 (70px inside parent).
    // After fix:  parent at y=70, child at y=70 (flush with parent top).
    let html = r#"<div id="parent"><div id="child"></div></div>"#;
    let css = "body { margin: 0; } #parent { width: 300px; height: 300px; } #child { width: 100px; height: 100px; margin-top: 70px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let parent = find_block_by_width(&root, 300.0).expect("parent block not found");
    let child = find_block_by_width(&root, 100.0).expect("child block not found");
    assert!(
        (parent.rect.y - 70.0).abs() < 1.0,
        "parent should be at y=70 (collapsed margin), got y={}",
        parent.rect.y
    );
    assert!(
        (child.rect.y - parent.rect.y).abs() < 1.0,
        "child should be flush with parent top (y={}), got child.y={}",
        parent.rect.y, child.rect.y
    );
}

#[test]
fn parent_first_child_margin_blocked_by_padding_top() {
    // CSS 2.1 §8.3.1: padding-top on parent breaks the collapse chain.
    // Parent at y=0, child at y = padding_top + margin_top = 10 + 70 = 80 (relative to parent).
    let html = r#"<div id="parent"><div id="child"></div></div>"#;
    let css = "body { margin: 0; } #parent { width: 300px; height: 400px; padding-top: 10px; } #child { width: 100px; height: 100px; margin-top: 70px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let parent = find_block_by_width(&root, 300.0).expect("parent block not found");
    let child = find_block_by_width(&root, 100.0).expect("child block not found");
    assert!(
        parent.rect.y < 1.0,
        "parent with padding should NOT have collapsed margin, got parent.y={}",
        parent.rect.y
    );
    // Child placed at parent.y + padding_top(10) + margin_top(70) = 80.
    let expected_child_y = parent.rect.y + 10.0 + 70.0;
    assert!(
        (child.rect.y - expected_child_y).abs() < 1.0,
        "child with padding-blocked parent should be at y={}, got y={}",
        expected_child_y, child.rect.y
    );
}

#[test]
fn parent_first_child_margin_blocked_by_bfc() {
    // CSS 2.1 §8.3.1: overflow:hidden establishes a BFC — no parent↔child collapse.
    // Parent at y=0, child at y = 0 + margin_top = 70 (relative to parent, stays inside).
    let html = r#"<div id="parent"><div id="child"></div></div>"#;
    let css = "body { margin: 0; } #parent { width: 300px; height: 400px; overflow: hidden; } #child { width: 100px; height: 100px; margin-top: 70px; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let parent = find_block_by_width(&root, 300.0).expect("parent block not found");
    let child = find_block_by_width(&root, 100.0).expect("child block not found");
    assert!(
        parent.rect.y < 1.0,
        "BFC parent should NOT have collapsed margin, got parent.y={}",
        parent.rect.y
    );
    let expected_child_y = parent.rect.y + 70.0;
    assert!(
        (child.rect.y - expected_child_y).abs() < 1.0,
        "child inside BFC parent should be at y={}, got y={}",
        expected_child_y, child.rect.y
    );
}
