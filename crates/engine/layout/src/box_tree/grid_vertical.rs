//! Grid-контейнер в вертикальном `writing-mode` (GRID-VWM).
//!
//! CSS Grid L1 §1.1 / CSS Writing Modes L3 §3: столбцы (`grid-template-columns`,
//! `justify-*`) идут по inline-оси режима, строки (`grid-template-rows`, `align-*`) — по
//! block-оси. В `vertical-*` inline-ось — физическая `y` (сверху вниз), block-ось — `x`
//! (справа налево для `vertical-rl`, слева направо для `vertical-lr`).
//!
//! Размещение, размеры дорожек и распределение `align-content` остаются логическими —
//! `build_grid_init` и проходы `grid_trampoline` считают их так же, как для горизонтального
//! режима. Здесь живёт то, что знает про физические оси: пролог контейнера (размеры
//! по inline/block), перевод ячейки в физические координаты, выравнивание элемента в
//! ячейке и итоговая ширина контейнера.
//!
//! Не реализовано: `direction: rtl` (столбцы снизу вверх; в горизонтальной сетке его
//! тоже нет), baseline-группы по физической оси `x`, элементы с ортогональным режимом
//! письма, авто-поля.

use super::*;
use super::block_flow_trampoline::DispatchOutcome;
use super::grid::{grid_track_span};
use super::baseline::{box_baseline_in_axis, BaselineSide};
use super::grid_trampoline::{GridInit, ItemBaseline};
use crate::style::{ContentSide, WritingMode};
use crate::subgrid::{SubgridContext, SubgridContextGuard};

/// Физическая геометрия вертикального grid-контейнера, нужная проходам `grid_trampoline`.
#[derive(Clone, Copy)]
pub(super) struct VGridGeom {
    /// block-ось идёт справа налево (`vertical-rl`, `sideways-rl`).
    pub(super) rl: bool,
    /// inline-ось идёт снизу вверх: `direction: rtl` в `vertical-*` либо `sideways-lr`
    /// (в нём линия письма поднимается вверх); оба сразу — снова сверху вниз.
    pub(super) inline_rev: bool,
    /// Доступный block-размер (физическая ширина), который видят элементы при замере,
    /// пока высота строк неизвестна.
    pub(super) block_avail: f32,
    /// padding + border по левой и правой сторонам контейнера.
    pub(super) frame_horiz: f32,
    /// Использованная ширина border box, если `width` определена (после `min-/max-width`).
    pub(super) explicit_width: Option<f32>,
    pub(super) min_block: f32,
    pub(super) max_block: f32,
}

/// Значение выравнивания, при котором элемент растягивается на область (`normal` у
/// grid-элемента ведёт себя как `stretch`).
fn is_stretch(v: AlignValue) -> bool {
    matches!(v, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch)
}

/// Смещение кромки начала border box элемента от кромки начала области вдоль оси (CSS Box
/// Alignment L3 §6): `cell` — размер области, `size` — border box элемента, `m_s`/`m_e` — поля
/// (авто = 0), `auto_s`/`auto_e` — какие из них `auto`. Авто-поля забирают положительное
/// свободное место (оба — пополам) и перекрывают выравнивание; иначе работает `align`.
pub(super) fn axis_offset(
    align: AlignValue,
    cell: f32,
    size: f32,
    m_s: f32,
    m_e: f32,
    auto_s: bool,
    auto_e: bool,
) -> f32 {
    if auto_s || auto_e {
        let free = (cell - size - m_s - m_e).max(0.0);
        return m_s
            + match (auto_s, auto_e) {
                (true, true) => free / 2.0,
                (true, false) => free,
                _ => 0.0,
            };
    }
    match align {
        AlignValue::End => cell - size - m_e,
        AlignValue::Center => (cell - size - m_s - m_e) / 2.0 + m_s,
        _ => m_s,
    }
}

