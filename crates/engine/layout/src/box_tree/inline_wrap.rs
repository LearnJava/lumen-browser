//! Text measurement/hyphenation + line wrapping (`wrap_inline_run`) and line
//! alignment/ellipsis/line-clamp.
//!
//! Перенесено батчем SPLIT-BT5 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn strip_soft_hyphens`) без правок тел.

use super::*;

/// Strips U+00AD (soft hyphens) from a word and collects break positions
/// (byte offsets in the returned display string).
pub(crate) fn strip_soft_hyphens(raw: &str) -> (String, Vec<usize>) {
    let mut display = String::with_capacity(raw.len());
    let mut positions: Vec<usize> = Vec::new();
    for ch in raw.chars() {
        if ch == '\u{00AD}' {
            positions.push(display.len());
        } else {
            display.push(ch);
        }
    }
    (display, positions)
}

/// Measures text width (letter_spacing applied between each character).
/// `tab_size` is used for `\t` characters; pass 0.0 when text contains no tabs.
pub fn measure_text_w(text: &str, font_size: f32, letter_spacing: f32, tab_size: f32, m: &dyn TextMeasurer) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' { tab_size } else { m.char_width(c, font_size) };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// Как [`measure_text_w`], но учитывает CSS `font-family` каскад.
///
/// Используется в `wrap_inline_run`, где для каждого `InlineSegment` доступен
/// `seg.style.font_family`. Позволяет `MultiFontMeasurer` выбирать правильный
/// шрифт для измерения ширины слов при перенос-расчёте.
pub fn measure_text_w_families(
    text: &str,
    font_size: f32,
    letter_spacing: f32,
    tab_size: f32,
    families: &[String],
    m: &dyn TextMeasurer,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' {
                tab_size
            } else {
                m.char_width_with_families(c, font_size, families)
            };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// Как [`measure_text_w_families`], но учитывает CSS `font-variation-settings`.
///
/// CSS Fonts L4 §6.3 — для variable fonts применяет HVAR advance width deltas.
/// Для статических шрифтов (без fvar/HVAR) эквивалентен [`measure_text_w_families`].
/// Используется в line wrapping когда `style.font_variation_settings` непустой.
pub fn measure_text_w_varied(
    text: &str,
    font_size: f32,
    letter_spacing: f32,
    tab_size: f32,
    families: &[String],
    axes: &[crate::style::FontVariationSetting],
    m: &dyn TextMeasurer,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' {
                tab_size
            } else {
                m.char_width_varied(c, font_size, axes, families)
            };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// CSS Fonts L4 §6.2 — множитель `font-size` для синтезированной капители.
///
/// Настоящая капитель приходит из OpenType-фич (`smcp`/`c2sc`/`pcap`/`c2pc`),
/// которых нет ни в bundled-Inter, ни в большинстве системных шрифтов. Как и
/// Gecko, синтезируем её геометрически: заглавная буква размера
/// `0.8 × font-size` примерно совпадает по высоте со строчной. Тот же
/// множитель используется и для `petite-caps` — отдельной, более низкой
/// капители у нас нет.
pub(crate) const SMALL_CAPS_SCALE: f32 = 0.8;

/// Ascent/(ascent+descent) для baseline-компенсации, когда measurer недоступен.
/// Совпадает с дефолтом [`TextMeasurer::ascent_px`].
const FALLBACK_ASCENT_RATIO: f32 = 0.8;

/// Роль символа при синтезе `font-variant-caps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapsRole {
    /// Рисуется как есть, шрифтом исходного размера.
    Plain,
    /// Рисуется как капитель: уменьшенный шрифт (и, кроме `unicase`,
    /// перевод в верхний регистр).
    Capital,
}

/// Определяет роль символа для заданного `font-variant-caps`.
///
/// Учитываются только символы, у которых есть регистр: `is_alphabetic()`
/// захватил бы CJK и арабицу, у которых капители не бывает, а уменьшать их
/// нельзя.
fn caps_role(ch: char, caps: FontVariantCaps) -> CapsRole {
    let cased = ch.is_lowercase() || ch.is_uppercase();
    let capital = match caps {
        // Капителью показываются только строчные.
        FontVariantCaps::SmallCaps | FontVariantCaps::PetiteCaps => ch.is_lowercase(),
        // Капителью показываются все буквы, включая уже заглавные.
        FontVariantCaps::AllSmallCaps | FontVariantCaps::AllPetiteCaps => cased,
        // `unicase`: капитель из заглавных, строчные не трогаем.
        FontVariantCaps::Unicase => ch.is_uppercase(),
        // `titling-caps` синтезировать нечем — уходит в шейпер фичей `titl`.
        FontVariantCaps::Normal | FontVariantCaps::TitlingCaps => false,
    };
    if capital { CapsRole::Capital } else { CapsRole::Plain }
}

/// `true`, если это значение `font-variant-caps` синтезируется в layout-е.
fn caps_needs_synthesis(caps: FontVariantCaps) -> bool {
    !matches!(caps, FontVariantCaps::Normal | FontVariantCaps::TitlingCaps)
}

