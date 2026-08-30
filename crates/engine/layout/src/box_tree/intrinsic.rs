//! Intrinsic (max-content/min-content/shrink-to-fit) width helpers for block
//! and flex layout — `preferred_inline_block_width`/`max_content_outer_width`/
//! `min_content_outer_width` and the flex-item-specific main-size probes
//! (`flex_item_max_main_outer`/`flex_auto_base_main_width`/
//! `flex_item_min_main_width`).
//!
//! Перенесено батчем SPLIT-BT12 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn contributes_to_intrinsic_width` до конца оставшегося региона,
//! перед `mod shapes_floats;`) без правок тел.

use super::*;

/// CSS Intrinsic Sizing L3 §4.1 / CSS 2.1 §10.3.7 — does `c` contribute to its
/// parent's intrinsic (max-content / min-content / shrink-to-fit) width?
///
/// Two kinds of children do not:
/// * `display: none` (`BoxKind::Skip`) — no box is generated at all, so not even
///   the element's own padding/border may be counted;
/// * out-of-flow boxes (`position: absolute`/`fixed`) — they are sized against a
///   containing block, not against their parent's content, and are laid out
///   after it. A nav item holding a hidden 1104px-wide mega-menu dropdown must
///   still be as wide as its label (BUG-738, `tbank.ru` top navigation).
fn contributes_to_intrinsic_width(c: &LayoutBox) -> bool {
    !matches!(c.kind, BoxKind::Skip)
        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
}

/// Is `b` a **row-direction** flex container (`display: flex`/`inline-flex`
/// with `flex-direction: row`/`row-reverse`)?
///
/// Only the row axis matters for intrinsic *width*: a column flex container
/// stacks its items vertically, exactly like a block container, so the existing
/// "widest child" rule is already right for it.
fn is_row_flex_container(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Flex | Display::InlineFlex)
        && !matches!(
            b.style.flex_direction,
            FlexDirection::Column | FlexDirection::ColumnReverse
        )
}

/// CSS Flexbox L1 §9.9 — intrinsic width contribution of a **row-direction**
/// flex container: its items sit side by side on the main axis, so the
/// container's intrinsic width is the *sum* of the items' outer (margin-box)
/// intrinsic widths plus the `column-gap` between them — not the maximum, which
/// is what the block-container rule (children stack vertically) yields.
///
/// `per_item` supplies the caller's own notion of an item's border-box
/// intrinsic width (max-content, min-content or shrink-to-fit preferred);
/// margins and gaps are added here so every caller agrees on them.
///
/// Same class of defect as [BUG-178] for floats: a formatting context whose
/// children are laid out horizontally was being measured with the vertical rule.
///
/// Item selection mirrors `lay_out_flex`: `Skip` boxes and absolutely-positioned
/// children are not flex items (§4.1) and contribute nothing.
///
/// Percentage `column-gap` resolves against the container's own content box,
/// which is exactly what intrinsic sizing does not know yet — it resolves to
/// zero here, consistent with every other percentage in these functions.
fn flex_row_intrinsic_sum(
    b: &LayoutBox,
    viewport: Size,
    per_item: &dyn Fn(&LayoutBox) -> f32,
) -> f32 {
    let gap = b
        .style
        .column_gap
        .resolve(b.style.font_size, Some(0.0), viewport)
        .unwrap_or(0.0)
        .max(0.0);
    let mut sum = 0.0_f32;
    let mut n_items = 0_usize;
    for c in &b.children {
        if !contributes_to_intrinsic_width(c) {
            continue;
        }
        let cem = c.style.font_size;
        let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
        let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
        sum += per_item(c) + ml + mr;
        n_items += 1;
    }
    sum + gap * n_items.saturating_sub(1) as f32
}

