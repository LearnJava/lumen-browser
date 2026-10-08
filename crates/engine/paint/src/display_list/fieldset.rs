//! HTML Rendering §15.3.13 — рамка `<fieldset>` с rendered legend: середина границы блока-начала
//! лежит на середине legend, а позади legend рамка не рисуется.

use super::*;
use lumen_layout::style::WritingMode;

/// Если у `b` есть rendered legend, который раскладка поставила на границу, возвращает рамку
/// (border box, у которого граница блока-начала опущена на `border_inset`) и три прямоугольника,
/// по которым её надо клиппить: до legend, после него и «под» ним (в глубину рамки). Позади
/// legend остаётся вырез.
///
/// Граница блока-начала — верхняя в `horizontal-tb`, правая у `vertical-rl`/`sideways-rl`,
/// левая у `vertical-lr`/`sideways-lr`.
/// Border box у `<fieldset>` с rendered legend: граница блока-начала опущена на `border_inset`.
/// `None` — legend не стоит на границе, рамка целая.
fn shifted_border_rect(b: &LayoutBox) -> Option<Rect> {
    let fl = b.fieldset_legend.filter(|l| l.placed)?;
    let (r, d) = (b.rect, fl.border_inset);
    Some(match b.style.writing_mode {
        WritingMode::HorizontalTb => Rect::new(r.x, r.y + d, r.width, (r.height - d).max(0.0)),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => Rect::new(r.x, r.y, (r.width - d).max(0.0), r.height),
        WritingMode::VerticalLr | WritingMode::SidewaysLr => Rect::new(r.x + d, r.y, (r.width - d).max(0.0), r.height),
    })
}

/// Бокс для фона, теней и скруглений fieldset'а: тот же, но с опущенной границей блока-начала
/// и без детей (они здесь не нужны, а клонировать поддерево на каждый кадр незачем).
pub(crate) fn fieldset_decoration_box(b: &LayoutBox) -> Option<LayoutBox> {
    let rect = shifted_border_rect(b)?;
    Some(LayoutBox {
        node: b.node,
        rect,
        style: b.style.clone(),
        used_line_height: b.used_line_height,
        grid_baselines: None,
        fieldset_legend: None,
        subgrid_tracks: None,
        kind: b.kind.clone(),
        children: Vec::new(),
        col_span: b.col_span,
        row_span: b.row_span,
        svg_group_transform: None,
        scroll_x: b.scroll_x,
        scroll_y: b.scroll_y,
        dirty: Default::default(),
        origin: b.origin,
    })
}

fn legend_border_clips(b: &LayoutBox) -> Option<(Rect, [Rect; 3])> {
    let fl = b.fieldset_legend.filter(|l| l.placed)?;
    let legend = b.children.get(fl.idx)?.rect;
    let (rect, s) = (shifted_border_rect(b)?, &b.style);
    match s.writing_mode {
        WritingMode::HorizontalTb => {
            let (x0, x1) = (legend.x.max(rect.x), (legend.x + legend.width).min(rect.right()));
            if x1 <= x0 {
                return None;
            }
            // Вырез идёт вниз до нижней кромки legend или толщины границы — что глубже.
            let cut = (legend.y + legend.height).max(rect.y + s.border_top_width).min(rect.bottom());
            Some((
                rect,
                [
                    Rect::new(rect.x, rect.y, x0 - rect.x, rect.height),
                    Rect::new(x1, rect.y, rect.right() - x1, rect.height),
                    Rect::new(x0, cut, x1 - x0, rect.bottom() - cut),
                ],
            ))
        }
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            let (y0, y1) = (legend.y.max(rect.y), (legend.y + legend.height).min(rect.bottom()));
            if y1 <= y0 {
                return None;
            }
            let cut = legend.x.min(rect.right() - s.border_right_width).max(rect.x);
            Some((
                rect,
                [
                    Rect::new(rect.x, rect.y, rect.width, y0 - rect.y),
                    Rect::new(rect.x, y1, rect.width, rect.bottom() - y1),
                    Rect::new(rect.x, y0, cut - rect.x, y1 - y0),
                ],
            ))
        }
        WritingMode::VerticalLr | WritingMode::SidewaysLr => {
            let (y0, y1) = (legend.y.max(rect.y), (legend.y + legend.height).min(rect.bottom()));
            if y1 <= y0 {
                return None;
            }
            let cut = (legend.x + legend.width).max(rect.x + s.border_left_width).min(rect.right());
            Some((
                rect,
                [
                    Rect::new(rect.x, rect.y, rect.width, y0 - rect.y),
                    Rect::new(rect.x, y1, rect.width, rect.bottom() - y1),
                    Rect::new(cut, y0, rect.right() - cut, y1 - y0),
                ],
            ))
        }
    }
}

/// Эмитит `DrawBorder` бокса; у fieldset с rendered legend — рамка, обрезанная вокруг legend.
pub(crate) fn emit_box_border(b: &LayoutBox, radii: CornerRadii, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let cur = s.color;
    let border = |rect: Rect| DisplayCommand::DrawBorder {
        rect,
        widths: [
            s.border_top_width,
            s.border_right_width,
            s.border_bottom_width,
            s.border_left_width,
        ],
        colors: [
            s.border_top_color.resolve(cur),
            s.border_right_color.resolve(cur),
            s.border_bottom_color.resolve(cur),
            s.border_left_color.resolve(cur),
        ],
        styles: [
            s.border_top_style,
            s.border_right_style,
            s.border_bottom_style,
            s.border_left_style,
        ],
        radii,
    };
    let Some((rect, clips)) = legend_border_clips(b) else {
        out.push(border(b.rect));
        return;
    };
    for clip in clips {
        if clip.width > 0.0 && clip.height > 0.0 {
            out.push(DisplayCommand::PushClipRect { rect: clip });
            out.push(border(rect));
            out.push(DisplayCommand::PopClip);
        }
    }
}
