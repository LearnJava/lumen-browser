//! `line-clamp` / `-webkit-line-clamp` по потоку контейнера (CSS Overflow L4
//! §line-clamp, ROADMAP `LINE-CLAMP-BOX`).
//!
//! Строки считаются по всему нормальному потоку контейнера: строки текстовых
//! прогонов, ряды atomic inline и строки вложенных блоков, не создающих
//! собственного контекста форматирования. Раскладка делает это в два шага вокруг
//! `finalize_block_height`:
//!
//! 1. [`find_cut`] — низ n-й строки: по нему урезается высота содержимого;
//! 2. [`apply_cut`] — после `finish_after_match` (абсолютные дети уже разложены по
//!    индексам `abs_deferred`, поэтому удалять детей раньше нельзя): убирает всё
//!    ниже линии отсечки и дописывает многоточие на последней видимой строке.
//!
//! `line-clamp: auto` берёт число строк из высоты: помещаются те строки, чей низ не
//! ниже границы `height`/`max-height` ([`auto_bound`]).

use super::baseline::is_in_flow_baseline_source;
use super::diagnostics::resolve_block_size;
use super::*;

/// Допуск сравнения координат: строки делят `rect.height` на их число.
const EPS: f32 = 0.01;

/// Линия отсечки: низ последней видимой строки (от верхней кромки border box
/// контейнера) и сумма нижних padding+border вложенных блоков, в которых она лежит —
/// контейнер заканчивается так, как если бы после неё содержимого не было.
#[derive(Clone, Copy)]
pub(super) struct Cut {
    pub line_bottom: f32,
    pub extra: f32,
}

struct Line {
    bottom: f32,
    frame: f32,
}

enum Mode {
    Count(usize),
    Auto,
}

/// Контейнер, чьи строки считает `line-clamp`: блочный бокс самого элемента с
/// горизонтальным режимом письма. Анонимные обёртки наследуют `line_clamp` через
/// клон стиля родителя, но отсекает один контейнер — иначе многоточие встанет дважды.
fn clamp_mode(b: &LayoutBox) -> Option<Mode> {
    if b.origin.role != BoxRole::Element || is_vertical_flow(&b.style) {
        return None;
    }
    // Legacy `-webkit-line-clamp` clamps only a `-webkit-box` laid out along the block axis.
    if b.style.line_clamp_legacy
        && !(matches!(b.style.display, Display::WebkitBox | Display::WebkitInlineBox)
            && b.style.box_orient == crate::style::WebkitBoxOrient::Vertical)
    {
        return None;
    }
    match b.style.line_clamp.filter(|&n| n > 0) {
        Some(n) => Some(Mode::Count(n as usize)),
        None => b.style.line_clamp_auto.then_some(Mode::Auto),
    }
}

fn is_vertical_flow(s: &ComputedStyle) -> bool {
    !matches!(s.writing_mode, crate::style::WritingMode::HorizontalTb)
}

/// Вложенный блок, строки которого считаются строками контейнера: обычный блок
/// того же потока (не флоат, не BFC-корень, не таблица/flex/grid).
fn is_transparent_block(c: &LayoutBox) -> bool {
    matches!(c.kind, BoxKind::Block)
        && matches!(c.style.display, Display::Block | Display::ListItem)
        && !super::bfc::establishes_bfc(c)
        && !is_vertical_flow(&c.style)
        && c.style.column_count.is_none()
        && c.style.column_width.is_none()
}

/// Нижние padding+border блока `c` — то, что остаётся под его последней строкой.
fn bottom_frame(c: &LayoutBox, parent_width: f32, viewport: Size) -> f32 {
    let pad = c.style.padding_bottom.resolve(c.style.font_size, Some(parent_width), viewport).unwrap_or(0.0);
    pad + c.style.border_bottom_width
}

/// Нижние кромки (абсолютные y) первых `limit` строк потока `b` в порядке следования.
fn collect_lines(b: &LayoutBox, viewport: Size, limit: usize, frame: f32, out: &mut Vec<Line>) {
    for c in b.children.iter().filter(|c| is_in_flow_baseline_source(c)) {
        if out.len() >= limit {
            return;
        }
        match &c.kind {
            BoxKind::InlineRun { lines, .. } if !lines.is_empty() => {
                let n = lines.len();
                for k in 0..n.min(limit - out.len()) {
                    out.push(Line { bottom: c.rect.y + c.rect.height * (k + 1) as f32 / n as f32, frame });
                }
            }
            BoxKind::InlineBlockRow => out.push(Line { bottom: c.rect.y + c.rect.height, frame }),
            _ if is_transparent_block(c) => {
                collect_lines(c, viewport, limit, frame + bottom_frame(c, b.rect.width, viewport), out);
            }
            _ => {}
        }
    }
}

