use super::*;

// ──────── background-* (CSS Backgrounds L3) ────────

#[test]
fn background_image_url_parses() {
    let root = lay("<p>x</p>", "p { background-image: url(\"bg.png\"); }");
    let s = first_p_style(&root);
    assert_eq!(s.background_layers[0].image, BackgroundImage::Url("bg.png".into()));
}

#[test]
fn background_image_url_unquoted() {
    let root = lay("<p>x</p>", "p { background-image: url(bg.png); }");
    assert_eq!(
        first_p_style(&root).background_layers[0].image,
        BackgroundImage::Url("bg.png".into())
    );
}

#[test]
fn background_image_none() {
    // Setting "none" after a URL replaces all layers with one None-image layer.
    let root = lay(
        "<p>x</p>",
        "p { background-image: url(\"x.png\"); background-image: none; }",
    );
    assert_eq!(first_p_style(&root).background_layers[0].image, BackgroundImage::None);
}

#[test]
fn background_image_gradient_parsed_linear() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: linear-gradient(to right, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, stops, .. }) => {
            assert!((angle_deg - 90.0).abs() < 0.1, "expected 90° for 'to right'");
            assert_eq!(stops.len(), 2);
        }
        other => panic!("expected ParsedGradient::Linear, got {other:?}"),
    }
}

// ── parse_gradient_stops ──────────────────────────────────────────────────

#[test]
fn gradient_stops_empty_string_returns_empty() {
    assert_eq!(parse_gradient_stops(""), vec![]);
}

#[test]
fn gradient_stops_no_parens_returns_empty() {
    assert_eq!(parse_gradient_stops("linear-gradient"), vec![]);
}

