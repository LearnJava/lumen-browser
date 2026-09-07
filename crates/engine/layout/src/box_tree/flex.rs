use super::*;

/// An explicit override for a box's own used width/height/box-sizing,
/// threaded through `lay_out_inner` instead of being burned into `b.style`
/// via `Arc::make_mut` and undone afterward — the role `SavedItemSizing`
/// (removed, BUG-341 S34) used to play for `lay_out_flex`'s item re-layout.
///
/// `lay_out_inner` applies the override to a *locally cloned* `ComputedStyle`
/// used only for the duration of that one call (see its `s` binding);
/// `b.style`'s `Arc` is never mutated. This matters beyond avoiding a
/// save/restore dance: BUG-341 S31 found that `SavedItemSizing`'s double
/// `Arc::make_mut` (Step-1 probe never touched it, but the final placement
/// pass did) meant a flex item's `b.style` pointer was *never* stable across
/// two layout passes of the same item — the exact precondition a
/// style-identity-keyed cache would need. With the override applied
/// out-of-place, `b.style` keeps the same `Arc` across both passes whenever
/// nothing else about the item's style changed, restoring that precondition.
///
/// `None` fields leave the corresponding style declaration exactly as
/// authored — only fields the caller explicitly resolved are overridden.
#[derive(Clone, Copy, Default)]
pub(crate) struct UsedSizeOverride {
    /// Resolved width in px (interpreted per `box_sizing`), or `None` to leave
    /// `style.width` as declared.
    pub(crate) width: Option<f32>,
    /// Resolved height in px (interpreted per `box_sizing`), or `None` to leave
    /// `style.height` as declared.
    pub(crate) height: Option<f32>,
    /// Forces `style.box_sizing`, or `None` to leave it as declared. Flex's
    /// column/cross-stretch re-layout passes force `border-box` so the
    /// resolved size (already border-box, per the flexbox algorithm) is used
    /// verbatim instead of having padding+border added on top of it
    /// (BUG-333/BUG-343); its row-direction pass does not, matching what
    /// `SavedItemSizing`'s three call sites each did before this refactor.
    pub(crate) box_sizing: Option<BoxSizing>,
}

/// The **margin-box** cross width a column flex item is laid out at — the value
/// `lay_out_flex` hands `lay_out_inner` as its `available_width`, in both the
/// Step-1 probe and the final placement pass (BUG-341 S41).
///
/// Two things make this one function rather than two copies of the formula.
/// It has to produce bit-identical results at both call sites, since the probe
/// replay's guard is an exact `to_bits` comparison of the two — a re-derivation
/// that rounds differently would silently disable the replay. And it is a
/// margin *box*: `lay_out_inner` treats `available_width` as the space the
/// item's margin box gets and subtracts the item's own margins from it, so
/// handing it the border-box size the alignment arms compute charged those
/// margins twice (a 200px column with `margin: 0 10px` produced a 160px item
/// instead of 180px).
///
/// `s` is the *container's* style — `align-items` is read from it whenever the
/// item leaves `align-self: auto`.
fn column_item_avail_cross(
    item: &LayoutBox,
    s: &ComputedStyle,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let is = &item.style;
    let iem = is.font_size;
    let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
    let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);
    // Поперечная ось колоночного контейнера — ГОРИЗОНТАЛЬ. До 2026-08-17 её не
    // было вовсе: элемент всегда растягивался на всю ширину контейнера, поэтому
    // ни `align-items: center`, ни `margin-left/right: auto` не двигали его с
    // левого края (живой случай — карточка формы входа `tbank.ru/login/`
    // внутри колоночной обёртки страницы).
    let avail_cross = (content_width - m_l - m_r).max(0.0);
    let auto_cross = matches!(is.margin_left, LengthOrAuto::Auto)
        || matches!(is.margin_right, LengthOrAuto::Auto);
    let cross_align = if matches!(is.align_self, AlignValue::Auto) {
        s.align_items
    } else {
        is.align_self
    };
    let aligned_cross = matches!(
        cross_align,
        AlignValue::Start | AlignValue::End | AlignValue::Center
    );
    // Выровненный (не растянутый) элемент занимает по поперечной оси свой
    // fit-content, а не всю ширину — иначе двигать нечего.
    let used_cross = if auto_cross || aligned_cross {
        let max_c = max_content_outer_width(item, measurer, viewport);
        let min_c = min_content_outer_width(item, measurer, viewport);
        max_c.min(avail_cross).max(min_c).min(avail_cross).max(0.0)
    } else {
        avail_cross
    };
    used_cross + m_l + m_r
}

