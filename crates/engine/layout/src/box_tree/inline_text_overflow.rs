//! `text-overflow: <string>` на готовых строках (CSS UI L4 §6.2).
//!
//! Строки приходят в логическом порядке с визуальными `x` (`bidi::reorder_line` уже отработал),
//! поэтому отсекается *визуальный* край: у LTR-абзаца правый, у RTL-абзаца левый, а внутри
//! фрагмента нечётного уровня оставляемая часть — это конец или начало логического текста.
//! Сама метка — отдельный абзац UAX #9 с направлением блока (тесты `text-overflow-string-005…008`).

use super::types::InlineFrag;
use crate::bidi;
use crate::style::{ComputedStyle, Direction, TextEmphasisStyle, VerticalAlign};
use crate::TextMeasurer;
use std::sync::Arc;

/// Ширина символа с учётом семейств и `letter-spacing`, как её считает перенос строк.
fn char_w(m: &dyn TextMeasurer, ch: char, st: &ComputedStyle) -> f32 {
    m.char_width_with_families(ch, st.font_size, &st.font_family) + st.letter_spacing
}

/// Оставляет в `f` символы, помещающиеся в `room` px от его визуального края: левого, если
/// `from_right == false`, иначе правого. Возвращает `false`, если не осталось ни одного
/// (при `force_one` остаётся хотя бы один). Отступы по краю обрезки обнуляются: срез —
/// конец инлайн-бокса.
fn truncate_frag_visual(
    f: &mut InlineFrag,
    room: f32,
    from_right: bool,
    force_one: bool,
    m: &dyn TextMeasurer,
) -> bool {
    if f.img_src.is_some() || f.text.is_empty() {
        return false;
    }
    // Визуально левый край нечётного уровня — конец логического текста, правый — начало.
    let odd = f.bidi_level % 2 == 1;
    let from_logical_start = from_right == odd;
    let chars: Vec<char> = f.text.chars().collect();
    let widths: Vec<f32> = chars.iter().map(|&ch| char_w(m, ch, &f.style)).collect();
    let mut kept = 0usize;
    let mut w = 0.0_f32;
    for k in 0..chars.len() {
        let cw = if from_logical_start { widths[k] } else { widths[chars.len() - 1 - k] };
        if w + cw > room {
            break;
        }
        w += cw;
        kept += 1;
    }
    if kept == 0 {
        if !force_one {
            return false;
        }
        kept = 1;
        w = if from_logical_start { widths[0] } else { widths[chars.len() - 1] };
    }
    let (new_text, cut_bytes): (String, usize) = if from_logical_start {
        (chars[..kept].iter().collect(), 0)
    } else {
        let head: String = chars[..chars.len() - kept].iter().collect();
        (chars[chars.len() - kept..].iter().collect(), head.len())
    };
    f.text = new_text;
    f.source_char_offset += cut_bytes as u32;
    let old_right = f.x + f.width;
    if from_right {
        f.width = f.padding_right + w;
        f.x = old_right - f.width;
        f.padding_left = 0.0;
    } else {
        f.width = f.padding_left + w;
        f.padding_right = 0.0;
    }
    f.merged_sources.clear();
    true
}

/// Метка как абзац UAX #9 с направлением блока: фрагменты по уровням, уже в визуальном порядке
/// и с `x` от нуля. `template` даёт источник и прочие поля; стиль — стиль блока без
/// `text-emphasis`.
fn marker_frags(
    marker: &str,
    rtl: bool,
    template: &InlineFrag,
    style: &Arc<ComputedStyle>,
    y_offset: f32,
    m: &dyn TextMeasurer,
) -> Vec<InlineFrag> {
    let dir = if rtl { Direction::Rtl } else { Direction::Ltr };
    let mut x = 0.0_f32;
    let mut out: Vec<InlineFrag> = Vec::new();
    for (piece, level) in bidi::split_by_level(marker, dir) {
        let width: f32 = piece.chars().map(|ch| char_w(m, ch, style)).sum();
        let mut e = template.clone();
        e.style = style.clone();
        e.text = piece.to_string();
        e.x = x;
        e.width = width;
        e.y_offset = y_offset;
        e.padding_left = 0.0;
        e.padding_right = 0.0;
        e.is_element_box = false;
        e.img_src = None;
        e.img_is_lazy = false;
        e.bidi_level = level;
        e.bidi_isolate = None;
        e.merged_sources.clear();
        x += width;
        out.push(e);
    }
    bidi::reorder_line(&mut out, dir);
    out
}

