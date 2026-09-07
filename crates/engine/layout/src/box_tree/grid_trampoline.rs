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
    pub(super) col_widths: Vec<f32>,
    pub(super) col_offsets: Vec<f32>,
    /// Masonry-stripped row template, owned — `grid::grid_track` reads it in
    /// both `post_probe_item` (auto-row growth) and `finish_probe_pass` (fr
    /// resolution/align-content), both of which outlive `build_grid_init`'s
    /// own borrow of `s.grid_template_rows`.
    pub(super) eff_row_template: Vec<GridTrackSize>,
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
    /// BUG-341 S33 probe-reuse cache — see `build_grid_init`'s doc comment.
    /// `(probe_x, probe_y, laid-out subtree)`, taken (and consumed) by the
    /// Final phase's `step_final_item`.
    pub(super) probe_reuse: Vec<Option<(f32, f32, LayoutBox)>>,
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
    };
    let mut stack: Vec<Frame> = Vec::new();

    loop {
        match current.pass {
            Pass::Probe => {
                if current.k >= current.init.item_idxs.len() {
                    finish_probe_pass(&mut current.init);
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
                        };
                        stack.push(current);
                        current = child_frame;
                    }
                }
            }
            Pass::Final => {
                if current.k >= current.init.item_idxs.len() {
                    finish_frame(&mut current, viewport);
                    match stack.pop() {
                        None => {
                            *b = current.b;
                            return;
                        }
                        Some(mut parent) => {
                            let i = parent.init.item_idxs[parent.k];
                            parent.b.children[i] = current.b;
                            match parent.pass {
                                Pass::Probe => post_probe_item(&mut parent, i),
                                Pass::Final => post_final_item(&mut parent, i, viewport),
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
    let (cs, ce, rs, re) = frame.init.placements[k];
    let n_cols = frame.init.n_cols;
    let n_rows = frame.init.n_rows;
    let c0 = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let c1 = (ce - 1).min(n_cols) as usize;
    let cell_w = grid_track_span(&frame.init.col_offsets, &frame.init.col_widths, c0, c1);
    let probe_x = frame.init.content_x + frame.init.col_offsets.get(c0).copied().unwrap_or(0.0);
    let pcb = frame.init.children_pcb;

    let child_col_subgrid = frame.b.children[i].style.grid_template_columns.first()
        == Some(&GridTrackSize::Subgrid);
    let child_row_subgrid = frame.b.children[i].style.grid_template_rows.first()
        == Some(&GridTrackSize::Subgrid);

    if child_col_subgrid || child_row_subgrid {
        let child_col_ctx = if child_col_subgrid && c1 > c0 {
            Some(SubgridContext::from_parent_tracks(&frame.init.col_widths[c0..c1], frame.init.col_gap))
        } else {
            None
        };
        let child_row_ctx = if child_row_subgrid {
            let r0 = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
            let re_eff = re.max(rs + 1);
            let r1 = (re_eff - 1).min(n_rows) as usize;
            if r1 > r0 {
                Some(SubgridContext::from_parent_tracks(&frame.init.row_heights[r0..r1], frame.init.row_gap))
            } else {
                None
            }
        } else {
            None
        };
        let _guard = SubgridContextGuard::set(child_col_ctx, child_row_ctx);
        let outcome = dispatch_box(
            &mut frame.b.children[i], probe_x, 0.0, cell_w, None, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        );
        drop(_guard);
        match outcome {
            DispatchOutcome::Done => { post_probe_item(frame, i); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            // LAYOUT-2 срез 6: a (subgrid) grid item that is itself a table —
            // same shape as the flex/block-flow arms above.
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 7: a (subgrid) grid item that is itself a
            // multicol container — same shape as the flex/block-flow/table
            // arms above.
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
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
        match dispatch_box(
            &mut frame.b.children[i], probe_x, 0.0, cell_w, None, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        ) {
            DispatchOutcome::Done => { post_probe_item(frame, i); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_probe_item(frame, i);
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
fn post_probe_item(frame: &mut Frame, i: usize) {
    let k = frame.k;
    let (_, _, rs, _) = frame.init.placements[k];
    let child_col_subgrid = frame.b.children[i].style.grid_template_columns.first()
        == Some(&GridTrackSize::Subgrid);
    let child_row_subgrid = frame.b.children[i].style.grid_template_rows.first()
        == Some(&GridTrackSize::Subgrid);
    if !(child_col_subgrid || child_row_subgrid) {
        let touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
        let outer = frame.probe_outer_cv;
        CV_AUTO_TOUCHED.with(|c| c.set(outer || touched_here));
        if !touched_here {
            let n_cols = frame.init.n_cols;
            let c0 = (frame.init.placements[k].0 - 1).min(n_cols.saturating_sub(1)) as usize;
            let probe_x = frame.init.content_x + frame.init.col_offsets.get(c0).copied().unwrap_or(0.0);
            frame.init.probe_reuse[k] = Some((probe_x, 0.0, frame.b.children[i].clone()));
        }
    }

    let r0 = (rs - 1) as usize;
    if r0 < frame.init.row_heights.len()
        && frame.init.inherited_rows.is_none()
        && matches!(
            grid_track(r0 as u32, &frame.init.eff_row_template, &frame.init.s.grid_auto_rows),
            GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::Fr(_)
        )
    {
        let item_h = frame.b.children[i].rect.height;
        if item_h > frame.init.row_heights[r0] {
            frame.init.row_heights[r0] = item_h;
        }
    }
}

/// Runs once every item has been probed — CSS Grid L1 §11.7 fr-track
/// resolution and §12.3 `align-content` auto-row stretch (both skipped when
/// the row axis is subgridded — sizes are then fixed), and the row top
/// offsets the Final phase positions cells against. Copied verbatim from the
/// removed code's tail between the two loops.
fn finish_probe_pass(init: &mut GridInit) {
    let n_rows = init.n_rows;
    let row_gap = init.row_gap;
    let total_row_gap = if n_rows > 1 { row_gap * (n_rows - 1) as f32 } else { 0.0 };
    if init.inherited_rows.is_none() {
        // CSS Grid L1 §11.7 — the free space available to flexible (`fr`) tracks is
        // the container's content size minus the base sizes of the OTHER tracks
        // only. `row_heights[r]` for an `fr` track was seeded from its content's
        // probed intrinsic height (the fallback used when the container's block
        // size is indefinite) — that probed value is a floor for the final
        // `.max()` below, not a "fixed" size to subtract here. Counting it against
        // `definite_content_height` double-dips (BUG-277).
        let fixed_row_total: f32 = (0..n_rows)
            .map(|r| {
                if grid_track(r, &init.eff_row_template, &init.s.grid_auto_rows).fr().is_some() {
                    0.0
                } else {
                    init.row_heights[r as usize]
                }
            })
            .sum::<f32>()
            + total_row_gap;
        let free_row = init.definite_content_height.map(|h| (h - fixed_row_total).max(0.0)).unwrap_or(0.0);
        let total_row_fr: f32 = (0..n_rows)
            .map(|r| grid_track(r, &init.eff_row_template, &init.s.grid_auto_rows).fr().unwrap_or(0.0))
            .sum();
        if total_row_fr > 0.0 && free_row > 0.0 {
            let fr_h = free_row / total_row_fr;
            for r in 0..n_rows {
                if let Some(f) = grid_track(r, &init.eff_row_template, &init.s.grid_auto_rows).fr() {
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
                .filter(|&r| matches!(grid_track(r, &init.eff_row_template, &init.s.grid_auto_rows), GridTrackSize::Auto))
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
            n_rows as usize,
        );
        let mut row_offsets: Vec<f32> = Vec::with_capacity(n_rows as usize);
        let mut y_off = ac_start;
        for r in 0..n_rows {
            row_offsets.push(y_off);
            y_off += init.row_heights[r as usize] + if r < n_rows - 1 { row_gap + ac_extra } else { 0.0 };
        }
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
    let content_x = frame.init.content_x;
    let content_y = frame.init.content_y;
    let content_width = frame.init.content_width;
    let pcb = frame.init.children_pcb;

    if cs == 0 || rs == 0 {
        // Unplaced — stack below grid content.
        let y = content_y + frame.init.y_off;
        return match dispatch_box(
            &mut frame.b.children[i], content_x, y, content_width, None, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        ) {
            DispatchOutcome::Done => { post_final_item(frame, i, viewport); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
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
    let cell_x = content_x + frame.init.col_offsets.get(c0).copied().unwrap_or(0.0);
    let cell_y = content_y + frame.init.row_offsets.get(r0).copied().unwrap_or(0.0);
    let cell_w = grid_track_span(&frame.init.col_offsets, &frame.init.col_widths, c0, c1);

    let child_col_subgrid = frame.b.children[i].style.grid_template_columns.first()
        == Some(&GridTrackSize::Subgrid);
    let child_row_subgrid = frame.b.children[i].style.grid_template_rows.first()
        == Some(&GridTrackSize::Subgrid);

    if child_col_subgrid || child_row_subgrid {
        let final_col_ctx = if child_col_subgrid && c1 > c0 {
            Some(SubgridContext::from_parent_tracks(&frame.init.col_widths[c0..c1], frame.init.col_gap))
        } else {
            None
        };
        let final_row_ctx = if child_row_subgrid && r1 > r0 {
            Some(SubgridContext::from_parent_tracks(&frame.init.row_heights[r0..r1], frame.init.row_gap))
        } else {
            None
        };
        let _guard = SubgridContextGuard::set(final_col_ctx, final_row_ctx);
        let outcome = dispatch_box(
            &mut frame.b.children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        );
        drop(_guard);
        match outcome {
            DispatchOutcome::Done => { post_final_item(frame, i, viewport); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
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
        post_final_item(frame, i, viewport);
        StepOutcome::Advance
    } else {
        // No usable probe: an unplaced-at-probe-time item can't reach here
        // (handled by the early-return above), so this is a subtree whose
        // probe touched `content-visibility: auto` and was refused for reuse.
        match dispatch_box(
            &mut frame.b.children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp,
            false, None, AlignValue::Auto, None,
        ) {
            DispatchOutcome::Done => { post_final_item(frame, i, viewport); StepOutcome::Advance }
            DispatchOutcome::NeedsBlockFlowLoop(ci) => {
                block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(ci) => {
                super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsGridLoop(ci) => StepOutcome::Descend(ci),
            DispatchOutcome::NeedsTableLoop(ci) => {
                super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsMulticolLoop(ci) => {
                super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
                post_final_item(frame, i, viewport);
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
fn post_final_item(frame: &mut Frame, i: usize, viewport: Size) {
    let k = frame.k;
    let (cs, ce, rs, re) = frame.init.placements[k];
    if cs == 0 || rs == 0 {
        frame.init.y_off += frame.b.children[i].rect.height;
        return;
    }

    let n_cols = frame.init.n_cols;
    let n_rows = frame.init.n_rows;
    let c0 = (cs - 1).min(n_cols.saturating_sub(1)) as usize;
    let c1 = (ce - 1).min(n_cols) as usize;
    let r0 = (rs - 1).min(n_rows.saturating_sub(1)) as usize;
    let r1 = (re - 1).min(n_rows) as usize;
    let cell_x = frame.init.content_x + frame.init.col_offsets.get(c0).copied().unwrap_or(0.0);
    let cell_y = frame.init.content_y + frame.init.row_offsets.get(r0).copied().unwrap_or(0.0);
    let cell_w = grid_track_span(&frame.init.col_offsets, &frame.init.col_widths, c0, c1);
    let cell_h = grid_track_span(&frame.init.row_offsets, &frame.init.row_heights, r0, r1);
    let content_width = frame.init.content_width;
    let s = Arc::clone(&frame.init.s);

    let item = &mut frame.b.children[i];
    let is = &item.style;
    let iem = is.font_size;
    let m_t = is.margin_top.resolve_or_zero(iem, content_width, viewport);
    let m_b = is.margin_bottom.resolve_or_zero(iem, content_width, viewport);
    let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
    let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);

    // align-items (cross / block axis within cell).
    let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
    let item_outer_h = item.rect.height + m_t + m_b;
    match align {
        AlignValue::End => {
            item.rect.y = cell_y + cell_h - item.rect.height - m_b;
        }
        AlignValue::Center => {
            item.rect.y = cell_y + (cell_h - item_outer_h) / 2.0 + m_t;
        }
        AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
            // CSS Grid §11.2: `stretch` only grows items whose used block size is
            // `auto`; an explicit `height` is preserved (the item is top-aligned in
            // the cell, leaving free space below — like Edge).
            if is.height.is_none() && item.rect.height < cell_h - m_t - m_b {
                item.rect.height = (cell_h - m_t - m_b).max(item.rect.height);
            }
            item.rect.y = cell_y + m_t;
        }
        _ => {
            item.rect.y = cell_y + m_t;
        }
    }

    // justify-items (inline axis within cell).
    let justify = if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self };
    let item_outer_w = item.rect.width + m_l + m_r;
    match justify {
        AlignValue::End => {
            item.rect.x = cell_x + cell_w - item.rect.width - m_r;
        }
        AlignValue::Center => {
            item.rect.x = cell_x + (cell_w - item_outer_w) / 2.0 + m_l;
        }
        AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
            item.rect.x = cell_x + m_l;
        }
        _ => {
            item.rect.x = cell_x + m_l;
        }
    }
}

/// Runs once every item is placed — (moved in from `layout_dispatch.rs`'s
/// former post-`lay_out_grid` code) the container's own `b.rect.height`.
/// Copied from the removed code's dispatch-arm tail. Grid has no
/// `flex_abs`-equivalent deferred absolutely-positioned children pass — see
/// `finish_container_height`'s doc comment.
fn finish_frame(frame: &mut Frame, viewport: Size) {
    let content_height = frame.init.y_off;
    finish_container_height(
        &mut frame.b,
        &frame.init.s,
        frame.init.em,
        frame.init.available_height,
        frame.init.padding_top,
        frame.init.padding_bottom,
        frame.init.size_contained,
        viewport,
        content_height,
    );
}

/// CSS 2.1 §10.6.3/§10.6.7, CSS Box Sizing L4 §5 — resolve a grid container's
/// own border-box `height` from its content height. Shared by `finish_frame`
/// (populated container) and `layout_dispatch.rs`'s grid dispatch arm (the
/// `build_grid_init` returns `None`/no-items case) so the empty-container
/// early exit does not need its own copy of this logic — both feed it
/// `content_height == 0.0` in that case, matching the removed `lay_out_grid`'s
/// `return 0.0` early exit exactly. Unlike `flex_trampoline::finish_frame`,
/// there is no `flex_abs`-equivalent tail here: absolutely-positioned
/// children of a grid container are not excluded from `item_idxs` upstream
/// (`grid::build_grid_init`, unlike `flex::build_flex_init`, does not filter
/// `Position::Absolute`/`Fixed` out of its item list) — a pre-existing gap
/// this slice reproduces unchanged, not introduces; not in scope to fix here.
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
}
