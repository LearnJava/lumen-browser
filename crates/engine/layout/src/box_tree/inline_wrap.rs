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

/// Ширина `text` как последовательности shaped run-ов между табуляциями
/// (TEXTQ-5): кернинг и лигатуры считает шейпер измерителя
/// ([`TextMeasurer::shaped_width`]) теми же фичами и осями, что и отрисовка;
/// посимвольная сумма остаётся запасным путём для измерителей без шейпера.
/// `letter-spacing` стоит между символами, после последнего его нет, `\t`
/// занимает `tab_size`. Табуляция разрывает run: лигатура и кернинговая пара
/// через неё не строятся — так же шейпит отрисовка.
fn measure_run(
    text: &str,
    tab_size: f32,
    params: &ShapeParams<'_>,
    css_axes: &[crate::style::FontVariationSetting],
    m: &dyn TextMeasurer,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let ls = params.letter_spacing;
    let mut total = 0.0_f32;
    for (i, piece) in text.split('\t').enumerate() {
        if i > 0 {
            total += tab_size + ls;
        }
        if piece.is_empty() {
            continue;
        }
        total += match m.shaped_width(piece, params) {
            Some(w) => w,
            None => piece
                .chars()
                .map(|c| m.char_width_varied(c, params.font_size_px, css_axes, params.families) + ls)
                .sum(),
        };
    }
    total - ls
}

/// Measures text width (letter_spacing applied between each character).
/// `tab_size` is used for `\t` characters; pass 0.0 when text contains no tabs.
pub fn measure_text_w(text: &str, font_size: f32, letter_spacing: f32, tab_size: f32, m: &dyn TextMeasurer) -> f32 {
    let params = ShapeParams { font_size_px: font_size, letter_spacing, families: &[], features: &[], axes: &[] };
    measure_run(text, tab_size, &params, &[], m)
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
    let params = ShapeParams { font_size_px: font_size, letter_spacing, families, features: &[], axes: &[] };
    measure_run(text, tab_size, &params, &[], m)
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
    let shape_axes: Vec<([u8; 4], f32)> = axes.iter().map(|a| (a.tag, a.value)).collect();
    let params = ShapeParams { font_size_px: font_size, letter_spacing, families, features: &[], axes: &shape_axes };
    measure_run(text, tab_size, &params, axes, m)
}

/// Ширина `text` с ПОЛНЫМИ шейпинг-параметрами `style` (TEXTQ-5): размер,
/// `letter-spacing`, `font-family`, `font-feature-settings`/`font-variant-*`
/// и оси вариаций (с автоматическими `opsz`/`wdth`) — ровно те, с которыми
/// стиль уйдёт в `DrawText`. Ширина слова в раскладке совпадает с нарисованной.
pub(crate) fn measure_text_w_styled(text: &str, style: &ComputedStyle, tab_size: f32, m: &dyn TextMeasurer) -> f32 {
    let features = crate::style::text_font_features(style);
    crate::style::with_text_variation_axes(style, |axes| {
        let params = ShapeParams {
            font_size_px: style.font_size,
            letter_spacing: style.letter_spacing,
            families: &style.font_family,
            features: &features,
            axes,
        };
        measure_run(text, tab_size, &params, &style.font_variation_settings, m)
    })
}

/// Tab-stop geometry of one run of text (CSS Text L3 §4.2 `tab-size`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TabStops {
    /// Distance between neighbouring tab stops, in px. `0` — no stops.
    pub(crate) unit: f32,
    /// Shortest rendered tab: a stop closer than this is skipped (`0.5ch`).
    pub(crate) min_w: f32,
}

impl TabStops {
    /// `tab-size: <number>` is `n` advances of U+0020 *including* its
    /// `letter-spacing` and `word-spacing`; `<length>` is taken as is.
    pub(crate) fn of(style: &ComputedStyle, m: &dyn TextMeasurer) -> Self {
        let fams = &style.font_family;
        let unit = match style.text_extra.tab_size_number {
            Some(n) => {
                let space = m.char_width_with_families(' ', style.font_size, fams)
                    + style.letter_spacing
                    + style.word_spacing
                    + style.word_spacing_pct * m.char_width_with_families(' ', style.font_size, fams);
                n * space
            }
            None => style.tab_size,
        };
        Self { unit, min_w: 0.5 * m.char_width_with_families('0', style.font_size, fams) }
    }

    /// Advance of a tab whose pen sits at `x` (measured from the line's start edge).
    pub(crate) fn advance(&self, x: f32) -> f32 {
        if self.unit <= 0.0 {
            return 0.0;
        }
        let adv = self.unit - x.max(0.0).rem_euclid(self.unit);
        if adv < self.min_w { adv + self.unit } else { adv }
    }
}

/// Width of `text` laid out from pen position `start_x`, where every `\t`
/// runs to the next tab stop. `letter_spacing` follows each character except
/// a tab (which ends exactly on its stop) and the last one.
pub(crate) fn measure_text_tabbed(
    text: &str,
    start_x: f32,
    style: &ComputedStyle,
    tabs: TabStops,
    m: &dyn TextMeasurer,
) -> f32 {
    let ls = style.letter_spacing;
    let mut pen = start_x;
    let mut trailing_ls = 0.0_f32;
    for (i, piece) in text.split('\t').enumerate() {
        if i > 0 {
            pen += tabs.advance(pen);
            trailing_ls = 0.0;
        }
        if !piece.is_empty() {
            // `measure_text_w_styled` не ставит интервал после последнего
            // символа run-а — он следует за каждым символом, кроме последнего
            // во всей строке.
            pen += measure_text_w_styled(piece, style, 0.0, m) + ls;
            trailing_ls = ls;
        }
    }
    pen - start_x - trailing_ls
}