/// CSS Positioned Layout L3 §9.4.3 — смещение `position: relative` (`left`/`right`/`top`/`bottom`),
/// как его применяет `dispatch_box`; проценты — от размера области по inline-оси `basis`.
/// Выравнивание в ячейке переставляет бокс заново и без этого теряло бы смещение.
pub(super) fn relative_shift(item: &LayoutBox, basis: f32, viewport: Size) -> (f32, f32) {
    let s = &item.style;
    if !matches!(s.position, Position::Relative) {
        return (0.0, 0.0);
    }
    let em = s.font_size;
    let pick = |near: &LengthOrAuto, far: &LengthOrAuto| match near {
        LengthOrAuto::Length(l) => l.resolve(em, Some(basis), viewport).unwrap_or(0.0),
        LengthOrAuto::Auto => match far {
            LengthOrAuto::Length(r) => -(r.resolve(em, Some(basis), viewport).unwrap_or(0.0)),
            LengthOrAuto::Auto => 0.0,
        },
    };
    (pick(&s.left, &s.right), pick(&s.top, &s.bottom))
}

/// `self-start` / `self-end` (`own`): начало и конец считаются по собственному режиму письма
/// элемента, а не контейнера (CSS Box Alignment L3 §4.2). `container_start_low` — начало оси
/// контейнера лежит с малой стороны (слева / сверху). Для остальных значений — без изменений.
pub(super) fn resolve_own(
    value: AlignValue,
    own: bool,
    item: &ComputedStyle,
    horizontal_axis: bool,
    container_start_low: bool,
) -> AlignValue {
    if !own || !matches!(value, AlignValue::Start | AlignValue::End) {
        return value;
    }
    if super::flex_trampoline::own_start_is_low(item, horizontal_axis) == container_start_low {
        value
    } else if value == AlignValue::Start {
        AlignValue::End
    } else {
        AlignValue::Start
    }
}

/// `justify-*: left | right` (`side`): физическая сторона строки письма (`left` — верх в
/// `vertical-*`, низ в `sideways-lr`) переводится в `Start`/`End` по inline-оси сетки.
pub(super) fn resolve_side(value: AlignValue, side: Option<ContentSide>, wm: WritingMode, inline_rev: bool) -> AlignValue {
    let Some(side) = side else { return value };
    let left_is_top = wm != WritingMode::SidewaysLr;
    let left_is_start = left_is_top == !inline_rev;
    if (side == ContentSide::Left) == left_is_start { AlignValue::Start } else { AlignValue::End }
}

impl GridInit {
    /// Дорожки родителя, которые наследует элемент-subgrid по своим осям (CSS Grid L2 §9):
    /// `(для grid-template-columns, для grid-template-rows)`; `None` — ось не subgrid или
    /// у родителя нет таких дорожек. Если режим письма элемента ортогонален контейнеру, его
    /// inline-ось (столбцы) совпадает с block-осью родителя (строки), а строки — со
    /// столбцами родителя: источники меняются местами.
    #[inline(never)]
    pub(super) fn subgrid_ctx(
        &self,
        item: &LayoutBox,
        c0: usize,
        c1: usize,
        r0: usize,
        r1: usize,
    ) -> (Option<SubgridContext>, Option<SubgridContext>) {
        let st = &item.style;
        let col_sub = st.grid_template_columns.first() == Some(&GridTrackSize::Subgrid);
        let row_sub = st.grid_template_rows.first() == Some(&GridTrackSize::Subgrid);
        let ortho = (st.writing_mode != WritingMode::HorizontalTb) != self.vertical.is_some();
        let cols = || {
            self.col_widths.get(c0..c1).filter(|t| !t.is_empty()).map(|t| {
                SubgridContext::from_parent_tracks(t, self.col_gap)
                    .with_names(crate::subgrid::line_names_between(&self.col_names, c0, c1))
            })
        };
        let rows = || {
            self.row_heights.get(r0..r1).filter(|t| !t.is_empty()).map(|t| {
                SubgridContext::from_parent_tracks(t, self.row_gap)
                    .with_names(crate::subgrid::line_names_between(&self.row_names, r0, r1))
            })
        };
        let (inline_src, block_src) = if ortho { (rows(), cols()) } else { (cols(), rows()) };
        (if col_sub { inline_src } else { None }, if row_sub { block_src } else { None })
    }

    /// Начало замера элемента, начинающегося в столбце `c0`: физические `(x, y)`.
    pub(super) fn probe_origin(&self, c0: usize) -> (f32, f32) {
        let off = self.col_offsets.get(c0).copied().unwrap_or(0.0);
        match self.vertical {
            None => (self.content_x + off, 0.0),
            Some(_) => (self.content_x, self.content_y + off),
        }
    }

