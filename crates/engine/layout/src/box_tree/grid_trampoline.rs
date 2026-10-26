use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};
use super::grid::{grid_content_distribution, grid_track, grid_track_span};
use crate::subgrid::{SubgridContext, SubgridContextGuard};

/// Loop-entry state for `grid::build_grid_init`'s per-item probe (CSS Grid L1
/// §12.3, row-height measurement) and final-placement (§11.2, cell alignment)
/// passes — everything the removed inline double loop (pre-LAYOUT-2-срез-4
/// `grid.rs`) read or wrote across items, plus what `layout_dispatch.rs`'s
/// grid dispatch arm used to do with `lay_out_grid`'s return value once it
/// came back (container height). Captured by `grid::build_grid_init` before
/// any item is probed — Steps 1–3 (placement resolution, column-track sizing)
/// still run natively in `build_grid_init`; see its doc comment for why (they
/// never call `lay_out` on a child, so there is nothing to suspend on).
pub(super) struct GridInit {
    pub(super) item_idxs: Vec<usize>,
    /// `(col_start, col_end, row_start, row_end)`, 1-based, indexed like `item_idxs`.
    pub(super) placements: Vec<(u32, u32, u32, u32)>,
    pub(super) n_cols: u32,
    pub(super) n_rows: u32,
    /// Число дорожек явной сетки по осям (линий — на одну больше): `GridAxis::n_tracks`,
    /// нужно для разбора линий абсолютно позиционированных детей (`grid_abspos`).
    pub(super) col_explicit: u32,
    pub(super) row_explicit: u32,
    pub(super) col_widths: Vec<f32>,
    pub(super) col_offsets: Vec<f32>,
    /// Names of the container's lines per axis (own, with `repeat()` expanded and, for a subgrid,
    /// merged with the parent's) — a subgrid item inherits the slice it spans.
    pub(super) col_names: Vec<Vec<String>>,
    pub(super) row_names: Vec<Vec<String>>,
    /// Masonry-stripped row template, owned — `grid::grid_track` reads it in
    /// both `post_probe_item` (auto-row growth) and `finish_probe_pass` (fr
    /// resolution/align-content), both of which outlive `build_grid_init`'s
    /// own borrow of `s.grid_template_rows`.
    pub(super) eff_row_template: Vec<GridTrackSize>,
    /// `grid-auto-rows` with `%` sizes read as `auto` when the container height is indefinite.
    pub(super) auto_row: Vec<GridTrackSize>,
    /// Row tracks of a `repeat(auto-fit, …)` that hold no item (CSS Grid L1 §7.2.3.2): sized
    /// `0` and without gutters. Same length as `eff_row_template`.
    pub(super) row_collapsed: Vec<bool>,
    pub(super) inherited_rows: Option<SubgridContext>,
    /// Row track sizes — the base sizes `build_grid_init` seeded, grown by
    /// each item's probed height in `post_probe_item`, then resolved to final
    /// (fr distribution + align-content stretch) by `finish_probe_pass`.
    pub(super) row_heights: Vec<f32>,
    /// Filled by `finish_probe_pass` once every item's probe height is in —
    /// empty/meaningless before the Probe phase completes.
    pub(super) row_offsets: Vec<f32>,
    /// Running block-axis extent: the total row-track span after
    /// `finish_probe_pass`, then bumped further by each unplaced item's
    /// height during the Final phase — the function's overall return value.
    pub(super) y_off: f32,
    pub(super) content_x: f32,
    pub(super) content_y: f32,
    pub(super) content_width: f32,
    pub(super) definite_content_height: Option<f32>,
    pub(super) col_gap: f32,
    pub(super) row_gap: f32,
    pub(super) s: Arc<ComputedStyle>,
    pub(super) children_pcb: Rect,
    // Phase-epilogue inputs (`finish_frame` only) — ride along unchanged from
    // `build_grid_init`, same as `FlexInit`'s equivalent fields.
    pub(super) em: f32,
    pub(super) available_height: Option<f32>,
    pub(super) padding_top: f32,
    pub(super) padding_bottom: f32,
    pub(super) size_contained: bool,
    /// Own `position != static` and the CB inherited from the parent — pick the
    /// containing block of absolutely-positioned children (see `grid_abspos::lay_out_abs`).
    pub(super) is_positioned: bool,
    pub(super) own_pcb: Rect,
    /// BUG-341 S33 probe-reuse cache — see `build_grid_init`'s doc comment.
    /// `(probe_x, probe_y, laid-out subtree)`, taken (and consumed) by the
    /// Final phase's `step_final_item`.
    pub(super) probe_reuse: Vec<Option<(f32, f32, LayoutBox)>>,
    /// CSS Grid L1 §6.2 baseline-выравнивание: подъём/спуск item'а, выровненного по
    /// базовой линии (индекс как у `item_idxs`); заполняется в Probe-фазе.
    pub(super) item_baselines: Vec<Option<ItemBaseline>>,
    /// По строкам: `(подъём, спуск)` группы `first baseline` (items с началом в
    /// строке) — общая базовая линия отстоит от верха строки на подъём.
    pub(super) row_first_group: Vec<(f32, f32)>,
    /// То же для группы `last baseline` (items с концом в строке) — базовая линия
    /// отстоит от низа строки на спуск.
    pub(super) row_last_group: Vec<(f32, f32)>,
    /// Baseline-выравнивание по оси столбцов (`justify-self: [first|last] baseline`, CSS Grid L1
    /// §6.2): линия вертикальна и измеряется по `x`. Участвуют только items ортогонального
    /// (вертикального) режима в горизонтальной сетке — у параллельных линии вдоль inline-оси
    /// нет, они прижимаются к краю (Align L3 §9.1). Индекс как у `item_idxs`.
    pub(super) item_col_baselines: Vec<Option<ItemBaseline>>,
    /// CSS Grid L1 §11.7.1: вклад items, чей диапазон строк содержит гибкую дорожку (`Nfr` или
    /// `minmax(_, Nfr)`), при неопределённой высоте контейнера — `(первая строка, за последней,
    /// внешняя высота item'а)`. Из них `finish_probe_pass` ищет гипотетический размер `fr`.
    pub(super) flex_contribs: Vec<(usize, usize, f32)>,
    /// По столбцам: `(подъём, спуск)` группы у левого края (`First`) и у правого (`Last`).
    pub(super) col_first_group: Vec<(f32, f32)>,
    pub(super) col_last_group: Vec<(f32, f32)>,
    /// GRID-VWM: контейнер в вертикальном `writing-mode` — столбцы идут по физической
    /// оси `y`, строки по `x`. Все размеры и смещения дорожек остаются логическими
    /// (inline / block), в физические координаты их переводят `grid_vertical`-хелперы.
    pub(super) vertical: Option<super::grid_vertical::VGridGeom>,
    /// GRID-RTL: горизонтальный контейнер с `direction: rtl` — столбцы считаются от правого
    /// края content box (смещения `col_offsets` остаются логическими, зеркалит `cell_origin`).
    pub(super) rtl: bool,
    /// `display: grid-lanes` с полосами-столбцами: выбор полосы и позиции элементов вдоль потока
    /// (`grid_lanes`). Строка `k` здесь — это элемент `k`, а не дорожка шаблона.
    pub(super) lanes: Option<super::grid_lanes::LanesState>,
    /// `display: grid-lanes` с полосами-строками: вклады элементов в размеры полос (строк).
    pub(super) row_lanes: Option<super::grid_lanes::RowLaneSizing>,
}

/// Базовая линия item'а, участвующего в baseline-группе строки.
#[derive(Clone, Copy)]
pub(super) struct ItemBaseline {
    pub(super) side: BaselineSide,
    /// От верхней кромки margin box до базовой линии.
    pub(super) ascent: f32,
}

/// Which of the two per-item loops a [`Frame`] is currently driving.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// CSS Grid L1 §12.3 — measure each item's intrinsic height at its
    /// resolved column width to grow `auto`/`fr` row tracks.
    Probe,
    /// CSS Grid L1 §11.2 — place each item in its final cell and apply
    /// `align-items`/`justify-items`.
    Final,
}

/// One level of the explicit stack `run` maintains in place of the native
/// call stack — the grid-container analogue of `flex_trampoline::Frame`.
/// `b` is owned (taken via `block_flow_trampoline::take_box`) for the
/// duration of this container's own probe + final passes.
struct Frame {
    b: LayoutBox,
    init: Box<GridInit>,
    pass: Pass,
    /// Index into `init.item_idxs`/`init.placements` for the current pass.
    k: usize,
    /// `CV_AUTO_TOUCHED`'s value just before item `k`'s Probe-phase dispatch
    /// started — read back by `post_probe_item` once the item (synchronously,
    /// or via a `NeedsGridLoop` descend/pop unwind) is fully resolved. Only
    /// meaningful while item `k`'s Probe-phase placement is in flight; see
    /// `CV_AUTO_TOUCHED`'s doc comment for why the save/restore must bracket
    /// the *whole* subtree, descend included, not just a synchronous call.
    probe_outer_cv: bool,
    /// Same bracket for `INDEFINITE_HEIGHT_CONSULTED`: an item whose probe
    /// (indefinite grid-area height) resolved a `%` block size to `auto` must be
    /// laid out again against the final cell height (CSS Grid §11.5 / CSS 2.1 §10.5).
    probe_outer_ih: bool,
}