#[test]
fn gradient_stops_two_named_colors_no_position() {
    let stops = parse_gradient_stops("linear-gradient(red, blue)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[0].position, None);
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
    assert_eq!(stops[1].position, None);
}

#[test]
fn gradient_stops_to_right_direction_skipped() {
    let stops = parse_gradient_stops("linear-gradient(to right, red, blue)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_angle_direction_skipped() {
    let stops = parse_gradient_stops("linear-gradient(45deg, red 0%, blue 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(100.0)));
}

#[test]
fn gradient_stops_percent_positions_parsed() {
    let stops = parse_gradient_stops("linear-gradient(red 0%, green 50%, blue 100%)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
    assert_eq!(stops[2].position, Some(Length::Percent(100.0)));
}

#[test]
fn gradient_stops_px_positions_parsed() {
    let stops = parse_gradient_stops("linear-gradient(red 0px, blue 200px)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].position, Some(Length::Px(0.0)));
    assert_eq!(stops[1].position, Some(Length::Px(200.0)));
}

#[test]
fn gradient_stops_hex_color_with_percent() {
    let stops = parse_gradient_stops("linear-gradient(#ff0000 20%, #0000ff 80%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[0].position, Some(Length::Percent(20.0)));
}

#[test]
fn gradient_stops_rgba_function_color() {
    let stops = parse_gradient_stops("linear-gradient(rgba(255,0,0,1) 0%, rgba(0,0,255,1) 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_two_position_stop_expands() {
    // `red 20% 60%` → two stops: red@20% and red@60%
    let stops = parse_gradient_stops("linear-gradient(red 20% 60%, blue)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(20.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(60.0)));
    assert_eq!(stops[1].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[2].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_color_hint_skipped() {
    // `50%` between stops is a color hint — no color → skipped
    let stops = parse_gradient_stops("linear-gradient(red 0%, 50%, blue 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_radial_shape_skipped() {
    let stops =
        parse_gradient_stops("radial-gradient(circle at 50% 50%, white, black)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 255, b: 255, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 0, a: 255 });
}

#[test]
fn gradient_stops_repeating_linear() {
    let stops =
        parse_gradient_stops("repeating-linear-gradient(red 0px, blue 10px)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[0].position, Some(Length::Px(0.0)));
    assert_eq!(stops[1].position, Some(Length::Px(10.0)));
}

#[test]
fn gradient_stops_zero_unitless_is_px_zero() {
    let stops = parse_gradient_stops("linear-gradient(red 0, blue 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].position, Some(Length::Px(0.0)));
}

// ── conic-gradient parsing ───────────────────────────────────────────────

#[test]
fn background_image_gradient_parsed_conic_default() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
        }) => {
            assert!((center_x_pct - 0.5).abs() < 1e-4);
            assert!((center_y_pct - 0.5).abs() < 1e-4);
            assert!(from_angle_deg.abs() < 1e-4, "default from-angle = 0°");
            assert_eq!(stops.len(), 2);
            assert!(!repeating);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_image_gradient_parsed_conic_from_and_at() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(from 90deg at 25% 75%, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, ..
        }) => {
            assert!((center_x_pct - 0.25).abs() < 1e-4);
            assert!((center_y_pct - 0.75).abs() < 1e-4);
            assert!((from_angle_deg - 90.0).abs() < 1e-3);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_image_gradient_parsed_repeating_conic() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: repeating-conic-gradient(red 0deg, blue 90deg); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic { repeating, stops, .. }) => {
            assert!(repeating);
            assert_eq!(stops.len(), 2);
            // 0deg → 0%, 90deg → 25%.
            assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
            if let Some(Length::Percent(p)) = stops[1].position {
                assert!((p - 25.0).abs() < 1e-3, "90deg should map to 25%, got {p}");
            } else {
                panic!("expected Percent position, got {:?}", stops[1].position);
            }
        }
        other => panic!("expected repeating Conic, got {other:?}"),
    }
}

#[test]
fn conic_stops_angles_converted_to_percent() {
    let stops = parse_gradient_stops("conic-gradient(red 0deg, green 180deg, blue 360deg)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
    if let Some(Length::Percent(p)) = stops[2].position {
        assert!((p - 100.0).abs() < 1e-3);
    } else {
        panic!("expected Percent");
    }
}

#[test]
fn conic_stops_turn_unit() {
    let stops = parse_gradient_stops("conic-gradient(red 0turn, blue 0.5turn)");
    assert_eq!(stops.len(), 2);
    if let Some(Length::Percent(p)) = stops[1].position {
        assert!((p - 50.0).abs() < 1e-3, "0.5turn should map to 50%, got {p}");
    } else {
        panic!("expected Percent");
    }
}

#[test]
fn conic_stops_percent_passthrough() {
    let stops = parse_gradient_stops("conic-gradient(red 0%, blue 25%, green 100%)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(25.0)));
    assert_eq!(stops[2].position, Some(Length::Percent(100.0)));
}

#[test]
fn conic_stops_named_colors_no_position() {
    // No explicit positions: auto-distributed by renderer; parser keeps None.
    let stops = parse_gradient_stops("conic-gradient(red, green, blue)");
    assert_eq!(stops.len(), 3);
    for s in &stops {
        assert!(s.position.is_none());
    }
}

#[test]
fn conic_from_and_at_parsed_independently() {
    use crate::style::ParsedGradient;
    // Only `at` clause, no `from`.
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(at 10% 20%, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, ..
        }) => {
            assert!((center_x_pct - 0.1).abs() < 1e-4);
            assert!((center_y_pct - 0.2).abs() < 1e-4);
            assert!(from_angle_deg.abs() < 1e-4);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn conic_from_turn_unit() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(from 0.25turn, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic { from_angle_deg, .. }) => {
            // 0.25turn = 90deg.
            assert!((from_angle_deg - 90.0).abs() < 1e-3, "got {from_angle_deg}");
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_image_gradient_parsed_conic_keyword_position() {
    use crate::style::ParsedGradient;
    // `at top left` → (0, 0).
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(at left top, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, ..
        }) => {
            assert!(center_x_pct.abs() < 1e-4);
            assert!(center_y_pct.abs() < 1e-4);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_repeat_values() {
    for (s, expected) in [
        ("repeat", BackgroundRepeat::REPEAT),
        ("no-repeat", BackgroundRepeat::NO_REPEAT),
        ("repeat-x", BackgroundRepeat::REPEAT_X),
        ("repeat-y", BackgroundRepeat::REPEAT_Y),
        ("round", BackgroundRepeat::ROUND),
        ("space", BackgroundRepeat::SPACE),
    ] {
        let css = format!("p {{ background-repeat: {s}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).background_layers[0].repeat, expected);
    }
}

#[test]
fn background_size_keywords() {
    for (s, expected) in [
        ("auto", BackgroundSize::Auto),
        ("cover", BackgroundSize::Cover),
        ("contain", BackgroundSize::Contain),
    ] {
        let css = format!("p {{ background-size: {s}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).background_layers[0].size, expected);
    }
}

#[test]
fn background_size_length_single() {
    let root = lay("<p>x</p>", "p { background-size: 200px; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Px(200.0));
            assert_eq!(h, BgSizeAxis::Auto);
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_size_length_pair() {
    let root = lay("<p>x</p>", "p { background-size: 200px 100px; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Px(200.0));
            assert_eq!(h, BgSizeAxis::Px(100.0));
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_size_percent_pair() {
    // BUG-115: percent background-size must be preserved as Percent fractions.
    let root = lay("<p>x</p>", "p { background-size: 40% 60%; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Percent(0.4));
            assert_eq!(h, BgSizeAxis::Percent(0.6));
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_size_mixed_px_percent() {
    // BUG-115: `20px 100%` — one fixed axis, one percent axis.
    let root = lay("<p>x</p>", "p { background-size: 20px 100%; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Px(20.0));
            assert_eq!(h, BgSizeAxis::Percent(1.0));
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_shorthand_percent_size() {
    // BUG-115: `background: <grad> left center / 40% 60% no-repeat` — the
    // percent size in the shorthand must reach the layer as Percent axes.
    let css = "p { background: linear-gradient(to right, #e74c3c, #c0392b) left center / 40% 60% no-repeat, #2c3e50; }";
    let root = lay("<p>x</p>", css);
    let layers = &first_p_style(&root).background_layers;
    assert_eq!(
        layers[0].size,
        BackgroundSize::Length(BgSizeAxis::Percent(0.4), BgSizeAxis::Percent(0.6))
    );
}

#[test]
fn background_attachment_values() {
    for (s, expected) in [
        ("scroll", BackgroundAttachment::Scroll),
        ("fixed", BackgroundAttachment::Fixed),
        ("local", BackgroundAttachment::Local),
    ] {
        let css = format!("p {{ background-attachment: {s}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).background_layers[0].attachment, expected);
    }
}

#[test]
fn background_properties_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { background-image: url(x.png); background-repeat: no-repeat; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // Child element has no background declarations → empty layers (initial state).
    assert!(p.style.background_layers.is_empty());
}

// ──────── place-items / align-* / justify-* (CSS Box Alignment L3) ────────

#[test]
fn align_items_center() {
    let root = lay("<p>x</p>", "p { align-items: center; }");
    assert_eq!(first_p_style(&root).align_items, AlignValue::Center);
}

#[test]
fn justify_content_space_between() {
    let root = lay("<p>x</p>", "p { justify-content: space-between; }");
    assert_eq!(first_p_style(&root).justify_content, AlignValue::SpaceBetween);
}

#[test]
fn flex_start_alias() {
    // CSS spec: flex-start alias для start (вне flex-контекста).
    let root = lay("<p>x</p>", "p { align-items: flex-start; }");
    assert_eq!(first_p_style(&root).align_items, AlignValue::Start);
}

#[test]
fn place_items_single_value() {
    let root = lay("<p>x</p>", "p { place-items: center; }");
    let s = first_p_style(&root);
    // Single value применяется к обоим осям.
    assert_eq!(s.align_items, AlignValue::Center);
    assert_eq!(s.justify_items, AlignValue::Center);
}

#[test]
fn place_items_two_values() {
    let root = lay("<p>x</p>", "p { place-items: start end; }");
    let s = first_p_style(&root);
    assert_eq!(s.align_items, AlignValue::Start);
    assert_eq!(s.justify_items, AlignValue::End);
}

#[test]
fn place_self_shorthand() {
    let root = lay("<p>x</p>", "p { place-self: center stretch; }");
    let s = first_p_style(&root);
    assert_eq!(s.align_self, AlignValue::Center);
    assert_eq!(s.justify_self, AlignValue::Stretch);
}

#[test]
fn place_content_shorthand() {
    let root = lay("<p>x</p>", "p { place-content: space-around; }");
    let s = first_p_style(&root);
    assert_eq!(s.align_content, AlignValue::SpaceAround);
    assert_eq!(s.justify_content, AlignValue::SpaceAround);
}

#[test]
fn align_unknown_value_ignored() {
    let root = lay("<p>x</p>", "p { align-items: garbage; }");
    // default (Auto) сохраняется.
    assert_eq!(first_p_style(&root).align_items, AlignValue::Auto);
}

#[test]
fn alignment_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { align-items: center; justify-content: space-between; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // У p должны быть defaults.
    assert_eq!(p.style.align_items, AlignValue::Auto);
    assert_eq!(p.style.justify_content, AlignValue::Auto);
    // У div — заданные.
    assert_eq!(div.style.align_items, AlignValue::Center);
    assert_eq!(div.style.justify_content, AlignValue::SpaceBetween);
}

#[test]
fn align_value_parse_all_keywords() {
    for (s, expected) in [
        ("auto", AlignValue::Auto),
        ("normal", AlignValue::Normal),
        ("stretch", AlignValue::Stretch),
        ("start", AlignValue::Start),
        ("end", AlignValue::End),
        ("center", AlignValue::Center),
        ("baseline", AlignValue::Baseline),
        ("space-between", AlignValue::SpaceBetween),
        ("space-around", AlignValue::SpaceAround),
        ("space-evenly", AlignValue::SpaceEvenly),
        ("flex-start", AlignValue::Start),
        ("flex-end", AlignValue::End),
        ("self-start", AlignValue::Start),
        ("CENTER", AlignValue::Center),  // case-insensitive
    ] {
        assert_eq!(AlignValue::parse(s), Some(expected), "input: {s}");
    }
}

#[test]
fn align_value_parse_unknown_returns_none() {
    assert_eq!(AlignValue::parse("garbage"), None);
    assert_eq!(AlignValue::parse(""), None);
}

#[test]
fn gap_and_aspect_ratio_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { gap: 20px; aspect-ratio: 2; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.row_gap, Length::Px(0.0));
    assert_eq!(p.style.aspect_ratio, None);
    assert_eq!(div.style.row_gap, Length::Px(20.0));
    assert!(div.style.aspect_ratio.is_some());
}

#[test]
fn media_prefers_color_scheme_light_default() {
    // Phase 0: prefers_dark=false → 'light' matches.
    let root = lay_with_viewport(
        "<p>x</p>",
        "@media (prefers-color-scheme: light) { p { color: red; } }",
        800.0,
        600.0,
    );
    let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

// ── CSS Quirks Mode — UA-rule для <table> ──────────────────────────────

/// В Quirks-mode (нет DOCTYPE) `<table>` сбрасывает font-size к
/// initial-значению, не наследует от родителя.
#[test]
fn quirks_table_font_size_resets_to_initial() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (body.style.font_size - 30.0).abs() < 0.01,
        "body должен наследовать заявленные 30px"
    );
    assert!(
        (table.style.font_size - 16.0).abs() < 0.01,
        "table в Quirks должен сбросить font-size к initial 16, получено {}",
        table.style.font_size
    );
}

/// В Standards mode (`<!DOCTYPE html>`) `<table>` наследует font-size
/// от родителя как обычный элемент.
#[test]
fn standards_table_font_size_inherits() {
    let root = lay(
        "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (table.style.font_size - 30.0).abs() < 0.01,
        "table в Standards должен наследовать 30px, получено {}",
        table.style.font_size
    );
}

/// В Quirks color у `<table>` сбрасывается к BLACK, не наследуется.
#[test]
fn quirks_table_color_resets_to_black() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { color: red; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(table.style.color, Color::BLACK);
}

/// В Standards color наследуется.
#[test]
fn standards_table_color_inherits() {
    let root = lay(
        "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
        "body { color: red; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// В Quirks font-weight у `<table>` сбрасывается к NORMAL.
#[test]
fn quirks_table_font_weight_resets_to_normal() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-weight: bold; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.font_weight, FontWeight::BOLD);
    assert_eq!(table.style.font_weight, FontWeight::NORMAL);
}

/// В Quirks font-style у `<table>` сбрасывается к Normal.
#[test]
fn quirks_table_font_style_resets_to_normal() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-style: italic; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.font_style, FontStyle::Italic);
    assert_eq!(table.style.font_style, FontStyle::Normal);
}

/// В Quirks text-align у `<table>` сбрасывается к initial (Start).
#[test]
fn quirks_table_text_align_resets_to_left() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { text-align: center; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.text_align, TextAlign::Center);
    assert_eq!(table.style.text_align, TextAlign::Start);
}

/// В Quirks white-space у `<table>` сбрасывается к Normal.
#[test]
fn quirks_table_white_space_resets_to_normal() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { white-space: nowrap; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.white_space, WhiteSpace::Nowrap);
    assert_eq!(table.style.white_space, WhiteSpace::Normal);
}

/// Author CSS поверх Quirks-reset выигрывает: spec-rule идёт как
/// низший cascade origin (UA).
#[test]
fn quirks_table_author_css_wins_over_reset() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; } table { font-size: 24px; color: blue; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (table.style.font_size - 24.0).abs() < 0.01,
        "author CSS должен переопределить Quirks-reset"
    );
    assert_eq!(table.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
}

/// Дочерние элементы `<table>` в Quirks наследуют от сброшенных
/// значений таблицы, не от прародителя.
#[test]
fn quirks_table_children_inherit_reset_values() {
    // <body>=30px → <table>=16 (reset) → <td>=16 (inherits from table).
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; }",
    );
    let body = &root;
    let table = first_element_child(body);
    // HTML5 parser inserts implicit <tbody>: table → tbody → tr → td.
    // Идём вглубь, пока не найдём td (Block inside a TableRow).
    fn find_td(b: &LayoutBox) -> Option<&LayoutBox> {
        for c in &b.children {
            if matches!(&c.kind, BoxKind::TableRow | BoxKind::TableRowGroup) {
                if let Some(td) = find_td(c) {
                    return Some(td);
                }
            } else if matches!(&c.kind, BoxKind::Block) {
                if let Some(td) = find_td(c) {
                    return Some(td);
                }
                return Some(c);
            }
        }
        None
    }
    let td = find_td(table).expect("td не найден");
    assert!(
        (td.style.font_size - 16.0).abs() < 0.01,
        "td должен унаследовать от table сброшенные 16px, получено {}",
        td.style.font_size
    );
}

/// Не-`<table>` элементы в Quirks-mode не сбрасывают inherited.
#[test]
fn quirks_non_table_inherits_normally() {
    let root = lay(
        "<body><p>x</p></body>",
        "body { font-size: 30px; color: red; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert!(
        (p.style.font_size - 30.0).abs() < 0.01,
        "<p> в Quirks-mode должен наследовать font-size, получено {}",
        p.style.font_size
    );
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// LimitedQuirks (HTML 4.01 Transitional) — table-reset не применяется
/// (spec §4.1: только в Quirks-mode).
#[test]
fn limited_quirks_does_not_apply_table_reset() {
    let root = lay(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; color: red; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (table.style.font_size - 30.0).abs() < 0.01,
        "table в LimitedQuirks должен наследовать font-size как в Standards"
    );
    assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

// ── CSS Quirks Mode §3.4 — «hashless hex color quirk» ──────────────────

/// В Quirks-mode `color: ff0000` (без `#`) парсится как red.
/// Это эквивалент `color: #ff0000` (CSS Quirks Mode §3.4).
#[test]
fn quirks_hashless_hex_in_color_property() {
    // Нет DOCTYPE → Quirks.
    let root = lay(
        "<body><p>x</p></body>",
        "p { color: ff0000; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// В Standards-mode `color: ff0000` (без `#`) — невалидное значение,
/// игнорируется. Цвет наследуется (по умолчанию BLACK).
#[test]
fn standards_hashless_hex_rejected_in_color_property() {
    let root = lay(
        "<!DOCTYPE html><body><p>x</p></body>",
        "p { color: ff0000; }",
    );
    let body = &root;
    let p = first_element_child(body);
    // ff0000 без `#` — невалидно в Standards, color остаётся inherited
    // от body (BLACK).
    assert_eq!(p.style.color, Color::BLACK);
}

/// В Quirks `background-color: 00ff00` (6-hex без `#`) парсится как green.
#[test]
fn quirks_hashless_hex_in_background_color() {
    let root = lay(
        "<body><p>x</p></body>",
        "p { background-color: 00ff00; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(p.style.background_color, Some(CssColor::Rgba(Color { r: 0, g: 255, b: 0, a: 255 })));
}

/// В Quirks 3-hex bare digit-ы тоже парсятся: `f00` → red.
#[test]
fn quirks_hashless_hex_3_digit_short() {
    let root = lay(
        "<body><p>x</p></body>",
        "p { color: f00; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// В Quirks border-color принимает bare hex.
#[test]
fn quirks_hashless_hex_in_border_color() {
    let root = lay(
        "<body><p>x</p></body>",
        "p { border: 1px solid 0000ff; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(
        p.style.border_top_color,
        CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 }),
    );
}

/// LimitedQuirks (HTML 4.01 Transitional) — hashless hex quirk
/// НЕ применяется (spec §1.1.1: «full quirks mode only»).
#[test]
fn limited_quirks_hashless_hex_rejected() {
    let root = lay(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><p>x</p></body>",
        "p { color: ff0000; }",
    );
    let body = &root;
    let p = first_element_child(body);
    // В LimitedQuirks bare hex — invalid, как в Standards.
    assert_eq!(p.style.color, Color::BLACK);
}

// ──────────────── CSS Quirks Mode §3.5 — html viewport height ────────────────

/// В quirks-mode `<html>` получает UA-правило `height: 100vh`, поэтому
/// его rect.height равен высоте viewport (600.0 в тестовом lay).
#[test]
fn quirks_html_height_equals_viewport() {
    let root = lay_full("<html><body><p>x</p></body></html>", "");
    let (html, _body) = html_and_body(&root);
    assert!(
        (html.rect.height - 600.0).abs() < 0.1,
        "quirks: html.rect.height={} (ожидалось 600.0)",
        html.rect.height
    );
}

/// В quirks-mode `body { height: 100% }` резолвится против viewport
/// через html-box с высотой 100vh.
#[test]
fn quirks_body_height_100pct_resolves_to_viewport() {
    let root = lay_full(
        "<html><body></body></html>",
        "body { height: 100%; }",
    );
    let (_html, body) = html_and_body(&root);
    assert!(
        (body.rect.height - 600.0).abs() < 0.1,
        "quirks: body.rect.height={} (ожидалось 600.0)",
        body.rect.height
    );
}

/// В standards-mode (с `<!DOCTYPE html>`) `<html>` с высотой auto
/// НЕ получает 100vh — высота определяется контентом (маленькая).
#[test]
fn standards_html_height_is_content_not_viewport() {
    let root = lay_full(
        "<!DOCTYPE html><html><body><p style=\"height:20px\">x</p></body></html>",
        "",
    );
    let (html, _body) = html_and_body(&root);
    // Контент высотой 20px + margins body → html значительно < 600.
    assert!(
        html.rect.height < 200.0,
        "standards: html.rect.height={} (ожидалось меньше 200.0)",
        html.rect.height
    );
}

/// В quirks-mode author CSS на `<html>` перекрывает UA-правило 100vh.
#[test]
fn quirks_html_author_height_overrides_ua_rule() {
    let root = lay_full(
        "<html><body></body></html>",
        "html { height: 300px; }",
    );
    let (html, _body) = html_and_body(&root);
    assert!(
        (html.rect.height - 300.0).abs() < 0.1,
        "quirks: author height=300px, html.rect.height={} (ожидалось 300.0)",
        html.rect.height
    );
}

/// В limited-quirks mode (HTML 4.01 Transitional + system_id) правило
/// §3.5 НЕ применяется — только full quirks mode.
#[test]
fn limited_quirks_html_height_is_content_not_viewport() {
    let root = lay_full(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \
         \"http://www.w3.org/TR/html4/loose.dtd\">\
         <html><body><p style=\"height:20px\">x</p></body></html>",
        "",
    );
    let (html, _body) = html_and_body(&root);
    assert!(
        html.rect.height < 200.0,
        "limited-quirks: html.rect.height={} (ожидалось меньше 200.0)",
        html.rect.height
    );
}

// ──────────────── :fullscreen / :modal / :popover-open (open-state pseudo-classes) ────────────────

/// `:fullscreen` (Fullscreen API §4.2) — Phase 0 без runtime top-layer
/// никакой элемент не считается fullscreen, правило не применяется.
#[test]
fn fullscreen_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<div>x</div>",
        "div:fullscreen { color: red; }",
        "div",
    );
    assert_eq!(c.r, 0);
}

/// `:fullscreen` не активируется даже на дочернем элементе с
/// контейнером — top-layer state runtime-only.
#[test]
fn fullscreen_pseudo_never_matches_nested() {
    let c = element_color(
        "<div><p>x</p></div>",
        "p:fullscreen { color: red; }",
        "p",
    );
    assert_eq!(c.r, 0);
}

/// `:modal` (CSS Selectors L4 §16.5.2) — Phase 0 без dialog runtime.
/// `<dialog open>` НЕ модален: атрибут `open` ставится и через
/// `dialog.show()` (non-modal), поэтому простая DOM-проверка не покрыла
/// бы spec — matcher всегда `false`.
#[test]
fn modal_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<dialog open>x</dialog>",
        "dialog:modal { color: red; }",
        "dialog",
    );
    assert_eq!(c.r, 0);
}

/// `:modal` не активируется и без атрибута `open`.
#[test]
fn modal_pseudo_never_matches_closed_dialog() {
    let c = element_color(
        "<dialog>x</dialog>",
        "dialog:modal { color: red; }",
        "dialog",
    );
    assert_eq!(c.r, 0);
}

/// `:popover-open` (HTML LS §6.12.2) — Phase 0 без Popover API runtime.
/// Наличие атрибута `popover` декларирует тип, но не открытое состояние.
#[test]
fn popover_open_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        r#"<div popover="auto">x</div>"#,
        "div:popover-open { color: red; }",
        "div",
    );
    assert_eq!(c.r, 0);
}

/// `:popover-open` не матчит и при отсутствии `popover`-атрибута.
#[test]
fn popover_open_pseudo_never_matches_non_popover() {
    let c = element_color(
        "<div>x</div>",
        "div:popover-open { color: red; }",
        "div",
    );
    assert_eq!(c.r, 0);
}

/// Specificity открытых-состояния pseudo-классов — class-уровня (0,1,0).
/// `:not(:fullscreen)` через always-false означает «всегда true» — это
/// удобный FOUC-protection idiom (если когда-нибудь fullscreen runtime
/// появится, правило сбросится). Проверяем, что `:not(:fullscreen)`
/// действительно матчит обычный element.
#[test]
fn not_fullscreen_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<div>x</div>",
        "div:not(:fullscreen) { color: red; }",
        "div",
    );
    assert_eq!(c.r, 255);
}

/// То же для `:not(:modal)`: элементы не в modal state — все элементы
/// в Phase 0.
#[test]
fn not_modal_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<dialog open>x</dialog>",
        "dialog:not(:modal) { color: red; }",
        "dialog",
    );
    assert_eq!(c.r, 255);
}

/// То же для `:not(:popover-open)`.
#[test]
fn not_popover_open_matches_all_elements_in_phase_0() {
    let c = element_color(
        r#"<div popover="auto">x</div>"#,
        "div:not(:popover-open) { color: red; }",
        "div",
    );
    assert_eq!(c.r, 255);
}