    /// Протяжённость content box по block-оси: определённая либо итоговая — сумма строк в
    /// границах `min-/max-width` (как её запишет `finish_container`). От неё зависит зеркало
    /// `vertical-rl`: первая строка прижата к правому краю итоговой ширины.
    fn block_extent(&self) -> f32 {
        if let Some(definite) = self.definite_content_height {
            return definite;
        }
        match self.vertical {
            None => self.y_off,
            Some(g) => {
                let content = if self.size_contained { 0.0 } else { self.y_off };
                ((g.frame_horiz + content).min(g.max_block).max(g.min_block) - g.frame_horiz).max(0.0)
            }
        }
    }

    /// Физический левый верхний угол области, начинающейся в столбце `c0` и строке `r0`
    /// и занимающей по block-оси `row_span`, по inline-оси `col_span` (нужны для зеркал
    /// `vertical-rl` и `direction: rtl`).
    pub(super) fn cell_origin(&self, c0: usize, r0: usize, row_span: f32, col_span: f32) -> (f32, f32) {
        let col_off = self.col_offsets.get(c0).copied().unwrap_or(0.0);
        let row_off = self.row_offsets.get(r0).copied().unwrap_or(0.0);
        match self.vertical {
            None => (self.content_x + col_off, self.content_y + row_off),
            Some(g) => {
                let x = if g.rl {
                    self.content_x + self.block_extent() - row_off - row_span
                } else {
                    self.content_x + row_off
                };
                let y = if g.inline_rev {
                    self.content_y + self.content_width - col_off - col_span
                } else {
                    self.content_y + col_off
                };
                (x, y)
            }
        }
    }

    /// inline-размер, который предлагается элементу в области размером `cell_in` по inline-оси
    /// (ширина в горизонтальной сетке, физическая высота — в вертикальной): вся область, а при
    /// `justify-self`, отличном от `stretch`, и автоматическом inline-размере элемента — по
    /// содержимому (`fit-content`, CSS Grid L1 §6.1). Ортогональный элемент получает область
    /// целиком: его inline-размер — другая физическая ось.
    fn inline_offer(
        &self,
        item: &LayoutBox,
        cell_in: f32,
        viewport: Size,
        measurer: Option<&dyn TextMeasurer>,
    ) -> f32 {
        let is = &item.style;
        let justify = if matches!(is.justify_self, AlignValue::Auto) { self.s.justify_items } else { is.justify_self };
        let vertical = self.vertical.is_some();
        let item_vertical = is.writing_mode != WritingMode::HorizontalTb;
        let explicit = if vertical { is.height.is_some() } else { is.width.is_some() };
        // Авто-поле по inline-оси отменяет растяжение: размер по содержимому, место отдано полю.
        let auto_margin = if vertical {
            is.margin_top.is_auto() || is.margin_bottom.is_auto()
        } else {
            is.margin_left.is_auto() || is.margin_right.is_auto()
        };
        if (is_stretch(justify) && !auto_margin) || explicit || item_vertical != vertical {
            return cell_in;
        }
        let em = is.font_size;
        let (m, pref) = if vertical {
            (
                is.margin_top.resolve_or_zero(em, self.content_width, viewport)
                    + is.margin_bottom.resolve_or_zero(em, self.content_width, viewport),
                max_content_outer_height(item, measurer, viewport),
            )
        } else {
            (
                is.margin_left.resolve_or_zero(em, self.content_width, viewport)
                    + is.margin_right.resolve_or_zero(em, self.content_width, viewport),
                max_content_outer_width(item, measurer, viewport),
            )
        };
        (pref + m).min(cell_in)
    }