enum StepOutcome {
    /// This item is fully placed (synchronously, or via the block-flow/flex
    /// trampoline) — move on to the next.
    Advance,
    /// This item is itself a grid container (regular or subgrid) with its own
    /// items to place — push the current frame and continue processing this one.
    Descend(Box<GridInit>),
}

/// Drives `init`'s probe and final-placement passes (and every further grid-
/// container descendant either meets — including subgrid, which resolves its
/// inherited track context synchronously inside `build_grid_init`, before any
/// suspension) on an explicit heap stack, so a chain of nested grid containers
/// no longer grows the native call stack one frame per level (LAYOUT-2's
/// acceptance criterion, applied to item (3) of its ROADMAP entry). `b` is the
/// box `dispatch_box`'s grid arm was originally called on.
pub(super) fn run(
    b: &mut LayoutBox,
    init: Box<GridInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let mut current = Frame {
        b: block_flow_trampoline::take_box(b),
        init,
        pass: Pass::Probe,
        k: 0,
        probe_outer_cv: false,
        probe_outer_ih: false,
    };
    let mut stack: Vec<Frame> = Vec::new();

    loop {
        match current.pass {
            Pass::Probe => {
                if current.k >= current.init.item_idxs.len() {
                    finish_probe_pass(&mut current.init, viewport);
                    current.pass = Pass::Final;
                    current.k = 0;
                    continue;
                }
                match step_probe_item(&mut current, measurer, viewport, hp) {
                    StepOutcome::Advance => current.k += 1,
                    StepOutcome::Descend(child_init) => {
                        let i = current.init.item_idxs[current.k];
                        let child_box = block_flow_trampoline::take_box(&mut current.b.children[i]);
                        let child_frame = Frame {
                            b: child_box,
                            init: child_init,
                            pass: Pass::Probe,
                            k: 0,
                            probe_outer_cv: false,
        probe_outer_ih: false,
                        };
                        stack.push(current);
                        current = child_frame;
                    }
                }
            }
            Pass::Final => {
                if current.k >= current.init.item_idxs.len() {
                    finish_frame(&mut current, measurer, viewport, hp);
                    match stack.pop() {
                        None => {
                            *b = current.b;
                            return;
                        }
                        Some(mut parent) => {
                            let i = parent.init.item_idxs[parent.k];
                            parent.b.children[i] = current.b;
                            match parent.pass {
                                Pass::Probe => post_probe_item(&mut parent, i, measurer, viewport),
                                Pass::Final => post_final_item(&mut parent, i, viewport, measurer, hp),
                            }
                            parent.k += 1;
                            current = parent;
                        }
                    }
                    continue;
                }
                match step_final_item(&mut current, measurer, viewport, hp) {
                    StepOutcome::Advance => current.k += 1,
                    StepOutcome::Descend(child_init) => {
                        let i = current.init.item_idxs[current.k];
                        let child_box = block_flow_trampoline::take_box(&mut current.b.children[i]);
                        let child_frame = Frame {
                            b: child_box,
                            init: child_init,
                            pass: Pass::Probe,
                            k: 0,
                            probe_outer_cv: false,
        probe_outer_ih: false,
                        };
                        stack.push(current);
                        current = child_frame;
                    }
                }
            }
        }
    }
}

/// Handles exactly one item's Probe-phase placement (CSS Grid L1 §12.3),
/// copied from the removed inline loop body (both the subgrid and non-subgrid
/// arms) except at the one recursive call in each, which now either finishes
/// synchronously (`Advance`, doing the same post-item bookkeeping the removed
/// loop did right after the call — see `post_probe_item`) or hands back the
/// item's `GridInit` for `run` to push and descend into.
fn step_probe_item(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> StepOutcome {
    let k = frame.k;
    let i = frame.init.item_idxs[k];
    if let Some(lanes) = frame.init.lanes.as_mut() {
        // Grid L3 §3.1: полоса зависит от высот уже уложенных элементов — выбор прямо перед замером.
        let (first, span) = lanes.choose(k);
        frame.init.placements[k] = (first as u32 + 1, (first + span) as u32 + 1, k as u32 + 1, k as u32 + 2);
    }
    let (cs, ce, rs, re) = frame.init.placements[k];
    let n_cols = frame.init.n_cols;
    let n_rows = frame.init.n_rows;
    let c0 = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let c1 = (ce - 1).min(n_cols) as usize;
    let cell_w = grid_track_span(&frame.init.col_offsets, &frame.init.col_widths, c0, c1);
    let pcb = frame.init.children_pcb;

    let child_col_subgrid = frame.b.children[i].style.grid_template_columns.first()
        == Some(&GridTrackSize::Subgrid);
    let child_row_subgrid = frame.b.children[i].style.grid_template_rows.first()
        == Some(&GridTrackSize::Subgrid);

    if child_col_subgrid || child_row_subgrid {
        let r0 = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
        let r1 = (re.max(rs + 1) - 1).min(n_rows) as usize;
        let (child_col_ctx, child_row_ctx) = frame.init.subgrid_ctx(&frame.b.children[i], c0, c1, r0, r1);
        let _guard = SubgridContextGuard::set(child_col_ctx, child_row_ctx);
        let (ax, ay, aw, ah) = frame.init.probe_args(c0, cell_w, &frame.b.children[i], viewport, measurer);
        let outcome = dispatch_box(
            &mut frame.b.children[i], ax, ay, aw, ah, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        );
        drop(_guard);
        match outcome {
            DispatchOutcome::Done => { post_probe_item(frame, i, measurer, viewport); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            // LAYOUT-2 срез 6: a (subgrid) grid item that is itself a table —
            // same shape as the flex/block-flow arms above.
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 7: a (subgrid) grid item that is itself a
            // multicol container — same shape as the flex/block-flow/table
            // arms above.
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: same shape, for a (subgrid) grid item that is
            // itself a vertical writing-mode container.
            DispatchOutcome::NeedsVerticalLoop(ci) => {
                super::vertical_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
        }
    } else {
        // BUG-341 S32/CV_AUTO_TOUCHED: the outer flag must bracket the whole
        // subtree this dispatch produces, including a further grid descent
        // that suspends and resumes later — so it is saved on the frame here
        // and only consulted once the item is fully resolved, in
        // `post_probe_item`, not inline after this call the way a purely
        // synchronous recursive call could get away with.
        frame.probe_outer_cv = CV_AUTO_TOUCHED.with(|c| c.replace(false));
        frame.probe_outer_ih = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.replace(false));
        let (ax, ay, aw, ah) = frame.init.probe_args(c0, cell_w, &frame.b.children[i], viewport, measurer);
        match dispatch_box(
            &mut frame.b.children[i], ax, ay, aw, ah, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        ) {
            DispatchOutcome::Done => { post_probe_item(frame, i, measurer, viewport); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: same shape, for a (subgrid) grid item that is
            // itself a vertical writing-mode container.
            DispatchOutcome::NeedsVerticalLoop(ci) => {
                super::vertical_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i, measurer, viewport);
                StepOutcome::Advance
            }
        }
    }
}

/// Runs right after item `k` finishes its Probe-phase placement — CV_AUTO
/// bookkeeping + BUG-341 S33 probe-reuse stash (non-subgrid items only, same
/// as the removed code's `if`/`else` split) and the auto/fr/subgrid-exempt
/// row-height growth (both arms, gated only by the container's own row axis
/// not being subgridded). Copied from immediately after the removed loop's
/// recursive call. Shared by `step_probe_item`'s synchronous paths and `run`'s
/// resume-after-descend path — both leave `frame.k` pointing at this exact
/// item, so re-deriving position from `frame.init.placements[k]` gives the
/// same answer either way; nothing needs to be stashed across the suspension
/// beyond `probe_outer_cv`, already on the frame.
fn post_probe_item(frame: &mut Frame, i: usize, measurer: Option<&dyn TextMeasurer>, viewport: Size) {
    let k = frame.k;
    let child_col_subgrid = frame.b.children[i].style.grid_template_columns.first()
        == Some(&GridTrackSize::Subgrid);
    let child_row_subgrid = frame.b.children[i].style.grid_template_rows.first()
        == Some(&GridTrackSize::Subgrid);
    if !(child_col_subgrid || child_row_subgrid) {
        let touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
        let outer = frame.probe_outer_cv;
        CV_AUTO_TOUCHED.with(|c| c.set(outer || touched_here));
        let ih_here = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.get());
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(frame.probe_outer_ih || ih_here));
        // Вертикальная сетка пробу не переиспользует: ортогональный элемент в финале получает
        // ширину области, а не доступную ширину контейнера, как в замере.
        if !touched_here && !ih_here && frame.init.vertical.is_none() {
            let n_cols = frame.init.n_cols;
            let c0 = (frame.init.placements[k].0 - 1).min(n_cols.saturating_sub(1)) as usize;
            let (probe_x, probe_y) = frame.init.probe_origin(c0);
            frame.init.probe_reuse[k] = Some((probe_x, probe_y, frame.b.children[i].clone()));
        }
    }

    // BUG-1397: under size containment the items add nothing to the row tracks.
    if frame.init.size_contained {
        return;
    }
    grow_rows_for_item(frame, k, i, viewport);
    if frame.init.lanes.is_some() {
        let h = frame.init.item_block_size(k, &frame.b.children[i], viewport);
        frame.init.row_heights[k] = h;
        let wants = super::grid_lanes::wants_flow_alignment(&frame.b.children[i].style, &frame.init.s, false);
        if let Some(lanes) = frame.init.lanes.as_mut() {
            lanes.wants[k] = wants;
            let (first, span) = lanes.settle(k, h);
            // Плотная упаковка выбирает окно только теперь, когда известна высота элемента.
            let want = (first as u32 + 1, (first + span) as u32 + 1, k as u32 + 1, k as u32 + 2);
            if frame.init.placements[k] != want {
                frame.init.placements[k] = want;
                frame.init.probe_reuse[k] = None;
            }
        }
    }
    record_flex_contrib(frame, i, viewport);
    record_item_baseline(frame, i, measurer, viewport);
    record_item_col_baseline(frame, i, measurer, viewport);
}

