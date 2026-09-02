//! Отрисовка — ветки `match prop` функции `apply_declaration`.
//!
//! цвет и фон, границы и их радиусы, `outline`, тени, `opacity`, фильтры и
//! режимы смешивания, маски и `clip-path`, SVG-краска (`fill`/`stroke`),
//! курсор, полосы прокрутки, `appearance`.
//!
//! Перенесено батчем SPLIT-ST8 из `crates/engine/layout/src/style.rs`: тела
//! веток скопированы побайтово, изменены только пути импортов и форма выхода
//! (`return` → `return true`, см. шапку `style/apply.rs`). Метка ветки в
//! группу не входит по алфавиту, а по смыслу — порядок веток внутри `match`
//! семантики не несёт, потому что все метки уникальны.

use crate::style::{
    Appearance,
    BackgroundAttachment,
    BackgroundClip,
    BackgroundImage,
    BackgroundLayer,
    BackgroundOrigin,
    BackgroundRepeat,
    BackgroundSize,
    ColorScheme,
    ComputedStyle,
    CssColor,
    FillRule,
    ForcedColorAdjust,
    ImageRendering,
    Isolation,
    MaskClip,
    MaskComposite,
    MaskLayer,
    MaskMode,
    MixBlendMode,
    ObjectPosition,
    OutlineColor,
    OutlineStyle,
    PointerEvents,
    PositionComponent,
    PrintColorAdjust,
    ScrollbarGutter,
    ScrollbarWidth,
    StrokeLinecap,
    StrokeLinejoin,
    SvgPaint,
    SvgPaintOrder,
    Visibility,
    parse_box_shadow_one,
    parse_cursor_kw,
    parse_length_q,
    parse_position_component,
    split_top_level_commas,
};
use crate::style::parse::box_sides::{
    apply_border_shorthand,
    apply_border_side_shorthand,
    expand_border_4,
    parse_border_style_kw,
    parse_line_width,
    parse_outline_color_opt,
    parse_outline_style_opt,
    parse_radius_length,
    resolve_box_length,
    resolve_svg_length,
    split_border_radius_slash,
    split_radius_pair,
};
use crate::style::parse::color::{parse_color_legacy, parse_css_color_legacy};
use crate::style::parse::counters::is_css_ident;
use crate::style::parse::image::{
    apply_mask_longhand,
    is_gradient_function,
    is_image_set_value,
    parse_background_gradient,
    parse_background_size_value,
    parse_cross_fade,
    parse_paint_function,
    parse_single_bg_layer,
    parse_single_mask_layer,
    parse_url_value,
};
use crate::style::parse::shape::parse_clip_path;
use crate::style::parse::transform::parse_filter_list;
use lumen_core::ColorSpace;
use lumen_core::geom::Size;