    /// Аргументы `dispatch_box` для замера элемента (CSS Grid L1 §12.3), начинающегося в
    /// столбце `c0`: `(start_x, start_y, available_width, available_height)`.
    pub(super) fn probe_args(
        &self,
        c0: usize,
        cell_in: f32,
        item: &LayoutBox,
        viewport: Size,
        measurer: Option<&dyn TextMeasurer>,
    ) -> (f32, f32, f32, Option<f32>) {
        let (x, y) = self.probe_origin(c0);
        let offer = self.inline_offer(item, cell_in, viewport, measurer);
        match self.vertical {
            None => (x, y, offer, None),
            Some(g) => {
                // Элемент ортогонального (горизонтального) режима: физическая ширина — его
                // inline-размер, и размер строки неизвестен, пока строки не разрешены, поэтому
                // замер идёт по содержимому, а не по всей ширине контейнера.
                let mut avail_w = g.block_avail;
                if item.style.writing_mode == WritingMode::HorizontalTb {
                    let em = item.style.font_size;
                    let m = item.style.margin_left.resolve_or_zero(em, self.content_width, viewport)
                        + item.style.margin_right.resolve_or_zero(em, self.content_width, viewport);
                    avail_w = avail_w.min(max_content_outer_width(item, measurer, viewport) + m);
                }
                (x, y, avail_w, Some(offer))
            }
        }
    }

    /// То же для окончательной раскладки в области с началом `origin`; `cell_block` —
    /// размер области по block-оси, когда он уже определён (строки разрешены).
    pub(super) fn final_args(
        &self,
        item: &LayoutBox,
        origin: (f32, f32),
        cell_in: f32,
        cell_block: Option<f32>,
        viewport: Size,
        measurer: Option<&dyn TextMeasurer>,
    ) -> (f32, f32, f32, Option<f32>) {
        let offer = self.inline_offer(item, cell_in, viewport, measurer);
        match self.vertical {
            None => (origin.0, origin.1, offer, cell_block),
            Some(g) => (origin.0, origin.1, cell_block.unwrap_or(g.block_avail), Some(offer)),
        }
    }

    /// Аргументы для элемента без размещения: горизонтальная сетка кладёт его стопкой под
    /// сеткой, вертикальная — в начало content box (такие элементы не возникают: авто-размещение
    /// даёт место каждому элементу).
    pub(super) fn unplaced_args(&self) -> (f32, f32, f32, Option<f32>) {
        match self.vertical {
            None => (self.content_x, self.content_y + self.y_off, self.content_width, None),
            Some(g) => (self.content_x, self.content_y, g.block_avail, Some(self.content_width)),
        }
    }

    /// Вклад элемента в размер строки (block-ось): высота border box в горизонтальном
    /// режиме, ширина margin box — в вертикальном.
    pub(super) fn item_block_size(&self, item: &LayoutBox, viewport: Size) -> f32 {
        if self.vertical.is_none() {
            return item.rect.height;
        }
        let em = item.style.font_size;
        item.rect.width
            + item.style.margin_left.resolve_or_zero(em, self.content_width, viewport)
            + item.style.margin_right.resolve_or_zero(em, self.content_width, viewport)
    }
}

