//! Разбор командной строки: `PageSource`, режимы дампа, `--screenshot`,
//! `--print-to-pdf`, `--trace-nav`, переопределение вьюпорта.

use super::*;

#[test]
fn page_source_from_arg_url() {
    assert!(matches!(
        PageSource::from_arg(Some("https://example.com")),
        PageSource::Url(ref u) if u == "https://example.com"
    ));
    assert!(matches!(
        PageSource::from_arg(Some("http://localhost:8080")),
        PageSource::Url(_)
    ));
}

#[test]
fn page_source_from_arg_file() {
    let s = PageSource::from_arg(Some("samples/page.html"));
    match s {
        PageSource::File(p) => assert_eq!(p, PathBuf::from("samples/page.html")),
        _ => panic!("expected File"),
    }
}

#[test]
fn page_source_from_arg_none_is_empty() {
    assert!(matches!(PageSource::from_arg(None), PageSource::Empty));
}

#[test]
fn page_source_describe() {
    assert_eq!(PageSource::Empty.describe(), "(РїСѓСЃС‚Р°СЏ РІРєР»Р°РґРєР°)");
    assert_eq!(
        PageSource::Url("https://x.test".to_owned()).describe(),
        "https://x.test",
    );
    assert_eq!(
        PageSource::File(PathBuf::from("a.html")).describe(),
        "a.html",
    );
}

#[test]
fn page_source_about_blank_from_arg() {
    assert!(matches!(
        PageSource::from_arg(Some("about:blank")),
        PageSource::AboutBlank
    ));
}

#[test]
fn page_source_about_blank_url_str() {
    assert_eq!(PageSource::AboutBlank.url_str(), Some("about:blank"));
}

#[test]
fn page_source_about_blank_describe() {
    assert_eq!(PageSource::AboutBlank.describe(), "about:blank");
}

#[test]
fn collect_link_hrefs_multiple() {
    let doc = lumen_html_parser::parse(
        r#"<html><head>
                <link rel="stylesheet" href="a.css">
                <link rel="stylesheet" href="b.css">
            </head><body></body></html>"#,
    );
    let mut hrefs = Vec::new();
    collect_link_hrefs(&doc, doc.root(), &mut hrefs, &screen_media_context(Size::new(1024.0, 720.0), false));
    let only_hrefs: Vec<&str> = hrefs.iter().map(|(_, h)| h.as_str()).collect();
    assert_eq!(only_hrefs, vec!["a.css", "b.css"]);
}

#[test]
fn dump_kind_from_flag_recognised() {
    assert_eq!(DumpKind::from_flag("--dump-source"), Some(DumpKind::Source));
    assert_eq!(DumpKind::from_flag("--dump-layout"), Some(DumpKind::Layout));
    assert_eq!(
        DumpKind::from_flag("--dump-display-list"),
        Some(DumpKind::DisplayList),
    );
}

#[test]
fn dump_kind_from_flag_unknown() {
    assert_eq!(DumpKind::from_flag("--dump"), None);
    assert_eq!(DumpKind::from_flag("--dump-html"), None);
    assert_eq!(DumpKind::from_flag("samples/page.html"), None);
    assert_eq!(DumpKind::from_flag(""), None);
}

#[test]
fn should_restore_session_empty_source_no_automation() {
    assert!(should_restore_session(&PageSource::Empty, false));
}

#[test]
fn should_restore_session_skipped_in_automation_mode() {
    assert!(!should_restore_session(&PageSource::Empty, true));
}

#[test]
fn should_restore_session_skipped_for_explicit_source() {
    assert!(!should_restore_session(&PageSource::AboutBlank, false));
    assert!(!should_restore_session(&PageSource::AboutBlank, true));
}

#[test]
fn parse_cli_no_args_is_empty_window() {
    assert!(matches!(
        parse_cli(&args(&[])),
        Ok(CliMode::OpenWindow(PageSource::Empty))
    ));
}

