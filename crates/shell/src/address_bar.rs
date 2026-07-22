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
//! пользовательским заметкам (§12.2); `@read-later <query>` — поиск по списку
//! «прочитать позже» (§12.3); `@tabs <query>` — поиск по открытым вкладкам
//! (§12.4); `@bookmarks <query>` — поиск по закладкам, с cosine-similarity
//! ранжированием при наличии AI-эмбеддинга (§12.8); `@ai <query>` — RAG-ответ
//! через `lumen-ai` `RagEngine` (§12.5), либо hint-строка если модуль не
//! собран (`--features ai`); без префикса — prefix-match по search_history +
//! FTS по умолчанию.

use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};
use lumen_core::geom::Rect;

use crate::panels::themes::Palette;

// ── Визуальные константы ──────────────────────────────────────────────────────
//
// Surface/text colours are theme-driven via [`Palette`] (passed into
// `build_bar_overlay`). The focus ring and caret now follow `pal.accent`
// (design-system prototype: `.omnibox:focus-within{ border-color:var(--accent) }`)
// instead of a fixed blue, so both track the active profile's accent colour.
// The result-tag legend below stays theme-invariant by design: it is a fixed
// per-category colour code (history/notes/tabs/…), same rationale as the
// lifecycle/container colours excluded from `Palette` in `tabs/strip.rs`.

/// Tag accent for FTS-history omnibox results — historically shared the same
/// blue as the focus ring; kept as its own constant now that the ring reads
/// `pal.accent` instead.
const HISTORY_TAG: Color = Color { r: 60, g: 120, b: 220, a: 255 };
/// Green tag accent for search-query omnibox results.
const ITEM_TAG: Color = Color { r: 72, g: 150, b: 90, a: 255 };

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
    /// `@read-later <query>` — поиск по сохранённым «прочитать позже» (§12.3).
    ///
    /// Поиск во время ввода; commit без выделения подсказки сохраняет ввод
    /// как URL (см. `omnibox::resolve` → `SaveReadLater`).
    ReadLater,
    /// `@tabs <query>` — поиск по открытым вкладкам (§12.4, заголовок + URL).
    Tabs,
    /// `@bookmarks <query>` — поиск по закладкам (§12.8): подстрочное
    /// совпадение title/url/тегов, при наличии AI-эмбеддинга результат
    /// дополнительно ранжируется по cosine-similarity к запросу.
    Bookmarks,
    /// `@ai <query>` — RAG-ответ через `lumen-ai` `RagEngine` (§12.5), либо
    /// hint-строка «AI module not enabled» если крейт не собран (`--features ai`).
    Ai,
    /// Обычный ввод: URL или поисковый запрос.
    Plain,
}