/// Пролог grid-контейнера с вертикальным `writing-mode`: размеры по inline-оси (физическая
/// высота) и block-оси (ширина), контекст для `build_grid_init`. Дальше проходы замера и
/// размещения ведёт `grid_trampoline::run`.
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch(
    b: &mut LayoutBox,
    s: &Arc<ComputedStyle>,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    cv_auto_skipped: bool,
) -> DispatchOutcome {
    let em = s.font_size;
    // Проценты отступов считаются от inline-размера содержащего блока — его высоты.
    let cb_pct = available_height.unwrap_or(viewport.height).max(0.0);
    let margin_left = s.margin_left.resolve_or_zero(em, cb_pct, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb_pct, viewport);
    let margin_bottom = s.margin_bottom.resolve_or_zero(em, cb_pct, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb_pct, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb_pct, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb_pct, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb_pct, viewport);
    let frame_horiz = padding_left + padding_right + s.border_left_width + s.border_right_width;
    let frame_vert = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;

    let size_contained = s.contain.0 & crate::style::ContainFlags::SIZE.0 != 0
        || s.content_visibility == crate::style::ContentVisibility::Hidden
        || cv_auto_skipped;

    // inline-размер (физическая высота): явная `height`; иначе блочный grid заполняет
    // доступное место за вычетом своих отступов, а inline-grid обтягивает дорожки
    // столбцов (CSS Grid L1 §11.5, как ширину горизонтального inline-grid).
    let inline_avail = available_height.unwrap_or(viewport.height).max(0.0);
    let explicit_inline = crate::vertical::resolve_axis_size(
        s.height.as_ref(), em, Some(inline_avail), viewport, s.box_sizing, frame_vert,
    );
    // Предпочтительный inline-размер по столбцам (с рамкой): ленивый — нужен не всегда.
    let preferred = |b: &LayoutBox| -> f32 {
        if size_contained {
            let ci = s
                .contain_intrinsic_height
                .as_ref()
                .and_then(|l| l.resolve(em, None, viewport))
                .map_or(0.0, |v| v.max(0.0));
            return ci + frame_vert;
        }
        super::intrinsic::grid_col_intrinsic_sum(
            b, viewport, true, &|c| max_content_outer_height(c, measurer, viewport),
        )
        .map(|sum| sum + frame_vert)
        .unwrap_or_else(|| max_content_outer_height(b, measurer, viewport))
    };
    let inline_size = explicit_inline.unwrap_or_else(|| {
        let fill = (inline_avail - margin_top - margin_bottom).max(0.0);
        match s.height.as_ref() {
            Some(Length::MaxContent | Length::MinContent) => preferred(b),
            Some(Length::FitContent(None)) => preferred(b).min(fill),
            Some(Length::FitContent(Some(arg))) => {
                let arg_bb = arg.resolve(em, Some(inline_avail), viewport).map_or(fill, |v| match s.box_sizing {
                    BoxSizing::ContentBox => v + frame_vert,
                    BoxSizing::BorderBox => v,
                });
                preferred(b).min(arg_bb).min(fill)
            }
            _ if s.display == Display::InlineGrid => preferred(b).min(fill),
            // Ортогональный поток (родитель не передал inline-размер): `height: auto` обтягивает
            // содержимое в пределах начального блока (CSS Writing Modes L3 §7.3.1).
            _ if available_height.is_none() => preferred(b).min(fill),
            _ => fill,
        }
    });
    let inline_limit = |len: Option<&Length>| -> Option<f32> {
        let raw = len.filter(|l| !l.is_intrinsic())?.resolve(em, Some(inline_avail), viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => raw + frame_vert,
            BoxSizing::BorderBox => raw.max(frame_vert),
        })
    };
    let inline_size = inline_size
        .min(inline_limit(s.max_height.as_ref()).unwrap_or(f32::INFINITY))
        .max(inline_limit(s.min_height.as_ref()).unwrap_or(0.0));
    b.rect.height = inline_size.max(frame_vert);

    // block-размер (физическая ширина): явная `width` и её границы.
    let block_limit = |len: Option<&Length>| -> Option<f32> {
        let raw = len.filter(|l| !l.is_intrinsic())?.resolve(em, Some(available_width.max(0.0)), viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => raw + frame_horiz,
            BoxSizing::BorderBox => raw.max(frame_horiz),
        })
    };
    let max_block = block_limit(s.max_width.as_ref()).unwrap_or(f32::INFINITY);
    let min_block = block_limit(s.min_width.as_ref()).unwrap_or(0.0);
    let explicit_width = crate::vertical::resolve_axis_size(
        s.width.as_ref(), em, Some(available_width.max(0.0)), viewport, s.box_sizing, frame_horiz,
    )
    .map(|w| w.min(max_block).max(min_block));

    let content_inline =
        (b.rect.height - frame_vert - scrollbar_gutter_block(s)).max(0.0);
    let content_x = b.rect.x + s.border_left_width + padding_left;
    let content_y = b.rect.y + s.border_top_width + padding_top + scrollbar_gutter_block_start(s);
    let definite_block = explicit_width.map(|w| (w - frame_horiz).max(0.0));
    let block_avail = match explicit_width {
        Some(w) => (w - frame_horiz).max(0.0),
        None => (available_width - margin_left - frame_horiz).max(0.0),
    };

    let is_positioned = super::multicol_abspos::establishes_abs_cb(s);
    let children_pcb = if is_positioned {
        Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            explicit_width.map_or(0.0, |w| (w - s.border_left_width - s.border_right_width).max(0.0)),
            0.0,
        )
    } else {
        pcb
    };
    let geom = VGridGeom {
        rl: matches!(s.writing_mode, WritingMode::VerticalRl | WritingMode::SidewaysRl),
        inline_rev: (s.direction == crate::style::Direction::Rtl) != (s.writing_mode == WritingMode::SidewaysLr),
        block_avail,
        frame_horiz,
        explicit_width,
        min_block,
        max_block,
    };

    // Дорожки для щелей `column-rule`/`row-rule` paint читает как физические спаны x/y — у
    // вертикальной сетки они логические, поэтому щели в ней пока не рисуются (GRID-VWM-2).
    b.subgrid_tracks = None;
    match super::grid::build_grid_init(
        &b.children, s, content_x, content_y, content_inline, definite_block, viewport, children_pcb,
        em, available_height, padding_top, padding_bottom, size_contained, is_positioned, pcb,
        measurer, Some(geom),
    ) {
        Some(init) => DispatchOutcome::NeedsGridLoop(init),
        None => {
            // Нет элементов: ширина — явная или только рамка.
            b.rect.width = explicit_width.unwrap_or(frame_horiz).min(max_block).max(min_block);
            super::grid_trampoline::lay_out_abs(
                b, s, is_positioned, pcb, content_x, content_y, measurer, viewport, hp,
            );
            DispatchOutcome::Done
        }
    }
}