/// CSS Flexbox L1 §9 — multi-line flex layout, Steps 1–3/justify precompute.
///
/// Алгоритм (LAYOUT-2 срез 3 — item-placement пасс вынесен в
/// [`super::flex_trampoline`], см. его doc comment):
/// 1. Для каждого flex-item вычисляем hypothetical main size из flex-basis
///    (Step 1 probe — рекурсия на нативном стеке, вне области среза, как
///    float placement в `block_flow_trampoline`).
/// 2. Разбиваем на flex lines (Step 2).
/// 3. Распределяем free space через flex-grow / flex-shrink и вычисляем
///    justify-content per line (Step 3–5 precompute) — ни то, ни другое не
///    зависит от placement другой линии, поэтому считается для всех линий
///    заранее, в естественном порядке, до входа в трамплин.
///
/// `explicit_cross` — явная высота контейнера (content box) для row flex;
/// используется в align-content для вычисления свободного пространства по cross axis.
///
/// `explicit_main` — определённый main-размер (content box) для column flex
/// (явная `height` или растяжение родителем). `None` = main размер неопределён,
/// тогда контейнер сжимается по содержимому и flex-grow не действует.
///
/// Остальные параметры (`em`..`is_positioned`) — то, что `layout_dispatch.rs`
/// раньше делало со значением, которое `lay_out_flex` возвращал (высота
/// контейнера, `flex_abs`-дети) — теперь это Phase D трамплина
/// (`flex_trampoline::finish_frame`), поэтому едет через `FlexInit`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_flex_init(
    children: &mut [LayoutBox],
    s: &Arc<ComputedStyle>,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    explicit_cross: Option<f32>,
    explicit_main: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    children_pcb: Rect,
    hp: &dyn HyphenationProvider,
    em: f32,
    available_height: Option<f32>,
    padding_top: f32,
    padding_bottom: f32,
    size_contained: bool,
    is_positioned: bool,
    own_pcb: Rect,
) -> Box<super::flex_trampoline::FlexInit> {
    use super::flex_trampoline::FlexInit;
    let is_column = matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
    let is_reverse = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let is_wrap = matches!(s.flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let is_wrap_reverse = matches!(s.flex_wrap, FlexWrap::WrapReverse);

    // Indices of non-Skip children (actual flex items).
    // CSS Flexbox L1 §4.1: an absolutely-positioned child of a flex container does
    // not participate in flex layout — it must not become a flex item nor advance
    // the main-axis cursor. Such children are positioned afterward against the
    // container's content box (see the flex dispatch branch in `lay_out`).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip)
            && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| i)
        .collect();
    // CSS Flexbox L1 §4 — stable sort by `order` (same-order items keep source order).
    item_idxs.sort_by_key(|&i| children[i].style.order);

    // No items: skip straight to a zero-line `FlexInit` — `flex_trampoline::run`
    // reaches `finish_frame` immediately with `content_height == 0.0`, matching
    // the removed `return 0.0` early exit (see that function's doc comment for
    // why constructing `lines = vec![[]]` here instead would NOT match: a
    // single empty line is a real line to Phase A/B, not a no-op).
    if item_idxs.is_empty() {
        return Box::new(FlexInit {
            item_idxs,
            line_inits: Vec::new(),
            ordered_line_idxs: Vec::new(),
            is_column,
            is_reverse,
            is_wrap,
            content_x,
            content_y,
            content_width,
            explicit_cross,
            item_gap: 0.0,
            cross_gap: 0.0,
            s: Arc::clone(s),
            probe_cross: Vec::new(),
            column_probe: Vec::new(),
            probed_main: Vec::new(),
            probe_ran: Vec::new(),
            main_cursor: 0.0,
            cross_cursor: 0.0,
            line_cross_sizes: Vec::new(),
            em,
            available_height,
            padding_top,
            padding_bottom,
            size_contained,
            is_positioned,
            children_pcb,
            own_pcb,
        });
    }

    // Container main size. For row it is always the definite content width. For
    // column it is the definite content height when known (explicit `height` or a
    // parent-imposed stretch — `explicit_main`), otherwise indefinite (auto):
    // the container then sizes to its items and flex-grow has no free space to
    // distribute (CSS Flexbox §9.7).
    let main_definite = if is_column { explicit_main } else { Some(content_width) };
    let container_main = main_definite.unwrap_or(0.0);

    // CSS Box Alignment §8: gap is fixed space between items, subtracted before
    // flex-grow/shrink. `em` is the function parameter (container font-size,
    // same value `layout_dispatch.rs`'s own `let em = s.font_size;` computed
    // from the same `s`) — not re-derived here to avoid a same-value shadow.
    // item_gap: gap between items along the main axis.
    // cross_gap: gap between flex lines along the cross axis (wrap only).
    let item_gap = if is_column {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };
    let cross_gap = if is_column {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };

    // Step 1 — preliminary layout for intrinsic sizes.
    //
    // Only run for items whose `all_hyp` computation below actually reads
    // `item.rect` back: column-direction items always need the item's real
    // content height, and row-direction `auto`/`content` items with no
    // explicit width need `item.rect.width`. Every other combination
    // resolves its main size from the style directly (`FlexBasis::Length`)
    // or from the existing cheap `flex_auto_base_main_width` probe (row,
    // `auto`/`content`, no explicit width) — for those, `item.rect` is never
    // read before the final placement pass below re-lays the item out anyway
    // with its resolved main size. Skipping the unneeded call avoids a full
    // recursive re-layout of the item's whole subtree that nothing reads
    // (BUG-341: every flex item paid for two full recursive layouts instead
    // of one, compounding multiplicatively with flex-nesting depth).
    //
    // BUG-802: skipping the call was only half the story. In a *column*
    // container `flex-basis: auto` — the default — makes the condition above a
    // constant `true`, so every item still paid two full recursive layouts
    // (this probe plus the final placement pass below), and those two multiply
    // down the tree: a chain of nested `flex-direction: column` boxes cost
    // ×2 per level (measured 0.27 s at depth 16, 1.21 s at 18, 4.91 s at 20 —
    // a page with 22-24 levels never finishes). The probe's result is now
    // stashed and replayed by the final pass whenever the two calls would
    // compute the same thing, which collapses the exponent to one layout per
    // level; see `column_probe` below for the three conditions.
    let cb = content_width;
    // BUG-802 — per item (indexed like `item_idxs`): the border-box height the
    // Step-1 probe produced, present only when that probe is replayable. `None`
    // means "lay the item out again in the final pass", which is what every
    // item did unconditionally before this.
    let mut column_probe: Vec<Option<f32>> = vec![None; item_idxs.len()];
    // BUG-802 — the height Step-1 measured for this item, whether it was probed
    // now or served from [`FLEX_COLUMN_PROBE_HEIGHTS`]. `None` for items that
    // were not probed at all (the row direction's usual case), where the
    // hypothetical size comes from the item's style or from
    // `flex_auto_base_main_width` instead.
    let mut probed_main: Vec<Option<f32>> = vec![None; item_idxs.len()];
    // The memo remembers a measurement across calls, so it must stand down
    // wherever an identical call can legitimately measure differently: while a
    // subgrid track context or a container-query basis is installed, neither of
    // which is part of any box's style (the same exclusion
    // `cacheable_for_layout_result_cache` makes for the subgrid half).
    let memo_usable = is_column && !crate::style::cq_context_active();
    // BUG-341 S41 — per item, whether a *real* Step-1 `lay_out` ran for it this
    // call. `column_probe[k] == None` cannot answer that: it is also `None` for
    // an item served from the memo (no layout at all) and the census's whole
    // question is how often the two full layouts happen to the same item.
    // Allocated only while the census is recording — the vector is dead weight
    // on the production path otherwise.
    let census_on = flex_column_census_on();
    let mut probe_ran: Vec<Option<f32>> =
        if census_on { vec![None; item_idxs.len()] } else { Vec::new() };
    // BUG-341 S41 — the margin-box cross width each column item will actually
    // be laid out at, resolved *before* Step 1 so the probe can run at that
    // width instead of at the container's full content width.
    //
    // This is the second half of removing the residual double layout, and the
    // one that reaches the items the first half does not. Cross sizing never
    // depended on the probe: both inputs are intrinsic
    // (`max_content_outer_width`/`min_content_outer_width` read style and
    // contents, never `rect` — the property BUG-802's memo comment already
    // relies on) and the alignment is pure style. Running the probe at the
    // container width and the final pass at a narrower aligned width made the
    // two calls genuinely different, so the replay had to be refused and the
    // subtree laid out twice — 17 of the 21 residual double layouts on the
    // chrome document. Resolving the width first makes them the same call.
    //
    // It is also the more correct measurement: CSS Flexbox §9.2 wants a
    // content-based flex base size measured against the item's *own* cross
    // size, not the container's.
    let mut probe_cross: Vec<f32> =
        if is_column { vec![0.0; item_idxs.len()] } else { Vec::new() };
    if is_column {
        for (k, &i) in item_idxs.iter().enumerate() {
            probe_cross[k] = column_item_avail_cross(
                &children[i], s, content_width, measurer, viewport,
            );
        }
    }
    for (k, &i) in item_idxs.iter().enumerate() {
        let needs_prelayout = {
            let is = &children[i].style;
            if is_column {
                match &is.flex_basis {
                    FlexBasis::Auto | FlexBasis::Content => true,
                    FlexBasis::Length(_) => {
                        is.min_height.is_none() && is.overflow_y == Overflow::Visible
                    }
                }
            } else {
                match &is.flex_basis {
                    FlexBasis::Auto | FlexBasis::Content => is.width.is_some(),
                    FlexBasis::Length(_) => false,
                }
            }
        };
        if needs_prelayout {
            if is_column {
                note_flex_column(|c| c.needed += 1);
            }
            // BUG-341 S41: keyed on the width the probe actually runs at, not
            // the container's — two column containers of different widths, or
            // one item aligned and one stretched, must not collide.
            let probe_width = if is_column { probe_cross[k] } else { content_width };
            let memoized = if memo_usable && cacheable_for_layout_result_cache(&children[i]) {
                let key: FlexProbeKey = (children[i].node, probe_width.to_bits());
                FLEX_COLUMN_PROBE_HEIGHTS.with(|m| {
                    m.borrow().get(&key).and_then(|(style, h)| {
                        Arc::ptr_eq(style, &children[i].style).then_some(*h)
                    })
                })
            } else {
                None
            };
            if let Some(h) = memoized {
                // Nothing else between here and the final placement pass reads
                // the probed *subtree* — `max_content_outer_width`,
                // `min_content_outer_width` and `flex_item_max_main_outer` are
                // all intrinsic (style plus contents, never `rect`) — so the
                // remembered height is the whole of what this probe was for.
                probed_main[k] = Some(h);
                note_flex_column(|c| c.memo_served += 1);
            } else if is_column {
                note_flex_column(|c| c.probed += 1);
                if census_on {
                    probe_ran[k] = Some(probe_width);
                }
                // The two flags are the correctness guard the replay needs: the
                // probe runs with an indefinite containing-block height and at a
                // temporary main-axis position, so a subtree that consulted
                // either (a percentage block size, `content-visibility: auto`'s
                // position-dependent skip) must not be replayed — see
                // `INDEFINITE_HEIGHT_CONSULTED` / `CV_AUTO_TOUCHED`.
                let outer_cv = CV_AUTO_TOUCHED.with(|c| c.replace(false));
                let outer_ih = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.replace(false));
                lay_out(&mut children[i], content_x, content_y, probe_width, None, measurer, viewport, children_pcb, hp, false);
                let cv_here = CV_AUTO_TOUCHED.with(|c| c.get());
                let ih_here = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.get());
                CV_AUTO_TOUCHED.with(|c| c.set(outer_cv || cv_here));
                INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(outer_ih || ih_here));
                probed_main[k] = Some(children[i].rect.height);
                if !cv_here && !ih_here {
                    column_probe[k] = Some(children[i].rect.height);
                }
                // `content-visibility: auto` decides whether to skip a subtree
                // from the scroll offset and a cross-frame ratchet, so its
                // measured height is not a property of the box alone and must
                // not be remembered. The indefinite-height flag is *not* a
                // reason to refuse here, unlike for the replay: both the stored
                // probe and the one being served pass `available_height: None`,
                // so whatever a percentage block size resolved to is the same
                // for each.
                if !cv_here && memo_usable && cacheable_for_layout_result_cache(&children[i]) {
                    let key: FlexProbeKey = (children[i].node, probe_width.to_bits());
                    let entry = (Arc::clone(&children[i].style), children[i].rect.height);
                    FLEX_COLUMN_PROBE_HEIGHTS.with(|m| {
                        m.borrow_mut().insert(key, entry);
                    });
                }
            } else {
                lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, children_pcb, hp, false);
            }
        }
    }

    // Compute hypothetical main sizes for all items (outer = including margins).
    let all_hyp: Vec<f32> = item_idxs
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let item = &children[i];
            // BUG-802: the height Step-1 measured — from the probe just run, or
            // remembered from the identical probe of an earlier pass over this
            // same item. `unwrap_or` covers the items Step-1 never probed.
            let probed_height = probed_main[k].unwrap_or(item.rect.height);
            let is = &item.style;
            let iem = is.font_size;
            let m_l = is.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = is.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
            match &is.flex_basis {
                FlexBasis::Auto | FlexBasis::Content => {
                    if is_column {
                        probed_height + m_t + m_b
                    } else {
                        // CSS Flexbox §9.2/§9.7: for `auto`/`content` flex-basis with no
                        // explicit width, the flex base size is the item's max-content
                        // width, clamped by its own min-width / max-width. Using the
                        // preliminary-pass `item.rect.width` was wrong: a block item
                        // stretches to the full container width there, so a label that
                        // sets only `min-width` and holds short text reported the whole
                        // container width as its base size and was then shrunk down to an
                        // equal share of the row instead of staying at its min-width
                        // (BUG-179, TEST-46 — second column drifted ~160px right).
                        let w = if is.width.is_none() {
                            flex_auto_base_main_width(item, cb, measurer, viewport)
                        } else {
                            item.rect.width
                        };
                        w + m_l + m_r
                    }
                }
                FlexBasis::Length(l) => {
                    let base = l.resolve(iem, Some(cb), viewport).unwrap_or(0.0).max(0.0);
                    if is_column {
                        // CSS Flexbox §4.5: a flex item's automatic minimum size. When
                        // its main-axis `min-height` is `auto` and the block-axis
                        // overflow is `visible`, the item cannot shrink below its
                        // content size suggestion. Without this floor, `flex: 1`
                        // (which sets `flex-basis: 0`) collapses a content-sized item
                        // to height 0 in an indefinite-height column container, so
                        // following siblings paint on top of it (BUG-158, lenta.ru
                        // news cards). `item.rect.height` from the preliminary pass is
                        // the floor: it is the content height, already clamped by any
                        // real explicit `height` (the spec's "specified size suggestion"
                        // cap). We deliberately do NOT skip this when `style.height` is
                        // Some, because flex layout itself writes a resolved px height
                        // back into the item's style (see the `is_column` branch below);
                        // on a re-layout pass that stale value must not disable the
                        // floor and re-collapse the item.
                        let auto_min = if is.min_height.is_none()
                            && is.overflow_y == Overflow::Visible
                        {
                            probed_height
                        } else {
                            0.0
                        };
                        base.max(auto_min) + m_t + m_b
                    } else {
                        base + m_l + m_r
                    }
                }
            }
        })
        .collect();

    // Step 2 — break items into flex lines.
    // Wrap only applies to row direction (column wrapping requires known container height, Phase 0: skip).
    let lines: Vec<Vec<usize>> = if is_wrap && !is_column && container_main > 0.0 {
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut cur_line: Vec<usize> = Vec::new();
        let mut cur_main = 0.0_f32;
        for (k, &item_main) in all_hyp.iter().enumerate() {
            let gap = if cur_line.is_empty() { 0.0 } else { item_gap };
            if !cur_line.is_empty() && cur_main + gap + item_main > container_main {
                lines.push(cur_line);
                cur_line = vec![k];
                cur_main = item_main;
            } else {
                cur_line.push(k);
                cur_main += gap + item_main;
            }
        }
        if !cur_line.is_empty() {
            lines.push(cur_line);
        }
        lines
    } else {
        vec![(0..item_idxs.len()).collect()]
    };

    // Step 3–5 precompute: grow/shrink and justify-content, per line — see
    // `build_line_inits`'s doc comment for why this is safe to do for every
    // line up front, independent of visiting order.
    let n_lines = lines.len();
    let ordered_line_idxs: Vec<usize> = if is_wrap_reverse {
        (0..n_lines).rev().collect()
    } else {
        (0..n_lines).collect()
    };
    let line_inits = build_line_inits(
        &lines, &item_idxs, children, &all_hyp, s, container_main, main_definite,
        item_gap, content_width, measurer, viewport, is_column, is_reverse,
    );

    Box::new(FlexInit {
        item_idxs,
        line_inits,
        ordered_line_idxs,
        is_column,
        is_reverse,
        is_wrap,
        content_x,
        content_y,
        content_width,
        explicit_cross,
        item_gap,
        cross_gap,
        s: Arc::clone(s),
        probe_cross,
        column_probe,
        probed_main,
        probe_ran,
        main_cursor: 0.0,
        cross_cursor: 0.0,
        line_cross_sizes: Vec::with_capacity(n_lines),
        em,
        available_height,
        padding_top,
        padding_bottom,
        size_contained,
        is_positioned,
        children_pcb,
        own_pcb,
    })
}