/// Item `i` (placement `k`) grows the content-sized row it starts in to its outer block size. A
/// row `subgrid` is not one item: each of its own items asks for the parent's row it sits in
/// (Grid L2 §9, `grid_row_subgrid`), otherwise the whole subgrid height lands on its first row.
#[inline(never)]
fn grow_rows_for_item(frame: &mut Frame, k: usize, i: usize, viewport: Size) {
    if frame.init.inherited_rows.is_some() {
        return;
    }
    let (_, _, rs, re) = frame.init.placements[k];
    let r0 = (rs - 1) as usize;
    let grow = |frame: &mut Frame, row: usize, h: f32| {
        if row < frame.init.row_heights.len()
            && row_grows_with_content(grid_track(row as u32, &frame.init.eff_row_template, &frame.init.auto_row))
            && h > frame.init.row_heights[row]
        {
            frame.init.row_heights[row] = h;
        }
    };
    if super::grid_row_subgrid::is_row_subgrid(&frame.b.children[i]) {
        let mut contribs = Vec::new();
        let n_rows = frame.init.n_rows;
        let end = (re.max(rs + 1) - 1).min(n_rows) as usize;
        let ok = super::grid_row_subgrid::collect_row_contributions(
            &frame.b.children[i],
            r0,
            frame.init.row_gap,
            (0.0, 0.0),
            viewport,
            frame.init.definite_content_height,
            &mut contribs,
        );
        if ok.is_some() {
            for c in contribs {
                if c.row < end.max(r0 + 1) {
                    grow(frame, c.row, c.size);
                }
            }
            return;
        }
    }
    let item_h = frame.init.item_block_size(k, &frame.b.children[i], viewport);
    if frame.init.row_lanes.is_some() {
        let n_rows = frame.init.n_rows as usize;
        if let Some(rl) = frame.init.row_lanes.as_mut() {
            rl.record(k, n_rows, item_h);
        }
        return;
    }
    grow(frame, r0, item_h);
}

/// CSS Grid L1 §11.7.1: item, пересекающий гибкую строку, при неопределённой высоте контейнера
/// вносит свою внешнюю высоту в поиск размера `fr` (`minmax(0, Nfr)` содержимым не растёт —
/// вклад единственный способ повлиять на неё).
fn record_flex_contrib(frame: &mut Frame, i: usize, viewport: Size) {
    let k = frame.k;
    let (_, _, rs, re) = frame.init.placements[k];
    let init = &frame.init;
    if rs == 0 || init.inherited_rows.is_some() || init.definite_content_height.is_some() {
        return;
    }
    let n_rows = init.n_rows as usize;
    let r0 = ((rs - 1) as usize).min(n_rows.saturating_sub(1));
    let r1 = ((re - 1) as usize).min(n_rows).max(r0 + 1);
    let flexible = (r0..r1.min(n_rows))
        .any(|r| grid_track(r as u32, &init.eff_row_template, &init.auto_row).flex_factor().is_some());
    if !flexible {
        return;
    }
    let h = frame.init.item_block_size(k, &frame.b.children[i], viewport);
    frame.init.flex_contribs.push((r0, r1, h));
}

/// CSS Grid L1 §6.2, ось столбцов: item с `justify-self: [first|last] baseline` входит в
/// baseline-группу своего столбца. Линия «поперечна» оси столбцов: в горизонтальной сетке она
/// вертикальна и читается по `x` (есть только у items вертикального режима — параллельные
/// прижимаются к краю, Align L3 §9.1), в вертикальной — горизонтальна и читается по `y`
/// (ортогональные горизонтальные items; у вертикальных линия синтезируется по середине). Группа —
/// по краю, к которому тянется линия item'а. Ширину `auto`-столбцов группа не растит (столбцы
/// размеряются раньше, чем известны items).
fn record_item_col_baseline(frame: &mut Frame, i: usize, measurer: Option<&dyn TextMeasurer>, viewport: Size) {
    use super::baseline::{baseline_phys_side_in_axis, box_baseline_in_axis, PhysSide};
    let k = frame.k;
    let (cs, ce, rs, _) = frame.init.placements[k];
    let item = &frame.b.children[i];
    if cs == 0
        || rs == 0
        || frame.init.s.grid_template_columns.first() == Some(&GridTrackSize::Subgrid)
        || (frame.init.vertical.is_none()
            && (!super::baseline::is_vertical(&item.style) || frame.init.s.direction == Direction::Rtl))
    {
        return;
    }
    let Some(requested) =
        super::baseline::align_baseline_side(super::grid_vertical::resolved_justify(&item.style, &frame.init.s))
    else {
        return;
    };
    let n_cols = frame.init.n_cols;
    let first_col = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let last_col = ((ce - 1).min(n_cols) as usize).saturating_sub(1).max(first_col);
    let em = item.style.font_size;
    let cw = frame.init.item_pct_base(k);
    let (m_l, m_r) = (
        item.style.margin_left.resolve_or_zero(em, cw, viewport),
        item.style.margin_right.resolve_or_zero(em, cw, viewport),
    );
    let (m_t, m_b) = (
        item.style.margin_top.resolve_or_zero(em, cw, viewport),
        item.style.margin_bottom.resolve_or_zero(em, cw, viewport),
    );
    // Подъём — от кромки начала оси столбцов (слева; у `inline_rev` сверху вниз наоборот — снизу).
    let (ascent, outer, start) = match frame.init.vertical {
        None => {
            let bx = box_baseline_in_axis(item, &frame.init.s, true, requested, measurer);
            (m_l + bx, item.rect.width + m_l + m_r, PhysSide::Min)
        }
        Some(g) => {
            let by = box_baseline_in_axis(item, &frame.init.s, false, requested, measurer);
            let outer = item.rect.height + m_t + m_b;
            if g.inline_rev { (outer - (m_t + by), outer, PhysSide::Max) } else { (m_t + by, outer, PhysSide::Min) }
        }
    };
    let descent = (outer - ascent).max(0.0);
    let axis_vertical = frame.init.vertical.is_none();
    let side = if baseline_phys_side_in_axis(item, &frame.init.s, axis_vertical, requested) == start {
        BaselineSide::First
    } else {
        BaselineSide::Last
    };
    frame.init.item_col_baselines[k] = Some(ItemBaseline { side, ascent });
    let group = match side {
        BaselineSide::First => &mut frame.init.col_first_group[first_col],
        BaselineSide::Last => &mut frame.init.col_last_group[last_col],
    };
    group.0 = group.0.max(ascent);
    group.1 = group.1.max(descent);
}

/// CSS Grid L1 §6.2 — item с `align-self: [first|last] baseline` входит в группу
/// строки: `first` — строки, где item начинается, `last` — где заканчивается.
/// Запоминает подъём/спуск item'а и обновляет размеры группы.
fn record_item_baseline(frame: &mut Frame, i: usize, measurer: Option<&dyn TextMeasurer>, viewport: Size) {
    let k = frame.k;
    let (cs, _, rs, re) = frame.init.placements[k];
    let item = &frame.b.children[i];
    let Some(side) = super::baseline::align_baseline_side(super::baseline::resolved_align(&item.style, &frame.init.s))
    else {
        return;
    };
    // Строки subgrid'а — часть родительских: общая группа с соседями родителя здесь не
    // строится, item остаётся у начала области (как до baseline-выравнивания).
    if cs == 0 || rs == 0 || frame.init.row_heights.is_empty() || frame.init.inherited_rows.is_some() {
        return;
    }
    let n_rows = frame.init.n_rows;
    let first_row = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
    let last_row = ((re - 1).min(n_rows) as usize).saturating_sub(1).max(first_row);
    let st = &item.style;
    let em = st.font_size;
    let pct_base = frame.init.item_pct_base(k);
    let m_t = st.margin_top.resolve_or_zero(em, pct_base, viewport);
    let m_b = st.margin_bottom.resolve_or_zero(em, pct_base, viewport);
    // Вертикальная сетка: линия вертикальна, подъём считается по block-оси (физический `x`).
    let (ascent, outer) = match frame.init.vertical {
        None => (
            m_t + super::baseline::box_baseline_in_axis(item, &frame.init.s, false, side, measurer),
            item.rect.height + m_t + m_b,
        ),
        Some(g) => super::grid_vertical::block_ascent(&frame.init, g, item, side, viewport, measurer),
    };
    let descent = (outer - ascent).max(0.0);
    // Группа — по краю, к которому тянется линия (Align L3 §9.3): у item'а с обратным ходом
    // блоков `first baseline` сидит в группе конца строки, и наоборот.
    let side = match frame.init.vertical {
        None => side,
        Some(g) => super::grid_vertical::group_side(&frame.init, g, item, side),
    };
    frame.init.item_baselines[k] = Some(ItemBaseline { side, ascent });
    let group = match side {
        BaselineSide::First => &mut frame.init.row_first_group[first_row],
        BaselineSide::Last => &mut frame.init.row_last_group[last_row],
    };
    group.0 = group.0.max(ascent);
    group.1 = group.1.max(descent);
}

