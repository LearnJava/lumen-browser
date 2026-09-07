use super::*;
use super::block_flow_trampoline::{self, DispatchOutcome};

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

/// BUG-341 S36 — the layout-result cache's one choke point, shared by
/// [`lay_out`] (`used_size_override: None`) and [`lay_out_with_used_size`]
/// (`used_size_override: Some(..)`, `lay_out_flex`'s three re-layout call
/// sites). Both wrappers pass `outer_floats: None, parent_justify_items:
/// Auto` unconditionally into `lay_out_inner` — the block-flow normal-child
/// recursion is the one `lay_out_inner` call site that threads real
/// floats/justify-items and is therefore never intercepted here, same
/// exclusion S32 established.
#[allow(clippy::too_many_arguments)]
fn lay_out_cache_checked(
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
    used_size_override: Option<UsedSizeOverride>,
) {
    // BUG-802: this wrapper is the one entry point every layout pass starts
    // from, so it is where a pass is delimited for the probe-height memo.
    let _pass = LayoutPassGuard::enter();
    // BUG-341 S40: the same box, at the same origin, with the same inputs,
    // already holds the answer — skip the whole recursive descent instead of
    // recomputing it into a copy. See `LayoutInPlaceKey`'s doc comment for why
    // this is a different mechanism from S32/S36/S38's layout-result cache and
    // not another policy variant of it.
    let in_place_key = (layout_in_place_reuse_enabled()
        && cacheable_for_layout_result_cache(b))
    .then(|| LayoutInPlaceKey {
        node: b.node,
        role: std::mem::discriminant(&b.origin.role),
        start_x_bits: start_x.to_bits(),
        start_y_bits: start_y.to_bits(),
        width_bits: available_width.to_bits(),
        height_bits: available_height.map(f32::to_bits),
        viewport_w_bits: viewport.width.to_bits(),
        viewport_h_bits: viewport.height.to_bits(),
        pcb_x_bits: pcb.x.to_bits(),
        pcb_y_bits: pcb.y.to_bits(),
        pcb_w_bits: pcb.width.to_bits(),
        pcb_h_bits: pcb.height.to_bits(),
        in_block_flow,
        measurer_ptr: measurer
            .map(|m| m as *const dyn TextMeasurer as *const () as usize)
            .unwrap_or(0),
        hp_ptr: hp as *const dyn HyphenationProvider as *const () as usize,
        used_size_override: UsedSizeOverrideBits::from(used_size_override.as_ref()),
    });
    if let Some(key) = in_place_key.as_ref()
        && layout_in_place_hit(b, key)
    {
        return;
    }
    if layout_result_cache_enabled() && cacheable_for_layout_result_cache(b) {
        let key = LayoutResultKey {
            node: b.node,
            width_bits: available_width.to_bits(),
            height_bits: available_height.map(f32::to_bits),
            viewport_w_bits: viewport.width.to_bits(),
            viewport_h_bits: viewport.height.to_bits(),
            pcb_x_bits: pcb.x.to_bits(),
            pcb_y_bits: pcb.y.to_bits(),
            pcb_w_bits: pcb.width.to_bits(),
            pcb_h_bits: pcb.height.to_bits(),
            in_block_flow,
            measurer_ptr: measurer
                .map(|m| m as *const dyn TextMeasurer as *const () as usize)
                .unwrap_or(0),
            hp_ptr: hp as *const dyn HyphenationProvider as *const () as usize,
            used_size_override: UsedSizeOverrideBits::from(used_size_override.as_ref()),
        };
        let hit = LAYOUT_RESULT_CACHE.with(|c| {
            c.borrow().get(&key).and_then(|e| {
                if Arc::ptr_eq(&e.style, &b.style) && crate::incremental::kind_layout_eq(&e.result.kind, &b.kind) {
                    Some((e.result.clone(), e.start_x, e.start_y))
                } else {
                    None
                }
            })
        });
        if let Some((mut result, cached_x, cached_y)) = hit {
            crate::incremental::translate_subtree(&mut result, start_x - cached_x, start_y - cached_y);
            *b = result;
            LAYOUT_RESULT_CACHE_STATS.with(|c| {
                let mut v = c.get();
                v.hits += 1;
                c.set(v);
            });
            return;
        }

        // Cache miss: compute normally, tracking whether the computation
        // touched `content-visibility: auto` anywhere in this subtree (see
        // `CV_AUTO_TOUCHED`'s doc comment).
        let outer_touched = CV_AUTO_TOUCHED.with(|c| c.replace(false));
        lay_out_inner(
            b, start_x, start_y, available_width, available_height,
            measurer, viewport, pcb, hp, in_block_flow, None, AlignValue::Auto,
            used_size_override,
        );
        let touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
        CV_AUTO_TOUCHED.with(|c| c.set(outer_touched || touched_here));
        if !touched_here {
            // BUG-341 S38: `Eager` always materializes (S36's original
            // policy); `Lazy` defers the subtree clone until a key's second
            // sighting confirms style-stability, per `LayoutResultCacheMode`'s
            // doc comment — the clone S37 measured at ~1.7μs/node is wasted
            // on the ~9% of keys this fixture's own census found never
            // recur at all.
            let deferred = match layout_result_cache_mode() {
                LayoutResultCacheMode::Lazy => {
                    let confirmed_repeat = LAYOUT_RESULT_CACHE_SEEN.with(|c| {
                        c.borrow().get(&key).map(|prev_style| Arc::ptr_eq(prev_style, &b.style)).unwrap_or(false)
                    });
                    if confirmed_repeat {
                        LAYOUT_RESULT_CACHE_SEEN.with(|c| {
                            c.borrow_mut().remove(&key);
                        });
                        false
                    } else {
                        LAYOUT_RESULT_CACHE_SEEN.with(|c| {
                            c.borrow_mut().insert(key, Arc::clone(&b.style));
                        });
                        true
                    }
                }
                LayoutResultCacheMode::Eager | LayoutResultCacheMode::Off => false,
            };
            if !deferred {
                LAYOUT_RESULT_CACHE.with(|c| {
                    c.borrow_mut().insert(
                        key,
                        LayoutResultEntry {
                            style: Arc::clone(&b.style),
                            start_x,
                            start_y,
                            result: b.clone(),
                        },
                    );
                });
            }
            LAYOUT_RESULT_CACHE_STATS.with(|c| {
                let mut v = c.get();
                v.misses += 1;
                if deferred {
                    v.deferred += 1;
                }
                c.set(v);
            });
        } else {
            LAYOUT_RESULT_CACHE_STATS.with(|c| {
                let mut v = c.get();
                v.poisoned += 1;
                c.set(v);
            });
        }
        // BUG-341 S40: both mechanisms can be active at once (the cache is
        // `Off` in production, so in practice only this branch's `else` runs) —
        // record the in-place witness on this path too, so enabling the cache
        // for an A/B does not silently disable in-place reuse.
        if let Some(key) = in_place_key {
            record_layout_in_place(b, key, touched_here);
        }
        return;
    }
    // BUG-341 S40: track `content-visibility: auto` across this subtree the
    // same way the cache branch above does — a subtree whose result depends on
    // the scroll offset must not be recorded as reusable. The flag is restored
    // to "outer OR here" so an ancestor's own recording sees it too.
    let outer_cv_touched = CV_AUTO_TOUCHED.with(|c| c.replace(false));
    lay_out_inner(
        b, start_x, start_y, available_width, available_height,
        measurer, viewport, pcb, hp, in_block_flow, None, AlignValue::Auto,
        used_size_override,
    );
    let cv_touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
    CV_AUTO_TOUCHED.with(|c| c.set(outer_cv_touched || cv_touched_here));
    if let Some(key) = in_place_key {
        record_layout_in_place(b, key, cv_touched_here);
    }
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
fn lay_out_inner(
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

/// LAYOUT-2 срез 1: computes `b`'s used height from `content_height` (the block-
/// flow/multicol/table content extent) — CSS 2.1 §10.6.3 explicit height,
/// §10.6.7 aspect-ratio-derived, CSS Box Sizing L4 §5 size-containment fallback,
/// CSS Basic UI L4 §4.4 field-sizing override, and the §10.4 min/max-height
/// clamp. Shared by the plain block-flow branch (dispatched inline before
/// LAYOUT-2, now via the explicit-stack driver in `block_flow_trampoline`) and
/// the multicol branch (`lay_out_multicol_children`'s caller), which both need
/// the exact same finishing sequence applied to two different `content_height`
/// sources. Extracted verbatim — no behavior change from the pre-LAYOUT-2 inline
/// version.
#[allow(clippy::too_many_arguments)]
pub(super) fn finalize_block_height(
    b: &mut LayoutBox,
    s: &ComputedStyle,
    em: f32,
    available_height: Option<f32>,
    viewport: Size,
    padding_top: f32,
    padding_bottom: f32,
    size_contained: bool,
    field_intrinsic: Option<(f32, f32)>,
    content_height: f32,
) {
    // Явная высота (CSS height: Npx) перекрывает авто-высоту по содержимому.
    // box-sizing работает симметрично width: content-box прибавляет
    // padding+border, border-box оставляет h как итоговую высоту.
    b.rect.height = if let Some(h_len) = &s.height {
        if let Some(h) = resolve_block_size(h_len, em, available_height, viewport) {
            let specified = match s.box_sizing {
                BoxSizing::ContentBox => h
                    + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width,
                BoxSizing::BorderBox => h.max(
                    padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width,
                ),
            };
            // CSS 2.1 §17.5.3: the `height` of a table cell is a minimum — the cell
            // grows to fit content taller than the specified height (unlike a regular
            // block, where overflow just spills). Without this the cell clamps to the
            // specified border-box height and content overflows into the inter-row
            // border-spacing gap, so row pitch is short by the overflow amount and the
            // error accumulates down the table (BUG-177).
            if s.display == Display::TableCell {
                let content_box = content_height
                    + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width;
                specified.max(content_box)
            } else {
                specified
            }
        } else {
            content_height + padding_top + padding_bottom
                + s.border_top_width + s.border_bottom_width
        }
    } else if let Some((aw, ah)) = s.aspect_ratio
        && aw > 0.0 && ah > 0.0
    {
        // CSS Sizing L4 §6.1: height auto + aspect-ratio → derive from width.
        // Phase 0: ratio applied in border-box space.
        (b.rect.width * ah / aw).max(0.0)
    } else {
        // CSS Containment L3 §3.3 / CSS Box Sizing L4 §5: size containment
        // suppresses children's contribution to auto height — the box uses
        // contain-intrinsic-height (or 0 when `none`/unset) instead.
        let ch = contained_content_height(size_contained, s, em, viewport, content_height);
        ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
    };
    // CSS Basic UI L4 §4.4 — field-sizing: content height override.
    // When s.height was not set by UA (field_intrinsic is Some), replace the
    // zero content_height with the padding-box height from the measurement.
    if let Some((_, ph)) = field_intrinsic
        && s.height.is_none()
    {
        b.rect.height = ph + s.border_top_width + s.border_bottom_width;
    }
    // CSS 2.1 §10.4: clamp [min-height, max-height]. Симметрия с width: max
    // сначала, потом min → «min побеждает max». Content оверфлоу-ит коробку
    // если min режет ниже — это правильное поведение CSS.
    let outer_vert = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_top + padding_bottom
            + s.border_top_width + s.border_bottom_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_height
        && let Some(max_h) = resolve_block_size(max_len, em, available_height, viewport)
    {
        b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
    }
    if let Some(min_len) = &s.min_height
        && let Some(min_h) = resolve_block_size(min_len, em, available_height, viewport)
    {
        b.rect.height = b.rect.height.max(outer_vert(min_h.max(0.0)));
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
            super::flex_trampoline::run(b, init, measurer, viewport, hp);
        }
        // LAYOUT-2 срез 4: the grid dispatch arm's probe + final-placement
        // passes, same shape as the flex case above — see `grid_trampoline::run`.
        DispatchOutcome::NeedsGridLoop(init) => {
            super::grid_trampoline::run(b, init, measurer, viewport, hp);
        }
    }
}

/// LAYOUT-2 срез 1: `pub(super)` so `block_flow_trampoline` can call it once per
/// child at the explicit-stack driver's one recursion point, exactly where this
/// function's own plain-block branch used to call `lay_out_inner` on each child
/// natively. See [`DispatchOutcome`] for the two possible results.
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_box(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    // CSS 2.1 §10.5: definite content height of the containing block, or None if auto.
    // None means percentage heights on children compute to 'auto'.
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    outer_floats: Option<&FloatContext>,
    parent_justify_items: AlignValue,
    used_size_override: Option<UsedSizeOverride>,
) -> DispatchOutcome {
    // DEVX-8a: `pcb` is the positioned containing block, threaded as a mandatory
    // parameter through every `lay_out`/`lay_out_inner` call — this is the choke
    // point proving "every box resolves a containing block" without a second
    // tree walk. A non-finite `pcb` means a caller propagated a bad rect (e.g.
    // through an unresolved percentage or a NaN from an earlier pass).
    debug_assert!(
        pcb.x.is_finite() && pcb.y.is_finite() && pcb.width.is_finite() && pcb.height.is_finite(),
        "DEVX-8a: non-finite containing block for node={:?}: pcb={:?}",
        b.node,
        pcb
    );
    if matches!(b.kind, BoxKind::Skip) {
        b.rect = Rect::new(start_x, start_y, 0.0, 0.0);
        return DispatchOutcome::Done;
    }

    // EE-3: incremental layout — skip clean subtrees entirely.
    // When INCREMENTAL_LAYOUT_MODE is on and the box has no dirty bits, translate
    // the existing rect to the new (start_x, start_y) without re-running layout.
    // The block-children loop in the parent already advanced child_y using the
    // existing height, so the position is consistent across siblings.
    if INCREMENTAL_LAYOUT_MODE.with(|m| m.get()) && b.dirty.is_clean() {
        let _prof = lumen_core::profile::scope_detail("lo_translate");
        crate::incremental::translate_subtree(b, start_x - b.rect.x, start_y - b.rect.y);
        return DispatchOutcome::Done;
    }

    record_layout_key_occurrence(b.node, start_x, start_y, available_width, available_height, &b.style, used_size_override.as_ref());

    // CSS Values L4 §5.1.1 — publish this box's real `ch`/`ex` metrics (advance of
    // the "0" glyph and the x-height at the used font-size) so `Length::{Ch,Ex}`
    // resolve against the actual font for this box and its descendants. The guard
    // restores the parent's value on every return path, keeping the thread-local
    // balanced across the recursive layout walk. Without a measurer the context is
    // cleared, so ch/ex fall back to the spec `0.5em` assumption.
    struct ChExGuard(Option<(f32, f32)>);
    impl Drop for ChExGuard {
        fn drop(&mut self) {
            crate::style::pop_ch_ex_context(self.0);
        }
    }
    let _ch_ex_guard = {
        let _prof = lumen_core::profile::scope_detail("lo_chex");
        let ch_ex = measurer.map(|m| {
            let fs = b.style.font_size.max(0.0);
            (
                m.char_width_with_families('0', fs, &b.style.font_family),
                m.x_height_px(fs),
            )
        });
        ChExGuard(crate::style::push_ch_ex_context(ch_ex))
    };

    // CSS Containment L3 §4.4 — content-visibility: auto (BB-4). When the box
    // flow position starts below the expanded viewport and the shell hasn't
    // ratcheted the node relevant, drop the children for this pass: the element
    // keeps its own box and paint emits nothing for the subtree. While skipped,
    // the element is size-contained, so its auto block-size collapses to the
    // `contain-intrinsic-height` placeholder (see `size_contained` below). The
    // shell drains `take_cv_skipped()` after layout and emits
    // ContentVisibilityChange events / triggers relayout on scroll.
    // CSS: content-visibility — parsing + ComputedStyle field already wired.
    // BUG-341 S32: `content-visibility: auto`'s skip decision below depends on
    // scroll position/a cross-frame ratchet, neither of which lives in
    // `b.style` — mark this subtree's computation as poisoned for the
    // layout-result cache regardless of which way `cv_should_skip` resolves
    // this time, since a *different* call at a different scroll offset could
    // resolve it the other way from the exact same `(node, constraints,
    // style)` key. See `CV_AUTO_TOUCHED`'s doc comment.
    if b.style.content_visibility == crate::style::ContentVisibility::Auto {
        CV_AUTO_TOUCHED.with(|c| c.set(true));
    }
    let cv_auto_skipped = b.style.content_visibility == crate::style::ContentVisibility::Auto
        && !b.children.is_empty()
        && crate::content_visibility::cv_should_skip(b.node, start_y, viewport.height);
    if cv_auto_skipped {
        b.children.clear();
    }

    // SVG root dispatches to its own layout algorithm: replaced-element sizing
    // from CSS width/height (or viewBox fallback), then SVG-coordinate shape positioning.
    if matches!(b.kind, BoxKind::SvgRoot { .. } | BoxKind::SvgShape { .. } | BoxKind::SvgText { .. }) {
        let _prof = lumen_core::profile::scope_detail("lo_svg");
        // BUG-802: this path reads `available_height` in another function, so
        // the flag cannot be maintained per resolution site here.
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
        lay_out_svg_root(b, start_x, start_y, available_width, available_height, viewport);
        return DispatchOutcome::Done;
    }

    // CSS Writing Modes L3 §3: vertical writing modes swap the block/inline axes.
    // Vertical block stacking and InlineRun flow (below, `lay_out_vertical_inline_run`)
    // are both implemented in the `vertical` module. FormControl and other box
    // kinds inside a vertical context still fall through to horizontal layout.
    // Glyph rotation is a paint concern — CPU rasterizer and wgpu renderer (live
    // default backend, ADR-017) both honor it, including the per-glyph `mixed`
    // CJK-upright/Latin-rotated split; femtovg (fallback backend) does not.
    if !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
    {
        // BUG-802: `available_height` is consumed inside `crate::vertical`,
        // out of reach of `resolve_block_size`'s per-site bookkeeping.
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
        crate::vertical::lay_out_vertical_block(
            b,
            start_x,
            start_y,
            available_width,
            available_height,
            measurer,
            viewport,
            pcb,
            hp,
        );
        return DispatchOutcome::Done;
    }

    // BUG-341 S12: an `Arc` bump, not a 3.2 KB deep copy, on the (overwhelming
    // majority) no-override path. The scope stays because its call count is the
    // honest "boxes fully laid out this pass" counter (`lo_translate`'s count is
    // the ones reused from `prev`), and because a future edit that reintroduces
    // an owned clone here would show up as this line growing from ~0.1 ms back
    // towards the 2.3 ms it was.
    //
    // BUG-341 S34: `used_size_override`, when present, is applied to a locally
    // cloned `ComputedStyle` here instead of being burned into `b.style` — see
    // [`UsedSizeOverride`]. `b.style`'s own `Arc` is never touched by this
    // function, so its pointer identity survives this call unconditionally.
    let s = {
        let _prof = lumen_core::profile::scope_detail("lo_style_ref");
        match used_size_override {
            Some(ov) => {
                let mut owned = (*b.style).clone();
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
            None => Arc::clone(&b.style),
        }
    };
    let em = s.font_size;
    let cb = available_width;

    // CSS Box Sizing L4 §5 — the box is subject to size containment (its size is
    // computed as if it had no contents) when `contain: size` is set, when
    // `content-visibility: hidden` (always skips/contains its subtree), or when
    // `content-visibility: auto` skipped the subtree this pass. Under size
    // containment, auto width/height come from `contain-intrinsic-*` (or 0 when
    // the value is `none`) instead of the content.
    let size_contained = s.contain.0 & crate::style::ContainFlags::SIZE.0 != 0
        || s.content_visibility == crate::style::ContentVisibility::Hidden
        || cv_auto_skipped;

    // Резолвим typed Length-поля с known containing block.
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_right = s.margin_right.resolve_or_zero(em, cb, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb, viewport);

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;
    // Block: auto-ширина = весь доступный inline-размер контейнера.
    // Replaced element (Image): auto-ширина = intrinsic (0 в Phase 0, без
    // декодированных пикселей). Это CSS 2.1 §10.3.2 — replaced-боксы
    // НЕ растягиваются на весь контейнер при отсутствии width.
    // CSS Display L3 §2.4: FormControl (`<button>`, `<select>`) is only
    // "replaced" for sizing while its used `display` keeps the UA-default
    // box type. An author `display: flex`/`grid` blockifies it into a real
    // flex/grid *container* with ordinary box-tree children (e.g. an icon +
    // text `<span>` inside `<button>`) — those children must get auto-width =
    // available space like any other block, not intrinsic-0. Leaving this
    // unconditional made `.ws-add`-style buttons (icon + label, no explicit
    // `width`, in a `flex-direction: column` sidebar) collapse to width 0 and
    // wrap their label onto two lines (BUG-425 item 3) — real browsers don't,
    // because `display: flex` overrides the replaced-sizing default.
    let is_replaced = matches!(b.kind, BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Iframe { .. })
        || (matches!(b.kind, BoxKind::FormControl { .. })
            && !matches!(s.display, Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid));
    // CSS Basic UI L4 §4.4 — field-sizing: content.
    // Pre-compute intrinsic (padding-box width, padding-box height) from text content.
    // Only applies to text-entry FormControls when UA did not supply explicit dimensions.
    let field_intrinsic: Option<(f32, f32)> = if s.field_sizing == FieldSizing::Content
        && is_replaced
        && s.width.is_none()
    {
        if let (BoxKind::FormControl { kind }, Some(m)) = (&b.kind, measurer) {
            let lh = b.used_line_height;
            match kind {
                FormControlKind::Input { value_text, .. } => {
                    Some(field_sizing_content_intrinsic("input", value_text, s.font_size, lh, m))
                }
                FormControlKind::Textarea { value_text } => {
                    Some(field_sizing_content_intrinsic("textarea", value_text, s.font_size, lh, m))
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    b.rect.width = if is_replaced {
        if let Some((pw, _)) = field_intrinsic {
            pw + s.border_left_width + s.border_right_width
        } else if let Some((aw, ah)) = s.aspect_ratio
            && aw > 0.0
            && ah > 0.0
            && s.width.is_none()
            && let Some(h_len) = &s.height
            && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
        {
            // BUG-734 / CSS 2.1 §10.6.2: `width: auto` + definite height +
            // известное соотношение → ширина выводится из высоты. Симметрично
            // ratio-ветке высоты ниже, поэтому считается в border-box
            // пространстве (у картинок padding/border почти всегда нулевые).
            let h_bb = match s.box_sizing {
                BoxSizing::ContentBox => {
                    h + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                }
                BoxSizing::BorderBox => h,
            };
            (h_bb * aw / ah).max(0.0)
        } else {
            0.0
        }
    } else {
        (available_width - margin_left - margin_right).max(0.0)
    };
    // Явная ширина (CSS width: Npx) перекрывает auto-ширину.
    // box-sizing определяет, к какой части бокса относится `width`:
    //   - content-box: width — это размер контента, padding+border прибавляются;
    //   - border-box: width — общий размер вместе с padding+border.
    if let Some(w_len) = &s.width {
        if w_len.is_intrinsic() {
            // CSS Intrinsic Sizing L3 §4 — min-content / max-content / fit-content.
            // max_content_outer_width / min_content_outer_width already include
            // the box's own padding+border (border-box width), so we assign directly.
            let avail_bb = (available_width - margin_left - margin_right).max(0.0);
            b.rect.width = match w_len {
                Length::MaxContent => max_content_outer_width(b, measurer, viewport),
                Length::MinContent => min_content_outer_width(b, measurer, viewport),
                Length::FitContent(max_arg) => {
                    let max_c = max_content_outer_width(b, measurer, viewport);
                    if let Some(arg) = max_arg {
                        // fit-content(<length>) = min(avail, max(min-content, arg))
                        let min_c = min_content_outer_width(b, measurer, viewport);
                        let arg_px = arg.resolve(em, Some(cb), viewport).unwrap_or(avail_bb);
                        // arg_px is a content-box length; convert to border-box:
                        let arg_bb = match s.box_sizing {
                            BoxSizing::ContentBox => arg_px + padding_left + padding_right
                                + s.border_left_width + s.border_right_width,
                            BoxSizing::BorderBox => arg_px,
                        };
                        max_c.min(min_c.max(arg_bb)).min(avail_bb)
                    } else {
                        // fit-content = min(available, max-content)
                        max_c.min(avail_bb)
                    }
                }
                _ => unreachable!(),
            };
        } else if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
            b.rect.width = match s.box_sizing {
                BoxSizing::ContentBox => (w + padding_left + padding_right
                    + s.border_left_width + s.border_right_width).max(0.0),
                BoxSizing::BorderBox => w.max(padding_left + padding_right + s.border_left_width + s.border_right_width),
            };
        }
    }
    // CSS 2.1 §10.4: tentative width → clamp в [min-width, max-width].
    // Intrinsic keywords in min-/max- also resolve to intrinsic values here.
    // Порядок «max сначала, потом min» автоматически даёт правило
    // «при min > max побеждает min». min-/max- интерпретируются в той же
    // box-sizing модели, что и width: content-box добавляет padding+border,
    // border-box оставляет как есть.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_left + padding_right
            + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(b, measurer, viewport))
        } else {
            max_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(max_w) = max_bb {
            b.rect.width = b.rect.width.min(max_w);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(b, measurer, viewport))
        } else {
            min_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(min_w) = min_bb {
            b.rect.width = b.rect.width.max(min_w);
        }
    }
    // Phase 0 shrink-to-fit для atomic inline-level бокса без явной CSS width.
    // Полный алгоритм (CSS 2.1 §10.3.9) требует двух проходов; здесь —
    // упрощение: ищем максимальную explicit-width среди потомков.
    // CSS Box Sizing L4 §5: a size-contained inline-block ignores its content
    // for auto inline-size and uses contain-intrinsic-width (content-box → +pad/
    // border), or 0 when `none`/unset — exactly as if it had no contents.
    //
    // BUG-739: `inline-flex`/`inline-grid` — тот же класс боксов (CSS Display L3
    // §2.1), их auto-ширина тоже shrink-to-fit, а не «весь доступный inline-
    // размер». Без этой ветки inline-flex-кнопка растягивалась бы на всю строку.
    if s.width.is_none()
        && matches!(
            s.display,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        )
    {
        if size_contained {
            let cw = s
                .contain_intrinsic_width
                .as_ref()
                .and_then(|l| l.resolve(em, None, viewport))
                .map_or(0.0, |v| v.max(0.0));
            b.rect.width = (cw + padding_left + padding_right
                + s.border_left_width + s.border_right_width)
                .min(b.rect.width);
        } else if let Some(pref_w) = preferred_inline_block_width(b, measurer, viewport) {
            b.rect.width = pref_w.min(b.rect.width);
        }
    }

    // CSS 2.1 §10.3.3 — auto horizontal-margin centering for block-level
    // non-replaced elements in normal flow with an explicit CSS width.
    // Remaining inline space distributes to auto margins: both auto → equal
    // halves (centered block); only left auto → left takes all remaining;
    // only right auto → no x shift (right margin absorbs remainder silently).
    // Does not apply to: replaced, inline-block, flex/grid containers, floats,
    // or absolute/fixed positioned elements.
    let ml_is_auto = s.margin_left.is_auto();
    let mr_is_auto = s.margin_right.is_auto();
    if (ml_is_auto || mr_is_auto)
        && s.width.is_some()
        && !is_replaced
        && !matches!(
            s.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
        )
        && !matches!(s.float_side, FloatSide::Left | FloatSide::Right)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
    {
        let ml_fixed = if ml_is_auto { 0.0 } else { margin_left };
        let mr_fixed = if mr_is_auto { 0.0 } else { margin_right };
        let remaining = (available_width - b.rect.width - ml_fixed - mr_fixed).max(0.0);
        let ml_computed = if ml_is_auto && mr_is_auto {
            remaining / 2.0
        } else if ml_is_auto {
            remaining
        } else {
            ml_fixed
        };
        b.rect.x = start_x + ml_computed;
    }

    // CSS Box Alignment L3 §5.2 — `justify-self` for block-level boxes in normal
    // flow with a definite inline size and no auto inline margins. Distributes the
    // free inline space (containing block − box margin box) within the containing
    // block: `center` centres, `end` flushes to the inline-end. `start` (and
    // `stretch`/`normal`, whose block-level behaviour is inline-start) leave the box
    // at the inline-start (current behaviour), so pages that don't align are
    // unaffected. Auto margins take precedence (handled above), matching the spec's
    // alignment/margin ordering. Same box class as auto-margin centring:
    // non-replaced block-level in flow.
    //
    // §6.3: `justify-self: auto` resolves to the parent's `justify-items`
    // (`parent_justify_items`, threaded from the in-flow block-child recursion).
    // Independent-formatting-context call sites pass `AlignValue::Auto`, so their
    // boxes keep the inline-start default.
    let effective_justify = if matches!(s.justify_self, AlignValue::Auto) {
        parent_justify_items
    } else {
        s.justify_self
    };
    if !ml_is_auto
        && !mr_is_auto
        && s.width.is_some()
        && matches!(effective_justify, AlignValue::Center | AlignValue::End)
        && !is_replaced
        && !matches!(
            s.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
        )
        && !matches!(s.float_side, FloatSide::Left | FloatSide::Right)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
    {
        let remaining = (available_width - b.rect.width - margin_left - margin_right).max(0.0);
        let shift = match effective_justify {
            AlignValue::Center => remaining / 2.0,
            AlignValue::End => remaining,
            _ => 0.0,
        };
        b.rect.x = start_x + margin_left + shift;
    }

    // CSS Scrollbars L1 §6.2: `scrollbar-gutter: stable both-edges` mirrors the
    // gutter onto the inline-start edge too, so children start `unit` further
    // in — not just narrower, as plain `stable`'s end-edge-only gutter does.
    // Under `direction: rtl`, plain `stable` shifts too — see
    // `scrollbar_gutter_inline_start`'s doc comment.
    let content_x = b.rect.x + padding_left + s.border_left_width + scrollbar_gutter_inline_start(&s);
    let content_y = b.rect.y + padding_top + s.border_top_width;
    let mut content_width = (b.rect.width
        - padding_left - padding_right
        - s.border_left_width - s.border_right_width).max(0.0);
    // CSS Scrollbars L1 §6.2: `scrollbar-gutter: stable` reserves gutter space in
    // layout so content shifts don't occur when the scrollbar track appears.
    content_width = (content_width - scrollbar_gutter_inline(&s)).max(0.0);

    // pcb для потомков: если текущий элемент positioned — он сам CB для абсолютных детей.
    // CSS Containment L3: contain:layout и contain:paint тоже устанавливают containing block.
    // Высота ещё неизвестна, используем 0 — корректируем after layout.
    let is_positioned = !matches!(s.position, Position::Static);
    let contain_establishes_cb = s.contain.0
        & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0 | ContainFlags::STRICT.0) != 0;
    let children_pcb = if is_positioned || contain_establishes_cb {
        // CSS Position L3 §2.2: CB for absolute descendants = padding edge of the element.
        Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            0.0,
        )
    } else {
        pcb
    };

    // Vertical InlineRun layout (Phase 2): text flows top→bottom with
    // glyph rotation handled in paint. Dispatches before the horizontal
    // InlineRun branch so vertical text gets axis-swapped wrapping.
    if !matches!(s.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(b.kind, BoxKind::InlineRun { .. })
    {
        // BUG-802 — see the sibling `lay_out_vertical_block` dispatch above.
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
        crate::vertical::lay_out_vertical_inline_run(
            b,
            start_x,
            start_y,
            available_width,
            available_height,
            measurer,
            viewport,
            pcb,
            hp,
        );
        return DispatchOutcome::Done;
    }

    // InlineRun обрабатывается до основного match.
    if let BoxKind::InlineRun { segments, lines, first_line_style } = &mut b.kind {
        if let Some(m) = measurer {
            // white-space: nowrap / text-wrap-mode: nowrap → infinite max_width so
            // the line-breaker never wraps; word-spacing/letter-spacing logic unchanged.
            let wrap_width = if s.white_space.is_nowrap() || s.text_wrap_mode == TextWrapMode::Nowrap {
                f32::INFINITY
            } else {
                content_width
            };
            let text_indent_px = s.text_indent.resolve_or_zero(em, cb, viewport);
            // UAX #9 P2–I2 once per paragraph, before any wrapping trial: the
            // result splits segments at embedding-level boundaries, and every
            // re-wrap (::first-line pass B, text-wrap: balance/pretty) must see
            // the same segment list the frags will be mapped back onto.
            // `b.kind` keeps the logical, unsplit segments — resolution is a
            // pure function of them, so a relayout reproduces it exactly.
            let resolved;
            let segments: &[InlineSegment] =
                if crate::bidi::needs_resolution(segments, s.direction) {
                    resolved = crate::bidi::resolve(segments, s.direction);
                    &resolved
                } else {
                    segments
                };
            *lines = if let Some(fls) = first_line_style.as_deref() {
                // CSS Pseudo-elements L4 §3.1 — ::first-line layout split (BB-1).
                // Pass A: wrap ALL segments under the ::first-line style to find the
                // true extent of the first formatted line (a larger ::first-line font
                // fits fewer words). Hyphenation is off for this pass: a first line
                // ending mid-word would make the word-level remainder split ambiguous,
                // so the first formatted line never auto-hyphenates (UA freedom).
                let fl_segments: Vec<InlineSegment> = segments
                    .iter()
                    .map(|seg| {
                        let mut fl_seg = seg.clone();
                        if fl_seg.img_src.is_none() {
                            // §3.4: the pseudo-element only supplies what the
                            // segment inherited — an inner `<b>`/`<em>` keeps its
                            // own metrics, so pass A measures the real glyphs.
                            fl_seg.style =
                                crate::style::merge_pseudo_inherited(&seg.style, &s, fls);
                        }
                        fl_seg
                    })
                    .collect();
                let mut lines_a = wrap_inline_run(
                    &fl_segments, wrap_width, fls.font_size, text_indent_px, viewport,
                    m, Hyphens::None, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                );
                if lines_a.len() <= 1 {
                    // Everything fits the first formatted line; ::first-line covers it all.
                    lines_a
                } else {
                    // Pass B: re-wrap the content NOT consumed by line 0 under the base
                    // style (its own font metrics, no text-indent — indent is first-line only).
                    let line0 = lines_a.remove(0);
                    let (_, rest_segs) = split_segments_at_first_line(
                        segments, &line0, s.white_space.preserves_whitespace(),
                    );
                    let raw_rest = wrap_inline_run(
                        &rest_segs, wrap_width, s.font_size, 0.0, viewport,
                        m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                    );
                    let rest = if wrap_width.is_finite() {
                        match s.text_wrap_style {
                            TextWrapStyle::Balance => balance_wrap(
                                &rest_segs, wrap_width, raw_rest, s.font_size, 0.0,
                                viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                            ),
                            TextWrapStyle::Pretty => pretty_wrap(
                                &rest_segs, wrap_width, raw_rest, s.font_size, 0.0,
                                viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                            ),
                            TextWrapStyle::Auto | TextWrapStyle::Stable => raw_rest,
                        }
                    } else {
                        raw_rest
                    };
                    let mut all = Vec::with_capacity(1 + rest.len());
                    all.push(line0);
                    all.extend(rest);
                    all
                }
            } else {
                let raw_lines = wrap_inline_run(segments, wrap_width, s.font_size, text_indent_px, viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break);
                // CSS Text L4 §6.4.2: apply text-wrap-style post-processing only when
                // wrapping is active (wrap_width is finite) and text actually wraps.
                if wrap_width.is_finite() {
                    match s.text_wrap_style {
                        TextWrapStyle::Balance => balance_wrap(
                            segments, wrap_width, raw_lines, s.font_size, text_indent_px,
                            viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                        ),
                        TextWrapStyle::Pretty => pretty_wrap(
                            segments, wrap_width, raw_lines, s.font_size, text_indent_px,
                            viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                        ),
                        // Auto / Stable: greedy result unchanged.
                        // Stable stability is about incremental editing; for static layout it's identical to auto.
                        TextWrapStyle::Auto | TextWrapStyle::Stable => raw_lines,
                    }
                } else {
                    raw_lines
                }
            };
            align_lines(lines, content_width, s.text_align, s.text_align_last, s.direction);
            // CSS Rhythmic Sizing L1 §2 — round each line box up to a multiple of line-height-step.
            let line_h = step_line_height(b.used_line_height, s.line_height_step);
            apply_inline_vertical_align(lines, line_h);
            // CSS Overflow L4 §3.2: -webkit-line-clamp / line-clamp — multi-line truncation.
            // Takes priority over text-overflow:ellipsis (both cannot apply simultaneously).
            if let Some(n) = s.line_clamp.filter(|&n| n > 0) {
                apply_line_clamp(lines, n, content_width, s.font_size, m);
            } else if s.text_overflow == TextOverflow::Ellipsis
                && (s.overflow_x != Overflow::Visible || s.overflow_y != Overflow::Visible)
            {
                // CSS UI L4 §10.1: text-overflow: ellipsis требует overflow != visible.
                apply_text_overflow_ellipsis(lines, content_width, s.font_size, m);
            }
        } else {
            *lines = one_line_fallback(segments);
        }
        // CSS Pseudo-elements L4 §3.1: ::first-line applies to the first formatted line.
        // Mark frags on lines[0] and apply pre-computed ::first-line style override.
        if let Some(first_line) = lines.first_mut() {
            for frag in first_line.iter_mut() {
                frag.is_first_line = true;
                // §3.4: ::first-line is the *parent* of the first line's content,
                // so it only supplies properties the fragment inherited; an inner
                // `<b>`/`<em>`/`style="color:…"` keeps its own declarations.
                if let Some(fls) = first_line_style {
                    frag.style = crate::style::merge_pseudo_inherited(&frag.style, &s, fls);
                }
            }
        }
        let line_count = lines.len().max(1);
        // CSS Pseudo-elements L4 §3.1: the first formatted line uses the ::first-line
        // style's own font metrics for its line box height (BB-1).
        // CSS Rhythmic Sizing L1 §2 — line-height-step rounds every line box (incl. ::first-line).
        let step = s.line_height_step;
        b.rect.height = match first_line_style.as_deref() {
            Some(fls) if !lines.is_empty() => {
                step_line_height(fls.font_size * fls.line_height, step)
                    + (line_count - 1) as f32 * step_line_height(b.used_line_height, step)
            }
            _ => line_count as f32 * step_line_height(b.used_line_height, step),
        };
        return DispatchOutcome::Done;
    }

    // Абсолютно-позиционированные дети: (index, static_x, static_y).
    // LAYOUT-2 срез 1: the plain block-flow branch (the one arm that used to
    // push into this) now returns `NeedsBlockFlowLoop` and defers to its own
    // copy inside `BlockFlowInit` — see the doc comment on the `else` branch
    // below. This one stays empty for every dispatch arm that still runs
    // inline (flex/grid resolve their own abs children directly; the rest
    // never had abs-positioned children to defer in the first place).
    let abs_deferred: Vec<(usize, f32, f32)> = Vec::new();

    match &mut b.kind {
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Audio { .. } | BoxKind::Iframe { .. } | BoxKind::FormControl { .. } => {
            // Flex containers dispatch to lay_out_flex before block-flow.
            if matches!(s.display, Display::Flex | Display::InlineFlex) {
                // For row flex, align-content needs the explicit container height (cross axis).
                let flex_explicit_cross = if !matches!(
                    s.flex_direction,
                    FlexDirection::Column | FlexDirection::ColumnReverse
                ) {
                    s.height.as_ref()
                        .and_then(|h| resolve_block_size(h, em, available_height, viewport))
                        .map(|h| match s.box_sizing {
                            BoxSizing::ContentBox => h,
                            BoxSizing::BorderBox => (h - padding_top - padding_bottom
                                - s.border_top_width - s.border_bottom_width)
                                .max(0.0),
                        })
                } else {
                    None
                };
                // CSS Flexbox §9.7: for a column flex container with a definite
                // main (block) size, free space is distributed to flex-grow items.
                // Compute that definite content-box height here so `lay_out_flex`
                // can grow children instead of collapsing them to flex-basis
                // (BUG-104 — `.right-col` children with `flex:1` were height 0).
                let flex_explicit_main = if matches!(
                    s.flex_direction,
                    FlexDirection::Column | FlexDirection::ColumnReverse
                ) {
                    s.height.as_ref()
                        .and_then(|h| resolve_block_size(h, em, available_height, viewport))
                        .map(|h| match s.box_sizing {
                            BoxSizing::ContentBox => h,
                            BoxSizing::BorderBox => (h - padding_top - padding_bottom
                                - s.border_top_width - s.border_bottom_width)
                                .max(0.0),
                        })
                } else {
                    None
                };
                // LAYOUT-2 срез 3: Steps 1–3/justify precompute run here
                // (native — see `build_flex_init`'s doc comment for why), but
                // the item-placement pass (and this container's own height +
                // `flex_abs` children, formerly right here) is deferred to
                // `flex_trampoline::run` so a chain of nested flex containers
                // drives on an explicit heap stack instead of recursing.
                let init = flex::build_flex_init(
                    &mut b.children, &s, content_x, content_y, content_width,
                    flex_explicit_cross, flex_explicit_main, measurer, viewport, children_pcb, hp,
                    em, available_height, padding_top, padding_bottom, size_contained, is_positioned, pcb,
                );
                return DispatchOutcome::NeedsFlexLoop(init);
            }
            // Grid containers dispatch to lay_out_grid before block-flow.
            if matches!(s.display, Display::Grid | Display::InlineGrid) {
                // CSS Box Alignment L3 §5: `align-content` distributes the block-axis
                // free space of the grid container, so the row-axis pass needs the
                // container's *definite* content-box height (None when the height is
                // content-derived — there is no free space to distribute then).
                let grid_definite_height = s.height.as_ref()
                    .and_then(|h| resolve_block_size(h, em, available_height, viewport))
                    .map(|h| match s.box_sizing {
                        BoxSizing::ContentBox => h,
                        BoxSizing::BorderBox => (h - padding_top - padding_bottom
                            - s.border_top_width - s.border_bottom_width)
                            .max(0.0),
                    });
                // LAYOUT-2 срез 4: Steps 1–3 (native — see `build_grid_init`'s
                // doc comment) run here, but the per-item probe and final-
                // placement passes (and this container's own height, formerly
                // right here) are deferred to `grid_trampoline::run` so a
                // chain of nested grid containers drives on an explicit heap
                // stack instead of recursing.
                match grid::build_grid_init(
                    &b.children, &s, content_x, content_y, content_width, grid_definite_height,
                    viewport, children_pcb, em, available_height, padding_top, padding_bottom,
                    size_contained,
                ) {
                    Some(init) => return DispatchOutcome::NeedsGridLoop(init),
                    None => {
                        // No items — same zero-height epilogue `grid_trampoline::
                        // finish_frame` uses for a populated container.
                        super::grid_trampoline::finish_container_height(
                            b, &s, em, available_height, padding_top, padding_bottom,
                            size_contained, viewport, 0.0,
                        );
                        return DispatchOutcome::Done;
                    }
                }
            }
            // Image не имеет flow-детей, поэтому child-цикл просто пуст —
            // объединяем с Block, чтобы общий код width/height/min-max/borders
            // не дублировался. content_height = 0 для Image без явной высоты
            // даёт коробку только из padding+border (что для пустой картинки
            // визуально корректно).
            // CSS 2.1 §10.5: definite content height for children's height percentage resolution.
            // Only available when this element itself has an explicit height.
            let children_available_height: Option<f32> = if let Some(h_len) = &s.height
                && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
            {
                let content_h = match s.box_sizing {
                    BoxSizing::ContentBox => h,
                    BoxSizing::BorderBox => (h - padding_top - padding_bottom
                        - s.border_top_width - s.border_bottom_width).max(0.0),
                };
                // CSS Scrollbars L1 §6.2: reserve the block-axis gutter (space for a
                // horizontal scrollbar at the block-end edge) so `%`-height children
                // don't shift when the scrollbar appears. Symmetric to the inline
                // `content_width -= scrollbar_gutter_inline(&s)` reduction above; the
                // box's own border-box height is unchanged, only the content area seen
                // by children shrinks.
                Some((content_h - scrollbar_gutter_block(&s)).max(0.0))
            } else {
                None
            };
            let content_height = if (s.column_count.is_some() || s.column_width.is_some())
                && !b.children.is_empty()
            {
                lay_out_multicol_children(
                    &mut b.children,
                    content_x, content_y, content_width,
                    &s, em, measurer, viewport, children_pcb, hp,
                    children_available_height,
                )
            } else {
                // CSS 2.1 §9.5 — float context for this block formatting context.
                // A non-BFC block laid out beside an enclosing context's floats
                // inherits them so its line boxes are shortened (it does not own
                // them). A BFC root starts fresh — it never overlaps outer floats.
                let fc = match outer_floats {
                    Some(p) if !establishes_bfc(b) => FloatContext::inheriting(p),
                    _ => FloatContext::new(),
                };
                let container_right = content_x + content_width;
                // CSS 2.1 §8.3.1: this block's top margin collapses with the top margin of
                // its first in-flow block child when nothing separates them — no top border,
                // no top padding, no BFC, and the box is itself a normal in-flow block (not a
                // flex/grid item or document root). In that case the first child's top margin
                // has already been folded into this box's position by the parent loop (via
                // `collapsed_top_margin`), so the child is placed flush at the content top.
                let b_collapses_top = in_block_flow
                    && matches!(b.kind, BoxKind::Block)
                    && !establishes_bfc(b)
                    && padding_top == 0.0
                    && s.border_top_width == 0.0;
                // CSS 2.1 §8.3.1: symmetric to `b_collapses_top` — this block's
                // bottom margin collapses with the bottom margin of its last in-flow
                // block child when nothing separates them: auto height, no bottom
                // padding, no bottom border, no BFC, and the box is a normal in-flow
                // block. In that case the last child's bottom margin escapes out of
                // this box (folded into its own bottom margin by the parent loop via
                // `collapsed_bottom_margin`) instead of inflating the content height.
                let b_collapses_bottom = in_block_flow
                    && matches!(b.kind, BoxKind::Block)
                    && !establishes_bfc(b)
                    && padding_bottom == 0.0
                    && s.border_bottom_width == 0.0
                    && s.height.is_none();
                // LAYOUT-2 срез 1: the child loop (float/marker/clear placement,
                // margin collapsing, the recursive descent) and the post-loop
                // escaped-bottom/float-enclosure epilogue that used to run here
                // inline now live in `block_flow_trampoline`, driven by an
                // explicit heap stack instead of this function's native one —
                // see `DispatchOutcome`'s doc comment. `content_height` is
                // computed there, once the loop finishes; the return below skips
                // the `finalize_block_height`/tail calls below this match, which
                // `block_flow_trampoline::run` invokes itself once the frame (and
                // every plain-block descendant it meets) is done.
                return DispatchOutcome::NeedsBlockFlowLoop(Box::new(block_flow_trampoline::BlockFlowInit {
                    fc,
                    container_right,
                    child_y: content_y,
                    prev_block_mb: 0.0,
                    b_collapses_top,
                    b_collapses_bottom,
                    seen_inflow_child: false,
                    inside_marker_w: 0.0,
                    abs_deferred: Vec::new(),
                    s,
                    em,
                    cb: available_width,
                    content_x,
                    content_y,
                    content_width,
                    children_pcb,
                    children_available_height,
                    is_positioned,
                    pcb,
                    padding_top,
                    padding_bottom,
                    size_contained,
                    field_intrinsic,
                    available_height,
                }));
            };
            finalize_block_height(
                b, &s, em, available_height, viewport,
                padding_top, padding_bottom, size_contained, field_intrinsic, content_height,
            );
        }
        BoxKind::InlineBlockRow => {
            // Двухфазный горизонтальный layout с переносом строк и
            // vertical-align (CSS 2.1 §9.4.3 + §10.8).
            //
            // Фаза 1: расставляем детей по X, группируем в строки.
            // Фаза 2: применяем вертикальное выравнивание внутри каждой строки.
            //
            // IFC strut (CSS §10.8 / верифицировано pixel-diff TEST-11/TEST-12):
            // strut участвует в высоте строки только если в ней есть хотя бы один
            // элемент с vertical-align: baseline (явный или InlineRun). Для строк,
            // где все элементы используют top/bottom/middle, strut не нужен —
            // baseline вообще не задействован (Edge/Blink подтверждено).
            // Strut — content area шрифта ряда БЕЗ half-leading, и это осознанное
            // расхождение со спекой, а не упрощение. `line-height: normal` в этом
            // движке — 1.2em, тогда как у настоящего шрифта это ascent + descent +
            // lineGap, то есть почти ровно content area; добавив half-leading от
            // 1.2em, строка из одних atomic inline становится на ~1.3px выше, чем
            // в Edge, и TEST-02/04/21/56 (ряды пустых inline-block) уходят в FAIL
            // на 0.68 % при пороге 0.5 %. Измерено A/B, IFC-1. Строки с текстом
            // это не задевает: у прогона своё half-leading, и оно всегда больше.
            let strut_descent = measurer.map_or(0.0, |m| {
                m.descent_px_with_families(b.style.font_size, &b.style.font_family)
            });
            let strut_ascent = measurer.map_or(0.0, |m| {
                m.ascent_px_with_families(b.style.font_size, &b.style.font_family)
            });
            // Half x-height of the row's font: locates `vertical-align: middle`
            // relative to the baseline (CSS 2.1 §10.8.1).
            let x_half = measurer.map_or(0.0, |m| m.x_height_px(b.style.font_size)) / 2.0;
            // Метрики каждого ребёнка как участника строки: ascent — от верхней
            // кромки margin box до его базовой линии, descent — остаток margin box
            // под ней. Считаются сразу после раскладки ребёнка, потому что фазе 1
            // нужна итоговая высота строки, чтобы сдвинуть cur_y (CSS 2.1 §10.8).
            let mut metrics: Vec<(f32, f32)> = vec![(0.0, 0.0); b.children.len()];
            // rows: (row_y, above, below, Vec<child_index>)
            let mut rows: Vec<(f32, f32, f32, Vec<usize>)> = Vec::new();
            let mut cur_x = content_x;
            let mut cur_y = content_y;
            let mut row_y = cur_y;
            let mut cur_row: Vec<usize> = Vec::new();
            let mut row_has_baseline = false;
            let mut total_h: f32 = 0.0;

            // CSS 2.1 §10.8 — размер line box: базовая линия ставится так, чтобы
            // вместить всех выровненных по ней участников, после чего top/bottom
            // раздвигают строку в противоположную сторону.
            let line_metrics = |children: &[LayoutBox],
                                metrics: &[(f32, f32)],
                                idxs: &[usize],
                                has_baseline: bool| -> (f32, f32) {
                let (mut above, mut below) = if has_baseline {
                    (strut_ascent, strut_descent)
                } else {
                    (0.0, 0.0)
                };
                for &idx in idxs {
                    let (a, d) = metrics[idx];
                    match inline_v_align(&children[idx]) {
                        VerticalAlign::Baseline => {
                            above = above.max(a);
                            below = below.max(d);
                        }
                        // `middle` совмещает центр бокса с (базовая линия − x/2),
                        // а не с центром line box: высокий top/bottom-участник
                        // уводит базовую линию от середины (BUG-182, TEST-24 row1).
                        VerticalAlign::Middle => {
                            above = above.max((a + d) / 2.0 + x_half);
                            below = below.max((a + d) / 2.0 - x_half);
                        }
                        _ => {}
                    }
                }
                for &idx in idxs {
                    let (a, d) = metrics[idx];
                    let fh = a + d;
                    match inline_v_align(&children[idx]) {
                        VerticalAlign::Top | VerticalAlign::TextTop => below = below.max(fh - above),
                        VerticalAlign::Bottom | VerticalAlign::TextBottom => {
                            above = above.max(fh - below)
                        }
                        _ => {}
                    }
                }
                (above, below)
            };

            for i in 0..b.children.len() {
                // InlineSpace: collapsed whitespace gap — advance cur_x only.
                if matches!(b.children[i].kind, BoxKind::InlineSpace) {
                    let space_w = measurer.map_or(0.0, |m| m.char_width(' ', b.style.font_size));
                    cur_x += space_w;
                    continue;
                }
                let is_run = matches!(b.children[i].kind, BoxKind::InlineRun { .. });
                // Схлопнутый пробел в начале текста существует только пока текст
                // не первый на строке: `wrap_inline_run` срезает его как пробел в
                // начале строки, поэтому зазор после atomic inline даёт этот сдвиг.
                let lead = if is_run && cur_x > content_x {
                    inline_run_lead_space(&b.children[i], measurer)
                } else {
                    0.0
                };
                // Snap inline-block x to integer CSS pixels (Chrome/Edge behaviour at DPR=1).
                // InlineSpace uses float advance (font metrics); accumulated sub-pixel error
                // would shift all subsequent elements by up to 1px relative to Edge.
                let place_x = if is_run { cur_x + lead } else { cur_x.floor() };
                let child_avail = if is_run {
                    (content_width - (place_x - content_x)).max(0.0)
                } else {
                    content_width
                };
                lay_out(&mut b.children[i], place_x, cur_y, child_avail, None, measurer, viewport, children_pcb, hp, false);
                if matches!(b.children[i].kind, BoxKind::Skip) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let child_mr = b.children[i].style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                let child_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let child_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                // Продвижение по строке: у текста — по последней строке прогона,
                // у остальных — по border box (см. `inline_run_advance`).
                let mut advance = if is_run {
                    inline_run_advance(&b.children[i], measurer)
                } else {
                    b.children[i].rect.width
                };
                let child_right = b.children[i].rect.x + advance + child_mr;

                if !is_run && child_right > content_x + content_width && cur_x > content_x {
                    let (above, below) =
                        line_metrics(&b.children, &metrics, &cur_row, row_has_baseline);
                    rows.push((row_y, above, below, std::mem::take(&mut cur_row)));
                    // Snap to integer CSS pixels (Chrome/Edge DPR=1 behaviour): fractional
                    // IFC strut from font metrics (descent_px) would otherwise drift row
                    // y-positions by sub-pixel amounts relative to a browser with a different
                    // default font.
                    let new_y = (cur_y + above + below).round();
                    let actual_spacing = new_y - cur_y;
                    total_h += actual_spacing;
                    cur_y = new_y;
                    row_y = cur_y;
                    cur_x = content_x;
                    row_has_baseline = false;
                    lay_out(&mut b.children[i], cur_x, cur_y, content_width, None, measurer, viewport, children_pcb, hp, false);
                    advance = b.children[i].rect.width;
                }
                cur_row.push(i);
                if matches!(inline_v_align(&b.children[i]), VerticalAlign::Baseline) {
                    row_has_baseline = true;
                }
                let fh = child_mt + b.children[i].rect.height + child_mb;
                // Нет собственной базовой линии — выравнивание по нижней кромке
                // margin box (CSS 2.1 §10.8.1).
                let asc = match inline_baseline(&b.children[i], measurer) {
                    Some(bl) => child_mt + bl,
                    None => fh,
                };
                metrics[i] = (asc, fh - asc);
                cur_x = b.children[i].rect.x + advance + child_mr;
            }
            let (last_above, last_below) =
                line_metrics(&b.children, &metrics, &cur_row, row_has_baseline);
            if !cur_row.is_empty() {
                rows.push((row_y, last_above, last_below, cur_row));
                b.rect.height = total_h + last_above + last_below;
            } else {
                b.rect.height = total_h;
            }

            // Фаза 2: vertical-align (CSS 2.1 §10.8.1). Дети сейчас стоят border
            // box'ом на верхней кромке строки; сдвигаем каждого туда, куда его
            // ставит его собственное выравнивание.
            let mut adjustments: Vec<(usize, f32)> = Vec::new();
            for (row_top, above, below, child_idxs) in &rows {
                for &idx in child_idxs {
                    let (a, d) = metrics[idx];
                    let fh = a + d;
                    let c_em = b.children[idx].style.font_size;
                    let mt = b.children[idx]
                        .style
                        .margin_top
                        .resolve_or_zero(c_em, content_width, viewport);
                    // Верхняя кромка margin box относительно верха строки.
                    let margin_top_y = match inline_v_align(&b.children[idx]) {
                        VerticalAlign::Baseline => above - a,
                        VerticalAlign::Bottom | VerticalAlign::TextBottom => above + below - fh,
                        VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                        VerticalAlign::Middle => above - x_half - fh / 2.0,
                        _ => 0.0,
                    };
                    let dy = row_top + margin_top_y + mt - b.children[idx].rect.y;
                    if dy.abs() > 0.001 {
                        adjustments.push((idx, dy));
                    }
                }
            }
            for (idx, dy) in adjustments {
                // Round dy to integer CSS pixels so vertical-aligned children land on
                // whole-pixel boundaries, matching the .round() applied to IFC row y-positions
                // above. Fractional dy causes 0.99% deviation vs Edge (BUG-081).
                shift_y_box(&mut b.children[idx], dy.round());
            }
        }
        BoxKind::TableRow => {
            // CSS 2.1 §17.5 — table row: ячейки раскладываются горизонтально.
            // col_widths=None → per-row auto-distribution (standalone <tr> outside <table>).
            let row_h = lay_out_table_row(
                b, content_x, content_y, content_width, None, None, 0.0, None, measurer, viewport, children_pcb, hp,
            );
            b.rect.height = if let Some(h_len) = &s.height
                && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
            {
                match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                }
            } else {
                row_h + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
        }
        BoxKind::Table => {
            // CSS 2.1 §17 / §17.5.2 — table container: compute global column widths, lay out rows.
            // When no explicit CSS width is given, tables use shrink-to-fit: the table box is
            // only as wide as its columns require (total column widths + border-spacing gaps).
            // This differs from block elements which fill the available inline size.
            if s.width.is_none() {
                let intrinsic = table_intrinsic_content_width(b, viewport);
                if intrinsic > 0.0 && intrinsic < content_width {
                    b.rect.width = (intrinsic + padding_left + padding_right
                        + s.border_left_width + s.border_right_width).max(0.0);
                    content_width = intrinsic;
                }
            }
            let content_height = lay_out_table(
                b, content_x, content_y, content_width, measurer, viewport, children_pcb, hp,
            );
            if let Some(h_len) = &s.height
                && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
            {
                b.rect.height = match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                };
            } else if !matches!(s.border_collapse, BorderCollapse::Collapse) {
                // Collapse mode sets b.rect.height directly in lay_out_table (the table border-box
                // coincides with the outer cells' collapsed borders).
                b.rect.height = content_height + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width;
            }
        }
        BoxKind::TableRowGroup => {
            // CSS 2.1 §17 — row group standalone (outside a <table>): block-flow of rows.
            // When inside a Table, rows are handled directly by lay_out_table.
            let mut cur_y = content_y;
            for i in 0..b.children.len() {
                if !matches!(b.children[i].kind, BoxKind::TableRow) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                lay_out(&mut b.children[i], content_x, cur_y + c_mt, content_width, None, measurer, viewport, children_pcb, hp, false);
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb;
            }
            b.rect.height = (cur_y - content_y) + padding_top + padding_bottom
                + s.border_top_width + s.border_bottom_width;
        }
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::InlineSpace => unreachable!(),
        BoxKind::Skip => unreachable!(),
        BoxKind::Contents => unreachable!("display:contents boxes must be flattened before lay_out"),
        BoxKind::Marker { .. } => {
            // Rect is set by the parent's block-flow loop; nothing to do here.
        }
        // SvgRoot, SvgShape, and SvgText are dispatched before this match (early return above).
        BoxKind::SvgRoot { .. } | BoxKind::SvgShape { .. } | BoxKind::SvgText { .. } => unreachable!(),
    }

    finish_after_match(b, &s, em, cb, is_positioned, pcb, &abs_deferred, measurer, viewport, hp);
    DispatchOutcome::Done
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

    // CSS Positioned Layout L3 §9.4.3 — position: relative — смещение после normal flow.
    if matches!(s.position, Position::Relative) {
        let off_x = match &s.left {
            LengthOrAuto::Length(l) => l.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.right {
                LengthOrAuto::Length(r) => -(r.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        let off_y = match &s.top {
            LengthOrAuto::Length(t) => t.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.bottom {
                LengthOrAuto::Length(bot) => -(bot.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        if off_x != 0.0 || off_y != 0.0 {
            shift_tree(b, off_x, off_y);
        }
    }
    // CSS: position: sticky — treated as normal flow here; inset values (top/right/
    // bottom/left) are resolved from ComputedStyle in lib.rs::collect_sticky_rec()
    // after this pass. P3 calls collect_sticky_boxes() + compute_sticky_offset() to
    // apply scroll-driven paint transforms at render time.
}
