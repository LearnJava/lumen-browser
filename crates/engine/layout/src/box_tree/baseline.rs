//! Модель базовой линии бокса для выравнивания `align-items/align-self:
//! baseline | last baseline` (CSS Box Alignment L3 §9, CSS Flexbox L1 §8.5).
//!
//! Отличается от [`inline_baseline`] (CSS 2.1 §10.8.1, inline-уровень): там
//! `inline-block` с `overflow` ≠ `visible` и пустой `inline-block` садятся на
//! нижнюю кромку margin box, а во flex-выравнивании базовая линия берётся из
//! содержимого всегда, а недостающая синтезируется вызывающим по border box.
//! Всё считается по уже разложенному дереву: отсчёт — от верхней кромки
//! border box самого бокса, `None` — «своей» базовой линии нет.

use super::*;
use super::inline_build::{control_value_baseline, form_control_has_text_baseline};
use crate::style::WritingMode;

/// Какая базовая линия нужна — первая (`baseline`/`first baseline`) или
/// последняя (`last baseline`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BaselineSide {
    First,
    Last,
}

/// Участвует ли бокс в нормальном потоке своего родителя — плавающие,
/// абсолютно позиционированные и служебные дети базовую линию родителю не дают.
fn is_in_flow_baseline_source(c: &LayoutBox) -> bool {
    c.style.float_side == FloatSide::None
        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
        && !matches!(c.kind, BoxKind::Skip | BoxKind::InlineSpace | BoxKind::Marker { .. })
}

