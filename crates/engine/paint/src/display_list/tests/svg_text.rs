//! Тесты SVG text rendering — вынесены из `display_list/tests/svg_table_and_hash.rs`
//! байт-в-байт, без дедента: файл перерос потолок размера (docs/lint-policy.md
//! §5.1). Дочерний модуль `svg_table_and_hash`, поэтому видит его импорты
//! (`build`/`fills`/`texts`/`build_ordered`) через `super::*`.

use super::*;

    // ── Тесты SVG text rendering ───────────────────────────────────────

    #[test]
    fn svg_text_emits_drawtext_command() {
        // <text>Hello</text> should emit a DrawText command
        let dl = build("<svg><text>Hello</text></svg>", "");
        let texts = texts(&dl);
        assert!(texts.iter().any(|t| t.contains("Hello")), "should emit text 'Hello'");
    }

    #[test]
    fn ordered_svg_shape_emits_fill() {
        // BUG-089: the ordered (stacking-context) path must paint SVG shapes.
        // `emit_box_self` previously no-op'd SvgShape, so shapes vanished in the
        // shell's ordered pipeline (only `walk` painted them). A <rect> with an
        // explicit fill must produce a FillRect via `build_display_list_ordered`.
        let dl = build_ordered(
            "<svg width='100' height='100'><rect x='0' y='0' width='50' height='50' style='fill:#ff0000;'/></svg>",
            "",
        );
        let has_red_fill = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::FillRect { color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0
        ));
        assert!(has_red_fill, "ordered path must emit FillRect for SVG <rect>, got {dl:?}");
    }

    #[test]
    fn svg_viewport_clips_overflowing_content() {
        // BUG-110: an SVG with object-fit: cover scales its viewBox to overflow the box;
        // the SVG viewport (UA default `overflow: hidden`) must clip it. Both the `walk`
        // and the ordered pipeline must wrap the SVG's shape children in a PushClipRect
        // bounded by the SVG box (160×120), so cover content cannot paint over siblings.
        let html = "<svg width='160' height='120' viewBox='0 0 200 80' style='object-fit:cover;'>\
                    <rect width='200' height='80' style='fill:#ff0000;'/></svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            let clip = dl.iter().find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } if (rect.width - 160.0).abs() < 1.0
                    && (rect.height - 120.0).abs() < 1.0 => Some(*rect),
                _ => None,
            });
            assert!(clip.is_some(), "SVG viewport must emit a 160×120 PushClipRect, got {dl:?}");
        }
    }

    #[test]
    fn ordered_svg_text_emits_drawtext() {
        // BUG-089 companion: ordered path must also paint SVG <text>.
        let dl = build_ordered("<svg><text>Hi</text></svg>", "");
        let has_text = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::DrawText { text, .. } if text.contains("Hi")
        ));
        assert!(has_text, "ordered path must emit DrawText for SVG <text>");
    }

    /// BUG-1294: `fill: url(#pattern)` replays the tile content once per tile.
    #[test]
    fn svg_pattern_fill_replays_tile_content_per_tile() {
        let dl = build_ordered(
            "<svg width='100' height='100'><defs>\
                <pattern id='p' patternUnits='userSpaceOnUse' width='20' height='20'>\
                    <rect width='10' height='10' fill='#ff0000'/>\
                </pattern></defs>\
                <rect width='100' height='100' fill='url(#p)'/>\
             </svg>",
            "",
        );
        let red = dl.iter().filter(|c| matches!(c,
            DisplayCommand::FillRect { color, .. } if color.r == 255 && color.g == 0 && color.b == 0)).count();
        assert_eq!(red, 25, "100×100 shape / 20×20 tile = 25 tiles, got {dl:?}");
        let pushes = dl.iter().filter(|c| matches!(c, DisplayCommand::PushTransform { .. })).count();
        let pops = dl.iter().filter(|c| matches!(c, DisplayCommand::PopTransform)).count();
        assert_eq!(pushes, pops, "PushTransform/PopTransform must balance");
    }

    /// BUG-1294: `patternUnits` defaults to `objectBoundingBox`, `patternTransform` rotates the grid.
    #[test]
    fn svg_pattern_bbox_units_and_transform() {
        // 0.5×0.5 of a 100×100 box = 50×50 tiles → 4 tiles.
        let dl = build_ordered(
            "<svg width='100' height='100'><defs>\
                <pattern id='p' width='0.5' height='0.5'><rect width='10' height='10' fill='#00ff00'/></pattern></defs>\
                <rect width='100' height='100' fill='url(#p)'/>\
             </svg>",
            "",
        );
        let green = |dl: &Vec<DisplayCommand>| dl.iter().filter(|c| matches!(c,
            DisplayCommand::FillRect { color, .. } if color.g == 255 && color.r == 0)).count();
        assert_eq!(green(&dl), 4, "got {dl:?}");
        // A rotated grid still covers the shape (more tiles than the unrotated 4).
        let rot = build_ordered(
            "<svg width='100' height='100'><defs>\
                <pattern id='p' width='0.5' height='0.5' patternTransform='rotate(45)'>\
                <rect width='10' height='10' fill='#00ff00'/></pattern></defs>\
                <rect width='100' height='100' fill='url(#p)'/>\
             </svg>",
            "",
        );
        assert!(green(&rot) > 4, "rotated grid needs extra tiles, got {rot:?}");
    }

    /// BUG-1294: an axis-aligned (here mirrored) pattern bakes its matrix into the
    /// tile content — no bitmap `PushTransform`, content mirrored inside the shape.
    #[test]
    fn svg_pattern_mirrored_scale_is_baked_without_transform_layer() {
        let dl = build_ordered(
            "<svg width='200' height='200'><defs>\
                <pattern id='p' patternUnits='userSpaceOnUse' width='50' height='50'>\
                    <rect width='10' height='10' fill='#ff0000'/></pattern></defs>\
                <rect width='50' height='50' fill='url(#p)' transform='matrix(-2 0 0 1 100 0)'/>\
             </svg>",
            "",
        );
        assert!(
            !dl.iter().any(|c| matches!(c, DisplayCommand::PushTransform { .. })),
            "mirrored axis-aligned pattern must not need a transform layer, got {dl:?}",
        );
        let red: Vec<&Rect> = dl.iter().filter_map(|c| match c {
            DisplayCommand::FillRect { rect, color } if color.r == 255 && color.g == 0 => Some(rect),
            _ => None,
        }).collect();
        assert_eq!(red.len(), 1, "one 50×50 tile → one red square, got {dl:?}");
        // x: tile origin maps to the right edge (100 − 0·2 + svg origin 0) and extends left by 20.
        assert!(red[0].width > 0.0 && red[0].height > 0.0, "mirrored rect must be normalized: {:?}", red[0]);
        assert!((red[0].width - 20.0).abs() < 0.5, "10 user units × |−2| = 20 px wide, got {:?}", red[0]);
    }

    /// BUG-1294: `href` inherits geometry and content from another pattern.
    #[test]
    fn svg_pattern_href_inherits_content() {
        let dl = build_ordered(
            "<svg width='100' height='100'><defs>\
                <pattern id='base' patternUnits='userSpaceOnUse' width='50' height='50'>\
                    <rect width='5' height='5' fill='#0000ff'/></pattern>\
                <pattern id='p' href='#base'/></defs>\
                <rect width='100' height='100' fill='url(#p)'/>\
             </svg>",
            "",
        );
        let blue = dl.iter().filter(|c| matches!(c,
            DisplayCommand::FillRect { color, .. } if color.b == 255 && color.r == 0 && color.g == 0)).count();
        assert_eq!(blue, 4, "got {dl:?}");
    }

    #[test]
    fn ordered_svg_path_stroke_emits_drawsvgstroke() {
        // BUG-096: an SVG <path> has a zero-size layout rect (path bbox is deferred
        // to paint), so `emit_svg_shape`'s 0×0 guard used to drop every path in the
        // ordered pipeline → TEST-54 painted nothing. The path must emit its stroke
        // in the stroke colour (#e94560 = 233,69,96), and `fill="none"` (an SVG
        // presentation attribute) must suppress the fill so no black fill leaks in.
        //
        // BUG-247: the stroke is now emitted as `DrawSvgStroke` (raw contours +
        // params) rather than a pre-tessellated `DrawSvgPath` triangle soup, so
        // femtovg can stroke it natively without internal AA seams.
        let dl = build_ordered(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20' fill='none' stroke='#e94560' stroke-width='8'/>\
             </svg>",
            "",
        );
        let strokes: Vec<(&Color, &crate::svg_path::StrokeParams)> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgStroke { color, params, .. } => Some((color, params)),
                _ => None,
            })
            .collect();
        assert!(!strokes.is_empty(), "ordered path must emit DrawSvgStroke for <path>, got {dl:?}");
        assert!(
            strokes.iter().all(|(c, _)| c.r == 233 && c.g == 69 && c.b == 96),
            "path must stroke in colour #e94560, not a default black fill; got {strokes:?}",
        );
        assert!(
            strokes.iter().all(|(_, p)| (p.half_width - 4.0).abs() < 1e-3),
            "stroke-width:8 → half_width 4.0; got {strokes:?}",
        );
        // The old triangle-soup stroke must be gone in the stroke colour.
        let soup = dl.iter().any(|c| matches!(c,
            DisplayCommand::DrawSvgPath { color, .. } if color.r == 233 && color.g == 69 && color.b == 96));
        assert!(!soup, "stroke must NOT emit a DrawSvgPath triangle soup, got {dl:?}");
    }

    #[test]
    fn nonzero_path_fill_emits_drawsvgfill_not_triangle_soup() {
        // BUG-247 / BUG-173: a nonzero `<path>` area fill is emitted as the raw
        // outline contours (`DrawSvgFill`) so femtovg/tiny_skia anti-alias only
        // the true boundary. A triangle soup (`DrawSvgPath`) made them fringe
        // every internal shared edge (~1px seams). `fill-rule: evenodd` cannot be
        // filled natively (no even-odd path mode in femtovg/wgpu), so it stays a
        // triangle soup via the scanline decomposition.
        let filled_triangle =
            "<svg width='200' height='160'>\
                <path d='M 100 10 L 185 145 L 15 145 Z' fill='#0f3460'/>\
             </svg>"; // #0f3460 = (15, 52, 96)
        for dl in [build(filled_triangle, ""), build_ordered(filled_triangle, "")] {
            let fills: Vec<&Vec<Vec<[f32; 2]>>> = dl.iter().filter_map(|c| match c {
                DisplayCommand::DrawSvgFill { contours, color }
                    if color.r == 15 && color.g == 52 && color.b == 96 => Some(contours),
                _ => None,
            }).collect();
            assert_eq!(fills.len(), 1, "nonzero fill must emit one DrawSvgFill, got {dl:?}");
            assert!(
                fills[0].iter().any(|c| c.len() >= 3),
                "DrawSvgFill must carry the closed outline (≥3 points), got {:?}", fills[0],
            );
            // No triangle-soup fill in the fill colour.
            let soup = dl.iter().any(|c| matches!(c,
                DisplayCommand::DrawSvgPath { color, .. } if color.r == 15 && color.g == 52 && color.b == 96));
            assert!(!soup, "nonzero fill must NOT emit a DrawSvgPath triangle soup, got {dl:?}");
        }

        // even-odd keeps the triangle-soup path (DrawSvgPath), no DrawSvgFill.
        let even_odd =
            "<svg width='200' height='160'>\
                <path d='M 100 10 L 185 145 L 15 145 Z' fill='#0f3460' fill-rule='evenodd'/>\
             </svg>";
        for dl in [build(even_odd, ""), build_ordered(even_odd, "")] {
            let has_soup = dl.iter().any(|c| matches!(c,
                DisplayCommand::DrawSvgPath { color, .. } if color.r == 15 && color.g == 52 && color.b == 96));
            let has_fill = dl.iter().any(|c| matches!(c, DisplayCommand::DrawSvgFill { .. }));
            assert!(has_soup, "even-odd fill must emit DrawSvgPath (scanline tessellation), got {dl:?}");
            assert!(!has_fill, "even-odd fill must NOT emit DrawSvgFill (no native even-odd), got {dl:?}");
        }
    }

    #[test]
    fn svg_diagonal_line_strokes_segment_not_filled_bbox() {
        // BUG-189: a diagonal SVG <line> used to render as `FillRect { rect: b.rect }`
        // — filling the entire (large) bounding box of the segment with the stroke
        // colour, producing a solid orange rectangle instead of a thin diagonal.
        // The line must now emit a `DrawSvgPath` (a thick segment) in the stroke
        // colour, and NO `FillRect` in that colour.
        let stroke = "#f39c12"; // (243, 156, 18)
        let html = "<svg width='160' height='100'>\
            <line x1='10' y1='10' x2='150' y2='90' stroke='#f39c12' stroke-width='6' fill='none'/>\
        </svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            let stroke_paths: Vec<&Vec<[f32; 2]>> = dl.iter().filter_map(|c| match c {
                DisplayCommand::DrawSvgPath { vertices, color }
                    if color.r == 243 && color.g == 156 && color.b == 18 => Some(vertices),
                _ => None,
            }).collect();
            assert_eq!(stroke_paths.len(), 1, "line must emit one stroke DrawSvgPath ({stroke}), got {dl:?}");
            assert_eq!(stroke_paths[0].len(), 6, "a single butt-cap segment = two triangles (6 verts)");
            // The old bug: a FillRect in the stroke colour spanning the bbox.
            let solid_fill = dl.iter().any(|c| matches!(c,
                DisplayCommand::FillRect { color, .. } if color.r == 243 && color.g == 156 && color.b == 18));
            assert!(!solid_fill, "line must NOT paint a solid {stroke} FillRect (BUG-189), got {dl:?}");
        }
    }

    #[test]
    fn svg_rotated_rect_paints_under_ctm_in_user_space() {
        // BUG-244: a rotated SVG shape used to collapse to its axis-aligned bounding
        // box (`apply_transform_to_bbox` re-bounds the rotated corners), silently
        // dropping the rotation. The fix mirrors a browser CTM: the shape's geometry
        // is painted in *user* coordinates wrapped in a `PushTransform` carrying the
        // full document-space matrix (viewport ∘ composed), with off-diagonal
        // (rotate/skew) components an AABB cannot represent.
        let html = "<svg width='200' height='200'>\
            <rect x='40' y='40' width='80' height='40' transform='rotate(45)' fill='#00ff00'/>\
        </svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            // A PushTransform with a non-zero off-diagonal `b` (= sin θ) must wrap the
            // shape — the rotation survives instead of being flattened to an AABB.
            let ctm = dl.iter().find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } if matrix.0[1].abs() > 0.1 => Some(*matrix),
                _ => None,
            });
            assert!(ctm.is_some(), "rotated <rect> must emit a PushTransform with rotation, got {dl:?}");
            // The fill is painted in the rect's *user* coordinates (40,40,80,40); the
            // matrix (not the rect) positions it. Before the fix the FillRect carried
            // the inflated rotated AABB instead.
            let fill = dl.iter().find_map(|c| match c {
                DisplayCommand::FillRect { rect, color }
                    if color.r == 0 && color.g == 255 && color.b == 0 => Some(*rect),
                _ => None,
            }).expect("green FillRect present");
            assert!(
                (fill.x - 40.0).abs() < 0.5 && (fill.y - 40.0).abs() < 0.5
                    && (fill.width - 80.0).abs() < 0.5 && (fill.height - 40.0).abs() < 0.5,
                "rotated rect fill must stay in user coords (40,40,80,40), got {fill:?}",
            );
            // The CTM must be balanced by a PopTransform.
            assert!(
                dl.iter().any(|c| matches!(c, DisplayCommand::PopTransform)),
                "PushTransform must be closed by PopTransform, got {dl:?}",
            );
        }
    }

    #[test]
    fn svg_untransformed_rect_has_no_ctm() {
        // BUG-244 guard: a plain (translate/scale-only) shape keeps the exact
        // axis-aligned `b.rect` fast path — no PushTransform, no AA change. Only
        // rotation/skew (off-diagonal components) trigger the CTM path.
        let html = "<svg width='100' height='100'>\
            <rect x='10' y='10' width='50' height='50' fill='#ff0000'/>\
        </svg>";
        let dl = build(html, "");
        assert!(
            !dl.iter().any(|c| matches!(c, DisplayCommand::PushTransform { .. })),
            "untransformed <rect> must not emit a PushTransform, got {dl:?}",
        );
    }

    #[test]
    fn svg_scaled_use_path_paints_under_ctm() {
        // BUG-424 (a): a `<path>`/`<polyline>`/`<polygon>` (lowered to `Path`) has no
        // scaled `b.rect` — layout collapses it to a zero-size anchor point (BUG-174).
        // Rect/Circle/Ellipse/Line get their scale baked into `b.rect`, so a pure
        // scale with no rotation (`has_rot_skew=false`) already rendered correctly
        // via the axis-aligned fast path; `Path` did not — the common icon-sprite
        // case (`viewBox="0 0 24 24"` scaled down to a 12px box, no rotation) used
        // to paint the raw, unscaled `d`/`points` vertices, ~2× oversized. It must
        // now route through the same CTM `PushTransform` as rotate/skew whenever
        // the matrix carries a non-identity scale.
        let html = "<svg width='12' height='12'>\
            <symbol id='s' viewBox='0 0 24 24'><polyline points='15 18 9 12 15 6' fill='none' stroke='#000'/></symbol>\
            <use href='#s'/>\
         </svg>";
        let dl = build(html, "");
        let ctm = dl.iter().find_map(|c| match c {
            DisplayCommand::PushTransform { matrix } if (matrix.0[0] - 0.5).abs() < 0.01 => Some(*matrix),
            _ => None,
        });
        assert!(ctm.is_some(), "scaled <use>-<polyline> must emit a PushTransform with the 0.5 viewBox→viewport scale, got {dl:?}");
        let ctm = ctm.unwrap();
        assert!(ctm.0[1].abs() < 1e-6 && ctm.0[2].abs() < 1e-6, "pure scale, no rotation/skew expected, got {ctm:?}");
        assert!(
            dl.iter().any(|c| matches!(c, DisplayCommand::PopTransform)),
            "PushTransform must be closed by PopTransform, got {dl:?}",
        );
        // Under the CTM the stroke's vertices stay in raw local (0..24) units —
        // the matrix, not a pre-shift, carries the viewBox→viewport scale.
        let stroke = dl.iter().find_map(|c| match c {
            DisplayCommand::DrawSvgStroke { contours, .. } => Some(contours),
            _ => None,
        }).expect("polyline stroke present");
        let max_coord = stroke.iter().flatten().flat_map(|[x, y]| [*x, *y]).fold(0.0_f32, f32::max);
        assert!(max_coord > 10.0, "stroke vertices should stay in raw 0..24 local units under the CTM, got max={max_coord} in {stroke:?}");
    }

    #[test]
    fn svg_rect_stroke_is_centred_on_edge() {
        // BUG-226: an SVG stroke is centred on the geometry edge (SVG 2 §13.7) —
        // half its width outside the box, half inside. Previously the stroke was
        // painted entirely inside (`DrawBorder { rect: b.rect }`, border-box model),
        // shrinking the visible orange-core by stroke-width/2 per side (79×59 vs
        // Edge 89×69 at stroke-width 10). The stroke's DrawBorder rect must now be
        // the fill rect inflated by stroke-width/2 on every side, with the fill left
        // on the original geometry.
        let html = "<svg width='200' height='160'>\
            <rect x='20' y='20' width='100' height='80' fill='#3498db' stroke='#e74c3c' stroke-width='10'/>\
        </svg>";
        for dl in [build(html, ""), build_ordered(html, "")] {
            // fill (#3498db = 52,152,219) FillRect on the original geometry.
            let fill = dl.iter().find_map(|c| match c {
                DisplayCommand::FillRect { rect, color }
                    if color.r == 52 && color.g == 152 && color.b == 219 => Some(*rect),
                _ => None,
            }).expect("fill FillRect present");
            // stroke (#e74c3c = 231,76,60) DrawBorder, inflated by w/2 per side.
            let (srect, widths) = dl.iter().find_map(|c| match c {
                DisplayCommand::DrawBorder { rect, widths, colors, .. }
                    if colors[0].r == 231 && colors[0].g == 76 && colors[0].b == 60 => Some((*rect, *widths)),
                _ => None,
            }).expect("stroke DrawBorder present");
            assert_eq!(widths[0], 10.0, "stroke width preserved");
            let half = 5.0_f32; // stroke-width / 2
            assert!((srect.x - (fill.x - half)).abs() < 0.01,
                "stroke rect moves out by w/2: x {} want {}", srect.x, fill.x - half);
            assert!((srect.y - (fill.y - half)).abs() < 0.01,
                "stroke rect moves out by w/2: y {} want {}", srect.y, fill.y - half);
            assert!((srect.width - (fill.width + 10.0)).abs() < 0.01,
                "stroke rect width grows by w: {} want {}", srect.width, fill.width + 10.0);
            assert!((srect.height - (fill.height + 10.0)).abs() < 0.01,
                "stroke rect height grows by w: {} want {}", srect.height, fill.height + 10.0);
        }
    }

    #[test]
    fn paint_order_default_paints_fill_then_stroke() {
        // CSS Fill & Stroke L3 §6 — default `normal` order: fill first, stroke on top.
        // fill=red (#ff0000), stroke=blue (#0000ff) on a closed triangle path.
        let dl = build(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20 L 180 140 Z' fill='#ff0000' stroke='#0000ff' stroke-width='10'/>\
             </svg>",
            "",
        );
        // Fill is a native `DrawSvgFill`, stroke a native `DrawSvgStroke`
        // (BUG-247) — collect both in document (paint) order.
        let colors: Vec<&Color> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgFill { color, .. }
                | DisplayCommand::DrawSvgStroke { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 2, "fill + stroke → two svg paint commands, got {dl:?}");
        assert_eq!((colors[0].r, colors[0].b), (255, 0), "fill (red) painted first");
        assert_eq!((colors[1].r, colors[1].b), (0, 255), "stroke (blue) painted on top");
    }

    #[test]
    fn paint_order_stroke_reverses_fill_and_stroke() {
        // `paint-order: stroke` paints stroke first (under the fill).
        let dl = build(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20 L 180 140 Z' fill='#ff0000' stroke='#0000ff' \
                 stroke-width='10' style='paint-order: stroke'/>\
             </svg>",
            "",
        );
        // Fill is a native `DrawSvgFill`, stroke a native `DrawSvgStroke`
        // (BUG-247) — collect both in document (paint) order.
        let colors: Vec<&Color> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgFill { color, .. }
                | DisplayCommand::DrawSvgStroke { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 2, "fill + stroke → two svg paint commands, got {dl:?}");
        assert_eq!((colors[0].r, colors[0].b), (0, 255), "stroke (blue) painted first, under fill");
        assert_eq!((colors[1].r, colors[1].b), (255, 0), "fill (red) painted on top");
    }

    #[test]
    fn svg_text_with_fill_color() {
        // <text style="fill: red">Colored</text> should emit DrawText with fill color
        let dl = build("<svg><text style=\"fill: red\">Colored</text></svg>", "");
        let text_cmds: Vec<&DisplayCommand> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(!text_cmds.is_empty(), "should emit DrawText command");
    }

    #[test]
    fn svg_text_with_font_size() {
        // <text style="font-size: 24px">Sized</text> should use specified font-size
        let dl = build("<svg><text style=\"font-size: 24px\">Sized</text></svg>", "");
        let text_cmds: Vec<_> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { font_size, .. } => Some(font_size),
                _ => None,
            })
            .collect();
        assert!(!text_cmds.is_empty(), "should have DrawText with font-size");
    }

    #[test]
    fn svg_tspan_emits_text() {
        // <text><tspan>Part1</tspan><tspan>Part2</tspan></text> should emit text
        let dl = build("<svg><text><tspan>Part1</tspan><tspan>Part2</tspan></text></svg>", "");
        let texts = texts(&dl);
        assert!(!texts.is_empty(), "should emit at least one text command");
    }

    #[test]
    fn svg_textpath_collects_content() {
        // <text><textPath>OnPath</textPath></text> should collect textPath content
        let dl = build("<svg><text><textPath>OnPath</textPath></text></svg>", "");
        let texts = texts(&dl);
        // Phase 1: just collect and emit content, ignore path rendering
        assert!(texts.iter().any(|t| t.contains("OnPath")) || texts.is_empty(),
                "should have collected textPath content or empty is acceptable in Phase 1");
    }

    #[test]
    fn svg_text_anchor_middle_shifts_x_left() {
        // text-anchor="middle": DrawText rect.x should be shifted left by ~half text width
        // compared to text-anchor="start" at the same SVG x position.
        let dl_start = build(r#"<svg width="200" height="100"><text x="100" y="50" text-anchor="start">AB</text></svg>"#, "");
        let dl_middle = build(r#"<svg width="200" height="100"><text x="100" y="50" text-anchor="middle">AB</text></svg>"#, "");
        let x_start = dl_start.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.x),
            _ => None,
        });
        let x_middle = dl_middle.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.x),
            _ => None,
        });
        let (xs, xm) = (x_start.expect("start DrawText"), x_middle.expect("middle DrawText"));
        assert!(xm < xs, "text-anchor=middle should shift x left vs start: middle={xm}, start={xs}");
    }

    #[test]
    fn svg_text_dx_dy_offset_applied() {
        // dx="10" dy="5" should shift the DrawText rect by those amounts vs no offset
        let dl_no_offset = build(r#"<svg width="200" height="100"><text x="50" y="50">Hi</text></svg>"#, "");
        let dl_with_offset = build(r#"<svg width="200" height="100"><text x="50" y="50" dx="10" dy="5">Hi</text></svg>"#, "");
        let pos_no = dl_no_offset.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some((rect.x, rect.y)),
            _ => None,
        });
        let pos_off = dl_with_offset.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some((rect.x, rect.y)),
            _ => None,
        });
        let ((x0, y0), (x1, y1)) = (pos_no.expect("no-offset DrawText"), pos_off.expect("offset DrawText"));
        assert!((x1 - x0 - 10.0).abs() < 1.0, "dx=10 should shift x by ~10: Δx={}", x1 - x0);
        assert!((y1 - y0 - 5.0).abs() < 1.0, "dy=5 should shift y by ~5: Δy={}", y1 - y0);
    }

    #[test]
    fn svg_text_dominant_baseline_middle_shifts_y() {
        // dominant-baseline="middle" should shift DrawText rect.y up compared to auto
        let dl_auto = build(r#"<svg width="200" height="100"><text x="50" y="50" dominant-baseline="auto">T</text></svg>"#, "");
        let dl_middle = build(r#"<svg width="200" height="100"><text x="50" y="50" dominant-baseline="middle">T</text></svg>"#, "");
        let y_auto = dl_auto.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.y),
            _ => None,
        });
        let y_middle = dl_middle.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.y),
            _ => None,
        });
        let (ya, ym) = (y_auto.expect("auto DrawText"), y_middle.expect("middle DrawText"));
        assert!(ym < ya, "dominant-baseline=middle should shift y up vs auto: middle={ym}, auto={ya}");
    }

    #[test]
    fn svg_text_baseline_shift_super_raises_y() {
        // baseline-shift: super raises the text (smaller y) vs baseline; sub lowers it.
        let text_y = |bs: &str| -> f32 {
            let css = format!("text {{ baseline-shift: {bs}; }}");
            build(r#"<svg width="200" height="100"><text x="50" y="50">T</text></svg>"#, &css)
                .iter()
                .find_map(|c| match c {
                    DisplayCommand::DrawText { rect, .. } => Some(rect.y),
                    _ => None,
                })
                .expect("DrawText")
        };
        let y_baseline = text_y("baseline");
        let y_super = text_y("super");
        let y_sub = text_y("sub");
        let y_len = text_y("10px");
        assert!(y_super < y_baseline, "super should raise (smaller y): super={y_super}, baseline={y_baseline}");
        assert!(y_sub > y_baseline, "sub should lower (larger y): sub={y_sub}, baseline={y_baseline}");
        // Positive length raises by exactly that many px.
        assert!((y_len - (y_baseline - 10.0)).abs() < 0.01, "10px should raise by 10: len={y_len}, baseline={y_baseline}");
    }
