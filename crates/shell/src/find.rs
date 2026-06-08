//! Find in page (Ctrl+F): состояние bar-а, поиск совпадений по display list,
//! сборка финального display list с подсветками и overlay-баром.
//!
//! Поиск работает в двух режимах:
//! - **Plain text** (по умолчанию): строковое вхождение (case-insensitive) по
//!   DrawText-командам display list — быстро и без зависимостей.
//! - **Regex** (Ctrl+R в открытом баре): полноценный регулярный паттерн через
//!   [`regex::Regex`], case-insensitive, по тексту [`lumen_layout::TextFragment`].
//!   Исходник матча — [`collect_visible_text`][lumen_layout::collect_visible_text]
//!   (layout tree), позиция подсветки — `TextFragment.rect` + пиксельный offset
//!   через [`TextMeasurer`].
//!
//! Подсветка в обоих режимах вставляется как `FillRect` перед соответствующей
//! `DrawText`-командой (painter's order: highlight под глифами, поверх фона).
//!
//! Phase 0 ограничения:
//! - letter-spacing / word-spacing внутри фрагмента смещают реальные глифы
//!   относительно вычисленного rect-а на величину аккумулированного spacing-а;
//! - find-bar фиксированных размеров в правом верхнем углу окна;
//! - regex: lookahead/lookbehind и Unicode category недоступны (NFA-движок);
//! - совпадения не пересекают границы TextFragment (одно слово / run).

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight, TextFragment, TextMeasurer};
use lumen_paint::{DisplayCommand, DisplayList};

/// Состояние find bar и текущего запроса.
#[derive(Debug, Default, Clone)]
pub struct FindState {
    open: bool,
    query: String,
    active: usize,
    /// Если `true` — запрос интерпретируется как regex-паттерн.
    regex_mode: bool,
}

impl FindState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn is_regex_mode(&self) -> bool {
        self.regex_mode
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.active = 0;
    }

    pub fn append_str(&mut self, s: &str) {
        if !self.open {
            return;
        }
        let before = self.query.len();
        for c in s.chars() {
            if !c.is_control() {
                self.query.push(c);
            }
        }
        if self.query.len() != before {
            self.active = 0;
        }
    }

    pub fn backspace(&mut self) {
        if !self.open {
            return;
        }
        if self.query.pop().is_some() {
            self.active = 0;
        }
    }

    /// Переключает режим plain-text ↔ regex. Сбрасывает счётчик активного
    /// совпадения, поскольку при смене режима набор матчей полностью меняется.
    pub fn toggle_regex_mode(&mut self) {
        self.regex_mode = !self.regex_mode;
        self.active = 0;
    }

    /// Циклически переходит к следующему совпадению. `total` — текущее число
    /// найденных матчей (вычисляется заново на каждом запросе, потому что
    /// меняется при resize / reload).
    pub fn next(&mut self, total: usize) {
        if total > 0 {
            self.active = (self.active + 1) % total;
        }
    }

    pub fn prev(&mut self, total: usize) {
        if total > 0 {
            self.active = (self.active + total - 1) % total;
        }
    }
}

/// Найденный матч: bounding box в координатах окна и индекс DrawText-команды
/// в исходном display list (нужен, чтобы вставить highlight-FillRect строго
/// перед своим текстом, а не глобально в начале списка).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FindMatch {
    pub rect: Rect,
    pub dl_index: usize,
}

pub const HIGHLIGHT_INACTIVE: Color = Color { r: 255, g: 235, b: 90, a: 255 };
pub const HIGHLIGHT_ACTIVE: Color = Color { r: 255, g: 150, b: 50, a: 255 };

/// Доля viewport-а сверху, в которую попадает match при scroll-to.
const SCROLL_MARGIN_FRACTION: f32 = 0.25;