/// CSS UI L4 §6.2 — `text-overflow: "<string>"`: строки шире `max_width` теряют конец,
/// на его место встаёт `marker`. В отличие от «…», который дописывается в стиль последнего
/// фрагмента, метка — отдельный фрагмент со стилем блока-контейнера (§6.2: метка стилизуется
/// как блок; `text-emphasis` на неё не действует). Метка шире строки обрезается краем бокса
/// (клип у `overflow`); первый символ строки остаётся всегда.
///
/// `rtl` — направление абзаца: строка RTL выходит за левый край, метка встаёт слева от
/// оставшейся части.
pub(crate) fn apply_text_overflow_string(
    lines: &mut [Vec<InlineFrag>],
    max_width: f32,
    marker: &str,
    block_style: &Arc<ComputedStyle>,
    line_h: f32,
    rtl: bool,
    m: &dyn TextMeasurer,
) {
    let marker_w: f32 = marker.chars().map(|ch| char_w(m, ch, block_style)).sum();
    let mut marker_style: Option<Arc<ComputedStyle>> = None;
    for line in lines.iter_mut() {
        if line.is_empty() {
            continue;
        }
        let lo = line.iter().map(|f| f.x).fold(f32::INFINITY, f32::min);
        let hi = line.iter().map(|f| f.x + f.width).fold(f32::NEG_INFINITY, f32::max);
        if if rtl { lo >= 0.0 } else { hi <= max_width } {
            continue;
        }
        let budget = (max_width - marker_w).max(0.0);
        // Visible side first: the leftmost frag of an LTR line, the rightmost of an RTL one.
        let mut order: Vec<usize> = (0..line.len()).collect();
        if rtl {
            order.sort_by(|&a, &b| (line[b].x + line[b].width).total_cmp(&(line[a].x + line[a].width)));
        } else {
            order.sort_by(|&a, &b| line[a].x.total_cmp(&line[b].x));
        }
        let mut keep = vec![false; line.len()];
        let mut any = false;
        for &i in &order {
            let f = &mut line[i];
            let fits = if rtl { f.x >= max_width - budget } else { f.x + f.width <= budget };
            if fits {
                keep[i] = true;
                any = true;
                continue;
            }
            // The frag crosses the budget: keep the chars that still fit (always the line's first).
            let room = if rtl {
                f.x + f.width - f.padding_right - (max_width - budget)
            } else {
                budget - (f.x + f.padding_left)
            };
            if truncate_frag_visual(f, room, rtl, !any, m) {
                keep[i] = true;
            }
            break;
        }
        let template = line[order[0]].clone();
        let mut idx = 0;
        line.retain(|_| {
            idx += 1;
            keep[idx - 1]
        });
        let template = line.last().cloned().unwrap_or(template);
        let edge = if rtl {
            line.iter().map(|f| f.x).fold(f32::INFINITY, f32::min)
        } else {
            line.iter().map(|f| f.x + f.width).fold(f32::NEG_INFINITY, f32::max)
        };
        let edge = if edge.is_finite() { edge } else if rtl { max_width } else { 0.0 };
        let style = marker_style.get_or_insert_with(|| {
            let mut st = (**block_style).clone();
            st.text_emphasis_style = TextEmphasisStyle::None;
            st.vertical_align = VerticalAlign::Baseline;
            Arc::new(st)
        });
        // Sits on the baseline of the frag it follows (`apply_inline_vertical_align` places
        // baseline-aligned frags of different fonts by their ascents); with nothing to follow —
        // the half-leading of a lone block-sized frag.
        let y_offset = if template.img_src.is_none()
            && !template.text.is_empty()
            && template.style.vertical_align == VerticalAlign::Baseline
        {
            template.y_offset + m.ascent_px_with_families(template.style.font_size, &template.style.font_family)
                - m.ascent_px_with_families(block_style.font_size, &block_style.font_family)
        } else {
            (line_h - block_style.font_size) / 2.0
        };
        let shift = if rtl { edge - marker_w } else { edge };
        for mut e in marker_frags(marker, rtl, &template, style, y_offset, m) {
            e.x += shift;
            line.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is 10 px wide.
    struct Fixed;
    impl TextMeasurer for Fixed {
        fn char_width(&self, _ch: char, _font_size_px: f32) -> f32 {
            10.0
        }
    }

    fn frag(x: f32, text: &str, level: u8) -> InlineFrag {
        InlineFrag {
            x,
            width: 10.0 * text.chars().count() as f32,
            y_offset: 0.0,
            text: text.to_string(),
            style: Arc::new(ComputedStyle::root()),
            padding_left: 0.0,
            padding_right: 0.0,
            is_element_box: false,
            effect_scopes: Vec::new(),
            img_src: None,
            img_is_lazy: false,
            is_first_line: false,
            source_node: lumen_dom::NodeId::from_index(0),
            source_char_offset: 0,
            bidi_level: level,
            bidi_isolate: None,
            merged_sources: Vec::new(),
            forced_end: false,
            font_underline: None,
        }
    }

    fn run(line: Vec<InlineFrag>, max_width: f32, marker: &str, rtl: bool) -> Vec<InlineFrag> {
        let mut lines = vec![line];
        apply_text_overflow_string(&mut lines, max_width, marker, &Arc::new(ComputedStyle::root()), 20.0, rtl, &Fixed);
        lines.remove(0)
    }

    #[test]
    fn ltr_keeps_the_left_part_and_puts_the_marker_after_it() {
        let out = run(vec![frag(0.0, "abcdefghij", 0)], 55.0, "-", false);
        assert_eq!((out[0].text.as_str(), out[0].x, out[0].width), ("abcd", 0.0, 40.0));
        assert_eq!((out[1].text.as_str(), out[1].x), ("-", 40.0));
    }

    #[test]
    fn rtl_keeps_the_right_part_and_puts_the_marker_before_it() {
        // The LTR word sits at level 2 in an RTL paragraph; its right edge is the box's right edge.
        let out = run(vec![frag(-45.0, "abcdefghij", 2)], 55.0, "-", true);
        assert_eq!((out[0].text.as_str(), out[0].x, out[0].width), ("ghij", 15.0, 40.0));
        assert_eq!((out[1].text.as_str(), out[1].x), ("-", 5.0));
    }

    #[test]
    fn odd_level_frag_loses_the_start_of_its_logical_text_on_the_left_edge_cut() {
        // Level 1 reads right to left, so the visual left end is the logical end.
        let out = run(vec![frag(0.0, "abcdefghij", 1)], 55.0, "-", false);
        assert_eq!((out[0].text.as_str(), out[0].source_char_offset), ("ghij", 6));
        assert_eq!(out[1].x, 40.0);
    }

    #[test]
    fn rtl_cut_of_an_odd_frag_keeps_the_logical_start() {
        let out = run(vec![frag(-45.0, "abcdefghij", 1)], 55.0, "-", true);
        assert_eq!((out[0].text.as_str(), out[0].x), ("abcd", 15.0));
    }

    #[test]
    fn first_visual_char_survives_when_nothing_fits() {
        let out = run(vec![frag(0.0, "abcdefghij", 0)], 5.0, "-", false);
        assert_eq!((out[0].text.as_str(), out[1].x), ("a", 10.0));
    }

    #[test]
    fn fitting_line_is_untouched() {
        let out = run(vec![frag(0.0, "abc", 0)], 55.0, "-", false);
        assert_eq!(out.len(), 1);
        let out = run(vec![frag(0.0, "abc", 1)], 55.0, "-", true);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn marker_is_its_own_bidi_paragraph() {
        // RTL paragraph, marker "A אב": the Latin letter is the logical start, so it sits rightmost.
        let out = run(vec![frag(-45.0, "abcdefghij", 2)], 55.0, "A \u{5d0}\u{5d1}", true);
        let marker: Vec<&InlineFrag> = out.iter().filter(|f| f.text != "j").collect();
        assert!(marker.len() >= 2);
        let a = marker.iter().find(|f| f.text == "A").expect("latin piece");
        let heb = marker.iter().find(|f| f.text.contains('\u{5d0}')).expect("hebrew piece");
        assert!(a.x > heb.x, "A must be right of the Hebrew letters: {} vs {}", a.x, heb.x);
        assert_eq!(a.bidi_level, 2);
    }

    #[test]
    fn hebrew_marker_in_ltr_paragraph_is_one_odd_piece() {
        let out = run(vec![frag(0.0, "abcdefghij", 0)], 55.0, "\u{5d0}\u{5d1}", false);
        // 55 px box: the marker (20) leaves 35 for text.
        let m = out.last().expect("marker");
        assert_eq!((m.text.as_str(), m.bidi_level, m.x), ("\u{5d0}\u{5d1}", 1, 30.0));
    }
}