/// Ширина контейнера по итогам раскладки строк: явная `width` либо рамка плюс сумма строк.
pub(super) fn finish_container(b: &mut LayoutBox, init: &GridInit, geom: VGridGeom) {
    let content_block = if init.size_contained { 0.0 } else { init.y_off };
    b.rect.width = geom
        .explicit_width
        .unwrap_or(geom.frame_horiz + content_block)
        .min(geom.max_block)
        .max(geom.min_block);
}

/// Подъём элемента вертикальной сетки для baseline-выравнивания: расстояние от кромки начала
/// block-оси строки (справа у `vertical-rl`, слева у `vertical-lr`) до базовой линии, считая
/// поле, и внешний размер margin box по block-оси. Базовая линия вертикального бокса
/// отсчитывается от левой кромки его border box.
pub(super) fn block_ascent(
    init: &GridInit,
    geom: VGridGeom,
    item: &LayoutBox,
    side: BaselineSide,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
) -> (f32, f32) {
    let em = item.style.font_size;
    let m_l = item.style.margin_left.resolve_or_zero(em, init.content_width, viewport);
    let m_r = item.style.margin_right.resolve_or_zero(em, init.content_width, viewport);
    let bx = box_baseline_in_axis(item, &init.s, true, side, measurer);
    let ascent = if geom.rl { m_r + (item.rect.width - bx) } else { m_l + bx };
    (ascent, item.rect.width + m_l + m_r)
}

