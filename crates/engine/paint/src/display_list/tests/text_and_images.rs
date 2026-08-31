//! P1/SPLIT-DL6: голова `mod tests` из `display_list.rs` — cull_rect
//! (ADR-016)/writing-mode vertical/bg-repeat:round/canvas bitmap (BUG-099)/
//! line wrapping/inline-flow/text-decoration/border-рендеринг/`<img>`/
//! loading="lazy"/`<video>` тесты, плюс общие хелперы группы DL: `build`/
//! `Fixed8`/`fills`/`texts`/`images` (уже читались сиблинг-модулями через
//! `super::tests::*` — DL-2…DL-5).
//! Перенесено байт-в-байт из `display_list.rs` без дедента (приём ST-1/DL-1).
//! Последний оставшийся кусок инлайнового `mod tests { … }` — обёртка снята
//! из `display_list.rs` этим же срезом (была пуста после этого переноса).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-6).

    use super::*;
    use lumen_core::geom::Size;

    // ── ADR-016 M0.2: DisplayCommand::cull_rect ────────────────────────────
    #[test]
    fn cull_rect_leaf_returns_own_box() {
        let cmd = DisplayCommand::FillRect {
            rect: Rect::new(10.0, 20.0, 30.0, 40.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        };
        let r = cmd.cull_rect().expect("leaf must report a box");
        assert_eq!((r.x, r.y, r.width, r.height), (10.0, 20.0, 30.0, 40.0));
    }

    #[test]
    fn cull_rect_structural_is_none() {
        // Push/Pop must never be culled — they keep the render stack balanced.
        for cmd in [
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 1.0, 1.0) },
            DisplayCommand::PopClip,
            DisplayCommand::PushTransform { matrix: Mat4::translation_2d(0.0, 0.0) },
            DisplayCommand::PopTransform,
            DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
            DisplayCommand::PopOpacity,
            DisplayCommand::PopScrollLayer,
            DisplayCommand::PageBreak,
        ] {
            assert!(cmd.cull_rect().is_none(), "structural cmd must be un-cullable: {}", cmd.variant_name());
        }
    }

    // ── Ph3 writing-mode vertical, Срез 3: `text-orientation: mixed` split ──
    #[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
    #[test]
    fn split_mixed_runs_groups_consecutive_non_cjk_and_isolates_cjk() {
        let segs = split_mixed_runs("Hi日本Bye");
        let described: Vec<(bool, String)> = segs
            .into_iter()
            .map(|s| match s {
                MixedSegment::Cjk(ch) => (true, ch.to_string()),
                MixedSegment::Other(s) => (false, s),
            })
            .collect();
        assert_eq!(
            described,
            vec![
                (false, "Hi".to_string()),
                (true, "日".to_string()),
                (true, "本".to_string()),
                (false, "Bye".to_string()),
            ],
        );
    }

    #[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
    #[test]
    fn split_mixed_runs_pure_latin_is_one_segment() {
        let segs = split_mixed_runs("Hello, world!");
        assert_eq!(segs.len(), 1);
        assert!(matches!(&segs[0], MixedSegment::Other(s) if s == "Hello, world!"));
    }

    #[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
    #[test]
    fn split_mixed_runs_empty_text_is_empty() {
        assert!(split_mixed_runs("").is_empty());
    }

    // ── CSS Backgrounds L3 §3.4: `background-repeat: round` tile rescale ────
    #[cfg(any(feature = "backend-femtovg", feature = "cpu-render"))]
    #[test]
    fn bg_tile_geometry_round_rescales_tile_to_whole_count() {
        // A 30px tile in a 100px area: round(100/30) = 3 copies, so the tile is
        // stretched to 100/3 ≈ 33.33px on both axes (no clipped partial tile).
        let pos = ObjectPosition::background_initial();
        let (tw, th, x0, y0, rx, ry, sx, sy) = bg_tile_geometry(
            BackgroundSize::Auto,
            &pos,
            BackgroundRepeat::Round,
            30.0,
            30.0,
            100.0,
            100.0,
            0.0,
            0.0,
        );
        assert!((tw - 100.0 / 3.0).abs() < 1e-3, "tile_w rounded: {tw}");
        assert!((th - 100.0 / 3.0).abs() < 1e-3, "tile_h rounded: {th}");
        // Default (top-left) position → tiling starts flush at the area origin.
        assert!((x0 - 0.0).abs() < 1e-3, "tile_x_start: {x0}");
        assert!((y0 - 0.0).abs() < 1e-3, "tile_y_start: {y0}");
        assert!(rx && ry, "round repeats on both axes");
        // `round` steps by the rescaled tile size (no inter-tile gap).
        assert!((sx - tw).abs() < 1e-3 && (sy - th).abs() < 1e-3, "round step == tile");
    }

    // `round` must NOT rescale when the tile already fits a whole number of
    // times, and stays a plain repeat otherwise (regression guard vs `Repeat`).
    #[cfg(any(feature = "backend-femtovg", feature = "cpu-render"))]
    #[test]
    fn bg_tile_geometry_round_exact_fit_is_unchanged() {
        let pos = ObjectPosition::background_initial();
        let (tw, th, ..) = bg_tile_geometry(
            BackgroundSize::Auto,
            &pos,
            BackgroundRepeat::Round,
            25.0,
            50.0,
            100.0,
            100.0,
            0.0,
            0.0,
        );
        // 100/25 = 4 and 100/50 = 2 are already whole → tile size preserved.
        assert!((tw - 25.0).abs() < 1e-3, "tile_w: {tw}");
        assert!((th - 50.0).abs() < 1e-3, "tile_h: {th}");
    }

    // CSS Backgrounds L3 §3.4 — `space`: whole tiles pinned to both edges with
    // equal gaps between them, first tile at the area origin, step = tile + gap.
    #[test]
    fn space_axis_geometry_distributes_gaps_between_whole_tiles() {
        // 30px tile in a 100px area starting at x=10: floor(100/30) = 3 tiles,
        // leftover 100 - 90 = 10 split across 2 gaps → 5px each, step = 35px.
        let (start, step, repeat) = space_axis_geometry(10.0, 100.0, 30.0, 40.0);
        assert!((start - 10.0).abs() < 1e-3, "start pinned to origin: {start}");
        assert!((step - 35.0).abs() < 1e-3, "step = tile + gap: {step}");
        assert!(repeat, "≥2 tiles repeat");
    }

    // `space` with room for at most one whole tile falls back to `no-repeat`:
    // a single tile placed at the `position` offset, no repeat.
    #[test]
    fn space_axis_geometry_single_tile_honors_position() {
        // 60px tile in a 100px area: floor(100/60) = 1 → no repeat, position kept.
        let (start, step, repeat) = space_axis_geometry(10.0, 100.0, 60.0, 25.0);
        assert!((start - 35.0).abs() < 1e-3, "start = origin + pos_off: {start}");
        assert!((step - 60.0).abs() < 1e-3, "step == tile: {step}");
        assert!(!repeat, "single tile does not repeat");
    }

    // Exact fit (no leftover) → gap is zero, step equals the tile, tiles repeat.
    #[test]
    fn space_axis_geometry_exact_fit_has_no_gap() {
        // 25px tile in a 100px area: 4 tiles, leftover 0 → gap 0, step 25.
        let (start, step, repeat) = space_axis_geometry(0.0, 100.0, 25.0, 0.0);
        assert!((start - 0.0).abs() < 1e-3, "start: {start}");
        assert!((step - 25.0).abs() < 1e-3, "step: {step}");
        assert!(repeat, "4 tiles repeat");
    }

    #[test]
    fn cull_rect_outline_grows_by_offset_plus_width() {
        let cmd = DisplayCommand::DrawOutline {
            rect: Rect::new(100.0, 100.0, 50.0, 50.0),
            width: 4.0,
            style: OutlineStyle::Solid,
            color: Color { r: 0, g: 0, b: 0, a: 255 },
            offset: 6.0,
        };
        let r = cmd.cull_rect().unwrap();
        // grown by offset(6) + width(4) = 10 on every side.
        assert_eq!((r.x, r.y, r.width, r.height), (90.0, 90.0, 70.0, 70.0));
    }

    #[test]
    fn cull_rect_scrollbar_unions_track_and_thumb() {
        let cmd = DisplayCommand::DrawScrollbar {
            track_rect: Rect::new(200.0, 0.0, 12.0, 400.0),
            thumb_rect: Rect::new(200.0, 120.0, 12.0, 80.0),
            vertical: true,
            thumb_color: [0.0; 4],
            track_color: [0.0; 4],
        };
        let r = cmd.cull_rect().unwrap();
        assert_eq!((r.x, r.y, r.width, r.height), (200.0, 0.0, 12.0, 400.0));
    }

    /// BUG-175: inner border radius = outer − adjacent side width, floored at 0.
    /// Horizontal radii drop by the left/right border, vertical by top/bottom.
    #[test]
    fn inner_for_border_subtracts_side_widths() {
        let outer = CornerRadii {
            tl: 20.0, tl_y: 20.0, tr: 20.0, tr_y: 20.0,
            br: 20.0, br_y: 20.0, bl: 20.0, bl_y: 20.0,
        };
        // widths: [top, right, bottom, left]
        let inner = outer.inner_for_border([4.0, 8.0, 6.0, 2.0]);
        assert!((inner.tl - 18.0).abs() < 1e-4, "tl_x -= left(2)");
        assert!((inner.tl_y - 16.0).abs() < 1e-4, "tl_y -= top(4)");
        assert!((inner.tr - 12.0).abs() < 1e-4, "tr_x -= right(8)");
        assert!((inner.tr_y - 16.0).abs() < 1e-4, "tr_y -= top(4)");
        assert!((inner.br - 12.0).abs() < 1e-4, "br_x -= right(8)");
        assert!((inner.br_y - 14.0).abs() < 1e-4, "br_y -= bottom(6)");
        assert!((inner.bl - 18.0).abs() < 1e-4, "bl_x -= left(2)");
        assert!((inner.bl_y - 14.0).abs() < 1e-4, "bl_y -= bottom(6)");
    }

    /// Border thicker than the outer radius floors the inner radius at 0 (square
    /// inner corner), never negative.
    #[test]
    fn inner_for_border_floors_at_zero() {
        let outer = CornerRadii { tl: 4.0, tl_y: 4.0, ..Default::default() };
        let inner = outer.inner_for_border([10.0, 10.0, 10.0, 10.0]);
        assert_eq!(inner.tl, 0.0);
        assert_eq!(inner.tl_y, 0.0);
    }

    /// Corner-overlap clamp caps every radius at half the smaller box dimension.
    #[test]
    fn clamped_to_box_caps_at_half() {
        let r = CornerRadii { tl: 999.0, tl_y: 999.0, tr: 999.0, tr_y: 999.0,
            br: 999.0, br_y: 999.0, bl: 999.0, bl_y: 999.0 };
        let c = r.clamped_to_box(140.0, 44.0);
        // Uniform radii: §5.5 single factor reduces them to min(140/2, 44/2) = 22.
        assert!((c.tl - 22.0).abs() < 1e-4);
        assert!((c.br_y - 22.0).abs() < 1e-4);
    }

    #[test]
    fn clamped_to_box_preserves_wide_ellipse() {
        // BUG-198: an SVG <ellipse> mapped into a 240×90 box produces full-extent
        // elliptical corners (rx = 120, ry = 45). §5.5 leaves these untouched
        // (each edge's two radii sum exactly to the edge length), so the corner
        // stays a true ellipse — not a circle/stadium (old `min(w/2,h/2)` cap → 45).
        let r = CornerRadii {
            tl: 120.0, tl_y: 45.0, tr: 120.0, tr_y: 45.0,
            br: 120.0, br_y: 45.0, bl: 120.0, bl_y: 45.0,
        };
        let c = r.clamped_to_box(240.0, 90.0);
        assert!((c.tl - 120.0).abs() < 1e-4, "x-radius collapsed: {}", c.tl);
        assert!((c.tl_y - 45.0).abs() < 1e-4, "y-radius wrong: {}", c.tl_y);
        assert!((c.br - 120.0).abs() < 1e-4);
        assert!((c.br_y - 45.0).abs() < 1e-4);
    }

    /// Neutralise the UA `body { margin: 8px }` (HTML Rendering §14.3.3, BUG-204)
    /// so display-list coordinates reflect the element under test, not the body
    /// margin — exactly as a real page does with `* { margin: 0 }`.
    const BODY_RESET: &str = "body{margin:0}";

    pub(crate) fn build(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        build_display_list(&tree)
    }

    pub(crate) struct Fixed8;
    impl lumen_layout::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn build_wrapped(html: &str, css: &str, width: f32) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8);
        build_display_list(&tree)
    }

    pub(crate) fn fills(dl: &DisplayList) -> Vec<&Color> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn texts(dl: &DisplayList) -> Vec<&str> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    // ── BUG-099: `<canvas>` bitmap is a content-box-sized intrinsic size ─────

    /// Destination rect of the `canvas:{nid}` bitmap in `dl`, as `(x, y, w, h)`.
    fn canvas_bitmap_rect(dl: &DisplayList) -> (f32, f32, f32, f32) {
        dl.iter()
            .find_map(|c| match c {
                DisplayCommand::DrawImage { rect, src, .. } if src.starts_with("canvas:") => {
                    Some((rect.x, rect.y, rect.width, rect.height))
                }
                _ => None,
            })
            .expect("<canvas> must emit a DrawImage for its bitmap")
    }

    /// Border box of the single `DrawBorder` in `dl`, as `(x, y, w, h)`.
    fn only_border_rect(dl: &DisplayList) -> (f32, f32, f32, f32) {
        dl.iter()
            .find_map(|c| match c {
                DisplayCommand::DrawBorder { rect, .. } => {
                    Some((rect.x, rect.y, rect.width, rect.height))
                }
                _ => None,
            })
            .expect("bordered <canvas> must emit a DrawBorder")
    }

    /// HTML Rendering §15.4.1 does not map the `<canvas>` dimension attributes
    /// to the `width`/`height` properties — they are the intrinsic (content-box)
    /// size, so `box-sizing: border-box` must not eat the border. Edge puts the
    /// TEST-57 `c3` element at a 186×156 border box, not 180×150.
    #[test]
    fn canvas_intrinsic_size_survives_border_box_sizing() {
        let dl = build(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{box-sizing:border-box;margin:0;padding:0}canvas{border:3px solid #38bdf8}",
        );
        assert_eq!(only_border_rect(&dl), (0.0, 0.0, 186.0, 156.0));
    }

    /// The bitmap belongs in the content box: painting it at the border box
    /// slid every canvas drawing under the border by the border width.
    #[test]
    fn canvas_bitmap_is_painted_into_the_content_box() {
        let dl = build(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{box-sizing:border-box;margin:0;padding:0}canvas{border:3px solid #38bdf8}",
        );
        assert_eq!(canvas_bitmap_rect(&dl), (3.0, 3.0, 180.0, 150.0));
    }

    /// Padding counts towards the content box the same way the border does —
    /// under `content-box` sizing the border box grows instead of the bitmap
    /// shrinking.
    #[test]
    fn canvas_bitmap_content_box_accounts_for_padding() {
        let dl = build(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{margin:0}canvas{border:2px solid #38bdf8;padding:10px}",
        );
        assert_eq!(only_border_rect(&dl), (0.0, 0.0, 204.0, 174.0));
        assert_eq!(canvas_bitmap_rect(&dl), (12.0, 12.0, 180.0, 150.0));
    }

    /// An explicit CSS `width`/`height` still wins over the intrinsic size, and
    /// under `border-box` it keeps its border-box meaning — the bitmap is then
    /// stretched into whatever content box is left (`object-fit: fill`).
    #[test]
    fn canvas_explicit_css_size_keeps_border_box_meaning() {
        let dl = build(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{box-sizing:border-box;margin:0;padding:0}\
             canvas{border:3px solid #38bdf8;width:100px;height:80px}",
        );
        assert_eq!(only_border_rect(&dl), (0.0, 0.0, 100.0, 80.0));
        assert_eq!(canvas_bitmap_rect(&dl), (3.0, 3.0, 94.0, 74.0));
    }

    fn rounded_fills(dl: &DisplayList) -> Vec<&Color> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRoundedRect { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }

    /// True when `dl` contains a solid `FillRect` of exactly `(r,g,b)`.
    fn has_fill_rgb(dl: &DisplayList, r: u8, g: u8, b: u8) -> bool {
        fills(dl).iter().any(|c| c.r == r && c.g == g && c.b == b)
    }

    /// Number of `DrawBorder` commands with at least one non-zero width.
    fn border_count(dl: &DisplayList) -> usize {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { widths, .. } if widths.iter().any(|w| *w > 0.0)))
            .count()
    }

    // CSS Tables L2 §17.6.1.1 — `empty-cells`.
    // Two cells with distinct backgrounds + borders; the first is empty, the
    // second has text. Separated-borders model (default).
    const EMPTY_CELLS_HTML: &str =
        "<table><tr><td class=e></td><td class=f>x</td></tr></table>";
    const EMPTY_CELLS_CSS_BASE: &str = "td{width:40px;height:30px;border:2px solid #000} \
         .e{background:rgb(11,22,33)} .f{background:rgb(44,55,66)}";

    #[test]
    fn empty_cells_hide_suppresses_empty_cell_background() {
        let css = format!("table{{empty-cells:hide}} {EMPTY_CELLS_CSS_BASE}");
        let dl = build(EMPTY_CELLS_HTML, &css);
        assert!(!has_fill_rgb(&dl, 11, 22, 33), "empty cell bg must be hidden");
        assert!(has_fill_rgb(&dl, 44, 55, 66), "non-empty cell bg must stay");
    }

    #[test]
    fn empty_cells_hide_suppresses_empty_cell_border() {
        let css = format!("table{{empty-cells:hide}} {EMPTY_CELLS_CSS_BASE}");
        let dl = build(EMPTY_CELLS_HTML, &css);
        // Only the non-empty cell keeps its border (the table itself has none).
        assert_eq!(border_count(&dl), 1, "only the filled cell draws a border");
    }

    #[test]
    fn empty_cells_show_keeps_empty_cell_background() {
        // `show` is the initial value — both cells paint normally.
        let css = format!("table{{empty-cells:show}} {EMPTY_CELLS_CSS_BASE}");
        let dl = build(EMPTY_CELLS_HTML, &css);
        assert!(has_fill_rgb(&dl, 11, 22, 33), "empty cell bg shown under `show`");
        assert!(has_fill_rgb(&dl, 44, 55, 66));
        assert_eq!(border_count(&dl), 2, "both cells draw borders under `show`");
    }

    #[test]
    fn empty_cells_hide_ignored_under_border_collapse() {
        // Under `border-collapse: collapse`, `empty-cells` has no effect.
        let css = format!(
            "table{{empty-cells:hide;border-collapse:collapse}} {EMPTY_CELLS_CSS_BASE}"
        );
        let dl = build(EMPTY_CELLS_HTML, &css);
        assert!(
            has_fill_rgb(&dl, 11, 22, 33),
            "collapse model ignores empty-cells: empty cell bg stays"
        );
    }

    /// CSS UI L4 §6.1 — `accent-color` tints a checked checkbox indicator.
    #[test]
    fn checkbox_accent_color_tints_indicator() {
        let dl = build(
            "<input type=checkbox checked>",
            "input { accent-color: rgb(10, 200, 30); }",
        );
        let f = fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 10 && c.g == 200 && c.b == 30),
            "checkbox indicator should use accent-color, got {f:?}"
        );
    }

    /// `accent-color: auto` (the default) keeps the UA blue indicator.
    #[test]
    fn checkbox_default_accent_is_ua_blue() {
        let dl = build("<input type=checkbox checked>", "");
        let f = fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 21 && c.g == 90 && c.b == 192),
            "default checkbox indicator should be UA blue, got {f:?}"
        );
    }

    /// Radio dot also honours `accent-color`. The dot is a circle, so it is
    /// emitted as a `FillRoundedRect` (rounded_fills), not a square FillRect.
    #[test]
    fn radio_accent_color_tints_dot() {
        let dl = build(
            "<input type=radio checked>",
            "input { accent-color: rgb(200, 0, 100); }",
        );
        let f = rounded_fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 200 && c.g == 0 && c.b == 100),
            "radio dot should use accent-color, got {f:?}"
        );
    }

    /// A checked radio's dot renders as a circle: a `FillRoundedRect` whose
    /// corner radius is half the smaller side (fully rounded). A checked
    /// checkbox, by contrast, stays a square `FillRect`.
    #[test]
    fn radio_indicator_is_circle_checkbox_is_square() {
        let radio = build("<input type=radio checked>", "");
        let circle = radio.iter().any(|c| matches!(
            c,
            DisplayCommand::FillRoundedRect { rect, radii, .. }
                if (radii.tl - rect.width.min(rect.height) / 2.0).abs() < 0.5 && radii.tl > 0.0
        ));
        assert!(circle, "radio dot should be a fully-rounded rect (circle), got {radio:?}");

        let checkbox = build("<input type=checkbox checked>", "");
        let square = fills(&checkbox).iter().any(|c| c.r == 21 && c.g == 90 && c.b == 192);
        assert!(square, "checkbox indicator should remain a square FillRect, got {checkbox:?}");
    }

    /// HTML §4.10.5.1.15 — a color input paints its value as a swatch,
    /// independent of any author `background`. An explicit value is honoured.
    #[test]
    fn color_input_paints_value_swatch() {
        let dl = build(
            r##"<input type=color value="#ff0000">"##,
            "input { background: #00ff00; }",
        );
        let f = fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 255 && c.g == 0 && c.b == 0),
            "color input should paint its value (#ff0000) as swatch, got {f:?}"
        );
    }

    /// A color input with no `value` defaults to a black swatch.
    #[test]
    fn color_input_default_swatch_is_black() {
        let dl = build("<input type=color>", "");
        let f = fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 0 && c.g == 0 && c.b == 0),
            "default color input swatch should be black, got {f:?}"
        );
    }

    /// BUG-187 — a text input paints its `value` as static content so the field
    /// is not blank (matching Edge). The exact string is drawn as DrawText.
    #[test]
    fn text_input_paints_value() {
        let dl = build(r#"<input type=text value="hello">"#, "");
        assert!(
            texts(&dl).contains(&"hello"),
            "text input should paint its value, got {:?}",
            texts(&dl)
        );
    }

    /// An empty-value text input paints no value text (blank field).
    #[test]
    fn empty_text_input_paints_no_value() {
        let dl = build(r#"<input type=text value="">"#, "");
        assert!(
            texts(&dl).is_empty(),
            "empty text input should paint no value, got {:?}",
            texts(&dl)
        );
    }

    /// A password input masks every character with U+2022 BULLET rather than
    /// revealing the value.
    #[test]
    fn password_input_masks_value() {
        let dl = build(r#"<input type=password value="secret">"#, "");
        let t = texts(&dl);
        assert!(
            t.contains(&"\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"),
            "password should be masked with 6 bullets, got {t:?}"
        );
        assert!(
            !t.iter().any(|s| s.contains("secret")),
            "password plaintext must not be painted, got {t:?}"
        );
    }

    /// `<input type=submit>` with no `value` renders the UA default label.
    #[test]
    fn submit_input_default_label() {
        let dl = build("<input type=submit>", "");
        assert!(
            texts(&dl).contains(&"Submit"),
            "submit input should render default 'Submit' label, got {:?}",
            texts(&dl)
        );
    }

    /// An explicit `value` on a submit input overrides the default label.
    #[test]
    fn submit_input_value_label() {
        let dl = build(r#"<input type=submit value="Go">"#, "");
        let t = texts(&dl);
        assert!(t.contains(&"Go"), "submit should use its value as label, got {t:?}");
        assert!(!t.contains(&"Submit"), "default label must not appear, got {t:?}");
    }

    /// Number/email/search inputs paint their value text just like plain text.
    #[test]
    fn number_search_inputs_paint_value() {
        let num = build(r#"<input type=number value="42">"#, "");
        assert!(texts(&num).contains(&"42"), "number input should paint value");
        let search = build(r#"<input type=search value="query">"#, "");
        assert!(texts(&search).contains(&"query"), "search input should paint value");
    }

    /// BUG-187 — an empty text input paints its `placeholder` attribute as a
    /// grey hint (`#757575`), so the field is not blank (matching Edge).
    #[test]
    fn empty_text_input_paints_placeholder() {
        let dl = build(r#"<input type=text value="" placeholder="text input">"#, "");
        let hint = dl.iter().find_map(|c| match c {
            DisplayCommand::DrawText { text, color, .. } if text == "text input" => Some(color),
            _ => None,
        });
        let color = hint.expect("placeholder text should be painted");
        assert_eq!(
            (color.r, color.g, color.b),
            (0x75, 0x75, 0x75),
            "placeholder should be grey, got {color:?}"
        );
    }

    /// CSS Pseudo-Elements L4 §4.10 — `input::placeholder { color: ... }`
    /// overrides the UA default grey hint.
    #[test]
    fn placeholder_pseudo_element_overrides_color() {
        let dl = build(
            r#"<input type=text value="" placeholder="text input">"#,
            "input::placeholder { color: #ff0000; }",
        );
        let hint = dl.iter().find_map(|c| match c {
            DisplayCommand::DrawText { text, color, .. } if text == "text input" => Some(color),
            _ => None,
        });
        let color = hint.expect("placeholder text should be painted");
        assert_eq!(
            (color.r, color.g, color.b),
            (0xff, 0, 0),
            "::placeholder color override should apply, got {color:?}"
        );
    }

    /// No `::placeholder` rule → the UA default grey hint is unaffected.
    #[test]
    fn placeholder_without_rule_keeps_ua_default_grey() {
        let dl = build(
            r#"<input type=text value="" placeholder="text input">"#,
            "input { color: blue; }",
        );
        let hint = dl.iter().find_map(|c| match c {
            DisplayCommand::DrawText { text, color, .. } if text == "text input" => Some(color),
            _ => None,
        });
        let color = hint.expect("placeholder text should be painted");
        assert_eq!(
            (color.r, color.g, color.b),
            (0x75, 0x75, 0x75),
            "no ::placeholder rule → UA default grey, got {color:?}"
        );
    }

    /// BUG-187 — a placeholder is only shown while the value is empty; a filled
    /// input paints its value and never the placeholder.
    #[test]
    fn filled_input_paints_value_not_placeholder() {
        let dl = build(r#"<input type=text value="typed" placeholder="hint">"#, "");
        let t = texts(&dl);
        assert!(t.contains(&"typed"), "filled input paints its value, got {t:?}");
        assert!(!t.contains(&"hint"), "placeholder must be hidden when value set, got {t:?}");
    }

    /// BUG-187 — a checked checkbox draws a white tick (a `DrawSvgPath`
    /// triangle soup) on top of the accent-filled box, matching the native widget.
    #[test]
    fn checked_checkbox_paints_white_tick() {
        let dl = build("<input type=checkbox checked>", "");
        let tick = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::DrawSvgPath { vertices, color }
                if !vertices.is_empty() && color.r == 255 && color.g == 255 && color.b == 255
        ));
        assert!(tick, "checked checkbox should draw a white tick, got {dl:?}");
    }

    /// An unchecked checkbox draws neither the accent fill nor the tick.
    #[test]
    fn unchecked_checkbox_paints_no_tick() {
        let dl = build("<input type=checkbox>", "");
        let tick = dl.iter().any(|c| matches!(c, DisplayCommand::DrawSvgPath { .. }));
        assert!(!tick, "unchecked checkbox must not draw a tick, got {dl:?}");
        assert!(
            !fills(&dl).iter().any(|c| c.r == 21 && c.g == 90 && c.b == 192),
            "unchecked checkbox must not paint the accent fill"
        );
    }

    /// BUG-187 — a checked radio draws a white centre dot (a fully-rounded
    /// `FillRoundedRect`) on top of the accent-filled disc.
    #[test]
    fn checked_radio_paints_white_center_dot() {
        let dl = build("<input type=radio checked>", "");
        let white_dot = rounded_fills(&dl)
            .iter()
            .any(|c| c.r == 255 && c.g == 255 && c.b == 255);
        assert!(white_dot, "checked radio should draw a white centre dot, got {dl:?}");
    }

    /// `<progress>` fill bar uses `accent-color` (a rounded-rect fill).
    #[test]
    fn progress_accent_color_tints_bar() {
        let dl = build(
            "<progress value=0.5 max=1></progress>",
            "progress { accent-color: rgb(7, 130, 240); }",
        );
        let f = rounded_fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 7 && c.g == 130 && c.b == 240),
            "progress bar should use accent-color, got {f:?}"
        );
    }

    /// `<input type=range>` filled track + thumb use `accent-color`; the gray
    /// background track is left untinted.
    #[test]
    fn range_accent_color_tints_fill_not_track() {
        let dl = build(
            "<input type=range value=50 min=0 max=100>",
            "input { accent-color: rgb(240, 60, 8); }",
        );
        let f = rounded_fills(&dl);
        assert!(
            f.iter().any(|c| c.r == 240 && c.g == 60 && c.b == 8),
            "range fill/thumb should use accent-color, got {f:?}"
        );
        assert!(
            f.iter().any(|c| c.r == 200 && c.g == 200 && c.b == 200),
            "range background track should stay gray, got {f:?}"
        );
    }

    /// CSS Basic UI L4 §4.2 — `appearance: none` removes the native checkbox
    /// tick: no UA-blue indicator fill is emitted.
    #[test]
    fn appearance_none_suppresses_checkbox_indicator() {
        let dl = build(
            "<input type=checkbox checked>",
            "input { appearance: none; }",
        );
        let f = fills(&dl);
        assert!(
            !f.iter().any(|c| c.r == 21 && c.g == 90 && c.b == 192),
            "appearance:none must suppress the checkbox indicator, got {f:?}"
        );
    }

    /// `appearance: none` also suppresses a custom `accent-color` indicator —
    /// the author opted out of the native control entirely.
    #[test]
    fn appearance_none_suppresses_accent_indicator() {
        let dl = build(
            "<input type=checkbox checked>",
            "input { appearance: none; accent-color: rgb(10, 200, 30); }",
        );
        let f = fills(&dl);
        assert!(
            !f.iter().any(|c| c.r == 10 && c.g == 200 && c.b == 30),
            "appearance:none must suppress even an accent-tinted indicator, got {f:?}"
        );
    }

    /// `appearance: none` removes the native `<progress>` bar (no rounded fill).
    #[test]
    fn appearance_none_suppresses_progress_bar() {
        let dl = build(
            "<progress value=0.5 max=1></progress>",
            "progress { appearance: none; }",
        );
        assert!(
            rounded_fills(&dl).is_empty(),
            "appearance:none must suppress the progress bar, got {:?}",
            rounded_fills(&dl)
        );
    }

    /// `appearance: none` removes the native range slider track and thumb.
    #[test]
    fn appearance_none_suppresses_range_slider() {
        let dl = build(
            "<input type=range value=50 min=0 max=100>",
            "input { appearance: none; }",
        );
        assert!(
            rounded_fills(&dl).is_empty(),
            "appearance:none must suppress the range slider, got {:?}",
            rounded_fills(&dl)
        );
    }

    /// BUG-225 — `appearance: none` must NOT suppress a text input's `value`
    /// text: the value is author content, not a UA primitive. Only the native
    /// primitives (tick/dot/slider/bar/arrow/swatch) are removed.
    #[test]
    fn appearance_none_keeps_text_input_value() {
        let dl = build(
            r#"<input type=text value="typed">"#,
            "input { appearance: none; }",
        );
        assert!(
            texts(&dl).contains(&"typed"),
            "appearance:none text input must still paint its value, got {:?}",
            texts(&dl)
        );
    }

    /// BUG-225 — `appearance: none` must keep the placeholder hint of an empty
    /// text input (placeholder is author content, not a UA primitive).
    #[test]
    fn appearance_none_keeps_text_input_placeholder() {
        let dl = build(
            r#"<input type=text value="" placeholder="hint here">"#,
            "input { appearance: none; }",
        );
        assert!(
            texts(&dl).contains(&"hint here"),
            "appearance:none empty input must still paint its placeholder, got {:?}",
            texts(&dl)
        );
    }

    /// BUG-225 — `appearance: none` must keep a button's label.
    #[test]
    fn appearance_none_keeps_button_label() {
        let dl = build(
            r#"<input type=submit value="Send">"#,
            "input { appearance: none; }",
        );
        assert!(
            texts(&dl).contains(&"Send"),
            "appearance:none button must still paint its label, got {:?}",
            texts(&dl)
        );
    }

    /// BUG-225 — `appearance: none` keeps the `<select>` selected option label
    /// but drops the native dropdown arrow (▼).
    #[test]
    fn appearance_none_keeps_select_label_drops_arrow() {
        let dl = build(
            "<select><option selected>Chosen</option></select>",
            "select { appearance: none; }",
        );
        let t = texts(&dl);
        assert!(
            t.contains(&"Chosen"),
            "appearance:none select must still paint the selected label, got {t:?}"
        );
        assert!(
            !t.iter().any(|s| s.contains('\u{25BC}')),
            "appearance:none select must drop the native dropdown arrow, got {t:?}"
        );
    }

    #[test]
    fn empty_input_empty_list() {
        let dl = build("", "");
        assert!(dl.is_empty());
    }

    #[test]
    fn block_with_background_emits_fill() {
        let dl = build("<p>x</p>", "p { background: red; }");
        let f = fills(&dl);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].r, 255);
    }

    #[test]
    fn block_without_background_no_fill() {
        let dl = build("<p>x</p>", "");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn text_node_emits_draw_text() {
        let dl = build("<p>hello</p>", "");
        assert_eq!(texts(&dl), vec!["hello"]);
    }

    #[test]
    fn cyrillic_text_preserved() {
        let dl = build("<p>Привет, мир</p>", "");
        assert_eq!(texts(&dl), vec!["Привет, мир"]);
    }

    #[test]
    fn nested_backgrounds_in_parent_then_child_order() {
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } p { background: blue; }",
        );
        let f = fills(&dl);
        assert_eq!(f.len(), 2);
        // Сначала parent (под текст), потом child — естественный paint-порядок.
        assert_eq!(f[0].r, 255);
        assert_eq!(f[1].b, 255);
    }

    #[test]
    fn transparent_background_omitted() {
        let dl = build("<p>x</p>", "p { background-color: transparent; }");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn skipped_boxes_emit_nothing() {
        let dl = build("<p>x</p><!-- comment --><p>y</p>", "");
        // Только два текстовых узла; комментарий не даёт команды.
        assert_eq!(texts(&dl).len(), 2);
    }

    #[test]
    fn display_none_emits_nothing() {
        let dl = build(
            r#"<p class="x">hidden</p><p>visible</p>"#,
            ".x { display: none; }",
        );
        assert_eq!(texts(&dl), vec!["visible"]);
    }

    // ── Тесты line wrapping ─────────────────────────────────────────────────

    /// При переносе текста на 2 строки должны быть эмитированы 2 DrawText.
    #[test]
    fn wrapped_text_emits_multiple_draw_text() {
        // "hello world" = 11×8 = 88px. Viewport 60px → перенос на 2 строки.
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        assert_eq!(texts(&dl), vec!["hello", "world"]);
    }

    /// Вторая строка у `DrawText` должна быть смещена по Y на line_height.
    #[test]
    fn wrapped_lines_have_correct_y_offset() {
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        let draw_texts: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(draw_texts.len(), 2);
        let line_h = 16.0_f32 * 1.2; // font_size=16, line_height=1.2 → 19.2
        // CSS 2.1 §10.8.1: the first line carries half-leading = (19.2-16)/2 = 1.6.
        let half_leading = (line_h - 16.0) / 2.0;
        assert!((draw_texts[0].y - half_leading).abs() < 0.01, "y0={}", draw_texts[0].y);
        assert!((draw_texts[1].y - (half_leading + line_h)).abs() < 0.1, "y1={}", draw_texts[1].y);
    }

    /// Текст без переноса всё равно рисуется одной командой.
    #[test]
    fn no_wrap_single_draw_text() {
        let dl = build_wrapped("<p>hi</p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hi"]);
    }

    /// BUG-432: `::first-line { background }` рисуется под первой строкой.
    /// CSS Pseudo-elements L4 §4.1 — background применяется к `::first-line`,
    /// но `emit_inline_run` рисовал только текст.
    #[test]
    fn first_line_background_paints_under_first_line() {
        // "hello world" = 11×8 = 88px, viewport 60px → перенос на 2 строки.
        let dl = build_wrapped(
            "<p>hello world</p>",
            "p::first-line { background: #336699 }",
            60.0,
        );
        let bg = Color { r: 0x33, g: 0x66, b: 0x99, a: 255 };
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::FillRect { rect, color } if *color == bg => Some(*rect),
                _ => None,
            })
            .expect("::first-line background must emit a FillRect");
        // §4.1: фиктивный inline-тег оборачивает содержимое строки, поэтому
        // ширина = экстент первой строки ("hello" = 40px), а не всего блока.
        assert!((rect.width - 40.0).abs() < 1.0, "width={} (line extent, not 60)", rect.width);
        assert!(rect.height > 0.0, "height={}", rect.height);
        // Фон — под текстом: FillRect идёт до первого DrawText.
        let fill_at = dl.iter().position(|c| matches!(c, DisplayCommand::FillRect { color, .. } if *color == bg));
        let text_at = dl.iter().position(|c| matches!(c, DisplayCommand::DrawText { .. }));
        assert!(fill_at < text_at, "fill={fill_at:?} text={text_at:?}");
    }

    /// Обратная сторона BUG-432: обычный анонимный `InlineRun` фон не рисует.
    /// Риск фикса был именно в этом — покраска всех inline-ранов подряд.
    #[test]
    fn inline_run_without_first_line_rule_paints_no_background() {
        let dl = build_wrapped("<p>hello world</p>", "p { background: #336699 }", 60.0);
        let bg = Color { r: 0x33, g: 0x66, b: 0x99, a: 255 };
        // Ровно один FillRect этого цвета — собственный фон блока `<p>`,
        // а не второй такой же от его анонимного inline-рана.
        let n = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if *color == bg))
            .count();
        assert_eq!(n, 1, "block background must not be repeated by its inline run");
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// Текст с <span> внутри — один DrawText (одинаковый стиль → фрагменты сливаются).
    #[test]
    fn inline_same_style_merges_into_one_draw_text() {
        let dl = build_wrapped("<p>hello <span>world</span></p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hello world"]);
    }

    /// <a> с цветом → два DrawText: "Hello" и "link" с разными цветами.
    #[test]
    fn inline_different_style_emits_separate_draw_texts() {
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let t = texts(&dl);
        assert_eq!(t, vec!["Hello", "link"]);
        // Второй DrawText должен быть синим.
        let blue_cmds: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } if text == "link" => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(blue_cmds.len(), 1);
        assert_eq!(blue_cmds[0].b, 255);
    }

    /// X-координата второго фрагмента должна быть правее первого.
    #[test]
    fn inline_fragments_have_increasing_x() {
        // "Hello" (5*8=40) + space(8) + "link" → link начинается в x=48.
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].x - 0.0).abs() < 0.01, "Hello должно быть в x=0");
        assert!(
            rects[1].x > rects[0].x,
            "link должно быть правее: Hello.x={}, link.x={}",
            rects[0].x,
            rects[1].x
        );
    }

    // ── Тесты text-decoration ───────────────────────────────────────────────

    fn fill_rects(dl: &DisplayList) -> Vec<&Rect> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { rect, .. } => Some(rect),
                _ => None,
            })
            .collect()
    }

    /// `<a>` с `text-decoration: underline` эмитирует и DrawText, и FillRect.
    #[test]
    fn underline_emits_draw_text_and_fill_rect() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        assert_eq!(texts(&dl), vec!["link"]);
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1, "expected one underline FillRect");
        // "link" = 4×8 = 32px.
        assert!((rects[0].width - 32.0).abs() < 0.01, "width={}", rects[0].width);
    }

    /// Underline должен идти ниже baseline (под глифами).
    #[test]
    fn underline_positioned_below_baseline() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // First line gets half-leading = (19.2-16)/2 = 1.6 (CSS 2.1 §10.8.1),
        // baseline ≈ 1.6 + 16*0.80 = 14.4, underline y ≈ 14.4 + 16*0.10 = 16.0.
        assert!(
            (rects[0].y - 16.0).abs() < 0.5,
            "underline y should be near 16.0, got {}",
            rects[0].y
        );
    }

    /// line-through лежит выше baseline, не ниже.
    #[test]
    fn line_through_positioned_above_baseline() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "span { text-decoration: line-through; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // baseline ≈ 1.6 (half-leading) + 12.8 = 14.4, line-through y ≈ 14.4 - 16*0.30 = 9.6.
        assert!(
            (rects[0].y - 9.6).abs() < 0.5,
            "line-through y should be near 9.6, got {}",
            rects[0].y
        );
    }

    /// overline лежит над текстом.
    #[test]
    fn overline_positioned_above_text() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "span { text-decoration: overline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // baseline ≈ 1.6 (half-leading) + 12.8 = 14.4, overline y ≈ 14.4 - 16*0.78 ≈ 1.9.
        assert!(
            rects[0].y < 2.5,
            "overline y should be near top, got {}",
            rects[0].y
        );
    }

    /// `text-decoration: underline line-through` эмитирует две линии.
    #[test]
    fn multiple_decorations_emit_multiple_rects() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { text-decoration: underline line-through; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 2, "expected underline + line-through rects");
    }

    /// text-decoration-color: explicit — линия использует его, не цвет текста.
    #[test]
    fn decoration_explicit_color_overrides_text_color() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { color: red; text-decoration: underline; text-decoration-color: blue; }",
            800.0,
        );
        let colors: Vec<Color> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 1);
        assert_eq!([colors[0].r, colors[0].g, colors[0].b], [0, 0, 255]);
    }

    /// Цвет линии совпадает с цветом текста (currentColor).
    #[test]
    fn decoration_uses_text_color() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { color: red; text-decoration: underline; }",
            800.0,
        );
        let colors: Vec<&Color> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].r, 255);
        assert_eq!(colors[0].g, 0);
    }

    /// Соседние фрагменты разной декорации не сливаются.
    #[test]
    fn fragments_with_different_decoration_dont_merge() {
        let dl = build_wrapped(
            "<p>plain <a>underlined</a> tail</p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        let t = texts(&dl);
        // 3 фрагмента: "plain", "underlined", "tail".
        assert_eq!(t, vec!["plain", "underlined", "tail"]);
        // Underline только под средним.
        assert_eq!(fill_rects(&dl).len(), 1);
    }

    /// Унаследованная декорация продолжает работать у потомков.
    #[test]
    fn decoration_inherits_into_descendants() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "p { text-decoration: underline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        // Span наследует underline → FillRect эмитится.
        assert!(!rects.is_empty(), "underline should propagate to span");
    }

    /// `text-decoration: none` на потомке отменяет наследуемую декорацию.
    #[test]
    fn none_on_descendant_overrides_inherited_underline() {
        let dl = build_wrapped(
            "<p><a>off</a></p>",
            "p { text-decoration: underline; } a { text-decoration: none; }",
            800.0,
        );
        assert!(fill_rects(&dl).is_empty(), "a should override underline");
    }

    /// `text-decoration: underline solid` — sanity, что explicit Solid ведёт
    /// себя как default (один FillRect).
    #[test]
    fn style_solid_emits_one_rect() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline solid; }",
            800.0,
        );
        assert_eq!(fill_rects(&dl).len(), 1);
    }

    /// `Double` — две параллельные линии той же ширины с gap = thickness;
    /// второй rect ниже первого на `2 × thickness`.
    #[test]
    fn style_double_emits_two_parallel_rects() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline double; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 2, "Double = two parallel lines");
        assert!((rects[0].width - rects[1].width).abs() < 0.01);
        let t = (16.0_f32 * 0.07).max(1.0);
        let dy = rects[1].y - rects[0].y;
        assert!(
            (dy - 2.0 * t).abs() < 0.05,
            "expected dy ≈ 2·t = {}, got {dy}",
            2.0 * t
        );
    }

    /// Двойной underline + line-through → 4 rect-а суммарно.
    #[test]
    fn double_with_multiple_lines_emits_four_rects() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline line-through double; }",
            800.0,
        );
        assert_eq!(fill_rects(&dl).len(), 4);
    }

    /// `Dotted` — серия квадратиков `thickness × thickness`, count > 5
    /// для текста шириной 80px (10 символов × 8px char-width).
    #[test]
    fn style_dotted_emits_square_dots() {
        let dl = build_wrapped(
            "<p><a>longertext</a></p>",
            "a { text-decoration: underline dotted; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() > 5, "got {} dots, expected many", rects.len());
        // Каждый dot — квадрат width = height = thickness.
        let t = (16.0_f32 * 0.07).max(1.0);
        for r in &rects {
            assert!(
                (r.width - r.height).abs() < 0.01,
                "dot not square: {}×{}",
                r.width,
                r.height
            );
            assert!(
                (r.width - t).abs() < 0.01,
                "dot width={}, expected t={t}",
                r.width
            );
        }
    }

    /// `Dashed` — серия штрихов длиной `2 × thickness`, count > 3.
    #[test]
    fn style_dashed_emits_dashes() {
        // skip-ink: none disables the default skip-ink behaviour so the dashed
        // pattern is continuous and individual dash widths are predictable.
        let dl = build_wrapped(
            "<p><a>longertext</a></p>",
            "a { text-decoration: underline dashed; text-decoration-skip-ink: none; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() > 3, "got {} dashes", rects.len());
        let t = (16.0_f32 * 0.07).max(1.0);
        // Все dashes кроме, возможно, последнего — длиной 2·t.
        // Высота — thickness.
        for r in &rects[..rects.len() - 1] {
            assert!(
                (r.width - 2.0 * t).abs() < 0.05,
                "dash width={}, expected {}",
                r.width,
                2.0 * t
            );
            assert!((r.height - t).abs() < 0.01);
        }
    }

    /// `Wavy` эмитит серию тонких axis-aligned столбцов, аппроксимирующих
    /// синусоиду. Каждый столбец = `step × thickness`, sin-сдвиг центра.
    #[test]
    fn style_wavy_emits_sampled_columns() {
        // Один inline char ≈ 8px @ 16px font; thickness = 16·0.07 ≈ 1.12,
        // step = max(1, 1.12·0.5) = 1.0 → ~8 столбцов.
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline wavy; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(
            rects.len() >= 4,
            "wavy emits multiple columns, got {}",
            rects.len()
        );
        // Sum of widths ≈ underline-width (8px).
        let total_w: f32 = rects.iter().map(|r| r.width).sum();
        assert!(
            (total_w - 8.0).abs() < 0.1,
            "columns cover full width: sum={}, expected ≈ 8",
            total_w
        );
        // Все столбцы — одной thickness (height).
        let h0 = rects[0].height;
        for r in &rects {
            assert!((r.height - h0).abs() < 0.01, "uniform thickness");
        }
        // Y-координаты не одинаковы — иначе это бы Solid line.
        let y_min = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let y_max = rects.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
        assert!(
            y_max - y_min > 0.5,
            "wavy must vertically displace columns: range={}",
            y_max - y_min
        );
    }

    /// Амплитуда sin-сдвига должна не превышать `1.5 × thickness`
    /// (peak deviation от центра в обе стороны). Sum-y-range ≤
    /// 2·A + thickness, и не сильно меньше — амплитуда должна
    /// достигаться хотя бы раз на достаточной ширине.
    #[test]
    fn style_wavy_amplitude_matches_factor() {
        // 40px ширина с большой толщиной → волна успевает достичь обоих peak-ов.
        let dl = build_wrapped(
            "<p><a>xxxxx</a></p>",
            "a { text-decoration: underline wavy; \
                  text-decoration-thickness: 4px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() >= 8);
        let y_min = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let y_max = rects.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
        // A = 4 * 1.5 = 6; peak-to-peak ≈ 12, отступы между centers
        // достигают этого диапазона.
        let y_range = y_max - y_min;
        assert!(
            y_range > 6.0,
            "amplitude expected ≥ 6, got range={}",
            y_range
        );
        assert!(
            y_range <= 13.0,
            "amplitude should not exceed 2·A=12 (+1 sampling tolerance), got {}",
            y_range
        );
    }

    /// Wavy uses the same color as Solid (text-decoration-color / fallback).
    #[test]
    fn style_wavy_preserves_color() {
        let dl = build_wrapped(
            "<p style=\"color: red\"><a>x</a></p>",
            "a { text-decoration: underline wavy; }",
            800.0,
        );
        let fills: Vec<_> = dl
            .iter()
            .filter_map(|cmd| match cmd {
                DisplayCommand::FillRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert!(!fills.is_empty());
        for c in &fills {
            assert_eq!([c.r, c.g, c.b, c.a], [255, 0, 0, 255]);
        }
    }

    /// Каждый wavy column не выпадает за горизонтальные границы линии:
    /// последний column обрезается до остатка, не overshoot-ит.
    #[test]
    fn style_wavy_columns_clip_to_width() {
        let dl = build_wrapped(
            "<p><a>xx</a></p>",
            "a { text-decoration: underline wavy; \
                  text-decoration-thickness: 3px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        // x-min равен старту линии; x-max не превышает старт+width.
        let x_start = rects.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
        let x_end = rects
            .iter()
            .map(|r| r.x + r.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let total_w: f32 = rects.iter().map(|r| r.width).sum();
        assert!(
            (x_end - x_start - total_w).abs() < 0.01,
            "columns are non-overlapping and tile the line",
        );
    }

    /// `text-decoration-thickness: 4px` override-ит default 7%.
    #[test]
    fn thickness_length_overrides_default() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: 4px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        assert!(
            (rects[0].height - 4.0).abs() < 0.01,
            "thickness height={}, expected 4",
            rects[0].height
        );
    }

    /// `text-decoration-thickness: 25%` → 25% от font-size (Phase 0 от
    /// frag.font_size, не parent — задокументировано в style.rs).
    #[test]
    fn thickness_percentage_resolves_against_font_size() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: 25%; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        assert!(
            (rects[0].height - 4.0).abs() < 0.01,
            "expected 0.25·16 = 4, got {}",
            rects[0].height
        );
    }

    /// `text-decoration-thickness: from-font` в Phase 0 — без font-доступа,
    /// поэтому совпадает с `Auto` (≈ 7% от font-size).
    #[test]
    fn thickness_from_font_falls_back_to_auto() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: from-font; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        let default = (16.0_f32 * 0.07).max(1.0);
        assert!(
            (rects[0].height - default).abs() < 0.01,
            "height={}, expected ≈ {default}",
            rects[0].height
        );
    }

    /// Inline-ран переносится: второй DrawText смещён по Y.
    #[test]
    fn inline_run_wrap_y_offset() {
        // "aa" (16px) + " " (8) + "bb" (16) = 40px > 30px viewport → перенос.
        let dl = build_wrapped("<p>aa <span>bb</span></p>", "", 30.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        let line_h = 16.0_f32 * 1.2;
        // First line carries half-leading = (19.2-16)/2 = 1.6 (CSS 2.1 §10.8.1).
        let half_leading = (line_h - 16.0) / 2.0;
        assert!((rects[0].y - half_leading).abs() < 0.01, "y0={}", rects[0].y);
        assert!((rects[1].y - (half_leading + line_h)).abs() < 0.1, "y1={}", rects[1].y);
    }

    // ── Тесты border рендеринга ─────────────────────────────────────────────

    fn borders(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect()
    }

    #[test]
    fn border_solid_emits_draw_border() {
        let dl = build("<p>x</p>", "p { border: 2px solid red; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1, "должна быть одна DrawBorder команда");
        if let DisplayCommand::DrawBorder { widths, colors, styles, .. } = b[0] {
            assert!((widths[0] - 2.0).abs() < 0.01, "top width");
            assert!((widths[1] - 2.0).abs() < 0.01, "right width");
            assert_eq!(colors[0].r, 255, "top color — red");
            assert_eq!(
                *styles,
                [
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                ],
            );
        }
    }

    #[test]
    fn border_dashed_styles_propagate_to_command() {
        let dl = build("<p>x</p>", "p { border: 3px dashed blue; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { styles, .. } = b[0] {
            assert_eq!(
                *styles,
                [
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                ],
            );
        }
    }

    #[test]
    fn border_mixed_styles_per_side() {
        let dl = build(
            "<p>x</p>",
            "p { border-top: 2px solid black; \
                 border-right: 2px dashed black; \
                 border-bottom: 2px dotted black; \
                 border-left: 2px solid black; }",
        );
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { styles, .. } = b[0] {
            assert_eq!(styles[0], BorderStyle::Solid);
            assert_eq!(styles[1], BorderStyle::Dashed);
            assert_eq!(styles[2], BorderStyle::Dotted);
            assert_eq!(styles[3], BorderStyle::Solid);
        }
    }

    #[test]
    fn serialize_drawborder_solid_omits_styles() {
        // bw-compat: чистый Solid не печатает `s=[...]` — snapshot-ы
        // прежней версии остаются валидными.
        let dl = build("<p>x</p>", "p { border: 2px solid black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"));
        assert!(!s.contains(" s=["), "Solid не печатает s=[...]: {s}");
    }

    #[test]
    fn serialize_drawborder_dashed_emits_styles_field() {
        let dl = build("<p>x</p>", "p { border: 2px dashed black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"));
        assert!(
            s.contains(" s=[da,da,da,da]"),
            "Dashed эмитит s=[...]: {s}"
        );
    }

    #[test]
    fn serialize_drawborder_dotted_short_marker() {
        let dl = build("<p>x</p>", "p { border: 2px dotted black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains(" s=[do,do,do,do]"), "Dotted: {s}");
    }

    #[test]
    fn serialize_drawborder_mixed_marks_only_non_solid() {
        let dl = build(
            "<p>x</p>",
            "p { border: 2px solid black; \
                 border-right-style: dashed; }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains(" s=[s,da,s,s]"), "Mixed: {s}");
    }

    #[test]
    fn border_none_style_no_draw_border() {
        // border-width без border-style (default None) → DrawBorder не эмитируется.
        let dl = build("<p>x</p>", "p { border-width: 2px; }");
        assert!(borders(&dl).is_empty());
    }

    #[test]
    fn border_increases_height() {
        // Без border: высота = font_size * line_height = 16 * 1.2 = 19.2
        let no_border = build("<p>x</p>", "");
        let with_border = build("<p>x</p>", "p { border: 5px solid black; }");

        let height_of = |dl: &DisplayList| -> f32 {
            dl.iter()
                .find_map(|c| match c {
                    DisplayCommand::DrawText { rect, .. } => Some(rect.y),
                    _ => None,
                })
                .unwrap_or(0.0)
        };
        // Текст должен быть смещён на 5px вниз из-за border-top.
        let y_no = height_of(&no_border);
        let y_with = height_of(&with_border);
        assert!(
            (y_with - y_no - 5.0).abs() < 0.1,
            "y_no={y_no}, y_with={y_with}"
        );
    }

    #[test]
    fn border_color_none_uses_current_color() {
        // border без color → currentColor (наследуется из color: blue).
        let dl = build("<p>x</p>", "p { color: blue; border: 2px solid; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { colors, .. } = b[0] {
            assert_eq!(colors[0].b, 255, "border color should be blue (currentColor)");
        }
    }

    #[test]
    fn border_shorthand_in_serialize() {
        // serialize_display_list корректно форматирует DrawBorder.
        let dl = build("<p>x</p>", "p { border: 3px solid red; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"), "должна быть строка DrawBorder");
        assert!(s.contains("3.00"), "ширина 3px");
    }

    // ── Тесты <img> / DrawImage ─────────────────────────────────────────────

    pub(crate) fn images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect()
    }

    #[test]
    fn img_emits_draw_image() {
        let dl = build(r#"<img src="logo.png" alt="Logo" width="100" height="50">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, src, alt, .. } = imgs[0] {
            assert_eq!(src, "logo.png");
            assert_eq!(alt, "Logo");
            assert!((rect.width - 100.0).abs() < 0.1);
            assert!((rect.height - 50.0).abs() < 0.1);
        }
    }

    #[test]
    fn img_with_background_and_border_paints_in_order() {
        // Painter's order для replaced element: FillRect (bg) → DrawBorder →
        // DrawImage. Image идёт последним, чтобы быть над фоном.
        let dl = build(
            r#"<img src="x" width="50" height="50">"#,
            "img { background: blue; border: 2px solid red; }",
        );
        // Должны присутствовать все три команды.
        let kinds: Vec<&str> = dl
            .iter()
            .map(|c| match c {
                DisplayCommand::FillRect { .. } => "FillRect",
                DisplayCommand::FillRoundedRect { .. } => "FillRoundedRect",
                DisplayCommand::DrawBorder { .. } => "DrawBorder",
                DisplayCommand::DrawOutline { .. } => "DrawOutline",
                DisplayCommand::DrawImage { .. } => "DrawImage",
                DisplayCommand::DrawBackgroundImage { .. } => "DrawBackgroundImage",
                DisplayCommand::DrawText { .. } => "DrawText",
                DisplayCommand::PushClipRect { .. } => "PushClipRect",
                DisplayCommand::PushClipRoundedRect { .. } => "PushClipRoundedRect",
                DisplayCommand::PushClipPath { .. } => "PushClipPath",
                DisplayCommand::PopClip => "PopClip",
                DisplayCommand::PushOpacity { .. } => "PushOpacity",
                DisplayCommand::PopOpacity => "PopOpacity",
                DisplayCommand::PushBlendMode { .. } => "PushBlendMode",
                DisplayCommand::PopBlendMode => "PopBlendMode",
                DisplayCommand::DrawLayerSnapshot { .. } => "DrawLayerSnapshot",
                DisplayCommand::PushTransform { .. } => "PushTransform",
                DisplayCommand::PopTransform => "PopTransform",
                DisplayCommand::DrawLinearGradient { .. } => "DrawLinearGradient",
                DisplayCommand::DrawRadialGradient { .. } => "DrawRadialGradient",
                DisplayCommand::DrawConicGradient { .. } => "DrawConicGradient",
                DisplayCommand::PushMaskImage { .. } => "PushMaskImage",
                DisplayCommand::PushMaskLinearGradient { .. } => "PushMaskLinearGradient",
                DisplayCommand::PushMaskRadialGradient { .. } => "PushMaskRadialGradient",
                DisplayCommand::PushMaskConicGradient { .. } => "PushMaskConicGradient",
                DisplayCommand::PopMask => "PopMask",
                DisplayCommand::PushMaskLayer { .. } => "PushMaskLayer",
                DisplayCommand::PopMaskLayer => "PopMaskLayer",
                DisplayCommand::PushFilter { .. } => "PushFilter",
                DisplayCommand::PopFilter => "PopFilter",
                DisplayCommand::PushBackdropFilter { .. } => "PushBackdropFilter",
                DisplayCommand::PopBackdropFilter => "PopBackdropFilter",
                DisplayCommand::BeginStickyLayer { .. } => "BeginStickyLayer",
                DisplayCommand::EndStickyLayer => "EndStickyLayer",
                DisplayCommand::BeginFixedLayer => "BeginFixedLayer",
                DisplayCommand::EndFixedLayer => "EndFixedLayer",
                DisplayCommand::PushScrollLayer { .. } => "PushScrollLayer",
                DisplayCommand::PopScrollLayer => "PopScrollLayer",
                DisplayCommand::DrawSvgPath { .. } => "DrawSvgPath",
                DisplayCommand::DrawSvgFill { .. } => "DrawSvgFill",
                DisplayCommand::DrawSvgStroke { .. } => "DrawSvgStroke",
                DisplayCommand::BoxModelOverlay { .. } => "BoxModelOverlay",
                DisplayCommand::DrawScrollbar { .. } => "DrawScrollbar",
                DisplayCommand::PageBreak => "PageBreak",
                DisplayCommand::DrawCrossFade { .. } => "DrawCrossFade",
                DisplayCommand::LazyImageSlot { .. } => "LazyImageSlot",
            })
            .collect();
        assert_eq!(kinds, vec!["FillRect", "DrawBorder", "DrawImage"]);
    }

    #[test]
    fn img_serialize_includes_src_and_alt() {
        let dl = build(
            r#"<img src="photo.jpg" alt="A photo" width="80" height="40">"#,
            "",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawImage"), "must contain DrawImage line");
        assert!(s.contains(r#"src="photo.jpg""#), "must contain src");
        assert!(s.contains(r#"alt="A photo""#), "must contain alt");
    }

    /// BUG-431: the bitmap belongs in the content box, same rule as `<canvas>`
    /// (BUG-099) — painting at the border box slid it under the border+padding.
    #[test]
    fn img_bitmap_is_painted_into_the_content_box() {
        let dl = build(
            r#"<img src="x" width="100" height="80">"#,
            "*{margin:0}img{border:10px solid red;padding:5px}",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 100.0, 80.0));
        }
    }

    // ── Тесты loading="lazy" / LazyImageSlot ───────────────────────────────

    fn lazy_slots(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::LazyImageSlot { .. }))
            .collect()
    }

    #[test]
    fn lazy_img_emits_lazy_image_slot_not_draw_image() {
        let dl = build(
            r#"<img src="hero.jpg" loading="lazy" width="200" height="100">"#,
            "",
        );
        // Must emit LazyImageSlot, not DrawImage.
        assert!(lazy_slots(&dl).len() == 1, "expected one LazyImageSlot");
        assert!(images(&dl).is_empty(), "must not emit DrawImage for lazy img");
    }

    #[test]
    fn eager_img_still_emits_draw_image() {
        let dl = build(r#"<img src="thumb.jpg" width="80" height="40">"#, "");
        assert!(images(&dl).len() == 1, "non-lazy img must emit DrawImage");
        assert!(lazy_slots(&dl).is_empty(), "non-lazy must not emit LazyImageSlot");
    }

    #[test]
    fn lazy_img_slot_has_correct_src_and_rect() {
        let dl = build(
            r#"<img src="banner.png" loading="lazy" width="300" height="150">"#,
            "",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { src, rect, .. } = slots[0] {
            assert_eq!(src, "banner.png");
            assert!((rect.width - 300.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 150.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    /// BUG-431: the lazy slot's rect is later reused as the loaded image's
    /// paint rect (shell BUG-163), so it must be content-box too, not just
    /// the eager `DrawImage` path.
    #[test]
    fn lazy_img_slot_is_content_box() {
        let dl = build(
            r#"<img src="banner.png" loading="lazy" width="300" height="150">"#,
            "*{margin:0}img{border:10px solid red;padding:5px}",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { rect, .. } = slots[0] {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 300.0, 150.0));
        }
    }

    #[test]
    fn lazy_img_case_insensitive() {
        let dl = build(
            r#"<img src="poster.jpg" loading="LAZY" width="50" height="50">"#,
            "",
        );
        assert_eq!(lazy_slots(&dl).len(), 1, "LAZY (uppercase) must emit LazyImageSlot");
    }

    #[test]
    fn lazy_img_node_id_set() {
        let dl = build(
            r#"<img src="lazy.png" loading="lazy" width="100" height="100">"#,
            "",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { node_id, .. } = slots[0] {
            // node_id must be > 0 (document root is 0; img elements get a non-zero id).
            assert!(*node_id > 0, "lazy img node_id must be non-zero, got {node_id}");
        }
    }

    #[test]
    fn lazy_img_slot_carries_object_fit() {
        // BUG-163: a lazy <img> keeps its loading="lazy" attribute even after the
        // shell fetches it, so it is painted via LazyImageSlot (not DrawImage).
        // The slot must therefore carry object_fit/object_position so the backend
        // can draw the loaded image with the correct CSS fitting, not a raw fill.
        let dl = build(
            r#"<img src="cover.jpg" loading="lazy" width="200" height="100" style="object-fit: cover">"#,
            "",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { object_fit, .. } = slots[0] {
            assert_eq!(*object_fit, ObjectFit::Cover, "lazy slot must carry object-fit");
        } else {
            panic!("expected LazyImageSlot");
        }
    }

    #[test]
    fn lazy_img_serialize_contains_lazy_image_slot() {
        let dl = build(
            r#"<img src="deferred.jpg" loading="lazy" width="100" height="50">"#,
            "",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("LazyImageSlot"), "serialize must include LazyImageSlot");
        assert!(s.contains(r#"src="deferred.jpg""#), "serialize must include src");
    }

    // ── Тесты <video> / DrawImage placeholder ───────────────────────────────

    #[test]
    fn video_without_poster_emits_no_draw_image() {
        // BUG-097: an empty <video> (no poster, no decoded frame) paints nothing —
        // the element box is transparent, matching Chromium/Edge. The grey image
        // placeholder is reserved for <img>, not media.
        let dl = build(r#"<video src="clip.mp4"></video>"#, "");
        let imgs = images(&dl);
        assert!(
            imgs.is_empty(),
            "posterless video should emit no DrawImage, got {}",
            imgs.len()
        );
    }

    #[test]
    fn video_with_poster_emits_draw_image_with_poster_src() {
        // When poster is set, DrawImage uses the poster URL so shell can register it.
        let dl = build(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { src, .. } = imgs[0] {
            assert_eq!(src, "thumb.jpg");
        }
    }

    #[test]
    fn video_ua_default_rect_300_by_150() {
        // Poster present so the replaced box paints a DrawImage at the UA-default rect.
        let dl = build(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!((rect.width - 300.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 150.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    #[test]
    fn video_css_dimensions_override_ua_default() {
        let dl = build(
            r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#,
            "video { width: 640px; height: 360px; }",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!((rect.width - 640.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 360.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    /// BUG-431: the poster frame belongs in the content box, same rule as
    /// `<img>`/`<canvas>` — painting at the border box slid it under the
    /// border+padding.
    #[test]
    fn video_poster_is_painted_into_the_content_box() {
        let dl = build(
            r#"<video src="clip.mp4" poster="thumb.jpg" width="100" height="80"></video>"#,
            "*{margin:0}video{border:10px solid red;padding:5px}",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 100.0, 80.0));
        }
    }