#[test]
fn parse_cli_single_target_is_window() {
    let cli = parse_cli(&args(&["samples/page.html"])).expect("ok");
    match cli {
        CliMode::OpenWindow(PageSource::File(p)) => {
            assert_eq!(p, PathBuf::from("samples/page.html"));
        }
        _ => panic!("expected OpenWindow(File)"),
    }
}

#[test]
fn parse_cli_single_url_is_window() {
    let cli = parse_cli(&args(&["https://example.com"])).expect("ok");
    assert!(matches!(
        cli,
        CliMode::OpenWindow(PageSource::Url(ref u)) if u == "https://example.com"
    ));
}

#[test]
fn parse_cli_dump_layout() {
    let cli = parse_cli(&args(&["--dump-layout", "samples/page.html"])).expect("ok");
    match cli {
        CliMode::Dump {
            source: PageSource::File(p),
            kind: DumpKind::Layout,
        } => assert_eq!(p, PathBuf::from("samples/page.html")),
        _ => panic!("expected Dump Layout File"),
    }
}

#[test]
fn parse_cli_dump_source_with_url() {
    let cli = parse_cli(&args(&["--dump-source", "https://example.com"])).expect("ok");
    assert!(matches!(
        cli,
        CliMode::Dump {
            source: PageSource::Url(ref u),
            kind: DumpKind::Source,
        } if u == "https://example.com"
    ));
}

#[test]
fn parse_cli_dump_display_list() {
    let cli = parse_cli(&args(&["--dump-display-list", "a.html"])).expect("ok");
    assert!(matches!(
        cli,
        CliMode::Dump {
            kind: DumpKind::DisplayList,
            ..
        }
    ));
}

#[test]
fn parse_cli_dump_flag_without_target_errors() {
    // --dump-X РІ РѕРґРёРЅРѕС‡РєСѓ вЂ” РЅРµС‚ С†РµР»Рё РґР»СЏ РїСЂРѕРіРѕРЅР° pipeline-Р°.
    let err = parse_cli(&args(&["--dump-layout"])).unwrap_err();
    assert!(err.contains("С‚СЂРµР±СѓРµС‚"), "got: {err}");
}

#[test]
fn parse_cli_unknown_flag_alone_errors() {
    let err = parse_cli(&args(&["--unknown"])).unwrap_err();
    assert!(err.contains("РЅРµРёР·РІРµСЃС‚РЅС‹Р№"), "got: {err}");
}

#[test]
fn parse_cli_two_args_first_is_target_errors() {
    // `lumen a.html b.html` вЂ” РјС‹ РЅРµ Р·РЅР°РµРј С‡С‚Рѕ РґРµР»Р°С‚СЊ; СЏРІРЅР°СЏ РѕС€РёР±РєР° Р»СѓС‡С€Рµ,
    // С‡РµРј В«РѕС‚РєСЂС‹С‚СЊ РїРµСЂРІС‹Р№, РїСЂРѕРёРіРЅРѕСЂРёСЂРѕРІР°С‚СЊ РІС‚РѕСЂРѕР№В».
    let err = parse_cli(&args(&["a.html", "b.html"])).unwrap_err();
    assert!(err.contains("РЅРµРёР·РІРµСЃС‚РЅС‹Р№"), "got: {err}");
}

#[test]
fn parse_cli_dump_flag_then_flag_errors() {
    // `lumen --dump-layout --dump-source` вЂ” РѕР±Р° С„Р»Р°Рі, target РЅРµС‚.
    let err =
        parse_cli(&args(&["--dump-layout", "--dump-source"])).unwrap_err();
    assert!(err.contains("РѕР¶РёРґР°Р»СЃСЏ"), "got: {err}");
}

#[test]
fn parse_cli_too_many_args_errors() {
    let err = parse_cli(&args(&["--dump-layout", "a.html", "b.html"])).unwrap_err();
    assert!(err.contains("РјРЅРѕРіРѕ"), "got: {err}");
}