/// Применить одну декларацию из группы «отрисовка».
///
/// Возвращает `true`, если свойство принадлежит этой группе и было обработано;
/// `false` — если метка не наша и декларацию нужно предложить следующему
/// помощнику в цепочке `apply_declaration`.
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn apply_decl_paint(
    style: &mut ComputedStyle,
    prop: &str,
    val: &str,
    em_basis: f32,
    viewport: Size,
    inherited: &ComputedStyle,
    is_quirks: bool,
    dark_mode: bool,
) -> bool {
    match prop {
        "color" => {
            match parse_css_color_legacy(val, is_quirks) {
                Some(CssColor::Rgba(c)) => {
                    style.color = c;
                    style.color_space = ColorSpace::Srgb;
                }
                Some(CssColor::CurrentColor) => {
                    style.color = inherited.color;
                    style.color_space = inherited.color_space;
                }
                Some(CssColor::Wide(f)) => {
                    style.color = f.to_srgb_color();
                    style.color_space = f.space;
                }
                // CSS Color 4 §6.2 — system color keywords resolve against
                // the element's used color scheme (pre-pass already set
                // style.color_scheme for this element).
                Some(CssColor::System(sc)) => {
                    let dark = style.color_scheme.used_dark(dark_mode);
                    style.color = sc.resolve_color(dark);
                    style.color_space = ColorSpace::Srgb;
                }
                None => {}
            }
        }
        "background-color" => {
            if let Some(c) = parse_css_color_legacy(val, is_quirks) {
                style.background_color = Some(c);
            }
        }
        "background" => {
            // CSS Backgrounds L3 §3 shorthand: comma-separated list of layers.
            // Each layer: [<image>] [<position>[/<size>]]? [<repeat>]
            //             [<attachment>] [<box> [<box>]?] + optional <color> in last.
            let trimmed = val.trim();
            let layer_strs = split_top_level_commas(trimmed);
            if layer_strs.is_empty() { return true; }

            // Сбрасываем текущие слои — shorthand всегда переписывает полностью.
            style.background_layers.clear();
            style.background_color = None;

            for (i, ls) in layer_strs.iter().enumerate() {
                let (layer, maybe_color) = parse_single_bg_layer(ls.trim(), em_basis, viewport, is_quirks);
                style.background_layers.push(layer);
                // Цвет допустим только в последнем слое.
                if i == layer_strs.len() - 1 && let Some(c) = maybe_color {
                    style.background_color = Some(c);
                }
            }
        }
        "accent-color" => {
            // CSS UI L4 §6.1: <color> | auto.
            // 'auto' = None — UA сама подберёт цвет (обычно системный акцент).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.accent_color = None;
            } else if let Some(c) = parse_color_legacy(trimmed, is_quirks) {
                style.accent_color = Some(c);
            }
        }
        "color-scheme" => {
            // CSS Color Adjustment L1 §3: normal | [ light | dark ]+ && only?
            let v = val.trim();
            let only = v.contains("only");
            let has_light = v.contains("light");
            let has_dark = v.contains("dark");
            style.color_scheme = match (v, only, has_light, has_dark) {
                ("normal", _, _, _) => ColorScheme::Normal,
                (_, true, true, false) => ColorScheme::OnlyLight,
                (_, true, false, true) => ColorScheme::OnlyDark,
                (_, false, true, false) => ColorScheme::Light,
                (_, false, false, true) => ColorScheme::Dark,
                (_, false, true, true) => {
                    // "light dark" vs "dark light" — first keyword wins
                    if v.find("light") < v.find("dark") {
                        ColorScheme::LightDark
                    } else {
                        ColorScheme::DarkLight
                    }
                }
                _ => style.color_scheme,
            };
        }
        "forced-color-adjust" => {
            style.forced_color_adjust = match val.trim() {
                "auto" => ForcedColorAdjust::Auto,
                "none" => ForcedColorAdjust::None,
                "preserve-parent-color" => ForcedColorAdjust::PreserveParentColor,
                _ => style.forced_color_adjust,
            };
        }
        "image-rendering" => {
            // CSS Images L3 §6.1: enum-keyword. Inherited.
            if let Some(v) = ImageRendering::parse(val) {
                style.image_rendering = v;
            }
        }
        "visibility" => {
            style.visibility = match val.trim() {
                "visible" => Visibility::Visible,
                "hidden" => Visibility::Hidden,
                "collapse" => Visibility::Collapse,
                _ => style.visibility,
            };
        }
        "cursor" => {
            // CSS UI L4 §8.1: список url(), затем обязательный keyword.
            // url(...) пока не поддерживаем — берём ПОСЛЕДНИЙ
            // comma-separated токен (это и есть keyword fallback).
            let last = val.rsplit(',').next().unwrap_or("").trim();
            if let Some(c) = parse_cursor_kw(last) {
                style.cursor = c;
            }
        }
        "box-shadow" => {
            // CSS Backgrounds L3 §4.6: comma-separated. `none` сбрасывает.
            if val.trim() == "none" {
                style.box_shadow = Vec::new();
            } else {
                let mut shadows = Vec::new();
                for piece in split_top_level_commas(val) {
                    if let Some(s) = parse_box_shadow_one(piece.trim(), em_basis, viewport, is_quirks) {
                        shadows.push(s);
                    }
                }
                if !shadows.is_empty() {
                    style.box_shadow = shadows;
                }
            }
        }
        "outline" => {
            // CSS Basic UI L4 §5.1 — `outline` shorthand сбрасывает все три
            // longhand-а в initial и парсит токены `[<'outline-color'> ||
            // <'outline-style'> || <'outline-width'>]` в любом порядке.
            // Каждый slot заполняется первым подходящим токеном.
            style.outline_width = 3.0; // medium
            style.outline_style = OutlineStyle::None;
            style.outline_color = OutlineColor::Auto;
            let mut width_set = false;
            let mut style_set = false;
            let mut color_set = false;
            for tok in val.split_whitespace() {
                if !style_set
                    && let Some(s) = parse_outline_style_opt(tok)
                {
                    style.outline_style = s;
                    style_set = true;
                } else if !width_set
                    && let Some(w) = parse_line_width(tok, em_basis, viewport, is_quirks)
                {
                    style.outline_width = w;
                    width_set = true;
                } else if !color_set
                    && let Some(c) = parse_outline_color_opt(tok, is_quirks)
                {
                    style.outline_color = c;
                    color_set = true;
                }
            }
        }
        "outline-width" => {
            if let Some(v) = parse_line_width(val, em_basis, viewport, is_quirks) {
                style.outline_width = v;
            }
        }
        "outline-style" => {
            if let Some(s) = parse_outline_style_opt(val) {
                style.outline_style = s;
            }
        }
        "outline-color" => {
            if let Some(c) = parse_outline_color_opt(val, is_quirks) {
                style.outline_color = c;
            }
        }
        "outline-offset" => {
            // <length>; отрицательные значения валидны (CSS UI L4 §3.4).
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.outline_offset = len;
            }
        }
        "clip-path" => {
            // CSS Masking L1 §3 — basic-shape | none. `none` чистит.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.clip_path = None;
            } else if let Some(cp) = parse_clip_path(trimmed) {
                style.clip_path = Some(cp);
            }
        }
        "filter" => {
            // CSS Filter Effects L1 §3 — `none | <filter-function-list>`.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.filter = Vec::new();
            } else {
                style.filter = parse_filter_list(trimmed);
            }
        }
        "backdrop-filter" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.backdrop_filter = Vec::new();
            } else {
                style.backdrop_filter = parse_filter_list(trimmed);
            }
        }
        "print-color-adjust" | "color-adjust" => {
            style.print_color_adjust = match val.trim() {
                "economy" => PrintColorAdjust::Economy,
                "exact" => PrintColorAdjust::Exact,
                _ => style.print_color_adjust,
            };
        }
        "background-image" => {
            // CSS Backgrounds L3 §3.1 — comma-separated list of images.
            let trimmed = val.trim();
            let pieces = split_top_level_commas(trimmed);
            // Сохраняем текущие per-layer данные для cycling после изменения размера.
            let old_layers = std::mem::take(&mut style.background_layers);
            style.background_layers = pieces.iter().enumerate().map(|(i, s)| {
                let s = s.trim();
                let image = if s.eq_ignore_ascii_case("none") {
                    BackgroundImage::None
                } else if is_image_set_value(s) {
                    // Store raw image-set(…) string; paint resolves per-DPR (CSS Images L4 §5).
                    BackgroundImage::Url(s.to_string())
                } else if let Some((a, b, t)) = parse_cross_fade(s) {
                    BackgroundImage::CrossFade { a: Box::new(a), b: Box::new(b), t }
                } else if let Some(paint_name) = parse_paint_function(s) {
                    // CSS Paint API (Houdini) — `paint(name)` invokes registered worklet.
                    BackgroundImage::Paint(paint_name)
                } else if let Some(url) = parse_url_value(s) {
                    BackgroundImage::Url(url)
                } else if is_gradient_function(s) {
                    BackgroundImage::Gradient(parse_background_gradient(s))
                } else {
                    BackgroundImage::None
                };
                // Прочие свойства берём из старого слоя (cycling) или defaults.
                let old = old_layers.get(i % old_layers.len().max(1)).cloned().unwrap_or_default();
                BackgroundLayer { image, ..old }
            }).collect();
        }
        "background-repeat" => {
            // CSS Backgrounds L3 §3.4 — comma-separated list (cycling).
            let pieces = split_top_level_commas(val.trim());
            let repeats: Vec<BackgroundRepeat> = pieces.iter()
                .filter_map(|s| BackgroundRepeat::parse(s.trim()))
                .collect();
            if repeats.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = repeats.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.repeat = repeats[i % n];
            }
        }
        "background-size" => {
            // CSS Backgrounds L3 §3.5 — comma-separated list (cycling).
            let pieces = split_top_level_commas(val.trim());
            let sizes: Vec<BackgroundSize> = pieces.iter()
                .map(|s| parse_background_size_value(s, em_basis, viewport, is_quirks))
                .collect();
            if sizes.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = sizes.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.size = sizes[i % n];
            }
        }
        "background-attachment" => {
            // CSS Backgrounds L3 §3.6 — comma-separated list (cycling).
            let pieces = split_top_level_commas(val.trim());
            let atts: Vec<BackgroundAttachment> = pieces.iter()
                .filter_map(|s| BackgroundAttachment::parse(s.trim()))
                .collect();
            if atts.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = atts.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.attachment = atts[i % n];
            }
        }
        "background-origin" => {
            // CSS Backgrounds L3 §3.7: border-box | padding-box | content-box.
            let pieces = split_top_level_commas(val.trim());
            let origins: Vec<BackgroundOrigin> = pieces.iter()
                .filter_map(|s| BackgroundOrigin::parse(s.trim()))
                .collect();
            if origins.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = origins.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.origin = origins[i % n];
            }
        }
        "background-clip" => {
            // CSS Backgrounds L3 §3.8 + L4 (`text`): comma-separated list.
            let pieces = split_top_level_commas(val.trim());
            let clips: Vec<BackgroundClip> = pieces.iter()
                .filter_map(|s| BackgroundClip::parse(s.trim()))
                .collect();
            if clips.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = clips.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.clip = clips[i % n];
            }
        }
        "background-blend-mode" => {
            // CSS Compositing L1 §8.3 — comma-separated list (cycling over layers).
            let pieces = split_top_level_commas(val.trim());
            let modes: Vec<MixBlendMode> = pieces.iter()
                .filter_map(|s| MixBlendMode::parse(s.trim()))
                .collect();
            if modes.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = modes.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.blend_mode = modes[i % n];
            }
        }
        "background-position" => {
            // CSS Backgrounds L3 §3.5 — comma-separated list (cycling).
            // Парсер `<position>` переиспользуется с `object-position`,
            // но default для background-position — `0% 0%`.
            let pieces = split_top_level_commas(val.trim());
            let positions: Vec<ObjectPosition> = pieces.iter()
                .filter_map(|s| ObjectPosition::parse(s.trim(), em_basis, viewport))
                .collect();
            if positions.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = positions.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.position = positions[i % n];
            }
        }
        "background-position-x" => {
            // CSS Backgrounds L4 §3.5 — standalone horizontal longhand,
            // `[ center | left | right | <length-percentage> ]#`. Edge-relative
            // offset form (`right -10px`) and `x-start`/`x-end` logical
            // keywords are not yet supported — same deferral as the
            // tri-/quad-form of `<position>` noted on `ObjectPosition::parse`.
            let xs: Vec<PositionComponent> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| parse_position_component(s.trim(), em_basis, viewport, false))
                .collect();
            if xs.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = xs.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.position.x = xs[i % n];
            }
        }
        "background-position-y" => {
            // CSS Backgrounds L4 §3.5 — standalone vertical longhand, mirrors
            // `background-position-x` above.
            let ys: Vec<PositionComponent> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| parse_position_component(s.trim(), em_basis, viewport, true))
                .collect();
            if ys.is_empty() { return true; }
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            let n = ys.len();
            for (i, layer) in style.background_layers.iter_mut().enumerate() {
                layer.position.y = ys[i % n];
            }
        }
        "will-change" => {
            // CSS Will Change L1: `auto | <ident-list>`. Lenient parser —
            // comma-separated ident-имена.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.will_change = Vec::new();
            } else {
                style.will_change = trimmed
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && is_css_ident(s))
                    .collect();
            }
        }
        "isolation" => {
            if let Some(v) = Isolation::parse(val) {
                style.isolation = v;
            }
        }
        "mix-blend-mode" => {
            if let Some(v) = MixBlendMode::parse(val) {
                style.mix_blend_mode = v;
            }
        }
        "pointer-events" => {
            if let Some(v) = PointerEvents::parse(val) {
                style.pointer_events = v;
            }
        }
        "appearance" | "-webkit-appearance" | "-moz-appearance" => {
            style.appearance = match val.trim() {
                "auto" => Appearance::Auto,
                "none" => Appearance::None,
                // HTML/CSS «Customizable Select»: opt into the author-styleable
                // widget tree. `base` is the shorthand that also resets other
                // appearance-related UA styling; treat both as `BaseSelect`.
                "base-select" | "base" => Appearance::BaseSelect,
                _ => Appearance::Compat,
            };
        }
        "caret-color" => {
            // CSS UI L4 §6.3: auto | <color>.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.caret_color = None;
            } else if let Some(c) = parse_color_legacy(trimmed, is_quirks) {
                style.caret_color = Some(c);
            }
        }
        "mask" => {
            // CSS Masking L1 §4.8 shorthand: comma-separated list of layers.
            // Each layer: <mask-reference> || <position> [/ <size>]? ||
            //             <repeat-style> || <geometry-box> ||
            //             [<geometry-box> | no-clip] || <compositing-operator> ||
            //             <masking-mode>.
            // Шортхенд всегда переписывает список целиком — свойства, не
            // указанные в слое, сбрасываются к initial (per-layer default).
            let layer_strs = split_top_level_commas(val.trim());
            if layer_strs.is_empty() {
                return true;
            }
            style.mask_layers = layer_strs
                .iter()
                .map(|ls| parse_single_mask_layer(ls.trim(), em_basis, viewport, is_quirks))
                .collect();
        }
        "mask-image" => {
            // CSS Masking L1 §4.1 — comma-separated list of images; задаёт
            // количество слоёв. Прочие per-layer свойства переносятся из
            // прежних слоёв циклически (как у `background-image`).
            let pieces = split_top_level_commas(val.trim());
            let old_layers = std::mem::take(&mut style.mask_layers);
            style.mask_layers = pieces
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let s = s.trim();
                    let image = if s.eq_ignore_ascii_case("none") {
                        BackgroundImage::None
                    } else if let Some(u) = parse_url_value(s) {
                        BackgroundImage::Url(u)
                    } else if is_gradient_function(s) {
                        BackgroundImage::Gradient(parse_background_gradient(s))
                    } else {
                        BackgroundImage::None
                    };
                    let old = old_layers
                        .get(i % old_layers.len().max(1))
                        .cloned()
                        .unwrap_or_default();
                    MaskLayer { image, ..old }
                })
                .collect();
        }
        "mask-repeat" => {
            // CSS Masking L1 §4.3 — comma-separated list (cycling).
            let values: Vec<BackgroundRepeat> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| BackgroundRepeat::parse(s.trim()))
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.repeat = v);
        }
        "mask-mode" => {
            // CSS Masking L1 §6.4 — comma-separated list (cycling).
            // `match-source` resolves to `Alpha` for the `<image>` sources we
            // support (gradients / raster URLs).
            let values: Vec<MaskMode> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| {
                    let s = s.trim();
                    if s.eq_ignore_ascii_case("luminance") {
                        Some(MaskMode::Luminance)
                    } else if s.eq_ignore_ascii_case("alpha")
                        || s.eq_ignore_ascii_case("match-source")
                    {
                        Some(MaskMode::Alpha)
                    } else {
                        None
                    }
                })
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.mode = v);
        }
        "mask-position" => {
            // CSS Masking L1 §4.4 — `<position>#`, same grammar as
            // background-position.
            let values: Vec<ObjectPosition> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| ObjectPosition::parse(s.trim(), em_basis, viewport))
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.position = v);
        }
        "mask-origin" => {
            // CSS Masking L1 §4.5 — `<geometry-box>#`.
            let values: Vec<BackgroundOrigin> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| BackgroundOrigin::parse(s.trim()))
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.origin = v);
        }
        "mask-clip" => {
            // CSS Masking L1 §4.6 — `[<coord-box> | no-clip]#` (superset of
            // background-clip: adds fill-box/stroke-box/view-box and no-clip).
            let values: Vec<MaskClip> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| MaskClip::parse(s.trim()))
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.clip = v);
        }
        "mask-composite" => {
            // CSS Masking L1 §4.7 — `<compositing-operator>#`.
            let values: Vec<MaskComposite> = split_top_level_commas(val.trim())
                .iter()
                .filter_map(|s| MaskComposite::parse(s.trim()))
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.composite = v);
        }
        "mask-size" => {
            // CSS Masking L1 §4.2 — `<bg-size>#`.
            let values: Vec<BackgroundSize> = split_top_level_commas(val.trim())
                .iter()
                .map(|s| parse_background_size_value(s, em_basis, viewport, is_quirks))
                .collect();
            apply_mask_longhand(style, &values, |layer, v| layer.size = v);
        }
        "scrollbar-width" => {
            if let Some(v) = ScrollbarWidth::parse(val) {
                style.scrollbar_width = v;
            }
        }
        "scrollbar-color" => {
            // `auto` или два цвета (thumb + track).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.scrollbar_color = None;
            } else {
                // Парсим два color-значения, разделённые whitespace.
                // Простая реализация: split по `)` чтобы разделить
                // `rgb(...)` пары. Иначе — split_whitespace.
                let mut pieces: Vec<String> = Vec::new();
                let mut current = String::new();
                let mut depth = 0i32;
                for c in trimmed.chars() {
                    current.push(c);
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            pieces.push(current.trim().to_string());
                            current.clear();
                        }
                    } else if c.is_whitespace() && depth == 0 && !current.trim().is_empty() {
                        let trimmed_piece = current.trim().to_string();
                        if !trimmed_piece.is_empty() {
                            pieces.push(trimmed_piece);
                        }
                        current.clear();
                    }
                }
                if !current.trim().is_empty() {
                    pieces.push(current.trim().to_string());
                }
                pieces.retain(|p| !p.is_empty());
                if pieces.len() == 2
                    && let (Some(thumb), Some(track)) =
                        (parse_color_legacy(&pieces[0], is_quirks), parse_color_legacy(&pieces[1], is_quirks))
                {
                    style.scrollbar_color = Some((thumb, track));
                }
            }
        }
        "scrollbar-gutter" => {
            if let Some(v) = ScrollbarGutter::parse(val) {
                style.scrollbar_gutter = v;
            }
        }
        "opacity" => {
            // CSS Color L3 §3.2: <number 0..1> или <percentage>. Out-of-range
            // clamp-ается. Невалидные значения игнорируются.
            let v = val.trim();
            let parsed = if let Some(pct) = v.strip_suffix('%') {
                pct.trim().parse::<f32>().ok().map(|n| n / 100.0)
            } else {
                v.parse::<f32>().ok()
            };
            if let Some(o) = parsed {
                style.opacity = o.clamp(0.0, 1.0);
            }
        }
        // SVG Presentation Attributes §11.2/11.3/11.4 — fill/stroke paint + opacity + width.
        "fill" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("none") {
                style.svg_fill = SvgPaint::None;
            } else if v.eq_ignore_ascii_case("currentcolor") {
                style.svg_fill = SvgPaint::CurrentColor;
            } else if let Some(id) = svg_paint_url_id(v) {
                // LIB-5: `url(#id)` — resolved against the DOM later, in
                // `box_tree/svg.rs` (cascade has no `Document` access).
                style.svg_fill = SvgPaint::Url(id);
            } else if let Some(c) = parse_color_legacy(v, is_quirks) {
                style.svg_fill = SvgPaint::Color(c);
            }
        }
        "fill-opacity" => {
            let v = val.trim();
            let parsed = if let Some(pct) = v.strip_suffix('%') {
                pct.trim().parse::<f32>().ok().map(|n| n / 100.0)
            } else {
                v.parse::<f32>().ok()
            };
            if let Some(o) = parsed {
                style.svg_fill_opacity = o.clamp(0.0, 1.0);
            }
        }
        "stroke" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("none") {
                style.svg_stroke = SvgPaint::None;
            } else if v.eq_ignore_ascii_case("currentcolor") {
                style.svg_stroke = SvgPaint::CurrentColor;
            } else if let Some(id) = svg_paint_url_id(v) {
                style.svg_stroke = SvgPaint::Url(id);
            } else if let Some(c) = parse_color_legacy(v, is_quirks) {
                style.svg_stroke = SvgPaint::Color(c);
            }
        }
        "stroke-opacity" => {
            let v = val.trim();
            let parsed = if let Some(pct) = v.strip_suffix('%') {
                pct.trim().parse::<f32>().ok().map(|n| n / 100.0)
            } else {
                v.parse::<f32>().ok()
            };
            if let Some(o) = parsed {
                style.svg_stroke_opacity = o.clamp(0.0, 1.0);
            }
        }
        "stroke-width" => {
            if let Some(w) = resolve_svg_length(val, em_basis, viewport, is_quirks) {
                style.svg_stroke_width = w.max(0.0);
            }
        }
        "fill-rule" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("evenodd") {
                style.svg_fill_rule = FillRule::EvenOdd;
            } else if v.eq_ignore_ascii_case("nonzero") {
                style.svg_fill_rule = FillRule::NonZero;
            }
        }
        "clip-rule" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("evenodd") {
                style.svg_clip_rule = FillRule::EvenOdd;
            } else if v.eq_ignore_ascii_case("nonzero") {
                style.svg_clip_rule = FillRule::NonZero;
            }
        }
        "stroke-linecap" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("round") {
                style.svg_stroke_linecap = StrokeLinecap::Round;
            } else if v.eq_ignore_ascii_case("square") {
                style.svg_stroke_linecap = StrokeLinecap::Square;
            } else if v.eq_ignore_ascii_case("butt") {
                style.svg_stroke_linecap = StrokeLinecap::Butt;
            }
        }
        "stroke-linejoin" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("round") {
                style.svg_stroke_linejoin = StrokeLinejoin::Round;
            } else if v.eq_ignore_ascii_case("bevel") {
                style.svg_stroke_linejoin = StrokeLinejoin::Bevel;
            } else if v.eq_ignore_ascii_case("miter") {
                style.svg_stroke_linejoin = StrokeLinejoin::Miter;
            }
        }
        "stroke-miterlimit" => {
            if let Ok(v) = val.trim().parse::<f32>()
                && v >= 1.0
            {
                style.svg_stroke_miterlimit = v;
            }
        }
        "stroke-dasharray" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("none") {
                style.svg_stroke_dasharray = Vec::new();
            } else {
                let dashes: Vec<f32> = v
                    .split(|c: char| c == ',' || c.is_ascii_whitespace())
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| resolve_svg_length(s, em_basis, viewport, is_quirks))
                    .filter(|&v| v >= 0.0)
                    .collect();
                if !dashes.is_empty() {
                    style.svg_stroke_dasharray = dashes;
                }
            }
        }
        "stroke-dashoffset" => {
            if let Some(v) = resolve_svg_length(val, em_basis, viewport, is_quirks) {
                style.svg_stroke_dashoffset = v;
            }
        }
        "paint-order" => {
            // CSS Fill & Stroke L3 §6 / SVG 2 §13.7.
            if let Some(o) = SvgPaintOrder::parse(val) {
                style.paint_order = o;
            }
        }
        // ── Borders ───────────────────────────────────────────────────────────
        "border" => apply_border_shorthand(style, val, em_basis, viewport, is_quirks),
        "border-top" => apply_border_side_shorthand(
            &mut style.border_top_width, &mut style.border_top_style,
            &mut style.border_top_color, val, em_basis, viewport, is_quirks),
        "border-right" => apply_border_side_shorthand(
            &mut style.border_right_width, &mut style.border_right_style,
            &mut style.border_right_color, val, em_basis, viewport, is_quirks),
        "border-bottom" => apply_border_side_shorthand(
            &mut style.border_bottom_width, &mut style.border_bottom_style,
            &mut style.border_bottom_color, val, em_basis, viewport, is_quirks),
        "border-left" => apply_border_side_shorthand(
            &mut style.border_left_width, &mut style.border_left_style,
            &mut style.border_left_color, val, em_basis, viewport, is_quirks),
        // CSS Logical Properties L1 §6.3 — border-inline-* / border-block-*.
        "border-inline-start" => apply_border_side_shorthand(
            &mut style.border_left_width, &mut style.border_left_style,
            &mut style.border_left_color, val, em_basis, viewport, is_quirks),
        "border-inline-end" => apply_border_side_shorthand(
            &mut style.border_right_width, &mut style.border_right_style,
            &mut style.border_right_color, val, em_basis, viewport, is_quirks),
        "border-block-start" => apply_border_side_shorthand(
            &mut style.border_top_width, &mut style.border_top_style,
            &mut style.border_top_color, val, em_basis, viewport, is_quirks),
        "border-block-end" => apply_border_side_shorthand(
            &mut style.border_bottom_width, &mut style.border_bottom_style,
            &mut style.border_bottom_color, val, em_basis, viewport, is_quirks),
        "border-inline" => {
            apply_border_side_shorthand(
                &mut style.border_left_width, &mut style.border_left_style,
                &mut style.border_left_color, val, em_basis, viewport, is_quirks);
            apply_border_side_shorthand(
                &mut style.border_right_width, &mut style.border_right_style,
                &mut style.border_right_color, val, em_basis, viewport, is_quirks);
        }
        "border-block" => {
            apply_border_side_shorthand(
                &mut style.border_top_width, &mut style.border_top_style,
                &mut style.border_top_color, val, em_basis, viewport, is_quirks);
            apply_border_side_shorthand(
                &mut style.border_bottom_width, &mut style.border_bottom_style,
                &mut style.border_bottom_color, val, em_basis, viewport, is_quirks);
        }
        // CSS Logical Properties L1 — border-inline-*-width / border-block-*-width.
        // Stored in logical fields; resolved to physical (left/right/top/bottom) in resolve_logical_properties().
        "border-inline-start-width" => { if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) { style.border_inline_start_width = v; } }
        "border-inline-end-width"   => { if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) { style.border_inline_end_width = v; } }
        "border-block-start-width"  => { if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) { style.border_block_start_width = v; } }
        "border-block-end-width"    => { if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) { style.border_block_end_width = v; } }
        "border-inline-start-style" => style.border_left_style = parse_border_style_kw(val),
        "border-inline-end-style"   => style.border_right_style = parse_border_style_kw(val),
        "border-block-start-style"  => style.border_top_style = parse_border_style_kw(val),
        "border-block-end-style"    => style.border_bottom_style = parse_border_style_kw(val),
        "border-inline-start-color" => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_left_color = c; } }
        "border-inline-end-color"   => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_right_color = c; } }
        "border-block-start-color"  => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_top_color = c; } }
        "border-block-end-color"    => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_bottom_color = c; } }
        "border-width" => {
            let sides = expand_border_4(val);
            if let Some(v) = resolve_box_length(sides[0], em_basis, viewport, is_quirks) { style.border_top_width = v; }
            if let Some(v) = resolve_box_length(sides[1], em_basis, viewport, is_quirks) { style.border_right_width = v; }
            if let Some(v) = resolve_box_length(sides[2], em_basis, viewport, is_quirks) { style.border_bottom_width = v; }
            if let Some(v) = resolve_box_length(sides[3], em_basis, viewport, is_quirks) { style.border_left_width = v; }
        }
        "border-style" => {
            let sides = expand_border_4(val);
            style.border_top_style = parse_border_style_kw(sides[0]);
            style.border_right_style = parse_border_style_kw(sides[1]);
            style.border_bottom_style = parse_border_style_kw(sides[2]);
            style.border_left_style = parse_border_style_kw(sides[3]);
        }
        "border-color" => {
            let sides = expand_border_4(val);
            if let Some(c) = parse_css_color_legacy(sides[0], is_quirks) { style.border_top_color = c; }
            if let Some(c) = parse_css_color_legacy(sides[1], is_quirks) { style.border_right_color = c; }
            if let Some(c) = parse_css_color_legacy(sides[2], is_quirks) { style.border_bottom_color = c; }
            if let Some(c) = parse_css_color_legacy(sides[3], is_quirks) { style.border_left_color = c; }
        }
        "border-radius" => {
            // CSS Backgrounds L3 §5.5 shorthand. Форма: H1..H4 [/ V1..V4].
            // Каждая часть раскрывается по правилу expand_border_4 (TL TR BR BL).
            // Если `/` нет — V-радиусы равны H-радиусам (круговые углы).
            let (h_part, v_part) = split_border_radius_slash(val);
            let h = expand_border_4(h_part);
            let v = if let Some(vp) = v_part { expand_border_4(vp) } else { h };
            if let Some(x) = parse_radius_length(h[0], em_basis, viewport, is_quirks) {
                style.border_top_left_radius = x;
            }
            if let Some(x) = parse_radius_length(h[1], em_basis, viewport, is_quirks) {
                style.border_top_right_radius = x;
            }
            if let Some(x) = parse_radius_length(h[2], em_basis, viewport, is_quirks) {
                style.border_bottom_right_radius = x;
            }
            if let Some(x) = parse_radius_length(h[3], em_basis, viewport, is_quirks) {
                style.border_bottom_left_radius = x;
            }
            if let Some(y) = parse_radius_length(v[0], em_basis, viewport, is_quirks) {
                style.border_top_left_radius_y = y;
            }
            if let Some(y) = parse_radius_length(v[1], em_basis, viewport, is_quirks) {
                style.border_top_right_radius_y = y;
            }
            if let Some(y) = parse_radius_length(v[2], em_basis, viewport, is_quirks) {
                style.border_bottom_right_radius_y = y;
            }
            if let Some(y) = parse_radius_length(v[3], em_basis, viewport, is_quirks) {
                style.border_bottom_left_radius_y = y;
            }
        }
        "border-top-left-radius" => {
            // CSS Backgrounds L3 §5.5: одно или два значения `rx [ry]`.
            let (rx, ry) = split_radius_pair(val);
            if let Some(x) = parse_radius_length(rx, em_basis, viewport, is_quirks) {
                style.border_top_left_radius = x;
            }
            let ry_val = ry.unwrap_or(rx);
            if let Some(y) = parse_radius_length(ry_val, em_basis, viewport, is_quirks) {
                style.border_top_left_radius_y = y;
            }
        }
        "border-top-right-radius" => {
            let (rx, ry) = split_radius_pair(val);
            if let Some(x) = parse_radius_length(rx, em_basis, viewport, is_quirks) {
                style.border_top_right_radius = x;
            }
            let ry_val = ry.unwrap_or(rx);
            if let Some(y) = parse_radius_length(ry_val, em_basis, viewport, is_quirks) {
                style.border_top_right_radius_y = y;
            }
        }
        "border-bottom-right-radius" => {
            let (rx, ry) = split_radius_pair(val);
            if let Some(x) = parse_radius_length(rx, em_basis, viewport, is_quirks) {
                style.border_bottom_right_radius = x;
            }
            let ry_val = ry.unwrap_or(rx);
            if let Some(y) = parse_radius_length(ry_val, em_basis, viewport, is_quirks) {
                style.border_bottom_right_radius_y = y;
            }
        }
        "border-bottom-left-radius" => {
            let (rx, ry) = split_radius_pair(val);
            if let Some(x) = parse_radius_length(rx, em_basis, viewport, is_quirks) {
                style.border_bottom_left_radius = x;
            }
            let ry_val = ry.unwrap_or(rx);
            if let Some(y) = parse_radius_length(ry_val, em_basis, viewport, is_quirks) {
                style.border_bottom_left_radius_y = y;
            }
        }
        "border-top-width" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_top_width = v;
            }
        }
        "border-right-width" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_right_width = v;
            }
        }
        "border-bottom-width" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_bottom_width = v;
            }
        }
        "border-left-width" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.border_left_width = v;
            }
        }
        "border-top-style" => style.border_top_style = parse_border_style_kw(val),
        "border-right-style" => style.border_right_style = parse_border_style_kw(val),
        "border-bottom-style" => style.border_bottom_style = parse_border_style_kw(val),
        "border-left-style" => style.border_left_style = parse_border_style_kw(val),
        "border-top-color" => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_top_color = c; } }
        "border-right-color" => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_right_color = c; } }
        "border-bottom-color" => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_bottom_color = c; } }
        "border-left-color" => { if let Some(c) = parse_css_color_legacy(val, is_quirks) { style.border_left_color = c; } }
        _ => return false,
    }
    true
}

/// LIB-5 — extracts the fragment id from an SVG `<paint>` `url(#id)` /
/// `url("#id")` / `url('#id')` reference. `None` for anything else (a color
/// keyword, `none`, `currentColor`, or a non-fragment `url(...)` — SVG paint
/// only supports same-document fragment references).
fn svg_paint_url_id(v: &str) -> Option<String> {
    if v.len() < 5 || !v[..4].eq_ignore_ascii_case("url(") {
        return None;
    }
    let inner = v[4..].strip_suffix(')')?.trim();
    let inner = inner.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
        .or_else(|| inner.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(inner);
    inner.strip_prefix('#').map(str::to_owned).filter(|id| !id.is_empty())
}
