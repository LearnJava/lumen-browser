//! `text-align: justify` — растяжение промежутков строки до ширины контейнера
//! (CSS Text L3 §7.1, §7.3 `text-justify`).
//!
//! Работает над готовой строкой в логическом порядке, до UAX #9 L2: растяжение
//! симметрично по направлению, а `reflect` в `bidi::reorder_line` сохраняет
//! промежутки. Добавка кладётся в стиль фрагмента: `word-spacing` (пробелы) или
//! `letter-spacing` (пары символов) — рисуют и измеряют её уже существующие пути
//! (`DrawText`, `text_geometry`), отдельной геометрии у растянутой строки нет.

use super::types::InlineFrag;
use crate::style::{TextJustify, WhiteSpace};
use std::sync::Arc;

/// Пара «символ — символ» в письме без пробелов между словами: CJK, кана, полная ширина.
fn is_ideographic(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F | 0x3190..=0x4DBF | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// Знаки, которые не начинают нового типографского символа (комбинируемые, вариантные
/// селекторы, соединители нулевой ширины).
fn is_combining(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200B..=0x200D | 0x20D0..=0x20FF | 0x3099..=0x309A
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0xE0100..=0xE01EF)
}

/// Курсивные письма: `inter-character` их не растягивает (CSS Text L3 §7.3).
fn is_cursive(c: char) -> bool {
    matches!(c as u32,
        0x0600..=0x077F | 0x07C0..=0x07FF | 0x08A0..=0x08FF | 0x1800..=0x18AF
        | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Сколько промежутков между соседними символами `text` можно растянуть.
fn char_gaps(text: &str) -> u32 {
    let mut gaps = 0u32;
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if is_combining(c) {
            continue;
        }
        if let Some(p) = prev
            && !(is_cursive(p) && is_cursive(c))
        {
            gaps += 1;
        }
        prev = Some(c);
    }
    gaps
}

/// Число ASCII-пробелов в `text` — то, чему рисование прибавляет `word-spacing`.
fn spaces(text: &str) -> u32 {
    text.bytes().filter(|&b| b == b' ').count() as u32
}

/// Промежутки фрагмента: внутри и на стыке со следующим.
#[derive(Clone, Copy, Default)]
struct Opp {
    /// Пробелы внутри фрагмента (растягиваются `word-spacing`).
    spaces: u32,
    /// Пары символов внутри фрагмента (растягиваются `letter-spacing`).
    chars: u32,
    /// Стык со следующим фрагментом.
    seam: u32,
}

/// Растягивает строку `line` до `content_width`. Ничего не делает, если места нет, у строки нет
/// возможностей растяжения или `text-justify: none`.
pub(crate) fn justify_line(line: &mut [InlineFrag], content_width: f32) {
    // Висящие пробелы в конце строки (`pre-wrap`) не входят ни в ширину, ни в возможности растяжения.
    let end = line.iter().rposition(|f| f.text.is_empty() || !f.text.bytes().all(|b| b == b' ')).map_or(0, |i| i + 1);
    let Some(last) = line[..end].last() else { return };
    let slack = content_width - (last.x + last.width);
    if slack.is_nan() || slack < 0.01 {
        return;
    }
    // `white-space: pre` не оборачивается и не растягивается (CSS Text L3 §7.1).
    if line.iter().any(|f| f.style.white_space == WhiteSpace::Pre) {
        return;
    }
    let mode = |f: &InlineFrag| f.style.text_extra.justify;
    let n = end;
    // Позиции табуляции задаёт раскладка до растяжения: растянуть можно только то, что стоит
    // после последней табуляции строки, иначе следующие за табами слова съедут с позиций.
    let first = line.iter().rposition(|f| f.text == "	").map_or(0, |i| i + 1);

    // Проход 1: пробелы и стыки в режимах `auto`/`inter-word`.
    let mut opps = vec![Opp::default(); line.len()];
    let mut words = 0u32;
    for (i, f) in line[..n].iter().enumerate().skip(first) {
        if matches!(mode(f), TextJustify::Auto | TextJustify::InterWord) && f.img_src.is_none() {
            opps[i].spaces = spaces(&f.text);
            words += opps[i].spaces;
        }
        if i + 1 < n {
            let next = &line[i + 1];
            let gap = next.x - (f.x + f.width);
            let wordy = |m: TextJustify| matches!(m, TextJustify::Auto | TextJustify::InterWord);
            if gap > 0.01 && wordy(mode(f)) && wordy(mode(next)) {
                opps[i].seam = 1;
                words += 1;
            }
        }
    }

    // Проход 2: пары символов — для `inter-character` и для `auto` в строке без пробелов.
    let mut chars = 0u32;
    for (i, f) in line[..n].iter().enumerate().skip(first) {
        let m = mode(f);
        let char_mode = m == TextJustify::InterCharacter
            || (m == TextJustify::Auto && words == 0 && f.text.chars().any(is_ideographic));
        if char_mode && f.img_src.is_none() {
            opps[i].chars = char_gaps(&f.text);
            chars += opps[i].chars;
        }
        if i + 1 < n && opps[i].seam == 0 {
            let next = &line[i + 1];
            let mn = mode(next);
            let inter = m == TextJustify::InterCharacter && mn != TextJustify::None;
            let cjk_pair = m == TextJustify::Auto
                && mn == TextJustify::Auto
                && words == 0
                && f.text.chars().next_back().is_some_and(is_ideographic)
                && next.text.chars().next().is_some_and(is_ideographic);
            if inter || cjk_pair {
                opps[i].seam = 1;
                chars += 1;
            }
        }
    }

    let total = words + chars;
    if total == 0 {
        return;
    }
    let extra = slack / total as f32;
    let mut shift = 0.0f32;
    for (f, o) in line.iter_mut().zip(opps) {
        f.x += shift;
        let own = o.spaces + o.chars;
        if own > 0 {
            let style = Arc::make_mut(&mut f.style);
            if o.spaces > 0 {
                style.word_spacing += extra;
            } else {
                style.letter_spacing += extra;
            }
            f.width += extra * own as f32;
        }
        shift += extra * (own + o.seam) as f32;
    }
}