/// Базовая линия `b` стороны `side` от верхней кромки его border box.
///
/// * `InlineRun` — первая/последняя строка текста;
/// * замещаемые элементы и нетекстовые контролы — `None` (синтез по border box);
/// * flex-контейнер — [`flex_container_baseline`] (§8.5);
/// * остальные контейнеры — базовая линия первого/последнего in-flow ребёнка,
///   у которого она есть (CSS 2.1 §10.8.1, CSS Align §9.1).
///
/// Бокс с вертикальным `writing-mode` базовой линии, параллельной строке
/// горизонтального контейнера, не имеет — `None`.
pub(crate) fn box_baseline(
    b: &LayoutBox,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<f32> {
    let bl = content_baseline(b, side, measurer)?;
    // CSS Align §9.1: базовая линия контейнера прокрутки берётся из содержимого,
    // но не может выйти за border box — переполнение её не двигает.
    let scrolls = |o: Overflow| matches!(o, Overflow::Hidden | Overflow::Scroll | Overflow::Auto);
    if scrolls(b.style.overflow_x) || scrolls(b.style.overflow_y) {
        return Some(bl.clamp(0.0, b.rect.height.max(0.0)));
    }
    Some(bl)
}

fn content_baseline(
    b: &LayoutBox,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<f32> {
    if b.style.writing_mode != WritingMode::HorizontalTb {
        return None;
    }
    match &b.kind {
        BoxKind::InlineRun { lines, .. } => {
            if lines.is_empty() {
                return None;
            }
            let m = measurer?;
            // Та же модель, что у `inline_baseline` (CSS 2.1 §10.8.1, сверена с
            // Edge на IFC-1): половинное межстрочье и подъём шрифта над низом
            // строки. Низ строки `k` — `(k + 1) * H / n`: так «первая»
            // базовая линия однострочного прогона совпадает с «последней», а у
            // многострочного не расходится с `::first-line` и `line-height-step`.
            let em = b.style.font_size;
            let line_h = step_line_height(em * b.style.line_height, b.style.line_height_step);
            let ascent = m.ascent_px_with_families(em, &b.style.font_family);
            let descent = m.descent_px_with_families(em, &b.style.font_family);
            let half_leading = (line_h - (ascent + descent)) / 2.0;
            let line_bottom = match side {
                BaselineSide::First => b.rect.height / lines.len() as f32,
                BaselineSide::Last => b.rect.height,
            };
            Some(line_bottom - line_h + half_leading + ascent)
        }
        // Ряд inline-уровневых боксов: базовая линия — у первого (последнего)
        // участника, выровненного по базовой линии. Atomic inline (inline-block,
        // замещаемый) садится на строку своей inline-базовой линией (CSS 2.1
        // §10.8.1), а при её отсутствии — нижней кромкой margin box; текстовый
        // прогон даёт линию своей строки.
        BoxKind::InlineBlockRow => {
            let part = |c: &LayoutBox| -> Option<f32> {
                if !is_in_flow_baseline_source(c) || !matches!(inline_v_align(c), VerticalAlign::Baseline) {
                    return None;
                }
                let bl = if matches!(c.kind, BoxKind::InlineRun { .. }) {
                    box_baseline(c, side, measurer)?
                } else {
                    inline_baseline(c, measurer).unwrap_or_else(|| {
                        let st = &c.style;
                        c.rect.height + st.margin_bottom.resolve_or_zero(st.font_size, 0.0, Size::ZERO)
                    })
                };
                Some(c.rect.y - b.rect.y + bl)
            };
            match side {
                BaselineSide::First => b.children.iter().find_map(part),
                BaselineSide::Last => b.children.iter().rev().find_map(part),
            }
        }
        BoxKind::Image { .. }
        | BoxKind::Video { .. }
        | BoxKind::Canvas { .. }
        | BoxKind::Audio { .. }
        | BoxKind::Iframe { .. }
        | BoxKind::SvgRoot { .. }
        | BoxKind::SvgShape { .. }
        | BoxKind::SvgText { .. } => None,
        BoxKind::FormControl { kind } => {
            if form_control_has_text_baseline(kind) {
                control_value_baseline(b, measurer)
            } else {
                None
            }
        }
        BoxKind::Table => super::table_valign::table_baseline(b, side, measurer),
        _ => {
            if matches!(b.style.display, Display::Flex | Display::InlineFlex) {
                return flex_container_baseline(b, side, measurer);
            }
            let scan = |c: &LayoutBox| -> Option<f32> {
                if !is_in_flow_baseline_source(c) {
                    return None;
                }
                box_baseline(c, side, measurer).map(|bl| c.rect.y - b.rect.y + bl)
            };
            match side {
                BaselineSide::First => b.children.iter().find_map(scan),
                BaselineSide::Last => b.children.iter().rev().find_map(scan),
            }
        }
    }
}

/// Базовая линия бокса для выравнивания: своя, а при её отсутствии —
/// синтезированная по нижней кромке border box (CSS Align §9.1: alphabetic
/// baseline = line-under edge).
pub(crate) fn box_baseline_or_synth(
    b: &LayoutBox,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> f32 {
    box_baseline(b, side, measurer).unwrap_or(b.rect.height)
}

/// `align-self` flex-item'а с учётом `align-items` контейнера.
pub(crate) fn resolved_align(item: &ComputedStyle, container: &ComputedStyle) -> AlignValue {
    if matches!(item.align_self, AlignValue::Auto) { container.align_items } else { item.align_self }
}

/// Сторона базовой линии, по которой выравнивается значение `align`, если оно
/// baseline-значение.
pub(crate) fn align_baseline_side(align: AlignValue) -> Option<BaselineSide> {
    match align {
        AlignValue::Baseline => Some(BaselineSide::First),
        AlignValue::LastBaseline => Some(BaselineSide::Last),
        _ => None,
    }
}

/// CSS Flexbox L1 §8.5 — первая/последняя базовая линия flex-контейнера по оси
/// строки; `None` у пустого контейнера.
///
/// 1. Если на первой (последней) линии есть items, выровненные по базовой
///    линии, — общая базовая линия этой группы.
/// 2. Иначе — базовая линия крайнего items линии: у «первой» — самого
///    верхнего/левого (визуально, поэтому у `*-reverse` это последний в порядке
///    обхода), у «последней» — самого нижнего/правого.
///
/// Линии восстанавливаются по раскладке: внутри линии позиция по главной оси
/// монотонна, и новая линия начинается там, где она перестаёт расти (у
/// `*-reverse` — убывать). «Первая» линия — визуально верхняя (при
/// `wrap-reverse` это последняя в порядке обхода).
pub(crate) fn flex_container_baseline(
    b: &LayoutBox,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<f32> {
    let s = &b.style;
    // Физические оси (FLEX-VWM): `direction: rtl` разворачивает ряд так же, как
    // `row-reverse` — позиции по главной оси убывают.
    let axes = super::flex::flex_axes(s);
    let (is_column, is_reverse) = (axes.main_vertical, axes.main_rev);
    let is_wrap_reverse = matches!(s.flex_wrap, FlexWrap::WrapReverse);

    let mut items: Vec<&LayoutBox> = b
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .collect();
    if items.is_empty() {
        return None;
    }
    items.sort_by_key(|c| c.style.order);

    // Линии в порядке обхода.
    let main_pos = |c: &LayoutBox| if is_column { c.rect.y } else { c.rect.x };
    let main_size = |c: &LayoutBox| if is_column { c.rect.height } else { c.rect.width };
    let mut lines: Vec<Vec<&LayoutBox>> = Vec::new();
    let mut prev: Option<&LayoutBox> = None;
    for it in items {
        let starts_line = match prev {
            None => true,
            Some(p) => {
                let (pp, ip) = (main_pos(p), main_pos(it));
                let advanced = if is_reverse { ip < pp } else { ip > pp };
                !advanced && !(ip == pp && main_size(p) <= 0.0)
            }
        };
        if starts_line {
            lines.push(Vec::new());
        }
        if let Some(l) = lines.last_mut() {
            l.push(it);
        }
        prev = Some(it);
    }

    // Визуально первая линия — первая в обходе, кроме `wrap-reverse` строки, где
    // линии идут снизу вверх: «первая» базовая линия — у верхней (так её считают
    // WPT `flex-align-baseline-flex-003`, `flexbox-baseline-multi-line-horiz-004`).
    let visual_first_is_first_visited = is_column || !is_wrap_reverse;
    let line = match (side, visual_first_is_first_visited) {
        (BaselineSide::First, true) | (BaselineSide::Last, false) => lines.first()?,
        _ => lines.last()?,
    };

    let shift = |c: &LayoutBox, bl: f32| c.rect.y - b.rect.y + bl;

    // 1. Items, выровненные по базовой линии (только строки: во flex-колонке
    // базовая линия — по ортогональной оси и группы не образует). Общая
    // базовая линия группы той же стороны, а если такой группы на линии нет —
    // другой.
    if !is_column {
        let other = match side {
            BaselineSide::First => BaselineSide::Last,
            BaselineSide::Last => BaselineSide::First,
        };
        for want in [side, other] {
            let participant = line.iter().copied().find(|c| {
                align_baseline_side(resolved_align(&c.style, s)) == Some(want)
                    && !matches!(c.style.margin_top, LengthOrAuto::Auto)
                    && !matches!(c.style.margin_bottom, LengthOrAuto::Auto)
            });
            if let Some(c) = participant {
                return Some(shift(c, box_baseline_or_synth(c, want, measurer)));
            }
        }
    }

    // 2. Крайний item линии.
    let first_visited = *line.first()?;
    let last_visited = *line.last()?;
    let extreme = match (side, is_reverse) {
        (BaselineSide::First, false) | (BaselineSide::Last, true) => first_visited,
        _ => last_visited,
    };
    Some(shift(extreme, box_baseline_or_synth(extreme, side, measurer)))
}
