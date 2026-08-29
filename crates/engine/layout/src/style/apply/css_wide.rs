//! CSS Cascade L4 §7 — применение CSS-wide keyword (`inherit` / `initial` / `unset` /
//! `revert`) к одному свойству.
//!
//! Перенесено батчем SPLIT-ST6 из `crates/engine/layout/src/style.rs`
//! (анкер `fn apply_css_wide_keyword`) без правок тела: изменены только
//! видимость функции и пути импортов.

use crate::style::{ComputedStyle, CssWideKeyword, WhiteSpace};


/// CSS Cascade L4 §7 — применить CSS-wide keyword к одному свойству.
///
/// Источник значения:
/// - `Inherit` — всегда родительский computed value (для любого свойства).
/// - `Initial` — всегда initial value свойства из спецификации
///   (берётся из `ComputedStyle::root()`).
/// - `Unset` — для inherited-свойств работает как `Inherit`, для
///   non-inherited как `Initial`.
/// - `Revert` — откат к `ua_baseline` (значение свойства, каким оно было бы
///   без author/user-правил — см. `CssWideKeyword` doc). Для свойств без
///   UA-хинта `ua_baseline` совпадает с `inherited`/`init`, так что итог
///   идентичен `Unset`.
///
/// Per-property список синхронизирован с `apply_declaration` и `compute_style`-init —
/// неизвестные имена молча игнорируются.
pub(in crate::style) fn apply_css_wide_keyword(
    style: &mut ComputedStyle,
    prop: &str,
    kw: CssWideKeyword,
    inherited: &ComputedStyle,
    ua_baseline: &ComputedStyle,
) {
    use CssWideKeyword::{Inherit, Revert, Unset};
    // Initial-значения как у root документа — кроме `Revert`, где ролью
    // «non-inherited fallback» играет UA-снэпшот. ComputedStyle::root()
    // выделяет несколько Vec/HashMap, но эта функция вызывается только при
    // обнаружении CSS-wide-keyword в декларации — редкий путь, накладные
    // расходы незаметны на типичной странице.
    let init = if kw == Revert { ua_baseline.clone() } else { ComputedStyle::root() };
    // Для `Revert` «родительское» значение тоже берётся из UA-снэпшота —
    // это покрывает и inherited-свойства, которые UA-хинты трогают
    // (font-style/font-weight/color/white-space/line-height и т.д.).
    let inherited: &ComputedStyle = if kw == Revert { ua_baseline } else { inherited };

    // Helper «inherited property»: Inherit/Unset/Revert → inherited, Initial → init.
    let inh = matches!(kw, Inherit | Unset | Revert);
    // Helper «non-inherited property»: Inherit → inherited, Initial/Unset/Revert → init.
    let inh_only_inherit = matches!(kw, Inherit);

    match prop {
        // ──────── Inherited properties ────────
        "color" => style.color = if inh { inherited.color } else { init.color },
        // `font-size` сюда не доходит: `apply_declaration` отсекает его до
        // keyword-ветки (BUG-731) — размер целиком считает pre-pass
        // `apply_font_size`, который один видит весь каскад, включая
        // `font`-shorthand.
        "line-height" => {
            style.line_height = if inh { inherited.line_height } else { init.line_height };
            style.line_height_is_relative = if inh {
                inherited.line_height_is_relative
            } else {
                init.line_height_is_relative
            };
        }
        "line-height-step" => {
            style.line_height_step =
                if inh { inherited.line_height_step } else { init.line_height_step };
        }
        "font-style" => {
            style.font_style = if inh { inherited.font_style } else { init.font_style };
        }
        "font-weight" => {
            style.font_weight = if inh { inherited.font_weight } else { init.font_weight };
        }
        "font-variant" | "font-variant-caps" => {
            style.font_variant_caps = if inh { inherited.font_variant_caps } else { init.font_variant_caps };
            if prop == "font-variant" {
                style.font_variant_emoji =
                    if inh { inherited.font_variant_emoji } else { init.font_variant_emoji };
            }
        }
        "font-variant-emoji" => {
            style.font_variant_emoji =
                if inh { inherited.font_variant_emoji } else { init.font_variant_emoji };
        }
        "font-stretch" => {
            style.font_stretch = if inh { inherited.font_stretch } else { init.font_stretch };
        }
        "font-family" => {
            style.font_family = if inh {
                inherited.font_family.clone()
            } else {
                init.font_family.clone()
            };
        }
        "font-variation-settings" => {
            style.font_variation_settings = if inh {
                inherited.font_variation_settings.clone()
            } else {
                init.font_variation_settings.clone()
            };
        }
        "font-feature-settings" => {
            style.font_feature_settings = if inh {
                inherited.font_feature_settings.clone()
            } else {
                init.font_feature_settings.clone()
            };
        }
        "font-palette" => {
            style.font_palette =
                if inh { inherited.font_palette.clone() } else { init.font_palette.clone() };
            style.font_palette_resolved = if inh {
                inherited.font_palette_resolved.clone()
            } else {
                init.font_palette_resolved.clone()
            };
        }
        "font-optical-sizing" => {
            style.font_optical_sizing =
                if inh { inherited.font_optical_sizing } else { init.font_optical_sizing };
        }
        "text-align" => {
            style.text_align = if inh { inherited.text_align } else { init.text_align };
        }
        "text-align-last" => {
            style.text_align_last = if inh_only_inherit {
                inherited.text_align_last
            } else {
                init.text_align_last
            };
        }
        "interpolate-size" => {
            style.interpolate_size =
                if inh { inherited.interpolate_size } else { init.interpolate_size };
        }
        "direction" => {
            style.direction = if inh { inherited.direction } else { init.direction };
        }
        "unicode-bidi" => {
            // Не наследуемое: голый `inherit` берёт родителя, `unset`/`initial`
            // возвращают `normal`.
            style.unicode_bidi = if inh_only_inherit {
                inherited.unicode_bidi
            } else {
                init.unicode_bidi
            };
        }
        "text-transform" => {
            style.text_transform = if inh { inherited.text_transform } else { init.text_transform };
        }
        "white-space" => {
            // L4 §2.1: shorthand — CSS-wide keyword применяется к обеим
            // longhand-компонентам.
            style.white_space = if inh { inherited.white_space } else { init.white_space };
            style.white_space_collapse =
                if inh { inherited.white_space_collapse } else { init.white_space_collapse };
            style.text_wrap_mode =
                if inh { inherited.text_wrap_mode } else { init.text_wrap_mode };
        }
        "white-space-collapse" => {
            style.white_space_collapse =
                if inh { inherited.white_space_collapse } else { init.white_space_collapse };
            style.white_space =
                WhiteSpace::combine(style.white_space_collapse, style.text_wrap_mode);
        }
        "text-indent" => {
            style.text_indent = if inh { inherited.text_indent.clone() } else { init.text_indent.clone() };
        }
        "letter-spacing" => {
            style.letter_spacing = if inh { inherited.letter_spacing } else { init.letter_spacing };
        }
        "word-spacing" => {
            style.word_spacing = if inh { inherited.word_spacing } else { init.word_spacing };
        }
        "text-decoration-line" | "text-decoration" => {
            style.text_decoration_line = if inh {
                inherited.text_decoration_line
            } else {
                init.text_decoration_line
            };
            style.text_decoration_color = if inh {
                inherited.text_decoration_color
            } else {
                init.text_decoration_color
            };
            // L3 shorthand сбрасывает также style (но не thickness — он
            // исключён из L3 shorthand-а; см. parse_text_decoration_shorthand_q).
            if prop == "text-decoration" {
                style.text_decoration_style = if inh {
                    inherited.text_decoration_style
                } else {
                    init.text_decoration_style
                };
            }
        }
        "text-decoration-color" => {
            style.text_decoration_color = if inh {
                inherited.text_decoration_color
            } else {
                init.text_decoration_color
            };
        }
        "text-decoration-style" => {
            style.text_decoration_style = if inh {
                inherited.text_decoration_style
            } else {
                init.text_decoration_style
            };
        }
        "text-decoration-thickness" => {
            style.text_decoration_thickness = if inh {
                inherited.text_decoration_thickness
            } else {
                init.text_decoration_thickness
            };
        }
        "text-emphasis-style" | "text-emphasis" => {
            style.text_emphasis_style = if inh {
                inherited.text_emphasis_style.clone()
            } else {
                init.text_emphasis_style.clone()
            };
            if prop == "text-emphasis" {
                style.text_emphasis_color = if inh {
                    inherited.text_emphasis_color
                } else {
                    init.text_emphasis_color
                };
            }
        }
        "text-emphasis-color" => {
            style.text_emphasis_color = if inh {
                inherited.text_emphasis_color
            } else {
                init.text_emphasis_color
            };
        }
        "text-emphasis-position" => {
            style.text_emphasis_position = if inh {
                inherited.text_emphasis_position
            } else {
                init.text_emphasis_position
            };
        }
        "text-underline-position" => {
            style.text_underline_position = if inh {
                inherited.text_underline_position
            } else {
                init.text_underline_position
            };
        }
        "text-underline-offset" => {
            style.text_underline_offset = if inh {
                inherited.text_underline_offset
            } else {
                init.text_underline_offset
            };
        }
        "text-decoration-skip-ink" => {
            style.text_decoration_skip_ink = if inh {
                inherited.text_decoration_skip_ink
            } else {
                init.text_decoration_skip_ink
            };
        }
        "text-shadow" => {
            style.text_shadow = if inh {
                inherited.text_shadow.clone()
            } else {
                init.text_shadow.clone()
            };
        }
        "visibility" => {
            style.visibility = if inh { inherited.visibility } else { init.visibility };
        }
        "cursor" => {
            style.cursor = if inh { inherited.cursor } else { init.cursor };
        }
        "writing-mode" => {
            style.writing_mode = if inh { inherited.writing_mode } else { init.writing_mode };
        }
        "text-orientation" => {
            style.text_orientation = if inh {
                inherited.text_orientation
            } else {
                init.text_orientation
            };
        }
        "ruby-position" => {
            style.ruby_position = if inh { inherited.ruby_position } else { init.ruby_position };
        }
        "ruby-align" => {
            style.ruby_align = if inh { inherited.ruby_align } else { init.ruby_align };
        }
        "ruby-merge" => {
            style.ruby_merge = if inh { inherited.ruby_merge } else { init.ruby_merge };
        }
        "math-style" => {
            style.math_style = if inh { inherited.math_style } else { init.math_style };
        }
        "math-depth" => {
            style.math_depth = if inh { inherited.math_depth } else { init.math_depth };
        }
        "accent-color" => {
            style.accent_color = if inh { inherited.accent_color } else { init.accent_color };
        }
        "color-scheme" => {
            style.color_scheme = if inh { inherited.color_scheme } else { init.color_scheme };
        }
        "line-break" => {
            style.line_break = if inh { inherited.line_break } else { init.line_break };
        }

        // ──────── Non-inherited properties ────────
        "resize" => {
            style.resize = if inh_only_inherit { inherited.resize } else { init.resize };
        }
        "touch-action" => {
            style.touch_action = if inh_only_inherit { inherited.touch_action } else { init.touch_action };
        }
        "appearance" | "-webkit-appearance" | "-moz-appearance" => {
            style.appearance = if inh_only_inherit { inherited.appearance } else { init.appearance };
        }
        "contain" => {
            style.contain = if inh_only_inherit { inherited.contain } else { init.contain };
        }
        "content-visibility" => {
            style.content_visibility = if inh_only_inherit {
                inherited.content_visibility
            } else {
                init.content_visibility
            };
        }
        "contain-intrinsic-width" | "contain-intrinsic-inline-size" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.contain_intrinsic_width = src.contain_intrinsic_width.clone();
            style.contain_intrinsic_width_auto = src.contain_intrinsic_width_auto;
        }
        "contain-intrinsic-height" | "contain-intrinsic-block-size" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.contain_intrinsic_height = src.contain_intrinsic_height.clone();
            style.contain_intrinsic_height_auto = src.contain_intrinsic_height_auto;
        }
        "contain-intrinsic-size" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.contain_intrinsic_width = src.contain_intrinsic_width.clone();
            style.contain_intrinsic_width_auto = src.contain_intrinsic_width_auto;
            style.contain_intrinsic_height = src.contain_intrinsic_height.clone();
            style.contain_intrinsic_height_auto = src.contain_intrinsic_height_auto;
        }
        "container-type" => {
            style.container_type = if inh_only_inherit {
                inherited.container_type
            } else {
                init.container_type
            };
        }
        "container-name" | "container" => {
            style.container_name = if inh_only_inherit {
                inherited.container_name.clone()
            } else {
                init.container_name.clone()
            };
        }
        "backdrop-filter" => {
            style.backdrop_filter = if inh_only_inherit {
                inherited.backdrop_filter.clone()
            } else {
                init.backdrop_filter.clone()
            };
        }
        "print-color-adjust" | "color-adjust" => {
            style.print_color_adjust = if inh_only_inherit {
                inherited.print_color_adjust
            } else {
                init.print_color_adjust
            };
        }
        "font-size-adjust" => {
            style.font_size_adjust = if inh_only_inherit {
                inherited.font_size_adjust
            } else {
                init.font_size_adjust
            };
        }
        "shape-outside" => {
            style.shape_outside = if inh_only_inherit {
                inherited.shape_outside.clone()
            } else {
                init.shape_outside.clone()
            };
        }
        "shape-margin" => {
            style.shape_margin = if inh_only_inherit { inherited.shape_margin.clone() } else { init.shape_margin.clone() };
        }
        "shape-image-threshold" => {
            style.shape_image_threshold = if inh_only_inherit { inherited.shape_image_threshold } else { init.shape_image_threshold };
        }
        "offset-path" => {
            style.offset_path = if inh_only_inherit {
                inherited.offset_path.clone()
            } else {
                init.offset_path.clone()
            };
        }
        "offset-distance" => {
            style.offset_distance = if inh_only_inherit { inherited.offset_distance.clone() } else { init.offset_distance.clone() };
        }
        "offset-rotate" => {
            style.offset_rotate = if inh_only_inherit { inherited.offset_rotate } else { init.offset_rotate };
        }
        "offset-anchor" => {
            style.offset_anchor = if inh_only_inherit { inherited.offset_anchor } else { init.offset_anchor };
        }
        "forced-color-adjust" => {
            // Inherited property: `unset`/`revert` behave as `inherit`.
            style.forced_color_adjust = if inh {
                inherited.forced_color_adjust
            } else {
                init.forced_color_adjust
            };
        }
        "display" => {
            style.display = if inh_only_inherit { inherited.display } else { init.display };
        }
        "background-color" => {
            style.background_color = if inh_only_inherit {
                inherited.background_color
            } else {
                init.background_color
            };
        }
        "background" => {
            style.background_color = if inh_only_inherit {
                inherited.background_color
            } else {
                init.background_color
            };
            style.background_layers = if inh_only_inherit {
                inherited.background_layers.clone()
            } else {
                Vec::new()
            };
        }
        "width" => style.width = if inh_only_inherit { inherited.width.clone() } else { init.width.clone() },
        "height" => style.height = if inh_only_inherit { inherited.height.clone() } else { init.height.clone() },
        // CSS Logical Properties L1 — inline-size / block-size.
        "inline-size" => style.inline_size = if inh_only_inherit { inherited.inline_size.clone() } else { init.inline_size.clone() },
        "block-size" => style.block_size = if inh_only_inherit { inherited.block_size.clone() } else { init.block_size.clone() },
        "min-width" => style.min_width = if inh_only_inherit { inherited.min_width.clone() } else { init.min_width.clone() },
        "max-width" => style.max_width = if inh_only_inherit { inherited.max_width.clone() } else { init.max_width.clone() },
        "min-height" => style.min_height = if inh_only_inherit { inherited.min_height.clone() } else { init.min_height.clone() },
        "max-height" => style.max_height = if inh_only_inherit { inherited.max_height.clone() } else { init.max_height.clone() },
        "margin-top" => style.margin_top = if inh_only_inherit { inherited.margin_top.clone() } else { init.margin_top.clone() },
        "margin-right" => style.margin_right = if inh_only_inherit { inherited.margin_right.clone() } else { init.margin_right.clone() },
        "margin-bottom" => style.margin_bottom = if inh_only_inherit { inherited.margin_bottom.clone() } else { init.margin_bottom.clone() },
        "margin-left" => style.margin_left = if inh_only_inherit { inherited.margin_left.clone() } else { init.margin_left.clone() },
        "margin" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.margin_top = src.margin_top.clone();
            style.margin_right = src.margin_right.clone();
            style.margin_bottom = src.margin_bottom.clone();
            style.margin_left = src.margin_left.clone();
        }
        // CSS Logical Properties L1 — margin-inline-* / margin-block-*.
        "margin-inline-start" => style.margin_inline_start = if inh_only_inherit { inherited.margin_inline_start.clone() } else { init.margin_inline_start.clone() },
        "margin-inline-end"   => style.margin_inline_end = if inh_only_inherit { inherited.margin_inline_end.clone() } else { init.margin_inline_end.clone() },
        "margin-block-start"  => style.margin_block_start = if inh_only_inherit { inherited.margin_block_start.clone() } else { init.margin_block_start.clone() },
        "margin-block-end"    => style.margin_block_end = if inh_only_inherit { inherited.margin_block_end.clone() } else { init.margin_block_end.clone() },
        "margin-inline" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.margin_inline_start = src.margin_inline_start.clone();
            style.margin_inline_end = src.margin_inline_end.clone();
        }
        "margin-block" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.margin_block_start = src.margin_block_start.clone();
            style.margin_block_end = src.margin_block_end.clone();
        }
        "padding-top" => style.padding_top = if inh_only_inherit { inherited.padding_top.clone() } else { init.padding_top.clone() },
        "padding-right" => style.padding_right = if inh_only_inherit { inherited.padding_right.clone() } else { init.padding_right.clone() },
        "padding-bottom" => style.padding_bottom = if inh_only_inherit { inherited.padding_bottom.clone() } else { init.padding_bottom.clone() },
        "padding-left" => style.padding_left = if inh_only_inherit { inherited.padding_left.clone() } else { init.padding_left.clone() },
        "padding" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.padding_top = src.padding_top.clone();
            style.padding_right = src.padding_right.clone();
            style.padding_bottom = src.padding_bottom.clone();
            style.padding_left = src.padding_left.clone();
        }
        // CSS Logical Properties L1 — padding-inline-* / padding-block-*.
        "padding-inline-start" => style.padding_inline_start = if inh_only_inherit { inherited.padding_inline_start.clone() } else { init.padding_inline_start.clone() },
        "padding-inline-end"   => style.padding_inline_end = if inh_only_inherit { inherited.padding_inline_end.clone() } else { init.padding_inline_end.clone() },
        "padding-block-start"  => style.padding_top = if inh_only_inherit { inherited.padding_top.clone() } else { init.padding_top.clone() },
        "padding-block-end"    => style.padding_bottom = if inh_only_inherit { inherited.padding_bottom.clone() } else { init.padding_bottom.clone() },
        "padding-inline" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.padding_left = src.padding_left.clone();
            style.padding_right = src.padding_right.clone();
        }
        "padding-block" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.padding_top = src.padding_top.clone();
            style.padding_bottom = src.padding_bottom.clone();
        }
        "box-sizing" => {
            style.box_sizing = if inh_only_inherit { inherited.box_sizing } else { init.box_sizing };
        }
        "opacity" => {
            style.opacity = if inh_only_inherit { inherited.opacity } else { init.opacity };
        }
        "fill" => {
            style.svg_fill = if inh_only_inherit { inherited.svg_fill } else { init.svg_fill };
        }
        "fill-opacity" => {
            style.svg_fill_opacity = if inh_only_inherit { inherited.svg_fill_opacity } else { init.svg_fill_opacity };
        }
        "stroke" => {
            style.svg_stroke = if inh_only_inherit { inherited.svg_stroke } else { init.svg_stroke };
        }
        "stroke-opacity" => {
            style.svg_stroke_opacity = if inh_only_inherit { inherited.svg_stroke_opacity } else { init.svg_stroke_opacity };
        }
        "stroke-width" => {
            style.svg_stroke_width = if inh_only_inherit { inherited.svg_stroke_width } else { init.svg_stroke_width };
        }
        "fill-rule" => {
            style.svg_fill_rule = if inh_only_inherit { inherited.svg_fill_rule } else { init.svg_fill_rule };
        }
        "clip-rule" => {
            style.svg_clip_rule = if inh_only_inherit { inherited.svg_clip_rule } else { init.svg_clip_rule };
        }
        "stroke-linecap" => {
            style.svg_stroke_linecap = if inh_only_inherit { inherited.svg_stroke_linecap } else { init.svg_stroke_linecap };
        }
        "stroke-linejoin" => {
            style.svg_stroke_linejoin = if inh_only_inherit { inherited.svg_stroke_linejoin } else { init.svg_stroke_linejoin };
        }
        "stroke-miterlimit" => {
            style.svg_stroke_miterlimit = if inh_only_inherit { inherited.svg_stroke_miterlimit } else { init.svg_stroke_miterlimit };
        }
        "stroke-dasharray" => {
            style.svg_stroke_dasharray = if inh_only_inherit { inherited.svg_stroke_dasharray.clone() } else { init.svg_stroke_dasharray.clone() };
        }
        "stroke-dashoffset" => {
            style.svg_stroke_dashoffset = if inh_only_inherit { inherited.svg_stroke_dashoffset } else { init.svg_stroke_dashoffset };
        }
        // CSS Fill & Stroke L3 §6 — paint-order is inherited: unset/revert → inherited.
        "paint-order" => {
            style.paint_order = if inh { inherited.paint_order } else { init.paint_order };
        }
        // SVG text-anchor / dominant-baseline are inherited: unset/revert → inherited.
        "text-anchor" => {
            style.text_anchor = if inh { inherited.text_anchor } else { init.text_anchor };
        }
        "dominant-baseline" => {
            style.dominant_baseline = if inh { inherited.dominant_baseline } else { init.dominant_baseline };
        }
        // SVG baseline-shift is NOT inherited: inherit → parent, else → initial.
        "baseline-shift" => {
            style.baseline_shift = if inh_only_inherit { inherited.baseline_shift } else { init.baseline_shift };
        }
        "overflow" => {
            let (x, y) = if inh_only_inherit {
                (inherited.overflow_x, inherited.overflow_y)
            } else {
                (init.overflow_x, init.overflow_y)
            };
            style.overflow_x = x;
            style.overflow_y = y;
        }
        "overflow-x" => {
            style.overflow_x = if inh_only_inherit { inherited.overflow_x } else { init.overflow_x };
        }
        "overflow-y" => {
            style.overflow_y = if inh_only_inherit { inherited.overflow_y } else { init.overflow_y };
        }
        "text-overflow" => {
            style.text_overflow = if inh_only_inherit { inherited.text_overflow } else { init.text_overflow };
        }
        "-webkit-line-clamp" | "line-clamp" => {
            style.line_clamp = if inh_only_inherit { inherited.line_clamp } else { init.line_clamp };
        }
        "box-shadow" => {
            style.box_shadow = if inh_only_inherit {
                inherited.box_shadow.clone()
            } else {
                init.box_shadow.clone()
            };
        }
        "outline-width" => {
            style.outline_width = if inh_only_inherit { inherited.outline_width } else { init.outline_width };
        }
        "outline-style" => {
            style.outline_style = if inh_only_inherit { inherited.outline_style } else { init.outline_style };
        }
        "outline-color" => {
            style.outline_color = if inh_only_inherit { inherited.outline_color } else { init.outline_color };
        }
        "outline-offset" => {
            style.outline_offset = if inh_only_inherit { inherited.outline_offset.clone() } else { init.outline_offset.clone() };
        }
        "outline" => {
            // shorthand: width + style + color (offset не входит per spec).
            if inh_only_inherit {
                style.outline_width = inherited.outline_width;
                style.outline_style = inherited.outline_style;
                style.outline_color = inherited.outline_color;
            } else {
                style.outline_width = init.outline_width;
                style.outline_style = init.outline_style;
                style.outline_color = init.outline_color;
            }
        }
        // border-* per-side individual + shorthands
        "border-top-width" => style.border_top_width = if inh_only_inherit { inherited.border_top_width } else { init.border_top_width },
        "border-right-width" => style.border_right_width = if inh_only_inherit { inherited.border_right_width } else { init.border_right_width },
        "border-bottom-width" => style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width },
        "border-left-width" => style.border_left_width = if inh_only_inherit { inherited.border_left_width } else { init.border_left_width },
        "border-top-style" => style.border_top_style = if inh_only_inherit { inherited.border_top_style } else { init.border_top_style },
        "border-right-style" => style.border_right_style = if inh_only_inherit { inherited.border_right_style } else { init.border_right_style },
        "border-bottom-style" => style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style },
        "border-left-style" => style.border_left_style = if inh_only_inherit { inherited.border_left_style } else { init.border_left_style },
        "border-top-color" => style.border_top_color = if inh_only_inherit { inherited.border_top_color } else { init.border_top_color },
        "border-right-color" => style.border_right_color = if inh_only_inherit { inherited.border_right_color } else { init.border_right_color },
        "border-bottom-color" => style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color },
        "border-left-color" => style.border_left_color = if inh_only_inherit { inherited.border_left_color } else { init.border_left_color },
        // border-width / -style / -color shorthand → 4 стороны
        "border-width" => {
            let v = if inh_only_inherit {
                (inherited.border_top_width, inherited.border_right_width, inherited.border_bottom_width, inherited.border_left_width)
            } else {
                (init.border_top_width, init.border_right_width, init.border_bottom_width, init.border_left_width)
            };
            style.border_top_width = v.0;
            style.border_right_width = v.1;
            style.border_bottom_width = v.2;
            style.border_left_width = v.3;
        }
        "border-style" => {
            let v = if inh_only_inherit {
                (inherited.border_top_style, inherited.border_right_style, inherited.border_bottom_style, inherited.border_left_style)
            } else {
                (init.border_top_style, init.border_right_style, init.border_bottom_style, init.border_left_style)
            };
            style.border_top_style = v.0;
            style.border_right_style = v.1;
            style.border_bottom_style = v.2;
            style.border_left_style = v.3;
        }
        "border-color" => {
            let v = if inh_only_inherit {
                (inherited.border_top_color, inherited.border_right_color, inherited.border_bottom_color, inherited.border_left_color)
            } else {
                (init.border_top_color, init.border_right_color, init.border_bottom_color, init.border_left_color)
            };
            style.border_top_color = v.0;
            style.border_right_color = v.1;
            style.border_bottom_color = v.2;
            style.border_left_color = v.3;
        }
        "border-collapse" => {
            style.border_collapse = if inh_only_inherit { inherited.border_collapse } else { init.border_collapse };
        }
        "empty-cells" => {
            style.empty_cells = if inh_only_inherit { inherited.empty_cells } else { init.empty_cells };
        }
        "border-spacing" => {
            style.border_spacing_h = if inh_only_inherit { inherited.border_spacing_h } else { init.border_spacing_h };
            style.border_spacing_v = if inh_only_inherit { inherited.border_spacing_v } else { init.border_spacing_v };
        }
        // border / border-top / -right / -bottom / -left shorthand: width + style + color на сторону.
        "border" => {
            let (w, s, c) = if inh_only_inherit {
                (inherited.border_top_width, inherited.border_top_style, inherited.border_top_color)
            } else {
                (init.border_top_width, init.border_top_style, init.border_top_color)
            };
            for (sw, ss, sc) in [
                (&mut style.border_top_width, &mut style.border_top_style, &mut style.border_top_color),
                (&mut style.border_right_width, &mut style.border_right_style, &mut style.border_right_color),
                (&mut style.border_bottom_width, &mut style.border_bottom_style, &mut style.border_bottom_color),
                (&mut style.border_left_width, &mut style.border_left_style, &mut style.border_left_color),
            ] {
                *sw = w;
                *ss = s;
                *sc = c;
            }
        }
        "border-top" => {
            style.border_top_width = if inh_only_inherit { inherited.border_top_width } else { init.border_top_width };
            style.border_top_style = if inh_only_inherit { inherited.border_top_style } else { init.border_top_style };
            style.border_top_color = if inh_only_inherit { inherited.border_top_color } else { init.border_top_color };
        }
        "border-right" => {
            style.border_right_width = if inh_only_inherit { inherited.border_right_width } else { init.border_right_width };
            style.border_right_style = if inh_only_inherit { inherited.border_right_style } else { init.border_right_style };
            style.border_right_color = if inh_only_inherit { inherited.border_right_color } else { init.border_right_color };
        }
        "border-bottom" => {
            style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width };
            style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style };
            style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color };
        }
        "border-left" => {
            style.border_left_width = if inh_only_inherit { inherited.border_left_width } else { init.border_left_width };
            style.border_left_style = if inh_only_inherit { inherited.border_left_style } else { init.border_left_style };
            style.border_left_color = if inh_only_inherit { inherited.border_left_color } else { init.border_left_color };
        }
        // CSS Logical Properties L1 §6.3 — border-inline-* / border-block-* CSS-wide.
        "border-inline-start" => {
            style.border_left_width = if inh_only_inherit { inherited.border_left_width } else { init.border_left_width };
            style.border_left_style = if inh_only_inherit { inherited.border_left_style } else { init.border_left_style };
            style.border_left_color = if inh_only_inherit { inherited.border_left_color } else { init.border_left_color };
        }
        "border-inline-end" => {
            style.border_right_width = if inh_only_inherit { inherited.border_right_width } else { init.border_right_width };
            style.border_right_style = if inh_only_inherit { inherited.border_right_style } else { init.border_right_style };
            style.border_right_color = if inh_only_inherit { inherited.border_right_color } else { init.border_right_color };
        }
        "border-block-start" => {
            style.border_top_width = if inh_only_inherit { inherited.border_top_width } else { init.border_top_width };
            style.border_top_style = if inh_only_inherit { inherited.border_top_style } else { init.border_top_style };
            style.border_top_color = if inh_only_inherit { inherited.border_top_color } else { init.border_top_color };
        }
        "border-block-end" => {
            style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width };
            style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style };
            style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color };
        }
        "border-inline" => {
            style.border_left_width  = if inh_only_inherit { inherited.border_left_width  } else { init.border_left_width  };
            style.border_left_style  = if inh_only_inherit { inherited.border_left_style  } else { init.border_left_style  };
            style.border_left_color  = if inh_only_inherit { inherited.border_left_color  } else { init.border_left_color  };
            style.border_right_width = if inh_only_inherit { inherited.border_right_width } else { init.border_right_width };
            style.border_right_style = if inh_only_inherit { inherited.border_right_style } else { init.border_right_style };
            style.border_right_color = if inh_only_inherit { inherited.border_right_color } else { init.border_right_color };
        }
        "border-block" => {
            style.border_top_width    = if inh_only_inherit { inherited.border_top_width    } else { init.border_top_width    };
            style.border_top_style    = if inh_only_inherit { inherited.border_top_style    } else { init.border_top_style    };
            style.border_top_color    = if inh_only_inherit { inherited.border_top_color    } else { init.border_top_color    };
            style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width };
            style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style };
            style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color };
        }
        "border-inline-start-width" => style.border_left_width   = if inh_only_inherit { inherited.border_left_width   } else { init.border_left_width   },
        "border-inline-end-width"   => style.border_right_width  = if inh_only_inherit { inherited.border_right_width  } else { init.border_right_width  },
        "border-block-start-width"  => style.border_top_width    = if inh_only_inherit { inherited.border_top_width    } else { init.border_top_width    },
        "border-block-end-width"    => style.border_bottom_width = if inh_only_inherit { inherited.border_bottom_width } else { init.border_bottom_width },
        "border-inline-start-style" => style.border_left_style   = if inh_only_inherit { inherited.border_left_style   } else { init.border_left_style   },
        "border-inline-end-style"   => style.border_right_style  = if inh_only_inherit { inherited.border_right_style  } else { init.border_right_style  },
        "border-block-start-style"  => style.border_top_style    = if inh_only_inherit { inherited.border_top_style    } else { init.border_top_style    },
        "border-block-end-style"    => style.border_bottom_style = if inh_only_inherit { inherited.border_bottom_style } else { init.border_bottom_style },
        "border-inline-start-color" => style.border_left_color   = if inh_only_inherit { inherited.border_left_color   } else { init.border_left_color   },
        "border-inline-end-color"   => style.border_right_color  = if inh_only_inherit { inherited.border_right_color  } else { init.border_right_color  },
        "border-block-start-color"  => style.border_top_color    = if inh_only_inherit { inherited.border_top_color    } else { init.border_top_color    },
        "border-block-end-color"    => style.border_bottom_color = if inh_only_inherit { inherited.border_bottom_color } else { init.border_bottom_color },
        // border-radius (CSS Backgrounds L3 §5) — 4 угла, x и y.
        "border-top-left-radius" => {
            style.border_top_left_radius   = if inh_only_inherit { inherited.border_top_left_radius.clone()   } else { init.border_top_left_radius.clone()   };
            style.border_top_left_radius_y = if inh_only_inherit { inherited.border_top_left_radius_y.clone() } else { init.border_top_left_radius_y.clone() };
        }
        "border-top-right-radius" => {
            style.border_top_right_radius   = if inh_only_inherit { inherited.border_top_right_radius.clone()   } else { init.border_top_right_radius.clone()   };
            style.border_top_right_radius_y = if inh_only_inherit { inherited.border_top_right_radius_y.clone() } else { init.border_top_right_radius_y.clone() };
        }
        "border-bottom-right-radius" => {
            style.border_bottom_right_radius   = if inh_only_inherit { inherited.border_bottom_right_radius.clone()   } else { init.border_bottom_right_radius.clone()   };
            style.border_bottom_right_radius_y = if inh_only_inherit { inherited.border_bottom_right_radius_y.clone() } else { init.border_bottom_right_radius_y.clone() };
        }
        "border-bottom-left-radius" => {
            style.border_bottom_left_radius   = if inh_only_inherit { inherited.border_bottom_left_radius.clone()   } else { init.border_bottom_left_radius.clone()   };
            style.border_bottom_left_radius_y = if inh_only_inherit { inherited.border_bottom_left_radius_y.clone() } else { init.border_bottom_left_radius_y.clone() };
        }
        "border-radius" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.border_top_left_radius       = src.border_top_left_radius.clone();
            style.border_top_right_radius      = src.border_top_right_radius.clone();
            style.border_bottom_right_radius   = src.border_bottom_right_radius.clone();
            style.border_bottom_left_radius    = src.border_bottom_left_radius.clone();
            style.border_top_left_radius_y     = src.border_top_left_radius_y.clone();
            style.border_top_right_radius_y    = src.border_top_right_radius_y.clone();
            style.border_bottom_right_radius_y = src.border_bottom_right_radius_y.clone();
            style.border_bottom_left_radius_y  = src.border_bottom_left_radius_y.clone();
        }
        // CSS Lists L3 §3 — не наследуются; Inherit пуллит из inherited,
        // прочие — initial (пустой Vec).
        "counter-reset" => {
            style.counter_reset = if inh_only_inherit {
                inherited.counter_reset.clone()
            } else {
                init.counter_reset.clone()
            };
        }
        "counter-increment" => {
            style.counter_increment = if inh_only_inherit {
                inherited.counter_increment.clone()
            } else {
                init.counter_increment.clone()
            };
        }
        "counter-set" => {
            style.counter_set = if inh_only_inherit {
                inherited.counter_set.clone()
            } else {
                init.counter_set.clone()
            };
        }
        // CSS Generated Content L3 §3.2 — quotes inherited.
        "quotes" => {
            style.quotes = if inh { inherited.quotes.clone() } else { init.quotes.clone() };
        }
        // Masking / Transforms / Filter — все non-inherited.
        "clip-path" => {
            style.clip_path = if inh_only_inherit { inherited.clip_path.clone() } else { init.clip_path.clone() };
        }
        "transform" => {
            style.transform = if inh_only_inherit { inherited.transform.clone() } else { init.transform.clone() };
        }
        "translate" => {
            style.translate = if inh_only_inherit { inherited.translate } else { init.translate };
        }
        "rotate" => {
            style.rotate = if inh_only_inherit { inherited.rotate } else { init.rotate };
        }
        "scale" => {
            style.scale = if inh_only_inherit { inherited.scale } else { init.scale };
        }
        "filter" => {
            style.filter = if inh_only_inherit { inherited.filter.clone() } else { init.filter.clone() };
        }
        // CSS Positioned Layout / Compositing — non-inherited.
        "position" => {
            style.position = if inh_only_inherit { inherited.position } else { init.position };
        }
        // CSS Anchor Positioning L1 — non-inherited properties.
        "anchor-name" => {
            style.anchor_name = if inh_only_inherit { inherited.anchor_name.clone() } else { None };
        }
        "position-anchor" => {
            style.position_anchor = if inh_only_inherit { inherited.position_anchor.clone() } else { None };
        }
        "inset-area" | "position-area" => {
            style.inset_area_row = if inh_only_inherit { inherited.inset_area_row } else { init.inset_area_row };
            style.inset_area_col = if inh_only_inherit { inherited.inset_area_col } else { init.inset_area_col };
        }
        "anchor-scope" => {
            style.anchor_scope = if inh_only_inherit { inherited.anchor_scope.clone() } else { init.anchor_scope.clone() };
        }
        "view-transition-name" => {
            style.view_transition_name = if inh_only_inherit { inherited.view_transition_name.clone() } else { None };
        }
        "top" => {
            style.top = if inh_only_inherit { inherited.top.clone() } else { init.top.clone() };
            style.anchor_top = if inh_only_inherit { inherited.anchor_top.clone() } else { None };
        }
        "right" => {
            style.right = if inh_only_inherit { inherited.right.clone() } else { init.right.clone() };
            style.anchor_right = if inh_only_inherit { inherited.anchor_right.clone() } else { None };
        }
        "bottom" => {
            style.bottom = if inh_only_inherit { inherited.bottom.clone() } else { init.bottom.clone() };
            style.anchor_bottom = if inh_only_inherit { inherited.anchor_bottom.clone() } else { None };
        }
        "left" => {
            style.left = if inh_only_inherit { inherited.left.clone() } else { init.left.clone() };
            style.anchor_left = if inh_only_inherit { inherited.anchor_left.clone() } else { None };
        }
        "inset" => {
            style.top = if inh_only_inherit { inherited.top.clone() } else { init.top.clone() };
            style.right = if inh_only_inherit { inherited.right.clone() } else { init.right.clone() };
            style.bottom = if inh_only_inherit { inherited.bottom.clone() } else { init.bottom.clone() };
            style.left = if inh_only_inherit { inherited.left.clone() } else { init.left.clone() };
            style.anchor_top = if inh_only_inherit { inherited.anchor_top.clone() } else { None };
            style.anchor_right = if inh_only_inherit { inherited.anchor_right.clone() } else { None };
            style.anchor_bottom = if inh_only_inherit { inherited.anchor_bottom.clone() } else { None };
            style.anchor_left = if inh_only_inherit { inherited.anchor_left.clone() } else { None };
        }
        // CSS Logical Properties L1 — inset-inline-* / inset-block-*.
        "inset-inline-start" => style.inset_inline_start = if inh_only_inherit { inherited.inset_inline_start.clone() } else { init.inset_inline_start.clone() },
        "inset-inline-end"   => style.inset_inline_end = if inh_only_inherit { inherited.inset_inline_end.clone() } else { init.inset_inline_end.clone() },
        "inset-block-start"  => style.inset_block_start = if inh_only_inherit { inherited.inset_block_start.clone() } else { init.inset_block_start.clone() },
        "inset-block-end"    => style.inset_block_end = if inh_only_inherit { inherited.inset_block_end.clone() } else { init.inset_block_end.clone() },
        "inset-inline" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.inset_inline_start = src.inset_inline_start.clone();
            style.inset_inline_end = src.inset_inline_end.clone();
        }
        "inset-block" => {
            let src = if inh_only_inherit { inherited } else { &init };
            style.inset_block_start = src.inset_block_start.clone();
            style.inset_block_end = src.inset_block_end.clone();
        }
        "z-index" => {
            style.z_index = if inh_only_inherit { inherited.z_index } else { init.z_index };
        }
        "float" => {
            style.float_side = if inh_only_inherit { inherited.float_side } else { init.float_side };
        }
        "clear" => {
            style.clear = if inh_only_inherit { inherited.clear } else { init.clear };
        }
        "isolation" => {
            style.isolation = if inh_only_inherit { inherited.isolation } else { init.isolation };
        }
        "mix-blend-mode" => {
            style.mix_blend_mode = if inh_only_inherit { inherited.mix_blend_mode } else { init.mix_blend_mode };
        }
        // CSS Images L3 §5.5 — object-fit / object-position non-inherited.
        "object-fit" => {
            style.object_fit = if inh_only_inherit { inherited.object_fit } else { init.object_fit };
        }
        "object-position" => {
            style.object_position = if inh_only_inherit {
                inherited.object_position
            } else {
                init.object_position
            };
        }
        // CSS 2.1 §10.8.1 — vertical-align non-inherited.
        "vertical-align" => {
            style.vertical_align = if inh_only_inherit {
                inherited.vertical_align
            } else {
                init.vertical_align
            };
        }
        // CSS Backgrounds L3 §3 + CSS Compositing L1 §8.3 — background-* non-inherited.
        "background-position" | "background-origin" | "background-clip"
        | "background-size" | "background-repeat" | "background-attachment"
        | "background-image" | "background-blend-mode" => {
            // Все background-* не наследуются: initial = пустые слои.
            style.background_layers = if inh_only_inherit {
                inherited.background_layers.clone()
            } else {
                Vec::new()
            };
        }
        // CSS Images L3 §6.1 — image-rendering inherited. inh — общий
        // алиас «брать inherited.value» (для inherited работает и при
        // Inherit, и при Unset; см. вычисление inh выше).
        "image-rendering" => {
            style.image_rendering = if inh {
                inherited.image_rendering
            } else {
                init.image_rendering
            };
        }
        // CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style
        // оба inherited; shorthand text-wrap раскрывается на два longhand-а
        // и применяет CSS-wide ключевое слово к каждому.
        "text-wrap-mode" => {
            style.text_wrap_mode = if inh {
                inherited.text_wrap_mode
            } else {
                init.text_wrap_mode
            };
            style.white_space =
                WhiteSpace::combine(style.white_space_collapse, style.text_wrap_mode);
        }
        "text-wrap-style" => {
            style.text_wrap_style = if inh {
                inherited.text_wrap_style
            } else {
                init.text_wrap_style
            };
        }
        "text-wrap" => {
            style.text_wrap_mode = if inh {
                inherited.text_wrap_mode
            } else {
                init.text_wrap_mode
            };
            style.text_wrap_style = if inh {
                inherited.text_wrap_style
            } else {
                init.text_wrap_style
            };
            style.white_space =
                WhiteSpace::combine(style.white_space_collapse, style.text_wrap_mode);
        }
        // CSS Fragmentation L3 §3.3 — orphans / widows inherited.
        "orphans" => {
            style.orphans = if inh { inherited.orphans } else { init.orphans };
        }
        "widows" => {
            style.widows = if inh { inherited.widows } else { init.widows };
        }
        // CSS Flexbox L1 §7 — flex-grow / flex-shrink / flex-basis non-inherited.
        "flex-grow" => {
            style.flex_grow = if inh_only_inherit { inherited.flex_grow } else { init.flex_grow };
        }
        "flex-shrink" => {
            style.flex_shrink = if inh_only_inherit { inherited.flex_shrink } else { init.flex_shrink };
        }
        "flex-basis" => {
            style.flex_basis = if inh_only_inherit {
                inherited.flex_basis.clone()
            } else {
                init.flex_basis.clone()
            };
        }
        "order" => {
            style.order = if inh_only_inherit { inherited.order } else { init.order };
        }
        "flex" => {
            style.flex_grow = if inh_only_inherit { inherited.flex_grow } else { init.flex_grow };
            style.flex_shrink = if inh_only_inherit { inherited.flex_shrink } else { init.flex_shrink };
            style.flex_basis = if inh_only_inherit {
                inherited.flex_basis.clone()
            } else {
                init.flex_basis.clone()
            };
        }
        // CSS Flexbox L1 §5 — flex-direction / flex-wrap non-inherited.
        "flex-direction" => {
            style.flex_direction = if inh_only_inherit {
                inherited.flex_direction
            } else {
                init.flex_direction
            };
        }
        "flex-wrap" => {
            style.flex_wrap = if inh_only_inherit {
                inherited.flex_wrap
            } else {
                init.flex_wrap
            };
        }
        "flex-flow" => {
            style.flex_direction = if inh_only_inherit {
                inherited.flex_direction
            } else {
                init.flex_direction
            };
            style.flex_wrap = if inh_only_inherit {
                inherited.flex_wrap
            } else {
                init.flex_wrap
            };
        }
        "grid-template-columns" | "grid-template-rows" | "grid-template-areas"
        | "grid-auto-flow" | "grid-auto-columns" | "grid-auto-rows"
        | "grid-column-start" | "grid-column-end"
        | "grid-row-start" | "grid-row-end" | "grid-column" | "grid-row" | "grid-area"
        | "grid-template" | "grid" => {
            // None of the grid properties are inherited.
            if inh_only_inherit {
                // inherit: copy from parent (non-inherited → initial)
                style.grid_template_columns = init.grid_template_columns.clone();
                style.grid_template_rows = init.grid_template_rows.clone();
                style.grid_template_col_auto_repeat = init.grid_template_col_auto_repeat.clone();
                style.grid_template_row_auto_repeat = init.grid_template_row_auto_repeat.clone();
                style.grid_template_areas = init.grid_template_areas.clone();
                style.grid_auto_flow = init.grid_auto_flow;
                style.grid_auto_columns = init.grid_auto_columns.clone();
                style.grid_auto_rows = init.grid_auto_rows.clone();
                style.grid_column_start = init.grid_column_start.clone();
                style.grid_column_end = init.grid_column_end.clone();
                style.grid_row_start = init.grid_row_start.clone();
                style.grid_row_end = init.grid_row_end.clone();
            } else {
                style.grid_template_columns = init.grid_template_columns.clone();
                style.grid_template_rows = init.grid_template_rows.clone();
                style.grid_template_col_auto_repeat = init.grid_template_col_auto_repeat.clone();
                style.grid_template_row_auto_repeat = init.grid_template_row_auto_repeat.clone();
                style.grid_template_areas = init.grid_template_areas.clone();
                style.grid_auto_flow = init.grid_auto_flow;
                style.grid_auto_columns = init.grid_auto_columns.clone();
                style.grid_auto_rows = init.grid_auto_rows.clone();
                style.grid_column_start = init.grid_column_start.clone();
                style.grid_column_end = init.grid_column_end.clone();
                style.grid_row_start = init.grid_row_start.clone();
                style.grid_row_end = init.grid_row_end.clone();
            }
        }
        // Прочие / неизвестные — silent no-op.
        _ => {}
    }
}