/// Вычисляет новое значение `scroll_y` так, чтобы `match_rect` попал в
/// видимую область. Возвращает `None`, если match уже полностью видим.
pub fn scroll_to_match(
    match_rect: Rect,
    viewport_height: f32,
    current_scroll: f32,
) -> Option<f32> {
    if viewport_height <= 0.0 {
        return None;
    }
    let match_top = match_rect.y;
    let match_bottom = match_rect.y + match_rect.height;
    let view_top = current_scroll;
    let view_bottom = current_scroll + viewport_height;
    if match_top >= view_top && match_bottom <= view_bottom {
        return None;
    }
    let target = (match_top - viewport_height * SCROLL_MARGIN_FRACTION).max(0.0);
    if (target - current_scroll).abs() < f32::EPSILON {
        return None;
    }
    Some(target)
}

/// Находит все непересекающиеся вхождения `query` в DrawText-командах `dl`.
/// Поиск case-insensitive (ASCII + Unicode 1:1 lowercase mapping).
pub fn find_matches(
    dl: &DisplayList,
    query: &str,
    measurer: &dyn TextMeasurer,
) -> Vec<FindMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_chars: Vec<char> = query.chars().collect();
    let mut out = Vec::new();
    for (idx, cmd) in dl.iter().enumerate() {
        if let DisplayCommand::DrawText { rect, text, font_size, .. } = cmd {
            collect_in_text(text, *font_size, *rect, idx, &query_chars, measurer, &mut out);
        }
    }
    out
}

fn collect_in_text(
    text: &str,
    font_size: f32,
    text_rect: Rect,
    dl_index: usize,
    query_chars: &[char],
    measurer: &dyn TextMeasurer,
    out: &mut Vec<FindMatch>,
) {
    let text_chars: Vec<char> = text.chars().collect();
    let q = query_chars.len();
    let n = text_chars.len();
    if q == 0 || q > n {
        return;
    }
    let mut i = 0;
    while i + q <= n {
        if (0..q).all(|k| chars_eq_ci(text_chars[i + k], query_chars[k])) {
            let prefix_w: f32 = text_chars[..i]
                .iter()
                .map(|c| measurer.char_width(*c, font_size))
                .sum();
            let match_w: f32 = text_chars[i..i + q]
                .iter()
                .map(|c| measurer.char_width(*c, font_size))
                .sum();
            out.push(FindMatch {
                rect: Rect::new(text_rect.x + prefix_w, text_rect.y, match_w, text_rect.height),
                dl_index,
            });
            i += q;
        } else {
            i += 1;
        }
    }
}

fn chars_eq_ci(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    let la = a.to_lowercase().next().unwrap_or(a);
    let lb = b.to_lowercase().next().unwrap_or(b);
    la == lb
}

/// Проверяет, является ли `pattern` корректным regex-паттерном.
/// Используется bar overlay-ем для вывода «ERR» вместо счётчика.
pub fn is_valid_regex_pattern(pattern: &str) -> bool {
    regex::Regex::new(pattern).is_ok()
}

