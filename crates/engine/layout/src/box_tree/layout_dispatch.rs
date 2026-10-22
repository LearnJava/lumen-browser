use super::*;
use super::block_flow_trampoline::{self, DispatchOutcome};
use super::layout_cache::{finalize_block_height, lay_out_cache_checked};

mod dispatch_box;
pub(super) use dispatch_box::dispatch_box;

/// `pcb` — rect positioned containing block (ближайший предок с position != static),
/// используется для layout абсолютно-позиционированных потомков.
///
/// `in_block_flow` — `true` only when this box is laid out as a normal in-flow
/// block child of a block container. It gates parent↔first-child margin
/// collapsing (CSS 2.1 §8.3.1): a box laid out as a flex/grid item, table cell,
/// or document root establishes an independent formatting context and must not
/// collapse its top margin into its first child, so those call sites pass
/// `false`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
) {
    // Thin wrapper: most call sites lay out boxes that establish an independent
    // formatting context (flex/grid items, table cells, the document root), so
    // they inherit no enclosing floats. The block-flow normal-child recursion in
    // `lay_out_inner` is the one site that propagates a parent `FloatContext`.
    lay_out_cache_checked(
        b, start_x, start_y, available_width, available_height,
        measurer, viewport, pcb, hp, in_block_flow, None,
    );
}

/// Same as [`lay_out`], but resolves `b`'s own used width/height/box-sizing
/// from `used_size_override` instead of from `b.style`'s declared values —
/// see [`UsedSizeOverride`] for why this replaces the old
/// capture-mutate-restore dance around `b.style` (BUG-341 S34).
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_with_used_size(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    used_size_override: UsedSizeOverride,
) {
    lay_out_cache_checked(
        b, start_x, start_y, available_width, available_height,
        measurer, viewport, pcb, hp, in_block_flow, Some(used_size_override),
    );
}


/// CSS 2.1 §9.5 — same as [`lay_out`] but threads `outer_floats`: the float
/// context of an *enclosing* block formatting context, present only when `b` is
/// an in-flow non-BFC block child laid out beside the parent's floats. When set,
/// `b`'s own float context inherits those floats so its (and its descendants')
/// line boxes are shortened by them, instead of the box itself being clipped.
///
/// `parent_justify_items` carries the enclosing block container's `justify-items`
/// value (CSS Box Alignment L3 §6.3), threaded only from the in-flow block-child
/// recursion. When `b`'s own `justify-self` is `auto`, it resolves to this value
/// (the container default); every independent-formatting-context call site passes
/// `AlignValue::Auto`, so those boxes fall back to the inline-start behaviour.
///
/// `used_size_override` — see [`UsedSizeOverride`]; `None` for every call site
/// except `lay_out_with_used_size`'s wrapper (`lay_out_flex`'s re-layout passes).
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_out_inner(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    outer_floats: Option<&FloatContext>,
    parent_justify_items: AlignValue,
    used_size_override: Option<UsedSizeOverride>,
) {
    lay_out_inner_impl(
        b, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp,
        in_block_flow, outer_floats, parent_justify_items, used_size_override,
    );
}


/// Font-relative-unit contexts for one box (CSS Values L4 §5.1.1): the real
/// `ch`/`ex` metrics of its font and its used line-height (`lh`). Restores the
/// parent's values on drop, keeping the thread-locals balanced across the
/// recursive layout walk. Without a measurer the contexts are cleared, so
/// `ch`/`ex`/`lh` fall back to the spec `0.5em`/`1.2em` assumption.
///
/// Trampolines resolve a box's block-size *after* `dispatch_box` has returned,
/// so their `run` entry points re-enter this context for the box they finish.
pub(super) struct FontContext {
    prev_ch_ex: Option<(f32, f32, f32)>,
    prev_lh: Option<f32>,
}

