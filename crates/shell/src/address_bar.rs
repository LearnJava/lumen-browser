//! Адресная строка (Ctrl+L): состояние overlay-бара и его сборка в display list.
//!
//! Паттерн идентичен `find.rs`: stateless рендер — `build_bar_overlay` каждый
//! кадр, stateful ввод — `AddressBarState` мутируется из event-handler-а.
//!
//! Commit-семантика: Enter → `take_commit()` возвращает URL/запрос и сбрасывает
//! состояние. Caller обязан обработать навигацию или запрос.
//!
//! Omnibox-интеграция: при изменении ввода caller вызывает `set_suggestions()`,
//! передавая результаты из `HistoryFts` + `SearchHistory`. Стрелки Up/Down
//! перемещают выделение; Enter коммитит выделенную строку или raw input.
//!
//! `@history <query>` — FTS-поиск по истории; `@notes <query>` — поиск по
//! пользовательским заметкам (§12.2); без префикса — prefix-match по
//! search_history + FTS по умолчанию.

use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};
use lumen_core::geom::Rect;

// ── Визуальные константы ──────────────────────────────────────────────────────

const BAR_BG: Color = Color { r: 32, g: 33, b: 36, a: 240 };
const BAR_BORDER: Color = Color { r: 60, g: 120, b: 220, a: 255 };
const BAR_FG: Color = Color { r: 232, g: 232, b: 236, a: 255 };
const BAR_DIM: Color = Color { r: 140, g: 140, b: 148, a: 255 };
const INPUT_BG: Color = Color { r: 18, g: 18, b: 22, a: 255 };
const CURSOR: Color = Color { r: 100, g: 160, b: 255, a: 220 };

// Dropdown
const ITEM_BG: Color = Color { r: 26, g: 27, b: 30, a: 245 };
const ITEM_SEL: Color = Color { r: 40, g: 72, b: 152, a: 255 };
const ITEM_FG: Color = Color { r: 218, g: 218, b: 228, a: 255 };
const ITEM_DIM: Color = Color { r: 118, g: 118, b: 138, a: 255 };
const ITEM_TAG: Color = Color { r: 72, g: 150, b: 90, a: 255 };
const DROP_BORDER: Color = Color { r: 55, g: 55, b: 70, a: 255 };

const BAR_W: f32 = 560.0;
const BAR_H: f32 = 52.0;
const PAD: f32 = 10.0;
const FONT: f32 = 16.0;
/// Высота одной строки в dropdown.
const ITEM_H: f32 = 36.0;
const ITEM_LABEL_SZ: f32 = 13.0;
const ITEM_SUB_SZ: f32 = 11.0;
const ITEM_PAD: f32 = 8.0;
/// Максимум строк в dropdown.
const MAX_VISIBLE: usize = 7;
/// Максимальная длина строки ввода. Защита от случайной paste-атаки.
const MAX_INPUT_LEN: usize = 2048;

// ── Omnibox prefix ────────────────────────────────────────────────────────────

/// Префикс @-команды, распознанный в строке ввода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmniboxPrefix {
    /// `@history <query>` — поиск по FTS5-индексу истории посещённых страниц.
    History,
    /// `@notes <query>` — поиск по пользовательским заметкам (§12.2).
    Notes,
    /// Обычный ввод: URL или поисковый запрос.
    Plain,
}