/// CSS Flexbox L1 §9.7 (grow/shrink) + §9.5 (justify-content within a line) —
/// precomputed for every line in `lines`, in NATURAL order, before
/// `flex_trampoline::run` places a single item. Neither step reads another
/// line's placement (only this line's own item styles and `all_hyp`), so
/// there is no ordering hazard in computing all of them ahead of the
/// (wrap-reverse-sensitive) visiting order `run` uses for the actual
/// placement pass. Copied verbatim from the removed inline per-line loop
/// head, split out of it at the point the removed loop went on to place
/// items (now `flex_trampoline::step_item`).
#[allow(clippy::too_many_arguments)]
fn build_line_inits(
    lines: &[Vec<usize>],
    item_idxs: &[usize],
    children: &[LayoutBox],
    all_hyp: &[f32],
    s: &ComputedStyle,
    container_main: f32,
    main_definite: Option<f32>,
    item_gap: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    is_column: bool,
    is_reverse: bool,
) -> Vec<super::flex_trampoline::FlexLineInit> {
    use super::flex_trampoline::FlexLineInit;
    let cb = content_width;

    lines
        .iter()
        .map(|line_keys| {
            let n = line_keys.len();
            let mut hyp_mains: Vec<f32> = line_keys.iter().map(|&k| all_hyp[k]).collect();

            // Free space after gaps.
            let line_gap_total = if n > 1 { item_gap * (n - 1) as f32 } else { 0.0 };
            let total_hyp: f32 = hyp_mains.iter().sum();
            let free_space = if main_definite.is_some() {
                container_main - total_hyp - line_gap_total
            } else {
                0.0
            };

            if free_space > 0.0 {
                let total_grow: f32 =
                    line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).sum();
                if total_grow > 0.0 {
                    // CSS Flexbox §9.7 шаг 4 «fix min/max violations» — тот же цикл
                    // заморозки, что при сжатии ниже, только потолок здесь
                    // `max-width`/`max-height` элемента.
                    let grows: Vec<f32> =
                        line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).collect();
                    let maxes: Vec<f32> = line_keys
                        .iter()
                        .map(|&k| {
                            flex_item_max_main_outer(&children[item_idxs[k]], cb, viewport, is_column)
                        })
                        .collect();
                    let base: Vec<f32> = hyp_mains.clone();
                    let mut frozen: Vec<bool> = grows.iter().map(|&g| g <= 0.0).collect();
                    // Каждый проход замораживает хотя бы один элемент, поэтому `n`
                    // проходов заведомо хватает.
                    for _ in 0..n {
                        let unfrozen: Vec<usize> = (0..n).filter(|&j| !frozen[j]).collect();
                        if unfrozen.is_empty() {
                            break;
                        }
                        let frozen_sum: f32 =
                            (0..n).filter(|&j| frozen[j]).map(|j| hyp_mains[j]).sum();
                        let unfrozen_base: f32 = unfrozen.iter().map(|&j| base[j]).sum();
                        let remaining = container_main - line_gap_total - frozen_sum - unfrozen_base;
                        let total_weight: f32 = unfrozen.iter().map(|&j| grows[j]).sum();
                        if remaining <= 0.0 || total_weight <= 0.0 {
                            for &j in &unfrozen {
                                hyp_mains[j] = base[j].min(maxes[j]);
                            }
                            break;
                        }
                        let mut violated = false;
                        for &j in &unfrozen {
                            let target = base[j] + remaining * (grows[j] / total_weight);
                            let clamped = target.min(maxes[j]);
                            hyp_mains[j] = clamped;
                            if clamped < target - 0.01 {
                                frozen[j] = true;
                                violated = true;
                            }
                        }
                        if !violated {
                            break;
                        }
                    }
                }
            } else if free_space < 0.0 {
                // CSS Flexbox L1 §9.7 step 4 — «fix min/max violations». See the
                // removed code's comment (BUG-433) for why shrinking needs the
                // same freeze-and-redistribute loop instead of a single pass.
                let mins: Vec<f32> = line_keys
                    .iter()
                    .map(|&k| {
                        let item = &children[item_idxs[k]];
                        if is_column {
                            return 0.0;
                        }
                        let is = &item.style;
                        let iem = is.font_size;
                        let m_l = is.margin_left.resolve_or_zero(iem, cb, viewport);
                        let m_r = is.margin_right.resolve_or_zero(iem, cb, viewport);
                        flex_item_min_main_width(item, cb, measurer, viewport) + m_l + m_r
                    })
                    .collect();
                let shrink: Vec<f32> =
                    line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_shrink).collect();
                let base: Vec<f32> = hyp_mains.clone();
                let mut frozen: Vec<bool> = shrink.iter().map(|&f| f <= 0.0).collect();
                for j in 0..n {
                    if frozen[j] {
                        hyp_mains[j] = base[j].max(mins[j]);
                    }
                }
                for _ in 0..n {
                    let unfrozen: Vec<usize> = (0..n).filter(|&j| !frozen[j]).collect();
                    if unfrozen.is_empty() {
                        break;
                    }
                    let frozen_sum: f32 = (0..n).filter(|&j| frozen[j]).map(|j| hyp_mains[j]).sum();
                    let unfrozen_base: f32 = unfrozen.iter().map(|&j| base[j]).sum();
                    let remaining = container_main - line_gap_total - frozen_sum - unfrozen_base;
                    let total_weight: f32 = unfrozen.iter().map(|&j| shrink[j] * base[j]).sum();
                    if remaining >= 0.0 || total_weight <= 0.0 {
                        for &j in &unfrozen {
                            hyp_mains[j] = base[j].max(mins[j]);
                        }
                        break;
                    }
                    let mut violated = false;
                    for &j in &unfrozen {
                        let target = base[j] + remaining * (shrink[j] * base[j] / total_weight);
                        let clamped = target.max(mins[j]).max(0.0);
                        hyp_mains[j] = clamped;
                        if clamped > target + 0.01 {
                            frozen[j] = true;
                            violated = true;
                        }
                    }
                    if !violated {
                        break;
                    }
                }
            }

            // Justify-content within the line.
            let resolved_main: f32 = hyp_mains.iter().sum();
            let remaining = if main_definite.is_some() {
                (container_main - resolved_main - line_gap_total).max(0.0)
            } else {
                0.0
            };
            // CSS Flexbox §8.1: `margin: auto` на ГЛАВНОЙ оси съедает всё
            // положительное свободное место ДО того, как спрашивают
            // `justify-content` — см. комментарий в удалённом коде (tbank.ru/login/).
            let auto_main: Vec<(bool, bool)> = (0..n)
                .map(|j| {
                    let is = &children[item_idxs[line_keys[j]]].style;
                    if is_column {
                        (
                            matches!(is.margin_top, LengthOrAuto::Auto),
                            matches!(is.margin_bottom, LengthOrAuto::Auto),
                        )
                    } else {
                        (
                            matches!(is.margin_left, LengthOrAuto::Auto),
                            matches!(is.margin_right, LengthOrAuto::Auto),
                        )
                    }
                })
                .collect();
            let auto_main_count =
                auto_main.iter().map(|(a, b)| usize::from(*a) + usize::from(*b)).sum::<usize>();
            let auto_main_share = if auto_main_count > 0 && remaining > 0.0 {
                remaining / auto_main_count as f32
            } else {
                0.0
            };

            let (jc_start, jc_gap) = if auto_main_share > 0.0 {
                // Свободного места уже нет — распределять `justify-content` нечего.
                (0.0, 0.0)
            } else {
                match s.justify_content {
                    AlignValue::End => (remaining, 0.0),
                    AlignValue::Center => (remaining / 2.0, 0.0),
                    AlignValue::SpaceBetween => {
                        if n <= 1 { (0.0, 0.0) } else { (0.0, remaining / (n - 1) as f32) }
                    }
                    AlignValue::SpaceAround => {
                        let per = remaining / n as f32;
                        (per / 2.0, per)
                    }
                    AlignValue::SpaceEvenly => {
                        let per = remaining / (n + 1) as f32;
                        (per, per)
                    }
                    _ => (0.0, 0.0),
                }
            };

            let ordered_keys: Vec<usize> =
                if is_reverse { (0..n).rev().collect() } else { (0..n).collect() };

            FlexLineInit {
                line_keys: line_keys.clone(),
                ordered_keys,
                hyp_mains,
                auto_main,
                jc_start,
                jc_gap,
                auto_main_share,
            }
        })
        .collect()
}
