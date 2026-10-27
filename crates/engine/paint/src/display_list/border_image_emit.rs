//! CSS Backgrounds L3 §6 — эмит `border-image`: вместо `DrawBorder` бокса рисуется картинка,
//! нарезанная на девять клеток. URL-источник уходит растеризатору одной командой
//! `DrawBorderImage` (размер картинки знает только он); градиент раскладывается здесь же: его
//! «размер картинки» — область border-image, а плитки считает тот же `border_image_quads`.

use super::*;
use crate::border_image::{
    BorderImageQuad, BorderImageSpec, RepeatMode, SliceValue, WidthValue, border_image_quads,
};
use lumen_layout::{BorderImageOutset, BorderImageRepeat, BorderImageSlice, BorderImageWidth};

/// Рисует `border-image` бокса `b`; `false` — рисовать нечем (нет источника, источник не
/// поддержан или область вырожденная), и рамку тогда рисует `border-style`.
pub(crate) fn emit_border_image(b: &LayoutBox, dpr: f32, out: &mut Vec<DisplayCommand>) -> bool {
    let s = &b.style;
    let bi = &s.border_image;
    if !bi.is_active() {
        return false;
    }
    let bw = [s.border_top_width, s.border_right_width, s.border_bottom_width, s.border_left_width];
    let mut off = [0.0_f32; 4];
    for (i, o) in bi.outset.iter().enumerate() {
        off[i] = match *o {
            BorderImageOutset::Number(n) => n * bw[i],
            BorderImageOutset::Length(px) => px,
        }
        .max(0.0);
    }
    let area = Rect::new(
        b.rect.x - off[3],
        b.rect.y - off[0],
        b.rect.width + off[1] + off[3],
        b.rect.height + off[0] + off[2],
    );
    if area.width <= 0.0 || area.height <= 0.0 {
        return false;
    }
    let mut width = [WidthValue::Auto; 4];
    for (i, w) in bi.width.iter().enumerate() {
        // top/bottom — высота области, left/right — ширина.
        let dim = if i % 2 == 0 { area.height } else { area.width };
        width[i] = match *w {
            BorderImageWidth::Number(n) => WidthValue::Px(n * bw[i]),
            BorderImageWidth::Length(px) => WidthValue::Px(px),
            BorderImageWidth::Percent(p) => WidthValue::Px(p / 100.0 * dim),
            BorderImageWidth::Auto => WidthValue::Auto,
        };
    }
    let spec = BorderImageSpec {
        slice: bi.slice.map(|v| match v {
            BorderImageSlice::Number(n) => SliceValue::Number(n),
            BorderImageSlice::Percent(p) => SliceValue::Percent(p),
        }),
        fill: bi.fill,
        width,
        repeat: bi.repeat.map(|r| match r {
            BorderImageRepeat::Stretch => RepeatMode::Stretch,
            BorderImageRepeat::Repeat => RepeatMode::Repeat,
            BorderImageRepeat::Round => RepeatMode::Round,
            BorderImageRepeat::Space => RepeatMode::Space,
        }),
    };
    match &bi.source {
        BackgroundImage::Url(src) if !src.is_empty() => {
            let resolved = if is_image_set(src) { select_image_set_url(src, dpr) } else { src.as_str() };
            if resolved.is_empty() {
                return false;
            }
            out.push(DisplayCommand::DrawBorderImage {
                area,
                src: resolved.to_string(),
                spec,
            });
            true
        }
        BackgroundImage::Gradient(g) => {
            let quads = border_image_quads(&spec, area, area.width, area.height);
            for BorderImageQuad { src, dst } in quads {
                let sx = dst.width / src.width;
                let sy = dst.height / src.height;
                // Вся «картинка», сдвинутая и растянутая так, что окно `src` ложится на `dst`.
                let full = Rect::new(
                    dst.x - src.x * sx,
                    dst.y - src.y * sy,
                    area.width * sx,
                    area.height * sy,
                );
                out.push(DisplayCommand::PushClipRect { rect: dst });
                push_gradient(out, g, full);
                out.push(DisplayCommand::PopClip);
            }
            true
        }
        _ => false,
    }
}

/// Один градиент на прямоугольнике `rect` (тот же выбор команды, что у фонового слоя).
fn push_gradient(out: &mut Vec<DisplayCommand>, g: &ParsedGradient, rect: Rect) {
    match g {
        ParsedGradient::Linear { angle_deg, corner, stops, repeating } => {
            let angle = corner.map_or(*angle_deg, |c| c.angle_deg(rect.width, rect.height));
            out.push(DisplayCommand::DrawLinearGradient {
                rect,
                angle_deg: angle,
                stops: stops.clone(),
                repeating: *repeating,
            });
        }
        ParsedGradient::Radial { center_x_pct, center_y_pct, shape, size, stops, repeating } => {
            let (radius_x, radius_y) = lumen_layout::radial_gradient_radii(
                *shape, *size, *center_x_pct, *center_y_pct, rect.width, rect.height,
            );
            out.push(DisplayCommand::DrawRadialGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                radius_x,
                radius_y,
                stops: stops.clone(),
                repeating: *repeating,
            });
        }
        ParsedGradient::Unknown(_) => {}
        ParsedGradient::Conic { center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
            out.push(DisplayCommand::DrawConicGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                from_angle_deg: *from_angle_deg,
                stops: stops.clone(),
                repeating: *repeating,
            });
        }
    }
}
