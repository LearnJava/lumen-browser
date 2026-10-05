//! Модель базовой линии бокса для выравнивания `align-items/align-self:
//! baseline | last baseline` (CSS Box Alignment L3 §9, CSS Flexbox L1 §8.5).
//!
//! Отличается от [`inline_baseline`] (CSS 2.1 §10.8.1, inline-уровень): там
//! `inline-block` с `overflow` ≠ `visible` и пустой `inline-block` садятся на
//! нижнюю кромку margin box, а во flex-выравнивании базовая линия берётся из
//! содержимого всегда, а недостающая синтезируется вызывающим по border box.
//! Всё считается по уже разложенному дереву: отсчёт — от верхней кромки
//! border box самого бокса (у бокса с вертикальным `writing-mode` — от левой:
//! его базовая линия вертикальна, а положение измеряется по горизонтали),
//! `None` — «своей» базовой линии нет.

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

/// Физический край бокса (верхний/левый — `Min`, нижний/правый — `Max`), к
/// которому «тянется» базовая линия при выравнивании.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PhysSide {
    Min,
    Max,
}

/// Вертикальный `writing-mode`: строки идут по вертикали, базовая линия —
/// вертикальная линия, её положение измеряется по горизонтали.
pub(crate) fn is_vertical(s: &ComputedStyle) -> bool {
    s.writing_mode != WritingMode::HorizontalTb
}

/// Блоки в вертикальном режиме идут справа налево (`vertical-rl`, `sideways-rl`).
fn block_flow_is_rtl(s: &ComputedStyle) -> bool {
    matches!(s.writing_mode, WritingMode::VerticalRl | WritingMode::SidewaysRl)
}

/// Край, к которому тянется базовая линия стороны `side` бокса со стилем `s`:
/// `first` — край начала блока (сверху; справа у `vertical-rl`), `last` — конца
/// (CSS Align L3 §9.3: запасное выравнивание — `start`/`end` режима самого
/// бокса). Поэтому `first baseline` бокса `vertical-rl` и `last baseline` бокса
/// `vertical-lr` делят одну группу выравнивания.
pub(crate) fn baseline_phys_side(s: &ComputedStyle, side: BaselineSide) -> PhysSide {
    let starts_at_min = !block_flow_is_rtl(s);
    match (side, starts_at_min) {
        (BaselineSide::First, true) | (BaselineSide::Last, false) => PhysSide::Min,
        _ => PhysSide::Max,
    }
}

/// Синтезированная базовая линия (CSS Align L3 §9.1): у горизонтального бокса —
/// нижняя кромка border box, у вертикального — центральная (середина border box).
fn synth_baseline(b: &LayoutBox) -> f32 {
    if is_vertical(&b.style) { b.rect.width / 2.0 } else { b.rect.height }
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
        let extent = if is_vertical(&b.style) { b.rect.width } else { b.rect.height };
        return Some(bl.clamp(0.0, extent.max(0.0)));
    }
    Some(bl)
}

