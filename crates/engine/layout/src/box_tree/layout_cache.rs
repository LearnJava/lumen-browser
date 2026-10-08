//! Layout-result cache choke point (`lay_out_cache_checked`) and the shared
//! block-height finishing sequence (`finalize_block_height`), split out of
//! `layout_dispatch.rs` (SPLIT-LB11, BUG-1100) — a pure move, no logic change.

use super::*;
use super::layout_dispatch::lay_out_inner;

/// BUG-341 S36 — the layout-result cache's one choke point, shared by
/// [`lay_out`] (`used_size_override: None`) and [`lay_out_with_used_size`]
/// (`used_size_override: Some(..)`, `lay_out_flex`'s three re-layout call
/// sites). Both wrappers pass `outer_floats: None, parent_justify_items:
/// Auto` unconditionally into `lay_out_inner` — the block-flow normal-child
/// recursion is the one `lay_out_inner` call site that threads real
/// floats/justify-items and is therefore never intercepted here, same
/// exclusion S32 established.
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_out_cache_checked(
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

/// LAYOUT-2 срез 1: computes `b`'s used height from `content_height` (the block-
/// flow/multicol/table content extent) — CSS 2.1 §10.6.3 explicit height,
/// §10.6.7 aspect-ratio-derived, CSS Box Sizing L4 §5 size-containment fallback,
/// Content-box высота replaced-элемента с `height: N%` при неопределённой
/// базе: intrinsic-соотношение от уже посчитанной ширины, иначе UA-дефолт
/// 150 (iframe/video, HTML LS §15.4.3) или размер холста. `None` — не replaced.
fn replaced_percent_height_as_auto(b: &LayoutBox, s: &ComputedStyle) -> Option<f32> {
    match &b.kind {
        BoxKind::Image { .. } => {
            let (aw, ah) = s.aspect_ratio.filter(|&(aw, ah)| aw > 0.0 && ah > 0.0)?;
            let content_w = b.rect.width
                - s.padding_left.resolve(0.0, None, Size::default()).unwrap_or(0.0)
                - s.padding_right.resolve(0.0, None, Size::default()).unwrap_or(0.0)
                - s.border_left_width
                - s.border_right_width;
            Some((content_w * ah / aw).max(0.0))
        }
        BoxKind::Iframe { .. } | BoxKind::Video { .. } => Some(150.0),
        BoxKind::Canvas { height, .. } => Some(*height as f32),
        _ => None,
    }
}

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
    cb: f32,
) {
    // CSS Sizing L4 §4.1 — `stretch` on the block axis fills the containing
    // block's definite content height minus this box's own vertical margins;
    // `resolve_block_size` yields the full height, the margins come off here.
    // An indefinite height leaves it `auto` (no resolution → content height).
    let stretch_margins = |l: &Length| -> f32 {
        if matches!(l, Length::Stretch) {
            s.margin_top.resolve_or_zero(em, cb, viewport) + s.margin_bottom.resolve_or_zero(em, cb, viewport)
        } else {
            0.0
        }
    };
    // Явная высота (CSS height: Npx) перекрывает авто-высоту по содержимому.
    // box-sizing работает симметрично width: content-box прибавляет
    // padding+border, border-box оставляет h как итоговую высоту.
    b.rect.height = if let Some(h_len) = &s.height {
        if let Some(h) = resolve_block_size(h_len, em, available_height, viewport).map(|h| (h - stretch_margins(h_len)).max(0.0)) {
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
        } else if let Some(auto_h) = replaced_percent_height_as_auto(b, s) {
            // CSS 2.1 §10.5: процентная высота при неопределённой высоте
            // containing block ведёт себя как `auto`; у replaced-элемента
            // это intrinsic-высота (BUG-1227).
            auto_h + padding_top + padding_bottom
                + s.border_top_width + s.border_bottom_width
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
    clamp_min_max_height(b, s, em, available_height, viewport, padding_top, padding_bottom, cb);
}

/// CSS 2.1 §10.4: clamp the border-box height to [min-height, max-height]. Симметрия с width:
/// max сначала, потом min → «min побеждает max». Content оверфлоу-ит коробку если min режет
/// ниже — это правильное поведение CSS. Общий хвост block-, flex- и grid-контейнеров.
#[allow(clippy::too_many_arguments)]
pub(super) fn clamp_min_max_height(
    b: &mut LayoutBox,
    s: &ComputedStyle,
    em: f32,
    available_height: Option<f32>,
    viewport: Size,
    padding_top: f32,
    padding_bottom: f32,
    cb: f32,
) {
    let stretch_margins = |l: &Length| -> f32 {
        if matches!(l, Length::Stretch) {
            s.margin_top.resolve_or_zero(em, cb, viewport) + s.margin_bottom.resolve_or_zero(em, cb, viewport)
        } else {
            0.0
        }
    };
    let outer_vert = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_top + padding_bottom
            + s.border_top_width + s.border_bottom_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_height
        && let Some(max_h) = resolve_block_size(max_len, em, available_height, viewport).map(|h| (h - stretch_margins(max_len)).max(0.0))
    {
        b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
    }
    if let Some(min_len) = &s.min_height
        && let Some(min_h) = resolve_block_size(min_len, em, available_height, viewport).map(|h| (h - stretch_margins(min_len)).max(0.0))
    {
        b.rect.height = b.rect.height.max(outer_vert(min_h.max(0.0)));
    }
}