/// Phase 0 shrink-to-fit: возвращает «предпочтительную» ширину inline-block-бокса
/// (включая padding+border самого бокса). Алгоритм: если у бокса явная CSS `width` —
/// берём её; иначе рекурсивно ищем максимальную preferred_width среди потомков
/// и добавляем padding+border текущего бокса. Возвращает `None` если явных размеров
/// нет ни у бокса, ни у его потомков.
///
/// Для typed-Length полей используем em = font_size, cb_width = 0 как
/// аппроксимацию (shrink-to-fit не знает cb_width заранее).
pub(crate) fn preferred_inline_block_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    let s = &b.style;
    let em = s.font_size;
    // % ширины на этом этапе не разрешима — трактуем как отсутствие.
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // CSS Sizing L3 §5.2.1 (BUG-742): процентная `width` в intrinsic-контексте
    // неразрешима и ведёт себя как `auto` — вклад считается по содержимому.
    // `percent_basis: None` (а не `Some(0.0)`) — единственное отличие от
    // остальных длин: иначе `width: 100%` давала бы 0 и целиком стирала вклад
    // поддерева, оставляя от бокса только его собственные padding + border.
    if let Some(w_len) = &s.width
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr
                + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return Some(outer.max(0.0));
    }
    // InlineRun — чисто-текстовый анонимный run: preferred = max-content ширина
    // текста (все сегменты на одной строке, без переноса). Без этой ветки
    // text-only inline-block (`<span style="display:inline-block">текст</span>`)
    // получал content_w = 0 (текст лежит в `segments`, а не в `children`) → None
    // → shrink-to-fit не применялся → бокс растягивался на всю доступную ширину
    // вместо обтягивания текста (BUG-202).
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        let text_w = measurer.map_or(0.0, |m| {
            segments
                .iter()
                .map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let ts = seg.style.tab_size
                        * m.char_width_with_families(' ', seg.style.font_size, fams);
                    measure_text_w_families(&seg.text, seg.style.font_size, ls, ts, fams, m)
                })
                .sum()
        });
        return if text_w > 0.0 { Some(text_w) } else { None };
    }
    // InlineBlockRow — горизонтальный поток: суммируем ширины детей + их margins.
    // InlineSpace — collapsed whitespace gap; его ширина = char_width(' ').
    // Остальные боксы (Block, Image и т.д.) — вертикальный поток: берём max.
    let content_w = if is_row_flex_container(b) {
        // Row flex container: items are laid side by side (see
        // `flex_row_intrinsic_sum`). A child with no preference of its own
        // contributes 0, matching the `unwrap_or(0.0)` used for the other
        // horizontal flow below.
        flex_row_intrinsic_sum(b, viewport, &|c| {
            preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0)
        })
    } else if matches!(b.kind, BoxKind::InlineBlockRow) {
        let sum: f32 = b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
            if matches!(c.kind, BoxKind::InlineSpace) {
                // Учитываем ширину collapsed space, чтобы при shrink-to-fit
                // не занижать ширину контейнера и не вызывать перенос соседних
                // inline-block элементов на следующую строку.
                return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
            }
            let cw = preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0);
            let cem = c.style.font_size;
            let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
            let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
            cw + ml + mr
        }).sum();
        sum
    } else {
        // Vertical (block) flow: in-flow children stack, so the container is as
        // wide as its widest child. Floated children, however, are placed side
        // by side on the same line (CSS 2.1 §9.5.1) — their margin-box widths
        // sum. The shrink-to-fit width is the larger of the two contributions.
        let mut inflow_max = 0.0_f32;
        let mut float_sum = 0.0_f32;
        for c in &b.children {
            if !contributes_to_intrinsic_width(c) {
                continue;
            }
            let Some(cw) = preferred_inline_block_width(c, measurer, viewport) else {
                continue;
            };
            if c.style.float_side != FloatSide::None {
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                float_sum += cw + ml.max(0.0) + mr.max(0.0);
            } else {
                inflow_max = inflow_max.max(cw);
            }
        }
        inflow_max.max(float_sum)
    };
    if content_w > 0.0 {
        Some(
            (content_w + pl + pr
                + s.border_left_width + s.border_right_width)
                .max(0.0),
        )
    } else {
        None
    }
}