/// CSS Fonts L4 §6.2 — синтез капители: режет сегменты на подсегменты по
/// границам «капитель / обычный текст».
///
/// Символы капители переводятся в верхний регистр (кроме `unicase`, где они
/// уже заглавные) и получают стиль с `font_size × `[`SMALL_CAPS_SCALE`].
/// Дальше measure/wrap/paint работают с подсегментами как с обычным текстом
/// разного размера — отдельного «capital»-режима ниже по конвейеру нет.
///
/// Второй элемент результата — по флагу на подсегмент: `true` означает
/// «перенос строки перед этим подсегментом запрещён». Разрез проходит
/// внутри слова (`Hello` → `H` + `ELLO`), а `wrap_inline_run` переносит по
/// границам сегментов — без флага слово рвалось бы пополам.
///
/// `m` нужен для baseline-компенсации: `apply_inline_vertical_align`
/// центрирует content-area фрагмента по half-leading, поэтому уменьшенный
/// фрагмент всплыл бы над базовой линией соседей. Сдвиг записывается в
/// `vertical-align: <length>` и только для фрагментов с `vertical-align:
/// baseline` — явно заданное автором выравнивание не трогаем.
///
/// Возвращает `None`, когда синтезировать нечего — вызывающий продолжает
/// работать с исходным срезом без аллокаций.
pub(crate) fn caps_synthesis(
    segments: &[InlineSegment],
    m: Option<&dyn TextMeasurer>,
) -> Option<(Vec<InlineSegment>, Vec<bool>)> {
    if !segments
        .iter()
        .any(|s| caps_needs_synthesis(s.style.font_variant_caps) && !s.text.is_empty())
    {
        return None;
    }
    let mut out: Vec<InlineSegment> = Vec::with_capacity(segments.len());
    let mut no_break: Vec<bool> = Vec::with_capacity(segments.len());
    for seg in segments {
        let caps = seg.style.font_variant_caps;
        if !caps_needs_synthesis(caps) || seg.text.is_empty() || seg.img_src.is_some() {
            out.push(seg.clone());
            no_break.push(false);
            continue;
        }
        // Стиль капители: уменьшенный кегль + компенсация базовой линии.
        let small = {
            let mut st = seg.style.clone();
            let big = seg.style.font_size;
            st.font_size = big * SMALL_CAPS_SCALE;
            if seg.style.vertical_align == VerticalAlign::Baseline {
                // apply_inline_vertical_align даёт y_offset = (line_h − h)/2 − px.
                // Нужно y_offset = (line_h − big)/2 + a·(big − h), откуда
                // px = (big − h)·(0.5 − a) и line_h сокращается.
                let ascent_ratio = m
                    .filter(|_| big > 0.0)
                    .map_or(FALLBACK_ASCENT_RATIO, |m| m.ascent_px(big) / big);
                let delta = big - st.font_size;
                st.vertical_align = VerticalAlign::Length(delta * (0.5 - ascent_ratio));
            }
            st
        };
        // Разрез на однородные по роли прогоны символов.
        let start = out.len();
        let mut prev_role: Option<CapsRole> = None;
        // Последний символ предыдущего прогона: перенос перед прогоном
        // разрешён только если он был пробельным.
        let mut prev_ch: Option<char> = None;
        for (idx, ch) in seg.text.char_indices() {
            let role = caps_role(ch, caps);
            if prev_role != Some(role) {
                out.push(InlineSegment {
                    text: String::new(),
                    style: if role == CapsRole::Capital { small.clone() } else { seg.style.clone() },
                    pre_space: 0.0,
                    post_space: 0.0,
                    is_element_box: seg.is_element_box,
                    img_src: None,
                    img_is_lazy: false,
                    img_width: 0.0,
                    forced_break: false,
                    // ::first-letter уже применён (apply_first_letter_pseudo
                    // работает до wrap) — маркер дальше не нужен.
                    pseudo_kind: PseudoKind::None,
                    source_node: seg.source_node,
                    source_char_offset: seg.source_char_offset.saturating_add(idx as u32),
                    bidi_level: seg.bidi_level,
                });
                no_break.push(
                    out.len() > start + 1
                        && !prev_ch.is_some_and(|c: char| c.is_whitespace()),
                );
                prev_role = Some(role);
            }
            // to_uppercase() может дать несколько символов (ß → SS) — так же
            // ведёт себя и настоящий OpenType-`smcp`.
            let Some(cur) = out.last_mut() else { continue };
            match (role, caps) {
                (CapsRole::Capital, FontVariantCaps::Unicase) => cur.text.push(ch),
                (CapsRole::Capital, _) => cur.text.extend(ch.to_uppercase()),
                (CapsRole::Plain, _) => cur.text.push(ch),
            }
            prev_ch = Some(ch);
        }
        // Внешние отступы inline-бокса остаются на краях исходного сегмента.
        if let Some(first) = out.get_mut(start) {
            first.pre_space = seg.pre_space;
        }
        if let Some(last) = out.last_mut() {
            last.post_space = seg.post_space;
        }
    }
    Some((out, no_break))
}

/// Tries to find a hyphenation break in `display` that fits within `available_w`.
/// `break_positions` are byte offsets in `display` (already sorted ascending).
/// Returns `(prefix_with_hyphen, suffix)` for the rightmost fitting break, or `None`.
pub(crate) fn try_hyp_break(
    display: &str,
    available_w: f32,
    font_size: f32,
    letter_spacing: f32,
    m: &dyn TextMeasurer,
    break_positions: &[usize],
) -> Option<(String, String)> {
    if break_positions.is_empty() || available_w <= 0.0 {
        return None;
    }
    let hyphen_w = m.char_width('-', font_size) + letter_spacing;
    // Try from rightmost to leftmost — most characters on current line preferred.
    for &pos in break_positions.iter().rev() {
        if !display.is_char_boundary(pos) || pos == 0 {
            continue;
        }
        let prefix = &display[..pos];
        let prefix_w = measure_text_w(prefix, font_size, letter_spacing, 0.0, m);
        if prefix_w + hyphen_w <= available_w {
            let mut pfx = prefix.to_string();
            pfx.push('-');
            return Some((pfx, display[pos..].to_string()));
        }
    }
    None
}

/// Разбивает потоковые сегменты на строки.
///
/// Алгоритм: жадный word-wrap + опциональные переносы (hyphens: manual/auto).
/// Слова одного стиля на одной строке сливаются
/// Returns the byte offset where `word` must be split so the prefix fits within
/// `avail_px`. Guarantees at least one character in the prefix to prevent
/// infinite loops when even a single character is wider than `avail_px`.
/// Returns `word.len()` when the whole word fits.
pub(crate) fn char_break_offset(
    word: &str,
    avail_px: f32,
    font_size: f32,
    ls: f32,
    families: &[String],
    m: &dyn TextMeasurer,
) -> usize {
    let mut w = 0.0_f32;
    for (char_idx, (byte_pos, ch)) in word.char_indices().enumerate() {
        let cw = m.char_width_with_families(ch, font_size, families);
        // Width of prefix ending at this char: sum(cw + ls) - ls.
        // For first char: width = cw (no trailing letter-spacing).
        let prefix_w = if char_idx == 0 { cw } else { w + ls + cw };
        if prefix_w > avail_px {
            if char_idx == 0 {
                // Even the first char overflows — emit it to avoid infinite loop.
                return byte_pos + ch.len_utf8();
            }
            return byte_pos;
        }
        w = prefix_w;
    }
    word.len()
}

