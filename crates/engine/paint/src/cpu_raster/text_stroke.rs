//! `-webkit-text-stroke` (WHATWG Compat §3.2): разворачивание
//! [`DisplayCommand::DrawTextStroke`] в [`DisplayCommand::DrawSvgStroke`].
//!
//! Раскладка повторяет цикл [`super::rasterize_text`] (те же кандидаты шрифта,
//! тот же шейпер, табы, `letter-spacing`/`word-spacing`, синтетический
//! наклон), но вместо покрытия глифа снимает его контур
//! ([`lumen_font::outline_polylines`]) и отдаёт штриху: центрированная
//! обводка, miter-join с лимитом 4 (как `stroke-linejoin: miter` по умолчанию
//! у Blink для текста).

use lumen_core::FontProvider;

use super::{build_cascade, itemize_by_cascade, shear_glyph};
use crate::cpu_font_resolve::resolve_face_candidates;
use crate::display_list::spacing_px;
use crate::DisplayCommand;
use crate::svg_path::{StrokeLinecap, StrokeLinejoin, StrokeParams};

/// Лимит среза miter-соединения для обводки текста (как у SVG по умолчанию).
const MITER_LIMIT: f32 = 4.0;

/// Контуры всех глифов команды в координатах страницы; `None`, если шрифт не
/// разобрался или ни у одного глифа нет контура (пробелы, пустая строка).
fn text_contours(cmd: &DisplayCommand, provider: Option<&dyn FontProvider>) -> Option<Vec<Vec<[f32; 2]>>> {
    let DisplayCommand::DrawTextStroke {
        rect, text, font_size, font_family, font_weight, font_style, font_features, tab_size,
        letter_spacing, word_spacing, width, ..
    } = cmd
    else {
        return None;
    };
    if text.is_empty() || *font_size <= 0.0 {
        return None;
    }
    let letter_spacing = spacing_px(*letter_spacing);
    let word_spacing = spacing_px(*word_spacing);
    // `rect` раздут на `width / 2` — позиция глифов считается от исходной.
    let inset = width / 2.0;
    let (origin_x, origin_y) = (rect.x + inset, rect.y + inset);

    let candidates = resolve_face_candidates(provider, font_family, font_weight.0, *font_style);
    let cascade = build_cascade(&candidates)?;
    let primary = &cascade[0];
    let ascent_units = primary
        .face
        .ascent_override
        .map_or(primary.face.ascent, |pct| pct * f32::from(primary.face.units_per_em));
    let descent_units = primary
        .face
        .descent_override
        .map_or(primary.face.descent, |pct| -(pct * f32::from(primary.face.units_per_em)));
    let denom = ascent_units - descent_units;
    let ascent_ratio = if denom != 0.0 { ascent_units / denom } else { 0.8 };
    let baseline_y = origin_y + font_size * primary.face.size_adjust.unwrap_or(1.0) * ascent_ratio;
    let synth_italic = !matches!(font_style, lumen_layout::FontStyle::Normal);

    let segments: Vec<&str> = if *tab_size > 0.0 { text.split('\t').collect() } else { vec![text.as_str()] };
    let mut cursor_x = origin_x;
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    for (i, segment) in segments.into_iter().enumerate() {
        if i > 0 {
            cursor_x += tab_size;
        }
        if segment.is_empty() {
            continue;
        }
        for (candidate_idx, sub_text) in itemize_by_cascade(segment, &cascade) {
            if sub_text.is_empty() {
                continue;
            }
            let candidate = &cascade[candidate_idx];
            let adjusted = font_size * candidate.face.size_adjust.unwrap_or(1.0);
            let advance_scale = adjusted / f32::from(candidate.face.units_per_em);
            let shaped = lumen_font::active_text_shaper().shape(
                candidate.bytes,
                sub_text,
                lumen_core::ext::ShapeDirection::LeftToRight,
                None,
                font_features,
                &[],
            );
            for sg in &shaped {
                let pen_x = cursor_x + sg.x_offset as f32 * advance_scale;
                let glyph_baseline = baseline_y - sg.y_offset as f32 * advance_scale;
                if let Ok(Some(mut glyph)) = candidate.face.font.glyph_resolved(sg.glyph_id) {
                    if synth_italic {
                        shear_glyph(&mut glyph);
                    }
                    contours.extend(lumen_font::outline_polylines(
                        &glyph,
                        adjusted,
                        candidate.face.units_per_em,
                        (pen_x, glyph_baseline),
                    ));
                }
                cursor_x += sg.x_advance as f32 * advance_scale + letter_spacing;
                if word_spacing != 0.0
                    && sub_text.get(sg.cluster as usize..).is_some_and(|t| t.starts_with(' '))
                {
                    cursor_x += word_spacing;
                }
            }
        }
    }
    (!contours.is_empty()).then_some(contours)
}

/// `DrawTextStroke` → `DrawSvgStroke`. `None` — нечего штриховать (команда
/// другого вида, шрифт не разобрался, у текста нет контуров).
pub(crate) fn expand(cmd: &DisplayCommand, provider: Option<&dyn FontProvider>) -> Option<DisplayCommand> {
    let DisplayCommand::DrawTextStroke { width, color, .. } = cmd else {
        return None;
    };
    let contours = text_contours(cmd, provider)?;
    Some(DisplayCommand::DrawSvgStroke {
        contours,
        color: *color,
        params: StrokeParams {
            half_width: width / 2.0,
            linecap: StrokeLinecap::Butt,
            linejoin: StrokeLinejoin::Miter,
            miterlimit: MITER_LIMIT,
            dasharray: Vec::new(),
            dashoffset: 0.0,
        },
    })
}