/// Находит все regex-матчи паттерна `pattern` по [`TextFragment`]-ам
/// (`collect_visible_text` layout tree). Возвращает абсолютные `Rect`-ы
/// с корректным `dl_index` для вставки highlight-FillRect перед DrawText.
///
/// Алгоритм:
/// 1. Компилирует `pattern` как case-insensitive regex (пустой паттерн или
///    невалидный → возвращает пустой Vec).
/// 2. Для каждого фрагмента ищет все вхождения через `re.find_iter`.
/// 3. Суб-rect каждого вхождения: `frag.rect.x + prefix_width`, ширина =
///    ширина совпавших символов по `measurer.char_width`.
/// 4. `dl_index`: первая `DrawText`-команда в `dl`, у которой `rect.x`/`rect.y`
///    совпадает с `frag.rect` (±0.5 px) и текст совпадает. Если не найдено —
///    матч пропускается (фрагмент не рендерится, например `visibility:hidden`).
pub fn find_matches_regex(
    frags: &[TextFragment],
    dl: &DisplayList,
    pattern: &str,
    measurer: &dyn TextMeasurer,
) -> Vec<FindMatch> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let re = match regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    // Индекс DrawText-команд по (x_quantized, y_quantized) для O(1) lookup.
    // Ключ: (round(x * 2) as i32, round(y * 2) as i32) — квантизация 0.5 px.
    let mut dl_map: std::collections::HashMap<(i32, i32, String), usize> =
        std::collections::HashMap::new();
    for (idx, cmd) in dl.iter().enumerate() {
        if let DisplayCommand::DrawText { rect, text, .. } = cmd {
            let key = quant_key(rect.x, rect.y, text);
            // Первое вхождение с таким ключом — именно его и хотим (document order).
            dl_map.entry(key).or_insert(idx);
        }
    }

    let mut out = Vec::new();
    for frag in frags {
        let dl_index = match dl_map.get(&quant_key(frag.rect.x, frag.rect.y, &frag.text)) {
            Some(&idx) => idx,
            None => continue,
        };

        // Получаем font_size из DisplayList-команды (TextFragment его не хранит).
        let font_size = match &dl[dl_index] {
            DisplayCommand::DrawText { font_size, .. } => *font_size,
            _ => 16.0,
        };

        for m in re.find_iter(&frag.text) {
            let prefix = &frag.text[..m.start()];
            let prefix_w: f32 = prefix
                .chars()
                .map(|c| measurer.char_width(c, font_size))
                .sum();
            let match_w: f32 = m
                .as_str()
                .chars()
                .map(|c| measurer.char_width(c, font_size))
                .sum();
            if match_w < f32::EPSILON {
                continue;
            }
            out.push(FindMatch {
                rect: Rect::new(
                    frag.rect.x + prefix_w,
                    frag.rect.y,
                    match_w,
                    frag.rect.height,
                ),
                dl_index,
            });
        }
    }
    out
}

/// Квантизирует координату до 0.5 px и возвращает ключ для HashMap.
fn quant_key(x: f32, y: f32, text: &str) -> (i32, i32, String) {
    ((x * 2.0).round() as i32, (y * 2.0).round() as i32, text.to_string())
}

/// Параметры overlay-бара.
pub struct BarOverlay {
    pub window_size: (u32, u32),
}

const BAR_BG: Color = Color { r: 40, g: 40, b: 45, a: 235 };
const BAR_FG: Color = Color { r: 245, g: 245, b: 245, a: 255 };
const BAR_DIM: Color = Color { r: 180, g: 180, b: 180, a: 255 };
const BAR_INPUT_BG: Color = Color { r: 25, g: 25, b: 28, a: 255 };
const BAR_REGEX_ON: Color = Color { r: 100, g: 200, b: 255, a: 255 };
const BAR_ERR: Color = Color { r: 255, g: 80, b: 80, a: 255 };

const BAR_WIDTH: f32 = 480.0;
const BAR_HEIGHT: f32 = 40.0;
const BAR_PAD: f32 = 12.0;
const BAR_FONT_SIZE: f32 = 16.0;

/// Собирает page-полосу display list-а: исходные команды + highlight-FillRect-ы
/// перед каждой DrawText с матчем.
pub fn build_page_with_highlights(
    base: &DisplayList,
    state: &FindState,
    matches: &[FindMatch],
) -> DisplayList {
    let mut out: DisplayList = Vec::with_capacity(base.len() + matches.len());

    let mut by_index: Vec<Vec<(usize, &FindMatch)>> = vec![Vec::new(); base.len()];
    for (i, m) in matches.iter().enumerate() {
        if m.dl_index < base.len() {
            by_index[m.dl_index].push((i, m));
        }
    }

    for (idx, cmd) in base.iter().enumerate() {
        for (i, m) in &by_index[idx] {
            let color = if *i == state.active {
                HIGHLIGHT_ACTIVE
            } else {
                HIGHLIGHT_INACTIVE
            };
            out.push(DisplayCommand::FillRect {
                rect: m.rect,
                color,
            });
        }
        out.push(cmd.clone());
    }
    out
}

/// Собирает overlay-полосу: только find-bar (фон + label + input + counter +
/// regex-индикатор). Рисуется поверх страницы без scroll-смещения.
pub fn build_bar_overlay(
    state: &FindState,
    matches_count: usize,
    bar: BarOverlay,
) -> DisplayList {
    let mut out: DisplayList = Vec::with_capacity(8);
    append_bar(&mut out, state, matches_count, bar.window_size);
    out
}