// в”Ђв”Ђ extract_print_to_pdf в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn extract_print_to_pdf_basic() {
    let (output, rest) = extract_print_to_pdf(&args(&["--print-to-pdf", "out.pdf", "page.html"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("out.pdf")));
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_print_to_pdf_no_flag() {
    let (output, rest) = extract_print_to_pdf(&args(&["page.html"]));
    assert!(output.is_none());
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_print_to_pdf_with_url_source() {
    let (output, rest) = extract_print_to_pdf(&args(&["--print-to-pdf", "result.pdf", "https://example.com"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("result.pdf")));
    assert_eq!(rest, args(&["https://example.com"]));
}

#[test]
fn extract_print_to_pdf_combined_with_other_flags() {
    // --print-to-pdf coexists with other pre-extracted flags.
    let (output, rest) = extract_print_to_pdf(&args(&["--print-to-pdf", "a.pdf", "b.html"]));
    assert!(output.is_some());
    assert_eq!(rest, args(&["b.html"]));
}

// в”Ђв”Ђ extract_screenshot в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn extract_screenshot_basic() {
    let (output, rest) = extract_screenshot(&args(&["--screenshot", "out.png", "page.html"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("out.png")));
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_screenshot_no_flag() {
    let (output, rest) = extract_screenshot(&args(&["page.html"]));
    assert!(output.is_none());
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_screenshot_with_url_source() {
    let (output, rest) =
        extract_screenshot(&args(&["--screenshot", "shot.png", "https://example.com"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("shot.png")));
    assert_eq!(rest, args(&["https://example.com"]));
}

#[test]
fn extract_screenshot_only_first_flag_consumed() {
    // A second --screenshot stays in the rest args (only the first is taken).
    let (output, rest) =
        extract_screenshot(&args(&["--screenshot", "a.png", "--screenshot", "b.png"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("a.png")));
    assert_eq!(rest, args(&["--screenshot", "b.png"]));
}

// в”Ђв”Ђ extract_trace_nav (PERF-1) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn extract_trace_nav_basic() {
    let (output, rest) = extract_trace_nav(&args(&["--trace-nav", "out.json", "page.html"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("out.json")));
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_trace_nav_no_flag() {
    let (output, rest) = extract_trace_nav(&args(&["page.html"]));
    assert!(output.is_none());
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_trace_nav_with_url_source() {
    let (output, rest) =
        extract_trace_nav(&args(&["--trace-nav", "t.json", "https://example.com"]));
    assert_eq!(output.as_deref(), Some(std::path::Path::new("t.json")));
    assert_eq!(rest, args(&["https://example.com"]));
}

#[test]
fn extract_viewport_override_basic() {
    let (size, rest) = extract_viewport_override(&args(&["--viewport", "1024x720", "page.html"]));
    assert_eq!(size, Some((1024.0, 720.0)));
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_viewport_override_no_flag() {
    let (size, rest) = extract_viewport_override(&args(&["page.html"]));
    assert!(size.is_none());
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn extract_viewport_override_malformed_kept_in_rest() {
    let (size, rest) = extract_viewport_override(&args(&["--viewport", "bogus", "page.html"]));
    assert!(size.is_none());
    assert_eq!(rest, args(&["--viewport", "bogus", "page.html"]));
}

#[test]
fn extract_viewport_override_missing_value_kept_in_rest() {
    let (size, rest) = extract_viewport_override(&args(&["--viewport"]));
    assert!(size.is_none());
    assert_eq!(rest, args(&["--viewport"]));
}

#[test]
fn extract_viewport_override_only_first_flag_consumed() {
    let (size, rest) =
        extract_viewport_override(&args(&["--viewport", "800x600", "--viewport", "1x1"]));
    assert_eq!(size, Some((800.0, 600.0)));
    assert_eq!(rest, args(&["--viewport", "1x1"]));
}

#[test]
fn encode_images_as_pdf_empty() {
    let pdf = encode_images_as_pdf(&[], 100, 100);
    // Non-empty: at minimum the %PDF header.
    assert!(pdf.starts_with(b"%PDF-"));
}

#[test]
fn encode_images_as_pdf_single_page() {
    let img = lumen_image::Image {
        width: 2,
        height: 2,
        format: lumen_image::PixelFormat::Rgba8,
        data: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255],
        icc_profile: None,
    };
    let pdf = encode_images_as_pdf(&[img], 2, 2);
    assert!(pdf.starts_with(b"%PDF-"));
    // PDF objects contain binary + ASCII text вЂ” search raw bytes for key strings.
    let contains = |needle: &[u8]| pdf.windows(needle.len()).any(|w| w == needle);
    assert!(contains(b"/Page") || contains(b"/MediaBox"),
        "expected /Page or /MediaBox in PDF output (len={})", pdf.len());
}

// в”Ђв”Ђ Scroll-state helpers в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn clamp_scroll_inside_range() {
    assert_eq!(clamp_scroll(50.0, 100.0), 50.0);
    assert_eq!(clamp_scroll(0.0, 100.0), 0.0);
    assert_eq!(clamp_scroll(100.0, 100.0), 100.0);
}

#[test]
fn clamp_scroll_clamps_negative_to_zero() {
    assert_eq!(clamp_scroll(-5.0, 100.0), 0.0);
    assert_eq!(clamp_scroll(f32::NEG_INFINITY, 100.0), 0.0);
}

#[test]
fn clamp_scroll_clamps_overshoot_to_max() {
    assert_eq!(clamp_scroll(200.0, 100.0), 100.0);
    assert_eq!(clamp_scroll(f32::INFINITY, 100.0), 100.0);
}

#[test]
fn clamp_scroll_zero_max_keeps_at_zero() {
    // РљРѕРЅС‚РµРЅС‚ РїРѕРјРµС‰Р°РµС‚СЃСЏ РІ viewport вЂ” max_scroll = 0.
    assert_eq!(clamp_scroll(50.0, 0.0), 0.0);
    assert_eq!(clamp_scroll(-5.0, 0.0), 0.0);
}

#[test]
fn clamp_scroll_nan_defaults_to_zero() {
    assert_eq!(clamp_scroll(f32::NAN, 100.0), 0.0);
}

#[test]
fn cursor_icon_thumb_hover_is_pointer() {
    assert_eq!(
        cursor_icon_for_hover(scrollbar::TrackClick::Thumb, false),
        CursorIcon::Pointer
    );
}

#[test]
fn cursor_icon_track_above_is_default() {
    // Track-click С‚РѕР¶Рµ clickable (page-jump), РЅРѕ cursor-change РЅР° РїСѓСЃС‚РѕРј
    // track-Рµ Р±С‹Р» Р±С‹ С€СѓРјРЅС‹Рј вЂ” СЃС‚Р°РЅРґР°СЂС‚ РІСЃРµС… Р±СЂР°СѓР·РµСЂРѕРІ: С‚РѕР»СЊРєРѕ thumb.
    assert_eq!(
        cursor_icon_for_hover(scrollbar::TrackClick::Above, false),
        CursorIcon::Default
    );
}

#[test]
fn cursor_icon_track_below_is_default() {
    assert_eq!(
        cursor_icon_for_hover(scrollbar::TrackClick::Below, false),
        CursorIcon::Default
    );
}

#[test]
fn cursor_icon_off_scrollbar_is_default() {
    assert_eq!(
        cursor_icon_for_hover(scrollbar::TrackClick::None, false),
        CursorIcon::Default
    );
}

#[test]
fn cursor_icon_drag_active_overrides_hover() {
    // Р’Рѕ РІСЂРµРјСЏ drag-Р° cursor РґРѕР»Р¶РµРЅ В«РїСЂРёР»РёРїРЅСѓС‚СЊВ» Рє Pointer РЅРµР·Р°РІРёСЃРёРјРѕ
    // РѕС‚ С‚РµРєСѓС‰РµР№ РїРѕР·РёС†РёРё РєСѓСЂСЃРѕСЂР° вЂ” winit С€Р»С‘С‚ CursorMoved Р·Р° РїСЂРµРґРµР»Р°РјРё
    // РѕРєРЅР°, hover-РєР»Р°СЃСЃРёС„РёРєР°С‚РѕСЂ С‚Р°Рј РІРµСЂРЅС‘С‚ None, РЅРѕ drag-С„Р»Р°Рі РїРѕР±РµР¶РґР°РµС‚.
    assert_eq!(
        cursor_icon_for_hover(scrollbar::TrackClick::None, true),
        CursorIcon::Pointer
    );
    assert_eq!(
        cursor_icon_for_hover(scrollbar::TrackClick::Above, true),
        CursorIcon::Pointer
    );
}

#[test]
fn page_step_is_below_full_viewport() {
    // 90% РѕС‚ viewport-Р° вЂ” РѕСЃС‚Р°РІР»СЏРµС‚ overlap, С‡С‚РѕР±С‹ РїСЂРё PageDown РїРѕР»СЊР·РѕРІР°С‚РµР»СЊ
    // РЅРµ С‚РµСЂСЏР» РїРѕСЃР»РµРґРЅСЋСЋ СЃС‚СЂРѕРєСѓ РёР· РІРёРґР°.
    assert!((page_step(720.0) - 648.0).abs() < 0.01);
    assert!(page_step(720.0) < 720.0);
}

#[test]
fn content_height_empty_list_is_zero() {
    assert_eq!(content_height_of(&Vec::new()), 0.0);
}

#[test]
fn content_height_takes_max_bottom() {
    use lumen_core::geom::Rect;
    use lumen_layout::Color;
    use lumen_paint::DisplayCommand;
    let dl: lumen_paint::DisplayList = vec![
        DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        },
        DisplayCommand::FillRect {
            rect: Rect::new(0.0, 200.0, 100.0, 30.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        },
        DisplayCommand::FillRect {
            rect: Rect::new(0.0, 100.0, 100.0, 20.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        },
    ];
    // max(50, 230, 120) = 230
    assert!((content_height_of(&dl) - 230.0).abs() < 0.01);
}

#[test]
fn content_height_ignores_pop_commands() {
    use lumen_paint::DisplayCommand;
    let dl: lumen_paint::DisplayList = vec![
        DisplayCommand::PopClip,
        DisplayCommand::PopOpacity,
        DisplayCommand::PopBlendMode,
    ];
    assert_eq!(content_height_of(&dl), 0.0);
}

// в”Ђв”Ђ content_width_of в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn content_width_empty_list_is_zero() {
    assert_eq!(content_width_of(&Vec::new()), 0.0);
}

#[test]
fn content_width_takes_max_right() {
    use lumen_core::geom::Rect;
    use lumen_layout::Color;
    use lumen_paint::DisplayCommand;
    let dl: lumen_paint::DisplayList = vec![
        DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        },
        DisplayCommand::FillRect {
            rect: Rect::new(300.0, 0.0, 80.0, 20.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        },
        DisplayCommand::FillRect {
            rect: Rect::new(150.0, 0.0, 60.0, 10.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        },
    ];
    // max(100, 380, 210) = 380
    assert!((content_width_of(&dl) - 380.0).abs() < 0.01);
}

#[test]
fn content_width_ignores_pop_commands() {
    use lumen_paint::DisplayCommand;
    let dl: lumen_paint::DisplayList = vec![
        DisplayCommand::PopClip,
        DisplayCommand::PopOpacity,
        DisplayCommand::PopBlendMode,
    ];
    assert_eq!(content_width_of(&dl), 0.0);
}

// в”Ђв”Ђ Scroll-keybindings в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn keybinding_arrow_down_scrolls() {
    assert_eq!(
        keybinding_for(KeyCode::ArrowDown, ModifiersState::empty()),
        Some(KeyCommand::ScrollLineDown),
    );
    assert_eq!(
        keybinding_for(KeyCode::ArrowUp, ModifiersState::empty()),
        Some(KeyCommand::ScrollLineUp),
    );
}

#[test]
fn keybinding_arrow_right_left_scroll_horizontal() {
    assert_eq!(
        keybinding_for(KeyCode::ArrowRight, ModifiersState::empty()),
        Some(KeyCommand::ScrollLineRight),
    );
    assert_eq!(
        keybinding_for(KeyCode::ArrowLeft, ModifiersState::empty()),
        Some(KeyCommand::ScrollLineLeft),
    );
}

#[test]
fn keybinding_arrow_with_modifier_is_none() {
    // Ctrl+СЃС‚СЂРµР»РєР° РЅРµ РЅР°С€Р° вЂ” РѕСЃС‚Р°РІР»РµРЅРѕ РґР»СЏ РІРѕР·РјРѕР¶РЅРѕР№ РёРЅС‚РµРіСЂР°С†РёРё СЃ
    // word-wise navigation РІ Р±СѓРґСѓС‰РµРј (РєРѕРіРґР° РїРѕСЏРІРёС‚СЃСЏ omnibox).
    assert_eq!(
        keybinding_for(KeyCode::ArrowDown, ModifiersState::CONTROL),
        None,
    );
}

#[test]
fn keybinding_page_keys_scroll() {
    assert_eq!(
        keybinding_for(KeyCode::PageDown, ModifiersState::empty()),
        Some(KeyCommand::ScrollPageDown),
    );
    assert_eq!(
        keybinding_for(KeyCode::PageUp, ModifiersState::empty()),
        Some(KeyCommand::ScrollPageUp),
    );
}

#[test]
fn keybinding_space_scrolls_page() {
    assert_eq!(
        keybinding_for(KeyCode::Space, ModifiersState::empty()),
        Some(KeyCommand::ScrollPageDown),
    );
    assert_eq!(
        keybinding_for(KeyCode::Space, ModifiersState::SHIFT),
        Some(KeyCommand::ScrollPageUp),
    );
}

#[test]
fn keybinding_home_end_jump() {
    assert_eq!(
        keybinding_for(KeyCode::Home, ModifiersState::empty()),
        Some(KeyCommand::ScrollHome),
    );
    assert_eq!(
        keybinding_for(KeyCode::End, ModifiersState::empty()),
        Some(KeyCommand::ScrollEnd),
    );
}

// в”Ђв”Ђ script execution gate в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn collect_inline_scripts_finds_inline() {
    let doc = lumen_html_parser::parse(
        r#"<html><head></head><body><script>console.log(1);</script></body></html>"#,
    );
    let mut scripts = Vec::new();
    let mut mods = Vec::new();
    collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
    assert_eq!(scripts.len(), 1);
    assert!(scripts[0].contains("console.log"));
    assert!(mods.is_empty());
}

#[test]
fn collect_inline_scripts_skips_empty() {
    let doc = lumen_html_parser::parse(
        r#"<html><head></head><body><script>   </script></body></html>"#,
    );
    let mut scripts = Vec::new();
    let mut mods = Vec::new();
    collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
    assert!(scripts.is_empty());
    assert!(mods.is_empty());
}

#[test]
fn collect_inline_scripts_multiple() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><script>a=1;</script><script>b=2;</script></body></html>"#,
    );
    let mut scripts = Vec::new();
    let mut mods = Vec::new();
    collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
    assert_eq!(scripts.len(), 2);
    assert!(mods.is_empty());
}

#[test]
fn collect_inline_scripts_separates_modules() {
    let doc = lumen_html_parser::parse(
        r#"<html><body>
              <script>var x = 1;</script>
              <script type="module">export const y = 2;</script>
            </body></html>"#,
    );
    let mut scripts = Vec::new();
    let mut mods = Vec::new();
    collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
    assert_eq!(scripts.len(), 1, "classic script counted");
    assert_eq!(mods.len(), 1, "module script counted");
    assert!(scripts[0].contains("var x"));
    assert!(mods[0].contains("export const y"));
}