/// Is a track sizing function content-based in the sense of CSS Grid L1 §11.5: `min-content`,
/// `max-content` and `auto`.
fn is_content_sized(t: &GridTrackSize) -> bool {
    matches!(t, GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent)
}

/// Does the row track grow with its items' contribution (CSS Grid L1 §11.5)? `auto`, `min-content`,
/// `max-content`, `<flex>` and a `minmax(min, max)` with a content-based min or max. `minmax(_, <flex>)`
/// is excluded — it is a flexible track, resolved by the fr-pass, not by the items.
fn row_grows_with_content(t: &GridTrackSize) -> bool {
    match t {
        GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::Fr(_) => true,
        GridTrackSize::Minmax(min, max) => !max.is_fr() && (is_content_sized(min) || is_content_sized(max)),
        _ => false,
    }
}

/// CSS Grid L1 §11.7 «Expand Flexible Tracks» при неопределённой высоте контейнера: размер `fr` —
/// наибольшее из «база гибкой строки / коэффициент (если он больше 1)» и вкладов items, чьи
/// диапазоны пересекают гибкие строки (§11.7.1). Полученная высота сетки затем считается
/// определённой, и размер `fr` ищется заново по ней (второй проход, Grid L2 §11.7.1): коэффициенты
/// с суммой меньше единицы тогда сжимают строки (`0.4fr` ×2 при вкладе 100 — 32px, не 40px).
#[inline(never)]
fn expand_flexible_rows(init: &mut GridInit) {
    let n_rows = init.n_rows as usize;
    let flex: Vec<Option<f32>> = (0..n_rows)
        .map(|r| grid_track(r as u32, &init.eff_row_template, &init.auto_row).flex_factor())
        .collect();
    if flex.iter().all(Option::is_none) {
        return;
    }
    let base = init.row_heights.clone();
    let total_gap = init.row_gap * super::grid::gutter_count(&init.row_collapsed, n_rows) as f32;
    let mut fr = 0.0_f32;
    for r in 0..n_rows {
        if let Some(f) = flex[r] {
            fr = fr.max(base[r] / f.max(1.0));
        }
    }
    for &(r0, r1, h) in &init.flex_contribs {
        let span = r0..r1.min(n_rows);
        let fixed: f32 = span.clone().filter(|&r| flex[r].is_none()).map(|r| base[r]).sum();
        let gaps = init.row_gap * span.len().saturating_sub(1) as f32;
        let sum_f: f32 = span.filter_map(|r| flex[r]).sum();
        fr = fr.max((h - fixed - gaps).max(0.0) / sum_f.max(1.0));
    }
    for r in 0..n_rows {
        if let Some(f) = flex[r] {
            init.row_heights[r] = base[r].max(f * fr);
        }
    }
    let first_pass_total = init.row_heights.iter().sum::<f32>() + total_gap;
    let (fr, frozen) = super::grid::find_fr_size(&flex, &base, first_pass_total, total_gap);
    for r in 0..n_rows {
        if let Some(f) = flex[r]
            && !frozen[r]
        {
            init.row_heights[r] = base[r].max(f * fr);
        }
    }
}

/// CSS Grid L1 §11.6 «Maximize Tracks»: `minmax(<min>, <length>)` rows grow from their base size to
/// the growth limit `max(<length>, base)`. With an indefinite container block size the free space is
/// infinite, so every such row reaches its limit; with a definite one the leftover space is shared
/// equally and a row that hits its limit is frozen.
#[inline(never)]
fn maximize_bounded_rows(init: &mut GridInit, viewport: Size) {
    let n_rows = init.n_rows as usize;
    let mut limits: Vec<(usize, f32)> = Vec::new();
    for r in 0..n_rows {
        if let GridTrackSize::Minmax(_, max) = grid_track(r as u32, &init.eff_row_template, &init.auto_row)
            && let GridTrackSize::Length(l) = &**max
            && let Some(v) = l.resolve(init.em, init.definite_content_height, viewport)
        {
            limits.push((r, v.max(init.row_heights[r])));
        }
    }
    if limits.is_empty() {
        return;
    }
    let Some(total) = init.definite_content_height else {
        for (r, lim) in limits {
            init.row_heights[r] = lim;
        }
        return;
    };
    let gaps = init.row_gap * super::grid::gutter_count(&init.row_collapsed, n_rows) as f32;
    let mut free = total - init.row_heights.iter().sum::<f32>() - gaps;
    while free > 0.0 && !limits.is_empty() {
        let share = free / limits.len() as f32;
        let mut used = 0.0;
        let mut next = Vec::with_capacity(limits.len());
        for (r, lim) in limits {
            let add = share.min(lim - init.row_heights[r]);
            init.row_heights[r] += add;
            used += add;
            if init.row_heights[r] + 1e-4 < lim {
                next.push((r, lim));
            }
        }
        if used <= 1e-4 {
            break;
        }
        free -= used;
        limits = next;
    }
}

/// Runs once every item has been probed — CSS Grid L1 §11.7 fr-track
/// resolution and §12.3 `align-content` auto-row stretch (both skipped when
/// the row axis is subgridded — sizes are then fixed), and the row top
/// offsets the Final phase positions cells against. Copied verbatim from the
/// removed code's tail between the two loops.
fn finish_probe_pass(init: &mut GridInit, viewport: Size) {
    if let Some(rl) = init.row_lanes.take() {
        let (tmpl, auto_row) = (&init.eff_row_template, &init.auto_row);
        rl.apply(&mut init.row_heights, init.row_gap, |r| row_grows_with_content(grid_track(r as u32, tmpl, auto_row)));
    }
    if let Some(lanes) = &init.lanes {
        // Строка `k` — элемент `k`: смещение — его позиция в полосе, высота содержимого — самый
        // длинный поток (без промежутка после последнего элемента).
        init.row_offsets = lanes.pos.clone();
        init.y_off = lanes.extent;
        // Область по оси потока — до высоты контейнера (определённой, иначе по самому длинному потоку).
        let total = init.definite_content_height.unwrap_or(lanes.extent);
        lanes.extend_last(&mut init.row_heights, total);
        if init.s.grid_lanes.direction.fill_reverse {
            // `fill-reverse`: элементы набраны от конца оси потока — отражение в том же отрезке.
            super::grid_lanes::mirror(&mut init.row_offsets, &init.row_heights, total);
        }
        return;
    }
    let n_rows = init.n_rows;
    let row_gap = init.row_gap;
    let total_row_gap = row_gap * super::grid::gutter_count(&init.row_collapsed, n_rows as usize) as f32;
    if init.inherited_rows.is_none() {
        // CSS Grid L1 §6.2 / §11.5 — группа baseline-выравнивания занимает в строке
        // «наибольший подъём + наибольший спуск»; растут только строки, размер
        // которых зависит от содержимого.
        for r in 0..n_rows as usize {
            if !row_grows_with_content(grid_track(r as u32, &init.eff_row_template, &init.auto_row)) {
                continue;
            }
            for (asc, desc) in [init.row_first_group[r], init.row_last_group[r]] {
                if asc + desc > init.row_heights[r] {
                    init.row_heights[r] = asc + desc;
                }
            }
        }
        maximize_bounded_rows(init, viewport);
        if init.definite_content_height.is_none() {
            expand_flexible_rows(init);
        }
        // CSS Grid L1 §11.7 — the free space available to flexible (`fr`) tracks is
        // the container's content size minus the base sizes of the OTHER tracks
        // only. `row_heights[r]` for an `fr` track was seeded from its content's
        // probed intrinsic height (the fallback used when the container's block
        // size is indefinite) — that probed value is a floor for the final
        // `.max()` below, not a "fixed" size to subtract here. Counting it against
        // `definite_content_height` double-dips (BUG-277).
        let fixed_row_total: f32 = (0..n_rows)
            .map(|r| {
                if grid_track(r, &init.eff_row_template, &init.auto_row).fr().is_some() {
                    0.0
                } else {
                    init.row_heights[r as usize]
                }
            })
            .sum::<f32>()
            + total_row_gap;
        let free_row = init.definite_content_height.map(|h| (h - fixed_row_total).max(0.0)).unwrap_or(0.0);
        let total_row_fr: f32 = (0..n_rows)
            .map(|r| grid_track(r, &init.eff_row_template, &init.auto_row).fr().unwrap_or(0.0))
            .sum();
        if total_row_fr > 0.0 && free_row > 0.0 {
            // §11.7.1: сумма коэффициентов меньше единицы считается единицей.
            let fr_h = free_row / total_row_fr.max(1.0);
            for r in 0..n_rows {
                if let Some(f) = grid_track(r, &init.eff_row_template, &init.auto_row).fr() {
                    init.row_heights[r as usize] = (f * fr_h).max(init.row_heights[r as usize]);
                }
            }
        }

        // CSS Grid L1 §12.3 — `align-content: normal` behaves as `stretch` for a
        // grid container: leftover block-axis space is shared equally between the
        // `auto`-sized rows. `minmax(_, auto)` rows do not participate — the
        // track-sizing pass resolves them from their min side, not as auto.
        if matches!(init.s.align_content, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch) {
            let auto_rows: Vec<u32> = (0..n_rows)
                .filter(|&r| matches!(grid_track(r, &init.eff_row_template, &init.auto_row), GridTrackSize::Auto))
                .collect();
            let used: f32 = init.row_heights.iter().sum::<f32>() + total_row_gap;
            let free = init.definite_content_height.map(|h| h - used).unwrap_or(0.0);
            if free > 0.0 && !auto_rows.is_empty() {
                let per = free / auto_rows.len() as f32;
                for r in auto_rows {
                    init.row_heights[r as usize] += per;
                }
            }
        }
    }

    let (row_offsets, y_off) = if let Some(ctx) = &init.inherited_rows {
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_rows as usize).cloned().collect();
        let total = ctx.total_size();
        (offsets, total)
    } else {
        // CSS Box Alignment L3 §5 — `align-content` distributes the block-axis free
        // space left over by the tracks (only ever non-zero for a definite height).
        let used_row_total: f32 = init.row_heights.iter().sum::<f32>() + total_row_gap;
        let (ac_start, ac_extra) = grid_content_distribution(
            init.s.align_content,
            init.definite_content_height.map(|h| h - used_row_total).unwrap_or(0.0),
            super::grid::gutter_count(&init.row_collapsed, n_rows as usize) + 1,
            init.s.content_align_extra.align_safe,
        );
        let row_offsets =
            super::grid::track_offsets(&init.row_heights, &init.row_collapsed, row_gap, ac_extra, ac_start);
        let y_off = row_offsets.last().copied().unwrap_or(ac_start) + init.row_heights.last().copied().unwrap_or(0.0);
        (row_offsets, y_off)
    };
    init.row_offsets = row_offsets;
    init.y_off = y_off;
}