fn content_baseline(
    b: &LayoutBox,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<f32> {
    if is_vertical(&b.style) {
        return vertical_content_baseline(b, side, measurer);
    }
    match &b.kind {
        BoxKind::InlineRun { lines, .. } => {
            let k = match side {
                BaselineSide::First => 0,
                BaselineSide::Last => lines.len().checked_sub(1)?,
            };
            run_line_baseline(b, k, measurer)
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
            // CSS Grid L1 §6.1: линии считает раскладка сетки (`grid_trampoline`).
            if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
                return b.grid_baselines.map(|(first, last)| if side == BaselineSide::First { first } else { last });
            }
            // CSS Overflow L4 §3.2 (`line-clamp`): последняя базовая линия — у
            // последней видимой строки, а не у последней строки содержимого.
            if let Some(n) = b.style.line_clamp.filter(|&n| n > 0)
                && side == BaselineSide::Last
            {
                let mut lines = Vec::new();
                collect_line_baselines(b, measurer, n as usize + 1, &mut lines);
                if lines.len() > n as usize {
                    return lines.get(n as usize - 1).copied();
                }
            }
            if (b.style.column_count.is_some() || b.style.column_width.is_some()) && !b.children.is_empty() {
                return multicol_baseline(b, side, measurer);
            }
            // Дети с ортогональным (вертикальным) режимом линий этой оси не дают. Rendered
            // legend fieldset'а в базовой линии не участвует (HTML Rendering §15.3.13).
            let legend = b.fieldset_legend.filter(|l| l.placed).map(|l| l.idx);
            let scan = |(i, c): (usize, &LayoutBox)| -> Option<f32> {
                if Some(i) == legend || !is_in_flow_baseline_source(c) || is_vertical(&c.style) {
                    return None;
                }
                box_baseline(c, side, measurer).map(|bl| c.rect.y - b.rect.y + bl)
            };
            match side {
                BaselineSide::First => b.children.iter().enumerate().find_map(scan),
                BaselineSide::Last => b.children.iter().enumerate().rev().find_map(scan),
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
    box_baseline(b, side, measurer).unwrap_or_else(|| synth_baseline(b))
}

/// Край линии выравнивания (верх/лево или низ/право), к которому тянется базовая
/// линия стороны `side` бокса `b` в контексте с вертикальностью `vertical`: у бокса
/// того же режима — край начала/конца его блока ([`baseline_phys_side`]), у
/// ортогонального — начало оси для `first` и конец для `last` в режиме письма
/// контейнера `container` (справа у `direction: rtl`, WPT
/// `flex-align-baseline-column-rtl-direction`).
pub(crate) fn baseline_phys_side_in_axis(
    b: &LayoutBox,
    container: &ComputedStyle,
    vertical: bool,
    side: BaselineSide,
) -> PhysSide {
    if is_vertical(&b.style) == vertical {
        return baseline_phys_side(&b.style, side);
    }
    let start_is_low = super::flex_trampoline::own_start_is_low(container, vertical);
    if (side == BaselineSide::First) == start_is_low { PhysSide::Min } else { PhysSide::Max }
}

/// Базовая линия `b` в контексте выравнивания с вертикальностью `vertical`
/// (`false` — линия горизонтальная, положение по y) контейнера `container`.
/// Ортогональный бокс своей базовой линии в этом контексте не имеет —
/// синтезируется по краю border box (CSS Align L3 §9.1): тому, к которому он
/// тянется ([`baseline_phys_side_in_axis`]), так бокс целиком лежит по одну сторону
/// общей линии (WPT `flex-align-baseline-005`, `align-items-baseline-column-horz`).
pub(crate) fn box_baseline_in_axis(
    b: &LayoutBox,
    container: &ComputedStyle,
    vertical: bool,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> f32 {
    if is_vertical(&b.style) == vertical {
        return box_baseline_or_synth(b, side, measurer);
    }
    match baseline_phys_side_in_axis(b, container, vertical, side) {
        PhysSide::Min => 0.0,
        PhysSide::Max => {
            if vertical { b.rect.width } else { b.rect.height }
        }
    }
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
    // Положение базовой линии контейнера — по его собственной оси строк: по y у
    // горизонтального режима, по x у вертикального.
    let cvert = is_vertical(s);
    let origin = |c: &LayoutBox| if cvert { c.rect.x - b.rect.x } else { c.rect.y - b.rect.y };

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

    // «Первая» линия — у края поперечной оси, ближайшего к началу строки/блока контейнера
    // в его режиме письма (сверху для горизонтального режима; слева у `ltr`-колонки;
    // справа у блоков `vertical-rl`), а не поперечное начало flex: при `wrap-reverse` линии
    // идут снизу вверх (справа налево), и «первая» базовая линия — у верхней (так её
    // считают WPT `flex-align-baseline-flex-003`, `flexbox-baseline-multi-line-horiz-004`).
    // Линии посещаются от поперечного начала: оно у низкого края без `cross_rev`.
    let cross_is_block_axis = cvert == is_column;
    let cross_start_low = super::flex_trampoline::own_start_is_low(s, is_column);
    let visual_first_is_first_visited = cross_start_low != axes.cross_rev;
    let line = match (side, visual_first_is_first_visited) {
        (BaselineSide::First, true) | (BaselineSide::Last, false) => lines.first()?,
        _ => lines.last()?,
    };

    // 1. Items, выровненные по базовой линии (поперечная ось — ось строк, и item с
    // тем же режимом: во flex-колонке горизонтального режима базовая линия по
    // ортогональной оси и группы не образует). Общая базовая линия группы той же
    // стороны, а если такой группы на линии нет — другой.
    if cross_is_block_axis {
        let other = match side {
            BaselineSide::First => BaselineSide::Last,
            BaselineSide::Last => BaselineSide::First,
        };
        for want in [side, other] {
            let participant = line.iter().copied().find(|c| {
                let auto_margin = if cvert {
                    matches!(c.style.margin_left, LengthOrAuto::Auto) || matches!(c.style.margin_right, LengthOrAuto::Auto)
                } else {
                    matches!(c.style.margin_top, LengthOrAuto::Auto) || matches!(c.style.margin_bottom, LengthOrAuto::Auto)
                };
                is_vertical(&c.style) == cvert
                    && align_baseline_side(resolved_align(&c.style, s)) == Some(want)
                    && !auto_margin
            });
            if let Some(c) = participant {
                return Some(origin(c) + box_baseline_or_synth(c, want, measurer));
            }
        }
    }

    // 2. Крайний item линии.
    let first_visited = *line.first()?;
    let last_visited = *line.last()?;
    // «Первая» базовая линия — у item'а, ближайшего к началу строки контейнера в его
    // режиме письма, «последняя» — к концу. В горизонтальном режиме это левый (верхний)
    // край, и `*-reverse` его меняет (визуально первый — последний в обходе); в
    // вертикальном начало главной оси задаёт режим: справа у блоков `vertical-rl`.
    let first_is_first_visited = if cvert {
        let start_is_low = super::flex_trampoline::own_start_is_low(s, !is_column);
        start_is_low != is_reverse
    } else {
        !is_reverse
    };
    let extreme = match (side, first_is_first_visited) {
        (BaselineSide::First, true) | (BaselineSide::Last, false) => first_visited,
        _ => last_visited,
    };
    Some(origin(extreme) + box_baseline_in_axis(extreme, s, cvert, side, measurer))
}

/// Базовая линия строки `k` текстового прогона `b` от верхней кромки прогона.
/// Та же модель, что у `inline_baseline` (CSS 2.1 §10.8.1, сверена с Edge на
/// IFC-1): половинное межстрочье и подъём шрифта над низом строки. Низ строки
/// `k` — `(k + 1) * H / n`: так «первая» базовая линия однострочного прогона
/// совпадает с «последней», а у многострочного не расходится с `::first-line` и
/// `line-height-step`.
fn run_line_baseline(b: &LayoutBox, k: usize, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    let BoxKind::InlineRun { lines, .. } = &b.kind else { return None };
    if k >= lines.len() {
        return None;
    }
    let m = measurer?;
    let em = b.style.font_size;
    let line_h = step_line_height(em * b.style.line_height, b.style.line_height_step);
    let ascent = m.ascent_px_with_families(em, &b.style.font_family);
    let descent = m.descent_px_with_families(em, &b.style.font_family);
    let half_leading = (line_h - (ascent + descent)) / 2.0;
    let line_bottom = b.rect.height * (k + 1) as f32 / lines.len() as f32;
    Some(line_bottom - line_h + half_leading + ascent)
}

/// Базовые линии первых `limit` строк блока `b` в порядке потока (от верхней
/// кромки `b`): строки текстовых прогонов, ряды inline-уровневых боксов и строки
/// вложенных блоков, не создающих собственного контекста.
fn collect_line_baselines(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>, limit: usize, out: &mut Vec<f32>) {
    for c in b.children.iter().filter(|c| is_in_flow_baseline_source(c) && !is_vertical(&c.style)) {
        if out.len() >= limit {
            return;
        }
        let dy = c.rect.y - b.rect.y;
        match &c.kind {
            BoxKind::InlineRun { lines, .. } => {
                for k in 0..lines.len().min(limit - out.len()) {
                    if let Some(bl) = run_line_baseline(c, k, measurer) {
                        out.push(dy + bl);
                    }
                }
            }
            BoxKind::InlineBlockRow => {
                if let Some(bl) = box_baseline(c, BaselineSide::First, measurer) {
                    out.push(dy + bl);
                }
            }
            BoxKind::Block if c.style.display == Display::Block && c.style.line_clamp.is_none() => {
                let mut inner = Vec::new();
                collect_line_baselines(c, measurer, limit - out.len(), &mut inner);
                out.extend(inner.into_iter().map(|bl| dy + bl));
            }
            _ => {}
        }
    }
}

/// Базовая линия бокса с вертикальным `writing-mode` — центральная (CSS Writing
/// Modes L3 §4.1, `text-orientation: mixed`): середина строки (колонки) по
/// горизонтали. Отсчёт от левой кромки border box; «первая» строка — у начала
/// блока (справа у `vertical-rl`, слева у `vertical-lr`).
fn vertical_content_baseline(b: &LayoutBox, side: BaselineSide, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    let rtl = block_flow_is_rtl(&b.style);
    match &b.kind {
        BoxKind::InlineRun { lines, .. } => {
            let n = lines.len();
            let k = match side {
                BaselineSide::First => 0,
                BaselineSide::Last => n.checked_sub(1)?,
            };
            let frac = (k as f32 + 0.5) / n as f32;
            Some(b.rect.width * if rtl { 1.0 - frac } else { frac })
        }
        // Ряд atomic inline: линия — у первого (последнего) участника с
        // `vertical-align: baseline`; у участника без своей линии — середина border box.
        BoxKind::InlineBlockRow => {
            let part = |c: &LayoutBox| -> Option<f32> {
                if !is_in_flow_baseline_source(c) || !matches!(inline_v_align(c), VerticalAlign::Baseline) {
                    return None;
                }
                Some(c.rect.x - b.rect.x + box_baseline_in_axis(c, &b.style, true, side, measurer))
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
        | BoxKind::SvgText { .. }
        | BoxKind::FormControl { .. }
        | BoxKind::Table => None,
        _ => {
            if matches!(b.style.display, Display::Flex | Display::InlineFlex) {
                return flex_container_baseline(b, side, measurer);
            }
            if matches!(b.style.display, Display::Grid | Display::InlineGrid) {
                return b.grid_baselines.map(|(first, last)| if side == BaselineSide::First { first } else { last });
            }
            // Дети с ортогональным режимом линий этой оси не дают; rendered legend fieldset'а
            // в базовой линии не участвует (HTML Rendering §15.3.13).
            let legend = b.fieldset_legend.filter(|l| l.placed).map(|l| l.idx);
            let scan = |(i, c): (usize, &LayoutBox)| -> Option<f32> {
                if Some(i) == legend || !is_in_flow_baseline_source(c) || !is_vertical(&c.style) {
                    return None;
                }
                box_baseline(c, side, measurer).map(|bl| c.rect.x - b.rect.x + bl)
            };
            match side {
                BaselineSide::First => b.children.iter().enumerate().find_map(scan),
                BaselineSide::Last => b.children.iter().enumerate().rev().find_map(scan),
            }
        }
    }
}

/// Базовая линия multicol-контейнера (CSS Align L3 §9.1, как её считают браузеры):
/// первая — линия первого in-flow сегмента, последняя — последнего. Сегмент —
/// `column-span: all` элемент (его собственная линия) либо набор колонок между
/// спаннерами: у набора «первая» — линия верхнего ребёнка первой колонки, а
/// «последняя» — самая нижняя последняя линия среди всех колонок.
fn multicol_baseline(b: &LayoutBox, side: BaselineSide, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    use super::multicol_span::is_column_spanner;
    let at = |c: &LayoutBox| box_baseline(c, side, measurer).map(|bl| c.rect.y - b.rect.y + bl);
    let flow = || b.children.iter().filter(|c| is_in_flow_baseline_source(c) && !is_vertical(&c.style));
    // Сегменты: ребёнок-спаннер — отдельный, подряд идущие остальные — набор колонок.
    let mut segments: Vec<Vec<&LayoutBox>> = Vec::new();
    let mut in_columns = false;
    for c in flow() {
        if is_column_spanner(c) {
            segments.push(vec![c]);
            in_columns = false;
        } else if in_columns {
            if let Some(seg) = segments.last_mut() {
                seg.push(c);
            }
        } else {
            segments.push(vec![c]);
            in_columns = true;
        }
    }
    let spans = |seg: &[&LayoutBox]| seg.len() == 1 && is_column_spanner(seg[0]);
    let segment_baseline = |seg: &[&LayoutBox]| -> Option<f32> {
        if spans(seg) {
            return at(seg[0]);
        }
        match side {
            BaselineSide::First => {
                let first_x = seg.iter().map(|c| c.rect.x).fold(f32::INFINITY, f32::min);
                seg.iter().filter(|c| c.rect.x <= first_x + 0.5).find_map(|c| at(c))
            }
            BaselineSide::Last => seg.iter().filter_map(|c| at(c)).reduce(f32::max),
        }
    };
    match side {
        BaselineSide::First => segments.iter().find_map(|seg| segment_baseline(seg)),
        BaselineSide::Last => segments.iter().rev().find_map(|seg| segment_baseline(seg)),
    }
}