/// Разбирает raw ввод → `(OmniboxPrefix, query_str)`.
///
/// `@history foo bar` → `(History, "foo bar")`.
/// `@notes foo bar` → `(Notes, "foo bar")`.
/// `@read-later foo` → `(ReadLater, "foo")`.
/// `@tabs foo` → `(Tabs, "foo")`.
/// Всё остальное → `(Plain, trimmed_input)`.
pub fn parse_omnibox_prefix(input: &str) -> (OmniboxPrefix, &str) {
    let s = input.trim_start();
    if let Some(rest) = s.strip_prefix("@history") {
        (OmniboxPrefix::History, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@notes") {
        (OmniboxPrefix::Notes, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@read-later") {
        (OmniboxPrefix::ReadLater, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@tabs") {
        (OmniboxPrefix::Tabs, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@bookmarks") {
        (OmniboxPrefix::Bookmarks, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@ai") {
        (OmniboxPrefix::Ai, rest.trim_start())
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
    /// Результат FTS5-поиска по списку «прочитать позже» (§12.3, `@read-later`).
    ///
    /// При выборе `commit_value()` возвращает `url` → обычная навигация на
    /// сохранённую страницу.
    ReadLater {
        /// URL сохранённой страницы — committed value (навигация).
        url: String,
        /// Заголовок страницы (может быть пустым → показываем URL).
        title: String,
        /// BM25 сниппет вокруг совпадения.
        snippet: String,
    },
    /// Открытая вкладка, совпавшая с `@tabs <query>` (§12.4).
    ///
    /// При выборе `commit_value()` возвращает `switch_value`
    /// (`switch-tab:<id>`), перехватываемый в `handle_omnibox_commit` для
    /// переключения на вкладку по её стабильному id.
    Tab {
        /// Заголовок вкладки (может быть пустым → показываем URL).
        title: String,
        /// URL открытой во вкладке страницы (для sub_label).
        url: String,
        /// `switch-tab:<id>` — committed value, переключает на вкладку.
        switch_value: String,
    },
    /// Результат поиска по закладкам (§12.8, `@bookmarks <query>`).
    ///
    /// При выборе `commit_value()` возвращает `url` → обычная навигация.
    Bookmark {
        /// Заголовок закладки (может быть пустым → показываем URL).
        title: String,
        /// URL закладки.
        url: String,
        /// AI-саммари страницы, если вычислено (см. `Bookmarks::set_semantic`),
        /// иначе пустая строка — `sub_label()` подставит URL.
        snippet: String,
    },
    /// Единственная строка ответа на `@ai <query>` (§12.5).
    ///
    /// При выборе `commit_value()` возвращает sentinel `"ai-answer:noop"`
    /// (перехватывается в `handle_omnibox_commit` — навигация не нужна, весь
    /// ответ уже показан в самой строке dropdown).
    Ai {
        /// RAG-ответ (`RagEngine::answer`), fallback-текст `NullAiBackend`
        /// если Ollama недоступен (ADR-019), либо hint «AI module not
        /// enabled» под `#[cfg(not(feature = "ai"))]`.
        answer: String,
    },
}

impl OmniboxSuggestion {
    /// Строка, которая будет зафиксирована при выборе этой подсказки.
    /// HistoryFts → URL навигации. Note → `note-viewer:<id>` (перехват в shell).
    /// SearchQuery → текст запроса. ReadLater → URL навигации.
    /// Tab → `switch-tab:<id>` (перехват в shell).
    pub fn commit_value(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { url, .. } => url,
            OmniboxSuggestion::Note { viewer_url, .. } => viewer_url,
            OmniboxSuggestion::SearchQuery { query, .. } => query,
            OmniboxSuggestion::ReadLater { url, .. } => url,
            OmniboxSuggestion::Tab { switch_value, .. } => switch_value,
            OmniboxSuggestion::Bookmark { url, .. } => url,
            OmniboxSuggestion::Ai { .. } => "ai-answer:noop",
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
            OmniboxSuggestion::ReadLater { title, url, .. }
            | OmniboxSuggestion::Tab { title, url, .. }
            | OmniboxSuggestion::Bookmark { title, url, .. } => {
                if title.is_empty() { url } else { title }
            }
            OmniboxSuggestion::Ai { answer } => answer,
        }
    }

    /// Дополнительный текст под основным label.
    /// HistoryFts: сниппет если непуст, иначе URL.
    /// Note: сниппет вокруг совпадения (или URL если сниппет пуст).
    /// SearchQuery: пустая строка (вся информация в label).
    /// ReadLater: сниппет если непуст, иначе URL.
    /// Tab: URL открытой страницы.
    /// Bookmark: AI-саммари если вычислено, иначе URL.
    /// Ai: пустая строка (весь ответ уже в label).
    pub fn sub_label(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Note { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::SearchQuery { .. } => "",
            OmniboxSuggestion::ReadLater { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Tab { url, .. } => url,
            OmniboxSuggestion::Bookmark { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Ai { .. } => "",
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
            OmniboxSuggestion::ReadLater { .. } => "позже".to_string(),
            OmniboxSuggestion::Tab { .. } => "вкладка".to_string(),
            OmniboxSuggestion::Bookmark { .. } => "закладка".to_string(),
            OmniboxSuggestion::Ai { .. } => "ai".to_string(),
        }
    }

    fn tag_color(&self) -> Color {
        match self {
            OmniboxSuggestion::HistoryFts { .. } => HISTORY_TAG,
            OmniboxSuggestion::Note { .. } => Color { r: 180, g: 120, b: 60, a: 255 },
            OmniboxSuggestion::SearchQuery { .. } => ITEM_TAG,
            OmniboxSuggestion::ReadLater { .. } => Color { r: 120, g: 90, b: 180, a: 255 },
            OmniboxSuggestion::Tab { .. } => Color { r: 60, g: 150, b: 170, a: 255 },
            OmniboxSuggestion::Bookmark { .. } => Color { r: 200, g: 160, b: 40, a: 255 },
            OmniboxSuggestion::Ai { .. } => Color { r: 150, g: 70, b: 200, a: 255 },
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
pub fn build_bar_overlay(state: &AddressBarState, bar: BarOverlay, pal: &Palette) -> DisplayList {
    let (ww, _wh) = bar.window_size;
    let x = ((ww as f32 - BAR_W) * 0.5).max(PAD);
    let y = PAD;

    let sugg = state.suggestions();
    let n_visible = sugg.len().min(MAX_VISIBLE);
    let drop_h = n_visible as f32 * ITEM_H;

    let cap = 6 + n_visible * 4;
    let mut out = DisplayList::with_capacity(cap);

    // ─ Основной бар ──────────────────────────────────────────────────────────

    // Рамка (на 1px шире с каждой стороны) — accent профиля/темы (design-system:
    // `.omnibox:focus-within{ border-color:var(--accent) }`).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x - 1.0, y - 1.0, BAR_W + 2.0, BAR_H + 2.0),
        color: pal.accent,
    });
    // Фон бара.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, BAR_W, BAR_H),
        color: pal.overlay_bg,
    });

    // Поле ввода.
    let input_x = x + PAD;
    let input_w = BAR_W - PAD * 2.0;
    let input_h = BAR_H - PAD * 2.0;
    let input_y = y + PAD;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(input_x, input_y, input_w, input_h),
        color: pal.input_bg,
    });

    // Отображаем строку выделенной подсказки в input field если она выбрана.
    let display_input = if let Some(idx) = state.selected_idx() {
        sugg.get(idx).map(|s| s.commit_value()).unwrap_or(state.input())
    } else {
        state.input()
    };
    let (display_text, text_color) = if display_input.is_empty() {
        ("Введите URL или поисковый запрос…", pal.text_dim)
    } else {
        (display_input, pal.text)
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
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
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
            color: Color { a: 220, ..pal.accent },
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
        color: pal.overlay_border,
    });
    // Фон dropdown.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(drop_x, drop_y, BAR_W, drop_h),
        color: pal.item_bg,
    });

    for (i, s) in sugg.iter().take(n_visible).enumerate() {
        let iy = drop_y + i as f32 * ITEM_H;
        let selected = state.selected_idx() == Some(i);

        if selected {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(drop_x, iy, BAR_W, ITEM_H),
                color: pal.item_selected_bg,
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
                color: pal.text,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
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
                color: pal.text_dim,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
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
                color: pal.text,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
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
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
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
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) }, &Palette::DARK);
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
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) }, &Palette::DARK);
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
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) }, &Palette::DARK);
        assert!(matches!(dl[0], DisplayCommand::FillRect { color, .. } if color == Palette::DARK.accent));
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
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) }, &Palette::DARK);
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

    // ── @read-later prefix ──────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_read_later() {
        let (prefix, q) = parse_omnibox_prefix("@read-later rust book");
        assert_eq!(prefix, OmniboxPrefix::ReadLater);
        assert_eq!(q, "rust book");
    }

    #[test]
    fn parse_prefix_read_later_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@read-later ");
        assert_eq!(prefix, OmniboxPrefix::ReadLater);
        assert_eq!(q, "");
    }

    #[test]
    fn read_later_suggestion_commit_value_is_url() {
        let s = OmniboxSuggestion::ReadLater {
            url: "https://example.com/article".into(),
            title: "Article".into(),
            snippet: "an **article** snippet".into(),
        };
        assert_eq!(s.commit_value(), "https://example.com/article");
        assert_eq!(s.label(), "Article");
        assert_eq!(s.sub_label(), "an **article** snippet");
    }

    #[test]
    fn read_later_suggestion_label_falls_back_to_url() {
        let s = OmniboxSuggestion::ReadLater {
            url: "https://example.com/x".into(),
            title: String::new(),
            snippet: String::new(),
        };
        assert_eq!(s.label(), "https://example.com/x");
        assert_eq!(s.sub_label(), "https://example.com/x");
    }

    // ── @tabs prefix ────────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_tabs() {
        let (prefix, q) = parse_omnibox_prefix("@tabs github");
        assert_eq!(prefix, OmniboxPrefix::Tabs);
        assert_eq!(q, "github");
    }

    #[test]
    fn parse_prefix_tabs_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@tabs");
        assert_eq!(prefix, OmniboxPrefix::Tabs);
        assert_eq!(q, "");
    }

    #[test]
    fn tab_suggestion_commit_value_is_switch_sentinel() {
        let s = OmniboxSuggestion::Tab {
            title: "GitHub".into(),
            url: "https://github.com/".into(),
            switch_value: "switch-tab:42".into(),
        };
        assert_eq!(s.commit_value(), "switch-tab:42");
        assert_eq!(s.label(), "GitHub");
        assert_eq!(s.sub_label(), "https://github.com/");
    }

    // ── @bookmarks prefix ────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_bookmarks() {
        let (prefix, q) = parse_omnibox_prefix("@bookmarks rust");
        assert_eq!(prefix, OmniboxPrefix::Bookmarks);
        assert_eq!(q, "rust");
    }

    #[test]
    fn parse_prefix_bookmarks_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@bookmarks");
        assert_eq!(prefix, OmniboxPrefix::Bookmarks);
        assert_eq!(q, "");
    }

    #[test]
    fn bookmark_suggestion_commit_value_is_url() {
        let s = OmniboxSuggestion::Bookmark {
            title: "Rust".into(),
            url: "https://rust-lang.org/".into(),
            snippet: "a systems programming language".into(),
        };
        assert_eq!(s.commit_value(), "https://rust-lang.org/");
        assert_eq!(s.label(), "Rust");
        assert_eq!(s.sub_label(), "a systems programming language");
    }

    #[test]
    fn bookmark_suggestion_label_and_sub_label_fall_back_to_url() {
        let s = OmniboxSuggestion::Bookmark {
            title: String::new(),
            url: "https://example.com/x".into(),
            snippet: String::new(),
        };
        assert_eq!(s.label(), "https://example.com/x");
        assert_eq!(s.sub_label(), "https://example.com/x");
    }

    // ── @ai prefix ───────────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_ai() {
        let (prefix, q) = parse_omnibox_prefix("@ai what did I read about rust?");
        assert_eq!(prefix, OmniboxPrefix::Ai);
        assert_eq!(q, "what did I read about rust?");
    }

    #[test]
    fn parse_prefix_ai_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@ai");
        assert_eq!(prefix, OmniboxPrefix::Ai);
        assert_eq!(q, "");
    }

    #[test]
    fn ai_suggestion_commit_value_is_noop_sentinel() {
        let s = OmniboxSuggestion::Ai { answer: "Rust is a systems language.".into() };
        assert_eq!(s.commit_value(), "ai-answer:noop");
        assert_eq!(s.label(), "Rust is a systems language.");
        assert_eq!(s.sub_label(), "");
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