/// Handles exactly one item's Final-phase placement (CSS Grid L1 §11.2),
/// copied from the removed inline loop body (unplaced/subgrid/probe-reuse/
/// fresh-layout arms) except at the two recursive calls (subgrid and fresh
/// layout), which now either finish synchronously (`Advance`) or hand back
/// the item's `GridInit` for `run` to push and descend into. The probe-reuse
/// arm never calls `dispatch_box` at all — `translate_subtree` alone resolves
/// the item, exactly as the removed code's `else if` branch did.
fn step_final_item(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> StepOutcome {
    let k = frame.k;
    let i = frame.init.item_idxs[k];
    let (cs, ce, rs, re) = frame.init.placements[k];
    let pcb = frame.init.children_pcb;

    if cs == 0 || rs == 0 {
        // Unplaced — stack below grid content.
        let (ux, uy, uw, uh) = frame.init.unplaced_args();
        return match dispatch_box(
            &mut frame.b.children[i], ux, uy, uw, uh, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        ) {
            DispatchOutcome::Done => { post_final_item(frame, i, viewport, measurer, hp); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: same shape, for a grid item that is itself a
            // vertical writing-mode container.
            DispatchOutcome::NeedsVerticalLoop(ci) => {
                super::vertical_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
        };
    }

    let n_cols = frame.init.n_cols;
    let n_rows = frame.init.n_rows;
    let c0 = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let c1 = (ce - 1).min(n_cols) as usize;
    let r0 = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
    let r1 = (re - 1).min(n_rows) as usize;
    let cell_w = grid_track_span(&frame.init.col_offsets, &frame.init.col_widths, c0, c1);
    let cell_h = grid_track_span(&frame.init.row_offsets, &frame.init.row_heights, r0, r1);
    let (cell_x, cell_y) = frame.init.cell_origin(c0, r0, cell_h, cell_w);

    let child_col_subgrid = frame.b.children[i].style.grid_template_columns.first()
        == Some(&GridTrackSize::Subgrid);
    let child_row_subgrid = frame.b.children[i].style.grid_template_rows.first()
        == Some(&GridTrackSize::Subgrid);

    if child_col_subgrid || child_row_subgrid {
        let (final_col_ctx, final_row_ctx) = frame.init.subgrid_ctx(&frame.b.children[i], c0, c1, r0, r1);
        let _guard = SubgridContextGuard::set(final_col_ctx, final_row_ctx);
        let (ax, ay, aw, ah) =
            frame.init.final_args(&frame.b.children[i], (cell_x, cell_y), cell_w, None, viewport, measurer);
        let outcome = dispatch_box(
            &mut frame.b.children[i], ax, ay, aw, ah, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        );
        drop(_guard);
        match outcome {
            DispatchOutcome::Done => { post_final_item(frame, i, viewport, measurer, hp); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: same shape, for a grid item that is itself a
            // vertical writing-mode container.
            DispatchOutcome::NeedsVerticalLoop(ci) => {
                super::vertical_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
        }
    } else if let Some((probe_x, probe_y, mut reused)) = frame.init.probe_reuse[k].take() {
        // BUG-341 S33: `cell_w` above was derived from the same
        // `col_offsets`/`col_widths`/`(c0, c1)` as the probe pass's, so the
        // subtree reused here already has the correct final size — only its
        // position needs to catch up to the resolved row offset.
        crate::incremental::translate_subtree(&mut reused, cell_x - probe_x, cell_y - probe_y);
        frame.b.children[i] = reused;
        post_final_item(frame, i, viewport, measurer, hp);
        StepOutcome::Advance
    } else {
        // No usable probe: an unplaced-at-probe-time item can't reach here
        // (handled by the early-return above), so this is a subtree whose
        // probe touched `content-visibility: auto` and was refused for reuse.
        // The grid area's block size is definite by now (rows are resolved), so a
        // `%` height/min-height/max-height of the item resolves against it.
        let (ax, ay, aw, ah) = frame.init.final_args(
            &frame.b.children[i], (cell_x, cell_y), cell_w, Some(cell_h), viewport, measurer,
        );
        match dispatch_box(
            &mut frame.b.children[i], ax, ay, aw, ah, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        ) {
            DispatchOutcome::Done => { post_final_item(frame, i, viewport, measurer, hp); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: same shape, for a grid item that is itself a
            // vertical writing-mode container.
            DispatchOutcome::NeedsVerticalLoop(ci) => {
                super::vertical_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport, measurer, hp);
                StepOutcome::Advance
            }
        }
    }
}

/// Runs right after item `k` finishes its Final-phase placement — the
/// unplaced item's `y_off` advance, or CSS Grid L1 §11.2's per-cell
/// `align-items`/`justify-items`, copied verbatim from immediately after the
/// removed loop's recursive call/reuse branch. Shared by `step_final_item`'s
/// synchronous paths and `run`'s resume-after-descend path — both leave
/// `frame.k` pointing at this exact item, so re-deriving cell geometry from
/// `frame.init.placements[k]` gives the same answer either way.
fn post_final_item(
    frame: &mut Frame,
    i: usize,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    hp: &dyn HyphenationProvider,
) {
    let k = frame.k;
    let (cs, ce, rs, re) = frame.init.placements[k];
    if cs == 0 || rs == 0 {
        frame.init.y_off += frame.init.item_block_size(k, &frame.b.children[i], viewport);
        return;
    }

    if frame.init.vertical.is_some() {
        super::grid_vertical::place_item(&frame.init, k, &mut frame.b.children[i], viewport, measurer, hp);
        return;
    }

    let n_cols = frame.init.n_cols;
    let n_rows = frame.init.n_rows;
    let c0 = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let c1 = (ce - 1).min(n_cols) as usize;
    let r0 = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
    let r1 = (re - 1).min(n_rows) as usize;
    let cell_w = grid_track_span(&frame.init.col_offsets, &frame.init.col_widths, c0, c1);
    let cell_h = grid_track_span(&frame.init.row_offsets, &frame.init.row_heights, r0, r1);
    let (cell_x, cell_y) = frame.init.cell_origin(c0, r0, cell_h, cell_w);
    let rtl = frame.init.rtl;
    let content_width = frame.init.content_width;
    let pcb = frame.init.children_pcb;
    let s = Arc::clone(&frame.init.s);

    let item = &mut frame.b.children[i];
    // Позицию ниже выставляем в `item.rect` целиком, а поддерево переезжает вслед за ней в конце.
    let (orig_x, orig_y) = (item.rect.x, item.rect.y);
    let is = &item.style;
    let iem = is.font_size;
    let m_t = is.margin_top.resolve_or_zero(iem, cell_w, viewport);
    let m_b = is.margin_bottom.resolve_or_zero(iem, cell_w, viewport);
    let m_l = is.margin_left.resolve_or_zero(iem, cell_w, viewport);
    let m_r = is.margin_right.resolve_or_zero(iem, cell_w, viewport);

    // align-items (cross / block axis within cell). `stretch_h` records a
    // grown height for the relayout below — deferred until `item.rect.x` has
    // its final value from the justify-items block further down.
    let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
    // `self-start`/`self-end` считаются по собственному режиму письма элемента (Box Alignment §4.2).
    let align = super::grid_vertical::resolve_own(
        align,
        if matches!(is.align_self, AlignValue::Auto) {
            s.content_align_extra.items_own
        } else {
            is.content_align_extra.self_own
        },
        is,
        false,
        true,
    );
    let item_outer_h = item.rect.height + m_t + m_b;
    // CSS Box Alignment L3 §4.4: `safe` falls back to `start` once the item
    // overflows its grid area, instead of overflowing past the start edge.
    let align_safe = if matches!(is.align_self, AlignValue::Auto) {
        s.content_align_extra.items_safe
    } else {
        is.content_align_extra.self_safe
    };
    let align = if align_safe && item_outer_h > cell_h && matches!(align, AlignValue::End | AlignValue::Center) {
        AlignValue::Start
    } else {
        align
    };
    let mut stretch_h: Option<f32> = None;
    let mut stretch_w: Option<f32> = None;
    // Box Alignment L3 §6.2: у replaced-бокса с пропорцией `normal` ведёт себя как `start`;
    // растягивает его только явный `stretch` — по auto-оси, вторая ось выводится из пропорции.
    let replaced_ratio = matches!(item.kind, BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. })
        && is.aspect_ratio.is_some();
    let h_auto = is.height.is_none() || is.height_is_intrinsic_hint;
    let w_auto = is.width.is_none() || is.width_is_intrinsic_hint;
    // Нативный контрол без пропорции (BUG-1609): `normal` и `stretch` растягивают каждую
    // `auto`-ось независимо; UA-размер (`13×13` у checkbox, `174×21` у текстового поля) — не
    // авторский `width`/`height`, а запасной intrinsic-размер.
    let form_ctl = matches!(item.kind, BoxKind::FormControl { .. }) && !replaced_ratio;
    let form_h_auto = form_ctl && (is.height.is_none() || is.height_is_ua_default);
    let form_w_auto = form_ctl && (is.width.is_none() || is.width_is_ua_default);
    let justify_stretch = matches!(
        if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self },
        AlignValue::Stretch
    );
    let (auto_t, auto_b) = (is.margin_top.is_auto(), is.margin_bottom.is_auto());
    let (auto_l, auto_r) = (is.margin_left.is_auto(), is.margin_right.is_auto());
    match align {
        // Авто-поле по block-оси забирает свободное место: ни растяжения, ни baseline.
        _ if auto_t || auto_b => {
            item.rect.y = cell_y
                + super::grid_vertical::axis_offset(align, cell_h, item.rect.height, m_t, m_b, auto_t, auto_b);
        }
        AlignValue::End => {
            item.rect.y = cell_y + cell_h - item.rect.height - m_b;
        }
        AlignValue::Center => {
            item.rect.y = cell_y + (cell_h - item_outer_h) / 2.0 + m_t;
        }
        AlignValue::Baseline | AlignValue::LastBaseline if frame.init.item_baselines[k].is_some() => {
            // CSS Grid L1 §6.2: items группы стоят так, чтобы их базовые линии
            // совпали с общей — на подъёме группы от верха строки (`first`) либо на
            // спуске группы от низа строки (`last`).
            let ib = frame.init.item_baselines[k].unwrap_or(ItemBaseline { side: BaselineSide::First, ascent: m_t });
            let bl = ib.ascent - m_t;
            item.rect.y = match ib.side {
                BaselineSide::First => cell_y + frame.init.row_first_group[r0].0 - bl,
                BaselineSide::Last => {
                    let last_row = r1.saturating_sub(1).max(r0);
                    cell_y + cell_h - frame.init.row_last_group[last_row].1 - bl
                }
            };
        }
        AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
            // CSS Grid §11.2: `stretch` only grows items whose used block size is
            // `auto`; an explicit `height` is preserved (the item is top-aligned in
            // the cell, leaving free space below — like Edge).
            let explicit = matches!(align, AlignValue::Stretch);
            if replaced_ratio && !explicit {
                // `normal` у replaced с пропорцией = `start`: размер остаётся intrinsic.
            } else if replaced_ratio {
                (stretch_h, stretch_w) = stretch_replaced_block(is, &mut item.rect, cell_h - m_t - m_b, justify_stretch);
            } else if form_h_auto && cell_h - m_t - m_b > 0.0 {
                let h = cell_h - m_t - m_b;
                item.rect.height = h;
                stretch_h = Some(h);
            } else if h_auto && is.height.is_none() && item.rect.height < cell_h - m_t - m_b {
                let h = (cell_h - m_t - m_b).max(item.rect.height);
                item.rect.height = h;
                stretch_h = Some(h);
            }
            item.rect.y = cell_y + m_t;
        }
        _ => {
            item.rect.y = cell_y + m_t;
        }
    }

    // justify-items (inline axis within cell).
    let justify = if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self };
    let justify_auto = matches!(is.justify_self, AlignValue::Auto);
    let justify = super::grid_vertical::resolve_own(
        justify,
        if justify_auto { s.content_align_extra.justify_items_own } else { is.content_align_extra.justify_self_own },
        is,
        true,
        !rtl,
    );
    let justify = super::grid_vertical::resolve_side(
        justify,
        if justify_auto { s.content_align_extra.justify_items_side } else { is.content_align_extra.justify_self_side },
        s.writing_mode,
        rtl,
    );
    // CSS Sizing L4 §5.1: у не растягиваемого по inline-оси item'а с `aspect-ratio` и определённой
    // высотой ширина `auto` берётся из пропорции, а не из содержимого.
    if !replaced_ratio && !form_ctl && w_auto && !h_auto && !matches!(justify, AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal)
        && let Some(w) = ratio_width_for_height(is, item.rect.height, content_width, viewport)
    {
        item.rect.width = w;
        stretch_w = Some(w);
        stretch_h = stretch_h.or(Some(item.rect.height));
    }
    // BUG-1385: `lay_out` gives a ratio'd block with a definite height the ratio width; an
    // explicit `justify-self: stretch` (not `normal`, which is `start` for such an item) fills the area.
    if !replaced_ratio
        && !form_ctl
        && w_auto
        && (!h_auto || stretch_h.is_some())
        && matches!(justify, AlignValue::Stretch)
        && is.aspect_ratio.is_some_and(|(aw, ah)| aw > 0.0 && ah > 0.0)
        && cell_w - m_l - m_r > 0.0
    {
        let w = cell_w - m_l - m_r;
        item.rect.width = w;
        stretch_w = Some(w);
        stretch_h = stretch_h.or(Some(item.rect.height));
    }
    let item_outer_w = item.rect.width + m_l + m_r;
    let justify_safe = if matches!(is.justify_self, AlignValue::Auto) {
        s.content_align_extra.justify_items_safe
    } else {
        is.content_align_extra.justify_self_safe
    };
    let justify = if justify_safe && item_outer_w > cell_w && matches!(justify, AlignValue::End | AlignValue::Center) {
        AlignValue::Start
    } else {
        justify
    };
    // Кромка начала по inline-оси: слева, а при `direction: rtl` — справа.
    let (m_is, m_ie, auto_is, auto_ie) = if rtl { (m_r, m_l, auto_r, auto_l) } else { (m_l, m_r, auto_l, auto_r) };
    let at_start = |w: f32| if rtl { cell_x + cell_w - w - m_is } else { cell_x + m_is };
    let at_end = |w: f32| if rtl { cell_x + m_ie } else { cell_x + cell_w - w - m_ie };
    match justify {
        _ if auto_l || auto_r => {
            let off = super::grid_vertical::axis_offset(justify, cell_w, item.rect.width, m_is, m_ie, auto_is, auto_ie);
            item.rect.x = if rtl { cell_x + cell_w - off - item.rect.width } else { cell_x + off };
        }
        AlignValue::End => {
            item.rect.x = at_end(item.rect.width);
        }
        AlignValue::Center => {
            item.rect.x = cell_x + (cell_w - item_outer_w) / 2.0 + m_l;
        }
        AlignValue::Baseline | AlignValue::LastBaseline if frame.init.item_col_baselines[k].is_some() => {
            // Общая вертикальная линия группы столбца: на подъёме группы от левого края
            // (`First`) либо на спуске от правого (`Last`).
            let ib = frame.init.item_col_baselines[k].unwrap_or(ItemBaseline { side: BaselineSide::First, ascent: m_l });
            let bl = ib.ascent - m_l;
            item.rect.x = match ib.side {
                BaselineSide::First => cell_x + frame.init.col_first_group[c0].0 - bl,
                BaselineSide::Last => {
                    let last_col = c1.saturating_sub(1).max(c0);
                    cell_x + cell_w - frame.init.col_last_group[last_col].1 - bl
                }
            };
        }
        // Линии вдоль inline-оси у параллельного item'а нет: `first` — к началу, `last` — к
        // концу (Align L3 §9.1, запасное выравнивание).
        AlignValue::LastBaseline => {
            item.rect.x = at_end(item.rect.width);
        }
        AlignValue::Stretch if replaced_ratio && w_auto => {
            // Явный `justify-self: stretch` у replaced: ширина = область; высота по пропорции,
            // если её не растянул `align-self`.
            (stretch_h, stretch_w) = stretch_replaced_inline(is, &mut item.rect, cell_w - m_l - m_r, stretch_h, stretch_w);
            item.rect.x = at_start(item.rect.width);
        }
        AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal if form_w_auto && cell_w - m_l - m_r > 0.0 => {
            let w = cell_w - m_l - m_r;
            item.rect.width = w;
            stretch_w = Some(w);
            item.rect.x = at_start(item.rect.width);
        }
        AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
            item.rect.x = at_start(item.rect.width);
        }
        _ => {
            item.rect.x = at_start(item.rect.width);
        }
    }

    // Выравнивание сдвигало только сам бокс: потомки оставались там, где их раскладка поставила
    // в начале области (`align-items: end` рисовал бокс внизу, а его содержимое — вверху).
    let (rel_x, rel_y) = super::grid_vertical::relative_shift(item, cell_w, cell_h, viewport, rtl);
    let (new_x, new_y) = (item.rect.x + rel_x, item.rect.y + rel_y);
    item.rect.x = orig_x;
    item.rect.y = orig_y;

    // BUG-644: `align-items: stretch` above only widened the outer box by
    // reassigning `item.rect.height` — the subtree underneath was laid out
    // (probe or reused-from-probe) against the smaller pre-stretch height, so
    // a child whose own layout depends on this item's block size (e.g. a
    // nested flex container's `align-items: center`) still has it resolved
    // against that stale height. Relay out with the final height as a used-
    // size override so it re-resolves, mirroring `flex_trampoline`'s
    // `relayout_column_flex` (`box_tree/flex_trampoline.rs`).
    if stretch_h.is_some() || stretch_w.is_some() {
        // Повторная раскладка читает размеры как border-box: нерастянутая ось контрола с
        // `content-box`-размером (UA `13px` + рамка) иначе потеряла бы рамку.
        if form_ctl {
            stretch_h = stretch_h.or(Some(item.rect.height));
            stretch_w = stretch_w.or(Some(item.rect.width));
        }
        // BUG-1414: override ниже читает размеры как border-box. Ось, которую растяжение не
        // тронуло, но у которой задан авторский размер (`width: 60px` + padding, content-box),
        // иначе потеряла бы рамку и поля — фиксируем её текущим border-box размером.
        if !w_auto {
            stretch_w = stretch_w.or(Some(item.rect.width));
        }
        if !h_auto {
            stretch_h = stretch_h.or(Some(item.rect.height));
        }
        // If `item` is itself a subgrid, its column/row tracks resolve through
        // the thread-local `SubgridContextGuard` `step_final_item` set up for
        // the original placement pass (already dropped by the time this
        // stretch-driven relayout runs) — re-establish it from the same
        // `col_widths[c0..c1]`/`row_heights[r0..r1]` slices, or a subgrid item
        // relaid out here loses its inherited tracks entirely (BUG-644 srez 2
        // regression: `a.w=0` in `grid_subgrid_column_layout`).
        let child_col_subgrid = item.style.grid_template_columns.first() == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = item.style.grid_template_rows.first() == Some(&GridTrackSize::Subgrid);
        let _guard = (child_col_subgrid || child_row_subgrid).then(|| {
            let (col_ctx, row_ctx) = frame.init.subgrid_ctx(&frame.b.children[i], c0, c1, r0, r1);
            SubgridContextGuard::set(col_ctx, row_ctx)
        });

        // От начала области, как и первая раскладка: `lay_out` сам добавляет поля к `start_*`.
        let (_, _, offer_w, _) =
            frame.init.final_args(&frame.b.children[i], (cell_x, cell_y), cell_w, None, viewport, measurer);
        let item = &mut frame.b.children[i];
        lay_out_with_used_size(
            item, cell_x, cell_y, offer_w, stretch_h, measurer, viewport, pcb, hp, false,
            UsedSizeOverride {
                height: stretch_h,
                width: stretch_w,
                box_sizing: Some(BoxSizing::BorderBox),
                ..Default::default()
            },
        );
    }
    let item = &mut frame.b.children[i];
    crate::incremental::translate_subtree(item, new_x - item.rect.x, new_y - item.rect.y);
}