/// Совместимая сборка: page + bar в один list. Только для тестов и dump-режимов.
#[cfg(test)]
pub fn build_with_overlay(
    base: &DisplayList,
    state: &FindState,
    matches: &[FindMatch],
    bar: BarOverlay,
) -> DisplayList {
    let mut out = build_page_with_highlights(base, state, matches);
    append_bar(&mut out, state, matches.len(), bar.window_size);
    out
}

fn append_bar(out: &mut DisplayList, state: &FindState, total: usize, (ww, _wh): (u32, u32)) {
    let x = (ww as f32 - BAR_WIDTH - BAR_PAD).max(BAR_PAD);
    let y = BAR_PAD;

    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, BAR_WIDTH, BAR_HEIGHT),
        color: BAR_BG,
    });

    // Regex-индикатор «.*» слева от лейбла.
    let regex_label_w = 28.0;
    let regex_color = if state.is_regex_mode() { BAR_REGEX_ON } else { BAR_DIM };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(x + 8.0, y + 10.0, regex_label_w, BAR_FONT_SIZE * 1.2),
        text: ".*".to_string(),
        font_size: BAR_FONT_SIZE - 2.0,
        color: regex_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    let label = "Найти:";
    let label_w = 60.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(x + 8.0 + regex_label_w + 4.0, y + 10.0, label_w, BAR_FONT_SIZE * 1.2),
        text: label.to_string(),
        font_size: BAR_FONT_SIZE,
        color: BAR_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    let input_x = x + 8.0 + regex_label_w + 4.0 + label_w + 8.0;
    let input_w = 248.0;
    let input_h = 26.0;
    let input_y = y + (BAR_HEIGHT - input_h) / 2.0;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(input_x, input_y, input_w, input_h),
        color: BAR_INPUT_BG,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(input_x + 6.0, input_y + 4.0, input_w - 12.0, input_h - 8.0),
        text: state.query().to_string(),
        font_size: BAR_FONT_SIZE,
        color: BAR_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    let status = if state.query().is_empty() {
        "Esc / Ctrl+R".to_string()
    } else if state.is_regex_mode() && !is_valid_regex_pattern(state.query()) {
        "ERR".to_string()
    } else if total == 0 {
        "0/0".to_string()
    } else {
        format!("{}/{}", state.active_index() + 1, total)
    };
    let status_color = if status == "ERR" { BAR_ERR } else { BAR_DIM };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(input_x + input_w + 8.0, y + 10.0, 110.0, BAR_FONT_SIZE * 1.2),
        text: status,
        font_size: BAR_FONT_SIZE - 2.0,
        color: status_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_paint::DisplayCommand;

    struct Fixed8;
    impl TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn draw_text(text: &str, x: f32, y: f32, w: f32, h: f32) -> DisplayCommand {
        DisplayCommand::DrawText {
            rect: Rect::new(x, y, w, h),
            text: text.to_string(),
            font_size: 16.0,
            color: Color::BLACK,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        }
    }

    // ── find_matches (plain-text) ──────────────────────────────────────────────

    #[test]
    fn empty_query_no_matches() {
        let dl = vec![draw_text("hello world", 0.0, 0.0, 200.0, 20.0)];
        assert!(find_matches(&dl, "", &Fixed8).is_empty());
    }

    #[test]
    fn simple_match() {
        let dl = vec![draw_text("hello world", 0.0, 0.0, 200.0, 20.0)];
        let m = find_matches(&dl, "world", &Fixed8);
        assert_eq!(m.len(), 1);
        assert!((m[0].rect.x - 48.0).abs() < 0.01, "x={}", m[0].rect.x);
        assert!((m[0].rect.width - 40.0).abs() < 0.01);
        assert!((m[0].rect.y - 0.0).abs() < 0.01);
        assert!((m[0].rect.height - 20.0).abs() < 0.01);
    }

    #[test]
    fn case_insensitive_ascii() {
        let dl = vec![draw_text("Hello World", 0.0, 0.0, 200.0, 20.0)];
        assert_eq!(find_matches(&dl, "WORLD", &Fixed8).len(), 1);
        assert_eq!(find_matches(&dl, "hello", &Fixed8).len(), 1);
    }

    #[test]
    fn case_insensitive_cyrillic() {
        let dl = vec![draw_text("Привет, Мир", 0.0, 0.0, 200.0, 20.0)];
        let m = find_matches(&dl, "мир", &Fixed8);
        assert_eq!(m.len(), 1);
        assert!((m[0].rect.x - 64.0).abs() < 0.01, "x={}", m[0].rect.x);
        let n = find_matches(&dl, "ПРИВЕТ", &Fixed8);
        assert_eq!(n.len(), 1);
    }

    #[test]
    fn multiple_matches_non_overlapping() {
        let dl = vec![draw_text("ababab", 10.0, 0.0, 200.0, 20.0)];
        let m = find_matches(&dl, "ab", &Fixed8);
        assert_eq!(m.len(), 3);
        assert!((m[0].rect.x - 10.0).abs() < 0.01);
        assert!((m[1].rect.x - 26.0).abs() < 0.01);
        assert!((m[2].rect.x - 42.0).abs() < 0.01);
    }

    #[test]
    fn matches_do_not_overlap_with_repeated_chars() {
        let dl = vec![draw_text("aaaa", 0.0, 0.0, 100.0, 20.0)];
        let m = find_matches(&dl, "aa", &Fixed8);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn no_match_returns_empty() {
        let dl = vec![draw_text("hello world", 0.0, 0.0, 200.0, 20.0)];
        assert!(find_matches(&dl, "xyz", &Fixed8).is_empty());
    }

    #[test]
    fn query_longer_than_text_no_match() {
        let dl = vec![draw_text("hi", 0.0, 0.0, 50.0, 20.0)];
        assert!(find_matches(&dl, "hello", &Fixed8).is_empty());
    }

    #[test]
    fn matches_across_multiple_draw_texts() {
        let dl = vec![
            draw_text("foo bar", 0.0, 0.0, 100.0, 20.0),
            draw_text("bar baz", 0.0, 20.0, 100.0, 20.0),
        ];
        let m = find_matches(&dl, "bar", &Fixed8);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].dl_index, 0);
        assert_eq!(m[1].dl_index, 1);
    }

    #[test]
    fn non_draw_text_commands_ignored() {
        let dl = vec![
            DisplayCommand::FillRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::BLACK,
            },
            draw_text("hello", 0.0, 0.0, 50.0, 20.0),
        ];
        let m = find_matches(&dl, "hello", &Fixed8);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].dl_index, 1);
    }

    // ── FindState ──────────────────────────────────────────────────────────────

    #[test]
    fn state_open_close_resets_query() {
        let mut s = FindState::default();
        s.open();
        s.append_str("ab");
        assert_eq!(s.query(), "ab");
        s.close();
        assert!(!s.is_open());
        assert_eq!(s.query(), "");
        assert_eq!(s.active_index(), 0);
    }

    #[test]
    fn state_ignores_input_when_closed() {
        let mut s = FindState::default();
        s.append_str("a");
        assert_eq!(s.query(), "");
    }

    #[test]
    fn state_ignores_control_chars() {
        let mut s = FindState::default();
        s.open();
        s.append_str("\n\t\x08");
        s.append_str("\rok");
        assert_eq!(s.query(), "ok");
    }

    #[test]
    fn state_backspace_pops_last_char() {
        let mut s = FindState::default();
        s.open();
        s.append_str("abc");
        s.backspace();
        assert_eq!(s.query(), "ab");
        s.backspace();
        s.backspace();
        s.backspace();
        assert_eq!(s.query(), "");
    }

    #[test]
    fn state_next_prev_cycles() {
        let mut s = FindState::default();
        s.open();
        s.next(3);
        assert_eq!(s.active_index(), 1);
        s.next(3);
        assert_eq!(s.active_index(), 2);
        s.next(3);
        assert_eq!(s.active_index(), 0);
        s.prev(3);
        assert_eq!(s.active_index(), 2);
    }

    #[test]
    fn state_next_with_zero_total_noop() {
        let mut s = FindState::default();
        s.open();
        s.next(0);
        assert_eq!(s.active_index(), 0);
    }

    #[test]
    fn state_typing_resets_active() {
        let mut s = FindState::default();
        s.open();
        s.append_str("ab");
        s.next(5);
        s.next(5);
        assert_eq!(s.active_index(), 2);
        s.append_str("c");
        assert_eq!(s.active_index(), 0);
    }

    #[test]
    fn state_toggle_regex_mode() {
        let mut s = FindState::default();
        assert!(!s.is_regex_mode());
        s.toggle_regex_mode();
        assert!(s.is_regex_mode());
        s.next(5);
        s.toggle_regex_mode();
        assert!(!s.is_regex_mode());
        assert_eq!(s.active_index(), 0);
    }

    // ── build_with_overlay (highlights + bar) ─────────────────────────────────

    #[test]
    fn build_with_overlay_inserts_highlight_before_text() {
        let dl = vec![draw_text("hello", 0.0, 0.0, 50.0, 20.0)];
        let m = find_matches(&dl, "ell", &Fixed8);
        assert_eq!(m.len(), 1);
        let mut state = FindState::default();
        state.open();
        state.append_str("ell");
        let final_dl = build_with_overlay(
            &dl,
            &state,
            &m,
            BarOverlay { window_size: (800, 600) },
        );

        match &final_dl[0] {
            DisplayCommand::FillRect { color, .. } => {
                assert_eq!(color.r, HIGHLIGHT_ACTIVE.r);
            }
            _ => panic!("expected FillRect highlight first"),
        }
        match &final_dl[1] {
            DisplayCommand::DrawText { text, .. } => assert_eq!(text, "hello"),
            _ => panic!("expected DrawText second"),
        }
    }

    #[test]
    fn build_with_overlay_appends_bar() {
        let dl = vec![draw_text("hi", 0.0, 0.0, 50.0, 20.0)];
        let m: Vec<FindMatch> = vec![];
        let mut state = FindState::default();
        state.open();
        let out = build_with_overlay(
            &dl,
            &state,
            &m,
            BarOverlay { window_size: (800, 600) },
        );
        let has_label = out
            .iter()
            .any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "Найти:"));
        assert!(has_label);
    }

    #[test]
    fn build_with_overlay_active_highlight_brighter() {
        let dl = vec![draw_text("abab", 0.0, 0.0, 50.0, 20.0)];
        let m = find_matches(&dl, "ab", &Fixed8);
        assert_eq!(m.len(), 2);
        let mut state = FindState::default();
        state.open();
        state.append_str("ab");
        state.next(2);
        let final_dl = build_with_overlay(
            &dl,
            &state,
            &m,
            BarOverlay { window_size: (800, 600) },
        );
        let match_highlights: Vec<&Color> = final_dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, rect } if (rect.height - 20.0).abs() < 0.01 => {
                    Some(color)
                }
                _ => None,
            })
            .collect();
        assert_eq!(match_highlights.len(), 2);
        assert_eq!(match_highlights[0].r, HIGHLIGHT_INACTIVE.r);
        assert_eq!(match_highlights[1].r, HIGHLIGHT_ACTIVE.r);
    }

    #[test]
    fn build_with_overlay_status_counter_present() {
        let dl = vec![draw_text("ab ab ab", 0.0, 0.0, 200.0, 20.0)];
        let m = find_matches(&dl, "ab", &Fixed8);
        assert_eq!(m.len(), 3);
        let mut state = FindState::default();
        state.open();
        state.append_str("ab");
        let final_dl = build_with_overlay(
            &dl,
            &state,
            &m,
            BarOverlay { window_size: (800, 600) },
        );
        let has_counter = final_dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "1/3"));
        assert!(has_counter);
    }

    #[test]
    fn build_with_overlay_no_matches_shows_zero_status() {
        let dl = vec![draw_text("hello", 0.0, 0.0, 50.0, 20.0)];
        let m: Vec<FindMatch> = vec![];
        let mut state = FindState::default();
        state.open();
        state.append_str("xyz");
        let final_dl = build_with_overlay(
            &dl,
            &state,
            &m,
            BarOverlay { window_size: (800, 600) },
        );
        let has_zero = final_dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "0/0"));
        assert!(has_zero);
    }

    #[test]
    fn find_returns_empty_when_query_empty_even_with_text() {
        let dl = vec![draw_text("anything", 0.0, 0.0, 100.0, 20.0)];
        assert!(find_matches(&dl, "", &Fixed8).is_empty());
    }

    // ── scroll_to_match ────────────────────────────────────────────────────────

    #[test]
    fn scroll_to_match_already_visible_returns_none() {
        let r = Rect::new(0.0, 200.0, 100.0, 20.0);
        assert!(scroll_to_match(r, 600.0, 100.0).is_none());
    }

    #[test]
    fn scroll_to_match_below_viewport_scrolls_down() {
        let r = Rect::new(0.0, 800.0, 100.0, 20.0);
        let target = scroll_to_match(r, 600.0, 0.0).expect("должен скроллить");
        assert!((target - 650.0).abs() < 0.01, "target={target}");
    }

    #[test]
    fn scroll_to_match_above_viewport_scrolls_up() {
        let r = Rect::new(0.0, 100.0, 100.0, 20.0);
        let target = scroll_to_match(r, 600.0, 500.0).expect("должен скроллить");
        assert!((target - 0.0).abs() < 0.01, "target={target}");
    }

    #[test]
    fn scroll_to_match_partially_below_scrolls() {
        let r = Rect::new(0.0, 90.0, 100.0, 40.0);
        let target = scroll_to_match(r, 100.0, 0.0).expect("должен скроллить");
        assert!((target - 65.0).abs() < 0.01, "target={target}");
    }

    #[test]
    fn scroll_to_match_partially_above_scrolls() {
        let r = Rect::new(0.0, 190.0, 100.0, 20.0);
        let target = scroll_to_match(r, 600.0, 200.0).expect("должен скроллить");
        assert!((target - 40.0).abs() < 0.01, "target={target}");
    }

    #[test]
    fn scroll_to_match_zero_viewport_returns_none() {
        let r = Rect::new(0.0, 100.0, 100.0, 20.0);
        assert!(scroll_to_match(r, 0.0, 0.0).is_none());
    }

    #[test]
    fn scroll_to_match_negative_viewport_returns_none() {
        let r = Rect::new(0.0, 100.0, 100.0, 20.0);
        assert!(scroll_to_match(r, -1.0, 0.0).is_none());
    }

    #[test]
    fn scroll_to_match_exact_top_no_scroll() {
        let r = Rect::new(0.0, 400.0, 100.0, 20.0);
        assert!(scroll_to_match(r, 400.0, 300.0).is_none());
    }

    #[test]
    fn scroll_to_match_does_not_overshoot_caller_max() {
        let r = Rect::new(0.0, 99_999.0, 100.0, 20.0);
        let target = scroll_to_match(r, 600.0, 0.0).expect("должен скроллить");
        assert!((target - 99_849.0).abs() < 0.1, "target={target}");
    }

    // ── find_matches_regex ─────────────────────────────────────────────────────

    fn make_frag(text: &str, x: f32, y: f32, w: f32, h: f32) -> TextFragment {
        use lumen_dom::NodeId;
        TextFragment {
            text: text.to_string(),
            rect: Rect::new(x, y, w, h),
            node: NodeId::from_index(1),
            char_offset: 0,
        }
    }

    #[test]
    fn regex_empty_pattern_returns_empty() {
        let frag = make_frag("hello world", 0.0, 0.0, 88.0, 20.0);
        let dl = vec![draw_text("hello world", 0.0, 0.0, 200.0, 20.0)];
        assert!(find_matches_regex(&[frag], &dl, "", &Fixed8).is_empty());
    }

    #[test]
    fn regex_invalid_pattern_returns_empty() {
        let frag = make_frag("hello", 0.0, 0.0, 40.0, 20.0);
        let dl = vec![draw_text("hello", 0.0, 0.0, 100.0, 20.0)];
        // Незакрытая группа — невалидный regex.
        assert!(find_matches_regex(&[frag], &dl, "(unclosed", &Fixed8).is_empty());
    }

    #[test]
    fn regex_simple_literal_match() {
        let frag = make_frag("hello world", 0.0, 0.0, 88.0, 20.0);
        let dl = vec![draw_text("hello world", 0.0, 0.0, 200.0, 20.0)];
        let m = find_matches_regex(&[frag], &dl, "world", &Fixed8);
        assert_eq!(m.len(), 1);
        // prefix "hello " = 6 chars * 8 = 48
        assert!((m[0].rect.x - 48.0).abs() < 0.01, "x={}", m[0].rect.x);
        assert!((m[0].rect.width - 40.0).abs() < 0.01); // "world" = 5 * 8
    }

    #[test]
    fn regex_case_insensitive() {
        let frag = make_frag("Hello World", 0.0, 0.0, 88.0, 20.0);
        let dl = vec![draw_text("Hello World", 0.0, 0.0, 200.0, 20.0)];
        let m = find_matches_regex(&[frag], &dl, "WORLD", &Fixed8);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn regex_digit_pattern() {
        let frag = make_frag("abc123def", 0.0, 0.0, 72.0, 20.0);
        let dl = vec![draw_text("abc123def", 0.0, 0.0, 200.0, 20.0)];
        let m = find_matches_regex(&[frag], &dl, r"\d+", &Fixed8);
        assert_eq!(m.len(), 1);
        // prefix "abc" = 3 * 8 = 24
        assert!((m[0].rect.x - 24.0).abs() < 0.01, "x={}", m[0].rect.x);
        assert!((m[0].rect.width - 24.0).abs() < 0.01); // "123" = 3 * 8
    }

    #[test]
    fn regex_multiple_matches_in_fragment() {
        let frag = make_frag("a1b2c3", 0.0, 0.0, 48.0, 20.0);
        let dl = vec![draw_text("a1b2c3", 0.0, 0.0, 100.0, 20.0)];
        let m = find_matches_regex(&[frag], &dl, r"\d", &Fixed8);
        assert_eq!(m.len(), 3);
        assert!((m[0].rect.x - 8.0).abs() < 0.01);  // after "a"
        assert!((m[1].rect.x - 24.0).abs() < 0.01); // after "a1b"
        assert!((m[2].rect.x - 40.0).abs() < 0.01); // after "a1b2c"
    }

    #[test]
    fn regex_fragment_not_in_dl_skipped() {
        let frag = make_frag("ghost text", 999.0, 999.0, 80.0, 20.0);
        let dl = vec![draw_text("other text", 0.0, 0.0, 100.0, 20.0)];
        let m = find_matches_regex(&[frag], &dl, "ghost", &Fixed8);
        assert!(m.is_empty());
    }

    #[test]
    fn regex_multiple_fragments() {
        let f1 = make_frag("foo", 0.0, 0.0, 24.0, 20.0);
        let f2 = make_frag("bar", 0.0, 20.0, 24.0, 20.0);
        let dl = vec![
            draw_text("foo", 0.0, 0.0, 100.0, 20.0),
            draw_text("bar", 0.0, 20.0, 100.0, 20.0),
        ];
        let m = find_matches_regex(&[f1, f2], &dl, "oo|ar", &Fixed8);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].dl_index, 0);
        assert_eq!(m[1].dl_index, 1);
    }

    // ── is_valid_regex_pattern ─────────────────────────────────────────────────

    #[test]
    fn valid_patterns() {
        assert!(is_valid_regex_pattern(r"\d+"));
        assert!(is_valid_regex_pattern("hello"));
        assert!(is_valid_regex_pattern(r"[a-z]+\s\d{3}"));
    }

    #[test]
    fn invalid_patterns() {
        assert!(!is_valid_regex_pattern("(unclosed"));
        assert!(!is_valid_regex_pattern(r"[invalid"));
    }
}