/// Longest prefix of `word[start..]` that fits into `avail_px`, cut only at a
/// soft wrap opportunity listed in `opps` (byte offsets into `word`, ascending)
/// or at the end of the word.
///
/// Returns `start` when not even the first opportunity fits — the caller then
/// wraps to a fresh line and retries. Width is accumulated per character, the
/// same approximation `char_break_offset` uses.
#[allow(clippy::too_many_arguments)]
fn opportunity_break_offset(
    word: &str,
    start: usize,
    opps: &[usize],
    avail_px: f32,
    font_size: f32,
    ls: f32,
    families: &[String],
    m: &dyn TextMeasurer,
) -> usize {
    // Last opportunity known to fit; `start` means "nothing fits".
    let mut fit = start;
    // Width of `word[start..pos]` for the position reached so far.
    let mut w = 0.0_f32;
    for (idx, (rel, ch)) in word[start..].char_indices().enumerate() {
        let pos = start + rel;
        if pos > start {
            // `w` only grows, so once it overflows no later cut can fit.
            if w > avail_px {
                return fit;
            }
            if opps.binary_search(&pos).is_ok() {
                fit = pos;
            }
        }
        let cw = m.char_width_with_families(ch, font_size, families);
        // Width of the prefix ending at this char: sum(cw + ls) - ls.
        w = if idx == 0 { cw } else { w + ls + cw };
    }
    if w <= avail_px { word.len() } else { fit }
}

// ─── text-wrap: balance / pretty (CSS Text L4 §6.4.2) ───────────────────────

/// Returns the pixel width of the widest single word across all text segments.
/// Used as the lower-bound for `balance_wrap` binary search (cannot wrap narrower
/// than the longest token without breaking words).
fn widest_word(segments: &[InlineSegment], m: &dyn TextMeasurer) -> f32 {
    let mut max_w: f32 = 1.0;
    for seg in segments {
        if seg.img_src.is_some() {
            max_w = max_w.max(seg.img_width);
            continue;
        }
        let em = seg.style.font_size;
        let ls = seg.style.letter_spacing;
        let tab = seg.style.tab_size;
        let families = &seg.style.font_family;
        for raw in seg.text.split_whitespace() {
            let (display, _) = strip_soft_hyphens(raw);
            let w = measure_text_w_families(&display, em, ls, tab, families, m);
            max_w = max_w.max(w);
        }
    }
    max_w
}