/// Border-box ширина по `aspect-ratio` (физическая `ширина / высота`) и border-box высоте `height`;
/// `None` без пропорции. Пропорция относится к box-sizing-боксу: у `content-box` рамка и поля
/// вычитаются из высоты и добавляются к ширине.
#[inline(never)]
fn ratio_width_for_height(is: &ComputedStyle, height: f32, pct_base: f32, viewport: Size) -> Option<f32> {
    let (aw, ah) = is.aspect_ratio?;
    if aw <= 0.0 || ah <= 0.0 {
        return None;
    }
    if matches!(is.box_sizing, BoxSizing::BorderBox) {
        return Some((height * aw / ah).max(0.0));
    }
    let em = is.font_size;
    let pad_w = is.border_left_width
        + is.border_right_width
        + is.padding_left.resolve_or_zero(em, pct_base, viewport)
        + is.padding_right.resolve_or_zero(em, pct_base, viewport);
    let pad_h = is.border_top_width
        + is.border_bottom_width
        + is.padding_top.resolve_or_zero(em, pct_base, viewport)
        + is.padding_bottom.resolve_or_zero(em, pct_base, viewport);
    Some(((height - pad_h).max(0.0) * aw / ah + pad_w).max(0.0))
}

/// Явный `align-self: stretch` у replaced с пропорцией (Box Alignment L3 §6.2): высота = область,
/// ширина по пропорции, если `justify-self` её не растягивает. Возвращает (высота, ширина).
#[inline(never)]
fn stretch_replaced_block(is: &ComputedStyle, rect: &mut Rect, avail_h: f32, justify_stretch: bool) -> (Option<f32>, Option<f32>) {
    let h = avail_h.max(0.0);
    if !(is.height.is_none() || is.height_is_intrinsic_hint) || h <= 0.0 {
        return (None, None);
    }
    let w_auto = is.width.is_none() || is.width_is_intrinsic_hint;
    let ratio = is.aspect_ratio;
    rect.height = h;
    let mut stretch_w = None;
    if w_auto && !justify_stretch && let Some((aw, ah)) = ratio && ah > 0.0 {
        let w = h * aw / ah;
        rect.width = w;
        stretch_w = Some(w);
    }
    (Some(h), stretch_w)
}