/// Есть ли в потоке `b` ниже `after` бокс с высотой — пусть и без строк (блок без
/// текста, BFC-корень, таблица, картинка): за точкой отсечки он должен быть скрыт.
fn has_content_after(b: &LayoutBox, after: f32) -> bool {
    b.children.iter().filter(|c| is_in_flow_baseline_source(c)).any(|c| {
        if c.rect.y >= after - EPS {
            c.rect.height > EPS
        } else {
            is_transparent_block(c) && has_content_after(c, after)
        }
    })
}

/// Нижняя граница строк для `line-clamp: auto` (абсолютный y): заданная `height` или
/// `max-height` (меньшая из них) по content box. `None` — высота не определена.
#[allow(clippy::too_many_arguments)]
pub(super) fn auto_bound(
    s: &ComputedStyle,
    em: f32,
    available_height: Option<f32>,
    viewport: Size,
    padding_top: f32,
    padding_bottom: f32,
    content_y: f32,
) -> Option<f32> {
    if !s.line_clamp_auto {
        return None;
    }
    let frame = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;
    let content_size = |len: &Length| -> Option<f32> {
        let h = resolve_block_size(len, em, available_height, viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => h,
            BoxSizing::BorderBox => (h - frame).max(0.0),
        })
    };
    let h = s.height.as_ref().and_then(content_size);
    let max = s.max_height.as_ref().and_then(content_size);
    let size = match (h, max) {
        (Some(h), Some(m)) => h.min(m),
        (Some(v), None) | (None, Some(v)) => v,
        (None, None) => return None,
    };
    Some(content_y + size)
}

/// Линия отсечки контейнера; `None`, если усекать нечего. `auto_bound` — результат
/// [`auto_bound`] для `line-clamp: auto`.
pub(super) fn find_cut(b: &LayoutBox, viewport: Size, auto_bound: Option<f32>) -> Option<Cut> {
    let mode = clamp_mode(b)?;
    let mut lines = Vec::new();
    let (keep, check_trailing) = match mode {
        Mode::Count(n) => {
            collect_lines(b, viewport, n + 1, 0.0, &mut lines);
            if lines.len() < n {
                return None;
            }
            (n, true)
        }
        Mode::Auto => {
            let bound = auto_bound?;
            collect_lines(b, viewport, usize::MAX, 0.0, &mut lines);
            let fit = lines.iter().take_while(|l| l.bottom + l.frame <= bound + EPS).count();
            (fit.max(1), false)
        }
    };
    let last = lines.get(keep - 1)?;
    let more = lines.len() > keep || (check_trailing && has_content_after(b, last.bottom));
    more.then_some(Cut { line_bottom: last.bottom - b.rect.y, extra: last.frame })
}

/// Убирает всё, что целиком ниже линии отсечки, режет прогон, на который она
/// приходится, и дописывает на его последней строке многоточие.
pub(super) fn apply_cut(b: &mut LayoutBox, cut: Cut, viewport: Size, measurer: Option<&dyn TextMeasurer>) {
    let cut_abs = b.rect.y + cut.line_bottom;
    truncate_flow(b, cut_abs, viewport, measurer);
}

fn truncate_flow(b: &mut LayoutBox, cut_abs: f32, viewport: Size, measurer: Option<&dyn TextMeasurer>) {
    b.children.retain(|c| !(is_in_flow_baseline_source(c) && c.rect.y >= cut_abs - EPS));
    let width = b.rect.width;
    for c in b.children.iter_mut().filter(|c| is_in_flow_baseline_source(c)) {
        let bottom = c.rect.y + c.rect.height;
        if bottom < cut_abs - EPS {
            continue;
        }
        if is_transparent_block(c) {
            truncate_flow(c, cut_abs, viewport, measurer);
            let kept = (cut_abs - c.rect.y).max(0.0) + bottom_frame(c, width, viewport);
            c.rect.height = kept.min(c.rect.height);
        } else if let BoxKind::InlineRun { lines, .. } = &mut c.kind
            && !lines.is_empty()
        {
            let old_n = lines.len();
            let line_h = c.rect.height / old_n as f32;
            let keep = (((cut_abs - c.rect.y) / line_h).round() as usize).clamp(1, old_n);
            lines.truncate(keep);
            c.rect.height = line_h * keep as f32;
            if let (Some(m), Some(last)) = (measurer, lines.last_mut()) {
                ellipsize_last_line(last, c.rect.width, c.style.font_size, m);
            }
        }
    }
}