impl FontContext {
    pub(super) fn enter(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> Self {
        let _prof = lumen_core::profile::scope_detail("lo_chex");
        let ch_ex = measurer.map(|m| {
            let fs = b.style.font_size.max(0.0);
            (
                m.char_width_with_families('0', fs, &b.style.font_family),
                m.x_height_px_with_families(fs, &b.style.font_family),
                m.cap_height_px_with_families(fs, &b.style.font_family),
            )
        });
        // `lh`: `used_line_height` is the measurer-resolved value; fall back to
        // the cascaded ratio when unresolved (BUG-1051).
        let lh = measurer.map(|_| {
            if b.used_line_height > 0.0 { b.used_line_height } else { b.style.font_size * b.style.line_height }
        });
        Self {
            prev_ch_ex: crate::style::push_ch_ex_context(ch_ex),
            prev_lh: crate::style::push_lh_context(lh),
        }
    }
}

impl Drop for FontContext {
    fn drop(&mut self) {
        crate::style::pop_ch_ex_context(self.prev_ch_ex);
        crate::style::pop_lh_context(self.prev_lh);
    }
}

/// `rlh`/`rex`/`rch`: metrics of the root element's font, published by the
/// outermost layout call below the (anonymous) document box and held for the
/// whole walk, trampolines included.
struct RootFontGuard(Option<Option<(f32, f32, f32)>>);

impl RootFontGuard {
    fn enter(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> Self {
        // The document node's own box (index 0) is anonymous — its font is the UA
        // default, not `<html>`'s — so measure its first rendered child instead.
        // Block-flow trampolines reach `<html>` through `dispatch_box` directly,
        // so only the call on the document box can hold the guard for the walk.
        let src = if b.node.index() == 0 {
            b.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip))
        } else {
            Some(b)
        };
        Self(match (measurer, src, crate::style::root_font_metrics()) {
            (Some(m), Some(src), None) => {
                let fs = src.style.font_size.max(0.0);
                let lh = if src.used_line_height > 0.0 { src.used_line_height } else { fs * src.style.line_height };
                Some(crate::style::push_root_font_metrics(Some((
                    lh,
                    m.char_width_with_families('0', fs, &src.style.font_family),
                    m.x_height_px_with_families(fs, &src.style.font_family),
                ))))
            }
            _ => None,
        })
    }
}