/// Явный `justify-self: stretch` у replaced с пропорцией: ширина = область, высота по пропорции,
/// если её не растянул `align-self`. Принимает и возвращает текущие (высота, ширина).
#[inline(never)]
fn stretch_replaced_inline(
    is: &ComputedStyle,
    rect: &mut Rect,
    avail_w: f32,
    stretch_h: Option<f32>,
    stretch_w: Option<f32>,
) -> (Option<f32>, Option<f32>) {
    let w = avail_w.max(0.0);
    if w <= 0.0 {
        return (stretch_h, stretch_w);
    }
    rect.width = w;
    let h_auto = is.height.is_none() || is.height_is_intrinsic_hint;
    let mut stretch_h = stretch_h;
    if stretch_h.is_none() && h_auto && let Some((aw, ah)) = is.aspect_ratio && aw > 0.0 {
        let h = w * ah / aw;
        rect.height = h;
        stretch_h = Some(h);
    }
    (stretch_h, Some(w))
}

/// Runs once every item is placed — (moved in from `layout_dispatch.rs`'s
/// former post-`lay_out_grid` code) the container's own `b.rect.height`.
/// Copied from the removed code's dispatch-arm tail, plus the deferred
/// absolutely-positioned children pass (`grid_abspos::lay_out_abs`).
fn finish_frame(frame: &mut Frame, measurer: Option<&dyn TextMeasurer>, viewport: Size, hp: &dyn HyphenationProvider) {
    if let Some(geom) = frame.init.vertical {
        super::grid_vertical::finish_container(&mut frame.b, &frame.init, geom);
        frame.b.grid_baselines = super::grid_vertical::container_baselines(&frame.b, &frame.init, geom, measurer);
        let init = &*frame.init;
        let content = Rect::new(
            init.content_x,
            init.content_y,
            (frame.b.rect.width - geom.frame_horiz).max(0.0),
            init.content_width,
        );
        super::grid_abspos::lay_out_abs(
            &mut frame.b, &init.s, Some(init), init.is_positioned, init.own_pcb, content, measurer, viewport, hp,
        );
        return;
    }
    let content_height = frame.init.y_off + super::fieldset::placed_content_offset(&frame.b);
    finish_container_height(
        &mut frame.b,
        &frame.init.s,
        frame.init.em,
        frame.init.available_height,
        frame.init.padding_top,
        frame.init.padding_bottom,
        // BUG-1397: the items are already left out of the tracks (`post_probe_item`); the explicit
        // tracks keep the grid's block size unless `contain-intrinsic-height` replaces it.
        frame.init.size_contained && frame.init.s.contain_intrinsic_height.is_some(),
        viewport,
        content_height,
    );
    frame.b.grid_baselines = container_baselines(frame, measurer);
    record_own_tracks(frame);
    record_used_tracks(frame);
    let init = &*frame.init;
    let content_bottom = frame.b.rect.y + frame.b.rect.height - init.s.border_bottom_width - init.padding_bottom;
    let content = Rect::new(
        init.content_x,
        init.content_y,
        init.content_width,
        (content_bottom - init.content_y).max(0.0),
    );
    super::grid_abspos::lay_out_abs(
        &mut frame.b, &init.s, Some(init), init.is_positioned, init.own_pcb, content, measurer, viewport, hp,
    );
}