/// Разбирает raw ввод → `(OmniboxPrefix, query_str)`.
///
/// `@history foo bar` → `(History, "foo bar")`.
/// `@notes foo bar` → `(Notes, "foo bar")`.
/// Всё остальное → `(Plain, trimmed_input)`.
pub fn parse_omnibox_prefix(input: &str) -> (OmniboxPrefix, &str) {
    let s = input.trim_start();
    if let Some(rest) = s.strip_prefix("@history") {
        (OmniboxPrefix::History, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@notes") {
        (OmniboxPrefix::Notes, rest.trim_start())
    } else {
        (OmniboxPrefix::Plain, s)
    }
}

// ── OmniboxSuggestion ─────────────────────────────────────────────────────────

/// Одна строка autocomplete в dropdown omnibox.
#[derive(Debug, Clone)]
pub enum OmniboxSuggestion {
    /// Результат FTS5-поиска по истории (`HistoryFts::search`).
    HistoryFts {
        /// URL посещённой страницы.
        url: String,
        /// Заголовок страницы (может быть пустым).
        title: String,
        /// Сниппет совпадения из текста страницы.
        snippet: String,
    },
    /// Результат FTS5-поиска по заметкам (§12.2, `@notes <query>`).
    ///
    /// При выборе пользователем `commit_value()` возвращает `viewer_url`
    /// (`note-viewer:<id>`), который перехватывается в `handle_omnibox_commit`
    /// для открытия `NoteViewerPanel`. Данные заметки (comment и проч.)
    /// запрашиваются напрямую из `notes_store` по id, поэтому хранить их
    /// здесь не нужно.
    Note {
        /// URL, к которому привязана заметка (отображается в dropdown label).
        url: String,
        /// Выделенный текст (selection) заметки.
        selection: String,
        /// BM25 сниппет вокруг совпадения.
        snippet: String,
        /// `note-viewer:<id>` — committed value, opens the note viewer.
        viewer_url: String,
    },
    /// Ранее введённый поисковый запрос (`SearchHistory::prefix_match`).
    SearchQuery {
        /// Исходная строка запроса (case-preserved).
        query: String,
        /// Частота использования — отображается как подсказка.
        frequency: i64,
    },
}

impl OmniboxSuggestion {
    /// Строка, которая будет зафиксирована при выборе этой подсказки.
    /// HistoryFts → URL навигации. Note → `note-viewer:<id>` (перехват в shell).
    /// SearchQuery → текст запроса.
    pub fn commit_value(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { url, .. } => url,
            OmniboxSuggestion::Note { viewer_url, .. } => viewer_url,
            OmniboxSuggestion::SearchQuery { query, .. } => query,
        }
    }

    /// Основной текст строки dropdown.
    pub fn label(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { title, url, .. } => {
                if title.is_empty() { url } else { title }
            }
            OmniboxSuggestion::Note { selection, .. } => selection,
            OmniboxSuggestion::SearchQuery { query, .. } => query,
        }
    }

    /// Дополнительный текст под основным label.
    /// HistoryFts: сниппет если непуст, иначе URL.
    /// Note: сниппет вокруг совпадения (или URL если сниппет пуст).
    /// SearchQuery: пустая строка (вся информация в label).
    pub fn sub_label(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Note { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::SearchQuery { .. } => "",
        }
    }

    /// Короткий тег-маркер типа для правой части строки.
    /// Для `SearchQuery` включает счётчик использований если > 1.
    fn tag(&self) -> String {
        match self {
            OmniboxSuggestion::HistoryFts { .. } => "история".to_string(),
            OmniboxSuggestion::Note { .. } => "заметка".to_string(),
            OmniboxSuggestion::SearchQuery { frequency, .. } if *frequency > 1 => {
                format!("×{frequency}")
            }
            OmniboxSuggestion::SearchQuery { .. } => "запрос".to_string(),
        }
    }

    fn tag_color(&self) -> Color {
        match self {
            OmniboxSuggestion::HistoryFts { .. } => BAR_BORDER,
            OmniboxSuggestion::Note { .. } => Color { r: 180, g: 120, b: 60, a: 255 },
            OmniboxSuggestion::SearchQuery { .. } => ITEM_TAG,
        }
    }
}

// ── Состояние ─────────────────────────────────────────────────────────────────

/// Состояние адресной строки. Хранится в `Lumen` struct наряду с `FindState`.
#[derive(Debug, Default, Clone)]
pub struct AddressBarState {
    open: bool,
    input: String,
    /// Если `Some`, caller должен навигироваться на это значение и вызвать
    /// `clear_commit()`. Устанавливается в `commit()`.
    pending_commit: Option<String>,
    /// Текущий список подсказок. Обновляется caller-ом через `set_suggestions()`
    /// после каждого изменения ввода.
    suggestions: Vec<OmniboxSuggestion>,
    /// Индекс выделенной подсказки в dropdown. `None` — курсор в поле ввода.
    selected_idx: Option<usize>,
}