/// CSS Intrinsic Sizing L3 §4 — max-content border-box width of `b`.
///
/// The max-content width is the width a box would use if line breaking were
/// suppressed: all content on one line. For block containers this is the
/// maximum over children's max-content widths. For `InlineRun` boxes it is
/// the sum of all segment text widths (no wrapping). Includes the box's own
/// padding + border in the returned value (border-box width).
///
/// Phase-0 approximation: only `char_width` per-character measurement is
/// available; inter-word spacing is included, but features like ligatures or
/// kerning are not. Word-break is not applied — text is treated as one run.
pub(crate) fn max_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Explicit non-intrinsic CSS width takes precedence (same logic as
    // preferred_inline_block_width). A percentage width is *not* explicit here:
    // it is unresolvable in an intrinsic context and behaves as `auto`
    // (CSS Sizing L3 §5.2.1, BUG-742) — hence `percent_basis: None`.
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // max-content = all segments on one line (no wrapping).
            measurer.map_or(0.0, |m| {
                segments.iter().map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let ts = seg.style.tab_size
                        * m.char_width_with_families(' ', seg.style.font_size, fams);
                    measure_text_w_families(&seg.text, seg.style.font_size, ls, ts, fams, m)
                }).sum()
            })
        }
        BoxKind::InlineBlockRow => {
            b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).sum()
        }
        // Row flex container: items sit side by side, so max-content is their
        // sum + gaps (CSS Flexbox §9.9). This holds for `flex-wrap: wrap` too —
        // max-content suppresses line breaking, so every item stays on one line.
        _ if is_row_flex_container(b) => {
            flex_row_intrinsic_sum(b, viewport, &|c| {
                max_content_outer_width(c, measurer, viewport)
            })
        }
        _ => {
            // Block container: in-flow children stack vertically → take the
            // widest. Floated children are laid side by side on one line
            // (CSS 2.1 §9.5.1), so their margin-box widths sum. The max-content
            // width is the larger of the in-flow maximum and the float run sum.
            let mut inflow_max = 0.0_f32;
            let mut float_sum = 0.0_f32;
            for c in &b.children {
                if !contributes_to_intrinsic_width(c) {
                    continue;
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                if c.style.float_side != FloatSide::None {
                    let cem = c.style.font_size;
                    let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                    let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                    float_sum += cw + ml.max(0.0) + mr.max(0.0);
                } else {
                    inflow_max = inflow_max.max(cw);
                }
            }
            inflow_max.max(float_sum)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Intrinsic Sizing L3 §4 — min-content border-box width of `b`.
///
/// The min-content width is the narrowest a box can be without overflowing:
/// the width of the longest unbreakable content unit (word, image, etc.).
///
/// Phase-0 approximation: computes the max word width per `InlineRun` by
/// splitting on ASCII whitespace. This gives correct results for Latin text
/// but may overestimate for languages without whitespace-based word breaks.
pub(crate) fn min_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Percentage width behaves as `auto` here — see [`max_content_outer_width`]
    // (CSS Sizing L3 §5.2.1, BUG-742).
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    min_content_outer_width_of_contents(b, measurer, viewport)
}

/// Same as [`min_content_outer_width`] but ignoring `b`'s own definite `width`:
/// the min-content width the box would have if it were sized by its contents.
///
/// This is the CSS Flexbox §4.5 *content size suggestion*, which is deliberately
/// intrinsic — a flex item with `width: 300px` whose contents can collapse to
/// nothing still has a content size suggestion of 0, and so may be shrunk below
/// its preferred width. Descendants keep their own explicit widths; only the
/// box's own preferred size is bypassed.
fn min_content_outer_width_of_contents(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // min-content = widest unbreakable stretch of text.
            //
            // A space is a soft-wrap opportunity only where the segment's own
            // `white-space`/`text-wrap-mode` permits wrapping. Under `nowrap`
            // (and `pre`) there are none, so the stretch runs to the end of the
            // segment — and on to the next segment, since nothing between two
            // adjacent non-wrapping segments can break either. Splitting such
            // text on spaces anyway reported the widest *word* as the whole
            // run's minimum, which is what let a row of `white-space: nowrap`
            // flex items shrink far below their text and paint over each other
            // (BUG-427, dzen.ru topic tabs: "Москва — город будущего" claimed
            // the width of "будущего").
            //
            // `pre` still breaks at preserved newlines, so its stretches are the
            // segment's `\n`-separated lines rather than the whole segment.
            measurer.map_or(0.0, |m| {
                let mut best = 0.0_f32;
                let mut run = 0.0_f32;
                for seg in segments {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let fs = seg.style.font_size;
                    let ts = seg.style.tab_size * m.char_width_with_families(' ', fs, fams);
                    let piece =
                        |t: &str| measure_text_w_families(t, fs, ls, ts, fams, m);
                    let no_wrap = seg.style.white_space.is_nowrap()
                        || seg.style.text_wrap_mode == TextWrapMode::Nowrap;
                    if no_wrap {
                        let mut lines = seg.text.split('\n');
                        // The first line continues the stretch built so far.
                        if let Some(first) = lines.next() {
                            run += piece(first);
                            best = best.max(run);
                        }
                        for line in lines {
                            run = piece(line);
                            best = best.max(run);
                        }
                    } else {
                        // Wrappable: every space is a break opportunity, so the
                        // longest word bounds the minimum (a leading word could
                        // extend the previous stretch — deliberately not modelled,
                        // as before).
                        run = 0.0;
                        for word in seg.text.split_whitespace() {
                            best = best.max(piece(word));
                        }
                    }
                }
                best
            })
        }
        BoxKind::InlineBlockRow => {
            // For inline-block row, min-content is the max over children.
            b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return 0.0; // spaces are breakable
                }
                let cw = min_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).fold(0.0_f32, f32::max)
        }
        // Row flex container with `flex-wrap: nowrap`: the items cannot be
        // pushed onto separate lines, so the narrowest the container can get is
        // the sum of its items' min-content widths + gaps (CSS Flexbox §9.9).
        // With `wrap` the items *can* break onto their own lines, so the
        // min-content width is the widest single item — the block rule below.
        _ if is_row_flex_container(b) && matches!(b.style.flex_wrap, FlexWrap::Nowrap) => {
            flex_row_intrinsic_sum(b, viewport, &|c| {
                min_content_outer_width(c, measurer, viewport)
            })
        }
        _ => {
            b.children.iter()
                .filter(|c| contributes_to_intrinsic_width(c))
                .map(|c| min_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Flexbox L1 §9.2/§9.7 — flex base size (main-axis, **border-box**) of a
/// row-direction flex item whose `flex-basis` is `auto`/`content` and which has
/// no explicit `width`. This is the item's max-content width clamped by its own
/// `min-width` / `max-width`. Margins are excluded (the caller adds them).
/// `cb` is the flex container's inner main size, used to resolve percentage
/// min/max-width. Replaces the old approximation that fell back to the
/// preliminary-pass stretched `item.rect.width` for text-only items (BUG-179).
/// Потолок главной оси флекс-элемента во ВНЕШНИХ величинах (граничная рамка
/// плюс поля) — `f32::INFINITY`, если максимум не задан или не разрешается в
/// длину.
///
/// Нужен шагу «fix min/max violations» (CSS Flexbox §9.7 шаг 4): растущий
/// элемент обязан замереть на своём `max-width`/`max-height`, а не забирать
/// всё свободное место строки. Величина внешняя, потому что гипотетические
/// главные размеры в `lay_out_flex` тоже внешние.
pub(crate) fn flex_item_max_main_outer(item: &LayoutBox, cb: f32, viewport: Size, is_column: bool) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let max_len = if is_column { s.max_height.as_ref() } else { s.max_width.as_ref() };
    let Some(max_len) = max_len else {
        return f32::INFINITY;
    };
    // Внутренние ключевые слова (`max-content` и родня) здесь не ограничивают:
    // их разрешение требует измерения содержимого, а промах в бо́льшую сторону
    // безопаснее, чем ложная заморозка элемента.
    if max_len.is_intrinsic() {
        return f32::INFINITY;
    }
    let Some(v) = max_len.resolve(em, Some(cb), viewport) else {
        return f32::INFINITY;
    };
    let (p_start, p_end, b_start, b_end) = if is_column {
        (
            s.padding_top.resolve_or_zero(em, cb, viewport),
            s.padding_bottom.resolve_or_zero(em, cb, viewport),
            s.border_top_width,
            s.border_bottom_width,
        )
    } else {
        (
            s.padding_left.resolve_or_zero(em, cb, viewport),
            s.padding_right.resolve_or_zero(em, cb, viewport),
            s.border_left_width,
            s.border_right_width,
        )
    };
    let border_box = match s.box_sizing {
        BoxSizing::ContentBox => v + p_start + p_end + b_start + b_end,
        BoxSizing::BorderBox => v,
    };
    let (m_start, m_end) = if is_column {
        (
            s.margin_top.resolve_or_zero(em, cb, viewport),
            s.margin_bottom.resolve_or_zero(em, cb, viewport),
        )
    } else {
        (
            s.margin_left.resolve_or_zero(em, cb, viewport),
            s.margin_right.resolve_or_zero(em, cb, viewport),
        )
    };
    (border_box + m_start + m_end).max(0.0)
}

pub(crate) fn flex_auto_base_main_width(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, cb, viewport);
    let pr = s.padding_right.resolve_or_zero(em, cb, viewport);
    // content-box → border-box conversion for a resolved min/max length.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + pl + pr + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    let mut base = max_content_outer_width(item, measurer, viewport);
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(item, measurer, viewport))
        } else {
            max_len
                .resolve(em, Some(cb), viewport)
                .map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(m) = max_bb {
            base = base.min(m);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(item, measurer, viewport))
        } else {
            min_len
                .resolve(em, Some(cb), viewport)
                .map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(m) = min_bb {
            base = base.max(m);
        }
    }
    base.max(0.0)
}