impl Drop for RootFontGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.0 {
            crate::style::pop_root_font_metrics(prev);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lay_out_inner_impl(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    outer_floats: Option<&FloatContext>,
    parent_justify_items: AlignValue,
    used_size_override: Option<UsedSizeOverride>,
) {
    // LAYOUT-2 срез 1: `dispatch_box` is every dispatch arm this function used
    // to run inline, minus the one non-tail-recursive case (the plain block-flow
    // children loop, CSS 2.1 §9.5/§8.3.1 — float placement + margin collapsing
    // read each child's `.rect` after laying it out, so it cannot be a simple
    // pre-order walk). That case returns `NeedsBlockFlowLoop` instead of
    // recursing, and `block_flow_trampoline::run` drives it — and every further
    // plain-block descendant it meets — on an explicit heap stack instead of
    // the native call stack (`<div>`×20000 no longer overflows it).
    let _root_font = RootFontGuard::enter(b, measurer);
    match dispatch_box(
        b, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp,
        in_block_flow, outer_floats, parent_justify_items, used_size_override,
    ) {
        DispatchOutcome::Done => {}
        DispatchOutcome::NeedsBlockFlowLoop(init) => {
            block_flow_trampoline::run(b, init, measurer, viewport, hp);
        }
        // LAYOUT-2 срез 3: the flex dispatch arm's item-placement loop, same
        // shape as the block-flow case above — see `flex_trampoline::run`.
        DispatchOutcome::NeedsFlexLoop(init) => {
            let _font_ctx = FontContext::enter(b, measurer);
            super::flex_trampoline::run(b, init, measurer, viewport, hp);
        }
        // LAYOUT-2 срез 4: the grid dispatch arm's probe + final-placement
        // passes, same shape as the flex case above — see `grid_trampoline::run`.
        DispatchOutcome::NeedsGridLoop(init) => {
            let _font_ctx = FontContext::enter(b, measurer);
            super::grid_trampoline::run(b, init, measurer, viewport, hp);
        }
        // LAYOUT-2 срез 6: the table dispatch arm's per-cell placement pass,
        // same shape as the flex/grid cases above — see `table_trampoline::run`.
        DispatchOutcome::NeedsTableLoop(init) => {
            let _font_ctx = FontContext::enter(b, measurer);
            super::table_trampoline::run(b, init, measurer, viewport, hp);
        }
        // LAYOUT-2 срез 7: the multicol dispatch arm's per-segment placement
        // pass, same shape as the flex/grid/table cases above — see
        // `multicol_trampoline::run`.
        DispatchOutcome::NeedsMulticolLoop(init) => {
            let _font_ctx = FontContext::enter(b, measurer);
            super::multicol_trampoline::run(b, init, measurer, viewport, hp);
        }
        // LAYOUT-2 срез 8: the vertical-writing-mode dispatch arm's per-child
        // stacking pass, same shape as the multicol case above — see
        // `vertical_trampoline::run`.
        DispatchOutcome::NeedsVerticalLoop(init) => {
            let _font_ctx = FontContext::enter(b, measurer);
            super::vertical_trampoline::run(b, init, measurer, viewport, hp);
        }
    }
}

/// CSS Containment L2 §content-visibility: hidden — "the user agent must lay out the skipped contents
/// when a geometry API asks for them" (BUG-1400). The skipped children of `b` (`b.cv_hidden`) are laid
/// out as the children of a copy of `b` with exactly `b`'s constraints: `content-visibility` itself is
/// dropped from the copy (it would hide them again) and size/layout/paint/style containment stands in
/// for it, so the copy sizes the way the hidden box does. Only the laid-out children are kept — back
/// in `b.cv_hidden`, where `collect_layout_rects` reads them; `b` itself stays a childless box.
#[allow(clippy::too_many_arguments)]
fn lay_out_cv_hidden_probe(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    outer_floats: Option<&FloatContext>,
    parent_justify_items: AlignValue,
    used_size_override: Option<UsedSizeOverride>,
) {
    let _prof = lumen_core::profile::scope_detail("lo_cv_hidden");
    let mut kids = std::mem::take(&mut b.cv_hidden);
    // The skipped boxes may be a previous pass's: lay every one out afresh.
    for k in &mut kids {
        crate::incremental::mark_subtree_dirty(k);
    }
    let mut probe = b.clone();
    probe.children = kids;
    let st = Arc::make_mut(&mut probe.style);
    st.content_visibility = crate::style::ContentVisibility::Visible;
    st.contain = crate::style::ContainFlags(
        st.contain.0
            | (crate::style::ContainFlags::SIZE.0
                | crate::style::ContainFlags::LAYOUT.0
                | crate::style::ContainFlags::PAINT.0
                | crate::style::ContainFlags::STYLE.0),
    );
    lay_out_inner(
        &mut probe, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp,
        in_block_flow, outer_floats, parent_justify_items, used_size_override,
    );
    b.cv_hidden = std::mem::take(&mut probe.children);
}

/// Border-box width of a replaced box from its content/border-box height `h` and ratio —
/// the mirror of `layout_cache::ratio_height` (BUG-1280). Out of line for the frame size.
#[inline(never)]
fn ratio_width(s: &ComputedStyle, h: f32, (aw, ah): (f32, f32), [padding_left, padding_right]: [f32; 2]) -> f32 {
    match s.box_sizing {
        BoxSizing::ContentBox => {
            (h * aw / ah + padding_left + padding_right + s.border_left_width + s.border_right_width).max(0.0)
        }
        BoxSizing::BorderBox => (h * aw / ah).max(0.0),
    }
}

/// BUG-1385: the border-box width of a non-replaced block-level box (block, float, flex/grid
/// container) whose `width` is `auto`, whose `height` is definite and which has a
/// `aspect-ratio`. Inline-level boxes shrink-to-fit elsewhere; tables size by their columns.
///
/// Sizing L4 §5.1: the automatic minimum size (`min-width: auto`) of a box that is not a scroll
/// container still applies, so wider contents make the box wider than the ratio says.
#[inline(never)]
fn block_ratio_width(
    b: &LayoutBox,
    s: &ComputedStyle,
    em: f32,
    available_height: Option<f32>,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    paddings: [f32; 2],
) -> Option<f32> {
    if s.width.is_some()
        || !matches!(s.display, Display::Block | Display::FlowRoot | Display::Flex | Display::Grid)
    {
        return None;
    }
    let (aw, ah) = s.aspect_ratio.filter(|&(aw, ah)| aw > 0.0 && ah > 0.0)?;
    let h = resolve_block_size(s.height.as_ref()?, em, available_height, viewport)?;
    let w = ratio_width(s, h, (aw, ah), paddings);
    if s.min_width.is_none() && matches!(s.overflow_x, Overflow::Visible | Overflow::Clip) {
        Some(w.max(min_content_outer_width_of_contents(b, measurer, viewport)))
    } else {
        Some(w)
    }
}

/// BUG-1280: the border-box width of an image whose only width is its natural size
/// (`build_box` stored it as an intrinsic hint, both axes `auto`), run through the
/// §10.4 table. Kept out of line: `lay_out` recurses per nesting level, and its frame
/// must not grow (`deep_grid_chain_lays_out_without_overflowing_the_stack`).
#[inline(never)]
fn natural_replaced_border_width(
    s: &ComputedStyle,
    em: f32,
    cb: f32,
    available_height: Option<f32>,
    viewport: Size,
    [padding_left, padding_right, padding_top, padding_bottom]: [f32; 4],
) -> Option<f32> {
    if !s.width_is_intrinsic_hint || s.height.is_some() {
        return None;
    }
    let (aw, ah) = s.aspect_ratio.filter(|&(aw, ah)| aw > 0.0 && ah > 0.0)?;
    let Some(Length::Px(w_nat)) = s.width else {
        return None;
    };
    let frame_h = padding_left + padding_right + s.border_left_width + s.border_right_width;
    let frame_v = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;
    let content_w = |l: &Option<Length>| {
        l.as_ref().and_then(|l| l.resolve(em, Some(cb), viewport)).map(|v| match s.box_sizing {
            BoxSizing::ContentBox => v,
            BoxSizing::BorderBox => (v - frame_h).max(0.0),
        })
    };
    let content_h = |l: &Option<Length>| {
        l.as_ref().and_then(|l| resolve_block_size(l, em, available_height, viewport)).map(|v| match s.box_sizing {
            BoxSizing::ContentBox => v,
            BoxSizing::BorderBox => (v - frame_v).max(0.0),
        })
    };
    let w = natural_width_constrained(
        w_nat,
        w_nat * ah / aw,
        content_w(&s.min_width),
        content_w(&s.max_width),
        content_h(&s.min_height),
        content_h(&s.max_height),
    );
    Some(w + frame_h)
}

/// CSS 2.1 §10.4 — the used content width of a replaced element with an intrinsic
/// ratio whose width and height are both `auto`: the natural `w`×`h` pushed through
/// the min-/max- constraint table. `None` bounds are unconstrained. A `min` above its
/// `max` wins (the caller clamps `max` first, then `min`, like the rest of the file).
fn natural_width_constrained(
    w: f32,
    h: f32,
    min_w: Option<f32>,
    max_w: Option<f32>,
    min_h: Option<f32>,
    max_h: Option<f32>,
) -> f32 {
    if w <= 0.0 || h <= 0.0 {
        return w.max(0.0);
    }
    let min_w = min_w.unwrap_or(0.0).max(0.0);
    let min_h = min_h.unwrap_or(0.0).max(0.0);
    let max_w = max_w.map_or(f32::INFINITY, |v| v.max(min_w));
    let max_h = max_h.map_or(f32::INFINITY, |v| v.max(min_h));
    let (wide, narrow) = (w > max_w, w < min_w);
    let (tall, short) = (h > max_h, h < min_h);
    match (wide, narrow, tall, short) {
        (true, _, true, _) => {
            if max_w / w <= max_h / h { max_w } else { (max_h * w / h).max(min_w) }
        }
        (_, true, _, true) => {
            if min_w / w <= min_h / h { (min_h * w / h).min(max_w) } else { min_w }
        }
        (_, true, true, _) => min_w,
        (true, _, _, true) => max_w,
        (true, ..) => max_w,
        (_, true, ..) => min_w,
        (_, _, true, _) => (max_h * w / h).max(min_w),
        (_, _, _, true) => (min_h * w / h).min(max_w),
        _ => w,
    }
}

/// The style a box is laid out with: its own `Arc` when there is no
/// [`UsedSizeOverride`] (the overwhelming majority — an `Arc` bump, not a deep
/// copy, see BUG-341 S12), otherwise a locally cloned `ComputedStyle` with the
/// override applied. `b.style` itself is never touched (BUG-341 S34).
fn style_with_used_size(style: &Arc<ComputedStyle>, used_size_override: Option<UsedSizeOverride>) -> Arc<ComputedStyle> {
    let Some(ov) = used_size_override else {
        return Arc::clone(style);
    };
    let mut owned = (**style).clone();
    // BUG-736: an intrinsic-hint width/height is a presentational fallback for
    // ordinary block/inline layout, not an authored size — a flex item must see
    // `auto` here so its used size comes from this override plus `aspect_ratio`
    // instead of the raw intrinsic pixels.
    if ov.clear_intrinsic_hint {
        if owned.width_is_intrinsic_hint {
            owned.width = None;
        }
        if owned.height_is_intrinsic_hint {
            owned.height = None;
        }
    }
    if let Some(bs) = ov.box_sizing {
        owned.box_sizing = bs;
    }
    if let Some(w) = ov.width {
        owned.width = Some(Length::Px(w));
    }
    if let Some(h) = ov.height {
        owned.height = Some(Length::Px(h));
    }
    Arc::new(owned)
}

/// BUG-1242 — how far a clean box moves to land where a fresh layout at `start_*` would
/// put it, or `None` when its placement is not just `start + margin` (auto inline
/// margins, a non-start `justify-self`, `position: relative`).
fn clean_box_shift(
    b: &LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    viewport: Size,
    used_size_override: Option<&UsedSizeOverride>,
    parent_justify_items: AlignValue,
) -> Option<(f32, f32)> {
    let s = &b.style;
    let effective_justify =
        if matches!(s.justify_self, AlignValue::Auto) { parent_justify_items } else { s.justify_self };
    if s.margin_left.is_auto()
        || s.margin_right.is_auto()
        || matches!(effective_justify, AlignValue::Center | AlignValue::End)
        || matches!(s.position, Position::Relative)
    {
        return None;
    }
    let em = s.font_size;
    let cb = used_size_override.and_then(|ov| ov.percentage_base).unwrap_or(available_width);
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb, viewport);
    Some((start_x + margin_left - b.rect.x, start_y + margin_top - b.rect.y))
}