impl AddressBarState {
    /// Открыть бар, предзаполнив поле текущим URL страницы.
    pub fn open(&mut self, current_url: &str) {
        self.open = true;
        self.input = current_url.to_owned();
        self.pending_commit = None;
        self.suggestions.clear();
        self.selected_idx = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.pending_commit = None;
        self.suggestions.clear();
        self.selected_idx = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    /// Текущий список подсказок (для рендера и клавиатурной навигации).
    pub fn suggestions(&self) -> &[OmniboxSuggestion] {
        &self.suggestions
    }

    /// Индекс выделенной подсказки. `None` — ни одна не выделена.
    pub fn selected_idx(&self) -> Option<usize> {
        self.selected_idx
    }

    /// Установить новый список подсказок и сбросить выделение.
    /// Вызывается caller-ом после каждого изменения ввода.
    pub fn set_suggestions(&mut self, suggestions: Vec<OmniboxSuggestion>) {
        self.suggestions = suggestions;
        self.selected_idx = None;
    }

    /// Перейти к следующей (вниз) подсказке.
    pub fn select_next(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        self.selected_idx = Some(match self.selected_idx {
            None => 0,
            Some(i) => (i + 1).min(self.suggestions.len() - 1),
        });
    }

    /// Перейти к предыдущей (вверх) подсказке. `None` если уже на первой.
    pub fn select_prev(&mut self) {
        self.selected_idx = match self.selected_idx {
            None | Some(0) => None,
            Some(i) => Some(i - 1),
        };
    }

    /// Добавить непечатаемые символы (printable chars из keyboard event).
    pub fn append_str(&mut self, s: &str) {
        if !self.open {
            return;
        }
        for c in s.chars() {
            if !c.is_control() && self.input.len() < MAX_INPUT_LEN {
                self.input.push(c);
            }
        }
        // Сбросить выделение при ручном вводе.
        self.selected_idx = None;
    }

    /// Backspace — удалить последний Unicode-символ.
    pub fn backspace(&mut self) {
        if self.open {
            self.input.pop();
            self.selected_idx = None;
        }
    }

    /// Зафиксировать текущий ввод или выделенную подсказку: закрыть бар и,
    /// если значение непусто, выставить pending_commit. Caller получает
    /// значение через `take_commit()`.
    pub fn commit(&mut self) {
        if !self.open {
            return;
        }
        let value = if let Some(idx) = self.selected_idx {
            self.suggestions.get(idx).map(|s| s.commit_value().to_owned())
        } else if !self.input.is_empty() {
            Some(self.input.clone())
        } else {
            None
        };
        self.close(); // сбрасывает input, open, suggestions, selected_idx, pending_commit
        self.pending_commit = value; // восстанавливаем после close
    }

    /// Вернуть зафиксированный URL/запрос (если есть) и сбросить его.
    /// Caller обязан обработать результат в этом же кадре.
    pub fn take_commit(&mut self) -> Option<String> {
        self.pending_commit.take()
    }
}

// ── Рендер ────────────────────────────────────────────────────────────────────

/// Параметры для сборки overlay display list.
pub struct BarOverlay {
    /// Размер окна в физических пикселях (для позиционирования по центру).
    pub window_size: (u32, u32),
}

/// Собирает display list адресной строки. Вызывается каждый кадр, пока
/// `state.is_open()`. Возвращаемый список рисуется поверх страницы без
/// scroll-смещения (viewport-locked).
pub fn build_bar_overlay(state: &AddressBarState, bar: BarOverlay) -> DisplayList {
    let (ww, _wh) = bar.window_size;
    let x = ((ww as f32 - BAR_W) * 0.5).max(PAD);
    let y = PAD;

    let sugg = state.suggestions();
    let n_visible = sugg.len().min(MAX_VISIBLE);
    let drop_h = n_visible as f32 * ITEM_H;

    let cap = 6 + n_visible * 4;
    let mut out = DisplayList::with_capacity(cap);

    // ─ Основной бар ──────────────────────────────────────────────────────────

    // Рамка (на 1px шире с каждой стороны) — синий accent.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x - 1.0, y - 1.0, BAR_W + 2.0, BAR_H + 2.0),
        color: BAR_BORDER,
    });
    // Фон бара.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, BAR_W, BAR_H),
        color: BAR_BG,
    });

    // Поле ввода.
    let input_x = x + PAD;
    let input_w = BAR_W - PAD * 2.0;
    let input_h = BAR_H - PAD * 2.0;
    let input_y = y + PAD;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(input_x, input_y, input_w, input_h),
        color: INPUT_BG,
    });

    // Отображаем строку выделенной подсказки в input field если она выбрана.
    let display_input = if let Some(idx) = state.selected_idx() {
        sugg.get(idx).map(|s| s.commit_value()).unwrap_or(state.input())
    } else {
        state.input()
    };
    let (display_text, text_color) = if display_input.is_empty() {
        ("Введите URL или поисковый запрос…", BAR_DIM)
    } else {
        (display_input, BAR_FG)
    };
    let text_margin = 6.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(
            input_x + text_margin,
            input_y + (input_h - FONT * 1.2) * 0.5,
            input_w - text_margin * 2.0 - 10.0,
            FONT * 1.2,
        ),
        text: display_text.to_string(),
        font_size: FONT,
        color: text_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Курсор — вертикальная линия. Не рисуется если выбрана подсказка.
    if !display_input.is_empty() && state.selected_idx().is_none() {
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(
                input_x + input_w - text_margin - 2.0,
                input_y + (input_h - FONT * 1.2) * 0.5,
                2.0,
                FONT * 1.2,
            ),
            color: CURSOR,
        });
    }

    // ─ Dropdown ───────────────────────────────────────────────────────────────

    if n_visible == 0 {
        return out;
    }

    let drop_y = y + BAR_H;
    let drop_x = x;

    // Граница dropdown.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(drop_x - 1.0, drop_y, BAR_W + 2.0, drop_h + 1.0),
        color: DROP_BORDER,
    });
    // Фон dropdown.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(drop_x, drop_y, BAR_W, drop_h),
        color: ITEM_BG,
    });

    for (i, s) in sugg.iter().take(n_visible).enumerate() {
        let iy = drop_y + i as f32 * ITEM_H;
        let selected = state.selected_idx() == Some(i);

        if selected {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(drop_x, iy, BAR_W, ITEM_H),
                color: ITEM_SEL,
            });
        }

        let label = s.label();
        let sub = s.sub_label();
        let tag = s.tag(); // String

        let has_sub = !sub.is_empty();
        if has_sub {
            // Двухстрочный layout: label сверху, sub снизу.
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    drop_x + ITEM_PAD,
                    iy + 4.0,
                    BAR_W - ITEM_PAD * 3.0 - 60.0,
                    ITEM_LABEL_SZ * 1.3,
                ),
                text: label.to_string(),
                font_size: ITEM_LABEL_SZ,
                color: ITEM_FG,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
                highlight_name: None,
            });
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    drop_x + ITEM_PAD,
                    iy + 4.0 + ITEM_LABEL_SZ * 1.3 + 1.0,
                    BAR_W - ITEM_PAD * 3.0 - 60.0,
                    ITEM_SUB_SZ * 1.3,
                ),
                text: sub.to_string(),
                font_size: ITEM_SUB_SZ,
                color: ITEM_DIM,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
                highlight_name: None,
            });
        } else {
            // Одна строка по центру высоты строки.
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    drop_x + ITEM_PAD,
                    iy + (ITEM_H - ITEM_LABEL_SZ * 1.3) * 0.5,
                    BAR_W - ITEM_PAD * 3.0 - 60.0,
                    ITEM_LABEL_SZ * 1.3,
                ),
                text: label.to_string(),
                font_size: ITEM_LABEL_SZ,
                color: ITEM_FG,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
                highlight_name: None,
            });
        }

        // Тег справа.
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                drop_x + BAR_W - 58.0,
                iy + (ITEM_H - ITEM_SUB_SZ * 1.3) * 0.5,
                54.0,
                ITEM_SUB_SZ * 1.3,
            ),
            text: tag,
            font_size: ITEM_SUB_SZ,
            color: s.tag_color(),
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
    }

    out
}