/// Дорожки самого контейнера для щелей `column-rule`/`row-rule` (CSS Gap Decorations L1): по
/// элементам их не восстановить, когда ни один не примыкает к соседу (`fr`-строки с одним
/// элементом, пустые дорожки, элемент-«мост»). Пишутся только для осей, которые контейнер
/// раскладывает сам и у которых нет `repeat(auto-*)` (у того пустые дорожки схлопываются, и
/// paint находит щели по элементам); ось `subgrid` уже несёт дорожки родителя
/// (`layout_dispatch`, `peek_tracks`). Координаты — от начала content box.
fn record_own_tracks(frame: &mut Frame) {
    let init = &frame.init;
    let spans = |offsets: &[f32], sizes: &[f32], n: u32| -> Option<Vec<(f32, f32)>> {
        let n = n as usize;
        (n >= 2 && offsets.len() >= n && sizes.len() >= n)
            .then(|| (0..n).map(|t| (offsets[t], offsets[t] + sizes[t])).collect())
    };
    let mut tracks = frame.b.subgrid_tracks.take().map(|t| *t).unwrap_or_default();
    if tracks.cols.is_none() && init.s.grid_template_col_auto_repeat.is_none() {
        tracks.cols = spans(&init.col_offsets, &init.col_widths, init.n_cols);
    }
    if tracks.rows.is_none() && init.s.grid_template_row_auto_repeat.is_none() && init.lanes.is_none() {
        tracks.rows = spans(&init.row_offsets, &init.row_heights, init.n_rows);
    }
    frame.b.subgrid_tracks = (tracks.cols.is_some() || tracks.rows.is_some()).then(|| Box::new(tracks));
}

/// CSSOM resolved value of `grid-template-columns`/`-rows` (Grid L1 §7.2.6): every track of the grid —
/// explicit and implicit, an emptied `auto-fit` track as `0` — with the line names the layout used.
#[inline(never)]
fn record_used_tracks(frame: &mut Frame) {
    let init = &frame.init;
    let (n_cols, n_rows) = (init.n_cols as usize, init.n_rows as usize);
    let sizes = |v: &[f32], n: usize| v[..n.min(v.len())].to_vec();
    frame.b.used_grid_tracks = Some(Box::new(UsedGridTracks {
        cols: sizes(&init.col_widths, n_cols),
        rows: sizes(&init.row_heights, n_rows),
        col_names: init.col_names.clone(),
        row_names: init.row_names.clone(),
    }));
}

/// CSS Grid L1 §6.1 — первая и последняя базовая линия контейнера, от верхней
/// кромки его border box. Первая: общая линия группы `first baseline` первой
/// строки, а без неё — первая (в порядке `order`) item'а, чья область задевает
/// первую строку; последняя — то же для последней строки и `last baseline`.
/// `None` у сетки без размещённых items.
fn container_baselines(frame: &Frame, measurer: Option<&dyn TextMeasurer>) -> Option<(f32, f32)> {
    let init = &frame.init;
    let n_rows = init.n_rows as usize;
    if n_rows == 0 || init.row_offsets.len() < n_rows {
        return None;
    }
    let mut placed: Vec<usize> = (0..init.item_idxs.len())
        .filter(|&k| init.placements[k].0 != 0 && init.placements[k].2 != 0)
        .collect();
    // Порядок сетки: `order`, затем по колонкам (в пределах строки) и порядку документа
    // (WPT `grid-baseline-004`: берётся item первой колонки независимо от порядка в DOM).
    placed.sort_by_key(|&k| (frame.b.children[init.item_idxs[k]].style.order, init.placements[k].0));
    let top = frame.b.rect.y;
    let row_top = |r: usize| init.content_y + init.row_offsets[r];
    let item_baseline = |k: usize, side: BaselineSide| {
        let c = &frame.b.children[init.item_idxs[k]];
        c.rect.y + super::baseline::box_baseline_in_axis(c, &init.s, false, side, measurer)
    };
    let starts_in_row = |k: usize, r: usize| init.placements[k].2 as usize == r + 1;
    let ends_in_row = |k: usize, r: usize| {
        let end = init.placements[k].3 as usize;
        end.saturating_sub(1).min(n_rows).saturating_sub(1).max(init.placements[k].2 as usize - 1) == r
    };
    // Участники baseline-выравнивания строки: сначала группа запрошенной стороны, а
    // при её отсутствии — группа другой стороны (§6.1: «участвуют в выравнивании»).
    let group_baseline = |r: usize, want: BaselineSide| -> Option<f32> {
        let in_group = |k: usize, side: BaselineSide| match side {
            BaselineSide::First => starts_in_row(k, r),
            BaselineSide::Last => ends_in_row(k, r),
        } && init.item_baselines[k].is_some_and(|ib| ib.side == side);
        let other = if want == BaselineSide::First { BaselineSide::Last } else { BaselineSide::First };
        [want, other].into_iter().find(|&side| placed.iter().any(|&k| in_group(k, side))).map(|side| match side {
            BaselineSide::First => row_top(r) + init.row_first_group[r].0,
            BaselineSide::Last => row_top(r) + init.row_heights[r] - init.row_last_group[r].1,
        })
    };

    // Пустая первая (последняя) строка не мешает: берётся ближайшая строка с items
    // (WPT `grid-baseline-004`: «items in the second row are evaluated»).
    let first = (0..n_rows).find_map(|r| {
        group_baseline(r, BaselineSide::First).or_else(|| {
            placed.iter().copied().find(|&k| starts_in_row(k, r)).map(|k| item_baseline(k, BaselineSide::First))
        })
    });
    let last = (0..n_rows).rev().find_map(|r| {
        group_baseline(r, BaselineSide::Last).or_else(|| {
            placed.iter().rev().copied().find(|&k| ends_in_row(k, r)).map(|k| item_baseline(k, BaselineSide::Last))
        })
    });
    match (first, last) {
        (None, None) => None,
        (f, l) => {
            let f = f.or(l)?;
            Some((f - top, l.unwrap_or(f) - top))
        }
    }
}

/// CSS 2.1 §10.6.3/§10.6.7, CSS Box Sizing L4 §5 — resolve a grid container's
/// own border-box `height` from its content height. Shared by `finish_frame`
/// (populated container) and `layout_dispatch.rs`'s grid dispatch arm (the
/// `build_grid_init` returns `None`/no-items case) so the empty-container
/// early exit does not need its own copy of this logic — both feed it
/// `content_height == 0.0` in that case, matching the removed `lay_out_grid`'s
/// `return 0.0` early exit exactly. Absolutely-positioned children are not
/// grid items and are laid out separately (`grid_abspos::lay_out_abs`).
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_container_height(
    b: &mut LayoutBox,
    s: &ComputedStyle,
    em: f32,
    available_height: Option<f32>,
    padding_top: f32,
    padding_bottom: f32,
    size_contained: bool,
    viewport: Size,
    content_height: f32,
) {
    b.rect.height = if let Some(h_len) = &s.height
        && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
    {
        match s.box_sizing {
            BoxSizing::ContentBox => {
                (h + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width).max(0.0)
            }
            BoxSizing::BorderBox => {
                h.max(padding_top + padding_bottom + s.border_top_width + s.border_bottom_width)
            }
        }
    } else if let Some((aw, ah)) = s.aspect_ratio
        && aw > 0.0
        && ah > 0.0
    {
        (b.rect.width * ah / aw).max(0.0)
    } else {
        let ch = contained_content_height(size_contained, s, em, viewport, content_height);
        ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
    };
    // CSS 2.1 §10.4 / Sizing L3 §5: clamp after track sizing — `max-height` does not
    // take part in resolving the row tracks (BUG-1314).
    super::layout_cache::clamp_min_max_height(b, s, em, available_height, viewport, padding_top, padding_bottom, 0.0);
}