/// LAYOUT-2 срез 1: the shared tail every `lay_out_inner_impl` dispatch arm
/// falls through to once `b.rect` is final — CSS Positioned Layout L3 §4
/// (absolutely/fixed-positioned children deferred by the block-flow branch,
/// `abs_deferred`; always empty for arms that resolve their own abs children
/// inline, e.g. flex/grid) and §9.4.3 (`position: relative` offset). Extracted
/// verbatim so the explicit-stack block-flow driver (`block_flow_trampoline`)
/// can call it once per finished frame, exactly like the non-block-flow arms
/// that fall through to it today.
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_after_match(
    b: &mut LayoutBox,
    s: &ComputedStyle,
    em: f32,
    cb: f32,
    available_height: Option<f32>,
    is_positioned: bool,
    pcb: Rect,
    abs_deferred: &[(usize, f32, f32)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    // CSS Positioned Layout L3 §4 — абсолютное / фиксированное позиционирование.
    // Деферированные дети (abs_deferred) собраны в Block-ветке выше.
    // Обрабатываем после finalize b.rect.height, чтобы знать высоту containing block.
    if !abs_deferred.is_empty() {
        let my_pcb = if is_positioned {
            // CSS Position L3 §2.2: CB for absolute descendants = padding edge.
            Rect::new(
                b.rect.x + s.border_left_width,
                b.rect.y + s.border_top_width,
                (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            )
        } else {
            pcb
        };
        lay_out_abs_children(b, abs_deferred, measurer, viewport, my_pcb, hp);
    }
    if is_positioned {
        super::multicol_abspos::fix_out_of_flow_descendants(b, measurer, viewport, hp);
    }

    // CSS Positioned Layout L3 §9.4.3 — position: relative — смещение после normal flow.
    if matches!(s.position, Position::Relative) {
        let (off_x, off_y) = relative_offset(s, em, cb, available_height, viewport);
        if off_x != 0.0 || off_y != 0.0 {
            shift_tree(b, off_x, off_y);
        }
    }
    // CSS: position: sticky — treated as normal flow here; inset values (top/right/
    // bottom/left) are resolved from ComputedStyle in sticky_snap.rs::collect_sticky_rec()
    // after this pass. P3 calls collect_sticky_boxes() + compute_sticky_offset() to
    // apply scroll-driven paint transforms at render time.
}

/// Takes the `position: relative` offset that `finish_after_match` applied off
/// `child` again and returns it, so a line-box layout can flow from the
/// unshifted position (§9.4.3) and re-apply the offset once the box is placed.
fn unshift_relative(child: &mut LayoutBox, cb: f32, cb_h: Option<f32>, viewport: Size) -> (f32, f32) {
    let (dx, dy) = relative_offset(&child.style, child.style.font_size, cb, cb_h, viewport);
    if dx != 0.0 || dy != 0.0 {
        shift_tree(child, -dx, -dy);
    }
    (dx, dy)
}

/// CSS Positioned Layout L3 §9.4.3 — смещение `position: relative` от `left/right/top/bottom`.
/// Бокс уже сдвинут на это значение (`shift_tree` в конце раскладки), а родительский поток
/// обязан отсчитывать курсор от несмещённой позиции — §9.4.3 «не влияет на соседей».
///
/// `left`/`right` в процентах считаются от ширины `cb`, `top`/`bottom` — от высоты `cb_h`
/// содержащего блока; при неопределённой высоте (`None`, `height: auto`) процент
/// вычисляется в `auto` (CSS Positioned Layout L3 §3.1.1).
pub(crate) fn relative_offset(s: &ComputedStyle, em: f32, cb: f32, cb_h: Option<f32>, viewport: Size) -> (f32, f32) {
    if !matches!(s.position, Position::Relative) {
        return (0.0, 0.0);
    }
    let resolve = |l: &LengthOrAuto, basis: Option<f32>| match l {
        LengthOrAuto::Length(l) => l.resolve(em, basis, viewport),
        LengthOrAuto::Auto => None,
    };
    let off_x = resolve(&s.left, Some(cb))
        .or_else(|| resolve(&s.right, Some(cb)).map(|r| -r))
        .unwrap_or(0.0);
    let off_y = resolve(&s.top, cb_h)
        .or_else(|| resolve(&s.bottom, cb_h).map(|b| -b))
        .unwrap_or(0.0);
    (off_x, off_y)
}