// ── Тесты ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_prefills_url() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        assert!(s.is_open());
        assert_eq!(s.input(), "https://example.com");
    }

    #[test]
    fn close_resets_state() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        s.close();
        assert!(!s.is_open());
        assert_eq!(s.input(), "");
        assert!(s.suggestions().is_empty());
        assert_eq!(s.selected_idx(), None);
    }

    #[test]
    fn append_adds_chars() {
        let mut s = AddressBarState::default();
        s.open("");
        s.append_str("https://");
        s.append_str("rust-lang.org");
        assert_eq!(s.input(), "https://rust-lang.org");
    }

    #[test]
    fn append_ignores_control_chars() {
        let mut s = AddressBarState::default();
        s.open("");
        s.append_str("abc\n\t\x08");
        assert_eq!(s.input(), "abc");
    }

    #[test]
    fn append_ignored_when_closed() {
        let mut s = AddressBarState::default();
        s.append_str("abc");
        assert_eq!(s.input(), "");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut s = AddressBarState::default();
        s.open("abc");
        s.backspace();
        assert_eq!(s.input(), "ab");
        s.backspace();
        s.backspace();
        s.backspace(); // no panic on empty
        assert_eq!(s.input(), "");
    }

    #[test]
    fn commit_takes_url_and_closes() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        s.append_str("/page");
        s.commit();
        assert!(!s.is_open());
        assert_eq!(s.take_commit(), Some("https://example.com/page".to_owned()));
        assert_eq!(s.take_commit(), None);
    }

    #[test]
    fn commit_empty_input_is_noop() {
        let mut s = AddressBarState::default();
        s.open("");
        s.commit();
        assert!(!s.is_open());
        assert_eq!(s.take_commit(), None);
    }

    #[test]
    fn max_len_enforced() {
        let mut s = AddressBarState::default();
        s.open("");
        let big = "a".repeat(MAX_INPUT_LEN + 100);
        s.append_str(&big);
        assert!(s.input().len() <= MAX_INPUT_LEN);
    }

    #[test]
    fn overlay_has_rect_and_text_when_open() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("https://example.com");
            x
        };
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        let has_text = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com"))
        });
        assert!(has_text);
    }

    #[test]
    fn overlay_shows_placeholder_when_empty() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("");
            x
        };
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        let has_placeholder = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("URL"))
        });
        assert!(has_placeholder);
    }

    #[test]
    fn overlay_has_border_rect_as_first_cmd() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("x");
            x
        };
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        assert!(matches!(dl[0], DisplayCommand::FillRect { color, .. } if color.b == BAR_BORDER.b));
    }

    // ── Omnibox prefix ────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_history() {
        let (prefix, q) = parse_omnibox_prefix("@history rust async");
        assert_eq!(prefix, OmniboxPrefix::History);
        assert_eq!(q, "rust async");
    }

    #[test]
    fn parse_prefix_plain() {
        let (prefix, q) = parse_omnibox_prefix("rust async");
        assert_eq!(prefix, OmniboxPrefix::Plain);
        assert_eq!(q, "rust async");
    }

    #[test]
    fn parse_prefix_history_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@history ");
        assert_eq!(prefix, OmniboxPrefix::History);
        assert_eq!(q, "");
    }

    #[test]
    fn parse_prefix_leading_space() {
        let (prefix, _q) = parse_omnibox_prefix("  @history foo");
        assert_eq!(prefix, OmniboxPrefix::History);
    }

    // ── Suggestion selection ──────────────────────────────────────────────────

    #[test]
    fn select_next_cycles() {
        let mut s = AddressBarState::default();
        s.open("r");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "rust".into(), frequency: 3 },
            OmniboxSuggestion::SearchQuery { query: "rayon".into(), frequency: 1 },
        ]);
        assert_eq!(s.selected_idx(), None);
        s.select_next();
        assert_eq!(s.selected_idx(), Some(0));
        s.select_next();
        assert_eq!(s.selected_idx(), Some(1));
        s.select_next(); // clamp at last
        assert_eq!(s.selected_idx(), Some(1));
    }

    #[test]
    fn select_prev_goes_to_none() {
        let mut s = AddressBarState::default();
        s.open("r");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "rust".into(), frequency: 3 },
        ]);
        s.select_next();
        assert_eq!(s.selected_idx(), Some(0));
        s.select_prev();
        assert_eq!(s.selected_idx(), None);
    }

    #[test]
    fn commit_uses_selected_suggestion() {
        let mut s = AddressBarState::default();
        s.open("ru");
        s.set_suggestions(vec![
            OmniboxSuggestion::HistoryFts {
                url: "https://rust-lang.org".into(),
                title: "Rust".into(),
                snippet: String::new(),
            },
        ]);
        s.select_next(); // selects index 0
        s.commit();
        assert_eq!(s.take_commit(), Some("https://rust-lang.org".to_owned()));
    }

    #[test]
    fn commit_falls_back_to_input_when_none_selected() {
        let mut s = AddressBarState::default();
        s.open("https://crates.io");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "crates".into(), frequency: 2 },
        ]);
        // Don't select any suggestion.
        s.commit();
        assert_eq!(s.take_commit(), Some("https://crates.io".to_owned()));
    }

    #[test]
    fn append_resets_selection() {
        let mut s = AddressBarState::default();
        s.open("r");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "rust".into(), frequency: 1 },
        ]);
        s.select_next();
        assert_eq!(s.selected_idx(), Some(0));
        s.append_str("u");
        assert_eq!(s.selected_idx(), None);
    }

    #[test]
    fn dropdown_rendered_for_suggestions() {
        let mut s = AddressBarState::default();
        s.open("rust");
        s.set_suggestions(vec![
            OmniboxSuggestion::HistoryFts {
                url: "https://rust-lang.org".into(),
                title: "Rust Programming Language".into(),
                snippet: "Systems programming".into(),
            },
            OmniboxSuggestion::SearchQuery { query: "rust async".into(), frequency: 5 },
        ]);
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        let text_count = dl.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        // Input text + label1 + sub1 + tag1 + label2 + tag2 >= 6
        assert!(text_count >= 6);
    }

    // ── @notes prefix ─────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_notes() {
        let (prefix, q) = parse_omnibox_prefix("@notes rust ownership");
        assert_eq!(prefix, OmniboxPrefix::Notes);
        assert_eq!(q, "rust ownership");
    }

    #[test]
    fn parse_prefix_notes_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@notes ");
        assert_eq!(prefix, OmniboxPrefix::Notes);
        assert_eq!(q, "");
    }

    #[test]
    fn parse_prefix_notes_no_match_for_plain() {
        let (prefix, _) = parse_omnibox_prefix("notes something");
        assert_eq!(prefix, OmniboxPrefix::Plain);
    }

    fn make_note_suggestion(note_id: i64) -> OmniboxSuggestion {
        OmniboxSuggestion::Note {
            url: "https://example.com/".into(),
            selection: "interesting text".into(),
            snippet: "interesting **text** here".into(),
            viewer_url: format!("note-viewer:{note_id}"),
        }
    }

    #[test]
    fn note_suggestion_commit_value_is_viewer_url() {
        let s = make_note_suggestion(7);
        assert_eq!(s.commit_value(), "note-viewer:7");
    }

    #[test]
    fn note_suggestion_label_is_selection() {
        let s = make_note_suggestion(1);
        assert_eq!(s.label(), "interesting text");
    }

    #[test]
    fn note_suggestion_sub_label_is_snippet() {
        let s = make_note_suggestion(2);
        assert_eq!(s.sub_label(), "interesting **text** here");
    }

    #[test]
    fn note_suggestion_sub_label_falls_back_to_url_when_snippet_empty() {
        let s = OmniboxSuggestion::Note {
            url: "https://example.com/".into(),
            selection: "sel".into(),
            snippet: String::new(),
            viewer_url: "note-viewer:3".into(),
        };
        assert_eq!(s.sub_label(), "https://example.com/");
    }
}