/// CSS Flexbox L1 §4.5 — automatic minimum size (main axis, **border-box**) of a
/// row-direction flex item. This is the floor below which the item may not be
/// shrunk by `flex-shrink` (§9.7 step 4). Margins are excluded (the caller adds
/// them).
///
/// * An explicit `min-width` always wins — it is simply resolved (an intrinsic
///   keyword resolves against the item's own min-content width).
/// * `min-width: auto` (the initial value, stored as `None`) means the
///   *content-based minimum size*: the smaller of the item's *content size
///   suggestion* (the min-content width of its **contents** — see
///   [`min_content_outer_width_of_contents`]) and its *specified size
///   suggestion* (its own definite `width`, when it has one), capped by a
///   definite `max-width`. Taking the smaller of the two is what keeps an item
///   whose contents can collapse — e.g. one holding only a `width: 100%` child —
///   shrinkable below its own preferred width.
///   It applies only while the main-axis overflow is `visible`; a scroll
///   container has no content-based minimum and may shrink to zero.
///
/// `cb` is the flex container's inner main size, used to resolve percentages.
pub(crate) fn flex_item_min_main_width(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, cb, viewport);
    let pr = s.padding_right.resolve_or_zero(em, cb, viewport);
    // content-box → border-box conversion for a resolved min/max length.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + pl + pr + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(min_len) = &s.min_width {
        let v = if min_len.is_intrinsic() {
            min_content_outer_width(item, measurer, viewport)
        } else {
            min_len
                .resolve(em, Some(cb), viewport)
                .map_or(0.0, |v| outer_horiz(v.max(0.0)))
        };
        return v.max(0.0);
    }
    if s.overflow_x != Overflow::Visible {
        return 0.0;
    }
    let mut floor = min_content_outer_width_of_contents(item, measurer, viewport);
    // Specified size suggestion — the item's own definite preferred main size.
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(cb), viewport)
    {
        floor = floor.min(outer_horiz(w).max(0.0));
    }
    if let Some(max_len) = &s.max_width
        && !max_len.is_intrinsic()
        && let Some(v) = max_len.resolve(em, Some(cb), viewport)
    {
        floor = floor.min(outer_horiz(v).max(0.0));
    }
    floor.max(0.0)
}
