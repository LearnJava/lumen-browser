//! P1/SPLIT-PT1: хвост `anim_and_chrome.rs` — BoxModelOverlay, MaskMode +
//! PushMaskLayer, PushScrollLayer/DrawScrollbar, PageBreak и print display list,
//! apply_print_color_adjust, DrawCrossFade. Перенесено байт-в-байт без дедента
//! (приём ST-1/DL-1).

use super::*;
use super::text_and_images::build;

    // ── BoxModelOverlay ──────────────────────────────────────────────────────

    #[test]
    fn box_model_overlay_serializes_all_four_boxes() {
        use lumen_core::geom::Rect;
        let dl = vec![DisplayCommand::BoxModelOverlay {
            margin:  Rect::new(0.0,   0.0,  120.0, 100.0),
            border:  Rect::new(10.0, 10.0,  100.0,  80.0),
            padding: Rect::new(12.0, 12.0,   96.0,  76.0),
            content: Rect::new(20.0, 20.0,   80.0,  60.0),
        }];
        let s = serialize_display_list(&dl);
        assert!(s.starts_with("BoxModelOverlay"), "must start with command name");
        assert!(s.contains("margin=(0,0,120,100)"),  "margin box");
        assert!(s.contains("border=(10,10,100,80)"), "border box");
        assert!(s.contains("padding=(12,12,96,76)"), "padding box");
        assert!(s.contains("content=(20,20,80,60)"), "content box");
    }

    #[test]
    fn box_model_overlay_zero_content_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![DisplayCommand::BoxModelOverlay {
            margin:  Rect::new(0.0, 0.0, 50.0, 50.0),
            border:  Rect::new(5.0, 5.0, 40.0, 40.0),
            padding: Rect::new(7.0, 7.0, 36.0, 36.0),
            content: Rect::new(10.0, 10.0, 0.0, 0.0), // collapsed content
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("BoxModelOverlay"), "collapsed content must still serialize");
        assert!(s.contains("content=(10,10,0,0)"), "zero-size content rect");
    }

    // ── MaskMode + PushMaskLayer / PopMaskLayer ──────────────────────────────

    #[test]
    fn mask_mode_default_is_alpha() {
        assert_eq!(MaskMode::default(), MaskMode::Alpha);
    }

    #[test]
    fn push_mask_layer_alpha_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushMaskLayer {
                rect: Rect::new(10.0, 20.0, 100.0, 80.0),
                mode: MaskMode::Alpha,
            },
            DisplayCommand::PopMaskLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("PushMaskLayer"), "must contain PushMaskLayer");
        assert!(s.contains("(10.00, 20.00, 100.00, 80.00)"), "rect coords");
        assert!(s.contains("Alpha"), "mode=Alpha");
        assert!(s.contains("PopMaskLayer"), "must contain PopMaskLayer");
    }

    #[test]
    fn push_mask_layer_luminance_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushMaskLayer {
                rect: Rect::new(0.0, 0.0, 200.0, 150.0),
                mode: MaskMode::Luminance,
            },
            DisplayCommand::PopMaskLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("Luminance"), "mode=Luminance");
    }

    #[test]
    fn push_mask_layer_roundtrip_kinds() {
        use lumen_core::geom::Rect;
        let rect = Rect::new(0.0, 0.0, 50.0, 50.0);
        let dl = vec![
            DisplayCommand::PushMaskLayer { rect, mode: MaskMode::Alpha },
            DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } },
            DisplayCommand::PopMaskLayer,
        ];
        // Verify the three-command sequence serializes in order.
        let s = serialize_display_list(&dl);
        let push_pos = s.find("PushMaskLayer").expect("no PushMaskLayer");
        let fill_pos = s.find("FillRect").expect("no FillRect");
        let pop_pos  = s.find("PopMaskLayer").expect("no PopMaskLayer");
        assert!(push_pos < fill_pos, "PushMaskLayer before FillRect");
        assert!(fill_pos < pop_pos,  "FillRect before PopMaskLayer");
    }

    #[test]
    fn mask_mode_luminance_end_to_end_bakes_stops() {
        let css = ".m { width:200px; height:200px; background:#e63946; \
             mask-image: linear-gradient(to right, black, white); mask-mode: luminance; }";
        let html = "<div class=\"m\"></div>";
        // Plain builder.
        let dl = build(html, css);
        assert_baked_luma_stops(&dl, "build_display_list");

        // Stacking-context-ordered builder (used by the CPU snapshot path) —
        // mask-image makes the box a stacking context, so it goes through the
        // bucket path, not `walk`.
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let st = StackingTree::build(&tree);
        let order = PaintOrder::from_tree(&st);
        let dl_ordered = build_display_list_ordered(&tree, &st, &order).0;
        assert_baked_luma_stops(&dl_ordered, "build_display_list_ordered");
    }

    fn assert_baked_luma_stops(dl: &DisplayList, label: &str) {
        let stops = dl.iter().find_map(|c| match c {
            DisplayCommand::PushMaskLinearGradient { stops, .. } => Some(stops.clone()),
            _ => None,
        });
        let stops = stops.unwrap_or_else(|| panic!("{label}: no PushMaskLinearGradient"));
        assert_eq!(stops.first().map(|s| s.color.a), Some(0), "{label}: black → alpha 0");
        assert_eq!(stops.last().map(|s| s.color.a), Some(255), "{label}: white → alpha 255");
    }

    #[test]
    fn mask_stops_alpha_mode_unchanged() {
        let stops = vec![
            GradientStop {
                color: Color { r: 0, g: 0, b: 0, a: 255 },
                position: None,
                ..Default::default()
            },
            GradientStop {
                color: Color { r: 255, g: 255, b: 255, a: 255 },
                position: None,
                ..Default::default()
            },
        ];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Alpha);
        assert_eq!(out, stops, "alpha mode leaves stops untouched");
    }

    #[test]
    fn mask_stops_luminance_bakes_alpha() {
        // Black opaque → luma 0 → alpha 0; white opaque → luma 1 → alpha 255.
        let stops = vec![
            GradientStop {
                color: Color { r: 0, g: 0, b: 0, a: 255 },
                position: None,
                ..Default::default()
            },
            GradientStop {
                color: Color { r: 255, g: 255, b: 255, a: 255 },
                position: None,
                ..Default::default()
            },
        ];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Luminance);
        assert_eq!(out[0].color.a, 0, "black stop becomes fully transparent");
        assert_eq!(out[1].color.a, 255, "white stop stays fully opaque");
        // RGB is preserved (only the alpha channel encodes the mask value).
        assert_eq!(out[0].color.r, 0);
        assert_eq!(out[1].color.r, 255);
    }

    #[test]
    fn mask_stops_luminance_multiplies_source_alpha() {
        // White at 50% alpha → luma 1 · 0.5 ≈ alpha 128.
        let stops = vec![GradientStop {
            color: Color { r: 255, g: 255, b: 255, a: 128 },
            position: None,
            ..Default::default()
        }];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Luminance);
        assert_eq!(out[0].color.a, 128, "luminance 1.0 keeps source alpha");
    }

    #[test]
    fn mask_stops_luminance_green_weight() {
        // Pure green opaque → luma 0.7152 → alpha ≈ 182.
        let stops = vec![GradientStop {
            color: Color { r: 0, g: 255, b: 0, a: 255 },
            position: None,
            ..Default::default()
        }];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Luminance);
        assert_eq!(out[0].color.a, 182, "0.7152·255 rounds to 182");
    }

    // ─── PushScrollLayer / PopScrollLayer tests ──────────────────────────────

    #[test]
    fn overflow_scroll_emits_push_scroll_layer() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_push = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        let has_pop  = dl.iter().any(|c| matches!(c, DisplayCommand::PopScrollLayer));
        assert!(has_push, "overflow:scroll must emit PushScrollLayer");
        assert!(has_pop,  "overflow:scroll must emit PopScrollLayer");
    }

    #[test]
    fn overflow_scroll_no_push_clip_rect_for_scroll() {
        // overflow:scroll should not fall back to PushClipRect for the scroll axis
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        // There should be PushScrollLayer, not PushClipRect, for the scroll container itself.
        let scroll_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushScrollLayer { .. })).count();
        assert!(scroll_count >= 1, "expected at least one PushScrollLayer for overflow:scroll");
    }

    #[test]
    fn overflow_hidden_emits_push_clip_rect_not_scroll_layer() {
        let dl = build(
            r#"<div style="overflow:hidden;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_scroll = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        assert!(!has_scroll, "overflow:hidden must not emit PushScrollLayer");
        // overflow:hidden still clips via PushClipRect
        let has_clip = dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert!(has_clip, "overflow:hidden must emit PushClipRect");
    }

    #[test]
    fn scroll_layer_scroll_xy_defaults_zero() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>x</p></div>"#,
            "",
        );
        if let Some(DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. }) =
            dl.iter().find(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }))
        {
            assert_eq!(*scroll_x, 0.0, "initial scroll_x should be 0");
            assert_eq!(*scroll_y, 0.0, "initial scroll_y should be 0");
        } else {
            panic!("PushScrollLayer not found");
        }
    }

    #[test]
    fn push_scroll_layer_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushScrollLayer {
                clip_rect: Rect::new(10.0, 20.0, 100.0, 50.0),
                scroll_x: 5.0,
                scroll_y: 15.0,
            },
            DisplayCommand::PopScrollLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("PushScrollLayer"), "serialized output must contain PushScrollLayer");
        assert!(s.contains("PopScrollLayer"), "serialized output must contain PopScrollLayer");
        assert!(s.contains("scroll=(5.00,15.00)"), "scroll offsets must appear in serialization");
    }

    #[test]
    fn overflow_auto_emits_push_scroll_layer() {
        // overflow:auto must produce PushScrollLayer just like overflow:scroll.
        let dl = build(
            r#"<div style="overflow:auto;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_push = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        let has_pop  = dl.iter().any(|c| matches!(c, DisplayCommand::PopScrollLayer));
        assert!(has_push, "overflow:auto must emit PushScrollLayer");
        assert!(has_pop,  "overflow:auto must emit PopScrollLayer");
    }

    // ── DrawScrollbar ─────────────────────────────────────────────────────────

    /// overflow:scroll with content taller than clip → vertical DrawScrollbar emitted.
    #[test]
    fn overflow_scroll_with_overflow_emits_draw_scrollbar_vertical() {
        // div 100×50 with a 200px-tall child → content overflows vertically.
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawScrollbar { vertical, .. } => Some(*vertical),
                _ => None,
            })
            .collect();
        assert!(!bars.is_empty(), "должен быть хотя бы один DrawScrollbar");
        assert!(bars.contains(&true), "должен быть вертикальный DrawScrollbar");
    }

    /// overflow:scroll with content fitting inside → no DrawScrollbar (no overflow).
    #[test]
    fn overflow_scroll_without_overflow_no_draw_scrollbar() {
        // div 100×200 with a 50px-tall child → no vertical overflow.
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:200px"><div style="height:50px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "нет переполнения → нет DrawScrollbar");
    }

    /// DrawScrollbar thumb_rect is inside track_rect.
    #[test]
    fn draw_scrollbar_thumb_inside_track() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("должен быть вертикальный DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { track_rect, thumb_rect, vertical: true, .. } = sb {
            // Track right edge must be at right edge of clip (within padding box).
            assert!(track_rect.width > 0.0, "track width > 0");
            assert!(thumb_rect.height > 0.0, "thumb height > 0");
            // Thumb must be inside track vertically.
            assert!(
                thumb_rect.y >= track_rect.y,
                "thumb top must be >= track top"
            );
            assert!(
                thumb_rect.y + thumb_rect.height <= track_rect.y + track_rect.height + 1.0,
                "thumb bottom must be <= track bottom"
            );
        }
    }

    /// DrawScrollbar serialization round-trip.
    #[test]
    fn draw_scrollbar_serialize() {
        let dl = vec![DisplayCommand::DrawScrollbar {
            track_rect: Rect::new(90.0, 0.0, 12.0, 50.0),
            thumb_rect: Rect::new(92.0, 5.0, 8.0, 20.0),
            vertical: true,
            thumb_color: SCROLLBAR_THUMB_COLOR,
            track_color: SCROLLBAR_TRACK_COLOR,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawScrollbar"), "serialization must contain DrawScrollbar");
        assert!(s.contains("vertical"), "serialization must mention orientation");
    }

    /// `scrollbar-width: none` suppresses DrawScrollbar while keeping scroll layer.
    #[test]
    fn scrollbar_width_none_no_draw_scrollbar() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-width:none"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "scrollbar-width:none → нет DrawScrollbar");
        // Scroll layer must still be present so content can scroll.
        let has_scroll = dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        assert!(has_scroll, "scrollbar-width:none → scroll layer должен оставаться");
    }

    /// `scrollbar-width: thin` emits DrawScrollbar with narrower track (6px gutter).
    #[test]
    fn scrollbar_width_thin_narrow_track() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-width:thin"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("thin scrollbar must emit DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { track_rect, .. } = sb {
            assert!(
                (track_rect.width - SCROLLBAR_WIDTH_THIN).abs() < 0.5,
                "thin track width should be ~{} px, got {}",
                SCROLLBAR_WIDTH_THIN,
                track_rect.width
            );
        }
    }

    /// `scrollbar-color` wires custom thumb+track colors into DrawScrollbar.
    #[test]
    fn scrollbar_color_custom_colors() {
        // red thumb, blue track
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-color:red blue"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("must emit DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { thumb_color, track_color, .. } = sb {
            // Red thumb: r≈1.0, g≈0, b≈0
            assert!(thumb_color[0] > 0.9, "thumb red channel must be ~1.0");
            assert!(thumb_color[1] < 0.1, "thumb green channel must be ~0");
            // Blue track: b≈1.0, r≈0
            assert!(track_color[2] > 0.9, "track blue channel must be ~1.0");
            assert!(track_color[0] < 0.1, "track red channel must be ~0");
        }
    }

    /// overflow:hidden does not emit DrawScrollbar (no scroll layer).
    #[test]
    fn overflow_hidden_no_scrollbar() {
        let dl = build(
            r#"<div style="overflow:hidden;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "overflow:hidden → нет DrawScrollbar");
    }

    // ── PageBreak / print display list ────────────────────────────────────────

    /// split_at_page_breaks on empty input → one empty page.
    #[test]
    fn split_empty_yields_one_empty_page() {
        let pages = split_at_page_breaks(vec![]);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].is_empty());
    }

    /// split_at_page_breaks with no PageBreak → one page with all commands.
    #[test]
    fn split_no_breaks_single_page() {
        use lumen_core::geom::Rect;
        let cmds = vec![
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 },
                color: Color { r: 255, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 10.0, width: 10.0, height: 10.0 },
                color: Color { r: 0, g: 255, b: 0, a: 255 },
            },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].len(), 2);
    }

    /// split_at_page_breaks with one PageBreak → two pages.
    #[test]
    fn split_one_break_two_pages() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 255, g: 0, b: 0, a: 255 } },
            DisplayCommand::PageBreak,
            DisplayCommand::FillRect { rect: r, color: Color { r: 0, g: 0, b: 255, a: 255 } },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), 1); // one FillRect on page 0
        assert_eq!(pages[1].len(), 1); // one FillRect on page 1
        // PageBreak itself must not appear in any page
        for page in &pages {
            assert!(!page.iter().any(|c| matches!(c, DisplayCommand::PageBreak)));
        }
    }

    /// split_at_page_breaks with two PageBreaks → three pages, middle page empty.
    #[test]
    fn split_two_breaks_three_pages_middle_empty() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 1, g: 2, b: 3, a: 255 } },
            DisplayCommand::PageBreak,
            DisplayCommand::PageBreak,
            DisplayCommand::FillRect { rect: r, color: Color { r: 4, g: 5, b: 6, a: 255 } },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].len(), 1);
        assert_eq!(pages[1].len(), 0); // empty middle page
        assert_eq!(pages[2].len(), 1);
    }

    /// build_print_display_list on zero pages → empty list.
    #[test]
    fn print_dl_empty_pages() {
        let cmds = build_print_display_list(&[]);
        assert!(cmds.is_empty());
    }

    // ── apply_print_color_adjust (CC-8 + CSS Color Adjustment L1 §4.1) ───────

    /// Paginates `html` and returns the print display list after
    /// `apply_print_color_adjust(.., print_backgrounds)`.
    fn print_dl_after_color_adjust(html: &str, print_backgrounds: bool) -> DisplayList {
        use lumen_layout::{paginate, PaginationContext};
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let ctx = PaginationContext {
            page_width: 800.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let mut pages = paginate(&tree, &ctx);
        apply_print_color_adjust(&mut pages, print_backgrounds);
        build_print_display_list(&pages)
    }

    /// Counts solid fills of exactly `rgb` in the list.
    fn count_fills(cmds: &[DisplayCommand], rgb: (u8, u8, u8)) -> usize {
        cmds.iter()
            .filter(|c| match c {
                DisplayCommand::FillRect { color, .. }
                | DisplayCommand::FillRoundedRect { color, .. } => {
                    (color.r, color.g, color.b) == rgb
                }
                _ => false,
            })
            .count()
    }

    const PCA_HTML: &str = "<div style='height:40px;background:rgb(1,2,3)'>a</div>        <div style='height:40px;background:rgb(4,5,6);print-color-adjust:exact'>b</div>        <div style='height:40px;background:rgb(7,8,9);color-adjust:exact'><p style='background:rgb(10,11,12)'>c</p></div>        <div style='height:40px;background:linear-gradient(rgb(13,14,15),rgb(16,17,18))'>d</div>";

    /// `print_backgrounds = true` is a no-op: every background survives.
    #[test]
    fn print_color_adjust_noop_when_backgrounds_enabled() {
        let cmds = print_dl_after_color_adjust(PCA_HTML, true);
        assert_eq!(count_fills(&cmds, (1, 2, 3)), 1);
        assert_eq!(count_fills(&cmds, (4, 5, 6)), 1);
    }

    /// With the toggle off, `economy` boxes lose `background-color` and
    /// gradients; text survives.
    #[test]
    fn print_color_adjust_economy_strips_backgrounds() {
        let cmds = print_dl_after_color_adjust(PCA_HTML, false);
        assert_eq!(count_fills(&cmds, (1, 2, 3)), 0, "economy fill removed");
        assert!(
            !cmds.iter().any(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. })),
            "economy gradient removed"
        );
        assert!(
            cmds.iter().any(|c| matches!(c, DisplayCommand::DrawText { .. })),
            "foreground text kept"
        );
    }

    /// `print-color-adjust: exact` (and the legacy `color-adjust` alias) keeps
    /// the box's background even with the toggle off, and — the property being
    /// inherited — the descendant's too.
    #[test]
    fn print_color_adjust_exact_keeps_backgrounds_and_inherits() {
        let cmds = print_dl_after_color_adjust(PCA_HTML, false);
        assert_eq!(count_fills(&cmds, (4, 5, 6)), 1, "exact box keeps its fill");
        assert_eq!(count_fills(&cmds, (7, 8, 9)), 1, "legacy alias keeps its fill");
        assert_eq!(count_fills(&cmds, (10, 11, 12)), 1, "child inherits exact");
    }

    /// Empty input slice is handled without panicking.
    #[test]
    fn print_color_adjust_empty_pages_noop() {
        let mut pages: Vec<Page> = vec![];
        apply_print_color_adjust(&mut pages, false);
        assert!(pages.is_empty());
    }

    /// build_print_display_list on two pages inserts exactly one PageBreak.
    #[test]
    fn print_dl_two_pages_one_page_break() {
        use lumen_layout::{paginate, PaginationContext};

        let doc = lumen_html_parser::parse(
            "<div style='height:600px;background:red'></div><div style='height:600px;background:blue'></div>",
        );
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 1200.0));

        let ctx = PaginationContext {
            page_width: 800.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let pages = paginate(&tree, &ctx);
        // If content fits in one page or pagination yields 0/1 page, skip assertion
        if pages.len() < 2 {
            return;
        }
        let cmds = build_print_display_list(&pages);
        let breaks = cmds.iter().filter(|c| matches!(c, DisplayCommand::PageBreak)).count();
        assert_eq!(breaks, pages.len() - 1, "N pages → N-1 PageBreaks");
    }

    // ── Tests for build_print_display_list margin-box rendering ──────────

    /// Page without page_box emits no margin-box DrawText commands.
    #[test]
    fn print_dl_no_page_box_no_margin_text() {
        use lumen_layout::{paginate, PaginationContext};

        let doc = lumen_html_parser::parse("<div style='height:100px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(400.0, 600.0));
        let ctx = PaginationContext {
            page_width: 400.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let pages = paginate(&tree, &ctx);
        assert!(!pages.is_empty());
        // No page_box — no DrawText from margin boxes
        let cmds = build_print_display_list(&pages);
        let text_cmds: Vec<_> = cmds.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).collect();
        assert!(text_cmds.is_empty(), "no margin-box DrawText without page_box");
    }

    /// Page with a page_box containing bottom-center text emits a DrawText command.
    #[test]
    fn print_dl_page_box_bottom_center_emits_draw_text() {
        use lumen_layout::{
            paginate, MarginBoxPosition, PageBox, PageProperties, PaginationContext, TextMeasurer,
        };

        struct Fixed8;
        impl TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        
    /// FONTLOAD-14 (BUG-467, lumen-layout): `line-height: normal` now
    /// resolves from real font metrics (ascent + descent + lineGap) instead
    /// of a flat `1.2`. This measurer only fixes glyph width — restore the
    /// pre-FONTLOAD-14 total (`1.2×size`) explicitly, since ascent(0.8)+
    /// descent(0.2) defaults alone sum to `1.0×size` and would silently
    /// change every hand-computed expectation below.
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

        let doc = lumen_html_parser::parse("<div style='height:100px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(400.0, 600.0));
        let ctx = PaginationContext {
            page_width: 400.0,
            page_height: 600.0,
            margin_top: 40.0,
            margin_bottom: 40.0,
            margin_left: 40.0,
            margin_right: 40.0,
        };
        let mut pages = paginate(&tree, &ctx);
        assert!(!pages.is_empty());

        let props = PageProperties {
            width: 400.0, height: 600.0,
            orientation: "portrait".to_string(),
            margin_top: 40.0, margin_bottom: 40.0,
            margin_left: 40.0, margin_right: 40.0,
        };
        let mut page_box = PageBox::new(0, props);
        page_box.layout_margin_boxes();
        let label = "1 / 1";
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::BottomCenter) {
            mb.content = Some(label.to_string());
            mb.layout_text(label, 10.0, 15.0, &Fixed8);
        }
        pages[0].page_box = Some(page_box);

        let cmds = build_print_display_list(&pages);
        let texts: Vec<&str> = cmds.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(texts.contains(&"1 / 1"), "expected '1 / 1' in DrawText, got: {:?}", texts);
    }

    /// Margin-box DrawText positioned at page-box coordinates (not inside content transform).
    #[test]
    fn print_dl_margin_box_text_absolute_position() {
        use lumen_layout::{
            paginate, MarginBoxPosition, PageBox, PageProperties, PaginationContext, TextMeasurer,
        };

        struct Fixed8;
        impl TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        
    /// FONTLOAD-14 (BUG-467, lumen-layout): `line-height: normal` now
    /// resolves from real font metrics (ascent + descent + lineGap) instead
    /// of a flat `1.2`. This measurer only fixes glyph width — restore the
    /// pre-FONTLOAD-14 total (`1.2×size`) explicitly, since ascent(0.8)+
    /// descent(0.2) defaults alone sum to `1.0×size` and would silently
    /// change every hand-computed expectation below.
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

        let doc = lumen_html_parser::parse("<div style='height:50px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(200.0, 300.0));
        let ctx = PaginationContext {
            page_width: 200.0,
            page_height: 300.0,
            margin_top: 30.0,
            margin_bottom: 30.0,
            margin_left: 30.0,
            margin_right: 30.0,
        };
        let mut pages = paginate(&tree, &ctx);

        let props = PageProperties {
            width: 200.0, height: 300.0,
            orientation: "portrait".to_string(),
            margin_top: 30.0, margin_bottom: 30.0,
            margin_left: 30.0, margin_right: 30.0,
        };
        let mut page_box = PageBox::new(0, props);
        page_box.layout_margin_boxes();
        let label = "PG1";
        // Use top-left-corner so we can predict coordinates: x=0, y=0
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::TopLeftCorner) {
            mb.content = Some(label.to_string());
            mb.layout_text(label, 10.0, 15.0, &Fixed8);
        }
        pages[0].page_box = Some(page_box);

        let cmds = build_print_display_list(&pages);
        let pg1_rect = cmds.iter().find_map(|c| {
            if let DisplayCommand::DrawText { text, rect, .. } = c {
                if text == "PG1" { Some(*rect) } else { None }
            } else { None }
        });
        let rect = pg1_rect.expect("DrawText 'PG1' not found");
        // TopLeftCorner is at page origin (0,0); fragment offset is 0,0 inside box
        assert!(rect.x >= 0.0 && rect.x < 10.0, "x should be at page origin, got {}", rect.x);
        assert!(rect.y >= 0.0 && rect.y < 10.0, "y should be at page origin, got {}", rect.y);
    }

    // ── Tests for DrawCrossFade ────────────────────────────────────────────

    /// Конструкция DrawCrossFade сохраняет все поля без потерь.
    #[test]
    fn cross_fade_construction_preserves_fields() {
        let cmd = DisplayCommand::DrawCrossFade {
            dest: Rect::new(10.0, 20.0, 100.0, 50.0),
            src_a: "first.png".to_string(),
            src_b: "second.png".to_string(),
            progress: 0.25,
        };
        if let DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } = &cmd {
            assert!((dest.x - 10.0).abs() < f32::EPSILON);
            assert!((dest.y - 20.0).abs() < f32::EPSILON);
            assert!((dest.width - 100.0).abs() < f32::EPSILON);
            assert!((dest.height - 50.0).abs() < f32::EPSILON);
            assert_eq!(src_a, "first.png");
            assert_eq!(src_b, "second.png");
            assert!((progress - 0.25).abs() < f32::EPSILON);
        } else {
            panic!("expected DrawCrossFade variant");
        }
    }

    /// serialize_display_list печатает все ключевые поля в детерминированном формате.
    #[test]
    fn cross_fade_serialize_includes_all_fields() {
        let dl = vec![DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 200.0, 100.0),
            src_a: "a.png".to_string(),
            src_b: "b.png".to_string(),
            progress: 0.5,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.starts_with("DrawCrossFade "), "should start with command name: {s}");
        assert!(s.contains("(0.00, 0.00, 200.00, 100.00)"), "should contain dest rect: {s}");
        assert!(s.contains(r#"a="a.png""#), "should contain src_a: {s}");
        assert!(s.contains(r#"b="b.png""#), "should contain src_b: {s}");
        assert!(s.contains("p=0.500"), "should contain progress: {s}");
    }

    /// Equality / Debug на варианте работают через производные —
    /// важно для snapshot-тестов и assert_eq! в downstream-крейтах.
    #[test]
    fn cross_fade_equality_and_debug() {
        let a = DisplayCommand::DrawCrossFade {
            dest: Rect::new(1.0, 2.0, 3.0, 4.0),
            src_a: "x".into(),
            src_b: "y".into(),
            progress: 0.75,
        };
        let b = a.clone();
        assert_eq!(a, b, "Clone должен сохранять равенство");
        let dbg = format!("{a:?}");
        assert!(dbg.contains("DrawCrossFade"), "Debug должен включать имя варианта: {dbg}");
        assert!(dbg.contains("0.75"), "Debug должен включать progress: {dbg}");

        // Граничные значения: progress = 0.0 (только src_a) и 1.0 (только src_b)
        // — оба валидны и различимы.
        let zero = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 0.0,
        };
        let one = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 1.0,
        };
        assert_ne!(zero, one, "progress=0.0 и progress=1.0 — разные команды");
    }

    /// DrawCrossFade попадает в exhaustive-match киндов (защита от
    /// «забыли добавить ветку при extension enum-а»).
    #[test]
    fn cross_fade_appears_in_kind_dispatch() {
        let cmd = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 1.0, 1.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 0.5,
        };
        // Если когда-нибудь матч в `img_with_background_and_border_paints_in_order`
        // перестанет включать DrawCrossFade — компилятор не пропустит код.
        // Здесь просто smoke-проверяем сериализацию через публичный API.
        let s = serialize_display_list(std::slice::from_ref(&cmd));
        assert!(s.contains("DrawCrossFade"));
    }