/// CSS Grid L1 §6.1 для вертикальной сетки: первая и последняя базовая линия контейнера как
/// расстояние по `x` от левой кромки его border box (так их читает
/// `baseline::vertical_content_baseline`). Первая — общая линия группы `first baseline` первой
/// строки либо линия первого item'а, чья область задевает её; последняя — симметрично.
pub(super) fn container_baselines(
    b: &LayoutBox,
    init: &GridInit,
    geom: VGridGeom,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<(f32, f32)> {
    let n_rows = init.n_rows as usize;
    if n_rows == 0 || init.row_offsets.len() < n_rows {
        return None;
    }
    let mut placed: Vec<usize> = (0..init.item_idxs.len())
        .filter(|&k| init.placements[k].0 != 0 && init.placements[k].2 != 0)
        .collect();
    placed.sort_by_key(|&k| (b.children[init.item_idxs[k]].style.order, init.placements[k].0));
    // Левая и правая кромки строки `r` в физических координатах.
    let row_edges = |r: usize| {
        let (left, _) = init.cell_origin(0, r, init.row_heights[r], 0.0);
        (left, left + init.row_heights[r])
    };
    let item_baseline = |k: usize, side: BaselineSide| {
        let c = &b.children[init.item_idxs[k]];
        c.rect.x + box_baseline_in_axis(c, &init.s, true, side, measurer)
    };
    let starts_in_row = |k: usize, r: usize| init.placements[k].2 as usize == r + 1;
    let ends_in_row = |k: usize, r: usize| {
        let end = init.placements[k].3 as usize;
        end.saturating_sub(1).min(n_rows).saturating_sub(1).max(init.placements[k].2 as usize - 1) == r
    };
    let group_baseline = |r: usize, want: BaselineSide| -> Option<f32> {
        let in_group = |k: usize, side: BaselineSide| match side {
            BaselineSide::First => starts_in_row(k, r),
            BaselineSide::Last => ends_in_row(k, r),
        } && init.item_baselines[k].is_some_and(|ib| ib.side == side);
        let other = if want == BaselineSide::First { BaselineSide::Last } else { BaselineSide::First };
        [want, other].into_iter().find(|&side| placed.iter().any(|&k| in_group(k, side))).map(|side| {
            let (left, right) = row_edges(r);
            match (side, geom.rl) {
                (BaselineSide::First, true) => right - init.row_first_group[r].0,
                (BaselineSide::First, false) => left + init.row_first_group[r].0,
                (BaselineSide::Last, true) => left + init.row_last_group[r].1,
                (BaselineSide::Last, false) => right - init.row_last_group[r].1,
            }
        })
    };
    let first = group_baseline(0, BaselineSide::First).or_else(|| {
        placed.iter().copied().find(|&k| starts_in_row(k, 0)).map(|k| item_baseline(k, BaselineSide::First))
    });
    let last_row = n_rows - 1;
    let last = group_baseline(last_row, BaselineSide::Last).or_else(|| {
        placed.iter().rev().copied().find(|&k| ends_in_row(k, last_row)).map(|k| item_baseline(k, BaselineSide::Last))
    });
    match (first, last) {
        (None, None) => None,
        (f, l) => {
            let f = f.or(l)?;
            Some((f - b.rect.x, l.unwrap_or(f) - b.rect.x))
        }
    }
}

/// Окончательная позиция элемента `k` в его области вертикальной сетки (CSS Grid L1 §11.2,
/// CSS Box Alignment L3): `align-*` по block-оси (физический `x`), `justify-*` по inline-оси
/// (физический `y`). Элемент и его поддерево переезжают на место целиком.
pub(super) fn place_item(
    init: &GridInit,
    k: usize,
    item: &mut LayoutBox,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    hp: &dyn HyphenationProvider,
) {
    let Some(geom) = init.vertical else { return };
    let (cs, ce, rs, re) = init.placements[k];
    let n_cols = init.n_cols;
    let n_rows = init.n_rows;
    let c0 = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let c1 = (ce - 1).min(n_cols) as usize;
    let r0 = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
    let r1 = (re - 1).min(n_rows) as usize;
    let cell_in = grid_track_span(&init.col_offsets, &init.col_widths, c0, c1);
    let cell_bl = grid_track_span(&init.row_offsets, &init.row_heights, r0, r1);
    let (cell_x, cell_y) = init.cell_origin(c0, r0, cell_bl, cell_in);

    let s = &init.s;
    let iem = item.style.font_size;
    let basis = init.content_width;
    let m_l = item.style.margin_left.resolve_or_zero(iem, basis, viewport);
    let m_r = item.style.margin_right.resolve_or_zero(iem, basis, viewport);
    let m_t = item.style.margin_top.resolve_or_zero(iem, basis, viewport);
    let m_b = item.style.margin_bottom.resolve_or_zero(iem, basis, viewport);
    // Поля вдоль block-оси: старт — правый край для `rl`, левый для `lr`.
    let (m_bs, m_be) = if geom.rl { (m_r, m_l) } else { (m_l, m_r) };

    // align-* — block-ось (физический `x`).
    let is = &item.style;
    let align_auto = matches!(is.align_self, AlignValue::Auto);
    let mut align = resolve_own(
        if align_auto { s.align_items } else { is.align_self },
        if align_auto { s.content_align_extra.items_own } else { is.content_align_extra.self_own },
        is,
        true,
        !geom.rl,
    );
    let align_safe = if align_auto { s.content_align_extra.items_safe } else { is.content_align_extra.self_safe };
    let outer_bl = item.rect.width + m_bs + m_be;
    let (auto_bs, auto_be) = if geom.rl {
        (is.margin_right.is_auto(), is.margin_left.is_auto())
    } else {
        (is.margin_left.is_auto(), is.margin_right.is_auto())
    };
    if align_safe && outer_bl > cell_bl && matches!(align, AlignValue::End | AlignValue::Center) {
        align = AlignValue::Start;
    }
    let mut stretch_to: Option<f32> = None;
    let mut off_bl = match align {
        _ if auto_bs || auto_be => axis_offset(align, cell_bl, item.rect.width, m_bs, m_be, auto_bs, auto_be),
        AlignValue::End => cell_bl - item.rect.width - m_be,
        AlignValue::Center => (cell_bl - outer_bl) / 2.0 + m_bs,
        AlignValue::Baseline | AlignValue::LastBaseline if init.item_baselines[k].is_some() => {
            // CSS Grid L1 §6.2: общая базовая линия группы строки — на подъёме группы от
            // кромки начала block-оси (`first`) либо на спуске от кромки конца (`last`).
            let ib = init.item_baselines[k].unwrap_or(ItemBaseline { side: BaselineSide::First, ascent: m_bs });
            let bl = ib.ascent - m_bs;
            match ib.side {
                BaselineSide::First => init.row_first_group[r0].0 - bl,
                BaselineSide::Last => {
                    let last_row = r1.saturating_sub(1).max(r0);
                    cell_bl - init.row_last_group[last_row].1 - bl
                }
            }
        }
        AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
            // `stretch` растягивает только автоматический block-размер (Grid §11.2).
            let target = cell_bl - m_bs - m_be;
            if is.width.is_none() && item.rect.width < target {
                stretch_to = Some(target);
            }
            m_bs
        }
        _ => m_bs,
    };

    // justify-* — inline-ось (физический `y`).
    let justify_auto = matches!(is.justify_self, AlignValue::Auto);
    let justify = if justify_auto { s.justify_items } else { is.justify_self };
    let justify = resolve_own(
        justify,
        if justify_auto { s.content_align_extra.justify_items_own } else { is.content_align_extra.justify_self_own },
        is,
        false,
        !geom.inline_rev,
    );
    let mut justify = resolve_side(
        justify,
        if justify_auto { s.content_align_extra.justify_items_side } else { is.content_align_extra.justify_self_side },
        s.writing_mode,
        geom.inline_rev,
    );
    let justify_safe =
        if justify_auto { s.content_align_extra.justify_items_safe } else { is.content_align_extra.justify_self_safe };
    let outer_in = item.rect.height + m_t + m_b;
    if justify_safe && outer_in > cell_in && matches!(justify, AlignValue::End | AlignValue::Center) {
        justify = AlignValue::Start;
    }
    // Поля вдоль inline-оси: старт — верхний край, а при `inline_rev` — нижний.
    let (m_is, m_ie) = if geom.inline_rev { (m_b, m_t) } else { (m_t, m_b) };
    let (auto_is, auto_ie) = if geom.inline_rev {
        (is.margin_bottom.is_auto(), is.margin_top.is_auto())
    } else {
        (is.margin_top.is_auto(), is.margin_bottom.is_auto())
    };
    let off_in = axis_offset(justify, cell_in, item.rect.height, m_is, m_ie, auto_is, auto_ie);

    // Растянутый по block-оси элемент раскладывается заново с итоговой шириной: потомки
    // (вложенные flex/grid, `%`) должны увидеть её, а не размер по содержимому.
    if let Some(w) = stretch_to {
        let child_col_subgrid = item.style.grid_template_columns.first() == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = item.style.grid_template_rows.first() == Some(&GridTrackSize::Subgrid);
        let _guard = (child_col_subgrid || child_row_subgrid).then(|| {
            let (col_ctx, row_ctx) = init.subgrid_ctx(item, c0, c1, r0, r1);
            SubgridContextGuard::set(col_ctx, row_ctx)
        });
        let (rx, ry) = (item.rect.x - m_l, item.rect.y - m_t);
        lay_out_with_used_size(
            item, rx, ry, cell_bl, Some(item.rect.height + m_t + m_b), measurer, viewport,
            init.children_pcb, hp, false,
            UsedSizeOverride { width: Some(w), box_sizing: Some(BoxSizing::BorderBox), ..Default::default() },
        );
        off_bl = m_bs;
    }

    let x = if geom.rl {
        cell_x + cell_bl - off_bl - item.rect.width
    } else {
        cell_x + off_bl
    };
    let y = if geom.inline_rev { cell_y + cell_in - off_in - item.rect.height } else { cell_y + off_in };
    let (rel_x, rel_y) = relative_shift(item, cell_bl, viewport);
    crate::incremental::translate_subtree(item, x + rel_x - item.rect.x, y + rel_y - item.rect.y);
}
