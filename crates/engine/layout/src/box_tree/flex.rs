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

/// CSS Flexbox L1 §9 — multi-line flex layout.
///
/// Алгоритм:
/// 1. Для каждого flex-item вычисляем hypothetical main size из flex-basis.
/// 2. Распределяем free space через flex-grow / flex-shrink.
/// 3. Раскладываем items с учётом justify-content и align-items.
/// 4. При flex-wrap: apply align-content across flex lines.
///
/// `explicit_cross` — явная высота контейнера (content box) для row flex;
/// используется в align-content для вычисления свободного пространства по cross axis.
///
/// `explicit_main` — определённый main-размер (content box) для column flex
/// (явная `height` или растяжение родителем). `None` = main размер неопределён,
/// тогда контейнер сжимается по содержимому и flex-grow не действует.
///
/// Возвращает `content_height` (вертикальный размер контентной зоны контейнера).
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_flex(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    explicit_cross: Option<f32>,
    explicit_main: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
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

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Container main size. For row it is always the definite content width. For
    // column it is the definite content height when known (explicit `height` or a
    // parent-imposed stretch — `explicit_main`), otherwise indefinite (auto):
    // the container then sizes to its items and flex-grow has no free space to
    // distribute (CSS Flexbox §9.7).
    let main_definite = if is_column { explicit_main } else { Some(content_width) };
    let container_main = main_definite.unwrap_or(0.0);

    // CSS Box Alignment §8: gap is fixed space between items, subtracted before flex-grow/shrink.
    let em = s.font_size;
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
            let memoized = if memo_usable && cacheable_for_layout_result_cache(&children[i]) {
                let key: FlexProbeKey = (children[i].node, content_width.to_bits());
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
            } else if is_column {
                // The two flags are the correctness guard the replay needs: the
                // probe runs with an indefinite containing-block height and at a
                // temporary main-axis position, so a subtree that consulted
                // either (a percentage block size, `content-visibility: auto`'s
                // position-dependent skip) must not be replayed — see
                // `INDEFINITE_HEIGHT_CONSULTED` / `CV_AUTO_TOUCHED`.
                let outer_cv = CV_AUTO_TOUCHED.with(|c| c.replace(false));
                let outer_ih = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.replace(false));
                lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, pcb, hp, false);
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
                    let key: FlexProbeKey = (children[i].node, content_width.to_bits());
                    let entry = (Arc::clone(&children[i].style), children[i].rect.height);
                    FLEX_COLUMN_PROBE_HEIGHTS.with(|m| {
                        m.borrow_mut().insert(key, entry);
                    });
                }
            } else {
                lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, pcb, hp, false);
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

    // Step 3–5: process each line (grow/shrink, justify, position, align).
    // cross_cursor tracks the current cross-axis offset across lines.
    let mut cross_cursor = 0.0_f32;

    let n_lines = lines.len();
    let ordered_line_idxs: Vec<usize> = if is_wrap_reverse {
        (0..n_lines).rev().collect()
    } else {
        (0..n_lines).collect()
    };
    // Track line cross-sizes for align-content.
    let mut line_cross_sizes: Vec<f32> = Vec::with_capacity(n_lines);


    for li in &ordered_line_idxs {
        let line_keys = &lines[*li]; // keys into item_idxs
        let n = line_keys.len();

        // Per-line hyp mains (mutable for grow/shrink).
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
            let total_grow: f32 = line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).sum();
            if total_grow > 0.0 {
                // CSS Flexbox §9.7 шаг 4 «fix min/max violations» — тот же цикл
                // заморозки, что при сжатии ниже, только потолок здесь
                // `max-width`/`max-height` элемента. Без него растущий элемент
                // проезжал свой максимум: раскладка выдавала ему всю ширину
                // строки, свободного места не оставалось (и `justify-content`,
                // и auto-поля получали ноль), а видимая ширина всё равно
                // упиралась в `max-width` при собственной раскладке элемента —
                // отсюда «карточка во всю строку, но нарисована слева».
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
                    let frozen_sum: f32 = (0..n).filter(|&j| frozen[j]).map(|j| hyp_mains[j]).sum();
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
            // CSS Flexbox L1 §9.7 step 4 — «fix min/max violations». Shrinking is not
            // a single proportional pass: every item has a main-axis minimum
            // (§4.5 automatic minimum size for the initial `min-width: auto`), and an
            // item that would be pushed below it is frozen at that minimum while the
            // *remaining* deficit is redistributed over the still-flexible items. The
            // loop is what makes a row of fixed-width items overflow its container
            // instead of collapsing to an equal share of it (BUG-433).
            //
            // Only the row axis gets the floor: the column axis folds its content-size
            // floor into the base size above (see the `is_column` arm of `all_hyp`).
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
                    // `mins` is compared against the *outer* (margin-box) sizes in
                    // `hyp_mains`, so the margins ride along with the floor.
                    flex_item_min_main_width(item, cb, measurer, viewport) + m_l + m_r
                })
                .collect();
            let shrink: Vec<f32> = line_keys
                .iter()
                .map(|&k| children[item_idxs[k]].style.flex_shrink)
                .collect();
            let base: Vec<f32> = hyp_mains.clone();
            // An item with `flex-shrink: 0` never shrinks — it starts out frozen at
            // its base size (still clamped by its own minimum, per step 4).
            let mut frozen: Vec<bool> = shrink.iter().map(|&f| f <= 0.0).collect();
            for j in 0..n {
                if frozen[j] {
                    hyp_mains[j] = base[j].max(mins[j]);
                }
            }
            // Each iteration freezes at least one item, so `n` passes always suffice.
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
                    // Deficit already absorbed by the frozen items (or nothing left that
                    // can absorb it) — the rest keep their base size.
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
        // `justify-content` — поэтому у элемента с `margin-left/right: auto`
        // в строчном контейнере ничего не остаётся на распределение, и он
        // встаёт по центру независимо от `justify-content`. Пока auto здесь
        // резолвился в ноль, такой элемент прижимался к началу строки: живой
        // пример — карточка формы входа `tbank.ru/login/` (`<main>` с
        // `margin: auto` внутри `_PageWrapper` с `space-between`), которая
        // стояла слева вместо центра.
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

        // Final layout: position items along main axis.
        let ordered_keys: Vec<usize> = if is_reverse { (0..n).rev().collect() } else { (0..n).collect() };
        let mut main_cursor = jc_start;

        for &j in &ordered_keys {
            let k = line_keys[j];
            let i = item_idxs[k];
            let outer_main = hyp_mains[j];
            let item_s = children[i].style.clone();
            let iem = item_s.font_size;
            let m_l = item_s.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = item_s.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = item_s.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = item_s.margin_bottom.resolve_or_zero(iem, cb, viewport);
            // Доля auto-полей главной оси: перед элементом — та, что лежит со
            // стороны начала обхода (у reverse-направления это поле конца).
            let (auto_before, auto_after) = if is_reverse {
                (auto_main[j].1, auto_main[j].0)
            } else {
                (auto_main[j].0, auto_main[j].1)
            };
            if auto_before {
                main_cursor += auto_main_share;
            }

            if is_column {
                let inner_main = (outer_main - m_t - m_b).max(0.0);
                // Поперечная ось колоночного контейнера — ГОРИЗОНТАЛЬ. До
                // 2026-08-17 её не было вовсе: элемент всегда растягивался на
                // всю ширину контейнера, поэтому ни `align-items: center`, ни
                // `margin-left/right: auto` не двигали его с левого края
                // (живой случай — карточка формы входа `tbank.ru/login/`
                // внутри колоночной обёртки страницы).
                let avail_cross = (content_width - m_l - m_r).max(0.0);
                let auto_cross_l = matches!(item_s.margin_left, LengthOrAuto::Auto);
                let auto_cross_r = matches!(item_s.margin_right, LengthOrAuto::Auto);
                let cross_align = if matches!(item_s.align_self, AlignValue::Auto) {
                    s.align_items
                } else {
                    item_s.align_self
                };
                let aligned_cross = matches!(
                    cross_align,
                    AlignValue::Start | AlignValue::End | AlignValue::Center
                );
                // Выровненный (не растянутый) элемент занимает по поперечной
                // оси свой fit-content, а не всю ширину — иначе двигать нечего.
                let used_cross = if auto_cross_l || auto_cross_r || aligned_cross {
                    let max_c = max_content_outer_width(&children[i], measurer, viewport);
                    let min_c = min_content_outer_width(&children[i], measurer, viewport);
                    max_c.min(avail_cross).max(min_c).min(avail_cross).max(0.0)
                } else {
                    avail_cross
                };
                // `inner_main` is the item's resolved *border-box* main size (it is
                // derived from the preliminary border-box height and the flex
                // grow/shrink result). Force border-box before re-layout so the value
                // is used verbatim instead of having border+padding added on top of it
                // for a content-box item (which double-counts the border). Mirrors the
                // cross-axis stretch path below.
                // BUG-802: the Step-1 probe above already laid this exact subtree
                // out — at `content_y` instead of `content_y + main_cursor`, with
                // an indefinite height instead of the resolved `inner_main`, and
                // with `content_width` instead of `used_cross`. When the last two
                // differences are *no* difference (the item neither grew nor
                // shrank, and its cross size is the full content width — no auto
                // margin, no `align-self` narrowing it to fit-content), and the
                // probe was clean of the two position/height-sensitive markers,
                // the final pass would recompute the identical subtree. Replay it
                // and move it into place instead: this is what turns the ×2 per
                // nesting level into ×1. Exact bit equality, not an epsilon — an
                // approximate match would replay geometry that differs from what
                // the second layout would have produced.
                let replayable = column_probe[k].is_some_and(|probed| {
                    probed.to_bits() == inner_main.to_bits()
                        && used_cross.to_bits() == content_width.to_bits()
                });
                if replayable {
                    // The shift is the difference between the two calls' *box*
                    // origins, not the bare `main_cursor`: `lay_out_inner` lands
                    // the box at `start_y + margin_top` (BUG-294), so subtracting
                    // the probe's own origin from the final one reproduces its
                    // arithmetic exactly instead of re-associating the sum. The
                    // difference matters: adding `main_cursor` to an already
                    // rounded `content_y + m_t` moved a box at y≈17000 by 0.01 px
                    // against what the second layout would have produced
                    // (`samples/heavy.html`, the one page of the whole
                    // graphic-test corpus where an A/B of the dumps caught it).
                    let dy = ((content_y + main_cursor) + m_t) - (content_y + m_t);
                    shift_tree(&mut children[i], 0.0, dy);
                } else {
                    // BUG-294: pass the item's *margin-box* start (no margin pre-added).
                    // `lay_out_inner` unconditionally adds the box's own `margin_left`/
                    // `margin_top` to the `start_x`/`start_y` it receives, so pre-adding
                    // `m_l`/`m_t` here double-counts the margin. Every other call site in
                    // this file passes the bare margin-box origin and lets `lay_out_inner`
                    // apply the margin once.
                    lay_out_with_used_size(
                        &mut children[i],
                        content_x,
                        content_y + main_cursor,
                        used_cross,
                        Some(inner_main),
                        measurer,
                        viewport,
                        pcb,
                        hp,
                        false,
                        UsedSizeOverride {
                            height: Some(inner_main),
                            box_sizing: Some(BoxSizing::BorderBox),
                            ..Default::default()
                        },
                    );
                }
                // Свободное место поперечной оси достаётся auto-полям, а если
                // их нет — выравниванию (CSS Flexbox §8.1: auto старше
                // `align-self`).
                let free_cross = (avail_cross - children[i].rect.width).max(0.0);
                let cross_shift = if auto_cross_l && auto_cross_r {
                    free_cross / 2.0
                } else if auto_cross_l {
                    free_cross
                } else if auto_cross_r {
                    0.0
                } else {
                    match cross_align {
                        AlignValue::Center => free_cross / 2.0,
                        AlignValue::End => free_cross,
                        _ => 0.0,
                    }
                };
                if cross_shift != 0.0 {
                    shift_tree(&mut children[i], cross_shift, 0.0);
                }
                main_cursor += outer_main + item_gap + jc_gap;
                if auto_after {
                    main_cursor += auto_main_share;
                }
            } else {
                let inner_main = (outer_main - m_l - m_r).max(0.0);
                // BUG-427: `inner_main` is a *border-box* main size — the flex base
                // size comes from `max_content_outer_width`, which already includes
                // the item's own padding+border. Handing it to a content-box item as
                // its used `width` made the re-layout add that padding+border a
                // second time: the item's rect came out `padding_x + border_x` too
                // wide while the main-axis cursor kept advancing by the correct
                // border-box size, so every pair of adjacent padded row items
                // overlapped by exactly that amount (dzen.ru topic tabs, 24 px of
                // padding → chips drawn on top of each other; items with an explicit
                // `width` escaped it because their base size came from style).
                // Converted here rather than by forcing `box_sizing: BorderBox` the
                // way the column arm does — that switch also reinterprets the item's
                // own `height`, which is a *cross*-axis size in this arm and must
                // keep its declared box-sizing (TEST-30's `.box`: 120px + 3px border
                // is 126 tall, not 120).
                let used_main = {
                    let is = &children[i].style;
                    match is.box_sizing {
                        BoxSizing::BorderBox => inner_main,
                        BoxSizing::ContentBox => {
                            let iem = is.font_size;
                            let pl = is.padding_left.resolve_or_zero(iem, cb, viewport);
                            let pr = is.padding_right.resolve_or_zero(iem, cb, viewport);
                            (inner_main - pl - pr
                                - is.border_left_width
                                - is.border_right_width)
                                .max(0.0)
                        }
                    }
                };
                // CSS Flexbox §9.8: percentage cross sizes (e.g. height:100%) resolve
                // against the flex container's definite cross size.
                // BUG-294: margin-box start — `lay_out_inner` adds `m_l`/`m_t` itself
                // (see the column arm above), so pre-adding them here double-counts.
                lay_out_with_used_size(
                    &mut children[i],
                    content_x + main_cursor,
                    content_y + cross_cursor,
                    inner_main,
                    explicit_cross,
                    measurer,
                    viewport,
                    pcb,
                    hp,
                    false,
                    UsedSizeOverride {
                        width: Some(used_main),
                        ..Default::default()
                    },
                );
                main_cursor += outer_main + item_gap + jc_gap;
                if auto_after {
                    main_cursor += auto_main_share;
                }
            }
        }

        // Align-items on cross axis for this line.
        let line_cross: f32 = if is_column {
            0.0 // column cross axis (width) not handled in wrap Phase 0
        } else {
            line_keys.iter().map(|&k| children[item_idxs[k]].rect.height).fold(0.0_f32, f32::max)
        };
        line_cross_sizes.push(line_cross);

        if !is_column {
            // CSS Flexbox §9.5: for a single-line (non-wrapping) flex container the line
            // cross size equals the container's inner cross size (if definite). This lets
            // align-items: center/end position items relative to the full container height
            // rather than just the tallest item in the line.
            let effective_cross = if !is_wrap {
                explicit_cross.unwrap_or(line_cross)
            } else {
                line_cross
            };
            for &k in line_keys {
                let i = item_idxs[k];
                let item = &mut children[i];
                let is = &item.style;
                let iem = is.font_size;
                let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
                let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
                let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
                // CSS Flexbox §8.1: auto-поле ПОПЕРЕЧНОЙ оси съедает свободное
                // место раньше `align-self`/`align-items` (и отменяет stretch):
                // два auto — по центру, одно — прижать к противоположному краю.
                let auto_cross_start = matches!(is.margin_top, LengthOrAuto::Auto);
                let auto_cross_end = matches!(is.margin_bottom, LengthOrAuto::Auto);
                let outer_cross = item.rect.height + m_t + m_b;
                if auto_cross_start || auto_cross_end {
                    let free = (effective_cross - outer_cross).max(0.0);
                    let shift = if auto_cross_start && auto_cross_end {
                        free / 2.0
                    } else if auto_cross_start {
                        free
                    } else {
                        0.0
                    };
                    let new_y = content_y + cross_cursor + m_t + shift;
                    shift_y_box(item, new_y - item.rect.y);
                    continue;
                }
                // The item was laid out at the line's cross-start (`content_y +
                // cross_cursor + m_t`). Cross alignment must move the *whole*
                // subtree, not just `rect.y`: the item's descendants were already
                // positioned in absolute coordinates during the main-axis pass, so
                // shifting only `rect.y` leaves nested content (e.g. an anonymous
                // text item's InlineRun) at the cross-start — BUG-194 (centered
                // digit labels stuck at the box top). Same rationale as BUG-165.
                match align {
                    AlignValue::End => {
                        let new_y = content_y + cross_cursor + effective_cross - outer_cross + m_t;
                        shift_y_box(item, new_y - item.rect.y);
                    }
                    AlignValue::Center => {
                        let new_y = content_y + cross_cursor + m_t + (effective_cross - outer_cross) / 2.0;
                        shift_y_box(item, new_y - item.rect.y);
                    }
                    AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                        // CSS Flexbox §9.5: stretch applies only when the item's cross size
                        // is auto (no explicit height). Items with explicit heights are not
                        // grown beyond their declared size.
                        let stretch_h = if is.height.is_none() {
                            (effective_cross - m_t - m_b).max(0.0)
                        } else {
                            item.rect.height
                        };
                        // BUG-104: a stretched item with no explicit height gains a
                        // definite block size it lacked during its own layout. If the
                        // item is itself a column flex container, its `flex-grow`
                        // children were collapsed to flex-basis against an indefinite
                        // main size — they must be re-laid-out against the stretched
                        // height so they fill it.
                        //
                        // BUG-209: gate the re-layout on a *definite* container cross
                        // size. When `explicit_cross` is None the effective cross size
                        // falls back to `line_cross` (the line's own tallest item), so
                        // the "stretch" is a no-op against the item's current height.
                        // Re-laying-out anyway writes a resolved px `style.height` back
                        // onto the item (below), which permanently clobbers its
                        // `height: auto` state. A later pass that *does* have a definite
                        // cross size then sees `is.height.is_some()` and skips the real
                        // stretch — collapsing nested flex cells to content height
                        // (TEST-90: cell-items stuck at ~40px instead of filling the row).
                        let relayout_column_flex = is.height.is_none()
                            && explicit_cross.is_some()
                            && stretch_h > 0.0
                            && matches!(is.display, Display::Flex | Display::InlineFlex)
                            && matches!(
                                is.flex_direction,
                                FlexDirection::Column | FlexDirection::ColumnReverse
                            );
                        if item.rect.height < stretch_h {
                            item.rect.height = stretch_h;
                        }
                        item.rect.y = content_y + cross_cursor + m_t;
                        if relayout_column_flex {
                            // Force border-box + explicit height so the definite main
                            // size is honoured regardless of the item's own box-sizing,
                            // then re-lay-out in place (origin/width already resolved).
                            let rx = item.rect.x;
                            let ry = item.rect.y;
                            let rw = item.rect.width;
                            lay_out_with_used_size(
                                item, rx, ry, rw, Some(stretch_h), measurer, viewport, pcb, hp, false,
                                UsedSizeOverride {
                                    height: Some(stretch_h),
                                    box_sizing: Some(BoxSizing::BorderBox),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    _ => {
                        item.rect.y = content_y + cross_cursor + m_t;
                    }
                }
            }
        }

        cross_cursor += line_cross + cross_gap;
    }

    // Remove the trailing cross gap accumulated by the loop. Each processed line
    // appends `line_cross + cross_gap` (5225), so after the loop there is always
    // exactly one surplus `cross_gap` — including single-line containers, where the
    // row-gap (from `gap`/`row-gap`) must NOT leak into the container's cross size
    // (nothing to separate). Subtract whenever at least one line was laid out.
    let mut total_cross = if n_lines > 0 {
        (cross_cursor - cross_gap).max(0.0)
    } else {
        cross_cursor
    };

    // Apply align-content to distribute remaining space between flex lines (row wrap only).
    // CSS Box Alignment L3: align-content applies to single-line wrapped containers too
    // (Chrome/Edge 103+ behavior). Removed `n_lines > 1` guard to match browsers.
    if !is_column && is_wrap {
        let line_gap_total = cross_gap * (n_lines.saturating_sub(1)) as f32;
        let used_cross: f32 = line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        let free_cross = explicit_cross.map_or(0.0, |h| (h - used_cross).max(0.0));

        if free_cross > 0.0 {
            let mut line_offsets: Vec<f32> = vec![0.0; n_lines];

            // CSS Box Alignment L3 §5.4: `normal`/`auto` align-content behaves as
            // `stretch` for flex containers. The default (`Auto`) therefore
            // distributes free cross-space by growing each flex line.
            let effective = match s.align_content {
                AlignValue::Auto | AlignValue::Normal => AlignValue::Stretch,
                other => other,
            };

            match effective {
                AlignValue::End => {
                    line_offsets.fill(free_cross);
                }
                AlignValue::Center => {
                    line_offsets.fill(free_cross / 2.0);
                }
                AlignValue::SpaceBetween if n_lines > 1 => {
                    let gap_per = free_cross / (n_lines - 1) as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate().skip(1) {
                        *offset = gap_per * i as f32;
                    }
                }
                AlignValue::SpaceAround => {
                    let per = free_cross / n_lines as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per / 2.0 + (per * i as f32);
                    }
                }
                AlignValue::SpaceEvenly => {
                    let per = free_cross / (n_lines + 1) as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * (i as f32 + 1.0);
                    }
                }
                AlignValue::Stretch => {
                    // CSS Flexbox §8.3: positive free space is split EQUALLY between
                    // all flex lines, increasing each line's cross size. Items on a
                    // later line shift toward the cross-end by the cumulative growth
                    // of all preceding lines (each grown line pushes the next down).
                    let per = free_cross / n_lines as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * i as f32;
                    }
                    for size in line_cross_sizes.iter_mut() {
                        *size += per;
                    }
                }
                _ => {
                }
            }

            for li in 0..n_lines {
                let line_keys = &lines[li];
                let offset = line_offsets[li];

                if !is_column && offset > 0.0 {
                    for &k in line_keys {
                        let i = item_idxs[k];
                        // Shift the whole item subtree, not just its own box: the
                        // item's descendants were already positioned in absolute
                        // coordinates during the flex layout pass, so an
                        // align-content offset must move them in lockstep. Bumping
                        // only `rect.y` would leave the item's content (and any
                        // nested flex lines) behind by `offset` — BUG-165.
                        shift_y_box(&mut children[i], offset);
                    }
                }
            }

            total_cross = line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        }
    }

    if is_column {
        // Column: return main-axis height (main_cursor from last line).
        // Re-compute from stored item positions.
        item_idxs
            .iter()
            .map(|&i| children[i].rect.y + children[i].rect.height - content_y)
            .fold(0.0_f32, f32::max)
    } else {
        total_cross
    }
}