/// Pushes `text` of `seg` as fragments of `line` starting at pen `*x`, and
/// advances `*x`. Each tab becomes a fragment of its own whose
/// `tab_size` is the tab's real advance, so paint draws it with the width
/// layout chose; text between tabs stays one fragment. `src_off` is the byte
/// offset of `text` inside `seg.text`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_tabbed_frags(
    seg: &InlineSegment,
    text: &str,
    src_off: usize,
    pad_l: f32,
    pad_r: f32,
    m: &dyn TextMeasurer,
    line: &mut Vec<InlineFrag>,
    x: &mut f32,
) {
    if text.is_empty() {
        return;
    }
    let style = &seg.style;
    let tabs = TabStops::of(style, m);
    let ls = style.letter_spacing;
    // `[start, end)` byte ranges: a run without tabs, or one tab.
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        if c == '\t' {
            if start < i {
                pieces.push((start, i));
            }
            pieces.push((i, i + 1));
            start = i + 1;
        }
    }
    if start < text.len() {
        pieces.push((start, text.len()));
    }
    let last = pieces.len() - 1;
    for (n, &(s, e)) in pieces.iter().enumerate() {
        let piece = &text[s..e];
        let is_tab = piece == "\t";
        let (width, frag_style) = if is_tab {
            let adv = tabs.advance(*x);
            let mut st = (**style).clone();
            // Paint splits on `\t` only for a positive `tab_size`.
            st.tab_size = adv.max(f32::MIN_POSITIVE);
            (adv, std::sync::Arc::new(st))
        } else {
            let w = measure_text_tabbed(piece, *x, style, tabs, m);
            // A tab follows: the letter-spacing after the last character is
            // part of the run, the tab then starts after it.
            (if n < last { w + ls } else { w }, style.clone())
        };
        line.push(InlineFrag {
            x: *x,
            y_offset: 0.0,
            width,
            text: piece.to_string(),
            style: frag_style,
            padding_left: if n == 0 { pad_l } else { 0.0 },
            padding_right: if n == last { pad_r } else { 0.0 },
            is_element_box: seg.is_element_box,
            effect_scopes: seg.effect_scopes.clone(),
            edge_start: n == 0 && seg.pre_space > 0.0,
            edge_end: n == last && seg.post_space > 0.0,
            img_src: None,
            img_is_lazy: false,
            is_first_line: false,
            source_node: seg.source_node,
            source_char_offset: seg.source_char_offset.saturating_add((src_off + s) as u32),
            bidi_level: seg.bidi_level,
            bidi_isolate: seg.bidi_isolate,
            merged_sources: Vec::new(),
            forced_end: false,
            font_underline: None,
        });
        *x += width;
    }
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
        let small = Arc::new({
            let mut st = (*seg.style).clone();
            let big = seg.style.font_size;
            st.font_size = big * SMALL_CAPS_SCALE;
            if seg.style.vertical_align == VerticalAlign::Baseline {
                // apply_inline_vertical_align даёт y_offset = (line_h − h)/2 − px.
                // Нужно y_offset = (line_h − big)/2 + a·(big − h), откуда
                // px = (big − h)·(0.5 − a) и line_h сокращается.
                let ascent_ratio = m
                    .filter(|_| big > 0.0)
                    .map_or(FALLBACK_ASCENT_RATIO, |m| {
                        m.ascent_px_with_families(big, &seg.style.font_family) / big
                    });
                let delta = big - st.font_size;
                st.vertical_align = VerticalAlign::Length(delta * (0.5 - ascent_ratio));
            }
            st
        });
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
                    effect_scopes: seg.effect_scopes.clone(),
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
                    bidi_scopes: Vec::new(),
                    bidi_isolate: seg.bidi_isolate,
                });
                no_break.push(
                    out.len() > start + 1
                        && !prev_ch.is_some_and(|c: char| is_wrap_whitespace(c)),
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
/// wraps to a fresh line and retries. Each candidate prefix is measured as a
/// whole, so shaped scripts (Indic matras, ligatures) get their real width
/// rather than a per-character sum.
#[allow(clippy::too_many_arguments)]
fn opportunity_break_offset(
    word: &str,
    start: usize,
    opps: &[usize],
    avail_px: f32,
    font_size: f32,
    ls: f32,
    families: &[String],
    axes: &[crate::style::FontVariationSetting],
    m: &dyn TextMeasurer,
) -> usize {
    let width = |end: usize| measure_text_w_varied(&word[start..end], font_size, ls, 0.0, families, axes, m);
    // Last opportunity known to fit; `start` means "nothing fits".
    let mut fit = start;
    let first = opps.partition_point(|&o| o <= start);
    for &pos in &opps[first..] {
        if pos >= word.len() {
            break;
        }
        if width(pos) > avail_px {
            return fit;
        }
        fit = pos;
    }
    if width(word.len()) <= avail_px { word.len() } else { fit }
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
        let tab = seg.style.tab_size;
        for raw in split_css_whitespace(&seg.text) {
            let (display, _) = strip_soft_hyphens(raw);
            let w = measure_text_w_styled(&display, &seg.style, tab, m);
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
        .map(|l| l.iter().map(|f| split_css_whitespace(&f.text).count()).sum())
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
        .split(is_wrap_whitespace)
        .rfind(|w| !w.is_empty())
        .map(|w| {
            let (display, _) = strip_soft_hyphens(w);
            measure_text_w_styled(&display, &last_frag.style, 0.0, m)
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
        .map(|l| l.iter().map(|f| split_css_whitespace(&f.text).count()).sum())
        .unwrap_or(0);
    if trial_last_words >= 2 && trial.len() <= target + 1 {
        trial
    } else {
        greedy_lines
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// CSS Text L3 §7.1: стартовые позиции строк при `text-indent` (с `hanging`/`each-line`).
#[derive(Clone, Copy)]
pub(crate) struct LineIndent {
    /// Отступ первой строки.
    pub(crate) first: f32,
    /// Отступ строки, начатой мягким переносом.
    pub(crate) soft: f32,
    /// Отступ строки после принудительного разрыва (`<br>`, перевод строки).
    pub(crate) after_forced: f32,
}

impl LineIndent {
    /// Только первая строка отступает на `first`.
    pub(crate) fn first_line(first: f32) -> Self {
        Self { first, soft: 0.0, after_forced: 0.0 }
    }

    /// `length` + флаги `hanging`/`each-line`; `inset` — сдвиг первой строки сверх отступа;
    /// `continuation` — прогон идёт после блока в потоке, его первая строка не стартовая
    /// (если не `each-line`).
    pub(crate) fn new(length: f32, hanging: bool, each_line: bool, inset: f32, continuation: bool) -> Self {
        let on = |indented: bool| if indented { length } else { 0.0 };
        let start_line = !continuation || each_line;
        Self {
            first: on(start_line != hanging) + inset,
            soft: on(hanging),
            after_forced: on(each_line != hanging),
        }
    }
}

/// Does UAX #14 (tailored by `line-break`/`word-break`) allow a soft wrap
/// between the end of the segments before `seg_idx` and the start of `word`?
fn tight_boundary_breaks(
    segments: &[InlineSegment],
    seg_idx: usize,
    word: &str,
    line_break: LineBreak,
    word_break: WordBreak,
) -> bool {
    let Some(prev) = segments[..seg_idx].iter().rev().find_map(|s| s.text.chars().next_back()) else {
        return true;
    };
    let Some(next) = word.chars().next() else {
        return true;
    };
    let mut pair = String::with_capacity(8);
    pair.push(prev);
    pair.push(next);
    crate::line_break::break_opportunities(&pair, line_break, word_break).contains(&prev.len_utf8())
}

/// Отступ только у первой строки; см. [`wrap_inline_run_indented`].
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
    wrap_inline_run_indented(
        segments, max_width, container_font_size, LineIndent::first_line(text_indent),
        viewport, m, hyphens, hp, white_space, word_break, overflow_wrap, line_break,
    )
}

/// Margin + border + padding of an inline box under `box-decoration-break: clone` (CSS Fragmentation
/// L3 §5.1): every line fragment of the box repeats both edges, so each line reserves room for them.
#[derive(Clone, Copy)]
struct CloneEdges {
    /// Resolved start margin + border + padding width.
    start_w: f32,
    /// Resolved end margin + border + padding width.
    end_w: f32,
    pad_r: f32,
}

impl CloneEdges {
    /// Edges of the segment's element, `None` when it slices (the default) or has no edges.
    fn of(seg: &InlineSegment, pad_l: f32, pad_r: f32, viewport: Size) -> Option<Self> {
        if !seg.style.box_decoration_clone || !seg.is_element_box {
            return None;
        }
        let em = seg.style.font_size;
        let start_w = seg.style.margin_left.resolve_or_zero(em, 0.0, viewport) + seg.style.border_left_width + pad_l;
        let end_w = seg.style.border_right_width + pad_r + seg.style.margin_right.resolve_or_zero(em, 0.0, viewport);
        (start_w + end_w > 0.0).then_some(Self { start_w, end_w, pad_r })
    }
}

/// Ends the current line inside an inline box. A `clone` box closes the line with its end edge
/// (when the line's last fragment is the box's own) and opens the next with its start edge;
/// `continuing` is false when the word that wraps is the first of the box, which brings its own
/// start edge through `pre_space`.
#[allow(clippy::too_many_arguments)]
fn break_line(
    result: &mut Vec<Vec<InlineFrag>>,
    current_line: &mut Vec<InlineFrag>,
    current_x: &mut f32,
    soft: f32,
    clone: Option<CloneEdges>,
    style: &std::sync::Arc<ComputedStyle>,
    continuing: bool,
    clone_restart: &mut bool,
) {
    let mut x = soft;
    if let Some(c) = clone
        && continuing
    {
        if let Some(last) = current_line.last_mut()
            && std::sync::Arc::ptr_eq(&last.style, style)
        {
            last.edge_end = true;
            last.padding_right = c.pad_r;
        }
        x += c.start_w;
        *clone_restart = true;
    }
    result.push(std::mem::take(current_line));
    *current_x = x;
}

/// Разбивает прогон сегментов на строки, склеивая слова
/// в один `InlineFrag`. Сегменты обрабатываются по одному, чтобы учитывать
/// `pre_space` / `post_space` (inline box model: margin + border + padding).
/// `white_space` controls whether whitespace is preserved (pre/pre-wrap).
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_inline_run_indented(
    segments: &[InlineSegment],
    max_width: f32,
    container_font_size: f32,
    indent: LineIndent,
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
    let mut current_x = indent.first;
    // CSS Text L3 §4.1.1 — whether the previous segment ended with collapsible
    // whitespace, so the first word of the next segment gets one inter-word gap.
    // A segment boundary with no whitespace on either side joins tightly (e.g.
    // `<q>` `::before` open-quote glued to the quoted text, `<a>link</a>!`).
    let mut prev_trailing_ws = false;
    // The previous word ended an inline box with a trailing margin/border/padding: the next word
    // must not be glued to its fragment, or the gap (`current_x += post`) is lost.
    let mut prev_edge = false;
    // pre-wrap / break-spaces: the previous segment ended with a preserved
    // space, i.e. a line may break before this segment's first word.
    let mut pre_wrap_space_before = false;

    for (seg_idx, seg) in segments.iter().enumerate() {
        // Перенос перед первым словом запрещён, когда сегмент — «хвост»
        // разрезанного капителью слова (см. `caps_synthesis`).
        let no_break_before = caps_no_break.get(seg_idx).copied().unwrap_or(false);
        // Forced line break from \n in white-space: pre/pre-wrap text.
        if seg.forced_break {
            if let Some(last) = current_line.last_mut() {
                last.forced_end = true;
                // `box-decoration-break: clone` — a `<br>` inside the box ends the line with its
                // end edge, like a soft wrap does.
                if last.style.box_decoration_clone && last.is_element_box && !last.edge_end && last.img_src.is_none() {
                    last.edge_end = true;
                    last.padding_right = last.style.padding_right.resolve_or_zero(last.style.font_size, max_width, viewport);
                }
            }
            result.push(std::mem::take(&mut current_line));
            current_x = indent.after_forced;
            prev_trailing_ws = false;
            pre_wrap_space_before = false;
            continue;
        }

        // Does this segment's source text carry collapsible whitespace at its
        // edges? Used to decide the boundary gap with the previous segment.
        let seg_lead_ws = seg.text.starts_with(|c: char| is_wrap_whitespace(c));
        let seg_trail_ws = seg.text.ends_with(|c: char| is_wrap_whitespace(c));

        // Pre-mode: whitespace preserved, no word wrapping, tabs are tab_size wide.
        if white_space.preserves_whitespace() {
            if seg.text.is_empty() {
                continue;
            }
            prev_trailing_ws = false;
            // BUG-1322: pre-wrap / break-spaces keep the spaces but still wrap
            // after them; only `pre` is a single unbreakable fragment.
            if !white_space.is_nowrap() {
                let starts_space = seg.text.starts_with([' ', '\t']);
                let params = super::inline_wrap_preserved::PreservedWrap {
                    max_width, viewport, m, white_space, word_break, overflow_wrap, line_break,
                    soft_x: indent.soft,
                    break_before: !no_break_before
                        && pre_wrap_space_before
                        && (white_space == crate::style::WhiteSpace::BreakSpaces || !starts_space),
                };
                super::inline_wrap_preserved::wrap_preserved_segment(
                    seg, &params, &mut result, &mut current_line, &mut current_x,
                );
                pre_wrap_space_before = seg.text.ends_with([' ', '\t']);
                continue;
            }
            let style = &seg.style;
            let em = style.font_size;
            let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_x += seg.pre_space;
            push_tabbed_frags(seg, &seg.text, 0, pad_l, pad_r, m, &mut current_line, &mut current_x);
            current_x += seg.post_space;
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
                current_x = indent.soft;
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
                effect_scopes: seg.effect_scopes.clone(),
                edge_start: true,
                edge_end: true,
                img_src: Some(img_src.clone()),
                img_is_lazy: seg.img_is_lazy,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
                bidi_isolate: seg.bidi_isolate,
                merged_sources: Vec::new(),
                forced_end: false,
                font_underline: None,
            });
            current_x += img_w + seg.post_space;
            // Trailing whitespace after the image (a collapsed ws-only node) is
            // recorded as a trailing space on its alt text by collect_inline_segments.
            prev_trailing_ws = seg_trail_ws;
            continue;
        }

        // An empty piece of an inline box cut by a block (CSS 2.1 §9.2.1.1): no text, but
        // its border/padding still occupy a (zero-width) fragment of the line.
        if seg.text.is_empty() && seg.is_element_box && (seg.pre_space > 0.0 || seg.post_space > 0.0) {
            let style = &seg.style;
            let em = style.font_size;
            current_x += seg.pre_space;
            current_line.push(InlineFrag {
                x: current_x,
                y_offset: 0.0,
                width: 0.0,
                text: String::new(),
                style: style.clone(),
                padding_left: style.padding_left.resolve_or_zero(em, max_width, viewport),
                padding_right: style.padding_right.resolve_or_zero(em, max_width, viewport),
                is_element_box: true,
                effect_scopes: seg.effect_scopes.clone(),
                edge_start: seg.pre_space > 0.0,
                edge_end: seg.post_space > 0.0,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
                bidi_isolate: seg.bidi_isolate,
                merged_sources: Vec::new(),
                forced_end: false,
                font_underline: None,
            });
            current_x += seg.post_space;
            prev_trailing_ws = false;
            continue;
        }

        // Collect words; split_css_whitespace preserves U+00AD within tokens.
        let raw_words: Vec<&str> = split_css_whitespace(&seg.text).collect();
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
        let ws = style.word_spacing + style.word_spacing_pct * space_w;
        let inter_word = space_w + ls + ws;

        // Resolved padding for this segment's inline box (for paint use).
        let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
        let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);
        let clone = CloneEdges::of(seg, pad_l, pad_r, viewport);
        // A `clone` box's start edge was just reserved at the head of a line: the next fragment owns it.
        let mut clone_restart = false;

        let n = raw_words.len();
        for (wi, raw_word) in raw_words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;

            // Strip soft hyphens for display + collect hyphenation break positions.
            let (display_word, shy_positions) = strip_soft_hyphens(raw_word);

            // Byte offset of this word within seg.text — used for Selection/Range mapping.
            // raw_word is a subslice produced by split_css_whitespace(), so pointer arithmetic is valid.
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
            // `box-decoration-break: clone`: a piece of the box that opens a line without the
            // box's own start (a `<br>` or soft wrap cut it) repeats the start edge; every line
            // that keeps going also keeps room for the end edge.
            let continuing = !(is_seg_first && seg.pre_space > 0.0);
            if let Some(c) = clone
                && is_seg_first
                && seg.pre_space == 0.0
                && current_line.is_empty()
                && !clone_restart
            {
                current_x += c.start_w;
                clone_restart = true;
            }
            let end_reserve = match clone {
                Some(c) if !is_seg_last => c.end_w,
                _ => 0.0,
            };

            let word_w = measure_text_w_styled(&display_word, style, 0.0, m);
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
            // Segments joined without white space (`<a>A</a>B`, `<b>x</b>-`) form
            // one word for UAX #14: the boundary breaks only where the pair of
            // characters around it does.
            let tight_join = is_seg_first && word_inter == 0.0 && !current_line.is_empty();
            let base_breakable = !is_seg_first || !no_break_before;
            // `overflow-wrap: anywhere|break-word` and `word-break: break-word` open a
            // wrap point between any two characters, so a seam is never glued then.
            // The segment's own `word-break`/`overflow-wrap` count too: the run-wide
            // ones come from the block container, the span may set its own.
            let seam_wb = if style.word_break == WordBreak::Normal { word_break } else { style.word_break };
            let ow_chars = seam_wb == WordBreak::BreakWord
                || matches!(overflow_wrap, OverflowWrap::BreakWord | OverflowWrap::Anywhere)
                || matches!(style.overflow_wrap, OverflowWrap::BreakWord | OverflowWrap::Anywhere);
            let glued_boundary = tight_join
                && !ow_chars
                && !tight_boundary_breaks(segments, seg_idx, &display_word, line_break, seam_wb);
            let breakable = base_breakable && !glued_boundary;
            // Слово не влезает в остаток строки. Для мягкого переноса и
            // `word-break: break-all` это достаточное условие, в том числе когда
            // слово начинает строку и переполняет контейнер в одиночку
            // (CSS Text L3 §5.2, §6.1).
            let overflows = breakable && current_x + gap + pre + word_w + end_reserve > max_width;
            // Wrap: слово не влезает (но первое слово строки добавляем всегда).
            let needs_wrap = !current_line.is_empty() && overflows;

            // CSS Text L3 §5.5 `line-break` — soft wrap opportunities *inside*
            // the word. CJK text carries no spaces, so `split_css_whitespace` hands
            // us whole paragraphs here; without this the run would either
            // overflow the container or be pushed onto a line of its own.
            // Only relevant when the word does not fit as-is. The opportunities
            // are UAX #14's (hyphens, dashes, ZWSP, ideographs, SA scripts),
            // tailored by `line-break` and `word-break`.
            let lb_avail = max_width - current_x - gap - pre - end_reserve;
            let lb_opps = if base_breakable && word_w > lb_avail {
                crate::line_break::break_opportunities(&display_word, line_break, word_break)
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
                    let avail = (max_width - current_x - chunk_gap - chunk_pre - end_reserve).max(0.0);
                    let mut end = opportunity_break_offset(
                        &display_word, start, &lb_opps, avail,
                        style.font_size, ls, &style.font_family, &style.font_variation_settings, m,
                    );
                    if end == start {
                        if !current_line.is_empty() && !(glued_boundary && first_chunk) {
                            // Nothing fits in what is left of this line — wrap
                            // and measure again against the full width.
                            let cont = continuing || !first_chunk;
                            break_line(
                                &mut result, &mut current_line, &mut current_x, indent.soft,
                                clone, style, cont, &mut clone_restart,
                            );
                            continue;
                        }
                        // The line is empty and even the shortest chunk
                        // overflows: emit it anyway so the loop terminates.
                        end = lb_opps
                            .iter()
                            .copied()
                            .find(|&o| o > start)
                            .unwrap_or(display_word.len());
                        // CSS Text L3 §8.1: the other opportunities are used
                        // first; a chunk still wider than the line is cut by
                        // characters under `overflow-wrap: break-word|anywhere`.
                        if ow_chars {
                            let whole = &display_word[start..end];
                            let w = measure_text_w_styled(whole, style, 0.0, m);
                            if w > avail {
                                end = start + char_break_offset(whole, avail, style.font_size, ls, &style.font_family, m);
                            }
                        }
                    }
                    let chunk = &display_word[start..end];
                    let chunk_w = measure_text_w_styled(chunk, style, 0.0, m);
                    current_x += chunk_gap + chunk_pre;
                    let cs = std::mem::take(&mut clone_restart);
                    current_line.push(InlineFrag {
                        x: current_x,
                        y_offset: 0.0,
                        width: chunk_w,
                        text: chunk.to_string(),
                        style: style.clone(),
                        padding_left: if cs || (first_chunk && is_seg_first) { pad_l } else { 0.0 },
                        padding_right: if end == display_word.len() && is_seg_last { pad_r } else { 0.0 },
                        is_element_box: seg.is_element_box,
                        effect_scopes: seg.effect_scopes.clone(),
                        edge_start: cs || (first_chunk && is_seg_first && pre > 0.0),
                        edge_end: end == display_word.len() && is_seg_last && post > 0.0,
                        img_src: None,
                        img_is_lazy: false,
                        is_first_line: false,
                        source_node: seg.source_node,
                        // Soft hyphens (stripped from `display_word`) would shift
                        // this; CJK text does not use them.
                        source_char_offset: frag_source_offset.saturating_add(start as u32),
                        bidi_level: seg.bidi_level,
                        bidi_isolate: seg.bidi_isolate,
                        merged_sources: Vec::new(),
                        forced_end: false,
                        font_underline: None,
                    });
                    current_x += chunk_w;
                    first_chunk = false;
                    start = end;
                    if start < display_word.len() {
                        break_line(
                            &mut result, &mut current_line, &mut current_x, indent.soft,
                            clone, style, true, &mut clone_restart,
                        );
                    }
                }
                current_x += post;
                prev_edge = post > 0.0;
                continue;
            }

            if overflows {
                // CSS Text L3 §6: try hyphenation before hard wrap.
                let hyph_result = if hyphens != Hyphens::None {
                    let mut break_pts = shy_positions.clone();
                    if hyphens == Hyphens::Auto && !display_word.is_empty() {
                        let auto_pts = hp.hyphenate(&display_word, "");
                        break_pts.extend_from_slice(&auto_pts);
                        break_pts.sort_unstable();
                        break_pts.dedup();
                    }
                    let avail = max_width - current_x - gap - pre - end_reserve;
                    try_hyp_break(&display_word, avail, style.font_size, ls, m, &break_pts)
                } else {
                    None
                };

                if let Some((pfx, sfx)) = hyph_result {
                    // Emit prefix (with trailing '-') to current line, then wrap.
                    let pfx_w = measure_text_w_styled(&pfx, style, 0.0, m);
                    current_x += gap + pre;
                    let cs = std::mem::take(&mut clone_restart);
                    current_line.push(InlineFrag {
                        x: current_x,
                        y_offset: 0.0,
                        width: pfx_w,
                        text: pfx,
                        style: style.clone(),
                        padding_left: if is_seg_first || cs { pad_l } else { 0.0 },
                        padding_right: 0.0,
                        is_element_box: seg.is_element_box,
                        effect_scopes: seg.effect_scopes.clone(),
                        edge_start: cs || (is_seg_first && pre > 0.0),
                        edge_end: false,
                        img_src: None,
                        img_is_lazy: false,
                        is_first_line: false,
                        source_node: seg.source_node,
                        source_char_offset: frag_source_offset,
                        bidi_level: seg.bidi_level,
                        bidi_isolate: seg.bidi_isolate,
                        merged_sources: Vec::new(),
                        forced_end: false,
                        font_underline: None,
                    });
                    break_line(
                        &mut result, &mut current_line, &mut current_x, indent.soft,
                        clone, style, true, &mut clone_restart,
                    );
                    // Emit suffix as first fragment on new line.
                    let sfx_w = measure_text_w_styled(&sfx, style, 0.0, m);
                    let cs = std::mem::take(&mut clone_restart);
                    current_line.push(InlineFrag {
                        x: current_x - indent.soft,
                        y_offset: 0.0,
                        width: sfx_w,
                        text: sfx,
                        style: style.clone(),
                        padding_left: if cs { pad_l } else { 0.0 },
                        padding_right: if is_seg_last { pad_r } else { 0.0 },
                        is_element_box: seg.is_element_box,
                        effect_scopes: seg.effect_scopes.clone(),
                        edge_start: cs,
                        edge_end: is_seg_last && post > 0.0,
                        img_src: None,
                        img_is_lazy: false,
                        is_first_line: false,
                        source_node: seg.source_node,
                        source_char_offset: frag_source_offset,
                        bidi_level: seg.bidi_level,
                        bidi_isolate: seg.bidi_isolate,
                        merged_sources: Vec::new(),
                        forced_end: false,
                        font_underline: None,
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
                            let head_w = measure_text_w_styled(head, style, 0.0, m);
                            let cs = std::mem::take(&mut clone_restart);
                            current_line.push(InlineFrag {
                                x: current_x,
                                y_offset: 0.0,
                                width: head_w,
                                text: head.to_string(),
                                style: style.clone(),
                                padding_left: if cs || (first_chunk && is_seg_first) { pad_l } else { 0.0 },
                                padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                                is_element_box: seg.is_element_box,
                                effect_scopes: seg.effect_scopes.clone(),
                                edge_start: cs || (first_chunk && is_seg_first && pre > 0.0),
                                edge_end: tail.is_empty() && is_seg_last && post > 0.0,
                                img_src: None,
                                img_is_lazy: false,
                                is_first_line: false,
                                source_node: seg.source_node,
                                source_char_offset: frag_source_offset,
                                bidi_level: seg.bidi_level,
                                bidi_isolate: seg.bidi_isolate,
                                merged_sources: Vec::new(),
                                forced_end: false,
                                font_underline: None,
                            });
                            current_x += head_w;
                            first_chunk = false;
                        }
                        rest = tail;
                        if !rest.is_empty() {
                            break_line(
                                &mut result, &mut current_line, &mut current_x, indent.soft,
                                clone, style, true, &mut clone_restart,
                            );
                        }
                    }
                    current_x += post;
                    prev_edge = post > 0.0;
                    continue;
                }

                // No hyphenation break found — normal wrap. A word that starts the
                // line has nowhere to wrap to: it stays and overflows.
                if needs_wrap {
                    break_line(
                        &mut result, &mut current_line, &mut current_x, indent.soft,
                        clone, style, continuing, &mut clone_restart,
                    );
                }
            }

            // CSS Text L3 §8.1: overflow-wrap: break-word / anywhere — char-break
            // words that are wider than the container (won't fit on any line).
            // word-break: break-word is a legacy alias for overflow-wrap: break-word.
            let ow_char_break = ow_chars && word_w > max_width;
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
                        let head_w = measure_text_w_styled(head, style, 0.0, m);
                        let cs = std::mem::take(&mut clone_restart);
                        current_line.push(InlineFrag {
                            x: current_x,
                            y_offset: 0.0,
                            width: head_w,
                            text: head.to_string(),
                            style: style.clone(),
                            padding_left: if cs || (first_chunk && is_seg_first) { pad_l } else { 0.0 },
                            padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                            is_element_box: seg.is_element_box,
                            effect_scopes: seg.effect_scopes.clone(),
                            edge_start: cs || (first_chunk && is_seg_first && pre > 0.0),
                            edge_end: tail.is_empty() && is_seg_last && post > 0.0,
                            img_src: None,
                            img_is_lazy: false,
                            is_first_line: false,
                            source_node: seg.source_node,
                            source_char_offset: frag_source_offset,
                            bidi_level: seg.bidi_level,
                            bidi_isolate: seg.bidi_isolate,
                            merged_sources: Vec::new(),
                            forced_end: false,
                            font_underline: None,
                        });
                        current_x += head_w;
                        first_chunk = false;
                    }
                    rest = tail;
                    if !rest.is_empty() {
                        break_line(
                            &mut result, &mut current_line, &mut current_x, indent.soft,
                            clone, style, true, &mut clone_restart,
                        );
                    }
                }
                current_x += post;
                prev_edge = post > 0.0;
                continue;
            }

            let line_gap = if current_line.is_empty() { 0.0 } else { word_inter };
            current_x += line_gap + pre;
            let frag_x = current_x;

            // Слияние: только когда нет pre/post space у данного слова
            // и предыдущий фраг тоже не заканчивается inline-box-ом.
            let no_box = pre == 0.0 && post == 0.0 && !prev_edge;
            let merged = if no_box {
                if let Some(last) = current_line.last_mut() {
                    // Fragments at different UAX #9 embedding levels must stay
                    // apart: L2 reorders and reverses per fragment, so merging
                    // an RTL word with its LTR neighbour would reverse both.
                    if last.style.text_rendering_eq(style)
                        && last.padding_right == 0.0
                        && last.bidi_level == seg.bidi_level
                        && last.effect_scopes == seg.effect_scopes
                        && last.effect_scopes == seg.effect_scopes
                    {
                        let prev_end_x = last.width;
                        // No separating space when the boundary joined tightly
                        // (word_inter == 0): the glyphs abut, e.g. `“`+`auto`.
                        if word_inter > 0.0 {
                            last.text.push(' ');
                        }
                        // GAP-HLHITTEST: a word from another DOM text node
                        // still shares this fragment (one `DrawText` for
                        // paint), but geometry needs to know where it starts.
                        let owner = last
                            .merged_sources
                            .last()
                            .map_or(last.source_node, |m| m.source_node);
                        if owner != seg.source_node {
                            last.merged_sources.push(crate::box_tree::MergedSource {
                                text_byte: last.text.len() as u32,
                                x: prev_end_x + word_inter,
                                prev_end_x,
                                source_node: seg.source_node,
                                source_char_offset: frag_source_offset,
                            });
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
                let cs = std::mem::take(&mut clone_restart);
                current_line.push(InlineFrag {
                    x: frag_x,
                    y_offset: 0.0,
                    width: word_w,
                    text: display_word,
                    style: style.clone(),
                    padding_left: if is_seg_first || cs { pad_l } else { 0.0 },
                    padding_right: if is_seg_last { pad_r } else { 0.0 },
                    is_element_box: seg.is_element_box,
                    effect_scopes: seg.effect_scopes.clone(),
                    edge_start: cs || (is_seg_first && pre > 0.0),
                    edge_end: is_seg_last && post > 0.0,
                    img_src: None,
                    img_is_lazy: false,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: frag_source_offset,
                    bidi_level: seg.bidi_level,
                    bidi_isolate: seg.bidi_isolate,
                    merged_sources: Vec::new(),
                    forced_end: false,
                    font_underline: None,
                });
                current_x += word_w;
            }

            current_x += post;
            prev_edge = post > 0.0;
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
/// Последняя строка блока и строка перед принудительным переносом (`<br>`) выравниваются
/// по `text_align_last` (CSS Text L3 §7.2): `Auto` на justify-блоке → Start; иначе → как text_align.
pub(crate) fn align_lines(
    lines: &mut [Vec<InlineFrag>],
    content_width: f32,
    text_align: TextAlign,
    text_align_last: TextAlignLast,
    direction: Direction,
) {
    let total = lines.len();
    for (idx, line) in lines.iter_mut().enumerate() {
        let is_last = idx + 1 == total || line.last().is_some_and(|f| f.forced_end);
        align_one_line(line, is_last, content_width, text_align, text_align_last, direction);
    }
}

/// One iteration of [`align_lines`]: reorder + align a single line box.
/// `is_last` selects `text-align-last`; `content_width` is the width of *this*
/// line box (a line shortened by a float aligns inside its own band).
pub(crate) fn align_one_line(
    line: &mut [InlineFrag],
    is_last: bool,
    content_width: f32,
    text_align: TextAlign,
    text_align_last: TextAlignLast,
    direction: Direction,
) {
    let is_rtl = direction == Direction::Rtl;
    {
        // CSS Text L3 §7.2: last line uses text-align-last.
        // Auto → text-align, но `justify` последней строки — это `start` (§7.1).
        let effective = if is_last {
            match text_align_last {
                TextAlignLast::Auto    => if text_align == TextAlign::Justify { TextAlign::Start } else { text_align },
                TextAlignLast::Left    => TextAlign::Left,
                TextAlignLast::Right   => TextAlign::Right,
                TextAlignLast::Center  => TextAlign::Center,
                TextAlignLast::Start   => TextAlign::Start,
                TextAlignLast::End     => TextAlign::End,
                TextAlignLast::Justify => TextAlign::Justify,
                TextAlignLast::MatchParent => TextAlign::Start,
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
        // `justify` без возможностей растяжения (одно слово, `text-justify: none`) ведёт себя как `start`.
        let physical = if physical == TextAlign::Justify {
            super::inline_justify::justify_line(line, content_width);
            if is_rtl { TextAlign::Right } else { TextAlign::Left }
        } else {
            physical
        };
        // Measured before reordering, while `wrap_inline_run`'s ascending-x
        // order still holds, so the last frag is the rightmost one.
        let Some(last_frag) = line.last() else { return };
        let line_width = last_frag.x + last_frag.width;
        // UAX #9 L2 — logical → visual placement. Subsumes the RTL line mirror
        // `align_lines` used to do itself, but level-aware, so an LTR island
        // inside an RTL paragraph keeps its own left-to-right order.
        crate::bidi::reorder_line(line, direction);
        // CSS Text L3 §7.1: a line too long for its box is start-aligned and overflows the end
        // edge — in an RTL paragraph that is the left one, so the line starts at the right edge.
        let offset = if is_rtl && line_width > content_width {
            content_width - line_width
        } else {
            match physical {
                TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
                TextAlign::Right  => (content_width - line_width).max(0.0),
                _                 => 0.0,
            }
        };
        if offset != 0.0 {
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

/// Прирост блока из-за боксов, вынесенных `vertical-align` за строку (CSS 2.1 §10.8.1):
/// `top` — выше строки 0, `bottom` — ниже последней строки.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct LineGrowth {
    pub top: f32,
    pub bottom: f32,
}

/// CSS 2.1 §10.8.1 — высота и baseline строк текстового рана по их фрагментам (BUG-1281).
///
/// Inline-бокс фрагмента с `vertical-align: baseline` тянется от baseline вверх на
/// `ascent + half-leading` и вниз на `descent + half-leading` СВОЕГО шрифта; line box доходит
/// до самого высокого верха и самого низкого низа среди них и strut-а блока. Раскладка у
/// рана остаётся равномерной (paint, selection и hit-test умножают индекс строки на
/// `used_line_height`), поэтому общая высота строки — максимум по строкам рана, а строка
/// короче максимума получает запас снизу. Ascent берётся тем же способом, каким paint ставит
/// baseline в прямоугольнике текста (`ascent_px_with_families`), так что фрагменты разных
/// кеглей садятся на одну базовую линию.
pub(crate) struct BaselineFit<'a> {
    m: &'a dyn TextMeasurer,
    strut: &'a ComputedStyle,
    strut_ascent: f32,
    /// `used_line_height` блока без роста — от него отсчитывается `grow`.
    pub(crate) base: f32,
    /// Насколько самая высокая строка рана выше `base` (0.0 — строки не выросли).
    pub(crate) grow: f32,
    /// Опускание baseline строки относительно baseline strut-а блока.
    shifts: Vec<f32>,
}

impl<'a> BaselineFit<'a> {
    pub(crate) fn new(
        lines: &[Vec<InlineFrag>],
        strut: &'a ComputedStyle,
        base: f32,
        m: &'a dyn TextMeasurer,
    ) -> Self {
        let fs = strut.font_size;
        let strut_ascent = m.ascent_px_with_families(fs, &strut.font_family);
        let strut_lead = (base - fs) / 2.0;
        let strut_top = strut_ascent + strut_lead;
        let strut_bottom = fs - strut_ascent + strut_lead;
        let mut fit = Self { m, strut, strut_ascent, base, grow: 0.0, shifts: Vec::new() };
        let mut tallest = 0.0_f32;
        for line in lines {
            let (mut top, mut bottom) = (strut_top, strut_bottom);
            for frag in line {
                // Картинки и пустые куски боксов — высота неизвестна; не-baseline
                // выравнивание от baseline не зависит.
                if frag.img_src.is_some()
                    || frag.text.is_empty()
                    || frag.style.vertical_align != VerticalAlign::Baseline
                    || fit.same_font_as_strut(&frag.style)
                {
                    continue;
                }
                let ffs = frag.style.font_size;
                let ascent = fit.ascent_of(&frag.style);
                let lead = (super::entry::used_line_height_px(&frag.style, m) - ffs) / 2.0;
                top = top.max(ascent + lead);
                bottom = bottom.max(ffs - ascent + lead);
            }
            fit.shifts.push(top - strut_top);
            tallest = tallest.max(top + bottom);
        }
        // Погрешность суммы `ascent + descent` против `base` не должна растить блок.
        if tallest > base + 0.01 {
            fit.grow = tallest - base;
        }
        fit
    }

    fn same_font_as_strut(&self, st: &ComputedStyle) -> bool {
        st.font_size == self.strut.font_size
            && st.line_height == self.strut.line_height
            && st.line_height_is_normal == self.strut.line_height_is_normal
            && st.font_family == self.strut.font_family
    }

    fn ascent_of(&self, st: &ComputedStyle) -> f32 {
        if self.same_font_as_strut(st) {
            self.strut_ascent
        } else {
            self.m.ascent_px_with_families(st.font_size, &st.font_family)
        }
    }
}

/// Вертикальная раскладка строк текстового рана целиком (CSS Rhythmic Sizing L1 §2 для шага
/// строки + CSS 2.1 §10.8.1 для роста по фрагментам, BUG-1281). Вынесена из `lay_out` в
/// отдельную функцию, чтобы не раздувать её кадр: глубокие цепочки боксов упираются в стек.
///
/// При `fit_fragments` строки подгоняются под фрагменты других кеглей, а `used_line_height`
/// рана перезаписывается высотой самой высокой строки; база берётся свежей из стиля, а не из
/// `used_line_height` — тот мог вырасти в прошлом проходе раскладки этого же бокса.
#[inline(never)]
pub(crate) fn align_run_vertically(
    lines: &mut [Vec<InlineFrag>],
    s: &ComputedStyle,
    used_line_height: &mut f32,
    m: &dyn TextMeasurer,
    grow: bool,
    fit_fragments: bool,
) -> LineGrowth {
    let fit = (grow && fit_fragments)
        .then(|| BaselineFit::new(lines, s, super::entry::used_line_height_px(s, m), m));
    if let Some(f) = &fit {
        *used_line_height = f.base + f.grow;
    }
    let line_h = step_line_height(*used_line_height, s.line_height_step);
    let (marks_top, marks_bottom) = if grow { emphasis_extra(lines, line_h) } else { (0.0, 0.0) };
    if marks_top + marks_bottom > 0.0 {
        // База свежая, не из `used_line_height`: тот мог вырасти в прошлом проходе.
        let base = fit.as_ref().map_or_else(|| super::entry::used_line_height_px(s, m), |f| f.base + f.grow);
        *used_line_height = base + marks_top + marks_bottom;
    }
    let growth = apply_inline_vertical_align(lines, line_h, grow, fit.as_ref());
    if marks_top > 0.0 {
        for frag in lines.iter_mut().flatten() {
            if !matches!(frag.style.vertical_align, VerticalAlign::Top | VerticalAlign::TextTop) {
                frag.y_offset += marks_top;
            }
        }
    }
    growth
}

/// CSS Text Decoration L3 §4.2 — метки `text-emphasis` занимают место в строке, если полуинтерлиньяжа
/// (`line-height` − кегль) / 2 им не хватает: строка растёт сверху для `over` и снизу для `under`
/// (в ruby-эталонах WPT `text-emphasis-line-height-*` метка — как `rt` в 50% кегля). Возвращает
/// `(сверху, снизу)` — наибольшую нехватку по фрагментам рана; раскладка равномерная, поэтому рост
/// одинаков у всех строк. Вертикальное письмо сюда не попадает (его ран раскладывается отдельно).
fn emphasis_extra(lines: &[Vec<InlineFrag>], line_h: f32) -> (f32, f32) {
    let (mut top, mut bottom) = (0.0_f32, 0.0_f32);
    for frag in lines.iter().flatten() {
        let st = &frag.style;
        if st.text_emphasis_style.is_none()
            || frag.img_src.is_some()
            || frag.text.is_empty()
            || st.writing_mode != crate::style::WritingMode::HorizontalTb
        {
            continue;
        }
        let need = (crate::style::TextEmphasisStyle::mark_size(st.font_size) - (line_h - st.font_size) / 2.0).max(0.0);
        if st.text_emphasis_position.is_over() {
            top = top.max(need);
        } else {
            bottom = bottom.max(need);
        }
    }
    (top, bottom)
}

/// CSS 2.1 §10.8 — применяет вертикальное выравнивание к inline-фрагментам.
/// Записывает `y_offset` (смещение от верхнего края line-box, вниз — положительное).
/// `line_h` = font_size * line_height контейнера (при `fit` — с учётом роста строк).
///
/// `fit` (BUG-1281): baseline-фрагменты садятся на общую базовую линию строки, а не
/// центрируются; сдвиг строки из-за более высокого соседа получают и displaced-фрагменты.
///
/// Half-leading (§10.8.1): когда line-height > content-area, разница делится пополам
/// и добавляется выше и ниже content-area. Для `baseline` — фрагмент сдвигается вниз
/// на `half_leading = (line_h - frag_h) / 2`, чтобы content-area была центрирована.
///
/// Рост строки (§10.8.1: line box доходит до верха самого высокого и низа самого низкого
/// inline-бокса): модель раскладки — равномерные строки по `line_h`, поэтому растёт
/// блок целиком. `top` берётся по строке 0, `bottom` — по последней строке; все строки
/// сдвигаются вниз на `top`, `top`/`bottom`-выровненные фрагменты липнут к выросшему
/// краю своей строки. Рост в середине абзаца по-прежнему наезжает на соседние строки.
/// `grow = false` (строки уложены вокруг float-ов) рост не считает.
pub(crate) fn apply_inline_vertical_align(
    lines: &mut [Vec<InlineFrag>],
    line_h: f32,
    grow: bool,
    fit: Option<&BaselineFit<'_>>,
) -> LineGrowth {
    let mut growth = LineGrowth::default();
    let last = lines.len().saturating_sub(1);
    for (idx, line) in lines.iter_mut().enumerate() {
        for frag in line.iter_mut() {
            // frag_h: content area height ≈ font-size (ascent + descent for normal line-height).
            let frag_h = frag.style.font_size;
            // CSS 2.1 §10.8.1: half-leading pushes content area away from line-box edges.
            // Отрицательное при line-height < content area — глиф выступает над строкой.
            let mut half_leading = (line_h - frag_h) / 2.0;
            let mut baseline_y = half_leading;
            if let Some(f) = fit {
                // Строка опущена из-за более высокого соседа; запас роста — снизу строки.
                let adj = f.shifts[idx] - f.grow / 2.0;
                half_leading += adj;
                baseline_y = (line_h - f.strut.font_size) / 2.0 + adj
                    + (f.strut_ascent - f.ascent_of(&frag.style));
            }
            // sub/super не растят строку: их сдвиг — эвристика от кегля, а не метрика шрифта.
            let displaced = matches!(
                frag.style.vertical_align,
                VerticalAlign::Length(_) | VerticalAlign::Percent(_)
            );
            frag.y_offset = match frag.style.vertical_align {
                // Baseline: content area centred via half-leading (top = half_leading).
                VerticalAlign::Baseline => baseline_y,
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
                // CSS 2.1 §10.8.1: a percentage is of the element's OWN `line-height`, not of
                // the block's line (a span with `line-height: 2` in a 16px block, BUG-1545).
                VerticalAlign::Percent(p) => {
                    let own_lh = if frag.style.line_height_is_normal { line_h } else { frag_h * frag.style.line_height };
                    half_leading - (p / 100.0 * own_lh)
                }
            };
            // Картинки и пустые куски боксов: высота inline-бокса неизвестна (frag_h —
            // приближение по кеглю), поэтому рост строки они не вызывают.
            if grow && displaced && frag.img_src.is_none() && !frag.text.is_empty() {
                // Inline-бокс фрагмента — его собственный line-height вокруг content area.
                let lead = if frag.style.line_height_is_normal {
                    0.0
                } else {
                    (frag_h * frag.style.line_height - frag_h) / 2.0
                };
                if idx == 0 {
                    growth.top = growth.top.max(lead - frag.y_offset);
                }
                if idx == last {
                    growth.bottom = growth.bottom.max(frag.y_offset + frag_h + lead - line_h);
                }
            }
        }
    }
    if growth.top == 0.0 && growth.bottom == 0.0 {
        return growth;
    }
    for (idx, line) in lines.iter_mut().enumerate() {
        // Верх строки idx после сдвига всех строк на `top`; строка 0 выросла вверх на `top`.
        let grown_top = if idx == 0 { growth.top } else { 0.0 };
        let grown_h = line_h + grown_top + if idx == last { growth.bottom } else { 0.0 };
        for frag in line.iter_mut() {
            let frag_h = frag.style.font_size;
            frag.y_offset += growth.top;
            match frag.style.vertical_align {
                VerticalAlign::Top | VerticalAlign::TextTop => frag.y_offset -= grown_top,
                VerticalAlign::Bottom | VerticalAlign::TextBottom => {
                    frag.y_offset = growth.top - grown_top + (grown_h - frag_h).max(0.0);
                }
                VerticalAlign::Middle => {
                    frag.y_offset = growth.top - grown_top + ((grown_h - frag_h) / 2.0).max(0.0);
                }
                _ => {}
            }
        }
    }
    growth
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
                effect_scopes: seg.effect_scopes.clone(),
                edge_start: false,
                edge_end: false,
                img_src: Some(img_src.clone()),
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
                bidi_isolate: seg.bidi_isolate,
                merged_sources: Vec::new(),
                forced_end: false,
                font_underline: None,
            });
            prev_trailing_ws = seg.text.ends_with(|c: char| is_wrap_whitespace(c));
            continue;
        }
        let seg_lead_ws = seg.text.starts_with(|c: char| is_wrap_whitespace(c));
        let seg_trail_ws = seg.text.ends_with(|c: char| is_wrap_whitespace(c));
        let text: String = split_css_whitespace(&seg.text).collect::<Vec<_>>().join(" ");
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
                && last.effect_scopes == seg.effect_scopes
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
                effect_scopes: seg.effect_scopes.clone(),
                edge_start: false,
                edge_end: false,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
                bidi_isolate: seg.bidi_isolate,
                merged_sources: Vec::new(),
                forced_end: false,
                font_underline: None,
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
/// Forces an ellipsis (U+2026) onto the *last* visible line of a clamped container to
/// signal omitted content (the container itself cuts the lines, see `line_clamp_flow`).
/// The ellipsis is appended to the last fragment if the line fits within `max_width`,
/// or replaces overflowing text if the line is already too wide.
///
/// Called only when a text measurer is available (same guard as `text-overflow: ellipsis`).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn ellipsize_last_line(
    last: &mut Vec<InlineFrag>,
    max_width: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
    rtl: bool,
) {
    let ellipsis = '\u{2026}';
    let ellipsis_w = m.char_width(ellipsis, font_size);
    if last.is_empty() {
        return;
    }
    // A right-to-left line ends on its left: the ellipsis is a frag of its own (paragraph
    // level, so bidi keeps it left of the text) in the room the right-aligned text leaves.
    if rtl {
        let first = last.iter().min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
        if first.x + 0.01 >= ellipsis_w {
            let mut e = first.clone();
            e.x = first.x - ellipsis_w;
            e.width = ellipsis_w;
            e.text = ellipsis.to_string();
            e.padding_left = 0.0;
            e.padding_right = 0.0;
            e.is_element_box = false;
            e.img_src = None;
            e.img_is_lazy = false;
            e.bidi_level = 1;
            e.merged_sources.clear();
            last.push(e);
            return;
        }
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

/// Заполняет [`InlineFrag::font_underline`] (BUG-1712): метрики подчёркивания шрифта нужны paint
/// только фрагментам с `underline`, у которых позиция или толщина стоят в `from-font`
/// (CSS Text Decoration L4 §3.4, §5.1). Остальным не платим за запрос к измерителю.
pub(crate) fn annotate_font_underline(lines: &mut [Vec<InlineFrag>], m: &dyn TextMeasurer) {
    for frag in lines.iter_mut().flatten() {
        let st = &frag.style;
        if !st.text_decoration_line.underline
            || (st.text_underline_position != crate::style::TextUnderlinePosition::FromFont
                && st.text_decoration_thickness != crate::style::TextDecorationThickness::FromFont)
        {
            continue;
        }
        let axes = crate::style::text_variation_axes(st);
        frag.font_underline = m.underline_metrics_with_families(st.font_size, &st.font_family, &axes);
    }
}