/// CSS Text L4 §6.4.2 `text-wrap: balance` — redistributes line breaks so
/// that all lines are roughly equal in length.
///
/// Binary-searches the interval `[widest_word, container_width]` for the
/// minimum wrap width that produces the same number of lines as the greedy
/// result.  20 iterations → sub-pixel convergence for any container up to
/// ~500 000 px.  Single-line text is returned unchanged (nothing to balance).
#[allow(clippy::too_many_arguments)]
pub(crate) fn balance_wrap(
    segments: &[InlineSegment],
    container_width: f32,
    greedy_lines: Vec<Vec<InlineFrag>>,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
    line_break: LineBreak,
) -> Vec<Vec<InlineFrag>> {
    let target = greedy_lines.len();
    if target <= 1 {
        return greedy_lines;
    }
    let min_w = widest_word(segments, m);
    let mut lo = min_w;
    let mut hi = container_width;
    for _ in 0..20 {
        if hi - lo < 0.5 {
            break;
        }
        let mid = (lo + hi) * 0.5;
        let n = wrap_inline_run(
            segments, mid, container_font_size, text_indent, viewport,
            m, hyphens, hp, white_space, word_break, overflow_wrap, line_break,
        ).len();
        if n <= target {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    // Only re-wrap if we found a genuinely narrower balanced width.
    if hi < container_width - 0.5 {
        wrap_inline_run(
            segments, hi, container_font_size, text_indent, viewport,
            m, hyphens, hp, white_space, word_break, overflow_wrap, line_break,
        )
    } else {
        greedy_lines
    }
}

/// CSS Text L4 §6.4.2 `text-wrap: pretty` — prevents typographic widows.
///
/// A widow occurs when the last line contains only a single fragment.
/// This function finds a wrap width that moves one word from the penultimate
/// line onto the last line, so the last line has ≥ 2 fragments.
/// The total line count may increase by at most 1.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn pretty_wrap(
    segments: &[InlineSegment],
    container_width: f32,
    greedy_lines: Vec<Vec<InlineFrag>>,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
    line_break: LineBreak,
) -> Vec<Vec<InlineFrag>> {
    // A "widow" is a last line with exactly one word. Words may be merged into a
    // single InlineFrag, so check word count, not frag count.
    let last_word_count: usize = greedy_lines
        .last()
        .map(|l| l.iter().map(|f| f.text.split_whitespace().count()).sum())
        .unwrap_or(0);
    if last_word_count != 1 || greedy_lines.len() < 2 {
        return greedy_lines;
    }
    let target = greedy_lines.len();
    let penult = &greedy_lines[greedy_lines.len() - 2];
    if penult.is_empty() {
        return greedy_lines;
    }
    let penult_end = penult.last().map(|f| f.x + f.width).unwrap_or(0.0);
    // BUG-128: ширина пробела берётся семейством первого сегмента, а не
    // bundled Inter-ом — иначе строка едет относительно нарисованного текста.
    let space_w = segments.first().map_or_else(
        || m.char_width(' ', container_font_size),
        |s| m.char_width_with_families(' ', container_font_size, &s.style.font_family),
    );
    // The penultimate line's last frag may be merged (e.g. "aaaa bb cc").
    // Extract the last word's width to find where a tighter wrap would push it down.
    let last_frag = penult.last().unwrap();
    let last_word_w = last_frag
        .text
        .split_whitespace()
        .last()
        .map(|w| {
            let (display, _) = strip_soft_hyphens(w);
            measure_text_w_families(
                &display,
                last_frag.style.font_size,
                last_frag.style.letter_spacing,
                0.0,
                &last_frag.style.font_family,
                m,
            )
        })
        .unwrap_or(last_frag.width);

    // Width at which the last word of the penultimate line wraps to the last line,
    // eliminating the widow.
    let trial_w = (penult_end - last_word_w - space_w).max(widest_word(segments, m));

    if trial_w >= container_width - 0.5 {
        return greedy_lines;
    }
    let trial = wrap_inline_run(
        segments, trial_w, container_font_size, text_indent, viewport,
        m, hyphens, hp, white_space, word_break, overflow_wrap, line_break,
    );
    // Accept if the new last line has ≥ 2 words (merged or not) and line count
    // didn't blow up by more than 1 line.
    let trial_last_words: usize = trial
        .last()
        .map(|l| l.iter().map(|f| f.text.split_whitespace().count()).sum())
        .unwrap_or(0);
    if trial_last_words >= 2 && trial.len() <= target + 1 {
        trial
    } else {
        greedy_lines
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// в один `InlineFrag`. Сегменты обрабатываются по одному, чтобы учитывать
/// `pre_space` / `post_space` (inline box model: margin + border + padding).
/// `white_space` controls whether whitespace is preserved (pre/pre-wrap).
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_inline_run(
    segments: &[InlineSegment],
    max_width: f32,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
    line_break: LineBreak,
) -> Vec<Vec<InlineFrag>> {
    let _prof = lumen_core::profile::scope_detail("lo_wrap");
    // BUG-128: ширина пробела берётся семейством первого сегмента, а не
    // bundled Inter-ом — иначе строка едет относительно нарисованного текста.
    let space_w = segments.first().map_or_else(
        || m.char_width(' ', container_font_size),
        |s| m.char_width_with_families(' ', container_font_size, &s.style.font_family),
    );

    // CSS Fonts L4 §6.2 — синтезированная капитель: сегменты режутся на
    // прогоны разного кегля до всех измерений, поэтому wrap/measure/paint
    // ниже уже не знают о `font-variant-caps`.
    let synthesized = caps_synthesis(segments, Some(m));
    let (segments, caps_no_break): (&[InlineSegment], &[bool]) = match &synthesized {
        Some((segs, flags)) => (segs, flags),
        None => (segments, &[]),
    };

    let mut result: Vec<Vec<InlineFrag>> = Vec::new();
    let mut current_line: Vec<InlineFrag> = Vec::new();
    // CSS Text L3 §7.1: text-indent только на первой строке.
    let mut current_x = text_indent;
    // CSS Text L3 §4.1.1 — whether the previous segment ended with collapsible
    // whitespace, so the first word of the next segment gets one inter-word gap.
    // A segment boundary with no whitespace on either side joins tightly (e.g.
    // `<q>` `::before` open-quote glued to the quoted text, `<a>link</a>!`).
    let mut prev_trailing_ws = false;

    for (seg_idx, seg) in segments.iter().enumerate() {
        // Перенос перед первым словом запрещён, когда сегмент — «хвост»
        // разрезанного капителью слова (см. `caps_synthesis`).
        let no_break_before = caps_no_break.get(seg_idx).copied().unwrap_or(false);
        // Forced line break from \n in white-space: pre/pre-wrap text.
        if seg.forced_break {
            result.push(std::mem::take(&mut current_line));
            current_x = 0.0;
            prev_trailing_ws = false;
            continue;
        }

        // Does this segment's source text carry collapsible whitespace at its
        // edges? Used to decide the boundary gap with the previous segment.
        let seg_lead_ws = seg.text.starts_with(|c: char| c.is_whitespace());
        let seg_trail_ws = seg.text.ends_with(|c: char| c.is_whitespace());

        // Pre-mode: whitespace preserved, no word wrapping, tabs are tab_size wide.
        if white_space.preserves_whitespace() {
            if seg.text.is_empty() {
                continue;
            }
            prev_trailing_ws = false;
            let style = &seg.style;
            let em = style.font_size;
            let ls = style.letter_spacing;
            let tab_size = style.tab_size;
            let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_x += seg.pre_space;
            let frag_x = current_x;
            let frag_w = measure_text_w_varied(&seg.text, em, ls, tab_size, &seg.style.font_family, &seg.style.font_variation_settings, m);
            current_line.push(InlineFrag {
                x: frag_x,
                y_offset: 0.0,
                width: frag_w,
                text: seg.text.clone(),
                style: style.clone(),
                padding_left: pad_l,
                padding_right: pad_r,
                is_element_box: seg.is_element_box,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
            });
            current_x += frag_w + seg.post_space;
            continue;
        }

        // Image segments are fixed-width, non-breakable inline replaced elements.
        if let Some(img_src) = &seg.img_src {
            let img_w = seg.img_width;
            // A space precedes the image only when collapsible whitespace did.
            let img_space = if prev_trailing_ws { space_w } else { 0.0 };
            let gap = if current_line.is_empty() { 0.0 } else { img_space };
            if !current_line.is_empty() && current_x + gap + seg.pre_space + img_w > max_width {
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
            }
            let line_gap = if current_line.is_empty() { 0.0 } else { img_space };
            current_x += line_gap + seg.pre_space;
            let em = seg.style.font_size;
            let pad_l = seg.style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = seg.style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_line.push(InlineFrag {
                x: current_x,
                y_offset: 0.0,
                width: img_w,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: pad_l,
                padding_right: pad_r,
                is_element_box: true,
                img_src: Some(img_src.clone()),
                img_is_lazy: seg.img_is_lazy,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
            });
            current_x += img_w + seg.post_space;
            // Trailing whitespace after the image (a collapsed ws-only node) is
            // recorded as a trailing space on its alt text by collect_inline_segments.
            prev_trailing_ws = seg_trail_ws;
            continue;
        }

        // Collect words; split_whitespace preserves U+00AD within tokens.
        let raw_words: Vec<&str> = seg.text.split_whitespace().collect();
        if raw_words.is_empty() {
            // Whitespace-only segment (rare in collapsing mode): propagate the gap.
            if seg_lead_ws || seg_trail_ws {
                prev_trailing_ws = true;
            }
            continue;
        }
        let style = &seg.style;
        let em = style.font_size;
        let ls = style.letter_spacing;
        let ws = style.word_spacing;
        let inter_word = space_w + ls + ws;

        // Resolved padding for this segment's inline box (for paint use).
        let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
        let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);

        let n = raw_words.len();
        for (wi, raw_word) in raw_words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;

            // Strip soft hyphens for display + collect hyphenation break positions.
            let (display_word, shy_positions) = strip_soft_hyphens(raw_word);

            // Byte offset of this word within seg.text — used for Selection/Range mapping.
            // raw_word is a subslice produced by split_whitespace(), so pointer arithmetic is valid.
            let frag_source_offset = {
                let raw_ptr = raw_word.as_ptr() as usize;
                let seg_ptr = seg.text.as_ptr() as usize;
                let word_off = if raw_ptr >= seg_ptr && raw_ptr <= seg_ptr + seg.text.len() {
                    (raw_ptr - seg_ptr) as u32
                } else {
                    0u32
                };
                seg.source_char_offset.saturating_add(word_off)
            };

            // Space that the inline box model contributes at the word boundaries.
            let pre = if is_seg_first { seg.pre_space } else { 0.0 };
            let post = if is_seg_last { seg.post_space } else { 0.0 };

            let word_w = measure_text_w_varied(&display_word, style.font_size, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
            // CSS Text L3 §4.1.1 — inter-word gap before this word. Words within a
            // segment are always separated (they were split on real whitespace);
            // the first word of a segment is separated from the previous fragment
            // only when collapsible whitespace bordered the segment boundary.
            let word_inter = if is_seg_first && !(prev_trailing_ws || seg_lead_ws) {
                0.0
            } else {
                inter_word
            };
            let gap = if current_line.is_empty() { 0.0 } else { word_inter };

            // Перенос перед этим словом разрешён? Запрещён он только на стыке
            // подсегментов, разрезанных капителью внутри слова.
            let breakable = !is_seg_first || !no_break_before;
            // Wrap: слово не влезает (но первое слово строки добавляем всегда).
            let needs_wrap = !current_line.is_empty()
                && breakable
                && current_x + gap + pre + word_w > max_width;

            // CSS Text L3 §5.5 `line-break` — soft wrap opportunities *inside*
            // the word. CJK text carries no spaces, so `split_whitespace` hands
            // us whole paragraphs here; without this the run would either
            // overflow the container or be pushed onto a line of its own.
            // Only relevant when the word does not fit as-is; `word-break:
            // keep-all` suppresses CJK breaking entirely, except under
            // `line-break: anywhere`, which overrides every prohibition.
            let lb_avail = max_width - current_x - gap - pre;
            let lb_allowed = word_break != WordBreak::KeepAll || line_break == LineBreak::Anywhere;
            let lb_opps = if breakable && lb_allowed && word_w > lb_avail {
                crate::line_break::break_opportunities(&display_word, line_break)
            } else {
                Vec::new()
            };

            if !lb_opps.is_empty() {
                // Byte offset of the part of the word still to be placed.
                let mut start = 0usize;
                let mut first_chunk = true;
                while start < display_word.len() {
                    let chunk_gap = if current_line.is_empty() { 0.0 } else { word_inter };
                    let chunk_pre = if first_chunk { pre } else { 0.0 };
                    let avail = (max_width - current_x - chunk_gap - chunk_pre).max(0.0);
                    let mut end = opportunity_break_offset(
                        &display_word, start, &lb_opps, avail,
                        style.font_size, ls, &style.font_family, m,
                    );
                    if end == start {
                        if !current_line.is_empty() {
                            // Nothing fits in what is left of this line — wrap
                            // and measure again against the full width.
                            result.push(std::mem::take(&mut current_line));
                            current_x = 0.0;
                            continue;
                        }
                        // The line is empty and even the shortest chunk
                        // overflows: emit it anyway so the loop terminates.
                        end = lb_opps
                            .iter()
                            .copied()
                            .find(|&o| o > start)
                            .unwrap_or(display_word.len());
                    }
                    let chunk = &display_word[start..end];
                    let chunk_w = measure_text_w_varied(chunk, style.font_size, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
                    current_x += chunk_gap + chunk_pre;
                    current_line.push(InlineFrag {
                        x: current_x,
                        y_offset: 0.0,
                        width: chunk_w,
                        text: chunk.to_string(),
                        style: style.clone(),
                        padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                        padding_right: if end == display_word.len() && is_seg_last { pad_r } else { 0.0 },
                        is_element_box: seg.is_element_box,
                        img_src: None,
                        img_is_lazy: false,
                        is_first_line: false,
                        source_node: seg.source_node,
                        // Soft hyphens (stripped from `display_word`) would shift
                        // this; CJK text does not use them.
                        source_char_offset: frag_source_offset.saturating_add(start as u32),
                        bidi_level: seg.bidi_level,
                    });
                    current_x += chunk_w;
                    first_chunk = false;
                    start = end;
                    if start < display_word.len() {
                        result.push(std::mem::take(&mut current_line));
                        current_x = 0.0;
                    }
                }
                current_x += post;
                continue;
            }

            if needs_wrap {
                // CSS Text L3 §6: try hyphenation before hard wrap.
                let hyph_result = if hyphens != Hyphens::None {
                    let mut break_pts = shy_positions.clone();
                    if hyphens == Hyphens::Auto && !display_word.is_empty() {
                        let auto_pts = hp.hyphenate(&display_word, "");
                        break_pts.extend_from_slice(&auto_pts);
                        break_pts.sort_unstable();
                        break_pts.dedup();
                    }
                    let avail = max_width - current_x - gap - pre;
                    try_hyp_break(&display_word, avail, style.font_size, ls, m, &break_pts)
                } else {
                    None
                };

                if let Some((pfx, sfx)) = hyph_result {
                    // Emit prefix (with trailing '-') to current line, then wrap.
                    let pfx_w = measure_text_w_varied(&pfx, style.font_size, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
                    current_x += gap + pre;
                    current_line.push(InlineFrag {
                        x: current_x,
                        y_offset: 0.0,
                        width: pfx_w,
                        text: pfx,
                        style: style.clone(),
                        padding_left: if is_seg_first { pad_l } else { 0.0 },
                        padding_right: 0.0,
                        is_element_box: seg.is_element_box,
                        img_src: None,
                        img_is_lazy: false,
                        is_first_line: false,
                        source_node: seg.source_node,
                        source_char_offset: frag_source_offset,
                        bidi_level: seg.bidi_level,
                    });
                    result.push(std::mem::take(&mut current_line));
                    current_x = 0.0;
                    // Emit suffix as first fragment on new line.
                    let sfx_w = measure_text_w_varied(&sfx, style.font_size, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
                    current_line.push(InlineFrag {
                        x: 0.0,
                        y_offset: 0.0,
                        width: sfx_w,
                        text: sfx,
                        style: style.clone(),
                        padding_left: 0.0,
                        padding_right: if is_seg_last { pad_r } else { 0.0 },
                        is_element_box: seg.is_element_box,
                        img_src: None,
                        img_is_lazy: false,
                        is_first_line: false,
                        source_node: seg.source_node,
                        source_char_offset: frag_source_offset,
                        bidi_level: seg.bidi_level,
                    });
                    current_x += sfx_w + post;
                    continue;
                }

                // CSS Text L3 §5.1: word-break: break-all — char-break at the
                // current line position before wrapping.
                if word_break == WordBreak::BreakAll {
                    let gap_w = if current_line.is_empty() { 0.0 } else { word_inter };
                    current_x += gap_w + pre;
                    let mut rest = display_word.as_str();
                    let mut first_chunk = true;
                    while !rest.is_empty() {
                        let avail = (max_width - current_x).max(0.0);
                        let split = char_break_offset(rest, avail, style.font_size, ls, &style.font_family, m);
                        let head = &rest[..split];
                        let tail = &rest[split..];
                        if !head.is_empty() {
                            let head_w = measure_text_w_varied(head, style.font_size, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
                            current_line.push(InlineFrag {
                                x: current_x,
                                y_offset: 0.0,
                                width: head_w,
                                text: head.to_string(),
                                style: style.clone(),
                                padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                                padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                                is_element_box: seg.is_element_box,
                                img_src: None,
                                img_is_lazy: false,
                                is_first_line: false,
                                source_node: seg.source_node,
                                source_char_offset: frag_source_offset,
                                bidi_level: seg.bidi_level,
                            });
                            current_x += head_w;
                            first_chunk = false;
                        }
                        rest = tail;
                        if !rest.is_empty() {
                            result.push(std::mem::take(&mut current_line));
                            current_x = 0.0;
                        }
                    }
                    current_x += post;
                    continue;
                }

                // No hyphenation break found — normal wrap.
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
            }

            // CSS Text L3 §8.1: overflow-wrap: break-word / anywhere — char-break
            // words that are wider than the container (won't fit on any line).
            // word-break: break-word is a legacy alias for overflow-wrap: break-word.
            let ow_char_break = (word_break == WordBreak::BreakWord
                || matches!(overflow_wrap, OverflowWrap::BreakWord | OverflowWrap::Anywhere))
                && word_w > max_width;
            if ow_char_break {
                let line_gap_ow = if current_line.is_empty() { 0.0 } else { word_inter };
                current_x += line_gap_ow + pre;
                let mut rest = display_word.as_str();
                let mut first_chunk = true;
                while !rest.is_empty() {
                    let avail = (max_width - current_x).max(0.0);
                    let split = char_break_offset(rest, avail, style.font_size, ls, &style.font_family, m);
                    let head = &rest[..split];
                    let tail = &rest[split..];
                    if !head.is_empty() {
                        let head_w = measure_text_w_varied(head, style.font_size, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
                        current_line.push(InlineFrag {
                            x: current_x,
                            y_offset: 0.0,
                            width: head_w,
                            text: head.to_string(),
                            style: style.clone(),
                            padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                            padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                            is_element_box: seg.is_element_box,
                            img_src: None,
                            img_is_lazy: false,
                            is_first_line: false,
                            source_node: seg.source_node,
                            source_char_offset: frag_source_offset,
                            bidi_level: seg.bidi_level,
                        });
                        current_x += head_w;
                        first_chunk = false;
                    }
                    rest = tail;
                    if !rest.is_empty() {
                        result.push(std::mem::take(&mut current_line));
                        current_x = 0.0;
                    }
                }
                current_x += post;
                continue;
            }

            let line_gap = if current_line.is_empty() { 0.0 } else { word_inter };
            current_x += line_gap + pre;
            let frag_x = current_x;

            // Слияние: только когда нет pre/post space у данного слова
            // и предыдущий фраг тоже не заканчивается inline-box-ом.
            let no_box = pre == 0.0 && post == 0.0;
            let merged = if no_box {
                if let Some(last) = current_line.last_mut() {
                    // Fragments at different UAX #9 embedding levels must stay
                    // apart: L2 reorders and reverses per fragment, so merging
                    // an RTL word with its LTR neighbour would reverse both.
                    if last.style.text_rendering_eq(style)
                        && last.padding_right == 0.0
                        && last.bidi_level == seg.bidi_level
                    {
                        // No separating space when the boundary joined tightly
                        // (word_inter == 0): the glyphs abut, e.g. `“`+`auto`.
                        if word_inter > 0.0 {
                            last.text.push(' ');
                        }
                        last.text.push_str(&display_word);
                        last.width += word_inter + word_w;
                        current_x += word_w;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !merged {
                current_line.push(InlineFrag {
                    x: frag_x,
                    y_offset: 0.0,
                    width: word_w,
                    text: display_word,
                    style: style.clone(),
                    padding_left: if is_seg_first { pad_l } else { 0.0 },
                    padding_right: if is_seg_last { pad_r } else { 0.0 },
                    is_element_box: seg.is_element_box,
                    img_src: None,
                    img_is_lazy: false,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: frag_source_offset,
                    bidi_level: seg.bidi_level,
                });
                current_x += word_w;
            }

            current_x += post;
        }
        // The next segment joins with a gap only if this one ended in whitespace.
        prev_trailing_ws = seg_trail_ws;
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    result
}

/// Переупорядочивает фрагменты каждой строки в визуальный порядок и сдвигает
/// их по text-align + direction.
///
/// Сначала UAX #9 rule L2 ([`crate::bidi::reorder_line`]): для LTR-параграфа
/// из чистого LTR-текста это no-op, для RTL — зеркалирование строки, для
/// смешанного текста — вложенные развороты по embedding level.
/// Затем `Start`/`End` разрешаются в Left/Right по direction (CSS Text L3
/// §7.1) и строка сдвигается как блок.
/// Последняя строка выравнивается по `text_align_last` (CSS Text L3 §7.2):
/// `Auto` на justify-блоке → Start; иначе → как text_align.
pub(crate) fn align_lines(
    lines: &mut [Vec<InlineFrag>],
    content_width: f32,
    text_align: TextAlign,
    text_align_last: TextAlignLast,
    direction: Direction,
) {
    let is_rtl = direction == Direction::Rtl;
    let total = lines.len();
    for (idx, line) in lines.iter_mut().enumerate() {
        let is_last = idx + 1 == total;
        // CSS Text L3 §7.2: last line uses text-align-last.
        // Auto → same as text-align (justify not yet in TextAlign, so no special case).
        // TextAlignLast::Justify → Start (word-spacing justification not yet implemented).
        let effective = if is_last {
            match text_align_last {
                TextAlignLast::Auto    => text_align,
                TextAlignLast::Left    => TextAlign::Left,
                TextAlignLast::Right   => TextAlign::Right,
                TextAlignLast::Center  => TextAlign::Center,
                TextAlignLast::Start   => TextAlign::Start,
                TextAlignLast::End     => TextAlign::End,
                TextAlignLast::Justify => TextAlign::Start,
            }
        } else {
            text_align
        };
        // Resolve Start/End to physical Left/Right.
        let physical = match effective {
            TextAlign::Start => if is_rtl { TextAlign::Right } else { TextAlign::Left },
            TextAlign::End   => if is_rtl { TextAlign::Left  } else { TextAlign::Right },
            other => other,
        };
        // Measured before reordering, while `wrap_inline_run`'s ascending-x
        // order still holds, so the last frag is the rightmost one.
        let Some(last_frag) = line.last() else { continue };
        let line_width = last_frag.x + last_frag.width;
        // UAX #9 L2 — logical → visual placement. Subsumes the RTL line mirror
        // `align_lines` used to do itself, but level-aware, so an LTR island
        // inside an RTL paragraph keeps its own left-to-right order.
        crate::bidi::reorder_line(line, direction);
        let offset = match physical {
            TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
            TextAlign::Right  => (content_width - line_width).max(0.0),
            _                 => 0.0,
        };
        if offset > 0.0 {
            for frag in line.iter_mut() {
                frag.x += offset;
            }
        }
    }
}

/// CSS Rhythmic Sizing L1 §2 — `line-height-step`. Округляет высоту line-box `raw`
/// вверх до ближайшего кратного `step` (в px). При `step <= 0` (свойство выключено)
/// возвращает `raw` без изменений. Дополнительный зазор распределяется как half-leading
/// штатным `apply_inline_vertical_align`, которому передаётся уже округлённая высота.
pub(crate) fn step_line_height(raw: f32, step: f32) -> f32 {
    if step > 0.0 {
        (raw / step).ceil() * step
    } else {
        raw
    }
}

/// CSS 2.1 §10.8 — применяет вертикальное выравнивание к inline-фрагментам.
/// Записывает `y_offset` (смещение от верхнего края line-box, вниз — положительное).
/// `line_h` = font_size * line_height контейнера.
///
/// Half-leading (§10.8.1): когда line-height > content-area, разница делится пополам
/// и добавляется выше и ниже content-area. Для `baseline` — фрагмент сдвигается вниз
/// на `half_leading = (line_h - frag_h) / 2`, чтобы content-area была центрирована.
pub(crate) fn apply_inline_vertical_align(lines: &mut [Vec<InlineFrag>], line_h: f32) {
    for line in lines.iter_mut() {
        for frag in line.iter_mut() {
            // frag_h: content area height ≈ font-size (ascent + descent for normal line-height).
            let frag_h = frag.style.font_size;
            // CSS 2.1 §10.8.1: half-leading pushes content area away from line-box edges.
            let half_leading = ((line_h - frag_h) / 2.0).max(0.0);
            frag.y_offset = match frag.style.vertical_align {
                // Baseline: content area centred via half-leading (top = half_leading).
                VerticalAlign::Baseline => half_leading,
                // Top/TextTop: fragment top-aligned to line-box top edge.
                VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                // Bottom/TextBottom: fragment bottom-aligned to line-box bottom edge.
                VerticalAlign::Bottom | VerticalAlign::TextBottom => (line_h - frag_h).max(0.0),
                // Middle: visual midpoint of fragment at midpoint of line-box.
                VerticalAlign::Middle => ((line_h - frag_h) / 2.0).max(0.0),
                // sub/super: relative shift from baseline (~0.8 * frag_h from frag top).
                VerticalAlign::Sub => half_leading + frag_h * 0.15,
                VerticalAlign::Super => half_leading - frag_h * 0.35,
                // CSS: positive length = shift up (above baseline) → negative screen y.
                VerticalAlign::Length(px) => half_leading - px,
                VerticalAlign::Percent(p) => half_leading - (p / 100.0 * line_h),
            };
        }
    }
}

/// Без измерителя: помещаем всё в одну строку. Ширина каждого фрагмента
/// без шрифтовых метрик неизвестна — оставляем 0.0; text-decoration в этом
/// режиме не рисуется. layout() для финального рендеринга всё равно ходит
/// через layout_measured().
pub(crate) fn one_line_fallback(segments: &[InlineSegment]) -> Vec<Vec<InlineFrag>> {
    // Капитель режется и здесь: без этого дамп без measurer-а показывал бы
    // исходный регистр, а с measurer-ом — капитель (расхождение путей).
    let synthesized = caps_synthesis(segments, None);
    let segments: &[InlineSegment] = match &synthesized {
        Some((segs, _)) => segs,
        None => segments,
    };
    let mut frags: Vec<InlineFrag> = Vec::new();
    // CSS Text L3 §4.1.1 — same boundary rule as wrap_inline_run: two segments
    // join with a single space only when collapsible whitespace bordered them;
    // otherwise they abut (e.g. `<q>` open-quote glued to the quoted text).
    let mut prev_trailing_ws = false;
    for seg in segments {
        // Image segment: emit with pre-computed width, don't merge with text.
        if let Some(img_src) = &seg.img_src {
            frags.push(InlineFrag {
                x: 0.0,
                y_offset: 0.0,
                width: seg.img_width,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: true,
                img_src: Some(img_src.clone()),
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
            });
            prev_trailing_ws = seg.text.ends_with(|c: char| c.is_whitespace());
            continue;
        }
        let seg_lead_ws = seg.text.starts_with(|c: char| c.is_whitespace());
        let seg_trail_ws = seg.text.ends_with(|c: char| c.is_whitespace());
        let text: String = seg.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            if seg_lead_ws || seg_trail_ws {
                prev_trailing_ws = true;
            }
            continue;
        }
        let boundary_space = prev_trailing_ws || seg_lead_ws;
        let merged = if let Some(last) = frags.last_mut() {
            // Same embedding-level guard as `wrap_inline_run` — see there.
            if last.style.text_rendering_eq(&seg.style)
                && last.img_src.is_none()
                && last.bidi_level == seg.bidi_level
            {
                if boundary_space {
                    last.text.push(' ');
                }
                last.text.push_str(&text);
                true
            } else {
                false
            }
        } else {
            false
        };
        if !merged {
            frags.push(InlineFrag {
                x: 0.0,
                y_offset: 0.0,
                width: 0.0,
                text,
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
            });
        }
        prev_trailing_ws = seg_trail_ws;
    }
    if frags.is_empty() { vec![] } else { vec![frags] }
}

/// CSS UI L4 §10.1 — усекает фрагменты строк, выходящих за `max_width`,
/// добавляя символ «…» (U+2026). Вызывается только когда `text-overflow:
/// ellipsis` И `overflow` создаёт clip.
pub(crate) fn apply_text_overflow_ellipsis(
    lines: &mut [Vec<InlineFrag>],
    max_width: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
) {
    let ellipsis = '\u{2026}'; // …
    let ellipsis_w = m.char_width(ellipsis, font_size);

    for line in lines.iter_mut() {
        let line_end = line.last().map(|f| f.x + f.width).unwrap_or(0.0);
        if line_end <= max_width {
            continue;
        }

        // Максимальная ширина для текстового контента перед «…».
        let budget = (max_width - ellipsis_w).max(0.0);

        // Ищем первый фрагмент, чьё начало выходит за budget.
        let cut = line.iter().position(|f| f.x > budget);

        match cut {
            Some(0) => {
                // Первый фрагмент уже за budget — показываем только «…».
                line[0].text = ellipsis.to_string();
                line[0].width = ellipsis_w;
                line.truncate(1);
            }
            Some(fi) => {
                // Усекаем фрагмент fi-1, удаляем fi и далее.
                let avail = budget - line[fi - 1].x;
                truncate_frag_with_ellipsis(&mut line[fi - 1], avail, font_size, m, ellipsis, ellipsis_w);
                line.truncate(fi);
            }
            None => {
                // Все фрагменты начинаются в пределах budget, но последний
                // выходит за max_width — усекаем его.
                let last = line.len() - 1;
                let avail = budget - line[last].x;
                truncate_frag_with_ellipsis(&mut line[last], avail, font_size, m, ellipsis, ellipsis_w);
            }
        }
    }
}

fn truncate_frag_with_ellipsis(
    frag: &mut InlineFrag,
    avail: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
    ellipsis: char,
    ellipsis_w: f32,
) {
    let mut buf = String::new();
    let mut w = 0.0_f32;
    for ch in frag.text.chars() {
        let cw = m.char_width(ch, font_size);
        if w + cw > avail {
            break;
        }
        buf.push(ch);
        w += cw;
    }
    buf.push(ellipsis);
    frag.text = buf;
    frag.width = w + ellipsis_w;
}

/// CSS Overflow L4 §3.2 / CSS Display L3 §7.2 — `-webkit-line-clamp` / `line-clamp`.
///
/// Truncates `lines` to at most `max_lines` entries. If truncation occurred, forces
/// an ellipsis (U+2026) onto the *last* visible line to signal omitted content.
/// The ellipsis is appended to the last fragment if the line fits within `max_width`,
/// or replaces overflowing text if the line is already too wide.
///
/// Called only when a text measurer is available (same guard as `text-overflow: ellipsis`).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn apply_line_clamp(
    lines: &mut Vec<Vec<InlineFrag>>,
    max_lines: u32,
    max_width: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
) {
    let n = max_lines as usize;
    if lines.len() <= n {
        return;
    }
    lines.truncate(n);

    let ellipsis = '\u{2026}';
    let ellipsis_w = m.char_width(ellipsis, font_size);
    let last = match lines.last_mut() {
        Some(l) => l,
        None => return,
    };
    if last.is_empty() {
        return;
    }

    let line_end = last.last().map(|f| f.x + f.width).unwrap_or(0.0);
    if line_end + ellipsis_w <= max_width {
        // Line fits: append "…" by extending the last fragment.
        let last_frag = last.last_mut().unwrap();
        last_frag.text.push(ellipsis);
        last_frag.width += ellipsis_w;
    } else {
        // Line overflows: truncate from the right to make room for "…".
        let budget = (max_width - ellipsis_w).max(0.0);
        let cut = last.iter().position(|f| f.x > budget);
        match cut {
            Some(0) => {
                last[0].text = ellipsis.to_string();
                last[0].width = ellipsis_w;
                last.truncate(1);
            }
            Some(fi) => {
                let avail = budget - last[fi - 1].x;
                truncate_frag_with_ellipsis(&mut last[fi - 1], avail, font_size, m, ellipsis, ellipsis_w);
                last.truncate(fi);
            }
            None => {
                let idx = last.len() - 1;
                let avail = budget - last[idx].x;
                truncate_frag_with_ellipsis(&mut last[idx], avail, font_size, m, ellipsis, ellipsis_w);
            }
        }
    }
}
